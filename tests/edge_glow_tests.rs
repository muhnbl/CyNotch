use cynotch::edge_glow::{
    EDGE_GLOW_DEPTH, EDGE_GLOW_SIDE_SPAN, EDGE_GLOW_TOP_SPAN, compute_edge_glow_rect,
};
use cynotch::models::NotchPlacement;
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
    let (tx, ty, tw, th) = compute_edge_glow_rect(mon, NotchPlacement::Top, 960, 28, 1.0);
    assert_eq!(ty, 0);
    assert_eq!(tw, EDGE_GLOW_TOP_SPAN);
    assert_eq!(th, EDGE_GLOW_DEPTH);
    assert_eq!(tx, 960 - (EDGE_GLOW_TOP_SPAN as i32) / 2);

    // Left placement: strictly flush at x = mon.left (0)
    let (lx, ly, lw, lh) = compute_edge_glow_rect(mon, NotchPlacement::Left, 32, 540, 1.0);
    assert_eq!(lx, 0);
    assert_eq!(lw, EDGE_GLOW_DEPTH);
    assert_eq!(lh, EDGE_GLOW_SIDE_SPAN);
    assert_eq!(ly, 540 - (EDGE_GLOW_SIDE_SPAN as i32) / 2);

    // Right placement: strictly flush at right edge (x + w == mon.right)
    let (rx, ry, rw, rh) = compute_edge_glow_rect(mon, NotchPlacement::Right, 1890, 540, 1.0);
    assert_eq!(rx + rw as i32, 1920);
    assert_eq!(rw, EDGE_GLOW_DEPTH);
    assert_eq!(rh, EDGE_GLOW_SIDE_SPAN);
    assert_eq!(ry, 540 - (EDGE_GLOW_SIDE_SPAN as i32) / 2);
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
    let (tx, ty, tw, _th) = compute_edge_glow_rect(left_mon, NotchPlacement::Top, -1900, 20, 1.0);
    assert_eq!(tx, -1920);
    assert_eq!(ty, 0);
    assert_eq!(tw, EDGE_GLOW_TOP_SPAN);

    // Right edge of negative-coordinate monitor
    let (rx, ry, rw, rh) = compute_edge_glow_rect(left_mon, NotchPlacement::Right, -20, 1050, 1.0);
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
    let (rx, _ry, rw, rh) = compute_edge_glow_rect(mon, NotchPlacement::Right, 2540, 720, 1.5);
    assert_eq!(rx + rw as i32, 2560);
    assert_eq!(rw, (EDGE_GLOW_DEPTH as f32 * 1.5).round() as u32);
    assert_eq!(rh, (EDGE_GLOW_SIDE_SPAN as f32 * 1.5).round() as u32);
}
