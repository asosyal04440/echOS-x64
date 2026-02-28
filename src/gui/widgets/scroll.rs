//! # echOS Kaydırma Çubuğu ve Kaydırıcı Widget'ları
//!
//! İçerik kaydırma ve değer seçimi için kullanılan widget'ları içerir.
//!
//! ## İçerilen Widget'lar
//! - [`ScrollBar`] — yatay veya dikey kaydırma çubuğu (thumb + ok düğmeleri)
//! - [`Slider`]    — minimum–maksimum aralığında değer seçici
//!
//! ## Etkileşim Modeli
//! Her iki widget da sürükleme (`on_drag`), tıklama (`on_click`) ve
//! fare üzerine gelme (`on_hover`) olaylarını destekler.
//! Değer değiştiğinde isteğe bağlı bir `on_change` işleyicisi tetiklenir.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;

/// Kaydırma çubuğunun yönünü belirler.
/// `Horizontal` yatay, `Vertical` dikey kulanım içindir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Kaydırma çubuğu widget'ı.
///
/// İçerik boyutu (`max_value`) ve görünür alan boyutu (`page_size`) arasındaki
/// orana göre "thumb" (kaydırma tutacağı) boyutu otomatik hesaplanır.
/// Thumb sürüklenebilir; ok düğmeleri 1 birim, iz alanı `page_size` kadar kaydırır.
pub struct ScrollBar {
    rect: Rect,
    orientation: Orientation,
    /// Mevcut kaydırma konumu (0 ≤ value ≤ max_value - page_size)
    value: usize,
    /// Toplam içerik boyutu (satır, piksel veya öğe sayısı)
    max_value: usize,
    /// Tek seferde görünen içerik miktarı; thumb boyutunu belirler
    page_size: usize,
    /// Thumb'ın sürükleniyor olup olmadığı
    dragging: bool,
    /// Sürükleme başladığında fare ile thumb başlangıcı arasındaki fark
    drag_start: i32,
    /// Farenin widget üzerinde olup olmadığı (hover vurgusu için)
    hovered: bool,
    /// Değer değiştiğinde çağrılacak isteğe bağlı işlev
    on_change: Option<fn(usize)>,
}

impl ScrollBar {
    /// Yeni bir kaydırma çubuğu oluşturur.
    /// Varsayılan değerler: max=100, page=10, değer=0.
    pub fn new(x: i32, y: i32, width: i32, height: i32, orientation: Orientation) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            orientation,
            value: 0,
            max_value: 100,
            page_size: 10,
            dragging: false,
            drag_start: 0,
            hovered: false,
            on_change: None,
        }
    }

    /// İçerik boyutunu ve görünür alan boyutunu ayarlar.
    pub fn with_range(mut self, max: usize, page: usize) -> Self {
        self.max_value = max;
        self.page_size = page;
        self
    }

    /// Kaydırma konumunu ayarlar.
    /// Değer, `max_value - page_size` üst sınırıyla kırpılır.
    pub fn set_value(&mut self, value: usize) {
        self.value = value.min(self.max_value.saturating_sub(self.page_size));
    }

    /// Mevcut kaydırma konumunu döndürür.
    pub fn value(&self) -> usize {
        self.value
    }

    /// Değer değiştiğinde çağrılacak işlevi ayarlar.
    pub fn with_change_handler(mut self, handler: fn(usize)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// Thumb'ın piksel cinsinden boyutunu hesaplar.
    ///
    /// Formül: `track_size * page_size / max_value`
    /// Sonuç en az 20 piksel, en fazla `track_size` pikseldir.
    /// İçerik yoksa (max=0) thumb iz boyutunu doldurur.
    fn thumb_size(&self) -> i32 {
        let (track_size, content_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.max_value),
            Orientation::Vertical => (self.rect.height, self.max_value),
        };

        if content_size == 0 {
            return track_size;
        }

        let thumb = (track_size as usize * self.page_size / content_size).max(20) as i32;
        thumb.min(track_size)
    }

    /// Thumb'ın iz üzerindeki piksel konumunu hesaplar.
    ///
    /// Formül: `track_range * value / max_scroll`
    /// `track_range = track_size - thumb_size` kullanılarak
    /// thumb ekran dışına çıkmaz.
    fn thumb_position(&self) -> i32 {
        let (track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.thumb_size()),
            Orientation::Vertical => (self.rect.height, self.thumb_size()),
        };

        let track_range = track_size - thumb_size;
        if self.max_value <= self.page_size {
            return 0;
        }

        let max_scroll = self.max_value - self.page_size;
        if max_scroll == 0 {
            return 0;
        }

        (track_range as usize * self.value / max_scroll) as i32
    }

    /// Thumb'ın ekrandaki dikdörtgen alanını döndürür.
    /// Yönüne göre ya X koordinatı (yatay) ya da Y koordinatı (dikey) kaydırılır.
    fn thumb_rect(&self) -> Rect {
        let thumb_size = self.thumb_size();
        let thumb_pos = self.thumb_position();

        match self.orientation {
            Orientation::Horizontal => Rect::new(
                self.rect.x + thumb_pos,
                self.rect.y,
                thumb_size,
                self.rect.height,
            ),
            Orientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + thumb_pos,
                self.rect.width,
                thumb_size,
            ),
        }
    }

    /// Pikseldeki thumb konumundan içerik kaydırma değerini tersine hesaplar.
    /// `pos / track_range * max_scroll` formülünü kullanır.
    fn value_from_position(&self, pos: i32) -> usize {
        let (track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.thumb_size()),
            Orientation::Vertical => (self.rect.height, self.thumb_size()),
        };

        let track_range = (track_size - thumb_size) as usize;
        if track_range == 0 {
            return 0;
        }

        let max_scroll = self.max_value.saturating_sub(self.page_size);
        let relative_pos = (pos as usize).min(track_range);
        relative_pos * max_scroll / track_range
    }
}

impl Widget for ScrollBar {
    /// Kaydırma çubuğunu çizer.
    /// Sırasıyla: iz arka planı → thumb (sürükleme/hover durumuna göre renkli) → ok düğmeleri.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // İz arka planı — gri dolgu alanı
        fb.draw_rect(x, y, w, h, Theme::BUTTON_BG.to_u32());

        // Thumb rengi: sürükleniyorsa aksent, üstündeyse hover, normalde soluk
        let thumb = self.thumb_rect();
        let thumb_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::TEXT_SECONDARY.to_u32()
        };

        fb.draw_rect(
            thumb.x as usize,
            thumb.y as usize,
            thumb.width as usize,
            thumb.height as usize,
            thumb_color,
        );

        // Ok düğmeleri — yöne göre "<"/">" veya "^"/"v" karakterleri
        match self.orientation {
            Orientation::Horizontal => {
                // Sol ok düğmesi
                fb.draw_rect(x, y, 16, h, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + 4, y + (h - 16) / 2, "<", Theme::TEXT_PRIMARY.to_u32());
                // Sağ ok düğmesi
                fb.draw_rect(x + w - 16, y, 16, h, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + w - 12, y + (h - 16) / 2, ">", Theme::TEXT_PRIMARY.to_u32());
            }
            Orientation::Vertical => {
                // Yukarı ok düğmesi
                fb.draw_rect(x, y, w, 16, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + (w - 8) / 2, y + 2, "^", Theme::TEXT_PRIMARY.to_u32());
                // Aşağı ok düğmesi
                fb.draw_rect(x, y + h - 16, w, 16, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + (w - 8) / 2, y + h - 14, "v", Theme::TEXT_PRIMARY.to_u32());
            }
        }
    }

    /// Tıklama olayını işler.
    /// Öncelik sırası: thumb tıklaması → ok düğmeleri → iz tıklaması (sayfa kaydırma).
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            self.dragging = false;
            return false;
        }

        let thumb = self.thumb_rect();

        // Thumb'a tıklandıysa sürükleme modunu başlat
        if thumb.contains(x, y) {
            self.dragging = true;
            match self.orientation {
                Orientation::Horizontal => self.drag_start = x - thumb.x,
                Orientation::Vertical => self.drag_start = y - thumb.y,
            }
            return true;
        }

        // Ok düğmelerine veya iz alanına tıklandığında kaydır
        match self.orientation {
            Orientation::Horizontal => {
                if x < self.rect.x + 16 {
                    // Sol ok — bir birim geri kaydır
                    if self.value > 0 {
                        self.value -= 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else if x >= self.rect.x + self.rect.width - 16 {
                    // Sağ ok — bir birim ileri kaydır
                    if self.value < self.max_value.saturating_sub(self.page_size) {
                        self.value += 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else {
                    // İz alanı tıklaması — thumb'ın soluna/sağına göre bir sayfa kaydır
                    let thumb_center = self.thumb_position() + self.thumb_size() / 2;
                    let click_pos = x - self.rect.x;
                    if click_pos < thumb_center {
                        self.value = self.value.saturating_sub(self.page_size);
                    } else {
                        self.value = (self.value + self.page_size).min(
                            self.max_value.saturating_sub(self.page_size)
                        );
                    }
                    if let Some(handler) = self.on_change {
                        handler(self.value);
                    }
                }
            }
            Orientation::Vertical => {
                if y < self.rect.y + 16 {
                    // Yukarı ok — bir birim geri kaydır
                    if self.value > 0 {
                        self.value -= 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else if y >= self.rect.y + self.rect.height - 16 {
                    // Aşağı ok — bir birim ileri kaydır
                    if self.value < self.max_value.saturating_sub(self.page_size) {
                        self.value += 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else {
                    // İz alanı tıklaması — thumb'ın üstüne/altına göre sayfa kaydır
                    let thumb_center = self.thumb_position() + self.thumb_size() / 2;
                    let click_pos = y - self.rect.y;
                    if click_pos < thumb_center {
                        self.value = self.value.saturating_sub(self.page_size);
                    } else {
                        self.value = (self.value + self.page_size).min(
                            self.max_value.saturating_sub(self.page_size)
                        );
                    }
                    if let Some(handler) = self.on_change {
                        handler(self.value);
                    }
                }
            }
        }
        true
    }

    /// Sürükleme olayını işler.
    /// Thumb'ın farenin delta hareketi kadar yeni konumu hesaplanır,
    /// ardından `value_from_position` ile içerik değerine dönüştürülür.
    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if !self.dragging {
            return false;
        }

        let (delta, track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => {
                let thumb = self.thumb_rect();
                let new_pos = (thumb.x + dx - self.rect.x).max(0);
                (new_pos, self.rect.width, self.thumb_size())
            }
            Orientation::Vertical => {
                let thumb = self.thumb_rect();
                let new_pos = (thumb.y + dy - self.rect.y).max(0);
                (new_pos, self.rect.height, self.thumb_size())
            }
        };

        let new_value = self.value_from_position(delta);
        if new_value != self.value {
            self.value = new_value;
            if let Some(handler) = self.on_change {
                handler(self.value);
            }
        }
        true
    }

    /// Hover durumunu günceller; değişiklik olduğunda true döner (yeniden çizim için).
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// Değer seçici kaydırıcı widget'ı.
///
/// Yatay bir iz üzerinde sürüklenebilir bir thumb ile `min_value`–`max_value`
/// aralığında tam sayı değerleri seçmeye yarar.
/// `step` parametresiyle değerler belirli adımlara yuvarlanır (snap).
pub struct Slider {
    rect: Rect,
    value: i32,
    min_value: i32,
    max_value: i32,
    /// Değerin hangi adım büyüklüğüne yuvarlanacağı (0 = serbest)
    step: i32,
    dragging: bool,
    hovered: bool,
    on_change: Option<fn(i32)>,
}

impl Slider {
    /// Varsayılan ayarlarla yeni bir kaydırıcı oluşturur.
    /// Aralık: 0–100, adım: 1, değer: 0.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            value: 0,
            min_value: 0,
            max_value: 100,
            step: 1,
            dragging: false,
            hovered: false,
            on_change: None,
        }
    }

    /// Değer aralığını ayarlar; mevcut değer yeni aralığa kırpılır.
    pub fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min_value = min;
        self.max_value = max;
        self.value = self.value.max(min).min(max);
        self
    }

    /// Adım büyüklüğünü ayarlar.
    /// Değer değiştiğinde en yakın `step` katına yuvarlanır.
    pub fn with_step(mut self, step: i32) -> Self {
        self.step = step;
        self
    }

    /// Değeri doğrudan ayarlar; `min_value`–`max_value` aralığına kırpılır.
    pub fn set_value(&mut self, value: i32) {
        self.value = value.max(self.min_value).min(self.max_value);
    }

    /// Mevcut değeri döndürür.
    pub fn value(&self) -> i32 {
        self.value
    }

    /// Değer değiştiğinde çağrılacak işlevi ayarlar.
    pub fn with_change_handler(mut self, handler: fn(i32)) -> Self {
        self.on_change = Some(handler);
        self
    }

    /// Mevcut değere karşılık gelen thumb piksel konumunu hesaplar.
    ///
    /// İz, soldan ve sağdan 10 piksel iç boşlukla (`track_width = width - 20`) çizilir.
    /// `(value - min) / range * track_width` formülü kullanılır.
    fn thumb_position(&self) -> i32 {
        let range = self.max_value - self.min_value;
        if range == 0 {
            return 0;
        }
        let track_width = self.rect.width - 20; // 10px padding each side
        (track_width as i64 * (self.value - self.min_value) as i64 / range as i64) as i32
    }

    /// Verilen X piksel konumundan kaydırıcı değerini tersine hesaplar.
    /// Sonuç `step` değerine yuvarlanır (snap); ardından aralığa kırpılır.
    fn value_from_position(&self, x: i32) -> i32 {
        let track_width = self.rect.width - 20;
        let relative_x = (x - self.rect.x - 10).max(0);
        let range = self.max_value - self.min_value;

        let value = self.min_value + (range as i64 * relative_x as i64 / track_width as i64) as i32;

        // Snap to step
        if self.step > 0 {
            ((value - self.min_value + self.step / 2) / self.step) * self.step + self.min_value
        } else {
            value
        }.max(self.min_value).min(self.max_value)
    }
}

impl Widget for Slider {
    /// Kaydırıcıyı çizer.
    /// Sırasıyla: gri iz → aksent renkli dolum bölümü → thumb dikdörtgeni → değer etiketi.
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        // İz dikey olarak ortalanır; yüksekliği 8 pikseldir
        let track_y = y + h / 2 - 4;

        // Gri iz arka planı — 10 px iç boşlukla başlar
        fb.draw_rect(x + 10, track_y, w - 20, 8, Theme::BUTTON_BG.to_u32());

        // Soldan thumb'a kadar dolumu aksent rengiyle göster
        let thumb_x = x + 10 + self.thumb_position() as usize;
        fb.draw_rect(x + 10, track_y, thumb_x - x - 10, 8, Theme::ACCENT_PRIMARY.to_u32());

        // Thumb dikdörtgeni — sürükleniyorsa aksent, üstündeyse hover rengi
        let thumb_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };

        fb.draw_rect(thumb_x - 8, y + 2, 16, h as usize - 4, thumb_color);

        // Sağ kenarında mevcut değerin metin etiketi
        let value_str = alloc::format!("{}", self.value);
        let label_x = x + w - value_str.len() * 8 - 5;
        fb.draw_string(label_x, y + (h - 16) / 2, &value_str, Theme::TEXT_SECONDARY.to_u32());
    }

    /// Tıklama olayını işler.
    /// Widget sınırları içine tıklandıysa sürükleme başlatılır ve
    /// tıklanan X pozisyonundan hemen yeni değer hesaplanır.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.dragging = true;
            let new_value = self.value_from_position(x);
            if new_value != self.value {
                self.value = new_value;
                if let Some(handler) = self.on_change {
                    handler(self.value);
                }
            }
            true
        } else {
            false
        }
    }

    /// Sürükleme olayını işler.
    /// Thumb'ın mevcut konumuna delta eklenerek yeni değer hesaplanır.
    fn on_drag(&mut self, dx: i32, _dy: i32) -> bool {
        if !self.dragging {
            return false;
        }

        // Recalculate value from current thumb position + delta
        let thumb_x = self.rect.x + 10 + self.thumb_position();
        let new_x = thumb_x + dx;
        let new_value = self.value_from_position(new_x);

        if new_value != self.value {
            self.value = new_value;
            if let Some(handler) = self.on_change {
                handler(self.value);
            }
        }
        true
    }

    /// Hover durumunu günceller; değişiklik olduğunda true döner.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    /// Widget sınırlarını döndürür.
    fn bounds(&self) -> Rect {
        self.rect
    }
}
