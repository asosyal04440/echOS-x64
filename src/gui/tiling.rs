//! # Window Tiling and Snapping
//!
//! Windows 7+ ve macOS tarzı pencere yapıştırma ve döşeme
//! Ekran kenarlarına sürükleme ile otomatik boyutlandırma

use crate::gop::framebuffer::Framebuffer;
use crate::gui::Rect;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// SNAP ZONES
// ============================================================================

/// Snap zone types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapZone {
    None,
    Left,       // Snap to left half
    Right,      // Snap to right half
    Top,        // Snap to top half
    Bottom,     // Snap to bottom half
    TopLeft,    // Snap to top-left quarter
    TopRight,   // Snap to top-right quarter
    BottomLeft, // Snap to bottom-left quarter
    BottomRight,// Snap to bottom-right quarter
    Maximize,   // Maximize window
    Center,     // Center window
}

/// Snap zone detection threshold (pixels from edge)
pub const SNAP_THRESHOLD: i32 = 20;

/// Snap preview highlight color
pub const SNAP_PREVIEW_COLOR: u32 = 0x3366CC;

/// Snap animation duration (frames)
pub const SNAP_ANIM_FRAMES: u8 = 10;

// ============================================================================
// WINDOW LAYOUT
// ============================================================================

/// Window layout state for tiling
#[derive(Clone, Debug)]
pub struct WindowLayout {
    /// Window ID
    pub window_id: usize,
    /// Current snap zone
    pub snap_zone: SnapZone,
    /// Original rect (before snap)
    pub original_rect: Rect,
    /// Target rect (after snap)
    pub target_rect: Rect,
    /// Animation progress
    pub anim_progress: u8,
}

impl WindowLayout {
    pub fn new(window_id: usize) -> Self {
        WindowLayout {
            window_id,
            snap_zone: SnapZone::None,
            original_rect: Rect { x: 0, y: 0, width: 800, height: 600 },
            target_rect: Rect { x: 0, y: 0, width: 800, height: 600 },
            anim_progress: 0,
        }
    }
}

// ============================================================================
// TILING MANAGER
// ============================================================================

/// Tiling manager for window snapping
pub struct TilingManager {
    /// Screen width
    screen_width: i32,
    /// Screen height
    screen_height: i32,
    /// Menu bar height
    menu_bar_height: i32,
    /// Dock height
    dock_height: i32,
    /// Window layouts
    layouts: Vec<WindowLayout>,
    /// Preview zone (during drag)
    preview_zone: SnapZone,
    /// Preview rect
    preview_rect: Rect,
    /// Show preview
    show_preview: bool,
}

impl TilingManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        TilingManager {
            screen_width: screen_width as i32,
            screen_height: screen_height as i32,
            menu_bar_height: 25,
            dock_height: 80,
            layouts: Vec::new(),
            preview_zone: SnapZone::None,
            preview_rect: Rect { x: 0, y: 0, width: 0, height: 0 },
            show_preview: false,
        }
    }
    
    /// Update screen dimensions
    pub fn update_screen(&mut self, width: usize, height: usize) {
        self.screen_width = width as i32;
        self.screen_height = height as i32;
    }
    
    /// Get available area (excluding menu bar and dock)
    fn available_area(&self) -> (i32, i32, i32, i32) {
        let x = 0;
        let y = self.menu_bar_height;
        let width = self.screen_width;
        let height = self.screen_height - self.menu_bar_height - self.dock_height;
        (x, y, width, height)
    }
    
    /// Detect snap zone from mouse position during drag
    pub fn detect_snap_zone(&self, mouse_x: i32, mouse_y: i32) -> SnapZone {
        let (ax, ay, _aw, ah) = self.available_area();
        
        // Top-left corner
        if mouse_x < SNAP_THRESHOLD && mouse_y < ay + SNAP_THRESHOLD {
            return SnapZone::TopLeft;
        }
        // Top-right corner
        if mouse_x > self.screen_width - SNAP_THRESHOLD && mouse_y < ay + SNAP_THRESHOLD {
            return SnapZone::TopRight;
        }
        // Bottom-left corner
        if mouse_x < SNAP_THRESHOLD && mouse_y > self.screen_height - SNAP_THRESHOLD {
            return SnapZone::BottomLeft;
        }
        // Bottom-right corner
        if mouse_x > self.screen_width - SNAP_THRESHOLD && mouse_y > self.screen_height - SNAP_THRESHOLD {
            return SnapZone::BottomRight;
        }
        
        // Left edge
        if mouse_x < SNAP_THRESHOLD {
            return SnapZone::Left;
        }
        // Right edge
        if mouse_x > self.screen_width - SNAP_THRESHOLD {
            return SnapZone::Right;
        }
        // Top edge (maximize)
        if mouse_y < ay + SNAP_THRESHOLD {
            return SnapZone::Maximize;
        }
        // Bottom edge
        if mouse_y > self.screen_height - self.dock_height - SNAP_THRESHOLD {
            return SnapZone::Bottom;
        }
        
        SnapZone::None
    }
    
    /// Calculate rect for snap zone
    pub fn get_snap_rect(&self, zone: SnapZone) -> Rect {
        let (ax, ay, aw, ah) = self.available_area();
        
        match zone {
            SnapZone::None => Rect { x: ax, y: ay, width: aw, height: ah },
            SnapZone::Left => Rect { 
                x: ax, 
                y: ay, 
                width: aw / 2, 
                height: ah 
            },
            SnapZone::Right => Rect { 
                x: ax + aw / 2, 
                y: ay, 
                width: aw / 2, 
                height: ah 
            },
            SnapZone::Top => Rect { 
                x: ax, 
                y: ay, 
                width: aw, 
                height: ah / 2 
            },
            SnapZone::Bottom => Rect { 
                x: ax, 
                y: ay + ah / 2, 
                width: aw, 
                height: ah / 2 
            },
            SnapZone::TopLeft => Rect { 
                x: ax, 
                y: ay, 
                width: aw / 2, 
                height: ah / 2 
            },
            SnapZone::TopRight => Rect { 
                x: ax + aw / 2, 
                y: ay, 
                width: aw / 2, 
                height: ah / 2 
            },
            SnapZone::BottomLeft => Rect { 
                x: ax, 
                y: ay + ah / 2, 
                width: aw / 2, 
                height: ah / 2 
            },
            SnapZone::BottomRight => Rect { 
                x: ax + aw / 2, 
                y: ay + ah / 2, 
                width: aw / 2, 
                height: ah / 2 
            },
            SnapZone::Maximize => Rect { 
                x: ax, 
                y: ay, 
                width: aw, 
                height: ah 
            },
            SnapZone::Center => Rect { 
                x: ax + aw / 4, 
                y: ay + ah / 4, 
                width: aw / 2, 
                height: ah / 2 
            },
        }
    }
    
    /// Start drag preview
    pub fn start_drag_preview(&mut self, mouse_x: i32, mouse_y: i32) {
        let zone = self.detect_snap_zone(mouse_x, mouse_y);
        if zone != SnapZone::None {
            self.preview_zone = zone;
            self.preview_rect = self.get_snap_rect(zone);
            self.show_preview = true;
        } else {
            self.show_preview = false;
        }
    }
    
    /// Update drag preview
    pub fn update_drag_preview(&mut self, mouse_x: i32, mouse_y: i32) {
        let zone = self.detect_snap_zone(mouse_x, mouse_y);
        if zone != SnapZone::None {
            self.preview_zone = zone;
            self.preview_rect = self.get_snap_rect(zone);
            self.show_preview = true;
        } else {
            self.show_preview = false;
        }
    }
    
    /// End drag preview and snap window
    pub fn end_drag_preview(&mut self) -> SnapZone {
        self.show_preview = false;
        self.preview_zone
    }
    
    /// Snap window to zone
    pub fn snap_window(&mut self, window_id: usize, zone: SnapZone, current_rect: Rect) -> Rect {
        if zone == SnapZone::None {
            return current_rect;
        }
        
        let target = self.get_snap_rect(zone);
        
        // Find or create layout
        if let Some(layout) = self.layouts.iter_mut().find(|l| l.window_id == window_id) {
            layout.original_rect = current_rect;
            layout.target_rect = target;
            layout.snap_zone = zone;
            layout.anim_progress = 0;
            target
        } else {
            let mut layout = WindowLayout::new(window_id);
            layout.original_rect = current_rect;
            layout.target_rect = target;
            layout.snap_zone = zone;
            layout.anim_progress = 0;
            self.layouts.push(layout);
            target
        }
    }
    
    /// Unsnap window
    pub fn unsnap_window(&mut self, window_id: usize) -> Option<Rect> {
        if let Some(pos) = self.layouts.iter().position(|l| l.window_id == window_id) {
            let layout = &self.layouts[pos];
            let original = layout.original_rect;
            self.layouts.remove(pos);
            Some(original)
        } else {
            None
        }
    }
    
    /// Check if window is snapped
    pub fn is_snapped(&self, window_id: usize) -> bool {
        self.layouts.iter().any(|l| l.window_id == window_id && l.snap_zone != SnapZone::None)
    }
    
    /// Get snap zone for window
    pub fn get_snap_zone(&self, window_id: usize) -> SnapZone {
        self.layouts.iter()
            .find(|l| l.window_id == window_id)
            .map(|l| l.snap_zone)
            .unwrap_or(SnapZone::None)
    }
    
    /// Draw snap preview overlay
    pub fn draw_preview(&self, fb: &mut Framebuffer) {
        if !self.show_preview {
            return;
        }
        
        // Draw semi-transparent overlay
        let rect = &self.preview_rect;
        
        // Border
        for i in 0..4i32 {
            fb.draw_rect_outline(
                (rect.x + i) as usize,
                (rect.y + i) as usize,
                (rect.width - i * 2) as usize,
                (rect.height - i * 2) as usize,
                SNAP_PREVIEW_COLOR,
            );
        }
        
        // Fill with semi-transparent color (simulated with pattern)
        for y in (rect.y..rect.y + rect.height).step_by(4) {
            for x in (rect.x..rect.x + rect.width).step_by(4) {
                if x >= 0 && (x as usize) < fb.width && y >= 0 && (y as usize) < fb.height {
                    let existing = fb.get_pixel(x as usize, y as usize);
                    // Blend with preview color
                    fb.plot_pixel(x as usize, y as usize, blend_color(existing, SNAP_PREVIEW_COLOR, 0.3));
                }
            }
        }
    }
    
    /// Update animations
    pub fn update_animations(&mut self) {
        for layout in &mut self.layouts {
            if layout.anim_progress < SNAP_ANIM_FRAMES {
                layout.anim_progress += 1;
            }
        }
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Blend two colors
fn blend_color(c1: u32, c2: u32, alpha: f32) -> u32 {
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

/// Handle keyboard shortcut for window snapping
pub fn handle_snap_shortcut(key_code: u8, _window_id: usize, _screen_width: usize, _screen_height: usize) -> Option<SnapZone> {
    // Windows + Arrow keys style shortcuts
    match key_code {
        // Left arrow - snap left
        0x25 => Some(SnapZone::Left),
        // Right arrow - snap right  
        0x27 => Some(SnapZone::Right),
        // Up arrow - maximize
        0x26 => Some(SnapZone::Maximize),
        // Down arrow - minimize/restore
        0x28 => Some(SnapZone::Center),
        _ => None,
    }
}
