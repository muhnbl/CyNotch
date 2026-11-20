use super::*;
use std::fmt::Write as _;

impl Renderer {
    pub(super) fn draw_minimized_idle_vertical(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let cx = w * 0.5;
        let anim_t = state.anim_time;
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
                NotificationKind::MemoryPurge => Icon::Zap,
                NotificationKind::Capture => Icon::Video,
                NotificationKind::Pasted | NotificationKind::General => Icon::Check,
            };
            icons::draw(p, ic, cx - 5.0, h * 0.5 - 5.0, 10.0, arc_col);
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
                p.arc(
                    cx,
                    badge_cy,
                    3.8,
                    -std::f32::consts::FRAC_PI_2,
                    std::f32::consts::TAU * frac,
                    1.4,
                    arc_col,
                );
            }
        }
        p.circle(cx, badge_cy, 2.0, arc_col);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // IDLE CAPSULE (Horizontal 280..390 × 42)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_idle_vertical(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let cx = w * 0.5;
        let anim_t = state.anim_time;
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
                NotificationKind::MemoryPurge => ("PURGE", Icon::Zap, tok::cyno_mist()),
                NotificationKind::Capture => ("SHOT", Icon::Video, tok::cyno_violet_hi()),
                NotificationKind::Pasted => ("PASTE", Icon::Check, tok::cyno_violet_hi()),
                NotificationKind::General => ("ALERT", Icon::Check, tok::cyno_violet_hi()),
            };
            p.text_center(tag, cx, mid_cy - 3.0, 9.0, Weight::Bold, ic_col);
            let sub = p.ellipsize_cow(&notif.title, 8.6, Weight::Semibold, w - 8.0);
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
        } else if state.media.is_playing {
            if state.equalizer_enabled {
                for i in 0..4 {
                    let wave = (state.anim_time * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5;
                    let bh = (6.0 + wave * 14.0).clamp(4.0, 20.0);
                    let bx = cx - 9.0 + (i as f32 * 5.0);
                    let by = mid_cy - 6.0 - bh * 0.5;
                    let c = if i % 2 == 0 {
                        tok::cyno_violet_hi()
                    } else {
                        tok::cyno_mist()
                    };
                    p.rrect(bx, by, 2.8, bh, 1.4, c);
                }
                let lbl = p.ellipsize_cow(&state.media.title, 8.6, Weight::Semibold, w - 8.0);
                p.text_center(&lbl, cx, mid_cy + 20.0, 8.6, Weight::Semibold, tok::text1());
            } else {
                p.text_center(
                    "PLAY",
                    cx,
                    mid_cy - 4.0,
                    8.5,
                    Weight::Bold,
                    tok::cyno_violet_hi(),
                );
                let lbl = p.ellipsize_cow(&state.media.title, 8.8, Weight::Semibold, w - 8.0);
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
                    p.rrect(
                        cx - 14.0,
                        mid_cy + 11.0,
                        (28.0 * pct).max(3.0),
                        4.0,
                        2.0,
                        tok::cyno_mist(),
                    );
                    (Icon::Zap, tok::cyno_mist())
                }
                IdleMetric::NetSpeed => {
                    p.text_center("NET", cx, mid_cy - 14.0, 8.2, Weight::Bold, tok::text2());
                    let mut dn_buf = StackStr::<32>::new();
                    let _ = write!(dn_buf, "↓{}", format_speed_short(state.telemetry.net_down_kbps));
                    let dn = p.ellipsize_cow(&dn_buf, 9.0, Weight::Bold, w - 6.0);
                    let mut up_buf = StackStr::<32>::new();
                    let _ = write!(up_buf, "↑{}", format_speed_short(state.telemetry.net_up_kbps));
                    let up = p.ellipsize_cow(&up_buf, 8.6, Weight::Semibold, w - 6.0);
                    p.text_center(&dn, cx, mid_cy + 1.0, 9.0, Weight::Bold, tok::cyan_hi());
                    p.text_center(
                        &up,
                        cx,
                        mid_cy + 15.0,
                        8.6,
                        Weight::Semibold,
                        tok::cyno_mist(),
                    );
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
                    p.rrect(
                        cx - 14.0,
                        mid_cy + 11.0,
                        (28.0 * pct).max(3.0),
                        4.0,
                        2.0,
                        tok::cyno_violet_hi(),
                    );
                    (Icon::Activity, tok::cyno_violet_hi())
                }
                IdleMetric::Clock => {
                                    let now = local_now();
                                    p.text_center(
                                        &format!("{:02}", now.hour),
                                        cx,
                                        mid_cy - 2.0,
                                        13.5,
                                        Weight::Bold,
                                        tok::text1(),
                                    );
                                    p.text_center(
                                        &format!("{:02}", now.minute),
                                        cx,
                                        mid_cy + 14.0,
                                        13.5,
                                        Weight::Bold,
                                        tok::cyno_mist(),
                                    );
                                    (Icon::Timer, tok::cyno_mist())
                                }
                IdleMetric::Minimal => {
                    p.rrect(
                        cx - 2.0,
                        mid_cy - 14.0,
                        4.0,
                        28.0,
                        2.0,
                        rgba(192, 132, 252, 95),
                    );
                    (Icon::Zap, tok::cyno_mist())
                }
            };
            Self::draw_progress_arc_badge(
                p,
                cx,
                badge_cy,
                11.5,
                if state.idle_metric == IdleMetric::Minimal {
                    None
                } else {
                    frac_opt
                },
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
    pub(super) fn draw_expanded_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        h: f32,
        _settled: bool,
    ) {
        Self::draw_expanded_vertical_header(p, state, w);
        let prev_ox = p.origin_x;
        let prev_alpha = p.global_alpha;
        if state.are_animations_active() && state.anim_mode_t < 0.995 {
            let me = crate::models::ease_out_cubic(state.anim_mode_t);
            p.origin_x = prev_ox + (1.0 - me) * 10.0 * state.anim_mode_dir * p.s;
            p.global_alpha = prev_alpha * (0.18 + 0.82 * me);
        }
        Self::draw_expanded_vertical_body(p, state, w, h);
        p.origin_x = prev_ox;
        p.global_alpha = prev_alpha;
    }

    fn draw_expanded_vertical_header(p: &mut PainterD2D, state: &AppState, w: f32) {
        if state.notification.is_none() {
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
                icons::draw(
                    p,
                    Icon::Settings,
                    gear_x + 7.0,
                    btn_y + 7.0,
                    14.0,
                    rgba(255, 255, 255, 255),
                );
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
                icons::draw(
                    p,
                    Icon::Lock,
                    pin_x + 7.0,
                    btn_y + 7.0,
                    14.0,
                    rgba(255, 255, 255, 255),
                );
            } else {
                p.tactile_surface_pill_t(pin_x, btn_y, 28.0, 28.0, 14.0, pin_ht, None);
                let pc = lerp_color(tok::text2(), tok::text1(), pin_ht);
                icons::draw(p, Icon::Lock, pin_x + 7.0, btn_y + 7.0, 14.0, pc);
            }
            p.hit(pin_x, btn_y, 28.0, 28.0, Action::TogglePin);

            let bell_x = pin_x - 34.0;
            let bell_ht = p.hot_t(bell_x, btn_y, 28.0, 28.0);
            if state.show_notif_history {
                p.luminous_pill_button_t(
                    bell_x,
                    btn_y,
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
                    btn_y + 7.0,
                    14.0,
                    rgba(255, 255, 255, 255),
                );
            } else {
                p.tactile_surface_pill_t(bell_x, btn_y, 28.0, 28.0, 14.0, bell_ht, None);
                let bc = lerp_color(tok::text2(), tok::text1(), bell_ht);
                icons::draw(p, Icon::Bell, bell_x + 7.0, btn_y + 7.0, 14.0, bc);
            }
            if !state.notif_history.is_empty() && !state.show_notif_history {
                p.circle(bell_x + 21.0, btn_y + 6.0, 2.7, tok::cyno_orchid());
            }
            p.hit(bell_x, btn_y, 28.0, 28.0, Action::ToggleNotifHistory);

            let title_x = 18.0;
            let title_y = 30.5;
            let title_limit = if !state.show_notif_history && state.mode == NotchMode::Music {
                let lyrics_w = 54.0;
                let lyrics_x = bell_x - lyrics_w - 8.0;
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
                    p.text_center_shadow(
                        "Lyrics",
                        lyrics_x + lyrics_w / 2.0,
                        lyrics_y + 17.0,
                        10.5,
                        Weight::Bold,
                        tok::text1(),
                    );
                } else {
                    p.tactile_surface_pill_t(
                        lyrics_x, lyrics_y, lyrics_w, 26.0, 13.0, lyrics_ht, None,
                    );
                    p.text_center(
                        "Lyrics",
                        lyrics_x + lyrics_w / 2.0,
                        lyrics_y + 17.0,
                        10.5,
                        Weight::Semibold,
                        tok::text_soft(),
                    );
                }
                p.hit(lyrics_x, lyrics_y, lyrics_w, 26.0, Action::ToggleLyrics);
                lyrics_x - 8.0
            } else {
                bell_x - 8.0
            };

            let mode_title = if state.show_notif_history {
                "Notifications"
            } else {
                match state.mode {
                    NotchMode::Home => "Command Hub",
                    NotchMode::Music => "Music",
                    NotchMode::Volume => "Volume Master",
                    NotchMode::MemReduct => "Mem Reduct",
                    NotchMode::Tasks => "Tasks",
                    NotchMode::Shelf => "Quick Shelf",
                    NotchMode::Capture => "Quick Capture",
                    NotchMode::Telemetry => "Screen Time",
                    NotchMode::Settings => "Quick Settings",
                }
            };
            let max_tw = (title_limit - title_x).max(24.0);
            let title_ell = p.ellipsize_cow(mode_title, 13.5, Weight::Bold, max_tw);
            p.text(
                &title_ell,
                title_x,
                title_y,
                13.5,
                Weight::Bold,
                tok::text1(),
            );
        }

        // Row 2: Centered Dynamic Module Dock (stays accessible even when toast overlay occupies Row 1)
        let dock_modes = state.visible_dock_modes();
        let (dock_w, item_w) =
            AppState::dock_metrics_for_placement(dock_modes.len(), state.placement);
        let dock_x = (w - dock_w) * 0.5;
        let dock_y = if state.notification.is_some() {
            54.0
        } else {
            46.0
        };
        let dock_h = 30.0;

        let well_col = if state.is_frosted_active() {
            tok::scale_frosted(tok::surface_well_frosted(), state.frosted_alpha_pct())
        } else {
            tok::surface_well()
        };
        p.rrect(dock_x, dock_y, dock_w, dock_h, 11.0, well_col);
        p.rrect_bevel_stroke(
            dock_x,
            dock_y,
            dock_w,
            dock_h,
            11.0,
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
                state.anim_dock_w.max(18.0),
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
    }

    fn draw_expanded_vertical_body(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let card_y = 88.0;
        if state.show_notif_history {
            Self::draw_notif_history_view(p, state, w, h, card_y);
        } else {
            match state.mode {
                NotchMode::Home => Self::draw_home_view_vertical(p, state, w, card_y),
                NotchMode::Music => Self::draw_music_view_vertical(p, state, w, card_y),
                NotchMode::Volume => Self::draw_volume_view_vertical(p, state, w, card_y),
                NotchMode::MemReduct => Self::draw_memreduct_view_vertical(p, state, w, card_y),
                NotchMode::Tasks => Self::draw_tasks_view_vertical(p, state, w, card_y),
                NotchMode::Shelf => Self::draw_shelf_view_vertical(p, state, w, card_y),
                NotchMode::Capture => Self::draw_capture_view_vertical(p, state, w, card_y),
                NotchMode::Telemetry => Self::draw_telemetry_view_vertical(p, state, w, card_y),
                NotchMode::Settings => Self::draw_settings_view_vertical(p, state, w, card_y),
            }
        }
        Self::draw_expanded_toast_overlay(p, state, w);
    }

    pub(super) fn draw_home_view_vertical(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: General Cyno Companion + RAM Bar + Purge CTA
        let c1_h = 134.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            16.0,
            Some((42.0, 34.0, 74.0, tok::cyno_violet())),
        );
        if state.show_mascot {
            let anim_t = state.anim_time;
            Self::draw_cyno_mascot(
                p,
                mx + 30.0,
                card_y + 31.0,
                0.96,
                state.mascot_mood(),
                anim_t,
                false,
            );
        } else {
            let emb_cx = mx + 30.0;
            let emb_cy = card_y + 31.0;
            p.circle(emb_cx, emb_cy, 15.5, rgba(24, 22, 42, 225));
            p.circle_stroke(emb_cx, emb_cy, 15.5, 1.0, rgba(192, 132, 252, 95));
            icons::draw(
                p,
                Icon::Zap,
                emb_cx - 7.5,
                emb_cy - 7.5,
                15.0,
                tok::cyno_mist(),
            );
        }

        p.text(
            "General Cyno",
            mx + 58.0,
            card_y + 27.0,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        let pwr_str = match (state.telemetry.battery_pct, state.telemetry.on_ac) {
            (Some(pct), true) => format!("CPU {:.0}% • {}% AC", state.telemetry.cpu_usage, pct),
            (Some(pct), false) => format!("CPU {:.0}% • {}% Bat", state.telemetry.cpu_usage, pct),
            (None, _) => format!("CPU {:.0}% • AC Power", state.telemetry.cpu_usage),
        };
        let pwr_ell = p.ellipsize_cow(&pwr_str, 10.0, Weight::Regular, cw - 74.0);
        p.text(
            &pwr_ell,
            mx + 58.0,
            card_y + 42.5,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let ram_label = format!("RAM {:.0}%", state.telemetry.memory_percent);
        p.text(
            &ram_label,
            mx + 16.0,
            card_y + 67.0,
            10.5,
            Weight::Semibold,
            tok::text_soft(),
        );
        let used_lbl = format!(
            "{:.1} / {:.1} GB",
            state.telemetry.memory_used_gb, state.telemetry.memory_total_gb
        );
        p.text_right(
            &used_lbl,
            mx + cw - 16.0,
            card_y + 67.0,
            10.5,
            Weight::Semibold,
            tok::cyno_mist(),
        );

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(
            mx + 16.0,
            card_y + 75.0,
            cw - 32.0,
            7.0,
            ram_pct,
            tok::cyno_deep(),
            tok::cyno_violet_hi(),
        );

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
        let purge_lbl = if state.mem_reduct.is_purging {
            "Purging RAM..."
        } else {
            "Purge RAM"
        };
        p.text_center_shadow(
            purge_lbl,
            purge_x + purge_w / 2.0,
            purge_y + 19.0 - 0.8 * purge_ht,
            11.5,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(purge_x, purge_y, purge_w, purge_h, Action::PurgeRam);

        // Card 2: Live Network & Telemetry
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 92.0;
        p.elevated_card(
            mx,
            c2_y,
            cw,
            c2_h,
            15.0,
            Some((cw - 34.0, 22.0, 72.0, tok::cyan_hi())),
        );
        let net_card_ht = p.hot_t(mx, c2_y, cw, c2_h);
        if net_card_ht > 0.02 {
            p.rrect_bevel_stroke(
                mx,
                c2_y,
                cw,
                c2_h,
                15.0,
                1.0,
                fade(tok::cyan_hi(), 0.35 * net_card_ht),
                fade(tok::cyno_violet(), 0.14 * net_card_ht),
            );
        }
        icons::draw(
            p,
            Icon::Activity,
            mx + 14.0,
            c2_y + 12.0,
            14.0,
            tok::cyan_hi(),
        );
        p.text(
            "Live Network",
            mx + 33.0,
            c2_y + 23.0,
            11.5,
            Weight::Bold,
            tok::text1(),
        );

        let pill_w = 50.0;
        let pill_h = 18.0;
        let pill_x = mx + cw - pill_w - 14.0;
        let pill_y = c2_y + 11.5;
        p.rrect(pill_x, pill_y, pill_w, pill_h, 9.0, rgba(125, 211, 252, 22));
        p.rrect_bevel_stroke(
            pill_x,
            pill_y,
            pill_w,
            pill_h,
            9.0,
            0.8,
            rgba(125, 211, 252, 72),
            rgba(125, 211, 252, 22),
        );
        let pulse = if state.are_animations_active() {
            0.65 + 0.35 * (state.anim_time * 3.0).sin().abs()
        } else {
            1.0
        };
        p.circle(
            pill_x + 10.5,
            pill_y + pill_h * 0.5,
            2.6,
            fade(tok::cyan_hi(), pulse),
        );
        p.text(
            "LIVE",
            pill_x + 18.0,
            pill_y + 12.2,
            8.2,
            Weight::Bold,
            tok::cyan_hi(),
        );

        let bar_base_x = mx + cw - 58.0;
        let bar_bot_y = c2_y + 72.0;
        let activity = ((state.telemetry.net_down_kbps + state.telemetry.net_up_kbps) / 2000.0)
            .clamp(0.18, 1.0);
        for i in 0..6 {
            let phase = (i as f32) * 0.85
                + if state.are_animations_active() {
                    state.anim_time * 2.4
                } else {
                    0.4
                };
            let wave = 0.35 + 0.65 * (0.5 + 0.5 * phase.sin());
            let bh = (5.0 + 17.0 * activity * wave).clamp(4.0, 22.0);
            let bx = bar_base_x + (i as f32) * 7.5;
            p.rrect(
                bx,
                bar_bot_y - bh,
                4.0,
                bh,
                2.0,
                fade(tok::cyan_hi(), 0.30 + 0.50 * wave),
            );
        }

        let mut dn_buf = StackStr::<48>::new();
        let _ = write!(dn_buf, "↓ {}", format_speed_long(state.telemetry.net_down_kbps));
        let dn = p.ellipsize_cow(&dn_buf, 10.8, Weight::Semibold, cw - 86.0);
        let mut up_buf = StackStr::<48>::new();
        let _ = write!(up_buf, "↑ {}", format_speed_long(state.telemetry.net_up_kbps));
        let up = p.ellipsize_cow(&up_buf, 10.5, Weight::Semibold, cw - 86.0);
        p.text(
            &dn,
            mx + 16.0,
            c2_y + 49.0,
            10.8,
            Weight::Semibold,
            tok::cyan_hi(),
        );
        p.text(
            &up,
            mx + 16.0,
            c2_y + 70.0,
            10.5,
            Weight::Semibold,
            tok::cyno_mist(),
        );
        p.hit(mx, c2_y, cw, c2_h, Action::SetMode(NotchMode::Telemetry));

        // Card 3 Row: Side-by-Side Media & Tasks/Capture (y = 338..464, h = 126)
        let c3_y = c2_y + c2_h + 12.0;
        let c3_h = 126.0;
        let sub_gap = 12.0;
        let sub_w = (cw - sub_gap) / 2.0;

        // Left: Media Overview
        p.elevated_card(mx, c3_y, sub_w, c3_h, 15.0, None);
        let media_ht = p.hot_t(mx, c3_y, sub_w, c3_h - 46.0);
        if media_ht > 0.02 {
            p.rrect_bevel_stroke(
                mx,
                c3_y,
                sub_w,
                c3_h,
                15.0,
                1.0,
                fade(tok::cyno_violet_hi(), 0.42 * media_ht),
                fade(tok::cyno_violet(), 0.16 * media_ht),
            );
        }
        let media_header = if state.media.is_idle() {
            "No Active Media"
        } else if state.media.is_playing {
            "Now Playing"
        } else {
            "Media Paused"
        };
        p.text(
            media_header,
            mx + 14.0,
            c3_y + 22.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        let song = p.ellipsize_cow(&state.media.title, 12.0, Weight::Bold, sub_w - 28.0);
        p.text(
            &song,
            mx + 14.0,
            c3_y + 44.0,
            12.0,
            Weight::Bold,
            tok::text1(),
        );
        let art_ell = p.ellipsize_cow(&state.media.artist, 10.0, Weight::Regular, sub_w - 28.0);
        p.text(
            &art_ell,
            mx + 14.0,
            c3_y + 62.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            mx,
            c3_y,
            sub_w,
            c3_h - 46.0,
            Action::SetMode(NotchMode::Music),
        );

        let mp_x = mx + 14.0;
        let mp_y = c3_y + 84.0;
        let mp_w = sub_w - 28.0;
        let mp_h = 28.0;
        let mp_ht = p.hot_t(mp_x, mp_y, mp_w, mp_h);
        p.tactile_surface_pill_t(mp_x, mp_y, mp_w, mp_h, 9.5, mp_ht, Some(tok::cyno_violet()));
        let mp_lbl = if state.media.is_playing {
            "Pause"
        } else {
            "Play"
        };
        p.text_center(
            mp_lbl,
            mp_x + mp_w / 2.0,
            mp_y + 18.0 - 0.55 * mp_ht,
            10.8,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(mp_x, mp_y, mp_w, mp_h, Action::TogglePlayPause);

        // Right: Tasks & Quick Screenshot
        let r_x = mx + sub_w + sub_gap;
        p.elevated_card(r_x, c3_y, sub_w, c3_h, 15.0, None);
        let tasks_ht = p.hot_t(r_x, c3_y, sub_w, c3_h - 46.0);
        if tasks_ht > 0.02 {
            p.rrect_bevel_stroke(
                r_x,
                c3_y,
                sub_w,
                c3_h,
                15.0,
                1.0,
                fade(tok::cyno_mist(), 0.42 * tasks_ht),
                fade(tok::cyno_orchid(), 0.16 * tasks_ht),
            );
        }
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        p.text(
            "Tasks Open",
            r_x + 14.0,
            c3_y + 22.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        p.text(
            &format!("{} active", pending),
            r_x + 14.0,
            c3_y + 46.0,
            14.0,
            Weight::Bold,
            tok::cyno_mist(),
        );
        let first_task = state
            .tasks
            .iter()
            .find(|t| !t.completed)
            .map(|t| t.text.as_str())
            .unwrap_or("All tasks done");
        let ft_ell = p.ellipsize_cow(first_task, 9.8, Weight::Regular, sub_w - 28.0);
        p.text(
            &ft_ell,
            r_x + 14.0,
            c3_y + 64.0,
            9.8,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            r_x,
            c3_y,
            sub_w,
            c3_h - 46.0,
            Action::SetMode(NotchMode::Tasks),
        );

        let shot_x = r_x + 14.0;
        let shot_y = c3_y + 84.0;
        let shot_w = sub_w - 28.0;
        let shot_h = 28.0;
        let shot_ht = p.hot_t(shot_x, shot_y, shot_w, shot_h);
        p.tactile_surface_pill_t(shot_x, shot_y, shot_w, shot_h, 9.5, shot_ht, None);
        p.text_center(
            "Screenshot",
            shot_x + shot_w / 2.0,
            shot_y + 18.0 - 0.55 * shot_ht,
            10.5,
            Weight::Semibold,
            tok::cyno_violet_hi(),
        );
        p.hit(shot_x, shot_y, shot_w, shot_h, Action::CaptureScreen);
    }

    pub(super) fn draw_music_view_vertical(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
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
            let anim_t = state.anim_time;
            Self::draw_cyno_mascot(
                p,
                hero_cx,
                hero_cy + 1.0,
                1.72,
                MascotMood::Music,
                anim_t,
                false,
            );
        } else {
            p.circle(hero_cx, hero_cy, 42.0, rgba(10, 9, 20, 140));
            icons::draw(
                p,
                Icon::Music,
                hero_cx - 20.0,
                hero_cy - 20.0,
                40.0,
                rgba(255, 255, 255, 255),
            );
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
        let badge_txt = p.ellipsize_cow(art_badge, 9.8, Weight::Bold, badge_w - 16.0);
        p.rrect(
            badge_x,
            card_y + 24.0,
            badge_w,
            24.0,
            12.0,
            rgba(10, 8, 22, 150),
        );
        p.text_center(
            &badge_txt,
            badge_x + badge_w / 2.0,
            card_y + 39.5,
            9.8,
            Weight::Bold,
            tok::text1(),
        );

        if state.equalizer_enabled {
            let bar_heights = [
                8.0, 14.0, 20.0, 15.0, 24.0, 26.0, 18.0, 12.0, 22.0, 26.0, 16.0, 10.0,
            ];
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
                let bar_col = if i % 4 == 0 {
                    tok::cyno_mist()
                } else {
                    rgba(255, 255, 255, 230)
                };
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
        p.text_center(
            state_lbl,
            badge_x + badge_w / 2.0,
            card_y + 118.0,
            10.2,
            Weight::Semibold,
            rgba(248, 250, 252, 235),
        );

        // Bottom Card: Controls OR Synced Lyrics (y = 246..464, h = 218)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 218.0;
        p.elevated_card(
            mx,
            c2_y,
            cw,
            c2_h,
            16.0,
            Some((cw - 26.0, 24.0, 82.0, tok::cyno_violet())),
        );

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
            p.glowing_striped_bar(
                pad_x,
                scrub_y,
                inner_w,
                6.5,
                played_pct,
                tok::cyno_deep(),
                tok::cyno_violet(),
            );
            let scrub_hot = p.hot(pad_x - 4.0, scrub_y - 6.0, inner_w + 8.0, 20.0);
            let thumb_x = (pad_x + inner_w * played_pct).clamp(pad_x + 5.0, pad_x + inner_w - 3.0);
            let thumb_y = scrub_y + 3.25;
            let thumb_r = if scrub_hot { 6.0 } else { 4.8 };
            p.radial_glow(
                thumb_x,
                thumb_y,
                if scrub_hot { 11.5 } else { 8.5 },
                rgba(233, 213, 255, if scrub_hot { 130 } else { 85 }),
                BlendMode::Plus,
                false,
            );
            p.circle(thumb_x, thumb_y, thumb_r, rgba(255, 255, 255, 250));
            p.circle(thumb_x, thumb_y, thumb_r * 0.44, tok::cyno_deep());
            p.hit(
                pad_x,
                scrub_y - 6.0,
                inner_w,
                20.0,
                Action::SeekMusic(played_pct),
            );
            Self::draw_transport_buttons(
                p,
                state.media.is_playing,
                pad_x + inner_w / 2.0,
                c2_y + 172.0,
            );
        } else {
            let title_text = p.ellipsize_cow(&state.media.title, 15.0, Weight::Bold, inner_w);
            p.text(
                &title_text,
                pad_x,
                c2_y + 30.0,
                15.0,
                Weight::Bold,
                tok::text1(),
            );
            let artist_txt = p.ellipsize_cow(&state.media.artist, 11.8, Weight::Regular, inner_w);
            p.text(
                &artist_txt,
                pad_x,
                c2_y + 49.0,
                11.8,
                Weight::Regular,
                tok::text2(),
            );

            let prog_y = c2_y + 72.0;
            p.glowing_striped_bar(
                pad_x,
                prog_y,
                inner_w,
                7.0,
                played_pct,
                tok::cyno_deep(),
                tok::cyno_violet(),
            );
            let scrub_hot = p.hot(pad_x - 4.0, prog_y - 6.0, inner_w + 8.0, 22.0);
            let thumb_x = (pad_x + inner_w * played_pct).clamp(pad_x + 5.0, pad_x + inner_w - 3.0);
            let thumb_y = prog_y + 3.5;
            let thumb_r = if scrub_hot { 6.2 } else { 5.0 };
            p.radial_glow(
                thumb_x,
                thumb_y,
                if scrub_hot { 12.0 } else { 9.0 },
                rgba(233, 213, 255, if scrub_hot { 135 } else { 90 }),
                BlendMode::Plus,
                false,
            );
            p.circle(thumb_x, thumb_y, thumb_r, rgba(255, 255, 255, 252));
            p.circle(thumb_x, thumb_y, thumb_r * 0.44, tok::cyno_deep());
            p.hit(
                pad_x,
                prog_y - 6.0,
                inner_w,
                22.0,
                Action::SeekMusic(played_pct),
            );

            let pos_sec = state.media.position_ms / 1000;
            let dur_sec = state.media.duration_ms / 1000;
            let rem_sec = dur_sec.saturating_sub(pos_sec);
            p.text(
                &format!("{}:{:02}", pos_sec / 60, pos_sec % 60),
                pad_x,
                prog_y + 22.0,
                10.2,
                Weight::Semibold,
                tok::text2(),
            );
            let right_t = if state.media.duration_ms > 0 {
                format!("-{}:{:02}", rem_sec / 60, rem_sec % 60)
            } else {
                "--:--".to_string()
            };
            p.text_right(
                &right_t,
                pad_x + inner_w,
                prog_y + 22.0,
                10.2,
                Weight::Semibold,
                tok::text2(),
            );

            let (snip_lines, snip_sz) = p.wrap_lines_fit(
                &state.current_lyric_text,
                12.2,
                9.8,
                Weight::Regular,
                inner_w,
                2,
            );
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

            Self::draw_transport_buttons(
                p,
                state.media.is_playing,
                pad_x + inner_w / 2.0,
                c2_y + 162.0,
            );
        }
    }

    pub(super) fn draw_volume_view_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        card_y: f32,
    ) {
        let smooth = state.micro_interactions;
        let mx = 16.0;
        let cw = w - 32.0;
        let vol_int = (state.volume_pct * 100.0).round() as i32;
        let is_muted = vol_int == 0;
        let aurora_col = if is_muted {
            rgba(239, 68, 68, 120)
        } else {
            tok::cyno_violet()
        };

        // Top Card: Large Volume Readout + Mute Toggle (y = 88..234, h = 146)
        let c1_h = 146.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            16.0,
            Some((64.0, 44.0, 68.0, aurora_col)),
        );

        let ic_cx = mx + 44.0;
        let ic_cy = card_y + 52.0;
        p.tactile_surface_pill(
            ic_cx - 19.0,
            ic_cy - 19.0,
            38.0,
            38.0,
            19.0,
            false,
            Some(aurora_col),
        );
        let vol_icon_col = if is_muted {
            rgba(252, 165, 165, 255)
        } else {
            tok::text1()
        };
        icons::draw(
            p,
            Icon::Volume,
            ic_cx - 9.0,
            ic_cy - 9.0,
            18.0,
            vol_icon_col,
        );

        p.text(
            &format!("{}%", vol_int),
            mx + 76.0,
            card_y + 58.0,
            26.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Master Output",
            mx + 76.0,
            card_y + 76.0,
            10.5,
            Weight::Regular,
            tok::text2(),
        );

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
        let mute_label = if is_muted {
            "Unmute Audio"
        } else {
            "Mute Audio"
        };
        let mute_ty = mute_y + 21.5
            - if is_muted {
                0.8 * mute_ht
            } else {
                0.55 * mute_ht
            };
        p.text_center_shadow(
            mute_label,
            mute_x + mute_w / 2.0,
            mute_ty,
            11.5,
            Weight::Semibold,
            tok::text1(),
        );
        p.hit(mute_x, mute_y, mute_w, mute_h, Action::ToggleMute);

        // Bottom Card: Slider & Presets (y = 246..464, h = 218)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 218.0;
        p.elevated_card(
            mx,
            c2_y,
            cw,
            c2_h,
            16.0,
            Some((cw - 30.0, 26.0, 86.0, tok::cyno_orchid())),
        );
        p.dot_matrix(
            mx + cw - 48.0,
            c2_y + 14.0,
            5,
            3,
            5.5,
            rgba(255, 255, 255, 32),
        );

        p.text(
            "System Volume Master",
            mx + 16.0,
            c2_y + 28.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Windows Core Audio • Drag slider or scroll",
            mx + 16.0,
            c2_y + 46.0,
            10.2,
            Weight::Regular,
            tok::text2(),
        );

        let slider_x = AppState::VERT_VOLUME_SLIDER_X;
        let slider_w = AppState::VERT_VOLUME_SLIDER_W;
        let slider_y = c2_y + 78.0; // y = 324 (< 365 drag cutoff)
        let slider_h = 14.0;
        let pct_clamped = if smooth {
            state.anim_vol_pct.clamp(0.0, 1.0)
        } else {
            state.volume_pct.clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(
            slider_x,
            slider_y,
            slider_w,
            slider_h,
            pct_clamped,
            tok::cyno_deep(),
            tok::cyno_violet(),
        );

        let thumb_x = (slider_x + slider_w * pct_clamped)
            .clamp(slider_x + 7.0, slider_x + slider_w - 4.0);
        let thumb_y = slider_y + slider_h / 2.0;
        p.radial_glow(
            thumb_x,
            thumb_y,
            14.0,
            if is_muted {
                rgba(239, 68, 68, 80)
            } else {
                rgba(255, 255, 255, 110)
            },
            BlendMode::Plus,
            false,
        );
        p.circle(
            thumb_x,
            thumb_y,
            8.5,
            if is_muted {
                rgba(226, 232, 240, 235)
            } else {
                rgba(255, 255, 255, 255)
            },
        );
        p.circle(thumb_x, thumb_y, 4.0, tok::cyno_deep());

        p.hit(
            slider_x,
            slider_y - 6.0,
            slider_w,
            26.0,
            Action::SetVolume(pct_clamped),
        );

        p.text(
            "Quick Presets",
            mx + 16.0,
            c2_y + 122.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        let presets: [(f32, &str); 5] = [
            (0.0, "0%"),
            (0.25, "25%"),
            (0.50, "50%"),
            (0.75, "75%"),
            (1.0, "100%"),
        ];
        let pb_gap = 6.0;
        let pb_w = (slider_w - (pb_gap * 4.0)) / 5.0;
        let pb_y = c2_y + 136.0; // y = 382 (>= 365 drag cutoff)
        let pb_h = 34.0;

        for (i, &(target_pct, label)) in presets.iter().enumerate() {
            let px = slider_x + (i as f32 * (pb_w + pb_gap));
            let is_cur = (state.volume_pct - target_pct).abs() <= 0.015;
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
                p.text_center_shadow(
                    label,
                    px + pb_w / 2.0,
                    pb_y + 21.0 - 0.8 * ht,
                    10.5,
                    Weight::Bold,
                    tok::text1(),
                );
            } else {
                p.tactile_surface_pill_t(px, pb_y, pb_w, pb_h, 9.5, ht, None);
                p.text_center(
                    label,
                    px + pb_w / 2.0,
                    pb_y + 21.0 - 0.55 * ht,
                    10.5,
                    Weight::Semibold,
                    lerp_color(tok::text_soft(), tok::text1(), ht),
                );
            }
            p.hit(px, pb_y, pb_w, pb_h, Action::SetVolume(target_pct));
        }
    }

    pub(super) fn draw_memreduct_view_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        card_y: f32,
    ) {
        let smooth = state.micro_interactions;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: Physical RAM + Pools (y = 88..182, h = 94)
        let c1_h = 94.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            15.0,
            Some((cw - 20.0, 16.0, 62.0, tok::cyno_violet())),
        );
        p.text(
            "Physical Memory",
            mx + 14.0,
            card_y + 20.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        let right_stat = if state.mem_reduct.is_purging {
            "Cleaning...".to_string()
        } else if state.mem_reduct.last_freed_mb > 0.0 {
            format!("Last: {:.0} MB", state.mem_reduct.last_freed_mb)
        } else if state.telemetry.has_admin_privileges {
            "Admin Mode".to_string()
        } else {
            "User Mode".to_string()
        };
        p.text_right(
            &right_stat,
            mx + cw - 14.0,
            card_y + 20.0,
            9.8,
            Weight::Bold,
            tok::cyno_mist(),
        );

        let ram_val = format!(
            "{:.1} / {:.1} GB ({:.0}%)",
            state.telemetry.memory_used_gb,
            state.telemetry.memory_total_gb,
            state.telemetry.memory_percent
        );
        p.text(
            &ram_val,
            mx + 14.0,
            card_y + 42.0,
            14.5,
            Weight::Bold,
            tok::text1(),
        );

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(
            mx + 14.0,
            card_y + 50.0,
            cw - 28.0,
            6.5,
            ram_pct,
            tok::cyno_deep(),
            tok::cyno_violet_hi(),
        );

        let pools_str = format!(
            "Page {:.0}% • SysCache {:.0} MB",
            state.telemetry.pagefile_percent, state.telemetry.sys_cache_mb
        );
        let pools_ell = p.ellipsize_cow(&pools_str, 9.5, Weight::Regular, cw - 28.0);
        p.text(
            &pools_ell,
            mx + 14.0,
            card_y + 73.0,
            9.5,
            Weight::Regular,
            tok::text2(),
        );

        // Card 2: 8 Clean Areas + Auto Clean Toggle (y = 192..304, h = 112)
        let c2_y = card_y + c1_h + 10.0;
        let c2_h = 112.0;
        p.elevated_card(mx, c2_y, cw, c2_h, 15.0, None);

        let auto_w = 76.0;
        let auto_x = mx + cw - auto_w - 14.0;
        let auto_y = c2_y + 9.0;

        let a_gap_x = 6.0;
        let a_gap_y = 6.0;
        let pill_w = (cw - 28.0 - a_gap_x * 3.0) / 4.0;
        let pill_h = 26.0;

        let hovered_area = (0..8).find(|&idx| {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let px = mx + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = c2_y + 38.0 + (row_i as f32 * (pill_h + a_gap_y));
            p.hot(px, py, pill_w, pill_h)
        });
        let header_max_w = (auto_x - (mx + 14.0) - 8.0).max(60.0);
        if let Some(desc) = hovered_area.and_then(crate::memreduct::clean_area_description) {
            let ell = p.ellipsize_cow(desc, 9.5, Weight::Semibold, header_max_w);
            p.text(
                &ell,
                mx + 14.0,
                c2_y + 21.0,
                9.5,
                Weight::Semibold,
                tok::cyno_violet_hi(),
            );
        } else {
            p.text(
                "Clean Areas",
                mx + 14.0,
                c2_y + 21.0,
                10.8,
                Weight::Semibold,
                tok::text2(),
            );
        }

        let auto_ht = p.hot_t(auto_x, auto_y, auto_w, 20.0);
        p.tactile_surface_pill_t(
            auto_x,
            auto_y,
            auto_w,
            20.0,
            10.0,
            auto_ht,
            if state.mem_reduct.auto_clean_enabled {
                Some(tok::cyno_violet())
            } else {
                None
            },
        );
        let auto_lbl = if state.mem_reduct.auto_clean_enabled {
            "Auto: ON"
        } else {
            "Auto: OFF"
        };
        p.text_center(
            auto_lbl,
            auto_x + auto_w / 2.0,
            auto_y + 13.5 - 0.45 * auto_ht,
            9.2,
            Weight::Bold,
            if state.mem_reduct.auto_clean_enabled {
                tok::text1()
            } else {
                lerp_color(tok::text2(), tok::text1(), auto_ht)
            },
        );
        p.hit(auto_x, auto_y, auto_w, 20.0, Action::ToggleAutoClean);

        let areas = [
            (
                "WS",
                state.mem_reduct.clean_working_sets,
                Action::ToggleArea(0),
            ),
            (
                "SysWS",
                state.mem_reduct.clean_system_ws,
                Action::ToggleArea(1),
            ),
            (
                "Standby",
                state.mem_reduct.clean_standby_list,
                Action::ToggleArea(2),
            ),
            (
                "ModPg",
                state.mem_reduct.clean_modified_page_list,
                Action::ToggleArea(3),
            ),
            (
                "StbyLo",
                state.mem_reduct.clean_standby_low,
                Action::ToggleArea(4),
            ),
            (
                "ModFile",
                state.mem_reduct.clean_modified_file_cache,
                Action::ToggleArea(5),
            ),
            (
                "RegCch",
                state.mem_reduct.clean_registry_cache,
                Action::ToggleArea(6),
            ),
            (
                "Combine",
                state.mem_reduct.clean_combine_memory_lists,
                Action::ToggleArea(7),
            ),
        ];
        for (idx, (label, active, act)) in areas.iter().enumerate() {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let px = mx + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = c2_y + 38.0 + (row_i as f32 * (pill_h + a_gap_y));
            let ht = p.hot_t(px, py, pill_w, pill_h);
            p.tactile_surface_pill_t(
                px,
                py,
                pill_w,
                pill_h,
                7.0,
                ht,
                if *active {
                    Some(tok::cyno_violet())
                } else {
                    None
                },
            );
            p.text_center(
                label,
                px + pill_w / 2.0,
                py + 16.8 - 0.45 * ht,
                8.8,
                if *active {
                    Weight::Bold
                } else {
                    Weight::Semibold
                },
                if *active {
                    tok::text1()
                } else {
                    lerp_color(tok::text2(), tok::text1(), ht)
                },
            );
            p.hit(px, py, pill_w, pill_h, *act);
        }

        // Card 3: Rules (y = 314..386, h = 72)
        let c3_y = c2_y + c2_h + 10.0;
        let c3_h = 72.0;
        p.elevated_card(mx, c3_y, cw, c3_h, 15.0, None);
        p.text(
            "Clean Above",
            mx + 14.0,
            c3_y + 20.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        p.text_right(
            "Clean Every",
            mx + cw - 14.0,
            c3_y + 20.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );

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
            (
                format!("> {}% RAM", state.mem_reduct.when_above_pct),
                tok::text1(),
            )
        };
        p.text_center(
            &th_str,
            chip1_x + chip_w / 2.0,
            chip_y + 19.0 - 0.55 * c1_ht,
            11.0,
            Weight::Bold,
            th_col,
        );
        p.hit(mx, c3_y, cw / 2.0, c3_h, Action::CycleThreshold);

        let c2_ht = p.hot_t(mx + cw / 2.0, c3_y, cw / 2.0, c3_h);
        p.tactile_surface_pill_t(chip2_x, chip_y, chip_w, chip_h, 8.5, c2_ht, None);
        let (int_str, int_col) = if state.mem_reduct.every_mins == 0 {
            ("Disabled".to_string(), tok::text2())
        } else {
            (
                format!("Every {}m", state.mem_reduct.every_mins),
                tok::cyno_mist(),
            )
        };
        p.text_center(
            &int_str,
            chip2_x + chip_w / 2.0,
            chip_y + 19.0 - 0.55 * c2_ht,
            11.0,
            Weight::Bold,
            int_col,
        );
        p.hit(mx + cw / 2.0, c3_y, cw / 2.0, c3_h, Action::CycleInterval);

        // Card 4: Purge Memory CTA (y = 396..464, h = 68)
        let c4_y = c3_y + c3_h + 10.0;
        let c4_h = 68.0;
        let purge_ht = p.hot_t(mx, c4_y, cw, c4_h);
        p.luminous_pill_button_t(
            mx,
            c4_y,
            cw,
            c4_h,
            15.0,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            purge_ht,
        );

        let dy = -0.8 * purge_ht;
        p.circle(
            mx + 30.0,
            c4_y + c4_h / 2.0 + dy,
            16.0,
            rgba(12, 7, 26, 165),
        );
        p.circle_stroke(
            mx + 30.0,
            c4_y + c4_h / 2.0 + dy,
            16.0,
            1.0,
            rgba(255, 255, 255, 130),
        );
        if state.show_mascot && state.mascot_mood() == MascotMood::Purging {
            let anim_t = state.anim_time;
            Self::draw_cyno_mascot(
                p,
                mx + 30.0,
                c4_y + c4_h / 2.0 + dy + 0.5,
                0.72,
                MascotMood::Purging,
                anim_t,
                true,
            );
        } else {
            icons::draw(
                p,
                Icon::Zap,
                mx + 22.5,
                c4_y + c4_h / 2.0 - 7.5 + dy,
                15.0,
                tok::cyno_mist(),
            );
        }
        let cta_title = if state.mem_reduct.is_purging {
            "Cleaning Memory..."
        } else {
            "Clean Memory"
        };
        p.text_shadow(
            cta_title,
            mx + 54.0,
            c4_y + 30.0 + dy,
            13.5,
            Weight::Bold,
            tok::text1(),
        );
        let status_sub = p.ellipsize_cow(
            &state.mem_reduct.last_cleaned_status,
            9.8,
            Weight::Semibold,
            cw - 66.0,
        );
        p.text_shadow(
            &status_sub,
            mx + 54.0,
            c4_y + 47.0 + dy,
            9.8,
            Weight::Semibold,
            rgba(241, 245, 249, 245),
        );
        p.hit(mx, c4_y, cw, c4_h, Action::PurgeRam);
    }

    pub(super) fn draw_tasks_view_vertical(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;
        let card_h = 376.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            card_h,
            16.0,
            Some((cw - 36.0, 22.0, 82.0, tok::cyno_violet())),
        );

        icons::draw(
            p,
            Icon::List,
            mx + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Tasks & Todos",
            mx + 42.0,
            card_y + 28.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

        let total = state.tasks.len();
        let pending = state.tasks.iter().filter(|t| !t.completed).count();
        let start_idx = state.tasks_scroll.min(total.saturating_sub(5));
        let sub = if total > 5 {
            format!(
                "{} open • {}-{} of {} (scroll)",
                pending,
                start_idx + 1,
                (start_idx + 5).min(total),
                total
            )
        } else {
            format!("{} open • Click item or × to manage", pending)
        };
        p.text(
            &sub,
            mx + 42.0,
            card_y + 44.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let btn_y = card_y + 58.0;
        let btn_h = 28.0;
        let btn_w = (cw - 32.0 - 8.0) / 2.0;
        let add_ht = p.hot_t(mx + 16.0, btn_y, btn_w, btn_h);
        p.tactile_surface_pill_t(
            mx + 16.0,
            btn_y,
            btn_w,
            btn_h,
            8.5,
            add_ht,
            Some(tok::cyno_violet()),
        );
        p.text_center(
            "+ From Clip",
            mx + 16.0 + btn_w / 2.0,
            btn_y + 18.0 - 0.55 * add_ht,
            10.5,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(mx + 16.0, btn_y, btn_w, btn_h, Action::PasteAsTask);

        let clr_x = mx + 16.0 + btn_w + 8.0;
        let clr_ht = p.hot_t(clr_x, btn_y, btn_w, btn_h);
        p.tactile_surface_pill_t(clr_x, btn_y, btn_w, btn_h, 8.5, clr_ht, None);
        p.text_center(
            "Clear Done",
            clr_x + btn_w / 2.0,
            btn_y + 18.0 - 0.55 * clr_ht,
            10.5,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), clr_ht),
        );
        p.hit(clr_x, btn_y, btn_w, btn_h, Action::ClearCompletedTasks);

        if state.tasks.is_empty() {
            p.text_center(
                "All tasks clear!",
                mx + cw / 2.0,
                card_y + 195.0,
                12.0,
                Weight::Semibold,
                tok::text1(),
            );
            p.text_center(
                "Copy text & tap '+ From Clip'",
                mx + cw / 2.0,
                card_y + 216.0,
                10.5,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            for (slot, (real_idx, task)) in state
                .tasks
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(5)
                .enumerate()
            {
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
                    p.radial_glow(
                        check_cx,
                        check_cy,
                        11.0,
                        rgba(139, 92, 246, 95),
                        BlendMode::Plus,
                        false,
                    );
                    p.circle(check_cx, check_cy, 7.0, tok::cyno_violet());
                    icons::draw(
                        p,
                        Icon::Check,
                        check_cx - 4.5,
                        check_cy - 4.5,
                        9.0,
                        rgba(255, 255, 255, 255),
                    );
                } else {
                    let well = p.well_col();
                    p.circle(check_cx, check_cy, 7.0, well);
                    p.circle_stroke(check_cx, check_cy, 7.0, 1.0, rgba(255, 255, 255, 70));
                }
                let task_text = p.ellipsize_cow(&task.text, 11.2, Weight::Regular, row_w - 42.0);
                let text_color = if task.completed {
                    tok::text3()
                } else {
                    tok::text1()
                };
                p.text(
                    &task_text,
                    item_x + 32.0,
                    item_y + 24.0 + dy,
                    11.2,
                    Weight::Regular,
                    text_color,
                );
                p.hit(item_x, item_y, row_w, item_h, Action::ToggleTask(real_idx));

                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 9.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(
                        del_x,
                        item_y - 0.55 * del_ht,
                        del_w,
                        item_h,
                        9.5,
                        fade(rgba(220, 38, 38, 195), del_ht),
                    );
                }
                icons::draw(
                    p,
                    Icon::Close,
                    del_x + 11.0,
                    item_y + 14.0 - 0.55 * del_ht,
                    12.0,
                    lerp_color(tok::text2(), tok::text1(), del_ht),
                );
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteTask(real_idx));
            }
        }
    }

    pub(super) fn draw_shelf_view_vertical(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let mx = 16.0;
        let cw = w - 32.0;
        let card_h = 376.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            card_h,
            16.0,
            Some((cw - 36.0, 22.0, 82.0, tok::cyno_orchid())),
        );

        icons::draw(
            p,
            Icon::Clipboard,
            mx + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Clipboard & Shelf",
            mx + 42.0,
            card_y + 28.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

        let clr_w = 56.0;
        let clr_h = 26.0;
        let clr_x = mx + cw - clr_w - 14.0;
        let clr_y = card_y + 14.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, clr_h);
        p.tactile_surface_pill_t(clr_x, clr_y, clr_w, clr_h, 8.0, clr_ht, None);
        p.text_center(
            "Clear",
            clr_x + clr_w / 2.0,
            clr_y + 17.0 - 0.55 * clr_ht,
            10.0,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), clr_ht),
        );
        p.hit(clr_x, clr_y, clr_w, clr_h, Action::ClearShelf);

        let total = state.shelf_items.len();
        let start_idx = state.shelf_scroll.min(total.saturating_sub(6));
        let sub = if total > 6 {
            format!(
                "Drop files or Ctrl+C • {}-{} of {}",
                start_idx + 1,
                (start_idx + 6).min(total),
                total
            )
        } else {
            "Drop files or Ctrl+C • Click to open/copy".to_string()
        };
        let sub_ell = p.ellipsize_cow(&sub, 9.8, Weight::Regular, cw - 32.0);
        p.text(
            &sub_ell,
            mx + 16.0,
            card_y + 46.0,
            9.8,
            Weight::Regular,
            tok::text2(),
        );

        if state.shelf_items.is_empty() {
            p.text_center(
                "Shelf empty",
                mx + cw / 2.0,
                card_y + 190.0,
                12.0,
                Weight::Semibold,
                tok::text1(),
            );
            p.text_center(
                "Drop files or copy text anywhere",
                mx + cw / 2.0,
                card_y + 212.0,
                10.2,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            for (slot, (real_idx, item)) in state
                .shelf_items
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(6)
                .enumerate()
            {
                let item_y = card_y + 64.0 + (slot as f32 * 50.0);
                let item_h = 38.0;
                let item_x = mx + 16.0;
                let del_w = 34.0;
                let row_w = cw - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 9.0, row_ht, None);
                let dy = -0.55 * row_ht;
                if row_ht > 0.02 {
                    p.rrect(
                        item_x,
                        item_y + dy,
                        row_w,
                        item_h,
                        9.0,
                        fade(tok::cyno_violet(), 0.25 * row_ht),
                    );
                }
                let (ic, ic_col) = match item.kind {
                    ShelfKind::File => (Icon::File, tok::cyno_mist()),
                    ShelfKind::Text => (Icon::Clipboard, tok::cyno_violet_hi()),
                };
                icons::draw(p, ic, item_x + 11.0, item_y + 12.5 + dy, 13.0, ic_col);

                let clip_snippet = p.ellipsize_cow(&item.label, 11.0, Weight::Regular, row_w - 42.0);
                p.text(
                    &clip_snippet,
                    item_x + 31.0,
                    item_y + 23.0 + dy,
                    11.0,
                    Weight::Regular,
                    tok::text1(),
                );
                p.hit(
                    item_x,
                    item_y,
                    row_w,
                    item_h,
                    Action::ActivateShelfItem(real_idx),
                );

                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 9.0, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(
                        del_x,
                        item_y - 0.55 * del_ht,
                        del_w,
                        item_h,
                        9.0,
                        fade(rgba(220, 38, 38, 195), del_ht),
                    );
                }
                icons::draw(
                    p,
                    Icon::Close,
                    del_x + 11.0,
                    item_y + 13.0 - 0.55 * del_ht,
                    12.0,
                    lerp_color(tok::text2(), tok::text1(), del_ht),
                );
                p.hit(
                    del_x,
                    item_y,
                    del_w,
                    item_h,
                    Action::DeleteShelfItem(real_idx),
                );
            }
        }
    }

    pub(super) fn draw_capture_view_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        card_y: f32,
    ) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: Instant Capture Triggers (y = 88..270, h = 182)
        let c1_h = 182.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            16.0,
            Some((cw * 0.5, 42.0, 78.0, tok::cyno_violet())),
        );
        icons::draw(
            p,
            Icon::Video,
            mx + 16.0,
            card_y + 18.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Screen Snapshot",
            mx + 42.0,
            card_y + 30.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Saves PNG & copies (Ctrl+Shift+S)",
            mx + 16.0,
            card_y + 50.0,
            10.2,
            Weight::Regular,
            tok::text2(),
        );

        let btn_x = mx + 16.0;
        let btn_w = cw - 32.0;
        let b1_y = card_y + 70.0;
        let b1_h = 44.0;
        let b1_ht = p.hot_t(btn_x, b1_y, btn_w, b1_h);
        p.luminous_pill_button_t(
            btn_x,
            b1_y,
            btn_w,
            b1_h,
            11.5,
            tok::cyno_indigo(),
            tok::cyno_violet(),
            tok::cyno_orchid(),
            b1_ht,
        );
        p.text_center_shadow(
            "Capture Full Screen",
            btn_x + btn_w / 2.0,
            b1_y + 26.5 - 0.8 * b1_ht,
            12.5,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(btn_x, b1_y, btn_w, b1_h, Action::CaptureScreen);

        let b2_y = card_y + 126.0;
        let b2_h = 36.0;
        let b2_ht = p.hot_t(btn_x, b2_y, btn_w, b2_h);
        p.tactile_surface_pill_t(btn_x, b2_y, btn_w, b2_h, 10.5, b2_ht, None);
        p.text_center(
            "Capture Active Window",
            btn_x + btn_w / 2.0,
            b2_y + 22.0 - 0.55 * b2_ht,
            11.5,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), b2_ht),
        );
        p.hit(btn_x, b2_y, btn_w, b2_h, Action::CaptureWindow);

        // Card 2: Capture Output (y = 282..464, h = 182)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 182.0;
        p.elevated_card(
            mx,
            c2_y,
            cw,
            c2_h,
            16.0,
            Some((cw - 32.0, 26.0, 76.0, tok::cyno_orchid())),
        );
        p.dot_matrix(
            mx + cw - 46.0,
            c2_y + 16.0,
            5,
            3,
            5.0,
            rgba(255, 255, 255, 30),
        );
        icons::draw(
            p,
            Icon::Folder,
            mx + 16.0,
            c2_y + 18.0,
            18.0,
            tok::cyno_mist(),
        );
        p.text(
            "Capture Output",
            mx + 42.0,
            c2_y + 30.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

        if let Some(ref _path) = state.capture.last_path {
            let fname = p.ellipsize_cow(
                &state.capture.last_filename,
                11.8,
                Weight::Semibold,
                cw - 32.0,
            );
            p.text(
                &fname,
                mx + 16.0,
                c2_y + 72.0,
                11.8,
                Weight::Semibold,
                tok::text1(),
            );
            let dims = format!(
                "{} • {}",
                state.capture.last_dims,
                if state.capture.clipboard_ok {
                    "Copied to Clipboard"
                } else {
                    "Saved to Disk"
                }
            );
            p.text(
                &dims,
                mx + 16.0,
                c2_y + 92.0,
                10.5,
                Weight::Regular,
                tok::cyno_violet_hi(),
            );
        } else {
            p.text(
                "No capture taken in this session",
                mx + 16.0,
                c2_y + 72.0,
                11.8,
                Weight::Semibold,
                tok::text1(),
            );
            p.text(
                "Output: Pictures\\CyNotch\\*.png",
                mx + 16.0,
                c2_y + 92.0,
                10.5,
                Weight::Regular,
                tok::text2(),
            );
        }

        let open_y = c2_y + 126.0;
        let open_h = 36.0;
        let open_ht = p.hot_t(btn_x, open_y, btn_w, open_h);
        p.tactile_surface_pill_t(
            btn_x,
            open_y,
            btn_w,
            open_h,
            10.5,
            open_ht,
            Some(tok::cyno_violet()),
        );
        p.text_center(
            "Open Pictures\\CyNotch",
            btn_x + btn_w / 2.0,
            open_y + 22.0 - 0.55 * open_ht,
            11.2,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(btn_x, open_y, btn_w, open_h, Action::OpenCaptureFolder);
    }

    pub(super) fn draw_telemetry_view_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        card_y: f32,
    ) {
        let mx = 16.0;
        let cw = w - 32.0;

        // Row 1: Two side-by-side summary cards (Today & 7-day avg, y = 88..198, h = 110)
        let r1_h = 110.0;
        let sub_gap = 12.0;
        let sub_w = (cw - sub_gap) / 2.0;

        p.elevated_card(
            mx,
            card_y,
            sub_w,
            r1_h,
            15.0,
            Some((sub_w - 22.0, 16.0, 56.0, tok::cyno_violet())),
        );
        p.dot_matrix(
            mx + sub_w - 38.0,
            card_y + 10.0,
            4,
            3,
            5.0,
            rgba(255, 255, 255, 30),
        );
        p.text(
            "Today",
            mx + 14.0,
            card_y + 24.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        let today_h = state.screen_time.today_secs / 3600;
        let today_m = (state.screen_time.today_secs % 3600) / 60;
        p.text(
            &format!("{} h {} m", today_h, today_m),
            mx + 14.0,
            card_y + 62.0,
            18.0,
            Weight::Bold,
            tok::text1(),
        );
        let cpu_sub = format!(
            "CPU {:.0}% • RAM {:.0}%",
            state.telemetry.cpu_usage, state.telemetry.memory_percent
        );
        p.text(
            &cpu_sub,
            mx + 14.0,
            card_y + 92.0,
            9.2,
            Weight::Regular,
            tok::text2(),
        );

        let r_x = mx + sub_w + sub_gap;
        p.elevated_card(r_x, card_y, sub_w, r1_h, 15.0, None);
        p.text(
            "7-day avg",
            r_x + 14.0,
            card_y + 24.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        p.text_right(
            &format!("{}d log", state.screen_time.tracked_days.max(1)),
            r_x + sub_w - 14.0,
            card_y + 24.0,
            9.2,
            Weight::Semibold,
            tok::text2(),
        );
        let avg_h = state.screen_time.seven_day_avg_secs / 3600;
        let avg_m = (state.screen_time.seven_day_avg_secs % 3600) / 60;
        p.text(
            &format!("{} h {} m", avg_h, avg_m),
            r_x + 14.0,
            card_y + 62.0,
            18.0,
            Weight::Bold,
            tok::cyno_violet_hi(),
        );
        let net_sub = format!(
            "↓{}  ↑{}",
            format_speed_short(state.telemetry.net_down_kbps),
            format_speed_short(state.telemetry.net_up_kbps)
        );
        let net_ell = p.ellipsize_cow(&net_sub, 9.5, Weight::Semibold, sub_w - 28.0);
        p.text(
            &net_ell,
            r_x + 14.0,
            card_y + 92.0,
            9.5,
            Weight::Semibold,
            tok::cyan_hi(),
        );

        // Row 2: Most Used App (y = 210..320, h = 110)
        let r2_y = card_y + r1_h + 12.0;
        let r2_h = 110.0;
        p.elevated_card(
            mx,
            r2_y,
            cw,
            r2_h,
            15.0,
            Some((cw - 24.0, 18.0, 66.0, tok::cyno_orchid())),
        );
        p.text(
            "Most used app",
            mx + 16.0,
            r2_y + 23.0,
            10.8,
            Weight::Semibold,
            tok::text2(),
        );
        if let Some((second_name, second_secs)) = state.screen_time.top_apps.get(1) {
            let mut runner_buf = StackStr::<64>::new();
            let _ = write!(runner_buf, "#2 {} {}m", second_name, second_secs / 60);
            let runner = p.ellipsize_cow(&runner_buf, 9.8, Weight::Regular, cw - 126.0);
            p.text_right(
                &runner,
                mx + cw - 16.0,
                r2_y + 23.0,
                9.8,
                Weight::Regular,
                tok::text2(),
            );
        }
        let top_name = p.ellipsize_cow(
            &state.screen_time.top_app_name,
            16.5,
            Weight::Bold,
            cw - 110.0,
        );
        p.text(
            &top_name,
            mx + 16.0,
            r2_y + 54.0,
            16.5,
            Weight::Bold,
            tok::cyno_mist(),
        );
        let top_h = state.screen_time.top_app_secs / 3600;
        let top_m = (state.screen_time.top_app_secs % 3600) / 60;
        p.text_right(
            &format!("{} h {} m", top_h, top_m),
            mx + cw - 16.0,
            r2_y + 54.0,
            12.5,
            Weight::Semibold,
            tok::text1(),
        );

        let share = if state.screen_time.today_secs > 0 {
            (state.screen_time.top_app_secs as f32 / state.screen_time.today_secs as f32)
                .clamp(0.08, 1.0)
        } else {
            0.0
        };
        p.glowing_striped_bar(
            mx + 16.0,
            r2_y + 74.0,
            cw - 32.0,
            8.0,
            share,
            tok::cyno_deep(),
            tok::cyno_orchid(),
        );

        // Row 3: Peak Hour + Full-width 24h Sparkline (y = 332..464, h = 132)
        let r3_y = r2_y + r2_h + 12.0;
        let r3_h = 132.0;
        p.elevated_card(
            mx,
            r3_y,
            cw,
            r3_h,
            15.0,
            Some((cw * 0.5, 42.0, 68.0, tok::cyno_violet())),
        );
        p.text(
            "Peak hour & 24h Activity",
            mx + 16.0,
            r3_y + 24.0,
            11.0,
            Weight::Semibold,
            tok::text2(),
        );
        p.text_right(
            &state.screen_time.peak_hour_str,
            mx + cw - 16.0,
            r3_y + 25.0,
            15.5,
            Weight::Bold,
            tok::text1(),
        );

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
        p.text(
            "00h",
            chart_x,
            r3_y + 123.0,
            8.8,
            Weight::Regular,
            tok::text3(),
        );
        p.text_center(
            "12h",
            chart_x + chart_w * 0.5,
            r3_y + 123.0,
            8.8,
            Weight::Regular,
            tok::text3(),
        );
        p.text_right(
            "22h",
            chart_x + chart_w,
            r3_y + 123.0,
            8.8,
            Weight::Regular,
            tok::text3(),
        );
    }

    pub(super) fn draw_settings_view_vertical(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        card_y: f32,
    ) {
        let smooth = state.micro_interactions;
        let mx = 16.0;
        let cw = w - 32.0;

        // Card 1: System Integration (y = 88..254, h = 166)
        let c1_h = 166.0;
        p.elevated_card(
            mx,
            card_y,
            cw,
            c1_h,
            16.0,
            Some((32.0, 22.0, 72.0, tok::cyno_violet())),
        );
        icons::draw(
            p,
            Icon::Settings,
            mx + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "System Integration",
            mx + 42.0,
            card_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Startup, chime & global hotkeys",
            mx + 42.0,
            card_y + 43.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let sw_w = 46.0;
        let sw_h = 22.0;
        let row_hit_w = cw - 32.0;
        let r1_y = card_y + 62.0;
        p.text(
            "Launch on Startup",
            mx + 16.0,
            r1_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw1_x = mx + cw - sw_w - 16.0;
        let sw1_ht = p.hot_t(mx + 16.0, r1_y, row_hit_w, sw_h);
        let st1 = if smooth {
            state.switch_t[ToggleId::Autostart as usize]
        } else if state.autostart {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, r1_y, sw_w, sw_h, st1, sw1_ht);
        p.hit(mx + 16.0, r1_y, row_hit_w, sw_h, Action::ToggleAutostart);

        let r2_y = card_y + 94.0;
        p.text(
            "Action Audio",
            mx + 16.0,
            r2_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw2_ht = p.hot_t(mx + 16.0, r2_y, row_hit_w, sw_h);
        let st2 = if smooth {
            state.switch_t[ToggleId::Chime as usize]
        } else if state.chime_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, r2_y, sw_w, sw_h, st2, sw2_ht);
        p.hit(mx + 16.0, r2_y, row_hit_w, sw_h, Action::ToggleChime);

        let r3_y = card_y + 128.0;
        p.text(
            "Hotkeys (C/M/S)",
            mx + 16.0,
            r3_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let badge_w = 112.0;
        let badge_x = mx + cw - badge_w - 16.0;
        let well = p.well_col();
        p.rrect(badge_x, r3_y, badge_w, 22.0, 7.0, well);
        p.rrect_bevel_stroke(
            badge_x,
            r3_y,
            badge_w,
            22.0,
            7.0,
            1.0,
            rgba(192, 132, 252, 90),
            rgba(255, 255, 255, 20),
        );
        p.text_center(
            "Ctrl+Shift+C/M/S",
            badge_x + badge_w / 2.0,
            r3_y + 14.8,
            9.2,
            Weight::Semibold,
            tok::cyno_violet_hi(),
        );

        // Card 2: Engine & Lifecycle (y = 266..464, h = 198)
        let c2_y = card_y + c1_h + 12.0;
        let c2_h = 198.0;
        p.elevated_card(
            mx,
            c2_y,
            cw,
            c2_h,
            16.0,
            Some((cw - 28.0, 22.0, 72.0, tok::cyno_orchid())),
        );
        icons::draw(
            p,
            Icon::Settings,
            mx + 16.0,
            c2_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Engine & Lifecycle",
            mx + 42.0,
            c2_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Preferences, positioning & exit",
            mx + 42.0,
            c2_y + 43.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let c2_r1_y = c2_y + 62.0;
        p.text(
            "Frosted Glass Backdrop",
            mx + 16.0,
            c2_r1_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw3_ht = p.hot_t(mx + 16.0, c2_r1_y, row_hit_w, sw_h);
        let st3 = if smooth {
            state.switch_t[ToggleId::FrostedGlass as usize]
        } else if state.frosted_glass_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, c2_r1_y, sw_w, sw_h, st3, sw3_ht);
        p.hit(
            mx + 16.0,
            c2_r1_y,
            row_hit_w,
            sw_h,
            Action::ToggleFrostedGlass,
        );

        let c2_r2_y = c2_y + 96.0;
        p.text(
            "Minimize Idle Island",
            mx + 16.0,
            c2_r2_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw4_ht = p.hot_t(mx + 16.0, c2_r2_y, row_hit_w, sw_h);
        let st4 = if smooth {
            state.switch_t[ToggleId::MinimizedIsland as usize]
        } else if state.minimized_island {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, c2_r2_y, sw_w, sw_h, st4, sw4_ht);
        p.hit(
            mx + 16.0,
            c2_r2_y,
            row_hit_w,
            sw_h,
            Action::ToggleMinimizedIsland,
        );

        let bot_y = c2_y + 140.0;
        let bot_h = 38.0;
        let quit_w = 88.0;
        let spec_w = cw - 32.0 - quit_w - 8.0;
        let moved = state.x_offset != 0 || state.y_offset != 0;
        let left_ht = p.hot_t(mx + 16.0, bot_y, spec_w, bot_h);
        p.tactile_surface_pill_t(mx + 16.0, bot_y, spec_w, bot_h, 9.5, left_ht, None);
        let pos_lbl = if moved {
            "Center Notch"
        } else {
            "Advanced Settings"
        };
        p.text_center(
            pos_lbl,
            mx + 16.0 + spec_w / 2.0,
            bot_y + 23.0 - 0.55 * left_ht,
            10.5,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), left_ht),
        );
        p.hit(
            mx + 16.0,
            bot_y,
            spec_w,
            bot_h,
            if moved {
                Action::ResetPosition
            } else {
                Action::OpenSettings
            },
        );

        let quit_x = mx + 16.0 + spec_w + 8.0;
        let quit_ht = p.hot_t(quit_x, bot_y, quit_w, bot_h);
        p.tactile_surface_pill_t(
            quit_x,
            bot_y,
            quit_w,
            bot_h,
            9.5,
            quit_ht,
            Some(rgba(220, 38, 38, 255)),
        );
        p.text_center_shadow(
            "Exit App",
            quit_x + quit_w / 2.0,
            bot_y + 23.0 - 0.55 * quit_ht,
            10.5,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(quit_x, bot_y, quit_w, bot_h, Action::Quit);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // CYBER-ANUBIS CYNO MASCOT (Icon-Matched Sculpted Vector Rig + 6 Mood Loops)
    // ─────────────────────────────────────────────────────────────────────────────
}
