//! Offscreen D2D snapshot harness: renders key notch + Preferences states to PNG
//! so v0.3.0 output can be compared against the v0.2.4.1 `--snapshot` baseline.
//! Usage: cargo run --release --example snap_d2d -- <out_dir>

use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_MAP_OPTIONS_READ, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{Interface, Result};

use cynotch::font_d2d::DWriteTextCache;
use cynotch::models::{self, AppState, NotchMode};
use cynotch::painter_d2d::D2DResourceCache;
use cynotch::renderer::Renderer;

fn create_ctx() -> Result<ID2D1DeviceContext> {
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
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dev = factory.CreateDevice(&dxgi)?;
        dev.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
    }
}

fn bitmap(ctx: &ID2D1DeviceContext, w: u32, h: u32, cpu: bool) -> Result<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: if cpu {
            D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW
        } else {
            D2D1_BITMAP_OPTIONS_TARGET
        },
        colorContext: ManuallyDrop::new(None),
    };
    unsafe { ctx.CreateBitmap(D2D_SIZE_U { width: w, height: h }, None, 0, &props) }
}

/// Renders via `draw` with transparent (or custom) backdrop, saves PNG.
fn snap_transparent(
    ctx: &ID2D1DeviceContext,
    dir: &str,
    name: &str,
    w: u32,
    h: u32,
    pad: u32,
    draw: impl FnOnce(&ID2D1DeviceContext),
) -> Result<()> {
    let target = bitmap(ctx, w, h, false)?;
    let readback = bitmap(ctx, w, h, true)?;
    unsafe {
        ctx.SetTarget(&target);
        ctx.BeginDraw();
        ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        ctx.Clear(None);
        draw(ctx);
        if let Err(e) = ctx.EndDraw(None, None) {
            eprintln!("{name}: EndDraw failed: {e:?}");
        }
        readback.CopyFromBitmap(None, &target, None)?;
        let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
        let (ow, oh) = (w + pad * 2, h + pad * 2);
        let mut out = vec![0u8; (ow * oh * 4) as usize];
        for y in 0..h {
            let src = std::slice::from_raw_parts(
                mapped.bits.add((y * mapped.pitch) as usize),
                (w * 4) as usize,
            );
            for x in 0..w {
                let s = &src[(x * 4) as usize..(x * 4 + 4) as usize];
                let o = (((y + pad) * ow + x + pad) * 4) as usize;
                // Convert premultiplied BGRA to straight BGRA for PNG saving
                let a = s[3] as u32;
                if a > 0 {
                    out[o] = ((s[0] as u32 * 255 + a / 2) / a).min(255) as u8;
                    out[o + 1] = ((s[1] as u32 * 255 + a / 2) / a).min(255) as u8;
                    out[o + 2] = ((s[2] as u32 * 255 + a / 2) / a).min(255) as u8;
                    out[o + 3] = a as u8;
                }
            }
        }
        readback.Unmap()?;
        let path = std::path::Path::new(dir).join(format!("{name}.png"));
        if !cynotch::capture::save_bgra_png_alpha(&path, ow, oh, &out) {
            eprintln!("{name}: PNG save failed");
        }
    }
    Ok(())
}

/// Renders via `draw`, composites over the baseline lavender backdrop with 40px pad, saves PNG.
fn snap(
    ctx: &ID2D1DeviceContext,
    dir: &str,
    name: &str,
    w: u32,
    h: u32,
    draw: impl FnOnce(&ID2D1DeviceContext),
) -> Result<()> {
    if std::env::var_os("SNAP_TRANSPARENT").is_some() {
        return snap_transparent(ctx, dir, name, w, h, 0, draw);
    }
    snap_bg(ctx, dir, name, w, h, [248, 233, 236, 255], draw)
}

fn snap_bg(
    ctx: &ID2D1DeviceContext,
    dir: &str,
    name: &str,
    w: u32,
    h: u32,
    bg_bgra: [u8; 4],
    draw: impl FnOnce(&ID2D1DeviceContext),
) -> Result<()> {
    let target = bitmap(ctx, w, h, false)?;
    let readback = bitmap(ctx, w, h, true)?;
    unsafe {
        ctx.SetTarget(&target);
        ctx.BeginDraw();
        ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        ctx.Clear(None);
        draw(ctx);
        if let Err(e) = ctx.EndDraw(None, None) {
            eprintln!("{name}: EndDraw failed: {e:?}");
        }
        readback.CopyFromBitmap(None, &target, None)?;
        let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
        let pad = 40u32;
        let (ow, oh) = (w + pad * 2, h + pad * 2);
        let mut out = vec![0u8; (ow * oh * 4) as usize];
        for px in out.chunks_exact_mut(4) {
            px.copy_from_slice(&bg_bgra);
        }
        for y in 0..h {
            let src = std::slice::from_raw_parts(
                mapped.bits.add((y * mapped.pitch) as usize),
                (w * 4) as usize,
            );
            for x in 0..w {
                let s = &src[(x * 4) as usize..(x * 4 + 4) as usize];
                let a = s[3] as u32;
                let o = (((y + pad) * ow + x + pad) * 4) as usize;
                for c in 0..3 {
                    let bg = out[o + c] as u32;
                    out[o + c] = (s[c] as u32 + bg * (255 - a) / 255).min(255) as u8;
                }
            }
        }
        readback.Unmap()?;
        let path = std::path::Path::new(dir).join(format!("{name}.png"));
        if !cynotch::capture::save_bgra_png_alpha(&path, ow, oh, &out) {
            eprintln!("{name}: PNG save failed");
        }
    }
    Ok(())
}

fn notch(ctx: &ID2D1DeviceContext, dir: &str, name: &str, s: &AppState) -> Result<()> {
    notch_ptr(ctx, dir, name, s, None)
}

fn notch_ptr(
    ctx: &ID2D1DeviceContext,
    dir: &str,
    name: &str,
    s: &AppState,
    pointer: Option<(f32, f32)>,
) -> Result<()> {
    let (w, h) = (
        s.current_width.round().max(1.0) as u32,
        s.current_height.round().max(1.0) as u32,
    );
    let mut r = Renderer::new(w, h).unwrap();
    snap(ctx, dir, name, w, h, |c| r.render_d2d(c, s, pointer))
}

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "snap_d2d".into());
    std::fs::create_dir_all(&dir).ok();
    let ctx = create_ctx()?;

    let mut s = AppState::new_for_preview();
    s.telemetry.memory_percent = 57.0;
    s.telemetry.memory_used_gb = 18.2;
    s.telemetry.memory_total_gb = 31.9;
    s.telemetry.cpu_usage = 12.4;
    s.telemetry.net_down_kbps = 1480.0;
    s.telemetry.net_up_kbps = 264.0;

    s.minimized_island = true;
    s.mode = NotchMode::Home;
    s.media.is_playing = false;
    s.anim_time = 1.25;
    s.set_hovered(false);
    s.sync_preview_settled();
    notch(&ctx, &dir, "minimized_idle", &s)?;
    s.minimized_island = false;

    let shots: [(&str, bool, NotchMode, bool); 5] = [
        ("idle", false, NotchMode::Home, false),
        ("home", true, NotchMode::Home, false),
        ("music", true, NotchMode::Music, false),
        ("memreduct", true, NotchMode::MemReduct, false),
        ("shelf", true, NotchMode::Shelf, false),
    ];
    for (name, hovered, mode, lyrics) in shots {
        s.mode = mode;
        s.show_lyrics = lyrics;
        s.is_pinned = name == "home";
        s.media.is_playing = name != "idle";
        s.anim_time = if name == "music" { 0.42 } else { 0.25 };
        s.set_hovered(hovered);
        s.notification = None;
        s.sync_preview_settled();
        notch(&ctx, &dir, name, &s)?;
    }

    // v0.3.0 Polish visual snapshots: physical screen-edge & corner glow, hotkey hint, MemReduct chip hover
    let mut glow_cache = D2DResourceCache::new(&ctx)?;
    for (name, anchor, gw, gh) in [
        (
            "edge_glow_top",
            cynotch::edge_glow::EdgeGlowAnchor::Top,
            cynotch::edge_glow::EDGE_GLOW_TOP_SPAN,
            cynotch::edge_glow::EDGE_GLOW_DEPTH,
        ),
        (
            "edge_glow_left",
            cynotch::edge_glow::EdgeGlowAnchor::Left,
            cynotch::edge_glow::EDGE_GLOW_DEPTH,
            cynotch::edge_glow::EDGE_GLOW_SIDE_SPAN,
        ),
        (
            "edge_glow_right",
            cynotch::edge_glow::EdgeGlowAnchor::Right,
            cynotch::edge_glow::EDGE_GLOW_DEPTH,
            cynotch::edge_glow::EDGE_GLOW_SIDE_SPAN,
        ),
        (
            "edge_glow_top_left",
            cynotch::edge_glow::EdgeGlowAnchor::TopLeft,
            cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN,
            cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN,
        ),
        (
            "edge_glow_top_right",
            cynotch::edge_glow::EdgeGlowAnchor::TopRight,
            cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN,
            cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN,
        ),
    ] {
        snap_bg(&ctx, &dir, name, gw, gh, [6, 3, 10, 255], |c| {
            cynotch::edge_glow::render_edge_glow_d2d(c, &mut glow_cache, anchor, gw, gh, 0.96);
        })?;
        let white_name = format!("{name}_white");
        snap_bg(&ctx, &dir, &white_name, gw, gh, [255, 255, 255, 255], |c| {
            cynotch::edge_glow::render_edge_glow_d2d(c, &mut glow_cache, anchor, gw, gh, 0.96);
        })?;
    }

    {
        let mut hs = s.clone();
        hs.placement = models::NotchPlacement::Top;
        hs.mode = NotchMode::Home;
        hs.media.is_playing = false;
        hs.seen_hotkey_hint = false;
        hs.set_hovered(false);
        hs.sync_preview_settled();
        notch(&ctx, &dir, "idle_hotkey_hint", &hs)?;
    }

    {
        let mut ms = s.clone();
        ms.placement = models::NotchPlacement::Top;
        ms.mode = NotchMode::MemReduct;
        ms.set_hovered(true);
        ms.sync_preview_settled();
        // Hover over SysWS chip (card 2, row 0, col 1)
        let bw = ms.base_expanded_width();
        let col_w = (bw - 40.0 - 12.0) * 0.5;
        let col2_x = 20.0 + col_w + 12.0;
        let pill_w = (col_w - 28.0 - 4.5 * 3.0) / 4.0;
        let px = col2_x + 14.0 + (pill_w + 4.5) + pill_w * 0.5;
        let py = 60.0 + 29.0 + 8.0;
        let scale = ms.expanded_scale();
        notch_ptr(&ctx, &dir, "memreduct_hover", &ms, Some((px * scale, py * scale)))?;
    }

    s.set_hovered(false);
    s.media.is_playing = false;
    s.anim_time = 0.25;
    let cats = [
        ("settings_general", models::SettingsCategory::General, 0.0f32),
        ("settings_general_sliders", models::SettingsCategory::General, 260.0f32),
        ("settings_motion", models::SettingsCategory::Motion, 0.0f32),
        ("settings_notifications", models::SettingsCategory::Notifications, 0.0f32),
        ("settings_system", models::SettingsCategory::System, 0.0f32),
        ("settings_system_bottom", models::SettingsCategory::System, 160.0f32),
    ];
    let (sw, sh, _) = cynotch::settings_window::settings_dims_for_state(&s);
    let mut cache = D2DResourceCache::new(&ctx)?;
    let mut fonts = DWriteTextCache::new()?;
    for (name, cat, scroll) in cats {
        s.settings_open = true;
        s.set_settings_category(cat);
        s.set_settings_scroll_immediate(cat, scroll);
        s.sync_preview_settled();
        snap(&ctx, &dir, name, sw, sh, |c| {
            let _ = cynotch::settings_window::render_settings_d2d(
                c, &mut cache, &mut fonts, sw, sh, &s, None,
            );
        })?;
    }

    if std::env::args().any(|a| a == "--all") {
        let extra_modes = [
            ("volume", NotchMode::Volume, false),
            ("shelf", NotchMode::Shelf, false),
            ("inline_settings", NotchMode::Settings, false),
            ("music_lyrics", NotchMode::Music, true),
        ];
        for (name, mode, lyrics) in extra_modes {
            let mut es = s.clone();
            es.placement = models::NotchPlacement::Top;
            es.mode = mode;
            es.show_lyrics = lyrics;
            es.media.is_playing = true;
            es.set_hovered(true);
            es.sync_preview_settled();
            notch(&ctx, &dir, name, &es)?;
        }

        // Home without mascot
        {
            let mut nm = s.clone();
            nm.mode = NotchMode::Home;
            nm.show_mascot = false;
            nm.set_hovered(true);
            nm.sync_preview_settled();
            notch(&ctx, &dir, "home_no_mascot", &nm)?;
        }

        // Notification banner & Expanded toast overlay
        {
            let mut nb = s.clone();
            nb.apply_setting_action(models::Action::SendTestNotification);
            nb.set_hovered(false);
            nb.sync_preview_settled();
            notch(&ctx, &dir, "notif_banner", &nb)?;

            nb.set_hovered(true);
            nb.mode = NotchMode::Home;
            nb.sync_preview_settled();
            notch(&ctx, &dir, "expanded_toast_overlay", &nb)?;

            nb.notification = None;
            nb.show_notif_history = true;
            nb.sync_preview_settled();
            notch(&ctx, &dir, "notif_history", &nb)?;
        }

        // Vertical placement views
        for (name, mode) in [
            ("vert_home", NotchMode::Home),
            ("vert_music", NotchMode::Music),
            ("vert_volume", NotchMode::Volume),
            ("vert_inline_settings", NotchMode::Settings),
        ] {
            let mut vs = s.clone();
            vs.placement = models::NotchPlacement::Left;
            vs.mode = mode;
            vs.media.is_playing = true;
            vs.set_hovered(true);
            vs.sync_preview_settled();
            notch(&ctx, &dir, name, &vs)?;
        }
    }

    if std::env::args().any(|a| a == "--web") {
        let web_modes = [
            ("cynotch-home", NotchMode::Home, false),
            ("cynotch-music", NotchMode::Music, true),
            ("cynotch-memreduct", NotchMode::MemReduct, false),
            ("cynotch-shelf", NotchMode::Shelf, false),
            ("cynotch-volume", NotchMode::Volume, false),
        ];
        for (name, mode, lyrics) in web_modes {
            let mut ws = s.clone();
            ws.placement = models::NotchPlacement::Top;
            ws.mode = mode;
            ws.show_lyrics = lyrics;
            ws.is_pinned = name == "cynotch-home";
            ws.media.is_playing = true;
            ws.anim_time = 0.42;
            ws.capture.last_path = Some("C:\\Users\\user\\Pictures\\CyNotch\\cyshot_desktop_20261120.png".to_string());
            ws.capture.last_filename = "cyshot_desktop_20261120.png".to_string();
            ws.capture.last_dims = "1920 × 1080".to_string();
            ws.capture.clipboard_ok = true;
            ws.set_hovered(true);
            ws.sync_preview_settled();
            let (w, h) = (
                ws.current_width.round().max(1.0) as u32,
                ws.current_height.round().max(1.0) as u32,
            );
            let mut r = Renderer::new(w, h).unwrap();
            snap_transparent(&ctx, &dir, name, w, h, 20, |c| r.render_d2d(c, &ws, None))?;
        }
        // Also export a transparent Preferences screenshot for the v0.3.0 web comparison
        {
            let mut ps = s.clone();
            ps.settings_open = true;
            ps.set_settings_category(models::SettingsCategory::Motion);
            ps.set_settings_scroll_immediate(models::SettingsCategory::Motion, 0.0);
            ps.sync_preview_settled();
            let (sw, sh, _) = cynotch::settings_window::settings_dims_for_state(&ps);
            snap_transparent(&ctx, &dir, "cynotch-settings-motion", sw, sh, 16, |c| {
                let _ = cynotch::settings_window::render_settings_d2d(
                    c, &mut cache, &mut fonts, sw, sh, &ps, None,
                );
            })?;
        }
    }

    if std::env::args().any(|a| a == "--v031") {
        let dark_bg = [26u8, 12, 16, 255]; // Deep Cyno dark BGRA (#100C1A)
        let white_clamped_bg = [122u8, 119, 119, 255]; // Simulated Option B clamped #FFFFFF BGRA

        let mut base = AppState::new_for_preview();
        base.frosted_glass_enabled = true;
        base.windows_transparency_enabled = true;
        base.push_shelf_item(
            models::ShelfKind::Text,
            "cargo build --release".into(),
            "cargo build --release".into(),
        );
        base.push_shelf_item(
            models::ShelfKind::Image,
            "Image_20261121_2030.png".into(),
            "C:\\tmp\\Image_20261121_2030.png".into(),
        );
        base.push_shelf_item(
            models::ShelfKind::File,
            "CyNotch_Design_Spec.pdf".into(),
            "C:\\tmp\\CyNotch_Design_Spec.pdf".into(),
        );

        // 1. Vertical Quick Shelf (Frosted + Auto-Stash Amber Bell Pip)
        {
            let mut vs = base.clone();
            vs.placement = models::NotchPlacement::Left;
            vs.mode = NotchMode::Shelf;
            vs.set_hovered(true);
            vs.push_system_toast(
                "Discord".into(),
                "Cyno".into(),
                "Sent a screenshot to #design-review".into(),
                None,
            );
            vs.sync_preview_settled();
            let (w, h) = (vs.current_width.round() as u32, vs.current_height.round() as u32);
            let mut r = Renderer::new(w, h).unwrap();
            snap_bg(&ctx, &dir, "v031_vert_shelf_frosted", w, h, dark_bg, |c| {
                r.render_d2d(c, &vs, None)
            })?;
            let mut r2 = Renderer::new(w, h).unwrap();
            snap_bg(&ctx, &dir, "v031_vert_shelf_white_clamped", w, h, white_clamped_bg, |c| {
                r2.render_d2d(c, &vs, None)
            })?;

            // Solid obsidian fallback (Windows Transparency OFF)
            vs.windows_transparency_enabled = false;
            let mut r3 = Renderer::new(w, h).unwrap();
            snap_bg(&ctx, &dir, "v031_vert_shelf_solid", w, h, dark_bg, |c| {
                r3.render_d2d(c, &vs, None)
            })?;
        }

        // 2. Horizontal Quick Shelf (Frosted + Auto-Stash Amber Bell Pip)
        {
            let mut hs = base.clone();
            hs.placement = models::NotchPlacement::Top;
            hs.mode = NotchMode::Shelf;
            hs.set_hovered(true);
            hs.push_system_toast(
                "Discord".into(),
                "Cyno".into(),
                "Sent a screenshot to #design-review".into(),
                None,
            );
            hs.sync_preview_settled();
            let (w, h) = (hs.current_width.round() as u32, hs.current_height.round() as u32);
            let mut r = Renderer::new(w, h).unwrap();
            snap_bg(&ctx, &dir, "v031_horiz_shelf_frosted", w, h, dark_bg, |c| {
                r.render_d2d(c, &hs, None)
            })?;
        }

        // 3. Dynamic Island Notification Capsule (Collapsed + Countdown Ring)
        {
            let mut ns = base.clone();
            ns.placement = models::NotchPlacement::Top;
            ns.set_hovered(false);
            ns.push_system_toast(
                "Discord".into(),
                "now".into(),
                "Cyno sent a screenshot to #design-review".into(),
                None,
            );
            ns.sync_preview_settled();
            let (w, h) = (ns.current_width.round() as u32, ns.current_height.round() as u32);
            let mut r = Renderer::new(w, h).unwrap();
            snap_bg(&ctx, &dir, "v031_notif_capsule", w, h, dark_bg, |c| {
                r.render_d2d(c, &ns, None)
            })?;
        }

        // 4. Settings General (Borderless Glass + Zero Sliders)
        {
            let mut ss = base.clone();
            ss.settings_open = true;
            ss.set_settings_category(models::SettingsCategory::General);
            ss.set_settings_scroll_immediate(models::SettingsCategory::General, 220.0);
            ss.sync_preview_settled();
            let (sw, sh, _) = cynotch::settings_window::settings_dims_for_state(&ss);
            snap_bg(&ctx, &dir, "v031_settings_general_frosted", sw, sh, dark_bg, |c| {
                let _ = cynotch::settings_window::render_settings_d2d(
                    c, &mut cache, &mut fonts, sw, sh, &ss, None,
                );
            })?;
        }
    }
    println!("snapshots written to {dir}");
    Ok(())
}
