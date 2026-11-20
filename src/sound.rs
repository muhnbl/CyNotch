#![allow(unsafe_op_in_unsafe_fn)]

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::OnceLock;
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};

static WAV_SNAPCHAT: &[u8] = include_bytes!("../assets/sounds/snapchat.wav");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotificationSound {
    Mute,
    #[default]
    #[serde(alias = "IPhone", alias = "iphone")]
    Snapchat,
}

impl NotificationSound {
    pub const ALL: [Self; 2] = [Self::Mute, Self::Snapchat];

    /// Short label shown in Preferences selector pills.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Snapchat => "Snapchat",
        }
    }

    /// Descriptive name for tooltips and summaries.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Mute => "Muted",
            Self::Snapchat => "Snapchat Pop",
        }
    }

    /// Returns the embedded static RIFF/WAVE bytes.
    pub fn wav_bytes(self) -> Option<&'static [u8]> {
        match self {
            Self::Mute => None,
            Self::Snapchat => Some(WAV_SNAPCHAT),
        }
    }
}

/// Locates the `data` subchunk in a standard RIFF/WAVE container.
/// Honors RIFF 2-byte word alignment for odd-sized subchunks and uses checked arithmetic
/// so malformed chunk sizes cannot overflow or loop infinitely (BH-22).
pub fn find_data_chunk(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos: usize = 12;
    while pos.checked_add(8).is_some_and(|hdr_end| hdr_end <= bytes.len()) {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let data_start = pos + 8;
        if chunk_id == b"data" {
            let data_end = data_start.saturating_add(chunk_size).min(bytes.len());
            return Some((data_start, data_end));
        }
        let pad = chunk_size & 1;
        pos = data_start.checked_add(chunk_size)?.checked_add(pad)?;
    }
    None
}

/// Scales 16-bit linear PCM audio samples within a WAV buffer to adjust volume.
/// Volume `1.0` returns an unscaled buffer; `0.0` produces complete silence.
pub fn scale_wav_volume(source: &[u8], volume: f32) -> Vec<u8> {
    let vol = volume.clamp(0.0, 1.0);
    let mut out = source.to_vec();
    if (vol - 1.0).abs() < 0.005 {
        return out;
    }
    if let Some((start, end)) = find_data_chunk(source) {
        for chunk in out[start..end].chunks_exact_mut(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            let scaled = ((sample as f32) * vol).round().clamp(-32768.0, 32767.0) as i16;
            let bytes = scaled.to_le_bytes();
            chunk[0] = bytes[0];
            chunk[1] = bytes[1];
        }
    }
    out
}

/// Fixed notification volume at 25% (0..=100 scale).
pub const NOTIFICATION_FIXED_VOLUME: u32 = 25;

/// Cooldown window to coalesce rapid notification bursts into a single sound.
pub const NOTIFICATION_SOUND_COOLDOWN_MS: u64 = 1800;

const LAST_PLAY_UNSET: u64 = u64::MAX;
static LAST_PLAY_MS: AtomicU64 = AtomicU64::new(LAST_PLAY_UNSET);
static START_INSTANT: OnceLock<Instant> = OnceLock::new();

fn get_now_ms() -> u64 {
    let start = START_INSTANT.get_or_init(Instant::now);
    (start.elapsed().as_millis() as u64).min(LAST_PLAY_UNSET - 1)
}

/// Thread-safe atomic burst gate: returns `true` at most once per `cooldown_ms`
/// window so simultaneous notifications (A, B, C) trigger only one chime.
pub fn try_acquire_notif_sound_slot_at(now_ms: u64, cooldown_ms: u64) -> bool {
    let clamped_now = now_ms.min(LAST_PLAY_UNSET - 1);
    let mut last = LAST_PLAY_MS.load(Ordering::Acquire);
    loop {
        if last != LAST_PLAY_UNSET && clamped_now.saturating_sub(last) < cooldown_ms {
            return false;
        }
        match LAST_PLAY_MS.compare_exchange_weak(
            last,
            clamped_now,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(actual) => last = actual,
        }
    }
}

pub fn reset_notif_sound_cooldown() {
    LAST_PLAY_MS.store(LAST_PLAY_UNSET, Ordering::Release);
}

enum AudioMessage {
    PlaySnapchat { volume: f32 },
}

static AUDIO_TX: OnceLock<Option<SyncSender<AudioMessage>>> = OnceLock::new();

fn audio_sender() -> Option<&'static SyncSender<AudioMessage>> {
    AUDIO_TX
        .get_or_init(|| {
            // BH-28: Bounded channel (8 slots) + fallible thread spawn (no panic on OS pressure).
            let (tx, rx) = sync_channel::<AudioMessage>(8);
            match std::thread::Builder::new()
                .name("cynotch-audio-worker".into())
                .spawn(move || {
                    while let Ok(msg) = rx.recv() {
                        // Drain any queued duplicate bursts within the same slice to prevent audio stutter
                        let mut latest_vol = match msg {
                            AudioMessage::PlaySnapchat { volume } => volume,
                        };
                        while let Ok(next) = rx.try_recv() {
                            match next {
                                AudioMessage::PlaySnapchat { volume } => latest_vol = volume,
                            }
                        }
                        if latest_vol <= 0.002 {
                            continue;
                        }
                        let wav_buf = scale_wav_volume(WAV_SNAPCHAT, latest_vol);
                        // SAFETY: `wav_buf` remains valid on this dedicated worker thread for the entire
                        // duration of synchronous `SND_SYNC` playback, guaranteeing zero use-after-free.
                        unsafe {
                            let _ = PlaySoundW(
                                PCWSTR(wav_buf.as_ptr() as *const u16),
                                None,
                                SND_MEMORY | SND_SYNC | SND_NODEFAULT,
                            );
                        }
                    }
                }) {
                Ok(_) => Some(tx),
                Err(_) => None,
            }
        })
        .as_ref()
}

/// Plays a notification sound asynchronously at the fixed volume (25%).
/// This bypasses the burst cooldown (used for immediate auditioning in Settings).
pub fn play_sound(sound: NotificationSound) {
    if sound == NotificationSound::Mute {
        return;
    }
    let vol = (NOTIFICATION_FIXED_VOLUME as f32) / 100.0;
    if let Some(tx) = audio_sender() {
        let _ = tx.try_send(AudioMessage::PlaySnapchat { volume: vol });
    }
}

/// Plays a notification chime only when notifications are enabled, unmuted, and not suppressed by fullscreen.
/// Coalesces rapid bursts using `NOTIFICATION_SOUND_COOLDOWN_MS`.
pub fn play_notification_sound(
    sound: NotificationSound,
    notifications_enabled: bool,
    suppressed_by_fullscreen: bool,
) -> bool {
    if !notifications_enabled || suppressed_by_fullscreen || sound == NotificationSound::Mute {
        return false;
    }

    if !try_acquire_notif_sound_slot_at(get_now_ms(), NOTIFICATION_SOUND_COOLDOWN_MS) {
        return false;
    }

    play_sound(sound);
    true
}

#[cfg(test)]
mod riff_edge_tests {
    use super::*;

    #[test]
    fn bh22_find_data_chunk_handles_odd_subchunk_padding_and_overflow_size() {
        // Build minimal RIFF/WAVE with an odd-sized 3-byte INFO chunk + 1 pad byte + 4-byte data chunk
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&32u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"JUNK");
        wav.extend_from_slice(&3u32.to_le_bytes()); // odd size
        wav.extend_from_slice(&[1, 2, 3, 0]); // 3 data bytes + 1 RIFF pad byte
        let data_hdr = wav.len();
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&[0x10, 0x00, 0x20, 0x00]);

        assert_eq!(find_data_chunk(&wav), Some((data_hdr + 8, data_hdr + 12)));

        // Malformed chunk with u32::MAX size must return None without overflow or hang
        let mut bad = Vec::new();
        bad.extend_from_slice(b"RIFF");
        bad.extend_from_slice(&20u32.to_le_bytes());
        bad.extend_from_slice(b"WAVE");
        bad.extend_from_slice(b"JUNK");
        bad.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(find_data_chunk(&bad), None);
    }
}
