//! # echOS Label Widget
//!
//! Metin etiketi bileşeni.
//!
//! ## Etiket Ne İşe Yarar?
//!
//! `Label`, salt okunur metin göstermek için en basit widget türüdür. Tıklanamaz,
//! odaklanamaz; yalnızca bir konuma metin yazar. Form elemanlarını açıklamak,
//! başlık göstermek veya dinamik değerleri (sayaç, durum mesajı vb.) ekrana
//! yansıtmak için kullanılır.
//!
//! ## Genişlik Hesabı
//!
//! Monospace (sabit aralıklı) yazı tipinde her karakter 8 piksel genişliğindedir.
//! Bu nedenle `width = text.len() * 8` formülü metnin piksel genişliğini verir.
//! Metin değiştiğinde `set_text` hem metni hem genişliği günceller; böylece
//! hit-testing sınır kutusu her zaman doğru kalır.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;

use alloc::string::String;
use alloc::string::ToString;

/// Salt okunur metin etiketi widget'ı.
///
/// `text: String` heap üzerinde sahip olunan metin verisidir; `Button<'a>`'den
/// farklı olarak referans değil değer tutar. Bu, etiketin oluşturulduktan sonra
/// metnin değiştirilmesine (`set_text`) olanak tanır.
///
/// `color: u32` ARGB/RGBA formatında 32-bit piksel rengidir.
pub struct Label {
    rect: Rect,
    text: String,
    color: u32,
}

impl Label {
    /// Yeni etiket oluşturur; genişlik metinden otomatik hesaplanır.
    ///
    /// `text.to_string()` `ToString` trait'i aracılığıyla `&str`'yi `String`'e
    /// dönüştürür. Alternatif olarak `String::from(text)` de kullanılabilir;
    /// ikisi işlevsel olarak eşdeğerdir.
    pub fn new(x: i32, y: i32, text: &str) -> Self {
        let width = (text.len() * 8) as i32;
        Self {
            rect: Rect::new(x, y, width, 16),
            text: text.to_string(),
            color: Theme::TEXT_PRIMARY.to_u32(),
        }
    }

    /// Builder: metin rengini özelleştirir.
    ///
    /// `mut self` alıp `Self` döndüren builder pattern;
    /// `Label::new(...).with_color(0xFF0000FF)` zincirleme kullanımına izin verir.
    pub fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    /// Metni günceller ve genişliği yeniden hesaplar.
    ///
    /// `rect.width` güncellendiği için etiket yeniden çizildiğinde
    /// sınır kutusu da doğru boyutu yansıtır. `set_text` `String` alır
    /// çünkü sahipliği devralır; `&str` almak isteseydi ek `to_string()` gerekirdi.
    pub fn set_text(&mut self, text: String) {
        self.text = text;
        // Boyutu güncelle
        self.rect.width = (self.text.len() * 8) as i32;
    }
}

impl Widget for Label {
    /// Metni framebuffer'a çizer.
    ///
    /// `draw_string` koordinatları `usize` ister; `as usize` güvenli dönüşüm
    /// sağlar (i32 pozitifse keserek dönüştürür). Etiket arka plan rengi
    /// çizmez; altındaki içerik üzerine metin yazar (saydam arka plan).
    fn draw(&self, fb: &mut Framebuffer) {
        fb.draw_string(
            self.rect.x as usize,
            self.rect.y as usize,
            &self.text,
            self.color,
        );
    }

    /// Etiketler tıklanamaz; her zaman false döner.
    ///
    /// `_x` ve `_y` parametreleri kullanılmaz; `_` öneki derleyici uyarısını
    /// bastırır. Bu, Widget trait'inin varsayılan olmayan zorunlu metodunun
    /// "bu widget için işlevsiz" implementasyonudur.
    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    /// Etiketin sınır dikdörtgenini döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Etiketlerin animasyon durumu yoktur; `update` boş.
    fn update(&mut self) {}
}
