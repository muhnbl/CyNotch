#![allow(unsafe_op_in_unsafe_fn)]

use crate::audio::set_master_volume;
use crate::backdrop::{self, Backdrop};
use crate::backdrop_geom::{self, ShellGeom};
use crate::capture::{
    capture_foreground_window_rect, copy_text_to_clipboard, grab_full_desktop_frame,
    grab_rect_frame, pictures_capture_dir, read_clipboard_text, save_frame, shell_open_path,
};
use crate::composition::DCompHost;
use crate::media::{next_track, prev_track, seek_to_position_ms, toggle_play_pause};
use crate::memreduct::execute_memory_clean_areas;
use crate::models::{
    Action, AppState, NotchMode, NotchPlacement, NotchStyle, NotificationKind, SettingEffects,
    ShelfKind,
};
use crate::notifications;
use crate::renderer::Renderer;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_TRANSITIONS_FORCEDISABLED,
    DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(u_period: u32) -> u32;
    fn timeEndPeriod(u_period: u32) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(utype: u32) -> i32;
    fn AddClipboardFormatListener(hwnd: HWND) -> i32;
    fn RemoveClipboardFormatListener(hwnd: HWND) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn DragAcceptFiles(hwnd: HWND, f_accept: i32);
    fn DragQueryFileW(h_drop: isize, i_file: u32, lpsz_file: *mut u16, cch: u32) -> u32;
    fn DragFinish(h_drop: isize);
}

const CLASS_NAME: PCWSTR = w!("CyNotchWindowClass");
const WINDOW_NAME: PCWSTR = w!("CyNotch");
const TIMER_ANIMATION_ID: usize = 1;
const TIMER_POLL_ID: usize = 2;
const TIMER_HOVER_OPEN_ID: usize = 3;
const WM_MOUSELEAVE_CODE: u32 = 0x02A3;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_DROPFILES_CODE: u32 = 0x0233;

pub const WM_APP_EXPAND_NOTCH: u32 = 0x8000 + 1;
pub const WM_APP_TOGGLE_NOTCH: u32 = 0x8000 + 2;
pub const WM_TRAY_ICON: u32 = 0x8000 + 3;
pub const WM_APP_REDRAW: u32 = 0x8000 + 4;
pub const WM_APP_SHOW_NOACTIVATE: u32 = 0x8000 + 5;
pub const WM_APP_TRIGGER_PURGE: u32 = 0x8000 + 6;
pub const WM_APP_QUIT: u32 = 0x8000 + 7;
pub const WM_APP_SYNC_FROSTED: u32 = 0x8000 + 8;
pub const WM_APP_UPDATE_TRAY: u32 = 0x8000 + 9;
pub const WM_APP_HIGHRES_ANIM_TICK: u32 = 0x8000 + 10;

const HOTKEY_TOGGLE_NOTCH: i32 = 1001;
const HOTKEY_PURGE_RAM: i32 = 1002;
const HOTKEY_CAPTURE_SCREEN: i32 = 1003;

const MENU_TOGGLE: usize = 2001;
const MENU_PURGE: usize = 2002;
const MENU_CAPTURE: usize = 2003;
const MENU_PIN: usize = 2004;
const MENU_SETTINGS: usize = 2005;
const MENU_EXIT: usize = 2006;
const MENU_MINIMIZE: usize = 2007;

pub(crate) unsafe fn load_app_icon(size: i32) -> HICON {
    if let Ok(h_mod) = GetModuleHandleW(None) {
        let h_inst: HINSTANCE = h_mod.into();
        if let Ok(handle) = LoadImageW(
            h_inst,
            PCWSTR(std::ptr::without_provenance::<u16>(1)),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR | LR_SHARED,
        ) && !handle.0.is_null()
        {
            return HICON(handle.0);
        }
    }
    LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
}

pub static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
pub static PURGE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
pub static CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
pub static IS_DRAGGING_VOLUME: AtomicBool = AtomicBool::new(false);
pub static IGNORE_NEXT_CLIPBOARD: AtomicBool = AtomicBool::new(false);
pub static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static NOTIF_SYNC_REQUESTED: AtomicBool = AtomicBool::new(false);
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
static HIGH_RES_SLOTS: AtomicU32 = AtomicU32::new(0);

pub fn set_high_res_slot(bit: u32, active: bool) {
    let mask = 1u32 << bit;
    let mut cur = HIGH_RES_SLOTS.load(Ordering::Acquire);
    loop {
        let next = if active { cur | mask } else { cur & !mask };
        if next == cur {
            return;
        }
        match HIGH_RES_SLOTS.compare_exchange(cur, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(prev) => {
                unsafe {
                    if prev == 0 && next != 0 {
                        let _ = timeBeginPeriod(1);
                    } else if prev != 0 && next == 0 {
                        let _ = timeEndPeriod(1);
                    }
                }
                return;
            }
            Err(observed) => cur = observed,
        }
    }
}

struct PacerSlot {
    hwnd: isize,
    msg: u32,
    interval_ms: u32,
    in_flight: bool,
    last_post: Option<std::time::Instant>,
    next_due: std::time::Instant,
}

impl PacerSlot {
    fn new(msg: u32) -> Self {
        Self {
            hwnd: 0,
            msg,
            interval_ms: 0,
            in_flight: false,
            last_post: None,
            next_due: std::time::Instant::now(),
        }
    }
}

struct HighResPacerState {
    notch: PacerSlot,
    settings: PacerSlot,
}

static PACER: std::sync::OnceLock<(Mutex<HighResPacerState>, std::sync::Condvar)> =
    std::sync::OnceLock::new();

fn ensure_pacer() -> &'static (Mutex<HighResPacerState>, std::sync::Condvar) {
    PACER.get_or_init(|| {
        let pair = (
            Mutex::new(HighResPacerState {
                notch: PacerSlot::new(WM_APP_HIGHRES_ANIM_TICK),
                settings: PacerSlot::new(crate::settings_window::WM_APP_SETTINGS_HIGHRES_TICK),
            }),
            std::sync::Condvar::new(),
        );
        let _ = std::thread::Builder::new()
            .name("cynotch-highres-pacer".to_string())
            .spawn(run_highres_pacer_loop);
        pair
    })
}

fn run_highres_pacer_loop() {
    use windows::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, SetWaitableTimer,
        TIMER_ALL_ACCESS, WaitForSingleObject,
    };
    let timer = unsafe {
        CreateWaitableTimerExW(
            None,
            None,
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS.0,
        )
        .ok()
    };
    let (lock, cv) = ensure_pacer();
    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    while !SHUTDOWN_REQUESTED.load(Ordering::Acquire) {
        let now = std::time::Instant::now();
        let mut to_post: [(isize, u32, usize); 2] = [(0, 0, 0), (0, 0, 1)];
        let mut post_len = 0usize;
        let mut min_wait: Option<std::time::Duration> = None;

        for idx in 0..2 {
            let slot = if idx == 0 {
                &mut guard.notch
            } else {
                &mut guard.settings
            };
            if slot.hwnd != 0 && slot.interval_ms > 0 {
                if slot.in_flight
                    && let Some(lp) = slot.last_post
                    && now.duration_since(lp) >= std::time::Duration::from_millis(150)
                {
                    slot.in_flight = false;
                }
                if !slot.in_flight {
                    if now >= slot.next_due {
                        slot.in_flight = true;
                        slot.last_post = Some(now);
                        let step = std::time::Duration::from_millis(slot.interval_ms as u64);
                        let next = slot.next_due + step;
                        slot.next_due = if next > now { next } else { now + step };
                        to_post[post_len] = (slot.hwnd, slot.msg, idx);
                        post_len += 1;
                    } else {
                        let rem = slot.next_due - now;
                        min_wait = Some(min_wait.map_or(rem, |m| m.min(rem)));
                    }
                } else {
                    let step = std::time::Duration::from_millis(slot.interval_ms as u64);
                    min_wait = Some(min_wait.map_or(step, |m| m.min(step)));
                }
            }
        }

        if post_len > 0 {
            drop(guard);
            let mut failed = [(false, 0isize); 2];
            for i in 0..post_len {
                let (hwnd_raw, msg, idx) = to_post[i];
                unsafe {
                    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
                    if !IsWindow(hwnd).as_bool()
                        || PostMessageW(hwnd, msg, WPARAM(0), LPARAM(0)).is_err()
                    {
                        failed[idx] = (true, hwnd_raw);
                    }
                }
            }
            guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            for (idx, (did_fail, hwnd_raw)) in failed.into_iter().enumerate() {
                if did_fail {
                    let slot = if idx == 0 {
                        &mut guard.notch
                    } else {
                        &mut guard.settings
                    };
                    if slot.hwnd == hwnd_raw {
                        slot.in_flight = false;
                        slot.interval_ms = 0;
                    }
                }
            }
            continue;
        }

        match min_wait {
            None => {
                guard = cv.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
            Some(dur) => {
                if let Some(ht) = timer {
                    drop(guard);
                    let due_100ns = -((dur.as_nanos() / 100).max(1_000) as i64);
                    unsafe {
                        if SetWaitableTimer(ht, &due_100ns, 0, None, None, false).is_ok() {
                            let _ = WaitForSingleObject(ht, 50);
                        } else {
                            std::thread::sleep(dur);
                        }
                    }
                    guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                } else {
                    let (g, _) = cv
                        .wait_timeout(guard, dur)
                        .unwrap_or_else(|e| e.into_inner());
                    guard = g;
                }
            }
        }
    }
    if let Some(ht) = timer {
        unsafe {
            let _ = CloseHandle(ht);
        }
    }
}

fn set_pacer_slot(is_settings: bool, hwnd: HWND, msg: u32, interval_ms: u32) {
    let hwnd_raw = hwnd.0 as isize;
    if interval_ms == 0 || hwnd_raw == 0 {
        if let Some((lock, cv)) = PACER.get()
            && let Ok(mut g) = lock.lock()
        {
            let slot = if is_settings {
                &mut g.settings
            } else {
                &mut g.notch
            };
            if hwnd_raw == 0 || slot.hwnd == hwnd_raw || slot.hwnd == 0 {
                slot.interval_ms = 0;
                slot.in_flight = false;
                slot.last_post = None;
                if hwnd_raw == 0 {
                    slot.hwnd = 0;
                }
                cv.notify_all();
            }
        }
        return;
    }
    let (lock, cv) = ensure_pacer();
    if let Ok(mut g) = lock.lock() {
        let slot = if is_settings {
            &mut g.settings
        } else {
            &mut g.notch
        };
        let changed = slot.hwnd != hwnd_raw || slot.interval_ms != interval_ms;
        slot.hwnd = hwnd_raw;
        slot.msg = msg;
        slot.interval_ms = interval_ms;
        if changed {
            slot.in_flight = false;
            slot.last_post = None;
            slot.next_due =
                std::time::Instant::now() + std::time::Duration::from_millis(interval_ms as u64);
        }
        cv.notify_all();
    }
}

pub fn set_notch_pacer(hwnd: HWND, interval_ms: u32) {
    set_pacer_slot(false, hwnd, WM_APP_HIGHRES_ANIM_TICK, interval_ms);
}

pub fn set_settings_pacer(hwnd: HWND, interval_ms: u32) {
    set_pacer_slot(
        true,
        hwnd,
        crate::settings_window::WM_APP_SETTINGS_HIGHRES_TICK,
        interval_ms,
    );
}

pub fn clear_notch_pacer_in_flight() {
    if let Some((lock, cv)) = PACER.get()
        && let Ok(mut g) = lock.lock()
    {
        g.notch.in_flight = false;
        cv.notify_all();
    }
}

pub fn clear_settings_pacer_in_flight() {
    if let Some((lock, cv)) = PACER.get()
        && let Ok(mut g) = lock.lock()
    {
        g.settings.in_flight = false;
        cv.notify_all();
    }
}

pub fn detect_monitor_hz() -> u32 {
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm).as_bool()
            && dm.dmDisplayFrequency >= 30
        {
            return dm.dmDisplayFrequency.clamp(60, 240);
        }
    }
    60
}

pub fn request_purge() {
    let raw = MAIN_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_TRIGGER_PURGE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn request_quit() {
    let raw = MAIN_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn post_redraw() {
    let raw = MAIN_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_REDRAW,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn post_update_tray() {
    let raw = MAIN_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_UPDATE_TRAY,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn is_fullscreen_suppressing_notifications(
    hide_in_fullscreen: bool,
    notif_over_fullscreen: bool,
) -> bool {
    if !hide_in_fullscreen || notif_over_fullscreen {
        return false;
    }
    let raw = MAIN_HWND.load(Ordering::Acquire);
    if raw == 0 {
        return false;
    }
    unsafe { is_foreign_fullscreen_active(HWND(raw as *mut std::ffi::c_void)) }
}

fn play_chime_async(_utype: u32) {
    if let Some(sound) = get_global_state()
        .and_then(|m| m.lock().ok().map(|s| s.notification_sound))
    {
        crate::sound::play_sound(sound);
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    #[default]
    None,
    Grip {
        start_cursor_x: i32,
        start_cursor_y: i32,
        start_offset: i32,
    },
    IdlePressOrDrag {
        start_cursor_x: i32,
        start_cursor_y: i32,
        start_offset: i32,
        moved: bool,
    },
    VolumeSlider,
    MusicScrubber {
        moved: bool,
    },
}

unsafe fn get_monitor_rect(hwnd: HWND) -> RECT {
    let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if !hmon.0.is_null() {
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return mi.rcMonitor;
        }
    }
    RECT {
        left: 0,
        top: 0,
        right: GetSystemMetrics(SM_CXSCREEN).max(1280),
        bottom: GetSystemMetrics(SM_CYSCREEN).max(720),
    }
}

fn floating_edge_gap(state: &AppState) -> i32 {
    if state.is_corner_docked() {
        return 0;
    }
    match state.notch_style {
        NotchStyle::EdgeFlush => 0,
        NotchStyle::Floating => {
            if state.minimized_island {
                3
            } else {
                10
            }
        }
    }
}

fn clamp_offset_for_mon(
    mon: &RECT,
    state: &AppState,
    canvas_w: i32,
    canvas_h: i32,
    raw: i32,
) -> i32 {
    let mon_w = (mon.right - mon.left).max(320);
    let mon_h = (mon.bottom - mon.top).max(240);
    let margin = 8;
    match state.placement {
        NotchPlacement::Top => {
            let max_off = ((mon_w - canvas_w) / 2 - margin).max(0);
            raw.clamp(-max_off, max_off)
        }
        NotchPlacement::Left | NotchPlacement::Right => {
            let max_off = ((mon_h - canvas_h) / 2 - margin).max(0);
            raw.clamp(-max_off, max_off)
        }
    }
}

fn compute_window_pos(mon: &RECT, state: &AppState, canvas_w: i32, canvas_h: i32) -> (i32, i32) {
    if state.is_top_left_corner_docked() {
        return (mon.left, mon.top);
    }
    if state.is_top_right_corner_docked() {
        return (mon.right - canvas_w, mon.top);
    }
    let gap = floating_edge_gap(state);
    let mon_w = (mon.right - mon.left).max(canvas_w);
    let mon_h = (mon.bottom - mon.top).max(canvas_h);
    match state.placement {
        NotchPlacement::Top => {
            let off = clamp_offset_for_mon(mon, state, canvas_w, canvas_h, state.x_offset);
            let min_x = mon.left + 4;
            let max_x = (mon.right - canvas_w - 4).max(min_x);
            let x = (mon.left + (mon_w - canvas_w) / 2 + off).clamp(min_x, max_x);
            let y = mon.top + gap;
            (x, y)
        }
        NotchPlacement::Left => {
            let off = clamp_offset_for_mon(mon, state, canvas_w, canvas_h, state.y_offset);
            let min_y = mon.top + 4;
            let max_y = (mon.bottom - canvas_h - 4).max(min_y);
            let x = mon.left + gap;
            let y = (mon.top + (mon_h - canvas_h) / 2 + off).clamp(min_y, max_y);
            (x, y)
        }
        NotchPlacement::Right => {
            let off = clamp_offset_for_mon(mon, state, canvas_w, canvas_h, state.y_offset);
            let min_y = mon.top + 4;
            let max_y = (mon.bottom - canvas_h - 4).max(min_y);
            let x = mon.right - canvas_w - gap;
            let y = (mon.top + (mon_h - canvas_h) / 2 + off).clamp(min_y, max_y);
            (x, y)
        }
    }
}

pub fn is_idle_collapsed(state: &AppState) -> bool {
    !state.is_hovered
        && !state.is_hover_pending
        && state.anim_edge_glow_t <= 0.003
        && state.notification.is_none()
        && state.needs_animation_timer_ms().is_none()
        && (state.current_width - state.idle_width()).abs() <= 0.5
        && (state.current_height - state.idle_height()).abs() <= 0.5
}

fn compute_active_window_layout(
    mon: &RECT,
    state: &AppState,
    compact_idle: bool,
) -> (i32, i32, i32, i32) {
    let full_w = state.canvas_width().max(1) as i32;
    let full_h = state.canvas_height().max(1) as i32;
    let (full_x, full_y) = compute_window_pos(mon, state, full_w, full_h);
    if !compact_idle {
        return (full_x, full_y, full_w, full_h);
    }

    let pad = 12.0f32;
    let (cw, ch) = match state.placement {
        NotchPlacement::Top => (
            (state.idle_width() + 2.0 * pad).ceil().max(1.0) as i32,
            (state.idle_height() + pad).ceil().max(1.0) as i32,
        ),
        NotchPlacement::Left | NotchPlacement::Right => (
            (state.idle_width() + pad).ceil().max(1.0) as i32,
            (state.idle_height() + 2.0 * pad).ceil().max(1.0) as i32,
        ),
    };
    let cw = cw.min(full_w);
    let ch = ch.min(full_h);
    let (full_ox, full_oy) = Renderer::compute_origin(state, full_w as f32, full_h as f32);
    let (comp_ox, comp_oy) = Renderer::compute_origin(state, cw as f32, ch as f32);
    let win_x = full_x + (full_ox - comp_ox).round() as i32;
    let win_y = full_y + (full_oy - comp_oy).round() as i32;
    (win_x, win_y, cw, ch)
}

/// Applies responsive edge/corner/center snapping during drag.
/// Returns `(did_snap, placement_changed)` so discrete jumps glide smoothly rather than teleporting.
fn apply_responsive_drag_snap(
    mon: &RECT,
    state: &mut AppState,
    pt: POINT,
    start_cursor_x: i32,
    start_cursor_y: i32,
    start_offset: i32,
) -> (bool, bool) {
    let mon_w = (mon.right - mon.left).max(320);
    let mon_h = (mon.bottom - mon.top).max(240);
    let prev_placement = state.placement;
    let prev_tl = state.is_top_left_corner_docked();
    let prev_tr = state.is_top_right_corner_docked();
    let prev_off = if state.placement.is_vertical() {
        state.y_offset
    } else {
        state.x_offset
    };
    let prev_center = prev_off == 0 && !prev_tl && !prev_tr;

    let side_edge_thresh = 68;
    let top_band_thresh = 104;
    let corner_pull_x = 135.min(mon_w / 5);
    let corner_top_y = 120;

    if pt.x <= mon.left + side_edge_thresh && pt.y > mon.top + corner_top_y {
        state.placement = NotchPlacement::Left;
    } else if pt.x >= mon.right - side_edge_thresh && pt.y > mon.top + corner_top_y {
        state.placement = NotchPlacement::Right;
    } else if state.placement.is_vertical()
        && pt.y <= mon.top + top_band_thresh
        && pt.x > mon.left + side_edge_thresh + 28
        && pt.x < mon.right - side_edge_thresh - 28
    {
        state.placement = NotchPlacement::Top;
    }

    let placement_changed = state.placement != prev_placement;
    if placement_changed {
        let axis_changed = prev_placement.is_vertical() != state.placement.is_vertical();
        if state.is_hovered {
            state.target_width = state.expanded_width();
            state.target_height = state.expanded_height();
        } else {
            state.sync_collapsed_targets();
        }
        if axis_changed {
            state.current_width = state.target_width;
            state.current_height = state.target_height;
        }
        let (dx, dw) = state.active_dock_target();
        state.anim_dock_x = dx;
        state.anim_dock_w = dw;
    }
    let active_cw = state.canvas_width() as i32;
    let active_ch = state.canvas_height() as i32;
    let margin = 8;

    match state.placement {
        NotchPlacement::Top => {
            let max_off = ((mon_w - active_cw) / 2 - margin).max(0);
            let raw = if placement_changed {
                pt.x - (mon.left + mon_w / 2)
            } else {
                start_offset + (pt.x - start_cursor_x)
            };

            let enter_tl = !prev_tl
                && pt.y <= mon.top + corner_top_y
                && (raw <= -(max_off + 18) || pt.x <= mon.left + corner_pull_x);
            let stay_tl = prev_tl && pt.y <= mon.top + corner_top_y && raw <= -(max_off - 28);

            let enter_tr = !prev_tr
                && pt.y <= mon.top + corner_top_y
                && (raw >= max_off + 18 || pt.x >= mon.right - corner_pull_x);
            let stay_tr = prev_tr && pt.y <= mon.top + corner_top_y && raw >= max_off - 28;

            if enter_tl || stay_tl {
                state.x_offset = -max_off;
                state.target_anchor_x = 0.0;
                state.target_anchor_y = 0.0;
            } else if enter_tr || stay_tr {
                state.x_offset = max_off;
                state.target_anchor_x = 1.0;
                state.target_anchor_y = 0.0;
            } else {
                let clamped = raw.clamp(-max_off, max_off);
                let center_snap = if prev_center { 18 } else { 12 };
                state.x_offset = if clamped.abs() <= center_snap {
                    0
                } else {
                    clamped
                };
                state.target_anchor_x = 0.5;
                state.target_anchor_y = 0.5;
            }
        }
        NotchPlacement::Left | NotchPlacement::Right => {
            let max_off = ((mon_h - active_ch) / 2 - margin).max(0);
            let raw = if placement_changed {
                pt.y - (mon.top + mon_h / 2)
            } else {
                start_offset + (pt.y - start_cursor_y)
            };
            let was_top_corner = prev_tl || prev_tr;
            let enter_top = !was_top_corner && (raw <= -(max_off + 18) || pt.y <= mon.top + 88);
            let stay_top = was_top_corner && raw <= -(max_off - 28);

            if enter_top || stay_top {
                state.y_offset = -max_off;
                state.target_anchor_x = if state.placement == NotchPlacement::Left {
                    0.0
                } else {
                    1.0
                };
                state.target_anchor_y = 0.0;
            } else {
                let clamped = raw.clamp(-max_off, max_off);
                let center_snap = if prev_center { 18 } else { 12 };
                state.y_offset = if clamped.abs() <= center_snap {
                    0
                } else {
                    clamped
                };
                state.target_anchor_x = 0.5;
                state.target_anchor_y = 0.5;
            }
        }
    }

    let cur_tl = state.is_top_left_corner_docked();
    let cur_tr = state.is_top_right_corner_docked();
    let cur_off = if state.placement.is_vertical() {
        state.y_offset
    } else {
        state.x_offset
    };
    let cur_center = cur_off == 0 && !cur_tl && !cur_tr;
    let did_snap =
        placement_changed || prev_tl != cur_tl || prev_tr != cur_tr || prev_center != cur_center;

    (did_snap, placement_changed)
}

pub struct NotchWindow {
    pub hwnd: HWND,
    pub state: Arc<Mutex<AppState>>,
}

static GLOBAL_STATE: Mutex<Option<Arc<Mutex<AppState>>>> = Mutex::new(None);

fn set_global_state(state: Arc<Mutex<AppState>>) {
    if let Ok(mut guard) = GLOBAL_STATE.lock() {
        *guard = Some(state);
    }
}

fn get_global_state() -> Option<Arc<Mutex<AppState>>> {
    GLOBAL_STATE.lock().ok().and_then(|guard| guard.clone())
}

#[derive(Default)]
struct NotchUi {
    renderer: Option<Renderer>,
    dcomp: Option<DCompHost>,
    is_tracking_mouse: bool,
    pointer_pos: Option<(f32, f32)>,
    hover_hit_idx: Option<usize>,
    drag_mode: DragMode,
    last_other_hwnd: Option<HWND>,
    anim_timer_interval_ms: u32,
    last_anim_instant: Option<std::time::Instant>,
    last_second_tick: Option<std::time::Instant>,
    hidden_for_fullscreen: bool,
    has_ulw_initialized: bool,
    dpi_generation: u64,
    prev_dib_w: i32,
    prev_dib_h: i32,
    last_win_x: i32,
    last_win_y: i32,
    anim_win_x: f32,
    anim_win_y: f32,
    win_pos_initialized: bool,
    snap_glide_active: bool,
    last_slider_redraw: Option<std::time::Instant>,
    last_rgn: Option<(i32, i32, i32, i32, i32, u8, u8, u8)>,
    idle_since: Option<std::time::Instant>,
    idle_caches_trimmed: bool,
    /// Paired frosted-blur window (see `backdrop.rs`). `None` until first needed.
    backdrop: Option<Backdrop>,
    backdrop_wanted: bool,
    backdrop_geom: Option<ShellGeom>,
    backdrop_inset: i32,
    /// Click-through physical screen-edge purple glow overlay (see `edge_glow.rs`).
    edge_glow: Option<crate::edge_glow::EdgeGlowOverlay>,
}

thread_local! {
    static BACKDROP_SYNCING: Cell<bool> = const { Cell::new(false) };
}

thread_local! {
    static NOTCH_UI: RefCell<NotchUi> = RefCell::new(NotchUi::default());
}

fn compute_window_region_spec_for_canvas(
    state: &AppState,
    cw: i32,
    ch: i32,
) -> (i32, i32, i32, i32, i32, u8, u8, u8) {
    let cw = cw.max(1);
    let ch = ch.max(1);
    let (ox, oy) = Renderer::compute_origin(state, cw as f32, ch as f32);
    let w = state.current_width.max(1.0);
    let h = state.current_height.max(1.0);
    let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();
    let max_r = r_tl.max(r_tr).max(r_br).max(r_bl);

    let x0 = (ox - 1.0).floor().clamp(0.0, cw as f32) as i32;
    let y0 = (oy - 1.0).floor().clamp(0.0, ch as f32) as i32;
    let x1 = (ox + w + 1.0)
        .ceil()
        .clamp((x0 + 1) as f32, cw.max(x0 + 1) as f32) as i32;
    let y1 = (oy + h + 1.0)
        .ceil()
        .clamp((y0 + 1) as f32, ch.max(y0 + 1) as f32) as i32;
    let max_d = (x1 - x0).min(y1 - y0).max(2);
    let diam = ((max_r * 2.0).round() as i32).clamp(2, max_d);
    let style_key = match state.notch_style {
        NotchStyle::Floating => 0,
        NotchStyle::EdgeFlush => 1,
    };
    let place_key = match state.placement {
        NotchPlacement::Top => 0,
        NotchPlacement::Left => 1,
        NotchPlacement::Right => 2,
    };
    let mut sharp_mask = 0u8;
    if r_tl < 1.0 {
        sharp_mask |= 1;
    }
    if r_tr < 1.0 {
        sharp_mask |= 2;
    }
    if r_br < 1.0 {
        sharp_mask |= 4;
    }
    if r_bl < 1.0 {
        sharp_mask |= 8;
    }
    (x0, y0, x1, y1, diam, style_key, place_key, sharp_mask)
}

/// Computes the exact Win32 window region parameters `(x0, y0, x1, y1, diam, style_key, place_key)`
/// in HWND client coordinates matching the active visible notch shell.
pub fn compute_window_region_spec(state: &AppState) -> (i32, i32, i32, i32, i32, u8, u8) {
    let s = compute_window_region_spec_for_canvas(
        state,
        state.canvas_width() as i32,
        state.canvas_height() as i32,
    );
    (s.0, s.1, s.2, s.3, s.4, s.5, s.6)
}

unsafe fn sync_notch_window_region(hwnd: HWND, state: &AppState, cw: i32, ch: i32) {
    let cw = cw.max(1);
    let ch = ch.max(1);
    let spec = compute_window_region_spec_for_canvas(state, cw, ch);
    let unchanged = NOTCH_UI
        .try_with(|cell| {
            cell.try_borrow()
                .map(|ui| ui.last_rgn == Some(spec))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if unchanged {
        return;
    }

    let (x0, y0, x1, y1, diam, style_key, place_key, sharp_mask) = spec;
    let x0 = (x0 - 2).max(0);
    let y0 = (y0 - 2).max(0);
    let x1 = (x1 + 2).min(cw);
    let y1 = (y1 + 2).min(ch);
    let rgn_diam = (diam + 4).clamp(2, (x1 - x0).min(y1 - y0).max(2));
    let hrgn = if style_key == 0 {
        CreateRoundRectRgn(x0, y0, x1, y1, rgn_diam, rgn_diam)
    } else {
        let ext = (rgn_diam / 2).max(4);
        let (rx0, ry0, rx1, ry1) = match place_key {
            0 => (x0, y0 - ext, x1, y1),
            1 => (x0 - ext, y0, x1, y1),
            _ => (x0, y0, x1 + ext, y1),
        };
        let r_round = CreateRoundRectRgn(rx0, ry0, rx1, ry1, rgn_diam, rgn_diam);
        if !r_round.is_invalid() {
            let r_box = CreateRectRgn(x0, y0, x1, y1);
            if !r_box.is_invalid() {
                let _ = CombineRgn(r_round, r_round, r_box, RGN_AND);
                let _ = DeleteObject(HGDIOBJ(r_box.0));
            }
        }
        r_round
    };

    if hrgn.is_invalid() {
        return;
    }
    if sharp_mask != 0 {
        let rad = ((diam + 1) / 2).max(2);
        let corners = [
            (1u8, x0, y0, (x0 + rad).min(x1), (y0 + rad).min(y1)),
            (2u8, (x1 - rad).max(x0), y0, x1, (y0 + rad).min(y1)),
            (4u8, (x1 - rad).max(x0), (y1 - rad).max(y0), x1, y1),
            (8u8, x0, (y1 - rad).max(y0), (x0 + rad).min(x1), y1),
        ];
        for (bit, cx0, cy0, cx1, cy1) in corners {
            if sharp_mask & bit != 0 && cx1 > cx0 && cy1 > cy0 {
                let patch = CreateRectRgn(cx0, cy0, cx1, cy1);
                if !patch.is_invalid() {
                    let _ = CombineRgn(hrgn, hrgn, patch, RGN_OR);
                    let _ = DeleteObject(HGDIOBJ(patch.0));
                }
            }
        }
    }
    if SetWindowRgn(hwnd, hrgn, false) != 0 {
        let _ = NOTCH_UI.try_with(|cell| {
            if let Ok(mut ui) = cell.try_borrow_mut() {
                ui.last_rgn = Some(spec);
            }
        });
    } else {
        let _ = DeleteObject(HGDIOBJ(hrgn.0));
    }
}

/// Visible-shell silhouette in content-window client coordinates, derived from
/// the same state snapshot (and canvas size) the renderer draws with, and clipped
/// to `[0, cw] x [0, ch]` so sliding banners never bleed onto adjacent monitors.
pub fn notch_backdrop_geom(state: &AppState, cw: i32, ch: i32) -> Option<ShellGeom> {
    let cw = cw.max(1);
    let ch = ch.max(1);
    let (ox, oy) = Renderer::compute_origin(state, cw as f32, ch as f32);
    let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();
    let max_r = r_tl.max(r_tr).max(r_br).max(r_bl);
    let mut sharp_mask = 0u8;
    if r_tl < 1.0 {
        sharp_mask |= 1;
    }
    if r_tr < 1.0 {
        sharp_mask |= 2;
    }
    if r_br < 1.0 {
        sharp_mask |= 4;
    }
    if r_bl < 1.0 {
        sharp_mask |= 8;
    }
    let flush = matches!(state.notch_style, NotchStyle::EdgeFlush);
    let place = match state.placement {
        NotchPlacement::Top => 0,
        NotchPlacement::Left => 1,
        NotchPlacement::Right => 2,
    };
    ShellGeom::from_shell_clipped(
        ox,
        oy,
        state.current_width.max(1.0),
        state.current_height.max(1.0),
        cw,
        ch,
        max_r,
        flush,
        place,
        sharp_mask,
    )
}

/// Keeps the paired blur window glued under the notch using the geometry cached
/// by the last `redraw_window`. Safe to call from any window message: it never
/// locks `AppState`, skips quietly if `NOTCH_UI` is mid-borrow, and ignores
/// re-entrant calls.
unsafe fn sync_notch_backdrop(hwnd: HWND, visible_override: Option<bool>) {
    if BACKDROP_SYNCING.get() {
        return;
    }
    BACKDROP_SYNCING.set(true);
    sync_notch_backdrop_inner(hwnd, visible_override);
    BACKDROP_SYNCING.set(false);
}

unsafe fn sync_notch_backdrop_inner(hwnd: HWND, visible_override: Option<bool>) {
    let snap = NOTCH_UI
        .try_with(|c| {
            c.try_borrow()
                .ok()
                .map(|ui| (ui.backdrop_wanted, ui.backdrop_geom, ui.backdrop_inset))
        })
        .ok()
        .flatten();
    let Some((wanted, geom, inset)) = snap else {
        return;
    };
    let visible = visible_override.unwrap_or_else(|| IsWindowVisible(hwnd).as_bool());
    let placement = if wanted && visible {
        geom.and_then(|g| {
            let mut rc = RECT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() {
                backdrop_geom::placement(rc.left, rc.top, &g, inset)
            } else {
                None
            }
        })
    } else {
        None
    };
    if placement.is_none() && !wanted {
        // Nothing to show: avoid creating a window just to hide it.
        let has = NOTCH_UI
            .try_with(|c| c.try_borrow().map(|ui| ui.backdrop.is_some()).unwrap_or(false))
            .unwrap_or(false);
        if !has {
            return;
        }
    }
    let taken = NOTCH_UI
        .try_with(|c| c.try_borrow_mut().ok().map(|mut ui| ui.backdrop.take()))
        .ok()
        .flatten();
    let Some(mut slot) = taken else {
        return;
    };
    let failed = backdrop::sync_slot(&mut slot, hwnd, placement);
    let _ = NOTCH_UI.try_with(|c| {
        if let Ok(mut ui) = c.try_borrow_mut() {
            ui.backdrop = slot;
        }
    });
    if failed {
        post_redraw();
    }
}

unsafe fn sync_animation_timer(hwnd: HWND, desired_ms: Option<u32>) {
    let target = desired_ms.unwrap_or(0);
    set_high_res_slot(1, target != 0 && target <= 30);
    let action = NOTCH_UI.with_borrow_mut(|ui| {
        if target == ui.anim_timer_interval_ms {
            None
        } else {
            let prev = ui.anim_timer_interval_ms;
            if target == 0 {
                ui.last_anim_instant = None;
            } else if prev == 0 {
                ui.last_anim_instant = Some(std::time::Instant::now());
            }
            ui.anim_timer_interval_ms = target;
            Some(target)
        }
    });
    if let Some(ms) = action {
        if ms == 0 {
            let _ = KillTimer(hwnd, TIMER_ANIMATION_ID);
            set_notch_pacer(hwnd, 0);
        } else if ms < 16 {
            let _ = KillTimer(hwnd, TIMER_ANIMATION_ID);
            set_notch_pacer(hwnd, ms);
        } else {
            set_notch_pacer(hwnd, 0);
            SetTimer(hwnd, TIMER_ANIMATION_ID, ms, None);
        }
    }
}

unsafe fn tick_notch_animation(hwnd: HWND) {
    let now = std::time::Instant::now();
    let dt = NOTCH_UI.with_borrow_mut(|ui| {
        let d = if let Some(prev) = ui.last_anim_instant {
            now.duration_since(prev).as_secs_f32().clamp(0.002, 0.05)
        } else {
            0.016
        };
        ui.last_anim_instant = Some(now);
        d
    });

    let mut needs_redraw = false;
    let mut desired_ms = None;
    let mut target_win_pos = None;
    let mut target_canvas = None;
    let mut active_interval = 16u32;
    let mut idle_collapsed = false;
    if let Some(state_mutex) = get_global_state()
        && let Ok(mut state) = state_mutex.lock()
    {
        needs_redraw = state.update_animation(dt);
        desired_ms = state.needs_animation_timer_ms();
        active_interval = state.desired_active_interval_ms();
        idle_collapsed = is_idle_collapsed(&state);
        let mon = get_monitor_rect(hwnd);
        let (wx, wy, cw, ch) = compute_active_window_layout(&mon, &state, idle_collapsed);
        target_win_pos = Some((wx, wy));
        target_canvas = Some((cw, ch));
    }

    let mut win_glide_still_active = false;
    let mut pos_when_no_redraw: Option<(i32, i32)> = None;
    if let Some((tx, ty)) = target_win_pos {
        NOTCH_UI.with_borrow_mut(|ui| {
            if !ui.win_pos_initialized {
                ui.anim_win_x = tx as f32;
                ui.anim_win_y = ty as f32;
                ui.win_pos_initialized = true;
            }
            if ui.snap_glide_active {
                let alpha = 1.0 - (-18.0_f32 * dt).exp();
                ui.anim_win_x += (tx as f32 - ui.anim_win_x) * alpha;
                ui.anim_win_y += (ty as f32 - ui.anim_win_y) * alpha;
                if (tx as f32 - ui.anim_win_x).abs() <= 0.65
                    && (ty as f32 - ui.anim_win_y).abs() <= 0.65
                {
                    ui.anim_win_x = tx as f32;
                    ui.anim_win_y = ty as f32;
                    ui.snap_glide_active = false;
                } else {
                    win_glide_still_active = true;
                }
                if !needs_redraw {
                    let rx = ui.anim_win_x.round() as i32;
                    let ry = ui.anim_win_y.round() as i32;
                    if rx != ui.last_win_x || ry != ui.last_win_y {
                        ui.last_win_x = rx;
                        ui.last_win_y = ry;
                        pos_when_no_redraw = Some((rx, ry));
                    }
                }
            }
        });
    }

    if let Some((wx, wy)) = pos_when_no_redraw {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            wx,
            wy,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    let effective_ms = if win_glide_still_active {
        Some(desired_ms.map_or(active_interval, |m| m.min(active_interval)))
    } else {
        desired_ms
    };
    sync_animation_timer(hwnd, effective_ms);
    let needs_compact_shrink = if !needs_redraw && idle_collapsed && !win_glide_still_active {
        target_canvas.is_some_and(|(cw, ch)| {
            NOTCH_UI.with_borrow(|ui| ui.prev_dib_w != cw || ui.prev_dib_h != ch)
        })
    } else {
        false
    };
    if needs_redraw || needs_compact_shrink {
        redraw_window(hwnd);
    }
}

/// Pure classifier distinguishing true Fullscreen / Borderless-Fullscreen apps from
/// normal or maximized desktop windows (including maximized windows with Auto-Hide Taskbar).
pub fn classify_foreign_fullscreen(
    quns_exclusive_or_presentation: bool,
    same_monitor: bool,
    covers_monitor_bounds: bool,
    style_bits: u32,
    ex_style_bits: u32,
    is_maximized: bool,
) -> bool {
    if !same_monitor {
        return false;
    }
    if quns_exclusive_or_presentation {
        return true;
    }
    if !covers_monitor_bounds {
        return false;
    }
    const WS_EX_TOOLWINDOW_BITS: u32 = 0x0000_0080;
    if (ex_style_bits & WS_EX_TOOLWINDOW_BITS) != 0 {
        return false;
    }
    const WS_CAPTION_BITS: u32 = 0x00C0_0000;
    const WS_THICKFRAME_BITS: u32 = 0x0004_0000;
    let has_caption = (style_bits & WS_CAPTION_BITS) == WS_CAPTION_BITS;
    let has_thickframe = (style_bits & WS_THICKFRAME_BITS) != 0;
    if is_maximized && (has_caption || has_thickframe) {
        return false;
    }
    true
}

unsafe fn is_foreign_fullscreen_active(notch_hwnd: HWND) -> bool {
    let fg = GetForegroundWindow();
    if fg.0.is_null() || fg == notch_hwnd || IsIconic(fg).as_bool() {
        return false;
    }
    let settings_raw = crate::settings_window::SETTINGS_HWND.load(Ordering::Acquire);
    if settings_raw != 0 && fg.0 as isize == settings_raw {
        return false;
    }
    let mut fg_pid: u32 = 0;
    GetWindowThreadProcessId(fg, Some(&mut fg_pid));
    if fg_pid != 0 && fg_pid == GetCurrentProcessId() {
        return false;
    }
    if !IsWindowVisible(fg).as_bool() {
        return false;
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(
        fg,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return false;
    }

    let mut cls_buf = [0u16; 64];
    let len = GetClassNameW(fg, &mut cls_buf);
    if len > 0 {
        let cls = &cls_buf[..len as usize];
        let is_shell_cls = [
            windows::core::w!("Progman").as_wide(),
            windows::core::w!("WorkerW").as_wide(),
            windows::core::w!("Shell_TrayWnd").as_wide(),
            windows::core::w!("Shell_SecondaryTrayWnd").as_wide(),
            windows::core::w!("Windows.UI.Core.CoreWindow").as_wide(),
            windows::core::w!("XamlExplorerHostIslandWindow").as_wide(),
            windows::core::w!("ApplicationManager_DesktopShellWindow").as_wide(),
            windows::core::w!("MultitaskingViewFrame").as_wide(),
            windows::core::w!("SearchHost").as_wide(),
            windows::core::w!("StartMenuExperienceHost").as_wide(),
            windows::core::w!("#32768").as_wide(),
            windows::core::w!("tooltips_class32").as_wide(),
        ]
        .contains(&cls);
        if is_shell_cls {
            return false;
        }
    }

    let notch_mon = MonitorFromWindow(notch_hwnd, MONITOR_DEFAULTTONEAREST);
    let fg_mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    if fg_mon.0.is_null() || (!notch_mon.0.is_null() && fg_mon != notch_mon) {
        return false;
    }

    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(fg_mon, &mut mi).as_bool() {
        return false;
    }
    let mr = mi.rcMonitor;

    let mut rc = RECT::default();
    let got_dwm = DwmGetWindowAttribute(
        fg,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rc as *mut RECT as *mut core::ffi::c_void,
        std::mem::size_of::<RECT>() as u32,
    )
    .is_ok();
    if !got_dwm && GetWindowRect(fg, &mut rc).is_err() {
        return false;
    }

    let covers_monitor =
        rc.left <= mr.left && rc.top <= mr.top && rc.right >= mr.right && rc.bottom >= mr.bottom;

    let quns_exclusive = SHQueryUserNotificationState()
        .map(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
        .unwrap_or(false);

    let style_bits = GetWindowLongW(fg, GWL_STYLE) as u32;
    let ex_style_bits = GetWindowLongW(fg, GWL_EXSTYLE) as u32;
    let is_maximized = IsZoomed(fg).as_bool();

    classify_foreign_fullscreen(
        quns_exclusive,
        true,
        covers_monitor,
        style_bits,
        ex_style_bits,
        is_maximized,
    )
}

unsafe fn ensure_topmost_zorder(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let mut needs_promote = (ex_style & WS_EX_TOPMOST.0) == 0;
    if !needs_promote {
        let settings_raw = crate::settings_window::SETTINGS_HWND.load(Ordering::Acquire);
        let mut cur = hwnd;
        let mut hops = 0u32;
        while hops < 16 {
            let Ok(prev) = GetWindow(cur, GW_HWNDPREV) else {
                break;
            };
            if prev.0.is_null() {
                break;
            }
            hops += 1;
            if prev.0 as isize != settings_raw && IsWindowVisible(prev).as_bool() {
                let mut cloaked: u32 = 0;
                let _ = DwmGetWindowAttribute(
                    prev,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut core::ffi::c_void,
                    std::mem::size_of::<u32>() as u32,
                );
                if cloaked == 0 {
                    let mut rc = RECT::default();
                    if GetWindowRect(prev, &mut rc).is_ok()
                        && (rc.right - rc.left) > 4
                        && (rc.bottom - rc.top) > 4
                    {
                        let mut cls_buf = [0u16; 64];
                        let clen = GetClassNameW(prev, &mut cls_buf);
                        let cls = if clen > 0 {
                            String::from_utf16_lossy(&cls_buf[..clen as usize])
                        } else {
                            String::new()
                        };
                        let transient_shell = matches!(
                            cls.as_str(),
                            "#32768"
                                | "tooltips_class32"
                                | "Shell_TrayWnd"
                                | "Shell_SecondaryTrayWnd"
                                | "Windows.UI.Core.CoreWindow"
                                | "XamlExplorerHostIslandWindow"
                                | "SearchHost"
                                | "StartMenuExperienceHost"
                                | backdrop::CLASS_NAME_STR
                                | crate::edge_glow::CLASS_NAME_STR
                        );
                        if !transient_shell {
                            needs_promote = true;
                            break;
                        }
                    }
                }
            }
            cur = prev;
        }
    }
    if needs_promote {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
        );
    }
}

unsafe fn attach_hwnd_resources(hwnd: HWND) {
    MAIN_HWND.store(hwnd.0 as isize, Ordering::Release);
    let force_disable: i32 = 1;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_TRANSITIONS_FORCEDISABLED,
        &force_disable as *const i32 as *const core::ffi::c_void,
        std::mem::size_of::<i32>() as u32,
    );
    crate::borders::set_window_borderless(hwnd);

    let _ = RegisterHotKey(
        hwnd,
        HOTKEY_TOGGLE_NOTCH,
        MOD_CONTROL | MOD_SHIFT,
        'C' as u32,
    );
    let _ = RegisterHotKey(hwnd, HOTKEY_PURGE_RAM, MOD_CONTROL | MOD_SHIFT, 'M' as u32);
    let _ = RegisterHotKey(
        hwnd,
        HOTKEY_CAPTURE_SCREEN,
        MOD_CONTROL | MOD_SHIFT,
        'S' as u32,
    );

    let _ = AddClipboardFormatListener(hwnd);
    DragAcceptFiles(hwnd, 1);

    add_tray_icon(hwnd);

    NOTCH_UI.with_borrow_mut(|ui| {
        ui.last_second_tick = Some(std::time::Instant::now());
    });
    SetTimer(hwnd, TIMER_POLL_ID, 250, None);
}

unsafe fn detach_hwnd_resources(hwnd: HWND) {
    let _ = KillTimer(hwnd, TIMER_ANIMATION_ID);
    let _ = KillTimer(hwnd, TIMER_POLL_ID);
    set_high_res_slot(1, false);
    remove_tray_icon(hwnd);
    let _ = RemoveClipboardFormatListener(hwnd);
    let _ = UnregisterHotKey(hwnd, HOTKEY_TOGGLE_NOTCH);
    let _ = UnregisterHotKey(hwnd, HOTKEY_PURGE_RAM);
    let _ = UnregisterHotKey(hwnd, HOTKEY_CAPTURE_SCREEN);
}

unsafe fn create_notch_hwnd(
    x: i32,
    y: i32,
    canvas_w: i32,
    canvas_h: i32,
    h_instance: HMODULE,
) -> Result<(HWND, DCompHost)> {
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        CLASS_NAME,
        WINDOW_NAME,
        WS_POPUP,
        x,
        y,
        canvas_w,
        canvas_h,
        None,
        None,
        h_instance,
        None,
    )?;
    let host = match DCompHost::new(hwnd, canvas_w.max(1) as u32, canvas_h.max(1) as u32) {
        Ok(h) => h,
        Err(e) => {
            // BH-23: Always clean up HWND if DComp initialization fails.
            let _ = DestroyWindow(hwnd);
            return Err(e);
        }
    };
    Ok((hwnd, host))
}

impl NotchWindow {
    pub fn create(state: Arc<Mutex<AppState>>) -> Result<Self> {
        let win_trans = notifications::is_windows_transparency_enabled();
        let win_anim = notifications::is_windows_animations_enabled();
        let snap = {
            let mut s = state.lock().unwrap();
            s.windows_transparency_enabled = win_trans;
            s.windows_animations_enabled = win_anim;
            s.clone()
        };
        let mon = unsafe {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN).max(1280),
                bottom: GetSystemMetrics(SM_CYSCREEN).max(720),
            }
        };
        let (x, y, canvas_w, canvas_h) =
            compute_active_window_layout(&mon, &snap, is_idle_collapsed(&snap));
        let r = Renderer::new(canvas_w as u32, canvas_h as u32);
        unsafe {
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.renderer = Some(r);
            });
            set_global_state(state.clone());
            let h_instance = GetModuleHandleW(None)?;

            if !CLASS_REGISTERED.swap(true, Ordering::AcqRel) {
                let app_icon = load_app_icon(32);
                let app_icon_sm = load_app_icon(16);

                let wnd_class = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: h_instance.into(),
                    hIcon: app_icon,
                    hIconSm: app_icon_sm,
                    lpszClassName: CLASS_NAME,
                    hCursor: LoadCursorW(None, IDC_ARROW)?,
                    ..Default::default()
                };

                RegisterClassExW(&wnd_class);
            }

            let (hwnd, host) = create_notch_hwnd(x, y, canvas_w, canvas_h, h_instance)?;
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.dcomp = Some(host);
            });

            attach_hwnd_resources(hwnd);
            if snap.remove_windows_borders {
                crate::borders::set_enabled(true);
            }

            redraw_window(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            Ok(Self { hwnd, state })
        }
    }

    pub fn run_loop(&self) {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// Stores `(raw_hicon, owned)`. Only `owned == true` handles (created via `CreateIconIndirect`)
/// may ever be destroyed via `DestroyIcon`; shared handles from `LR_SHARED` / `IDI_APPLICATION`
/// must never be destroyed (BH-24).
static TRAY_HICON: Mutex<(isize, bool)> = Mutex::new((0, false));
static LAST_TRAY_RAM_PCT: AtomicI32 = AtomicI32::new(-1);
static LAST_TRAY_USED_TENTH: AtomicI32 = AtomicI32::new(-1);

unsafe fn swap_tray_hicon(new_hicon: HICON, owned: bool) {
    let prev = {
        let mut guard = TRAY_HICON.lock().unwrap_or_else(|e| e.into_inner());
        let old = *guard;
        *guard = (new_hicon.0 as isize, owned);
        old
    };
    if prev.1 && prev.0 != 0 {
        let _ = DestroyIcon(HICON(prev.0 as *mut core::ffi::c_void));
    }
}

const DIGIT_5X7: [[u8; 7]; 10] = [
    [
        0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
    ], // 0
    [
        0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ], // 1
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
    ], // 2
    [
        0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
    ], // 3
    [
        0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ], // 4
    [
        0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
    ], // 5
    [
        0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
    ], // 6
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ], // 7
    [
        0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ], // 8
    [
        0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
    ], // 9
];

unsafe fn create_ram_tray_icon(pct: u32, size: i32) -> Option<HICON> {
    let sz = size.clamp(16, 48);
    let scale = (sz / 16).max(1);
    let mut bgra = vec![0u32; (sz * sz) as usize];

    let accent_bgra: u32 = if pct >= 90 {
        0xFF_EF_44_44 // red
    } else if pct >= 80 {
        0xFF_F5_9E_0B // amber
    } else {
        0xFF_8B_5C_F6 // purple
    };
    let fill_bgra: u32 = 0xF5_12_14_24;
    let rim_bgra: u32 = if pct >= 90 {
        0xFF_EF_44_44
    } else if pct >= 80 {
        0xFF_F5_9E_0B
    } else {
        0xFF_7C_3A_ED
    };
    let text_bgra: u32 = 0xFF_F6_F4_FF;

    // Rounded dark indigo-obsidian square badge + 1px border
    for y in 0..sz {
        for x in 0..sz {
            let corner = (x < scale && y < scale)
                || (x >= sz - scale && y < scale)
                || (x < scale && y >= sz - scale)
                || (x >= sz - scale && y >= sz - scale);
            if corner {
                continue;
            }
            let is_border = x < scale || x >= sz - scale || y < scale || y >= sz - scale;
            bgra[(y * sz + x) as usize] = if is_border { rim_bgra } else { fill_bgra };
        }
    }

    // Bottom progress bar (inside border)
    let bar_y0 = sz - scale * 3;
    let bar_y1 = sz - scale * 2;
    let inner_w = (sz - scale * 4).max(1);
    let filled_w = ((inner_w as f32) * (pct.min(100) as f32 / 100.0)).round() as i32;
    for y in bar_y0..bar_y1 {
        for x in (scale * 2)..(scale * 2 + filled_w).min(sz - scale * 2) {
            bgra[(y * sz + x) as usize] = accent_bgra;
        }
    }

    // 2-digit percentage centered in upper area
    let val = pct.min(99);
    let d0 = (val / 10) as usize;
    let d1 = (val % 10) as usize;
    let digits_w = 11 * scale; // 5 + 1 gap + 5
    let digits_h = 7 * scale;
    let ox = ((sz - digits_w) / 2).max(scale);
    let oy = ((bar_y0 - digits_h) / 2).max(scale);

    for (di, d) in [d0, d1].iter().enumerate() {
        let dx_off = ox + (di as i32) * 6 * scale;
        let glyph = DIGIT_5X7[*d];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = dx_off + col * scale + sx;
                            let py = oy + (row as i32) * scale + sy;
                            if px >= 0 && px < sz && py >= 0 && py < sz {
                                bgra[(py * sz + px) as usize] = text_bgra;
                            }
                        }
                    }
                }
            }
        }
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: sz,
            biHeight: -sz,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let hbm_color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0).ok()?;
    if bits_ptr.is_null() {
        let _ = DeleteObject(hbm_color);
        return None;
    }
    std::ptr::copy_nonoverlapping(
        bgra.as_ptr() as *const u8,
        bits_ptr as *mut u8,
        bgra.len() * 4,
    );
    let hbm_mask = CreateBitmap(sz, sz, 1, 1, None);
    let ii = ICONINFO {
        fIcon: TRUE,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };
    let hicon = CreateIconIndirect(&ii).ok();
    let _ = DeleteObject(hbm_color);
    let _ = DeleteObject(hbm_mask);
    hicon
}

fn current_ram_snapshot() -> (f32, f32, f32) {
    if let Some(arc) = get_global_state()
        && let Ok(st) = arc.lock()
    {
        return (
            st.telemetry.memory_percent,
            st.telemetry.memory_used_gb,
            st.telemetry.memory_total_gb,
        );
    }
    (0.0, 0.0, 0.0)
}

unsafe fn add_tray_icon(hwnd: HWND) {
    let (pct_f, used_gb, total_gb) = current_ram_snapshot();
    let pct = pct_f.round().clamp(0.0, 100.0) as i32;
    let used_tenth = (used_gb * 10.0).round() as i32;
    let icon_sz = GetSystemMetrics(SM_CXSMICON).clamp(16, 48);
    let (hicon, owned) = match create_ram_tray_icon(pct as u32, icon_sz) {
        Some(ic) => (ic, true),
        None => (load_app_icon(icon_sz), false),
    };
    swap_tray_hicon(hicon, owned);
    LAST_TRAY_RAM_PCT.store(pct, Ordering::Release);
    LAST_TRAY_USED_TENTH.store(used_tenth, Ordering::Release);

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY_ICON,
        hIcon: hicon,
        ..Default::default()
    };
    let tip_str = format!(
        "CyNotch · RAM {}% ({:.1}/{:.1} GB)\0",
        pct, used_gb, total_gb
    );
    let tip: Vec<u16> = tip_str.encode_utf16().collect();
    let copy_len = tip.len().min(nid.szTip.len());
    nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn update_tray_icon(hwnd: HWND) {
    let (pct_f, used_gb, total_gb) = current_ram_snapshot();
    let pct = pct_f.round().clamp(0.0, 100.0) as i32;
    let used_tenth = (used_gb * 10.0).round() as i32;
    let prev_pct = LAST_TRAY_RAM_PCT.load(Ordering::Acquire);
    let prev_tenth = LAST_TRAY_USED_TENTH.load(Ordering::Acquire);
    if pct == prev_pct && used_tenth == prev_tenth {
        return;
    }

    let icon_sz = GetSystemMetrics(SM_CXSMICON).clamp(16, 48);
    let new_icon = if pct != prev_pct {
        create_ram_tray_icon(pct as u32, icon_sz)
    } else {
        None
    };

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    if let Some(ic) = new_icon {
        nid.uFlags |= NIF_ICON;
        nid.hIcon = ic;
    }
    let tip_str = format!(
        "CyNotch · RAM {}% ({:.1}/{:.1} GB)\0",
        pct, used_gb, total_gb
    );
    let tip: Vec<u16> = tip_str.encode_utf16().collect();
    let copy_len = tip.len().min(nid.szTip.len());
    nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);

    if Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
        LAST_TRAY_RAM_PCT.store(pct, Ordering::Release);
        LAST_TRAY_USED_TENTH.store(used_tenth, Ordering::Release);
        if let Some(ic) = new_icon {
            swap_tray_hicon(ic, true);
        }
    } else if let Some(ic) = new_icon {
        let _ = DestroyIcon(ic);
    }
}

unsafe fn show_notch_context_menu(hwnd: HWND, bottom_align: bool) {
    if let Ok(hmenu) = CreatePopupMenu() {
        let (minimized, pinned) = get_global_state()
            .and_then(|m| m.lock().ok().map(|s| (s.minimized_island, s.is_pinned)))
            .unwrap_or((false, false));
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_TOGGLE,
            w!("Toggle CyNotch (Ctrl+Shift+C)"),
        );
        let _ = AppendMenuW(
            hmenu,
            if minimized {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            },
            MENU_MINIMIZE,
            w!("Minimize Idle Island"),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            MENU_CAPTURE,
            w!("Capture Screen (Ctrl+Shift+S)"),
        );
        let _ = AppendMenuW(hmenu, MF_STRING, MENU_PURGE, w!("Purge RAM (Ctrl+Shift+M)"));
        let _ = AppendMenuW(
            hmenu,
            if pinned {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            },
            MENU_PIN,
            w!("Pin / Unpin Open"),
        );
        let _ = AppendMenuW(hmenu, MF_STRING, MENU_SETTINGS, w!("Open Settings"));
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(hmenu, MF_STRING, MENU_EXIT, w!("Exit CyNotch"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let flags = if bottom_align {
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN
        } else {
            TPM_RIGHTBUTTON | TPM_TOPALIGN
        };
        let _ = TrackPopupMenu(hmenu, flags, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(hmenu);
    }
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    swap_tray_hicon(HICON::default(), false);
}

unsafe fn track_last_foreground(exclude_hwnd: HWND) {
    let fg = GetForegroundWindow();
    if !fg.0.is_null()
        && fg != exclude_hwnd
        && IsWindowVisible(fg).as_bool()
        && !IsIconic(fg).as_bool()
    {
        NOTCH_UI.with_borrow_mut(|ui| {
            ui.last_other_hwnd = Some(fg);
        });
    }
}

unsafe fn perform_capture_async(hwnd: HWND, active_window_only: bool) {
    if CAPTURE_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        return;
    }
    let last_other = NOTCH_UI.with_borrow(|ui| ui.last_other_hwnd);
    let target_rect = if active_window_only {
        let mut rect = None;
        if let Some(prev) = last_other
            && prev != hwnd
            && IsWindowVisible(prev).as_bool()
            && !IsIconic(prev).as_bool()
        {
            let mut rc = RECT::default();
            if GetWindowRect(prev, &mut rc).is_ok() {
                let w = rc.right - rc.left;
                let h = rc.bottom - rc.top;
                if w > 80 && h > 80 {
                    rect = Some((rc.left, rc.top, w, h));
                }
            }
        }
        rect.or_else(|| capture_foreground_window_rect(hwnd))
    } else {
        None
    };

    let copy_to_clip = get_global_state()
        .and_then(|m| m.lock().ok().map(|s| s.capture_to_clipboard))
        .unwrap_or(true);

    NOTCH_UI.with_borrow_mut(|ui| {
        ui.hidden_for_fullscreen = false;
    });
    let _ = ShowWindow(hwnd, SW_HIDE);
    let hwnd_raw = hwnd.0 as isize;

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(65));

        let grab = if let Some((x, y, w, h)) = target_rect {
            grab_rect_frame(x, y, w, h, "win")
        } else {
            grab_full_desktop_frame()
        };

        // Immediately bring CyNotch back before PNG compression & disk I/O
        let _ = PostMessageW(
            HWND(hwnd_raw as *mut std::ffi::c_void),
            WM_APP_SHOW_NOACTIVATE,
            WPARAM(0),
            LPARAM(0),
        );

        let res = grab.and_then(|g| {
            if copy_to_clip {
                IGNORE_NEXT_CLIPBOARD.store(true, Ordering::Release);
            }
            let saved = save_frame(g, copy_to_clip);
            if copy_to_clip && !saved.as_ref().map(|r| r.clipboard_ok).unwrap_or(false) {
                IGNORE_NEXT_CLIPBOARD.store(false, Ordering::Release);
            }
            saved
        });

        let mut chime = false;
        if let Some(state_mutex) = get_global_state()
            && let Ok(mut state) = state_mutex.lock()
        {
            if let Some(cap) = res {
                state.capture.last_path = Some(cap.path.clone());
                state.capture.last_filename = cap.filename.clone();
                state.capture.last_dims = format!("{} × {}", cap.width, cap.height);
                state.capture.clipboard_ok = cap.clipboard_ok;
                state.capture.total_captures += 1;

                chime = state.chime_enabled;
                let dims_label = state.capture.last_dims.clone();
                state.set_notification(
                    "Screenshot Captured".to_string(),
                    format!("{} ({})", cap.filename, dims_label),
                    NotificationKind::Capture,
                    true,
                );
            } else {
                state.set_notification(
                    "Capture Failed".to_string(),
                    "Unable to grab desktop frame".to_string(),
                    NotificationKind::Capture,
                    false,
                );
            }
        }
        if chime {
            let _ = MessageBeep(0x40);
        }
        CAPTURE_IN_PROGRESS.store(false, Ordering::Release);
        let _ = PostMessageW(
            HWND(hwnd_raw as *mut std::ffi::c_void),
            WM_APP_REDRAW,
            WPARAM(0),
            LPARAM(0),
        );
    });
}

unsafe fn trigger_purge_ram_async(hwnd: HWND) {
    if PURGE_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        return;
    }
    let areas = if let Some(state_mutex) = get_global_state() {
        if let Ok(mut state) = state_mutex.lock() {
            state.mem_reduct.is_purging = true;
            state.mem_reduct.last_cleaned_status = "Cleaning RAM...".to_string();
            state.mem_reduct.to_clean_areas()
        } else {
            PURGE_IN_PROGRESS.store(false, Ordering::Release);
            return;
        }
    } else {
        PURGE_IN_PROGRESS.store(false, Ordering::Release);
        return;
    };

    redraw_window(hwnd);
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let res = execute_memory_clean_areas(&areas);
        let mut chime = false;
        if let Some(state_mutex) = get_global_state()
            && let Ok(mut state) = state_mutex.lock()
        {
            state.mem_reduct.is_purging = false;
            if let Some(clean_res) = res {
                chime = state.chime_enabled;
                state.apply_clean_result(clean_res);
            } else {
                state.mem_reduct.last_cleaned_status = "Ready • Clean finished".to_string();
            }
        }
        if chime {
            let _ = MessageBeep(0x40);
        }
        PURGE_IN_PROGRESS.store(false, Ordering::Release);
        let _ = PostMessageW(
            HWND(hwnd_raw as *mut std::ffi::c_void),
            WM_APP_REDRAW,
            WPARAM(0),
            LPARAM(0),
        );
        crate::settings_window::post_redraw();
    });
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wnd_proc_inner(hwnd, msg, wparam, lparam)
    })) {
        Ok(res) => res,
        Err(_) => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn wnd_proc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_WINDOWPOSCHANGED => {
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            sync_notch_backdrop(hwnd, None);
            r
        }
        WM_SHOWWINDOW => {
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            // IsWindowVisible is not yet updated inside this message: use wparam.
            sync_notch_backdrop(hwnd, Some(wparam.0 != 0));
            r
        }
        WM_WINDOWPOSCHANGING => {
            if lparam.0 != 0 {
                let wp = &mut *(lparam.0 as *mut WINDOWPOS);
                let hidden = NOTCH_UI
                    .try_with(|cell| {
                        cell.try_borrow()
                            .map(|ui| ui.hidden_for_fullscreen)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !hidden
                    && !CAPTURE_IN_PROGRESS.load(Ordering::Acquire)
                    && (wp.flags.0 & SWP_HIDEWINDOW.0) == 0
                    && (wp.flags.0 & SWP_NOZORDER.0) == 0
                {
                    wp.hwndInsertAfter = HWND_TOPMOST;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DISPLAYCHANGE => {
            let hz = detect_monitor_hz();
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut s) = state_mutex.lock()
            {
                s.detected_hz = hz;
            }
            let hidden = NOTCH_UI.with_borrow_mut(|ui| {
                ui.has_ulw_initialized = false;
                ui.hidden_for_fullscreen
            });
            redraw_window(hwnd);
            if !hidden && !CAPTURE_IN_PROGRESS.load(Ordering::Acquire) {
                ensure_topmost_zorder(hwnd);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let _dpi_x = ((wparam.0 & 0xffff) as u32).max(96);
            let _dpi_y = (((wparam.0 >> 16) & 0xffff) as u32).max(96);
            if lparam.0 != 0 {
                let suggested = *(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    suggested.left,
                    suggested.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let hz = detect_monitor_hz();
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut s) = state_mutex.lock()
            {
                s.detected_hz = hz;
            }
            let hidden = NOTCH_UI.with_borrow_mut(|ui| {
                ui.has_ulw_initialized = false;
                ui.dpi_generation = ui.dpi_generation.wrapping_add(1);
                ui.win_pos_initialized = false;
                ui.snap_glide_active = false;
                ui.prev_dib_w = 0;
                ui.prev_dib_h = 0;
                if let Some(ref mut glow) = ui.edge_glow {
                    glow.hide();
                }
                ui.hidden_for_fullscreen
            });
            redraw_window(hwnd);
            if !hidden && !CAPTURE_IN_PROGRESS.load(Ordering::Acquire) {
                ensure_topmost_zorder(hwnd);
            }
            LRESULT(0)
        }
        WM_APP_TRIGGER_PURGE => {
            trigger_purge_ram_async(hwnd);
            LRESULT(0)
        }
        WM_APP_QUIT => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let win_trans = notifications::is_windows_transparency_enabled();
            let win_anim = notifications::is_windows_animations_enabled();
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut s) = state_mutex.lock()
            {
                s.windows_transparency_enabled = win_trans;
                s.windows_animations_enabled = win_anim;
            }
            redraw_window(hwnd);
            crate::settings_window::post_redraw();
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let is_dragging = NOTCH_UI
                .try_with(|cell| {
                    cell.try_borrow()
                        .map(|ui| !matches!(ui.drag_mode, DragMode::None))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if is_dragging {
                return LRESULT(1); // HTCLIENT
            }
            let sx = (lparam.0 & 0xffff) as i16 as i32;
            let sy = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let mut pt = POINT { x: sx, y: sy };
            let _ = ScreenToClient(hwnd, &mut pt);
            let in_capsule = if let Some(state_mutex) = get_global_state()
                && let Ok(st) = state_mutex.lock()
            {
                let (cw, ch) = NOTCH_UI.with_borrow(|ui| {
                    if ui.prev_dib_w > 0 && ui.prev_dib_h > 0 {
                        (ui.prev_dib_w as f32, ui.prev_dib_h as f32)
                    } else {
                        (st.canvas_width() as f32, st.canvas_height() as f32)
                    }
                });
                Renderer::point_in_capsule(&st, cw, ch, pt.x as f32, pt.y as f32)
            } else {
                false
            };
            if in_capsule {
                LRESULT(1) // HTCLIENT
            } else {
                LRESULT(-1) // HTTRANSPARENT
            }
        }
        WM_APP_REDRAW => {
            redraw_window(hwnd);
            LRESULT(0)
        }
        WM_APP_HIGHRES_ANIM_TICK => {
            tick_notch_animation(hwnd);
            clear_notch_pacer_in_flight();
            LRESULT(0)
        }
        WM_APP_UPDATE_TRAY => {
            update_tray_icon(hwnd);
            LRESULT(0)
        }
        WM_APP_SHOW_NOACTIVATE => {
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.hidden_for_fullscreen = false;
            });
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            redraw_window(hwnd);
            LRESULT(0)
        }
        WM_APP_EXPAND_NOTCH => {
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.hidden_for_fullscreen = false;
            });
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
            {
                state.set_hovered(true);
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            redraw_window(hwnd);
            LRESULT(0)
        }
        WM_APP_TOGGLE_NOTCH => {
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.hidden_for_fullscreen = false;
            });
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
            {
                let next = !state.is_hovered;
                state.set_hovered(next);
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            redraw_window(hwnd);
            LRESULT(0)
        }
        WM_TRAY_ICON => {
            let evt = (lparam.0 & 0xffff) as u32;
            if evt == WM_LBUTTONUP || evt == WM_LBUTTONDBLCLK {
                NOTCH_UI.with_borrow_mut(|ui| {
                    ui.hidden_for_fullscreen = false;
                });
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    let next = !state.is_hovered;
                    state.set_hovered(next);
                    if state.dismiss_hotkey_hint() {
                        state.save_config();
                    }
                }
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                redraw_window(hwnd);
            } else if evt == WM_RBUTTONUP {
                show_notch_context_menu(hwnd, true);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = wparam.0 & 0xffff;
            match cmd {
                MENU_TOGGLE => {
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.hidden_for_fullscreen = false;
                    });
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(mut state) = state_mutex.lock()
                    {
                        let next = !state.is_hovered;
                        state.set_hovered(next);
                    }
                    redraw_window(hwnd);
                }
                MENU_CAPTURE => {
                    perform_capture_async(hwnd, false);
                }
                MENU_PURGE => {
                    trigger_purge_ram_async(hwnd);
                }
                MENU_PIN => {
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.hidden_for_fullscreen = false;
                    });
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(mut state) = state_mutex.lock()
                    {
                        state.is_pinned = !state.is_pinned;
                        if state.is_pinned {
                            state.set_hovered(true);
                        }
                        state.save_config();
                    }
                    redraw_window(hwnd);
                    crate::settings_window::post_redraw();
                }
                MENU_SETTINGS => {
                    if let Some(state_mutex) = get_global_state() {
                        crate::settings_window::open_or_focus(state_mutex);
                    }
                    redraw_window(hwnd);
                }
                MENU_MINIMIZE => {
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.hidden_for_fullscreen = false;
                    });
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(mut state) = state_mutex.lock()
                    {
                        state.minimized_island = !state.minimized_island;
                        if !state.is_pinned {
                            state.set_hovered(false);
                        }
                        state.save_config();
                    }
                    redraw_window(hwnd);
                    crate::settings_window::post_redraw();
                }
                MENU_EXIT => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let hotkey_id = wparam.0 as i32;
            if hotkey_id == HOTKEY_TOGGLE_NOTCH {
                NOTCH_UI.with_borrow_mut(|ui| {
                    ui.hidden_for_fullscreen = false;
                });
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    let next = !state.is_hovered;
                    state.set_hovered(next);
                    if state.dismiss_hotkey_hint() {
                        state.save_config();
                    }
                }
                redraw_window(hwnd);
            } else if hotkey_id == HOTKEY_PURGE_RAM {
                trigger_purge_ram_async(hwnd);
            } else if hotkey_id == HOTKEY_CAPTURE_SCREEN {
                perform_capture_async(hwnd, false);
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            if IGNORE_NEXT_CLIPBOARD.swap(false, Ordering::AcqRel) {
                return LRESULT(0);
            }
            let watch_enabled = get_global_state()
                .and_then(|m| m.lock().ok().map(|s| s.clipboard_watch_enabled))
                .unwrap_or(true);
            if !watch_enabled {
                return LRESULT(0);
            }
            if let Some(text) = read_clipboard_text(hwnd) {
                let app =
                    crate::screentime::get_active_app_name().unwrap_or_else(|| "App".to_string());
                let mut updated = false;
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    state.push_clipboard(text, &app);
                    updated = true;
                }
                if updated {
                    redraw_window(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_DROPFILES_CODE => {
            let h_drop = wparam.0 as isize;
            let count = DragQueryFileW(h_drop, 0xFFFF_FFFF, std::ptr::null_mut(), 0);
            let mut play_chime = false;
            if count > 0
                && let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
            {
                for i in 0..count.min(8) {
                    let len = DragQueryFileW(h_drop, i, std::ptr::null_mut(), 0);
                    if len > 0 {
                        let mut buf = vec![0u16; (len as usize) + 1];
                        let copied = DragQueryFileW(h_drop, i, buf.as_mut_ptr(), len + 1);
                        if copied > 0 {
                            let path = String::from_utf16_lossy(&buf[..copied as usize]);
                            state.push_dropped_file(path);
                        }
                    }
                }
                state.mode = NotchMode::Shelf;
                state.set_hovered(true);
                play_chime = state.chime_enabled;
            }
            DragFinish(h_drop);
            if play_chime {
                play_chime_async(0);
            }
            redraw_window(hwnd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as i16;
            let steps = if delta >= 0 { 1 } else { -1 };
            let mut changed = false;
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
                && state.is_hovered
            {
                match state.mode {
                    NotchMode::Volume => {
                        let next = (state.volume_pct + (steps as f32) * 0.05).clamp(0.0, 1.0);
                        state.volume_pct = next;
                        if next > 0.05 {
                            state.pre_mute_volume = next;
                        }
                        set_master_volume(next);
                        changed = true;
                    }
                    NotchMode::Music => {
                        if state.show_lyrics {
                            state.lyrics_offset = (state.lyrics_offset - steps).clamp(-20, 20);
                            changed = true;
                        } else if !state.media.is_idle() && state.media.duration_ms > 0 {
                            let cur = state.media.position_ms as i64;
                            let target = (cur + (steps as i64) * 5_000)
                                .clamp(0, state.media.duration_ms as i64)
                                as u64;
                            state.media.position_ms = target;
                            seek_to_position_ms(target);
                            changed = true;
                        }
                    }
                    NotchMode::Tasks => {
                        let vis = if state.placement.is_vertical() { 5 } else { 3 };
                        let max_scroll = state.tasks.len().saturating_sub(vis);
                        let next_scroll = if steps > 0 {
                            state.tasks_scroll.saturating_sub(1)
                        } else {
                            (state.tasks_scroll + 1).min(max_scroll)
                        };
                        if next_scroll != state.tasks_scroll {
                            state.tasks_scroll = next_scroll;
                            changed = true;
                        }
                    }
                    NotchMode::Shelf => {
                        let vis = if state.placement.is_vertical() { 6 } else { 3 };
                        let max_scroll = state.shelf_items.len().saturating_sub(vis);
                        let next_scroll = if steps > 0 {
                            state.shelf_scroll.saturating_sub(1)
                        } else {
                            (state.shelf_scroll + 1).min(max_scroll)
                        };
                        if next_scroll != state.shelf_scroll {
                            state.shelf_scroll = next_scroll;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                redraw_window(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let px = (lparam.0 & 0xffff) as i16 as f32;
            let py = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
            let drag_mode = NOTCH_UI.with_borrow_mut(|ui| {
                ui.pointer_pos = Some((px, py));
                ui.drag_mode
            });

            // Handle active 1:1 hardware drag or slider drags
            match drag_mode {
                DragMode::Grip {
                    start_cursor_x,
                    start_cursor_y,
                    start_offset,
                } => {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let mon = get_monitor_rect(hwnd);
                        let mut move_immediate: Option<(i32, i32)> = None;
                        let mut trigger_glide_redraw = false;
                        let mut glide_interval = None;
                        if let Some(state_mutex) = get_global_state()
                            && let Ok(mut state) = state_mutex.lock()
                        {
                            let (did_snap, placement_changed) = apply_responsive_drag_snap(
                                &mon,
                                &mut state,
                                pt,
                                start_cursor_x,
                                start_cursor_y,
                                start_offset,
                            );
                            let (win_x, win_y, _cw, _ch) = compute_active_window_layout(
                                &mon,
                                &state,
                                is_idle_collapsed(&state),
                            );
                            let new_off = if state.placement.is_vertical() {
                                state.y_offset
                            } else {
                                state.x_offset
                            };
                            let desired = state.needs_animation_timer_ms();
                            let active_iv = state.desired_active_interval_ms();
                            NOTCH_UI.with_borrow_mut(|ui| {
                                if placement_changed {
                                    ui.drag_mode = DragMode::Grip {
                                        start_cursor_x: pt.x,
                                        start_cursor_y: pt.y,
                                        start_offset: new_off,
                                    };
                                }
                                if did_snap {
                                    ui.snap_glide_active = true;
                                    ui.last_anim_instant = Some(std::time::Instant::now());
                                    trigger_glide_redraw = true;
                                    glide_interval =
                                        Some(desired.map_or(active_iv, |m| m.min(active_iv)));
                                } else if !ui.snap_glide_active {
                                    ui.snap_glide_active = false;
                                    ui.anim_win_x = win_x as f32;
                                    ui.anim_win_y = win_y as f32;
                                    ui.win_pos_initialized = true;
                                    ui.last_win_x = win_x;
                                    ui.last_win_y = win_y;
                                    move_immediate = Some((win_x, win_y));
                                } else {
                                    glide_interval =
                                        Some(desired.map_or(active_iv, |m| m.min(active_iv)));
                                }
                            });
                        }
                        if let Some(iv) = glide_interval {
                            sync_animation_timer(hwnd, Some(iv));
                        }
                        if trigger_glide_redraw {
                            redraw_window(hwnd);
                        } else if let Some((win_x, win_y)) = move_immediate {
                            let _ = SetWindowPos(
                                hwnd,
                                HWND_TOPMOST,
                                win_x,
                                win_y,
                                0,
                                0,
                                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                            );
                        }
                    }
                    return LRESULT(0);
                }
                DragMode::IdlePressOrDrag {
                    start_cursor_x,
                    start_cursor_y,
                    start_offset,
                    moved,
                } => {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let now_moved = moved
                            || (pt.x - start_cursor_x).abs() > 4
                            || (pt.y - start_cursor_y).abs() > 4;
                        if now_moved {
                            if !moved {
                                NOTCH_UI.with_borrow_mut(|ui| {
                                    ui.drag_mode = DragMode::IdlePressOrDrag {
                                        start_cursor_x,
                                        start_cursor_y,
                                        start_offset,
                                        moved: true,
                                    };
                                });
                            }
                            let mon = get_monitor_rect(hwnd);
                            let mut move_immediate: Option<(i32, i32)> = None;
                            let mut trigger_glide_redraw = false;
                            let mut glide_interval = None;
                            if let Some(state_mutex) = get_global_state()
                                && let Ok(mut state) = state_mutex.lock()
                            {
                                let (did_snap, placement_changed) = apply_responsive_drag_snap(
                                    &mon,
                                    &mut state,
                                    pt,
                                    start_cursor_x,
                                    start_cursor_y,
                                    start_offset,
                                );
                                let (win_x, win_y, _cw, _ch) = compute_active_window_layout(
                                    &mon,
                                    &state,
                                    is_idle_collapsed(&state),
                                );
                                let new_off = if state.placement.is_vertical() {
                                    state.y_offset
                                } else {
                                    state.x_offset
                                };
                                let desired = state.needs_animation_timer_ms();
                                let active_iv = state.desired_active_interval_ms();
                                NOTCH_UI.with_borrow_mut(|ui| {
                                    if placement_changed {
                                        ui.drag_mode = DragMode::IdlePressOrDrag {
                                            start_cursor_x: pt.x,
                                            start_cursor_y: pt.y,
                                            start_offset: new_off,
                                            moved: true,
                                        };
                                    }
                                    if did_snap {
                                        ui.snap_glide_active = true;
                                        ui.last_anim_instant = Some(std::time::Instant::now());
                                        trigger_glide_redraw = true;
                                        glide_interval =
                                            Some(desired.map_or(active_iv, |m| m.min(active_iv)));
                                    } else if !ui.snap_glide_active {
                                        ui.snap_glide_active = false;
                                        ui.anim_win_x = win_x as f32;
                                        ui.anim_win_y = win_y as f32;
                                        ui.win_pos_initialized = true;
                                        ui.last_win_x = win_x;
                                        ui.last_win_y = win_y;
                                        move_immediate = Some((win_x, win_y));
                                    } else {
                                        glide_interval =
                                            Some(desired.map_or(active_iv, |m| m.min(active_iv)));
                                    }
                                });
                            }
                            if let Some(iv) = glide_interval {
                                sync_animation_timer(hwnd, Some(iv));
                            }
                            if trigger_glide_redraw {
                                redraw_window(hwnd);
                            } else if let Some((win_x, win_y)) = move_immediate {
                                let _ = SetWindowPos(
                                    hwnd,
                                    HWND_TOPMOST,
                                    win_x,
                                    win_y,
                                    0,
                                    0,
                                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                                );
                            }
                        }
                    }
                    return LRESULT(0);
                }
                DragMode::VolumeSlider => {
                    let mut min_iv_ms = 12u32;
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(mut state) = state_mutex.lock()
                    {
                        let (cw, ch) = (state.canvas_width() as f32, state.canvas_height() as f32);
                        let (origin_x, _origin_y) = Renderer::compute_origin(&state, cw, ch);
                        let local_px = (px - origin_x) / state.expanded_scale().max(0.5);
                        let (sx, sw) = if state.placement.is_vertical() {
                            (
                                AppState::VERT_VOLUME_SLIDER_X,
                                AppState::VERT_VOLUME_SLIDER_W,
                            )
                        } else {
                            (AppState::VOLUME_SLIDER_X, AppState::VOLUME_SLIDER_W)
                        };
                        let frac = ((local_px - sx) / sw).clamp(0.0, 1.0);
                        state.volume_pct = frac;
                        if frac > 0.05 {
                            state.pre_mute_volume = frac;
                        }
                        set_master_volume(frac);
                        min_iv_ms = state.desired_active_interval_ms().clamp(6, 16);
                    }
                    let now = std::time::Instant::now();
                    let do_redraw = NOTCH_UI.with_borrow_mut(|ui| {
                        let ready = ui.last_slider_redraw.is_none_or(|prev| {
                            now.duration_since(prev)
                                >= std::time::Duration::from_millis(min_iv_ms as u64)
                        });
                        if ready {
                            ui.last_slider_redraw = Some(now);
                        }
                        ready
                    });
                    if do_redraw {
                        redraw_window(hwnd);
                    }
                    return LRESULT(0);
                }
                DragMode::MusicScrubber { .. } => {
                    let mut min_iv_ms = 12u32;
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.drag_mode = DragMode::MusicScrubber { moved: true };
                    });
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(mut state) = state_mutex.lock()
                        && !state.media.is_idle()
                        && state.media.duration_ms > 0
                    {
                        let (cw, ch) = (state.canvas_width() as f32, state.canvas_height() as f32);
                        let (origin_x, _origin_y) = Renderer::compute_origin(&state, cw, ch);
                        let local_px = (px - origin_x) / state.expanded_scale().max(0.5);
                        let (sx, sw) = if state.placement.is_vertical() {
                            (
                                AppState::VERT_MUSIC_SCRUBBER_X,
                                AppState::VERT_MUSIC_SCRUBBER_W,
                            )
                        } else if state.show_lyrics {
                            (
                                AppState::MUSIC_SCRUBBER_X,
                                AppState::MUSIC_SCRUBBER_W - 138.0,
                            )
                        } else {
                            (AppState::MUSIC_SCRUBBER_X, AppState::MUSIC_SCRUBBER_W)
                        };
                        let frac = ((local_px - sx) / sw.max(1.0)).clamp(0.0, 1.0);
                        let pos = ((state.media.duration_ms as f32) * frac) as u64;
                        state.media.position_ms = pos;
                        min_iv_ms = state.desired_active_interval_ms().clamp(6, 16);
                    }
                    let now = std::time::Instant::now();
                    let do_redraw = NOTCH_UI.with_borrow_mut(|ui| {
                        let ready = ui.last_slider_redraw.is_none_or(|prev| {
                            now.duration_since(prev)
                                >= std::time::Duration::from_millis(min_iv_ms as u64)
                        });
                        if ready {
                            ui.last_slider_redraw = Some(now);
                        }
                        ready
                    });
                    if do_redraw {
                        redraw_window(hwnd);
                    }
                    return LRESULT(0);
                }
                DragMode::None => {}
            }

            let entered = NOTCH_UI.with_borrow_mut(|ui| {
                if !ui.is_tracking_mouse {
                    ui.is_tracking_mouse = true;
                    true
                } else {
                    false
                }
            });
            if entered {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                track_last_foreground(hwnd);

                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                    && !state.click_to_open
                    && state.notification.is_none()
                    && !state.is_hovered
                    && !state.is_hover_pending
                {
                    // Keep notification banners unexpanded on hover so the user can reach the dismiss/action buttons.
                    let dwell_ms = state.hover_dwell_ms() as u32;
                    state.begin_hover_dwell(std::time::Instant::now());
                    let _ = SetTimer(hwnd, TIMER_HOVER_OPEN_ID, dwell_ms, None);
                }
            }

            let (new_idx, idx_changed) = NOTCH_UI.with_borrow_mut(|ui| {
                let new_idx = ui.renderer.as_ref().and_then(|r| {
                    r.hits.iter().position(|h| {
                        px >= h.rect.x()
                            && px <= h.rect.x() + h.rect.width()
                            && py >= h.rect.y()
                            && py <= h.rect.y() + h.rect.height()
                    })
                });
                let changed = entered || new_idx != ui.hover_hit_idx;
                if new_idx != ui.hover_hit_idx {
                    ui.hover_hit_idx = new_idx;
                }
                (new_idx, changed)
            });

            if idx_changed {
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    state.hover_target_idx = new_idx;
                    if let Some(i) = new_idx
                        && state.micro_interactions
                        && i < state.hover_slots.len()
                    {
                        state.hover_slots[i] = state.hover_slots[i].max(0.02);
                    }
                }
                redraw_window(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE_CODE => {
            // Guard against synthetic WM_MOUSELEAVE caused by transient SetWindowPos/SetWindowRgn:
            // if the physical cursor is still inside the visible notch capsule, re-arm tracking.
            let mut cur_pt = POINT::default();
            let still_in_capsule =
                if GetCursorPos(&mut cur_pt).is_ok() && IsWindowVisible(hwnd).as_bool() {
                    let mut client_pt = cur_pt;
                    let _ = ScreenToClient(hwnd, &mut client_pt);
                    if let Some(state_mutex) = get_global_state()
                        && let Ok(st) = state_mutex.lock()
                    {
                        let (cw, ch) = NOTCH_UI.with_borrow(|ui| {
                            if ui.prev_dib_w > 0 && ui.prev_dib_h > 0 {
                                (ui.prev_dib_w as f32, ui.prev_dib_h as f32)
                            } else {
                                (st.canvas_width() as f32, st.canvas_height() as f32)
                            }
                        });
                        Renderer::point_in_capsule(
                            &st,
                            cw,
                            ch,
                            client_pt.x as f32,
                            client_pt.y as f32,
                        )
                    } else {
                        false
                    }
                } else {
                    false
                };
            if still_in_capsule {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                return LRESULT(0);
            }

            let _ = KillTimer(hwnd, TIMER_HOVER_OPEN_ID);
            let is_idle_drag = NOTCH_UI.with_borrow_mut(|ui| {
                ui.is_tracking_mouse = false;
                ui.pointer_pos = None;
                ui.hover_hit_idx = None;
                matches!(ui.drag_mode, DragMode::None)
            });
            let mut should_redraw = true;
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
            {
                state.hover_target_idx = None;
                if state.is_hover_pending {
                    state.cancel_hover_dwell();
                }
                if is_idle_drag {
                    if !state.is_pinned {
                        state.set_hovered(false);
                    } else if !state.is_hovered {
                        should_redraw = false;
                    }
                }
            }
            if should_redraw {
                redraw_window(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let x = (lparam.0 & 0xffff) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
            let hit_action = NOTCH_UI.with_borrow(|ui| {
                ui.renderer.as_ref().and_then(|r| {
                    r.hits
                        .iter()
                        .find(|h| {
                            x >= h.rect.x()
                                && x <= h.rect.x() + h.rect.width()
                                && y >= h.rect.y()
                                && y <= h.rect.y() + h.rect.height()
                        })
                        .map(|h| h.action)
                })
            });
            if matches!(hit_action, Some(Action::StartGripDrag)) {
                let _ = ReleaseCapture();
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    state.x_offset = 0;
                    state.y_offset = 0;
                    state.reset_anchor_for_placement();
                    state.save_config();
                }
                NOTCH_UI.with_borrow_mut(|ui| {
                    ui.drag_mode = DragMode::None;
                    ui.snap_glide_active = true;
                    ui.last_anim_instant = Some(std::time::Instant::now());
                });
                crate::settings_window::post_redraw();
                redraw_window(hwnd);
                return LRESULT(0);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            show_notch_context_menu(hwnd, false);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            IS_DRAGGING_VOLUME.store(false, Ordering::Release);
            let prev = NOTCH_UI.with_borrow_mut(|ui| {
                ui.last_slider_redraw = None;
                std::mem::take(&mut ui.drag_mode)
            });
            if !matches!(prev, DragMode::None) {
                if matches!(
                    prev,
                    DragMode::Grip { .. } | DragMode::IdlePressOrDrag { moved: true, .. }
                ) && let Some(state_mutex) = get_global_state()
                    && let Ok(state) = state_mutex.lock()
                {
                    state.save_config();
                }
                redraw_window(hwnd);
                crate::settings_window::post_redraw();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            IS_DRAGGING_VOLUME.store(false, Ordering::Release);
            let prev = NOTCH_UI.with_borrow_mut(|ui| {
                ui.last_slider_redraw = None;
                std::mem::take(&mut ui.drag_mode)
            });
            if !matches!(prev, DragMode::None) {
                let _ = ReleaseCapture();
                match prev {
                    DragMode::Grip { .. } | DragMode::IdlePressOrDrag { moved: true, .. } => {
                        let mut glide_on_release = false;
                        if let Some(state_mutex) = get_global_state()
                            && let Ok(mut state) = state_mutex.lock()
                        {
                            let cur_off = if state.placement.is_vertical() {
                                state.y_offset
                            } else {
                                state.x_offset
                            };
                            if !state.is_corner_docked() && cur_off != 0 && cur_off.abs() <= 20 {
                                state.x_offset = 0;
                                state.y_offset = 0;
                                state.reset_anchor_for_placement();
                                glide_on_release = true;
                            }
                            state.save_config();
                        }
                        if glide_on_release {
                            NOTCH_UI.with_borrow_mut(|ui| {
                                ui.snap_glide_active = true;
                                ui.last_anim_instant = Some(std::time::Instant::now());
                            });
                        }
                        crate::settings_window::post_redraw();
                        redraw_window(hwnd);
                    }
                    DragMode::IdlePressOrDrag { moved: false, .. } => {
                        if let Some(state_mutex) = get_global_state()
                            && let Ok(mut state) = state_mutex.lock()
                        {
                            state.set_hovered(true);
                            if state.record_notch_expand_for_hint() {
                                state.save_config();
                            }
                        }
                        redraw_window(hwnd);
                    }
                    DragMode::VolumeSlider => {
                        redraw_window(hwnd);
                    }
                    DragMode::MusicScrubber { moved: true } => {
                        let mut seek_pos = None;
                        if let Some(state_mutex) = get_global_state()
                            && let Ok(s) = state_mutex.lock()
                            && !s.media.is_idle()
                        {
                            seek_pos = Some(s.media.position_ms);
                        }
                        if let Some(pos) = seek_pos {
                            seek_to_position_ms(pos);
                        }
                        redraw_window(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let _ = KillTimer(hwnd, TIMER_HOVER_OPEN_ID);
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
                && state.is_hover_pending
            {
                state.cancel_hover_dwell();
            }
            let x = (lparam.0 & 0xffff) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;

            let hit_action = NOTCH_UI.with_borrow(|ui| {
                ui.renderer.as_ref().and_then(|r| {
                    r.hits
                        .iter()
                        .find(|h| {
                            x >= h.rect.x()
                                && x <= h.rect.x() + h.rect.width()
                                && y >= h.rect.y()
                                && y <= h.rect.y() + h.rect.height()
                        })
                        .map(|h| h.action)
                })
            });

            let clip_for_task = if matches!(hit_action, Some(Action::PasteAsTask)) {
                read_clipboard_text(hwnd)
            } else {
                None
            };

            let mut cap_trigger: Option<bool> = None;
            let mut purge_trigger = false;
            let mut chime_to_play: Option<u32> = None;
            let mut open_path: Option<String> = None;
            let mut clip_copy: Option<String> = None;
            let mut create_capture_dir = false;
            let mut autostart_update: Option<bool> = None;
            let mut open_settings_win = false;
            let mut notify_settings = false;
            let mut start_grip_offset: Option<i32> = None;
            let mut start_idle_press: Option<i32> = None;
            let mut new_drag_mode: Option<DragMode> = None;
            let mut extra_effects: Option<SettingEffects> = None;
            let mut activate_notif: Option<(String, Option<String>)> = None;

            if let Some(state_mutex) = get_global_state()
                && let Ok(mut state) = state_mutex.lock()
            {
                if let Some(action) = hit_action {
                    match action {
                        Action::SetMode(mode) => {
                            state.set_mode(mode);
                        }
                        Action::TogglePin => {
                            state.is_pinned = !state.is_pinned;
                            state.save_config();
                            notify_settings = true;
                        }
                        Action::StartGripDrag => {
                            start_grip_offset = Some(if state.placement.is_vertical() {
                                state.y_offset
                            } else {
                                state.x_offset
                            });
                        }
                        Action::ResetPosition => {
                            state.x_offset = 0;
                            state.y_offset = 0;
                            state.reset_anchor_for_placement();
                            state.save_config();
                            notify_settings = true;
                            NOTCH_UI.with_borrow_mut(|ui| {
                                ui.snap_glide_active = true;
                                ui.last_anim_instant = Some(std::time::Instant::now());
                            });
                        }
                        Action::ToggleLyrics => {
                            state.show_lyrics = !state.show_lyrics;
                            state.lyrics_offset = 0;
                        }
                        Action::ScrollLyrics(delta) => {
                            state.lyrics_offset = (state.lyrics_offset + delta).clamp(-20, 20);
                        }
                        Action::PrevTrack => {
                            if !state.media.is_idle() {
                                prev_track();
                            }
                        }
                        Action::TogglePlayPause => {
                            if !state.media.is_idle() {
                                toggle_play_pause();
                                state.media.is_playing = !state.media.is_playing;
                            } else {
                                state.set_notification(
                                    "No Media Active".to_string(),
                                    "Open Spotify, YouTube, or a player first".to_string(),
                                    NotificationKind::General,
                                    false,
                                );
                            }
                        }
                        Action::NextTrack => {
                            if !state.media.is_idle() {
                                next_track();
                            }
                        }
                        Action::PurgeRam => {
                            purge_trigger = true;
                        }
                        Action::ToggleAutoClean => {
                            state.mem_reduct.auto_clean_enabled =
                                !state.mem_reduct.auto_clean_enabled;
                            state.save_config();
                            notify_settings = true;
                        }
                        Action::ToggleArea(idx) => {
                            state.toggle_mem_area(idx);
                            notify_settings = true;
                        }
                        Action::CycleThreshold => {
                            state.cycle_threshold();
                            notify_settings = true;
                        }
                        Action::CycleInterval => {
                            state.cycle_interval();
                            notify_settings = true;
                        }
                        Action::ToggleTask(idx) => {
                            if let Some(task) = state.tasks.get_mut(idx) {
                                task.completed = !task.completed;
                                state.save_tasks();
                            }
                        }
                        Action::DeleteTask(idx) => {
                            if idx < state.tasks.len() {
                                state.tasks.remove(idx);
                                let vis = if state.placement.is_vertical() { 5 } else { 3 };
                                let max_s = state.tasks.len().saturating_sub(vis);
                                state.tasks_scroll = state.tasks_scroll.min(max_s);
                                state.save_tasks();
                            }
                        }
                        Action::ClearCompletedTasks => {
                            state.tasks.retain(|t| !t.completed);
                            let vis = if state.placement.is_vertical() { 5 } else { 3 };
                            let max_s = state.tasks.len().saturating_sub(vis);
                            state.tasks_scroll = state.tasks_scroll.min(max_s);
                            state.save_tasks();
                        }
                        Action::PasteAsTask => {
                            let raw_opt = clip_for_task.or_else(|| {
                                state
                                    .shelf_items
                                    .iter()
                                    .find(|s| s.kind == ShelfKind::Text)
                                    .map(|s| s.label.clone())
                            });
                            let clean = raw_opt
                                .as_deref()
                                .and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
                                .unwrap_or("")
                                .to_string();
                            if !clean.is_empty() {
                                state.tasks.insert(
                                    0,
                                    crate::models::TaskItem {
                                        text: clean,
                                        completed: false,
                                    },
                                );
                                state.tasks_scroll = 0;
                                state.save_tasks();
                                if state.chime_enabled {
                                    chime_to_play = Some(0);
                                }
                            } else {
                                state.set_notification(
                                    "Clipboard Empty".to_string(),
                                    "Copy text first to paste a task".to_string(),
                                    NotificationKind::General,
                                    false,
                                );
                            }
                        }
                        Action::ActivateShelfItem(idx) => {
                            if let Some(item) = state.shelf_items.get(idx).cloned() {
                                match item.kind {
                                    ShelfKind::Text => {
                                        let char_len = item.payload.chars().count();
                                        clip_copy = Some(item.payload);
                                        state.set_notification(
                                            "Copied to Clipboard".to_string(),
                                            format!("{} chars ready to paste", char_len),
                                            NotificationKind::Pasted,
                                            true,
                                        );
                                    }
                                    ShelfKind::File => {
                                        open_path = Some(item.payload);
                                        state.set_notification(
                                            "Opened File".to_string(),
                                            item.label,
                                            NotificationKind::Pasted,
                                            true,
                                        );
                                    }
                                }
                                if state.chime_enabled {
                                    chime_to_play = Some(0);
                                }
                            }
                        }
                        Action::DeleteShelfItem(idx) => {
                            if idx < state.shelf_items.len() {
                                state.shelf_items.remove(idx);
                                let vis = if state.placement.is_vertical() { 6 } else { 3 };
                                let max_s = state.shelf_items.len().saturating_sub(vis);
                                state.shelf_scroll = state.shelf_scroll.min(max_s);
                                state.save_shelf();
                            }
                        }
                        Action::ClearShelf => {
                            state.shelf_items.clear();
                            state.shelf_scroll = 0;
                            state.save_shelf();
                        }
                        Action::CaptureScreen => {
                            cap_trigger = Some(false);
                        }
                        Action::CaptureWindow => {
                            cap_trigger = Some(true);
                        }
                        Action::OpenCaptureFolder => {
                            create_capture_dir = true;
                            if matches!(
                                state.notification.as_ref().map(|n| n.kind),
                                Some(NotificationKind::Capture)
                            ) {
                                state.advance_notification_queue();
                            }
                        }
                        Action::SeekMusic(_pct) => {
                            if !state.media.is_idle() && state.media.duration_ms > 0 {
                                new_drag_mode = Some(DragMode::MusicScrubber { moved: false });
                                let (cw, ch) =
                                    (state.canvas_width() as f32, state.canvas_height() as f32);
                                let (ox, _oy) = Renderer::compute_origin(&state, cw, ch);
                                let local_px = (x - ox) / state.expanded_scale().max(0.5);
                                let (sx, sw) = if state.placement.is_vertical() {
                                    (
                                        AppState::VERT_MUSIC_SCRUBBER_X,
                                        AppState::VERT_MUSIC_SCRUBBER_W,
                                    )
                                } else if state.show_lyrics {
                                    (
                                        AppState::MUSIC_SCRUBBER_X,
                                        AppState::MUSIC_SCRUBBER_W - 138.0,
                                    )
                                } else {
                                    (AppState::MUSIC_SCRUBBER_X, AppState::MUSIC_SCRUBBER_W)
                                };
                                let exact_pct = ((local_px - sx) / sw.max(1.0)).clamp(0.0, 1.0);
                                let pos = ((state.media.duration_ms as f32) * exact_pct)
                                    .clamp(0.0, state.media.duration_ms as f32)
                                    as u64;
                                state.media.position_ms = pos;
                                seek_to_position_ms(pos);
                            }
                        }
                        Action::SetVolume(pct) => {
                            let (cw, ch) =
                                (state.canvas_width() as f32, state.canvas_height() as f32);
                            let (ox, oy) = Renderer::compute_origin(&state, cw, ch);
                            let ly = (y - oy) / state.expanded_scale().max(0.5);
                            let cutoff = if state.placement.is_vertical() {
                                365.0
                            } else {
                                150.0
                            };
                            let clamped = if ly < cutoff {
                                new_drag_mode = Some(DragMode::VolumeSlider);
                                IS_DRAGGING_VOLUME.store(true, Ordering::Release);
                                let local_px = (x - ox) / state.expanded_scale().max(0.5);
                                let (sx, sw) = if state.placement.is_vertical() {
                                    (
                                        AppState::VERT_VOLUME_SLIDER_X,
                                        AppState::VERT_VOLUME_SLIDER_W,
                                    )
                                } else {
                                    (AppState::VOLUME_SLIDER_X, AppState::VOLUME_SLIDER_W)
                                };
                                ((local_px - sx) / sw.max(1.0)).clamp(0.0, 1.0)
                            } else {
                                pct.clamp(0.0, 1.0)
                            };
                            state.volume_pct = clamped;
                            if clamped > 0.05 {
                                state.pre_mute_volume = clamped;
                            }
                            set_master_volume(clamped);
                        }
                        Action::ToggleMute => {
                            if state.volume_pct <= 0.01 {
                                let restore = if state.pre_mute_volume > 0.05 {
                                    state.pre_mute_volume
                                } else {
                                    0.50
                                };
                                state.volume_pct = restore;
                                set_master_volume(restore);
                            } else {
                                state.pre_mute_volume = state.volume_pct;
                                state.volume_pct = 0.0;
                                set_master_volume(0.0);
                            }
                        }
                        Action::OpenSettings => {
                            open_settings_win = true;
                        }
                        Action::ToggleAutostart => {
                            state.autostart = !state.autostart;
                            autostart_update = Some(state.autostart);
                            notify_settings = true;
                        }
                        Action::ToggleChime => {
                            state.chime_enabled = !state.chime_enabled;
                            state.save_config();
                            notify_settings = true;
                        }
                        Action::ActivateNotification => {
                            if let Some(notif) = state.notification.take() {
                                activate_notif = Some((notif.app_name, notif.aumid));
                                state.advance_notification_queue();
                                if state.notification.is_none() && !state.is_pinned {
                                    state.set_hovered(false);
                                }
                            }
                        }
                        Action::ActivateHistoryItem(idx) => {
                            if let Some(entry) = state.notif_history.get(idx).cloned() {
                                activate_notif = Some((entry.app_name, entry.aumid));
                                if !state.is_pinned {
                                    state.set_hovered(false);
                                }
                            }
                        }
                        Action::Quit => {
                            drop(state);
                            let _ = DestroyWindow(hwnd);
                            return LRESULT(0);
                        }
                        other => {
                            extra_effects = state.apply_setting_action(other);
                            notify_settings = true;
                        }
                    }
                } else if !state.is_hovered {
                    start_idle_press = Some(if state.placement.is_vertical() {
                        state.y_offset
                    } else {
                        state.x_offset
                    });
                } else {
                    let (cw, ch) = (state.canvas_width() as f32, state.canvas_height() as f32);
                    let (_ox, oy) = Renderer::compute_origin(&state, cw, ch);
                    let ly = (y - oy) / state.expanded_scale().max(0.5);
                    if (0.0..=56.0).contains(&ly) {
                        start_grip_offset = Some(if state.placement.is_vertical() {
                            state.y_offset
                        } else {
                            state.x_offset
                        });
                    }
                }
            }

            if let Some(start_offset) = start_grip_offset {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.drag_mode = DragMode::Grip {
                            start_cursor_x: pt.x,
                            start_cursor_y: pt.y,
                            start_offset,
                        };
                    });
                    SetCapture(hwnd);
                }
            } else if let Some(start_offset) = start_idle_press {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    NOTCH_UI.with_borrow_mut(|ui| {
                        ui.drag_mode = DragMode::IdlePressOrDrag {
                            start_cursor_x: pt.x,
                            start_cursor_y: pt.y,
                            start_offset,
                            moved: false,
                        };
                    });
                    SetCapture(hwnd);
                }
                return LRESULT(0);
            } else if let Some(dm) = new_drag_mode {
                NOTCH_UI.with_borrow_mut(|ui| {
                    ui.drag_mode = dm;
                });
                SetCapture(hwnd);
            }

            if let Some((app_name, aumid)) = activate_notif {
                notifications::activate_source_app(&app_name, aumid.as_deref());
            }

            let active_hwnd = hwnd;
            if let Some(eff) = extra_effects {
                if let Some(enabled) = eff.update_autostart {
                    autostart_update = Some(enabled);
                }
                if eff.reset_screentime {
                    crate::screentime::RESET_SCREENTIME_REQUESTED.store(true, Ordering::Release);
                }
                if eff.trigger_purge {
                    purge_trigger = true;
                }
                if eff.open_win_notif_settings {
                    let needs_privacy = get_global_state()
                        .and_then(|arc| {
                            arc.lock().ok().map(|st| {
                                !matches!(
                                    st.notif_listener_status.as_str(),
                                    "Connected" | "Listening" | "Off"
                                )
                            })
                        })
                        .unwrap_or(false);
                    if needs_privacy {
                        notifications::open_windows_notification_privacy_settings();
                    } else {
                        notifications::open_windows_notification_settings();
                    }
                    NOTIF_SYNC_REQUESTED.store(true, Ordering::Release);
                }
                if eff.open_win_personalize_colors {
                    notifications::open_windows_personalize_colors();
                }
                if eff.request_notif_access {
                    NOTIF_SYNC_REQUESTED.store(true, Ordering::Release);
                }
                if let Some(sound) = eff.play_sound {
                    crate::sound::play_sound(sound);
                }
                if let Some(mute_win_sound) = eff.sync_win_notif_sound_mute {
                    notifications::sync_windows_notification_sound_mute(mute_win_sound);
                }
                if let Some(exclusive) = eff.sync_exclusive_banners
                    && let Some(st_arc) = get_global_state()
                {
                    notifications::enqueue_exclusive_windows_banners(
                        exclusive,
                        Box::new(move |suppressed_count| {
                            if let Ok(mut st) = st_arc.lock() {
                                st.notif_suppressed_apps = suppressed_count;
                            }
                            post_redraw();
                            crate::settings_window::post_redraw();
                        }),
                    );
                }
                if let Some(rm_borders) = eff.sync_windows_borders {
                    crate::borders::set_enabled(rm_borders);
                }
                if eff.quit_app {
                    let _ = DestroyWindow(active_hwnd);
                    return LRESULT(0);
                }
            }

            if let Some(enabled) = autostart_update {
                crate::autostart::set_autostart(enabled);
            }
            if let Some(payload) = clip_copy {
                IGNORE_NEXT_CLIPBOARD.store(true, Ordering::Release);
                if !copy_text_to_clipboard(active_hwnd, &payload) {
                    IGNORE_NEXT_CLIPBOARD.store(false, Ordering::Release);
                }
            }
            if create_capture_dir {
                let dir = pictures_capture_dir();
                let _ = std::fs::create_dir_all(&dir);
                shell_open_path(&dir.to_string_lossy());
            } else if let Some(path) = open_path {
                shell_open_path(&path);
            }
            if let Some(c) = chime_to_play {
                play_chime_async(c);
            }
            if open_settings_win {
                if let Some(state_mutex) = get_global_state() {
                    crate::settings_window::open_or_focus(state_mutex);
                }
            } else if notify_settings {
                crate::settings_window::post_redraw();
            }

            if let Some(window_only) = cap_trigger {
                perform_capture_async(active_hwnd, window_only);
                return LRESULT(0);
            }
            if purge_trigger {
                trigger_purge_ram_async(active_hwnd);
                return LRESULT(0);
            }

            redraw_window(active_hwnd);
            let refreshed = NOTCH_UI.with_borrow_mut(|ui| {
                let r = ui.pointer_pos.and_then(|(mx, my)| {
                    ui.renderer.as_ref().and_then(|ren| {
                        ren.hits.iter().position(|h| {
                            mx >= h.rect.x()
                                && mx <= h.rect.x() + h.rect.width()
                                && my >= h.rect.y()
                                && my <= h.rect.y() + h.rect.height()
                        })
                    })
                });
                ui.hover_hit_idx = r;
                r
            });
            let mut post_ms = None;
            if let Some(state_mutex) = get_global_state()
                && let Ok(mut st) = state_mutex.lock()
            {
                st.hover_target_idx = refreshed;
                if let Some(i) = refreshed
                    && st.micro_interactions
                    && i < st.hover_slots.len()
                {
                    st.hover_slots[i] = st.hover_slots[i].max(0.02);
                }
                post_ms = st.needs_animation_timer_ms();
            }
            sync_animation_timer(active_hwnd, post_ms);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATION_ID {
                tick_notch_animation(hwnd);
            } else if wparam.0 == TIMER_HOVER_OPEN_ID {
                let _ = KillTimer(hwnd, TIMER_HOVER_OPEN_ID);
                let mut should_redraw = false;
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                    && state.is_hover_pending
                {
                    state.cancel_hover_dwell();
                    if !state.click_to_open && state.notification.is_none() && !state.is_hovered {
                        state.set_hovered(true);
                        if state.record_notch_expand_for_hint() {
                            state.save_config();
                        }
                        should_redraw = true;
                    }
                }
                if should_redraw {
                    redraw_window(hwnd);
                }
            } else if wparam.0 == TIMER_POLL_ID {
                let now = std::time::Instant::now();
                let one_sec_elapsed = NOTCH_UI.with_borrow_mut(|ui| match ui.last_second_tick {
                    Some(prev)
                        if now.duration_since(prev) >= std::time::Duration::from_millis(950) =>
                    {
                        ui.last_second_tick = Some(now);
                        true
                    }
                    None => {
                        ui.last_second_tick = Some(now);
                        false
                    }
                    _ => false,
                });
                let mut needs_redraw = false;
                let mut hide_fullscreen_pref = false;
                let mut is_protected = false;
                let mut idle_collapsed = false;
                if let Some(state_mutex) = get_global_state()
                    && let Ok(mut state) = state_mutex.lock()
                {
                    hide_fullscreen_pref = state.hide_in_fullscreen;
                    is_protected = state.is_hovered
                        || state.is_pinned
                        || CAPTURE_IN_PROGRESS.load(Ordering::Acquire)
                        || state.notification_overlays_fullscreen();
                    if state.notification.is_some() && state.expire_notification() {
                        needs_redraw = true;
                    }
                    if one_sec_elapsed
                        && state.media.is_playing
                        && !state.media.is_idle()
                        && state.media.duration_ms > 0
                    {
                        state.media.position_ms =
                            (state.media.position_ms + 1000).min(state.media.duration_ms);
                        if state.is_hovered && state.mode == NotchMode::Music {
                            needs_redraw = true;
                        }
                    }
                    idle_collapsed = is_idle_collapsed(&state);
                    if one_sec_elapsed {
                        let now_sec = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if now_sec % 10 == 0 {
                            let win_trans = notifications::is_windows_transparency_enabled();
                            let win_anim = notifications::is_windows_animations_enabled();
                            if state.windows_transparency_enabled != win_trans
                                || state.windows_animations_enabled != win_anim
                            {
                                state.windows_transparency_enabled = win_trans;
                                state.windows_animations_enabled = win_anim;
                                needs_redraw = true;
                                crate::settings_window::post_redraw();
                            }
                        }
                    }
                }

                NOTCH_UI.with_borrow_mut(|ui| {
                    if idle_collapsed {
                        let since = ui.idle_since.get_or_insert(now);
                        if !ui.idle_caches_trimmed
                            && now.duration_since(*since) >= std::time::Duration::from_secs(3)
                        {
                            if let Some(ren) = ui.renderer.as_mut() {
                                ren.trim_idle_caches();
                            }
                            ui.idle_caches_trimmed = true;
                        }
                    } else {
                        ui.idle_since = None;
                        ui.idle_caches_trimmed = false;
                    }
                });

                if !idle_collapsed || one_sec_elapsed {
                    track_last_foreground(hwnd);
                    if hide_fullscreen_pref && !is_protected && is_foreign_fullscreen_active(hwnd) {
                        let became_hidden = NOTCH_UI.with_borrow_mut(|ui| {
                            if let Some(ref mut glow) = ui.edge_glow {
                                glow.hide();
                            }
                            if !ui.hidden_for_fullscreen {
                                ui.hidden_for_fullscreen = true;
                                true
                            } else {
                                false
                            }
                        });
                        if became_hidden {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }
                    } else if !CAPTURE_IN_PROGRESS.load(Ordering::Acquire) {
                        let became_shown = NOTCH_UI.with_borrow_mut(|ui| {
                            if ui.hidden_for_fullscreen {
                                ui.hidden_for_fullscreen = false;
                                true
                            } else {
                                false
                            }
                        });
                        if became_shown {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            let _ = SetWindowPos(
                                hwnd,
                                HWND_TOPMOST,
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                            needs_redraw = true;
                        } else {
                            ensure_topmost_zorder(hwnd);
                        }
                    }
                }
                if needs_redraw {
                    redraw_window(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            SHUTDOWN_REQUESTED.store(true, Ordering::Release);
            // BH-16: Always restore system-wide DWM borders before process shutdown.
            crate::borders::set_enabled(false);
            set_notch_pacer(hwnd, 0);
            set_settings_pacer(HWND::default(), 0);
            set_high_res_slot(1, false);
            set_high_res_slot(2, false);
            MAIN_HWND.store(0, Ordering::Release);
            crate::settings_window::close_if_open();
            detach_hwnd_resources(hwnd);
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.dcomp = None;
                ui.renderer = None;
                ui.pointer_pos = None;
                ui.hover_hit_idx = None;
                ui.drag_mode = DragMode::None;
                ui.is_tracking_mouse = false;
                ui.anim_timer_interval_ms = 0;
                ui.last_anim_instant = None;
                ui.last_second_tick = None;
                ui.hidden_for_fullscreen = false;
                ui.has_ulw_initialized = false;
                ui.prev_dib_w = 0;
                ui.prev_dib_h = 0;
                ui.last_win_x = 0;
                ui.last_win_y = 0;
                ui.anim_win_x = 0.0;
                ui.anim_win_y = 0.0;
                ui.win_pos_initialized = false;
                ui.snap_glide_active = false;
                ui.last_slider_redraw = None;
                ui.last_rgn = None;
                ui.idle_since = None;
                ui.idle_caches_trimmed = false;
                ui.backdrop = None;
                ui.backdrop_wanted = false;
                ui.backdrop_geom = None;
                ui.edge_glow = None;
            });
            crate::composition::release_thread_locals();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn redraw_window(hwnd: HWND) {
    let (state_snapshot, desired_timer_ms) = if let Some(state_mutex) = get_global_state() {
        if let Ok(state) = state_mutex.lock() {
            (state.clone(), state.needs_animation_timer_ms())
        } else {
            return;
        }
    } else {
        return;
    };

    if !state_snapshot.hide_in_fullscreen || state_snapshot.notification_overlays_fullscreen() {
        let should_unhide = NOTCH_UI.with_borrow_mut(|ui| {
            if ui.hidden_for_fullscreen {
                ui.hidden_for_fullscreen = false;
                true
            } else {
                false
            }
        });
        if should_unhide {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    let (effective_timer_ms, snap_glide) = NOTCH_UI.with_borrow(|ui| {
        let eff = if ui.snap_glide_active {
            let act = state_snapshot.desired_active_interval_ms();
            Some(desired_timer_ms.map_or(act, |m| m.min(act)))
        } else {
            desired_timer_ms
        };
        (eff, ui.snap_glide_active)
    });
    sync_animation_timer(hwnd, effective_timer_ms);

    let idle_collapsed = is_idle_collapsed(&state_snapshot);
    let compact_idle = idle_collapsed && !snap_glide;
    let mon = get_monitor_rect(hwnd);
    let (target_x, target_y, canvas_w, canvas_h) =
        compute_active_window_layout(&mon, &state_snapshot, compact_idle);
    let (win_x, win_y) = NOTCH_UI.with_borrow_mut(|ui| {
        if idle_collapsed {
            if ui.idle_since.is_none() {
                ui.idle_since = Some(std::time::Instant::now());
            }
        } else {
            ui.idle_since = None;
            ui.idle_caches_trimmed = false;
        }
        if !ui.win_pos_initialized || !ui.snap_glide_active {
            ui.anim_win_x = target_x as f32;
            ui.anim_win_y = target_y as f32;
            ui.win_pos_initialized = true;
            ui.snap_glide_active = false;
            (target_x, target_y)
        } else {
            (ui.anim_win_x.round() as i32, ui.anim_win_y.round() as i32)
        }
    });

    let (prev_init, dpi_gen_before, size_changed, pos_changed, pointer_pos, ren_opt, mut host_opt) =
        NOTCH_UI.with_borrow_mut(|ui| {
            let prev_init = ui.has_ulw_initialized;
            let dpi_gen = ui.dpi_generation;
            let size_changed = ui.prev_dib_w != canvas_w || ui.prev_dib_h != canvas_h;
            let pos_changed = ui.last_win_x != win_x || ui.last_win_y != win_y;
            (
                prev_init,
                dpi_gen,
                size_changed,
                pos_changed,
                ui.pointer_pos,
                ui.renderer.take(),
                ui.dcomp.take(),
            )
        });
    let force_full = !prev_init || size_changed || pos_changed;

    {
        // Cache the shell silhouette BEFORE any SetWindowPos so the
        // WM_WINDOWPOSCHANGED hook positions the blur from fresh numbers.
        // Only apply extra inset when capsule geometry is actually resizing (not on idle mascot/dwell).
        let geom_moving = state_snapshot.is_size_moving();
        let wanted = state_snapshot.is_frosted_active()
            && backdrop::blur_available()
            && !(backdrop::HIDE_WHILE_ANIMATING && geom_moving);
        backdrop::set_blur_px(state_snapshot.frosted_blur_px);
        let geom = notch_backdrop_geom(&state_snapshot, canvas_w, canvas_h);
        let base_inset = geom
            .as_ref()
            .map(|g| backdrop::inset_for_radius(g.diam as f32 * 0.5))
            .unwrap_or(backdrop::REST_INSET);
        let inset = if geom_moving {
            base_inset + backdrop::ANIMATION_EXTRA_INSET
        } else {
            base_inset
        };
        NOTCH_UI.with_borrow_mut(|ui| {
            ui.backdrop_wanted = wanted;
            ui.backdrop_geom = geom;
            ui.backdrop_inset = inset;
        });
    }

    let Some(mut ren) = ren_opt else {
        if let Some(h) = host_opt {
            NOTCH_UI.with_borrow_mut(|ui| ui.dcomp = Some(h));
        }
        return;
    };

    // Render into the resized SwapChain BEFORE SetWindowPos moves/resizes the HWND so
    // DWM never flashes a stale frame shifted to the new window origin ("teleporting ghost").
    ren.resize(canvas_w as u32, canvas_h as u32);
    let mut ok = true;
    if let Some(host) = host_opt.as_mut() {
        let resize_res = if !prev_init {
            host.rebind_for_dpi_change(canvas_w as u32, canvas_h as u32)
        } else {
            host.resize(canvas_w as u32, canvas_h as u32)
        };
        if resize_res.is_ok()
            && let Ok(ctx) = host.begin_d2d_draw()
        {
            ren.render_d2d(&ctx, &state_snapshot, pointer_pos);
            ok = host.end_d2d_draw_and_present().is_ok();
        } else {
            ok = false;
        }
        if !ok {
            ren.clear_d2d_cache();
            if let Ok(new_host) = DCompHost::new(hwnd, canvas_w as u32, canvas_h as u32) {
                *host = new_host;
                if let Ok(ctx) = host.begin_d2d_draw() {
                    ren.render_d2d(&ctx, &state_snapshot, pointer_pos);
                    ok = host.end_d2d_draw_and_present().is_ok();
                }
            }
        }
    }

    // BH-18: Restore renderer & dcomp to NOTCH_UI BEFORE calling SetWindowPos so any
    // synchronous WM_DPICHANGED re-entrancy can redraw and rebind cleanly instead of
    // aborting on None and having its has_ulw_initialized=false clobbered afterward.
    NOTCH_UI.with_borrow_mut(|ui| {
        ui.renderer = Some(ren);
        ui.dcomp = host_opt;
        if ok && ui.dpi_generation == dpi_gen_before {
            ui.has_ulw_initialized = true;
            ui.prev_dib_w = canvas_w;
            ui.prev_dib_h = canvas_h;
            ui.last_win_x = win_x;
            ui.last_win_y = win_y;
        }
    });

    if force_full {
        // Clear any stale client-space HRGN prior to moving/resizing so the shifted old
        // region never momentarily ejects the mouse cursor and triggers a spurious WM_MOUSELEAVE.
        if size_changed || pos_changed {
            let _ = SetWindowRgn(hwnd, HRGN::default(), false);
            NOTCH_UI.with_borrow_mut(|ui| {
                ui.last_rgn = None;
            });
        }
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            win_x,
            win_y,
            canvas_w,
            canvas_h,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }

    let dpi_changed_during_pos = NOTCH_UI.with_borrow(|ui| ui.dpi_generation != dpi_gen_before);
    if dpi_changed_during_pos {
        // WM_DPICHANGED already ran its own redraw + region + backdrop sync for the new DPI.
        return;
    }
    let _ = ok;
    // Clip the DComp HWND input region to capsule + 2px outward margin:
    // WM_NCHITTEST's HTTRANSPARENT only passes through to same-thread windows,
    // so without SetWindowRgn the full expanded canvas blocks clicks to other
    // apps when minimized. The +2px margin keeps anti-aliased corners unclipped
    // while WM_NCHITTEST enforces sub-pixel precision inside it.
    unsafe {
        sync_notch_window_region(hwnd, &state_snapshot, canvas_w, canvas_h);
    }
    sync_notch_backdrop(hwnd, None);

    // Sync the dedicated click-through physical screen-edge purple glow overlay.
    let glow_intensity = state_snapshot.anim_edge_glow_t;
    let hidden_fs = NOTCH_UI.with_borrow(|ui| ui.hidden_for_fullscreen);
    if glow_intensity > 0.003 && !hidden_fs && !CAPTURE_IN_PROGRESS.load(Ordering::Acquire) {
        let (ox, oy) = Renderer::compute_origin(&state_snapshot, canvas_w as f32, canvas_h as f32);
        let notch_cx = win_x + (ox + state_snapshot.current_width * 0.5).round() as i32;
        let notch_cy = win_y + (oy + state_snapshot.current_height * 0.5).round() as i32;
        let dpi_scale = unsafe {
            let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
            if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 }
        };
        let glow_scale = (state_snapshot.type_scale.ui_scale() * dpi_scale).max(0.5);
        let glow_rect = crate::edge_glow::compute_edge_glow_rect(
            mon,
            state_snapshot.placement,
            notch_cx,
            notch_cy,
            glow_scale,
        );
        let mut glow_slot = NOTCH_UI.with_borrow_mut(|ui| ui.edge_glow.take());
        if glow_slot.is_none() {
            glow_slot = crate::edge_glow::EdgeGlowOverlay::create(glow_rect);
        }
        if let Some(ref mut overlay) = glow_slot {
            overlay.sync(hwnd, glow_rect, state_snapshot.placement, glow_intensity);
        }
        NOTCH_UI.with_borrow_mut(|ui| {
            ui.edge_glow = glow_slot;
        });
    } else {
        NOTCH_UI.with_borrow_mut(|ui| {
            if let Some(ref mut overlay) = ui.edge_glow {
                overlay.hide();
            }
        });
    }
}
