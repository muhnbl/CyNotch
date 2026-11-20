use crate::localtime::{format_capture_stamp, local_now};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use windows::Win32::Foundation::{GENERIC_WRITE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
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

unsafe fn copy_bgra_dib_to_clipboard(w: i32, h: i32, bgra_top_down: &[u8]) -> bool {
    let _clip_guard = CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let total_size = hdr_size + bgra_top_down.len();
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

const BLOCKED_SHELL_EXTENSIONS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
    "scr", "pif", "msi", "msp", "hta", "cpl", "msc", "reg", "lnk", "url", "jar", "inf", "scf",
    "dll", "sys", "drv", "ocx",
];

/// Explicit security policy for `ShellExecuteW("open", ...)` targets originating from
/// serialized shelf data or drag-and-drop payloads (BH-14):
/// - Rejects empty strings, control characters, and UNC/network paths (`\\host\...`, `//host/...`, `\\?\UNC\...`).
/// - Permits only `http://` and `https://` URI schemes (blocks `ms-msdt:`, `search-ms:`, `file:`, etc.).
/// - For local filesystem targets, requires an existing local file/directory whose extension
///   is not an executable, script, or shell shortcut.
pub fn validate_shell_open_target(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control()) {
        return Err("empty or control characters in target");
    }

    let lower = trimmed.to_ascii_lowercase();
    // Block UNC and extended-UNC network paths to prevent NTLM leaks & remote execution.
    if trimmed.starts_with(r"\\") || trimmed.starts_with("//") || lower.starts_with(r"\\?\unc\") {
        return Err("UNC network paths are blocked");
    }

    // Detect URI scheme (anything with `:` other than Windows drive letter like `C:\...` or `C:/...`).
    if let Some(colon_idx) = trimmed.find(':') {
        let is_drive_letter = colon_idx == 1
            && trimmed
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic())
            && trimmed
                .as_bytes()
                .get(2)
                .is_some_and(|&b| b == b'\\' || b == b'/');
        if !is_drive_letter {
            if lower.starts_with("https://") || lower.starts_with("http://") {
                return Ok(trimmed.to_string());
            }
            return Err("URI scheme is not permitted");
        }
    }

    let path = std::path::Path::new(trimmed);
    if !path.exists() {
        return Err("local target does not exist");
    }
    if path.is_file()
        && let Some(ext) = path.extension().and_then(|e| e.to_str())
        && BLOCKED_SHELL_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    {
        return Err("executable or script file type is blocked");
    }
    Ok(trimmed.to_string())
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
    fn bh14_blocks_unc_dangerous_schemes_and_executables() {
        assert!(validate_shell_open_target("").is_err());
        assert!(validate_shell_open_target(r"\\attacker\share\doc.pdf").is_err());
        assert!(validate_shell_open_target("//attacker/share/doc.pdf").is_err());
        assert!(validate_shell_open_target(r"\\?\UNC\attacker\share\a.txt").is_err());
        assert!(validate_shell_open_target("ms-msdt:/id PCWDiagnostic").is_err());
        assert!(validate_shell_open_target("search-ms:query=test").is_err());
        assert!(validate_shell_open_target("file:///C:/Windows/System32/cmd.exe").is_err());
        assert!(validate_shell_open_target("javascript:alert(1)").is_err());
        assert!(validate_shell_open_target("https://example.com/docs").is_ok());
        assert!(validate_shell_open_target("http://localhost:8080/").is_ok());

        let dir = std::env::temp_dir().join(format!("cynotch_shell_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let safe_txt = dir.join("notes.txt");
        let evil_bat = dir.join("run.bat");
        let evil_lnk = dir.join("shortcut.lnk");
        std::fs::write(&safe_txt, "hello").unwrap();
        std::fs::write(&evil_bat, "@echo hi").unwrap();
        std::fs::write(&evil_lnk, "fake").unwrap();

        assert!(validate_shell_open_target(safe_txt.to_str().unwrap()).is_ok());
        assert!(validate_shell_open_target(dir.to_str().unwrap()).is_ok());
        assert!(validate_shell_open_target(evil_bat.to_str().unwrap()).is_err());
        assert!(validate_shell_open_target(evil_lnk.to_str().unwrap()).is_err());
        assert!(
            validate_shell_open_target(dir.join("nonexistent.txt").to_str().unwrap()).is_err()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
