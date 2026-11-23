# CyNotch v1.0.0 — Obsidian Liquid Glass Design System (Approved)

> **Architecture**: Atomic Design Hierarchy (`Tokens → Atoms → Molecules → Organisms → Templates/Pages`)
> **Material**: Fixed Calibrated Translucent Obsidian Glass (`CALIBRATED_BLUR_PX = 15`, zero opacity/blur sliders)
> **Canvas & Placement**: Locked Top-Center (`NotchPlacement::Top`, non-draggable) · `648 × 292` expanded base bounds (`Shelf` Hug Contents auto-height up to 6 rows / 24 items) · 4pt/8pt spatial grid (`24px` outer margins, `16px` card gaps) · Settings `900 × 760` (`20px` card gaps)
> **Rule of Affordance**: Windows, Cards, and Sections are **borderless** (`0px` outer stroke, separated purely by surface luminance or real transparent gaps). **Bottom-to-top faded outlines (`~50% → 0%`) belong exclusively to interactive buttons & rows.**
> **Icons**: Clean Tailwind / Heroicons SVG strokes (`1.8–2.0px`), zero raw emojis.

---

## 1. Foundations & Design Tokens

### 1.1 Color Palette
| Role | Token | Value | Usage |
| :--- | :--- | :--- | :--- |
| **Primary Surface** | `--obsidian-top` | `rgba(24, 26, 44, 0.42)` | Top translucent shell & card veil (~80% visual base) |
| **Primary Deep** | `--obsidian-bot` | `rgba(20, 14, 40, 0.44)` | Deep translucent shell base |
| **Primary Luminous** | `--violet-core` | `#7C3AED` (`rgba(124, 58, 237, 0.20–0.34)`) | Translucent Primary Fill button & card bottom-bloom |
| **Primary Orchid** | `--violet-bloom` | `#A855F7` (`rgba(168, 85, 247, 0.22–0.28)`) | Bottom ambient light catch & refraction wave |
| **Secondary Ink 1** | `--silver-1` | `#F8F7FF` | Primary labels, headlines, transport buttons (~15% clarity) |
| **Secondary Ink 2** | `--silver-2` | `#C7C2DF` | Secondary descriptions, timestamps, metadata |
| **Secondary Ink 3** | `--silver-3` | `#9E98BD` | Muted placeholders, inactive dock icons |
| **Accent (Brushed Metallic Gold)** | `--gold-deep` | `#945604` | Deep burnished 24k gold shadow stop |
| **Accent (Brushed Metallic Gold)** | `--gold-bronze` | `#D68608` | Rich saturated burnished metallic gold |
| **Accent (Brushed Metallic Gold)** | `--gold-mid` | `#F8B610` | High-saturation radiant metallic gold (~5% global support/accent) |
| **Accent (Brushed Metallic Gold)** | `--gold-champagne` | `#FFE248` | Electric luminous gold highlight, 3D metallic bars, hover bloom |
| **Accent (Brushed Metallic Gold)** | `--gold-shimmer` | `#FFFCE0` | Incandescent metallic specular glint & toggle jewel pinpoint |
| **Semantic Status: Correct** | `--status-correct` | `#10B981` | Emerald Green (success / copied / verified) |
| **Semantic Status: Warning** | `--status-warning` | `#F8B610` | Brushed Metallic Gold (warning / attention) |
| **Semantic Status: Alert** | `--status-alert` | `#DC2626` | Crimson Red (error / critical / muted) |

### 1.2 Fixed Calibrated Glass Recipe (No Sliders · Optical Luminosity Clamp)
- **Backdrop Blur + Luminosity Clamp**: Fixed `15px` GaussianBlur + **Optical Luminosity Compression** (`brightness(0.46) contrast(1.12) saturate(165%)` in CSS; `GaussianBlurEffect(15px)` → `ColorMatrixEffect` / `BlendEffect::Luminosity` in WinRT Composition `src/dcomp_blur.rs`). Compresses `#FFFFFF` PDF/light-theme documents behind the notch into deep smoked obsidian (`~10.8:1` WCAG AAA contrast) while keeping airy translucency over dark wallpapers — **0% CPU screen-capture**.
- **Outer Window Shell**: Borderless (`0px` stroke). Translucent smoked obsidian gradient (`rgba(18,17,34,0.56)` → `rgba(11,11,24,0.52)` → `rgba(20,13,38,0.58)`) + subtle `rgba(124, 58, 237, 0.28)` bottom bloom + ambient shadow `0 26px 58px rgba(3, 2, 12, 0.46)`.
- **Inner Card / Section**: Borderless (`0px` stroke). Translucent glass veil + soft pearlescent bottom-left bloom + luminous violet bottom sweep + soft elevation shadow `0 12px 30px rgba(4, 2, 16, 0.32)`.
- **Text Legibility Guarantee**: Ultra-subtle button text shadow `0 1px 3px rgba(8, 4, 20, 0.05)` (`5%` opacity).
- **Button Outline Opacity**: Max `~50%` at bottom edge (`rgba(255,255,255,0.50)` / `rgba(233,213,255,0.48)`), smoothly fading to `0%` at top.
- **Hover Micro-Interaction (Gentle Amber Liquid Glow)**: Zero outer glow halo, zero hard streak/slash. On hover, a soft diffused warm Amber-Gold liquid bloom rises smoothly inside the glass (`rgba(253,230,138,0.28) → transparent`).
- **Exclusive Windows Transparency Gate**: When Windows Transparency Effects = ON (`windows_transparency_enabled == true`), **100% of windows, cards, sections, docks, rows, and buttons stay `< 100%` translucent** with live `15px` blur. When OS Transparency = OFF, cleanly falls back to solid Purple Obsidian surfaces (`#161728 → #0e0f1c`).
- **Anti-Aliasing Invariant (Zero Pixelation)**: Direct2D `D2D1_ANTIALIAS_MODE_PER_PRIMITIVE` + `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`, subpixel-aligned strokes, never clipping rounded corners with 1-bit GDI `SetWindowRgn`.

### 1.3 Typography & Radii
- **Font**: `Segoe UI Variable` / `Segoe UI` (DirectWrite).
- **Scale**: Title `14px Bold`, Section `13px Semibold`, Body/Row `11.5px Medium`, Micro/Sub `10.5px Regular`.
- **Radii**: Window Shell `28px`, Inner Card `22px`, Shelf Row `12px`, Buttons/Pills `9999px` (full capsule).

---

## 1.4 Settings Window — Graphite + CyNotch violet (v0.3.1+)

The Preferences window uses a **separate local palette** (`cyno_tk` in `src/settings_window.rs`) that pairs macOS-inspired calm graphite with the signature CyNotch violet accent. The notch itself keeps the calibrated Obsidian Liquid Glass identity above. This is an explicit product decision, not drift:

- Surfaces: neutral graphite (`#28282C` → `#1C1C20`), flat grouped-list cards (`#2C2C30`) with a 1px hairline rim, no aurora bloom or heavy shadow (`apple_section_card`).
- Accent: CyNotch violet `#7C3AED` (`accent`), deep indigo `#3E188A` (`accent_deep`), orchid `#A855F7` (`accent_hi`), pearlescent `#E9DAFF` (`accent_mist`). Used for selected pills, toggles, active sidebar item, and the settings glow.
- Text: `#F5F5F7` primary, `#A1A1A6` secondary.
- Sidebar: flat, no enclosing card, quiet 1px divider; keyboard shortcuts as plain footer text.
- Copy: plain human language, no engineer jargon (`Not locked`, `Centered`, `Shrink to a slim bar when idle`).
- The mascot orb in the header keeps its violet/gold identity as the bridge to the notch.

Rendering helpers `luminous_pill_button_t` and `draw_toggle_switch` in `painter_d2d.rs` honour this local palette for settings calls only; the notch draw path is untouched. This accent is applied consistently across every settings state, window and button.

## 2. Atomic Design Hierarchy

### Atoms
1. **Primary Fill / Selected Button (Airy Translucent Glass)**: `rgba(155, 92, 255, 0.14)` → `rgba(124, 58, 237, 0.20)` → `rgba(192, 132, 252, 0.22)` luminous crystal violet glass (never opaque solid fill) + **1px bottom-to-top faded outline (`0.50 → 0.0`)**.
2. **Primary Outline Button**: Ultra-clear glass (`rgba(255,255,255,0.02)` → `rgba(139,92,246,0.12)`) + **1px bottom-to-top faded outline (`0.48 → 0.0`)**.
3. **Text-Only Button (Ghost)**: Zero fill, zero outline. Hover rises a **pure bottom-to-top gentle liquid glow** (`0 → 1`).
4. **Circle Icon Button**: Translucent obsidian glass sphere with `~48%` bottom-to-top faded rim + Heroicon SVG. Bell button displays an `#FBBF24` amber notification pip when a toast is auto-stashed.
5. **Dismiss Countdown Button (`×`)**: Circle Icon Button wrapped in a `1.9px` rounded-cap lavender progress arc (`#C084FC`) updated autonomously at `30 Hz`.
6. **Toggle Switch**: Translucent Primary Fill track when ON (white thumb with amber dot), Outline track when OFF.
7. **Amber Micro-Silhouettes**: Heroicon lightning/sparkle accents & `6px` status/stash pip (`#FBBF24`).

### Molecules
1. **Shelf Item Row**: Unclipped translucent obsidian-violet pill (`1px` `0.38 → 0.0` bottom-to-top faded rim) + Heroicon Kind Icon + Ellipsized Label + Text-Only `✕` button.
2. **Mode Dock Bar**: Borderless recessed glass capsule (`rgba(12, 10, 26, 0.16)`) with a **140ms Raycast sliding translucent indicator pill**.
3. **Header Action Cluster**: Title + Obsidian Icon Orb (left) + Circle Icon Buttons (right, including Auto-Stash Bell badge).
4. **Notification Capsule**: Single continuous borderless obsidian glass pill (`368×64`, `r=32px`) with App Icon orb, title + amber separator dot + message, and autonomous countdown `×` button. Never overlaps expanded workspace (auto-stashes into Bell icon).
5. **Settings Control Row**: Label + Subtitle (left) + Segmented Fill/Outline/Text controls or Toggle (right).

### Organisms
1. **Borderless Obsidian Card**: Shadow-elevated translucent container (`0px` border) with bottom violet-pearl bloom.
2. **Command Hub (`Home`)**: Companion Card (`154×154` CPU/RAM + Purge CTA) + 2 full-height (`154px`) side-by-side cards (`Now Playing` + `Quick Shelf`).
3. **3-Box Floating Music Player (`Music`)**:
   - **Box 1 (`y = 0..164`)**: Top notch shell with Mode Dock, Centered Large Lyrics (`74px` lyrics box, `12–16px` clear gap between active & prev/next lines), and Full-Width Seek Bar (`y = 148`).
   - **Gap 1 (`12px` transparent)** → **Box 2 (`y = 176..242`, `h = 66px`, `0px` border, no drop-shadow)**: Real SMTC Album Art cover (`50×50px`, Fill Height with `8px` padding + locked `1:1` aspect ratio, Music icon fallback), Title (`14px Bold`), Artist (`11.5px Regular`), and Timestamp (`11px`).
   - **Gap 2 (`8px` transparent)** → **Box 3 (`y = 250..286`, `6px` bottom safety margin)**: 3 separated floating media buttons (`56×36`, `gap = 12px`) using `PrimaryOutline` / `PrimaryFill`.
   - All 3 boxes share identical `15px` WinRT blur + Optical Luminosity Clamp via `BlurSpec.box1_h` & `BlurSpec.extra_boxes` (`src/composition.rs`).
4. **Vertical Quick Shelf (`Shelf`)**: 3-visible-row vertical stacked list (`Shelf Item Row` with kind icon, ellipsized label, hover highlight, `✕` remove button, and vertical scroll track) with staggered entrance — never over-extends.
5. **Settings Window**: Borderless glass window with 4 streamlined sections (`General & Notch`, `Motion & Perf`, `Notifications`, `Memory & System`) containing shadow-elevated borderless Section Cards — **zero transparency/blur sliders**.

---

## 3. Direct2D & WinRT Composition Porting Map (`CyNotch-v0.3.1`)
- `src/dcomp_blur.rs` & `src/composition.rs`:
  - Chain `GaussianBlurEffect(15px)` → **Optical Luminosity Clamp** (`ColorMatrixEffect` luminance compression) on WinRT `CompositionBackdropBrush`, with `BlurSpec.box1_h` + `BlurSpec.extra_boxes` rendering multi-box alpha masks onto `mask_swap_chain` so separated floating boxes blur identically while gaps stay 100% transparent.
- `src/painter_d2d.rs`:
  - `elevated_card`: Remove outer bevel stroke when `frosted == true`; translucent obsidian-to-violet gradient + subtle bottom-left radial bloom (no `rrect_drop_shadow` on separated transparent-gap cards).
  - `tactile_surface_pill_t`: Support `ButtonVariant::{PrimaryFill, PrimaryOutline, TextOnly}`; continuous vertical `ID2D1LinearGradientBrush` rim (`bottom ~0.50 → top 0.0`) for `PrimaryFill` & `PrimaryOutline`; pure bottom-up liquid glow for `TextOnly` hover; 5% opacity text shadow.
- `src/renderer/views.rs`: 2-card Home Hub, 3-box floating Music UI (no lyric chips), vertical 3-row Quick Shelf, Safe Trim MemReduct UI, `--ray-fast` (`120ms`) staggered mode entrance.
- `src/settings_window.rs`: Fixed calibrated glass constants (`CALIBRATED_BLUR_PX = 15`), Top-Center placement lock, 4 sections (`General & Notch`, `Motion & Perf`, `Notifications`, `Memory & System`), `Ctrl+Alt+C/M/S` hotkeys sidebar, and category-switch card stagger.
