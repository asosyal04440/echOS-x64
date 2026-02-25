//! # echOS Container Widgets
//!
//! Panel, TabControl, Splitter for layout management.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Panel container widget
pub struct Panel<'a> {
    rect: Rect,
    children: Vec<Box<dyn Widget + 'a>>,
    background: u32,
    border: bool,
    title: Option<String>,
}

impl<'a> Panel<'a> {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            children: Vec::new(),
            background: Theme::WINDOW_BG.to_u32(),
            border: true,
            title: None,
        }
    }

    pub fn with_title(mut self, title: &str) -> Self {
        self.title = Some(String::from(title));
        self
    }

    pub fn with_background(mut self, color: u32) -> Self {
        self.background = color;
        self
    }

    pub fn with_border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    pub fn add_child(&mut self, child: Box<dyn Widget + 'a>) {
        self.children.push(child);
    }

    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    pub fn children(&self) -> &Vec<Box<dyn Widget + 'a>> {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget + 'a>> {
        &mut self.children
    }
}

impl<'a> Widget for Panel<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Background
        fb.draw_rect(x, y, w, h, self.background);

        // Title bar if present
        let content_y = if let Some(title) = &self.title {
            let title_h = 24;
            fb.draw_rect(x, y, w, title_h, Theme::TITLEBAR_BG.to_u32());
            fb.draw_string(x + 8, y + 4, title, Theme::TEXT_PRIMARY.to_u32());
            title_h as usize
        } else {
            0
        };

        // Border
        if self.border {
            for col in x..(x + w) {
                fb.plot_pixel(col, y, Theme::BORDER.to_u32());
                fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
            }
            for row in y..(y + h) {
                fb.plot_pixel(x, row, Theme::BORDER.to_u32());
                fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
            }
        }

        // Children
        for child in &self.children {
            child.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }

        // Propagate to children in reverse order (top to bottom)
        for child in self.children.iter_mut().rev() {
            if child.on_click(x, y) {
                return true;
            }
        }
        true
    }

    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        for child in &mut self.children {
            if child.on_key(key, modifiers, scancode) {
                return true;
            }
        }
        false
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        for child in &mut self.children {
            if child.on_hover(x, y) {
                changed = true;
            }
        }
        changed
    }

    fn on_scroll(&mut self, delta: i32) -> bool {
        for child in &mut self.children {
            if child.on_scroll(delta) {
                return true;
            }
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn update(&mut self) {
        for child in &mut self.children {
            child.update();
        }
    }
}

/// Tab page
pub struct TabPage<'a> {
    title: String,
    panel: Panel<'a>,
}

impl<'a> TabPage<'a> {
    pub fn new(title: &str) -> Self {
        Self {
            title: String::from(title),
            panel: Panel::new(0, 0, 0, 0),
        }
    }

    pub fn with_content(mut self, panel: Panel<'a>) -> Self {
        self.panel = panel;
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn panel(&self) -> &Panel<'a> {
        &self.panel
    }

    pub fn panel_mut(&mut self) -> &mut Panel<'a> {
        &mut self.panel
    }
}

/// TabControl widget
pub struct TabControl<'a> {
    rect: Rect,
    tabs: Vec<TabPage<'a>>,
    active_tab: usize,
    tab_height: usize,
    hovered_tab: Option<usize>,
    on_tab_change: Option<fn(usize)>,
}

impl<'a> TabControl<'a> {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            tabs: Vec::new(),
            active_tab: 0,
            tab_height: 28,
            hovered_tab: None,
            on_tab_change: None,
        }
    }

    pub fn add_tab(&mut self, tab: TabPage<'a>) {
        self.tabs.push(tab);
    }

    pub fn with_tab_change_handler(mut self, handler: fn(usize)) -> Self {
        self.on_tab_change = Some(handler);
        self
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            if let Some(handler) = self.on_tab_change {
                handler(index);
            }
        }
    }

    fn tab_rect(&self, index: usize) -> Rect {
        let mut tab_x = self.rect.x;
        for i in 0..index {
            let title = &self.tabs[i].title;
            tab_x += (title.len() * 8 + 24) as i32;
        }
        let title = &self.tabs[index].title;
        let tab_width = (title.len() * 8 + 24) as i32;
        
        Rect::new(tab_x, self.rect.y, tab_width, self.tab_height as i32)
    }

    fn content_rect(&self) -> Rect {
        Rect::new(
            self.rect.x,
            self.rect.y + self.tab_height as i32,
            self.rect.width,
            self.rect.height - self.tab_height as i32,
        )
    }
}

impl<'a> Widget for TabControl<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Tab bar background
        fb.draw_rect(x, y, w, self.tab_height, Theme::TITLEBAR_BG.to_u32());

        // Draw tabs
        for (i, tab) in self.tabs.iter().enumerate() {
            let tab_rect = self.tab_rect(i);
            let tx = tab_rect.x as usize;
            let ty = tab_rect.y as usize;
            let tw = tab_rect.width as usize;
            let th = tab_rect.height as usize;

            // Tab background
            let bg_color = if i == self.active_tab {
                Theme::WINDOW_BG.to_u32()
            } else if self.hovered_tab == Some(i) {
                Theme::BUTTON_HOVER.to_u32()
            } else {
                Theme::TITLEBAR_BG.to_u32()
            };
            fb.draw_rect(tx, ty, tw, th, bg_color);

            // Tab border
            for col in tx..(tx + tw) {
                fb.plot_pixel(col, ty, Theme::BORDER.to_u32());
            }
            for row in ty..(ty + th) {
                fb.plot_pixel(tx, row, Theme::BORDER.to_u32());
                fb.plot_pixel(tx + tw - 1, row, Theme::BORDER.to_u32());
            }

            // Tab title
            let text_x = tx + 12;
            let text_y = ty + (th - 16) / 2;
            let text_color = if i == self.active_tab {
                Theme::TEXT_PRIMARY.to_u32()
            } else {
                Theme::TEXT_SECONDARY.to_u32()
            };
            fb.draw_string(text_x, text_y, &tab.title, text_color);
        }

        // Content area
        let content = self.content_rect();
        fb.draw_rect(
            content.x as usize,
            content.y as usize,
            content.width as usize,
            content.height as usize,
            Theme::WINDOW_BG.to_u32(),
        );

        // Content border
        for col in content.x as usize..(content.x as usize + content.width as usize) {
            fb.plot_pixel(col, content.y as usize, Theme::BORDER.to_u32());
        }
        for row in content.y as usize..(content.y as usize + content.height as usize) {
            fb.plot_pixel(content.x as usize, row, Theme::BORDER.to_u32());
            fb.plot_pixel(content.x as usize + content.width as usize - 1, row, Theme::BORDER.to_u32());
        }

        // Draw active tab content
        if !self.tabs.is_empty() && self.active_tab < self.tabs.len() {
            // Update content panel position
            let panel = &self.tabs[self.active_tab].panel;
            // Note: In a real implementation, we'd need interior mutability here
            // For now, just draw the panel as-is
            panel.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }

        // Check tab clicks
        for i in 0..self.tabs.len() {
            if self.tab_rect(i).contains(x, y) {
                self.set_active_tab(i);
                return true;
            }
        }

        // Propagate to active tab content
        if self.active_tab < self.tabs.len() {
            self.tabs[self.active_tab].panel_mut().on_click(x, y);
        }
        true
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_tab;
        
        self.hovered_tab = None;
        for i in 0..self.tabs.len() {
            if self.tab_rect(i).contains(x, y) {
                self.hovered_tab = Some(i);
                break;
            }
        }
        
        old_hovered != self.hovered_tab
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn update(&mut self) {
        if self.active_tab < self.tabs.len() {
            self.tabs[self.active_tab].panel_mut().update();
        }
    }
}

/// Splitter orientation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

/// Splitter widget (resizable split panel)
pub struct Splitter<'a> {
    rect: Rect,
    orientation: SplitOrientation,
    split_pos: i32,
    min_first: i32,
    min_second: i32,
    first: Option<Box<dyn Widget + 'a>>,
    second: Option<Box<dyn Widget + 'a>>,
    dragging: bool,
    splitter_size: i32,
}

impl<'a> Splitter<'a> {
    pub fn new(x: i32, y: i32, width: i32, height: i32, orientation: SplitOrientation) -> Self {
        let split_pos = match orientation {
            SplitOrientation::Horizontal => width / 2,
            SplitOrientation::Vertical => height / 2,
        };
        
        Self {
            rect: Rect::new(x, y, width, height),
            orientation,
            split_pos,
            min_first: 50,
            min_second: 50,
            first: None,
            second: None,
            dragging: false,
            splitter_size: 5,
        }
    }

    pub fn with_first(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.first = Some(widget);
        self
    }

    pub fn with_second(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.second = Some(widget);
        self
    }

    pub fn with_split_pos(mut self, pos: i32) -> Self {
        self.split_pos = pos;
        self
    }

    pub fn with_min_sizes(mut self, first: i32, second: i32) -> Self {
        self.min_first = first;
        self.min_second = second;
        self
    }

    pub fn split_pos(&self) -> i32 {
        self.split_pos
    }

    fn first_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x,
                self.rect.y,
                self.split_pos - self.splitter_size / 2,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y,
                self.rect.width,
                self.split_pos - self.splitter_size / 2,
            ),
        }
    }

    fn second_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x + self.split_pos + self.splitter_size / 2 + 1,
                self.rect.y,
                self.rect.width - self.split_pos - self.splitter_size / 2 - 1,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + self.split_pos + self.splitter_size / 2 + 1,
                self.rect.width,
                self.rect.height - self.split_pos - self.splitter_size / 2 - 1,
            ),
        }
    }

    fn splitter_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x + self.split_pos - self.splitter_size / 2,
                self.rect.y,
                self.splitter_size,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + self.split_pos - self.splitter_size / 2,
                self.rect.width,
                self.splitter_size,
            ),
        }
    }

    fn clamp_split_pos(&mut self, pos: i32) {
        let max_pos = match self.orientation {
            SplitOrientation::Horizontal => self.rect.width - self.min_second,
            SplitOrientation::Vertical => self.rect.height - self.min_second,
        };
        self.split_pos = pos.max(self.min_first).min(max_pos);
    }
}

impl<'a> Widget for Splitter<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        // Draw first panel
        if let Some(first) = &self.first {
            first.draw(fb);
        }

        // Draw splitter
        let splitter = self.splitter_rect();
        let splitter_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        fb.draw_rect(
            splitter.x as usize,
            splitter.y as usize,
            splitter.width as usize,
            splitter.height as usize,
            splitter_color,
        );

        // Draw second panel
        if let Some(second) = &self.second {
            second.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            self.dragging = false;
            return false;
        }

        // Check splitter
        if self.splitter_rect().contains(x, y) {
            self.dragging = true;
            return true;
        }

        self.dragging = false;

        // Propagate to panels
        let first_rect = self.first_rect();
        let second_rect = self.second_rect();
        
        if let Some(first) = &mut self.first {
            if first_rect.contains(x, y) {
                return first.on_click(x, y);
            }
        }
        if let Some(second) = &mut self.second {
            if second_rect.contains(x, y) {
                return second.on_click(x, y);
            }
        }
        false
    }

    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if !self.dragging {
            return false;
        }

        let delta = match self.orientation {
            SplitOrientation::Horizontal => dx,
            SplitOrientation::Vertical => dy,
        };
        
        self.clamp_split_pos(self.split_pos + delta);
        true
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        // Propagate hover to children
        let mut changed = false;
        
        let first_rect = self.first_rect();
        let second_rect = self.second_rect();
        
        if let Some(first) = &mut self.first {
            if first_rect.contains(x, y) {
                changed = first.on_hover(x, y) || changed;
            }
        }
        if let Some(second) = &mut self.second {
            if second_rect.contains(x, y) {
                changed = second.on_hover(x, y) || changed;
            }
        }
        changed
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn update(&mut self) {
        if let Some(first) = &mut self.first {
            first.update();
        }
        if let Some(second) = &mut self.second {
            second.update();
        }
    }
}
