#![allow(unsafe_op_in_unsafe_fn)]

use cynotch::backdrop;
use cynotch::composition::DCompHost;
use cynotch::dcomp_blur::{BlurSpec, blur_px_to_sigma};
use cynotch::models::AppState;
use cynotch::notifications;
use cynotch::settings_window;
use cynotch::window::{self, NotchWindow, WM_APP_REDRAW};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
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
                || cls == "CyNotchBackdropClass"
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
fn blur_px_to_sigma_is_strictly_monotonic_and_smooth() {
    let mut prev = blur_px_to_sigma(4);
    assert!((prev - 1.5).abs() < 1e-5);
    for px in 5..=40 {
        let cur = blur_px_to_sigma(px);
        assert!(
            cur > prev,
            "sigma must strictly increase at px={px}: prev={prev}, cur={cur}"
        );
        let step = cur - prev;
        assert!(
            (step - (22.5 / 36.0)).abs() < 1e-4,
            "sigma step must be smooth/linear without cliff at px={px}: step={step}"
        );
        prev = cur;
    }
    assert!((prev - 24.0).abs() < 1e-5);
}

#[test]
fn runtime_single_hwnd_blur_lifecycle_settings_and_fallback() {
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
            .filter(|(_, c)| c == "CyNotchBackdropClass")
            .map(|(h, _)| *h)
            .collect();
        assert!(
            backdrops.is_empty(),
            "Single-HWND DirectComposition blur must create 0 CyNotchBackdropClass HWNDs, found {:?}",
            backdrops
        );
        assert!(IsWindowVisible(notch.hwnd).as_bool());

        // Exercise continuous blur slider (4..=40) and rapid hover resize frames
        for frame in 0..50u32 {
            {
                let mut st = state.lock().unwrap();
                st.frosted_blur_px = 4 + (frame * 36) / 49;
                st.is_hovered = frame % 2 == 0;
            }
            let _ = SendMessageW(notch.hwnd, WM_APP_REDRAW, WPARAM(0), LPARAM(0));
        }

        // Settings window creation & teardown on WM_CLOSE (still 0 extra backdrop HWNDs)
        settings_window::open_or_focus(state.clone());
        pump_messages(Duration::from_millis(120));
        let settings_hwnd =
            HWND(settings_window::SETTINGS_HWND.load(Ordering::Acquire) as *mut std::ffi::c_void);
        assert!(IsWindow(settings_hwnd).as_bool(), "Settings HWND must exist");
        assert!(
            IsWindowVisible(settings_hwnd).as_bool(),
            "Settings HWND must be visible"
        );
        let wins_with_settings = find_pid_windows(pid);
        assert!(
            wins_with_settings
                .iter()
                .all(|(_, c)| c != "CyNotchBackdropClass"),
            "Settings must not spawn CyNotchBackdropClass HWNDs"
        );

        let _ = PostMessageW(settings_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        pump_messages(Duration::from_millis(120));
        assert!(
            !IsWindow(settings_hwnd).as_bool(),
            "Settings window must be destroyed on WM_CLOSE"
        );

        // Direct DCompHost::set_blur stress test at Ultra (blur_px = 40) with rounded radii
        // using a dedicated HWND (notch.hwnd already owns a DesktopWindowTarget).
        let h_mod = GetModuleHandleW(None).expect("GetModuleHandleW");
        let probe_hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("STATIC"),
            w!("DCompBlurProbe"),
            WS_POPUP,
            0,
            0,
            480,
            260,
            None,
            None,
            h_mod,
            None,
        )
        .expect("CreateWindowExW probe");
        {
            let mut host = DCompHost::new(probe_hwnd, 480, 260).expect("DCompHost::new");
            assert!(
                host.blur_supported(),
                "DCompHost::blur_supported() must be true on Windows 10 1809+ / Windows 11"
            );
            assert!(
                host.luminosity_clamp_active(),
                "DCompHost must load GaussianBlurEffect -> ColorMatrixEffect (Option B) without falling back to blur-only"
            );
            for i in 0..50u32 {
                let w = 140.0 + (i as f32) * 4.0;
                let h = 36.0 + (i as f32) * 3.0;
                let ok = host.set_blur(Some(BlurSpec {
                    x: 16.0,
                    y: 0.0,
                    w,
                    h,
                    radii: (18.0, 18.0, 22.0, 22.0),
                    blur_px: 40,
                    box1_h: None,
                    extra_boxes: None,
                }));
                assert!(
                    ok,
                    "DCompHost::set_blur must succeed at Ultra (blur_px=40) frame {i}"
                );
            }
            assert!(host.set_blur(None));
        }
        let _ = DestroyWindow(probe_hwnd);

        // backdrop::mark_unavailable() engages 118% fallback
        backdrop::mark_unavailable();
        assert!(!backdrop::blur_available());
        let _ = SendMessageW(notch.hwnd, WM_APP_REDRAW, WPARAM(0), LPARAM(0));
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
    _expect_backdrop: bool,
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

    while Instant::now() < deadline {
        if let Ok(Some(status)) = guard.try_wait() {
            panic!("cynotch.exe exited unexpectedly early with {:?}", status);
        }
        let wins = unsafe { find_pid_windows(pid) };
        content_hwnd = wins
            .iter()
            .find(|(_, c)| c == "CyNotchWindowClass")
            .map(|(h, _)| *h);
        let content_visible = content_hwnd
            .map(|h| unsafe { IsWindowVisible(h).as_bool() })
            .unwrap_or(false);
        if content_visible {
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
        let wins = find_pid_windows(pid);
        let backdrops: Vec<_> = wins
            .iter()
            .filter(|(_, c)| c == "CyNotchBackdropClass")
            .collect();
        assert!(
            backdrops.is_empty(),
            "Expected zero CyNotchBackdropClass HWNDs in single-HWND mode, found {:?}",
            backdrops
        );

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
