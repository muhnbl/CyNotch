use cynotch::models::{Action, AppState, HoverDwellSpeed};
use cynotch::sound::{
    reset_notif_sound_cooldown, try_acquire_notif_sound_slot_at, NOTIFICATION_FIXED_VOLUME,
    NotificationSound, find_data_chunk, play_notification_sound, scale_wav_volume,
};
use std::sync::Mutex;

static SOUND_COOLDOWN_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn notification_sound_serde_roundtrip_and_labels() {
    for sound in NotificationSound::ALL {
        let json = serde_json::to_string(&sound).expect("serialize sound");
        let parsed: NotificationSound = serde_json::from_str(&json).expect("deserialize sound");
        assert_eq!(parsed, sound);
        assert!(!sound.label().is_empty());
        assert!(!sound.display_name().is_empty());
    }
    assert_eq!(NotificationSound::default(), NotificationSound::Snapchat);
}

#[test]
fn notification_sound_legacy_serde_iphone_alias() {
    let json_caps = "\"IPhone\"";
    let json_lower = "\"iphone\"";
    let parsed_caps: NotificationSound = serde_json::from_str(json_caps).expect("deserialize IPhone");
    let parsed_lower: NotificationSound =
        serde_json::from_str(json_lower).expect("deserialize iphone");
    assert_eq!(parsed_caps, NotificationSound::Snapchat);
    assert_eq!(parsed_lower, NotificationSound::Snapchat);
}

#[test]
fn embedded_snapchat_wav_has_valid_pcm_header() {
    assert!(NotificationSound::Mute.wav_bytes().is_none());

    let bytes = NotificationSound::Snapchat
        .wav_bytes()
        .expect("snapchat sound must embed wav");
    assert!(bytes.len() >= 44, "wav too short");
    assert_eq!(&bytes[0..4], b"RIFF", "missing RIFF header");
    assert_eq!(&bytes[8..12], b"WAVE", "missing WAVE header");
    assert_eq!(&bytes[12..16], b"fmt ", "missing fmt chunk");
    let audio_fmt = u16::from_le_bytes([bytes[20], bytes[21]]);
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits = u16::from_le_bytes([bytes[34], bytes[35]]);
    assert_eq!(audio_fmt, 1, "must be PCM");
    assert_eq!(channels, 2, "stereo audio");
    assert_eq!(sample_rate, 44100, "must be 44.1kHz");
    assert_eq!(bits, 16, "must be 16-bit");

    let data_chunk = find_data_chunk(bytes);
    assert!(data_chunk.is_some(), "data chunk must be located");
}

#[test]
fn volume_scaling_scales_pcm_samples_accurately() {
    let bytes = NotificationSound::Snapchat.wav_bytes().expect("snapchat wav");
    let scaled_fixed = scale_wav_volume(bytes, NOTIFICATION_FIXED_VOLUME as f32 / 100.0);
    assert_eq!(bytes.len(), scaled_fixed.len());

    let (start, end) = find_data_chunk(bytes).unwrap();
    for (orig_chunk, scaled_chunk) in bytes[start..end]
        .chunks_exact(2)
        .zip(scaled_fixed[start..end].chunks_exact(2))
    {
        let orig = i16::from_le_bytes([orig_chunk[0], orig_chunk[1]]);
        let scaled = i16::from_le_bytes([scaled_chunk[0], scaled_chunk[1]]);
        if orig.abs() > 100 {
            let expected = ((orig as f32) * 0.25).round() as i16;
            assert!((scaled - expected).abs() <= 1);
            break;
        }
    }
}

#[test]
fn notification_sound_suppression_rules() {
    let _guard = SOUND_COOLDOWN_TEST_LOCK.lock().unwrap();
    reset_notif_sound_cooldown();
    // Muted never plays
    assert!(!play_notification_sound(NotificationSound::Mute, true, false));
    // Disabled notifications never play
    assert!(!play_notification_sound(
        NotificationSound::Snapchat,
        false,
        false
    ));
    // Fullscreen suppression blocks playback
    assert!(!play_notification_sound(
        NotificationSound::Snapchat,
        true,
        true
    ));
    // Valid state plays
    assert!(play_notification_sound(
        NotificationSound::Snapchat,
        true,
        false
    ));
}

#[test]
fn burst_notifications_play_sound_only_once() {
    let _guard = SOUND_COOLDOWN_TEST_LOCK.lock().unwrap();
    // Deterministic slot test (including t = 0 edge case)
    reset_notif_sound_cooldown();
    assert!(try_acquire_notif_sound_slot_at(0, 1800));
    assert!(!try_acquire_notif_sound_slot_at(0, 1800));
    assert!(!try_acquire_notif_sound_slot_at(150, 1800));
    assert!(try_acquire_notif_sound_slot_at(1800, 1800));

    reset_notif_sound_cooldown();
    assert!(try_acquire_notif_sound_slot_at(1000, 1800));
    assert!(!try_acquire_notif_sound_slot_at(1150, 1800));
    assert!(!try_acquire_notif_sound_slot_at(1600, 1800));
    assert!(try_acquire_notif_sound_slot_at(2800, 1800));

    // Live API test
    reset_notif_sound_cooldown();
    assert!(play_notification_sound(
        NotificationSound::Snapchat,
        true,
        false
    ));
    assert!(!play_notification_sound(
        NotificationSound::Snapchat,
        true,
        false
    ));
    assert!(!play_notification_sound(
        NotificationSound::Snapchat,
        true,
        false
    ));
}

#[test]
fn test_notification_sound_action_emits_effect() {
    let mut state = AppState::new_for_preview();
    state.notification_sound = NotificationSound::Snapchat;
    let eff = state
        .apply_setting_action(Action::TestNotificationSound)
        .expect("TestNotificationSound must return SettingEffects");
    assert_eq!(eff.play_sound, Some(NotificationSound::Snapchat));
    assert!(!eff.redraw_notch);
}

#[test]
fn setting_hover_dwell_speed_updates_state_and_dwell_ms() {
    let mut state = AppState::new_for_preview();
    assert_eq!(state.hover_dwell_speed, HoverDwellSpeed::Normal);
    assert_eq!(state.hover_dwell_ms(), 400);

    state.apply_setting_action(Action::SetHoverDwellSpeed(HoverDwellSpeed::Quick));
    assert_eq!(state.hover_dwell_speed, HoverDwellSpeed::Quick);
    assert_eq!(state.hover_dwell_ms(), 120);

    state.apply_setting_action(Action::SetHoverDwellSpeed(HoverDwellSpeed::Slow));
    assert_eq!(state.hover_dwell_speed, HoverDwellSpeed::Slow);
    assert_eq!(state.hover_dwell_ms(), 700);
}

#[test]
fn toggling_cynotch_notifications_syncs_windows_sound_mute_rule() {
    let mut state = AppState::new_for_preview();
    state.notifications_enabled = true;
    state.notif_listener_enabled = true;
    assert!(state.should_mute_windows_notif_sound());

    // Turning off CyNotch notifications must unmute native Windows notification sounds
    let eff_off = state
        .apply_setting_action(Action::ToggleNotifications)
        .expect("ToggleNotifications returns effects");
    assert!(!state.should_mute_windows_notif_sound());
    assert_eq!(eff_off.sync_win_notif_sound_mute, Some(false));

    // Turning CyNotch notifications back ON must mute native Windows notification sounds
    let eff_on = state
        .apply_setting_action(Action::ToggleNotifications)
        .expect("ToggleNotifications returns effects");
    assert!(state.should_mute_windows_notif_sound());
    assert_eq!(eff_on.sync_win_notif_sound_mute, Some(true));
}
