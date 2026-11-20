use std::collections::HashMap;
use windows::core::{Result, HSTRING};
use windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F;
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1DeviceContext, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_LINE_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_TEXT_METRICS,
};
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular = 0,
    Semibold = 1,
    Bold = 2,
}

pub struct DWriteTextCache {
    dwrite: IDWriteFactory,
    formats: HashMap<(u16, u32), IDWriteTextFormat>,
    layouts: HashMap<(String, u16, u32, u32), (IDWriteTextLayout, f32)>,
}

impl DWriteTextCache {
    pub fn new() -> Result<Self> {
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(Self {
            dwrite,
            formats: HashMap::new(),
            layouts: HashMap::new(),
        })
    }

    /// Strips invisible Unicode bidi/isolate format characters (`U+2066..U+2069` FSI/PDI/LRI/RLI,
    /// `U+200E..U+200F` LRM/RLM, `U+202A..U+202E` LRE/RLE/PDF/LRO/RLO, `U+FEFF` BOM, `U+200B` ZWSP)
    /// that `Segoe UI Variable Text` renders as visible dotted-box `[FSI]` / `[PDI]` glyphs.
    #[inline]
    pub fn clean_display_str(text: &str) -> std::borrow::Cow<'_, str> {
        if text.chars().any(crate::notifications::is_invisible_format_char) {
            std::borrow::Cow::Owned(
                text.chars()
                    .filter(|&c| !crate::notifications::is_invisible_format_char(c))
                    .collect(),
            )
        } else {
            std::borrow::Cow::Borrowed(text)
        }
    }

    fn get_format(&mut self, weight: Weight, size_quarter_px: u32) -> Result<IDWriteTextFormat> {
        let weight_u16 = match weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL.0 as u16,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD.0 as u16,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD.0 as u16,
        };

        if let Some(format) = self.formats.get(&(weight_u16, size_quarter_px)) {
            return Ok(format.clone());
        }

        let font_family = HSTRING::from("Segoe UI Variable Text");
        // Fallback to Segoe UI if needed, but for now we follow the prompt's suggestion.
        
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                &font_family,
                None,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT(weight_u16 as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                (size_quarter_px as f32) / 4.0,
                &HSTRING::from("en-us"),
            )?
        };

        unsafe {
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        }

        // BH-25: Cap cached IDWriteTextFormat entries so scale animations cannot grow unboundedly.
        if self.formats.len() >= 64 {
            self.formats.clear();
        }
        self.formats.insert((weight_u16, size_quarter_px), format.clone());
        Ok(format)
    }

    fn get_layout_with_baseline(
        &mut self,
        text: &str,
        weight: Weight,
        size_quarter_px: u32,
        max_w_quarter_px: u32,
    ) -> Result<(IDWriteTextLayout, f32)> {
        let cleaned = Self::clean_display_str(text);
        let text = cleaned.as_ref();
        let weight_u16 = match weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL.0 as u16,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD.0 as u16,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD.0 as u16,
        };

        let key = (text.to_string(), weight_u16, size_quarter_px, max_w_quarter_px);
        if let Some((layout, baseline)) = self.layouts.get(&key) {
            return Ok((layout.clone(), *baseline));
        }

        if self.layouts.len() >= 512 {
            self.clear();
        }

        let format = self.get_format(weight, size_quarter_px)?;
        let text_hstring = HSTRING::from(text);

        let max_w = if max_w_quarter_px == u32::MAX {
            1e6
        } else {
            (max_w_quarter_px as f32) / 4.0
        };

        let layout = unsafe {
            self.dwrite.CreateTextLayout(
                text_hstring.as_wide(),
                &format,
                max_w,
                1e6,
            )?
        };

        let baseline = unsafe {
            let mut lm = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0u32;
            if layout.GetLineMetrics(Some(&mut lm), &mut count).is_ok() && count > 0 {
                lm[0].baseline
            } else {
                (size_quarter_px as f32) / 4.0 * 1.04
            }
        };

        self.layouts.insert(key, (layout.clone(), baseline));
        Ok((layout, baseline))
    }

    fn get_layout(
        &mut self,
        text: &str,
        weight: Weight,
        size_quarter_px: u32,
        max_w_quarter_px: u32,
    ) -> Result<IDWriteTextLayout> {
        Ok(self.get_layout_with_baseline(text, weight, size_quarter_px, max_w_quarter_px)?.0)
    }

    pub fn measure(&mut self, text: &str, px: f32, weight: Weight) -> (f32, f32) {
        let size_quarter_px = (px * 4.0).round() as u32;
        let layout = match self.get_layout(text, weight, size_quarter_px, u32::MAX) {
            Ok(l) => l,
            Err(_) => return (0.0, 0.0),
        };

        unsafe {
            let mut metrics: DWRITE_TEXT_METRICS = std::mem::zeroed();
            if layout.GetMetrics(&mut metrics).is_ok() {
                (metrics.widthIncludingTrailingWhitespace, metrics.height)
            } else {
                (0.0, 0.0)
            }
        }
    }

    pub fn ellipsize_cow<'a>(
        &mut self,
        text: &'a str,
        px: f32,
        w: Weight,
        max_w: f32,
    ) -> std::borrow::Cow<'a, str> {
        if self.measure(text, px, w).0 <= max_w {
            return std::borrow::Cow::Borrowed(text);
        }
        let char_indices: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let mut low = 0;
        let mut high = char_indices.len();
        while low < high {
            let mid = (low + high + 1) / 2;
            let end_idx = if mid < char_indices.len() { char_indices[mid] } else { text.len() };
            let prefix = &text[..end_idx];
            let candidate = format!("{}…", prefix);
            if self.measure(&candidate, px, w).0 <= max_w {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let end_idx = if low < char_indices.len() { char_indices[low] } else { text.len() };
        let prefix = &text[..end_idx];
        let result = format!("{}…", prefix);
        std::borrow::Cow::Owned(result)
    }

    pub fn ellipsize(&mut self, text: &str, px: f32, w: Weight, max_w: f32) -> String {
        self.ellipsize_cow(text, px, w, max_w).into_owned()
    }

    pub fn wrap_lines(&mut self, text: &str, px: f32, w: Weight, max_w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for word in text.split_whitespace() {
            if lines.is_empty() {
                lines.push(word.to_string());
                continue;
            }
            let last = lines.last_mut().unwrap();
            let candidate = format!("{} {}", last, word);
            if self.measure(&candidate, px, w).0 <= max_w {
                *last = candidate;
            } else {
                lines.push(word.to_string());
            }
        }
        lines
    }


    pub fn draw_text_baseline(
        &mut self,
        ctx: &ID2D1DeviceContext,
        brush: &ID2D1Brush,
        text: &str,
        x: f32,
        y_baseline: f32,
        px: f32,
        weight: Weight,
        max_w: Option<f32>,
    ) -> Result<()> {
        let size_quarter_px = (px * 4.0).round() as u32;
        let max_w_quarter_px = max_w.map(|w| (w * 4.0).round() as u32).unwrap_or(u32::MAX);
        let (layout, baseline) = self.get_layout_with_baseline(text, weight, size_quarter_px, max_w_quarter_px)?;
        unsafe {
            ctx.DrawTextLayout(
                D2D_POINT_2F { x, y: y_baseline - baseline },
                &layout,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
        Ok(())
    }

    pub fn draw_text(
        &mut self,
        ctx: &ID2D1DeviceContext,
        brush: &ID2D1Brush,
        text: &str,
        x: f32,
        y_top: f32,
        px: f32,
        weight: Weight,
        max_w: Option<f32>,
    ) -> Result<()> {
        let size_quarter_px = (px * 4.0).round() as u32;
        let max_w_quarter_px = max_w.map(|w| (w * 4.0).round() as u32).unwrap_or(u32::MAX);
        
        let layout = self.get_layout(text, weight, size_quarter_px, max_w_quarter_px)?;
        
        unsafe {
            ctx.DrawTextLayout(
                D2D_POINT_2F { x, y: y_top },
                &layout,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
        
        Ok(())
    }

    pub fn clear(&mut self) {
        self.layouts.clear();
        self.formats.clear();
    }

    pub fn trim(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ test]
    fn test_dwrite_cache_new() {
        let cache = DWriteTextCache::new();
        assert!(cache.is_ok());
    }

    #[test]
    fn test_measure() {
        let mut cache = DWriteTextCache::new().unwrap();
        let (w, h) = cache.measure("CyNotch", 16.0, Weight::Regular);
        assert!(w > 0.0);
        assert!(h > 0.0);

        let (w2, h2) = cache.measure("🎵🔥", 16.0, Weight::Regular);
        assert!(w2 > 0.0);
        assert!(h2 > 0.0);
    }

    #[test]
    fn test_measure_scaling() {
        let mut cache = DWriteTextCache::new().unwrap();
        let (w12, _) = cache.measure("CyNotch", 12.0, Weight::Regular);
        let (w24, _) = cache.measure("CyNotch", 24.0, Weight::Regular);
        assert!(w24 > w12);
    }

    #[test]
    fn test_ellipsize_cow() {
        let mut cache = DWriteTextCache::new().unwrap();
        let text = "CyNotch is a cool framework";
        let (w, _) = cache.measure("CyNotch...", 16.0, Weight::Regular);
        let result = cache.ellipsize_cow(text, 16.0, Weight::Regular, w);
        assert!(result.ends_with('…'));
        assert!(cache.measure(&result, 16.0, Weight::Regular).0 <= w);
    }

    #[test]
    fn test_wrap_lines() {
        let mut cache = DWriteTextCache::new().unwrap();
        let text = "CyNotch is a cool framework";
        let lines = cache.wrap_lines(text, 16.0, Weight::Regular, 100.0);
        assert!(lines.len() > 1);
        for line in lines {
            assert!(cache.measure(&line, 16.0, Weight::Regular).0 <= 100.0);
        }
    }

    #[test]
    fn bh25_formats_cache_is_bounded_and_cleared_on_trim() {
        let mut cache = DWriteTextCache::new().unwrap();
        for i in 0..100 {
            let sz = 8.0 + (i as f32) * 0.25;
            let _ = cache.measure("A", sz, Weight::Regular);
        }
        assert!(cache.formats.len() <= 64, "formats exceeded cap: {}", cache.formats.len());
        cache.trim();
        assert!(cache.formats.is_empty());
        assert!(cache.layouts.is_empty());
    }
}
