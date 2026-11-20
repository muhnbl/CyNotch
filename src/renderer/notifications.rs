use super::*;
use crate::painter_d2d::{PainterD2D, Color, tok, rgba, fade, lerp_color};
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
        let now = std::time::Instant::now();
        if notif.expires_at > now {
            (notif
                .expires_at
                .saturating_duration_since(now)
                .as_secs_f32()
                / notif.total_secs.max(1.0))
            .clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub(super) fn draw_notification_banner(p: &mut PainterD2D, state: &AppState, w: f32, h: f32) {
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
        p.radial_glow(
            left_cx,
            cy,
            26.0,
            fade(tok::cyno_violet(), 0.34),
            BlendMode::Plus,
            true,
        );
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
        icons::draw(
            p,
            app_icon,
            left_cx - 9.5,
            cy - 9.5,
            19.0,
            tok::cyno_lavender_white(),
        );

        // Right dismiss button + countdown ring
        let dismiss_cx = (w - 23.0).max(120.0);
        let dis_ht = p.hot_t(
            dismiss_cx - 14.0,
            (cy - 14.0).max(0.0),
            28.0,
            28.0_f32.min(h),
        );
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
        if !state.notif_queue.is_empty() {
            let q_lbl = format!("+{}", state.notif_queue.len());
            let q_w = (p.measure(&q_lbl, 9.6, Weight::Bold) + 14.0).clamp(26.0, 44.0);
            let q_x = (right_edge - q_w - 4.0).max(120.0);
            let q_y = (cy - 11.0).max(4.0);
            let q_ht = p.hot_t(q_x, q_y, q_w, 22.0);
            p.rrect(
                q_x,
                q_y,
                q_w,
                22.0,
                11.0,
                lerp_color(rgba(124, 58, 237, 68), rgba(139, 92, 246, 125), q_ht),
            );
            p.rrect_bevel_stroke(
                q_x,
                q_y,
                q_w,
                22.0,
                11.0,
                0.9,
                rgba(221, 214, 254, 130),
                rgba(168, 85, 247, 80),
            );
            p.text_center_fit(
                &q_lbl,
                q_x + q_w * 0.5,
                q_y + 14.5 - 0.3 * q_ht,
                9.6,
                Weight::Bold,
                tok::cyno_lavender_white(),
                q_w - 6.0,
            );
            p.hit(q_x, q_y, q_w, 22.0, Action::ClearNotificationQueue);
            right_edge = q_x - 6.0;
        }

        let tx = 60.0;
        let max_tw = (right_edge - tx).max(100.0);
        let app_tag_owned;
        let app_tag = if notif.grouped_count > 1 && !notif.app_name.trim().is_empty() {
            app_tag_owned = format!("{} • +{}", notif.app_name.trim(), notif.grouped_count - 1);
            app_tag_owned.as_str()
        } else {
            notif.app_name.trim()
        };
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
            let tag_txt = p.ellipsize_cow(
                app_tag,
                badge_raw_sz,
                Weight::Bold,
                (max_tw * 0.42).max(58.0),
            );
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

        let title_str = p.ellipsize_cow(&notif.title, title_raw_sz, Weight::Bold, title_w);
        p.text(
            &title_str,
            title_x,
            row1_y,
            title_raw_sz,
            Weight::Bold,
            tok::text1(),
        );

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

        let body_action = match notif.kind {
            NotificationKind::SystemToast => Action::ActivateNotification,
            NotificationKind::Capture if notif.is_success => Action::OpenCaptureFolder,
            _ => Action::DismissNotification,
        };
        p.hit(0.0, 0.0, w, h, body_action);
    }

    pub(super) fn draw_expanded_toast_overlay(p: &mut PainterD2D, state: &AppState, w: f32) {
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

        p.rrect_vertical_gradient(
            bx,
            by,
            bw,
            bh,
            12.5,
            rgba(24, 26, 42, 248),
            rgba(14, 15, 26, 250),
        );
        p.rrect_bevel_stroke(
            bx,
            by,
            bw,
            bh,
            12.5,
            1.0,
            rgba(235, 230, 255, 65),
            rgba(192, 132, 252, 65),
        );
        p.radial_glow(
            bx + 22.0,
            cy,
            34.0,
            fade(tok::cyno_violet(), 0.28),
            BlendMode::SourceOver,
            true,
        );

        let ic_cx = bx + 20.0;
        p.rrect_vertical_gradient(
            ic_cx - 12.5,
            cy - 12.5,
            25.0,
            25.0,
            12.5,
            rgba(38, 40, 62, 246),
            rgba(16, 17, 30, 250),
        );
        p.circle_stroke(ic_cx, cy, 12.5, 0.9, rgba(235, 230, 255, 72));
        icons::draw(
            p,
            app_icon,
            ic_cx - 6.5,
            cy - 6.5,
            13.0,
            tok::cyno_lavender_white(),
        );

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
        p.hit(dis_cx - 14.0, by, 28.0, bh, Action::DismissNotification);

        let right_edge = dis_cx - 16.0;

        let tx = bx + 42.0;
        let max_tw = (right_edge - tx).max(70.0);
        let mut title_x = tx;
        let mut title_w = max_tw;
        let app_tag = notif.app_name.trim();
        if !app_tag.is_empty() {
            let tag = p.ellipsize_cow(app_tag, 9.4, Weight::Bold, (max_tw * 0.36).max(48.0));
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
        let ttl = p.ellipsize_cow(&notif.title, 12.4, Weight::Bold, title_w);
        p.text(&ttl, title_x, by + 19.0, 12.4, Weight::Bold, tok::text1());

        let sub = notif.subtitle.trim();
        if !sub.is_empty() {
            let sub_line = p.ellipsize_cow(sub, 11.4, Weight::Regular, max_tw);
            p.text(
                &sub_line,
                tx,
                by + 37.5,
                11.4,
                Weight::Regular,
                rgba(232, 227, 252, 245),
            );
        }

        let body_action = match notif.kind {
            NotificationKind::SystemToast => Action::ActivateNotification,
            NotificationKind::Capture if notif.is_success => Action::OpenCaptureFolder,
            _ => Action::DismissNotification,
        };
        p.hit(bx, by, bw, bh, body_action);
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
        let mx = 16.0;
        let cw = (w - mx * 2.0).max(160.0);

        if state.notif_history.is_empty() {
            let empty_h = (h - card_y - 16.0).clamp(110.0, 220.0);
            p.rrect_vertical_gradient(
                mx,
                card_y,
                cw,
                empty_h,
                14.0,
                rgba(24, 26, 42, 210),
                rgba(14, 15, 26, 225),
            );
            p.rrect_bevel_stroke(
                mx,
                card_y,
                cw,
                empty_h,
                14.0,
                0.9,
                rgba(235, 230, 255, 36),
                rgba(168, 85, 247, 36),
            );
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
            return;
        }

        // Top action row: summary + Clear All pill
        let count_lbl = format!("{} recent", state.notif_history.len());
        p.text(
            &count_lbl,
            mx + 2.0,
            card_y + 13.0,
            11.0,
            Weight::Semibold,
            tok::text2(),
        );

        let clr_w = 66.0;
        let clr_x = mx + cw - clr_w;
        let clr_y = card_y - 2.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, 22.0);
        p.tactile_surface_pill_t(
            clr_x,
            clr_y,
            clr_w,
            22.0,
            11.0,
            clr_ht,
            Some(tok::cyno_violet()),
        );
        p.text_center_fit(
            "Clear all",
            clr_x + clr_w * 0.5,
            clr_y + 14.5 - 0.3 * clr_ht,
            10.2,
            Weight::Bold,
            tok::text1(),
            clr_w - 8.0,
        );
        p.hit(clr_x, clr_y, clr_w, 22.0, Action::ClearNotifHistory);

        let list_y = card_y + 26.0;
        let avail_h = (h - list_y - 12.0).max(52.0);
        let row_h = 50.0;
        let gap = 6.0;
        let max_rows = ((avail_h + gap) / (row_h + gap)).floor().max(1.0) as usize;

        for (idx, entry) in state.notif_history.iter().take(max_rows).enumerate() {
            let ry = list_y + (idx as f32) * (row_h + gap);
            if ry + row_h > h - 6.0 {
                break;
            }
            let row_ht = p.hot_t(mx, ry, cw, row_h);
            p.rrect_vertical_gradient(
                mx,
                ry,
                cw,
                row_h,
                12.0,
                lerp_color(rgba(26, 28, 44, 210), rgba(36, 38, 60, 235), row_ht),
                lerp_color(rgba(16, 17, 29, 220), rgba(22, 23, 39, 240), row_ht),
            );
            p.rrect_bevel_stroke(
                mx,
                ry,
                cw,
                row_h,
                12.0,
                0.9,
                lerp_color(rgba(235, 230, 255, 36), rgba(221, 214, 254, 96), row_ht),
                rgba(168, 85, 247, 36),
            );

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

            // Dismiss button (registered BEFORE row activation hit)
            let dis_cx = mx + cw - 18.0;
            let dis_ht = p.hot_t(dis_cx - 12.0, ry + 6.0, 24.0, row_h - 12.0);
            p.circle(
                dis_cx,
                ic_cy,
                10.0,
                lerp_color(rgba(255, 255, 255, 14), rgba(255, 255, 255, 38), dis_ht),
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
                format!("{} • {}", entry.app_name.trim(), entry.title.trim())
            };
            let hdr_fit = p.ellipsize_cow(&hdr, 11.4, Weight::Bold, title_max_w);
            p.text(&hdr_fit, tx, ry + 20.5, 11.4, Weight::Bold, tok::text1());

            let sub_max_w = (dis_cx - 16.0 - tx).max(60.0);
            let sub_fit = p.ellipsize_cow(entry.subtitle.trim(), 10.6, Weight::Regular, sub_max_w);
            p.text(&sub_fit, tx, ry + 37.5, 10.6, Weight::Regular, tok::text2());

            p.hit(mx, ry, cw, row_h, Action::ActivateHistoryItem(idx));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // ULTRA-COMPACT MINIMIZED STEALTH PILL (86×18 Horizontal, 18×86 Vertical)
    // ─────────────────────────────────────────────────────────────────────────────
}
