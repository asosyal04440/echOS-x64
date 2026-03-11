//! # echOS Font Rendering (Font Görüntüleme)
//!
//! TrueType/OpenType font ayrıştırma, rasterleştirme ve metin düzeni.
//!
//! ## Font Pipeline (Akışı):
//!
//! ```
//!  Ham Bayt Verisi (.ttf dosyası)
//!          │
//!          ▼
//!  [TrueTypeFont::parse]   <── truetype.rs: Tablo okuma, glyph çıkarma
//!          │
//!          ▼
//!    Glyph Outline         <── Bezier eğri noktaları (vektör form)
//!          │
//!          ▼
//!  [Rasterizer::rasterize] <── rasterizer.rs: Scanline doldurma
//!          │
//!          ▼
//!    RasterGlyph Bitmap    <── Piksel alfa değerleri (0-255)
//!          │
//!          ▼
//!  [TextLayout::layout]    <── text_layout.rs: Satırları ve konumları hesapla
//!          │
//!          ▼
//!    Framebuffer'a çizim
//! ```

mod rasterizer;
mod text_layout;
mod truetype;

pub use rasterizer::{RasterGlyph, RasterMetrics, Rasterizer};
pub use text_layout::{LayoutGlyph, LayoutLine, LayoutRun, TextLayout};
pub use truetype::{FontHeader, Glyph, TrueTypeFont};

use alloc::string::String;
use alloc::vec::Vec;

/// Fontu render etmek için üst düzey tutamaç (handle).
/// TrueTypeFont + Rasterizer birleşimini sarar.
pub struct Font {
    inner: TrueTypeFont,
    rasterizer: Rasterizer,
    size: f32,
}

impl Font {
    /// Bayt diliminden font yükler (örn: include_bytes! ile gömülü font)
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let inner = TrueTypeFont::parse(data)?;
        let rasterizer = Rasterizer::new();
        Some(Self {
            inner,
            rasterizer,
            size: 16.0,
        })
    }

    /// Font boyutunu piksel cinsinden ayarlar (örn: 12.0, 16.0, 24.0)
    pub fn set_size(&mut self, size: f32) {
        self.size = size;
    }

    /// Mevcut font boyutunu döndürür
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Belirtilen karakter için ham Glyph verisini döndürür
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.inner.glyph(c)
    }

    /// Mevcut boyutta bir glyphi rasterleştirir (piksel bitmap üretir)
    pub fn rasterize_glyph(&mut self, c: char) -> Option<RasterGlyph> {
        let glyph = self.inner.glyph(c)?;
        self.rasterizer.rasterize(&self.inner, glyph, self.size)
    }

    /// Bir karakter için ilerleme genişliğini (advance width) döndürür.
    /// İki karakter arasındaki yatay mesafeyi belirler.
    pub fn advance(&self, c: char) -> f32 {
        self.inner.advance(c, self.size)
    }

    /// Satır yüksekliğini döndürür (font boyutunun 1.2 katı, standart tipografi değeri)
    pub fn line_height(&self) -> f32 {
        self.size * 1.2
    }

    /// Metnin piksel genişliğini ölçer (her karakterin advance genişliklerini toplar)
    pub fn measure_text(&self, text: &str) -> f32 {
        let mut width = 0.0;
        for c in text.chars() {
            width += self.advance(c);
        }
        width
    }

    /// Metni render için düzenler (layout hesaplar).
    /// max_width verilirse kelime kaydırma (word wrap) uygulanır.
    pub fn layout_text(&self, text: &str, max_width: Option<f32>) -> TextLayout {
        TextLayout::layout(text, &self.inner, self.size, max_width)
    }

    /// Font ailesi adını döndürür (örn: "Arial", "DejaVu Sans")
    pub fn family_name(&self) -> &str {
        &self.inner.family_name
    }

    /// Fontun sabit genişlikli (monospace) olup olmadığını kontrol eder.
    /// Kod editörleri için önemli: her karakter aynı genişlikte olmalı.
    pub fn is_monospace(&self) -> bool {
        self.inner.is_monospace
    }
}

/// Birden fazla fontu yöneten yönetici yapı.
///
/// ## Kullanım Örneği:
/// ```
/// let mut fm = FontManager::new();
/// fm.register("sans", sans_font);
/// fm.register("mono", mono_font);
/// fm.set_default("sans");
/// ```
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
        self.fonts
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f)
    }

    pub fn default_font(&self) -> Option<&Font> {
        self.default_idx
            .and_then(|i| self.fonts.get(i))
            .map(|(_, f)| f)
    }

    pub fn default_font_mut(&mut self) -> Option<&mut Font> {
        self.default_idx
            .and_then(|i| self.fonts.get_mut(i))
            .map(|(_, f)| f)
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
