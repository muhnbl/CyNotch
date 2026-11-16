use crate::models::{LyricLine, MediaTrack};

#[cfg(windows)]
pub struct MediaPoller {
    manager: Option<windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager>,
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
        Self { manager: None }
    }

    pub fn poll(&mut self) -> Option<MediaTrack> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                    return None;
                }
            };

            let media_props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
            let title = media_props.Title().unwrap_or_default().to_string();
            let artist = media_props.Artist().unwrap_or_default().to_string();
            let album = media_props.AlbumTitle().unwrap_or_default().to_string();

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

            let extra_ms = if is_playing {
                timeline
                    .as_ref()
                    .and_then(|t| t.LastUpdatedTime().ok())
                    .map(|dt| {
                        let ft = unsafe { GetSystemTimeAsFileTime() };
                        let now_100ns = ((ft.dwHighDateTime as i64) << 32) | (ft.dwLowDateTime as u32 as i64);
                        let delta_ms = (now_100ns - dt.UniversalTime) / 10_000;
                        if (0..=3_600_000).contains(&delta_ms) {
                            delta_ms as u64
                        } else {
                            0
                        }
                    })
                    .unwrap_or(0)
            } else {
                0
            };

            let mut position_ms = raw_pos_ms.saturating_add(extra_ms);
            if duration_ms > 0 {
                position_ms = position_ms.min(duration_ms);
            }

            if title.is_empty() && artist.is_empty() {
                return None;
            }

            Some(MediaTrack {
                title,
                artist,
                album,
                is_playing,
                position_ms,
                duration_ms,
            })
        }))
        .ok()
        .flatten()
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
pub fn seek_to_position_ms(pos_ms: u64) {
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            if let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                && let Ok(manager) = op.get()
                    && let Ok(session) = manager.GetCurrentSession() {
                        let ticks_100ns = (pos_ms as i64).saturating_mul(10_000);
                        if let Ok(seek_op) = session.TryChangePlaybackPositionAsync(ticks_100ns) {
                            let _ = seek_op.get();
                        }
                    }
        });
    });
}

#[cfg(not(windows))]
pub fn seek_to_position_ms(_pos_ms: u64) {}

pub fn default_demo_lyrics() -> Vec<LyricLine> {
    vec![
        LyricLine { seconds: 15.0, text: "Desert twilight fading on the highway line".to_string() },
        LyricLine { seconds: 40.0, text: "Violet lightning humming underneath the sky".to_string() },
        LyricLine { seconds: 60.0, text: "Nowhere else we need to go".to_string() },
        LyricLine { seconds: 75.0, text: "Chasing golden horizons through the midnight glow".to_string() },
        LyricLine { seconds: 98.0, text: "Steady rhythm keeps our focus sharp and cold".to_string() },
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
        use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
        keybd_event(vk, 0, Default::default(), 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}
