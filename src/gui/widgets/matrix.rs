//! # echOS Matrix Rain Widget
//!
//! Demo ve stres testi icin retained raster tabanli "Matrix" efekti.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::widgets::{draw_render_objects, raster_object, Rect, Widget};
use crate::random;
use alloc::vec;
use alloc::vec::Vec;

struct Column {
    x: i32,
    y: i32,
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
        draw_render_objects(fb, self.rect, &self.render_objects());
    }

    fn update(&mut self) {
        self.tick += 1;
        for col in &mut self.columns {
            if self.tick % 2 == 0 {
                col.y += col.speed;
            }

            if random::next_range(100) < 5 {
                let idx = random::next_range(col.chars.len() as u32) as usize;
                col.chars[idx] = if col.chars[idx] == '0' { '1' } else { '0' };
            }

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

    fn render_objects(&self) -> Vec<RenderObject> {
        let width = self.rect.width.max(1) as usize;
        let height = self.rect.height.max(1) as usize;
        let mut pixels = vec![0u32; width.saturating_mul(height)];

        for col in &self.columns {
            for i in 0..col.len {
                let char_y = col.y - (i * 12);
                if char_y < self.rect.y || char_y >= self.rect.y + self.rect.height {
                    continue;
                }

                let color = if i == 0 {
                    0xFFFFFF
                } else if i < 4 {
                    0x88FF88
                } else {
                    0x00AA00
                };
                let char_idx = (i as usize) % col.chars.len();
                let glyph = crate::font::vga_font::get_font_data(col.chars[char_idx]);
                let local_x = col.x - self.rect.x;
                let local_y = char_y - self.rect.y;

                for (row, bits) in glyph.iter().enumerate() {
                    let py = local_y + row as i32;
                    if py < 0 || py >= self.rect.height {
                        continue;
                    }
                    for bit in 0..8 {
                        if (bits >> (7 - bit)) & 1 == 0 {
                            continue;
                        }
                        let px = local_x + bit;
                        if px < 0 || px >= self.rect.width {
                            continue;
                        }
                        pixels[py as usize * width + px as usize] = color;
                    }
                }
            }
        }

        Vec::from([raster_object(
            ((self.rect.x as u64) << 32) ^ self.rect.y as u64 ^ 0x900,
            self.rect,
            pixels,
            DamageLane::Window,
            0,
        )])
    }
}
