//! # echOS Checkbox ve RadioButton Widget'ları
//!
//! Boolean (evet/hayır) seçim bileşenleri.
//!
//! ## CheckBox vs RadioButton
//!
//! `CheckBox`: Bağımsız açma/kapama (toggle) kontrolü. Her checkbox birbirinden
//! bağımsız çalışır; birden fazlası aynı anda seçili olabilir.
//!
//! `RadioButton`: Grup içinde tek seçim (mutually exclusive) kontrolü. Bir gruptaki
//! radio butonlardan yalnızca biri seçili olabilir; birini seçmek diğerlerini kaldırır.
//! `RadioGroup` struct'ı bu mantığı yönetir.
//!
//! ## Trigonometri: no_std Ortamında Sin/Cos Yaklaşımı
//!
//! Standart kütüphane olmayan (no_std) çekirdek ortamında `f64::sin()` gibi
//! matematik fonksiyonları kullanılamaz. Taylor serisi açılımı ile yaklaşık
//! sinüs ve kosinüs değerleri hesaplanır. Bu yöntem ilk birkaç terimle bile
//! iyi bir doğruluk sağlar.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

/// no_std ortamı için Taylor serisi tabanlı sinüs yaklaşımı.
///
/// Formül: sin(x) = x - x³/3! + x⁵/5! - x⁷/7! + ...
/// `x % (2π)` ile önce açıyı [0, 2π) aralığına normalize ederiz.
/// `term` değişkeni her adımda bir sonraki terimi verimli hesaplamak için
/// önceki terme göre çarpım faktörü uygulanarak güncellenir.
/// 7 iterasyon çoğu grafik uygulaması için yeterli hassasiyeti sağlar.
fn sin_approx(x: f64) -> f64 {
    // Taylor series approximation
    let x = x % (2.0 * core::f64::consts::PI);
    let mut result = 0.0;
    let mut term = x;
    for i in 1..=7 {
        result += term;
        term *= -x * x / ((2.0 * i as f64) * (2.0 * i as f64 + 1.0));
    }
    result
}

/// no_std ortamı için kosinüs yaklaşımı.
///
/// cos(x) = sin(x + π/2) trigonometrik özdeşliğini kullanır.
/// Ayrı bir Taylor serisi yazmak yerine var olan `sin_approx` fonksiyonunu
/// faz kaydırmasıyla yeniden kullanır; bu kod tekrarını önler.
fn cos_approx(x: f64) -> f64 {
    sin_approx(x + core::f64::consts::PI / 2.0)
}

/// Onay kutusu (checkbox) widget'ı; bağımsız boolean toggle kontrolü.
///
/// `on_toggle` alanı isteğe bağlı bir callback fonksiyon işaretçisidir.
/// `Option<fn(bool)>` türü: `None` ise handler yok, `Some(f)` ise toggle
/// olduğunda `f(yeni_durum)` çağrılır. Fonksiyon işaretçisi kullanmak
/// closure yerine heap allocation gerektirmez (no_std uyumlu).
pub struct CheckBox {
    rect: Rect,
    label: String,
    checked: bool,
    hovered: bool,
    on_toggle: Option<fn(bool)>,
}

impl CheckBox {
    /// Yeni checkbox oluşturur; başlangıçta işaretsiz durumdadır.
    ///
    /// Sabit boyut (200x24 piksel) kullanılır. Gerçek uygulamalarda
    /// metin uzunluğuna göre dinamik genişlik hesaplanabilir.
    pub fn new(x: i32, y: i32, label: &str) -> Self {
        Self {
            rect: Rect::new(x, y, 200, 24),
            label: String::from(label),
            checked: false,
            hovered: false,
            on_toggle: None,
        }
    }

    /// Toggle handler'ı zincir yöntemiyle (builder pattern) ekler.
    ///
    /// Builder pattern: `mut self` alıp `Self` döndürmek, oluşturma sırasında
    /// zincirleme yapılandırma sağlar: `CheckBox::new(...).with_toggle_handler(f)`
    pub fn with_toggle_handler(mut self, handler: fn(bool)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    /// Mevcut durumu döndürür.
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// Durumu doğrudan ayarlar; callback tetiklenmez.
    ///
    /// Programatik güncelleme için kullanılır. Kullanıcı tıklamasından farklı
    /// olarak callback çağrılmaz; bu sayede sonsuz döngü riski olmadan
    /// dışarıdan durum senkronizasyonu yapılabilir.
    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
    }

    /// Durumu tersine çevirir ve varsa callback'i tetikler.
    ///
    /// `if let Some(handler) = self.on_toggle` deseni: Option içindeki değeri
    /// güvenle açar. `on_toggle` `None` ise if let bloğu çalışmaz, panic olmaz.
    pub fn toggle(&mut self) {
        self.checked = !self.checked;
        if let Some(handler) = self.on_toggle {
            handler(self.checked);
        }
    }
}

impl Widget for CheckBox {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let box_size = 18usize;

        // Checkbox kutusu: hover durumunda farklı arka plan rengi
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, box_size, box_size, bg_color);

        // Kenarlık: seçili ise vurgu rengi, değilse normal kenarlık rengi.
        // Bu görsel geri bildirim kullanıcıya kutunun seçili olduğunu gösterir.
        let border_color = if self.checked {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };

        for col in x..(x + box_size) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + box_size - 1, border_color);
        }
        for row in y..(y + box_size) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + box_size - 1, row, border_color);
        }

        // Tik işareti (checkmark): X şeklinde çizilir.
        // İki köşegen çizgisi örtüşerek bir X deseni oluşturur.
        // Gerçek bir tik (v-şekli) için çizgi noktalarını farklı hesaplamak gerekir.
        if self.checked {
            let check_color = Theme::ACCENT_PRIMARY.to_u32();
            // Basit X deseni: iki köşegen yönünde 6 piksel çizilir
            for i in 0..6 {
                fb.plot_pixel(x + 4 + i, y + 4 + i, check_color);
                fb.plot_pixel(x + 4 + i, y + 12 - i, check_color);
            }
        }

        // Etiket metni: kutunun sağına 8 piksel boşlukla yerleştirilir.
        // Dikey ortalama: box_size=18, metin yüksekliği~16; (18-16)/2 = 1 olduğundan
        // +3 piksel offset yeterli görsel hizalamayı sağlar.
        fb.draw_string(x + box_size + 8, y + 3, &self.label, Theme::TEXT_PRIMARY.to_u32());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.toggle();
            true
        } else {
            false
        }
    }

    /// Hover durumunu günceller ve durum değiştiyse true döndürür.
    ///
    /// `was_hovered != self.hovered` karşılaştırması: yalnızca durum geçişlerinde
    /// (hover başladı veya bitti) true döner; sürekli hover'da gereksiz
    /// yeniden çizimden kaçınılır.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// Radyo düğmesi widget'ı; grup içinde tek seçim kontrolü.
///
/// `group_id` aynı gruptaki butonları ilişkilendirir. `RadioGroup` struct'ı
/// bu id'yi kullanarak hangi butonun seçildiğini ve diğerlerinin seçimini
/// kaldırması gerektiğini belirler.
pub struct RadioButton {
    rect: Rect,
    label: String,
    selected: bool,
    hovered: bool,
    group_id: u32,
    on_select: Option<fn(u32)>,
}

impl RadioButton {
    /// Yeni radyo düğmesi oluşturur.
    ///
    /// `group_id` parametresi bu butonu bir gruba atar. Aynı group_id'ye sahip
    /// tüm butonlar birbirini dışlayan bir seçim grubu oluşturur.
    pub fn new(x: i32, y: i32, label: &str, group_id: u32) -> Self {
        Self {
            rect: Rect::new(x, y, 200, 24),
            label: String::from(label),
            selected: false,
            hovered: false,
            group_id,
            on_select: None,
        }
    }

    /// Seçim handler'ını builder pattern ile ekler.
    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    /// Seçili durumu döndürür.
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Seçili durumu doğrudan ayarlar; callback tetiklenmez.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    /// Bu butonun ait olduğu grup kimliğini döndürür.
    pub fn group_id(&self) -> u32 {
        self.group_id
    }

    /// Butonu seçer ve varsa callback'i tetikler.
    fn select(&mut self) {
        self.selected = true;
        if let Some(handler) = self.on_select {
            handler(self.group_id);
        }
    }
}

impl Widget for RadioButton {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let circle_size = 18usize;
        let center = circle_size / 2;

        // Radyo çemberi arka planı
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, circle_size, circle_size, bg_color);

        // Çember çerçevesi: trigonometri ile çizilir.
        // 0°-360° aralığında her açı için koordinat hesaplanır.
        // Yarıçap 8 piksel; `f64 as usize` dönüşümü kesme (floor) yapar.
        let border_color = if self.selected {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };

        // 1 derecelik adımlarla çember çizimi (yaklaşık trigonometri kullanır)
        for angle in 0..360 {
            let rad = (angle as f64) * core::f64::consts::PI / 180.0;
            let px = (center as f64 + 8.0 * cos_approx(rad)) as usize;
            let py = (center as f64 + 8.0 * sin_approx(rad)) as usize;
            if px < circle_size && py < circle_size {
                fb.plot_pixel(x + px, y + py, border_color);
            }
        }

        // Seçili ise iç dolu çember: yarıçap 4 piksel ile daha küçük bir
        // çember çizilir; bu klasik radyo butonu görünümünü verir.
        if self.selected {
            let fill_color = Theme::ACCENT_PRIMARY.to_u32();
            for angle in 0..360 {
                let rad = (angle as f64) * core::f64::consts::PI / 180.0;
                let px = (center as f64 + 4.0 * cos_approx(rad)) as usize;
                let py = (center as f64 + 4.0 * sin_approx(rad)) as usize;
                if px < circle_size && py < circle_size {
                    fb.plot_pixel(x + px, y + py, fill_color);
                }
            }
        }

        // Etiket metni: çemberin sağına yerleştirilir
        fb.draw_string(x + circle_size + 8, y + 3, &self.label, Theme::TEXT_PRIMARY.to_u32());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.select();
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// Radyo düğmesi grubu; grup içinde tek seçim mantığını yönetir.
///
/// `selected_index` hangi butonun seçili olduğunu saklar. `select(i)` çağrıldığında
/// önce tüm butonlar temizlenir (for döngüsü), ardından yalnızca istenen butona
/// `set_selected(true)` uygulanır. Bu "deselect all, select one" kalıbı
/// radyo butonlarının temel mantığıdır.
pub struct RadioGroup {
    buttons: Vec<RadioButton>,
    selected_index: Option<usize>,
}

impl RadioGroup {
    /// Boş radyo düğmesi grubu oluşturur.
    pub fn new() -> Self {
        Self {
            buttons: Vec::new(),
            selected_index: None,
        }
    }

    /// Gruba yeni bir radyo düğmesi ekler.
    pub fn add_button(&mut self, button: RadioButton) {
        self.buttons.push(button);
    }

    /// Belirtilen indeksteki butonu seçer; diğerlerinin seçimini kaldırır.
    pub fn select(&mut self, index: usize) {
        if index < self.buttons.len() {
            // Önce tüm butonların seçimini kaldır
            for btn in &mut self.buttons {
                btn.set_selected(false);
            }
            // Yalnızca istenen butonu seç
            self.buttons[index].set_selected(true);
            self.selected_index = Some(index);
        }
    }

    /// Seçili butonun indeksini döndürür; henüz seçim yapılmadıysa None.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Tüm butonlara salt okunur referans döndürür.
    pub fn buttons(&self) -> &Vec<RadioButton> {
        &self.buttons
    }

    /// Tüm butonlara değiştirilebilir referans döndürür.
    pub fn buttons_mut(&mut self) -> &mut Vec<RadioButton> {
        &mut self.buttons
    }
}

impl Default for RadioGroup {
    fn default() -> Self {
        Self::new()
    }
}
