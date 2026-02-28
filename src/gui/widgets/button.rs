//! # echOS Button Widget
//!
//! Tıklanabilir buton bileşeni.
//!
//! ## Lifetime Parametresi `'a`
//!
//! `Button<'a>` struct'ındaki `'a` lifetime parametresi `text: &'a str` alanından
//! gelir. Buton, gösterilecek metni kopyalamak yerine bir referans olarak tutar.
//! Bu sayede heap allocation olmadan (no_std uyumlu) metin gösterimi sağlanır.
//! Butonun var olduğu süre boyunca metin de geçerli kalmalıdır.
//!
//! ## Durum Makinesi
//!
//! Butonun görsel durumu `hovered` ve `pressed` boolean'larıyla izlenir.
//! Bu iki bayrak birlikte küçük bir durum makinesi oluşturur ve Tema sistemi
//! üzerinden her duruma farklı renk atanır.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;

/// Tıklanabilir buton widget'ı.
///
/// Buton, bir metin etiketi, arka plan rengi ve kenarlıktan oluşur.
/// Hover (üzerine gelme) ve pressed (basılı) için farklı görünümler sunar.
/// `bg_color` ve `text_color` alanları oluşturma sırasında tema renklerinden
/// alınır; bu sayede tema değiştiğinde tüm butonların rengi güncellenir.
pub struct Button<'a> {
    rect: Rect,
    text: &'a str,
    bg_color: u32,
    text_color: u32,
    hovered: bool,
    pressed: bool,
}

impl<'a> Button<'a> {
    /// Yeni buton oluşturur.
    ///
    /// Renk değerleri `Theme` sabitleri üzerinden `to_u32()` conversion yöntemiyle
    /// 32-bit ARGB/RGBA formatına dönüştürülür. Framebuffer pikseller 32-bit
    /// tam sayılarla temsil edilir; her byte bir renk kanalına karşılık gelir.
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

        // Duruma göre renk seçimi:
        // pressed ve hovered aynı rengi kullanır; daha ince bir efekt için
        // ayrı renkler de tanımlanabilir. if-else zinciri bir öncelik sırası
        // oluşturur: önce pressed kontrol edilir, sonra hovered, son olarak
        // normal durum.
        let color = if self.pressed {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            self.bg_color
        };

        // Arkaplan: tüm piksellerle teker teker doldurulur.
        // Bu iç içe döngü O(w*h) karmaşıklığındadır. Çizim her frame'de
        // yapıldığında performanslı bir draw_rect yardımcı fonksiyonu
        // kullanmak daha verimlidir; burada doğrudan piksel döngüsü eğiticidir.
        for row in y..(y + h) {
            for col in x..(x + w) {
                fb.plot_pixel(col, row, color);
            }
        }

        // Kenarlık: dört kenarı ayrı ayrı tarar.
        // Üst ve alt kenar için yatay döngü, sol ve sağ kenar için dikey
        // döngü kullanılır; köşe pikseller her iki döngüde de çizilir.
        let border_color = Theme::BORDER.to_u32();
        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color); // Üst
            fb.plot_pixel(col, y + h - 1, border_color); // Alt
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color); // Sol
            fb.plot_pixel(x + w - 1, row, border_color); // Sağ
        }

        // Metin (Ortalanmış)
        // Her karakter 8 piksel genişliğinde sabit aralıklı (monospace) yazı
        // tipinde çizilir. Metnin toplam genişliği `len * 8` olarak hesaplanır.
        // Yatay ortalama: (alan_genişliği - metin_genişliği) / 2.
        // Dikey ortalama: 16 piksellik karakter yüksekliği sabit alınır.
        let text_width = self.text.len() * 8;
        let text_x = if text_width < w {
            x + (w - text_width) / 2
        } else {
            x + 5
        };
        let text_y = y + (h - 16) / 2;

        fb.draw_string(text_x, text_y, self.text, self.text_color);
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.pressed = !self.pressed; // Toggle efekti: her tıklamada durum değişir
            true
        } else {
            false
        }
    }

    /// Butonun sınır dikdörtgenini döndürür.
    ///
    /// `Rect` türü `Copy` trait'ini implement ettiği için bu döndürme bir
    /// kopyalama işlemidir; referans döndürmeye gerek yoktur. `Copy` türleri
    /// bellek adresi yerine değer üzerinden kopyalanır.
    fn bounds(&self) -> Rect {
        self.rect
    }
}
