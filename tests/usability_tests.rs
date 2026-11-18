use cynotch::font::FontRenderer;
use cynotch::models::{
    Action, AppState, ExpandedSizePreset, HardwareTelemetry, IdleMetric, LyricSize, LyricStyle,
    MascotMood, MediaTrack, MemReductConfig, NotchMode, NotchPlacement, NotchSizePreset,
    NotchStyle, NotificationKind, ScreenTimeStats, SettingsCategory, ShelfItem, ShelfKind,
    TaskItem, TypeScale, ease_exp,
};
use cynotch::notifications::sanitize_notification_text;
use cynotch::painter::tok;
use cynotch::renderer::Renderer;
use cynotch::screentime::{compute_peak_hour, compute_seven_day_avg, format_hour_label};
use cynotch::settings_window::{
    render_settings_pixmap, render_settings_pixmap_with_meta, settings_dims_for_state,
};
use cynotch::telemetry::{TelemetryCollector, format_speed_long, format_speed_short};
use std::collections::HashMap;
use tiny_skia::{Color, Mask, Pixmap};

// ── WCAG 2.2 Relative Luminance & Contrast Ratio Calculator ──────────────────
fn srgb_to_linear(val: f32) -> f32 {
    if val <= 0.04045 {
        val / 12.92
    } else {
        ((val + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: Color) -> f32 {
    let r = srgb_to_linear(c.red());
    let g = srgb_to_linear(c.green());
    let b = srgb_to_linear(c.blue());
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_ratio(c1: Color, c2: Color) -> f32 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (bright, dark) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (bright + 0.05) / (dark + 0.05)
}

#[test]
fn test_wcag_color_contrast_cyno_theme() {
    let bg = Color::from_rgba8(11, 11, 16, 255);
    let card_bg = Color::from_rgba8(24, 24, 33, 255);

    let text1 = tok::text1();
    let text2 = tok::text2();
    let violet = tok::cyno_violet_hi();
    let gold = tok::cyno_gold_hi();
    let cyan = tok::cyan_hi();

    // 1. Text1 on background (Must be >= 7.0 for AAA)
    let cr_text1 = contrast_ratio(text1, bg);
    assert!(
        cr_text1 >= 7.0,
        "Text1 contrast ratio {:.2} failed AAA (>= 7.0)",
        cr_text1
    );

    // 2. Text2 on card background (Must be >= 4.5 for AA)
    let cr_text2 = contrast_ratio(text2, card_bg);
    assert!(
        cr_text2 >= 4.5,
        "Text2 contrast ratio {:.2} failed AA (>= 4.5)",
        cr_text2
    );

    // 3. Cyno Violet Hi (#C084FC) on background (Must be >= 4.5 for AA)
    let cr_violet = contrast_ratio(violet, bg);
    assert!(
        cr_violet >= 4.5,
        "Cyno Violet contrast ratio {:.2} failed AA (>= 4.5)",
        cr_violet
    );

    // 4. Cyno Gold Hi (#FDE68A) on card background (Must be >= 4.5 for AA)
    let cr_gold = contrast_ratio(gold, card_bg);
    assert!(
        cr_gold >= 4.5,
        "Cyno Gold contrast ratio {:.2} failed AA (>= 4.5)",
        cr_gold
    );

    // 5. Hydration Cyan Hi (#7DD3FC) on card background (Must be >= 4.5 for AA)
    let cr_cyan = contrast_ratio(cyan, card_bg);
    assert!(
        cr_cyan >= 4.5,
        "Cyan Hi contrast ratio {:.2} failed AA (>= 4.5)",
        cr_cyan
    );

    // 6. Luminous CTA Pill gradient stops against White text1() (Must be >= 4.5 across entire gradient)
    for (name, stop_col) in [
        ("cyno_indigo", tok::cyno_indigo()),
        ("cyno_violet", tok::cyno_violet()),
        ("cyno_amber_deep", tok::cyno_amber_deep()),
    ] {
        let cr_btn = contrast_ratio(text1, stop_col);
        assert!(
            cr_btn >= 4.5,
            "Luminous button stop {} failed AA white text contrast ({:.2} < 4.5)",
            name,
            cr_btn
        );
    }
}

#[test]
fn test_interactive_dock_hit_targets() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.sync_preview_settled();

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    let expected_modes = [
        NotchMode::Home,
        NotchMode::Music,
        NotchMode::Volume,
        NotchMode::MemReduct,
        NotchMode::Tasks,
        NotchMode::Shelf,
        NotchMode::Capture,
        NotchMode::Telemetry,
    ];

    for mode in expected_modes {
        let hit = r.hits.iter().find(|h| h.action == Action::SetMode(mode));
        assert!(hit.is_some(), "Dock missing hit target for mode {:?}", mode);
        let h = hit.unwrap();
        assert!(
            h.rect.width() >= 20.0,
            "Hit target too narrow for {:?}: {}",
            mode,
            h.rect.width()
        );
        assert!(
            h.rect.height() >= 20.0,
            "Hit target too short for {:?}: {}",
            mode,
            h.rect.height()
        );
    }

    assert!(
        r.hits.iter().any(|h| h.action == Action::TogglePin),
        "Missing TogglePin hit target"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::OpenSettings),
        "Missing Settings gear hit target"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::StartGripDrag),
        "Missing Grip drag hit target"
    );
}

#[test]
fn test_home_hub_controls_and_live_network_card() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Home;
    state.sync_preview_settled();

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    assert!(
        r.hits.iter().any(|h| h.action == Action::PurgeRam),
        "Home missing PurgeRam hit"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::SetMode(NotchMode::Telemetry)),
        "Home missing Live Network (Telemetry) card hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::TogglePlayPause),
        "Home missing mini play/pause hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::CaptureScreen),
        "Home missing quick capture hit"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::SetMode(NotchMode::Tasks)),
        "Home missing Tasks card hit"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::SetMode(NotchMode::Music)),
        "Home missing Media card hit"
    );
}

#[test]
fn test_media_controls_and_synced_lyrics_mode() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Music;
    state.show_lyrics = false;
    state.sync_preview_settled();

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    assert!(
        r.hits.iter().any(|h| h.action == Action::PrevTrack),
        "Missing PrevTrack hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::TogglePlayPause),
        "Missing TogglePlayPause hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::NextTrack),
        "Missing NextTrack hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleLyrics),
        "Missing ToggleLyrics hit"
    );
    let seek_count = r
        .hits
        .iter()
        .filter(|h| matches!(h.action, Action::SeekMusic(_)))
        .count();
    assert_eq!(
        seek_count, 10,
        "Expected 10 scrubber seek hit segments in Now Playing"
    );

    // Switch to Synced Lyrics multi-line mode
    state.show_lyrics = true;
    state.current_lyric_index = Some(2);
    r.render(&state);
    assert!(
        r.hits.iter().any(|h| h.action == Action::ScrollLyrics(-1)),
        "Missing previous lyric line scroll hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ScrollLyrics(1)),
        "Missing next lyric line scroll hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::TogglePlayPause),
        "Lyrics view missing TogglePlayPause"
    );
    let mini_seek_count = r
        .hits
        .iter()
        .filter(|h| matches!(h.action, Action::SeekMusic(_)))
        .count();
    assert_eq!(
        mini_seek_count, 8,
        "Expected 8 scrubber seek segments in Synced Lyrics mode"
    );
}

#[test]
fn test_memory_reduct_controls_and_cycles() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::MemReduct;
    state.sync_preview_settled();

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    for area_idx in 0u8..8u8 {
        assert!(
            r.hits
                .iter()
                .any(|h| h.action == Action::ToggleArea(area_idx)),
            "Missing ToggleArea({}) hit in MemReduct view",
            area_idx
        );
    }
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleAutoClean),
        "Missing Auto Clean ON/OFF toggle hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::CycleThreshold),
        "Missing CycleThreshold hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::CycleInterval),
        "Missing CycleInterval hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::PurgeRam),
        "Missing PurgeRam hit"
    );

    // Threshold cycle: 70 -> 80 -> 90 -> 0 (Disable) -> 60 -> 70
    state.mem_reduct.when_above_pct = 70;
    for expected in [80, 90, 0, 60, 70] {
        state.cycle_threshold();
        assert_eq!(state.mem_reduct.when_above_pct, expected);
    }

    // Interval cycle: 0 (Disable) -> 5 -> 15 -> 30 -> 60 -> 90 -> 0
    state.mem_reduct.every_mins = 0;
    for expected in [5, 15, 30, 60, 90, 0] {
        state.cycle_interval();
        assert_eq!(state.mem_reduct.every_mins, expected);
    }

    // Independent Disable / Enable rule evaluation
    state.mem_reduct.auto_clean_enabled = true;
    state.telemetry.memory_percent = 92.0;
    state.mem_reduct.when_above_pct = 0;
    state.mem_reduct.every_mins = 0;
    assert!(!state.should_trigger_threshold_clean(3600));
    assert!(!state.should_trigger_interval_clean(3600));

    state.mem_reduct.when_above_pct = 85;
    assert!(!state.should_trigger_threshold_clean(30)); // cooldown < 60s
    assert!(state.should_trigger_threshold_clean(60));
    assert!(!state.should_trigger_interval_clean(3600)); // still disabled independently

    state.mem_reduct.when_above_pct = 0;
    state.mem_reduct.every_mins = 15;
    assert!(!state.should_trigger_threshold_clean(3600));
    assert!(!state.should_trigger_interval_clean(14 * 60));
    assert!(state.should_trigger_interval_clean(15 * 60));

    state.mem_reduct.auto_clean_enabled = false;
    state.mem_reduct.when_above_pct = 60;
    assert!(!state.should_trigger_threshold_clean(3600));
    assert!(!state.should_trigger_interval_clean(3600));
}

#[test]
fn test_tasks_toggle_delete_scroll_and_actions() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Tasks;
    state.sync_preview_settled();
    state.tasks = vec![
        TaskItem {
            text: "Task Alpha".to_string(),
            completed: false,
        },
        TaskItem {
            text: "Task Beta".to_string(),
            completed: true,
        },
        TaskItem {
            text: "Task Gamma".to_string(),
            completed: false,
        },
        TaskItem {
            text: "Task Delta".to_string(),
            completed: false,
        },
    ];
    state.tasks_scroll = 1;

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    // Scrolled window starts at index 1 -> indices 1, 2, 3 must be rendered with Toggle + Delete hits
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleTask(1)),
        "Missing ToggleTask(1) when scrolled"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::DeleteTask(1)),
        "Missing DeleteTask(1) when scrolled"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleTask(3)),
        "Missing ToggleTask(3) when scrolled"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::PasteAsTask),
        "Missing PasteAsTask hit"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::ClearCompletedTasks),
        "Missing ClearCompletedTasks hit"
    );

    // Verify empty JSON vec serialization does not coerce into non-empty
    let empty: Vec<TaskItem> = Vec::new();
    let json = serde_json::to_string(&empty).unwrap();
    let parsed: Vec<TaskItem> = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_empty());
}

#[test]
fn test_shelf_text_files_and_notifications() {
    let mut state = AppState::new_for_preview();
    state.is_hovered = true;
    state.sync_preview_settled();
    state.mode = NotchMode::Shelf;
    state.shelf_items = vec![
        ShelfItem {
            kind: ShelfKind::Text,
            label: "cargo build --release".to_string(),
            payload: "cargo build --release".to_string(),
        },
        ShelfItem {
            kind: ShelfKind::File,
            label: "design.png".to_string(),
            payload: "C:\\temp\\design.png".to_string(),
        },
    ];
    state.shelf_scroll = 0;

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::ActivateShelfItem(0)),
        "Missing ActivateShelfItem(0)"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::DeleteShelfItem(0)),
        "Missing DeleteShelfItem(0)"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::ActivateShelfItem(1)),
        "Missing ActivateShelfItem(1)"
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::DeleteShelfItem(1)),
        "Missing DeleteShelfItem(1)"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ClearShelf),
        "Missing ClearShelf hit"
    );

    // Push dropped file & clipboard
    state.push_dropped_file("C:\\Users\\muhna\\Documents\\spec.pdf".to_string());
    assert_eq!(state.shelf_items[0].kind, ShelfKind::File);
    assert_eq!(state.shelf_items[0].label, "spec.pdf");

    state.push_clipboard("https://iamlayzee.com/ test url".to_string(), "Chrome");
    assert_eq!(state.notification.as_ref().unwrap().title, "Pasted");
}

#[test]
fn test_capture_view_hit_targets() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Capture;
    state.sync_preview_settled();
    state.capture.last_path = Some("C:\\Users\\muhna\\Pictures\\CyNotch\\shot.png".to_string());
    state.capture.last_filename = "shot.png".to_string();
    state.capture.last_dims = "1920 × 1080".to_string();

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    assert!(
        r.hits.iter().any(|h| h.action == Action::CaptureScreen),
        "Missing CaptureScreen hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::CaptureWindow),
        "Missing CaptureWindow hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::OpenCaptureFolder),
        "Missing OpenCaptureFolder hit"
    );
}

#[test]
fn test_screentime_hourly_peak_and_seven_day_avg() {
    assert_eq!(format_hour_label(0), "12 AM");
    assert_eq!(format_hour_label(9), "9 AM");
    assert_eq!(format_hour_label(12), "12 PM");
    assert_eq!(format_hour_label(14), "2 PM");

    let mut hourly = [0u64; 24];
    hourly[10] = 600;
    hourly[19] = 4200;
    let (best_h, best_str) = compute_peak_hour(&hourly, 9);
    assert_eq!(best_h, 19);
    assert_eq!(best_str, "7 PM");

    let mut daily = HashMap::new();
    daily.insert("2026-11-09".to_string(), 3600);
    daily.insert("2026-11-10".to_string(), 7200);
    let (avg, days) = compute_seven_day_avg(&daily, "2026-11-11", 5400);
    assert_eq!(days, 3);
    assert_eq!(avg, (3600 + 7200 + 5400) / 3);
}

#[test]
fn test_smooth_spring_animation_convergence() {
    let mut state = AppState::new_for_preview();
    state.is_pinned = false;
    state.set_hovered(false);
    state.sync_preview_settled();

    state.set_hovered(true);
    assert_eq!(state.target_width, state.expanded_width());
    assert_eq!(state.target_height, state.expanded_height());

    for _ in 0..30 {
        state.update_animation(0.016);
    }

    assert_eq!(state.current_width, state.expanded_width());
    assert_eq!(state.current_height, state.expanded_height());

    state.set_hovered(false);
    assert_eq!(state.target_width, state.idle_width());
    assert_eq!(state.target_height, state.idle_height());

    for _ in 0..30 {
        state.update_animation(0.016);
    }

    assert_eq!(state.current_width, state.idle_width());
    assert_eq!(state.current_height, state.idle_height());
}

#[test]
fn test_volume_controls_hit_targets_and_presets() {
    let mut state = AppState::new_for_preview();
    state.is_hovered = true;
    state.sync_preview_settled();
    state.mode = NotchMode::Volume;

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    let volume_hits: Vec<_> = r
        .hits
        .iter()
        .filter_map(|h| match h.action {
            Action::SetVolume(pct) => Some(pct),
            _ => None,
        })
        .collect();

    assert!(
        volume_hits.len() >= 15,
        "Expected 10 slider segments + 5 presets, got {}",
        volume_hits.len()
    );
    assert!(
        volume_hits.iter().any(|&v| (v - 0.0).abs() < 0.01),
        "Missing Mute 0% preset"
    );
    assert!(
        volume_hits.iter().any(|&v| (v - 0.5).abs() < 0.01),
        "Missing 50% preset"
    );
    assert!(
        volume_hits.iter().any(|&v| (v - 1.0).abs() < 0.01),
        "Missing 100% preset"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleMute),
        "Missing ToggleMute button hit"
    );
}

#[test]
fn test_settings_view_hits_and_center_notch() {
    let mut state = AppState::new_for_preview();
    state.is_hovered = true;
    state.sync_preview_settled();
    state.mode = NotchMode::Settings;
    state.x_offset = 120;

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );
    r.render(&state);

    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleAutostart),
        "Missing ToggleAutostart hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ToggleChime),
        "Missing ToggleChime hit"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::ResetPosition),
        "Missing ResetPosition hit when offset != 0"
    );
    assert!(
        r.hits.iter().any(|h| h.action == Action::Quit),
        "Missing Quit hit target"
    );

    // When already centered, the inline button opens the dedicated Settings window
    state.x_offset = 0;
    r.render(&state);
    assert!(
        r.hits.iter().any(|h| h.action == Action::OpenSettings),
        "Missing OpenSettings hit when centered (x_offset == 0)"
    );
}

#[test]
fn test_all_hit_targets_within_expanded_bounds() {
    let mut state = AppState::new_for_preview();
    state.is_hovered = true;
    state.sync_preview_settled();
    state.x_offset = 40;

    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );

    let cases = [
        (NotchMode::Home, false),
        (NotchMode::Music, false),
        (NotchMode::Music, true),
        (NotchMode::Volume, false),
        (NotchMode::MemReduct, false),
        (NotchMode::Tasks, false),
        (NotchMode::Shelf, false),
        (NotchMode::Capture, false),
        (NotchMode::Telemetry, false),
        (NotchMode::Settings, false),
    ];

    for (mode, show_lyrics) in cases {
        state.mode = mode;
        state.show_lyrics = show_lyrics;
        r.render(&state);

        assert!(
            !r.hits.is_empty(),
            "No hits rendered for {:?} (lyrics={})",
            mode,
            show_lyrics
        );
        for hit in &r.hits {
            assert!(
                hit.rect.left() >= 0.0
                    && hit.rect.top() >= 0.0
                    && hit.rect.right() <= state.expanded_width() + 0.5
                    && hit.rect.bottom() <= state.expanded_height() + 0.5,
                "Out-of-bounds hit {:?} in {:?} (lyrics={}): {:?}",
                hit.action,
                mode,
                show_lyrics,
                hit.rect
            );
        }
    }
}

#[test]
fn test_real_defaults_have_no_dummy_data() {
    let media_idle = MediaTrack::idle();
    assert!(media_idle.is_idle());
    assert!(!media_idle.is_playing);
    assert_eq!(media_idle.position_ms, 0);
    assert_eq!(media_idle.duration_ms, 0);

    let st_default = ScreenTimeStats::default();
    assert_eq!(st_default.today_secs, 0);
    assert_eq!(st_default.top_app_name, "--");
    assert_eq!(st_default.peak_hour_str, "--");
    assert!(st_default.top_apps.is_empty());
    assert!(st_default.hourly_secs.iter().all(|&s| s == 0));

    let mem_default = MemReductConfig::default();
    assert_eq!(mem_default.last_freed_mb, 0.0);
    assert!(!mem_default.is_purging);

    let hw = HardwareTelemetry::initial_real();
    assert!(
        hw.memory_total_gb > 0.5,
        "Expected real physical RAM > 0.5 GB, got {}",
        hw.memory_total_gb
    );

    let (_, empty_peak) = compute_peak_hour(&[0u64; 24], 14);
    assert_eq!(
        empty_peak, "--",
        "All-zero hourly array should report '--', not a guessed hour"
    );

    let mut real_state = AppState::new();
    real_state.persistence_enabled = false;
    assert!(
        real_state.media.is_idle(),
        "Production AppState::new() must start idle with no fake song"
    );
    assert!(!real_state.media.is_playing);
    assert_eq!(real_state.screen_time.today_secs, 0);
    assert_eq!(real_state.screen_time.top_app_name, "--");
    assert!(real_state.telemetry.memory_total_gb > 0.5);
}

#[test]
fn test_adaptive_animation_timer_and_renderer_mask_reuse() {
    let mut state = AppState::new();
    state.persistence_enabled = false;
    state.reset_all_settings();
    state.is_pinned = false;
    state.set_hovered(false);
    state.sync_preview_settled();

    // With mascot hidden -> static idle disables animation timer (0% idle CPU)
    state.show_mascot = false;
    assert_eq!(state.needs_animation_timer_ms(), None);

    // Expanding triggers high-refresh morph timer
    state.set_hovered(true);
    assert_eq!(state.needs_animation_timer_ms(), Some(16));

    // Once settled on static non-mascot expanded card (e.g. Tasks), animation timer turns off again
    state.mode = NotchMode::Tasks;
    state.sync_preview_settled();
    assert_eq!(state.needs_animation_timer_ms(), None);

    // With mascot visible in Idle -> lightweight 28ms timer (~35 FPS smooth idle)
    state.show_mascot = true;
    state.set_hovered(false);
    state.sync_preview_settled();
    assert_eq!(state.needs_animation_timer_ms(), Some(28));

    // Playing media with visible equalizer/mascot requests 20ms smooth timer (50 FPS)
    state.media = MediaTrack::preview_sample();
    state.set_hovered(true);
    state.mode = NotchMode::Music;
    state.show_lyrics = false;
    state.sync_preview_settled();
    assert_eq!(state.needs_animation_timer_ms(), Some(20));

    // Lyrics mode keeps left cover mascot grooving when show_mascot=true, and pauses timer cleanly when show_mascot=false
    state.show_lyrics = true;
    assert_eq!(state.needs_animation_timer_ms(), Some(20));
    state.show_mascot = false;
    assert_eq!(state.needs_animation_timer_ms(), None);
    state.show_mascot = true;

    // Memory purge activates 16ms surge timer
    state.mem_reduct.is_purging = true;
    assert_eq!(state.needs_animation_timer_ms(), Some(16));
    state.mem_reduct.is_purging = false;

    // Idle music renders cleanly and Renderer reuses clip_mask across resizes
    state.media = MediaTrack::idle();
    state.show_lyrics = false;
    let mut r = Renderer::new(AppState::IDLE_WIDTH as u32, AppState::IDLE_HEIGHT as u32);
    state.set_hovered(false);
    state.current_width = AppState::IDLE_WIDTH;
    state.current_height = AppState::IDLE_HEIGHT;
    r.render(&state);

    state.set_hovered(true);
    state.current_width = AppState::EXPANDED_WIDTH;
    state.current_height = AppState::EXPANDED_HEIGHT;
    r.resize(
        AppState::EXPANDED_WIDTH as u32,
        AppState::EXPANDED_HEIGHT as u32,
    );
    r.render(&state);
    assert_eq!(r.pixmap.width(), AppState::EXPANDED_WIDTH as u32);
    assert_eq!(r.pixmap.height(), AppState::EXPANDED_HEIGHT as u32);
}

#[test]
fn test_concurrent_startup_and_win32_safety() {
    let mut handles = Vec::new();
    for _ in 0..6 {
        handles.push(std::thread::spawn(|| {
            for _ in 0..4 {
                let mut s = AppState::new();
                s.persistence_enabled = false;
                if let Some(v) = cynotch::audio::get_master_volume() {
                    assert!((0.0..=1.0).contains(&v));
                }
                assert!(s.telemetry.memory_total_gb > 0.5);
            }
        }));
    }
    for h in handles {
        h.join().expect("Concurrent AppState/Win32 thread panicked");
    }
}

#[test]
fn test_settings_window_all_categories_render_and_bounds() {
    let mut state = AppState::new_for_preview();
    state.settings_open = true;
    let sw = AppState::SETTINGS_WIDTH as u32;
    let sh = AppState::SETTINGS_HEIGHT as u32;
    let mut pm = Pixmap::new(sw, sh).unwrap();
    let mut mask = Mask::new(sw, sh).unwrap();
    let mut fonts = FontRenderer::new();

    for cat in SettingsCategory::ALL {
        state.set_settings_category(cat);
        state.set_settings_scroll(cat, 0.0);
        state.sync_preview_settled();
        let (hits_top, max_scroll) =
            render_settings_pixmap_with_meta(&state, None, &mut pm, &mut mask, &mut fonts);
        let mut combined_actions: Vec<Action> = hits_top.iter().map(|h| h.action).collect();

        if max_scroll > 1.0 {
            state.set_settings_scroll(cat, max_scroll);
            let (hits_bot, _) =
                render_settings_pixmap_with_meta(&state, None, &mut pm, &mut mask, &mut fonts);
            for h in &hits_bot {
                assert!(
                    h.rect.left() >= 0.0
                        && h.rect.top() >= 0.0
                        && h.rect.right() <= AppState::SETTINGS_WIDTH + 0.5
                        && h.rect.bottom() <= AppState::SETTINGS_HEIGHT + 0.5,
                    "Scrolled out-of-bounds hit in settings {:?}: {:?} -> {:?}",
                    cat,
                    h.action,
                    h.rect
                );
                if !combined_actions.contains(&h.action) {
                    combined_actions.push(h.action);
                }
            }
            state.set_settings_scroll(cat, 0.0);
        }

        assert!(
            !hits_top.is_empty(),
            "Expected hit targets in settings category {:?}",
            cat
        );
        assert!(
            hits_top.iter().any(|h| h.action == Action::CloseSettings),
            "Category {:?} missing CloseSettings button",
            cat
        );
        for side_cat in SettingsCategory::ALL {
            assert!(
                hits_top
                    .iter()
                    .any(|h| h.action == Action::SetSettingsCategory(side_cat)),
                "Sidebar missing category {:?} while rendering {:?}",
                side_cat,
                cat
            );
        }

        for h in &hits_top {
            assert!(
                h.rect.left() >= 0.0
                    && h.rect.top() >= 0.0
                    && h.rect.right() <= AppState::SETTINGS_WIDTH + 0.5
                    && h.rect.bottom() <= AppState::SETTINGS_HEIGHT + 0.5,
                "Out-of-bounds hit in settings {:?}: {:?} -> {:?}",
                cat,
                h.action,
                h.rect
            );
        }

        match cat {
            SettingsCategory::General => {
                assert!(combined_actions.contains(&Action::SetNotchStyle(NotchStyle::EdgeFlush)));
                assert!(combined_actions.contains(&Action::SetPlacement(NotchPlacement::Left)));
                assert!(combined_actions.contains(&Action::SetPlacement(NotchPlacement::Right)));
                assert!(combined_actions.contains(&Action::SetSizePreset(NotchSizePreset::Wide)));
                assert!(
                    combined_actions.contains(&Action::SetExpandedSize(ExpandedSizePreset::Large))
                );
                assert!(combined_actions.contains(&Action::SetTypeScale(TypeScale::Large)));
                assert!(combined_actions.contains(&Action::SetTypeScale(TypeScale::ExtraLarge)));
                assert!(combined_actions.contains(&Action::SetIdleMetric(IdleMetric::NetSpeed)));
                assert!(combined_actions.contains(&Action::ToggleMinimizedIsland));
                assert!(combined_actions.contains(&Action::ToggleClickToOpen));
                assert!(combined_actions.contains(&Action::ToggleHideFullscreen));
                assert!(combined_actions.contains(&Action::ToggleShowMascot));
                assert!(combined_actions.contains(&Action::ToggleProgressArc));
                assert!(combined_actions.contains(&Action::ToggleRemoveWindowsBorders));
            }
            SettingsCategory::Motion => {
                assert!(combined_actions.contains(&Action::SetFpsCap(120)));
                assert!(combined_actions.contains(&Action::ToggleMicroInteractions));
                assert!(combined_actions.contains(&Action::ToggleEqualizer));
            }
            SettingsCategory::Modules => {
                assert!(combined_actions.contains(&Action::ToggleModule(NotchMode::Music)));
                assert!(combined_actions.contains(&Action::ToggleModule(NotchMode::Telemetry)));
                assert!(combined_actions.contains(&Action::SetLyricSize(LyricSize::Large)));
                assert!(combined_actions.contains(&Action::SetLyricStyle(LyricStyle::KaraokeGlow)));
                assert!(combined_actions.contains(&Action::ToggleClipboardWatch));
                assert!(combined_actions.contains(&Action::ToggleLyricsMonitor));
                assert!(combined_actions.contains(&Action::ToggleCaptureClipboard));
            }
            SettingsCategory::Notifications => {
                assert!(combined_actions.contains(&Action::ToggleNotifications));
                assert!(combined_actions.contains(&Action::ToggleNotifOverFullscreen));
                assert!(combined_actions.contains(&Action::SetNotificationSecs(6)));
                assert!(combined_actions.contains(&Action::ToggleNotifListener));
                assert!(combined_actions.contains(&Action::ToggleNotifExclusive));
                assert!(combined_actions.contains(&Action::ToggleNotifAutoClear));
                assert!(combined_actions.contains(&Action::OpenWinNotifSettings));
            }
            SettingsCategory::System => {
                assert!(combined_actions.contains(&Action::ToggleAutoClean));
                assert!(combined_actions.contains(&Action::ToggleShowCleanResults));
                for area_idx in 0u8..8u8 {
                    assert!(
                        combined_actions.contains(&Action::ToggleArea(area_idx)),
                        "Settings System tab missing ToggleArea({})",
                        area_idx
                    );
                }
                assert!(combined_actions.contains(&Action::SetCleanThreshold(0)));
                assert!(combined_actions.contains(&Action::SetCleanThreshold(80)));
                assert!(combined_actions.contains(&Action::SetCleanInterval(0)));
                assert!(combined_actions.contains(&Action::SetCleanInterval(15)));
                assert!(combined_actions.contains(&Action::PurgeRam));
                assert!(combined_actions.contains(&Action::ToggleAutostart));
                assert!(combined_actions.contains(&Action::ToggleScreenTime));
                assert!(combined_actions.contains(&Action::ResetScreenTimeToday));
                assert!(combined_actions.contains(&Action::ResetAllSettings));
                assert!(combined_actions.contains(&Action::Quit));
            }
        }
    }
}

#[test]
fn test_zero_bounce_monotonic_ease_exp_and_animations() {
    // Upward ease_exp must be monotonically non-decreasing with zero overshoot
    let mut cur = 0.0f32;
    let target = 576.0f32;
    for _ in 0..80 {
        let (next, _) = ease_exp(cur, target, 15.0, 0.016, 0.5);
        assert!(
            next >= cur,
            "Upward ease_exp dipped backward: {} -> {}",
            cur,
            next
        );
        assert!(
            next <= target,
            "Upward ease_exp overshot target {}: {}",
            target,
            next
        );
        cur = next;
    }
    assert_eq!(cur, target);

    // Downward ease_exp must be monotonically non-increasing with zero overshoot
    let mut down = 576.0f32;
    let low_target = 280.0f32;
    for _ in 0..80 {
        let (next, _) = ease_exp(down, low_target, 15.0, 0.016, 0.5);
        assert!(
            next <= down,
            "Downward ease_exp bounced upward: {} -> {}",
            down,
            next
        );
        assert!(
            next >= low_target,
            "Downward ease_exp undershot target {}: {}",
            low_target,
            next
        );
        down = next;
    }
    assert_eq!(down, low_target);

    // Notch hover expansion converges cleanly without bounce and turns timer off when mascot disabled
    let mut state = AppState::new_for_preview();
    state.show_mascot = false;
    state.is_pinned = false;
    state.set_hovered(false);
    state.sync_preview_settled();
    state.media = MediaTrack::idle();
    state.mode = NotchMode::Tasks;

    let exp_w = state.expanded_width();
    let mut prev_w = state.current_width;
    state.set_hovered(true);
    for _ in 0..60 {
        state.update_animation(0.016);
        assert!(
            state.current_width >= prev_w,
            "Notch width bounced back while expanding"
        );
        assert!(
            state.current_width <= exp_w,
            "Notch width overshot expanded width"
        );
        prev_w = state.current_width;
    }
    assert_eq!(state.current_width, exp_w);
    assert_eq!(state.needs_animation_timer_ms(), None);

    // Settings sidebar pill slide + switch toggle convergence turns timer off cleanly
    state.settings_open = true;
    state.apply_setting_action(Action::SetSettingsCategory(SettingsCategory::System));
    state.apply_setting_action(Action::ToggleHideFullscreen);
    assert!(state.needs_settings_timer_ms().is_some());
    for _ in 0..60 {
        state.update_settings_animation(0.016);
    }
    assert_eq!(state.needs_settings_timer_ms(), None);

    // FPS pacing intervals for Auto / 60 / 90 / 120 Hz
    state.fps_cap = 0;
    state.detected_hz = 60;
    assert_eq!(state.desired_active_interval_ms(), 16);
    state.detected_hz = 120;
    assert_eq!(state.desired_active_interval_ms(), 8);
    state.fps_cap = 60;
    assert_eq!(state.desired_active_interval_ms(), 16);
    state.fps_cap = 90;
    assert_eq!(state.desired_active_interval_ms(), 11);
    state.fps_cap = 120;
    assert_eq!(state.desired_active_interval_ms(), 8);
}

#[test]
fn test_settings_actions_and_legacy_config_compat() {
    let mut state = AppState::new_for_preview();

    state.apply_setting_action(Action::SetNotchStyle(NotchStyle::EdgeFlush));
    assert_eq!(state.notch_style, NotchStyle::EdgeFlush);

    state.apply_setting_action(Action::SetSizePreset(NotchSizePreset::Wide));
    assert_eq!(
        state.idle_width(),
        (NotchSizePreset::Wide.idle_width() * state.type_scale.idle_ui_scale()).round()
    );

    state.apply_setting_action(Action::TogglePin);
    assert!(state.is_pinned && state.is_hovered);

    // Hiding the currently open dock module falls back cleanly to Home
    state.set_mode(NotchMode::Telemetry);
    state.apply_setting_action(Action::ToggleModule(NotchMode::Telemetry));
    assert!(!state.modules.telemetry);
    assert_eq!(state.mode, NotchMode::Home);

    state.apply_setting_action(Action::ResetScreenTimeToday);
    assert_eq!(state.screen_time.today_secs, 0);

    state.apply_setting_action(Action::ResetAllSettings);
    assert_eq!(state.notch_style, NotchStyle::Floating);
    assert_eq!(state.size_preset, NotchSizePreset::Standard);

    // Legacy v1.4 config JSON (only original keys, including removed water_interval_mins) deserializes with safe defaults
    let legacy_v14_json = r#"{
        "chime_enabled": false,
        "water_interval_mins": 60,
        "clean_working_sets": true,
        "clean_system_ws": false,
        "clean_standby_list": true,
        "when_above_pct": 90,
        "every_mins": 30
    }"#;
    assert!(
        state.apply_config_json(legacy_v14_json),
        "Legacy v1.4 PersistedConfig JSON should deserialize seamlessly"
    );
    assert!(!state.chime_enabled);
    assert!(!state.mem_reduct.clean_system_ws);
    assert_eq!(state.mem_reduct.when_above_pct, 90);
    assert_eq!(state.notch_style, NotchStyle::Floating);
    assert!(!state.minimized_island);
}

#[test]
fn test_minimized_island_mode_centered_and_smooth_morph() {
    let mut state = AppState::new_for_preview();
    state.is_pinned = false;
    state.set_hovered(false);
    state.sync_preview_settled();

    assert!(!state.minimized_island);
    assert_eq!(
        state.idle_width(),
        (AppState::IDLE_WIDTH * state.type_scale.idle_ui_scale()).round()
    );
    assert_eq!(
        state.idle_height(),
        (AppState::IDLE_HEIGHT * state.type_scale.idle_ui_scale()).round()
    );

    state.apply_setting_action(Action::ToggleMinimizedIsland);
    assert!(state.minimized_island);
    assert_eq!(state.idle_width(), AppState::MINIMIZED_WIDTH);
    assert_eq!(state.idle_height(), AppState::MINIMIZED_HEIGHT);
    assert_eq!(state.target_width, AppState::MINIMIZED_WIDTH);
    assert_eq!(state.target_height, AppState::MINIMIZED_HEIGHT);

    // Converge monotonically down to minimized micro-capsule (86x18)
    assert_eq!(AppState::MINIMIZED_WIDTH, 86.0);
    assert_eq!(AppState::MINIMIZED_HEIGHT, 18.0);
    let mut prev_w = state.current_width;
    for _ in 0..80 {
        state.update_animation(0.016);
        assert!(state.current_width <= prev_w);
        assert!(state.current_width >= AppState::MINIMIZED_WIDTH);
        prev_w = state.current_width;
    }
    assert_eq!(state.current_width, AppState::MINIMIZED_WIDTH);
    assert_eq!(state.current_height, AppState::MINIMIZED_HEIGHT);

    // Verify centered fixed-canvas rendering puts minimized capsule in center [267..353, 0..18]
    state.set_notification(
        "Ping".to_string(),
        "Body".to_string(),
        NotificationKind::Capture,
        false,
    );
    let mut r = Renderer::new(
        AppState::EXPANDED_WIDTH as u32,
        AppState::EXPANDED_HEIGHT as u32,
    );
    r.render_centered(&state, None);
    let expected_origin = (AppState::EXPANDED_WIDTH - AppState::MINIMIZED_WIDTH) * 0.5;
    for h in &r.hits {
        assert!(
            h.rect.left() >= expected_origin
                && h.rect.right() <= expected_origin + AppState::MINIMIZED_WIDTH + 1.0,
            "Centered minimized hit out of centered capsule X bounds: {:?} vs origin {}",
            h.rect,
            expected_origin
        );
        assert!(
            h.rect.bottom() <= AppState::MINIMIZED_HEIGHT + 0.5,
            "Minimized hit taller than capsule: {:?}",
            h.rect
        );
    }

    // Expand smoothly from minimized (86x18) to scaled expanded dims without bounce
    let exp_w = state.expanded_width();
    let exp_h = state.expanded_height();
    state.set_hovered(true);
    let mut exp_prev_w = state.current_width;
    let mut exp_prev_h = state.current_height;
    for _ in 0..80 {
        state.update_animation(0.016);
        assert!(state.current_width >= exp_prev_w);
        assert!(state.current_height >= exp_prev_h);
        assert!(state.current_width <= exp_w);
        assert!(state.current_height <= exp_h);
        exp_prev_w = state.current_width;
        exp_prev_h = state.current_height;
    }
    assert_eq!(state.current_width, exp_w);
    assert_eq!(state.current_height, exp_h);
}

#[test]
fn test_real_memory_pools_telemetry() {
    let pools = cynotch::memreduct::query_memory_pools();
    assert!(
        pools.phys_total_gb > 0.5,
        "Expected real physical RAM > 0.5 GB, got {}",
        pools.phys_total_gb
    );
    assert!(pools.phys_used_gb > 0.0);
    assert!((1.0..=100.0).contains(&pools.phys_percent));
    assert!(pools.page_total_gb >= pools.phys_total_gb * 0.5);
    assert!(pools.sys_cache_peak_mb >= pools.sys_cache_mb);
}

#[test]
fn test_fixed_canvas_render_and_slider_drag_alignment() {
    let mut state = AppState::new_for_preview();
    let sc = state.expanded_scale();
    let mut r = Renderer::new(
        state.expanded_width() as u32,
        state.expanded_height() as u32,
    );

    // Verify rendered Volume slider and Music scrubber match drag constants
    state.set_hovered(true);
    state.mode = NotchMode::Volume;
    state.sync_preview_settled();
    r.render_centered(&state, None);
    let vol_hits: Vec<_> = r
        .hits
        .iter()
        .filter(|h| matches!(h.action, Action::SetVolume(_)) && h.rect.top() < 150.0 * sc)
        .collect();
    assert_eq!(vol_hits.len(), 10);
    let first_vx = vol_hits.first().unwrap().rect.left();
    let last_vx = vol_hits.last().unwrap().rect.right();
    assert!((first_vx - AppState::VOLUME_SLIDER_X * sc).abs() < 1.0);
    assert!(((last_vx - first_vx) - AppState::VOLUME_SLIDER_W * sc).abs() < 1.0);

    state.mode = NotchMode::Music;
    state.show_lyrics = false;
    r.render_centered(&state, None);
    let music_hits: Vec<_> = r
        .hits
        .iter()
        .filter(|h| matches!(h.action, Action::SeekMusic(_)))
        .collect();
    assert_eq!(music_hits.len(), 10);
    let first_mx = music_hits.first().unwrap().rect.left();
    let last_mx = music_hits.last().unwrap().rect.right();
    assert!((first_mx - AppState::MUSIC_SCRUBBER_X * sc).abs() < 1.0);
    assert!(((last_mx - first_mx) - AppState::MUSIC_SCRUBBER_W * sc).abs() < 1.0);

    // Performance smoke: 60 fixed-canvas renders with zero buffer reallocations
    let orig_ptr = r.pixmap.data().as_ptr();
    let start = std::time::Instant::now();
    for i in 0..60 {
        state.set_hovered(i % 2 == 0);
        state.sync_preview_settled();
        r.render_centered(&state, None);
    }
    let elapsed = start.elapsed();
    assert_eq!(
        r.pixmap.data().as_ptr(),
        orig_ptr,
        "Fixed canvas render_centered must never reallocate Pixmap backing memory"
    );
    let max_secs = if cfg!(debug_assertions) { 25.0 } else { 1.5 };
    assert!(
        elapsed.as_secs_f32() < max_secs,
        "60 centered frames took unexpectedly long: {:?} (limit {:.1}s)",
        elapsed,
        max_secs
    );
}

#[test]
fn test_mascot_mood_priority_and_states_render() {
    let mut state = AppState::new_for_preview();
    state.media = MediaTrack::idle();
    state.notification = None;
    state.mem_reduct.is_purging = false;
    state.set_hovered(false);
    state.sync_preview_settled();
    assert_eq!(state.mascot_mood(), MascotMood::Idle);
    assert_eq!(state.needs_animation_timer_ms(), Some(28));

    // Lowest -> highest precedence
    state.media.is_playing = true;
    assert_eq!(state.mascot_mood(), MascotMood::Music);
    assert_eq!(state.needs_animation_timer_ms(), Some(20));

    state.set_notification(
        "Captured".to_string(),
        "desktop.png".to_string(),
        NotificationKind::Capture,
        true,
    );
    assert_eq!(state.mascot_mood(), MascotMood::Capture);
    assert_eq!(state.needs_animation_timer_ms(), Some(16));
    state.sync_preview_settled();
    assert_eq!(state.needs_animation_timer_ms(), Some(20));

    // Purge wins over everything else
    state.mem_reduct.is_purging = true;
    assert_eq!(state.mascot_mood(), MascotMood::Purging);
    assert_eq!(state.needs_animation_timer_ms(), Some(16));

    // Verify rendering across moods & minimized mode produces non-identical frames without panicking
    state.mem_reduct.is_purging = false;
    state.notification = None;
    state.media.is_playing = false;
    state.set_hovered(false);
    state.sync_preview_settled();

    let mut r = Renderer::new(
        AppState::EXPANDED_WIDTH as u32,
        AppState::EXPANDED_HEIGHT as u32,
    );
    state.anim_time = 0.25;
    r.render_centered(&state, None);
    let idle_bytes = r.pixmap.data().to_vec();

    state.mem_reduct.is_purging = true;
    state.anim_time = 0.55;
    r.render_centered(&state, None);
    let purge_bytes = r.pixmap.data().to_vec();
    assert_ne!(
        idle_bytes, purge_bytes,
        "Purging mascot animation frame should visually differ from Idle"
    );

    // Minimized capsule with Music mood
    state.mem_reduct.is_purging = false;
    state.minimized_island = true;
    state.media.is_playing = true;
    state.sync_preview_settled();
    r.render_centered(&state, None);
}

#[test]
fn test_classify_foreign_fullscreen_and_topmost_rules() {
    use cynotch::window::classify_foreign_fullscreen;
    const WS_CAPTION: u32 = 0x00C0_0000;
    const WS_THICKFRAME: u32 = 0x0004_0000;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;

    // 1. Different monitor never hides CyNotch on primary monitor
    assert!(!classify_foreign_fullscreen(
        true, false, true, WS_POPUP, 0, false
    ));

    // 2. Normal & Maximized windows with WS_CAPTION or WS_THICKFRAME NEVER hide CyNotch (even if Auto-Hide Taskbar lets them touch screen edges)
    assert!(!classify_foreign_fullscreen(
        false,
        true,
        true,
        WS_CAPTION | WS_THICKFRAME,
        0,
        true
    ));
    assert!(!classify_foreign_fullscreen(
        false,
        true,
        true,
        WS_THICKFRAME,
        0,
        true
    ));
    assert!(!classify_foreign_fullscreen(
        false, true, false, WS_CAPTION, 0, true
    ));

    // 3. Tool windows never count as fullscreen
    assert!(!classify_foreign_fullscreen(
        false,
        true,
        true,
        WS_POPUP,
        WS_EX_TOOLWINDOW,
        false
    ));

    // 4. True F11 / Borderless Fullscreen (no caption, no thickframe, covers full monitor) DOES hide idle notch
    assert!(classify_foreign_fullscreen(
        false, true, true, WS_POPUP, 0, false
    ));
    assert!(classify_foreign_fullscreen(
        false, true, true, WS_POPUP, 0, true
    ));

    // 5. Exclusive D3D game or Presentation mode on same monitor DOES hide idle notch
    assert!(classify_foreign_fullscreen(
        true, true, false, WS_CAPTION, 0, false
    ));
}

#[test]
fn test_v18_telemetry_cpu_and_network_formatting() {
    assert_eq!(format_speed_short(0.0), "0K");
    assert_eq!(format_speed_short(42.4), "42K");
    assert_eq!(format_speed_short(1536.0), "1.5M");
    assert_eq!(format_speed_long(1536.0), "1.5 MB/s");
    assert_eq!(format_speed_long(88.0), "88 KB/s");

    let mut collector = TelemetryCollector::new();
    std::thread::sleep(std::time::Duration::from_millis(60));
    let telem = collector.poll();
    assert!(telem.memory_total_gb > 0.5);
    assert!((0.0..=100.0).contains(&telem.cpu_usage));
    assert!(telem.net_down_kbps >= 0.0 && telem.net_down_kbps.is_finite());
    assert!(telem.net_up_kbps >= 0.0 && telem.net_up_kbps.is_finite());
}

#[test]
fn test_v18_home_live_net_card_and_legacy_status_ring_alias() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Home;
    state.sync_preview_settled();

    // Home Hub shows Live Network card with Telemetry navigation
    let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
    r.render(&state);
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::SetMode(NotchMode::Telemetry)),
        "Home Hub Live Network card must expose SetMode(Telemetry) click target"
    );

    // Legacy status_ring_enabled JSON alias maps cleanly to progress_arc_enabled
    let alias_json = r#"{
        "chime_enabled": true,
        "water_interval_mins": 45,
        "clean_working_sets": true,
        "clean_system_ws": true,
        "clean_standby_list": true,
        "when_above_pct": 70,
        "every_mins": 0,
        "status_ring_enabled": false
    }"#;
    assert!(state.apply_config_json(alias_json));
    assert!(!state.progress_arc_enabled);
}

#[test]
fn test_v18_idle_metrics_and_progress_arc_render() {
    let mut state = AppState::new_for_preview();
    state.media = MediaTrack::idle();
    state.notification = None;
    state.mem_reduct.is_purging = false;
    state.telemetry.net_down_kbps = 1840.0;
    state.telemetry.net_up_kbps = 320.0;
    state.telemetry.cpu_usage = 24.0;
    state.telemetry.memory_percent = 57.0;
    state.set_hovered(false);
    state.sync_preview_settled();

    let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
    let mut frames = Vec::new();
    for metric in [
        IdleMetric::Ram,
        IdleMetric::NetSpeed,
        IdleMetric::Cpu,
        IdleMetric::Clock,
        IdleMetric::Minimal,
    ] {
        state.apply_setting_action(Action::SetIdleMetric(metric));
        r.render_centered(&state, None);
        frames.push(r.pixmap.data().to_vec());
    }
    assert_ne!(
        frames[0], frames[1],
        "Ram and NetSpeed idle capsules should render differently"
    );
    assert_ne!(
        frames[1], frames[4],
        "NetSpeed and Minimal idle capsules should render differently"
    );

    state.apply_setting_action(Action::SetIdleMetric(IdleMetric::Ram));
    state.apply_setting_action(Action::ToggleProgressArc);
    assert!(!state.progress_arc_enabled);
    r.render_centered(&state, None);
    let no_arc = r.pixmap.data().to_vec();
    assert_ne!(
        frames[0], no_arc,
        "ProgressArc toggle should alter rendered idle badge"
    );
}

#[test]
fn test_v18_expanded_size_presets_and_canvas_scaling() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Volume;

    for (preset, expected_scale) in [
        (ExpandedSizePreset::Compact, 0.90f32),
        (ExpandedSizePreset::Standard, 1.00f32),
        (ExpandedSizePreset::Large, 1.15f32),
    ] {
        state.apply_setting_action(Action::SetExpandedSize(preset));
        state.sync_preview_settled();
        let total_s = expected_scale * state.type_scale.ui_scale();
        let ew = (AppState::EXPANDED_WIDTH * total_s).round();
        let eh = (AppState::EXPANDED_HEIGHT * total_s).round();
        assert_eq!(state.expanded_width(), ew);
        assert_eq!(state.expanded_height(), eh);

        let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
        r.render_centered(&state, None);
        for hit in &r.hits {
            assert!(
                hit.rect.left() >= 0.0
                    && hit.rect.top() >= 0.0
                    && hit.rect.right() <= ew + 0.5
                    && hit.rect.bottom() <= eh + 0.5,
                "Scaled {:?} hit out of bounds: {:?}",
                preset,
                hit.rect
            );
        }

        // Verify volume slider geometry scales cleanly with expanded_scale()
        let vol_hits: Vec<_> = r
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::SetVolume(_)) && h.rect.top() < 150.0 * total_s)
            .collect();
        assert_eq!(vol_hits.len(), 10);
        let vx0 = vol_hits.first().unwrap().rect.left();
        let vw = vol_hits.last().unwrap().rect.right() - vx0;
        assert!((vx0 - AppState::VOLUME_SLIDER_X * total_s).abs() < 1.0);
        assert!((vw - AppState::VOLUME_SLIDER_W * total_s).abs() < 1.0);
    }
}

#[test]
fn test_v18_vertical_placement_idle_and_all_10_modes() {
    let mut state = AppState::new_for_preview();
    state.media.is_playing = true;
    state.notification = None;

    for placement in [NotchPlacement::Left, NotchPlacement::Right] {
        state.apply_setting_action(Action::SetPlacement(placement));
        state.set_hovered(false);
        state.minimized_island = true;
        state.sync_preview_settled();
        assert_eq!(state.idle_width(), 18.0);
        assert_eq!(state.idle_height(), 86.0);

        state.minimized_island = false;
        state.sync_preview_settled();
        let idle_s = state.type_scale.idle_ui_scale();
        assert_eq!(
            state.idle_width(),
            (AppState::VERT_IDLE_WIDTH * idle_s).round()
        );
        assert_eq!(
            state.idle_height(),
            (AppState::VERT_IDLE_HEIGHT * idle_s).round()
        );

        let (ox, _) = Renderer::compute_origin(
            &state,
            state.canvas_width() as f32,
            state.canvas_height() as f32,
        );
        if placement == NotchPlacement::Left {
            assert_eq!(ox, 0.0);
        } else {
            assert!((ox - (state.canvas_width() as f32 - state.current_width)).abs() < 0.5);
        }

        let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
        r.render_centered(&state, None);

        // Expand vertically across ALL 10 NotchModes + Lyrics + Home no-health
        state.set_hovered(true);
        state.sync_preview_settled();
        let es = state.expanded_scale();
        assert_eq!(
            state.expanded_width(),
            (AppState::VERT_EXPANDED_WIDTH * es).round()
        );
        assert_eq!(
            state.expanded_height(),
            (AppState::VERT_EXPANDED_HEIGHT * es).round()
        );

        let cases = [
            (NotchMode::Home, false),
            (NotchMode::Music, false),
            (NotchMode::Music, true),
            (NotchMode::Volume, false),
            (NotchMode::MemReduct, false),
            (NotchMode::Tasks, false),
            (NotchMode::Shelf, false),
            (NotchMode::Capture, false),
            (NotchMode::Telemetry, false),
            (NotchMode::Settings, false),
        ];

        for (mode, show_lyrics) in cases {
            state.mode = mode;
            state.show_lyrics = show_lyrics;
            state.y_offset = 45;
            r.render_centered(&state, None);

            assert!(
                !r.hits.is_empty(),
                "Vertical {:?} (mode={:?}, lyrics={}) rendered zero hit targets",
                placement,
                mode,
                show_lyrics
            );
            for hit in &r.hits {
                assert!(
                    hit.rect.left() >= 0.0
                        && hit.rect.top() >= 0.0
                        && hit.rect.right() <= state.expanded_width() + 0.5
                        && hit.rect.bottom() <= state.expanded_height() + 0.5,
                    "Vertical {:?} out-of-bounds hit {:?} in {:?}: {:?}",
                    placement,
                    hit.action,
                    mode,
                    hit.rect
                );
            }
        }
    }
}

#[test]
fn test_v18_partial_dirty_rect_optimization() {
    let mut state = AppState::new_for_preview();
    state.placement = NotchPlacement::Top;
    state.minimized_island = true;
    state.set_hovered(true);
    state.sync_preview_settled();

    let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
    // Frame 1: Full expanded notch (620x232)
    r.render_centered(&state, None);

    // Frame 2: Collapse immediately to 86x18 -> dirty union spans 0..expanded_height (to wipe old expanded pixels)
    state.set_hovered(false);
    state.sync_preview_settled();
    r.render_centered(&state, None);
    assert_eq!(r.dirty_rect.3, state.expanded_height() as u32);

    // Verify stale pixels below y=18 were completely cleared to transparent (alpha == 0)
    let stride = (r.pixmap.width() as usize) * 4;
    let row_200 = &r.pixmap.data()[200 * stride..201 * stride];
    assert!(
        row_200.iter().all(|&b| b == 0),
        "Expected row 200 to be fully transparent after collapsing to minimized idle"
    );

    // Frame 3: Steady minimized idle -> dirty_rect only touches top micro strip (height < 40px instead of 232px)
    r.render_centered(&state, None);
    assert!(
        r.dirty_rect.3 <= 40,
        "Steady minimized dirty_rect bottom should stay compact (got {}), saving clear/swizzle work",
        r.dirty_rect.3
    );
}

#[test]
fn test_v19_settings_viewport_clipping_and_no_hit_overlaps() {
    let mut state = AppState::new_for_preview();
    state.settings_open = true;
    let sw = AppState::SETTINGS_WIDTH as u32;
    let sh = AppState::SETTINGS_HEIGHT as u32;
    assert_eq!(sw, 860);
    assert_eq!(sh, 720);

    let mut pm = Pixmap::new(sw, sh).unwrap();
    let mut mask = Mask::new(sw, sh).unwrap();
    let mut fonts = FontRenderer::new();

    for cat in SettingsCategory::ALL {
        state.set_settings_category(cat);
        state.set_settings_scroll(cat, 0.0);
        state.sync_preview_settled();
        let (hits0, max_scroll) =
            render_settings_pixmap_with_meta(&state, None, &mut pm, &mut mask, &mut fonts);

        if matches!(cat, SettingsCategory::General | SettingsCategory::Modules) {
            assert!(
                max_scroll > 20.0,
                "Expected scrollable overflow on {:?}, got max_scroll={}",
                cat,
                max_scroll
            );
        }

        for (scroll_label, hits) in [
            ("top", hits0),
            ("bottom", {
                state.set_settings_scroll(cat, max_scroll);
                render_settings_pixmap(&state, None, &mut pm, &mut mask, &mut fonts)
            }),
        ] {
            // Scrolled right-pane hits must stay inside the viewport clip (y = 78..698);
            // the top-right CloseSettings button sits in the fixed window header above PANE_Y.
            for h in &hits {
                if h.rect.left() >= 230.0 && h.action != Action::CloseSettings {
                    assert!(
                        h.rect.top() >= 77.5 && h.rect.bottom() <= 698.5,
                        "Settings {:?} ({}) pane hit escaped viewport clip: {:?} -> {:?}",
                        cat,
                        scroll_label,
                        h.action,
                        h.rect
                    );
                }
            }

            // Verify zero pairwise overlap among rendered interactive controls
            for i in 0..hits.len() {
                for j in (i + 1)..hits.len() {
                    let a = hits[i].rect;
                    let b = hits[j].rect;
                    let ix = (a.right().min(b.right()) - a.left().max(b.left())).max(0.0);
                    let iy = (a.bottom().min(b.bottom()) - a.top().max(b.top())).max(0.0);
                    assert!(
                        ix * iy <= 1.0,
                        "Overlapping Settings hits in {:?} ({}): {:?} ({:?}) vs {:?} ({:?})",
                        cat,
                        scroll_label,
                        hits[i].action,
                        a,
                        hits[j].action,
                        b
                    );
                }
            }
        }
    }
}

#[test]
fn test_v19_lyrics_styles_sizes_horizontal_and_vertical() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Music;
    state.show_lyrics = true;
    state.current_lyric_index = Some(2);

    // Cycle helpers cover all sizes & styles cleanly
    state.lyric_size = LyricSize::Compact;
    state.apply_setting_action(Action::CycleLyricSize);
    assert_eq!(state.lyric_size, LyricSize::Standard);
    state.apply_setting_action(Action::CycleLyricSize);
    assert_eq!(state.lyric_size, LyricSize::Large);
    state.apply_setting_action(Action::CycleLyricSize);
    assert_eq!(state.lyric_size, LyricSize::Compact);

    state.lyric_style = LyricStyle::ClassicList;
    state.apply_setting_action(Action::CycleLyricStyle);
    assert_eq!(state.lyric_style, LyricStyle::KaraokeGlow);
    state.apply_setting_action(Action::CycleLyricStyle);
    assert_eq!(state.lyric_style, LyricStyle::MinimalSingle);
    state.apply_setting_action(Action::CycleLyricStyle);
    assert_eq!(state.lyric_style, LyricStyle::CompactStream);
    state.apply_setting_action(Action::CycleLyricStyle);
    assert_eq!(state.lyric_style, LyricStyle::ClassicList);

    for placement in [NotchPlacement::Top, NotchPlacement::Right] {
        state.apply_setting_action(Action::SetPlacement(placement));
        state.sync_preview_settled();
        let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
        let mut rendered_styles = Vec::new();
        for style in [
            LyricStyle::ClassicList,
            LyricStyle::KaraokeGlow,
            LyricStyle::MinimalSingle,
            LyricStyle::CompactStream,
        ] {
            for size in [LyricSize::Compact, LyricSize::Standard, LyricSize::Large] {
                state.lyric_style = style;
                state.lyric_size = size;
                r.render_centered(&state, None);
                assert!(
                    r.hits.iter().any(|h| h.action == Action::CycleLyricSize),
                    "Missing CycleLyricSize chip in {:?} {:?}/{:?}",
                    placement,
                    style,
                    size
                );
                assert!(
                    r.hits.iter().any(|h| h.action == Action::CycleLyricStyle),
                    "Missing CycleLyricStyle chip in {:?} {:?}/{:?}",
                    placement,
                    style,
                    size
                );
                assert!(
                    r.hits
                        .iter()
                        .any(|h| matches!(h.action, Action::ScrollLyrics(_))),
                    "Missing ScrollLyrics hit in {:?} {:?}/{:?}",
                    placement,
                    style,
                    size
                );
                assert!(
                    r.hits.iter().any(|h| h.action == Action::TogglePlayPause),
                    "Missing TogglePlayPause in {:?} {:?}/{:?}",
                    placement,
                    style,
                    size
                );
                if size == LyricSize::Standard {
                    rendered_styles.push(r.pixmap.data().to_vec());
                }
            }
        }
        assert_ne!(
            rendered_styles[0], rendered_styles[1],
            "ClassicList vs KaraokeGlow must differ"
        );
        assert_ne!(
            rendered_styles[1], rendered_styles[2],
            "KaraokeGlow vs MinimalSingle must differ"
        );
        assert_ne!(
            rendered_styles[2], rendered_styles[3],
            "MinimalSingle vs CompactStream must differ"
        );
    }
}

#[test]
fn test_v19_system_toast_and_sanitization() {
    assert_eq!(
        sanitize_notification_text("  Hello\r\n\tWorld!\x07  "),
        "Hello World!"
    );
    assert_eq!(sanitize_notification_text("\n\t "), "");

    let mut state = AppState::new_for_preview();
    state.set_hovered(false);
    state.set_system_toast(
        "Discord".to_string(),
        "Kai (#general)".to_string(),
        "Build green across all targets".to_string(),
    );
    let notif = state
        .notification
        .as_ref()
        .expect("System toast should enqueue banner");
    assert_eq!(notif.kind, NotificationKind::SystemToast);
    assert_eq!(notif.app_name, "Discord");

    for (placement, minimized) in [
        (NotchPlacement::Top, false),
        (NotchPlacement::Top, true),
        (NotchPlacement::Left, false),
        (NotchPlacement::Left, true),
    ] {
        state.placement = placement;
        state.minimized_island = minimized;
        state.sync_preview_settled();
        let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
        r.render_centered(&state, None);
        let non_zero_alpha = r
            .pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] > 200);
        assert!(
            non_zero_alpha,
            "SystemToast capsule should render opaque foreground content ({:?}, min={})",
            placement, minimized
        );
    }
}

#[test]
fn test_v19_frosted_glass_alpha_and_config_roundtrip() {
    let mut state = AppState::new_for_preview();
    state.set_hovered(true);
    state.mode = NotchMode::Home;
    state.sync_preview_settled();

    // Frosted glass active when both frosted_glass_enabled and windows_transparency_enabled are true
    state.windows_transparency_enabled = true;
    state.frosted_glass_enabled = true;
    assert!(state.is_frosted_active());

    // Sample interior background shell pixel away from foreground text/cards
    let sample_x = (state.expanded_width() * 0.48) as usize;
    let sample_y = (state.expanded_height() - 6.0) as usize;

    let mut r_frosted = Renderer::new(state.canvas_width(), state.canvas_height());
    r_frosted.render_centered(&state, None);
    let idx_fr = (sample_y * r_frosted.pixmap.width() as usize + sample_x) * 4;
    let frosted_alpha = r_frosted.pixmap.data()[idx_fr + 3];
    assert!(
        frosted_alpha < 245,
        "Expected active frosted shell alpha < 245, got {}",
        frosted_alpha
    );

    // Opaque fallback when Windows Transparency effects are disabled
    state.windows_transparency_enabled = false;
    assert!(!state.is_frosted_active());
    let mut r_opaque = Renderer::new(state.canvas_width(), state.canvas_height());
    r_opaque.render_centered(&state, None);
    let idx = (sample_y * r_opaque.pixmap.width() as usize + sample_x) * 4;
    let opaque_alpha = r_opaque.pixmap.data()[idx + 3];
    assert!(
        opaque_alpha >= 245,
        "Expected opaque obsidian shell alpha >= 245, got {}",
        opaque_alpha
    );

    // v1.9 PersistedConfig JSON fields round-trip & legacy fallback
    let v19_json = r#"{
        "chime_enabled": true,
        "water_interval_mins": 30,
        "clean_working_sets": true,
        "clean_system_ws": true,
        "clean_standby_list": true,
        "when_above_pct": 80,
        "every_mins": 15,
        "lyric_size": "Large",
        "lyric_style": "KaraokeGlow",
        "frosted_glass_enabled": false,
        "notif_listener_enabled": true,
        "notif_exclusive_mode": true,
        "notif_auto_clear_action_center": true
    }"#;
    assert!(state.apply_config_json(v19_json));
    assert_eq!(state.lyric_size, LyricSize::Large);
    assert_eq!(state.lyric_style, LyricStyle::KaraokeGlow);
    assert!(!state.frosted_glass_enabled);
    assert!(state.notif_listener_enabled);
    assert!(state.notif_exclusive_mode);
    assert!(state.notif_auto_clear_action_center);

    let legacy_json = r#"{
        "chime_enabled": true,
        "water_interval_mins": 45,
        "clean_working_sets": true,
        "clean_system_ws": true,
        "clean_standby_list": true,
        "when_above_pct": 70,
        "every_mins": 0
    }"#;
    assert!(state.apply_config_json(legacy_json));
    assert_eq!(state.lyric_size, LyricSize::Standard);
    assert_eq!(state.lyric_style, LyricStyle::ClassicList);
    assert!(state.frosted_glass_enabled);
    assert!(
        state.notif_listener_enabled,
        "Windows Notification Bridge should default to ON (including migrated pre-v2 configs)"
    );
    assert!(!state.notif_exclusive_mode);
    assert!(!state.notif_over_fullscreen);
}

#[test]
fn test_v20_lyrics_word_wrap_no_ellipsis_and_fullscreen_notif_overlay() {
    let fonts = FontRenderer::new();
    let long_lyric = "Chasing golden horizons through the midnight glow while the violet lightning hums underneath the desert sky";
    let wrapped = fonts.wrap_lines(long_lyric, 15.4, cynotch::font::Weight::Bold, 340.0);
    assert!(
        wrapped.len() >= 2,
        "Long lyric must wrap across multiple lines instead of truncating"
    );
    for line in &wrapped {
        assert!(
            !line.contains('…') && !line.contains("..."),
            "Wrapped lyric line must never contain ellipsis: {}",
            line
        );
    }
    assert_eq!(wrapped.join(" "), long_lyric);

    let mut state = AppState::new_for_preview();
    assert!(
        state.notif_listener_enabled,
        "Windows Notification Bridge must default to ON"
    );
    assert!(
        !state.notif_exclusive_mode,
        "Exclusive banners must default to OFF"
    );
    assert!(
        !state.notif_over_fullscreen,
        "Fullscreen notification overlay must default to OFF"
    );
    assert!(!state.notification_overlays_fullscreen());

    state.set_system_toast(
        "Steam".to_string(),
        "Achievement Unlocked".to_string(),
        "Mahamatra General".to_string(),
    );
    assert!(
        !state.notification_overlays_fullscreen(),
        "Should not overlay fullscreen while notif_over_fullscreen is OFF"
    );

    state.apply_setting_action(Action::ToggleNotifOverFullscreen);
    assert!(state.notif_over_fullscreen);
    assert!(
        state.notification_overlays_fullscreen(),
        "Active toast must overlay fullscreen when notif_over_fullscreen is ON"
    );

    // Render long multi-line wrapped lyrics across all 4 styles & 3 sizes
    state.notification = None;
    state.set_hovered(true);
    state.mode = NotchMode::Music;
    state.show_lyrics = true;
    state.lyrics[2].text = long_lyric.to_string();
    state.current_lyric_index = Some(2);
    for style in [
        LyricStyle::ClassicList,
        LyricStyle::KaraokeGlow,
        LyricStyle::MinimalSingle,
        LyricStyle::CompactStream,
    ] {
        for size in [LyricSize::Compact, LyricSize::Standard, LyricSize::Large] {
            state.lyric_style = style;
            state.lyric_size = size;
            state.sync_preview_settled();
            let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
            r.render_centered(&state, None);
            assert!(
                !r.hits.is_empty(),
                "Wrapped lyrics view should render cleanly for {:?}/{:?}",
                style,
                size
            );
        }
    }
}

#[test]
fn test_v21_notification_banner_popout_dismiss_and_settings() {
    for placement in [NotchPlacement::Top, NotchPlacement::Right] {
        let mut state = AppState::new_for_preview();
        state.placement = placement;
        state.minimized_island = true;
        state.set_hovered(false);
        state.sync_preview_settled();

        let (idle_w, idle_h) = if placement.is_vertical() {
            (
                AppState::MINIMIZED_VERT_WIDTH,
                AppState::MINIMIZED_VERT_HEIGHT,
            )
        } else {
            (AppState::MINIMIZED_WIDTH, AppState::MINIMIZED_HEIGHT)
        };
        assert_eq!((state.target_width, state.target_height), (idle_w, idle_h));

        // Trigger Test Banner -> must pop out to readable banner size even while minimized
        let eff = state
            .apply_setting_action(Action::SendTestNotification)
            .expect("SendTestNotification must return effects");
        assert!(eff.redraw_notch);
        assert!(state.notification.is_some());
        let (exp_bw, exp_bh) = if placement.is_vertical() {
            (
                (AppState::VERT_BANNER_WIDTH * state.banner_scale()).round(),
                (AppState::VERT_BANNER_HEIGHT * state.banner_scale()).round(),
            )
        } else {
            (
                (AppState::BANNER_WIDTH * state.banner_scale()).round(),
                (AppState::BANNER_HEIGHT * state.banner_scale()).round(),
            )
        };
        assert_eq!(
            (state.target_width, state.target_height),
            (exp_bw, exp_bh),
            "Incoming notification must pop minimized {:?} capsule out to banner size",
            placement
        );

        state.sync_preview_settled();
        let cw = state.canvas_width();
        let ch = state.canvas_height();
        let mut r = Renderer::new(cw, ch);
        r.render_centered(&state, None);

        let dismiss_hit = r
            .hits
            .iter()
            .find(|h| h.action == Action::DismissNotification)
            .expect("Notification banner must expose clickable DismissNotification hit target");
        assert!(
            dismiss_hit.rect.left() >= 0.0
                && dismiss_hit.rect.top() >= 0.0
                && dismiss_hit.rect.right() <= cw as f32 + 1.0
                && dismiss_hit.rect.bottom() <= ch as f32 + 1.0,
            "Dismiss hit {:?} must lie inside banner canvas ({}x{})",
            dismiss_hit.rect,
            cw,
            ch
        );

        // Dismissing returns cleanly to minimized dimensions
        state.apply_setting_action(Action::DismissNotification);
        assert!(state.notification.is_none());
        assert_eq!((state.target_width, state.target_height), (idle_w, idle_h));

        // Expanded/pinned shell also shows inline toast overlay with dismiss action
        state.set_hovered(true);
        state.set_system_toast(
            "WhatsApp".to_string(),
            "Rina".to_string(),
            "Meeting pindah ke jam 3 sore ya".to_string(),
        );
        state.sync_preview_settled();
        let mut r_exp = Renderer::new(state.canvas_width(), state.canvas_height());
        r_exp.render_centered(&state, None);
        assert!(
            r_exp
                .hits
                .iter()
                .any(|h| h.action == Action::DismissNotification),
            "Expanded/pinned {:?} shell must still display dismissible toast overlay",
            placement
        );
    }

    // Notifications settings pane renders Test Banner + Win Notif Settings without overlap
    let mut state = AppState::new_for_preview();
    state.notif_listener_status = "Connected".to_string();
    state.set_settings_category(SettingsCategory::Notifications);
    state.sync_preview_settled();
    let sw = AppState::SETTINGS_WIDTH as u32;
    let sh = AppState::SETTINGS_HEIGHT as u32;
    let mut pm = Pixmap::new(sw, sh).unwrap();
    let mut mask = Mask::new(sw, sh).unwrap();
    let mut fonts = FontRenderer::new();
    let (hits, _) = render_settings_pixmap_with_meta(&state, None, &mut pm, &mut mask, &mut fonts);
    let tb = hits
        .iter()
        .find(|h| h.action == Action::SendTestNotification)
        .expect("Notifications pane must expose Test Banner button");
    let wn = hits
        .iter()
        .find(|h| h.action == Action::OpenWinNotifSettings)
        .expect("Notifications pane must expose Win Notif Settings button");
    assert!(
        tb.rect.right() <= wn.rect.left(),
        "Test Banner ({:?}) must not overlap Win Notif Settings ({:?})",
        tb.rect,
        wn.rect
    );
}

#[test]
fn test_v22_desktop_type_scale_and_banner_readability() {
    for raw in [9.4f32, 10.0, 12.2, 13.6, 18.0] {
        let c = TypeScale::Compact.scale_px(raw);
        let d = TypeScale::Standard.scale_px(raw);
        let l = TypeScale::Large.scale_px(raw);
        let xl = TypeScale::ExtraLarge.scale_px(raw);
        assert!(
            c < d && d < l && l < xl,
            "Type scale must remain strictly monotonic for raw={}: {}, {}, {}, {}",
            raw,
            c,
            d,
            l,
            xl
        );
    }
    assert!(
        TypeScale::Compact.caption_lift_px(9.4) >= 10.6,
        "Compact captions must still meet desktop legibility >= 10.6px (got {})",
        TypeScale::Compact.caption_lift_px(9.4)
    );
    assert!(
        TypeScale::Standard.scale_px(13.6) >= 15.0,
        "Desktop notification title scale must be at least ~15px (got {})",
        TypeScale::Standard.scale_px(13.6)
    );
    assert!(
        TypeScale::ExtraLarge.scale_px(13.6) >= 19.0,
        "X-Large notification title scale must be visibly extra-large >= 19px (got {})",
        TypeScale::ExtraLarge.scale_px(13.6)
    );

    let mut state = AppState::new_for_preview();
    assert_eq!(state.type_scale, TypeScale::Standard);
    let eff = state
        .apply_setting_action(Action::SetTypeScale(TypeScale::ExtraLarge))
        .expect("SetTypeScale must return effects");
    assert!(eff.redraw_notch);
    assert_eq!(state.type_scale, TypeScale::ExtraLarge);

    // Legacy config without `type_scale` defaults cleanly to Desktop (`Standard`)
    let legacy = r#"{"chime_enabled": true, "water_interval_mins": 60, "clean_working_sets": true, "clean_system_ws": true, "clean_standby_list": true, "when_above_pct": 80, "every_mins": 30}"#;
    assert!(state.apply_config_json(legacy));
    assert_eq!(state.type_scale, TypeScale::Standard);

    // Responsive Settings buttons, Expanded shell, Idle capsule, and Notification Banners
    // scale cleanly across all 4 TypeScale presets with strictly growing button & UI geometry
    let mut fonts = FontRenderer::new();
    let mut prev_dims: Option<(u32, u32, f32, f32, f32, f32)> = None;

    for ts in [
        TypeScale::Compact,
        TypeScale::Standard,
        TypeScale::Large,
        TypeScale::ExtraLarge,
    ] {
        state.apply_setting_action(Action::SetTypeScale(ts));
        state.set_settings_category(SettingsCategory::General);
        state.sync_preview_settled();

        let (sw, sh, _) = settings_dims_for_state(&state);
        let mut pm = Pixmap::new(sw, sh).unwrap();
        let mut mask = Mask::new(sw, sh).unwrap();
        let (hits, _) =
            render_settings_pixmap_with_meta(&state, None, &mut pm, &mut mask, &mut fonts);
        let ts_hit = hits
            .iter()
            .find(|h| h.action == Action::SetTypeScale(ts))
            .unwrap_or_else(|| panic!("TypeScale button {:?} must be rendered & clickable", ts));
        let btn_h = ts_hit.rect.height();
        for h in &hits {
            assert!(
                h.rect.left() >= 0.0
                    && h.rect.top() >= 0.0
                    && h.rect.right() <= sw as f32 + 0.5
                    && h.rect.bottom() <= sh as f32 + 0.5,
                "Responsive settings hit out of bounds for {:?}: {:?}",
                ts,
                h.rect
            );
        }

        // Idle (non-minimized) & Expanded also grow responsively
        state.minimized_island = false;
        state.set_hovered(false);
        state.notification = None;
        state.sync_preview_settled();
        let idle_w = state.idle_width();
        let exp_w = state.expanded_width();

        // Notification Banner + Dismiss button grow cleanly across TypeScales
        state.set_system_toast(
            "Discord".to_string(),
            "Desktop Notification Scale".to_string(),
            "Notifikasi Windows real dengan font desktop yang jauh lebih nyaman dibaca".to_string(),
        );
        state.sync_preview_settled();
        let ban_w = state.collapsed_width();
        let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
        r.render_centered(&state, None);
        let dis_hit = r
            .hits
            .iter()
            .find(|h| h.action == Action::DismissNotification)
            .unwrap_or_else(|| panic!("Banner must remain dismissible under TypeScale::{:?}", ts));
        let dis_h = dis_hit.rect.height();

        if let Some((psw, psh, pbh, pidle, pexp, pban)) = prev_dims {
            assert!(
                sw > psw && sh > psh,
                "Settings window dims must grow monotonically for {:?}: {}x{} vs {}x{}",
                ts,
                sw,
                sh,
                psw,
                psh
            );
            assert!(
                btn_h > pbh + 1.5,
                "Settings button height must grow responsively for {:?}: {} vs {}",
                ts,
                btn_h,
                pbh
            );
            assert!(
                idle_w > pidle && exp_w > pexp && ban_w > pban,
                "Island/Expanded/Banner widths must grow monotonically for {:?}",
                ts
            );
        }
        assert!(dis_h >= 28.0 * ts.ui_scale() - 1.0);
        prev_dims = Some((sw, sh, btn_h, idle_w, exp_w, ban_w));
    }
}

#[test]
fn test_v24_dock_edge_switch_no_panic_across_all_states() {
    use cynotch::models::IdleMetric;

    let metrics = [
        IdleMetric::Minimal,
        IdleMetric::Ram,
        IdleMetric::Cpu,
        IdleMetric::NetSpeed,
        IdleMetric::Clock,
    ];
    let placements = [
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
        NotchPlacement::Top,
    ];

    for &minimized in &[true, false] {
        for &style in &[NotchStyle::Floating, NotchStyle::EdgeFlush] {
            for &metric in &metrics {
                for &hovered in &[false, true] {
                    let mut state = AppState::new_for_preview();
                    state.minimized_island = minimized;
                    state.notch_style = style;
                    state.idle_metric = metric;
                    state.set_hovered(hovered);
                    state.sync_preview_settled();

                    let mut r = Renderer::new(state.canvas_width(), state.canvas_height());
                    for &next_pl in &placements {
                        state.apply_setting_action(Action::SetPlacement(next_pl));
                        r.resize(state.canvas_width(), state.canvas_height());
                        r.render_centered(&state, None);

                        // Also test extreme small transition dims (e.g. mid-morph horizontal->vertical)
                        if !hovered {
                            state.current_width = 86.0;
                            state.current_height = 18.0;
                            r.resize(state.canvas_width(), state.canvas_height());
                            r.render_centered(&state, None);
                            state.current_width = 18.0;
                            state.current_height = 86.0;
                            r.resize(state.canvas_width(), state.canvas_height());
                            r.render_centered(&state, None);
                            state.sync_preview_settled();
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_v24_idle_size_presets_horizontal_and_vertical_both_modes() {
    let presets = [
        NotchSizePreset::Compact,
        NotchSizePreset::Standard,
        NotchSizePreset::Wide,
    ];

    for &minimized in &[false, true] {
        // Horizontal (Top)
        let mut prev_w = 0.0f32;
        for &preset in &presets {
            let mut st = AppState::new_for_preview();
            st.placement = NotchPlacement::Top;
            st.minimized_island = minimized;
            st.set_hovered(false);
            st.apply_setting_action(Action::SetSizePreset(preset));
            assert!(
                st.target_width > prev_w,
                "Horizontal {:?} minimized={} width {} must exceed prev {}",
                preset,
                minimized,
                st.target_width,
                prev_w
            );
            assert_eq!(st.target_width, st.idle_width());
            assert_eq!(st.target_height, st.idle_height());
            st.sync_preview_settled();
            let mut r = Renderer::new(st.canvas_width(), st.canvas_height());
            r.render_centered(&st, None);
            prev_w = st.target_width;
        }

        // Vertical (Left / Right)
        for &pl in &[NotchPlacement::Left, NotchPlacement::Right] {
            let mut prev_h = 0.0f32;
            let mut prev_vw = 0.0f32;
            for &preset in &presets {
                let mut st = AppState::new_for_preview();
                st.placement = pl;
                st.minimized_island = minimized;
                st.set_hovered(false);
                st.apply_setting_action(Action::SetSizePreset(preset));
                assert!(
                    st.target_height > prev_h && st.target_width > prev_vw,
                    "Vertical {:?} {:?} minimized={} dims {}x{} must exceed prev {}x{}",
                    pl,
                    preset,
                    minimized,
                    st.target_width,
                    st.target_height,
                    prev_vw,
                    prev_h
                );
                assert_eq!(st.target_width, st.idle_width());
                assert_eq!(st.target_height, st.idle_height());
                st.sync_preview_settled();
                let mut r = Renderer::new(st.canvas_width(), st.canvas_height());
                r.render_centered(&st, None);
                prev_h = st.target_height;
                prev_vw = st.target_width;
            }
        }
    }
}

#[test]
fn test_v24_dynamic_hitbox_and_window_region_accuracy() {
    use cynotch::window::compute_window_region_spec;

    for &pl in &[
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
    ] {
        for &style in &[NotchStyle::Floating, NotchStyle::EdgeFlush] {
            let mut st = AppState::new_for_preview();
            st.placement = pl;
            st.notch_style = style;
            st.minimized_island = false;
            st.set_hovered(false);
            st.sync_preview_settled();

            let idle_rgn = compute_window_region_spec(&st);
            let idle_area = (idle_rgn.2 - idle_rgn.0) * (idle_rgn.3 - idle_rgn.1);

            let cw = st.canvas_width() as f32;
            let ch = st.canvas_height() as f32;
            let (ox, oy) = Renderer::compute_origin(&st, cw, ch);
            let w = st.current_width;
            let h = st.current_height;

            // Center of visible idle pill must be clickable
            assert!(Renderer::point_in_capsule(
                &st,
                cw,
                ch,
                ox + w * 0.5,
                oy + h * 0.5
            ));

            // Transparent area outside visible pill inside canvas must pass clicks through
            match pl {
                NotchPlacement::Top => {
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        (ox - 6.0).max(1.0),
                        oy + h * 0.5
                    ));
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        ox + w + 6.0,
                        oy + h * 0.5
                    ));
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        ox + w * 0.5,
                        oy + h + 6.0
                    ));
                }
                NotchPlacement::Left => {
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        ox + w + 6.0,
                        oy + h * 0.5
                    ));
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        ox + w * 0.5,
                        (oy - 6.0).max(1.0)
                    ));
                }
                NotchPlacement::Right => {
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        (ox - 6.0).max(1.0),
                        oy + h * 0.5
                    ));
                    assert!(!Renderer::point_in_capsule(
                        &st,
                        cw,
                        ch,
                        ox + w * 0.5,
                        oy + h + 6.0
                    ));
                }
            }

            // Rounded outer corner test for Floating pill
            if style == NotchStyle::Floating {
                assert!(!Renderer::point_in_capsule(&st, cw, ch, ox + 0.2, oy + 0.2));
            }

            // Expanded state dynamically expands region & hitbox
            st.set_hovered(true);
            st.sync_preview_settled();
            let exp_rgn = compute_window_region_spec(&st);
            let exp_area = (exp_rgn.2 - exp_rgn.0) * (exp_rgn.3 - exp_rgn.1);
            assert!(
                exp_area > idle_area * 3,
                "Expanded region area {} must vastly exceed idle region area {} for {:?}",
                exp_area,
                idle_area,
                pl
            );
            let ecw = st.canvas_width() as f32;
            let ech = st.canvas_height() as f32;
            let (eox, eoy) = Renderer::compute_origin(&st, ecw, ech);
            assert!(Renderer::point_in_capsule(
                &st,
                ecw,
                ech,
                eox + st.current_width * 0.5,
                eoy + st.current_height * 0.5
            ));

            // Minimized state tightens region further
            st.set_hovered(false);
            st.minimized_island = true;
            st.sync_collapsed_targets();
            st.sync_preview_settled();
            let min_rgn = compute_window_region_spec(&st);
            let min_area = (min_rgn.2 - min_rgn.0) * (min_rgn.3 - min_rgn.1);
            assert!(
                min_area < idle_area,
                "Minimized region {} must be tighter than idle {}",
                min_area,
                idle_area
            );
        }
    }
}

// -- Action Center poll gating ------------------------------------------------
// `GetNotificationsAsync` marshals every toast across RPC (~1 ms CPU per toast,
// measured 25-35 ms with 21 toasts). Polling it once per second is the largest
// idle cost, so the poll is gated on a cheap DB signature with a forced refresh.

#[test]
fn notif_poll_runs_on_first_tick_and_when_db_signature_changes() {
    use cynotch::notifications::{NotifDbSignature, should_poll_toasts};

    let absent: NotifDbSignature = [None; 3];
    let sig_a: NotifDbSignature = [Some((100, 4096)), Some((200, 512)), Some((300, 32))];
    let sig_b: NotifDbSignature = [Some((101, 4096)), Some((200, 512)), Some((300, 32))];

    // First tick after connect: no baseline yet.
    assert!(should_poll_toasts(None, &sig_a, 0));
    // Signature changed (a toast arrived): poll immediately.
    assert!(should_poll_toasts(Some(sig_a), &sig_b, 0));
    // Unchanged signature, well inside the forced-refresh window: skip.
    assert!(!should_poll_toasts(Some(sig_a), &sig_a, 4));
    // Unchanged signature, forced refresh due: poll anyway.
    assert!(should_poll_toasts(Some(sig_a), &sig_a, 5));
    // DB unavailable (non-standard install): always poll, never starve banners.
    assert!(should_poll_toasts(Some(absent), &absent, 0));
}
// -- State directory resolution ----------------------------------------------
// Crash/error logs used to be written to a hardcoded path inside a different
// app's install folder, so diagnostics were lost whenever that folder was
// absent. Everything the app writes must resolve from LOCALAPPDATA instead.

#[test]
fn appdata_dir_resolves_from_localappdata() {
    let dir = cynotch::models::appdata_dir();
    let base = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA is set on Windows");
    assert_eq!(dir, std::path::PathBuf::from(&base).join("CyNotch"));
    // It must be derived from the environment, never from another app's folder.
    assert!(dir.starts_with(std::path::PathBuf::from(&base)));
    let shown = dir.to_string_lossy().to_lowercase();
    assert!(
        !shown.contains("gemini"),
        "state dir leaked into another app: {shown}"
    );
}

#[test]
fn crash_log_path_lives_in_appdata_dir() {
    let path = cynotch::models::crash_log_path("panic.txt");
    assert_eq!(
        path.parent(),
        Some(cynotch::models::appdata_dir().as_path())
    );
    assert_eq!(path.file_name().unwrap(), "panic.txt");
}

#[test]
fn test_v26_frosted_opacity_blur_exclusivity() {
    use cynotch::models::{Action, AppState};
    use cynotch::painter::tok;

    let mut st = AppState::new_for_preview();
    assert_eq!(st.frosted_opacity_pct, 68);
    assert_eq!(st.frosted_blur_px, 20);

    st.frosted_glass_enabled = true;
    st.windows_transparency_enabled = false;
    assert!(!st.is_frosted_active());

    st.windows_transparency_enabled = true;
    assert!(st.is_frosted_active());

    let eff_op = st
        .apply_setting_action(Action::SetFrostedOpacity(52))
        .expect("SetFrostedOpacity must return SettingEffects");
    assert!(eff_op.sync_frosted_window);
    assert_eq!(st.frosted_opacity_pct, 52);
    st.apply_setting_action(Action::SetFrostedOpacity(10));
    assert_eq!(st.frosted_opacity_pct, 25);

    let eff_blur = st
        .apply_setting_action(Action::SetFrostedBlur(32))
        .expect("SetFrostedBlur must return SettingEffects");
    assert!(eff_blur.sync_frosted_window);
    assert_eq!(st.frosted_blur_px, 32);
    st.apply_setting_action(Action::SetFrostedBlur(99));
    assert_eq!(st.frosted_blur_px, 40);
    let low = tok::scale_frosted(tok::obsidian_shell_frosted(), 35);
    let high = tok::scale_frosted(tok::obsidian_shell_frosted(), 95);
    assert!(low.alpha() < high.alpha());
}

#[test]
fn test_corner_dock_radii_and_minimized_origin_alignment() {
    use cynotch::models::{Action, AppState, NotchPlacement, NotchStyle};
    use cynotch::renderer::Renderer;

    let mut st = AppState::new_for_preview();
    st.placement = NotchPlacement::Top;
    st.notch_style = NotchStyle::Floating;
    st.set_hovered(false);
    st.minimized_island = true;
    st.sync_collapsed_targets();
    st.sync_preview_settled();

    assert!(!st.is_corner_docked());
    let (rtl, rtr, rbr, rbl) = st.corner_radii();
    assert!(rtl > 4.0 && rtr > 4.0 && rbr > 4.0 && rbl > 4.0);

    // Dock Top-Left corner: TL/TR/BL sharpen to 0 while inner BR corner stays rounded,
    // and minimized idle island origin tucks flush to (0, 0).
    st.target_anchor_x = 0.0;
    st.target_anchor_y = 0.0;
    st.sync_preview_settled();
    assert!(st.is_top_left_corner_docked());
    assert!(st.is_corner_docked());
    let (rtl, rtr, rbr, rbl) = st.corner_radii();
    assert!(rtl < 1.0 && rtr < 1.0 && rbl < 1.0 && rbr > 4.0);
    let cw = st.canvas_width() as f32;
    let ch = st.canvas_height() as f32;
    let (ox, oy) = Renderer::compute_origin(&st, cw, ch);
    assert!(ox.abs() < 0.01 && oy.abs() < 0.01);

    // Dock Top-Right corner: TL/TR/BR sharpen to 0 while inner BL corner stays rounded,
    // and minimized idle island tucks flush to right canvas edge.
    st.target_anchor_x = 1.0;
    st.target_anchor_y = 0.0;
    st.sync_preview_settled();
    assert!(st.is_top_right_corner_docked());
    let (rtl, rtr, rbr, rbl) = st.corner_radii();
    assert!(rtl < 1.0 && rtr < 1.0 && rbr < 1.0 && rbl > 4.0);
    let (ox, oy) = Renderer::compute_origin(&st, cw, ch);
    assert!((ox + st.current_width - cw).abs() < 0.5 && oy.abs() < 0.01);

    // Liquid transition: ResetPosition updates targets immediately without teleporting anim_anchor_x.
    st.apply_setting_action(Action::ResetPosition);
    assert!((st.target_anchor_x - 0.5).abs() < 0.01);
    assert!(
        st.anim_anchor_x > 0.85,
        "Gentle motion must animate anchor smoothly rather than teleporting"
    );
    st.update_animation(0.016);
    assert!(st.anim_anchor_x < 1.0 && st.anim_anchor_x > 0.5);
    for _ in 0..80 {
        st.update_animation(0.016);
    }
    assert!((st.anim_anchor_x - 0.5).abs() < 0.01);
}

#[test]
fn high_refresh_120hz_unlocks_equalizer_and_mascot_timers() {
    use cynotch::models::AppState;

    let mut st = AppState::new_for_preview();
    st.fps_cap = 0;
    st.set_hovered(false);
    st.minimized_island = false;
    st.notification = None;
    st.mem_reduct.is_purging = false;

    // 1. Idle mascot at 120 Hz vs 60 Hz
    st.show_mascot = true;
    st.equalizer_enabled = false;
    st.media.is_playing = false;
    st.sync_collapsed_targets();
    st.sync_preview_settled();

    st.detected_hz = 120;
    assert_eq!(st.needs_animation_timer_ms(), Some(8));
    st.detected_hz = 60;
    assert_eq!(st.needs_animation_timer_ms(), Some(28));

    // 2. Playing equalizer at 120 Hz vs 60 Hz
    st.show_mascot = false;
    st.equalizer_enabled = true;
    st.media.is_playing = true;
    st.media.title = "Track".to_string();
    st.sync_collapsed_targets();
    st.sync_preview_settled();

    st.detected_hz = 120;
    assert_eq!(st.needs_animation_timer_ms(), Some(8));
    st.detected_hz = 60;
    assert_eq!(st.needs_animation_timer_ms(), Some(20));
}

#[test]
fn test_v27_grip_right_placement_nonblank_drag_and_frosted_glass() {
    use cynotch::models::{Action, AppState, NotchMode, NotchPlacement};
    use cynotch::renderer::Renderer;

    // 1. Grip handle is always to the right of Settings in Top, Left, and Right placements
    for placement in [
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
    ] {
        let mut st = AppState::new_for_preview();
        st.mode = NotchMode::Home;
        st.apply_setting_action(Action::SetPlacement(placement));
        st.set_hovered(true);
        st.sync_preview_settled();

        let mut r = Renderer::new(st.canvas_width(), st.canvas_height());
        r.render_centered(&st, None);
        let grip = r
            .hits
            .iter()
            .find(|h| h.action == Action::StartGripDrag)
            .expect("Missing StartGripDrag hit target");
        let gear = r
            .hits
            .iter()
            .find(|h| h.action == Action::OpenSettings)
            .expect("Missing OpenSettings hit target");
        assert!(
            grip.rect.left() > gear.rect.left(),
            "Expected Grip to the right of Settings in {:?} (grip={}, gear={})",
            placement,
            grip.rect.left(),
            gear.rect.left()
        );
    }

    // 2. Settled expanded cross-axis size never blanks out content into an empty shell
    let mut st = AppState::new_for_preview();
    st.apply_setting_action(Action::SetPlacement(NotchPlacement::Left));
    st.set_hovered(true);
    st.sync_preview_settled();
    st.current_width = 620.0;
    st.current_height = 232.0;
    let mut r_drag = Renderer::new(620, 232);
    r_drag.render_centered(&st, None);
    assert!(
        !r_drag.hits.is_empty(),
        "Expanded cross-axis transition should keep UI controls visible rather than blanking out"
    );

    // 3. With frosted glass active at user 82% opacity, shell + card interior remains genuinely translucent
    let mut st_op = AppState::new_for_preview();
    st_op.mode = NotchMode::Home;
    st_op.set_hovered(true);
    st_op.sync_preview_settled();
    st_op.frosted_glass_enabled = false;
    st_op.windows_transparency_enabled = false;
    let mut r_op = Renderer::new(st_op.canvas_width(), st_op.canvas_height());
    r_op.render_centered(&st_op, None);

    let mut st_fr = st_op.clone();
    st_fr.frosted_glass_enabled = true;
    st_fr.windows_transparency_enabled = true;
    st_fr.frosted_opacity_pct = 82;
    assert!(st_fr.is_frosted_active());
    let mut r_fr = Renderer::new(st_fr.canvas_width(), st_fr.canvas_height());
    r_fr.render_centered(&st_fr, None);

    // Sample interior of right-side Live Network card away from text strokes (~x=280, y=100)
    let sx = 280usize;
    let sy = 100usize;
    let idx = (sy * r_fr.pixmap.width() as usize + sx) * 4;
    let opaque_a = r_op.pixmap.data()[idx + 3];
    let frosted_a = r_fr.pixmap.data()[idx + 3];
    assert!(
        opaque_a >= 235,
        "Expected opaque card alpha >= 235, got {}",
        opaque_a
    );
    assert!(
        frosted_a < opaque_a,
        "Expected frosted card alpha ({}) < opaque card alpha ({})",
        frosted_a,
        opaque_a
    );
    assert!(
        frosted_a < 235 && frosted_a >= 80,
        "Expected translucent frosted card alpha in [80, 235), got {}",
        frosted_a
    );
}

#[test]
fn test_v28_remove_windows_borders_toggle_and_settings_hit() {
    use cynotch::font::FontRenderer;
    use cynotch::models::{Action, AppState, SettingsCategory, ToggleId};
    use cynotch::settings_window::render_settings_pixmap_with_meta;
    use tiny_skia::{Mask, Pixmap};

    let mut st = AppState::new_for_preview();
    assert!(!st.remove_windows_borders);
    assert!(!st.toggle_target(ToggleId::RemoveWindowsBorders));

    let eff = st
        .apply_setting_action(Action::ToggleRemoveWindowsBorders)
        .expect("ToggleRemoveWindowsBorders must return SettingEffects");
    assert_eq!(eff.sync_windows_borders, Some(true));
    assert!(st.remove_windows_borders);
    assert!(st.toggle_target(ToggleId::RemoveWindowsBorders));

    let eff2 = st
        .apply_setting_action(Action::ToggleRemoveWindowsBorders)
        .expect("ToggleRemoveWindowsBorders off");
    assert_eq!(eff2.sync_windows_borders, Some(false));
    assert!(!st.remove_windows_borders);

    // Verify General settings pane renders the toggle hit target cleanly when scrolled
    st.settings_category = SettingsCategory::General;
    let w = AppState::SETTINGS_WIDTH as u32;
    let h = AppState::SETTINGS_HEIGHT as u32;
    let mut pm = Pixmap::new(w, h).unwrap();
    let mut mask = Mask::new(w, h).unwrap();
    let mut fonts = FontRenderer::new();
    let (_hits, max_scroll) =
        render_settings_pixmap_with_meta(&st, None, &mut pm, &mut mask, &mut fonts);
    st.set_settings_scroll(SettingsCategory::General, max_scroll);
    let (scrolled_hits, _) =
        render_settings_pixmap_with_meta(&st, None, &mut pm, &mut mask, &mut fonts);
    assert!(
        scrolled_hits
            .iter()
            .any(|hit| hit.action == Action::ToggleRemoveWindowsBorders),
        "Expected ToggleRemoveWindowsBorders hit target in General settings pane"
    );
}

#[test]
fn test_v29_frosted_blur_toggle_in_settings_window() {
    use cynotch::font::FontRenderer;
    use cynotch::models::{Action, AppState, SettingsCategory, ToggleId};
    use cynotch::settings_window::render_settings_pixmap_with_meta;
    use tiny_skia::{Mask, Pixmap};

    let mut st = AppState::new_for_preview();
    let initial = st.frosted_glass_enabled;
    assert_eq!(st.toggle_target(ToggleId::FrostedGlass), initial);

    let eff = st
        .apply_setting_action(Action::ToggleFrostedGlass)
        .expect("ToggleFrostedGlass must return SettingEffects");
    assert!(eff.sync_frosted_window);
    assert!(eff.redraw_notch);
    assert_eq!(st.frosted_glass_enabled, !initial);

    st.settings_category = SettingsCategory::General;
    let w = AppState::SETTINGS_WIDTH as u32;
    let h = AppState::SETTINGS_HEIGHT as u32;
    let mut pm = Pixmap::new(w, h).unwrap();
    let mut mask = Mask::new(w, h).unwrap();
    let mut fonts = FontRenderer::new();
    let (_hits, max_scroll) =
        render_settings_pixmap_with_meta(&st, None, &mut pm, &mut mask, &mut fonts);
    st.set_settings_scroll(SettingsCategory::General, max_scroll);
    let (scrolled_hits, _) =
        render_settings_pixmap_with_meta(&st, None, &mut pm, &mut mask, &mut fonts);
    assert!(
        scrolled_hits
            .iter()
            .any(|hit| hit.action == Action::ToggleFrostedGlass),
        "Expected ToggleFrostedGlass hit target in General settings pane"
    );
    assert!(
        scrolled_hits
            .iter()
            .any(|hit| matches!(hit.action, Action::SetFrostedOpacity(_))),
        "Expected SetFrostedOpacity hit target in General settings pane"
    );
    assert!(
        scrolled_hits
            .iter()
            .any(|hit| matches!(hit.action, Action::SetFrostedBlur(_))),
        "Expected SetFrostedBlur hit target in General settings pane"
    );
}

#[test]
fn test_notification_banner_slides_from_placement_edge() {
    let mut r = Renderer::new(1, 1);
    for placement in [
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
    ] {
        let mut st = AppState::new_for_preview();
        st.placement = placement;
        st.minimized_island = true;
        st.notification_secs = 30;
        st.set_hovered(false);
        st.sync_preview_settled();

        st.set_notification(
            "Slide Test".to_string(),
            "Banner slides from notch edge".to_string(),
            NotificationKind::General,
            true,
        );
        assert_eq!(st.anim_notif_t, 0.0);

        let cw = st.canvas_width() as f32;
        let ch = st.canvas_height() as f32;

        // First animation step snaps to full banner size and starts sliding in
        assert!(st.update_animation(1.0 / 60.0));
        assert_eq!(st.current_width, st.target_width);
        assert_eq!(st.current_height, st.target_height);
        assert!(st.anim_notif_t > 0.0 && st.anim_notif_t < 1.0);

        let (ox_mid, oy_mid) = Renderer::compute_origin(&st, cw, ch);
        match placement {
            NotchPlacement::Top => assert!(
                oy_mid < 0.0,
                "Top banner must slide down from y < 0 (got {oy_mid})"
            ),
            NotchPlacement::Left => assert!(
                ox_mid < 0.0,
                "Left banner must slide right from x < 0 (got {ox_mid})"
            ),
            NotchPlacement::Right => assert!(
                ox_mid > cw - st.current_width,
                "Right banner must slide left from off-right (got {ox_mid} vs {})",
                cw - st.current_width
            ),
        }

        r.resize(cw as u32, ch as u32);
        r.render_centered(&st, None);

        for _ in 0..120 {
            st.update_animation(1.0 / 60.0);
        }
        assert!((st.anim_notif_t - 1.0).abs() < 0.01);
        let (ox_end, oy_end) = Renderer::compute_origin(&st, cw, ch);
        match placement {
            NotchPlacement::Top => assert!((oy_end - 0.0).abs() < 0.5),
            NotchPlacement::Left => assert!((ox_end - 0.0).abs() < 0.5),
            NotchPlacement::Right => {
                assert!((ox_end - (cw - st.current_width).max(0.0)).abs() < 0.5)
            }
        }
        r.render_centered(&st, None);
    }
}

#[test]
fn test_multi_notification_coalescing_queue_and_history() {
    let mut st = AppState::new_for_preview();
    st.set_hovered(false);
    st.sync_preview_settled();

    // 1. First Discord notification opens the banner from idle
    st.push_system_toast(
        "Discord".to_string(),
        "Kai (#general)".to_string(),
        "msg 1".to_string(),
        Some("com.squirrel.Discord.Discord".to_string()),
    );
    assert!(st.notification.is_some());
    assert_eq!(st.notif_queue.len(), 0);
    assert_eq!(st.notif_history.len(), 1);
    assert_eq!(st.notification.as_ref().unwrap().grouped_count, 1);

    // Let banner settle
    for _ in 0..90 {
        st.update_animation(1.0 / 60.0);
    }
    assert!((st.anim_notif_t - 1.0).abs() < 0.02);

    // 2. Second Discord notification coalesces into active banner with subtle pulse (>= 0.86)
    st.push_system_toast(
        "Discord".to_string(),
        "Kai (#general)".to_string(),
        "msg 2".to_string(),
        Some("com.squirrel.Discord.Discord".to_string()),
    );
    assert_eq!(st.notif_queue.len(), 0);
    assert_eq!(st.notif_history.len(), 2);
    assert_eq!(st.notification.as_ref().unwrap().grouped_count, 2);
    assert_eq!(st.notification.as_ref().unwrap().subtitle, "msg 2");
    assert!(
        st.anim_notif_t >= 0.85,
        "Coalescing must keep banner seamlessly open"
    );

    // 3. Different app goes into notif_queue; second message from that app coalesces in queue
    st.push_system_toast(
        "WhatsApp".to_string(),
        "Budi".to_string(),
        "halo 1".to_string(),
        Some("WhatsApp.App".to_string()),
    );
    st.push_system_toast(
        "WhatsApp".to_string(),
        "Budi".to_string(),
        "halo 2".to_string(),
        Some("WhatsApp.App".to_string()),
    );
    assert_eq!(st.notif_queue.len(), 1);
    assert_eq!(st.notif_queue[0].grouped_count, 2);
    assert_eq!(st.notif_queue[0].subtitle, "halo 2");
    assert_eq!(st.notif_history.len(), 4);

    // Render collapsed banner and verify +1 queue badge + ActivateNotification + DismissNotification
    st.sync_preview_settled();
    let mut r = Renderer::new(st.canvas_width() as u32, st.canvas_height() as u32);
    r.render_centered(&st, None);
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::DismissNotification)
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::ClearNotificationQueue)
    );
    assert!(
        r.hits
            .iter()
            .any(|h| h.action == Action::ActivateNotification)
    );

    // 4. Dismissing active banner seamlessly advances to WhatsApp without collapsing anim_notif_t to 0
    st.apply_setting_action(Action::DismissNotification);
    assert_eq!(st.notification.as_ref().unwrap().app_name, "WhatsApp");
    assert_eq!(st.notif_queue.len(), 0);
    assert!(
        st.anim_notif_t >= 0.75,
        "Advancing queue should stay seamlessly open"
    );

    // 5. ClearNotificationQueue clears everything
    st.apply_setting_action(Action::ClearNotificationQueue);
    assert!(st.notification.is_none());
    assert!(st.notif_queue.is_empty());

    // 6. Expanded notification history renders in both Top and Left placements
    for placement in [NotchPlacement::Top, NotchPlacement::Left] {
        st.placement = placement;
        st.set_hovered(true);
        st.show_notif_history = true;
        st.sync_preview_settled();
        let mut rh = Renderer::new(st.canvas_width() as u32, st.canvas_height() as u32);
        rh.render_centered(&st, None);
        assert!(
            rh.hits
                .iter()
                .any(|h| h.action == Action::ToggleNotifHistory)
        );
        assert!(
            rh.hits
                .iter()
                .any(|h| h.action == Action::ClearNotifHistory)
        );
        assert!(
            rh.hits
                .iter()
                .any(|h| h.action == Action::ActivateHistoryItem(0))
        );
        assert!(
            rh.hits
                .iter()
                .any(|h| h.action == Action::DismissHistoryItem(0))
        );
    }
}

#[test]
fn test_notch_backdrop_geom_clips_slide_in_and_corner_dock_sharp_mask() {
    use cynotch::models::{AppState, NotchPlacement, NotchStyle};
    use cynotch::window::notch_backdrop_geom;

    // 1. Corner-docked Floating top-left notch carries sharp_mask != 0 while flush == false
    let mut st_corner = AppState::new_for_preview();
    st_corner.placement = NotchPlacement::Top;
    st_corner.notch_style = NotchStyle::Floating;
    st_corner.target_anchor_x = 0.0;
    st_corner.target_anchor_y = 0.0;
    st_corner.sync_preview_settled();
    assert!(st_corner.is_corner_docked());
    let g_corner = notch_backdrop_geom(
        &st_corner,
        st_corner.canvas_width() as i32,
        st_corner.canvas_height() as i32,
    )
    .expect("settled corner-docked Floating notch should produce backdrop geom");
    assert!(!g_corner.flush);
    assert_ne!(
        g_corner.sharp_mask, 0,
        "corner-docked Floating notch should propagate sharp_mask to backdrop"
    );

    // 2. Sliding notification banners must be clipped to [0, cw] x [0, ch]
    for placement in [
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
    ] {
        let mut st = AppState::new_for_preview();
        st.placement = placement;
        st.notch_style = NotchStyle::Floating;
        st.set_hovered(false);
        st.push_system_toast(
            "CyNotch".to_string(),
            "Clip Check".to_string(),
            "Sliding banner".to_string(),
            None,
        );
        st.current_width = 320.0;
        st.current_height = 68.0;
        st.anim_notif_t = 0.45; // mid-slide: extends outside canvas outer edge
        let cw = st.canvas_width() as i32;
        let ch = st.canvas_height() as i32;
        let g = notch_backdrop_geom(&st, cw, ch)
            .expect("mid-slide notification (>8px visible) should produce clipped geom");
        assert!(g.ox >= 0 && g.oy >= 0);
        assert!(g.ox + g.w <= cw && g.oy + g.h <= ch);
        assert!(g.flush, "clipped outer edge should be marked flush");

        // Early sliver (< MIN_SIDE) returns None instead of bleeding or creating tiny region
        st.anim_notif_t = 0.02;
        assert!(
            notch_backdrop_geom(&st, cw, ch).is_none(),
            "tiny < 8px notification sliver should suppress backdrop for {:?}",
            placement
        );
    }
}
