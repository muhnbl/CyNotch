use crate::font::{FontRenderer, Weight};
use crate::icons::{self, Icon};
use crate::models::{
    Action, AppState, IdleMetric, LyricSize, LyricStyle, MascotMood, MotionPreset, NotchMode,
    NotchPlacement, NotificationKind, ShelfKind, ToggleId, TypeScale,
};
use crate::painter::{fade, lerp_color, rgba, tok, Hit, Painter};
use crate::telemetry::{format_speed_long, format_speed_short};
use chrono::{Local, Timelike};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tiny_skia::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct NotchChromeKey {
    pm_w: u32,
    pm_h: u32,
    origin_x_bits: u32,
    origin_y_bits: u32,
    w_bits: u32,
    h_bits: u32,
    r_tl_bits: u32,
    r_tr_bits: u32,
    r_br_bits: u32,
    r_bl_bits: u32,
    expand_milli: u32,
    frosted: bool,
    frosted_opacity_pct: u32,
    placement: u8,
}

pub struct Renderer {
    pub pixmap: Pixmap,
    pub clip_mask: Mask,
    pub fonts: FontRenderer,
    pub hits: Vec<Hit>,
    pub dirty_rect: (u32, u32, u32, u32),
    last_drawn_rect: Option<(u32, u32, u32, u32)>,
    chrome_cache: Option<(NotchChromeKey, Pixmap)>,
    expanded_cache: Option<(u64, Pixmap, Vec<Hit>)>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let pixmap = Pixmap::new(w, h).unwrap();
        let clip_mask = Mask::new(w, h).unwrap();
        let fonts = FontRenderer::new();
        Self {
            pixmap,
            clip_mask,
            fonts,
            hits: Vec::new(),
            dirty_rect: (0, 0, w, h),
            last_drawn_rect: None,
            chrome_cache: None,
            expanded_cache: None,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap = Pixmap::new(w, h).unwrap();
            self.clip_mask = Mask::new(w, h).unwrap();
            self.last_drawn_rect = None;
            self.chrome_cache = None;
            self.expanded_cache = None;
            self.dirty_rect = (0, 0, w, h);
        }
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
            (r_tl.min(half), r_tl.min(half), r_tl.min(half), lx < r_tl.min(half) && ly < r_tl.min(half)),
            (r_tr.min(half), w - r_tr.min(half), r_tr.min(half), lx > w - r_tr.min(half) && ly < r_tr.min(half)),
            (r_br.min(half), w - r_br.min(half), h - r_br.min(half), lx > w - r_br.min(half) && ly > h - r_br.min(half)),
            (r_bl.min(half), r_bl.min(half), h - r_bl.min(half), lx < r_bl.min(half) && ly > h - r_bl.min(half)),
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

    pub fn render(&mut self, state: &AppState) {
        self.render_interactive(state, None);
    }

    pub fn render_interactive(&mut self, state: &AppState, pointer: Option<(f32, f32)>) {
        self.render_with_origin(state, pointer, 0.0, 0.0);
    }

    pub fn render_centered(&mut self, state: &AppState, pointer: Option<(f32, f32)>) {
        let pm_w = self.pixmap.width() as f32;
        let pm_h = self.pixmap.height() as f32;
        let (origin_x, origin_y) = Self::compute_origin(state, pm_w, pm_h);
        self.render_with_origin(state, pointer, origin_x, origin_y);
    }

    fn render_with_origin(
        &mut self,
        state: &AppState,
        pointer: Option<(f32, f32)>,
        origin_x: f32,
        origin_y: f32,
    ) {
        let pm_w = self.pixmap.width();
        let pm_h = self.pixmap.height();
        let w = state.current_width;
        let h = state.current_height;

        let pad = 12.0f32;
        let cur_x0 = (origin_x - pad).floor().max(0.0) as u32;
        let cur_y0 = (origin_y - pad).floor().max(0.0) as u32;
        let cur_x1 = ((origin_x + w + pad).ceil().max(0.0) as u32).min(pm_w);
        let cur_y1 = ((origin_y + h + pad).ceil().max(0.0) as u32).min(pm_h);
        let cur_rect = (cur_x0, cur_y0, cur_x1, cur_y1);

        let (clear_x0, clear_y0, clear_x1, clear_y1) = if let Some(prev) = self.last_drawn_rect {
            (
                cur_x0.min(prev.0).min(pm_w),
                cur_y0.min(prev.1).min(pm_h),
                cur_x1.max(prev.2).min(pm_w),
                cur_y1.max(prev.3).min(pm_h),
            )
        } else {
            (0, 0, pm_w, pm_h)
        };

        if self.last_drawn_rect.is_none()
            || (clear_x0 == 0 && clear_y0 == 0 && clear_x1 == pm_w && clear_y1 == pm_h)
        {
            self.pixmap.data_mut().fill(0);
        } else if clear_y1 > clear_y0 && clear_x1 > clear_x0 {
            // Clear only the dirty columns: the transparent margin outside the
            // capsule never receives pixels, so a full-row memset just burns
            // memory bandwidth on every frame.
            let stride = (pm_w as usize) * 4;
            let col_start = (clear_x0 as usize) * 4;
            let col_end = (clear_x1 as usize) * 4;
            let data = self.pixmap.data_mut();
            for row in (clear_y0 as usize)..(clear_y1 as usize) {
                let off = row * stride;
                data[off + col_start..off + col_end].fill(0);
            }
        }
        self.dirty_rect = (clear_x0, clear_y0, clear_x1, clear_y1);
        self.last_drawn_rect = Some(cur_rect);
        self.hits.clear();

        let exp_w = state.expanded_width();
        let exp_h = state.expanded_height();
        let base_w = state.base_expanded_width();
        let base_h = state.base_expanded_height();

        let settled_expanded = state.is_hovered
            && (w - exp_w).abs() < 0.5
            && (h - exp_h).abs() < 0.5;
        let t = if settled_expanded {
            1.0
        } else {
            state.anim_expand_t.clamp(0.0, 1.0)
        };

        let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();

        let frosted = state.is_frosted_active();
        let chrome_key = NotchChromeKey {
            pm_w,
            pm_h,
            origin_x_bits: origin_x.to_bits(),
            origin_y_bits: origin_y.to_bits(),
            w_bits: w.to_bits(),
            h_bits: h.to_bits(),
            r_tl_bits: r_tl.to_bits(),
            r_tr_bits: r_tr.to_bits(),
            r_br_bits: r_br.to_bits(),
            r_bl_bits: r_bl.to_bits(),
            expand_milli: (t * 1000.0).round() as u32,
            frosted,
            frosted_opacity_pct: state.frosted_opacity_pct,
            placement: state.placement as u8,
        };
        let can_cache_expanded = settled_expanded
            && !state.placement.is_vertical()
            && state.notification.is_none()
            && matches!(state.mode, NotchMode::Home | NotchMode::Music);
        let exp_sig = if can_cache_expanded {
            Self::compute_expanded_static_sig(state, &chrome_key, pointer)
        } else {
            0
        };
        let exp_hit = can_cache_expanded
            && matches!(
                &self.expanded_cache,
                Some((sig, pm, _)) if *sig == exp_sig && pm.width() == pm_w && pm.height() == pm_h
            );
        if exp_hit && let Some((_, cached_pm, cached_hits)) = self.expanded_cache.as_ref() {
            let stride = (pm_w as usize) * 4;
            let col_start = (cur_x0 as usize) * 4;
            let col_end = (cur_x1 as usize) * 4;
            if col_end > col_start && cur_y1 > cur_y0 {
                let src = cached_pm.data();
                let dst = self.pixmap.data_mut();
                for row in (cur_y0 as usize)..(cur_y1 as usize) {
                    let off = row * stride;
                    dst[off + col_start..off + col_end]
                        .copy_from_slice(&src[off + col_start..off + col_end]);
                }
            }
            let mut hits = cached_hits.iter().map(|h| Hit { rect: h.rect, action: h.action.clone() }).collect::<Vec<Hit>>();
            let mut p = Painter {
                pm: &mut self.pixmap,
                clip_mask: &mut self.clip_mask,
                fonts: &mut self.fonts,
                s: state.expanded_scale(),
                origin_x,
                origin_y,
                pointer,
                hits: &mut hits,
                interactive: false,
                global_alpha: 1.0,
                hover_slots: state.hover_slots,
                micro_interactions: state.micro_interactions,
                frosted,
                frosted_opacity_pct: state.frosted_opacity_pct,
                type_scale: state.type_scale,
                clip_bounds: None,
            };
            Self::draw_expanded_anim_overlay(&mut p, state);
            self.hits = hits;
            return;
        }

        let chrome_hit = matches!(
            &self.chrome_cache,
            Some((k, pm)) if *k == chrome_key && pm.width() == pm_w && pm.height() == pm_h
        );
        if chrome_hit && let Some((_, cached_pm)) = self.chrome_cache.as_ref() {
            let stride = (pm_w as usize) * 4;
            let col_start = (cur_x0 as usize) * 4;
            let col_end = (cur_x1 as usize) * 4;
            if col_end > col_start && cur_y1 > cur_y0 {
                let src = cached_pm.data();
                let dst = self.pixmap.data_mut();
                for row in (cur_y0 as usize)..(cur_y1 as usize) {
                    let off = row * stride;
                    dst[off + col_start..off + col_end]
                        .copy_from_slice(&src[off + col_start..off + col_end]);
                }
            }
        }

        let mut hits = std::mem::take(&mut self.hits);
        {
            let mut p = Painter {
                pm: &mut self.pixmap,
                clip_mask: &mut self.clip_mask,
                fonts: &mut self.fonts,
                s: 1.0,
                origin_x,
                origin_y,
                pointer,
                hits: &mut hits,
                interactive: true,
                global_alpha: 1.0,
                hover_slots: state.hover_slots,
                micro_interactions: state.micro_interactions,
                frosted,
                frosted_opacity_pct: state.frosted_opacity_pct,
                type_scale: state.type_scale,
                clip_bounds: None,
            };

            if !chrome_hit {
                // 1. Volumetric Obsidian Shell
                let (shell_top, shell_bot) = if frosted {
                    (
                        tok::scale_frosted(tok::obsidian_shell_frosted(), state.frosted_opacity_pct),
                        tok::scale_frosted(tok::obsidian_deep_frosted(), state.frosted_opacity_pct),
                    )
                } else {
                    (tok::obsidian_shell(), tok::obsidian_deep())
                };
                p.rrect_corners4_vertical_gradient(
                    0.0,
                    0.0,
                    w,
                    h,
                    r_tl,
                    r_tr,
                    r_br,
                    r_bl,
                    shell_top,
                    shell_bot,
                );

                // 2. Clipped Internal Atmospheric Aurora Glows (Photo-matched bottom purple bloom)
                let glow_scale = if frosted {
                    (state.frosted_opacity_pct as f32 / 68.0).clamp(0.25, 1.0)
                } else {
                    1.0
                };
                if p.prepare_corners4_clip(0.0, 0.0, w, h, r_tl, r_tr, r_br, r_bl) {
                    if t > 0.05 {
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
                    }
                    if t < 0.95 {
                        let g_x = if state.placement == NotchPlacement::Right { w - 24.0 } else { 24.0 };
                        p.radial_glow(
                            g_x,
                            h * 0.5,
                            (h.min(w) * 0.85).max(16.0),
                            fade(rgba(139, 92, 246, 38), (1.0 - t) * glow_scale),
                            BlendMode::SourceOver,
                            true,
                        );
                    }
                }

                let needs_new = !matches!(
                    &self.chrome_cache,
                    Some((_, pm)) if pm.width() == pm_w && pm.height() == pm_h
                );
                if needs_new {
                    self.chrome_cache = Pixmap::new(pm_w, pm_h).map(|pm| (chrome_key, pm));
                }
                if let Some((ref mut k, ref mut cached_pm)) = self.chrome_cache {
                    *k = chrome_key;
                    let stride = (pm_w as usize) * 4;
                    let col_start = (cur_x0 as usize) * 4;
                    let col_end = (cur_x1 as usize) * 4;
                    if col_end > col_start && cur_y1 > cur_y0 {
                        let src = p.pm.data();
                        let dst = cached_pm.data_mut();
                        for row in (cur_y0 as usize)..(cur_y1 as usize) {
                            let off = row * stride;
                            dst[off + col_start..off + col_end]
                                .copy_from_slice(&src[off + col_start..off + col_end]);
                        }
                    }
                }
            }

            // Always ensure clip_mask matches the current capsule bounds even on chrome_hit frames,
            // so notification banner glows and overlays never bleed outside the notch.
            let _ = p.prepare_corners4_clip(0.0, 0.0, w, h, r_tl, r_tr, r_br, r_bl);

            let show_idle_content = !settled_expanded && t < 0.32;
            if show_idle_content {
                let fade_out = if state.motion_preset == MotionPreset::Reduced {
                    1.0
                } else {
                    ((0.32 - t) / 0.32).clamp(0.0, 1.0)
                };
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
                let fade_in = if settled_expanded || state.motion_preset == MotionPreset::Reduced {
                    1.0
                } else {
                    ((t - 0.32) / 0.68).clamp(0.0, 1.0)
                };
                if fade_in > 0.01 {
                    p.global_alpha = fade_in;
                    let target_s = state.expanded_scale();
                    if settled_expanded {
                        p.s = target_s;
                    } else {
                        let scale_fit = (w / base_w).min(h / base_h).min(target_s).max(0.15);
                        p.s = scale_fit;
                        p.origin_x = origin_x + (w - base_w * scale_fit) * 0.5;
                        p.origin_y = origin_y + (h - base_h * scale_fit) * 0.5;
                    }
                    if state.placement.is_vertical() {
                        Self::draw_expanded_vertical(&mut p, state, base_w, base_h, settled_expanded);
                    } else {
                        Self::draw_expanded(&mut p, state, base_w, base_h, settled_expanded);
                        if can_cache_expanded {
                            let needs_new = !matches!(
                                &self.expanded_cache,
                                Some((_, pm, _)) if pm.width() == pm_w && pm.height() == pm_h
                            );
                            if needs_new && let Some(pm) = Pixmap::new(pm_w, pm_h) {
                                self.expanded_cache = Some((exp_sig, pm, Vec::new()));
                            }
                            if let Some((ref mut sig, ref mut cached_pm, ref mut cached_hits)) = self.expanded_cache {
                                *sig = exp_sig;
                                cached_pm.data_mut().fill(0);
                                let stride = (pm_w as usize) * 4;
                                let col_start = (cur_x0 as usize) * 4;
                                let col_end = (cur_x1 as usize) * 4;
                                if col_end > col_start && cur_y1 > cur_y0 {
                                    let src = p.pm.data();
                                    let dst = cached_pm.data_mut();
                                    for row in (cur_y0 as usize)..(cur_y1 as usize) {
                                        let off = row * stride;
                                        dst[off + col_start..off + col_end]
                                            .copy_from_slice(&src[off + col_start..off + col_end]);
                                    }
                                }
                                *cached_hits = p
                                    .hits
                                    .iter()
                                    .map(|h| Hit {
                                        rect: h.rect,
                                        action: h.action.clone(),
                                    })
                                    .collect();
                            }
                            Self::draw_expanded_anim_overlay(&mut p, state);
                        }
                    }
                }
            }
        }
        self.hits = hits;
    }

    fn compute_expanded_static_sig(
        state: &AppState,
        chrome_key: &NotchChromeKey,
        pointer: Option<(f32, f32)>,
    ) -> u64 {
        let mut h = DefaultHasher::new();
        chrome_key.hash(&mut h);
        (
            state.mode as u8,
            state.type_scale as u8,
            state.expanded_size as u8,
            state.is_pinned,
            state.settings_open,
            state.motion_preset as u8,
            state.micro_interactions,
            state.show_mascot,
            state.mascot_mood() as u8,
        )
            .hash(&mut h);
        for id in ToggleId::ALL {
            state.toggle_target(id).hash(&mut h);
        }
        (state.anim_dock_x.to_bits(), state.anim_dock_w.to_bits()).hash(&mut h);
        for v in &state.hover_slots {
            v.to_bits().hash(&mut h);
        }
        if !(state.micro_interactions && state.motion_preset != MotionPreset::Reduced) {
            pointer.map(|(x, y)| (x.to_bits(), y.to_bits())).hash(&mut h);
        }
        (
            state.telemetry.battery_pct,
            state.telemetry.on_ac,
            state.telemetry.cpu_usage.to_bits(),
            state.telemetry.memory_percent.to_bits(),
            state.telemetry.memory_used_gb.to_bits(),
            state.anim_ram_pct.to_bits(),
            state.mem_reduct.is_purging,
        )
            .hash(&mut h);
        (
            state.health_reminders_enabled,
            state.water.taken_ml,
            state.water.goal_ml,
            state.water.glass_ml,
            state.anim_water_pct.to_bits(),
            state.telemetry.net_down_kbps.to_bits(),
            state.telemetry.net_up_kbps.to_bits(),
            state.timer.remaining_secs,
            state.timer.is_running,
        )
            .hash(&mut h);
        (
            &state.media.title,
            &state.media.artist,
            &state.media.album,
            state.media.is_playing,
            state.media.position_ms,
            state.media.duration_ms,
        )
            .hash(&mut h);
        (
            state.tasks.len(),
            state.tasks.iter().filter(|t| !t.completed).count(),
        )
            .hash(&mut h);
        (
            state.show_lyrics,
            state.equalizer_enabled,
            state.lyric_size as u8,
            state.lyric_style as u8,
            state.lyrics_offset,
            state.current_lyric_index,
            &state.current_lyric_text,
            state.lyrics.len(),
        )
            .hash(&mut h);
        for l in &state.lyrics {
            l.text.hash(&mut h);
            l.seconds.to_bits().hash(&mut h);
        }
        h.finish()
    }

    fn draw_expanded_anim_overlay(p: &mut Painter, state: &AppState) {
        let card_y = 60.0;
        let anim_t = if state.motion_preset == MotionPreset::Reduced {
            0.0
        } else {
            state.anim_time
        };
        match state.mode {
            NotchMode::Home => {
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
                        6.0, 10.0, 14.0, 11.0, 16.0, 18.0, 13.0, 9.0, 15.0, 18.0, 12.0, 7.0,
                        14.0, 10.0,
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

    fn idle_arc_fraction_and_color(state: &AppState) -> (Option<f32>, Color) {
        if let Some(ref notif) = state.notification {
            let col = match notif.kind {
                NotificationKind::SystemToast | NotificationKind::Water => tok::cyan_hi(),
                NotificationKind::MemoryPurge => tok::cyno_mist(),
                NotificationKind::Capture | NotificationKind::Pasted | NotificationKind::General => {
                    tok::cyno_violet_hi()
                }
            };
            return (Some(1.0), col);
        }
        if state.timer.is_running {
            let frac = (state.timer.remaining_secs as f32 / state.timer.total_secs.max(1) as f32)
                .clamp(0.0, 1.0);
            let col = if state.timer.is_break {
                tok::cyan_hi()
            } else {
                tok::cyno_mist()
            };
            return (Some(frac), col);
        }
        if state.media.is_playing && state.media.duration_ms > 0 {
            let frac = (state.media.position_ms as f32 / state.media.duration_ms as f32)
                .clamp(0.0, 1.0);
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
                let mins = (Local::now().minute() as f32 / 60.0).clamp(0.04, 1.0);
                (Some(mins), tok::cyno_mist())
            }
            IdleMetric::Minimal => (None, rgba(255, 255, 255, 36)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_progress_arc_badge(
        p: &mut Painter,
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

    fn notification_accent_and_icon(notif: &crate::models::NotificationBanner) -> (Color, Icon) {
        let accent = match notif.kind {
            NotificationKind::MemoryPurge => tok::cyno_orchid(),
            NotificationKind::Water
            | NotificationKind::Capture
            | NotificationKind::Pasted
            | NotificationKind::SystemToast
            | NotificationKind::General => tok::cyno_violet_hi(),
        };
        let icon = match notif.kind {
            NotificationKind::Water => Icon::Droplet,
            NotificationKind::MemoryPurge => Icon::Zap,
            NotificationKind::Capture => Icon::Video,
            NotificationKind::Pasted => Icon::Clipboard,
            NotificationKind::SystemToast => Icon::Bell,
            NotificationKind::General => {
                if notif.is_success {
                    Icon::Check
                } else {
                    Icon::Bell
                }
            }
        };
        (accent, icon)
    }

    fn notification_remaining_frac(notif: &crate::models::NotificationBanner) -> f32 {
        let now = std::time::Instant::now();
        if notif.expires_at > now {
            (notif.expires_at.saturating_duration_since(now).as_secs_f32()
                / notif.total_secs.max(1.0))
            .clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    fn draw_notification_banner(p: &mut Painter, state: &AppState, w: f32, h: f32) {
        let Some(ref notif) = state.notification else {
            return;
        };
        let (accent, kind_icon) = Self::notification_accent_and_icon(notif);
        let app_icon = icons::for_app(&notif.app_name, kind_icon);
        let rem_frac = Self::notification_remaining_frac(notif);
        let cy = h * 0.5;

        // Photo-matched luminous bottom-edge purple/lavender bloom + left ambient halo
        p.elliptical_glow(
            w * 0.48,
            h + 4.0,
            w * 0.56,
            (h * 0.62).max(28.0),
            rgba(124, 58, 237, 48),
            BlendMode::Plus,
            true,
        );
        p.elliptical_glow(
            w * 0.48,
            h + 6.0,
            w * 0.34,
            (h * 0.34).max(16.0),
            rgba(221, 214, 254, 24),
            BlendMode::Plus,
            true,
        );
        p.radial_glow(
            30.0,
            cy,
            48.0,
            fade(tok::cyno_violet(), 0.28),
            BlendMode::SourceOver,
            true,
        );

        // Left Circular Dark Frosted Glass App-Icon Badge (matches photo top-left icon badge)
        let left_cx = 30.0;
        let badge_r = 18.0;
        p.circle(left_cx, cy + 1.5, badge_r + 0.5, rgba(0, 0, 0, 95));
        p.radial_glow(left_cx, cy, 26.0, fade(tok::cyno_violet(), 0.34), BlendMode::Plus, true);
        p.rrect_vertical_gradient(
            left_cx - badge_r,
            cy - badge_r,
            badge_r * 2.0,
            badge_r * 2.0,
            badge_r,
            rgba(38, 40, 62, 246),
            rgba(16, 17, 30, 250),
        );
        p.elliptical_glow(
            left_cx,
            cy - badge_r * 0.52,
            badge_r * 0.62,
            badge_r * 0.34,
            rgba(245, 240, 255, 38),
            BlendMode::Plus,
            false,
        );
        p.rrect_bevel_stroke(
            left_cx - badge_r,
            cy - badge_r,
            badge_r * 2.0,
            badge_r * 2.0,
            badge_r,
            1.0,
            rgba(235, 230, 255, 86),
            rgba(192, 132, 252, 48),
        );
        icons::draw(p, app_icon, left_cx - 9.5, cy - 9.5, 19.0, tok::cyno_lavender_white());

        // Right dismiss button + countdown ring
        let dismiss_cx = (w - 23.0).max(120.0);
        let dis_ht = p.hot_t(dismiss_cx - 14.0, (cy - 14.0).max(0.0), 28.0, 28.0_f32.min(h));
        Self::draw_progress_arc_badge(
            p,
            dismiss_cx,
            cy,
            12.5,
            Some(rem_frac),
            accent,
            lerp_color(rgba(235, 230, 255, 18), rgba(235, 230, 255, 42), dis_ht),
            Icon::Close,
            lerp_color(tok::text_soft(), tok::text1(), dis_ht),
            state.progress_arc_enabled,
        );
        p.hit(
            dismiss_cx - 15.0,
            (cy - 15.0).max(0.0),
            30.0,
            30.0_f32.min(h),
            Action::DismissNotification,
        );

        let mut right_edge = dismiss_cx - 19.0;
        if notif.kind == NotificationKind::Water {
            let act_lbl = format!("+{}ml", state.water.glass_ml);
            let act_w = (p.measure(&act_lbl, 10.2, Weight::Bold) + 22.0).clamp(64.0, 96.0);
            let act_x = (right_edge - act_w - 4.0).max(120.0);
            let act_y = (cy - 14.0).max(6.0);
            let act_ht = p.hot_t(act_x, act_y, act_w, 28.0);
            p.tactile_surface_pill_t(act_x, act_y, act_w, 28.0, 9.0, act_ht, Some(accent));
            p.text_center_fit(
                &act_lbl,
                act_x + act_w * 0.5,
                act_y + 18.0 - 0.5 * act_ht,
                10.2,
                Weight::Bold,
                tok::text1(),
                act_w - 8.0,
            );
            p.hit(
                act_x,
                act_y,
                act_w,
                28.0,
                Action::DrinkWater(state.water.glass_ml),
            );
            right_edge = act_x - 8.0;
        }

        let tx = 60.0;
        let max_tw = (right_edge - tx).max(100.0);
        let app_tag = notif.app_name.trim();
        let show_tag = !app_tag.is_empty();
        let sub_text = notif.subtitle.trim();
        let (sub_lines, sub_sz) = if sub_text.is_empty() {
            (Vec::new(), 12.6)
        } else {
            p.wrap_lines_fit(sub_text, 12.6, 11.2, Weight::Regular, max_tw, 2)
        };

        let line_gap = (p.eff_size(sub_sz) * 1.28).clamp(15.2, 18.5);
        let (row1_y, sub_y0, sub_step) = match sub_lines.len() {
            0 => (cy + 5.0, 0.0, 0.0),
            1 => {
                if h >= 84.0 {
                    (33.0, 58.0, line_gap)
                } else {
                    (30.0, 53.0, line_gap)
                }
            }
            _ => {
                if h >= 84.0 {
                    (27.0, 48.5, line_gap)
                } else {
                    (25.0, 44.5, line_gap.min(18.0))
                }
            }
        };

        let title_raw_sz = 14.0;
        let badge_raw_sz = 10.2;
        let mut title_x = tx;
        let mut title_w = max_tw;
        if show_tag {
            let tag_txt = p.ellipsize(app_tag, badge_raw_sz, Weight::Bold, (max_tw * 0.42).max(58.0));
            let tag_tw = p.measure(&tag_txt, badge_raw_sz, Weight::Bold);
            let pill_w = (tag_tw + 15.0).min(max_tw * 0.48);
            let pill_h = (p.eff_size(badge_raw_sz) + 6.5).clamp(18.0, 22.5);
            let pill_y = row1_y - pill_h * 0.72;
            p.rrect(tx, pill_y, pill_w, pill_h, 7.0, rgba(124, 58, 237, 58));
            p.rrect_bevel_stroke(
                tx,
                pill_y,
                pill_w,
                pill_h,
                7.0,
                0.9,
                rgba(221, 214, 254, 130),
                rgba(168, 85, 247, 75),
            );
            p.text_center_fit(
                &tag_txt,
                tx + pill_w * 0.5,
                row1_y - 0.5,
                badge_raw_sz,
                Weight::Bold,
                tok::cyno_mist(),
                pill_w - 6.0,
            );
            title_x = tx + pill_w + 8.0;
            title_w = (max_tw - pill_w - 8.0).max(48.0);
        }

        let title_str = p.ellipsize(&notif.title, title_raw_sz, Weight::Bold, title_w);
        p.text(&title_str, title_x, row1_y, title_raw_sz, Weight::Bold, tok::text1());

        let body_col = rgba(232, 227, 252, 245);
        for (idx, line) in sub_lines.iter().enumerate() {
            p.text(
                line,
                tx,
                sub_y0 + (idx as f32 * sub_step),
                sub_sz,
                Weight::Regular,
                body_col,
            );
        }
    }

    fn draw_expanded_toast_overlay(p: &mut Painter, state: &AppState, w: f32) {
        let Some(ref notif) = state.notification else {
            return;
        };
        let (accent, kind_icon) = Self::notification_accent_and_icon(notif);
        let app_icon = icons::for_app(&notif.app_name, kind_icon);
        let rem_frac = Self::notification_remaining_frac(notif);
        let bx = 10.0;
        let by = 6.0;
        let bw = (w - 20.0).max(160.0);
        let bh = 48.0;
        let cy = by + bh * 0.5;

        p.rrect_vertical_gradient(bx, by, bw, bh, 12.5, rgba(24, 26, 42, 248), rgba(14, 15, 26, 250));
        p.rrect_bevel_stroke(bx, by, bw, bh, 12.5, 1.0, rgba(235, 230, 255, 65), rgba(192, 132, 252, 65));
        p.radial_glow(
            bx + 22.0,
            cy,
            34.0,
            fade(tok::cyno_violet(), 0.28),
            BlendMode::SourceOver,
            true,
        );

        let ic_cx = bx + 20.0;
        p.rrect_vertical_gradient(ic_cx - 12.5, cy - 12.5, 25.0, 25.0, 12.5, rgba(38, 40, 62, 246), rgba(16, 17, 30, 250));
        p.circle_stroke(ic_cx, cy, 12.5, 0.9, rgba(235, 230, 255, 72));
        icons::draw(p, app_icon, ic_cx - 6.5, cy - 6.5, 13.0, tok::cyno_lavender_white());

        let dis_cx = bx + bw - 20.0;
        let dis_ht = p.hot_t(dis_cx - 13.0, cy - 13.0, 26.0, 26.0);
        Self::draw_progress_arc_badge(
            p,
            dis_cx,
            cy,
            11.5,
            Some(rem_frac),
            accent,
            lerp_color(rgba(255, 255, 255, 18), rgba(255, 255, 255, 40), dis_ht),
            Icon::Close,
            lerp_color(tok::text2(), tok::text1(), dis_ht),
            state.progress_arc_enabled,
        );
        p.hit(
            dis_cx - 14.0,
            by,
            28.0,
            bh,
            Action::DismissNotification,
        );

        let mut right_edge = dis_cx - 16.0;
        if notif.kind == NotificationKind::Water {
            let act_lbl = format!("+{}ml", state.water.glass_ml);
            let aw = (p.measure(&act_lbl, 9.8, Weight::Bold) + 20.0).clamp(58.0, 88.0);
            let ax = right_edge - aw - 4.0;
            let ay = by + 10.0;
            let aht = p.hot_t(ax, ay, aw, 28.0);
            p.tactile_surface_pill_t(ax, ay, aw, 28.0, 8.0, aht, Some(accent));
            p.text_center_fit(
                &act_lbl,
                ax + aw * 0.5,
                ay + 17.5 - 0.5 * aht,
                9.8,
                Weight::Bold,
                tok::text1(),
                aw - 6.0,
            );
            p.hit(ax, ay, aw, 28.0, Action::DrinkWater(state.water.glass_ml));
            right_edge = ax - 8.0;
        }

        let tx = bx + 42.0;
        let max_tw = (right_edge - tx).max(70.0);
        let mut title_x = tx;
        let mut title_w = max_tw;
        let app_tag = notif.app_name.trim();
        if !app_tag.is_empty() {
            let tag = p.ellipsize(app_tag, 9.4, Weight::Bold, (max_tw * 0.36).max(48.0));
            let pw = p.measure(&tag, 9.4, Weight::Bold) + 12.0;
            let ph = (p.eff_size(9.4) + 5.5).clamp(15.0, 20.0);
            p.rrect(tx, by + 6.5, pw, ph, 5.0, rgba(124, 58, 237, 58));
            p.text_center_fit(
                &tag,
                tx + pw * 0.5,
                by + 6.5 + ph * 0.72,
                9.4,
                Weight::Bold,
                tok::cyno_mist(),
                pw - 6.0,
            );
            title_x = tx + pw + 7.0;
            title_w = (max_tw - pw - 7.0).max(32.0);
        }
        let ttl = p.ellipsize(&notif.title, 12.4, Weight::Bold, title_w);
        p.text(&ttl, title_x, by + 19.0, 12.4, Weight::Bold, tok::text1());

        let sub = notif.subtitle.trim();
        if !sub.is_empty() {
            let sub_line = p.ellipsize(sub, 11.4, Weight::Regular, max_tw);
            p.text(&sub_line, tx, by + 37.5, 11.4, Weight::Regular, rgba(232, 227, 252, 245));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // ULTRA-COMPACT MINIMIZED STEALTH PILL (86×18 Horizontal, 18×86 Vertical)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_minimized_idle_capsule(p: &mut Painter, state: &AppState, w: f32, h: f32) {
        let cy = h * 0.5;
        let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
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
            let short_lbl = match notif.kind {
                NotificationKind::SystemToast => {
                    if !notif.app_name.trim().is_empty() {
                        notif.app_name.trim().chars().take(10).collect()
                    } else {
                        "Toast".to_string()
                    }
                }
                NotificationKind::Water => "Water".to_string(),
                NotificationKind::MemoryPurge => "Purged".to_string(),
                NotificationKind::Capture => "Shot".to_string(),
                NotificationKind::Pasted => "Pasted".to_string(),
                NotificationKind::General => notif.title.clone(),
            };
            let title = p.ellipsize(&short_lbl, 8.6, Weight::Bold, max_txt_w);
            p.text(&title, left_x, cy + 3.0, 8.6, Weight::Bold, tok::text1());
            if state.progress_arc_enabled {
                p.circle(badge_cx, cy, 3.8, fade(arc_col, 0.35));
            }
            p.circle(badge_cx, cy, 2.4, arc_col);
            if notif.kind == NotificationKind::Water {
                let hx = (badge_cx - 14.0).max(0.0);
                p.hit(hx, 0.0, (w - hx).max(20.0), h, Action::DrinkWater(state.water.glass_ml));
            }
        } else if state.timer.is_running {
            let m = state.timer.remaining_secs / 60;
            let s = state.timer.remaining_secs % 60;
            let txt = p.ellipsize(&format!("{:02}:{:02}", m, s), 8.8, Weight::Bold, max_txt_w);
            p.text_center(&txt, (left_x + badge_cx - 6.0) * 0.5, cy + 3.0, 8.8, Weight::Bold, arc_col);
            if state.progress_arc_enabled {
                p.arc(badge_cx, cy, 3.8, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * frac_opt.unwrap_or(1.0), 1.4, arc_col);
            }
            p.circle(badge_cx, cy, 2.1, arc_col);
        } else if state.media.is_playing {
            let eq_w = if state.equalizer_enabled { 13.0 } else { 0.0 };
            let txt = p.ellipsize(&state.media.title, 8.5, Weight::Semibold, (max_txt_w - eq_w).max(16.0));
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
            let metric_str = match state.idle_metric {
                IdleMetric::Ram => format!("{:.0}%", state.telemetry.memory_percent),
                IdleMetric::NetSpeed => format!("↓{}", format_speed_short(state.telemetry.net_down_kbps)),
                IdleMetric::Cpu => format!("C{:.0}%", state.telemetry.cpu_usage),
                IdleMetric::Clock => Local::now().format("%H:%M").to_string(),
                IdleMetric::Minimal => "•••".to_string(),
            };
            let txt = p.ellipsize(&metric_str, 8.8, Weight::Bold, max_txt_w);
            p.text_center(&txt, (left_x + badge_cx - 5.5) * 0.5, cy + 3.0, 8.8, Weight::Bold, tok::text_soft());
            if state.progress_arc_enabled {
                p.circle_stroke(badge_cx, cy, 3.8, 1.1, rgba(255, 255, 255, 24));
                if let Some(frac) = frac_opt
                    && frac > 0.02
                {
                    p.arc(badge_cx, cy, 3.8, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * frac, 1.4, arc_col);
                }
            }
            p.circle(badge_cx, cy, 2.0, arc_col);
        }
    }

    fn draw_minimized_idle_vertical(p: &mut Painter, state: &AppState, w: f32, h: f32) {
        let cx = w * 0.5;
        let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
        let mood = state.mascot_mood();
        if state.show_mascot {
            Self::draw_cyno_mascot(p, cx, 12.5, 0.34, mood, anim_t, true);
        } else {
            p.circle(cx, 11.0, 2.8, tok::cyno_violet_hi());
        }

        let (frac_opt, arc_col) = Self::idle_arc_fraction_and_color(state);
        let badge_cy = (h - 11.0).max(26.0);
        let mid_lo = 22.0f32;
        let mid_hi = (badge_cy - 8.0).max(mid_lo + 4.0);

        if let Some(ref notif) = state.notification {
            let ic = match notif.kind {
                NotificationKind::SystemToast => Icon::Bell,
                NotificationKind::Water => Icon::Droplet,
                NotificationKind::MemoryPurge => Icon::Zap,
                NotificationKind::Capture => Icon::Video,
                NotificationKind::Pasted | NotificationKind::General => Icon::Check,
            };
            icons::draw(p, ic, cx - 5.0, h * 0.5 - 5.0, 10.0, arc_col);
            if notif.kind == NotificationKind::Water {
                let hy = (h - 26.0).max(0.0);
                p.hit(0.0, hy, w, (h - hy).max(20.0), Action::DrinkWater(state.water.glass_ml));
            }
        } else if state.media.is_playing && state.equalizer_enabled {
            let eq_top = ((mid_lo + mid_hi) * 0.5 - 6.0).max(mid_lo);
            for i in 0..3 {
                let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                let bw = (4.0 + wave * 6.0).clamp(3.0, 10.0);
                let by = eq_top + (i as f32 * 5.0);
                p.rrect(cx - bw * 0.5, by, bw, 2.2, 1.1, tok::cyno_violet_hi());
            }
        } else {
            let avail_h = (mid_hi - mid_lo).max(8.0);
            let bar_h = (h * 0.36).clamp(12.0, 44.0).min(avail_h);
            let max_y = (mid_hi - bar_h).max(mid_lo);
            let bar_y = (h * 0.5 - bar_h * 0.5).clamp(mid_lo, max_y);
            p.rrect(cx - 1.8, bar_y, 3.6, bar_h, 1.8, rgba(255, 255, 255, 24));
            let fill_h = (bar_h * frac_opt.unwrap_or(0.35)).clamp(4.0, bar_h.max(4.0));
            p.rrect(cx - 1.8, bar_y + bar_h - fill_h, 3.6, fill_h, 1.8, arc_col);
        }

        if state.progress_arc_enabled {
            p.circle_stroke(cx, badge_cy, 3.8, 1.1, rgba(255, 255, 255, 24));
            if let Some(frac) = frac_opt
                && frac > 0.02
            {
                p.arc(cx, badge_cy, 3.8, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * frac, 1.4, arc_col);
            }
        }
        p.circle(cx, badge_cy, 2.0, arc_col);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // IDLE CAPSULE (Horizontal 280..390 × 42)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_idle_capsule(p: &mut Painter, state: &AppState, w: f32, h: f32) {
        let cy = h / 2.0;
        let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
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
            let title_raw = if notif.kind == NotificationKind::SystemToast && !notif.app_name.is_empty() {
                format!("[{}] {}", notif.app_name, notif.title)
            } else {
                notif.title.clone()
            };
            let title = p.ellipsize(&title_raw, 12.2, Weight::Bold, max_txt_w);
            p.text(&title, text_x, 18.5, 12.2, Weight::Bold, tok::text1());
            let sub_src = if notif.kind == NotificationKind::SystemToast && notif.subtitle.is_empty() {
                "Windows Toast"
            } else {
                notif.subtitle.as_str()
            };
            let sub = p.ellipsize(sub_src, 10.5, Weight::Regular, max_txt_w);
            p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());

            let (icon, ic_col) = match notif.kind {
                NotificationKind::SystemToast => (Icon::Bell, tok::cyan_hi()),
                NotificationKind::Water => (Icon::Droplet, tok::cyan_hi()),
                NotificationKind::MemoryPurge => (Icon::Zap, tok::cyno_mist()),
                NotificationKind::Capture => (Icon::Video, tok::cyno_violet_hi()),
                NotificationKind::Pasted | NotificationKind::General => (Icon::Check, tok::cyno_violet_hi()),
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
            if notif.kind == NotificationKind::Water {
                let hx = (badge_cx - 18.0).max(0.0);
                p.hit(hx, 2.0, (w - hx).max(24.0), h - 4.0, Action::DrinkWater(state.water.glass_ml));
            }
        } else if state.timer.is_running {
            let label = if state.timer.is_break { "Break Time" } else { "Focus Sprint" };
            let m = state.timer.remaining_secs / 60;
            let s = state.timer.remaining_secs % 60;
            let title = p.ellipsize(&format!("{} • {:02}:{:02}", label, m, s), 12.5, Weight::Bold, max_txt_w);
            p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
            let sub = p.ellipsize(&format!("Cycle #{} active", state.timer.cycles_completed + 1), 10.5, Weight::Regular, max_txt_w);
            p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::cyno_mist());

            Self::draw_progress_arc_badge(
                p,
                badge_cx,
                cy,
                11.5,
                frac_opt,
                arc_col,
                tok::cyno_indigo(),
                Icon::Timer,
                arc_col,
                state.progress_arc_enabled,
            );
        } else if state.media.is_playing {
            let show_eq = state.equalizer_enabled;
            let music_txt_w = if show_eq {
                (w - text_x - 68.0).max(55.0)
            } else {
                max_txt_w
            };
            let title = p.ellipsize(&state.media.title, 12.5, Weight::Bold, music_txt_w);
            p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
            let sub = p.ellipsize(&state.media.artist, 10.5, Weight::Regular, music_txt_w);
            p.text(&sub, text_x, 33.0, 10.5, Weight::Regular, tok::text2());

            if show_eq {
                for i in 0..4 {
                    let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                    let bh = (4.0 + wave * 10.0).clamp(3.0, 14.0);
                    let bx = w - 58.0 + (i as f32 * 4.5);
                    let by = cy - bh / 2.0;
                    let c = if i % 2 == 0 { tok::cyno_violet_hi() } else { tok::cyno_mist() };
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
                    let raw = if state.health_reminders_enabled {
                        format!(
                            "{:.0}% RAM • {}/{} ml",
                            state.telemetry.memory_percent, state.water.taken_ml, state.water.goal_ml
                        )
                    } else {
                        format!(
                            "{:.0}% RAM • CPU {:.0}%",
                            state.telemetry.memory_percent, state.telemetry.cpu_usage
                        )
                    };
                    let sub = p.ellipsize(&raw, 10.5, Weight::Regular, max_txt_w);
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
                    let raw = format!(
                        "↓ {}   ↑ {}",
                        format_speed_short(state.telemetry.net_down_kbps),
                        format_speed_short(state.telemetry.net_up_kbps)
                    );
                    let sub = p.ellipsize(&raw, 10.5, Weight::Semibold, max_txt_w);
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
                    let title = p.ellipsize(&format!("CPU {:.0}%", state.telemetry.cpu_usage), 12.5, Weight::Bold, max_txt_w);
                    p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let sub = p.ellipsize(
                        &format!(
                            "RAM {:.0}% ({:.1}G)",
                            state.telemetry.memory_percent, state.telemetry.memory_used_gb
                        ),
                        10.5,
                        Weight::Regular,
                        max_txt_w,
                    );
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
                    let now = Local::now();
                    let title = p.ellipsize(&now.format("%H:%M • %a %d").to_string(), 12.5, Weight::Bold, max_txt_w);
                    p.text(&title, text_x, 18.5, 12.5, Weight::Bold, tok::text1());
                    let sub = p.ellipsize(
                        &format!(
                            "RAM {:.0}% • CPU {:.0}%",
                            state.telemetry.memory_percent, state.telemetry.cpu_usage
                        ),
                        10.5,
                        Weight::Regular,
                        max_txt_w,
                    );
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
                    p.text("CyNotch", text_x, cy + 4.2, 12.5, Weight::Bold, tok::text1());
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
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // VERTICAL IDLE CAPSULE (52 × 168/204/248 for Left / Right Edges)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_idle_vertical(p: &mut Painter, state: &AppState, w: f32, h: f32) {
        let cx = w * 0.5;
        let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
        let mood = state.mascot_mood();

        if state.show_mascot {
            Self::draw_cyno_mascot(p, cx, 23.0, 0.84, mood, anim_t, true);
        } else {
            let top_col = p.surface_top_col();
            p.circle(cx, 22.0, 10.0, top_col);
            p.circle_stroke(cx, 22.0, 10.0, 1.0, rgba(255, 255, 255, 34));
            p.circle(cx, 22.0, 3.6, tok::cyno_violet_hi());
        }

        let badge_cy = h - 22.0;
        let mid_cy = (44.0 + (h - 42.0)) * 0.5;
        let (frac_opt, arc_col) = Self::idle_arc_fraction_and_color(state);

        if let Some(ref notif) = state.notification {
            let (tag, icon, ic_col) = match notif.kind {
                NotificationKind::SystemToast => ("TOAST", Icon::Bell, tok::cyan_hi()),
                NotificationKind::Water => ("WATER", Icon::Droplet, tok::cyan_hi()),
                NotificationKind::MemoryPurge => ("PURGE", Icon::Zap, tok::cyno_mist()),
                NotificationKind::Capture => ("SHOT", Icon::Video, tok::cyno_violet_hi()),
                NotificationKind::Pasted => ("PASTE", Icon::Check, tok::cyno_violet_hi()),
                NotificationKind::General => ("ALERT", Icon::Check, tok::cyno_violet_hi()),
            };
            p.text_center(tag, cx, mid_cy - 3.0, 9.0, Weight::Bold, ic_col);
            let sub = p.ellipsize(&notif.title, 8.6, Weight::Semibold, w - 8.0);
            p.text_center(&sub, cx, mid_cy + 12.0, 8.6, Weight::Semibold, tok::text1());
            Self::draw_progress_arc_badge(
                p,
                cx,
                badge_cy,
                11.5,
                frac_opt,
                arc_col,
                tok::surface_top(),
                icon,
                ic_col,
                state.progress_arc_enabled,
            );
            if notif.kind == NotificationKind::Water {
                let hy = (h - 38.0).max(0.0);
                p.hit(2.0, hy, w - 4.0, (h - hy).max(22.0), Action::DrinkWater(state.water.glass_ml));
            }
        } else if state.timer.is_running {
            let m = state.timer.remaining_secs / 60;
            let s = state.timer.remaining_secs % 60;
            let tag = if state.timer.is_break { "BRK" } else { "FOC" };
            p.text_center(tag, cx, mid_cy - 14.0, 8.2, Weight::Bold, tok::text2());
            p.text_center(&format!("{:02}", m), cx, mid_cy + 2.0, 12.5, Weight::Bold, arc_col);
            p.text_center(&format!(":{:02}", s), cx, mid_cy + 16.0, 10.5, Weight::Semibold, tok::text1());
            Self::draw_progress_arc_badge(
                p,
                cx,
                badge_cy,
                11.5,
                frac_opt,
                arc_col,
                tok::cyno_indigo(),
                Icon::Timer,
                arc_col,
                state.progress_arc_enabled,
            );
        } else if state.media.is_playing {
            if state.equalizer_enabled {
                for i in 0..4 {
                    let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                    let bh = (6.0 + wave * 14.0).clamp(4.0, 20.0);
                    let bx = cx - 9.0 + (i as f32 * 5.0);
                    let by = mid_cy - 6.0 - bh * 0.5;
                    let c = if i % 2 == 0 { tok::cyno_violet_hi() } else { tok::cyno_mist() };
                    p.rrect(bx, by, 2.8, bh, 1.4, c);
                }
                let lbl = p.ellipsize(&state.media.title, 8.6, Weight::Semibold, w - 8.0);
                p.text_center(&lbl, cx, mid_cy + 20.0, 8.6, Weight::Semibold, tok::text1());
            } else {
                p.text_center("PLAY", cx, mid_cy - 4.0, 8.5, Weight::Bold, tok::cyno_violet_hi());
                let lbl = p.ellipsize(&state.media.title, 8.8, Weight::Semibold, w - 8.0);
                p.text_center(&lbl, cx, mid_cy + 11.0, 8.8, Weight::Semibold, tok::text1());
            }
            Self::draw_progress_arc_badge(
                p,
                cx,
                badge_cy,
                11.5,
                frac_opt,
                arc_col,
                tok::cyno_indigo(),
                Icon::Music,
                rgba(255, 255, 255, 245),
                state.progress_arc_enabled,
            );
        } else {
            let (icon, ic_col) = match state.idle_metric {
                IdleMetric::Ram => {
                    p.text_center("RAM", cx, mid_cy - 12.0, 8.4, Weight::Bold, tok::text2());
                    p.text_center(
                        &format!("{:.0}%", state.telemetry.memory_percent),
                        cx,
                        mid_cy + 4.0,
                        12.0,
                        Weight::Bold,
                        tok::text1(),
                    );
                    let pct = (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0);
                    let well = p.well_col();
                    p.rrect(cx - 14.0, mid_cy + 11.0, 28.0, 4.0, 2.0, well);
                    p.rrect(cx - 14.0, mid_cy + 11.0, (28.0 * pct).max(3.0), 4.0, 2.0, tok::cyno_mist());
                    (Icon::Zap, tok::cyno_mist())
                }
                IdleMetric::NetSpeed => {
                    p.text_center("NET", cx, mid_cy - 14.0, 8.2, Weight::Bold, tok::text2());
                    let dn = p.ellipsize(&format!("↓{}", format_speed_short(state.telemetry.net_down_kbps)), 9.0, Weight::Bold, w - 6.0);
                    let up = p.ellipsize(&format!("↑{}", format_speed_short(state.telemetry.net_up_kbps)), 8.6, Weight::Semibold, w - 6.0);
                    p.text_center(&dn, cx, mid_cy + 1.0, 9.0, Weight::Bold, tok::cyan_hi());
                    p.text_center(&up, cx, mid_cy + 15.0, 8.6, Weight::Semibold, tok::cyno_mist());
                    (Icon::Activity, tok::cyan_hi())
                }
                IdleMetric::Cpu => {
                    p.text_center("CPU", cx, mid_cy - 12.0, 8.4, Weight::Bold, tok::text2());
                    p.text_center(
                        &format!("{:.0}%", state.telemetry.cpu_usage),
                        cx,
                        mid_cy + 4.0,
                        12.0,
                        Weight::Bold,
                        tok::text1(),
                    );
                    let pct = (state.telemetry.cpu_usage / 100.0).clamp(0.0, 1.0);
                    let well = p.well_col();
                    p.rrect(cx - 14.0, mid_cy + 11.0, 28.0, 4.0, 2.0, well);
                    p.rrect(cx - 14.0, mid_cy + 11.0, (28.0 * pct).max(3.0), 4.0, 2.0, tok::cyno_violet_hi());
                    (Icon::Activity, tok::cyno_violet_hi())
                }
                IdleMetric::Clock => {
                    let now = Local::now();
                    p.text_center(&now.format("%H").to_string(), cx, mid_cy - 2.0, 13.5, Weight::Bold, tok::text1());
                    p.text_center(&now.format("%M").to_string(), cx, mid_cy + 14.0, 13.5, Weight::Bold, tok::cyno_mist());
                    (Icon::Timer, tok::cyno_mist())
                }
                IdleMetric::Minimal => {
                    p.rrect(cx - 2.0, mid_cy - 14.0, 4.0, 28.0, 2.0, rgba(192, 132, 252, 95));
                    (Icon::Zap, tok::cyno_mist())
                }
            };
            Self::draw_progress_arc_badge(
                p,
                cx,
                badge_cy,
                11.5,
                if state.idle_metric == IdleMetric::Minimal { None } else { frac_opt },
                arc_col,
                tok::surface_top(),
                icon,
                ic_col,
                state.progress_arc_enabled && state.idle_metric != IdleMetric::Minimal,
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // EXPANDED HEADER & DYNAMIC VOLUMETRIC DOCK (620×232 horizontal base space)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_expanded(p: &mut Painter, state: &AppState, w: f32, _h: f32, settled: bool) {
        let dock_x = 20.0;
        let dock_y = 14.0;
        let dock_h = 34.0;

        let dock_modes = state.visible_dock_modes();
        let (dock_w, item_w) = AppState::dock_metrics(dock_modes.len());

        let well_col = if state.is_frosted_active() {
            tok::scale_frosted(tok::surface_well_frosted(), state.frosted_opacity_pct)
        } else {
            tok::surface_well()
        };
        p.rrect(dock_x, dock_y, dock_w, dock_h, 11.5, well_col);
        p.rrect_bevel_stroke(dock_x, dock_y, dock_w, dock_h, 11.5, 1.0, rgba(0, 0, 0, 120), rgba(255, 255, 255, 26));

        let active_visible = dock_modes.contains(&state.mode);
        let (target_dock_x, target_dock_w) = state.active_dock_target();
        let (pill_x, pill_w) = if !state.micro_interactions || state.motion_preset == MotionPreset::Reduced {
            (target_dock_x, target_dock_w)
        } else {
            (
                state
                    .anim_dock_x
                    .clamp(dock_x + 4.0, (dock_x + dock_w - 4.0 - item_w).max(dock_x + 4.0)),
                state.anim_dock_w.max(20.0),
            )
        };

        if active_visible {
            let active_ht = if state.micro_interactions && state.motion_preset != MotionPreset::Reduced {
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
                NotchMode::FocusTimer => Icon::Timer,
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
            icons::draw(p, Icon::Settings, gear_x + 7.0, dock_y + 10.0, 14.0, rgba(255, 255, 255, 255));
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
            icons::draw(p, Icon::Lock, pin_x + 7.0, dock_y + 10.0, 14.0, rgba(255, 255, 255, 255));
        } else {
            p.tactile_surface_pill_t(pin_x, dock_y + 3.0, 28.0, 28.0, 14.0, pin_ht, None);
            let pc = lerp_color(tok::text2(), tok::text1(), pin_ht);
            icons::draw(p, Icon::Lock, pin_x + 7.0, dock_y + 10.0, 14.0, pc);
        }
        p.hit(pin_x, dock_y + 3.0, 28.0, 28.0, Action::TogglePin);

        // Mode Header Title + Lyrics Pill
        let title_x = dock_x + dock_w + 14.0;
        let title_y = dock_y + 21.5;
        let max_title_w = (pin_x - title_x - 10.0).max(0.0);
        let draw_title = |p: &mut Painter, txt: &str| {
            if max_title_w >= 28.0 {
                let s = p.ellipsize(txt, 13.0, Weight::Bold, max_title_w);
                p.text(&s, title_x, title_y, 13.0, Weight::Bold, tok::text1());
            }
        };

        match state.mode {
            NotchMode::Home => draw_title(p, "Command Hub"),
            NotchMode::Music => {
                let tab_y = dock_y + 4.0;
                let lyrics_pill_w = 58.0;
                let lyrics_pill_x = pin_x - lyrics_pill_w - 10.0;
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
                    p.text_center_shadow("Lyrics", lyrics_pill_x + lyrics_pill_w / 2.0, tab_y + 17.0, 11.0, Weight::Bold, tok::text1());
                } else {
                    p.tactile_surface_pill_t(lyrics_pill_x, tab_y, lyrics_pill_w, 26.0, 13.0, lyrics_ht, None);
                    p.text_center("Lyrics", lyrics_pill_x + lyrics_pill_w / 2.0, tab_y + 17.0, 11.0, Weight::Semibold, tok::text_soft());
                }
                p.hit(lyrics_pill_x, tab_y, lyrics_pill_w, 26.0, Action::ToggleLyrics);
            }
            NotchMode::Volume => draw_title(p, "Volume Master"),
            NotchMode::MemReduct => draw_title(p, "Mem Reduct"),
            NotchMode::FocusTimer => draw_title(p, "Focus Timer"),
            NotchMode::Tasks => draw_title(p, "Tasks"),
            NotchMode::Shelf => draw_title(p, "Quick Shelf"),
            NotchMode::Capture => draw_title(p, "Quick Capture"),
            NotchMode::Telemetry => draw_title(p, "Screen Time"),
            NotchMode::Settings => draw_title(p, "Quick Settings"),
        }

        let card_y = 60.0;
        let mut draw_anim_here = !settled || state.notification.is_some();
        match state.mode {
            NotchMode::Home => Self::draw_home_view(p, state, w, card_y),
            NotchMode::Music => Self::draw_music_view(p, state, w, card_y),
            NotchMode::Volume => {
                draw_anim_here = false;
                Self::draw_volume_view(p, state, w, card_y);
            }
            NotchMode::MemReduct => {
                draw_anim_here = false;
                Self::draw_memreduct_view(p, state, w, card_y);
            }
            NotchMode::FocusTimer => {
                draw_anim_here = false;
                Self::draw_focustimer_view(p, state, w, card_y);
            }
            NotchMode::Tasks => {
                draw_anim_here = false;
                Self::draw_tasks_view(p, state, w, card_y);
            }
            NotchMode::Shelf => {
                draw_anim_here = false;
                Self::draw_shelf_view(p, state, w, card_y);
            }
            NotchMode::Capture => {
                draw_anim_here = false;
                Self::draw_capture_view(p, state, w, card_y);
            }
            NotchMode::Telemetry => {
                draw_anim_here = false;
                Self::draw_screentime_view(p, state, w, card_y);
            }
            NotchMode::Settings => {
                draw_anim_here = false;
                Self::draw_settings_view(p, state, w, card_y);
            }
        }
        if draw_anim_here {
            Self::draw_expanded_anim_overlay(p, state);
        }
        Self::draw_expanded_toast_overlay(p, state, w);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 0. HOME COMMAND HUB (Cyno Companion + Hydration OR Net/Focus + Media/Tasks)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_home_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let margin_x = 20.0;
        let card_h = 154.0;
        let left_w = 192.0;
        let gap = 14.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;

        p.elevated_card(margin_x, card_y, left_w, card_h, 16.0, Some((40.0, 32.0, 72.0, tok::cyno_violet())));

        p.text("General Cyno", margin_x + 56.0, card_y + 27.0, 12.5, Weight::Bold, tok::text1());
        let pwr_str = match (state.telemetry.battery_pct, state.telemetry.on_ac) {
            (Some(pct), true) => format!("CPU {:.0}% • {}% AC", state.telemetry.cpu_usage, pct),
            (Some(pct), false) => format!("CPU {:.0}% • {}% Bat", state.telemetry.cpu_usage, pct),
            (None, _) => format!("CPU {:.0}% • AC Power", state.telemetry.cpu_usage),
        };
        p.text(&pwr_str, margin_x + 56.0, card_y + 42.0, 9.8, Weight::Regular, tok::text2());

        let ram_label = format!("RAM {:.0}%", state.telemetry.memory_percent);
        p.text(&ram_label, margin_x + 16.0, card_y + 70.0, 10.5, Weight::Semibold, tok::text_soft());
        let used_lbl = format!("{:.1}G", state.telemetry.memory_used_gb);
        p.text_right(&used_lbl, margin_x + left_w - 16.0, card_y + 70.0, 10.5, Weight::Semibold, tok::cyno_mist());

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(margin_x + 16.0, card_y + 79.0, left_w - 32.0, 7.5, ram_pct, tok::cyno_deep(), tok::cyno_violet_hi());

        let purge_x = margin_x + 16.0;
        let purge_y = card_y + 106.0;
        let purge_w = left_w - 32.0;
        let purge_h = 30.0;
        let purge_ht = p.hot_t(purge_x, purge_y, purge_w, purge_h);
        p.luminous_pill_button_t(
            purge_x,
            purge_y,
            purge_w,
            purge_h,
            11.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            purge_ht,
        );
        let purge_lbl = if state.mem_reduct.is_purging { "Purging RAM..." } else { "Purge RAM" };
        p.text_center_shadow(purge_lbl, purge_x + purge_w / 2.0, purge_y + 19.0 - 0.8 * purge_ht, 11.5, Weight::Bold, tok::text1());
        p.hit(purge_x, purge_y, purge_w, purge_h, Action::PurgeRam);

        // Right Top Card: Daily Hydration OR Live Network & Quick Focus
        let top_h = 71.0;
        if state.health_reminders_enabled {
            p.elevated_card(right_x, card_y, right_w, top_h, 15.0, Some((right_w - 34.0, 22.0, 74.0, rgba(56, 189, 248, 160))));
            icons::draw(p, Icon::Droplet, right_x + 16.0, card_y + 13.0, 14.5, tok::cyan_hi());
            p.text("Daily Hydration", right_x + 35.0, card_y + 23.5, 11.8, Weight::Bold, tok::text1());

            let drink_w = 84.0;
            let drink_h = 28.0;
            let drink_x = right_x + right_w - drink_w - 13.0;
            let drink_y = card_y + 11.5;
            let water_stat = format!("{} / {} ml", state.water.taken_ml, state.water.goal_ml);
            p.text_right(&water_stat, drink_x - 10.0, card_y + 23.5, 10.5, Weight::Semibold, tok::cyan_hi());

            let water_pct = if smooth {
                state.anim_water_pct.clamp(0.0, 1.0)
            } else {
                (state.water.taken_ml as f32 / state.water.goal_ml.max(1) as f32).clamp(0.0, 1.0)
            };
            p.glowing_striped_bar(
                right_x + 16.0,
                card_y + 46.0,
                drink_x - (right_x + 16.0) - 12.0,
                7.0,
                water_pct,
                rgba(3, 105, 161, 255),
                rgba(56, 189, 248, 255),
            );

            let drink_ht = p.hot_t(drink_x, drink_y, drink_w, drink_h);
            p.tactile_surface_pill_t(drink_x, drink_y, drink_w, drink_h, 10.0, drink_ht, Some(tok::cyno_violet()));
            let glass_lbl = format!("+{} ml", state.water.glass_ml);
            p.text_center(&glass_lbl, drink_x + drink_w / 2.0, drink_y + 18.5 - 0.55 * drink_ht, 11.0, Weight::Bold, tok::text1());
            p.hit(drink_x, drink_y, drink_w, drink_h, Action::DrinkWater(state.water.glass_ml));
        } else {
            p.elevated_card(right_x, card_y, right_w, top_h, 15.0, Some((right_w - 34.0, 22.0, 74.0, tok::cyan_hi())));
            let net_card_ht = p.hot_t(right_x, card_y, right_w - 132.0, top_h);
            if net_card_ht > 0.02 {
                p.rrect_bevel_stroke(
                    right_x,
                    card_y,
                    right_w - 132.0,
                    top_h,
                    15.0,
                    1.0,
                    fade(tok::cyan_hi(), 0.38 * net_card_ht),
                    fade(tok::cyno_violet(), 0.14 * net_card_ht),
                );
            }
            icons::draw(p, Icon::Activity, right_x + 16.0, card_y + 13.0, 14.5, tok::cyan_hi());
            p.text("Live Network & Focus", right_x + 35.0, card_y + 23.5, 11.5, Weight::Bold, tok::text1());

            let foc_w = 110.0;
            let foc_h = 32.0;
            let foc_x = right_x + right_w - foc_w - 13.0;
            let foc_y = card_y + 11.5;

            let net_txt = format!(
                "↓ {}   ↑ {}",
                format_speed_long(state.telemetry.net_down_kbps),
                format_speed_long(state.telemetry.net_up_kbps)
            );
            let net_ell = p.ellipsize(&net_txt, 10.8, Weight::Semibold, foc_x - (right_x + 16.0) - 10.0);
            p.text(&net_ell, right_x + 16.0, card_y + 47.5, 10.8, Weight::Semibold, tok::cyan_hi());
            p.hit(right_x, card_y, foc_x - right_x - 8.0, top_h, Action::SetMode(NotchMode::Telemetry));

            let foc_ht = p.hot_t(foc_x, foc_y, foc_w, foc_h);
            let m = state.timer.remaining_secs / 60;
            let s = state.timer.remaining_secs % 60;
            if state.timer.is_running {
                p.luminous_pill_button_t(
                    foc_x,
                    foc_y,
                    foc_w,
                    foc_h,
                    10.5,
                    tok::cyno_indigo(),
                    tok::cyno_violet(),
                    tok::cyno_orchid(),
                    foc_ht,
                );
                let lbl = format!("Pause {:02}:{:02}", m, s);
                p.text_center_shadow(&lbl, foc_x + foc_w / 2.0, foc_y + 20.0 - 0.7 * foc_ht, 10.5, Weight::Bold, tok::text1());
            } else {
                p.tactile_surface_pill_t(foc_x, foc_y, foc_w, foc_h, 10.5, foc_ht, Some(tok::cyno_violet()));
                let lbl = format!("Focus {:02}:{:02}", m, s);
                p.text_center(&lbl, foc_x + foc_w / 2.0, foc_y + 20.0 - 0.55 * foc_ht, 10.5, Weight::Bold, tok::text1());
            }
            p.hit(foc_x, foc_y, foc_w, foc_h, Action::ToggleFocusTimer);
        }

        // Right Bottom Split: Media Overview + Tasks & Capture
        let bot_y = card_y + top_h + 12.0;
        let bot_h = 71.0;
        let sub_w = (right_w - 12.0) / 2.0;

        // Bottom-Left: Media Overview
        p.elevated_card(right_x, bot_y, sub_w, bot_h, 15.0, None);
        let media_card_ht = p.hot_t(right_x, bot_y, sub_w - 42.0, bot_h);
        if media_card_ht > 0.02 {
            p.rrect_bevel_stroke(
                right_x,
                bot_y,
                sub_w,
                bot_h,
                15.0,
                1.0,
                fade(tok::cyno_violet_hi(), 0.45 * media_card_ht),
                fade(tok::cyno_violet(), 0.18 * media_card_ht),
            );
        }
        let media_header = if state.media.is_idle() {
            "No Active Media"
        } else if state.media.is_playing {
            "Now Playing"
        } else {
            "Media Paused"
        };
        p.text(media_header, right_x + 14.0, bot_y + 22.0, 10.0, Weight::Semibold, tok::text2());
        let song = p.ellipsize(&state.media.title, 12.0, Weight::Bold, sub_w - 56.0);
        p.text(&song, right_x + 14.0, bot_y + 46.0, 12.0, Weight::Bold, tok::text1());
        p.hit(right_x, bot_y, sub_w - 42.0, bot_h, Action::SetMode(NotchMode::Music));

        let mini_play_x = right_x + sub_w - 40.0;
        let mini_play_y = bot_y + 21.5;
        let mp_ht = p.hot_t(mini_play_x, mini_play_y, 28.0, 28.0);
        p.tactile_surface_pill_t(mini_play_x, mini_play_y, 28.0, 28.0, 14.0, mp_ht, Some(tok::cyno_violet()));
        icons::draw(
            p,
            if state.media.is_playing { Icon::Pause } else { Icon::Play },
            mini_play_x + 8.0,
            mini_play_y + 8.0 - 0.55 * mp_ht,
            12.0,
            tok::text1(),
        );
        p.hit(mini_play_x, mini_play_y, 28.0, 28.0, Action::TogglePlayPause);

        // Bottom-Right: Tasks & Quick Screen Shot
        let br_x = right_x + sub_w + 12.0;
        p.elevated_card(br_x, bot_y, sub_w, bot_h, 15.0, None);
        let tasks_card_ht = p.hot_t(br_x, bot_y, sub_w - 42.0, bot_h);
        if tasks_card_ht > 0.02 {
            p.rrect_bevel_stroke(
                br_x,
                bot_y,
                sub_w,
                bot_h,
                15.0,
                1.0,
                fade(tok::cyno_mist(), 0.42 * tasks_card_ht),
                fade(tok::cyno_orchid(), 0.16 * tasks_card_ht),
            );
        }
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        p.text("Tasks Open", br_x + 14.0, bot_y + 22.0, 10.0, Weight::Semibold, tok::text2());
        let task_num = format!("{} active", pending);
        p.text(&task_num, br_x + 14.0, bot_y + 46.0, 13.5, Weight::Bold, tok::cyno_mist());
        p.hit(br_x, bot_y, sub_w - 42.0, bot_h, Action::SetMode(NotchMode::Tasks));

        let shot_x = br_x + sub_w - 40.0;
        let shot_y = bot_y + 21.5;
        let shot_ht = p.hot_t(shot_x, shot_y, 28.0, 28.0);
        p.tactile_surface_pill_t(shot_x, shot_y, 28.0, 28.0, 14.0, shot_ht, None);
        icons::draw(
            p,
            Icon::Video,
            shot_x + 7.5,
            shot_y + 7.5 - 0.55 * shot_ht,
            13.0,
            tok::cyno_violet_hi(),
        );
        p.hit(shot_x, shot_y, 28.0, 28.0, Action::CaptureScreen);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 1. MUSIC PLAYER (Aligned 154x154 Art + Real Multi-Line Lyrics View)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_music_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let card_x = 20.0;
        let card_size = 154.0;

        // Luminous Volumetric Album Art Cover (matches right card height symmetrically!)
        p.luminous_pill_button(
            card_x,
            card_y,
            card_size,
            card_size,
            22.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            false,
        );
        p.dot_matrix(card_x + 16.0, card_y + 16.0, 4, 3, 5.5, rgba(255, 255, 255, 55));

        if state.show_mascot && state.media.is_playing {
            let hero_cx = card_x + card_size * 0.5;
            let hero_cy = card_y + 62.0;
            p.circle(hero_cx, hero_cy - 1.5, 41.0, rgba(10, 9, 20, 145));
            p.circle_stroke(hero_cx, hero_cy - 1.5, 41.0, 1.0, rgba(255, 255, 255, 45));
        } else {
            icons::draw(p, Icon::Music, card_x + (card_size - 42.0) / 2.0, card_y + 42.0 + 1.5, 42.0, rgba(0, 0, 0, 85));
            icons::draw(p, Icon::Music, card_x + (card_size - 42.0) / 2.0, card_y + 42.0, 42.0, rgba(255, 255, 255, 255));
        }

        let art_badge = if !state.media.album.is_empty() {
            state.media.album.as_str()
        } else if state.media.is_idle() {
            "IDLE"
        } else if state.media.is_playing {
            "LIVE AUDIO"
        } else {
            "PAUSED"
        };
        let badge_txt = p.ellipsize(art_badge, 9.5, Weight::Bold, card_size - 28.0);
        p.rrect(card_x + 16.0, card_y + 116.0, card_size - 32.0, 22.0, 11.0, rgba(10, 8, 22, 150));
        p.text_center(&badge_txt, card_x + card_size / 2.0, card_y + 130.5, 9.5, Weight::Bold, tok::text1());

        let info_x = card_x + card_size + 14.0;
        let info_w = (w - 20.0) - info_x;

        p.elevated_card(
            info_x,
            card_y,
            info_w,
            card_size,
            16.0,
            Some((info_w - 22.0, 22.0, 80.0, tok::cyno_violet())),
        );

        let pad_x = AppState::MUSIC_SCRUBBER_X;
        let inner_w = AppState::MUSIC_SCRUBBER_W;

        if state.show_lyrics {
            Self::draw_lyrics_panel(p, state, pad_x, card_y + 8.0, inner_w, 101.0, false);

            let played_pct = if state.media.duration_ms > 0 {
                (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mini_w = inner_w - 138.0;
            p.glowing_striped_bar(pad_x, card_y + 123.0, mini_w, 6.0, played_pct, tok::cyno_deep(), tok::cyno_violet());
            for s in 0..8 {
                let sw = mini_w / 8.0;
                p.hit(pad_x + s as f32 * sw, card_y + 115.0, sw, 22.0, Action::SeekMusic((s as f32 + 0.5) / 8.0));
            }
            Self::draw_transport_buttons(p, state.media.is_playing, pad_x + inner_w - 60.0, card_y + 112.0);
            return;
        }

        // Standard Now Playing View
        let title_y = card_y + 26.0;
        let title_max_w = if state.equalizer_enabled { inner_w - 72.0 } else { inner_w };
        let title_text = p.ellipsize(&state.media.title, 15.5, Weight::Bold, title_max_w);
        p.text(&title_text, pad_x, title_y, 15.5, Weight::Bold, tok::text1());

        let artist_y = title_y + 19.0;
        let artist_txt = p.ellipsize(&state.media.artist, 12.0, Weight::Regular, inner_w);
        p.text(&artist_txt, pad_x, artist_y, 12.0, Weight::Regular, tok::text2());

        let prog_y = artist_y + 13.0;
        let played_pct = if state.media.duration_ms > 0 {
            (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.glowing_striped_bar(pad_x, prog_y, inner_w, 6.5, played_pct, tok::cyno_deep(), tok::cyno_violet());

        let seg_count = 10;
        let seg_w = inner_w / seg_count as f32;
        for s in 0..seg_count {
            let frac = (s as f32 + 0.5) / seg_count as f32;
            p.hit(pad_x + (s as f32 * seg_w), prog_y - 6.0, seg_w, 19.0, Action::SeekMusic(frac));
        }

        let time_y = prog_y + 17.0;
        let pos_sec = state.media.position_ms / 1000;
        let dur_sec = state.media.duration_ms / 1000;
        let rem_sec = dur_sec.saturating_sub(pos_sec);
        let left_time = format!("{}:{:02}", pos_sec / 60, pos_sec % 60);
        let right_time = if state.media.duration_ms > 0 {
            format!("-{}:{:02}", rem_sec / 60, rem_sec % 60)
        } else {
            "--:--".to_string()
        };
        p.text(&left_time, pad_x, time_y, 10.0, Weight::Semibold, tok::text2());
        p.text_right(&right_time, pad_x + inner_w, time_y, 10.0, Weight::Semibold, tok::text2());

        let lyric_y = time_y + 16.0;
        let (snip_lines, snip_sz) =
            p.wrap_lines_fit(&state.current_lyric_text, 12.0, 9.5, Weight::Regular, inner_w, 2);
        for (li, line) in snip_lines.iter().take(2).enumerate() {
            p.text(
                line,
                pad_x,
                lyric_y + (li as f32) * (snip_sz + 2.0),
                snip_sz,
                Weight::Regular,
                tok::text_soft(),
            );
        }

        let ctrl_cx = pad_x + inner_w / 2.0;
        let ctrl_y = card_y + 113.0;
        Self::draw_transport_buttons(p, state.media.is_playing, ctrl_cx, ctrl_y);
    }

    fn draw_lyrics_panel(
        p: &mut Painter,
        state: &AppState,
        pad_x: f32,
        top_y: f32,
        inner_w: f32,
        body_h: f32,
        is_vertical: bool,
    ) {
        let chip_h = 20.0;
        let style_w = if is_vertical { 58.0 } else { 64.0 };
        let size_w = if is_vertical { 28.0 } else { 32.0 };
        let chip_gap = 6.0;
        let style_x = pad_x + inner_w - style_w;
        let size_x = style_x - chip_gap - size_w;

        let max_title_w = (size_x - pad_x - 10.0).max(48.0);
        let title_mini = p.ellipsize(&state.media.title, 12.2, Weight::Bold, max_title_w);
        p.text(&title_mini, pad_x, top_y + 13.5, 12.2, Weight::Bold, tok::text1());

        let size_ht = p.hot_t(size_x, top_y, size_w, chip_h);
        p.tactile_surface_pill_t(size_x, top_y, size_w, chip_h, 10.0, size_ht, Some(tok::cyno_orchid()));
        p.text_center(
            state.lyric_size.short_label(),
            size_x + size_w * 0.5,
            top_y + 13.4 - 0.45 * size_ht,
            9.2,
            Weight::Bold,
            tok::cyno_mist(),
        );
        p.hit(size_x, top_y, size_w, chip_h, Action::CycleLyricSize);

        let style_ht = p.hot_t(style_x, top_y, style_w, chip_h);
        p.tactile_surface_pill_t(style_x, top_y, style_w, chip_h, 10.0, style_ht, Some(tok::cyno_violet()));
        p.text_center(
            state.lyric_style.short_label(),
            style_x + style_w * 0.5,
            top_y + 13.4 - 0.45 * style_ht,
            9.2,
            Weight::Bold,
            tok::cyno_violet_hi(),
        );
        p.hit(style_x, top_y, style_w, chip_h, Action::CycleLyricStyle);

        let ctx_y = top_y + 22.0;
        let ctx_h = (body_h - 22.0).max(56.0);
        let center_y = ctx_y + ctx_h * 0.5;

        if state.lyrics.is_empty() {
            p.text_center(
                "No synced lyrics found for this track yet.",
                pad_x + inner_w * 0.5,
                center_y + 4.0,
                12.2,
                Weight::Regular,
                tok::text2(),
            );
            return;
        }

        let base_idx = state.current_lyric_index.unwrap_or(0) as i32;
        let max_idx = (state.lyrics.len() as i32) - 1;
        let center_idx = (base_idx + state.lyrics_offset).clamp(0, max_idx) as usize;

        let (prev_sz, act_sz) = match state.lyric_size {
            LyricSize::Compact => (11.2, 13.5),
            LyricSize::Standard => (12.2, 15.4),
            LyricSize::Large => (13.4, 17.4),
        };

        match state.lyric_style {
            LyricStyle::ClassicList | LyricStyle::KaraokeGlow => {
                let (act_lines, fit_act_sz) = p.wrap_lines_fit(
                    &state.lyrics[center_idx].text,
                    act_sz,
                    10.8,
                    Weight::Bold,
                    inner_w - 22.0,
                    2,
                );
                let line_cnt = act_lines.len().max(1);
                let line_step = fit_act_sz * 1.15;
                let base_pill_h = if line_cnt == 1 {
                    (fit_act_sz + 13.0).clamp(26.0, 34.0)
                } else {
                    (line_cnt as f32 * line_step + 10.0).clamp(36.0, 48.0)
                };
                let pill_h = if state.lyric_style == LyricStyle::KaraokeGlow {
                    (base_pill_h + 3.0).min(ctx_h * 0.58)
                } else {
                    base_pill_h.min(ctx_h * 0.56)
                };
                let pill_y = center_y - pill_h * 0.5;
                let neighbor_max_lines = if is_vertical && line_cnt == 1 { 2 } else { 1 };

                if center_idx > 0 {
                    let (prev_lines, fit_prev_sz) = p.wrap_lines_fit(
                        &state.lyrics[center_idx - 1].text,
                        prev_sz,
                        9.2,
                        Weight::Regular,
                        inner_w - 10.0,
                        neighbor_max_lines,
                    );
                    let p_cnt = prev_lines.len().max(1);
                    let p_step = fit_prev_sz * 1.14;
                    let last_base = (pill_y - 5.0).max(ctx_y + (p_cnt as f32) * p_step - 2.0);
                    for (li, line) in prev_lines.iter().enumerate() {
                        let by = last_base - ((p_cnt - 1 - li) as f32) * p_step;
                        p.text_center(line, pad_x + inner_w * 0.5, by, fit_prev_sz, Weight::Regular, tok::text2());
                    }
                    p.hit(pad_x, ctx_y, inner_w, (pill_y - ctx_y).max(16.0), Action::ScrollLyrics(-1));
                }

                let text_block_h = (line_cnt as f32 - 1.0) * line_step;
                if state.lyric_style == LyricStyle::KaraokeGlow {
                    p.luminous_pill_button(
                        pad_x,
                        pill_y,
                        inner_w,
                        pill_h,
                        9.5,
                        tok::cyno_indigo(),
                        tok::cyno_violet(),
                        tok::cyno_orchid(),
                        false,
                    );
                    let cur_sec = state.lyrics[center_idx].seconds;
                    let next_sec = state.lyrics.get(center_idx + 1).map(|l| l.seconds).unwrap_or(cur_sec + 4.0);
                    let span = (next_sec - cur_sec).max(1.0);
                    let pos_sec = state.media.position_ms as f64 / 1000.0;
                    let karaoke_pct = if state.lyrics_offset == 0 && state.media.duration_ms > 0 && pos_sec >= cur_sec {
                        ((pos_sec - cur_sec) / span).clamp(0.14, 1.0) as f32
                    } else {
                        0.64
                    };
                    p.rrect(pad_x + 10.0, pill_y + pill_h - 4.0, inner_w - 20.0, 2.2, 1.1, rgba(10, 8, 22, 135));
                    p.rrect(pad_x + 10.0, pill_y + pill_h - 4.0, (inner_w - 20.0) * karaoke_pct, 2.2, 1.1, tok::cyno_mist());

                    let first_base = pill_y + (pill_h - 2.5) * 0.5 - text_block_h * 0.5 + fit_act_sz * 0.33;
                    for (li, line) in act_lines.iter().enumerate() {
                        p.text_center_shadow(
                            line,
                            pad_x + inner_w * 0.5,
                            first_base + (li as f32) * line_step,
                            fit_act_sz,
                            Weight::Bold,
                            tok::text1(),
                        );
                    }
                } else {
                    p.tactile_surface_pill(pad_x, pill_y, inner_w, pill_h, 9.0, false, Some(tok::cyno_violet()));
                    let first_base = pill_y + pill_h * 0.5 - text_block_h * 0.5 + fit_act_sz * 0.34;
                    for (li, line) in act_lines.iter().enumerate() {
                        p.text_center_shadow(
                            line,
                            pad_x + inner_w * 0.5,
                            first_base + (li as f32) * line_step,
                            fit_act_sz,
                            Weight::Bold,
                            tok::text1(),
                        );
                    }
                }

                if center_idx + 1 < state.lyrics.len() {
                    let (next_lines, fit_next_sz) = p.wrap_lines_fit(
                        &state.lyrics[center_idx + 1].text,
                        prev_sz,
                        9.2,
                        Weight::Regular,
                        inner_w - 10.0,
                        neighbor_max_lines,
                    );
                    let n_step = fit_next_sz * 1.14;
                    let first_next_y = (pill_y + pill_h + fit_next_sz + 3.0).min(ctx_y + ctx_h - 2.0);
                    for (li, line) in next_lines.iter().enumerate() {
                        let by = first_next_y + (li as f32) * n_step;
                        if by <= ctx_y + ctx_h + 2.0 {
                            p.text_center(line, pad_x + inner_w * 0.5, by, fit_next_sz, Weight::Regular, tok::text2());
                        }
                    }
                    p.hit(
                        pad_x,
                        pill_y + pill_h + 1.0,
                        inner_w,
                        (ctx_y + ctx_h - (pill_y + pill_h)).max(16.0),
                        Action::ScrollLyrics(1),
                    );
                }
            }
            LyricStyle::MinimalSingle => {
                let (hero_sz, cue_sz) = match state.lyric_size {
                    LyricSize::Compact => (14.8, 11.4),
                    LyricSize::Standard => (17.2, 12.4),
                    LyricSize::Large => (19.6, 13.4),
                };
                p.radial_glow(
                    pad_x + inner_w * 0.5,
                    center_y - 2.0,
                    inner_w * 0.36,
                    rgba(139, 92, 246, 38),
                    BlendMode::SourceOver,
                    true,
                );
                let (hero_lines, fit_hero_sz) = p.wrap_lines_fit(
                    &state.lyrics[center_idx].text,
                    hero_sz,
                    11.5,
                    Weight::Bold,
                    inner_w - 12.0,
                    2,
                );
                let h_cnt = hero_lines.len().max(1);
                let h_step = fit_hero_sz * 1.15;
                let hero_top_base = if h_cnt == 1 {
                    center_y - 4.0
                } else {
                    center_y - h_step * 0.65
                };
                for (li, line) in hero_lines.iter().enumerate() {
                    p.text_center_shadow(
                        line,
                        pad_x + inner_w * 0.5,
                        hero_top_base + (li as f32) * h_step,
                        fit_hero_sz,
                        Weight::Bold,
                        tok::text1(),
                    );
                }
                let last_hero_base = hero_top_base + ((h_cnt - 1) as f32) * h_step;
                let bar_y = last_hero_base + 5.5;
                p.rrect(pad_x + inner_w * 0.5 - 18.0, bar_y, 36.0, 2.2, 1.1, tok::cyno_mist());
                let cue = state
                    .lyrics
                    .get(center_idx + 1)
                    .map(|l| l.text.as_str())
                    .unwrap_or("End of lyrics");
                let cue_max_lines = if h_cnt == 1 { 2 } else { 1 };
                let (cue_lines, fit_cue_sz) =
                    p.wrap_lines_fit(cue, cue_sz, 9.4, Weight::Regular, inner_w - 16.0, cue_max_lines);
                for (li, line) in cue_lines.iter().enumerate() {
                    let cy = bar_y + 3.0 + fit_cue_sz + (li as f32) * (fit_cue_sz * 1.14);
                    if cy <= ctx_y + ctx_h + 2.0 {
                        p.text_center(line, pad_x + inner_w * 0.5, cy, fit_cue_sz, Weight::Regular, tok::text2());
                    }
                }
                p.hit(pad_x, ctx_y, inner_w * 0.5, ctx_h, Action::ScrollLyrics(-1));
                p.hit(pad_x + inner_w * 0.5, ctx_y, inner_w * 0.5, ctx_h, Action::ScrollLyrics(1));
            }
            LyricStyle::CompactStream => {
                let (base_font, act_font): (f32, f32) = match state.lyric_size {
                    LyricSize::Compact => (11.4, 12.4),
                    LyricSize::Standard => (12.6, 13.8),
                    LyricSize::Large => (14.0, 15.2),
                };
                let max_slots = if state.lyric_size == LyricSize::Large && !is_vertical { 3 } else { 4 };
                let start_idx = if state.lyrics.len() <= max_slots {
                    0
                } else {
                    center_idx.saturating_sub(1).min(state.lyrics.len() - max_slots)
                };
                let mut cur_y = ctx_y;
                for slot in 0..max_slots {
                    let idx = start_idx + slot;
                    if idx >= state.lyrics.len() {
                        break;
                    }
                    let is_cur = idx == center_idx;
                    let target_sz = if is_cur { act_font } else { base_font };
                    let weight = if is_cur { Weight::Bold } else { Weight::Regular };
                    let allow_lines = if is_cur { 2 } else { 1 };
                    let (lines, fit_sz) = p.wrap_lines_fit(
                        &state.lyrics[idx].text,
                        target_sz,
                        9.2,
                        weight,
                        inner_w - 20.0,
                        allow_lines,
                    );
                    let l_cnt = lines.len().max(1);
                    let l_step = fit_sz * 1.15;
                    let item_h = (l_cnt as f32) * l_step + 5.0;
                    if slot > 0 && cur_y + item_h > ctx_y + ctx_h + 4.0 {
                        break;
                    }
                    if is_cur {
                        p.rrect(pad_x, cur_y, inner_w, item_h - 1.0, 6.5, rgba(124, 58, 237, 58));
                        p.rrect(pad_x + 4.0, cur_y + 3.5, 2.8, (item_h - 8.0).max(6.0), 1.4, tok::cyno_mist());
                    }
                    for (li, line) in lines.iter().enumerate() {
                        p.text(
                            line,
                            pad_x + 12.0,
                            cur_y + 2.0 + fit_sz + (li as f32) * l_step,
                            fit_sz,
                            weight,
                            if is_cur { tok::text1() } else { tok::text2() },
                        );
                    }
                    if !is_cur {
                        p.hit(pad_x, cur_y, inner_w, item_h, Action::ScrollLyrics(idx as i32 - center_idx as i32));
                    }
                    cur_y += item_h;
                }
            }
        }
    }

    fn draw_transport_buttons(p: &mut Painter, is_playing: bool, ctrl_cx: f32, ctrl_y: f32) {
        let prev_x = ctrl_cx - 60.0;
        let prev_ht = p.hot_t(prev_x, ctrl_y, 32.0, 26.0);
        p.tactile_surface_pill_t(prev_x, ctrl_y, 32.0, 26.0, 13.0, prev_ht, None);
        icons::draw(p, Icon::Prev, prev_x + 8.5, ctrl_y + 5.5 - 0.55 * prev_ht, 15.0, tok::text1());
        p.hit(prev_x, ctrl_y, 32.0, 26.0, Action::PrevTrack);

        let play_w = 46.0;
        let play_x = ctrl_cx - play_w / 2.0;
        let play_ht = p.hot_t(play_x, ctrl_y - 2.0, play_w, 30.0);
        p.luminous_pill_button_t(
            play_x,
            ctrl_y - 2.0,
            play_w,
            30.0,
            15.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            play_ht,
        );
        let play_icon = if is_playing { Icon::Pause } else { Icon::Play };
        let dy = -0.8 * play_ht;
        icons::draw(p, play_icon, play_x + 15.5, ctrl_y + 5.5 + dy, 15.0, rgba(0, 0, 0, 110));
        icons::draw(p, play_icon, play_x + 15.5, ctrl_y + 5.0 + dy, 15.0, rgba(255, 255, 255, 255));
        p.hit(play_x, ctrl_y - 2.0, play_w, 30.0, Action::TogglePlayPause);

        let next_x = ctrl_cx + 28.0;
        let next_ht = p.hot_t(next_x, ctrl_y, 32.0, 26.0);
        p.tactile_surface_pill_t(next_x, ctrl_y, 32.0, 26.0, 13.0, next_ht, None);
        icons::draw(p, Icon::Next, next_x + 8.5, ctrl_y + 5.5 - 0.55 * next_ht, 15.0, tok::text1());
        p.hit(next_x, ctrl_y, 32.0, 26.0, Action::NextTrack);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 2. VOLUME MASTER VIEW
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_volume_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let margin_x = 20.0;
        let card_h = 154.0;
        let left_w = 148.0;
        let gap = 14.0;
        let right_w = w - (margin_x * 2.0) - left_w - gap;
        let right_x = margin_x + left_w + gap;

        let vol_int = (state.volume_pct * 100.0).round() as i32;
        let is_muted = vol_int == 0;

        let aurora_col = if is_muted { rgba(239, 68, 68, 120) } else { tok::cyno_violet() };
        p.elevated_card(margin_x, card_y, left_w, card_h, 16.0, Some((left_w * 0.5, 34.0, 58.0, aurora_col)));

        let ic_cx = margin_x + left_w / 2.0;
        let ic_cy = card_y + 34.0;
        p.tactile_surface_pill(ic_cx - 18.0, ic_cy - 18.0, 36.0, 36.0, 18.0, false, Some(aurora_col));
        let vol_icon_col = if is_muted { rgba(252, 165, 165, 255) } else { tok::text1() };
        icons::draw(p, Icon::Volume, ic_cx - 9.0, ic_cy - 9.0, 18.0, vol_icon_col);

        let pct_str = format!("{}%", vol_int);
        p.text_center(&pct_str, margin_x + left_w / 2.0, card_y + 82.0, 23.0, Weight::Bold, tok::text1());

        let mute_btn_w = 104.0;
        let mute_btn_h = 28.0;
        let mute_btn_x = margin_x + (left_w - mute_btn_w) / 2.0;
        let mute_btn_y = card_y + 106.0;
        let mute_ht = p.hot_t(mute_btn_x, mute_btn_y, mute_btn_w, mute_btn_h);

        if is_muted {
            p.luminous_pill_button_t(
                mute_btn_x,
                mute_btn_y,
                mute_btn_w,
                mute_btn_h,
                14.0,
                rgba(153, 27, 27, 255),
                rgba(220, 38, 38, 255),
                rgba(180, 83, 9, 255),
                mute_ht,
            );
        } else {
            p.tactile_surface_pill_t(mute_btn_x, mute_btn_y, mute_btn_w, mute_btn_h, 14.0, mute_ht, None);
        }
        let mute_label = if is_muted { "Unmute" } else { "Mute" };
        let mute_ty = mute_btn_y + 18.0 - if is_muted { 0.8 * mute_ht } else { 0.55 * mute_ht };
        p.text_center_shadow(mute_label, mute_btn_x + mute_btn_w / 2.0, mute_ty, 11.0, Weight::Semibold, tok::text1());
        p.hit(mute_btn_x, mute_btn_y, mute_btn_w, mute_btn_h, Action::ToggleMute);

        // Right Card
        p.elevated_card(
            right_x,
            card_y,
            right_w,
            card_h,
            16.0,
            Some((right_w - 30.0, 24.0, 88.0, tok::cyno_orchid())),
        );
        p.dot_matrix(right_x + right_w - 50.0, card_y + 14.0, 6, 3, 5.5, rgba(255, 255, 255, 32));

        p.text("System Volume Master", right_x + 18.0, card_y + 29.0, 14.0, Weight::Bold, tok::text1());
        p.text("Windows Core Audio • Drag slider or scroll wheel", right_x + 18.0, card_y + 45.0, 10.5, Weight::Regular, tok::text2());

        let slider_x = AppState::VOLUME_SLIDER_X;
        let slider_y = card_y + 66.0;
        let slider_w = AppState::VOLUME_SLIDER_W;
        let slider_h = 14.0;
        let pct_clamped = if smooth {
            state.anim_vol_pct.clamp(0.0, 1.0)
        } else {
            state.volume_pct.clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(slider_x, slider_y, slider_w, slider_h, pct_clamped, tok::cyno_deep(), tok::cyno_violet());

        if pct_clamped > 0.01 {
            let thumb_x = (slider_x + slider_w * pct_clamped).clamp(slider_x + 8.0, slider_x + slider_w - 4.0);
            let thumb_y = slider_y + slider_h / 2.0;
            p.radial_glow(thumb_x, thumb_y, 14.0, rgba(255, 255, 255, 110), BlendMode::Plus, false);
            p.circle(thumb_x, thumb_y, 8.5, rgba(255, 255, 255, 255));
            p.circle(thumb_x, thumb_y, 4.0, tok::cyno_deep());
        }

        let seg_count = 10;
        let seg_w = slider_w / seg_count as f32;
        for s in 0..seg_count {
            let frac = (s as f32 + 0.5) / seg_count as f32;
            p.hit(slider_x + (s as f32 * seg_w), slider_y - 6.0, seg_w, 26.0, Action::SetVolume(frac));
        }

        let presets: [(f32, &str); 5] = [(0.0, "0%"), (0.25, "25%"), (0.50, "50%"), (0.75, "75%"), (1.0, "100%")];
        let pb_gap = 8.0;
        let pb_w = (slider_w - (pb_gap * 4.0)) / 5.0;
        let pb_y = card_y + 100.0;
        let pb_h = 30.0;

        for (i, &(target_pct, label)) in presets.iter().enumerate() {
            let px = slider_x + (i as f32 * (pb_w + pb_gap));
            let is_cur = (state.volume_pct - target_pct).abs() < 0.12;
            let ht = p.hot_t(px, pb_y, pb_w, pb_h);

            if is_cur {
                p.luminous_pill_button_t(
                    px,
                    pb_y,
                    pb_w,
                    pb_h,
                    9.5,
                    tok::cyno_indigo(),
                    tok::cyno_violet(),
                    tok::cyno_orchid(),
                    ht,
                );
                p.text_center_shadow(label, px + pb_w / 2.0, pb_y + 19.0 - 0.8 * ht, 11.0, Weight::Bold, tok::text1());
            } else {
                p.tactile_surface_pill_t(px, pb_y, pb_w, pb_h, 9.5, ht, None);
                let tc = lerp_color(tok::text_soft(), tok::text1(), ht);
                p.text_center(label, px + pb_w / 2.0, pb_y + 19.0 - 0.55 * ht, 11.0, Weight::Semibold, tc);
            }
            p.hit(px, pb_y, pb_w, pb_h, Action::SetVolume(target_pct));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 3. MEMORY REDUCT VIEW (Full 8-Area Parity + Pool Metrics + Disable Triggers)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_memreduct_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let margin_x = 20.0;
        let grid_w = w - (margin_x * 2.0);
        let col_gap = 14.0;
        let row_gap = 12.0;
        let col_w = (grid_w - col_gap) / 2.0;
        let card_h = 71.0;

        let col1_x = margin_x;
        let col2_x = margin_x + col_w + col_gap;
        let row1_y = card_y;
        let row2_y = row1_y + card_h + row_gap;

        // Card 1: Physical + Pagefile + SysCache + Elevation badge
        p.elevated_card(col1_x, row1_y, col_w, card_h, 15.0, Some((col_w - 20.0, 15.0, 62.0, tok::cyno_violet())));
        p.text("Physical Memory", col1_x + 14.0, row1_y + 17.5, 10.2, Weight::Semibold, tok::text2());
        let right_stat = if state.mem_reduct.is_purging {
            "Cleaning...".to_string()
        } else if state.mem_reduct.last_freed_mb > 0.0 {
            format!("Last: {:.0} MB", state.mem_reduct.last_freed_mb)
        } else if state.telemetry.has_admin_privileges {
            "Admin Mode".to_string()
        } else {
            "User Mode".to_string()
        };
        p.text_right(&right_stat, col1_x + col_w - 14.0, row1_y + 17.5, 9.8, Weight::Bold, tok::cyno_mist());

        let ram_val = format!(
            "{:.1} / {:.1} GB ({:.0}%)",
            state.telemetry.memory_used_gb,
            state.telemetry.memory_total_gb,
            state.telemetry.memory_percent
        );
        p.text(&ram_val, col1_x + 14.0, row1_y + 36.5, 14.0, Weight::Bold, tok::text1());

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(col1_x + 14.0, row1_y + 43.5, col_w - 28.0, 6.0, ram_pct, tok::cyno_deep(), tok::cyno_violet_hi());

        let pools_str = format!(
            "Page {:.0}% • SysCache {:.0} MB",
            state.telemetry.pagefile_percent,
            state.telemetry.sys_cache_mb
        );
        let pools_ell = p.ellipsize(&pools_str, 9.2, Weight::Regular, col_w - 28.0);
        p.text(&pools_ell, col1_x + 14.0, row1_y + 62.0, 9.2, Weight::Regular, tok::text2());

        // Card 2: All 8 Mem Reduct Clean Areas + Auto Clean Toggle
        p.elevated_card(col2_x, row1_y, col_w, card_h, 15.0, None);
        p.text("Clean Areas", col2_x + 14.0, row1_y + 16.0, 10.0, Weight::Semibold, tok::text2());

        let auto_w = 68.0;
        let auto_x = col2_x + col_w - auto_w - 12.0;
        let auto_y = row1_y + 6.0;
        let auto_h = 16.0;
        let auto_ht = p.hot_t(auto_x, auto_y, auto_w, auto_h);
        p.tactile_surface_pill_t(
            auto_x,
            auto_y,
            auto_w,
            auto_h,
            8.0,
            auto_ht,
            if state.mem_reduct.auto_clean_enabled { Some(tok::cyno_violet()) } else { None },
        );
        let auto_lbl = if state.mem_reduct.auto_clean_enabled { "Auto: ON" } else { "Auto: OFF" };
        p.text_center(
            auto_lbl,
            auto_x + auto_w / 2.0,
            auto_y + 11.0 - 0.45 * auto_ht,
            9.0,
            Weight::Bold,
            if state.mem_reduct.auto_clean_enabled { tok::text1() } else { lerp_color(tok::text2(), tok::text1(), auto_ht) },
        );
        p.hit(auto_x, auto_y, auto_w, auto_h, Action::ToggleAutoClean);

        let a_gap_x = 4.5;
        let a_gap_y = 4.0;
        let pill_w = (col_w - 28.0 - a_gap_x * 3.0) / 4.0;
        let pill_h = 17.0;
        let areas = [
            ("WS", state.mem_reduct.clean_working_sets, Action::ToggleArea(0)),
            ("SysWS", state.mem_reduct.clean_system_ws, Action::ToggleArea(1)),
            ("Standby", state.mem_reduct.clean_standby_list, Action::ToggleArea(2)),
            ("ModPg", state.mem_reduct.clean_modified_page_list, Action::ToggleArea(3)),
            ("StbyLo", state.mem_reduct.clean_standby_low, Action::ToggleArea(4)),
            ("ModFile", state.mem_reduct.clean_modified_file_cache, Action::ToggleArea(5)),
            ("RegCch", state.mem_reduct.clean_registry_cache, Action::ToggleArea(6)),
            ("Combine", state.mem_reduct.clean_combine_memory_lists, Action::ToggleArea(7)),
        ];

        for (idx, (label, active, act)) in areas.iter().enumerate() {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let px = col2_x + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = row1_y + 26.0 + (row_i as f32 * (pill_h + a_gap_y));
            let ht = p.hot_t(px, py, pill_w, pill_h);
            p.tactile_surface_pill_t(
                px,
                py,
                pill_w,
                pill_h,
                6.0,
                ht,
                if *active { Some(tok::cyno_violet()) } else { None },
            );
            p.text_center(
                label,
                px + pill_w / 2.0,
                py + 11.5 - 0.45 * ht,
                8.5,
                if *active { Weight::Bold } else { Weight::Semibold },
                if *active { tok::text1() } else { lerp_color(tok::text2(), tok::text1(), ht) },
            );
            p.hit(px, py, pill_w, pill_h, *act);
        }

        // Card 3: Rules (Independent Threshold & Interval triggers with Disabled option)
        p.elevated_card(col1_x, row2_y, col_w, card_h, 15.0, None);
        p.text("Clean Above", col1_x + 14.0, row2_y + 19.5, 10.0, Weight::Semibold, tok::text2());
        p.text_right("Clean Every", col1_x + col_w - 14.0, row2_y + 19.5, 10.0, Weight::Semibold, tok::text2());

        let chip_y = row2_y + 28.0;
        let chip_h = 30.0;
        let chip_w = (col_w - 28.0 - 8.0) / 2.0;
        let chip1_x = col1_x + 14.0;
        let chip2_x = chip1_x + chip_w + 8.0;

        let c1_ht = p.hot_t(col1_x, row2_y, col_w / 2.0, card_h);
        p.tactile_surface_pill_t(chip1_x, chip_y, chip_w, chip_h, 8.5, c1_ht, None);
        let (th_str, th_col) = if state.mem_reduct.when_above_pct == 0 {
            ("Disabled".to_string(), tok::text2())
        } else {
            (format!("> {}% RAM", state.mem_reduct.when_above_pct), tok::text1())
        };
        p.text_center(&th_str, chip1_x + chip_w / 2.0, chip_y + 19.0 - 0.55 * c1_ht, 11.5, Weight::Bold, th_col);
        p.hit(col1_x, row2_y, col_w / 2.0, card_h, Action::CycleThreshold);

        let c2_ht = p.hot_t(col1_x + col_w / 2.0, row2_y, col_w / 2.0, card_h);
        p.tactile_surface_pill_t(chip2_x, chip_y, chip_w, chip_h, 8.5, c2_ht, None);
        let (int_str, int_col) = if state.mem_reduct.every_mins == 0 {
            ("Disabled".to_string(), tok::text2())
        } else {
            (format!("Every {}m", state.mem_reduct.every_mins), tok::cyno_mist())
        };
        p.text_center(&int_str, chip2_x + chip_w / 2.0, chip_y + 19.0 - 0.55 * c2_ht, 11.5, Weight::Bold, int_col);
        p.hit(col1_x + col_w / 2.0, row2_y, col_w / 2.0, card_h, Action::CycleInterval);

        // Card 4: Clean Memory CTA with Live Status
        let purge_ht = p.hot_t(col2_x, row2_y, col_w, card_h);
        p.luminous_pill_button_t(
            col2_x,
            row2_y,
            col_w,
            card_h,
            15.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            purge_ht,
        );

        let dy = -0.8 * purge_ht;
        p.circle(col2_x + 30.0, row2_y + card_h / 2.0 + dy, 16.0, rgba(12, 7, 26, 165));
        p.circle_stroke(col2_x + 30.0, row2_y + card_h / 2.0 + dy, 16.0, 1.0, rgba(255, 255, 255, 130));
        if state.show_mascot && state.mascot_mood() == MascotMood::Purging {
            let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
            Self::draw_cyno_mascot(
                p,
                col2_x + 30.0,
                row2_y + card_h / 2.0 + dy + 0.5,
                0.72,
                MascotMood::Purging,
                anim_t,
                true,
            );
        } else {
            icons::draw(p, Icon::Zap, col2_x + 22.5, row2_y + card_h / 2.0 - 7.5 + dy, 15.0, tok::cyno_mist());
        }

        let cta_title = if state.mem_reduct.is_purging { "Cleaning Memory..." } else { "Clean Memory" };
        p.text_shadow(cta_title, col2_x + 54.0, row2_y + 31.0 + dy, 14.0, Weight::Bold, tok::text1());
        let status_sub = p.ellipsize(&state.mem_reduct.last_cleaned_status, 9.8, Weight::Semibold, col_w - 66.0);
        p.text_shadow(&status_sub, col2_x + 54.0, row2_y + 48.0 + dy, 9.8, Weight::Semibold, rgba(241, 245, 249, 245));
        p.hit(col2_x, row2_y, col_w, card_h, Action::PurgeRam);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 4. SCREEN TIME TELEMETRY VIEW (Real Hourly Sparkline + Multi-day Average)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_screentime_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let grid_w = w - (margin_x * 2.0);
        let col_gap = 14.0;
        let row_gap = 12.0;
        let col_w = (grid_w - col_gap) / 2.0;
        let card_h = 71.0;

        let col1_x = margin_x;
        let col2_x = margin_x + col_w + col_gap;
        let row1_y = card_y;
        let row2_y = row1_y + card_h + row_gap;

        // Card 1: Today
        p.elevated_card(col1_x, row1_y, col_w, card_h, 15.0, Some((col_w - 25.0, 15.0, 65.0, tok::cyno_violet())));
        p.dot_matrix(col1_x + col_w - 42.0, row1_y + 10.0, 5, 3, 5.0, rgba(255, 255, 255, 32));
        p.text("Today", col1_x + 16.0, row1_y + 21.0, 10.5, Weight::Semibold, tok::text2());
        let today_h = state.screen_time.today_secs / 3600;
        let today_m = (state.screen_time.today_secs % 3600) / 60;
        let today_str = format!("{} h {} m", today_h, today_m);
        p.text(&today_str, col1_x + 16.0, row1_y + 50.0, 19.0, Weight::Bold, tok::text1());

        // Card 2: Most used + runner-up
        p.elevated_card(col2_x, row1_y, col_w, card_h, 15.0, Some((col_w - 25.0, 15.0, 65.0, tok::cyno_orchid())));
        p.text("Most used", col2_x + 16.0, row1_y + 20.0, 10.5, Weight::Semibold, tok::text2());
        if let Some((second_name, second_secs)) = state.screen_time.top_apps.get(1) {
            let runner = p.ellipsize(&format!("#2 {} {}m", second_name, second_secs / 60), 9.5, Weight::Regular, col_w - 92.0);
            p.text_right(&runner, col2_x + col_w - 16.0, row1_y + 20.0, 9.5, Weight::Regular, tok::text2());
        }
        let top_name = p.ellipsize(&state.screen_time.top_app_name, 15.0, Weight::Bold, col_w - 96.0);
        p.text(&top_name, col2_x + 16.0, row1_y + 42.0, 15.0, Weight::Bold, tok::cyno_mist());
        let top_h = state.screen_time.top_app_secs / 3600;
        let top_m = (state.screen_time.top_app_secs % 3600) / 60;
        let top_str = format!("{} h {} m", top_h, top_m);
        p.text_right(&top_str, col2_x + col_w - 16.0, row1_y + 42.0, 11.5, Weight::Semibold, tok::text1());

        let share = if state.screen_time.today_secs > 0 {
            (state.screen_time.top_app_secs as f32 / state.screen_time.today_secs as f32).clamp(0.08, 1.0)
        } else {
            0.0
        };
        p.glowing_striped_bar(col2_x + 16.0, row1_y + 53.0, col_w - 32.0, 6.5, share, tok::cyno_deep(), tok::cyno_orchid());

        // Card 3: 7-day average
        p.elevated_card(col1_x, row2_y, col_w, card_h, 15.0, None);
        p.text("7-day average", col1_x + 16.0, row2_y + 21.0, 10.5, Weight::Semibold, tok::text2());
        let days_lbl = format!("{}d log", state.screen_time.tracked_days.max(1));
        p.text_right(&days_lbl, col1_x + col_w - 16.0, row2_y + 21.0, 9.5, Weight::Semibold, tok::text2());
        let avg_h = state.screen_time.seven_day_avg_secs / 3600;
        let avg_m = (state.screen_time.seven_day_avg_secs % 3600) / 60;
        let avg_str = format!("{} h {} m", avg_h, avg_m);
        p.text(&avg_str, col1_x + 16.0, row2_y + 50.0, 19.0, Weight::Bold, tok::cyno_violet_hi());

        // Card 4: Computed Peak hour + 12-bin 24h Sparkline
        p.elevated_card(col2_x, row2_y, col_w, card_h, 15.0, Some((col2_x * 0.1 + 40.0, 42.0, 50.0, tok::cyno_violet())));
        p.text("Peak hour", col2_x + 16.0, row2_y + 21.0, 10.5, Weight::Semibold, tok::text2());
        p.text(&state.screen_time.peak_hour_str, col2_x + 16.0, row2_y + 50.0, 19.0, Weight::Bold, tok::text1());

        // Mini 12-bin (2h buckets across 24h) activity chart on right side of Card 4
        let mut bins = [0u64; 12];
        for (h_idx, &secs) in state.screen_time.hourly_secs.iter().enumerate() {
            bins[h_idx / 2] += secs;
        }
        let max_bin = bins.iter().copied().max().unwrap_or(1).max(1);
        let peak_bin = state.screen_time.peak_hour / 2;
        let chart_x = col2_x + col_w - 124.0;
        let chart_base_y = row2_y + 57.0;
        let max_bar_h = 34.0;
        for (b, &val) in bins.iter().enumerate() {
            let bx = chart_x + (b as f32 * 9.0);
            let bh = if val == 0 {
                2.0
            } else {
                ((val as f32 / max_bin as f32) * max_bar_h).clamp(4.0, max_bar_h)
            };
            let bar_col = if val > 0 && b == peak_bin {
                tok::cyno_mist()
            } else if val > 0 {
                tok::cyno_violet()
            } else {
                rgba(255, 255, 255, 22)
            };
            p.rrect(bx, chart_base_y - bh, 5.5, bh, 2.5, bar_col);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 5. FOCUS TIMER (Luminous Ring + Tip Orb + Work/Break Phase & Cycle Count)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_focustimer_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let card_h = 154.0;
        let gap = 14.0;
        let left_w = 192.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;

        // Left Card: Circular Gauge
        p.elevated_card(
            margin_x,
            card_y,
            left_w,
            card_h,
            16.0,
            Some((left_w * 0.5, card_h * 0.5, 76.0, if state.timer.is_break { tok::cyan_hi() } else { tok::cyno_violet() })),
        );
        let cx = margin_x + left_w / 2.0;
        let cy = card_y + card_h / 2.0;
        let r = 52.0;

        let well = p.well_col();
        p.circle(cx, cy, r - 4.0, well);
        p.arc(cx, cy, r, 0.0, std::f32::consts::TAU, 6.0, rgba(255, 255, 255, 18));

        let frac = (state.timer.remaining_secs as f32 / state.timer.total_secs.max(1) as f32).clamp(0.0, 1.0);
        let ring_col = if state.timer.is_break { tok::cyan_hi() } else { tok::cyno_violet_hi() };
        let sweep = std::f32::consts::TAU * frac;
        let start_a = -std::f32::consts::FRAC_PI_2;
        p.arc(cx, cy, r, start_a, sweep, 10.0, fade(ring_col, 0.25));
        p.arc(cx, cy, r, start_a, sweep, 5.5, ring_col);

        // Luminous Gold/White Orb at end of arc
        if frac > 0.02 {
            let end_a = start_a + sweep;
            let ox = cx + r * end_a.cos();
            let oy = cy + r * end_a.sin();
            p.radial_glow(ox, oy, 11.0, tok::cyno_mist(), BlendMode::Plus, false);
            p.circle(ox, oy, 4.2, rgba(255, 255, 255, 255));
        }

        let mins = state.timer.remaining_secs / 60;
        let secs = state.timer.remaining_secs % 60;
        let time_str = format!("{:02}:{:02}", mins, secs);
        p.text_center(&time_str, cx, cy + 6.5, 22.0, Weight::Bold, tok::text1());

        let status = match (state.timer.is_break, state.timer.is_running) {
            (true, true) => "BREAK ACTIVE".to_string(),
            (true, false) => "BREAK READY".to_string(),
            (false, true) => format!("DEEP WORK • #{}", state.timer.cycles_completed + 1),
            (false, false) => format!("READY • #{}", state.timer.cycles_completed + 1),
        };
        p.text_center(&status, cx, cy + 23.0, 8.8, Weight::Bold, tok::cyno_mist());

        // Right Card: Duration Presets + Work/Break Toggle + Start/Reset
        p.elevated_card(right_x, card_y, right_w, card_h, 16.0, None);
        p.text("Session Duration", right_x + 18.0, card_y + 26.0, 13.0, Weight::Bold, tok::text1());
        p.text("Pick focus or switch to break", right_x + 18.0, card_y + 42.0, 10.0, Weight::Regular, tok::text2());

        // Phase Toggle pill top-right: [Work / Break]
        let ph_w = 82.0;
        let ph_x = right_x + right_w - ph_w - 16.0;
        let ph_y = card_y + 15.0;
        let ph_h = 24.0;
        let ph_ht = p.hot_t(ph_x, ph_y, ph_w, ph_h);
        p.tactile_surface_pill_t(
            ph_x,
            ph_y,
            ph_w,
            ph_h,
            12.0,
            ph_ht,
            if state.timer.is_break { Some(rgba(56, 189, 248, 220)) } else { None },
        );
        let ph_lbl = if state.timer.is_break {
            format!("Break {}m", state.default_break_mins)
        } else {
            "Work Mode".to_string()
        };
        p.text_center(&ph_lbl, ph_x + ph_w / 2.0, ph_y + 15.5 - 0.55 * ph_ht, 9.8, Weight::Bold, lerp_color(tok::text_soft(), tok::text1(), ph_ht));
        p.hit(ph_x, ph_y, ph_w, ph_h, Action::ToggleTimerPhase);

        let presets: [(u32, &str); 4] = [(5, "5m"), (15, "15m"), (25, "25m"), (45, "45m")];
        let p_gap = 8.0;
        let p_w = (right_w - 36.0 - (p_gap * 3.0)) / 4.0;
        let p_y = card_y + 57.0;
        let p_h = 28.0;

        for (i, &(m, lbl)) in presets.iter().enumerate() {
            let px = right_x + 18.0 + (i as f32 * (p_w + p_gap));
            let is_cur = state.timer.total_secs == m * 60;
            let ht = p.hot_t(px, p_y, p_w, p_h);

            p.tactile_surface_pill_t(
                px,
                p_y,
                p_w,
                p_h,
                8.0,
                ht,
                if is_cur { Some(tok::cyno_violet()) } else { None },
            );
            p.text_center(
                lbl,
                px + p_w / 2.0,
                p_y + 18.0 - 0.55 * ht,
                11.0,
                if is_cur { Weight::Bold } else { Weight::Semibold },
                if is_cur { tok::text1() } else { lerp_color(tok::text_soft(), tok::text1(), ht) },
            );
            p.hit(px, p_y, p_w, p_h, Action::SetFocusDuration(m));
        }

        let btn_y = card_y + 100.0;
        let btn_h = 36.0;
        let btn1_w = (right_w - 36.0 - 10.0) * 0.65;
        let btn2_w = (right_w - 36.0 - 10.0) * 0.35;
        let btn1_x = right_x + 18.0;
        let btn2_x = btn1_x + btn1_w + 10.0;

        let b1_ht = p.hot_t(btn1_x, btn_y, btn1_w, btn_h);
        p.luminous_pill_button_t(
            btn1_x,
            btn_y,
            btn1_w,
            btn_h,
            11.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            b1_ht,
        );
        let btn1_txt = if state.timer.is_running { "Pause Focus" } else { "Start Focus" };
        p.text_center_shadow(btn1_txt, btn1_x + btn1_w / 2.0, btn_y + 22.0 - 0.8 * b1_ht, 12.0, Weight::Bold, tok::text1());
        p.hit(btn1_x, btn_y, btn1_w, btn_h, Action::ToggleFocusTimer);

        let b2_ht = p.hot_t(btn2_x, btn_y, btn2_w, btn_h);
        p.tactile_surface_pill_t(btn2_x, btn_y, btn2_w, btn_h, 11.0, b2_ht, None);
        p.text_center(
            "Reset",
            btn2_x + btn2_w / 2.0,
            btn_y + 22.0 - 0.55 * b2_ht,
            12.0,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), b2_ht),
        );
        p.hit(btn2_x, btn_y, btn2_w, btn_h, Action::ResetFocusTimer);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 6. TASKS CHECKLIST VIEW (Scrollable + Individual Delete × + From Clip)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_tasks_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let card_w = w - (margin_x * 2.0);
        let card_h = 154.0;

        p.elevated_card(
            margin_x,
            card_y,
            card_w,
            card_h,
            16.0,
            Some((card_w - 40.0, 20.0, 84.0, tok::cyno_violet())),
        );

        icons::draw(p, Icon::List, margin_x + 18.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Tasks & Todos", margin_x + 44.0, card_y + 27.5, 13.5, Weight::Bold, tok::text1());

        let total = state.tasks.len();
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        let start_idx = state.tasks_scroll.min(total.saturating_sub(3));

        // Action pills
        let btn_h = 24.0;
        let btn_y = card_y + 15.0;
        let clear_w = 78.0;
        let clear_x = margin_x + card_w - clear_w - 16.0;
        let clear_ht = p.hot_t(clear_x, btn_y, clear_w, btn_h);
        p.tactile_surface_pill_t(clear_x, btn_y, clear_w, btn_h, 7.5, clear_ht, None);
        p.text_center("Clear Done", clear_x + clear_w / 2.0, btn_y + 16.0 - 0.55 * clear_ht, 10.0, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), clear_ht));
        p.hit(clear_x, btn_y, clear_w, btn_h, Action::ClearCompletedTasks);

        let add_w = 86.0;
        let add_x = clear_x - add_w - 8.0;
        let add_ht = p.hot_t(add_x, btn_y, add_w, btn_h);
        p.tactile_surface_pill_t(add_x, btn_y, add_w, btn_h, 7.5, add_ht, Some(tok::cyno_violet()));
        p.text_center("+ From Clip", add_x + add_w / 2.0, btn_y + 16.0 - 0.55 * add_ht, 10.0, Weight::Bold, tok::text1());
        p.hit(add_x, btn_y, add_w, btn_h, Action::PasteAsTask);

        let sub = if total > 3 {
            format!("{} open • {}-{} of {} (scroll)", pending, start_idx + 1, (start_idx + 3).min(total), total)
        } else {
            format!("{} open • toggle or × remove", pending)
        };
        let sub_max_w = (add_x - (margin_x + 44.0) - 10.0).max(60.0);
        let sub_ell = p.ellipsize(&sub, 10.0, Weight::Regular, sub_max_w);
        p.text(&sub_ell, margin_x + 44.0, card_y + 42.5, 10.0, Weight::Regular, tok::text2());

        if state.tasks.is_empty() {
            p.text_center(
                "All tasks clear! Copy text and click '+ From Clip' to add.",
                margin_x + card_w / 2.0,
                card_y + 98.0,
                11.5,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            for (slot, (real_idx, task)) in state.tasks.iter().enumerate().skip(start_idx).take(3).enumerate() {
                let item_y = card_y + 56.0 + (slot as f32 * 30.0);
                let item_h = 26.0;
                let item_x = margin_x + 16.0;
                let del_w = 26.0;
                let row_w = card_w - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 7.5, row_ht, None);

                let dy = -0.55 * row_ht;
                let check_cx = item_x + 15.0;
                let check_cy = item_y + item_h / 2.0 + dy;
                if task.completed {
                    p.radial_glow(check_cx, check_cy, 10.0, rgba(139, 92, 246, 95), BlendMode::Plus, false);
                    p.circle(check_cx, check_cy, 6.5, tok::cyno_violet());
                    icons::draw(p, Icon::Check, check_cx - 4.5, check_cy - 4.5, 9.0, rgba(255, 255, 255, 255));
                } else {
                    let well = p.well_col();
                    p.circle(check_cx, check_cy, 6.5, well);
                    p.circle_stroke(check_cx, check_cy, 6.5, 1.0, rgba(255, 255, 255, 70));
                }

                let task_text = p.ellipsize(&task.text, 11.5, Weight::Regular, row_w - 44.0);
                let text_color = if task.completed { tok::text3() } else { tok::text1() };
                p.text(&task_text, item_x + 30.0, item_y + 17.0 + dy, 11.5, Weight::Regular, text_color);
                p.hit(item_x, item_y, row_w, item_h, Action::ToggleTask(real_idx));

                // Delete pill button
                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 7.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(del_x, item_y - 0.55 * del_ht, del_w, item_h, 7.5, fade(rgba(220, 38, 38, 195), del_ht));
                }
                icons::draw(p, Icon::Close, del_x + 7.0, item_y + 7.0 - 0.55 * del_ht, 12.0, lerp_color(tok::text2(), tok::text1(), del_ht));
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteTask(real_idx));
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 7. SHELF (Persistent Clipboard History + Drag-and-Drop File Shelf)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_shelf_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let card_w = w - (margin_x * 2.0);
        let card_h = 154.0;

        p.elevated_card(
            margin_x,
            card_y,
            card_w,
            card_h,
            16.0,
            Some((card_w - 40.0, 20.0, 84.0, tok::cyno_orchid())),
        );

        icons::draw(p, Icon::Clipboard, margin_x + 18.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Clipboard & File Shelf", margin_x + 44.0, card_y + 27.5, 13.5, Weight::Bold, tok::text1());

        let total = state.shelf_items.len();
        let start_idx = state.shelf_scroll.min(total.saturating_sub(3));

        let clr_w = 64.0;
        let clr_h = 24.0;
        let clr_x = margin_x + card_w - clr_w - 16.0;
        let clr_y = card_y + 15.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, clr_h);
        p.tactile_surface_pill_t(clr_x, clr_y, clr_w, clr_h, 7.5, clr_ht, None);
        p.text_center("Clear", clr_x + clr_w / 2.0, clr_y + 16.0 - 0.55 * clr_ht, 10.0, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), clr_ht));
        p.hit(clr_x, clr_y, clr_w, clr_h, Action::ClearShelf);

        let sub = if total > 3 {
            format!("Drop files or Ctrl+C • {}-{} of {}", start_idx + 1, (start_idx + 3).min(total), total)
        } else {
            "Drop files or Ctrl+C • Click to copy/open".to_string()
        };
        let sub_max_w = (clr_x - (margin_x + 44.0) - 10.0).max(60.0);
        let sub_ell = p.ellipsize(&sub, 10.0, Weight::Regular, sub_max_w);
        p.text(&sub_ell, margin_x + 44.0, card_y + 42.5, 10.0, Weight::Regular, tok::text2());

        if state.shelf_items.is_empty() {
            p.text_center(
                "Shelf empty • Drop files onto notch or press Ctrl+C anywhere",
                margin_x + card_w / 2.0,
                card_y + 98.0,
                11.5,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            for (slot, (real_idx, item)) in state.shelf_items.iter().enumerate().skip(start_idx).take(3).enumerate() {
                let item_y = card_y + 56.0 + (slot as f32 * 30.0);
                let item_h = 26.0;
                let item_x = margin_x + 16.0;
                let del_w = 26.0;
                let row_w = card_w - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 7.5, row_ht, None);
                let dy = -0.55 * row_ht;
                if row_ht > 0.02 {
                    p.rrect(item_x, item_y + dy, row_w, item_h, 7.5, fade(tok::cyno_violet(), 0.25 * row_ht));
                }

                let (ic, ic_col) = match item.kind {
                    ShelfKind::File => (Icon::File, tok::cyno_mist()),
                    ShelfKind::Text => (Icon::Clipboard, tok::cyno_violet_hi()),
                };
                icons::draw(p, ic, item_x + 10.0, item_y + 6.5 + dy, 13.0, ic_col);

                let clip_snippet = p.ellipsize(&item.label, 11.0, Weight::Regular, row_w - 44.0);
                p.text(&clip_snippet, item_x + 30.0, item_y + 17.0 + dy, 11.0, Weight::Regular, tok::text1());
                p.hit(item_x, item_y, row_w, item_h, Action::ActivateShelfItem(real_idx));

                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 7.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(del_x, item_y - 0.55 * del_ht, del_w, item_h, 7.5, fade(rgba(220, 38, 38, 195), del_ht));
                }
                icons::draw(p, Icon::Close, del_x + 7.0, item_y + 7.0 - 0.55 * del_ht, 12.0, lerp_color(tok::text2(), tok::text1(), del_ht));
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteShelfItem(real_idx));
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 8. QUICK CAPTURE VIEW (Lazyshot-style Instant Desktop/Window PNG + Clipboard)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_capture_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let card_h = 154.0;
        let gap = 14.0;
        let left_w = 242.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;

        // Left Card: Instant Capture Triggers
        p.elevated_card(margin_x, card_y, left_w, card_h, 16.0, Some((left_w * 0.5, 36.0, 75.0, tok::cyno_violet())));
        icons::draw(p, Icon::Video, margin_x + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Screen Snapshot", margin_x + 42.0, card_y + 27.5, 13.0, Weight::Bold, tok::text1());
        let cap_sub = p.ellipsize("Saves PNG & copies (Ctrl+Shift+S)", 9.8, Weight::Regular, left_w - 32.0);
        p.text(&cap_sub, margin_x + 16.0, card_y + 45.0, 9.8, Weight::Regular, tok::text2());

        let btn_w = left_w - 32.0;
        let b1_x = margin_x + 16.0;
        let b1_y = card_y + 62.0;
        let b1_h = 36.0;
        let b1_ht = p.hot_t(b1_x, b1_y, btn_w, b1_h);
        p.luminous_pill_button_t(
            b1_x,
            b1_y,
            btn_w,
            b1_h,
            11.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            b1_ht,
        );
        p.text_center_shadow("Capture Full Screen", b1_x + btn_w / 2.0, b1_y + 22.0 - 0.8 * b1_ht, 12.0, Weight::Bold, tok::text1());
        p.hit(b1_x, b1_y, btn_w, b1_h, Action::CaptureScreen);

        let b2_y = card_y + 108.0;
        let b2_h = 30.0;
        let b2_ht = p.hot_t(b1_x, b2_y, btn_w, b2_h);
        p.tactile_surface_pill_t(b1_x, b2_y, btn_w, b2_h, 9.5, b2_ht, None);
        p.text_center("Capture Active Window", b1_x + btn_w / 2.0, b2_y + 19.0 - 0.55 * b2_ht, 11.0, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), b2_ht));
        p.hit(b1_x, b2_y, btn_w, b2_h, Action::CaptureWindow);

        // Right Card: Latest Capture Status & Folder Launcher
        p.elevated_card(right_x, card_y, right_w, card_h, 16.0, Some((right_w - 30.0, 24.0, 75.0, tok::cyno_orchid())));
        p.dot_matrix(right_x + right_w - 44.0, card_y + 14.0, 5, 3, 5.0, rgba(255, 255, 255, 30));
        icons::draw(p, Icon::Folder, right_x + 16.0, card_y + 16.0, 18.0, tok::cyno_mist());
        p.text("Capture Output", right_x + 42.0, card_y + 27.5, 13.0, Weight::Bold, tok::text1());

        if let Some(ref _path) = state.capture.last_path {
            let fname = p.ellipsize(&state.capture.last_filename, 11.5, Weight::Semibold, right_w - 32.0);
            p.text(&fname, right_x + 16.0, card_y + 64.0, 11.5, Weight::Semibold, tok::text1());
            let dims = format!(
                "{} • {}",
                state.capture.last_dims,
                if state.capture.clipboard_ok { "Copied to Clipboard" } else { "Saved to Disk" }
            );
            p.text(&dims, right_x + 16.0, card_y + 82.0, 10.5, Weight::Regular, tok::cyno_violet_hi());
        } else {
            p.text("No capture taken in this session", right_x + 16.0, card_y + 64.0, 11.5, Weight::Semibold, tok::text1());
            p.text("Output: Pictures\\CyNotch\\*.png", right_x + 16.0, card_y + 82.0, 10.5, Weight::Regular, tok::text2());
        }

        let open_w = right_w - 32.0;
        let open_x = right_x + 16.0;
        let open_y = card_y + 108.0;
        let open_h = 30.0;
        let open_ht = p.hot_t(open_x, open_y, open_w, open_h);
        p.tactile_surface_pill_t(open_x, open_y, open_w, open_h, 9.5, open_ht, Some(tok::cyno_violet()));
        p.text_center("Open Pictures\\CyNotch", open_x + open_w / 2.0, open_y + 19.0 - 0.55 * open_ht, 11.0, Weight::Bold, tok::text1());
        p.hit(open_x, open_y, open_w, open_h, Action::OpenCaptureFolder);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 9. INLINE SETTINGS VIEW (Quick Controls + Launcher to Advanced Window)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_settings_view(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let margin_x = 20.0;
        let card_h = 154.0;
        let gap = 14.0;
        let card_w = (w - (margin_x * 2.0) - gap) / 2.0;

        let left_x = margin_x;
        let right_x = margin_x + card_w + gap;

        p.elevated_card(left_x, card_y, card_w, card_h, 16.0, Some((25.0, 20.0, 70.0, tok::cyno_violet())));

        icons::draw(p, Icon::Settings, left_x + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("System Integration", left_x + 42.0, card_y + 27.5, 13.0, Weight::Bold, tok::text1());
        p.text("Startup, chime & global hotkeys", left_x + 42.0, card_y + 42.0, 10.0, Weight::Regular, tok::text2());

        let r1_y = card_y + 58.0;
        p.text("Launch on Startup", left_x + 18.0, r1_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let sw_w = 46.0;
        let sw_h = 22.0;
        let sw1_x = left_x + card_w - sw_w - 18.0;
        let sw1_ht = p.hot_t(sw1_x, r1_y, sw_w, sw_h);
        let st1 = if smooth {
            state.switch_t[ToggleId::Autostart as usize]
        } else if state.autostart {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, r1_y, sw_w, sw_h, st1, sw1_ht);
        p.hit(sw1_x, r1_y, sw_w, sw_h, Action::ToggleAutostart);

        let r2_y = card_y + 88.0;
        p.text("Chime Audio", left_x + 18.0, r2_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let sw2_x = left_x + card_w - sw_w - 18.0;
        let sw2_ht = p.hot_t(sw2_x, r2_y, sw_w, sw_h);
        let st2 = if smooth {
            state.switch_t[ToggleId::Chime as usize]
        } else if state.chime_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw2_x, r2_y, sw_w, sw_h, st2, sw2_ht);
        p.hit(sw2_x, r2_y, sw_w, sw_h, Action::ToggleChime);

        let r3_y = card_y + 118.0;
        p.text("Hotkeys (C/M/S)", left_x + 18.0, r3_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let badge_w = 108.0;
        let badge_x = left_x + card_w - badge_w - 18.0;
        let well = p.well_col();
        p.rrect(badge_x, r3_y, badge_w, 22.0, 7.0, well);
        p.rrect_bevel_stroke(badge_x, r3_y, badge_w, 22.0, 7.0, 1.0, rgba(192, 132, 252, 90), rgba(255, 255, 255, 20));
        p.text_center("Ctrl+Shift+C/M/S", badge_x + badge_w / 2.0, r3_y + 14.8, 9.2, Weight::Semibold, tok::cyno_violet_hi());

        // Right Card
        p.elevated_card(right_x, card_y, card_w, card_h, 16.0, Some((card_w - 25.0, 20.0, 70.0, tok::cyno_orchid())));

        icons::draw(p, Icon::Droplet, right_x + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Health & Engine", right_x + 42.0, card_y + 27.5, 13.0, Weight::Bold, tok::text1());
        p.text("Hydration & lifecycle control", right_x + 42.0, card_y + 42.0, 10.0, Weight::Regular, tok::text2());

        p.text("Water Interval:", right_x + 18.0, card_y + 64.0, 11.0, Weight::Semibold, tok::text1());
        let intervals: [(u32, &str); 4] = [(30, "30m"), (45, "45m"), (60, "60m"), (0, "Off")];
        let pb_gap = 6.0;
        let pb_w = (card_w - 36.0 - (pb_gap * 3.0)) / 4.0;
        let pb_y = card_y + 74.0;
        let pb_h = 26.0;

        for (i, &(mins, label)) in intervals.iter().enumerate() {
            let px = right_x + 18.0 + (i as f32 * (pb_w + pb_gap));
            let is_cur = state.water_interval_mins == mins;
            let ht = p.hot_t(px, pb_y, pb_w, pb_h);
            p.tactile_surface_pill_t(
                px,
                pb_y,
                pb_w,
                pb_h,
                7.0,
                ht,
                if is_cur { Some(tok::cyno_violet()) } else { None },
            );
            p.text_center(
                label,
                px + pb_w / 2.0,
                pb_y + 17.0 - 0.55 * ht,
                10.2,
                if is_cur { Weight::Bold } else { Weight::Semibold },
                if is_cur { tok::text1() } else { lerp_color(tok::text_soft(), tok::text1(), ht) },
            );
            p.hit(px, pb_y, pb_w, pb_h, Action::SetWaterInterval(mins));
        }

        let bot_y = card_y + 112.0;
        let bot_h = 28.0;
        let quit_w = 84.0;
        let spec_w = card_w - 36.0 - quit_w - 8.0;
        let left_ht = p.hot_t(right_x + 18.0, bot_y, spec_w, bot_h);
        p.tactile_surface_pill_t(right_x + 18.0, bot_y, spec_w, bot_h, 8.0, left_ht, None);
        let pos_lbl = if state.x_offset != 0 || state.y_offset != 0 {
            "Center Notch"
        } else {
            "Advanced Settings"
        };
        p.text_center(
            pos_lbl,
            right_x + 18.0 + spec_w / 2.0,
            bot_y + 18.0 - 0.55 * left_ht,
            9.8,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), left_ht),
        );
        p.hit(
            right_x + 18.0,
            bot_y,
            spec_w,
            bot_h,
            if state.x_offset != 0 || state.y_offset != 0 {
                Action::ResetPosition
            } else {
                Action::OpenSettings
            },
        );

        let quit_x = right_x + 18.0 + spec_w + 8.0;
        let quit_ht = p.hot_t(quit_x, bot_y, quit_w, bot_h);
        p.tactile_surface_pill_t(quit_x, bot_y, quit_w, bot_h, 8.0, quit_ht, Some(rgba(220, 38, 38, 255)));
        p.text_center_shadow("Exit App", quit_x + quit_w / 2.0, bot_y + 18.0 - 0.55 * quit_ht, 10.2, Weight::Bold, tok::text1());
        p.hit(quit_x, bot_y, quit_w, bot_h, Action::Quit);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // VERTICAL EXPANDED SHELL & 10 STACKED CARD VIEWS (316×480 Left/Right edge)
    // ─────────────────────────────────────────────────────────────────────────────
    fn draw_expanded_vertical(p: &mut Painter, state: &AppState, w: f32, _h: f32, _settled: bool) {
        // Row 1 Header: [Mode Title] ... [Lyrics?] [Pin] [Settings Gear] [Grip]
        let btn_y = 12.0;
        let right_x = w - 14.0;
        let grip_x = right_x - 18.0;
        let grip_ht = p.hot_t(grip_x - 4.0, btn_y, 26.0, 28.0);
        let grip_col = lerp_color(rgba(148, 163, 184, 125), tok::text1(), grip_ht);
        icons::draw(p, Icon::Grip, grip_x, btn_y + 6.0, 16.0, grip_col);
        p.hit(grip_x - 4.0, btn_y, 26.0, 28.0, Action::StartGripDrag);

        let gear_x = grip_x - 38.0;
        let is_settings_active = state.settings_open || state.mode == NotchMode::Settings;
        let gear_ht = p.hot_t(gear_x, btn_y, 28.0, 28.0);
        if is_settings_active {
            p.luminous_pill_button_t(
                gear_x,
                btn_y,
                28.0,
                28.0,
                14.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                gear_ht,
            );
            icons::draw(p, Icon::Settings, gear_x + 7.0, btn_y + 7.0, 14.0, rgba(255, 255, 255, 255));
        } else {
            p.tactile_surface_pill_t(gear_x, btn_y, 28.0, 28.0, 14.0, gear_ht, None);
            let gc = lerp_color(tok::text2(), tok::text1(), gear_ht);
            icons::draw(p, Icon::Settings, gear_x + 7.0, btn_y + 7.0, 14.0, gc);
        }
        p.hit(gear_x, btn_y, 28.0, 28.0, Action::OpenSettings);

        let pin_x = gear_x - 34.0;
        let pin_ht = p.hot_t(pin_x, btn_y, 28.0, 28.0);
        if state.is_pinned {
            p.luminous_pill_button_t(
                pin_x,
                btn_y,
                28.0,
                28.0,
                14.0,
                tok::cyno_indigo(),
                tok::cyno_violet(),
                tok::cyno_orchid(),
                pin_ht,
            );
            icons::draw(p, Icon::Lock, pin_x + 7.0, btn_y + 7.0, 14.0, rgba(255, 255, 255, 255));
        } else {
            p.tactile_surface_pill_t(pin_x, btn_y, 28.0, 28.0, 14.0, pin_ht, None);
            let pc = lerp_color(tok::text2(), tok::text1(), pin_ht);
            icons::draw(p, Icon::Lock, pin_x + 7.0, btn_y + 7.0, 14.0, pc);
        }
        p.hit(pin_x, btn_y, 28.0, 28.0, Action::TogglePin);

        let title_x = 18.0;
        let title_y = 30.5;
        let title_limit = if state.mode == NotchMode::Music {
            let lyrics_w = 54.0;
            let lyrics_x = pin_x - lyrics_w - 8.0;
            let lyrics_y = 13.0;
            let lyrics_ht = p.hot_t(lyrics_x, lyrics_y, lyrics_w, 26.0);
            if state.show_lyrics {
                p.luminous_pill_button_t(
                    lyrics_x,
                    lyrics_y,
                    lyrics_w,
                    26.0,
                    13.0,
                    tok::cyno_indigo(),
                    tok::cyno_violet(),
                    tok::cyno_orchid(),
                    lyrics_ht,
                );
                p.text_center_shadow("Lyrics", lyrics_x + lyrics_w / 2.0, lyrics_y + 17.0, 10.5, Weight::Bold, tok::text1());
            } else {
                p.tactile_surface_pill_t(lyrics_x, lyrics_y, lyrics_w, 26.0, 13.0, lyrics_ht, None);
                p.text_center("Lyrics", lyrics_x + lyrics_w / 2.0, lyrics_y + 17.0, 10.5, Weight::Semibold, tok::text_soft());
            }
            p.hit(lyrics_x, lyrics_y, lyrics_w, 26.0, Action::ToggleLyrics);
            lyrics_x - 8.0
        } else {
            pin_x - 8.0
        };

        let mode_title = match state.mode {
            NotchMode::Home => "Command Hub",
            NotchMode::Music => "Music",
            NotchMode::Volume => "Volume Master",
            NotchMode::MemReduct => "Mem Reduct",
            NotchMode::FocusTimer => "Focus Timer",
            NotchMode::Tasks => "Tasks",
            NotchMode::Shelf => "Quick Shelf",
            NotchMode::Capture => "Quick Capture",
            NotchMode::Telemetry => "Screen Time",
            NotchMode::Settings => "Quick Settings",
        };
        let max_tw = (title_limit - title_x).max(24.0);
        let title_ell = p.ellipsize(mode_title, 13.5, Weight::Bold, max_tw);
        p.text(&title_ell, title_x, title_y, 13.5, Weight::Bold, tok::text1());

        // Row 2: Centered Dynamic Module Dock
        let dock_modes = state.visible_dock_modes();
        let (dock_w, item_w) = AppState::dock_metrics_for_placement(dock_modes.len(), state.placement);
        let dock_x = (w - dock_w) * 0.5;
        let dock_y = 46.0;
        let dock_h = 32.0;

        let well_col = if state.is_frosted_active() {
            tok::scale_frosted(tok::surface_well_frosted(), state.frosted_opacity_pct)
        } else {
            tok::surface_well()
        };
        p.rrect(dock_x, dock_y, dock_w, dock_h, 11.0, well_col);
        p.rrect_bevel_stroke(dock_x, dock_y, dock_w, dock_h, 11.0, 1.0, rgba(0, 0, 0, 120), rgba(255, 255, 255, 26));

        let active_visible = dock_modes.contains(&state.mode);
        let (target_dock_x, target_dock_w) = state.active_dock_target();
        let (pill_x, pill_w) = if !state.micro_interactions || state.motion_preset == MotionPreset::Reduced {
            (target_dock_x, target_dock_w)
        } else {
            (
                state
                    .anim_dock_x
                    .clamp(dock_x + 4.0, (dock_x + dock_w - 4.0 - item_w).max(dock_x + 4.0)),
                state.anim_dock_w.max(18.0),
            )
        };

        if active_visible {
            let active_ht = if state.micro_interactions && state.motion_preset != MotionPreset::Reduced {
                dock_modes
                    .iter()
                    .position(|&m| m == state.mode)
                    .and_then(|idx| state.hover_slots.get(idx).copied())
                    .unwrap_or(0.0)
                    * 0.85
            } else if p.hot(pill_x, dock_y + 4.0, pill_w, 24.0) {
                0.85
            } else {
                0.0
            };
            p.luminous_pill_button_t(
                pill_x,
                dock_y + 4.0,
                pill_w,
                24.0,
                7.5,
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
                NotchMode::FocusTimer => Icon::Timer,
                NotchMode::Tasks => Icon::List,
                NotchMode::Shelf => Icon::Clipboard,
                NotchMode::Capture => Icon::Video,
                NotchMode::Telemetry => Icon::Display,
                NotchMode::Settings => Icon::Settings,
            };
            let ix = dock_x + 4.0 + (i as f32 * item_w);
            let iy = dock_y + 4.0;
            let ht = p.hot_t(ix, iy, item_w, 24.0);
            let under_pill = if active_visible {
                (1.0 - ((pill_x - ix).abs() / item_w.max(1.0))).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if under_pill < 0.5 && ht > 0.02 {
                p.rrect(ix, iy, item_w, 24.0, 7.5, fade(rgba(255, 255, 255, 22), ht));
            }
            let inactive_col = lerp_color(rgba(148, 163, 184, 165), rgba(255, 255, 255, 245), ht);
            let col = lerp_color(inactive_col, rgba(17, 17, 23, 255), under_pill);
            icons::draw(p, icon, ix + (item_w - 13.5) / 2.0, iy + 5.25, 13.5, col);
            p.hit(ix, iy, item_w, 24.0, Action::SetMode(*mode));
        }

        let card_y = 88.0;
        match state.mode {
            NotchMode::Home => Self::draw_home_view_vertical(p, state, w, card_y),
            NotchMode::Music => Self::draw_music_view_vertical(p, state, w, card_y),
            NotchMode::Volume => Self::draw_volume_view_vertical(p, state, w, card_y),
            NotchMode::MemReduct => Self::draw_memreduct_view_vertical(p, state, w, card_y),
            NotchMode::FocusTimer => Self::draw_focustimer_view_vertical(p, state, w, card_y),
            NotchMode::Tasks => Self::draw_tasks_view_vertical(p, state, w, card_y),
            NotchMode::Shelf => Self::draw_shelf_view_vertical(p, state, w, card_y),
            NotchMode::Capture => Self::draw_capture_view_vertical(p, state, w, card_y),
            NotchMode::Telemetry => Self::draw_telemetry_view_vertical(p, state, w, card_y),
            NotchMode::Settings => Self::draw_settings_view_vertical(p, state, w, card_y),
        }
        Self::draw_expanded_toast_overlay(p, state, w);
    }

    fn draw_home_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: General Cyno Companion + RAM Bar + Purge CTA
        let c1_h = 134.0;
        p.elevated_card(mx, card_y, cw, c1_h, 16.0, Some((42.0, 34.0, 74.0, tok::cyno_violet())));
        let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
        Self::draw_cyno_mascot(p, mx + 30.0, card_y + 31.0, 0.96, state.mascot_mood(), anim_t, false);

        p.text("General Cyno", mx + 58.0, card_y + 27.0, 13.0, Weight::Bold, tok::text1());
        let pwr_str = match (state.telemetry.battery_pct, state.telemetry.on_ac) {
            (Some(pct), true) => format!("CPU {:.0}% • {}% AC", state.telemetry.cpu_usage, pct),
            (Some(pct), false) => format!("CPU {:.0}% • {}% Bat", state.telemetry.cpu_usage, pct),
            (None, _) => format!("CPU {:.0}% • AC Power", state.telemetry.cpu_usage),
        };
        let pwr_ell = p.ellipsize(&pwr_str, 10.0, Weight::Regular, cw - 74.0);
        p.text(&pwr_ell, mx + 58.0, card_y + 42.5, 10.0, Weight::Regular, tok::text2());

        let ram_label = format!("RAM {:.0}%", state.telemetry.memory_percent);
        p.text(&ram_label, mx + 16.0, card_y + 67.0, 10.5, Weight::Semibold, tok::text_soft());
        let used_lbl = format!("{:.1} / {:.1} GB", state.telemetry.memory_used_gb, state.telemetry.memory_total_gb);
        p.text_right(&used_lbl, mx + cw - 16.0, card_y + 67.0, 10.5, Weight::Semibold, tok::cyno_mist());

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(mx + 16.0, card_y + 75.0, cw - 32.0, 7.0, ram_pct, tok::cyno_deep(), tok::cyno_violet_hi());

        let purge_x = mx + 16.0;
        let purge_y = card_y + 92.0;
        let purge_w = cw - 32.0;
        let purge_h = 30.0;
        let purge_ht = p.hot_t(purge_x, purge_y, purge_w, purge_h);
        p.luminous_pill_button_t(
            purge_x,
            purge_y,
            purge_w,
            purge_h,
            10.5,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            purge_ht,
        );
        let purge_lbl = if state.mem_reduct.is_purging { "Purging RAM..." } else { "Purge RAM" };
        p.text_center_shadow(purge_lbl, purge_x + purge_w / 2.0, purge_y + 19.0 - 0.8 * purge_ht, 11.5, Weight::Bold, tok::text1());
        p.hit(purge_x, purge_y, purge_w, purge_h, Action::PurgeRam);

        // Card 2: Daily Hydration OR Live Network & Quick Focus
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 92.0;
        if state.health_reminders_enabled {
            p.elevated_card(mx, c2_y, cw, c2_h, 15.0, Some((cw - 34.0, 22.0, 72.0, rgba(56, 189, 248, 160))));
            icons::draw(p, Icon::Droplet, mx + 16.0, c2_y + 13.0, 15.0, tok::cyan_hi());
            p.text("Daily Hydration", mx + 36.0, c2_y + 24.5, 12.0, Weight::Bold, tok::text1());
            let water_stat = format!("{} / {} ml", state.water.taken_ml, state.water.goal_ml);
            p.text_right(&water_stat, mx + cw - 16.0, c2_y + 24.5, 10.5, Weight::Semibold, tok::cyan_hi());

            let water_pct = if smooth {
                state.anim_water_pct.clamp(0.0, 1.0)
            } else {
                (state.water.taken_ml as f32 / state.water.goal_ml.max(1) as f32).clamp(0.0, 1.0)
            };
            p.glowing_striped_bar(mx + 16.0, c2_y + 37.5, cw - 32.0, 7.5, water_pct, rgba(3, 105, 161, 255), rgba(56, 189, 248, 255));

            let drink_x = mx + 16.0;
            let drink_y = c2_y + 53.0;
            let drink_w = cw - 32.0;
            let drink_h = 28.0;
            let drink_ht = p.hot_t(drink_x, drink_y, drink_w, drink_h);
            p.tactile_surface_pill_t(drink_x, drink_y, drink_w, drink_h, 9.5, drink_ht, Some(tok::cyno_violet()));
            let glass_lbl = format!("Drink +{} ml", state.water.glass_ml);
            p.text_center(&glass_lbl, drink_x + drink_w / 2.0, drink_y + 18.0 - 0.55 * drink_ht, 11.0, Weight::Bold, tok::text1());
            p.hit(drink_x, drink_y, drink_w, drink_h, Action::DrinkWater(state.water.glass_ml));
        } else {
            p.elevated_card(mx, c2_y, cw, c2_h, 15.0, Some((cw - 34.0, 22.0, 72.0, tok::cyan_hi())));
            let foc_w = 112.0;
            let foc_h = 38.0;
            let foc_x = mx + cw - foc_w - 14.0;
            let foc_y = c2_y + 29.0;

            let net_card_ht = p.hot_t(mx, c2_y, foc_x - mx - 6.0, c2_h);
            if net_card_ht > 0.02 {
                p.rrect_bevel_stroke(
                    mx,
                    c2_y,
                    foc_x - mx - 6.0,
                    c2_h,
                    15.0,
                    1.0,
                    fade(tok::cyan_hi(), 0.35 * net_card_ht),
                    fade(tok::cyno_violet(), 0.14 * net_card_ht),
                );
            }
            icons::draw(p, Icon::Activity, mx + 14.0, c2_y + 12.0, 14.0, tok::cyan_hi());
            p.text("Live Net & Focus", mx + 33.0, c2_y + 23.0, 11.5, Weight::Bold, tok::text1());

            let dn = p.ellipsize(&format!("↓ {}", format_speed_long(state.telemetry.net_down_kbps)), 10.8, Weight::Semibold, foc_x - mx - 24.0);
            let up = p.ellipsize(&format!("↑ {}", format_speed_long(state.telemetry.net_up_kbps)), 10.5, Weight::Semibold, foc_x - mx - 24.0);
            p.text(&dn, mx + 16.0, c2_y + 49.0, 10.8, Weight::Semibold, tok::cyan_hi());
            p.text(&up, mx + 16.0, c2_y + 70.0, 10.5, Weight::Semibold, tok::cyno_mist());
            p.hit(mx, c2_y, foc_x - mx - 6.0, c2_h, Action::SetMode(NotchMode::Telemetry));

            let foc_ht = p.hot_t(foc_x, foc_y, foc_w, foc_h);
            let m = state.timer.remaining_secs / 60;
            let s = state.timer.remaining_secs % 60;
            if state.timer.is_running {
                p.luminous_pill_button_t(
                    foc_x,
                    foc_y,
                    foc_w,
                    foc_h,
                    11.0,
                    tok::cyno_indigo(),
                    tok::cyno_violet(),
                    tok::cyno_orchid(),
                    foc_ht,
                );
                let lbl = format!("Pause {:02}:{:02}", m, s);
                p.text_center_shadow(&lbl, foc_x + foc_w / 2.0, foc_y + 23.0 - 0.7 * foc_ht, 10.5, Weight::Bold, tok::text1());
            } else {
                p.tactile_surface_pill_t(foc_x, foc_y, foc_w, foc_h, 11.0, foc_ht, Some(tok::cyno_violet()));
                let lbl = format!("Focus {:02}:{:02}", m, s);
                p.text_center(&lbl, foc_x + foc_w / 2.0, foc_y + 23.0 - 0.55 * foc_ht, 10.5, Weight::Bold, tok::text1());
            }
            p.hit(foc_x, foc_y, foc_w, foc_h, Action::ToggleFocusTimer);
        }

        // Card 3 Row: Side-by-Side Media & Tasks/Capture (y = 338..464, h = 126)
        let c3_y = c2_y + c2_h + 12.0;
        let c3_h = 126.0;
        let sub_gap = 12.0;
        let sub_w = (cw - sub_gap) / 2.0;

        // Left: Media Overview
        p.elevated_card(mx, c3_y, sub_w, c3_h, 15.0, None);
        let media_ht = p.hot_t(mx, c3_y, sub_w, c3_h - 46.0);
        if media_ht > 0.02 {
            p.rrect_bevel_stroke(mx, c3_y, sub_w, c3_h, 15.0, 1.0, fade(tok::cyno_violet_hi(), 0.42 * media_ht), fade(tok::cyno_violet(), 0.16 * media_ht));
        }
        let media_header = if state.media.is_idle() {
            "No Active Media"
        } else if state.media.is_playing {
            "Now Playing"
        } else {
            "Media Paused"
        };
        p.text(media_header, mx + 14.0, c3_y + 22.0, 10.0, Weight::Semibold, tok::text2());
        let song = p.ellipsize(&state.media.title, 12.0, Weight::Bold, sub_w - 28.0);
        p.text(&song, mx + 14.0, c3_y + 44.0, 12.0, Weight::Bold, tok::text1());
        let art_ell = p.ellipsize(&state.media.artist, 10.0, Weight::Regular, sub_w - 28.0);
        p.text(&art_ell, mx + 14.0, c3_y + 62.0, 10.0, Weight::Regular, tok::text2());
        p.hit(mx, c3_y, sub_w, c3_h - 46.0, Action::SetMode(NotchMode::Music));

        let mp_x = mx + 14.0;
        let mp_y = c3_y + 84.0;
        let mp_w = sub_w - 28.0;
        let mp_h = 28.0;
        let mp_ht = p.hot_t(mp_x, mp_y, mp_w, mp_h);
        p.tactile_surface_pill_t(mp_x, mp_y, mp_w, mp_h, 9.5, mp_ht, Some(tok::cyno_violet()));
        let mp_lbl = if state.media.is_playing { "Pause" } else { "Play" };
        p.text_center(mp_lbl, mp_x + mp_w / 2.0, mp_y + 18.0 - 0.55 * mp_ht, 10.8, Weight::Bold, tok::text1());
        p.hit(mp_x, mp_y, mp_w, mp_h, Action::TogglePlayPause);

        // Right: Tasks & Quick Screenshot
        let r_x = mx + sub_w + sub_gap;
        p.elevated_card(r_x, c3_y, sub_w, c3_h, 15.0, None);
        let tasks_ht = p.hot_t(r_x, c3_y, sub_w, c3_h - 46.0);
        if tasks_ht > 0.02 {
            p.rrect_bevel_stroke(r_x, c3_y, sub_w, c3_h, 15.0, 1.0, fade(tok::cyno_mist(), 0.42 * tasks_ht), fade(tok::cyno_orchid(), 0.16 * tasks_ht));
        }
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        p.text("Tasks Open", r_x + 14.0, c3_y + 22.0, 10.0, Weight::Semibold, tok::text2());
        p.text(&format!("{} active", pending), r_x + 14.0, c3_y + 46.0, 14.0, Weight::Bold, tok::cyno_mist());
        let first_task = state.tasks.iter().find(|t| !t.completed).map(|t| t.text.as_str()).unwrap_or("All tasks done");
        let ft_ell = p.ellipsize(first_task, 9.8, Weight::Regular, sub_w - 28.0);
        p.text(&ft_ell, r_x + 14.0, c3_y + 64.0, 9.8, Weight::Regular, tok::text2());
        p.hit(r_x, c3_y, sub_w, c3_h - 46.0, Action::SetMode(NotchMode::Tasks));

        let shot_x = r_x + 14.0;
        let shot_y = c3_y + 84.0;
        let shot_w = sub_w - 28.0;
        let shot_h = 28.0;
        let shot_ht = p.hot_t(shot_x, shot_y, shot_w, shot_h);
        p.tactile_surface_pill_t(shot_x, shot_y, shot_w, shot_h, 9.5, shot_ht, None);
        p.text_center("Screenshot", shot_x + shot_w / 2.0, shot_y + 18.0 - 0.55 * shot_ht, 10.5, Weight::Semibold, tok::cyno_violet_hi());
        p.hit(shot_x, shot_y, shot_w, shot_h, Action::CaptureScreen);
    }

    fn draw_music_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Top Card: Luminous Album / Cyno Hero Card (h = 146)
        let c1_h = 146.0;
        p.luminous_pill_button(
            mx,
            card_y,
            cw,
            c1_h,
            20.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            false,
        );
        p.dot_matrix(mx + 14.0, card_y + 14.0, 4, 3, 5.5, rgba(255, 255, 255, 55));

        let hero_cx = mx + 72.0;
        let hero_cy = card_y + c1_h * 0.5;
        if state.show_mascot && state.media.is_playing {
            p.circle(hero_cx, hero_cy, 44.0, rgba(10, 9, 20, 150));
            p.circle_stroke(hero_cx, hero_cy, 44.0, 1.0, rgba(255, 255, 255, 45));
            let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
            Self::draw_cyno_mascot(p, hero_cx, hero_cy + 1.0, 1.72, MascotMood::Music, anim_t, false);
        } else {
            p.circle(hero_cx, hero_cy, 42.0, rgba(10, 9, 20, 140));
            icons::draw(p, Icon::Music, hero_cx - 20.0, hero_cy - 20.0, 40.0, rgba(255, 255, 255, 255));
        }

        let badge_x = mx + 138.0;
        let badge_w = cw - 154.0;
        let art_badge = if !state.media.album.is_empty() {
            state.media.album.as_str()
        } else if state.media.is_idle() {
            "IDLE"
        } else if state.media.is_playing {
            "LIVE AUDIO"
        } else {
            "PAUSED"
        };
        let badge_txt = p.ellipsize(art_badge, 9.8, Weight::Bold, badge_w - 16.0);
        p.rrect(badge_x, card_y + 24.0, badge_w, 24.0, 12.0, rgba(10, 8, 22, 150));
        p.text_center(&badge_txt, badge_x + badge_w / 2.0, card_y + 39.5, 9.8, Weight::Bold, tok::text1());

        if state.equalizer_enabled {
            let bar_heights = [8.0, 14.0, 20.0, 15.0, 24.0, 26.0, 18.0, 12.0, 22.0, 26.0, 16.0, 10.0];
            let eq_x = badge_x + 8.0;
            let eq_y = card_y + 68.0;
            for (i, &base_bh) in bar_heights.iter().enumerate() {
                let bh = if state.media.is_playing {
                    let wave = (state.anim_time * 8.5 + i as f32 * 0.75).sin() * 0.5 + 0.5;
                    (base_bh * 0.35 + wave * (base_bh * 0.85)).clamp(4.0, 26.0)
                } else {
                    (base_bh * 0.32).max(4.0)
                };
                let bx = eq_x + (i as f32 * 9.5);
                let by = eq_y + (26.0 - bh) / 2.0;
                let bar_col = if i % 4 == 0 { tok::cyno_mist() } else { rgba(255, 255, 255, 230) };
                p.rrect(bx, by, 4.0, bh, 2.0, bar_col);
            }
        }
        let state_lbl = if state.media.is_playing {
            "Playing now"
        } else if state.media.is_idle() {
            "Waiting for audio"
        } else {
            "Playback paused"
        };
        p.text_center(state_lbl, badge_x + badge_w / 2.0, card_y + 118.0, 10.2, Weight::Semibold, rgba(248, 250, 252, 235));

        // Bottom Card: Controls OR Synced Lyrics (y = 246..464, h = 218)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 218.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 16.0, Some((cw - 26.0, 24.0, 82.0, tok::cyno_violet())));

        let pad_x = AppState::VERT_MUSIC_SCRUBBER_X;
        let inner_w = AppState::VERT_MUSIC_SCRUBBER_W;

        let played_pct = if state.media.duration_ms > 0 {
            (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if state.show_lyrics {
            Self::draw_lyrics_panel(p, state, pad_x, c2_y + 10.0, inner_w, 128.0, true);

            let scrub_y = c2_y + 144.0;
            p.glowing_striped_bar(pad_x, scrub_y, inner_w, 6.5, played_pct, tok::cyno_deep(), tok::cyno_violet());
            for s in 0..10 {
                let sw = inner_w / 10.0;
                p.hit(pad_x + (s as f32 * sw), scrub_y - 6.0, sw, 20.0, Action::SeekMusic((s as f32 + 0.5) / 10.0));
            }
            Self::draw_transport_buttons(p, state.media.is_playing, pad_x + inner_w / 2.0, c2_y + 172.0);
        } else {
            let title_text = p.ellipsize(&state.media.title, 15.0, Weight::Bold, inner_w);
            p.text(&title_text, pad_x, c2_y + 30.0, 15.0, Weight::Bold, tok::text1());
            let artist_txt = p.ellipsize(&state.media.artist, 11.8, Weight::Regular, inner_w);
            p.text(&artist_txt, pad_x, c2_y + 49.0, 11.8, Weight::Regular, tok::text2());

            let prog_y = c2_y + 72.0;
            p.glowing_striped_bar(pad_x, prog_y, inner_w, 7.0, played_pct, tok::cyno_deep(), tok::cyno_violet());
            for s in 0..10 {
                let sw = inner_w / 10.0;
                p.hit(pad_x + (s as f32 * sw), prog_y - 6.0, sw, 22.0, Action::SeekMusic((s as f32 + 0.5) / 10.0));
            }

            let pos_sec = state.media.position_ms / 1000;
            let dur_sec = state.media.duration_ms / 1000;
            let rem_sec = dur_sec.saturating_sub(pos_sec);
            p.text(&format!("{}:{:02}", pos_sec / 60, pos_sec % 60), pad_x, prog_y + 22.0, 10.2, Weight::Semibold, tok::text2());
            let right_t = if state.media.duration_ms > 0 {
                format!("-{}:{:02}", rem_sec / 60, rem_sec % 60)
            } else {
                "--:--".to_string()
            };
            p.text_right(&right_t, pad_x + inner_w, prog_y + 22.0, 10.2, Weight::Semibold, tok::text2());

            let (snip_lines, snip_sz) =
                p.wrap_lines_fit(&state.current_lyric_text, 12.2, 9.8, Weight::Regular, inner_w, 2);
            for (li, line) in snip_lines.iter().take(2).enumerate() {
                p.text(
                    line,
                    pad_x,
                    prog_y + 42.0 + (li as f32) * (snip_sz + 3.0),
                    snip_sz,
                    Weight::Regular,
                    tok::text_soft(),
                );
            }

            Self::draw_transport_buttons(p, state.media.is_playing, pad_x + inner_w / 2.0, c2_y + 162.0);
        }
    }

    fn draw_volume_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let mx = 16.0;
        let cw = w - 32.0;
        let vol_int = (state.volume_pct * 100.0).round() as i32;
        let is_muted = vol_int == 0;
        let aurora_col = if is_muted { rgba(239, 68, 68, 120) } else { tok::cyno_violet() };

        // Top Card: Large Volume Readout + Mute Toggle (y = 88..234, h = 146)
        let c1_h = 146.0;
        p.elevated_card(mx, card_y, cw, c1_h, 16.0, Some((64.0, 44.0, 68.0, aurora_col)));

        let ic_cx = mx + 44.0;
        let ic_cy = card_y + 52.0;
        p.tactile_surface_pill(ic_cx - 19.0, ic_cy - 19.0, 38.0, 38.0, 19.0, false, Some(aurora_col));
        let vol_icon_col = if is_muted { rgba(252, 165, 165, 255) } else { tok::text1() };
        icons::draw(p, Icon::Volume, ic_cx - 9.0, ic_cy - 9.0, 18.0, vol_icon_col);

        p.text(&format!("{}%", vol_int), mx + 76.0, card_y + 58.0, 26.0, Weight::Bold, tok::text1());
        p.text("Master Output", mx + 76.0, card_y + 76.0, 10.5, Weight::Regular, tok::text2());

        let mute_w = cw - 32.0;
        let mute_h = 34.0;
        let mute_x = mx + 16.0;
        let mute_y = card_y + 96.0;
        let mute_ht = p.hot_t(mute_x, mute_y, mute_w, mute_h);
        if is_muted {
            p.luminous_pill_button_t(
                mute_x,
                mute_y,
                mute_w,
                mute_h,
                11.0,
                rgba(153, 27, 27, 255),
                rgba(220, 38, 38, 255),
                rgba(180, 83, 9, 255),
                mute_ht,
            );
        } else {
            p.tactile_surface_pill_t(mute_x, mute_y, mute_w, mute_h, 11.0, mute_ht, None);
        }
        let mute_label = if is_muted { "Unmute Audio" } else { "Mute Audio" };
        let mute_ty = mute_y + 21.5 - if is_muted { 0.8 * mute_ht } else { 0.55 * mute_ht };
        p.text_center_shadow(mute_label, mute_x + mute_w / 2.0, mute_ty, 11.5, Weight::Semibold, tok::text1());
        p.hit(mute_x, mute_y, mute_w, mute_h, Action::ToggleMute);

        // Bottom Card: Slider & Presets (y = 246..464, h = 218)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 218.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 16.0, Some((cw - 30.0, 26.0, 86.0, tok::cyno_orchid())));
        p.dot_matrix(mx + cw - 48.0, c2_y + 14.0, 5, 3, 5.5, rgba(255, 255, 255, 32));

        p.text("System Volume Master", mx + 16.0, c2_y + 28.0, 13.5, Weight::Bold, tok::text1());
        p.text("Windows Core Audio • Drag slider or scroll", mx + 16.0, c2_y + 46.0, 10.2, Weight::Regular, tok::text2());

        let slider_x = AppState::VERT_VOLUME_SLIDER_X;
        let slider_w = AppState::VERT_VOLUME_SLIDER_W;
        let slider_y = c2_y + 78.0; // y = 324 (< 365 drag cutoff)
        let slider_h = 14.0;
        let pct_clamped = if smooth {
            state.anim_vol_pct.clamp(0.0, 1.0)
        } else {
            state.volume_pct.clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(slider_x, slider_y, slider_w, slider_h, pct_clamped, tok::cyno_deep(), tok::cyno_violet());

        if pct_clamped > 0.01 {
            let thumb_x = (slider_x + slider_w * pct_clamped).clamp(slider_x + 8.0, slider_x + slider_w - 4.0);
            let thumb_y = slider_y + slider_h / 2.0;
            p.radial_glow(thumb_x, thumb_y, 14.0, rgba(255, 255, 255, 110), BlendMode::Plus, false);
            p.circle(thumb_x, thumb_y, 8.5, rgba(255, 255, 255, 255));
            p.circle(thumb_x, thumb_y, 4.0, tok::cyno_deep());
        }

        let seg_count = 10;
        let seg_w = slider_w / seg_count as f32;
        for s in 0..seg_count {
            let frac = (s as f32 + 0.5) / seg_count as f32;
            p.hit(slider_x + (s as f32 * seg_w), slider_y - 6.0, seg_w, 26.0, Action::SetVolume(frac));
        }

        p.text("Quick Presets", mx + 16.0, c2_y + 122.0, 10.5, Weight::Semibold, tok::text2());
        let presets: [(f32, &str); 5] = [(0.0, "0%"), (0.25, "25%"), (0.50, "50%"), (0.75, "75%"), (1.0, "100%")];
        let pb_gap = 6.0;
        let pb_w = (slider_w - (pb_gap * 4.0)) / 5.0;
        let pb_y = c2_y + 136.0; // y = 382 (>= 365 drag cutoff)
        let pb_h = 34.0;

        for (i, &(target_pct, label)) in presets.iter().enumerate() {
            let px = slider_x + (i as f32 * (pb_w + pb_gap));
            let is_cur = (state.volume_pct - target_pct).abs() < 0.12;
            let ht = p.hot_t(px, pb_y, pb_w, pb_h);
            if is_cur {
                p.luminous_pill_button_t(
                    px,
                    pb_y,
                    pb_w,
                    pb_h,
                    9.5,
                    tok::cyno_indigo(),
                    tok::cyno_violet(),
                    tok::cyno_orchid(),
                    ht,
                );
                p.text_center_shadow(label, px + pb_w / 2.0, pb_y + 21.0 - 0.8 * ht, 10.5, Weight::Bold, tok::text1());
            } else {
                p.tactile_surface_pill_t(px, pb_y, pb_w, pb_h, 9.5, ht, None);
                p.text_center(label, px + pb_w / 2.0, pb_y + 21.0 - 0.55 * ht, 10.5, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), ht));
            }
            p.hit(px, pb_y, pb_w, pb_h, Action::SetVolume(target_pct));
        }
    }

    fn draw_memreduct_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: Physical RAM + Pools (y = 88..182, h = 94)
        let c1_h = 94.0;
        p.elevated_card(mx, card_y, cw, c1_h, 15.0, Some((cw - 20.0, 16.0, 62.0, tok::cyno_violet())));
        p.text("Physical Memory", mx + 14.0, card_y + 20.0, 10.5, Weight::Semibold, tok::text2());
        let right_stat = if state.mem_reduct.is_purging {
            "Cleaning...".to_string()
        } else if state.mem_reduct.last_freed_mb > 0.0 {
            format!("Last: {:.0} MB", state.mem_reduct.last_freed_mb)
        } else if state.telemetry.has_admin_privileges {
            "Admin Mode".to_string()
        } else {
            "User Mode".to_string()
        };
        p.text_right(&right_stat, mx + cw - 14.0, card_y + 20.0, 9.8, Weight::Bold, tok::cyno_mist());

        let ram_val = format!("{:.1} / {:.1} GB ({:.0}%)", state.telemetry.memory_used_gb, state.telemetry.memory_total_gb, state.telemetry.memory_percent);
        p.text(&ram_val, mx + 14.0, card_y + 42.0, 14.5, Weight::Bold, tok::text1());

        let ram_pct = if smooth { state.anim_ram_pct.clamp(0.0, 1.0) } else { (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0) };
        p.glowing_striped_bar(mx + 14.0, card_y + 50.0, cw - 28.0, 6.5, ram_pct, tok::cyno_deep(), tok::cyno_violet_hi());

        let pools_str = format!("Page {:.0}% • SysCache {:.0} MB", state.telemetry.pagefile_percent, state.telemetry.sys_cache_mb);
        let pools_ell = p.ellipsize(&pools_str, 9.5, Weight::Regular, cw - 28.0);
        p.text(&pools_ell, mx + 14.0, card_y + 73.0, 9.5, Weight::Regular, tok::text2());

        // Card 2: 8 Clean Areas + Auto Clean Toggle (y = 192..304, h = 112)
        let c2_y = card_y + c1_h + 10.0;
        let c2_h = 112.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 15.0, None);
        p.text("Clean Areas", mx + 14.0, c2_y + 21.0, 10.8, Weight::Semibold, tok::text2());

        let auto_w = 76.0;
        let auto_x = mx + cw - auto_w - 14.0;
        let auto_y = c2_y + 9.0;
        let auto_ht = p.hot_t(auto_x, auto_y, auto_w, 20.0);
        p.tactile_surface_pill_t(
            auto_x,
            auto_y,
            auto_w,
            20.0,
            10.0,
            auto_ht,
            if state.mem_reduct.auto_clean_enabled { Some(tok::cyno_violet()) } else { None },
        );
        let auto_lbl = if state.mem_reduct.auto_clean_enabled { "Auto: ON" } else { "Auto: OFF" };
        p.text_center(
            auto_lbl,
            auto_x + auto_w / 2.0,
            auto_y + 13.5 - 0.45 * auto_ht,
            9.2,
            Weight::Bold,
            if state.mem_reduct.auto_clean_enabled { tok::text1() } else { lerp_color(tok::text2(), tok::text1(), auto_ht) },
        );
        p.hit(auto_x, auto_y, auto_w, 20.0, Action::ToggleAutoClean);

        let a_gap_x = 6.0;
        let a_gap_y = 6.0;
        let pill_w = (cw - 28.0 - a_gap_x * 3.0) / 4.0;
        let pill_h = 26.0;
        let areas = [
            ("WS", state.mem_reduct.clean_working_sets, Action::ToggleArea(0)),
            ("SysWS", state.mem_reduct.clean_system_ws, Action::ToggleArea(1)),
            ("Standby", state.mem_reduct.clean_standby_list, Action::ToggleArea(2)),
            ("ModPg", state.mem_reduct.clean_modified_page_list, Action::ToggleArea(3)),
            ("StbyLo", state.mem_reduct.clean_standby_low, Action::ToggleArea(4)),
            ("ModFile", state.mem_reduct.clean_modified_file_cache, Action::ToggleArea(5)),
            ("RegCch", state.mem_reduct.clean_registry_cache, Action::ToggleArea(6)),
            ("Combine", state.mem_reduct.clean_combine_memory_lists, Action::ToggleArea(7)),
        ];
        for (idx, (label, active, act)) in areas.iter().enumerate() {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let px = mx + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = c2_y + 38.0 + (row_i as f32 * (pill_h + a_gap_y));
            let ht = p.hot_t(px, py, pill_w, pill_h);
            p.tactile_surface_pill_t(px, py, pill_w, pill_h, 7.0, ht, if *active { Some(tok::cyno_violet()) } else { None });
            p.text_center(
                label,
                px + pill_w / 2.0,
                py + 16.8 - 0.45 * ht,
                8.8,
                if *active { Weight::Bold } else { Weight::Semibold },
                if *active { tok::text1() } else { lerp_color(tok::text2(), tok::text1(), ht) },
            );
            p.hit(px, py, pill_w, pill_h, *act);
        }

        // Card 3: Rules (y = 314..386, h = 72)
        let c3_y = c2_y + c2_h + 10.0;
        let c3_h = 72.0;
        p.elevated_card(mx, c3_y, cw, c3_h, 15.0, None);
        p.text("Clean Above", mx + 14.0, c3_y + 20.0, 10.0, Weight::Semibold, tok::text2());
        p.text_right("Clean Every", mx + cw - 14.0, c3_y + 20.0, 10.0, Weight::Semibold, tok::text2());

        let chip_y = c3_y + 29.0;
        let chip_h = 30.0;
        let chip_w = (cw - 28.0 - 8.0) / 2.0;
        let chip1_x = mx + 14.0;
        let chip2_x = chip1_x + chip_w + 8.0;

        let c1_ht = p.hot_t(mx, c3_y, cw / 2.0, c3_h);
        p.tactile_surface_pill_t(chip1_x, chip_y, chip_w, chip_h, 8.5, c1_ht, None);
        let (th_str, th_col) = if state.mem_reduct.when_above_pct == 0 {
            ("Disabled".to_string(), tok::text2())
        } else {
            (format!("> {}% RAM", state.mem_reduct.when_above_pct), tok::text1())
        };
        p.text_center(&th_str, chip1_x + chip_w / 2.0, chip_y + 19.0 - 0.55 * c1_ht, 11.0, Weight::Bold, th_col);
        p.hit(mx, c3_y, cw / 2.0, c3_h, Action::CycleThreshold);

        let c2_ht = p.hot_t(mx + cw / 2.0, c3_y, cw / 2.0, c3_h);
        p.tactile_surface_pill_t(chip2_x, chip_y, chip_w, chip_h, 8.5, c2_ht, None);
        let (int_str, int_col) = if state.mem_reduct.every_mins == 0 {
            ("Disabled".to_string(), tok::text2())
        } else {
            (format!("Every {}m", state.mem_reduct.every_mins), tok::cyno_mist())
        };
        p.text_center(&int_str, chip2_x + chip_w / 2.0, chip_y + 19.0 - 0.55 * c2_ht, 11.0, Weight::Bold, int_col);
        p.hit(mx + cw / 2.0, c3_y, cw / 2.0, c3_h, Action::CycleInterval);

        // Card 4: Purge Memory CTA (y = 396..464, h = 68)
        let c4_y = c3_y + c3_h + 10.0;
        let c4_h = 68.0;
        let purge_ht = p.hot_t(mx, c4_y, cw, c4_h);
        p.luminous_pill_button_t(mx, c4_y, cw, c4_h, 15.0, tok::cyno_indigo(), tok::cyno_violet(), tok::cyno_orchid(), purge_ht);

        let dy = -0.8 * purge_ht;
        p.circle(mx + 30.0, c4_y + c4_h / 2.0 + dy, 16.0, rgba(12, 7, 26, 165));
        p.circle_stroke(mx + 30.0, c4_y + c4_h / 2.0 + dy, 16.0, 1.0, rgba(255, 255, 255, 130));
        if state.show_mascot && state.mascot_mood() == MascotMood::Purging {
            let anim_t = if state.motion_preset == MotionPreset::Reduced { 0.0 } else { state.anim_time };
            Self::draw_cyno_mascot(p, mx + 30.0, c4_y + c4_h / 2.0 + dy + 0.5, 0.72, MascotMood::Purging, anim_t, true);
        } else {
            icons::draw(p, Icon::Zap, mx + 22.5, c4_y + c4_h / 2.0 - 7.5 + dy, 15.0, tok::cyno_mist());
        }
        let cta_title = if state.mem_reduct.is_purging { "Cleaning Memory..." } else { "Clean Memory" };
        p.text_shadow(cta_title, mx + 54.0, c4_y + 30.0 + dy, 13.5, Weight::Bold, tok::text1());
        let status_sub = p.ellipsize(&state.mem_reduct.last_cleaned_status, 9.8, Weight::Semibold, cw - 66.0);
        p.text_shadow(&status_sub, mx + 54.0, c4_y + 47.0 + dy, 9.8, Weight::Semibold, rgba(241, 245, 249, 245));
        p.hit(mx, c4_y, cw, c4_h, Action::PurgeRam);
    }

    fn draw_focustimer_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Top Card: Large Circular Gauge (y = 88..274, h = 186)
        let c1_h = 186.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            16.0,
            Some((cw * 0.5, c1_h * 0.5, 82.0, if state.timer.is_break { tok::cyan_hi() } else { tok::cyno_violet() })),
        );
        let cx = mx + cw / 2.0;
        let cy = card_y + c1_h / 2.0;
        let r = 62.0;

        let well = p.well_col();
        p.circle(cx, cy, r - 4.0, well);
        p.arc(cx, cy, r, 0.0, std::f32::consts::TAU, 6.5, rgba(255, 255, 255, 18));

        let frac = (state.timer.remaining_secs as f32 / state.timer.total_secs.max(1) as f32).clamp(0.0, 1.0);
        let ring_col = if state.timer.is_break { tok::cyan_hi() } else { tok::cyno_violet_hi() };
        let sweep = std::f32::consts::TAU * frac;
        let start_a = -std::f32::consts::FRAC_PI_2;
        p.arc(cx, cy, r, start_a, sweep, 11.0, fade(ring_col, 0.25));
        p.arc(cx, cy, r, start_a, sweep, 6.0, ring_col);

        if frac > 0.02 {
            let end_a = start_a + sweep;
            let ox = cx + r * end_a.cos();
            let oy = cy + r * end_a.sin();
            p.radial_glow(ox, oy, 12.0, tok::cyno_mist(), BlendMode::Plus, false);
            p.circle(ox, oy, 4.5, rgba(255, 255, 255, 255));
        }

        let mins = state.timer.remaining_secs / 60;
        let secs = state.timer.remaining_secs % 60;
        p.text_center(&format!("{:02}:{:02}", mins, secs), cx, cy + 7.5, 25.0, Weight::Bold, tok::text1());
        let status = match (state.timer.is_break, state.timer.is_running) {
            (true, true) => "BREAK ACTIVE".to_string(),
            (true, false) => "BREAK READY".to_string(),
            (false, true) => format!("DEEP WORK • #{}", state.timer.cycles_completed + 1),
            (false, false) => format!("READY • #{}", state.timer.cycles_completed + 1),
        };
        p.text_center(&status, cx, cy + 26.0, 9.2, Weight::Bold, tok::cyno_mist());

        // Bottom Card: Duration Presets + Work/Break + Start/Reset (y = 286..464, h = 178)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 178.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 16.0, None);
        p.text("Session Duration", mx + 16.0, c2_y + 27.0, 13.0, Weight::Bold, tok::text1());
        p.text("Pick focus or switch to break", mx + 16.0, c2_y + 44.0, 10.0, Weight::Regular, tok::text2());

        let ph_w = 84.0;
        let ph_x = mx + cw - ph_w - 14.0;
        let ph_y = c2_y + 15.0;
        let ph_h = 24.0;
        let ph_ht = p.hot_t(ph_x, ph_y, ph_w, ph_h);
        p.tactile_surface_pill_t(ph_x, ph_y, ph_w, ph_h, 12.0, ph_ht, if state.timer.is_break { Some(rgba(56, 189, 248, 220)) } else { None });
        let ph_lbl = if state.timer.is_break { format!("Break {}m", state.default_break_mins) } else { "Work Mode".to_string() };
        p.text_center(&ph_lbl, ph_x + ph_w / 2.0, ph_y + 15.5 - 0.55 * ph_ht, 9.5, Weight::Bold, lerp_color(tok::text_soft(), tok::text1(), ph_ht));
        p.hit(ph_x, ph_y, ph_w, ph_h, Action::ToggleTimerPhase);

        let presets: [(u32, &str); 4] = [(5, "5m"), (15, "15m"), (25, "25m"), (45, "45m")];
        let p_gap = 8.0;
        let p_w = (cw - 32.0 - (p_gap * 3.0)) / 4.0;
        let p_y = c2_y + 62.0;
        let p_h = 34.0;
        for (i, &(m, lbl)) in presets.iter().enumerate() {
            let px = mx + 16.0 + (i as f32 * (p_w + p_gap));
            let is_cur = state.timer.total_secs == m * 60;
            let ht = p.hot_t(px, p_y, p_w, p_h);
            p.tactile_surface_pill_t(px, p_y, p_w, p_h, 8.5, ht, if is_cur { Some(tok::cyno_violet()) } else { None });
            p.text_center(
                lbl,
                px + p_w / 2.0,
                p_y + 21.0 - 0.55 * ht,
                11.0,
                if is_cur { Weight::Bold } else { Weight::Semibold },
                if is_cur { tok::text1() } else { lerp_color(tok::text_soft(), tok::text1(), ht) },
            );
            p.hit(px, p_y, p_w, p_h, Action::SetFocusDuration(m));
        }

        let btn_y = c2_y + 116.0;
        let btn_h = 40.0;
        let btn1_w = (cw - 32.0 - 10.0) * 0.65;
        let btn2_w = (cw - 32.0 - 10.0) * 0.35;
        let btn1_x = mx + 16.0;
        let btn2_x = btn1_x + btn1_w + 10.0;

        let b1_ht = p.hot_t(btn1_x, btn_y, btn1_w, btn_h);
        p.luminous_pill_button_t(btn1_x, btn_y, btn1_w, btn_h, 11.5, tok::cyno_indigo(), tok::cyno_violet(), tok::cyno_orchid(), b1_ht);
        let btn1_txt = if state.timer.is_running { "Pause Focus" } else { "Start Focus" };
        p.text_center_shadow(btn1_txt, btn1_x + btn1_w / 2.0, btn_y + 24.5 - 0.8 * b1_ht, 12.0, Weight::Bold, tok::text1());
        p.hit(btn1_x, btn_y, btn1_w, btn_h, Action::ToggleFocusTimer);

        let b2_ht = p.hot_t(btn2_x, btn_y, btn2_w, btn_h);
        p.tactile_surface_pill_t(btn2_x, btn_y, btn2_w, btn_h, 11.5, b2_ht, None);
        p.text_center("Reset", btn2_x + btn2_w / 2.0, btn_y + 24.5 - 0.55 * b2_ht, 11.5, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), b2_ht));
        p.hit(btn2_x, btn_y, btn2_w, btn_h, Action::ResetFocusTimer);
    }

    fn draw_tasks_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;
        let card_h = 376.0;
        p.elevated_card(mx, card_y, cw, card_h, 16.0, Some((cw - 36.0, 22.0, 82.0, tok::cyno_violet())));

        icons::draw(p, Icon::List, mx + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Tasks & Todos", mx + 42.0, card_y + 28.0, 13.5, Weight::Bold, tok::text1());

        let total = state.tasks.len();
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        let start_idx = state.tasks_scroll.min(total.saturating_sub(5));
        let sub = if total > 5 {
            format!("{} open • {}-{} of {} (scroll)", pending, start_idx + 1, (start_idx + 5).min(total), total)
        } else {
            format!("{} open • Click item or × to manage", pending)
        };
        p.text(&sub, mx + 42.0, card_y + 44.0, 10.0, Weight::Regular, tok::text2());

        let btn_y = card_y + 58.0;
        let btn_h = 28.0;
        let btn_w = (cw - 32.0 - 8.0) / 2.0;
        let add_ht = p.hot_t(mx + 16.0, btn_y, btn_w, btn_h);
        p.tactile_surface_pill_t(mx + 16.0, btn_y, btn_w, btn_h, 8.5, add_ht, Some(tok::cyno_violet()));
        p.text_center("+ From Clip", mx + 16.0 + btn_w / 2.0, btn_y + 18.0 - 0.55 * add_ht, 10.5, Weight::Bold, tok::text1());
        p.hit(mx + 16.0, btn_y, btn_w, btn_h, Action::PasteAsTask);

        let clr_x = mx + 16.0 + btn_w + 8.0;
        let clr_ht = p.hot_t(clr_x, btn_y, btn_w, btn_h);
        p.tactile_surface_pill_t(clr_x, btn_y, btn_w, btn_h, 8.5, clr_ht, None);
        p.text_center("Clear Done", clr_x + btn_w / 2.0, btn_y + 18.0 - 0.55 * clr_ht, 10.5, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), clr_ht));
        p.hit(clr_x, btn_y, btn_w, btn_h, Action::ClearCompletedTasks);

        if state.tasks.is_empty() {
            p.text_center("All tasks clear!", mx + cw / 2.0, card_y + 195.0, 12.0, Weight::Semibold, tok::text1());
            p.text_center("Copy text & tap '+ From Clip'", mx + cw / 2.0, card_y + 216.0, 10.5, Weight::Regular, tok::text2());
        } else {
            for (slot, (real_idx, task)) in state.tasks.iter().enumerate().skip(start_idx).take(5).enumerate() {
                let item_y = card_y + 100.0 + (slot as f32 * 52.0);
                let item_h = 40.0;
                let item_x = mx + 16.0;
                let del_w = 34.0;
                let row_w = cw - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 9.5, row_ht, None);
                let dy = -0.55 * row_ht;
                let check_cx = item_x + 17.0;
                let check_cy = item_y + item_h / 2.0 + dy;
                if task.completed {
                    p.radial_glow(check_cx, check_cy, 11.0, rgba(139, 92, 246, 95), BlendMode::Plus, false);
                    p.circle(check_cx, check_cy, 7.0, tok::cyno_violet());
                    icons::draw(p, Icon::Check, check_cx - 4.5, check_cy - 4.5, 9.0, rgba(255, 255, 255, 255));
                } else {
                    let well = p.well_col();
                    p.circle(check_cx, check_cy, 7.0, well);
                    p.circle_stroke(check_cx, check_cy, 7.0, 1.0, rgba(255, 255, 255, 70));
                }
                let task_text = p.ellipsize(&task.text, 11.2, Weight::Regular, row_w - 42.0);
                let text_color = if task.completed { tok::text3() } else { tok::text1() };
                p.text(&task_text, item_x + 32.0, item_y + 24.0 + dy, 11.2, Weight::Regular, text_color);
                p.hit(item_x, item_y, row_w, item_h, Action::ToggleTask(real_idx));

                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 9.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(del_x, item_y - 0.55 * del_ht, del_w, item_h, 9.5, fade(rgba(220, 38, 38, 195), del_ht));
                }
                icons::draw(p, Icon::Close, del_x + 11.0, item_y + 14.0 - 0.55 * del_ht, 12.0, lerp_color(tok::text2(), tok::text1(), del_ht));
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteTask(real_idx));
            }
        }
    }

    fn draw_shelf_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;
        let card_h = 376.0;
        p.elevated_card(mx, card_y, cw, card_h, 16.0, Some((cw - 36.0, 22.0, 82.0, tok::cyno_orchid())));

        icons::draw(p, Icon::Clipboard, mx + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Clipboard & Shelf", mx + 42.0, card_y + 28.0, 13.5, Weight::Bold, tok::text1());

        let clr_w = 56.0;
        let clr_h = 26.0;
        let clr_x = mx + cw - clr_w - 14.0;
        let clr_y = card_y + 14.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, clr_h);
        p.tactile_surface_pill_t(clr_x, clr_y, clr_w, clr_h, 8.0, clr_ht, None);
        p.text_center("Clear", clr_x + clr_w / 2.0, clr_y + 17.0 - 0.55 * clr_ht, 10.0, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), clr_ht));
        p.hit(clr_x, clr_y, clr_w, clr_h, Action::ClearShelf);

        let total = state.shelf_items.len();
        let start_idx = state.shelf_scroll.min(total.saturating_sub(6));
        let sub = if total > 6 {
            format!("Drop files or Ctrl+C • {}-{} of {}", start_idx + 1, (start_idx + 6).min(total), total)
        } else {
            "Drop files or Ctrl+C • Click to open/copy".to_string()
        };
        let sub_ell = p.ellipsize(&sub, 9.8, Weight::Regular, cw - 32.0);
        p.text(&sub_ell, mx + 16.0, card_y + 46.0, 9.8, Weight::Regular, tok::text2());

        if state.shelf_items.is_empty() {
            p.text_center("Shelf empty", mx + cw / 2.0, card_y + 190.0, 12.0, Weight::Semibold, tok::text1());
            p.text_center("Drop files or copy text anywhere", mx + cw / 2.0, card_y + 212.0, 10.2, Weight::Regular, tok::text2());
        } else {
            for (slot, (real_idx, item)) in state.shelf_items.iter().enumerate().skip(start_idx).take(6).enumerate() {
                let item_y = card_y + 64.0 + (slot as f32 * 50.0);
                let item_h = 38.0;
                let item_x = mx + 16.0;
                let del_w = 34.0;
                let row_w = cw - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 9.0, row_ht, None);
                let dy = -0.55 * row_ht;
                if row_ht > 0.02 {
                    p.rrect(item_x, item_y + dy, row_w, item_h, 9.0, fade(tok::cyno_violet(), 0.25 * row_ht));
                }
                let (ic, ic_col) = match item.kind {
                    ShelfKind::File => (Icon::File, tok::cyno_mist()),
                    ShelfKind::Text => (Icon::Clipboard, tok::cyno_violet_hi()),
                };
                icons::draw(p, ic, item_x + 11.0, item_y + 12.5 + dy, 13.0, ic_col);

                let clip_snippet = p.ellipsize(&item.label, 11.0, Weight::Regular, row_w - 42.0);
                p.text(&clip_snippet, item_x + 31.0, item_y + 23.0 + dy, 11.0, Weight::Regular, tok::text1());
                p.hit(item_x, item_y, row_w, item_h, Action::ActivateShelfItem(real_idx));

                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 9.0, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(del_x, item_y - 0.55 * del_ht, del_w, item_h, 9.0, fade(rgba(220, 38, 38, 195), del_ht));
                }
                icons::draw(p, Icon::Close, del_x + 11.0, item_y + 13.0 - 0.55 * del_ht, 12.0, lerp_color(tok::text2(), tok::text1(), del_ht));
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteShelfItem(real_idx));
            }
        }
    }

    fn draw_capture_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: Instant Capture Triggers (y = 88..270, h = 182)
        let c1_h = 182.0;
        p.elevated_card(mx, card_y, cw, c1_h, 16.0, Some((cw * 0.5, 42.0, 78.0, tok::cyno_violet())));
        icons::draw(p, Icon::Video, mx + 16.0, card_y + 18.0, 18.0, tok::cyno_violet_hi());
        p.text("Screen Snapshot", mx + 42.0, card_y + 30.0, 13.5, Weight::Bold, tok::text1());
        p.text("Saves PNG & copies (Ctrl+Shift+S)", mx + 16.0, card_y + 50.0, 10.2, Weight::Regular, tok::text2());

        let btn_x = mx + 16.0;
        let btn_w = cw - 32.0;
        let b1_y = card_y + 70.0;
        let b1_h = 44.0;
        let b1_ht = p.hot_t(btn_x, b1_y, btn_w, b1_h);
        p.luminous_pill_button_t(btn_x, b1_y, btn_w, b1_h, 11.5, tok::cyno_indigo(), tok::cyno_violet(), tok::cyno_orchid(), b1_ht);
        p.text_center_shadow("Capture Full Screen", btn_x + btn_w / 2.0, b1_y + 26.5 - 0.8 * b1_ht, 12.5, Weight::Bold, tok::text1());
        p.hit(btn_x, b1_y, btn_w, b1_h, Action::CaptureScreen);

        let b2_y = card_y + 126.0;
        let b2_h = 36.0;
        let b2_ht = p.hot_t(btn_x, b2_y, btn_w, b2_h);
        p.tactile_surface_pill_t(btn_x, b2_y, btn_w, b2_h, 10.5, b2_ht, None);
        p.text_center("Capture Active Window", btn_x + btn_w / 2.0, b2_y + 22.0 - 0.55 * b2_ht, 11.5, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), b2_ht));
        p.hit(btn_x, b2_y, btn_w, b2_h, Action::CaptureWindow);

        // Card 2: Capture Output (y = 282..464, h = 182)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 182.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 16.0, Some((cw - 32.0, 26.0, 76.0, tok::cyno_orchid())));
        p.dot_matrix(mx + cw - 46.0, c2_y + 16.0, 5, 3, 5.0, rgba(255, 255, 255, 30));
        icons::draw(p, Icon::Folder, mx + 16.0, c2_y + 18.0, 18.0, tok::cyno_mist());
        p.text("Capture Output", mx + 42.0, c2_y + 30.0, 13.5, Weight::Bold, tok::text1());

        if let Some(ref _path) = state.capture.last_path {
            let fname = p.ellipsize(&state.capture.last_filename, 11.8, Weight::Semibold, cw - 32.0);
            p.text(&fname, mx + 16.0, c2_y + 72.0, 11.8, Weight::Semibold, tok::text1());
            let dims = format!(
                "{} • {}",
                state.capture.last_dims,
                if state.capture.clipboard_ok { "Copied to Clipboard" } else { "Saved to Disk" }
            );
            p.text(&dims, mx + 16.0, c2_y + 92.0, 10.5, Weight::Regular, tok::cyno_violet_hi());
        } else {
            p.text("No capture taken in this session", mx + 16.0, c2_y + 72.0, 11.8, Weight::Semibold, tok::text1());
            p.text("Output: Pictures\\CyNotch\\*.png", mx + 16.0, c2_y + 92.0, 10.5, Weight::Regular, tok::text2());
        }

        let open_y = c2_y + 126.0;
        let open_h = 36.0;
        let open_ht = p.hot_t(btn_x, open_y, btn_w, open_h);
        p.tactile_surface_pill_t(btn_x, open_y, btn_w, open_h, 10.5, open_ht, Some(tok::cyno_violet()));
        p.text_center("Open Pictures\\CyNotch", btn_x + btn_w / 2.0, open_y + 22.0 - 0.55 * open_ht, 11.2, Weight::Bold, tok::text1());
        p.hit(btn_x, open_y, btn_w, open_h, Action::OpenCaptureFolder);
    }

    fn draw_telemetry_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Row 1: Two side-by-side summary cards (Today & 7-day avg, y = 88..198, h = 110)
        let r1_h = 110.0;
        let sub_gap = 12.0;
        let sub_w = (cw - sub_gap) / 2.0;

        p.elevated_card(mx, card_y, sub_w, r1_h, 15.0, Some((sub_w - 22.0, 16.0, 56.0, tok::cyno_violet())));
        p.dot_matrix(mx + sub_w - 38.0, card_y + 10.0, 4, 3, 5.0, rgba(255, 255, 255, 30));
        p.text("Today", mx + 14.0, card_y + 24.0, 10.5, Weight::Semibold, tok::text2());
        let today_h = state.screen_time.today_secs / 3600;
        let today_m = (state.screen_time.today_secs % 3600) / 60;
        p.text(&format!("{} h {} m", today_h, today_m), mx + 14.0, card_y + 62.0, 18.0, Weight::Bold, tok::text1());
        let cpu_sub = format!("CPU {:.0}% • RAM {:.0}%", state.telemetry.cpu_usage, state.telemetry.memory_percent);
        p.text(&cpu_sub, mx + 14.0, card_y + 92.0, 9.2, Weight::Regular, tok::text2());

        let r_x = mx + sub_w + sub_gap;
        p.elevated_card(r_x, card_y, sub_w, r1_h, 15.0, None);
        p.text("7-day avg", r_x + 14.0, card_y + 24.0, 10.5, Weight::Semibold, tok::text2());
        p.text_right(&format!("{}d log", state.screen_time.tracked_days.max(1)), r_x + sub_w - 14.0, card_y + 24.0, 9.2, Weight::Semibold, tok::text2());
        let avg_h = state.screen_time.seven_day_avg_secs / 3600;
        let avg_m = (state.screen_time.seven_day_avg_secs % 3600) / 60;
        p.text(&format!("{} h {} m", avg_h, avg_m), r_x + 14.0, card_y + 62.0, 18.0, Weight::Bold, tok::cyno_violet_hi());
        let net_sub = format!("↓{}  ↑{}", format_speed_short(state.telemetry.net_down_kbps), format_speed_short(state.telemetry.net_up_kbps));
        let net_ell = p.ellipsize(&net_sub, 9.5, Weight::Semibold, sub_w - 28.0);
        p.text(&net_ell, r_x + 14.0, card_y + 92.0, 9.5, Weight::Semibold, tok::cyan_hi());

        // Row 2: Most Used App (y = 210..320, h = 110)
        let r2_y = card_y + r1_h + 12.0;
        let r2_h = 110.0;
        p.elevated_card(mx, r2_y, cw, r2_h, 15.0, Some((cw - 24.0, 18.0, 66.0, tok::cyno_orchid())));
        p.text("Most used app", mx + 16.0, r2_y + 23.0, 10.8, Weight::Semibold, tok::text2());
        if let Some((second_name, second_secs)) = state.screen_time.top_apps.get(1) {
            let runner = p.ellipsize(&format!("#2 {} {}m", second_name, second_secs / 60), 9.8, Weight::Regular, cw - 126.0);
            p.text_right(&runner, mx + cw - 16.0, r2_y + 23.0, 9.8, Weight::Regular, tok::text2());
        }
        let top_name = p.ellipsize(&state.screen_time.top_app_name, 16.5, Weight::Bold, cw - 110.0);
        p.text(&top_name, mx + 16.0, r2_y + 54.0, 16.5, Weight::Bold, tok::cyno_mist());
        let top_h = state.screen_time.top_app_secs / 3600;
        let top_m = (state.screen_time.top_app_secs % 3600) / 60;
        p.text_right(&format!("{} h {} m", top_h, top_m), mx + cw - 16.0, r2_y + 54.0, 12.5, Weight::Semibold, tok::text1());

        let share = if state.screen_time.today_secs > 0 {
            (state.screen_time.top_app_secs as f32 / state.screen_time.today_secs as f32).clamp(0.08, 1.0)
        } else {
            0.0
        };
        p.glowing_striped_bar(mx + 16.0, r2_y + 74.0, cw - 32.0, 8.0, share, tok::cyno_deep(), tok::cyno_orchid());

        // Row 3: Peak Hour + Full-width 24h Sparkline (y = 332..464, h = 132)
        let r3_y = r2_y + r2_h + 12.0;
        let r3_h = 132.0;
        p.elevated_card(mx, r3_y, cw, r3_h, 15.0, Some((cw * 0.5, 42.0, 68.0, tok::cyno_violet())));
        p.text("Peak hour & 24h Activity", mx + 16.0, r3_y + 24.0, 11.0, Weight::Semibold, tok::text2());
        p.text_right(&state.screen_time.peak_hour_str, mx + cw - 16.0, r3_y + 25.0, 15.5, Weight::Bold, tok::text1());

        let mut bins = [0u64; 12];
        for (h_idx, &secs) in state.screen_time.hourly_secs.iter().enumerate() {
            bins[h_idx / 2] += secs;
        }
        let max_bin = bins.iter().copied().max().unwrap_or(1).max(1);
        let peak_bin = state.screen_time.peak_hour / 2;
        let chart_x = mx + 20.0;
        let chart_w = cw - 40.0;
        let slot_w = chart_w / 12.0;
        let bar_w = (slot_w - 6.0).max(8.0);
        let chart_base_y = r3_y + 108.0;
        let max_bar_h = 56.0;
        for (b, &val) in bins.iter().enumerate() {
            let bx = chart_x + (b as f32 * slot_w) + (slot_w - bar_w) * 0.5;
            let bh = if val == 0 {
                3.0
            } else {
                ((val as f32 / max_bin as f32) * max_bar_h).clamp(6.0, max_bar_h)
            };
            let bar_col = if val > 0 && b == peak_bin {
                tok::cyno_mist()
            } else if val > 0 {
                tok::cyno_violet()
            } else {
                rgba(255, 255, 255, 22)
            };
            p.rrect(bx, chart_base_y - bh, bar_w, bh, 3.0, bar_col);
        }
        p.text("00h", chart_x, r3_y + 123.0, 8.8, Weight::Regular, tok::text3());
        p.text_center("12h", chart_x + chart_w * 0.5, r3_y + 123.0, 8.8, Weight::Regular, tok::text3());
        p.text_right("22h", chart_x + chart_w, r3_y + 123.0, 8.8, Weight::Regular, tok::text3());
    }

    fn draw_settings_view_vertical(p: &mut Painter, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions && state.motion_preset != MotionPreset::Reduced;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: System Integration (y = 88..254, h = 166)
        let c1_h = 166.0;
        p.elevated_card(mx, card_y, cw, c1_h, 16.0, Some((32.0, 22.0, 72.0, tok::cyno_violet())));
        icons::draw(p, Icon::Settings, mx + 16.0, card_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("System Integration", mx + 42.0, card_y + 27.5, 13.0, Weight::Bold, tok::text1());
        p.text("Startup, chime & global hotkeys", mx + 42.0, card_y + 43.0, 10.0, Weight::Regular, tok::text2());

        let sw_w = 46.0;
        let sw_h = 22.0;
        let r1_y = card_y + 62.0;
        p.text("Launch on Startup", mx + 16.0, r1_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let sw1_x = mx + cw - sw_w - 16.0;
        let sw1_ht = p.hot_t(sw1_x, r1_y, sw_w, sw_h);
        let st1 = if smooth { state.switch_t[ToggleId::Autostart as usize] } else if state.autostart { 1.0 } else { 0.0 };
        p.draw_toggle_switch(sw1_x, r1_y, sw_w, sw_h, st1, sw1_ht);
        p.hit(sw1_x, r1_y, sw_w, sw_h, Action::ToggleAutostart);

        let r2_y = card_y + 94.0;
        p.text("Chime Audio", mx + 16.0, r2_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let sw2_ht = p.hot_t(sw1_x, r2_y, sw_w, sw_h);
        let st2 = if smooth { state.switch_t[ToggleId::Chime as usize] } else if state.chime_enabled { 1.0 } else { 0.0 };
        p.draw_toggle_switch(sw1_x, r2_y, sw_w, sw_h, st2, sw2_ht);
        p.hit(sw1_x, r2_y, sw_w, sw_h, Action::ToggleChime);

        let r3_y = card_y + 128.0;
        p.text("Hotkeys (C/M/S)", mx + 16.0, r3_y + 15.0, 11.5, Weight::Regular, tok::text1());
        let badge_w = 112.0;
        let badge_x = mx + cw - badge_w - 16.0;
        let well = p.well_col();
        p.rrect(badge_x, r3_y, badge_w, 22.0, 7.0, well);
        p.rrect_bevel_stroke(badge_x, r3_y, badge_w, 22.0, 7.0, 1.0, rgba(192, 132, 252, 90), rgba(255, 255, 255, 20));
        p.text_center("Ctrl+Shift+C/M/S", badge_x + badge_w / 2.0, r3_y + 14.8, 9.2, Weight::Semibold, tok::cyno_violet_hi());

        // Card 2: Health & Engine (y = 266..464, h = 198)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 198.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 16.0, Some((cw - 28.0, 22.0, 72.0, tok::cyno_orchid())));
        icons::draw(p, Icon::Droplet, mx + 16.0, c2_y + 16.0, 18.0, tok::cyno_violet_hi());
        p.text("Health & Engine", mx + 42.0, c2_y + 27.5, 13.0, Weight::Bold, tok::text1());
        p.text("Hydration interval & lifecycle", mx + 42.0, c2_y + 43.0, 10.0, Weight::Regular, tok::text2());

        p.text("Water Interval:", mx + 16.0, c2_y + 72.0, 11.0, Weight::Semibold, tok::text1());
        let intervals: [(u32, &str); 4] = [(30, "30m"), (45, "45m"), (60, "60m"), (0, "Off")];
        let pb_gap = 6.0;
        let pb_w = (cw - 32.0 - (pb_gap * 3.0)) / 4.0;
        let pb_y = c2_y + 84.0;
        let pb_h = 30.0;
        for (i, &(mins, label)) in intervals.iter().enumerate() {
            let px = mx + 16.0 + (i as f32 * (pb_w + pb_gap));
            let is_cur = state.water_interval_mins == mins;
            let ht = p.hot_t(px, pb_y, pb_w, pb_h);
            p.tactile_surface_pill_t(px, pb_y, pb_w, pb_h, 8.0, ht, if is_cur { Some(tok::cyno_violet()) } else { None });
            p.text_center(
                label,
                px + pb_w / 2.0,
                pb_y + 19.0 - 0.55 * ht,
                10.5,
                if is_cur { Weight::Bold } else { Weight::Semibold },
                if is_cur { tok::text1() } else { lerp_color(tok::text_soft(), tok::text1(), ht) },
            );
            p.hit(px, pb_y, pb_w, pb_h, Action::SetWaterInterval(mins));
        }

        let bot_y = c2_y + 138.0;
        let bot_h = 38.0;
        let quit_w = 88.0;
        let spec_w = cw - 32.0 - quit_w - 8.0;
        let moved = state.x_offset != 0 || state.y_offset != 0;
        let left_ht = p.hot_t(mx + 16.0, bot_y, spec_w, bot_h);
        p.tactile_surface_pill_t(mx + 16.0, bot_y, spec_w, bot_h, 9.5, left_ht, None);
        let pos_lbl = if moved { "Center Notch" } else { "Advanced Settings" };
        p.text_center(pos_lbl, mx + 16.0 + spec_w / 2.0, bot_y + 23.0 - 0.55 * left_ht, 10.5, Weight::Semibold, lerp_color(tok::text_soft(), tok::text1(), left_ht));
        p.hit(mx + 16.0, bot_y, spec_w, bot_h, if moved { Action::ResetPosition } else { Action::OpenSettings });

        let quit_x = mx + 16.0 + spec_w + 8.0;
        let quit_ht = p.hot_t(quit_x, bot_y, quit_w, bot_h);
        p.tactile_surface_pill_t(quit_x, bot_y, quit_w, bot_h, 9.5, quit_ht, Some(rgba(220, 38, 38, 255)));
        p.text_center_shadow("Exit App", quit_x + quit_w / 2.0, bot_y + 23.0 - 0.55 * quit_ht, 10.5, Weight::Bold, tok::text1());
        p.hit(quit_x, bot_y, quit_w, bot_h, Action::Quit);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // CYBER-ANUBIS CYNO MASCOT (Icon-Matched Sculpted Vector Rig + 6 Mood Loops)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(crate) fn draw_cyno_mascot(
        p: &mut Painter,
        cx: f32,
        cy: f32,
        scale: f32,
        mood: MascotMood,
        anim_time: f32,
        compact_fx: bool,
    ) {
        let k = scale;
        let t = anim_time.max(0.0);

        let rot_at = |deg: f32, px: f32, py: f32| -> Transform {
            Transform::from_translate(px, py)
                .pre_rotate(deg)
                .pre_translate(-px, -py)
        };

        // 1. Derive articulated kinematic & glow parameters per mood (zero-bounce smooth waves)
        let (bob_y, tilt_deg, sx, sy, ear_l_deg, ear_r_deg, eye_open, eye_boost, diamond_boost) =
            match mood {
                MascotMood::Idle => {
                    let bob = (t * 2.1).sin() * 0.72;
                    let tilt = (t * 0.95).sin() * 1.4;
                    let breathe = 1.0 + (t * 2.1 + 0.4).sin() * 0.015;
                    // Smooth single-ear alert twitch inside 5.4s cycle
                    let cycle = t % 5.4;
                    let twitch_l = if (1.2..1.62).contains(&cycle) {
                        ((cycle - 1.2) / 0.42 * std::f32::consts::PI).sin() * 4.5
                    } else {
                        0.0
                    };
                    let twitch_r = if (3.6..4.02).contains(&cycle) {
                        -((cycle - 3.6) / 0.42 * std::f32::consts::PI).sin() * 4.2
                    } else {
                        0.0
                    };
                    let ear_sway = (t * 1.5).sin() * 1.1;
                    // Smooth natural blink every 4.4s
                    let blk = t % 4.4;
                    let open = if blk > 4.18 {
                        let u = (blk - 4.18) / 0.22;
                        1.0 - (u * std::f32::consts::PI).sin() * 0.88
                    } else {
                        1.0
                    };
                    let glow_b = 0.85 + 0.25 * ((t * 2.4).sin() * 0.5 + 0.5);
                    (bob, tilt, breathe, breathe, ear_sway + twitch_l, -ear_sway + twitch_r, open, glow_b, glow_b)
                }
                MascotMood::Purging => {
                    let bob = (t * 9.0).sin() * 0.95 - 0.55;
                    let tilt = (t * 13.0).sin() * 2.1;
                    let surge = 1.0 + 0.042 * ((t * 11.0).sin() * 0.5 + 0.5);
                    let ear_vib = (t * 18.0).sin() * 2.4;
                    (bob, tilt, surge, surge * 1.02, -4.5 + ear_vib, 4.5 - ear_vib, 1.08, 1.75, 1.85)
                }
                MascotMood::Music => {
                    // ~120 BPM groove cadence
                    let beat = t * std::f32::consts::TAU;
                    let bob = (beat * 2.0).sin() * 0.95;
                    let tilt = beat.sin() * 4.6;
                    let groove = 1.0 + 0.03 * ((beat * 2.0).cos() * 0.5 + 0.5);
                    let ear_l = -1.5 + (beat * 2.0 - 0.35).sin() * 3.8;
                    let ear_r = 1.5 - (beat * 2.0 + 0.35).sin() * 3.8;
                    let pulse = 1.05 + 0.35 * ((beat * 2.0).cos() * 0.5 + 0.5);
                    (bob, tilt, groove, groove, ear_l, ear_r, 1.0, pulse, pulse)
                }
                MascotMood::Focus => {
                    let bob = (t * 1.4).sin() * 0.55;
                    let blk = t % 5.6;
                    let open = if blk > 5.36 {
                        let u = (blk - 5.36) / 0.24;
                        1.0 - (u * std::f32::consts::PI).sin() * 0.85
                    } else {
                        1.0
                    };
                    let amber = 1.1 + 0.2 * ((t * 2.0).sin() * 0.5 + 0.5);
                    (bob, 0.0, 1.0, 1.0, 0.8, -0.8, open, 1.15, amber * 1.25)
                }
                MascotMood::Hydrate => {
                    let bob = (t * 2.8).sin() * 0.85;
                    let tilt = (t * 1.4).sin() * 2.0;
                    let ripple = 1.0 + 0.025 * ((t * 3.5).sin() * 0.5 + 0.5);
                    (bob, tilt, ripple, ripple, -1.2, 1.2, 1.0, 1.2, 1.15)
                }
                MascotMood::Capture => {
                    let flash = ((t * 7.0).sin() * 0.5 + 0.5).powf(2.0);
                    let scale_p = 1.0 + 0.035 * flash;
                    (flash * -0.45, 0.0, scale_p, scale_p, -2.5, 2.5, 1.04, 1.35 + flash * 0.4, 1.3 + flash * 0.4)
                }
            };

        let head_cx = cx;
        let head_cy = cy + bob_y * k;

        let head_tf = Transform::from_translate(head_cx, head_cy)
            .pre_rotate(tilt_deg)
            .pre_scale(k * sx, k * sy);
        let ear_l_tf = head_tf.pre_concat(rot_at(ear_l_deg, -5.5, -5.2));
        let ear_r_tf = head_tf.pre_concat(rot_at(ear_r_deg, 5.5, -5.2));

        // 2. Background Aura & Mood Effects Layer (rendered behind sculpted bust)
        match mood {
            MascotMood::Idle => {
                let a = (18.0 + 14.0 * ((t * 2.2).sin() * 0.5 + 0.5)) as u8;
                p.radial_glow(head_cx, head_cy, 15.0 * k, rgba(139, 92, 246, a), BlendMode::Plus, false);
            }
            MascotMood::Purging => {
                p.radial_glow(head_cx, head_cy, 19.0 * k, rgba(168, 85, 247, 72), BlendMode::Plus, false);
                p.radial_glow(head_cx, head_cy - 3.5 * k, 13.0 * k, rgba(221, 214, 254, 52), BlendMode::Plus, false);
                // Dual staggered expanding shockwave rings
                for ring_i in 0..2 {
                    let phase = (t * 1.45 + ring_i as f32 * 0.5) % 1.0;
                    let max_r = if compact_fx { 13.5 } else { 19.0 };
                    let rr = (6.5 + phase * (max_r - 6.5)) * k;
                    let fade_a = ((1.0 - phase).powf(1.4) * 175.0) as u8;
                    let col = if ring_i == 0 {
                        rgba(192, 132, 252, fade_a)
                    } else {
                        rgba(233, 218, 255, fade_a)
                    };
                    p.circle_stroke(head_cx, head_cy, rr, (1.15 * (1.0 - phase * 0.45) * k).max(0.6), col);
                }
                // Crackling temple & ear lightning arcs
                if !compact_fx {
                    let flip = ((t * 12.0).floor() as i32) % 2 == 0;
                    let jitter = if flip { 0.8 } else { -0.7 };
                    let bolts = [
                        (
                            [-10.2, -13.5, -13.6 + jitter, -8.8, -11.4 - jitter, -4.5],
                            rgba(192, 132, 252, 220),
                        ),
                        (
                            [10.2, -13.5, 13.8 - jitter, -8.6, 11.4 + jitter, -4.2],
                            rgba(233, 218, 255, 225),
                        ),
                    ];
                    for (pts, col) in bolts {
                        let mut pb = PathBuilder::new();
                        pb.move_to(pts[0], pts[1]);
                        pb.line_to(pts[2], pts[3]);
                        pb.line_to(pts[4], pts[5]);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(&path, 1.05, col, Transform::from_translate(head_cx, head_cy).pre_scale(k, k));
                        }
                    }
                }
            }
            MascotMood::Music => {
                let beat_amp = (t * std::f32::consts::TAU * 2.0).cos() * 0.5 + 0.5;
                p.radial_glow(
                    head_cx,
                    head_cy,
                    (15.5 + 3.5 * beat_amp) * k,
                    rgba(168, 85, 247, (44.0 + 36.0 * beat_amp) as u8),
                    BlendMode::Plus,
                    false,
                );
                p.radial_glow(
                    head_cx,
                    head_cy - 5.0 * k,
                    11.0 * k,
                    rgba(221, 214, 254, (24.0 + 24.0 * beat_amp) as u8),
                    BlendMode::Plus,
                    false,
                );
                if !compact_fx && k >= 1.25 {
                    // Music Hero flanking mini equalizer spectrum bars
                    let bar_offsets = [-24.0, -20.5, -17.0, 17.0, 20.5, 24.0];
                    for (bi, &bx_loc) in bar_offsets.iter().enumerate() {
                        let wv = (t * 8.2 + bi as f32 * 1.15).sin() * 0.5 + 0.5;
                        let bh = 3.2 + wv * 7.8;
                        let col = if bi % 2 == 0 {
                            rgba(192, 132, 252, 210)
                        } else {
                            rgba(233, 218, 255, 215)
                        };
                        let mut pb = PathBuilder::new();
                        pb.move_to(bx_loc, -bh * 0.5);
                        pb.line_to(bx_loc, bh * 0.5);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(&path, 1.65, col, Transform::from_translate(head_cx, head_cy).pre_scale(k, k));
                        }
                    }
                }
            }
            MascotMood::Focus => {
                p.radial_glow(head_cx, head_cy - 1.5 * k, 17.0 * k, rgba(168, 85, 247, 46), BlendMode::Plus, false);
                // Rotating Mahamatra Sacred Golden Halo Ring behind helmet
                let halo_r = 14.2;
                let rot_halo = t * 0.85;
                let world_s = Transform::from_translate(head_cx, head_cy - 1.0 * k).pre_scale(k, k);
                for arc_i in 0..3 {
                    let a0 = rot_halo + (arc_i as f32) * (std::f32::consts::TAU / 3.0);
                    let a1 = a0 + 1.48;
                    let steps = 8;
                    let mut pb = PathBuilder::new();
                    for s in 0..=steps {
                        let ang = a0 + (a1 - a0) * (s as f32 / steps as f32);
                        let px = ang.cos() * halo_r;
                        let py = ang.sin() * halo_r;
                        if s == 0 { pb.move_to(px, py); } else { pb.line_to(px, py); }
                    }
                    if let Some(path) = pb.finish() {
                        p.stroke_path_tf(&path, 1.15, rgba(196, 148, 255, 175), world_s);
                    }
                }
                if !compact_fx {
                    for pip_i in 0..2 {
                        let ang = rot_halo * 1.4 + (pip_i as f32) * std::f32::consts::PI;
                        p.circle(
                            head_cx + ang.cos() * (halo_r + 1.6) * k,
                            head_cy - 1.0 * k + ang.sin() * (halo_r + 1.6) * k,
                            1.1 * k,
                            rgba(254, 240, 138, 220),
                        );
                    }
                }
            }
            MascotMood::Hydrate => {
                p.radial_glow(head_cx, head_cy, 16.5 * k, rgba(56, 189, 248, 52), BlendMode::Plus, false);
                let phase = (t * 1.15) % 1.0;
                let max_r = if compact_fx { 13.0 } else { 17.5 };
                let rr = (6.5 + phase * (max_r - 6.5)) * k;
                let fade_a = ((1.0 - phase).powf(1.3) * 165.0) as u8;
                p.circle_stroke(head_cx, head_cy, rr, (1.0 * k).max(0.6), rgba(125, 211, 252, fade_a));
            }
            MascotMood::Capture => {
                p.radial_glow(head_cx, head_cy, 16.0 * k, rgba(192, 132, 252, 54), BlendMode::Plus, false);
                if !compact_fx {
                    let squeeze = 14.2 - ((t * 5.0).sin() * 0.5 + 0.5) * 1.4;
                    let arm = 3.2;
                    let tf_s = Transform::from_translate(head_cx, head_cy).pre_scale(k, k);
                    for (sx_s, sy_s) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        let mut pb = PathBuilder::new();
                        let vx = sx_s * squeeze;
                        let vy = sy_s * squeeze;
                        pb.move_to(vx - sx_s * arm, vy);
                        pb.line_to(vx, vy);
                        pb.line_to(vx, vy - sy_s * arm);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(&path, 0.9, rgba(233, 213, 255, 195), tf_s);
                        }
                    }
                }
            }
        }

        // 3. Dual Sculpted Ceremonial Side Lappets (obsidan wings + champagne-gold inner blades)
        for sign in [-1.0f32, 1.0f32] {
            // Outer Obsidian-Violet Lappet Wing
            let mut pb_wing = PathBuilder::new();
            pb_wing.move_to(sign * 6.0, -3.5);
            pb_wing.line_to(sign * 13.3, 11.4);
            pb_wing.line_to(sign * 10.4, 13.7);
            pb_wing.line_to(sign * 7.4, 12.2);
            pb_wing.line_to(sign * 5.2, 1.5);
            pb_wing.close();
            if let Some(path) = pb_wing.finish() {
                p.fill_linear_path_tf(
                    &path,
                    sign * 6.5,
                    -3.5,
                    sign * 12.2,
                    13.2,
                    rgba(34, 30, 56, 255),
                    rgba(11, 10, 20, 255),
                    Some(rgba(21, 19, 36, 255)),
                    head_tf,
                );
            }

            let mut pb_wing_rim = PathBuilder::new();
            pb_wing_rim.move_to(sign * 6.4, -2.5);
            pb_wing_rim.line_to(sign * 13.0, 11.2);
            if let Some(path) = pb_wing_rim.finish() {
                p.stroke_path_tf(&path, 0.55, rgba(168, 85, 247, 135), head_tf);
            }

            // Inner Brushed Champagne-Gold Lappet Blade
            let mut pb_gold_lap = PathBuilder::new();
            pb_gold_lap.move_to(sign * 5.2, 1.7);
            pb_gold_lap.line_to(sign * 8.4, 2.2);
            pb_gold_lap.line_to(sign * 10.0, 12.9);
            pb_gold_lap.line_to(sign * 5.3, 14.7);
            pb_gold_lap.close();
            if let Some(path) = pb_gold_lap.finish() {
                p.fill_linear_path_tf(
                    &path,
                    sign * 6.8,
                    1.8,
                    sign * 7.6,
                    14.6,
                    rgba(253, 230, 138, 255),
                    rgba(120, 53, 15, 255),
                    Some(rgba(217, 119, 6, 255)),
                    head_tf,
                );
            }
            let mut pb_lap_hi = PathBuilder::new();
            pb_lap_hi.move_to(sign * 5.4, 2.2);
            pb_lap_hi.line_to(sign * 5.4, 14.2);
            if let Some(path) = pb_lap_hi.finish() {
                p.stroke_path_tf(&path, 0.55, rgba(254, 249, 195, 175), head_tf);
            }
        }

        // 4. Tall Pointed Articulated Cyber-Jackal Ears (Obsidian shell + Champagne-Gold inner bowls)
        for (sign, ear_tf) in [(-1.0f32, ear_l_tf), (1.0f32, ear_r_tf)] {
            let mut pb_ear = PathBuilder::new();
            pb_ear.move_to(sign * 7.2, -3.2);
            pb_ear.cubic_to(sign * 9.2, -9.0, sign * 9.2, -14.8, sign * 8.0, -18.2);
            pb_ear.cubic_to(sign * 5.8, -14.2, sign * 4.4, -9.5, sign * 3.8, -5.8);
            pb_ear.close();
            if let Some(path) = pb_ear.finish() {
                p.fill_linear_path_tf(
                    &path,
                    sign * 6.0,
                    -18.0,
                    sign * 6.2,
                    -3.5,
                    rgba(56, 58, 78, 255),
                    rgba(12, 12, 19, 255),
                    Some(rgba(25, 25, 38, 255)),
                    ear_tf,
                );
            }

            let mut pb_ear_rim = PathBuilder::new();
            pb_ear_rim.move_to(sign * 7.3, -4.0);
            pb_ear_rim.cubic_to(sign * 9.0, -9.2, sign * 9.0, -14.6, sign * 8.0, -18.0);
            if let Some(path) = pb_ear_rim.finish() {
                p.stroke_path_tf(&path, 0.65, rgba(203, 213, 225, 135), ear_tf);
            }

            // Inner Champagne Gold Cavity
            let mut pb_gold_ear = PathBuilder::new();
            pb_gold_ear.move_to(sign * 6.7, -4.9);
            pb_gold_ear.cubic_to(sign * 8.1, -9.5, sign * 8.0, -13.9, sign * 7.45, -16.1);
            pb_gold_ear.cubic_to(sign * 6.0, -13.2, sign * 5.0, -9.8, sign * 4.7, -6.3);
            pb_gold_ear.close();
            if let Some(path) = pb_gold_ear.finish() {
                p.fill_linear_path_tf(
                    &path,
                    sign * 6.8,
                    -16.0,
                    sign * 6.0,
                    -5.0,
                    rgba(254, 240, 138, 255),
                    rgba(120, 53, 15, 255),
                    Some(rgba(234, 179, 8, 255)),
                    ear_tf,
                );
            }
        }

        // 5. Obsidian Cranium, Armor Brow, Tapered Jackal Snout & Gold Cheek Chamfers
        let mut pb_head = PathBuilder::new();
        pb_head.move_to(0.0, -8.8);
        pb_head.cubic_to(4.2, -8.8, 7.6, -6.0, 8.2, -1.6);
        pb_head.cubic_to(8.5, 1.0, 6.2, 4.6, 4.3, 8.2);
        pb_head.cubic_to(3.8, 10.6, 2.8, 12.6, 0.0, 12.9);
        pb_head.cubic_to(-2.8, 12.6, -3.8, 10.6, -4.3, 8.2);
        pb_head.cubic_to(-6.2, 4.6, -8.5, 1.0, -8.2, -1.6);
        pb_head.cubic_to(-7.6, -6.0, -4.2, -8.8, 0.0, -8.8);
        pb_head.close();
        if let Some(path) = pb_head.finish() {
            p.fill_linear_path_tf(
                &path,
                0.0,
                -8.8,
                0.0,
                12.9,
                rgba(72, 74, 96, 255),
                rgba(9, 9, 15, 255),
                Some(rgba(26, 26, 38, 255)),
                head_tf,
            );
        }

        // Lower Cheek Metallic Gold Accents
        for sign in [-1.0f32, 1.0f32] {
            let mut pb_chk = PathBuilder::new();
            pb_chk.move_to(sign * 7.5, 0.2);
            pb_chk.line_to(sign * 5.8, 2.6);
            pb_chk.line_to(sign * 5.0, 6.2);
            pb_chk.line_to(sign * 6.9, 3.6);
            pb_chk.close();
            if let Some(path) = pb_chk.finish() {
                p.fill_linear_path_tf(
                    &path,
                    sign * 7.2,
                    0.5,
                    sign * 5.2,
                    6.0,
                    rgba(253, 230, 138, 240),
                    rgba(146, 64, 14, 240),
                    None,
                    head_tf,
                );
            }
        }

        // Recessed Dark Eye Sockets shadow
        for sign in [-1.0f32, 1.0f32] {
            let mut pb_sock = PathBuilder::new();
            pb_sock.move_to(sign * 1.1, 1.4);
            pb_sock.cubic_to(sign * 2.4, -0.5, sign * 4.8, -1.1, sign * 7.0, -1.0);
            pb_sock.cubic_to(sign * 6.0, 1.6, sign * 3.6, 2.5, sign * 1.1, 1.4);
            pb_sock.close();
            if let Some(path) = pb_sock.finish() {
                p.fill_path_tf(&path, rgba(6, 6, 11, 235), head_tf);
            }
        }

        // Sculpted Overhanging V-Brow Plate
        let mut pb_brow = PathBuilder::new();
        pb_brow.move_to(-7.4, -2.5);
        pb_brow.cubic_to(-5.0, -4.4, -2.5, -5.8, 0.0, -6.2);
        pb_brow.cubic_to(2.5, -5.8, 5.0, -4.4, 7.4, -2.5);
        pb_brow.line_to(6.0, -1.3);
        pb_brow.line_to(1.8, -0.9);
        pb_brow.line_to(0.0, 0.25);
        pb_brow.line_to(-1.8, -0.9);
        pb_brow.line_to(-6.0, -1.3);
        pb_brow.close();
        if let Some(path) = pb_brow.finish() {
            p.fill_linear_path_tf(
                &path,
                0.0,
                -6.2,
                0.0,
                0.3,
                rgba(48, 47, 68, 255),
                rgba(15, 14, 24, 255),
                None,
                head_tf,
            );
        }
        let mut pb_brow_rim = PathBuilder::new();
        pb_brow_rim.move_to(-6.9, -1.7);
        pb_brow_rim.line_to(-1.8, -0.9);
        pb_brow_rim.line_to(0.0, 0.25);
        pb_brow_rim.line_to(1.8, -0.9);
        pb_brow_rim.line_to(6.9, -1.7);
        if let Some(path) = pb_brow_rim.finish() {
            p.stroke_path_tf(&path, 0.55, rgba(203, 213, 225, 130), head_tf);
        }

        // Raised Jackal Nose Bridge & Specular Ridge
        let mut pb_bridge = PathBuilder::new();
        pb_bridge.move_to(0.0, 0.2);
        pb_bridge.line_to(1.7, 7.9);
        pb_bridge.line_to(0.0, 8.3);
        pb_bridge.line_to(-1.7, 7.9);
        pb_bridge.close();
        if let Some(path) = pb_bridge.finish() {
            p.fill_linear_path_tf(
                &path,
                0.0,
                0.2,
                0.0,
                8.3,
                rgba(52, 53, 72, 255),
                rgba(19, 19, 30, 255),
                None,
                head_tf,
            );
        }
        let mut pb_ridge = PathBuilder::new();
        pb_ridge.move_to(0.0, 0.9);
        pb_ridge.line_to(0.0, 7.6);
        if let Some(path) = pb_ridge.finish() {
            p.stroke_path_tf(&path, 0.65, rgba(203, 213, 225, 110), head_tf);
        }

        // Glossy Pitch-Black Jackal Nose Tip + Chin Specular Bevel
        let mut pb_nose = PathBuilder::new();
        pb_nose.move_to(0.0, 7.75);
        pb_nose.cubic_to(1.85, 7.65, 2.05, 8.85, 1.25, 10.1);
        pb_nose.cubic_to(0.6, 10.75, -0.6, 10.75, -1.25, 10.1);
        pb_nose.cubic_to(-2.05, 8.85, -1.85, 7.65, 0.0, 7.75);
        pb_nose.close();
        if let Some(path) = pb_nose.finish() {
            p.fill_linear_path_tf(
                &path,
                0.0,
                7.7,
                0.0,
                10.7,
                rgba(12, 12, 18, 255),
                rgba(3, 3, 5, 255),
                None,
                head_tf,
            );
        }
        let mut pb_nose_hi = PathBuilder::new();
        pb_nose_hi.move_to(-1.05, 8.15);
        pb_nose_hi.cubic_to(-0.35, 7.9, 0.35, 7.9, 1.05, 8.15);
        if let Some(path) = pb_nose_hi.finish() {
            p.stroke_path_tf(&path, 0.55, rgba(226, 232, 240, 170), head_tf);
        }
        let mut pb_chin_rim = PathBuilder::new();
        pb_chin_rim.move_to(-2.3, 12.15);
        pb_chin_rim.cubic_to(-0.9, 12.75, 0.9, 12.75, 2.3, 12.15);
        if let Some(path) = pb_chin_rim.finish() {
            p.stroke_path_tf(&path, 0.55, rgba(167, 139, 250, 95), head_tf);
        }

        // 6. Electric-Violet Glowing Slanted Anubis Eyes
        let eye_bloom_a = (128.0 * eye_boost).clamp(40.0, 230.0) as u8;
        let eye_mid_col = if mood == MascotMood::Purging {
            rgba(216, 180, 254, 255)
        } else {
            rgba(168, 85, 247, 255)
        };
        let eye_core_col = if mood == MascotMood::Purging {
            rgba(255, 255, 255, 255)
        } else {
            rgba(245, 208, 254, 255)
        };

        for sign in [-1.0f32, 1.0f32] {
            let eye_tf = if (eye_open - 1.0).abs() > 0.01 {
                head_tf.pre_concat(
                    Transform::from_translate(0.0, 0.65)
                        .pre_scale(1.0, eye_open)
                        .pre_translate(0.0, -0.65),
                )
            } else {
                head_tf
            };

            let mut pb_eye = PathBuilder::new();
            pb_eye.move_to(sign * 1.45, 1.25);
            pb_eye.cubic_to(sign * 2.55, -0.15, sign * 4.55, -0.72, sign * 6.45, -0.72);
            pb_eye.cubic_to(sign * 5.45, 1.22, sign * 3.35, 1.92, sign * 1.45, 1.25);
            pb_eye.close();
            if let Some(path) = pb_eye.finish() {
                let bloom_tf = eye_tf.pre_concat(
                    Transform::from_translate(sign * 3.85, 0.62)
                        .pre_scale(1.38, 1.55)
                        .pre_translate(-sign * 3.85, -0.62),
                );
                p.fill_path_tf_blend(&path, rgba(168, 85, 247, eye_bloom_a), bloom_tf, BlendMode::Plus);
                p.fill_path_tf(&path, eye_mid_col, eye_tf);
            }

            // Razor-hot Neon Inner Iris Core
            let mut pb_core = PathBuilder::new();
            pb_core.move_to(sign * 2.15, 1.05);
            pb_core.cubic_to(sign * 2.95, 0.25, sign * 4.25, -0.05, sign * 5.45, -0.05);
            pb_core.cubic_to(sign * 4.75, 0.95, sign * 3.35, 1.35, sign * 2.15, 1.05);
            pb_core.close();
            if let Some(path) = pb_core.finish() {
                p.fill_path_tf(&path, eye_core_col, eye_tf);
            }
        }

        // 7. 4-Facet Luminous Golden Forehead Diamond Crest
        let gem_cx = 0.0;
        let gem_cy = -5.05;
        let mut gem_pt = Point::from_xy(gem_cx, gem_cy);
        head_tf.map_point(&mut gem_pt);
        let gem_bloom_a = (86.0 * diamond_boost).clamp(40.0, 220.0) as u8;
        p.radial_glow(
            gem_pt.x,
            gem_pt.y,
            (6.2 * diamond_boost.min(1.45)) * k,
            rgba(251, 191, 36, gem_bloom_a),
            BlendMode::Plus,
            false,
        );

        let diamond_facets: [(f32, f32, f32, f32, Color); 4] = [
            (gem_cx, -7.75, -2.3, gem_cy, rgba(254, 249, 195, 255)),
            (2.3, gem_cy, gem_cx, -7.75, rgba(250, 204, 21, 255)),
            (-2.3, gem_cy, gem_cx, -2.35, rgba(245, 158, 11, 255)),
            (gem_cx, -2.35, 2.3, gem_cy, rgba(180, 83, 9, 255)),
        ];
        for (x1, y1, x2, y2, col) in diamond_facets {
            let mut pb_f = PathBuilder::new();
            pb_f.move_to(gem_cx, gem_cy);
            pb_f.line_to(x1, y1);
            pb_f.line_to(x2, y2);
            pb_f.close();
            if let Some(path) = pb_f.finish() {
                p.fill_path_tf(&path, col, head_tf);
            }
        }
        let mut pb_gem_rim = PathBuilder::new();
        pb_gem_rim.move_to(gem_cx, -7.75);
        pb_gem_rim.line_to(2.3, gem_cy);
        pb_gem_rim.line_to(gem_cx, -2.35);
        pb_gem_rim.line_to(-2.3, gem_cy);
        pb_gem_rim.close();
        if let Some(path) = pb_gem_rim.finish() {
            p.stroke_path_tf(&path, 0.45, rgba(254, 240, 138, 210), head_tf);
        }

        // 8. Foreground Accent Sparks / Notes / Droplets / Purge Cross-Star
        match mood {
            MascotMood::Purging => {
                // Radiant 4-point energy star over forehead diamond
                let star_r = (3.4 + 1.2 * ((t * 14.0).sin() * 0.5 + 0.5)) * k;
                let (gx, gy) = (gem_pt.x, gem_pt.y);
                let mut pb_st = PathBuilder::new();
                pb_st.move_to(gx - star_r, gy);
                pb_st.line_to(gx + star_r, gy);
                pb_st.move_to(gx, gy - star_r);
                pb_st.line_to(gx, gy + star_r);
                if let Some(path) = pb_st.finish() {
                    p.stroke_path(&path, 1.15 * k, rgba(255, 251, 235, 225));
                }
            }
            MascotMood::Music if !compact_fx => {
                // Floating glowing music notes (higher richness on large Music hero)
                let note_specs: [(f32, f32, f32, Color); 2] = [
                    (-13.8, -14.0, 0.0, rgba(232, 121, 249, 235)),
                    (14.5, -15.5, 1.7, rgba(251, 191, 36, 235)),
                ];
                let scale_factor = if k >= 1.25 { 1.0 } else { 0.85 };
                for (nx_base, ny_base, ph, col) in note_specs {
                    let lift = (t * 2.4 + ph).sin() * 1.4;
                    let drift = (t * 1.6 + ph).cos() * 0.8;
                    let nx = head_cx + nx_base * scale_factor * k + drift * k;
                    let ny = head_cy + (ny_base * scale_factor + lift) * k;
                    let nr = (1.35 * k).max(1.0);
                    p.circle(nx, ny, nr, col);
                    let mut pb_stem = PathBuilder::new();
                    pb_stem.move_to(nx + nr * 0.65, ny);
                    pb_stem.line_to(nx + nr * 0.65, ny - 4.2 * k);
                    pb_stem.line_to(nx + 2.6 * k, ny - 3.3 * k);
                    if let Some(path) = pb_stem.finish() {
                        p.stroke_path(&path, (0.9 * k).max(0.75), col);
                    }
                }
            }
            MascotMood::Hydrate if !compact_fx => {
                for (dx_base, ph) in [(-11.8f32, 0.0f32), (12.2f32, 1.9f32)] {
                    let u = (t * 0.9 + ph) % 1.0;
                    let alpha = ((1.0 - u) * 210.0) as u8;
                    let drop_x = head_cx + dx_base * k;
                    let drop_y = head_cy + (2.0 - u * 15.0) * k;
                    p.circle(drop_x, drop_y, (1.25 * k).max(0.9), rgba(125, 211, 252, alpha));
                }
            }
            MascotMood::Capture if !compact_fx => {
                // Crisp lens star glint top-right
                let gx = head_cx + 12.2 * k;
                let gy = head_cy - 13.0 * k;
                let r = (2.8 * (0.7 + 0.3 * ((t * 7.0).sin() * 0.5 + 0.5))) * k;
                let mut pb_gl = PathBuilder::new();
                pb_gl.move_to(gx - r, gy);
                pb_gl.line_to(gx + r, gy);
                pb_gl.move_to(gx, gy - r);
                pb_gl.line_to(gx, gy + r);
                if let Some(path) = pb_gl.finish() {
                    p.stroke_path(&path, (1.0 * k).max(0.75), rgba(243, 232, 255, 235));
                }
            }
            _ => {}
        }
    }
}
