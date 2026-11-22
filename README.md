# ⚡ CyNotch — General Mahamatra Dynamic Island for Windows

**CyNotch** is an ultra-lightweight, GPU-accelerated **Dynamic Island & Ambient Command Hub** for Windows 10 & 11, written from scratch in **Rust** (`~1.66 MB` single portable executable). Inspired by Nusa & Layzee, CyNotch features a **Cyno (Genshin Impact)** Electro Violet aesthetic, hardware Direct2D + DirectWrite rendering, a physical screen-edge portal glow, and a built-in **Memory Reduct** engine for one-click and automated RAM optimization.

---

## 🌟 Highlights in v0.3.1

1. **🐺 Pure Stealth Idle (Glowing Wolf Eyes) & Staged Choreography**
   - Replaced static idle text with **2 Glowing Purple Wolf Eyes** featuring subtle breathing luminescence, natural blinks, and a `~650ms` predator squint on clipboard/capture events.
   - Added **Staged Choreography** (`420ms` roll-down exit + overlapped roll-in + `220ms` shrink delay) for Music → Idle and Song A → Song B transitions.
2. **🪟 Obsidian Liquid Glass & Optical Luminosity Clamp**
   - Fixed `24px` WinRT `GaussianBlurEffect` chained with **Optical Luminosity Clamp** (`src/dcomp_blur.rs`) so `#FFFFFF` light-theme documents maintain `~10.8:1` AAA contrast with `0%` CPU screen-capture.
   - Borderless (`0px` stroke) elevated cards, bottom-to-top faded rims on interactive controls, and clean solid Obsidian fallback when Windows Transparency Effects are off.
3. **🔔 Dynamic Island Notifications, Auto-Stash & 30Hz Countdown**
   - Opening the notch while a notification is active **auto-stashes** it into the Bell icon (`#FBBF24` pip) with zero workspace overlap; the `×` countdown ring ticks autonomously at `30 Hz`.
4. **📎 Native OLE Drag-and-Drop Shelf & Snipping Tool Capture**
   - Ephemeral **Quick Shelf** (`IDropTarget` / `IDropSource`) supporting files, PNG/DIB images, URLs, and text with `8s` Undo, `Win+Shift+S` `CF_DIB` clipboard capture, and Raycast Mass Absorption + procedural water-plop feedback.
5. **🖱️ True Click-Through Default, Grip-Only Drag & Side Dock (`-90° / +90°`)**
   - Collapsed notch uses `WS_EX_TRANSPARENT` so clicks pass through to apps behind it; dragging is scoped to the 6-dot grip handle, with `300ms / 650ms / 1000ms` Hover Dwell presets and rotated Side Dock stealth/music capsules.
6. **⚡ Hardware GPU Direct2D + DirectWrite Engine & 8-Area Memory Reduct**
   - Built on `ID2D1DeviceContext` + DirectWrite + DirectComposition (`~0.93 ms` CPU / `1.08 ms` GPU expanded frame, `0 Hz` idle), plus an 8-area Memory Reduct engine (`Ctrl+Shift+M`).

---

## 📊 Benchmark & Comparison: v0.2.4.1 vs. v0.3.0

| Metric / Subsystem | CyNotch v0.2.4.1 (CPU Raster) | CyNotch v0.3.0 (GPU Direct2D) | Improvement |
| :--- | :--- | :--- | :--- |
| **Standalone Binary (`cynotch.exe`)** | `3.95 MB` (`3,949,056` B) | **`1.66 MB` (`1,662,976` B)** | **−57.9% smaller** |
| **Portable ZIP Archive** | `1.82 MB` | **`0.88 MB`** | **−51.6% smaller** |
| **Idle Capsule Frame Time** | `412.7 µs` (CPU `tiny-skia`) | **`59.3 µs` CPU / `122.6 µs` GPU** | **3.4×–7.0× faster** |
| **Expanded Hub / Heavy Frame Time** | `8.51 – 10.96 ms` (CPU) | **`0.93 ms` CPU / `1.08 ms` GPU** | **~9.8× faster** |
| **Idle Settled CPU Usage** | `~2.6%` of 1 core | **`0.0%` (timers fully hibernated)** | **Zero idle timer wakeups** |
| **Vector & Typography Stack** | `tiny-skia` + `fontdue` + GDI fallback | **Hardware Direct2D + DirectWrite Color Fonts** | Native sub-pixel AA + Bidi sanitizer |
| **Hover Edge Glow** | Static border stroke on capsule | **Volumetric bezel portal aura + 1.2s fade-out** | Seamless physical screen-edge bleed |
| **Motion & Choreography** | Fixed exponential decay | **Framer-Motion damped springs + Win Animation gate** | Respects OS accessibility & refresh rate |
| **Notification Audio** | Multi-variant + manual volume | **Fixed 25% Snapchat Pop + burst deduplication** | Plays once per burst, auto-mutes Win beep |

---

## 📦 Download & Quick Start (Portable)

Pre-built Windows x64 binaries are available on the [**Releases**](https://github.com/muhnbl/CyNotch/releases) page:

1. Download either `CyNotch-v0.3.1-portable.exe` (standalone portable executable) or `CyNotch-v0.3.1-portable-win64.zip` (includes helper scripts).
2. Extract (if using `.zip`) and run `cynotch.exe` (or `Start-CyNotch.bat`).
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
