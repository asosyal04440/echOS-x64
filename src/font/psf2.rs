//! # PSF2 Font Loader
//!
//! PC Screen Font Version 2 (PSF2) bitmap font formatını yükler.
//! Linux konsolları ve embedded sistemlerde yaygın kullanılır.
//!
//! ## PSF2 Format Yapısı
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │ Header (32 bytes)                                   │
//! │ ├─ magic: [u8; 4] = [0x72, 0xb5, 0x4a, 0x86]       │
//! │ ├─ version: u32 = 0                                 │
//! │ ├─ headersize: u32 = 32                             │
//! │ ├─ flags: u32 (0x01 = has unicode table)           │
//! │ ├─ numglyph: u32                                    │
//! │ ├─ bytesperglyph: u32                               │
//! │ ├─ height: u32                                      │
//! │ └─ width: u32                                       │
//! ├─────────────────────────────────────────────────────┤
//! │ Glyph Data (numglyph × bytesperglyph bytes)         │
//! │ ├─ Glyph 0: [u8; bytesperglyph]                    │
//! │ ├─ Glyph 1: [u8; bytesperglyph]                    │
//! │ └─ ...                                              │
//! ├─────────────────────────────────────────────────────┤
//! │ Unicode Table (optional, if flags & 0x01)           │
//! │ ├─ Glyph 0 mappings: unicode* 0xFF                 │
//! │ ├─ Glyph 1 mappings: unicode* 0xFF                 │
//! │ └─ ...                                              │
//! └─────────────────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// PSF2 magic bytes
const PSF2_MAGIC: [u8; 4] = [0x72, 0xb5, 0x4a, 0x86];

/// PSF2 flag: Unicode table present
const PSF2_HAS_UNICODE_TABLE: u32 = 0x01;

/// PSF2 font header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Psf2Header {
    pub magic: [u8; 4],
    pub version: u32,
    pub headersize: u32,
    pub flags: u32,
    pub numglyph: u32,
    pub bytesperglyph: u32,
    pub height: u32,
    pub width: u32,
}

impl Psf2Header {
    /// PSF2 başlığını doğrula
    pub fn is_valid(&self) -> bool {
        self.magic == PSF2_MAGIC && self.version == 0 && self.headersize >= 32
    }

    /// Unicode tablosu var mı?
    pub fn has_unicode_table(&self) -> bool {
        (self.flags & PSF2_HAS_UNICODE_TABLE) != 0
    }
}

/// PSF2 font
#[derive(Debug, Clone)]
pub struct Psf2Font {
    pub header: Psf2Header,
    /// Glyph bitmap verileri
    pub glyph_data: Vec<u8>,
    /// Unicode → glyph index mapping (optional)
    pub unicode_map: BTreeMap<u32, u32>,
}

impl Psf2Font {
    /// PSF2 dosyasından font yükle
    ///
    /// # Errors
    /// - Magic/version hatalıysa None döner
    /// - Veri yetersizse None döner
    pub fn load(data: &[u8]) -> Option<Self> {
        if data.len() < 32 {
            return None;
        }

        // Header'ı oku
        let header = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Psf2Header) };

        if !header.is_valid() {
            return None;
        }

        let glyph_data_size = (header.numglyph * header.bytesperglyph) as usize;
        let glyph_data_start = header.headersize as usize;
        let glyph_data_end = glyph_data_start + glyph_data_size;

        if data.len() < glyph_data_end {
            return None;
        }

        let glyph_data = data[glyph_data_start..glyph_data_end].to_vec();

        // Unicode tablosunu parse et (varsa)
        let mut unicode_map = BTreeMap::new();

        if header.has_unicode_table() && data.len() > glyph_data_end {
            let unicode_table = &data[glyph_data_end..];
            parse_unicode_table(unicode_table, header.numglyph, &mut unicode_map);
        } else {
            // Unicode tablosu yoksa, ilk 256 glyph = ASCII/Latin-1 varsay
            for i in 0..header.numglyph.min(256) {
                unicode_map.insert(i, i);
            }
        }

        Some(Psf2Font {
            header,
            glyph_data,
            unicode_map,
        })
    }

    /// Unicode code point için glyph index'i bul
    pub fn glyph_index(&self, codepoint: u32) -> Option<u32> {
        self.unicode_map.get(&codepoint).copied()
    }

    /// Glyph index için bitmap verisini al
    pub fn glyph_bitmap(&self, index: u32) -> Option<&[u8]> {
        if index >= self.header.numglyph {
            return None;
        }
        let offset = (index * self.header.bytesperglyph) as usize;
        let end = offset + self.header.bytesperglyph as usize;
        Some(&self.glyph_data[offset..end])
    }

    /// Unicode codepoint için bitmap verisini al
    pub fn bitmap_for_char(&self, codepoint: u32) -> Option<&[u8]> {
        let index = self.glyph_index(codepoint)?;
        self.glyph_bitmap(index)
    }

    /// Font genişliği (piksel)
    pub fn width(&self) -> u32 {
        self.header.width
    }

    /// Font yüksekliği (piksel)
    pub fn height(&self) -> u32 {
        self.header.height
    }

    /// Bytes per row (her satırdaki byte sayısı)
    pub fn bytes_per_row(&self) -> u32 {
        (self.header.width + 7) / 8
    }
}

/// Unicode tablosunu parse et
/// Format: her glyph için unicode değerler + 0xFF terminator
fn parse_unicode_table(data: &[u8], numglyph: u32, map: &mut BTreeMap<u32, u32>) {
    let mut offset = 0;
    let mut glyph_idx = 0u32;

    while glyph_idx < numglyph && offset < data.len() {
        // Her glyph için unicode değerleri oku
        loop {
            if offset >= data.len() {
                break;
            }

            let byte = data[offset];

            // 0xFF = terminator
            if byte == 0xFF {
                offset += 1;
                break;
            }

            // 0xFE = sequence start (PSF2 extension, skip)
            if byte == 0xFE {
                // Sequence'ı atla (0xFF'e kadar)
                while offset < data.len() && data[offset] != 0xFF {
                    offset += 1;
                }
                continue;
            }

            // UTF-8 decode
            let (codepoint, bytes_read) = utf8_decode(&data[offset..]);
            if let Some(cp) = codepoint {
                map.insert(cp, glyph_idx);
            }
            offset += bytes_read;
        }

        glyph_idx += 1;
    }
}

/// UTF-8 byte dizisinden tek karakter decode et
fn utf8_decode(data: &[u8]) -> (Option<u32>, usize) {
    if data.is_empty() {
        return (None, 0);
    }

    let first = data[0];

    // ASCII (0xxxxxxx)
    if first < 0x80 {
        return (Some(first as u32), 1);
    }

    // 2-byte (110xxxxx 10xxxxxx)
    if first >= 0xC0 && first < 0xE0 && data.len() >= 2 {
        let cp = ((first as u32 & 0x1F) << 6) | (data[1] as u32 & 0x3F);
        return (Some(cp), 2);
    }

    // 3-byte (1110xxxx 10xxxxxx 10xxxxxx)
    if first >= 0xE0 && first < 0xF0 && data.len() >= 3 {
        let cp = ((first as u32 & 0x0F) << 12)
            | ((data[1] as u32 & 0x3F) << 6)
            | (data[2] as u32 & 0x3F);
        return (Some(cp), 3);
    }

    // 4-byte (11110xxx 10xxxxxx 10xxxxxx 10xxxxxx)
    if first >= 0xF0 && first < 0xF8 && data.len() >= 4 {
        let cp = ((first as u32 & 0x07) << 18)
            | ((data[1] as u32 & 0x3F) << 12)
            | ((data[2] as u32 & 0x3F) << 6)
            | (data[3] as u32 & 0x3F);
        return (Some(cp), 4);
    }

    // Invalid sequence
    (None, 1)
}

// ============================================================================
// DEFAULT EMBEDDED PSF2 FONT
// ============================================================================

/// Yerleşik 8x16 VGA font (PSF2 formatında değil, doğrudan glyph data)
/// vga_font.rs'den alınır, PSF2 wrapper olarak sunulur
pub fn embedded_vga_font() -> Psf2Font {
    let header = Psf2Header {
        magic: PSF2_MAGIC,
        version: 0,
        headersize: 32,
        flags: 0,
        numglyph: 95, // ASCII 32-126
        bytesperglyph: 16,
        height: 16,
        width: 8,
    };

    // VGA font verisini al
    let glyph_data = super::vga_font::get_font_data_raw().to_vec();

    // ASCII mapping oluştur
    let mut unicode_map = BTreeMap::new();
    for i in 0..95u32 {
        unicode_map.insert(32 + i, i); // ASCII 32-126
    }

    Psf2Font {
        header,
        glyph_data,
        unicode_map,
    }
}
