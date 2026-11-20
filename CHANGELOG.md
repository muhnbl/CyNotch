# Changelog

## v0.3.0 (2026-11-20)
### 1. GPU Direct2D + DirectWrite Renderer & 57.9% Binary Size Reduction
- Migrated all notch and Preferences rendering (`src/renderer/*`, `src/settings_window.rs`, `src/icons.rs`, `src/window.rs`) from CPU `tiny-skia` + `fontdue` to **GPU-accelerated Direct2D (`ID2D1DeviceContext`) and DirectWrite (`DWriteTextCache`)** bound directly to the DirectComposition DXGI SwapChain (`src/composition.rs`, `src/painter_d2d.rs`, `src/font_d2d.rs`), cutting expanded frame render time from **8.51–10.96 ms (CPU)** down to **0.93 ms CPU record / 1.08 ms GPU flush (~10× faster)** and idle capsule renders down to **59 µs CPU / 123 µs GPU**.
- Removed `tiny-skia`, `fontdue`, `ureq`, and `chrono` crates: replaced `ureq` with native Win32 `WinHTTP` (`src/lyrics.rs`), `chrono` with Win32 `GetLocalTime` / `GetTimeZoneInformation` (`src/localtime.rs`), and PNG screenshot encoding with Win32 WIC (`src/capture.rs`), shrinking `cynotch.exe` from **3.95 MB (`3,949,056` B) in v0.2.4.1 down to 1.66 MB (`1,662,976` B) in v0.3.0 (−57.9%)**.

### 2. Physical Screen-Edge Portal Glow & Framer-Motion Spring Choreography
- Added an independent click-through `WS_EX_LAYERED | WS_EX_TRANSPARENT` screen-edge portal glow window (`src/edge_glow.rs`) hugging the physical monitor bezel (`Top`, `Left`, `Right`) with a borderless multi-layer radial violet aura (`#C4B5FD` -> `#A855F7` -> `#6D28D9` -> `#2E1065`), 5th-order smootherstep opening (`t³(6t² − 15t + 10)`), and a seamless ~1.2s atmospheric dissipation fade-out.
- Upgraded notch expansion, dock active pill gliding, and Preferences category switching to critically-damped Framer-Motion spring physics (`spring_step` in `src/models.rs`), exclusively gated behind Windows *Animation Effects* (`SPI_GETCLIENTAREAANIMATION` — snaps instantaneously when Windows animations are disabled).
- Added configurable **Hover Delay** presets (`Quick 120ms`, `Normal 240ms`, `Slow 450ms`) in `Motion & Perf`.

### 3. Notification Audio Deduplication & Unicode Bidi Box Fix
- Replaced legacy sound variants with a gentle **fixed 25% volume Snapchat Pop chime** (`src/sound.rs`) with burst deduplication (plays once per simultaneous notification batch) and automatic native Windows toast sound muting/restoration (`sync_windows_notification_sound_mute`).
- Fixed `[FSI]` / `[PDI]` dotted boxes in Discord/WhatsApp/browser notifications and media titles by stripping invisible Unicode bidi isolate and format code points (`U+2066..U+206F`, `U+200B..U+200F`, `U+2028..U+202E`, `U+FEFF`) in both `sanitize_notification_text` (`src/notifications.rs`) and `DWriteTextCache::clean_display_str` (`src/font_d2d.rs`) while preserving `U+200D` (`ZWJ`) compound emojis.

### 4. Interactive Sliders, Unclipped Preferences & Pre-Release QA Polish
- Replaced percentage-button presets for **Glass Opacity** (`Airy`..`Solid`) and **Blur Intensity** (`Soft`..`Ultra`) with smooth continuous drag sliders that auto-enable `Frosted Blur` and provide a one-click **`Enable in Win`** deep-link when Windows Transparency Effects are off.
- Unified Music seek bar and Volume slider into 1:1 sub-pixel click-and-drag scrubbers with glowing thumbs, full-row (`556×40px`) toggle click targets, right-click context menu directly on the notch window, `WM_CAPTURECHANGED` drag-reset safety, keyboard navigation (`Up`/`Down`/`PgUp`/`PgDn`/`Home`/`End`/`1`–`5`/`Tab`), and calibrated Preferences tabs so `Motion`, `Modules`, `Notifications`, and `System` fit 100% unclipped at `scroll = 0`.

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
