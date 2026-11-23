use super::*;
use crate::painter_d2d::{PathBuilder, lerp};
use std::fmt::Write as _;

// 4-Point Spacing Scale (UX Planet 4-pt system: consistent 4px multiples & Law of Proximity)
const SP_2: f32 = 8.0;
const SP_4: f32 = 16.0;
const SP_6: f32 = 24.0;

impl Renderer {
    pub(super) fn draw_home_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 24.0;
        let card_h = 200.0;
        let left_w = 200.0;
        let gap = 16.0;
        let right_x = margin_x + left_w + gap;
        let right_w = w - (margin_x * 2.0) - left_w - gap;
        let prev_alpha = p.global_alpha;

        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        p.global_alpha = prev_alpha * a0;
        let c0_y = card_y + dy0;
        p.push_scale_about(margin_x + left_w * 0.5, c0_y + card_h * 0.5, sc0);

        p.elevated_card(
            margin_x,
            c0_y,
            left_w,
            card_h,
            16.0,
            Some((40.0, 32.0, 72.0, tok::cyno_violet())),
        );

        if !state.show_mascot {
            let emb_cx = margin_x + 30.0;
            let emb_cy = c0_y + 31.0;
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
            c0_y + 29.0,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        let mut pwr_str = StackStr::<48>::new();
        match (state.telemetry.battery_pct, state.telemetry.on_ac) {
            (Some(pct), true) => {
                let _ = write!(pwr_str, "CPU {:.0}%  -  {}% AC", state.telemetry.cpu_usage, pct);
            }
            (Some(pct), false) => {
                let _ = write!(pwr_str, "CPU {:.0}%  -  {}% Bat", state.telemetry.cpu_usage, pct);
            }
            (None, _) => {
                let _ = write!(pwr_str, "CPU {:.0}%  -  AC Power", state.telemetry.cpu_usage);
            }
        }
        p.text(
            &pwr_str,
            margin_x + 56.0,
            c0_y + 46.0,
            10.2,
            Weight::Regular,
            tok::text2(),
        );

        let mut ram_label = StackStr::<24>::new();
        let _ = write!(ram_label, "RAM {:.0}%", state.telemetry.memory_percent);
        p.text(
            &ram_label,
            margin_x + 16.0,
            c0_y + 82.0,
            11.0,
            Weight::Semibold,
            tok::text_soft(),
        );
        let mut used_lbl = StackStr::<24>::new();
        let _ = write!(used_lbl, "{:.1}G", state.telemetry.memory_used_gb);
        p.text_right(
            &used_lbl,
            margin_x + left_w - 16.0,
            c0_y + 82.0,
            11.0,
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
            c0_y + 94.0,
            left_w - 32.0,
            8.0,
            ram_pct,
            tok::gold_deep(),
            tok::gold_champagne(),
        );

        let purge_x = margin_x + 16.0;
        let purge_y = c0_y + 146.0;
        let purge_w = left_w - 32.0;
        let purge_h = 36.0;
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
            purge_y + 22.0 - 0.8 * purge_ht,
            11.5,
            Weight::Bold,
            tok::text1(),
        );
        p.hit(purge_x, purge_y, purge_w, purge_h, Action::PurgeRam);
        p.pop_scale_transform();

        // Right Split: 2 Spacious Full-Height Cards (Now Playing + Quick Shelf)
        let bot_h = 200.0;
        let sub_w = (right_w - 16.0) / 2.0;

        // Card Left: Media Overview
        let (dy1, a1, sc1) = Self::mode_stagger(state, 1);
        p.global_alpha = prev_alpha * a1;
        let bot_y1 = card_y + dy1;
        p.push_scale_about(right_x + sub_w * 0.5, bot_y1 + bot_h * 0.5, sc1);
        p.elevated_card(
            right_x,
            bot_y1,
            sub_w,
            bot_h,
            16.0,
            Some((sub_w - 24.0, 22.0, 68.0, tok::cyno_violet())),
        );
        let media_card_ht = p.hot_t(right_x, bot_y1, sub_w - 44.0, bot_h);
        if media_card_ht > 0.02 && p.prepare_rrect_clip(right_x, bot_y1, sub_w, bot_h, 16.0) {
            p.elliptical_glow(
                right_x + sub_w * 0.5,
                bot_y1 + bot_h + 2.0,
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
            bot_y1 + 18.0,
            15.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            media_header,
            right_x + 37.0,
            bot_y1 + 29.0,
            10.8,
            Weight::Semibold,
            tok::text2(),
        );
        let song = p.ellipsize_cow(&state.media.title, 14.0, Weight::Bold, sub_w - 32.0);
        p.text(
            &song,
            right_x + 16.0,
            bot_y1 + 78.0,
            14.0,
            Weight::Bold,
            tok::text1(),
        );
        let artist_lbl = if state.media.artist.is_empty() {
            "Open Music Player"
        } else {
            state.media.artist.as_str()
        };
        let artist_ell = p.ellipsize_cow(artist_lbl, 11.0, Weight::Regular, sub_w - 32.0);
        p.text(
            &artist_ell,
            right_x + 16.0,
            bot_y1 + 100.0,
            11.0,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            right_x,
            bot_y1,
            sub_w - 48.0,
            bot_h,
            Action::SetMode(NotchMode::Music),
        );

        let mini_play_x = right_x + sub_w - 44.0;
        let mini_play_y = bot_y1 + bot_h - 44.0;
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
        p.pop_scale_transform();

        // Card Right: Quick Shelf
        let (dy2, a2, sc2) = Self::mode_stagger(state, 2);
        p.global_alpha = prev_alpha * a2;
        let bot_y2 = card_y + dy2;
        let br_x = right_x + sub_w + 16.0;
        p.push_scale_about(br_x + sub_w * 0.5, bot_y2 + bot_h * 0.5, sc2);
        p.elevated_card(
            br_x,
            bot_y2,
            sub_w,
            bot_h,
            16.0,
            Some((sub_w - 24.0, 22.0, 68.0, tok::gold_mid())),
        );
        let shelf_card_ht = p.hot_t(br_x, bot_y2, sub_w, bot_h);
        if shelf_card_ht > 0.02 && p.prepare_rrect_clip(br_x, bot_y2, sub_w, bot_h, 16.0) {
            p.elliptical_glow(
                br_x + sub_w * 0.5,
                bot_y2 + bot_h + 2.0,
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
            bot_y2 + 18.0,
            15.0,
            tok::amber_glow(),
        );
        p.text(
            "Quick Shelf",
            br_x + 37.0,
            bot_y2 + 29.0,
            10.8,
            Weight::Semibold,
            tok::text2(),
        );
        let mut shelf_num = StackStr::<24>::new();
        let _ = write!(shelf_num, "{} parked", state.shelf_items.len());
        p.text(
            &shelf_num,
            br_x + 16.0,
            bot_y2 + 78.0,
            16.0,
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
            bot_y2 + 100.0,
            11.0,
            Weight::Regular,
            tok::text2(),
        );
        p.hit(
            br_x,
            bot_y2,
            sub_w,
            bot_h,
            Action::SetMode(NotchMode::Shelf),
        );
        p.pop_scale_transform();
        p.global_alpha = prev_alpha;
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // 1. MUSIC PLAYER (Aligned 154x154 Art + Real Multi-Line Lyrics View)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    pub(super) fn draw_music_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let cx = w * 0.5;
        let prev_alpha = p.global_alpha;

        // 1. BOX 1 (Top Notch Box y=0..164): Centered Large Lyrics + Full-Width Progress Bar
        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        p.global_alpha = prev_alpha * a0;
        let ctx_y = card_y + dy0; // y = 68.0 (18px breathing room below dock)
        let ctx_h = 72.0; // ends y = 140.0 (8px above seek bar at 148.0)
        p.push_scale_about(cx, ctx_y + 44.0, sc0);
        let center_y = ctx_y + ctx_h * 0.5;
        if state.lyrics.is_empty() {
            p.text_center(
                "No synced lyrics found for this track yet.",
                cx,
                center_y + 4.0,
                12.5,
                Weight::Regular,
                tok::text2(),
            );
        } else {
            let base_idx = state.current_lyric_index.unwrap_or(0) as i32;
            let max_idx = (state.lyrics.len() as i32) - 1;
            let center_idx = (base_idx + state.lyrics_offset).clamp(0, max_idx) as usize;
            let dimmed = rgba(199, 194, 223, 108);

            let (act_lines, fit_act_sz) = p.wrap_lines_fit(
                &state.lyrics[center_idx].text,
                17.0,
                12.5,
                Weight::Bold,
                w - 80.0,
                2,
            );
            let line_cnt = act_lines.len().max(1);
            let line_step = fit_act_sz * 1.24;
            let text_block_h = (line_cnt as f32 - 1.0) * line_step;

            p.radial_glow(
                cx,
                center_y,
                (ctx_h * 0.52).max(24.0),
                rgba(139, 92, 246, 26),
                BlendMode::SourceOver,
                true,
            );

            let active_top = center_y - text_block_h * 0.5 - fit_act_sz * 0.45;
            let active_bot = center_y + text_block_h * 0.5 + fit_act_sz * 0.55;

            if center_idx > 0 && line_cnt == 1 {
                let prev_ell = p.ellipsize_cow(
                    &state.lyrics[center_idx - 1].text,
                    11.2,
                    Weight::Regular,
                    w - 96.0,
                );
                p.text_center(
                    &prev_ell,
                    cx,
                    (active_top - 12.0).max(ctx_y + 10.0),
                    11.2,
                    Weight::Regular,
                    dimmed,
                );
                p.hit(24.0, ctx_y, w - 48.0, 22.0, Action::ScrollLyrics(-1));
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
                    11.2,
                    Weight::Regular,
                    w - 96.0,
                );
                p.text_center(
                    &next_ell,
                    cx,
                    (active_bot + 16.0).min(ctx_y + ctx_h - 2.0),
                    11.2,
                    Weight::Regular,
                    dimmed,
                );
                p.hit(
                    24.0,
                    active_bot,
                    w - 48.0,
                    (ctx_y + ctx_h - active_bot).max(18.0),
                    Action::ScrollLyrics(1),
                );
            }
        }

        // Full-Width Progress / Seek Bar at bottom of Box 1 (y=148)
        let played_pct = if state.media.duration_ms > 0 {
            (state.media.position_ms as f32 / state.media.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let scrub_x = 32.0;
        let scrub_w = (w - 64.0).max(80.0);
        let scrub_y = 148.0 + dy0;
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
        p.pop_scale_transform();

        // 2. BOX 2 (Floating Obsidian Glass Track Info y=176..242, h=66px, 12px transparent gap below Box 1)
        // Outer shell stays at 176..242 to match BlurSpec mask; inner content cascades with Blur-Fade Rise.
        let (dy1, a1, sc1) = Self::mode_stagger(state, 1);
        p.global_alpha = prev_alpha * a1;
        let box2_y = 176.0;
        let box2_h = 66.0;
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
                24.0,
                rgba(124, 58, 237, if p.frosted { 30 } else { 48 }),
                BlendMode::Plus,
                true,
            );
            p.pop_clip();
        }

        // Cover art: Fill Height (box2_h - 16px uniform 8px padding) + Lock 1:1 Aspect Ratio (50x50px)
        let inner_box2_y = box2_y + dy1;
        p.push_scale_about(cx, inner_box2_y + box2_h * 0.5, sc1);
        let art_pad = SP_2;
        let art_sz = box2_h - art_pad * 2.0; // 50.0 x 50.0 (1:1 locked)
        let art_x = art_pad;
        let art_y = inner_box2_y + art_pad;
        let drew_cover = state
            .media
            .cover_art
            .as_ref()
            .is_some_and(|cov| p.draw_media_cover(cov, art_x, art_y, art_sz, art_sz, 10.0));
        if !drew_cover {
            p.rrect_vertical_gradient(
                art_x,
                art_y,
                art_sz,
                art_sz,
                10.0,
                rgba(88, 48, 168, 130),
                rgba(34, 22, 68, 155),
            );
            icons::draw(
                p,
                Icon::Music,
                art_x + (art_sz - 20.0) * 0.5,
                art_y + (art_sz - 20.0) * 0.5,
                20.0,
                tok::text1(),
            );
        }
        if state.media.is_playing {
            p.circle(art_x + art_sz - 5.0, art_y + 5.0, 2.8, tok::amber_glow());
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
            w - SP_4,
            inner_box2_y + box2_h * 0.5 + 4.0,
            11.0,
            Weight::Semibold,
            tok::text3(),
        );

        let text_x = art_x + art_sz + SP_4;
        let text_max_w = (w - text_x - 112.0).max(80.0);
        let title_ell = p.ellipsize_cow(&state.media.title, 14.0, Weight::Bold, text_max_w);
        p.text(&title_ell, text_x, inner_box2_y + 28.0, 14.0, Weight::Bold, tok::text1());
        let artist_str = if state.media.artist.is_empty() {
            "Unknown Artist"
        } else {
            state.media.artist.as_str()
        };
        let artist_ell = p.ellipsize_cow(artist_str, 11.5, Weight::Regular, text_max_w);
        p.text(&artist_ell, text_x, inner_box2_y + 48.0, 11.5, Weight::Regular, tok::text2());
        p.pop_scale_transform();

        // 3. BOX 3 (3 Separated Floating Obsidian Glass Buttons y=250..286, 6px safe margin from bottom)
        // Outer button shells stay at 250..286 to match BlurSpec & >=3px bottom margin; pills & icons cascade.
        let btn_y = 250.0;
        let btn_w = 56.0;
        let btn_h = 36.0;
        let btn_gap = 12.0;
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
        for (bi, (bx, ic, act, is_primary)) in btns.into_iter().enumerate() {
            let (dy_b, ab, sc_b) = Self::mode_stagger(state, 2 + bi);
            p.global_alpha = prev_alpha * ab;
            p.push_scale_about(bx + btn_w * 0.5, btn_y + btn_h * 0.5, sc_b);
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
                btn_y + (btn_h - 16.0) * 0.5 - 0.5 * ht + dy_b * 0.45,
                16.0,
                tok::text1(),
            );
            p.hit(bx, btn_y, btn_w, btn_h, act);
            p.pop_scale_transform();
        }
        p.global_alpha = prev_alpha;
    }

    pub(super) fn draw_volume_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 24.0;
        let card_h = 200.0;
        let left_w = 152.0;
        let gap = 16.0;
        let right_w = w - (margin_x * 2.0) - left_w - gap;
        let right_x = margin_x + left_w + gap;
        let prev_alpha = p.global_alpha;

        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        p.global_alpha = prev_alpha * a0;
        let c0_y = card_y + dy0;
        p.push_scale_about(margin_x + left_w * 0.5, c0_y + card_h * 0.5, sc0);

        let vol_int = (state.volume_pct * 100.0).round() as i32;
        let is_muted = vol_int == 0;

        let aurora_col = if is_muted {
            rgba(239, 68, 68, 120)
        } else {
            tok::cyno_violet()
        };
        p.elevated_card(
            margin_x,
            c0_y,
            left_w,
            card_h,
            16.0,
            Some((left_w * 0.5, 34.0, 58.0, aurora_col)),
        );

        let ic_cx = margin_x + left_w / 2.0;
        let ic_cy = c0_y + 40.0;
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
            c0_y + 94.0,
            24.0,
            Weight::Bold,
            tok::text1(),
        );

        let mute_btn_w = 108.0;
        let mute_btn_h = 32.0;
        let mute_btn_x = margin_x + (left_w - mute_btn_w) / 2.0;
        let mute_btn_y = c0_y + 132.0;
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
        p.pop_scale_transform();

        // Right Card
        let (dy1, a1, sc1) = Self::mode_stagger(state, 1);
        p.global_alpha = prev_alpha * a1;
        let c1_y = card_y + dy1;
        p.push_scale_about(right_x + right_w * 0.5, c1_y + card_h * 0.5, sc1);
        p.elevated_card(
            right_x,
            c1_y,
            right_w,
            card_h,
            16.0,
            Some((right_w - 30.0, 24.0, 88.0, tok::cyno_orchid())),
        );
        p.dot_matrix(
            right_x + right_w - 50.0,
            c1_y + 14.0,
            6,
            3,
            5.5,
            rgba(255, 255, 255, 32),
        );

        p.text(
            "System Volume Master",
            right_x + 18.0,
            c1_y + 29.0,
            14.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Windows Core Audio  -  Drag slider or scroll wheel",
            right_x + 18.0,
            c1_y + 45.0,
            10.5,
            Weight::Regular,
            tok::text2(),
        );

        let slider_x = AppState::VOLUME_SLIDER_X;
        let slider_y = c1_y + 78.0;
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
        let pb_y = c1_y + 122.0;
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
        p.pop_scale_transform();
        p.global_alpha = prev_alpha;
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // 3. MEMORY REDUCT VIEW (Full 8-Area Parity + Pool Metrics + Disable Triggers)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    pub(super) fn draw_memreduct_view(p: &mut PainterD2D, state: &AppState, w: f32, card_y: f32) {
        let smooth = state.micro_interactions;
        let margin_x = 24.0;
        let grid_w = w - (margin_x * 2.0);
        let col_gap = 16.0;
        let row_gap = 16.0;
        let col_w = (grid_w - col_gap) / 2.0;
        let card_h = 92.0;

        let col1_x = margin_x;
        let col2_x = margin_x + col_w + col_gap;
        let row1_y = card_y;
        let row2_y = row1_y + card_h + row_gap;
        let prev_alpha = p.global_alpha;

        // Card 1: Physical + Pagefile + SysCache + Elevation badge
        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        p.global_alpha = prev_alpha * a0;
        let r1_c0_y = row1_y + dy0;
        p.push_scale_about(col1_x + col_w * 0.5, r1_c0_y + card_h * 0.5, sc0);
        p.elevated_card(
            col1_x,
            r1_c0_y,
            col_w,
            card_h,
            15.0,
            Some((col_w - 20.0, 15.0, 62.0, tok::cyno_violet())),
        );
        p.text(
            "Physical Memory",
            col1_x + 16.0,
            r1_c0_y + 20.0,
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
            col1_x + col_w - 16.0,
            r1_c0_y + 20.0,
            10.0,
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
            col1_x + 16.0,
            r1_c0_y + 40.0,
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
            col1_x + 16.0,
            r1_c0_y + 50.0,
            col_w - 32.0,
            6.5,
            ram_pct,
            tok::gold_deep(),
            tok::gold_champagne(),
        );

        let pools_str = format!(
            "Page {:.0}%  -  SysCache {:.0} MB",
            state.telemetry.pagefile_percent, state.telemetry.sys_cache_mb
        );
        let pools_ell = p.ellipsize_cow(&pools_str, 10.0, Weight::Regular, col_w - 32.0);
        p.text(
            &pools_ell,
            col1_x + 16.0,
            r1_c0_y + 71.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );
        p.pop_scale_transform();

        // Card 2: Safe Trim Profile (centered/balanced text, no Auto button)
        let (dy1, a1, sc1) = Self::mode_stagger(state, 1);
        p.global_alpha = prev_alpha * a1;
        let r1_c1_y = row1_y + dy1;
        p.push_scale_about(col2_x + col_w * 0.5, r1_c1_y + card_h * 0.5, sc1);
        p.elevated_card(col2_x, r1_c1_y, col_w, card_h, 15.0, None);
        p.text_center(
            "Safe Trim Profile",
            col2_x + col_w * 0.5,
            r1_c1_y + 32.0,
            12.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text_center_fit(
            "Frees unused RAM safely  -  keeps apps & tabs intact",
            col2_x + col_w * 0.5,
            r1_c1_y + 56.0,
            10.2,
            Weight::Regular,
            tok::text2(),
            col_w - 28.0,
        );
        p.pop_scale_transform();

        // Card 3: Rules (Independent Threshold & Interval triggers with Disabled option)
        let (dy2, a2, sc2) = Self::mode_stagger(state, 2);
        p.global_alpha = prev_alpha * a2;
        let r2_c0_y = row2_y + dy2;
        p.push_scale_about(col1_x + col_w * 0.5, r2_c0_y + card_h * 0.5, sc2);
        let chip_y = r2_c0_y + 34.0;
        let chip_h = 34.0;
        let chip_w = (col_w - 32.0 - 8.0) / 2.0;
        let chip1_x = col1_x + 16.0;
        let chip2_x = chip1_x + chip_w + 8.0;

        p.elevated_card(col1_x, r2_c0_y, col_w, card_h, 15.0, None);
        p.text(
            "Clean Above",
            chip1_x,
            r2_c0_y + 22.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );
        p.text(
            "Clean Every",
            chip2_x,
            r2_c0_y + 22.0,
            10.5,
            Weight::Semibold,
            tok::text2(),
        );

        let c1_ht = p.hot_t(col1_x, r2_c0_y, col_w / 2.0, card_h);
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
        p.hit(col1_x, r2_c0_y, col_w / 2.0, card_h, Action::CycleThreshold);

        let c2_ht = p.hot_t(col1_x + col_w / 2.0, r2_c0_y, col_w / 2.0, card_h);
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
            r2_c0_y,
            col_w / 2.0,
            card_h,
            Action::CycleInterval,
        );
        p.pop_scale_transform();

        // Card 4: Clean Memory CTA with Live Status
        let (dy3, a3, sc3) = Self::mode_stagger(state, 3);
        p.global_alpha = prev_alpha * a3;
        let r2_c1_y = row2_y + dy3;
        p.push_scale_about(col2_x + col_w * 0.5, r2_c1_y + card_h * 0.5, sc3);
        let purge_ht = p.hot_t(col2_x, r2_c1_y, col_w, card_h);
        p.luminous_pill_button_t(
            col2_x,
            r2_c1_y,
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
            r2_c1_y + card_h / 2.0 + dy,
            16.0,
            rgba(12, 7, 26, 165),
        );
        p.circle_stroke(
            col2_x + 30.0,
            r2_c1_y + card_h / 2.0 + dy,
            16.0,
            1.0,
            rgba(255, 255, 255, 130),
        );
        if state.show_mascot && state.mascot_mood() == MascotMood::Purging {
            let anim_t = state.anim_time;
            Self::draw_cyno_mascot(
                p,
                col2_x + 30.0,
                r2_c1_y + card_h / 2.0 + dy + 0.5,
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
                r2_c1_y + card_h / 2.0 - 7.5 + dy,
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
            col2_x + 56.0,
            r2_c1_y + 36.0 + dy,
            14.0,
            Weight::Bold,
            tok::text1(),
        );
        let status_sub = p.ellipsize_cow(
            &state.mem_reduct.last_cleaned_status,
            10.2,
            Weight::Semibold,
            col_w - 72.0,
        );
        p.text_shadow(
            &status_sub,
            col2_x + 56.0,
            r2_c1_y + 55.0 + dy,
            10.2,
            Weight::Semibold,
            rgba(241, 245, 249, 245),
        );
        p.hit(col2_x, r2_c1_y, col_w, card_h, Action::PurgeRam);
        p.pop_scale_transform();
        p.global_alpha = prev_alpha;
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // 4. TEMPORARY SHELF VIEW (4-Col Hug-Contents Bento Grid)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    pub(super) fn draw_shelf_view(
        p: &mut PainterD2D,
        state: &AppState,
        w: f32,
        live_h: f32,
        card_y: f32,
    ) {
        let now = std::time::Instant::now();
        // Appear transition for the Shelf card. Deliberately alpha-floored at
        // 0.9: the old full 0->1 fade (mode_stagger) re-ran on hover/mode
        // changes and read as a fast flicker of "Temporary Shelf". A gentle
        // rise + scale with a near-opaque alpha gives a visible appear without
        // ever blanking the header.
        let appear_t = if state.are_animations_active() {
            state.anim_mode_t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let appear_e = 1.0 - (1.0 - appear_t).powi(3);
        let card_dy = (1.0 - appear_e) * 8.0;
        let card_alpha = 0.9 + 0.1 * appear_e;
        let card_sc = 0.985 + 0.015 * appear_e;
        let prev_alpha = p.global_alpha;
        p.global_alpha = prev_alpha * card_alpha;

        const COLS: usize = 4;
        let total = state.shelf_items.len();
        let rows = total.div_ceil(COLS).clamp(1, 6);
        let tile_h = 104.0;
        let gap = SP_4;

        let card_y = card_y + card_dy;
        let margin_x = SP_6;
        let card_w = w - (margin_x * 2.0);
        let full_card_h = 200.0 + ((rows - 1) as f32) * (tile_h + gap);
        // Track live shell height smoothly so Shelf extends/retracts downward without popping dock/header.
        let card_h = (live_h - card_y - 24.0).max(200.0);

        p.push_scale_about(margin_x + card_w * 0.5, card_y + card_h * 0.5, card_sc);
        p.elevated_card(margin_x, card_y, card_w, card_h, 22.0, None);

        icons::draw(
            p,
            Icon::Clipboard,
            margin_x + SP_4,
            card_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Temporary Shelf",
            margin_x + 42.0,
            card_y + 27.5,
            13.5,
            Weight::Bold,
            tok::text1(),
        );

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

        let sub = if total > 0 {
            format!("{} / 24 parked  -  Click or drag out", total)
        } else {
            "Drop anything or Ctrl+C  -  Click or drag out".to_string()
        };
        let sub_max_w = (right_bound - (margin_x + 42.0) - 10.0).max(60.0);
        let sub_ell = p.ellipsize_cow(&sub, 10.0, Weight::Regular, sub_max_w);
        p.text(
            &sub_ell,
            margin_x + 42.0,
            card_y + 42.5,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        if state.shelf_items.is_empty() {
            p.text_center(
                "Shelf empty  -  Drop files, images, text or press Ctrl+C",
                margin_x + card_w / 2.0,
                card_y + 122.0,
                12.0,
                Weight::Regular,
                tok::text2(),
            );
            p.pop_scale_transform();
        } else {
            p.pop_scale_transform();
            let start_x = margin_x + SP_4;
            let inner_w = card_w - SP_4 * 2.0;
            let tile_w = (inner_w - gap * ((COLS - 1) as f32)) / (COLS as f32);
            let grid_y = card_y + 64.0;
            let clip_grid = card_h < full_card_h - 1.0;
            if clip_grid {
                p.push_rect_clip(
                    margin_x + 4.0,
                    card_y + 54.0,
                    card_w - 8.0,
                    (card_h - 62.0).max(8.0),
                );
            }

            for (idx, item) in state.shelf_items.iter().take(24).enumerate() {
                let col = idx % COLS;
                let row = idx / COLS;
                // Only the newly dropped item animates in. Existing tiles stay
                // fully opaque so adding a 5th item just extends the grid down.
                let (fx_y, fx_a) = if state.are_animations_active() {
                    (
                        state.shelf_fx.buoyancy_offset(item.id, now),
                        state.shelf_fx.row_entry_alpha(item.id, now),
                    )
                } else {
                    (0.0, 1.0)
                };
                let tile_sc = (0.96 + 0.04 * fx_a) * (0.985 + 0.015 * appear_e);
                p.global_alpha = prev_alpha * fx_a * (0.9 + 0.1 * appear_e);
                let tx = start_x + (col as f32) * (tile_w + gap);
                let ty = grid_y + (row as f32) * (tile_h + gap) + fx_y
                    + (1.0 - appear_e) * 10.0;
                p.push_scale_about(tx + tile_w * 0.5, ty + tile_h * 0.5, tile_sc);
                Self::draw_shelf_bento_tile(p, item, tx, ty, tile_w, tile_h, false);
                p.pop_scale_transform();
            }
            if clip_grid {
                p.pop_rect_clip();
            }
        }

        p.global_alpha = prev_alpha * card_alpha;
        Self::draw_shelf_liquid_fx(p, state, margin_x, card_y, card_w, card_h, 22.0);
        p.global_alpha = prev_alpha;
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

        // Non-overlapping hitboxes: Activate on tile body, Delete on top-right `Ã—` pill
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

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // 8. QUICK CAPTURE VIEW (Lazyshot-style Instant Desktop/Window PNG + Clipboard)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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

        // 1. Drag-over subtle cursor radial lens (MOTION_SYSTEM.md Â§2.2: rgba(216,180,254,0.14))
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
        let margin_x = 24.0;
        let card_h = 200.0;
        let gap = 16.0;
        let card_w = (w - (margin_x * 2.0) - gap) / 2.0;

        let left_x = margin_x;
        let right_x = margin_x + card_w + gap;
        let prev_alpha = p.global_alpha;

        let (dy0, a0, sc0) = Self::mode_stagger(state, 0);
        p.global_alpha = prev_alpha * a0;
        let c0_y = card_y + dy0;
        p.push_scale_about(left_x + card_w * 0.5, c0_y + card_h * 0.5, sc0);

        p.elevated_card(
            left_x,
            c0_y,
            card_w,
            card_h,
            16.0,
            Some((25.0, 20.0, 70.0, tok::cyno_violet())),
        );

        icons::draw(
            p,
            Icon::Settings,
            left_x + 16.0,
            c0_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "System Integration",
            left_x + 42.0,
            c0_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Startup, chime & global hotkeys",
            left_x + 42.0,
            c0_y + 42.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let r1_y = c0_y + 58.0;
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

        let r2_y = c0_y + 88.0;
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

        let r3_y = c0_y + 118.0;
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
            "Ctrl+Alt+C/M/S",
            badge_x + badge_w / 2.0,
            r3_y + 14.8,
            9.2,
            Weight::Semibold,
            tok::cyno_violet_hi(),
        );
        p.pop_scale_transform();

        // Right Card
        let (dy1, a1, sc1) = Self::mode_stagger(state, 1);
        p.global_alpha = prev_alpha * a1;
        let c1_y = card_y + dy1;
        p.push_scale_about(right_x + card_w * 0.5, c1_y + card_h * 0.5, sc1);
        p.elevated_card(
            right_x,
            c1_y,
            card_w,
            card_h,
            16.0,
            Some((card_w - 25.0, 20.0, 70.0, tok::cyno_orchid())),
        );

        icons::draw(
            p,
            Icon::Settings,
            right_x + 16.0,
            c1_y + 16.0,
            18.0,
            tok::cyno_violet_hi(),
        );
        p.text(
            "Engine & Lifecycle",
            right_x + 42.0,
            c1_y + 27.5,
            13.0,
            Weight::Bold,
            tok::text1(),
        );
        p.text(
            "Preferences, positioning & exit",
            right_x + 42.0,
            c1_y + 42.0,
            10.0,
            Weight::Regular,
            tok::text2(),
        );

        let rr1_y = c1_y + 58.0;
        let rr2_y = c1_y + 88.0;
        let sw_right_x = right_x + card_w - sw_w - 18.0;
        p.text(
            "Frosted Glass",
            right_x + 18.0,
            rr1_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw3_ht = p.hot_t(right_x + 18.0, rr1_y, row_hit_w, sw_h);
        let st3 = if smooth {
            state.switch_t[ToggleId::FrostedGlass as usize]
        } else if state.frosted_glass_enabled {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw_right_x, rr1_y, sw_w, sw_h, st3, sw3_ht);
        p.hit(right_x + 18.0, rr1_y, row_hit_w, sw_h, Action::ToggleFrostedGlass);

        p.text(
            "Minimize Island",
            right_x + 18.0,
            rr2_y + 15.0,
            11.5,
            Weight::Regular,
            tok::text1(),
        );
        let sw4_ht = p.hot_t(right_x + 18.0, rr2_y, row_hit_w, sw_h);
        let st4 = if smooth {
            state.switch_t[ToggleId::MinimizedIsland as usize]
        } else if state.minimized_island {
            1.0
        } else {
            0.0
        };
        p.draw_toggle_switch(sw_right_x, rr2_y, sw_w, sw_h, st4, sw4_ht);
        p.hit(
            right_x + 18.0,
            rr2_y,
            row_hit_w,
            sw_h,
            Action::ToggleMinimizedIsland,
        );

        let bot_y = c1_y + 114.0;
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
        p.pop_scale_transform();
        p.global_alpha = prev_alpha;
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // VERTICAL EXPANDED SHELL & 10 STACKED CARD VIEWS (316Ã—480 Left/Right edge)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
}
