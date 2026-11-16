use super::*;
use std::sync::OnceLock;

struct MascotSidePaths {
    wing: Path,
    wing_rim: Path,
    gold_lap: Path,
    lap_hi: Path,
    ear: Path,
    ear_rim: Path,
    gold_ear: Path,
    chk: Path,
    sock: Path,
    eye: Path,
    core: Path,
}

struct MascotPaths {
    sides: [MascotSidePaths; 2],
    head: Path,
    brow: Path,
    brow_rim: Path,
    bridge: Path,
    ridge: Path,
    nose: Path,
    nose_hi: Path,
    chin_rim: Path,
    diamond_facets: [(Path, Color); 4],
    gem_rim: Path,
}

static MASCOT_PATHS: OnceLock<MascotPaths> = OnceLock::new();

fn build_side_paths(sign: f32) -> MascotSidePaths {
    let mut pb_wing = PathBuilder::new();
    pb_wing.move_to(sign * 6.0, -3.5);
    pb_wing.line_to(sign * 13.3, 11.4);
    pb_wing.line_to(sign * 10.4, 13.7);
    pb_wing.line_to(sign * 7.4, 12.2);
    pb_wing.line_to(sign * 5.2, 1.5);
    pb_wing.close();

    let mut pb_wing_rim = PathBuilder::new();
    pb_wing_rim.move_to(sign * 6.4, -2.5);
    pb_wing_rim.line_to(sign * 13.0, 11.2);

    let mut pb_gold_lap = PathBuilder::new();
    pb_gold_lap.move_to(sign * 5.2, 1.7);
    pb_gold_lap.line_to(sign * 8.4, 2.2);
    pb_gold_lap.line_to(sign * 10.0, 12.9);
    pb_gold_lap.line_to(sign * 5.3, 14.7);
    pb_gold_lap.close();

    let mut pb_lap_hi = PathBuilder::new();
    pb_lap_hi.move_to(sign * 5.4, 2.2);
    pb_lap_hi.line_to(sign * 5.4, 14.2);

    let mut pb_ear = PathBuilder::new();
    pb_ear.move_to(sign * 7.2, -3.2);
    pb_ear.cubic_to(sign * 9.2, -9.0, sign * 9.2, -14.8, sign * 8.0, -18.2);
    pb_ear.cubic_to(sign * 5.8, -14.2, sign * 4.4, -9.5, sign * 3.8, -5.8);
    pb_ear.close();

    let mut pb_ear_rim = PathBuilder::new();
    pb_ear_rim.move_to(sign * 7.3, -4.0);
    pb_ear_rim.cubic_to(sign * 9.0, -9.2, sign * 9.0, -14.6, sign * 8.0, -18.0);

    let mut pb_gold_ear = PathBuilder::new();
    pb_gold_ear.move_to(sign * 6.7, -4.9);
    pb_gold_ear.cubic_to(sign * 8.1, -9.5, sign * 8.0, -13.9, sign * 7.45, -16.1);
    pb_gold_ear.cubic_to(sign * 6.0, -13.2, sign * 5.0, -9.8, sign * 4.7, -6.3);
    pb_gold_ear.close();

    let mut pb_chk = PathBuilder::new();
    pb_chk.move_to(sign * 7.5, 0.2);
    pb_chk.line_to(sign * 5.8, 2.6);
    pb_chk.line_to(sign * 5.0, 6.2);
    pb_chk.line_to(sign * 6.9, 3.6);
    pb_chk.close();

    let mut pb_sock = PathBuilder::new();
    pb_sock.move_to(sign * 1.1, 1.4);
    pb_sock.cubic_to(sign * 2.4, -0.5, sign * 4.8, -1.1, sign * 7.0, -1.0);
    pb_sock.cubic_to(sign * 6.0, 1.6, sign * 3.6, 2.5, sign * 1.1, 1.4);
    pb_sock.close();

    let mut pb_eye = PathBuilder::new();
    pb_eye.move_to(sign * 1.45, 1.25);
    pb_eye.cubic_to(sign * 2.55, -0.15, sign * 4.55, -0.72, sign * 6.45, -0.72);
    pb_eye.cubic_to(sign * 5.45, 1.22, sign * 3.35, 1.92, sign * 1.45, 1.25);
    pb_eye.close();

    let mut pb_core = PathBuilder::new();
    pb_core.move_to(sign * 2.15, 1.05);
    pb_core.cubic_to(sign * 2.95, 0.25, sign * 4.25, -0.05, sign * 5.45, -0.05);
    pb_core.cubic_to(sign * 4.75, 0.95, sign * 3.35, 1.35, sign * 2.15, 1.05);
    pb_core.close();

    MascotSidePaths {
        wing: pb_wing.finish().unwrap(),
        wing_rim: pb_wing_rim.finish().unwrap(),
        gold_lap: pb_gold_lap.finish().unwrap(),
        lap_hi: pb_lap_hi.finish().unwrap(),
        ear: pb_ear.finish().unwrap(),
        ear_rim: pb_ear_rim.finish().unwrap(),
        gold_ear: pb_gold_ear.finish().unwrap(),
        chk: pb_chk.finish().unwrap(),
        sock: pb_sock.finish().unwrap(),
        eye: pb_eye.finish().unwrap(),
        core: pb_core.finish().unwrap(),
    }
}

fn mascot_paths() -> &'static MascotPaths {
    MASCOT_PATHS.get_or_init(|| {
        let mut pb_head = PathBuilder::new();
        pb_head.move_to(0.0, -8.8);
        pb_head.cubic_to(4.2, -8.8, 7.6, -6.0, 8.2, -1.6);
        pb_head.cubic_to(8.5, 1.0, 6.2, 4.6, 4.3, 8.2);
        pb_head.cubic_to(3.8, 10.6, 2.8, 12.6, 0.0, 12.9);
        pb_head.cubic_to(-2.8, 12.6, -3.8, 10.6, -4.3, 8.2);
        pb_head.cubic_to(-6.2, 4.6, -8.5, 1.0, -8.2, -1.6);
        pb_head.cubic_to(-7.6, -6.0, -4.2, -8.8, 0.0, -8.8);
        pb_head.close();

        let mut pb_brow = PathBuilder::new();
        pb_brow.move_to(-7.4, -2.5);
        pb_brow.cubic_to(-5.0, -4.4, -2.5, -5.8, 0.0, -6.2);
        pb_brow.cubic_to(2.5, -5.8, 5.0, -4.4, 7.4, -2.5);
        pb_brow.line_to(6.0, -1.3);
        pb_brow.line_to(1.8, -0.9);
        pb_brow.line_to(0.0, 0.25);
        pb_brow.line_to(-1.8, -0.9);
        pb_brow.line_to(-6.0, -1.3);
        pb_brow.close();

        let mut pb_brow_rim = PathBuilder::new();
        pb_brow_rim.move_to(-6.9, -1.7);
        pb_brow_rim.line_to(-1.8, -0.9);
        pb_brow_rim.line_to(0.0, 0.25);
        pb_brow_rim.line_to(1.8, -0.9);
        pb_brow_rim.line_to(6.9, -1.7);

        let mut pb_bridge = PathBuilder::new();
        pb_bridge.move_to(0.0, 0.2);
        pb_bridge.line_to(1.7, 7.9);
        pb_bridge.line_to(0.0, 8.3);
        pb_bridge.line_to(-1.7, 7.9);
        pb_bridge.close();

        let mut pb_ridge = PathBuilder::new();
        pb_ridge.move_to(0.0, 0.9);
        pb_ridge.line_to(0.0, 7.6);

        let mut pb_nose = PathBuilder::new();
        pb_nose.move_to(0.0, 7.75);
        pb_nose.cubic_to(1.85, 7.65, 2.05, 8.85, 1.25, 10.1);
        pb_nose.cubic_to(0.6, 10.75, -0.6, 10.75, -1.25, 10.1);
        pb_nose.cubic_to(-2.05, 8.85, -1.85, 7.65, 0.0, 7.75);
        pb_nose.close();

        let mut pb_nose_hi = PathBuilder::new();
        pb_nose_hi.move_to(-1.05, 8.15);
        pb_nose_hi.cubic_to(-0.35, 7.9, 0.35, 7.9, 1.05, 8.15);

        let mut pb_chin_rim = PathBuilder::new();
        pb_chin_rim.move_to(-2.3, 12.15);
        pb_chin_rim.cubic_to(-0.9, 12.75, 0.9, 12.75, 2.3, 12.15);

        let gem_cx = 0.0;
        let gem_cy = -5.05;
        let facet_specs: [(f32, f32, f32, f32, Color); 4] = [
            (gem_cx, -7.75, -2.3, gem_cy, rgba(254, 249, 195, 255)),
            (2.3, gem_cy, gem_cx, -7.75, rgba(250, 204, 21, 255)),
            (-2.3, gem_cy, gem_cx, -2.35, rgba(245, 158, 11, 255)),
            (gem_cx, -2.35, 2.3, gem_cy, rgba(180, 83, 9, 255)),
        ];
        let diamond_facets = facet_specs.map(|(x1, y1, x2, y2, col)| {
            let mut pb_f = PathBuilder::new();
            pb_f.move_to(gem_cx, gem_cy);
            pb_f.line_to(x1, y1);
            pb_f.line_to(x2, y2);
            pb_f.close();
            (pb_f.finish().unwrap(), col)
        });

        let mut pb_gem_rim = PathBuilder::new();
        pb_gem_rim.move_to(gem_cx, -7.75);
        pb_gem_rim.line_to(2.3, gem_cy);
        pb_gem_rim.line_to(gem_cx, -2.35);
        pb_gem_rim.line_to(-2.3, gem_cy);
        pb_gem_rim.close();

        MascotPaths {
            sides: [build_side_paths(-1.0), build_side_paths(1.0)],
            head: pb_head.finish().unwrap(),
            brow: pb_brow.finish().unwrap(),
            brow_rim: pb_brow_rim.finish().unwrap(),
            bridge: pb_bridge.finish().unwrap(),
            ridge: pb_ridge.finish().unwrap(),
            nose: pb_nose.finish().unwrap(),
            nose_hi: pb_nose_hi.finish().unwrap(),
            chin_rim: pb_chin_rim.finish().unwrap(),
            diamond_facets,
            gem_rim: pb_gem_rim.finish().unwrap(),
        }
    })
}

impl Renderer {
    pub(crate) fn draw_cyno_mascot(
        p: &mut Painter,
        cx: f32,
        cy: f32,
        scale: f32,
        mood: MascotMood,
        anim_time: f32,
        compact_fx: bool,
    ) {
        let k = scale;
        let t = anim_time.max(0.0);

        let rot_at = |deg: f32, px: f32, py: f32| -> Transform {
            Transform::from_translate(px, py)
                .pre_rotate(deg)
                .pre_translate(-px, -py)
        };

        // 1. Derive articulated kinematic & glow parameters per mood (zero-bounce smooth waves)
        let (bob_y, tilt_deg, sx, sy, ear_l_deg, ear_r_deg, eye_open, eye_boost, diamond_boost) =
            match mood {
                MascotMood::Idle => {
                    let bob = (t * 2.1).sin() * 0.72;
                    let tilt = (t * 0.95).sin() * 1.4;
                    let breathe = 1.0 + (t * 2.1 + 0.4).sin() * 0.015;
                    // Smooth single-ear alert twitch inside 5.4s cycle
                    let cycle = t % 5.4;
                    let twitch_l = if (1.2..1.62).contains(&cycle) {
                        ((cycle - 1.2) / 0.42 * std::f32::consts::PI).sin() * 4.5
                    } else {
                        0.0
                    };
                    let twitch_r = if (3.6..4.02).contains(&cycle) {
                        -((cycle - 3.6) / 0.42 * std::f32::consts::PI).sin() * 4.2
                    } else {
                        0.0
                    };
                    let ear_sway = (t * 1.5).sin() * 1.1;
                    // Smooth natural blink every 4.4s
                    let blk = t % 4.4;
                    let open = if blk > 4.18 {
                        let u = (blk - 4.18) / 0.22;
                        1.0 - (u * std::f32::consts::PI).sin() * 0.88
                    } else {
                        1.0
                    };
                    let glow_b = 0.85 + 0.25 * ((t * 2.4).sin() * 0.5 + 0.5);
                    (
                        bob,
                        tilt,
                        breathe,
                        breathe,
                        ear_sway + twitch_l,
                        -ear_sway + twitch_r,
                        open,
                        glow_b,
                        glow_b,
                    )
                }
                MascotMood::Purging => {
                    let bob = (t * 9.0).sin() * 0.95 - 0.55;
                    let tilt = (t * 13.0).sin() * 2.1;
                    let surge = 1.0 + 0.042 * ((t * 11.0).sin() * 0.5 + 0.5);
                    let ear_vib = (t * 18.0).sin() * 2.4;
                    (
                        bob,
                        tilt,
                        surge,
                        surge * 1.02,
                        -4.5 + ear_vib,
                        4.5 - ear_vib,
                        1.08,
                        1.75,
                        1.85,
                    )
                }
                MascotMood::Music => {
                    // ~120 BPM groove cadence
                    let beat = t * std::f32::consts::TAU;
                    let bob = (beat * 2.0).sin() * 0.95;
                    let tilt = beat.sin() * 4.6;
                    let groove = 1.0 + 0.03 * ((beat * 2.0).cos() * 0.5 + 0.5);
                    let ear_l = -1.5 + (beat * 2.0 - 0.35).sin() * 3.8;
                    let ear_r = 1.5 - (beat * 2.0 + 0.35).sin() * 3.8;
                    let pulse = 1.05 + 0.35 * ((beat * 2.0).cos() * 0.5 + 0.5);
                    (bob, tilt, groove, groove, ear_l, ear_r, 1.0, pulse, pulse)
                }
                MascotMood::Capture => {
                    let flash = ((t * 7.0).sin() * 0.5 + 0.5).powf(2.0);
                    let scale_p = 1.0 + 0.035 * flash;
                    (
                        flash * -0.45,
                        0.0,
                        scale_p,
                        scale_p,
                        -2.5,
                        2.5,
                        1.04,
                        1.35 + flash * 0.4,
                        1.3 + flash * 0.4,
                    )
                }
            };

        let head_cx = cx;
        let head_cy = cy + bob_y * k;

        let head_tf = Transform::from_translate(head_cx, head_cy)
            .pre_rotate(tilt_deg)
            .pre_scale(k * sx, k * sy);
        let ear_l_tf = head_tf.pre_concat(rot_at(ear_l_deg, -5.5, -5.2));
        let ear_r_tf = head_tf.pre_concat(rot_at(ear_r_deg, 5.5, -5.2));

        // 2. Background Aura & Mood Effects Layer (rendered behind sculpted bust)
        match mood {
            MascotMood::Idle => {
                let a = (18.0 + 14.0 * ((t * 2.2).sin() * 0.5 + 0.5)) as u8;
                p.radial_glow(
                    head_cx,
                    head_cy,
                    15.0 * k,
                    rgba(139, 92, 246, a),
                    BlendMode::Plus,
                    false,
                );
            }
            MascotMood::Purging => {
                p.radial_glow(
                    head_cx,
                    head_cy,
                    19.0 * k,
                    rgba(168, 85, 247, 72),
                    BlendMode::Plus,
                    false,
                );
                p.radial_glow(
                    head_cx,
                    head_cy - 3.5 * k,
                    13.0 * k,
                    rgba(221, 214, 254, 52),
                    BlendMode::Plus,
                    false,
                );
                // Dual staggered expanding shockwave rings
                for ring_i in 0..2 {
                    let phase = (t * 1.45 + ring_i as f32 * 0.5) % 1.0;
                    let max_r = if compact_fx { 13.5 } else { 19.0 };
                    let rr = (6.5 + phase * (max_r - 6.5)) * k;
                    let fade_a = ((1.0 - phase).powf(1.4) * 175.0) as u8;
                    let col = if ring_i == 0 {
                        rgba(192, 132, 252, fade_a)
                    } else {
                        rgba(233, 218, 255, fade_a)
                    };
                    p.circle_stroke(
                        head_cx,
                        head_cy,
                        rr,
                        (1.15 * (1.0 - phase * 0.45) * k).max(0.6),
                        col,
                    );
                }
                // Crackling temple & ear lightning arcs
                if !compact_fx {
                    let flip = ((t * 12.0).floor() as i32) % 2 == 0;
                    let jitter = if flip { 0.8 } else { -0.7 };
                    let bolts = [
                        (
                            [-10.2, -13.5, -13.6 + jitter, -8.8, -11.4 - jitter, -4.5],
                            rgba(192, 132, 252, 220),
                        ),
                        (
                            [10.2, -13.5, 13.8 - jitter, -8.6, 11.4 + jitter, -4.2],
                            rgba(233, 218, 255, 225),
                        ),
                    ];
                    for (pts, col) in bolts {
                        let mut pb = PathBuilder::new();
                        pb.move_to(pts[0], pts[1]);
                        pb.line_to(pts[2], pts[3]);
                        pb.line_to(pts[4], pts[5]);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(
                                &path,
                                1.05,
                                col,
                                Transform::from_translate(head_cx, head_cy).pre_scale(k, k),
                            );
                        }
                    }
                }
            }
            MascotMood::Music => {
                let beat_amp = (t * std::f32::consts::TAU * 2.0).cos() * 0.5 + 0.5;
                p.radial_glow(
                    head_cx,
                    head_cy,
                    (15.5 + 3.5 * beat_amp) * k,
                    rgba(168, 85, 247, (44.0 + 36.0 * beat_amp) as u8),
                    BlendMode::Plus,
                    false,
                );
                p.radial_glow(
                    head_cx,
                    head_cy - 5.0 * k,
                    11.0 * k,
                    rgba(221, 214, 254, (24.0 + 24.0 * beat_amp) as u8),
                    BlendMode::Plus,
                    false,
                );
                if !compact_fx && k >= 1.25 {
                    // Music Hero flanking mini equalizer spectrum bars
                    let bar_offsets = [-24.0, -20.5, -17.0, 17.0, 20.5, 24.0];
                    for (bi, &bx_loc) in bar_offsets.iter().enumerate() {
                        let wv = (t * 8.2 + bi as f32 * 1.15).sin() * 0.5 + 0.5;
                        let bh = 3.2 + wv * 7.8;
                        let col = if bi % 2 == 0 {
                            rgba(192, 132, 252, 210)
                        } else {
                            rgba(233, 218, 255, 215)
                        };
                        let mut pb = PathBuilder::new();
                        pb.move_to(bx_loc, -bh * 0.5);
                        pb.line_to(bx_loc, bh * 0.5);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(
                                &path,
                                1.65,
                                col,
                                Transform::from_translate(head_cx, head_cy).pre_scale(k, k),
                            );
                        }
                    }
                }
            }
            MascotMood::Capture => {
                p.radial_glow(
                    head_cx,
                    head_cy,
                    16.0 * k,
                    rgba(192, 132, 252, 54),
                    BlendMode::Plus,
                    false,
                );
                if !compact_fx {
                    let squeeze = 14.2 - ((t * 5.0).sin() * 0.5 + 0.5) * 1.4;
                    let arm = 3.2;
                    let tf_s = Transform::from_translate(head_cx, head_cy).pre_scale(k, k);
                    for (sx_s, sy_s) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        let mut pb = PathBuilder::new();
                        let vx = sx_s * squeeze;
                        let vy = sy_s * squeeze;
                        pb.move_to(vx - sx_s * arm, vy);
                        pb.line_to(vx, vy);
                        pb.line_to(vx, vy - sy_s * arm);
                        if let Some(path) = pb.finish() {
                            p.stroke_path_tf(&path, 0.9, rgba(233, 213, 255, 195), tf_s);
                        }
                    }
                }
            }
        }

        let mp = mascot_paths();

        // 3. Dual Sculpted Ceremonial Side Lappets (obsidan wings + champagne-gold inner blades)
        for (idx, sign) in [-1.0f32, 1.0f32].into_iter().enumerate() {
            let sp = &mp.sides[idx];
            // Outer Obsidian-Violet Lappet Wing
            p.fill_linear_path_tf(
                &sp.wing,
                sign * 6.5,
                -3.5,
                sign * 12.2,
                13.2,
                rgba(34, 30, 56, 255),
                rgba(11, 10, 20, 255),
                Some(rgba(21, 19, 36, 255)),
                head_tf,
            );
            p.stroke_path_tf(&sp.wing_rim, 0.55, rgba(168, 85, 247, 135), head_tf);

            // Inner Brushed Champagne-Gold Lappet Blade
            p.fill_linear_path_tf(
                &sp.gold_lap,
                sign * 6.8,
                1.8,
                sign * 7.6,
                14.6,
                rgba(253, 230, 138, 255),
                rgba(120, 53, 15, 255),
                Some(rgba(217, 119, 6, 255)),
                head_tf,
            );
            p.stroke_path_tf(&sp.lap_hi, 0.55, rgba(254, 249, 195, 175), head_tf);
        }

        // 4. Tall Pointed Articulated Cyber-Jackal Ears (Obsidian shell + Champagne-Gold inner bowls)
        for (idx, (sign, ear_tf)) in [(-1.0f32, ear_l_tf), (1.0f32, ear_r_tf)]
            .into_iter()
            .enumerate()
        {
            let sp = &mp.sides[idx];
            p.fill_linear_path_tf(
                &sp.ear,
                sign * 6.0,
                -18.0,
                sign * 6.2,
                -3.5,
                rgba(56, 58, 78, 255),
                rgba(12, 12, 19, 255),
                Some(rgba(25, 25, 38, 255)),
                ear_tf,
            );
            p.stroke_path_tf(&sp.ear_rim, 0.65, rgba(203, 213, 225, 135), ear_tf);

            // Inner Champagne Gold Cavity
            p.fill_linear_path_tf(
                &sp.gold_ear,
                sign * 6.8,
                -16.0,
                sign * 6.0,
                -5.0,
                rgba(254, 240, 138, 255),
                rgba(120, 53, 15, 255),
                Some(rgba(234, 179, 8, 255)),
                ear_tf,
            );
        }

        // 5. Obsidian Cranium, Armor Brow, Tapered Jackal Snout & Gold Cheek Chamfers
        p.fill_linear_path_tf(
            &mp.head,
            0.0,
            -8.8,
            0.0,
            12.9,
            rgba(72, 74, 96, 255),
            rgba(9, 9, 15, 255),
            Some(rgba(26, 26, 38, 255)),
            head_tf,
        );

        // Lower Cheek Metallic Gold Accents
        for (idx, sign) in [-1.0f32, 1.0f32].into_iter().enumerate() {
            let sp = &mp.sides[idx];
            p.fill_linear_path_tf(
                &sp.chk,
                sign * 7.2,
                0.5,
                sign * 5.2,
                6.0,
                rgba(253, 230, 138, 240),
                rgba(146, 64, 14, 240),
                None,
                head_tf,
            );
        }

        // Recessed Dark Eye Sockets shadow
        for sp in &mp.sides {
            p.fill_path_tf(&sp.sock, rgba(6, 6, 11, 235), head_tf);
        }

        // Sculpted Overhanging V-Brow Plate
        p.fill_linear_path_tf(
            &mp.brow,
            0.0,
            -6.2,
            0.0,
            0.3,
            rgba(48, 47, 68, 255),
            rgba(15, 14, 24, 255),
            None,
            head_tf,
        );
        p.stroke_path_tf(&mp.brow_rim, 0.55, rgba(203, 213, 225, 130), head_tf);

        // Raised Jackal Nose Bridge & Specular Ridge
        p.fill_linear_path_tf(
            &mp.bridge,
            0.0,
            0.2,
            0.0,
            8.3,
            rgba(52, 53, 72, 255),
            rgba(19, 19, 30, 255),
            None,
            head_tf,
        );
        p.stroke_path_tf(&mp.ridge, 0.65, rgba(203, 213, 225, 110), head_tf);

        // Glossy Pitch-Black Jackal Nose Tip + Chin Specular Bevel
        p.fill_linear_path_tf(
            &mp.nose,
            0.0,
            7.7,
            0.0,
            10.7,
            rgba(12, 12, 18, 255),
            rgba(3, 3, 5, 255),
            None,
            head_tf,
        );
        p.stroke_path_tf(&mp.nose_hi, 0.55, rgba(226, 232, 240, 170), head_tf);
        p.stroke_path_tf(&mp.chin_rim, 0.55, rgba(167, 139, 250, 95), head_tf);

        // 6. Electric-Violet Glowing Slanted Anubis Eyes
        let eye_bloom_a = (128.0 * eye_boost).clamp(40.0, 230.0) as u8;
        let eye_mid_col = if mood == MascotMood::Purging {
            rgba(216, 180, 254, 255)
        } else {
            rgba(168, 85, 247, 255)
        };
        let eye_core_col = if mood == MascotMood::Purging {
            rgba(255, 255, 255, 255)
        } else {
            rgba(245, 208, 254, 255)
        };

        for (idx, sign) in [-1.0f32, 1.0f32].into_iter().enumerate() {
            let sp = &mp.sides[idx];
            let eye_tf = if (eye_open - 1.0).abs() > 0.01 {
                head_tf.pre_concat(
                    Transform::from_translate(0.0, 0.65)
                        .pre_scale(1.0, eye_open)
                        .pre_translate(0.0, -0.65),
                )
            } else {
                head_tf
            };

            let bloom_tf = eye_tf.pre_concat(
                Transform::from_translate(sign * 3.85, 0.62)
                    .pre_scale(1.38, 1.55)
                    .pre_translate(-sign * 3.85, -0.62),
            );
            p.fill_path_tf_blend(
                &sp.eye,
                rgba(168, 85, 247, eye_bloom_a),
                bloom_tf,
                BlendMode::Plus,
            );
            p.fill_path_tf(&sp.eye, eye_mid_col, eye_tf);

            // Razor-hot Neon Inner Iris Core
            p.fill_path_tf(&sp.core, eye_core_col, eye_tf);
        }

        // 7. 4-Facet Luminous Golden Forehead Diamond Crest
        let gem_cx = 0.0;
        let gem_cy = -5.05;
        let mut gem_pt = Point::from_xy(gem_cx, gem_cy);
        head_tf.map_point(&mut gem_pt);
        let gem_bloom_a = (86.0 * diamond_boost).clamp(40.0, 220.0) as u8;
        p.radial_glow(
            gem_pt.x,
            gem_pt.y,
            (6.2 * diamond_boost.min(1.45)) * k,
            rgba(251, 191, 36, gem_bloom_a),
            BlendMode::Plus,
            false,
        );

        for (path, col) in &mp.diamond_facets {
            p.fill_path_tf(path, *col, head_tf);
        }
        p.stroke_path_tf(&mp.gem_rim, 0.45, rgba(254, 240, 138, 210), head_tf);

        // 8. Foreground Accent Sparks / Notes / Droplets / Purge Cross-Star
        match mood {
            MascotMood::Purging => {
                // Radiant 4-point energy star over forehead diamond
                let star_r = (3.4 + 1.2 * ((t * 14.0).sin() * 0.5 + 0.5)) * k;
                let (gx, gy) = (gem_pt.x, gem_pt.y);
                let mut pb_st = PathBuilder::new();
                pb_st.move_to(gx - star_r, gy);
                pb_st.line_to(gx + star_r, gy);
                pb_st.move_to(gx, gy - star_r);
                pb_st.line_to(gx, gy + star_r);
                if let Some(path) = pb_st.finish() {
                    p.stroke_path(&path, 1.15 * k, rgba(255, 251, 235, 225));
                }
            }
            MascotMood::Music if !compact_fx => {
                // Floating glowing music notes (higher richness on large Music hero)
                let note_specs: [(f32, f32, f32, Color); 2] = [
                    (-13.8, -14.0, 0.0, rgba(232, 121, 249, 235)),
                    (14.5, -15.5, 1.7, rgba(251, 191, 36, 235)),
                ];
                let scale_factor = if k >= 1.25 { 1.0 } else { 0.85 };
                for (nx_base, ny_base, ph, col) in note_specs {
                    let lift = (t * 2.4 + ph).sin() * 1.4;
                    let drift = (t * 1.6 + ph).cos() * 0.8;
                    let nx = head_cx + nx_base * scale_factor * k + drift * k;
                    let ny = head_cy + (ny_base * scale_factor + lift) * k;
                    let nr = (1.35 * k).max(1.0);
                    p.circle(nx, ny, nr, col);
                    let mut pb_stem = PathBuilder::new();
                    pb_stem.move_to(nx + nr * 0.65, ny);
                    pb_stem.line_to(nx + nr * 0.65, ny - 4.2 * k);
                    pb_stem.line_to(nx + 2.6 * k, ny - 3.3 * k);
                    if let Some(path) = pb_stem.finish() {
                        p.stroke_path(&path, (0.9 * k).max(0.75), col);
                    }
                }
            }
            MascotMood::Capture if !compact_fx => {
                // Crisp lens star glint top-right
                let gx = head_cx + 12.2 * k;
                let gy = head_cy - 13.0 * k;
                let r = (2.8 * (0.7 + 0.3 * ((t * 7.0).sin() * 0.5 + 0.5))) * k;
                let mut pb_gl = PathBuilder::new();
                pb_gl.move_to(gx - r, gy);
                pb_gl.line_to(gx + r, gy);
                pb_gl.move_to(gx, gy - r);
                pb_gl.line_to(gx, gy + r);
                if let Some(path) = pb_gl.finish() {
                    p.stroke_path(&path, (1.0 * k).max(0.75), rgba(243, 232, 255, 235));
                }
            }
            _ => {}
        }
    }
}
