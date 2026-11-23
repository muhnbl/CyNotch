# ⚡ CyNotch — General Mahamatra Dynamic Island for Windows

**CyNotch** is an ultra-lightweight, GPU-accelerated **Dynamic Island & Ambient Command Hub** for Windows 10 & 11, written from scratch in **Rust** (`~1.70 MB` single portable executable). Inspired by Nusa & Layzee, CyNotch features a **Cyno (Genshin Impact)** Electro Violet & Graphite Obsidian aesthetic, hardware Direct2D + DirectWrite + triple-buffered DirectComposition rendering (`144Hz`-ready), and a built-in **Memory Reduct** engine for one-click and automated RAM optimization.

---

## 🌟 Highlights in v1.0.0

1. **⚡ 144Hz GPU-Cached Expand/Close & Triple-Buffered DirectComposition**
   - Pre-warmed offscreen `ID2D1Bitmap1` snapshot caching cuts expand/close frame times to **`2.4–3.5 ms`** (`3.7×` faster), paired with fixed-size DirectComposition blur visuals (`0` per-frame DWM texture reallocations), triple-buffered DXGI swapchains, and `7ms` high-res waitable-timer pacing.
2. **🐺 Pure Stealth Idle (Glowing Wolf Eyes) & Exclusive Capsule Hover Wave**
   - **2 Glowing Purple Wolf Eyes** in Minimal idle mode with `20 FPS` low-power breathing luminescence, natural blinks, and a `~650ms` predator squint on clipboard/capture events.
   - Smooth 3-stop violet optical hover-dwell wave strictly confined to the collapsed capsule (`0% → crest → 0%` opacity before the right edge) with self-healing dwell completion.
3. **🪟 Obsidian Liquid Glass & Graphite Preferences**
   - Fixed `15px` WinRT `GaussianBlurEffect` chained with **Optical Luminosity Clamp** (`src/dcomp_blur.rs`) so `#FFFFFF` light-theme documents maintain `~10.8:1` AAA contrast with `0%` CPU screen-capture.
   - Redesigned **CyNotch Preferences**: macOS-inspired neutral graphite obsidian grouped cards, Linear-style rows with leading glyphs & hairline separators, and CyNotch violet accents across all 4 sections.
4. **🎵 3-Box Floating Music UI, WinRT SMTC & LRCLIB Lyrics**
   - Separated floating glass boxes (Lyrics + Seek Bar, Track Info, Transport Controls) with monotonic SMTC progress clamping and zero-resize mode switching.
5. **📎 Native OLE Drag-and-Drop Quick Shelf & Clipboard Privacy**
   - Ephemeral **Quick Shelf** (`IDropTarget` / `IDropSource`) supporting files, PNG/DIB images, URLs, and text with `8s` Undo, `Win+Shift+S` `CF_DIB` capture, and Windows clipboard privacy checks that automatically skip password managers.
6. **🖱️ Locked Top-Center, Click-Through Default, Notifications & Safe Trim RAM**
   - Locked Top-Center (`5` core modes: `Home`, `Music`, `Volume`, `MemReduct`, `Shelf`), `WS_EX_TRANSPARENT` click-through when collapsed, `SHQueryUserNotificationState` fullscreen auto-suppress, auto-stashed notifications, conflict-free global hotkeys (`Ctrl+Alt+C` Toggle, `Ctrl+Alt+M` Purge, `Ctrl+Alt+S` Capture), and non-stuttering **Safe Trim** Memory Reduct.

---

## 📊 Benchmark & Comparison: v0.2.4.1 vs. v1.0.0

| Metric / Subsystem | CyNotch v0.2.4.1 (CPU Raster) | CyNotch v1.0.0 (GPU Direct2D + DComp) | Improvement |
| :--- | :--- | :--- | :--- |
| **Standalone Binary (`cynotch.exe`)** | `3.95 MB` (`3,949,056` B) | **`1.70 MB` (`1,704,448` B)** | **−56.8% smaller** |
| **Idle Capsule Frame Time** | `412.7 µs` (CPU `tiny-skia`) | **`89 µs` CPU / `235 µs` GPU** | **~4.6× faster** |
| **Expand / Close Frame Time (144Hz)** | `8.51 – 10.96 ms` (CPU) | **`2.4 – 3.5 ms` GPU cached quad** | **~3.7×–4.5× faster** |
| **Automated Test Coverage** | Basic unit tests | **`147/147` unit, D2D, soak & hover tests** | Production hardened |
| **Vector & Typography Stack** | `tiny-skia` + `fontdue` + GDI fallback | **Hardware Direct2D + DirectWrite Color Fonts** | Native sub-pixel AA + Bidi sanitizer |
| **Motion & Choreography** | Fixed exponential decay | **Damped springs + triple-buffered DComp + Win Animation gate** | 144Hz-ready, respects OS accessibility |
| **Notification Audio** | Multi-variant + manual volume | **Fixed 25% Snapchat Pop + burst deduplication** | Plays once per burst, auto-mutes Win beep |

---

## 📦 Download & Quick Start (Portable)

Pre-built Windows x64 binaries are available on the [**Releases**](https://github.com/muhnbl/CyNotch/releases) page:

1. Download `cynotch.exe` (standalone portable executable) or the portable archive from **v1.0.0**.
2. Run `cynotch.exe` (or `Start-CyNotch.bat`).
3. *(Optional)* Toggle **Launch on Startup** directly in CyNotch Preferences, or run `Enable-AutoStart.ps1` / `Disable-AutoStart.ps1`.

> **Requirements:** Windows 10 or Windows 11 (x64). Zero external dependencies or runtimes required.

---

## 🛠️ Building from Source

```powershell
git clone https://github.com/muhnbl/CyNotch.git
cd CyNotch
cargo build --release
.\target\release\cynotch.exe
```

### Automated Tests & GPU Benchmark
```powershell
cargo test
cargo test --release --test perf_bench_d2d -- --nocapture
```

---

## 🙏 Credits
- Inspired by **Nusa** & **Layzee**
- Character theme inspired by **Cyno** (Genshin Impact / HoYoverse)
- Lyrics sync powered by **LRCLIB**
