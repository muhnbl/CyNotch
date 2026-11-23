#![allow(unsafe_op_in_unsafe_fn)]

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Mutex, OnceLock};
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
        for chunk in out[start..end].as_chunks_mut::<2>().0 {
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
    PlayWaterPlop { volume: f32 },
}

static WAV_WATER_PLOP: OnceLock<Vec<u8>> = OnceLock::new();

/// Synthesizes a smooth, organic 44.1 kHz 16-bit mono PCM WAV "water plop" sound
/// based on Minnaert bubble resonance (continuous-phase exponential pitch rise),
/// with a raised-cosine attack, long 80ms raised-cosine tail fadeout to exact zero,
/// and 45ms of trailing digital silence so `PlaySoundW` never clips or pops.
pub fn synthesize_water_plop_wav() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    const ACTIVE_SECS: f64 = 0.185;
    const PAD_SECS: f64 = 0.045;
    const ATTACK_SECS: f64 = 0.006;
    const FADEOUT_START: f64 = 0.105;
    const FADEOUT_LEN: f64 = ACTIVE_SECS - FADEOUT_START; // 80ms smooth cosine tail
    const F0: f64 = 260.0;
    const F1: f64 = 660.0;
    const SWEEP_SECS: f64 = 0.145;

    let active_samples = (SAMPLE_RATE as f64 * ACTIVE_SECS).round() as usize;
    let pad_samples = (SAMPLE_RATE as f64 * PAD_SECS).round() as usize;
    let total_samples = active_samples + pad_samples;
    let data_bytes = (total_samples * 2) as u32;
    let riff_size = 36 + data_bytes;

    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // ByteRate
    wav.extend_from_slice(&2u16.to_le_bytes()); // BlockAlign
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    let dt = 1.0 / (SAMPLE_RATE as f64);
    let mut phase = 0.0_f64;
    let mut body_phase = 0.0_f64;

    for i in 0..total_samples {
        if i >= active_samples {
            wav.extend_from_slice(&0i16.to_le_bytes());
            continue;
        }
        let t = (i as f64) * dt;
        // Exponential bubble chirp (Minnaert resonance rise)
        let u = (t / SWEEP_SECS).min(1.0);
        let freq = F0 * (F1 / F0).powf(u);
        phase += std::f64::consts::TAU * freq * dt;

        // Soft low-frequency impact body (145Hz -> 92Hz)
        let body_freq = 145.0 * (92.0 / 145.0_f64).powf((t / 0.040).min(1.0));
        body_phase += std::f64::consts::TAU * body_freq * dt;

        // Raised-cosine attack (0..6ms) for zero start click
        let attack = if t < ATTACK_SECS {
            0.5 * (1.0 - (std::f64::consts::PI * t / ATTACK_SECS).cos())
        } else {
            1.0
        };

        // Smooth exponential decay
        let decay = (-t / 0.052).exp();
        let harm2_decay = (-t / 0.032).exp();
        let body_decay = (-t / 0.022).exp();

        // Long 80ms raised-cosine tail fadeout to exact 0.0
        let tail = if t > FADEOUT_START {
            let ft = ((t - FADEOUT_START) / FADEOUT_LEN).clamp(0.0, 1.0);
            0.5 * (1.0 + (std::f64::consts::PI * ft).cos())
        } else {
            1.0
        };

        let main_sig = phase.sin() * decay
            + 0.16 * (2.0 * phase).sin() * harm2_decay
            + 0.22 * body_phase.sin() * body_decay;

        // Delicate peak (~0.24 raw -> ~-22 dBFS after playback volume scaling)
        let sample_f = main_sig * attack * tail * 0.24;
        let sample = (sample_f * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    wav
}

fn water_plop_wav() -> &'static [u8] {
    WAV_WATER_PLOP.get_or_init(synthesize_water_plop_wav).as_slice()
}

static AUDIO_TX: Mutex<Option<SyncSender<AudioMessage>>> = Mutex::new(None);

fn audio_sender() -> Option<SyncSender<AudioMessage>> {
    let mut slot = AUDIO_TX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tx) = slot.as_ref() {
        return Some(tx.clone());
    }
    // BH-28/BH-41: Bounded channel (8 slots) + retryable fallible thread spawn (no panic, no permanent brick).
    let (tx, rx) = sync_channel::<AudioMessage>(8);
    if std::thread::Builder::new()
        .name("cynotch-audio-worker".into())
        .spawn(move || {
            while let Ok(mut latest) = rx.recv() {
                while let Ok(next) = rx.try_recv() {
                    latest = next;
                }
                let (source, vol) = match latest {
                    AudioMessage::PlaySnapchat { volume } => (WAV_SNAPCHAT, volume),
                    AudioMessage::PlayWaterPlop { volume } => (water_plop_wav(), volume),
                };
                if vol <= 0.002 {
                    continue;
                }
                let wav_buf = scale_wav_volume(source, vol);
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
        })
        .is_ok()
    {
        *slot = Some(tx.clone());
        Some(tx)
    } else {
        None
    }
}

/// Plays the seamless procedural water-plop sound asynchronously when chimes are enabled.
pub fn play_water_plop(chime_enabled: bool, suppressed_by_fullscreen: bool) {
    if !chime_enabled || suppressed_by_fullscreen {
        return;
    }
    if let Some(tx) = audio_sender() {
        let _ = tx.try_send(AudioMessage::PlayWaterPlop { volume: 0.42 });
    }
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

    #[test]
    fn water_plop_wav_is_valid_and_seamless_with_zero_endpoints_and_silence_tail() {
        let wav = synthesize_water_plop_wav();
        let (start, end) = find_data_chunk(&wav).expect("valid RIFF/WAVE data chunk");
        assert!(end > start + 2000);
        let samples: Vec<i16> = wav[start..end]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        // Zero start sample (no click)
        assert_eq!(samples[0], 0);
        // Trailing silence padding (~45ms = ~1984 samples) must be exact 0
        let tail_pad = &samples[samples.len() - 1500..];
        assert!(tail_pad.iter().all(|&s| s == 0), "trailing silence must be zero");
        // Peak must be audible yet gentle (no clipping)
        let peak = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap_or(0);
        assert!(peak > 1500 && peak < 14000, "unexpected peak amplitude: {peak}");
    }
}
