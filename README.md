# ⚡ CyNotch — General Mahamatra Dynamic Island for Windows

**CyNotch** is an ultra-lightweight **Dynamic Island & Ambient Command Hub** for Windows 10 & 11, written from scratch in **Rust**. Inspired by Nusa & Layzee, CyNotch features a **Cyno (Genshin Impact)** Electro Purple & Sacred Gold aesthetic and ships with a built-in **Memory Reduct** engine for one-click and automated RAM optimization.

---

## 🌟 Features

### 1. ⚡ Built-in Memory Reduct Engine
- **Clean Areas:**
  - **Working Sets:** Trims active process working sets via `K32EmptyWorkingSet`.
  - **System WS:** Trims internal system working set memory.
  - **Standby List:** Optimizes standby memory cache.
- **Threshold Trigger ("When Above"):** Automatically cleans RAM when usage exceeds `70%`, `80%`, or `90%`.
- **Interval Trigger ("Every"):** Scheduled cleanup every `5 minutes`, `15 minutes`, or `30 minutes`.
- **Instant Purge Button:** One-click **`[ ⚡ PURGE RAM (JUDGMENT) ]`** with real-time reporting of freed megabytes.

### 2. 🐺 Cyno Mascot & Electro Sumeru Aesthetic
- **Custom Vector Mascot:** Anubis-hood silhouette, layered silver/white hair with electro gold headband, and glowing amber eyes with animated electro sparks.
- **Dynamic Dialogue:** Cyno's signature status lines (*"Judgment is passed! A heavy burden of RAM has been purged."*).
- **Color Palette:** Obsidian Violet (`#0D0A18`), Electro Violet (`#9333EA`, `#A855F7`), and Sacred Gold (`#F59E0B`).

### 3. 🎵 Smarter Media & LRCLIB Lyrics
- **WinRT GSMTC Integration** (`GlobalSystemMediaTransportControlsSessionManager`) for Spotify, YouTube, Chrome, and other Windows media players.
- **Live Synced Lyrics** powered by the [LRCLIB](https://lrclib.net) API.
- **Inline Controls:** Play/Pause directly from the notch.

### 4. ⏱️ Pomodoro Focus Timer
- 25/5-minute focus timer with a circular progress indicator and live countdown.

### 5. 📊 Real-Time Hardware Telemetry
- Live CPU load, RAM usage, and network speed monitoring.

### 6. 📋 Quick Shelf
- Instant scratchpad for quick notes right inside the expanded hub.

---

## 📦 Download & Quick Start (Portable)

Pre-built Windows x64 binaries are available on the [**Releases**](https://github.com/muhnbl/CyNotch/releases) page:

1. Download either `CyNotch-v0.2.1-portable.exe` (standalone executable) or `CyNotch-v0.2.1-portable-win64.zip` (includes helper scripts).
2. Extract (if using `.zip`) and run `cynotch.exe` (or `Start-CyNotch.bat`).
3. *(Optional)* Run `Enable-AutoStart.ps1` to launch CyNotch automatically on Windows startup, or `Disable-AutoStart.ps1` to remove it.

> **Requirements:** Windows 10 or Windows 11 (x64). No external runtime required.

---

## 🛠️ Building from Source

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (Edition 2024, MSVC toolchain on Windows)

### Build & Run
```powershell
git clone https://github.com/muhnbl/CyNotch.git
cd CyNotch
cargo build --release
.\target\release\cynotch.exe
```

### Tests & Benchmarks
```powershell
cargo test --release
cargo run --release --example bench_render -- 150
```

Optional benchmark arguments: `cargo run --release --example bench_render -- <iters> <state>` where `<state>` can be `minimized_vert`, `idle`, `idle_vert`, `home`, `lyrics`, `settings`, or `glowmicro`.

---

## 🚀 Tech Stack & Performance

- **Language:** Rust (Edition 2024)
- **Rendering:** Win32 Layered Window + DirectComposition/D3D11 frosted glass + `tiny-skia` vector rendering + `fontdue` / Windows GDI typography
- **Windows APIs:** `windows` & `windows-core` crates (WinRT GSMTC, UserNotificationListener, CoreAudio, IP Helper, Registry, Memory Management)
- **Networking & Serialization:** `ureq`, `serde`, `serde_json`, `chrono`
- **Binary Size:** **~3.8 MB** (`opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`)
- **Idle CPU Usage:** **~2.6% of one core** (v0.2.1 / v2.6 architecture; down from ~6.9% baseline)
- **Memory Footprint:** **~11.5 MB** working set after Windows trim

See [`OPTIMIZATION_REPORT.md`](OPTIMIZATION_REPORT.md) for detailed profiling and A/B benchmark results.

---

## 🙏 Credits
- Inspired by **Nusa** & **Layzee**
- Character theme inspired by **Cyno** (Genshin Impact / HoYoverse)
- Lyrics sync powered by **LRCLIB**
