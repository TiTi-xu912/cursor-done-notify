use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use tiny_skia::Pixmap;

struct Part {
    font: FontRef<'static>,
    source: String,
}

// Fonts are memory-mapped rather than read: msyh.ttc alone is ~19 MB, and mapped pages
// stay shared and evictable instead of counting as private memory.
fn map_font(candidates: &[(&str, u32)]) -> Option<Part> {
    for (path, index) in candidates {
        if !Path::new(path).exists() {
            continue;
        }
        let Ok(file) = File::open(path) else { continue };
        let Ok(map) = (unsafe { Mmap::map(&file) }) else { continue };
        let map: &'static Mmap = Box::leak(Box::new(map));
        if let Ok(font) = FontRef::try_from_slice_and_index(&map[..], *index) {
            return Some(Part { font, source: format!("{path}#{index}") });
        }
    }
    None
}

fn px_scale(font: &FontRef<'static>, em_px: f32) -> PxScale {
    let upem = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(em_px * font.height_unscaled() / upem)
}

/// CJK fonts often carry full-width Latin and punctuation; those come from `latin` instead.
fn wants_latin(ch: char) -> bool {
    ch < '\u{2E80}'
}

pub struct Face {
    main: Part,
    latin: Option<Part>,
}

impl Face {
    pub fn open(candidates: &[(&str, u32)]) -> Option<Face> {
        Some(Face { main: map_font(candidates)?, latin: None })
    }

    fn with_latin(mut self, candidates: &[(&str, u32)]) -> Face {
        self.latin = map_font(candidates);
        self
    }

    pub fn describe(&self) -> String {
        match &self.latin {
            Some(l) => format!("{} + {}", self.main.source, l.source),
            None => self.main.source.clone(),
        }
    }

    fn pick(&self, ch: char) -> (&FontRef<'static>, GlyphId, bool) {
        if let Some(latin) = &self.latin {
            let id = latin.font.glyph_id(ch);
            if wants_latin(ch) && id.0 != 0 {
                return (&latin.font, id, true);
            }
        }
        (&self.main.font, self.main.font.glyph_id(ch), false)
    }

    pub fn measure(&self, text: &str, em_px: f32) -> f32 {
        let mut width = 0.0;
        let mut prev: Option<(GlyphId, bool)> = None;
        for ch in text.chars() {
            let (font, id, is_latin) = self.pick(ch);
            let scaled = font.as_scaled(px_scale(font, em_px));
            if let Some((p, _)) = prev.filter(|(_, l)| *l == is_latin) {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some((id, is_latin));
        }
        width
    }

    pub fn line_height(&self, em_px: f32) -> f32 {
        let font = &self.main.font;
        let scaled = font.as_scaled(px_scale(font, em_px));
        scaled.ascent() - scaled.descent() + scaled.line_gap()
    }

    /// Draws one line with its top edge at `top`; returns the advance width.
    pub fn draw(&self, pm: &mut Pixmap, text: &str, x: f32, top: f32, em_px: f32, rgba: [u8; 4]) -> f32 {
        let main = &self.main.font;
        let baseline = (top + main.as_scaled(px_scale(main, em_px)).ascent()).round();
        let mut caret = x;
        let mut prev: Option<(GlyphId, bool)> = None;
        let width = pm.width() as i32;
        let height = pm.height() as i32;
        let data = pm.data_mut();
        for ch in text.chars() {
            let (font, id, is_latin) = self.pick(ch);
            let scale = px_scale(font, em_px);
            let scaled = font.as_scaled(scale);
            if let Some((p, _)) = prev.filter(|(_, l)| *l == is_latin) {
                caret += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline));
            caret += scaled.h_advance(id);
            prev = Some((id, is_latin));
            let Some(outline) = font.outline_glyph(glyph) else { continue };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= width || py >= height {
                    return;
                }
                blend(data, ((py * width + px) * 4) as usize, rgba, coverage);
            });
        }
        caret - x
    }
}

fn blend(data: &mut [u8], at: usize, rgba: [u8; 4], coverage: f32) {
    let sa = (rgba[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if sa <= 0.0 {
        return;
    }
    let inv = 1.0 - sa;
    for c in 0..3 {
        let src = rgba[c] as f32 * sa;
        data[at + c] = (src + data[at + c] as f32 * inv).round().min(255.0) as u8;
    }
    data[at + 3] = (sa * 255.0 + data[at + 3] as f32 * inv).round().min(255.0) as u8;
}

pub struct Fonts {
    pub regular: Face,
    pub bold: Face,
}

impl Fonts {
    pub fn load() -> Option<Fonts> {
        #[cfg(windows)]
        let (regular, bold): (&[(&str, u32)], &[(&str, u32)]) = (
            &[(r"C:\Windows\Fonts\msyh.ttc", 1), (r"C:\Windows\Fonts\msyh.ttc", 0), (r"C:\Windows\Fonts\simsun.ttc", 0)],
            &[(r"C:\Windows\Fonts\msyhbd.ttc", 1), (r"C:\Windows\Fonts\msyhbd.ttc", 0), (r"C:\Windows\Fonts\msyh.ttc", 1)],
        );
        #[cfg(target_os = "macos")]
        let (regular, bold): (&[(&str, u32)], &[(&str, u32)]) = (
            &[("/System/Library/Fonts/PingFang.ttc", 0), ("/System/Library/Fonts/STHeiti Light.ttc", 0), ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0)],
            &[("/System/Library/Fonts/STHeiti Medium.ttc", 0), ("/System/Library/Fonts/Hiragino Sans GB.ttc", 1), ("/System/Library/Fonts/PingFang.ttc", 0)],
        );
        #[cfg(not(any(windows, target_os = "macos")))]
        let (regular, bold): (&[(&str, u32)], &[(&str, u32)]) = (
            &[("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 2)],
            &[("/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc", 2)],
        );
        // Microsoft YaHei's own Latin is fine; Mac CJK fonts set it too wide.
        #[cfg(target_os = "macos")]
        let (latin, latin_bold): (&[(&str, u32)], &[(&str, u32)]) =
            (&[("/System/Library/Fonts/Helvetica.ttc", 0)], &[("/System/Library/Fonts/Helvetica.ttc", 1)]);
        #[cfg(not(target_os = "macos"))]
        let (latin, latin_bold): (&[(&str, u32)], &[(&str, u32)]) = (&[], &[]);
        let bold = Face::open(bold).or_else(|| Face::open(regular))?.with_latin(latin_bold);
        let regular = Face::open(regular)?.with_latin(latin);
        Some(Fonts { regular, bold })
    }

    pub fn describe(&self) -> String {
        format!("regular: {}\nbold: {}", self.regular.describe(), self.bold.describe())
    }
}

/// Cuts `text` to fit `max_w`, appending an ellipsis when it had to cut.
pub fn ellipsize(face: &Face, text: &str, em_px: f32, max_w: f32) -> String {
    if face.measure(text, em_px) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if face.measure(&candidate, em_px) <= max_w {
            return candidate;
        }
        keep -= 1;
    }
    "…".to_string()
}

/// Breaks by character (fine for CJK; Latin words may split) into at most `max_lines`.
pub fn wrap(face: &Face, text: &str, em_px: f32, max_w: f32, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut next = current.clone();
        next.push(chars[i]);
        if face.measure(&next, em_px) > max_w && !current.is_empty() {
            if lines.len() + 1 == max_lines {
                let rest: String = current.clone() + &chars[i..].iter().collect::<String>();
                lines.push(ellipsize(face, &rest, em_px, max_w));
                return lines;
            }
            lines.push(std::mem::take(&mut current));
            continue;
        }
        current = next;
        i += 1;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
