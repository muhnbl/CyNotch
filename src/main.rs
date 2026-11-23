#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cynotch::localtime::{format_rfc3339, local_now};
use cynotch::models::{self, AppState, IdleMetric, NotchMode};
use cynotch::telemetry::TelemetryCollector;
use cynotch::window::NotchWindow;
use cynotch::*;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
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

#[repr(C)]
struct RawExceptionRecord {
    exception_code: u32,
    exception_flags: u32,
    exception_record: *const RawExceptionRecord,
    exception_address: *const core::ffi::c_void,
    number_parameters: u32,
    exception_information: [usize; 15],
}

#[repr(C)]
struct RawExceptionPointers {
    exception_record: *const RawExceptionRecord,
    context_record: *const core::ffi::c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetUnhandledExceptionFilter(
        filter: Option<unsafe extern "system" fn(*const RawExceptionPointers) -> i32>,
    ) -> *const core::ffi::c_void;
}

unsafe extern "system" fn unhandled_seh_filter(ptrs: *const RawExceptionPointers) -> i32 {
    if !ptrs.is_null() {
        let rec = unsafe { (*ptrs).exception_record };
        if !rec.is_null() {
            let code = unsafe { (*rec).exception_code };
            let addr = unsafe { (*rec).exception_address as usize };
            let nparams = unsafe { ((*rec).number_parameters as usize).min(4) };
            let p0 = if nparams > 0 { unsafe { (*rec).exception_information[0] } } else { 0 };
            let p1 = if nparams > 1 { unsafe { (*rec).exception_information[1] } } else { 0 };
            let path = cynotch::models::crash_log_path("crash.txt");
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
                    "--- {} ---\nSEH CRASH: code=0x{:08X} addr=0x{:016X} p0=0x{:X} p1=0x{:X}",
                    format_rfc3339(local_now()),
                    code,
                    addr,
                    p0,
                    p1
                );
            }
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

fn main() {
    unsafe {
        SetUnhandledExceptionFilter(Some(unhandled_seh_filter));
    }
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
                format_rfc3339(local_now()),
                info,
                bt
            );
        }
    }));

    unsafe {
        let skip_single_instance =
            std::env::var_os("CYNOTCH_SKIP_SINGLE_INSTANCE").is_some_and(|v| v == "1");
        if !skip_single_instance {
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
                    log_info!(
                        "[CyNotch] Active instance signaled to expand. Exiting second launcher."
                    );
                    let _ = CloseHandle(m);
                    return;
                }
                let _ = m;
            }
        }

        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // OLE STA required for RegisterDragDrop / DoDragDrop on the main UI thread.
        let _ = OleInitialize(None);
    }

    // Safe to wipe ephemeral shelf-cache only AFTER single-instance mutex succeeds.
    cynotch::shelf_dnd::wipe_shelf_cache();

    log_info!("============================================================");
    log_info!("   CyNotch (General Mahamatra Dynamic Island for Windows)");
    log_info!("  Hotkeys: Ctrl+Alt+C (Toggle), M (Purge RAM), S (Capture)");
    log_info!("============================================================");

    let is_smoke = std::env::var_os("CYNOTCH_SMOKE_EXIT_MS").is_some();
    let mut init_state = AppState::new();
    init_state.detected_hz = cynotch::window::detect_monitor_hz();
    if !is_smoke {
        notifications::recover_windows_sound_mute_on_startup(
            init_state.should_mute_windows_notif_sound(),
        );
    }
    let state = Arc::new(Mutex::new(init_state));

    let poll_state = state.clone();
    let poll_handle = thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        cynotch::memreduct::enable_memory_privileges();
        let mut collector = TelemetryCollector::new();
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
                let mut first_pass = true;
                let mut notif_poll_tick: u32 = 0;
                let mut notif_db_sig: Option<notifications::NotifDbSignature> = None;
                let mut last_notif_poll = Instant::now();
                let mut last_clock_minute: u32 = local_now().minute;

                loop {
                    if first_pass {
                        first_pass = false;
                    } else {
                        thread::sleep(Duration::from_millis(500));
                    }

            if cynotch::window::SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
                break;
            }

            let media_opt = { media_poller.poll() };
            let vol_opt = if !cynotch::window::IS_DRAGGING_VOLUME.load(Ordering::Acquire)
                && !audio::recently_wrote_volume()
            {
                audio_mon.read_master_volume()
            } else {
                None
            };

            let (notif_enabled, notif_auto_clear, need_net) = poll_state
                .lock()
                .map(|s| {
                    (
                        s.notif_listener_enabled,
                        s.notif_auto_clear_action_center,
                        s.is_hovered || s.idle_metric == IdleMetric::NetSpeed,
                    )
                })
                .unwrap_or((false, false, false));

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
                        let perm_needed = st_now.eq_ignore_ascii_case("permission needed");
                        let should_try = st_now == "Off"
                            || (!perm_needed && notif_retry_ticks >= 10)
                            || (perm_needed && notif_retry_ticks >= 20);
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

            let telem_opt = if last_sec_tick.elapsed() >= Duration::from_millis(950) {
                last_sec_tick = Instant::now();
                Some(collector.poll_with_network(need_net))
            } else {
                None
            };

            let mut auto_clean_params: Option<cynotch::memreduct::CleanAreas> = None;
            let mut song_to_fetch: Option<(String, String)> = None;
            let mut notif_audio: Option<(cynotch::sound::NotificationSound, bool, bool, bool)> = None;
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
                    if s.notifications_enabled {
                        notif_audio = Some((
                            s.notification_sound,
                            s.notifications_enabled,
                            s.hide_in_fullscreen,
                            s.notif_over_fullscreen,
                        ));
                    }
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
                            && matches!(s.mode, NotchMode::Home | NotchMode::MemReduct)
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

                let cur_min = local_now().minute;
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
                        s.current_lyric_text = format!("Playing  -  {}", m.title);
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
                            // Prevent SMTC LastUpdatedTime clock skew from jumping the progress bar backward while playing.
                            if s.media.is_playing
                                && m.is_playing
                                && m.position_ms < s.media.position_ms
                                && s.media.position_ms - m.position_ms < 2500
                            {
                                m.position_ms = s.media.position_ms;
                            } else {
                                last_gsmtc_pos_ms = m.position_ms;
                                if s.is_hovered && s.mode == NotchMode::Music {
                                    ui_changed = true;
                                }
                            }
                        }
                    }
                    s.media = m;
                    s.sync_capsule_state_transition();
                } else if !s.media.is_idle() {
                    s.media = models::MediaTrack::idle();
                    s.lyrics.clear();
                    s.current_lyric_index = None;
                    s.current_lyric_text = "Play a track to sync lyrics".to_string();
                    s.lyrics_offset = 0;
                    last_song_title.clear();
                    last_gsmtc_pos_ms = 0;
                    s.sync_capsule_state_transition();
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

            if let Some((sound, enabled, hide_fs, over_fs)) = notif_audio {
                let suppressed =
                    cynotch::window::is_fullscreen_suppressing_notifications(hide_fs, over_fs);
                cynotch::sound::play_notification_sound(sound, enabled, suppressed);
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
                                "Ready  -  Auto clean done".to_string();
                        }
                    }
                    cynotch::window::PURGE_IN_PROGRESS.store(false, Ordering::Release);
                    cynotch::window::post_redraw();
                    cynotch::settings_window::post_redraw();
                });
            }

            if let Some((title, artist)) = song_to_fetch {
                // BH-30: Coalescing single lyrics worker prevents unbounded thread/WinHTTP
                // spawning when the user rapidly skips tracks.
                enqueue_lyrics_fetch(poll_state.clone(), title, artist);
            }
        }
    });

    if !is_smoke {
        let mute_win_sound = state
            .lock()
            .map(|s| s.should_mute_windows_notif_sound())
            .unwrap_or(false);
        if mute_win_sound {
            notifications::sync_windows_notification_sound_mute(true);
        }
    }

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
    // BH-41: Always signal shutdown and join poll_handle even if window creation failed.
    cynotch::window::SHUTDOWN_REQUESTED.store(true, Ordering::Release);
    let _ = poll_handle.join();

    if !is_smoke {
        notifications::sync_windows_notification_sound_mute(false);
    }
    cynotch::shelf_dnd::wipe_shelf_cache();
    unsafe {
        OleUninitialize();
    }
}

/// Coalescing single background worker for lyrics fetching (BH-30, BH-41).
/// Retryable on transient OS spawn failure and panic-isolated via `catch_unwind`.
fn enqueue_lyrics_fetch(state: Arc<Mutex<AppState>>, title: String, artist: String) {
    use std::sync::Condvar;
    struct LyricsWorker {
        pending: Mutex<Option<(Arc<Mutex<AppState>>, String, String)>>,
        cvar: Condvar,
    }
    static WORKER: Mutex<Option<Arc<LyricsWorker>>> = Mutex::new(None);

    let worker = {
        let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = slot.as_ref() {
            Some(Arc::clone(existing))
        } else {
            let w = Arc::new(LyricsWorker {
                pending: Mutex::new(None),
                cvar: Condvar::new(),
            });
            let bg = Arc::clone(&w);
            if thread::Builder::new()
                .name("cynotch-lyrics".to_string())
                .spawn(move || loop {
                    let job = {
                        let mut guard = bg.pending.lock().unwrap_or_else(|e| e.into_inner());
                        while guard.is_none() {
                            guard = bg.cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
                        }
                        guard.take()
                    };
                    if let Some((fetch_state, title, artist)) = job {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                        }));
                    }
                })
                .is_ok()
            {
                *slot = Some(Arc::clone(&w));
                Some(w)
            } else {
                None
            }
        }
    };

    if let Some(worker) = worker {
        let mut guard = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some((state, title, artist));
        worker.cvar.notify_one();
    }
}
