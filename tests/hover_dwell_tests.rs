use std::time::{Duration, Instant};

use cynotch::models::AppState;
use cynotch::window::is_idle_collapsed;

#[test]
fn hover_dwell_smoothstep_advances_to_one_at_650ms_and_stays_0hz_without_glow() {
    let mut state = AppState::new_for_preview();
    state.show_mascot = false;
    state.equalizer_enabled = false;
    state.sync_preview_settled();
    assert_eq!(state.hover_dwell_t, 0.0);
    assert_eq!(state.anim_edge_glow_t, 0.0);
    assert!(state.hover_dwell_start.is_none());
    assert!(!state.is_hover_pending);
    assert_eq!(state.needs_animation_timer_ms(), None);

    let t0 = Instant::now();
    state.begin_hover_dwell(t0);
    assert!(state.is_hover_pending);
    assert_eq!(state.hover_dwell_t, 0.0);
    assert_eq!(state.anim_edge_glow_t, 0.0);
    // Hover dwell without edge glow stays 0Hz until expand triggers
    assert_eq!(state.needs_animation_timer_ms(), None);

    // Halfway (325ms): smoothstep(0.5) == 0.5
    let changed_mid = state.update_hover_dwell(t0 + Duration::from_millis(AppState::HOVER_DWELL_MS / 2));
    assert!(changed_mid);
    assert!(
        (state.hover_dwell_t - 0.5).abs() < 0.01,
        "expected ~0.5 at 325ms, got {}",
        state.hover_dwell_t
    );

    // Quarter: smoothstep(0.25) = 0.15625
    let mut state_q = AppState::new_for_preview();
    state_q.begin_hover_dwell(t0);
    state_q.update_hover_dwell(t0 + Duration::from_millis(AppState::HOVER_DWELL_MS / 4));
    assert!(
        (state_q.hover_dwell_t - 0.15625).abs() < 0.01,
        "expected smoothstep(0.25)=0.15625, got {}",
        state_q.hover_dwell_t
    );

    // Full 650ms: reaches 1.0
    let changed_end = state.update_hover_dwell(t0 + Duration::from_millis(AppState::HOVER_DWELL_MS));
    assert!(changed_end);
    assert!((state.hover_dwell_t - 1.0).abs() < f32::EPSILON);

    // Mouse leave / cancel resets dwell immediately, and edge glow stays 0.0
    state.cancel_hover_dwell();
    assert!(!state.is_hover_pending);
    assert_eq!(state.hover_dwell_t, 0.0);
    assert!(state.hover_dwell_start.is_none());
    for _ in 0..20 {
        state.update_animation(0.016);
    }
    assert_eq!(state.anim_edge_glow_t, 0.0);
    assert_eq!(state.needs_animation_timer_ms(), None);
}

#[test]
fn hover_dwell_resets_when_expanded() {
    let mut state = AppState::new_for_preview();
    let t0 = Instant::now();
    state.begin_hover_dwell(t0);
    state.update_hover_dwell(t0 + Duration::from_millis(450));
    assert!(state.hover_dwell_t > 0.5);

    state.set_hovered(true);
    assert!(!state.is_hover_pending);
    assert_eq!(state.hover_dwell_t, 0.0);
    assert!(state.hover_dwell_start.is_none());
}

#[test]
fn idle_collapsed_never_flips_false_to_true_on_hover_pending() {
    // Regression test for the 60Hz hover blink / teleporting ghost bug:
    // With mascot ON (default), idle window is full-canvas (is_idle_collapsed == false).
    // Entering hover pending MUST NOT flip is_idle_collapsed to true!
    let mut state = AppState::new_for_preview();
    state.show_mascot = true;
    state.sync_preview_settled();
    assert!(!is_idle_collapsed(&state));

    state.begin_hover_dwell(Instant::now());
    assert!(
        !is_idle_collapsed(&state),
        "is_idle_collapsed must never flip true while hover is pending"
    );
}

#[test]
fn edge_glow_stays_disabled_during_hover_dwell() {
    let mut state = AppState::new_for_preview();
    state.windows_animations_enabled = true;
    state.sync_preview_settled();

    state.begin_hover_dwell(Instant::now() - Duration::from_millis(AppState::HOVER_DWELL_MS));
    for _ in 0..15 {
        state.update_animation(0.016);
    }
    assert_eq!(state.anim_edge_glow_t, 0.0);

    state.set_hovered(true);
    for _ in 0..18 {
        state.update_animation(0.016);
    }
    assert_eq!(state.anim_edge_glow_t, 0.0);
}
