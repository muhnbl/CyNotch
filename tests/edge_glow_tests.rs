use cynotch::edge_glow::{
    EDGE_GLOW_CORNER_SPAN, EDGE_GLOW_DEPTH, EDGE_GLOW_SIDE_SPAN, EDGE_GLOW_TOP_SPAN,
    EdgeGlowAnchor, compute_edge_glow_rect, render_edge_glow_d2d,
};
use cynotch::models::NotchPlacement;
use cynotch::painter_d2d::D2DResourceCache;
use windows::Win32::Foundation::RECT;

#[test]
fn edge_glow_hugs_physical_monitor_edges_for_all_placements() {
    let mon = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    // Top placement: strictly flush at y = mon.top (0)
    let (tx, ty, tw, th) = compute_edge_glow_rect(mon, EdgeGlowAnchor::Top, 960, 28, 1.0);
    assert_eq!(ty, 0);
    assert_eq!(tw, EDGE_GLOW_TOP_SPAN);
    assert_eq!(th, EDGE_GLOW_DEPTH);
    assert_eq!(tx, 960 - (EDGE_GLOW_TOP_SPAN as i32) / 2);

    // Left placement: strictly flush at x = mon.left (0)
    let (lx, ly, lw, lh) = compute_edge_glow_rect(mon, EdgeGlowAnchor::Left, 32, 540, 1.0);
    assert_eq!(lx, 0);
    assert_eq!(lw, EDGE_GLOW_DEPTH);
    assert_eq!(lh, EDGE_GLOW_SIDE_SPAN);
    assert_eq!(ly, 540 - (EDGE_GLOW_SIDE_SPAN as i32) / 2);

    // Right placement: strictly flush at right edge (x + w == mon.right)
    let (rx, ry, rw, rh) = compute_edge_glow_rect(mon, EdgeGlowAnchor::Right, 1890, 540, 1.0);
    assert_eq!(rx + rw as i32, 1920);
    assert_eq!(rw, EDGE_GLOW_DEPTH);
    assert_eq!(rh, EDGE_GLOW_SIDE_SPAN);
    assert_eq!(ry, 540 - (EDGE_GLOW_SIDE_SPAN as i32) / 2);

    // Top-left corner: flush at (mon.left, mon.top)
    let (tlx, tly, tlw, tlh) = compute_edge_glow_rect(mon, EdgeGlowAnchor::TopLeft, 80, 20, 1.0);
    assert_eq!(tlx, 0);
    assert_eq!(tly, 0);
    assert_eq!(tlw, EDGE_GLOW_CORNER_SPAN);
    assert_eq!(tlh, EDGE_GLOW_CORNER_SPAN);

    // Top-right corner: flush at (mon.right - w, mon.top)
    let (trx, try_, trw, trh) =
        compute_edge_glow_rect(mon, EdgeGlowAnchor::TopRight, 1840, 20, 1.0);
    assert_eq!(trx + trw as i32, 1920);
    assert_eq!(try_, 0);
    assert_eq!(trw, EDGE_GLOW_CORNER_SPAN);
    assert_eq!(trh, EDGE_GLOW_CORNER_SPAN);
}

#[test]
fn edge_glow_anchor_prefers_corners_when_docked() {
    assert_eq!(
        EdgeGlowAnchor::from_placement_and_corners(NotchPlacement::Top, true, false),
        EdgeGlowAnchor::TopLeft
    );
    assert_eq!(
        EdgeGlowAnchor::from_placement_and_corners(NotchPlacement::Left, true, false),
        EdgeGlowAnchor::TopLeft
    );
    assert_eq!(
        EdgeGlowAnchor::from_placement_and_corners(NotchPlacement::Top, false, true),
        EdgeGlowAnchor::TopRight
    );
    assert_eq!(
        EdgeGlowAnchor::from_placement_and_corners(NotchPlacement::Right, false, true),
        EdgeGlowAnchor::TopRight
    );
    assert_eq!(
        EdgeGlowAnchor::from_placement_and_corners(NotchPlacement::Top, false, false),
        EdgeGlowAnchor::Top
    );
}

#[test]
fn edge_glow_clamps_to_negative_multi_monitor_bounds() {
    let left_mon = RECT {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 1080,
    };

    // Near top-left corner of negative-coordinate monitor
    let (tx, ty, tw, _th) = compute_edge_glow_rect(left_mon, EdgeGlowAnchor::Top, -1900, 20, 1.0);
    assert_eq!(tx, -1920);
    assert_eq!(ty, 0);
    assert_eq!(tw, EDGE_GLOW_TOP_SPAN);

    // Corner-docked top-right on negative-coordinate monitor
    let (trx, try_, trw, trh) =
        compute_edge_glow_rect(left_mon, EdgeGlowAnchor::TopRight, -40, 20, 1.0);
    assert_eq!(trx + trw as i32, 0);
    assert_eq!(try_, 0);
    assert_eq!(trw, EDGE_GLOW_CORNER_SPAN);
    assert_eq!(trh, EDGE_GLOW_CORNER_SPAN);

    // Right edge of negative-coordinate monitor
    let (rx, ry, rw, rh) = compute_edge_glow_rect(left_mon, EdgeGlowAnchor::Right, -20, 1050, 1.0);
    assert_eq!(rx + rw as i32, 0);
    assert_eq!(ry + rh as i32, 1080);
}

#[test]
fn edge_glow_volumetric_portal_depth_and_dpi_scaling() {
    assert!(
        EDGE_GLOW_DEPTH >= 96,
        "volumetric portal bloom requires generous depth (>= 96px)"
    );
    let mon = RECT {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    let (rx, _ry, rw, rh) = compute_edge_glow_rect(mon, EdgeGlowAnchor::Right, 2540, 720, 1.5);
    assert_eq!(rx + rw as i32, 2560);
    assert_eq!(rw, (EDGE_GLOW_DEPTH as f32 * 1.5).round() as u32);
    assert_eq!(rh, (EDGE_GLOW_SIDE_SPAN as f32 * 1.5).round() as u32);
}

/// Renders an edge glow anchor into an offscreen WARP Direct2D bitmap and returns BGRA pixels.
fn render_glow_to_bgra(anchor: EdgeGlowAnchor, w: u32, h: u32) -> Vec<u8> {
    use windows::Win32::Graphics::Direct2D::Common::*;
    use windows::Win32::Graphics::Direct2D::*;
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::core::Interface;

    unsafe {
        let mut d3d_dev = None;
        let mut d3d_ctx = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_dev),
            None,
            Some(&mut d3d_ctx),
        )
        .expect("WARP D3D11 device");
        let d3d_dev = d3d_dev.unwrap();
        let d3d_ctx = d3d_ctx.unwrap();
        let dxgi_dev: IDXGIDevice = d3d_dev.cast().unwrap();

        let factory: ID2D1Factory1 =
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
        let d2d_dev = factory.CreateDevice(&dxgi_dev).unwrap();
        let d2d_ctx = d2d_dev
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
            .unwrap();

        let tex_desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            ..Default::default()
        };
        let mut tex = None;
        d3d_dev
            .CreateTexture2D(&tex_desc, None, Some(&mut tex))
            .unwrap();
        let tex = tex.unwrap();
        let surface: IDXGISurface = tex.cast().unwrap();

        let bmp_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        let target = d2d_ctx
            .CreateBitmapFromDxgiSurface(&surface, Some(&bmp_props))
            .unwrap();
        d2d_ctx.SetTarget(&target);

        let mut cache = D2DResourceCache::new(&d2d_ctx).unwrap();
        d2d_ctx.BeginDraw();
        render_edge_glow_d2d(&d2d_ctx, &mut cache, anchor, w, h, 1.0);
        d2d_ctx.EndDraw(None, None).unwrap();

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..tex_desc
        };
        let mut staging = None;
        d3d_dev
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .unwrap();
        let staging = staging.unwrap();
        d3d_ctx.CopyResource(&staging, &tex);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        d3d_ctx
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .unwrap();
        let mut out = vec![0u8; (w * h * 4) as usize];
        let src = mapped.pData as *const u8;
        for row in 0..h {
            let s_off = (row * mapped.RowPitch) as usize;
            let d_off = (row * w * 4) as usize;
            std::ptr::copy_nonoverlapping(
                src.add(s_off),
                out.as_mut_ptr().add(d_off),
                (w * 4) as usize,
            );
        }
        d3d_ctx.Unmap(&staging, 0);
        out
    }
}

#[test]
fn corner_edge_glow_radiates_from_corner_with_zero_cutoff() {
    let w = 240u32;
    let h = 240u32;
    let alpha_at = |buf: &[u8], x: u32, y: u32| -> u8 { buf[((y * w + x) * 4 + 3) as usize] };

    // TopLeft: brightest at (0, 0), fades diagonally, 0 at far inner edges
    let tl = render_glow_to_bgra(EdgeGlowAnchor::TopLeft, w, h);
    let tl_corner = alpha_at(&tl, 1, 1);
    let tl_mid_top = alpha_at(&tl, w / 2, 1);
    let tl_far_right = alpha_at(&tl, w - 1, 1);
    let tl_far_bottom = alpha_at(&tl, 1, h - 1);
    assert!(
        tl_corner > 100,
        "TopLeft corner should be bright, got {tl_corner}"
    );
    assert!(
        tl_corner > tl_mid_top * 2,
        "TopLeft must peak at corner ({tl_corner}), not mid-top ({tl_mid_top})"
    );
    assert_eq!(tl_far_right, 0, "TopLeft far right edge must be 0 alpha");
    assert_eq!(tl_far_bottom, 0, "TopLeft far bottom edge must be 0 alpha");

    // TopRight: brightest at (w - 1, 0), fades diagonally, 0 at far inner edges
    let tr = render_glow_to_bgra(EdgeGlowAnchor::TopRight, w, h);
    let tr_corner = alpha_at(&tr, w - 2, 1);
    let tr_mid_top = alpha_at(&tr, w / 2, 1);
    let tr_far_left = alpha_at(&tr, 0, 1);
    let tr_far_bottom = alpha_at(&tr, w - 2, h - 1);
    assert!(
        tr_corner > 100,
        "TopRight corner should be bright, got {tr_corner}"
    );
    assert!(
        tr_corner > tr_mid_top * 2,
        "TopRight must peak at corner ({tr_corner}), not mid-top ({tr_mid_top})"
    );
    assert_eq!(tr_far_left, 0, "TopRight far left edge must be 0 alpha");
    assert_eq!(tr_far_bottom, 0, "TopRight far bottom edge must be 0 alpha");
}

#[test]
fn edge_glow_overlay_is_truly_click_through_cross_process() {
    use cynotch::edge_glow::EdgeGlowOverlay;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, HWND_TOPMOST, IsWindowVisible, WS_EX_LAYERED,
        WS_EX_TRANSPARENT, WindowFromPoint,
    };

    let rect = (120, 40, 240, 120);
    let mut overlay =
        EdgeGlowOverlay::create(rect).expect("EdgeGlowOverlay::create should succeed");
    overlay.sync(HWND_TOPMOST, rect, EdgeGlowAnchor::Top, 0.85);
    let hwnd = overlay.hwnd();

    unsafe {
        assert!(
            IsWindowVisible(hwnd).as_bool(),
            "EdgeGlowOverlay should be visible after sync with intensity > 0.003"
        );
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        assert_ne!(
            ex & WS_EX_TRANSPARENT.0,
            0,
            "EdgeGlowOverlay must have WS_EX_TRANSPARENT"
        );
        assert_ne!(
            ex & WS_EX_LAYERED.0,
            0,
            "EdgeGlowOverlay must have WS_EX_LAYERED so WS_EX_TRANSPARENT passes clicks through cross-process"
        );
        let hit = WindowFromPoint(POINT {
            x: rect.0 + 60,
            y: rect.1 + 40,
        });
        assert_ne!(
            hit, hwnd,
            "WindowFromPoint must pass through EdgeGlowOverlay instead of intercepting clicks near the notch"
        );
    }
}
