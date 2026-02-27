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

impl Default for Rect {
    fn default() -> Self {
        Self { x: 0, y: 0, width: 0, height: 0 }
    }
}

impl Rect {
    /// Yeni dikdörtgen oluşturur.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Verilen nokta bu dikdörtgenin içinde mi?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        let self_right = self.x + self.width;
        let self_bottom = self.y + self.height;
        let other_right = other.x + other.width;
        let other_bottom = other.y + other.height;
        self.x < other_right
            && self_right > other.x
            && self.y < other_bottom
            && self_bottom > other.y
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = if self.x < other.x { self.x } else { other.x };
        let y1 = if self.y < other.y { self.y } else { other.y };
        let x2 = if self.x + self.width > other.x + other.width {
            self.x + self.width
        } else {
            other.x + other.width
        };
        let y2 = if self.y + self.height > other.y + other.height {
            self.y + self.height
        } else {
            other.y + other.height
        };
        Rect::new(x1, y1, x2 - x1, y2 - y1)
    }
}

/// Klavye modifier tuşları
pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_SUPER: u8 = 0x08;

/// Tüm widget'ların implement etmesi gereken trait.
pub trait Widget: Send {
    /// Widget'ı framebuffer'a çizer.
    fn draw(&self, fb: &mut Framebuffer);

    /// Mouse click event'ini işler. True dönerse event yakalandı demektir.
    fn on_click(&mut self, x: i32, y: i32) -> bool;

    /// Klavye event'ini işler. True dönerse event yakalandı demektir.
    fn on_key(&mut self, _key: char, _modifiers: u8, _scancode: u8) -> bool {
        false
    }

    /// Mouse hover event'ini işler.
    fn on_hover(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    /// Mouse drag event'ini işler.
    fn on_drag(&mut self, _dx: i32, _dy: i32) -> bool {
        false
    }

    /// Mouse scroll event'ini işler.
    fn on_scroll(&mut self, _delta: i32) -> bool {
        false
    }

    /// Widget'ın sınır kutusunu döndürür.
    fn bounds(&self) -> Rect;

    /// Widget durumunu günceller (animasyonlar için).
    fn update(&mut self) {}

    /// Widget odaklı mı?
    fn is_focused(&self) -> bool {
        false
    }

    /// Widget odak durumunu ayarlar.
    fn set_focus(&mut self, _focused: bool) {}
}

/// Button widget
pub mod button;
/// Label widget (text display)
pub mod label;
/// Matrix animasyon widget (Matrix filmi efekti)
pub mod matrix;
/// Text input widget (TextBox, TextArea)
pub mod text_input;
/// Checkbox and RadioButton widgets
pub mod checkbox;
/// ListView and TreeView widgets
pub mod list;
/// Menu widgets (Menu, ContextMenu, MenuItem)
pub mod menu;
/// ScrollBar and Slider widgets
pub mod scroll;
/// ProgressBar and Spinner widgets
pub mod progress;
/// Dialog widgets (Dialog, MessageBox, FileDialog)
pub mod dialog;
/// Container widgets (Panel, TabControl, Splitter)
pub mod container;
