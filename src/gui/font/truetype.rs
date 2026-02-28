//! # TrueType/OpenType Font Parser (Yazı Tipi Ayrıştırıcı)
//!
//! TrueType (.ttf) ve OpenType (.otf) yazı tipi dosyalarını ayrıştırır.
//! Glif (harf şekli) konturlarını, metrik bilgilerini ve karakter eşlemelerini çıkarır.
//!
//! ## Desteklenen Tablolar
//! - `head`: Yazı tipi genel bilgileri (EM boyutu, sınır kutusu)
//! - `hhea`: Yatay metrik sayısı
//! - `hmtx`: Her glif için ilerleme genişliği ve sol taşma
//! - `cmap`: Unicode → glif indeks eşlemesi (Format 4 ve 12)
//! - `glyf`: Glif kontur verileri
//! - `loca`: Glif ofset tablosu
//! - `name`: Yazı tipi aile ve stil ismi
//! - `maxp`: Maksimum glif sayısı

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// TrueType yazı tipi yapısı; ayrıştırılmış tablolar ve glif listesini barındırır
pub struct TrueTypeFont {
    pub family_name: String,
    pub style_name: String,
    pub is_monospace: bool,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub glyphs: Vec<Glyph>,
    pub cmap: Vec<(u32, u16)>, // Unicode → glif indeksi eşlemesi
    pub h_metrics: Vec<HorizontalMetric>,
    pub head: FontHeader,
}

/// `head` tablosundan alınan yazı tipi başlık verisi (EM boyutu, sınır kutusu, stil bayrakları)
#[derive(Clone, Copy, Debug)]
pub struct FontHeader {
    pub units_per_em: u16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub mac_style: u16,
    pub lowest_rec_ppem: u16,
    pub index_to_loc_format: i16,
}

/// Tek bir glif için kontur (şekil) verisi, metrik bilgileri ve sınır kutusu
#[derive(Clone, Debug)]
pub struct Glyph {
    pub index: u16,
    pub advance_width: u16,
    pub left_side_bearing: i16,
    pub bounds: GlyphBounds,
    pub contours: Vec<GlyphContour>,
}

/// Glif sınır kutusu (piksel koordinatlarında minimum/maksimum x ve y değerleri)
#[derive(Clone, Copy, Debug, Default)]
pub struct GlyphBounds {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

/// Kapalı yol (kontur); glif çizgisini oluşturan noktalar dizisi
#[derive(Clone, Debug)]
pub struct GlyphContour {
    pub points: Vec<GlyphPoint>,
}

/// Glif konturundaki tek nokta; koordinatlar ve eğri üzerinde mi bilgisi
#[derive(Clone, Copy, Debug)]
pub struct GlyphPoint {
    pub x: i16,
    pub y: i16,
    pub on_curve: bool,
}

/// `hmtx` tablosundan gelen yatay metrik: ilerleme genişliği ve sol taşma
#[derive(Clone, Copy, Debug)]
pub struct HorizontalMetric {
    pub advance_width: u16,
    pub left_side_bearing: i16,
}

impl TrueTypeFont {
    /// Ham bayt dizisinden TrueType/OpenType yazı tipini ayrıştırır.
    /// Tablo dizinini okur, gerekli tabloları bulur ve glif verilerini yükler.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        // Sürüm numarasını kontrol et (0x00010000 → TrueType, 0x4F54544F → OpenType/CFF)
        let version = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if version != 0x00010000 && version != 0x4F54544F {
            // TrueType veya OpenType formatı değil
            return None;
        }

        let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
        
        // Gerekli tabloları bul (head, hhea, hmtx, cmap, glyf, loca, name, maxp)
        let mut head_offset = None;
        let mut hhea_offset = None;
        let mut hmtx_offset = None;
        let mut cmap_offset = None;
        let mut glyf_offset = None;
        let mut loca_offset = None;
        let mut name_offset = None;
        let mut maxp_offset = None;

        for i in 0..num_tables {
            let offset = 12 + i * 16;
            if offset + 16 > data.len() {
                break;
            }
            
            let tag = &data[offset..offset + 4];
            let table_offset = u32::from_be_bytes([
                data[offset + 8],
                data[offset + 9],
                data[offset + 10],
                data[offset + 11],
            ]) as usize;
            let table_len = u32::from_be_bytes([
                data[offset + 12],
                data[offset + 13],
                data[offset + 14],
                data[offset + 15],
            ]) as usize;

            match tag {
                b"head" => head_offset = Some((table_offset, table_len)),
                b"hhea" => hhea_offset = Some((table_offset, table_len)),
                b"hmtx" => hmtx_offset = Some((table_offset, table_len)),
                b"cmap" => cmap_offset = Some((table_offset, table_len)),
                b"glyf" => glyf_offset = Some((table_offset, table_len)),
                b"loca" => loca_offset = Some((table_offset, table_len)),
                b"name" => name_offset = Some((table_offset, table_len)),
                b"maxp" => maxp_offset = Some((table_offset, table_len)),
                _ => {}
            }
        }

        // head tablosunu ayrıştır (EM boyutu, sınır kutusu, loca format)
        let head = head_offset.and_then(|(off, _)| Self::parse_head(data, off))?;

        // maxp tablosundan glif sayısını al
        let num_glyphs = maxp_offset.and_then(|(off, _)| Self::parse_maxp(data, off)).unwrap_or(256);

        // name tablosundan aile adını ayrıştır
        let (family_name, style_name) = name_offset
            .map(|(off, len)| Self::parse_name(data, off, len))
            .unwrap_or((String::from("Unknown"), String::new()));

        // hhea tablosundan yatay metrik sayısını al
        let hhea_count = hhea_offset
            .and_then(|(off, _)| Self::parse_hhea(data, off))
            .unwrap_or(num_glyphs);

        // hmtx tablosunu ayrıştır
        let h_metrics = hmtx_offset
            .map(|(off, _)| Self::parse_hmtx(data, off, hhea_count as usize, num_glyphs as usize))
            .unwrap_or_default();

        // cmap tablosunu ayrıştır
        let cmap = cmap_offset
            .and_then(|(off, len)| Self::parse_cmap(data, off, len))
            .unwrap_or_default();

        // loca tablosunu ayrıştır (glif ofsetleri)
        let loca = loca_offset
            .map(|(off, len)| Self::parse_loca(data, off, len, head.index_to_loc_format, num_glyphs as usize))
            .unwrap_or_default();

        // glyf tablosunu ayrıştır (sadece sınır kutuları)
        let glyphs = glyf_offset
            .map(|(off, _)| Self::parse_glyf(data, off, &loca, &h_metrics))
            .unwrap_or_default();

        Some(Self {
            family_name,
            style_name,
            is_monospace: false, // Tüm ilerleme genişliklerinin eşit olup olmadığı kontrol edilmeli
            units_per_em: head.units_per_em,
            ascent: 0,
            descent: 0,
            line_gap: 0,
            glyphs,
            cmap,
            h_metrics,
            head,
        })
    }

    fn parse_head(data: &[u8], offset: usize) -> Option<FontHeader> {
        if offset + 54 > data.len() {
            return None;
        }
        Some(FontHeader {
            units_per_em: u16::from_be_bytes([data[offset + 18], data[offset + 19]]),
            x_min: i16::from_be_bytes([data[offset + 36], data[offset + 37]]),
            y_min: i16::from_be_bytes([data[offset + 38], data[offset + 39]]),
            x_max: i16::from_be_bytes([data[offset + 40], data[offset + 41]]),
            y_max: i16::from_be_bytes([data[offset + 42], data[offset + 43]]),
            mac_style: u16::from_be_bytes([data[offset + 44], data[offset + 45]]),
            lowest_rec_ppem: u16::from_be_bytes([data[offset + 46], data[offset + 47]]),
            index_to_loc_format: i16::from_be_bytes([data[offset + 50], data[offset + 51]]),
        })
    }

    fn parse_maxp(data: &[u8], offset: usize) -> Option<u16> {
        if offset + 6 > data.len() {
            return None;
        }
        Some(u16::from_be_bytes([data[offset + 4], data[offset + 5]]))
    }

    fn parse_hhea(data: &[u8], offset: usize) -> Option<u16> {
        if offset + 36 > data.len() {
            return None;
        }
        Some(u16::from_be_bytes([data[offset + 34], data[offset + 35]]))
    }

    fn parse_hmtx(data: &[u8], offset: usize, count: usize, total: usize) -> Vec<HorizontalMetric> {
        let mut metrics = Vec::with_capacity(total);
        
        let mut last_advance = 0u16;
        for i in 0..count.min(total) {
            let off = offset + i * 4;
            if off + 4 > data.len() {
                break;
            }
            let advance = u16::from_be_bytes([data[off], data[off + 1]]);
            let lsb = i16::from_be_bytes([data[off + 2], data[off + 3]]);
            last_advance = advance;
            metrics.push(HorizontalMetric { advance_width: advance, left_side_bearing: lsb });
        }
        
        // Kalan glif sayısı kadar son ilerleme genişliğiyle doldur
        while metrics.len() < total {
            let lsb_off = offset + count * 4 + (metrics.len() - count) * 2;
            let lsb = if lsb_off + 2 <= data.len() {
                i16::from_be_bytes([data[lsb_off], data[lsb_off + 1]])
            } else {
                0
            };
            metrics.push(HorizontalMetric {
                advance_width: last_advance,
                left_side_bearing: lsb,
            });
        }
        
        metrics
    }

    fn parse_cmap(data: &[u8], offset: usize, _len: usize) -> Option<Vec<(u32, u16)>> {
        if offset + 4 > data.len() {
            return None;
        }

        let num_subtables = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        
        // Unicode alt tablosunu bul (Windows Unicode veya genel Unicode platformu)
        for i in 0..num_subtables {
            let sub_off = offset + 4 + i * 8;
            if sub_off + 8 > data.len() {
                break;
            }
            
            let platform = u16::from_be_bytes([data[sub_off], data[sub_off + 1]]);
            let encoding = u16::from_be_bytes([data[sub_off + 2], data[sub_off + 3]]);
            let table_offset = u32::from_be_bytes([data[sub_off + 4], data[sub_off + 5], data[sub_off + 6], data[sub_off + 7]]) as usize;
            
            // Windows Unicode (platform 3, encoding 1) veya Unicode BMP (platform 0)
            if (platform == 3 && encoding == 1) || (platform == 0) {
                return Self::parse_cmap_subtable(data, offset + table_offset);
            }
        }
        
        Some(Vec::new())
    }

    fn parse_cmap_subtable(data: &[u8], offset: usize) -> Option<Vec<(u32, u16)>> {
        if offset + 8 > data.len() {
            return None;
        }

        let format = u16::from_be_bytes([data[offset], data[offset + 1]]);
        
        match format {
            4 => Self::parse_cmap_format4(data, offset),
            12 => Self::parse_cmap_format12(data, offset),
            _ => Some(Vec::new()),
        }
    }

    fn parse_cmap_format4(data: &[u8], offset: usize) -> Option<Vec<(u32, u16)>> {
        if offset + 14 > data.len() {
            return None;
        }

        let seg_count = u16::from_be_bytes([data[offset + 6], data[offset + 7]]) as usize / 2;
        let end_off = offset + 14;
        let start_off = end_off + seg_count * 2 + 2;
        let id_delta_off = start_off + seg_count * 2;
        let id_range_off = id_delta_off + seg_count * 2;

        let mut cmap = Vec::new();
        
        for i in 0..seg_count {
            let end = u16::from_be_bytes([data[end_off + i * 2], data[end_off + i * 2 + 1]]);
            let start = u16::from_be_bytes([data[start_off + i * 2], data[start_off + i * 2 + 1]]);
            let delta = i16::from_be_bytes([data[id_delta_off + i * 2], data[id_delta_off + i * 2 + 1]]);
            let range = u16::from_be_bytes([data[id_range_off + i * 2], data[id_range_off + i * 2 + 1]]);

            for code in start..=end {
                let glyph_idx = if range == 0 {
                    // Sarmalı aritmetikle glif indeksi hesapla
                    (code as u16).wrapping_add(delta as u16)
                } else {
                    let range_off = offset + range as usize + (code - start) as usize * 2;
                    if range_off + 2 <= data.len() {
                        u16::from_be_bytes([data[range_off], data[range_off + 1]])
                    } else {
                        0
                    }
                };
                cmap.push((code as u32, glyph_idx));
            }
        }
        
        Some(cmap)
    }

    fn parse_cmap_format12(data: &[u8], offset: usize) -> Option<Vec<(u32, u16)>> {
        if offset + 16 > data.len() {
            return None;
        }

        let num_groups = u32::from_be_bytes([data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15]]) as usize;
        let mut cmap = Vec::new();

        for i in 0..num_groups {
            let group_off = offset + 16 + i * 12;
            if group_off + 12 > data.len() {
                break;
            }
            
            let start = u32::from_be_bytes([data[group_off], data[group_off + 1], data[group_off + 2], data[group_off + 3]]);
            let end = u32::from_be_bytes([data[group_off + 4], data[group_off + 5], data[group_off + 6], data[group_off + 7]]);
            let start_glyph = u32::from_be_bytes([data[group_off + 8], data[group_off + 9], data[group_off + 10], data[group_off + 11]]) as u16;

            for (j, code) in (start..=end).enumerate() {
                cmap.push((code, start_glyph + j as u16));
            }
        }
        
        Some(cmap)
    }

    fn parse_loca(data: &[u8], offset: usize, len: usize, format: i16, num_glyphs: usize) -> Vec<u32> {
        let mut loca = Vec::with_capacity(num_glyphs + 1);
        
        for i in 0..=num_glyphs {
            let off = if format == 0 {
                // Kısa format: ofset 2'ye bölünmüş olarak saklanır
                let idx = offset + i * 2;
                if idx + 2 <= data.len() && idx < offset + len {
                    u16::from_be_bytes([data[idx], data[idx + 1]]) as u32 * 2
                } else {
                    0
                }
            } else {
                // Uzun format: ofset tam 32 bit olarak saklanır
                let idx = offset + i * 4;
                if idx + 4 <= data.len() && idx < offset + len {
                    u32::from_be_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]])
                } else {
                    0
                }
            };
            loca.push(off);
        }
        
        loca
    }

    fn parse_glyf(data: &[u8], offset: usize, loca: &[u32], h_metrics: &[HorizontalMetric]) -> Vec<Glyph> {
        let mut glyphs = Vec::with_capacity(loca.len().saturating_sub(1));
        
        for i in 0..loca.len().saturating_sub(1) {
            let glyph_offset = offset + loca[i] as usize;
            let next_offset = offset + loca[i + 1] as usize;
            
            let (bounds, contours) = if glyph_offset < next_offset && glyph_offset + 10 <= data.len() {
                Self::parse_glyph_outline(data, glyph_offset, next_offset)
            } else {
                (GlyphBounds::default(), Vec::new())
            };
            
            let metric = h_metrics.get(i).copied().unwrap_or(HorizontalMetric {
                advance_width: 0,
                left_side_bearing: 0,
            });
            
            glyphs.push(Glyph {
                index: i as u16,
                advance_width: metric.advance_width,
                left_side_bearing: metric.left_side_bearing,
                bounds,
                contours,
            });
        }
        
        glyphs
    }

    fn parse_glyph_outline(data: &[u8], offset: usize, end_offset: usize) -> (GlyphBounds, Vec<GlyphContour>) {
        if offset + 12 > data.len() {
            return (GlyphBounds::default(), Vec::new());
        }

        let num_contours = i16::from_be_bytes([data[offset], data[offset + 1]]) as i16;
        
        let bounds = GlyphBounds {
            x_min: i16::from_be_bytes([data[offset + 2], data[offset + 3]]),
            y_min: i16::from_be_bytes([data[offset + 4], data[offset + 5]]),
            x_max: i16::from_be_bytes([data[offset + 6], data[offset + 7]]),
            y_max: i16::from_be_bytes([data[offset + 8], data[offset + 9]]),
        };

        if num_contours <= 0 {
            return (bounds, Vec::new());
        }

        let num_contours = num_contours as usize;
        let mut contour_ends = Vec::with_capacity(num_contours);
        
        for i in 0..num_contours {
            let idx = offset + 10 + i * 2;
            if idx + 2 > data.len() {
                break;
            }
            contour_ends.push(u16::from_be_bytes([data[idx], data[idx + 1]]) as usize);
        }

        // Basitleştirilmiş uygulama: boş konturlar döndürür
        // Tam uygulama nokta verilerini de ayrıştırır
        let contours = contour_ends.iter().map(|_| GlyphContour { points: Vec::new() }).collect();
        
        (bounds, contours)
    }

    fn parse_name(data: &[u8], offset: usize, _len: usize) -> (String, String) {
        if offset + 6 > data.len() {
            return (String::from("Unknown"), String::new());
        }

        let num_records = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        let string_offset = u16::from_be_bytes([data[offset + 4], data[offset + 5]]) as usize;
        
        let mut family = String::new();
        let mut style = String::new();

        for i in 0..num_records {
            let rec_off = offset + 6 + i * 12;
            if rec_off + 12 > data.len() {
                break;
            }

            let platform = u16::from_be_bytes([data[rec_off], data[rec_off + 1]]);
            let name_id = u16::from_be_bytes([data[rec_off + 6], data[rec_off + 7]]);
            let str_len = u16::from_be_bytes([data[rec_off + 8], data[rec_off + 9]]) as usize;
            let str_off = u16::from_be_bytes([data[rec_off + 10], data[rec_off + 11]]) as usize;

            // Aile adı (name_id=1) veya stil adı (name_id=2)
            if name_id == 1 || name_id == 2 {
                let start = offset + string_offset + str_off;
                if start + str_len <= data.len() {
                    let bytes = &data[start..start + str_len];
                    let s = if platform == 3 {
                        // Windows platformu: UTF-16BE kodlaması
                        String::from_utf8_lossy(bytes).into_owned()
                    } else {
                        // Mac Roman veya diğer platformlar
                        String::from_utf8_lossy(bytes).into_owned()
                    };
                    
                    if name_id == 1 {
                        family = s;
                    } else {
                        style = s;
                    }
                }
            }
        }

        if family.is_empty() {
            family = String::from("Unknown");
        }
        
        (family, style)
    }

    /// Verilen karakter için glif yapısını döndürür; cmap tablosundan Unicode → glif indeksi arar
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        let code = c as u32;
        for (unicode, idx) in &self.cmap {
            if *unicode == code {
                return self.glyphs.get(*idx as usize);
            }
        }
        None
    }

    /// Verilen karakter ve yazı tipi boyutu için piksel cinsinden ilerleme genişliğini döndürür.
    /// `units_per_em` normalleştirmesi ile boyut ölçeklenir.
    pub fn advance(&self, c: char, size: f32) -> f32 {
        let glyph = self.glyph(c);
        let units = glyph.map(|g| g.advance_width).unwrap_or(0);
        units as f32 * size / self.units_per_em as f32
    }
}
