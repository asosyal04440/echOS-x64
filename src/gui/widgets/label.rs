//! # echOS Label Widget
//!
//! Metin etiketi bileşeni.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;

use alloc::string::String;
use alloc::string::ToString;

pub struct Label {
    rect: Rect,
    text: String,
    color: u32,
}

impl Label {
    pub fn new(x: i32, y: i32, text: &str) -> Self {
        let width = (text.len() * 8) as i32;
        Self {
            rect: Rect::new(x, y, width, 16),
            text: text.to_string(),
            color: Theme::TEXT_PRIMARY.to_u32(),
        }
    }

    pub fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        // Boyutu güncelle
        self.rect.width = (self.text.len() * 8) as i32;
    }
}

impl Widget for Label {
    fn draw(&self, fb: &mut Framebuffer) {
        fb.draw_string(
            self.rect.x as usize,
            self.rect.y as usize,
            &self.text,
            self.color,
        );
    }

    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn update(&mut self) {}
}
