//! # System Preferences Application
//!
//! macOS-style System Preferences with preference panes
//! Organized settings for display, sound, network, users, etc.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// PREFERENCES CONSTANTS
// ============================================================================

/// Sidebar width
pub const SIDEBAR_WIDTH: usize = 220;

/// Toolbar height
pub const TOOLBAR_HEIGHT: usize = 44;

/// Search field height
pub const SEARCH_HEIGHT: usize = 28;

/// Preference pane icon size
pub const PANE_ICON_SIZE: usize = 64;

// ============================================================================
// PREFERENCE PANE
// ============================================================================

/// A preference pane
#[derive(Clone, Debug)]
pub struct PreferencePane {
    /// Pane ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Icon
    pub icon: PreferenceIcon,
    /// Category
    pub category: PreferenceCategory,
    /// Settings
    pub settings: Vec<Setting>,
    /// Is visible in search
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferenceIcon {
    General,
    DesktopScreenSaver,
    DockMenuBar,
    MissionControl,
    LanguageRegion,
    Notifications,
    InternetAccounts,
    Wallet,
    TouchID,
    UsersGroups,
    Accessibility,
    SecurityPrivacy,
    Network,
    Bluetooth,
    Sound,
    Keyboard,
    Trackpad,
    Mouse,
    Displays,
    Battery,
    Date,
    Sharing,
    TimeMachine,
    StartupDisk,
    Extensions,
    Custom(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferenceCategory {
    General,
    Personal,
    Hardware,
    System,
    Other,
}

impl PreferencePane {
    pub fn new(id: &str, name: &str, icon: PreferenceIcon, category: PreferenceCategory) -> Self {
        PreferencePane {
            id: String::from(id),
            name: String::from(name),
            icon,
            category,
            settings: Vec::new(),
            visible: true,
        }
    }
    
    pub fn add_setting(&mut self, setting: Setting) {
        self.settings.push(setting);
    }
}

// ============================================================================
// SETTING
// ============================================================================

/// A setting item
#[derive(Clone, Debug)]
pub struct Setting {
    /// Setting ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Setting type
    pub setting_type: SettingType,
    /// Current value
    pub value: SettingValue,
    /// Default value
    pub default: SettingValue,
    /// Requires restart
    pub requires_restart: bool,
    /// Is enabled
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub enum SettingType {
    Toggle,
    Slider { min: f32, max: f32, step: f32 },
    Dropdown { options: Vec<String> },
    Input { placeholder: String },
    Color,
    Font,
    Shortcut,
    List { items: Vec<ListItem> },
    Group { settings: Vec<Setting> },
}

#[derive(Clone, Debug)]
pub enum SettingValue {
    Bool(bool),
    Float(f32),
    Int(i32),
    String(String),
    Color(u32),
    Shortcut { modifiers: u8, key: char },
    None,
}

#[derive(Clone, Debug)]
pub struct ListItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub selected: bool,
}

impl Setting {
    pub fn toggle(id: &str, name: &str, default: bool) -> Self {
        Setting {
            id: String::from(id),
            name: String::from(name),
            description: String::new(),
            setting_type: SettingType::Toggle,
            value: SettingValue::Bool(default),
            default: SettingValue::Bool(default),
            requires_restart: false,
            enabled: true,
        }
    }
    
    pub fn slider(id: &str, name: &str, min: f32, max: f32, default: f32) -> Self {
        Setting {
            id: String::from(id),
            name: String::from(name),
            description: String::new(),
            setting_type: SettingType::Slider { min, max, step: 1.0 },
            value: SettingValue::Float(default),
            default: SettingValue::Float(default),
            requires_restart: false,
            enabled: true,
        }
    }
    
    pub fn dropdown(id: &str, name: &str, options: Vec<&str>, default: &str) -> Self {
        Setting {
            id: String::from(id),
            name: String::from(name),
            description: String::new(),
            setting_type: SettingType::Dropdown {
                options: options.iter().map(|s| String::from(*s)).collect(),
            },
            value: SettingValue::String(String::from(default)),
            default: SettingValue::String(String::from(default)),
            requires_restart: false,
            enabled: true,
        }
    }
    
    pub fn input(id: &str, name: &str, placeholder: &str, default: &str) -> Self {
        Setting {
            id: String::from(id),
            name: String::from(name),
            description: String::new(),
            setting_type: SettingType::Input { placeholder: String::from(placeholder) },
            value: SettingValue::String(String::from(default)),
            default: SettingValue::String(String::from(default)),
            requires_restart: false,
            enabled: true,
        }
    }
    
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = String::from(desc);
        self
    }
    
    pub fn with_restart(mut self) -> Self {
        self.requires_restart = true;
        self
    }
    
    fn get_bool(&self) -> bool {
        match &self.value {
            SettingValue::Bool(b) => *b,
            _ => false,
        }
    }
    
    fn get_float(&self) -> f32 {
        match &self.value {
            SettingValue::Float(f) => *f,
            _ => 0.0,
        }
    }
    
    fn get_string(&self) -> &str {
        match &self.value {
            SettingValue::String(s) => s,
            _ => "",
        }
    }
}

// ============================================================================
// SYSTEM PREFERENCES WINDOW
// ============================================================================

/// System Preferences window
pub struct SystemPreferences {
    /// Window title
    pub title: String,
    /// Window rect
    pub rect: Rect,
    /// Preference panes
    pub panes: Vec<PreferencePane>,
    /// Selected pane index
    pub selected_pane: Option<usize>,
    /// Search query
    pub search_query: String,
    /// Search focused
    pub search_focused: bool,
    /// Hovered pane
    pub hovered_pane: Option<usize>,
    /// Hovered setting
    pub hovered_setting: Option<usize>,
    /// Scroll offset
    pub scroll_offset: usize,
    /// Show all button state
    pub show_all: bool,
}

impl SystemPreferences {
    pub fn new(rect: Rect) -> Self {
        let mut prefs = SystemPreferences {
            title: String::from("System Preferences"),
            rect,
            panes: Vec::new(),
            selected_pane: None,
            search_query: String::new(),
            search_focused: false,
            hovered_pane: None,
            hovered_setting: None,
            scroll_offset: 0,
            show_all: true,
        };
        
        prefs.init_panes();
        prefs
    }
    
    fn init_panes(&mut self) {
        // General
        let mut general = PreferencePane::new("general", "General", PreferenceIcon::General, PreferenceCategory::General);
        general.add_setting(Setting::dropdown("appearance", "Appearance", 
            vec!["Light", "Dark", "Auto"], "Dark"));
        general.add_setting(Setting::dropdown("accent", "Accent color",
            vec!["Blue", "Purple", "Pink", "Red", "Orange", "Yellow", "Green", "Gray"], "Blue"));
        general.add_setting(Setting::toggle("auto_hide_menu", "Automatically hide and show the menu bar", false));
        general.add_setting(Setting::toggle("show_scroll_bars", "Show scroll bars", true));
        general.add_setting(Setting::dropdown("click_scroll_bar", "Click in scroll bar to",
            vec!["Jump to next page", "Jump to spot clicked"], "Jump to spot clicked"));
        general.add_setting(Setting::toggle("ask_save_changes", "Ask to keep changes when closing documents", true));
        general.add_setting(Setting::toggle("close_confirms", "Close windows when quitting an app", false));
        self.panes.push(general);
        
        // Desktop & Screen Saver
        let mut desktop = PreferencePane::new("desktop", "Desktop & Screen Saver", PreferenceIcon::DesktopScreenSaver, PreferenceCategory::Personal);
        desktop.add_setting(Setting::dropdown("wallpaper", "Desktop",
            vec!["Solid Black", "Solid Dark", "Sunset", "Ocean", "Forest", "Stars", "Aurora"], "Stars"));
        desktop.add_setting(Setting::dropdown("screensaver", "Screen Saver",
            vec!["None", "Flurry", "Stars", "Matrix", "Photos"], "None"));
        desktop.add_setting(Setting::slider("screensaver_delay", "Start after (minutes)", 1.0, 60.0, 5.0));
        self.panes.push(desktop);
        
        // Dock & Menu Bar
        let mut dock = PreferencePane::new("dock", "Dock & Menu Bar", PreferenceIcon::DockMenuBar, PreferenceCategory::Personal);
        dock.add_setting(Setting::slider("dock_size", "Size", 0.3, 1.0, 0.6));
        dock.add_setting(Setting::toggle("magnification", "Magnification", true));
        dock.add_setting(Setting::slider("magnification_size", "Magnification size", 0.5, 1.5, 1.0));
        dock.add_setting(Setting::dropdown("position", "Position on screen",
            vec!["Left", "Bottom", "Right"], "Bottom"));
        dock.add_setting(Setting::toggle("auto_hide_dock", "Automatically hide and show the Dock", false));
        dock.add_setting(Setting::toggle("animate_opening", "Animate opening applications", true));
        dock.add_setting(Setting::toggle("show_indicator", "Show indicators for open applications", true));
        self.panes.push(dock);
        
        // Mission Control
        let mut mission = PreferencePane::new("mission_control", "Mission Control", PreferenceIcon::MissionControl, PreferenceCategory::Personal);
        mission.add_setting(Setting::toggle("auto_arrange", "Automatically rearrange Spaces", true));
        mission.add_setting(Setting::toggle("switch_space", "When switching to an application, switch to a Space with open windows", true));
        mission.add_setting(Setting::toggle("group_windows", "Group windows by application", false));
        mission.add_setting(Setting::dropdown("displays", "Displays have separate Spaces",
            vec!["Yes", "No"], "Yes"));
        self.panes.push(mission);
        
        // Notifications
        let mut notifications = PreferencePane::new("notifications", "Notifications", PreferenceIcon::Notifications, PreferenceCategory::Personal);
        notifications.add_setting(Setting::toggle("do_not_disturb", "Do Not Disturb", false));
        notifications.add_setting(Setting::toggle("show_preview", "Show notifications on lock screen", true));
        notifications.add_setting(Setting::toggle("sounds", "Play sounds for notifications", true));
        notifications.add_setting(Setting::dropdown("alert_style", "Alert Style",
            vec!["None", "Banners", "Alerts"], "Banners"));
        self.panes.push(notifications);
        
        // Network
        let mut network = PreferencePane::new("network", "Network", PreferenceIcon::Network, PreferenceCategory::Hardware);
        network.add_setting(Setting::dropdown("wifi", "Wi-Fi",
            vec!["On", "Off"], "On"));
        network.add_setting(Setting::input("ssid", "Network Name", "Enter SSID", ""));
        network.add_setting(Setting::toggle("ask_join", "Ask to join new networks", true));
        network.add_setting(Setting::dropdown("ethernet", "Ethernet",
            vec!["Connected", "Disconnected"], "Connected"));
        self.panes.push(network);
        
        // Bluetooth
        let mut bluetooth = PreferencePane::new("bluetooth", "Bluetooth", PreferenceIcon::Bluetooth, PreferenceCategory::Hardware);
        bluetooth.add_setting(Setting::dropdown("bluetooth", "Bluetooth",
            vec!["On", "Off"], "On"));
        bluetooth.add_setting(Setting::toggle("discoverable", "Discoverable", true));
        bluetooth.add_setting(Setting::toggle("show_in_menu", "Show Bluetooth in menu bar", true));
        self.panes.push(bluetooth);
        
        // Sound
        let mut sound = PreferencePane::new("sound", "Sound", PreferenceIcon::Sound, PreferenceCategory::Hardware);
        sound.add_setting(Setting::slider("output_volume", "Output Volume", 0.0, 100.0, 75.0));
        sound.add_setting(Setting::toggle("output_mute", "Mute", false));
        sound.add_setting(Setting::slider("input_volume", "Input Volume", 0.0, 100.0, 50.0));
        sound.add_setting(Setting::toggle("play_feedback", "Play feedback when volume is changed", true));
        sound.add_setting(Setting::dropdown("output_device", "Output Device",
            vec!["Internal Speakers", "Headphones", "External Speakers"], "Internal Speakers"));
        sound.add_setting(Setting::dropdown("input_device", "Input Device",
            vec!["Internal Microphone", "External Microphone"], "Internal Microphone"));
        self.panes.push(sound);
        
        // Displays
        let mut displays = PreferencePane::new("displays", "Displays", PreferenceIcon::Displays, PreferenceCategory::Hardware);
        displays.add_setting(Setting::slider("brightness", "Brightness", 0.0, 100.0, 80.0));
        displays.add_setting(Setting::toggle("auto_brightness", "Automatically adjust brightness", true));
        displays.add_setting(Setting::dropdown("resolution", "Resolution",
            vec!["1920x1080", "1680x1050", "1440x900", "1280x720", "Scaled"], "1920x1080"));
        displays.add_setting(Setting::dropdown("refresh_rate", "Refresh Rate",
            vec!["60 Hz", "75 Hz", "120 Hz", "144 Hz"], "60 Hz"));
        displays.add_setting(Setting::toggle("night_shift", "Night Shift", false));
        displays.add_setting(Setting::toggle("true_tone", "True Tone", true));
        self.panes.push(displays);
        
        // Keyboard
        let mut keyboard = PreferencePane::new("keyboard", "Keyboard", PreferenceIcon::Keyboard, PreferenceCategory::Hardware);
        keyboard.add_setting(Setting::slider("key_repeat", "Key Repeat", 0.0, 100.0, 50.0));
        keyboard.add_setting(Setting::slider("delay_until_repeat", "Delay Until Repeat", 0.0, 100.0, 50.0));
        keyboard.add_setting(Setting::toggle("adjust_brightness", "Adjust keyboard brightness in low light", true));
        keyboard.add_setting(Setting::toggle("fn_key", "Use F1, F2 as standard function keys", false));
        keyboard.add_setting(Setting::dropdown("modifier_keys", "Modifier Keys...",
            vec!["Configure...", "Default"], "Default"));
        self.panes.push(keyboard);
        
        // Mouse
        let mut mouse = PreferencePane::new("mouse", "Mouse", PreferenceIcon::Mouse, PreferenceCategory::Hardware);
        mouse.add_setting(Setting::slider("tracking_speed", "Tracking Speed", 0.0, 100.0, 50.0));
        mouse.add_setting(Setting::slider("scroll_speed", "Scrolling Speed", 0.0, 100.0, 50.0));
        mouse.add_setting(Setting::toggle("natural_scrolling", "Natural scrolling", true));
        mouse.add_setting(Setting::toggle("secondary_click", "Secondary click", true));
        mouse.add_setting(Setting::dropdown("click", "Click",
            vec!["Light", "Medium", "Firm"], "Medium"));
        self.panes.push(mouse);
        
        // Users & Groups
        let mut users = PreferencePane::new("users", "Users & Groups", PreferenceIcon::UsersGroups, PreferenceCategory::System);
        users.add_setting(Setting::dropdown("current_user", "Current User",
            vec!["Administrator", "Guest"], "Administrator"));
        users.add_setting(Setting::toggle("auto_login", "Automatic login", false));
        users.add_setting(Setting::toggle("login_window", "Show fast user switching menu", true));
        users.add_setting(Setting::dropdown("login_items", "Login Items",
            vec!["Manage...", "None"], "None"));
        self.panes.push(users);
        
        // Security & Privacy
        let mut security = PreferencePane::new("security", "Security & Privacy", PreferenceIcon::SecurityPrivacy, PreferenceCategory::System);
        security.add_setting(Setting::toggle("require_password", "Require password", true));
        security.add_setting(Setting::dropdown("password_delay", "Require password after",
            vec!["Immediately", "5 seconds", "1 minute", "5 minutes", "1 hour"], "Immediately"));
        security.add_setting(Setting::toggle("filevault", "FileVault disk encryption", true));
        security.add_setting(Setting::toggle("firewall", "Firewall", true));
        security.add_setting(Setting::toggle("location_services", "Location Services", true));
        self.panes.push(security);
        
        // Date & Time
        let mut datetime = PreferencePane::new("datetime", "Date & Time", PreferenceIcon::Date, PreferenceCategory::System);
        datetime.add_setting(Setting::toggle("set_auto", "Set date and time automatically", true));
        datetime.add_setting(Setting::dropdown("timezone", "Time Zone",
            vec!["UTC", "EST", "PST", "CET", "GMT"], "UTC"));
        datetime.add_setting(Setting::dropdown("clock_style", "Clock Style",
            vec!["12-hour", "24-hour"], "24-hour"));
        datetime.add_setting(Setting::toggle("show_date", "Show date in menu bar", true));
        datetime.add_setting(Setting::toggle("show_day", "Show day of week", true));
        self.panes.push(datetime);
        
        // Battery
        let mut battery = PreferencePane::new("battery", "Battery", PreferenceIcon::Battery, PreferenceCategory::Hardware);
        battery.add_setting(Setting::slider("brightness", "Display brightness", 0.0, 100.0, 80.0));
        battery.add_setting(Setting::toggle("low_power_mode", "Low Power Mode", false));
        battery.add_setting(Setting::dropdown("sleep", "Turn display off after",
            vec!["1 min", "5 min", "10 min", "30 min", "Never"], "10 min"));
        battery.add_setting(Setting::dropdown("sleep_computer", "Put computer to sleep after",
            vec!["1 min", "5 min", "10 min", "30 min", "Never"], "30 min"));
        battery.add_setting(Setting::toggle("show_percentage", "Show battery percentage in menu bar", true));
        self.panes.push(battery);
        
        // Sharing
        let mut sharing = PreferencePane::new("sharing", "Sharing", PreferenceIcon::Sharing, PreferenceCategory::System);
        sharing.add_setting(Setting::toggle("screen_sharing", "Screen Sharing", false));
        sharing.add_setting(Setting::toggle("file_sharing", "File Sharing", false));
        sharing.add_setting(Setting::toggle("printer_sharing", "Printer Sharing", false));
        sharing.add_setting(Setting::toggle("remote_login", "Remote Login", false));
        sharing.add_setting(Setting::input("computer_name", "Computer Name", "Computer Name", "echOS"));
        self.panes.push(sharing);
    }
    
    pub fn select_pane(&mut self, index: usize) {
        if index < self.panes.len() {
            self.selected_pane = Some(index);
            self.scroll_offset = 0;
        }
    }
    
    pub fn go_back(&mut self) {
        self.selected_pane = None;
        self.scroll_offset = 0;
    }
    
    pub fn search(&mut self) {
        if self.search_query.is_empty() {
            for pane in &mut self.panes {
                pane.visible = true;
            }
            return;
        }
        
        let query = self.search_query.to_lowercase();
        
        for pane in &mut self.panes {
            pane.visible = pane.name.to_lowercase().contains(&query) ||
                pane.settings.iter().any(|s| s.name.to_lowercase().contains(&query));
        }
    }
    
    /// Draw System Preferences
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Window background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // No internal toolbar — WM Cyber titlebar is the chrome
        if let Some(pane_idx) = self.selected_pane {
            // Draw pane content
            self.draw_pane_content(fb, x, y, w, h, pane_idx);
        } else {
            // Draw pane grid
            fb.draw_rect(x, y, SIDEBAR_WIDTH, h, Theme::SIDEBAR_BG.to_u32());
            self.draw_pane_grid(fb, x, y, w, h);
        }
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        // Back button (if pane selected)
        if self.selected_pane.is_some() {
            fb.draw_rect(x + 8, y + 8, 28, 28, Theme::SIDEBAR_BG.to_u32());
            fb.draw_string(x + 14, y + 12, "◀", Theme::TEXT_PRIMARY.to_u32());
        }
        
        // Title
        let title = if let Some(idx) = self.selected_pane {
            &self.panes[idx].name
        } else {
            "System Preferences"
        };
        fb.draw_string(x + w / 2 - title.len() * 4, y + 12, title, Theme::TEXT_PRIMARY.to_u32());
        
        // Search field
        let search_x = x + w - 180;
        fb.draw_rect(search_x, y + 8, 160, SEARCH_HEIGHT, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(search_x + 8, y + 12, "🔍", Theme::TEXT_SECONDARY.to_u32());
        
        if self.search_query.is_empty() && !self.search_focused {
            fb.draw_string(search_x + 28, y + 12, "Search", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(search_x + 28, y + 12, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }
        
        if self.search_focused {
            let cursor_x = search_x + 28 + self.search_query.len() * 8;
            fb.draw_rect(cursor_x, y + 12, 2, 14, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn draw_pane_grid(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Sidebar with categories
        let categories = [
            (PreferenceCategory::General, "General"),
            (PreferenceCategory::Personal, "Personal"),
            (PreferenceCategory::Hardware, "Hardware"),
            (PreferenceCategory::System, "System"),
        ];
        
        let mut item_y = y + 8;
        
        for (cat, cat_name) in &categories {
            // Category header
            fb.draw_string(x + 12, item_y, cat_name, Theme::TEXT_SECONDARY.to_u32());
            item_y += 20;
            
            for (i, pane) in self.panes.iter().enumerate() {
                if pane.category == *cat && pane.visible {
                    let is_hovered = self.hovered_pane == Some(i);
                    let bg = if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() } else { Theme::TRANSPARENT.to_u32() };
                    
                    fb.draw_rect(x, item_y, SIDEBAR_WIDTH, 28, bg);
                    
                    let icon = self.get_pane_icon(pane.icon);
                    fb.draw_string(x + 12, item_y + 4, icon, Theme::TEXT_PRIMARY.to_u32());
                    fb.draw_string(x + 40, item_y + 4, &pane.name, Theme::TEXT_PRIMARY.to_u32());
                    
                    item_y += 28;
                }
            }
            
            item_y += 8;
        }
        
        // Main area with icons
        let main_x = x + SIDEBAR_WIDTH;
        let main_w = w - SIDEBAR_WIDTH;
        
        let cols = main_w / (PANE_ICON_SIZE + 32);
        let mut icon_x = main_x + 16;
        let mut icon_y = y + 16;
        
        for (i, pane) in self.panes.iter().enumerate() {
            if !pane.visible {
                continue;
            }
            
            let is_hovered = self.hovered_pane == Some(i);
            
            // Background
            if is_hovered {
                fb.draw_rect(icon_x, icon_y, PANE_ICON_SIZE + 16, PANE_ICON_SIZE + 32, Theme::LIST_ITEM_HOVER.to_u32());
            }
            
            // Icon
            let icon = self.get_pane_icon(pane.icon);
            fb.draw_string(icon_x + 8, icon_y + 8, icon, Theme::TEXT_PRIMARY.to_u32());
            
            // Name
            let name = if pane.name.len() > 12 { format!("{}...", &pane.name[..9]) } else { pane.name.clone() };
            let text_w = name.len() * 8;
            let container_w = PANE_ICON_SIZE + 16;
            let name_x = icon_x + container_w.saturating_sub(text_w) / 2;
            fb.draw_string(name_x, icon_y + PANE_ICON_SIZE + 12, &name, Theme::TEXT_PRIMARY.to_u32());
            
            // Next position
            icon_x += PANE_ICON_SIZE + 32;
            if icon_x + PANE_ICON_SIZE > main_x + main_w {
                icon_x = main_x + 16;
                icon_y += PANE_ICON_SIZE + 48;
            }
        }
    }
    
    fn draw_pane_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, pane_idx: usize) {
        let pane = &self.panes[pane_idx];
        
        // Sidebar with setting categories
        fb.draw_rect(x, y, SIDEBAR_WIDTH, h, Theme::SIDEBAR_BG.to_u32());
        
        let mut setting_y = y + 8;
        let visible_settings = pane.settings.len();
        let visible_height = h - 16;
        let scroll = self.scroll_offset;
        
        for (i, setting) in pane.settings.iter().skip(scroll).enumerate() {
            if setting_y + 40 > y + visible_height {
                break;
            }
            
            let is_hovered = self.hovered_setting == Some(scroll + i);
            let bg = if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(x + 4, setting_y, SIDEBAR_WIDTH - 8, 32, bg);
            fb.draw_string(x + 12, setting_y + 8, &setting.name, Theme::TEXT_PRIMARY.to_u32());
            
            setting_y += 36;
        }
        
        // Main content area
        let content_x = x + SIDEBAR_WIDTH;
        let content_w = w - SIDEBAR_WIDTH;
        
        fb.draw_rect(content_x, y, content_w, h, Theme::WINDOW_BG.to_u32());
        
        // Draw selected setting details
        setting_y = y + 20;
        
        for (i, setting) in pane.settings.iter().skip(scroll).enumerate() {
            if setting_y + 60 > y + h {
                break;
            }
            
            // Setting name
            fb.draw_string(content_x + 20, setting_y, &setting.name, Theme::TEXT_PRIMARY.to_u32());
            
            // Setting control
            let control_x = content_x + content_w - 200;
            
            match &setting.setting_type {
                SettingType::Toggle => {
                    let is_on = setting.get_bool();
                    let toggle_color = if is_on { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::BORDER.to_u32() };
                    
                    fb.draw_rect(control_x, setting_y + 4, 44, 24, toggle_color);
                    
                    let knob_x = if is_on { control_x + 20 } else { control_x + 2 };
                    fb.draw_rect(knob_x, setting_y + 6, 20, 20, 0xFFFFFFFF);
                }
                SettingType::Slider { min, max, step: _ } => {
                    let value = setting.get_float();
                    let pct = (value - *min) / (max - min);
                    
                    fb.draw_rect(control_x, setting_y + 12, 180, 8, Theme::BORDER.to_u32());
                    fb.draw_rect(control_x, setting_y + 12, (180.0 * pct) as usize, 8, Theme::ACCENT_PRIMARY.to_u32());
                    
                    let value_text = format!("{:.0}", value);
                    fb.draw_string(control_x + 190, setting_y + 4, &value_text, Theme::TEXT_SECONDARY.to_u32());
                }
                SettingType::Dropdown { options } => {
                    fb.draw_rect(control_x, setting_y + 2, 180, 24, Theme::SIDEBAR_BG.to_u32());
                    fb.draw_string(control_x + 8, setting_y + 6, setting.get_string(), Theme::TEXT_PRIMARY.to_u32());
                    fb.draw_string(control_x + 160, setting_y + 6, "▾", Theme::TEXT_SECONDARY.to_u32());
                }
                SettingType::Input { placeholder } => {
                    fb.draw_rect(control_x, setting_y + 2, 180, 24, Theme::SIDEBAR_BG.to_u32());
                    
                    let text = setting.get_string();
                    if text.is_empty() {
                        fb.draw_string(control_x + 8, setting_y + 6, placeholder, Theme::TEXT_SECONDARY.to_u32());
                    } else {
                        fb.draw_string(control_x + 8, setting_y + 6, text, Theme::TEXT_PRIMARY.to_u32());
                    }
                }
                _ => {}
            }
            
            // Description
            if !setting.description.is_empty() {
                fb.draw_string(content_x + 20, setting_y + 20, &setting.description, Theme::TEXT_SECONDARY.to_u32());
            }
            
            setting_y += 60;
        }
    }
    
    fn get_pane_icon(&self, icon: PreferenceIcon) -> &'static str {
        match icon {
            PreferenceIcon::General => "⚙",
            PreferenceIcon::DesktopScreenSaver => "🖼",
            PreferenceIcon::DockMenuBar => "📱",
            PreferenceIcon::MissionControl => "⊞",
            PreferenceIcon::LanguageRegion => "🌐",
            PreferenceIcon::Notifications => "🔔",
            PreferenceIcon::InternetAccounts => "☁",
            PreferenceIcon::Wallet => "💳",
            PreferenceIcon::TouchID => "👆",
            PreferenceIcon::UsersGroups => "👤",
            PreferenceIcon::Accessibility => "♿",
            PreferenceIcon::SecurityPrivacy => "🔒",
            PreferenceIcon::Network => "📡",
            PreferenceIcon::Bluetooth => "🔵",
            PreferenceIcon::Sound => "🔊",
            PreferenceIcon::Keyboard => "⌨",
            PreferenceIcon::Trackpad => "🖱",
            PreferenceIcon::Mouse => "🖱",
            PreferenceIcon::Displays => "🖥",
            PreferenceIcon::Battery => "🔋",
            PreferenceIcon::Date => "📅",
            PreferenceIcon::Sharing => "📤",
            PreferenceIcon::TimeMachine => "⏰",
            PreferenceIcon::StartupDisk => "💾",
            PreferenceIcon::Extensions => "🧩",
            PreferenceIcon::Custom(_) => "📄",
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> PreferencesAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        
        // Back button
        if self.selected_pane.is_some() {
            if mx >= x + 8 && mx < x + 36
                && my >= y + 8 && my < y + 36 {
                self.go_back();
                return PreferencesAction::None;
            }
        }
        
        // Search field
        let search_x = x + w - 180;
        if mx >= search_x && mx < search_x + 160
            && my >= y + 8 && my < y + 36 {
            self.search_focused = true;
            return PreferencesAction::None;
        }
        
        // Pane grid or content
        let content_y = y + TOOLBAR_HEIGHT as i32;
        
        if self.selected_pane.is_none() {
            // Sidebar
            if mx >= x && mx < x + SIDEBAR_WIDTH as i32 && my >= content_y {
                let mut item_y = content_y + 8;
                
                for pane in self.panes.iter() {
                    if !pane.visible {
                        continue;
                    }
                    
                    // Skip category headers
                    if my >= item_y && my < item_y + 28 {
                        if let Some(idx) = self.panes.iter().position(|p| p.id == pane.id) {
                            self.select_pane(idx);
                            return PreferencesAction::None;
                        }
                    }
                    item_y += 28;
                }
            }
            
            // Icon grid
            let main_x = x + SIDEBAR_WIDTH as i32;
            let main_w = (w as usize) - SIDEBAR_WIDTH;
            let cols = main_w / (PANE_ICON_SIZE + 32);
            
            if mx >= main_x && my >= content_y {
                let rel_x = mx - main_x;
                let rel_y = my - content_y;
                
                let col = rel_x as usize / (PANE_ICON_SIZE + 32);
                let row = rel_y as usize / (PANE_ICON_SIZE + 48);
                
                let idx = row * cols + col;
                let mut visible_idx = 0;
                
                for (i, pane) in self.panes.iter().enumerate() {
                    if pane.visible {
                        if visible_idx == idx {
                            self.select_pane(i);
                            return PreferencesAction::None;
                        }
                        visible_idx += 1;
                    }
                }
            }
        } else if let Some(pane_idx) = self.selected_pane {
            // Setting controls
            let content_x = x + SIDEBAR_WIDTH as i32;
            let content_w = (w as usize) - SIDEBAR_WIDTH;
            
            let mut setting_y = content_y + 20;
            let pane = &self.panes[pane_idx];
            
            for setting in &pane.settings {
                let control_x = content_x + (content_w - 200) as i32;
                
                if my >= setting_y && my < setting_y + 40 {
                    match &setting.setting_type {
                        SettingType::Toggle => {
                            if mx >= control_x && mx < control_x + 44 {
                                // Toggle the setting
                                return PreferencesAction::SettingChanged(setting.id.clone());
                            }
                        }
                        SettingType::Slider { .. } => {
                            if mx >= control_x as i32 && mx < (control_x + 180) as i32 {
                                // Would adjust slider
                                return PreferencesAction::SettingChanged(setting.id.clone());
                            }
                        }
                        SettingType::Dropdown { .. } => {
                            if mx >= control_x as i32 && mx < (control_x + 180) as i32 {
                                // Would show dropdown
                                return PreferencesAction::SettingChanged(setting.id.clone());
                            }
                        }
                        _ => {}
                    }
                }
                
                setting_y += 60;
            }
        }
        
        PreferencesAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> PreferencesAction {
        if self.search_focused {
            if c == '\x1b' { // Escape
                self.search_focused = false;
                self.search_query.clear();
                self.search();
            } else if c == '\n' { // Enter
                self.search_focused = false;
            } else if c == '\x08' { // Backspace
                self.search_query.pop();
                self.search();
            } else if !c.is_control() {
                self.search_query.push(c);
                self.search();
            }
            return PreferencesAction::None;
        }
        
        PreferencesAction::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Preferences actions
#[derive(Clone, Debug)]
pub enum PreferencesAction {
    None,
    SettingChanged(String),
    RestartRequired,
}

// ============================================================================
// GLOBAL SYSTEM PREFERENCES
// ============================================================================

lazy_static::lazy_static! {
    static ref PREFERENCES: Mutex<SystemPreferences> = Mutex::new(SystemPreferences::new(Rect {
        x: 100,
        y: 100,
        width: 900,
        height: 600,
    }));
}

/// Initialize System Preferences
pub fn init() {
    crate::serial_println!("[GUI] System Preferences initialized");
}

/// Get System Preferences
pub fn get_preferences() -> &'static Mutex<SystemPreferences> {
    &PREFERENCES
}
