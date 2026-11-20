# ⚡ CyNotch — Performance & Architecture Optimization Report (v0.2.1 → v0.2.4.1 → v0.3.0)

This document records the measured performance benchmarks, rendering architecture evolution, and binary footprint reductions across **CyNotch v0.2.1**, **v0.2.4.1**, and **v0.3.0**.

---

## 1. v0.3.0 Hardware Direct2D + DirectWrite Migration & Dependency Elimination

In **v0.3.0**, CyNotch replaced its entire CPU software rasterization stack (`tiny-skia` + `fontdue` + `ureq` + `chrono`) with native Windows hardware acceleration and OS services:

1. **Direct2D (`ID2D1DeviceContext`) + DirectComposition DXGI SwapChain** (`src/composition.rs`, `src/painter_d2d.rs`):
   - Every rounded shell, gradient card, radial glow, progress arc, and vector icon renders directly on the GPU via cached `ID2D1SolidColorBrush`, `ID2D1LinearGradientBrush`, `ID2D1RadialGradientBrush`, and `ID2D1PathGeometry` resources (`D2DResourceCache`).
   - Automatic DXGI device-lost detection and transparent resource recreation.
2. **DirectWrite Typography & Bidi Sanitizer** (`src/font_d2d.rs`, `src/notifications.rs`):
   - Cached `IDWriteTextFormat` and `IDWriteTextLayout` (`DWriteTextCache`) with `D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT` for native multi-color emoji rendering.
   - Built-in Unicode formatting/bidi isolate filter (`is_invisible_format_char`) strips `FSI` (`U+2068`), `PDI` (`U+2069`), `LRI`/`RLI`, `LRM`/`RLM`, and `BOM` code points injected by Discord/WhatsApp so `Segoe UI Variable Text` never displays dotted-box sigla.
3. **Zero-Crate Win32 Networking, Time & Imaging**:
   - `ureq` + TLS stack replaced by native Win32 **WinHTTP** (`src/lyrics.rs`).
   - `chrono` replaced by Win32 **`GetLocalTime` / `GetTimeZoneInformation`** (`src/localtime.rs`).
   - PNG encoding/decoding handled by Win32 **Windows Imaging Component (WIC)** (`src/capture.rs`).
4. **True 0% Idle CPU Timer Hibernation** (`src/window.rs`, `src/settings_window.rs`, `src/models.rs`):
   - `needs_notch_timer_ms()` and `needs_settings_timer_ms()` return `None` once spring velocities (`spring_step`) and the screen-edge portal glow (`anim_edge_glow_t`) settle, calling `KillTimer` and releasing the high-resolution frame pacer slot so the UI thread consumes **0.0% CPU** at rest.

---

## 2. Head-to-Head Benchmark: v0.2.4.1 (CPU `tiny-skia`) vs. v0.3.0 (GPU `Direct2D`)

Measured on the same Windows 11 x64 machine using `cargo test --release --test perf_bench_d2d -- --nocapture` (40 timed iterations after 3 warm-up frames at `676×253` resolution) compared against the v0.2.4.1 `bench_render` baseline:

| Benchmark Scenario / Metric | v0.2.4.1 (CPU `tiny-skia`) | v0.3.0 (GPU `Direct2D`) | Speedup / Delta |
| :--- | :--- | :--- | :--- |
| **Idle Capsule Render (`676×253`)** | `412.7 µs` (CPU raster) | **`59.27 µs` CPU record / `122.64 µs` GPU flush** | **3.37× – 6.96× faster** |
| **Expanded Heavy Scene (`25× radial glow + cards + text`)** | `8.51 ms – 10.96 ms` | **`0.934 ms` CPU record / `1.081 ms` GPU flush** | **9.11× – 10.14× faster** |
| **Release Binary Size (`cynotch.exe`)** | `3,949,056 bytes` (`3.95 MB`) | **`1,662,976 bytes` (`1.66 MB`)** | **−57.89% (`−2.29 MB`)** |
| **Portable ZIP Package Size** | `1.82 MB` | **`0.88 MB`** | **−51.65%** |
| **Settled Idle UI Thread CPU** | `~2.63%` of 1 core | **`0.00%` (timers killed when settled)** | **−100% idle timer overhead** |
| **Automated Verification Suite** | `73 / 73` tests passing | **`93 / 93` unit + integration + QA swarm tests** | **+20 regression tests** |

---

## 3. Historical CPU Renderer Optimizations (v0.2.1 – v0.2.4.1 Reference)

Prior to the v0.3.0 Direct2D GPU migration, the following poll-thread and memory optimizations were established and remain active in v0.3.0:

- **`AudioMonitor` Endpoint Caching (`src/audio.rs`):** Caches the activated `IAudioEndpointVolume` COM interface and re-resolves endpoints every 3 seconds (or on read error), cutting poll cost from `~2.0 ms` to `~0.4 ms`.
- **SQLite WAL Signature Guard for Action Center Polling (`src/notifications.rs`):** Checks file modification signatures on `%LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db-wal` before invoking `GetNotificationsAsync`, eliminating `~25–35 ms` of WinRT RPC marshaling on idle ticks.
- **Collapsed Network Telemetry Gate (`src/telemetry.rs`):** Skips `GetIfTable2` kernel table enumeration when the notch is collapsed and `IdleMetric` is not `NetSpeed`.
- **Throttled ScreenTime Persistence (`src/screentime.rs`):** Flushes screen-time JSON to disk every 60 seconds instead of every poll tick and sorts top applications only while the ScreenTime view is active.
