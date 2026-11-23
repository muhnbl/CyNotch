# Design System: CyNotch v1.0.0 Web Experience

## 1. Visual Theme & Atmosphere
An editorial, kinetic dark-mode showcase fusing **Made to Disrupt's** high-contrast architectural typography and sticky-stacking split panels with **Cyno / CyNotch's** Electro Obsidian Mahamatra identity. Density is Gallery-Balanced (5/10), Variance is Asymmetric Editorial (8/10), and Motion is Fluid GPU Choreography (7/10) — strictly `transform` and `opacity` with zero neon AI slop.

## 2. Color Palette & Roles
- **Obsidian Canvas** (`#0D0D10`) — Primary page background (never `#000000`)
- **Graphite Panel** (`#121218`) — Split-card copy surfaces and elevated bento containers
- **Obsidian Glass** (`rgba(20, 20, 28, 0.72)`) — Sticky nav pill and interactive live notch preview
- **Alabaster Ink** (`#F5F3FF`) — Primary headings, high-contrast marquee ribbon background
- **Muted Slate** (`#A19EB0`) — Body copy, secondary descriptions (WCAG AA `>= 7.2:1` on `#0D0D10`)
- **Kinetic Dim** (`#5A5868`) — Pre-reveal scroll-manifesto text state
- **Hairline Border** (`rgba(245, 243, 255, 0.12)`) — `1px` structural dividers and pill outlines
- **Cyno Electro Violet** (`#7C3AED`) — Primary interactive accent for CTAs, active tabs, and focus rings
- **Sumeru Champagne Gold** (`#F4EBB5`) — Restrained editorial subhead & metadata accent inspired by Cyno's desert gold and Made to Disrupt's warm champagne highlight

## 3. Typography Rules
- **Display:** `Clash Display` — Extended geometric uppercase (`-0.03em` tracking, `clamp()` fluid scale) for hero statements, section markers, and pillar titles
- **Body:** `Outfit` — Clean geometric sans (`1.0625rem`, `1.65` leading, `65ch` max width)
- **Mono:** `JetBrains Mono` — Verified engineering telemetry, binary sizes, frame times, and hotkeys
- **Banned:** `Inter`, generic system serifs, emoji clutter, and rainbow gradient text on display headers

## 4. Component Stylings
- **Sticky Editorial Nav:** 3-part bar — sliced `CY/NOTCH` wordmark left, pill navigation center, single `GET v1.0.0` pill CTA right (`44px` minimum height)
- **Interactive Top-Bezel Capsule:** Live hoverable CyNotch capsule preview featuring Glowing Purple Wolf Eyes and the signature 3-stop `0% -> crest -> 0%` violet sweep
- **Tilted Marquee Ribbons:** Full-width `-1.5deg` / `+1.5deg` continuous CSS-transform ribbons creating kinetic rhythm between editorial sections
- **Sticky-Stacking Split Pillars:** 50/50 split cards (`position: sticky` on desktop, single-column on `< 768px`) pairing real v1.0.0 Direct2D screenshots with `#121218` specification panels
- **Buttons:** Full-pill tactile controls (`-1px` translate on hover, `0px` neon outer glow, `:focus-visible` `2px` violet ring)

## 5. Layout & Responsive Principles
- Max container width `1360px` centered; `min-height: 100dvh` hero (never `100vh`)
- Strictly zero overlapping text/images and zero horizontal overflow across `1440x900`, `768x1024`, and `390x844`
- All multi-column grids and sticky pillars collapse cleanly to single-column below `768px`
- All interactive elements meet `>= 44x44px` touch target minimum

## 6. Motion Philosophy
- GPU-only `transform` and `opacity` transitions with `cubic-bezier(0.22, 1, 0.36, 1)` deceleration
- Character/word scroll-reveal manifesto (`#5A5868 -> #F5F3FF`) driven by `IntersectionObserver` / throttled `requestAnimationFrame`
- Full `prefers-reduced-motion: reduce` respect (disables marquee drift, starfield motion, and tilt transforms)

## 7. Anti-Patterns (Never Do)
- No emojis in UI chrome
- No `Inter` font or `#000000` pure black
- No fabricated metrics, fake uptime percentages, or unverified benchmarks (display only verified `cargo test`, `perf_bench_d2d`, and binary size data)
- No generic 3-equal-card feature rows or "Scroll to explore" filler
