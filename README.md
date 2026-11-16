# ⚡ CyNotch (General Mahamatra Dynamic Island for Windows)

**CyNotch** is an ultra-lightweight Dynamic Island & Ambient Command Hub application for Windows 10 & 11 written from scratch in **Rust**. Inspired by Nusa & Layzee, CyNotch adopts the **Cyno (Genshin Impact)** theme with an Electro Purple & Gold color palette, and includes a heavy-class memory-optimisation module **Memory Reduct** built in.

---

## 🌟 Main Features

### 1. ⚡ Memory Reduct Engine
- **Clean Areas:**
  - `[✓] Working Sets`: Clears the working set of all active processes via `K32EmptyWorkingSet`.
  - `[✓] System WS`: Trims the system's internal working set memory.
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
  - Silver/white hair layered with an electro gold headband.
  - Amber/gold eyes glowing with electro sparks.
- **Cyno's Signature Quote:** Provides dynamic status and dialogue (*"Judgment is passed! A heavy burden of RAM has been purged."*).
- **Color Palette:** Obsidian Violet (`#0D0A18`), Electro Violet (`#9333EA`, `#A855F7`), and Sacred Gold (`#F59E0B`).

### 3. 🎵 Smarter Media & LRCLIB Lyrics
- Integration with WinRT GSMTC (`GlobalSystemMediaTransportControlsSessionManager`) for Spotify, YouTube, Chrome, etc.
- Live lyric sync via the LRCLIB API (`lrclib.net`).
- Direct Play/Pause media control from the notch.

### 4. ⏱️ Pomodoro Focus Timer
- 25/5-minute focus timer with a circular progress bar and countdown status.

### 5. 📊 Realtime Hardware Telemetry
- Live monitoring of CPU load and RAM usage.

### 6. 📋 Quick Shelf
- Space for quick notes and an instant scratchpad.

---

## 🚀 Performance & Technical Architecture

- **Binary Size:** **3.84 MB** (release `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`).
- **CPU idle:** **~2.6 % of one core** (v2.6, measured end-to-end; v2.5: ~5.3 %).
- **Memory Footprint:** **~11.5 MB** working set after Windows trim (private ~64 MB, including DComp/D3D11 surface for *frosted glass*).
- **Rendering:** Direct2D / Win32 layered window with per-pixel alpha transparency via `tiny-skia` + Windows GDI typography.
- **Execution Location:** `C:\Users\muhna\.local\bin\cynotch.exe`.
- **Render Benchmark:** `cargo run --release --example bench_render` (optional `-- <iters> <state>` with states `minimized_vert`, `idle`, `idle_vert`, `home`, `lyrics`, `settings`, `glowmicro`).

### Optimization Notes (v2.6)
Full details are in [`OPTIMIZATION_REPORT_v2.6.md`](OPTIMIZATION_REPORT_v2.6.md). Summary:

| Metric | Before | After |
| --- | --- | --- |
| CPU idle — poll thread (1 core) | 3.05 % | **0.78–1.02 %** (-73 % to -76 %) |
| CPU idle — total app (1 core) | 5.29 % | **2.63 %** (-50 %, measured end-to-end) |
| Notification poll cost | every 1 s, 25–35 ms CPU | only when Action Center changes |
| Test | 41 | **44** |

The Action Center poll no longer runs blind every second: the app watches the Windows notification database file timestamp (≈microseconds) and only performs the expensive poll when its contents change, with a **forced poll every 5 seconds** as a safety net. The `NotificationChanged` event cannot be used — the platform rejects it for unpackaged apps (`0x80070490`).

### Optimization Notes (v2.5)
Full details are in [`OPTIMIZATION_REPORT.md`](OPTIMIZATION_REPORT.md). Summary:

| Metric | Before | After |
| --- | --- | --- |
| CPU idle (1 core, average of 3 samples) | ~6.9 % | **~4.6 %** |
| Render capsule minimized | 249 µs/frame | **200 µs/frame** |
| Render idle capsule | 496 µs/frame | **413 µs/frame** |
| Render expanded Home | 9.90 ms/frame | **8.51 ms/frame** |
| Render Preferences (937×785) | 33.9 ms/frame | **20.4 ms/frame** |
| Binary size | 4.15 MB | **3.84 MB** |
