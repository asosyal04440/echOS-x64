//! # Mission Control
//!
//! macOS-style window overview with spaces
//! Shows all windows, desktop spaces, and dashboard

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// MISSION CONTROL CONSTANTS
// ============================================================================

/// Window thumbnail scale
pub const THUMBNAIL_SCALE: f32 = 0.25;

/// Space thumbnail width
pub const SPACE_WIDTH: usize = 200;

/// Space thumbnail height
pub const SPACE_HEIGHT: usize = 120;

/// Space spacing
pub const SPACE_SPACING: usize = 16;

/// Window spacing
pub const WINDOW_SPACING: usize = 20;

// ============================================================================
// WINDOW THUMBNAIL
// ============================================================================

/// A window thumbnail in Mission Control
#[derive(Clone, Debug)]
pub struct WindowThumbnail {
    /// Window ID
    pub window_id: u32,
    /// Window title
    pub title: String,
    /// App name
    pub app_name: String,
    /// Original position and size
    pub original_rect: (i32, i32, usize, usize), // x, y, w, h
    /// Thumbnail position (animated)
    pub thumbnail_pos: (f32, f32),
    /// Thumbnail size
    pub thumbnail_size: (usize, usize),
    /// Is selected
    pub selected: bool,
    /// Animation progress
    pub anim_progress: f32,
    /// Is visible
    pub visible: bool,
    /// Z-order
    pub z_order: usize,
    /// App icon
    pub app_icon: AppIcon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppIcon {
    Finder,
    Safari,
    Mail,
    Terminal,
    Settings,
    Files,
    TextEdit,
    Music,
    Custom(u16),
}

impl WindowThumbnail {
    pub fn new(window_id: u32, title: &str, app_name: &str, x: i32, y: i32, w: usize, h: usize) -> Self {
        // Calculate thumbnail size
        let scale = THUMBNAIL_SCALE;
        let thumb_w = (w as f32 * scale) as usize;
        let thumb_h = (h as f32 * scale) as usize;
        
        WindowThumbnail {
            window_id,
            title: String::from(title),
            app_name: String::from(app_name),
            original_rect: (x, y, w, h),
            thumbnail_pos: (x as f32, y as f32),
            thumbnail_size: (thumb_w.max(150), thumb_h.max(100)),
            selected: false,
            anim_progress: 0.0,
            visible: true,
            z_order: 0,
            app_icon: AppIcon::Finder,
        }
    }
    
    /// Update animation
    pub fn update(&mut self, dt: f32, target_pos: (f32, f32)) {
        // Smooth position animation
        let dx = target_pos.0 - self.thumbnail_pos.0;
        let dy = target_pos.1 - self.thumbnail_pos.1;
        
        self.thumbnail_pos.0 += dx * 0.15;
        self.thumbnail_pos.1 += dy * 0.15;
        
        // Animation progress
        if self.anim_progress < 1.0 {
            self.anim_progress = (self.anim_progress + dt * 3.0).min(1.0);
        }
    }
    
    /// Draw thumbnail
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible || self.anim_progress < 0.01 {
            return;
        }
        
        let x = self.thumbnail_pos.0 as usize;
        let y = self.thumbnail_pos.1 as usize;
        let w = self.thumbnail_size.0;
        let h = self.thumbnail_size.1;
        
        let alpha = self.anim_progress;
        
        // Draw shadow
        for sy in 0..8 {
            let shadow_alpha = (0.3 - sy as f32 * 0.04) * alpha;
            let shadow_y = y + h + sy;
            
            for sx in 0..w {
                let screen_x = x + sx;
                if screen_x < fb.width && shadow_y < fb.height {
                    let ptr = unsafe { 
                        (fb.base_addr as *mut u32).add(shadow_y * fb.pixels_per_scan_line + screen_x) 
                    };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = MissionControl::blend_color(bg, 0x000000, shadow_alpha); }
                }
            }
        }
        
        // Draw window background
        let bg_color = Self::blend_color(Theme::WINDOW_BG.to_u32(), alpha);
        fb.draw_rect(x, y, w, h, bg_color);
        
        // Draw title bar
        let titlebar_color = Self::blend_color(Theme::TITLEBAR_BG.to_u32(), alpha);
        fb.draw_rect(x, y, w, 24, titlebar_color);
        
        // Draw title
        let title_color = Self::blend_color(Theme::TEXT_PRIMARY.to_u32(), alpha);
        let title_display = if self.title.len() > w / 8 - 4 {
            format!("{}...", &self.title[..w / 8 - 7])
        } else {
            self.title.clone()
        };
        fb.draw_string(x + 8, y + 4, &title_display, title_color);
        
        // Draw close button
        let close_color = Self::blend_color(Theme::ERROR.to_u32(), alpha);
        fb.draw_rect(x + w - 20, y + 4, 16, 16, close_color);
        
        // Draw app icon
        self.draw_app_icon(fb, x + 4, y + h + 4, self.app_icon);
        
        // Draw selection highlight
        if self.selected {
            let highlight_color = Self::blend_color(Theme::ACCENT_PRIMARY.to_u32(), 0.5 * alpha);
            fb.draw_rect_outline(x - 2, y - 2, w + 4, h + 4, highlight_color);
        }
    }
    
    fn draw_app_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: AppIcon) {
        let icon_color = match icon {
            AppIcon::Finder => 0xFF3D67FF,
            AppIcon::Safari => 0xFF007AFF,
            AppIcon::Mail => 0xFF007AFF,
            AppIcon::Terminal => 0xFF1E1E1E,
            AppIcon::Settings => 0xFF8E8E93,
            AppIcon::Files => 0xFF007AFF,
            AppIcon::TextEdit => 0xFFFFCC00,
            AppIcon::Music => 0xFFFC3C44,
            AppIcon::Custom(code) => {
                match code % 8 {
                    0 => 0xFFFF3B30,
                    1 => 0xFFFF9500,
                    2 => 0xFFFFCC00,
                    3 => 0xFF34C759,
                    4 => 0xFF00C7BE,
                    5 => 0xFF007AFF,
                    6 => 0xFF5856D6,
                    _ => 0xFFFF2D55,
                }
            }
        };
        
        // Draw small icon circle
        for py in 0..16 {
            for px in 0..16 {
                let dx = px as i32 - 8;
                let dy = py as i32 - 8;
                if dx * dx + dy * dy <= 64 {
                    fb.plot_pixel(x + px, y + py, icon_color);
                }
            }
        }
    }
    
    fn blend_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }
    
    /// Hit test
    pub fn hit_test(&self, mx: i32, my: i32) -> bool {
        let x = self.thumbnail_pos.0 as i32;
        let y = self.thumbnail_pos.1 as i32;
        let w = self.thumbnail_size.0 as i32;
        let h = self.thumbnail_size.1 as i32;
        
        mx >= x && mx < x + w && my >= y && my < y + h
    }
}

// ============================================================================
// SPACE (DESKTOP)
// ============================================================================

/// A desktop space in Mission Control
#[derive(Clone, Debug)]
pub struct Space {
    /// Space ID
    pub id: u32,
    /// Space name
    pub name: String,
    /// Thumbnail position
    pub pos: (usize, usize),
    /// Is selected
    pub selected: bool,
    /// Has windows
    pub has_windows: bool,
    /// Wallpaper color
    pub wallpaper_color: u32,
    /// Animation progress
    pub anim_progress: f32,
}

impl Space {
    pub fn new(id: u32, name: &str) -> Self {
        Space {
            id,
            name: String::from(name),
            pos: (0, 0),
            selected: false,
            has_windows: false,
            wallpaper_color: Theme::DESKTOP_BG.to_u32(),
            anim_progress: 0.0,
        }
    }
    
    /// Update animation
    pub fn update(&mut self, dt: f32) {
        if self.anim_progress < 1.0 {
            self.anim_progress = (self.anim_progress + dt * 4.0).min(1.0);
        }
    }
    
    /// Draw space thumbnail
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.pos.0;
        let y = self.pos.1;
        let w = SPACE_WIDTH;
        let h = SPACE_HEIGHT;
        
        let alpha = self.anim_progress;
        
        // Background
        let bg_color = Self::blend_color(self.wallpaper_color, alpha);
        fb.draw_rect(x, y, w, h, bg_color);
        
        // Border
        let border_color = if self.selected {
            Self::blend_color(Theme::ACCENT_PRIMARY.to_u32(), alpha)
        } else {
            Self::blend_color(Theme::BORDER.to_u32(), alpha)
        };
        fb.draw_rect_outline(x, y, w, h, border_color);
        
        // Name
        let text_color = Self::blend_color(Theme::TEXT_PRIMARY.to_u32(), alpha);
        fb.draw_string(x + 8, y + h + 4, &self.name, text_color);
        
        // Window indicator
        if self.has_windows {
            let dot_x = x + w / 2 - 3;
            let dot_y = y + h + 20;
            fb.draw_rect(dot_x, dot_y, 6, 6, border_color);
        }
    }
    
    fn blend_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }
    
    /// Hit test
    pub fn hit_test(&self, mx: i32, my: i32) -> bool {
        let x = self.pos.0 as i32;
        let y = self.pos.1 as i32;
        
        mx >= x && mx < x + SPACE_WIDTH as i32 && my >= y && my < y + SPACE_HEIGHT as i32
    }
}

// ============================================================================
// MISSION CONTROL
// ============================================================================

/// Mission Control overlay
pub struct MissionControl {
    /// Is visible
    pub visible: bool,
    /// Window thumbnails
    pub windows: Vec<WindowThumbnail>,
    /// Desktop spaces
    pub spaces: Vec<Space>,
    /// Current space index
    pub current_space: usize,
    /// Animation progress
    pub animation_progress: f32,
    /// Screen width
    pub screen_width: usize,
    /// Screen height
    pub screen_height: usize,
    /// Selected window index
    pub selected_window: Option<usize>,
    /// Selected space index
    pub selected_space: Option<usize>,
    /// Hover window index
    pub hover_window: Option<usize>,
    /// Spaces bar Y position
    pub spaces_bar_y: usize,
}

impl MissionControl {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut mc = MissionControl {
            visible: false,
            windows: Vec::new(),
            spaces: Vec::new(),
            current_space: 0,
            animation_progress: 0.0,
            screen_width,
            screen_height,
            selected_window: None,
            selected_space: None,
            hover_window: None,
            spaces_bar_y: screen_height - 180,
        };
        
        mc.add_default_spaces();
        mc
    }
    
    fn add_default_spaces(&mut self) {
        self.spaces.push(Space::new(0, "Desktop 1"));
        self.spaces.push(Space::new(1, "Desktop 2"));
        self.spaces.push(Space::new(2, "Desktop 3"));
        
        self.spaces[0].selected = true;
    }
    
    /// Show Mission Control
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
        self.selected_window = None;
        
        // Reset animations
        for window in &mut self.windows {
            window.anim_progress = 0.0;
        }
        for space in &mut self.spaces {
            space.anim_progress = 0.0;
        }
        
        // Calculate thumbnail positions
        self.layout_windows();
        self.layout_spaces();
    }
    
    /// Hide Mission Control
    pub fn hide(&mut self) {
        self.visible = false;
        self.animation_progress = 0.0;
    }
    
    /// Toggle visibility
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }
    
    /// Add window
    pub fn add_window(&mut self, window: WindowThumbnail) {
        self.windows.push(window);
    }
    
    /// Remove window
    pub fn remove_window(&mut self, window_id: u32) {
        self.windows.retain(|w| w.window_id != window_id);
    }
    
    /// Layout windows in grid
    fn layout_windows(&mut self) {
        if self.windows.is_empty() {
            return;
        }
        
        // Calculate grid layout
        let padding = 60;
        let available_width = self.screen_width - padding * 2;
        let available_height = self.spaces_bar_y - padding * 2 - 40;
        
        // Calculate how many columns we can fit
        let cols = ((available_width + WINDOW_SPACING) / (300 + WINDOW_SPACING)).max(1);
        let rows = ((available_height + WINDOW_SPACING) / (200 + WINDOW_SPACING)).max(1);
        
        // Calculate starting position (centered)
        let total_width = cols * (300 + WINDOW_SPACING) - WINDOW_SPACING;
        let total_height = rows * (200 + WINDOW_SPACING) - WINDOW_SPACING;
        let start_x = (self.screen_width - total_width) / 2;
        let start_y = (self.spaces_bar_y - total_height) / 2 + 20;
        
        for (i, window) in self.windows.iter_mut().enumerate() {
            let col = i % cols;
            let row = i / cols;
            
            let target_x = (start_x + col * (300 + WINDOW_SPACING) + (300 - window.thumbnail_size.0) / 2) as f32;
            let target_y = (start_y + row * (200 + WINDOW_SPACING)) as f32;
            
            window.thumbnail_pos = (window.original_rect.0 as f32, window.original_rect.1 as f32);
            // Target position will be animated towards
        }
    }
    
    /// Layout spaces bar
    fn layout_spaces(&mut self) {
        let total_width = self.spaces.len() * (SPACE_WIDTH + SPACE_SPACING) - SPACE_SPACING;
        let start_x = (self.screen_width - total_width) / 2;
        
        for (i, space) in self.spaces.iter_mut().enumerate() {
            space.pos = (start_x + i * (SPACE_WIDTH + SPACE_SPACING), self.spaces_bar_y);
        }
    }
    
    /// Update animation
    pub fn update(&mut self, dt: f32) {
        if self.visible {
            if self.animation_progress < 1.0 {
                self.animation_progress = (self.animation_progress + dt * 4.0).min(1.0);
            }
            
            // Update windows
            for window in &mut self.windows {
                window.update(dt, window.thumbnail_pos);
            }
            
            // Update spaces
            for space in &mut self.spaces {
                space.update(dt);
            }
        } else if self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 4.0).max(0.0);
        }
    }
    
    /// Draw Mission Control
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }
        
        // Dim background
        let bg_alpha = 0.6 * self.animation_progress;
        for y in 0..self.screen_height {
            for x in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha as f32);
                unsafe { *ptr = dimmed; }
            }
        }
        
        // Draw windows (sorted by z-order)
        let mut sorted_windows: Vec<_> = self.windows.iter().collect();
        sorted_windows.sort_by_key(|w| w.z_order);
        
        for window in sorted_windows {
            window.draw(fb);
        }
        
        // Draw spaces bar background
        let bar_y = self.spaces_bar_y - 20;
        let bar_h = 160;
        let bar_color = Self::blend_color(0x20202020, 0x20202020, self.animation_progress);
        fb.draw_rect(0, bar_y, self.screen_width, bar_h, bar_color);
        
        // Draw spaces
        for space in &self.spaces {
            space.draw(fb);
        }
        
        // Draw "Add Desktop" button
        let add_x = self.spaces.last().map(|s| s.pos.0 + SPACE_WIDTH + SPACE_SPACING).unwrap_or(100);
        let add_y = self.spaces_bar_y;
        
        fb.draw_rect(add_x, add_y, SPACE_WIDTH, SPACE_HEIGHT, Self::blend_color(Theme::SIDEBAR_BG.to_u32(), Theme::SIDEBAR_BG.to_u32(), self.animation_progress));
        fb.draw_rect_outline(add_x, add_y, SPACE_WIDTH, SPACE_HEIGHT, Self::blend_color(Theme::BORDER.to_u32(), Theme::BORDER.to_u32(), self.animation_progress));
        fb.draw_string(add_x + SPACE_WIDTH / 2 - 20, add_y + SPACE_HEIGHT / 2 - 6, "+ New", Self::blend_color(Theme::TEXT_SECONDARY.to_u32(), Theme::TEXT_SECONDARY.to_u32(), self.animation_progress));
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
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> MissionControlEvent {
        // Check spaces
        for (i, space) in self.spaces.iter().enumerate() {
            if space.hit_test(mx, my) {
                self.selected_space = Some(i);
                
                // Switch to space
                for s in &mut self.spaces {
                    s.selected = false;
                }
                self.spaces[i].selected = true;
                self.current_space = i;
                
                self.hide();
                return MissionControlEvent::SpaceSelected(i);
            }
        }
        
        // Check "Add Desktop" button
        let add_x = self.spaces.last().map(|s| s.pos.0 + SPACE_WIDTH + SPACE_SPACING).unwrap_or(100);
        if mx >= add_x as i32 && mx < (add_x + SPACE_WIDTH) as i32 
            && my >= self.spaces_bar_y as i32 && my < (self.spaces_bar_y + SPACE_HEIGHT) as i32 {
            
            let new_id = self.spaces.len() as u32;
            let name = format!("Desktop {}", new_id + 1);
            self.spaces.push(Space::new(new_id, &name));
            self.layout_spaces();
            
            return MissionControlEvent::SpaceCreated(new_id);
        }
        
        // Check windows
        for (i, window) in self.windows.iter().enumerate() {
            if window.hit_test(mx, my) {
                let window_id = window.window_id;
                self.selected_window = Some(i);
                self.hide();
                return MissionControlEvent::WindowSelected(window_id);
            }
        }
        
        // Click outside - close
        self.hide();
        MissionControlEvent::Cancelled
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.hover_window = None;
        
        for (i, window) in self.windows.iter().enumerate() {
            if window.hit_test(mx, my) {
                self.hover_window = Some(i);
                break;
            }
        }
    }
    
    /// Switch to next space
    pub fn next_space(&mut self) {
        if self.current_space < self.spaces.len() - 1 {
            self.current_space += 1;
            self.update_space_selection();
        }
    }
    
    /// Switch to previous space
    pub fn prev_space(&mut self) {
        if self.current_space > 0 {
            self.current_space -= 1;
            self.update_space_selection();
        }
    }
    
    fn update_space_selection(&mut self) {
        for space in &mut self.spaces {
            space.selected = false;
        }
        self.spaces[self.current_space].selected = true;
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
        self.spaces_bar_y = height - 180;
        self.layout_windows();
        self.layout_spaces();
    }
}

/// Mission Control events
#[derive(Clone, Debug)]
pub enum MissionControlEvent {
    None,
    WindowSelected(u32),
    SpaceSelected(usize),
    SpaceCreated(u32),
    SpaceDeleted(u32),
    Cancelled,
}

// ============================================================================
// GLOBAL MISSION CONTROL
// ============================================================================

lazy_static::lazy_static! {
    static ref MISSION_CONTROL: Mutex<MissionControl> = Mutex::new(MissionControl::new(1920, 1080));
}

/// Initialize Mission Control
pub fn init(width: usize, height: usize) {
    let mut mc = MISSION_CONTROL.lock();
    mc.resize(width, height);
    crate::serial_println!("[GUI] Mission Control initialized");
}

/// Get Mission Control
pub fn get_mission_control() -> &'static Mutex<MissionControl> {
    &MISSION_CONTROL
}
