use super::*;
use std::fmt::Write as _;

impl Renderer {
    pub(super) fn draw_home_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 20.0;
        let card_h = 154.0;
        let left_w = 192.0;
        let gap = 14.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;

        p.elevated_card(
            margin_x,
            card_y,
            left_w,
            card_h,
            16.0,
            Some((40.0, 32.0, 72.0, tok::cyno_violet())),
        );

        if !state.show_mascot {
            let emb_cx = margin_x + 30.0;
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
            margin_x + 56.0,
            card_y + 27.0,
            12.5,
            Weight::Bold,
            tok::text1(),
        );
        let mut pwr_str = StackStr::<48>::new();
        match (state.telemetry.battery_pct, state.telemetry.on_ac) {
            (Some(pct), true) => {
                let _ = write!(pwr_str, "CPU {:.0}% • {}% AC", state.telemetry.cpu_usage, pct);
            }
            (Some(pct), false) => {
                let _ = write!(pwr_str, "CPU {:.0}% • {}% Bat", state.telemetry.cpu_usage, pct);
            }
            (None, _) => {
                let _ = write!(pwr_str, "CPU {:.0}% • AC Power", state.telemetry.cpu_usage);
            }
        }
        p.text(
            &pwr_str,
            margin_x + 56.0,
            card_y + 42.0,
            9.8,
            Weight::Regular,
            tok::text2(),
        );

        let mut ram_label = StackStr::<24>::new();
        let _ = write!(ram_label, "RAM {:.0}%", state.telemetry.memory_percent);
        p.text(
            &ram_label,
            margin_x + 16.0,
            card_y + 70.0,
            10.5,
            Weight::Semibold,
            tok::text_soft(),
        );
        let mut used_lbl = StackStr::<24>::new();
        let _ = write!(used_lbl, "{:.1}G", state.telemetry.memory_used_gb);
        p.text_right(
            &used_lbl,
            margin_x + left_w - 16.0,
            card_y + 70.0,
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
            margin_x + 16.0,
            card_y + 79.0,
            left_w - 32.0,
            7.5,
            ram_pct,
            tok::cyno_deep(),
            tok::cyno_violet_hi(),
        );

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

        // Right Top Card: Live Network & Telemetry
        let top_h = 71.0;
        p.elevated_card(
            right_x,
            card_y,
            right_w,
            top_h,
            15.0,
            Some((right_w - 34.0, 22.0, 74.0, tok::cyan_hi())),
        );
        let net_card_ht = p.hot_t(right_x, card_y, right_w, top_h);
        if net_card_ht > 0.02 {
            p.rrect_bevel_stroke(
                right_x,
                card_y,
                right_w,
                top_h,
                15.0,
                1.0,
                fade(tok::cyan_hi(), 0.38 * net_card_ht),
                fade(tok::cyno_violet(), 0.14 * net_card_ht),
            );
        }
        icons::draw(
            p,
            Icon::Activity,
            right_x + 16.0,
            card_y + 13.0,
            14.5,
            tok::cyan_hi(),
        );
        p.text(
            "Live Network",
            right_x + 35.0,
            card_y + 23.5,
            11.5,
            Weight::Bold,
            tok::text1(),
        );

        // Compact LIVE pill + subtle activity bars on right side of Network card
        let pill_w = 50.0;
        let pill_h = 18.0;
        let pill_x = right_x + right_w - pill_w - 14.0;
        let pill_y = card_y + 11.5;
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

        let bar_base_x = right_x + right_w - 58.0;
        let bar_bot_y = card_y + 54.0;
        let activity = ((state.telemetry.net_down_kbps + state.telemetry.net_up_kbps) / 2000.0)
            .clamp(0.18, 1.0);
        for i in 0..6 {
            let phase = (i as f32) * 0.85 + if state.are_animations_active() { state.anim_time * 2.4 } else { 0.4 };
            let wave = 0.35 + 0.65 * (0.5 + 0.5 * phase.sin());
            let bh = (4.0 + 13.0 * activity * wave).clamp(3.5, 17.0);
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

        let mut net_txt = StackStr::<64>::new();
        let _ = write!(
            net_txt,
            "↓ {}   ↑ {}",
            format_speed_long(state.telemetry.net_down_kbps),
            format_speed_long(state.telemetry.net_up_kbps)
        );
        let net_ell = p.ellipsize_cow(&net_txt, 10.8, Weight::Semibold, right_w - 86.0);
        p.text(
            &net_ell,
            right_x + 16.0,
            card_y + 47.5,
            10.8,
            Weight::Semibold,
            tok::cyan_hi(),
        );
        p.hit(
            right_x,
            card_y,
            right_w,
            top_h,
            Action::SetMode(NotchMode::Telemetry),
        );

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
        p.text(
            media_header,
            right_x + 14.0,
            bot_y + 22.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        let song = p.ellipsize_cow(&state.media.title, 12.0, Weight::Bold, sub_w - 56.0);
        p.text(
            &song,
            right_x + 14.0,
            bot_y + 46.0,
            12.0,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(
            right_x,
            bot_y,
            sub_w - 42.0,
            bot_h,
            Action::SetMode(NotchMode::Music),
        );

        let mini_play_x = right_x + sub_w - 40.0;
        let mini_play_y = bot_y + 21.5;
        let mp_ht = p.hot_t(mini_play_x, mini_play_y, 28.0, 28.0);
        p.tactile_surface_pill_t(
            mini_play_x,
            mini_play_y,
            28.0,
            28.0,
            14.0,
            mp_ht,
            Some(tok::cyno_violet()),
        );
        icons::draw(
            p,
            if state.media.is_playing {
                Icon::Pause
            } else {
                Icon::Play
            },
            mini_play_x + 8.0,
            mini_play_y + 8.0 - 0.55 * mp_ht,
            12.0,
            tok::text1(),
        );
        p.hit(
            mini_play_x,
            mini_play_y,
            28.0,
            28.0,
            Action::TogglePlayPause,
        );

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
        p.text(
            "Tasks Open",
            br_x + 14.0,
            bot_y + 22.0,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        let mut task_num = StackStr::<24>::new();
        let _ = write!(task_num, "{} active", pending);
        p.text(
            &task_num,
            br_x + 14.0,
            bot_y + 46.0,
            13.5,
            Weight::Bold,
            tok::cyno_mist(),
        );
        p.hit(
            br_x,
            bot_y,
            sub_w - 42.0,
            bot_h,
            Action::SetMode(NotchMode::Tasks),
        );

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
    pub(super) fn draw_music_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
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
        p.dot_matrix(
            card_x + 16.0,
            card_y + 16.0,
            4,
            3,
            5.5,
            rgba(255, 255, 255, 55),
        );

        if state.show_mascot && state.media.is_playing {
            let hero_cx = card_x + card_size * 0.5;
            let hero_cy = card_y + 62.0;
            p.circle(hero_cx, hero_cy - 1.5, 41.0, rgba(10, 9, 20, 145));
            p.circle_stroke(hero_cx, hero_cy - 1.5, 41.0, 1.0, rgba(255, 255, 255, 45));
        } else {
            icons::draw(
                p,
                Icon::Music,
                card_x + (card_size - 42.0) / 2.0,
                card_y + 42.0 + 1.5,
                42.0,
                rgba(0, 0, 0, 85),
            );
            icons::draw(
                p,
                Icon::Music,
                card_x + (card_size - 42.0) / 2.0,
                card_y + 42.0,
                42.0,
                rgba(255, 255, 255, 255),
            );
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
        let badge_txt = p.ellipsize_cow(art_badge, 9.5, Weight::Bold, card_size - 28.0);
        p.rrect(
            card_x + 16.0,
            card_y + 116.0,
            card_size - 32.0,
            22.0,
            11.0,
            rgba(10, 8, 22, 150),
        );
        p.text_center(
            &badge_txt,
            card_x + card_size / 2.0,
            card_y + 130.5,
            9.5,
            Weight::Bold,
            tok::text1(),
        );

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
            let mini_y = card_y + 123.0;
            p.glowing_striped_bar(
                pad_x,
                mini_y,
                mini_w,
                6.0,
                played_pct,
                tok::cyno_deep(),
                tok::cyno_violet(),
            );
            let mini_hot = p.hot(pad_x - 4.0, card_y + 115.0, mini_w + 8.0, 22.0);
            let thumb_x = (pad_x + mini_w * played_pct).clamp(pad_x + 4.0, pad_x + mini_w - 3.0);
            let thumb_y = mini_y + 3.0;
            let thumb_r = if mini_hot { 5.8 } else { 4.6 };
            p.radial_glow(
                thumb_x,
                thumb_y,
                if mini_hot { 11.0 } else { 8.5 },
                rgba(233, 213, 255, if mini_hot { 130 } else { 85 }),
                BlendMode::Plus,
                false,
            );
            p.circle(thumb_x, thumb_y, thumb_r, rgba(255, 255, 255, 250));
            p.circle(thumb_x, thumb_y, thumb_r * 0.44, tok::cyno_deep());
            p.hit(
                pad_x,
                card_y + 115.0,
                mini_w,
                22.0,
                Action::SeekMusic(played_pct),
            );
            Self::draw_transport_buttons(
                p,
                state.media.is_playing,
                pad_x + inner_w - 60.0,
                card_y + 112.0,
            );
            return;
        }

        // Standard Now Playing View
        let title_y = card_y + 26.0;
        let title_max_w = if state.equalizer_enabled {
            inner_w - 72.0
        } else {
            inner_w
        };
        let title_text = p.ellipsize_cow(&state.media.title, 15.5, Weight::Bold, title_max_w);
        p.text(
            &title_text,
            pad_x,
            title_y,
            15.5,
            Weight::Bold,
            tok::text1(),
        );

        let artist_y = title_y + 19.0;
        let artist_txt = p.ellipsize_cow(&state.media.artist, 12.0, Weight::Regular, inner_w);
        p.text(
            &artist_txt,
            pad_x,
            artist_y,
            12.0,
            Weight::Regular,
            tok::text2(),
        );

        let prog_y = artist_y + 13.0;
        let played_pct = if state.media.duration_ms > 0 {
            (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.glowing_striped_bar(
            pad_x,
            prog_y,
            inner_w,
            6.5,
            played_pct,
            tok::cyno_deep(),
            tok::cyno_violet(),
        );
        let scrub_hot = p.hot(pad_x - 4.0, prog_y - 6.0, inner_w + 8.0, 19.0);
        let thumb_x = (pad_x + inner_w * played_pct).clamp(pad_x + 5.0, pad_x + inner_w - 3.0);
        let thumb_y = prog_y + 3.25;
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
            19.0,
            Action::SeekMusic(played_pct),
        );

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
        p.text(
            &left_time,
            pad_x,
            time_y,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        p.text_right(
            &right_time,
            pad_x + inner_w,
            time_y,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );

        let lyric_y = time_y + 16.0;
        let (snip_lines, snip_sz) = p.wrap_lines_fit(
            &state.current_lyric_text,
            12.0,
            9.5,
            Weight::Regular,
            inner_w,
            2,
        );
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

    pub(super) fn draw_lyrics_panel(
        p: &mut PainterD2D,
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
        let title_mini = p.ellipsize_cow(&state.media.title, 12.2, Weight::Bold, max_title_w);
        p.text(
            &title_mini,
            pad_x,
            top_y + 13.5,
            12.2,
            Weight::Bold,
            tok::text1(),
        );

        let size_ht = p.hot_t(size_x, top_y, size_w, chip_h);
        p.tactile_surface_pill_t(
            size_x,
            top_y,
            size_w,
            chip_h,
            10.0,
            size_ht,
            Some(tok::cyno_orchid()),
        );
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
        p.tactile_surface_pill_t(
            style_x,
            top_y,
            style_w,
            chip_h,
            10.0,
            style_ht,
            Some(tok::cyno_violet()),
        );
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
                    (line_cnt as f32 * line_step + 13.0).clamp(38.0, 52.0)
                };
                let pill_h = if state.lyric_style == LyricStyle::KaraokeGlow {
                    (base_pill_h + 3.0).min(ctx_h * 0.60)
                } else {
                    base_pill_h.min(ctx_h * 0.58)
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
                    let last_base = (pill_y - 7.0).max(ctx_y + (p_cnt as f32) * p_step - 2.0);
                    for (li, line) in prev_lines.iter().enumerate() {
                        let by = last_base - ((p_cnt - 1 - li) as f32) * p_step;
                        p.text_center(
                            line,
                            pad_x + inner_w * 0.5,
                            by,
                            fit_prev_sz,
                            Weight::Regular,
                            tok::text2(),
                        );
                    }
                    p.hit(
                        pad_x,
                        ctx_y,
                        inner_w,
                        (pill_y - ctx_y).max(16.0),
                        Action::ScrollLyrics(-1),
                    );
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
                    let next_sec = state
                        .lyrics
                        .get(center_idx + 1)
                        .map(|l| l.seconds)
                        .unwrap_or(cur_sec + 4.0);
                    let span = (next_sec - cur_sec).max(1.0);
                    let pos_sec = state.media.position_ms as f64 / 1000.0;
                    let karaoke_pct = if state.lyrics_offset == 0
                        && state.media.duration_ms > 0
                        && pos_sec >= cur_sec
                    {
                        ((pos_sec - cur_sec) / span).clamp(0.14, 1.0) as f32
                    } else {
                        0.64
                    };
                    p.rrect(
                        pad_x + 10.0,
                        pill_y + pill_h - 3.5,
                        inner_w - 20.0,
                        2.0,
                        1.0,
                        rgba(10, 8, 22, 135),
                    );
                    p.rrect(
                        pad_x + 10.0,
                        pill_y + pill_h - 3.5,
                        (inner_w - 20.0) * karaoke_pct,
                        2.0,
                        1.0,
                        tok::cyno_mist(),
                    );

                    let first_base =
                        pill_y + (pill_h - 4.0) * 0.5 - text_block_h * 0.5 + fit_act_sz * 0.32;
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
                    p.tactile_surface_pill(
                        pad_x,
                        pill_y,
                        inner_w,
                        pill_h,
                        9.0,
                        false,
                        Some(tok::cyno_violet()),
                    );
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
                    let first_next_y =
                        (pill_y + pill_h + fit_next_sz + 5.0).min(ctx_y + ctx_h - 2.0);
                    for (li, line) in next_lines.iter().enumerate() {
                        let by = first_next_y + (li as f32) * n_step;
                        if by <= ctx_y + ctx_h + 2.0 {
                            p.text_center(
                                line,
                                pad_x + inner_w * 0.5,
                                by,
                                fit_next_sz,
                                Weight::Regular,
                                tok::text2(),
                            );
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
                p.rrect(
                    pad_x + inner_w * 0.5 - 18.0,
                    bar_y,
                    36.0,
                    2.2,
                    1.1,
                    tok::cyno_mist(),
                );
                let cue = state
                    .lyrics
                    .get(center_idx + 1)
                    .map(|l| l.text.as_str())
                    .unwrap_or("End of lyrics");
                let cue_max_lines = if h_cnt == 1 { 2 } else { 1 };
                let (cue_lines, fit_cue_sz) = p.wrap_lines_fit(
                    cue,
                    cue_sz,
                    9.4,
                    Weight::Regular,
                    inner_w - 16.0,
                    cue_max_lines,
                );
                for (li, line) in cue_lines.iter().enumerate() {
                    let cy = bar_y + 3.0 + fit_cue_sz + (li as f32) * (fit_cue_sz * 1.14);
                    if cy <= ctx_y + ctx_h + 2.0 {
                        p.text_center(
                            line,
                            pad_x + inner_w * 0.5,
                            cy,
                            fit_cue_sz,
                            Weight::Regular,
                            tok::text2(),
                        );
                    }
                }
                p.hit(pad_x, ctx_y, inner_w * 0.5, ctx_h, Action::ScrollLyrics(-1));
                p.hit(
                    pad_x + inner_w * 0.5,
                    ctx_y,
                    inner_w * 0.5,
                    ctx_h,
                    Action::ScrollLyrics(1),
                );
            }
            LyricStyle::CompactStream => {
                let (base_font, act_font): (f32, f32) = match state.lyric_size {
                    LyricSize::Compact => (11.4, 12.4),
                    LyricSize::Standard => (12.6, 13.8),
                    LyricSize::Large => (14.0, 15.2),
                };
                let max_slots = if state.lyric_size == LyricSize::Large && !is_vertical {
                    3
                } else {
                    4
                };
                let start_idx = if state.lyrics.len() <= max_slots {
                    0
                } else {
                    center_idx
                        .saturating_sub(1)
                        .min(state.lyrics.len() - max_slots)
                };
                let mut cur_y = ctx_y;
                for slot in 0..max_slots {
                    let idx = start_idx + slot;
                    if idx >= state.lyrics.len() {
                        break;
                    }
                    let is_cur = idx == center_idx;
                    let target_sz = if is_cur { act_font } else { base_font };
                    let weight = if is_cur {
                        Weight::Bold
                    } else {
                        Weight::Regular
                    };
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
                        p.rrect(
                            pad_x,
                            cur_y,
                            inner_w,
                            item_h - 1.0,
                            6.5,
                            rgba(124, 58, 237, 58),
                        );
                        p.rrect(
                            pad_x + 4.0,
                            cur_y + 3.5,
                            2.8,
                            (item_h - 8.0).max(6.0),
                            1.4,
                            tok::cyno_mist(),
                        );
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
                        p.hit(
                            pad_x,
                            cur_y,
                            inner_w,
                            item_h,
                            Action::ScrollLyrics(idx as i32 - center_idx as i32),
                        );
                    }
                    cur_y += item_h;
                }
            }
        }
    }

    pub(super) fn draw_transport_buttons(
        p: &mut PainterD2D,
        is_playing: bool,
        ctrl_cx: f32,
        ctrl_y: f32,
    ) {
        let prev_x = ctrl_cx - 60.0;
        let prev_ht = p.hot_t(prev_x, ctrl_y, 32.0, 26.0);
        p.tactile_surface_pill_t(prev_x, ctrl_y, 32.0, 26.0, 13.0, prev_ht, None);
        icons::draw(
            p,
            Icon::Prev,
            prev_x + 8.5,
            ctrl_y + 5.5 - 0.55 * prev_ht,
            15.0,
            tok::text1(),
        );
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
        icons::draw(
            p,
            play_icon,
            play_x + 15.5,
            ctrl_y + 5.5 + dy,
            15.0,
            rgba(0, 0, 0, 110),
        );
        icons::draw(
            p,
            play_icon,
            play_x + 15.5,
            ctrl_y + 5.0 + dy,
            15.0,
            rgba(255, 255, 255, 255),
        );
        p.hit(play_x, ctrl_y - 2.0, play_w, 30.0, Action::TogglePlayPause);

        let next_x = ctrl_cx + 28.0;
        let next_ht = p.hot_t(next_x, ctrl_y, 32.0, 26.0);
        p.tactile_surface_pill_t(next_x, ctrl_y, 32.0, 26.0, 13.0, next_ht, None);
        icons::draw(
            p,
            Icon::Next,
            next_x + 8.5,
            ctrl_y + 5.5 - 0.55 * next_ht,
            15.0,
            tok::text1(),
        );
        p.hit(next_x, ctrl_y, 32.0, 26.0, Action::NextTrack);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 2. VOLUME MASTER VIEW
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_volume_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 20.0;
        let card_h = 154.0;
        let left_w = 148.0;
        let gap = 14.0;
        let right_w = w - (margin_x * 2.0) - left_w - gap;
        let right_x = margin_x + left_w + gap;

        let vol_int = (state.volume_pct * 100.0).round() as i32;
        let is_muted = vol_int == 0;

        let aurora_col = if is_muted {
            rgba(239, 68, 68, 120)
        } else {
            tok::cyno_violet()
        };
        p.elevated_card(
            margin_x,
            card_y,
            left_w,
            card_h,
            16.0,
            Some((left_w * 0.5, 34.0, 58.0, aurora_col)),
        );

        let ic_cx = margin_x + left_w / 2.0;
        let ic_cy = card_y + 34.0;
        p.tactile_surface_pill(
            ic_cx - 18.0,
            ic_cy - 18.0,
            36.0,
            36.0,
            18.0,
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

        let pct_str = format!("{}%", vol_int);
        p.text_center(
            &pct_str,
            margin_x + left_w / 2.0,
            card_y + 82.0,
            23.0,
            Weight::Bold,
            tok::text1(),
        );

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
            p.tactile_surface_pill_t(
                mute_btn_x, mute_btn_y, mute_btn_w, mute_btn_h, 14.0, mute_ht, None,
            );
        }
        let mute_label = if is_muted { "Unmute" } else { "Mute" };
        let mute_ty = mute_btn_y + 18.0
            - if is_muted {
                0.8 * mute_ht
            } else {
                0.55 * mute_ht
            };
        p.text_center_shadow(
            mute_label,
            mute_btn_x + mute_btn_w / 2.0,
            mute_ty,
            11.0,
            Weight::Semibold,
            tok::text1(),
        );
        p.hit(
            mute_btn_x,
            mute_btn_y,
            mute_btn_w,
            mute_btn_h,
            Action::ToggleMute,
        );

        // Right Card
        p.elevated_card(
            right_x,
            card_y,
            right_w,
            card_h,
            16.0,
            Some((right_w - 30.0, 24.0, 88.0, tok::cyno_orchid())),
        );
        p.dot_matrix(
            right_x + right_w - 50.0,
            card_y + 14.0,
            6,
            3,
            5.5,
            rgba(255, 255, 255, 32),
        );

        p.text(
            "System Volume Master",
            right_x + 18.0,
            card_y + 29.0,
            14.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Windows Core Audio • Drag slider or scroll wheel",
            right_x + 18.0,
            card_y + 45.0,
            10.5,
            Weight::Regular,
            tok::text2(),
        );

        let slider_x = AppState::VOLUME_SLIDER_X;
        let slider_y = card_y + 66.0;
        let slider_w = AppState::VOLUME_SLIDER_W;
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

        let presets: [(f32, &str); 5] = [
            (0.0, "0%"),
            (0.25, "25%"),
            (0.50, "50%"),
            (0.75, "75%"),
            (1.0, "100%"),
        ];
        let pb_gap = 8.0;
        let pb_w = (slider_w - (pb_gap * 4.0)) / 5.0;
        let pb_y = card_y + 100.0;
        let pb_h = 30.0;

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
                    pb_y + 19.0 - 0.8 * ht,
                    11.0,
                    Weight::Bold,
                    tok::text1(),
                );
            } else {
                p.tactile_surface_pill_t(px, pb_y, pb_w, pb_h, 9.5, ht, None);
                let tc = lerp_color(tok::text_soft(), tok::text1(), ht);
                p.text_center(
                    label,
                    px + pb_w / 2.0,
                    pb_y + 19.0 - 0.55 * ht,
                    11.0,
                    Weight::Semibold,
                    tc,
                );
            }
            p.hit(px, pb_y, pb_w, pb_h, Action::SetVolume(target_pct));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 3. MEMORY REDUCT VIEW (Full 8-Area Parity + Pool Metrics + Disable Triggers)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_memreduct_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
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
        p.elevated_card(
            col1_x,
            row1_y,
            col_w,
            card_h,
            15.0,
            Some((col_w - 20.0, 15.0, 62.0, tok::cyno_violet())),
        );
        p.text(
            "Physical Memory",
            col1_x + 14.0,
            row1_y + 17.5,
            10.2,
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
            col1_x + col_w - 14.0,
            row1_y + 17.5,
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
            col1_x + 14.0,
            row1_y + 36.5,
            14.0,
            Weight::Bold,
            tok::text1(),
        );

        let ram_pct = if smooth {
            state.anim_ram_pct.clamp(0.0, 1.0)
        } else {
            (state.telemetry.memory_percent / 100.0).clamp(0.0, 1.0)
        };
        p.glowing_striped_bar(
            col1_x + 14.0,
            row1_y + 43.5,
            col_w - 28.0,
            6.0,
            ram_pct,
            tok::cyno_deep(),
            tok::cyno_violet_hi(),
        );

        let pools_str = format!(
            "Page {:.0}% • SysCache {:.0} MB",
            state.telemetry.pagefile_percent, state.telemetry.sys_cache_mb
        );
        let pools_ell = p.ellipsize_cow(&pools_str, 9.2, Weight::Regular, col_w - 28.0);
        p.text(
            &pools_ell,
            col1_x + 14.0,
            row1_y + 62.0,
            9.2,
            Weight::Regular,
            tok::text2(),
        );

        // Card 2: All 8 Mem Reduct Clean Areas + Auto Clean Toggle
        p.elevated_card(col2_x, row1_y, col_w, card_h, 15.0, None);

        let auto_w = 68.0;
        let auto_x = col2_x + col_w - auto_w - 12.0;
        let auto_y = row1_y + 6.0;
        let auto_h = 16.0;

        let a_gap_x = 4.5;
        let a_gap_y = 3.5;
        let pill_w = (col_w - 28.0 - a_gap_x * 3.0) / 4.0;
        let pill_h = 16.0;

        let hovered_area = (0..8).find(|&idx| {
            let col_i = idx % 4;
            let row_i = idx / 4;
            let px = col2_x + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = row1_y + 29.0 + (row_i as f32 * (pill_h + a_gap_y));
            p.hot(px, py, pill_w, pill_h)
        });
        let header_max_w = (auto_x - (col2_x + 14.0) - 8.0).max(60.0);
        if let Some(desc) = hovered_area.and_then(crate::memreduct::clean_area_description) {
            let ell = p.ellipsize_cow(desc, 9.2, Weight::Semibold, header_max_w);
            p.text(
                &ell,
                col2_x + 14.0,
                row1_y + 16.0,
                9.2,
                Weight::Semibold,
                tok::cyno_violet_hi(),
            );
        } else {
            p.text(
                "Clean Areas",
                col2_x + 14.0,
                row1_y + 16.0,
                10.0,
                Weight::Semibold,
                tok::text2(),
            );
        }

        let auto_ht = p.hot_t(auto_x, auto_y, auto_w, auto_h);
        p.tactile_surface_pill_t(
            auto_x,
            auto_y,
            auto_w,
            auto_h,
            8.0,
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
            auto_y + 11.0 - 0.45 * auto_ht,
            9.0,
            Weight::Bold,
            if state.mem_reduct.auto_clean_enabled {
                tok::text1()
            } else {
                lerp_color(tok::text2(), tok::text1(), auto_ht)
            },
        );
        p.hit(auto_x, auto_y, auto_w, auto_h, Action::ToggleAutoClean);

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
            let px = col2_x + 14.0 + (col_i as f32 * (pill_w + a_gap_x));
            let py = row1_y + 29.0 + (row_i as f32 * (pill_h + a_gap_y));
            let ht = p.hot_t(px, py, pill_w, pill_h);
            p.tactile_surface_pill_t(
                px,
                py,
                pill_w,
                pill_h,
                6.0,
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
                py + 11.5 - 0.45 * ht,
                8.5,
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

        // Card 3: Rules (Independent Threshold & Interval triggers with Disabled option)
        let chip_y = row2_y + 28.0;
        let chip_h = 30.0;
        let chip_w = (col_w - 28.0 - 8.0) / 2.0;
        let chip1_x = col1_x + 14.0;
        let chip2_x = chip1_x + chip_w + 8.0;

        p.elevated_card(col1_x, row2_y, col_w, card_h, 15.0, None);
        p.text(
            "Clean Above",
            chip1_x,
            row2_y + 19.5,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );
        p.text(
            "Clean Every",
            chip2_x,
            row2_y + 19.5,
            10.0,
            Weight::Semibold,
            tok::text2(),
        );

        let c1_ht = p.hot_t(col1_x, row2_y, col_w / 2.0, card_h);
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
            11.5,
            Weight::Bold,
            th_col,
        );
        p.hit(col1_x, row2_y, col_w / 2.0, card_h, Action::CycleThreshold);

        let c2_ht = p.hot_t(col1_x + col_w / 2.0, row2_y, col_w / 2.0, card_h);
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
            11.5,
            Weight::Bold,
            int_col,
        );
        p.hit(
            col1_x + col_w / 2.0,
            row2_y,
            col_w / 2.0,
            card_h,
            Action::CycleInterval,
        );

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
        p.circle(
            col2_x + 30.0,
            row2_y + card_h / 2.0 + dy,
            16.0,
            rgba(12, 7, 26, 165),
        );
        p.circle_stroke(
            col2_x + 30.0,
            row2_y + card_h / 2.0 + dy,
            16.0,
            1.0,
            rgba(255, 255, 255, 130),
        );
        if state.show_mascot && state.mascot_mood() == MascotMood::Purging {
            let anim_t = state.anim_time;
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
            icons::draw(
                p,
                Icon::Zap,
                col2_x + 22.5,
                row2_y + card_h / 2.0 - 7.5 + dy,
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
            col2_x + 54.0,
            row2_y + 31.0 + dy,
            14.0,
            Weight::Bold,
            tok::text1(),
        );
        let status_sub = p.ellipsize_cow(
            &state.mem_reduct.last_cleaned_status,
            9.8,
            Weight::Semibold,
            col_w - 66.0,
        );
        p.text_shadow(
            &status_sub,
            col2_x + 54.0,
            row2_y + 48.0 + dy,
            9.8,
            Weight::Semibold,
            rgba(241, 245, 249, 245),
        );
        p.hit(col2_x, row2_y, col_w, card_h, Action::PurgeRam);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 4. SCREEN TIME TELEMETRY VIEW (Real Hourly Sparkline + Multi-day Average)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_screentime_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
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
        p.elevated_card(
            col1_x,
            row1_y,
            col_w,
            card_h,
            15.0,
            Some((col_w - 25.0, 15.0, 65.0, tok::cyno_violet())),
        );
        p.dot_matrix(
            col1_x + col_w - 42.0,
            row1_y + 10.0,
            5,
            3,
            5.0,
            rgba(255, 255, 255, 32),
        );
        p.text(
            "Today",
            col1_x + 16.0,
            row1_y + 21.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        let today_h = state.screen_time.today_secs / 3600;
        let today_m = (state.screen_time.today_secs % 3600) / 60;
        let today_str = format!("{} h {} m", today_h, today_m);
        p.text(
            &today_str,
            col1_x + 16.0,
            row1_y + 50.0,
            19.0,
            Weight::Bold,
            tok::text1(),
        );

        // Card 2: Most used + runner-up
        p.elevated_card(
            col2_x,
            row1_y,
            col_w,
            card_h,
            15.0,
            Some((col_w - 25.0, 15.0, 65.0, tok::cyno_orchid())),
        );
        p.text(
            "Most used",
            col2_x + 16.0,
            row1_y + 20.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        if let Some((second_name, second_secs)) = state.screen_time.top_apps.get(1) {
            let mut runner_buf = StackStr::<64>::new();
            let _ = write!(runner_buf, "#2 {} {}m", second_name, second_secs / 60);
            let runner = p.ellipsize_cow(&runner_buf, 9.5, Weight::Regular, col_w - 92.0);
            p.text_right(
                &runner,
                col2_x + col_w - 16.0,
                row1_y + 20.0,
                9.5,
                Weight::Regular,
                tok::text2(),
            );
        }
        let top_name = p.ellipsize_cow(
            &state.screen_time.top_app_name,
            15.0,
            Weight::Bold,
            col_w - 96.0,
        );
        p.text(
            &top_name,
            col2_x + 16.0,
            row1_y + 42.0,
            15.0,
            Weight::Bold,
            tok::cyno_mist(),
        );
        let top_h = state.screen_time.top_app_secs / 3600;
        let top_m = (state.screen_time.top_app_secs % 3600) / 60;
        let top_str = format!("{} h {} m", top_h, top_m);
        p.text_right(
            &top_str,
            col2_x + col_w - 16.0,
            row1_y + 42.0,
            11.5,
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
            col2_x + 16.0,
            row1_y + 53.0,
            col_w - 32.0,
            6.5,
            share,
            tok::cyno_deep(),
            tok::cyno_orchid(),
        );

        // Card 3: 7-day average
        p.elevated_card(col1_x, row2_y, col_w, card_h, 15.0, None);
        p.text(
            "7-day average",
            col1_x + 16.0,
            row2_y + 21.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        let days_lbl = format!("{}d log", state.screen_time.tracked_days.max(1));
        p.text_right(
            &days_lbl,
            col1_x + col_w - 16.0,
            row2_y + 21.0,
            9.5,
            Weight::Semibold,
            tok::text2(),
        );
        let avg_h = state.screen_time.seven_day_avg_secs / 3600;
        let avg_m = (state.screen_time.seven_day_avg_secs % 3600) / 60;
        let avg_str = format!("{} h {} m", avg_h, avg_m);
        p.text(
            &avg_str,
            col1_x + 16.0,
            row2_y + 50.0,
            19.0,
            Weight::Bold,
            tok::cyno_violet_hi(),
        );

        // Card 4: Computed Peak hour + 12-bin 24h Sparkline
        p.elevated_card(
            col2_x,
            row2_y,
            col_w,
            card_h,
            15.0,
            Some((col2_x * 0.1 + 40.0, 42.0, 50.0, tok::cyno_violet())),
        );
        p.text(
            "Peak hour",
            col2_x + 16.0,
            row2_y + 21.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        p.text(
            &state.screen_time.peak_hour_str,
            col2_x + 16.0,
            row2_y + 50.0,
            19.0,
            Weight::Bold,
            tok::text1(),
        );

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
    // 5. TASKS CHECKLIST VIEW (Scrollable + Individual Delete × + From Clip)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_tasks_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
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

        icons::draw(
            p,
            Icon::List,
            margin_x + 18.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Tasks & Todos",
            margin_x + 44.0,
            card_y + 27.5,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

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
        p.text_center(
            "Clear Done",
            clear_x + clear_w / 2.0,
            btn_y + 16.0 - 0.55 * clear_ht,
            10.0,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), clear_ht),
        );
        p.hit(clear_x, btn_y, clear_w, btn_h, Action::ClearCompletedTasks);

        let add_w = 86.0;
        let add_x = clear_x - add_w - 8.0;
        let add_ht = p.hot_t(add_x, btn_y, add_w, btn_h);
        p.tactile_surface_pill_t(
            add_x,
            btn_y,
            add_w,
            btn_h,
            7.5,
            add_ht,
            Some(tok::cyno_violet()),
        );
        p.text_center(
            "+ From Clip",
            add_x + add_w / 2.0,
            btn_y + 16.0 - 0.55 * add_ht,
            10.0,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(add_x, btn_y, add_w, btn_h, Action::PasteAsTask);

        let sub = if total > 3 {
            format!(
                "{} open • {}-{} of {} (scroll)",
                pending,
                start_idx + 1,
                (start_idx + 3).min(total),
                total
            )
        } else {
            format!("{} open • toggle or × remove", pending)
        };
        let sub_max_w = (add_x - (margin_x + 44.0) - 10.0).max(60.0);
        let sub_ell = p.ellipsize_cow(&sub, 10.0, Weight::Regular, sub_max_w);
        p.text(
            &sub_ell,
            margin_x + 44.0,
            card_y + 42.5,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

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
            for (slot, (real_idx, task)) in state
                .tasks
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(3)
                .enumerate()
            {
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
                    p.radial_glow(
                        check_cx,
                        check_cy,
                        10.0,
                        rgba(139, 92, 246, 95),
                        BlendMode::Plus,
                        false,
                    );
                    p.circle(check_cx, check_cy, 6.5, tok::cyno_violet());
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
                    p.circle(check_cx, check_cy, 6.5, well);
                    p.circle_stroke(check_cx, check_cy, 6.5, 1.0, rgba(255, 255, 255, 70));
                }

                let task_text = p.ellipsize_cow(&task.text, 11.5, Weight::Regular, row_w - 44.0);
                let text_color = if task.completed {
                    tok::text3()
                } else {
                    tok::text1()
                };
                p.text(
                    &task_text,
                    item_x + 30.0,
                    item_y + 17.0 + dy,
                    11.5,
                    Weight::Regular,
                    text_color,
                );
                p.hit(item_x, item_y, row_w, item_h, Action::ToggleTask(real_idx));

                // Delete pill button
                let del_x = item_x + row_w + 8.0;
                let del_ht = p.hot_t(del_x, item_y, del_w, item_h);
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 7.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(
                        del_x,
                        item_y - 0.55 * del_ht,
                        del_w,
                        item_h,
                        7.5,
                        fade(rgba(220, 38, 38, 195), del_ht),
                    );
                }
                icons::draw(
                    p,
                    Icon::Close,
                    del_x + 7.0,
                    item_y + 7.0 - 0.55 * del_ht,
                    12.0,
                    lerp_color(tok::text2(), tok::text1(), del_ht),
                );
                p.hit(del_x, item_y, del_w, item_h, Action::DeleteTask(real_idx));
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 7. SHELF (Persistent Clipboard History + Drag-and-Drop File Shelf)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_shelf_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
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

        icons::draw(
            p,
            Icon::Clipboard,
            margin_x + 18.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Clipboard & File Shelf",
            margin_x + 44.0,
            card_y + 27.5,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

        let total = state.shelf_items.len();
        let start_idx = state.shelf_scroll.min(total.saturating_sub(3));

        let clr_w = 64.0;
        let clr_h = 24.0;
        let clr_x = margin_x + card_w - clr_w - 16.0;
        let clr_y = card_y + 15.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, clr_h);
        p.tactile_surface_pill_t(clr_x, clr_y, clr_w, clr_h, 7.5, clr_ht, None);
        p.text_center(
            "Clear",
            clr_x + clr_w / 2.0,
            clr_y + 16.0 - 0.55 * clr_ht,
            10.0,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), clr_ht),
        );
        p.hit(clr_x, clr_y, clr_w, clr_h, Action::ClearShelf);

        let sub = if total > 3 {
            format!(
                "Drop files or Ctrl+C • {}-{} of {}",
                start_idx + 1,
                (start_idx + 3).min(total),
                total
            )
        } else {
            "Drop files or Ctrl+C • Click to copy/open".to_string()
        };
        let sub_max_w = (clr_x - (margin_x + 44.0) - 10.0).max(60.0);
        let sub_ell = p.ellipsize_cow(&sub, 10.0, Weight::Regular, sub_max_w);
        p.text(
            &sub_ell,
            margin_x + 44.0,
            card_y + 42.5,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

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
            for (slot, (real_idx, item)) in state
                .shelf_items
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(3)
                .enumerate()
            {
                let item_y = card_y + 56.0 + (slot as f32 * 30.0);
                let item_h = 26.0;
                let item_x = margin_x + 16.0;
                let del_w = 26.0;
                let row_w = card_w - 32.0 - del_w - 8.0;
                let row_ht = p.hot_t(item_x, item_y, row_w, item_h);

                p.tactile_surface_pill_t(item_x, item_y, row_w, item_h, 7.5, row_ht, None);
                let dy = -0.55 * row_ht;
                if row_ht > 0.02 {
                    p.rrect(
                        item_x,
                        item_y + dy,
                        row_w,
                        item_h,
                        7.5,
                        fade(tok::cyno_violet(), 0.25 * row_ht),
                    );
                }

                let (ic, ic_col) = match item.kind {
                    ShelfKind::File => (Icon::File, tok::cyno_mist()),
                    ShelfKind::Text => (Icon::Clipboard, tok::cyno_violet_hi()),
                };
                icons::draw(p, ic, item_x + 10.0, item_y + 6.5 + dy, 13.0, ic_col);

                let clip_snippet = p.ellipsize_cow(&item.label, 11.0, Weight::Regular, row_w - 44.0);
                p.text(
                    &clip_snippet,
                    item_x + 30.0,
                    item_y + 17.0 + dy,
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
                p.tactile_surface_pill_t(del_x, item_y, del_w, item_h, 7.5, del_ht, None);
                if del_ht > 0.02 {
                    p.rrect(
                        del_x,
                        item_y - 0.55 * del_ht,
                        del_w,
                        item_h,
                        7.5,
                        fade(rgba(220, 38, 38, 195), del_ht),
                    );
                }
                icons::draw(
                    p,
                    Icon::Close,
                    del_x + 7.0,
                    item_y + 7.0 - 0.55 * del_ht,
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

    // ─────────────────────────────────────────────────────────────────────────────
    // 8. QUICK CAPTURE VIEW (Lazyshot-style Instant Desktop/Window PNG + Clipboard)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_capture_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let margin_x = 20.0;
        let card_h = 154.0;
        let gap = 14.0;
        let left_w = 242.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;

        // Left Card: Instant Capture Triggers
        p.elevated_card(
            margin_x,
            card_y,
            left_w,
            card_h,
            16.0,
            Some((left_w * 0.5, 36.0, 75.0, tok::cyno_violet())),
        );
        icons::draw(
            p,
            Icon::Video,
            margin_x + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Screen Snapshot",
            margin_x + 42.0,
            card_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        let cap_sub = p.ellipsize_cow(
            "Saves PNG & copies (Ctrl+Shift+S)",
            9.8,
            Weight::Regular,
            left_w - 32.0,
        );
        p.text(
            &cap_sub,
            margin_x + 16.0,
            card_y + 45.0,
            9.8,
            Weight::Regular,
            tok::text2(),
        );

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
        p.text_center_shadow(
            "Capture Full Screen",
            b1_x + btn_w / 2.0,
            b1_y + 22.0 - 0.8 * b1_ht,
            12.0,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(b1_x, b1_y, btn_w, b1_h, Action::CaptureScreen);

        let b2_y = card_y + 108.0;
        let b2_h = 30.0;
        let b2_ht = p.hot_t(b1_x, b2_y, btn_w, b2_h);
        p.tactile_surface_pill_t(b1_x, b2_y, btn_w, b2_h, 9.5, b2_ht, None);
        p.text_center(
            "Capture Active Window",
            b1_x + btn_w / 2.0,
            b2_y + 19.0 - 0.55 * b2_ht,
            11.0,
            Weight::Semibold,
            lerp_color(tok::text_soft(), tok::text1(), b2_ht),
        );
        p.hit(b1_x, b2_y, btn_w, b2_h, Action::CaptureWindow);

        // Right Card: Latest Capture Status & Folder Launcher
        p.elevated_card(
            right_x,
            card_y,
            right_w,
            card_h,
            16.0,
            Some((right_w - 30.0, 24.0, 75.0, tok::cyno_orchid())),
        );
        p.dot_matrix(
            right_x + right_w - 44.0,
            card_y + 14.0,
            5,
            3,
            5.0,
            rgba(255, 255, 255, 30),
        );
        icons::draw(
            p,
            Icon::Folder,
            right_x + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_mist(),
        );
        p.text(
            "Capture Output",
            right_x + 42.0,
            card_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );

        if let Some(ref _path) = state.capture.last_path {
            let fname = p.ellipsize_cow(
                &state.capture.last_filename,
                11.5,
                Weight::Semibold,
                right_w - 32.0,
            );
            p.text(
                &fname,
                right_x + 16.0,
                card_y + 64.0,
                11.5,
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
                right_x + 16.0,
                card_y + 82.0,
                10.5,
                Weight::Regular,
                tok::cyno_violet_hi(),
            );
        } else {
            p.text(
                "No capture taken in this session",
                right_x + 16.0,
                card_y + 64.0,
                11.5,
                Weight::Semibold,
                tok::text1(),
            );
            p.text(
                "Output: Pictures\\CyNotch\\*.png",
                right_x + 16.0,
                card_y + 82.0,
                10.5,
                Weight::Regular,
                tok::text2(),
            );
        }

        let open_w = right_w - 32.0;
        let open_x = right_x + 16.0;
        let open_y = card_y + 108.0;
        let open_h = 30.0;
        let open_ht = p.hot_t(open_x, open_y, open_w, open_h);
        p.tactile_surface_pill_t(
            open_x,
            open_y,
            open_w,
            open_h,
            9.5,
            open_ht,
            Some(tok::cyno_violet()),
        );
        p.text_center(
            "Open Pictures\\CyNotch",
            open_x + open_w / 2.0,
            open_y + 19.0 - 0.55 * open_ht,
            11.0,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(open_x, open_y, open_w, open_h, Action::OpenCaptureFolder);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 9. INLINE SETTINGS VIEW (Quick Controls + Launcher to Advanced Window)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_settings_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 20.0;
        let card_h = 154.0;
        let gap = 14.0;
        let card_w = (w - (margin_x * 2.0) - gap) / 2.0;

        let left_x = margin_x;
        let right_x = margin_x + card_w + gap;

        p.elevated_card(
            left_x,
            card_y,
            card_w,
            card_h,
            16.0,
            Some((25.0, 20.0, 70.0, tok::cyno_violet())),
        );

        icons::draw(
            p,
            Icon::Settings,
            left_x + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "System Integration",
            left_x + 42.0,
            card_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Startup, chime & global hotkeys",
            left_x + 42.0,
            card_y + 42.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let r1_y = card_y + 58.0;
        let row_hit_w = card_w - 36.0;
        p.text(
            "Launch on Startup",
            left_x + 18.0,
            r1_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw_w = 46.0;
        let sw_h = 22.0;
        let sw1_x = left_x + card_w - sw_w - 18.0;
        let sw1_ht = p.hot_t(left_x + 18.0, r1_y, row_hit_w, sw_h);
        let st1 = if smooth {
            state.switch_t[ToggleId::Autostart as usize]
        } else if state.autostart {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw1_x, r1_y, sw_w, sw_h, st1, sw1_ht);
        p.hit(left_x + 18.0, r1_y, row_hit_w, sw_h, Action::ToggleAutostart);

        let r2_y = card_y + 88.0;
        p.text(
            "Action Audio",
            left_x + 18.0,
            r2_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw2_x = left_x + card_w - sw_w - 18.0;
        let sw2_ht = p.hot_t(left_x + 18.0, r2_y, row_hit_w, sw_h);
        let st2 = if smooth {
            state.switch_t[ToggleId::Chime as usize]
        } else if state.chime_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw2_x, r2_y, sw_w, sw_h, st2, sw2_ht);
        p.hit(left_x + 18.0, r2_y, row_hit_w, sw_h, Action::ToggleChime);

        let r3_y = card_y + 118.0;
        p.text(
            "Hotkeys (C/M/S)",
            left_x + 18.0,
            r3_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let badge_w = 108.0;
        let badge_x = left_x + card_w - badge_w - 18.0;
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

        // Right Card
        p.elevated_card(
            right_x,
            card_y,
            card_w,
            card_h,
            16.0,
            Some((card_w - 25.0, 20.0, 70.0, tok::cyno_orchid())),
        );

        icons::draw(
            p,
            Icon::Settings,
            right_x + 16.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Engine & Lifecycle",
            right_x + 42.0,
            card_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Preferences, positioning & exit",
            right_x + 42.0,
            card_y + 42.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let sw_right_x = right_x + card_w - sw_w - 18.0;
        p.text(
            "Frosted Glass",
            right_x + 18.0,
            r1_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw3_ht = p.hot_t(right_x + 18.0, r1_y, row_hit_w, sw_h);
        let st3 = if smooth {
            state.switch_t[ToggleId::FrostedGlass as usize]
        } else if state.frosted_glass_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw_right_x, r1_y, sw_w, sw_h, st3, sw3_ht);
        p.hit(right_x + 18.0, r1_y, row_hit_w, sw_h, Action::ToggleFrostedGlass);

        p.text(
            "Minimize Island",
            right_x + 18.0,
            r2_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw4_ht = p.hot_t(right_x + 18.0, r2_y, row_hit_w, sw_h);
        let st4 = if smooth {
            state.switch_t[ToggleId::MinimizedIsland as usize]
        } else if state.minimized_island {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw_right_x, r2_y, sw_w, sw_h, st4, sw4_ht);
        p.hit(
            right_x + 18.0,
            r2_y,
            row_hit_w,
            sw_h,
            Action::ToggleMinimizedIsland,
        );

        let bot_y = card_y + 114.0;
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
        p.tactile_surface_pill_t(
            quit_x,
            bot_y,
            quit_w,
            bot_h,
            8.0,
            quit_ht,
            Some(rgba(220, 38, 38, 255)),
        );
        p.text_center_shadow(
            "Exit App",
            quit_x + quit_w / 2.0,
            bot_y + 18.0 - 0.55 * quit_ht,
            10.2,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(quit_x, bot_y, quit_w, bot_h, Action::Quit);
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // VERTICAL EXPANDED SHELL & 10 STACKED CARD VIEWS (316×480 Left/Right edge)
    // ─────────────────────────────────────────────────────────────────────────────
}
