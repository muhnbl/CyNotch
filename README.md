# ⚡ CyNotch — General Mahamatra Dynamic Island for Windows

**CyNotch** is an ultra-lightweight, GPU-accelerated **Dynamic Island & Ambient Command Hub** for Windows 10 & 11, written from scratch in **Rust** (`~1.66 MB` single portable executable). Inspired by Nusa & Layzee, CyNotch features a **Cyno (Genshin Impact)** Electro Violet aesthetic, hardware Direct2D + DirectWrite rendering, a physical screen-edge portal glow, and a built-in **Memory Reduct** engine for one-click and automated RAM optimization.

---

## 🌟 Highlights in v0.3.0

1. **⚡ Hardware GPU Direct2D + DirectWrite Engine (`1.66 MB` Binary)**
   - Complete migration from CPU `tiny-skia`/`fontdue` to hardware **Direct2D (`ID2D1DeviceContext`) + DirectWrite (`DWriteTextCache`)** bound directly to the DirectComposition DXGI SwapChain.
   - **~10× faster expanded rendering:** Expanded frame time dropped from `8.51–10.96 ms` (CPU software rasterization in v0.2.4.1) to **`0.93 ms` CPU record / `1.08 ms` GPU flush** (`59 µs` CPU / `123 µs` GPU for idle capsule).
   - **57.9% smaller binary:** Replaced `tiny-skia`, `fontdue`, `ureq`, and `chrono` with native Win32 APIs (`WinHTTP`, `WIC`, `GetLocalTime`), shrinking `cynotch.exe` from `3.95 MB` (v0.2.4.1) to **`1.66 MB` (`1,662,976` bytes)**.
2. **🌌 Physical Screen-Edge Portal Glow & Framer-Motion Springs**
   - Borderless multi-layer radial violet portal aura hugging the physical monitor bezel (`Top`, `Left`, `Right`) with 5th-order smootherstep opening and a ~1.2s atmospheric dissipation fade-out.
   - Configurable **Hover Delay** (`Quick 120ms`, `Normal 240ms`, `Slow 450ms`) and spring choreography automatically gated by Windows *Animation Effects* (`SPI_GETCLIENTAREAANIMATION`).
3. **🔔 Windows Notifications Bridge & Burst-Deduplicated Snapchat Chime**
   - Streams live Windows Action Center toasts (Discord, WhatsApp, Browser) into the notch with Unicode bidi isolate filtering (eliminating `[FSI]` / `[PDI]` glyph boxes) and full color emoji support.
   - Gentle **fixed 25% Snapchat Pop chime** that plays once per notification burst and automatically mutes/restores native Windows toast sounds to prevent double-beeping.
4. **🎚️ Live Frosted Glass Sliders & 1:1 Sub-Pixel Scrubbers**
   - Continuous drag sliders for **Glass Opacity** (`Airy`..`Solid`) and **Blur Intensity** (`Soft`..`Ultra`) plus 1:1 sub-pixel Music seek & Volume Master scrubbers.
5. **🧹 Built-in 8-Area Memory Reduct Engine**
   - Trims Working Sets (`K32EmptyWorkingSet`), System Working Set, Standby List, Modified Page List, and optional kernel caches on demand (`Ctrl+Shift+M`), by RAM threshold (`60%`–`90%`), or on a schedule (`5m`–`90m`).

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

1. Download either `CyNotch-v0.3.0-portable.exe` (standalone `1.66 MB` executable) or `CyNotch-v0.3.0-portable-win64.zip` (includes helper scripts).
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
