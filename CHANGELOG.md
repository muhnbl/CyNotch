# Changelog

## v0.2.4.1 (2026-11-19)
### Hover Dwell Delay & Deadlock Fix (`src/window.rs`, `src/renderer/mod.rs`)
- Added a 240ms hover dwell timer (`TIMER_HOVER_OPEN_ID = 3`) before expanding the collapsed notch on mouse hover, preventing accidental opens when moving the cursor near the top edge.
- Added a subtle pending-hover border cue during the 240ms dwell window.
- Fixed UI-thread `Mutex` deadlock by moving `redraw_window(hwnd)` outside all `state_mutex.lock()` scopes, and preserved immediate click/drag (`IdlePressOrDrag`) responsiveness on `WM_LBUTTONDOWN`.

## v0.2.4 (2026-11-18)
### Frosted Blur Backdrop & Transparency Controls (`src/backdrop.rs`, `src/backdrop_geom.rs`, `src/settings_window.rs`)
- Added paired redirected `WS_POPUP` frosted-blur backdrop windows (`CyNotchBackdropClass`) below both the notch and Preferences windows, preserving `WS_EX_NOREDIRECTIONBITMAP` on the content windows for screenshot/OBS visibility.
- Enabled Windows 11 `DWMWCP_ROUND` corner rounding and radius-aware inset (`inset_for_radius`) so rounded shells never show sharp 90-degree backdrop corners across all notch states and DPI scales.
- Added `--no-frost` / `CYNOTCH_NO_FROST=1` opt-out and automatic fallback when Windows Transparency Effects are disabled.
- Added **Frosted Blur & Transparency**, **Glass Opacity** (`35%`, `50%`, `68%`, `85%`), and **Blur Intensity** (`Soft`, `Medium`, `Deep`, `Ultra`) controls in Preferences (`General -> Interaction & Idle Visuals`).
- Fixed DComp/DispatcherQueue thread-local teardown crash (`0x0000087a`) on exit.

## v0.2.3 (2026-11-17)
### Color Emoji & Unicode Fallback (`src/font.rs`)
- Added Windows Direct2D + DirectWrite + WIC color-emoji rasterizer (`D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT`) with cluster segmentation (`next_cluster`) for ZWJ, flag, keycap, and skin-tone sequences.
- Added `seguisym.ttf` and `seguihis.ttf` monochrome fallbacks so notification emojis and Unicode symbols no longer render as `▯` tofu boxes.
- Added `windows` crate features: `Win32_Graphics_Direct2D`, `Win32_Graphics_Direct2D_Common`, `Win32_Graphics_DirectWrite`, `Win32_Graphics_Imaging`.
