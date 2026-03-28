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

use super::{
    border_rect_objects, draw_render_objects, raster_object, solid_rect_object,
    text_render_object_with_width, AccessRole, AccessState, AccessibilityInfo, Rect, Widget,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec;
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
    /// Devre dışı — tıklama yok sayılır, soluk renkte çizilir.
    enabled: bool,
    /// Belirsiz (indeterminate) — kısmen seçili, tire (dash) gösterir.
    indeterminate: bool,
    /// Odak durumu — klavye ile Space ile toggle.
    focused: bool,
}

impl CheckBox {
    /// Yeni checkbox oluşturur; başlangıçta işaretsiz durumdadır.
    pub fn new(x: i32, y: i32, label: &str) -> Self {
        Self {
            rect: Rect::new(x, y, 200, 24),
            label: String::from(label),
            checked: false,
            hovered: false,
            on_toggle: None,
            enabled: true,
            indeterminate: false,
            focused: false,
        }
    }

    /// Etkinlik durumunu ayarlar.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Belirsiz (indeterminate) durumunu ayarlar.
    pub fn set_indeterminate(&mut self, v: bool) {
        self.indeterminate = v;
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
    pub fn toggle(&mut self) {
        if !self.enabled {
            return;
        }
        self.indeterminate = false; // Toggle, belirsiz durumu temizler
        self.checked = !self.checked;
        if let Some(handler) = self.on_toggle {
            handler(self.checked);
        }
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let box_rect = Rect::new(self.rect.x, self.rect.y + 3, 18, 18);
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        let border_color = if self.focused || self.checked || self.indeterminate {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        let text_color = if self.enabled {
            Theme::TEXT_PRIMARY.to_u32()
        } else {
            Theme::TEXT_DISABLED.to_u32()
        };
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64);

        objects.push(solid_rect_object(
            base_id,
            box_rect,
            bg_color,
            DamageLane::Window,
            0,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x10,
            box_rect,
            border_color,
            DamageLane::Window,
            1,
        ));

        if self.checked {
            objects.push(solid_rect_object(
                base_id ^ 0x20,
                Rect::new(box_rect.x + 4, box_rect.y + 8, 4, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
            objects.push(solid_rect_object(
                base_id ^ 0x21,
                Rect::new(box_rect.x + 7, box_rect.y + 10, 2, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
            objects.push(solid_rect_object(
                base_id ^ 0x22,
                Rect::new(box_rect.x + 8, box_rect.y + 9, 2, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
            objects.push(solid_rect_object(
                base_id ^ 0x23,
                Rect::new(box_rect.x + 9, box_rect.y + 8, 2, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
            objects.push(solid_rect_object(
                base_id ^ 0x24,
                Rect::new(box_rect.x + 10, box_rect.y + 6, 2, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
            objects.push(solid_rect_object(
                base_id ^ 0x25,
                Rect::new(box_rect.x + 11, box_rect.y + 4, 2, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
        } else if self.indeterminate {
            objects.push(solid_rect_object(
                base_id ^ 0x30,
                Rect::new(box_rect.x + 4, box_rect.y + 8, 10, 2),
                Theme::ACCENT_PRIMARY.to_u32(),
                DamageLane::Window,
                2,
            ));
        }

        objects.push(text_render_object_with_width(
            base_id ^ 0x40,
            Rect::new(
                self.rect.x + 26,
                self.rect.y + 3,
                (self.rect.width - 26).max(1),
                18,
            ),
            &self.label,
            text_color,
            false,
            DamageLane::Text,
            3,
        ));
        objects
    }
}

impl Widget for CheckBox {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
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

    fn on_key(&mut self, key: char, _modifiers: u8, _scancode: u8) -> bool {
        if self.focused && (key == ' ' || key == '\n') {
            self.toggle();
            return true;
        }
        false
    }

    fn can_focus(&self) -> bool {
        self.enabled
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        if !self.enabled {
            state = state.with(AccessState::DISABLED);
        }
        if self.checked {
            state = state.with(AccessState::CHECKED);
        }
        AccessibilityInfo {
            role: AccessRole::Checkbox,
            label: &self.label,
            value: if self.indeterminate {
                "mixed"
            } else if self.checked {
                "checked"
            } else {
                "unchecked"
            },
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
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
    focused: bool,
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
            focused: false,
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

    fn render_primitives(&self) -> Vec<RenderObject> {
        let circle_size = 18usize;
        let center = circle_size / 2;
        let mut pixels = vec![0u32; circle_size * circle_size];
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        let border_color = if self.selected {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };

        for pixel in pixels.iter_mut() {
            *pixel = bg_color;
        }
        for angle in 0..360 {
            let rad = (angle as f64) * core::f64::consts::PI / 180.0;
            let px = (center as f64 + 8.0 * cos_approx(rad)) as usize;
            let py = (center as f64 + 8.0 * sin_approx(rad)) as usize;
            if px < circle_size && py < circle_size {
                pixels[py * circle_size + px] = border_color;
            }
        }
        if self.selected {
            let fill_color = Theme::ACCENT_PRIMARY.to_u32();
            for angle in 0..360 {
                let rad = (angle as f64) * core::f64::consts::PI / 180.0;
                let px = (center as f64 + 4.0 * cos_approx(rad)) as usize;
                let py = (center as f64 + 4.0 * sin_approx(rad)) as usize;
                if px < circle_size && py < circle_size {
                    pixels[py * circle_size + px] = fill_color;
                }
            }
        }

        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64);
        vec![
            raster_object(
                base_id,
                Rect::new(
                    self.rect.x,
                    self.rect.y + 3,
                    circle_size as i32,
                    circle_size as i32,
                ),
                pixels,
                DamageLane::Window,
                0,
            ),
            text_render_object_with_width(
                base_id ^ 0x10,
                Rect::new(
                    self.rect.x + circle_size as i32 + 8,
                    self.rect.y + 3,
                    (self.rect.width - circle_size as i32 - 8).max(1),
                    18,
                ),
                &self.label,
                Theme::TEXT_PRIMARY.to_u32(),
                false,
                DamageLane::Text,
                1,
            ),
        ]
    }
}

impl Widget for RadioButton {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
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

    fn on_key(&mut self, key: char, _modifiers: u8, _scancode: u8) -> bool {
        if self.focused && (key == ' ' || key == '\n') {
            self.select();
            return true;
        }
        false
    }

    fn can_focus(&self) -> bool {
        true
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        if self.selected {
            state = state.with(AccessState::SELECTED);
        }
        AccessibilityInfo {
            role: AccessRole::RadioButton,
            label: &self.label,
            value: if self.selected { "selected" } else { "clear" },
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
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
