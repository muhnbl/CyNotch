use std::mem::ManuallyDrop;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_MAP_OPTIONS_READ, D2D1_PRIMITIVE_BLEND_ADD, D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Bitmap1,
    ID2D1DeviceContext, ID2D1Factory1, ID2D1RenderTarget,
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
        ctx.SetTransform(&Matrix3x2::identity());
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
    let dir = std::env::args().nth(1).unwrap_or_else(|| "experiment_touched".into());
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

    // -------------------------------------------------------------
    // SPECIMEN 01: Dynamic Island Capsule (Touched Up)
    // - Primary: Obsidian Smoked Glass
    // - Secondary: Gleaming Silver-White #F8F7FF text
    // - Accent: Vibrant Amber-Gold (#FBBF24 / #FDE68A) countdown progress arc
    // -------------------------------------------------------------
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
        snap_transparent(&ctx, &dir, "1_notif_capsule_touched", w, h, |c| {
            r.render_d2d(c, &ns, None);

            // Touch-up: Overdraw the dismiss countdown arc in radiant amber gold!
            let dismiss_cx = (w as f32 - 24.0).max(120.0);
            let cy = h as f32 * 0.5;
            let radius = 13.5;
            unsafe {
                let rt: ID2D1RenderTarget = c.cast().unwrap();
                let brush_amber = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 1.0,
                        },
                        None,
                    )
                    .unwrap();

                // Draw radiant 2.2px Amber-Gold progress arc
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                c.DrawEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: dismiss_cx,
                            y: cy,
                        },
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &brush_amber,
                    2.0,
                    None,
                );

                // Gentle amber bloom around the dismiss ring
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
                let brush_bloom = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 253.0 / 255.0,
                            g: 230.0 / 255.0,
                            b: 138.0 / 255.0,
                            a: 0.28,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: dismiss_cx,
                            y: cy,
                        },
                        radiusX: radius + 3.0,
                        radiusY: radius + 3.0,
                    },
                    &brush_bloom,
                );
            }
        })?;
    }

    // -------------------------------------------------------------
    // SPECIMEN 02: Music Player View (Touched Up)
    // - Primary: Obsidian purple body
    // - Secondary: Silver-white typography & transport controls
    // - Accent: Amber-Gold Scrubber & Glowing Thumb
    // -------------------------------------------------------------
    {
        let mut ms = base.clone();
        ms.placement = models::NotchPlacement::Top;
        ms.mode = NotchMode::Music;
        ms.media.is_playing = true;
        ms.anim_time = 0.42;
        ms.set_hovered(true);
        ms.sync_preview_settled();
        let (w, h) = (ms.current_width.round() as u32, ms.current_height.round() as u32);
        let mut r = Renderer::new(w, h).unwrap();
        snap_transparent(&ctx, &dir, "2_music_touched", w, h, |c| {
            r.render_d2d(c, &ms, None);

            // Touch-up: Overdraw the seek progress bar and thumb with warm Amber-Gold!
            let card_y = 6.0;
            let pad_x = AppState::MUSIC_SCRUBBER_X;
            let inner_w = AppState::MUSIC_SCRUBBER_W;
            let title_y = card_y + 26.0;
            let artist_y = title_y + 19.0;
            let prog_y = artist_y + 13.0;
            let played_pct = if ms.media.duration_ms > 0 {
                (ms.media.position_ms as f32 / ms.media.duration_ms as f32).clamp(0.0, 1.0)
            } else {
                0.42
            };
            let played_w = inner_w * played_pct;

            unsafe {
                let rt: ID2D1RenderTarget = c.cast().unwrap();

                // 1. Amber-Gold Gradient Progress Fill
                let stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: D2D1_COLOR_F {
                            r: 245.0 / 255.0,
                            g: 158.0 / 255.0,
                            b: 11.0 / 255.0,
                            a: 0.95,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: D2D1_COLOR_F {
                            r: 253.0 / 255.0,
                            g: 230.0 / 255.0,
                            b: 138.0 / 255.0,
                            a: 1.0,
                        },
                    },
                ];
                let grad_stops = rt
                    .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                    .unwrap();
                let grad_brush = rt
                    .CreateLinearGradientBrush(
                        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                            startPoint: D2D_POINT_2F {
                                x: pad_x,
                                y: prog_y,
                            },
                            endPoint: D2D_POINT_2F {
                                x: pad_x + played_w,
                                y: prog_y,
                            },
                        },
                        None,
                        &grad_stops,
                    )
                    .unwrap();

                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                c.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: pad_x,
                            top: prog_y,
                            right: pad_x + played_w,
                            bottom: prog_y + 6.5,
                        },
                        radiusX: 3.25,
                        radiusY: 3.25,
                    },
                    &grad_brush,
                );

                // 2. Glowing Amber Thumb
                let thumb_x =
                    (pad_x + inner_w * played_pct).clamp(pad_x + 5.0, pad_x + inner_w - 3.0);
                let thumb_y = prog_y + 3.25;
                let thumb_r = 5.6;

                // Ambient amber radial glow
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
                let brush_glow = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 0.35,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: thumb_x,
                            y: thumb_y,
                        },
                        radiusX: 13.0,
                        radiusY: 13.0,
                    },
                    &brush_glow,
                );

                // Pure silver-white core thumb with amber dot
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                let brush_white = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: thumb_x,
                            y: thumb_y,
                        },
                        radiusX: thumb_r,
                        radiusY: thumb_r,
                    },
                    &brush_white,
                );

                let brush_amber_core = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 245.0 / 255.0,
                            g: 158.0 / 255.0,
                            b: 11.0 / 255.0,
                            a: 1.0,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F {
                            x: thumb_x,
                            y: thumb_y,
                        },
                        radiusX: thumb_r * 0.44,
                        radiusY: thumb_r * 0.44,
                    },
                    &brush_amber_core,
                );
            }
        })?;
    }

    // -------------------------------------------------------------
    // SPECIMEN 03: MemReduct Manager Hover (Touched Up)
    // - Primary: Obsidian Purple Cards
    // - Secondary: Crisp Silver Typography
    // - Accent: Liquid Amber-Gold Bloom on Active SysWS Chip
    // -------------------------------------------------------------
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
        let mut r = Renderer::new(w, h).unwrap();
        snap_transparent(&ctx, &dir, "3_memreduct_hover_touched", w, h, |c| {
            r.render_d2d(c, &ms, Some((px * scale, py * scale)));

            // Touch-up: Infuse warm amber gold bloom on the hovered SysWS chip!
            let chip_x = col2_x + 14.0 + (pill_w + 4.5);
            let chip_y = 60.0 + 29.0 - 4.0;
            let chip_h = 24.0;

            unsafe {
                let rt: ID2D1RenderTarget = c.cast().unwrap();

                // Gentle bottom-up warm amber liquid bloom
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
                let stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: D2D1_COLOR_F {
                            r: 253.0 / 255.0,
                            g: 230.0 / 255.0,
                            b: 138.0 / 255.0,
                            a: 0.0,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 0.42,
                        },
                    },
                ];
                let grad_stops = rt
                    .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                    .unwrap();
                let grad_brush = rt
                    .CreateLinearGradientBrush(
                        &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                            startPoint: D2D_POINT_2F {
                                x: chip_x,
                                y: chip_y,
                            },
                            endPoint: D2D_POINT_2F {
                                x: chip_x,
                                y: chip_y + chip_h,
                            },
                        },
                        None,
                        &grad_stops,
                    )
                    .unwrap();

                c.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: chip_x,
                            top: chip_y,
                            right: chip_x + pill_w,
                            bottom: chip_y + chip_h,
                        },
                        radiusX: 6.0,
                        radiusY: 6.0,
                    },
                    &grad_brush,
                );

                // Glowing amber outline
                let brush_amber_border = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 0.70,
                        },
                        None,
                    )
                    .unwrap();
                c.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: chip_x,
                            top: chip_y,
                            right: chip_x + pill_w,
                            bottom: chip_y + chip_h,
                        },
                        radiusX: 6.0,
                        radiusY: 6.0,
                    },
                    &brush_amber_border,
                    1.2,
                    None,
                );
            }
        })?;
    }

    // -------------------------------------------------------------
    // SPECIMEN 04: Vertical Quick Shelf (Touched Up)
    // - Primary: Obsidian Liquid Glass
    // - Secondary: Pure Silver-White #F8F7FF Typography & Heroicons
    // - Accent: Vibrant Amber-Gold (#FBBF24) Auto-Stash Bell Pip
    // -------------------------------------------------------------
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
        snap_transparent(&ctx, &dir, "4_vertical_shelf_touched", w, h, |c| {
            r.render_d2d(c, &vs, None);

            // Touch-up: Emphasize the Auto-Stash Bell amber pip with vivid amber glow!
            // Bell icon is at top header (near w - 24, y = 24)
            let bell_cx = w as f32 - 26.0;
            let bell_cy = 24.0;
            let pip_x = bell_cx + 5.5;
            let pip_y = bell_cy - 5.5;

            unsafe {
                let rt: ID2D1RenderTarget = c.cast().unwrap();

                // Amber radial halo
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
                let brush_amber_glow = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 0.50,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: pip_x, y: pip_y },
                        radiusX: 7.0,
                        radiusY: 7.0,
                    },
                    &brush_amber_glow,
                );

                // Solid crisp Amber Pip
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                let brush_amber = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 1.0,
                        },
                        None,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: pip_x, y: pip_y },
                        radiusX: 3.2,
                        radiusY: 3.2,
                    },
                    &brush_amber,
                );

                // Crisp silver outer rim on pip
                let brush_silver = rt
                    .CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 248.0 / 255.0,
                            g: 247.0 / 255.0,
                            b: 1.0,
                            a: 0.85,
                        },
                        None,
                    )
                    .unwrap();
                c.DrawEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: pip_x, y: pip_y },
                        radiusX: 3.2,
                        radiusY: 3.2,
                    },
                    &brush_silver,
                    0.8,
                    None,
                );
            }
        })?;
    }

    // -------------------------------------------------------------
    // SPECIMEN 05: Corner Portal Glow (Touched Up - Tri-Color Chromatic Aura)
    // - Primary: Deep Obsidian Violet Halo (Outer)
    // - Secondary: Brilliant Silver-White Bezel Crest (Rim)
    // - Accent: Celestial Amber-Gold Warm Core (Origin)
    // -------------------------------------------------------------
    {
        let gw = cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN;
        let gh = cynotch::edge_glow::EDGE_GLOW_CORNER_SPAN;
        snap_transparent(&ctx, &dir, "5_edge_glow_corner_touched", gw, gh, |c| {
            let mut glow_cache = D2DResourceCache::new(c).unwrap();
            cynotch::edge_glow::render_edge_glow_d2d(
                c,
                &mut glow_cache,
                cynotch::edge_glow::EdgeGlowAnchor::TopLeft,
                gw,
                gh,
                0.96,
            );

            // Multi-temperature touch-up: Add warm Amber-Gold celestial micro-core + Silver-White bezel crest!
            unsafe {
                let rt: ID2D1RenderTarget = c.cast().unwrap();

                // 1. Silver-white rim crest along physical bezel corner (origin: 0,0)
                c.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD);
                let silver_stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: D2D1_COLOR_F {
                            r: 248.0 / 255.0,
                            g: 247.0 / 255.0,
                            b: 1.0,
                            a: 0.45,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 0.4,
                        color: D2D1_COLOR_F {
                            r: 192.0 / 255.0,
                            g: 132.0 / 255.0,
                            b: 252.0 / 255.0,
                            a: 0.20,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: D2D1_COLOR_F {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        },
                    },
                ];
                let s_coll = rt
                    .CreateGradientStopCollection(
                        &silver_stops,
                        D2D1_GAMMA_2_2,
                        D2D1_EXTEND_MODE_CLAMP,
                    )
                    .unwrap();
                let s_brush = rt
                    .CreateRadialGradientBrush(
                        &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                            center: D2D_POINT_2F { x: 0.0, y: 0.0 },
                            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                            radiusX: gw as f32 * 0.40,
                            radiusY: gh as f32 * 0.40,
                        },
                        None,
                        &s_coll,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: 0.0, y: 0.0 },
                        radiusX: gw as f32 * 0.40,
                        radiusY: gh as f32 * 0.40,
                    },
                    &s_brush,
                );

                // 2. Celestial Amber-Gold Warm Micro-Core (3200K focal light)
                let amber_stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: D2D1_COLOR_F {
                            r: 253.0 / 255.0,
                            g: 230.0 / 255.0,
                            b: 138.0 / 255.0,
                            a: 0.65,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 0.25,
                        color: D2D1_COLOR_F {
                            r: 251.0 / 255.0,
                            g: 191.0 / 255.0,
                            b: 36.0 / 255.0,
                            a: 0.38,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: D2D1_COLOR_F {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        },
                    },
                ];
                let a_coll = rt
                    .CreateGradientStopCollection(
                        &amber_stops,
                        D2D1_GAMMA_2_2,
                        D2D1_EXTEND_MODE_CLAMP,
                    )
                    .unwrap();
                let a_brush = rt
                    .CreateRadialGradientBrush(
                        &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                            center: D2D_POINT_2F { x: 0.0, y: 0.0 },
                            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                            radiusX: 95.0,
                            radiusY: 95.0,
                        },
                        None,
                        &a_coll,
                    )
                    .unwrap();
                c.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: 0.0, y: 0.0 },
                        radiusX: 95.0,
                        radiusY: 95.0,
                    },
                    &a_brush,
                );
            }
        })?;
    }

    println!("All 5 Touched-Up Specimen Captures generated in {dir}");
    Ok(())
}
