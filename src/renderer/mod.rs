mod mascot;
mod notifications;
mod vertical;
mod views;

use crate::font_d2d::{DWriteTextCache, Weight};
use crate::icons::{self, Icon};
use crate::models::{
    Action, AppState, IdleMetric, LyricSize, LyricStyle, MascotMood, NotchMode, NotchPlacement,
    NotificationKind, ShelfKind, ToggleId, TypeScale,
};
use crate::painter_d2d::Hit;
use crate::painter_d2d::{BlendMode, Color, D2DResourceCache, PainterD2D, StackStr, fade, lerp_color, rgba, tok};
use crate::telemetry::{format_speed_long, format_speed_short};
use crate::localtime::{format_hhmm, local_now};
use std::borrow::Cow;
use std::fmt::Write as _;

pub struct Renderer {
    pub width: u32,
    pub height: u32,
    pub fonts: DWriteTextCache,
    pub d2d_cache: Option<D2DResourceCache>,
    pub hits: Vec<Hit>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let fonts = DWriteTextCache::new().expect("DWrite");
        Self {
            width: w,
            height: h,
            fonts,
            d2d_cache: None,
            hits: Vec::new(),
        }
            }

            pub fn resize(&mut self, width: u32, height: u32) {
                let w = width.max(1);
                let h = height.max(1);
                if self.width != w || self.height != h {
                    self.width = w;
                    self.height = h;
                    self.d2d_cache = None;
                    self.hits.clear();
                }
            }

            /// Drop cached expanded/chrome pixmaps when idle to free heap memory.
            pub fn trim_idle_caches(&mut self) {
                self.fonts.trim();
            }

            pub fn clear_d2d_cache(&mut self) {
                self.d2d_cache = None;
            }

            pub fn render_d2d(&mut self, ctx: &windows::Win32::Graphics::Direct2D::ID2D1DeviceContext, state: &AppState, pointer: Option<(f32, f32)>) {
                if self.d2d_cache.is_none() {
                    match D2DResourceCache::new(ctx) {
                        Ok(c) => self.d2d_cache = Some(c),
                        Err(_) => return,
                    }
                }
                unsafe {
                    ctx.Clear(Some(&windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
                }
                let (ox, oy) = Self::compute_origin(state, self.width as f32, self.height as f32);
                let Some(cache) = self.d2d_cache.as_mut() else {
                    return;
                };
                self.hits.clear();
                let frosted = state.is_frosted_active();
                let w = state.current_width;
                let h = state.current_height;
                let exp_w = state.expanded_width();
                let exp_h = state.expanded_height();
                let base_w = state.base_expanded_width();
                let base_h = state.base_expanded_height();
                let settled_expanded = state.is_hovered && (w - exp_w).abs() < 0.5 && (h - exp_h).abs() < 0.5;
                let t = if settled_expanded { 1.0 } else { state.anim_expand_t.clamp(0.0, 1.0) };
                let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();

                let mut p = PainterD2D::new(
                    ctx,
                    cache,
                    &mut self.fonts,
                    1.0,
                    ox,
                    oy,
                    pointer,
                    &mut self.hits,
                    true,
                    1.0,
                    state.hover_slots,
                    state.micro_interactions,
                    frosted,
                    state.frosted_opacity_pct,
                    state.type_scale,
                );

                // 1. Volumetric Obsidian Shell (matching v0.2.4.1 exactly: no permanent outer border)
                let (shell_top, shell_bot) = if frosted {
                    (
                        tok::scale_frosted(tok::obsidian_shell_frosted(), state.frosted_alpha_pct()),
                        tok::scale_frosted(tok::obsidian_deep_frosted(), state.frosted_alpha_pct()),
                    )
                } else {
                    (tok::obsidian_shell(), tok::obsidian_deep())
                };
                p.rrect_corners4_vertical_gradient(0.0, 0.0, w, h, (r_tl, r_tr, r_br, r_bl), shell_top, shell_bot);

                // 2. Internal Atmospheric Aurora Glows (matching v0.2.4.1)
                let glow_scale = if frosted {
                    (state.frosted_alpha_pct() as f32 / 68.0).clamp(0.25, 1.0)
                } else {
                    1.0
                };
                if t > 0.05 {
                    if p.prepare_corners4_clip(0.0, 0.0, w, h, r_tl, r_tr, r_br, r_bl) {
                        p.radial_glow(
                            48.0,
                            24.0,
                            126.0,
                            fade(rgba(124, 58, 237, 46), t * glow_scale),
                            BlendMode::SourceOver,
                            true,
                        );
                        p.radial_glow(
                            w - 52.0,
                            h - 10.0,
                            116.0,
                            fade(rgba(124, 58, 237, 48), t * glow_scale),
                            BlendMode::SourceOver,
                            true,
                        );
                        p.elliptical_glow(
                            w * 0.5,
                            h + 6.0,
                            w * 0.56,
                            (h * 0.36).max(24.0),
                            fade(rgba(168, 85, 247, 34), t * glow_scale),
                            BlendMode::Plus,
                            true,
                        );
                        p.radial_glow(
                            w * 0.5,
                            -10.0,
                            w * 0.45,
                            fade(rgba(235, 230, 255, 20), t * glow_scale),
                            BlendMode::Plus,
                            true,
                        );
                        if t < 0.95 {
                            let g_x = if state.placement == NotchPlacement::Right {
                                w - 24.0
                            } else {
                                24.0
                            };
                            p.radial_glow(
                                g_x,
                                h * 0.5,
                                (h.min(w) * 0.85).max(16.0),
                                fade(rgba(139, 92, 246, 38), (1.0 - t) * glow_scale),
                                BlendMode::SourceOver,
                                true,
                            );
                        }
                        // Whisper-subtle physical obsidian glass top specular sheen
                        let sheen_inset = (r_tl.max(r_tr) * 0.65).min(w * 0.22);
                        p.line_gradient_blend(
                            sheen_inset,
                            1.0,
                            (w - sheen_inset).max(sheen_inset + 4.0),
                            1.0,
                            1.0,
                            rgba(255, 255, 255, 0),
                            fade(rgba(236, 224, 255, 34), (0.55 + 0.45 * t) * glow_scale),
                            BlendMode::Plus,
                        );
                        p.pop_clip();
                    }
                } else {
                    // Idle collapsed capsule: fit glow strictly inside pill bounds to avoid
                    // allocating an offscreen D2D PushLayer render target 30 times/sec.
                    let max_r = (h.min(w) * 0.48).max(6.0);
                    let g_x = if state.placement == NotchPlacement::Right {
                        (w - max_r).max(w * 0.5)
                    } else {
                        max_r.min(w * 0.5)
                    };
                    p.radial_glow(
                        g_x,
                        h * 0.5,
                        max_r,
                        fade(rgba(139, 92, 246, 38), glow_scale),
                        BlendMode::SourceOver,
                        false,
                    );
                    if w >= 60.0 && h >= 18.0 {
                        let sheen_inset = (h * 0.48).min(w * 0.24);
                        p.line_gradient_blend(
                            sheen_inset,
                            1.0,
                            (w - sheen_inset).max(sheen_inset + 4.0),
                            1.0,
                            0.9,
                            rgba(255, 255, 255, 0),
                            fade(rgba(232, 218, 255, 32), glow_scale),
                            BlendMode::Plus,
                        );
                    }
                }

                let show_idle_content = !settled_expanded && t < 0.32;
                if show_idle_content {
                    let fade_out = ((0.32 - t) / 0.32).clamp(0.0, 1.0);
                    if fade_out > 0.02 {
                        p.global_alpha = fade_out;
                        if state.notification.is_some() && w >= 220.0 && h >= 48.0 {
                            let target_bs = state.banner_scale();
                            let (base_bw, base_bh) = if state.placement.is_vertical() {
                                (AppState::VERT_BANNER_WIDTH, AppState::VERT_BANNER_HEIGHT)
                            } else {
                                (AppState::BANNER_WIDTH, AppState::BANNER_HEIGHT)
                            };
                            let bs = (w / base_bw).min(h / base_bh).min(target_bs).max(0.65);
                            p.s = bs;
                            Self::draw_notification_banner(&mut p, state, w / bs, h / bs);
                        } else if state.placement.is_vertical() {
                            if state.minimized_island || w < 36.0 || h < 120.0 {
                                p.type_scale = TypeScale::Compact;
                                Self::draw_minimized_idle_vertical(&mut p, state, w, h);
                            } else {
                                let target_is = state.type_scale.idle_ui_scale();
                                let is = (w / state.size_preset.idle_width_vertical())
                                    .min(h / state.size_preset.idle_height_vertical())
                                    .min(target_is)
                                    .max(0.75);
                                p.s = is;
                                Self::draw_idle_vertical(&mut p, state, w / is, h / is);
                            }
                        } else if state.minimized_island || w < 210.0 || h < 32.0 {
                            p.type_scale = TypeScale::Compact;
                            Self::draw_minimized_idle_capsule(&mut p, state, w, h);
                        } else {
                            let target_is = state.type_scale.idle_ui_scale();
                            let is = (w / state.size_preset.idle_width())
                                .min(h / state.size_preset.idle_height_horizontal())
                                .min(target_is)
                                .max(0.75);
                            p.s = is;
                            Self::draw_idle_capsule(&mut p, state, w / is, h / is);
                        }
                    }
                } else {
                    let fade_in = if settled_expanded { 1.0 } else { ((t - 0.32) / 0.68).clamp(0.0, 1.0) };
                    if fade_in > 0.01 {
                        p.global_alpha = fade_in;
                        let target_s = state.expanded_scale();
                        if settled_expanded {
                            p.s = target_s;
                        } else {
                            let scale_fit = (w / base_w).min(h / base_h).min(target_s).max(0.15);
                            p.s = scale_fit;
                            p.origin_x = ox + (w - base_w * scale_fit) * 0.5;
                            p.origin_y = oy + (h - base_h * scale_fit) * 0.5;
                        }
                        if state.placement.is_vertical() {
                            Self::draw_expanded_vertical(&mut p, state, base_w, base_h, settled_expanded);
                        } else {
                            Self::draw_expanded(&mut p, state, base_w, base_h, settled_expanded);
                        }
                    }
                }
            }

            pub fn compute_origin(state: &AppState, canvas_w: f32, canvas_h: f32) -> (f32, f32) {
                let ax = state.anim_anchor_x.clamp(0.0, 1.0);
                let ay = state.anim_anchor_y.clamp(0.0, 1.0);
                let (base_x, base_y) = match state.placement {
                    NotchPlacement::Top => (((canvas_w - state.current_width) * ax).max(0.0), 0.0),
                    NotchPlacement::Left => (0.0, ((canvas_h - state.current_height) * ay).max(0.0)),
                    NotchPlacement::Right => (
                        (canvas_w - state.current_width).max(0.0),
                        ((canvas_h - state.current_height) * ay).max(0.0),
                    ),
                };
                if state.notification.is_some()
                    && !state.is_hovered
                    && state.current_width >= 220.0
                    && state.current_height >= 48.0
                {
                    let hidden = 1.0 - state.anim_notif_t.clamp(0.0, 1.0);
                    match state.placement {
                        NotchPlacement::Top => (base_x, -state.current_height * hidden),
                        NotchPlacement::Left => (-state.current_width * hidden, base_y),
                        NotchPlacement::Right => (base_x + state.current_width * hidden, base_y),
                    }
                } else {
                    (base_x, base_y)
                }
            }

    /// Exact hit-test against the visible rounded notch shell (0.5px tolerance for the 1px rim stroke).
    pub fn point_in_capsule(
        state: &AppState,
        canvas_w: f32,
        canvas_h: f32,
        px: f32,
        py: f32,
    ) -> bool {
        let (ox, oy) = Self::compute_origin(state, canvas_w, canvas_h);
        let w = state.current_width.max(1.0);
        let h = state.current_height.max(1.0);
        let tol = 0.5;
        let lx = px - ox;
        let ly = py - oy;
        if lx < -tol || lx > w + tol || ly < -tol || ly > h + tol {
            return false;
        }
        let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();
        let half = (w.min(h) * 0.5).max(0.0);
        let checks = [
            (
                r_tl.min(half),
                r_tl.min(half),
                r_tl.min(half),
                lx < r_tl.min(half) && ly < r_tl.min(half),
            ),
            (
                r_tr.min(half),
                w - r_tr.min(half),
                r_tr.min(half),
                lx > w - r_tr.min(half) && ly < r_tr.min(half),
            ),
            (
                r_br.min(half),
                w - r_br.min(half),
                h - r_br.min(half),
                lx > w - r_br.min(half) && ly > h - r_br.min(half),
            ),
            (
                r_bl.min(half),
                r_bl.min(half),
                h - r_bl.min(half),
                lx < r_bl.min(half) && ly > h - r_bl.min(half),
            ),
        ];
        for (r, cx, cy, in_corner_box) in checks {
            if in_corner_box && r > 0.5 {
                let dx = lx - cx;
                let dy = ly - cy;
                let limit = r + tol;
                if dx * dx + dy * dy > limit * limit {
                    return false;
                }
            }
        }
        true
            }

            pub(super) fn draw_expanded_anim_overlay(p: &mut PainterD2D, state: &AppState) {
        if state.show_notif_history {
            return;
        }
        let card_y = 60.0;
        let anim_t = state.anim_time;
        match state.mode {
            NotchMode::Home => {
                if state.show_mascot {
                    let margin_x = 20.0;
                    Self::draw_cyno_mascot(
                        p,
                        margin_x + 30.0,
                        card_y + 30.5,
                        0.96,
                        state.mascot_mood(),
                        anim_t,
                        false,
                    );
                }
            }
            NotchMode::Music => {
                let card_x = 20.0;
                let card_size = 154.0;
                if state.show_mascot && state.media.is_playing {
                    let hero_cx = card_x + card_size * 0.5;
                    let hero_cy = card_y + 62.0;
                    Self::draw_cyno_mascot(
                        p,
                        hero_cx,
                        hero_cy,
                        1.76,
                        MascotMood::Music,
                        anim_t,
                        false,
                    );
                }
                if !state.show_lyrics && state.equalizer_enabled {
                    let pad_x = AppState::MUSIC_SCRUBBER_X;
                    let inner_w = AppState::MUSIC_SCRUBBER_W;
                    let title_y = card_y + 26.0;
                    let wave_x = pad_x + inner_w - 58.0;
                    let wave_y = title_y - 12.0;
                    let bar_heights = [
                        6.0, 10.0, 14.0, 11.0, 16.0, 18.0, 13.0, 9.0, 15.0, 18.0, 12.0, 7.0, 14.0,
                        10.0,
                    ];
                    let is_playing = state.media.is_playing;
                    for (i, &base_bh) in bar_heights.iter().enumerate() {
                        let bh = if is_playing {
                            let wave = (state.anim_time * 8.5 + i as f32 * 0.75).sin() * 0.5 + 0.5;
                            (base_bh * 0.35 + wave * (base_bh * 0.85)).clamp(3.0, 18.0)
                        } else {
                            (base_bh * 0.3).max(3.0)
                        };
                        let bx = wave_x + (i as f32 * 4.2);
                        let by = wave_y + (18.0 - bh) / 2.0;
                        let bar_col = if i % 4 == 0 {
                            tok::cyno_mist()
                        } else {
                            tok::cyno_violet_hi()
                        };
                        p.rrect(bx, by, 2.4, bh, 1.2, bar_col);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn idle_arc_fraction_and_color(state: &AppState) -> (Option<f32>, Color) {
        if let Some(ref notif) = state.notification {
            let col = match notif.kind {
                NotificationKind::SystemToast => tok::cyan_hi(),
                NotificationKind::MemoryPurge => tok::cyno_mist(),
                NotificationKind::Capture
                | NotificationKind::Pasted
                | NotificationKind::General => tok::cyno_violet_hi(),
            };
            return (Some(1.0), col);
        }
        if state.media.is_playing && state.media.duration_ms > 0 {
            let frac =
                (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0);
            return (Some(frac), tok::cyno_violet_hi());
        }
        match state.idle_metric {
            IdleMetric::Ram => (
                Some((state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)),
                tok::cyno_mist(),
            ),
            IdleMetric::Cpu => (
                Some((state.telemetry.cpu_usage / 100.0).clamp(0.0, 1.0)),
                tok::cyno_violet_hi(),
            ),
            IdleMetric::NetSpeed => {
                let kb = (state.telemetry.net_down_kbps + state.telemetry.net_up_kbps).max(0.0);
                let frac = if kb < 1.0 {
                    0.05
                } else {
                    (kb.log10() / 4.5).clamp(0.08, 1.0)
                };
                (Some(frac), tok::cyan_hi())
            }
            IdleMetric::Clock => {
                let mins = (local_now().minute as f32 / 60.0).clamp(0.04, 1.0);
                (Some(mins), tok::cyno_mist())
            }
            IdleMetric::Minimal => (None, rgba(255, 255, 255, 36)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_progress_arc_badge(
        p: &mut PainterD2D,
        cx: f32,
        cy: f32,
        r: f32,
        frac_opt: Option<f32>,
        ring_col: Color,
        bg_col: Color,
        icon: Icon,
        icon_col: Color,
        arc_enabled: bool,
    ) {
        let bg = if p.frosted && bg_col == tok::surface_top() {
            p.surface_top_col()
        } else {
            bg_col
        };
        p.circle(cx, cy, r, bg);
        if arc_enabled {
            p.circle_stroke(cx, cy, r, 1.35, rgba(255, 255, 255, 22));
            if let Some(frac) = frac_opt
                && frac > 0.015
            {
                let sweep = std::f32::consts::TAU * frac.clamp(0.0, 1.0);
                let start = -std::f32::consts::FRAC_PI_2;
                p.arc(cx, cy, r, start, sweep, 3.4, fade(ring_col, 0.25));
                p.arc(cx, cy, r, start, sweep, 1.85, ring_col);
            }
        } else {
            p.circle_stroke(cx, cy, r, 1.0, rgba(255, 255, 255, 28));
        }
        let ic_sz = (r * 1.04).round().clamp(10.0, 13.0);
        icons::draw(p, icon, cx - ic_sz * 0.5, cy - ic_sz * 0.5, ic_sz, icon_col);
    }

    pub(super) fn draw_minimized_idle_capsule(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let cy = h * 0.5;
        let anim_t = state.anim_time;
        let mood = state.mascot_mood();
        let left_x = if state.show_mascot {
            Self::draw_cyno_mascot(p, 11.5, cy + 0.4, 0.34, mood, anim_t, true);
            20.0
        } else {
            p.circle(10.0, cy, 2.8, tok::cyno_violet_hi());
            17.5
        };

        let badge_cx = (w - 9.0).max(left_x + 12.0);
        let max_txt_w = (badge_cx - 6.5 - left_x).max(20.0);
        let (frac_opt, arc_col) = Self::idle_arc_fraction_and_color(state);

        if let Some(ref notif) = state.notification {
            let mut buf = StackStr::<64>::new();
            let short_lbl: &str = match notif.kind {
                NotificationKind::SystemToast => {
                    let trimmed = notif.app_name.trim();
                    if !trimmed.is_empty() {
                        for ch in trimmed.chars().take(10) {
                            let _ = buf.write_char(ch);
                        }
                        buf.as_str()
                    } else {
                        "Toast"
                    }
                }
                NotificationKind::MemoryPurge => "Purged",
                NotificationKind::Capture => "Shot",
                NotificationKind::Pasted => "Pasted",
                NotificationKind::General => notif.title.as_str(),
            };
            let title = p.ellipsize_cow(short_lbl, 8.6, Weight::Bold, max_txt_w);
            p.text(&title, left_x, cy + 3.0, 8.6, Weight::Bold, tok::text1());
            if state.progress_arc_enabled {
                p.circle(badge_cx, cy, 3.8, fade(arc_col, 0.35));
            }
            p.circle(badge_cx, cy, 2.4, arc_col);
        } else if state.media.is_playing {
            let eq_w = if state.equalizer_enabled { 13.0 } else { 0.0 };
            let txt = p.ellipsize_cow(
                &state.media.title,
                8.5,
                Weight::Semibold,
                (max_txt_w - eq_w).max(16.0),
            );
            p.text(&txt, left_x, cy + 2.9, 8.5, Weight::Semibold, tok::text1());
            if state.equalizer_enabled {
                for i in 0..3 {
                    let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                    let bh = (2.8 + wave * 5.2).clamp(2.5, 8.0);
                    let bx = badge_cx - 14.0 + (i as f32 * 3.4);
                    p.rrect(bx, cy - bh * 0.5, 1.8, bh, 0.9, tok::cyno_violet_hi());
                }
            }
            p.circle(badge_cx, cy, 2.3, tok::cyno_violet_hi());
        } else {
            let mut buf = StackStr::<64>::new();
            let metric_str: &str = match state.idle_metric {
                IdleMetric::Ram => {
                    let _ = write!(buf, "{:.0}%", state.telemetry.memory_percent);
                    buf.as_str()
                }
                IdleMetric::NetSpeed => {
                    let _ = write!(buf, "↓{}", format_speed_short(state.telemetry.net_down_kbps));
                    buf.as_str()
                }
                IdleMetric::Cpu => {
                    let _ = write!(buf, "C{:.0}%", state.telemetry.cpu_usage);
                    buf.as_str()
                }
                IdleMetric::Clock => {
                    let _ = write!(buf, "{}", format_hhmm(local_now()));
                    buf.as_str()
                }
                IdleMetric::Minimal => "•••",
            };
            let txt = p.ellipsize_cow(metric_str, 8.8, Weight::Bold, max_txt_w);
            p.text_center(
                &txt,
                (left_x + badge_cx - 5.5) * 0.5,
                cy + 3.0,
                8.8,
                Weight::Bold,
                tok::text_soft(),
            );
            if state.progress_arc_enabled {
                p.circle_stroke(badge_cx, cy, 3.8, 1.1, rgba(255, 255, 255, 24));
                if let Some(frac) = frac_opt
                    && frac > 0.02
                {
                    p.arc(
                        badge_cx,
                        cy,
                        3.8,
                        -std::f32::consts::FRAC_PI_2,
                        std::f32::consts::TAU * frac,
                        1.4,
                        arc_col,
                    );
                }
            }
            p.circle(badge_cx, cy, 2.0, arc_col);
        }
    }

    pub(super) fn draw_idle_capsule(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let cy = h / 2.0;
        let anim_t = state.anim_time;
        let mood = state.mascot_mood();

        let text_x = if state.show_mascot {
            Self::draw_cyno_mascot(p, 24.5, cy + 0.5, 0.90, mood, anim_t, false);
            51.0
        } else {
            20.0
        };
        let badge_cx = w - 24.0;
        let max_txt_w = (badge_cx - 20.0 - text_x).max(60.0);
        let (frac_opt, arc_col) = Self::idle_arc_fraction_and_color(state);

        if let Some(ref notif) = state.notification {
            let title_raw: Cow<'_, str> =
                if notif.kind == NotificationKind::SystemToast && !notif.app_name.is_empty() {
                    Cow::Owned(format!("[{}] {}", notif.app_name, notif.title))
                } else {
                    Cow::Borrowed(notif.title.as_str())
                };
            let title = p.ellipsize_cow(&title_raw, 12.2, Weight::Bold, max_txt_w);
            p.text(&title, text_x, 18.5, 12.2, Weight::Bold, tok::text1());
            let sub_src =
                if notif.kind == NotificationKind::SystemToast && notif.subtitle.is_empty() {
                    "Windows Toast"
                } else {
                    notif.subtitle.as_str()
                };
            let sub = p.ellipsize_cow(sub_src, 10.5, Weight::Regular, max_txt_w);
            p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());

            let (icon, ic_col) = match notif.kind {
                NotificationKind::SystemToast => (Icon::Bell, tok::cyan_hi()),
                NotificationKind::MemoryPurge => (Icon::Zap, tok::cyno_mist()),
                NotificationKind::Capture => (Icon::Video, tok::cyno_violet_hi()),
                NotificationKind::Pasted | NotificationKind::General => {
                    (Icon::Check, tok::cyno_violet_hi())
                }
            };
            let badge_bg = p.surface_top_col();
            Self::draw_progress_arc_badge(
                p,
                badge_cx,
                cy,
                11.5,
                frac_opt,
                arc_col,
                badge_bg,
                icon,
                ic_col,
                state.progress_arc_enabled,
            );
        } else if state.media.is_playing {
            let show_eq = state.equalizer_enabled;
            let music_txt_w = if show_eq {
                (w - text_x - 68.0).max(55.0)
            } else {
                max_txt_w
            };
            let title = p.ellipsize_cow(&state.media.title, 12.5, Weight::Bold, music_txt_w);
            p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
            let sub = p.ellipsize_cow(&state.media.artist, 10.5, Weight::Regular, music_txt_w);
            p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());

            if show_eq {
                for i in 0..4 {
                    let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                    let bh = (4.0 + wave * 10.0).clamp(3.0, 14.0);
                    let bx = w - 58.0 + (i as f32 * 4.5);
                    let by = cy - bh / 2.0;
                    let c = if i % 2 == 0 {
                        tok::cyno_violet_hi()
                    } else {
                        tok::cyno_mist()
                    };
                    p.rrect(bx, by, 2.5, bh, 1.2, c);
                }
            }

            Self::draw_progress_arc_badge(
                p,
                badge_cx,
                cy,
                11.5,
                frac_opt,
                arc_col,
                tok::cyno_indigo(),
                Icon::Music,
                rgba(255, 255, 255, 245),
                state.progress_arc_enabled,
            );
        } else {
            match state.idle_metric {
                IdleMetric::Ram => {
                    p.text("CyNotch", text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let mut raw = StackStr::<64>::new();
                    let _ = write!(
                        raw,
                        "{:.0}% RAM • CPU {:.0}%",
                        state.telemetry.memory_percent, state.telemetry.cpu_usage
                    );
                    let sub = p.ellipsize_cow(raw.as_str(), 10.5, Weight::Regular, max_txt_w);
                    p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());
                    let badge_bg = p.surface_top_col();
                    Self::draw_progress_arc_badge(
                        p,
                        badge_cx,
                        cy,
                        11.5,
                        frac_opt,
                        arc_col,
                        badge_bg,
                        Icon::Zap,
                        tok::cyno_mist(),
                        state.progress_arc_enabled,
                    );
                }
                IdleMetric::NetSpeed => {
                    p.text("Network", text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let mut raw = StackStr::<64>::new();
                    let _ = write!(
                        raw,
                        "↓ {}   ↑ {}",
                        format_speed_short(state.telemetry.net_down_kbps),
                        format_speed_short(state.telemetry.net_up_kbps)
                    );
                    let sub = p.ellipsize_cow(raw.as_str(), 10.5, Weight::Semibold, max_txt_w);
                    p.text(&sub, text_x, 33.0, 10.5, Weight::Semibold, tok::cyan_hi());
                    let badge_bg = p.surface_top_col();
                    Self::draw_progress_arc_badge(
                        p,
                        badge_cx,
                        cy,
                        11.5,
                        frac_opt,
                        arc_col,
                        badge_bg,
                        Icon::Activity,
                        tok::cyan_hi(),
                        state.progress_arc_enabled,
                    );
                }
                IdleMetric::Cpu => {
                    let mut cpu_buf = StackStr::<32>::new();
                    let _ = write!(cpu_buf, "CPU {:.0}%", state.telemetry.cpu_usage);
                    let title = p.ellipsize_cow(cpu_buf.as_str(), 12.5, Weight::Bold, max_txt_w);
                    p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let mut ram_buf = StackStr::<64>::new();
                    let _ = write!(
                        ram_buf,
                        "RAM {:.0}% ({:.1}G)",
                        state.telemetry.memory_percent, state.telemetry.memory_used_gb
                    );
                    let sub = p.ellipsize_cow(ram_buf.as_str(), 10.5, Weight::Regular, max_txt_w);
                    p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());
                    let badge_bg = p.surface_top_col();
                    Self::draw_progress_arc_badge(
                        p,
                        badge_cx,
                        cy,
                        11.5,
                        frac_opt,
                        arc_col,
                        badge_bg,
                        Icon::Activity,
                        tok::cyno_violet_hi(),
                        state.progress_arc_enabled,
                    );
                }
                IdleMetric::Clock => {
                    let now = local_now();
                    let mut clk_buf = StackStr::<48>::new();
                    let _ = write!(clk_buf, "{:02}:{:02} • {:02}/{:02}", now.hour, now.minute, now.month, now.day);
                    let title = p.ellipsize_cow(clk_buf.as_str(), 12.5, Weight::Bold, max_txt_w);
                    p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let mut sub_buf = StackStr::<64>::new();
                    let _ = write!(
                        sub_buf,
                        "RAM {:.0}% • CPU {:.0}%",
                        state.telemetry.memory_percent, state.telemetry.cpu_usage
                    );
                    let sub = p.ellipsize_cow(sub_buf.as_str(), 10.5, Weight::Regular, max_txt_w);
                    p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());
                    let badge_bg = p.surface_top_col();
                    Self::draw_progress_arc_badge(
                        p,
                        badge_cx,
                        cy,
                        11.5,
                        frac_opt,
                        arc_col,
                        badge_bg,
                        Icon::Timer,
                        tok::cyno_mist(),
                        state.progress_arc_enabled,
                    );
                }
                IdleMetric::Minimal => {
                    p.text(
                        "CyNotch",
                        text_x,
                        cy + 4.2,
                        12.5,
                        Weight::Bold,
                        tok::text1(),
                    );
                    let badge_bg = p.surface_top_col();
                    Self::draw_progress_arc_badge(
                        p,
                        badge_cx,
                        cy,
                        11.5,
                        None,
                        rgba(255, 255, 255, 36),
                        badge_bg,
                        Icon::Zap,
                        tok::cyno_mist(),
                        false,
                    );
                }
            }

            // First-run subtle onboarding hint pill (`Ctrl+Shift+C`) until user presses hotkey or expands 3x
            if !state.seen_hotkey_hint {
                let title_w = match state.idle_metric {
                    IdleMetric::Ram | IdleMetric::Minimal => p.measure("CyNotch", 12.5, Weight::Bold),
                    IdleMetric::NetSpeed => p.measure("Network", 12.5, Weight::Bold),
                    IdleMetric::Cpu => {
                        let mut b = StackStr::<32>::new();
                        let _ = write!(b, "CPU {:.0}%", state.telemetry.cpu_usage);
                        p.measure(b.as_str(), 12.5, Weight::Bold)
                    }
                    IdleMetric::Clock => p.measure("00:00 • 00/00", 12.5, Weight::Bold),
                };
                let hint_txt = "Ctrl+Shift+C";
                let pill_w = (p.measure(hint_txt, 8.2, Weight::Semibold) + 12.0).clamp(58.0, 86.0);
                let pill_h = 14.0;
                let pill_x = badge_cx - 18.0 - pill_w;
                let pill_y = if state.idle_metric == IdleMetric::Minimal {
                    cy - pill_h * 0.5
                } else {
                    7.5
                };
                if pill_x >= text_x + title_w + 10.0 {
                    p.rrect(pill_x, pill_y, pill_w, pill_h, 5.0, rgba(168, 85, 247, 34));
                    p.rrect_stroke(
                        pill_x,
                        pill_y,
                        pill_w,
                        pill_h,
                        5.0,
                        0.8,
                        rgba(216, 180, 254, 78),
                    );
                    p.text_center_fit(
                        hint_txt,
                        pill_x + pill_w * 0.5,
                        pill_y + 10.2,
                        8.2,
                        Weight::Semibold,
                        tok::cyno_mist(),
                        pill_w - 6.0,
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // VERTICAL IDLE CAPSULE (52 × 168/204/248 for Left / Right Edges)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_expanded(p: &mut PainterD2D, state: &AppState, w: f32, h: f32, settled: bool) {
        if state.notification.is_none() {
            Self::draw_expanded_header(p, state, w);
        }
        let prev_ox = p.origin_x;
        let prev_alpha = p.global_alpha;
        if state.are_animations_active() && state.anim_mode_t < 0.995 {
            let me = crate::models::ease_out_cubic(state.anim_mode_t);
            p.origin_x = prev_ox + (1.0 - me) * 12.0 * state.anim_mode_dir * p.s;
            p.global_alpha = prev_alpha * (0.18 + 0.82 * me);
        }
        Self::draw_expanded_body(p, state, w, h, settled);
        p.origin_x = prev_ox;
        p.global_alpha = prev_alpha;
    }

    fn draw_expanded_header(p: &mut PainterD2D, state: &AppState, w: f32) {
        let dock_x = 20.0;
        let dock_y = 14.0;
        let dock_h = 34.0;

        let dock_modes = state.visible_dock_modes();
        let (dock_w, item_w) = AppState::dock_metrics(dock_modes.len());

        let well_col = if state.is_frosted_active() {
            tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
        } else {
            tok::surface_well()
        };
        p.rrect(dock_x, dock_y, dock_w, dock_h, 11.5, well_col);
        p.rrect_bevel_stroke(
            dock_x,
            dock_y,
            dock_w,
            dock_h,
            11.5,
            1.0,
            rgba(0, 0, 0, 120),
            rgba(255, 255, 255, 26),
        );

        let active_visible = dock_modes.contains(&state.mode);
        let (target_dock_x, target_dock_w) = state.active_dock_target();
        let (pill_x, pill_w) = if !state.micro_interactions {
            (target_dock_x, target_dock_w)
        } else {
            (
                state.anim_dock_x.clamp(
                    dock_x + 4.0,
                    (dock_x + dock_w - 4.0 - item_w).max(dock_x + 4.0),
                ),
                state.anim_dock_w.max(20.0),
            )
        };

        if active_visible {
            let active_ht = if state.micro_interactions {
                dock_modes
                    .iter()
                    .position(|&m| m == state.mode)
                    .and_then(|idx| state.hover_slots.get(idx).copied())
                    .unwrap_or(0.0)
                    * 0.85
            } else if p.hot(pill_x, dock_y + 4.0, pill_w, 26.0) {
                0.85
            } else {
                0.0
            };
            p.luminous_pill_button_t(
                pill_x,
                dock_y + 4.0,
                pill_w,
                26.0,
                8.0,
                tok::cyno_lavender_white(),
                tok::cyno_mist(),
                tok::cyno_violet_hi(),
                active_ht,
            );
        }

        for (i, mode) in dock_modes.iter().enumerate() {
            let icon = match mode {
                NotchMode::Home => Icon::Home,
                NotchMode::Music => Icon::Music,
                NotchMode::Volume => Icon::Volume,
                NotchMode::MemReduct => Icon::Wave,
                NotchMode::Tasks => Icon::List,
                NotchMode::Shelf => Icon::Clipboard,
                NotchMode::Capture => Icon::Video,
                NotchMode::Telemetry => Icon::Display,
                NotchMode::Settings => Icon::Settings,
            };
            let ix = dock_x + 4.0 + (i as f32 * item_w);
            let iy = dock_y + 4.0;
            let ht = p.hot_t(ix, iy, item_w, 26.0);
            let under_pill = if active_visible {
                (1.0 - ((pill_x - ix).abs() / item_w.max(1.0))).clamp(0.0, 1.0)
            } else {
                0.0
            };

            if under_pill < 0.5 && ht > 0.02 {
                p.rrect(ix, iy, item_w, 26.0, 8.0, fade(rgba(255, 255, 255, 22), ht));
            }
            let inactive_col = lerp_color(rgba(148, 163, 184, 165), rgba(255, 255, 255, 245), ht);
            let col = lerp_color(inactive_col, rgba(17, 17, 23, 255), under_pill);
            icons::draw(p, icon, ix + (item_w - 14.0) / 2.0, iy + 6.0, 14.0, col);

            p.hit(ix, iy, item_w, 26.0, Action::SetMode(*mode));
        }

        // Right Actions: [Lyrics Pill] [Pin Lock] [Settings Gear] [Grip]
        let right_x = w - 20.0;
        let grip_x = right_x - 18.0;
        let grip_ht = p.hot_t(grip_x - 4.0, dock_y, 26.0, dock_h);
        let grip_col = lerp_color(rgba(148, 163, 184, 115), tok::text1(), grip_ht);
        icons::draw(p, Icon::Grip, grip_x, dock_y + 9.0, 16.0, grip_col);
        p.hit(grip_x - 4.0, dock_y, 26.0, dock_h, Action::StartGripDrag);

        let gear_x = grip_x - 38.0;
        let is_settings_active = state.settings_open || state.mode == NotchMode::Settings;
        let gear_ht = p.hot_t(gear_x, dock_y + 3.0, 28.0, 28.0);
        if is_settings_active {
            p.luminous_pill_button_t(
                gear_x,
                dock_y + 3.0,
                28.0,
                28.0,
                14.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                gear_ht,
            );
            icons::draw(
                p,
                Icon::Settings,
                gear_x + 7.0,
                dock_y + 10.0,
                14.0,
                rgba(255, 255, 255, 255),
            );
        } else {
            p.tactile_surface_pill_t(gear_x, dock_y + 3.0, 28.0, 28.0, 14.0, gear_ht, None);
            let gc = lerp_color(tok::text2(), tok::text1(), gear_ht);
            icons::draw(p, Icon::Settings, gear_x + 7.0, dock_y + 10.0, 14.0, gc);
        }
        p.hit(gear_x, dock_y + 3.0, 28.0, 28.0, Action::OpenSettings);

        let pin_x = gear_x - 34.0;
        let pin_ht = p.hot_t(pin_x, dock_y + 3.0, 28.0, 28.0);
        if state.is_pinned {
            p.luminous_pill_button_t(
                pin_x,
                dock_y + 3.0,
                28.0,
                28.0,
                14.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                pin_ht,
            );
            icons::draw(
                p,
                Icon::Lock,
                pin_x + 7.0,
                dock_y + 10.0,
                14.0,
                rgba(255, 255, 255, 255),
            );
        } else {
            p.tactile_surface_pill_t(pin_x, dock_y + 3.0, 28.0, 28.0, 14.0, pin_ht, None);
            let pc = lerp_color(tok::text2(), tok::text1(), pin_ht);
            icons::draw(p, Icon::Lock, pin_x + 7.0, dock_y + 10.0, 14.0, pc);
        }
        p.hit(pin_x, dock_y + 3.0, 28.0, 28.0, Action::TogglePin);

        let bell_x = pin_x - 34.0;
        let bell_ht = p.hot_t(bell_x, dock_y + 3.0, 28.0, 28.0);
        if state.show_notif_history {
            p.luminous_pill_button_t(
                bell_x,
                dock_y + 3.0,
                28.0,
                28.0,
                14.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                bell_ht,
            );
            icons::draw(
                p,
                Icon::Bell,
                bell_x + 7.0,
                dock_y + 10.0,
                14.0,
                rgba(255, 255, 255, 255),
            );
        } else {
            p.tactile_surface_pill_t(bell_x, dock_y + 3.0, 28.0, 28.0, 14.0, bell_ht, None);
            let bc = lerp_color(tok::text2(), tok::text1(), bell_ht);
            icons::draw(p, Icon::Bell, bell_x + 7.0, dock_y + 10.0, 14.0, bc);
        }
        if !state.notif_history.is_empty() && !state.show_notif_history {
            p.circle(bell_x + 21.0, dock_y + 9.0, 2.7, tok::cyno_orchid());
        }
        p.hit(bell_x, dock_y + 3.0, 28.0, 28.0, Action::ToggleNotifHistory);

        // Mode Header Title + Lyrics Pill
        let title_x = dock_x + dock_w + 14.0;
        let title_y = dock_y + 21.5;
        let max_title_w = (bell_x - title_x - 10.0).max(0.0);
        let draw_title = |p: &mut PainterD2D, txt: &str| {
            if max_title_w >= 28.0 {
                let s = p.ellipsize(txt, 13.0, Weight::Bold, max_title_w);
                p.text(&s, title_x, title_y, 13.0, Weight::Bold, tok::text1());
            }
        };

        if state.show_notif_history {
            draw_title(p, "Notifications");
        } else {
            match state.mode {
                NotchMode::Home => draw_title(p, "Command Hub"),
                NotchMode::Music => {
                    let tab_y = dock_y + 4.0;
                    let lyrics_pill_w = 58.0;
                    let lyrics_pill_x = bell_x - lyrics_pill_w - 8.0;
                    let music_max = (lyrics_pill_x - title_x - 8.0).max(0.0);
                    if music_max >= 26.0 {
                        let s = p.ellipsize("Music", 13.0, Weight::Bold, music_max);
                        p.text(&s, title_x, title_y, 13.0, Weight::Bold, tok::text1());
                    }

                    let lyrics_ht = p.hot_t(lyrics_pill_x, tab_y, lyrics_pill_w, 26.0);
                    if state.show_lyrics {
                        p.luminous_pill_button_t(
                            lyrics_pill_x,
                            tab_y,
                            lyrics_pill_w,
                            26.0,
                            13.0,
                            tok::cyno_indigo(),
                            tok::cyno_violet(),
                            tok::cyno_orchid(),
                            lyrics_ht,
                        );
                        p.text_center_shadow(
                            "Lyrics",
                            lyrics_pill_x + lyrics_pill_w / 2.0,
                            tab_y + 17.0,
                            11.0,
                            Weight::Bold,
                            tok::text1(),
                        );
                    } else {
                        p.tactile_surface_pill_t(
                            lyrics_pill_x,
                            tab_y,
                            lyrics_pill_w,
                            26.0,
                            13.0,
                            lyrics_ht,
                            None,
                        );
                        p.text_center(
                            "Lyrics",
                            lyrics_pill_x + lyrics_pill_w / 2.0,
                            tab_y + 17.0,
                            11.0,
                            Weight::Semibold,
                            tok::text_soft(),
                        );
                    }
                    p.hit(
                        lyrics_pill_x,
                        tab_y,
                        lyrics_pill_w,
                        26.0,
                        Action::ToggleLyrics,
                    );
                }
                NotchMode::Volume => draw_title(p, "Volume Master"),
                NotchMode::MemReduct => draw_title(p, "Mem Reduct"),
                NotchMode::Tasks => draw_title(p, "Tasks"),
                NotchMode::Shelf => draw_title(p, "Quick Shelf"),
                NotchMode::Capture => draw_title(p, "Quick Capture"),
                NotchMode::Telemetry => draw_title(p, "Screen Time"),
                NotchMode::Settings => draw_title(p, "Quick Settings"),
            }
        }
    }

    fn draw_expanded_body(p: &mut PainterD2D, state: &AppState, w: f32, h: f32, _settled: bool) {
        let card_y = 60.0;
        if state.show_notif_history {
            Self::draw_notif_history_view(p, state, w, h, card_y);
        } else {
            match state.mode {
                NotchMode::Home => {
                    Self::draw_home_view(p, state, w, card_y);
                    Self::draw_expanded_anim_overlay(p, state);
                }
                NotchMode::Music => {
                    Self::draw_music_view(p, state, w, card_y);
                    Self::draw_expanded_anim_overlay(p, state);
                }
                NotchMode::Volume => Self::draw_volume_view(p, state, w, card_y),
                NotchMode::MemReduct => Self::draw_memreduct_view(p, state, w, card_y),
                NotchMode::Tasks => Self::draw_tasks_view(p, state, w, card_y),
                NotchMode::Shelf => Self::draw_shelf_view(p, state, w, card_y),
                NotchMode::Capture => Self::draw_capture_view(p, state, w, card_y),
                NotchMode::Telemetry => Self::draw_screentime_view(p, state, w, card_y),
                NotchMode::Settings => Self::draw_settings_view(p, state, w, card_y),
            }
        }
        Self::draw_expanded_toast_overlay(p, state, w);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 0. HOME COMMAND HUB (Cyno Companion + Hydration OR Net/Focus + Media/Tasks)
    // ─────────────────────────────────────────────────────────────────────────────
}
