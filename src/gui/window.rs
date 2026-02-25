//! # echOS Pencere Bileşeni
//!
//! Pencereler, başlık çubuğu (titlebar), kenarlıklar ve içerik alanından oluşur.
//! İçine `Widget` eklenebilir.

use super::theme::Theme;
use super::widgets::Widget;
use crate::gop::framebuffer::Framebuffer;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// GUI Penceresi
pub struct Window<'a> {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: String,
    pub is_active: bool,
    pub content_lines: Vec<String>,
    pub titlebar_height: usize,
    pub widgets: Vec<Box<dyn Widget + 'a>>,
}

impl<'a> Window<'a> {
    pub fn new(x: usize, y: usize, width: usize, height: usize, title: &str) -> Self {
        Self {
            x,
            y,
            width,
            height,
            title: String::from(title),
            is_active: true,
            content_lines: Vec::new(),
            titlebar_height: 28,
            widgets: Vec::new(),
        }
    }

    /// Pencereye widget ekler.
    pub fn add_widget(&mut self, widget: Box<dyn Widget + 'a>) {
        self.widgets.push(widget);
    }

    /// Pencere içeriğine metin satırı ekler (Konsol benzeri).
    pub fn add_line(&mut self, text: &str) {
        self.content_lines.push(String::from(text));
        // Sadece son N satırı tut
        let max_lines = (self.height - self.titlebar_height - 10) / 18;
        while self.content_lines.len() > max_lines {
            self.content_lines.remove(0);
        }
    }

    /// Pencere içeriğini temizler.
    pub fn clear(&mut self) {
        self.content_lines.clear();
    }

    /// Son satırı günceller (Shell komut yazımı için).
    pub fn update_last_line(&mut self, text: &str) {
        if let Some(last) = self.content_lines.last_mut() {
            *last = String::from(text);
        } else {
            self.content_lines.push(String::from(text));
        }
    }

    /// Pencereyi çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Gölge çiz
        fb.draw_rect(
            self.x + 4,
            self.y + 4,
            self.width,
            self.height,
            Theme::SHADOW.to_u32(),
        );

        // Pencere arkaplanı
        fb.draw_rect(
            self.x,
            self.y,
            self.width,
            self.height,
            Theme::WINDOW_BG.to_u32(),
        );

        // Başlık çubuğu
        let titlebar_color = if self.is_active {
            Theme::TITLEBAR_ACTIVE.to_u32()
        } else {
            Theme::TITLEBAR_BG.to_u32()
        };
        fb.draw_rect(
            self.x,
            self.y,
            self.width,
            self.titlebar_height,
            titlebar_color,
        );

        // Başlık metni
        fb.draw_string(
            self.x + 10,
            self.y + 6,
            &self.title,
            Theme::TEXT_PRIMARY.to_u32(),
        );

        // Kapatma butonu (Görsel)
        let close_x = self.x + self.width - 24;
        let close_y = self.y + 6;
        fb.draw_rect(close_x, close_y, 16, 16, Theme::ACCENT_ERROR.to_u32());

        // Kenarlıklar
        self.draw_border(fb);

        // İçerik
        self.draw_content(fb);

        // Widgetlar
        for widget in &self.widgets {
            widget.draw(fb);
        }
    }

    fn draw_border(&self, fb: &mut Framebuffer) {
        let color = Theme::BORDER.to_u32();

        // Üst
        for x in self.x..self.x + self.width {
            fb.plot_pixel(x, self.y, color);
        }
        // Alt
        for x in self.x..self.x + self.width {
            fb.plot_pixel(x, self.y + self.height - 1, color);
        }
        // Sol
        for y in self.y..self.y + self.height {
            fb.plot_pixel(self.x, y, color);
        }
        // Sağ
        for y in self.y..self.y + self.height {
            fb.plot_pixel(self.x + self.width - 1, y, color);
        }
    }

    fn draw_content(&self, fb: &mut Framebuffer) {
        let content_y = self.y + self.titlebar_height + 5;
        let line_height = 18;

        for (i, line) in self.content_lines.iter().enumerate() {
            let y = content_y + i * line_height;
            if y + 16 < self.y + self.height - 5 {
                fb.draw_string(self.x + 10, y, line, Theme::TEXT_PRIMARY.to_u32());
            }
        }
    }

    /// Tıklamanın başlık çubuğunda olup olmadığını kontrol eder.
    pub fn is_titlebar_hit(&self, x: i32, y: i32) -> bool {
        x >= self.x as i32
            && x < (self.x + self.width) as i32
            && y >= self.y as i32
            && y < (self.y + self.titlebar_height) as i32
    }

    /// Tıklama olayını işler.
    pub fn on_click(&mut self, x: i32, y: i32) -> bool {
        if x >= self.x as i32
            && x < (self.x + self.width) as i32
            && y >= self.y as i32
            && y < (self.y + self.height) as i32
        {
            // Widgetlara ilet (Ters sırayla, üsttekine önce)
            for widget in self.widgets.iter_mut().rev() {
                if widget.on_click(x, y) {
                    return true;
                }
            }

            true // Pencere yakaladı
        } else {
            false
        }
    }

    /// Klavye olayını işler.
    pub fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        for widget in self.widgets.iter_mut().rev() {
            if widget.on_key(key, modifiers, scancode) {
                return true;
            }
        }
        false
    }

    pub fn update(&mut self) -> bool {
        for widget in &mut self.widgets {
            widget.update();
        }

        // Widget varsa animasyon olabilir, redraw iste.
        !self.widgets.is_empty()
    }
}
