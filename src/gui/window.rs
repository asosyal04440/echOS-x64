//! # echOS Pencere Bileşeni
//!
//! Pencereler, başlık çubuğu (titlebar), kenarlıklar ve içerik alanından oluşur.
//! İçine `Widget` eklenebilir.
//!
//! ## Animasyon Algoritması
//! Açılma animasyonu ease-out dörtlü (quadratic) enterpolasyon kullanır:
//! t ∈ [0,1] için ease = 1 - (1-t)² — başlangıçta hızlı, sona yakın yavaş.
//! Pencere merkezden küçük başlayıp hedef boyutuna ulaşır.

use super::theme::Theme;
use super::widgets::Widget;
use crate::gop::framebuffer::Framebuffer;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Animation state for window
#[derive(Clone, Copy, PartialEq)]
pub enum AnimationState {
    None,
    Opening,    // Fade-in + scale up
    Closing,    // Fade-out + scale down
}

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
    /// Animation state
    pub animation: AnimationState,
    /// Animation progress (0.0 to 1.0)
    pub anim_progress: f32,
    /// Target dimensions for animation
    pub target_x: usize,
    pub target_y: usize,
    pub target_width: usize,
    pub target_height: usize,
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
            animation: AnimationState::None,
            anim_progress: 1.0,
            target_x: x,
            target_y: y,
            target_width: width,
            target_height: height,
        }
    }
    
    /// Start opening animation
    pub fn start_open_animation(&mut self) {
        self.animation = AnimationState::Opening;
        self.anim_progress = 0.0;
        // Start from center, small
        self.x = self.target_x + self.target_width / 4;
        self.y = self.target_y + self.target_height / 4;
        self.width = self.target_width / 2;
        self.height = self.target_height / 2;
    }
    
    /// Update animation (call each frame)
    /// Returns true if animation is still running
    pub fn update_animation(&mut self) -> bool {
        if self.animation == AnimationState::None {
            return false;
        }
        
        // Ease-out animation
        self.anim_progress += 0.08;
        if self.anim_progress >= 1.0 {
            self.anim_progress = 1.0;
            self.animation = AnimationState::None;
            self.x = self.target_x;
            self.y = self.target_y;
            self.width = self.target_width;
            self.height = self.target_height;
            return false;
        }
        
        // Interpolate size and position
        let t = self.anim_progress;
        let ease = 1.0 - (1.0 - t) * (1.0 - t); // Ease-out quad
        
        self.x = self.target_x + (self.target_width as f32 * (1.0 - ease) * 0.25) as usize;
        self.y = self.target_y + (self.target_height as f32 * (1.0 - ease) * 0.25) as usize;
        self.width = (self.target_width as f32 * (0.5 + 0.5 * ease)) as usize;
        self.height = (self.target_height as f32 * (0.5 + 0.5 * ease)) as usize;
        
        true
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
