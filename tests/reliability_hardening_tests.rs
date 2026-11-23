#![allow(unsafe_op_in_unsafe_fn)]

use cynotch::autostart::format_autostart_command;
use cynotch::capture::validate_shell_open_target;
use cynotch::font_d2d::DWriteTextCache;
use cynotch::lyrics::{parse_lrc, parse_lrclib_response};
use cynotch::models::{
    Action, AppState, ExpandedSizePreset, NotchPlacement, NotchSizePreset, NotchStyle,
    NotificationKind, TaskItem,
};
use cynotch::painter_d2d::{D2DPainter, D2DResourceCache, color_to_d2d};
use cynotch::persistence::{LoadOutcome, atomic_write_json, backup_path, load_json_with_recovery};
use cynotch::renderer::Renderer;
use cynotch::sound::{find_data_chunk, scale_wav_volume};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_MAP_OPTIONS_READ,
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::Interface;

static SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let id = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "cynotch_hardening_{}_{}_{}",
            tag,
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_isolated_d2d_target(w: u32, h: u32) -> (ID2D1Factory1, ID2D1DeviceContext, ID2D1Bitmap1) {
    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut d3d_dev: Option<ID3D11Device> = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut d3d_dev),
            None,
            None,
        )
        .expect("WARP D3D11CreateDevice");
        let dxgi: IDXGIDevice = d3d_dev.unwrap().cast().unwrap();
        let factory: ID2D1Factory1 =
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
        let d2d_dev: ID2D1Device = factory.CreateDevice(&dxgi).unwrap();
        let ctx: ID2D1DeviceContext = d2d_dev
            .CreateDeviceContext(
                windows::Win32::Graphics::Direct2D::D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
            )
            .unwrap();
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let target = ctx
            .CreateBitmap(
                D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                None,
                0,
                &props,
            )
            .unwrap();
        ctx.SetTarget(&target);
        (factory, ctx, target)
    }
}

fn readback_nonzero_alpha_pixels(
    ctx: &ID2D1DeviceContext,
    target: &ID2D1Bitmap1,
    w: u32,
    h: u32,
) -> usize {
    unsafe {
        let staging_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let staging = ctx
            .CreateBitmap(
                D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                None,
                0,
                &staging_props,
            )
            .unwrap();
        staging.CopyFromBitmap(None, target, None).unwrap();
        let mapped = staging.Map(D2D1_MAP_OPTIONS_READ).unwrap();
        let mut lit = 0usize;
        for row in 0..h as usize {
            let row_ptr = mapped.bits.add(row * mapped.pitch as usize);
            let slice = std::slice::from_raw_parts(row_ptr, (w as usize) * 4);
            for px in slice.chunks_exact(4) {
                if px[3] > 0 {
                    lit += 1;
                }
            }
        }
        staging.Unmap().unwrap();
        lit
    }
}

#[test]
fn bh02_cross_factory_device_lost_recovery_renders_without_wrong_factory_error() {
    let mut state = AppState::new_for_preview();
    state.is_hovered = true;
    state.anim_expand_t = 1.0;
    state.current_width = state.expanded_width();
    state.current_height = state.expanded_height();
    state.set_notification(
        "Recovery Test".to_string(),
        "Simulated GPU factory recreation".to_string(),
        NotificationKind::General,
        true,
    );

    let mut ren = Renderer::new(560, 320).unwrap();

    // Generation 1: Render on Factory #1 (draws mascot, icons, stroke_path, notification banner)
    {
        let (_f1, ctx1, _t1) = create_isolated_d2d_target(560, 320);
        unsafe { ctx1.BeginDraw() };
        ren.render_d2d(&ctx1, &state, None);
        unsafe { ctx1.EndDraw(None, None) }.expect("Factory #1 EndDraw must succeed");
    }

    // Simulate GPU device-lost recovery: drop D2D cache and create a brand-new ID2D1Factory1
    ren.clear_d2d_cache();

    // Generation 2: Render on Factory #2 — would fail with D2DERR_WRONG_FACTORY (0x88990012)
    // before BH-02 fix because thread_local STROKE_STYLE belonged to Factory #1!
    {
        let (_f2, ctx2, target2) = create_isolated_d2d_target(560, 320);
        unsafe { ctx2.BeginDraw() };
        ren.render_d2d(&ctx2, &state, None);
        unsafe { ctx2.EndDraw(None, None) }
            .expect("Factory #2 EndDraw must succeed without D2DERR_WRONG_FACTORY");
        let lit = readback_nonzero_alpha_pixels(&ctx2, &target2, 560, 320);
        assert!(
            lit > 500,
            "recovered frame must draw visible pixels, got {}",
            lit
        );
    }
}

#[test]
fn bh20_full_circle_360_degree_arc_is_visible_in_direct2d() {
    let (_f, ctx, target) = create_isolated_d2d_target(128, 128);
    let mut cache = D2DResourceCache::new(&ctx).unwrap();
    let _fonts = DWriteTextCache::new().unwrap();

    unsafe {
        ctx.BeginDraw();
        ctx.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));
    }
    {
        let mut p = D2DPainter::new(&ctx, &mut cache, 1.0, 0.0, 0.0, 1.0);
        // Full 360-degree sweep (TAU) previously produced start_pt == end_pt and drew 0 pixels.
        p.arc(
            64.0,
            64.0,
            40.0,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::TAU,
            4.0,
            color_to_d2d(155, 89, 252, 255, 1.0),
        )
        .unwrap();
    }
    unsafe { ctx.EndDraw(None, None) }.unwrap();

    let lit = readback_nonzero_alpha_pixels(&ctx, &target, 128, 128);
    assert!(
        lit > 400,
        "full 360-degree progress arc must render visible ring pixels, got {}",
        lit
    );
}

#[test]
fn bh05_push_rrect_clip_repeated_stress_has_no_errors() {
    let (_f, ctx, _target) = create_isolated_d2d_target(128, 128);
    let mut cache = D2DResourceCache::new(&ctx).unwrap();

    unsafe { ctx.BeginDraw() };
    {
        let mut p = D2DPainter::new(&ctx, &mut cache, 1.0, 0.0, 0.0, 1.0);
        for _ in 0..200 {
            p.push_rrect_clip(10.0, 10.0, 100.0, 100.0, 16.0).unwrap();
            p.rrect(
                0.0,
                0.0,
                128.0,
                128.0,
                8.0,
                color_to_d2d(120, 80, 255, 255, 1.0),
            )
            .unwrap();
            p.pop_clip().unwrap();
        }
    }
    unsafe { ctx.EndDraw(None, None) }.expect("balanced push/pop clip stress must pass");
}

#[test]
fn bh09_persistence_stress_and_screentime_recovery() {
    let tmp = TempDir::new("stress_persist");
    let state_file = tmp.path().join("tasks.json");

    // 50 rapid atomic writes
    for i in 1..=50usize {
        let tasks = vec![TaskItem {
            text: format!("Task #{i}\twith\nnewlines"),
            completed: i % 2 == 0,
        }];
        atomic_write_json(&state_file, &tasks, true).expect("atomic write must succeed");
    }

    let loaded = load_json_with_recovery::<Vec<TaskItem>, _>(&state_file, |v| !v.is_empty());
    match loaded {
        LoadOutcome::Loaded(v) => assert_eq!(v[0].text, "Task #50\twith\nnewlines"),
        other => panic!("expected Loaded, got {:?}", other),
    }

    // Corrupt primary file -> must recover v49 from .bak and heal primary
    std::fs::write(&state_file, "{ truncated").unwrap();
    let recovered = load_json_with_recovery::<Vec<TaskItem>, _>(&state_file, |v| !v.is_empty());
    match recovered {
        LoadOutcome::RecoveredFromBackup(v) => assert_eq!(v[0].text, "Task #49\twith\nnewlines"),
        other => panic!("expected RecoveredFromBackup, got {:?}", other),
    }

    // Subsequent atomic write must preserve v49 in `.bak` (never poison `.bak` with corrupt bytes)
    let v51 = vec![TaskItem {
        text: "Task #51".to_string(),
        completed: false,
    }];
    atomic_write_json(&state_file, &v51, true).unwrap();
    match load_json_with_recovery::<Vec<TaskItem>, _>(&backup_path(&state_file), |_| true) {
        LoadOutcome::Loaded(v) => assert_eq!(v[0].text, "Task #49\twith\nnewlines"),
        other => panic!("expected .bak to hold Task #49, got {:?}", other),
    }

    // Fault injection: write to a directory path fails cleanly without panicking
    assert!(atomic_write_json(tmp.path(), &v51, true).is_err());
}

#[test]
fn bh08_bh13_bh14_bh22_security_and_state_stress_invariants() {
    // BH-08: Zero developer personal paths in preview/default state
    let preview = AppState::new_for_preview();
    for item in &preview.shelf_items {
        assert!(
            !item.payload.to_ascii_lowercase().contains("muhna"),
            "personal path leaked in shelf preview: {}",
            item.payload
        );
    }

    // BH-13 & BH-14 policy checks
    assert!(
        format_autostart_command(Path::new(r"C:\Program Files\CyNotch\cynotch.exe"))
            .unwrap()
            .starts_with('"')
    );
    assert!(validate_shell_open_target(r"\\evil\share\payload.exe").is_err());
    assert!(validate_shell_open_target("ms-msdt:/id Test").is_err());

    // BH-12 & lyrics parser resilience on malformed/garbage input
    assert!(parse_lrclib_response("not json at all").is_empty());
    assert!(parse_lrclib_response("{}").is_empty());
    assert!(parse_lrc("[bad:time] text\n[99:99.99] ok\nno bracket").len() == 1);

    // BH-22 WAV parser resilience on malformed/truncated inputs
    assert_eq!(find_data_chunk(&[]), None);
    assert_eq!(find_data_chunk(b"RIFF\x00\x00\x00\x00WAVE"), None);
    let garbage = b"RIFF\xff\xff\xff\xffWAVEJUNK\xff\xff\xff\xff";
    assert_eq!(find_data_chunk(garbage), None);
    assert_eq!(scale_wav_volume(garbage, 0.5), garbage);

    // Rapid settings/preset/placement/config stress without panics or degenerate dimensions
    let mut st = AppState::new_for_preview();
    for placement in [
        NotchPlacement::Top,
        NotchPlacement::Left,
        NotchPlacement::Right,
    ] {
        for style in [NotchStyle::Floating, NotchStyle::EdgeFlush] {
            for preset in [
                NotchSizePreset::Compact,
                NotchSizePreset::Standard,
                NotchSizePreset::Wide,
            ] {
                for exp in [
                    ExpandedSizePreset::Compact,
                    ExpandedSizePreset::Standard,
                    ExpandedSizePreset::Large,
                ] {
                    st.apply_setting_action(Action::SetPlacement(placement));
                    st.apply_setting_action(Action::SetNotchStyle(style));
                    st.apply_setting_action(Action::SetSizePreset(preset));
                    st.apply_setting_action(Action::SetExpandedSize(exp));
                    assert!(st.idle_width() > 0.0 && st.idle_height() > 0.0);
                    assert!(st.expanded_width() > 0.0 && st.expanded_height() > 0.0);
                }
            }
        }
    }

    // Extreme / malformed JSON config clamping
    assert!(!st.apply_config_json("{ broken"));
    let mut cfg_val: serde_json::Value = serde_json::from_str(&st.to_config_json()).unwrap();
    cfg_val["notification_secs"] = serde_json::json!(999);
    cfg_val["notification_volume"] = serde_json::json!(255);
    cfg_val["frosted_opacity_pct"] = serde_json::json!(1);
    cfg_val["frosted_blur_px"] = serde_json::json!(500);
    assert!(st.apply_config_json(&cfg_val.to_string()));
    assert_eq!(st.notification_secs, 10);
    assert_eq!(st.notification_volume, 100);
    assert_eq!(st.frosted_opacity_pct, 25);
    assert_eq!(st.frosted_blur_px, 40);
}
