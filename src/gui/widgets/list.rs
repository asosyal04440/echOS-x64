//! # echOS ListView and TreeView Widgets
//!
//! List and tree selection widgets.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

/// List item
#[derive(Clone)]
pub struct ListItem {
    pub text: String,
    pub id: usize,
    pub selected: bool,
    pub icon: Option<u8>,  // Icon index (optional)
}

impl ListItem {
    pub fn new(id: usize, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            selected: false,
            icon: None,
        }
    }

    pub fn with_icon(mut self, icon: u8) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// ListView widget (single/multi-column list)
pub struct ListView {
    rect: Rect,
    items: Vec<ListItem>,
    selected_index: Option<usize>,
    scroll_offset: usize,
    item_height: usize,
    multi_select: bool,
    hovered_index: Option<usize>,
    on_select: Option<fn(usize)>,
    on_double_click: Option<fn(usize)>,
}

impl ListView {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            items: Vec::new(),
            selected_index: None,
            scroll_offset: 0,
            item_height: 24,
            multi_select: false,
            hovered_index: None,
            on_select: None,
            on_double_click: None,
        }
    }

    pub fn with_multi_select(mut self, enabled: bool) -> Self {
        self.multi_select = enabled;
        self
    }

    pub fn add_item(&mut self, item: ListItem) {
        self.items.push(item);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    pub fn items(&self) -> &Vec<ListItem> {
        &self.items
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn selected_item(&self) -> Option<&ListItem> {
        self.selected_index.and_then(|i| self.items.get(i))
    }

    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 4) / self.item_height
    }

    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 2;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / self.item_height);
        if index < self.items.len() {
            Some(index)
        } else {
            None
        }
    }

    fn select(&mut self, index: usize) {
        // Clear previous selection
        if !self.multi_select {
            for item in &mut self.items {
                item.selected = false;
            }
        }
        
        if index < self.items.len() {
            self.items[index].selected = true;
            self.selected_index = Some(index);
            
            // Scroll to visible
            let visible = self.visible_items();
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + visible {
                self.scroll_offset = index - visible + 1;
            }

            if let Some(handler) = self.on_select {
                handler(index);
            }
        }
    }
}

impl Widget for ListView {
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

        // Draw items
        let visible = self.visible_items();
        let item_y_start = y + 2;
        
        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.items.len() {
                break;
            }

            let item = &self.items[item_index];
            let item_y = item_y_start + i * self.item_height;

            // Selection background
            if item.selected {
                fb.draw_rect(x + 1, item_y, w - 2, self.item_height, Theme::ACCENT_PRIMARY.to_u32());
            } else if self.hovered_index == Some(item_index) {
                fb.draw_rect(x + 1, item_y, w - 2, self.item_height, Theme::BUTTON_HOVER.to_u32());
            }

            // Icon (if any)
            let mut text_x = x + 4;
            if let Some(_icon) = item.icon {
                // Draw icon placeholder
                fb.draw_rect(text_x, item_y + 4, 16, 16, Theme::TEXT_SECONDARY.to_u32());
                text_x += 20;
            }

            // Text
            let text_y = item_y + (self.item_height - 16) / 2;
            let text_color = if item.selected {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(text_x, text_y, &item.text, text_color);
        }

        // Scroll indicator
        if self.items.len() > visible {
            let scroll_bar_height = (h * visible / self.items.len()).max(20);
            let scroll_bar_y = y + (h * self.scroll_offset / self.items.len());
            fb.draw_rect(x + w - 8, scroll_bar_y, 6, scroll_bar_height, Theme::BUTTON_BG.to_u32());
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            if let Some(index) = self.item_at(y) {
                self.select(index);
            }
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.item_at(y)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    fn on_scroll(&mut self, delta: i32) -> bool {
        let visible = self.visible_items();
        let max_scroll = self.items.len().saturating_sub(visible);
        
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

/// Tree node
#[derive(Clone)]
pub struct TreeNode {
    pub text: String,
    pub id: usize,
    pub children: Vec<TreeNode>,
    pub expanded: bool,
    pub selected: bool,
    pub level: usize,
}

impl TreeNode {
    pub fn new(id: usize, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            children: Vec::new(),
            expanded: false,
            selected: false,
            level: 0,
        }
    }

    pub fn add_child(mut self, child: TreeNode) -> Self {
        let mut child = child;
        child.level = self.level + 1;
        self.children.push(child);
        self
    }

    fn flatten(&self, result: &mut Vec<(usize, String, bool, bool, usize)>) {
        result.push((self.id, self.text.clone(), self.expanded, self.selected, self.level));
        if self.expanded {
            for child in &self.children {
                child.flatten(result);
            }
        }
    }
}

/// TreeView widget
pub struct TreeView {
    rect: Rect,
    root_nodes: Vec<TreeNode>,
    flattened: Vec<(usize, String, bool, bool, usize)>, // id, text, expanded, selected, level
    selected_id: Option<usize>,
    scroll_offset: usize,
    item_height: usize,
    hovered_index: Option<usize>,
}

impl TreeView {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            root_nodes: Vec::new(),
            flattened: Vec::new(),
            selected_id: None,
            scroll_offset: 0,
            item_height: 22,
            hovered_index: None,
        }
    }

    pub fn add_root(&mut self, node: TreeNode) {
        self.root_nodes.push(node);
        self.rebuild_flattened();
    }

    pub fn clear(&mut self) {
        self.root_nodes.clear();
        self.flattened.clear();
        self.selected_id = None;
        self.scroll_offset = 0;
    }

    fn rebuild_flattened(&mut self) {
        self.flattened.clear();
        for node in &self.root_nodes {
            node.flatten(&mut self.flattened);
        }
    }

    fn toggle_expand(&mut self, index: usize) {
        // Find and toggle the node
        if index < self.flattened.len() {
            let id = self.flattened[index].0;
            let expanded = self.flattened[index].2;
            // Toggle in root_nodes
            Self::toggle_node_recursive_static(&mut self.root_nodes, id, !expanded);
            self.rebuild_flattened();
        }
    }

    fn toggle_node_recursive_static(nodes: &mut Vec<TreeNode>, id: usize, new_expanded: bool) -> bool {
        for node in nodes {
            if node.id == id {
                node.expanded = new_expanded;
                return true;
            }
            if Self::toggle_node_recursive_static(&mut node.children, id, new_expanded) {
                return true;
            }
        }
        false
    }

    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 4) / self.item_height
    }

    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 2;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / self.item_height);
        if index < self.flattened.len() {
            Some(index)
        } else {
            None
        }
    }
}

impl Widget for TreeView {
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

        // Draw items
        let visible = self.visible_items();
        let item_y_start = y + 2;

        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.flattened.len() {
                break;
            }

            let (id, text, expanded, selected, level) = &self.flattened[item_index];
            let item_y = item_y_start + i * self.item_height;

            // Selection background
            if *selected {
                fb.draw_rect(x + 1, item_y, w - 2, self.item_height, Theme::ACCENT_PRIMARY.to_u32());
            } else if self.hovered_index == Some(item_index) {
                fb.draw_rect(x + 1, item_y, w - 2, self.item_height, Theme::BUTTON_HOVER.to_u32());
            }

            // Indent
            let indent = level * 16;
            let text_x = x + 4 + indent;

            // Expand/collapse indicator
            let has_children = false; // Would need to check actual children
            if has_children {
                let indicator = if *expanded { "-" } else { "+" };
                fb.draw_string(text_x, item_y + 3, indicator, Theme::TEXT_SECONDARY.to_u32());
            }

            // Text
            let text_y = item_y + (self.item_height - 16) / 2;
            let text_color = if *selected {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(text_x + 12, text_y, text, text_color);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            if let Some(index) = self.item_at(y) {
                // Check if clicked on expand indicator
                let (_, _, _, _, level) = self.flattened[index];
                let indent = level * 16;
                let indicator_x = self.rect.x + 4 + indent as i32;
                
                if x >= indicator_x && x < indicator_x + 12 {
                    self.toggle_expand(index);
                } else {
                    // Select item
                    for item in &mut self.flattened {
                        item.3 = false;
                    }
                    self.flattened[index].3 = true;
                    self.selected_id = Some(self.flattened[index].0);
                }
            }
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.item_at(y)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    fn on_scroll(&mut self, delta: i32) -> bool {
        let visible = self.visible_items();
        let max_scroll = self.flattened.len().saturating_sub(visible);
        
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
