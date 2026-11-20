use cynotch::models::{AppState, NotchMode, SettingsCategory, spring_step};

#[test]
fn spring_step_converges_smoothly_to_target() {
    let mut x = 0.0f32;
    let mut v = 0.0f32;
    for _ in 0..120 {
        let (nx, nv, _) = spring_step(x, v, 100.0, 360.0, 33.0, 1.0 / 60.0, 0.25, 2.0);
        x = nx;
        v = nv;
    }
    assert!((x - 100.0).abs() <= 0.5, "spring did not settle near target: {x}");
    assert!(v.abs() <= 2.5, "spring velocity did not settle: {v}");
}

#[test]
fn windows_animations_off_snaps_all_notch_and_settings_transitions() {
    let mut state = AppState::new_for_preview();
    state.windows_animations_enabled = false;
    state.settings_open = true;

    // Trigger notch expand + mode switch + settings category switch + wheel scroll
    state.set_hovered(true);
    state.set_mode(NotchMode::Music);
    state.set_settings_category(SettingsCategory::Notifications);
    state.set_settings_scroll(SettingsCategory::Notifications, 120.0);

    // Must snap immediately with zero animation timer
    assert_eq!(state.anim_mode_t, 1.0);
    assert_eq!(state.anim_settings_pane_t, 1.0);
    assert_eq!(
        state.anim_settings_cat_y,
        AppState::settings_cat_pill_y(SettingsCategory::Notifications)
    );
    assert_eq!(state.settings_scroll(SettingsCategory::Notifications), 120.0);

    state.update_animation(1.0 / 60.0);
    assert_eq!(state.current_width, state.target_width);
    assert_eq!(state.current_height, state.target_height);
    assert_eq!(state.anim_expand_t, 1.0);
    assert!(state.needs_animation_timer_ms().is_none());
    assert!(state.needs_settings_timer_ms().is_none());
}

#[test]
fn windows_animations_on_drives_spring_motion_and_pane_transitions() {
    let mut state = AppState::new_for_preview();
    state.windows_animations_enabled = true;
    state.settings_open = true;
    state.sync_preview_settled();

    // Notch hover morph & tab switch start animating smoothly
    state.set_hovered(true);
    state.set_mode(NotchMode::Telemetry);
    assert_eq!(state.anim_mode_t, 0.0);
    assert!(state.needs_animation_timer_ms().is_some());

    state.update_animation(1.0 / 60.0);
    assert!(state.anim_mode_t > 0.0 && state.anim_mode_t < 1.0);
    assert!(state.current_width > AppState::IDLE_WIDTH && state.current_width < state.target_width);

    // Settings sidebar pill + pane transition + smooth scroll animate smoothly
    state.set_settings_category(SettingsCategory::System);
    assert_eq!(state.anim_settings_pane_t, 0.0);
    assert!(state.needs_settings_timer_ms().is_some());

    let start_pill_y = state.anim_settings_cat_y;
    let end_pill_y = AppState::settings_cat_pill_y(SettingsCategory::System);
    state.update_settings_animation(1.0 / 60.0);
    assert!(state.anim_settings_pane_t > 0.0 && state.anim_settings_pane_t < 1.0);
    assert!(
        (state.anim_settings_cat_y - start_pill_y).abs() > 0.5
            && (state.anim_settings_cat_y - end_pill_y).abs() > 0.5,
        "sidebar pill should spring-glide smoothly between categories"
    );
}

#[test]
fn frosted_sliders_clamp_and_update_live_without_disk_writes() {
    let mut state = AppState::new_for_preview();
    assert!(state.set_frosted_opacity_live(42));
    assert_eq!(state.frosted_opacity_pct, 42);
    assert!(!state.set_frosted_opacity_live(42));

    assert!(state.set_frosted_opacity_live(5));
    assert_eq!(state.frosted_opacity_pct, 25);

    assert!(state.set_frosted_opacity_live(120));
    assert_eq!(state.frosted_opacity_pct, 95);

    assert!(state.set_frosted_blur_live(28));
    assert_eq!(state.frosted_blur_px, 28);
    assert!(state.set_frosted_blur_live(0));
    assert_eq!(state.frosted_blur_px, 4);
    assert!(state.set_frosted_blur_live(99));
    assert_eq!(state.frosted_blur_px, 40);
}
