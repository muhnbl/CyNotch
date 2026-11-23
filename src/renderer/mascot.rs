use super::*;
use crate::painter_d2d::{BlendMode, PainterD2D, Path, PathBuilder, Transform, rgba, tok};
use std::sync::OnceLock;

/// Cached Direct2D paths for the Liquid Glass Spirit mascot (Option 4).
/// Normalized to local coordinates ~[-13..+13] so `scale = 1.0` fits a 32x32 icon slot cleanly.
struct SpiritPaths {
    body: Path,
    left_spec: Path,
    right_spec: Path,
    crown_ring: Path,
    halo_outer: Path,
    halo_glint: Path,
    eye_oval: Path,
    eye_happy: Path,
    eye_intense_l: Path,
    eye_intense_r: Path,
}

static SPIRIT_PATHS: OnceLock<SpiritPaths> = OnceLock::new();

fn build_ellipse_path(rx: f32, ry: f32) -> Path {
    // 4-cubic-bezier circle/ellipse approximation (kappa = 0.55228475)
    let kx = rx * 0.5522848;
    let ky = ry * 0.5522848;
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, -ry);
    pb.cubic_to(kx, -ry, rx, -ky, rx, 0.0);
    pb.cubic_to(rx, ky, kx, ry, 0.0, ry);
    pb.cubic_to(-kx, ry, -rx, ky, -rx, 0.0);
    pb.cubic_to(-rx, -ky, -kx, -ry, 0.0, -ry);
    pb.close();
    pb.finish().unwrap_or_default()
}

fn spirit_paths() -> &'static SpiritPaths {
    SPIRIT_PATHS.get_or_init(|| {
        // 1. Asymmetric smooth obsidian liquid-glass teardrop leaning gently up-right
        // Scaled by 0.21 from the [-58..+56, -58..+64] master design space
        let s = 0.205;
        let mut pb_body = PathBuilder::new();
        pb_body.move_to(34.0 * s, -52.0 * s);
        pb_body.cubic_to(44.0 * s, -46.0 * s, 36.0 * s, -24.0 * s, 44.0 * s, -6.0 * s);
        pb_body.cubic_to(56.0 * s, 22.0 * s, 42.0 * s, 62.0 * s, 0.0, 64.0 * s);
        pb_body.cubic_to(-44.0 * s, 64.0 * s, -58.0 * s, 24.0 * s, -44.0 * s, -10.0 * s);
        pb_body.cubic_to(-30.0 * s, -40.0 * s, 8.0 * s, -58.0 * s, 34.0 * s, -52.0 * s);
        pb_body.close();

        // 2. Left brushed silver glass specular curve
        let mut pb_lspec = PathBuilder::new();
        pb_lspec.move_to(-36.0 * s, -12.0 * s);
        pb_lspec.quad_to(-48.0 * s, 22.0 * s, -26.0 * s, 48.0 * s);

        // 3. Right subtle silver rim catch
        let mut pb_rspec = PathBuilder::new();
        pb_rspec.move_to(41.0 * s, -2.0 * s);
        pb_rspec.quad_to(48.0 * s, 28.0 * s, 28.0 * s, 49.0 * s);

        // 4. Halo reflection oval on the glass crown
        let crown_ring = build_ellipse_path(12.5 * s, 4.0 * s);

        // 5. Floating brushed Champagne Gold halo ring
        let halo_outer = build_ellipse_path(26.0 * s, 6.2 * s);

        // 6. Front specular highlight arc along the halo
        let mut pb_hglint = PathBuilder::new();
        pb_hglint.move_to(-21.0 * s, 1.8 * s);
        pb_hglint.quad_to(0.0, 6.8 * s, 21.0 * s, 1.8 * s);

        // 7. Expressive glowing Champagne Gold eyes
        let eye_oval = build_ellipse_path(5.8 * s, 10.2 * s);

        let mut pb_happy = PathBuilder::new();
        pb_happy.move_to(-6.8 * s, 2.2 * s);
        pb_happy.quad_to(0.0, -6.5 * s, 6.8 * s, 2.2 * s);

        let mut pb_int_l = PathBuilder::new();
        pb_int_l.move_to(-6.2 * s, -3.5 * s);
        pb_int_l.quad_to(0.5 * s, -1.5 * s, 6.2 * s, 2.0 * s);
        pb_int_l.quad_to(0.0, 6.8 * s, -6.2 * s, -3.5 * s);
        pb_int_l.close();

        let mut pb_int_r = PathBuilder::new();
        pb_int_r.move_to(6.2 * s, -3.5 * s);
        pb_int_r.quad_to(-0.5 * s, -1.5 * s, -6.2 * s, 2.0 * s);
        pb_int_r.quad_to(0.0, 6.8 * s, 6.2 * s, -3.5 * s);
        pb_int_r.close();

        SpiritPaths {
            body: pb_body.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            left_spec: pb_lspec.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            right_spec: pb_rspec.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            crown_ring,
            halo_outer,
            halo_glint: pb_hglint.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            eye_oval,
            eye_happy: pb_happy.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            eye_intense_l: pb_int_l.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
            eye_intense_r: pb_int_r.finish().unwrap_or_else(|| build_ellipse_path(1.0, 1.0)),
        }
    })
}

impl Renderer {
    /// Renders the animated **Liquid Glass Spirit** mascot (Option 4) using GPU Direct2D.
    /// Signature preserved as `draw_cyno_mascot` for zero-churn integration across all views.
    pub(crate) fn draw_cyno_mascot(
        p: &mut PainterD2D,
        cx: f32,
        cy: f32,
        scale: f32,
        mood: MascotMood,
        anim_time: f32,
        compact_fx: bool,
    ) {
        let k = scale;
        let t = anim_time.max(0.0);
        let sp = spirit_paths();

        // 1. Mood-specific organic kinematics (weightless liquid physics)
        let mut bob_y = (t * 2.1).sin() * 1.1;
        let mut tilt_deg = (t * 1.4).sin() * 1.8;
        let mut sx = 1.0 + (t * 2.1).sin() * 0.015;
        let mut sy = 1.0 - (t * 2.1).sin() * 0.015;
        let mut halo_y = -15.2 + (t * 2.1 - 0.5).sin() * 0.9;
        let mut halo_tilt = (t * 1.4 - 0.4).sin() * 2.8;
        let mut eye_scale_y = 1.0_f32;
        let mut happy_eyes = false;
        let mut right_wink = false;
        let mut intense_eyes = false;
        let mut ring_pulse = 0.0_f32;

        match mood {
            MascotMood::Idle => {
                let blink_cycle = t % 3.6;
                if blink_cycle > 3.42 && blink_cycle < 3.56 {
                    let ph = (blink_cycle - 3.42) / 0.14 * std::f32::consts::PI;
                    eye_scale_y = ph.cos().abs().max(0.10);
                }
            }
            MascotMood::Music => {
                let beat = (t * std::f32::consts::PI * 2.0).sin().abs();
                bob_y = -beat * 2.0 + 0.6;
                tilt_deg = (t * std::f32::consts::PI).sin() * 6.2;
                sx = 1.0 + beat * 0.045;
                sy = 1.0 - beat * 0.035;
                halo_y = -16.0 - beat * 1.2;
                halo_tilt = -tilt_deg * 1.25;
                happy_eyes = true;
                ring_pulse = beat;
            }
            MascotMood::Purging => {
                bob_y = -2.0 + (t * 24.0).sin() * 0.4;
                tilt_deg = (t * 18.0).sin() * 2.2;
                sx = 0.96 + (t * 12.0).sin() * 0.02;
                sy = 1.06 - (t * 12.0).sin() * 0.02;
                halo_y = -16.8 + (t * 10.0).sin() * 0.5;
                halo_tilt = (t * 8.0).sin() * 9.0;
                intense_eyes = true;
                ring_pulse = (t * 1.4) % 1.0;
            }
            MascotMood::Capture => {
                let cyc = t % 2.6;
                if cyc < 0.45 {
                    let ph = (cyc / 0.45 * std::f32::consts::PI).sin();
                    bob_y = -ph * 2.0;
                    halo_y = -15.2 - ph * 2.8;
                    right_wink = true;
                } else if cyc < 1.15 {
                    right_wink = true;
                }
            }
        }

        // Shift center slightly down so halo fits cleanly inside the slot
        let base_cy = cy + 2.4 * k;

        // 2. Ambient backdrop aura & mood energy ring
        if !compact_fx {
            p.radial_glow(
                cx,
                base_cy + (bob_y + 1.5) * k,
                16.5 * k,
                rgba(124, 58, 237, 68),
                BlendMode::Plus,
                true,
            );
            if ring_pulse > 0.02 {
                let ring_col = if mood == MascotMood::Purging {
                    rgba(16, 185, 129, ((1.0 - ring_pulse) * 145.0) as u8)
                } else {
                    rgba(235, 178, 42, ((1.0 - ring_pulse) * 115.0) as u8)
                };
                p.circle_stroke(
                    cx,
                    base_cy + (bob_y + 1.0) * k,
                    (11.5 + ring_pulse * 7.5) * k,
                    (1.1 * k).max(0.8),
                    ring_col,
                );
            }
        }

        // 3. Floating Brushed Champagne Gold Halo
        let halo_cx = cx;
        let halo_cy = base_cy + halo_y * k;
        let halo_tf = Transform::from_translate(halo_cx, halo_cy)
            .pre_rotate(halo_tilt)
            .pre_scale(k, k);

        // Soft warm Champagne Gold halo glow
        p.radial_glow(
            halo_cx,
            halo_cy,
            (8.5 * k).max(4.0),
            rgba(235, 178, 42, if mood == MascotMood::Purging { 120 } else { 78 }),
            BlendMode::Plus,
            true,
        );
        // Deep bronze-gold under-stroke + bright champagne body + incandescent shimmer arc
        p.stroke_path_tf(&sp.halo_outer, (1.9 * k).max(1.2), tok::gold_deep(), halo_tf);
        p.stroke_path_tf(&sp.halo_outer, (1.35 * k).max(0.95), tok::gold_champagne(), halo_tf);
        p.stroke_path_tf(
            &sp.halo_glint,
            (0.65 * k).max(0.55),
            rgba(255, 250, 220, 225),
            halo_tf,
        );

        // 4. Liquid Obsidian Glass Teardrop Body
        let body_cy = base_cy + bob_y * k;
        let body_tf = Transform::from_translate(cx, body_cy)
            .pre_rotate(tilt_deg)
            .pre_scale(k * sx, k * sy);

        // Smoked Obsidian-Purple body gradient
        p.fill_linear_path_tf(
            &sp.body,
            -9.0,
            -11.0,
            9.0,
            13.0,
            rgba(54, 28, 118, 228),
            rgba(20, 14, 42, 244),
            Some(rgba(30, 18, 66, 238)),
            body_tf,
        );

        // Internal luminous violet refractive belly glow
        p.radial_glow(
            cx + 1.5 * k,
            body_cy + 5.8 * k,
            9.2 * k,
            rgba(139, 92, 246, 138),
            BlendMode::Plus,
            true,
        );

        // Crown golden halo reflection oval on glass top
        let crown_tf = body_tf
            .pre_translate(1.6, -8.5)
            .pre_rotate(-12.0);
        p.stroke_path_tf(
            &sp.crown_ring,
            (0.65 * k).max(0.45),
            rgba(253, 224, 96, 82),
            crown_tf,
        );

        // Crisp Putih Silver left & right specular glass reflections
        p.stroke_path_tf(
            &sp.left_spec,
            (1.15 * k).max(0.75),
            rgba(248, 247, 255, 185),
            body_tf,
        );
        p.stroke_path_tf(
            &sp.right_spec,
            (0.85 * k).max(0.6),
            rgba(224, 217, 252, 125),
            body_tf,
        );

        // Outer silver-glass rim stroke
        p.stroke_path_tf(
            &sp.body,
            (0.78 * k).max(0.55),
            rgba(248, 247, 255, 165),
            body_tf,
        );

        // 5. Expressive Glowing Rich Metallic Champagne Gold Eyes
        let eye_offsets = [(-3.4_f32, 0.4_f32, false), (2.3_f32, 0.4_f32, true)];
        for (ex, ey, is_right) in eye_offsets {
            let eye_tf = body_tf.pre_translate(ex, ey);
            let world_ex = cx + ex * k * sx;
            let world_ey = body_cy + ey * k * sy;

            // Warm Champagne Gold eye bloom
            p.radial_glow(
                world_ex,
                world_ey,
                (4.6 * k).max(2.6),
                rgba(
                    253,
                    224,
                    96,
                    if intense_eyes { 170 } else { 115 },
                ),
                BlendMode::Plus,
                true,
            );

            if happy_eyes || (right_wink && is_right) {
                p.stroke_path_tf(
                    &sp.eye_happy,
                    (1.35 * k).max(0.95),
                    tok::gold_champagne(),
                    eye_tf,
                );
            } else if intense_eyes {
                let path = if is_right {
                    &sp.eye_intense_r
                } else {
                    &sp.eye_intense_l
                };
                p.fill_linear_path_tf(
                    path,
                    0.0,
                    -2.0,
                    0.0,
                    2.0,
                    tok::gold_shimmer(),
                    tok::gold_bronze(),
                    Some(tok::gold_champagne()),
                    eye_tf,
                );
            } else {
                let blink_tf = eye_tf.pre_rotate(4.0).pre_scale(1.0, eye_scale_y);
                p.fill_linear_path_tf(
                    &sp.eye_oval,
                    0.0,
                    -2.2,
                    0.0,
                    2.2,
                    tok::gold_shimmer(),
                    tok::gold_bronze(),
                    Some(tok::gold_champagne()),
                    blink_tf,
                );
            }
        }
    }

    /// Renders a pair of glowing **Amethyst Purple Wolf Eyes** in the dark idle Dynamic Island capsule.
    /// - `open_t` (`0.0..=1.0`): controls smooth eyelid opening/closing and glow fade during state transitions.
    /// - `pulse_i` (`0.0..=1.0`): Opsi A micro-reaction on Copy/Paste & Snipping Tool (predator squint + violet flash).
    /// - `gaze_dir` (`-1.0..=1.0`): directional gaze (`0.0` Top center).
    /// - `rot_deg`: `0.0` (Top), `-90.0` (Left edge 270deg), `+90.0` (Right edge 90deg).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_wolf_eyes(
        p: &mut PainterD2D,
        cx: f32,
        cy: f32,
        scale: f32,
        anim_time: f32,
        open_t: f32,
        pulse_i: f32,
        gaze_dir: f32,
        rot_deg: f32,
    ) {
        let open = open_t.clamp(0.0, 1.0);
        if open <= 0.01 {
            return;
        }
        let wp = wolf_eye_paths();
        let t = anim_time.max(0.0);
        let pulse = pulse_i.clamp(0.0, 1.0);
        let rad = rot_deg.to_radians();
        let (sin_r, cos_r) = rad.sin_cos();
        let rot_pt = |lx: f32, ly: f32| -> (f32, f32) {
            (cx + lx * cos_r - ly * sin_r, cy + lx * sin_r + ly * cos_r)
        };

        // Organic breathing luminescence & periodic slow predator blink (every 5.4s)
        let breathe = 0.88 + 0.12 * (t * 2.1).sin();
        let blink_cycle = t % 5.4;
        let nat_blink = if blink_cycle < 0.17 {
            ((blink_cycle / 0.17) * std::f32::consts::PI).sin().powi(2)
        } else {
            0.0
        };

        // Eyelid vertical opening: closes into a thin glowing slit when open_t -> 0 or blinking,
        // and narrows into a fierce predator squint during Opsi A copy/paste & snipping reaction.
        let lid_sy = ((0.08 + 0.92 * open) * (1.0 - 0.86 * nat_blink) * (1.0 - 0.42 * pulse))
            .clamp(0.06, 1.05);
        let lid_sx = 1.0 + 0.08 * pulse;
        let alpha = (open * (0.85 + 0.15 * breathe)).clamp(0.0, 1.0);

        // Directional gaze offset
        let drift_x = (t * 0.85).sin() * 0.25;
        let gaze_shift_x = gaze_dir * 1.35 + drift_x;

        let sep_x = 8.4 * scale;
        let eyes = [
            (-sep_x, &wp.eye_l, -1.0_f32),
            (sep_x, &wp.eye_r, 1.0_f32),
        ];

        // Subtle central bridge ambient violet mist
        let (mx, my) = rot_pt(gaze_shift_x * scale * 0.5, 0.0);
        let (rx, ry) = if rot_deg.abs() > 45.0 {
            (8.5 * scale, 19.0 * scale)
        } else {
            (19.0 * scale, 8.5 * scale)
        };
        p.elliptical_glow(
            mx,
            my,
            rx,
            ry,
            rgba(124, 58, 237, ((48.0 + 42.0 * pulse) * alpha) as u8),
            BlendMode::Plus,
            true,
        );

        for (dx, eye_path, side_sign) in eyes {
            let (ex, ey) = rot_pt(dx + gaze_shift_x * scale * 0.45, 0.0);

            // 1. Outer Amethyst Violet eye bloom
            let glow_r = (7.2 + 2.6 * pulse) * scale;
            p.radial_glow(
                ex,
                ey,
                glow_r,
                rgba(147, 51, 234, ((115.0 + 75.0 * pulse) * alpha) as u8),
                BlendMode::Plus,
                true,
            );

            // 2. Angular Wolf Eye body (Electric Amethyst #C084FC -> Deep Royal Violet #7C3AED)
            let tilt_deg = rot_deg + side_sign * (-3.5 - 4.0 * pulse);
            let eye_tf = Transform::from_translate(ex, ey)
                .pre_rotate(tilt_deg)
                .pre_scale(scale * lid_sx, scale * lid_sy);

            let top_col = rgba(
                (216.0 + 34.0 * pulse).min(255.0) as u8,
                (180.0 + 55.0 * pulse).min(255.0) as u8,
                254,
                (245.0 * alpha) as u8,
            );
            let mid_col = rgba(
                (168.0 + 35.0 * pulse).min(255.0) as u8,
                (85.0 + 45.0 * pulse).min(255.0) as u8,
                247,
                (238.0 * alpha) as u8,
            );
            let bot_col = rgba(124, 58, 237, (220.0 * alpha) as u8);

            p.fill_linear_path_tf(
                eye_path,
                0.0,
                -3.2,
                0.0,
                2.8,
                top_col,
                bot_col,
                Some(mid_col),
                eye_tf,
            );

            // 3. Crisp electric lavender rim catch along the upper wolf eyelid
            p.stroke_path_tf(
                eye_path,
                (0.55 * scale).max(0.45),
                rgba(243, 232, 255, ((175.0 + 70.0 * pulse) * alpha) as u8),
                eye_tf,
            );

            // 4. Inner Lavender-White glowing predator iris core
            let (px, py) = rot_pt(dx + gaze_shift_x * scale * 0.65, 0.1 * scale);
            let pupil_tf = Transform::from_translate(px, py)
                .pre_rotate(rot_deg)
                .pre_scale(scale * (0.95 + 0.18 * pulse), scale * lid_sy);
            p.fill_path_tf(
                &wp.pupil_core,
                rgba(245, 238, 255, ((225.0 + 30.0 * pulse) * alpha) as u8),
                pupil_tf,
            );
        }
    }
}

struct WolfEyePaths {
    eye_l: Path,
    eye_r: Path,
    pupil_core: Path,
}

static WOLF_EYE_PATHS: OnceLock<WolfEyePaths> = OnceLock::new();

fn wolf_eye_paths() -> &'static WolfEyePaths {
    WOLF_EYE_PATHS.get_or_init(|| {
        // Left predator wolf eye: inner tear-duct dips sharply right-down (+5.4, +1.45),
        // upper brow sweeps to an angular peak and tapers to a sharp outer wing (-6.2, -1.95).
        let mut pb_l = PathBuilder::new();
        pb_l.move_to(5.4, 1.45);
        pb_l.cubic_to(2.8, -2.5, -1.8, -3.3, -6.2, -1.95);
        pb_l.cubic_to(-3.4, 2.0, 1.6, 2.8, 5.4, 1.45);
        pb_l.close();

        // Right predator wolf eye (mirror)
        let mut pb_r = PathBuilder::new();
        pb_r.move_to(-5.4, 1.45);
        pb_r.cubic_to(-2.8, -2.5, 1.8, -3.3, 6.2, -1.95);
        pb_r.cubic_to(3.4, 2.0, -1.6, 2.8, -5.4, 1.45);
        pb_r.close();

        // Sharp diamond-oval inner glowing core
        let pupil_core = build_ellipse_path(1.45, 2.05);

        WolfEyePaths {
            eye_l: pb_l.finish().unwrap(),
            eye_r: pb_r.finish().unwrap(),
            pupil_core,
        }
    })
}
