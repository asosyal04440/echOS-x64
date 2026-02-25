//! # echOS Text Input Widget
//!
//! TextBox (single line) and TextArea (multi-line) widgets.

use super::{Rect, Widget, MOD_CTRL};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Single-line text input
pub struct TextBox {
    rect: Rect,
    text: String,
    placeholder: String,
    cursor_pos: usize,
    focused: bool,
    scroll_offset: usize,
    max_length: usize,
    password_mode: bool,
}

impl TextBox {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            text: String::new(),
            placeholder: String::new(),
            cursor_pos: 0,
            focused: false,
            scroll_offset: 0,
            max_length: 256,
            password_mode: false,
        }
    }

    pub fn with_placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = String::from(placeholder);
        self
    }

    pub fn with_max_length(mut self, max: usize) -> Self {
        self.max_length = max;
        self
    }

    pub fn set_password_mode(&mut self, enabled: bool) {
        self.password_mode = enabled;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
        self.cursor_pos = self.text.len().min(self.max_length);
        self.update_scroll();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor_pos = 0;
        self.scroll_offset = 0;
    }

    fn update_scroll(&mut self) {
        let char_width = 8;
        let visible_chars = (self.rect.width as usize - 10) / char_width;
        
        if self.cursor_pos < self.scroll_offset {
            self.scroll_offset = self.cursor_pos;
        } else if self.cursor_pos > self.scroll_offset + visible_chars {
            self.scroll_offset = self.cursor_pos - visible_chars;
        }
    }

    fn insert_char(&mut self, c: char) {
        if self.text.len() < self.max_length && c.is_ascii_graphic() || c == ' ' {
            self.text.insert(self.cursor_pos, c);
            self.cursor_pos += 1;
            self.update_scroll();
        }
    }

    fn delete_char(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.text.remove(self.cursor_pos);
        }
    }

    fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.text.remove(self.cursor_pos);
            self.update_scroll();
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.update_scroll();
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.cursor_pos += 1;
            self.update_scroll();
        }
    }

    fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
        self.scroll_offset = 0;
    }

    fn move_cursor_end(&mut self) {
        self.cursor_pos = self.text.len();
        self.update_scroll();
    }
}

impl Widget for TextBox {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Background
        let bg_color = if self.focused {
            Theme::WINDOW_BG.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, w, h, bg_color);

        // Border
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        
        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + h - 1, border_color);
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + w - 1, row, border_color);
        }

        // Text or placeholder
        let text_y = y + (h - 16) / 2;
        let text_x = x + 5;

        if self.text.is_empty() && !self.focused {
            fb.draw_string(text_x, text_y, &self.placeholder, Theme::TEXT_SECONDARY.to_u32());
        } else {
            let display_text = if self.password_mode {
                alloc::string::ToString::to_string(&"*".repeat(self.text.len()))
            } else {
                let start = self.scroll_offset;
                let end = (start + (w - 10) / 8).min(self.text.len());
                alloc::string::ToString::to_string(&self.text[start..end])
            };
            fb.draw_string(text_x, text_y, &display_text, Theme::TEXT_PRIMARY.to_u32());
        }

        // Cursor
        if self.focused {
            let cursor_char_pos = self.cursor_pos.saturating_sub(self.scroll_offset);
            let cursor_x = text_x + cursor_char_pos * 8;
            if cursor_x < x + w - 5 {
                for dy in 0..16 {
                    fb.plot_pixel(cursor_x, text_y + dy, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.focused = true;
            // Calculate cursor position from click
            let text_x = self.rect.x + 5;
            let click_offset = ((x - text_x) / 8) as usize;
            self.cursor_pos = (self.scroll_offset + click_offset).min(self.text.len());
            true
        } else {
            self.focused = false;
            false
        }
    }

    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.focused {
            return false;
        }

        match scancode {
            0x0E => self.backspace(),           // Backspace
            0x53 => self.delete_char(),          // Delete
            0x4B => self.move_cursor_left(),     // Left arrow
            0x4D => self.move_cursor_right(),    // Right arrow
            0x47 => self.move_cursor_home(),     // Home
            0x4F => self.move_cursor_end(),      // End
            _ => {
                if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                    self.insert_char(key);
                }
            }
        }
        true
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }
}

/// Multi-line text area
pub struct TextArea {
    rect: Rect,
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    scroll_line: usize,
    scroll_col: usize,
    focused: bool,
    line_height: usize,
}

impl TextArea {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            scroll_line: 0,
            scroll_col: 0,
            focused: false,
            line_height: 18,
        }
    }

    pub fn text(&self) -> String {
        alloc::string::ToString::to_string(&self.lines.join("\n"))
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_line = 0;
        self.scroll_col = 0;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(String::new());
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll_line = 0;
        self.scroll_col = 0;
    }

    fn visible_lines(&self) -> usize {
        (self.rect.height as usize - 10) / self.line_height
    }

    fn visible_cols(&self) -> usize {
        (self.rect.width as usize - 10) / 8
    }

    fn insert_char(&mut self, c: char) {
        if c == '\n' {
            // Split line at cursor
            let current_line = self.lines[self.cursor_line].clone();
            let after_cursor: String = current_line[self.cursor_col..].into();
            self.lines[self.cursor_line].truncate(self.cursor_col);
            self.lines.insert(self.cursor_line + 1, after_cursor);
            self.cursor_line += 1;
            self.cursor_col = 0;
        } else if c.is_ascii_graphic() || c == ' ' || c == '\t' {
            self.lines[self.cursor_line].insert(self.cursor_col, c);
            self.cursor_col += 1;
        }
        self.update_scroll();
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.lines[self.cursor_line].remove(self.cursor_col);
        } else if self.cursor_line > 0 {
            // Merge with previous line
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].len();
            let current = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&current);
        }
        self.update_scroll();
    }

    fn update_scroll(&mut self) {
        let visible_lines = self.visible_lines();
        let visible_cols = self.visible_cols();

        if self.cursor_line < self.scroll_line {
            self.scroll_line = self.cursor_line;
        } else if self.cursor_line >= self.scroll_line + visible_lines {
            self.scroll_line = self.cursor_line - visible_lines + 1;
        }

        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        } else if self.cursor_col >= self.scroll_col + visible_cols {
            self.scroll_col = self.cursor_col - visible_cols + 1;
        }
    }
}

impl Widget for TextArea {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Background
        let bg_color = Theme::WINDOW_BG.to_u32();
        fb.draw_rect(x, y, w, h, bg_color);

        // Border
        let border_color = if self.focused {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        
        for col in x..(x + w) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + h - 1, border_color);
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + w - 1, row, border_color);
        }

        // Draw visible lines
        let text_x = x + 5;
        let mut text_y = y + 5;
        let visible_lines = self.visible_lines();
        let visible_cols = self.visible_cols();

        for i in 0..visible_lines {
            let line_idx = self.scroll_line + i;
            if line_idx >= self.lines.len() {
                break;
            }

            let line = &self.lines[line_idx];
            let start = self.scroll_col.min(line.len());
            let end = (start + visible_cols).min(line.len());
            let display = &line[start..end];
            
            fb.draw_string(text_x, text_y, display, Theme::TEXT_PRIMARY.to_u32());
            text_y += self.line_height;
        }

        // Cursor
        if self.focused {
            let cursor_screen_line = self.cursor_line - self.scroll_line;
            let cursor_screen_col = self.cursor_col.saturating_sub(self.scroll_col);
            let cursor_x = text_x + cursor_screen_col * 8;
            let cursor_y = y + 5 + cursor_screen_line * self.line_height;
            
            if cursor_x < x + w - 5 && cursor_y < y + h - 5 {
                for dy in 0..16 {
                    fb.plot_pixel(cursor_x, cursor_y + dy, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.focused = true;
            
            // Calculate cursor position
            let text_x = self.rect.x + 5;
            let text_y = self.rect.y + 5;
            
            self.cursor_line = self.scroll_line + ((y - text_y) as usize / self.line_height);
            self.cursor_col = self.scroll_col + ((x - text_x) as usize / 8);
            
            self.cursor_line = self.cursor_line.min(self.lines.len() - 1);
            self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
            
            true
        } else {
            self.focused = false;
            false
        }
    }

    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.focused {
            return false;
        }

        match scancode {
            0x0E => self.backspace(),
            0x48 => { // Up arrow
                if self.cursor_line > 0 {
                    self.cursor_line -= 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x50 => { // Down arrow
                if self.cursor_line < self.lines.len() - 1 {
                    self.cursor_line += 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_line].len());
                    self.update_scroll();
                }
            }
            0x4B => { // Left arrow
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.update_scroll();
                }
            }
            0x4D => { // Right arrow
                if self.cursor_col < self.lines[self.cursor_line].len() {
                    self.cursor_col += 1;
                    self.update_scroll();
                }
            }
            _ => {
                if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                    self.insert_char(key);
                }
            }
        }
        true
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }
}
