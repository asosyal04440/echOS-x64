//! # echOS Window Manager
//!
//! Window management: minimize, maximize, resize, snap, alt+tab.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::window::Window;
use crate::gui::widgets::{Rect, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// Window state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    SnappedLeft,
    SnappedRight,
    SnappedTop,
    SnappedBottom,
}

/// Window info for management
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub state: WindowState,
    pub normal_rect: Rect,
    pub current_rect: Rect,
    pub z_index: usize,
    pub focused: bool,
    pub resizable: bool,
    pub minimizable: bool,
    pub maximizable: bool,
}

impl WindowInfo {
    pub fn new(id: u32, title: &str, x: i32, y: i32, width: i32, height: i32) -> Self {
        let rect = Rect::new(x, y, width, height);
        Self {
            id,
            title: String::from(title),
            state: WindowState::Normal,
            normal_rect: rect,
            current_rect: rect,
            z_index: 0,
            focused: false,
            resizable: true,
            minimizable: true,
            maximizable: true,
        }
    }
}

/// Resize edge
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    None,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Window Manager
pub struct WindowManager {
    windows: Vec<WindowInfo>,
    focused_id: Option<u32>,
    resize_edge: ResizeEdge,
    resize_start: (i32, i32),
    resize_window_id: Option<u32>,
    drag_offset: (i32, i32),
    dragging_id: Option<u32>,
    screen_width: usize,
    screen_height: usize,
    taskbar_height: usize,
    snap_threshold: i32,
}

impl WindowManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Self {
            windows: Vec::new(),
            focused_id: None,
            resize_edge: ResizeEdge::None,
            resize_start: (0, 0),
            resize_window_id: None,
            drag_offset: (0, 0),
            dragging_id: None,
            screen_width,
            screen_height,
            taskbar_height: 40,
            snap_threshold: 20,
        }
    }

    pub fn add_window(&mut self, mut window: WindowInfo) -> u32 {
        window.z_index = self.windows.len();
        let id = window.id;
        self.windows.push(window);
        self.focus_window(id);
        id
    }

    pub fn remove_window(&mut self, id: u32) {
        self.windows.retain(|w| w.id != id);
        if self.focused_id == Some(id) {
            // Focus topmost remaining window
            self.focused_id = self.windows.iter()
                .max_by_key(|w| w.z_index)
                .map(|w| w.id);
        }
    }

    pub fn windows(&self) -> &Vec<WindowInfo> {
        &self.windows
    }

    pub fn window_mut(&mut self, id: u32) -> Option<&mut WindowInfo> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn window(&self, id: u32) -> Option<&WindowInfo> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn focused_window(&self) -> Option<&WindowInfo> {
        self.focused_id.and_then(|id| self.window(id))
    }

    pub fn focused_window_mut(&mut self) -> Option<&mut WindowInfo> {
        self.focused_id.and_then(|id| self.window_mut(id))
    }

    pub fn focus_window(&mut self, id: u32) {
        // Unfocus all
        for w in &mut self.windows {
            w.focused = false;
        }
        
        // Find window index
        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            // Bring to front
            let max_z = self.windows.iter().map(|w| w.z_index).max().unwrap_or(0);
            self.windows[idx].focused = true;
            self.windows[idx].z_index = max_z + 1;
            self.focused_id = Some(id);
        }
    }

    pub fn minimize(&mut self, id: u32) {
        if let Some(window) = self.window_mut(id) {
            if window.minimizable {
                window.state = WindowState::Minimized;
                // Focus next window
                if self.focused_id == Some(id) {
                    self.focused_id = self.windows.iter()
                        .filter(|w| w.state != WindowState::Minimized)
                        .max_by_key(|w| w.z_index)
                        .map(|w| w.id);
                }
            }
        }
    }

    pub fn restore(&mut self, id: u32) {
        if let Some(window) = self.window_mut(id) {
            window.state = WindowState::Normal;
            window.current_rect = window.normal_rect;
            self.focus_window(id);
        }
    }

    pub fn maximize(&mut self, id: u32) {
        // Get screen dimensions first
        let screen_w = self.screen_width as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;
        
        if let Some(window) = self.window_mut(id) {
            if window.maximizable {
                if window.state == WindowState::Maximized {
                    // Restore
                    window.state = WindowState::Normal;
                    window.current_rect = window.normal_rect;
                } else {
                    // Maximize
                    window.normal_rect = window.current_rect;
                    window.state = WindowState::Maximized;
                    window.current_rect = Rect::new(0, 0, screen_w, screen_h);
                }
            }
        }
    }

    pub fn snap_left(&mut self, id: u32) {
        // Get screen dimensions first
        let half_w = (self.screen_width / 2) as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;
        
        if let Some(window) = self.window_mut(id) {
            if window.resizable {
                window.normal_rect = window.current_rect;
                window.state = WindowState::SnappedLeft;
                window.current_rect = Rect::new(0, 0, half_w, screen_h);
            }
        }
    }

    pub fn snap_right(&mut self, id: u32) {
        // Get screen dimensions first
        let half_w = (self.screen_width / 2) as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;
        
        if let Some(window) = self.window_mut(id) {
            if window.resizable {
                window.normal_rect = window.current_rect;
                window.state = WindowState::SnappedRight;
                window.current_rect = Rect::new(half_w, 0, half_w, screen_h);
            }
        }
    }

    pub fn start_drag(&mut self, id: u32, x: i32, y: i32) {
        // Get window state and rect first
        let (is_maximized, normal_rect) = self.window(id)
            .map(|w| (w.state == WindowState::Maximized, w.normal_rect))
            .unwrap_or((false, Rect::new(0, 0, 0, 0)));
        
        // Get current rect for drag offset
        let current_rect = self.window(id)
            .map(|w| w.current_rect)
            .unwrap_or(Rect::new(0, 0, 0, 0));
        
        // If maximized, unmaximize first
        if is_maximized {
            if let Some(window) = self.window_mut(id) {
                window.state = WindowState::Normal;
                window.current_rect = normal_rect;
            }
        }
        
        self.dragging_id = Some(id);
        self.drag_offset = (x - current_rect.x, y - current_rect.y);
        self.focus_window(id);
    }

    pub fn drag(&mut self, x: i32, y: i32) -> bool {
        let dragging_id = self.dragging_id;
        let drag_offset_x = self.drag_offset.0;
        let drag_offset_y = self.drag_offset.1;
        let snap_threshold = self.snap_threshold;
        let screen_width = self.screen_width as i32;
        
        if let Some(id) = dragging_id {
            if let Some(window) = self.window_mut(id) {
                let new_x = x - drag_offset_x;
                let new_y = y - drag_offset_y;
                window.current_rect.x = new_x;
                window.current_rect.y = new_y;
                window.normal_rect.x = new_x;
                window.normal_rect.y = new_y;
                window.state = WindowState::Normal;
                
                // Check for snap
                if x < snap_threshold {
                    // Snap left preview
                } else if x > screen_width - snap_threshold {
                    // Snap right preview
                }
                
                return true;
            }
        }
        false
    }

    pub fn end_drag(&mut self, x: i32, _y: i32) {
        if let Some(id) = self.dragging_id {
            // Check for snap
            if x < self.snap_threshold {
                self.snap_left(id);
            } else if x > self.screen_width as i32 - self.snap_threshold {
                self.snap_right(id);
            }
        }
        self.dragging_id = None;
    }

    pub fn start_resize(&mut self, id: u32, edge: ResizeEdge, x: i32, y: i32) {
        let (resizable, state) = self.window(id)
            .map(|w| (w.resizable, w.state))
            .unwrap_or((false, WindowState::Normal));
        
        if resizable && state == WindowState::Normal {
            self.resize_window_id = Some(id);
            self.resize_edge = edge;
            self.resize_start = (x, y);
            self.focus_window(id);
        }
    }

    pub fn resize(&mut self, x: i32, y: i32) -> bool {
        let resize_id = self.resize_window_id;
        let edge = self.resize_edge;
        let start_x = self.resize_start.0;
        let start_y = self.resize_start.1;
        
        if let Some(id) = resize_id {
            if let Some(window) = self.window_mut(id) {
                let dx = x - start_x;
                let dy = y - start_y;
                
                let min_width = 200;
                let min_height = 150;
                
                match edge {
                    ResizeEdge::Left => {
                        let new_width = window.current_rect.width - dx;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                    }
                    ResizeEdge::Right => {
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                    }
                    ResizeEdge::Top => {
                        let new_height = window.current_rect.height - dy;
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                    }
                    ResizeEdge::Bottom => {
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::TopLeft => {
                        let new_width = window.current_rect.width - dx;
                        let new_height = window.current_rect.height - dy;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                    }
                    ResizeEdge::TopRight => {
                        let new_height = window.current_rect.height - dy;
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                    }
                    ResizeEdge::BottomLeft => {
                        let new_width = window.current_rect.width - dx;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::BottomRight => {
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::None => {}
                }
                
                window.normal_rect = window.current_rect;
            }
        }
        
        // Update resize_start after the match
        self.resize_start = (x, y);
        
        resize_id.is_some()
    }

    pub fn end_resize(&mut self) {
        self.resize_window_id = None;
        self.resize_edge = ResizeEdge::None;
    }

    pub fn detect_resize_edge(&self, id: u32, x: i32, y: i32) -> ResizeEdge {
        if let Some(window) = self.window(id) {
            if window.state != WindowState::Normal || !window.resizable {
                return ResizeEdge::None;
            }
            
            let rect = window.current_rect;
            let border = 8;
            
            let near_left = x >= rect.x - border && x <= rect.x + border;
            let near_right = x >= rect.x + rect.width - border && x <= rect.x + rect.width + border;
            let near_top = y >= rect.y - border && y <= rect.y + border;
            let near_bottom = y >= rect.y + rect.height - border && y <= rect.y + rect.height + border;
            
            if near_top && near_left {
                ResizeEdge::TopLeft
            } else if near_top && near_right {
                ResizeEdge::TopRight
            } else if near_bottom && near_left {
                ResizeEdge::BottomLeft
            } else if near_bottom && near_right {
                ResizeEdge::BottomRight
            } else if near_top {
                ResizeEdge::Top
            } else if near_bottom {
                ResizeEdge::Bottom
            } else if near_left {
                ResizeEdge::Left
            } else if near_right {
                ResizeEdge::Right
            } else {
                ResizeEdge::None
            }
        } else {
            ResizeEdge::None
        }
    }

    pub fn window_at(&self, x: i32, y: i32) -> Option<u32> {
        // Check from top to bottom (highest z-index first)
        let mut sorted: Vec<_> = self.windows.iter().collect();
        sorted.sort_by(|a, b| b.z_index.cmp(&a.z_index));
        
        for window in sorted {
            if window.state != WindowState::Minimized {
                if window.current_rect.contains(x, y) {
                    return Some(window.id);
                }
            }
        }
        None
    }

    pub fn cycle_windows(&mut self, forward: bool) {
        let visible: Vec<_> = self.windows.iter()
            .filter(|w| w.state != WindowState::Minimized)
            .map(|w| w.id)
            .collect();
        
        if visible.is_empty() {
            return;
        }
        
        let current_idx = self.focused_id
            .and_then(|id| visible.iter().position(|&x| x == id))
            .unwrap_or(0);
        
        let next_idx = if forward {
            (current_idx + 1) % visible.len()
        } else {
            (current_idx + visible.len() - 1) % visible.len()
        };
        
        self.focus_window(visible[next_idx]);
    }

    pub fn update_screen_size(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
}
