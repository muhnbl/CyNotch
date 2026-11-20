//! Direct2D-backed painter with resource caching for zero-COM-allocation hot paths.
//! Mirrors the `Painter` API from `painter.rs` but targets `ID2D1DeviceContext`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::mem::ManuallyDrop;
use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1GeometrySink, ID2D1GradientStopCollection, ID2D1LinearGradientBrush, ID2D1PathGeometry,
    ID2D1RadialGradientBrush, ID2D1RenderTarget, ID2D1RoundedRectangleGeometry,
    ID2D1SolidColorBrush, D2D1_ARC_SIZE, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_ROUNDED_RECT,
};

pub use crate::font_d2d::Weight;
use std::borrow::Cow;
use crate::models::Action;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    #[inline]
    pub fn from_xywh(x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        if w > 0.0 && h > 0.0 && x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() {
            Some(Self { x, y, w, h })
        } else {
            None
        }
    }
    #[inline] pub fn x(&self) -> f32 { self.x }
    #[inline] pub fn y(&self) -> f32 { self.y }
    #[inline] pub fn width(&self) -> f32 { self.w }
    #[inline] pub fn height(&self) -> f32 { self.h }
    #[inline] pub fn left(&self) -> f32 { self.x }
    #[inline] pub fn top(&self) -> f32 { self.y }
    #[inline] pub fn right(&self) -> f32 { self.x + self.w }
    #[inline] pub fn bottom(&self) -> f32 { self.y + self.h }
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub rect: Rect,
    pub action: Action,
}

pub struct StackStr<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackStr<N> {
    #[inline]
    pub fn new() -> Self {
        Self { buf: [0u8; N], len: 0 }
    }
    #[inline]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Default for StackStr<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> std::ops::Deref for StackStr<N> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str { self.as_str() }
}

impl<const N: usize> std::fmt::Write for StackStr<N> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let avail = N.saturating_sub(self.len);
        let bytes = s.as_bytes();
        if bytes.len() <= avail {
            self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
            Ok(())
        } else {
            let mut copy_len = 0;
            for ch in s.chars() {
                let clen = ch.len_utf8();
                if copy_len + clen > avail { break; }
                copy_len += clen;
            }
            self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
            self.len += copy_len;
            Ok(())
        }
    }
}

/// 2D transform matrix (3x2, row-major): [sx, ky, kx, sy, tx, ty]
/// Matches tiny_skia::Transform layout for zero-change call sites.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Transform {
    pub sx: f32, pub ky: f32, pub kx: f32, pub sy: f32, pub tx: f32, pub ty: f32,
}

impl Transform {
    #[inline]
    pub const fn identity() -> Self {
        Self { sx: 1.0, ky: 0.0, kx: 0.0, sy: 1.0, tx: 0.0, ty: 0.0 }
    }

    #[inline]
    pub fn from_translate(tx: f32, ty: f32) -> Self {
        Self { sx: 1.0, ky: 0.0, kx: 0.0, sy: 1.0, tx, ty }
    }

    #[inline]
    pub fn from_scale(sx: f32, sy: f32) -> Self {
        Self { sx, ky: 0.0, kx: 0.0, sy, tx: 0.0, ty: 0.0 }
    }

    #[inline]
    pub fn from_rotate(deg: f32) -> Self {
        let rad = deg * PI / 180.0;
        let c = rad.cos();
        let s = rad.sin();
        Self { sx: c, ky: s, kx: -s, sy: c, tx: 0.0, ty: 0.0 }
    }

    #[inline]
    pub const fn from_row(sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self { sx, ky, kx, sy, tx, ty }
    }

    #[inline]
    pub fn post_translate(&mut self, tx: f32, ty: f32) {
        self.tx += tx;
        self.ty += ty;
    }

    #[inline]
    pub fn post_scale(&mut self, sx: f32, sy: f32) {
        self.sx *= sx;
        self.ky *= sy;
        self.kx *= sx;
        self.sy *= sy;
        self.tx *= sx;
        self.ty *= sy;
    }

    #[inline]
    pub fn post_concat(&mut self, other: Transform) {
        // other * self (apply self first, then other)
        let nsx = other.sx * self.sx + other.kx * self.ky;
        let nky = other.ky * self.sx + other.sy * self.ky;
        let nkx = other.sx * self.kx + other.kx * self.sy;
        let nsy = other.ky * self.kx + other.sy * self.sy;
        let ntx = other.sx * self.tx + other.kx * self.ty + other.tx;
        let nty = other.ky * self.tx + other.sy * self.ty + other.ty;
        self.sx = nsx; self.ky = nky; self.kx = nkx; self.sy = nsy; self.tx = ntx; self.ty = nty;
    }

    #[inline]
    pub fn to_d2d_matrix3x2(self) -> windows::Foundation::Numerics::Matrix3x2 {
        windows::Foundation::Numerics::Matrix3x2 {
            M11: self.sx, M12: self.ky,
            M21: self.kx, M22: self.sy,
            M31: self.tx, M32: self.ty,
        }
    }

    #[inline]
    pub fn pre_rotate(self, deg: f32) -> Self {
        self.pre_concat(Transform::from_rotate(deg))
    }

    #[inline]
    pub fn pre_scale(self, sx: f32, sy: f32) -> Self {
        self.pre_concat(Transform::from_scale(sx, sy))
    }

    #[inline]
    pub fn pre_concat(self, other: Self) -> Self {
        // self * other (apply other first, then self)
        let mut r = other;
        r.post_concat(self);
        r
    }

    #[inline]
    pub fn pre_translate(self, tx: f32, ty: f32) -> Self {
        self.pre_concat(Transform::from_translate(tx, ty))
    }

    /// Applies the transform to a point in-place.
    #[inline]
    pub fn map_point(&self, pt: &mut Point) {
        let nx = self.sx * pt.x + self.kx * pt.y + self.tx;
        let ny = self.ky * pt.x + self.sy * pt.y + self.ty;
        pt.x = nx;
        pt.y = ny;
    }
}

/// 2D point matching tiny_skia::Point for zero-change call sites.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[inline]
    pub fn from_xy(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Blend mode for compositing operations.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendMode {
    SourceOver,
    Plus,
    Screen,
    Multiply,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl BlendMode {
    #[inline]
    pub fn to_d2d(self) -> windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND {
        use windows::Win32::Graphics::Direct2D::*;
        match self {
            BlendMode::SourceOver => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Plus => D2D1_PRIMITIVE_BLEND_ADD,
            BlendMode::Screen => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Multiply => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Overlay => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Darken => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Lighten => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::ColorDodge => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::ColorBurn => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::HardLight => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::SoftLight => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Difference => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            BlendMode::Exclusion => D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
        }
    }
}

/// Path verb types for PathBuilder.
#[derive(Copy, Clone, Debug, PartialEq)]
enum PathVerb {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Lightweight path type matching tiny_skia::Path API.
#[derive(Clone, Debug, Default)]
pub struct Path {
    verbs: Vec<PathVerb>,
    bounds: Option<(f32, f32, f32, f32)>,
    hash: u64,
}

impl Path {
    #[inline]
    pub fn new() -> Self { Self::default() }

    #[inline]
    pub fn is_empty(&self) -> bool { self.verbs.is_empty() }

    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        if let Some(b) = self.bounds { return Some(b); }
        if self.verbs.is_empty() { return None; }
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for v in &self.verbs {
            match v {
                PathVerb::MoveTo(x, y) | PathVerb::LineTo(x, y) => {
                    min_x = min_x.min(*x); min_y = min_y.min(*y);
                    max_x = max_x.max(*x); max_y = max_y.max(*y);
                }
                PathVerb::QuadTo(x1, y1, x, y) => {
                    min_x = min_x.min(*x1).min(*x); min_y = min_y.min(*y1).min(*y);
                    max_x = max_x.max(*x1).max(*x); max_y = max_y.max(*y1).max(*y);
                }
                PathVerb::CubicTo(x1, y1, x2, y2, x, y) => {
                    min_x = min_x.min(*x1).min(*x2).min(*x);
                    min_y = min_y.min(*y1).min(*y2).min(*y);
                    max_x = max_x.max(*x1).max(*x2).max(*x);
                    max_y = max_y.max(*y1).max(*y2).max(*y);
                }
                _ => {}
            }
        }
        if min_x.is_infinite() { return None; }
        Some((min_x, min_y, max_x - min_x, max_y - min_y))
    }
}

/// Builder for constructing Path objects.
pub struct PathBuilder {
    verbs: Vec<PathVerb>,
    started: bool,
}

impl PathBuilder {
    pub fn new() -> Self { Self { verbs: Vec::new(), started: false } }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::MoveTo(x, y));
        self.started = true;
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        if !self.started { self.move_to(x, y); }
        self.verbs.push(PathVerb::LineTo(x, y));
    }

    pub fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if !self.started { return; }
        self.verbs.push(PathVerb::QuadTo(x1, y1, x, y));
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if !self.started { return; }
        self.verbs.push(PathVerb::CubicTo(x1, y1, x2, y2, x, y));
    }

    pub fn close(&mut self) {
        if self.started {
            self.verbs.push(PathVerb::Close);
        }
    }

    pub fn finish(self) -> Option<Path> {
        if self.verbs.is_empty() { return None; }
        let mut h: u64 = 0xcbf29ce484222325;
        let mix = |h: &mut u64, v: u32| {
            *h ^= v as u64;
            *h = h.wrapping_mul(0x100000001b3);
        };
        for verb in &self.verbs {
            match *verb {
                PathVerb::MoveTo(x, y) => { mix(&mut h, 1); mix(&mut h, x.to_bits()); mix(&mut h, y.to_bits()); }
                PathVerb::LineTo(x, y) => { mix(&mut h, 2); mix(&mut h, x.to_bits()); mix(&mut h, y.to_bits()); }
                PathVerb::QuadTo(x1, y1, x, y) => {
                    mix(&mut h, 3); mix(&mut h, x1.to_bits()); mix(&mut h, y1.to_bits());
                    mix(&mut h, x.to_bits()); mix(&mut h, y.to_bits());
                }
                PathVerb::CubicTo(x1, y1, x2, y2, x, y) => {
                    mix(&mut h, 4); mix(&mut h, x1.to_bits()); mix(&mut h, y1.to_bits());
                    mix(&mut h, x2.to_bits()); mix(&mut h, y2.to_bits());
                    mix(&mut h, x.to_bits()); mix(&mut h, y.to_bits());
                }
                PathVerb::Close => { mix(&mut h, 5); }
            }
        }
        Some(Path { verbs: self.verbs, bounds: None, hash: h })
    }

    /// Creates a circle path using 4 cubic Bezier segments.
    /// `k = 0.55228475 * r` is the control point offset for a circular approximation.
    pub fn from_circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
        if r <= 0.0 { return None; }
        let k = 0.55228475 * r;
        let mut b = PathBuilder::new();
        b.move_to(cx, cy - r);
        b.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
        b.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
        b.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
        b.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
        b.close();
        b.finish()
    }
}

/// Converts a Path to an ID2D1PathGeometry using the provided factory.
pub fn path_to_d2d_geometry(
    factory: &windows::Win32::Graphics::Direct2D::ID2D1Factory1,
    path: &Path,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    use windows::Win32::Graphics::Direct2D::*;
    let geom: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()?.into() };
    let sink: ID2D1GeometrySink = unsafe { geom.Open()? };
    unsafe {
        let mut first = true;
        for v in &path.verbs {
            match v {
                PathVerb::MoveTo(x, y) => {
                    if !first {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(D2D_POINT_2F { x: *x, y: *y }, D2D1_FIGURE_BEGIN_FILLED);
                    first = false;
                }
                PathVerb::LineTo(x, y) => {
                    sink.AddLine(D2D_POINT_2F { x: *x, y: *y });
                }
                PathVerb::QuadTo(x1, y1, x, y) => {
                    sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: *x1, y: *y1 },
                        point2: D2D_POINT_2F { x: *x, y: *y },
                    });
                }
                PathVerb::CubicTo(x1, y1, x2, y2, x, y) => {
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: D2D_POINT_2F { x: *x1, y: *y1 },
                        point2: D2D_POINT_2F { x: *x2, y: *y2 },
                        point3: D2D_POINT_2F { x: *x, y: *y },
                    });
                }
                PathVerb::Close => {
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    first = true;
                }
            }
        }
        if !first {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
    }
    Ok(geom)
}

/// Creates a rounded rectangle path.
pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 { return None; }
    let max_r = (w.min(h) * 0.5).max(0.0);
    let r = r.clamp(0.0, max_r);
    let mut b = PathBuilder::new();
    if r <= 0.0 {
        b.move_to(x, y);
        b.line_to(x + w, y);
        b.line_to(x + w, y + h);
        b.line_to(x, y + h);
        b.line_to(x, y);
        b.close();
        return b.finish();
    }
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    if r > 0.0 {
        b.quad_to(x + w, y, x + w, y + r);
    } else {
        b.line_to(x + w, y + r);
    }
    b.line_to(x + w, y + h - r);
    if r > 0.0 {
        b.quad_to(x + w, y + h, x + w - r, y + h);
    } else {
        b.line_to(x + w - r, y + h);
    }
    b.line_to(x + r, y + h);
    if r > 0.0 {
        b.quad_to(x, y + h, x, y + h - r);
    } else {
        b.line_to(x, y + h - r);
    }
    b.line_to(x, y + r);
    if r > 0.0 {
        b.quad_to(x, y, x + r, y);
    } else {
        b.line_to(x + r, y);
    }
    b.close();
    b.finish()
}

/// Lightweight color type matching `tiny_skia::Color` API for zero-change call sites.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    #[inline]
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline]
    pub fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Option<Self> {
        if !(0.0..=1.0).contains(&r)
            || !(0.0..=1.0).contains(&g)
            || !(0.0..=1.0).contains(&b)
            || !(0.0..=1.0).contains(&a)
        {
            return None;
        }
        Some(Self {
            r: (r * 255.0 + 0.5) as u8,
            g: (g * 255.0 + 0.5) as u8,
            b: (b * 255.0 + 0.5) as u8,
            a: (a * 255.0 + 0.5) as u8,
        })
    }

    #[inline]
    pub const fn red(self) -> f32 { self.r as f32 / 255.0 }
    #[inline]
    pub const fn green(self) -> f32 { self.g as f32 / 255.0 }
    #[inline]
    pub const fn blue(self) -> f32 { self.b as f32 / 255.0 }
    #[inline]
    pub const fn alpha(self) -> f32 { self.a as f32 / 255.0 }

    #[inline]
    pub const fn to_d2d(self, global_alpha: f32) -> D2D1_COLOR_F {
        let ga = global_alpha.clamp(0.0, 1.0);
        D2D1_COLOR_F {
            r: self.r as f32 / 255.0,
            g: self.g as f32 / 255.0,
            b: self.b as f32 / 255.0,
            a: (self.a as f32 / 255.0) * ga,
        }
    }
}

/// Creates a color from sRGBA u8 components.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// Returns a copy of `c` with alpha replaced by `a`.
#[inline]
pub fn with_alpha(c: Color, a: u8) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, a)
}

/// Multiplies a color's alpha (used for disabled / fading content).
#[inline]
pub fn fade(c: Color, k: f32) -> Color {
    let a = (c.alpha() * k).clamp(0.0, 1.0);
    Color::from_rgba(c.red(), c.green(), c.blue(), a).unwrap_or(c)
}

/// Linear interpolation between two floats.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Linear interpolation between two colors.
#[inline]
pub fn lerp_color(c0: Color, c1: Color, t: f32) -> Color {
    let u = t.clamp(0.0, 1.0);
    Color::from_rgba(
        lerp(c0.red(), c1.red(), u),
        lerp(c0.green(), c1.green(), u),
        lerp(c0.blue(), c1.blue(), u),
        lerp(c0.alpha(), c1.alpha(), u),
    ).unwrap_or(c1)
}

/// Calibrated Design Tokens (Indigo-Obsidian Glass + Luminous Violet)
pub mod tok {
    use super::{rgba, Color};

    // Cool Indigo-Obsidian Glass Surfaces (Never pure #000000)
    pub fn obsidian_shell() -> Color { rgba(14, 16, 26, 250) }
    pub fn obsidian_deep() -> Color { rgba(9, 10, 18, 254) }
    pub fn surface_top() -> Color { rgba(28, 30, 46, 255) }
    pub fn surface_base() -> Color { rgba(16, 17, 28, 255) }
    pub fn surface_well() -> Color { rgba(10, 11, 19, 240) }

    // Frosted Indigo-Obsidian Palette (Used under GPU WinRT Composition HostBackdrop + GaussianBlur)
    pub fn obsidian_shell_frosted() -> Color { rgba(14, 16, 27, 108) }
    pub fn obsidian_deep_frosted() -> Color { rgba(9, 10, 19, 122) }
    pub fn surface_top_frosted() -> Color { rgba(28, 30, 47, 62) }
    pub fn surface_base_frosted() -> Color { rgba(16, 17, 29, 48) }
    pub fn surface_well_frosted() -> Color { rgba(10, 11, 20, 64) }

    pub fn scale_frosted(c: Color, opacity_pct: u32) -> Color {
        let pct = opacity_pct.clamp(25, 130);
        if pct == 68 { return c; }
        let a = ((c.alpha() * (pct as f32 / 68.0)).clamp(0.08, 0.96) * 255.0).round() as u8;
        Color::from_rgba8(
            (c.red() * 255.0).round() as u8,
            (c.green() * 255.0).round() as u8,
            (c.blue() * 255.0).round() as u8,
            a,
        )
    }

    pub fn text1() -> Color { rgba(247, 245, 255, 255) } // #F7F5FF crisp lavender-tinted white
    pub fn text2() -> Color { rgba(176, 172, 206, 255) } // #B0ACCE crisp cool slate-lavender (high WCAG AA contrast)
    pub fn text3() -> Color { rgba(148, 144, 182, 255) } // #9490B6 readable muted slate-lavender (WCAG AA >= 4.5:1)
    pub fn surface() -> Color { rgba(235, 230, 255, 15) }
    pub fn surface_hi() -> Color { rgba(235, 230, 255, 30) }
    pub fn hairline() -> Color { rgba(235, 230, 255, 26) }

    // Photo-Matched Luminous Violet & Pearlescent Lavender Palette
    pub fn cyno_indigo() -> Color { rgba(62, 24, 138, 255) }    // #3E188A deep royal indigo-violet
    pub fn cyno_deep() -> Color { rgba(93, 41, 196, 255) }       // #5D29C4 rich royal purple
    pub fn cyno_violet() -> Color { rgba(124, 58, 237, 255) }    // #7C3AED vivid iris violet
    pub fn cyno_orchid() -> Color { rgba(155, 89, 252, 255) }    // #9B59FC bright orchid bloom
    pub fn cyno_violet_hi() -> Color { rgba(196, 148, 255, 255) } // #C494FF luminous lavender-orchid accent
    pub fn cyno_mist() -> Color { rgba(233, 218, 255, 255) }     // #E9DAFF pearlescent lavender highlight
    pub fn cyno_lavender_white() -> Color { rgba(245, 240, 255, 255) } // #F5F0FF bright lavender-white accent
    pub fn cyno_amber_deep() -> Color { rgba(161, 98, 7, 255) }  // legacy/contrast token
    pub fn cyno_gold() -> Color { rgba(245, 158, 11, 255) }      // semantic warning/gold token
    pub fn cyno_gold_hi() -> Color { rgba(253, 230, 138, 255) }  // semantic warning highlight
    pub fn cyno_peach_gold() -> Color { rgba(233, 218, 255, 255) } // lavender-white pill end
    pub fn text_soft() -> Color { rgba(224, 217, 252, 255) }     // #E0D9FC crisp secondary lavender ink
    pub fn cyan_hi() -> Color { rgba(125, 211, 252, 255) }       // #7DD3FC crisp hydration sky accent
    pub fn cyno_badge_bg() -> Color { rgba(109, 40, 217, 78) }
    pub fn cyno_badge_ring() -> Color { rgba(168, 85, 247, 195) }
}

/// Converts sRGB u8 components + global alpha to D2D1_COLOR_F with straight alpha.
/// D2D uses straight alpha (0.0..=1.0) in D2D1_COLOR_F even on premultiplied targets.
fn create_stops(ctx: &ID2D1DeviceContext, c0: D2D1_COLOR_F, c1: D2D1_COLOR_F) -> Result<ID2D1GradientStopCollection> {
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: c0 },
        D2D1_GRADIENT_STOP { position: 1.0, color: c1 },
    ];
    let rt: ID2D1RenderTarget = ctx.cast()?;
    unsafe { rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) }
}
#[inline]
pub fn color_to_d2d(r: u8, g: u8, b: u8, a: u8, global_alpha: f32) -> D2D1_COLOR_F {
    let ga = global_alpha.clamp(0.0, 1.0);
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: (a as f32 / 255.0) * ga,
    }
}

/// Caches reusable D2D resources to avoid per-frame COM allocations.
/// The solid brush is reused via `SetColor` — created once, updated every draw call.
#[inline]
fn pack_rgba8(c: D2D1_COLOR_F) -> u32 {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u32;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u32;
    (r << 24) | (g << 16) | (b << 8) | a
}

#[inline]
fn q_half_px(v: f32) -> i32 {
    (v * 2.0).round() as i32
}

#[inline]
fn linear3_cache_key(
    c0: D2D1_COLOR_F,
    c_mid: D2D1_COLOR_F,
    c1: D2D1_COLOR_F,
    mid_pos: f32,
    start: D2D_POINT_2F,
    end: D2D_POINT_2F,
) -> (u32, u32, u32, i32, i32, i32, i32, i32) {
    (
        pack_rgba8(c0),
        pack_rgba8(c_mid),
        pack_rgba8(c1),
        (mid_pos.clamp(0.0, 1.0) * 1000.0).round() as i32,
        q_half_px(start.x),
        q_half_px(start.y),
        q_half_px(end.x),
        q_half_px(end.y),
    )
}

pub struct D2DResourceCache {
    solid_brushes: RefCell<HashMap<u32, ID2D1SolidColorBrush>>,
    linear_brushes: RefCell<HashMap<(u32, u32, i32, i32, i32, i32), ID2D1LinearGradientBrush>>,
    linear3_brushes: RefCell<HashMap<(u32, u32, u32, i32, i32, i32, i32, i32), ID2D1LinearGradientBrush>>,
    radial_brushes: RefCell<HashMap<(u32, u32, i32, i32, i32, i32), ID2D1RadialGradientBrush>>,
    path_geoms: RefCell<HashMap<u64, ID2D1PathGeometry>>,
    stroke_style_round: RefCell<Option<windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>>,
    d2d_factory: ID2D1Factory1,
}

impl D2DResourceCache {
    /// Creates a new cache from a device context (obtains factory via `ctx.GetFactory()`).
    pub fn new(ctx: &ID2D1DeviceContext) -> Result<Self> {
        let factory: ID2D1Factory1 = unsafe { ctx.GetFactory()? }.cast()?;
        Ok(Self {
            solid_brushes: RefCell::new(HashMap::with_capacity(128)),
            linear_brushes: RefCell::new(HashMap::with_capacity(128)),
            linear3_brushes: RefCell::new(HashMap::with_capacity(64)),
            radial_brushes: RefCell::new(HashMap::with_capacity(128)),
            path_geoms: RefCell::new(HashMap::with_capacity(128)),
            stroke_style_round: RefCell::new(None),
            d2d_factory: factory,
        })
    }

    /// Returns the cached D2D factory.
    #[inline]
    pub fn factory(&self) -> &ID2D1Factory1 {
        &self.d2d_factory
    }

    /// Returns a cached round-cap/round-join stroke style (created once per cache).
    pub fn stroke_style_round(&self) -> Option<windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle> {
        let mut cell = self.stroke_style_round.borrow_mut();
        if let Some(s) = cell.as_ref() {
            return Some(s.clone());
        }
        let props = windows::Win32::Graphics::Direct2D::D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: windows::Win32::Graphics::Direct2D::D2D1_CAP_STYLE_ROUND,
            endCap: windows::Win32::Graphics::Direct2D::D2D1_CAP_STYLE_ROUND,
            dashCap: windows::Win32::Graphics::Direct2D::D2D1_CAP_STYLE_ROUND,
            lineJoin: windows::Win32::Graphics::Direct2D::D2D1_LINE_JOIN_ROUND,
            miterLimit: 10.0,
            dashOffset: 0.0,
            dashStyle: windows::Win32::Graphics::Direct2D::D2D1_DASH_STYLE_SOLID,
            transformType: windows::Win32::Graphics::Direct2D::D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
        };
        let style: windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle =
            unsafe { self.d2d_factory.CreateStrokeStyle(&props, None).ok()?.into() };
        *cell = Some(style.clone());
        Some(style)
    }

    /// Returns a cached ID2D1PathGeometry for a Path (eliminates per-frame mascot/icon tessellation).
    pub fn path_geom(&self, path: &Path) -> Result<ID2D1PathGeometry> {
        let mut map = self.path_geoms.borrow_mut();
        if let Some(g) = map.get(&path.hash) {
            return Ok(g.clone());
        }
        if map.len() >= 512 {
            map.clear();
        }
        let geom = path_to_d2d_geometry(&self.d2d_factory, path)?;
        map.insert(path.hash, geom.clone());
        Ok(geom)
    }

    /// Gets or creates an immutable solid color brush keyed by RGBA8.
    pub fn solid(&self, ctx: &ID2D1DeviceContext, col: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        let key = pack_rgba8(col);
        let mut map = self.solid_brushes.borrow_mut();
        if let Some(b) = map.get(&key) {
            return Ok(b.clone());
        }
        if map.len() >= 512 {
            map.clear();
        }
        let b: ID2D1SolidColorBrush = unsafe { ctx.CreateSolidColorBrush(&col, None)? };
        map.insert(key, b.clone());
        Ok(b)
    }

    /// Gets or creates a cached 2-stop linear gradient brush keyed by RGBA8 + 0.5px quantized endpoints.
    pub fn linear_brush(
        &self,
        ctx: &ID2D1DeviceContext,
        c0: D2D1_COLOR_F,
        c1: D2D1_COLOR_F,
        start: D2D_POINT_2F,
        end: D2D_POINT_2F,
    ) -> Result<ID2D1LinearGradientBrush> {
        let key = (
            pack_rgba8(c0),
            pack_rgba8(c1),
            q_half_px(start.x),
            q_half_px(start.y),
            q_half_px(end.x),
            q_half_px(end.y),
        );
        let mut map = self.linear_brushes.borrow_mut();
        if let Some(b) = map.get(&key) {
            return Ok(b.clone());
        }
        if map.len() >= 512 {
            map.clear();
        }
        let collection = create_stops(ctx, c0, c1)?;
        let props = windows::Win32::Graphics::Direct2D::D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: start,
            endPoint: end,
        };
        let b: ID2D1LinearGradientBrush =
            unsafe { ctx.CreateLinearGradientBrush(&props, None, &collection)? };
        map.insert(key, b.clone());
        Ok(b)
    }

    /// Gets or creates a cached 3-stop linear gradient brush (0.0, mid_pos, 1.0) for bevel strokes.
    pub fn linear_brush_3stop(
        &self,
        ctx: &ID2D1DeviceContext,
        c0: D2D1_COLOR_F,
        c_mid: D2D1_COLOR_F,
        c1: D2D1_COLOR_F,
        mid_pos: f32,
        start: D2D_POINT_2F,
        end: D2D_POINT_2F,
    ) -> Result<ID2D1LinearGradientBrush> {
        let key = linear3_cache_key(c0, c_mid, c1, mid_pos, start, end);
        let mut map = self.linear3_brushes.borrow_mut();
        if let Some(b) = map.get(&key) {
            return Ok(b.clone());
        }
        if map.len() >= 512 {
            map.clear();
        }
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c0 },
            D2D1_GRADIENT_STOP { position: mid_pos, color: c_mid },
            D2D1_GRADIENT_STOP { position: 1.0, color: c1 },
        ];
        let rt: ID2D1RenderTarget = ctx.cast()?;
        let collection = unsafe { rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)? };
        let props = windows::Win32::Graphics::Direct2D::D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: start,
            endPoint: end,
        };
        let b: ID2D1LinearGradientBrush =
            unsafe { ctx.CreateLinearGradientBrush(&props, None, &collection)? };
        map.insert(key, b.clone());
        Ok(b)
    }

    /// Gets or creates a cached radial gradient brush with v0.2.4.1's 4-stop smooth ease-out glow curve.
    pub fn radial_brush(
        &self,
        ctx: &ID2D1DeviceContext,
        c_center: D2D1_COLOR_F,
        c_edge: D2D1_COLOR_F,
        center: D2D_POINT_2F,
        rx: f32,
        ry: f32,
    ) -> Result<ID2D1RadialGradientBrush> {
        let key = (
            pack_rgba8(c_center),
            pack_rgba8(c_edge),
            q_half_px(center.x),
            q_half_px(center.y),
            q_half_px(rx),
            q_half_px(ry),
        );
        let mut map = self.radial_brushes.borrow_mut();
        if let Some(b) = map.get(&key) {
            return Ok(b.clone());
        }
        if map.len() >= 512 {
            map.clear();
        }
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c_center },
            D2D1_GRADIENT_STOP {
                position: 0.45,
                color: D2D1_COLOR_F { r: c_center.r, g: c_center.g, b: c_center.b, a: c_center.a * 0.42 },
            },
            D2D1_GRADIENT_STOP {
                position: 0.75,
                color: D2D1_COLOR_F { r: c_center.r, g: c_center.g, b: c_center.b, a: c_center.a * 0.12 },
            },
            D2D1_GRADIENT_STOP { position: 1.0, color: c_edge },
        ];
        let rt: ID2D1RenderTarget = ctx.cast()?;
        let collection = unsafe { rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)? };
        let props = windows::Win32::Graphics::Direct2D::D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center,
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: rx,
            radiusY: ry,
        };
        let b: ID2D1RadialGradientBrush =
            unsafe { ctx.CreateRadialGradientBrush(&props, None, &collection)? };
        map.insert(key, b.clone());
        Ok(b)
    }

    /// Clears cached resources (called on device lost).
    pub fn clear(&self) {
        self.solid_brushes.borrow_mut().clear();
        self.linear_brushes.borrow_mut().clear();
        self.linear3_brushes.borrow_mut().clear();
        self.radial_brushes.borrow_mut().clear();
        self.path_geoms.borrow_mut().clear();
        *self.stroke_style_round.borrow_mut() = None;
    }
}

/// Arc endpoints and sweep calculation helper.
/// Returns (start_point, end_point, arc_size) for ID2D1PathGeometry::AddArc.
fn arc_endpoints(
    cx: f32,
    cy: f32,
    r: f32,
    a0: f32,
    sweep: f32,
) -> (D2D_POINT_2F, D2D_POINT_2F, D2D1_ARC_SIZE) {
    let start_x = cx + r * a0.cos();
    let start_y = cy + r * a0.sin();
    let end_a = a0 + sweep;
    let end_x = cx + r * end_a.cos();
    let end_y = cy + r * end_a.sin();
    let arc_size = if sweep.abs() > PI {
        D2D1_ARC_SIZE_LARGE
    } else {
        D2D1_ARC_SIZE_SMALL
    };
    (
        D2D_POINT_2F {
            x: start_x,
            y: start_y,
        },
        D2D_POINT_2F { x: end_x, y: end_y },
        arc_size,
    )
}

/// Direct2D painter wrapping `ID2D1DeviceContext` with resource cache.
/// Mirrors the `Painter` API from `painter.rs` for easy migration.
pub struct D2DPainter<'a> {
    ctx: &'a ID2D1DeviceContext,
    cache: &'a mut D2DResourceCache,
    s: f32,
    origin_x: f32,
    origin_y: f32,
    global_alpha: f32,
    // Hit testing state
    mx: f32,
    my: f32,
    mdown: bool,
    mclicked: bool,
    // Clip layer tracking
    clip_stack: Vec<(f32, f32, f32, f32, f32)>,
}

impl<'a> D2DPainter<'a> {
    /// Creates a new painter from a device context and resource cache.
    pub fn new(
        ctx: &'a ID2D1DeviceContext,
        cache: &'a mut D2DResourceCache,
        s: f32,
        origin_x: f32,
        origin_y: f32,
        global_alpha: f32,
    ) -> Self {
        Self {
            ctx,
            cache,
            s,
            origin_x,
            origin_y,
            global_alpha,
            mx: 0.0,
            my: 0.0,
            mdown: false,
            mclicked: false,
            clip_stack: Vec::new(),
        }
    }

    /// Applies the transform (scale + origin) to a logical point.
    #[inline]
    fn tf_pt(&self, x: f32, y: f32) -> (f32, f32) {
        (self.origin_x + x * self.s, self.origin_y + y * self.s)
    }

    /// Clears the render target to transparent black.
    pub fn clear(&self) -> Result<()> {
        unsafe {
            self.ctx.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
        }
        Ok(())
    }

    /// Fills a rounded rectangle.
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, mut col: D2D1_COLOR_F) -> Result<()> {
        col.a *= self.global_alpha;
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let dr = r * self.s;
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dw,
                bottom: dy + dh,
            },
            radiusX: dr.clamp(0.0, max_r),
            radiusY: dr.clamp(0.0, max_r),
        };
        let brush = self.cache.solid(self.ctx, col)?;
        unsafe { self.ctx.FillRoundedRectangle(&rr, &brush) };
        Ok(())
    }

    /// Strokes a rounded rectangle.
    pub fn rrect_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        width: f32,
        mut col: D2D1_COLOR_F,
    ) -> Result<()> {
        col.a *= self.global_alpha;
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let dr = r * self.s;
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dw,
                bottom: dy + dh,
            },
            radiusX: dr.clamp(0.0, max_r),
            radiusY: dr.clamp(0.0, max_r),
        };
        let brush = self.cache.solid(self.ctx, col)?;
        unsafe { self.ctx.DrawRoundedRectangle(&rr, &brush, width * self.s, None) };
        Ok(())
    }

    /// Strokes a rounded rectangle with per-corner radii.
    pub fn rrect_corners_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_tl: f32,
        r_tr: f32,
        r_br: f32,
        r_bl: f32,
        width: f32,
        mut col: D2D1_COLOR_F,
    ) -> Result<()> {
        col.a *= self.global_alpha;
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rt = (r_tl * self.s).clamp(0.0, max_r);
        let rtr = (r_tr * self.s).clamp(0.0, max_r);
        let rbr = (r_br * self.s).clamp(0.0, max_r);
        let rbl = (r_bl * self.s).clamp(0.0, max_r);

        let path: ID2D1PathGeometry = unsafe { self.cache.factory().CreatePathGeometry()?.into() };
        let sink: ID2D1GeometrySink = unsafe { path.Open()? };
        unsafe {
            sink.BeginFigure(
                D2D_POINT_2F { x: dx + rt, y: dy },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(D2D_POINT_2F { x: dx + dw - rtr, y: dy });
            if rtr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rtr, dy + rtr, rtr, -PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rtr, height: rtr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy + dh - rbr });
            if rbr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rbr, dy + dh - rbr, rbr, 0.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbr, height: rbr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + rbl, y: dy + dh });
            if rbl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rbl, dy + dh - rbl, rbl, PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbl, height: rbl },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx, y: dy + rt });
            if rt > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rt, dy + rt, rt, PI, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rt, height: rt },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
        }

        let brush = self.cache.solid(self.ctx, col)?;
        unsafe { self.ctx.DrawGeometry(&path, &brush, width * self.s, None) };
        Ok(())
    }

    /// Fills a rounded rectangle with a vertical linear gradient (top to bottom).
    pub fn rrect_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        mut c0: D2D1_COLOR_F,
        mut c1: D2D1_COLOR_F,
    ) -> Result<()> {
        c0.a *= self.global_alpha;
        c1.a *= self.global_alpha;
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let dr = r * self.s;
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dw,
                bottom: dy + dh,
            },
            radiusX: dr.clamp(0.0, max_r),
            radiusY: dr.clamp(0.0, max_r),
        };
        let brush = self.cache.linear_brush(
            self.ctx,
            c0,
            c1,
            D2D_POINT_2F { x: dx, y: dy },
            D2D_POINT_2F { x: dx, y: dy + dh },
        )?;
        unsafe { self.ctx.FillRoundedRectangle(&rr, &brush) };
        Ok(())
    }

    /// Fills a radial gradient glow (circle).
    pub fn radial_glow(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        mut c_center: D2D1_COLOR_F,
        mut c_edge: D2D1_COLOR_F,
    ) -> Result<()> {
        c_center.a *= self.global_alpha;
        c_edge.a *= self.global_alpha;
        let (dcx, dcy) = self.tf_pt(cx, cy);
        let dr = r * self.s;
        let center = D2D_POINT_2F { x: dcx, y: dcy };
        let brush = self.cache.radial_brush(self.ctx, c_center, c_edge, center, dr, dr)?;
        let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: center,
            radiusX: dr,
            radiusY: dr,
        };
        unsafe { self.ctx.FillEllipse(&ellipse, &brush) };
        Ok(())
    }

    /// Fills an elliptical radial gradient glow.
    pub fn elliptical_glow(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        mut c_center: D2D1_COLOR_F,
        mut c_edge: D2D1_COLOR_F,
    ) -> Result<()> {
        c_center.a *= self.global_alpha;
        c_edge.a *= self.global_alpha;
        let (dcx, dcy) = self.tf_pt(cx, cy);
        let (drx, dry) = (rx * self.s, ry * self.s);
        let center = D2D_POINT_2F { x: dcx, y: dcy };
        let brush = self.cache.radial_brush(self.ctx, c_center, c_edge, center, drx, dry)?;
        let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: center,
            radiusX: drx,
            radiusY: dry,
        };
        unsafe { self.ctx.FillEllipse(&ellipse, &brush) };
        Ok(())
    }

    /// Fills a circle.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, mut col: D2D1_COLOR_F) -> Result<()> {
        col.a *= self.global_alpha;
        let (dcx, dcy) = self.tf_pt(cx, cy);
        let dr = r * self.s;
        let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: dcx, y: dcy },
            radiusX: dr,
            radiusY: dr,
        };
        let brush = self.cache.solid(self.ctx, col)?;
        unsafe { self.ctx.FillEllipse(&ellipse, &brush) };
        Ok(())
    }

    /// Strokes a circle.
    pub fn circle_stroke(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        width: f32,
        mut col: D2D1_COLOR_F,
    ) -> Result<()> {
        col.a *= self.global_alpha;
        let (dcx, dcy) = self.tf_pt(cx, cy);
        let dr = r * self.s;
        let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: dcx, y: dcy },
            radiusX: dr,
            radiusY: dr,
        };
        let brush = self.cache.solid(self.ctx, col)?;
        unsafe { self.ctx.DrawEllipse(&ellipse, &brush, width * self.s, None) };
        Ok(())
    }

    /// Strokes an arc using ID2D1PathGeometry (or `DrawEllipse` for full 360-degree sweeps
    /// where `start_pt == end_pt` would make Direct2D `AddArc` degenerate and invisible — BH-20).
    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        a0: f32,
        sweep: f32,
        width: f32,
        mut col: D2D1_COLOR_F,
    ) -> Result<()> {
        col.a *= self.global_alpha;
        if sweep.abs() < 1e-4 {
            return Ok(());
        }
        let (dcx, dcy) = self.tf_pt(cx, cy);
        let dr = r * self.s;
        let brush = self.cache.solid(self.ctx, col)?;
        let style = self.cache.stroke_style_round();

        if sweep.abs() >= std::f32::consts::TAU - 1e-3 {
            let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: dcx, y: dcy },
                radiusX: dr,
                radiusY: dr,
            };
            unsafe {
                self.ctx
                    .DrawEllipse(&ellipse, &brush, width * self.s, style.as_ref());
            }
            return Ok(());
        }

        let (start_pt, end_pt, arc_size) = arc_endpoints(dcx, dcy, dr, a0, sweep);

        let geom: ID2D1PathGeometry = unsafe { self.cache.factory().CreatePathGeometry()?.into() };
        let sink: ID2D1GeometrySink = unsafe { geom.Open()? };
        unsafe {
            sink.BeginFigure(
                start_pt,
                windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_BEGIN_HOLLOW,
            );
            sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                point: end_pt,
                size: D2D_SIZE_F {
                    width: dr,
                    height: dr,
                },
                rotationAngle: 0.0,
                sweepDirection: if sweep >= 0.0 {
                    windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE
                } else {
                    windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                },
                arcSize: arc_size,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
        }

        unsafe { self.ctx.DrawGeometry(&geom, &brush, width * self.s, style.as_ref()) };
        Ok(())
    }

    /// Draws text using the provided font cache.
    pub fn text(
        &mut self,
        fonts: &mut crate::font_d2d::DWriteTextCache,
        text: &str,
        x: f32,
        y_top: f32,
        px: f32,
        weight: Weight,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> Result<()> {
        let (dx, dy) = self.tf_pt(x, y_top);
        let col = color_to_d2d(r, g, b, a, self.global_alpha);
        let brush = self.cache.solid(self.ctx, col)?;
        let brush_as_brush: ID2D1Brush = brush.cast()?;
        fonts.draw_text(self.ctx, &brush_as_brush, text, dx, dy, px * self.s, weight, None)
    }

    /// Pushes a rounded-rectangle clip layer.
    pub fn push_rrect_clip(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> Result<()> {
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let dr = r * self.s;
        let max_r = (dw.min(dh) * 0.5).max(0.0);

        let geom: ID2D1RoundedRectangleGeometry = unsafe {
            self.cache.factory().CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: dx,
                    top: dy,
                    right: dx + dw,
                    bottom: dy + dh,
                },
                radiusX: dr.clamp(0.0, max_r),
                radiusY: dr.clamp(0.0, max_r),
            })?
        };

        // BH-05: Bind layer_params and drop ManuallyDrop<Option<ID2D1Geometry>> after PushLayer
        // so the COM refcount of ID2D1RoundedRectangleGeometry is never leaked.
        let layer_params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: dx,
                top: dy,
                right: dx + dw,
                bottom: dy + dh,
            },
            geometricMask: ManuallyDrop::new(Some(geom.into())),
            maskAntialiasMode:
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        unsafe {
            self.ctx.PushLayer(&layer_params, None);
        }
        let _ = ManuallyDrop::into_inner(layer_params.geometricMask);
        Ok(())
    }

    /// Pops the current clip layer.
    pub fn pop_clip(&mut self) -> Result<()> {
        unsafe { self.ctx.PopLayer() };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;
    use windows::Win32::Graphics::Direct2D::D2D1CreateFactory;
    use windows::Win32::Graphics::Direct2D::D2D1_FACTORY_TYPE_SINGLE_THREADED;

    #[test]
    fn test_color_to_d2d_basic_conversion() {
        // Pure opaque white -> r=g=b=1.0, a=1.0
        let c = color_to_d2d(255, 255, 255, 255, 1.0);
        assert!((c.r - 1.0).abs() < 1e-5);
        assert!((c.g - 1.0).abs() < 1e-5);
        assert!((c.b - 1.0).abs() < 1e-5);
        assert!((c.a - 1.0).abs() < 1e-5);

        // Pure transparent black -> all 0.0
        let c = color_to_d2d(0, 0, 0, 0, 1.0);
        assert!((c.r - 0.0).abs() < 1e-5);
        assert!((c.g - 0.0).abs() < 1e-5);
        assert!((c.b - 0.0).abs() < 1e-5);
        assert!((c.a - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_color_to_d2d_global_alpha_scaling() {
        // 50% global alpha scales alpha only (straight alpha)
        let c = color_to_d2d(255, 128, 64, 255, 0.5);
        assert!((c.r - 1.0).abs() < 1e-5);
        assert!((c.g - 128.0 / 255.0).abs() < 1e-5);
        assert!((c.b - 64.0 / 255.0).abs() < 1e-5);
        assert!((c.a - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_color_to_d2d_straight_alpha() {
        // D2D uses straight alpha: color components are NOT premultiplied by alpha
        let c = color_to_d2d(255, 0, 0, 128, 1.0); // 50% alpha red
        assert!((c.r - 1.0).abs() < 1e-5);
        assert!((c.a - 128.0 / 255.0).abs() < 1e-5);
    }

    #[test]
    fn test_color_to_d2d_clamping() {
        // global_alpha clamped to [0, 1]
        let c = color_to_d2d(255, 255, 255, 255, 1.5);
        assert!((c.r - 1.0).abs() < 1e-5);
        assert!((c.a - 1.0).abs() < 1e-5);

        let c = color_to_d2d(255, 255, 255, 255, -0.5);
        assert!((c.r - 1.0).abs() < 1e-5);
        assert!((c.a - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_arc_endpoints_small_sweep() {
        let (start, end, arc_size) = arc_endpoints(100.0, 100.0, 50.0, 0.0, PI / 2.0);
        // +90 degree clockwise arc in screen coords (+y down): 3 o'clock -> 6 o'clock
        assert!((start.x - 150.0).abs() < 1e-5);
        assert!((start.y - 100.0).abs() < 1e-5);
        assert!((end.x - 100.0).abs() < 1e-5);
        assert!((end.y - 150.0).abs() < 1e-5);
        assert_eq!(arc_size, D2D1_ARC_SIZE_SMALL);
    }

    #[test]
    fn test_arc_endpoints_large_sweep() {
        let (start, end, arc_size) = arc_endpoints(100.0, 100.0, 50.0, 0.0, 3.0 * PI / 2.0);
        // 270 degree clockwise arc -> large arc size (3 o'clock -> 12 o'clock)
        assert!((start.x - 150.0).abs() < 1e-5);
        assert!((start.y - 100.0).abs() < 1e-5);
        assert!((end.x - 100.0).abs() < 1e-5);
        assert!((end.y - 50.0).abs() < 1e-5);
        assert_eq!(arc_size, D2D1_ARC_SIZE_LARGE);
    }

    #[test]
    fn test_arc_endpoints_negative_sweep() {
        let (start, end, arc_size) = arc_endpoints(100.0, 100.0, 50.0, 0.0, -PI / 2.0);
        // -90 degree counter-clockwise arc in screen coords: 3 o'clock -> 12 o'clock
        assert!((start.x - 150.0).abs() < 1e-5);
        assert!((start.y - 100.0).abs() < 1e-5);
        assert!((end.x - 100.0).abs() < 1e-5);
        assert!((end.y - 50.0).abs() < 1e-5);
        assert_eq!(arc_size, D2D1_ARC_SIZE_SMALL);
    }

    #[test]
    fn test_arc_endpoints_exactly_pi() {
        // Exactly PI -> small (boundary case)
        let (_, _, arc_size) = arc_endpoints(0.0, 0.0, 1.0, 0.0, PI);
        assert_eq!(arc_size, D2D1_ARC_SIZE_SMALL);

        // Just over PI -> large
        let (_, _, arc_size) = arc_endpoints(0.0, 0.0, 1.0, 0.0, PI + 0.001);
        assert_eq!(arc_size, D2D1_ARC_SIZE_LARGE);
    }

    #[test]
    fn test_d2d_factory_creation() {
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.unwrap();
        // Factory should be valid
        let _ = factory;
    }

    #[test]
    fn test_rounded_rect_geometry_smoke() {
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.unwrap();
        let geom: ID2D1RoundedRectangleGeometry = unsafe {
            factory
                .CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: 100.0,
                        bottom: 50.0,
                    },
                    radiusX: 10.0,
                    radiusY: 10.0,
                })
                .unwrap()
        };
        let _ = geom;
    }

    #[test]
    fn test_transform_identity() {
        let t = Transform::identity();
        assert_eq!(t.sx, 1.0);
        assert_eq!(t.ky, 0.0);
        assert_eq!(t.kx, 0.0);
        assert_eq!(t.sy, 1.0);
        assert_eq!(t.tx, 0.0);
        assert_eq!(t.ty, 0.0);
    }

    #[test]
    fn test_transform_from_translate() {
        let t = Transform::from_translate(10.0, 20.0);
        assert_eq!(t.sx, 1.0);
        assert_eq!(t.sy, 1.0);
        assert_eq!(t.tx, 10.0);
        assert_eq!(t.ty, 20.0);
    }

    #[test]
    fn test_transform_from_scale() {
        let t = Transform::from_scale(2.0, 3.0);
        assert_eq!(t.sx, 2.0);
        assert_eq!(t.sy, 3.0);
        assert_eq!(t.tx, 0.0);
        assert_eq!(t.ty, 0.0);
    }

    #[test]
    fn test_transform_from_rotate() {
        let t = Transform::from_rotate(90.0);
        assert!((t.sx - 0.0).abs() < 1e-5); // cos(90°) ≈ 0
        assert!((t.ky - 1.0).abs() < 1e-5); // sin(90°) ≈ 1
        assert!((t.kx - (-1.0)).abs() < 1e-5); // -sin(90°) ≈ -1
        assert!((t.sy - 0.0).abs() < 1e-5); // cos(90°) ≈ 0
        assert_eq!(t.tx, 0.0);
        assert_eq!(t.ty, 0.0);

        let t = Transform::from_rotate(180.0);
        assert!((t.sx - (-1.0)).abs() < 1e-5);
        assert!((t.ky - 0.0).abs() < 1e-5);
        assert!((t.kx - 0.0).abs() < 1e-5);
        assert!((t.sy - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_transform_post_translate() {
        let mut t = Transform::from_scale(2.0, 2.0);
        t.post_translate(5.0, 10.0);
        assert_eq!(t.tx, 5.0);
        assert_eq!(t.ty, 10.0);

        // pre_translate applies translation before rotation
        let t = Transform::from_rotate(90.0).pre_translate(10.0, 0.0);
        assert!((t.tx - 0.0).abs() < 1e-5); // 10 * cos(90) + 0 * -sin(90) = 0
        assert!((t.ty - 10.0).abs() < 1e-5); // 10 * sin(90) + 0 * cos(90) = 10
    }

    #[test]
    fn test_transform_post_scale() {
        let mut t = Transform::from_translate(10.0, 20.0);
        t.post_scale(2.0, 3.0);
        assert_eq!(t.sx, 2.0);
        assert_eq!(t.sy, 3.0);
        assert_eq!(t.tx, 20.0); // 10 * 2
        assert_eq!(t.ty, 60.0); // 20 * 3
    }

    #[test]
    fn test_transform_post_concat() {
        let mut t = Transform::from_translate(10.0, 20.0);
        let other = Transform::from_scale(2.0, 3.0);
        t.post_concat(other);
        // Result: translate then scale
        assert_eq!(t.sx, 2.0);
        assert_eq!(t.sy, 3.0);
        assert_eq!(t.tx, 20.0); // 10 * 2
        assert_eq!(t.ty, 60.0); // 20 * 3
    }

    #[test]
    fn test_transform_to_d2d_matrix3x2() {
        let t = Transform::from_row(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        let m = t.to_d2d_matrix3x2();
        assert_eq!(m.M11, 1.0);
        assert_eq!(m.M12, 2.0);
        assert_eq!(m.M21, 3.0);
        assert_eq!(m.M22, 4.0);
        assert_eq!(m.M31, 5.0);
        assert_eq!(m.M32, 6.0);
    }

    #[test]
    fn test_path_builder_basic() {
        let mut b = PathBuilder::new();
        b.move_to(0.0, 0.0);
        b.line_to(10.0, 0.0);
        b.line_to(10.0, 10.0);
        b.line_to(0.0, 10.0);
        b.close();
        let path = b.finish().unwrap();
        assert_eq!(path.verbs.len(), 5); // move, line, line, line, close
        assert!(!path.is_empty());
    }

    #[test]
    fn test_path_builder_quad_cubic() {
        let mut b = PathBuilder::new();
        b.move_to(0.0, 0.0);
        b.quad_to(5.0, 5.0, 10.0, 0.0);
        b.cubic_to(15.0, 0.0, 15.0, 10.0, 10.0, 10.0);
        b.close();
        let path = b.finish().unwrap();
        assert_eq!(path.verbs.len(), 4); // move, quad, cubic, close
        assert!(!path.is_empty());
    }

    #[test]
    fn test_path_builder_empty() {
        let b = PathBuilder::new();
        let path = b.finish();
        assert!(path.is_none());
    }

    #[test]
    fn test_path_bounds() {
        let mut b = PathBuilder::new();
        b.move_to(10.0, 20.0);
        b.line_to(50.0, 60.0);
        b.close();
        let path = b.finish().unwrap();
        let bounds = path.bounds().unwrap();
        assert!((bounds.0 - 10.0).abs() < 1e-5);
        assert!((bounds.1 - 20.0).abs() < 1e-5);
        assert!((bounds.2 - 40.0).abs() < 1e-5);
        assert!((bounds.3 - 40.0).abs() < 1e-5);
    }

    #[test]
    fn test_rrect_path_basic() {
        let path = rrect_path(0.0, 0.0, 100.0, 50.0, 10.0).unwrap();
        assert!(!path.is_empty());
        // Should have: move, 4 lines, 4 quads, close = 10 verbs
        assert_eq!(path.verbs.len(), 10);
        let bounds = path.bounds().unwrap();
        assert!((bounds.0 - 0.0).abs() < 1e-5);
        assert!((bounds.1 - 0.0).abs() < 1e-5);
        assert!((bounds.2 - 100.0).abs() < 1e-5);
        assert!((bounds.3 - 50.0).abs() < 1e-5);
    }

    #[test]
    fn test_rrect_path_zero_radius() {
        let path = rrect_path(0.0, 0.0, 100.0, 50.0, 0.0).unwrap();
        assert!(!path.is_empty());
        // Should have: move, 4 lines, close = 6 verbs (no quads)
        assert_eq!(path.verbs.len(), 6);
    }

    #[test]
    fn test_rrect_path_clamped_radius() {
        // Radius larger than half the smaller dimension should be clamped
        let path = rrect_path(0.0, 0.0, 100.0, 50.0, 100.0).unwrap();
        assert!(!path.is_empty());
        // Max radius is 25 (half of 50)
        assert_eq!(path.verbs.len(), 10);
    }

    #[test]
    fn test_rrect_path_invalid_size() {
        assert!(rrect_path(0.0, 0.0, 0.0, 50.0, 10.0).is_none());
        assert!(rrect_path(0.0, 0.0, 100.0, 0.0, 10.0).is_none());
        assert!(rrect_path(0.0, 0.0, -10.0, 50.0, 10.0).is_none());
    }
}

// ========================================================================
// D2DPainter - Full Painter API Parity (ported from painter.rs)
// ========================================================================

impl<'a> D2DPainter<'a> {
    /// Returns the "well" surface color token.
    #[inline]
    pub fn well_col(&self) -> Color {
        tok::surface_well()
    }

    /// Returns the "surface top" color token.
    #[inline]
    pub fn surface_top_col(&self) -> Color {
        tok::surface_top()
    }

    /// Returns the effective text size for a given logical size.
    #[inline]
    pub fn eff_size(&self, base: f32) -> f32 {
        base * self.s
    }

    // ──────────────────────────────────────────────────────────────
    // Text Measurement & Layout
    // ──────────────────────────────────────────────────────────────

    /// Measures text width using DWrite.
    pub fn measure(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, size_px: f32, weight: Weight) -> f32 {
        fonts.measure(text, size_px * self.s, weight).0 / self.s
    }

    /// Ellipsizes text to fit within max_width, returning (text, was_ellipsized).
    pub fn ellipsize_cow<'b>(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &'b str, max_width: f32, size_px: f32, weight: Weight) -> (std::borrow::Cow<'b, str>, bool) {
        let scaled_max = max_width * self.s;
        let scaled_size = size_px * self.s;
        let measured = fonts.measure(text, scaled_size, weight).0;
        if measured <= scaled_max {
            return (std::borrow::Cow::Borrowed(text), false);
        }
        let ellipsis = "…";
        let ellipsis_w = fonts.measure(ellipsis, scaled_size, weight).0;
        let target = scaled_max - ellipsis_w;
        if target <= 0.0 {
            return (std::borrow::Cow::Owned(ellipsis.to_string()), true);
        }
        let mut lo = 0;
        let mut hi = text.chars().count();
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let substr: String = text.chars().take(mid).collect();
            if fonts.measure(&substr, scaled_size, weight).0 <= target {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if lo == 0 {
            (std::borrow::Cow::Owned(ellipsis.to_string()), true)
        } else {
            let substr: String = text.chars().take(lo).collect();
            (std::borrow::Cow::Owned(substr + ellipsis), true)
        }
    }

    /// Ellipsizes text in place.
    pub fn ellipsize(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &mut String, max_width: f32, size_px: f32, weight: Weight) -> bool {
        let (cow, changed) = self.ellipsize_cow(fonts, text, max_width, size_px, weight);
        if changed {
            *text = cow.into_owned();
        }
        changed
    }

    /// Wraps text into lines that fit within max_width.
    pub fn wrap_lines(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, max_width: f32, size_px: f32, weight: Weight) -> Vec<String> {
        let scaled_max = max_width * self.s;
        let scaled_size = size_px * self.s;
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return vec![String::new()];
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in words {
            let test = if current.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current, word)
            };
            if fonts.measure(&test, scaled_size, weight).0 <= scaled_max {
                current = test;
            } else {
                if !current.is_empty() {
                    lines.push(current);
                }
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    /// Finds the largest font size that fits text in a single line within max_width.
    pub fn fit_text_size(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, max_width: f32, max_size: f32, min_size: f32, weight: Weight) -> f32 {
        let mut lo = min_size;
        let mut hi = max_size;
        for _ in 0..8 {
            let mid = (lo + hi) / 2.0;
            if fonts.measure(text, mid * self.s, weight).0 <= max_width * self.s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Wraps text and finds the largest font size that fits within max_width and max_lines.
    pub fn wrap_lines_fit(&self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, max_width: f32, max_lines: usize, max_size: f32, min_size: f32, weight: Weight) -> (Vec<String>, f32) {
        let mut lo = min_size;
        let mut hi = max_size;
        let mut best_lines = vec![text.to_string()];
        let mut best_size = min_size;
        for _ in 0..8 {
            let mid = (lo + hi) / 2.0;
            let lines = self.wrap_lines(fonts, text, max_width, mid, weight);
            if lines.len() <= max_lines {
                best_lines = lines;
                best_size = mid;
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (best_lines, best_size)
    }

    // ──────────────────────────────────────────────────────────────
    // Text Drawing Helpers
    // ──────────────────────────────────────────────────────────────

    /// Draws right-aligned text.
    pub fn text_right(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, size_px: f32, color: Color, weight: Weight) -> Result<()> {
        let w = fonts.measure(text, size_px * self.s, weight).0;
        let (dx, dy) = self.tf_pt(x, y);
        let dx = dx - w;
        let col = color.to_d2d(self.global_alpha);
        let brush = self.cache.solid(self.ctx, col)?;
        let brush_as_brush: ID2D1Brush = brush.cast()?;
        fonts.draw_text(self.ctx, &brush_as_brush, text, dx, dy, size_px * self.s, weight, None)
    }

    /// Draws centered text.
    pub fn text_center(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, size_px: f32, color: Color, weight: Weight) -> Result<()> {
        let w = fonts.measure(text, size_px * self.s, weight).0;
        let (dx, dy) = self.tf_pt(x, y);
        let dx = dx - w * 0.5;
        let col = color.to_d2d(self.global_alpha);
        let brush = self.cache.solid(self.ctx, col)?;
        let brush_as_brush: ID2D1Brush = brush.cast()?;
        fonts.draw_text(self.ctx, &brush_as_brush, text, dx, dy, size_px * self.s, weight, None)
    }

    /// Draws centered text with auto-fit size.
    pub fn text_center_fit(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, max_width: f32, max_size: f32, min_size: f32, color: Color, weight: Weight) -> Result<()> {
        let size = self.fit_text_size(fonts, text, max_width, max_size, min_size, weight);
        self.text_center(fonts, text, x, y, size, color, weight)
    }

    /// Draws text with a shadow offset.
    pub fn text_shadow(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, size_px: f32, color: Color, shadow: Color, offset: f32, weight: Weight) -> Result<()> {
        let (dx, dy) = self.tf_pt(x, y);
        let soff = offset * self.s;
        // Shadow
        let scol = shadow.to_d2d(self.global_alpha);
        let sbrush = self.cache.solid(self.ctx, scol)?;
        let sbrush_as_brush: ID2D1Brush = sbrush.cast()?;
        fonts.draw_text(self.ctx, &sbrush_as_brush, text, dx + soff, dy + soff, size_px * self.s, weight, None)?;
        // Foreground
        let fcol = color.to_d2d(self.global_alpha);
        let fbrush = self.cache.solid(self.ctx, fcol)?;
        let fbrush_as_brush: ID2D1Brush = fbrush.cast()?;
        fonts.draw_text(self.ctx, &fbrush_as_brush, text, dx, dy, size_px * self.s, weight, None)
    }

    /// Draws centered text with a shadow offset.
    pub fn text_center_shadow(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, size_px: f32, color: Color, shadow: Color, offset: f32, weight: Weight) -> Result<()> {
        let w = fonts.measure(text, size_px * self.s, weight).0;
        let (dx, dy) = self.tf_pt(x, y);
        let dx = dx - w * 0.5;
        let soff = offset * self.s;
        // Shadow
        let scol = shadow.to_d2d(self.global_alpha);
        let sbrush = self.cache.solid(self.ctx, scol)?;
        let sbrush_as_brush: ID2D1Brush = sbrush.cast()?;
        fonts.draw_text(self.ctx, &sbrush_as_brush, text, dx + soff, dy + soff, size_px * self.s, weight, None)?;
        // Foreground
        let fcol = color.to_d2d(self.global_alpha);
        let fbrush = self.cache.solid(self.ctx, fcol)?;
        let fbrush_as_brush: ID2D1Brush = fbrush.cast()?;
        fonts.draw_text(self.ctx, &fbrush_as_brush, text, dx, dy, size_px * self.s, weight, None)
    }

    /// Draws centered text with auto-fit size and shadow.
    pub fn text_center_shadow_fit(&mut self, fonts: &mut crate::font_d2d::DWriteTextCache, text: &str, x: f32, y: f32, max_width: f32, max_size: f32, min_size: f32, color: Color, shadow: Color, offset: f32, weight: Weight) -> Result<()> {
        let size = self.fit_text_size(fonts, text, max_width, max_size, min_size, weight);
        self.text_center_shadow(fonts, text, x, y, size, color, shadow, offset, weight)
    }

    /// Calculates the vertical center baseline for text.
    pub fn center_baseline(&self, fonts: &mut crate::font_d2d::DWriteTextCache, y: f32, size_px: f32, weight: Weight) -> f32 {
        let (_, height) = fonts.measure("M", size_px * self.s, weight);
        y + height * 0.5 / self.s
    }

    // ──────────────────────────────────────────────────────────────
    // Hit Testing Helpers
    // ──────────────────────────────────────────────────────────────

    /// Returns true if point is inside a rectangle.
    #[inline]
    pub fn hot(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.mx >= x && self.mx < x + w && self.my >= y && self.my < y + h
    }

    /// Returns true if point is inside a rectangle and mouse is down.
    #[inline]
    pub fn hot_t(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.hot(x, y, w, h) && self.mdown
    }

    /// Returns true if point was clicked (hot + mouse went down/up).
    #[inline]
    pub fn hit(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.hot(x, y, w, h) && self.mclicked
    }

    // ──────────────────────────────────────────────────────────────
    // Primitive Drawing
    // ──────────────────────────────────────────────────────────────

    /// Draws a line.
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Color) -> Result<()> {
        let col = color.to_d2d(self.global_alpha);
        let brush = self.cache.solid(self.ctx, col)?;
        let (x1d, y1d) = self.tf_pt(x1, y1);
        let (x2d, y2d) = self.tf_pt(x2, y2);
        let p1 = D2D_POINT_2F { x: x1d, y: y1d };
        let p2 = D2D_POINT_2F { x: x2d, y: y2d };
        unsafe { self.ctx.DrawLine(p1, p2, &brush, width * self.s, None) };
        Ok(())
    }

    /// Draws a line with a 3-stop linear gradient (c_end -> c_mid -> c_end).
    pub fn line_gradient_3stop(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        c_end: Color,
        c_mid: Color,
    ) -> Result<()> {
        let (x1d, y1d) = self.tf_pt(x1, y1);
        let (x2d, y2d) = self.tf_pt(x2, y2);
        let p1 = D2D_POINT_2F { x: x1d, y: y1d };
        let p2 = D2D_POINT_2F { x: x2d, y: y2d };
        let d_end = c_end.to_d2d(self.global_alpha);
        let d_mid = c_mid.to_d2d(self.global_alpha);
        let brush = self
            .cache
            .linear_brush_3stop(self.ctx, d_end, d_mid, d_end, 0.5, p1, p2)?;
        unsafe { self.ctx.DrawLine(p1, p2, &brush, width * self.s, None) };
        Ok(())
    }

    // ──────────────────────────────────────────────────────────────
    // Clip Layer Helpers
    // ──────────────────────────────────────────────────────────────

    /// Pushes a rounded-rectangle clip layer.
    pub fn prepare_rrect_clip(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> Result<()> {
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let dr = (r * self.s).clamp(0.0, max_r);

        let geom: ID2D1RoundedRectangleGeometry = unsafe {
            self.cache.factory().CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: dx,
                    top: dy,
                    right: dx + dw,
                    bottom: dy + dh,
                },
                radiusX: dr,
                radiusY: dr,
            })?
        };

        let layer_params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F { left: dx, top: dy, right: dx + dw, bottom: dy + dh },
            geometricMask: ManuallyDrop::new(Some(geom.into())),
            maskAntialiasMode: windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };

        unsafe { self.ctx.PushLayer(&layer_params, None) };
        let _ = ManuallyDrop::into_inner(layer_params.geometricMask);
        self.clip_stack.push((dx, dy, dw, dh, dr));
        Ok(())
    }

    /// Pushes a clip layer with per-corner radii.
    pub fn prepare_corners_clip(&mut self, x: f32, y: f32, w: f32, h: f32, r_tl: f32, r_tr: f32, r_br: f32, r_bl: f32) -> Result<()> {
        if (r_tl - r_tr).abs() < 1e-3 && (r_tr - r_br).abs() < 1e-3 && (r_br - r_bl).abs() < 1e-3 {
            return self.prepare_rrect_clip(x, y, w, h, r_tl);
        }
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rtl = (r_tl * self.s).clamp(0.0, max_r);
        let rtr = (r_tr * self.s).clamp(0.0, max_r);
        let rbr = (r_br * self.s).clamp(0.0, max_r);
        let rbl = (r_bl * self.s).clamp(0.0, max_r);

        let path: ID2D1PathGeometry = unsafe { self.cache.factory().CreatePathGeometry()?.into() };
        let sink: ID2D1GeometrySink = unsafe { path.Open()? };
        unsafe {
            sink.BeginFigure(
                D2D_POINT_2F { x: dx + rtl, y: dy },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(D2D_POINT_2F { x: dx + dw - rtr, y: dy });
            if rtr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rtr, dy + rtr, rtr, -PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rtr, height: rtr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            } else {
                sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy + dh - rbr });
            if rbr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rbr, dy + dh - rbr, rbr, 0.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbr, height: rbr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            } else {
                sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy + dh });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + rbl, y: dy + dh });
            if rbl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rbl, dy + dh - rbl, rbl, PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbl, height: rbl },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            } else {
                sink.AddLine(D2D_POINT_2F { x: dx, y: dy + dh });
            }
            sink.AddLine(D2D_POINT_2F { x: dx, y: dy + rtl });
            if rtl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rtl, dy + rtl, rtl, PI, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rtl, height: rtl },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            } else {
                sink.AddLine(D2D_POINT_2F { x: dx, y: dy });
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
        }

        let layer_params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F { left: dx, top: dy, right: dx + dw, bottom: dy + dh },
            geometricMask: ManuallyDrop::new(Some(path.into())),
            maskAntialiasMode: windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };

        unsafe { self.ctx.PushLayer(&layer_params, None) };
        let _ = ManuallyDrop::into_inner(layer_params.geometricMask);
        self.clip_stack.push((dx, dy, dw, dh, 0.0));
        Ok(())
    }

    /// Pushes a clip layer with 4 corner radii (tl, tr, br, bl as tuple).
    pub fn prepare_corners4_clip(&mut self, x: f32, y: f32, w: f32, h: f32, radii: (f32, f32, f32, f32)) -> Result<()> {
        self.prepare_corners_clip(x, y, w, h, radii.0, radii.1, radii.2, radii.3)
    }

    // ──────────────────────────────────────────────────────────────
    // High-Level UI Components
    // ──────────────────────────────────────────────────────────────

    /// Elevated card background with subtle border and shadow.
    pub fn elevated_card(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> Result<()> {
        // Shadow
        self.rrect_drop_shadow(x, y, w, h, r, 8.0, tok::obsidian_deep())?;
        // Background
        self.rrect(x, y, w, h, r, tok::surface_base().to_d2d(self.global_alpha))?;
        // Border
        self.rrect_stroke(x, y, w, h, r, 1.0, tok::hairline().to_d2d(self.global_alpha))
    }

    /// Luminous pill button (tonal variant).
    pub fn luminous_pill_button_t(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        base: Color,
        highlight: Color,
        is_hot: bool,
        is_active: bool,
    ) -> Result<()> {
        let r = h * 0.5;
        let t = if is_active { 1.0 } else if is_hot { 0.5 } else { 0.0 };
        let bg = lerp_color(base, highlight, t);
        self.rrect(x, y, w, h, r, bg.to_d2d(self.global_alpha))?;
        self.rrect_stroke(x, y, w, h, r, 1.0, tok::cyno_badge_ring().to_d2d(self.global_alpha))
    }

    /// Luminous pill button (full variant with text).
    pub fn luminous_pill_button(
        &mut self,
        fonts: &mut crate::font_d2d::DWriteTextCache,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        size_px: f32,
        base: Color,
        highlight: Color,
        text_col: Color,
        is_hot: bool,
        is_active: bool,
        weight: Weight,
    ) -> Result<()> {
        self.luminous_pill_button_t(x, y, w, h, base, highlight, is_hot, is_active)?;
        self.text_center(fonts, text, x + w * 0.5, y + h * 0.5, size_px, text_col, weight)
    }

    /// Tactile surface pill (tonal variant).
    pub fn tactile_surface_pill_t(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        base: Color,
        highlight: Color,
        is_hot: bool,
        is_active: bool,
    ) -> Result<()> {
        let r = h * 0.5;
        let t = if is_active { 1.0 } else if is_hot { 0.3 } else { 0.0 };
        let bg = lerp_color(base, highlight, t);
        self.rrect(x, y, w, h, r, bg.to_d2d(self.global_alpha))?;
        self.rrect_stroke(x, y, w, h, r, 1.0, tok::surface_hi().to_d2d(self.global_alpha))
    }

    /// Tactile surface pill (full variant with text).
    pub fn tactile_surface_pill(
        &mut self,
        fonts: &mut crate::font_d2d::DWriteTextCache,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        text: &str,
        size_px: f32,
        base: Color,
        highlight: Color,
        text_col: Color,
        is_hot: bool,
        is_active: bool,
        weight: Weight,
    ) -> Result<()> {
        self.tactile_surface_pill_t(x, y, w, h, base, highlight, is_hot, is_active)?;
        self.text_center(fonts, text, x + w * 0.5, y + h * 0.5, size_px, text_col, weight)
    }

    /// Draws a toggle switch.
    pub fn draw_toggle_switch(&mut self, x: f32, y: f32, w: f32, h: f32, on: bool, hot: bool) -> Result<()> {
        let r = h * 0.5;
        let track_bg = if on { tok::cyno_violet() } else { tok::surface_well() };
        let track_bg = if hot { fade(track_bg, 1.15) } else { track_bg };
        self.rrect(x, y, w, h, r, track_bg.to_d2d(self.global_alpha))?;
        self.rrect_stroke(x, y, w, h, r, 1.0, tok::hairline().to_d2d(self.global_alpha))?;

        let knob_d = h - 4.0;
        let knob_x = if on { x + w - knob_d - 2.0 } else { x + 2.0 };
        let knob_y = y + 2.0;
        self.rrect(knob_x, knob_y, knob_d, knob_d, knob_d * 0.5, tok::cyno_lavender_white().to_d2d(self.global_alpha))?;
        self.rrect_stroke(knob_x, knob_y, knob_d, knob_d, knob_d * 0.5, 1.0, tok::hairline().to_d2d(self.global_alpha))
    }

    /// Draws a glowing striped bar (progress/loading indicator).
    pub fn glowing_striped_bar(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, progress: f32, base: Color, glow: Color) -> Result<()> {
        self.rrect(x, y, w, h, r, base.to_d2d(self.global_alpha))?;
        let fill_w = w * progress.clamp(0.0, 1.0);
        if fill_w > 0.0 {
            self.rrect_corners_vertical_gradient(x, y, fill_w, h, r, 0.0, r, 0.0, glow, base)?;
        }
        self.rrect_stroke(x, y, w, h, r, 1.0, tok::hairline().to_d2d(self.global_alpha))
    }

    /// Draws a dot matrix pattern.
    pub fn dot_matrix(&mut self, x: f32, y: f32, w: f32, h: f32, spacing: f32, dot_r: f32, color: Color) -> Result<()> {
        let col = color.to_d2d(self.global_alpha);
        let mut cx = x;
        while cx < x + w {
            let mut cy = y;
            while cy < y + h {
                self.circle(cx, cy, dot_r, col)?;
                cy += spacing;
            }
            cx += spacing;
        }
        Ok(())
    }

    // ──────────────────────────────────────────────────────────────
    // Advanced Rounded Rectangle Variants
    // ──────────────────────────────────────────────────────────────

    /// Rounded rect with single 1px 3-stop vertical gradient bevel stroke (matching v0.2.4.1 exactly).
    pub fn rrect_bevel_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, stroke_w: f32, highlight: Color, shadow: Color) -> Result<()> {
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let dr = (r * self.s).clamp(0.0, max_r);
        let c0 = highlight.to_d2d(self.global_alpha);
        let c_mid = D2D1_COLOR_F { r: c0.r, g: c0.g, b: c0.b, a: c0.a * 0.35 };
        let c1 = shadow.to_d2d(self.global_alpha);
        let brush = self.cache.linear_brush_3stop(
            self.ctx,
            c0,
            c_mid,
            c1,
            0.55,
            D2D_POINT_2F { x: dx, y: dy },
            D2D_POINT_2F { x: dx, y: dy + dh },
        )?;
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: dx, top: dy, right: dx + dw, bottom: dy + dh },
            radiusX: dr,
            radiusY: dr,
        };
        let style = self.cache.stroke_style_round();
        unsafe { self.ctx.DrawRoundedRectangle(&rr, &brush, stroke_w * self.s, style.as_ref()) };
        Ok(())
    }

    /// Rounded rect with per-corner single 3-stop vertical gradient bevel stroke (matching v0.2.4.1).
    pub fn rrect_corners_bevel_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_tl: f32,
        r_tr: f32,
        r_br: f32,
        r_bl: f32,
        stroke_w: f32,
        highlight: Color,
        shadow: Color,
    ) -> Result<()> {
        if (r_tl - r_tr).abs() < 1e-3 && (r_tr - r_br).abs() < 1e-3 && (r_br - r_bl).abs() < 1e-3 {
            return self.rrect_bevel_stroke(x, y, w, h, r_tl, stroke_w, highlight, shadow);
        }
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rtl = (r_tl * self.s).clamp(0.0, max_r);
        let rtr = (r_tr * self.s).clamp(0.0, max_r);
        let rbr = (r_br * self.s).clamp(0.0, max_r);
        let rbl = (r_bl * self.s).clamp(0.0, max_r);

        let path: ID2D1PathGeometry = unsafe { self.cache.factory().CreatePathGeometry()?.into() };
        let sink: ID2D1GeometrySink = unsafe { path.Open()? };
        unsafe {
            sink.BeginFigure(D2D_POINT_2F { x: dx + rtl, y: dy }, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(D2D_POINT_2F { x: dx + dw - rtr, y: dy });
            if rtr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rtr, dy + rtr, rtr, -PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt, size: D2D_SIZE_F { width: rtr, height: rtr }, rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy + dh - rbr });
            if rbr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rbr, dy + dh - rbr, rbr, 0.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt, size: D2D_SIZE_F { width: rbr, height: rbr }, rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + rbl, y: dy + dh });
            if rbl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rbl, dy + dh - rbl, rbl, PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt, size: D2D_SIZE_F { width: rbl, height: rbl }, rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx, y: dy + rtl });
            if rtl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rtl, dy + rtl, rtl, PI, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt, size: D2D_SIZE_F { width: rtl, height: rtl }, rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: arc_size,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
        }
        let c0 = highlight.to_d2d(self.global_alpha);
        let c_mid = D2D1_COLOR_F { r: c0.r, g: c0.g, b: c0.b, a: c0.a * 0.35 };
        let c1 = shadow.to_d2d(self.global_alpha);
        let brush = self.cache.linear_brush_3stop(
            self.ctx,
            c0,
            c_mid,
            c1,
            0.55,
            D2D_POINT_2F { x: dx, y: dy },
            D2D_POINT_2F { x: dx, y: dy + dh },
        )?;
        let style = self.cache.stroke_style_round();
        unsafe { self.ctx.DrawGeometry(&path, &brush, stroke_w * self.s, style.as_ref()) };
        Ok(())
    }

    /// Rounded rect with 4 corner radii and bevel stroke.
    pub fn rrect_corners4_bevel_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: (f32, f32, f32, f32),
        stroke_w: f32,
        highlight: Color,
        shadow: Color,
    ) -> Result<()> {
        self.rrect_corners_bevel_stroke(x, y, w, h, radii.0, radii.1, radii.2, radii.3, stroke_w, highlight, shadow)
    }

    /// Rounded rect with vertical gradient and per-corner radii.
    pub fn rrect_corners_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_tl: f32,
        r_tr: f32,
        r_br: f32,
        r_bl: f32,
        top: Color,
        bottom: Color,
    ) -> Result<()> {
        if (r_tl - r_tr).abs() < 1e-3 && (r_tr - r_br).abs() < 1e-3 && (r_br - r_bl).abs() < 1e-3 {
            // BH-20: rrect_vertical_gradient multiplies global_alpha internally; pass alpha=1.0.
            return self.rrect_vertical_gradient(
                x,
                y,
                w,
                h,
                r_tl,
                top.to_d2d(1.0),
                bottom.to_d2d(1.0),
            );
        }
        let (dx, dy) = self.tf_pt(x, y);
        let (dw, dh) = (w * self.s, h * self.s);
        let max_r = (dw.min(dh) * 0.5).max(0.0);
        let rt = (r_tl * self.s).clamp(0.0, max_r);
        let rtr = (r_tr * self.s).clamp(0.0, max_r);
        let rbr = (r_br * self.s).clamp(0.0, max_r);
        let rbl = (r_bl * self.s).clamp(0.0, max_r);

        let path: ID2D1PathGeometry = unsafe { self.cache.factory().CreatePathGeometry()?.into() };
        let sink: ID2D1GeometrySink = unsafe { path.Open()? };
        unsafe {
            sink.BeginFigure(
                D2D_POINT_2F { x: dx + rt, y: dy },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(D2D_POINT_2F { x: dx + dw - rtr, y: dy });
            if rtr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rtr, dy + rtr, rtr, -PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rtr, height: rtr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + dw, y: dy + dh - rbr });
            if rbr > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + dw - rbr, dy + dh - rbr, rbr, 0.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbr, height: rbr },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx + rbl, y: dy + dh });
            if rbl > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rbl, dy + dh - rbl, rbl, PI / 2.0, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rbl, height: rbl },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.AddLine(D2D_POINT_2F { x: dx, y: dy + rt });
            if rt > 0.0 {
                let (_, end_pt, arc_size) = arc_endpoints(dx + rt, dy + rt, rt, PI, PI / 2.0);
                sink.AddArc(&windows::Win32::Graphics::Direct2D::D2D1_ARC_SEGMENT {
                    point: end_pt,
                    size: D2D_SIZE_F { width: rt, height: rt },
                    rotationAngle: 0.0,
                    sweepDirection: windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: arc_size,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
        }

        let c0 = top.to_d2d(self.global_alpha);
        let c1 = bottom.to_d2d(self.global_alpha);
        let stops = self.cache.linear_brush(self.ctx, c0, c1, D2D_POINT_2F { x: dx, y: dy }, D2D_POINT_2F { x: dx, y: dy + dh })?;
        unsafe { self.ctx.FillGeometry(&path, &stops, None) };
        Ok(())
    }

    /// Rounded rect with vertical gradient and 4 corner radii.
    pub fn rrect_corners4_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: (f32, f32, f32, f32),
        top: Color,
        bottom: Color,
    ) -> Result<()> {
        self.rrect_corners_vertical_gradient(x, y, w, h, radii.0, radii.1, radii.2, radii.3, top, bottom)
    }

    /// Rounded rect with glow aura.
    pub fn rrect_glow_aura(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, glow_r: f32, color: Color) -> Result<()> {
        // Draw outer glow as multiple expanded rects with fading alpha
        let steps = 4;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let exp = glow_r * t;
            let alpha_mult = 1.0 - t * 0.85;
            // BH-20: self.rrect applies self.global_alpha once; pass 1.0 here.
            let c = fade(color, alpha_mult).to_d2d(1.0);
            self.rrect(x - exp, y - exp, w + 2.0 * exp, h + 2.0 * exp, r + exp, c)?;
        }
        Ok(())
    }

    /// Rounded rect with drop shadow.
    pub fn rrect_drop_shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, color: Color) -> Result<()> {
        // Simple shadow: draw multiple expanded rects with fading alpha behind
        let steps = 5;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let exp = blur * t;
            let alpha_mult = (1.0 - t) * 0.4;
            // BH-20: self.rrect applies self.global_alpha once; pass 1.0 here.
            let c = fade(color, alpha_mult).to_d2d(1.0);
            self.rrect(x - exp, y - exp + blur * 0.3, w + 2.0 * exp, h + 2.0 * exp, r + exp, c)?;
        }
        Ok(())
    }
}// --- NEW PainterD2D STRUCT AND METHODS ---

pub struct PainterD2D<'a> {
    pub ctx: &'a ID2D1DeviceContext,
    pub cache: &'a mut D2DResourceCache,
    pub fonts: &'a mut crate::font_d2d::DWriteTextCache,
    pub s: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub pointer: Option<(f32, f32)>,
    pub hits: &'a mut Vec<Hit>,
    pub interactive: bool,
    pub global_alpha: f32,
    pub hover_slots: [f32; 48],
    pub micro_interactions: bool,
    pub frosted: bool,
    pub frosted_opacity_pct: u32,
    pub type_scale: crate::models::TypeScale,
    // Clip state
    clip_bounds: Option<(f32, f32, f32, f32)>,
}

impl<'a> PainterD2D<'a> {
    pub fn new(
        ctx: &'a ID2D1DeviceContext,
        cache: &'a mut D2DResourceCache,
        fonts: &'a mut crate::font_d2d::DWriteTextCache,
        s: f32,
        origin_x: f32,
        origin_y: f32,
        pointer: Option<(f32, f32)>,
        hits: &'a mut Vec<Hit>,
        interactive: bool,
        global_alpha: f32,
        hover_slots: [f32; 48],
        micro_interactions: bool,
        frosted: bool,
        frosted_opacity_pct: u32,
        type_scale: crate::models::TypeScale,
    ) -> Self {
        Self {
            ctx,
            cache,
            fonts,
            s,
            origin_x,
            origin_y,
            pointer,
            hits,
            interactive,
            global_alpha,
            hover_slots,
            micro_interactions,
            frosted,
            frosted_opacity_pct,
            type_scale,
            clip_bounds: None,
        }
    }

    fn painter(&mut self) -> D2DPainter<'_> {
        let mut p = D2DPainter::new(self.ctx, self.cache, self.s, self.origin_x, self.origin_y, self.global_alpha);
        if let Some((x, y)) = self.pointer {
            p.mx = x;
            p.my = y;
        }
        p
    }

    pub fn well_col(&self) -> Color {
        if self.frosted {
            tok::scale_frosted(tok::surface_well_frosted(), self.frosted_opacity_pct)
        } else {
            tok::surface_well()
        }
    }

    pub fn surface_top_col(&self) -> Color {
        if self.frosted {
            tok::scale_frosted(tok::surface_top_frosted(), self.frosted_opacity_pct)
        } else {
            tok::surface_top()
        }
    }

    fn tf(&self) -> Transform {
        Transform::from_translate(self.origin_x, self.origin_y).pre_scale(self.s, self.s)
    }

    // ── High-Level UI Components (matching Painter exactly) ────────────────────

    pub fn elevated_card(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        aurora: Option<(f32, f32, f32, Color)>,
    ) {
        let shadow_alpha = if self.frosted { 28 } else { 120 };
        self.rrect_drop_shadow(x, y, w, h, r, 3.0, 7.0, shadow_alpha);

        let (c_top, c_bot) = if self.frosted {
            (
                tok::scale_frosted(tok::surface_top_frosted(), self.frosted_opacity_pct),
                tok::scale_frosted(tok::surface_base_frosted(), self.frosted_opacity_pct),
            )
        } else {
            (tok::surface_top(), tok::surface_base())
        };
        self.rrect_vertical_gradient(x, y, w, h, r, c_top, c_bot);

        if self.prepare_rrect_clip(x, y, w, h, r) {
            if let Some((gx, gy, gr, gcol)) = aurora {
                self.radial_glow(
                    x + gx,
                    y + gy,
                    gr,
                    fade(gcol, 0.38),
                    BlendMode::SourceOver,
                    true,
                );
            }
            self.elliptical_glow(
                x + w * 0.5,
                y + h + 4.0,
                w * 0.56,
                (h * 0.48).max(20.0),
                rgba(124, 58, 237, 24),
                BlendMode::Plus,
                true,
            );
            self.elliptical_glow(
                x + w * 0.5,
                y,
                w * 0.52,
                (h * 0.42).max(16.0),
                rgba(240, 235, 255, 18),
                BlendMode::Plus,
                true,
            );
            self.pop_clip();
        }

        self.rrect_bevel_stroke(
            x,
            y,
            w,
            h,
            r,
            1.0,
            rgba(235, 230, 255, 62),
            rgba(168, 85, 247, 36),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn luminous_pill_button_t(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        c_left: Color,
        c_mid: Color,
        c_right: Color,
        hover_t: f32,
    ) {
        let ht = hover_t.clamp(0.0, 1.0);
        let lift_y = y - 0.8 * ht;

        let glow_strength = lerp(0.50, 0.88, ht);
        self.rrect_glow_aura(
            x,
            lift_y,
            w,
            h,
            r,
            lerp(5.5, 8.5, ht),
            fade(c_mid, glow_strength),
        );

        self.rrect_drop_shadow(
            x,
            lift_y,
            w,
            h,
            r,
            lerp(2.5, 3.2, ht),
            lerp(4.0, 5.5, ht),
            115,
        );

        self.rrect_vertical_gradient(x, lift_y, w, h, r, c_left, c_right);

        if self.prepare_rrect_clip(x, lift_y, w, h, r) {
            self.elliptical_glow(
                x + w * 0.5,
                lift_y,
                w * 0.46,
                (h * 0.42).clamp(6.0, 19.0),
                rgba(248, 244, 255, lerp(92.0, 132.0, ht) as u8),
                BlendMode::Plus,
                true,
            );
            self.elliptical_glow(
                x + w * 0.5,
                lift_y + h,
                w * 0.44,
                (h * 0.32).clamp(5.0, 14.0),
                rgba(233, 213, 255, lerp(54.0, 88.0, ht) as u8),
                BlendMode::Plus,
                true,
            );
            self.pop_clip();
        }

        self.rrect_bevel_stroke(
            x,
            lift_y,
            w,
            h,
            r,
            1.15,
            rgba(248, 244, 255, lerp(205.0, 238.0, ht) as u8),
            rgba(221, 214, 254, lerp(95.0, 145.0, ht) as u8),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn luminous_pill_button(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        c_left: Color,
        c_mid: Color,
        c_right: Color,
        is_hot: bool,
    ) {
        self.luminous_pill_button_t(
            x,
            y,
            w,
            h,
            r,
            c_left,
            c_mid,
            c_right,
            if is_hot { 1.0 } else { 0.0 },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tactile_surface_pill_t(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        hover_t: f32,
        active_col: Option<Color>,
    ) {
        let ht = hover_t.clamp(0.0, 1.0);
        let lift_y = y - 0.55 * ht;
        if let Some(ac) = active_col {
            self.rrect_glow_aura(
                x,
                lift_y,
                w,
                h,
                r,
                lerp(4.2, 6.8, ht),
                fade(ac, lerp(0.45, 0.62, ht)),
            );
            self.rrect_vertical_gradient(
                x,
                lift_y,
                w,
                h,
                r,
                fade(ac, lerp(0.42, 0.56, ht)),
                fade(ac, lerp(0.20, 0.30, ht)),
            );
            if self.prepare_rrect_clip(x, lift_y, w, h, r) {
                self.elliptical_glow(
                    x + w * 0.5,
                    lift_y,
                    w * 0.45,
                    (h * 0.40).clamp(4.0, 12.0),
                    rgba(255, 255, 255, lerp(45.0, 74.0, ht) as u8),
                    BlendMode::Plus,
                    true,
                );
                self.pop_clip();
            }
            self.rrect_bevel_stroke(
                x,
                lift_y,
                w,
                h,
                r,
                1.0,
                rgba(255, 255, 255, lerp(135.0, 210.0, ht) as u8),
                fade(ac, lerp(0.45, 0.70, ht)),
            );
        } else {
            let shadow_alpha = if self.frosted { 24 } else { 80 };
            self.rrect_drop_shadow(x, lift_y, w, h, r, 1.5, lerp(2.8, 4.2, ht), shadow_alpha);
            let (top, bot) = if self.frosted {
                let top_base = tok::scale_frosted(rgba(36, 38, 58, 92), self.frosted_opacity_pct);
                let top_hot = tok::scale_frosted(rgba(72, 56, 114, 152), self.frosted_opacity_pct);
                let bot_base = tok::scale_frosted(rgba(20, 22, 36, 78), self.frosted_opacity_pct);
                let bot_hot = tok::scale_frosted(rgba(44, 32, 74, 138), self.frosted_opacity_pct);
                (
                    lerp_color(top_base, top_hot, ht),
                    lerp_color(bot_base, bot_hot, ht),
                )
            } else {
                let top = lerp_color(rgba(32, 34, 50, 255), rgba(64, 50, 102, 255), ht);
                let bot = lerp_color(rgba(19, 20, 32, 245), rgba(38, 28, 66, 255), ht);
                (top, bot)
            };
            self.rrect_vertical_gradient(x, lift_y, w, h, r, top, bot);
            if ht > 0.05 {
                self.rrect_glow_aura(
                    x,
                    lift_y,
                    w,
                    h,
                    r,
                    4.2 * ht,
                    fade(tok::cyno_violet(), 0.28 * ht),
                );
            }
            let rim_top = lerp_color(rgba(235, 230, 255, 54), rgba(196, 148, 255, 195), ht);
            self.rrect_bevel_stroke(x, lift_y, w, h, r, 1.0, rim_top, rgba(168, 85, 247, 22));
        }
    }

    pub fn tactile_surface_pill(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        is_hot: bool,
        active_col: Option<Color>,
    ) {
        self.tactile_surface_pill_t(x, y, w, h, r, if is_hot { 1.0 } else { 0.0 }, active_col);
    }

    pub fn draw_toggle_switch(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        switch_t: f32,
        hover_t: f32,
    ) {
        let st = switch_t.clamp(0.0, 1.0);
        let ht = hover_t.clamp(0.0, 1.0);
        let r = h / 2.0;

        if st > 0.03 || ht > 0.05 {
            let aura_c = lerp_color(tok::cyno_violet(), tok::cyno_violet_hi(), st * 0.35);
            let aura_a = (st * 0.48 + ht * 0.28).clamp(0.0, 0.78);
            self.rrect_glow_aura(x, y, w, h, r, lerp(4.0, 6.5, ht), fade(aura_c, aura_a));
        }

        let (off_top, off_bot) = if self.frosted {
            (
                lerp_color(
                    tok::scale_frosted(rgba(30, 32, 48, 86), self.frosted_opacity_pct),
                    tok::scale_frosted(rgba(52, 46, 78, 128), self.frosted_opacity_pct),
                    ht,
                ),
                tok::scale_frosted(rgba(16, 17, 28, 76), self.frosted_opacity_pct),
            )
        } else {
            (
                lerp_color(rgba(28, 30, 44, 255), rgba(46, 42, 68, 255), ht),
                rgba(14, 15, 24, 255),
            )
        };
        let on_top = lerp_color(tok::cyno_violet(), tok::cyno_orchid(), ht * 0.55);
        let on_bot = tok::cyno_indigo();

        let track_top = lerp_color(off_top, on_top, st);
        let track_bot = lerp_color(off_bot, on_bot, st);
        self.rrect_vertical_gradient(x, y, w, h, r, track_top, track_bot);

        let rim_top = lerp_color(
            rgba(235, 230, 255, lerp(54.0, 150.0, ht) as u8),
            rgba(245, 240, 255, 210),
            st,
        );
        let rim_bot = lerp_color(rgba(235, 230, 255, 18), rgba(196, 148, 255, 120), st);
        self.rrect_bevel_stroke(x, y, w, h, r, 1.0, rim_top, rim_bot);

        let pad = 3.0;
        let knob_r = (h - pad * 2.0) / 2.0 + 0.5 * ht;
        let k_left = x + pad + (h - pad * 2.0) / 2.0;
        let k_right = x + w - pad - (h - pad * 2.0) / 2.0;
        let kx = lerp(k_left, k_right, st);
        let ky = y + h / 2.0;

        self.circle(kx, ky + 1.0, knob_r, rgba(0, 0, 0, 110));
        let knob_col = lerp_color(tok::text2(), tok::cyno_lavender_white(), st.max(ht * 0.45));
        self.circle(kx, ky, knob_r, knob_col);
        if st > 0.2 {
            self.circle(kx, ky, knob_r * 0.34, fade(tok::cyno_deep(), st * 0.75));
        }
    }

    pub fn glowing_striped_bar(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        pct: f32,
        c0: Color,
        c1: Color,
    ) {
        let r = h / 2.0;
        let well = self.well_col();
        self.rrect(x, y, w, h, r, well);
        self.rrect_bevel_stroke(
            x,
            y,
            w,
            h,
            r,
            0.8,
            rgba(0, 0, 0, 120),
            rgba(255, 255, 255, 22),
        );

        let fill_w = (w * pct.clamp(0.0, 1.0)).max(0.0);
        if fill_w > 2.0 {
            self.rrect_glow_aura(x, y, fill_w, h, r, 5.5, fade(c1, 0.75));

            self.rrect_vertical_gradient(x, y, fill_w, h, r, c0, c1);

            if self.prepare_rrect_clip(x, y, fill_w, h, r) {
                let mut pb = PathBuilder::new();
                let step = 10.0;
                let mut sx = x - h;
                while sx < x + fill_w + h {
                    pb.move_to(sx, y + h);
                    pb.line_to(sx + h * 0.7, y);
                    pb.line_to(sx + h * 0.7 + 4.0, y);
                    pb.line_to(sx + 4.0, y + h);
                    pb.close();
                    sx += step;
                }
                if let Some(stripe_path) = pb.finish() {
                    self.fill_path(&stripe_path, rgba(255, 255, 255, 32));
                }

                if let Some(top_glint) = rrect_path(
                    x + 1.0,
                    y + 0.5,
                    (fill_w - 2.0).max(0.0),
                    (h * 0.42).max(1.0),
                    r * 0.5,
                ) {
                    self.fill_path(&top_glint, rgba(255, 255, 255, 75));
                }
                self.pop_clip();
            }

            self.rrect_bevel_stroke(
                x,
                y,
                fill_w,
                h,
                r,
                0.8,
                rgba(255, 255, 255, 140),
                rgba(255, 255, 255, 25),
            );
        }
    }

    pub fn dot_matrix(&mut self, x: f32, y: f32, cols: usize, rows: usize, spacing: f32, c: Color) {
        for r in 0..rows {
            let row_fade = 1.0 - (r as f32 / rows as f32) * 0.7;
            for col_idx in 0..cols {
                let col_fade = 1.0 - (col_idx as f32 / cols as f32) * 0.5;
                let alpha_mul = (row_fade * col_fade).clamp(0.1, 1.0);
                let cx = x + col_idx as f32 * spacing;
                let cy = y + r as f32 * spacing;
                self.circle(cx, cy, 1.0, fade(c, alpha_mul));
            }
        }
    }

    // ── Basic Shapes ──────────────────────────────────────────────────────────
    // BH-20: self.painter() inherits self.global_alpha and D2DPainter multiplies
    // col.a *= self.global_alpha once; convert Color with 1.0 to avoid ga^2 fading.

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.rrect(x, y, w, h, r, c.to_d2d(1.0));
    }

    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.rrect_stroke(x, y, w, h, r, width, c.to_d2d(1.0));
    }

    pub fn rrect_vertical_gradient(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c0: Color, c1: Color) {
        let mut p = self.painter();
        let _ = p.rrect_vertical_gradient(x, y, w, h, r, c0.to_d2d(1.0), c1.to_d2d(1.0));
    }

    pub fn rrect_corners4_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: (f32, f32, f32, f32),
        c0: Color,
        c1: Color,
    ) {
        let mut p = self.painter();
        let _ = p.rrect_corners4_vertical_gradient(x, y, w, h, radii, c0, c1);
    }

    pub fn rrect_bevel_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, hi: Color, sh: Color) {
        let mut p = self.painter();
        let _ = p.rrect_bevel_stroke(x, y, w, h, r, width, hi, sh);
    }

    pub fn rrect_corners4_bevel_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: (f32, f32, f32, f32),
        width: f32,
        hi: Color,
        sh: Color,
    ) {
        let mut p = self.painter();
        let _ = p.rrect_corners4_bevel_stroke(x, y, w, h, radii, width, hi, sh);
    }

    pub fn radial_glow(&mut self, cx: f32, cy: f32, r: f32, c: Color, blend: BlendMode, _use_clip: bool) {
        let is_add = matches!(blend, BlendMode::Plus);
        if is_add {
            unsafe { self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_ADD) };
        }
        let mut p = self.painter();
        let c_center = c.to_d2d(1.0);
        let c_edge = D2D1_COLOR_F { r: c_center.r, g: c_center.g, b: c_center.b, a: 0.0 };
        let _ = p.radial_glow(cx, cy, r, c_center, c_edge);
        if is_add {
            unsafe { self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_SOURCE_OVER) };
        }
    }

    pub fn elliptical_glow(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Color, blend: BlendMode, _use_clip: bool) {
        let is_add = matches!(blend, BlendMode::Plus);
        if is_add {
            unsafe { self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_ADD) };
        }
        let mut p = self.painter();
        let c_center = c.to_d2d(1.0);
        let c_edge = D2D1_COLOR_F { r: c_center.r, g: c_center.g, b: c_center.b, a: 0.0 };
        let _ = p.elliptical_glow(cx, cy, rx, ry, c_center, c_edge);
        if is_add {
            unsafe { self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_SOURCE_OVER) };
        }
    }

    pub fn rrect_glow_aura(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, spread: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.rrect_glow_aura(x, y, w, h, r, spread, c);
    }

    pub fn rrect_drop_shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, _offset_y: f32, spread: f32, max_alpha: u8) {
        let mut p = self.painter();
        let _ = p.rrect_drop_shadow(x, y, w, h, r, spread, rgba(0, 0, 0, max_alpha));
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.circle(cx, cy, r, c.to_d2d(1.0));
    }

    pub fn circle_stroke(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.circle_stroke(cx, cy, r, width, c.to_d2d(1.0));
    }

    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, sweep: f32, width: f32, c: Color) {
        let mut p = self.painter();
        let _ = p.arc(cx, cy, r, a0, sweep, width, c.to_d2d(1.0));
    }

    pub fn line_gradient_blend(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        c_end: Color,
        c_mid: Color,
        blend: BlendMode,
    ) {
        let is_add = matches!(blend, BlendMode::Plus);
        if is_add {
            unsafe {
                self.ctx
                    .SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_ADD)
            };
        }
        let mut p = self.painter();
        let _ = p.line_gradient_3stop(x1, y1, x2, y2, width, c_end, c_mid);
        if is_add {
            unsafe {
                self.ctx.SetPrimitiveBlend(
                    windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
                )
            };
        }
    }

    // ── Text (baseline-anchored, logical units) ──────────────────────────────

    pub fn eff_size(&self, size: f32) -> f32 {
        self.type_scale.caption_lift_px(size)
    }

    pub fn measure(&mut self, t: &str, size: f32, w: Weight) -> f32 {
        let sz = self.eff_size(size) * self.s;
        self.fonts.measure(t, sz, w).0 / self.s
    }

    pub fn ellipsize_cow<'b>(&mut self, t: &'b str, size: f32, w: Weight, max_w: f32) -> Cow<'b, str> {
        let sz = self.eff_size(size) * self.s;
        self.fonts.ellipsize_cow(t, sz, w, max_w * self.s)
    }

    pub fn ellipsize(&mut self, t: &str, size: f32, w: Weight, max_w: f32) -> String {
        let sz = self.eff_size(size) * self.s;
        self.fonts.ellipsize(t, sz, w, max_w * self.s)
    }

    pub fn wrap_lines(&mut self, t: &str, size: f32, w: Weight, max_w: f32) -> Vec<String> {
        let sz = self.eff_size(size) * self.s;
        self.fonts.wrap_lines(t, sz, w, max_w * self.s)
    }

    pub fn fit_text_size(&mut self, t: &str, size: f32, w: Weight, max_w: f32) -> f32 {
        let cur = self.measure(t, size, w);
        if cur <= max_w || cur <= 1.0 || max_w <= 1.0 {
            size
        } else {
            (size * (max_w / cur)).max(6.8)
        }
    }

    pub fn wrap_lines_fit(
        &mut self,
        t: &str,
        base_size: f32,
        min_size: f32,
        w: Weight,
        max_w: f32,
        max_lines: usize,
    ) -> (Vec<String>, f32) {
        let target_lines = max_lines.max(1);
        let floor_sz = min_size.min(base_size).max(7.5);
        let mut sz = base_size;
        for _ in 0..6 {
            let lines = self.wrap_lines(t, sz, w, max_w);
            if lines.len() <= target_lines || (sz - floor_sz).abs() < 0.15 {
                if lines.len() > target_lines {
                    let mut merged: Vec<String> = lines[..target_lines - 1].to_vec();
                    let tail = lines[target_lines - 1..].join(" ");
                    let tail_w = self.measure(&tail, sz, w);
                    let fit_sz = if tail_w > max_w && tail_w > 1.0 {
                        (sz * (max_w / tail_w)).max(7.0)
                    } else {
                        sz
                    };
                    merged.push(tail);
                    return (merged, fit_sz);
                }
                return (lines, sz);
            }
            sz = (sz - (base_size - floor_sz) / 4.0).max(floor_sz);
        }
        let lines = self.wrap_lines(t, floor_sz, w, max_w);
        (lines, floor_sz)
    }

    pub fn text(&mut self, t: &str, x: f32, baseline: f32, size: f32, w: Weight, c: Color) -> f32 {
        let s = self.s;
        let eff = self.eff_size(size);
        let baseline_adj = baseline + (eff - size) * 0.34;
        let (dx, dy) = (self.origin_x + x * s, self.origin_y + baseline_adj * s);
        let col = color_to_d2d(c.r, c.g, c.b, c.a, self.global_alpha);
        if let Ok(brush) = self.cache.solid(self.ctx, col) {
            if let Ok(brush_as_brush) = brush.cast::<ID2D1Brush>() {
                let _ = self.fonts.draw_text_baseline(
                    self.ctx,
                    &brush_as_brush,
                    t,
                    dx,
                    dy,
                    eff * s,
                    w,
                    None,
                );
            }
        }
        self.fonts.measure(t, eff * s, w).0 / s
    }

    pub fn text_right(
        &mut self,
        t: &str,
        right: f32,
        baseline: f32,
        size: f32,
        w: Weight,
        c: Color,
    ) {
        let tw = self.measure(t, size, w);
        self.text(t, right - tw, baseline, size, w, c);
    }

    pub fn text_center(&mut self, t: &str, cx: f32, baseline: f32, size: f32, w: Weight, c: Color) {
        let tw = self.measure(t, size, w);
        self.text(t, cx - tw / 2.0, baseline, size, w, c);
    }

    pub fn text_center_fit(
        &mut self,
        t: &str,
        cx: f32,
        baseline: f32,
        size: f32,
        w: Weight,
        c: Color,
        max_w: f32,
    ) {
        let fit = self.fit_text_size(t, size, w, max_w);
        let tw = self.measure(t, fit, w);
        self.text(t, cx - tw / 2.0, baseline, fit, w, c);
    }

    pub fn text_shadow(
        &mut self,
        t: &str,
        x: f32,
        baseline: f32,
        size: f32,
        w: Weight,
        c: Color,
    ) -> f32 {
        self.text(t, x, baseline + 1.0, size, w, rgba(8, 4, 20, 145));
        self.text(t, x, baseline, size, w, c)
    }

    pub fn text_center_shadow(
        &mut self,
        t: &str,
        cx: f32,
        baseline: f32,
        size: f32,
        w: Weight,
        c: Color,
    ) {
        let tw = self.measure(t, size, w);
        self.text_shadow(t, cx - tw / 2.0, baseline, size, w, c);
    }

    pub fn text_center_shadow_fit(
        &mut self,
        t: &str,
        cx: f32,
        baseline: f32,
        size: f32,
        w: Weight,
        c: Color,
        max_w: f32,
    ) {
        let fit = self.fit_text_size(t, size, w, max_w);
        let tw = self.measure(t, fit, w);
        self.text_shadow(t, cx - tw / 2.0, baseline, fit, w, c);
    }

    pub fn center_baseline(box_y: f32, box_h: f32, size: f32) -> f32 {
        box_y + box_h / 2.0 + size * 0.35
    }

    // ── Interaction ──────────────────────────────────────────────────────────

    pub fn hot(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let ax = self.origin_x + x * self.s;
        let ay = self.origin_y + y * self.s;
        let aw = w * self.s;
        let ah = h * self.s;
        self.interactive
            && matches!(self.pointer, Some((px, py)) if px >= ax && px < ax + aw && py >= ay && py < ay + ah)
    }

    pub fn hot_t(&self, x: f32, y: f32, w: f32, h: f32) -> f32 {
        let is_hot = self.hot(x, y, w, h);
        if !self.micro_interactions {
            return if is_hot { 1.0 } else { 0.0 };
        }
        let idx = self.hits.len();
        if idx < self.hover_slots.len() {
            let anim = self.hover_slots[idx];
            if anim > 0.0 {
                return anim;
            }
        }
        if is_hot { 1.0 } else { 0.0 }
    }

    pub fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, action: Action) {
        if !self.interactive {
            return;
        }
        if let Some(rect) = Rect::from_xywh(
            self.origin_x + x * self.s,
            self.origin_y + y * self.s,
            w * self.s,
            h * self.s,
        ) {
            self.hits.push(Hit { rect, action });
        }
    }

    // ── Clip Layer Helpers ───────────────────────────────────────────────────

    pub fn prepare_rrect_clip(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
        self.prepare_corners4_clip(x, y, w, h, r, r, r, r)
    }

    pub fn prepare_corners_clip(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_top: f32,
        r_bot: f32,
    ) -> bool {
        self.prepare_corners4_clip(x, y, w, h, r_top, r_top, r_bot, r_bot)
    }

    pub fn prepare_corners4_clip(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_tl: f32,
        r_tr: f32,
        r_br: f32,
        r_bl: f32,
    ) -> bool {
        if w <= 0.2 || h <= 0.2 {
            return false;
        }
        let (dev_x, dev_y) = (self.origin_x + x * self.s, self.origin_y + y * self.s);
        let (dev_w, dev_h) = (w * self.s, h * self.s);
        let mut p = self.painter();
        if p.prepare_corners_clip(x, y, w, h, r_tl, r_tr, r_br, r_bl).is_ok() {
            self.clip_bounds = Some((dev_x, dev_y, dev_x + dev_w, dev_y + dev_h));
            true
        } else {
            false
        }
    }

    pub fn prepare_rrect_mask(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
        if w <= 0.2 || h <= 0.2 {
            return false;
        }
        let mut p = self.painter();
        p.prepare_rrect_clip(x, y, w, h, r).is_ok()
    }

    pub fn pop_clip(&mut self) {
        let mut p = self.painter();
        let _ = p.pop_clip();
    }

    // ── Path Operations ──────────────────────────────────────────────────────

    pub fn fill_path(&mut self, path: &Path, c: Color) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let col = c.to_d2d(ga);
            if let Ok(brush) = self.cache.solid(self.ctx, col) {
                let tf = self.tf().to_d2d_matrix3x2();
                unsafe {
                    self.ctx.SetTransform(&tf);
                    self.ctx.FillGeometry(&geom, &brush, None);
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
        }
    }

    pub fn stroke_path(&mut self, path: &Path, width: f32, c: Color) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let col = c.to_d2d(ga);
            if let Ok(brush) = self.cache.solid(self.ctx, col) {
                let style = self.cache.stroke_style_round();
                let tf = self.tf().to_d2d_matrix3x2();
                unsafe {
                    self.ctx.SetTransform(&tf);
                    self.ctx.DrawGeometry(&geom, &brush, width, style.as_ref());
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
        }
    }

    pub fn stroke_path_tf(&mut self, path: &Path, width: f32, c: Color, local_tf: Transform) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let col = c.to_d2d(ga);
            if let Ok(brush) = self.cache.solid(self.ctx, col) {
                let style = self.cache.stroke_style_round();
                let combined = self.tf().pre_concat(local_tf);
                let tf = combined.to_d2d_matrix3x2();
                unsafe {
                    self.ctx.SetTransform(&tf);
                    self.ctx.DrawGeometry(&geom, &brush, width, style.as_ref());
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
        }
    }

    pub fn fill_path_tf(&mut self, path: &Path, c: Color, local_tf: Transform) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let col = c.to_d2d(ga);
            if let Ok(brush) = self.cache.solid(self.ctx, col) {
                let tf = self.tf().pre_concat(local_tf).to_d2d_matrix3x2();
                unsafe {
                    self.ctx.SetTransform(&tf);
                    self.ctx.FillGeometry(&geom, &brush, None);
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
        }
    }

    pub fn fill_path_tf_blend(&mut self, path: &Path, c: Color, local_tf: Transform, blend: BlendMode) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let col = c.to_d2d(ga);
            if let Ok(brush) = self.cache.solid(self.ctx, col) {
                let tf = self.tf().pre_concat(local_tf).to_d2d_matrix3x2();
                let is_add = matches!(blend, BlendMode::Plus);
                unsafe {
                    if is_add {
                        self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_ADD);
                    }
                    self.ctx.SetTransform(&tf);
                    self.ctx.FillGeometry(&geom, &brush, None);
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                    if is_add {
                        self.ctx.SetPrimitiveBlend(windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                    }
                }
            }
        }
    }

    pub fn fill_linear_path_tf(
        &mut self,
        path: &Path,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        c0: Color,
        c1: Color,
        c_mid: Option<Color>,
        local_tf: Transform,
    ) {
        let ga = self.global_alpha;
        if let Ok(geom) = self.cache.path_geom(path) {
            let d2d_c0 = c0.to_d2d(ga);
            let d2d_c1 = c1.to_d2d(ga);
            let start = windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: x0, y: y0 };
            let end = windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F { x: x1, y: y1 };
            let tf = self.tf().pre_concat(local_tf).to_d2d_matrix3x2();
            if let Some(cm) = c_mid {
                let d2d_cm = cm.to_d2d(ga);
                if let Ok(brush) = self.cache.linear_brush_3stop(self.ctx, d2d_c0, d2d_cm, d2d_c1, 0.5, start, end) {
                    unsafe {
                        self.ctx.SetTransform(&tf);
                        self.ctx.FillGeometry(&geom, &brush, None);
                        self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                    }
                }
            } else if let Ok(brush) = self.cache.linear_brush(self.ctx, d2d_c0, d2d_c1, start, end) {
                unsafe {
                    self.ctx.SetTransform(&tf);
                    self.ctx.FillGeometry(&geom, &brush, None);
                    self.ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
        }
    }
}

#[cfg(test)]
mod graphics_reliability_tests {
    use super::*;

    #[test]
    fn bh26_path_bounds_includes_cubic_endpoint() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.cubic_to(10.0, 5.0, 20.0, 15.0, 100.0, 80.0);
        let path = pb.finish().unwrap();
        let (x, y, w, h) = path.bounds().unwrap();
        assert!((x - 0.0).abs() < 1e-3);
        assert!((y - 0.0).abs() < 1e-3);
        assert!((w - 100.0).abs() < 1e-3, "width must include cubic endpoint 100.0, got {}", w);
        assert!((h - 80.0).abs() < 1e-3, "height must include cubic endpoint 80.0, got {}", h);
    }

    #[test]
    fn bh26_linear3_cache_key_distinguishes_mid_pos() {
        let c0 = color_to_d2d(255, 255, 255, 255, 1.0);
        let cm = color_to_d2d(150, 100, 255, 200, 1.0);
        let c1 = color_to_d2d(0, 0, 0, 255, 1.0);
        let s = D2D_POINT_2F { x: 0.0, y: 0.0 };
        let e = D2D_POINT_2F { x: 0.0, y: 100.0 };
        let k50 = linear3_cache_key(c0, cm, c1, 0.50, s, e);
        let k55 = linear3_cache_key(c0, cm, c1, 0.55, s, e);
        assert_ne!(k50, k55, "linear_brush_3stop keys must differentiate mid_pos");
    }
}
