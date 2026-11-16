//! Stroke icons drawn as vector paths on a 24×24 grid (Lucide-style geometry).
//! Segoe UI has no emoji/symbol glyphs, so text icons rendered as tofu boxes.

use crate::painter::{Painter, rrect_path};
use tiny_skia::{Color, PathBuilder};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Home,
    Music,
    Volume,
    Wave,
    Timer,
    List,
    Clipboard,
    Video,
    Display,
    Settings,
    Grip,
    Droplet,
    Zap,
    Check,
    Prev,
    Next,
    Play,
    Pause,
    Reset,
    Lock,
    Activity,
    Close,
    File,
    Folder,
    Plus,
    Bell,
    AppDiscord,
    AppChat,
    AppMail,
    AppBrowser,
    AppCode,
    AppWindows,
}

/// Resolve an app-specific icon from a notification's `app_name` (case-insensitive).
pub fn for_app(app_name: &str, fallback: Icon) -> Icon {
    let lower = app_name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return fallback;
    }
    if lower.contains("discord") {
        Icon::AppDiscord
    } else if lower.contains("spotify")
        || lower.contains("music")
        || lower.contains("apple music")
        || lower.contains("tidal")
        || lower.contains("foobar")
    {
        Icon::Music
    } else if lower.contains("telegram")
        || lower.contains("whatsapp")
        || lower.contains("slack")
        || lower.contains("teams")
        || lower.contains("signal")
        || lower.contains("line")
        || lower.contains("chat")
        || lower.contains("messenger")
    {
        Icon::AppChat
    } else if lower.contains("mail") || lower.contains("outlook") || lower.contains("thunderbird") {
        Icon::AppMail
    } else if lower.contains("chrome")
        || lower.contains("edge")
        || lower.contains("firefox")
        || lower.contains("brave")
        || lower.contains("opera")
        || lower.contains("arc")
        || lower.contains("zen")
        || lower.contains("vivaldi")
    {
        Icon::AppBrowser
    } else if lower.contains("code")
        || lower.contains("studio")
        || lower.contains("terminal")
        || lower.contains("powershell")
        || lower.contains("cursor")
        || lower.contains("windsurf")
        || lower.contains("git")
    {
        Icon::AppCode
    } else if lower.contains("windows") || lower.contains("explorer") || lower.contains("security")
    {
        Icon::AppWindows
    } else if lower.contains("cynotch") {
        Icon::Zap
    } else {
        fallback
    }
}

struct G {
    x: f32,
    y: f32,
    k: f32,
}

impl G {
    fn p(&self, u: f32, v: f32) -> (f32, f32) {
        (self.x + u * self.k, self.y + v * self.k)
    }
}

fn poly(g: &G, pts: &[(f32, f32)], close: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (i, &(u, v)) in pts.iter().enumerate() {
        let (x, y) = g.p(u, v);
        if i == 0 {
            pb.move_to(x, y)
        } else {
            pb.line_to(x, y)
        }
    }
    if close {
        pb.close();
    }
    pb.finish()
}

/// Draw `icon` with its 24-grid box at (x, y) scaled to `size`.
pub fn draw(p: &mut Painter, icon: Icon, x: f32, y: f32, size: f32, c: Color) {
    let g = G {
        x,
        y,
        k: size / 24.0,
    };
    let sw = 1.9 * g.k; // stroke width in logical units
    match icon {
        Icon::Home => {
            if let Some(roof) = poly(&g, &[(3.0, 10.0), (12.0, 3.0), (21.0, 10.0)], false) {
                p.stroke_path(&roof, sw, c);
            }
            if let Some(body) = poly(
                &g,
                &[(5.0, 9.5), (5.0, 20.0), (19.0, 20.0), (19.0, 9.5)],
                false,
            ) {
                p.stroke_path(&body, sw, c);
            }
            if let Some(door) = poly(
                &g,
                &[(9.5, 20.0), (9.5, 13.5), (14.5, 13.5), (14.5, 20.0)],
                false,
            ) {
                p.stroke_path(&door, sw, c);
            }
        }
        Icon::Music => {
            if let Some(path) = poly(
                &g,
                &[(9.0, 18.0), (9.0, 5.0), (20.0, 3.0), (20.0, 16.0)],
                false,
            ) {
                p.stroke_path(&path, sw, c);
            }
            for (cu, cv) in [(6.5, 18.0), (17.5, 16.0)] {
                let (cx, cy) = g.p(cu, cv);
                if let Some(circ) = PathBuilder::from_circle(cx, cy, 2.8 * g.k) {
                    p.stroke_path(&circ, sw, c);
                }
            }
        }
        Icon::Volume => {
            if let Some(body) = poly(
                &g,
                &[
                    (11.0, 5.0),
                    (6.0, 9.0),
                    (3.0, 9.0),
                    (3.0, 15.0),
                    (6.0, 15.0),
                    (11.0, 19.0),
                ],
                true,
            ) {
                p.stroke_path(&body, sw, c);
            }
            let (cx, cy) = g.p(11.0, 12.0);
            p.arc(cx, cy, 4.5 * g.k, -0.75, 1.5, sw, c);
            p.arc(cx, cy, 8.0 * g.k, -0.85, 1.7, sw, c);
        }
        Icon::Wave => {
            let mut pb = PathBuilder::new();
            let (x0, y0) = g.p(2.0, 12.0);
            let (c1x, c1y) = g.p(5.0, 6.0);
            let (c2x, c2y) = g.p(8.0, 18.0);
            let (x1, y1) = g.p(12.0, 12.0);
            let (c3x, c3y) = g.p(16.0, 6.0);
            let (c4x, c4y) = g.p(19.0, 18.0);
            let (x2, y2) = g.p(22.0, 12.0);
            pb.move_to(x0, y0);
            pb.cubic_to(c1x, c1y, c2x, c2y, x1, y1);
            pb.cubic_to(c3x, c3y, c4x, c4y, x2, y2);
            if let Some(path) = pb.finish() {
                p.stroke_path(&path, sw, c);
            }
        }
        Icon::Timer => {
            let (cx, cy) = g.p(12.0, 13.5);
            if let Some(circ) = PathBuilder::from_circle(cx, cy, 7.5 * g.k) {
                p.stroke_path(&circ, sw, c);
            }
            for seg in [[(10.0, 2.5), (14.0, 2.5)], [(12.0, 13.5), (15.5, 10.0)]] {
                if let Some(path) = poly(&g, &seg, false) {
                    p.stroke_path(&path, sw, c);
                }
            }
        }
        Icon::List => {
            for (v, len) in [(7.0, 15.0), (12.0, 15.0), (17.0, 11.0)] {
                if let Some(line) = poly(&g, &[(8.0, v), (8.0 + len, v)], false) {
                    p.stroke_path(&line, sw, c);
                }
                let (cx, cy) = g.p(4.0, v);
                if let Some(dot) = PathBuilder::from_circle(cx, cy, 1.2 * g.k) {
                    p.fill_path(&dot, c);
                }
            }
        }
        Icon::Clipboard => {
            let (bx, by) = g.p(4.5, 4.0);
            if let Some(body) = rrect_path(bx, by, 15.0 * g.k, 18.0 * g.k, 2.5 * g.k) {
                p.stroke_path(&body, sw, c);
            }
            let (tx, ty) = g.p(8.5, 2.0);
            if let Some(tab) = rrect_path(tx, ty, 7.0 * g.k, 4.0 * g.k, 1.2 * g.k) {
                p.stroke_path(&tab, sw, c);
            }
        }
        Icon::Video => {
            // Viewfinder / Camera Capture Frame + shutter iris
            let (bx, by) = g.p(2.5, 5.5);
            if let Some(body) = rrect_path(bx, by, 19.0 * g.k, 14.0 * g.k, 2.8 * g.k) {
                p.stroke_path(&body, sw, c);
            }
            if let Some(bump) = poly(
                &g,
                &[(8.0, 5.5), (9.5, 3.0), (14.5, 3.0), (16.0, 5.5)],
                false,
            ) {
                p.stroke_path(&bump, sw, c);
            }
            let (cx, cy) = g.p(12.0, 12.5);
            if let Some(lens) = PathBuilder::from_circle(cx, cy, 3.6 * g.k) {
                p.stroke_path(&lens, sw, c);
            }
        }
        Icon::Display => {
            let (bx, by) = g.p(2.5, 3.5);
            if let Some(screen) = rrect_path(bx, by, 19.0 * g.k, 13.0 * g.k, 2.2 * g.k) {
                p.stroke_path(&screen, sw, c);
            }
            if let Some(stand) = poly(&g, &[(12.0, 16.5), (12.0, 20.5)], false) {
                p.stroke_path(&stand, sw, c);
            }
            if let Some(base) = poly(&g, &[(7.5, 20.5), (16.5, 20.5)], false) {
                p.stroke_path(&base, sw, c);
            }
        }
        Icon::Settings => {
            let (cx, cy) = g.p(12.0, 12.0);
            let r_outer = 6.8 * g.k;
            let r_hole = 2.8 * g.k;
            if let Some(rim) = PathBuilder::from_circle(cx, cy, r_outer) {
                p.stroke_path(&rim, sw * 0.9, c);
            }
            if let Some(hole) = PathBuilder::from_circle(cx, cy, r_hole) {
                p.stroke_path(&hole, sw * 0.9, c);
            }
            for i in 0..6 {
                let ang = (i as f32) * (std::f32::consts::TAU / 6.0);
                let (cos_a, sin_a) = (ang.cos(), ang.sin());
                let (nx, ny) = (-sin_a, cos_a);
                let base_r = 6.2 * g.k;
                let tip_r = 9.2 * g.k;
                let half_w = 1.6 * g.k;

                let mut pb = PathBuilder::new();
                pb.move_to(
                    cx + base_r * cos_a - half_w * nx,
                    cy + base_r * sin_a - half_w * ny,
                );
                pb.line_to(
                    cx + tip_r * cos_a - half_w * 0.7 * nx,
                    cy + tip_r * sin_a - half_w * 0.7 * ny,
                );
                pb.line_to(
                    cx + tip_r * cos_a + half_w * 0.7 * nx,
                    cy + tip_r * sin_a + half_w * 0.7 * ny,
                );
                pb.line_to(
                    cx + base_r * cos_a + half_w * nx,
                    cy + base_r * sin_a + half_w * ny,
                );
                pb.close();
                if let Some(tooth) = pb.finish() {
                    p.fill_path(&tooth, c);
                }
            }
        }
        Icon::Grip => {
            for col in [8.5, 15.5] {
                for row in [6.0, 12.0, 18.0] {
                    let (cx, cy) = g.p(col, row);
                    if let Some(dot) = PathBuilder::from_circle(cx, cy, 1.4 * g.k) {
                        p.fill_path(&dot, c);
                    }
                }
            }
        }
        Icon::Droplet => {
            let mut pb = PathBuilder::new();
            let (top_x, top_y) = g.p(12.0, 3.5);
            let (r_cp1_x, r_cp1_y) = g.p(17.0, 9.5);
            let (r_cp2_x, r_cp2_y) = g.p(19.5, 14.0);
            let (r_mid_x, r_mid_y) = g.p(19.5, 16.5);
            let (b_cp1_x, b_cp1_y) = g.p(19.5, 21.0);
            let (b_cp2_x, b_cp2_y) = g.p(4.5, 21.0);
            let (l_mid_x, l_mid_y) = g.p(4.5, 16.5);
            let (l_cp1_x, l_cp1_y) = g.p(4.5, 14.0);
            let (l_cp2_x, l_cp2_y) = g.p(7.0, 9.5);
            pb.move_to(top_x, top_y);
            pb.cubic_to(r_cp1_x, r_cp1_y, r_cp2_x, r_cp2_y, r_mid_x, r_mid_y);
            pb.cubic_to(b_cp1_x, b_cp1_y, b_cp2_x, b_cp2_y, l_mid_x, l_mid_y);
            pb.cubic_to(l_cp1_x, l_cp1_y, l_cp2_x, l_cp2_y, top_x, top_y);
            pb.close();
            if let Some(path) = pb.finish() {
                p.stroke_path(&path, sw, c);
            }
        }
        Icon::Zap => {
            if let Some(path) = poly(
                &g,
                &[
                    (13.0, 2.0),
                    (4.0, 14.0),
                    (12.0, 14.0),
                    (11.0, 22.0),
                    (20.0, 10.0),
                    (12.0, 10.0),
                ],
                true,
            ) {
                p.stroke_path(&path, sw, c);
            }
        }
        Icon::Check => {
            if let Some(path) = poly(&g, &[(20.0, 6.0), (9.0, 17.0), (4.0, 12.0)], false) {
                p.stroke_path(&path, sw * 1.25, c);
            }
        }
        Icon::Play => {
            if let Some(path) = poly(&g, &[(7.0, 4.5), (19.5, 12.0), (7.0, 19.5)], true) {
                p.fill_path(&path, c);
                p.stroke_path(&path, 1.4 * g.k, c);
            }
        }
        Icon::Pause => {
            for u in [6.5, 14.5] {
                let (px, py) = g.p(u, 4.5);
                p.rrect(px, py, 3.2 * g.k, 15.0 * g.k, 1.2 * g.k, c);
            }
        }
        Icon::Prev | Icon::Next => {
            let flip = icon == Icon::Next;
            let m = |u: f32| if flip { 24.0 - u } else { u };
            if let Some(tri) = poly(&g, &[(m(19.0), 5.5), (m(9.5), 12.0), (m(19.0), 18.5)], true) {
                p.fill_path(&tri, c);
                p.stroke_path(&tri, 1.4 * g.k, c);
            }
            if let Some(bar) = poly(&g, &[(m(6.0), 5.5), (m(6.0), 18.5)], false) {
                p.stroke_path(&bar, 2.2 * g.k, c);
            }
        }
        Icon::Reset => {
            let (cx, cy) = g.p(12.0, 12.0);
            p.arc(cx, cy, 7.5 * g.k, -2.4, 4.9, sw, c);
            if let Some(head) = poly(&g, &[(3.5, 4.5), (4.2, 9.2), (8.9, 8.4)], false) {
                p.stroke_path(&head, sw, c);
            }
        }
        Icon::Lock => {
            let (bx, by) = g.p(5.0, 11.0);
            if let Some(body) = rrect_path(bx, by, 14.0 * g.k, 9.5 * g.k, 2.2 * g.k) {
                p.stroke_path(&body, sw, c);
            }
            let (cx, cy) = g.p(12.0, 10.5);
            p.arc(
                cx,
                cy - 1.5 * g.k,
                4.0 * g.k,
                std::f32::consts::PI,
                std::f32::consts::PI,
                sw,
                c,
            );
            for u in [8.0, 16.0] {
                if let Some(leg) = poly(&g, &[(u, 9.0), (u, 11.0)], false) {
                    p.stroke_path(&leg, sw, c);
                }
            }
        }
        Icon::Activity => {
            if let Some(path) = poly(
                &g,
                &[
                    (22.0, 12.0),
                    (18.0, 12.0),
                    (15.0, 21.0),
                    (9.0, 3.0),
                    (6.0, 12.0),
                    (2.0, 12.0),
                ],
                false,
            ) {
                p.stroke_path(&path, sw, c);
            }
        }
        Icon::Close => {
            for seg in [[(6.5, 6.5), (17.5, 17.5)], [(17.5, 6.5), (6.5, 17.5)]] {
                if let Some(path) = poly(&g, &seg, false) {
                    p.stroke_path(&path, sw * 1.1, c);
                }
            }
        }
        Icon::File => {
            if let Some(page) = poly(
                &g,
                &[
                    (6.0, 3.0),
                    (14.5, 3.0),
                    (18.5, 7.0),
                    (18.5, 21.0),
                    (6.0, 21.0),
                ],
                true,
            ) {
                p.stroke_path(&page, sw, c);
            }
            if let Some(fold) = poly(&g, &[(14.5, 3.0), (14.5, 7.0), (18.5, 7.0)], false) {
                p.stroke_path(&fold, sw * 0.85, c);
            }
        }
        Icon::Folder => {
            if let Some(body) = poly(
                &g,
                &[
                    (3.0, 6.0),
                    (9.5, 6.0),
                    (11.5, 8.5),
                    (21.0, 8.5),
                    (21.0, 19.5),
                    (3.0, 19.5),
                ],
                true,
            ) {
                p.stroke_path(&body, sw, c);
            }
        }
        Icon::Plus => {
            for seg in [[(12.0, 5.5), (12.0, 18.5)], [(5.5, 12.0), (18.5, 12.0)]] {
                if let Some(path) = poly(&g, &seg, false) {
                    p.stroke_path(&path, sw * 1.1, c);
                }
            }
        }
        Icon::Bell => {
            let mut pb = PathBuilder::new();
            let (x0, y0) = g.p(6.0, 16.5);
            let (x1, y1) = g.p(6.0, 10.5);
            let (c1x, c1y) = g.p(6.0, 6.2);
            let (c2x, c2y) = g.p(8.8, 4.0);
            let (tx, ty) = g.p(12.0, 4.0);
            let (c3x, c3y) = g.p(15.2, 4.0);
            let (c4x, c4y) = g.p(18.0, 6.2);
            let (x2, y2) = g.p(18.0, 10.5);
            let (x3, y3) = g.p(18.0, 16.5);
            pb.move_to(x0, y0);
            pb.line_to(x1, y1);
            pb.cubic_to(c1x, c1y, c2x, c2y, tx, ty);
            pb.cubic_to(c3x, c3y, c4x, c4y, x2, y2);
            pb.line_to(x3, y3);
            if let Some(dome) = pb.finish() {
                p.stroke_path(&dome, sw, c);
            }
            if let Some(brim) = poly(&g, &[(4.0, 16.5), (20.0, 16.5)], false) {
                p.stroke_path(&brim, sw, c);
            }
            let (cx, cy) = g.p(12.0, 19.5);
            p.arc(cx, cy, 2.1 * g.k, 0.0, std::f32::consts::PI, sw, c);
        }
        Icon::AppDiscord => {
            // Discord Clyde silhouette (outer contour + two expressive eyes)
            let mut pb = PathBuilder::new();
            let (x0, y0) = g.p(5.2, 6.2);
            pb.move_to(x0, y0);
            let (x1, y1) = g.p(9.2, 5.0);
            pb.line_to(x1, y1);
            let (x2, y2) = g.p(9.8, 6.3);
            pb.line_to(x2, y2);
            let (x3, y3) = g.p(14.2, 6.3);
            pb.line_to(x3, y3);
            let (x4, y4) = g.p(14.8, 5.0);
            pb.line_to(x4, y4);
            let (x5, y5) = g.p(18.8, 6.2);
            pb.line_to(x5, y5);
            let (c1x, c1y) = g.p(20.8, 9.6);
            let (c2x, c2y) = g.p(21.2, 13.8);
            let (x6, y6) = g.p(19.6, 17.8);
            pb.cubic_to(c1x, c1y, c2x, c2y, x6, y6);
            let (x7, y7) = g.p(16.2, 19.2);
            pb.line_to(x7, y7);
            let (x8, y8) = g.p(15.0, 17.4);
            pb.line_to(x8, y8);
            let (x9, y9) = g.p(9.0, 17.4);
            pb.line_to(x9, y9);
            let (x10, y10) = g.p(7.8, 19.2);
            pb.line_to(x10, y10);
            let (x11, y11) = g.p(4.4, 17.8);
            pb.line_to(x11, y11);
            let (c3x, c3y) = g.p(2.8, 13.8);
            let (c4x, c4y) = g.p(3.2, 9.6);
            pb.cubic_to(c3x, c3y, c4x, c4y, x0, y0);
            pb.close();
            if let Some(shell) = pb.finish() {
                p.stroke_path(&shell, sw * 0.95, c);
            }
            for (ex, ey) in [(8.8, 12.2), (15.2, 12.2)] {
                let (cx, cy) = g.p(ex, ey);
                p.circle(cx, cy, 1.85 * g.k, c);
            }
        }
        Icon::AppChat => {
            if let Some(bubble) = rrect_path(
                g.x + 3.0 * g.k,
                g.y + 4.0 * g.k,
                18.0 * g.k,
                13.5 * g.k,
                4.5 * g.k,
            ) {
                p.stroke_path(&bubble, sw, c);
            }
            if let Some(tail) = poly(&g, &[(7.5, 17.5), (5.5, 20.5), (11.0, 17.5)], false) {
                p.stroke_path(&tail, sw, c);
            }
        }
        Icon::AppMail => {
            if let Some(env) = rrect_path(
                g.x + 3.0 * g.k,
                g.y + 5.0 * g.k,
                18.0 * g.k,
                14.0 * g.k,
                2.5 * g.k,
            ) {
                p.stroke_path(&env, sw, c);
            }
            if let Some(flap) = poly(&g, &[(3.5, 6.2), (12.0, 13.0), (20.5, 6.2)], false) {
                p.stroke_path(&flap, sw, c);
            }
        }
        Icon::AppBrowser => {
            let (cx, cy) = g.p(12.0, 12.0);
            p.circle_stroke(cx, cy, 9.0 * g.k, sw, c);
            if let Some(eq) = poly(&g, &[(3.2, 12.0), (20.8, 12.0)], false) {
                p.stroke_path(&eq, sw * 0.85, c);
            }
            p.arc(cx - 3.5 * g.k, cy, 9.6 * g.k, -1.05, 2.1, sw * 0.85, c);
            p.arc(cx + 3.5 * g.k, cy, 9.6 * g.k, 2.09, 2.1, sw * 0.85, c);
        }
        Icon::AppCode => {
            if let Some(left) = poly(&g, &[(9.0, 7.0), (3.5, 12.0), (9.0, 17.0)], false) {
                p.stroke_path(&left, sw, c);
            }
            if let Some(right) = poly(&g, &[(15.0, 7.0), (20.5, 12.0), (15.0, 17.0)], false) {
                p.stroke_path(&right, sw, c);
            }
            if let Some(slash) = poly(&g, &[(13.5, 5.5), (10.5, 18.5)], false) {
                p.stroke_path(&slash, sw * 0.9, c);
            }
        }
        Icon::AppWindows => {
            let s = 7.4 * g.k;
            for (u, v) in [(3.5, 3.5), (13.1, 3.5), (3.5, 13.1), (13.1, 13.1)] {
                let (px, py) = g.p(u, v);
                if let Some(pane) = rrect_path(px, py, s, s, 1.4 * g.k) {
                    p.stroke_path(&pane, sw * 0.88, c);
                }
            }
        }
    }
}
