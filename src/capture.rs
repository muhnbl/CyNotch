use crate::localtime::{format_capture_stamp, local_now};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::{GUID, HSTRING};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDNEXT, GWL_EXSTYLE, GetForegroundWindow, GetSystemMetrics, GetTopWindow, GetWindow,
    GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Serialize all Win32 clipboard operations within this process so concurrent
/// callers (UI thread, background capture thread, parallel tests) never race on
/// `OpenClipboard(NULL)` / `CloseClipboard()` / shared `HGLOBAL` handles.
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(hwnd: HWND) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn GetClipboardData(uformat: u32) -> *mut c_void;
    fn SetClipboardData(uformat: u32, h_mem: *mut c_void) -> *mut c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(uflags: u32, dwbytes: usize) -> *mut c_void;
    fn GlobalLock(h_mem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(h_mem: *mut c_void) -> i32;
    fn GlobalSize(h_mem: *const c_void) -> usize;
    fn GlobalFree(h_mem: *mut c_void) -> *mut c_void;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: HWND,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show_cmd: i32,
    ) -> isize;
}

const CF_DIB: u32 = 8;

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub path: String,
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub clipboard_ok: bool,
}

#[derive(Debug, Clone)]
pub struct GrabbedFrame {
    pub w: i32,
    pub h: i32,
    pub bgra: Vec<u8>,
    pub prefix: String,
}

pub fn pictures_capture_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Pictures").join("CyNotch")
}

pub fn grab_full_desktop_frame() -> Option<GrabbedFrame> {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let (x, y, w, h) = if vw > 0 && vh > 0 {
            (vx, vy, vw, vh)
        } else {
            (
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        };
        grab_rect_frame(x, y, w, h, "desktop")
    }
}

pub fn capture_full_desktop() -> Option<CaptureResult> {
    grab_full_desktop_frame().and_then(save_and_copy_frame)
}

pub fn capture_foreground_window_rect(exclude_hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() && fg != exclude_hwnd && !IsIconic(fg).as_bool() {
            let mut rc = RECT::default();
            if GetWindowRect(fg, &mut rc).is_ok() {
                let w = rc.right - rc.left;
                let h = rc.bottom - rc.top;
                if w > 80 && h > 80 {
                    return Some((rc.left, rc.top, w, h));
                }
            }
        }
        // If CyNotch currently has focus (from clicking Capture Active Window),
        // walk Z-order from top down to find the first visible non-tool app window behind it.
        const WS_EX_TOOLWINDOW_FLAG: i32 = 0x0000_0080;
        let mut cur = GetTopWindow(None).ok();
        let mut steps = 0usize;
        while let Some(h) = cur {
            if h.0.is_null() || steps > 64 {
                break;
            }
            steps += 1;
            if h != exclude_hwnd && IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() {
                let ex = GetWindowLongW(h, GWL_EXSTYLE);
                if (ex & WS_EX_TOOLWINDOW_FLAG) == 0 {
                    let mut rc = RECT::default();
                    if GetWindowRect(h, &mut rc).is_ok() {
                        let w = rc.right - rc.left;
                        let h_px = rc.bottom - rc.top;
                        if w > 120 && h_px > 120 {
                            return Some((rc.left, rc.top, w, h_px));
                        }
                    }
                }
            }
            cur = GetWindow(h, GW_HWNDNEXT).ok();
        }
        None
    }
}

pub fn grab_rect_frame(x: i32, y: i32, w: i32, h: i32, prefix: &str) -> Option<GrabbedFrame> {
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let hdc_screen = GetDC(None);
        if hdc_screen.is_invalid() {
            return None;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.is_invalid() {
            let _ = ReleaseDC(None, hdc_screen);
            return None;
        }
        let h_bmp = CreateCompatibleBitmap(hdc_screen, w, h);
        if h_bmp.is_invalid() {
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);
            return None;
        }
        let old_obj = SelectObject(hdc_mem, h_bmp);
        let blt_ok = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, x, y, SRCCOPY).is_ok();

        let mut grabbed = None;
        if blt_ok {
            let row_bytes = (w as usize) * 4;
            let buf_len = row_bytes * (h as usize);
            let mut bgra = vec![0u8; buf_len];

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down for Pixmap
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: buf_len as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let lines = GetDIBits(
                hdc_mem,
                h_bmp,
                0,
                h as u32,
                Some(bgra.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            if lines > 0 {
                grabbed = Some(GrabbedFrame {
                    w,
                    h,
                    bgra,
                    prefix: prefix.to_string(),
                });
            }
        }

        SelectObject(hdc_mem, old_obj);
        let _ = DeleteObject(h_bmp);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
        grabbed
    }
}

pub fn save_bgra_png(path: &std::path::Path, w: u32, h: u32, bgra: &[u8]) -> bool {
    if w == 0 || h == 0 || bgra.len() < (w as usize) * (h as usize) * 4 {
        return false;
    }
    let mut opaque = bgra[..(w as usize) * (h as usize) * 4].to_vec();
    for px in opaque.chunks_exact_mut(4) {
        px[3] = 255;
    }
    save_bgra_png_alpha(path, w, h, &opaque)
}

/// Saves a straight-alpha 32-bit BGRA buffer to PNG preserving exact per-pixel alpha transparency.
pub fn save_bgra_png_alpha(path: &std::path::Path, w: u32, h: u32, bgra: &[u8]) -> bool {
    if w == 0 || h == 0 || bgra.len() < (w as usize) * (h as usize) * 4 {
        return false;
    }
    let opaque = &bgra[..(w as usize) * (h as usize) * 4];
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(factory): windows::core::Result<IWICImagingFactory> =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
        else {
            return false;
        };
        let Ok(stream) = factory.CreateStream() else {
            return false;
        };
        let wide_path = HSTRING::from(path.as_os_str());
        if stream
            .InitializeFromFilename(&wide_path, GENERIC_WRITE.0)
            .is_err()
        {
            return false;
        }
        let Ok(encoder) =
            factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null::<GUID>())
        else {
            return false;
        };
        if encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .is_err()
        {
            return false;
        }
        let mut frame = None;
        let mut bag = None;
        if encoder.CreateNewFrame(&mut frame, &mut bag).is_err() {
            return false;
        }
        let Some(frame) = frame else {
            return false;
        };
        if frame.Initialize(bag.as_ref()).is_err() {
            return false;
        }
        if frame.SetSize(w, h).is_err() {
            return false;
        }
        let mut fmt = GUID_WICPixelFormat32bppBGRA;
        if frame.SetPixelFormat(&mut fmt).is_err() {
            return false;
        }
        let stride = w * 4;
        if frame.WritePixels(h, stride, opaque).is_err() {
            return false;
        }
        if frame.Commit().is_err() {
            return false;
        }
        encoder.Commit().is_ok()
    }
}

/// Parses `#RRGGBB` or `#RGB` hex color occurrences from SVG source so `.svg` shelf tiles
/// render an instant, color-accurate vector preview thumbnail.
fn extract_svg_hex_colors(svg: &str) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    let bytes = svg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let rem = &bytes[i + 1..];
            let hex_len = rem.iter().take(6).take_while(|b| b.is_ascii_hexdigit()).count();
            if hex_len == 6 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(std::str::from_utf8(&rem[0..2]).unwrap_or("0"), 16),
                    u8::from_str_radix(std::str::from_utf8(&rem[2..4]).unwrap_or("0"), 16),
                    u8::from_str_radix(std::str::from_utf8(&rem[4..6]).unwrap_or("0"), 16),
                ) {
                    let rgb = [r, g, b];
                    if !out.contains(&rgb) {
                        out.push(rgb);
                        if out.len() >= 4 {
                            break;
                        }
                    }
                }
                i += 7;
                continue;
            } else if hex_len >= 3 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(std::str::from_utf8(&rem[0..1]).unwrap_or("0"), 16),
                    u8::from_str_radix(std::str::from_utf8(&rem[1..2]).unwrap_or("0"), 16),
                    u8::from_str_radix(std::str::from_utf8(&rem[2..3]).unwrap_or("0"), 16),
                ) {
                    let rgb = [r * 17, g * 17, b * 17];
                    if !out.contains(&rgb) {
                        out.push(rgb);
                        if out.len() >= 4 {
                            break;
                        }
                    }
                }
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn render_svg_thumbnail_pbgra(path: &std::path::Path, max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    let content = std::fs::read_to_string(path).ok()?;
    if !content.contains("<svg") && !content.contains("<SVG") {
        return None;
    }
    let colors = extract_svg_hex_colors(&content);
    let c0 = colors.first().copied().unwrap_or([124, 58, 237]);
    let c1 = colors.get(1).copied().unwrap_or([248, 182, 16]);
    let c2 = colors.get(2).copied().unwrap_or([233, 218, 255]);

    let w = max_dim.clamp(64, 260);
    let h = ((w as f32 * 0.65).round() as u32).max(48);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let wf = w as f32;
    let hf = h as f32;

    for y in 0..h {
        let vf = y as f32 / hf;
        for x in 0..w {
            let uf = x as f32 / wf;
            // Subtle dark obsidian vector artboard with soft radial glow from SVG palette
            let dx1 = uf - 0.34;
            let dy1 = vf - 0.46;
            let d1 = (dx1 * dx1 + dy1 * dy1).sqrt();
            let g1 = (1.0 - d1 * 2.1).clamp(0.0, 1.0);

            let dx2 = uf - 0.68;
            let dy2 = vf - 0.54;
            let d2 = (dx2 * dx2 + dy2 * dy2).sqrt();
            let g2 = (1.0 - d2 * 2.4).clamp(0.0, 1.0);

            // Vector bezier wave ribbon across center
            let wave_y = 0.50 + 0.14 * (uf * 5.2).sin();
            let dist_wave = (vf - wave_y).abs();
            let ribbon = (1.0 - (dist_wave / 0.045)).clamp(0.0, 1.0);

            // Artboard grid dots every 16px
            let grid = if (x % 16 == 0) && (y % 16 == 0) { 0.14 } else { 0.0 };

            let r = (20.0
                + c0[0] as f32 * g1 * 0.62
                + c1[0] as f32 * g2 * 0.52
                + c2[0] as f32 * ribbon * 0.75
                + 255.0 * grid)
                .clamp(0.0, 255.0) as u8;
            let g = (16.0
                + c0[1] as f32 * g1 * 0.62
                + c1[1] as f32 * g2 * 0.52
                + c2[1] as f32 * ribbon * 0.75
                + 255.0 * grid)
                .clamp(0.0, 255.0) as u8;
            let b = (36.0
                + c0[2] as f32 * g1 * 0.62
                + c1[2] as f32 * g2 * 0.52
                + c2[2] as f32 * ribbon * 0.75
                + 255.0 * grid)
                .clamp(0.0, 255.0) as u8;

            let idx = ((y * w + x) * 4) as usize;
            buf[idx] = b;
            buf[idx + 1] = g;
            buf[idx + 2] = r;
            buf[idx + 3] = 255;
        }
    }
    Some((w, h, buf))
}

/// Decodes any local image file (`png`, `jpg`, `jpeg`, `webp`, `bmp`, `gif`, `ico`, `tif`, `tiff`, `svg`)
/// into a downscaled premultiplied 32-bit `PBGRA` thumbnail buffer (`(width, height, bytes)`).
pub fn load_image_thumbnail_pbgra(raw_path: &str, max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = std::path::Path::new(trimmed);
    if !path.is_file() {
        return None;
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        return render_svg_thumbnail_pbgra(path, max_dim);
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = factory.CreateStream().ok()?;
        let wide_path = HSTRING::from(path.as_os_str());
        stream
            .InitializeFromFilename(&wide_path, GENERIC_READ.0)
            .ok()?;
        let decoder = factory
            .CreateDecoderFromStream(
                &stream,
                std::ptr::null::<GUID>(),
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let mut src_w = 0u32;
        let mut src_h = 0u32;
        frame.GetSize(&mut src_w, &mut src_h).ok()?;
        if src_w == 0 || src_h == 0 {
            return None;
        }

        let cap = max_dim.clamp(32, 512);
        let (dst_w, dst_h) = if src_w <= cap && src_h <= cap {
            (src_w, src_h)
        } else if src_w >= src_h {
            let dw = cap;
            let dh = ((src_h as u64 * cap as u64) / (src_w as u64)).max(1) as u32;
            (dw, dh)
        } else {
            let dh = cap;
            let dw = ((src_w as u64 * cap as u64) / (src_h as u64)).max(1) as u32;
            (dw, dh)
        };

        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let source: IWICBitmapSource = if dst_w != src_w || dst_h != src_h {
            let scaler = factory.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&converter, dst_w, dst_h, WICBitmapInterpolationModeFant)
                .ok()?;
            scaler.into()
        } else {
            converter.into()
        };

        let stride = dst_w.checked_mul(4)?;
        let buf_len = (stride as usize).checked_mul(dst_h as usize)?;
        let mut pbgra = vec![0u8; buf_len];
        source
            .CopyPixels(std::ptr::null(), stride, &mut pbgra)
            .ok()?;
        Some((dst_w, dst_h, pbgra))
    }
}

pub fn save_frame(frame: GrabbedFrame, copy_to_clipboard: bool) -> Option<CaptureResult> {
    let w = frame.w;
    let h = frame.h;
    if w <= 0 || h <= 0 {
        return None;
    }

    let dir = pictures_capture_dir();
    let _ = std::fs::create_dir_all(&dir);
    let stamp = format_capture_stamp(local_now());
    let filename = format!("cyshot_{}_{}.png", frame.prefix, stamp);
    let full_path = dir.join(&filename);

    if save_bgra_png(&full_path, w as u32, h as u32, &frame.bgra) {
        let clip_ok = if copy_to_clipboard {
            unsafe { copy_bgra_dib_to_clipboard(w, h, &frame.bgra) }
        } else {
            false
        };
        Some(CaptureResult {
            path: full_path.to_string_lossy().to_string(),
            filename,
            width: w as u32,
            height: h as u32,
            clipboard_ok: clip_ok,
        })
    } else {
        None
    }
}

pub fn save_and_copy_frame(frame: GrabbedFrame) -> Option<CaptureResult> {
    save_frame(frame, true)
}

pub fn capture_rect(x: i32, y: i32, w: i32, h: i32, prefix: &str) -> Option<CaptureResult> {
    grab_rect_frame(x, y, w, h, prefix).and_then(save_and_copy_frame)
}

/// Validates DIB dimensions and pixel buffer length against integer overflow
/// and out-of-bounds reads before allocating/copying into `GlobalAlloc` memory (BH-29).
fn validate_dib_buffer(w: i32, h: i32, bgra_len: usize) -> Option<(usize, usize, usize)> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let width = w as usize;
    let height = h as usize;
    let row_bytes = width.checked_mul(4)?;
    let pixel_bytes = row_bytes.checked_mul(height)?;
    if bgra_len < pixel_bytes || pixel_bytes > u32::MAX as usize {
        return None;
    }
    let total_size = std::mem::size_of::<BITMAPINFOHEADER>().checked_add(pixel_bytes)?;
    Some((row_bytes, pixel_bytes, total_size))
}

unsafe fn copy_bgra_dib_to_clipboard(w: i32, h: i32, bgra_top_down: &[u8]) -> bool {
    let Some((row_bytes, pixel_bytes, total_size)) = validate_dib_buffer(w, h, bgra_top_down.len())
    else {
        return false;
    };
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    unsafe {
        let h_mem = GlobalAlloc(0x0002, total_size);
        if h_mem.is_null() {
            return false;
        }
        let ptr = GlobalLock(h_mem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(h_mem);
            return false;
        }

        // Bottom-up positive height DIB is universally accepted across Paint, Discord, WhatsApp, Edge, Word
        let hdr = BITMAPINFOHEADER {
            biSize: hdr_size as u32,
            biWidth: w,
            biHeight: h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: pixel_bytes as u32,
            ..Default::default()
        };
        std::ptr::copy_nonoverlapping(&hdr as *const BITMAPINFOHEADER as *const u8, ptr, hdr_size);

        let dst_pixels = ptr.add(hdr_size);
        for row in 0..(h as usize) {
            let src_row = (h as usize - 1 - row) * row_bytes;
            std::ptr::copy_nonoverlapping(
                bgra_top_down.as_ptr().add(src_row),
                dst_pixels.add(row * row_bytes),
                row_bytes,
            );
        }
        let _ = GlobalUnlock(h_mem);

        for attempt in 0..3 {
            if OpenClipboard(HWND::default()) != 0 {
                let _ = EmptyClipboard();
                let placed = SetClipboardData(CF_DIB, h_mem);
                let _ = CloseClipboard();
                if !placed.is_null() {
                    return true;
                }
                let _ = GlobalFree(h_mem);
                return false;
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let _ = GlobalFree(h_mem);
        false
    }
}

pub fn copy_text_to_clipboard(owner: HWND, text: &str) -> bool {
    const CF_UNICODETEXT: u32 = 13;
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = utf16.len() * 2;
        let h_mem = GlobalAlloc(0x0002, bytes);
        if h_mem.is_null() {
            return false;
        }
        let ptr = GlobalLock(h_mem);
        if ptr.is_null() {
            let _ = GlobalFree(h_mem);
            return false;
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, ptr as *mut u8, bytes);
        let _ = GlobalUnlock(h_mem);

        for attempt in 0..3 {
            if OpenClipboard(owner) != 0 {
                let _ = EmptyClipboard();
                let placed = SetClipboardData(CF_UNICODETEXT, h_mem);
                let _ = CloseClipboard();
                if !placed.is_null() {
                    return true;
                }
                let _ = GlobalFree(h_mem);
                return false;
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let _ = GlobalFree(h_mem);
        false
    }
}

pub fn read_clipboard_text(owner: HWND) -> Option<String> {
    const CF_UNICODETEXT: u32 = 13;
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        for attempt in 0..3 {
            if OpenClipboard(owner) != 0 {
                let mut out = None;
                let h_data = GetClipboardData(CF_UNICODETEXT);
                if !h_data.is_null() {
                    let byte_size = GlobalSize(h_data);
                    let max_bytes = byte_size.min(4096 * 2);
                    if max_bytes >= 2 {
                        let ptr = GlobalLock(h_data);
                        if !ptr.is_null() {
                            let slice_u8 = std::slice::from_raw_parts(ptr as *const u8, max_bytes);
                            let mut utf16 = Vec::with_capacity(max_bytes / 2);
                            for pair in slice_u8.as_chunks::<2>().0 {
                                let code = u16::from_le_bytes([pair[0], pair[1]]);
                                if code == 0 {
                                    break;
                                }
                                utf16.push(code);
                            }
                            let _ = GlobalUnlock(h_data);
                            let text = String::from_utf16_lossy(&utf16);
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                out = Some(trimmed.to_string());
                            }
                        }
                    }
                }
                let _ = CloseClipboard();
                return out;
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        None
    }
}

/// Returns true if the Windows clipboard currently holds a bitmap/screenshot (`CF_DIB` or `CF_BITMAP`).
pub fn has_clipboard_image() -> bool {
    const CF_BITMAP_FMT: u32 = 2;
    const CF_DIB_FMT: u32 = 8;
    unsafe {
        IsClipboardFormatAvailable(CF_DIB_FMT) != 0
            || IsClipboardFormatAvailable(CF_BITMAP_FMT) != 0
    }
}

/// Reads a copied image or Snipping Tool screenshot (`CF_DIB`) from the Windows clipboard,
/// saves it as a cached PNG under `%LOCALAPPDATA%\CyNotch\shelf-cache\`, and returns `(filename, full_path)`.
pub fn read_clipboard_dib_to_shelf(owner: HWND) -> Option<(String, String)> {
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        if IsClipboardFormatAvailable(CF_DIB) == 0 {
            return None;
        }
        let mut extracted: Option<(u32, u32, Vec<u8>)> = None;
        for attempt in 0..3 {
            if OpenClipboard(owner) != 0 {
                let h_data = GetClipboardData(CF_DIB);
                if !h_data.is_null() {
                    let byte_size = GlobalSize(h_data);
                    let hdr_min = std::mem::size_of::<BITMAPINFOHEADER>();
                    if byte_size > hdr_min && byte_size <= 64 * 1024 * 1024 {
                        let ptr = GlobalLock(h_data) as *const u8;
                        if !ptr.is_null() {
                            let hdr = std::ptr::read_unaligned(ptr as *const BITMAPINFOHEADER);
                            let w = hdr.biWidth;
                            let h_abs = hdr.biHeight.unsigned_abs() as i32;
                            let bottom_up = hdr.biHeight > 0;
                            let bpp = hdr.biBitCount;
                            // BI_RGB = 0, BI_BITFIELDS = 3
                            if (1..=8192).contains(&w)
                                && (1..=8192).contains(&h_abs)
                                && (bpp == 32 || bpp == 24)
                                && (hdr.biCompression == 0 || hdr.biCompression == 3)
                            {
                                let extra_masks = if hdr.biCompression == 3 && hdr.biSize == 40 {
                                    12usize
                                } else {
                                    0usize
                                };
                                let pixel_offset = (hdr.biSize as usize).saturating_add(extra_masks);
                                let src_stride = (((w as usize) * (bpp as usize) + 31) / 32) * 4;
                                let needed = pixel_offset
                                    .saturating_add(src_stride.saturating_mul(h_abs as usize));
                                if needed <= byte_size {
                                    let src_pixels =
                                        std::slice::from_raw_parts(ptr.add(pixel_offset), byte_size - pixel_offset);
                                    let dst_stride = (w as usize) * 4;
                                    let mut bgra = vec![0u8; dst_stride * (h_abs as usize)];
                                    for row in 0..(h_abs as usize) {
                                        let src_row = if bottom_up {
                                            (h_abs as usize - 1 - row) * src_stride
                                        } else {
                                            row * src_stride
                                        };
                                        let dst_row = row * dst_stride;
                                        if bpp == 32 {
                                            bgra[dst_row..dst_row + dst_stride].copy_from_slice(
                                                &src_pixels[src_row..src_row + dst_stride],
                                            );
                                        } else {
                                            for col in 0..(w as usize) {
                                                let s = src_row + col * 3;
                                                let d = dst_row + col * 4;
                                                bgra[d] = src_pixels[s];
                                                bgra[d + 1] = src_pixels[s + 1];
                                                bgra[d + 2] = src_pixels[s + 2];
                                                bgra[d + 3] = 255;
                                            }
                                        }
                                    }
                                    extracted = Some((w as u32, h_abs as u32, bgra));
                                }
                            }
                            let _ = GlobalUnlock(h_data);
                        }
                    }
                }
                let _ = CloseClipboard();
                break;
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let (w, h, bgra) = extracted?;
        let dir = crate::models::appdata_dir().join("shelf-cache");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = format_capture_stamp(local_now());
        let filename = format!("snip_{}.png", stamp);
        let full_path = dir.join(&filename);
        if save_bgra_png(&full_path, w, h, &bgra) {
            Some((filename, full_path.to_string_lossy().into_owned()))
        } else {
            None
        }
    }
}

const BLOCKED_SHELL_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
    "scr", "pif", "msi", "msp", "hta", "cpl", "msc", "reg", "lnk", "url", "jar", "inf", "scf",
    "dll", "sys", "drv", "ocx", "chm", "iso", "img", "vhd", "vhdx", "appref-ms", "application",
    "msix", "appx", "appinstaller", "diagcab", "settingcontent-ms", "library-ms",
    "searchconnector-ms", "py", "pyw", "jnlp", "sct", "wsc", "xll", "docm", "xlsm", "pptm",
];

fn has_blocked_extension(path: &std::path::Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let clean_name = file_name.trim_end_matches(['.', ' ']);
    let Some(ext) = std::path::Path::new(clean_name)
        .extension()
        .and_then(|e| e.to_str())
    else {
        return false;
    };
    BLOCKED_SHELL_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Explicit security policy for `ShellExecuteW("open", ...)` targets originating from
/// serialized shelf data or drag-and-drop payloads (BH-14, BH-32):
/// - Rejects empty strings, control characters, and ANY leading slash/backslash pair
///   (`\\`, `//`, `\/`, `/\`, `\\?\`, `\\.\`) to block UNC/NTLM leaks and device namespaces.
/// - Permits only well-formed `http://` and `https://` URLs with a non-empty host and no
///   whitespace or quote/shell metacharacters.
/// - For local filesystem targets, requires an absolute path with NO secondary `:` (blocks
///   NTFS Alternate Data Streams such as `safe.txt:evil.exe` and `evil.bat::$DATA`), NO
///   trailing dots/spaces on path segments (blocks Windows `calc.exe.` normalization bypass),
///   canonicalizes via `fs::canonicalize` (resolving symlinks and rejecting canonical UNC),
///   and checks both original and canonical filenames against the executable/script denylist.
pub fn validate_shell_open_target(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control()) {
        return Err("empty or control characters in target");
    }

    let bytes = trimmed.as_bytes();
    // Block UNC, mixed-slash UNC (`\/`, `/\`), extended `\\?\`, and device `\\.\` paths.
    if bytes.len() >= 2
        && (bytes[0] == b'\\' || bytes[0] == b'/')
        && (bytes[1] == b'\\' || bytes[1] == b'/')
    {
        return Err("UNC or device paths are blocked");
    }

    let lower = trimmed.to_ascii_lowercase();
    // Detect URI scheme (anything with `:` other than Windows drive letter like `C:\...` or `C:/...`).
    if let Some(colon_idx) = trimmed.find(':') {
        let is_drive_letter = colon_idx == 1
            && bytes.first().is_some_and(|b| b.is_ascii_alphabetic())
            && bytes.get(2).is_some_and(|&b| b == b'\\' || b == b'/');
        if !is_drive_letter {
            let rest = if lower.starts_with("https://") {
                &trimmed[8..]
            } else if lower.starts_with("http://") {
                &trimmed[7..]
            } else {
                return Err("URI scheme is not permitted");
            };
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            if host.is_empty()
                || trimmed.chars().any(|c| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`')
                })
            {
                return Err("malformed or unsafe HTTP(S) URL");
            }
            return Ok(trimmed.to_string());
        }
        // Reject any second colon after `C:` to block NTFS Alternate Data Streams (`:stream`, `::$DATA`).
        if trimmed[2..].contains(':') {
            return Err("NTFS alternate data streams are blocked");
        }
    }

    // Reject Windows trailing-dot/space path normalization tricks on any segment (`calc.exe.`).
    for seg in trimmed.split(['\\', '/']) {
        if !seg.is_empty() && (seg.ends_with('.') || seg.ends_with(' ')) {
            return Err("trailing dot or space in path segment is blocked");
        }
    }

    let path = std::path::Path::new(trimmed);
    if !path.is_absolute() {
        return Err("relative paths are blocked");
    }
    if has_blocked_extension(path) {
        return Err("executable or script file type is blocked");
    }

    let canonical = std::fs::canonicalize(path).map_err(|_| "local target does not exist")?;
    let canon_str = canonical
        .to_str()
        .ok_or("non-UTF-8 canonical path")?;
    if canon_str.to_ascii_lowercase().starts_with(r"\\?\unc\") {
        return Err("canonical UNC path is blocked");
    }
    let clean_canon = canon_str.strip_prefix(r"\\?\").unwrap_or(canon_str);
    let clean_path = std::path::Path::new(clean_canon);

    if clean_path.is_file() && has_blocked_extension(clean_path) {
        return Err("canonical target is an executable or script");
    }
    Ok(clean_canon.to_string())
}

struct ShellComGuard(bool);

impl Drop for ShellComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

pub fn shell_open_path(path: &str) {
    let Ok(safe_target) = validate_shell_open_target(path) else {
        return;
    };
    std::thread::spawn(move || {
        let wide_op: Vec<u16> = "open\0".encode_utf16().collect();
        let wide_path: Vec<u16> = safe_target
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let hr = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
            let _com = ShellComGuard(hr.is_ok());
            ShellExecuteW(
                HWND::default(),
                wide_op.as_ptr(),
                wide_path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bh14_bh32_blocks_unc_ads_trailing_dots_schemes_and_executables() {
        assert!(validate_shell_open_target("").is_err());
        assert!(validate_shell_open_target(r"\\attacker\share\doc.pdf").is_err());
        assert!(validate_shell_open_target("//attacker/share/doc.pdf").is_err());
        assert!(validate_shell_open_target(r"\/attacker\share\doc.pdf").is_err());
        assert!(validate_shell_open_target(r"/\attacker\share\doc.pdf").is_err());
        assert!(validate_shell_open_target(r"\\?\UNC\attacker\share\a.txt").is_err());
        assert!(validate_shell_open_target(r"\\.\C:\Windows\win.ini").is_err());
        assert!(validate_shell_open_target("ms-msdt:/id PCWDiagnostic").is_err());
        assert!(validate_shell_open_target("search-ms:query=test").is_err());
        assert!(validate_shell_open_target("file:///C:/Windows/System32/cmd.exe").is_err());
        assert!(validate_shell_open_target("javascript:alert(1)").is_err());
        // URL argument smuggling & empty hosts
        assert!(validate_shell_open_target("https://").is_err());
        assert!(validate_shell_open_target("https://example.com\" --arg").is_err());
        assert!(validate_shell_open_target("https://example.com/a b").is_err());
        assert!(validate_shell_open_target("https://example.com/docs").is_ok());
        assert!(validate_shell_open_target("http://localhost:8080/").is_ok());

        let dir = std::env::temp_dir().join(format!("cynotch_shell_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let safe_txt = dir.join("notes.txt");
        let evil_bat = dir.join("run.bat");
        let evil_lnk = dir.join("shortcut.lnk");
        let evil_chm = dir.join("help.chm");
        let evil_iso = dir.join("disk.iso");
        std::fs::write(&safe_txt, "hello").unwrap();
        std::fs::write(&evil_bat, "@echo hi").unwrap();
        std::fs::write(&evil_lnk, "fake").unwrap();
        std::fs::write(&evil_chm, "fake").unwrap();
        std::fs::write(&evil_iso, "fake").unwrap();

        assert!(validate_shell_open_target(safe_txt.to_str().unwrap()).is_ok());
        assert!(validate_shell_open_target(dir.to_str().unwrap()).is_ok());
        assert!(validate_shell_open_target(evil_bat.to_str().unwrap()).is_err());
        assert!(validate_shell_open_target(evil_lnk.to_str().unwrap()).is_err());
        assert!(validate_shell_open_target(evil_chm.to_str().unwrap()).is_err());
        assert!(validate_shell_open_target(evil_iso.to_str().unwrap()).is_err());

        // BH-32: Trailing-dot bypass (`run.bat.`), NTFS ADS (`::$DATA` / `:evil.exe`), relative path
        let trailing_dot_bat = format!("{}.", evil_bat.to_str().unwrap());
        let ads_data_bat = format!("{}::$DATA", evil_bat.to_str().unwrap());
        let ads_stream_txt = format!("{}:evil.exe", safe_txt.to_str().unwrap());
        assert!(validate_shell_open_target(&trailing_dot_bat).is_err());
        assert!(validate_shell_open_target(&ads_data_bat).is_err());
        assert!(validate_shell_open_target(&ads_stream_txt).is_err());
        assert!(validate_shell_open_target("notes.txt").is_err());
        assert!(
            validate_shell_open_target(dir.join("nonexistent.txt").to_str().unwrap()).is_err()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bh29_validate_dib_buffer_rejects_negative_zero_overflow_and_short_buffers() {
        assert!(validate_dib_buffer(0, 10, 400).is_none());
        assert!(validate_dib_buffer(-1, 10, 400).is_none());
        assert!(validate_dib_buffer(10, -5, 400).is_none());
        assert!(validate_dib_buffer(i32::MAX, i32::MAX, usize::MAX).is_none());
        assert!(validate_dib_buffer(10, 10, 399).is_none());
        let (row_b, pix_b, total) = validate_dib_buffer(10, 10, 400).expect("valid 10x10 DIB");
        assert_eq!(row_b, 40);
        assert_eq!(pix_b, 400);
        assert_eq!(total, std::mem::size_of::<BITMAPINFOHEADER>() + 400);
    }
}
