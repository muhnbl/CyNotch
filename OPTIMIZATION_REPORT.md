# ⚡ CyNotch — Performance Optimization Report

This document summarizes the **performance optimization** work on **CyNotch**.

**Scope:** Performance only. No features were added, removed, or behaviorally altered. All visuals, animations, dimensions, and interactions remain identical — verified via pixel-regression testing (see §4).

---

## 1. Methodology

Optimizations were driven by measurements rather than assumptions:

1. **Measure real bottlenecks first.** Temporary `std::time::Instant` instrumentation was added to the poll thread, window thread (`WM_TIMER`), and each `Painter` drawing primitive, then removed once data was collected.
2. **Permanent benchmark harness** added in [`examples/bench_render.rs`](examples/bench_render.rs) so every change can be measured in seconds (`cargo run --release --example bench_render -- 150`), covering *minimized* capsule, horizontal/vertical idle, expanded Home, lyrics, and Preferences.
3. **Real A/B builds.** Baseline and optimized binaries were compiled with identical release profiles and benchmarked side by side on the same machine.
4. **Visual verification.** `--snapshot` mode generated 46 PNGs across all states, compared per-pixel against the baseline.

---

## 2. Key Profiling Findings

| Area | Finding |
| --- | --- |
| Idle render | One *minimized* capsule frame = **~250 µs** across ~50 vector ops. Cyno mascot alone accounted for ~95 µs; shell + chrome ~120 µs. |
| Expanded / hover | One frame = **9.9–13 ms**. Cost breakdown: masked *radial glow* 25× × ~190 µs = **~50% of frame**, card gradients ~25%, strokes ~14%. |
| Shading | `tiny-skia` shader cost is per-pixel: radial gradient **~10 ns/px** vs solid **~2.4 ns/px**. Cost scales with **shaded area**, not call count. |
| Preferences | 937×785 panel took **~34 ms/frame**, with 3 ~840 px-wide *aurora glows* unchanged between frames. |
| Poll thread | `audio.read_master_volume()` **~2.0 ms/poll** (endpoint enumeration + `Activate` every 500 ms); `notif.poll_new_toasts()` **~60 ms blocked/poll** (marshaling Action Center via RPC); media 1.3 ms; telemetry 1.2 ms. |
| Per-frame allocations | Multi-MB Preferences `Pixmap` + `Mask` reallocated every frame; `Vec<Hit>` per frame; clip mask cleared across the entire canvas on every `prepare_*_clip` (~21×/frame in expanded state); pixmap clear wrote full rows instead of dirty columns. |
| Build | Without `[profile.release]`: 4.15 MB, no LTO/codegen-units/strip. |

---

## 3. Applied Optimizations

### 3.1 Rendering — Shade Only Visible Regions
[`src/painter.rs`](src/painter.rs)

- `radial_glow` / `elliptical_glow` now compute the **minimum required rect**: intersecting the gradient bounding box with the active clip mask bounding box (`Painter::clip_bounds`), then filling via `fill_rect` (fast scan path, no AA) instead of an anti-aliased circle `fill_path`. **Lossless**: glow alpha reaches 0 at its radius, so clipping to the active mask removes zero visible pixels.
- `prepare_corners4_clip` no longer clears the entire canvas. The previous clip bounding box is tracked per mask pointer (thread-local) and only that region is cleared; freshly allocated masks skip clearing altogether (~21 full-canvas memsets saved per expanded frame).

### 3.2 Renderer — Allocations & Memory Bandwidth
- Clear pixmap only across **dirty columns** rather than full canvas rows.
- Reuse `Vec<Hit>` across frames (`mem::take`) instead of reallocating.

### 3.3 Preferences — Eliminate Identical Per-Frame Work
[`src/settings_window.rs`](src/settings_window.rs)

- Outer shell + 3 *aurora glows* + bevel stroke are **pixel-identical** while window size, frosted mode, and type scale are unchanged. Cached once keyed by `(width, height, frosted, type_scale)` and restored via `memcpy`.
- Multi-MB *scroll pane* (`Pixmap` + `Mask`) reused via thread-local scratch buffers.
- Skipped `pixmap.fill(TRANSPARENT)` when the chrome cache is hit (eliminates a 3 MB memset per frame).

### 3.4 Poll Thread — Reduce Redundant COM/WinRT & RPC Work
[`src/audio.rs`](src/audio.rs), [`src/main.rs`](src/main.rs)

- `AudioMonitor` caches the activated `IAudioEndpointVolume` and re-resolves endpoints every **3 seconds** (or on read failure), cutting cost from **~2.0 ms → ~0.4 ms per poll** while preserving real-time volume and device-switch detection.
- Windows notification polling (`GetNotificationsAsync().get()`) is file-timestamp-watched and only performs the expensive Action Center poll when the notification database changes (with a 5-second safety-net fallback), cutting idle poll-thread CPU by **~73–76%**.

### 3.5 Release Build Profile
[`Cargo.toml`](Cargo.toml)

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "unwind"      # kept intentionally for catch_unwind in wnd_proc
strip = "symbols"
```

---

## 4. Benchmark Results

### 4.1 Per-Frame Render Time (A/B, Same Machine, 150 Iterations)

| State | Before | After | Change |
| --- | --- | --- | --- |
| *Minimized* capsule (right, vertical) | 248.6 µs | **199.9 µs** | −19.6% |
| Idle capsule (horizontal) | 495.8 µs | **412.7 µs** | −16.8% |
| Idle capsule (vertical) | 618.2 µs | **529.8 µs** | −14.3% |
| Expanded Home (hover) | 9.90 ms | **8.51 ms** | −14.0% |
| Expanded Music + Lyrics | 13.09 ms | **10.96 ms** | −16.3% |
| Preferences panel (937×785) | 33.9 ms | **20.4 ms** | **−39.8%** |
| Chrome only (mascot off) | 133.0 µs | 120.6 µs | −9.3% |

### 4.2 Idle CPU & Binary Size

| Metric | Baseline | After v2.5 | After v2.6 (v0.2.1) |
| --- | --- | --- | --- |
| Idle CPU (1 core) | ~6.9% | ~4.6% | **~2.63%** |
| Binary size (`cynotch.exe`) | 4.15 MB | **3.84 MB** | **~3.8 MB** |
| Working set (after Windows trim) | ~24 MB | **~11.5 MB** | **~11.5 MB** |

### 4.3 Code Quality & Regression Checks
- `cargo clippy --all-targets --release -- -D warnings`: **0 warnings, 0 errors**.
- `cargo test --release`: **44/44 passing**.
- **Visual regression:** 46 PNG snapshots compared per-pixel against baseline with zero visible regressions.
