//! # Enhanced Taskbar
//!
//! Modern taskbar with start menu, pinned apps, system tray, and clock
//! Supports window previews, jump lists, and notifications

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// TASKBAR CONSTANTS
// ============================================================================

/// Default taskbar height
pub const TASKBAR_HEIGHT: usize = 48;

/// Icon size in taskbar
pub const ICON_SIZE: usize = 32;

/// Button width
pub const BUTTON_WIDTH: usize = 44;

/// Spacing between elements
pub const SPACING: usize = 4;

// ============================================================================
// TASKBAR BUTTON
// ============================================================================

/// A button in the taskbar
#[derive(Clone)]
pub struct TaskbarButton {
    /// Button ID
    id: u32,
    /// Button type
    button_type: ButtonType,
    /// Position
    x: usize,
    y: usize,
    /// Size
    width: usize,
    height: usize,
    /// Is hovered
    hovered: bool,
    /// Is pressed
    pressed: bool,
    /// Is active (for window buttons)
    active: bool,
    /// Icon (simplified - would be texture)
    icon_type: IconType,
    /// Tooltip text
    tooltip: String,
    /// Badge count (for notifications)
    badge_count: u32,
    /// Progress (0.0 - 1.0)
    progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonType {
    Start,
    Search,
    TaskView,
    PinnedApp,
    RunningApp,
    SystemTray,
    Clock,
    ShowDesktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Start,
    Search,
    TaskView,
    FileExplorer,
    Settings,
    Terminal,
    Browser,
    Music,
    Video,
    Photos,
    Mail,
    Calendar,
    Calculator,
    Notes,
    Custom(u16),
}

impl TaskbarButton {
    pub fn new(id: u32, button_type: ButtonType, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Custom(0),
            tooltip: String::new(),
            badge_count: 0,
            progress: 0.0,
        }
    }
    
    /// Create start button
    pub fn start(id: u32, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::Start,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Start,
            tooltip: String::from("Start"),
            badge_count: 0,
            progress: 0.0,
        }
    }
    
    /// Create search button
    pub fn search(id: u32, x: usize) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::Search,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: IconType::Search,
            tooltip: String::from("Search"),
            badge_count: 0,
            progress: 0.0,
        }
    }
    
    /// Create pinned app button
    pub fn pinned_app(id: u32, x: usize, icon: IconType, tooltip: &str) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::PinnedApp,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active: false,
            icon_type: icon,
            tooltip: String::from(tooltip),
            badge_count: 0,
            progress: 0.0,
        }
    }
    
    /// Create running app button
    pub fn running_app(id: u32, x: usize, icon: IconType, tooltip: &str, active: bool) -> Self {
        TaskbarButton {
            id,
            button_type: ButtonType::RunningApp,
            x,
            y: SPACING,
            width: BUTTON_WIDTH,
            height: TASKBAR_HEIGHT - SPACING * 2,
            hovered: false,
            pressed: false,
            active,
            icon_type: icon,
            tooltip: String::from(tooltip),
            badge_count: 0,
            progress: 0.0,
        }
    }
    
    /// Set hovered state
    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }
    
    /// Set pressed state
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }
    
    /// Set active state
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }
    
    /// Set badge count
    pub fn set_badge(&mut self, count: u32) {
        self.badge_count = count;
    }
    
    /// Set progress
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.max(0.0).min(1.0);
    }
    
    /// Check if point is inside button
    pub fn hit_test(&self, x: i32, y: i32) -> bool {
        x >= self.x as i32 && x < (self.x + self.width) as i32
            && y >= self.y as i32 && y < (self.y + self.height) as i32
    }
    
    /// Get bounds
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x as i32, self.y as i32, self.width as i32, self.height as i32)
    }
    
    /// Draw the button
    pub fn draw(&self, fb: &mut Framebuffer, fb_width: usize, fb_height: usize) {
        let x = self.x;
        let y = fb_height - TASKBAR_HEIGHT + self.y;
        
        // Draw background
        let bg_color = if self.pressed {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else if self.active {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::TRANSPARENT.to_u32()
        };
        
        // Draw rounded rectangle background
        self.draw_rounded_rect(fb, x, y, self.width, self.height, 4, bg_color);
        
        // Draw icon
        let icon_x = x + (self.width - ICON_SIZE) / 2;
        let icon_y = y + (self.height - ICON_SIZE) / 2;
        self.draw_icon(fb, icon_x, icon_y, ICON_SIZE);
        
        // Draw active indicator (underline)
        if self.active {
            let indicator_y = y + self.height - 3;
            let indicator_width = self.width / 2;
            let indicator_x = x + (self.width - indicator_width) / 2;
            
            fb.draw_rect(
                indicator_x, indicator_y,
                indicator_width, 3,
                Theme::ACCENT_PRIMARY.to_u32()
            );
        }
        
        // Draw progress bar
        if self.progress > 0.0 {
            let progress_width = (self.width as f32 * self.progress) as usize;
            let progress_y = y + self.height - 2;
            
            fb.draw_rect(
                x, progress_y,
                progress_width, 2,
                Theme::ACCENT_PRIMARY.to_u32()
            );
        }
        
        // Draw badge
        if self.badge_count > 0 {
            let badge_x = x + self.width - 16;
            let badge_y = y + 4;
            let badge_radius = 8;
            
            // Badge background
            for py in 0..badge_radius * 2 {
                for px in 0..badge_radius * 2 {
                    let dx = px as i32 - badge_radius as i32;
                    let dy = py as i32 - badge_radius as i32;
                    if dx * dx + dy * dy < (badge_radius * badge_radius) as i32 {
                        fb.plot_pixel(badge_x + px, badge_y + py, Theme::ERROR.to_u32());
                    }
                }
            }
            
            // Badge text (simplified - just show count)
            if self.badge_count < 10 {
                let digit = char::from(b'0' + self.badge_count as u8);
                fb.draw_char(badge_x + 3, badge_y + 2, digit, Theme::TEXT_ON_ACCENT.to_u32());
            } else {
                fb.draw_string(badge_x, badge_y + 2, "9+", Theme::TEXT_ON_ACCENT.to_u32());
            }
        }
    }
    
    /// Draw rounded rectangle
    fn draw_rounded_rect(
        &self,
        fb: &mut Framebuffer,
        x: usize, y: usize,
        width: usize, height: usize,
        radius: usize,
        color: u32,
    ) {
        // Draw main rectangle
        fb.draw_rect(x + radius, y, width - radius * 2, height, color);
        fb.draw_rect(x, y + radius, width, height - radius * 2, color);
        
        // Draw corners (circle approximation)
        for py in 0..radius {
            for px in 0..radius {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    // Top-left
                    fb.plot_pixel(x + px, y + py, color);
                    // Top-right
                    fb.plot_pixel(x + width - radius + px, y + py, color);
                    // Bottom-left
                    fb.plot_pixel(x + px, y + height - radius + py, color);
                    // Bottom-right
                    fb.plot_pixel(x + width - radius + px, y + height - radius + py, color);
                }
            }
        }
    }
    
    /// Draw icon
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let color = Theme::TEXT_PRIMARY.to_u32();
        let accent = Theme::ACCENT_PRIMARY.to_u32();
        
        match self.icon_type {
            IconType::Start => {
                // Draw start icon (grid of 4 squares)
                let square_size = size / 3;
                let gap = 2;
                
                // Top-left
                fb.draw_rect(x + gap, y + gap, square_size - gap, square_size - gap, accent);
                // Top-right
                fb.draw_rect(x + square_size + gap, y + gap, square_size - gap, square_size - gap, color);
                // Bottom-left
                fb.draw_rect(x + gap, y + square_size + gap, square_size - gap, square_size - gap, color);
                // Bottom-right
                fb.draw_rect(x + square_size + gap, y + square_size + gap, square_size - gap, square_size - gap, accent);
            }
            
            IconType::Search => {
                // Draw search icon (magnifying glass)
                let center = size / 3;
                let radius = size / 4;
                
                // Circle
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (radius * radius) as i32 
                            && dx * dx + dy * dy > ((radius - 2) * (radius - 2)) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }
                
                // Handle
                let handle_x = x + center + radius / 2;
                let handle_y = y + center + radius / 2;
                for i in 0..(size / 3) {
                    fb.plot_pixel(handle_x + i, handle_y + i, color);
                    fb.plot_pixel(handle_x + i + 1, handle_y + i, color);
                }
            }
            
            IconType::TaskView => {
                // Draw task view icon (two overlapping rectangles)
                let rect_w = size / 2;
                let rect_h = size / 2;
                
                fb.draw_rect(x + 2, y + 2, rect_w - 2, rect_h - 2, color);
                fb.draw_rect(x + size / 3, y + size / 3, rect_w - 2, rect_h - 2, accent);
            }
            
            IconType::FileExplorer => {
                // Draw folder icon
                let tab_h = size / 4;
                fb.draw_rect(x + 2, y + 2, size / 2, tab_h, 0xFFC107);
                fb.draw_rect(x + 2, y + tab_h, size - 4, size - tab_h - 2, 0xFFC107);
            }
            
            IconType::Settings => {
                // Draw gear icon
                let center = size / 2;
                let outer_r = size / 3;
                let inner_r = size / 6;
                
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let tooth_x = x as i32 + center as i32 + (cosf(a) * outer_r as f32) as i32;
                    let tooth_y = y as i32 + center as i32 + (sinf(a) * outer_r as f32) as i32;
                    
                    fb.draw_rect(
                        (tooth_x - 2).max(0) as usize,
                        (tooth_y - 2).max(0) as usize,
                        4, 4, color
                    );
                }
                
                // Center circle
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (inner_r * inner_r) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }
            }
            
            IconType::Terminal => {
                // Draw terminal icon
                fb.draw_rect(x + 2, y + 2, size - 4, size - 4, 0x1E1E1E);
                
                // Prompt
                fb.draw_string(x + 4, y + 6, ">_", color);
            }
            
            IconType::Browser => {
                // Draw globe icon
                let center = size / 2;
                let radius = size / 3;
                
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center as i32;
                        let dy = py as i32 - center as i32;
                        if dx * dx + dy * dy <= (radius * radius) as i32 
                            && dx * dx + dy * dy > ((radius - 2) * (radius - 2)) as i32 {
                            fb.plot_pixel(x + px, y + py, accent);
                        }
                    }
                }
                
                // Horizontal and vertical lines
                for i in 0..(radius * 2) {
                    fb.plot_pixel(x + center - radius + i, y + center, accent);
                    fb.plot_pixel(x + center, y + center - radius + i, accent);
                }
            }
            
            IconType::Music => {
                // Draw music note
                let note_x = x + size / 3;
                let note_y = y + size / 4;
                let note_size = size / 4;
                
                // Note head
                for py in 0..note_size {
                    for px in 0..note_size {
                        let dx = px as i32 - note_size as i32 / 2;
                        let dy = py as i32 - note_size as i32 / 2;
                        if dx * dx + dy * dy <= (note_size * note_size / 4) as i32 {
                            fb.plot_pixel(note_x + px, note_y + py, accent);
                        }
                    }
                }
                
                // Stem
                fb.draw_rect(note_x + note_size - 2, note_y, 2, size / 2, accent);
                
                // Flag
                fb.draw_rect(note_x + note_size - 2, note_y, size / 4, 3, accent);
            }
            
            _ => {
                // Default: colored square
                fb.draw_rect(x + 4, y + 4, size - 8, size - 8, accent);
            }
        }
    }
    
    /// Get button type
    pub fn button_type(&self) -> ButtonType {
        self.button_type
    }
    
    /// Get ID
    pub fn id(&self) -> u32 {
        self.id
    }
}

// ============================================================================
// SYSTEM TRAY ITEM
// ============================================================================

/// System tray item
pub struct SystemTrayItem {
    id: u32,
    icon_type: IconType,
    tooltip: String,
    x: usize,
    hovered: bool,
}

impl SystemTrayItem {
    pub fn new(id: u32, icon_type: IconType, tooltip: &str) -> Self {
        SystemTrayItem {
            id,
            icon_type,
            tooltip: String::from(tooltip),
            x: 0,
            hovered: false,
        }
    }
    
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        // Draw small icon (16x16)
        let color = Theme::TEXT_PRIMARY.to_u32();
        
        match self.icon_type {
            IconType::Custom(100) => {
                // WiFi icon
                let bars = [4, 8, 12, 16];
                for (i, &bar_h) in bars.iter().enumerate() {
                    let bar_x = x + i * 4;
                    let bar_y = y + 16 - bar_h;
                    fb.draw_rect(bar_x, bar_y, 3, bar_h, color);
                }
            }
            IconType::Custom(101) => {
                // Volume icon
                fb.draw_rect(x + 2, y + 6, 6, 8, color);
                fb.draw_rect(x + 8, y + 4, 6, 12, color);
            }
            IconType::Custom(102) => {
                // Battery icon
                fb.draw_rect(x + 2, y + 4, 12, 12, color);
                fb.draw_rect(x + 14, y + 6, 2, 8, color);
                // Fill level
                fb.draw_rect(x + 4, y + 6, 8, 8, Theme::ACCENT_SUCCESS.to_u32());
            }
            IconType::Custom(103) => {
                // Network icon
                fb.draw_rect(x + 2, y + 10, 12, 6, color);
                fb.draw_rect(x + 6, y + 4, 4, 6, color);
            }
            _ => {
                // Default icon
                fb.draw_rect(x + 2, y + 2, 12, 12, color);
            }
        }
    }
    
    pub fn hit_test(&self, mx: i32, my: i32, x: usize, y: usize) -> bool {
        mx >= x as i32 && mx < (x + 16) as i32 && my >= y as i32 && my < (y + 16) as i32
    }
}

// ============================================================================
// ENHANCED TASKBAR
// ============================================================================

/// Enhanced taskbar with all features
pub struct EnhancedTaskbar {
    /// Taskbar position
    position: TaskbarPosition,
    /// Height
    height: usize,
    /// Width (screen width)
    width: usize,
    /// Left buttons (start, search, task view, pinned apps)
    left_buttons: Vec<TaskbarButton>,
    /// Center buttons (running apps)
    center_buttons: Vec<TaskbarButton>,
    /// System tray items
    system_tray: Vec<SystemTrayItem>,
    /// Clock string
    clock_string: String,
    /// Date string
    date_string: String,
    /// Next button ID
    next_id: u32,
    /// Hovered button
    hovered_button: Option<u32>,
    /// Pressed button
    pressed_button: Option<u32>,
    /// Auto-hide enabled
    auto_hide: bool,
    /// Is visible (for auto-hide)
    visible: bool,
    /// Last mouse position
    last_mouse: (i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarPosition {
    Bottom,
    Top,
    Left,
    Right,
}

impl EnhancedTaskbar {
    pub fn new(width: usize) -> Self {
        let mut taskbar = EnhancedTaskbar {
            position: TaskbarPosition::Bottom,
            height: TASKBAR_HEIGHT,
            width,
            left_buttons: Vec::new(),
            center_buttons: Vec::new(),
            system_tray: Vec::new(),
            clock_string: String::from("00:00"),
            date_string: String::from("Jan 1"),
            next_id: 1,
            hovered_button: None,
            pressed_button: None,
            auto_hide: false,
            visible: true,
            last_mouse: (0, 0),
        };
        
        // Add default buttons
        taskbar.add_default_buttons();
        
        // Add default system tray
        taskbar.add_default_tray();
        
        taskbar
    }
    
    /// Add default buttons
    fn add_default_buttons(&mut self) {
        let mut x = SPACING;
        
        // Start button
        self.left_buttons.push(TaskbarButton::start(self.next_id, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;
        
        // Search button
        self.left_buttons.push(TaskbarButton::search(self.next_id, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;
        
        // Task view button
        self.left_buttons.push(TaskbarButton::new(self.next_id, ButtonType::TaskView, x));
        self.next_id += 1;
        x += BUTTON_WIDTH + SPACING;
        
        // Pinned apps
        let pinned_apps = [
            (IconType::FileExplorer, "File Explorer"),
            (IconType::Browser, "Browser"),
            (IconType::Terminal, "Terminal"),
            (IconType::Settings, "Settings"),
        ];
        
        for (icon, tooltip) in pinned_apps {
            self.left_buttons.push(TaskbarButton::pinned_app(self.next_id, x, icon, tooltip));
            self.next_id += 1;
            x += BUTTON_WIDTH + SPACING;
        }
    }
    
    /// Add default system tray items
    fn add_default_tray(&mut self) {
        self.system_tray.push(SystemTrayItem::new(100, IconType::Custom(100), "Network"));
        self.system_tray.push(SystemTrayItem::new(101, IconType::Custom(101), "Volume"));
        self.system_tray.push(SystemTrayItem::new(102, IconType::Custom(102), "Battery"));
    }
    
    /// Add running app
    pub fn add_running_app(&mut self, icon: IconType, tooltip: &str, active: bool) -> u32 {
        let x = self.center_buttons.len() * (BUTTON_WIDTH + SPACING);
        let button = TaskbarButton::running_app(self.next_id, x, icon, tooltip, active);
        self.center_buttons.push(button);
        self.next_id += 1;
        self.next_id - 1
    }
    
    /// Remove running app
    pub fn remove_running_app(&mut self, id: u32) {
        self.center_buttons.retain(|b| b.id != id);
        // Recalculate positions
        for (i, button) in self.center_buttons.iter_mut().enumerate() {
            button.x = i * (BUTTON_WIDTH + SPACING);
        }
    }
    
    /// Update clock
    pub fn update_clock(&mut self, hours: u8, minutes: u8, month: u8, day: u8) {
        self.clock_string = format!("{:02}:{:02}", hours, minutes);
        
        let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", 
                          "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        if month > 0 && month <= 12 {
            self.date_string = format!("{} {}", month_names[(month - 1) as usize], day);
        }
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, x: i32, y: i32, fb_height: usize) -> TaskbarEvent {
        self.last_mouse = (x, y);
        
        // Check auto-hide
        if self.auto_hide {
            let taskbar_y = fb_height as i32 - self.height as i32;
            if y >= taskbar_y {
                self.visible = true;
            } else if y < taskbar_y - 20 {
                self.visible = false;
            }
        }
        
        // Check button hover
        let mut found_hover = false;
        
        for button in self.left_buttons.iter_mut() {
            let was_hovered = button.hovered;
            button.hovered = button.hit_test(x, y);
            
            if button.hovered && !was_hovered {
                self.hovered_button = Some(button.id);
                return TaskbarEvent::ButtonHovered(button.id, button.tooltip.clone());
            }
            if button.hovered {
                found_hover = true;
            }
        }
        
        for button in self.center_buttons.iter_mut() {
            let was_hovered = button.hovered;
            button.hovered = button.hit_test(x, y);
            
            if button.hovered && !was_hovered {
                self.hovered_button = Some(button.id);
                return TaskbarEvent::ButtonHovered(button.id, button.tooltip.clone());
            }
            if button.hovered {
                found_hover = true;
            }
        }
        
        if !found_hover {
            self.hovered_button = None;
        }
        
        TaskbarEvent::None
    }
    
    /// Handle mouse down
    pub fn on_mouse_down(&mut self, x: i32, y: i32) -> TaskbarEvent {
        // Check left buttons
        for button in self.left_buttons.iter_mut() {
            if button.hit_test(x, y) {
                button.pressed = true;
                self.pressed_button = Some(button.id);
                
                return match button.button_type {
                    ButtonType::Start => TaskbarEvent::StartMenuRequested,
                    ButtonType::Search => TaskbarEvent::SearchRequested,
                    ButtonType::TaskView => TaskbarEvent::TaskViewRequested,
                    ButtonType::PinnedApp => TaskbarEvent::AppLaunched(button.icon_type),
                    _ => TaskbarEvent::ButtonPressed(button.id),
                };
            }
        }
        
        // Check center buttons (running apps)
        for button in self.center_buttons.iter_mut() {
            if button.hit_test(x, y) {
                button.pressed = true;
                self.pressed_button = Some(button.id);
                return TaskbarEvent::WindowActivated(button.id);
            }
        }
        
        // Check system tray
        let tray_x = self.width - 100;
        let tray_y = y as usize;
        for item in &self.system_tray {
            if item.hit_test(x, y, tray_x, tray_y) {
                return TaskbarEvent::TrayItemClicked(item.id);
            }
        }
        
        TaskbarEvent::None
    }
    
    /// Handle mouse up
    pub fn on_mouse_up(&mut self) {
        for button in self.left_buttons.iter_mut() {
            button.pressed = false;
        }
        for button in self.center_buttons.iter_mut() {
            button.pressed = false;
        }
        self.pressed_button = None;
    }
    
    /// Draw the taskbar
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        
        let y = fb.height - self.height;
        
        // Draw background
        fb.draw_rect(0, y, self.width, self.height, Theme::TASKBAR_BG.to_u32());
        
        // Draw top border
        fb.draw_rect(0, y, self.width, 1, Theme::BORDER.to_u32());
        
        // Draw left buttons
        for button in &self.left_buttons {
            button.draw(fb, self.width, fb.height);
        }
        
        // Draw center buttons (running apps)
        let center_start = self.width / 2 - (self.center_buttons.len() * (BUTTON_WIDTH + SPACING)) / 2;
        for (i, button) in self.center_buttons.iter().enumerate() {
            let mut btn = button.clone();
            btn.x = center_start + i * (BUTTON_WIDTH + SPACING);
            btn.draw(fb, self.width, fb.height);
        }
        
        // Draw system tray
        let mut tray_x = self.width - 16;
        for item in self.system_tray.iter().rev() {
            item.draw(fb, tray_x, y + (self.height - 16) / 2);
            tray_x -= 20;
        }
        
        // Draw clock and date
        let clock_x = self.width - 100;
        let clock_y = y + 8;
        
        fb.draw_string(clock_x, clock_y, &self.clock_string, Theme::TEXT_PRIMARY.to_u32());
        fb.draw_string(clock_x, clock_y + 14, &self.date_string, Theme::TEXT_SECONDARY.to_u32());
        
        // Draw show desktop button (far right)
        let desktop_btn_x = self.width - 4;
        fb.draw_rect(desktop_btn_x, y + 4, 2, self.height - 8, Theme::BORDER.to_u32());
    }
    
    /// Resize taskbar
    pub fn resize(&mut self, width: usize) {
        self.width = width;
    }
    
    /// Get height
    pub fn height(&self) -> usize {
        if self.visible {
            self.height
        } else {
            0
        }
    }
    
    /// Set auto-hide
    pub fn set_auto_hide(&mut self, enabled: bool) {
        self.auto_hide = enabled;
    }
    
    /// Get running app button by ID
    pub fn get_app_button(&mut self, id: u32) -> Option<&mut TaskbarButton> {
        self.center_buttons.iter_mut().find(|b| b.id == id)
    }
}

/// Events from taskbar
#[derive(Clone, Debug)]
pub enum TaskbarEvent {
    None,
    ButtonPressed(u32),
    ButtonHovered(u32, String),
    StartMenuRequested,
    SearchRequested,
    TaskViewRequested,
    AppLaunched(IconType),
    WindowActivated(u32),
    TrayItemClicked(u32),
}

// ============================================================================
// GLOBAL TASKBAR
// ============================================================================

lazy_static::lazy_static! {
    static ref TASKBAR: Mutex<EnhancedTaskbar> = Mutex::new(EnhancedTaskbar::new(1920));
}

/// Initialize taskbar
pub fn init(width: usize) {
    let mut taskbar = TASKBAR.lock();
    taskbar.resize(width);
    crate::serial_println!("[GUI] Taskbar initialized ({}px wide)", width);
}

/// Get taskbar
pub fn get() -> &'static Mutex<EnhancedTaskbar> {
    &TASKBAR
}
