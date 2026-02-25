//! # echOS Menu Widgets
//!
//! Menu, ContextMenu, MenuItem for dropdown and popup menus.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

/// Menu item
#[derive(Clone)]
pub struct MenuItem {
    pub text: String,
    pub id: u32,
    pub shortcut: String,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: Option<Vec<MenuItem>>,
}

impl MenuItem {
    pub fn new(id: u32, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            shortcut: String::new(),
            enabled: true,
            separator: false,
            submenu: None,
        }
    }

    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = String::from(shortcut);
        self
    }

    pub fn separator() -> Self {
        Self {
            text: String::new(),
            id: 0,
            shortcut: String::new(),
            enabled: true,
            separator: true,
            submenu: None,
        }
    }

    pub fn with_submenu(mut self, items: Vec<MenuItem>) -> Self {
        self.submenu = Some(items);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Menu bar (horizontal menu strip)
pub struct MenuBar {
    rect: Rect,
    menus: Vec<(String, Vec<MenuItem>)>,
    open_menu: Option<usize>,
    hovered_menu: Option<usize>,
    on_select: Option<fn(u32)>,
}

impl MenuBar {
    pub fn new(x: i32, y: i32, width: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, 28),
            menus: Vec::new(),
            open_menu: None,
            hovered_menu: None,
            on_select: None,
        }
    }

    pub fn add_menu(&mut self, title: &str, items: Vec<MenuItem>) {
        self.menus.push((String::from(title), items));
    }

    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    fn menu_at(&self, x: i32) -> Option<usize> {
        let mut menu_x = self.rect.x + 5;
        for (i, (title, _)) in self.menus.iter().enumerate() {
            let menu_width = (title.len() * 8 + 16) as i32;
            if x >= menu_x && x < menu_x + menu_width {
                return Some(i);
            }
            menu_x += menu_width;
        }
        None
    }

    fn menu_width(&self, index: usize) -> i32 {
        if index < self.menus.len() {
            (self.menus[index].0.len() * 8 + 16) as i32
        } else {
            0
        }
    }

    fn menu_x(&self, index: usize) -> i32 {
        let mut x = self.rect.x + 5;
        for i in 0..index {
            x += self.menu_width(i);
        }
        x
    }
}

impl Widget for MenuBar {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Background
        fb.draw_rect(x, y, w, h, Theme::TITLEBAR_BG.to_u32());

        // Bottom border
        for col in x..(x + w) {
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }

        // Draw menu titles
        let mut menu_x = x + 5;
        for (i, (title, _)) in self.menus.iter().enumerate() {
            let menu_w = title.len() * 8 + 16;
            
            // Highlight if open or hovered
            if self.open_menu == Some(i) {
                fb.draw_rect(menu_x, y, menu_w, h, Theme::ACCENT_PRIMARY.to_u32());
            } else if self.hovered_menu == Some(i) {
                fb.draw_rect(menu_x, y, menu_w, h, Theme::BUTTON_HOVER.to_u32());
            }

            // Title text
            let text_x = menu_x + 8;
            let text_y = y + (h - 16) / 2;
            let text_color = if self.open_menu == Some(i) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(text_x, text_y, title, text_color);

            menu_x += menu_w;
        }

        // Draw dropdown if open
        if let Some(menu_idx) = self.open_menu {
            let (_, items) = &self.menus[menu_idx];
            let dropdown_x = self.menu_x(menu_idx) as usize;
            let dropdown_y = y + h;
            let item_height = 24;
            let dropdown_w = 200;
            let dropdown_h = items.len() * item_height;

            // Dropdown background
            fb.draw_rect(dropdown_x, dropdown_y, dropdown_w, dropdown_h, Theme::WINDOW_BG.to_u32());

            // Border
            for col in dropdown_x..(dropdown_x + dropdown_w) {
                fb.plot_pixel(col, dropdown_y, Theme::BORDER.to_u32());
                fb.plot_pixel(col, dropdown_y + dropdown_h - 1, Theme::BORDER.to_u32());
            }
            for row in dropdown_y..(dropdown_y + dropdown_h) {
                fb.plot_pixel(dropdown_x, row, Theme::BORDER.to_u32());
                fb.plot_pixel(dropdown_x + dropdown_w - 1, row, Theme::BORDER.to_u32());
            }

            // Items
            for (i, item) in items.iter().enumerate() {
                let item_y = dropdown_y + i * item_height;

                if item.separator {
                    // Separator line
                    for col in (dropdown_x + 5)..(dropdown_x + dropdown_w - 5) {
                        fb.plot_pixel(col, item_y + item_height / 2, Theme::BORDER.to_u32());
                    }
                } else {
                    // Item text
                    let text_color = if item.enabled {
                        Theme::TEXT_PRIMARY.to_u32()
                    } else {
                        Theme::TEXT_SECONDARY.to_u32()
                    };
                    fb.draw_string(dropdown_x + 8, item_y + 4, &item.text, text_color);

                    // Shortcut
                    if !item.shortcut.is_empty() {
                        let shortcut_x = dropdown_x + dropdown_w - item.shortcut.len() * 8 - 8;
                        fb.draw_string(shortcut_x, item_y + 4, &item.shortcut, Theme::TEXT_SECONDARY.to_u32());
                    }

                    // Submenu arrow
                    if item.submenu.is_some() {
                        fb.draw_string(dropdown_x + dropdown_w - 16, item_y + 4, ">", Theme::TEXT_SECONDARY.to_u32());
                    }
                }
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        // Check if clicked on menu title
        if y >= self.rect.y && y < self.rect.y + self.rect.height {
            if let Some(menu_idx) = self.menu_at(x) {
                if self.open_menu == Some(menu_idx) {
                    self.open_menu = None;
                } else {
                    self.open_menu = Some(menu_idx);
                }
                return true;
            }
        }

        // Check if clicked on dropdown item
        if let Some(menu_idx) = self.open_menu {
            let dropdown_x = self.menu_x(menu_idx);
            let dropdown_y = self.rect.y + self.rect.height;
            let (_, items) = &self.menus[menu_idx];
            let item_height = 24;
            let dropdown_w = 200;

            if x >= dropdown_x && x < dropdown_x + dropdown_w {
                let relative_y = y - dropdown_y;
                if relative_y >= 0 {
                    let item_idx = (relative_y / item_height) as usize;
                    if item_idx < items.len() {
                        let item = &items[item_idx];
                        if item.enabled && !item.separator {
                            if let Some(handler) = self.on_select {
                                handler(item.id);
                            }
                        }
                    }
                }
            }
            self.open_menu = None;
            return true;
        }

        self.open_menu = None;
        false
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_menu;
        
        if y >= self.rect.y && y < self.rect.y + self.rect.height {
            self.hovered_menu = self.menu_at(x);
            
            // If menu is open, switch to hovered menu
            if self.open_menu.is_some() && self.hovered_menu != old_hovered {
                self.open_menu = self.hovered_menu;
            }
        } else {
            self.hovered_menu = None;
        }
        
        old_hovered != self.hovered_menu
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// Context menu (right-click popup)
pub struct ContextMenu {
    items: Vec<MenuItem>,
    visible: bool,
    x: i32,
    y: i32,
    width: i32,
    hovered_index: Option<usize>,
    on_select: Option<fn(u32)>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            visible: false,
            x: 0,
            y: 0,
            width: 200,
            hovered_index: None,
            on_select: None,
        }
    }

    pub fn add_item(&mut self, item: MenuItem) {
        self.items.push(item);
    }

    pub fn show(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.hovered_index = None;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.y;
        if relative_y >= 0 {
            let index = (relative_y / 24) as usize;
            if index < self.items.len() {
                return Some(index);
            }
        }
        None
    }
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ContextMenu {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let x = self.x as usize;
        let y = self.y as usize;
        let w = self.width as usize;
        let h = self.items.len() * 24;
        let item_height = 24usize;

        // Shadow
        fb.draw_rect(x + 4, y + 4, w, h, Theme::SHADOW.to_u32());

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

        // Items
        for (i, item) in self.items.iter().enumerate() {
            let item_y = y + i * item_height;

            if item.separator {
                for col in (x + 5)..(x + w - 5) {
                    fb.plot_pixel(col, item_y + item_height / 2, Theme::BORDER.to_u32());
                }
            } else {
                // Hover highlight
                if self.hovered_index == Some(i) && item.enabled {
                    fb.draw_rect(x + 1, item_y, w - 2, item_height, Theme::BUTTON_HOVER.to_u32());
                }

                // Text
                let text_color = if item.enabled {
                    Theme::TEXT_PRIMARY.to_u32()
                } else {
                    Theme::TEXT_SECONDARY.to_u32()
                };
                fb.draw_string(x + 8, item_y + 4, &item.text, text_color);

                // Shortcut
                if !item.shortcut.is_empty() {
                    let shortcut_x = x + w - item.shortcut.len() * 8 - 8;
                    fb.draw_string(shortcut_x, item_y + 4, &item.shortcut, Theme::TEXT_SECONDARY.to_u32());
                }
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        // Check if clicked outside
        if x < self.x || x >= self.x + self.width || y < self.y {
            self.hide();
            return false;
        }

        // Check item click
        if let Some(index) = self.item_at(y) {
            let item = &self.items[index];
            if item.enabled && !item.separator {
                if let Some(handler) = self.on_select {
                    handler(item.id);
                }
            }
        }
        self.hide();
        true
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        let old_hovered = self.hovered_index;
        if x >= self.x && x < self.x + self.width && y >= self.y {
            self.hovered_index = self.item_at(y);
        } else {
            self.hovered_index = None;
        }
        old_hovered != self.hovered_index
    }

    fn bounds(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, (self.items.len() * 24) as i32)
    }
}
