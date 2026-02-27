//! # Virtual Desktops
//!
//! macOS/Linux tarzı çoklu sanal masaüstü desteği
//! Her desktop kendi pencere setine sahip

use crate::gop::framebuffer::Framebuffer;
use crate::gui::Rect;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

// ============================================================================
// VIRTUAL DESKTOP
// ============================================================================

/// Virtual desktop
#[derive(Clone, Debug)]
pub struct VirtualDesktop {
    /// Desktop ID
    pub id: usize,
    /// Desktop name
    pub name: String,
    /// Desktop index (for ordering)
    pub index: usize,
    /// Window IDs on this desktop
    pub windows: Vec<usize>,
    /// Background color
    pub bg_color: u32,
    /// Is active
    pub active: bool,
    /// Wallpaper path (future)
    pub wallpaper: String,
}

impl VirtualDesktop {
    pub fn new(id: usize, name: &str, index: usize) -> Self {
        VirtualDesktop {
            id,
            name: String::from(name),
            index,
            windows: Vec::new(),
            bg_color: 0x1E1E1E, // Dark background
            active: false,
            wallpaper: String::new(),
        }
    }
    
    /// Add window to desktop
    pub fn add_window(&mut self, window_id: usize) {
        if !self.windows.contains(&window_id) {
            self.windows.push(window_id);
        }
    }
    
    /// Remove window from desktop
    pub fn remove_window(&mut self, window_id: usize) {
        self.windows.retain(|&id| id != window_id);
    }
    
    /// Check if desktop has window
    pub fn has_window(&self, window_id: usize) -> bool {
        self.windows.contains(&window_id)
    }
    
    /// Get window count
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }
}

// ============================================================================
// DESKTOP MANAGER
// ============================================================================

/// Desktop switch animation
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwitchAnimation {
    None,
    SlideLeft,
    SlideRight,
    Fade,
    Cube,
}

/// Virtual desktop manager
pub struct DesktopManager {
    /// Virtual desktops
    desktops: Vec<VirtualDesktop>,
    /// Active desktop index
    active_index: usize,
    /// Previous desktop index
    prev_index: usize,
    /// Maximum desktops
    max_desktops: usize,
    /// Switch animation
    switch_animation: SwitchAnimation,
    /// Animation progress (0.0 to 1.0)
    anim_progress: f32,
    /// Is animating
    is_animating: bool,
    /// Screen width
    screen_width: usize,
    /// Screen height
    screen_height: usize,
}

impl DesktopManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = DesktopManager {
            desktops: Vec::new(),
            active_index: 0,
            prev_index: 0,
            max_desktops: 16,
            switch_animation: SwitchAnimation::SlideLeft,
            anim_progress: 0.0,
            is_animating: false,
            screen_width,
            screen_height,
        };
        
        // Create default desktops
        manager.add_desktop("Desktop 1");
        manager.add_desktop("Desktop 2");
        manager.add_desktop("Desktop 3");
        
        // Set first as active
        if let Some(d) = manager.desktops.first_mut() {
            d.active = true;
        }
        
        manager
    }
    
    /// Update screen dimensions
    pub fn update_screen(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
    
    /// Add new desktop
    pub fn add_desktop(&mut self, name: &str) -> bool {
        if self.desktops.len() >= self.max_desktops {
            return false;
        }
        
        let id = self.desktops.len();
        let index = id;
        let desktop = VirtualDesktop::new(id, name, index);
        self.desktops.push(desktop);
        true
    }
    
    /// Remove desktop by index
    pub fn remove_desktop(&mut self, index: usize) -> bool {
        if self.desktops.len() <= 1 {
            return false; // Keep at least one desktop
        }
        
        if index < self.desktops.len() {
            self.desktops.remove(index);
            
            // Update indices
            for (i, d) in self.desktops.iter_mut().enumerate() {
                d.index = i;
            }
            
            // Adjust active index if needed
            if self.active_index >= self.desktops.len() {
                self.active_index = self.desktops.len() - 1;
            }
            
            true
        } else {
            false
        }
    }
    
    /// Switch to desktop by index
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.desktops.len() || index == self.active_index {
            return false;
        }
        
        // Store previous
        self.prev_index = self.active_index;
        
        // Update active states
        self.desktops[self.active_index].active = false;
        self.desktops[index].active = true;
        self.active_index = index;
        
        // Start animation
        self.anim_progress = 0.0;
        self.is_animating = true;
        
        // Set animation direction
        if index > self.prev_index {
            self.switch_animation = SwitchAnimation::SlideLeft;
        } else {
            self.switch_animation = SwitchAnimation::SlideRight;
        }
        
        true
    }
    
    /// Switch to next desktop
    pub fn switch_next(&mut self) -> bool {
        let next = if self.active_index + 1 >= self.desktops.len() {
            0
        } else {
            self.active_index + 1
        };
        self.switch_to(next)
    }
    
    /// Switch to previous desktop
    pub fn switch_prev(&mut self) -> bool {
        let prev = if self.active_index == 0 {
            self.desktops.len() - 1
        } else {
            self.active_index - 1
        };
        self.switch_to(prev)
    }
    
    /// Get active desktop
    pub fn active_desktop(&self) -> Option<&VirtualDesktop> {
        self.desktops.get(self.active_index)
    }
    
    /// Get active desktop mutable
    pub fn active_desktop_mut(&mut self) -> Option<&mut VirtualDesktop> {
        self.desktops.get_mut(self.active_index)
    }
    
    /// Get desktop by index
    pub fn get_desktop(&self, index: usize) -> Option<&VirtualDesktop> {
        self.desktops.get(index)
    }
    
    /// Get desktop by index mutable
    pub fn get_desktop_mut(&mut self, index: usize) -> Option<&mut VirtualDesktop> {
        self.desktops.get_mut(index)
    }
    
    /// Get desktop count
    pub fn desktop_count(&self) -> usize {
        self.desktops.len()
    }
    
    /// Get active index
    pub fn active_index(&self) -> usize {
        self.active_index
    }
    
    /// Rename desktop
    pub fn rename_desktop(&mut self, index: usize, name: &str) -> bool {
        if let Some(d) = self.desktops.get_mut(index) {
            d.name = String::from(name);
            true
        } else {
            false
        }
    }
    
    /// Move window to desktop
    pub fn move_window_to_desktop(&mut self, window_id: usize, from_desktop: usize, to_desktop: usize) -> bool {
        if from_desktop >= self.desktops.len() || to_desktop >= self.desktops.len() {
            return false;
        }
        
        self.desktops[from_desktop].remove_window(window_id);
        self.desktops[to_desktop].add_window(window_id);
        true
    }
    
    /// Add window to active desktop
    pub fn add_window_to_active(&mut self, window_id: usize) {
        if let Some(d) = self.active_desktop_mut() {
            d.add_window(window_id);
        }
    }
    
    /// Remove window from all desktops
    pub fn remove_window(&mut self, window_id: usize) {
        for d in &mut self.desktops {
            d.remove_window(window_id);
        }
    }
    
    /// Find desktop containing window
    pub fn find_window_desktop(&self, window_id: usize) -> Option<usize> {
        self.desktops.iter()
            .position(|d| d.has_window(window_id))
    }
    
    /// Update animation
    pub fn update_animation(&mut self) {
        if !self.is_animating {
            return;
        }
        
        self.anim_progress += 0.1;
        if self.anim_progress >= 1.0 {
            self.anim_progress = 1.0;
            self.is_animating = false;
        }
    }
    
    /// Is animating
    pub fn is_animating(&self) -> bool {
        self.is_animating
    }
    
    /// Draw desktop switch animation
    pub fn draw_animation(&self, fb: &mut Framebuffer) {
        if !self.is_animating {
            return;
        }
        
        match self.switch_animation {
            SwitchAnimation::None => {}
            SwitchAnimation::SlideLeft | SwitchAnimation::SlideRight => {
                // Slide animation - draw indicator
                let indicator_y = self.screen_height - 40;
                let indicator_width = self.desktops.len() * 20;
                let start_x = (self.screen_width - indicator_width) / 2;
                
                for (i, _) in self.desktops.iter().enumerate() {
                    let x = start_x + i * 20;
                    let color = if i == self.active_index {
                        0xFFFFFF
                    } else {
                        0x666666
                    };
                    fb.draw_rect(x, indicator_y, 10, 10, color);
                }
            }
            SwitchAnimation::Fade => {
                // Fade overlay
                let alpha = 1.0 - self.anim_progress;
                let overlay_color = (alpha * 255.0) as u32;
                // Simple fade effect
                for y in 0..self.screen_height {
                    for x in 0..self.screen_width {
                        if x % 8 == 0 && y % 8 == 0 {
                            let existing = fb.get_pixel(x, y);
                            fb.plot_pixel(x, y, blend_colors(existing, 0x000000, alpha));
                        }
                    }
                }
            }
            SwitchAnimation::Cube => {
                // Cube effect - simplified as slide with perspective
                // Just draw slide indicator for now
                self.draw_slide_indicator(fb);
            }
        }
    }
    
    /// Draw slide indicator
    fn draw_slide_indicator(&self, fb: &mut Framebuffer) {
        let indicator_y = self.screen_height - 40;
        let indicator_width = self.desktops.len() * 20;
        let start_x = (self.screen_width - indicator_width) / 2;
        
        for (i, _) in self.desktops.iter().enumerate() {
            let x = start_x + i * 20;
            let color = if i == self.active_index {
                0xFFFFFF
            } else if i == self.prev_index && self.is_animating {
                0xAAAAAA
            } else {
                0x666666
            };
            fb.draw_rect(x, indicator_y, 10, 10, color);
        }
    }
    
    /// Draw desktop indicator (for UI)
    pub fn draw_indicator(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        // Draw desktop dots
        for (i, d) in self.desktops.iter().enumerate() {
            let dot_x = x + i * 16;
            let color = if i == self.active_index {
                0xFFFFFF
            } else {
                0x888888
            };
            
            // Draw dot
            fb.draw_rect(dot_x, y, 8, 8, color);
            
            // Draw window count indicator
            if d.window_count() > 0 {
                fb.draw_rect(dot_x + 3, y + 10, 2, 2, 0xAAAAAA);
            }
        }
    }
    
    /// Get desktop names
    pub fn get_desktop_names(&self) -> Vec<String> {
        self.desktops.iter().map(|d| d.name.clone()).collect()
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Blend two colors
fn blend_colors(c1: u32, c2: u32, alpha: f32) -> u32 {
    let r1 = ((c1 >> 16) & 0xFF) as f32;
    let g1 = ((c1 >> 8) & 0xFF) as f32;
    let b1 = (c1 & 0xFF) as f32;
    
    let r2 = ((c2 >> 16) & 0xFF) as f32;
    let g2 = ((c2 >> 8) & 0xFF) as f32;
    let b2 = (c2 & 0xFF) as f32;
    
    let r = (r1 * (1.0 - alpha) + r2 * alpha) as u32;
    let g = (g1 * (1.0 - alpha) + g2 * alpha) as u32;
    let b = (b1 * (1.0 - alpha) + b2 * alpha) as u32;
    
    (r << 16) | (g << 8) | b
}

// ============================================================================
// KEYBOARD SHORTCUTS
// ============================================================================

/// Handle desktop switch shortcut
pub fn handle_desktop_shortcut(key_code: u8, manager: &mut DesktopManager) -> bool {
    match key_code {
        // Left arrow - previous desktop
        0x25 => manager.switch_prev(),
        // Right arrow - next desktop
        0x27 => manager.switch_next(),
        // Number keys 1-9 - switch to desktop
        k if k >= 0x02 && k <= 0x0A => {
            let desktop_idx = (k - 0x02) as usize;
            manager.switch_to(desktop_idx)
        }
        _ => false,
    }
}
