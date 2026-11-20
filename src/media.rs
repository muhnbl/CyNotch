use crate::models::{LyricLine, MediaTrack};

#[cfg(windows)]
pub struct MediaPoller {
    manager: Option<windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager>,
    last_source_id: String,
    last_title: String,
    last_artist: String,
    last_album: String,
    last_end_ms: u64,
    last_raw_pos_ms: u64,
    props_refresh_tick: u32,
}

#[cfg(windows)]
impl Default for MediaPoller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl MediaPoller {
    pub fn new() -> Self {
        Self {
            manager: None,
            last_source_id: String::new(),
            last_title: String::new(),
            last_artist: String::new(),
            last_album: String::new(),
            last_end_ms: 0,
            last_raw_pos_ms: 0,
            props_refresh_tick: 0,
        }
    }

    fn clear_cache(&mut self) {
        self.last_source_id.clear();
        self.last_title.clear();
        self.last_artist.clear();
        self.last_album.clear();
        self.last_end_ms = 0;
        self.last_raw_pos_ms = 0;
        self.props_refresh_tick = 0;
    }

    pub fn poll(&mut self) -> Option<MediaTrack> {
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
            use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

            if self.manager.is_none() {
                self.manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                    .ok()?
                    .get()
                    .ok();
            }

            let manager = self.manager.as_ref()?;
            let session = match manager.GetCurrentSession() {
                Ok(s) => s,
                Err(e) => {
                    // S_OK (empty Error) means no current media session; negative HRESULT means dead COM proxy
                    if e.code().is_err() {
                        self.manager = None;
                    }
                    self.clear_cache();
                    return None;
                }
            };

            let playback_info = session.GetPlaybackInfo().ok()?;
            let status = playback_info.PlaybackStatus().ok()?;
            let is_playing = status.0 == 4; // PlaybackStatus::Playing = 4

            let timeline = session.GetTimelineProperties().ok();
            let raw_pos_ms = timeline
                .as_ref()
                .and_then(|t| t.Position().ok())
                .map(|d| (d.Duration / 10_000).max(0) as u64)
                .unwrap_or(0);

            let duration_ms = timeline
                .as_ref()
                .and_then(|t| t.EndTime().ok())
                .map(|d| (d.Duration / 10_000).max(0) as u64)
                .unwrap_or(0);

            let source_id = session
                .SourceAppUserModelId()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();

            self.props_refresh_tick = self.props_refresh_tick.wrapping_add(1);
            let need_props = (self.last_title.is_empty() && self.last_artist.is_empty())
                || source_id != self.last_source_id
                || duration_ms != self.last_end_ms
                || raw_pos_ms.saturating_add(1500) < self.last_raw_pos_ms
                || self.props_refresh_tick >= 6;

            if need_props {
                let media_props = match session.TryGetMediaPropertiesAsync().ok().and_then(|op| op.get().ok()) {
                    Some(p) => p,
                    None => {
                        self.clear_cache();
                        return None;
                    }
                };
                self.last_source_id = source_id;
                self.last_title = crate::notifications::sanitize_notification_text(
                    &media_props.Title().unwrap_or_default().to_string(),
                );
                self.last_artist = crate::notifications::sanitize_notification_text(
                    &media_props.Artist().unwrap_or_default().to_string(),
                );
                self.last_album = crate::notifications::sanitize_notification_text(
                    &media_props.AlbumTitle().unwrap_or_default().to_string(),
                );
                self.props_refresh_tick = 0;
            }
            self.last_end_ms = duration_ms;
            self.last_raw_pos_ms = raw_pos_ms;

            if self.last_title.is_empty() && self.last_artist.is_empty() {
                return None;
            }

            let extra_ms = if is_playing {
                timeline
                    .as_ref()
                    .and_then(|t| t.LastUpdatedTime().ok())
                    .map(|dt| {
                        let ft = unsafe { GetSystemTimeAsFileTime() };
                        let now_100ns =
                            ((ft.dwHighDateTime as i64) << 32) | (ft.dwLowDateTime as u32 as i64);
                        compute_playback_extra_ms(now_100ns, dt.UniversalTime)
                    })
                    .unwrap_or(0)
            } else {
                0
            };

            let mut position_ms = raw_pos_ms.saturating_add(extra_ms);
            if duration_ms > 0 {
                position_ms = position_ms.min(duration_ms);
            }

            Some(MediaTrack {
                title: self.last_title.clone(),
                artist: self.last_artist.clone(),
                album: self.last_album.clone(),
                is_playing,
                position_ms,
                duration_ms,
            })
        }));
        res.ok().flatten()
    }
}

/// Safely computes elapsed playback milliseconds since WinRT `LastUpdatedTime`
/// without overflowing on sentinel or out-of-range timestamps (BH-27).
fn compute_playback_extra_ms(now_100ns: i64, updated_100ns: i64) -> u64 {
    let Some(diff) = now_100ns.checked_sub(updated_100ns) else {
        return 0;
    };
    let delta_ms = diff / 10_000;
    if (0..=3_600_000).contains(&delta_ms) {
        delta_ms as u64
    } else {
        0
    }
}

#[cfg(windows)]
pub fn poll_system_media() -> Option<MediaTrack> {
    let mut p = MediaPoller::new();
    p.poll()
}

#[cfg(not(windows))]
pub fn poll_system_media() -> Option<MediaTrack> {
    None
}

#[cfg(windows)]
struct SeekComGuard(bool);

#[cfg(windows)]
impl Drop for SeekComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

struct CoalescingSlot<T> {
    pending: std::sync::Mutex<Option<T>>,
    cvar: std::sync::Condvar,
}

impl<T> CoalescingSlot<T> {
    fn new() -> Self {
        Self {
            pending: std::sync::Mutex::new(None),
            cvar: std::sync::Condvar::new(),
        }
    }

    fn post(&self, value: T) {
        let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(value);
        self.cvar.notify_one();
    }

    fn take_blocking(&self) -> T {
        let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(v) = guard.take() {
                return v;
            }
            guard = self.cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(windows)]
pub fn seek_to_position_ms(pos_ms: u64) {
    use std::sync::{Arc, OnceLock};
    static SEEK_WORKER: OnceLock<Arc<CoalescingSlot<u64>>> = OnceLock::new();

    let slot = SEEK_WORKER.get_or_init(|| {
        let s = Arc::new(CoalescingSlot::new());
        let bg = Arc::clone(&s);
        let _ = std::thread::Builder::new()
            .name("cynotch-media-seek".to_string())
            .spawn(move || {
                use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
                use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
                let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let _com = SeekComGuard(hr.is_ok());
                let mut manager: Option<GlobalSystemMediaTransportControlsSessionManager> = None;

                loop {
                    let target_ms = bg.take_blocking();
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if manager.is_none()
                            && let Ok(op) =
                                GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                        {
                            manager = op.get().ok();
                        }
                        if let Some(ref mgr) = manager {
                            match mgr.GetCurrentSession() {
                                Ok(session) => {
                                    let ticks_100ns =
                                        i64::try_from(target_ms)
                                            .unwrap_or(i64::MAX)
                                            .saturating_mul(10_000);
                                    if let Ok(seek_op) =
                                        session.TryChangePlaybackPositionAsync(ticks_100ns)
                                    {
                                        if seek_op.get().is_err() {
                                            manager = None;
                                        }
                                    } else {
                                        manager = None;
                                    }
                                }
                                Err(_) => {
                                    manager = None;
                                }
                            }
                        }
                    }));
                }
            });
        s
    });

    slot.post(pos_ms);
}

#[cfg(not(windows))]
pub fn seek_to_position_ms(_pos_ms: u64) {}

pub fn default_demo_lyrics() -> Vec<LyricLine> {
    vec![
        LyricLine {
            seconds: 15.0,
            text: "Desert twilight fading on the highway line".to_string(),
        },
        LyricLine {
            seconds: 40.0,
            text: "Violet lightning humming underneath the sky".to_string(),
        },
        LyricLine {
            seconds: 60.0,
            text: "Nowhere else we need to go".to_string(),
        },
        LyricLine {
            seconds: 75.0,
            text: "Chasing golden horizons through the midnight glow".to_string(),
        },
        LyricLine {
            seconds: 98.0,
            text: "Steady rhythm keeps our focus sharp and cold".to_string(),
        },
    ]
}

// Media Control Key simulation (Spotify, YouTube, VLC, Apple Music, etc.)
pub fn toggle_play_pause() {
    send_media_key(0xB3); // VK_MEDIA_PLAY_PAUSE
}

pub fn next_track() {
    send_media_key(0xB0); // VK_MEDIA_NEXT_TRACK
}

pub fn prev_track() {
    send_media_key(0xB1); // VK_MEDIA_PREV_TRACK
}

fn send_media_key(vk: u8) {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, keybd_event};
        keybd_event(vk, 0, Default::default(), 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn bh21_coalescing_slot_preserves_monotonic_latest_value() {
        let slot = Arc::new(CoalescingSlot::<u64>::new());
        let bg = Arc::clone(&slot);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));
        let seen_bg = Arc::clone(&seen);

        let worker = std::thread::spawn(move || loop {
            let v = bg.take_blocking();
            if v == u64::MAX {
                break;
            }
            seen_bg.lock().unwrap().push(v);
            std::thread::sleep(std::time::Duration::from_millis(2));
        });

        for ms in 1..=200 {
            slot.post(ms);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        slot.post(u64::MAX);
        worker.join().unwrap();

        let vals = seen.lock().unwrap();
        assert!(!vals.is_empty());
        // Strictly ordered (no out-of-order seek regressions) and coalesced
        for w in vals.windows(2) {
            assert!(w[1] > w[0], "seeks must never regress out-of-order: {:?}", *vals);
        }
        assert_eq!(*vals.last().unwrap(), 200);
    }

    #[test]
    fn bh27_playback_extra_ms_handles_sentinel_and_overflow_timestamps() {
        assert_eq!(compute_playback_extra_ms(1_000_000, i64::MIN), 0);
        assert_eq!(compute_playback_extra_ms(i64::MIN, i64::MAX), 0);
        assert_eq!(compute_playback_extra_ms(10_000_000, 15_000_000), 0);
        assert_eq!(compute_playback_extra_ms(50_000_000, 20_000_000), 3_000);
    }
}
