//! Pure geometry for the paired frosted-blur backdrop window.
//!
//! No Win32 imports on purpose: everything here is plain integer math so it can
//! be unit-tested on any host. `backdrop.rs` turns a [`Placement`] into real
//! `SetWindowPos` / `SetWindowRgn` calls.

/// Smallest backdrop side in pixels. Anything smaller is not worth a window.
pub const MIN_SIDE: i32 = 8;

/// Visible shell silhouette, in the *content window's* client coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellGeom {
    pub ox: i32,
    pub oy: i32,
    pub w: i32,
    pub h: i32,
    /// Corner diameter (2 * max corner radius), same meaning as `CreateRoundRectRgn`.
    pub diam: i32,
    /// Edge-flush (docked) style: the screen-edge side is square.
    pub flush: bool,
    /// 0 = top, 1 = left, 2 = right (matches `NotchPlacement`).
    pub place: u8,
    /// Bit mask of corners that must stay square (1 = TL, 2 = TR, 4 = BR, 8 = BL).
    pub sharp_mask: u8,
}

/// Final backdrop window placement, in screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub diam: i32,
    pub flush: bool,
    pub place: u8,
    pub sharp_mask: u8,
}

impl ShellGeom {
    /// Builds a [`ShellGeom`] from the renderer's float shell rectangle.
    /// The right/bottom edges are rounded first so width/height never drift
    /// from the pixel grid the renderer draws on.
    #[allow(clippy::too_many_arguments)]
    pub fn from_shell(
        ox: f32,
        oy: f32,
        w: f32,
        h: f32,
        max_radius: f32,
        flush: bool,
        place: u8,
        sharp_mask: u8,
    ) -> Self {
        let x0 = ox.round() as i32;
        let y0 = oy.round() as i32;
        let x1 = (ox + w).round() as i32;
        let y1 = (oy + h).round() as i32;
        Self {
            ox: x0,
            oy: y0,
            w: (x1 - x0).max(1),
            h: (y1 - y0).max(1),
            diam: (max_radius * 2.0).round() as i32,
            flush,
            place,
            sharp_mask,
        }
    }

    /// Builds a [`ShellGeom`] clipped to the content window canvas `[0, cw] x [0, ch]`.
    /// When a sliding notification banner extends past an outer screen edge, the
    /// offscreen part is clipped and `flush`/`place` are updated so the clipped edge
    /// stays square and never bleeds onto adjacent monitors. Returns `None` if the
    /// visible portion is smaller than [`MIN_SIDE`].
    #[allow(clippy::too_many_arguments)]
    pub fn from_shell_clipped(
        ox: f32,
        oy: f32,
        w: f32,
        h: f32,
        cw: i32,
        ch: i32,
        max_radius: f32,
        mut flush: bool,
        mut place: u8,
        sharp_mask: u8,
    ) -> Option<Self> {
        let cw = cw.max(1);
        let ch = ch.max(1);
        let raw_x0 = ox.round() as i32;
        let raw_y0 = oy.round() as i32;
        let raw_x1 = (ox + w).round() as i32;
        let raw_y1 = (oy + h).round() as i32;

        let x0 = raw_x0.clamp(0, cw);
        let y0 = raw_y0.clamp(0, ch);
        let x1 = raw_x1.clamp(0, cw);
        let y1 = raw_y1.clamp(0, ch);

        let vw = x1 - x0;
        let vh = y1 - y0;
        if vw < MIN_SIDE || vh < MIN_SIDE {
            return None;
        }

        if raw_y0 < 0 {
            flush = true;
            place = 0;
        } else if raw_x0 < 0 {
            flush = true;
            place = 1;
        } else if raw_x1 > cw {
            flush = true;
            place = 2;
        }

        Some(Self {
            ox: x0,
            oy: y0,
            w: vw,
            h: vh,
            diam: (max_radius * 2.0).round() as i32,
            flush,
            place,
            sharp_mask,
        })
    }

    /// Whole-window silhouette (used by the Preferences window).
    pub fn whole_window(w: i32, h: i32, radius: f32) -> Self {
        Self {
            ox: 0,
            oy: 0,
            w: w.max(1),
            h: h.max(1),
            diam: (radius * 2.0).round() as i32,
            flush: false,
            place: 0,
            sharp_mask: 0,
        }
    }
}

/// Converts a shell silhouette into a screen-space backdrop placement.
///
/// `inset` shrinks every side by that many pixels (and the corner radius with
/// it). GDI regions are not anti-aliased, so the backdrop is kept slightly
/// *smaller* than the painted shell; the shell's own edge stroke then hides the
/// aliased region edge.
///
/// Returns `None` when the shell is too small to carry a backdrop.
pub fn placement(win_x: i32, win_y: i32, g: &ShellGeom, inset: i32) -> Option<Placement> {
    let i = inset.max(0);
    let twice = i.saturating_mul(2);
    let w = g.w.saturating_sub(twice);
    let h = g.h.saturating_sub(twice);
    if w < MIN_SIDE || h < MIN_SIDE {
        return None;
    }
    let max_d = w.min(h).max(2);
    let diam = g.diam.saturating_sub(twice).clamp(2, max_d);
    Some(Placement {
        x: win_x.saturating_add(g.ox).saturating_add(i),
        y: win_y.saturating_add(g.oy).saturating_add(i),
        w,
        h,
        diam,
        flush: g.flush,
        place: g.place,
        sharp_mask: g.sharp_mask,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom() -> ShellGeom {
        ShellGeom::from_shell(40.0, 0.0, 200.0, 36.0, 18.0, false, 0, 0)
    }

    #[test]
    fn from_shell_rounds_edges_not_sizes() {
        let g = ShellGeom::from_shell(10.4, 5.6, 100.4, 30.4, 14.0, false, 0, 0);
        assert_eq!((g.ox, g.oy), (10, 6));
        assert_eq!((g.ox + g.w, g.oy + g.h), (111, 36));
        assert_eq!(g.diam, 28);
    }

    #[test]
    fn placement_offsets_into_screen_space() {
        let p = placement(1000, 20, &geom(), 1).unwrap();
        assert_eq!((p.x, p.y), (1000 + 40 + 1, 20 + 1));
        assert_eq!((p.w, p.h), (198, 34));
    }

    #[test]
    fn placement_shrinks_radius_with_inset() {
        let p = placement(0, 0, &geom(), 1).unwrap();
        assert_eq!(p.diam, 34); // 36 - 2, clamped to min(w,h) = 34
        let p3 = placement(0, 0, &geom(), 3).unwrap();
        assert_eq!(p3.diam, 30);
    }

    #[test]
    fn diameter_never_exceeds_shortest_side() {
        let g = ShellGeom::from_shell(0.0, 0.0, 120.0, 20.0, 40.0, false, 0, 0);
        let p = placement(0, 0, &g, 0).unwrap();
        assert!(p.diam <= p.h);
        assert!(p.diam >= 2);
    }

    #[test]
    fn tiny_shell_has_no_backdrop() {
        let g = ShellGeom::from_shell(0.0, 0.0, 9.0, 9.0, 4.0, false, 0, 0);
        assert!(placement(0, 0, &g, 1).is_none());
        assert!(placement(0, 0, &g, 0).is_some());
    }

    #[test]
    fn negative_inset_is_treated_as_zero() {
        let a = placement(5, 5, &geom(), -4).unwrap();
        let b = placement(5, 5, &geom(), 0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn flags_are_carried_through() {
        let g = ShellGeom::from_shell(0.0, 0.0, 300.0, 40.0, 16.0, true, 2, 0b0101);
        let p = placement(0, 0, &g, 1).unwrap();
        assert!(p.flush);
        assert_eq!(p.place, 2);
        assert_eq!(p.sharp_mask, 0b0101);
    }

    #[test]
    fn whole_window_geometry() {
        let g = ShellGeom::whole_window(1080, 720, 22.0);
        let p = placement(100, 50, &g, 1).unwrap();
        assert_eq!((p.x, p.y, p.w, p.h), (101, 51, 1078, 718));
        assert_eq!(p.diam, 42);
    }

    #[test]
    fn corner_docked_floating_preserves_sharp_mask() {
        let g = ShellGeom::from_shell(0.0, 0.0, 280.0, 44.0, 18.0, false, 0, 0b0011);
        let p = placement(100, 0, &g, 1).unwrap();
        assert!(!p.flush);
        assert_eq!(p.sharp_mask, 0b0011);
    }

    #[test]
    fn clipped_slide_in_clamps_to_canvas_and_marks_flush() {
        // Top slide-in partially above canvas (oy = -30, h = 80 -> visible 50px)
        let top = ShellGeom::from_shell_clipped(50.0, -30.0, 300.0, 80.0, 400, 160, 18.0, false, 0, 0)
            .expect("visible 50px height should produce geometry");
        assert_eq!((top.ox, top.oy, top.w, top.h), (50, 0, 300, 50));
        assert!(top.flush);
        assert_eq!(top.place, 0);

        // Right slide-in extending past canvas right edge (ox = 220, w = 240, cw = 400 -> visible 180px)
        let right =
            ShellGeom::from_shell_clipped(220.0, 20.0, 240.0, 100.0, 400, 160, 18.0, false, 2, 0)
                .expect("visible 180px width should produce geometry");
        assert_eq!((right.ox, right.oy, right.w, right.h), (220, 20, 180, 100));
        assert!(right.flush);
        assert_eq!(right.place, 2);

        // Left slide-in with tiny sliver (< MIN_SIDE = 8) returns None
        let sliver =
            ShellGeom::from_shell_clipped(-295.0, 10.0, 300.0, 80.0, 400, 160, 18.0, false, 1, 0);
        assert!(sliver.is_none());
    }

    #[test]
    fn extreme_inset_saturates_without_overflow() {
        let g = ShellGeom::from_shell(0.0, 0.0, 200.0, 40.0, 18.0, false, 0, 0);
        assert!(placement(i32::MAX, i32::MAX, &g, i32::MAX).is_none());
        assert!(placement(10, 20, &g, 100).is_none());
    }
}
