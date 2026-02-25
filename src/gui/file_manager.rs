//! # echOS File Manager
//!
//! Graphical file browser for the desktop environment.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Rect, Widget};
use crate::gui::widgets::list::{ListView, ListItem};
use crate::gui::widgets::button::Button;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// File entry type
#[derive(Clone, Debug)]
pub enum FileEntryType {
    File,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
}

/// File entry information
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub entry_type: FileEntryType,
    pub size: u64,
    pub modified: u64,
    pub readonly: bool,
    pub hidden: bool,
}

impl FileEntry {
    pub fn file(name: &str, path: &str, size: u64) -> Self {
        Self {
            name: String::from(name),
            path: String::from(path),
            entry_type: FileEntryType::File,
            size,
            modified: 0,
            readonly: false,
            hidden: false,
        }
    }

    pub fn directory(name: &str, path: &str) -> Self {
        Self {
            name: String::from(name),
            path: String::from(path),
            entry_type: FileEntryType::Directory,
            size: 0,
            modified: 0,
            readonly: false,
            hidden: false,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.entry_type {
            FileEntryType::Directory => "[D]",
            FileEntryType::File => {
                let ext = self.name.rsplit('.').next().unwrap_or("");
                match ext {
                    "txt" | "md" => "[T]",
                    "rs" | "c" | "h" | "py" | "js" => "[C]",
                    "png" | "jpg" | "gif" | "bmp" => "[I]",
                    "mp3" | "wav" | "ogg" => "[A]",
                    "mp4" | "avi" | "mkv" => "[V]",
                    "zip" | "tar" | "gz" => "[Z]",
                    "exe" | "bin" => "[X]",
                    _ => "[F]",
                }
            }
            FileEntryType::Symlink => "[L]",
            FileEntryType::BlockDevice => "[B]",
            FileEntryType::CharDevice => "[C]",
        }
    }

    fn format_size(&self) -> String {
        if matches!(self.entry_type, FileEntryType::Directory) {
            return String::from("<DIR>");
        }
        
        if self.size < 1024 {
            alloc::format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            alloc::format!("{} KB", self.size / 1024)
        } else if self.size < 1024 * 1024 * 1024 {
            alloc::format!("{} MB", self.size / (1024 * 1024))
        } else {
            alloc::format!("{} GB", self.size / (1024 * 1024 * 1024))
        }
    }
}

/// File Manager widget
pub struct FileManager {
    rect: Rect,
    current_path: String,
    entries: Vec<FileEntry>,
    selected_index: Option<usize>,
    scroll_offset: usize,
    show_hidden: bool,
    view_mode: ViewMode,
    on_open: Option<fn(&FileEntry)>,
    on_select: Option<fn(&FileEntry)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Icons,
    Details,
}

impl FileManager {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            current_path: String::from("/"),
            entries: Vec::new(),
            selected_index: None,
            scroll_offset: 0,
            show_hidden: false,
            view_mode: ViewMode::List,
            on_open: None,
            on_select: None,
        }
    }

    pub fn set_path(&mut self, path: &str) {
        self.current_path = String::from(path);
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    pub fn set_entries(&mut self, entries: Vec<FileEntry>) {
        self.entries = entries;
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    pub fn add_entry(&mut self, entry: FileEntry) {
        self.entries.push(entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index.and_then(|i| self.entries.get(i))
    }

    pub fn current_path(&self) -> &str {
        &self.current_path
    }

    pub fn navigate_up(&mut self) {
        if self.current_path != "/" {
            let last_slash = self.current_path.rfind('/');
            if let Some(pos) = last_slash {
                if pos == 0 {
                    self.current_path = String::from("/");
                } else {
                    self.current_path.truncate(pos);
                }
            }
        }
    }

    fn item_height(&self) -> usize {
        match self.view_mode {
            ViewMode::List => 22,
            ViewMode::Icons => 80,
            ViewMode::Details => 24,
        }
    }

    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 60) / self.item_height()
    }

    fn entry_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 55;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / self.item_height());
        if index < self.entries.len() {
            Some(index)
        } else {
            None
        }
    }

    fn select(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected_index = Some(index);
            
            // Scroll to visible
            let visible = self.visible_items();
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + visible {
                self.scroll_offset = index - visible + 1;
            }

            if let Some(handler) = self.on_select {
                if let Some(entry) = self.entries.get(index) {
                    handler(entry);
                }
            }
        }
    }

    fn open(&mut self, index: usize) {
        if let Some(handler) = self.on_open {
            if let Some(entry) = self.entries.get(index) {
                handler(entry);
            }
        }
    }
}

impl Widget for FileManager {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        // Border
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        // Toolbar
        fb.draw_rect(x, y, w, 30, Theme::TITLEBAR_BG.to_u32());
        
        // Back button
        fb.draw_rect(x + 5, y + 5, 24, 20, Theme::BUTTON_BG.to_u32());
        fb.draw_string(x + 10, y + 7, "<", Theme::TEXT_PRIMARY.to_u32());
        
        // Up button
        fb.draw_rect(x + 35, y + 5, 24, 20, Theme::BUTTON_BG.to_u32());
        fb.draw_string(x + 42, y + 7, "^", Theme::TEXT_PRIMARY.to_u32());

        // Path bar
        fb.draw_rect(x + 70, y + 5, w - 80, 20, Theme::BUTTON_BG.to_u32());
        fb.draw_string(x + 75, y + 7, &self.current_path, Theme::TEXT_SECONDARY.to_u32());

        // Column headers (for details view)
        if self.view_mode == ViewMode::Details {
            let header_y = y + 32;
            fb.draw_rect(x, header_y, w, 20, Theme::TITLEBAR_BG.to_u32());
            fb.draw_string(x + 10, header_y + 2, "Name", Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(x + w - 100, header_y + 2, "Size", Theme::TEXT_SECONDARY.to_u32());
        }

        // File list
        let item_height = self.item_height();
        let visible = self.visible_items();
        let item_y_start = y + 55;

        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.entries.len() {
                break;
            }

            let entry = &self.entries[item_index];
            let item_y = item_y_start + i * item_height;

            // Skip hidden files if not showing
            if entry.hidden && !self.show_hidden {
                continue;
            }

            // Selection background
            if self.selected_index == Some(item_index) {
                fb.draw_rect(x + 1, item_y, w - 2, item_height, Theme::ACCENT_PRIMARY.to_u32());
            }

            // Icon
            let icon = entry.icon();
            fb.draw_string(x + 5, item_y + 3, icon, Theme::TEXT_ACCENT.to_u32());

            // Name
            let text_color = if self.selected_index == Some(item_index) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(x + 30, item_y + 3, &entry.name, text_color);

            // Size (for details view)
            if self.view_mode == ViewMode::Details {
                let size_str = entry.format_size();
                fb.draw_string(x + w - 80, item_y + 3, &size_str, Theme::TEXT_SECONDARY.to_u32());
            }
        }

        // Status bar
        let status_y = y + h - 25;
        fb.draw_rect(x, status_y, w, 25, Theme::TITLEBAR_BG.to_u32());
        
        let count = self.entries.len();
        let status = alloc::format!("{} items", count);
        fb.draw_string(x + 10, status_y + 5, &status, Theme::TEXT_SECONDARY.to_u32());

        // Scroll bar
        if self.entries.len() > visible {
            let scroll_bar_height = ((h - 85) * visible / self.entries.len()).max(20);
            let scroll_bar_y = y + 55 + ((h - 85) * self.scroll_offset / self.entries.len());
            fb.draw_rect(x + w - 10, scroll_bar_y, 8, scroll_bar_height, Theme::BUTTON_BG.to_u32());
        }
    }

    fn on_click(&mut self, click_x: i32, click_y: i32) -> bool {
        if !self.rect.contains(click_x, click_y) {
            return false;
        }

        let x = self.rect.x;
        let y = self.rect.y;

        // Back button
        if click_x >= x + 5 && click_x < x + 29 && click_y >= y + 5 && click_y < y + 25 {
            self.navigate_up();
            return true;
        }

        // Up button
        if click_x >= x + 35 && click_x < x + 59 && click_y >= y + 5 && click_y < y + 25 {
            self.navigate_up();
            return true;
        }

        // File list
        if let Some(index) = self.entry_at(click_y) {
            self.select(index);
            return true;
        }

        true
    }

    fn on_scroll(&mut self, delta: i32) -> bool {
        let visible = self.visible_items();
        let max_scroll = self.entries.len().saturating_sub(visible);
        
        if delta > 0 && self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            return true;
        } else if delta < 0 && self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
            return true;
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}
