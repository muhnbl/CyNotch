use std::mem::ManuallyDrop;
use std::time::Instant;
use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;

use cynotch::painter_d2d::{D2DPainter, D2DResourceCache, color_to_d2d};
use cynotch::font_d2d::DWriteTextCache;
use cynotch::font_d2d::Weight;

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext, ID2D1Device, ID2D1DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.unwrap();
    let context = context.unwrap();

    let dxgi_device: IDXGIDevice = device.cast()?;
    let factory: ID2D1Factory1 = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
    let d2d_device = unsafe { factory.CreateDevice(&dxgi_device)? };
    let d2d_context = unsafe { d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };

    Ok((device, context, d2d_device, d2d_context))
}

fn create_target(ctx: &ID2D1DeviceContext, w: u32, h: u32) -> Result<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        colorContext: ManuallyDrop::new(None),
    };
    unsafe { ctx.CreateBitmap(D2D_SIZE_U { width: w, height: h }, None, 0, &props) }
}

#[test]
fn bench_d2d_perf() -> Result<()> {
    let (_d3d_device, d3d_context, _d2d_device, d2d_context) = create_device()?;
    let target = create_target(&d2d_context, 676, 253)?;
    unsafe { d2d_context.SetTarget(&target) };

    let mut cache = D2DResourceCache::new(&d2d_context)?;
    let mut fonts = DWriteTextCache::new()?;

    let iters = 40;
    let warmup = 3;

    // Scenarios
    // 1. Idle Capsule (676x253)
    let mut idle_cpu = Vec::with_capacity(iters);
    let mut idle_gpu = Vec::with_capacity(iters);

    for i in 0..(iters + warmup) {
        let start = Instant::now();
        unsafe { d2d_context.BeginDraw() };
        {
            let mut p = D2DPainter::new(&d2d_context, &mut cache, 1.0, 0.0, 0.0, 1.0);
            p.clear()?;
            // rounded shell
            p.rrect(10.0, 10.0, 656.0, 233.0, 20.0, color_to_d2d(14, 16, 26, 250, 1.0))?;
            // bevel stroke
            p.rrect_stroke(10.0, 10.0, 656.0, 233.0, 20.0, 1.0, color_to_d2d(235, 230, 255, 62, 1.0))?;
            // 2x text
            p.text(&mut fonts, "CyNotch Idle", 30.0, 50.0, 24.0, Weight::Bold, 246, 244, 255, 255)?;
            p.text(&mut fonts, "12:34 PM", 30.0, 90.0, 16.0, Weight::Regular, 165, 161, 194, 255)?;
        }
        unsafe { d2d_context.EndDraw(None, None)? };
        let cpu_done = start.elapsed();
        
        unsafe { d3d_context.Flush() };
        let gpu_done = start.elapsed();

        if i >= warmup {
            idle_cpu.push(cpu_done);
            idle_gpu.push(gpu_done);
        }
    }

    // 2. Expanded Heavy (676x253)
    let mut heavy_cpu = Vec::with_capacity(iters);
    let mut heavy_gpu = Vec::with_capacity(iters);

    for i in 0..(iters + warmup) {
        let start = Instant::now();
        unsafe { d2d_context.BeginDraw() };
        {
            let mut p = D2DPainter::new(&d2d_context, &mut cache, 1.0, 0.0, 0.0, 1.0);
            p.clear()?;
            // rounded shell
            p.rrect(0.0, 0.0, 676.0, 253.0, 24.0, color_to_d2d(14, 16, 26, 255, 1.0))?;
            
            // 4x elevated gradient cards (simplified)
            for j in 0..4 {
                let x = 20.0 + (j as f32) * 160.0;
                p.rrect_vertical_gradient(x, 40.0, 150.0, 180.0, 12.0, 
                    color_to_d2d(28, 30, 46, 255, 1.0),
                    color_to_d2d(16, 17, 28, 255, 1.0))?;
            }

            // 25x radial_glow
            for j in 0..25 {
                let x = 50.0 + (j as f32 % 5.0) * 120.0;
                let y = 50.0 + (j as f32 / 5.0) * 40.0;
                p.radial_glow(x, y, 30.0, 
                    color_to_d2d(124, 58, 237, 40, 1.0),
                    color_to_d2d(124, 58, 237, 0, 1.0))?;
            }

            // 8x text
            for j in 0..8 {
                p.text(&mut fonts, "Heavy Text Sample", 40.0, 20.0 + (j as f32) * 25.0, 14.0, Weight::Semibold, 246, 244, 255, 200)?;
            }

            // emoji
            p.text(&mut fonts, "🎵", 600.0, 20.0, 32.0, Weight::Regular, 255, 255, 255, 255)?;

            // clipped arc
            p.push_rrect_clip(550.0, 150.0, 100.0, 80.0, 10.0)?;
            p.arc(600.0, 190.0, 40.0, 0.0, std::f32::consts::PI, 4.0, color_to_d2d(155, 89, 252, 255, 1.0))?;
            p.pop_clip()?;
        }
        unsafe { d2d_context.EndDraw(None, None)? };
        let cpu_done = start.elapsed();
        
        unsafe { d3d_context.Flush() };
        let gpu_done = start.elapsed();

        if i >= warmup {
            heavy_cpu.push(cpu_done);
            heavy_gpu.push(gpu_done);
        }
    }

    let avg_idle_cpu = idle_cpu.iter().sum::<std::time::Duration>() / iters as u32;
    let avg_idle_gpu = idle_gpu.iter().sum::<std::time::Duration>() / iters as u32;
    let avg_heavy_cpu = heavy_cpu.iter().sum::<std::time::Duration>() / iters as u32;
    let avg_heavy_gpu = heavy_gpu.iter().sum::<std::time::Duration>() / iters as u32;

    println!("\nD2D Performance Benchmark (676x253):");
    println!("Scenario: D2D Idle Capsule");
    println!("  CPU Record:  {:?}", avg_idle_cpu);
    println!("  GPU Flush:   {:?}", avg_idle_gpu);
    println!("Scenario: D2D Expanded Heavy");
    println!("  CPU Record:  {:?}", avg_heavy_cpu);
    println!("  GPU Flush:   {:?}", avg_heavy_gpu);

    Ok(())
}
