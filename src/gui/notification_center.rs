//! # Notification Center
//!
//! macOS-style notification center with widgets
//! Shows notifications, calendar, weather, and custom widgets

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::sinf;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// NOTIFICATION
// ============================================================================

/// A notification
#[derive(Clone, Debug)]
pub struct Notification {
    /// Unique ID
    pub id: u32,
    /// Title
    pub title: String,
    /// Subtitle
    pub subtitle: String,
    /// Body text
    pub body: String,
    /// App name
    pub app_name: String,
    /// Icon
    pub icon: NotificationIcon,
    /// Timestamp
    pub timestamp: u64,
    /// Priority
    pub priority: NotificationPriority,
    /// Is read
    pub read: bool,
    /// Action
    pub action: NotificationAction,
    /// Has buttons
    pub has_buttons: bool,
    /// Button labels
    pub buttons: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationIcon {
    App,
    Message,
    Mail,
    Calendar,
    Alert,
    Download,
    Update,
    Custom(u16),
}

#[derive(Clone, Debug)]
pub enum NotificationAction {
    None,
    OpenApp(String),
    OpenUrl(String),
    Dismiss,
    Custom(String),
}

impl Notification {
    pub fn new(id: u32, title: &str, body: &str, app: &str) -> Self {
        Notification {
            id,
            title: String::from(title),
            subtitle: String::new(),
            body: String::from(body),
            app_name: String::from(app),
            icon: NotificationIcon::App,
            timestamp: 0,
            priority: NotificationPriority::Normal,
            read: false,
            action: NotificationAction::None,
            has_buttons: false,
            buttons: Vec::new(),
        }
    }
    
    pub fn message(id: u32, from: &str, text: &str) -> Self {
        Notification {
            id,
            title: String::from(from),
            subtitle: String::from("Message"),
            body: String::from(text),
            app_name: String::from("Messages"),
            icon: NotificationIcon::Message,
            timestamp: 0,
            priority: NotificationPriority::Normal,
            read: false,
            action: NotificationAction::OpenApp(String::from("messages")),
            has_buttons: true,
            buttons: vec![String::from("Reply"), String::from("Dismiss")],
        }
    }
    
    pub fn mail(id: u32, from: &str, subject: &str) -> Self {
        Notification {
            id,
            title: String::from(from),
            subtitle: String::from("Email"),
            body: String::from(subject),
            app_name: String::from("Mail"),
            icon: NotificationIcon::Mail,
            timestamp: 0,
            priority: NotificationPriority::Normal,
            read: false,
            action: NotificationAction::OpenApp(String::from("mail")),
            has_buttons: false,
            buttons: Vec::new(),
        }
    }
    
    pub fn calendar(id: u32, event: &str, time: &str) -> Self {
        Notification {
            id,
            title: String::from(event),
            subtitle: String::from(time),
            body: String::new(),
            app_name: String::from("Calendar"),
            icon: NotificationIcon::Calendar,
            timestamp: 0,
            priority: NotificationPriority::High,
            read: false,
            action: NotificationAction::OpenApp(String::from("calendar")),
            has_buttons: true,
            buttons: vec![String::from("Snooze"), String::from("Dismiss")],
        }
    }
    
    pub fn alert(id: u32, title: &str, message: &str) -> Self {
        Notification {
            id,
            title: String::from(title),
            subtitle: String::new(),
            body: String::from(message),
            app_name: String::from("System"),
            icon: NotificationIcon::Alert,
            timestamp: 0,
            priority: NotificationPriority::Critical,
            read: false,
            action: NotificationAction::None,
            has_buttons: true,
            buttons: vec![String::from("OK")],
        }
    }
}

// ============================================================================
// WIDGET
// ============================================================================

/// Widget trait
pub trait Widget: Send + Sync {
    fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize);
    fn height(&self) -> usize;
    fn on_click(&mut self, mx: i32, my: i32) -> WidgetAction;
    fn update(&mut self, dt: f32);
    fn title(&self) -> &str;
}

#[derive(Clone, Debug)]
pub enum WidgetAction {
    None,
    OpenApp(String),
    OpenSetting(String),
}

// ============================================================================
// CALENDAR WIDGET
// ============================================================================

pub struct CalendarWidget {
    title: String,
    current_month: u8,
    current_year: u32,
    current_day: u8,
    events: Vec<CalendarEvent>,
    selected_day: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct CalendarEvent {
    day: u8,
    title: String,
    time: String,
}

impl CalendarWidget {
    pub fn new() -> Self {
        CalendarWidget {
            title: String::from("Calendar"),
            current_month: 1,
            current_year: 2024,
            current_day: 1,
            events: Vec::new(),
            selected_day: None,
        }
    }
    
    fn days_in_month(&self, month: u8, year: u32) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 },
            _ => 30,
        }
    }
    
    fn day_of_week(&self, day: u8, month: u8, year: u32) -> u8 {
        // Simplified Zeller's congruence
        let m = if month < 3 { month + 12 } else { month };
        let y = if month < 3 { year - 1 } else { year };
        
        let h = (day as u32 + (13 * (m as u32 + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
        ((h + 5) % 7) as u8 // 0 = Monday, 6 = Sunday
    }
    
    fn month_name(&self) -> &'static str {
        match self.current_month {
            1 => "January", 2 => "February", 3 => "March", 4 => "April",
            5 => "May", 6 => "June", 7 => "July", 8 => "August",
            9 => "September", 10 => "October", 11 => "November", 12 => "December",
            _ => "Unknown",
        }
    }
}

impl Widget for CalendarWidget {
    fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        // Header
        fb.draw_rect(x, y, width, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 6, &format!("{} {}", self.month_name(), self.current_year), Theme::TEXT_PRIMARY.to_u32());
        
        // Day headers
        let days = ["M", "T", "W", "T", "F", "S", "S"];
        let cell_w = width / 7;
        let header_y = y + 32;
        
        for (i, &day) in days.iter().enumerate() {
            let day_x = x + i * cell_w + cell_w / 2 - 4;
            fb.draw_string(day_x, header_y, day, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Calendar grid
        let days_in_month = self.days_in_month(self.current_month, self.current_year);
        let start_day = self.day_of_week(1, self.current_month, self.current_year) as usize;
        
        let grid_y = header_y + 20;
        let cell_h = 20;
        
        for day in 1..=days_in_month {
            let pos = start_day + (day - 1) as usize;
            let row = pos / 7;
            let col = pos % 7;
            
            let cell_x = x + col * cell_w;
            let cell_y = grid_y + row * cell_h;
            
            let is_today = day == self.current_day;
            let is_selected = Some(day) == self.selected_day;
            let has_event = self.events.iter().any(|e| e.day == day);
            
            // Background
            if is_today {
                fb.draw_rect(cell_x, cell_y, cell_w - 2, cell_h - 2, Theme::ACCENT_PRIMARY.to_u32());
            } else if is_selected {
                fb.draw_rect(cell_x, cell_y, cell_w - 2, cell_h - 2, Theme::LIST_ITEM_HOVER.to_u32());
            }
            
            // Day number
            let text_color = if is_today {
                Theme::TEXT_ON_ACCENT.to_u32()
            } else if has_event {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            
            let day_str = format!("{}", day);
            fb.draw_string(cell_x + cell_w / 2 - 4, cell_y + 2, &day_str, text_color);
            
            // Event dot
            if has_event {
                fb.draw_rect(cell_x + cell_w / 2 - 2, cell_y + cell_h - 6, 4, 4, Theme::ACCENT_PRIMARY.to_u32());
            }
        }
    }
    
    fn height(&self) -> usize {
        160
    }
    
    fn on_click(&mut self, mx: i32, my: i32) -> WidgetAction {
        // Check if clicking on a day
        let grid_y = 52;
        let cell_h = 20;
        
        if my >= grid_y as i32 {
            let row = ((my - grid_y as i32) / cell_h as i32) as usize;
            let col = (mx as usize / 35).min(6);
            let pos = row * 7 + col;
            
            let start_day = self.day_of_week(1, self.current_month, self.current_year) as usize;
            if pos >= start_day {
                let day = (pos - start_day + 1) as u8;
                if day <= self.days_in_month(self.current_month, self.current_year) {
                    self.selected_day = Some(day);
                }
            }
        }
        
        WidgetAction::None
    }
    
    fn update(&mut self, _dt: f32) {}
    
    fn title(&self) -> &str {
        &self.title
    }
}

// ============================================================================
// WEATHER WIDGET
// ============================================================================

pub struct WeatherWidget {
    title: String,
    location: String,
    temperature: i32,
    condition: WeatherCondition,
    high: i32,
    low: i32,
    forecast: Vec<ForecastDay>,
}

#[derive(Clone, Copy, Debug)]
pub enum WeatherCondition {
    Sunny,
    PartlyCloudy,
    Cloudy,
    Rain,
    Storm,
    Snow,
}

#[derive(Clone, Debug)]
pub struct ForecastDay {
    day: String,
    high: i32,
    low: i32,
    condition: WeatherCondition,
}

impl WeatherWidget {
    pub fn new() -> Self {
        WeatherWidget {
            title: String::from("Weather"),
            location: String::from("Istanbul"),
            temperature: 22,
            condition: WeatherCondition::PartlyCloudy,
            high: 25,
            low: 18,
            forecast: vec![
                ForecastDay { day: String::from("Mon"), high: 25, low: 18, condition: WeatherCondition::Sunny },
                ForecastDay { day: String::from("Tue"), high: 24, low: 17, condition: WeatherCondition::PartlyCloudy },
                ForecastDay { day: String::from("Wed"), high: 22, low: 16, condition: WeatherCondition::Rain },
                ForecastDay { day: String::from("Thu"), high: 23, low: 17, condition: WeatherCondition::Cloudy },
                ForecastDay { day: String::from("Fri"), high: 26, low: 19, condition: WeatherCondition::Sunny },
            ],
        }
    }
    
    fn condition_icon(&self, condition: WeatherCondition) -> &'static str {
        match condition {
            WeatherCondition::Sunny => "☀",
            WeatherCondition::PartlyCloudy => "⛅",
            WeatherCondition::Cloudy => "☁",
            WeatherCondition::Rain => "🌧",
            WeatherCondition::Storm => "⛈",
            WeatherCondition::Snow => "❄",
        }
    }
}

impl Widget for WeatherWidget {
    fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        // Header with location
        fb.draw_rect(x, y, width, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 6, &self.location, Theme::TEXT_PRIMARY.to_u32());
        
        // Current weather
        let current_y = y + 32;
        let icon = self.condition_icon(self.condition);
        fb.draw_string(x + 16, current_y, icon, 0xFFFFFFFF);
        
        let temp_str = format!("{}°", self.temperature);
        fb.draw_string(x + 60, current_y, &temp_str, Theme::TEXT_PRIMARY.to_u32());
        
        let range = format!("H:{}° L:{}°", self.high, self.low);
        fb.draw_string(x + 60, current_y + 16, &range, Theme::TEXT_SECONDARY.to_u32());
        
        // Forecast
        let forecast_y = current_y + 48;
        let day_w = width / 5;
        
        for (i, day) in self.forecast.iter().enumerate() {
            let day_x = x + i * day_w;
            
            fb.draw_string(day_x + 4, forecast_y, &day.day, Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(day_x + 4, forecast_y + 16, self.condition_icon(day.condition), 0xFFFFFFFF);
            fb.draw_string(day_x + 4, forecast_y + 36, &format!("{}°", day.high), Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(day_x + 4, forecast_y + 52, &format!("{}°", day.low), Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn height(&self) -> usize {
        140
    }
    
    fn on_click(&mut self, _mx: i32, _my: i32) -> WidgetAction {
        WidgetAction::OpenApp(String::from("weather"))
    }
    
    fn update(&mut self, _dt: f32) {}
    
    fn title(&self) -> &str {
        &self.title
    }
}

// ============================================================================
// QUICK NOTES WIDGET
// ============================================================================

pub struct QuickNotesWidget {
    title: String,
    notes: Vec<String>,
    selected: Option<usize>,
}

impl QuickNotesWidget {
    pub fn new() -> Self {
        QuickNotesWidget {
            title: String::from("Notes"),
            notes: vec![
                String::from("Review GUI components"),
                String::from("Test notification system"),
                String::from("Update documentation"),
            ],
            selected: None,
        }
    }
}

impl Widget for QuickNotesWidget {
    fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 6, &self.title, Theme::TEXT_PRIMARY.to_u32());
        
        let note_y = y + 32;
        for (i, note) in self.notes.iter().enumerate() {
            let is_selected = Some(i) == self.selected;
            let bg = if is_selected { Theme::LIST_ITEM_HOVER.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(x, note_y + i * 24, width, 24, bg);
            
            // Bullet
            fb.draw_string(x + 8, note_y + i * 24 + 4, "•", Theme::TEXT_SECONDARY.to_u32());
            
            // Note text (truncated)
            let text = if note.len() > 25 { format!("{}...", &note[..22]) } else { note.clone() };
            fb.draw_string(x + 20, note_y + i * 24 + 4, &text, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn height(&self) -> usize {
        32 + self.notes.len() * 24
    }
    
    fn on_click(&mut self, _mx: i32, my: i32) -> WidgetAction {
        let note_y = 32;
        let idx = ((my - note_y) / 24) as usize;
        
        if idx < self.notes.len() {
            self.selected = Some(idx);
        }
        
        WidgetAction::OpenApp(String::from("notes"))
    }
    
    fn update(&mut self, _dt: f32) {}
    
    fn title(&self) -> &str {
        &self.title
    }
}

// ============================================================================
// SYSTEM STATUS WIDGET
// ============================================================================

pub struct SystemStatusWidget {
    title: String,
    cpu_usage: f32,
    memory_usage: f32,
    disk_usage: f32,
    network_up: f32,
    network_down: f32,
    uptime: u64,
}

impl SystemStatusWidget {
    pub fn new() -> Self {
        SystemStatusWidget {
            title: String::from("System"),
            cpu_usage: 25.0,
            memory_usage: 45.0,
            disk_usage: 32.0,
            network_up: 0.0,
            network_down: 0.0,
            uptime: 0,
        }
    }
}

impl Widget for SystemStatusWidget {
    fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 6, &self.title, Theme::TEXT_PRIMARY.to_u32());
        
        let stat_y = y + 32;
        let bar_width = width - 80;
        
        // CPU
        fb.draw_string(x + 8, stat_y, "CPU", Theme::TEXT_SECONDARY.to_u32());
        fb.draw_rect(x + 48, stat_y, bar_width, 12, Theme::BORDER.to_u32());
        fb.draw_rect(x + 48, stat_y, (bar_width as f32 * self.cpu_usage / 100.0) as usize, 12, Theme::ACCENT_PRIMARY.to_u32());
        fb.draw_string(x + 52 + bar_width, stat_y, &format!("{:.0}%", self.cpu_usage), Theme::TEXT_PRIMARY.to_u32());
        
        // Memory
        fb.draw_string(x + 8, stat_y + 20, "RAM", Theme::TEXT_SECONDARY.to_u32());
        fb.draw_rect(x + 48, stat_y + 20, bar_width, 12, Theme::BORDER.to_u32());
        fb.draw_rect(x + 48, stat_y + 20, (bar_width as f32 * self.memory_usage / 100.0) as usize, 12, 0xFF34C759);
        fb.draw_string(x + 52 + bar_width, stat_y + 20, &format!("{:.0}%", self.memory_usage), Theme::TEXT_PRIMARY.to_u32());
        
        // Disk
        fb.draw_string(x + 8, stat_y + 40, "Disk", Theme::TEXT_SECONDARY.to_u32());
        fb.draw_rect(x + 48, stat_y + 40, bar_width, 12, Theme::BORDER.to_u32());
        fb.draw_rect(x + 48, stat_y + 40, (bar_width as f32 * self.disk_usage / 100.0) as usize, 12, 0xFFFF9500);
        fb.draw_string(x + 52 + bar_width, stat_y + 40, &format!("{:.0}%", self.disk_usage), Theme::TEXT_PRIMARY.to_u32());
        
        // Uptime
        let hours = self.uptime / 3600;
        let mins = (self.uptime % 3600) / 60;
        fb.draw_string(x + 8, stat_y + 60, &format!("Uptime: {}h {}m", hours, mins), Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn height(&self) -> usize {
        120
    }
    
    fn on_click(&mut self, _mx: i32, _my: i32) -> WidgetAction {
        WidgetAction::OpenSetting(String::from("System"))
    }
    
    fn update(&mut self, dt: f32) {
        self.uptime += dt as u64;
        // Simulate fluctuating values
        self.cpu_usage = 20.0 + 10.0 * sinf(self.uptime as f32 / 10.0);
    }
    
    fn title(&self) -> &str {
        &self.title
    }
}

// ============================================================================
// NOTIFICATION CENTER
// ============================================================================

pub struct NotificationCenter {
    /// Is visible
    pub visible: bool,
    /// Notifications
    pub notifications: Vec<Notification>,
    /// Widgets
    pub widgets: Vec<Box<dyn Widget>>,
    /// Animation progress
    pub animation_progress: f32,
    /// Screen width
    pub screen_width: usize,
    /// Screen height
    pub screen_height: usize,
    /// Panel width
    pub panel_width: usize,
    /// Scroll offset
    pub scroll_offset: usize,
    /// Next notification ID
    pub next_id: u32,
    /// Selected notification
    pub selected_notification: Option<usize>,
}

impl NotificationCenter {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut center = NotificationCenter {
            visible: false,
            notifications: Vec::new(),
            widgets: Vec::new(),
            animation_progress: 0.0,
            screen_width,
            screen_height,
            panel_width: 350,
            scroll_offset: 0,
            next_id: 1,
            selected_notification: None,
        };
        
        center.add_default_widgets();
        center.add_test_notifications();
        center
    }
    
    fn add_default_widgets(&mut self) {
        self.widgets.push(Box::new(CalendarWidget::new()));
        self.widgets.push(Box::new(WeatherWidget::new()));
        self.widgets.push(Box::new(QuickNotesWidget::new()));
        self.widgets.push(Box::new(SystemStatusWidget::new()));
    }
    
    fn add_test_notifications(&mut self) {
        self.notifications.push(Notification::message(
            self.next_id, "John Doe", "Hey, how's the GUI coming along?"
        ));
        self.next_id += 1;
        
        self.notifications.push(Notification::mail(
            self.next_id, "Newsletter", "Weekly Tech Digest"
        ));
        self.next_id += 1;
        
        self.notifications.push(Notification::calendar(
            self.next_id, "Team Meeting", "In 30 minutes"
        ));
        self.next_id += 1;
    }
    
    /// Show notification center
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
    }
    
    /// Hide notification center
    pub fn hide(&mut self) {
        self.visible = false;
    }
    
    /// Toggle visibility
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }
    
    /// Add notification
    pub fn add_notification(&mut self, notification: Notification) {
        self.notifications.insert(0, notification);
    }
    
    /// Remove notification
    pub fn remove_notification(&mut self, id: u32) {
        self.notifications.retain(|n| n.id != id);
    }
    
    /// Clear all notifications
    pub fn clear_all(&mut self) {
        self.notifications.clear();
    }
    
    /// Update
    pub fn update(&mut self, dt: f32) {
        // Update animation
        if self.visible && self.animation_progress < 1.0 {
            self.animation_progress = (self.animation_progress + dt * 6.0).min(1.0);
        } else if !self.visible && self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 6.0).max(0.0);
        }
        
        // Update widgets
        for widget in &mut self.widgets {
            widget.update(dt);
        }
    }
    
    /// Draw notification center
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }
        
        let progress = self.animation_progress;
        let panel_x = (self.screen_width - self.panel_width) as f32 * progress;
        let panel_x = panel_x as usize;
        
        // Dim background
        let bg_alpha = 0.3 * progress;
        for y in 0..self.screen_height {
            for x in 0..(self.screen_width - self.panel_width) {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha as f32);
                unsafe { *ptr = dimmed; }
            }
        }
        
        // Panel background
        fb.draw_rect(panel_x, 0, self.panel_width, self.screen_height, Theme::WINDOW_BG.to_u32());
        
        // Header
        fb.draw_rect(panel_x, 0, self.panel_width, 40, Theme::TOOLBAR_BG.to_u32());
        fb.draw_string(panel_x + 12, 8, "Notification Center", Theme::TEXT_PRIMARY.to_u32());
        
        // Clear button
        fb.draw_string(panel_x + self.panel_width - 60, 8, "Clear All", Theme::ACCENT_PRIMARY.to_u32());
        
        let mut y = 48;
        
        // Draw notifications
        if !self.notifications.is_empty() {
            fb.draw_string(panel_x + 12, y, "Notifications", Theme::TEXT_SECONDARY.to_u32());
            y += 24;
            
            for (i, notif) in self.notifications.iter().enumerate() {
                let notif_height = 80;
                let is_selected = Some(i) == self.selected_notification;
                
                // Notification background
                let bg = if is_selected { Theme::LIST_ITEM_HOVER.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
                fb.draw_rect(panel_x + 8, y, self.panel_width - 16, notif_height, bg);
                
                // Icon
                let icon = self.get_notification_icon(notif.icon);
                fb.draw_string(panel_x + 16, y + 8, icon, Theme::TEXT_PRIMARY.to_u32());
                
                // App name
                fb.draw_string(panel_x + 48, y + 8, &notif.app_name, Theme::TEXT_SECONDARY.to_u32());
                
                // Title
                fb.draw_string(panel_x + 48, y + 24, &notif.title, Theme::TEXT_PRIMARY.to_u32());
                
                // Body
                let body = if notif.body.len() > 35 { format!("{}...", &notif.body[..32]) } else { notif.body.clone() };
                fb.draw_string(panel_x + 48, y + 40, &body, Theme::TEXT_SECONDARY.to_u32());
                
                // Buttons
                if notif.has_buttons {
                    let mut btn_x = panel_x + 48;
                    for btn in &notif.buttons {
                        fb.draw_rect(btn_x, y + 56, btn.len() * 8 + 16, 20, Theme::BORDER.to_u32());
                        fb.draw_string(btn_x + 8, y + 58, btn, Theme::TEXT_PRIMARY.to_u32());
                        btn_x += btn.len() * 8 + 24;
                    }
                }
                
                y += notif_height + 8;
            }
            
            y += 8;
        }
        
        // Draw widgets
        fb.draw_string(panel_x + 12, y, "Widgets", Theme::TEXT_SECONDARY.to_u32());
        y += 24;
        
        for widget in &self.widgets {
            // Widget container
            let widget_height = widget.height();
            fb.draw_rect(panel_x + 8, y, self.panel_width - 16, widget_height, Theme::SIDEBAR_BG.to_u32());
            
            widget.draw(fb, panel_x + 8, y, self.panel_width - 16);
            
            y += widget_height + 8;
        }
    }
    
    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;
        
        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;
        
        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;
        
        (r << 16) | (g << 8) | b
    }
    
    fn get_notification_icon(&self, icon: NotificationIcon) -> &'static str {
        match icon {
            NotificationIcon::App => "📱",
            NotificationIcon::Message => "💬",
            NotificationIcon::Mail => "📧",
            NotificationIcon::Calendar => "📅",
            NotificationIcon::Alert => "⚠",
            NotificationIcon::Download => "⬇",
            NotificationIcon::Update => "🔄",
            NotificationIcon::Custom(_) => "📌",
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> NotificationEvent {
        let panel_x = self.screen_width - self.panel_width;
        
        // Clear all button
        if mx >= panel_x as i32 + self.panel_width as i32 - 60 && mx < panel_x as i32 + self.panel_width as i32 - 8
            && my >= 8 && my < 24 {
            self.clear_all();
            return NotificationEvent::ClearedAll;
        }
        
        // Check notifications
        let mut y = 72;
        for (i, notif) in self.notifications.iter().enumerate() {
            let notif_height = 80;
            if my >= y && my < y + notif_height as i32 {
                self.selected_notification = Some(i);
                
                // Check buttons
                if notif.has_buttons {
                    let mut btn_x = panel_x as i32 + 48;
                    for btn in &notif.buttons {
                        if mx >= btn_x && mx < btn_x + btn.len() as i32 * 8 + 16 {
                            return NotificationEvent::ButtonClicked(notif.id, btn.clone());
                        }
                        btn_x += btn.len() as i32 * 8 + 24;
                    }
                }
                
                return NotificationEvent::NotificationSelected(notif.id);
            }
            y += notif_height as i32 + 8;
        }
        
        // Check widgets
        y += 32;
        for widget in &mut self.widgets {
            let widget_height = widget.height() as i32;
            if my >= y && my < y + widget_height {
                let local_mx = mx - panel_x as i32 - 8;
                let local_my = my - y;
                let action = widget.on_click(local_mx, local_my);
                return NotificationEvent::WidgetAction(action);
            }
            y += widget_height + 8;
        }
        
        NotificationEvent::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
}

/// Notification events
#[derive(Clone, Debug)]
pub enum NotificationEvent {
    None,
    NotificationSelected(u32),
    ButtonClicked(u32, String),
    ClearedAll,
    WidgetAction(WidgetAction),
}

// ============================================================================
// GLOBAL NOTIFICATION CENTER
// ============================================================================

lazy_static::lazy_static! {
    static ref NOTIFICATION_CENTER: Mutex<NotificationCenter> = Mutex::new(NotificationCenter::new(1920, 1080));
}

/// Initialize notification center
pub fn init(width: usize, height: usize) {
    let mut center = NOTIFICATION_CENTER.lock();
    center.resize(width, height);
    crate::serial_println!("[GUI] Notification Center initialized");
}

/// Get notification center
pub fn get_center() -> &'static Mutex<NotificationCenter> {
    &NOTIFICATION_CENTER
}
