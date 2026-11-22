use cynotch::font_d2d::DWriteTextCache;
use cynotch::models::{
    Action, AppState, HoverDwellSpeed, NotchMode, NotchPlacement, NotificationKind,
    SettingsCategory, ToggleId, TypeScale,
};
use cynotch::painter_d2d::D2DResourceCache;
use cynotch::renderer::Renderer;
use cynotch::settings_window::{render_settings_d2d, settings_dims_for_state};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{Interface, Result};

fn make_ctx() -> Result<ID2D1DeviceContext> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let dxgi: IDXGIDevice = device.unwrap().cast()?;
        let factory: ID2D1Factory1 =
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dev = factory.CreateDevice(&dxgi)?;
        dev.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
    }
}

fn make_target(ctx: &ID2D1DeviceContext, w: u32, h: u32) -> Result<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
        colorContext: ManuallyDrop::new(None),
    };
    unsafe { ctx.CreateBitmap(D2D_SIZE_U { width: w, height: h }, None, 0, &props) }
}

#[test]
fn qa_iss09_hover_dwell_speed_persists_across_config_json_roundtrip() {
    for speed in [
        HoverDwellSpeed::Quick,
        HoverDwellSpeed::Normal,
        HoverDwellSpeed::Slow,
    ] {
        let mut s = AppState::new_for_preview();
        assert!(s.apply_setting_action(Action::SetHoverDwellSpeed(speed)).is_some());
        assert_eq!(s.hover_dwell_speed, speed);

        let json = s.to_config_json();
        let mut loaded = AppState::new_for_preview();
        loaded.hover_dwell_speed = match speed {
            HoverDwellSpeed::Quick => HoverDwellSpeed::Slow,
            HoverDwellSpeed::Normal => HoverDwellSpeed::Quick,
            HoverDwellSpeed::Slow => HoverDwellSpeed::Normal,
        };
        assert!(loaded.apply_config_json(&json));
        assert_eq!(
            loaded.hover_dwell_speed, speed,
            "HoverDwellSpeed {:?} failed to roundtrip through config JSON",
            speed
        );
    }
}

#[test]
fn qa_iss14_frosted_sliders_auto_enable_frosted_glass_when_off() {
    // 1. Live opacity drag while frosted_glass_enabled == false
    let mut s = AppState::new_for_preview();
    s.frosted_glass_enabled = false;
    s.switch_t[ToggleId::FrostedGlass as usize] = 0.0;
    assert!(s.set_frosted_opacity_live(72));
    assert!(
        s.frosted_glass_enabled,
        "Dragging Glass Opacity slider should auto-enable frosted_glass_enabled"
    );
    assert_eq!(s.frosted_opacity_pct, 72);
    assert_eq!(s.switch_t[ToggleId::FrostedGlass as usize], 1.0);

    // 2. Live blur drag while frosted_glass_enabled == false
    let mut s2 = AppState::new_for_preview();
    s2.frosted_glass_enabled = false;
    s2.switch_t[ToggleId::FrostedGlass as usize] = 0.0;
    assert!(s2.set_frosted_blur_live(28));
    assert!(
        s2.frosted_glass_enabled,
        "Dragging Blur Intensity slider should auto-enable frosted_glass_enabled"
    );
    assert_eq!(s2.frosted_blur_px, 28);
    assert_eq!(s2.switch_t[ToggleId::FrostedGlass as usize], 1.0);

    // 3. Action::SetFrostedOpacity and Action::SetFrostedBlur
    let mut s3 = AppState::new_for_preview();
    s3.frosted_glass_enabled = false;
    assert!(s3.apply_setting_action(Action::SetFrostedOpacity(65)).is_some());
    assert!(s3.frosted_glass_enabled);
    assert_eq!(s3.frosted_opacity_pct, 65);

    s3.frosted_glass_enabled = false;
    assert!(s3.apply_setting_action(Action::SetFrostedBlur(32)).is_some());
    assert!(s3.frosted_glass_enabled);
    assert_eq!(s3.frosted_blur_px, 32);
}

#[test]
fn qa_iss04_iss05_settings_categories_fit_unclipped_and_have_full_row_hitboxes() -> Result<()> {
    let ctx = make_ctx()?;
    let mut cache = D2DResourceCache::new(&ctx)?;
    let mut fonts = DWriteTextCache::new()?;

    let mut s = AppState::new_for_preview();
    s.settings_open = true;
    let (sw, sh, _) = settings_dims_for_state(&s);
    let target = make_target(&ctx, sw, sh)?;

    // Motion, Modules, Notifications, and System must fit 100% at scroll=0 (max_scroll == 0.0)
    for cat in [
        SettingsCategory::Motion,
        SettingsCategory::Modules,
        SettingsCategory::Notifications,
        SettingsCategory::System,
    ] {
        s.set_settings_category(cat);
        s.set_settings_scroll_immediate(cat, 0.0);
        s.sync_preview_settled();

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            let (hits, _slots, max_scroll) =
                render_settings_d2d(&ctx, &mut cache, &mut fonts, sw, sh, &s, None);
            ctx.EndDraw(None, None)?;

            assert_eq!(
                max_scroll, 0.0,
                "Settings category {:?} overflows pane height at scroll=0 (max_scroll = {})",
                cat, max_scroll
            );
            assert!(
                !hits.is_empty(),
                "Settings category {:?} produced zero hit boxes",
                cat
            );
        }
    }

    // Verify full-row toggle hit boxes across General, Notifications, and Modules (width >= 500px, height >= 38px)
    for (cat, expected_action) in [
        (SettingsCategory::General, Action::ToggleMinimizedIsland),
        (SettingsCategory::General, Action::ToggleClickToOpen),
        (SettingsCategory::General, Action::ToggleHideFullscreen),
        (SettingsCategory::General, Action::ToggleShowMascot),
        (SettingsCategory::Notifications, Action::ToggleNotifications),
        (SettingsCategory::Notifications, Action::ToggleNotifListener),
        (SettingsCategory::Modules, Action::ToggleClipboardWatch),
        (SettingsCategory::Modules, Action::ToggleLyricsMonitor),
        (SettingsCategory::Modules, Action::ToggleCaptureClipboard),
    ] {
        s.set_settings_category(cat);
        s.set_settings_scroll_immediate(cat, 0.0);
        s.sync_preview_settled();

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            let (hits, _, _) =
                render_settings_d2d(&ctx, &mut cache, &mut fonts, sw, sh, &s, None);
            ctx.EndDraw(None, None)?;

            let hb = hits
                .iter()
                .find(|h| h.action == expected_action)
                .unwrap_or_else(|| {
                    panic!(
                        "Expected hit box {:?} in category {:?} at scroll=0",
                        expected_action, cat
                    )
                });
            let w = hb.rect.width();
            let h = hb.rect.height();
            assert!(
                w >= 500.0 && h >= 38.0,
                "Toggle {:?} hit box too small: {}x{} (expected full row >= 500x38)",
                expected_action,
                w,
                h
            );
        }
    }

    // Verify "Enable in Win" button appears in General when windows_transparency_enabled == false
    s.set_settings_category(SettingsCategory::General);
    s.set_settings_scroll_immediate(SettingsCategory::General, 240.0);
    s.windows_transparency_enabled = false;
    s.sync_preview_settled();
    unsafe {
        ctx.SetTarget(&target);
        ctx.BeginDraw();
        let (hits, _, _) =
            render_settings_d2d(&ctx, &mut cache, &mut fonts, sw, sh, &s, None);
        ctx.EndDraw(None, None)?;
        assert!(
            hits.iter()
                .any(|h| h.action == Action::OpenWinPersonalizeColors),
            "Expected Action::OpenWinPersonalizeColors hit box when windows_transparency_enabled is false"
        );
    }

    Ok(())
}

#[test]
fn qa_iss03_iss10_iss12_iss15_notch_renderer_hitboxes_and_overlays() -> Result<()> {
    let ctx = make_ctx()?;
    let target = make_target(&ctx, 1000, 1000)?;

    // 1. ISS-03: Inline Quick Settings (Horizontal & Vertical) includes FrostedGlass & MinimizedIsland toggles
    for placement in [NotchPlacement::Top, NotchPlacement::Left] {
        let mut s = AppState::new_for_preview();
        s.placement = placement;
        s.mode = NotchMode::Settings;
        s.set_hovered(true);
        s.sync_preview_settled();
        let (w, h) = (
            s.current_width.round().max(1.0) as u32,
            s.current_height.round().max(1.0) as u32,
        );
        let mut r = Renderer::new(w, h);

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            r.render_d2d(&ctx, &s, None);
            ctx.EndDraw(None, None)?;

            for required in [
                Action::ToggleAutostart,
                Action::ToggleChime,
                Action::ToggleFrostedGlass,
                Action::ToggleMinimizedIsland,
                Action::OpenSettings,
                Action::Quit,
            ] {
                assert!(
                    r.hits.iter().any(|hb| hb.action == required),
                    "Inline settings ({:?}) missing hit box {:?}",
                    placement,
                    required
                );
            }
        }
    }

    // 2. ISS-10: Music mode has single full-width SeekMusic hit box in Standard, Lyrics, and Vertical views
    for (placement, lyrics, expected_min_w) in [
        (NotchPlacement::Top, false, 300.0f32),
        (NotchPlacement::Top, true, 180.0f32),
        (NotchPlacement::Left, false, 240.0f32),
    ] {
        let mut s = AppState::new_for_preview();
        s.placement = placement;
        s.mode = NotchMode::Music;
        s.show_lyrics = lyrics;
        s.media.is_playing = true;
        s.set_hovered(true);
        s.sync_preview_settled();
        let (w, h) = (
            s.current_width.round().max(1.0) as u32,
            s.current_height.round().max(1.0) as u32,
        );
        let mut r = Renderer::new(w, h);

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            r.render_d2d(&ctx, &s, None);
            ctx.EndDraw(None, None)?;

            let seek_boxes: Vec<_> = r
                .hits
                .iter()
                .filter(|hb| matches!(hb.action, Action::SeekMusic(_)))
                .collect();
            assert_eq!(
                seek_boxes.len(),
                1,
                "Expected 1 unified SeekMusic hit box in ({:?}, lyrics={}), got {}",
                placement,
                lyrics,
                seek_boxes.len()
            );
            let sb_w = seek_boxes[0].rect.width();
            assert!(
                sb_w >= expected_min_w,
                "SeekMusic hit box width {} < {} in ({:?}, lyrics={})",
                sb_w,
                expected_min_w,
                placement,
                lyrics
            );
        }
    }

    // 3. ISS-12 (updated for v0.3.1 Auto-Stash): Vertical expanded view with active notification
    // auto-stashes into the header Bell icon (zero floating overlay collision) and keeps dock tabs clickable.
    {
        let mut s = AppState::new_for_preview();
        s.placement = NotchPlacement::Left;
        s.mode = NotchMode::Home;
        let _ = s.apply_setting_action(Action::SendTestNotification);
        s.set_hovered(true);
        s.sync_preview_settled();
        assert!(
            s.is_notification_stashed(),
            "Active notification must Auto-Stash into header Bell when vertical notch expands"
        );
        let (w, h) = (
            s.current_width.round().max(1.0) as u32,
            s.current_height.round().max(1.0) as u32,
        );
        let mut r = Renderer::new(w, h);

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            r.render_d2d(&ctx, &s, None);
            ctx.EndDraw(None, None)?;

            assert!(
                r.hits
                    .iter()
                    .any(|hb| hb.action == Action::SetMode(NotchMode::Music)),
                "Vertical expanded view with stashed notification must keep module dock tabs clickable"
            );
            assert!(
                r.hits
                    .iter()
                    .any(|hb| hb.action == Action::ToggleNotifHistory),
                "Vertical expanded view with stashed notification must expose header Bell peek hit box"
            );
            assert!(
                r.hits
                    .iter()
                    .all(|hb| hb.action != Action::DismissNotification),
                "Vertical expanded view must not render legacy floating toast overlay"
            );
        }
    }

    // 4. ISS-15: Non-SystemToast notification banners have clickable body actions
    for (kind, is_success, expected_body_action) in [
        (
            NotificationKind::Capture,
            true,
            Action::OpenCaptureFolder,
        ),
        (
            NotificationKind::MemoryPurge,
            true,
            Action::DismissNotification,
        ),
        (
            NotificationKind::General,
            true,
            Action::DismissNotification,
        ),
    ] {
        let mut s = AppState::new_for_preview();
        s.notifications_enabled = true;
        s.set_notification(
            "Test Alert".to_string(),
            "Detail payload".to_string(),
            kind,
            is_success,
        );
        s.set_hovered(false);
        s.sync_preview_settled();
        let (w, h) = (
            s.current_width.round().max(1.0) as u32,
            s.current_height.round().max(1.0) as u32,
        );
        let mut r = Renderer::new(w, h);

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            r.render_d2d(&ctx, &s, None);
            ctx.EndDraw(None, None)?;

            assert!(
                r.hits.iter().any(|hb| hb.action == expected_body_action),
                "Notification banner kind {:?} missing body action {:?}",
                kind,
                expected_body_action
            );
        }
    }

    Ok(())
}

#[test]
fn qa_rapid_interaction_and_multi_scale_stability_stress() -> Result<()> {
    let ctx = make_ctx()?;
    let target = make_target(&ctx, 1600, 1200)?;

    let mut s = AppState::new_for_preview();
    let modes = [
        NotchMode::Home,
        NotchMode::Music,
        NotchMode::Volume,
        NotchMode::MemReduct,
        NotchMode::Shelf,
        NotchMode::Settings,
    ];
    let scales = [
        TypeScale::Compact,
        TypeScale::Standard,
        TypeScale::Large,
        TypeScale::ExtraLarge,
    ];

    // Rapidly cycle modes, placements, scales, and toast bursts while rendering & ticking physics
    for i in 0..120 {
        s.mode = modes[i % modes.len()];
        s.type_scale = scales[i % scales.len()];
        s.placement = match i % 3 {
            0 => NotchPlacement::Top,
            1 => NotchPlacement::Left,
            _ => NotchPlacement::Right,
        };
        s.set_hovered(i % 2 == 0);
        if i % 10 == 0 {
            let _ = s.apply_setting_action(Action::SendTestNotification);
        }
        let _ = s.update_animation(0.016);
        let (w, h) = (
            s.current_width.round().max(1.0) as u32,
            s.current_height.round().max(1.0) as u32,
        );
        assert!(w > 0 && h > 0 && w <= 1600 && h <= 1200);
        let mut r = Renderer::new(w, h);

        unsafe {
            ctx.SetTarget(&target);
            ctx.BeginDraw();
            r.render_d2d(&ctx, &s, None);
            ctx.EndDraw(None, None)?;
        }
    }

    Ok(())
}

#[test]
fn qa_iss16_discord_fsi_pdi_bidi_boxes_stripped_cleanly_while_preserving_emoji() {
    let raw_title = "\u{2068}aya\u{2069} (\u{2068}#☕coffee-talk\u{2069}, \u{2068}n\u{2069})";
    let raw_sub =
        "Reacted \u{2068}☝️\u{2069} to you \"\u{2068}info malam jumat horror merinding triple a\u{2069}\"";

    let clean_title = cynotch::notifications::sanitize_notification_text(raw_title);
    let clean_sub = cynotch::notifications::sanitize_notification_text(raw_sub);

    assert_eq!(clean_title, "aya (#☕coffee-talk, n)");
    assert_eq!(
        clean_sub,
        "Reacted ☝️ to you \"info malam jumat horror merinding triple a\""
    );
    assert!(!clean_title.contains('\u{2068}') && !clean_title.contains('\u{2069}'));
    assert!(!clean_sub.contains('\u{2068}') && !clean_sub.contains('\u{2069}'));

    // Also verify DWriteTextCache::clean_display_str strips FSI/PDI/LRM/RLM/BOM even on un-sanitized strings
    let mut fonts = DWriteTextCache::new().unwrap();
    let (w_dirty, _) = fonts.measure(raw_title, 13.0, cynotch::font_d2d::Weight::Bold);
    let (w_clean, _) = fonts.measure(&clean_title, 13.0, cynotch::font_d2d::Weight::Bold);
    assert!(
        (w_dirty - w_clean).abs() < 0.01,
        "DirectWrite layout width for FSI/PDI string ({}) should match clean string ({}) after filtering",
        w_dirty,
        w_clean
    );
}

