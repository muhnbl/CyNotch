# Changelog

## v1.0.0 (2026-11-24)
- First stable release (final major patch line promoted directly from v0.3.1 to v1.0.0).
- **144Hz GPU Expand/Close & DWM Pipeline Overhaul**:
  - Pre-rendered offscreen GPU `ID2D1Bitmap1` snapshot during hover dwell (`hover_dwell_t >= 0.25`) and close transitions, eliminating per-frame DirectWrite glyph re-rasterization and cutting expand/close frame time from `10–25ms` down to **`2.4–3.5ms`** (`3.7×` faster).
  - Kept the DirectComposition `HostBackdropBrush` blur visual at fixed size/offset `(0, 0)` with full-canvas mask coordinates and upgraded both main & mask DXGI swapchains to **triple-buffering (`BufferCount: 3`)**, eliminating `dwm.exe` blur target reallocation stalls during expand/close.
  - Added native `144Hz` (`7ms`) pacing, instant condvar wakeup on `clear_notch_pacer_in_flight()`, high-res waitable-timer pacing for `<= 16ms` (60Hz–144Hz), and `last_anim_instant` reset on idle-to-active transitions (eliminating frame-1 `33ms` close lurch).
  - Deferred `WS_EX_TRANSPARENT` re-enabling until close animations settle (`!is_size_moving()`) and while `is_hover_pending`, preventing mid-close DWM freezes and synthetic `WM_MOUSELEAVE` hover-cancel races.
- **Exclusive Capsule Hover Sweep & Self-Healing Hover Dwell**:
  - Strictly gated the hover-dwell sweep to the collapsed capsule (`!is_hovered && anim_expand_t <= 0.005`) and cleared dwell/afterglow immediately on expand so it never bleeds over the expanded panel.
  - Replaced the 3px-quantized 55%-alpha-floor scanline with a smooth 3-stop optical light wave that enters at `0%` opacity and dissolves cleanly to `0%` opacity before the right edge.
  - Tightened collapsed click-through hover polling (`250ms → 50ms`) while throttling heavy fullscreen/Z-order checks to `250ms`, and added a self-healing dwell-completion check in `TIMER_POLL_ID` so rapid hover open/close cycles never stall.
- **Preferences Redesign & Zero-Churn Motion**:
  - Graphite obsidian, macOS-inspired grouped layout with CyNotch violet accent (no blue), Linear-style rows (leading glyph, hover lift, hairline separators), and flat outer surface without per-frame offscreen aurora `PushLayer` overhead.
  - Replaced non-1.0 text scaling on Preferences open/category-switch and expanded tab switches with smooth fade + Y-slide (`scale = 1.0`), eliminating DirectWrite glyph churn.
  - Acrylic backdrop blur is always requested (in-app Frosted toggle removed); removed "Position & Lock" row (lock via notch padlock, center via drag); larger UI scale defaults (`1.12 / 1.20 / 1.32 / 1.50`) and humanized copy.
- **Idle Efficiency & Polish**:
  - Throttled idle Wolf Eyes breathing to `20 FPS` (`50ms`), gated expanded poll snapshots to foreground-change / `250ms` ticks, lazy-allocated unused `bgra_scratch`, and raised D2D brush cache caps to `1024`.
  - Fixed text clipping at screen edges (`idle_ui_scale`), repaired mojibake (`â€¢`, `â€"`), centered Purge RAM label, and confirmed zero sleep/screensaver-blocking APIs (`SetThreadExecutionState` unused).
- **Verified**: Release build (`1.70 MB` / `1,704,448` bytes) with `0` warnings, `147/147` tests passing, `clippy` 0 errors, visual D2D snapshots verified.

## v0.3.1 (2026-11-22)
### 1. Pure Stealth Idle (Glowing Purple Wolf Eyes) & Staged Choreography
- Replaced static `"CyNotch"` idle branding text in Minimal mode with **2 Glowing Purple Wolf Eyes** (`draw_wolf_eyes` in `src/renderer/mascot.rs`) featuring subtle breathing luminescence, slow natural blinks, and `~650ms` predator squint + violet micro-reaction on clipboard/capture events.
- Implemented **Staged Choreography** calibrated to **Claude Fast** timing (`420ms` `anim_capsule_t`, `220ms` `held_shrink_dims`, `0.0..0.42` roll-down exit + `0.30..1.00` overlapped roll-in) for Music → Stealth Idle and Song A → Song B transitions.
- Refined Music capsule typography & layout (`src/renderer/mod.rs`, `src/painter_d2d.rs`): `1.05x` scale, `music_max_width` (`240px` / `290px` / `350px`), **`Judul Lagu`** (Bold 100%) **`- Nama Artis`** (Regular 75%), with Direct2D `OpacityMask` fade-out before the Music badge.

### 2. 3-Box Floating Music UI, SMTC Controls & Uniform `620×234` Choreography
- Redesigned `NotchMode::Music` (`src/renderer/views.rs`) into 3 separated floating Obsidian Liquid Glass sections within the uniform `620×234` expanded bounds (zero pop-up/resize on tab switch):
  - **Box 1 (`y=0..134`)**: Mode Dock + Centered Large Lyrics (Apple Music focus, no mascot or `Lyrics` toggle) + Full-Width Seek Bar.
  - **Box 2 (`y=146..188`)**: Floating Track Info card (cover tile, title, artist, timestamps).
  - **Box 3 (`y=194..230`)**: 3 separated floating media buttons (`Prev`, `Play/Pause`, `Next`).
- Added `BlurSpec.box1_h` & `BlurSpec.extra_boxes` (`src/dcomp_blur.rs`, `src/composition.rs`) so all 3 separated boxes receive identical `15px` WinRT `GaussianBlur` + Optical Luminosity Clamp while gaps remain 100% transparent.
- Upgraded media transport (`src/media.rs`) to **WinRT SMTC** (`TryTogglePlayPauseAsync`, `TrySkipNextAsync`, `TrySkipPreviousAsync`) with `VK_MEDIA` fallback, and added monotonic SMTC progress clamping in `src/main.rs` to eliminate playhead jitter.

### 3. Focused 5-Mode Hub, Top-Center Lock, Vertical Shelf & Privacy Hardening
- **Locked Top-Center & Pruned Scope**: Locked notch to `NotchPlacement::Top` (removed grip drag, edge snap, `-90°/+90°` side dock, `Tasks`, `Capture`, `ScreenTime`, `Dock & Modules`, lyric size/style chips, and manual 8-area MemReduct selection in favor of the non-stuttering **Safe Trim** profile: *"Frees unused RAM safely • keeps apps & tabs intact"*).
- **Vertical Quick Shelf & Staggered Mode Motion**: Converted `Quick Shelf` to a 3-visible-row vertical stacked list (`draw_shelf_item_row` with kind icon, ellipsized label, hover highlight, `✕` remove button, and vertical scroll), and upgraded all 5 modes (`Home`, `Music`, `Volume`, `MemReduct`, `Shelf`) to `--ray-fast` (`120ms`, `+6px → 0` + fade) per-card/per-box staggered entrances respecting `SPI_GETCLIENTAREAANIMATION`.
- **Streamlined 4-Section Preferences**: Consolidated Settings into `General & Notch` (including Background Listeners), `Motion & Perf`, `Notifications`, and `Memory & System`, with card stagger on category switch.
- **Click-Through, Fullscreen Auto-Suppress & Clipboard Privacy (`src/window.rs`)**: `WS_EX_LAYERED | WS_EX_TRANSPARENT` when collapsed (`click_to_open` OFF), `HoverDwellSpeed` defaulting to `Normal (650ms)` (`300ms / 650ms / 1000ms`), conflict-free global hotkeys (`Ctrl+Alt+C` / `Ctrl+Alt+M` / `Ctrl+Alt+S`), `SHQueryUserNotificationState` fullscreen auto-suppress over games/videos, and Windows clipboard privacy format checks (`ExcludeClipboardContentFromMonitorProcessing` & `CanIncludeInClipboardHistory`) skipping password managers.
- **Verified**: All test suites passing (`cargo test`), visual D2D snapshots verified (`snap_d2d`), and release build deployed to `%USERPROFILE%\.local\bin\cynotch.exe`.

## v0.3.0 (2026-11-20)
### 1. GPU Direct2D + DirectWrite Renderer & 55.9% Binary Size Reduction
- Migrated all notch and Preferences rendering (`src/renderer/*`, `src/settings_window.rs`, `src/icons.rs`, `src/window.rs`) from CPU `tiny-skia` + `fontdue` to **GPU-accelerated Direct2D (`ID2D1DeviceContext`) and DirectWrite (`DWriteTextCache`)** bound directly to the DirectComposition DXGI SwapChain (`src/composition.rs`, `src/painter_d2d.rs`, `src/font_d2d.rs`), cutting expanded frame render time from **8.51–10.96 ms (CPU)** down to **0.93 ms CPU record / 1.08 ms GPU flush (~10× faster)** and idle capsule renders down to **59 µs CPU / 123 µs GPU**.
- Removed `tiny-skia`, `fontdue`, `ureq`, and `chrono` crates: replaced `ureq` with native Win32 `WinHTTP` (`src/lyrics.rs`), `chrono` with Win32 `GetLocalTime` / `GetTimeZoneInformation` (`src/localtime.rs`), and PNG screenshot encoding with Win32 WIC (`src/capture.rs`), shrinking `cynotch.exe` from **3.95 MB (`3,949,056` B) in v0.2.4.1 down to 1.74 MB (`1,742,848` B) in v0.3.0 (−55.9%)**.

### 2. Physical Screen-Edge & Corner Portal Glow + Framer-Motion Spring Choreography
- Added an independent click-through DirectComposition screen-edge portal glow window (`src/edge_glow.rs`) hugging the physical monitor bezel (`Top`, `Left`, `Right`) and **originating directly from the physical top-left / top-right corner (`TopLeft`, `TopRight`) when corner-docked**, with a 32-stop 16bpc float `C^2`-continuous radial violet aura, 5th-order smootherstep opening (`t³(6t² − 15t + 10)`), and a seamless ~1.2s atmospheric dissipation fade-out.
- Upgraded notch expansion, dock active pill gliding, and Preferences category switching to critically-damped Framer-Motion spring physics (`spring_step` in `src/models.rs`), exclusively gated behind Windows *Animation Effects* (`SPI_GETCLIENTAREAANIMATION` — snaps instantaneously when Windows animations are disabled).
- Added configurable **Hover Delay** presets (`Quick 120ms`, `Normal 400ms`, `Slow 700ms`) in `Motion & Perf`, plus true cross-process click-through (`WS_EX_LAYERED | WS_EX_TRANSPARENT`) on `EdgeGlowOverlay` and physical cursor re-verification on dwell expiry so accidental brush-throughs never swallow nearby clicks.

### 3. Notification Audio Deduplication & Unicode Bidi Box Fix
- Replaced legacy sound variants with a gentle **fixed 25% volume Snapchat Pop chime** (`src/sound.rs`) with burst deduplication (plays once per simultaneous notification batch) and automatic native Windows toast sound muting/restoration (`sync_windows_notification_sound_mute`).
- Fixed `[FSI]` / `[PDI]` dotted boxes in Discord/WhatsApp/browser notifications and media titles by stripping invisible Unicode bidi isolate and format code points (`U+2066..U+206F`, `U+200B..U+200F`, `U+2028..U+202E`, `U+FEFF`) in both `sanitize_notification_text` (`src/notifications.rs`) and `DWriteTextCache::clean_display_str` (`src/font_d2d.rs`) while preserving `U+200D` (`ZWJ`) compound emojis.

### 4. Interactive Sliders, Unclipped Preferences & Pre-Release QA Polish
- Replaced percentage-button presets for **Glass Opacity** (`Airy`..`Solid`) and **Blur Intensity** (`Soft`..`Ultra`) with smooth continuous drag sliders that auto-enable `Frosted Blur` and provide a one-click **`Enable in Win`** deep-link when Windows Transparency Effects are off.
- Unified Music seek bar and Volume slider into 1:1 sub-pixel click-and-drag scrubbers with glowing thumbs, full-row (`556×40px`) toggle click targets, right-click context menu directly on the notch window, `WM_CAPTURECHANGED` drag-reset safety, keyboard navigation (`Up`/`Down`/`PgUp`/`PgDn`/`Home`/`End`/`1`–`5`/`Tab`), and calibrated Preferences tabs so `Motion`, `Modules`, `Notifications`, and `System` fit 100% unclipped at `scroll = 0`.

### 5. Production Reliability, Security & GPU Recovery Hardening
- **Memory & FFI Safety:** Fixed WinHTTP UTF-16 null-terminated pointer handling with bounded reads, status checks, and read deadlines (`src/lyrics.rs`); migrated Direct2D `StrokeStyle` ownership into `D2DResourceCache` (`src/painter_d2d.rs`); added DIB overflow guards and proper COM `ManuallyDrop` / `HICON` lifecycle cleanup.
- **DirectComposition GPU Recovery:** Added shared DComp generation tracking (`SHARED_DCOMP_GEN`), target close-on-drop, and same-HWND target recreation (`src/composition.rs`, `src/edge_glow.rs`) so GPU driver resets and display changes recover cleanly without blank frames.
- **Crash-Resistant Atomic Persistence:** Implemented write-temp + flush + atomic replace with `.bak` recovery and corruption preservation across config, tasks, shelf, screen-time, and notification backups (`src/persistence.rs`).
- **Security & Registry Hardening:** Hardened Quick Shelf `ShellExecuteW` policy to block executable extensions, trailing dots, NTFS ADS, UNC/device paths, and symlink bypasses (`src/capture.rs`); hardened autostart registry writes and PowerShell helper (`src/autostart.rs`, `Enable-AutoStart.ps1`); and ensured safe backup-before-mutation and clean restore for Windows notification suppression and border hooks (`src/notifications.rs`, `src/borders.rs`).
- **Verified by 132/132 Automated, Adversarial & Soak Tests:** Full unit, WARP Direct2D pixel, concurrency, and leak/handle soak coverage (`tests/reliability_hardening_tests.rs`, `tests/soak_and_adversarial_tests.rs`, `tests/edge_glow_tests.rs`).

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
