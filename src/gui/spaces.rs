//! # Virtual Desktops / Spaces
//!
//! Multiple virtual desktop support with smooth transitions
//! Each space has its own windows, wallpaper, and settings

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// SPACE CONSTANTS
// ============================================================================

/// Maximum number of spaces
pub const MAX_SPACES: usize = 16;

/// Transition animation duration (frames)
pub const TRANSITION_DURATION: f32 = 0.3;

// ============================================================================
// SPACE WINDOW
// ============================================================================

/// Window info in a space
#[derive(Clone, Debug)]
pub struct SpaceWindow {
    /// Window ID
    pub window_id: u32,
    /// Window position in this space
    pub x: i32,
    pub y: i32,
    /// Window size
    pub width: usize,
    pub height: usize,
    /// Is minimized
    pub minimized: bool,
    /// Is maximized
    pub maximized: bool,
    /// Is fullscreen
    pub fullscreen: bool,
    /// Z-order
    pub z_order: u32,
    /// App ID
    pub app_id: String,
}

impl SpaceWindow {
    pub fn new(window_id: u32, app_id: &str, x: i32, y: i32, width: usize, height: usize) -> Self {
        SpaceWindow {
            window_id,
            x,
            y,
            width,
            height,
            minimized: false,
            maximized: false,
            fullscreen: false,
            z_order: 0,
            app_id: String::from(app_id),
        }
    }
}

// ============================================================================
// SPACE
// ============================================================================

/// A virtual desktop space
#[derive(Clone, Debug)]
pub struct Space {
    /// Space ID
    pub id: u32,
    /// Space name
    pub name: String,
    /// Windows in this space
    pub windows: Vec<SpaceWindow>,
    /// Wallpaper path or color
    pub wallpaper: Wallpaper,
    /// Is visible (current)
    pub is_current: bool,
    /// Transition offset (-1.0 to 1.0)
    pub transition_offset: f32,
    /// Space index
    pub index: usize,
}

#[derive(Clone, Debug)]
pub enum Wallpaper {
    Color(u32),
    Gradient(u32, u32),
    Image(String),
}

impl Space {
    pub fn new(id: u32, name: &str, index: usize) -> Self {
        Space {
            id,
            name: String::from(name),
            windows: Vec::new(),
            wallpaper: Wallpaper::Color(Theme::DESKTOP_BG.to_u32()),
            is_current: false,
            transition_offset: 0.0,
            index,
        }
    }
    
    /// Add window to space
    pub fn add_window(&mut self, window: SpaceWindow) {
        // Set z-order to top
        let max_z = self.windows.iter().map(|w| w.z_order).max().unwrap_or(0);
        let mut window = window;
        window.z_order = max_z + 1;
        
        self.windows.push(window);
    }
    
    /// Remove window from space
    pub fn remove_window(&mut self, window_id: u32) {
        self.windows.retain(|w| w.window_id != window_id);
    }
    
    /// Get window by ID
    pub fn get_window(&self, window_id: u32) -> Option<&SpaceWindow> {
        self.windows.iter().find(|w| w.window_id == window_id)
    }
    
    /// Get window mutably
    pub fn get_window_mut(&mut self, window_id: u32) -> Option<&mut SpaceWindow> {
        self.windows.iter_mut().find(|w| w.window_id == window_id)
    }
    
    /// Bring window to front
    pub fn bring_to_front(&mut self, window_id: u32) {
        let max_z = self.windows.iter().map(|w| w.z_order).max().unwrap_or(0);
        if let Some(window) = self.get_window_mut(window_id) {
            window.z_order = max_z + 1;
        }
    }
    
    /// Get windows sorted by z-order
    pub fn windows_sorted(&self) -> Vec<&SpaceWindow> {
        let mut windows: Vec<_> = self.windows.iter().collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }
    
    /// Draw wallpaper
    pub fn draw_wallpaper(&self, fb: &mut Framebuffer, offset_x: i32) {
        match &self.wallpaper {
            Wallpaper::Color(color) => {
                if offset_x == 0 {
                    // Simple fill
                    for y in 0..fb.height {
                        for x in 0..fb.width {
                            fb.plot_pixel(x, y, *color);
                        }
                    }
                } else {
                    // Draw with offset (for transition)
                    let start_x = if offset_x > 0 { offset_x as usize } else { 0 };
                    let end_x = if offset_x > 0 { fb.width } else { (fb.width as i32 + offset_x) as usize };
                    
                    for y in 0..fb.height {
                        for x in start_x..end_x {
                            fb.plot_pixel(x, y, *color);
                        }
                    }
                }
            }
            Wallpaper::Gradient(color1, color2) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*color1, *color2, t);
                    
                    let start_x = if offset_x > 0 { offset_x as usize } else { 0 };
                    let end_x = if offset_x > 0 { fb.width } else { (fb.width as i32 + offset_x) as usize };
                    
                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            Wallpaper::Image(_path) => {
                // Would load and draw image - for now use color
                self.draw_wallpaper(fb, offset_x);
            }
        }
    }
    
    fn lerp_color(c1: u32, c2: u32, t: f32) -> u32 {
        let r1 = ((c1 >> 16) & 0xFF) as f32;
        let g1 = ((c1 >> 8) & 0xFF) as f32;
        let b1 = (c1 & 0xFF) as f32;
        
        let r2 = ((c2 >> 16) & 0xFF) as f32;
        let g2 = ((c2 >> 8) & 0xFF) as f32;
        let b2 = (c2 & 0xFF) as f32;
        
        let r = (r1 + (r2 - r1) * t) as u32;
        let g = (g1 + (g2 - g1) * t) as u32;
        let b = (b1 + (b2 - b1) * t) as u32;
        
        (r << 16) | (g << 8) | b
    }
}

// ============================================================================
// SPACES MANAGER
// ============================================================================

/// Virtual desktop spaces manager
pub struct SpacesManager {
    /// All spaces
    pub spaces: Vec<Space>,
    /// Current space index
    pub current_space: usize,
    /// Screen width
    pub screen_width: usize,
    /// Screen height
    pub screen_height: usize,
    /// Transition animation progress (0.0 - 1.0)
    pub transition_progress: f32,
    /// Transition direction (1 = right, -1 = left)
    pub transition_direction: i32,
    /// Is transitioning
    pub transitioning: bool,
    /// Previous space (for transition)
    pub previous_space: usize,
    /// Space switch callback
    pub on_space_switch: Option<fn(u32)>,
    /// Window move callback
    pub on_window_move: Option<fn(u32, u32)>, // window_id, target_space_id
}

impl SpacesManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = SpacesManager {
            spaces: Vec::new(),
            current_space: 0,
            screen_width,
            screen_height,
            transition_progress: 0.0,
            transition_direction: 0,
            transitioning: false,
            previous_space: 0,
            on_space_switch: None,
            on_window_move: None,
        };
        
        // Create default space
        manager.create_space("Desktop 1");
        manager.spaces[0].is_current = true;
        
        manager
    }
    
    /// Create new space
    pub fn create_space(&mut self, name: &str) -> u32 {
        if self.spaces.len() >= MAX_SPACES {
            return self.spaces.last().map(|s| s.id).unwrap_or(0);
        }
        
        let id = self.spaces.len() as u32;
        let space = Space::new(id, name, self.spaces.len());
        self.spaces.push(space);
        
        id
    }
    
    /// Delete space
    pub fn delete_space(&mut self, space_id: u32) -> bool {
        if self.spaces.len() <= 1 {
            return false; // Can't delete last space
        }
        
        let idx = self.spaces.iter().position(|s| s.id == space_id);
        if let Some(idx) = idx {
            // Move windows to adjacent space
            let target_idx = if idx < self.spaces.len() - 1 { idx + 1 } else { idx - 1 };
            let windows: Vec<_> = self.spaces[idx].windows.clone();
            
            for window in windows {
                self.spaces[target_idx].add_window(window);
            }
            
            self.spaces.remove(idx);
            
            // Update indices
            for (i, space) in self.spaces.iter_mut().enumerate() {
                space.index = i;
            }
            
            // Adjust current space if needed
            if self.current_space >= self.spaces.len() {
                self.current_space = self.spaces.len() - 1;
            }
            
            return true;
        }
        
        false
    }
    
    /// Switch to space
    pub fn switch_to_space(&mut self, space_index: usize) {
        if space_index >= self.spaces.len() || space_index == self.current_space {
            return;
        }
        
        self.previous_space = self.current_space;
        self.current_space = space_index;
        self.transition_direction = if space_index > self.previous_space { 1 } else { -1 };
        self.transition_progress = 0.0;
        self.transitioning = true;
        
        // Update current flags
        for space in &mut self.spaces {
            space.is_current = false;
        }
        self.spaces[self.current_space].is_current = true;
        
        // Callback
        if let Some(callback) = self.on_space_switch {
            callback(self.spaces[self.current_space].id);
        }
    }
    
    /// Switch to next space
    pub fn next_space(&mut self) {
        if self.current_space < self.spaces.len() - 1 {
            self.switch_to_space(self.current_space + 1);
        } else if !self.spaces.is_empty() {
            // Wrap around
            self.switch_to_space(0);
        }
    }
    
    /// Switch to previous space
    pub fn prev_space(&mut self) {
        if self.current_space > 0 {
            self.switch_to_space(self.current_space - 1);
        } else if !self.spaces.is_empty() {
            // Wrap around
            self.switch_to_space(self.spaces.len() - 1);
        }
    }
    
    /// Get current space
    pub fn current_space_ref(&self) -> &Space {
        &self.spaces[self.current_space]
    }
    
    /// Get current space mutably
    pub fn current_space_mut(&mut self) -> &mut Space {
        &mut self.spaces[self.current_space]
    }
    
    /// Get space by index
    pub fn get_space(&self, index: usize) -> Option<&Space> {
        self.spaces.get(index)
    }
    
    /// Get space by ID
    pub fn get_space_by_id(&self, id: u32) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }
    
    /// Add window to current space
    pub fn add_window_to_current(&mut self, window: SpaceWindow) {
        self.spaces[self.current_space].add_window(window);
    }
    
    /// Move window to another space
    pub fn move_window_to_space(&mut self, window_id: u32, target_space_index: usize) -> bool {
        if target_space_index >= self.spaces.len() || target_space_index == self.current_space {
            return false;
        }
        
        // Find and remove window from current space
        if let Some(window) = self.spaces[self.current_space].windows.iter()
            .find(|w| w.window_id == window_id).cloned() {
            
            self.spaces[self.current_space].remove_window(window_id);
            self.spaces[target_space_index].add_window(window);
            
            // Callback
            if let Some(callback) = self.on_window_move {
                callback(window_id, self.spaces[target_space_index].id);
            }
            
            return true;
        }
        
        false
    }
    
    /// Update transition animation
    pub fn update(&mut self, dt: f32) {
        if self.transitioning {
            self.transition_progress += dt / TRANSITION_DURATION;
            
            if self.transition_progress >= 1.0 {
                self.transition_progress = 1.0;
                self.transitioning = false;
                self.transition_direction = 0;
            }
        }
    }
    
    /// Draw spaces (with transition if active)
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.transitioning {
            // Draw both spaces during transition
            let offset = self.screen_width as f32 * (1.0 - self.transition_progress) * self.transition_direction as f32;
            
            // Previous space (sliding out)
            let prev_offset = offset as i32 - self.transition_direction * self.screen_width as i32;
            self.spaces[self.previous_space].draw_wallpaper(fb, prev_offset);
            
            // Current space (sliding in)
            self.spaces[self.current_space].draw_wallpaper(fb, offset as i32);
        } else {
            // Just draw current space
            self.spaces[self.current_space].draw_wallpaper(fb, 0);
        }
    }
    
    /// Get space count
    pub fn space_count(&self) -> usize {
        self.spaces.len()
    }
    
    /// Rename space
    pub fn rename_space(&mut self, space_index: usize, new_name: &str) -> bool {
        if let Some(space) = self.spaces.get_mut(space_index) {
            space.name = String::from(new_name);
            return true;
        }
        false
    }
    
    /// Set space wallpaper
    pub fn set_wallpaper(&mut self, space_index: usize, wallpaper: Wallpaper) -> bool {
        if let Some(space) = self.spaces.get_mut(space_index) {
            space.wallpaper = wallpaper;
            return true;
        }
        false
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
    
    /// Get space info for Mission Control
    pub fn get_space_info(&self) -> Vec<SpaceInfo> {
        self.spaces.iter().map(|s| SpaceInfo {
            id: s.id,
            name: s.name.clone(),
            window_count: s.windows.len(),
            is_current: s.is_current,
        }).collect()
    }
}

/// Space info for external use
#[derive(Clone, Debug)]
pub struct SpaceInfo {
    pub id: u32,
    pub name: String,
    pub window_count: usize,
    pub is_current: bool,
}

// ============================================================================
// GLOBAL SPACES MANAGER
// ============================================================================

lazy_static::lazy_static! {
    static ref SPACES: Mutex<SpacesManager> = Mutex::new(SpacesManager::new(1920, 1080));
}

/// Initialize spaces
pub fn init(width: usize, height: usize) {
    let mut spaces = SPACES.lock();
    spaces.resize(width, height);
    crate::serial_println!("[GUI] Spaces manager initialized");
}

/// Get spaces manager
pub fn get_spaces() -> &'static Mutex<SpacesManager> {
    &SPACES
}
