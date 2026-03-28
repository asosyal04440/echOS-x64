//! # Font Rasterleyici (Rasterizer)
//!
//! Glyph (harf ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ekli) dÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ hat vektÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rlerini piksel bitmap'lerine dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼r.
//!
//! ## Rasterizasyon Nedir?
//! VektÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶r tabanlÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± font verileri (bezier eÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸rileri, doÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸ru parÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§alarÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±) matematiksel
//! formÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼llere dayanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r; ekrana ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§izmek iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in bu formÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼lleri piksel ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±zgarasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±na
//! dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rmek gerekir. Bu iÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸leme "rasterizasyon" denir.
//!
//! ## Temel Kavramlar
//! - **Glyph**: Tek bir karakterin gÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rsel temsili (ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rn. 'A' harfinin ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ekli)
//! - **Contour**: Bir glyphin kapalÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± yolunu oluÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸turan nokta dizisi
//! - **Scanline algoritmasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±**: Her yatay tarama satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in yol kesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸imlerini
//!   bulur ve doldurulacak pikselleri belirler
//! - **Winding number**: Bir noktanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±n contour iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§inde mi dÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nda mÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± olduÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸unu
//!   belirleyen sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±; imzalÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± kesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸im sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±yla hesaplanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r
//! - **Anti-aliasing**: Kenar piksellerine kÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±smi saydamlÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±k deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸eri atanarak
//!   kademeli geÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§iÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ oluÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸turulmasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± ve pÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼zlÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ gÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼mÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼n azaltÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lmasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±
//! - **Advance width**: Bir karakterden sonra bir sonrakini yerleÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tirmek iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in
//!   ilerlenecek yatay piksel mesafesi

use super::truetype::{Glyph, GlyphContour, GlyphPoint, TrueTypeFont};
use alloc::vec;
use alloc::vec::Vec;
use libm::{powf, roundf};

/// no_std ortamÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶zel tavan (ceiling) fonksiyonu.
/// Standart kÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼tÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼phanede `f32::ceil()` bulunur; ancak ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§ekirdek modunda
/// kayan nokta runtime desteÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸i olmadÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ndan cast hilesine baÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸vuruyoruz:
/// tamsayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ya dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼m otomatik olarak `truncate` (sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±fÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ra yuvarlama)
/// yapar; pozitif sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lar iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in truncate == floor olduÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸undan bir ekleme
/// ile tavan elde edilir.
fn ceil_f32(x: f32) -> f32 {
    let i = x as i32 as f32;
    if x > i {
        i + 1.0
    } else {
        i
    }
}

/// no_std ortamÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶zel taban (floor) fonksiyonu.
/// `ceil_f32` ile aynÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± mantÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±k; negatif sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±larda truncate deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸eri
/// matematiksel tabandan bÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼k olduÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸undan bunu dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼zeltmek iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in
/// bir ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±karma yapÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r.
fn floor_f32(x: f32) -> f32 {
    let i = x as i32 as f32;
    if x < i {
        i - 1.0
    } else {
        i
    }
}

/// Rasterize edilmiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ (pikselleÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tirilmiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸) glyph bitmap'i.
///
/// VektÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rden piksel ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±zgarasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±na dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼lmÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ harf verisi.
/// `bitmap` alanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±, her piksel iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in 0ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â€šÂ¬Ã…â€œ255 arasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± alfa (saydamlÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±k)
/// deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸erleri iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§erir: 0 = tam saydam, 255 = tam opak.
#[derive(Clone, Debug)]
pub struct RasterGlyph {
    pub width: usize,
    pub height: usize,
    pub offset_x: i32,
    pub offset_y: i32,
    pub advance: f32,
    pub bitmap: Vec<u8>, // Her piksel iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in alfa deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸eri (0-255)
}

/// Rasterizasyon metrik verileri.
///
/// Bitmap'in framebuffer ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼zerindeki konumunu belirlemek iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in kullanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r:
/// `offset_x`/`offset_y` glyphin sol altÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ndan ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶lÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼len imzero-point ofsetidir.
#[derive(Clone, Copy, Debug)]
pub struct RasterMetrics {
    pub width: usize,
    pub height: usize,
    pub offset_x: i32,
    pub offset_y: i32,
}

/// Glyph rasterleyici ana yapÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±.
///
/// Dahili tamponlarÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± yeniden kullanarak her rasterizasyon iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in
/// heap ayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rma maliyetini dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼r. `scanline` tamponu bir satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rdaki
/// her sÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼tun iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in kaplama (coverage) deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸erini tutar;
/// `winding` tamponu ise o noktanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±n contour iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§inde olup olmadÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±
/// belirlemek iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in imzalÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± kesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸im sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± depolar.
pub struct Rasterizer {
    // Tarama satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± iÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸leme iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in dahili tampon; her ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§aÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±da temizlenir
    scanline: Vec<f32>,
    // Winding sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± tamponu; pozitif = sola dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶ndÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rme, negatif = saÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸a dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶ndÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼rme
    winding: Vec<i32>,
    subpixel_aa: bool,
    gamma: f32,
}

impl Rasterizer {
    pub fn new() -> Self {
        Self {
            scanline: Vec::new(),
            winding: Vec::new(),
            subpixel_aa: true,
            gamma: 2.2,
        }
    }

    pub fn rasterize(
        &mut self,
        font: &TrueTypeFont,
        glyph: &Glyph,
        size: f32,
    ) -> Option<RasterGlyph> {
        let scale = if font.units_per_em == 0 {
            1.0
        } else {
            size / font.units_per_em as f32
        };
        let metrics = Self::compute_metrics(glyph, scale);
        if metrics.width == 0 || metrics.height == 0 {
            return Some(RasterGlyph {
                width: 0,
                height: 0,
                offset_x: metrics.offset_x,
                offset_y: metrics.offset_y,
                advance: glyph.advance_width as f32 * scale,
                bitmap: Vec::new(),
            });
        }

        let mut bitmap = vec![0u8; metrics.width * metrics.height];
        if glyph.contours.is_empty() {
            self.render_simple(&mut bitmap, metrics.width, metrics.height, glyph, scale);
        } else {
            self.render_outline(&mut bitmap, metrics.width, metrics.height, glyph, scale);
        }
        if self.subpixel_aa {
            self.apply_subpixel_gamma(&mut bitmap, metrics.width, metrics.height);
        }

        Some(RasterGlyph {
            width: metrics.width,
            height: metrics.height,
            offset_x: metrics.offset_x,
            offset_y: metrics.offset_y,
            advance: glyph.advance_width as f32 * scale,
            bitmap,
        })
    }

    fn compute_metrics(glyph: &Glyph, scale: f32) -> RasterMetrics {
        let x_min = floor_f32(glyph.bounds.x_min as f32 * scale) as i32;
        let y_min = floor_f32(glyph.bounds.y_min as f32 * scale) as i32;
        let x_max = ceil_f32(glyph.bounds.x_max as f32 * scale) as i32;
        let y_max = ceil_f32(glyph.bounds.y_max as f32 * scale) as i32;
        RasterMetrics {
            width: x_max.saturating_sub(x_min).max(0) as usize,
            height: y_max.saturating_sub(y_min).max(0) as usize,
            offset_x: x_min,
            offset_y: y_max,
        }
    }

    /// Bitmap fallback for contourless glyphs.
    ///
    /// Real outline glyphs now flow through `render_outline()`; this path stays
    /// limited to contour-free glyphs whose bounds still need a visible alpha mask.
    fn render_simple(
        &mut self,
        bitmap: &mut [u8],
        width: usize,
        height: usize,
        glyph: &Glyph,
        scale: f32,
    ) {
        let bounds = &glyph.bounds;

        // SÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rlayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±cÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± kutu deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸iÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸kenleri; ileride bezier ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rnekleme iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in kullanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lacak
        let x_start = 0usize;
        let x_end = width;
        let y_start = 0usize;
        let y_end = height;

        // TÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼m pikselleri tam opak (255) ile doldur
        for y in y_start..y_end {
            for x in x_start..x_end {
                let idx = y * width + x;
                if idx < bitmap.len() {
                    // Contoursuz glyph fallback pathi tam opak ic bolge ile yayinlanir.
                    bitmap[idx] = 255;
                }
            }
        }

        // Fallback yolunda kenarlar yari opak yazilir; outline glyphler bu yolu kullanmaz.
        // Outline coverage ayrik supersampling ile ayrica hesaplanir.
        if width > 2 && height > 2 {
            for x in 0..width {
                // ÃƒÆ’Ã†â€™Ãƒâ€¦Ã¢â‚¬Å“st kenar
                bitmap[x] = 128;
                // Alt kenar
                bitmap[(height - 1) * width + x] = 128;
            }
            for y in 0..height {
                // Sol kenar
                bitmap[y * width] = 128;
                // SaÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸ kenar
                bitmap[y * width + width - 1] = 128;
            }
        }
    }

    /// Scanline algoritmasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±yla glyph dÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ hatlarÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± rasterize eder.
    ///
    /// Klasik dolu-ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§okgen rasterizasyon yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶ntemi:
    /// 1. Her yatay tarama satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± (scanline) iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§in tÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼m contour kenarlarÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± dolaÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸.
    /// 2. KenarÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±n o satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rla kesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ip kesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸mediÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸ini kontrol et.
    /// 3. KesiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸im noktasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ndaki winding sayÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±nÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â± gÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ncelle.
    /// 4. Winding ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â°Ãƒâ€šÃ‚Â  0 olan sÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼tunlar ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§izgi iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§indedir ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€Â¢ pikseli doldur.
    /// Bu yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶ntem TrueType'ÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±n "non-zero winding" doldurma kuralÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±na uygundur.
    fn apply_subpixel_gamma(&self, bitmap: &mut [u8], width: usize, height: usize) {
        if width < 3 || height == 0 {
            return;
        }

        let mut filtered = vec![0u8; bitmap.len()];
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let l = bitmap[y * width + x.saturating_sub(1)] as u32;
                let c = bitmap[idx] as u32;
                let r = bitmap[y * width + (x + 1).min(width - 1)] as u32;
                let linear = (l + (2 * c) + r) / 4;
                let norm = (linear as f32 / 255.0).clamp(0.0, 1.0);
                let gamma_corrected = powf(norm, 1.0 / self.gamma.max(1.0));
                filtered[idx] = (gamma_corrected * 255.0) as u8;
            }
        }
        bitmap.copy_from_slice(&filtered);
    }

    fn render_outline(
        &mut self,
        bitmap: &mut [u8],
        width: usize,
        height: usize,
        glyph: &Glyph,
        scale: f32,
    ) {
        if self.scanline.len() < width {
            self.scanline.resize(width, 0.0);
        }
        if self.winding.len() < width {
            self.winding.resize(width, 0);
        }

        let contours: Vec<Vec<(f32, f32)>> = glyph
            .contours
            .iter()
            .map(|contour| self.flatten_contour(&contour.points, scale))
            .filter(|contour| contour.len() >= 3)
            .collect();

        if contours.is_empty() {
            self.render_simple(bitmap, width, height, glyph, scale);
            return;
        }

        let sample_offsets = [0.125f32, 0.375f32, 0.625f32, 0.875f32];
        let bounds = &glyph.bounds;

        for y in 0..height {
            for x in 0..width {
                let mut covered = 0u32;
                for &sy in &sample_offsets {
                    let sample_y = bounds.y_max as f32 - (y as f32 + sy) / scale;
                    for &sx in &sample_offsets {
                        let sample_x = bounds.x_min as f32 + (x as f32 + sx) / scale;
                        if Self::point_in_outline(sample_x, sample_y, &contours) {
                            covered += 1;
                        }
                    }
                }

                let coverage = covered as f32 / 16.0;
                self.scanline[x] = coverage;
                bitmap[y * width + x] = roundf(coverage * 255.0) as u8;
            }
        }
    }

    fn flatten_contour(&self, points: &[GlyphPoint], scale: f32) -> Vec<(f32, f32)> {
        if points.is_empty() {
            return Vec::new();
        }

        let mut expanded = Vec::with_capacity(points.len() * 2);
        for i in 0..points.len() {
            let current = points[i];
            let next = points[(i + 1) % points.len()];
            expanded.push(current);
            if !current.on_curve && !next.on_curve {
                expanded.push(GlyphPoint {
                    x: ((current.x as i32 + next.x as i32) / 2) as i16,
                    y: ((current.y as i32 + next.y as i32) / 2) as i16,
                    on_curve: true,
                });
            }
        }

        let start_idx = expanded
            .iter()
            .position(|point| point.on_curve)
            .unwrap_or(0);
        let mut ordered = Vec::with_capacity(expanded.len());
        for i in 0..expanded.len() {
            ordered.push(expanded[(start_idx + i) % expanded.len()]);
        }

        if ordered.is_empty() {
            return Vec::new();
        }

        let mut flattened = Vec::new();
        let start = (ordered[0].x as f32, ordered[0].y as f32);
        flattened.push(start);

        let mut cursor = 0usize;
        while cursor < ordered.len() {
            let current = ordered[cursor % ordered.len()];
            let next = ordered[(cursor + 1) % ordered.len()];

            if next.on_curve {
                if (cursor + 1) % ordered.len() != 0 {
                    flattened.push((next.x as f32, next.y as f32));
                }
                cursor += 1;
                continue;
            }

            let end = ordered[(cursor + 2) % ordered.len()];
            self.append_quadratic(
                &mut flattened,
                (current.x as f32, current.y as f32),
                (next.x as f32, next.y as f32),
                (end.x as f32, end.y as f32),
                scale,
            );
            cursor += 2;
        }

        if flattened.last().copied() != Some(start) {
            flattened.push(start);
        }

        flattened
    }

    fn append_quadratic(
        &self,
        flattened: &mut Vec<(f32, f32)>,
        start: (f32, f32),
        control: (f32, f32),
        end: (f32, f32),
        scale: f32,
    ) {
        let span = ((start.0 - control.0).abs() + (start.1 - control.1).abs())
            .max((control.0 - end.0).abs() + (control.1 - end.1).abs())
            .max((start.0 - end.0).abs() + (start.1 - end.1).abs());
        let segments = ceil_f32((span * scale) / 8.0).clamp(4.0, 24.0) as usize;

        for step in 1..=segments {
            let t = step as f32 / segments as f32;
            let inv_t = 1.0 - t;
            let x = inv_t * inv_t * start.0 + 2.0 * inv_t * t * control.0 + t * t * end.0;
            let y = inv_t * inv_t * start.1 + 2.0 * inv_t * t * control.1 + t * t * end.1;
            flattened.push((x, y));
        }
    }

    fn point_in_outline(px: f32, py: f32, contours: &[Vec<(f32, f32)>]) -> bool {
        let mut winding = 0i32;

        for contour in contours {
            if contour.len() < 2 {
                continue;
            }

            for edge in contour.windows(2) {
                let (x0, y0) = edge[0];
                let (x1, y1) = edge[1];

                if y0 <= py {
                    if y1 > py {
                        let cross = (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0);
                        if cross > 0.0 {
                            winding += 1;
                        }
                    }
                } else if y1 <= py {
                    let cross = (x1 - x0) * (py - y0) - (px - x0) * (y1 - y0);
                    if cross < 0.0 {
                        winding -= 1;
                    }
                }
            }
        }

        winding != 0
    }

    /// 8ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â8 piksel bitmap font karakterini rasterize eder.
    ///
    /// Bitmap fontlar, her karakteri 8 baytlÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±k bir dizi ile temsil eder:
    /// her bayt bir satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±, her bit ise o satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rdaki bir pikseli gÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶sterir.
    /// `0x80 >> col` maskesi ile sÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼tun biti test edilir; `scale` faktÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶rÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼
    /// ile bÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼tme yapÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±larak her bit birden fazla piksel kapsar.
    /// Bu yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶ntem TrueType ayrÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸tÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rma baÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸arÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±sÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±z olduÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸unda geri dÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¶nÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ olarak kullanÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±lÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r.
    pub fn rasterize_bitmap(&self, char_bits: &[u8; 8], size: f32) -> RasterGlyph {
        let scale = (size / 8.0).max(1.0) as usize;
        let width = 8 * scale;
        let height = 8 * scale;

        let mut bitmap = vec![0u8; width * height];

        for (row, &bits) in char_bits.iter().enumerate() {
            for col in 0..8 {
                if bits & (0x80 >> col) != 0 {
                    // ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“lÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§eklenmiÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ piksel bloÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸unu doldur:
                    // scale=2 ise her bit 2ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â2 piksel bloÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸una geniÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ler
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = col * scale + dx;
                            let y = row * scale + dy;
                            let idx = y * width + x;
                            if idx < bitmap.len() {
                                bitmap[idx] = 255;
                            }
                        }
                    }
                }
            }
        }

        RasterGlyph {
            width,
            height,
            offset_x: 0,
            offset_y: height as i32,
            advance: width as f32,
            bitmap,
        }
    }
}

impl Default for Rasterizer {
    fn default() -> Self {
        Self::new()
    }
}

/// YerleÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸ik 8ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â8 piksel bitmap font verisi (96 yazdÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±rÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±labilir ASCII karakter).
///
/// ASCII tablosunda 0x20 (boÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸luk) ile 0x7F (DEL) arasÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±ndaki karakterleri iÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â§erir.
/// Her karakter 8 baytla temsil edilir: bir bayt = bir yatay satÃƒÆ’Ã¢â‚¬ÂÃƒâ€šÃ‚Â±r,
/// bir bit = bir piksel (1=dolu, 0=boÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€¦Ã‚Â¸). En yÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¼ksek deÃƒÆ’Ã¢â‚¬ÂÃƒâ€¦Ã‚Â¸erlikli bit (MSB) solda.
///
/// ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“rnek: 'A' (0x41)
/// ```text
/// 0x38 = 00111000  ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€Â¢   ***
/// 0x6C = 01101100  ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€Â¢  ** **
/// 0xC6 = 11000110  ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€Â¢ **   **
/// 0xFE = 11111110  ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€Â¢ *******
/// ... vb.
/// ```
pub const BITMAP_FONT: [[u8; 8]; 96] = [
    // Space (0x20)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // ! (0x21)
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00],
    // " (0x22)
    [0x6C, 0x6C, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00],
    // # (0x23)
    [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00],
    // $ (0x24)
    [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00],
    // % (0x25)
    [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00],
    // & (0x26)
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00],
    // ' (0x27)
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00],
    // ( (0x28)
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00],
    // ) (0x29)
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00],
    // * (0x2A)
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00],
    // + (0x2B)
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00],
    // , (0x2C)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30],
    // - (0x2D)
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
    // . (0x2E)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
    // / (0x2F)
    [0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00],
    // 0 (0x30)
    [0x7C, 0xC6, 0xCE, 0xD6, 0xE6, 0xC6, 0x7C, 0x00],
    // 1 (0x31)
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
    // 2 (0x32)
    [0x7C, 0xC6, 0x06, 0x1C, 0x30, 0x66, 0xFE, 0x00],
    // 3 (0x33)
    [0x7C, 0xC6, 0x06, 0x3C, 0x06, 0xC6, 0x7C, 0x00],
    // 4 (0x34)
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00],
    // 5 (0x35)
    [0xFE, 0xC0, 0xC0, 0xFC, 0x06, 0xC6, 0x7C, 0x00],
    // 6 (0x36)
    [0x38, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00],
    // 7 (0x37)
    [0xFE, 0xC6, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00],
    // 8 (0x38)
    [0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00],
    // 9 (0x39)
    [0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x78, 0x00],
    // : (0x3A)
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
    // ; (0x3B)
    [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x30, 0x00],
    // < (0x3C)
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00],
    // = (0x3D)
    [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00],
    // > (0x3E)
    [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00],
    // ? (0x3F)
    [0x7C, 0xC6, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00],
    // @ (0x40)
    [0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x78, 0x00],
    // A (0x41)
    [0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00],
    // B (0x42)
    [0xFC, 0x66, 0x66, 0x7C, 0x66, 0x66, 0xFC, 0x00],
    // C (0x43)
    [0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0x66, 0x3C, 0x00],
    // D (0x44)
    [0xF8, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00],
    // E (0x45)
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x62, 0xFE, 0x00],
    // F (0x46)
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x60, 0xF0, 0x00],
    // G (0x47)
    [0x3C, 0x66, 0xC0, 0xC0, 0xCE, 0x66, 0x3A, 0x00],
    // H (0x48)
    [0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00],
    // I (0x49)
    [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
    // J (0x4A)
    [0x1E, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00],
    // K (0x4B)
    [0xE6, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0xE6, 0x00],
    // L (0x4C)
    [0xF0, 0x60, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00],
    // M (0x4D)
    [0xC6, 0xEE, 0xFE, 0xFE, 0xD6, 0xC6, 0xC6, 0x00],
    // N (0x4E)
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00],
    // O (0x4F)
    [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
    // P (0x50)
    [0xFC, 0x66, 0x66, 0x7C, 0x60, 0x60, 0xF0, 0x00],
    // Q (0x51)
    [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xCE, 0x7C, 0x06],
    // R (0x52)
    [0xFC, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0xE6, 0x00],
    // S (0x53)
    [0x7C, 0xC6, 0x60, 0x38, 0x0C, 0xC6, 0x7C, 0x00],
    // T (0x54)
    [0x7E, 0x7E, 0x5A, 0x18, 0x18, 0x18, 0x3C, 0x00],
    // U (0x55)
    [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
    // V (0x56)
    [0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00],
    // W (0x57)
    [0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0x00],
    // X (0x58)
    [0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00],
    // Y (0x59)
    [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x3C, 0x00],
    // Z (0x5A)
    [0xFE, 0xC6, 0x8C, 0x18, 0x32, 0x66, 0xFE, 0x00],
    // [ (0x5B)
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00],
    // \ (0x5C)
    [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00],
    // ] (0x5D)
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00],
    // ^ (0x5E)
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00],
    // _ (0x5F)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF],
    // ` (0x60)
    [0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00],
    // a (0x61)
    [0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0x76, 0x00],
    // b (0x62)
    [0xE0, 0x60, 0x60, 0x7C, 0x66, 0x66, 0xDC, 0x00],
    // c (0x63)
    [0x00, 0x00, 0x78, 0xCC, 0xC0, 0xCC, 0x78, 0x00],
    // d (0x64)
    [0x1C, 0x0C, 0x0C, 0x7C, 0x6C, 0x6C, 0x3E, 0x00],
    // e (0x65)
    [0x00, 0x00, 0x78, 0xCC, 0xFC, 0xC0, 0x78, 0x00],
    // f (0x66)
    [0x38, 0x6C, 0x60, 0xF0, 0x60, 0x60, 0xF0, 0x00],
    // g (0x67)
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0xF8],
    // h (0x68)
    [0xE0, 0x60, 0x6C, 0x76, 0x66, 0x66, 0xE6, 0x00],
    // i (0x69)
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00],
    // j (0x6A)
    [0x06, 0x00, 0x06, 0x06, 0x06, 0x66, 0x66, 0x3C],
    // k (0x6B)
    [0xE0, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0xE6, 0x00],
    // l (0x6C)
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
    // m (0x6D)
    [0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xD6, 0xD6, 0x00],
    // n (0x6E)
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x00],
    // o (0x6F)
    [0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0x00],
    // p (0x70)
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x7C, 0x60, 0xE0],
    // q (0x71)
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0x1E],
    // r (0x72)
    [0x00, 0x00, 0xDC, 0x76, 0x60, 0x60, 0xF0, 0x00],
    // s (0x73)
    [0x00, 0x00, 0x7E, 0xC0, 0x7C, 0x06, 0xFC, 0x00],
    // t (0x74)
    [0x30, 0x30, 0xFC, 0x30, 0x30, 0x36, 0x1C, 0x00],
    // u (0x75)
    [0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00],
    // v (0x76)
    [0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00],
    // w (0x77)
    [0x00, 0x00, 0xC6, 0xD6, 0xD6, 0xFE, 0x6C, 0x00],
    // x (0x78)
    [0x00, 0x00, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0x00],
    // y (0x79)
    [0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0xFC],
    // z (0x7A)
    [0x00, 0x00, 0x7E, 0x4C, 0x18, 0x32, 0x7E, 0x00],
    // { (0x7B)
    [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00],
    // | (0x7C)
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
    // } (0x7D)
    [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00],
    // ~ (0x7E)
    [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    // DEL (0x7F)
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
];

#[cfg(test)]
mod tests {
    use super::*;

    fn load_font() -> TrueTypeFont {
        TrueTypeFont::parse(include_bytes!("../../../assets/fonts/Roboto.ttf"))
            .expect("Roboto.ttf should parse as a TrueType font")
    }

    #[test]
    fn truetype_parser_emits_real_outline_points_for_roboto_a() {
        let font = load_font();
        let glyph = font.glyph('A').expect("Roboto should contain glyph A");

        assert!(!glyph.contours.is_empty());
        assert!(glyph
            .contours
            .iter()
            .all(|contour| contour.points.len() >= 3));
        let total_points: usize = glyph
            .contours
            .iter()
            .map(|contour| contour.points.len())
            .sum();
        assert!(total_points > glyph.contours.len() * 3);
    }

    #[test]
    fn outline_rasterizer_preserves_holes_and_exterior_space() {
        let font = load_font();
        let glyph = font.glyph('O').expect("Roboto should contain glyph O");
        let mut rasterizer = Rasterizer::new();
        let raster = rasterizer
            .rasterize(&font, glyph, 48.0)
            .expect("contour glyph should rasterize");

        assert!(raster.bitmap.iter().any(|&pixel| pixel == 0));
        assert!(raster.bitmap.iter().any(|&pixel| pixel > 0));
        let opaque_pixels = raster.bitmap.iter().filter(|&&pixel| pixel > 200).count();
        assert!(opaque_pixels > 0);
        assert!(opaque_pixels < raster.bitmap.len());
    }
}
