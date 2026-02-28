//! # echOS Sistem Tepsisi
//!
//! Görev çubuğu için sistem tepsisi simgeleri: saat, ağ, ses ve pil.
//!
//! ## Simge Düzeni
//!
//! ```text
//!  Görev çubuğu sağ kısmı:
//!  ┌──────────────────────────────────────────┐
//!  │  ... │ [Ağ] │ [Ses] │ [Pil] │  HH:MM   │
//!  └──────────────────────────────────────────┘
//!    ◄── simgeler soldan sağa icon_x() ile sıralanır
//!
//!  Her simgenin genişliği:
//!    Saat → 60px (saat:dakika metni için geniş)
//!    Diğer → 28px (simge resmi)
//! ```
//!
//! ## Pil Simgesi Tasviri
//!
//! ```text
//!  ┌──────────────┐ │   ← ana gövde (18×12)
//!  │  ████████░░░ │ │   ← doluluğu gösteren dolgu
//!  └──────────────┘     ← sağ uç başlık (2×6)
//!
//!  Renk:
//!    Şarj oluyor → yeşil (ACCENT_SUCCESS)
//!    %20 altı    → kırmızı (ACCENT_ERROR)
//!    Normal      → beyaz (TEXT_PRIMARY)
//! ```

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Rect, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// Tepsi simgesi durumu ve meta verisi
#[derive(Clone, Debug)]
pub struct TrayIcon {
    /// Simge kimliği (tıklama olaylarında kullanılır)
    pub id: u32,
    /// Simge türü
    pub icon: TrayIconType,
    /// Araç ipucu metni (hover'da gösterilir)
    pub tooltip: String,
    /// Simge aktif mi
    pub active: bool,
}

/// Sistem tepsisi simge türleri
#[derive(Clone, Copy, Debug)]
pub enum TrayIconType {
    /// Saat ve tarih göstergesi
    Clock,
    /// Ağ/Wi-Fi bağlantı durumu
    Network,
    /// Ses seviyesi
    Volume,
    /// Pil durumu
    Battery,
    /// Özel simge (örn. üçüncü parti uygulama)
    Custom(u8),
}

impl TrayIcon {
    pub fn new(id: u32, icon: TrayIconType) -> Self {
        Self {
            id,
            icon,
            tooltip: String::new(),
            active: true,
        }
    }

    pub fn with_tooltip(mut self, tooltip: &str) -> Self {
        self.tooltip = String::from(tooltip);
        self
    }
}

/// Sistem Tepsisi widget'ı.
/// Saat, ağ, ses ve pil gibi sistem durumu bilgilerini görev çubuğunda görüntüler.
pub struct SystemTray {
    /// Tepsi alanının sınırları
    rect: Rect,
    /// Tepsi simgeleri listesi
    icons: Vec<TrayIcon>,
    /// Üzerine gelinen simge indeksi
    hovered_index: Option<usize>,
    /// Geçerli saat (saat)
    time_hours: u8,
    /// Geçerli dakika
    time_minutes: u8,
    /// Geçerli saniye
    time_seconds: u8,
    /// Gün numarası
    date_day: u8,
    /// Ay numarası (1-12)
    date_month: u8,
    /// Ağa bağlı mı
    network_connected: bool,
    /// Ses seviyesi (0-100)
    volume_level: u8,
    /// Pil yüzdesi (0-100)
    battery_percent: u8,
    /// Şarj oluyor mu
    battery_charging: bool,
    /// Simgeye tıklandığında çağrılır (simge ID'si ile)
    on_click: Option<fn(u32)>,
}

impl SystemTray {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            icons: Vec::new(),
            hovered_index: None,
            time_hours: 0,
            time_minutes: 0,
            time_seconds: 0,
            date_day: 1,
            date_month: 1,
            network_connected: false,
            volume_level: 75,
            battery_percent: 100,
            battery_charging: false,
            on_click: None,
        }
    }

    /// Tepsi simgesi ekle
    pub fn add_icon(&mut self, icon: TrayIcon) {
        self.icons.push(icon);
    }

    /// Saati güncelle (saat, dakika, saniye)
    pub fn set_time(&mut self, hours: u8, minutes: u8, seconds: u8) {
        self.time_hours = hours;
        self.time_minutes = minutes;
        self.time_seconds = seconds;
    }

    /// Tarihi güncelle (gün, ay)
    pub fn set_date(&mut self, day: u8, month: u8) {
        self.date_day = day;
        self.date_month = month;
    }

    /// Ağ bağlantı durumunu güncelle
    pub fn set_network(&mut self, connected: bool) {
        self.network_connected = connected;
    }

    /// Ses seviyesini güncelle (0-100)
    pub fn set_volume(&mut self, level: u8) {
        self.volume_level = level;
    }

    /// Pil durumunu güncelle (yüzde ve şarj durumu)
    pub fn set_battery(&mut self, percent: u8, charging: bool) {
        self.battery_percent = percent;
        self.battery_charging = charging;
    }

    /// Tıklama geri çağırımını ayarla
    pub fn with_click_handler(mut self, handler: fn(u32)) -> Self {
        self.on_click = Some(handler);
        self
    }

    /// Belirtilen simgenin piksel genişliğini döndür
    fn icon_width(&self, index: usize) -> i32 {
        match self.icons.get(index).map(|i| i.icon) {
            Some(TrayIconType::Clock) => 60, // Saat metni için daha geniş alan
            _ => 28,
        }
    }

    /// Belirtilen simgenin X koordinatını hesapla (önceki simgelerin genişliklerini toplar)
    fn icon_x(&self, index: usize) -> i32 {
        let mut x = self.rect.x;
        for i in 0..index {
            x += self.icon_width(i) + 4; // 4px simge arası boşluk
        }
        x
    }

    /// Verilen X koordinatında hangi simge olduğunu bul
    fn icon_at(&self, click_x: i32) -> Option<usize> {
        for i in 0..self.icons.len() {
            let icon_x = self.icon_x(i);
            let icon_w = self.icon_width(i);
            if click_x >= icon_x && click_x < icon_x + icon_w {
                return Some(i);
            }
        }
        None
    }

    /// Saati "HH:MM" formatında metin olarak döndür
    fn format_time(&self) -> String {
        alloc::format!("{:02}:{:02}", self.time_hours, self.time_minutes)
    }

    /// Tarihi "GG Ay" formatında döndür (örn. "28 Feb")
    fn format_date(&self) -> String {
        let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                      "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let month_str = months.get(self.date_month.saturating_sub(1) as usize).unwrap_or(&"???");
        alloc::format!("{} {}", self.date_day, month_str)
    }
}

impl Widget for SystemTray {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let h = self.rect.height as usize;

        // Her simgeyi çiz
        for (i, icon) in self.icons.iter().enumerate() {
            let icon_x = self.icon_x(i) as usize;
            let icon_w = self.icon_width(i) as usize;

            // Hover arka planı
            if self.hovered_index == Some(i) {
                fb.draw_rect(icon_x, y, icon_w, h, Theme::BUTTON_HOVER.to_u32());
            }

            match icon.icon {
                TrayIconType::Clock => {
                    // Saat metni gösterimi
                    let time_str = self.format_time();
                    fb.draw_string(icon_x + 5, y + (h - 16) / 2, &time_str, Theme::TEXT_PRIMARY.to_u32());
                }
                TrayIconType::Network => {
                    // Ağ simgesi: bağlıysa yeşil, değilse kırmızı
                    let color = if self.network_connected {
                        Theme::ACCENT_SUCCESS.to_u32()
                    } else {
                        Theme::ACCENT_ERROR.to_u32()
                    };
                    // Basit Wi-Fi dalgası (4 yatay çubuk, yukarıdan aşağı küçülür)
                    fb.draw_rect(icon_x + 4, y + 8, 20, 3, color);
                    fb.draw_rect(icon_x + 7, y + 12, 14, 3, color);
                    fb.draw_rect(icon_x + 10, y + 16, 8, 3, color);
                    fb.draw_rect(icon_x + 13, y + 20, 2, 4, color);
                }
                TrayIconType::Volume => {
                    // Ses simgesi: M = sessiz, V = sesli
                    let vol_text = if self.volume_level == 0 { "M" } else { "V" };
                    fb.draw_string(icon_x + 6, y + (h - 16) / 2, vol_text, Theme::TEXT_PRIMARY.to_u32());
                }
                TrayIconType::Battery => {
                    // Pil rengi: şarj → yeşil, düşük pil → kırmızı, normal → beyaz
                    let color = if self.battery_charging {
                        Theme::ACCENT_SUCCESS.to_u32()
                    } else if self.battery_percent < 20 {
                        Theme::ACCENT_ERROR.to_u32()
                    } else {
                        Theme::TEXT_PRIMARY.to_u32()
                    };

                    // Pil gövdesi
                    fb.draw_rect(icon_x + 4, y + 8, 18, 12, color);
                    // Pil başlığı (sağ uçtaki küçük çıkıntı)
                    fb.draw_rect(icon_x + 22, y + 11, 2, 6, color);

                    // Doluluk çubuğu (yüzdeye göre genişlik)
                    let fill_width = (16 * self.battery_percent as usize / 100) as usize;
                    fb.draw_rect(icon_x + 5, y + 9, fill_width, 10, color);
                }
                TrayIconType::Custom(code) => {
                    // Özel simge yer tutucu (kod numarasını gösterir)
                    let text = alloc::format!("{}", code);
                    fb.draw_string(icon_x + 6, y + (h - 16) / 2, &text, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    fn on_click(&mut self, x: i32, _y: i32) -> bool {
        if let Some(index) = self.icon_at(x) {
            let icon_id = self.icons[index].id;
            if let Some(handler) = self.on_click {
                handler(icon_id);
            }
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.icon_at(x)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}
