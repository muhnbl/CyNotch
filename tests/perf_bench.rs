use cynotch::models::{AppState, NotchMode, NotchPlacement, NotifHistoryEntry};
use cynotch::renderer::Renderer;
use std::time::Instant;

fn bench_case<F: FnMut(usize)>(label: &str, iters: usize, mut f: F) {
    // Warmup
    for i in 0..3 {
        f(i);
    }
    let t0 = Instant::now();
    for i in 0..iters {
        f(i);
    }
    let dt = t0.elapsed();
    let avg_us = dt.as_secs_f64() * 1_000_000.0 / (iters as f64);
    let avg_ms = avg_us / 1_000.0;
    let fps = if avg_us > 0.0 {
        1_000_000.0 / avg_us
    } else {
        0.0
    };
    println!(
        "{label:<44} | {avg_us:8.1} us ({avg_ms:6.3} ms) | {fps:8.1} FPS ({iters} iters)"
    );
}

#[test]
fn bench_v022_render_paths() {
    let iters = 40;
    println!("\n=== CyNotch v0.2.2 Render Benchmark ===");

    // 1. Top Music + Notifications (overlap fix path)
    let mut st_music_notif = AppState::new_for_preview();
    st_music_notif.placement = NotchPlacement::Top;
    st_music_notif.mode = NotchMode::Music;
    st_music_notif.set_hovered(true);
    st_music_notif.show_notif_history = true;
    st_music_notif.media.is_playing = true;
    st_music_notif.notif_history.push_back(NotifHistoryEntry {
        app_name: "System".into(),
        aumid: None,
        title: "Bench Notification".into(),
        subtitle: "Testing overlap-free drawer".into(),
        received_at: Instant::now(),
    });
    st_music_notif.sync_preview_settled();
    let mut r_mn = Renderer::new(
        st_music_notif.canvas_width(),
        st_music_notif.canvas_height(),
    );
    bench_case("Top Music + Notifications", iters, |i| {
        st_music_notif.anim_time = (i as f32) * 0.016;
        r_mn.render_centered(&st_music_notif, None);
    });

    // 2. Top Expanded (cache hit vs cache miss, Frosted ON vs OFF)
    for frosted in [false, true] {
        let mut st = AppState::new_for_preview();
        st.placement = NotchPlacement::Top;
        st.mode = NotchMode::Home;
        st.set_hovered(true);
        st.windows_transparency_enabled = frosted;
        st.sync_preview_settled();

        let mut r_hit = Renderer::new(st.canvas_width(), st.canvas_height());
        r_hit.render_centered(&st, None);
        let tag = if frosted { "Frosted ON" } else { "Frosted OFF" };
        bench_case(&format!("Top Expanded Cache Hit ({tag})"), iters, |i| {
            st.anim_time = (i as f32) * 0.016;
            r_hit.render_centered(&st, None);
        });

        let mut r_miss = Renderer::new(st.canvas_width(), st.canvas_height());
        bench_case(&format!("Top Expanded Cache Miss ({tag})"), iters, |i| {
            st.telemetry.cpu_usage = 20.0 + (i % 50) as f32;
            r_miss.render_centered(&st, None);
        });
    }

    // 3. Right Vertical Expanded (Frosted ON vs OFF)
    for frosted in [false, true] {
        let mut st = AppState::new_for_preview();
        st.placement = NotchPlacement::Right;
        st.mode = NotchMode::Home;
        st.set_hovered(true);
        st.windows_transparency_enabled = frosted;
        st.sync_preview_settled();
        let mut r = Renderer::new(st.canvas_width(), st.canvas_height());
        let tag = if frosted { "Frosted ON" } else { "Frosted OFF" };
        bench_case(&format!("Right Vertical Expanded ({tag})"), iters, |i| {
            st.anim_time = (i as f32) * 0.016;
            r.render_centered(&st, None);
        });
    }

    // 4. Idle Capsule (Frosted ON vs OFF)
    for frosted in [false, true] {
        let mut st = AppState::new_for_preview();
        st.placement = NotchPlacement::Top;
        st.set_hovered(false);
        st.windows_transparency_enabled = frosted;
        st.sync_preview_settled();
        let mut r = Renderer::new(st.canvas_width(), st.canvas_height());
        let tag = if frosted { "Frosted ON" } else { "Frosted OFF" };
        bench_case(&format!("Idle Capsule ({tag})"), iters, |i| {
            st.anim_time = (i as f32) * 0.016;
            r.render_centered(&st, None);
        });
    }
}
