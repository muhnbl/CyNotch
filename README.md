# ⚡ CyNotch (General Mahamatra Dynamic Island for Windows)

**CyNotch** is an ultra-lightweight Dynamic Island & Ambient Command Hub application for Windows 10 & 11 written from scratch using **Rust**. Inspired by Nusa & Layzee, CyNotch adopts the **Cyno (Genshin Impact)** theme with an Electro Purple & Gold color palette, and includes a heavy-class memory-optimisation module **Memory Reduct** built in.

---

## 🌟 Main Features

### 1. ⚡ Memory Reduct Engine
- **Clean Areas:**
  - `[✓] Working Sets`: Clears the working set of all active processes via `K32EmptyWorkingSet`.
  - `[✓] System WS`: Trims the internal system working set memory.
  - `[✓] Standby List`: Optimises the standby memory cache.
- **When Above (Threshold Trigger):**
  - Options: `70%`, `80%`, `90%`.
  - Automatically performs cleaning when RAM usage exceeds the selected threshold.
- **Every (Interval Trigger):**
  - Options: `5 minutes`, `15 minutes`, `30 minutes`.
- **Instant Purge Button:**
  - **`[ ⚡ PURGE RAM (JUDGMENT) ]`** button for one-click instant cleaning.
  - Shows the total megabytes freed in real time.

### 2. 🐺 Cyno Mascot & Electro Sumeru Aesthetic
- **Cyno Mascot Silhouette:**
  - Wolf/feral-ear headdress (Anubis hood) typical of the General Mahamatra.
  - Silver/white layered hair with an electro gold headband.
  - Amber/gold eyes glowing with electro sparks.
- **Cyno's Signature Quote:** Provides dynamic status and dialogue (*"Judgment is passed! A heavy burden of RAM has been purged."*).
- **Color Palette:** Obsidian Violet (`#0D0A18`), Electro Violet (`#9333EA`, `#A855F7`), and Sacred Gold (`#F59E0B`).

### 3. 🎵 Smarter Media & LRCLIB Lyrics
- Integration of WinRT GSMTC (`GlobalSystemMediaTransportControlsSessionManager`) for Spotify, YouTube, Chrome, etc.
- Live lyric synchronization via the LRCLIB API (`lrclib.net`).
- Direct Play/Pause media control from the notch.

### 4. ⏱️ Pomodoro Focus Timer
- 25/5-minute focus timer with a circular progress bar and countdown status.

### 5. 📊 Real-time Hardware Telemetry
- Live monitoring of CPU load and RAM usage.

### 6. 📋 Quick Shelf
- Space for quick notes and an instant scratchpad.

---

## 🚀 Performance & Technical Architecture

- **Binary Size:** **3.84 MB** (release `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`).
- **CPU Usage when Idle:** **~4.6 % of one core** (A/B 3×30 seconds in *minimized island* active state; down from ~6.9 % before v2.5 optimisation).
- **Memory Footprint:** **~11.5 MB** working set after Windows trim (private ~64 MB, including DComp/D3D11 surface for *frosted glass*).
- **Rendering:** Direct2D / Win32 layered window with per-pixel alpha transparency via `tiny-skia` + Windows GDI typography.
- **Execution Location:** `C:\Users\muhna\.local\bin\cynotch.exe`.
- **Render Benchmark:** `cargo run --release --example bench_render` (optional `-- <iters> <state>` with states `minimized_vert`, `idle`, `idle_vert`, `home`, `lyrics`, `settings`, `glowmicro`).

### Optimisation Notes (v2.5)
Full details are in [`OPTIMIZATION_REPORT.md`](OPTIMIZATION_REPORT.md). Summary:

| Metric | Before | After |
| --- | --- | --- |
| CPU idle (1 core, average of 3 samples) | ~6.9 % | **~4.6 %** |
| Render capsule minimized | 249 µs/frame | **200 µs/frame** |
| Render idle capsule | 496 µs/frame | **413 µs/frame** |
| Render expanded Home | 9.90 ms/frame | **8.51 ms/frame** |
| Render Preferences (937×785) | 33.9 ms/frame | **20.4 ms/frame** |
| Binary size | 4.15 MB | **3.84 MB** |
