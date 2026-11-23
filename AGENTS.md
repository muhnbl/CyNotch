# CyNotch Project Rules (`AGENTS.md`)

Strict invariants for any future change to **CyNotch**:

## 1. Product & Scope Lock
- **Locked Top-Center**: `NotchPlacement::Top` only (`x_offset = 0`, `y_offset = 0`). Never re-add grip drag, edge snapping, or `-90° / +90°` vertical side docks.
- **5 Core Dock Modes Only**: `Home` (`Command Hub`), `Music`, `Volume`, `MemReduct` (safe non-stutter profile: `Working Sets + System File Cache + Low-Priority Standby`), and `Shelf` (ephemeral OLE DnD + `Win+Shift+S` `CF_DIB`).
- **Expanded Height (`292.0` base + Shelf Hug Contents)**: `AppState::base_expanded_height()` returns `Self::EXPANDED_HEIGHT` (`292.0`, width `648.0`) for `Home`/`Music`/`Volume`/`MemReduct`, and dynamically grows (`+120px` per additional row of 4, up to 24 items) for `Shelf` (Hug Contents, no scrollbar).

## 1.1 Settings Palette Exception
- The Preferences window uses a local palette (`cyno_tk` in `src/settings_window.rs`): macOS-inspired neutral graphite surfaces + the signature CyNotch violet accent (`#7C3AED`, deep `#3E188A`, orchid `#A855F7`) on every state, window and button, with flat grouped-list cards and no aurora bloom. This is an approved exception to the Obsidian *surface* recipe; do not reintroduce blue or revert the graphite layout. The notch, blur, edge glow, music and shelf keep the invariants below.

## 2. Visual, Material & DirectComposition Invariants
- **Single Source of Truth**: Follow `DESIGN_SYSTEM.md` and `MOTION_SYSTEM.md` strictly.
- **Calibrated Obsidian Liquid Glass**: Fixed `CALIBRATED_BLUR_PX = 15` (`GaussianBlur` + `ColorMatrixEffect` Optical Luminosity Clamp in `src/dcomp_blur.rs`). No opacity/blur sliders.
- **Borderless Cards, Rimmed Buttons**: Cards/sections use `0px` outer stroke (`obsidian_shell_frosted` / `obsidian_deep_frosted`). Only interactive buttons/rows use `1px` bottom-to-top faded rims (`ButtonVariant::{PrimaryFill, PrimaryOutline}`).
- **Multi-Box Floating Mask Invariant**: Whenever a mode renders separated floating boxes with transparent gaps (e.g. `NotchMode::Music`: Box 1 `0..164`, Box 2 `176..242`, Box 3 `250..286`), you MUST:
  1. Populate `BlurSpec.box1_h` and `BlurSpec.extra_boxes` in `src/window.rs` so `mask_swap_chain` (`src/composition.rs`) blurs every floating box identically while keeping gaps 100% transparent.
  2. Never call `rrect_drop_shadow` on separated floating boxes over a transparent SwapChain (produces muddy shadow halos).
  3. Keep `>= 3px` margin inside `292.0` bottom / `0..w` bounds so `SetWindowRgn` and SwapChain edges never clip anti-aliased rounded corners.

## 3. Media & SMTC Invariants
- **SMTC First**: Media transport (`toggle_play_pause`, `next_track`, `prev_track` in `src/media.rs`) must dispatch via `GlobalSystemMediaTransportControlsSession` on a background thread, with `VK_MEDIA` `keybd_event` as fallback only.
- **Monotonic Progress Clamp**: In `src/main.rs`, clamp minor backward SMTC `Position` / `LastUpdatedTime` jitter (`< 2500ms`) while playing so the seek bar never jumps backward visually.

## 4. Mandatory Verification Gate (Before Reporting Done)
1. Run `cargo test` (must exit `0`, 100% pass).
2. For any UI/layout/shader change, run `cargo run --example snap_d2d -- snap_test`, visually inspect the affected PNGs (`read_file`), and delete `snap_test/` afterward.
3. Run `cargo build --release`, deploy `target\release\cynotch.exe` to `C:\Users\muhna\.local\bin\cynotch.exe`, and keep `git status` clean.
