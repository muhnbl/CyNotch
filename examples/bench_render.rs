//! Micro-benchmark harness for CyNotch's render pipeline.
//!
//! Run with:
//!   `cargo run --release --example bench_render`              (all scenarios)
//!   `cargo run --release --example bench_render -- 300 idle` (single scenario + primitive profile)

use cynotch::models::{AppState, IdleMetric, NotchMode, NotchPlacement, TypeScale};
use cynotch::renderer::Renderer;
use std::time::Instant;

fn scenario(name: &str) -> AppState {
    let mut s = AppState::new_for_preview();
    match name {
        // Production-like minimized right-edge island (user's live config).
        "minimized_vert" => {
            s.placement = NotchPlacement::Right;
            s.minimized_island = true;
            s.idle_metric = IdleMetric::Minimal;
            s.type_scale = TypeScale::Large;
            s.show_mascot = true;
            s.is_hovered = false;
            s.is_pinned = false;
            s.notification = None;
            s.media.is_playing = false;
            s.timer.is_running = false;
        }
        "idle" => {
            s.mode = NotchMode::Home;
            s.is_hovered = false;
            s.is_pinned = false;
            s.notification = None;
            s.media.is_playing = false;
            s.timer.is_running = false;
            s.show_mascot = true;
            s.idle_metric = IdleMetric::Ram;
        }
        "idle_vert" => {
            s.placement = NotchPlacement::Right;
            s.idle_metric = IdleMetric::NetSpeed;
            s.is_hovered = false;
            s.is_pinned = false;
            s.notification = None;
            s.media.is_playing = false;
            s.timer.is_running = false;
            s.show_mascot = true;
        }
        "home" => {
            s.mode = NotchMode::Home;
            s.is_hovered = true;
            s.is_pinned = true;
            s.media.is_playing = true;
            s.anim_time = 1.2;
        }
        "lyrics" => {
            s.mode = NotchMode::Music;
            s.show_lyrics = true;
            s.is_hovered = true;
            s.is_pinned = true;
            s.media.is_playing = true;
        }
        "nostruct" => {
            // Same as minimized_vert but without the mascot, to attribute its cost.
            s.placement = NotchPlacement::Right;
            s.minimized_island = true;
            s.idle_metric = IdleMetric::Minimal;
            s.type_scale = TypeScale::Large;
            s.show_mascot = false;
            s.is_hovered = false;
            s.is_pinned = false;
            s.notification = None;
            s.media.is_playing = false;
            s.timer.is_running = false;
        }
        _ => {}
    }
    s.sync_preview_settled();
    s
}

fn run(name: &str, iters: u32) -> f64 {
    let base = scenario(name);
    let mut r = Renderer::new(base.canvas_width(), base.canvas_height());
    let mut phase = 0.35f32;
    let frame = |r: &mut Renderer, phase: &mut f32| {
        *phase += 0.028;
        let mut s = base.clone();
        s.anim_time = *phase;
        r.render(&s);
    };
    for _ in 0..8 {
        frame(&mut r, &mut phase);
    }
    let t0 = Instant::now();
    for _ in 0..iters {
        frame(&mut r, &mut phase);
    }
    let per = t0.elapsed().as_secs_f64() * 1_000_000.0 / iters as f64;
    println!("{name:<16} {per:>9.1} us/frame  (canvas {}x{})", base.canvas_width(), base.canvas_height());
    per
}

fn glow_micro() {
    use tiny_skia::*;
    let mut pm = Pixmap::new(676, 253).unwrap();
    let sc = Color::from_rgba8(124, 58, 237, 60);
    let stops = |s: Color| {
        vec![
            GradientStop::new(0.0, s),
            GradientStop::new(0.45, Color::from_rgba(s.red(), s.green(), s.blue(), s.alpha() * 0.42).unwrap()),
            GradientStop::new(0.75, Color::from_rgba(s.red(), s.green(), s.blue(), s.alpha() * 0.12).unwrap()),
            GradientStop::new(1.0, Color::from_rgba(s.red(), s.green(), s.blue(), 0.0).unwrap()),
        ]
    };
    let it = 400u32;
    let mut bench = |label: &str, f: &mut dyn FnMut(&mut Pixmap)| {
        for _ in 0..20 {
            f(&mut pm);
        }
        let t0 = Instant::now();
        for _ in 0..it {
            f(&mut pm);
        }
        println!("{label:<40} {:>8.1} us", t0.elapsed().as_secs_f64() * 1e6 / it as f64);
    };

    // (a) current: AA circle path, radius 118
    let path = PathBuilder::from_circle(300.0, 120.0, 118.0).unwrap();
    bench("a) AA circle r=118 + radial", &mut |pm| {
        let sh = RadialGradient::new(
            Point::from_xy(300.0, 120.0),
            Point::from_xy(300.0, 120.0),
            118.0,
            stops(sc),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let p = Paint { shader: sh, anti_alias: true, ..Default::default() };
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    });

    // (b) bbox rect + radial gradient, no AA
    let rect = Rect::from_xywh(182.0, 2.0, 236.0, 236.0).unwrap();
    bench("b) rect 236x236 + radial (no AA)", &mut |pm| {
        let sh = RadialGradient::new(
            Point::from_xy(300.0, 120.0),
            Point::from_xy(300.0, 120.0),
            118.0,
            stops(sc),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let p = Paint { shader: sh, anti_alias: false, ..Default::default() };
        pm.fill_rect(rect, &p, Transform::identity(), None);
    });

    // (b2) card-sized rect (300x70) + radial gradient, no AA
    let card = Rect::from_xywh(180.0, 90.0, 300.0, 70.0).unwrap();
    bench("b2) rect 300x70 + radial (no AA)", &mut |pm| {
        let sh = RadialGradient::new(
            Point::from_xy(300.0, 120.0),
            Point::from_xy(300.0, 120.0),
            118.0,
            stops(sc),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let p = Paint { shader: sh, anti_alias: false, ..Default::default() };
        pm.fill_rect(card, &p, Transform::identity(), None);
    });

    // (c) downscaled pattern composite, 236x236
    let mut small = Pixmap::new(70, 70).unwrap();
    {
        let sp = PathBuilder::from_circle(35.0, 35.0, 34.0).unwrap();
        let sh = RadialGradient::new(
            Point::from_xy(35.0, 35.0),
            Point::from_xy(35.0, 35.0),
            34.0,
            stops(sc),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let p = Paint { shader: sh, anti_alias: true, ..Default::default() };
        small.fill_path(&sp, &p, FillRule::Winding, Transform::identity(), None);
    }
    bench("c) 236x236 bilinear pattern (1/3.4x)", &mut |pm| {
        let pat = Pattern::new(small.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::from_scale(3.47, 3.47));
        let p = Paint { shader: pat, anti_alias: false, ..Default::default() };
        pm.fill_rect(rect, &p, Transform::identity(), None);
    });

    // (d) solid rect fill 236x236 for reference
    bench("d) solid rect 236x236", &mut |pm| {
        let mut p = Paint::default();
        p.set_color(sc);
        p.anti_alias = false;
        pm.fill_rect(rect, &p, Transform::identity(), None);
    });
}

fn bench_settings(iters: u32) {
    use cynotch::font::FontRenderer;
    use cynotch::models::SettingsCategory;
    use cynotch::settings_window;
    let mut state = AppState::new_for_preview();
    state.settings_open = true;
    state.set_settings_category(SettingsCategory::General);
    state.set_settings_scroll(SettingsCategory::General, 0.0);
    state.sync_preview_settled();
    let (w, h, _) = settings_window::settings_dims_for_state(&state);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    let mut fonts = FontRenderer::new();
    let render = |pm: &mut tiny_skia::Pixmap, mask: &mut tiny_skia::Mask, fonts: &mut FontRenderer| {
        settings_window::render_settings_pixmap_with_meta(&state, Some((400.0, 300.0)), pm, mask, fonts);
    };
    for _ in 0..4 {
        render(&mut pm, &mut mask, &mut fonts);
    }
    let t0 = Instant::now();
    for _ in 0..iters {
        render(&mut pm, &mut mask, &mut fonts);
    }
    println!(
        "settings pane  {:>9.1} ms/frame  ({}x{})",
        t0.elapsed().as_secs_f64() * 1000.0 / iters as f64,
        w,
        h
    );

}

fn main() {
    let mut args = std::env::args().skip(1);
    let iters: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(300);
    let only = args.next();

    if only.as_deref() == Some("glowmicro") { glow_micro(); return; }
    if only.as_deref() == Some("settings") { bench_settings(iters.max(10)); return; }
    let all = ["minimized_vert", "nostruct", "idle", "idle_vert", "home", "lyrics"];
    match only.as_deref() {
        Some(name) => {
            run(name, iters);
        }
        None => {
            for name in all {
                run(name, iters);
            }
        }
    }

    // Print per-primitive attribution when profiling is on, divided by frame count.

}
