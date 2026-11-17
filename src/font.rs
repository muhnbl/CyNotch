#![allow(clippy::too_many_arguments)]

use fontdue::{Font, FontSettings, Metrics};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use tiny_skia::{Color, Pixmap};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1CreateFactory,
    ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::core::PCWSTR;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular = 0,
    Semibold = 1,
    Bold = 2,
}

#[derive(Clone)]
struct CachedCluster {
    width: usize,
    height: usize,
    xmin: i32,
    ymin_from_baseline: i32,
    advance_width: f32,
    is_color: bool,
    /// Premultiplied RGBA bytes (4 * width * height) when `is_color` is true,
    /// or alpha coverage bytes (width * height) when `is_color` is false.
    pixels: Vec<u8>,
}

/// Glyph rasterizer + cache on top of fontdue + DirectWrite/Direct2D color emoji fallback,
/// compositing straight into a premultiplied tiny-skia pixmap. Coordinates here are *physical* pixels.
pub struct FontRenderer {
    fonts: &'static FontPack,
    cache: HashMap<(char, Weight, u32), (Metrics, Vec<u8>)>,
    cluster_cache: HashMap<(String, Weight, u32), CachedCluster>,
}

struct FontPack {
    primary: [Font; 3],
    fallbacks: Vec<Font>,
}

static SHARED_FONTS: OnceLock<FontPack> = OnceLock::new();

fn load(path: &str) -> Option<Font> {
    std::fs::read(path)
        .ok()
        .and_then(|b| Font::from_bytes(b, FontSettings::default()).ok())
}

// ── Thread-local DirectWrite / Direct2D / WIC Rasterizer ─────────────────────

struct DWriteRasterizer {
    wic: IWICImagingFactory,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
}

thread_local! {
    static DWRITE_RASTERIZER: RefCell<Option<Option<DWriteRasterizer>>> = const { RefCell::new(None) };
}

impl DWriteRasterizer {
    fn new() -> Option<Self> {
        unsafe {
            // Ignore RPC_E_CHANGED_MODE if thread already initialized COM with another apartment model.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            Some(Self { wic, d2d, dwrite })
        }
    }

    fn rasterize(&self, cluster: &str, px: f32, weight: Weight, is_color: bool) -> Option<CachedCluster> {
        let wide: Vec<u16> = cluster.encode_utf16().collect();
        if wide.is_empty() {
            return None;
        }
        let dwrite_weight = match weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_REGULAR,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
        };
        let locale: [u16; 6] = [b'e' as u16, b'n' as u16, b'-' as u16, b'U' as u16, b'S' as u16, 0];
        let family_segoeuie: [u16; 15] = [
            b'S' as u16, b'e' as u16, b'g' as u16, b'o' as u16, b'e' as u16, b' ' as u16,
            b'U' as u16, b'I' as u16, b' ' as u16, b'E' as u16, b'm' as u16, b'o' as u16,
            b'j' as u16, b'i' as u16, 0,
        ];
        let family_segoeui: [u16; 9] = [
            b'S' as u16, b'e' as u16, b'g' as u16, b'o' as u16, b'e' as u16, b' ' as u16,
            b'U' as u16, b'I' as u16, 0,
        ];
        let family = if is_color {
            PCWSTR(family_segoeuie.as_ptr())
        } else {
            PCWSTR(family_segoeui.as_ptr())
        };

        unsafe {
            let format = self
                .dwrite
                .CreateTextFormat(
                    family,
                    None,
                    dwrite_weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    px.max(1.0),
                    PCWSTR(locale.as_ptr()),
                )
                .ok()?;

            let layout = self
                .dwrite
                .CreateTextLayout(&wide, &format, 1000.0, 1000.0)
                .ok()?;

            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;

            let mut line_metrics = [DWRITE_LINE_METRICS::default(); 1];
            let mut line_count = 0u32;
            layout
                .GetLineMetrics(Some(&mut line_metrics), &mut line_count)
                .ok()?;

            let baseline_offset = if line_count > 0 {
                line_metrics[0].baseline
            } else {
                px * 0.8
            };
            let advance_width = metrics.widthIncludingTrailingWhitespace.max(0.0);

            let pad = (px * 0.35).ceil().max(4.0);
            let bmp_w = (metrics.width.max(advance_width) + pad * 2.0)
                .ceil()
                .clamp(1.0, 512.0) as u32;
            let bmp_h = (metrics.height.max(line_metrics[0].height) + pad * 2.0)
                .ceil()
                .clamp(1.0, 512.0) as u32;

            let wic_bmp = self
                .wic
                .CreateBitmap(
                    bmp_w,
                    bmp_h,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )
                .ok()?;

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };

            let rt = self
                .d2d
                .CreateWicBitmapRenderTarget(&wic_bmp, &rt_props)
                .ok()?;
            let brush = rt
                .CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    None,
                )
                .ok()?;

            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            rt.DrawTextLayout(
                D2D_POINT_2F { x: pad, y: pad },
                &layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
            rt.EndDraw(None, None).ok()?;

            let lock = wic_bmp.Lock(std::ptr::null(), WICBitmapLockRead.0 as u32).ok()?;
            let mut buf_size = 0u32;
            let mut data_ptr = std::ptr::null_mut();
            lock.GetDataPointer(&mut buf_size, &mut data_ptr).ok()?;
            let stride = lock.GetStride().ok()? as usize;
            let raw = std::slice::from_raw_parts(data_ptr, stride * bmp_h as usize);

            // Trim transparent bounding box to keep glyph cache compact.
            let mut min_x = bmp_w as usize;
            let mut max_x = 0usize;
            let mut min_y = bmp_h as usize;
            let mut max_y = 0usize;
            for y in 0..bmp_h as usize {
                let row = &raw[y * stride..y * stride + bmp_w as usize * 4];
                for x in 0..bmp_w as usize {
                    let a = row[x * 4 + 3];
                    if a > 0 {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x + 1);
                        min_y = min_y.min(y);
                        max_y = max_y.max(y + 1);
                    }
                }
            }

            if min_x >= max_x || min_y >= max_y {
                return Some(CachedCluster {
                    width: 0,
                    height: 0,
                    xmin: 0,
                    ymin_from_baseline: 0,
                    advance_width,
                    is_color,
                    pixels: Vec::new(),
                });
            }

            let tw = max_x - min_x;
            let th = max_y - min_y;
            let pad_i = pad.round() as i32;
            let base_i = baseline_offset.round() as i32;
            let xmin = min_x as i32 - pad_i;
            // Top of trimmed bitmap relative to baseline (negative = above baseline).
            let ymin_from_baseline = min_y as i32 - pad_i - base_i;

            let pixels = if is_color {
                let mut out = Vec::with_capacity(tw * th * 4);
                for y in min_y..max_y {
                    let row = &raw[y * stride..];
                    for x in min_x..max_x {
                        let i = x * 4;
                        // PBGRA -> PRGBA
                        out.push(row[i + 2]);
                        out.push(row[i + 1]);
                        out.push(row[i]);
                        out.push(row[i + 3]);
                    }
                }
                out
            } else {
                let mut out = Vec::with_capacity(tw * th);
                for y in min_y..max_y {
                    let row = &raw[y * stride..];
                    for x in min_x..max_x {
                        out.push(row[x * 4 + 3]);
                    }
                }
                out
            };

            Some(CachedCluster {
                width: tw,
                height: th,
                xmin,
                ymin_from_baseline,
                advance_width,
                is_color,
                pixels,
            })
        }
    }
}

fn with_dwrite<R>(f: impl FnOnce(&DWriteRasterizer) -> Option<R>) -> Option<R> {
    DWRITE_RASTERIZER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(DWriteRasterizer::new());
        }
        borrow.as_ref().and_then(|opt| opt.as_ref()).and_then(f)
    })
}

// ── Emoji & Cluster Segmentation ─────────────────────────────────────────────

#[inline]
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

#[inline]
fn is_skin_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

#[inline]
fn is_tag_char(c: char) -> bool {
    ('\u{E0020}'..='\u{E007E}').contains(&c)
}

#[inline]
fn is_variation_or_joiner(c: char) -> bool {
    matches!(c, '\u{FE0E}' | '\u{FE0F}' | '\u{200D}' | '\u{20E3}')
}

#[inline]
fn is_emoji_codepoint(c: char) -> bool {
    let u = c as u32;
    matches!(
        u,
        0x203C
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x2194..=0x2199
            | 0x21A9..=0x21AA
            | 0x231A..=0x231B
            | 0x2328
            | 0x23CF
            | 0x23E9..=0x23F3
            | 0x23F8..=0x23FA
            | 0x24C2
            | 0x25AA..=0x25AB
            | 0x25B6
            | 0x25C0
            | 0x25FB..=0x25FE
            | 0x2600..=0x27BF
            | 0x2934..=0x2935
            | 0x2B05..=0x2B07
            | 0x2B1B..=0x2B1C
            | 0x2B50
            | 0x2B55
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
            | 0x1F000..=0x1FAFF
    )
}

#[inline]
fn is_keycap_base(c: char) -> bool {
    matches!(c, '0'..='9' | '#' | '*')
}

/// Splits the next renderable cluster from `s`.
/// Returns `(cluster_slice, is_color_emoji)`.
fn next_cluster(s: &str) -> (&str, bool) {
    let mut iter = s.char_indices();
    let Some((_, first)) = iter.next() else {
        return ("", false);
    };
    let mut end = first.len_utf8();

    // Flag pair: two regional indicators
    if is_regional_indicator(first) {
        if let Some((idx, second)) = iter.next()
            && is_regional_indicator(second)
        {
            return (&s[..idx + second.len_utf8()], true);
        }
        return (&s[..end], true);
    }

    // Keycap sequence: [0-9#*] + optional FE0F + 20E3
    if is_keycap_base(first) {
        let mut look = iter.clone();
        if let Some((idx1, c1)) = look.next() {
            if c1 == '\u{20E3}' {
                return (&s[..idx1 + c1.len_utf8()], true);
            }
            if c1 == '\u{FE0F}'
                && let Some((idx2, c2)) = look.next()
                && c2 == '\u{20E3}'
            {
                return (&s[..idx2 + c2.len_utf8()], true);
            }
        }
    }

    // Tag sequence (e.g. England/Scotland/Wales flags starting with U+1F3F4)
    if first == '\u{1F3F4}' {
        let mut has_tags = false;
        for (idx, c) in iter.clone() {
            if is_tag_char(c) {
                has_tags = true;
                end = idx + c.len_utf8();
            } else if has_tags && c == '\u{E007F}' {
                return (&s[..idx + c.len_utf8()], true);
            } else {
                break;
            }
        }
    }

    // Emoji / ZWJ / variation selector / skin tone chain
    let mut is_emoji = is_emoji_codepoint(first);
    let mut rest = &s[end..];
    loop {
        let mut chars = rest.chars();
        let Some(next) = chars.next() else {
            break;
        };
        if next == '\u{FE0F}' {
            is_emoji = true;
            end += next.len_utf8();
            rest = &s[end..];
        } else if next == '\u{FE0E}' {
            is_emoji = false;
            end += next.len_utf8();
            rest = &s[end..];
        } else if is_skin_tone(next) && is_emoji {
            end += next.len_utf8();
            rest = &s[end..];
        } else if next == '\u{200D}' {
            // ZWJ: consume ZWJ + following emoji component if present
            let after_zwj = &rest[next.len_utf8()..];
            if let Some(joined) = after_zwj.chars().next() {
                is_emoji = true;
                end += next.len_utf8() + joined.len_utf8();
                rest = &s[end..];
            } else {
                break;
            }
        } else {
            break;
        }
    }

    (&s[..end], is_emoji)
}

impl FontRenderer {
    pub fn new() -> Self {
        let fonts = SHARED_FONTS.get_or_init(|| {
            let regular = load(r"C:\Windows\Fonts\segoeui.ttf").expect("Segoe UI not found");
            let semibold = load(r"C:\Windows\Fonts\seguisb.ttf")
                .unwrap_or_else(|| load(r"C:\Windows\Fonts\segoeui.ttf").unwrap());
            let bold = load(r"C:\Windows\Fonts\segoeuib.ttf")
                .unwrap_or_else(|| load(r"C:\Windows\Fonts\segoeui.ttf").unwrap());

            let mut fallbacks = Vec::new();
            for path in [
                r"C:\Windows\Fonts\seguisym.ttf",
                r"C:\Windows\Fonts\seguihis.ttf",
            ] {
                if let Some(f) = load(path) {
                    fallbacks.push(f);
                }
            }
            FontPack {
                primary: [regular, semibold, bold],
                fallbacks,
            }
        });
        Self {
            fonts,
            cache: HashMap::new(),
            cluster_cache: HashMap::new(),
        }
    }

    fn font(&self, w: Weight) -> &Font {
        &self.fonts.primary[w as usize]
    }

    /// Resolves a single non-emoji character to the first font that has a glyph for it.
    fn font_for_char(&self, c: char, w: Weight) -> Option<&Font> {
        let primary = self.font(w);
        if primary.lookup_glyph_index(c) != 0 {
            return Some(primary);
        }
        self.fonts
            .fallbacks
            .iter()
            .find(|&fb| fb.lookup_glyph_index(c) != 0)
    }

    fn measure_cluster(&self, cluster: &str, is_emoji: bool, px: f32, w: Weight) -> f32 {
        if !is_emoji {
            let mut chars = cluster.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if let Some(f) = self.font_for_char(c, w) {
                    return f.metrics(c, px).advance_width;
                }
                if is_variation_or_joiner(c) {
                    return 0.0;
                }
            }
        }
        let size_key = (px * 4.0).round() as u32;
        let key = (cluster.to_string(), w, size_key);
        if let Some(cached) = self.cluster_cache.get(&key) {
            return cached.advance_width;
        }
        let raster_px = size_key as f32 / 4.0;
        with_dwrite(|dw| dw.rasterize(cluster, raster_px, w, is_emoji))
            .map(|c| c.advance_width)
            .unwrap_or_else(|| self.font(w).metrics(cluster.chars().next().unwrap_or(' '), px).advance_width)
    }

    pub fn measure(&self, text: &str, px: f32, w: Weight) -> f32 {
        let f = self.font(w);
        let mut width = 0.0;
        let mut prev_primary: Option<char> = None;
        let mut rest = text;
        while !rest.is_empty() {
            let (cluster, is_emoji) = next_cluster(rest);
            rest = &rest[cluster.len()..];

            if !is_emoji {
                let mut chars = cluster.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && f.lookup_glyph_index(c) != 0
                {
                    if let Some(p) = prev_primary {
                        width += f.horizontal_kern(p, c, px).unwrap_or(0.0);
                    }
                    width += f.metrics(c, px).advance_width;
                    prev_primary = Some(c);
                    continue;
                }
            }
            prev_primary = None;
            width += self.measure_cluster(cluster, is_emoji, px, w);
        }
        width
    }

    pub fn ellipsize_cow<'a>(&self, text: &'a str, px: f32, w: Weight, max_w: f32) -> Cow<'a, str> {
        if self.measure(text, px, w) <= max_w {
            return Cow::Borrowed(text);
        }
        let ell = "…";
        let budget = max_w - self.measure(ell, px, w);
        let f = self.font(w);
        let mut out = String::with_capacity(text.len().min(64));
        let mut acc = 0.0;
        let mut prev_primary: Option<char> = None;
        let mut rest = text;

        while !rest.is_empty() {
            let (cluster, is_emoji) = next_cluster(rest);
            rest = &rest[cluster.len()..];

            let mut next_primary = None;
            let a = if !is_emoji {
                let mut chars = cluster.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    if f.lookup_glyph_index(c) != 0 {
                        let kern = prev_primary
                            .and_then(|p| f.horizontal_kern(p, c, px))
                            .unwrap_or(0.0);
                        next_primary = Some(c);
                        kern + f.metrics(c, px).advance_width
                    } else {
                        self.measure_cluster(cluster, false, px, w)
                    }
                } else {
                    self.measure_cluster(cluster, false, px, w)
                }
            } else {
                self.measure_cluster(cluster, true, px, w)
            };

            if acc + a > budget {
                break;
            }
            acc += a;
            out.push_str(cluster);
            prev_primary = next_primary;
        }

        let trimmed_len = out.trim_end().len();
        out.truncate(trimmed_len);
        out.push('…');
        Cow::Owned(out)
    }

    pub fn ellipsize(&self, text: &str, px: f32, w: Weight, max_w: f32) -> String {
        self.ellipsize_cow(text, px, w, max_w).into_owned()
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
                    // Break an abnormally long single token across lines at cluster boundaries
                    let mut chunk = String::new();
                    let mut rest = word;
                    while !rest.is_empty() {
                        let (cluster, _) = next_cluster(rest);
                        rest = &rest[cluster.len()..];
                        let prev_len = chunk.len();
                        chunk.push_str(cluster);
                        if prev_len > 0 && self.measure(&chunk, px, w) > budget {
                            chunk.truncate(prev_len);
                            lines.push(std::mem::take(&mut chunk));
                            chunk.push_str(cluster);
                        }
                    }
                    current = chunk;
                }
            } else {
                let mut chunk = String::new();
                let mut rest = word;
                while !rest.is_empty() {
                    let (cluster, _) = next_cluster(rest);
                    rest = &rest[cluster.len()..];
                    let prev_len = chunk.len();
                    chunk.push_str(cluster);
                    if prev_len > 0 && self.measure(&chunk, px, w) > budget {
                        chunk.truncate(prev_len);
                        lines.push(std::mem::take(&mut chunk));
                        chunk.push_str(cluster);
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

    /// Draw `text` with its baseline at `baseline` (physical px) and return the advance width in physical px.
    pub fn draw(
        &mut self,
        pm: &mut Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        px: f32,
        w: Weight,
        color: Color,
    ) -> f32 {
        let size_key = (px * 4.0).round() as u32;
        let raster_px = size_key as f32 / 4.0;
        let baseline = baseline.round();
        let mut pen = x;
        let mut prev_primary: Option<char> = None;
        let mut rest = text;

        while !rest.is_empty() {
            let (cluster, is_emoji) = next_cluster(rest);
            rest = &rest[cluster.len()..];

            if !is_emoji {
                let mut chars = cluster.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    if self.fonts.primary[w as usize].lookup_glyph_index(ch) != 0 {
                        if let Some(p) = prev_primary {
                            pen += self.fonts.primary[w as usize]
                                .horizontal_kern(p, ch, raster_px)
                                .unwrap_or(0.0);
                        }
                        let key = (ch, w, size_key);
                        if !self.cache.contains_key(&key) {
                            const MAX_GLYPH_CACHE_ENTRIES: usize = 512;
                            if self.cache.len() >= MAX_GLYPH_CACHE_ENTRIES {
                                self.cache.clear();
                            }
                            let g = self.fonts.primary[w as usize].rasterize(ch, raster_px);
                            self.cache.insert(key, g);
                        }
                        let (m, bmp) = &self.cache[&key];
                        let gx = pen.round() as i32 + m.xmin;
                        let gy = baseline as i32 - m.height as i32 - m.ymin;
                        blend(pm, bmp, m.width, m.height, gx, gy, color);
                        pen += m.advance_width;
                        prev_primary = Some(ch);
                        continue;
                    }

                    // Non-emoji fallback in Symbol/Historic fonts (still crisp monochrome tinted with `color`)
                    if let Some(fb_idx) = self
                        .fonts
                        .fallbacks
                        .iter()
                        .position(|f| f.lookup_glyph_index(ch) != 0)
                    {
                        prev_primary = None;
                        let key = (ch, w, size_key);
                        if !self.cache.contains_key(&key) {
                            const MAX_GLYPH_CACHE_ENTRIES: usize = 512;
                            if self.cache.len() >= MAX_GLYPH_CACHE_ENTRIES {
                                self.cache.clear();
                            }
                            let g = self.fonts.fallbacks[fb_idx].rasterize(ch, raster_px);
                            self.cache.insert(key, g);
                        }
                        let (m, bmp) = &self.cache[&key];
                        let gx = pen.round() as i32 + m.xmin;
                        let gy = baseline as i32 - m.height as i32 - m.ymin;
                        blend(pm, bmp, m.width, m.height, gx, gy, color);
                        pen += m.advance_width;
                        continue;
                    }

                    if is_variation_or_joiner(ch) {
                        prev_primary = None;
                        continue;
                    }
                }
            }

            prev_primary = None;
            let key = (cluster.to_string(), w, size_key);
            if !self.cluster_cache.contains_key(&key) {
                const MAX_CLUSTER_CACHE_ENTRIES: usize = 256;
                if self.cluster_cache.len() >= MAX_CLUSTER_CACHE_ENTRIES {
                    self.cluster_cache.clear();
                }
                if let Some(rasterized) =
                    with_dwrite(|dw| dw.rasterize(cluster, raster_px, w, is_emoji))
                {
                    self.cluster_cache.insert(key.clone(), rasterized);
                }
            }

            if let Some(c) = self.cluster_cache.get(&key) {
                let gx = pen.round() as i32 + c.xmin;
                let gy = baseline as i32 + c.ymin_from_baseline;
                if c.is_color {
                    blend_rgba(pm, &c.pixels, c.width, c.height, gx, gy, color.alpha());
                } else {
                    blend(pm, &c.pixels, c.width, c.height, gx, gy, color);
                }
                pen += c.advance_width;
            }
        }
        pen - x
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

fn blend_rgba(pm: &mut Pixmap, rgba: &[u8], w: usize, h: usize, gx: i32, gy: i32, alpha: f32) {
    if w == 0 || h == 0 {
        return;
    }
    let ca = (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    if ca == 0 {
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

    let pw_usize = pw as usize;
    let data = pm.data_mut();
    for row in row_start..row_end {
        let py = (gy + row as i32) as usize;
        let src_row_off = row * w;
        let dst_row_off = py * pw_usize;
        for col in col_start..col_end {
            let si = (src_row_off + col) * 4;
            let sa = rgba[si + 3] as u32;
            if sa == 0 {
                continue;
            }
            let di = (dst_row_off + (gx + col as i32) as usize) * 4;
            if ca == 255 {
                if sa == 255 {
                    data[di] = rgba[si];
                    data[di + 1] = rgba[si + 1];
                    data[di + 2] = rgba[si + 2];
                    data[di + 3] = 255;
                } else {
                    let inv_a = 255 - sa;
                    data[di] = (rgba[si] as u32 + div255(data[di] as u32 * inv_a)).min(255) as u8;
                    data[di + 1] =
                        (rgba[si + 1] as u32 + div255(data[di + 1] as u32 * inv_a)).min(255) as u8;
                    data[di + 2] =
                        (rgba[si + 2] as u32 + div255(data[di + 2] as u32 * inv_a)).min(255) as u8;
                    data[di + 3] = (sa + div255(data[di + 3] as u32 * inv_a)).min(255) as u8;
                }
            } else {
                let eff_a = div255(sa * ca);
                if eff_a == 0 {
                    continue;
                }
                let inv_a = 255 - eff_a;
                let sr = div255(rgba[si] as u32 * ca);
                let sg = div255(rgba[si + 1] as u32 * ca);
                let sb = div255(rgba[si + 2] as u32 * ca);
                data[di] = (sr + div255(data[di] as u32 * inv_a)).min(255) as u8;
                data[di + 1] = (sg + div255(data[di + 1] as u32 * inv_a)).min(255) as u8;
                data[di + 2] = (sb + div255(data[di + 2] as u32 * inv_a)).min(255) as u8;
                data[di + 3] = (eff_a + div255(data[di + 3] as u32 * inv_a)).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_segmentation_preserves_emoji_sequences() {
        let s = "Hi 🔥👨‍👩‍👧‍👦🇮🇩1️⃣!";
        let mut clusters = Vec::new();
        let mut rest = s;
        while !rest.is_empty() {
            let (c, e) = next_cluster(rest);
            clusters.push((c, e));
            rest = &rest[c.len()..];
        }
        assert_eq!(
            clusters,
            vec![
                ("H", false),
                ("i", false),
                (" ", false),
                ("🔥", true),
                ("👨‍👩‍👧‍👦", true),
                ("🇮🇩", true),
                ("1️⃣", true),
                ("!", false),
            ]
        );
    }

    #[test]
    fn draws_color_emoji_without_tofu() {
        let mut fr = FontRenderer::new();
        let mut pm = Pixmap::new(160, 48).unwrap();
        let w = fr.draw(
            &mut pm,
            "🔥 ❤️ ✨",
            8.0,
            30.0,
            18.0,
            Weight::Regular,
            Color::WHITE,
        );
        assert!(w > 20.0, "expected positive advance width, got {w}");
        // Color emoji should produce non-white colored pixels (e.g. red/orange/yellow)
        let has_color_pixel = pm
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] > 50 && (px[0] != px[1] || px[1] != px[2]));
        assert!(has_color_pixel, "expected color emoji pixels, got monochrome only");
    }

    #[test]
    fn ellipsize_never_splits_zwj_emoji() {
        let fr = FontRenderer::new();
        let s = "Hello 👨‍👩‍👧‍👦 world";
        for max_w in [20.0, 40.0, 60.0, 80.0, 120.0] {
            let out = fr.ellipsize(s, 14.0, Weight::Regular, max_w);
            assert!(
                !out.ends_with("\u{200D}…"),
                "ellipsize split ZWJ sequence at max_w={max_w}: {out:?}"
            );
        }
    }
}
