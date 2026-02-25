//! # echOS Matrix Rain Widget
//!
//! Görsel stres testi ve demo amaçlı "Matrix" efekti bileşeni.
//! Rastgele karakterleri aşağı doğru yağdırır.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::widgets::{Rect, Widget};
use crate::random;
use alloc::vec::Vec;

struct Column {
    x: i32,
    y: i32, // Başlangıç Y pozisyonu
    speed: i32,
    len: i32,
    chars: Vec<char>,
}

pub struct MatrixRain {
    rect: Rect,
    columns: Vec<Column>,
    tick: usize,
}

impl MatrixRain {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        let mut columns = Vec::new();
        let col_width = 10;
        let num_cols = width / col_width;

        for i in 0..num_cols {
            columns.push(Column {
                x: x + i * col_width,
                y: random::next_range(height as u32) as i32 - (random::next_range(100) as i32),
                speed: (random::next_range(3) + 1) as i32,
                len: (random::next_range(15) + 5) as i32,
                chars: generate_random_chars(20),
            });
        }

        Self {
            rect: Rect::new(x, y, width, height),
            columns,
            tick: 0,
        }
    }
}

fn generate_random_chars(len: usize) -> Vec<char> {
    let mut v = Vec::with_capacity(len);
    let symbols = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'X', 'Z',
        '$', '#',
    ];
    for _ in 0..len {
        let idx = random::next_range(symbols.len() as u32) as usize;
        v.push(symbols[idx]);
    }
    v
}

impl Widget for MatrixRain {
    fn draw(&self, fb: &mut Framebuffer) {
        // Arkaplanı siyah boya
        fb.draw_rect(
            self.rect.x as usize,
            self.rect.y as usize,
            self.rect.width as usize,
            self.rect.height as usize,
            0x000000,
        );

        for col in &self.columns {
            // İzi (Trail) çiz
            for i in 0..col.len {
                let char_y = col.y - (i * 12); // 12px karakter yüksekliği
                if char_y >= self.rect.y && char_y < self.rect.y + self.rect.height {
                    // İze göre renk belirle
                    let color = if i == 0 {
                        0xFFFFFF // Baş: Beyaz
                    } else if i < 4 {
                        0x88FF88 // Parlak Yeşil
                    } else {
                        0x00AA00 // Koyu Yeşil
                    };

                    // Karakter seçimi
                    let char_idx = (i as usize) % col.chars.len();
                    fb.draw_char(
                        col.x as usize,
                        char_y as usize,
                        col.chars[char_idx],
                        color as u32,
                    );
                }
            }
        }
    }

    fn update(&mut self) {
        self.tick += 1;

        // Sütunları güncelle
        for col in &mut self.columns {
            // Aşağı hareket
            if self.tick % 2 == 0 {
                // Biraz yavaşlat
                col.y += col.speed;
            }

            // Rastgele karakter değişimi
            if random::next_range(100) < 5 {
                let idx = random::next_range(col.chars.len() as u32) as usize;
                col.chars[idx] = if col.chars[idx] == '0' { '1' } else { '0' };
            }

            // Aşağıdan çıkınca yukarı taşı
            if col.y - (col.len * 12) > self.rect.y + self.rect.height {
                col.y = self.rect.y - (random::next_range(50) as i32);
                col.speed = (random::next_range(4) + 2) as i32;
            }
        }
    }

    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        true
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}
