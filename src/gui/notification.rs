//! # echOS Bildirim Sistemi
//!
//! Durum bildirimleri (toast notifications) ve uyarılar.
//! Bildirimler ekranın sağ alt köşesinde gösterilir ve otomatik olarak kapanır.
//!
//! ## Mimari
//! - `Notification`: Başlık, mesaj, tür, öncelik ve zaman aşımı içeren bildirim verisi
//! - `NotificationPopup`: Tek bir bildirimi görüntüleyen widget; sol kenar rengi ve eylem butonu içerir
//! - `NotificationManager`: Birden fazla bildirimi kuyruk halinde yöneten yönetici
//!
//! ## Çizim Algoritması
//! Bildirim; sol kenar vurgu çubuğu (türe özgü renk), gölge, kenarlık,
//! başlık metni, kelime kaydırmalı mesaj ve isteğe bağlı eylem butonu içerir.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Rect, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// Bildirim önceliği
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationPriority {
    Low,    // Düşük
    Normal, // Normal
    High,   // Yüksek
    Urgent, // Acil
}

/// Bildirim türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationType {
    Info,    // Bilgi
    Success, // Başarı
    Warning, // Uyarı
    Error,   // Hata
}

/// Bildirim verisi
#[derive(Clone)]
pub struct Notification {
    pub id: u32,
    pub title: String,
    pub message: String,
    pub notification_type: NotificationType,
    pub priority: NotificationPriority,
    pub timeout_ms: u32,     // Otomatik kapanma süresi (milisaniye)
    pub elapsed_ms: u32,     // Geçen süre
    pub action_text: Option<String>, // Eylem butonu metni
    pub on_action: Option<u32>, // Eylem kimliği
}

impl Notification {
    pub fn new(id: u32, title: &str, message: &str) -> Self {
        Self {
            id,
            title: String::from(title),
            message: String::from(message),
            notification_type: NotificationType::Info,
            priority: NotificationPriority::Normal,
            timeout_ms: 5000,  // Varsayılan: 5 saniye sonra otomatik kapat
            elapsed_ms: 0,
            action_text: None,
            on_action: None,
        }
    }

    /// Bildirim türünü ayarlar (builder deseni — zincirleme çağrı destekler).
    pub fn with_type(mut self, notification_type: NotificationType) -> Self {
        self.notification_type = notification_type;
        self
    }

    /// Bildirim önceliğini ayarlar.
    /// Urgent öncelikli bildirimler en üstte tutulabilir.
    pub fn with_priority(mut self, priority: NotificationPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Otomatik kapanma süresini (ms) ayarlar. 0 verilirse hiç kapanmaz.
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Bildirime eylem butonu ekler.
    /// `text`: Buton etiketi, `action_id`: Tıklandığında geri çağrıma iletilen kimlik.
    pub fn with_action(mut self, text: &str, action_id: u32) -> Self {
        self.action_text = Some(String::from(text));
        self.on_action = Some(action_id);
        self
    }

    /// Bildirimin süresi dolmuş mu kontrol eder.
    /// timeout_ms = 0 ise süresiz gösterilir (asla sona ermez).
    fn is_expired(&self) -> bool {
        self.timeout_ms > 0 && self.elapsed_ms >= self.timeout_ms
    }
}

/// Bildirim açılır penceresi (widget)
pub struct NotificationPopup {
    rect: Rect,
    notification: Option<Notification>,
    visible: bool,
    hovered: bool,             // Üzerine gelinmiş mi
    on_dismiss: Option<fn(u32)>,     // Kapatıldığında çağrılan geri çağrım
    on_action: Option<fn(u32, u32)>, // Eylem tıklandığında çağrılan geri çağrım
}

impl NotificationPopup {
    pub fn new(x: i32, y: i32, width: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, 80),
            notification: None,
            visible: false,
            hovered: false,
            on_dismiss: None,
            on_action: None,
        }
    }

    /// Belirtilen bildirimi gösterir ve popup'ı sağ alt köşeye konumlandırır.
    /// Ekran boyutları parametre olarak verilir; taşma engellenir.
    pub fn show(&mut self, notification: Notification, screen_width: usize, screen_height: usize) {
        // Sağ alt köşeye konumlandır
        self.rect.x = (screen_width as i32 - self.rect.width - 10).max(0);
        self.rect.y = (screen_height as i32 - self.rect.height - 50).max(0);
        self.notification = Some(notification);
        self.visible = true;
        self.hovered = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.notification = None;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn update(&mut self, delta_ms: u32) -> bool {
        if !self.visible || self.hovered {
            return false;
        }

        if let Some(ref mut notif) = self.notification {
            notif.elapsed_ms += delta_ms;
            if notif.is_expired() {
                if let Some(handler) = self.on_dismiss {
                    handler(notif.id);
                }
                self.hide();
                return true;
            }
        }
        false
    }

    pub fn with_dismiss_handler(mut self, handler: fn(u32)) -> Self {
        self.on_dismiss = Some(handler);
        self
    }

    pub fn with_action_handler(mut self, handler: fn(u32, u32)) -> Self {
        self.on_action = Some(handler);
        self
    }

    /// Bildirimin türüne karşılık gelen vurgu rengini döndürür.
    /// Bu renk sol kenar çubuğunda ve eylem butonunda kullanılır.
    fn type_color(&self) -> u32 {
        if let Some(ref notif) = self.notification {
            match notif.notification_type {
                NotificationType::Info => Theme::ACCENT_PRIMARY.to_u32(),
                NotificationType::Success => Theme::ACCENT_SUCCESS.to_u32(),
                NotificationType::Warning => Theme::ACCENT_WARNING.to_u32(),
                NotificationType::Error => Theme::ACCENT_ERROR.to_u32(),
            }
        } else {
            Theme::ACCENT_PRIMARY.to_u32()
        }
    }

    /// Eylem butonunun piksel dikdörtgenini hesaplar.
    /// Eğer bildirimde eylem butonu yoksa sıfır boyutlu boş bir Rect döner.
    fn action_rect(&self) -> Rect {
        if let Some(ref notif) = self.notification {
            if let Some(ref text) = notif.action_text {
                let text_width = (text.len() * 8 + 16) as i32;
                return Rect::new(
                    self.rect.x + self.rect.width - text_width - 10,
                    self.rect.y + self.rect.height - 28,
                    text_width,
                    22,
                );
            }
        }
        Rect::new(0, 0, 0, 0)
    }

    /// Kapatma (X) butonunun piksel dikdörtgenini döndürür.
    /// Sağ üst köşeye sabitlenmiş 15×15 piksellik bir alandır.
    fn close_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + self.rect.width - 20,
            self.rect.y + 5,
            15,
            15,
        )
    }
}

impl Widget for NotificationPopup {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Gölge
        fb.draw_rect(x + 4, y + 4, w, h, Theme::SHADOW.to_u32());

        // Arkaplan
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        // Sol kenar vurgu çubuğu (türe özgü renk)
        let accent_color = self.type_color();
        fb.draw_rect(x, y, 4, h, accent_color);

        // Kenarlık
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        if let Some(ref notif) = self.notification {
            // Başlık
            fb.draw_string(x + 12, y + 8, &notif.title, Theme::TEXT_PRIMARY.to_u32());

            // Mesaj (kelime kaydırma)
            let msg_y = y + 28;
            let max_width = w - 20;
            let mut line_y = msg_y;
            for line in notif.message.split('\n') {
                if line.len() * 8 > max_width {
                    let mut start = 0;
                    while start < line.len() {
                        let end = (start + max_width / 8).min(line.len());
                        fb.draw_string(x + 12, line_y, &line[start..end], Theme::TEXT_SECONDARY.to_u32());
                        line_y += 14;
                        start = end;
                    }
                } else {
                    fb.draw_string(x + 12, line_y, line, Theme::TEXT_SECONDARY.to_u32());
                    line_y += 14;
                }
            }

            // Eylem butonu
            if let Some(ref text) = notif.action_text {
                let action_rect = self.action_rect();
                fb.draw_rect(
                    action_rect.x as usize,
                    action_rect.y as usize,
                    action_rect.width as usize,
                    action_rect.height as usize,
                    accent_color,
                );
                fb.draw_string(
                    action_rect.x as usize + 8,
                    action_rect.y as usize + 3,
                    text,
                    Theme::TEXT_PRIMARY.to_u32(),
                );
            }

            // Kapatma butonu
            let close_rect = self.close_rect();
            fb.draw_rect(
                close_rect.x as usize,
                close_rect.y as usize,
                close_rect.width as usize,
                close_rect.height as usize,
                Theme::ACCENT_ERROR.to_u32(),
            );
            fb.draw_string(
                close_rect.x as usize + 4,
                close_rect.y as usize + 1,
                "X",
                Theme::TEXT_PRIMARY.to_u32(),
            );
        }
    }

    fn on_click(&mut self, click_x: i32, click_y: i32) -> bool {
        if !self.visible {
            return false;
        }

        // Kapatma butonu
        if self.close_rect().contains(click_x, click_y) {
            if let Some(ref notif) = self.notification {
                if let Some(handler) = self.on_dismiss {
                    handler(notif.id);
                }
            }
            self.hide();
            return true;
        }

        // Eylem butonu
        if let Some(ref notif) = self.notification {
            if self.action_rect().contains(click_x, click_y) {
                if let (Some(handler), Some(action_id)) = (self.on_action, notif.on_action) {
                    handler(notif.id, action_id);
                }
                self.hide();
                return true;
            }
        }

        self.rect.contains(click_x, click_y)
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

/// Bildirim yöneticisi (birden fazla bildirimi kuyruk halinde yönetir)
pub struct NotificationManager {
    notifications: Vec<Notification>,
    next_id: u32,              // Sonraki bildirim kimliği
    max_visible: usize,        // Aynı anda görünen maksimum bildirim sayısı
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            next_id: 1,
            max_visible: 3,
        }
    }

    /// Var olan bir `Notification` nesnesini kuyruğa ekler; kimliğini döner.
    pub fn push(&mut self, notification: Notification) -> u32 {
        let id = notification.id;
        self.notifications.push(notification);
        id
    }

    /// Yeni benzersiz kimlikli bir `Notification` taslağı oluşturur.
    /// Builder zinciriyle tür, öncelik ve süre eklenebilir; ardından `push` ile kuyruğa alınır.
    pub fn create(&mut self, title: &str, message: &str) -> Notification {
        let id = self.next_id;
        self.next_id += 1;
        Notification::new(id, title, message)
    }

    /// Kimliğe göre bildirimi kuyrudan kaldırır (kullanıcı kapattıysa vb.).
    pub fn dismiss(&mut self, id: u32) {
        self.notifications.retain(|n| n.id != id);
    }

    /// Tüm bildirimleri kuyruktan temizler.
    pub fn clear_all(&mut self) {
        self.notifications.clear();
    }

    /// Mevcut bildirim listesine salt-okunur erişim sağlar.
    pub fn notifications(&self) -> &Vec<Notification> {
        &self.notifications
    }

    /// Aynı anda gösterilecek bildirim sayısını döner (`max_visible` üst sınırıyla).
    pub fn visible_count(&self) -> usize {
        self.notifications.len().min(self.max_visible)
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}
