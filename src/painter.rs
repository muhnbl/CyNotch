#![allow(clippy::too_many_arguments)]

use crate::font::{FontRenderer, Weight};
use crate::models::{Action, TypeScale};
use std::borrow::Cow;
use tiny_skia::*;

/// Fixed-capacity stack string buffer for zero-allocation formatting in hot render paths.
pub struct StackStr<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackStr<N> {
    #[inline]
    pub fn new() -> Self {
        Self {
            buf: [0u8; N],
            len: 0,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Default for StackStr<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> std::ops::Deref for StackStr<N> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
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
            // Copy as many complete UTF-8 chars as fit
            let mut copy_len = 0;
            for ch in s.chars() {
                let clen = ch.len_utf8();
                if copy_len + clen > avail {
                    break;
                }
                copy_len += clen;
            }
            self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
            self.len += copy_len;
            Ok(())
        }
    }
}

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

pub fn with_alpha(c: Color, a: u8) -> Color {
    Color::from_rgba(c.red(), c.green(), c.blue(), a as f32 / 255.0).unwrap_or(c)
}

/// Multiply a color's alpha (used for disabled / fading content).
pub fn fade(c: Color, k: f32) -> Color {
    Color::from_rgba(
        c.red(),
        c.green(),
        c.blue(),
        (c.alpha() * k).clamp(0.0, 1.0),
    )
    .unwrap_or(c)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_color(c0: Color, c1: Color, t: f32) -> Color {
    let u = t.clamp(0.0, 1.0);
    Color::from_rgba(
        lerp(c0.red(), c1.red(), u),
        lerp(c0.green(), c1.green(), u),
        lerp(c0.blue(), c1.blue(), u),
        lerp(c0.alpha(), c1.alpha(), u),
    )
    .unwrap_or(c1)
}

// ── Calibrated Design Tokens (Indigo-Obsidian Glass + Luminous Violet) ────────
pub mod tok {
    use super::rgba;
    use tiny_skia::Color;

    // Cool Indigo-Obsidian Glass Surfaces (Never pure #000000)
    pub fn obsidian_shell() -> Color {
        rgba(14, 16, 26, 250)
    }
    pub fn obsidian_deep() -> Color {
        rgba(9, 10, 18, 254)
    }
    pub fn surface_top() -> Color {
        rgba(28, 30, 46, 255)
    }
    pub fn surface_base() -> Color {
        rgba(16, 17, 28, 255)
    }
    pub fn surface_well() -> Color {
        rgba(10, 11, 19, 240)
    }

    // Frosted Indigo-Obsidian Palette (Used under GPU WinRT Composition HostBackdrop + GaussianBlur)
    pub fn obsidian_shell_frosted() -> Color {
        rgba(14, 16, 27, 108)
    }
    pub fn obsidian_deep_frosted() -> Color {
        rgba(9, 10, 19, 122)
    }
    pub fn surface_top_frosted() -> Color {
        rgba(28, 30, 47, 62)
    }
    pub fn surface_base_frosted() -> Color {
        rgba(16, 17, 29, 48)
    }
    pub fn surface_well_frosted() -> Color {
        rgba(10, 11, 20, 64)
    }

    pub fn scale_frosted(c: Color, opacity_pct: u32) -> Color {
        let pct = opacity_pct.clamp(25, 130);
        if pct == 68 {
            return c;
        }
        let a = ((c.alpha() * (pct as f32 / 68.0)).clamp(0.08, 0.96) * 255.0).round() as u8;
        Color::from_rgba8(
            (c.red() * 255.0).round() as u8,
            (c.green() * 255.0).round() as u8,
            (c.blue() * 255.0).round() as u8,
            a,
        )
    }

    pub fn text1() -> Color {
        rgba(246, 244, 255, 255)
    } // #F6F4FF crisp lavender-tinted white
    pub fn text2() -> Color {
        rgba(165, 161, 194, 255)
    } // #A5A1C2 cool slate-lavender
    pub fn text3() -> Color {
        rgba(138, 134, 170, 255)
    } // #8A86AA muted slate-lavender (WCAG AA >= 4.5:1 on dark surfaces)
    pub fn surface() -> Color {
        rgba(235, 230, 255, 15)
    }
    pub fn surface_hi() -> Color {
        rgba(235, 230, 255, 30)
    }
    pub fn hairline() -> Color {
        rgba(235, 230, 255, 26)
    }

    // Photo-Matched Luminous Violet & Pearlescent Lavender Palette
    pub fn cyno_indigo() -> Color {
        rgba(62, 24, 138, 255)
    } // #3E188A deep royal indigo-violet
    pub fn cyno_deep() -> Color {
        rgba(93, 41, 196, 255)
    } // #5D29C4 rich royal purple
    pub fn cyno_violet() -> Color {
        rgba(124, 58, 237, 255)
    } // #7C3AED vivid iris violet
    pub fn cyno_orchid() -> Color {
        rgba(155, 89, 252, 255)
    } // #9B59FC bright orchid bloom
    pub fn cyno_violet_hi() -> Color {
        rgba(196, 148, 255, 255)
    } // #C494FF luminous lavender-orchid accent
    pub fn cyno_mist() -> Color {
        rgba(233, 218, 255, 255)
    } // #E9DAFF pearlescent lavender highlight
    pub fn cyno_lavender_white() -> Color {
        rgba(245, 240, 255, 255)
    } // #F5F0FF bright lavender-white accent
    pub fn cyno_amber_deep() -> Color {
        rgba(161, 98, 7, 255)
    } // legacy/contrast token
    pub fn cyno_gold() -> Color {
        rgba(245, 158, 11, 255)
    } // semantic warning/gold token
    pub fn cyno_gold_hi() -> Color {
        rgba(253, 230, 138, 255)
    } // semantic warning highlight
    pub fn cyno_peach_gold() -> Color {
        rgba(233, 218, 255, 255)
    } // lavender-white pill end
    pub fn text_soft() -> Color {
        rgba(224, 217, 252, 255)
    } // #E0D9FC crisp secondary lavender ink
    pub fn cyan_hi() -> Color {
        rgba(125, 211, 252, 255)
    } // #7DD3FC crisp hydration sky accent
    pub fn cyno_badge_bg() -> Color {
        rgba(109, 40, 217, 78)
    }
    pub fn cyno_badge_ring() -> Color {
        rgba(168, 85, 247, 195)
    }
}

pub struct Hit {
    pub rect: Rect,
    pub action: Action,
}

pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    rrect_corners4_path(x, y, w, h, r, r, r, r)
}

pub fn rrect_corners_path(x: f32, y: f32, w: f32, h: f32, r_top: f32, r_bot: f32) -> Option<Path> {
    rrect_corners4_path(x, y, w, h, r_top, r_top, r_bot, r_bot)
}

pub fn rrect_corners4_path(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r_tl: f32,
    r_tr: f32,
    r_br: f32,
    r_bl: f32,
) -> Option<Path> {
    let max_r = (w * 0.5).min(h * 0.5).max(0.0);
    let rtl = r_tl.min(max_r).max(0.0);
    let rtr = r_tr.min(max_r).max(0.0);
    let rbr = r_br.min(max_r).max(0.0);
    let rbl = r_bl.min(max_r).max(0.0);
    let (ktl, ktr, kbr, kbl) = (0.5523 * rtl, 0.5523 * rtr, 0.5523 * rbr, 0.5523 * rbl);
    let mut pb = PathBuilder::new();
    pb.move_to(x + rtl, y);
    pb.line_to(x + w - rtr, y);
    if rtr > 0.0 {
        pb.cubic_to(x + w - rtr + ktr, y, x + w, y + rtr - ktr, x + w, y + rtr);
    }
    pb.line_to(x + w, y + h - rbr);
    if rbr > 0.0 {
        pb.cubic_to(
            x + w,
            y + h - rbr + kbr,
            x + w - rbr + kbr,
            y + h,
            x + w - rbr,
            y + h,
        );
    }
    pb.line_to(x + rbl, y + h);
    if rbl > 0.0 {
        pb.cubic_to(x + rbl - kbl, y + h, x, y + h - rbl + kbl, x, y + h - rbl);
    }
    pb.line_to(x, y + rtl);
    if rtl > 0.0 {
        pb.cubic_to(x, y + rtl - ktl, x + rtl - ktl, y, x + rtl, y);
    }
    pb.close();
    pb.finish()
}

pub struct Painter<'a> {
    pub pm: &'a mut Pixmap,
    pub clip_mask: &'a mut Mask,
    pub fonts: &'a mut FontRenderer,
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
    pub type_scale: TypeScale,
    /// Bounding box (device px) of the currently prepared clip mask, when known.
    /// Lets shading-heavy primitives rasterize only the visible region.
    pub clip_bounds: Option<(f32, f32, f32, f32)>,
}

// Device-space bounding box of the last clip written into each mask surface.
// Keyed by the mask's data pointer so a freshly allocated (zeroed) mask needs no
// clearing at all, and reused masks only clear the region they previously
// touched instead of the whole canvas on every clip preparation.
thread_local! {
    static CLIP_DIRTY: std::cell::Cell<(usize, u32, u32, u32, u32)> =
        const { std::cell::Cell::new((0, 0, 0, 0, 0)) };
    static CLIP_RADII: std::cell::Cell<[f32; 4]> =
        const { std::cell::Cell::new([0.0; 4]) };
}

impl<'a> Painter<'a> {
    fn tf(&self) -> Transform {
        if self.origin_x.abs() < 1e-4 && self.origin_y.abs() < 1e-4 && (self.s - 1.0).abs() < 1e-4 {
            Transform::identity()
        } else {
            Transform::from_row(self.s, 0.0, 0.0, self.s, self.origin_x, self.origin_y)
        }
    }

    fn col(&self, c: Color) -> Color {
        if self.global_alpha >= 0.999 {
            c
        } else {
            fade(c, self.global_alpha)
        }
    }

    fn paint(c: Color) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(c);
        p.anti_alias = true;
        p
    }

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
        let max_r = (dev_w.min(dev_h) * 0.5).max(0.0);
        self.clip_bounds = Some((dev_x, dev_y, dev_x + dev_w, dev_y + dev_h));
        CLIP_RADII.set([
            (r_tl * self.s).clamp(0.0, max_r),
            (r_tr * self.s).clamp(0.0, max_r),
            (r_br * self.s).clamp(0.0, max_r),
            (r_bl * self.s).clamp(0.0, max_r),
        ]);
        true
    }

    fn prepare_rrect_mask(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
        if w <= 0.2 || h <= 0.2 {
            return false;
        }
        let ptr = self.clip_mask.data().as_ptr() as usize;
        let (last_ptr, px0, py0, px1, py1) = CLIP_DIRTY.get();
        if last_ptr == ptr && px1 > px0 && py1 > py0 {
            let width = self.clip_mask.width();
            let height = self.clip_mask.height();
            let data = self.clip_mask.data_mut();
            let row_len = width as usize;
            for row in py0..py1.min(height) {
                let off = row as usize * row_len;
                data[off + px0 as usize..off + (px1 as usize).min(row_len)].fill(0);
            }
        } else {
            self.clip_mask.data_mut().fill(0);
        }
        let (dev_x, dev_y) = (self.origin_x + x * self.s, self.origin_y + y * self.s);
        let (dev_w, dev_h) = (w * self.s, h * self.s);
        let bx0 = dev_x.floor().max(0.0) as u32;
        let by0 = dev_y.floor().max(0.0) as u32;
        let bx1 = (dev_x + dev_w).ceil().max(0.0) as u32;
        let by1 = (dev_y + dev_h).ceil().max(0.0) as u32;
        CLIP_DIRTY.set((ptr, bx0, by0, bx1, by1));
        if let Some(path) = rrect_path(x, y, w, h, r) {
            let tf = self.tf();
            self.clip_mask.fill_path(&path, FillRule::Winding, true, tf);
            true
        } else {
            false
        }
    }

    // ── Basic Shapes ──────────────────────────────────────────────────────────
    pub fn fill_path(&mut self, path: &Path, c: Color) {
        let tf = self.tf();
        let col = self.col(c);
        self.pm
            .fill_path(path, &Self::paint(col), FillRule::Winding, tf, None);
    }

    pub fn fill_path_tf(&mut self, path: &Path, c: Color, local_tf: Transform) {
        let tf = self.tf().pre_concat(local_tf);
        let col = self.col(c);
        self.pm
            .fill_path(path, &Self::paint(col), FillRule::Winding, tf, None);
    }

    pub fn fill_path_tf_blend(
        &mut self,
        path: &Path,
        c: Color,
        local_tf: Transform,
        blend: BlendMode,
    ) {
        let tf = self.tf().pre_concat(local_tf);
        let col = self.col(c);
        let mut p = Self::paint(col);
        p.blend_mode = blend;
        self.pm.fill_path(path, &p, FillRule::Winding, tf, None);
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
        c2: Option<Color>,
        local_tf: Transform,
    ) {
        let (sc0, sc1) = (self.col(c0), self.col(c1));
        let stops = if let Some(mid) = c2 {
            vec![
                GradientStop::new(0.0, sc0),
                GradientStop::new(0.5, self.col(mid)),
                GradientStop::new(1.0, sc1),
            ]
        } else {
            vec![GradientStop::new(0.0, sc0), GradientStop::new(1.0, sc1)]
        };
        let Some(shader) = LinearGradient::new(
            Point::from_xy(x0, y0),
            Point::from_xy(x1, y1),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return self.fill_path_tf(path, c0, local_tf);
        };
        let p = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let tf = self.tf().pre_concat(local_tf);
        self.pm.fill_path(path, &p, FillRule::Winding, tf, None);
    }

    pub fn stroke_path(&mut self, path: &Path, width: f32, c: Color) {
        let tf = self.tf();
        let col = self.col(c);
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        self.pm
            .stroke_path(path, &Self::paint(col), &stroke, tf, None);
    }

    pub fn stroke_path_tf(&mut self, path: &Path, width: f32, c: Color, local_tf: Transform) {
        let tf = self.tf().pre_concat(local_tf);
        let col = self.col(c);
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        self.pm
            .stroke_path(path, &Self::paint(col), &stroke, tf, None);
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        if let Some(p) = rrect_path(x, y, w, h, r) {
            self.fill_path(&p, c);
        }
    }

    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Color) {
        if let Some(p) = rrect_path(x, y, w, h, r) {
            self.stroke_path(&p, width, c);
        }
    }

    pub fn rrect_gradient(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c0: Color, c1: Color) {
        let Some(path) = rrect_path(x, y, w, h, r) else {
            return;
        };
        let (sc0, sc1) = (self.col(c0), self.col(c1));
        let Some(shader) = LinearGradient::new(
            Point::from_xy(x, y),
            Point::from_xy(x + w, y + h),
            vec![GradientStop::new(0.0, sc0), GradientStop::new(1.0, sc1)],
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return self.fill_path(&path, c0);
        };
        let p = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let tf = self.tf();
        self.pm.fill_path(&path, &p, FillRule::Winding, tf, None);
    }

    pub fn rrect_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top_col: Color,
        bot_col: Color,
    ) {
        self.rrect_corners_vertical_gradient(x, y, w, h, r, r, top_col, bot_col);
    }

    pub fn rrect_corners_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_top: f32,
        r_bot: f32,
        top_col: Color,
        bot_col: Color,
    ) {
        self.rrect_corners4_vertical_gradient(
            x, y, w, h, r_top, r_top, r_bot, r_bot, top_col, bot_col,
        );
    }

    pub fn rrect_corners4_vertical_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_tl: f32,
        r_tr: f32,
        r_br: f32,
        r_bl: f32,
        top_col: Color,
        bot_col: Color,
    ) {
        let Some(path) = rrect_corners4_path(x, y, w, h, r_tl, r_tr, r_br, r_bl) else {
            return;
        };
        let (sc0, sc1) = (self.col(top_col), self.col(bot_col));
        let Some(shader) = LinearGradient::new(
            Point::from_xy(x, y),
            Point::from_xy(x, y + h),
            vec![GradientStop::new(0.0, sc0), GradientStop::new(1.0, sc1)],
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return self.fill_path(&path, top_col);
        };
        let p = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let tf = self.tf();
        self.pm.fill_path(&path, &p, FillRule::Winding, tf, None);
    }

    pub fn rrect_bevel_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        width: f32,
        top_col: Color,
        bot_col: Color,
    ) {
        self.rrect_corners4_bevel_stroke(x, y, w, h, r, r, r, r, width, top_col, bot_col);
    }

    pub fn rrect_corners_bevel_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_top: f32,
        r_bot: f32,
        width: f32,
        top_col: Color,
        bot_col: Color,
    ) {
        self.rrect_corners4_bevel_stroke(
            x, y, w, h, r_top, r_top, r_bot, r_bot, width, top_col, bot_col,
        );
    }

    pub fn rrect_corners4_bevel_stroke(
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
        top_col: Color,
        bot_col: Color,
    ) {
        let Some(path) = rrect_corners4_path(x, y, w, h, r_tl, r_tr, r_br, r_bl) else {
            return;
        };
        let (sc0, sc1) = (self.col(top_col), self.col(bot_col));
        let Some(shader) = LinearGradient::new(
            Point::from_xy(x, y),
            Point::from_xy(x, y + h),
            vec![
                GradientStop::new(0.0, sc0),
                GradientStop::new(0.55, fade(sc0, 0.35)),
                GradientStop::new(1.0, sc1),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return self.stroke_path(&path, width, top_col);
        };
        let p = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        let tf = self.tf();
        self.pm.stroke_path(&path, &p, &stroke, tf, None);
    }

    // ── Advanced Volumetric Glow, Shadow & Gloss Primitives ───────────────────

    fn glow_curve_radial() -> &'static [f32; 256] {
        static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
        LUT.get_or_init(|| {
            Self::build_glow_curve(&[(0.0, 1.0), (0.45, 0.42), (0.75, 0.12), (1.0, 0.0)])
        })
    }

    fn glow_curve_elliptical() -> &'static [f32; 256] {
        static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
        LUT.get_or_init(|| {
            Self::build_glow_curve(&[(0.0, 1.0), (0.42, 0.45), (0.76, 0.10), (1.0, 0.0)])
        })
    }

    fn build_glow_curve(stops: &[(f32, f32); 4]) -> [f32; 256] {
        let mut curve = [0.0f32; 256];
        for (i, out) in curve.iter_mut().enumerate() {
            let d = (i as f32 / 255.0).sqrt();
            *out = if d <= stops[1].0 {
                let u = d / stops[1].0;
                stops[0].1 + (stops[1].1 - stops[0].1) * u
            } else if d <= stops[2].0 {
                let u = (d - stops[1].0) / (stops[2].0 - stops[1].0);
                stops[1].1 + (stops[2].1 - stops[1].1) * u
            } else {
                let u = ((d - stops[2].0) / (stops[3].0 - stops[2].0)).clamp(0.0, 1.0);
                stops[2].1 + (stops[3].1 - stops[2].1) * u
            };
        }
        curve
    }

    #[inline(always)]
    fn div255(v: u32) -> u8 {
        let t = v + 128;
        ((t + (t >> 8)) >> 8) as u8
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_glow_lut(
        &mut self,
        cxd: f32,
        cyd: f32,
        rxd: f32,
        ryd: f32,
        sc: Color,
        curve: &[f32; 256],
        blend: BlendMode,
        clipped: bool,
    ) {
        if rxd <= 0.2 || ryd <= 0.2 || sc.alpha() <= 0.001 {
            return;
        }
        let Some(rect) = self.glow_shade_rect(cxd - rxd, cyd - ryd, cxd + rxd, cyd + ryd, clipped)
        else {
            return;
        };

        if !matches!(blend, BlendMode::SourceOver | BlendMode::Plus) {
            let stops = vec![
                GradientStop::new(0.0, sc),
                GradientStop::new(0.45, fade(sc, 0.42)),
                GradientStop::new(0.75, fade(sc, 0.12)),
                GradientStop::new(1.0, fade(sc, 0.0)),
            ];
            let ts = Transform::from_translate(cxd, cyd).pre_scale(rxd, ryd);
            if let Some(shader) = RadialGradient::new(
                Point::from_xy(0.0, 0.0),
                Point::from_xy(0.0, 0.0),
                1.0,
                stops,
                SpreadMode::Pad,
                ts,
            ) {
                let p = Paint {
                    shader,
                    blend_mode: blend,
                    anti_alias: false,
                    ..Default::default()
                };
                let mask_opt = if clipped {
                    Some(&*self.clip_mask)
                } else {
                    None
                };
                self.pm.fill_rect(rect, &p, Transform::identity(), mask_opt);
            }
            return;
        }

        let br = sc.red();
        let bg = sc.green();
        let bb = sc.blue();
        let ba = sc.alpha();
        let mut lut = [[0u8; 4]; 256];
        for i in 0..256 {
            let a_f = (ba * curve[i]).clamp(0.0, 1.0);
            let a_u8 = (a_f * 255.0 + 0.5) as u8;
            let k = a_f * 255.0;
            lut[i] = [
                (br * k + 0.5) as u8,
                (bg * k + 0.5) as u8,
                (bb * k + 0.5) as u8,
                a_u8,
            ];
        }

        let pw = self.pm.width() as usize;
        let ph = self.pm.height() as usize;
        let ix0 = (rect.left().floor() as i32).clamp(0, pw as i32);
        let iy0 = (rect.top().floor() as i32).clamp(0, ph as i32);
        let ix1 = (rect.right().ceil() as i32).clamp(ix0, pw as i32);
        let iy1 = (rect.bottom().ceil() as i32).clamp(iy0, ph as i32);
        let inv_rx = 1.0 / rxd;
        let inv_ry = 1.0 / ryd;
        let plus = blend == BlendMode::Plus;

        let clip_rrect = if clipped {
            self.clip_bounds
                .map(|(cx0, cy0, cx1, cy1)| (cx0, cy0, cx1, cy1, CLIP_RADII.get()))
        } else {
            None
        };
        let pm_data = self.pm.data_mut();

        for py in iy0..iy1 {
            let yc = py as f32 + 0.5;
            let dy = (yc - cyd) * inv_ry;
            let dy2 = dy * dy;
            if dy2 >= 1.0 {
                continue;
            }
            let span = rxd * (1.0 - dy2).sqrt();
            let mut row_x0 = ix0.max((cxd - span).floor() as i32);
            let mut row_x1 = ix1.min((cxd + span).ceil() as i32);
            if let Some((cx0, cy0, cx1, cy1, [rtl, rtr, rbr, rbl])) = clip_rrect {
                if yc < cy0 || yc > cy1 {
                    continue;
                }
                let mut left_x = cx0;
                let mut right_x = cx1;
                if rtl > 0.5 && yc < cy0 + rtl {
                    let cdy = (cy0 + rtl) - yc;
                    left_x = cx0 + rtl - (rtl * rtl - cdy * cdy).max(0.0).sqrt();
                } else if rbl > 0.5 && yc > cy1 - rbl {
                    let cdy = yc - (cy1 - rbl);
                    left_x = cx0 + rbl - (rbl * rbl - cdy * cdy).max(0.0).sqrt();
                }
                if rtr > 0.5 && yc < cy0 + rtr {
                    let cdy = (cy0 + rtr) - yc;
                    right_x = cx1 - rtr + (rtr * rtr - cdy * cdy).max(0.0).sqrt();
                } else if rbr > 0.5 && yc > cy1 - rbr {
                    let cdy = yc - (cy1 - rbr);
                    right_x = cx1 - rbr + (rbr * rbr - cdy * cdy).max(0.0).sqrt();
                }
                row_x0 = row_x0.max((left_x - 0.5).ceil() as i32);
                row_x1 = row_x1.min((right_x + 0.5).floor() as i32);
            }
            if row_x0 >= row_x1 {
                continue;
            }

            let mut dx = (row_x0 as f32 + 0.5 - cxd) * inv_rx;
            let row_px_off = (py as usize) * pw;
            for px in row_x0..row_x1 {
                let d2 = dx * dx + dy2;
                dx += inv_rx;
                if d2 >= 1.0 {
                    continue;
                }
                let idx = ((d2 * 255.0) as usize).min(255);
                let [sr, sg, sb, sa] = lut[idx];
                if sa == 0 {
                    continue;
                }
                let off = (row_px_off + (px as usize)) * 4;
                if plus {
                    pm_data[off] = pm_data[off].saturating_add(sr);
                    pm_data[off + 1] = pm_data[off + 1].saturating_add(sg);
                    pm_data[off + 2] = pm_data[off + 2].saturating_add(sb);
                    pm_data[off + 3] = pm_data[off + 3].saturating_add(sa);
                } else {
                    let inv_a = (255 - sa) as u32;
                    pm_data[off] = sr.saturating_add(Self::div255(pm_data[off] as u32 * inv_a));
                    pm_data[off + 1] =
                        sg.saturating_add(Self::div255(pm_data[off + 1] as u32 * inv_a));
                    pm_data[off + 2] =
                        sb.saturating_add(Self::div255(pm_data[off + 2] as u32 * inv_a));
                    pm_data[off + 3] =
                        sa.saturating_add(Self::div255(pm_data[off + 3] as u32 * inv_a));
                }
            }
        }
    }

    /// Device-space rectangle the glow actually needs to shade: its own bounds,
    /// cropped to the prepared clip mask when one is active. The gradient is
    /// fully transparent at its radius, so cropping loses no visible pixels.
    fn glow_shade_rect(
        &self,
        dev_x0: f32,
        dev_y0: f32,
        dev_x1: f32,
        dev_y1: f32,
        clipped: bool,
    ) -> Option<Rect> {
        let (mut x0, mut y0, mut x1, mut y1) = (dev_x0, dev_y0, dev_x1, dev_y1);
        if clipped && let Some((cx0, cy0, cx1, cy1)) = self.clip_bounds {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        let (pw, ph) = (self.pm.width() as f32, self.pm.height() as f32);
        let x0 = x0.max(0.0);
        let y0 = y0.max(0.0);
        let x1 = x1.min(pw);
        let y1 = y1.min(ph);
        if x1 - x0 < 0.5 || y1 - y0 < 0.5 {
            return None;
        }
        Rect::from_ltrb(x0, y0, x1, y1)
    }

    pub fn radial_glow(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        c: Color,
        blend: BlendMode,
        clipped: bool,
    ) {
        if r <= 0.0 {
            return;
        }
        let sc = self.col(c);
        let s = self.s;
        let (cxd, cyd) = (self.origin_x + cx * s, self.origin_y + cy * s);
        let rd = r * s;
        self.fill_glow_lut(
            cxd,
            cyd,
            rd,
            rd,
            sc,
            Self::glow_curve_radial(),
            blend,
            clipped,
        );
    }

    pub fn elliptical_glow(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        c: Color,
        blend: BlendMode,
        clipped: bool,
    ) {
        if rx <= 0.2 || ry <= 0.2 {
            return;
        }
        let sc = self.col(c);
        let s = self.s;
        let (cxd, cyd) = (self.origin_x + cx * s, self.origin_y + cy * s);
        let (rxd, ryd) = (rx * s, ry * s);
        self.fill_glow_lut(
            cxd,
            cyd,
            rxd,
            ryd,
            sc,
            Self::glow_curve_elliptical(),
            blend,
            clipped,
        );
    }

    pub fn rrect_glow_aura(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        spread: f32,
        c: Color,
    ) {
        let steps = 3;
        let tf = self.tf();
        let sc = self.col(c);
        for i in (0..steps).rev() {
            let t = (i + 1) as f32 / steps as f32;
            let s_pad = spread * t;
            let alpha_factor = (1.0 - t) * (1.0 - t) * 0.56 + 0.08;
            let col = fade(sc, alpha_factor);
            if let Some(path) = rrect_path(
                x - s_pad,
                y - s_pad,
                w + s_pad * 2.0,
                h + s_pad * 2.0,
                r + s_pad,
            ) {
                let mut p = Self::paint(col);
                p.blend_mode = BlendMode::Plus;
                self.pm.fill_path(&path, &p, FillRule::Winding, tf, None);
            }
        }
    }

    pub fn rrect_drop_shadow(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        offset_y: f32,
        spread: f32,
        max_alpha: u8,
    ) {
        let steps = 2;
        for i in (0..steps).rev() {
            let t = (i + 1) as f32 / steps as f32;
            let pad = spread * t;
            let alpha = ((max_alpha as f32) * (1.0 - t * 0.45) / steps as f32) as u8;
            if let Some(path) = rrect_path(
                x - pad * 0.5,
                y + offset_y * t - pad * 0.25,
                w + pad,
                h + pad,
                r + pad * 0.5,
            ) {
                self.fill_path(&path, rgba(0, 0, 0, alpha));
            }
        }
    }

    /// Elevated 3D Indigo-Obsidian Glass Card (with bottom-edge violet bloom & top specular bevel)
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

    pub fn elevated_card(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        aurora: Option<(f32, f32, f32, Color)>,
    ) {
        // 1. Soft Ambient Drop Shadow below card
        let shadow_alpha = if self.frosted { 28 } else { 120 };
        self.rrect_drop_shadow(x, y, w, h, r, 3.0, 7.0, shadow_alpha);

        // 2. Vertical Gradient Surface (Lit Top -> Deep Bottom)
        let (c_top, c_bot) = if self.frosted {
            (
                tok::scale_frosted(tok::surface_top_frosted(), self.frosted_opacity_pct),
                tok::scale_frosted(tok::surface_base_frosted(), self.frosted_opacity_pct),
            )
        } else {
            (tok::surface_top(), tok::surface_base())
        };
        self.rrect_vertical_gradient(x, y, w, h, r, c_top, c_bot);

        // 3. Clipped internal lighting (Aurora + Bottom Violet Mist + Specular top oval wash)
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
            // Subtle bottom-edge luminous violet mist matching the glass card aesthetic
            self.elliptical_glow(
                x + w * 0.5,
                y + h + 4.0,
                w * 0.56,
                (h * 0.48).max(20.0),
                rgba(124, 58, 237, 24),
                BlendMode::Plus,
                true,
            );
            // Top subtle oval specular dome inside card (hugs upper bezel cleanly)
            self.elliptical_glow(
                x + w * 0.5,
                y,
                w * 0.52,
                (h * 0.42).max(16.0),
                rgba(240, 235, 255, 18),
                BlendMode::Plus,
                true,
            );
        }

        // 4. Cool lavender-white top rim fading into soft violet bottom rim
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

    /// Volumetric Glossy Pill Button with smooth hover intensity `hover_t` (0.0..=1.0).
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

        // 1. Ambient Halo Bloom
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

        // 2. Crisp Separation Drop Shadow
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

        // 3. Multi-stop diagonal mesh base fill (c_left -> c_mid -> c_right)
        if let Some(path) = rrect_path(x, lift_y, w, h, r) {
            let (sc0, sc1, sc2) = (self.col(c_left), self.col(c_mid), self.col(c_right));
            if let Some(shader) = LinearGradient::new(
                Point::from_xy(x, lift_y),
                Point::from_xy(x + w, lift_y + h * 0.35),
                vec![
                    GradientStop::new(0.0, sc0),
                    GradientStop::new(0.55, sc1),
                    GradientStop::new(1.0, sc2),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                let p = Paint {
                    shader,
                    anti_alias: true,
                    ..Default::default()
                };
                self.pm
                    .fill_path(&path, &p, FillRule::Winding, self.tf(), None);
            } else {
                self.fill_path(&path, c_mid);
            }
        }

        // 4. Tight Elliptical Top Oval Glint + Luminous Lavender-White Bottom Rim Bounce
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
        }

        // 5. Crisp Volumetric Bevel Rim Stroke (Lavender-White Top -> Orchid-Lavender Bottom)
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

    /// Tactile Secondary Pill / Switch / Control Button with smooth `hover_t` micro-interaction.
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

    /// Smoothly animated pill toggle switch (`switch_t`: 0.0 off .. 1.0 on, `hover_t`: 0.0..1.0).
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

    /// Glowing Striped Liquid Energy Bar (Directly inspired by Flight Card & Walk Fitness Card!)
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
        // 1. Sunken track well
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
            // 2. Ambient Luminous Bloom Halo around active portion
            self.rrect_glow_aura(x, y, fill_w, h, r, 5.5, fade(c1, 0.75));

            // 3. Multi-stop Horizontal Liquid Energy Fill
            if let Some(path) = rrect_path(x, y, fill_w, h, r) {
                let (sc0, sc1, sc2) = (self.col(c0), self.col(c1), self.col(tok::cyno_mist()));
                if let Some(shader) = LinearGradient::new(
                    Point::from_xy(x, y),
                    Point::from_xy(x + fill_w, y),
                    vec![
                        GradientStop::new(0.0, sc0),
                        GradientStop::new(0.75, sc1),
                        GradientStop::new(1.0, sc2),
                    ],
                    SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    let p = Paint {
                        shader,
                        anti_alias: true,
                        ..Default::default()
                    };
                    self.pm
                        .fill_path(&path, &p, FillRule::Winding, self.tf(), None);
                } else {
                    self.fill_path(&path, c1);
                }
            }

            // 4. Diagonal Translucent Sheen Stripes & Specular Top Line inside Clipped Fill
            if self.prepare_rrect_mask(x, y, fill_w, h, r) {
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
                    let mut p = Self::paint(self.col(rgba(255, 255, 255, 32)));
                    p.blend_mode = BlendMode::Plus;
                    self.pm.fill_path(
                        &stripe_path,
                        &p,
                        FillRule::Winding,
                        self.tf(),
                        Some(&*self.clip_mask),
                    );
                }

                // Specular bright top rim inside progress fill
                if let Some(top_glint) = rrect_path(
                    x + 1.0,
                    y + 0.5,
                    (fill_w - 2.0).max(0.0),
                    (h * 0.42).max(1.0),
                    r * 0.5,
                ) {
                    let mut p = Self::paint(self.col(rgba(255, 255, 255, 75)));
                    p.blend_mode = BlendMode::Plus;
                    self.pm.fill_path(
                        &top_glint,
                        &p,
                        FillRule::Winding,
                        self.tf(),
                        Some(&*self.clip_mask),
                    );
                }
            }

            // 5. Crisp border highlight on filled liquid pill
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

    /// Halftone Micro-Dot Matrix Pattern (Inspired by YYZ->HND Flight Card!)
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

    // ── Legacy & Core Helpers ─────────────────────────────────────────────────
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.fill_path(&p, c);
        }
    }

    pub fn circle_stroke(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Color) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.stroke_path(&p, width, c);
        }
    }

    /// Stroke an arc from angle `a0` sweeping `sweep` radians (0 = 3 o'clock).
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, sweep: f32, width: f32, c: Color) {
        if sweep.abs() < 0.001 {
            return;
        }
        let steps = ((sweep.abs() / std::f32::consts::TAU) * 96.0)
            .ceil()
            .max(2.0) as usize;
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let a = a0 + sweep * (i as f32 / steps as f32);
            let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
            if i == 0 {
                pb.move_to(x, y)
            } else {
                pb.line_to(x, y)
            }
        }
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, width, c);
        }
    }

    // ── Text (baseline-anchored, logical units) ──────────────────────────────
    pub fn eff_size(&self, size: f32) -> f32 {
        self.type_scale.caption_lift_px(size)
    }

    pub fn measure(&self, t: &str, size: f32, w: Weight) -> f32 {
        self.fonts.measure(t, self.eff_size(size) * self.s, w) / self.s
    }

    pub fn ellipsize_cow<'b>(&self, t: &'b str, size: f32, w: Weight, max_w: f32) -> Cow<'b, str> {
        self.fonts
            .ellipsize_cow(t, self.eff_size(size) * self.s, w, max_w * self.s)
    }

    pub fn ellipsize(&self, t: &str, size: f32, w: Weight, max_w: f32) -> String {
        self.fonts
            .ellipsize(t, self.eff_size(size) * self.s, w, max_w * self.s)
    }

    pub fn wrap_lines(&self, t: &str, size: f32, w: Weight, max_w: f32) -> Vec<String> {
        self.fonts
            .wrap_lines(t, self.eff_size(size) * self.s, w, max_w * self.s)
    }

    /// Returns a raw size <= `size` whose scaled rendering fits inside `max_w`.
    pub fn fit_text_size(&self, t: &str, size: f32, w: Weight, max_w: f32) -> f32 {
        let cur = self.measure(t, size, w);
        if cur <= max_w || cur <= 1.0 || max_w <= 1.0 {
            size
        } else {
            (size * (max_w / cur)).max(6.8)
        }
    }

    /// Wraps `t` without ellipsis, automatically stepping font size down toward `min_size`
    /// if needed so that the full text fits within `max_lines` lines whenever possible.
    pub fn wrap_lines_fit(
        &self,
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
                    // Merge any trailing overflow onto the final line and scale width-fit so 0 text is lost
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
        let col = self.col(c);
        let adv_px = self.fonts.draw(
            self.pm,
            t,
            self.origin_x + x * s,
            self.origin_y + baseline_adj * s,
            eff * s,
            w,
            col,
        );
        adv_px / s
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

    /// Baseline that vertically centers a line of `size` text inside a box.
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

    /// Returns smoothly animated hover intensity (0.0..=1.0) for the next hit target slot.
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
}
