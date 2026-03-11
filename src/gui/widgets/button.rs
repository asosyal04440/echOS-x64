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
use crate::gui::theme::{ButtonRole, Theme, ThemeMode};

/// Tıklanabilir buton widget'ı.
///
/// Buton, bir metin etiketi, arka plan rengi ve kenarlıktan oluşur.
/// Hover (üzerine gelme) ve pressed (basılı) için farklı görünümler sunar.
/// `bg_color` ve `text_color` alanları oluşturma sırasında tema renklerinden
/// alınır; bu sayede tema değiştiğinde tüm butonların rengi güncellenir.
pub struct Button<'a> {
    rect: Rect,
    text: &'a str,
    role: ButtonRole,
    hovered: bool,
    pressed: bool,
    /// Devre dışı durumu — tıklama yok sayılır, soluk renkte çizilir.
    enabled: bool,
    /// Odak durumu — klavye ile Enter/Space ile tetiklenebilir.
    focused: bool,
    /// Tıklama geri çağırma (opsiyonel). None ise toggle efekti.
    on_click_fn: Option<fn()>,
}

impl<'a> Button<'a> {
    /// Yeni buton oluşturur.
    pub fn new(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self {
            rect: Rect::new(
                x,
                y,
                width.max(Theme::MIN_HIT_WIDTH),
                height.max(Theme::MIN_HIT_HEIGHT),
            ),
            text,
            role: ButtonRole::Secondary,
            hovered: false,
            pressed: false,
            enabled: true,
            focused: false,
            on_click_fn: None,
        }
    }

    pub fn primary(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self::new(x, y, width, height, text).with_role(ButtonRole::Primary)
    }

    pub fn tertiary(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self::new(x, y, width, height, text).with_role(ButtonRole::Tertiary)
    }

    pub fn with_role(mut self, role: ButtonRole) -> Self {
        self.role = role;
        self
    }

    /// Tıklama geri çağırması ayarlar.
    pub fn with_on_click(mut self, cb: fn()) -> Self {
        self.on_click_fn = Some(cb);
        self
    }

    /// Butonun etkinlik durumunu ayarlar.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Butonun etkin olup olmadığını döndürür.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl<'a> Widget for Button<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        let mode = ThemeMode::Dark;

        // Disabled → soluk gri; pressed → koyu; hovered → parlak; normal → temel
        let color = if !self.enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else {
            Theme::button_fill(self.role, mode, self.pressed, self.hovered)
        };

        let text_c = if !self.enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else {
            Theme::button_text(self.role, mode)
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
        let border_color = if self.focused {
            Theme::BORDER_FOCUS.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
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

        fb.draw_string(text_x, text_y, self.text, text_c);

        // Odak halkası — focused ise kenarlık ACCENT renginde çizilir
        if self.focused && self.enabled {
            let focus_color = Theme::INPUT_FOCUS.to_u32();
            for col in x..(x + w) {
                fb.plot_pixel(col, y, focus_color);
                fb.plot_pixel(col, y + h - 1, focus_color);
            }
            for row in y..(y + h) {
                fb.plot_pixel(x, row, focus_color);
                fb.plot_pixel(x + w - 1, row, focus_color);
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.enabled {
            return false;
        }
        if self.rect.contains(x, y) {
            if let Some(cb) = self.on_click_fn {
                cb();
            }
            self.pressed = !self.pressed;
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was = self.hovered;
        self.hovered = self.rect.contains(x, y);
        self.hovered != was
    }

    fn on_key(&mut self, key: char, _modifiers: u8, _scancode: u8) -> bool {
        if !self.enabled || !self.focused {
            return false;
        }
        // Enter veya Space ile buton tetikleme
        if key == '\n' || key == ' ' {
            if let Some(cb) = self.on_click_fn {
                cb();
            }
            self.pressed = !self.pressed;
            return true;
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn is_focused(&self) -> bool {
        self.focused
    }
    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn accessibility_info(&self) -> super::AccessibilityInfo {
        use super::{AccessRole, AccessState, AccessibilityInfo};
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        if !self.enabled {
            state = state.with(AccessState::DISABLED);
        }
        AccessibilityInfo {
            role: AccessRole::Button,
            label: self.text,
            value: if self.pressed { "pressed" } else { "" },
            state,
        }
    }
}
