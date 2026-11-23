#![allow(unsafe_op_in_unsafe_fn)]

use crate::localtime::local_now;
use crate::models::{ShelfItem, ShelfKind};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows::Win32::Foundation::{
    BOOL, DV_E_FORMATETC, DV_E_TYMED, E_NOTIMPL, E_UNEXPECTED, HGLOBAL, HWND, POINT, POINTL, S_OK,
};
use windows::Win32::Graphics::Gdi::{
    BI_BITFIELDS, BI_RGB, BITMAPINFOHEADER, ScreenToClient,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, DATADIR_GET, DVASPECT_CONTENT,
    FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
    IStream, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{
    CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK,
    DROPEFFECT_NONE, DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{
    CLSID_DragDropHelper, DragQueryFileW, HDROP, IDragSourceHelper, IDropTargetHelper,
    IShellFolder, SHBindToParent, SHCreateStdEnumFmtEtc, SHParseDisplayName,
};
use windows::core::{HRESULT, HSTRING, Result, implement, w};

const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100);
const DRAGDROP_S_CANCEL: HRESULT = HRESULT(0x0004_0101);
const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = HRESULT(0x0004_0102);

const MAX_CACHE_FILES: usize = 50;
const MAX_CACHE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

static CACHE_SEQ: AtomicU64 = AtomicU64::new(1);
/// True while CyNotch itself is running a synchronous `DoDragDrop` from Shelf.
/// Prevents self-drop duplication even when Shell `IDataObject` rejects custom `SetData`.
pub static IS_DRAGGING_OUT_SHELF: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub enum DroppedPayload {
    File(String),
    Image { label: String, path: String },
    Url(String),
    Text(String),
}

// ── Registered Clipboard Formats ──────────────────────────────────────────────

fn fmt_png() -> u16 {
    static FMT: OnceLock<u16> = OnceLock::new();
    *FMT.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("PNG")) as u16 })
}

fn fmt_inet_url_w() -> u16 {
    static FMT: OnceLock<u16> = OnceLock::new();
    *FMT.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) as u16 })
}

fn fmt_exclude_monitor() -> u32 {
    static FMT: OnceLock<u32> = OnceLock::new();
    *FMT.get_or_init(|| unsafe {
        RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
    })
}

fn fmt_can_include_history() -> u32 {
    static FMT: OnceLock<u32> = OnceLock::new();
    *FMT.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")) })
}

/// Returns `true` when the active clipboard payload is flagged as sensitive by
/// password managers (Bitwarden, 1Password, KeePass, Windows Clipboard History opt-out).
pub fn is_clipboard_sensitive(hwnd: HWND) -> bool {
    unsafe {
        let excl = fmt_exclude_monitor();
        if excl != 0 && IsClipboardFormatAvailable(excl).is_ok() {
            return true;
        }
        let hist = fmt_can_include_history();
        if hist != 0 && IsClipboardFormatAvailable(hist).is_ok()
            && OpenClipboard(hwnd).is_ok() {
                let mut disallowed = false;
                if let Ok(handle) = GetClipboardData(hist) {
                    let hglobal = HGLOBAL(handle.0);
                    let size = GlobalSize(hglobal);
                    if size >= 4 {
                        let ptr = GlobalLock(hglobal) as *const u32;
                        if !ptr.is_null() {
                            disallowed = *ptr == 0;
                            let _ = GlobalUnlock(hglobal);
                        }
                    }
                }
                let _ = CloseClipboard();
                if disallowed {
                    return true;
                }
            }
        false
    }
}

// ── Ephemeral Temp Image Cache ────────────────────────────────────────────────

pub fn shelf_cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("CyNotch").join("shelf-cache")
}

/// Deletes the ephemeral `shelf-cache` folder. Called once at startup (after
/// single-instance mutex succeeds) and on clean exit.
pub fn wipe_shelf_cache() {
    let dir = shelf_cache_dir();
    if std::fs::symlink_metadata(&dir)
        .map(|m| m.file_type().is_dir() && !m.file_type().is_symlink())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn enforce_cache_budget(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut folders: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total_bytes: u64 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = entry.metadata().ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mut size = 0u64;
        if path.is_dir() {
            if let Ok(sub) = std::fs::read_dir(&path) {
                for f in sub.flatten() {
                    size = size.saturating_add(f.metadata().map(|m| m.len()).unwrap_or(0));
                }
            }
        } else {
            size = meta.map(|m| m.len()).unwrap_or(0);
        }
        total_bytes = total_bytes.saturating_add(size);
        folders.push((modified, size, path));
    }
    if folders.len() <= MAX_CACHE_FILES && total_bytes <= MAX_CACHE_BYTES {
        return;
    }
    folders.sort_by_key(|(t, _, _)| *t);
    let mut count = folders.len();
    for (_, sz, p) in folders {
        if count <= MAX_CACHE_FILES && total_bytes <= MAX_CACHE_BYTES {
            break;
        }
        if p.is_dir() {
            let _ = std::fs::remove_dir_all(&p);
        } else {
            let _ = std::fs::remove_file(&p);
        }
        count = count.saturating_sub(1);
        total_bytes = total_bytes.saturating_sub(sz);
    }
}

fn next_friendly_image_path() -> Option<(String, PathBuf)> {
    let root = shelf_cache_dir();
    // Refuse to write through a reparse point planted by another process:
    // the cache root lives under %LOCALAPPDATA% and is wiped on exit.
    if std::fs::symlink_metadata(&root)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return None;
    }
    let _ = std::fs::create_dir_all(&root);
    enforce_cache_budget(&root);
    let seq = CACHE_SEQ.fetch_add(1, Ordering::Relaxed);
    let now = local_now();
    let filename = format!(
        "Image_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
        now.year, now.month, now.day, now.hour, now.minute, now.second
    );
    let sub = root.join(format!("item_{seq}"));
    std::fs::create_dir_all(&sub).ok()?;
    Some((filename.clone(), sub.join(filename)))
}

pub fn save_png_bytes_to_cache(bytes: &[u8]) -> Option<(String, String)> {
    if bytes.len() < 8 || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    // Verify PNG magic signature
    if &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let (label, path) = next_friendly_image_path()?;
    std::fs::write(&path, bytes).ok()?;
    Some((label, path.to_string_lossy().into_owned()))
}

/// Decodes a Windows `CF_DIB` / `CF_DIBV5` memory block (24-bit or 32-bit,
/// `BI_RGB` or `BI_BITFIELDS`, bottom-up or top-down) into a PNG in `shelf-cache`.
pub fn save_dib_to_cache(dib: &[u8]) -> Option<(String, String)> {
    let hdr_min = std::mem::size_of::<BITMAPINFOHEADER>();
    if dib.len() < hdr_min || dib.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let hdr: BITMAPINFOHEADER = unsafe { std::ptr::read_unaligned(dib.as_ptr() as *const _) };
    let header_size = hdr.biSize as usize;
    if header_size < hdr_min || header_size > dib.len() {
        return None;
    }
    let w = hdr.biWidth;
    let h_raw = hdr.biHeight;
    if w <= 0 || h_raw == 0 || w > 8192 || h_raw.abs() > 8192 {
        return None;
    }
    let width = w as usize;
    let height = h_raw.unsigned_abs() as usize;
    let top_down = h_raw < 0;
    let bpp = hdr.biBitCount;
    let compression = hdr.biCompression;
    if bpp != 24 && bpp != 32 {
        return None;
    }
    if compression != BI_RGB.0 && compression != BI_BITFIELDS.0 {
        return None;
    }

    // BI_BITFIELDS with standard 40-byte BITMAPINFOHEADER places 3 DWORD color masks after header.
    let mask_bytes = if compression == BI_BITFIELDS.0 && header_size == hdr_min {
        12usize
    } else {
        0usize
    };
    let palette_bytes = (hdr.biClrUsed as usize) * 4;
    let pixel_offset = header_size.checked_add(mask_bytes)?.checked_add(palette_bytes)?;
    let row_stride = (width * (bpp as usize)).div_ceil(32) * 4;
    let required = pixel_offset.checked_add(row_stride.checked_mul(height)?)?;
    if dib.len() < required {
        return None;
    }

    let pixels = &dib[pixel_offset..required];
    let mut bgra = vec![0u8; width * height * 4];
    let mut any_nonzero_alpha = false;

    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let src_row = &pixels[src_y * row_stride..(src_y + 1) * row_stride];
        let dst_row = &mut bgra[y * width * 4..(y + 1) * width * 4];
        if bpp == 32 {
            for x in 0..width {
                let s = x * 4;
                let a = src_row[s + 3];
                if a != 0 {
                    any_nonzero_alpha = true;
                }
                dst_row[s] = src_row[s];
                dst_row[s + 1] = src_row[s + 1];
                dst_row[s + 2] = src_row[s + 2];
                dst_row[s + 3] = a;
            }
        } else {
            any_nonzero_alpha = true;
            for x in 0..width {
                let s = x * 3;
                let d = x * 4;
                dst_row[d] = src_row[s];
                dst_row[d + 1] = src_row[s + 1];
                dst_row[d + 2] = src_row[s + 2];
                dst_row[d + 3] = 255;
            }
        }
    }

    // Many 32bpp CF_DIB sources leave alpha=0 everywhere; promote to opaque 255 so image isn't invisible.
    if !any_nonzero_alpha {
        for px in bgra.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }

    let (label, path) = next_friendly_image_path()?;
    if crate::capture::save_bgra_png_alpha(&path, width as u32, height as u32, &bgra) {
        Some((label, path.to_string_lossy().into_owned()))
    } else {
        None
    }
}

// ── RAII STGMEDIUM Guard & Format Extraction ──────────────────────────────────

struct StgMediumGuard(STGMEDIUM);

impl Drop for StgMediumGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseStgMedium(&mut self.0);
        }
    }
}

fn make_formatetc(cf: u16, tymed: u32) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed,
    }
}

unsafe fn read_medium_bytes(med: &STGMEDIUM, max_bytes: usize) -> Option<Vec<u8>> {
    if med.tymed == TYMED_HGLOBAL.0 as u32 {
        let hglobal = med.u.hGlobal;
        if hglobal.is_invalid() {
            return None;
        }
        let size = GlobalSize(hglobal);
        if size == 0 || size > max_bytes {
            return None;
        }
        let ptr = GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let data = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hglobal);
        Some(data)
    } else if med.tymed == TYMED_ISTREAM.0 as u32 {
        let stream_opt: &Option<IStream> = &med.u.pstm;
        let stream = stream_opt.as_ref()?;
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut read = 0u32;
            let hr = stream.Read(buf.as_mut_ptr() as *mut _, buf.len() as u32, Some(&mut read));
            if hr.is_err() || read == 0 {
                break;
            }
            if out.len().saturating_add(read as usize) > max_bytes {
                return None;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        if out.is_empty() { None } else { Some(out) }
    } else {
        None
    }
}

unsafe fn read_utf16_z(bytes: &[u8]) -> String {
    let u16s: Vec<u16> = bytes
        .as_chunks::<2>().0.iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&w| w != 0)
        .collect();
    String::from_utf16_lossy(&u16s).trim().to_string()
}

unsafe fn data_object_supports_shelf(obj: &IDataObject) -> bool {
    let candidates = [
        make_formatetc(CF_HDROP.0, TYMED_HGLOBAL.0 as u32),
        make_formatetc(fmt_png(), (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32),
        make_formatetc(CF_DIBV5.0, TYMED_HGLOBAL.0 as u32),
        make_formatetc(CF_DIB.0, TYMED_HGLOBAL.0 as u32),
        make_formatetc(fmt_inet_url_w(), TYMED_HGLOBAL.0 as u32),
        make_formatetc(CF_UNICODETEXT.0, TYMED_HGLOBAL.0 as u32),
    ];
    candidates.iter().any(|f| obj.QueryGetData(f).is_ok())
}

unsafe fn extract_dropped_payloads(obj: &IDataObject) -> Vec<DroppedPayload> {
    // 1. CF_HDROP (files / folders -> individual items, capped at 24)
    let f_hdrop = make_formatetc(CF_HDROP.0, TYMED_HGLOBAL.0 as u32);
    if let Ok(med) = obj.GetData(&f_hdrop) {
        let guard = StgMediumGuard(med);
        if guard.0.tymed == TYMED_HGLOBAL.0 as u32 && !guard.0.u.hGlobal.0.is_null() {
            let hdrop = HDROP(guard.0.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
            if count > 0 {
                let mut files = Vec::with_capacity(count.min(24) as usize);
                for i in 0..count.min(24) {
                    let len = DragQueryFileW(hdrop, i, None);
                    if len > 0 {
                        let mut buf = vec![0u16; (len as usize) + 1];
                        let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
                        if copied > 0 {
                            let path = String::from_utf16_lossy(&buf[..copied as usize]);
                            files.push(DroppedPayload::File(path));
                        }
                    }
                }
                if !files.is_empty() {
                    return files;
                }
            }
        }
    }

    // 2. Registered "PNG" (preserves alpha directly from browsers / Snipping Tool)
    let png_cf = fmt_png();
    if png_cf != 0 {
        let f_png = make_formatetc(png_cf, (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32);
        if let Ok(med) = obj.GetData(&f_png) {
            let guard = StgMediumGuard(med);
            if let Some(bytes) = read_medium_bytes(&guard.0, MAX_IMAGE_BYTES)
                && let Some((label, path)) = save_png_bytes_to_cache(&bytes)
            {
                return vec![DroppedPayload::Image { label, path }];
            }
        }
    }

    // 3. CF_DIBV5 / CF_DIB
    for cf in [CF_DIBV5.0, CF_DIB.0] {
        let f_dib = make_formatetc(cf, TYMED_HGLOBAL.0 as u32);
        if let Ok(med) = obj.GetData(&f_dib) {
            let guard = StgMediumGuard(med);
            if let Some(bytes) = read_medium_bytes(&guard.0, MAX_IMAGE_BYTES)
                && let Some((label, path)) = save_dib_to_cache(&bytes)
            {
                return vec![DroppedPayload::Image { label, path }];
            }
        }
    }

    // 4. CFSTR_INETURLW (links dragged from browser address bar / page)
    let url_cf = fmt_inet_url_w();
    if url_cf != 0 {
        let f_url = make_formatetc(url_cf, TYMED_HGLOBAL.0 as u32);
        if let Ok(med) = obj.GetData(&f_url) {
            let guard = StgMediumGuard(med);
            if let Some(bytes) = read_medium_bytes(&guard.0, 64 * 1024) {
                let url = read_utf16_z(&bytes);
                if !url.is_empty() {
                    return vec![DroppedPayload::Url(url)];
                }
            }
        }
    }

    // 5. CF_UNICODETEXT (selected text or URL)
    let f_txt = make_formatetc(CF_UNICODETEXT.0, TYMED_HGLOBAL.0 as u32);
    if let Ok(med) = obj.GetData(&f_txt) {
        let guard = StgMediumGuard(med);
        if let Some(bytes) = read_medium_bytes(&guard.0, 512 * 1024) {
            let text = read_utf16_z(&bytes);
            if !text.is_empty() {
                if crate::models::AppState::is_likely_url(&text) {
                    return vec![DroppedPayload::Url(text)];
                }
                return vec![DroppedPayload::Text(text)];
            }
        }
    }

    Vec::new()
}

// ── OLE Drag-In: IDropTarget ──────────────────────────────────────────────────

#[implement(IDropTarget)]
struct ShelfDropTarget {
    hwnd: HWND,
    helper: Option<IDropTargetHelper>,
    helper_active: AtomicBool,
    accepting: AtomicBool,
}

impl ShelfDropTarget {
    fn new(hwnd: HWND) -> Self {
        let helper = unsafe {
            CoCreateInstance::<_, IDropTargetHelper>(
                &CLSID_DragDropHelper,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .ok()
        };
        Self {
            hwnd,
            helper,
            helper_active: AtomicBool::new(false),
            accepting: AtomicBool::new(false),
        }
    }

    unsafe fn to_client(&self, pt: &POINTL) -> (f32, f32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        let _ = ScreenToClient(self.hwnd, &mut p);
        (p.x as f32, p.y as f32)
    }
}

impl IDropTarget_Impl for ShelfDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Option<&IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let res = catch_unwind(AssertUnwindSafe(|| unsafe {
            let allowed = if pdweffect.is_null() {
                DROPEFFECT_NONE
            } else {
                *pdweffect
            };
            let is_self = IS_DRAGGING_OUT_SHELF.load(Ordering::Acquire);
            let supported = !is_self
                && (allowed.0 & DROPEFFECT_COPY.0) != 0
                && pdataobj.is_some_and(|o| data_object_supports_shelf(o));
            let eff = if supported {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
            self.accepting.store(supported, Ordering::Release);
            if !pdweffect.is_null() {
                *pdweffect = eff;
            }
            if let Some(ref helper) = self.helper
                && let Some(obj) = pdataobj
            {
                let p = POINT { x: pt.x, y: pt.y };
                let ok = helper.DragEnter(self.hwnd, obj, &p, eff).is_ok();
                self.helper_active.store(ok, Ordering::Release);
            } else {
                self.helper_active.store(false, Ordering::Release);
            }
            if supported {
                let (cx, cy) = self.to_client(pt);
                crate::window::on_ole_drag_enter(self.hwnd, cx, cy);
            }
            Ok(())
        }));
        match res {
            Ok(r) => r,
            Err(_) => Err(E_UNEXPECTED.into()),
        }
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let res = catch_unwind(AssertUnwindSafe(|| unsafe {
            let allowed = if pdweffect.is_null() {
                DROPEFFECT_NONE
            } else {
                *pdweffect
            };
            let ok = self.accepting.load(Ordering::Acquire)
                && (allowed.0 & DROPEFFECT_COPY.0) != 0;
            let eff = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
            if !pdweffect.is_null() {
                *pdweffect = eff;
            }
            if self.helper_active.load(Ordering::Acquire)
                && let Some(ref helper) = self.helper
            {
                let p = POINT { x: pt.x, y: pt.y };
                let _ = helper.DragOver(&p, eff);
            }
            if ok {
                let (cx, cy) = self.to_client(pt);
                crate::window::on_ole_drag_over(self.hwnd, cx, cy);
            }
            Ok(())
        }));
        match res {
            Ok(r) => r,
            Err(_) => Err(E_UNEXPECTED.into()),
        }
    }

    fn DragLeave(&self) -> Result<()> {
        let res = catch_unwind(AssertUnwindSafe(|| unsafe {
            let was_accepting = self.accepting.swap(false, Ordering::AcqRel);
            if self.helper_active.swap(false, Ordering::AcqRel)
                && let Some(ref helper) = self.helper
            {
                let _ = helper.DragLeave();
            }
            if was_accepting {
                crate::window::on_ole_drag_leave(self.hwnd);
            }
            Ok(())
        }));
        match res {
            Ok(r) => r,
            Err(_) => Err(E_UNEXPECTED.into()),
        }
    }

    fn Drop(
        &self,
        pdataobj: Option<&IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        let res = catch_unwind(AssertUnwindSafe(|| unsafe {
            let was_accepting = self.accepting.swap(false, Ordering::AcqRel);
            let allowed = if pdweffect.is_null() {
                DROPEFFECT_NONE
            } else {
                *pdweffect
            };
            let payloads = if was_accepting
                && (allowed.0 & DROPEFFECT_COPY.0) != 0
                && let Some(obj) = pdataobj
            {
                extract_dropped_payloads(obj)
            } else {
                Vec::new()
            };
            let eff = if !payloads.is_empty() {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
            if !pdweffect.is_null() {
                *pdweffect = eff;
            }
            if self.helper_active.swap(false, Ordering::AcqRel)
                && let Some(ref helper) = self.helper
                && let Some(obj) = pdataobj
            {
                let p = POINT { x: pt.x, y: pt.y };
                let _ = helper.Drop(obj, &p, eff);
            }
            let (cx, cy) = self.to_client(pt);
            // Defer state/window/DComp mutations off the synchronous COM RPC Drop callback
            // so Explorer finishes DoDragDrop cleanly before SetWindowPos/ResizeBuffers/WIC run.
            crate::window::enqueue_ole_drop(self.hwnd, cx, cy, payloads);
            Ok(())
        }));
        match res {
            Ok(r) => r,
            Err(_) => Err(E_UNEXPECTED.into()),
        }
    }
}

/// # Safety
/// `hwnd` must be a valid, live top-level window handle owned by this thread.
/// It must not be destroyed until [`revoke_drop_target`] has been called.
pub unsafe fn register_drop_target(hwnd: HWND) -> bool {
    let target: IDropTarget = ShelfDropTarget::new(hwnd).into();
    RegisterDragDrop(hwnd, &target).is_ok()
}

/// # Safety
/// `hwnd` must be the same valid handle previously passed to
/// [`register_drop_target`], and must still be alive.
pub unsafe fn revoke_drop_target(hwnd: HWND) {
    let _ = RevokeDragDrop(hwnd);
}

// ── OLE Drag-Out: IDropSource + TextDataObject + Shell File IDataObject ──────

#[implement(IDropSource)]
struct ShelfDropSource;

impl IDropSource_Impl for ShelfDropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: BOOL,
        grfkeystate: MODIFIERKEYS_FLAGS,
    ) -> HRESULT {
        match catch_unwind(AssertUnwindSafe(|| {
            if fescapepressed.as_bool() {
                DRAGDROP_S_CANCEL
            } else if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
                DRAGDROP_S_DROP
            } else {
                S_OK
            }
        })) {
            Ok(hr) => hr,
            Err(_) => E_UNEXPECTED,
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[implement(IDataObject)]
struct TextDataObject {
    text: String,
    is_url: bool,
}

impl TextDataObject {
    fn formats(&self) -> Vec<FORMATETC> {
        let mut v = vec![make_formatetc(CF_UNICODETEXT.0, TYMED_HGLOBAL.0 as u32)];
        if self.is_url {
            let cf = fmt_inet_url_w();
            if cf != 0 {
                v.push(make_formatetc(cf, TYMED_HGLOBAL.0 as u32));
            }
        }
        v
    }

    unsafe fn alloc_utf16_hglobal(s: &str) -> Result<HGLOBAL> {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        let hmem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes)?;
        let ptr = GlobalLock(hmem) as *mut u16;
        if ptr.is_null() {
            return Err(E_UNEXPECTED.into());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(hmem);
        Ok(hmem)
    }
}

impl IDataObject_Impl for TextDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        match catch_unwind(AssertUnwindSafe(|| unsafe {
            if pformatetcin.is_null() {
                return Err(DV_E_FORMATETC.into());
            }
            let req = &*pformatetcin;
            if (req.tymed & TYMED_HGLOBAL.0 as u32) == 0 {
                return Err(DV_E_TYMED.into());
            }
            let url_cf = fmt_inet_url_w();
            let supported = req.cfFormat == CF_UNICODETEXT.0
                || (self.is_url && url_cf != 0 && req.cfFormat == url_cf);
            if !supported {
                return Err(DV_E_FORMATETC.into());
            }
            let hmem = TextDataObject::alloc_utf16_hglobal(&self.text)?;
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hmem },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        })) {
            Ok(r) => r,
            Err(_) => Err(E_UNEXPECTED.into()),
        }
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        match catch_unwind(AssertUnwindSafe(|| unsafe {
            if pformatetc.is_null() {
                return DV_E_FORMATETC;
            }
            let req = &*pformatetc;
            if (req.tymed & TYMED_HGLOBAL.0 as u32) == 0 {
                return DV_E_TYMED;
            }
            let url_cf = fmt_inet_url_w();
            if req.cfFormat == CF_UNICODETEXT.0
                || (self.is_url && url_cf != 0 && req.cfFormat == url_cf)
            {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        })) {
            Ok(hr) => hr,
            Err(_) => E_UNEXPECTED,
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        unsafe {
            if !pformatetcout.is_null() {
                (*pformatetcout).ptd = std::ptr::null_mut();
            }
        }
        HRESULT(0x0004_0130) // DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        let fmts = self.formats();
        unsafe { SHCreateStdEnumFmtEtc(&fmts) }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Option<&IAdviseSink>,
    ) -> Result<u32> {
        Err(E_NOTIMPL.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

unsafe fn create_shell_file_data_object(hwnd: HWND, path: &str) -> Result<IDataObject> {
    let wide = HSTRING::from(path);
    let mut pidl = std::ptr::null_mut();
    SHParseDisplayName(&wide, None, &mut pidl, 0, None)?;
    struct PidlGuard(*mut windows::Win32::UI::Shell::Common::ITEMIDLIST);
    impl Drop for PidlGuard {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0 as *const _));
            }
        }
    }
    let _guard = PidlGuard(pidl);
    let mut child_pidl = std::ptr::null_mut();
    let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child_pidl))?;
    let children = [child_pidl as *const _];
    let data_obj: IDataObject = folder.GetUIObjectOf(hwnd, &children, None)?;
    Ok(data_obj)
}

/// Starts a synchronous OLE drag-out operation for a Shelf item.
/// Caller MUST release all `AppState` locks before calling this (nested message loop!).
/// Returns `true` if the drop target accepted the item (`COPY` or `LINK`).
///
/// # Safety
/// `hwnd` must be a valid, live window handle owned by the calling thread.
/// `item.payload` must remain a valid path for the duration of the nested
/// OLE message loop, and no `AppState` lock may be held on entry.
pub unsafe fn start_shelf_drag_out(hwnd: HWND, item: &ShelfItem) -> bool {
    let data_obj: IDataObject = match item.kind {
        ShelfKind::File | ShelfKind::Image => {
            if !Path::new(&item.payload).exists() {
                return false;
            }
            match create_shell_file_data_object(hwnd, &item.payload) {
                Ok(obj) => obj,
                Err(_) => return false,
            }
        }
        ShelfKind::Text => TextDataObject {
            text: item.payload.clone(),
            is_url: false,
        }
        .into(),
        ShelfKind::Url => TextDataObject {
            text: item.payload.clone(),
            is_url: true,
        }
        .into(),
    };

    // Attach Shell drag-image ghost preview when available (non-fatal if unsupported).
    if let Ok(helper) = CoCreateInstance::<_, IDragSourceHelper>(
        &CLSID_DragDropHelper,
        None,
        CLSCTX_INPROC_SERVER,
    ) {
        let pt = POINT { x: 0, y: 0 };
        let _ = helper.InitializeFromWindow(hwnd, Some(&pt), &data_obj);
    }

    let source: IDropSource = ShelfDropSource.into();
    struct DragOutGuard;
    impl Drop for DragOutGuard {
        fn drop(&mut self) {
            IS_DRAGGING_OUT_SHELF.store(false, Ordering::Release);
        }
    }
    IS_DRAGGING_OUT_SHELF.store(true, Ordering::Release);
    let _drag_guard = DragOutGuard;

    let mut effect = DROPEFFECT_NONE;
    // CRITICAL: Only allow COPY | LINK (never MOVE) so Explorer never relocates user's original file.
    let hr = DoDragDrop(
        &data_obj,
        &source,
        DROPEFFECT_COPY | DROPEFFECT_LINK,
        &mut effect,
    );
    hr == DRAGDROP_S_DROP
        && (effect.0 & (DROPEFFECT_COPY.0 | DROPEFFECT_LINK.0)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_32bpp_bottom_up_decodes_and_saves_png() {
        // Minimal 2x2 32bpp bottom-up BI_RGB DIB with alpha=0 (should promote to opaque 255)
        let mut dib = Vec::new();
        let hdr = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 2,
            biHeight: 2,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 16,
            ..Default::default()
        };
        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(
                (&hdr as *const BITMAPINFOHEADER) as *const u8,
                std::mem::size_of::<BITMAPINFOHEADER>(),
            )
        };
        dib.extend_from_slice(hdr_bytes);
        // 4 pixels (BGRA) with alpha=0
        for _ in 0..4 {
            dib.extend_from_slice(&[255, 128, 64, 0]);
        }
        let saved = save_dib_to_cache(&dib);
        assert!(saved.is_some(), "valid 32bpp DIB should save to shelf-cache");
        let (label, path) = saved.unwrap();
        assert!(label.starts_with("Image_") && label.ends_with(".png"));
        let p = PathBuf::from(&path);
        assert!(p.exists());
        let raw = std::fs::read(&p).unwrap();
        assert_eq!(&raw[0..8], b"\x89PNG\r\n\x1a\n");
        if let Some(parent) = p.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}
