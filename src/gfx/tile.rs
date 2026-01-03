//! # echOS Tile Sistemi
//! 
//! Tile-based rendering için temel yapılar.
//! Ekran 32x32 piksellik tile'lara bölünür.

use core::ops::Range;

/// Tile boyutu (32x32 piksel = 4KB buffer)
pub const TILE_SIZE: usize = 32;

/// Ekranın 32x32 piksellik bir bölgesi.
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    /// Tile grid X koordinatı
    pub x: usize,
    /// Tile grid Y koordinatı
    pub y: usize,
    /// Sol üst köşe piksel X
    pub pixel_x: usize,
    /// Sol üst köşe piksel Y
    pub pixel_y: usize,
    /// Tile genişliği (kenar tile'lar için < 32 olabilir)
    pub width: usize,
    /// Tile yüksekliği (kenar tile'lar için < 32 olabilir)
    pub height: usize,
}

/// Tüm tile'lar üzerinde iterasyon için iterator.
pub struct TileIterator {
    screen_width: usize,
    screen_height: usize,
    current_x: usize,
    current_y: usize,
}

impl TileIterator {
    /// Yeni tile iterator oluşturur.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            screen_width: width,
            screen_height: height,
            current_x: 0,
            current_y: 0,
        }
    }
}

impl Iterator for TileIterator {
    type Item = Tile;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_y * TILE_SIZE >= self.screen_height {
            return None;
        }

        let px = self.current_x * TILE_SIZE;
        let py = self.current_y * TILE_SIZE;

        // Kenar tile'lar için boyut düzeltmesi
        let w = if px + TILE_SIZE > self.screen_width {
            self.screen_width - px
        } else {
            TILE_SIZE
        };

        let h = if py + TILE_SIZE > self.screen_height {
            self.screen_height - py
        } else {
            TILE_SIZE
        };

        let tile = Tile {
            x: self.current_x,
            y: self.current_y,
            pixel_x: px,
            pixel_y: py,
            width: w,
            height: h,
        };

        // Sonraki tile'a ilerle
        self.current_x += 1;
        if self.current_x * TILE_SIZE >= self.screen_width {
            self.current_x = 0;
            self.current_y += 1;
        }

        Some(tile)
    }
}

/// 32-byte aligned tile buffer (AVX2 için).
#[repr(align(32))]
pub struct AlignedTileBuffer {
    pub data: [u32; TILE_SIZE * TILE_SIZE],
}

impl AlignedTileBuffer {
    /// Sıfırlanmış yeni buffer oluşturur.
    pub fn new() -> Self {
        Self {
            data: [0u32; TILE_SIZE * TILE_SIZE],
        }
    }
}
