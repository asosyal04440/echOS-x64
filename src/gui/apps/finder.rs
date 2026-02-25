//! # Finder Application
//!
//! macOS Finder-like file browser with sidebar, tabs, and column/list/icon views

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// FINDER CONSTANTS
// ============================================================================

/// Sidebar width
pub const SIDEBAR_WIDTH: usize = 180;

/// Toolbar height
pub const TOOLBAR_HEIGHT: usize = 40;

/// Tab bar height
pub const TAB_BAR_HEIGHT: usize = 28;

/// Status bar height
pub const STATUS_BAR_HEIGHT: usize = 24;

/// Row height in list view
pub const LIST_ROW_HEIGHT: usize = 22;

/// Icon size in icon view
pub const ICON_VIEW_SIZE: usize = 96;

/// Column width in column view
pub const COLUMN_WIDTH: usize = 200;

// ============================================================================
// FILE ENTRY
// ============================================================================

/// File or folder entry
#[derive(Clone, Debug)]
pub struct FinderEntry {
    /// Entry name
    pub name: String,
    /// Full path
    pub path: String,
    /// Is directory
    pub is_dir: bool,
    /// File size
    pub size: u64,
    /// Modified timestamp
    pub modified: u64,
    /// Created timestamp
    pub created: u64,
    /// File extension
    pub extension: String,
    /// Is hidden
    pub hidden: bool,
    /// Is selected
    pub selected: bool,
    /// Is expanded (for folders in list view)
    pub expanded: bool,
    /// Icon type
    pub icon: FileIcon,
    /// Tags/colors
    pub tags: Vec<FileTag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileIcon {
    Folder,
    FolderOpen,
    File,
    Image,
    Document,
    Audio,
    Video,
    Archive,
    Code,
    Executable,
    Symlink,
    Custom(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTag {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Gray,
}

impl FinderEntry {
    pub fn folder(name: &str, path: &str) -> Self {
        FinderEntry {
            name: String::from(name),
            path: String::from(path),
            is_dir: true,
            size: 0,
            modified: 0,
            created: 0,
            extension: String::new(),
            hidden: name.starts_with('.'),
            selected: false,
            expanded: false,
            icon: FileIcon::Folder,
            tags: Vec::new(),
        }
    }
    
    pub fn file(name: &str, path: &str, size: u64) -> Self {
        let extension = name.split('.').last().unwrap_or("").to_lowercase();
        let icon = Self::get_icon_for_extension(&extension);
        
        FinderEntry {
            name: String::from(name),
            path: String::from(path),
            is_dir: false,
            size,
            modified: 0,
            created: 0,
            extension,
            hidden: name.starts_with('.'),
            selected: false,
            expanded: false,
            icon,
            tags: Vec::new(),
        }
    }
    
    fn get_icon_for_extension(ext: &str) -> FileIcon {
        match ext {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" => FileIcon::Image,
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma" => FileIcon::Audio,
            "mp4" | "avi" | "mkv" | "mov" | "webm" | "wmv" => FileIcon::Video,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" => FileIcon::Archive,
            "rs" | "c" | "cpp" | "h" | "hpp" | "py" | "js" | "ts" | "go" | "java" | "kt" | "swift" => FileIcon::Code,
            "txt" | "md" | "rtf" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => FileIcon::Document,
            "exe" | "bin" | "app" | "dmg" | "pkg" | "deb" | "rpm" => FileIcon::Executable,
            _ => FileIcon::File,
        }
    }
    
    fn format_size(&self) -> String {
        if self.is_dir {
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
    
    fn format_date(&self) -> String {
        // Would format timestamp
        String::from("Jan 1, 2024")
    }
}

// ============================================================================
// SIDEBAR ITEM
// ============================================================================

/// Sidebar item
#[derive(Clone, Debug)]
pub struct SidebarItem {
    /// Item name
    pub name: String,
    /// Path or action
    pub path: String,
    /// Icon
    pub icon: SidebarIcon,
    /// Is selected
    pub selected: bool,
    /// Is section header
    pub is_header: bool,
    /// Can eject (for external drives)
    pub can_eject: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarIcon {
    AirDrop,
    Recents,
    Applications,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Home,
    Favorites,
    iCloud,
    External,
    Network,
    Tag(FileTag),
    Folder,
}

impl SidebarItem {
    pub fn new(name: &str, path: &str, icon: SidebarIcon) -> Self {
        SidebarItem {
            name: String::from(name),
            path: String::from(path),
            icon,
            selected: false,
            is_header: false,
            can_eject: false,
        }
    }
    
    pub fn header(name: &str) -> Self {
        SidebarItem {
            name: String::from(name),
            path: String::new(),
            icon: SidebarIcon::Favorites,
            selected: false,
            is_header: true,
            can_eject: false,
        }
    }
}

// ============================================================================
// FINDER TAB
// ============================================================================

/// A tab in Finder
#[derive(Clone, Debug)]
pub struct FinderTab {
    /// Tab ID
    pub id: u32,
    /// Current path
    pub path: String,
    /// Tab title
    pub title: String,
    /// View mode
    pub view_mode: ViewMode,
    /// Scroll position
    pub scroll_offset: usize,
    /// Sort column
    pub sort_column: SortColumn,
    /// Sort ascending
    pub sort_ascending: bool,
    /// Selected entries
    pub selected_entries: Vec<usize>,
    /// History
    pub history: Vec<String>,
    /// History position
    pub history_pos: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Icons,
    List,
    Columns,
    Gallery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Date,
    Kind,
}

impl FinderTab {
    pub fn new(id: u32, path: &str) -> Self {
        FinderTab {
            id,
            path: String::from(path),
            title: String::from("Home"),
            view_mode: ViewMode::List,
            scroll_offset: 0,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            selected_entries: Vec::new(),
            history: vec![String::from(path)],
            history_pos: 0,
        }
    }
    
    pub fn navigate(&mut self, path: &str) {
        // Add to history
        if self.history_pos < self.history.len() - 1 {
            self.history.truncate(self.history_pos + 1);
        }
        self.history.push(String::from(path));
        self.history_pos = self.history.len() - 1;
        
        self.path = String::from(path);
        self.selected_entries.clear();
        self.scroll_offset = 0;
        self.update_title();
    }
    
    pub fn go_back(&mut self) -> bool {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            self.path = self.history[self.history_pos].clone();
            self.selected_entries.clear();
            self.update_title();
            return true;
        }
        false
    }
    
    pub fn go_forward(&mut self) -> bool {
        if self.history_pos < self.history.len() - 1 {
            self.history_pos += 1;
            self.path = self.history[self.history_pos].clone();
            self.selected_entries.clear();
            self.update_title();
            return true;
        }
        false
    }
    
    fn update_title(&mut self) {
        self.title = self.path.rsplit('/').next().unwrap_or("Finder").to_string();
    }
}

// ============================================================================
// FINDER WINDOW
// ============================================================================

/// Finder window
pub struct FinderWindow {
    /// Window rect
    pub rect: Rect,
    /// Tabs
    pub tabs: Vec<FinderTab>,
    /// Active tab index
    pub active_tab: usize,
    /// Sidebar items
    pub sidebar: Vec<SidebarItem>,
    /// Current directory entries
    pub entries: Vec<FinderEntry>,
    /// Show sidebar
    pub show_sidebar: bool,
    /// Show status bar
    pub show_status_bar: bool,
    /// Show hidden files
    pub show_hidden: bool,
    /// Search query
    pub search_query: String,
    /// Is searching
    pub searching: bool,
    /// Search results
    pub search_results: Vec<FinderEntry>,
    /// Hovered sidebar item
    pub hovered_sidebar: Option<usize>,
    /// Hovered entry
    pub hovered_entry: Option<usize>,
    /// Column scroll offsets (for column view)
    pub column_offsets: Vec<usize>,
    /// Column paths (for column view)
    pub column_paths: Vec<String>,
    /// Next tab ID
    pub next_tab_id: u32,
    /// Dragging entry
    pub dragging_entry: Option<usize>,
    /// Drop target
    pub drop_target: Option<usize>,
    /// Rename target
    pub rename_target: Option<usize>,
    /// Rename text
    pub rename_text: String,
}

impl FinderWindow {
    pub fn new(rect: Rect) -> Self {
        let mut finder = FinderWindow {
            rect,
            tabs: Vec::new(),
            active_tab: 0,
            sidebar: Vec::new(),
            entries: Vec::new(),
            show_sidebar: true,
            show_status_bar: true,
            show_hidden: false,
            search_query: String::new(),
            searching: false,
            search_results: Vec::new(),
            hovered_sidebar: None,
            hovered_entry: None,
            column_offsets: Vec::new(),
            column_paths: Vec::new(),
            next_tab_id: 1,
            dragging_entry: None,
            drop_target: None,
            rename_target: None,
            rename_text: String::new(),
        };
        
        finder.init_sidebar();
        finder.new_tab("/home");
        finder.load_directory();
        
        finder
    }
    
    fn init_sidebar(&mut self) {
        self.sidebar = vec![
            SidebarItem::header("Favorites"),
            SidebarItem::new("AirDrop", "airdrop://", SidebarIcon::AirDrop),
            SidebarItem::new("Recents", "recents://", SidebarIcon::Recents),
            SidebarItem::new("Applications", "/applications", SidebarIcon::Applications),
            SidebarItem::new("Desktop", "/home/desktop", SidebarIcon::Desktop),
            SidebarItem::new("Documents", "/home/documents", SidebarIcon::Documents),
            SidebarItem::new("Downloads", "/home/downloads", SidebarIcon::Downloads),
            SidebarItem::new("Pictures", "/home/pictures", SidebarIcon::Pictures),
            SidebarItem::new("Music", "/home/music", SidebarIcon::Music),
            SidebarItem::new("Videos", "/home/videos", SidebarIcon::Videos),
            SidebarItem::new("Home", "/home", SidebarIcon::Home),
            SidebarItem::header("iCloud"),
            SidebarItem::new("iCloud Drive", "icloud://", SidebarIcon::iCloud),
            SidebarItem::header("Locations"),
            SidebarItem::new("Network", "network://", SidebarIcon::Network),
        ];
    }
    
    pub fn new_tab(&mut self, path: &str) {
        let tab = FinderTab::new(self.next_tab_id, path);
        self.next_tab_id += 1;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.load_directory();
    }
    
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() > 1 {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            self.load_directory();
        }
    }
    
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.load_directory();
        }
    }
    
    pub fn load_directory(&mut self) {
        self.entries.clear();
        
        let tab = &self.tabs[self.active_tab];
        
        // Add parent directory
        if tab.path != "/" {
            self.entries.push(FinderEntry::folder("..", &format!("{}/..", tab.path)));
        }
        
        // Add test entries (would load from filesystem)
        self.entries.push(FinderEntry::folder("Documents", "/home/documents"));
        self.entries.push(FinderEntry::folder("Pictures", "/home/pictures"));
        self.entries.push(FinderEntry::folder("Music", "/home/music"));
        self.entries.push(FinderEntry::folder("Videos", "/home/videos"));
        self.entries.push(FinderEntry::folder("Downloads", "/home/downloads"));
        self.entries.push(FinderEntry::folder("Desktop", "/home/desktop"));
        self.entries.push(FinderEntry::folder("Projects", "/home/projects"));
        self.entries.push(FinderEntry::folder(".config", "/home/.config"));
        self.entries.push(FinderEntry::file("readme.txt", "/home/readme.txt", 1024));
        self.entries.push(FinderEntry::file("notes.md", "/home/notes.md", 2048));
        self.entries.push(FinderEntry::file("photo.png", "/home/photo.png", 2048000));
        self.entries.push(FinderEntry::file("music.mp3", "/home/music.mp3", 4096000));
        self.entries.push(FinderEntry::file("video.mp4", "/home/video.mp4", 102400000));
        self.entries.push(FinderEntry::file("archive.zip", "/home/archive.zip", 10240000));
        self.entries.push(FinderEntry::file("source.rs", "/home/source.rs", 8192));
        self.entries.push(FinderEntry::file("document.pdf", "/home/document.pdf", 512000));
        
        // Filter hidden files
        if !self.show_hidden {
            self.entries.retain(|e| !e.hidden);
        }
        
        // Sort
        self.sort_entries();
    }
    
    fn sort_entries(&mut self) {
        let tab = &self.tabs[self.active_tab];
        
        self.entries.sort_by(|a, b| {
            // Folders first
            if a.is_dir && !b.is_dir {
                return core::cmp::Ordering::Less;
            }
            if !a.is_dir && b.is_dir {
                return core::cmp::Ordering::Greater;
            }
            
            let cmp = match tab.sort_column {
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Size => a.size.cmp(&b.size),
                SortColumn::Date => a.modified.cmp(&b.modified),
                SortColumn::Kind => a.icon.cmp(&b.icon),
            };
            
            if tab.sort_ascending { cmp } else { cmp.reverse() }
        });
    }
    
    pub fn navigate(&mut self, path: &str) {
        self.tabs[self.active_tab].navigate(path);
        self.load_directory();
    }
    
    pub fn go_back(&mut self) {
        if self.tabs[self.active_tab].go_back() {
            self.load_directory();
        }
    }
    
    pub fn go_forward(&mut self) {
        if self.tabs[self.active_tab].go_forward() {
            self.load_directory();
        }
    }
    
    pub fn go_parent(&mut self) {
        let current = self.tabs[self.active_tab].path.clone();
        if current != "/" {
            if let Some(idx) = current.rfind('/') {
                let parent = if idx == 0 { "/" } else { &current[..idx] };
                let parent = parent.to_string();
                self.navigate(&parent);
            }
        }
    }
    
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.tabs[self.active_tab].view_mode = mode;
    }
    
    pub fn toggle_sort(&mut self, column: SortColumn) {
        let tab = &mut self.tabs[self.active_tab];
        if tab.sort_column == column {
            tab.sort_ascending = !tab.sort_ascending;
        } else {
            tab.sort_column = column;
            tab.sort_ascending = true;
        }
        self.sort_entries();
    }
    
    pub fn select_entry(&mut self, index: usize, append: bool) {
        if index >= self.entries.len() {
            return;
        }
        
        if !append {
            for entry in &mut self.entries {
                entry.selected = false;
            }
            self.tabs[self.active_tab].selected_entries.clear();
        }
        
        self.entries[index].selected = true;
        self.tabs[self.active_tab].selected_entries.push(index);
    }
    
    pub fn open_entry(&mut self, index: usize) -> FinderAction {
        if index >= self.entries.len() {
            return FinderAction::None;
        }
        
        let (is_dir, path) = {
            let entry = &self.entries[index];
            (entry.is_dir, entry.path.clone())
        };
        
        if is_dir {
            self.navigate(&path);
            FinderAction::None
        } else {
            FinderAction::OpenFile(path)
        }
    }
    
    pub fn start_search(&mut self) {
        self.searching = true;
        self.search_query.clear();
        self.search_results.clear();
    }
    
    pub fn update_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_results.clear();
            return;
        }
        
        // Simple search (would use indexed search)
        let query = self.search_query.to_lowercase();
        self.search_results = self.entries.iter()
            .filter(|e| e.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
    }
    
    /// Draw Finder window
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Window background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Toolbar
        fb.draw_rect(x, y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_toolbar(fb, x, y, w);
        
        // Tab bar
        let tab_y = y + TOOLBAR_HEIGHT;
        fb.draw_rect(x, tab_y, w, TAB_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
        self.draw_tabs(fb, x, tab_y, w);
        
        // Sidebar
        let content_y = tab_y + TAB_BAR_HEIGHT;
        let content_h = h - TOOLBAR_HEIGHT - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT;
        
        if self.show_sidebar {
            fb.draw_rect(x, content_y, SIDEBAR_WIDTH, content_h, Theme::SIDEBAR_BG.to_u32());
            self.draw_sidebar(fb, x, content_y, content_h);
        }
        
        // Main content area
        let content_x = if self.show_sidebar { x + SIDEBAR_WIDTH } else { x };
        let content_w = if self.show_sidebar { w - SIDEBAR_WIDTH } else { w };
        
        let tab = &self.tabs[self.active_tab];
        
        match tab.view_mode {
            ViewMode::List => self.draw_list_view(fb, content_x, content_y, content_w, content_h),
            ViewMode::Icons => self.draw_icon_view(fb, content_x, content_y, content_w, content_h),
            ViewMode::Columns => self.draw_column_view(fb, content_x, content_y, content_w, content_h),
            ViewMode::Gallery => self.draw_gallery_view(fb, content_x, content_y, content_w, content_h),
        }
        
        // Status bar
        let status_y = y + h - STATUS_BAR_HEIGHT;
        fb.draw_rect(x, status_y, w, STATUS_BAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_status_bar(fb, x, status_y, w);
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let mut btn_x = x + 8;
        
        // Back button
        let can_back = self.tabs[self.active_tab].history_pos > 0;
        let color = if can_back { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "◀", color);
        btn_x += 32;
        
        // Forward button
        let can_forward = self.tabs[self.active_tab].history_pos < self.tabs[self.active_tab].history.len() - 1;
        let color = if can_forward { Theme::TEXT_PRIMARY.to_u32() } else { Theme::TEXT_DISABLED.to_u32() };
        fb.draw_rect(btn_x, y + 8, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(btn_x + 6, y + 12, "▶", color);
        btn_x += 36;
        
        // View buttons
        let views = [("⊞", ViewMode::Icons), ("≡", ViewMode::List), ("▤", ViewMode::Columns)];
        for (icon, mode) in views {
            let bg = if self.tabs[self.active_tab].view_mode == mode { 
                Theme::ACCENT_PRIMARY.to_u32() 
            } else { 
                Theme::SIDEBAR_BG.to_u32() 
            };
            fb.draw_rect(btn_x, y + 8, 24, 24, bg);
            fb.draw_string(btn_x + 4, y + 12, icon, Theme::TEXT_PRIMARY.to_u32());
            btn_x += 28;
        }
        
        // Search box
        let search_x = x + w - 180;
        fb.draw_rect(search_x, y + 8, 160, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(search_x + 8, y + 12, "🔍", Theme::TEXT_SECONDARY.to_u32());
        
        if self.searching && !self.search_query.is_empty() {
            fb.draw_string(search_x + 28, y + 12, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        } else {
            fb.draw_string(search_x + 28, y + 12, "Search", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let tab_width = 150.min(w / self.tabs.len().max(1));
        let mut tab_x = x + 8;
        
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let bg = if is_active { Theme::WINDOW_BG.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            
            fb.draw_rect(tab_x, y, tab_width, TAB_BAR_HEIGHT, bg);
            
            // Tab title
            let title = if tab.title.len() > 12 { format!("{}...", &tab.title[..9]) } else { tab.title.clone() };
            fb.draw_string(tab_x + 8, y + 6, &title, Theme::TEXT_PRIMARY.to_u32());
            
            // Close button
            if self.tabs.len() > 1 {
                fb.draw_string(tab_x + tab_width - 20, y + 6, "×", Theme::TEXT_SECONDARY.to_u32());
            }
            
            tab_x += tab_width + 4;
        }
        
        // New tab button
        fb.draw_string(tab_x + 4, y + 6, "+", Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_sidebar(&self, fb: &mut Framebuffer, x: usize, y: usize, h: usize) {
        let mut item_y = y + 4;
        
        for (i, item) in self.sidebar.iter().enumerate() {
            if item.is_header {
                // Section header
                fb.draw_string(x + 8, item_y, &item.name, Theme::TEXT_SECONDARY.to_u32());
                item_y += 20;
            } else {
                let bg = if item.selected { Theme::ACCENT_PRIMARY.to_u32() }
                         else if self.hovered_sidebar == Some(i) { Theme::LIST_ITEM_HOVER.to_u32() }
                         else { Theme::TRANSPARENT.to_u32() };
                
                fb.draw_rect(x, item_y, SIDEBAR_WIDTH, 20, bg);
                
                let icon = self.get_sidebar_icon(item.icon);
                let text_color = if item.selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
                
                fb.draw_string(x + 8, item_y + 2, icon, text_color);
                fb.draw_string(x + 28, item_y + 2, &item.name, text_color);
                
                // Eject button for external drives
                if item.can_eject {
                    fb.draw_string(x + SIDEBAR_WIDTH - 20, item_y + 2, "⏏", Theme::TEXT_SECONDARY.to_u32());
                }
                
                item_y += 22;
            }
            
            if item_y > y + h {
                break;
            }
        }
    }
    
    fn draw_list_view(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Column headers
        let header_y = y;
        fb.draw_rect(x, header_y, w, 24, Theme::TOOLBAR_BG.to_u32());
        
        let tab = &self.tabs[self.active_tab];
        
        // Name column
        let name_color = if tab.sort_column == SortColumn::Name { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
        fb.draw_string(x + 8, header_y + 4, "Name ▾", name_color);
        
        // Size column
        fb.draw_string(x + w - 200, header_y + 4, "Size", Theme::TEXT_SECONDARY.to_u32());
        
        // Date column
        fb.draw_string(x + w - 100, header_y + 4, "Date", Theme::TEXT_SECONDARY.to_u32());
        
        // Entries
        let scroll = self.tabs[self.active_tab].scroll_offset;
        let visible_rows = (h - 24) / LIST_ROW_HEIGHT;
        
        for (i, entry) in self.entries.iter().skip(scroll).take(visible_rows).enumerate() {
            let row_y = y + 24 + i * LIST_ROW_HEIGHT;
            let actual_idx = scroll + i;
            
            let bg = if entry.selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if self.hovered_entry == Some(actual_idx) { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(x, row_y, w, LIST_ROW_HEIGHT, bg);
            
            let text_color = if entry.selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let icon = self.get_file_icon(entry.icon, entry.is_dir);
            
            // Icon
            fb.draw_string(x + 8, row_y + 2, icon, text_color);
            
            // Name
            let name = if entry.name.len() > 30 { format!("{}...", &entry.name[..27]) } else { entry.name.clone() };
            fb.draw_string(x + 32, row_y + 2, &name, text_color);
            
            // Size
            fb.draw_string(x + w - 200, row_y + 2, &entry.format_size(), Theme::TEXT_SECONDARY.to_u32());
            
            // Date
            fb.draw_string(x + w - 100, row_y + 2, &entry.format_date(), Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_icon_view(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let cols = w / (ICON_VIEW_SIZE + 16);
        let scroll = self.tabs[self.active_tab].scroll_offset;
        let start_idx = scroll * cols;
        
        for (i, entry) in self.entries.iter().skip(start_idx).enumerate() {
            let col = i % cols;
            let row = i / cols;
            
            let icon_x = x + col * (ICON_VIEW_SIZE + 16) + 8;
            let icon_y = y + row * (ICON_VIEW_SIZE + 32) + 8;
            
            if icon_y + ICON_VIEW_SIZE > y + h {
                break;
            }
            
            let bg = if entry.selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
            fb.draw_rect(icon_x, icon_y, ICON_VIEW_SIZE, ICON_VIEW_SIZE, bg);
            
            // Draw icon
            let icon = self.get_file_icon(entry.icon, entry.is_dir);
            fb.draw_string(icon_x + ICON_VIEW_SIZE / 2 - 8, icon_y + ICON_VIEW_SIZE / 2 - 8, icon, Theme::TEXT_PRIMARY.to_u32());
            
            // Name
            let name = if entry.name.len() > 12 { format!("{}...", &entry.name[..9]) } else { entry.name.clone() };
            fb.draw_string(icon_x + (ICON_VIEW_SIZE - name.len() * 8) / 2, icon_y + ICON_VIEW_SIZE + 4, &name, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn draw_column_view(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Multiple columns for navigation
        let num_cols = (w / COLUMN_WIDTH).max(1);
        
        for col in 0..num_cols {
            let col_x = x + col * COLUMN_WIDTH;
            fb.draw_rect(col_x, y, COLUMN_WIDTH, h, Theme::WINDOW_BG.to_u32());
            fb.draw_rect(col_x + COLUMN_WIDTH - 1, y, 1, h, Theme::BORDER.to_u32());
            
            // Would show entries for each column path
            if col == 0 {
                let scroll = self.tabs[self.active_tab].scroll_offset;
                let visible_rows = h / LIST_ROW_HEIGHT;
                
                for (i, entry) in self.entries.iter().skip(scroll).take(visible_rows).enumerate() {
                    let row_y = y + i * LIST_ROW_HEIGHT;
                    
                    let bg = if entry.selected { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TRANSPARENT.to_u32() };
                    fb.draw_rect(col_x, row_y, COLUMN_WIDTH - 1, LIST_ROW_HEIGHT, bg);
                    
                    let icon = self.get_file_icon(entry.icon, entry.is_dir);
                    let text_color = if entry.selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
                    
                    fb.draw_string(col_x + 8, row_y + 2, icon, text_color);
                    fb.draw_string(col_x + 28, row_y + 2, &entry.name, text_color);
                    
                    if entry.is_dir {
                        fb.draw_string(col_x + COLUMN_WIDTH - 20, row_y + 2, "▶", Theme::TEXT_SECONDARY.to_u32());
                    }
                }
            }
        }
    }
    
    fn draw_gallery_view(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Large preview with thumbnails
        if let Some(&idx) = self.tabs[self.active_tab].selected_entries.first() {
            if idx < self.entries.len() {
                let entry = &self.entries[idx];
                
                // Large preview area
                let preview_w = w - 200;
                fb.draw_rect(x, y, preview_w, h, Theme::SIDEBAR_BG.to_u32());
                
                // Preview icon
                let icon = self.get_file_icon(entry.icon, entry.is_dir);
                fb.draw_string(x + preview_w / 2 - 40, y + h / 2 - 40, icon, Theme::TEXT_PRIMARY.to_u32());
                
                // File info
                fb.draw_string(x + 8, y + h - 60, &entry.name, Theme::TEXT_PRIMARY.to_u32());
                fb.draw_string(x + 8, y + h - 40, &entry.format_size(), Theme::TEXT_SECONDARY.to_u32());
            }
        }
        
        // Thumbnails on right
        let thumb_x = x + w - 180;
        fb.draw_rect(thumb_x, y, 180, h, Theme::SIDEBAR_BG.to_u32());
        
        for (i, entry) in self.entries.iter().take(10).enumerate() {
            let thumb_y = y + i * 60 + 8;
            if thumb_y + 50 > y + h { break; }
            
            let icon = self.get_file_icon(entry.icon, entry.is_dir);
            fb.draw_string(thumb_x + 8, thumb_y + 4, icon, Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(thumb_x + 40, thumb_y + 8, &entry.name, Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn draw_status_bar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let selected_count = self.tabs[self.active_tab].selected_entries.len();
        let total_count = self.entries.len();
        
        if selected_count > 0 {
            let text = format!("{} items selected", selected_count);
            fb.draw_string(x + 8, y + 4, &text, Theme::TEXT_SECONDARY.to_u32());
        } else {
            let text = format!("{} items", total_count);
            fb.draw_string(x + 8, y + 4, &text, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Disk space
        fb.draw_string(x + w - 120, y + 4, "256 GB free", Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn get_file_icon(&self, icon: FileIcon, is_dir: bool) -> &'static str {
        match icon {
            FileIcon::Folder | FileIcon::FolderOpen => "📁",
            FileIcon::File => "📄",
            FileIcon::Image => "🖼",
            FileIcon::Document => "📝",
            FileIcon::Audio => "🎵",
            FileIcon::Video => "🎬",
            FileIcon::Archive => "📦",
            FileIcon::Code => "💻",
            FileIcon::Executable => "⚙",
            FileIcon::Symlink => "🔗",
            FileIcon::Custom(_) => "📄",
        }
    }
    
    fn get_sidebar_icon(&self, icon: SidebarIcon) -> &'static str {
        match icon {
            SidebarIcon::AirDrop => "📡",
            SidebarIcon::Recents => "🕐",
            SidebarIcon::Applications => "📱",
            SidebarIcon::Desktop => "🖥",
            SidebarIcon::Documents => "📄",
            SidebarIcon::Downloads => "⬇",
            SidebarIcon::Pictures => "🖼",
            SidebarIcon::Music => "🎵",
            SidebarIcon::Videos => "🎬",
            SidebarIcon::Home => "🏠",
            SidebarIcon::Favorites => "⭐",
            SidebarIcon::iCloud => "☁",
            SidebarIcon::External => "💾",
            SidebarIcon::Network => "🌐",
            SidebarIcon::Tag(tag) => match tag {
                FileTag::Red => "🔴",
                FileTag::Orange => "🟠",
                FileTag::Yellow => "🟡",
                FileTag::Green => "🟢",
                FileTag::Blue => "🔵",
                FileTag::Purple => "🟣",
                FileTag::Gray => "⚪",
            },
            SidebarIcon::Folder => "📁",
        }
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> FinderAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        let h = self.rect.height;
        
        // Toolbar buttons
        if my >= y + 8 && my < y + 32 {
            let mut btn_x = x + 8;
            
            // Back
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_back();
                return FinderAction::None;
            }
            btn_x += 32;
            
            // Forward
            if mx >= btn_x && mx < btn_x + 28 {
                self.go_forward();
                return FinderAction::None;
            }
            btn_x += 36;
            
            // View buttons
            let views = [ViewMode::Icons, ViewMode::List, ViewMode::Columns];
            for mode in views {
                if mx >= btn_x && mx < btn_x + 24 {
                    self.set_view_mode(mode);
                    return FinderAction::None;
                }
                btn_x += 28;
            }
            
            // Search
            let search_x = x + w - 180;
            if mx >= search_x && mx < search_x + 160 {
                self.start_search();
                return FinderAction::None;
            }
        }
        
        // Tabs
        let tab_y = y + TOOLBAR_HEIGHT as i32;
        if my >= tab_y && my < tab_y + TAB_BAR_HEIGHT as i32 {
            let tab_width = 150.min(w as usize / self.tabs.len().max(1)) as i32;
            let mut tab_x = x + 8;
            
            for i in 0..self.tabs.len() {
                if mx >= tab_x && mx < tab_x + tab_width {
                    // Close button
                    if mx > tab_x + tab_width - 20 && self.tabs.len() > 1 {
                        self.close_tab(i);
                    } else {
                        self.select_tab(i);
                    }
                    return FinderAction::None;
                }
                tab_x += tab_width + 4;
            }
            
            // New tab
            if mx >= tab_x {
                self.new_tab("/home");
            }
        }
        
        // Sidebar
        let content_y = tab_y + TAB_BAR_HEIGHT as i32;
        if self.show_sidebar && mx >= x && mx < x + SIDEBAR_WIDTH as i32 
            && my >= content_y {
            let mut item_y = content_y + 4;
            
            for item in &self.sidebar {
                if item.is_header {
                    item_y += 20;
                } else {
                    if my >= item_y && my < item_y + 20 {
                        if !item.path.is_empty() && !item.is_header {
                            let path = item.path.clone();
                            self.navigate(&path);
                        }
                        return FinderAction::None;
                    }
                    item_y += 22;
                }
            }
        }
        
        // Content area
        let content_x = if self.show_sidebar { x + SIDEBAR_WIDTH as i32 } else { x };
        let content_w = if self.show_sidebar { w - SIDEBAR_WIDTH as i32 } else { w };
        
        if mx >= content_x && my >= content_y + 24 {
            let tab = &self.tabs[self.active_tab];
            
            match tab.view_mode {
                ViewMode::List => {
                    let scroll = tab.scroll_offset;
                    let row_idx = ((my - content_y - 24) / LIST_ROW_HEIGHT as i32) as usize;
                    let actual_idx = scroll + row_idx;
                    
                    if actual_idx < self.entries.len() {
                        return self.open_entry(actual_idx);
                    }
                }
                ViewMode::Icons => {
                    let cols = (content_w as usize) / (ICON_VIEW_SIZE + 16);
                    let scroll = tab.scroll_offset;
                    let col = ((mx - content_x) / (ICON_VIEW_SIZE + 16) as i32) as usize;
                    let row = ((my - content_y - 8) / (ICON_VIEW_SIZE + 32) as i32) as usize;
                    let actual_idx = scroll * cols + row * cols + col;
                    
                    if actual_idx < self.entries.len() {
                        return self.open_entry(actual_idx);
                    }
                }
                _ => {}
            }
        }
        
        FinderAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> FinderAction {
        if self.searching {
            if c == '\x1b' { // Escape
                self.searching = false;
                self.search_query.clear();
            } else if c == '\n' { // Enter
                self.searching = false;
            } else if c == '\x08' { // Backspace
                self.search_query.pop();
                self.update_search();
            } else if !c.is_control() {
                self.search_query.push(c);
                self.update_search();
            }
            return FinderAction::None;
        }
        
        match c {
            '\x1b' => return FinderAction::None,
            '\n' => {
                if let Some(&idx) = self.tabs[self.active_tab].selected_entries.first() {
                    return self.open_entry(idx);
                }
            }
            _ => {}
        }
        
        FinderAction::None
    }
    
    /// Handle double click
    pub fn on_double_click(&mut self, mx: i32, my: i32) -> FinderAction {
        // Double click opens entry
        self.on_click(mx, my)
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Finder actions
#[derive(Clone, Debug)]
pub enum FinderAction {
    None,
    OpenFile(String),
    OpenFolder(String),
    Copy(Vec<String>),
    Move(Vec<String>, String),
    Delete(Vec<String>),
    Rename(String, String),
    NewFolder(String),
}

// ============================================================================
// GLOBAL FINDER
// ============================================================================

lazy_static::lazy_static! {
    static ref FINDER: Mutex<FinderWindow> = Mutex::new(FinderWindow::new(Rect {
        x: 100,
        y: 100,
        width: 900,
        height: 600,
    }));
}

/// Initialize Finder
pub fn init() {
    crate::serial_println!("[GUI] Finder initialized");
}

/// Get Finder
pub fn get_finder() -> &'static Mutex<FinderWindow> {
    &FINDER
}
