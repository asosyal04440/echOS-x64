//! # echOS Button Widget
//! 
//! Tıklanabilir buton bileşeni.

use crate::gop::framebuffer::Framebuffer;
use super::{Widget, Rect};
use crate::gui::theme::{Theme, Color};

pub struct Button<'a> {
    rect: Rect,
    text: &'a str,
    bg_color: u32,
    text_color: u32,
    hovered: bool,
    pressed: bool,
}

impl<'a> Button<'a> {
    pub fn new(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            text,
            bg_color: Theme::BUTTON_BG.to_u32(),
            text_color: Theme::BUTTON_TEXT.to_u32(),
            hovered: false,
            pressed: false,
        }
    }
}

impl<'a> Widget for Button<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Duruma göre renk seçimi
        let color = if self.pressed {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            self.bg_color
        };
        
        // Arkaplan
        for row in y..(y + h) {
            for col in x..(x + w) {
                fb.plot_pixel(col, row, color);
            }
        }
        
        // Kenarlık
        let border_color = Theme::BORDER.to_u32();
        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color);          // Üst
            fb.plot_pixel(col, y + h - 1, border_color);  // Alt
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color);          // Sol
            fb.plot_pixel(x + w - 1, row, border_color);  // Sağ
        }
        
        // Metin (Ortalanmış)
        let text_width = self.text.len() * 8;
        let text_x = if text_width < w { x + (w - text_width) / 2 } else { x + 5 };
        let text_y = y + (h - 16) / 2;
        
        fb.draw_string(text_x, text_y, self.text, self.text_color);
    }
    
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.pressed = !self.pressed; // Toggle efekti
            true
        } else {
            false
        }
    }
    
    fn bounds(&self) -> Rect {
        self.rect
    }
}
