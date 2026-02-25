//! # macOS-style Dock
//!
//! Animated dock with magnification effect on hover
//! Supports app indicators, badges, and drag-drop reordering

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::animation::{Animation, EasingType, AnimationTarget, AnimationTargetType};

// ============================================================================
// DOCK CONSTANTS
// ============================================================================

/// Default dock height
pub const DOCK_HEIGHT: usize = 70;

/// Default icon size
pub const ICON_SIZE: usize = 48;

/// Maximum magnified icon size
pub const MAX_ICON_SIZE: usize = 80;

/// Icon spacing
pub const ICON_SPACING: usize = 8;

/// Magnification radius (how far the effect spreads)
pub const MAG_RADIUS: usize = 100;

// ============================================================================
// DOCK ITEM
// ============================================================================

/// A single dock item
pub struct DockItem {
    /// Unique ID
    pub id: u32,
    /// Display name
    pub name: String,
    /// Icon type
    pub icon: DockIcon,
    /// Is application running
    pub running: bool,
    /// Is application active (frontmost)
    pub active: bool,
    /// Badge count (notifications)
    pub badge_count: u32,
    /// Progress (0.0 - 1.0)
    pub progress: f32,
    /// Action when clicked
    pub action: DockAction,
    /// Current display size (for animation)
    pub current_size: f32,
    /// Target size (for animation)
    pub target_size: f32,
    /// Current Y offset (for bounce animation)
    pub bounce_offset: f32,
    /// Bounce velocity
    pub bounce_velocity: f32,
    /// Is bouncing
    pub bouncing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockIcon {
    Finder,
    Launchpad,
    Settings,
    Safari,
    Mail,
    Messages,
    Maps,
    Photos,
    Music,
    Notes,
    Calendar,
    Terminal,
    Files,
    TextEdit,
    Calculator,
    Trash,
    Downloads,
    Custom(u16),
}

#[derive(Clone, Debug)]
pub enum DockAction {
    LaunchApp(String),
    OpenFolder(String),
    OpenSettings,
    EmptyTrash,
    ShowLaunchpad,
    None,
}

impl DockItem {
    pub fn new(id: u32, name: &str, icon: DockIcon) -> Self {
        DockItem {
            id,
            name: String::from(name),
            icon,
            running: false,
            active: false,
            badge_count: 0,
            progress: 0.0,
            action: DockAction::None,
            current_size: ICON_SIZE as f32,
            target_size: ICON_SIZE as f32,
            bounce_offset: 0.0,
            bounce_velocity: 0.0,
            bouncing: false,
        }
    }
    
    /// Create app dock item
    pub fn app(id: u32, name: &str, icon: DockIcon, app_id: &str) -> Self {
        let mut item = Self::new(id, name, icon);
        item.action = DockAction::LaunchApp(String::from(app_id));
        item
    }
    
    /// Create folder dock item
    pub fn folder(id: u32, name: &str, path: &str) -> Self {
        let mut item = Self::new(id, name, DockIcon::Files);
        item.action = DockAction::OpenFolder(String::from(path));
        item
    }
    
    /// Start bounce animation
    pub fn start_bounce(&mut self) {
        self.bouncing = true;
        self.bounce_velocity = -15.0; // Initial upward velocity
    }
    
    /// Update bounce animation
    pub fn update_bounce(&mut self, dt: f32) {
        if !self.bouncing {
            return;
        }
        
        // Apply gravity
        self.bounce_velocity += 0.8; // Gravity
        self.bounce_offset += self.bounce_velocity;
        
        // Check if landed
        if self.bounce_offset >= 0.0 {
            self.bounce_offset = 0.0;
            self.bounce_velocity = -self.bounce_velocity * 0.5; // Bounce with damping
            
            // Stop if velocity is too low
            if self.bounce_velocity.abs() < 2.0 {
                self.bouncing = false;
                self.bounce_offset = 0.0;
                self.bounce_velocity = 0.0;
            }
        }
    }
    
    /// Update size with smooth animation
    pub fn update_size(&mut self, dt: f32) {
        let diff = self.target_size - self.current_size;
        if diff.abs() > 0.1 {
            self.current_size += diff * 0.3; // Smooth interpolation
        } else {
            self.current_size = self.target_size;
        }
    }
    
    /// Draw the dock item
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, dock_y: usize) {
        let icon_size = size;
        let icon_x = x;
        let icon_y = y as i32 - (self.bounce_offset as i32);
        
        // Draw shadow
        let shadow_size = icon_size + 4;
        let shadow_y = dock_y + DOCK_HEIGHT - 10;
        fb.draw_rect(icon_x - 2, shadow_y, shadow_size, 4, 0x40000000);
        
        // Draw icon background (rounded rect)
        let bg_color = if self.active {
            0x40FFFFFF // Brighter for active
        } else if self.running {
            0x20FFFFFF // Dimmer for running
        } else {
            0x10FFFFFF // Very dim for not running
        };
        
        self.draw_rounded_icon_bg(fb, icon_x, icon_y as usize, icon_size, bg_color);
        
        // Draw icon
        self.draw_icon(fb, icon_x, icon_y as usize, icon_size);
        
        // Draw running indicator (dot below)
        if self.running {
            let dot_y = dock_y + DOCK_HEIGHT - 6;
            let dot_size = if self.active { 5 } else { 4 };
            let dot_color = if self.active { 
                Theme::ACCENT_PRIMARY.to_u32() 
            } else { 
                0x80FFFFFF 
            };
            
            let dot_x = icon_x + icon_size / 2 - dot_size / 2;
            self.draw_dot(fb, dot_x, dot_y, dot_size, dot_color);
        }
        
        // Draw badge
        if self.badge_count > 0 {
            let badge_x = icon_x + icon_size - 16;
            let badge_y = icon_y as usize - 4;
            
            // Badge background
            for py in 0..16 {
                for px in 0..16 {
                    let dx = px as i32 - 8;
                    let dy = py as i32 - 8;
                    if dx * dx + dy * dy <= 64 {
                        fb.plot_pixel(badge_x + px, badge_y + py, Theme::ERROR.to_u32());
                    }
                }
            }
            
            // Badge text
            if self.badge_count < 10 {
                let digit = char::from(b'0' + self.badge_count as u8);
                fb.draw_char(badge_x + 5, badge_y + 2, digit, Theme::TEXT_ON_ACCENT.to_u32());
            } else {
                fb.draw_string(badge_x + 2, badge_y + 2, "9+", Theme::TEXT_ON_ACCENT.to_u32());
            }
        }
        
        // Draw progress bar
        if self.progress > 0.0 {
            let bar_width = (icon_size as f32 * self.progress) as usize;
            let bar_y = dock_y + DOCK_HEIGHT - 3;
            fb.draw_rect(icon_x, bar_y, bar_width, 2, Theme::ACCENT_PRIMARY.to_u32());
        }
    }
    
    fn draw_rounded_icon_bg(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, color: u32) {
        let radius = size / 5;
        
        for py in 0..size {
            for px in 0..size {
                let in_corner = 
                    (px < radius && py < radius && 
                     (radius - px) as i32 * (radius - px) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px >= size - radius && py < radius && 
                     (px - (size - radius)) as i32 * (px - (size - radius)) as i32 + (radius - py) as i32 * (radius - py) as i32 > radius as i32 * radius as i32) ||
                    (px < radius && py >= size - radius && 
                     (radius - px) as i32 * (radius - px) as i32 + (py - (size - radius)) as i32 * (py - (size - radius)) as i32 > radius as i32 * radius as i32) ||
                    (px >= size - radius && py >= size - radius && 
                     (px - (size - radius)) as i32 * (px - (size - radius)) as i32 + (py - (size - radius)) as i32 * (py - (size - radius)) as i32 > radius as i32 * radius as i32);
                
                if !in_corner {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }
    
    fn draw_dot(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize, color: u32) {
        let radius = size / 2;
        for py in 0..size {
            for px in 0..size {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }
    
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        let center_x = x + size / 2;
        let center_y = y + size / 2;
        let icon_scale = size as f32 / ICON_SIZE as f32;
        
        match self.icon {
            DockIcon::Finder => {
                // Smiley face icon
                let face_color = 0xFF3D67FF; // Blue
                self.draw_circle(fb, center_x, center_y, (size as f32 * 0.4) as usize, face_color);
                
                // Eyes
                let eye_y = center_y - size / 8;
                fb.draw_rect(center_x - size / 5, eye_y, 4, 4, 0xFFFFFFFF);
                fb.draw_rect(center_x + size / 5 - 4, eye_y, 4, 4, 0xFFFFFFFF);
                
                // Smile
                let smile_y = center_y + size / 10;
                fb.draw_rect(center_x - size / 6, smile_y, size / 3, 2, 0xFFFFFFFF);
            }
            
            DockIcon::Launchpad => {
                // Grid of dots
                let dot_size = (6.0 * icon_scale) as usize;
                let spacing = (14.0 * icon_scale) as usize;
                let start_x = center_x - spacing;
                let start_y = center_y - spacing;
                
                for row in 0..3 {
                    for col in 0..3 {
                        let dot_x = start_x + col * spacing - dot_size / 2;
                        let dot_y = start_y + row * spacing - dot_size / 2;
                        let color = if (row + col) % 2 == 0 { 
                            Theme::ACCENT_PRIMARY.to_u32() 
                        } else { 
                            0xFF666666 
                        };
                        fb.draw_rect(dot_x, dot_y, dot_size, dot_size, color);
                    }
                }
            }
            
            DockIcon::Settings => {
                // Gear icon
                let outer_r = (size as f32 * 0.35) as usize;
                let inner_r = (size as f32 * 0.15) as usize;
                
                // Outer gear teeth
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let tooth_x = center_x as i32 + (cosf(a) * outer_r as f32) as i32;
                    let tooth_y = center_y as i32 + (sinf(a) * outer_r as f32) as i32;
                    let tooth_size = (8.0 * icon_scale) as usize;
                    fb.draw_rect(
                        ((tooth_x - tooth_size as i32 / 2).max(0)) as usize,
                        ((tooth_y - tooth_size as i32 / 2).max(0)) as usize,
                        tooth_size, tooth_size,
                        0xFF888888
                    );
                }
                
                // Center circle
                self.draw_circle(fb, center_x, center_y, inner_r, 0xFF666666);
            }
            
            DockIcon::Safari => {
                // Compass icon
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle_outline(fb, center_x, center_y, r, 0xFF007AFF);
                
                // Compass needle
                fb.draw_rect(center_x - 2, center_y - r + 4, 4, r - 4, 0xFFFF3B30);
                fb.draw_rect(center_x - 2, center_y + 4, 4, r - 4, 0xFFFFFFFF);
            }
            
            DockIcon::Mail => {
                // Envelope icon
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.4) as usize;
                let mail_x = center_x - w / 2;
                let mail_y = center_y - h / 2;
                
                fb.draw_rect(mail_x, mail_y, w, h, 0xFF007AFF);
                
                // Envelope flap
                for i in 0..w/2 {
                    let flap_y = mail_y + i * h / w;
                    fb.plot_pixel(mail_x + i, flap_y, 0xFF0055D5);
                    fb.plot_pixel(mail_x + w - 1 - i, flap_y, 0xFF0055D5);
                }
            }
            
            DockIcon::Messages => {
                // Speech bubble
                let r = (size as f32 * 0.3) as usize;
                self.draw_circle(fb, center_x - 4, center_y - 4, r, 0xFF34C759);
                
                // Tail
                fb.draw_rect(center_x + r / 2, center_y + r / 2, 8, 8, 0xFF34C759);
            }
            
            DockIcon::Music => {
                // Music note
                let note_color = 0xFFFC3C44;
                let note_size = (size as f32 * 0.3) as usize;
                
                // Note head
                self.draw_ellipse(fb, center_x - note_size / 3, center_y + note_size / 2, 
                                  note_size, note_size / 2, note_color);
                
                // Stem
                fb.draw_rect(center_x + note_size / 3, center_y - note_size, 3, note_size * 2, note_color);
                
                // Flag
                fb.draw_rect(center_x + note_size / 3, center_y - note_size, note_size / 2, 4, note_color);
            }
            
            DockIcon::Terminal => {
                // Terminal window
                let w = (size as f32 * 0.7) as usize;
                let h = (size as f32 * 0.6) as usize;
                let term_x = center_x - w / 2;
                let term_y = center_y - h / 2;
                
                fb.draw_rect(term_x, term_y, w, h, 0xFF1E1E1E);
                fb.draw_rect(term_x, term_y, w, h / 4, 0xFF333333);
                
                // Prompt
                fb.draw_string(term_x + 4, term_y + h / 4 + 2, ">_", 0xFF00FF00);
            }
            
            DockIcon::Files => {
                // Folder icon
                let folder_color = 0xFF007AFF;
                let w = (size as f32 * 0.7) as usize;
                let h = (size as f32 * 0.55) as usize;
                let folder_x = center_x - w / 2;
                let folder_y = center_y - h / 2;
                
                // Tab
                fb.draw_rect(folder_x, folder_y, w / 2, h / 4, folder_color);
                // Body
                fb.draw_rect(folder_x, folder_y + h / 4, w, h * 3 / 4, folder_color);
            }
            
            DockIcon::Trash => {
                // Trash can
                let trash_color = 0xFF8E8E93;
                let w = (size as f32 * 0.5) as usize;
                let h = (size as f32 * 0.6) as usize;
                let trash_x = center_x - w / 2;
                let trash_y = center_y - h / 2;
                
                // Lid
                fb.draw_rect(trash_x - 2, trash_y, w + 4, h / 5, trash_color);
                // Body
                fb.draw_rect(trash_x, trash_y + h / 5, w, h * 4 / 5, trash_color);
                
                // Lines
                for i in 1..4 {
                    let line_x = trash_x + i as usize * w / 4;
                    fb.draw_rect(line_x, trash_y + h / 5 + 2, 2, h * 3 / 5, 0xFF666666);
                }
            }
            
            DockIcon::TextEdit => {
                // Document with text
                let doc_color = 0xFFFFCC00;
                let w = (size as f32 * 0.5) as usize;
                let h = (size as f32 * 0.65) as usize;
                let doc_x = center_x - w / 2;
                let doc_y = center_y - h / 2;
                
                fb.draw_rect(doc_x, doc_y, w, h, doc_color);
                
                // Fold corner
                fb.draw_rect(doc_x + w - 6, doc_y, 6, 6, 0xFFE6B800);
                
                // Text lines
                for i in 0..3 {
                    let line_y = doc_y + 10 + i * 6;
                    let line_w = w - 4 - i as usize * 3;
                    fb.draw_rect(doc_x + 2, line_y, line_w, 2, 0xFF333333);
                }
            }
            
            DockIcon::Calculator => {
                // Calculator
                let calc_color = 0xFF1C1C1E;
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.75) as usize;
                let calc_x = center_x - w / 2;
                let calc_y = center_y - h / 2;
                
                fb.draw_rect(calc_x, calc_y, w, h, calc_color);
                
                // Display
                fb.draw_rect(calc_x + 2, calc_y + 2, w - 4, h / 4, 0xFF505050);
                
                // Buttons
                let btn_size = (w - 8) / 4;
                for row in 0..4 {
                    for col in 0..4 {
                        let btn_x = calc_x + 2 + col * (btn_size + 1);
                        let btn_y = calc_y + h / 4 + 4 + row * (btn_size + 1);
                        let btn_color = if row == 3 { 0xFFFF9500 } else { 0xFF333333 };
                        fb.draw_rect(btn_x, btn_y, btn_size, btn_size, btn_color);
                    }
                }
            }
            
            DockIcon::Calendar => {
                // Calendar icon with current day
                let cal_color = 0xFFFFFFFF;
                let w = (size as f32 * 0.65) as usize;
                let h = (size as f32 * 0.65) as usize;
                let cal_x = center_x - w / 2;
                let cal_y = center_y - h / 2;
                
                fb.draw_rect(cal_x, cal_y, w, h, cal_color);
                
                // Red top bar
                fb.draw_rect(cal_x, cal_y, w, h / 4, 0xFFFF3B30);
                
                // Day number (placeholder - would use real date)
                fb.draw_string(center_x - 8, center_y, "25", 0xFF333333);
            }
            
            DockIcon::Downloads => {
                // Down arrow in circle
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, 0xFF007AFF);
                
                // Arrow
                let arrow_h = r;
                let arrow_w = r / 2;
                fb.draw_rect(center_x - 3, center_y - arrow_h / 2, 6, arrow_h, 0xFFFFFFFF);
                fb.draw_rect(center_x - arrow_w, center_y, arrow_w, 3, 0xFFFFFFFF);
                fb.draw_rect(center_x + 3, center_y, arrow_w, 3, 0xFFFFFFFF);
            }
            
            DockIcon::Maps => {
                // Map pin
                let pin_color = 0xFFFF3B30;
                let pin_r = (size as f32 * 0.25) as usize;
                
                // Pin head
                self.draw_circle(fb, center_x, center_y - pin_r, pin_r, pin_color);
                
                // Pin point
                for i in 0..pin_r {
                    let w = pin_r * 2 - i * 2;
                    fb.draw_rect(center_x - w / 2, center_y + i, w, 1, pin_color);
                }
            }
            
            DockIcon::Photos => {
                // Flower/petals icon
                let petal_r = (size as f32 * 0.2) as usize;
                let center_r = (size as f32 * 0.12) as usize;
                
                let colors = [0xFFFF2D55, 0xFFFF9500, 0xFFFFCC00, 0xFF34C759, 0xFF007AFF];
                
                for (i, &color) in colors.iter().enumerate() {
                    let angle = i as f32 * 2.0 * core::f32::consts::PI / 5.0;
                    let px = center_x as i32 + (cosf(angle) * petal_r as f32 * 1.2) as i32;
                    let py = center_y as i32 + (sinf(angle) * petal_r as f32 * 1.2) as i32;
                    self.draw_circle(fb, px.max(0) as usize, py.max(0) as usize, petal_r, color);
                }
                
                // Center
                self.draw_circle(fb, center_x, center_y, center_r, 0xFFFFFFFF);
            }
            
            DockIcon::Notes => {
                // Yellow notepad
                let note_color = 0xFFFFCC00;
                let w = (size as f32 * 0.6) as usize;
                let h = (size as f32 * 0.7) as usize;
                let note_x = center_x - w / 2;
                let note_y = center_y - h / 2;
                
                fb.draw_rect(note_x, note_y, w, h, note_color);
                
                // Lines
                for i in 1..4 {
                    let line_y = note_y + i as usize * h / 4;
                    fb.draw_rect(note_x + 4, line_y, w - 8, 1, 0xFFB38F00);
                }
            }
            
            DockIcon::Custom(code) => {
                // Custom icon - colored square with letter
                let color = match code % 8 {
                    0 => 0xFFFF3B30,
                    1 => 0xFFFF9500,
                    2 => 0xFFFFCC00,
                    3 => 0xFF34C759,
                    4 => 0xFF00C7BE,
                    5 => 0xFF007AFF,
                    6 => 0xFF5856D6,
                    _ => 0xFFFF2D55,
                };
                
                let r = (size as f32 * 0.35) as usize;
                self.draw_circle(fb, center_x, center_y, r, color);
                
                // Letter
                let letter = char::from(b'A' + (code % 26) as u8);
                fb.draw_char(center_x - 4, center_y - 6, letter, 0xFFFFFFFF);
            }
        }
    }
    
    fn draw_circle(&self, fb: &mut Framebuffer, x: usize, y: usize, radius: usize, color: u32) {
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    fb.plot_pixel(x + px - radius, y + py - radius, color);
                }
            }
        }
    }
    
    fn draw_circle_outline(&self, fb: &mut Framebuffer, x: usize, y: usize, radius: usize, color: u32) {
        for py in 0..radius * 2 {
            for px in 0..radius * 2 {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                let dist = dx * dx + dy * dy;
                if dist <= (radius * radius) as i32 && dist > ((radius - 2) * (radius - 2)) as i32 {
                    fb.plot_pixel(x + px - radius, y + py - radius, color);
                }
            }
        }
    }
    
    fn draw_ellipse(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for py in 0..h {
            for px in 0..w {
                let dx = (px as f32 / w as f32 - 0.5) * 2.0;
                let dy = (py as f32 / h as f32 - 0.5) * 2.0;
                if dx * dx + dy * dy <= 1.0 {
                    fb.plot_pixel(x + px, y + py, color);
                }
            }
        }
    }
}

// ============================================================================
// DOCK
// ============================================================================

/// macOS-style Dock with magnification
pub struct Dock {
    /// Dock items
    pub items: Vec<DockItem>,
    /// Next item ID
    next_id: u32,
    /// Dock position
    position: DockPosition,
    /// Dock visibility
    visible: bool,
    /// Auto-hide enabled
    auto_hide: bool,
    /// Is hidden (for auto-hide)
    hidden: bool,
    /// Hide animation progress (0.0 - 1.0)
    hide_progress: f32,
    /// Current mouse X position
    mouse_x: i32,
    /// Current mouse Y position
    mouse_y: i32,
    /// Hovered item index
    hovered_index: Option<usize>,
    /// Clicked item index
    clicked_index: Option<usize>,
    /// Dragging item index
    dragging_index: Option<usize>,
    /// Drag start position
    drag_start_x: i32,
    /// Screen width
    screen_width: usize,
    /// Screen height
    screen_height: usize,
    /// Dock Y position (animated)
    dock_y: f32,
    /// Magnification enabled
    magnification: bool,
    /// Magnification intensity
    mag_intensity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPosition {
    Bottom,
    Left,
    Right,
}

impl Dock {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut dock = Dock {
            items: Vec::new(),
            next_id: 1,
            position: DockPosition::Bottom,
            visible: true,
            auto_hide: false,
            hidden: false,
            hide_progress: 0.0,
            mouse_x: 0,
            mouse_y: 0,
            hovered_index: None,
            clicked_index: None,
            dragging_index: None,
            drag_start_x: 0,
            screen_width,
            screen_height,
            dock_y: screen_height as f32,
            magnification: true,
            mag_intensity: 1.0,
        };
        
        dock.add_default_items();
        dock
    }
    
    fn add_default_items(&mut self) {
        // Finder (always first)
        let mut finder = DockItem::app(self.next_id, "Finder", DockIcon::Finder, "finder");
        finder.running = true;
        self.items.push(finder);
        self.next_id += 1;
        
        // Launchpad
        let mut launchpad = DockItem::new(self.next_id, "Launchpad", DockIcon::Launchpad);
        launchpad.action = DockAction::ShowLaunchpad;
        self.items.push(launchpad);
        self.next_id += 1;
        
        // Safari
        self.items.push(DockItem::app(self.next_id, "Safari", DockIcon::Safari, "safari"));
        self.next_id += 1;
        
        // Mail
        self.items.push(DockItem::app(self.next_id, "Mail", DockIcon::Mail, "mail"));
        self.next_id += 1;
        
        // Messages
        self.items.push(DockItem::app(self.next_id, "Messages", DockIcon::Messages, "messages"));
        self.next_id += 1;
        
        // Music
        self.items.push(DockItem::app(self.next_id, "Music", DockIcon::Music, "music"));
        self.next_id += 1;
        
        // Terminal
        let mut terminal = DockItem::app(self.next_id, "Terminal", DockIcon::Terminal, "terminal");
        terminal.running = true;
        self.items.push(terminal);
        self.next_id += 1;
        
        // Files
        self.items.push(DockItem::app(self.next_id, "Files", DockIcon::Files, "files"));
        self.next_id += 1;
        
        // Settings
        self.items.push(DockItem::app(self.next_id, "Settings", DockIcon::Settings, "settings"));
        self.next_id += 1;
        
        // Separator (trash section)
        self.items.push(DockItem::new(self.next_id, "", DockIcon::Custom(100)));
        self.next_id += 1;
        
        // Trash
        self.items.push(DockItem::new(self.next_id, "Trash", DockIcon::Trash));
        self.next_id += 1;
        
        // Downloads
        self.items.push(DockItem::new(self.next_id, "Downloads", DockIcon::Downloads));
        self.next_id += 1;
    }
    
    /// Add item to dock
    pub fn add_item(&mut self, item: DockItem) {
        // Insert before trash section
        let insert_pos = self.items.len().saturating_sub(2);
        self.items.insert(insert_pos, item);
    }
    
    /// Remove item from dock
    pub fn remove_item(&mut self, id: u32) {
        self.items.retain(|i| i.id != id);
    }
    
    /// Update dock state
    pub fn update(&mut self, dt: f32) {
        // Update hide animation
        if self.auto_hide {
            let target_y = if self.hidden {
                self.screen_height as f32
            } else {
                (self.screen_height - DOCK_HEIGHT) as f32
            };
            
            self.dock_y += (target_y - self.dock_y) * 0.2;
        } else {
            self.dock_y = (self.screen_height - DOCK_HEIGHT) as f32;
        }
        
        // Update magnification
        if self.magnification {
            self.update_magnification();
        }
        
        // Update bounce animations
        for item in &mut self.items {
            item.update_bounce(dt);
            item.update_size(dt);
        }
    }
    
    /// Calculate magnification for each item
    fn update_magnification(&mut self) {
        if self.items.is_empty() {
            return;
        }
        
        // Calculate dock center position
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING);
        let dock_start = (self.screen_width - total_width) / 2;
        
        for (i, item) in self.items.iter_mut().enumerate() {
            let item_center_x = dock_start + i * (ICON_SIZE + ICON_SPACING) + ICON_SIZE / 2;
            
            // Distance from mouse
            let dist = (self.mouse_x - item_center_x as i32).abs() as f32;
            
            // Calculate magnification
            if dist < MAG_RADIUS as f32 {
                let factor = 1.0 - (dist / MAG_RADIUS as f32);
                let mag = 1.0 + factor * (MAX_ICON_SIZE as f32 / ICON_SIZE as f32 - 1.0) * self.mag_intensity;
                item.target_size = ICON_SIZE as f32 * mag;
            } else {
                item.target_size = ICON_SIZE as f32;
            }
        }
    }
    
    /// Draw the dock
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        
        let dock_y = self.dock_y as usize;
        
        // Draw dock background (glass effect)
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING) + ICON_SPACING * 2;
        let dock_x = (self.screen_width - total_width) / 2;
        
        // Background with blur effect (simplified)
        let bg_height = DOCK_HEIGHT + 4;
        let bg_y = dock_y;
        
        // Draw semi-transparent background
        for y in 0..bg_height {
            let alpha = if y < 4 { 0x20 } else if y > bg_height - 8 { 0x10 } else { 0x40 };
            let row_color = (alpha << 24) | 0x00FFFFFF;
            
            for x in 0..total_width {
                let px = dock_x + x;
                let py = bg_y + y;
                
                if px < self.screen_width && py < self.screen_height {
                    // Blend with background
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(py * fb.pixels_per_scan_line + px) };
                    let bg = unsafe { *ptr };
                    let blended = Self::blend_colors(bg, row_color);
                    unsafe { *ptr = blended; }
                }
            }
        }
        
        // Draw border
        fb.draw_rect(dock_x, dock_y, total_width, 1, 0x60FFFFFF);
        
        // Draw items with magnification
        let mut item_x = dock_x + ICON_SPACING;
        
        for (i, item) in self.items.iter().enumerate() {
            let size = item.current_size as usize;
            let x_offset = (ICON_SIZE as i32 - size as i32) / 2;
            let y_offset = (ICON_SIZE as i32 - size as i32) / 2;
            
            let draw_x = (item_x as i32 + x_offset) as usize;
            let draw_y = dock_y + DOCK_HEIGHT - size - 8 + y_offset as usize;
            
            item.draw(fb, draw_x, draw_y, size, dock_y);
            
            // Advance x position based on current size
            item_x += ((item.current_size + ICON_SPACING as f32) / 2.0 + ICON_SIZE as f32 / 2.0) as usize;
        }
    }
    
    fn blend_colors(bg: u32, fg: u32) -> u32 {
        let alpha = ((fg >> 24) & 0xFF) as f32 / 255.0;
        
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
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) -> DockEvent {
        self.mouse_x = mx;
        self.mouse_y = my;
        
        // Check if mouse is near dock for auto-hide
        if self.auto_hide {
            let dock_top = self.screen_height as i32 - DOCK_HEIGHT as i32 - 20;
            if my >= dock_top {
                self.hidden = false;
            } else if my < dock_top - 50 {
                self.hidden = true;
            }
        }
        
        // Find hovered item
        let total_width = self.items.len() * (ICON_SIZE + ICON_SPACING);
        let dock_start = (self.screen_width - total_width) / 2;
        
        let mut new_hovered = None;
        for (i, item) in self.items.iter().enumerate() {
            let item_center_x = dock_start + i * (ICON_SIZE + ICON_SPACING) + ICON_SIZE / 2;
            let item_size = item.current_size as i32;
            let half_size = item_size / 2;
            
            if mx >= item_center_x as i32 - half_size && mx <= item_center_x as i32 + half_size {
                new_hovered = Some(i);
                break;
            }
        }
        
        if new_hovered != self.hovered_index {
            self.hovered_index = new_hovered;
            
            if let Some(idx) = new_hovered {
                return DockEvent::ItemHovered(idx, self.items[idx].name.clone());
            }
        }
        
        DockEvent::None
    }
    
    /// Handle mouse down
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> DockEvent {
        if let Some(idx) = self.hovered_index {
            self.clicked_index = Some(idx);
            
            // Start bounce animation
            self.items[idx].start_bounce();
            
            return DockEvent::ItemClicked(idx, self.items[idx].id);
        }
        
        DockEvent::None
    }
    
    /// Handle mouse up
    pub fn on_mouse_up(&mut self) -> DockEvent {
        if let Some(clicked_idx) = self.clicked_index {
            if self.hovered_index == Some(clicked_idx) {
                let item = &self.items[clicked_idx];
                let action = item.action.clone();
                
                self.clicked_index = None;
                
                return DockEvent::ItemActivated(clicked_idx, item.id, action);
            }
        }
        
        self.clicked_index = None;
        DockEvent::None
    }
    
    /// Set item running state
    pub fn set_item_running(&mut self, id: u32, running: bool) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.running = running;
            if running {
                item.start_bounce();
            }
        }
    }
    
    /// Set item active state
    pub fn set_item_active(&mut self, id: u32, active: bool) {
        // Deactivate all others first
        for item in &mut self.items {
            item.active = false;
        }
        
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.active = active;
        }
    }
    
    /// Set item badge
    pub fn set_item_badge(&mut self, id: u32, count: u32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.badge_count = count;
        }
    }
    
    /// Set item progress
    pub fn set_item_progress(&mut self, id: u32, progress: f32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.progress = progress;
        }
    }
    
    /// Resize dock
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
    
    /// Get dock height
    pub fn height(&self) -> usize {
        if self.visible && !self.hidden {
            DOCK_HEIGHT
        } else {
            0
        }
    }
    
    /// Set magnification
    pub fn set_magnification(&mut self, enabled: bool) {
        self.magnification = enabled;
    }
    
    /// Set magnification intensity
    pub fn set_mag_intensity(&mut self, intensity: f32) {
        self.mag_intensity = intensity.max(0.0).min(1.0);
    }
}

/// Dock events
#[derive(Clone, Debug)]
pub enum DockEvent {
    None,
    ItemHovered(usize, String),
    ItemClicked(usize, u32),
    ItemActivated(usize, u32, DockAction),
}

// ============================================================================
// GLOBAL DOCK
// ============================================================================

lazy_static::lazy_static! {
    static ref DOCK: Mutex<Dock> = Mutex::new(Dock::new(1920, 1080));
}

/// Initialize dock
pub fn init(width: usize, height: usize) {
    let mut dock = DOCK.lock();
    dock.resize(width, height);
    crate::serial_println!("[GUI] Dock initialized ({}x{})", width, height);
}

/// Get dock
pub fn get_dock() -> &'static Mutex<Dock> {
    &DOCK
}
