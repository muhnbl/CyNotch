#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use tiny_skia::{ColorU8, Pixmap};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
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

pub fn save_frame(frame: GrabbedFrame, copy_to_clipboard: bool) -> Option<CaptureResult> {
    let w = frame.w;
    let h = frame.h;
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    let pixels = pm.pixels_mut();
    for (dst, src) in pixels.iter_mut().zip(frame.bgra.as_chunks::<4>().0.iter()) {
        *dst = ColorU8::from_rgba(src[2], src[1], src[0], 255).premultiply();
    }

    let dir = pictures_capture_dir();
    let _ = std::fs::create_dir_all(&dir);
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
    let filename = format!("cyshot_{}_{}.png", frame.prefix, stamp);
    let full_path = dir.join(&filename);

    if pm.save_png(&full_path).is_ok() {
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

unsafe fn copy_bgra_dib_to_clipboard(w: i32, h: i32, bgra_top_down: &[u8]) -> bool {
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let total_size = hdr_size + bgra_top_down.len();
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
        biSizeImage: bgra_top_down.len() as u32,
        ..Default::default()
    };
    std::ptr::copy_nonoverlapping(&hdr as *const BITMAPINFOHEADER as *const u8, ptr, hdr_size);

    let row_bytes = (w as usize) * 4;
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

pub fn shell_open_path(path: &str) {
    let path_owned = path.to_string();
    std::thread::spawn(move || {
        let wide_op: Vec<u16> = "open\0".encode_utf16().collect();
        let wide_path: Vec<u16> = path_owned
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
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
