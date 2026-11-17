# Changelog

## v0.2.3 (2026-11-17)
### Color Emoji & Unicode Fallback (`src/font.rs`)
- Added Windows Direct2D + DirectWrite + WIC color-emoji rasterizer (`D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT`) with cluster segmentation (`next_cluster`) for ZWJ, flag, keycap, and skin-tone sequences.
- Added `seguisym.ttf` and `seguihis.ttf` monochrome fallbacks so notification emojis and Unicode symbols no longer render as `▯` tofu boxes.
- Added `windows` crate features: `Win32_Graphics_Direct2D`, `Win32_Graphics_Direct2D_Common`, `Win32_Graphics_DirectWrite`, `Win32_Graphics_Imaging`.
