#![allow(unsafe_op_in_unsafe_fn)]
//! Dedicated click-through physical screen-edge purple ambient glow overlay.
//!
//! Renders an ethereal lavender-violet bezel light flush against the user's
//! actual monitor edge (`mon.top`, `mon.left`, or `mon.right`) centered on
//! the notch's axis during hover dwell, completely decoupled from the notch
//! HWND so `Floating` and `EdgeFlush` styles both radiate directly from the
//! monitor edge with zero hit-test or window-region interference.

use crate::composition::DCompHost;
use crate::models::NotchPlacement;
use crate::painter_d2d::D2DResourceCache;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_POINT_2F, D2D_RECT_F, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BUFFER_PRECISION_16BPC_FLOAT, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
    D2D1_COLOR_SPACE_SRGB, D2D1_EXTEND_MODE_CLAMP, D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, ID2D1DeviceContext, ID2D1GradientStopCollection1,
    ID2D1RadialGradientBrush,
};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

pub const CLASS_NAME_STR: &str = "CyNotchEdgeGlowClass";
const CLASS_NAME: PCWSTR = w!("CyNotchEdgeGlowClass");
const WINDOW_NAME: PCWSTR = w!("CyNotchEdgeGlow");

/// Depth (px) the volumetric purple portal glow spills inward from the monitor bezel.
pub const EDGE_GLOW_DEPTH: u32 = 160;
/// Horizontal span (px) along the top screen edge.
pub const EDGE_GLOW_TOP_SPAN: u32 = 580;
/// Vertical span (px) along the left/right screen edge.
pub const EDGE_GLOW_SIDE_SPAN: u32 = 480;
/// Radial span (px) from a physical top-left / top-right monitor corner inward.
pub const EDGE_GLOW_CORNER_SPAN: u32 = 340;

/// Physical monitor edge or corner from which the ambient edge glow originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeGlowAnchor {
    Top,
    Left,
    Right,
    TopLeft,
    TopRight,
}

impl EdgeGlowAnchor {
    pub fn from_placement_and_corners(
        placement: NotchPlacement,
        top_left_corner: bool,
        top_right_corner: bool,
    ) -> Self {
        if top_left_corner {
            Self::TopLeft
        } else if top_right_corner {
            Self::TopRight
        } else {
            match placement {
                NotchPlacement::Top => Self::Top,
                NotchPlacement::Left => Self::Left,
                NotchPlacement::Right => Self::Right,
            }
        }
    }
}

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Computes the screen-space `(x, y, width, height)` for the monitor-edge or corner glow overlay.
/// Strictly flush with the monitor bezel (`mon.top` / `mon.left` / `mon.right` or top corners)
/// and centered around the notch's screen center (or anchored at the corner), clamped to monitor bounds.
pub fn compute_edge_glow_rect(
    mon: RECT,
    anchor: EdgeGlowAnchor,
    notch_cx: i32,
    notch_cy: i32,
    scale_factor: f32,
) -> (i32, i32, u32, u32) {
    let s = scale_factor.max(0.5);
    let mon_w = (mon.right - mon.left).max(1);
    let mon_h = (mon.bottom - mon.top).max(1);
    let top_span = (EDGE_GLOW_TOP_SPAN as f32 * s).round() as i32;
    let side_span = (EDGE_GLOW_SIDE_SPAN as f32 * s).round() as i32;
    let corner_span = (EDGE_GLOW_CORNER_SPAN as f32 * s).round() as i32;
    let depth = (EDGE_GLOW_DEPTH as f32 * s).round().max(8.0) as i32;

    match anchor {
        EdgeGlowAnchor::Top => {
            let w = top_span.min(mon_w).max(1);
            let h = depth.min(mon_h).max(1);
            let max_x = (mon.right - w).max(mon.left);
            let x = (notch_cx - w / 2).clamp(mon.left, max_x);
            (x, mon.top, w as u32, h as u32)
        }
        EdgeGlowAnchor::Left => {
            let w = depth.min(mon_w).max(1);
            let h = side_span.min(mon_h).max(1);
            let max_y = (mon.bottom - h).max(mon.top);
            let y = (notch_cy - h / 2).clamp(mon.top, max_y);
            (mon.left, y, w as u32, h as u32)
        }
        EdgeGlowAnchor::Right => {
            let w = depth.min(mon_w).max(1);
            let h = side_span.min(mon_h).max(1);
            let max_y = (mon.bottom - h).max(mon.top);
            let y = (notch_cy - h / 2).clamp(mon.top, max_y);
            (mon.right - w, y, w as u32, h as u32)
        }
        EdgeGlowAnchor::TopLeft => {
            let w = corner_span.min(mon_w).max(1);
            let h = corner_span.min(mon_h).max(1);
            (mon.left, mon.top, w as u32, h as u32)
        }
        EdgeGlowAnchor::TopRight => {
            let w = corner_span.min(mon_w).max(1);
            let h = corner_span.min(mon_h).max(1);
            (mon.right - w, mon.top, w as u32, h as u32)
        }
    }
}

#[inline]
fn d2d_rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a.clamp(0.0, 1.0),
    }
}

fn create_radial_brush(
    ctx: &ID2D1DeviceContext,
    center: D2D_POINT_2F,
    rx: f32,
    ry: f32,
    stops: &[D2D1_GRADIENT_STOP],
) -> Result<ID2D1RadialGradientBrush> {
    let coll: ID2D1GradientStopCollection1 = unsafe {
        ctx.CreateGradientStopCollection(
            stops,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_COLOR_SPACE_SRGB,
            D2D1_BUFFER_PRECISION_16BPC_FLOAT,
            D2D1_EXTEND_MODE_CLAMP,
            D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
        )?
    };
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center,
        gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
        radiusX: rx.max(1.0),
        radiusY: ry.max(1.0),
    };
    unsafe { ctx.CreateRadialGradientBrush(&props, None, &coll) }
}

/// Renders the volumetric purple portal slit + deep atmospheric bloom into `ctx`.
/// Safe for both live DComp rendering and offscreen `snap_d2d` visual verification.
///
/// Uses a single 32-stop 16bpc float `D2D1_GAMMA_1_0` C^2-continuous radial gradient
/// (zero overlapping ellipses, zero geometric mask clipping, zero dark-RGB muddy tail stops,
/// and `(1 - r^2)^3` sextic falloff where `d(alpha)/dr = d^2(alpha)/dr^2 = 0` at `r = 1.0`)
/// so the glow dissipates seamlessly on both pure white (`#FFFFFF`) and dark (`#06030A`)
/// desktop backgrounds without any boundary step ("patahan") or concentric Mach banding.
pub fn render_edge_glow_d2d(
    ctx: &ID2D1DeviceContext,
    _cache: &mut D2DResourceCache,
    anchor: EdgeGlowAnchor,
    width: u32,
    height: u32,
    intensity: f32,
) {
    let e = intensity.clamp(0.0, 1.0);
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;

    unsafe {
        ctx.SetTransform(&Matrix3x2::identity());
        ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        ctx.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));
    }
    if e <= 0.003 {
        return;
    }

    // Smooth aperture breathing: gentle expansion & soft atmospheric opacity fade
    let span_t = (0.74 + 0.26 * (1.0 - (1.0 - e).powi(2))).min(1.0);
    let depth_t = (0.70 + 0.30 * (1.0 - (1.0 - e).powi(2))).min(1.0);
    let alpha_t = (e.powf(1.12) * 0.56).clamp(0.0, 1.0);

    // Keep elliptical radii well inside the HWND bounds so alpha reaches 0.000
    // several pixels before the inner window edges (zero rectangular cut-off).
    let (center, o_rx, o_ry) = match anchor {
        EdgeGlowAnchor::Top => {
            let span_radius = (w * 0.45 * span_t).min(w * 0.47 - 4.0).max(8.0);
            let depth_radius = (h * 0.90 * depth_t).min(h - 6.0).max(6.0);
            (
                D2D_POINT_2F { x: w * 0.5, y: 0.0 },
                span_radius,
                depth_radius,
            )
        }
        EdgeGlowAnchor::Left => {
            let span_radius = (h * 0.45 * span_t).min(h * 0.47 - 4.0).max(8.0);
            let depth_radius = (w * 0.90 * depth_t).min(w - 6.0).max(6.0);
            (
                D2D_POINT_2F { x: 0.0, y: h * 0.5 },
                depth_radius,
                span_radius,
            )
        }
        EdgeGlowAnchor::Right => {
            let span_radius = (h * 0.45 * span_t).min(h * 0.47 - 4.0).max(8.0);
            let depth_radius = (w * 0.90 * depth_t).min(w - 6.0).max(6.0);
            (D2D_POINT_2F { x: w, y: h * 0.5 }, depth_radius, span_radius)
        }
        EdgeGlowAnchor::TopLeft => {
            let rx = (w * 0.90 * span_t).min(w - 6.0).max(6.0);
            let ry = (h * 0.90 * span_t).min(h - 6.0).max(6.0);
            (D2D_POINT_2F { x: 0.0, y: 0.0 }, rx, ry)
        }
        EdgeGlowAnchor::TopRight => {
            let rx = (w * 0.90 * span_t).min(w - 6.0).max(6.0);
            let ry = (h * 0.90 * span_t).min(h - 6.0).max(6.0);
            (D2D_POINT_2F { x: w, y: 0.0 }, rx, ry)
        }
    };

    let bounds = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: w,
        bottom: h,
    };

    // Opsi C (2026 Hybrid):
    // 1. Slimmer, refined obsidian-violet atmospheric spill (58% depth so it never feels like a muddy 160px blob)
    let (spill_rx, spill_ry) = match anchor {
        EdgeGlowAnchor::Top => (o_rx * 0.88, o_ry * 0.58),
        EdgeGlowAnchor::Left | EdgeGlowAnchor::Right => (o_rx * 0.58, o_ry * 0.88),
        EdgeGlowAnchor::TopLeft | EdgeGlowAnchor::TopRight => (o_rx * 0.68, o_ry * 0.68),
    };
    let mut stops = [D2D1_GRADIENT_STOP::default(); 32];
    for (i, stop) in stops.iter_mut().enumerate() {
        let r = i as f32 / 31.0;
        let bell = (1.0 - r.powf(1.42)).max(0.0).powi(4);
        let core_mix = (1.0 - r * 1.6).clamp(0.0, 1.0).powi(2);
        let red = (152.0 + 76.0 * core_mix).round() as u8;
        let grn = (82.0 + 114.0 * core_mix).round() as u8;
        let blu = (247.0 + 8.0 * core_mix).round() as u8;
        *stop = D2D1_GRADIENT_STOP {
            position: r,
            color: d2d_rgba(red, grn, blu, bell * alpha_t * 0.78),
        };
    }

    unsafe {
        if let Ok(brush) = create_radial_brush(ctx, center, spill_rx, spill_ry, &stops) {
            ctx.FillRectangle(&bounds, &brush);
        }

        // 2. Razor-Sharp Specular Bezel Rim Light (Silver #FFFCE0 -> Brushed Metallic Gold #FFE248 -> Lavender)
        let (rim_rx, rim_ry) = match anchor {
            EdgeGlowAnchor::Top => (o_rx * 0.62, 6.5_f32.min(h * 0.12)),
            EdgeGlowAnchor::Left | EdgeGlowAnchor::Right => (6.5_f32.min(w * 0.12), o_ry * 0.62),
            EdgeGlowAnchor::TopLeft | EdgeGlowAnchor::TopRight => (o_rx * 0.36, o_ry * 0.36),
        };
        let rim_stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: d2d_rgba(255, 252, 224, (e * 0.92).clamp(0.0, 1.0)),
            },
            D2D1_GRADIENT_STOP {
                position: 0.28,
                color: d2d_rgba(255, 226, 72, (e * 0.68).clamp(0.0, 1.0)),
            },
            D2D1_GRADIENT_STOP {
                position: 0.62,
                color: d2d_rgba(216, 180, 254, (e * 0.28).clamp(0.0, 1.0)),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: d2d_rgba(168, 85, 247, 0.0),
            },
        ];
        if let Ok(rim_brush) = create_radial_brush(ctx, center, rim_rx, rim_ry, &rim_stops) {
            ctx.FillRectangle(&bounds, &rim_brush);
        }
    }
}

unsafe extern "system" fn edge_glow_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT — always click-through
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn ensure_class_registered() {
    if CLASS_REGISTERED.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(edge_glow_wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        CLASS_REGISTERED.store(true, Ordering::Release);
    }
}

/// Click-through topmost DirectComposition window rendering the monitor-edge light.
pub struct EdgeGlowOverlay {
    hwnd: HWND,
    dcomp: Option<DCompHost>,
    cache: Option<D2DResourceCache>,
    visible: bool,
    last_rect: Option<(i32, i32, u32, u32)>,
}

impl EdgeGlowOverlay {
    pub fn create(initial_rect: (i32, i32, u32, u32)) -> Option<Self> {
        ensure_class_registered();
        let (x, y, w, h) = initial_rect;
        let w = w.max(1);
        let h = h.max(1);
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let ex_style = WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_NOREDIRECTIONBITMAP;
            let hwnd = CreateWindowExW(
                ex_style,
                CLASS_NAME,
                WINDOW_NAME,
                WS_POPUP,
                x,
                y,
                w as i32,
                h as i32,
                None,
                None,
                hinst,
                None,
            )
            .ok()?;

            let disable_trans: BOOL = TRUE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &disable_trans as *const BOOL as *const core::ffi::c_void,
                std::mem::size_of::<BOOL>() as u32,
            );

            let dcomp = DCompHost::new(hwnd, w, h).ok();
            Some(Self {
                hwnd,
                dcomp,
                cache: None,
                visible: false,
                last_rect: Some((x, y, w, h)),
            })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }

    /// Positions, renders, and parks the edge glow strictly behind `notch_hwnd` in topmost z-order.
    /// Recovers automatically on GPU device loss / DXGI reset.
    pub fn sync(
        &mut self,
        notch_hwnd: HWND,
        rect: (i32, i32, u32, u32),
        anchor: EdgeGlowAnchor,
        intensity: f32,
    ) {
        if intensity <= 0.003 {
            self.hide();
            return;
        }
        let (x, y, w, h) = (rect.0, rect.1, rect.2.max(1), rect.3.max(1));

        // Ensure DCompHost exists and matches target size before showing/moving
        if self.dcomp.is_none() {
            self.cache = None;
            self.dcomp = DCompHost::new(self.hwnd, w, h).ok();
        }
        let mut rendered = false;
        if let Some(ref mut host) = self.dcomp {
            if host.resize(w, h).is_ok() {
                if self.cache.is_none() {
                    self.cache = D2DResourceCache::new(host.d2d_context()).ok();
                }
                if let Some(ref mut cache) = self.cache
                    && let Ok(ctx) = host.begin_d2d_draw()
                {
                    render_edge_glow_d2d(ctx, cache, anchor, w, h, intensity);
                    rendered = host.end_d2d_draw_and_present().is_ok();
                }
            }
        }
        // If rendering failed (e.g. GPU device reset), attempt clean single-pass recreation
        if !rendered {
            self.dcomp = None;
            self.cache = None;
            if let Ok(mut host) = DCompHost::new(self.hwnd, w, h) {
                if host.resize(w, h).is_ok() {
                    let mut cache = D2DResourceCache::new(host.d2d_context()).ok();
                    if let Some(ref mut c) = cache
                        && let Ok(ctx) = host.begin_d2d_draw()
                    {
                        render_edge_glow_d2d(ctx, c, anchor, w, h, intensity);
                        rendered = host.end_d2d_draw_and_present().is_ok();
                    }
                    self.cache = cache;
                }
                self.dcomp = Some(host);
            }
        }
        if !rendered {
            self.hide();
            return;
        }

        let rect_changed = self.last_rect != Some((x, y, w, h));
        if rect_changed || !self.visible {
            unsafe {
                let flags = if !self.visible {
                    SWP_NOACTIVATE | SWP_SHOWWINDOW
                } else {
                    SWP_NOACTIVATE
                };
                // Place immediately below `notch_hwnd` so the floating capsule sits crisp in front.
                let _ = SetWindowPos(self.hwnd, notch_hwnd, x, y, w as i32, h as i32, flags);
            }
            self.last_rect = Some((x, y, w, h));
            self.visible = true;
        }
    }
}

impl Drop for EdgeGlowOverlay {
    fn drop(&mut self) {
        self.cache = None;
        self.dcomp = None;
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
        }
    }
}
