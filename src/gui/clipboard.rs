//! # Clipboard Manager
//!
//! System clipboard with history and multiple formats
//! Supports text, images, files, and custom data types

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// CLIPBOARD CONSTANTS
// ============================================================================

/// Maximum history items
pub const MAX_HISTORY: usize = 50;

/// Maximum item size (bytes)
pub const MAX_ITEM_SIZE: usize = 10 * 1024 * 1024; // 10 MB

// ============================================================================
// CLIPBOARD DATA
// ============================================================================

/// Clipboard data types
#[derive(Clone, Debug)]
pub enum ClipboardData {
    /// No data
    Empty,
    /// Plain text
    Text(String),
    /// Rich text (HTML/RTF)
    RichText { html: String, plain: String },
    /// Image data
    Image { width: usize, height: usize, data: Vec<u32> },
    /// File paths
    Files(Vec<String>),
    /// URL
    Url(String),
    /// Custom data with format identifier
    Custom { format: String, data: Vec<u8> },
}

impl ClipboardData {
    pub fn is_empty(&self) -> bool {
        match self {
            ClipboardData::Empty => true,
            ClipboardData::Text(t) => t.is_empty(),
            ClipboardData::RichText { html, plain } => html.is_empty() && plain.is_empty(),
            ClipboardData::Image { data, .. } => data.is_empty(),
            ClipboardData::Files(f) => f.is_empty(),
            ClipboardData::Url(u) => u.is_empty(),
            ClipboardData::Custom { data, .. } => data.is_empty(),
        }
    }
    
    pub fn size(&self) -> usize {
        match self {
            ClipboardData::Empty => 0,
            ClipboardData::Text(t) => t.len(),
            ClipboardData::RichText { html, plain } => html.len() + plain.len(),
            ClipboardData::Image { width, height, .. } => width * height * 4,
            ClipboardData::Files(f) => f.iter().map(|p| p.len()).sum(),
            ClipboardData::Url(u) => u.len(),
            ClipboardData::Custom { data, .. } => data.len(),
        }
    }
    
    pub fn format_size(&self) -> String {
        let size = self.size();
        
        if size < 1024 {
            format!("{} B", size)
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        }
    }
    
    pub fn preview(&self, max_len: usize) -> String {
        match self {
            ClipboardData::Empty => String::from("(empty)"),
            ClipboardData::Text(t) => {
                if t.len() > max_len {
                    format!("{}...", &t[..max_len])
                } else {
                    t.clone()
                }
            }
            ClipboardData::RichText { plain, .. } => {
                if plain.len() > max_len {
                    format!("{}...", &plain[..max_len])
                } else {
                    plain.clone()
                }
            }
            ClipboardData::Image { width, height, .. } => {
                format!("Image: {}x{}", width, height)
            }
            ClipboardData::Files(files) => {
                if files.len() == 1 {
                    let name = files[0].rsplit('/').next().unwrap_or(&files[0]);
                    if name.len() > max_len {
                        format!("{}...", &name[..max_len])
                    } else {
                        name.to_string()
                    }
                } else {
                    format!("{} files", files.len())
                }
            }
            ClipboardData::Url(u) => {
                if u.len() > max_len {
                    format!("{}...", &u[..max_len])
                } else {
                    u.clone()
                }
            }
            ClipboardData::Custom { format, .. } => {
                format!("Custom: {}", format)
            }
        }
    }
    
    pub fn data_type(&self) -> &'static str {
        match self {
            ClipboardData::Empty => "empty",
            ClipboardData::Text(_) => "text",
            ClipboardData::RichText { .. } => "rich text",
            ClipboardData::Image { .. } => "image",
            ClipboardData::Files(_) => "files",
            ClipboardData::Url(_) => "url",
            ClipboardData::Custom { .. } => "custom",
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            ClipboardData::Empty => "📋",
            ClipboardData::Text(_) => "📝",
            ClipboardData::RichText { .. } => "📄",
            ClipboardData::Image { .. } => "🖼",
            ClipboardData::Files(_) => "📁",
            ClipboardData::Url(_) => "🔗",
            ClipboardData::Custom { .. } => "📦",
        }
    }
}

// ============================================================================
// CLIPBOARD ITEM
// ============================================================================

/// A clipboard history item
#[derive(Clone, Debug)]
pub struct ClipboardItem {
    /// Item ID
    pub id: u32,
    /// Clipboard data
    pub data: ClipboardData,
    /// Source application
    pub source_app: String,
    /// Timestamp (seconds since epoch)
    pub timestamp: u64,
    /// Is pinned (won't be removed)
    pub pinned: bool,
    /// Is favorite
    pub favorite: bool,
    /// Copy count
    pub copy_count: u32,
    /// Tags
    pub tags: Vec<String>,
}

impl ClipboardItem {
    pub fn new(id: u32, data: ClipboardData, source: &str) -> Self {
        ClipboardItem {
            id,
            data,
            source_app: String::from(source),
            timestamp: 0, // Would use actual time
            pinned: false,
            favorite: false,
            copy_count: 1,
            tags: Vec::new(),
        }
    }
    
    pub fn touch(&mut self) {
        self.timestamp = 0; // Would update with actual time
        self.copy_count += 1;
    }
    
    pub fn format_time(&self) -> String {
        // Would format actual timestamp
        String::from("Just now")
    }
}

// ============================================================================
// CLIPBOARD MANAGER
// ============================================================================

/// Clipboard manager with history
pub struct ClipboardManager {
    /// Current clipboard content
    pub current: ClipboardData,
    /// History items (most recent first)
    pub history: VecDeque<ClipboardItem>,
    /// Next item ID
    pub next_id: u32,
    /// Maximum history size
    pub max_history: usize,
    /// Sync across devices
    pub sync_enabled: bool,
    /// Show in menu bar
    pub show_in_menu: bool,
    /// Keyboard shortcut
    pub shortcut: String,
    /// Selected item in history
    pub selected_item: Option<u32>,
    /// Search query
    pub search_query: String,
    /// Filter type
    pub filter_type: Option<ClipboardFilter>,
    /// Hovered item
    pub hovered_item: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardFilter {
    All,
    Text,
    Images,
    Files,
    URLs,
    Pinned,
}

impl ClipboardManager {
    pub fn new() -> Self {
        ClipboardManager {
            current: ClipboardData::Empty,
            history: VecDeque::with_capacity(MAX_HISTORY),
            next_id: 1,
            max_history: MAX_HISTORY,
            sync_enabled: false,
            show_in_menu: true,
            shortcut: String::from("⌘⇧V"),
            selected_item: None,
            search_query: String::new(),
            filter_type: None,
            hovered_item: None,
        }
    }
    
    /// Copy data to clipboard
    pub fn copy(&mut self, data: ClipboardData, source: &str) {
        if data.is_empty() || data.size() > MAX_ITEM_SIZE {
            return;
        }
        
        // Check if same as current
        if self.is_same_data(&data) {
            // Update timestamp of existing item
            if let Some(item) = self.history.front_mut() {
                item.touch();
            }
            return;
        }
        
        // Create new item
        let item = ClipboardItem::new(self.next_id, data.clone(), source);
        self.next_id += 1;
        
        // Add to history
        self.history.push_front(item);
        
        // Trim history
        while self.history.len() > self.max_history {
            // Don't remove pinned items
            if let Some(last) = self.history.back() {
                if last.pinned {
                    // Find non-pinned item to remove
                    let idx = self.history.iter().rposition(|i| !i.pinned);
                    if let Some(idx) = idx {
                        self.history.remove(idx);
                    }
                } else {
                    self.history.pop_back();
                }
            }
        }
        
        // Update current
        self.current = data;
    }
    
    fn is_same_data(&self, data: &ClipboardData) -> bool {
        match (&self.current, data) {
            (ClipboardData::Text(a), ClipboardData::Text(b)) => a == b,
            (ClipboardData::Url(a), ClipboardData::Url(b)) => a == b,
            (ClipboardData::Files(a), ClipboardData::Files(b)) => a == b,
            _ => false,
        }
    }
    
    /// Paste from clipboard
    pub fn paste(&self) -> Option<&ClipboardData> {
        if self.current.is_empty() {
            None
        } else {
            Some(&self.current)
        }
    }
    
    /// Paste from history item
    pub fn paste_from_history(&mut self, item_id: u32) -> Option<ClipboardData> {
        for item in &mut self.history.iter_mut() {
            if item.id == item_id {
                item.touch();
                self.current = item.data.clone();
                return Some(item.data.clone());
            }
        }
        None
    }
    
    /// Clear clipboard
    pub fn clear(&mut self) {
        self.current = ClipboardData::Empty;
    }
    
    /// Clear history
    pub fn clear_history(&mut self) {
        // Keep pinned items
        self.history.retain(|i| i.pinned);
    }
    
    /// Pin/unpin item
    pub fn toggle_pin(&mut self, item_id: u32) {
        for item in &mut self.history {
            if item.id == item_id {
                item.pinned = !item.pinned;
                break;
            }
        }
    }
    
    /// Toggle favorite
    pub fn toggle_favorite(&mut self, item_id: u32) {
        for item in &mut self.history {
            if item.id == item_id {
                item.favorite = !item.favorite;
                break;
            }
        }
    }
    
    /// Delete item
    pub fn delete_item(&mut self, item_id: u32) {
        self.history.retain(|i| i.id != item_id);
    }
    
    /// Search history
    pub fn search(&mut self) {
        if self.search_query.is_empty() {
            self.filter_type = None;
            return;
        }
        
        let query = self.search_query.to_lowercase();
        
        for item in &self.history {
            let matches = match &item.data {
                ClipboardData::Text(t) => t.to_lowercase().contains(&query),
                ClipboardData::RichText { plain, .. } => plain.to_lowercase().contains(&query),
                ClipboardData::Url(u) => u.to_lowercase().contains(&query),
                ClipboardData::Files(files) => files.iter().any(|f| f.to_lowercase().contains(&query)),
                _ => false,
            };
            
            // Would mark items as visible/hidden based on match
        }
    }
    
    /// Get filtered history
    pub fn get_filtered_history(&self) -> Vec<&ClipboardItem> {
        let query = self.search_query.to_lowercase();
        
        self.history.iter()
            .filter(|item| {
                // Filter by type
                let type_match = match self.filter_type {
                    None | Some(ClipboardFilter::All) => true,
                    Some(ClipboardFilter::Text) => matches!(item.data, ClipboardData::Text(_) | ClipboardData::RichText { .. }),
                    Some(ClipboardFilter::Images) => matches!(item.data, ClipboardData::Image { .. }),
                    Some(ClipboardFilter::Files) => matches!(item.data, ClipboardData::Files(_)),
                    Some(ClipboardFilter::URLs) => matches!(item.data, ClipboardData::Url(_)),
                    Some(ClipboardFilter::Pinned) => item.pinned,
                };
                
                if !type_match {
                    return false;
                }
                
                // Filter by search query
                if query.is_empty() {
                    return true;
                }
                
                match &item.data {
                    ClipboardData::Text(t) => t.to_lowercase().contains(&query),
                    ClipboardData::RichText { plain, .. } => plain.to_lowercase().contains(&query),
                    ClipboardData::Url(u) => u.to_lowercase().contains(&query),
                    ClipboardData::Files(files) => files.iter().any(|f| f.to_lowercase().contains(&query)),
                    _ => false,
                }
            })
            .collect()
    }
    
    /// Draw clipboard manager UI
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        // Background
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, width, height, Theme::BORDER.to_u32());
        
        // Header
        fb.draw_rect(x, y, width, 40, Theme::TOOLBAR_BG.to_u32());
        fb.draw_string(x + 8, y + 10, "Clipboard History", Theme::TEXT_PRIMARY.to_u32());
        
        // Search field
        let search_y = y + 48;
        fb.draw_rect(x + 8, search_y, width - 16, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 16, search_y + 6, "🔍", Theme::TEXT_SECONDARY.to_u32());
        
        if self.search_query.is_empty() {
            fb.draw_string(x + 36, search_y + 6, "Search clipboard...", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(x + 36, search_y + 6, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }
        
        // Filter tabs
        let tabs_y = search_y + 36;
        let tabs = ["All", "Text", "Images", "Files", "Pinned"];
        let mut tab_x = x + 8;
        
        for (i, tab) in tabs.iter().enumerate() {
            let is_active = match (self.filter_type, i) {
                (None, 0) | (Some(ClipboardFilter::All), 0) => true,
                (Some(ClipboardFilter::Text), 1) => true,
                (Some(ClipboardFilter::Images), 2) => true,
                (Some(ClipboardFilter::Files), 3) => true,
                (Some(ClipboardFilter::URLs), 4) => true,
                (Some(ClipboardFilter::Pinned), 5) => true,
                _ => false,
            };
            
            let bg = if is_active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            let text_color = if is_active { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(tab_x, tabs_y, tab.len() * 8 + 16, 24, bg);
            fb.draw_string(tab_x + 8, tabs_y + 4, tab, text_color);
            
            tab_x += tab.len() * 8 + 20;
        }
        
        // History list
        let list_y = tabs_y + 32;
        let list_height = height - 140;
        let item_height = 64;
        
        let filtered = self.get_filtered_history();
        
        for (i, item) in filtered.iter().enumerate() {
            let item_y = list_y + i * item_height;
            
            if item_y + item_height > y + height {
                break;
            }
            
            let is_selected = self.selected_item == Some(item.id);
            let is_hovered = self.hovered_item == Some(item.id);
            
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::WINDOW_BG.to_u32() };
            
            fb.draw_rect(x, item_y, width, item_height, bg);
            
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let secondary_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
            
            // Icon
            fb.draw_string(x + 8, item_y + 8, item.data.icon(), text_color);
            
            // Preview
            let preview = item.data.preview(40);
            fb.draw_string(x + 36, item_y + 8, &preview, text_color);
            
            // Type and size
            let info = format!("{} • {}", item.data.data_type(), item.data.format_size());
            fb.draw_string(x + 36, item_y + 28, &info, secondary_color);
            
            // Time and source
            let meta = format!("{} • {}", item.format_time(), item.source_app);
            fb.draw_string(x + 36, item_y + 44, &meta, secondary_color);
            
            // Pinned indicator
            if item.pinned {
                fb.draw_string(x + width - 24, item_y + 8, "📌", text_color);
            }
            
            // Favorite indicator
            if item.favorite {
                fb.draw_string(x + width - 48, item_y + 8, "⭐", text_color);
            }
        }
        
        // Empty state
        if filtered.is_empty() {
            let empty_text = if self.search_query.is_empty() {
                "No clipboard items"
            } else {
                "No matching items"
            };
            fb.draw_string(x + width / 2 - empty_text.len() * 4, y + height / 2, empty_text, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Footer
        let footer_y = y + height - 32;
        fb.draw_rect(x, footer_y, width, 32, Theme::TOOLBAR_BG.to_u32());
        
        let count_text = format!("{} items", self.history.len());
        fb.draw_string(x + 8, footer_y + 8, &count_text, Theme::TEXT_SECONDARY.to_u32());
        
        // Keyboard shortcut hint
        fb.draw_string(x + width - 80, footer_y + 8, &self.shortcut, Theme::TEXT_SECONDARY.to_u32());
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32, x: usize, y: usize, width: usize, height: usize) -> ClipboardAction {
        // Search field
        let search_y = y + 48;
        if mx >= (x + 8) as i32 && mx < (x + width - 8) as i32
            && my >= search_y as i32 && my < (search_y + 28) as i32 {
            return ClipboardAction::FocusSearch;
        }
        
        // Filter tabs
        let tabs_y = search_y + 36;
        let tabs = [
            ClipboardFilter::All,
            ClipboardFilter::Text,
            ClipboardFilter::Images,
            ClipboardFilter::Files,
            ClipboardFilter::URLs,
            ClipboardFilter::Pinned,
        ];
        let mut tab_x = x + 8;
        
        for filter in tabs {
            let tab_name = match filter {
                ClipboardFilter::All => "All",
                ClipboardFilter::Text => "Text",
                ClipboardFilter::Images => "Images",
                ClipboardFilter::Files => "Files",
                ClipboardFilter::Pinned => "Pinned",
                ClipboardFilter::URLs => "URLs",
            };
            
            let tab_width = tab_name.len() * 8 + 16;
            
            if mx >= tab_x as i32 && mx < (tab_x + tab_width) as i32
                && my >= tabs_y as i32 && my < (tabs_y + 24) as i32 {
                self.filter_type = Some(filter);
                return ClipboardAction::None;
            }
            
            tab_x += tab_width + 4;
        }
        
        // History items
        let list_y = tabs_y + 32;
        let item_height = 64;
        let filtered = self.get_filtered_history();
        
        for (i, item) in filtered.iter().enumerate() {
            let item_y = list_y + i * item_height;
            
            if my >= item_y as i32 && my < (item_y + item_height) as i32 {
                // Check if clicking on pin button
                if mx >= (x + width - 24) as i32 {
                    return ClipboardAction::TogglePin(item.id);
                }
                
                // Check if clicking on favorite button
                if mx >= (x + width - 48) as i32 && mx < (x + width - 24) as i32 {
                    return ClipboardAction::ToggleFavorite(item.id);
                }
                
                // Select and paste
                let selected_id = item.id;
                self.selected_item = Some(selected_id);
                return ClipboardAction::SelectItem(selected_id);
            }
        }
        
        ClipboardAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> ClipboardAction {
        if c == '\x08' { // Backspace
            self.search_query.pop();
        } else if c == '\x1b' { // Escape
            self.search_query.clear();
            self.filter_type = None;
        } else if c == '\n' { // Enter
            if let Some(&id) = self.selected_item.as_ref() {
                return ClipboardAction::PasteItem(id);
            }
        } else if !c.is_control() {
            self.search_query.push(c);
        }
        
        ClipboardAction::None
    }
}

/// Clipboard actions
#[derive(Clone, Debug)]
pub enum ClipboardAction {
    None,
    Copy(ClipboardData),
    Paste,
    PasteItem(u32),
    SelectItem(u32),
    TogglePin(u32),
    ToggleFavorite(u32),
    DeleteItem(u32),
    ClearHistory,
    FocusSearch,
}

// ============================================================================
// GLOBAL CLIPBOARD
// ============================================================================

lazy_static::lazy_static! {
    static ref CLIPBOARD: Mutex<ClipboardManager> = Mutex::new(ClipboardManager::new());
}

/// Initialize clipboard
pub fn init() {
    crate::serial_println!("[GUI] Clipboard manager initialized");
}

/// Get clipboard manager
pub fn get_clipboard() -> &'static Mutex<ClipboardManager> {
    &CLIPBOARD
}
