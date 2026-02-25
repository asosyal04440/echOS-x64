//! # echOS Start Menu
//!
//! Application launcher and system menu.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Rect, Widget};
use crate::gui::widgets::list::{ListView, ListItem};
use crate::gui::widgets::text_input::TextBox;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// Application entry
#[derive(Clone)]
pub struct AppEntry {
    pub name: String,
    pub icon: String,
    pub exec: String,
    pub category: AppCategory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCategory {
    System,
    Utilities,
    Development,
    Graphics,
    Multimedia,
    Network,
    Office,
    Games,
    Other,
}

impl AppEntry {
    pub fn new(name: &str, exec: &str) -> Self {
        Self {
            name: String::from(name),
            icon: String::new(),
            exec: String::from(exec),
            category: AppCategory::Other,
        }
    }

    pub fn with_icon(mut self, icon: &str) -> Self {
        self.icon = String::from(icon);
        self
    }

    pub fn with_category(mut self, category: AppCategory) -> Self {
        self.category = category;
        self
    }
}

/// Start Menu widget
pub struct StartMenu {
    rect: Rect,
    visible: bool,
    apps: Vec<AppEntry>,
    filtered_apps: Vec<usize>,
    selected_index: Option<usize>,
    hovered_index: Option<usize>,
    search_text: String,
    search_focused: bool,
    scroll_offset: usize,
    on_launch: Option<fn(&AppEntry)>,
}

impl StartMenu {
    pub fn new() -> Self {
        Self {
            rect: Rect::new(0, 0, 280, 400),
            visible: false,
            apps: Vec::new(),
            filtered_apps: Vec::new(),
            selected_index: None,
            hovered_index: None,
            search_text: String::new(),
            search_focused: false,
            scroll_offset: 0,
            on_launch: None,
        }
    }

    pub fn add_app(&mut self, app: AppEntry) {
        self.apps.push(app);
    }

    pub fn set_apps(&mut self, apps: Vec<AppEntry>) {
        self.apps = apps;
        self.update_filter();
    }

    pub fn show(&mut self, x: i32, y: i32) {
        self.rect.x = x;
        self.rect.y = y;
        self.visible = true;
        self.search_text.clear();
        self.search_focused = true;
        self.selected_index = None;
        self.scroll_offset = 0;
        self.update_filter();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.search_focused = false;
    }

    pub fn toggle(&mut self, x: i32, y: i32) {
        if self.visible {
            self.hide();
        } else {
            self.show(x, y);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn with_launch_handler(mut self, handler: fn(&AppEntry)) -> Self {
        self.on_launch = Some(handler);
        self
    }

    fn update_filter(&mut self) {
        self.filtered_apps.clear();
        
        let search_lower = self.search_text.to_lowercase();
        
        for (i, app) in self.apps.iter().enumerate() {
            if search_lower.is_empty() || app.name.to_lowercase().contains(&search_lower) {
                self.filtered_apps.push(i);
            }
        }
        
        self.scroll_offset = 0;
        self.selected_index = None;
    }

    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 80) / 32
    }

    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 70;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / 32);
        if index < self.filtered_apps.len() {
            Some(index)
        } else {
            None
        }
    }

    fn select(&mut self, index: usize) {
        if index < self.filtered_apps.len() {
            self.selected_index = Some(index);
            
            let visible = self.visible_items();
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + visible {
                self.scroll_offset = index - visible + 1;
            }
        }
    }

    fn launch(&mut self, index: usize) {
        if let Some(&app_idx) = self.filtered_apps.get(index) {
            if let Some(handler) = self.on_launch {
                handler(&self.apps[app_idx]);
            }
            self.hide();
        }
    }
}

impl Default for StartMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for StartMenu {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Shadow
        fb.draw_rect(x + 6, y + 6, w, h, Theme::SHADOW.to_u32());

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

        // Header
        fb.draw_rect(x, y, w, 50, Theme::ACCENT_PRIMARY.to_u32());
        fb.draw_string(x + 10, y + 15, "echOS", Theme::DESKTOP_BG.to_u32());
        fb.draw_string(x + 10, y + 30, "Applications", Theme::DESKTOP_BG.to_u32());

        // Search box
        let search_y = y + 55;
        fb.draw_rect(x + 10, search_y, w - 20, 24, if self.search_focused {
            Theme::WINDOW_BG.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        });
        
        // Search border
        for col in (x + 10)..(x + w - 10) {
            fb.plot_pixel(col, search_y, if self.search_focused {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::BORDER.to_u32()
            });
            fb.plot_pixel(col, search_y + 23, Theme::BORDER.to_u32());
        }
        
        // Search text
        let search_display = if self.search_text.is_empty() && !self.search_focused {
            "Search..."
        } else {
            &self.search_text
        };
        fb.draw_string(x + 15, search_y + 4, search_display, 
            if self.search_text.is_empty() && !self.search_focused {
                Theme::TEXT_SECONDARY.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            }
        );

        // App list
        let item_height = 32;
        let visible = self.visible_items();
        let item_y_start = y + 85;

        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.filtered_apps.len() {
                break;
            }

            let app_idx = self.filtered_apps[item_index];
            let app = &self.apps[app_idx];
            let item_y = item_y_start + i * item_height;

            // Selection/hover background
            if self.selected_index == Some(item_index) {
                fb.draw_rect(x + 2, item_y, w - 4, item_height, Theme::ACCENT_PRIMARY.to_u32());
            } else if self.hovered_index == Some(item_index) {
                fb.draw_rect(x + 2, item_y, w - 4, item_height, Theme::BUTTON_HOVER.to_u32());
            }

            // Icon placeholder
            fb.draw_rect(x + 8, item_y + 4, 24, 24, Theme::TEXT_SECONDARY.to_u32());
            
            // App name
            let text_color = if self.selected_index == Some(item_index) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(x + 40, item_y + 8, &app.name, text_color);
        }

        // Footer - Power options
        let footer_y = y + h - 35;
        fb.draw_rect(x, footer_y, w, 35, Theme::TITLEBAR_BG.to_u32());
        
        // Power button
        fb.draw_rect(x + w - 80, footer_y + 5, 70, 25, Theme::ACCENT_ERROR.to_u32());
        fb.draw_string(x + w - 60, footer_y + 10, "Power", Theme::TEXT_PRIMARY.to_u32());
    }

    fn on_click(&mut self, click_x: i32, click_y: i32) -> bool {
        if !self.visible {
            return false;
        }

        if !self.rect.contains(click_x, click_y) {
            self.hide();
            return false;
        }

        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        let h = self.rect.height;

        // Search box
        if click_x >= x + 10 && click_x < x + w - 10 && click_y >= y + 55 && click_y < y + 79 {
            self.search_focused = true;
            return true;
        } else {
            self.search_focused = false;
        }

        // App list
        if let Some(index) = self.item_at(click_y) {
            self.select(index);
            return true;
        }

        // Power button
        if click_x >= x + w - 80 && click_x < x + w - 10 && click_y >= y + h - 30 && click_y < y + h - 5 {
            // Power action - could trigger shutdown dialog
            self.hide();
            return true;
        }

        true
    }

    fn on_key(&mut self, key: char, _modifiers: u8, scancode: u8) -> bool {
        if !self.visible || !self.search_focused {
            return false;
        }

        match scancode {
            0x0E => { // Backspace
                if !self.search_text.is_empty() {
                    self.search_text.pop();
                    self.update_filter();
                }
                true
            }
            0x1C => { // Enter
                if let Some(index) = self.selected_index {
                    self.launch(index);
                } else if !self.filtered_apps.is_empty() {
                    self.launch(0);
                }
                true
            }
            0x48 => { // Up arrow
                if let Some(idx) = self.selected_index {
                    if idx > 0 {
                        self.select(idx - 1);
                    }
                } else if !self.filtered_apps.is_empty() {
                    self.select(0);
                }
                true
            }
            0x50 => { // Down arrow
                if let Some(idx) = self.selected_index {
                    if idx < self.filtered_apps.len() - 1 {
                        self.select(idx + 1);
                    }
                } else if !self.filtered_apps.is_empty() {
                    self.select(0);
                }
                true
            }
            _ => {
                if key != '\0' {
                    self.search_text.push(key);
                    self.update_filter();
                    true
                } else {
                    false
                }
            }
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.item_at(y)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}
