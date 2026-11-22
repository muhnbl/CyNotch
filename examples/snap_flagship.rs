use std::mem::ManuallyDrop;
use windows::Foundation::Numerics::Matrix3x2;
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
    path: &std::path::Path,
    w: u32,
    h: u32,
    draw: impl FnOnce(&ID2D1DeviceContext),
) -> Result<()> {
    let target = bitmap(ctx, w, h, false)?;
    let readback = bitmap(ctx, w, h, true)?;
    unsafe {
        ctx.SetTarget(&target);
        ctx.BeginDraw();
        ctx.SetTransform(&Matrix3x2::identity());
        ctx.Clear(None);
        draw(ctx);
        if let Err(e) = ctx.EndDraw(None, None) {
            eprintln!("EndDraw failed: {e:?}");
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
        if !cynotch::capture::save_bgra_png_alpha(path, ow, oh, &out) {
            eprintln!("PNG save failed to {:?}", path);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let ctx = create_ctx()?;

    let mut ms = AppState::new_for_preview();
    ms.frosted_glass_enabled = true;
    ms.windows_transparency_enabled = true;
    ms.placement = models::NotchPlacement::Top;
    ms.mode = NotchMode::Music;
    ms.media.is_playing = true;
    ms.media.title = "Starboy (feat. Daft Punk)".into();
    ms.media.artist = "The Weeknd".into();
    ms.media.album = "Starboy".into();
    ms.media.position_ms = 102_000;
    ms.media.duration_ms = 230_000;
    ms.anim_time = 0.42;
    ms.set_hovered(true);
    ms.sync_preview_settled();

    let (w, h) = (ms.current_width.round() as u32, ms.current_height.round() as u32);
    let mut r = Renderer::new(w, h);

    let p1 = std::path::PathBuf::from(r"C:\Users\muhna\.gemini\antigravity\scratch\CyNotch-v0.3.1\cynotch_flagship_music.png");
    let p2 = std::path::PathBuf::from(r"C:\Users\muhna\.gemini\antigravity\brain\78655c5b-3932-4943-ac62-62da1685b003\cynotch_flagship_music.png");

    snap_transparent(&ctx, &p1, w, h, |c| {
        r.render_d2d(c, &ms, None);
    })?;
    snap_transparent(&ctx, &p2, w, h, |c| {
        r.render_d2d(c, &ms, None);
    })?;

    println!("Flagship Music rendered directly from engine ({}x{}): {:?} and {:?}", w, h, p1, p2);
    Ok(())
}
