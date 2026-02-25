//! # echOS Font Rendering
//!
//! TrueType/OpenType font parsing, rasterization, and text layout.

mod truetype;
mod rasterizer;
mod text_layout;

pub use truetype::{TrueTypeFont, FontHeader, Glyph};
pub use rasterizer::{Rasterizer, RasterGlyph, RasterMetrics};
pub use text_layout::{TextLayout, LayoutLine, LayoutRun, LayoutGlyph};

use alloc::vec::Vec;
use alloc::string::String;

/// Font handle for rendering
pub struct Font {
    inner: TrueTypeFont,
    rasterizer: Rasterizer,
    size: f32,
}

impl Font {
    /// Load font from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let inner = TrueTypeFont::parse(data)?;
        let rasterizer = Rasterizer::new();
        Some(Self {
            inner,
            rasterizer,
            size: 16.0,
        })
    }

    /// Set font size in pixels
    pub fn set_size(&mut self, size: f32) {
        self.size = size;
    }

    /// Get current font size
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Get glyph for character
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.inner.glyph(c)
    }

    /// Rasterize glyph at current size
    pub fn rasterize_glyph(&mut self, c: char) -> Option<RasterGlyph> {
        let glyph = self.inner.glyph(c)?;
        self.rasterizer.rasterize(&self.inner, glyph, self.size)
    }

    /// Get advance width for character
    pub fn advance(&self, c: char) -> f32 {
        self.inner.advance(c, self.size)
    }

    /// Get line height
    pub fn line_height(&self) -> f32 {
        self.size * 1.2
    }

    /// Measure text width
    pub fn measure_text(&self, text: &str) -> f32 {
        let mut width = 0.0;
        for c in text.chars() {
            width += self.advance(c);
        }
        width
    }

    /// Layout text for rendering
    pub fn layout_text(&self, text: &str, max_width: Option<f32>) -> TextLayout {
        TextLayout::layout(text, &self.inner, self.size, max_width)
    }

    /// Get font family name
    pub fn family_name(&self) -> &str {
        &self.inner.family_name
    }

    /// Check if font is monospace
    pub fn is_monospace(&self) -> bool {
        self.inner.is_monospace
    }
}

/// Font manager for handling multiple fonts
pub struct FontManager {
    fonts: Vec<(String, Font)>,
    default_idx: Option<usize>,
}

impl FontManager {
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            default_idx: None,
        }
    }

    pub fn register(&mut self, name: &str, font: Font) {
        let is_first = self.fonts.is_empty();
        self.fonts.push((String::from(name), font));
        if is_first {
            self.default_idx = Some(0);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Font> {
        self.fonts.iter().find(|(n, _)| n == name).map(|(_, f)| f)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Font> {
        self.fonts.iter_mut().find(|(n, _)| n == name).map(|(_, f)| f)
    }

    pub fn default_font(&self) -> Option<&Font> {
        self.default_idx.and_then(|i| self.fonts.get(i)).map(|(_, f)| f)
    }

    pub fn default_font_mut(&mut self) -> Option<&mut Font> {
        self.default_idx.and_then(|i| self.fonts.get_mut(i)).map(|(_, f)| f)
    }

    pub fn set_default(&mut self, name: &str) -> bool {
        if let Some(idx) = self.fonts.iter().position(|(n, _)| n == name) {
            self.default_idx = Some(idx);
            true
        } else {
            false
        }
    }

    pub fn font_names(&self) -> Vec<&str> {
        self.fonts.iter().map(|(n, _)| n.as_str()).collect()
    }
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}
