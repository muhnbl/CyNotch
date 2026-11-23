//! Frosted-blur availability and fallback policy.
//!
//! As of v0.3.1, frosted blur is rendered natively inside the single content
//! HWND via `DCompHost::set_blur` (`HostBackdropBrush` + `D2D1GaussianBlur` +
//! hardware-antialiased `CompositionGeometricClip`). No paired `WS_POPUP`
//! backdrop window or GDI `SetWindowRgn` is used for blur.

use std::ffi::OsStr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Master switch. `false` == plain translucent glass without backdrop blur.
pub const ENABLE_BLUR_BACKDROP: bool = true;

/// Tint opacity (percent, see `tok::scale_frosted`) used when blur is unavailable.
/// Without blur the translucent shell must be denser to stay readable.
pub const GLASS_FALLBACK_OPACITY_PCT: u32 = 118;

static BLUR_BROKEN: AtomicBool = AtomicBool::new(false);
static BLUR_PX: AtomicU32 = AtomicU32::new(20);

/// Updates the global blur intensity (clamped `4..=40`).
pub fn set_blur_px(px: u32) {
    BLUR_PX.store(px.clamp(4, 40), Ordering::Release);
}

/// Returns the current global blur intensity (`4..=40`).
pub fn blur_px() -> u32 {
    BLUR_PX.load(Ordering::Acquire)
}

pub(crate) fn env_disables_frost(v: &OsStr) -> bool {
    if v.is_empty() {
        return false;
    }
    let s = v.to_string_lossy();
    s != "0" && !s.eq_ignore_ascii_case("false")
}

fn cli_disabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::args().any(|a| a.eq_ignore_ascii_case("--no-frost"))
            || std::env::var_os("CYNOTCH_NO_FROST").is_some_and(|v| env_disables_frost(&v))
    })
}

/// True while native DirectComposition backdrop blur may be used (switch on,
/// no CLI/env opt-out, no earlier permanent failure this session).
pub fn blur_available() -> bool {
    ENABLE_BLUR_BACKDROP && !cli_disabled() && !BLUR_BROKEN.load(Ordering::Acquire)
}

/// Marks blur as unusable for the rest of the session (host-backdrop creation
/// failed). Callers then render dense translucent glass instead.
pub fn mark_unavailable() {
    BLUR_BROKEN.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_disables_frost_treats_empty_zero_false_as_enabled() {
        assert!(!env_disables_frost(OsStr::new("")));
        assert!(!env_disables_frost(OsStr::new("0")));
        assert!(!env_disables_frost(OsStr::new("false")));
        assert!(!env_disables_frost(OsStr::new("FALSE")));
        assert!(!env_disables_frost(OsStr::new("False")));

        assert!(env_disables_frost(OsStr::new("1")));
        assert!(env_disables_frost(OsStr::new("true")));
        assert!(env_disables_frost(OsStr::new("yes")));
    }
}
