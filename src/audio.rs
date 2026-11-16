use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Instant;
use windows::Win32::Media::Audio::Endpoints::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

/// Reusable COM audio enumerator per worker thread to avoid calling
/// CoCreateInstance(&MMDeviceEnumerator) on every poll or slider pixel drag.
pub struct AudioMonitor {
    enumerator: Option<IMMDeviceEnumerator>,
    /// Cached `IAudioEndpointVolume` for the default render endpoint. Enumerating
    /// the endpoint and activating the interface costs ~2 ms per poll, while the
    /// actual volume read is a cheap property fetch. The interface is re-resolved
    /// periodically so a device switch is still picked up quickly.
    endpoint_vol: Option<IAudioEndpointVolume>,
    last_resolve: Option<Instant>,
}

impl AudioMonitor {
    pub fn new() -> Self {
        Self {
            enumerator: None,
            endpoint_vol: None,
            last_resolve: None,
        }
    }

    fn resolve_endpoint(&mut self) -> Option<()> {
        unsafe {
            if self.enumerator.is_none() {
                self.enumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok();
            }
            let enumerator = self.enumerator.as_ref()?;
            let device = match enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) {
                Ok(d) => d,
                Err(_) => {
                    self.enumerator = None;
                    return None;
                }
            };
            let endpoint_vol: IAudioEndpointVolume = match device.Activate(CLSCTX_ALL, None) {
                Ok(v) => v,
                Err(_) => {
                    self.enumerator = None;
                    return None;
                }
            };
            self.endpoint_vol = Some(endpoint_vol);
            self.last_resolve = Some(Instant::now());
            Some(())
        }
    }

    pub fn read_master_volume(&mut self) -> Option<f32> {
        const ENDPOINT_REFRESH: std::time::Duration = std::time::Duration::from_millis(3000);
        let stale = match (self.endpoint_vol.is_some(), self.last_resolve) {
            (true, Some(t)) => t.elapsed() >= ENDPOINT_REFRESH,
            _ => true,
        };
        if stale {
            self.resolve_endpoint()?;
        }
        unsafe {
            let endpoint_vol = self.endpoint_vol.as_ref()?;
            let muted = endpoint_vol.GetMute().ok().map(|b| b.as_bool()).unwrap_or(false);
            if muted {
                return Some(0.0);
            }
            match endpoint_vol.GetMasterVolumeLevelScalar() {
                Ok(v) => Some(v),
                Err(_) => {
                    // Endpoint went away (device switch / driver reset): resolve again next poll.
                    self.endpoint_vol = None;
                    None
                }
            }
        }
    }
}

impl Default for AudioMonitor {
    fn default() -> Self {
        Self::new()
    }
}

struct ComInitGuard(bool);

impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub fn get_master_volume() -> Option<f32> {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // S_OK / S_FALSE require balanced CoUninitialize; RPC_E_CHANGED_MODE must not uninit.
    let _com = ComInitGuard(hr.is_ok());
    let mut m = AudioMonitor::new();
    m.read_master_volume()
}

struct VolWorker {
    pending: Mutex<Option<f32>>,
    cvar: Condvar,
}

static VOL_WORKER: OnceLock<VolWorker> = OnceLock::new();
static LAST_VOL_WRITE_EPOCH: OnceLock<Instant> = OnceLock::new();
static LAST_VOL_WRITE_MS: AtomicU64 = AtomicU64::new(0);

fn monotonic_ms() -> u64 {
    let base = LAST_VOL_WRITE_EPOCH.get_or_init(Instant::now);
    base.elapsed().as_millis() as u64 + 1
}

pub fn recently_wrote_volume() -> bool {
    let last = LAST_VOL_WRITE_MS.load(Ordering::Acquire);
    last != 0 && monotonic_ms().saturating_sub(last) < 800
}

fn apply_master_volume_com(enumerator: &mut Option<IMMDeviceEnumerator>, level: f32) -> Option<()> {
    unsafe {
        if enumerator.is_none() {
            *enumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok();
        }
        let enum_ref = enumerator.as_ref()?;
        let device = match enum_ref.GetDefaultAudioEndpoint(eRender, eMultimedia) {
            Ok(d) => d,
            Err(_) => {
                *enumerator = None;
                return None;
            }
        };
        let endpoint_vol: IAudioEndpointVolume = match device.Activate(CLSCTX_ALL, None) {
            Ok(v) => v,
            Err(_) => {
                *enumerator = None;
                return None;
            }
        };
        let clamped = level.clamp(0.0, 1.0);
        let _ = endpoint_vol.SetMute(clamped <= 0.001, std::ptr::null());
        if clamped > 0.001 {
            if endpoint_vol.SetMasterVolumeLevelScalar(clamped, std::ptr::null()).is_err() {
                *enumerator = None;
                return None;
            }
        } else {
            let _ = endpoint_vol.SetMasterVolumeLevelScalar(0.0, std::ptr::null());
        }
        Some(())
    }
}

/// Non-blocking master volume update: coalesces rapid slider drags onto a
/// single COM worker thread so Win32 UI `WM_MOUSEMOVE` never blocks.
pub fn set_master_volume(level: f32) {
    let worker = VOL_WORKER.get_or_init(|| {
        std::thread::Builder::new()
            .name("cynotch-audio".to_string())
            .spawn(|| {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let w = VOL_WORKER.get().expect("VOL_WORKER initialized");
                let mut enumerator: Option<IMMDeviceEnumerator> = None;
                loop {
                    let target = {
                        let mut guard = w.pending.lock().unwrap();
                        while guard.is_none() {
                            guard = w.cvar.wait(guard).unwrap();
                        }
                        guard.take()
                    };
                    if let Some(val) = target {
                        let _ = apply_master_volume_com(&mut enumerator, val);
                    }
                }
            })
            .ok();
        VolWorker {
            pending: Mutex::new(None),
            cvar: Condvar::new(),
        }
    });

    LAST_VOL_WRITE_MS.store(monotonic_ms(), Ordering::Release);
    if let Ok(mut guard) = worker.pending.lock() {
        *guard = Some(level.clamp(0.0, 1.0));
        worker.cvar.notify_one();
    }
}
