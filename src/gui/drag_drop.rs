//! # Drag and Drop Support
//!
//! System-wide drag and drop functionality for GUI elements
//! Supports file dragging, text selection, and widget reordering

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// DRAG DATA TYPES
// ============================================================================

/// Types of data that can be dragged
#[derive(Clone, Debug)]
pub enum DragData {
    /// No data
    None,
    /// File paths
    Files(Vec<String>),
    /// Text string
    Text(String),
    /// Image data (raw pixels)
    Image { width: usize, height: usize, data: Vec<u32> },
    /// Widget reference
    Widget { window_id: u32, widget_id: u32 },
    /// Custom data with MIME type
    Custom { mime_type: String, data: Vec<u8> },
}

impl DragData {
    pub fn is_empty(&self) -> bool {
        match self {
            DragData::None => true,
            DragData::Files(f) => f.is_empty(),
            DragData::Text(t) => t.is_empty(),
            DragData::Image { data, .. } => data.is_empty(),
            DragData::Custom { data, .. } => data.is_empty(),
            _ => false,
        }
    }
    
    pub fn get_text(&self) -> Option<&str> {
        match self {
            DragData::Text(t) => Some(t),
            _ => None,
        }
    }
    
    pub fn get_files(&self) -> Option<&Vec<String>> {
        match self {
            DragData::Files(f) => Some(f),
            _ => None,
        }
    }
    
    pub fn description(&self) -> String {
        match self {
            DragData::None => String::from("Nothing"),
            DragData::Files(f) => {
                if f.len() == 1 {
                    format!("File: {}", f[0])
                } else {
                    format!("{} files", f.len())
                }
            }
            DragData::Text(t) => {
                if t.len() > 20 {
                    format!("Text: {}...", &t[..20])
                } else {
                    format!("Text: {}", t)
                }
            }
            DragData::Image { width, height, .. } => {
                format!("Image: {}x{}", width, height)
            }
            DragData::Widget { .. } => String::from("Widget"),
            DragData::Custom { mime_type, .. } => {
                format!("Custom: {}", mime_type)
            }
        }
    }
}

// ============================================================================
// DROP TARGET
// ============================================================================

/// A drop target area
#[derive(Clone, Debug)]
pub struct DropTarget {
    /// Target ID
    pub id: u32,
    /// Target bounds
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    /// Accepted data types
    pub accepts: Vec<DragDataType>,
    /// Target type
    pub target_type: DropTargetType,
    /// Is highlighted
    pub highlighted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragDataType {
    Files,
    Text,
    Image,
    Widget,
    Custom,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTargetType {
    Window,
    Folder,
    TextArea,
    ImageWell,
    List,
    Trash,
    Custom,
}

impl DropTarget {
    pub fn new(id: u32, x: i32, y: i32, width: usize, height: usize) -> Self {
        DropTarget {
            id,
            x,
            y,
            width,
            height,
            accepts: vec![DragDataType::All],
            target_type: DropTargetType::Custom,
            highlighted: false,
        }
    }
    
    pub fn accepts_type(&self, data_type: DragDataType) -> bool {
        self.accepts.contains(&DragDataType::All) || self.accepts.contains(&data_type)
    }
    
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width as i32
            && py >= self.y && py < self.y + self.height as i32
    }
    
    pub fn highlight(&mut self, highlight: bool) {
        self.highlighted = highlight;
    }
}

// ============================================================================
// DRAG OPERATION
// ============================================================================

/// Current drag operation state
#[derive(Clone, Debug)]
pub struct DragOperation {
    /// Is a drag in progress
    pub active: bool,
    /// Dragged data
    pub data: DragData,
    /// Source window/widget
    pub source: Option<DragSource>,
    /// Current mouse position
    pub position: (i32, i32),
    /// Offset from drag start
    pub offset: (i32, i32),
    /// Drag image offset
    pub image_offset: (i32, i32),
    /// Current drop target
    pub target: Option<u32>,
    /// Drag effect
    pub effect: DragEffect,
    /// Drag started
    pub started: bool,
    /// Drag preview image
    pub preview: Option<DragPreview>,
}

#[derive(Clone, Debug)]
pub struct DragSource {
    /// Source window ID
    pub window_id: u32,
    /// Source widget ID
    pub widget_id: Option<u32>,
    /// Source bounds
    pub bounds: (i32, i32, usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEffect {
    None,
    Copy,
    Move,
    Link,
}

#[derive(Clone, Debug)]
pub struct DragPreview {
    /// Preview image width
    pub width: usize,
    /// Preview image height
    pub height: usize,
    /// Preview pixel data
    pub data: Vec<u32>,
    /// Show badge
    pub badge: Option<DragBadge>,
    /// Opacity
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct DragBadge {
    /// Badge icon
    pub icon: BadgeIcon,
    /// Badge position
    pub offset: (i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeIcon {
    Copy,
    Move,
    Link,
    Trash,
    Plus,
}

impl DragOperation {
    pub fn new() -> Self {
        DragOperation {
            active: false,
            data: DragData::None,
            source: None,
            position: (0, 0),
            offset: (0, 0),
            image_offset: (0, 0),
            target: None,
            effect: DragEffect::None,
            started: false,
            preview: None,
        }
    }
    
    /// Start a drag operation
    pub fn start(&mut self, data: DragData, source: DragSource, start_pos: (i32, i32)) {
        self.active = true;
        self.data = data;
        self.source = Some(source);
        self.position = start_pos;
        self.offset = (0, 0);
        self.target = None;
        self.effect = DragEffect::Copy;
        self.started = true;
        
        // Create preview
        self.create_preview();
    }
    
    /// Update drag position
    pub fn update_position(&mut self, x: i32, y: i32) {
        if !self.active {
            return;
        }
        
        self.offset = (x - self.position.0, y - self.position.1);
        self.position = (x, y);
        
        // Check if drag distance threshold met
        if !self.started {
            let dist = (self.offset.0 * self.offset.0 + self.offset.1 * self.offset.1) as f32;
            if dist > 16.0 { // 4 pixel threshold
                self.started = true;
            }
        }
    }
    
    /// End drag operation
    pub fn end(&mut self) -> (DragData, Option<DragSource>, Option<u32>, DragEffect) {
        let result = (
            self.data.clone(),
            self.source.clone(),
            self.target,
            self.effect,
        );
        
        self.active = false;
        self.data = DragData::None;
        self.source = None;
        self.target = None;
        self.effect = DragEffect::None;
        self.started = false;
        self.preview = None;
        
        result
    }
    
    /// Cancel drag operation
    pub fn cancel(&mut self) {
        self.active = false;
        self.data = DragData::None;
        self.source = None;
        self.target = None;
        self.effect = DragEffect::None;
        self.started = false;
        self.preview = None;
    }
    
    /// Set drop target
    pub fn set_target(&mut self, target_id: Option<u32>, effect: DragEffect) {
        self.target = target_id;
        self.effect = effect;
        
        // Update badge based on effect
        if let Some(ref mut preview) = self.preview {
            preview.badge = match effect {
                DragEffect::Copy => Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                DragEffect::Move => Some(DragBadge { icon: BadgeIcon::Move, offset: (8, 8) }),
                DragEffect::Link => Some(DragBadge { icon: BadgeIcon::Link, offset: (8, 8) }),
                DragEffect::None => None,
            };
        }
    }
    
    fn create_preview(&mut self) {
        let preview = match &self.data {
            DragData::Files(files) => {
                if files.len() == 1 {
                    // Single file - show file icon
                    DragPreview {
                        width: 64,
                        height: 64,
                        data: Self::create_file_preview(&files[0]),
                        badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                        opacity: 0.8,
                    }
                } else {
                    // Multiple files - show count
                    DragPreview {
                        width: 80,
                        height: 80,
                        data: Self::create_multi_file_preview(files.len()),
                        badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                        opacity: 0.8,
                    }
                }
            }
            DragData::Text(text) => {
                DragPreview {
                    width: (text.len().min(20) * 8 + 16).max(60),
                    height: 24,
                    data: Self::create_text_preview(text),
                    badge: None,
                    opacity: 0.8,
                }
            }
            DragData::Image { width, height, data } => {
                DragPreview {
                    width: (*width).min(128),
                    height: (*height).min(128),
                    data: data.clone(),
                    badge: Some(DragBadge { icon: BadgeIcon::Copy, offset: (8, 8) }),
                    opacity: 0.8,
                }
            }
            _ => {
                DragPreview {
                    width: 48,
                    height: 48,
                    data: vec![0x40808080; 48 * 48],
                    badge: None,
                    opacity: 0.6,
                }
            }
        };
        
        self.preview = Some(preview);
    }
    
    fn create_file_preview(_path: &str) -> Vec<u32> {
        // Create a simple file icon preview
        let mut data = vec![0x00000000; 64 * 64];
        
        // Draw file icon outline
        for y in 8..56 {
            for x in 12..52 {
                let is_corner = (x < 16 && y < 16) || (x > 44 && y < 16);
                if !is_corner {
                    data[y * 64 + x] = 0xE0FFFFFF;
                }
            }
        }
        
        data
    }
    
    fn create_multi_file_preview(count: usize) -> Vec<u32> {
        let mut data = vec![0x00000000; 80 * 80];
        
        // Draw stacked file icons
        for offset in 0..3 {
            let x_off = offset * 4;
            let y_off = offset * 4;
            
            for y in 8 + y_off..56 + y_off {
                for x in 12 + x_off..52 + x_off {
                    if y < 80 && x < 80 {
                        data[y * 80 + x] = 0xE0FFFFFF;
                    }
                }
            }
        }
        
        // Draw count badge
        let count_str = format!("{}", count);
        let badge_x = 56;
        let badge_y = 56;
        
        for y in badge_y..badge_y + 20 {
            for x in badge_x..badge_x + 20 {
                data[y * 80 + x] = 0xFF007AFF;
            }
        }
        
        data
    }
    
    fn create_text_preview(text: &str) -> Vec<u32> {
        let width = (text.len().min(20) * 8 + 16).max(60);
        let mut data = vec![0xE0FFFFFF; width * 24];
        
        // Would draw actual text - for now just white background
        data
    }
}

// ============================================================================
// DRAG DROP MANAGER
// ============================================================================

/// Global drag and drop manager
pub struct DragDropManager {
    /// Current drag operation
    pub operation: DragOperation,
    /// Registered drop targets
    pub targets: Vec<DropTarget>,
    /// Drag threshold (pixels)
    pub drag_threshold: i32,
    /// Spring loading delay (for folders)
    pub spring_delay: f32,
    /// Spring loading timer
    pub spring_timer: f32,
    /// Spring loading target
    pub spring_target: Option<u32>,
    /// Auto scroll enabled
    pub auto_scroll: bool,
    /// Auto scroll speed
    pub scroll_speed: i32,
    /// Next target ID
    pub next_target_id: u32,
}

impl DragDropManager {
    pub fn new() -> Self {
        DragDropManager {
            operation: DragOperation::new(),
            targets: Vec::new(),
            drag_threshold: 4,
            spring_delay: 0.5,
            spring_timer: 0.0,
            spring_target: None,
            auto_scroll: true,
            scroll_speed: 8,
            next_target_id: 1,
        }
    }
    
    /// Register a drop target
    pub fn register_target(&mut self, x: i32, y: i32, width: usize, height: usize, accepts: Vec<DragDataType>) -> u32 {
        let id = self.next_target_id;
        self.next_target_id += 1;
        
        let mut target = DropTarget::new(id, x, y, width, height);
        target.accepts = accepts;
        
        self.targets.push(target);
        id
    }
    
    /// Unregister a drop target
    pub fn unregister_target(&mut self, id: u32) {
        self.targets.retain(|t| t.id != id);
    }
    
    /// Update target position
    pub fn update_target(&mut self, id: u32, x: i32, y: i32, width: usize, height: usize) {
        if let Some(target) = self.targets.iter_mut().find(|t| t.id == id) {
            target.x = x;
            target.y = y;
            target.width = width;
            target.height = height;
        }
    }
    
    /// Start drag
    pub fn start_drag(&mut self, data: DragData, source: DragSource, start_pos: (i32, i32)) {
        self.operation.start(data, source, start_pos);
    }
    
    /// Update drag position
    pub fn update_drag(&mut self, x: i32, y: i32) -> Option<DropEvent> {
        if !self.operation.active {
            return None;
        }
        
        self.operation.update_position(x, y);
        
        // Find drop target under cursor
        let data_type = self.get_data_type();
        let mut found_target_id: Option<u32> = None;
        for target in &self.targets {
            if target.contains(x, y) && target.accepts_type(data_type) {
                found_target_id = Some(target.id);
                break;
            }
        }
        
        // Update highlights
        for target in &mut self.targets {
            let should_highlight = found_target_id == Some(target.id);
            target.highlight(should_highlight);
        }
        
        // Set target and effect
        if let Some(target_id) = found_target_id {
            let effect = if let Some(target) = self.targets.iter().find(|t| t.id == target_id) {
                self.determine_effect(target)
            } else {
                DragEffect::None
            };
            self.operation.set_target(Some(target_id), effect);
            
            // Spring loading for folders
            if let Some(target) = self.targets.iter().find(|t| t.id == target_id) {
                if target.target_type == DropTargetType::Folder {
                    if self.spring_target != Some(target.id) {
                        self.spring_target = Some(target.id);
                        self.spring_timer = 0.0;
                    }
                } else {
                    self.spring_target = None;
                    self.spring_timer = 0.0;
                }
            } else {
                self.spring_target = None;
                self.spring_timer = 0.0;
            }
            
            Some(DropEvent::TargetChanged(target_id, effect))
        } else {
            self.operation.set_target(None, DragEffect::None);
            self.spring_target = None;
            self.spring_timer = 0.0;
            Some(DropEvent::TargetChanged(0, DragEffect::None))
        }
    }
    
    /// End drag
    pub fn end_drag(&mut self) -> DropEvent {
        let (data, source, target_id, effect) = self.operation.end();
        
        // Clear highlights
        for target in &mut self.targets {
            target.highlight(false);
        }
        
        self.spring_target = None;
        self.spring_timer = 0.0;
        
        if let Some(target_id) = target_id {
            DropEvent::Dropped { data, source, target_id, effect }
        } else {
            DropEvent::Cancelled
        }
    }
    
    /// Cancel drag
    pub fn cancel_drag(&mut self) {
        self.operation.cancel();
        
        for target in &mut self.targets {
            target.highlight(false);
        }
        
        self.spring_target = None;
        self.spring_timer = 0.0;
    }
    
    /// Update spring loading
    pub fn update(&mut self, dt: f32) -> Option<DropEvent> {
        if self.spring_target.is_some() {
            self.spring_timer += dt;
            
            if self.spring_timer >= self.spring_delay {
                let target_id = self.spring_target.unwrap();
                self.spring_target = None;
                self.spring_timer = 0.0;
                return Some(DropEvent::SpringLoaded(target_id));
            }
        }
        
        None
    }
    
    /// Draw drag overlay
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Draw highlighted targets
        for target in &self.targets {
            if target.highlighted {
                self.draw_drop_highlight(fb, target);
            }
        }
        
        // Draw drag preview
        if self.operation.active && self.operation.started {
            self.draw_drag_preview(fb);
        }
    }
    
    fn draw_drop_highlight(&self, fb: &mut Framebuffer, target: &DropTarget) {
        let color = match self.operation.effect {
            DragEffect::Copy => Theme::ACCENT_PRIMARY.to_u32(),
            DragEffect::Move => Theme::ACCENT_WARNING.to_u32(),
            DragEffect::Link => Theme::ACCENT_SUCCESS.to_u32(),
            DragEffect::None => Theme::ERROR.to_u32(),
        };
        
        // Draw border
        for i in 0..3 {
            let x = (target.x + i) as usize;
            let y = (target.y + i) as usize;
            let w = target.width - (i * 2) as usize;
            let h = target.height - (i * 2) as usize;
            
            fb.draw_rect_outline(x, y, w, h, color);
        }
        
        // Draw insert indicator for lists
        if target.target_type == DropTargetType::List {
            let insert_y = self.operation.position.1.min(target.y + target.height as i32 - 2).max(target.y);
            let insert_x = target.x;
            let insert_w = target.width;
            
            // Draw line
            fb.draw_rect(insert_x as usize, insert_y as usize, insert_w, 2, color);
            
            // Draw arrows
            fb.draw_rect(insert_x as usize, insert_y as usize - 4, 8, 10, color);
            fb.draw_rect((insert_x + insert_w as i32 - 8) as usize, insert_y as usize - 4, 8, 10, color);
        }
    }
    
    fn draw_drag_preview(&self, fb: &mut Framebuffer) {
        if let Some(ref preview) = self.operation.preview {
            let x = (self.operation.position.0 + self.operation.image_offset.0) as usize;
            let y = (self.operation.position.1 + self.operation.image_offset.1) as usize;
            
            // Draw preview image
            for py in 0..preview.height {
                for px in 0..preview.width {
                    let screen_x = x + px;
                    let screen_y = y + py;
                    
                    if screen_x < fb.width && screen_y < fb.height {
                        let color = preview.data[py * preview.width + px];
                        if color != 0 {
                            fb.plot_pixel(screen_x, screen_y, color);
                        }
                    }
                }
            }
            
            // Draw badge
            if let Some(ref badge) = preview.badge {
                let badge_x = x + badge.offset.0 as usize;
                let badge_y = y + badge.offset.1 as usize;
                
                // Badge background
                fb.draw_rect(badge_x, badge_y, 20, 20, 0xFF007AFF);
                
                // Badge icon
                let icon = match badge.icon {
                    BadgeIcon::Copy => "+",
                    BadgeIcon::Move => "↗",
                    BadgeIcon::Link => "⌘",
                    BadgeIcon::Trash => "⌫",
                    BadgeIcon::Plus => "+",
                };
                fb.draw_string(badge_x + 4, badge_y + 2, icon, 0xFFFFFFFF);
            }
        }
    }
    
    fn get_data_type(&self) -> DragDataType {
        match &self.operation.data {
            DragData::None => DragDataType::All,
            DragData::Files(_) => DragDataType::Files,
            DragData::Text(_) => DragDataType::Text,
            DragData::Image { .. } => DragDataType::Image,
            DragData::Widget { .. } => DragDataType::Widget,
            DragData::Custom { .. } => DragDataType::Custom,
        }
    }
    
    fn determine_effect(&self, target: &DropTarget) -> DragEffect {
        // Default to copy, can be modified with modifier keys
        // Option = copy, Command = move, Control+Option = link
        DragEffect::Copy
    }
    
    /// Is drag in progress
    pub fn is_dragging(&self) -> bool {
        self.operation.active && self.operation.started
    }
    
    /// Get current drag data
    pub fn get_drag_data(&self) -> Option<&DragData> {
        if self.operation.active {
            Some(&self.operation.data)
        } else {
            None
        }
    }
}

/// Drop events
#[derive(Clone, Debug)]
pub enum DropEvent {
    None,
    TargetChanged(u32, DragEffect),
    Dropped {
        data: DragData,
        source: Option<DragSource>,
        target_id: u32,
        effect: DragEffect,
    },
    Cancelled,
    SpringLoaded(u32),
}

// ============================================================================
// GLOBAL DRAG DROP MANAGER
// ============================================================================

lazy_static::lazy_static! {
    static ref DRAG_DROP: Mutex<DragDropManager> = Mutex::new(DragDropManager::new());
}

/// Initialize drag and drop
pub fn init() {
    crate::serial_println!("[GUI] Drag and drop initialized");
}

/// Get drag and drop manager
pub fn get_drag_drop() -> &'static Mutex<DragDropManager> {
    &DRAG_DROP
}
