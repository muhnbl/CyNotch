#![allow(unsafe_op_in_unsafe_fn)]

use cynotch::backdrop;
use cynotch::backdrop_geom;
use cynotch::models::AppState;
use cynotch::notifications;
use cynotch::settings_window;
use cynotch::window::{self, NotchWindow, WM_APP_REDRAW};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);
static RUNTIME_TEST_LOCK: Mutex<()> = Mutex::new(());

struct EnvRestoreGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl Drop for EnvRestoreGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

struct ChildGuard {
    child: Option<std::process::Child>,
}

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn id(&self) -> u32 {
        self.child.as_ref().expect("child still active").id()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.child.as_mut() {
            Some(c) => {
                let res = c.try_wait()?;
                if res.is_some() {
                    self.child = None;
                }
                Ok(res)
            }
            None => Ok(None),
        }
    }

    fn kill_and_wait(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

struct TempLocalAppData {
    root: PathBuf,
}

impl TempLocalAppData {
    fn new(tag: &str, write_config: bool) -> Self {
        let id = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "cynotch_rt_{}_{}_{}",
            tag,
            std::process::id(),
            id
        ));
        let cfg_dir = root.join("CyNotch");
        std::fs::create_dir_all(&cfg_dir).expect("create temp LOCALAPPDATA\\CyNotch");
        if write_config {
            let cfg = serde_json::json!({
                "frosted_glass_enabled": true,
                "minimized_island": false,
                "hide_in_fullscreen": false,
                "notch_style": "Floating",
                "placement": "Top"
            });
            std::fs::write(cfg_dir.join("config.json"), cfg.to_string())
                .expect("write temp config.json");
        }
        Self { root }
    }
}

impl Drop for TempLocalAppData {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct EnumTarget {
    pid: u32,
    matches: Vec<(HWND, String)>,
}

unsafe extern "system" fn enum_pid_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target = &mut *(lparam.0 as *mut EnumTarget);
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == target.pid {
        let mut buf = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            if cls == "CyNotchWindowClass"
                || cls == backdrop::CLASS_NAME_STR
                || cls == "CyNotchSettingsClass"
            {
                target.matches.push((hwnd, cls));
            }
        }
    }
    BOOL(1)
}

unsafe fn find_pid_windows(pid: u32) -> Vec<(HWND, String)> {
    let mut target = EnumTarget {
        pid,
        matches: Vec::new(),
    };
    let _ = EnumWindows(
        Some(enum_pid_windows_cb),
        LPARAM(&mut target as *mut EnumTarget as isize),
    );
    target.matches
}

unsafe fn pump_messages(duration: Duration) {
    let deadline = Instant::now() + duration;
    let mut msg = MSG::default();
    while Instant::now() < deadline {
        let mut drained = false;
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            drained = true;
            if msg.message == WM_QUIT {
                return;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !drained {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

unsafe fn pump_until_quit(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut msg = MSG::default();
    while Instant::now() < deadline {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return true;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn runtime_backdrop_lifecycle_zorder_settings_and_fallback() {
    let _runtime_guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempLocalAppData::new("lib", true);
    let _local_appdata_guard = EnvRestoreGuard {
        key: "LOCALAPPDATA",
        prev: std::env::var_os("LOCALAPPDATA"),
    };
    unsafe {
        std::env::set_var("LOCALAPPDATA", &tmp.root);
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    assert!(
        notifications::is_windows_transparency_enabled(),
        "Host Windows Transparency effects must be enabled for this test"
    );
    assert!(
        backdrop::blur_available(),
        "backdrop::blur_available() should be true in normal runtime test"
    );

    let mut init_state = AppState::new();
    init_state.show_mascot = false;
    init_state.sync_preview_settled();
    assert!(init_state.is_frosted_active());
    let state = Arc::new(Mutex::new(init_state));

    let notch = NotchWindow::create(state.clone()).expect("NotchWindow::create must succeed");
    unsafe {
        pump_messages(Duration::from_millis(120));

        let pid = GetCurrentProcessId();
        let wins = find_pid_windows(pid);
        let backdrops: Vec<HWND> = wins
            .iter()
            .filter(|(_, c)| c == backdrop::CLASS_NAME_STR)
            .map(|(h, _)| *h)
            .collect();
        assert_eq!(
            backdrops.len(),
            1,
            "Expected exactly 1 backdrop HWND after NotchWindow::create, found {:?}",
            wins
        );
        let backdrop_hwnd = backdrops[0];

        assert!(IsWindowVisible(notch.hwnd).as_bool());
        assert!(IsWindowVisible(backdrop_hwnd).as_bool());
        assert_eq!(
            GetWindow(notch.hwnd, GW_HWNDNEXT),
            Ok(backdrop_hwnd),
            "Backdrop HWND must sit directly below content HWND"
        );

        let ex_style = GetWindowLongW(backdrop_hwnd, GWL_EXSTYLE) as u32;
        assert_ne!(ex_style & WS_EX_TRANSPARENT.0, 0, "Missing WS_EX_TRANSPARENT");
        assert_ne!(ex_style & WS_EX_TOOLWINDOW.0, 0, "Missing WS_EX_TOOLWINDOW");
        assert_ne!(ex_style & WS_EX_NOACTIVATE.0, 0, "Missing WS_EX_NOACTIVATE");
        assert_ne!(ex_style & WS_EX_TOPMOST.0, 0, "Missing WS_EX_TOPMOST");
        assert_eq!(
            ex_style & WS_EX_NOREDIRECTIONBITMAP.0,
            0,
            "Backdrop must be redirected (no WS_EX_NOREDIRECTIONBITMAP)"
        );

        // Verify REST_INSET rect matches notch_backdrop_geom + backdrop_geom::placement
        let mut notch_rc = RECT::default();
        let mut bd_rc = RECT::default();
        GetWindowRect(notch.hwnd, &mut notch_rc).expect("GetWindowRect(notch)");
        GetWindowRect(backdrop_hwnd, &mut bd_rc).expect("GetWindowRect(backdrop)");
        let cw = notch_rc.right - notch_rc.left;
        let ch = notch_rc.bottom - notch_rc.top;
        let expected_p = {
            let st = state.lock().unwrap();
            let g = window::notch_backdrop_geom(&st, cw, ch).expect("notch_backdrop_geom");
            let inset = backdrop::inset_for_radius(g.diam as f32 * 0.5);
            backdrop_geom::placement(notch_rc.left, notch_rc.top, &g, inset)
                .expect("expected placement")
        };
        assert_eq!(
            (
                bd_rc.left,
                bd_rc.top,
                bd_rc.right - bd_rc.left,
                bd_rc.bottom - bd_rc.top
            ),
            (expected_p.x, expected_p.y, expected_p.w, expected_p.h),
            "Backdrop rect must match REST_INSET-adjusted shell geometry"
        );

        // Visibility sync: SW_HIDE / SW_SHOWNOACTIVATE
        let _ = ShowWindow(notch.hwnd, SW_HIDE);
        pump_messages(Duration::from_millis(60));
        assert!(
            !IsWindowVisible(backdrop_hwnd).as_bool(),
            "Backdrop must hide when content window hides"
        );
        let _ = ShowWindow(notch.hwnd, SW_SHOWNOACTIVATE);
        pump_messages(Duration::from_millis(60));
        assert!(
            IsWindowVisible(backdrop_hwnd).as_bool(),
            "Backdrop must reappear when content window shows"
        );
        assert_eq!(
            GetWindow(notch.hwnd, GW_HWNDNEXT),
            Ok(backdrop_hwnd),
            "Backdrop must re-seat below content after SW_SHOWNOACTIVATE"
        );

        // Z-order re-seating: place a temporary topmost window above notch, then re-promote notch
        let h_mod = GetModuleHandleW(None).expect("GetModuleHandleW");
        let temp_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("STATIC"),
            w!("TempTopmostProbe"),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            24,
            24,
            None,
            None,
            h_mod,
            None,
        )
        .expect("CreateWindowExW temp topmost");
        SetWindowPos(
            temp_hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .expect("SetWindowPos temp_hwnd topmost");
        assert_eq!(
            GetWindow(notch.hwnd, GW_HWNDPREV),
            Ok(temp_hwnd),
            "temp_hwnd should sit immediately above notch before notch re-promotes"
        );
        SetWindowPos(
            notch.hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .expect("SetWindowPos notch topmost");
        pump_messages(Duration::from_millis(50));
        assert_eq!(
            GetWindow(notch.hwnd, GW_HWNDNEXT),
            Ok(backdrop_hwnd),
            "Backdrop must re-seat directly below content after notch re-promotes past temp_hwnd"
        );
        let _ = DestroyWindow(temp_hwnd);

        // Settings window backdrop creation & teardown on WM_CLOSE
        settings_window::open_or_focus(state.clone());
        pump_messages(Duration::from_millis(120));
        let settings_hwnd =
            HWND(settings_window::SETTINGS_HWND.load(Ordering::Acquire) as *mut std::ffi::c_void);
        assert!(IsWindow(settings_hwnd).as_bool(), "Settings HWND must exist");
        assert!(
            IsWindowVisible(settings_hwnd).as_bool(),
            "Settings HWND must be visible"
        );
        let settings_bd = GetWindow(settings_hwnd, GW_HWNDNEXT).expect("settings GW_HWNDNEXT");
        let mut cls_buf = [0u16; 64];
        let clen = GetClassNameW(settings_bd, &mut cls_buf);
        let s_cls = String::from_utf16_lossy(&cls_buf[..clen.max(0) as usize]);
        assert_eq!(
            s_cls,
            backdrop::CLASS_NAME_STR,
            "Settings window must have its own CyNotchBackdropClass directly below it"
        );
        assert_ne!(settings_bd, backdrop_hwnd);
        assert!(IsWindowVisible(settings_bd).as_bool());
        {
            use windows::Win32::Graphics::Dwm::{
                DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                DwmGetWindowAttribute,
            };
            let mut corner = DWM_WINDOW_CORNER_PREFERENCE::default();
            DwmGetWindowAttribute(
                settings_bd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &mut corner as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
            .expect("DwmGetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE)");
            assert_eq!(
                corner, DWMWCP_ROUND,
                "Settings backdrop must use DWMWCP_ROUND so Windows 11 DWM rounds its corners"
            );
        }

        let _ = PostMessageW(settings_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        pump_messages(Duration::from_millis(120));
        assert!(
            !IsWindow(settings_hwnd).as_bool(),
            "Settings window must be destroyed on WM_CLOSE"
        );
        assert!(
            !IsWindow(settings_bd).as_bool(),
            "Settings backdrop must be destroyed when settings closes"
        );

        // windows_transparency_enabled = false hides backdrop, true restores it
        {
            let mut st = state.lock().unwrap();
            st.windows_transparency_enabled = false;
        }
        let _ = SendMessageW(notch.hwnd, WM_APP_REDRAW, WPARAM(0), LPARAM(0));
        assert!(
            !IsWindowVisible(backdrop_hwnd).as_bool(),
            "Backdrop must hide when windows_transparency_enabled = false"
        );
        {
            let mut st = state.lock().unwrap();
            st.windows_transparency_enabled = true;
        }
        let _ = SendMessageW(notch.hwnd, WM_APP_REDRAW, WPARAM(0), LPARAM(0));
        assert!(
            IsWindowVisible(backdrop_hwnd).as_bool(),
            "Backdrop must restore when windows_transparency_enabled = true"
        );
        assert_eq!(GetWindow(notch.hwnd, GW_HWNDNEXT), Ok(backdrop_hwnd));

        // backdrop::mark_unavailable() destroys backdrop and engages 118% fallback
        backdrop::mark_unavailable();
        assert!(!backdrop::blur_available());
        let _ = SendMessageW(notch.hwnd, WM_APP_REDRAW, WPARAM(0), LPARAM(0));
        assert!(
            !IsWindow(backdrop_hwnd).as_bool(),
            "Backdrop HWND must be destroyed after mark_unavailable()"
        );
        assert_eq!(
            state.lock().unwrap().frosted_alpha_pct(),
            backdrop::GLASS_FALLBACK_OPACITY_PCT
        );

        // Clean WM_CLOSE teardown
        let _ = PostMessageW(notch.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        assert!(
            pump_until_quit(Duration::from_secs(3)),
            "Notch message loop should receive WM_QUIT on WM_CLOSE"
        );
        assert!(window::SHUTDOWN_REQUESTED.load(Ordering::Acquire));
        assert!(!IsWindow(notch.hwnd).as_bool());
        let remaining = find_pid_windows(pid);
        assert!(
            remaining.is_empty(),
            "All CyNotch windows must be destroyed after WM_CLOSE, found {:?}",
            remaining
        );
    }
}

fn spawn_cynotch_and_wait_windows(
    tmp: &TempLocalAppData,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
    expect_backdrop: bool,
) {
    let exe = env!("CARGO_BIN_EXE_cynotch");
    let mut cmd = Command::new(exe);
    cmd.args(extra_args)
        .env("LOCALAPPDATA", &tmp.root)
        .env("CYNOTCH_SKIP_SINGLE_INSTANCE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let mut guard = ChildGuard::new(cmd.spawn().expect("spawn cynotch.exe"));
    let pid = guard.id();

    let deadline = Instant::now() + Duration::from_secs(25);
    let mut content_hwnd = None;
    let mut backdrop_hwnd = None;

    while Instant::now() < deadline {
        if let Ok(Some(status)) = guard.try_wait() {
            panic!("cynotch.exe exited unexpectedly early with {:?}", status);
        }
        let wins = unsafe { find_pid_windows(pid) };
        content_hwnd = wins
            .iter()
            .find(|(_, c)| c == "CyNotchWindowClass")
            .map(|(h, _)| *h);
        backdrop_hwnd = wins
            .iter()
            .find(|(_, c)| c == backdrop::CLASS_NAME_STR)
            .map(|(h, _)| *h);
        let content_visible = content_hwnd
            .map(|h| unsafe { IsWindowVisible(h).as_bool() })
            .unwrap_or(false);
        let backdrop_ready = if expect_backdrop {
            backdrop_hwnd
                .map(|h| unsafe { IsWindowVisible(h).as_bool() })
                .unwrap_or(false)
        } else {
            true
        };
        if content_visible && backdrop_ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let c_hwnd = content_hwnd.expect("CyNotchWindowClass HWND did not appear");
    unsafe {
        assert!(
            IsWindowVisible(c_hwnd).as_bool(),
            "CyNotchWindowClass HWND did not become visible within 25s"
        );
        if expect_backdrop {
            let b_hwnd = backdrop_hwnd.expect("CyNotchBackdropClass HWND did not appear");
            assert!(
                IsWindowVisible(b_hwnd).as_bool(),
                "CyNotchBackdropClass HWND did not become visible within 25s"
            );
            assert_eq!(
                GetWindow(c_hwnd, GW_HWNDNEXT),
                Ok(b_hwnd),
                "Subprocess backdrop must sit directly below content HWND"
            );
        } else {
            // Give the app a 500ms settle window after c_hwnd is visible to ensure no backdrop is created lazily.
            std::thread::sleep(Duration::from_millis(500));
            let wins = find_pid_windows(pid);
            let backdrops: Vec<_> = wins
                .iter()
                .filter(|(_, c)| c == backdrop::CLASS_NAME_STR)
                .collect();
            assert!(
                backdrops.is_empty(),
                "Expected zero CyNotchBackdropClass HWNDs in no-frost mode, found {:?}",
                backdrops
            );
        }

        let _ = PostMessageW(c_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }

    let exit_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Some(status)) = guard.try_wait() {
            assert!(
                status.success(),
                "cynotch.exe should exit 0 on WM_CLOSE, got {:?}",
                status
            );
            return;
        }
        if Instant::now() >= exit_deadline {
            guard.kill_and_wait();
            panic!("cynotch.exe did not exit within 8s after WM_CLOSE");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn subprocess_normal_mode_creates_backdrop_and_exits_cleanly() {
    let _runtime_guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempLocalAppData::new("sub_norm", true);
    spawn_cynotch_and_wait_windows(&tmp, &[], &[], true);
}

#[test]
fn subprocess_no_frost_flag_skips_backdrop_and_exits_cleanly() {
    let _runtime_guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempLocalAppData::new("sub_flag", true);
    spawn_cynotch_and_wait_windows(&tmp, &["--no-frost"], &[], false);
}

#[test]
fn subprocess_env_no_frost_skips_backdrop_and_exits_cleanly() {
    let _runtime_guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempLocalAppData::new("sub_env", true);
    spawn_cynotch_and_wait_windows(&tmp, &[], &[("CYNOTCH_NO_FROST", "1")], false);
}
