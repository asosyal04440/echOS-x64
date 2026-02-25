//! # Desktop Icons System
//!
//! Draggable desktop icons with double-click launch support
//! Grid-based layout with auto-arrange option

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use core::cmp::{min, max};
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// ICON SIZE
// ============================================================================

/// Standard icon sizes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Small = 16,
    Medium = 32,
    Large = 48,
    ExtraLarge = 64,
    Jumbo = 128,
}

impl IconSize {
    pub fn size(&self) -> usize {
        *self as usize
    }
    
    pub fn from_size(size: usize) -> Self {
        match size {
            0..=24 => IconSize::Small,
            25..=40 => IconSize::Medium,
            41..=56 => IconSize::Large,
            57..=96 => IconSize::ExtraLarge,
            _ => IconSize::Jumbo,
        }
    }
}

// ============================================================================
// DESKTOP ICON
// ============================================================================

/// A single desktop icon
pub struct DesktopIcon {
    /// Unique ID
    id: u32,
    /// Display name
    name: String,
    /// Icon type
    icon_type: IconType,
    /// Position (x, y) in pixels
    x: i32,
    y: i32,
    /// Icon size
    size: IconSize,
    /// Grid position (for auto-arrange)
    grid_x: i32,
    grid_y: i32,
    /// Is selected
    selected: bool,
    /// Is being dragged
    dragging: bool,
    /// Drag offset from click point
    drag_offset_x: i32,
    drag_offset_y: i32,
    /// Associated action
    action: IconAction,
    /// Cached bounding rect
    bounds: Rect,
    /// Last click time for double-click detection
    last_click_time: u64,
    /// Double-click threshold in ms
    double_click_threshold: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Folder,
    File,
    Application,
    Drive,
    Trash,
    Home,
    Computer,
    Network,
    Settings,
    Custom(u16),
}

#[derive(Clone, Debug)]
pub enum IconAction {
    None,
    OpenFolder(String),
    OpenFile(String),
    LaunchApp(String),
    OpenSettings,
    EmptyTrash,
}

impl DesktopIcon {
    pub fn new(id: u32, name: &str, icon_type: IconType, x: i32, y: i32) -> Self {
        let size = IconSize::Large;
        let bounds = Rect::new(x, y, size.size() as i32, size.size() as i32 + 20); // Icon + label
        
        DesktopIcon {
            id,
            name: String::from(name),
            icon_type,
            x,
            y,
            size,
            grid_x: 0,
            grid_y: 0,
            selected: false,
            dragging: false,
            drag_offset_x: 0,
            drag_offset_y: 0,
            action: IconAction::None,
            bounds,
            last_click_time: 0,
            double_click_threshold: 500, // 500ms
        }
    }
    
    /// Create folder icon
    pub fn folder(id: u32, name: &str, path: &str, x: i32, y: i32) -> Self {
        let mut icon = Self::new(id, name, IconType::Folder, x, y);
        icon.action = IconAction::OpenFolder(String::from(path));
        icon
    }
    
    /// Create file icon
    pub fn file(id: u32, name: &str, path: &str, x: i32, y: i32) -> Self {
        let icon_type = Self::get_file_icon_type(name);
        let mut icon = Self::new(id, name, icon_type, x, y);
        icon.action = IconAction::OpenFile(String::from(path));
        icon
    }
    
    /// Create application icon
    pub fn app(id: u32, name: &str, app_id: &str, x: i32, y: i32) -> Self {
        let mut icon = Self::new(id, name, IconType::Application, x, y);
        icon.action = IconAction::LaunchApp(String::from(app_id));
        icon
    }
    
    /// Get icon type from file extension
    fn get_file_icon_type(filename: &str) -> IconType {
        let ext = filename.rsplit('.').next().unwrap_or("");
        match ext.to_lowercase().as_str() {
            "txt" | "md" | "doc" | "docx" => IconType::Custom(0), // Text
            "png" | "jpg" | "jpeg" | "gif" | "bmp" => IconType::Custom(1), // Image
            "mp3" | "wav" | "ogg" | "flac" => IconType::Custom(2), // Audio
            "mp4" | "avi" | "mkv" | "mov" => IconType::Custom(3), // Video
            "rs" | "c" | "cpp" | "h" | "py" | "js" => IconType::Custom(4), // Code
            "zip" | "tar" | "gz" | "7z" => IconType::Custom(5), // Archive
            "exe" | "bin" | "sh" => IconType::Application,
            _ => IconType::File,
        }
    }
    
    /// Update bounds after position/size change
    fn update_bounds(&mut self) {
        let icon_size = self.size.size() as i32;
        self.bounds = Rect::new(
            self.x,
            self.y,
            icon_size,
            icon_size + 20, // Icon + label height
        );
    }
    
    /// Set position
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.update_bounds();
    }
    
    /// Get position
    pub fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }
    
    /// Set size
    pub fn set_size(&mut self, size: IconSize) {
        self.size = size;
        self.update_bounds();
    }
    
    /// Set selected
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
    
    /// Check if point is inside icon
    pub fn hit_test(&self, x: i32, y: i32) -> bool {
        self.bounds.contains(x, y)
    }
    
    /// Handle mouse down
    pub fn on_mouse_down(&mut self, x: i32, y: i32, time: u64) -> IconEvent {
        // Check for double-click
        if time - self.last_click_time < self.double_click_threshold {
            self.last_click_time = 0;
            return IconEvent::DoubleClick(self.id);
        }
        
        self.last_click_time = time;
        
        // Start drag
        self.dragging = true;
        self.drag_offset_x = self.x - x;
        self.drag_offset_y = self.y - y;
        
        IconEvent::Selected(self.id)
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> bool {
        if self.dragging {
            self.x = x + self.drag_offset_x;
            self.y = y + self.drag_offset_y;
            self.update_bounds();
            true
        } else {
            false
        }
    }
    
    /// Handle mouse up
    pub fn on_mouse_up(&mut self) -> IconEvent {
        self.dragging = false;
        IconEvent::DragEnd(self.id)
    }
    
    /// Draw the icon
    pub fn draw(&self, fb: &mut Framebuffer) {
        let icon_size = self.size.size() as usize;
        let x = self.x as usize;
        let y = self.y as usize;
        
        // Draw selection background if selected
        if self.selected {
            let padding = 4;
            fb.draw_rect(
                x.saturating_sub(padding),
                y.saturating_sub(padding),
                icon_size + padding * 2,
                icon_size + 20 + padding * 2,
                Theme::ACCENT_PRIMARY.to_u32(),
            );
        }
        
        // Draw icon based on type
        self.draw_icon(fb, x, y, icon_size);
        
        // Draw label below icon
        let label_y = y + icon_size + 4;
        let label_color = if self.selected {
            Theme::TEXT_PRIMARY.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };
        
        // Center label under icon
        let label_width = self.name.len() * 8;
        let label_x = if label_width > icon_size {
            x.saturating_sub((label_width - icon_size) / 2)
        } else {
            x + (icon_size - label_width) / 2
        };
        
        // Draw label with background for readability
        let bg_rect = Rect::new(
            label_x as i32 - 2,
            label_y as i32 - 1,
            label_width as i32 + 4,
            12,
        );
        
        // Semi-transparent background
        for py in bg_rect.y..bg_rect.y + bg_rect.height {
            for px in bg_rect.x..bg_rect.x + bg_rect.width {
                if px >= 0 && py >= 0 && (px as usize) < fb.width && (py as usize) < fb.height {
                    let idx = (py as usize) * fb.pixels_per_scan_line + (px as usize);
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(idx) };
                    unsafe {
                        let bg = *ptr;
                        // Blend with semi-transparent black
                        *ptr = ((bg & 0xFF) >> 1) | ((bg >> 1) & 0x007F7F7F);
                    }
                }
            }
        }
        
        fb.draw_string(label_x, label_y, &self.name, label_color);
    }
    
    /// Draw the icon graphic
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        match self.icon_type {
            IconType::Folder => {
                // Draw folder icon (yellow rectangle with tab)
                let color = 0xFFC107; // Yellow
                let tab_height = size / 4;
                let tab_width = size / 2;
                
                // Tab
                fb.draw_rect(x, y, tab_width, tab_height, color);
                // Body
                fb.draw_rect(x, y + tab_height, size, size - tab_height, color);
            }
            
            IconType::File => {
                // Draw file icon (white rectangle with fold)
                let color = Theme::TEXT_PRIMARY.to_u32();
                let fold_size = size / 4;
                
                // Main body
                fb.draw_rect(x, y, size - fold_size, size, color);
                // Fold
                fb.draw_rect(x + size - fold_size * 2, y, fold_size * 2, fold_size, color);
            }
            
            IconType::Application => {
                // Draw app icon (colored rectangle with gear)
                let color = Theme::ACCENT_PRIMARY.to_u32();
                fb.draw_rect(x, y, size, size, color);
                
                // Simple gear representation
                let center = size / 2;
                let radius = size / 4;
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let dx = (cosf(a) * radius as f32) as i32;
                    let dy = (sinf(a) * radius as f32) as i32;
                    fb.draw_rect(
                        (x as i32 + center as i32 + dx - 2) as usize,
                        (y as i32 + center as i32 + dy - 2) as usize,
                        4, 4,
                        Theme::DESKTOP_BG.to_u32()
                    );
                }
            }
            
            IconType::Drive => {
                // Draw drive icon (gray rectangle with slot)
                let color = 0x607D8B; // Blue Gray
                fb.draw_rect(x, y, size, size, color);
                
                // Slot
                let slot_y = y + size * 2 / 3;
                let slot_height = size / 6;
                fb.draw_rect(x + size / 4, slot_y, size / 2, slot_height, 0x000000);
            }
            
            IconType::Trash => {
                // Draw trash icon
                let color = 0x9E9E9E; // Gray
                let lid_height = size / 4;
                
                // Lid
                fb.draw_rect(x, y, size, lid_height, color);
                // Body
                fb.draw_rect(x + size / 8, y + lid_height, size * 3 / 4, size - lid_height, color);
            }
            
            IconType::Home => {
                // Draw home icon (house shape)
                let color = 0x4CAF50; // Green
                
                // Roof (triangle approximation)
                let roof_height = size / 3;
                for row in 0..roof_height {
                    let width = (size as f32 * (1.0 - row as f32 / roof_height as f32)) as usize;
                    let start_x = x + (size - width) / 2;
                    fb.draw_rect(start_x, y + row, width, 1, color);
                }
                
                // Body
                let body_y = y + roof_height;
                let body_height = size - roof_height;
                fb.draw_rect(x + size / 4, body_y, size / 2, body_height, color);
            }
            
            IconType::Computer => {
                // Draw computer icon (monitor)
                let color = Theme::TEXT_PRIMARY.to_u32();
                let screen_height = size * 3 / 4;
                
                // Screen
                fb.draw_rect(x, y, size, screen_height, color);
                // Screen content (dark)
                fb.draw_rect(x + 2, y + 2, size - 4, screen_height - 4, Theme::DESKTOP_BG.to_u32());
                // Stand
                let stand_width = size / 3;
                let stand_x = x + (size - stand_width) / 2;
                fb.draw_rect(stand_x, y + screen_height, stand_width, size - screen_height, color);
            }
            
            IconType::Network => {
                // Draw network icon (globe)
                let color = 0x2196F3; // Blue
                let center_x = x + size / 2;
                let center_y = y + size / 2;
                let radius = size / 3;
                
                // Circle approximation
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center_x as i32;
                        let dy = py as i32 - center_y as i32;
                        if dx * dx + dy * dy < (radius * radius) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }
            }
            
            IconType::Settings => {
                // Draw settings icon (gear)
                let color = Theme::TEXT_SECONDARY.to_u32();
                let center = size / 2;
                let radius = size / 3;
                
                // Outer gear
                for angle in 0..12 {
                    let a = angle as f32 * core::f32::consts::PI / 6.0;
                    let dx = (cosf(a) * radius as f32) as i32;
                    let dy = (sinf(a) * radius as f32) as i32;
                    fb.draw_rect(
                        (x as i32 + center as i32 + dx - 3) as usize,
                        (y as i32 + center as i32 + dy - 3) as usize,
                        6, 6, color
                    );
                }
                
                // Center circle
                fb.draw_rect(x + center - 4, y + center - 4, 8, 8, color);
            }
            
            IconType::Custom(subtype) => {
                // Draw custom icon based on subtype
                let color = match subtype {
                    0 => Theme::TEXT_PRIMARY.to_u32(),    // Text
                    1 => 0x4CAF50,                        // Image (green)
                    2 => 0xE91E63,                        // Audio (pink)
                    3 => 0xFF5722,                        // Video (orange)
                    4 => 0x00BCD4,                        // Code (cyan)
                    5 => 0x795548,                        // Archive (brown)
                    _ => Theme::TEXT_SECONDARY.to_u32(),
                };
                
                fb.draw_rect(x, y, size, size, color);
                
                // Add letter indicator
                let letter = match subtype {
                    0 => "T",
                    1 => "I",
                    2 => "A",
                    3 => "V",
                    4 => "C",
                    5 => "Z",
                    _ => "?",
                };
                fb.draw_string(x + size / 2 - 4, y + size / 2 - 4, letter, Theme::DESKTOP_BG.to_u32());
            }
        }
    }
    
    /// Get bounds
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
    
    /// Get ID
    pub fn id(&self) -> u32 {
        self.id
    }
    
    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Get action
    pub fn action(&self) -> &IconAction {
        &self.action
    }
}

/// Events from icon interaction
#[derive(Clone, Copy, Debug)]
pub enum IconEvent {
    None,
    Selected(u32),
    Deselected(u32),
    DoubleClick(u32),
    DragStart(u32),
    DragEnd(u32),
    ContextMenu(u32),
}

// ============================================================================
// DESKTOP ICONS MANAGER
// ============================================================================

/// Manages all desktop icons
pub struct DesktopIconsManager {
    /// All icons
    icons: BTreeMap<u32, DesktopIcon>,
    /// Next icon ID
    next_id: u32,
    /// Grid cell size
    grid_cell_size: i32,
    /// Auto-arrange enabled
    auto_arrange: bool,
    /// Snap to grid enabled
    snap_to_grid: bool,
    /// Desktop width
    width: i32,
    /// Desktop height (minus taskbar)
    height: i32,
    /// Currently selected icons
    selected: Vec<u32>,
    /// Currently dragged icon
    dragged: Option<u32>,
    /// Selection rectangle (for multi-select)
    selection_rect: Option<Rect>,
    /// Selection start point
    selection_start: Option<(i32, i32)>,
}

impl DesktopIconsManager {
    pub fn new(width: i32, height: i32) -> Self {
        DesktopIconsManager {
            icons: BTreeMap::new(),
            next_id: 1,
            grid_cell_size: 80, // 80x80 grid cells
            auto_arrange: true,
            snap_to_grid: true,
            width,
            height,
            selected: Vec::new(),
            dragged: None,
            selection_rect: None,
            selection_start: None,
        }
    }
    
    /// Add an icon
    pub fn add_icon(&mut self, mut icon: DesktopIcon) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        
        icon.id = id;
        
        // Position icon
        if self.auto_arrange {
            let (gx, gy) = self.find_next_grid_position();
            icon.grid_x = gx;
            icon.grid_y = gy;
            icon.x = gx * self.grid_cell_size + 10;
            icon.y = gy * self.grid_cell_size + 10;
            icon.update_bounds();
        }
        
        self.icons.insert(id, icon);
        id
    }
    
    /// Remove an icon
    pub fn remove_icon(&mut self, id: u32) {
        self.icons.remove(&id);
        self.selected.retain(|&i| i != id);
    }
    
    /// Find next available grid position
    fn find_next_grid_position(&self) -> (i32, i32) {
        let cols = self.width / self.grid_cell_size;
        
        for gy in 0..100 {
            for gx in 0..cols {
                let occupied = self.icons.values().any(|i| i.grid_x == gx && i.grid_y == gy);
                if !occupied {
                    return (gx, gy);
                }
            }
        }
        
        (0, 0)
    }
    
    /// Snap position to grid
    fn snap_to_grid(&self, x: i32, y: i32) -> (i32, i32) {
        if !self.snap_to_grid {
            return (x, y);
        }
        
        let gx = x / self.grid_cell_size;
        let gy = y / self.grid_cell_size;
        
        (gx * self.grid_cell_size + 10, gy * self.grid_cell_size + 10)
    }
    
    /// Handle mouse down
    pub fn on_mouse_down(&mut self, x: i32, y: i32, time: u64) -> IconEvent {
        // Check if clicking on an icon
        let hit_id = self
            .icons
            .iter()
            .rev()
            .find(|(_, icon)| icon.hit_test(x, y))
            .map(|(&id, _)| id);

        if let Some(id) = hit_id {
            let selected_ids = self.selected.clone();
            for sel_id in selected_ids {
                if sel_id != id {
                    if let Some(sel_icon) = self.icons.get_mut(&sel_id) {
                        sel_icon.set_selected(false);
                    }
                }
            }

            self.selected.clear();
            self.selected.push(id);

            if let Some(icon) = self.icons.get_mut(&id) {
                let event = icon.on_mouse_down(x, y, time);
                self.dragged = Some(id);
                return event;
            }
        }
        
        // Click on empty space - start selection rectangle
        self.clear_selection();
        self.selection_start = Some((x, y));
        self.selection_rect = Some(Rect::new(x, y, 0, 0));
        
        IconEvent::None
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> bool {
        let mut needs_redraw = false;
        
        // Handle icon dragging
        if let Some(dragged_id) = self.dragged {
            if let Some(icon) = self.icons.get_mut(&dragged_id) {
                needs_redraw = icon.on_mouse_move(x, y);
            }
        }
        
        // Handle selection rectangle
        if let Some((sx, sy)) = self.selection_start {
            let rect = Rect::new(
                min(sx, x),
                min(sy, y),
                (x - sx).abs(),
                (y - sy).abs(),
            );
            self.selection_rect = Some(rect);
            
            // Select icons within rectangle
            for (&id, icon) in self.icons.iter_mut() {
                let was_selected = self.selected.contains(&id);
                let in_rect = rect.intersects(&icon.bounds());
                
                if in_rect && !was_selected {
                    icon.set_selected(true);
                    self.selected.push(id);
                    needs_redraw = true;
                } else if !in_rect && was_selected {
                    icon.set_selected(false);
                    self.selected.retain(|&i| i != id);
                    needs_redraw = true;
                }
            }
        }
        
        needs_redraw
    }
    
    /// Handle mouse up
    pub fn on_mouse_up(&mut self, x: i32, y: i32) -> IconEvent {
        let mut event = IconEvent::None;
        
        // End icon drag
        if let Some(dragged_id) = self.dragged {
            let (icon_x, icon_y) = self
                .icons
                .get(&dragged_id)
                .map(|icon| (icon.x, icon.y))
                .unwrap_or((0, 0));
            let snap = if self.snap_to_grid {
                Some(self.snap_to_grid(icon_x, icon_y))
            } else {
                None
            };

            if let Some(icon) = self.icons.get_mut(&dragged_id) {
                event = icon.on_mouse_up();
                
                // Snap to grid
                if let Some((snap_x, snap_y)) = snap {
                    icon.set_position(snap_x, snap_y);
                    
                    // Update grid position
                    icon.grid_x = snap_x / self.grid_cell_size;
                    icon.grid_y = snap_y / self.grid_cell_size;
                }
            }
        }
        
        self.dragged = None;
        self.selection_start = None;
        self.selection_rect = None;
        
        event
    }
    
    /// Handle double click
    pub fn on_double_click(&mut self, x: i32, y: i32) -> Option<&IconAction> {
        for icon in self.icons.values_mut().rev() {
            if icon.hit_test(x, y) {
                return Some(&icon.action);
            }
        }
        None
    }
    
    /// Clear all selections
    pub fn clear_selection(&mut self) {
        for &id in &self.selected {
            if let Some(icon) = self.icons.get_mut(&id) {
                icon.set_selected(false);
            }
        }
        self.selected.clear();
    }
    
    /// Select all icons
    pub fn select_all(&mut self) {
        self.selected.clear();
        for (&id, icon) in self.icons.iter_mut() {
            icon.set_selected(true);
            self.selected.push(id);
        }
    }
    
    /// Draw all icons
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Draw selection rectangle if active
        if let Some(rect) = self.selection_rect {
            // Draw semi-transparent selection rectangle
            for py in rect.y..rect.y + rect.height {
                for px in rect.x..rect.x + rect.width {
                    if px >= 0 && py >= 0 && (px as usize) < fb.width && (py as usize) < fb.height {
                        let idx = (py as usize) * fb.pixels_per_scan_line + (px as usize);
                        let ptr = unsafe { (fb.base_addr as *mut u32).add(idx) };
                        unsafe {
                            let bg = *ptr;
                            // Blend with selection color
                            *ptr = ((bg >> 1) & 0x003F3F3F) | (Theme::ACCENT_PRIMARY.to_u32() >> 1);
                        }
                    }
                }
            }
        }
        
        // Draw icons
        for icon in self.icons.values() {
            icon.draw(fb);
        }
    }
    
    /// Resize desktop
    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
        
        // Re-arrange icons if auto-arrange is on
        if self.auto_arrange {
            self.arrange_icons();
        }
    }
    
    /// Auto-arrange all icons
    pub fn arrange_icons(&mut self) {
        let cols = self.width / self.grid_cell_size;
        let mut sorted_ids: Vec<u32> = self.icons.keys().copied().collect();
        sorted_ids.sort();
        
        for (idx, &id) in sorted_ids.iter().enumerate() {
            let gx = (idx as i32) % cols;
            let gy = (idx as i32) / cols;
            
            if let Some(icon) = self.icons.get_mut(&id) {
                icon.grid_x = gx;
                icon.grid_y = gy;
                icon.x = gx * self.grid_cell_size + 10;
                icon.y = gy * self.grid_cell_size + 10;
                icon.update_bounds();
            }
        }
    }
    
    /// Get icon count
    pub fn count(&self) -> usize {
        self.icons.len()
    }
    
    /// Get selected icons
    pub fn selected(&self) -> &[u32] {
        &self.selected
    }
    
    /// Get icon by ID
    pub fn get(&self, id: u32) -> Option<&DesktopIcon> {
        self.icons.get(&id)
    }
    
    /// Set auto-arrange
    pub fn set_auto_arrange(&mut self, enabled: bool) {
        self.auto_arrange = enabled;
        if enabled {
            self.arrange_icons();
        }
    }
    
    /// Set snap to grid
    pub fn set_snap_to_grid(&mut self, enabled: bool) {
        self.snap_to_grid = enabled;
    }
}

// ============================================================================
// GLOBAL DESKTOP ICONS
// ============================================================================

lazy_static::lazy_static! {
    static ref DESKTOP_ICONS: Mutex<DesktopIconsManager> = Mutex::new(DesktopIconsManager::new(1920, 1080));
}

/// Initialize desktop icons
pub fn init(width: i32, height: i32) {
    let mut icons = DESKTOP_ICONS.lock();
    icons.resize(width, height);
    
    // Add default icons
    icons.add_icon(DesktopIcon::folder(0, "Home", "/home", 10, 10));
    icons.add_icon(DesktopIcon::folder(0, "Documents", "/home/documents", 10, 10));
    icons.add_icon(DesktopIcon::folder(0, "Downloads", "/home/downloads", 10, 10));
    icons.add_icon(DesktopIcon::app(0, "Settings", "settings", 10, 10));
    icons.add_icon(DesktopIcon::new(0, "Trash", IconType::Trash, 10, 10));
    
    crate::serial_println!("[GUI] Desktop icons initialized ({} icons)", icons.count());
}

/// Get desktop icons manager
pub fn get_icons() -> &'static Mutex<DesktopIconsManager> {
    &DESKTOP_ICONS
}
