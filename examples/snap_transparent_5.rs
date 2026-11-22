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

fn snap_transparent(
    ctx: &ID2D1DeviceContext,
    dir: &str,
    name: &str,
    w: u32,
    h: u32,
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
        let (ow, oh) = (w, h);
        let mut out = vec![0u8; (ow * oh * 4) as usize];
        for y in 0..h {
            let src = std::slice::from_raw_parts(
                mapped.bits.add((y * mapped.pitch) as usize),
                (w * 4) as usize,
            );
            for x in 0..w {
                let s = &src[(x * 4) as usize..(x * 4 + 4) as usize];
                let o = ((y * ow + x) * 4) as usize;
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

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "experiment_transparent".into());
    std::fs::create_dir_all(&dir).ok();
    let ctx = create_ctx()?;

    let mut base = AppState::new_for_preview();
    base.frosted_glass_enabled = true;
    base.windows_transparency_enabled = true;
    base.telemetry.memory_percent = 57.0;
    base.telemetry.memory_used_gb = 18.2;
    base.telemetry.memory_total_gb = 31.9;
    base.telemetry.cpu_usage = 12.4;
    base.telemetry.net_down_kbps = 1480.0;
    base.telemetry.net_up_kbps = 264.0;
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
        let mut r = Renderer::new(w, h);
        snap_transparent(&ctx, &dir, "1_vertical_shelf_transparent", w, h, |c| {
            r.render_d2d(c, &vs, None)
        })?;
    }

    // 2. Dynamic Island Notification Capsule (Collapsed + Countdown Ring)
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
        let mut r = Renderer::new(w, h);
        snap_transparent(&ctx, &dir, "2_notif_capsule_transparent", w, h, |c| {
            r.render_d2d(c, &ns, None)
        })?;
    }

    // 3. Music Player View (Seek Scrubber 1:1, Volume, Controls)
    {
        let mut ms = base.clone();
        ms.placement = models::NotchPlacement::Top;
        ms.mode = NotchMode::Music;
        ms.media.is_playing = true;
        ms.anim_time = 0.42;
        ms.set_hovered(true);
        ms.sync_preview_settled();
        let (w, h) = (ms.current_width.round() as u32, ms.current_height.round() as u32);
        let mut r = Renderer::new(w, h);
        snap_transparent(&ctx, &dir, "3_music_transparent", w, h, |c| {
            r.render_d2d(c, &ms, None)
        })?;
    }

    // 4. MemReduct Manager & Button Hover
    {
        let mut ms = base.clone();
        ms.placement = models::NotchPlacement::Top;
        ms.mode = NotchMode::MemReduct;
        ms.set_hovered(true);
        ms.sync_preview_settled();
        let bw = ms.base_expanded_width();
        let col_w = (bw - 40.0 - 12.0) * 0.5;
        let col2_x = 20.0 + col_w + 12.0;
        let pill_w = (col_w - 28.0 - 4.5 * 3.0) / 4.0;
        let px = col2_x + 14.0 + (pill_w + 4.5) + pill_w * 0.5;
        let py = 60.0 + 29.0 + 8.0;
        let scale = ms.expanded_scale();
        let (w, h) = (ms.current_width.round() as u32, ms.current_height.round() as u32);
        let mut r = Renderer::new(w, h);
        snap_transparent(&ctx, &dir, "4_memreduct_hover_transparent", w, h, |c| {
            r.render_d2d(c, &ms, Some((px * scale, py * scale)))
        })?;
    }

    // 5. Corner Portal Glow (DirectComposition 32-stop radial float aura)
    {
        let mut glow_cache = D2DResourceCache::new(&ctx)?;
        let gw = cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN;
        let gh = cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN;
        snap_transparent(&ctx, &dir, "5_edge_glow_corner_transparent", gw, gh, |c| {
            cynotch::edge_glow::render_edge_glow_d2d(
                c,
                &mut glow_cache,
                cynotch::edge_glow::EdgeGlowAnchor::TopLeft,
                gw,
                gh,
                0.96,
            );
        })?;
    }

    println!("Transparent snapshots written to {dir}");
    Ok(())
}
