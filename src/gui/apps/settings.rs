//! # Settings Application
//!
//! System settings with categories for display, sound, network, etc.
//! Modern UI with search and navigation

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};
use crate::gui::window::Window;

// ============================================================================
// SETTINGS CATEGORIES
// ============================================================================

/// Settings category
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingsCategory {
    System,
    Display,
    Personalization,
    Apps,
    Network,
    Sound,
    Storage,
    Privacy,
    Update,
    About,
}

impl SettingsCategory {
    pub fn name(&self) -> &'static str {
        match self {
            SettingsCategory::System => "System",
            SettingsCategory::Display => "Display",
            SettingsCategory::Personalization => "Personalization",
            SettingsCategory::Apps => "Apps",
            SettingsCategory::Network => "Network",
            SettingsCategory::Sound => "Sound",
            SettingsCategory::Storage => "Storage",
            SettingsCategory::Privacy => "Privacy",
            SettingsCategory::Update => "Update",
            SettingsCategory::About => "About",
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            SettingsCategory::System => "⚙",
            SettingsCategory::Display => "🖥",
            SettingsCategory::Personalization => "🎨",
            SettingsCategory::Apps => "📦",
            SettingsCategory::Network => "🌐",
            SettingsCategory::Sound => "🔊",
            SettingsCategory::Storage => "💾",
            SettingsCategory::Privacy => "🔒",
            SettingsCategory::Update => "⬆",
            SettingsCategory::About => "ℹ",
        }
    }
    
    pub fn all() -> &'static [SettingsCategory] {
        &[
            SettingsCategory::System,
            SettingsCategory::Display,
            SettingsCategory::Personalization,
            SettingsCategory::Apps,
            SettingsCategory::Network,
            SettingsCategory::Sound,
            SettingsCategory::Storage,
            SettingsCategory::Privacy,
            SettingsCategory::Update,
            SettingsCategory::About,
        ]
    }
}

// ============================================================================
// SETTINGS ITEM
// ============================================================================

/// A single settings item
pub struct SettingsItem {
    /// Item ID
    id: u32,
    /// Display name
    name: String,
    /// Description
    description: String,
    /// Item type
    item_type: SettingsItemType,
    /// Current value
    value: SettingsValue,
    /// Category
    category: SettingsCategory,
}

#[derive(Clone, Debug)]
pub enum SettingsItemType {
    Toggle,
    Slider { min: f32, max: f32, step: f32 },
    Dropdown { options: Vec<String> },
    Text,
    Color,
    Button,
    Info,
}

#[derive(Clone, Debug)]
pub enum SettingsValue {
    Bool(bool),
    Float(f32),
    Int(i32),
    String(String),
    Color(u32),
    None,
}

impl SettingsValue {
    pub fn as_bool(&self) -> bool {
        match self {
            SettingsValue::Bool(b) => *b,
            _ => false,
        }
    }
    
    pub fn as_float(&self) -> f32 {
        match self {
            SettingsValue::Float(f) => *f,
            SettingsValue::Int(i) => *i as f32,
            _ => 0.0,
        }
    }
    
    pub fn as_int(&self) -> i32 {
        match self {
            SettingsValue::Int(i) => *i,
            SettingsValue::Float(f) => *f as i32,
            _ => 0,
        }
    }
    
    pub fn as_string(&self) -> &str {
        match self {
            SettingsValue::String(s) => s,
            _ => "",
        }
    }
    
    pub fn as_color(&self) -> u32 {
        match self {
            SettingsValue::Color(c) => *c,
            _ => 0,
        }
    }
}

impl SettingsItem {
    pub fn toggle(id: u32, name: &str, description: &str, value: bool, category: SettingsCategory) -> Self {
        SettingsItem {
            id,
            name: String::from(name),
            description: String::from(description),
            item_type: SettingsItemType::Toggle,
            value: SettingsValue::Bool(value),
            category,
        }
    }
    
    pub fn slider(id: u32, name: &str, description: &str, value: f32, min: f32, max: f32, step: f32, category: SettingsCategory) -> Self {
        SettingsItem {
            id,
            name: String::from(name),
            description: String::from(description),
            item_type: SettingsItemType::Slider { min, max, step },
            value: SettingsValue::Float(value),
            category,
        }
    }
    
    pub fn dropdown(id: u32, name: &str, description: &str, value: &str, options: Vec<String>, category: SettingsCategory) -> Self {
        SettingsItem {
            id,
            name: String::from(name),
            description: String::from(description),
            item_type: SettingsItemType::Dropdown { options },
            value: SettingsValue::String(String::from(value)),
            category,
        }
    }
    
    pub fn info(id: u32, name: &str, description: &str, value: &str, category: SettingsCategory) -> Self {
        SettingsItem {
            id,
            name: String::from(name),
            description: String::from(description),
            item_type: SettingsItemType::Info,
            value: SettingsValue::String(String::from(value)),
            category,
        }
    }
    
    pub fn button(id: u32, name: &str, description: &str, category: SettingsCategory) -> Self {
        SettingsItem {
            id,
            name: String::from(name),
            description: String::from(description),
            item_type: SettingsItemType::Button,
            value: SettingsValue::None,
            category,
        }
    }
}

// ============================================================================
// SETTINGS PANEL
// ============================================================================

/// Settings panel for a category
pub struct SettingsPanel {
    category: SettingsCategory,
    items: Vec<SettingsItem>,
    scroll_offset: usize,
}

impl SettingsPanel {
    pub fn new(category: SettingsCategory) -> Self {
        let items = Self::create_items_for_category(category);
        SettingsPanel {
            category,
            items,
            scroll_offset: 0,
        }
    }
    
    fn create_items_for_category(category: SettingsCategory) -> Vec<SettingsItem> {
        let mut items = Vec::new();
        let mut id = (category as usize as u32) * 100;
        
        match category {
            SettingsCategory::System => {
                items.push(SettingsItem::info(id, "OS Version", "Current operating system version", "echOS v0.1.0", category));
                id += 1;
                items.push(SettingsItem::info(id, "Kernel", "Kernel version and build", "echOS Kernel x64", category));
                id += 1;
                items.push(SettingsItem::info(id, "Uptime", "Time since last boot", "0:00:00", category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Developer Mode", "Enable developer features", false, category));
            }
            
            SettingsCategory::Display => {
                items.push(SettingsItem::slider(id, "Brightness", "Screen brightness level", 100.0, 0.0, 100.0, 5.0, category));
                id += 1;
                items.push(SettingsItem::slider(id, "Scale", "Display scale factor", 100.0, 50.0, 200.0, 25.0, category));
                id += 1;
                items.push(SettingsItem::dropdown(id, "Resolution", "Screen resolution", "1920x1080", 
                    vec!["1280x720".into(), "1920x1080".into(), "2560x1440".into()], category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Night Mode", "Reduce blue light at night", false, category));
                id += 1;
                items.push(SettingsItem::slider(id, "Night Mode Temperature", "Color temperature (K)", 6500.0, 2700.0, 6500.0, 100.0, category));
            }
            
            SettingsCategory::Personalization => {
                items.push(SettingsItem::dropdown(id, "Theme", "Color theme", "Dark",
                    vec!["Light".into(), "Dark".into(), "Auto".into()], category));
                id += 1;
                items.push(SettingsItem::dropdown(id, "Accent Color", "Primary accent color", "Blue",
                    vec!["Blue".into(), "Purple".into(), "Green".into(), "Orange".into(), "Red".into()], category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Transparency", "Enable transparency effects", true, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Animations", "Enable UI animations", true, category));
                id += 1;
                items.push(SettingsItem::dropdown(id, "Desktop Background", "Background style", "Solid Color",
                    vec!["Solid Color".into(), "Gradient".into(), "Image".into()], category));
            }
            
            SettingsCategory::Apps => {
                items.push(SettingsItem::info(id, "Installed Apps", "Number of installed applications", "5", category));
                id += 1;
                items.push(SettingsItem::info(id, "Default Apps", "Default application settings", "Configure defaults", category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Auto-update Apps", "Automatically update applications", true, category));
            }
            
            SettingsCategory::Network => {
                items.push(SettingsItem::info(id, "Status", "Network connection status", "Connected", category));
                id += 1;
                items.push(SettingsItem::info(id, "IP Address", "Current IP address", "192.168.1.100", category));
                id += 1;
                items.push(SettingsItem::info(id, "MAC Address", "Hardware address", "00:00:00:00:00:00", category));
                id += 1;
                items.push(SettingsItem::toggle(id, "WiFi", "Enable wireless networking", true, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Airplane Mode", "Disable all wireless", false, category));
            }
            
            SettingsCategory::Sound => {
                items.push(SettingsItem::slider(id, "Master Volume", "Main volume level", 75.0, 0.0, 100.0, 1.0, category));
                id += 1;
                items.push(SettingsItem::slider(id, "System Sounds", "System event sounds", 50.0, 0.0, 100.0, 1.0, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Mute", "Mute all sounds", false, category));
            }
            
            SettingsCategory::Storage => {
                items.push(SettingsItem::info(id, "Total Storage", "Total disk capacity", "256 GB", category));
                id += 1;
                items.push(SettingsItem::info(id, "Used Storage", "Currently used space", "32 GB", category));
                id += 1;
                items.push(SettingsItem::info(id, "Available", "Free space", "224 GB", category));
                id += 1;
                items.push(SettingsItem::button(id, "Clear Cache", "Remove temporary files", category));
                id += 1;
                items.push(SettingsItem::button(id, "Disk Cleanup", "Remove unnecessary files", category));
            }
            
            SettingsCategory::Privacy => {
                items.push(SettingsItem::toggle(id, "Location Services", "Allow apps to access location", false, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Camera Access", "Allow apps to use camera", true, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Microphone Access", "Allow apps to use microphone", true, category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Diagnostic Data", "Send anonymous usage data", false, category));
            }
            
            SettingsCategory::Update => {
                items.push(SettingsItem::info(id, "Current Version", "Installed version", "0.1.0", category));
                id += 1;
                items.push(SettingsItem::info(id, "Last Checked", "Last update check", "Never", category));
                id += 1;
                items.push(SettingsItem::button(id, "Check for Updates", "Look for system updates", category));
                id += 1;
                items.push(SettingsItem::toggle(id, "Auto-update", "Automatically install updates", true, category));
            }
            
            SettingsCategory::About => {
                items.push(SettingsItem::info(id, "echOS", "Operating System", "echOS v0.1.0", category));
                id += 1;
                items.push(SettingsItem::info(id, "Kernel", "Kernel version", "0.1.0", category));
                id += 1;
                items.push(SettingsItem::info(id, "Architecture", "CPU architecture", "x86_64", category));
                id += 1;
                items.push(SettingsItem::info(id, "CPU", "Processor information", "Unknown", category));
                id += 1;
                items.push(SettingsItem::info(id, "Memory", "Installed RAM", "Unknown", category));
                id += 1;
                items.push(SettingsItem::info(id, "License", "Software license", "MIT", category));
            }
        }
        
        items
    }
    
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let mut item_y = y as i32 - self.scroll_offset as i32;
        let item_height = 60;
        let padding = 16;
        
        for item in &self.items {
            if item_y + (item_height as i32) < y as i32 {
                item_y += item_height as i32;
                continue;
            }
            if item_y >= (y + height) as i32 {
                break;
            }
            
            if item_y >= y as i32 {
                self.draw_item(item, fb, x + padding, item_y as usize, width - padding * 2);
            }
            
            item_y += item_height as i32;
        }
    }
    
    fn draw_item(&self, item: &SettingsItem, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        // Draw item background
        fb.draw_rect(x, y, width, 56, Theme::WINDOW_BG.to_u32());
        
        // Draw name
        fb.draw_string(x + 8, y + 8, &item.name, Theme::TEXT_PRIMARY.to_u32());
        
        // Draw description
        fb.draw_string(x + 8, y + 24, &item.description, Theme::TEXT_SECONDARY.to_u32());
        
        // Draw control based on type
        let control_x = x + width - 150;
        let control_y = y + 16;
        
        match &item.item_type {
            SettingsItemType::Toggle => {
                self.draw_toggle(fb, control_x, control_y, item.value.as_bool());
            }
            SettingsItemType::Slider { min, max, step } => {
                self.draw_slider(fb, control_x, control_y, 130, item.value.as_float(), *min, *max);
            }
            SettingsItemType::Dropdown { options } => {
                self.draw_dropdown(fb, control_x, control_y, 130, item.value.as_string(), options);
            }
            SettingsItemType::Info => {
                fb.draw_string(control_x, control_y, item.value.as_string(), Theme::TEXT_SECONDARY.to_u32());
            }
            SettingsItemType::Button => {
                self.draw_button(fb, control_x - 20, control_y - 4, 130, 32, &item.name);
            }
            _ => {}
        }
        
        // Draw separator
        fb.draw_rect(x + 8, y + 54, width - 16, 1, Theme::BORDER.to_u32());
    }
    
    fn draw_toggle(&self, fb: &mut Framebuffer, x: usize, y: usize, on: bool) {
        let width = 44;
        let height = 22;
        let radius = height / 2;
        
        // Background
        let bg_color = if on { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::BORDER.to_u32() };
        
        // Draw rounded rectangle
        for py in 0..height {
            for px in 0..width {
                let in_corner = (px < radius && (radius - px) as i32 * (radius - px) as i32 + (py as i32 - radius as i32).pow(2) > radius as i32 * radius as i32)
                    || (px > width - radius && (px as i32 - width as i32 + radius as i32).pow(2) + (py as i32 - radius as i32).pow(2) > radius as i32 * radius as i32);
                
                if !in_corner {
                    fb.plot_pixel(x + px, y + py, bg_color);
                }
            }
        }
        
        // Knob
        let knob_x = if on { x + width - radius - 2 } else { x + radius - radius + 2 };
        let knob_color = Theme::TEXT_ON_ACCENT.to_u32();
        
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= ((radius - 2) * (radius - 2)) as i32 {
                    fb.plot_pixel(knob_x + px, y + 2 + py, knob_color);
                }
            }
        }
    }
    
    fn draw_slider(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, value: f32, min: f32, max: f32) {
        let track_height = 4;
        let thumb_radius = 8;
        
        // Track background
        fb.draw_rect(x, y + 6, width, track_height, Theme::BORDER.to_u32());
        
        // Filled portion
        let fill_width = ((value - min) / (max - min) * width as f32) as usize;
        fb.draw_rect(x, y + 6, fill_width, track_height, Theme::ACCENT_PRIMARY.to_u32());
        
        // Thumb
        let thumb_x = x + fill_width;
        let thumb_y = y + 8;
        
        for py in 0..thumb_radius * 2 {
            for px in 0..thumb_radius * 2 {
                let dx = px as i32 - thumb_radius as i32;
                let dy = py as i32 - thumb_radius as i32;
                if dx * dx + dy * dy <= thumb_radius * thumb_radius {
                    fb.plot_pixel((thumb_x as i32 + px as i32 - thumb_radius as i32) as usize, (thumb_y as i32 + py as i32 - thumb_radius as i32) as usize, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
        
        // Value label
        let value_text = format!("{:.0}", value);
        fb.draw_string(x + width + 8, y, &value_text, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_dropdown(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, value: &str, _options: &[String]) {
        // Dropdown box
        fb.draw_rect(x, y, width, 28, Theme::INPUT_BG.to_u32());
        fb.draw_rect_outline(x, y, width, 28, Theme::BORDER.to_u32());
        
        // Current value
        fb.draw_string(x + 8, y + 6, value, Theme::TEXT_PRIMARY.to_u32());
        
        // Dropdown arrow
        fb.draw_string(x + width - 16, y + 6, "▼", Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_button(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize, text: &str) {
        // Button background
        fb.draw_rect(x, y, width, height, Theme::ACCENT_PRIMARY.to_u32());
        
        // Button text (centered)
        let text_width = text.len() * 8;
        let text_x = x + (width - text_width) / 2;
        let text_y = y + (height - 12) / 2;
        fb.draw_string(text_x, text_y, text, Theme::TEXT_ON_ACCENT.to_u32());
    }
    
    pub fn scroll(&mut self, amount: i32) {
        if amount > 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(amount as usize);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-amount) as usize);
        }
    }
    
    pub fn hit_test(&self, x: i32, y: i32, panel_x: usize, panel_y: usize) -> Option<u32> {
        let item_height = 60;
        let adjusted_y = y + self.scroll_offset as i32 - panel_y as i32;
        
        if adjusted_y >= 0 {
            let item_idx = adjusted_y as usize / item_height;
            if item_idx < self.items.len() {
                return Some(self.items[item_idx].id);
            }
        }
        
        None
    }
    
    pub fn get_item_mut(&mut self, id: u32) -> Option<&mut SettingsItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

// ============================================================================
// SETTINGS APP
// ============================================================================

/// Main Settings Application
pub struct SettingsApp {
    /// Current category
    current_category: SettingsCategory,
    /// Category panels
    panels: BTreeMap<SettingsCategory, SettingsPanel>,
    /// Window position and size
    rect: Rect,
    /// Sidebar width
    sidebar_width: usize,
    /// Search query
    search_query: String,
    /// Scroll position
    scroll_offset: usize,
    /// Hovered category
    hovered_category: Option<SettingsCategory>,
    /// Selected item
    selected_item: Option<u32>,
}

impl SettingsApp {
    pub fn new() -> Self {
        let mut panels = BTreeMap::new();
        
        for &category in SettingsCategory::all() {
            panels.insert(category, SettingsPanel::new(category));
        }
        
        SettingsApp {
            current_category: SettingsCategory::System,
            panels,
            rect: Rect::new(100, 100, 800, 600),
            sidebar_width: 200,
            search_query: String::new(),
            scroll_offset: 0,
            hovered_category: None,
            selected_item: None,
        }
    }
    
    /// Draw the settings app
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;
        
        // Draw window background
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        
        // Draw title bar
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        fb.draw_string(x + 12, y + 8, "Settings", Theme::TEXT_PRIMARY.to_u32());
        
        // Draw close button
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());
        
        // Draw sidebar
        let sidebar_x = x;
        let sidebar_y = y + 32;
        
        fb.draw_rect(sidebar_x, sidebar_y, self.sidebar_width, height - 32, Theme::SIDEBAR_BG.to_u32());
        
        // Draw search box
        let search_y = sidebar_y + 8;
        fb.draw_rect(sidebar_x + 8, search_y, self.sidebar_width - 16, 28, Theme::INPUT_BG.to_u32());
        fb.draw_string(sidebar_x + 12, search_y + 6, "🔍 Search...", Theme::TEXT_SECONDARY.to_u32());
        
        // Draw category list
        let mut category_y = sidebar_y + 48;
        for &category in SettingsCategory::all() {
            let is_selected = category == self.current_category;
            let is_hovered = self.hovered_category == Some(category);
            
            let bg_color = if is_selected {
                Theme::ACCENT_PRIMARY.to_u32()
            } else if is_hovered {
                Theme::LIST_ITEM_HOVER.to_u32()
            } else {
                Theme::TRANSPARENT.to_u32()
            };
            
            // Category item background
            fb.draw_rect(sidebar_x + 4, category_y, self.sidebar_width - 8, 36, bg_color);
            
            // Icon and name
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            fb.draw_string(sidebar_x + 12, category_y + 10, category.icon(), text_color);
            fb.draw_string(sidebar_x + 36, category_y + 10, category.name(), text_color);
            
            category_y += 40;
        }
        
        // Draw content area
        let content_x = x + self.sidebar_width;
        let content_y = sidebar_y;
        let content_width = width - self.sidebar_width;
        let content_height = height - 32;
        
        // Content background
        fb.draw_rect(content_x, content_y, content_width, content_height, Theme::WINDOW_BG.to_u32());
        
        // Draw category title
        fb.draw_string(content_x + 16, content_y + 16, self.current_category.name(), Theme::TEXT_PRIMARY.to_u32());
        fb.draw_rect(content_x + 16, content_y + 36, content_width - 32, 1, Theme::BORDER.to_u32());
        
        // Draw panel content
        if let Some(panel) = self.panels.get(&self.current_category) {
            panel.draw(fb, content_x + 8, content_y + 48, content_width - 16, content_height - 56);
        }
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        let sidebar_x = self.rect.x;
        let sidebar_y = self.rect.y + 32;
        
        // Check sidebar categories
        if mx >= sidebar_x && mx < sidebar_x + self.sidebar_width as i32 {
            let mut category_y = sidebar_y + 48;
            for &category in SettingsCategory::all() {
                if my >= category_y && my < category_y + 36 {
                    self.hovered_category = Some(category);
                    return;
                }
                category_y += 40;
            }
        }
        
        self.hovered_category = None;
    }
    
    /// Handle mouse click
    pub fn on_click(&mut self, mx: i32, my: i32) -> SettingsAction {
        let sidebar_x = self.rect.x;
        let sidebar_y = self.rect.y + 32;
        
        // Check close button
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return SettingsAction::Close;
        }
        
        // Check sidebar categories
        if mx >= sidebar_x && mx < sidebar_x + self.sidebar_width as i32 {
            let mut category_y = sidebar_y + 48;
            for &category in SettingsCategory::all() {
                if my >= category_y && my < category_y + 36 {
                    self.current_category = category;
                    return SettingsAction::CategoryChanged(category);
                }
                category_y += 40;
            }
        }
        
        // Check content area items
        let content_x = self.rect.x + self.sidebar_width as i32;
        let content_y = self.rect.y + 32 + 48;
        
        if mx >= content_x && my >= content_y {
            if let Some(panel) = self.panels.get_mut(&self.current_category) {
                if let Some(item_id) = panel.hit_test(mx, my, content_x as usize, content_y as usize) {
                    if let Some(item) = panel.get_item_mut(item_id) {
                        match &item.item_type {
                            SettingsItemType::Toggle => {
                                item.value = SettingsValue::Bool(!item.value.as_bool());
                                return SettingsAction::SettingChanged(item_id, item.value.clone());
                            }
                            SettingsItemType::Button => {
                                return SettingsAction::ButtonClicked(item_id);
                            }
                            _ => {
                                self.selected_item = Some(item_id);
                            }
                        }
                    }
                }
            }
        }
        
        SettingsAction::None
    }
    
    /// Handle scroll
    pub fn on_scroll(&mut self, delta: i32) {
        if let Some(panel) = self.panels.get_mut(&self.current_category) {
            panel.scroll(delta * 30);
        }
    }
    
    /// Get rect
    pub fn rect(&self) -> Rect {
        self.rect
    }
    
    /// Set rect
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

/// Actions from settings app
#[derive(Clone, Debug)]
pub enum SettingsAction {
    None,
    Close,
    CategoryChanged(SettingsCategory),
    SettingChanged(u32, SettingsValue),
    ButtonClicked(u32),
}

impl Default for SettingsApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL SETTINGS
// ============================================================================

lazy_static::lazy_static! {
    static ref SETTINGS_APP: Mutex<SettingsApp> = Mutex::new(SettingsApp::new());
}

/// Get settings app
pub fn get_app() -> &'static Mutex<SettingsApp> {
    &SETTINGS_APP
}

/// Initialize settings
pub fn init() {
    crate::serial_println!("[GUI] Settings application initialized");
}
