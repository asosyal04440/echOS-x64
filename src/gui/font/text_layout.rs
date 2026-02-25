//! # Text Layout
//!
//! Text shaping and layout for rendering.

use super::truetype::{TrueTypeFont, Glyph};
use super::rasterizer::RasterGlyph;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// Layout glyph position
#[derive(Clone, Copy, Debug)]
pub struct LayoutGlyph {
    pub glyph_index: u16,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}

/// Layout run (contiguous text with same formatting)
#[derive(Clone, Debug)]
pub struct LayoutRun {
    pub start: usize,
    pub end: usize,
    pub glyphs: Vec<LayoutGlyph>,
    pub width: f32,
}

/// Layout line
#[derive(Clone, Debug)]
pub struct LayoutLine {
    pub start: usize,
    pub end: usize,
    pub runs: Vec<LayoutRun>,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

/// Text layout result
#[derive(Clone, Debug)]
pub struct TextLayout {
    pub text: String,
    pub lines: Vec<LayoutLine>,
    pub width: f32,
    pub height: f32,
    pub max_width: Option<f32>,
}

impl TextLayout {
    /// Layout text with optional max width
    pub fn layout(text: &str, font: &TrueTypeFont, size: f32, max_width: Option<f32>) -> Self {
        let scale = size / font.units_per_em as f32;
        let line_height = size * 1.2;
        
        let mut lines = Vec::new();
        let mut current_line_start = 0;
        let mut current_run_glyphs = Vec::new();
        let mut current_run_start = 0;
        let mut current_run_width = 0.0;
        let mut current_line_width = 0.0;
        let mut current_x = 0.0;
        
        for (i, c) in text.char_indices() {
            let is_newline = c == '\n';
            let is_space = c == ' ';
            let is_break_char = c == ' ' || c == '-' || c == '\t';
            
            // Get glyph
            let glyph = font.glyph(c);
            let advance = glyph.map(|g| g.advance_width as f32 * scale).unwrap_or(size * 0.3);
            
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
        }
        
        // Finish last line
        if !current_run_glyphs.is_empty() || current_line_start < text.len() {
            lines.push(LayoutLine {
                start: current_line_start,
                end: text.len(),
                runs: vec![LayoutRun {
                    start: current_run_start,
                    end: text.len(),
                    glyphs: current_run_glyphs,
                    width: current_run_width,
                }],
                width: current_line_width,
                height: line_height,
                baseline: line_height * 0.8,
            });
        }
        
        // Calculate total dimensions
        let total_width = lines.iter().map(|l| l.width).fold(0.0f32, |a: f32, b: f32| if a > b { a } else { b });
        let total_height = lines.len() as f32 * line_height;
        
        Self {
            text: String::from(text),
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
        for line in &mut self.lines {
            let offset = match alignment {
                TextAlignment::Left => 0.0,
                TextAlignment::Center => (max_width - line.width) / 2.0,
                TextAlignment::Right => max_width - line.width,
                TextAlignment::Justify => {
                    // Distribute extra space among glyphs
                    0.0 // TODO: Implement justification
                }
            };
            
            // Offset all glyphs
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
