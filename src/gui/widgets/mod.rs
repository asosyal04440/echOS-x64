//! # echOS Widget Sistemi
//! 
//! GUI widget'ları için temel trait ve ortak türler.
//! Button, Label, Matrix gibi widget'lar için altyapı.

use crate::gop::framebuffer::Framebuffer;

/// Ekran üzerindeki dikdörtgen bölge.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// Yeni dikdörtgen oluşturur.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
    
    /// Verilen nokta bu dikdörtgenin içinde mi?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width &&
        y >= self.y && y < self.y + self.height
    }
}

/// Tüm widget'ların implement etmesi gereken trait.
pub trait Widget {
    /// Widget'ı framebuffer'a çizer.
    fn draw(&self, fb: &mut Framebuffer);
    
    /// Mouse click event'ini işler. True dönerse event yakalandı demektir.
    fn on_click(&mut self, x: i32, y: i32) -> bool;
    
    /// Widget'ın sınır kutusunu döndürür.
    fn bounds(&self) -> Rect;
    
    /// Widget durumunu günceller (animasyonlar için).
    fn update(&mut self) {}
}

/// Button widget
pub mod button;
/// Label widget (text display)
pub mod label;
/// Matrix animasyon widget (Matrix filmi efekti)
pub mod matrix;
