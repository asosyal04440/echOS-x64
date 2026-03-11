//! # FontRenderer API
//!
//! Tüm font tiplerini birleştiren yüksek seviye metin render API.
//! Bitmap fontlar (VGA, PSF2) ve ileride TrueType için ortak arayüz sağlar.
//!
//! ## Kullanım
//!
//! ```rust,ignore
//! use crate::font::renderer::{FontRenderer, TextOptions};
//!
//! let renderer = FontRenderer::default_vga();
//! renderer.draw_str(framebuffer, 10, 20, "Hello, World!", 0xFFFFFF, None);
//! ```

use crate::gop::framebuffer::Framebuffer;
use alloc::string::String;

/// Metin render seçenekleri
#[derive(Clone, Copy, Debug)]
pub struct TextOptions {
    /// Arka plan rengi (None = transparent)
    pub background: Option<u32>,
    /// Satır aralığı (piksel)
    pub line_spacing: i32,
    /// Karakter aralığı (piksel)
    pub char_spacing: i32,
    /// Ölçekleme faktörü (1 = orijinal boyut)
    pub scale: u8,
}

impl Default for TextOptions {
    fn default() -> Self {
        TextOptions {
            background: None,
            line_spacing: 0,
            char_spacing: 0,
            scale: 1,
        }
    }
}

impl TextOptions {
    /// Arka plan rengini ayarla
    pub fn with_background(mut self, color: u32) -> Self {
        self.background = Some(color);
        self
    }

    /// Ölçeklemeyi ayarla
    pub fn with_scale(mut self, scale: u8) -> Self {
        self.scale = scale.max(1);
        self
    }
}

/// Font kaynak türü
#[derive(Clone)]
pub enum FontSource {
    /// Yerleşik VGA 8x16 font
    VgaBuiltin,
    /// PSF2 font dosyası
    Psf2(super::psf2::Psf2Font),
}

/// FontRenderer - birleşik font render API
pub struct FontRenderer {
    source: FontSource,
    glyph_width: u32,
    glyph_height: u32,
}

impl FontRenderer {
    /// Varsayılan VGA 8x16 font ile oluştur
    pub fn default_vga() -> Self {
        FontRenderer {
            source: FontSource::VgaBuiltin,
            glyph_width: 8,
            glyph_height: 16,
        }
    }

    /// PSF2 font ile oluştur
    pub fn from_psf2(font: super::psf2::Psf2Font) -> Self {
        let w = font.width();
        let h = font.height();
        FontRenderer {
            source: FontSource::Psf2(font),
            glyph_width: w,
            glyph_height: h,
        }
    }

    /// Glyph genişliği (piksel)
    pub fn glyph_width(&self) -> u32 {
        self.glyph_width
    }

    /// Glyph yüksekliği (piksel)
    pub fn glyph_height(&self) -> u32 {
        self.glyph_height
    }

    /// Metin genişliğini hesapla (piksel)
    pub fn text_width(&self, text: &str, options: Option<TextOptions>) -> u32 {
        let opts = options.unwrap_or_default();
        let scale = opts.scale as u32;
        let char_spacing = opts.char_spacing as u32;
        let char_count = text.chars().count() as u32;

        if char_count == 0 {
            return 0;
        }

        (self.glyph_width * scale * char_count) + (char_spacing * (char_count - 1))
    }

    /// Tek karakter çiz
    pub fn draw_char(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        c: char,
        color: u32,
        options: Option<TextOptions>,
    ) {
        let opts = options.unwrap_or_default();
        let scale = opts.scale.max(1) as i32;

        // Glyph verisini al
        let glyph_data: [u8; 16] = match &self.source {
            FontSource::VgaBuiltin => super::vga_font::get_font_data(c),
            FontSource::Psf2(font) => {
                let codepoint = c as u32;
                if let Some(bitmap) = font.bitmap_for_char(codepoint) {
                    let mut data = [0u8; 16];
                    let len = bitmap.len().min(16);
                    data[..len].copy_from_slice(&bitmap[..len]);
                    data
                } else {
                    // Fallback to VGA font
                    super::vga_font::get_font_data(c)
                }
            }
        };

        let fb_width = fb.width as i32;
        let fb_height = fb.height as i32;
        let stride = fb.pixels_per_scan_line;

        // Arka plan çiz (varsa)
        if let Some(bg) = opts.background {
            let w = (self.glyph_width as i32) * scale;
            let h = (self.glyph_height as i32) * scale;
            for py in 0..h {
                let screen_y = y + py;
                if screen_y < 0 || screen_y >= fb_height {
                    continue;
                }
                for px in 0..w {
                    let screen_x = x + px;
                    if screen_x < 0 || screen_x >= fb_width {
                        continue;
                    }
                    let offset = (screen_y as usize) * stride + (screen_x as usize);
                    let buffer = fb.buffer_mut();
                    if offset < buffer.len() {
                        buffer[offset] = bg;
                    }
                }
            }
        }

        // Glyph pikselleri çiz
        for row in 0..self.glyph_height {
            let byte = glyph_data[row as usize];
            for col in 0..self.glyph_width {
                if (byte >> (7 - col)) & 1 == 1 {
                    // Ölçekleme ile piksel çiz
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let screen_x = x + (col as i32) * scale + sx;
                            let screen_y = y + (row as i32) * scale + sy;

                            if screen_x >= 0
                                && screen_x < fb_width
                                && screen_y >= 0
                                && screen_y < fb_height
                            {
                                let offset = (screen_y as usize) * stride + (screen_x as usize);
                                let buffer = fb.buffer_mut();
                                if offset < buffer.len() {
                                    buffer[offset] = color;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Metin çiz
    pub fn draw_str(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        text: &str,
        color: u32,
        options: Option<TextOptions>,
    ) {
        let opts = options.unwrap_or_default();
        let scale = opts.scale.max(1) as i32;
        let char_spacing = opts.char_spacing;
        let line_spacing = opts.line_spacing;

        let char_width = (self.glyph_width as i32) * scale + char_spacing;
        let line_height = (self.glyph_height as i32) * scale + line_spacing;

        let mut cursor_x = x;
        let mut cursor_y = y;

        for c in text.chars() {
            match c {
                '\n' => {
                    cursor_x = x;
                    cursor_y += line_height;
                }
                '\r' => {
                    cursor_x = x;
                }
                '\t' => {
                    // Tab = 4 karakter boşluk
                    cursor_x += char_width * 4;
                }
                _ => {
                    self.draw_char(fb, cursor_x, cursor_y, c, color, Some(opts));
                    cursor_x += char_width;
                }
            }
        }
    }

    /// Metin çiz - satır sonunda wrap ile
    pub fn draw_str_wrapped(
        &self,
        fb: &mut Framebuffer,
        x: i32,
        y: i32,
        max_width: i32,
        text: &str,
        color: u32,
        options: Option<TextOptions>,
    ) -> i32 {
        let opts = options.unwrap_or_default();
        let scale = opts.scale.max(1) as i32;
        let char_spacing = opts.char_spacing;
        let line_spacing = opts.line_spacing;

        let char_width = (self.glyph_width as i32) * scale + char_spacing;
        let line_height = (self.glyph_height as i32) * scale + line_spacing;

        let mut cursor_x = x;
        let mut cursor_y = y;

        for c in text.chars() {
            match c {
                '\n' => {
                    cursor_x = x;
                    cursor_y += line_height;
                }
                '\r' => {
                    cursor_x = x;
                }
                '\t' => {
                    let tab_width = char_width * 4;
                    if cursor_x + tab_width > x + max_width {
                        cursor_x = x;
                        cursor_y += line_height;
                    } else {
                        cursor_x += tab_width;
                    }
                }
                _ => {
                    if cursor_x + char_width > x + max_width {
                        cursor_x = x;
                        cursor_y += line_height;
                    }
                    self.draw_char(fb, cursor_x, cursor_y, c, color, Some(opts));
                    cursor_x += char_width;
                }
            }
        }

        // Kullanılan toplam yüksekliği döndür
        cursor_y - y + line_height
    }
}

// ============================================================================
// GLOBAL FONT RENDERER
// ============================================================================

use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    /// Global varsayılan font renderer
    static ref DEFAULT_FONT: Mutex<FontRenderer> = Mutex::new(FontRenderer::default_vga());
}

/// Varsayılan font ile metin çiz (kolaylık fonksiyonu)
pub fn draw_str(fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32) {
    DEFAULT_FONT.lock().draw_str(fb, x, y, text, color, None);
}

/// Varsayılan font ile metin çiz - seçeneklerle
pub fn draw_str_opts(
    fb: &mut Framebuffer,
    x: i32,
    y: i32,
    text: &str,
    color: u32,
    options: TextOptions,
) {
    DEFAULT_FONT
        .lock()
        .draw_str(fb, x, y, text, color, Some(options));
}

/// Varsayılan font ile tek karakter çiz
pub fn draw_char(fb: &mut Framebuffer, x: i32, y: i32, c: char, color: u32) {
    DEFAULT_FONT.lock().draw_char(fb, x, y, c, color, None);
}

/// Varsayılan font glyph genişliği
pub fn glyph_width() -> u32 {
    DEFAULT_FONT.lock().glyph_width()
}

/// Varsayılan font glyph yüksekliği
pub fn glyph_height() -> u32 {
    DEFAULT_FONT.lock().glyph_height()
}

/// Metin genişliğini hesapla
pub fn text_width(text: &str) -> u32 {
    DEFAULT_FONT.lock().text_width(text, None)
}
