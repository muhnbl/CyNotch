use super::*;
use crate::painter_d2d::{PathBuilder, lerp};
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
            tok::gold_deep(),
            tok::gold_champagne(),
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
        // Right Split: 2 Spacious Full-Height Cards (Now Playing + Quick Shelf)
        let bot_y = card_y;
        let bot_h = 154.0;
        let sub_w = (right_w - 12.0) / 2.0;

        // Card Left: Media Overview
        p.elevated_card(
            right_x,
            bot_y,
            sub_w,
            bot_h,
            16.0,
            Some((sub_w - 24.0, 22.0, 68.0, tok::cyno_violet())),
        );
        let media_card_ht = p.hot_t(right_x, bot_y, sub_w - 44.0, bot_h);
        if media_card_ht > 0.02 && p.prepare_rrect_clip(right_x, bot_y, sub_w, bot_h, 16.0) {
            p.elliptical_glow(
                right_x + sub_w * 0.5,
                bot_y + bot_h + 2.0,
                sub_w * 0.52,
                bot_h * 0.58,
                rgba(253, 230, 138, (46.0 * media_card_ht) as u8),
                BlendMode::Plus,
                true,
            );
            p.pop_clip();
        }
        let media_header = if state.media.is_idle() {
            "No Active Media"
        } else if state.media.is_playing {
            "Now Playing"
        } else {
            "Media Paused"
        };
        icons::draw(
            p,
            Icon::Music,
            right_x + 16.0,
            bot_y + 18.0,
            15.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            media_header,
            right_x + 37.0,
            bot_y + 29.0,
            10.8,
            Weight::Semibold,
            tok::text2(),
        );
        let song = p.ellipsize_cow(&state.media.title, 13.5, Weight::Bold, sub_w - 32.0);
        p.text(
            &song,
            right_x + 16.0,
            bot_y + 68.0,
            13.5,
            Weight::Bold,
            tok::text1(),
        );
        let artist_lbl = if state.media.artist.is_empty() {
            "Open Music Player"
        } else {
            state.media.artist.as_str()
        };
        let artist_ell = p.ellipsize_cow(artist_lbl, 10.5, Weight::Regular, sub_w - 32.0);
        p.text(
            &artist_ell,
            right_x + 16.0,
            bot_y + 88.0,
            10.5,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            right_x,
            bot_y,
            sub_w - 48.0,
            bot_h,
            Action::SetMode(NotchMode::Music),
        );

        let mini_play_x = right_x + sub_w - 44.0;
        let mini_play_y = bot_y + bot_h - 44.0;
        let mp_ht = p.hot_t(mini_play_x, mini_play_y, 30.0, 30.0);
        p.tactile_surface_pill_t(
            mini_play_x,
            mini_play_y,
            30.0,
            30.0,
            15.0,
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
            mini_play_x + 9.0,
            mini_play_y + 9.0 - 0.55 * mp_ht,
            12.0,
            tok::text1(),
        );
        p.hit(
            mini_play_x,
            mini_play_y,
            30.0,
            30.0,
            Action::TogglePlayPause,
        );

        // Card Right: Quick Shelf
        let br_x = right_x + sub_w + 12.0;
        p.elevated_card(
            br_x,
            bot_y,
            sub_w,
            bot_h,
            16.0,
            Some((sub_w - 24.0, 22.0, 68.0, tok::gold_mid())),
        );
        let shelf_card_ht = p.hot_t(br_x, bot_y, sub_w, bot_h);
        if shelf_card_ht > 0.02 && p.prepare_rrect_clip(br_x, bot_y, sub_w, bot_h, 16.0) {
            p.elliptical_glow(
                br_x + sub_w * 0.5,
                bot_y + bot_h + 2.0,
                sub_w * 0.52,
                bot_h * 0.58,
                rgba(253, 230, 138, (46.0 * shelf_card_ht) as u8),
                BlendMode::Plus,
                true,
            );
            p.pop_clip();
        }
        icons::draw(
            p,
            Icon::Clipboard,
            br_x + 16.0,
            bot_y + 18.0,
            15.0,
            tok::amber_glow(),
        );
        p.text(
            "Quick Shelf",
            br_x + 37.0,
            bot_y + 29.0,
            10.8,
            Weight::Semibold,
            tok::text2(),
        );
        let mut shelf_num = StackStr::<24>::new();
        let _ = write!(shelf_num, "{} parked", state.shelf_items.len());
        p.text(
            &shelf_num,
            br_x + 16.0,
            bot_y + 68.0,
            15.5,
            Weight::Bold,
            tok::cyno_mist(),
        );
        let shelf_sub = if state.shelf_items.is_empty() {
            "Drop files or Ctrl+C"
        } else {
            "Click to open shelf"
        };
        p.text(
            shelf_sub,
            br_x + 16.0,
            bot_y + 88.0,
            10.5,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            br_x,
            bot_y,
            sub_w,
            bot_h,
            Action::SetMode(NotchMode::Shelf),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 1. MUSIC PLAYER (Aligned 154x154 Art + Real Multi-Line Lyrics View)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_music_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let cx = w * 0.5;

        // 1. BOX 1 (Top Notch Box y=0..136): Centered Large Lyrics + Full-Width Progress Bar
        let ctx_y = card_y - 4.0; // y = 56.0 (right below header dock)
        let ctx_h = 56.0;
        let center_y = ctx_y + ctx_h * 0.5;
        if state.lyrics.is_empty() {
            p.text_center(
                "No synced lyrics found for this track yet.",
                cx,
                center_y + 4.0,
                12.0,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            let base_idx = state.current_lyric_index.unwrap_or(0) as i32;
            let max_idx = (state.lyrics.len() as i32) - 1;
            let center_idx = (base_idx + state.lyrics_offset).clamp(0, max_idx) as usize;
            let dimmed = rgba(199, 194, 223, 102);

            let (act_lines, fit_act_sz) = p.wrap_lines_fit(
                &state.lyrics[center_idx].text,
                17.0,
                12.0,
                Weight::Bold,
                w - 72.0,
                2,
            );
            let line_cnt = act_lines.len().max(1);
            let line_step = fit_act_sz * 1.12;
            let text_block_h = (line_cnt as f32 - 1.0) * line_step;

            p.radial_glow(
                cx,
                center_y,
                (ctx_h * 0.55).max(20.0),
                rgba(139, 92, 246, 26),
                BlendMode::SourceOver,
                true,
            );

            let active_top = center_y - text_block_h * 0.5 - fit_act_sz * 0.45;
            let active_bot = center_y + text_block_h * 0.5 + fit_act_sz * 0.52;

            if center_idx > 0 && line_cnt == 1 {
                let prev_ell = p.ellipsize_cow(
                    &state.lyrics[center_idx - 1].text,
                    10.8,
                    Weight::Regular,
                    w - 80.0,
                );
                p.text_center(
                    &prev_ell,
                    cx,
                    (active_top - 4.0).max(ctx_y + 10.0),
                    10.8,
                    Weight::Regular,
                    dimmed,
                );
                p.hit(24.0, ctx_y, w - 48.0, 18.0, Action::ScrollLyrics(-1));
            }

            let first_base = center_y - text_block_h * 0.5 + fit_act_sz * 0.34;
            for (li, line) in act_lines.iter().enumerate() {
                p.text_center_shadow(
                    line,
                    cx,
                    first_base + (li as f32) * line_step,
                    fit_act_sz,
                    Weight::Bold,
                    tok::text1(),
                );
            }

            if center_idx + 1 < state.lyrics.len() {
                let next_ell = p.ellipsize_cow(
                    &state.lyrics[center_idx + 1].text,
                    10.8,
                    Weight::Regular,
                    w - 80.0,
                );
                p.text_center(
                    &next_ell,
                    cx,
                    (active_bot + 10.0).min(ctx_y + ctx_h - 2.0),
                    10.8,
                    Weight::Regular,
                    dimmed,
                );
                p.hit(
                    24.0,
                    active_bot,
                    w - 48.0,
                    (ctx_y + ctx_h - active_bot).max(16.0),
                    Action::ScrollLyrics(1),
                );
            }
        }

        // Full-Width Progress / Seek Bar at bottom of Box 1 (y=120)
        let played_pct = if state.media.duration_ms > 0 {
            (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let scrub_x = 32.0;
        let scrub_w = (w - 64.0).max(80.0);
        let scrub_y = 118.0;
        p.glowing_striped_bar(
            scrub_x,
            scrub_y,
            scrub_w,
            5.5,
            played_pct,
            tok::gold_deep(),
            tok::gold_champagne(),
        );
        let mini_hot = p.hot(scrub_x - 4.0, scrub_y - 7.0, scrub_w + 8.0, 20.0);
        let thumb_x = (scrub_x + scrub_w * played_pct).clamp(scrub_x + 4.0, scrub_x + scrub_w - 3.0);
        let thumb_y = scrub_y + 2.75;
        let thumb_r = if mini_hot { 5.5 } else { 4.4 };
        p.radial_glow(
            thumb_x,
            thumb_y,
            if mini_hot { 10.0 } else { 8.0 },
            rgba(200, 155, 60, if mini_hot { 125 } else { 80 }),
            BlendMode::Plus,
            false,
        );
        p.circle(thumb_x, thumb_y, thumb_r, rgba(255, 255, 255, 250));
        p.circle(thumb_x, thumb_y, thumb_r * 0.44, tok::amber_glow());
        p.hit(scrub_x, scrub_y - 7.0, scrub_w, 20.0, Action::SeekMusic(played_pct));

        // 2. BOX 2 (Floating Obsidian Glass Track Info y=146..188, 12px transparent gap below Box 1)
        let box2_y = 146.0;
        let box2_h = 42.0;
        let (card_top, card_bot) = if p.frosted {
            (tok::obsidian_shell_frosted(), tok::obsidian_deep_frosted())
        } else {
            (tok::obsidian_shell(), tok::obsidian_deep())
        };
        p.rrect_corners4_vertical_gradient(
            0.0,
            box2_y,
            w,
            box2_h,
            (16.0, 16.0, 16.0, 16.0),
            card_top,
            card_bot,
        );
        if p.prepare_rrect_clip(0.0, box2_y, w, box2_h, 16.0) {
            p.elliptical_glow(
                w * 0.5,
                box2_y + box2_h + 3.0,
                w * 0.5,
                20.0,
                rgba(124, 58, 237, if p.frosted { 30 } else { 48 }),
                BlendMode::Plus,
                true,
            );
            p.pop_clip();
        }

        let art_x = 10.0;
        let art_y = box2_y + 6.0;
        let art_sz = 30.0;
        p.rrect_vertical_gradient(
            art_x,
            art_y,
            art_sz,
            art_sz,
            9.0,
            rgba(88, 48, 168, 130),
            rgba(34, 22, 68, 155),
        );
        icons::draw(p, Icon::Music, art_x + 7.5, art_y + 7.5, 15.0, tok::text1());
        if state.media.is_playing {
            p.circle(art_x + art_sz - 4.0, art_y + 4.0, 2.4, tok::amber_glow());
        }

        let pos_s = state.media.position_ms / 1000;
        let dur_s = state.media.duration_ms / 1000;
        let rem_s = dur_s.saturating_sub(pos_s);
        let mut time_str = StackStr::<32>::new();
        let _ = write!(
            time_str,
            "{}:{:02} / -{}:{:02}",
            pos_s / 60,
            pos_s % 60,
            rem_s / 60,
            rem_s % 60
        );
        p.text_right(
            &time_str,
            w - 14.0,
            box2_y + 24.5,
            10.0,
            Weight::Semibold,
            tok::text3(),
        );

        let text_x = art_x + art_sz + 10.0;
        let text_max_w = (w - text_x - 98.0).max(80.0);
        let title_ell = p.ellipsize_cow(&state.media.title, 12.2, Weight::Bold, text_max_w);
        p.text(&title_ell, text_x, box2_y + 18.5, 12.2, Weight::Bold, tok::text1());
        let artist_str = if state.media.artist.is_empty() {
            "Unknown Artist"
        } else {
            state.media.artist.as_str()
        };
        let artist_ell = p.ellipsize_cow(artist_str, 10.2, Weight::Regular, text_max_w);
        p.text(&artist_ell, text_x, box2_y + 32.5, 10.2, Weight::Regular, tok::text2());

        // 3. BOX 3 (3 Separated Floating Obsidian Glass Buttons y=194..230, 4px safe margin from bottom)
        let btn_y = 194.0;
        let btn_w = 54.0;
        let btn_h = 36.0;
        let btn_gap = 10.0;
        let start_x = cx - (btn_w * 3.0 + btn_gap * 2.0) * 0.5;

        let btns = [
            (start_x, Icon::Prev, Action::PrevTrack, false),
            (
                start_x + btn_w + btn_gap,
                if state.media.is_playing { Icon::Pause } else { Icon::Play },
                Action::TogglePlayPause,
                true,
            ),
            (start_x + (btn_w + btn_gap) * 2.0, Icon::Next, Action::NextTrack, false),
        ];
        for (bx, ic, act, is_primary) in btns {
            let ht = p.hot_t(bx, btn_y, btn_w, btn_h);
            p.rrect_corners4_vertical_gradient(
                bx,
                btn_y,
                btn_w,
                btn_h,
                (13.0, 13.0, 13.0, 13.0),
                card_top,
                card_bot,
            );
            p.button_pill_variant(
                bx,
                btn_y,
                btn_w,
                btn_h,
                13.0,
                ht,
                if is_primary {
                    ButtonVariant::PrimaryFill
                } else {
                    ButtonVariant::PrimaryOutline
                },
            );
            icons::draw(
                p,
                ic,
                bx + (btn_w - 16.0) * 0.5,
                btn_y + (btn_h - 16.0) * 0.5 - 0.5 * ht,
                16.0,
                tok::text1(),
            );
            p.hit(bx, btn_y, btn_w, btn_h, act);
        }
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
                    let dimmed = rgba(199, 194, 223, 102);
                    for (li, line) in prev_lines.iter().enumerate() {
                        let by = last_base - ((p_cnt - 1 - li) as f32) * p_step;
                        p.text_center(
                            line,
                            pad_x + inner_w * 0.5,
                            by,
                            fit_prev_sz,
                            Weight::Regular,
                            dimmed,
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

                // Apple Music Clean Focus: pure typography hierarchy without heavy pill box
                let text_block_h = (line_cnt as f32 - 1.0) * line_step;
                let safe_r = (inner_w * 0.34).min((ctx_h * 0.44).max(12.0));
                p.radial_glow(
                    pad_x + inner_w * 0.5,
                    center_y,
                    safe_r,
                    rgba(139, 92, 246, 26),
                    BlendMode::SourceOver,
                    true,
                );
                let first_base = center_y - text_block_h * 0.5 + fit_act_sz * 0.34;
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
                if state.lyric_style == LyricStyle::KaraokeGlow {
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
                    let bar_w = (inner_w * 0.42).min(140.0);
                    let bar_x = pad_x + (inner_w - bar_w) * 0.5;
                    let bar_y = pill_y + pill_h - 3.0;
                    p.rrect(bar_x, bar_y, bar_w, 2.0, 1.0, rgba(255, 255, 255, 28));
                    p.rrect(bar_x, bar_y, bar_w * karaoke_pct, 2.0, 1.0, tok::cyno_mist());
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
                    let dimmed = rgba(199, 194, 223, 102);
                    for (li, line) in next_lines.iter().enumerate() {
                        let by = first_next_y + (li as f32) * n_step;
                        if by <= ctx_y + ctx_h + 2.0 {
                            p.text_center(
                                line,
                                pad_x + inner_w * 0.5,
                                by,
                                fit_next_sz,
                                Weight::Regular,
                                dimmed,
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
                // BH-42: Clamp glow radius softly within lyrics region instead of pushing a tight
                // inner clip box that creates a visible horizontal seam below the header bar.
                let safe_r = (inner_w * 0.36).min((ctx_h * 0.48).max(12.0));
                p.radial_glow(
                    pad_x + inner_w * 0.5,
                    center_y - 2.0,
                    safe_r,
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
            tok::gold_deep(),
            tok::gold_champagne(),
        );

        let thumb_x = (slider_x + slider_w * pct_clamped)
            .clamp(slider_x + 7.0, slider_x + slider_w - 4.0);
        let thumb_y = slider_y + slider_h / 2.0;
        p.radial_glow(
            thumb_x,
            thumb_y,
            14.0,
            if is_muted {
                rgba(220, 38, 38, 95)
            } else {
                rgba(200, 155, 60, 90)
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
        p.circle(thumb_x, thumb_y, 4.0, tok::amber_glow());

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
            tok::gold_deep(),
            tok::gold_champagne(),
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

        // Card 2: Safe Auto Clean Profile (no manual area grid)
        p.elevated_card(col2_x, row1_y, col_w, card_h, 15.0, None);
        p.text(
            "Safe Trim Profile",
            col2_x + 14.0,
            row1_y + 18.5,
            10.2,
            Weight::Semibold,
            tok::text2(),
        );
        let auto_w = 72.0;
        let auto_x = col2_x + col_w - auto_w - 12.0;
        let auto_y = row1_y + 8.0;
        let auto_h = 20.0;
        let auto_ht = p.hot_t(auto_x, auto_y, auto_w, auto_h);
        p.tactile_surface_pill_t(
            auto_x,
            auto_y,
            auto_w,
            auto_h,
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
            auto_y + 13.2 - 0.45 * auto_ht,
            9.2,
            Weight::Bold,
            if state.mem_reduct.auto_clean_enabled {
                tok::text1()
            } else {
                lerp_color(tok::text2(), tok::text1(), auto_ht)
            },
        );
        p.hit(auto_x, auto_y, auto_w, auto_h, Action::ToggleAutoClean);
        p.text(
            "Working Sets • SysWS • Low Standby",
            col2_x + 14.0,
            row1_y + 42.5,
            10.5,
            Weight::Semibold,
            tok::text1(),
        );
        p.text(
            "Recommended non-stuttering targets",
            col2_x + 14.0,
            row1_y + 59.5,
            9.0,
            Weight::Regular,
            tok::text2(),
        );

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
    pub(super) fn draw_shelf_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let now = std::time::Instant::now();
        let absorb_y = if state.are_animations_active() {
            state.shelf_fx.card_absorb_offset(now)
        } else {
            0.0
        };
        let card_y = card_y + absorb_y;
        let margin_x = 20.0;
        let card_w = w - (margin_x * 2.0);
        let card_h = 154.0;

        p.elevated_card(margin_x, card_y, card_w, card_h, 22.0, None);

        icons::draw(
            p,
            Icon::Clipboard,
            margin_x + 18.0,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Temporary Shelf",
            margin_x + 44.0,
            card_y + 27.5,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

        let total = state.shelf_items.len();
        let start_idx = state.shelf_scroll.min(total.saturating_sub(3));

        let clr_w = 50.0;
        let clr_h = 24.0;
        let clr_x = margin_x + card_w - clr_w - 14.0;
        let clr_y = card_y + 15.0;
        let clr_ht = p.hot_t(clr_x, clr_y, clr_w, clr_h);
        p.button_pill_variant(clr_x, clr_y, clr_w, clr_h, 12.0, clr_ht, ButtonVariant::TextOnly);
        p.text_center(
            "Clear",
            clr_x + clr_w / 2.0,
            clr_y + 15.5,
            10.5,
            Weight::Semibold,
            lerp_color(tok::text2(), tok::text1(), clr_ht),
        );
        p.hit(clr_x, clr_y, clr_w, clr_h, Action::ClearShelf);

        let mut right_bound = clr_x;
        if state.has_active_shelf_undo() {
            let undo_w = 52.0;
            let undo_x = clr_x - undo_w - 6.0;
            let undo_ht = p.hot_t(undo_x, clr_y, undo_w, clr_h);
            p.button_pill_variant(
                undo_x,
                clr_y,
                undo_w,
                clr_h,
                12.0,
                undo_ht,
                ButtonVariant::PrimaryOutline,
            );
            p.text_center(
                "Undo",
                undo_x + undo_w / 2.0,
                clr_y + 15.5,
                10.0,
                Weight::Semibold,
                lerp_color(tok::amber_glow(), tok::text1(), undo_ht),
            );
            p.hit(undo_x, clr_y, undo_w, clr_h, Action::UndoShelfRemove);
            right_bound = undo_x;
        }

        let sub = if total > 3 {
            format!(
                "Drag out to move • {}-{} of {}",
                start_idx + 1,
                (start_idx + 3).min(total),
                total
            )
        } else {
            "Drop anything or Ctrl+C • Drag out or click".to_string()
        };
        let sub_max_w = (right_bound - (margin_x + 44.0) - 10.0).max(60.0);
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
                "Shelf empty • Drop files, images, text or press Ctrl+C",
                margin_x + card_w / 2.0,
                card_y + 98.0,
                11.5,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            let prev_alpha = p.global_alpha;
            let visible: Vec<&crate::models::ShelfItem> = state
                .shelf_items
                .iter()
                .skip(start_idx)
                .take(3)
                .collect();
            let inner_x = margin_x + 14.0;
            let inner_w = card_w - 28.0;
            let gap = 10.0;
            let grid_y = card_y + 52.0;
            let tile_h = 88.0;

            let tile_w = (inner_w - gap * 2.0) / 3.0;
            let mut cur_x = inner_x;
            for (slot, item) in visible.into_iter().enumerate() {
                let (entry_y, entry_a) = if state.are_animations_active() {
                    (
                        state.shelf_fx.buoyancy_offset(item.id, now),
                        state.shelf_fx.row_entry_alpha(item.id, now),
                    )
                } else {
                    (0.0, 1.0)
                };
                p.global_alpha = prev_alpha * entry_a;
                let _ = slot;
                Self::draw_shelf_bento_tile(
                    p,
                    item,
                    cur_x,
                    grid_y + entry_y,
                    tile_w,
                    tile_h,
                    false,
                );
                cur_x += tile_w + gap;
            }
            p.global_alpha = prev_alpha;
        }

        Self::draw_shelf_liquid_fx(p, state, margin_x, card_y, card_w, card_h, 22.0);
    }

    /// Renders an individual Bento Shelf Tile (Hero Image Tile with real decoded WIC/SVG thumbnail,
    /// Multi-line Text Snippet Tile with `...`, Domain/Path URL Tile, or Extension File Tile).
    pub(super) fn draw_shelf_bento_tile(
        p: &mut PainterD2D,
        item: &crate::models::ShelfItem,
        tx: f32,
        ty: f32,
        tw: f32,
        th: f32,
        is_hero: bool,
    ) {
        let del_w = 22.0;
        let del_h = 22.0;
        let act_w = (tw - del_w - 4.0).max(24.0);
        let tile_ht = p.hot_t(tx, ty, act_w, th);
        let dy = -1.2 * tile_ht;
        let r = 13.0;

        p.button_pill_variant(
            tx,
            ty + dy,
            tw,
            th,
            r,
            tile_ht,
            ButtonVariant::PrimaryOutline,
        );

        let badge = item.badge_label();

        if item.is_image() {
            // 1. Live Image Thumbnail (WIC / SVG) or Procedural Vector Preview Backdrop
            let pad = 2.0;
            let ix = tx + pad;
            let iy = ty + pad + dy;
            let iw = (tw - pad * 2.0).max(8.0);
            let ih = (th - pad * 2.0).max(8.0);
            let ir = (r - 1.5).max(6.0);

            let drew_real = p.draw_shelf_thumbnail(&item.payload, ix, iy, iw, ih, ir);
            if !drew_real && p.prepare_rrect_clip(ix, iy, iw, ih, ir) {
                // Fallback procedural glass artwork preview when path is synthetic/in-memory
                p.rrect_vertical_gradient(
                    ix,
                    iy,
                    iw,
                    ih,
                    ir,
                    rgba(58, 32, 112, 165),
                    rgba(22, 14, 48, 210),
                );
                p.radial_glow(
                    ix + iw * 0.72,
                    iy + ih * 0.32,
                    iw.min(ih) * 0.45,
                    fade(tok::gold_champagne(), 0.42),
                    BlendMode::Plus,
                    true,
                );
                p.circle(
                    ix + iw * 0.74,
                    iy + ih * 0.30,
                    if is_hero { 9.0 } else { 6.5 },
                    fade(tok::gold_champagne(), 0.85),
                );
                // Vector mountain silhouette
                let mut m1 = PathBuilder::new();
                m1.move_to(ix, iy + ih);
                m1.line_to(ix + iw * 0.36, iy + ih * 0.44);
                m1.line_to(ix + iw * 0.68, iy + ih * 0.76);
                m1.line_to(ix + iw * 0.84, iy + ih * 0.54);
                m1.line_to(ix + iw, iy + ih * 0.72);
                m1.line_to(ix + iw, iy + ih);
                m1.close();
                if let Some(path) = m1.finish() {
                    p.fill_path(&path, rgba(139, 92, 246, 95));
                }
                p.pop_clip();
            }

            // Bottom dark glass scrim for caption + format pill badge legibility
            if p.prepare_rrect_clip(ix, iy, iw, ih, ir) {
                let scrim_h = if is_hero { 30.0 } else { 26.0 };
                let scrim_y = iy + ih - scrim_h;
                p.rrect_vertical_gradient(
                    ix,
                    scrim_y,
                    iw,
                    scrim_h,
                    0.0,
                    rgba(10, 8, 22, 20),
                    rgba(10, 8, 22, 218),
                );
                p.pop_clip();
            }

            // Format Badge Pill (PNG / JPG / SVG) in Brushed Metallic Gold
            let bw = (p.measure(&badge, 8.2, Weight::Bold) + 12.0).clamp(28.0, 44.0);
            let bx = tx + 8.0;
            let by = ty + th - 22.0 + dy;
            p.rrect(bx, by, bw, 15.0, 5.0, rgba(20, 14, 38, 210));
            p.rrect_stroke(bx, by, bw, 15.0, 5.0, 0.8, fade(tok::gold_champagne(), 0.75));
            p.text_center(
                &badge,
                bx + bw * 0.5,
                by + 10.6,
                8.0,
                Weight::Bold,
                tok::gold_champagne(),
            );

            // Filename caption with `...` truncation
            let cap_x = bx + bw + 6.0;
            let cap_max_w = (tw - (cap_x - tx) - 8.0).max(28.0);
            let short_lbl = p.ellipsize_cow(&item.label, 9.6, Weight::Semibold, cap_max_w);
            p.text_shadow(
                &short_lbl,
                cap_x,
                by + 11.2,
                9.6,
                Weight::Semibold,
                tok::text1(),
            );
        } else {
            // Top micro-header: Icon + Format Badge Pill
            let (ic, ic_col, badge_col) = match item.kind {
                ShelfKind::Text => (Icon::Clipboard, tok::cyno_violet_hi(), tok::text2()),
                ShelfKind::Url => (Icon::AppBrowser, tok::amber_glow(), tok::gold_champagne()),
                ShelfKind::File | ShelfKind::Image => (Icon::File, tok::cyno_mist(), tok::cyno_violet_hi()),
            };
            icons::draw(p, ic, tx + 10.0, ty + 9.0 + dy, 12.0, ic_col);

            let bw = (p.measure(&badge, 8.0, Weight::Bold) + 10.0).clamp(28.0, 46.0);
            let bx = tx + 26.0;
            let by = ty + 7.5 + dy;
            p.rrect(bx, by, bw, 14.5, 4.5, rgba(255, 255, 255, 14));
            p.text_center(&badge, bx + bw * 0.5, by + 10.2, 7.8, Weight::Bold, badge_col);

            let content_x = tx + 10.0;
            let content_w = (tw - 20.0).max(32.0);

            match item.kind {
                ShelfKind::Text => {
                    // Multi-line text snippet preview with "..." truncation
                    let clean: String = item
                        .payload
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let src_txt = if clean.is_empty() {
                        item.label.trim()
                    } else {
                        clean.as_str()
                    };
                    let max_lines = if th >= 82.0 { 3 } else { 2 };
                    let font_sz = 9.6;
                    let line_step = 13.0;
                    let raw_lines = p.wrap_lines(src_txt, font_sz, Weight::Regular, content_w);
                    let start_y = ty + 36.5 + dy;

                    for i in 0..raw_lines.len().min(max_lines) {
                        let is_last = i + 1 == max_lines && raw_lines.len() > max_lines;
                        let line_str = if is_last {
                            let with_dots = format!("{}...", raw_lines[i].trim_end_matches('.'));
                            p.ellipsize_cow(&with_dots, font_sz, Weight::Regular, content_w)
                                .into_owned()
                        } else if p.measure(&raw_lines[i], font_sz, Weight::Regular) > content_w {
                            p.ellipsize_cow(&raw_lines[i], font_sz, Weight::Regular, content_w)
                                .into_owned()
                        } else {
                            raw_lines[i].clone()
                        };
                        p.text(
                            &line_str,
                            content_x,
                            start_y + (i as f32) * line_step,
                            font_sz,
                            if i == 0 { Weight::Semibold } else { Weight::Regular },
                            if i == 0 { tok::text1() } else { tok::text2() },
                        );
                    }
                }
                ShelfKind::Url => {
                    let (host, path_part) = item.url_host_and_path();
                    let host_ell = p.ellipsize_cow(&host, 10.4, Weight::Bold, content_w);
                    p.text(
                        &host_ell,
                        content_x,
                        ty + 39.0 + dy,
                        10.4,
                        Weight::Bold,
                        tok::text1(),
                    );
                    let path_ell = p.ellipsize_cow(&path_part, 9.2, Weight::Regular, content_w);
                    p.text(
                        &path_ell,
                        content_x,
                        ty + 54.0 + dy,
                        9.2,
                        Weight::Regular,
                        tok::text2(),
                    );
                }
                ShelfKind::File | ShelfKind::Image => {
                    let max_lines = if th >= 82.0 { 2 } else { 1 };
                    let raw_lines = p.wrap_lines(&item.label, 10.0, Weight::Semibold, content_w);
                    let start_y = ty + 40.0 + dy;
                    for i in 0..raw_lines.len().min(max_lines) {
                        let is_last = i + 1 == max_lines && raw_lines.len() > max_lines;
                        let line_str = if is_last {
                            let with_dots = format!("{}...", raw_lines[i]);
                            p.ellipsize_cow(&with_dots, 10.0, Weight::Semibold, content_w)
                                .into_owned()
                        } else {
                            p.ellipsize_cow(&raw_lines[i], 10.0, Weight::Semibold, content_w)
                                .into_owned()
                        };
                        p.text(
                            &line_str,
                            content_x,
                            start_y + (i as f32) * 13.5,
                            10.0,
                            Weight::Semibold,
                            tok::text1(),
                        );
                    }
                }
            }
        }

        // Non-overlapping hitboxes: Activate on tile body, Delete on top-right `×` pill
        p.hit(
            tx,
            ty,
            act_w,
            th,
            Action::ActivateShelfItem(item.id),
        );

        let del_x = tx + tw - del_w - 3.0;
        let del_y = ty + 3.0;
        let del_ht = p.hot_t(del_x, del_y, del_w, del_h);
        if item.is_image() {
            p.rrect(
                del_x,
                del_y + dy,
                del_w,
                del_h,
                11.0,
                rgba(12, 10, 26, lerp(145.0, 215.0, del_ht) as u8),
            );
        } else {
            p.button_pill_variant(
                del_x,
                del_y + dy,
                del_w,
                del_h,
                11.0,
                del_ht,
                ButtonVariant::TextOnly,
            );
        }
        icons::draw(
            p,
            Icon::Close,
            del_x + 6.0,
            del_y + 6.0 + dy,
            10.0,
            lerp_color(tok::text2(), tok::text1(), del_ht),
        );
        p.hit(
            del_x,
            del_y,
            del_w + 3.0,
            del_h + 4.0,
            Action::DeleteShelfItem(item.id),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // 8. QUICK CAPTURE VIEW (Lazyshot-style Instant Desktop/Window PNG + Clipboard)
    // ─────────────────────────────────────────────────────────────────────────────
    pub(super) fn draw_shelf_liquid_fx(
        p: &mut PainterD2D,
        state: &AppState,
        card_x: f32,
        card_y: f32,
        card_w: f32,
        card_h: f32,
        radius: f32,
    ) {
        let now = std::time::Instant::now();
        let hover_t = state.shelf_fx.drag_hover_t;
        let plop_t = if state.are_animations_active() {
            state
                .shelf_fx
                .plop_started_at
                .map(|s| now.saturating_duration_since(s).as_secs_f32())
                .filter(|&t| t < crate::models::LIQUID_PLOP_DURATION_SECS)
        } else {
            None
        };
        if hover_t <= 0.005 && plop_t.is_none() {
            return;
        }

        let clipped = p.prepare_rrect_mask(card_x, card_y, card_w, card_h, radius);

        // 1. Drag-over subtle cursor radial lens (MOTION_SYSTEM.md §2.2: rgba(216,180,254,0.14))
        if hover_t > 0.005 {
            let cx = state
                .shelf_fx
                .drag_cursor_smooth
                .0
                .clamp(card_x + 16.0, card_x + card_w - 16.0);
            let cy = state
                .shelf_fx
                .drag_cursor_smooth
                .1
                .clamp(card_y + 16.0, card_y + card_h - 16.0);
            p.radial_glow(
                cx,
                cy,
                84.0,
                fade(rgba(216, 180, 254, 36), hover_t),
                BlendMode::Plus,
                false,
            );
            if hover_t > 0.25 {
                let badge_w = 112.0;
                let badge_h = 22.0;
                let bx = card_x + (card_w - badge_w) * 0.5;
                let by = card_y + card_h - badge_h - 10.0;
                p.button_pill_variant(
                    bx,
                    by,
                    badge_w,
                    badge_h,
                    11.0,
                    hover_t * 0.4,
                    ButtonVariant::PrimaryOutline,
                );
                icons::draw(
                    p,
                    Icon::Droplet,
                    bx + 10.0,
                    by + 5.0,
                    11.5,
                    fade(tok::amber_glow(), hover_t),
                );
                p.text(
                    "Drop to Shelf",
                    bx + 26.0,
                    by + 14.5,
                    10.0,
                    Weight::Semibold,
                    fade(tok::text1(), hover_t),
                );
            }
        }

        // 2. Raycast Tactile drop impact: single crisp 380ms glass refraction hairline ring
        //    (r: 6px -> 84px, ease-out cubic, alpha -> 0; zero sparks, zero arcs, zero particles)
        if let Some(t) = plop_t {
            let ox = state
                .shelf_fx
                .plop_origin
                .0
                .clamp(card_x + 20.0, card_x + card_w - 20.0);
            let oy = state
                .shelf_fx
                .plop_origin
                .1
                .clamp(card_y + 20.0, card_y + card_h - 20.0);
            let u = (t / crate::models::LIQUID_PLOP_DURATION_SECS).clamp(0.0, 1.0);
            let ease = crate::models::ease_out_cubic(u);
            let radius_px = 6.0 + ease * 78.0;
            let alpha = (1.0 - u).powf(1.6);
            let stroke_w = (1.25 - u * 0.65).max(0.55);
            if alpha > 0.01 {
                p.circle_stroke(
                    ox,
                    oy,
                    radius_px,
                    stroke_w,
                    fade(rgba(233, 213, 255, 165), alpha),
                );
            }
        }

        if clipped {
            p.pop_clip();
        }
    }

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
