use cynotch::models::{AppState, ShelfKind};
use std::time::Instant;

#[test]
fn shelf_is_ephemeral_deduped_and_capped_at_24() {
    let mut state = AppState::new();
    assert!(
        state.shelf_items.is_empty(),
        "v0.3.1 Shelf must start empty on restart"
    );

    let id1 = state.push_shelf_item(
        ShelfKind::File,
        "a.png".to_string(),
        "C:\\tmp\\a.png".to_string(),
    );
    let id2 = state.push_shelf_item(
        ShelfKind::Text,
        "hello".to_string(),
        "hello".to_string(),
    );
    // Re-pushing same file dedupes and moves it back to index 0 with same stable ID
    let id1_again = state.push_shelf_item(
        ShelfKind::File,
        "a.png".to_string(),
        "C:\\tmp\\a.png".to_string(),
    );
    assert_eq!(id1, id1_again);
    assert_eq!(state.shelf_items.len(), 2);
    assert_eq!(state.shelf_items[0].id, id1);
    assert_eq!(state.shelf_items[1].id, id2);

    // Push 30 more items -> capped at 24
    for i in 0..30 {
        state.push_shelf_item(
            ShelfKind::Text,
            format!("item-{i}"),
            format!("payload-{i}"),
        );
    }
    assert_eq!(state.shelf_items.len(), 24);
}

#[test]
fn shelf_auto_remove_and_undo_restores_item() {
    let mut state = AppState::new();
    let id = state.push_shelf_item(
        ShelfKind::Url,
        "https://example.com".to_string(),
        "https://example.com".to_string(),
    );
    assert!(!state.has_active_shelf_undo());

    let removed = state.remove_shelf_item_by_id(id, true);
    assert!(removed.is_some());
    assert!(state.shelf_items.is_empty());
    assert!(state.has_active_shelf_undo());

    assert!(state.undo_shelf_remove());
    assert_eq!(state.shelf_items.len(), 1);
    assert_eq!(state.shelf_items[0].id, id);
    assert!(!state.has_active_shelf_undo());
}

#[test]
fn url_detection_and_liquid_plop_physics() {
    assert!(AppState::is_likely_url("https://github.com/cynotch"));
    assert!(AppState::is_likely_url("http://localhost:3000/test"));
    assert!(!AppState::is_likely_url("not a url https://foo.com"));
    assert!(!AppState::is_likely_url("hello world"));

    let mut state = AppState::new();
    let id = state.push_shelf_item(
        ShelfKind::Image,
        "Image_20261121_180000.png".to_string(),
        "C:\\tmp\\Image_20261121_180000.png".to_string(),
    );
    state.shelf_fx.trigger_plop(120.0, 64.0, id);
    let now = Instant::now();
    assert!(state.shelf_fx.is_active(now));
    // At t≈0, buoyancy_offset should be positive (~12px Blur-Fade Rise)
    let bob = state.shelf_fx.buoyancy_offset(id, now);
    assert!(bob > 10.0 && bob <= 12.5, "unexpected initial buoyancy: {bob}");

    // With animations disabled, step snaps FX off cleanly
    state.shelf_fx.step(0.016, false, now);
    assert!(!state.shelf_fx.is_plop_running(now));
}

#[test]
fn shelf_bento_detects_images_and_decodes_png_and_svg_thumbnails() {
    let mut state = AppState::new();
    state.push_dropped_file("C:\\tmp\\mockup.svg".to_string());
    assert!(state.shelf_items[0].is_image());
    assert_eq!(state.shelf_items[0].badge_label(), "SVG");

    state.push_dropped_file("C:\\tmp\\photo.jpeg".to_string());
    assert!(state.shelf_items[0].is_image());
    assert_eq!(state.shelf_items[0].badge_label(), "JPG");

    state.push_shelf_item(
        ShelfKind::Url,
        "https://www.youtube.com/watch?v=123".to_string(),
        "https://www.youtube.com/watch?v=123".to_string(),
    );
    assert_eq!(state.shelf_items[0].badge_label(), "LINK");
    let (host, path) = state.shelf_items[0].url_host_and_path();
    assert_eq!(host, "youtube.com");
    assert_eq!(path, "/watch?v=123");

    // Verify real WIC PNG + SVG thumbnail decoding on disk
    let tmp_dir = std::env::temp_dir().join(format!("cynotch_thumb_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let png_path = tmp_dir.join("sample.png");
    let bgra = vec![200u8; 32 * 24 * 4];
    assert!(cynotch::capture::save_bgra_png(&png_path, 32, 24, &bgra));
    let (pw, ph, pbuf) =
        cynotch::capture::load_image_thumbnail_pbgra(png_path.to_str().unwrap(), 128)
            .expect("decode PNG thumbnail");
    assert_eq!((pw, ph), (32, 24));
    assert_eq!(pbuf.len(), 32 * 24 * 4);

    let svg_path = tmp_dir.join("icon.svg");
    std::fs::write(
        &svg_path,
        r##"<svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="40" fill="#7C3AED"/><path d="M20 50 L80 50" stroke="#FFE248"/></svg>"##,
    )
    .unwrap();
    let (sw, sh, sbuf) =
        cynotch::capture::load_image_thumbnail_pbgra(svg_path.to_str().unwrap(), 120)
            .expect("decode SVG thumbnail");
    assert!(sw > 0 && sh > 0 && sbuf.len() == (sw * sh * 4) as usize);
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn clipboard_and_screenshot_trigger_skenario2_lingering_peak_fast_snap_pulse_without_banner() {
    for minimized in [false, true] {
        let mut state = AppState::new_for_preview();
        state.minimized_island = minimized;
        state.sync_collapsed_targets();
        state.current_width = state.target_width;
        state.current_height = state.target_height;

        state.push_clipboard("copied snippet".to_string(), "VS Code");
        assert!(
            state.notification.is_none(),
            "Ctrl+C must NOT trigger a 4-second popup banner"
        );
        let t0 = state.shelf_fx.clipboard_pulse_at.expect("pulse started");

        // Skenario 2 (Fluid Luxury 0.90s): Immediate ignition + Slow Lingering Peak (0..380ms > 0.90), then Snap-Off tail (840ms < 0.22)
        let (i_start, _) = state
            .shelf_fx
            .clipboard_pulse_curves(t0 + std::time::Duration::from_millis(20))
            .unwrap();
        let (i_linger, _) = state
            .shelf_fx
            .clipboard_pulse_curves(t0 + std::time::Duration::from_millis(320))
            .unwrap();
        let (i_tail, _) = state
            .shelf_fx
            .clipboard_pulse_curves(t0 + std::time::Duration::from_millis(840))
            .unwrap();
        assert!(i_start > 0.99, "expected immediate ignition, got {i_start}");
        assert!(i_linger > 0.90, "expected slow lingering peak, got {i_linger}");
        assert!(i_tail < 0.22, "expected fast snap-off tail, got {i_tail}");
    }

    // Snipping Tool system toast & built-in capture also use Skenario 2 pulse instead of popup banner
    let mut state = AppState::new_for_preview();
    state.shelf_fx.clipboard_pulse_at = None;
    state.push_system_toast(
        "Snipping Tool".to_string(),
        "Screenshot copied to clipboard".to_string(),
        "Select here to mark up and share the image".to_string(),
        Some("Microsoft.ScreenSketch_8wekyb3d8bbwe!App".to_string()),
    );
    assert!(
        state.notification.is_none(),
        "Snipping Tool toast must use Skenario 2 pulse instead of popup banner"
    );
    assert!(state.shelf_fx.clipboard_pulse_at.is_some());

    state.shelf_fx.clipboard_pulse_at = None;
    state.record_capture_pulse(
        "cyshot_full_120000.png".to_string(),
        "C:\\Pictures\\CyNotch\\cyshot_full_120000.png".to_string(),
    );
    assert!(state.notification.is_none());
    assert!(state.shelf_fx.clipboard_pulse_at.is_some());
}

#[test]
fn stealth_idle_morphs_and_animates_seamlessly_when_changing_state_to_music() {
    use cynotch::models::IdleMetric;
    let mut state = AppState::new_for_preview();
    state.media.is_playing = false;
    state.idle_metric = IdleMetric::Minimal;
    state.sync_capsule_state_transition();
    state.current_width = state.target_width;
    state.anim_capsule_t = 1.0;

    let stealth_w = state.idle_width();
    assert!(
        stealth_w < 200.0,
        "Stealth idle width should be compact (mascot + badge only), got {stealth_w}"
    );

    // Transition from Stealth Idle -> Music Playing
    state.media.is_playing = true;
    state.media.title = "Starboy".to_string();
    state.media.artist = "The Weeknd".to_string();
    state.sync_capsule_state_transition();

    assert!(
        state.target_width > stealth_w + 80.0,
        "Music playing must expand target width from stealth {stealth_w} to {}",
        state.target_width
    );
    assert_eq!(
        state.anim_capsule_t, 0.0,
        "State transition must trigger anim_capsule_t crossfade from 0.0"
    );
    state.update_animation(1.0 / 60.0);
    assert!(state.anim_capsule_t > 0.0 && state.anim_capsule_t < 1.0);
    assert!(state.current_width > stealth_w);
}
