# CyNotch v1.0.0 — Motion System (Final Locked Spec)

> **Hybrid Architecture**:
> - **Shell & Notifications**: **iOS / macOS Dynamic Island** (coupled closed-form fluid springs, organic surface-tension settle, velocity-preserving interruption, coalesced replacement, Auto-Stash to Bell on expand).
> - **Dock, Shelf & Controls**: **Raycast Tactile** (snappy `70ms–140ms` critically-damped response, Mass Absorption drop, zero friction).
> - **Zero AI-Slop Invariant**: NO procedural sparks/particles, NO electric lightning arcs, NO multi-ring confetti, NO wobbly buoyancy bounce.

---

## 1. Physics & Timing Tokens

### 1.1 Closed-Form Coupled Springs (`Fluid Luxury` Tempo — ~1.6× Calmer)
Evaluated analytically from `(x0, v0, dt)` so `60Hz–240Hz` produce identical trajectories and mid-motion interruptions preserve velocity:

| Transition | Axis | `f` (Hz) | `ζ` (Damping) | Overshoot | Character |
| :--- | :--- | :---: | :---: | :---: | :--- |
| **Notif Open** (`136×34 → 368×64`) | Width / Height / Radius | `2.00` / `1.90` / `1.90` | `0.88` / `0.90` / `0.90` | `~0.3%` | Fluid Luxury organic expand (`~420ms` settle) |
| **Notif Coalesced Replace** | Width / Height velocity kick | `+140px/s` / `+26px/s` | `0.88` / `0.90` | subtle pulse | Updates toast in-place without close/reopen |
| **Notif / Shell Collapse** | Width / Height / Radius | `1.75` / `1.80` / `1.80` | `0.94` / `0.94` / `0.94` | `0%` | Calm critically-damped shrink (`~480ms` settle) |
| **Shell Expand** (`→ Expanded Notch`) | Width / Height / Radius | `2.15` / `2.05` / `2.05` | `0.88` / `0.88` / `0.88` | `~0.3%` | Smooth, unhurried hover expand (`~420ms` settle) |
| **Staged Capsule Choreography** | `anim_capsule_t` (`650ms`) | `0.0..0.42` exit / `0.30..1.0` enter | `220ms` shrink delay | `0%` | Roll-down exit + overlapped roll-in before capsule shrinks |

### 1.2 Unified Signature Appear Animation (`Blur-Fade Rise`) & Controls
Every section, card, bento tile, notification row, and Settings category pane shares one cohesive entrance signature:
- **Signature**: **`Blur-Fade Rise`** (`480ms`, quartic ease-out `1 - (1 - u)^4`, `0%` overshoot):
  - Vertical rise: `+12px → 0px`
  - Opacity dissolve: `0.0 → 1.0`
  - Optical scale: `0.96 → 1.0` (`0.97 → 1.0` for full Settings pane)
  - **Staggered Cascade**: `~45ms` (`0.095 × 480ms`) per card/tile/row.
- **Triggers**: Expanding notch from idle, switching notch tabs (`Home` / `Music` / `Volume` / `MemReduct` / `Shelf` / `Notifications`), new item entering Temporary Shelf (`Ctrl+C`, `Win+Shift+S`, drag-drop), and switching categories in Settings.

| Token | Duration | Curve | Overshoot | Used For |
| :--- | :--- | :--- | :---: | :--- |
| **`--blur-fade-rise`** | `480ms` | quartic ease-out (`1 - (1 - u)^4`) | `0%` | Unified section, card, shelf tile, notif row & Settings pane appear (`+12px → 0`, fade `0 → 1`, scale `0.96 → 1.0`) |
| **`--ray-pill`** | `~260ms` (`f = 3.2 Hz`) | critically damped spring | `0%` | Mode dock sliding indicator pill (`x` + `width`), settings sidebar pill |
| **`--ray-absorb`** | `380ms` | `ease-out` | `0%` | Shelf drop **Mass Absorption** (`+3.5px` vertical dip & recover) |
| **`--ray-refract`** | `600ms` | `ease-out cubic` | `0%` | Single crisp glass refraction hairline ring (`r: 6px → 84px`, alpha → `0`) |
| **`--ray-hover`** | `~550ms` in / `~380ms` out | exponential (`6.8` / `10.0`) | `0%` | Gentle bottom-up Amber-Gold liquid bloom inside glass buttons |
| **`--clipboard-pulse`** | `900ms` | lingering peak + snap-off | `0%` | Skenario 2 clipboard / Snipping Tool pulse without popup banner |

---

## 2. State Choreography & Bug Fixes

### 2.1 Notifications (iOS/macOS Island + Zero-Overlap Auto-Stash)
- **Single Continuous Shell**: `Idle Pill` ↔ `Notification` ↔ `Expanded Quick Shelf` morphs on one continuous rounded surface; content is laid out at final bounds, revealed by clip + smooth opacity/dissolve (never stretched or warped).
- **Zero-Overlap Auto-Stash on Expand**:
  - **Removed**: `draw_expanded_toast_overlay` floating box at `by = 6.0` (which collided with header & shelf).
  - **Replaced With**: Opening/hovering the notch while a notification is active **auto-stashes** it into the top-right **Bell icon** with an Amber notification pip (`#FBBF24`). Workspace stays `100%` unobstructed. Clicking the Bell peeks/restores it; collapsing the notch resumes remaining countdown cleanly.
- **Autonomous 30Hz Countdown Ring (Fix for Frozen `×` Timer)**:
  - Root cause fixed: `AppState::needs_animation_timer_ms()` in `src/models.rs` **must** return `Some(33)` whenever `self.notification.is_some()` (even when OS animation effects are off or mouse is stationary), so the `×` countdown progress arc ticks smoothly at `30 Hz` without requiring `WM_MOUSEMOVE`.

### 2.2 Temporary Shelf (OLE Drag-and-Drop)
- **Drag-Over**: `120ms` dwell auto-expand; subtle cursor radial lens (`rgba(216,180,254,0.14)`).
- **Drop Event (Mass Absorption)**:
  1. Shelf card dips `+3.5px` over `240ms` (`--ray-absorb`).
  2. Single soft hairline glass refraction ring (`380ms`), zero sparks, zero arcs.
  3. New item enters at top in `120ms` (`+6px → 0`), zero bounce.
  4. Audio: crisp acoustic water/wood tap (`360Hz → 115Hz`, `6ms` attack, `80ms` smooth decay to exact `0`).
- **Drag-Out & Undo**: Auto-removes on drag-out; `8s` Undo chip restores item instantly on click.

### 2.3 Mode Dock & Accessibility
- **Mode Dock**: Translucent active pill glides `140ms` across the 7 Heroicons (`0%` overshoot).
- **OS Animations OFF (`SPI_GETCLIENTAREAANIMATION == FALSE`)**: Springs & tweens snap to target in 1 frame; refraction ring skipped; countdown ring still updates via `33ms` tick.

---

## 3. Direct2D & High-Refresh Invariants
1. **Frame-Rate Independence**: Monotonic QPC `dt` clamped to `≤ 1/30s`.
2. **Zero Idle GPU (`0 Hz`)**: Once springs settle (`|x - target| < 0.04 && |v| < 0.4`) and `self.notification.is_none()` and no active FX remain, snap to integer device pixels and stop `TIMER_ANIMATION_ID`.
3. **Immutable Per-Key Brushes & Balanced Layers**: Never mutate Direct2D brushes mid-frame; strictly balance `PushLayer`/`PopLayer`.
