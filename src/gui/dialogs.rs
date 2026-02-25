//! # File Dialogs
//!
//! Open/Save file dialogs with navigation and filtering
//! macOS-style file browser with sidebar and preview

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// DIALOG CONSTANTS
// ============================================================================

/// Dialog width
pub const DIALOG_WIDTH: usize = 700;

/// Dialog height
pub const DIALOG_HEIGHT: usize = 500;

/// Sidebar width
pub const SIDEBAR_WIDTH: usize = 180;

/// Row height
pub const ROW_HEIGHT: usize = 24;

/// Icon size
pub const ICON_SIZE: usize = 16;

// ============================================================================
// FILE ENTRY
// ============================================================================

/// A file or folder entry
#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Entry name
    pub name: String,
    /// Full path
    pub path: String,
    /// Is directory
    pub is_dir: bool,
    /// File size (0 for directories)
    pub size: u64,
    /// Modified timestamp
    pub modified: u64,
    /// File type icon
    pub icon: FileIcon,
    /// Is selected
    pub selected: bool,
    /// Extension
    pub extension: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileIcon {
    Folder,
    File,
    Image,
    Document,
    Audio,
    Video,
    Archive,
    Code,
    Executable,
    Custom(u16),
}

impl FileEntry {
    pub fn folder(name: &str, path: &str) -> Self {
        FileEntry {
            name: String::from(name),
            path: String::from(path),
            is_dir: true,
            size: 0,
            modified: 0,
            icon: FileIcon::Folder,
            selected: false,
            extension: String::new(),
        }
    }
    
    pub fn file(name: &str, path: &str, size: u64) -> Self {
        let extension = name.split('.').last().unwrap_or("").to_lowercase();
        let icon = Self::get_icon_for_extension(&extension);
        
        FileEntry {
            name: String::from(name),
            path: String::from(path),
            is_dir: false,
            size,
            modified: 0,
            icon,
            selected: false,
            extension,
        }
    }
    
    fn get_icon_for_extension(ext: &str) -> FileIcon {
        match ext {
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => FileIcon::Image,
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" => FileIcon::Audio,
            "mp4" | "avi" | "mkv" | "mov" | "webm" => FileIcon::Video,
            "zip" | "rar" | "7z" | "tar" | "gz" => FileIcon::Archive,
            "rs" | "c" | "cpp" | "h" | "py" | "js" | "ts" | "go" | "java" => FileIcon::Code,
            "txt" | "md" | "rtf" | "pdf" | "doc" | "docx" => FileIcon::Document,
            "exe" | "bin" | "sh" | "bat" => FileIcon::Executable,
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
}

// ============================================================================
// SIDEBAR ITEM
// ============================================================================

/// Sidebar shortcut item
#[derive(Clone, Debug)]
pub struct SidebarItem {
    /// Item name
    pub name: String,
    /// Path
    pub path: String,
    /// Icon
    pub icon: SidebarIcon,
    /// Is selected
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarIcon {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Applications,
    Favorites,
    External,
    Cloud,
}

impl SidebarItem {
    pub fn new(name: &str, path: &str, icon: SidebarIcon) -> Self {
        SidebarItem {
            name: String::from(name),
            path: String::from(path),
            icon,
            selected: false,
        }
    }
}

// ============================================================================
// FILE DIALOG
// ============================================================================

/// File dialog type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogType {
    OpenFile,
    SaveFile,
    OpenFolder,
    OpenMultiple,
}

/// File dialog
pub struct FileDialog {
    /// Dialog type
    pub dialog_type: DialogType,
    /// Is visible
    pub visible: bool,
    /// Dialog title
    pub title: String,
    /// Current directory
    pub current_path: String,
    /// Directory entries
    pub entries: Vec<FileEntry>,
    /// Sidebar items
    pub sidebar: Vec<SidebarItem>,
    /// Selected entries
    pub selected_entries: Vec<usize>,
    /// Filename input (for save)
    pub filename_input: String,
    /// File filter
    pub filter: String,
    /// Allowed extensions
    pub allowed_extensions: Vec<String>,
    /// Scroll offset
    pub scroll_offset: usize,
    /// Dialog position
    pub position: (usize, usize),
    /// Screen dimensions
    pub screen_width: usize,
    pub screen_height: usize,
    /// Hovered entry
    pub hovered_entry: Option<usize>,
    /// Hovered sidebar
    pub hovered_sidebar: Option<usize>,
    /// Active button
    pub active_button: Option<DialogButton>,
    /// Show hidden files
    pub show_hidden: bool,
    /// Sort column
    pub sort_column: SortColumn,
    /// Sort ascending
    pub sort_ascending: bool,
    /// Result
    pub result: Option<DialogResult>,
    /// Preview visible
    pub preview_visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    Open,
    Save,
    Cancel,
    NewFolder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
    Kind,
}

#[derive(Clone, Debug)]
pub enum DialogResult {
    Open(String),
    Save(String),
    OpenMultiple(Vec<String>),
    Cancelled,
}

impl FileDialog {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut dialog = FileDialog {
            dialog_type: DialogType::OpenFile,
            visible: false,
            title: String::from("Open"),
            current_path: String::from("/home"),
            entries: Vec::new(),
            sidebar: Vec::new(),
            selected_entries: Vec::new(),
            filename_input: String::new(),
            filter: String::new(),
            allowed_extensions: Vec::new(),
            scroll_offset: 0,
            position: (0, 0),
            screen_width,
            screen_height,
            hovered_entry: None,
            hovered_sidebar: None,
            active_button: None,
            show_hidden: false,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            result: None,
            preview_visible: true,
        };
        
        dialog.init_sidebar();
        dialog.center_dialog();
        dialog
    }
    
    fn init_sidebar(&mut self) {
        self.sidebar = vec![
            SidebarItem::new("Home", "/home", SidebarIcon::Home),
            SidebarItem::new("Desktop", "/home/desktop", SidebarIcon::Desktop),
            SidebarItem::new("Documents", "/home/documents", SidebarIcon::Documents),
            SidebarItem::new("Downloads", "/home/downloads", SidebarIcon::Downloads),
            SidebarItem::new("Pictures", "/home/pictures", SidebarIcon::Pictures),
            SidebarItem::new("Music", "/home/music", SidebarIcon::Music),
            SidebarItem::new("Videos", "/home/videos", SidebarIcon::Videos),
            SidebarItem::new("Applications", "/applications", SidebarIcon::Applications),
        ];
    }
    
    fn center_dialog(&mut self) {
        self.position = (
            (self.screen_width - DIALOG_WIDTH) / 2,
            (self.screen_height - DIALOG_HEIGHT) / 2,
        );
    }
    
    /// Show open file dialog
    pub fn show_open(&mut self, title: &str, path: &str, extensions: &[&str]) {
        self.dialog_type = DialogType::OpenFile;
        self.title = String::from(title);
        self.current_path = String::from(path);
        self.allowed_extensions = extensions.iter().map(|s| String::from(*s)).collect();
        self.visible = true;
        self.result = None;
        self.selected_entries.clear();
        self.filename_input.clear();
        self.center_dialog();
        self.load_directory();
    }
    
    /// Show save file dialog
    pub fn show_save(&mut self, title: &str, path: &str, default_name: &str, extensions: &[&str]) {
        self.dialog_type = DialogType::SaveFile;
        self.title = String::from(title);
        self.current_path = String::from(path);
        self.allowed_extensions = extensions.iter().map(|s| String::from(*s)).collect();
        self.filename_input = String::from(default_name);
        self.visible = true;
        self.result = None;
        self.selected_entries.clear();
        self.center_dialog();
        self.load_directory();
    }
    
    /// Show open folder dialog
    pub fn show_open_folder(&mut self, title: &str, path: &str) {
        self.dialog_type = DialogType::OpenFolder;
        self.title = String::from(title);
        self.current_path = String::from(path);
        self.allowed_extensions.clear();
        self.visible = true;
        self.result = None;
        self.selected_entries.clear();
        self.filename_input.clear();
        self.center_dialog();
        self.load_directory();
    }
    
    /// Hide dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }
    
    /// Load directory contents
    pub fn load_directory(&mut self) {
        self.entries.clear();
        
        // Add parent directory
        if self.current_path != "/" {
            self.entries.push(FileEntry::folder("..", &format!("{}/..", self.current_path)));
        }
        
        // Add test entries (would load from filesystem)
        self.entries.push(FileEntry::folder("Documents", "/home/documents"));
        self.entries.push(FileEntry::folder("Pictures", "/home/pictures"));
        self.entries.push(FileEntry::folder("Music", "/home/music"));
        self.entries.push(FileEntry::file("readme.txt", "/home/readme.txt", 1024));
        self.entries.push(FileEntry::file("image.png", "/home/image.png", 2048000));
        self.entries.push(FileEntry::file("document.pdf", "/home/document.pdf", 512000));
        self.entries.push(FileEntry::file("music.mp3", "/home/music.mp3", 4096000));
        self.entries.push(FileEntry::file("video.mp4", "/home/video.mp4", 102400000));
        self.entries.push(FileEntry::file("source.rs", "/home/source.rs", 8192));
        self.entries.push(FileEntry::file("archive.zip", "/home/archive.zip", 10240000));
        
        // Apply filter
        self.apply_filter();
        
        // Sort
        self.sort_entries();
    }
    
    fn apply_filter(&mut self) {
        if self.filter.is_empty() && self.allowed_extensions.is_empty() {
            return;
        }
        
        self.entries.retain(|e| {
            if e.is_dir {
                return true;
            }
            
            if !self.filter.is_empty() {
                if !e.name.to_lowercase().contains(&self.filter.to_lowercase()) {
                    return false;
                }
            }
            
            if !self.allowed_extensions.is_empty() {
                return self.allowed_extensions.iter().any(|ext| {
                    e.extension.to_lowercase() == ext.to_lowercase().trim_start_matches('.')
                });
            }
            
            true
        });
    }
    
    fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| {
            // Folders first
            if a.is_dir && !b.is_dir {
                return core::cmp::Ordering::Less;
            }
            if !a.is_dir && b.is_dir {
                return core::cmp::Ordering::Greater;
            }
            
            let cmp = match self.sort_column {
                SortColumn::Name => a.name.cmp(&b.name),
                SortColumn::Size => a.size.cmp(&b.size),
                SortColumn::Modified => a.modified.cmp(&b.modified),
                SortColumn::Kind => a.icon.cmp(&b.icon),
            };
            
            if self.sort_ascending { cmp } else { cmp.reverse() }
        });
    }
    
    /// Navigate to directory
    pub fn navigate_to(&mut self, path: &str) {
        self.current_path = String::from(path);
        self.selected_entries.clear();
        self.scroll_offset = 0;
        self.load_directory();
    }
    
    /// Navigate to parent
    pub fn navigate_parent(&mut self) {
        if self.current_path == "/" {
            return;
        }
        
        let parent = self.current_path.rfind('/')
            .map(|i| if i == 0 { "/" } else { &self.current_path[..i] })
            .unwrap_or("/");
        let parent = parent.to_string();
        
        self.navigate_to(&parent);
    }
    
    /// Select entry
    pub fn select_entry(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        
        // Clear previous selection
        for entry in &mut self.entries {
            entry.selected = false;
        }
        
        self.entries[index].selected = true;
        self.selected_entries = vec![index];
        
        // Update filename input for files
        if !self.entries[index].is_dir {
            self.filename_input = self.entries[index].name.clone();
        }
    }
    
    /// Double-click entry
    pub fn double_click_entry(&mut self, index: usize) -> DialogResult {
        if index >= self.entries.len() {
            return DialogResult::Cancelled;
        }
        
        let (is_dir, path) = {
            let entry = &self.entries[index];
            (entry.is_dir, entry.path.clone())
        };
        
        if is_dir {
            self.navigate_to(&path);
            return DialogResult::Cancelled;
        }
        
        // Open file
        self.result = Some(DialogResult::Open(path.clone()));
        self.visible = false;
        DialogResult::Open(path)
    }
    
    /// Confirm selection
    pub fn confirm(&mut self) -> DialogResult {
        match self.dialog_type {
            DialogType::OpenFile | DialogType::OpenFolder => {
                if let Some(&idx) = self.selected_entries.first() {
                    if idx < self.entries.len() {
                        let path = self.entries[idx].path.clone();
                        self.result = Some(DialogResult::Open(path.clone()));
                        self.visible = false;
                        return DialogResult::Open(path);
                    }
                }
            }
            DialogType::SaveFile => {
                if !self.filename_input.is_empty() {
                    let path = format!("{}/{}", self.current_path, self.filename_input);
                    self.result = Some(DialogResult::Save(path.clone()));
                    self.visible = false;
                    return DialogResult::Save(path);
                }
            }
            DialogType::OpenMultiple => {
                let paths: Vec<String> = self.selected_entries.iter()
                    .filter(|&&i| i < self.entries.len())
                    .map(|&i| self.entries[i].path.clone())
                    .collect();
                
                if !paths.is_empty() {
                    self.result = Some(DialogResult::OpenMultiple(paths.clone()));
                    self.visible = false;
                    return DialogResult::OpenMultiple(paths);
                }
            }
        }
        
        DialogResult::Cancelled
    }
    
    /// Cancel dialog
    pub fn cancel(&mut self) {
        self.result = Some(DialogResult::Cancelled);
        self.visible = false;
    }
    
    /// Draw dialog
    pub fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        
        let (x, y) = self.position;
        
        // Dim background
        for py in 0..self.screen_height {
            for px in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(py * fb.pixels_per_scan_line + px) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, 0.3);
                unsafe { *ptr = dimmed; }
            }
        }
        
        // Dialog background
        fb.draw_rect(x, y, DIALOG_WIDTH, DIALOG_HEIGHT, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, DIALOG_WIDTH, DIALOG_HEIGHT, Theme::BORDER.to_u32());
        
        // Title bar
        fb.draw_rect(x, y, DIALOG_WIDTH, 40, Theme::TITLEBAR_BG.to_u32());
        fb.draw_string(x + 16, y + 12, &self.title, Theme::TEXT_PRIMARY.to_u32());
        
        // Close button
        fb.draw_rect(x + DIALOG_WIDTH - 36, y + 8, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + DIALOG_WIDTH - 30, y + 12, "×", 0xFFFFFFFF);
        
        // Toolbar
        let toolbar_y = y + 44;
        fb.draw_rect(x, toolbar_y, DIALOG_WIDTH, 32, Theme::TOOLBAR_BG.to_u32());
        
        // Back/Forward buttons
        fb.draw_rect(x + 8, toolbar_y + 4, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 14, toolbar_y + 8, "◀", Theme::TEXT_PRIMARY.to_u32());
        
        fb.draw_rect(x + 40, toolbar_y + 4, 28, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + 46, toolbar_y + 8, "▶", Theme::TEXT_PRIMARY.to_u32());
        
        // Path bar
        let path_bar_x = x + 76;
        let path_bar_width = DIALOG_WIDTH - 180;
        fb.draw_rect(path_bar_x, toolbar_y + 4, path_bar_width, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(path_bar_x + 8, toolbar_y + 8, &self.current_path, Theme::TEXT_SECONDARY.to_u32());
        
        // Search box
        let search_x = x + DIALOG_WIDTH - 96;
        fb.draw_rect(search_x, toolbar_y + 4, 88, 24, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(search_x + 8, toolbar_y + 8, "🔍 Search", Theme::TEXT_SECONDARY.to_u32());
        
        // Sidebar
        let sidebar_x = x;
        let sidebar_y = toolbar_y + 32;
        fb.draw_rect(sidebar_x, sidebar_y, SIDEBAR_WIDTH, DIALOG_HEIGHT - 120, Theme::SIDEBAR_BG.to_u32());
        
        // Sidebar items
        for (i, item) in self.sidebar.iter().enumerate() {
            let item_y = sidebar_y + i * 28;
            let bg = if item.selected { Theme::ACCENT_PRIMARY.to_u32() } 
                     else if self.hovered_sidebar == Some(i) { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(sidebar_x, item_y, SIDEBAR_WIDTH, 28, bg);
            
            let icon = self.get_sidebar_icon(item.icon);
            let text_color = if item.selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_string(sidebar_x + 12, item_y + 6, icon, text_color);
            fb.draw_string(sidebar_x + 36, item_y + 6, &item.name, text_color);
        }
        
        // File list area
        let list_x = x + SIDEBAR_WIDTH;
        let list_y = sidebar_y;
        let list_width = DIALOG_WIDTH - SIDEBAR_WIDTH;
        let list_height = DIALOG_HEIGHT - 120;
        
        fb.draw_rect(list_x, list_y, list_width, list_height, Theme::WINDOW_BG.to_u32());
        
        // Column headers
        let header_y = list_y;
        fb.draw_rect(list_x, header_y, list_width, 24, Theme::TOOLBAR_BG.to_u32());
        
        fb.draw_string(list_x + 8, header_y + 4, "Name", Theme::TEXT_SECONDARY.to_u32());
        fb.draw_string(list_x + list_width - 120, header_y + 4, "Size", Theme::TEXT_SECONDARY.to_u32());
        
        // File entries
        let visible_rows = (list_height - 24) / ROW_HEIGHT;
        let start_row = self.scroll_offset;
        
        for (i, entry) in self.entries.iter().skip(start_row).take(visible_rows).enumerate() {
            let row_y = list_y + 24 + i * ROW_HEIGHT;
            let bg = if entry.selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if self.hovered_entry == Some(start_row + i) { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::TRANSPARENT.to_u32() };
            
            fb.draw_rect(list_x, row_y, list_width, ROW_HEIGHT, bg);
            
            let text_color = if entry.selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let icon = self.get_file_icon(entry.icon);
            
            fb.draw_string(list_x + 8, row_y + 4, icon, text_color);
            
            let name = if entry.name.len() > 30 { format!("{}...", &entry.name[..27]) } else { entry.name.clone() };
            fb.draw_string(list_x + 32, row_y + 4, &name, text_color);
            
            if !entry.is_dir {
                fb.draw_string(list_x + list_width - 80, row_y + 4, &entry.format_size(), Theme::TEXT_SECONDARY.to_u32());
            }
        }
        
        // Footer
        let footer_y = y + DIALOG_HEIGHT - 44;
        fb.draw_rect(x, footer_y, DIALOG_WIDTH, 44, Theme::TOOLBAR_BG.to_u32());
        
        // Filename input (for save dialog)
        if self.dialog_type == DialogType::SaveFile {
            fb.draw_string(x + 16, footer_y + 8, "Save as:", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_rect(x + 88, footer_y + 4, DIALOG_WIDTH - 280, 28, Theme::SIDEBAR_BG.to_u32());
            fb.draw_string(x + 96, footer_y + 8, &self.filename_input, Theme::TEXT_PRIMARY.to_u32());
        }
        
        // Buttons
        let btn_y = footer_y + 8;
        
        // Cancel button
        fb.draw_rect(x + DIALOG_WIDTH - 180, btn_y, 72, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(x + DIALOG_WIDTH - 160, btn_y + 6, "Cancel", Theme::TEXT_PRIMARY.to_u32());
        
        // Open/Save button
        let btn_text = match self.dialog_type {
            DialogType::OpenFile | DialogType::OpenFolder | DialogType::OpenMultiple => "Open",
            DialogType::SaveFile => "Save",
        };
        
        let btn_enabled = !self.selected_entries.is_empty() || (!self.filename_input.is_empty() && self.dialog_type == DialogType::SaveFile);
        let btn_color = if btn_enabled { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
        
        fb.draw_rect(x + DIALOG_WIDTH - 96, btn_y, 80, 28, btn_color);
        fb.draw_string(x + DIALOG_WIDTH - 80, btn_y + 6, btn_text, if btn_enabled { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_DISABLED.to_u32() });
    }
    
    fn get_file_icon(&self, icon: FileIcon) -> &'static str {
        match icon {
            FileIcon::Folder => "📁",
            FileIcon::File => "📄",
            FileIcon::Image => "🖼",
            FileIcon::Document => "📝",
            FileIcon::Audio => "🎵",
            FileIcon::Video => "🎬",
            FileIcon::Archive => "📦",
            FileIcon::Code => "💻",
            FileIcon::Executable => "⚙",
            FileIcon::Custom(_) => "📄",
        }
    }
    
    fn get_sidebar_icon(&self, icon: SidebarIcon) -> &'static str {
        match icon {
            SidebarIcon::Home => "🏠",
            SidebarIcon::Desktop => "🖥",
            SidebarIcon::Documents => "📄",
            SidebarIcon::Downloads => "⬇",
            SidebarIcon::Pictures => "🖼",
            SidebarIcon::Music => "🎵",
            SidebarIcon::Videos => "🎬",
            SidebarIcon::Applications => "📱",
            SidebarIcon::Favorites => "⭐",
            SidebarIcon::External => "💾",
            SidebarIcon::Cloud => "☁",
        }
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
    pub fn on_click(&mut self, mx: i32, my: i32) -> DialogResult {
        let (x, y) = self.position;
        
        // Close button
        if mx >= (x + DIALOG_WIDTH - 36) as i32 && mx < (x + DIALOG_WIDTH - 12) as i32
            && my >= (y + 8) as i32 && my < (y + 32) as i32 {
            self.cancel();
            return DialogResult::Cancelled;
        }
        
        // Sidebar
        let sidebar_y = y + 76;
        if mx >= x as i32 && mx < (x + SIDEBAR_WIDTH) as i32 
            && my >= sidebar_y as i32 && my < (sidebar_y + DIALOG_HEIGHT - 120) as i32 {
            
            let idx = ((my - sidebar_y as i32) / 28) as usize;
            if idx < self.sidebar.len() {
                self.navigate_to(&self.sidebar[idx].path.clone());
            }
        }
        
        // File list
        let list_x = x + SIDEBAR_WIDTH;
        let list_y = sidebar_y;
        
        if mx >= list_x as i32 && mx < (list_x + DIALOG_WIDTH - SIDEBAR_WIDTH) as i32
            && my >= (list_y + 24) as i32 {
            
            let row_idx = ((my - list_y as i32 - 24) / ROW_HEIGHT as i32) as usize;
            let actual_idx = self.scroll_offset + row_idx;
            
            if actual_idx < self.entries.len() {
                self.select_entry(actual_idx);
            }
        }
        
        // Cancel button
        if mx >= (x + DIALOG_WIDTH - 180) as i32 && mx < (x + DIALOG_WIDTH - 108) as i32
            && my >= (y + DIALOG_HEIGHT - 36) as i32 {
            self.cancel();
            return DialogResult::Cancelled;
        }
        
        // Open/Save button
        if mx >= (x + DIALOG_WIDTH - 96) as i32 && mx < (x + DIALOG_WIDTH - 16) as i32
            && my >= (y + DIALOG_HEIGHT - 36) as i32 {
            return self.confirm();
        }
        
        DialogResult::Cancelled
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> DialogResult {
        if c == '\x1b' { // Escape
            self.cancel();
            return DialogResult::Cancelled;
        }
        
        if c == '\n' || c == '\r' { // Enter
            return self.confirm();
        }
        
        // Filename input for save dialog
        if self.dialog_type == DialogType::SaveFile {
            if c == '\x08' { // Backspace
                self.filename_input.pop();
            } else if !c.is_control() {
                self.filename_input.push(c);
            }
        }
        
        DialogResult::Cancelled
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
        self.center_dialog();
    }
}

// ============================================================================
// GLOBAL FILE DIALOG
// ============================================================================

lazy_static::lazy_static! {
    static ref FILE_DIALOG: Mutex<FileDialog> = Mutex::new(FileDialog::new(1920, 1080));
}

/// Initialize file dialog
pub fn init(width: usize, height: usize) {
    let mut dialog = FILE_DIALOG.lock();
    dialog.resize(width, height);
    crate::serial_println!("[GUI] File dialogs initialized");
}

/// Get file dialog
pub fn get_dialog() -> &'static Mutex<FileDialog> {
    &FILE_DIALOG
}
