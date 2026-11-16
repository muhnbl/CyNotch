#![allow(clippy::too_many_arguments)]

use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use tiny_skia::{Color, Pixmap};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular = 0,
    Semibold = 1,
    Bold = 2,
}

/// Glyph rasterizer + cache on top of fontdue, compositing straight into a
/// premultiplied tiny-skia pixmap. Coordinates here are *physical* pixels.
pub struct FontRenderer {
    fonts: [Font; 3],
    cache: HashMap<(char, Weight, u32), (Metrics, Vec<u8>)>,
}

fn load(path: &str) -> Option<Font> {
    std::fs::read(path)
        .ok()
        .and_then(|b| Font::from_bytes(b, FontSettings::default()).ok())
}

impl FontRenderer {
    pub fn new() -> Self {
        let regular = load(r"C:\Windows\Fonts\segoeui.ttf").expect("Segoe UI not found");
        let semibold = load(r"C:\Windows\Fonts\seguisb.ttf")
            .unwrap_or_else(|| load(r"C:\Windows\Fonts\segoeui.ttf").unwrap());
        let bold = load(r"C:\Windows\Fonts\segoeuib.ttf")
            .unwrap_or_else(|| load(r"C:\Windows\Fonts\segoeui.ttf").unwrap());
        Self {
            fonts: [regular, semibold, bold],
            cache: HashMap::new(),
        }
    }

    fn font(&self, w: Weight) -> &Font {
        &self.fonts[w as usize]
    }

    pub fn measure(&self, text: &str, px: f32, w: Weight) -> f32 {
        let f = self.font(w);
        let mut width = 0.0;
        let mut prev: Option<char> = None;
        for c in text.chars() {
            if let Some(p) = prev {
                width += f.horizontal_kern(p, c, px).unwrap_or(0.0);
            }
            width += f.metrics(c, px).advance_width;
            prev = Some(c);
        }
        width
    }

    pub fn ellipsize(&self, text: &str, px: f32, w: Weight, max_w: f32) -> String {
        if self.measure(text, px, w) <= max_w {
            return text.to_string();
        }
        let ell = "…";
        let budget = max_w - self.measure(ell, px, w);
        let f = self.font(w);
        let mut out = String::new();
        let mut acc = 0.0;
        for c in text.chars() {
            let a = f.metrics(c, px).advance_width;
            if acc + a > budget {
                break;
            }
            acc += a;
            out.push(c);
        }
        format!("{}{}", out.trim_end(), ell)
    }

    /// Word-wrap `text` into lines of width `<= max_w` (physical px) without ellipsis truncation.
    pub fn wrap_lines(&self, text: &str, px: f32, w: Weight, max_w: f32) -> Vec<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return vec![String::new()];
        }
        let budget = max_w.max(24.0);
        if self.measure(trimmed, px, w) <= budget {
            return vec![trimmed.to_string()];
        }

        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();

        for word in trimmed.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };

            if self.measure(&candidate, px, w) <= budget {
                current = candidate;
            } else if !current.is_empty() {
                lines.push(current);
                if self.measure(word, px, w) <= budget {
                    current = word.to_string();
                } else {
                    // Break an abnormally long single token across lines without ellipsis
                    let mut chunk = String::new();
                    for ch in word.chars() {
                        let next_chunk = format!("{chunk}{ch}");
                        if !chunk.is_empty() && self.measure(&next_chunk, px, w) > budget {
                            lines.push(chunk);
                            chunk = ch.to_string();
                        } else {
                            chunk = next_chunk;
                        }
                    }
                    current = chunk;
                }
            } else {
                let mut chunk = String::new();
                for ch in word.chars() {
                    let next_chunk = format!("{chunk}{ch}");
                    if !chunk.is_empty() && self.measure(&next_chunk, px, w) > budget {
                        lines.push(chunk);
                        chunk = ch.to_string();
                    } else {
                        chunk = next_chunk;
                    }
                }
                current = chunk;
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }
        if lines.is_empty() {
            lines.push(trimmed.to_string());
        }
        lines
    }

    /// Draw `text` with its baseline at `baseline` (physical px).
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, x: f32, baseline: f32, px: f32, w: Weight, color: Color) {
        let size_key = (px * 4.0).round() as u32;
        let raster_px = size_key as f32 / 4.0;
        let baseline = baseline.round();
        let mut pen = x;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            if let Some(p) = prev {
                pen += self.fonts[w as usize].horizontal_kern(p, ch, raster_px).unwrap_or(0.0);
            }
            let key = (ch, w, size_key);
            if !self.cache.contains_key(&key) {
                if self.cache.len() > 4096 {
                    self.cache.clear();
                }
                let g = self.fonts[w as usize].rasterize(ch, raster_px);
                self.cache.insert(key, g);
            }
            let (m, bmp) = &self.cache[&key];
            let gx = pen.round() as i32 + m.xmin;
            let gy = baseline as i32 - m.height as i32 - m.ymin;
            blend(pm, bmp, m.width, m.height, gx, gy, color);
            pen += m.advance_width;
            prev = Some(ch);
        }
    }
}

impl Default for FontRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn div255(x: u32) -> u32 {
    let t = x + 128;
    (t + (t >> 8)) >> 8
}

fn blend(pm: &mut Pixmap, bmp: &[u8], w: usize, h: usize, gx: i32, gy: i32, color: Color) {
    if w == 0 || h == 0 {
        return;
    }
    let pw = pm.width() as i32;
    let ph = pm.height() as i32;
    let row_start = (-gy).max(0) as usize;
    let row_end = (ph - gy).min(h as i32).max(0) as usize;
    let col_start = (-gx).max(0) as usize;
    let col_end = (pw - gx).min(w as i32).max(0) as usize;
    if row_start >= row_end || col_start >= col_end {
        return;
    }

    let cr = (color.red() * 255.0 + 0.5) as u32;
    let cg = (color.green() * 255.0 + 0.5) as u32;
    let cb = (color.blue() * 255.0 + 0.5) as u32;
    let ca = (color.alpha() * 255.0 + 0.5) as u32;
    if ca == 0 {
        return;
    }

    let pw_usize = pw as usize;
    let data = pm.data_mut();
    for row in row_start..row_end {
        let py = (gy + row as i32) as usize;
        let src_row_off = row * w;
        let dst_row_off = py * pw_usize;
        for col in col_start..col_end {
            let cov = bmp[src_row_off + col];
            if cov == 0 {
                continue;
            }
            let i = (dst_row_off + (gx + col as i32) as usize) * 4;
            if cov == 255 && ca == 255 {
                data[i] = cr as u8;
                data[i + 1] = cg as u8;
                data[i + 2] = cb as u8;
                data[i + 3] = 255;
                continue;
            }
            let src_a = div255(cov as u32 * ca);
            if src_a == 0 {
                continue;
            }
            let inv_a = 255 - src_a;
            let sr = div255(cr * src_a);
            let sg = div255(cg * src_a);
            let sb = div255(cb * src_a);
            data[i] = (sr + div255(data[i] as u32 * inv_a)).min(255) as u8;
            data[i + 1] = (sg + div255(data[i + 1] as u32 * inv_a)).min(255) as u8;
            data[i + 2] = (sb + div255(data[i + 2] as u32 * inv_a)).min(255) as u8;
            data[i + 3] = (src_a + div255(data[i + 3] as u32 * inv_a)).min(255) as u8;
        }
    }
}
