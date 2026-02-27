//! # File Explorer Application
//!
//! Modern file browser with navigation, search, and file operations
//! Supports multiple view modes and context menus

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// FILE ENTRY
// ============================================================================

/// File or directory entry
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: u64,
    pub modified: u64,
    pub file_type: FileType,
    pub is_hidden: bool,
    pub is_readonly: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileType {
    Unknown,
    Directory,
    File,
    Text,
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Executable,
    Document,
}

impl FileType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "txt" | "md" | "log" | "cfg" | "conf" | "ini" => FileType::Text,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => FileType::Image,
            "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" => FileType::Audio,
            "mp4" | "avi" | "mkv" | "mov" | "webm" => FileType::Video,
            "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" => FileType::Archive,
            "rs" | "c" | "cpp" | "h" | "py" | "js" | "ts" | "go" | "java" | "kt" | "rb" | "php" => FileType::Code,
            "exe" | "bin" | "sh" | "bat" | "app" => FileType::Executable,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => FileType::Document,
            _ => FileType::File,
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            FileType::Unknown => "📄",
            FileType::Directory => "📁",
            FileType::File => "📄",
            FileType::Text => "📝",
            FileType::Image => "🖼",
            FileType::Audio => "🎵",
            FileType::Video => "🎬",
            FileType::Archive => "📦",
            FileType::Code => "💻",
            FileType::Executable => "⚙",
            FileType::Document => "📑",
        }
    }
    
    pub fn color(&self) -> u32 {
        match self {
            FileType::Directory => 0xFFC107,      // Yellow
            FileType::Image => 0x4CAF50,          // Green
            FileType::Audio => 0xE91E63,          // Pink
            FileType::Video => 0xFF5722,          // Orange
            FileType::Code => 0x00BCD4,           // Cyan
            FileType::Executable => 0x9C27B0,     // Purple
            FileType::Archive => 0x795548,        // Brown
            _ => Theme::TEXT_PRIMARY.to_u32(),
        }
    }
}

impl FileEntry {
    pub fn directory(name: &str, path: &str) -> Self {
        FileEntry {
            name: String::from(name),
            path: String::from(path),
            is_directory: true,
            size: 0,
            modified: 0,
            file_type: FileType::Directory,
            is_hidden: name.starts_with('.'),
            is_readonly: false,
        }
    }
    
    pub fn file(name: &str, path: &str, size: u64) -> Self {
        let ext = name.rsplit('.').next().unwrap_or("");
        FileEntry {
            name: String::from(name),
            path: String::from(path),
            is_directory: false,
            size,
            modified: 0,
            file_type: FileType::from_extension(ext),
            is_hidden: name.starts_with('.'),
            is_readonly: false,
        }
    }
    
    pub fn format_size(&self) -> String {
        if self.is_directory {
            String::from("--")
        } else if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else if self.size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", self.size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

// ============================================================================
// VIEW MODE
// ============================================================================

/// File view mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Icons,
    List,
    Details,
    Tiles,
}

impl ViewMode {
    pub fn icon(&self) -> &'static str {
        match self {
            ViewMode::Icons => "⊞",
            ViewMode::List => "☰",
            ViewMode::Details => "▦",
            ViewMode::Tiles => "◫",
        }
    }
}

// ============================================================================
// FILE EXPLORER
// ============================================================================

/// File Explorer Application
pub struct FileExplorer {
    /// Window position and size
    rect: Rect,
    /// Current path
    current_path: String,
    /// Navigation history
    history: VecDeque<String>,
    /// History position
    history_pos: usize,
    /// File entries
    entries: Vec<FileEntry>,
    /// Selected entries
    selected: Vec<usize>,
    /// Scroll offset
    scroll_offset: usize,
    /// View mode
    view_mode: ViewMode,
    /// Show hidden files
    show_hidden: bool,
    /// Sort by
    sort_by: SortBy,
    /// Sort ascending
    sort_ascending: bool,
    /// Search query
    search_query: String,
    /// Hovered entry
    hovered_entry: Option<usize>,
    /// Editing name
    editing_name: Option<usize>,
    /// Context menu
    context_menu: Option<ContextMenu>,
    /// Toolbar height
    toolbar_height: usize,
    /// Status bar height
    status_height: usize,
    /// Sidebar width
    sidebar_width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Type,
    Modified,
}

#[derive(Clone, Debug)]
pub struct ContextMenu {
    x: i32,
    y: i32,
    items: Vec<MenuItem>,
    selected: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    label: String,
    action: MenuAction,
    enabled: bool,
    separator: bool,
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    Open,
    OpenWith,
    Cut,
    Copy,
    Paste,
    Delete,
    Rename,
    NewFolder,
    NewFile,
    Properties,
    Refresh,
}

impl FileExplorer {
    pub fn new() -> Self {
        FileExplorer {
            rect: Rect::new(150, 100, 900, 600),
            current_path: String::from("/"),
            history: VecDeque::new(),
            history_pos: 0,
            entries: Vec::new(),
            selected: Vec::new(),
            scroll_offset: 0,
            view_mode: ViewMode::Icons,
            show_hidden: false,
            sort_by: SortBy::Name,
            sort_ascending: true,
            search_query: String::new(),
            hovered_entry: None,
            editing_name: None,
            context_menu: None,
            toolbar_height: 40,
            status_height: 28,
            sidebar_width: 180,
        }
    }
    
    /// Navigate to path
    pub fn navigate_to(&mut self, path: &str) {
        // Add current to history
        if self.history_pos >= self.history.len() {
            self.history.push_back(self.current_path.clone());
        } else {
            self.history.truncate(self.history_pos + 1);
            self.history.push_back(self.current_path.clone());
        }
        self.history_pos = self.history.len() - 1;
        
        self.current_path = String::from(path);
        self.load_directory();
        self.scroll_offset = 0;
        self.selected.clear();
    }
    
    /// Go back
    pub fn go_back(&mut self) {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            if let Some(path) = self.history.get(self.history_pos) {
                self.current_path = path.clone();
                self.load_directory();
            }
        }
    }
    
    /// Go forward
    pub fn go_forward(&mut self) {
        if self.history_pos < self.history.len() - 1 {
            self.history_pos += 1;
            if let Some(path) = self.history.get(self.history_pos) {
                self.current_path = path.clone();
                self.load_directory();
            }
        }
    }
    
    /// Go up one directory
    pub fn go_up(&mut self) {
        if self.current_path != "/" {
            let parent = self.current_path.rfind('/').map(|i| {
                if i == 0 { "/" } else { &self.current_path[..i] }
            }).unwrap_or("/");
            let parent = parent.to_string();
            self.navigate_to(&parent);
        }
    }
    
    /// Load directory contents
    fn load_directory(&mut self) {
        self.entries.clear();
        
        // Try to read directory from filesystem
        if let Ok(entries) = crate::fs::f2fs::list_dir(&self.current_path) {
            for entry in entries {
                let path = if self.current_path.ends_with('/') {
                    format!("{}{}", self.current_path, entry.name)
                } else {
                    format!("{}/{}", self.current_path, entry.name)
                };
                let file_entry = if entry.is_dir {
                    FileEntry::directory(&entry.name, &path)
                } else {
                    FileEntry::file(&entry.name, &path, entry.size as u64)
                };
                
                if self.show_hidden || !file_entry.is_hidden {
                    self.entries.push(file_entry);
                }
            }
        }
        
        // If empty, add placeholder entries
        if self.entries.is_empty() && self.current_path == "/" {
            self.entries.push(FileEntry::directory("home", "/home"));
            self.entries.push(FileEntry::directory("system", "/system"));
            self.entries.push(FileEntry::directory("tmp", "/tmp"));
        }
        
        self.sort_entries();
    }
    
    /// Sort entries
    fn sort_entries(&mut self) {
        // Directories first, then sort
        self.entries.sort_by(|a, b| {
            if a.is_directory != b.is_directory {
                return b.is_directory.cmp(&a.is_directory);
            }
            
            let cmp = match self.sort_by {
                SortBy::Name => a.name.cmp(&b.name),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Type => a.file_type.cmp(&b.file_type),
                SortBy::Modified => a.modified.cmp(&b.modified),
            };
            
            if self.sort_ascending { cmp } else { cmp.reverse() }
        });
    }
    
    /// Draw the file explorer
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;
        
        // Window background (WM Cyber titlebar is the chrome — no internal title/toolbar)
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());

        // Sidebar
        self.draw_sidebar(fb, x, y);

        // Main content
        let content_x = x + self.sidebar_width;
        let content_y = y;
        let content_width = width - self.sidebar_width;
        let content_height = height.saturating_sub(self.status_height);
        
        self.draw_content(fb, content_x, content_y, content_width, content_height);
        
        // Status bar
        self.draw_status_bar(fb, x, y + height - self.status_height, width);
        
        // Context menu
        if let Some(ref menu) = self.context_menu {
            self.draw_context_menu(fb, menu);
        }
    }
    
    fn draw_title_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        
        // Title
        let title = format!("File Explorer - {}", self.current_path);
        fb.draw_string(x + 12, y + 8, &title, Theme::TEXT_PRIMARY.to_u32());
        
        // Close button
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());
        
        // Minimize button
        fb.draw_rect(x + width - 56, y + 4, 24, 24, Theme::BORDER.to_u32());
        fb.draw_string(x + width - 48, y + 8, "−", Theme::TEXT_PRIMARY.to_u32());
        
        // Maximize button
        fb.draw_rect(x + width - 84, y + 4, 24, 24, Theme::BORDER.to_u32());
        fb.draw_string(x + width - 76, y + 8, "□", Theme::TEXT_PRIMARY.to_u32());
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, self.toolbar_height, Theme::TOOLBAR_BG.to_u32());
        
        let mut btn_x = x + 8;
        let btn_y = y + 6;
        let btn_size = 28;
        
        // Back button
        self.draw_toolbar_button(fb, btn_x, btn_y, "◀", self.history_pos > 0);
        btn_x += btn_size + 4;
        
        // Forward button
        self.draw_toolbar_button(fb, btn_x, btn_y, "▶", self.history_pos < self.history.len().saturating_sub(1));
        btn_x += btn_size + 4;
        
        // Up button
        self.draw_toolbar_button(fb, btn_x, btn_y, "▲", self.current_path != "/");
        btn_x += btn_size + 4;
        
        // Refresh button
        self.draw_toolbar_button(fb, btn_x, btn_y, "↻", true);
        btn_x += btn_size + 8;
        
        // Separator
        fb.draw_rect(btn_x, btn_y, 1, btn_size, Theme::BORDER.to_u32());
        btn_x += 8;
        
        // View mode buttons
        let views = [ViewMode::Icons, ViewMode::List, ViewMode::Details, ViewMode::Tiles];
        for view in &views {
            let is_active = *view == self.view_mode;
            self.draw_toolbar_button_active(fb, btn_x, btn_y, view.icon(), true, is_active);
            btn_x += btn_size + 2;
        }
        
        btn_x += 8;
        
        // Separator
        fb.draw_rect(btn_x, btn_y, 1, btn_size, Theme::BORDER.to_u32());
        btn_x += 8;
        
        // Search box
        let search_width = 200;
        fb.draw_rect(btn_x, btn_y, search_width, btn_size, Theme::INPUT_BG.to_u32());
        fb.draw_rect_outline(btn_x, btn_y, search_width, btn_size, Theme::BORDER.to_u32());
        
        if self.search_query.is_empty() {
            fb.draw_string(btn_x + 8, btn_y + 6, "🔍 Search...", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(btn_x + 8, btn_y + 6, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn draw_toolbar_button(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str, enabled: bool) {
        let color = if enabled { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(x, y, 28, 28, Theme::TRANSPARENT.to_u32());
        fb.draw_string(x + 8, y + 6, icon, color);
    }
    
    fn draw_toolbar_button_active(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str, enabled: bool, active: bool) {
        let bg = if active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
        let color = if !enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else if active {
            Theme::TEXT_ON_ACCENT.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };
        
        fb.draw_rect(x, y, 28, 28, bg);
        fb.draw_string(x + 8, y + 6, icon, color);
    }
    
    fn draw_sidebar(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        let height = self.rect.height as usize - 32 - self.toolbar_height - self.status_height;
        
        fb.draw_rect(x, y, self.sidebar_width, height, Theme::SIDEBAR_BG.to_u32());
        
        // Quick access items
        let quick_access = [
            ("🏠", "Home", "/home"),
            ("📁", "Documents", "/home/documents"),
            ("⬇", "Downloads", "/home/downloads"),
            ("🖼", "Pictures", "/home/pictures"),
            ("🎵", "Music", "/home/music"),
            ("🎬", "Videos", "/home/videos"),
        ];
        
        let mut item_y = y + 16;
        fb.draw_string(x + 12, item_y, "Quick Access", Theme::TEXT_SECONDARY.to_u32());
        item_y += 24;
        
        for (icon, name, _path) in &quick_access {
            let is_active = self.current_path == *name.to_lowercase();
            let bg = if is_active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            let text_color = if is_active { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(x + 4, item_y, self.sidebar_width - 8, 28, bg);
            fb.draw_string(x + 12, item_y + 6, icon, text_color);
            fb.draw_string(x + 36, item_y + 6, name, text_color);
            
            item_y += 32;
        }
        
        // This PC
        item_y += 16;
        fb.draw_string(x + 12, item_y, "This PC", Theme::TEXT_SECONDARY.to_u32());
        item_y += 24;
        
        let drives = [
            ("💾", "Local Disk", "/"),
            ("💿", "CD Drive", "/media/cd"),
        ];
        
        for (icon, name, _path) in &drives {
            fb.draw_rect(x + 4, item_y, self.sidebar_width - 8, 28, Theme::TRANSPARENT.to_u32());
            fb.draw_string(x + 12, item_y + 6, icon, Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(x + 36, item_y + 6, name, Theme::TEXT_PRIMARY.to_u32());
            
            item_y += 32;
        }
    }
    
    fn draw_content(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        // Background
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        
        // Column headers for details view
        if self.view_mode == ViewMode::Details {
            let header_y = y;
            fb.draw_rect(x, header_y, width, 24, Theme::TOOLBAR_BG.to_u32());
            
            fb.draw_string(x + 8, header_y + 4, "Name", Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(x + 300, header_y + 4, "Type", Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(x + 400, header_y + 4, "Size", Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(x + 500, header_y + 4, "Modified", Theme::TEXT_PRIMARY.to_u32());
            
            fb.draw_rect(x, header_y + 24, width, 1, Theme::BORDER.to_u32());
        }
        
        // Draw entries
        let content_start_y = if self.view_mode == ViewMode::Details { y + 24 } else { y };
        let content_height = height - (content_start_y - y);
        
        match self.view_mode {
            ViewMode::Icons => self.draw_icons_view(fb, x, content_start_y, width, content_height),
            ViewMode::List => self.draw_list_view(fb, x, content_start_y, width, content_height),
            ViewMode::Details => self.draw_details_view(fb, x, content_start_y, width, content_height),
            ViewMode::Tiles => self.draw_tiles_view(fb, x, content_start_y, width, content_height),
        }
    }
    
    fn draw_icons_view(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let icon_size = 80;
        let cols = width / icon_size;
        let padding = 8;
        
        let mut entry_idx = 0;
        let mut row = 0;
        
        while entry_idx < self.entries.len() {
            let col = entry_idx % cols;
            
            let icon_x = x + col * icon_size + padding;
            let icon_y = y as i32 + row as i32 * (icon_size as i32 + padding as i32) as i32 - self.scroll_offset as i32;
            
            if icon_y + icon_size as i32 >= y as i32 && icon_y < (y + height) as i32 {
                if icon_y >= y as i32 {
                    self.draw_entry_icon(fb, entry_idx, icon_x, icon_y as usize, icon_size - padding * 2);
                }
            }
            
            entry_idx += 1;
            if col == cols - 1 {
                row += 1;
            }
        }
    }
    
    fn draw_list_view(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let item_height = 24;
        
        for (idx, entry) in self.entries.iter().enumerate() {
            let item_y = y as i32 + idx as i32 * item_height - self.scroll_offset as i32;
            
            if item_y + item_height >= y as i32 && item_y < (y + height) as i32 {
                if item_y >= y as i32 {
                    let is_selected = self.selected.contains(&idx);
                    let is_hovered = self.hovered_entry == Some(idx);
                    
                    let bg = if is_selected {
                        Theme::ACCENT_PRIMARY.to_u32()
                    } else if is_hovered {
                        Theme::LIST_ITEM_HOVER.to_u32()
                    } else {
                        Theme::TRANSPARENT.to_u32()
                    };
                    
                    fb.draw_rect(x, item_y.max(0) as usize, width, item_height as usize, bg);
                    
                    let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
                    
                    // Icon
                    fb.draw_string(x + 8, item_y.max(0) as usize + 4, entry.file_type.icon(), text_color);
                    
                    // Name
                    fb.draw_string(x + 32, item_y.max(0) as usize + 4, &entry.name, text_color);
                }
            }
        }
    }
    
    fn draw_details_view(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let item_height = 24;
        
        for (idx, entry) in self.entries.iter().enumerate() {
            let item_y = y as i32 + idx as i32 * item_height - self.scroll_offset as i32;
            
            if item_y + item_height >= y as i32 && item_y < (y + height) as i32 {
                if item_y >= y as i32 {
                    let is_selected = self.selected.contains(&idx);
                    let is_hovered = self.hovered_entry == Some(idx);
                    
                    let bg = if is_selected {
                        Theme::ACCENT_PRIMARY.to_u32()
                    } else if is_hovered {
                        Theme::LIST_ITEM_HOVER.to_u32()
                    } else {
                        Theme::TRANSPARENT.to_u32()
                    };
                    
                    fb.draw_rect(x, item_y.max(0) as usize, width, item_height as usize, bg);
                    
                    let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
                    
                    // Icon + Name
                    fb.draw_string(x + 8, item_y.max(0) as usize + 4, entry.file_type.icon(), text_color);
                    fb.draw_string(x + 32, item_y.max(0) as usize + 4, &entry.name, text_color);
                    
                    // Type
                    let type_name = if entry.is_directory { "Folder" } else { "File" };
                    fb.draw_string(x + 300, item_y.max(0) as usize + 4, type_name, text_color);
                    
                    // Size
                    fb.draw_string(x + 400, item_y.max(0) as usize + 4, &entry.format_size(), text_color);
                    
                    // Modified
                    fb.draw_string(x + 500, item_y.max(0) as usize + 4, "--", text_color);
                }
            }
        }
    }
    
    fn draw_tiles_view(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let tile_width = 200;
        let tile_height = 48;
        let cols = width / tile_width;
        
        for (idx, entry) in self.entries.iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            
            let tile_x = x + col * tile_width;
            let tile_y = y as i32 + row as i32 * tile_height as i32 - self.scroll_offset as i32;
            
            if tile_y + tile_height as i32 >= y as i32 && tile_y < (y + height) as i32 {
                if tile_y >= y as i32 {
                    let is_selected = self.selected.contains(&idx);
                    let is_hovered = self.hovered_entry == Some(idx);
                    
                    let bg = if is_selected {
                        Theme::ACCENT_PRIMARY.to_u32()
                    } else if is_hovered {
                        Theme::LIST_ITEM_HOVER.to_u32()
                    } else {
                        Theme::TRANSPARENT.to_u32()
                    };
                    
                    fb.draw_rect(tile_x, tile_y as usize, tile_width - 4, tile_height, bg);
                    
                    let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
                    
                    // Icon
                    fb.draw_string(tile_x + 8, tile_y as usize + 16, entry.file_type.icon(), text_color);
                    
                    // Name
                    fb.draw_string(tile_x + 48, tile_y as usize + 8, &entry.name, text_color);
                    
                    // Size
                    fb.draw_string(tile_x + 48, tile_y as usize + 24, &entry.format_size(), Theme::TEXT_SECONDARY.to_u32());
                }
            }
        }
    }
    
    fn draw_entry_icon(&self, fb: &mut Framebuffer, idx: usize, x: usize, y: usize, size: usize) {
        let entry = &self.entries[idx];
        let is_selected = self.selected.contains(&idx);
        let is_hovered = self.hovered_entry == Some(idx);
        
        // Selection background
        if is_selected || is_hovered {
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::LIST_ITEM_HOVER.to_u32() };
            fb.draw_rect(x, y, size, size + 20, bg);
        }
        
        // Icon
        let icon_size = size.min(48);
        let icon_x = x + (size - icon_size) / 2;
        let icon_y = y + 8;
        
        // Draw icon background
        let icon_bg = entry.file_type.color();
        fb.draw_rect(icon_x, icon_y, icon_size, icon_size, icon_bg);
        
        // Draw icon symbol
        fb.draw_string(icon_x + icon_size / 2 - 8, icon_y + icon_size / 2 - 6, entry.file_type.icon(), Theme::TEXT_ON_ACCENT.to_u32());
        
        // Name below
        let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
        let name = if entry.name.len() > 12 {
            format!("{}...", &entry.name[..9])
        } else {
            entry.name.clone()
        };
        
        let name_width = name.len() * 8;
        let name_x = x + (size - name_width) / 2;
        fb.draw_string(name_x, y + icon_size + 12, &name, text_color);
    }
    
    fn draw_status_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        fb.draw_rect(x, y, width, self.status_height, Theme::TOOLBAR_BG.to_u32());
        
        // Item count
        let status = format!("{} items", self.entries.len());
        fb.draw_string(x + 12, y + 6, &status, Theme::TEXT_SECONDARY.to_u32());
        
        // Selected count
        if !self.selected.is_empty() {
            let selected_info = format!("  |  {} selected", self.selected.len());
            fb.draw_string(x + 100, y + 6, &selected_info, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // View mode
        fb.draw_string(x + width - 100, y + 6, self.view_mode.icon(), Theme::TEXT_SECONDARY.to_u32());
        fb.draw_string(x + width - 80, y + 6, match self.view_mode {
            ViewMode::Icons => "Icons",
            ViewMode::List => "List",
            ViewMode::Details => "Details",
            ViewMode::Tiles => "Tiles",
        }, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_context_menu(&self, fb: &mut Framebuffer, menu: &ContextMenu) {
        let width = 160;
        let item_height = 28;
        let height = menu.items.len() * item_height;
        
        // Background
        fb.draw_rect(menu.x as usize, menu.y as usize, width, height, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(menu.x as usize, menu.y as usize, width, height, Theme::BORDER.to_u32());
        
        // Items
        for (idx, item) in menu.items.iter().enumerate() {
            let item_y = menu.y as usize + idx * item_height;
            
            if item.separator {
                fb.draw_rect(menu.x as usize + 8, item_y + 12, width - 16, 1, Theme::BORDER.to_u32());
            } else {
                let is_hovered = menu.selected == Some(idx);
                let bg = if is_hovered { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
                let text_color = if item.enabled {
                    if is_hovered { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() }
                } else {
                    Theme::TEXT_DISABLED.to_u32()
                };
                
                fb.draw_rect(menu.x as usize, item_y, width, item_height, bg);
                fb.draw_string(menu.x as usize + 12, item_y + 6, &item.label, text_color);
            }
        }
    }
    
    /// Handle mouse move
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        // Check content area
        let content_x = self.rect.x + self.sidebar_width as i32;
        let content_y = self.rect.y + 32 + self.toolbar_height as i32;
        
        if mx >= content_x && my >= content_y {
            self.hovered_entry = self.hit_test_entry(mx, my);
        } else {
            self.hovered_entry = None;
        }
        
        // Update context menu selection
        if let Some(ref mut menu) = self.context_menu {
            if mx >= menu.x && my >= menu.y {
                let item_idx = ((my - menu.y) / 28) as usize;
                menu.selected = Some(item_idx.min(menu.items.len() - 1));
            }
        }
    }
    
    fn hit_test_entry(&self, mx: i32, my: i32) -> Option<usize> {
        let content_x = self.rect.x + self.sidebar_width as i32;
        let content_y = self.rect.y + 32 + self.toolbar_height as i32;
        let content_height = self.rect.height - 32 - self.toolbar_height as i32 - self.status_height as i32;
        
        let adjusted_y = my - content_y + self.scroll_offset as i32;
        
        match self.view_mode {
            ViewMode::Icons => {
                let icon_size: i32 = 80;
                let cols = (self.rect.width - self.sidebar_width as i32) / icon_size;
                let row = (adjusted_y / icon_size) as usize;
                let col = ((mx - content_x) / icon_size).min(cols - 1) as usize;
                let idx = row * cols as usize + col;
                
                if idx < self.entries.len() { Some(idx) } else { None }
            }
            ViewMode::List | ViewMode::Details => {
                let item_height = 24;
                let idx = (adjusted_y / item_height) as usize;
                if idx < self.entries.len() { Some(idx) } else { None }
            }
            ViewMode::Tiles => {
                let tile_width: i32 = 200;
                let tile_height: i32 = 48;
                let cols = (self.rect.width - self.sidebar_width as i32) / tile_width;
                let row = (adjusted_y / tile_height) as usize;
                let col = ((mx - content_x) / tile_width).min(cols - 1) as usize;
                let idx = row * cols as usize + col;
                
                if idx < self.entries.len() { Some(idx) } else { None }
            }
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32, right_click: bool) -> FileExplorerAction {
        // Close button
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return FileExplorerAction::Close;
        }
        
        // Toolbar buttons
        let toolbar_y = self.rect.y + 32;
        if my >= toolbar_y && my < toolbar_y + self.toolbar_height as i32 {
            let mut btn_x = self.rect.x + 8;
            
            // Back
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_back();
                return FileExplorerAction::Navigate;
            }
            btn_x += 32;
            
            // Forward
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_forward();
                return FileExplorerAction::Navigate;
            }
            btn_x += 32;
            
            // Up
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_up();
                return FileExplorerAction::Navigate;
            }
            btn_x += 32;
            
            // Refresh
            if mx >= btn_x && mx < btn_x + 28 {
                self.load_directory();
                return FileExplorerAction::Refresh;
            }
        }
        
        // Content area
        let content_x = self.rect.x + self.sidebar_width as i32;
        let content_y = self.rect.y + 32 + self.toolbar_height as i32;
        
        if mx >= content_x && my >= content_y {
            if let Some(idx) = self.hit_test_entry(mx, my) {
                if right_click {
                    self.show_context_menu(mx, my, idx);
                    return FileExplorerAction::None;
                }
                
                // Double-click handling would be separate
                if self.entries[idx].is_directory {
                    let path = self.entries[idx].path.clone();
                    self.navigate_to(&path);
                    return FileExplorerAction::Navigate;
                } else {
                    self.selected.clear();
                    self.selected.push(idx);
                    return FileExplorerAction::OpenFile(self.entries[idx].path.clone());
                }
            } else {
                self.selected.clear();
            }
        }
        
        // Context menu
        if let Some(ref menu) = self.context_menu {
            if let Some(idx) = menu.selected {
                if idx < menu.items.len() {
                    let action = menu.items[idx].action.clone();
                    self.context_menu = None;
                    return FileExplorerAction::MenuAction(action);
                }
            }
            self.context_menu = None;
        }
        
        FileExplorerAction::None
    }
    
    fn show_context_menu(&mut self, mx: i32, my: i32, _entry_idx: usize) {
        self.context_menu = Some(ContextMenu {
            x: mx,
            y: my,
            items: vec![
                MenuItem { label: String::from("Open"), action: MenuAction::Open, enabled: true, separator: false },
                MenuItem { label: String::from("Open with..."), action: MenuAction::OpenWith, enabled: true, separator: false },
                MenuItem { label: String::new(), action: MenuAction::Open, enabled: false, separator: true },
                MenuItem { label: String::from("Cut"), action: MenuAction::Cut, enabled: true, separator: false },
                MenuItem { label: String::from("Copy"), action: MenuAction::Copy, enabled: true, separator: false },
                MenuItem { label: String::from("Paste"), action: MenuAction::Paste, enabled: true, separator: false },
                MenuItem { label: String::new(), action: MenuAction::Open, enabled: false, separator: true },
                MenuItem { label: String::from("Delete"), action: MenuAction::Delete, enabled: true, separator: false },
                MenuItem { label: String::from("Rename"), action: MenuAction::Rename, enabled: true, separator: false },
                MenuItem { label: String::new(), action: MenuAction::Open, enabled: false, separator: true },
                MenuItem { label: String::from("Properties"), action: MenuAction::Properties, enabled: true, separator: false },
            ],
            selected: None,
        });
    }
    
    /// Handle scroll
    pub fn on_scroll(&mut self, delta: i32) {
        let item_height = match self.view_mode {
            ViewMode::Icons => 80,
            ViewMode::List | ViewMode::Details => 24,
            ViewMode::Tiles => 48,
        };
        
        self.scroll_offset = (self.scroll_offset as i32 + delta * item_height).max(0) as usize;
    }
    
    /// Set view mode
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.scroll_offset = 0;
    }
    
    /// Get rect
    pub fn rect(&self) -> Rect {
        self.rect
    }
    
    /// Set rect
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }
}

/// Actions from file explorer
#[derive(Clone, Debug)]
pub enum FileExplorerAction {
    None,
    Close,
    Navigate,
    Refresh,
    OpenFile(String),
    MenuAction(MenuAction),
}

impl Default for FileExplorer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL FILE EXPLORER
// ============================================================================

lazy_static::lazy_static! {
    static ref FILE_EXPLORER: Mutex<FileExplorer> = Mutex::new(FileExplorer::new());
}

/// Get file explorer
pub fn get_explorer() -> &'static Mutex<FileExplorer> {
    &FILE_EXPLORER
}

/// Initialize file explorer
pub fn init() {
    let mut explorer = FILE_EXPLORER.lock();
    explorer.load_directory();
    crate::serial_println!("[GUI] File Explorer initialized");
}
