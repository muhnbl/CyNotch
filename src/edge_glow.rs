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
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_POINT_2F, D2D_RECT_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_GRADIENT_STOP,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_PRIMITIVE_BLEND_ADD, D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, ID2D1DeviceContext, ID2D1Factory1, ID2D1GeometrySink,
    ID2D1LinearGradientBrush, ID2D1PathGeometry, ID2D1RadialGradientBrush, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

pub const CLASS_NAME_STR: &str = "CyNotchEdgeGlowClass";
const CLASS_NAME: PCWSTR = w!("CyNotchEdgeGlowClass");
const WINDOW_NAME: PCWSTR = w!("CyNotchEdgeGlow");

/// Depth (px) the volumetric purple portal glow spills inward from the monitor bezel.
pub const EDGE_GLOW_DEPTH: u32 = 136;
/// Horizontal span (px) along the top screen edge.
pub const EDGE_GLOW_TOP_SPAN: u32 = 560;
/// Vertical span (px) along the left/right screen edge.
pub const EDGE_GLOW_SIDE_SPAN: u32 = 460;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Computes the screen-space `(x, y, width, height)` for the monitor-edge glow strip.
/// Strictly flush with the monitor bezel (`mon.top` / `mon.left` / `mon.right`)
/// and centered around the notch's screen center, clamped to monitor bounds.
pub fn compute_edge_glow_rect(
    mon: RECT,
    placement: NotchPlacement,
    notch_cx: i32,
    notch_cy: i32,
    scale_factor: f32,
) -> (i32, i32, u32, u32) {
    let s = scale_factor.max(0.5);
    let mon_w = (mon.right - mon.left).max(1);
    let mon_h = (mon.bottom - mon.top).max(1);
    let top_span = (EDGE_GLOW_TOP_SPAN as f32 * s).round() as i32;
    let side_span = (EDGE_GLOW_SIDE_SPAN as f32 * s).round() as i32;
    let depth = (EDGE_GLOW_DEPTH as f32 * s).round().max(8.0) as i32;

    match placement {
        NotchPlacement::Top => {
            let w = top_span.min(mon_w).max(1);
            let h = depth.min(mon_h).max(1);
            let max_x = (mon.right - w).max(mon.left);
            let x = (notch_cx - w / 2).clamp(mon.left, max_x);
            (x, mon.top, w as u32, h as u32)
        }
        NotchPlacement::Left => {
            let w = depth.min(mon_w).max(1);
            let h = side_span.min(mon_h).max(1);
            let max_y = (mon.bottom - h).max(mon.top);
            let y = (notch_cy - h / 2).clamp(mon.top, max_y);
            (mon.left, y, w as u32, h as u32)
        }
        NotchPlacement::Right => {
            let w = depth.min(mon_w).max(1);
            let h = side_span.min(mon_h).max(1);
            let max_y = (mon.bottom - h).max(mon.top);
            let y = (notch_cy - h / 2).clamp(mon.top, max_y);
            (mon.right - w, y, w as u32, h as u32)
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
    let rt: ID2D1RenderTarget = ctx.cast()?;
    let coll = unsafe {
        rt.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?
    };
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center,
        gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
        radiusX: rx.max(1.0),
        radiusY: ry.max(1.0),
    };
    unsafe { ctx.CreateRadialGradientBrush(&props, None, &coll) }
}

#[allow(dead_code)]
fn create_linear_brush(
    ctx: &ID2D1DeviceContext,
    start: D2D_POINT_2F,
    end: D2D_POINT_2F,
    stops: &[D2D1_GRADIENT_STOP],
) -> Result<ID2D1LinearGradientBrush> {
    let rt: ID2D1RenderTarget = ctx.cast()?;
    let coll = unsafe {
        rt.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?
    };
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
        startPoint: start,
        endPoint: end,
    };
    unsafe { ctx.CreateLinearGradientBrush(&props, None, &coll) }
}

fn create_crescent_path(
    ctx: &ID2D1DeviceContext,
    placement: NotchPlacement,
    w: f32,
    h: f32,
    slit_half: f32,
    depth: f32,
) -> Result<ID2D1PathGeometry> {
    let rt: ID2D1RenderTarget = ctx.cast()?;
    let factory: ID2D1Factory1 = unsafe { rt.GetFactory()?.cast()? };
    let geom: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()?.into() };
    let sink: ID2D1GeometrySink = unsafe { geom.Open()? };

    unsafe {
        match placement {
            NotchPlacement::Top => {
                let cx = w * 0.5;
                let start = D2D_POINT_2F { x: cx - slit_half, y: 0.0 };
                let end = D2D_POINT_2F { x: cx + slit_half, y: 0.0 };
                let apex = D2D_POINT_2F { x: cx, y: depth };

                sink.BeginFigure(start, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: cx - slit_half + slit_half * 0.25, y: depth * 0.50 },
                    point2: D2D_POINT_2F { x: cx - slit_half * 0.35, y: depth * 0.90 },
                    point3: apex,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: cx + slit_half * 0.35, y: depth * 0.90 },
                    point2: D2D_POINT_2F { x: cx + slit_half - slit_half * 0.25, y: depth * 0.50 },
                    point3: end,
                });
                sink.AddLine(start);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
            }
            NotchPlacement::Left => {
                let cy = h * 0.5;
                let start = D2D_POINT_2F { x: 0.0, y: cy - slit_half };
                let end = D2D_POINT_2F { x: 0.0, y: cy + slit_half };
                let apex = D2D_POINT_2F { x: depth, y: cy };

                sink.BeginFigure(start, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: depth * 0.50, y: cy - slit_half + slit_half * 0.25 },
                    point2: D2D_POINT_2F { x: depth * 0.90, y: cy - slit_half * 0.35 },
                    point3: apex,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: depth * 0.90, y: cy + slit_half * 0.35 },
                    point2: D2D_POINT_2F { x: depth * 0.50, y: cy + slit_half - slit_half * 0.25 },
                    point3: end,
                });
                sink.AddLine(start);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
            }
            NotchPlacement::Right => {
                let cy = h * 0.5;
                let start = D2D_POINT_2F { x: w, y: cy - slit_half };
                let end = D2D_POINT_2F { x: w, y: cy + slit_half };
                let apex = D2D_POINT_2F { x: w - depth, y: cy };

                sink.BeginFigure(start, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: w - depth * 0.50, y: cy - slit_half + slit_half * 0.25 },
                    point2: D2D_POINT_2F { x: w - depth * 0.90, y: cy - slit_half * 0.35 },
                    point3: apex,
                });
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: D2D_POINT_2F { x: w - depth * 0.90, y: cy + slit_half * 0.35 },
                    point2: D2D_POINT_2F { x: w - depth * 0.50, y: cy + slit_half - slit_half * 0.25 },
                    point3: end,
                });
                sink.AddLine(start);
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
            }
        }
    }
    Ok(geom)
}

/// Renders the volumetric purple portal slit + deep atmospheric bloom into `ctx`.
/// Safe for both live DComp rendering and offscreen `snap_d2d` visual verification.
pub fn render_edge_glow_d2d(
    ctx: &ID2D1DeviceContext,
    _cache: &mut D2DResourceCache,
    placement: NotchPlacement,
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

    // Portal aperture parameters: expansive spatial presence, soft atmospheric opacity fade
    let span_t = (0.65 + 0.35 * (1.0 - (1.0 - e).powi(2))).min(1.0);
    let depth_t = (0.60 + 0.40 * (1.0 - (1.0 - e).powi(2))).min(1.0);
    let alpha_t = (e.powf(1.10) * 0.72).clamp(0.0, 1.0);

    let (edge_len, max_depth) = match placement {
        NotchPlacement::Top => (w, h),
        NotchPlacement::Left | NotchPlacement::Right => (h, w),
    };

    let slit_half = (edge_len * 0.42 * span_t).min(edge_len * 0.46).max(8.0);
    let crescent_depth = (max_depth * 0.94 * depth_t).min(max_depth - 1.0).max(4.0);

    let center = match placement {
        NotchPlacement::Top => D2D_POINT_2F { x: w * 0.5, y: 0.0 },
        NotchPlacement::Left => D2D_POINT_2F { x: 0.0, y: h * 0.5 },
        NotchPlacement::Right => D2D_POINT_2F { x: w, y: h * 0.5 },
    };

    let bounds = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };

    // Create geometric mask matching the needle-cusp crescent lens
    if let Ok(crescent_geo) = create_crescent_path(ctx, placement, w, h, slit_half, crescent_depth) {
        let layer_params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: bounds,
            geometricMask: ManuallyDrop::new(Some(crescent_geo.into())),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };

        unsafe {
            ctx.PushLayer(&layer_params, None);

            // Layer 1: Ethereal Deep-Violet Atmospheric Fog (delicate translucent feather)
            let outer_stops = [
                D2D1_GRADIENT_STOP { position: 0.00, color: d2d_rgba(150, 75, 245, 0.65 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.25, color: d2d_rgba(125, 48, 230, 0.48 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.55, color: d2d_rgba(90, 24, 185, 0.26 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.78, color: d2d_rgba(55, 10, 130, 0.09 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.92, color: d2d_rgba(25, 3, 70, 0.02 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 1.00, color: d2d_rgba(10, 1, 30, 0.0) },
            ];
            let (o_rx, o_ry) = match placement {
                NotchPlacement::Top => (slit_half * 0.98, crescent_depth * 0.98),
                NotchPlacement::Left | NotchPlacement::Right => (crescent_depth * 0.98, slit_half * 0.98),
            };
            if let Ok(brush) = create_radial_brush(ctx, center, o_rx, o_ry, &outer_stops) {
                ctx.FillRectangle(&bounds, &brush);
            }

            // Layer 2: Gentle Electric-Orchid Volumetric Crescent (Subtle additive bloom)
            let mid_stops = [
                D2D1_GRADIENT_STOP { position: 0.00, color: d2d_rgba(195, 130, 255, 0.46 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.30, color: d2d_rgba(160, 85, 245, 0.30 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.60, color: d2d_rgba(115, 40, 215, 0.12 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.85, color: d2d_rgba(70, 15, 155, 0.025 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 1.00, color: d2d_rgba(40, 6, 95, 0.0) },
            ];
            let (m_rx, m_ry) = match placement {
                NotchPlacement::Top => (slit_half * 0.88, crescent_depth * 0.86),
                NotchPlacement::Left | NotchPlacement::Right => (crescent_depth * 0.86, slit_half * 0.88),
            };
            ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
            if let Ok(brush) = create_radial_brush(ctx, center, m_rx, m_ry, &mid_stops) {
                ctx.FillRectangle(&bounds, &brush);
            }

            // Layer 3: Soft Lavender Iris (Gentle seamless inner radiance, no hard white line)
            let inner_stops = [
                D2D1_GRADIENT_STOP { position: 0.00, color: d2d_rgba(225, 175, 255, 0.38 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.40, color: d2d_rgba(185, 120, 245, 0.20 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 0.75, color: d2d_rgba(135, 55, 215, 0.05 * alpha_t) },
                D2D1_GRADIENT_STOP { position: 1.00, color: d2d_rgba(85, 18, 160, 0.0) },
            ];
            let (i_rx, i_ry) = match placement {
                NotchPlacement::Top => (slit_half * 0.58, crescent_depth * 0.54),
                NotchPlacement::Left | NotchPlacement::Right => (crescent_depth * 0.54, slit_half * 0.58),
            };
            if let Ok(brush) = create_radial_brush(ctx, center, i_rx, i_ry, &inner_stops) {
                ctx.FillRectangle(&bounds, &brush);
            }

            ctx.PopLayer();
            let _ = ManuallyDrop::into_inner(layer_params.geometricMask);
            let _ = ManuallyDrop::into_inner(layer_params.opacityBrush);
        }
    }

    unsafe { ctx.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER) };
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
        placement: NotchPlacement,
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
                    render_edge_glow_d2d(ctx, cache, placement, w, h, intensity);
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
                        render_edge_glow_d2d(ctx, c, placement, w, h, intensity);
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
