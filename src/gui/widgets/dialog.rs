//! # echOS Dialog Widgets
//!
//! Dialog, MessageBox, FileDialog for user interactions.

use super::{Rect, Widget, MOD_CTRL};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::button::Button;
use crate::gui::widgets::label::Label;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Dialog result
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    Ok,
    Cancel,
    Yes,
    No,
    Retry,
    Abort,
    Ignore,
    None,
}

/// Message box type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageBoxType {
    Info,
    Warning,
    Error,
    Question,
}

/// Dialog widget (modal popup)
pub struct Dialog<'a> {
    rect: Rect,
    title: String,
    visible: bool,
    dragging: bool,
    drag_offset: (i32, i32),
    buttons: Vec<(String, DialogResult)>,
    result: DialogResult,
    content_widgets: Vec<Box<dyn Widget + 'a>>,
    on_close: Option<fn(DialogResult)>,
}

impl<'a> Dialog<'a> {
    pub fn new(title: &str, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(0, 0, width, height),
            title: String::from(title),
            visible: false,
            dragging: false,
            drag_offset: (0, 0),
            buttons: Vec::new(),
            result: DialogResult::None,
            content_widgets: Vec::new(),
            on_close: None,
        }
    }

    pub fn add_button(mut self, text: &str, result: DialogResult) -> Self {
        self.buttons.push((String::from(text), result));
        self
    }

    pub fn add_widget(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.content_widgets.push(widget);
        self
    }

    pub fn with_close_handler(mut self, handler: fn(DialogResult)) -> Self {
        self.on_close = Some(handler);
        self
    }

    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        // Center on screen
        self.rect.x = ((screen_width as i32 - self.rect.width) / 2).max(0);
        self.rect.y = ((screen_height as i32 - self.rect.height) / 2).max(0);
        self.visible = true;
        self.result = DialogResult::None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn result(&self) -> DialogResult {
        self.result
    }

    fn titlebar_height(&self) -> i32 {
        28
    }

    fn is_titlebar_hit(&self, x: i32, y: i32) -> bool {
        y >= self.rect.y && y < self.rect.y + self.titlebar_height()
            && x >= self.rect.x && x < self.rect.x + self.rect.width
    }

    fn close_button_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + self.rect.width - 24,
            self.rect.y + 4,
            20,
            20,
        )
    }

    fn button_rect(&self, index: usize) -> Rect {
        let button_width = 80;
        let button_height = 28;
        let spacing = 10;
        let total_width = (self.buttons.len() as i32 * button_width) + ((self.buttons.len() - 1) as i32 * spacing);
        let start_x = self.rect.x + (self.rect.width - total_width) / 2;
        let button_y = self.rect.y + self.rect.height - button_height - 15;
        
        Rect::new(
            start_x + (index as i32 * (button_width + spacing)),
            button_y,
            button_width,
            button_height,
        )
    }
}

impl<'a> Widget for Dialog<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }

        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        let titlebar_h = self.titlebar_height() as usize;

        // Shadow
        fb.draw_rect(x + 6, y + 6, w, h, Theme::SHADOW.to_u32());

        // Background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());

        // Titlebar
        fb.draw_rect(x, y, w, titlebar_h, Theme::TITLEBAR_ACTIVE.to_u32());

        // Title text
        fb.draw_string(x + 10, y + 6, &self.title, Theme::TEXT_PRIMARY.to_u32());

        // Close button
        let close_rect = self.close_button_rect();
        fb.draw_rect(
            close_rect.x as usize,
            close_rect.y as usize,
            close_rect.width as usize,
            close_rect.height as usize,
            Theme::ACCENT_ERROR.to_u32(),
        );
        fb.draw_string(
            close_rect.x as usize + 6,
            close_rect.y as usize + 2,
            "X",
            Theme::TEXT_PRIMARY.to_u32(),
        );

        // Border
        for col in x..(x + w) {
            fb.plot_pixel(col, y, Theme::BORDER.to_u32());
            fb.plot_pixel(col, y + h - 1, Theme::BORDER.to_u32());
        }
        for row in y..(y + h) {
            fb.plot_pixel(x, row, Theme::BORDER.to_u32());
            fb.plot_pixel(x + w - 1, row, Theme::BORDER.to_u32());
        }

        // Content widgets
        for widget in &self.content_widgets {
            widget.draw(fb);
        }

        // Buttons
        for (i, (text, _)) in self.buttons.iter().enumerate() {
            let btn_rect = self.button_rect(i);
            fb.draw_rect(
                btn_rect.x as usize,
                btn_rect.y as usize,
                btn_rect.width as usize,
                btn_rect.height as usize,
                Theme::BUTTON_BG.to_u32(),
            );
            
            // Button border
            for col in btn_rect.x as usize..(btn_rect.x as usize + btn_rect.width as usize) {
                fb.plot_pixel(col, btn_rect.y as usize, Theme::BORDER.to_u32());
                fb.plot_pixel(col, btn_rect.y as usize + btn_rect.height as usize - 1, Theme::BORDER.to_u32());
            }
            for row in btn_rect.y as usize..(btn_rect.y as usize + btn_rect.height as usize) {
                fb.plot_pixel(btn_rect.x as usize, row, Theme::BORDER.to_u32());
                fb.plot_pixel(btn_rect.x as usize + btn_rect.width as usize - 1, row, Theme::BORDER.to_u32());
            }
            
            // Button text
            let text_x = btn_rect.x as usize + (btn_rect.width as usize - text.len() * 8) / 2;
            let text_y = btn_rect.y as usize + (btn_rect.height as usize - 16) / 2;
            fb.draw_string(text_x, text_y, text, Theme::TEXT_PRIMARY.to_u32());
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }

        // Check close button
        if self.close_button_rect().contains(x, y) {
            self.result = DialogResult::Cancel;
            if let Some(handler) = self.on_close {
                handler(self.result);
            }
            self.hide();
            return true;
        }

        // Check dialog buttons
        for (i, (_, result)) in self.buttons.iter().enumerate() {
            if self.button_rect(i).contains(x, y) {
                self.result = *result;
                if let Some(handler) = self.on_close {
                    handler(self.result);
                }
                self.hide();
                return true;
            }
        }

        // Check titlebar for dragging
        if self.is_titlebar_hit(x, y) {
            self.dragging = true;
            self.drag_offset = (x - self.rect.x, y - self.rect.y);
            return true;
        }

        // Check content widgets
        for widget in &mut self.content_widgets {
            if widget.on_click(x, y) {
                return true;
            }
        }

        self.rect.contains(x, y)
    }

    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if self.dragging {
            self.rect.x += dx;
            self.rect.y += dy;
            true
        } else {
            false
        }
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// MessageBox dialog
pub struct MessageBox {
    dialog: Dialog<'static>,
    message: String,
    msg_type: MessageBoxType,
}

impl MessageBox {
    pub fn new(title: &str, message: &str, msg_type: MessageBoxType) -> Self {
        let width = 400;
        let height = 150;
        
        let mut dialog = Dialog::new(title, width, height);
        
        // Add appropriate buttons based on type
        match msg_type {
            MessageBoxType::Info | MessageBoxType::Warning | MessageBoxType::Error => {
                dialog = dialog.add_button("OK", DialogResult::Ok);
            }
            MessageBoxType::Question => {
                dialog = dialog.add_button("Yes", DialogResult::Yes);
                dialog = dialog.add_button("No", DialogResult::No);
            }
        }
        
        Self {
            dialog,
            message: String::from(message),
            msg_type,
        }
    }

    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        self.dialog.show(screen_width, screen_height);
    }

    pub fn hide(&mut self) {
        self.dialog.hide();
    }

    pub fn is_visible(&self) -> bool {
        self.dialog.is_visible()
    }

    pub fn result(&self) -> DialogResult {
        self.dialog.result()
    }

    fn icon_char(&self) -> &'static str {
        match self.msg_type {
            MessageBoxType::Info => "i",
            MessageBoxType::Warning => "!",
            MessageBoxType::Error => "X",
            MessageBoxType::Question => "?",
        }
    }

    fn icon_color(&self) -> u32 {
        match self.msg_type {
            MessageBoxType::Info => Theme::ACCENT_PRIMARY.to_u32(),
            MessageBoxType::Warning => Theme::ACCENT_WARNING.to_u32(),
            MessageBoxType::Error => Theme::ACCENT_ERROR.to_u32(),
            MessageBoxType::Question => Theme::TEXT_ACCENT.to_u32(),
        }
    }
}

impl Widget for MessageBox {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.dialog.is_visible() {
            return;
        }

        // Draw dialog
        self.dialog.draw(fb);

        // Draw icon
        let icon_x = self.dialog.rect.x + 20;
        let icon_y = self.dialog.rect.y + 50;
        fb.draw_rect(icon_x as usize, icon_y as usize, 32, 32, self.icon_color());
        fb.draw_string(icon_x as usize + 12, icon_y as usize + 8, self.icon_char(), Theme::TEXT_PRIMARY.to_u32());

        // Draw message
        let msg_x = self.dialog.rect.x + 65;
        let msg_y = self.dialog.rect.y + 50;
        
        // Word wrap message
        let max_width = self.dialog.rect.width - 85;
        let mut line_y = msg_y;
        for line in self.message.split('\n') {
            if line.len() * 8 > max_width as usize {
                // Need to wrap
                let mut start = 0;
                while start < line.len() {
                    let end = (start + (max_width as usize / 8)).min(line.len());
                    fb.draw_string(msg_x as usize, line_y as usize, &line[start..end], Theme::TEXT_PRIMARY.to_u32());
                    line_y += 18;
                    start = end;
                }
            } else {
                fb.draw_string(msg_x as usize, line_y as usize, line, Theme::TEXT_PRIMARY.to_u32());
                line_y += 18;
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        self.dialog.on_click(x, y)
    }

    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        self.dialog.on_drag(dx, dy)
    }

    fn bounds(&self) -> Rect {
        self.dialog.bounds()
    }
}

/// File dialog type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogType {
    Open,
    Save,
    SelectFolder,
}

/// FileDialog widget (simplified - no actual filesystem integration)
pub struct FileDialog {
    dialog: Dialog<'static>,
    dialog_type: FileDialogType,
    current_path: String,
    files: Vec<String>,
    selected_file: Option<usize>,
    filename_input: String,
}

impl FileDialog {
    pub fn new(dialog_type: FileDialogType) -> Self {
        let title = match dialog_type {
            FileDialogType::Open => "Open File",
            FileDialogType::Save => "Save File",
            FileDialogType::SelectFolder => "Select Folder",
        };
        
        let mut dialog = Dialog::new(title, 500, 400);
        dialog = dialog.add_button("Cancel", DialogResult::Cancel);
        dialog = dialog.add_button("Open", DialogResult::Ok);
        
        Self {
            dialog,
            dialog_type,
            current_path: String::from("/"),
            files: Vec::new(),
            selected_file: None,
            filename_input: String::new(),
        }
    }

    pub fn set_path(&mut self, path: &str) {
        self.current_path = String::from(path);
    }

    pub fn set_files(&mut self, files: Vec<String>) {
        self.files = files;
    }

    pub fn selected_file(&self) -> Option<&str> {
        self.selected_file.and_then(|i| self.files.get(i)).map(|s| s.as_str())
    }

    pub fn filename(&self) -> &str {
        &self.filename_input
    }

    pub fn show(&mut self, screen_width: usize, screen_height: usize) {
        self.dialog.show(screen_width, screen_height);
    }

    pub fn hide(&mut self) {
        self.dialog.hide();
    }

    pub fn is_visible(&self) -> bool {
        self.dialog.is_visible()
    }

    pub fn result(&self) -> DialogResult {
        self.dialog.result()
    }

    fn file_list_rect(&self) -> Rect {
        Rect::new(
            self.dialog.rect.x + 10,
            self.dialog.rect.y + 60,
            self.dialog.rect.width - 20,
            250,
        )
    }

    fn filename_rect(&self) -> Rect {
        Rect::new(
            self.dialog.rect.x + 100,
            self.dialog.rect.y + self.dialog.rect.height - 60,
            self.dialog.rect.width - 110,
            24,
        )
    }
}

impl Widget for FileDialog {
    fn draw(&self, fb: &mut Framebuffer) {
        if !self.dialog.is_visible() {
            return;
        }

        self.dialog.draw(fb);

        // Path bar
        let path_y = self.dialog.rect.y + 35;
        fb.draw_rect(
            self.dialog.rect.x as usize + 10,
            path_y as usize,
            (self.dialog.rect.width - 20) as usize,
            20,
            Theme::BUTTON_BG.to_u32(),
        );
        fb.draw_string(
            self.dialog.rect.x as usize + 15,
            path_y as usize + 2,
            &self.current_path,
            Theme::TEXT_SECONDARY.to_u32(),
        );

        // File list
        let list_rect = self.file_list_rect();
        fb.draw_rect(
            list_rect.x as usize,
            list_rect.y as usize,
            list_rect.width as usize,
            list_rect.height as usize,
            Theme::BUTTON_BG.to_u32(),
        );
        
        // Draw files
        let mut file_y = list_rect.y + 5;
        for (i, file) in self.files.iter().enumerate() {
            if file_y + 18 > list_rect.y + list_rect.height {
                break;
            }
            
            let bg_color = if self.selected_file == Some(i) {
                Theme::ACCENT_PRIMARY.to_u32()
            } else {
                Theme::BUTTON_BG.to_u32()
            };
            
            fb.draw_rect(
                list_rect.x as usize + 2,
                file_y as usize,
                (list_rect.width - 4) as usize,
                18,
                bg_color,
            );
            
            let text_color = if self.selected_file == Some(i) {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            fb.draw_string(list_rect.x as usize + 5, file_y as usize + 1, file, text_color);
            
            file_y += 20;
        }

        // Filename label
        fb.draw_string(
            self.dialog.rect.x as usize + 10,
            self.dialog.rect.y as usize + self.dialog.rect.height as usize - 55,
            "Filename:",
            Theme::TEXT_PRIMARY.to_u32(),
        );

        // Filename input
        let filename_rect = self.filename_rect();
        fb.draw_rect(
            filename_rect.x as usize,
            filename_rect.y as usize,
            filename_rect.width as usize,
            filename_rect.height as usize,
            Theme::WINDOW_BG.to_u32(),
        );
        fb.draw_string(
            filename_rect.x as usize + 5,
            filename_rect.y as usize + 4,
            &self.filename_input,
            Theme::TEXT_PRIMARY.to_u32(),
        );
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.dialog.is_visible() {
            return false;
        }

        // Check file list
        let list_rect = self.file_list_rect();
        if list_rect.contains(x, y) {
            let relative_y = y - list_rect.y - 5;
            let index = (relative_y / 20) as usize;
            if index < self.files.len() {
                self.selected_file = Some(index);
                self.filename_input = self.files[index].clone();
            }
            return true;
        }

        self.dialog.on_click(x, y)
    }

    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        if !self.dialog.is_visible() {
            return false;
        }

        // Handle filename input
        if self.dialog.rect.contains(self.filename_rect().x, self.filename_rect().y) {
            if scancode == 0x0E && !self.filename_input.is_empty() {
                // Backspace
                self.filename_input.pop();
                return true;
            } else if key != '\0' && (modifiers & MOD_CTRL) == 0 {
                self.filename_input.push(key);
                return true;
            }
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.dialog.bounds()
    }
}
