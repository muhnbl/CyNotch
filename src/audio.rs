use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
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
        const ENDPOINT_REFRESH: std::time::Duration = std::time::Duration::from_millis(10_000);
        let stale = match (self.endpoint_vol.is_some(), self.last_resolve) {
            (true, Some(t)) => t.elapsed() >= ENDPOINT_REFRESH,
            _ => true,
        };
        if stale {
            self.resolve_endpoint()?;
        }
        unsafe {
            let endpoint_vol = self.endpoint_vol.as_ref()?;
            let muted = endpoint_vol
                .GetMute()
                .ok()
                .map(|b| b.as_bool())
                .unwrap_or(false);
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

impl VolWorker {
    fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            cvar: Condvar::new(),
        }
    }

    fn post(&self, level: f32) {
        let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(level);
        self.cvar.notify_one();
    }

    fn take_blocking(&self) -> f32 {
        let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(val) = guard.take() {
                return val;
            }
            guard = self.cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }
}

static VOL_WORKER: Mutex<Option<Arc<VolWorker>>> = Mutex::new(None);
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
            if endpoint_vol
                .SetMasterVolumeLevelScalar(clamped, std::ptr::null())
                .is_err()
            {
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
/// Retryable on transient OS thread-spawn failure and panic-isolated via `catch_unwind` (BH-41).
pub fn set_master_volume(level: f32) {
    let worker = {
        let mut slot = VOL_WORKER.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = slot.as_ref() {
            Some(Arc::clone(existing))
        } else {
            let w = Arc::new(VolWorker::new());
            let bg = Arc::clone(&w);
            if std::thread::Builder::new()
                .name("cynotch-audio".to_string())
                .spawn(move || {
                    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                    let _com = ComInitGuard(hr.is_ok());
                    let mut enumerator: Option<IMMDeviceEnumerator> = None;
                    loop {
                        let val = bg.take_blocking();
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let _ = apply_master_volume_com(&mut enumerator, val);
                        }));
                    }
                })
                .is_ok()
            {
                *slot = Some(Arc::clone(&w));
                Some(w)
            } else {
                None
            }
        }
    };

    LAST_VOL_WRITE_MS.store(monotonic_ms(), Ordering::Release);
    if let Some(w) = worker {
        w.post(level.clamp(0.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bh03_vol_worker_coalesces_deterministically_without_init_race() {
        let w = Arc::new(VolWorker::new());
        let bg = Arc::clone(&w);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let gate_bg = Arc::clone(&gate);
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let done_bg = Arc::clone(&done);
        let received = Arc::new(Mutex::new(Vec::<f32>::new()));
        let rec_bg = Arc::clone(&received);

        let handle = std::thread::spawn(move || {
            // Wait until producer has posted a burst so coalescing is deterministically exercised.
            {
                let (lock, cvar) = &*gate_bg;
                let mut g = lock.lock().unwrap();
                while !*g {
                    g = cvar.wait(g).unwrap();
                }
            }
            loop {
                let v = bg.take_blocking();
                if v < 0.0 {
                    break;
                }
                rec_bg.lock().unwrap().push(v);
                if (v - 1.0).abs() < 1e-4 {
                    let (lock, cvar) = &*done_bg;
                    *lock.lock().unwrap() = true;
                    cvar.notify_all();
                }
            }
        });

        for i in 1..=50 {
            w.post((i as f32) / 50.0);
        }
        {
            let (lock, cvar) = &*gate;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        }
        {
            let (lock, cvar) = &*done;
            let mut g = lock.lock().unwrap();
            while !*g {
                g = cvar.wait(g).unwrap();
            }
        }
        w.post(-1.0);
        handle.join().expect("worker thread must not panic");

        let vals = received.lock().unwrap();
        assert_eq!(*vals.last().unwrap(), 1.0);
        assert!(
            vals.len() < 50,
            "burst of 50 updates should coalesce, saw {}",
            vals.len()
        );
    }
}
