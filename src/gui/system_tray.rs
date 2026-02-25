//! # echOS System Tray
//!
//! System tray icons for taskbar (clock, network, volume, battery).

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Rect, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// Tray icon status
#[derive(Clone, Debug)]
pub struct TrayIcon {
    pub id: u32,
    pub icon: TrayIconType,
    pub tooltip: String,
    pub active: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum TrayIconType {
    Clock,
    Network,
    Volume,
    Battery,
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

/// System Tray widget
pub struct SystemTray {
    rect: Rect,
    icons: Vec<TrayIcon>,
    hovered_index: Option<usize>,
    time_hours: u8,
    time_minutes: u8,
    time_seconds: u8,
    date_day: u8,
    date_month: u8,
    network_connected: bool,
    volume_level: u8,
    battery_percent: u8,
    battery_charging: bool,
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

    pub fn add_icon(&mut self, icon: TrayIcon) {
        self.icons.push(icon);
    }

    pub fn set_time(&mut self, hours: u8, minutes: u8, seconds: u8) {
        self.time_hours = hours;
        self.time_minutes = minutes;
        self.time_seconds = seconds;
    }

    pub fn set_date(&mut self, day: u8, month: u8) {
        self.date_day = day;
        self.date_month = month;
    }

    pub fn set_network(&mut self, connected: bool) {
        self.network_connected = connected;
    }

    pub fn set_volume(&mut self, level: u8) {
        self.volume_level = level;
    }

    pub fn set_battery(&mut self, percent: u8, charging: bool) {
        self.battery_percent = percent;
        self.battery_charging = charging;
    }

    pub fn with_click_handler(mut self, handler: fn(u32)) -> Self {
        self.on_click = Some(handler);
        self
    }

    fn icon_width(&self, index: usize) -> i32 {
        match self.icons.get(index).map(|i| i.icon) {
            Some(TrayIconType::Clock) => 60,
            _ => 28,
        }
    }

    fn icon_x(&self, index: usize) -> i32 {
        let mut x = self.rect.x;
        for i in 0..index {
            x += self.icon_width(i) + 4;
        }
        x
    }

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

    fn format_time(&self) -> String {
        alloc::format!("{:02}:{:02}", self.time_hours, self.time_minutes)
    }

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

        // Draw each icon
        for (i, icon) in self.icons.iter().enumerate() {
            let icon_x = self.icon_x(i) as usize;
            let icon_w = self.icon_width(i) as usize;

            // Hover background
            if self.hovered_index == Some(i) {
                fb.draw_rect(icon_x, y, icon_w, h, Theme::BUTTON_HOVER.to_u32());
            }

            match icon.icon {
                TrayIconType::Clock => {
                    // Time display
                    let time_str = self.format_time();
                    fb.draw_string(icon_x + 5, y + (h - 16) / 2, &time_str, Theme::TEXT_PRIMARY.to_u32());
                }
                TrayIconType::Network => {
                    // Network icon
                    let color = if self.network_connected {
                        Theme::ACCENT_SUCCESS.to_u32()
                    } else {
                        Theme::ACCENT_ERROR.to_u32()
                    };
                    // Simple WiFi icon
                    fb.draw_rect(icon_x + 4, y + 8, 20, 3, color);
                    fb.draw_rect(icon_x + 7, y + 12, 14, 3, color);
                    fb.draw_rect(icon_x + 10, y + 16, 8, 3, color);
                    fb.draw_rect(icon_x + 13, y + 20, 2, 4, color);
                }
                TrayIconType::Volume => {
                    // Volume icon
                    let vol_text = if self.volume_level == 0 { "M" } else { "V" };
                    fb.draw_string(icon_x + 6, y + (h - 16) / 2, vol_text, Theme::TEXT_PRIMARY.to_u32());
                }
                TrayIconType::Battery => {
                    // Battery icon
                    let color = if self.battery_charging {
                        Theme::ACCENT_SUCCESS.to_u32()
                    } else if self.battery_percent < 20 {
                        Theme::ACCENT_ERROR.to_u32()
                    } else {
                        Theme::TEXT_PRIMARY.to_u32()
                    };
                    
                    // Battery outline
                    fb.draw_rect(icon_x + 4, y + 8, 18, 12, color);
                    fb.draw_rect(icon_x + 22, y + 11, 2, 6, color);
                    
                    // Fill level
                    let fill_width = (16 * self.battery_percent as usize / 100) as usize;
                    fb.draw_rect(icon_x + 5, y + 9, fill_width, 10, color);
                }
                TrayIconType::Custom(code) => {
                    // Custom icon placeholder
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
