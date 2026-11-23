use super::*;
use crate::painter_d2d::{ButtonVariant, Color, PainterD2D, lerp_color, rgba, tok};
use crate::font_d2d::Weight;

impl Renderer {
    pub(super) fn notification_accent_and_icon(
        notif: &crate::models::NotificationBanner,
    ) -> (Color, Icon) {
        let accent = match notif.kind {
            NotificationKind::MemoryPurge => tok::cyno_orchid(),
            NotificationKind::Capture
            | NotificationKind::Pasted
            | NotificationKind::SystemToast
            | NotificationKind::General => tok::cyno_violet_hi(),
        };
        let icon = match notif.kind {
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

    pub(super) fn notification_remaining_frac(notif: &crate::models::NotificationBanner) -> f32 {
        notif.remaining_frac(std::time::Instant::now())
    }

    pub(super) fn draw_notification_banner(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
        let Some(ref notif) = state.notification else {
            return;
        };
        let (_accent, kind_icon) = Self::notification_accent_and_icon(notif);
        let app_icon = icons::for_app(&notif.app_name, kind_icon);
        let rem_frac = Self::notification_remaining_frac(notif);
        let cy = h * 0.5;

        // BH-20: Clip subtle bottom violet bloom strictly inside the notification capsule bounds.
        let (r_tl, r_tr, r_br, r_bl) = state.corner_radii();
        let inv_s = if p.s > 1e-3 { 1.0 / p.s } else { 1.0 };
        let clipped = p.prepare_corners4_clip(
            0.0,
            0.0,
            w,
            h,
            r_tl * inv_s,
            r_tr * inv_s,
            r_br * inv_s,
            r_bl * inv_s,
        );
        p.elliptical_glow(
            w * 0.48,
            h + 4.0,
            w * 0.56,
            (h * 0.60).max(26.0),
            rgba(124, 58, 237, 44),
            BlendMode::Plus,
            true,
        );
        if clipped {
            p.pop_clip();
        }

        // Left Circular Translucent Obsidian App-Icon Orb (PrimaryOutline with 48% bottom-to-top rim)
        let left_cx = 30.0;
        let badge_r = 18.0;
        p.button_pill_variant(
            left_cx - badge_r,
            cy - badge_r,
            badge_r * 2.0,
            badge_r * 2.0,
            badge_r,
            0.0,
            ButtonVariant::PrimaryOutline,
        );
        icons::draw(
            p,
            app_icon,
            left_cx - 9.0,
            cy - 9.0,
            18.0,
            tok::cyno_mist(),
        );

        // Right dismiss button + autonomous 30Hz lavender countdown ring (#C084FC)
        let dismiss_cx = (w - 24.0).max(120.0);
        let dis_ht = p.hot_t(
            dismiss_cx - 14.0,
            (cy - 14.0).max(0.0),
            28.0,
            28.0_f32.min(h),
        );
        p.button_pill_variant(
            dismiss_cx - 13.5,
            cy - 13.5,
            27.0,
            27.0,
            13.5,
            dis_ht,
            ButtonVariant::PrimaryOutline,
        );
        Self::draw_progress_arc_badge(
            p,
            dismiss_cx,
            cy,
            13.5,
            Some(rem_frac),
            tok::amber_glow(),
            rgba(0, 0, 0, 0),
            Icon::Close,
            lerp_color(tok::text_soft(), tok::text1(), dis_ht),
            true,
        );
        p.hit(
            dismiss_cx - 15.0,
            (cy - 15.0).max(0.0),
            30.0,
            30.0_f32.min(h),
            Action::DismissNotification,
        );

        let mut right_edge = dismiss_cx - 20.0;
        if !state.notif_queue.is_empty() {
            let q_lbl = format!("+{}", state.notif_queue.len());
            let q_w = (p.measure(&q_lbl, 9.6, Weight::Bold) + 14.0).clamp(26.0, 44.0);
            let q_x = (right_edge - q_w - 4.0).max(120.0);
            let q_y = (cy - 11.0).max(4.0);
            let q_ht = p.hot_t(q_x, q_y, q_w, 22.0);
            p.button_pill_variant(q_x, q_y, q_w, 22.0, 11.0, q_ht, ButtonVariant::PrimaryFill);
            p.text_center_fit(
                &q_lbl,
                q_x + q_w * 0.5,
                q_y + 14.5,
                9.6,
                Weight::Bold,
                tok::cyno_lavender_white(),
                q_w - 6.0,
            );
            p.hit(q_x, q_y, q_w, 22.0, Action::ClearNotificationQueue);
            right_edge = q_x - 6.0;
        }

        let right_edge = right_edge.min(dismiss_cx - 22.0);
        let tx = 58.0;
        let max_tw = (right_edge - tx).max(100.0);
        let app_tag_owned;
        let app_tag = if notif.grouped_count > 1 && !notif.app_name.trim().is_empty() {
            app_tag_owned = format!("{} +{}", notif.app_name.trim(), notif.grouped_count - 1);
            app_tag_owned.as_str()
        } else {
            notif.app_name.trim()
        };
        let show_tag = !app_tag.is_empty();
        let sub_text = notif.subtitle.trim();
        let has_spaces = sub_text.contains(char::is_whitespace);
        let (sub_lines, sub_sz) = if sub_text.is_empty() {
            (Vec::new(), 12.2)
        } else if !has_spaces && p.measure(sub_text, 11.5, Weight::Regular) > max_tw {
            // Single unbroken URL / long filename: keep on 1 line and fade smoothly on right edge
            (vec![sub_text.to_string()], 11.5)
        } else {
            p.wrap_lines_fit(sub_text, 12.2, 11.0, Weight::Regular, max_tw, 2)
        };

        let line_gap = (p.eff_size(sub_sz) * 1.26).clamp(15.0, 18.0);
        let (row1_y, sub_y0, sub_step) = match sub_lines.len() {
            0 => (cy + 5.0, 0.0, 0.0),
            1 => {
                if h >= 84.0 {
                    (33.0, 56.0, line_gap)
                } else {
                    (29.5, 51.0, line_gap)
                }
            }
            _ => {
                if h >= 84.0 {
                    (27.0, 48.0, line_gap)
                } else {
                    (24.5, 43.5, line_gap.min(17.5))
                }
            }
        };

        let title_raw_sz = 13.5;
        let mut title_x = tx;
        let mut title_w = max_tw;
        if show_tag {
            let tag_txt = p.ellipsize_cow(
                app_tag,
                13.0,
                Weight::Bold,
                (max_tw * 0.42).max(58.0),
            );
            let tag_tw = p.measure(&tag_txt, 13.0, Weight::Bold);
            p.text(&tag_txt, tx, row1_y, 13.0, Weight::Bold, tok::text1());
            // Amber support separator dot (#FBBF24) matching DESIGN_SYSTEM.md §2 Molecule #4
            let dot_x = tx + tag_tw + 7.5;
            p.circle(dot_x, row1_y - 4.0, 2.0, tok::amber_glow());
            title_x = dot_x + 7.5;
            title_w = (max_tw - (title_x - tx)).max(48.0);
        }

        p.text_fade_right(
            &notif.title,
            title_x,
            row1_y,
            title_raw_sz,
            if show_tag { Weight::Semibold } else { Weight::Bold },
            if show_tag { tok::text2() } else { tok::text1() },
            title_w,
        );

        let body_col = rgba(235, 231, 255, 245);
        for (idx, line) in sub_lines.iter().enumerate() {
            p.text_fade_right(
                line,
                tx,
                sub_y0 + (idx as f32 * sub_step),
                sub_sz,
                Weight::Regular,
                body_col,
                max_tw,
            );
        }

        let body_action = match notif.kind {
            NotificationKind::SystemToast => Action::ActivateNotification,
            NotificationKind::Capture if notif.is_success => Action::OpenCaptureFolder,
            _ => Action::DismissNotification,
        };
        p.hit(0.0, 0.0, w, h, body_action);
    }

    pub(super) fn format_relative_age(elapsed_secs: u64) -> String {
        if elapsed_secs < 45 {
            "now".to_string()
        } else if elapsed_secs < 3600 {
            format!("{}m", elapsed_secs / 60)
        } else {
            format!("{}h", elapsed_secs / 3600)
        }
    }

    pub(super) fn draw_notif_history_view(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        h: f32,
        card_y: f32,
    ) {
        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        let prev_alpha = p.global_alpha;
        p.global_alpha = prev_alpha * a0;
        let card_y = card_y + dy0;
        let mx = 24.0;
        let cw = (w - mx * 2.0).max(160.0);

        if state.notif_history.is_empty() {
            let empty_h = (h - card_y - 24.0).clamp(110.0, 220.0);
            p.push_scale_about(mx + cw * 0.5, card_y + empty_h * 0.5, sc0);
            p.elevated_card(mx, card_y, cw, empty_h, 20.0, None);
            let cx = mx + cw * 0.5;
            let cy = card_y + empty_h * 0.5 - 10.0;
            p.circle(cx, cy, 18.0, rgba(124, 58, 237, 40));
            icons::draw(p, Icon::Bell, cx - 9.0, cy - 9.0, 18.0, tok::cyno_mist());
            p.text_center(
                "No recent notifications",
                cx,
                cy + 34.0,
                12.0,
                Weight::Bold,
                tok::text2(),
            );
            p.pop_scale_transform();
            p.global_alpha = prev_alpha;
            return;
        }

        // Top action row: summary + Clear All TextOnly button
        let count_lbl = format!("{} recent", state.notif_history.len());
        p.text(
            &count_lbl,
            mx + 2.0,
            card_y + 13.0,
            11.0,
            Weight::Semibold,
            tok::text2(),
        );

        let clr_w = 64.0;
        let clr_x = mx + cw - clr_w;
        let clr_y = card_y - 2.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, 22.0);
        p.button_pill_variant(clr_x, clr_y, clr_w, 22.0, 11.0, clr_ht, ButtonVariant::TextOnly);
        p.text_center_fit(
            "Clear all",
            clr_x + clr_w * 0.5,
            clr_y + 14.5,
            10.2,
            Weight::Semibold,
            lerp_color(tok::text2(), tok::text1(), clr_ht),
            clr_w - 8.0,
        );
        p.hit(clr_x, clr_y, clr_w, 22.0, Action::ClearNotifHistory);

        let list_y = card_y + 26.0;
        let avail_h = (h - list_y - 12.0).max(52.0);
        let row_h = 50.0;
        let gap = 6.0;
        let max_rows = ((avail_h + gap) / (row_h + gap)).floor().max(1.0) as usize;

        for (idx, entry) in state.notif_history.iter().take(max_rows).enumerate() {
            let (dy_r, a_r, sc_r) = Self::mode_stagger(state, (1 + idx).min(5));
            let base_ry = list_y + (idx as f32) * (row_h + gap);
            if base_ry + row_h > h - 6.0 {
                break;
            }
            let ry = base_ry + dy_r;
            p.global_alpha = prev_alpha * a_r;
            p.push_scale_about(mx + cw * 0.5, ry + row_h * 0.5, sc_r);
            let row_ht = p.hot_t(mx, ry, cw, row_h);
            p.button_pill_variant(mx, ry, cw, row_h, 12.0, row_ht, ButtonVariant::PrimaryOutline);

            let ic_cx = mx + 22.0;
            let ic_cy = ry + row_h * 0.5;
            p.circle(ic_cx, ic_cy, 13.0, rgba(124, 58, 237, 46));
            let app_icon = icons::for_app(&entry.app_name, Icon::Bell);
            icons::draw(
                p,
                app_icon,
                ic_cx - 7.0,
                ic_cy - 7.0,
                14.0,
                tok::cyno_lavender_white(),
            );

            // Dismiss button (TextOnly, registered BEFORE row activation hit)
            let dis_cx = mx + cw - 18.0;
            let dis_ht = p.hot_t(dis_cx - 12.0, ry + 6.0, 24.0, row_h - 12.0);
            p.button_pill_variant(
                dis_cx - 11.0,
                ic_cy - 11.0,
                22.0,
                22.0,
                11.0,
                dis_ht,
                ButtonVariant::TextOnly,
            );
            icons::draw(
                p,
                Icon::Close,
                dis_cx - 5.0,
                ic_cy - 5.0,
                10.0,
                lerp_color(tok::text2(), tok::text1(), dis_ht),
            );
            p.hit(
                dis_cx - 12.0,
                ry + 4.0,
                24.0,
                row_h - 8.0,
                Action::DismissHistoryItem(idx),
            );

            let age = Self::format_relative_age(entry.received_at.elapsed().as_secs());
            let age_w = p.measure(&age, 9.6, Weight::Semibold);
            let age_x = dis_cx - 16.0 - age_w;
            p.text(&age, age_x, ry + 20.0, 9.6, Weight::Semibold, tok::text2());

            let tx = mx + 44.0;
            let title_max_w = (age_x - tx - 8.0).max(60.0);
            let hdr = if entry.app_name.trim().is_empty() {
                entry.title.clone()
            } else {
                format!("{}  -  {}", entry.app_name.trim(), entry.title.trim())
            };
            let hdr_fit = p.ellipsize_cow(&hdr, 11.4, Weight::Bold, title_max_w);
            p.text(&hdr_fit, tx, ry + 20.5, 11.4, Weight::Bold, tok::text1());

            let sub_max_w = (dis_cx - 16.0 - tx).max(60.0);
            let sub_fit = p.ellipsize_cow(entry.subtitle.trim(), 10.6, Weight::Regular, sub_max_w);
            p.text(&sub_fit, tx, ry + 37.5, 10.6, Weight::Regular, tok::text2());

            p.hit(mx, ry, cw, row_h, Action::ActivateHistoryItem(idx));
            p.pop_scale_transform();
        }
        p.global_alpha = prev_alpha;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // ULTRA-COMPACT MINIMIZED STEALTH PILL (86x18 Horizontal, 18x86 Vertical)
    // ─────────────────────────────────────────────────────────────────────────────
}
