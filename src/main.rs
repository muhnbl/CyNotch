#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Timelike;
use cynotch::models::{
    self, AppState, ExpandedSizePreset, IdleMetric, LyricSize, LyricStyle, NotchMode,
    NotchPlacement,
};
use cynotch::screentime::ScreenTimeTracker;
use cynotch::telemetry::TelemetryCollector;
use cynotch::window::NotchWindow;
use cynotch::*;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::core::*;

macro_rules! log_info {
    ($($arg:tt)*) => {
        let _ = write!(std::io::stdout(), "{}\n", format_args!($($arg)*));
    };
}

macro_rules! log_err {
    ($($arg:tt)*) => {
        let _ = write!(std::io::stderr(), "{}\n", format_args!($($arg)*));
    };
}

fn save_notch_snapshot(dir: &str, name: &str, s: &AppState) {
    let (w, h) = (
        s.current_width.round().max(1.0) as u32,
        s.current_height.round().max(1.0) as u32,
    );
    let mut r = renderer::Renderer::new(w, h);
    r.render(s);
    let pad = 40;
    let mut bg = tiny_skia::Pixmap::new(w + pad * 2, h + pad * 2).unwrap();
    bg.fill(tiny_skia::Color::from_rgba8(236, 233, 248, 255));
    bg.draw_pixmap(
        pad as i32,
        pad as i32,
        r.pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    bg.save_png(format!("{dir}/{name}.png")).unwrap();
}

fn save_frosted_notch_snapshot(dir: &str, name: &str, s: &AppState) {
    let (w, h) = (
        s.current_width.round().max(1.0) as u32,
        s.current_height.round().max(1.0) as u32,
    );
    let mut r = renderer::Renderer::new(w, h);
    r.render(s);
    let pad = 40;
    let total_w = w + pad * 2;
    let total_h = h + pad * 2;
    let mut bg = tiny_skia::Pixmap::new(total_w, total_h).unwrap();
    let paint = tiny_skia::Paint {
        shader: tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(0.0, 0.0),
            tiny_skia::Point::from_xy(total_w as f32, total_h as f32),
            vec![
                tiny_skia::GradientStop::new(0.0, tiny_skia::Color::from_rgba8(124, 58, 237, 255)),
                tiny_skia::GradientStop::new(0.5, tiny_skia::Color::from_rgba8(14, 116, 144, 255)),
                tiny_skia::GradientStop::new(1.0, tiny_skia::Color::from_rgba8(217, 119, 6, 255)),
            ],
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        )
        .unwrap(),
        ..Default::default()
    };
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, total_w as f32, total_h as f32).unwrap();
    bg.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    bg.draw_pixmap(
        pad as i32,
        pad as i32,
        r.pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    bg.save_png(format!("{dir}/{name}.png")).unwrap();
}

fn snapshot(dir: &str) {
    use models::NotchMode;
    std::fs::create_dir_all(dir).ok();
    let mut s = AppState::new_for_preview();
    s.telemetry.memory_percent = 57.0;
    s.telemetry.memory_used_gb = 18.2;
    s.telemetry.memory_total_gb = 31.9;
    s.telemetry.pagefile_used_gb = 21.4;
    s.telemetry.pagefile_total_gb = 36.0;
    s.telemetry.pagefile_percent = 59.4;
    s.telemetry.sys_cache_mb = 842.0;
    s.telemetry.sys_cache_peak_mb = 1240.0;
    s.telemetry.sys_cache_percent = 67.9;
    s.telemetry.has_admin_privileges = true;
    s.telemetry.cpu_usage = 12.4;
    s.telemetry.net_down_kbps = 1480.0;
    s.telemetry.net_up_kbps = 264.0;
    s.capture.last_path =
        Some("C:\\Users\\muhna\\Pictures\\CyNotch\\cyshot_desktop_20261111_184000.png".to_string());
    s.capture.last_filename = "cyshot_desktop_20261111_184000.png".to_string();
    s.capture.last_dims = "2560 × 1440".to_string();
    s.capture.clipboard_ok = true;

    // Minimized micro-capsule idle preview (86×18, MascotMood::Idle)
    {
        s.minimized_island = true;
        s.mode = NotchMode::Home;
        s.is_pinned = false;
        s.notification = None;
        s.media.is_playing = false;
        s.mem_reduct.is_purging = false;
        s.anim_time = 1.25;
        s.set_hovered(false);
        s.sync_preview_settled();
        save_notch_snapshot(dir, "minimized_idle", &s);
        s.minimized_island = false;
        s.media.is_playing = true;
    }

    // Dedicated mascot animation mood showcases (Purging / Music / MemReduct-Purging)
    let mood_shots: [(&str, bool, NotchMode, bool, bool, f32); 3] = [
        ("purge_anim", false, NotchMode::Home, false, true, 0.35),
        ("music_idle", false, NotchMode::Home, true, false, 0.42),
        (
            "memreduct_purging",
            true,
            NotchMode::MemReduct,
            false,
            true,
            0.35,
        ),
    ];
    for (name, hovered, mode, playing, purging, atime) in mood_shots {
        s.mode = mode;
        s.show_lyrics = false;
        s.is_pinned = hovered;
        s.set_hovered(hovered);
        s.notification = None;
        s.media.is_playing = playing;
        s.mem_reduct.is_purging = purging;
        s.anim_time = atime;
        if purging {
            s.mem_reduct.last_cleaned_status = "Cleaning RAM...".to_string();
        }
        s.sync_preview_settled();
        save_notch_snapshot(dir, name, &s);
    }
    s.mem_reduct.is_purging = false;
    s.mem_reduct.last_cleaned_status = "Ready • Click to clean".to_string();
    s.anim_time = 0.0;

    let shots: [(&str, bool, NotchMode, bool, Option<&str>); 12] = [
        ("idle", false, NotchMode::Home, false, None),
        ("pasted", false, NotchMode::Home, false, Some("pasted")),
        ("home", true, NotchMode::Home, false, None),
        ("music", true, NotchMode::Music, false, None),
        ("lyrics", true, NotchMode::Music, true, None),
        ("volume", true, NotchMode::Volume, false, None),
        ("memreduct", true, NotchMode::MemReduct, false, None),
        ("stats", true, NotchMode::Telemetry, false, None),
        ("tasks", true, NotchMode::Tasks, false, None),
        ("shelf", true, NotchMode::Shelf, false, None),
        ("capture", true, NotchMode::Capture, false, None),
        ("settings", true, NotchMode::Settings, false, None),
    ];
    for (name, hovered, mode, show_lyrics, notif_opt) in shots {
        s.mode = mode;
        s.show_lyrics = show_lyrics;
        s.is_pinned = name == "home";
        s.media.is_playing = name != "idle";
        s.anim_time = if name == "music" { 0.42 } else { 0.25 };
        s.set_hovered(hovered);
        s.notification = None;
        if notif_opt == Some("pasted") {
            s.set_notification(
                "Pasted".to_string(),
                "14 words · Chrome".to_string(),
                models::NotificationKind::Pasted,
                true,
            );
        }
        s.sync_preview_settled();
        save_notch_snapshot(dir, name, &s);
    }

    // v1.8 showcases: IdleMetric::NetSpeed, Large expanded preset, Vertical Left & Right docks
    {
        s.notification = None;
        s.show_lyrics = false;
        s.mem_reduct.is_purging = false;

        // 1. Idle Net Speed badge
        s.placement = NotchPlacement::Top;
        s.minimized_island = false;
        s.expanded_size = ExpandedSizePreset::Standard;
        s.idle_metric = IdleMetric::NetSpeed;
        s.media.is_playing = false;
        s.is_pinned = false;
        s.mode = NotchMode::Home;
        s.anim_time = 0.25;
        s.set_hovered(false);
        s.sync_preview_settled();
        save_notch_snapshot(dir, "idle_netspeed", &s);

        // 2. Expanded Large hover size preset (1.15x)
        s.media.is_playing = true;
        s.is_pinned = true;
        s.idle_metric = IdleMetric::Ram;
        s.set_hovered(true);
        s.expanded_size = ExpandedSizePreset::Large;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "expanded_large", &s);

        // 4. Vertical Left minimized micro-pill (18×86)
        s.expanded_size = ExpandedSizePreset::Standard;
        s.placement = NotchPlacement::Left;
        s.minimized_island = true;
        s.media.is_playing = false;
        s.is_pinned = false;
        s.set_hovered(false);
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_left_minimized", &s);

        // 5. Vertical Left standard idle (52×204)
        s.minimized_island = false;
        s.idle_metric = IdleMetric::NetSpeed;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_left_idle", &s);

        // 6. Vertical Left expanded Home (316×480)
        s.idle_metric = IdleMetric::Ram;
        s.media.is_playing = true;
        s.is_pinned = true;
        s.mode = NotchMode::Home;
        s.set_hovered(true);
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_left_home", &s);

        // 7. Vertical Right expanded Music (316×480)
        s.placement = NotchPlacement::Right;
        s.mode = NotchMode::Music;
        s.anim_time = 0.42;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_right_music", &s);

        // 8. Vertical Right expanded MemReduct (316×480)
        s.mode = NotchMode::MemReduct;
        s.anim_time = 0.25;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_right_memreduct", &s);

        // v1.9 / v2.0 showcases: Lyrics size/style variants, Vertical Lyrics, System Toast, Frosted alpha backdrop
        s.placement = NotchPlacement::Top;
        s.mode = NotchMode::Music;
        s.show_lyrics = true;
        s.media.is_playing = true;
        s.is_pinned = true;
        s.set_hovered(true);
        if s.lyrics.len() >= 3 {
            s.lyrics[2].text =
                "Nowhere else we need to go when violet lightning lights the desert road"
                    .to_string();
        }

        s.lyric_style = LyricStyle::KaraokeGlow;
        s.lyric_size = LyricSize::Large;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "lyrics_karaoke_large", &s);

        s.lyric_style = LyricStyle::MinimalSingle;
        s.lyric_size = LyricSize::Compact;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "lyrics_minimal_compact", &s);

        s.lyric_style = LyricStyle::CompactStream;
        s.lyric_size = LyricSize::Standard;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "lyrics_stream_standard", &s);

        s.placement = NotchPlacement::Right;
        s.lyric_style = LyricStyle::KaraokeGlow;
        s.lyric_size = LyricSize::Standard;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "vertical_music_karaoke", &s);

        s.placement = NotchPlacement::Top;
        s.show_lyrics = false;
        s.lyric_size = LyricSize::Standard;
        s.lyric_style = LyricStyle::ClassicList;
        s.mode = NotchMode::Home;
        s.is_pinned = false;
        s.media.is_playing = false;
        s.set_hovered(false);
        s.set_system_toast(
            "Discord".to_string(),
            "Kai (#dev-sync)".to_string(),
            "PR #42 merged — ready for QA".to_string(),
        );
        s.sync_preview_settled();
        save_notch_snapshot(dir, "notif_system_toast", &s);

        s.type_scale = models::TypeScale::Compact;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "notif_compact", &s);

        s.type_scale = models::TypeScale::ExtraLarge;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "notif_xlarge", &s);
        s.type_scale = models::TypeScale::Standard;

        s.placement = NotchPlacement::Right;
        s.minimized_island = true;
        s.sync_preview_settled();
        save_notch_snapshot(dir, "notif_vertical_right_minimized", &s);
        s.minimized_island = false;
        s.placement = NotchPlacement::Top;

        s.notification = None;
        s.mode = NotchMode::Home;
        s.is_pinned = true;
        s.set_hovered(true);
        s.frosted_glass_enabled = true;
        s.windows_transparency_enabled = true;
        s.sync_preview_settled();
        save_frosted_notch_snapshot(dir, "frosted_alpha_preview", &s);
        s.windows_transparency_enabled = false;

        // Reset defaults before Settings window capture
        s.placement = NotchPlacement::Top;
        s.expanded_size = ExpandedSizePreset::Standard;
        s.idle_metric = IdleMetric::Ram;
        s.minimized_island = false;
        s.is_pinned = false;
        s.mode = NotchMode::Home;
        s.show_lyrics = false;
    }

    let cat_shots = [
        ("settings_general", models::SettingsCategory::General),
        ("settings_motion", models::SettingsCategory::Motion),
        ("settings_modules", models::SettingsCategory::Modules),
        (
            "settings_notifications",
            models::SettingsCategory::Notifications,
        ),
        ("settings_system", models::SettingsCategory::System),
    ];
    let (sw, sh, _) = settings_window::settings_dims_for_state(&s);
    let mut pm = tiny_skia::Pixmap::new(sw, sh).unwrap();
    let mut mask = tiny_skia::Mask::new(sw, sh).unwrap();
    let mut fonts = font::FontRenderer::new();
    s.media.is_playing = false;
    s.mem_reduct.is_purging = false;
    s.notification = None;
    s.anim_time = 0.25;
    for (name, cat) in cat_shots {
        s.settings_open = true;
        s.set_settings_category(cat);
        s.set_settings_scroll(cat, 0.0);
        s.sync_preview_settled();
        let (_hits, max_scroll) = settings_window::render_settings_pixmap_with_meta(
            &s, None, &mut pm, &mut mask, &mut fonts,
        );
        let pad = 40;
        let mut bg = tiny_skia::Pixmap::new(sw + pad * 2, sh + pad * 2).unwrap();
        bg.fill(tiny_skia::Color::from_rgba8(236, 233, 248, 255));
        bg.draw_pixmap(
            pad as i32,
            pad as i32,
            pm.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        bg.save_png(format!("{dir}/{name}.png")).unwrap();

        if max_scroll > 20.0 {
            s.set_settings_scroll(cat, max_scroll);
            settings_window::render_settings_pixmap(&s, None, &mut pm, &mut mask, &mut fonts);
            let mut bg_bot = tiny_skia::Pixmap::new(sw + pad * 2, sh + pad * 2).unwrap();
            bg_bot.fill(tiny_skia::Color::from_rgba8(236, 233, 248, 255));
            bg_bot.draw_pixmap(
                pad as i32,
                pad as i32,
                pm.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
            bg_bot.save_png(format!("{dir}/{name}_bottom.png")).unwrap();
        }
    }

    s.type_scale = models::TypeScale::ExtraLarge;
    s.set_settings_category(models::SettingsCategory::General);
    s.set_settings_scroll(models::SettingsCategory::General, 0.0);
    s.sync_preview_settled();
    let (xl_w, xl_h, _) = settings_window::settings_dims_for_state(&s);
    let mut xl_pm = tiny_skia::Pixmap::new(xl_w, xl_h).unwrap();
    let mut xl_mask = tiny_skia::Mask::new(xl_w, xl_h).unwrap();
    let _ = settings_window::render_settings_pixmap_with_meta(
        &s,
        None,
        &mut xl_pm,
        &mut xl_mask,
        &mut fonts,
    );
    let pad = 40;
    let mut xl_bg = tiny_skia::Pixmap::new(xl_w + pad * 2, xl_h + pad * 2).unwrap();
    xl_bg.fill(tiny_skia::Color::from_rgba8(236, 233, 248, 255));
    xl_bg.draw_pixmap(
        pad as i32,
        pad as i32,
        xl_pm.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    xl_bg
        .save_png(format!("{dir}/settings_general_xlarge.png"))
        .unwrap();
    log_info!("snapshots written to {dir}");
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let bt = std::backtrace::Backtrace::capture();
        let path = cynotch::models::crash_log_path("panic.txt");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(
                f,
                "--- {} ---\nPANIC: {}\n{}",
                chrono::Local::now().to_rfc3339(),
                info,
                bt
            );
        }
    }));

    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        snapshot(args.get(i + 1).map(String::as_str).unwrap_or("snapshots"));
        return;
    }
    unsafe {
        let mutex_name: PCWSTR = w!("CyNotch_SingleInstance_Mutex_v2");
        let h_mutex = CreateMutexW(None, true, mutex_name);
        if let Ok(m) = h_mutex {
            if GetLastError() == ERROR_ALREADY_EXISTS {
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
                let class_name: PCWSTR = w!("CyNotchWindowClass");
                let win_name: PCWSTR = w!("CyNotch");
                if let Ok(hwnd) = FindWindowW(class_name, win_name)
                    && hwnd.0 as isize != 0
                {
                    let _ = PostMessageW(hwnd, 0x8000 + 1, WPARAM(0), LPARAM(0));
                }
                log_info!("[CyNotch] Active instance signaled to expand. Exiting second launcher.");
                return;
            }
            let _ = m;
        }

        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    log_info!("============================================================");
    log_info!("  ⚡ CyNotch (General Mahamatra Dynamic Island for Windows)");
    log_info!("  Hotkeys: Ctrl+Shift+C (Toggle), M (Purge RAM), S (Capture)");
    log_info!("============================================================");

    let mut init_state = AppState::new();
    init_state.detected_hz = cynotch::window::detect_monitor_hz();
    let state = Arc::new(Mutex::new(init_state));

    let poll_state = state.clone();
    thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        cynotch::memreduct::enable_memory_privileges();
        let mut collector = TelemetryCollector::new();
        let mut tracker = ScreenTimeTracker::new();
        let mut media_poller = media::MediaPoller::new();
        let mut audio_mon = audio::AudioMonitor::new();
        let mut notif_bridge = notifications::SystemNotificationBridge::new();
        let mut notif_retry_ticks: u32 = 0;
        let mut incoming_toasts: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut last_threshold_clean = Instant::now();
        let mut last_interval_clean = Instant::now();
        let mut last_sec_tick = Instant::now();
        let mut last_song_title = String::new();
        let mut last_gsmtc_pos_ms: u64 = 0;
        let mut last_clock_minute: u32 = chrono::Local::now().minute();
        let mut first_pass = true;
        let mut notif_poll_tick: u32 = 0;
        let mut notif_db_sig: Option<notifications::NotifDbSignature> = None;
        let mut last_notif_poll = Instant::now();

        loop {
            if first_pass {
                first_pass = false;
                let init_hour = (chrono::Local::now().hour() as usize).min(23);
                let init_stats = tracker.stats(init_hour);
                if let Ok(mut s) = poll_state.lock() {
                    s.screen_time = init_stats;
                }
            } else {
                thread::sleep(Duration::from_millis(500));
            }

            if cynotch::window::SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
                tracker.save();
                break;
            }

            if cynotch::screentime::RESET_SCREENTIME_REQUESTED.swap(false, Ordering::AcqRel) {
                tracker.reset_today();
            }

            let media_opt = { media_poller.poll() };
            let vol_opt = if !cynotch::window::IS_DRAGGING_VOLUME.load(Ordering::Acquire)
                && !audio::recently_wrote_volume()
            {
                audio_mon.read_master_volume()
            } else {
                None
            };

            let (screentime_enabled, notif_enabled, notif_auto_clear) = poll_state
                .lock()
                .map(|s| {
                    (
                        s.screentime_enabled,
                        s.notif_listener_enabled,
                        s.notif_auto_clear_action_center,
                    )
                })
                .unwrap_or((true, false, false));

            if cynotch::window::NOTIF_SYNC_REQUESTED.swap(false, Ordering::AcqRel) && notif_enabled
            {
                notif_bridge.reset();
                notif_retry_ticks = 0;
                notif_db_sig = None;
            }

            let mut notif_status_update: Option<String> = None;
            {
                if notif_enabled {
                    if !notif_bridge.is_connected() {
                        let st_now = notif_bridge.status();
                        let should_try = st_now == "Off"
                            || (st_now != "Permission needed" && notif_retry_ticks >= 10)
                            || (st_now == "Permission needed" && notif_retry_ticks >= 20);
                        if should_try {
                            notif_retry_ticks = 0;
                            let st_str = notif_bridge.try_init();
                            // A reconnect must poll immediately, not wait for the
                            // forced-refresh window.
                            notif_db_sig = None;
                            notif_status_update = Some(st_str);
                        } else {
                            notif_retry_ticks = notif_retry_ticks.saturating_add(1);
                        }
                    }
                    if notif_bridge.is_connected() {
                        notif_retry_ticks = 0;
                        // `GetNotificationsAsync().get()` marshals the whole Action
                        // Center across RPC on every call: ~1 ms CPU per toast, so
                        // 25-35 ms with a normal Action Center. The toast database
                        // only changes when a toast arrives, is dismissed or expires,
                        // so poll on that signal instead of on a blind 1 Hz timer.
                        // `should_poll_toasts` still forces a poll every few seconds,
                        // so banners cannot starve if the signal ever goes quiet.
                        notif_poll_tick = notif_poll_tick.wrapping_add(1);
                        if notif_poll_tick.is_multiple_of(2) {
                            let sig = notifications::notifications_db_signature();
                            let due = notifications::should_poll_toasts(
                                notif_db_sig,
                                &sig,
                                last_notif_poll.elapsed().as_secs(),
                            );
                            if due {
                                let batch = notif_bridge.poll_new_toasts_with_aumid();
                                if notif_auto_clear {
                                    for (id, _, _, _, _) in &batch {
                                        notif_bridge.remove_toast(*id);
                                    }
                                }
                                // Baseline captured *after* the poll and after our own
                                // auto-clear removals, so those writes do not look like
                                // a fresh change and trigger another 55 ms poll.
                                notif_db_sig = Some(notifications::notifications_db_signature());
                                last_notif_poll = Instant::now();
                                if !notif_bridge.is_connected() {
                                    notif_status_update = Some(notif_bridge.status().to_string());
                                }
                                for (_, app, title, sub, aumid) in batch {
                                    incoming_toasts.push((app, title, sub, aumid));
                                }
                            }
                        }
                    }
                } else if notif_bridge.is_connected() || notif_bridge.status() != "Off" {
                    notif_bridge.reset();
                    notif_retry_ticks = 0;
                    notif_db_sig = None;
                    incoming_toasts.clear();
                    notif_status_update = Some("Off".to_string());
                }
            }

            let (telem_opt, screen_stats) = if last_sec_tick.elapsed() >= Duration::from_millis(950)
            {
                last_sec_tick = Instant::now();
                let st_opt = {
                    if screentime_enabled {
                        Some(tracker.tick())
                    } else {
                        let hour = (chrono::Local::now().hour() as usize).min(23);
                        Some(tracker.stats(hour))
                    }
                };
                let telem = { collector.poll() };
                (Some(telem), st_opt)
            } else {
                (None, None)
            };

            let mut auto_clean_params: Option<cynotch::memreduct::CleanAreas> = None;
            let mut song_to_fetch: Option<(String, String)> = None;
            let mut ui_changed = false;
            let mut settings_changed = false;

            if let Ok(mut s) = poll_state.lock() {
                if let Some(ref next_status) = notif_status_update
                    && s.notif_listener_status != *next_status
                {
                    s.notif_listener_status = next_status.clone();
                    settings_changed = true;
                }
                for (app, title, sub, aumid) in incoming_toasts.drain(..) {
                    s.push_system_toast(app, title, sub, aumid);
                    ui_changed = true;
                }

                if let Some(telem) = telem_opt {
                    let prev_ram_int = s.telemetry.memory_percent.round() as i32;
                    let prev_used_tenth = (s.telemetry.memory_used_gb * 10.0).round() as i32;
                    let new_ram_int = telem.memory_percent.round() as i32;
                    let new_used_tenth = (telem.memory_used_gb * 10.0).round() as i32;
                    let ram_changed =
                        (s.telemetry.memory_percent - telem.memory_percent).abs() >= 0.5;
                    let cpu_changed = (s.telemetry.cpu_usage - telem.cpu_usage).abs() >= 1.0;
                    let net_changed = (s.telemetry.net_down_kbps - telem.net_down_kbps).abs()
                        >= 5.0
                        || (s.telemetry.net_up_kbps - telem.net_up_kbps).abs() >= 5.0;
                    let telem_changed = ram_changed
                        || (s.telemetry.pagefile_percent - telem.pagefile_percent).abs() >= 0.5
                        || (s.telemetry.sys_cache_mb - telem.sys_cache_mb).abs() >= 16.0
                        || cpu_changed
                        || net_changed
                        || s.telemetry.on_ac != telem.on_ac
                        || s.telemetry.battery_pct != telem.battery_pct
                        || s.telemetry.has_admin_privileges != telem.has_admin_privileges;
                    s.telemetry = telem;
                    if new_ram_int != prev_ram_int || new_used_tenth != prev_used_tenth {
                        crate::window::post_update_tray();
                    }
                    if telem_changed {
                        settings_changed = true;
                        if s.is_hovered
                            && matches!(
                                s.mode,
                                NotchMode::Home | NotchMode::MemReduct | NotchMode::Telemetry
                            )
                        {
                            ui_changed = true;
                        } else if !s.is_hovered
                            && !s.minimized_island
                            && s.notification.is_none()
                            && !s.media.is_playing
                        {
                            let idle_metric_changed = (s.progress_arc_enabled && ram_changed)
                                || match s.idle_metric {
                                    IdleMetric::Ram => ram_changed,
                                    IdleMetric::Cpu => cpu_changed,
                                    IdleMetric::NetSpeed => net_changed,
                                    IdleMetric::Clock | IdleMetric::Minimal => false,
                                };
                            if idle_metric_changed {
                                ui_changed = true;
                            }
                        }
                    }
                }

                let cur_min = chrono::Local::now().minute();
                if cur_min != last_clock_minute {
                    last_clock_minute = cur_min;
                    if !s.is_hovered
                        && !s.minimized_island
                        && s.notification.is_none()
                        && !s.media.is_playing
                        && s.idle_metric == IdleMetric::Clock
                    {
                        ui_changed = true;
                    }
                }

                if let Some(stats) = screen_stats {
                    let st_changed = stats.today_secs != s.screen_time.today_secs
                        || stats.top_app_name != s.screen_time.top_app_name;
                    s.screen_time = stats;
                    if st_changed && s.is_hovered && s.mode == NotchMode::Telemetry {
                        ui_changed = true;
                    }
                }

                if let Some(v) = vol_opt
                    && (s.volume_pct - v).abs() > 0.015
                {
                    s.volume_pct = v;
                    if v > 0.05 {
                        s.pre_mute_volume = v;
                    }
                    if s.is_hovered && s.mode == NotchMode::Volume {
                        ui_changed = true;
                    }
                }

                if let Some(mut m) = media_opt {
                    let same_song = m.title == last_song_title && m.artist == s.media.artist;
                    if !same_song {
                        last_song_title = m.title.clone();
                        last_gsmtc_pos_ms = m.position_ms;
                        s.lyrics.clear();
                        s.current_lyric_index = None;
                        s.current_lyric_text = format!("Playing • {}", m.title);
                        s.lyrics_offset = 0;
                        if s.lyrics_enabled && !m.title.is_empty() {
                            song_to_fetch = Some((m.title.clone(), m.artist.clone()));
                        }
                        ui_changed = true;
                    } else {
                        if m.is_playing != s.media.is_playing || m.album != s.media.album {
                            ui_changed = true;
                        }
                        if m.position_ms == last_gsmtc_pos_ms {
                            m.position_ms = s.media.position_ms;
                        } else {
                            last_gsmtc_pos_ms = m.position_ms;
                            if s.is_hovered && s.mode == NotchMode::Music {
                                ui_changed = true;
                            }
                        }
                    }
                    s.media = m;
                } else if !s.media.is_idle() {
                    s.media = models::MediaTrack::idle();
                    s.lyrics.clear();
                    s.current_lyric_index = None;
                    s.current_lyric_text = "Play a track to sync lyrics".to_string();
                    s.lyrics_offset = 0;
                    last_song_title.clear();
                    last_gsmtc_pos_ms = 0;
                    ui_changed = true;
                }

                if !s.lyrics.is_empty() && !s.media.is_idle() {
                    let pos_sec = s.media.position_ms as f64 / 1000.0;
                    if let Some(idx) = s.lyrics.iter().rposition(|l| l.seconds <= pos_sec)
                        && s.current_lyric_index != Some(idx)
                    {
                        s.current_lyric_index = Some(idx);
                        s.current_lyric_text = s.lyrics[idx].text.clone();
                        if s.is_hovered && s.mode == NotchMode::Music && s.show_lyrics {
                            ui_changed = true;
                        }
                    }
                }

                if s.mem_reduct.every_mins == 0 {
                    last_interval_clean = Instant::now();
                }
                let trig_threshold =
                    s.should_trigger_threshold_clean(last_threshold_clean.elapsed().as_secs());
                let trig_interval =
                    s.should_trigger_interval_clean(last_interval_clean.elapsed().as_secs());

                if (trig_threshold || trig_interval)
                    && cynotch::window::PURGE_IN_PROGRESS
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    if trig_threshold {
                        last_threshold_clean = Instant::now();
                    }
                    if trig_interval {
                        last_interval_clean = Instant::now();
                    }
                    s.mem_reduct.is_purging = true;
                    s.mem_reduct.last_cleaned_status = "Auto cleaning RAM...".to_string();
                    auto_clean_params = Some(s.mem_reduct.to_clean_areas());
                    settings_changed = true;
                    if s.is_hovered && s.mode == NotchMode::MemReduct {
                        ui_changed = true;
                    }
                }
            }

            if ui_changed {
                cynotch::window::post_redraw();
            }
            if settings_changed {
                cynotch::settings_window::post_redraw();
            }

            if let Some(areas) = auto_clean_params {
                let clean_state = poll_state.clone();
                thread::spawn(move || {
                    let res = memreduct::execute_memory_clean_areas(&areas);
                    if let Ok(mut s) = clean_state.lock() {
                        s.mem_reduct.is_purging = false;
                        if let Some(clean_res) = res {
                            s.apply_clean_result(clean_res);
                        } else {
                            s.mem_reduct.last_cleaned_status =
                                "Ready • Auto clean done".to_string();
                        }
                    }
                    cynotch::window::PURGE_IN_PROGRESS.store(false, Ordering::Release);
                    cynotch::window::post_redraw();
                    cynotch::settings_window::post_redraw();
                });
            }

            if let Some((title, artist)) = song_to_fetch {
                let fetch_state = poll_state.clone();
                thread::spawn(move || {
                    let lines = lyrics::fetch_lyrics(&title, &artist);
                    if !lines.is_empty() {
                        let mut updated = false;
                        if let Ok(mut s) = fetch_state.lock()
                            && s.media.title == title
                        {
                            s.lyrics = lines;
                            updated = true;
                        }
                        if updated {
                            cynotch::window::post_redraw();
                        }
                    }
                });
            }
        }
    });

    match NotchWindow::create(state) {
        Ok(window) => {
            log_info!("CyNotch initialized successfully at screen top-center!");
            window.run_loop();
        }
        Err(e) => {
            let path = cynotch::models::crash_log_path("err.txt");
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, format!("Failed to create window: {:?}", e));
            log_err!("Failed to create CyNotch window: {:?}", e);
        }
    }
}
