//! # Metin DÃƒÂ¼zeni (Text Layout)
//!
//! Metin biÃƒÂ§imlendirme ve dÃƒÂ¼zen hesaplamasÃ„Â±; karakterleri ekran koordinatlarÃ„Â±na yerleÃ…Å¸tirir.
//!
//! ## Metin DÃƒÂ¼zeni Nedir?
//! Ham bir karakter dizisini ekranda doÃ„Å¸ru konuma yerleÃ…Å¸tirmek iÃƒÂ§in gereken
//! tÃƒÂ¼m geometrik hesaplama sÃƒÂ¼recine "text layout" denir. Bu sÃƒÂ¼reÃƒÂ§ Ã…Å¸unlarÃ„Â± iÃƒÂ§erir:
//!
//! - **Shaping**: Unicode karakterleri glyph dizilerine dÃƒÂ¶nÃƒÂ¼Ã…Å¸tÃƒÂ¼rme
//! - **Metrics**: Her glyphin geniÃ…Å¸lik (advance) ve yÃƒÂ¼kseklik bilgisi
//! - **Line breaking**: Maksimum geniÃ…Å¸liÃ„Å¸e gÃƒÂ¶re satÃ„Â±r kÃ„Â±rma
//! - **Alignment**: Sola/saÃ„Å¸a/ortaya/iki yana hizalama
//! - **Hit testing**: Fare tÃ„Â±klamasÃ„Â±ndan karakter konumu bulma
//!
//! ## Veri YapÃ„Â±larÃ„Â±
//! - `LayoutGlyph`: Tek bir glyphin x/y konumu ve advance geniÃ…Å¸liÃ„Å¸i
//! - `LayoutRun`: AynÃ„Â± biÃƒÂ§imlendirmeye sahip ardÃ„Â±Ã…Å¸Ã„Â±k karakter dizisi
//! - `LayoutLine`: Bir metin satÃ„Â±rÃ„Â±; birden fazla run iÃƒÂ§erebilir
//! - `TextLayout`: TÃƒÂ¼m satÃ„Â±rlarÃ„Â± kapsayan dÃƒÂ¼zen sonucu

use super::rasterizer::RasterGlyph;
use super::truetype::{Glyph, TrueTypeFont};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

fn is_rtl_char(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
    )
}

fn reorder_bidi_runs(text: &str) -> String {
    let mut out = String::new();
    let mut run = Vec::new();
    let mut run_is_rtl = false;

    for c in text.chars() {
        let rtl = is_rtl_char(c);
        if run.is_empty() {
            run_is_rtl = rtl;
            run.push(c);
            continue;
        }

        if rtl == run_is_rtl {
            run.push(c);
        } else {
            if run_is_rtl {
                for ch in run.iter().rev() {
                    out.push(*ch);
                }
            } else {
                for ch in run.iter() {
                    out.push(*ch);
                }
            }
            run.clear();
            run_is_rtl = rtl;
            run.push(c);
        }
    }

    if run_is_rtl {
        for ch in run.iter().rev() {
            out.push(*ch);
        }
    } else {
        for ch in run.iter() {
            out.push(*ch);
        }
    }

    out
}

fn apply_ligatures(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == 'f' && chars[i + 1] == 'f' && chars[i + 2] == 'i' {
            out.push('\u{FB03}'); // ffi
            i += 3;
            continue;
        }
        if i + 2 < chars.len() && chars[i] == 'f' && chars[i + 1] == 'f' && chars[i + 2] == 'l' {
            out.push('\u{FB04}'); // ffl
            i += 3;
            continue;
        }
        if i + 1 < chars.len() && chars[i] == 'f' && chars[i + 1] == 'i' {
            out.push('\u{FB01}'); // fi
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i] == 'f' && chars[i + 1] == 'l' {
            out.push('\u{FB02}'); // fl
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i] == 'f' && chars[i + 1] == 'f' {
            out.push('\u{FB00}'); // ff
            i += 2;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

fn kerning_adjust(prev: Option<char>, current: char, size: f32) -> f32 {
    let Some(p) = prev else { return 0.0 };
    let pair = [p, current];
    let base = size * 0.06;
    match pair {
        ['A', 'V'] | ['A', 'W'] | ['A', 'Y'] | ['T', 'o'] | ['T', 'a'] | ['Y', 'o'] => -base,
        ['L', 'T'] | ['F', 'o'] => -(base * 0.75),
        _ => 0.0,
    }
}

/// DÃƒÂ¼zenlenmiÃ…Å¸ tek bir glyph'in ekran ÃƒÂ¼zerindeki konumu.
///
/// `glyph_index`: Font iÃƒÂ§indeki glyph tablosu indeksi (cmap ile Unicode'dan tÃƒÂ¼retilir).
/// `x` / `y`: SatÃ„Â±r baÃ…Å¸Ã„Â±na gÃƒÂ¶re gÃƒÂ¶reli piksel koordinatÃ„Â±.
/// `advance`: Sonraki glyphÃ„Â±n baÃ…Å¸layacaÃ„Å¸Ã„Â± x kaydÃ„Â±rma miktarÃ„Â± (piksel).
#[derive(Clone, Copy, Debug)]
pub struct LayoutGlyph {
    pub glyph_index: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}

/// AynÃ„Â± biÃƒÂ§imlendirme ÃƒÂ¶zelliklerine (renk, stil vb.) sahip ardÃ„Â±Ã…Å¸Ã„Â±k glyph dizisi.
///
/// Zengin metin sistemlerinde farklÃ„Â± renkler veya kalÃ„Â±nlÃ„Â±k gibi ÃƒÂ¶zellikler deÃ„Å¸iÃ…Å¸tikÃƒÂ§e
/// yeni bir run baÃ…Å¸lar. Sade metinde tek satÃ„Â±r genellikle tek run'dan oluÃ…Å¸ur.
#[derive(Clone, Debug)]
pub struct LayoutRun {
    pub start: usize,
    pub end: usize,
    pub glyphs: Vec<LayoutGlyph>,
    pub width: f32,
}

/// Tek bir metin satÃ„Â±rÃ„Â±nÃ„Â±n tÃƒÂ¼m geometrik verisi.
///
/// `baseline`: Metnin ana ÃƒÂ§izgisinin y koordinatÃ„Â±; harflerin diplerinin oturduÃ„Å¸u ÃƒÂ§izgi.
/// Genellikle satÃ„Â±r yÃƒÂ¼ksekliÃ„Å¸inin %80'i olarak hesaplanÃ„Â±r.
#[derive(Clone, Debug)]
pub struct LayoutLine {
    pub start: usize,
    pub end: usize,
    pub runs: Vec<LayoutRun>,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

/// TÃƒÂ¼m metnin dÃƒÂ¼zen sonucu: satÃ„Â±rlar, toplam boyut ve sarmalama geniÃ…Å¸liÃ„Å¸i.
#[derive(Clone, Debug)]
pub struct TextLayout {
    pub text: String,
    pub lines: Vec<LayoutLine>,
    pub width: f32,
    pub height: f32,
    pub max_width: Option<f32>,
}

impl TextLayout {
    /// Metni dÃƒÂ¼zenler; isteÃ„Å¸e baÃ„Å¸lÃ„Â± maksimum geniÃ…Å¸liÃ„Å¸e gÃƒÂ¶re satÃ„Â±ra sarar.
    ///
    /// `scale = size / units_per_em` ile font birimi Ã¢â€ â€™ piksel dÃƒÂ¶nÃƒÂ¼Ã…Å¸ÃƒÂ¼mÃƒÂ¼ yapÃ„Â±lÃ„Â±r.
    /// `line_height = size * 1.2` tipik bir satÃ„Â±r aralÃ„Â±Ã„Å¸Ã„Â± katsayÃ„Â±sÃ„Â±dÃ„Â±r (%120).
    /// Algoritma karakterleri tek tek dolaÃ…Å¸Ã„Â±r:
    /// - `\n` Ã¢â€ â€™ satÃ„Â±r sonu zorlamasÃ„Â±
    /// - BoÃ…Å¸luk/tire ve max_width aÃ…Å¸Ã„Â±mÃ„Â± Ã¢â€ â€™ kelime sarmalama
    pub fn layout(text: &str, font: &TrueTypeFont, size: f32, max_width: Option<f32>) -> Self {
        let bidi_reordered = reorder_bidi_runs(text);
        let shaped_text = apply_ligatures(&bidi_reordered);
        let scale = size / font.units_per_em as f32;
        let line_height = size * 1.2;

        let mut lines = Vec::new();
        let mut current_line_start = 0;
        let mut current_run_glyphs = Vec::new();
        let mut current_run_start = 0;
        let mut current_run_width = 0.0;
        let mut current_line_width = 0.0;
        let mut current_x = 0.0;
        let mut prev_char: Option<char> = None;

        for (i, c) in shaped_text.char_indices() {
            let is_newline = c == '\n';
            let is_break_char = c == ' ' || c == '-' || c == '\t';

            // Get glyph
            let glyph = font.glyph(c);
            let mut advance = glyph
                .map(|g| g.advance_width as f32 * scale)
                .unwrap_or(size * 0.3);
            let kern = kerning_adjust(prev_char, c, size);
            advance += kern;

            // Check for line break
            if is_newline {
                // Finish current run and line
                if !current_run_glyphs.is_empty() {
                    current_run_glyphs.push(LayoutGlyph {
                        glyph_index: 0,
                        x: current_x,
                        y: 0.0,
                        advance: 0.0,
                    });

                    lines.push(LayoutLine {
                        start: current_line_start,
                        end: i,
                        runs: vec![LayoutRun {
                            start: current_run_start,
                            end: i,
                            glyphs: current_run_glyphs.clone(),
                            width: current_run_width,
                        }],
                        width: current_line_width,
                        height: line_height,
                        baseline: line_height * 0.8,
                    });
                } else {
                    lines.push(LayoutLine {
                        start: current_line_start,
                        end: i,
                        runs: Vec::new(),
                        width: 0.0,
                        height: line_height,
                        baseline: line_height * 0.8,
                    });
                }

                current_line_start = i + c.len_utf8();
                current_run_start = i + c.len_utf8();
                current_run_glyphs.clear();
                current_run_width = 0.0;
                current_line_width = 0.0;
                current_x = 0.0;
                prev_char = None;
                continue;
            }

            // Check for word wrap
            if let Some(max_w) = max_width {
                if current_line_width + advance > max_w && is_break_char {
                    // Word wrap - finish current line
                    if !current_run_glyphs.is_empty() {
                        lines.push(LayoutLine {
                            start: current_line_start,
                            end: i,
                            runs: vec![LayoutRun {
                                start: current_run_start,
                                end: i,
                                glyphs: current_run_glyphs.clone(),
                                width: current_run_width,
                            }],
                            width: current_line_width,
                            height: line_height,
                            baseline: line_height * 0.8,
                        });

                        current_line_start = i;
                        current_run_start = i;
                        current_run_glyphs.clear();
                        current_run_width = 0.0;
                        current_line_width = 0.0;
                        current_x = 0.0;
                    }
                }
            }

            // Add glyph to current run
            let glyph_index = glyph.map(|g| g.index).unwrap_or(0);
            current_run_glyphs.push(LayoutGlyph {
                glyph_index,
                x: current_x,
                y: 0.0,
                advance,
            });

            current_x += advance;
            current_run_width += advance;
            current_line_width += advance;
            prev_char = Some(c);
        }

        // Finish last line
        if !current_run_glyphs.is_empty() || current_line_start < shaped_text.len() {
            lines.push(LayoutLine {
                start: current_line_start,
                end: shaped_text.len(),
                runs: vec![LayoutRun {
                    start: current_run_start,
                    end: shaped_text.len(),
                    glyphs: current_run_glyphs,
                    width: current_run_width,
                }],
                width: current_line_width,
                height: line_height,
                baseline: line_height * 0.8,
            });
        }

        // Calculate total dimensions
        let total_width = lines
            .iter()
            .map(|l| l.width)
            .fold(0.0f32, |a: f32, b: f32| if a > b { a } else { b });
        let total_height = lines.len() as f32 * line_height;

        Self {
            text: shaped_text,
            lines,
            width: total_width,
            height: total_height,
            max_width,
        }
    }

    /// Get line at y position
    pub fn line_at(&self, y: f32) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }

        let line_height = self.lines[0].height;
        let line_idx = (y / line_height) as usize;

        if line_idx < self.lines.len() {
            Some(line_idx)
        } else {
            None
        }
    }

    /// Get character position at (x, y)
    pub fn hit_test(&self, x: f32, y: f32) -> Option<usize> {
        let line_idx = self.line_at(y)?;
        let line = self.lines.get(line_idx)?;

        // Find run
        for run in &line.runs {
            let mut current_x = 0.0;
            for glyph in &run.glyphs {
                if x >= current_x && x < current_x + glyph.advance {
                    // Found character
                    // Approximate character position within glyph
                    return Some(run.start);
                }
                current_x += glyph.advance;
            }
        }

        // Return end of line
        Some(line.end)
    }

    /// Get cursor position for character index
    pub fn cursor_position(&self, char_index: usize) -> (f32, f32) {
        for (line_idx, line) in self.lines.iter().enumerate() {
            if char_index >= line.start && char_index <= line.end {
                let line_height = line.height;
                let y = line_idx as f32 * line_height;

                // Find x position
                for run in &line.runs {
                    if char_index >= run.start && char_index <= run.end {
                        let mut x = 0.0;
                        for (i, glyph) in run.glyphs.iter().enumerate() {
                            if run.start + i >= char_index {
                                return (x, y);
                            }
                            x += glyph.advance;
                        }
                        return (x, y);
                    }
                }

                return (line.width, y);
            }
        }

        (0.0, 0.0)
    }

    /// Get selection rectangles for range
    pub fn selection_rects(&self, start: usize, end: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut rects = Vec::new();

        for (line_idx, line) in self.lines.iter().enumerate() {
            if end <= line.start || start >= line.end {
                continue;
            }

            let line_start = start.max(line.start);
            let line_end = end.min(line.end);

            let (start_x, _) = self.cursor_position(line_start);
            let (end_x, _) = self.cursor_position(line_end);

            let y = line_idx as f32 * line.height;
            let height = line.height;

            rects.push((start_x, y, end_x - start_x, height));
        }

        rects
    }

    /// Layout with alignment
    pub fn with_alignment(mut self, alignment: TextAlignment, max_width: f32) -> Self {
        let shaped_text = self.text.clone();
        let line_count = self.lines.len();

        for line_idx in 0..line_count {
            let line = &mut self.lines[line_idx];
            let mut offset = 0.0;

            match alignment {
                TextAlignment::Left => {}
                TextAlignment::Center => {
                    offset = (max_width - line.width) / 2.0;
                }
                TextAlignment::Right => {
                    offset = max_width - line.width;
                }
                TextAlignment::Justify => {
                    let extra = max_width - line.width;
                    let is_last_line = line_idx + 1 == line_count;
                    if !is_last_line && extra > 0.0 {
                        let mut whitespace_count = 0usize;
                        for run in &line.runs {
                            whitespace_count += shaped_text[run.start..run.end]
                                .chars()
                                .take(run.glyphs.len())
                                .filter(|ch| ch.is_whitespace())
                                .count();
                        }

                        if whitespace_count > 0 {
                            let extra_per_gap = extra / whitespace_count as f32;
                            let mut distributed_total = 0.0;
                            for run in &mut line.runs {
                                let mut run_extra = 0.0;
                                for (glyph, ch) in run
                                    .glyphs
                                    .iter_mut()
                                    .zip(shaped_text[run.start..run.end].chars())
                                {
                                    glyph.x += distributed_total;
                                    if ch.is_whitespace() {
                                        glyph.advance += extra_per_gap;
                                        distributed_total += extra_per_gap;
                                        run_extra += extra_per_gap;
                                    }
                                }
                                run.width += run_extra;
                            }
                            line.width = max_width;
                        }
                    }
                }
            }

            for run in &mut line.runs {
                for glyph in &mut run.glyphs {
                    glyph.x += offset;
                }
            }
        }

        self
    }
}

/// Text alignment
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
    Justify,
}

/// Text attributes for rich text
#[derive(Clone, Copy, Debug)]
pub struct TextAttributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: u32,
}

impl Default for TextAttributes {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            color: 0xFFFFFF,
        }
    }
}

/// Rich text run with attributes
#[derive(Clone, Debug)]
pub struct RichTextRun {
    pub text: String,
    pub attributes: TextAttributes,
}

/// Rich text layout
#[derive(Clone, Debug)]
pub struct RichTextLayout {
    pub runs: Vec<RichTextRun>,
    pub layout: TextLayout,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_font() -> TrueTypeFont {
        TrueTypeFont::parse(include_bytes!("../../../assets/fonts/Roboto.ttf"))
            .expect("Roboto.ttf should parse as a TrueType font")
    }

    fn line_space_advances(layout: &TextLayout, line_idx: usize) -> Vec<f32> {
        let line = &layout.lines[line_idx];
        let mut advances = Vec::new();
        for run in &line.runs {
            for (glyph, ch) in run
                .glyphs
                .iter()
                .zip(layout.text[run.start..run.end].chars())
            {
                if ch.is_whitespace() {
                    advances.push(glyph.advance);
                }
            }
        }
        advances
    }

    #[test]
    fn justify_distributes_extra_width_across_whitespace_on_non_terminal_lines() {
        let font = load_font();
        let layout = TextLayout::layout("alpha beta\nomega", &font, 18.0, None);
        let target_width = layout.lines[0].width + 48.0;
        let justified = layout
            .clone()
            .with_alignment(TextAlignment::Justify, target_width);

        assert!((justified.lines[0].width - target_width).abs() < 0.01);
        assert!((justified.lines[1].width - layout.lines[1].width).abs() < 0.01);

        let original_spaces = line_space_advances(&layout, 0);
        let justified_spaces = line_space_advances(&justified, 0);
        assert_eq!(original_spaces.len(), justified_spaces.len());
        assert!(justified_spaces
            .iter()
            .zip(original_spaces.iter())
            .all(|(after, before)| after > before));
    }
}
