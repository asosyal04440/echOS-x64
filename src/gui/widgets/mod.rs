//! # echOS Widget Sistemi
//!
//! GUI widget'ları için temel trait ve ortak türler.
//! Button, Label, Matrix gibi widget'lar için altyapı.
//!
//! ## Widget Nedir?
//!
//! Widget, kullanıcı arayüzündeki temel görsel bileşenlerdir: butonlar,
//! etiketler, liste kutuları vb. Her widget ekranda bir dikdörtgen alan kaplar
//! ve kullanıcı etkileşimlerine (tıklama, klavye, kaydırma) yanıt verir.
//!
//! ## Trait Tabanlı Tasarım
//!
//! Rust'ta polimorfizm için trait'ler kullanılır. `Widget` trait'i sayesinde
//! farklı widget türleri aynı arayüz üzerinden yönetilebilir. Bu, nesne
//! yönelimli programlamadaki abstract class kavramına benzer.

use crate::gop::framebuffer::Framebuffer;

/// Ekran üzerindeki dikdörtgen bölge.
///
/// GUI sistemlerindeki her bileşen bir dikdörtgen alanla tanımlanır.
/// `x`, `y` sol üst köşenin koordinatları; `width` ve `height` ise
/// bileşenin piksel cinsinden boyutlarıdır.
///
/// `i32` kullanılmasının sebebi: negatif koordinatlar (ekran dışı konumlar)
/// mümkün olmalı ve kırpma/çakışma hesaplamalarında negatif ara değerler çıkabilir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
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
    ///
    /// Hit-testing için kullanılır: kullanıcı tıkladığında hangi widget'ın
    /// tıklandığını bulmak için her widget'ın `bounds()` alanı bu yöntemle
    /// kontrol edilir. `x < self.x + self.width` koşulu sağ sınırı dahil etmez
    /// (yarı açık aralık [x, x+w) ), bu standart piksel sınırı konvansiyonudur.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// İki dikdörtgenin kesişip kesişmediğini kontrol eder.
    ///
    /// Ekran güncelleme optimizasyonunda (dirty region tracking) kullanılır:
    /// sadece değişen bölgeleri yeniden çizmek için hangi widget'ların
    /// etkilendiğini bulmaya yarar. AABB (Axis-Aligned Bounding Box) algoritması
    /// iki dikdörtgenin birbirinin dışında olmadığını kontrol eder.
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

    /// İki dikdörtgeni kapsayan en küçük dikdörtgeni döndürür.
    ///
    /// Birden fazla widget'ın güncellenmesi gerektiğinde her ikisini de
    /// kapsayan tek bir bölgeyi yeniden çizmek için kullanılır. Min/max
    /// hesaplaması yaparak iki dikdörtgenin birleşim (union) bounding box'ını
    /// döndürür.
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
///
/// Bit bayrakları olarak tanımlanır; birden fazla modifier aynı anda
/// basılı tutulabilir (örn. CTRL+SHIFT = 0x01 | 0x02 = 0x03).
/// `&` operatörü ile belirli bir modifier aktif mi diye kontrol edilir:
/// `if modifiers & MOD_CTRL != 0 { ... }`.
pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_SUPER: u8 = 0x08;

// ────────────────────────────────────────────────────────────
// Layout Kısıtlamaları
// ────────────────────────────────────────────────────────────

/// Üst kap widget'ının çocuğa sunduğu alan kısıtlamaları.
///
/// Flex/grid layout engine'in `layout()` çağrısıyla aktarılır.
/// `max_width` / `max_height` = 0 ise sınırsız demektir.
#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    pub x: i32,
    pub y: i32,
    pub max_width: i32,
    pub max_height: i32,
}

impl LayoutConstraints {
    pub fn new(x: i32, y: i32, max_width: i32, max_height: i32) -> Self {
        Self {
            x,
            y,
            max_width,
            max_height,
        }
    }

    pub fn unconstrained() -> Self {
        Self {
            x: 0,
            y: 0,
            max_width: 0,
            max_height: 0,
        }
    }
}

// ────────────────────────────────────────────────────────────
// Erişilebilirlik (Accessibility)
// ────────────────────────────────────────────────────────────

/// Widget erişilebilirlik rolü (WAI-ARIA benzeri).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRole {
    Generic,
    Button,
    TextInput,
    Checkbox,
    RadioButton,
    Slider,
    ProgressBar,
    List,
    ListItem,
    Menu,
    MenuItem,
    Dialog,
    Label,
    Container,
    ScrollBar,
    Tab,
    TabPanel,
}

/// Erişilebilirlik durumu bayrakları.
#[derive(Debug, Clone, Copy)]
pub struct AccessState(u8);

impl AccessState {
    pub const FOCUSED: u8 = 0x01;
    pub const DISABLED: u8 = 0x02;
    pub const CHECKED: u8 = 0x04;
    pub const EXPANDED: u8 = 0x08;
    pub const SELECTED: u8 = 0x10;

    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn has(&self, flag: u8) -> bool {
        self.0 & flag != 0
    }
    pub const fn with(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }
}

/// Erişilebilirlik bilgisi — ekran okuyucular ve yardımcı teknolojiler için.
#[derive(Debug, Clone, Copy)]
pub struct AccessibilityInfo<'a> {
    pub role: AccessRole,
    pub label: &'a str,
    pub value: &'a str,
    pub state: AccessState,
}

/// Tüm widget'ların implement etmesi gereken trait.
///
/// `Send` bound'u: widget'ların thread'ler arasında güvenle taşınabilmesini
/// sağlar. `no_std` ortamında async runtime olmasa da bu güvenlik garantisi
/// önemlidir. Trait object (`dyn Widget`) kullanımı için `Sized` olmayan
/// türlere de uygulanabilirlik sağlanır.
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

    /// Widget odaklanabilir mi?
    fn can_focus(&self) -> bool {
        false
    }

    /// Verilen noktanın bu widget üzerine düşüp düşmediğini test eder.
    ///
    /// Varsayılan implementasyon `bounds().contains()` kullanır.
    /// Yuvarlak köşeli veya düzensiz şekilli widget'lar override edebilir.
    fn hit_test(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    /// Üst kap tarafından sunulan kısıtlamalar içinde widget boyutunu hesaplar.
    ///
    /// `constraints`: üst kap tarafından verilen mevcut alan (x, y, max_width, max_height).
    /// Varsayılan implementasyon mevcut bounds'u korur (absolute positioning).
    /// Layout engine kullanan widget'lar override eder.
    fn layout(&mut self, _constraints: LayoutConstraints) {}

    /// Erişilebilirlik bilgisi döndürür.
    ///
    /// Ekran okuyucular ve yardımcı teknolojiler için widget rolü, etiketi
    /// ve durumunu bildirir. Varsayılan: Generic rol, boş etiket.
    fn accessibility_info(&self) -> AccessibilityInfo {
        AccessibilityInfo {
            role: AccessRole::Generic,
            label: "",
            value: "",
            state: AccessState::empty(),
        }
    }

    fn screen_reader_snapshot(&self) -> Option<AccessibilityInfo<'_>> {
        let info = self.accessibility_info();
        if info.label.is_empty() && info.value.is_empty() {
            None
        } else {
            Some(info)
        }
    }
}

/// Button widget
pub mod button;
/// Checkbox and RadioButton widgets
pub mod checkbox;
/// Container widgets (Panel, TabControl, Splitter)
pub mod container;
/// Dialog widgets (Dialog, MessageBox, FileDialog)
pub mod dialog;
/// Label widget (text display)
pub mod label;
/// ListView and TreeView widgets
pub mod list;
/// Matrix animasyon widget (Matrix filmi efekti)
pub mod matrix;
/// Menu widgets (Menu, ContextMenu, MenuItem)
pub mod menu;
/// ProgressBar and Spinner widgets
pub mod progress;
/// ScrollBar and Slider widgets
pub mod scroll;
/// Text input widget (TextBox, TextArea)
pub mod text_input;
