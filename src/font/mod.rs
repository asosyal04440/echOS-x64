//! # echOS Font Modülü
//!
//! Bitmap font verileri ve metin rendering desteği.
//!
//! ## PSF (PC Screen Font) Formatı
//!
//! PSF, Linux ve embedded sistemlerde kullanılan basit bitmap font formatıdır.
//! echOS bu formattaki VGA 8x16 fontu kullanır.
//!
//! Her karakter (glyph) 16 byte'tan oluşur.
//! Her byte bir tarama satırını (scan row) temsil eder.
//! Her bit, o satırdaki bir pikselin açık (1) ya da kapalı (0) olduğunu gösterir.
//!
//! ## 'A' Harfi Glyph Bitmap Diyagramı (8x16)
//!
//! ```text
//!  Byte  Hex   7 6 5 4 3 2 1 0   Görsel
//!  ----  ----  ---------------   ------
//!   0    0x00  . . . . . . . .
//!   1    0x00  . . . . . . . .
//!   2    0x18  . . . 1 1 . . .       **
//!   3    0x3C  . . 1 1 1 1 . .      ****
//!   4    0x66  . 1 1 . . 1 1 .     **  **
//!   5    0xC3  1 1 . . . . 1 1    **    **
//!   6    0xC3  1 1 . . . . 1 1    **    **
//!   7    0xC3  1 1 . . . . 1 1    **    **
//!   8    0xFF  1 1 1 1 1 1 1 1    ********  ← bu satır = 0xFF
//!   9    0xC3  1 1 . . . . 1 1    **    **
//!  10    0xC3  1 1 . . . . 1 1    **    **
//!  11    0xC3  1 1 . . . . 1 1    **    **
//!  12    0x00  . . . . . . . .
//!  13    0x00  . . . . . . . .
//!  14    0x00  . . . . . . . .
//!  15    0x00  . . . . . . . .
//! ```
//!
//! Okuma yöntemi: Her byte'ın en yüksek anlamlı biti (bit 7) sol piksele karşılık gelir.
//! `(byte >> (7 - col)) & 1` ifadesi ile col=0..8 arası bit okunur.

/// VGA 8x16 bitmap font verisi (ASCII 0x20–0x7E ve Türkçe Unicode'lar)
pub mod vga_font;

/// PSF2 font loader (PC Screen Font v2)
pub mod psf2;

/// FontRenderer API - tüm font tiplerini birleştiren yüksek seviye render API
pub mod renderer;
