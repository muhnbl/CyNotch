use cynotch::dcomp_blur::{
    CALIBRATED_BLUR_PX, apply_color_matrix_5x4, optical_luminosity_clamp_matrix,
};
use cynotch::font_d2d::DWriteTextCache;
use cynotch::models::{
    Action, AppState, LIQUID_PLOP_DURATION_SECS, NotchMode, NotchPlacement, NotificationKind,
    RAY_ABSORB_DURATION_SECS, RAY_ROW_ENTRY_DURATION_SECS, SettingsCategory, ShelfKind,
    spring_step_analytic,
};
use cynotch::painter_d2d::D2DResourceCache;
use cynotch::renderer::Renderer;
use cynotch::settings_window::{render_settings_d2d, settings_dims_for_state};
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
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
            D3D_DRIVER_TYPE_WARP,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
        let dxgi: IDXGIDevice = device.unwrap().cast()?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
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

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn rel_luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * srgb_to_linear(rgb[0])
        + 0.7152 * srgb_to_linear(rgb[1])
        + 0.0722 * srgb_to_linear(rgb[2])
}

fn wcag_contrast(fg: [f32; 3], bg: [f32; 3]) -> f32 {
    let l1 = rel_luminance(fg);
    let l2 = rel_luminance(bg);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn notification_countdown_ring_ticks_autonomously_with_animations_on_and_off() {
    // 1. Animations ON, mascot OFF, springs settled
    let mut st = AppState::new_for_preview();
    st.windows_animations_enabled = true;
    st.show_mascot = false;
    st.media.is_playing = false;
    st.set_hovered(false);
    st.notification = None;
    st.sync_preview_settled();
    assert_eq!(st.needs_animation_timer_ms(), None, "idle must be 0 Hz");

    st.set_notification(
        "Build Ready".into(),
        "All checks passed".into(),
        NotificationKind::General,
        true,
    );
    st.sync_preview_settled();
    assert_eq!(
        st.needs_animation_timer_ms(),
        Some(33),
        "active notification must keep 30 Hz countdown timer even after springs settle"
    );
    assert!(
        st.update_animation(1.0 / 30.0),
        "update_animation must request redraw while visible countdown ring is ticking"
    );

    // 2. Windows Animations OFF must STILL tick countdown ring at 30 Hz
    st.windows_animations_enabled = false;
    assert_eq!(
        st.needs_animation_timer_ms(),
        Some(33),
        "active notification countdown must tick at 30 Hz even with OS Animation Effects OFF"
    );
    assert!(st.update_animation(1.0 / 30.0));

    // 2b. While Auto-Stashed (expanded/hovered & paused) on a non-mascot tab (Shelf) and settled, timer must drop to 0 Hz
    st.mode = NotchMode::Shelf;
    st.set_hovered(true);
    st.sync_preview_settled();
    assert!(st.is_notification_stashed());
    assert_eq!(
        st.needs_animation_timer_ms(),
        None,
        "stashed/paused notification with OS Animations OFF must idle at 0 Hz"
    );
    st.windows_animations_enabled = true;
    st.sync_preview_settled();
    assert_eq!(
        st.needs_animation_timer_ms(),
        None,
        "stashed/paused notification once expand springs settle must idle at 0 Hz"
    );
    st.set_hovered(false);
    st.sync_preview_settled();
    assert_eq!(st.needs_animation_timer_ms(), Some(33));

    // 3. Dismissing returns cleanly to 0 Hz idle
    st.apply_setting_action(Action::DismissNotification);
    st.sync_preview_settled();
    assert!(st.notification.is_none());
    assert_eq!(st.needs_animation_timer_ms(), None);
}

#[test]
fn notification_auto_stash_pauses_on_expand_peeks_on_bell_and_resumes_on_collapse() {
    let mut st = AppState::new_for_preview();
    st.windows_animations_enabled = true;
    st.show_mascot = false;
    st.set_hovered(false);
    st.sync_preview_settled();

    st.push_system_toast(
        "Discord".into(),
        "Cyno".into(),
        "Sent a screenshot to #design-review".into(),
        None,
    );
    assert!(st.notification.is_some());
    assert!(!st.is_notification_stashed());

    // Coalesced same-app toast adds spring velocity kick
    st.anim_width_vel = 0.0;
    st.anim_height_vel = 0.0;
    st.push_system_toast(
        "Discord".into(),
        "Cyno".into(),
        "Follow-up message".into(),
        None,
    );
    assert_eq!(st.notification.as_ref().unwrap().grouped_count, 2);
    assert!(st.anim_width_vel >= 139.0);
    assert!(st.anim_height_vel >= 25.0);

    // Expand notch -> Auto-Stash into Bell icon & pause countdown
    st.set_hovered(true);
    assert!(st.is_notification_stashed());
    assert!(st.notification.as_ref().unwrap().paused_remaining.is_some());

    // Even if expires_at is backdated while stashed, expire_notification must NOT drop it
    if let Some(ref mut n) = st.notification {
        n.expires_at = Instant::now() - Duration::from_secs(10);
    }
    assert!(
        !st.expire_notification(),
        "stashed notification must stay paused while notch is expanded"
    );
    assert!(st.notification.is_some());

    // Clicking Bell icon while stashed peeks/restores it back to Dynamic Island & resumes timer
    st.is_pinned = true;
    st.apply_setting_action(Action::ToggleNotifHistory);
    assert!(!st.is_hovered, "Bell click while stashed must collapse to peek notification");
    assert!(!st.is_pinned);
    assert!(!st.is_notification_stashed());
    assert!(st.notification.as_ref().unwrap().paused_remaining.is_none());
    assert!(
        st.notification.as_ref().unwrap().remaining_frac(Instant::now()) > 0.5,
        "remaining timer must resume cleanly instead of expiring immediately"
    );

    // Incoming toast while notch is already expanded auto-stashes immediately
    st.notification = None;
    st.set_hovered(true);
    st.push_system_toast("Slack".into(), "Deploy".into(), "v0.3.1 live".into(), None);
    assert!(st.is_notification_stashed());
    assert!(st.notification.as_ref().unwrap().paused_remaining.is_some());
    st.set_hovered(false);
    assert!(!st.is_notification_stashed());
    assert!(st.notification.as_ref().unwrap().paused_remaining.is_none());

    // Bell peek on a nearly-expired stashed notification (30ms remaining) grants a readable peek floor
    st.set_hovered(true);
    if let Some(ref mut n) = st.notification {
        n.paused_remaining = Some(Duration::from_millis(30));
    }
    st.apply_setting_action(Action::ToggleNotifHistory);
    assert!(!st.is_hovered);
    let peek_rem = st
        .notification
        .as_ref()
        .unwrap()
        .remaining_duration(Instant::now());
    assert!(
        peek_rem >= Duration::from_millis(1800),
        "Bell peek must keep notification visible long enough to read, got {:?}",
        peek_rem
    );
}

#[test]
fn closed_form_spring_is_framerate_independent_and_respects_damping_profiles() {
    // Frame-rate independence: 60 Hz vs 120 Hz vs 240 Hz over 0.25s must match within < 0.05px
    let sim = |hz: usize| -> (f32, f32) {
        let dt = 1.0 / hz as f32;
        let steps = hz / 4; // 0.25s
        let (mut x, mut v) = (136.0_f32, 0.0_f32);
        for _ in 0..steps {
            let (nx, nv, _) = spring_step_analytic(x, v, 368.0, 2.85, 0.78, dt, 0.04, 0.4);
            x = nx;
            v = nv;
        }
        (x, v)
    };
    let (x60, v60) = sim(60);
    let (x120, v120) = sim(120);
    let (x240, v240) = sim(240);
    assert!((x60 - x120).abs() < 0.05, "60Hz vs 120Hz diverged: {x60} vs {x120}");
    assert!((x120 - x240).abs() < 0.05, "120Hz vs 240Hz diverged: {x120} vs {x240}");
    assert!((v60 - v120).abs() < 0.5 && (v120 - v240).abs() < 0.5, "velocity diverged: {v60} vs {v120} vs {v240}");

    // Notif open (zeta = 0.78) has slight organic overshoot (~1..4%) then settles cleanly
    let (mut x, mut v) = (136.0_f32, 0.0_f32);
    let mut max_x = x;
    for _ in 0..180 {
        let (nx, nv, _) = spring_step_analytic(x, v, 368.0, 2.85, 0.78, 1.0 / 60.0, 0.04, 0.4);
        x = nx;
        v = nv;
        max_x = max_x.max(x);
    }
    assert!(max_x > 368.5 && max_x < 380.0, "expected subtle ~2% organic settle, max={max_x}");
    assert!((x - 368.0).abs() <= 0.05 && v.abs() <= 0.5);

    // Collapse (zeta = 0.94) has crisp snap with virtually 0% overshoot
    let (mut cx, mut cv) = (368.0_f32, 0.0_f32);
    let mut min_x = cx;
    for _ in 0..120 {
        let (nx, nv, _) = spring_step_analytic(cx, cv, 136.0, 3.60, 0.94, 1.0 / 60.0, 0.04, 0.4);
        cx = nx;
        cv = nv;
        min_x = min_x.min(cx);
    }
    assert!(min_x >= 135.8, "collapse overshot target: min={min_x}");
    assert!((cx - 136.0).abs() <= 0.05);

    // Edge cases: zero / negative / huge dt never NaN
    for bad_dt in [0.0_f32, -0.5, 10.0] {
        let (nx, nv, _) = spring_step_analytic(100.0, 10.0, 200.0, 3.0, 0.8, bad_dt, 0.04, 0.4);
        assert!(nx.is_finite() && nv.is_finite());
    }
}

#[test]
fn raycast_tactile_shelf_fx_has_mass_absorption_and_zero_buoyancy_oscillation() {
    assert!((LIQUID_PLOP_DURATION_SECS - 0.38).abs() < 1e-4);
    assert!((RAY_ABSORB_DURATION_SECS - 0.24).abs() < 1e-4);
    assert!((RAY_ROW_ENTRY_DURATION_SECS - 0.12).abs() < 1e-4);

    let mut st = AppState::new();
    let id = st.push_shelf_item(ShelfKind::File, "Spec.pdf".into(), "C:\\Spec.pdf".into());
    let t0 = Instant::now();
    st.shelf_fx.plop_started_at = Some(t0);
    st.shelf_fx.plop_item_id = Some(id);

    // Row entry must monotonically decay from +6px -> 0 over 120ms with ZERO negative bounce
    let mut prev = st.shelf_fx.buoyancy_offset(id, t0);
    assert!((prev - 6.0).abs() < 0.2);
    for ms in (10..=150).step_by(10) {
        let t = t0 + Duration::from_millis(ms);
        let cur = st.shelf_fx.buoyancy_offset(id, t);
        assert!(cur >= 0.0, "row entry must never overshoot negative at {ms}ms: {cur}");
        assert!(cur <= prev + 1e-4, "row entry must monotonically decrease at {ms}ms");
        prev = cur;
    }
    assert_eq!(st.shelf_fx.buoyancy_offset(id, t0 + Duration::from_millis(125)), 0.0);
    assert_eq!(st.shelf_fx.row_entry_alpha(id, t0 + Duration::from_millis(125)), 1.0);

    // Mass Absorption card dip peaks around +3.5px and settles to 0.0 by 240ms
    let peak = st.shelf_fx.card_absorb_offset(t0 + Duration::from_millis(55));
    assert!(peak > 3.0 && peak <= 3.51, "unexpected Mass Absorption peak: {peak}");
    assert_eq!(st.shelf_fx.card_absorb_offset(t0 + Duration::from_millis(245)), 0.0);
}

#[test]
fn optical_luminosity_clamp_keeps_aaa_contrast_over_pure_white_pdf() {
    assert_eq!(CALIBRATED_BLUR_PX, 15);
    let m = optical_luminosity_clamp_matrix();

    // Pure #FFFFFF backdrop compressed through Option B Airy Liquid Luminosity Clamp
    let clamped_white = apply_color_matrix_5x4([1.0, 1.0, 1.0], &m);
    assert!(clamped_white[0] < 0.72 && clamped_white[1] < 0.72 && clamped_white[2] < 0.72);

    // Composite calibrated airy 18% obsidian shell veil rgba(18, 15, 34, 46) over clamped backdrop
    let a = 46.0 / 255.0;
    let shell = [18.0 / 255.0, 15.0 / 255.0, 34.0 / 255.0];
    let final_bg = [
        shell[0] * a + clamped_white[0] * (1.0 - a),
        shell[1] * a + clamped_white[1] * (1.0 - a),
        shell[2] * a + clamped_white[2] * (1.0 - a),
    ];
    let text_fg = [248.0 / 255.0, 247.0 / 255.0, 255.0 / 255.0];
    let ratio = wcag_contrast(text_fg, final_bg);
    assert!(
        ratio >= 2.5,
        "Option B Airy Liquid Clamp over #FFFFFF must keep readable contrast, got {ratio:.2}:1"
    );

    // Pure black stays non-negative deep obsidian; dark violet keeps purple hue richness
    let clamped_black = apply_color_matrix_5x4([0.0, 0.0, 0.0], &m);
    assert!(clamped_black.iter().all(|&c| c >= 0.0 && c < 0.05));
    let clamped_violet = apply_color_matrix_5x4([0.25, 0.10, 0.45], &m);
    assert!(clamped_violet[2] > clamped_violet[0] && clamped_violet[0] > clamped_violet[1]);
}

#[test]
fn settings_general_removes_glass_sliders_and_expanded_notch_has_no_floating_toast_overlay()
-> Result<()> {
    let ctx = make_ctx()?;
    let mut cache = D2DResourceCache::new(&ctx)?;
    let mut fonts = DWriteTextCache::new()?;

    // 1. Settings General pane must have zero opacity/blur sliders & fit cleanly
    let mut st = AppState::new_for_preview();
    st.settings_open = true;
    st.set_settings_category(SettingsCategory::General);
    st.set_settings_scroll_immediate(SettingsCategory::General, 0.0);
    st.sync_preview_settled();
    let (sw, sh, _) = settings_dims_for_state(&st);
    let target = make_target(&ctx, sw, sh)?;
    unsafe {
        ctx.SetTarget(&target);
        ctx.BeginDraw();
        let (hits, _, _) = render_settings_d2d(&ctx, &mut cache, &mut fonts, sw, sh, &st, None);
        ctx.EndDraw(None, None)?;
        assert!(
            hits.iter()
                .all(|h| !matches!(h.action, Action::SetFrostedOpacity(_) | Action::SetFrostedBlur(_))),
            "FrostedOpacity/FrostedBlur sliders must not exist in Settings"
        );
    }

    // 2. Expanded notch with active notification must Auto-Stash (NO floating DismissNotification overlay!)
    for placement in [NotchPlacement::Top, NotchPlacement::Left] {
        let mut ns = AppState::new_for_preview();
        ns.placement = placement;
        ns.mode = NotchMode::Shelf;
        ns.frosted_glass_enabled = true;
        ns.windows_transparency_enabled = true;
        ns.push_shelf_item(ShelfKind::File, "CyNotch_Spec.pdf".into(), "C:\\CyNotch_Spec.pdf".into());
        ns.set_hovered(true);
        ns.push_system_toast("Discord".into(), "Cyno".into(), "Test stash".into(), None);
        ns.sync_preview_settled();

        let (nw, nh) = (ns.current_width.round() as u32, ns.current_height.round() as u32);
        let ntarget = make_target(&ctx, nw, nh)?;
        let mut ren = Renderer::new(nw, nh);
        unsafe {
            ctx.SetTarget(&ntarget);
            ctx.BeginDraw();
            ren.render_d2d(&ctx, &ns, None);
            ctx.EndDraw(None, None)?;
        }
        assert!(
            ren.hits.iter().all(|h| h.action != Action::DismissNotification),
            "Expanded {:?} notch must not draw floating toast overlay",
            placement
        );
        assert!(
            ren.hits.iter().any(|h| h.action == Action::ToggleNotifHistory),
            "Expanded {:?} notch must keep header Bell hit target accessible for Auto-Stash peek",
            placement
        );
    }
    Ok(())
}

#[test]
fn winrt_color_matrix_effect_chain_succeeds_without_fallback() -> Result<()> {
    use cynotch::composition::ensure_dispatcher_queue;
    use cynotch::dcomp_blur::{ColorMatrixEffect, GaussianBlurEffect, blur_px_to_sigma};
    use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffectSource};
    use windows::UI::Composition::{
        CompositionBrush, CompositionEffectSourceParameter, Compositor,
    };

    ensure_dispatcher_queue()?;
    let compositor = Compositor::new()?;
    let host_backdrop = compositor.CreateHostBackdropBrush()?;
    let source_param =
        CompositionEffectSourceParameter::Create(&windows::core::HSTRING::from("Backdrop"))?;
    let source_iface: IGraphicsEffectSource = source_param.cast()?;
    let sigma = blur_px_to_sigma(CALIBRATED_BLUR_PX);
    let anim_props: windows::Foundation::Collections::IIterable<windows::core::HSTRING> =
        vec![windows::core::HSTRING::from("Blur.BlurAmount")].try_into()?;

    let blur_for_chain = GaussianBlurEffect::new(sigma, Some(source_iface));
    let blur_src: IGraphicsEffectSource = blur_for_chain.into();
    let clamped_effect = ColorMatrixEffect::new(optical_luminosity_clamp_matrix(), Some(blur_src));
    let clamped_iface: IGraphicsEffect = clamped_effect.into();

    let eff_factory = compositor
        .CreateEffectFactoryWithProperties(&clamped_iface, &anim_props)
        .expect("WinRT Compositor must accept GaussianBlurEffect -> ColorMatrixEffect without silent fallback");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut status = eff_factory.LoadStatus()?;
    while status == windows::UI::Composition::CompositionEffectFactoryLoadStatus::Pending
        && Instant::now() < deadline
    {
        unsafe {
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
        status = eff_factory.LoadStatus()?;
    }
    let ext_err = eff_factory.ExtendedError()?;
    assert_eq!(
        status,
        windows::UI::Composition::CompositionEffectFactoryLoadStatus::Success,
        "ColorMatrixEffect factory LoadStatus must be Success, got {:?} (ext={:?})",
        status,
        ext_err
    );
    assert_eq!(ext_err, windows::core::HRESULT(0));
    let brush = eff_factory.CreateBrush()?;
    let backdrop_brush: CompositionBrush = host_backdrop.cast()?;
    brush.SetSourceParameter(&windows::core::HSTRING::from("Backdrop"), &backdrop_brush)?;
    brush
        .Properties()?
        .InsertScalar(&windows::core::HSTRING::from("Blur.BlurAmount"), sigma)?;
    Ok(())
}

#[test]
fn d2d1_color_matrix_shader_pixel_readback_matches_option_b_and_settings_ignores_legacy_opacity()
-> Result<()> {
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D_MATRIX_5X4_F, D2D_POINT_2F,
    };
    use windows::Win32::Graphics::Direct2D::{
        CLSID_D2D1ColorMatrix, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ,
        D2D1_COLORMATRIX_PROP_ALPHA_MODE, D2D1_COLORMATRIX_PROP_CLAMP_OUTPUT,
        D2D1_COLORMATRIX_PROP_COLOR_MATRIX, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        D2D1_MAP_OPTIONS_READ,
    };

    let ctx = make_ctx()?;
    let m = optical_luminosity_clamp_matrix();
    let d2d_m = D2D_MATRIX_5X4_F {
        Anonymous: windows::Win32::Graphics::Direct2D::Common::D2D_MATRIX_5X4_F_0 { m },
    };

    let eval_pixel = |r: f32, g: f32, b: f32| -> Result<[f32; 3]> {
        let src_bmp = make_target(&ctx, 4, 4)?;
        let dst_bmp = make_target(&ctx, 4, 4)?;
        let cpu_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe {
            let cpu_bmp = ctx.CreateBitmap(D2D_SIZE_U { width: 4, height: 4 }, None, 0, &cpu_props)?;
            ctx.SetTarget(&src_bmp);
            ctx.BeginDraw();
            ctx.Clear(Some(&D2D1_COLOR_F { r, g, b, a: 1.0 }));
            ctx.EndDraw(None, None)?;

            let eff = ctx.CreateEffect(&CLSID_D2D1ColorMatrix)?;
            eff.SetInput(0, &src_bmp, true);
            eff.SetValue(
                D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32,
                windows::Win32::Graphics::Direct2D::D2D1_PROPERTY_TYPE_MATRIX_5X4,
                std::slice::from_raw_parts(
                    &d2d_m as *const D2D_MATRIX_5X4_F as *const u8,
                    std::mem::size_of::<D2D_MATRIX_5X4_F>(),
                ),
            )?;
            let alpha_mode = D2D1_ALPHA_MODE_PREMULTIPLIED.0 as u32;
            eff.SetValue(
                D2D1_COLORMATRIX_PROP_ALPHA_MODE.0 as u32,
                windows::Win32::Graphics::Direct2D::D2D1_PROPERTY_TYPE_ENUM,
                &alpha_mode.to_ne_bytes(),
            )?;
            let clamp_true = 1u32;
            eff.SetValue(
                D2D1_COLORMATRIX_PROP_CLAMP_OUTPUT.0 as u32,
                windows::Win32::Graphics::Direct2D::D2D1_PROPERTY_TYPE_BOOL,
                &clamp_true.to_ne_bytes(),
            )?;

            ctx.SetTarget(&dst_bmp);
            ctx.BeginDraw();
            ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            ctx.DrawImage(
                &eff.GetOutput()?,
                Some(&D2D_POINT_2F { x: 0.0, y: 0.0 }),
                None,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            ctx.EndDraw(None, None)?;

            cpu_bmp.CopyFromBitmap(None, &dst_bmp, None)?;
            let mapped = cpu_bmp.Map(D2D1_MAP_OPTIONS_READ)?;
            let px = std::slice::from_raw_parts(mapped.bits, 4);
            let rgb = [px[2] as f32 / 255.0, px[1] as f32 / 255.0, px[0] as f32 / 255.0];
            cpu_bmp.Unmap()?;
            Ok(rgb)
        }
    };

    // Verify Direct2D hardware/WARP CLSID_D2D1ColorMatrix matches our CPU formula on white & violet
    for sample in [[1.0_f32, 1.0, 1.0], [0.25, 0.10, 0.45]] {
        let gpu = eval_pixel(sample[0], sample[1], sample[2])?;
        let cpu = apply_color_matrix_5x4(sample, &m);
        for c in 0..3 {
            assert!(
                (gpu[c] - cpu[c]).abs() <= 2.0 / 255.0,
                "D2D1ColorMatrix pixel mismatch for {:?} ch {c}: d2d={} vs cpu={}",
                sample,
                gpu[c],
                cpu[c]
            );
        }
    }

    // Verify Settings System/Motion wells ignore legacy v0.3.0 frosted_opacity_pct values (25 vs 95)
    let mut cache = D2DResourceCache::new(&ctx)?;
    let mut fonts = DWriteTextCache::new()?;
    let render_bytes = |pct: u32, cache: &mut D2DResourceCache, fonts: &mut DWriteTextCache| -> Result<Vec<u8>> {
        let mut st = AppState::new_for_preview();
        st.settings_open = true;
        st.frosted_glass_enabled = true;
        st.windows_transparency_enabled = true;
        st.frosted_opacity_pct = pct;
        st.set_settings_category(SettingsCategory::System);
        st.set_settings_scroll_immediate(SettingsCategory::System, 0.0);
        st.sync_preview_settled();
        let (sw, sh, _) = settings_dims_for_state(&st);
        let tgt = make_target(&ctx, sw, sh)?;
        let cpu_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe {
            let cpu_bmp = ctx.CreateBitmap(D2D_SIZE_U { width: sw, height: sh }, None, 0, &cpu_props)?;
            ctx.SetTarget(&tgt);
            ctx.BeginDraw();
            let _ = render_settings_d2d(&ctx, cache, fonts, sw, sh, &st, None);
            ctx.EndDraw(None, None)?;
            cpu_bmp.CopyFromBitmap(None, &tgt, None)?;
            let mapped = cpu_bmp.Map(D2D1_MAP_OPTIONS_READ)?;
            let mut out = Vec::with_capacity((sw * sh * 4) as usize);
            for row in 0..sh as usize {
                let row_ptr = mapped.bits.add(row * mapped.pitch as usize);
                out.extend_from_slice(std::slice::from_raw_parts(row_ptr, (sw * 4) as usize));
            }
            cpu_bmp.Unmap()?;
            Ok(out)
        }
    };
    let b25 = render_bytes(25, &mut cache, &mut fonts)?;
    let b95 = render_bytes(95, &mut cache, &mut fonts)?;
    assert_eq!(
        b25, b95,
        "Settings frosted glass must be fixed calibrated recipe independent of legacy frosted_opacity_pct"
    );
    Ok(())
}
