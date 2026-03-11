//! # Font Rasterleyici (Rasterizer)
//!
//! Glyph (harf şekli) dış hat vektörlerini piksel bitmap'lerine dönüştürür.
//!
//! ## Rasterizasyon Nedir?
//! Vektör tabanlı font verileri (bezier eğrileri, doğru parçaları) matematiksel
//! formüllere dayanır; ekrana çizmek için bu formülleri piksel ızgarasına
//! dönüştürmek gerekir. Bu işleme "rasterizasyon" denir.
//!
//! ## Temel Kavramlar
//! - **Glyph**: Tek bir karakterin görsel temsili (örn. 'A' harfinin şekli)
//! - **Contour**: Bir glyphin kapalı yolunu oluşturan nokta dizisi
//! - **Scanline algoritması**: Her yatay tarama satırı için yol kesişimlerini
//!   bulur ve doldurulacak pikselleri belirler
//! - **Winding number**: Bir noktanın contour içinde mi dışında mı olduğunu
//!   belirleyen sayı; imzalı kesişim sayısıyla hesaplanır
//! - **Anti-aliasing**: Kenar piksellerine kısmi saydamlık değeri atanarak
//!   kademeli geçiş oluşturulması ve pürüzlü görünümün azaltılması
//! - **Advance width**: Bir karakterden sonra bir sonrakini yerleştirmek için
//!   ilerlenecek yatay piksel mesafesi

use super::truetype::{Glyph, GlyphContour, GlyphPoint, TrueTypeFont};
use alloc::vec;
use alloc::vec::Vec;
use libm::powf;

/// no_std ortamı için özel tavan (ceiling) fonksiyonu.
/// Standart kütüphanede `f32::ceil()` bulunur; ancak çekirdek modunda
/// kayan nokta runtime desteği olmadığından cast hilesine başvuruyoruz:
/// tamsayıya dönüşüm otomatik olarak `truncate` (sıfıra yuvarlama)
/// yapar; pozitif sayılar için truncate == floor olduğundan bir ekleme
/// ile tavan elde edilir.
fn ceil_f32(x: f32) -> f32 {
    let i = x as i32 as f32;
    if x > i {
        i + 1.0
    } else {
        i
    }
}

/// no_std ortamı için özel taban (floor) fonksiyonu.
/// `ceil_f32` ile aynı mantık; negatif sayılarda truncate değeri
/// matematiksel tabandan büyük olduğundan bunu düzeltmek için
/// bir çıkarma yapılır.
fn floor_f32(x: f32) -> f32 {
    let i = x as i32 as f32;
    if x < i {
        i - 1.0
    } else {
        i
    }
}

/// Rasterize edilmiş (pikselleştirilmiş) glyph bitmap'i.
///
/// Vektörden piksel ızgarasına dönüştürülmüş harf verisi.
/// `bitmap` alanı, her piksel için 0–255 arası alfa (saydamlık)
/// değerleri içerir: 0 = tam saydam, 255 = tam opak.
#[derive(Clone, Debug)]
pub struct RasterGlyph {
    pub width: usize,
    pub height: usize,
    pub offset_x: i32,
    pub offset_y: i32,
    pub advance: f32,
    pub bitmap: Vec<u8>, // Her piksel için alfa değeri (0-255)
}

/// Rasterizasyon metrik verileri.
///
/// Bitmap'in framebuffer üzerindeki konumunu belirlemek için kullanılır:
/// `offset_x`/`offset_y` glyphin sol altından ölçülen imzero-point ofsetidir.
#[derive(Clone, Copy, Debug)]
pub struct RasterMetrics {
    pub width: usize,
    pub height: usize,
    pub offset_x: i32,
    pub offset_y: i32,
}

/// Glyph rasterleyici ana yapısı.
///
/// Dahili tamponları yeniden kullanarak her rasterizasyon için
/// heap ayırma maliyetini düşürür. `scanline` tamponu bir satırdaki
/// her sütun için kaplama (coverage) değerini tutar;
/// `winding` tamponu ise o noktanın contour içinde olup olmadığını
/// belirlemek için imzalı kesişim sayısını depolar.
pub struct Rasterizer {
    // Tarama satırı işleme için dahili tampon; her çağrıda temizlenir
    scanline: Vec<f32>,
    // Winding sayı tamponu; pozitif = sola döndürme, negatif = sağa döndürme
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

    /// Belirli bir boyutta glyph'i rasterize eder (vektörden piksele dönüştürür).
    ///
    /// `size` noktasal font büyüklüğüne karşılık gelir (örn. 16.0 = 16pt).
    /// `scale = size / units_per_em` formülüyle font koordinat birimlerinden
    /// piksel koordinatlarına dönüşüm katsayısı hesaplanır.
    /// Sıfır boyutlu glyphler (boşluk karakteri gibi) için boş bitmap döndürülür.
    pub fn rasterize(
        &mut self,
        font: &TrueTypeFont,
        glyph: &Glyph,
        size: f32,
    ) -> Option<RasterGlyph> {
        let scale = size / font.units_per_em as f32;

        // Bitmap boyutunu hesapla: glyphin sınırlayıcı kutusunu ölçeklendirip
        // tavan alma yoluyla tam piksel boyutuna yuvarlıyoruz
        let bounds = &glyph.bounds;
        let width = ceil_f32((bounds.x_max - bounds.x_min) as f32 * scale) as usize;
        let height = ceil_f32((bounds.y_max - bounds.y_min) as f32 * scale) as usize;

        if width == 0 || height == 0 {
            return Some(RasterGlyph {
                width: 0,
                height: 0,
                offset_x: 0,
                offset_y: 0,
                advance: glyph.advance_width as f32 * scale,
                bitmap: Vec::new(),
            });
        }

        let offset_x = floor_f32(glyph.left_side_bearing as f32 * scale) as i32;
        let offset_y = floor_f32(bounds.y_max as f32 * scale) as i32;

        let mut bitmap = vec![0u8; width * height];

        // Gerçek outline rendering yerine şimdilik sade dikdörtgen çizimi;
        // ileride bezier eğrileriyle tam vektör rasterizasyonu yapılacak
        self.render_simple(&mut bitmap, width, height, glyph, scale);

        if self.subpixel_aa {
            self.apply_subpixel_gamma(&mut bitmap, width, height);
        }

        Some(RasterGlyph {
            width,
            height,
            offset_x,
            offset_y,
            advance: glyph.advance_width as f32 * scale,
            bitmap,
        })
    }

    /// Sade dikdörtgen dolgu ile basit rasterizasyon (tam outline rendering için yer tutucu).
    ///
    /// Gerçek bir FontRenderer bezier eğrilerini örnekleyerek her piksel için
    /// kaplama değeri hesaplar. Bu basit sürüm tüm glyphi dolu bir kutu olarak
    /// çizer; kenar piksellerine 128 (yarı saydam) değeri atanarak ham bir
    /// anti-aliasing taklidi yapılır.
    fn render_simple(
        &mut self,
        bitmap: &mut [u8],
        width: usize,
        height: usize,
        glyph: &Glyph,
        scale: f32,
    ) {
        let bounds = &glyph.bounds;

        // Sınırlayıcı kutu değişkenleri; ileride bezier örnekleme için kullanılacak
        let x_start = 0usize;
        let x_end = width;
        let y_start = 0usize;
        let y_end = height;

        // Tüm pikselleri tam opak (255) ile doldur
        for y in y_start..y_end {
            for x in x_start..x_end {
                let idx = y * width + x;
                if idx < bitmap.len() {
                    // Gelecekte buraya anti-aliasing kaplama değeri gelecek
                    bitmap[idx] = 255;
                }
            }
        }

        // Kenar pikselleri için basit anti-aliasing: 128 = %50 saydamlık
        // Gerçek AA, piksel merkezinin eğriye olan mesafesine göre hesaplanır
        if width > 2 && height > 2 {
            for x in 0..width {
                // Üst kenar
                bitmap[x] = 128;
                // Alt kenar
                bitmap[(height - 1) * width + x] = 128;
            }
            for y in 0..height {
                // Sol kenar
                bitmap[y * width] = 128;
                // Sağ kenar
                bitmap[y * width + width - 1] = 128;
            }
        }
    }

    /// Scanline algoritmasıyla glyph dış hatlarını rasterize eder.
    ///
    /// Klasik dolu-çokgen rasterizasyon yöntemi:
    /// 1. Her yatay tarama satırı (scanline) için tüm contour kenarlarını dolaş.
    /// 2. Kenarın o satırla kesişip kesişmediğini kontrol et.
    /// 3. Kesişim noktasındaki winding sayısını güncelle.
    /// 4. Winding ≠ 0 olan sütunlar çizgi içindedir → pikseli doldur.
    /// Bu yöntem TrueType'ın "non-zero winding" doldurma kuralına uygundur.
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

    #[allow(dead_code)]
    fn render_outline(
        &mut self,
        bitmap: &mut [u8],
        width: usize,
        height: usize,
        contours: &[GlyphContour],
        scale: f32,
    ) {
        // Tarama satırı tamponu büyüklüğünü güvence altına al
        if self.scanline.len() < width {
            self.scanline.resize(width, 0.0);
            self.winding.resize(width, 0);
        }

        // Her tarama satırını işle
        for y in 0..height {
            // Tamponu temizle: bir önceki satırdan kalan veriler silinir
            for i in 0..width {
                self.scanline[i] = 0.0;
                self.winding[i] = 0;
            }

            // Font koordinat sistemindeki y değeri (ölçeklenmiş gerçek koordinat)
            let scan_y = y as f32 / scale;
            for contour in contours {
                self.process_contour(&contour.points, scan_y, scale);
            }

            // Winding sıfırdan farklıyken içi doldur; değişim noktaları kenar pikseli
            let mut inside = false;
            for x in 0..width {
                if self.winding[x] != 0 {
                    inside = !inside;
                }
                if inside {
                    bitmap[y * width + x] = 255;
                }
            }
        }
    }

    #[allow(dead_code)]
    fn process_contour(&mut self, points: &[GlyphPoint], scan_y: f32, scale: f32) {
        if points.len() < 2 {
            return;
        }

        let n = points.len();
        for i in 0..n {
            let p0 = &points[i];
            let p1 = &points[(i + 1) % n]; // Kapalı yol: son nokta ilk noktaya bağlanır

            // Kenarın tarama satırını kesip kesmediğini kontrol et:
            // Bir kenar ancak bir ucu satırın üstünde, diğeri altındaysa keser
            let y0 = p0.y as f32;
            let y1 = p1.y as f32;

            if (y0 <= scan_y && y1 > scan_y) || (y1 <= scan_y && y0 > scan_y) {
                // Lineer interpolasyonla kesişim x koordinatı: x = x0 + t*(x1-x0)
                let t = (scan_y - y0) / (y1 - y0);
                let x = (p0.x as f32 + t * (p1.x as f32 - p0.x as f32)) * scale;
                let x_idx = x as usize;

                if x_idx < self.winding.len() {
                    // Non-zero winding kuralı: yukarı gide → +1, aşağı gide → -1
                    if y0 < y1 {
                        self.winding[x_idx] += 1;
                    } else {
                        self.winding[x_idx] -= 1;
                    }
                }
            }
        }
    }

    /// 8×8 piksel bitmap font karakterini rasterize eder.
    ///
    /// Bitmap fontlar, her karakteri 8 baytlık bir dizi ile temsil eder:
    /// her bayt bir satırı, her bit ise o satırdaki bir pikseli gösterir.
    /// `0x80 >> col` maskesi ile sütun biti test edilir; `scale` faktörü
    /// ile büyütme yapılarak her bit birden fazla piksel kapsar.
    /// Bu yöntem TrueType ayrıştırma başarısız olduğunda geri dönüş olarak kullanılır.
    pub fn rasterize_bitmap(&self, char_bits: &[u8; 8], size: f32) -> RasterGlyph {
        let scale = (size / 8.0).max(1.0) as usize;
        let width = 8 * scale;
        let height = 8 * scale;

        let mut bitmap = vec![0u8; width * height];

        for (row, &bits) in char_bits.iter().enumerate() {
            for col in 0..8 {
                if bits & (0x80 >> col) != 0 {
                    // Ölçeklenmiş piksel bloğunu doldur:
                    // scale=2 ise her bit 2×2 piksel bloğuna genişler
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

/// Yerleşik 8×8 piksel bitmap font verisi (96 yazdırılabilir ASCII karakter).
///
/// ASCII tablosunda 0x20 (boşluk) ile 0x7F (DEL) arasındaki karakterleri içerir.
/// Her karakter 8 baytla temsil edilir: bir bayt = bir yatay satır,
/// bir bit = bir piksel (1=dolu, 0=boş). En yüksek değerlikli bit (MSB) solda.
///
/// Örnek: 'A' (0x41)
/// ```text
/// 0x38 = 00111000  →   ***
/// 0x6C = 01101100  →  ** **
/// 0xC6 = 11000110  → **   **
/// 0xFE = 11111110  → *******
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
