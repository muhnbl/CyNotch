//! System-wide Windows 11 DWM border removal (`DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`).
//! Applies across Explorer, browsers, terminals, and all visible top-level windows.

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CLOAKED, DWMWA_COLOR_DEFAULT, DWMWA_COLOR_NONE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MOVESIZEEND, EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetMessageW, GetWindowLongPtrW,
    IsWindow, IsWindowVisible, KillTimer, MSG, OBJID_WINDOW, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetTimer, TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WM_QUIT, WM_TIMER, WS_CAPTION, WS_EX_TOOLWINDOW, WS_THICKFRAME,
};

type HWINEVENTHOOK = *mut c_void;
type WINEVENTPROC = unsafe extern "system" fn(
    h_win_event_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    id_event_thread: u32,
    dwms_event_time: u32,
);

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWinEventHook(
        event_min: u32,
        event_max: u32,
        hmod_win_event_proc: *mut c_void,
        lpfn_win_event_proc: WINEVENTPROC,
        id_process: u32,
        id_thread: u32,
        dw_flags: u32,
    ) -> HWINEVENTHOOK;
    fn UnhookWinEvent(h_win_event_hook: HWINEVENTHOOK) -> BOOL;
}

struct BorderWorkerHandle {
    tid: u32,
    join: JoinHandle<()>,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static WORKER: Mutex<Option<BorderWorkerHandle>> = Mutex::new(None);

/// Sets `DWMWA_BORDER_COLOR` on a single HWND to `DWMWA_COLOR_NONE` (`0xFFFFFFFE`).
pub fn set_window_borderless(hwnd: HWND) {
    unsafe {
        let color_none: u32 = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color_none as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
}

/// Sets `DWMWA_WINDOW_CORNER_PREFERENCE` to `DWMWCP_ROUND` for crisp DWM Acrylic corners.
pub fn set_window_rounded_corners(hwnd: HWND) {
    unsafe {
        let pref: i32 = DWMWCP_ROUND.0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
    }
}

unsafe fn is_eligible_window(hwnd: HWND) -> bool {
    unsafe {
        if hwnd.0.is_null() || !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        if (style & (WS_CAPTION.0 | WS_THICKFRAME.0)) == 0 {
            return false;
        }
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
            return false;
        }
        let mut cloaked: u32 = 0;
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        );
        if hr.is_ok() && cloaked != 0 {
            return false;
        }
        true
    }
}

unsafe fn apply_border_color(hwnd: HWND, color: u32) {
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
}

unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        if is_eligible_window(hwnd) {
            apply_border_color(hwnd, lparam.0 as u32);
        }
        BOOL(1)
    }
}

unsafe extern "system" fn win_event_cb(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    unsafe {
        if ACTIVE.load(Ordering::Acquire)
            && id_object == OBJID_WINDOW.0
            && id_child == 0
            && !hwnd.0.is_null()
            && is_eligible_window(hwnd)
        {
            apply_border_color(hwnd, DWMWA_COLOR_NONE);
        }
    }
}

fn apply_all(color: u32) {
    unsafe {
        let _ = EnumWindows(Some(enum_windows_cb), LPARAM(color as isize));
    }
}

/// Enables or disables system-wide Windows app border removal.
/// Serialized via `WORKER` mutex so rapid enable/disable toggles never orphan threads
/// or race `PostThreadMessageW` before the worker message queue exists.
pub fn set_enabled(enabled: bool) {
    let mut slot = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if enabled {
        if slot.is_some() {
            ACTIVE.store(true, Ordering::Release);
            apply_all(DWMWA_COLOR_NONE);
            return;
        }
        ACTIVE.store(true, Ordering::Release);
        let ready = Arc::new((Mutex::new(None::<u32>), Condvar::new()));
        let ready_bg = Arc::clone(&ready);

        if let Ok(join) = std::thread::Builder::new()
            .name("cynotch-borders".to_string())
            .spawn(move || unsafe {
                // Force creation of this thread's Win32 message queue BEFORE publishing TID.
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                let tid = GetCurrentThreadId();
                {
                    let (lock, cvar) = &*ready_bg;
                    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some(tid);
                    cvar.notify_all();
                }

                if !ACTIVE.load(Ordering::Acquire) {
                    return;
                }

                apply_all(DWMWA_COLOR_NONE);

                let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
                let hook_show = SetWinEventHook(
                    EVENT_OBJECT_CREATE,
                    EVENT_OBJECT_SHOW,
                    std::ptr::null_mut(),
                    win_event_cb,
                    0,
                    0,
                    flags,
                );
                let hook_fg = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_MOVESIZEEND,
                    std::ptr::null_mut(),
                    win_event_cb,
                    0,
                    0,
                    flags,
                );
                let timer_id = SetTimer(None, 1, 30_000, None);

                while ACTIVE.load(Ordering::Acquire)
                    && GetMessageW(&mut msg, None, 0, 0).as_bool()
                {
                    if !ACTIVE.load(Ordering::Acquire) {
                        break;
                    }
                    if msg.message == WM_TIMER {
                        apply_all(DWMWA_COLOR_NONE);
                    } else {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                if timer_id != 0 {
                    let _ = KillTimer(None, timer_id);
                }
                if !hook_show.is_null() {
                    let _ = UnhookWinEvent(hook_show);
                }
                if !hook_fg.is_null() {
                    let _ = UnhookWinEvent(hook_fg);
                }
            })
        {
            let (lock, cvar) = &*ready;
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            while guard.is_none() {
                guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
            let tid = guard.unwrap_or(0);
            *slot = Some(BorderWorkerHandle { tid, join });
        }
    } else {
        let was_active = ACTIVE.swap(false, Ordering::AcqRel);
        if let Some(handle) = slot.take() {
            if handle.tid != 0 {
                unsafe {
                    let _ = PostThreadMessageW(handle.tid, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
            let _ = handle.join.join();
        }
        if was_active {
            apply_all(DWMWA_COLOR_DEFAULT);
        }
    }
}
