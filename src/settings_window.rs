#![allow(unsafe_op_in_unsafe_fn, clippy::too_many_arguments)]

use crate::backdrop::{self, Backdrop};
use crate::backdrop_geom::{self, ShellGeom};
use crate::composition::DCompHost;
use crate::font::{FontRenderer, Weight};
use crate::icons::{self, Icon};
use crate::models::{
    Action, AppState, ExpandedSizePreset, IdleMetric, LyricSize, LyricStyle, NotchMode,
    NotchPlacement, NotchSizePreset, NotchStyle, SettingsCategory, ToggleId, TypeScale,
};
use crate::notifications;
use crate::painter::{Hit, Painter, fade, lerp_color, rgba, tok};
use crate::renderer::Renderer;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use tiny_skia::{BlendMode, Color, Mask, Pixmap};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

const SETTINGS_CLASS_NAME: PCWSTR = w!("CyNotchSettingsClass");
const SETTINGS_WINDOW_NAME: PCWSTR = w!("CyNotch Preferences");
const TIMER_SETTINGS_ANIM_ID: usize = 31;
const WM_MOUSELEAVE_CODE: u32 = 0x02A3;
pub const WM_APP_SETTINGS_REDRAW: u32 = 0x8000 + 21;
pub const WM_APP_SETTINGS_CLOSE: u32 = 0x8000 + 22;
pub const WM_APP_SETTINGS_HIGHRES_TICK: u32 = 0x8000 + 24;

const PANE_X: f32 = 240.0;
const PANE_Y: f32 = 78.0;
const PANE_W: f32 = 600.0;
const PANE_H: f32 = 620.0;
const PANE_BLEED: f32 = 12.0;

pub static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
static SETTINGS_STATE: Mutex<Option<Arc<Mutex<AppState>>>> = Mutex::new(None);

#[derive(Default)]
struct SettingsUi {
    pixmap: Option<Pixmap>,
    mask: Option<Mask>,
    pane_pm: Option<Pixmap>,
    pane_mask: Option<Mask>,
    fonts: Option<FontRenderer>,
    hits: Vec<Hit>,
    slot_map: Vec<usize>,
    last_max_scroll: f32,
    dcomp: Option<DCompHost>,
    pointer: Option<(f32, f32)>,
    tracking_mouse: bool,
    hover_idx: Option<usize>,
    drag_origin: Option<(POINT, POINT)>,
    timer_ms: u32,
    last_anim: Option<std::time::Instant>,
    /// Paired frosted-blur window (see `backdrop.rs`).
    backdrop: Option<Backdrop>,
    backdrop_wanted: bool,
    backdrop_geom: Option<ShellGeom>,
}

thread_local! {
    static SETTINGS_BACKDROP_SYNCING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    static SETTINGS_UI: RefCell<SettingsUi> = RefCell::new(SettingsUi::default());
}

fn set_settings_state(state: Arc<Mutex<AppState>>) {
    if let Ok(mut slot) = SETTINGS_STATE.lock() {
        *slot = Some(state);
    }
}

fn get_settings_state() -> Option<Arc<Mutex<AppState>>> {
    SETTINGS_STATE.lock().ok().and_then(|slot| slot.clone())
}

pub fn is_open() -> bool {
    SETTINGS_HWND.load(Ordering::Acquire) != 0
}

pub fn post_redraw() {
    let raw = SETTINGS_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_SETTINGS_REDRAW,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn close_if_open() {
    let raw = SETTINGS_HWND.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                HWND(raw as *mut std::ffi::c_void),
                WM_APP_SETTINGS_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Compute the responsive Settings scale clamped safely to the monitor work
/// area so `X-Large` never overflows smaller laptop displays.
fn effective_settings_scale(hwnd: HWND, desired_scale: f32) -> f32 {
    unsafe {
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mon = if !hwnd.0.is_null() {
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
        } else {
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
        };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let work_w = (mi.rcWork.right - mi.rcWork.left) as f32;
            let work_h = (mi.rcWork.bottom - mi.rcWork.top) as f32;
            let max_sw = ((work_w * 0.94) / AppState::SETTINGS_WIDTH).max(0.82);
            let max_sh = ((work_h * 0.94) / AppState::SETTINGS_HEIGHT).max(0.82);
            return desired_scale.min(max_sw).min(max_sh).max(0.85);
        }
    }
    desired_scale
}

pub fn settings_dims_for_state(state: &AppState) -> (u32, u32, f32) {
    let s = state.type_scale.ui_scale();
    (
        (AppState::SETTINGS_WIDTH * s).round().max(1.0) as u32,
        (AppState::SETTINGS_HEIGHT * s).round().max(1.0) as u32,
        s,
    )
}

unsafe fn ensure_settings_buffers(ui: &mut SettingsUi, w: u32, h: u32, pane_w: u32, pane_h: u32) {
    let needs_outer = ui
        .pixmap
        .as_ref()
        .is_none_or(|pm| pm.width() != w || pm.height() != h);
    if needs_outer {
        ui.pixmap = Pixmap::new(w, h);
        ui.mask = Mask::new(w, h);
    } else if ui.mask.is_none() {
        ui.mask = Mask::new(w, h);
    }
    let needs_pane = ui
        .pane_pm
        .as_ref()
        .is_none_or(|pm| pm.width() != pane_w || pm.height() != pane_h);
    if needs_pane {
        ui.pane_pm = Pixmap::new(pane_w, pane_h);
        ui.pane_mask = Mask::new(pane_w, pane_h);
    } else if ui.pane_mask.is_none() {
        ui.pane_mask = Mask::new(pane_w, pane_h);
    }
    if ui.fonts.is_none() {
        ui.fonts = Some(FontRenderer::new());
    }
}

unsafe fn create_settings_hwnd(
    win_x: i32,
    win_y: i32,
    w: i32,
    h: i32,
    h_instance: HMODULE,
) -> std::result::Result<(HWND, DCompHost), ()> {
    if let Ok(hwnd) = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW,
        SETTINGS_CLASS_NAME,
        SETTINGS_WINDOW_NAME,
        WS_POPUP,
        win_x,
        win_y,
        w,
        h,
        None,
        None,
        h_instance,
        None,
    ) {
        if let Ok(host) = DCompHost::new(hwnd, w.max(1) as u32, h.max(1) as u32) {
            return Ok((hwnd, host));
        }
        let _ = DestroyWindow(hwnd);
    }

    Err(())
}

pub fn open_or_focus(state: Arc<Mutex<AppState>>) {
    unsafe {
        let existing = SETTINGS_HWND.load(Ordering::Acquire);
        if existing != 0 {
            let hwnd = HWND(existing as *mut std::ffi::c_void);
            if IsWindow(hwnd).as_bool() {
                set_settings_state(state.clone());
                if let Ok(mut st) = state.lock() {
                    st.settings_open = true;
                    st.windows_transparency_enabled =
                        notifications::is_windows_transparency_enabled();
                    st.anim_settings_cat_y = AppState::settings_cat_pill_y(st.settings_category);
                    st.sync_switches_immediate();
                }
                redraw_settings_with_cache(hwnd, true);
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                );
                let _ = SetForegroundWindow(hwnd);
                return;
            }
            SETTINGS_HWND.store(0, Ordering::Release);
        }

        set_settings_state(state.clone());
        let (desired_scale, cat) = if let Ok(mut st) = state.lock() {
            st.windows_transparency_enabled = notifications::is_windows_transparency_enabled();
            st.settings_open = true;
            st.settings_hover_idx = None;
            st.settings_hover_slots = [0.0; 48];
            st.anim_settings_cat_y = AppState::settings_cat_pill_y(st.settings_category);
            st.sync_switches_immediate();
            (st.type_scale.ui_scale(), st.settings_category)
        } else {
            (1.0, SettingsCategory::General)
        };
        let s = effective_settings_scale(HWND::default(), desired_scale);
        let w = (AppState::SETTINGS_WIDTH * s).round().max(1.0) as u32;
        let h = (AppState::SETTINGS_HEIGHT * s).round().max(1.0) as u32;
        let pane_w = ((PANE_W * s).round() + (PANE_BLEED * s).round() * 2.0).max(1.0) as u32;
        let full_pane_h = (pane_content_height(cat) + 10.0).max(PANE_H);
        let pane_h = (full_pane_h * s).round().max(1.0) as u32;
        SETTINGS_UI.with_borrow_mut(|ui| {
            ensure_settings_buffers(ui, w, h, pane_w, pane_h);
        });

        let Ok(h_instance) = GetModuleHandleW(None) else {
            return;
        };

        if !CLASS_REGISTERED.swap(true, Ordering::AcqRel) {
            let app_icon = crate::window::load_app_icon(32);
            let app_icon_sm = crate::window::load_app_icon(16);
            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(settings_wnd_proc),
                hInstance: h_instance.into(),
                hIcon: app_icon,
                hIconSm: app_icon_sm,
                lpszClassName: SETTINGS_CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wnd_class);
        }

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let win_x = ((screen_w - w as i32) / 2).max(16);
        let win_y = ((screen_h - h as i32) / 2).max(16);

        if let Ok((hwnd, host)) = create_settings_hwnd(win_x, win_y, w as i32, h as i32, h_instance)
        {
            SETTINGS_UI.with_borrow_mut(|ui| {
                ui.dcomp = Some(host);
            });
            SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Release);
            let force_disable: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &force_disable as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );
            crate::borders::set_window_borderless(hwnd);
            redraw_settings(hwnd);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                win_x,
                win_y,
                w as i32,
                h as i32,
                SWP_SHOWWINDOW,
            );
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

unsafe fn tick_settings_animation(hwnd: HWND) {
    let now = std::time::Instant::now();
    let dt = SETTINGS_UI.with_borrow_mut(|ui| {
        let d = if let Some(prev) = ui.last_anim {
            now.duration_since(prev).as_secs_f32().clamp(0.001, 0.033)
        } else {
            0.016
        };
        ui.last_anim = Some(now);
        d
    });

    let mut changed = false;
    let mut desired_ms = None;
    if let Some(state_mutex) = get_settings_state()
        && let Ok(mut st) = state_mutex.lock()
    {
        changed |= st.update_settings_animation(dt);
        desired_ms = st.needs_settings_timer_ms();
    }
    sync_settings_timer(hwnd, desired_ms);
    if changed {
        redraw_settings_with_cache(hwnd, true);
    }
}

unsafe fn sync_settings_timer(hwnd: HWND, desired_ms: Option<u32>) {
    let target = desired_ms.unwrap_or(0);
    crate::window::set_high_res_slot(2, target != 0 && target <= 30);
    let action = SETTINGS_UI.with_borrow_mut(|ui| {
        if target == ui.timer_ms {
            None
        } else {
            let prev = ui.timer_ms;
            if target == 0 {
                ui.last_anim = None;
            } else if prev == 0 {
                ui.last_anim = Some(std::time::Instant::now());
            }
            ui.timer_ms = target;
            Some(target)
        }
    });
    if let Some(ms) = action {
        if ms == 0 {
            let _ = KillTimer(hwnd, TIMER_SETTINGS_ANIM_ID);
            crate::window::set_settings_pacer(hwnd, 0);
        } else if ms <= 30 {
            let _ = KillTimer(hwnd, TIMER_SETTINGS_ANIM_ID);
            crate::window::set_settings_pacer(hwnd, ms);
        } else {
            crate::window::set_settings_pacer(hwnd, 0);
            SetTimer(hwnd, TIMER_SETTINGS_ANIM_ID, ms, None);
        }
    }
}

/// Keeps the paired blur window under the Preferences window. Same contract as
/// `window::sync_notch_backdrop`: no `AppState` lock, skips if `SETTINGS_UI`
/// is mid-borrow, ignores re-entrancy.
unsafe fn sync_settings_backdrop(hwnd: HWND, visible_override: Option<bool>) {
    if SETTINGS_BACKDROP_SYNCING.get() {
        return;
    }
    SETTINGS_BACKDROP_SYNCING.set(true);
    sync_settings_backdrop_inner(hwnd, visible_override);
    SETTINGS_BACKDROP_SYNCING.set(false);
}

unsafe fn sync_settings_backdrop_inner(hwnd: HWND, visible_override: Option<bool>) {
    let snap = SETTINGS_UI
        .try_with(|c| {
            c.try_borrow()
                .ok()
                .map(|ui| (ui.backdrop_wanted, ui.backdrop_geom, ui.backdrop.is_some()))
        })
        .ok()
        .flatten();
    let Some((wanted, geom, has)) = snap else {
        return;
    };
    if !wanted && !has {
        return;
    }
    let visible = visible_override.unwrap_or_else(|| IsWindowVisible(hwnd).as_bool());
    let placement = if wanted && visible {
        geom.and_then(|g| {
            let mut rc = RECT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() {
                backdrop_geom::placement(rc.left, rc.top, &g, backdrop::REST_INSET)
            } else {
                None
            }
        })
    } else {
        None
    };
    let taken = SETTINGS_UI
        .try_with(|c| c.try_borrow_mut().ok().map(|mut ui| ui.backdrop.take()))
        .ok()
        .flatten();
    let Some(mut slot) = taken else {
        return;
    };
    let failed = backdrop::sync_slot(&mut slot, hwnd, placement);
    let _ = SETTINGS_UI.try_with(|c| {
        if let Ok(mut ui) = c.try_borrow_mut() {
            ui.backdrop = slot;
        }
    });
    if failed {
        crate::window::post_redraw();
        post_redraw();
    }
}

unsafe fn redraw_settings(hwnd: HWND) {
    redraw_settings_with_cache(hwnd, false);
}

unsafe fn redraw_settings_with_cache(hwnd: HWND, allow_pane_cache: bool) {
    PANE_CACHE_ALLOWED.with(|c| c.set(allow_pane_cache));
    let (snapshot, desired_ms) = if let Some(state_mutex) = get_settings_state() {
        if let Ok(st) = state_mutex.lock() {
            (st.clone(), st.needs_settings_timer_ms())
        } else {
            return;
        }
    } else {
        return;
    };

    sync_settings_timer(hwnd, desired_ms);

    let s = effective_settings_scale(hwnd, snapshot.type_scale.ui_scale());
    let int_w = (AppState::SETTINGS_WIDTH * s).round().max(1.0) as i32;
    let int_h = (AppState::SETTINGS_HEIGHT * s).round().max(1.0) as i32;
    let pane_w_px = ((PANE_W * s).round() + (PANE_BLEED * s).round() * 2.0).max(1.0) as u32;
    let full_pane_h = (pane_content_height(snapshot.settings_category) + 10.0).max(PANE_H);
    let pane_h_px = (full_pane_h * s).round().max(1.0) as u32;

    {
        // Cache the window silhouette before any SetWindowPos below so the
        // WM_WINDOWPOSCHANGED hook positions the blur from fresh numbers.
        backdrop::set_blur_px(snapshot.frosted_blur_px);
        let wanted = snapshot.is_frosted_active() && backdrop::blur_available();
        let geom = ShellGeom::whole_window(int_w, int_h, 10.0 * s);
        SETTINGS_UI.with_borrow_mut(|ui| {
            ui.backdrop_wanted = wanted;
            ui.backdrop_geom = Some(geom);
        });
    }

    let mut cur_rc = RECT::default();
    if GetWindowRect(hwnd, &mut cur_rc).is_ok() {
        let cur_w = cur_rc.right - cur_rc.left;
        let cur_h = cur_rc.bottom - cur_rc.top;
        if cur_w != int_w || cur_h != int_h {
            let cx = cur_rc.left + cur_w / 2;
            let cy = cur_rc.top + cur_h / 2;
            let mut nx = cx - int_w / 2;
            let mut ny = cy - int_h / 2;
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                nx = nx.clamp(
                    mi.rcWork.left,
                    (mi.rcWork.right - int_w).max(mi.rcWork.left),
                );
                ny = ny.clamp(mi.rcWork.top, (mi.rcWork.bottom - int_h).max(mi.rcWork.top));
            }
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                nx,
                ny,
                int_w,
                int_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    SETTINGS_UI.with_borrow_mut(|ui| {
        ensure_settings_buffers(ui, int_w as u32, int_h as u32, pane_w_px, pane_h_px);
        let SettingsUi {
            ref mut pixmap,
            ref mut mask,
            ref mut pane_pm,
            ref mut pane_mask,
            ref mut fonts,
            ref mut hits,
            ref mut slot_map,
            ref mut last_max_scroll,
            ref mut dcomp,
            pointer,
            ..
        } = *ui;
        let (Some(pm), Some(mask), Some(ppm), Some(pmask), Some(fonts)) = (
            pixmap.as_mut(),
            mask.as_mut(),
            pane_pm.as_mut(),
            pane_mask.as_mut(),
            fonts.as_mut(),
        ) else {
            return;
        };
        let (new_hits, new_slots, max_scroll) =
            render_settings_pixmap_into(&snapshot, pointer, pm, mask, ppm, pmask, fonts);
        PANE_CACHE_ALLOWED.with(|c| c.set(true));
        *hits = new_hits;
        *slot_map = new_slots;
        *last_max_scroll = max_scroll;

        if let Some(host) = dcomp.as_mut() {
            let _ = host.present_rgba_premultiplied(pm.data(), int_w as u32, int_h as u32);
        }
    });
    sync_settings_backdrop(hwnd, None);
}

unsafe fn refresh_hover_after_redraw(hwnd: HWND) {
    let refreshed_slot = SETTINGS_UI.with_borrow_mut(|ui| {
        let r = ui.pointer.and_then(|(mx, my)| {
            ui.hits.iter().position(|h| {
                mx >= h.rect.x()
                    && mx <= h.rect.x() + h.rect.width()
                    && my >= h.rect.y()
                    && my <= h.rect.y() + h.rect.height()
            })
        });
        let slot = r.and_then(|idx| ui.slot_map.get(idx).copied());
        ui.hover_idx = slot;
        slot
    });
    let mut post_ms = None;
    if let Some(state_mutex) = get_settings_state()
        && let Ok(mut st) = state_mutex.lock()
    {
        st.settings_hover_idx = refreshed_slot;
        if let Some(i) = refreshed_slot
            && st.micro_interactions
            && i < st.settings_hover_slots.len()
        {
            st.settings_hover_slots[i] = st.settings_hover_slots[i].max(0.02);
        }
        post_ms = st.needs_settings_timer_ms();
    }
    sync_settings_timer(hwnd, post_ms);
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings_wnd_proc_inner(hwnd, msg, wparam, lparam)
    })) {
        Ok(res) => res,
        Err(_) => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn hide_settings_window(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_HIDE);
    crate::window::clear_settings_pacer_in_flight();
    SETTINGS_UI.with_borrow_mut(|ui| {
        if ui.drag_origin.take().is_some() {
            let _ = ReleaseCapture();
        }
    });
    let _ = DestroyWindow(hwnd);
}

unsafe fn settings_wnd_proc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_WINDOWPOSCHANGED => {
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            sync_settings_backdrop(hwnd, None);
            r
        }
        WM_SHOWWINDOW => {
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            // IsWindowVisible is not yet updated inside this message: use wparam.
            sync_settings_backdrop(hwnd, Some(wparam.0 != 0));
            r
        }
        WM_APP_SETTINGS_REDRAW => {
            redraw_settings_with_cache(hwnd, true);
            LRESULT(0)
        }
        WM_APP_SETTINGS_CLOSE | WM_CLOSE => {
            hide_settings_window(hwnd);
            LRESULT(0)
        }
        WM_APP_SETTINGS_HIGHRES_TICK => {
            tick_settings_animation(hwnd);
            crate::window::clear_settings_pacer_in_flight();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let win_trans = notifications::is_windows_transparency_enabled();
            if let Some(state_mutex) = get_settings_state()
                && let Ok(mut st) = state_mutex.lock()
            {
                st.windows_transparency_enabled = win_trans;
            }
            redraw_settings(hwnd);
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let (w, h) = SETTINGS_UI.with_borrow(|ui| {
                ui.pixmap
                    .as_ref()
                    .map(|p| (p.width() as i32, p.height() as i32))
                    .unwrap_or((
                        AppState::SETTINGS_WIDTH as i32,
                        AppState::SETTINGS_HEIGHT as i32,
                    ))
            });
            let sx = (lparam.0 & 0xffff) as i16 as i32;
            let sy = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let mut pt = POINT { x: sx, y: sy };
            let _ = ScreenToClient(hwnd, &mut pt);
            if pt.x >= 0 && pt.x < w && pt.y >= 0 && pt.y < h {
                return LRESULT(1); // HTCLIENT
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as u16 as i16) as f32;
            let scroll_delta = (-delta / 120.0) * 96.0;
            let max_scroll = SETTINGS_UI.with_borrow(|ui| ui.last_max_scroll);
            let mut moved = false;
            if let Some(state_mutex) = get_settings_state()
                && let Ok(mut st) = state_mutex.lock()
            {
                let cat = st.settings_category;
                let cur = st.settings_scroll(cat);
                let clamped = (cur + scroll_delta).clamp(0.0, max_scroll.max(0.0));
                if (clamped - cur).abs() > 0.1 {
                    st.set_settings_scroll(cat, clamped);
                    moved = true;
                }
            }
            if moved {
                redraw_settings_with_cache(hwnd, true);
                refresh_hover_after_redraw(hwnd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u16 == VK_ESCAPE.0 {
                hide_settings_window(hwnd);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let px = (lparam.0 & 0xffff) as i16 as f32;
            let py = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
            let drag_origin = SETTINGS_UI.with_borrow_mut(|ui| {
                ui.pointer = Some((px, py));
                ui.drag_origin
            });

            if let Some((start_cur, start_win)) = drag_origin {
                let mut cur = POINT::default();
                if GetCursorPos(&mut cur).is_ok() {
                    let nx = start_win.x + (cur.x - start_cur.x);
                    let ny = start_win.y + (cur.y - start_cur.y);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        nx,
                        ny,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                return LRESULT(0);
            }

            let need_track = SETTINGS_UI.with_borrow_mut(|ui| {
                if !ui.tracking_mouse {
                    ui.tracking_mouse = true;
                    true
                } else {
                    false
                }
            });
            if need_track {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }

            let (new_slot, idx_changed) = SETTINGS_UI.with_borrow_mut(|ui| {
                let raw_pos = ui.hits.iter().position(|h| {
                    px >= h.rect.x()
                        && px <= h.rect.x() + h.rect.width()
                        && py >= h.rect.y()
                        && py <= h.rect.y() + h.rect.height()
                });
                let slot = raw_pos.and_then(|idx| ui.slot_map.get(idx).copied());
                let changed = slot != ui.hover_idx;
                if changed {
                    ui.hover_idx = slot;
                }
                (slot, changed)
            });

            if idx_changed {
                if let Some(state_mutex) = get_settings_state()
                    && let Ok(mut st) = state_mutex.lock()
                {
                    st.settings_hover_idx = new_slot;
                    if let Some(i) = new_slot
                        && st.micro_interactions
                        && i < st.settings_hover_slots.len()
                    {
                        st.settings_hover_slots[i] = st.settings_hover_slots[i].max(0.02);
                    }
                }
                redraw_settings_with_cache(hwnd, true);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE_CODE => {
            SETTINGS_UI.with_borrow_mut(|ui| {
                ui.tracking_mouse = false;
                ui.pointer = None;
                ui.hover_idx = None;
            });
            if let Some(state_mutex) = get_settings_state()
                && let Ok(mut st) = state_mutex.lock()
            {
                st.settings_hover_idx = None;
            }
            redraw_settings_with_cache(hwnd, true);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let was_dragging = SETTINGS_UI.with_borrow_mut(|ui| ui.drag_origin.take().is_some());
            if was_dragging {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let px = (lparam.0 & 0xffff) as i16 as f32;
            let py = ((lparam.0 >> 16) & 0xffff) as i16 as f32;

            let hit_action = SETTINGS_UI.with_borrow(|ui| {
                ui.hits
                    .iter()
                    .find(|h| {
                        px >= h.rect.x()
                            && px <= h.rect.x() + h.rect.width()
                            && py >= h.rect.y()
                            && py <= h.rect.y() + h.rect.height()
                    })
                    .map(|h| h.action)
            });

            if let Some(act) = hit_action {
                let mut effects = None;
                if let Some(state_mutex) = get_settings_state()
                    && let Ok(mut st) = state_mutex.lock()
                {
                    effects = st.apply_setting_action(act);
                }
                let active_hwnd = hwnd;
                if let Some(eff) = effects {
                    if let Some(enabled) = eff.update_autostart {
                        crate::autostart::set_autostart(enabled);
                    }
                    if eff.reset_screentime {
                        crate::screentime::RESET_SCREENTIME_REQUESTED
                            .store(true, Ordering::Release);
                    }
                    if eff.trigger_purge {
                        crate::window::request_purge();
                    }
                    if eff.open_win_notif_settings {
                        let needs_privacy = get_settings_state()
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
                        crate::window::NOTIF_SYNC_REQUESTED.store(true, Ordering::Release);
                    }
                    if eff.open_win_personalize_colors {
                        notifications::open_windows_personalize_colors();
                    }
                    if eff.request_notif_access {
                        crate::window::NOTIF_SYNC_REQUESTED.store(true, Ordering::Release);
                    }
                    if let Some(exclusive) = eff.sync_exclusive_banners
                        && let Some(st_arc) = get_settings_state()
                    {
                        std::thread::spawn(move || {
                            let count = notifications::set_exclusive_windows_banners(exclusive);
                            if let Ok(mut st) = st_arc.lock() {
                                st.notif_suppressed_apps = count;
                            }
                            post_redraw();
                        });
                    }
                    if let Some(rm_borders) = eff.sync_windows_borders {
                        crate::borders::set_enabled(rm_borders);
                    }
                    if eff.sync_frosted_window
                        && let Some(st_arc) = get_settings_state()
                        && let Ok(mut st) = st_arc.lock()
                    {
                        st.windows_transparency_enabled =
                            notifications::is_windows_transparency_enabled();
                    }
                    if eff.redraw_notch {
                        crate::window::post_redraw();
                    }
                    if eff.quit_app {
                        let _ = DestroyWindow(active_hwnd);
                        crate::window::request_quit();
                        return LRESULT(0);
                    }
                    if eff.close_settings {
                        hide_settings_window(active_hwnd);
                        return LRESULT(0);
                    }
                }
                redraw_settings(active_hwnd);
                refresh_hover_after_redraw(active_hwnd);
            } else {
                let drag_h = SETTINGS_UI.with_borrow(|ui| {
                    ui.pixmap
                        .as_ref()
                        .map(|p| 64.0 * (p.width() as f32 / AppState::SETTINGS_WIDTH))
                        .unwrap_or(64.0)
                });
                if py < drag_h {
                    let mut cur = POINT::default();
                    let mut rc = RECT::default();
                    if GetCursorPos(&mut cur).is_ok() && GetWindowRect(hwnd, &mut rc).is_ok() {
                        SETTINGS_UI.with_borrow_mut(|ui| {
                            ui.drag_origin = Some((
                                cur,
                                POINT {
                                    x: rc.left,
                                    y: rc.top,
                                },
                            ));
                        });
                        SetCapture(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_SETTINGS_ANIM_ID {
                tick_settings_animation(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            SETTINGS_HWND.store(0, Ordering::Release);
            let _ = KillTimer(hwnd, TIMER_SETTINGS_ANIM_ID);
            crate::window::set_settings_pacer(hwnd, 0);
            crate::window::clear_settings_pacer_in_flight();
            crate::window::set_high_res_slot(2, false);
            SETTINGS_UI.with_borrow_mut(|ui| {
                *ui = SettingsUi::default();
            });
            PANE_SCRATCH.with_borrow_mut(|slot| *slot = None);
            SETTINGS_CHROME.with_borrow_mut(|slot| *slot = None);
            PANE_CACHE.with_borrow_mut(|slot| *slot = None);
            if let Some(state_mutex) = get_settings_state()
                && let Ok(mut st) = state_mutex.lock()
            {
                st.settings_open = false;
                st.settings_hover_idx = None;
                st.settings_hover_slots = [0.0; 48];
            }
            crate::window::post_redraw();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

const PAD: f32 = 24.0;
const CARD_GAP: f32 = 16.0;
const SEG_PITCH: f32 = 42.0;
const TOGGLE_PITCH: f32 = 48.0;
const CTRL_X_OFF: f32 = 188.0;

fn pane_content_height(cat: SettingsCategory) -> f32 {
    match cat {
        SettingsCategory::General => 274.0 + CARD_GAP + 580.0,
        SettingsCategory::Motion => 212.0 + CARD_GAP + 172.0,
        SettingsCategory::Modules => 270.0 + CARD_GAP + 158.0 + CARD_GAP + 214.0,
        SettingsCategory::Notifications => 228.0 + CARD_GAP + 250.0,
        SettingsCategory::System => 502.0 + CARD_GAP + 194.0,
    }
}

/// Pure, testable Preferences UI renderer for the 860×720 Obsidian Glass window.
pub fn render_settings_pixmap(
    state: &AppState,
    pointer: Option<(f32, f32)>,
    pixmap: &mut Pixmap,
    clip_mask: &mut Mask,
    fonts: &mut FontRenderer,
) -> Vec<Hit> {
    let (hits, _max_scroll) =
        render_settings_pixmap_with_meta(state, pointer, pixmap, clip_mask, fonts);
    hits
}

pub fn render_settings_pixmap_with_meta(
    state: &AppState,
    pointer: Option<(f32, f32)>,
    pixmap: &mut Pixmap,
    clip_mask: &mut Mask,
    fonts: &mut FontRenderer,
) -> (Vec<Hit>, f32) {
    let s = (pixmap.width() as f32 / AppState::SETTINGS_WIDTH)
        .min(pixmap.height() as f32 / AppState::SETTINGS_HEIGHT)
        .max(0.5);
    let pane_w = ((PANE_W * s).round() + (PANE_BLEED * s).round() * 2.0).max(1.0) as u32;
    let full_pane_h = ((pane_content_height(state.settings_category) + 10.0).max(PANE_H) * s)
        .round()
        .max(1.0) as u32;
    // Reuse the scroll-pane scratch surface across frames: a fresh Pixmap+Mask
    // pair here costs a multi-megabyte alloc/zero on every rendered frame.
    pane_scratch(pane_w, full_pane_h, |pane_pm, pane_mask| {
        let (hits, _slot_map, max_scroll) = render_settings_pixmap_into(
            state, pointer, pixmap, clip_mask, pane_pm, pane_mask, fonts,
        );
        (hits, max_scroll)
    })
}

/// Identity of the cached settings chrome surface: pixel size, type scale,
/// active category, frost mode/opacity and sidebar hover state.
type ChromeKey = (u32, u32, u8, u8, bool, u32, [u32; 5]);

#[derive(PartialEq)]
struct PaneCacheKey {
    cat: SettingsCategory,
    w: u32,
    h: u32,
    type_scale: u8,
    state_sig: u64,
    pointer_px: Option<(i32, i32)>,
    switch_bits: [u32; 48],
    hover_bits: [u32; 48],
}

struct CachedPane {
    key: PaneCacheKey,
    pm: Pixmap,
    hits: Vec<Hit>,
}

fn compute_pane_state_sig(state: &AppState) -> u64 {
    let mut h = DefaultHasher::new();
    (
        state.notch_style as u8,
        state.placement as u8,
        state.size_preset as u8,
        state.expanded_size as u8,
        state.is_pinned,
        state.x_offset,
        state.y_offset,
    )
        .hash(&mut h);
    (
        state.idle_metric as u8,
        state.type_scale as u8,
        state.windows_transparency_enabled,
        state.frosted_alpha_pct(),
        state.remove_windows_borders,
    )
        .hash(&mut h);
    (
        state.fps_cap,
        state.detected_hz,
        state.micro_interactions,
        state.equalizer_enabled,
    )
        .hash(&mut h);
    (
        state.notification_secs,
        state.notif_listener_enabled,
        &state.notif_listener_status,
        state.notif_exclusive_mode,
        state.notif_suppressed_apps,
    )
        .hash(&mut h);
    (state.lyric_size as u8, state.lyric_style as u8).hash(&mut h);
    if state.settings_category == SettingsCategory::System {
        (
            state.telemetry.has_admin_privileges,
            state.telemetry.memory_used_gb.to_bits(),
            state.telemetry.memory_total_gb.to_bits(),
            state.telemetry.memory_percent.to_bits(),
            state.telemetry.pagefile_used_gb.to_bits(),
            state.telemetry.pagefile_total_gb.to_bits(),
            state.telemetry.pagefile_percent.to_bits(),
            state.telemetry.sys_cache_mb.to_bits(),
            state.telemetry.sys_cache_percent.to_bits(),
        )
            .hash(&mut h);
        (
            state.mem_reduct.show_clean_results,
            state.mem_reduct.auto_clean_enabled,
            state.mem_reduct.when_above_pct,
            state.mem_reduct.every_mins,
            &state.mem_reduct.last_cleaned_status,
            state.mem_reduct.is_purging,
        )
            .hash(&mut h);
    }
    for id in ToggleId::ALL {
        state.toggle_target(id).hash(&mut h);
    }
    h.finish()
}

#[inline]
fn blit_div255(v: u32) -> u8 {
    let t = v + 128;
    ((t + (t >> 8)) >> 8) as u8
}

fn blit_pane_slice(
    dst: &mut Pixmap,
    src: &Pixmap,
    dst_x: i32,
    dst_y: i32,
    src_y: i32,
    slice_h: i32,
) {
    let dw = dst.width() as i32;
    let dh = dst.height() as i32;
    let sw = src.width() as i32;
    let sh = src.height() as i32;

    let sy0 = src_y.clamp(0, sh);
    let sy1 = (src_y + slice_h).clamp(sy0, sh);
    let vis_h = sy1 - sy0;
    if vis_h <= 0 {
        return;
    }

    let x0 = dst_x.max(0);
    let y0 = dst_y.max(0);
    let x1 = (dst_x + sw).min(dw);
    let y1 = (dst_y + vis_h).min(dh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let copy_cols = (x1 - x0) as usize;
    let sx_start = (x0 - dst_x) as usize;
    let sy_start = (sy0 + (y0 - dst_y)) as usize;
    let dst_stride = dst.width() as usize * 4;
    let src_stride = src.width() as usize * 4;
    let dst_data = dst.data_mut();
    let src_data = src.data();

    for row in 0..(y1 - y0) as usize {
        let s_row_off = (sy_start + row) * src_stride + sx_start * 4;
        let d_row_off = (y0 as usize + row) * dst_stride + (x0 as usize) * 4;
        let s_row = &src_data[s_row_off..s_row_off + copy_cols * 4];
        let d_row = &mut dst_data[d_row_off..d_row_off + copy_cols * 4];
        for (dp, sp) in d_row.chunks_exact_mut(4).zip(s_row.chunks_exact(4)) {
            let sa = sp[3];
            if sa == 0 {
                continue;
            }
            if sa == 255 {
                dp.copy_from_slice(sp);
            } else {
                let inv_a = (255 - sa) as u32;
                dp[0] = sp[0].saturating_add(blit_div255(dp[0] as u32 * inv_a));
                dp[1] = sp[1].saturating_add(blit_div255(dp[1] as u32 * inv_a));
                dp[2] = sp[2].saturating_add(blit_div255(dp[2] as u32 * inv_a));
                dp[3] = sa.saturating_add(blit_div255(dp[3] as u32 * inv_a));
            }
        }
    }
}

thread_local! {
    static PANE_SCRATCH: std::cell::RefCell<Option<(Pixmap, Mask, u32, u32)>> =
        const { std::cell::RefCell::new(None) };
    /// Pixel-exact copy of the outer shell + aurora wash + static sidebar chrome,
    /// keyed by window size, frost mode/opacity and type scale.
    static SETTINGS_CHROME: std::cell::RefCell<Option<(Pixmap, ChromeKey)>> =
        const { std::cell::RefCell::new(None) };
    static PANE_CACHE: std::cell::RefCell<Option<CachedPane>> =
        const { std::cell::RefCell::new(None) };
    static PANE_CACHE_ALLOWED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn pane_scratch<R>(w: u32, h: u32, f: impl FnOnce(&mut Pixmap, &mut Mask) -> R) -> R {
    PANE_SCRATCH.with_borrow_mut(|slot| {
        let needs_new = match slot {
            Some((pm, mask, sw, sh)) => {
                *sw != w
                    || *sh != h
                    || pm.width() != w
                    || pm.height() != h
                    || mask.width() != w
                    || mask.height() != h
            }
            None => true,
        };
        if needs_new {
            let pm = Pixmap::new(w, h).expect("pane pixmap");
            let mask = Mask::new(w, h).expect("pane mask");
            *slot = Some((pm, mask, w, h));
        }
        let (pm, mask, _, _) = slot.as_mut().expect("pane scratch initialized");
        f(pm, mask)
    })
}

fn render_settings_pixmap_into(
    state: &AppState,
    pointer: Option<(f32, f32)>,
    pixmap: &mut Pixmap,
    clip_mask: &mut Mask,
    pane_pm: &mut Pixmap,
    pane_mask: &mut Mask,
    fonts: &mut FontRenderer,
) -> (Vec<Hit>, Vec<usize>, f32) {
    let s = (pixmap.width() as f32 / AppState::SETTINGS_WIDTH)
        .min(pixmap.height() as f32 / AppState::SETTINGS_HEIGHT)
        .max(0.5);
    let w = AppState::SETTINGS_WIDTH;
    let h = AppState::SETTINGS_HEIGHT;
    let r = 10.0;
    let frosted = state.is_frosted_active();

    let pane_top_pad = 4.0;
    let pane_bot_pad = 6.0;
    let content_h = pane_content_height(state.settings_category);
    let max_scroll = (content_h + pane_top_pad + pane_bot_pad - PANE_H).max(0.0);
    let scroll_y = state
        .settings_scroll(state.settings_category)
        .clamp(0.0, max_scroll);

    let mut hits = Vec::new();
    let mut slot_map = Vec::new();

    // The outer shell + atmospheric aurora wash never change while the window
    // keeps its size/scale, yet they are the most expensive primitives in the
    // frame (three full-surface gradient glows). Keep a pixel-exact copy and
    // memcpy it back instead of re-rasterizing it on every redraw.
    let mut sidebar_hover_bits = [0u32; 5];
    for (idx, dst) in sidebar_hover_bits.iter_mut().enumerate() {
        let slot_idx = 1 + idx;
        if slot_idx < state.settings_hover_slots.len() {
            *dst = state.settings_hover_slots[slot_idx].to_bits();
        }
    }
    let chrome_key: ChromeKey = (
        pixmap.width(),
        pixmap.height(),
        state.type_scale as u8,
        state.settings_category as u8,
        frosted,
        state.frosted_alpha_pct(),
        sidebar_hover_bits,
    );
    let chrome_hit = SETTINGS_CHROME.with_borrow(|slot| {
        matches!(slot, Some((pm, key)) if *key == chrome_key && pm.data().len() == pixmap.data().len())
    });
    if chrome_hit {
        SETTINGS_CHROME.with_borrow(|slot| {
            if let Some((pm, _)) = slot.as_ref() {
                pixmap.data_mut().copy_from_slice(pm.data());
            }
        });
    } else {
        pixmap.fill(Color::TRANSPARENT);
    }

    {
        let mut p = Painter {
            pm: pixmap,
            clip_mask,
            fonts,
            s,
            origin_x: 0.0,
            origin_y: 0.0,
            pointer,
            hits: &mut hits,
            interactive: true,
            global_alpha: 1.0,
            hover_slots: state.settings_hover_slots,
            micro_interactions: state.micro_interactions,
            frosted,
            frosted_opacity_pct: state.frosted_alpha_pct(),
            type_scale: state.type_scale,
            clip_bounds: None,
        };

        let well_col = if frosted {
            tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
        } else {
            tok::surface_well()
        };
        let sb_x = 20.0;
        let sb_y = 78.0;
        let sb_w = 204.0;
        let sb_h = 620.0;

        // 1. Volumetric Obsidian Outer Shell + Static Header Text & Sidebar Shell
        if !chrome_hit {
            let (shell_top, shell_bot) = if frosted {
                (
                    tok::scale_frosted(tok::obsidian_shell_frosted(), state.frosted_alpha_pct()),
                    tok::scale_frosted(tok::obsidian_deep_frosted(), state.frosted_alpha_pct()),
                )
            } else {
                (tok::obsidian_shell(), tok::obsidian_deep())
            };
            p.rrect_vertical_gradient(0.0, 0.0, w, h, r, shell_top, shell_bot);
            let glow_scale = if frosted {
                (state.frosted_alpha_pct() as f32 / 68.0).clamp(0.25, 1.0)
            } else {
                1.0
            };
            if p.prepare_rrect_clip(0.0, 0.0, w, h, r) {
                p.radial_glow(
                    92.0,
                    40.0,
                    184.0,
                    fade(rgba(124, 58, 237, 46), glow_scale),
                    BlendMode::SourceOver,
                    true,
                );
                p.radial_glow(
                    w - 72.0,
                    h - 32.0,
                    168.0,
                    fade(rgba(124, 58, 237, 48), glow_scale),
                    BlendMode::SourceOver,
                    true,
                );
                p.elliptical_glow(
                    w * 0.5,
                    h + 12.0,
                    w * 0.58,
                    132.0,
                    fade(rgba(168, 85, 247, 36), glow_scale),
                    BlendMode::Plus,
                    true,
                );
                p.radial_glow(
                    w * 0.5,
                    -14.0,
                    w * 0.45,
                    fade(rgba(235, 230, 255, 20), glow_scale),
                    BlendMode::Plus,
                    true,
                );
            }
            p.text(
                "CyNotch Preferences",
                62.0,
                28.0,
                15.2,
                Weight::Bold,
                tok::text1(),
            );
            p.text(
                "Mahamatra Command Center · v0.2.4.1",
                62.0,
                45.5,
                10.2,
                Weight::Regular,
                tok::text2(),
            );
            p.rrect(20.0, 64.0, w - 40.0, 1.0, 0.0, rgba(255, 255, 255, 18));

            // Static Left Category Sidebar Card Shell + Sections Header + Global Hotkeys Footer
            p.elevated_card(
                sb_x,
                sb_y,
                sb_w,
                sb_h,
                16.0,
                Some((48.0, 34.0, 74.0, tok::cyno_violet())),
            );
            p.text(
                "SECTIONS",
                sb_x + 18.0,
                sb_y + 28.0,
                9.5,
                Weight::Bold,
                tok::text3(),
            );

            let foot_y = sb_y + sb_h - 122.0;
            let foot_h = 108.0;
            p.rrect(sb_x + 12.0, foot_y, sb_w - 24.0, foot_h, 12.0, well_col);
            p.rrect_bevel_stroke(
                sb_x + 12.0,
                foot_y,
                sb_w - 24.0,
                foot_h,
                12.0,
                1.0,
                rgba(255, 255, 255, 26),
                rgba(255, 255, 255, 10),
            );
            p.text(
                "Global Hotkeys",
                sb_x + 22.0,
                foot_y + 23.0,
                10.5,
                Weight::Bold,
                tok::cyno_mist(),
            );
            p.text(
                "Ctrl+Shift+C · Toggle",
                sb_x + 22.0,
                foot_y + 46.0,
                9.8,
                Weight::Regular,
                tok::text2(),
            );
            p.text(
                "Ctrl+Shift+M · Purge",
                sb_x + 22.0,
                foot_y + 66.0,
                9.8,
                Weight::Regular,
                tok::text2(),
            );
            p.text(
                "Ctrl+Shift+S · Capture",
                sb_x + 22.0,
                foot_y + 86.0,
                9.8,
                Weight::Regular,
                tok::text2(),
            );

            // Static + Category-Keyed Sidebar Pills (cached across scroll frames)
            let pill_x = sb_x + 12.0;
            let pill_w = sb_w - 24.0;
            let pill_h = 44.0;
            let active_pill_y = AppState::settings_cat_pill_y(state.settings_category);
            p.luminous_pill_button_t(
                pill_x,
                active_pill_y,
                pill_w,
                pill_h,
                11.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                0.0,
            );
            for (idx, cat) in SettingsCategory::ALL.into_iter().enumerate() {
                let iy = AppState::settings_cat_pill_y(cat);
                let slot_idx = 1 + idx;
                let ht = if slot_idx < state.settings_hover_slots.len() {
                    state.settings_hover_slots[slot_idx]
                } else {
                    0.0
                };
                let under_pill = (1.0 - ((active_pill_y - iy).abs() / 52.0)).clamp(0.0, 1.0);
                if under_pill < 0.5 && ht > 0.02 {
                    p.rrect(
                        pill_x,
                        iy - 0.35 * ht,
                        pill_w,
                        pill_h,
                        11.0,
                        fade(rgba(255, 255, 255, 20), ht),
                    );
                }
                let ic = match cat {
                    SettingsCategory::General => Icon::Display,
                    SettingsCategory::Motion => Icon::Activity,
                    SettingsCategory::Modules => Icon::List,
                    SettingsCategory::Notifications => Icon::Bell,
                    SettingsCategory::System => Icon::Settings,
                };
                let rest_col = lerp_color(tok::text2(), tok::text1(), ht);
                let col = lerp_color(rest_col, tok::text1(), under_pill);
                icons::draw(&mut p, ic, pill_x + 12.0, iy + 15.0, 14.0, col);
                p.text(
                    cat.label(),
                    pill_x + 34.0,
                    iy + 26.5,
                    11.8,
                    if under_pill > 0.5 {
                        Weight::Bold
                    } else {
                        Weight::Semibold
                    },
                    col,
                );
            }

            SETTINGS_CHROME.with_borrow_mut(|slot| {
                if let Some((pm, key)) = slot.as_mut()
                    && pm.data().len() == p.pm.data().len()
                {
                    pm.data_mut().copy_from_slice(p.pm.data());
                    *key = chrome_key;
                } else {
                    *slot = Some((p.pm.clone(), chrome_key));
                }
            });
        }

        // 2. Draggable Top Header Dynamic Controls (Mascot, Close Button)
        let anim_t = state.anim_time;
        Renderer::draw_cyno_mascot(&mut p, 36.0, 32.0, 1.04, state.mascot_mood(), anim_t, false);

        let close_x = w - 48.0;
        let close_y = 17.0;
        let close_sz = 28.0;

        let close_ht = p.hot_t(close_x, close_y, close_sz, close_sz);
        p.tactile_surface_pill_t(close_x, close_y, close_sz, close_sz, 14.0, close_ht, None);
        if close_ht > 0.02 {
            p.rrect(
                close_x,
                close_y - 0.55 * close_ht,
                close_sz,
                close_sz,
                14.0,
                fade(rgba(220, 38, 38, 195), close_ht),
            );
        }
        icons::draw(
            &mut p,
            Icon::Close,
            close_x + 8.0,
            close_y + 8.0 - 0.55 * close_ht,
            12.0,
            lerp_color(tok::text2(), tok::text1(), close_ht),
        );
        p.hit(close_x, close_y, close_sz, close_sz, Action::CloseSettings);

        // 3. Register Sidebar Category Hit Targets (visuals cached in SETTINGS_CHROME)
        let pill_x = sb_x + 12.0;
        let pill_w = sb_w - 24.0;
        let pill_h = 44.0;
        for cat in SettingsCategory::ALL {
            let iy = AppState::settings_cat_pill_y(cat);
            p.hit(pill_x, iy, pill_w, pill_h, Action::SetSettingsCategory(cat));
        }
    }

    for i in 0..hits.len() {
        slot_map.push(i);
    }

    // 4. Right Viewport-Clipped Offscreen Detail Pane
    let base_slots = hits.len();
    let mut pane_hover_slots = [0.0f32; 48];
    for (i, dst) in pane_hover_slots.iter_mut().enumerate() {
        if base_slots + i < state.settings_hover_slots.len() {
            *dst = state.settings_hover_slots[base_slots + i];
        }
    }
    let pane_x_px = (PANE_X * s).round();
    let pane_y_px = (PANE_Y * s).round();
    let pane_w_px = (PANE_W * s).round();
    let pane_h_px = (PANE_H * s).round();
    let bleed_px = (PANE_BLEED * s).round();

    let scroll_px = (scroll_y * s).round();
    let pane_pointer = pointer.and_then(|(mx, my)| {
        if (pane_x_px..=pane_x_px + pane_w_px).contains(&mx)
            && (pane_y_px..=pane_y_px + pane_h_px).contains(&my)
        {
            Some((mx - (pane_x_px - bleed_px), my - pane_y_px + scroll_px))
        } else {
            None
        }
    });

    let state_sig = compute_pane_state_sig(state);
    let mut switch_bits = [0u32; 48];
    for (dst, src) in switch_bits.iter_mut().zip(state.switch_t.iter()) {
        *dst = src.to_bits();
    }
    let mut hover_bits = [0u32; 48];
    for (dst, src) in hover_bits.iter_mut().zip(pane_hover_slots.iter()) {
        *dst = src.to_bits();
    }
    let pane_key = PaneCacheKey {
        cat: state.settings_category,
        w: pane_pm.width(),
        h: pane_pm.height(),
        type_scale: state.type_scale as u8,
        state_sig,
        pointer_px: if state.micro_interactions {
            None
        } else {
            pane_pointer.map(|(x, y)| (x.round() as i32, y.round() as i32))
        },
        switch_bits,
        hover_bits,
    };
    let use_cached_pane = PANE_CACHE_ALLOWED.with(|c| c.get())
        && PANE_CACHE.with_borrow(|slot| matches!(slot, Some(cp) if cp.key == pane_key));

    let mut pane_hits = Vec::new();
    if use_cached_pane {
        PANE_CACHE.with_borrow(|slot| {
            if let Some(cp) = slot.as_ref() {
                blit_pane_slice(
                    pixmap,
                    &cp.pm,
                    (pane_x_px - bleed_px) as i32,
                    pane_y_px as i32,
                    scroll_px as i32,
                    pane_h_px as i32,
                );
                pane_hits = cp
                    .hits
                    .iter()
                    .map(|h| Hit {
                        rect: h.rect,
                        action: h.action,
                    })
                    .collect();
            }
        });
    } else {
        pane_pm.fill(Color::TRANSPARENT);
        let mut pp = Painter {
            pm: pane_pm,
            clip_mask: pane_mask,
            fonts,
            s,
            origin_x: bleed_px,
            origin_y: pane_top_pad * s,
            pointer: pane_pointer,
            hits: &mut pane_hits,
            interactive: true,
            global_alpha: 1.0,
            hover_slots: pane_hover_slots,
            micro_interactions: state.micro_interactions,
            frosted,
            frosted_opacity_pct: state.frosted_alpha_pct(),
            type_scale: state.type_scale,
            clip_bounds: None,
        };
        match state.settings_category {
            SettingsCategory::General => draw_general_pane(&mut pp, state, 0.0, 0.0, PANE_W),
            SettingsCategory::Motion => draw_motion_pane(&mut pp, state, 0.0, 0.0, PANE_W),
            SettingsCategory::Modules => draw_modules_pane(&mut pp, state, 0.0, 0.0, PANE_W),
            SettingsCategory::Notifications => {
                draw_notifications_pane(&mut pp, state, 0.0, 0.0, PANE_W)
            }
            SettingsCategory::System => draw_system_pane(&mut pp, state, 0.0, 0.0, PANE_W),
        }
        PANE_CACHE.with_borrow_mut(|slot| {
            if let Some(cp) = slot.as_mut()
                && cp.pm.width() == pane_pm.width()
                && cp.pm.height() == pane_pm.height()
            {
                cp.key = pane_key;
                cp.pm.data_mut().copy_from_slice(pane_pm.data());
                cp.hits.clear();
                cp.hits.extend(pane_hits.iter().map(|h| Hit {
                    rect: h.rect,
                    action: h.action,
                }));
            } else {
                *slot = Some(CachedPane {
                    key: pane_key,
                    pm: pane_pm.clone(),
                    hits: pane_hits
                        .iter()
                        .map(|h| Hit {
                            rect: h.rect,
                            action: h.action,
                        })
                        .collect(),
                });
            }
        });
        blit_pane_slice(
            pixmap,
            pane_pm,
            (pane_x_px - bleed_px) as i32,
            pane_y_px as i32,
            scroll_px as i32,
            pane_h_px as i32,
        );
    }

    for (raw_idx, h_item) in pane_hits.into_iter().enumerate() {
        let wx = h_item.rect.x() + (pane_x_px - bleed_px);
        let wy = h_item.rect.y() + pane_y_px - scroll_px;
        let y0 = wy.max(pane_y_px);
        let y1 = (wy + h_item.rect.height()).min(pane_y_px + pane_h_px);
        if y1 - y0 >= 8.0 * s
            && let Some(rect) = tiny_skia::Rect::from_xywh(wx, y0, h_item.rect.width(), y1 - y0)
        {
            hits.push(Hit {
                rect,
                action: h_item.action,
            });
            slot_map.push(base_slots + raw_idx);
        }
    }

    // 5. Crisp Scrollbar Pill when pane scrolls
    if max_scroll > 0.5 {
        let mut sb_hits = Vec::new();
        let mut sp = Painter {
            pm: pixmap,
            clip_mask,
            fonts,
            s,
            origin_x: 0.0,
            origin_y: 0.0,
            pointer: None,
            hits: &mut sb_hits,
            interactive: false,
            global_alpha: 1.0,
            hover_slots: [0.0; 48],
            micro_interactions: false,
            frosted,
            frosted_opacity_pct: state.frosted_alpha_pct(),
            type_scale: state.type_scale,
            clip_bounds: None,
        };
        let track_x = PANE_X + PANE_W + 7.0;
        let track_y = PANE_Y + 6.0;
        let track_h = PANE_H - 12.0;
        sp.rrect(track_x, track_y, 4.0, track_h, 2.0, rgba(255, 255, 255, 18));
        let total_span = content_h + pane_top_pad + pane_bot_pad;
        let thumb_h = ((PANE_H / total_span) * track_h).clamp(44.0, track_h);
        let thumb_y = track_y + (scroll_y / max_scroll) * (track_h - thumb_h);
        sp.rrect(
            track_x,
            thumb_y,
            4.0,
            thumb_h,
            2.0,
            rgba(192, 132, 252, 135),
        );
    }

    (hits, slot_map, max_scroll)
}

fn switch_val(state: &AppState, id: ToggleId) -> f32 {
    if state.micro_interactions {
        state.switch_t[id as usize]
    } else if state.toggle_target(id) {
        1.0
    } else {
        0.0
    }
}

fn draw_segmented_row<T: Copy + PartialEq>(
    p: &mut Painter,
    label: &str,
    row_x: f32,
    row_y: f32,
    ctrl_x: f32,
    ctrl_w: f32,
    current: T,
    items: &[(&str, T, Action)],
) {
    p.text(
        label,
        row_x,
        row_y + 18.5,
        11.5,
        Weight::Semibold,
        tok::text1(),
    );
    let gap = 7.0;
    let btn_w =
        (ctrl_w - gap * (items.len().saturating_sub(1) as f32)) / (items.len().max(1) as f32);
    let btn_h = 28.0;
    let font_sz = if items.len() >= 6 { 9.8 } else { 10.4 };
    for (i, &(txt, val, act)) in items.iter().enumerate() {
        let bx = ctrl_x + (i as f32) * (btn_w + gap);
        let active = current == val;
        let ht = p.hot_t(bx, row_y, btn_w, btn_h);
        if active {
            p.luminous_pill_button_t(
                bx,
                row_y,
                btn_w,
                btn_h,
                8.5,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                ht,
            );
            p.text_center_shadow_fit(
                txt,
                bx + btn_w / 2.0,
                row_y + 18.0 - 0.7 * ht,
                font_sz,
                Weight::Bold,
                tok::text1(),
                btn_w - 8.0,
            );
        } else {
            p.tactile_surface_pill_t(bx, row_y, btn_w, btn_h, 8.5, ht, None);
            p.text_center_fit(
                txt,
                bx + btn_w / 2.0,
                row_y + 18.0 - 0.55 * ht,
                font_sz,
                Weight::Semibold,
                lerp_color(tok::text_soft(), tok::text1(), ht),
                btn_w - 8.0,
            );
        }
        p.hit(bx, row_y, btn_w, btn_h, act);
    }
}

fn draw_toggle_row(
    p: &mut Painter,
    state: &AppState,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    sub: &str,
    id: ToggleId,
    action: Action,
) {
    let max_text_w = (w - 64.0).max(80.0);
    let t_ell = p.ellipsize(title, 12.0, Weight::Semibold, max_text_w);
    let s_ell = p.ellipsize(sub, 9.8, Weight::Regular, max_text_w);
    p.text(&t_ell, x, y + 14.5, 12.0, Weight::Semibold, tok::text1());
    p.text(&s_ell, x, y + 32.0, 9.8, Weight::Regular, tok::text2());
    let sw_w = 46.0;
    let sw_h = 22.0;
    let sw_x = x + w - sw_w;
    let sw_y = y + 8.0;
    let ht = p.hot_t(sw_x, sw_y, sw_w, sw_h);
    let st = switch_val(state, id);
    p.draw_toggle_switch(sw_x, sw_y, sw_w, sw_h, st, ht);
    p.hit(sw_x, sw_y, sw_w, sw_h, action);
}

fn draw_general_pane(p: &mut Painter, state: &AppState, px: f32, py: f32, pw: f32) {
    let ix = px + PAD;
    let inner_w = pw - PAD * 2.0;
    let ctrl_x = px + CTRL_X_OFF;
    let ctrl_w = pw - CTRL_X_OFF - PAD;

    // Card 1: Notch Shell & Placement (h = 274)
    let c1_h = 274.0;
    p.elevated_card(
        px,
        py,
        pw,
        c1_h,
        16.0,
        Some((pw - 44.0, 28.0, 76.0, tok::cyno_violet())),
    );
    p.text(
        "Notch Shell & Placement",
        ix,
        py + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Shape, edge dock, size presets & window lock",
        ix,
        py + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_segmented_row(
        p,
        "Shell Style",
        ix,
        py + 60.0,
        ctrl_x,
        ctrl_w,
        state.notch_style,
        &[
            (
                "Floating",
                NotchStyle::Floating,
                Action::SetNotchStyle(NotchStyle::Floating),
            ),
            (
                "Edge Flush",
                NotchStyle::EdgeFlush,
                Action::SetNotchStyle(NotchStyle::EdgeFlush),
            ),
        ],
    );
    draw_segmented_row(
        p,
        "Dock Edge",
        ix,
        py + 60.0 + SEG_PITCH,
        ctrl_x,
        ctrl_w,
        state.placement,
        &[
            (
                "Top",
                NotchPlacement::Top,
                Action::SetPlacement(NotchPlacement::Top),
            ),
            (
                "Left",
                NotchPlacement::Left,
                Action::SetPlacement(NotchPlacement::Left),
            ),
            (
                "Right",
                NotchPlacement::Right,
                Action::SetPlacement(NotchPlacement::Right),
            ),
        ],
    );
    draw_segmented_row(
        p,
        "Idle Size",
        ix,
        py + 60.0 + SEG_PITCH * 2.0,
        ctrl_x,
        ctrl_w,
        state.size_preset,
        &[
            (
                "Compact",
                NotchSizePreset::Compact,
                Action::SetSizePreset(NotchSizePreset::Compact),
            ),
            (
                "Standard",
                NotchSizePreset::Standard,
                Action::SetSizePreset(NotchSizePreset::Standard),
            ),
            (
                "Wide",
                NotchSizePreset::Wide,
                Action::SetSizePreset(NotchSizePreset::Wide),
            ),
        ],
    );
    draw_segmented_row(
        p,
        "Hover Size",
        ix,
        py + 60.0 + SEG_PITCH * 3.0,
        ctrl_x,
        ctrl_w,
        state.expanded_size,
        &[
            (
                "Compact",
                ExpandedSizePreset::Compact,
                Action::SetExpandedSize(ExpandedSizePreset::Compact),
            ),
            (
                "Standard",
                ExpandedSizePreset::Standard,
                Action::SetExpandedSize(ExpandedSizePreset::Standard),
            ),
            (
                "Large",
                ExpandedSizePreset::Large,
                Action::SetExpandedSize(ExpandedSizePreset::Large),
            ),
        ],
    );

    let row5_y = py + 60.0 + SEG_PITCH * 4.0;
    p.text(
        "Position & Lock",
        ix,
        row5_y + 18.5,
        11.5,
        Weight::Semibold,
        tok::text1(),
    );
    let btn_w = (ctrl_w - 8.0) / 2.0;
    let pin_ht = p.hot_t(ctrl_x, row5_y, btn_w, 28.0);
    p.tactile_surface_pill_t(
        ctrl_x,
        row5_y,
        btn_w,
        28.0,
        8.5,
        pin_ht,
        if state.is_pinned {
            Some(tok::cyno_violet())
        } else {
            None
        },
    );
    let pin_lbl = if state.is_pinned {
        "Pinned Open: ON"
    } else {
        "Pin Open: OFF"
    };
    p.text_center(
        pin_lbl,
        ctrl_x + btn_w / 2.0,
        row5_y + 18.0 - 0.55 * pin_ht,
        10.4,
        Weight::Bold,
        tok::text1(),
    );
    p.hit(ctrl_x, row5_y, btn_w, 28.0, Action::TogglePin);

    let moved = state.x_offset != 0 || state.y_offset != 0;
    let ctr_x = ctrl_x + btn_w + 8.0;
    let ctr_ht = p.hot_t(ctr_x, row5_y, btn_w, 28.0);
    p.tactile_surface_pill_t(
        ctr_x,
        row5_y,
        btn_w,
        28.0,
        8.5,
        ctr_ht,
        if moved {
            Some(tok::cyno_violet())
        } else {
            None
        },
    );
    let ctr_lbl = if !moved {
        "Centered (0px)".to_string()
    } else if state.placement.is_vertical() {
        format!("Center ({:+}px)", state.y_offset)
    } else {
        format!("Center ({:+}px)", state.x_offset)
    };
    p.text_center(
        &ctr_lbl,
        ctr_x + btn_w / 2.0,
        row5_y + 18.0 - 0.55 * ctr_ht,
        10.4,
        Weight::Semibold,
        if moved {
            tok::text1()
        } else {
            tok::text_soft()
        },
    );
    p.hit(ctr_x, row5_y, btn_w, 28.0, Action::ResetPosition);

    // Card 2: Interaction, Type Scale & Idle Visuals (h = 448)
    let c2_y = py + c1_h + CARD_GAP;
    let c2_h = 580.0;
    p.elevated_card(
        px,
        c2_y,
        pw,
        c2_h,
        16.0,
        Some((pw - 44.0, 28.0, 76.0, tok::cyno_orchid())),
    );
    p.text(
        "Interaction & Idle Visuals",
        ix,
        c2_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Desktop font scaling, idle readout, stealth pill & live progress arc",
        ix,
        c2_y + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_segmented_row(
        p,
        "Idle Metric",
        ix,
        c2_y + 60.0,
        ctrl_x,
        ctrl_w,
        state.idle_metric,
        &[
            (
                "RAM",
                IdleMetric::Ram,
                Action::SetIdleMetric(IdleMetric::Ram),
            ),
            (
                "Net",
                IdleMetric::NetSpeed,
                Action::SetIdleMetric(IdleMetric::NetSpeed),
            ),
            (
                "CPU",
                IdleMetric::Cpu,
                Action::SetIdleMetric(IdleMetric::Cpu),
            ),
            (
                "Clock",
                IdleMetric::Clock,
                Action::SetIdleMetric(IdleMetric::Clock),
            ),
            (
                "Minimal",
                IdleMetric::Minimal,
                Action::SetIdleMetric(IdleMetric::Minimal),
            ),
        ],
    );
    draw_segmented_row(
        p,
        "Type Scale",
        ix,
        c2_y + 60.0 + SEG_PITCH,
        ctrl_x,
        ctrl_w,
        state.type_scale,
        &[
            (
                "Compact",
                TypeScale::Compact,
                Action::SetTypeScale(TypeScale::Compact),
            ),
            (
                "Desktop",
                TypeScale::Standard,
                Action::SetTypeScale(TypeScale::Standard),
            ),
            (
                "Large",
                TypeScale::Large,
                Action::SetTypeScale(TypeScale::Large),
            ),
            (
                "X-Large",
                TypeScale::ExtraLarge,
                Action::SetTypeScale(TypeScale::ExtraLarge),
            ),
        ],
    );

    let t_base = c2_y + 106.0 + SEG_PITCH;
    draw_toggle_row(
        p,
        state,
        ix,
        t_base,
        inner_w,
        "Minimize Idle Island",
        "Ultra-compact 86×18px stealth capsule when collapsed",
        ToggleId::MinimizedIsland,
        Action::ToggleMinimizedIsland,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH,
        inner_w,
        "Expand on Click Only",
        "Require click instead of cursor hover to open notch",
        ToggleId::ClickToOpen,
        Action::ToggleClickToOpen,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH * 2.0,
        inner_w,
        "Hide in Fullscreen Apps",
        "Conceal idle notch automatically over fullscreen games or video",
        ToggleId::HideFullscreen,
        Action::ToggleHideFullscreen,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH * 3.0,
        inner_w,
        "Show Cyno Mascot",
        "Display animated Cyber-Anubis mascot across notch states",
        ToggleId::ShowMascot,
        Action::ToggleShowMascot,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH * 4.0,
        inner_w,
        "Live Progress Arc",
        "Wrap idle badge in live progress ring (RAM, track)",
        ToggleId::ProgressArc,
        Action::ToggleProgressArc,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH * 5.0,
        inner_w,
        "Remove Windows App Borders",
        "Removes DWM window borders across Explorer & all apps",
        ToggleId::RemoveWindowsBorders,
        Action::ToggleRemoveWindowsBorders,
    );
    let frost_sub = if state.windows_transparency_enabled {
        "Translucent acrylic blur backdrop behind notch & settings"
    } else {
        "Requires Windows Transparency Effects (currently Off in Windows)"
    };
    draw_toggle_row(
        p,
        state,
        ix,
        t_base + TOGGLE_PITCH * 6.0,
        inner_w,
        "Frosted Blur & Transparency",
        frost_sub,
        ToggleId::FrostedGlass,
        Action::ToggleFrostedGlass,
    );
    let op_presets = [35u32, 50, 68, 85];
    let nearest_op = *op_presets
        .iter()
        .min_by_key(|&&v| v.abs_diff(state.frosted_opacity_pct))
        .unwrap_or(&68);
    draw_segmented_row(
        p,
        "Glass Opacity",
        ix,
        t_base + TOGGLE_PITCH * 7.0,
        ctrl_x,
        ctrl_w,
        nearest_op,
        &[
            ("35%", 35u32, Action::SetFrostedOpacity(35)),
            ("50%", 50u32, Action::SetFrostedOpacity(50)),
            ("68%", 68u32, Action::SetFrostedOpacity(68)),
            ("85%", 85u32, Action::SetFrostedOpacity(85)),
        ],
    );
    let blur_presets = [10u32, 20, 30, 40];
    let nearest_blur = *blur_presets
        .iter()
        .min_by_key(|&&v| v.abs_diff(state.frosted_blur_px))
        .unwrap_or(&20);
    draw_segmented_row(
        p,
        "Blur Intensity",
        ix,
        t_base + TOGGLE_PITCH * 7.0 + SEG_PITCH,
        ctrl_x,
        ctrl_w,
        nearest_blur,
        &[
            ("Soft", 10u32, Action::SetFrostedBlur(10)),
            ("Medium", 20u32, Action::SetFrostedBlur(20)),
            ("Deep", 30u32, Action::SetFrostedBlur(30)),
            ("Ultra", 40u32, Action::SetFrostedBlur(40)),
        ],
    );
}

fn draw_motion_pane(p: &mut Painter, state: &AppState, px: f32, py: f32, pw: f32) {
    let ix = px + PAD;
    let inner_w = pw - PAD * 2.0;
    let ctrl_x = px + CTRL_X_OFF;
    let ctrl_w = pw - CTRL_X_OFF - PAD;
    let well_col = if state.is_frosted_active() {
        tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
    } else {
        tok::surface_well()
    };

    // Card 1: Zero-Bounce Motion Engine (h = 166)
    let c1_h = 166.0;
    p.elevated_card(
        px,
        py,
        pw,
        c1_h,
        16.0,
        Some((pw - 44.0, 28.0, 82.0, tok::cyno_violet())),
    );
    p.text(
        "Zero-Bounce Motion Engine",
        ix,
        py + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    let hz_sub = format!(
        "Monitor refresh: {} Hz · Adaptive pacing drops to 0% CPU when idle",
        state.detected_hz.max(60)
    );
    p.text(&hz_sub, ix, py + 43.0, 10.0, Weight::Regular, tok::text2());

    draw_segmented_row(
        p,
        "FPS Target",
        ix,
        py + 62.0,
        ctrl_x,
        ctrl_w,
        state.fps_cap,
        &[
            ("Auto", 0, Action::SetFpsCap(0)),
            ("60", 60, Action::SetFpsCap(60)),
            ("90", 90, Action::SetFpsCap(90)),
            ("120", 120, Action::SetFpsCap(120)),
        ],
    );

    let info_y = py + 108.0;
    p.rrect(ix, info_y, inner_w, 34.0, 9.0, well_col);
    p.rrect_bevel_stroke(
        ix,
        info_y,
        inner_w,
        34.0,
        9.0,
        1.0,
        rgba(255, 255, 255, 20),
        rgba(168, 85, 247, 18),
    );
    p.text(
        "Exponential ease_exp() decay · Zero overshoot · Single-surface canvas",
        ix + 14.0,
        info_y + 21.0,
        10.0,
        Weight::Semibold,
        tok::cyno_violet_hi(),
    );

    // Card 2: Micro-Interactions & Audio Spectrum (h = 172)
    let c2_y = py + c1_h + CARD_GAP;
    let c2_h = 172.0;
    p.elevated_card(
        px,
        c2_y,
        pw,
        c2_h,
        16.0,
        Some((pw - 44.0, 28.0, 76.0, tok::cyno_orchid())),
    );
    p.text(
        "Micro-Interactions & Audio Spectrum",
        ix,
        c2_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Hover glow glides, micro-lift physics & live media spectrum bars",
        ix,
        c2_y + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_toggle_row(
        p,
        state,
        ix,
        c2_y + 62.0,
        inner_w,
        "Hover Micro-Interactions",
        "Smooth hover glow fades, micro-lift & dock indicator glide",
        ToggleId::MicroInteractions,
        Action::ToggleMicroInteractions,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        c2_y + 62.0 + TOGGLE_PITCH,
        inner_w,
        "Audio Spectrum Equalizer",
        "Show lightweight electro bars during active media playback",
        ToggleId::Equalizer,
        Action::ToggleEqualizer,
    );
}

fn draw_notifications_pane(p: &mut Painter, state: &AppState, px: f32, py: f32, pw: f32) {
    let ix = px + PAD;
    let inner_w = pw - PAD * 2.0;
    let ctrl_x = px + CTRL_X_OFF;
    let ctrl_w = pw - CTRL_X_OFF - PAD;
    let well_col = if state.is_frosted_active() {
        tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
    } else {
        tok::surface_well()
    };

    // Card 1: Notch Banner & Fullscreen Rules (h = 228)
    let c1_h = 228.0;
    p.elevated_card(
        px,
        py,
        pw,
        c1_h,
        16.0,
        Some((pw - 44.0, 28.0, 78.0, tok::cyno_orchid())),
    );
    p.text(
        "Notch Banner & Fullscreen Rules",
        ix,
        py + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Popup alerts, display duration & fullscreen/borderless game overlay behavior",
        ix,
        py + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_toggle_row(
        p,
        state,
        ix,
        py + 62.0,
        inner_w,
        "Notch Status Notifications",
        "Popup alerts in capsule for system toasts, captures, clipboard & hydration",
        ToggleId::Notifications,
        Action::ToggleNotifications,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        py + 62.0 + TOGGLE_PITCH,
        inner_w,
        "Overlap Notification Banner in Fullscreen",
        "Show active notification banners on top of fullscreen/borderless games & video",
        ToggleId::NotifOverFullscreen,
        Action::ToggleNotifOverFullscreen,
    );
    draw_segmented_row(
        p,
        "Banner Duration",
        ix,
        py + 172.0,
        ctrl_x,
        ctrl_w,
        state.notification_secs,
        &[
            ("2s", 2, Action::SetNotificationSecs(2)),
            ("4s", 4, Action::SetNotificationSecs(4)),
            ("6s", 6, Action::SetNotificationSecs(6)),
            ("8s", 8, Action::SetNotificationSecs(8)),
        ],
    );

    // Card 2: Windows Notifications Bridge (h = 250)
    let c2_y = py + c1_h + CARD_GAP;
    let c2_h = 250.0;
    p.elevated_card(
        px,
        c2_y,
        pw,
        c2_h,
        16.0,
        Some((pw - 44.0, 28.0, 78.0, tok::cyno_violet())),
    );
    p.text(
        "Windows Notifications Bridge",
        ix,
        c2_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );

    let st_w =
        (p.measure(&state.notif_listener_status, 9.2, Weight::Bold) + 26.0).clamp(104.0, 152.0);
    let st_x = ix + inner_w - st_w;
    let st_y = c2_y + 13.0;
    let is_listening = state.notif_listener_enabled
        && matches!(
            state.notif_listener_status.as_str(),
            "Connected" | "Listening"
        );
    p.rrect(st_x, st_y, st_w, 22.0, 11.0, well_col);
    p.rrect_bevel_stroke(
        st_x,
        st_y,
        st_w,
        22.0,
        11.0,
        1.0,
        if is_listening {
            rgba(56, 189, 248, 110)
        } else {
            rgba(255, 255, 255, 28)
        },
        rgba(255, 255, 255, 12),
    );
    let st_lbl = p.ellipsize(&state.notif_listener_status, 9.2, Weight::Bold, st_w - 14.0);
    p.text_center(
        &st_lbl,
        st_x + st_w * 0.5,
        st_y + 14.5,
        9.2,
        Weight::Bold,
        if is_listening {
            tok::cyan_hi()
        } else {
            tok::text2()
        },
    );

    draw_toggle_row(
        p,
        state,
        ix,
        c2_y + 52.0,
        inner_w,
        "Forward Windows Notifications to CyNotch",
        "Stream real app toast alerts (Discord, WA, Browser) into the island",
        ToggleId::NotifListener,
        Action::ToggleNotifListener,
    );
    let excl_sub = if state.notif_exclusive_mode && state.notif_suppressed_apps > 0 {
        format!(
            "Windows popups silenced ({} apps backed up safely)",
            state.notif_suppressed_apps
        )
    } else {
        "Suppress native Windows toast banners so alerts show in CyNotch only".to_string()
    };
    draw_toggle_row(
        p,
        state,
        ix,
        c2_y + 52.0 + TOGGLE_PITCH,
        inner_w,
        "Exclusive CyNotch Banners",
        &excl_sub,
        ToggleId::NotifExclusive,
        Action::ToggleNotifExclusive,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        c2_y + 52.0 + TOGGLE_PITCH * 2.0,
        inner_w,
        "Auto-Clear Action Center",
        "Dismiss forwarded toasts from Windows Action Center after capture",
        ToggleId::NotifAutoClear,
        Action::ToggleNotifAutoClear,
    );

    let bot2_y = c2_y + 206.0;
    let wn_lbl = "Win Notif Settings";
    let tb_lbl = "Test Banner";
    let wn_w = (p.measure(wn_lbl, 9.8, Weight::Bold) + 24.0).clamp(132.0, 176.0);
    let tb_w = (p.measure(tb_lbl, 9.6, Weight::Bold) + 22.0).clamp(90.0, 128.0);
    let wn_x = ix + inner_w - wn_w;
    let tb_x = wn_x - 8.0 - tb_w;
    let footer_max_w = (tb_x - ix - 12.0).max(120.0);
    let footer_txt = if is_listening {
        "Connected — pop-out banners active on incoming toasts"
    } else if state.notif_listener_status == "Permission needed" {
        "Blocked — enable Windows 'Notification access' for CyNotch"
    } else if state.notif_listener_enabled {
        "Connecting Windows 'Notification access' for CyNotch..."
    } else {
        "Requires Windows 'Notification access' permission"
    };
    let footer_ell = p.ellipsize(footer_txt, 9.4, Weight::Regular, footer_max_w);
    p.text(
        &footer_ell,
        ix,
        bot2_y + 17.5,
        9.4,
        Weight::Regular,
        if state.notif_listener_status == "Permission needed" {
            rgba(251, 191, 36, 235)
        } else {
            tok::text2()
        },
    );
    let tb_ht = p.hot_t(tb_x, bot2_y, tb_w, 28.0);
    p.tactile_surface_pill_t(tb_x, bot2_y, tb_w, 28.0, 8.5, tb_ht, Some(tok::cyan_hi()));
    p.text_center_fit(
        "Test Banner",
        tb_x + tb_w * 0.5,
        bot2_y + 18.0 - 0.55 * tb_ht,
        9.6,
        Weight::Bold,
        tok::cyan_hi(),
        tb_w - 8.0,
    );
    p.hit(tb_x, bot2_y, tb_w, 28.0, Action::SendTestNotification);

    let wn_ht = p.hot_t(wn_x, bot2_y, wn_w, 28.0);
    p.tactile_surface_pill_t(
        wn_x,
        bot2_y,
        wn_w,
        28.0,
        8.5,
        wn_ht,
        Some(tok::cyno_violet()),
    );
    p.text_center_fit(
        "Win Notif Settings",
        wn_x + wn_w * 0.5,
        bot2_y + 18.0 - 0.55 * wn_ht,
        9.8,
        Weight::Bold,
        tok::cyno_violet_hi(),
        wn_w - 8.0,
    );
    p.hit(wn_x, bot2_y, wn_w, 28.0, Action::OpenWinNotifSettings);
}

fn draw_modules_pane(p: &mut Painter, state: &AppState, px: f32, py: f32, pw: f32) {
    let ix = px + PAD;
    let inner_w = pw - PAD * 2.0;
    let ctrl_x = px + CTRL_X_OFF;
    let ctrl_w = pw - CTRL_X_OFF - PAD;

    // Card 1: Dock Modules (h = 270)
    let c1_h = 270.0;
    p.elevated_card(
        px,
        py,
        pw,
        c1_h,
        16.0,
        Some((pw - 44.0, 28.0, 78.0, tok::cyno_violet())),
    );
    p.text(
        "Dock Modules",
        ix,
        py + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Show or hide tabs in the notch dock (Command Hub stays pinned)",
        ix,
        py + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    let tiles = [
        (
            Icon::Music,
            "Music Player",
            ToggleId::ModMusic,
            NotchMode::Music,
        ),
        (
            Icon::Volume,
            "Volume Master",
            ToggleId::ModVolume,
            NotchMode::Volume,
        ),
        (
            Icon::Wave,
            "Memory Reduct",
            ToggleId::ModMemReduct,
            NotchMode::MemReduct,
        ),
        (
            Icon::List,
            "Tasks Todo",
            ToggleId::ModTasks,
            NotchMode::Tasks,
        ),
        (
            Icon::Clipboard,
            "File Shelf",
            ToggleId::ModShelf,
            NotchMode::Shelf,
        ),
        (
            Icon::Video,
            "Quick Capture",
            ToggleId::ModCapture,
            NotchMode::Capture,
        ),
        (
            Icon::Display,
            "Screen Time",
            ToggleId::ModTelemetry,
            NotchMode::Telemetry,
        ),
    ];

    let col_gap = 12.0;
    let row_gap = 12.0;
    let tile_w = (inner_w - col_gap) / 2.0;
    let tile_h = 38.0;
    let grid_y = py + 62.0;

    for (idx, (ic, name, tid, mode)) in tiles.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let tx = ix + (col as f32) * (tile_w + col_gap);
        let ty = grid_y + (row as f32) * (tile_h + row_gap);
        let active = state.toggle_target(*tid);
        let ht = p.hot_t(tx, ty, tile_w, tile_h);

        p.tactile_surface_pill_t(tx, ty, tile_w, tile_h, 10.0, ht, None);
        if active {
            p.rrect(
                tx,
                ty - 0.55 * ht,
                tile_w,
                tile_h,
                10.0,
                fade(rgba(124, 58, 237, 28), 0.7 + 0.3 * ht),
            );
        }
        let dy = -0.55 * ht;
        icons::draw(
            p,
            *ic,
            tx + 12.0,
            ty + 12.0 + dy,
            14.0,
            if active {
                tok::cyno_violet_hi()
            } else {
                tok::text2()
            },
        );
        p.text(
            name,
            tx + 34.0,
            ty + 23.0 + dy,
            11.4,
            Weight::Semibold,
            if active {
                tok::text1()
            } else {
                tok::text_soft()
            },
        );
        let st = switch_val(state, *tid);
        p.draw_toggle_switch(tx + tile_w - 48.0, ty + 10.0 + dy, 38.0, 18.0, st, ht);
        p.hit(tx, ty, tile_w, tile_h, Action::ToggleModule(*mode));
    }

    // Card 2: Music Lyrics Display (h = 158)
    let c2_y = py + c1_h + CARD_GAP;
    let c2_h = 158.0;
    p.elevated_card(
        px,
        c2_y,
        pw,
        c2_h,
        16.0,
        Some((pw - 44.0, 28.0, 76.0, tok::cyno_violet())),
    );
    p.text(
        "Music Lyrics Display",
        ix,
        c2_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Customize synced lyrics font scale and presentation layout in Music mode",
        ix,
        c2_y + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_segmented_row(
        p,
        "Lyric Size",
        ix,
        c2_y + 62.0,
        ctrl_x,
        ctrl_w,
        state.lyric_size,
        &[
            (
                "Compact",
                LyricSize::Compact,
                Action::SetLyricSize(LyricSize::Compact),
            ),
            (
                "Standard",
                LyricSize::Standard,
                Action::SetLyricSize(LyricSize::Standard),
            ),
            (
                "Large",
                LyricSize::Large,
                Action::SetLyricSize(LyricSize::Large),
            ),
        ],
    );
    draw_segmented_row(
        p,
        "Lyric Style",
        ix,
        c2_y + 62.0 + SEG_PITCH,
        ctrl_x,
        ctrl_w,
        state.lyric_style,
        &[
            (
                "Classic",
                LyricStyle::ClassicList,
                Action::SetLyricStyle(LyricStyle::ClassicList),
            ),
            (
                "Karaoke",
                LyricStyle::KaraokeGlow,
                Action::SetLyricStyle(LyricStyle::KaraokeGlow),
            ),
            (
                "Minimal",
                LyricStyle::MinimalSingle,
                Action::SetLyricStyle(LyricStyle::MinimalSingle),
            ),
            (
                "Stream",
                LyricStyle::CompactStream,
                Action::SetLyricStyle(LyricStyle::CompactStream),
            ),
        ],
    );

    // Card 3: Background Listeners (h = 214)
    let c3_y = c2_y + c2_h + CARD_GAP;
    let c3_h = 214.0;
    p.elevated_card(
        px,
        c3_y,
        pw,
        c3_h,
        16.0,
        Some((pw - 44.0, 28.0, 76.0, tok::cyno_orchid())),
    );
    p.text(
        "Background Listeners",
        ix,
        c3_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );
    p.text(
        "Clipboard history, lyrics lookup & snapshot output rules",
        ix,
        c3_y + 43.0,
        10.0,
        Weight::Regular,
        tok::text2(),
    );

    draw_toggle_row(
        p,
        state,
        ix,
        c3_y + 62.0,
        inner_w,
        "Watch Clipboard History",
        "Auto-save copied text snippets into Quick Shelf",
        ToggleId::ClipboardWatch,
        Action::ToggleClipboardWatch,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        c3_y + 62.0 + TOGGLE_PITCH,
        inner_w,
        "Fetch Synced Track Lyrics",
        "Lookup live timestamped lyrics when songs change",
        ToggleId::LyricsMonitor,
        Action::ToggleLyricsMonitor,
    );
    draw_toggle_row(
        p,
        state,
        ix,
        c3_y + 62.0 + TOGGLE_PITCH * 2.0,
        inner_w,
        "Copy Captures to Clipboard",
        "Copy screenshot image alongside saving PNG to disk",
        ToggleId::CaptureClipboard,
        Action::ToggleCaptureClipboard,
    );
}

fn draw_system_pane(p: &mut Painter, state: &AppState, px: f32, py: f32, pw: f32) {
    let ix = px + PAD;
    let inner_w = pw - PAD * 2.0;
    let well_col = if state.is_frosted_active() {
        tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
    } else {
        tok::surface_well()
    };

    // Card 1: Mem Reduct Engine (h = 502, Dock-Modules toggle tiles + generous breathing room)
    let c1_h = 502.0;
    p.elevated_card(
        px,
        py,
        pw,
        c1_h,
        16.0,
        Some((pw - 44.0, 28.0, 82.0, tok::cyno_violet())),
    );
    p.text(
        "Mem Reduct Engine",
        ix,
        py + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );

    let is_admin = state.telemetry.has_admin_privileges;
    let priv_w = 118.0;
    let priv_h = 22.0;
    let priv_x = ix + inner_w - priv_w;
    let priv_y = py + 12.0;
    p.rrect(
        priv_x,
        priv_y,
        priv_w,
        priv_h,
        11.0,
        if is_admin {
            rgba(124, 58, 237, 55)
        } else {
            well_col
        },
    );
    p.rrect_bevel_stroke(
        priv_x,
        priv_y,
        priv_w,
        priv_h,
        11.0,
        1.0,
        if is_admin {
            rgba(196, 148, 255, 135)
        } else {
            rgba(235, 230, 255, 28)
        },
        rgba(235, 230, 255, 14),
    );
    p.text_center(
        if is_admin {
            "Elevated (Admin)"
        } else {
            "Standard (User)"
        },
        priv_x + priv_w / 2.0,
        priv_y + 14.5,
        9.4,
        Weight::Bold,
        if is_admin {
            tok::cyno_mist()
        } else {
            tok::text2()
        },
    );

    // 3-Pool Telemetry Summary Well
    let well_y = py + 46.0;
    let well_h = 34.0;
    p.rrect(ix, well_y, inner_w, well_h, 9.0, well_col);
    p.rrect_bevel_stroke(
        ix,
        well_y,
        inner_w,
        well_h,
        9.0,
        1.0,
        rgba(255, 255, 255, 22),
        rgba(168, 85, 247, 16),
    );
    let pool_cw = inner_w / 3.0;
    let phys_txt = format!(
        "Phys {:.1}/{:.1}G ({:.0}%)",
        state.telemetry.memory_used_gb,
        state.telemetry.memory_total_gb,
        state.telemetry.memory_percent
    );
    let page_txt = format!(
        "Page {:.1}/{:.1}G ({:.0}%)",
        state.telemetry.pagefile_used_gb,
        state.telemetry.pagefile_total_gb,
        state.telemetry.pagefile_percent
    );
    let sys_txt = format!(
        "SysCache {:.0} MB ({:.0}%)",
        state.telemetry.sys_cache_mb, state.telemetry.sys_cache_percent
    );
    p.text_center(
        &phys_txt,
        ix + pool_cw * 0.5,
        well_y + 21.0,
        9.8,
        Weight::Semibold,
        tok::text1(),
    );
    p.rrect(
        ix + pool_cw,
        well_y + 8.0,
        1.0,
        well_h - 16.0,
        0.0,
        rgba(255, 255, 255, 24),
    );
    p.text_center(
        &page_txt,
        ix + pool_cw * 1.5,
        well_y + 21.0,
        9.8,
        Weight::Semibold,
        tok::cyno_violet_hi(),
    );
    p.rrect(
        ix + pool_cw * 2.0,
        well_y + 8.0,
        1.0,
        well_h - 16.0,
        0.0,
        rgba(255, 255, 255, 24),
    );
    p.text_center(
        &sys_txt,
        ix + pool_cw * 2.5,
        well_y + 21.0,
        9.8,
        Weight::Semibold,
        tok::cyno_mist(),
    );

    // Section Header + Compact Toggle Switch Pills (Auto Clean & Show Results)
    let hdr_y = py + 94.0;
    p.text(
        "CLEANING AREAS",
        ix,
        hdr_y + 18.0,
        9.8,
        Weight::Bold,
        tok::text3(),
    );

    let sr_w = 136.0;
    let sr_h = 28.0;
    let sr_x = ix + inner_w - sr_w;
    let sr_ht = p.hot_t(sr_x, hdr_y, sr_w, sr_h);
    p.tactile_surface_pill_t(sr_x, hdr_y, sr_w, sr_h, 9.0, sr_ht, None);
    if state.mem_reduct.show_clean_results {
        p.rrect(
            sr_x,
            hdr_y - 0.5 * sr_ht,
            sr_w,
            sr_h,
            9.0,
            fade(rgba(124, 58, 237, 26), 0.7 + 0.3 * sr_ht),
        );
    }
    let sr_dy = -0.5 * sr_ht;
    p.text(
        "Show Results",
        sr_x + 11.0,
        hdr_y + 17.5 + sr_dy,
        9.8,
        Weight::Semibold,
        if state.mem_reduct.show_clean_results {
            tok::text1()
        } else {
            tok::text_soft()
        },
    );
    let sr_st = switch_val(state, ToggleId::ShowCleanResults);
    p.draw_toggle_switch(
        sr_x + sr_w - 40.0,
        hdr_y + 6.0 + sr_dy,
        32.0,
        16.0,
        sr_st,
        sr_ht,
    );
    p.hit(sr_x, hdr_y, sr_w, sr_h, Action::ToggleShowCleanResults);

    let ac_w = 124.0;
    let ac_x = sr_x - ac_w - 10.0;
    let ac_ht = p.hot_t(ac_x, hdr_y, ac_w, sr_h);
    p.tactile_surface_pill_t(ac_x, hdr_y, ac_w, sr_h, 9.0, ac_ht, None);
    if state.mem_reduct.auto_clean_enabled {
        p.rrect(
            ac_x,
            hdr_y - 0.5 * ac_ht,
            ac_w,
            sr_h,
            9.0,
            fade(rgba(124, 58, 237, 26), 0.7 + 0.3 * ac_ht),
        );
    }
    let ac_dy = -0.5 * ac_ht;
    p.text(
        "Auto Clean",
        ac_x + 11.0,
        hdr_y + 17.5 + ac_dy,
        9.8,
        Weight::Semibold,
        if state.mem_reduct.auto_clean_enabled {
            tok::text1()
        } else {
            tok::text_soft()
        },
    );
    let ac_st = switch_val(state, ToggleId::AutoClean);
    p.draw_toggle_switch(
        ac_x + ac_w - 40.0,
        hdr_y + 6.0 + ac_dy,
        32.0,
        16.0,
        ac_st,
        ac_ht,
    );
    p.hit(ac_x, hdr_y, ac_w, sr_h, Action::ToggleAutoClean);

    // 8 Mem Reduct Areas — 2 × 4 Dock-Modules Toggle Cards
    let areas = [
        (
            Icon::Zap,
            "Working Sets",
            ToggleId::AreaWs,
            Action::ToggleArea(0),
        ),
        (
            Icon::Wave,
            "Sys File Cache",
            ToggleId::AreaSys,
            Action::ToggleArea(1),
        ),
        (
            Icon::Activity,
            "Standby List",
            ToggleId::AreaStandby,
            Action::ToggleArea(2),
        ),
        (
            Icon::List,
            "Mod Page List",
            ToggleId::AreaModPage,
            Action::ToggleArea(3),
        ),
        (
            Icon::Droplet,
            "Standby Low",
            ToggleId::AreaStandbyLow,
            Action::ToggleArea(4),
        ),
        (
            Icon::Folder,
            "Mod File Cache",
            ToggleId::AreaModFile,
            Action::ToggleArea(5),
        ),
        (
            Icon::Settings,
            "Registry Cache",
            ToggleId::AreaRegistry,
            Action::ToggleArea(6),
        ),
        (
            Icon::Display,
            "Combine Lists",
            ToggleId::AreaCombine,
            Action::ToggleArea(7),
        ),
    ];
    let col_gap = 12.0;
    let row_gap = 10.0;
    let tile_w = (inner_w - col_gap) / 2.0;
    let tile_h = 38.0;
    let grid_y = py + 132.0;

    for (idx, &(ic, name, tid, act)) in areas.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let tx = ix + (col as f32) * (tile_w + col_gap);
        let ty = grid_y + (row as f32) * (tile_h + row_gap);
        let active = state.toggle_target(tid);
        let ht = p.hot_t(tx, ty, tile_w, tile_h);

        p.tactile_surface_pill_t(tx, ty, tile_w, tile_h, 10.0, ht, None);
        if active {
            p.rrect(
                tx,
                ty - 0.55 * ht,
                tile_w,
                tile_h,
                10.0,
                fade(rgba(124, 58, 237, 28), 0.7 + 0.3 * ht),
            );
        }
        let dy = -0.55 * ht;
        icons::draw(
            p,
            ic,
            tx + 12.0,
            ty + 12.0 + dy,
            14.0,
            if active {
                tok::cyno_violet_hi()
            } else {
                tok::text2()
            },
        );
        p.text(
            name,
            tx + 34.0,
            ty + 23.0 + dy,
            11.4,
            Weight::Semibold,
            if active {
                tok::text1()
            } else {
                tok::text_soft()
            },
        );
        let st = switch_val(state, tid);
        p.draw_toggle_switch(tx + tile_w - 48.0, ty + 10.0 + dy, 38.0, 18.0, st, ht);
        p.hit(tx, ty, tile_w, tile_h, act);
    }

    // Subtle separator & generous breathing room (34px) between Cleaning Areas and Clean Above / Clean Every
    p.rrect(ix, py + 330.0, inner_w, 1.0, 0.0, rgba(255, 255, 255, 16));

    let ctrl_x = px + 154.0;
    let ctrl_w = pw - 154.0 - PAD;
    draw_segmented_row(
        p,
        "Clean Above",
        ix,
        py + 348.0,
        ctrl_x,
        ctrl_w,
        state.mem_reduct.when_above_pct,
        &[
            ("Disable", 0, Action::SetCleanThreshold(0)),
            ("60%", 60, Action::SetCleanThreshold(60)),
            ("70%", 70, Action::SetCleanThreshold(70)),
            ("80%", 80, Action::SetCleanThreshold(80)),
            ("85%", 85, Action::SetCleanThreshold(85)),
            ("90%", 90, Action::SetCleanThreshold(90)),
        ],
    );
    draw_segmented_row(
        p,
        "Clean Every",
        ix,
        py + 348.0 + SEG_PITCH,
        ctrl_x,
        ctrl_w,
        state.mem_reduct.every_mins,
        &[
            ("Disable", 0, Action::SetCleanInterval(0)),
            ("5m", 5, Action::SetCleanInterval(5)),
            ("15m", 15, Action::SetCleanInterval(15)),
            ("30m", 30, Action::SetCleanInterval(30)),
            ("60m", 60, Action::SetCleanInterval(60)),
            ("90m", 90, Action::SetCleanInterval(90)),
        ],
    );

    // Bottom Status + Clean Memory CTA + Progress Bar
    let act_row_y = py + 440.0;
    let cta_w = 136.0;
    let cta_h = 30.0;
    let cta_x = ix + inner_w - cta_w;
    let status_ell = p.ellipsize(
        &state.mem_reduct.last_cleaned_status,
        10.2,
        Weight::Regular,
        inner_w - cta_w - 16.0,
    );
    p.text(
        &status_ell,
        ix,
        act_row_y + 19.0,
        10.2,
        Weight::Regular,
        tok::text2(),
    );

    let cta_ht = p.hot_t(cta_x, act_row_y, cta_w, cta_h);
    p.luminous_pill_button_t(
        cta_x,
        act_row_y,
        cta_w,
        cta_h,
        9.5,
        tok::cyno_indigo(),
        tok::cyno_violet(),
        tok::cyno_orchid(),
        cta_ht,
    );
    let cta_lbl = if state.mem_reduct.is_purging {
        "Cleaning..."
    } else {
        "Clean Memory"
    };
    p.text_center_shadow(
        cta_lbl,
        cta_x + cta_w / 2.0,
        act_row_y + 19.0 - 0.7 * cta_ht,
        10.8,
        Weight::Bold,
        tok::text1(),
    );
    p.hit(cta_x, act_row_y, cta_w, cta_h, Action::PurgeRam);

    let bar_pct = (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
    p.glowing_striped_bar(
        ix,
        py + 480.0,
        inner_w,
        6.0,
        bar_pct,
        tok::cyno_deep(),
        tok::cyno_violet_hi(),
    );

    // Card 2: OS Integration & Maintenance (h = 194)
    let c2_y = py + c1_h + CARD_GAP;
    let c2_h = 194.0;
    p.elevated_card(
        px,
        c2_y,
        pw,
        c2_h,
        16.0,
        Some((pw - 44.0, 28.0, 74.0, tok::cyno_orchid())),
    );
    p.text(
        "OS Integration & Maintenance",
        ix,
        c2_y + 26.0,
        13.5,
        Weight::Bold,
        tok::text1(),
    );

    let tile_y = c2_y + 48.0;
    let tile_h = 60.0;
    let t_gap = 12.0;
    let tile_w = (inner_w - t_gap * 2.0) / 3.0;
    let os_tiles = [
        (
            "Autostart",
            "Launch at login",
            ToggleId::Autostart,
            Action::ToggleAutostart,
        ),
        (
            "Chime SFX",
            "Feedback chimes",
            ToggleId::Chime,
            Action::ToggleChime,
        ),
        (
            "Screen Time",
            "App tracking",
            ToggleId::ScreenTime,
            Action::ToggleScreenTime,
        ),
    ];

    for (i, &(title, sub, tid, act)) in os_tiles.iter().enumerate() {
        let tx = ix + (i as f32) * (tile_w + t_gap);
        let ht = p.hot_t(tx, tile_y, tile_w, tile_h);
        let active = state.toggle_target(tid);
        p.tactile_surface_pill_t(tx, tile_y, tile_w, tile_h, 10.0, ht, None);
        if active {
            p.rrect(
                tx,
                tile_y - 0.5 * ht,
                tile_w,
                tile_h,
                10.0,
                fade(rgba(124, 58, 237, 24), 0.7 + 0.3 * ht),
            );
        }
        let dy = -0.5 * ht;
        p.text(
            title,
            tx + 14.0,
            tile_y + 23.0 + dy,
            11.2,
            Weight::Bold,
            tok::text1(),
        );
        let st = switch_val(state, tid);
        p.draw_toggle_switch(tx + tile_w - 50.0, tile_y + 10.0 + dy, 38.0, 18.0, st, ht);
        p.text(
            sub,
            tx + 14.0,
            tile_y + 44.0 + dy,
            9.6,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(tx, tile_y, tile_w, tile_h, act);
    }

    let act_y = c2_y + 128.0;
    let b_h = 36.0;

    let rs_x = ix;
    let rs_ht = p.hot_t(rs_x, act_y, tile_w, b_h);
    p.tactile_surface_pill_t(rs_x, act_y, tile_w, b_h, 9.5, rs_ht, None);
    p.text_center(
        "Reset Today's Stats",
        rs_x + tile_w / 2.0,
        act_y + 22.0 - 0.55 * rs_ht,
        10.4,
        Weight::Semibold,
        lerp_color(tok::text_soft(), tok::text1(), rs_ht),
    );
    p.hit(rs_x, act_y, tile_w, b_h, Action::ResetScreenTimeToday);

    let rd_x = rs_x + tile_w + t_gap;
    let rd_ht = p.hot_t(rd_x, act_y, tile_w, b_h);
    p.tactile_surface_pill_t(rd_x, act_y, tile_w, b_h, 9.5, rd_ht, None);
    p.text_center(
        "Reset All Defaults",
        rd_x + tile_w / 2.0,
        act_y + 22.0 - 0.55 * rd_ht,
        10.4,
        Weight::Semibold,
        lerp_color(tok::text_soft(), tok::text1(), rd_ht),
    );
    p.hit(rd_x, act_y, tile_w, b_h, Action::ResetAllSettings);

    let q_x = rd_x + tile_w + t_gap;
    let q_ht = p.hot_t(q_x, act_y, tile_w, b_h);
    p.tactile_surface_pill_t(
        q_x,
        act_y,
        tile_w,
        b_h,
        9.5,
        q_ht,
        Some(rgba(220, 38, 38, 255)),
    );
    p.text_center_shadow(
        "Exit CyNotch",
        q_x + tile_w / 2.0,
        act_y + 22.0 - 0.55 * q_ht,
        10.6,
        Weight::Bold,
        tok::text1(),
    );
    p.hit(q_x, act_y, tile_w, b_h, Action::Quit);
}
