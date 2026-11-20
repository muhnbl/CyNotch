#![allow(unsafe_op_in_unsafe_fn)]

use cynotch::capture::validate_shell_open_target;
use cynotch::composition::{DCompHost, release_thread_locals};
use cynotch::font_d2d::{DWriteTextCache, Weight};
use cynotch::lyrics::{parse_lrc, parse_lrclib_response};
use cynotch::models::{
    Action, AppState, ExpandedSizePreset, NotchPlacement, NotchSizePreset, NotchStyle,
    NotificationKind, TaskItem,
};
use cynotch::painter_d2d::{D2DPainter, D2DResourceCache, PainterD2D, rgba};
use cynotch::persistence::{atomic_write_json, load_json_with_recovery};
use cynotch::sound::{find_data_chunk, scale_wav_volume};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
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
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{Interface, w};

#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessHandleCount(h_process: *mut c_void, pdw_handle_count: *mut u32) -> i32;
    fn K32GetProcessMemoryInfo(
        process: *mut c_void,
        ppsmem_counters: *mut ProcessMemoryCounters,
        cb: u32,
    ) -> i32;
}

fn sample_process_metrics() -> (u32, usize) {
    unsafe {
        let proc_h = GetCurrentProcess();
        let mut handles: u32 = 0;
        let _ = GetProcessHandleCount(proc_h, &mut handles);
        let mut pmc = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        let _ = K32GetProcessMemoryInfo(proc_h, &mut pmc, pmc.cb);
        (handles, pmc.working_set_size / 1024)
    }
}

struct ComStaGuard(bool);

impl Drop for ComStaGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

unsafe extern "system" fn dummy_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

struct HiddenTestHwnd(HWND);

impl HiddenTestHwnd {
    fn create() -> Self {
        unsafe {
            let h_inst = GetModuleHandleW(None).unwrap();
            let class_name = w!("CyNotchSoakTestHiddenWnd");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(dummy_wnd_proc),
                hInstance: h_inst.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("CyNotchSoakTest"),
                WS_POPUP,
                -2000,
                -2000,
                160,
                64,
                None,
                None,
                h_inst,
                None,
            )
            .expect("CreateWindowExW hidden test HWND");
            Self(hwnd)
        }
    }
}

impl Drop for HiddenTestHwnd {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

fn create_warp_d2d_target(w: u32, h: u32) -> (ID2D1Factory1, ID2D1DeviceContext, ID2D1Bitmap1) {
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

fn read_pixel_alpha(
    ctx: &ID2D1DeviceContext,
    target: &ID2D1Bitmap1,
    w: u32,
    h: u32,
    px_x: usize,
    px_y: usize,
) -> u8 {
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
        let row_ptr = mapped.bits.add(px_y * mapped.pitch as usize);
        let alpha = *row_ptr.add(px_x * 4 + 3);
        staging.Unmap().unwrap();
        alpha
    }
}

#[test]
fn bh33_dcomp_same_hwnd_drop_and_recreate_recovery() {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _com = ComStaGuard(hr.is_ok());
    let wnd = HiddenTestHwnd::create();

    for cycle in 0..5u32 {
        let mut host = DCompHost::new(wnd.0, 200 + cycle * 10, 64).unwrap_or_else(|e| {
            panic!("DCompHost::new failed on recovery cycle {}: {:?}", cycle, e)
        });
        let _ctx = host.begin_d2d_draw().expect("begin_d2d_draw");
        host.end_d2d_draw_and_present()
            .expect("end_d2d_draw_and_present");
        host.rebind_for_dpi_change(240 + cycle * 10, 72)
            .expect("rebind_for_dpi_change");
        // Explicitly drop host to trigger DesktopWindowTarget::Close() before next cycle
        drop(host);
    }

    release_thread_locals();
}

#[test]
fn bh20_bh36_single_global_alpha_and_nan_guards() {
    let (_f, ctx, target) = create_warp_d2d_target(128, 128);
    let mut cache = D2DResourceCache::new(&ctx).unwrap();
    let mut fonts = DWriteTextCache::new().unwrap();
    let mut hits = Vec::new();
    let hover_slots = [0.0f32; 48];

    // 1. Verify PainterD2D::rrect applies global_alpha = 0.5 ONCE (~128, NOT ~64 ga^2)
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
        let mut p = PainterD2D::new(
            &ctx,
            &mut cache,
            &mut fonts,
            1.0,
            0.0,
            0.0,
            None,
            &mut hits,
            false,
            0.5,
            hover_slots,
            false,
            false,
            0,
            cynotch::models::TypeScale::Standard,
        );
        p.rrect(16.0, 16.0, 96.0, 96.0, 4.0, rgba(255, 255, 255, 255));
    }
    unsafe { ctx.EndDraw(None, None) }.unwrap();
    let a_painter = read_pixel_alpha(&ctx, &target, 128, 128, 64, 64);
    assert!(
        (120..=136).contains(&a_painter),
        "PainterD2D::rrect at global_alpha=0.5 must have alpha ~128 (1x), got {}",
        a_painter
    );

    // 2. Verify D2DPainter high-level methods (dot_matrix, arc NaN/zero guards) apply 1x alpha
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
        let mut dp = D2DPainter::new(&ctx, &mut cache, 1.0, 0.0, 0.0, 0.5);
        dp.dot_matrix(64.0, 64.0, 10.0, 10.0, 10.0, 8.0, rgba(255, 255, 255, 255))
            .unwrap();
        // Zero/negative spacing must return immediately without infinite loop
        dp.dot_matrix(0.0, 0.0, 10.0, 10.0, 0.0, 2.0, rgba(255, 255, 255, 255))
            .unwrap();
        dp.dot_matrix(0.0, 0.0, 10.0, 10.0, -2.0, 2.0, rgba(255, 255, 255, 255))
            .unwrap();
        // NaN / non-positive arc inputs must safely no-op without D2D error
        dp.arc(
            64.0,
            64.0,
            20.0,
            0.0,
            f32::NAN,
            3.0,
            rgba(255, 255, 255, 255).to_d2d(1.0),
        )
        .unwrap();
        dp.arc(
            64.0,
            64.0,
            -5.0,
            0.0,
            1.5,
            3.0,
            rgba(255, 255, 255, 255).to_d2d(1.0),
        )
        .unwrap();
        dp.arc(
            64.0,
            64.0,
            20.0,
            0.0,
            1.5,
            0.0,
            rgba(255, 255, 255, 255).to_d2d(1.0),
        )
        .unwrap();
    }
    unsafe { ctx.EndDraw(None, None) }.unwrap();
    let a_dot = read_pixel_alpha(&ctx, &target, 128, 128, 64, 64);
    assert!(
        (120..=136).contains(&a_dot),
        "D2DPainter::dot_matrix at global_alpha=0.5 must have alpha ~128 (1x), got {}",
        a_dot
    );
}

#[test]
fn phase6_resource_and_performance_soak() {
    static SOAK_SEQ: AtomicU64 = AtomicU64::new(1);
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _com = ComStaGuard(hr.is_ok());

    let soak_dir: PathBuf = std::env::temp_dir().join(format!(
        "cynotch_soak_{}_{}",
        std::process::id(),
        SOAK_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&soak_dir).unwrap();

    // Warm up COM/DWrite/DComp once before capturing baseline metrics
    let wnd = HiddenTestHwnd::create();
    {
        let mut warm = DCompHost::new(wnd.0, 200, 64).unwrap();
        let _ = warm.begin_d2d_draw().unwrap();
        warm.end_d2d_draw_and_present().unwrap();
    }

    let t0 = Instant::now();
    let (h_init, mem_init_kb) = sample_process_metrics();
    let mut h_peak = h_init;
    let mut mem_peak_kb = mem_init_kb;

    // 1. Persistence soak: 150 atomic JSON writes + reads
    let cfg_path = soak_dir.join("soak_tasks.json");
    for i in 0..150usize {
        let tasks = vec![TaskItem {
            text: format!("Soak item #{i}\tcol\nline2"),
            completed: i % 2 == 0,
        }];
        atomic_write_json(&cfg_path, &tasks, true).unwrap();
        if i % 30 == 0 {
            assert!(
                load_json_with_recovery::<Vec<TaskItem>, _>(&cfg_path, |_| true)
                    .into_option()
                    .is_some()
            );
        }
    }
    let (h1, m1) = sample_process_metrics();
    h_peak = h_peak.max(h1);
    mem_peak_kb = mem_peak_kb.max(m1);

    // 2. D2D + DWrite cache soak: 300 measure/draw/trim iterations
    {
        let (_f, ctx, _target) = create_warp_d2d_target(256, 128);
        let mut cache = D2DResourceCache::new(&ctx).unwrap();
        let mut fonts = DWriteTextCache::new().unwrap();
        for i in 0..300u32 {
            let sz = 10.0 + ((i % 80) as f32) * 0.25;
            let _ = fonts.measure("CyNotch Soak Test 🎵", sz, Weight::Semibold);
            if i % 50 == 49 {
                fonts.trim();
            }
            unsafe { ctx.BeginDraw() };
            {
                let mut dp = D2DPainter::new(&ctx, &mut cache, 1.0, 0.0, 0.0, 0.75);
                dp.arc(
                    64.0,
                    64.0,
                    24.0,
                    0.0,
                    std::f32::consts::TAU,
                    3.0,
                    rgba(168, 85, 247, 255).to_d2d(1.0),
                )
                .unwrap();
            }
            unsafe { ctx.EndDraw(None, None) }.unwrap();
        }
    }
    let (h2, m2) = sample_process_metrics();
    h_peak = h_peak.max(h2);
    mem_peak_kb = mem_peak_kb.max(m2);

    // 3. DCompHost resize + DPI rebind + drop/recreate soak
    {
        let mut host = DCompHost::new(wnd.0, 220, 64).unwrap();
        for i in 0..60u32 {
            let w = 180 + (i % 6) * 40;
            let h = 48 + (i % 4) * 24;
            if i % 10 == 0 {
                host.rebind_for_dpi_change(w, h).unwrap();
            } else {
                host.resize(w, h).unwrap();
            }
            let _ = host.begin_d2d_draw().unwrap();
            host.end_d2d_draw_and_present().unwrap();
        }
    }
    release_thread_locals();
    let (h3, m3) = sample_process_metrics();
    h_peak = h_peak.max(h3);
    mem_peak_kb = mem_peak_kb.max(m3);

    // 4. State + malformed/adversarial input fuzz soak (500 cycles)
    let mut st = AppState::new_for_preview();
    for i in 0..500usize {
        let p = [
            NotchPlacement::Top,
            NotchPlacement::Left,
            NotchPlacement::Right,
        ][i % 3];
        let s = [NotchStyle::Floating, NotchStyle::EdgeFlush][i % 2];
        let sz = [
            NotchSizePreset::Compact,
            NotchSizePreset::Standard,
            NotchSizePreset::Wide,
        ][i % 3];
        let ex = [
            ExpandedSizePreset::Compact,
            ExpandedSizePreset::Standard,
            ExpandedSizePreset::Large,
        ][i % 3];
        st.apply_setting_action(Action::SetPlacement(p));
        st.apply_setting_action(Action::SetNotchStyle(s));
        st.apply_setting_action(Action::SetSizePreset(sz));
        st.apply_setting_action(Action::SetExpandedSize(ex));
        if i % 100 == 0 {
            st.set_notification(
                format!("Soak #{i}"),
                "Body".to_string(),
                NotificationKind::General,
                true,
            );
            st.apply_setting_action(Action::ResetAllSettings);
        }
        assert!(validate_shell_open_target(r"\/evil\share\a.txt").is_err());
        assert!(parse_lrc("[NaN:00] x\n[-5:00] y").is_empty());
        assert!(parse_lrclib_response("[[[[[broken").is_empty());
        let bad_wav = [
            b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'A', b'V', b'E', b'X', b'Y', b'Z', b'W',
            0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(find_data_chunk(&bad_wav), None);
        assert_eq!(scale_wav_volume(&bad_wav, 0.5), bad_wav);
    }

    drop(wnd);
    let _ = std::fs::remove_dir_all(&soak_dir);

    let elapsed_ms = t0.elapsed().as_millis();
    let (h_final, mem_final_kb) = sample_process_metrics();
    h_peak = h_peak.max(h_final);
    mem_peak_kb = mem_peak_kb.max(mem_final_kb);

    eprintln!(
        "[SOAK] duration={}ms | handles: init={} peak={} final={} (delta={}) | working_set_kb: init={} peak={} final={}",
        elapsed_ms,
        h_init,
        h_peak,
        h_final,
        h_final as i64 - h_init as i64,
        mem_init_kb,
        mem_peak_kb,
        mem_final_kb
    );

    // Assert no severe handle leak across 150 writes + 300 D2D/DWrite + 60 DComp cycles + 500 state cycles
    assert!(
        h_final <= h_init + 48,
        "possible handle leak: init={} final={} peak={}",
        h_init,
        h_final,
        h_peak
    );
}
