//! # Shortcut Cheatsheet
//!
//! Kısayolları gösteren overlay.
//! Super+? ile açılır.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::widgets::Rect;
use crate::gui::theme::Theme;
use crate::gui::echos_wm::{SHORTCUT_TABLE, Modifiers};
use alloc::string::String;
use alloc::format;

pub struct Cheatsheet {
    visible: bool,
    rect: Rect,
}

impl Cheatsheet {
    pub fn new(screen_w: usize, screen_h: usize) -> Self {
        let width = 600;
        let height = 500;
        let x = (screen_w - width) / 2;
        let y = (screen_h - height) / 2;
        
        Self {
            visible: false,
            rect: Rect::new(x as i32, y as i32, width as i32, height as i32),
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible { return; }
        
        // Arka plan
        let bg_color = 0xF0101010; // Koyu, opak
        for y in self.rect.y..self.rect.y + self.rect.height {
            for x in self.rect.x..self.rect.x + self.rect.width {
                if x >= 0 && x < fb.width as i32 && y >= 0 && y < fb.height as i32 {
                    fb.plot_pixel(x as usize, y as usize, bg_color);
                }
            }
        }
        
        // Başlık
        let title_x = self.rect.x + 20;
        let title_y = self.rect.y + 20;
        fb.draw_string(title_x as usize, title_y as usize, "Keyboard Shortcuts", Theme::get_accent());
        
        // Kısayolları listele
        let mut y = title_y + 40;
        let col1_x = title_x;
        let col2_x = title_x + 300;
        
        for (i, (mods, scancode, id)) in SHORTCUT_TABLE.iter().enumerate() {
            if y > self.rect.y + self.rect.height - 30 { break; }
            
            let mut key_str = String::new();
            if mods & Modifiers::SUPER != 0 { key_str.push_str("Super + "); }
            if mods & Modifiers::CTRL != 0 { key_str.push_str("Ctrl + "); }
            if mods & Modifiers::ALT != 0 { key_str.push_str("Alt + "); }
            if mods & Modifiers::SHIFT != 0 { key_str.push_str("Shift + "); }
            
            // Scancode to Key name (Basit mapping)
            let key_name = match scancode {
                0x14 => "T",
                0x19 => "P",
                0x3D => "F3",
                0x4B => "Left",
                0x4D => "Right",
                0x48 => "Up",
                0x50 => "Down",
                0x3B => "F4",
                0x23 => "H",
                0x21 => "F",
                0x0F => "Tab",
                0x20 => "D",
                0x26 => "L",
                0x02..=0x05 => "Num", // 1-4
                0x1E => "A",
                0x39 => "Space",
                0x1F => "S",
                0x37 => "Print",
                0x2E => "C",
                0x2D => "X",
                0x2F => "V",
                _ => "?",
            };
            key_str.push_str(key_name);
            
            let desc = format!("{:?}", id);
            
            let x = if i % 2 == 0 { col1_x } else { col2_x };
            fb.draw_string(x as usize, y as usize, &key_str, Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string((x + 120) as usize, y as usize, &desc, Theme::TEXT_SECONDARY.to_u32());
            
            if i % 2 == 1 {
                y += 20;
            }
        }
    }
}
