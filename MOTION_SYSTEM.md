# CyNotch v0.3.1 — Motion System (Final Locked Spec)

> **Hybrid Architecture**:
> - **Shell & Notifications**: **iOS / macOS Dynamic Island** (coupled closed-form fluid springs, organic surface-tension settle, velocity-preserving interruption, coalesced replacement, Auto-Stash to Bell on expand).
> - **Dock, Shelf & Controls**: **Raycast Tactile** (snappy `70ms–140ms` critically-damped response, Mass Absorption drop, zero friction).
> - **Zero AI-Slop Invariant**: NO procedural sparks/particles, NO electric lightning arcs, NO multi-ring confetti, NO wobbly buoyancy bounce.

---

## 1. Physics & Timing Tokens

### 1.1 Closed-Form Coupled Springs (Shell & Notifications — iOS/macOS Feel)
Evaluated analytically from `(x0, v0, dt)` so `60Hz–240Hz` produce identical trajectories and mid-motion interruptions preserve velocity:

| Transition | Axis | `f` (Hz) | `ζ` (Damping) | Overshoot | Character |
| :--- | :--- | :---: | :---: | :---: | :--- |
| **Notif Open** (`136×34 → 368×64`) | Width / Height / Radius | `3.10` / `2.90` / `2.90` | `0.86` / `0.88` / `0.88` | `~0.5%` | Claude Fast snappy expand (`~250ms` settle) |
| **Notif Coalesced Replace** | Width / Height velocity kick | `+140px/s` / `+26px/s` | `0.86` / `0.88` | subtle pulse | Updates toast in-place without close/reopen |
| **Notif / Shell Collapse** | Width / Height / Radius | `2.75` / `2.85` / `2.85` | `0.92` / `0.92` / `0.92` | `0%` | Smooth critically-damped shrink (`~300ms` settle) |
| **Shell Expand** (`→ Quick Shelf`) | Width / Height / Radius | `3.40` / `3.20` / `3.20` | `0.86` / `0.86` / `0.86` | `~0.5%` | Snappy user-triggered hover expand (`~240ms` settle) |
| **Staged Capsule Choreography** | `anim_capsule_t` (`420ms`) | `0.0..0.42` exit / `0.30..1.0` enter | `220ms` shrink delay | `0%` | Roll-down exit + overlapped roll-in before capsule shrinks to Wolf Eyes |

### 1.2 Raycast Tactile Tweens (Controls, Dock & Shelf)
| Token | Duration | Curve | Overshoot | Used For |
| :--- | :--- | :--- | :---: | :--- |
| **`--ray-press`** | `70ms` | `cubic-bezier(0.16, 1, 0.3, 1)` | `0%` | Button / row `:active` micro-press (`scale(0.975)` + `1.12x` brightness) |
| **`--ray-fast`** | `120ms` | `cubic-bezier(0.16, 1, 0.3, 1)` | `0%` | Shelf item entry (`+6px → 0`, `opacity 0 → 1`), row hover, content fade |
| **`--ray-pill`** | `140ms` | `cubic-bezier(0.16, 1, 0.3, 1)` | `0%` | Mode dock sliding indicator pill (`x` + `width`), settings segmented pill |
| **`--ray-absorb`** | `240ms` | `cubic-bezier(0.16, 1, 0.3, 1)` | `0%` | Shelf drop **Mass Absorption** (`+3.5px` vertical dip & recover) |
| **`--ray-refract`** | `380ms` | `ease-out cubic` | `0%` | Single crisp glass refraction hairline ring (`r: 6px → 84px`, alpha → `0`) |
| **`--ray-hover`** | `420ms` in / `280ms` out | `cubic-bezier(0.22, 1, 0.36, 1)` | `0%` | Gentle bottom-up Amber-Gold liquid bloom inside glass buttons |

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
