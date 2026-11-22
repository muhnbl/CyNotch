mod mascot;
mod notifications;
mod views;

use crate::font_d2d::{DWriteTextCache, Weight};
use crate::icons::{self, Icon};
use crate::models::{
    Action, AppState, IdleMetric, LyricSize, LyricStyle, MascotMood, NotchMode, NotchPlacement,
    NotificationKind, ShelfKind, ToggleId, TypeScale,
};
use crate::painter_d2d::Hit;
use crate::painter_d2d::{BlendMode, ButtonVariant, Color, D2DResourceCache, PainterD2D, StackStr, fade, lerp_color, rgba, tok};
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

                // 1. Borderless Volumetric Obsidian Shell (Box 1 only in Music mode so Box 2 & 3 float independently!)
                let shell_h = if state.is_hovered && state.mode == NotchMode::Music && !state.show_notif_history {
                    (176.0 * state.expanded_scale()).min(h)
                } else {
                    h
                };
                let (shell_top, shell_bot) = if frosted {
                    (tok::obsidian_shell_frosted(), tok::obsidian_deep_frosted())
                } else {
                    (tok::obsidian_shell(), tok::obsidian_deep())
                };
                p.rrect_corners4_vertical_gradient(0.0, 0.0, w, shell_h, (r_tl, r_tr, r_br, r_bl), shell_top, shell_bot);

                // 2. Internal Atmospheric Aurora Glows
                let glow_scale = if frosted { 0.38 } else { 1.0 };
                if t > 0.05 {
                    if p.prepare_corners4_clip(0.0, 0.0, w, shell_h, r_tl, r_tr, r_br, r_bl) {
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
                            shell_h - 10.0,
                            116.0,
                            fade(rgba(124, 58, 237, 48), t * glow_scale),
                            BlendMode::SourceOver,
                            true,
                        );
                        p.elliptical_glow(
                            w * 0.5,
                            shell_h + 6.0,
                            w * 0.56,
                            (shell_h * 0.36).max(24.0),
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

                let show_idle_content = !settled_expanded && t < 0.28;
                if show_idle_content {
                    let fade_out = ((0.28 - t) / 0.28).clamp(0.0, 1.0);
                    if fade_out > 0.02 {
                        p.global_alpha = fade_out;
                        if state.notification.is_some() && w >= 220.0 && h >= 48.0 {
                            let target_bs = state.banner_scale();
                            let (base_bw, base_bh) = (AppState::BANNER_WIDTH, AppState::BANNER_HEIGHT);
                            let notif_alpha = if state.are_animations_active() {
                                crate::models::ease_out_cubic(state.anim_notif_t)
                            } else {
                                1.0
                            };
                            p.global_alpha = fade_out * notif_alpha.clamp(0.0, 1.0);
                            let bs = (w / base_bw).min(h / base_bh).min(target_bs).max(0.65);
                            p.s = bs;
                            Self::draw_notification_banner(&mut p, state, w / bs, h / bs);
                        } else if state.minimized_island || w < 48.0 || h < 21.0 {
                            p.type_scale = TypeScale::Compact;
                            Self::draw_minimized_idle_capsule(&mut p, state, w, h);
                        } else {
                            let is = state.type_scale.idle_ui_scale();
                            p.s = is;
                            Self::draw_idle_capsule(&mut p, state, w / is, h / is);
                        }
                    }
                } else {
                    let fade_in = if settled_expanded { 1.0 } else { ((t - 0.26) / 0.74).clamp(0.0, 1.0) };
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
                        Self::draw_expanded(&mut p, state, base_w, base_h, settled_expanded);
                    }
                }

                // 3. Opsi A (Wolf Eyes Micro-Reaction Only): capsule shell stays clean & calm (no rim glow or bounce).
                p.s = 1.0;
                p.origin_x = ox;
                p.origin_y = oy;
                p.global_alpha = 1.0;
                Self::draw_clipboard_gold_pulse(&mut p, state, w, h, (r_tl, r_tr, r_br, r_bl));
            }

            pub(super) fn draw_clipboard_gold_pulse(
                _p: &mut PainterD2D,
                _state: &AppState,
                _w: f32,
                _h: f32,
                _radii: (f32, f32, f32, f32),
            ) {
                // Opsi A: Copy/Paste & Snipping Tool reactions are rendered directly via
                // the Purple Wolf Eyes predator squint/flash inside the idle capsule.
            }

            pub fn compute_origin(state: &AppState, canvas_w: f32, canvas_h: f32) -> (f32, f32) {
                let ax = state.anim_anchor_x.clamp(0.0, 1.0);
                let ay = state.anim_anchor_y.clamp(0.0, 1.0);
                match state.placement {
                    NotchPlacement::Top => (((canvas_w - state.current_width) * ax).max(0.0), 0.0),
                    NotchPlacement::Left => (0.0, ((canvas_h - state.current_height) * ay).max(0.0)),
                    NotchPlacement::Right => (
                        (canvas_w - state.current_width).max(0.0),
                        ((canvas_h - state.current_height) * ay).max(0.0),
                    ),
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
        let base = if let Some(ref notif) = state.notification {
            let col = match notif.kind {
                NotificationKind::SystemToast => tok::cyan_hi(),
                NotificationKind::MemoryPurge => tok::cyno_mist(),
                NotificationKind::Capture
                | NotificationKind::Pasted
                | NotificationKind::General => tok::cyno_violet_hi(),
            };
            (Some(1.0), col)
        } else if state.media.is_playing && state.media.duration_ms > 0 {
            let frac =
                (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0);
            (Some(frac), tok::cyno_violet_hi())
        } else {
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
        };

        // During Opsi A Skenario 2 Pulse, surge the idle progress ring & Minimized Idle Island
        // indicator jewel toward Brushed Metallic Champagne Gold.
        if let Some((pulse_i, _u)) =
            state.shelf_fx.clipboard_pulse_curves(std::time::Instant::now())
            && pulse_i > 0.02
        {
            let base_f = base.0.unwrap_or(0.25);
            let surged_f = (base_f + (1.0 - base_f) * pulse_i).clamp(0.08, 1.0);
            let surged_col = lerp_color(base.1, tok::gold_champagne(), (pulse_i * 1.15).clamp(0.0, 1.0));
            return (Some(surged_f), surged_col);
        }
        base
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
        let pulse_i = state
            .shelf_fx
            .clipboard_pulse_curves(std::time::Instant::now())
            .map(|(i, _)| i)
            .unwrap_or(0.0);
        let is_minimal_idle = state.notification.is_none()
            && !state.media.is_playing
            && state.idle_metric == IdleMetric::Minimal;
        if is_minimal_idle {
            Self::draw_wolf_eyes(p, w * 0.5, cy, 0.46, anim_t, 1.0, pulse_i, 0.0, 0.0);
            return;
        }
        let left_x = 10.0;

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
            let txt = p.ellipsize_cow(
                &state.media.title,
                8.6,
                Weight::Semibold,
                max_txt_w.max(16.0),
            );
            p.text(&txt, left_x, cy + 2.9, 8.6, Weight::Semibold, tok::text1());
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
                IdleMetric::Minimal => "",
            };
            if !metric_str.is_empty() {
                let txt = p.ellipsize_cow(metric_str, 8.8, Weight::Bold, max_txt_w);
                p.text_center(
                    &txt,
                    (left_x + badge_cx - 5.5) * 0.5,
                    cy + 3.0,
                    8.8,
                    Weight::Bold,
                    tok::text_soft(),
                );
            }
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

    /// Draws single-line `Judul Lagu - Nama Artis (75% opacity)` with a pure alpha fade-out
    /// at the trailing edge so the right/bottom Music icon appears to float with depth over the text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_music_inline_text(
        p: &mut PainterD2D,
        title: &str,
        artist: &str,
        x: f32,
        baseline: f32,
        clip_w: f32,
        clip_h: f32,
        alpha: f32,
    ) {
        if clip_w <= 12.0 || alpha <= 0.02 || title.is_empty() {
            return;
        }
        let fade_w = (clip_w * 0.22).clamp(16.0, 34.0);
        let masked = p.push_fade_mask(x, 0.0, clip_w, clip_h, fade_w, false, false);
        let title_w = p.text(title, x, baseline, 12.8, Weight::Bold, fade(tok::text1(), alpha));
        let artist_trimmed = artist.trim();
        if !artist_trimmed.is_empty() && title_w + 10.0 < clip_w {
            let mut sep = StackStr::<128>::new();
            let _ = write!(sep, " - {}", artist_trimmed);
            p.text(
                sep.as_str(),
                x + title_w,
                baseline,
                12.2,
                Weight::Regular,
                fade(tok::text1(), alpha * 0.75),
            );
        }
        if masked {
            p.pop_clip();
        }
    }

    pub(super) fn draw_idle_capsule(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let cy = h / 2.0;
        let anim_t = state.anim_time;
        let pulse_i = state
            .shelf_fx
            .clipboard_pulse_curves(std::time::Instant::now())
            .map(|(i, _)| i)
            .unwrap_or(0.0);

        // Staged gentle state-change transition (`0.0 -> 1.0` over ~650ms)
        let cap_t = if state.are_animations_active() {
            state.anim_capsule_t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let ct = crate::models::ease_out_cubic(cap_t);
        let in_dx = (1.0 - ct) * 7.5;
        let in_alpha = ct.clamp(0.0, 1.0);

        let is_stealth_target = state.notification.is_none()
            && !state.media.is_playing
            && state.idle_metric == IdleMetric::Minimal;

        // Active states (Music / Notification / Metrics) have NO mascot — clean 14px left padding.
        let text_x = 14.0;
        let badge_r = (h * 0.36).clamp(9.2, 11.2);
        let badge_cx = (w - badge_r - 5.5).max(text_x + 4.0);
        let avail_txt_w = badge_cx - badge_r - 3.0 - text_x;
        let max_txt_w = avail_txt_w.max(24.0);
        let (frac_opt, arc_col) = Self::idle_arc_fraction_and_color(state);

        // Outgoing content choreography (`0.0..0.42` of 420ms = ~176ms):
        // - Leaving Stealth Idle: Wolf Eyes blink closed & fade out gently.
        // - Leaving Music / Switching Song: Previous title - artist rolls DOWNWARD (+15px) & fades out.
        let out_phase = (cap_t / 0.42).clamp(0.0, 1.0);
        if out_phase < 1.0 {
            let smooth = out_phase * out_phase * (3.0 - 2.0 * out_phase);
            let out_a = (1.0 - smooth).clamp(0.0, 1.0);
            let out_a2 = out_a * out_a;
            if !is_stealth_target && state.prev_capsule_title.is_empty() {
                Self::draw_wolf_eyes(p, w * 0.5, cy, 0.90, anim_t, out_a, pulse_i, 0.0, 0.0);
            } else if !state.prev_capsule_title.is_empty() && avail_txt_w >= 28.0 {
                let roll_down_y = 15.0 * smooth;
                if state.media.is_playing || is_stealth_target {
                    Self::draw_music_inline_text(
                        p,
                        &state.prev_capsule_title,
                        &state.prev_capsule_sub,
                        text_x,
                        cy + 4.3 + roll_down_y,
                        avail_txt_w.max(24.0),
                        h,
                        out_a2,
                    );
                } else {
                    let old_t = p.ellipsize_cow(
                        &state.prev_capsule_title,
                        12.5,
                        Weight::Bold,
                        max_txt_w,
                    );
                    p.text(
                        &old_t,
                        text_x,
                        cy + 4.2 + roll_down_y,
                        12.5,
                        Weight::Bold,
                        fade(tok::text1(), out_a2 * 0.72),
                    );
                }
            }
        }

        if let Some(ref notif) = state.notification {
            if avail_txt_w >= 24.0 {
                let title_raw: Cow<'_, str> =
                    if notif.kind == NotificationKind::SystemToast && !notif.app_name.is_empty() {
                        Cow::Owned(format!("[{}] {}", notif.app_name, notif.title))
                    } else {
                        Cow::Borrowed(notif.title.as_str())
                    };
                let title = p.ellipsize_cow(&title_raw, 12.2, Weight::Bold, max_txt_w);
                p.text(
                    &title,
                    text_x + in_dx,
                    18.5,
                    12.2,
                    Weight::Bold,
                    fade(tok::text1(), in_alpha),
                );
                let sub_src =
                    if notif.kind == NotificationKind::SystemToast && notif.subtitle.is_empty() {
                        "Windows Toast"
                    } else {
                        notif.subtitle.as_str()
                    };
                let sub = p.ellipsize_cow(sub_src, 10.5, Weight::Regular, max_txt_w);
                p.text(
                    &sub,
                    text_x + in_dx,
                    33.0,
                    10.5,
                    Weight::Regular,
                    fade(tok::text2(), in_alpha),
                );
            }

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
                badge_r,
                frac_opt,
                arc_col,
                badge_bg,
                icon,
                ic_col,
                state.progress_arc_enabled,
            );
        } else if state.media.is_playing {
            // Music State: Roll-in from top (`0.30..1.00`, 30% overlap), `Judul - Artis (75%)` with right alpha fade-out
            let roll_in_u = if cap_t < 0.99 {
                ((cap_t - 0.30) / 0.70).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let roll_e = crate::models::ease_out_cubic(roll_in_u);
            let roll_in_y = -15.0 * (1.0 - roll_e);
            if avail_txt_w >= 24.0 && roll_e > 0.02 {
                Self::draw_music_inline_text(
                    p,
                    &state.media.title,
                    &state.media.artist,
                    text_x,
                    cy + 4.3 + roll_in_y,
                    avail_txt_w.max(24.0),
                    h,
                    roll_e,
                );
            }

            Self::draw_progress_arc_badge(
                p,
                badge_cx,
                cy,
                badge_r,
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
                    if avail_txt_w >= 24.0 && in_alpha > 0.02 {
                        let mut ram_title = StackStr::<32>::new();
                        let _ = write!(ram_title, "RAM {:.0}%", state.telemetry.memory_percent);
                        let title =
                            p.ellipsize_cow(ram_title.as_str(), 12.5, Weight::Bold, max_txt_w);
                        p.text(
                            &title,
                            text_x + in_dx,
                            18.5,
                            12.5,
                            Weight::Bold,
                            fade(tok::text1(), in_alpha),
                        );
                        let mut raw = StackStr::<64>::new();
                        let _ = write!(
                            raw,
                            "{:.1} GB • CPU {:.0}%",
                            state.telemetry.memory_used_gb, state.telemetry.cpu_usage
                        );
                        let sub = p.ellipsize_cow(raw.as_str(), 10.5, Weight::Regular, max_txt_w);
                        p.text(
                            &sub,
                            text_x + in_dx,
                            33.0,
                            10.5,
                            Weight::Regular,
                            fade(tok::text2(), in_alpha),
                        );
                    }
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
                    if avail_txt_w >= 24.0 && in_alpha > 0.02 {
                        p.text(
                            "Network",
                            text_x + in_dx,
                            18.5,
                            12.5,
                            Weight::Bold,
                            fade(tok::text1(), in_alpha),
                        );
                        let mut raw = StackStr::<64>::new();
                        let _ = write!(
                            raw,
                            "↓ {}   ↑ {}",
                            format_speed_short(state.telemetry.net_down_kbps),
                            format_speed_short(state.telemetry.net_up_kbps)
                        );
                        let sub = p.ellipsize_cow(raw.as_str(), 10.5, Weight::Semibold, max_txt_w);
                        p.text(
                            &sub,
                            text_x + in_dx,
                            33.0,
                            10.5,
                            Weight::Semibold,
                            fade(tok::cyan_hi(), in_alpha),
                        );
                    }
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
                    if avail_txt_w >= 24.0 && in_alpha > 0.02 {
                        let mut cpu_buf = StackStr::<32>::new();
                        let _ = write!(cpu_buf, "CPU {:.0}%", state.telemetry.cpu_usage);
                        let title =
                            p.ellipsize_cow(cpu_buf.as_str(), 12.5, Weight::Bold, max_txt_w);
                        p.text(
                            &title,
                            text_x + in_dx,
                            18.5,
                            12.5,
                            Weight::Bold,
                            fade(tok::text1(), in_alpha),
                        );
                        let mut ram_buf = StackStr::<64>::new();
                        let _ = write!(
                            ram_buf,
                            "RAM {:.0}% ({:.1}G)",
                            state.telemetry.memory_percent, state.telemetry.memory_used_gb
                        );
                        let sub =
                            p.ellipsize_cow(ram_buf.as_str(), 10.5, Weight::Regular, max_txt_w);
                        p.text(
                            &sub,
                            text_x + in_dx,
                            33.0,
                            10.5,
                            Weight::Regular,
                            fade(tok::text2(), in_alpha),
                        );
                    }
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
                    if avail_txt_w >= 24.0 && in_alpha > 0.02 {
                        let now = local_now();
                        let mut clk_buf = StackStr::<48>::new();
                        let _ = write!(
                            clk_buf,
                            "{:02}:{:02} • {:02}/{:02}",
                            now.hour, now.minute, now.month, now.day
                        );
                        let title =
                            p.ellipsize_cow(clk_buf.as_str(), 12.5, Weight::Bold, max_txt_w);
                        p.text(
                            &title,
                            text_x + in_dx,
                            18.5,
                            12.5,
                            Weight::Bold,
                            fade(tok::text1(), in_alpha),
                        );
                        let mut sub_buf = StackStr::<64>::new();
                        let _ = write!(
                            sub_buf,
                            "RAM {:.0}% • CPU {:.0}%",
                            state.telemetry.memory_percent, state.telemetry.cpu_usage
                        );
                        let sub =
                            p.ellipsize_cow(sub_buf.as_str(), 10.5, Weight::Regular, max_txt_w);
                        p.text(
                            &sub,
                            text_x + in_dx,
                            33.0,
                            10.5,
                            Weight::Regular,
                            fade(tok::text2(), in_alpha),
                        );
                    }
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
                    // Pure Stealth Mode: 2 Glowing Purple Wolf Eyes appear gently after outgoing text rolls down.
                    let eye_open = if cap_t < 0.99 && !state.prev_capsule_title.is_empty() {
                        crate::models::ease_out_cubic(((cap_t - 0.30) / 0.70).clamp(0.0, 1.0))
                    } else {
                        ct
                    };
                    Self::draw_wolf_eyes(p, w * 0.5, cy, 0.90, anim_t, eye_open, pulse_i, 0.0, 0.0);
                }
            }

            // First-run subtle onboarding hint pill (`Ctrl+Shift+C`) until user presses hotkey or expands 3x
            if !state.seen_hotkey_hint && state.idle_metric != IdleMetric::Minimal {
                let title_w = match state.idle_metric {
                    IdleMetric::Ram => p.measure("RAM 100%", 12.5, Weight::Bold),
                    IdleMetric::Minimal => 0.0,
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
                let pill_y = 7.5;
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
        Self::draw_expanded_header(p, state, w);
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

        let well_col = p.well_col();
        // Borderless recessed glass capsule for Mode Dock (DESIGN_SYSTEM.md §2 Molecule #2)
        p.rrect(dock_x, dock_y, dock_w, dock_h, 17.0, well_col);

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
            p.button_pill_variant(
                pill_x,
                dock_y + 4.0,
                pill_w,
                26.0,
                13.0,
                active_ht,
                ButtonVariant::PrimaryFill,
            );
        }

        for (i, mode) in dock_modes.iter().enumerate() {
            let icon = match mode {
                NotchMode::Home => Icon::Home,
                NotchMode::Music => Icon::Music,
                NotchMode::Volume => Icon::Volume,
                NotchMode::MemReduct => Icon::Wave,
                NotchMode::Shelf => Icon::Clipboard,
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
                p.button_pill_variant(ix, iy, item_w, 26.0, 13.0, ht, ButtonVariant::TextOnly);
            }
            let inactive_col = lerp_color(tok::text3(), tok::text1(), ht);
            let col = lerp_color(inactive_col, tok::text1(), under_pill);
            icons::draw(p, icon, ix + (item_w - 14.0) / 2.0, iy + 6.0, 14.0, col);

            p.hit(ix, iy, item_w, 26.0, Action::SetMode(*mode));
        }

        // Right Actions: [Lyrics Pill] [Bell] [Pin Lock] [Settings Gear]
        let right_x = w - 20.0;
        let gear_x = right_x - 28.0;
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
        if state.is_notification_stashed() {
            // Auto-Stash active notification pip: amber #FBBF24 with soft halo (click Bell to peek/restore)
            p.radial_glow(
                bell_x + 22.5,
                dock_y + 7.5,
                7.0,
                fade(tok::amber_glow(), 0.48),
                BlendMode::Plus,
                false,
            );
            p.circle(bell_x + 22.5, dock_y + 7.5, 3.2, tok::amber_glow());
        } else if !state.notif_history.is_empty() && !state.show_notif_history {
            p.circle(bell_x + 21.0, dock_y + 9.0, 2.5, tok::cyno_orchid());
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
                NotchMode::Music => draw_title(p, "Music"),
                NotchMode::Volume => draw_title(p, "Volume Master"),
                NotchMode::MemReduct => draw_title(p, "Mem Reduct"),
                NotchMode::Shelf => draw_title(p, "Quick Shelf"),
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
                NotchMode::Music => Self::draw_music_view(p, state, w, card_y),
                NotchMode::Volume => Self::draw_volume_view(p, state, w, card_y),
                NotchMode::MemReduct => Self::draw_memreduct_view(p, state, w, card_y),
                NotchMode::Shelf => Self::draw_shelf_view(p, state, w, card_y),
                NotchMode::Settings => Self::draw_settings_view(p, state, w, card_y),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 0. HOME COMMAND HUB (Cyno Companion + Hydration OR Net/Focus + Media/Tasks)
    // ─────────────────────────────────────────────────────────────────────────────
}
