#![allow(unsafe_op_in_unsafe_fn)]
//! Paired frosted-blur backdrop window.
//!
//! # Why a second window
//! DWM blur is per-HWND and rectangular, and `SetWindowRgn` is only honoured on
//! *redirected* windows. CyNotch's content HWND is `WS_EX_NOREDIRECTIONBITMAP`
//! (DirectComposition) with a canvas much larger than the visible shell, so any
//! blur applied to it paints a rectangle over the whole canvas. Blur and content
//! therefore live in two windows:
//!
//! * **content window** - unchanged, fully transparent DComp surface that draws
//!   the shell, UI, tint, noise and edge stroke.
//! * **backdrop window** (this module) - an ordinary redirected `WS_POPUP`,
//!   click-through, never activated, parked directly *below* the content window
//!   in z-order, shaped with `SetWindowRgn` to the shell silhouette and given an
//!   accent blur-behind.
//!
//! Everything here degrades instead of failing: if the window or the blur
//! cannot be created, [`Backdrop::create`] returns `None` and the caller falls
//! back to plain translucent glass.

use crate::backdrop_geom::Placement;
use std::ffi::{OsStr, c_void};
use std::mem::{size_of, size_of_val};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DWM_BB_ENABLE, DWM_BLURBEHIND, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmEnableBlurBehindWindow, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// Window class name. `ensure_topmost_zorder` skips windows of this class.
pub const CLASS_NAME_STR: &str = "CyNotchBackdropClass";
const CLASS_NAME: PCWSTR = w!("CyNotchBackdropClass");
const WINDOW_NAME: PCWSTR = w!("CyNotchBackdrop");

/// Master switch. `false` == exactly the v0.2.2 look (no backdrop windows).
pub const ENABLE_BLUR_BACKDROP: bool = true;

/// Extra inset (px) applied while the shell is animating, so a one-frame lag
/// between the two windows can never show blur outside the shell.
pub const ANIMATION_EXTRA_INSET: i32 = 2;

/// Resting inset (px). Hides GDI region aliasing under the shell's own edge.
pub const REST_INSET: i32 = 1;

/// Resting inset (px) sized so an ~8px Windows 11 `DWMWCP_ROUND` backdrop
/// corner never pokes outside a shell corner of radius `max_r`.
pub fn inset_for_radius(max_r: f32) -> i32 {
    if max_r <= 10.0 {
        REST_INSET
    } else {
        1 + ((max_r - 10.0) * 0.25).ceil() as i32
    }
}

/// If the one-frame lag during animation is visible on your machine, set this
/// to `true`: the blur is hidden while the shell moves and snaps back when it
/// settles.
pub const HIDE_WHILE_ANIMATING: bool = false;

/// Tint opacity (percent, see `tok::scale_frosted`) used when blur is active.
/// Without blur the translucent shell must be denser to stay readable.
pub const GLASS_FALLBACK_OPACITY_PCT: u32 = 118;

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

static BLUR_BROKEN: AtomicBool = AtomicBool::new(false);
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
static BLUR_PX: AtomicU32 = AtomicU32::new(20);

/// Updates the global blur intensity (clamped 4..=40). Existing backdrop
/// windows re-apply their accent policy on the next `apply` call.
pub fn set_blur_px(px: u32) {
    BLUR_PX.store(px.clamp(4, 40), Ordering::Release);
}

pub(crate) fn env_disables_frost(v: &OsStr) -> bool {
    if v.is_empty() {
        return false;
    }
    let s = v.to_string_lossy();
    s != "0" && !s.eq_ignore_ascii_case("false")
}

fn cli_disabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::args().any(|a| a.eq_ignore_ascii_case("--no-frost"))
            || std::env::var_os("CYNOTCH_NO_FROST").is_some_and(|v| env_disables_frost(&v))
    })
}

/// True while a blur backdrop may be created (switch on, no CLI opt-out, no
/// earlier permanent failure this session).
pub fn blur_available() -> bool {
    ENABLE_BLUR_BACKDROP && !cli_disabled() && !BLUR_BROKEN.load(Ordering::Acquire)
}

/// Marks blur as unusable for the rest of the session (creation or self-check
/// failed). Callers then render dense translucent glass instead.
pub fn mark_unavailable() {
    BLUR_BROKEN.store(true, Ordering::Release);
}

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WinCompAttrData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WinCompAttrData) -> BOOL;

unsafe fn resolve_set_wca() -> Option<SetWindowCompositionAttributeFn> {
    let user32 = GetModuleHandleW(w!("user32.dll")).ok()?;
    let proc = GetProcAddress(user32, s!("SetWindowCompositionAttribute"))?;
    // SAFETY: the export has exactly this signature on every Windows 10/11 build.
    Some(std::mem::transmute::<
        unsafe extern "system" fn() -> isize,
        SetWindowCompositionAttributeFn,
    >(proc))
}

/// Enables blur-behind on `hwnd`. Tries the accent API first (honours
/// `SetWindowRgn`), then the classic DWM call. Returns whether either took.
unsafe fn apply_blur(hwnd: HWND) -> bool {
    // The client area must be "glass" so pixels painted with alpha 0 reveal
    // the blur instead of an opaque redirection surface.
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

    if let Some(set_wca) = resolve_set_wca() {
        let px = BLUR_PX.load(Ordering::Acquire);
        let (accent_state, accent_flags, gradient_color) = if px > 20 {
            let t = ((px - 20) as f32 / 20.0).clamp(0.0, 1.0);
            let alpha = (12.0 + t * 36.0).round() as u32;
            (ACCENT_ENABLE_ACRYLICBLURBEHIND, 2, (alpha << 24) | 0x001C_1226)
        } else {
            (ACCENT_ENABLE_BLURBEHIND, 0, 0)
        };
        let mut policy = AccentPolicy {
            accent_state,
            accent_flags,
            gradient_color,
            animation_id: 0,
        };
        let mut data = WinCompAttrData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut policy as *mut AccentPolicy as *mut c_void,
            size: size_of::<AccentPolicy>(),
        };
        if set_wca(hwnd, &mut data).as_bool() {
            return true;
        }
    }

    let bb = DWM_BLURBEHIND {
        dwFlags: DWM_BB_ENABLE,
        fEnable: TRUE,
        hRgnBlur: HRGN::default(),
        fTransitionOnMaximized: FALSE,
    };
    DwmEnableBlurBehindWindow(hwnd, &bb).is_ok()
}

unsafe extern "system" fn backdrop_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match msg {
        // Never steal mouse or focus from the content window above us.
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            // Black + alpha 0 on a glass client area == "show the blur".
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                let brush = HBRUSH(GetStockObject(BLACK_BRUSH).0);
                FillRect(hdc, &ps.rcPaint, brush);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // DWM restarted or composition toggled: the accent policy is lost.
        0x031E /* WM_DWMCOMPOSITIONCHANGED */ => {
            let _ = apply_blur(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    })) {
        Ok(r) => r,
        Err(_) => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn ensure_class() -> bool {
    if CLASS_REGISTERED.load(Ordering::Acquire) {
        return true;
    }
    let Ok(h_module) = GetModuleHandleW(None) else {
        return false;
    };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(backdrop_wnd_proc),
        hInstance: h_module.into(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // Atom 0 means failure, but "class already exists" also lands here when a
    // previous (failed) run registered it: the later CreateWindowExW decides.
    let _ = RegisterClassExW(&wc);
    CLASS_REGISTERED.store(true, Ordering::Release);
    true
}

/// Builds the GDI region for a placement, in backdrop-window coordinates.
/// Returns an invalid handle on failure.
unsafe fn build_region(p: &Placement) -> HRGN {
    let (w, h, d) = (p.w, p.h, p.diam);
    let rgn = if !p.flush {
        CreateRoundRectRgn(0, 0, w, h, d, d)
    } else {
        // Edge-flush: round rect extended past the screen edge, clipped back to the
        // box, so the edge side stays square and the other three corners round.
        let ext = (d / 2).max(4);
        let (rx0, ry0, rx1, ry1) = match p.place {
            0 => (0, -ext, w, h),
            1 => (-ext, 0, w, h),
            _ => (0, 0, w + ext, h),
        };
        let r = CreateRoundRectRgn(rx0, ry0, rx1, ry1, d, d);
        if r.is_invalid() {
            return r;
        }
        let boxr = CreateRectRgn(0, 0, w, h);
        if !boxr.is_invalid() {
            let _ = CombineRgn(r, r, boxr, RGN_AND);
            let _ = DeleteObject(HGDIOBJ(boxr.0));
        }
        r
    };
    if rgn.is_invalid() {
        return rgn;
    }
    if p.sharp_mask != 0 {
        let rad = ((d + 1) / 2).max(2);
        let corners = [
            (1u8, 0, 0, rad.min(w), rad.min(h)),
            (2u8, (w - rad).max(0), 0, w, rad.min(h)),
            (4u8, (w - rad).max(0), (h - rad).max(0), w, h),
            (8u8, 0, (h - rad).max(0), rad.min(w), h),
        ];
        for (bit, x0, y0, x1, y1) in corners {
            if p.sharp_mask & bit != 0 && x1 > x0 && y1 > y0 {
                let patch = CreateRectRgn(x0, y0, x1, y1);
                if !patch.is_invalid() {
                    let _ = CombineRgn(rgn, rgn, patch, RGN_OR);
                    let _ = DeleteObject(HGDIOBJ(patch.0));
                }
            }
        }
    }
    rgn
}

/// Owns one backdrop HWND. UI-thread only. Destroys the window on drop.
pub struct Backdrop {
    hwnd: HWND,
    last: Option<Placement>,
    shown: bool,
    blur_px: u32,
}

impl Backdrop {
    /// Creates a hidden backdrop window with blur applied. `None` on any failure
    /// (the caller must then fall back to dense translucent glass).
    ///
    /// # Safety
    /// Call from the UI thread that will own (and later drop) the window.
    pub unsafe fn create() -> Option<Self> {
        if !blur_available() || !ensure_class() {
            return None;
        }
        let h_module = GetModuleHandleW(None).ok()?;
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            CLASS_NAME,
            WINDOW_NAME,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            h_module,
            None,
        )
        .ok()?;

        let no_transitions: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &no_transitions as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
        let corner = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const _ as *const c_void,
            size_of_val(&corner) as u32,
        );
        crate::borders::set_window_borderless(hwnd);

        if !apply_blur(hwnd) {
            let _ = DestroyWindow(hwnd);
            return None;
        }
        Some(Self {
            hwnd,
            last: None,
            shown: false,
            blur_px: BLUR_PX.load(Ordering::Acquire),
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Moves/shapes the backdrop to `placement` directly below `content`, or
    /// hides it when `placement` is `None`. Cheap when nothing changed.
    ///
    /// # Safety
    /// UI thread only; `content` must be a valid window handle.
    pub unsafe fn apply(&mut self, content: HWND, placement: Option<Placement>) {
        if !IsWindow(self.hwnd).as_bool() {
            return;
        }
        let Some(p) = placement else {
            if self.shown {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                self.shown = false;
            }
            self.last = None;
            return;
        };
        if !IsWindow(content).as_bool() {
            return;
        }

        let cur_blur_px = BLUR_PX.load(Ordering::Acquire);
        if self.blur_px != cur_blur_px {
            let _ = apply_blur(self.hwnd);
            self.blur_px = cur_blur_px;
        }

        let content_topmost = (GetWindowLongW(content, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
        let self_topmost = (GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
        let mut promoted = false;
        if content_topmost && !self_topmost {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            promoted = true;
        }

        let shape = (p.w, p.h, p.diam, p.flush, p.place, p.sharp_mask);
        let prev_shape = self
            .last
            .map(|l| (l.w, l.h, l.diam, l.flush, l.place, l.sharp_mask));
        let shape_changed = prev_shape != Some(shape);
        let pos_changed = self.last.is_none_or(|l| l.x != p.x || l.y != p.y);

        if !self.shown {
            // Position and shape while still hidden, then show with SWP_SHOWWINDOW
            // so the unclipped rectangle never flashes for a frame.
            let _ = SetWindowPos(
                self.hwnd,
                content,
                p.x,
                p.y,
                p.w,
                p.h,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            if shape_changed {
                let corner = if p.diam >= 8 && p.sharp_mask != 0x0F {
                    DWMWCP_ROUND
                } else {
                    DWMWCP_DONOTROUND
                };
                let _ = DwmSetWindowAttribute(
                    self.hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &corner as *const _ as *const c_void,
                    size_of_val(&corner) as u32,
                );
                let hrgn = build_region(&p);
                if !hrgn.is_invalid() && SetWindowRgn(self.hwnd, hrgn, false) == 0 {
                    // On success the system owns the region; on failure we must free it.
                    let _ = DeleteObject(HGDIOBJ(hrgn.0));
                }
            }
            let _ = SetWindowPos(
                self.hwnd,
                content,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );
            self.last = Some(p);
            self.shown = true;
        } else if shape_changed {
            let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            if !pos_changed {
                flags |= SWP_NOMOVE;
            }
            let _ = SetWindowPos(self.hwnd, content, p.x, p.y, p.w, p.h, flags);
            let corner = if p.diam >= 8 && p.sharp_mask != 0x0F {
                DWMWCP_ROUND
            } else {
                DWMWCP_DONOTROUND
            };
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const c_void,
                size_of_val(&corner) as u32,
            );
            let hrgn = build_region(&p);
            if !hrgn.is_invalid() && SetWindowRgn(self.hwnd, hrgn, true) == 0 {
                let _ = DeleteObject(HGDIOBJ(hrgn.0));
            }
            self.last = Some(p);
        } else if pos_changed {
            let _ = SetWindowPos(
                self.hwnd,
                content,
                p.x,
                p.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            self.last = Some(p);
        } else if promoted || !GetWindow(content, GW_HWNDNEXT).is_ok_and(|next| next == self.hwnd) {
            // Content was promoted (topmost re-assert, another window slipped
            // between us): slide back directly under it.
            let _ = SetWindowPos(
                self.hwnd,
                content,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        // SAFETY: only touches a handle this struct created; guarded by IsWindow.
        unsafe {
            if IsWindow(self.hwnd).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

/// Shared create / apply / drop policy for both CyNotch windows.
///
/// `slot` is the window's backdrop holder, `content` its content HWND and
/// `placement` where the blur should be (`None` = hidden). Returns `true` when
/// blur just became unavailable, so the caller should redraw with the denser
/// glass fallback.
///
/// # Safety
/// UI thread only; `content` must be the live content window of this backdrop.
pub unsafe fn sync_slot(
    slot: &mut Option<Backdrop>,
    content: HWND,
    placement: Option<Placement>,
) -> bool {
    if !blur_available() {
        *slot = None;
        return false;
    }
    if let Some(b) = slot.as_ref()
        && !IsWindow(b.hwnd()).as_bool()
    {
        *slot = None;
    }
    let mut failed = false;
    if placement.is_some() && slot.is_none() {
        *slot = Backdrop::create();
        if slot.is_none() {
            mark_unavailable();
            failed = true;
        }
    }
    if let Some(b) = slot.as_mut() {
        b.apply(content, placement);
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_disables_frost_treats_empty_zero_false_as_enabled() {
        assert!(!env_disables_frost(OsStr::new("")));
        assert!(!env_disables_frost(OsStr::new("0")));
        assert!(!env_disables_frost(OsStr::new("false")));
        assert!(!env_disables_frost(OsStr::new("FALSE")));
        assert!(!env_disables_frost(OsStr::new("False")));

        assert!(env_disables_frost(OsStr::new("1")));
        assert!(env_disables_frost(OsStr::new("true")));
        assert!(env_disables_frost(OsStr::new("yes")));
    }

    #[test]
    fn build_region_applies_sharp_mask_when_floating() {
        let p = Placement {
            x: 0,
            y: 0,
            w: 120,
            h: 48,
            diam: 24,
            flush: false,
            place: 0,
            sharp_mask: 1 | 2, // top-left & top-right sharp (corner-docked Floating)
        };
        unsafe {
            let rgn = build_region(&p);
            assert!(!rgn.is_invalid(), "build_region must return a valid HRGN");
            // Sharp top-left and top-right corners must be inside the region.
            assert!(
                PtInRegion(rgn, 0, 0).as_bool(),
                "Top-left corner should be sharp (inside region) when sharp_mask bit 1 is set"
            );
            assert!(
                PtInRegion(rgn, 118, 0).as_bool(),
                "Top-right corner should be sharp (inside region) when sharp_mask bit 2 is set"
            );
            // Unmasked bottom-left and bottom-right corners stay rounded (outside corner pixel).
            assert!(
                !PtInRegion(rgn, 0, 47).as_bool(),
                "Bottom-left corner should remain rounded (outside corner pixel)"
            );
            assert!(
                !PtInRegion(rgn, 119, 47).as_bool(),
                "Bottom-right corner should remain rounded (outside corner pixel)"
            );
            let _ = DeleteObject(HGDIOBJ(rgn.0));
        }
    }
}
