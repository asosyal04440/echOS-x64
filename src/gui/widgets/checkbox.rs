//! # echOS Checkbox and RadioButton Widgets
//!
//! Boolean selection widgets.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

/// Simple sin approximation for no_std
fn sin_approx(x: f64) -> f64 {
    // Taylor series approximation
    let x = x % (2.0 * core::f64::consts::PI);
    let mut result = 0.0;
    let mut term = x;
    for i in 1..=7 {
        result += term;
        term *= -x * x / ((2.0 * i as f64) * (2.0 * i as f64 + 1.0));
    }
    result
}

/// Simple cos approximation for no_std
fn cos_approx(x: f64) -> f64 {
    sin_approx(x + core::f64::consts::PI / 2.0)
}

/// Checkbox widget (boolean toggle)
pub struct CheckBox {
    rect: Rect,
    label: String,
    checked: bool,
    hovered: bool,
    on_toggle: Option<fn(bool)>,
}

impl CheckBox {
    pub fn new(x: i32, y: i32, label: &str) -> Self {
        Self {
            rect: Rect::new(x, y, 200, 24),
            label: String::from(label),
            checked: false,
            hovered: false,
            on_toggle: None,
        }
    }

    pub fn with_toggle_handler(mut self, handler: fn(bool)) -> Self {
        self.on_toggle = Some(handler);
        self
    }

    pub fn is_checked(&self) -> bool {
        self.checked
    }

    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
    }

    pub fn toggle(&mut self) {
        self.checked = !self.checked;
        if let Some(handler) = self.on_toggle {
            handler(self.checked);
        }
    }
}

impl Widget for CheckBox {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let box_size = 18usize;

        // Checkbox box
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, box_size, box_size, bg_color);

        // Border
        let border_color = if self.checked {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        
        for col in x..(x + box_size) {
            fb.plot_pixel(col, y, border_color);
            fb.plot_pixel(col, y + box_size - 1, border_color);
        }
        for row in y..(y + box_size) {
            fb.plot_pixel(x, row, border_color);
            fb.plot_pixel(x + box_size - 1, row, border_color);
        }

        // Checkmark
        if self.checked {
            let check_color = Theme::ACCENT_PRIMARY.to_u32();
            // Simple X pattern for checkmark
            for i in 0..6 {
                fb.plot_pixel(x + 4 + i, y + 4 + i, check_color);
                fb.plot_pixel(x + 4 + i, y + 12 - i, check_color);
            }
        }

        // Label
        fb.draw_string(x + box_size + 8, y + 3, &self.label, Theme::TEXT_PRIMARY.to_u32());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.toggle();
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// RadioButton widget (exclusive selection in a group)
pub struct RadioButton {
    rect: Rect,
    label: String,
    selected: bool,
    hovered: bool,
    group_id: u32,
    on_select: Option<fn(u32)>,
}

impl RadioButton {
    pub fn new(x: i32, y: i32, label: &str, group_id: u32) -> Self {
        Self {
            rect: Rect::new(x, y, 200, 24),
            label: String::from(label),
            selected: false,
            hovered: false,
            group_id,
            on_select: None,
        }
    }

    pub fn with_select_handler(mut self, handler: fn(u32)) -> Self {
        self.on_select = Some(handler);
        self
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    pub fn group_id(&self) -> u32 {
        self.group_id
    }

    fn select(&mut self) {
        self.selected = true;
        if let Some(handler) = self.on_select {
            handler(self.group_id);
        }
    }
}

impl Widget for RadioButton {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let circle_size = 18usize;
        let center = circle_size / 2;

        // Radio circle background
        let bg_color = if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::BUTTON_BG.to_u32()
        };
        fb.draw_rect(x, y, circle_size, circle_size, bg_color);

        // Draw circle outline
        let border_color = if self.selected {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        
        // Simple circle approximation
        for angle in 0..360 {
            let rad = (angle as f64) * core::f64::consts::PI / 180.0;
            let px = (center as f64 + 8.0 * cos_approx(rad)) as usize;
            let py = (center as f64 + 8.0 * sin_approx(rad)) as usize;
            if px < circle_size && py < circle_size {
                fb.plot_pixel(x + px, y + py, border_color);
            }
        }

        // Filled circle for selected
        if self.selected {
            let fill_color = Theme::ACCENT_PRIMARY.to_u32();
            for angle in 0..360 {
                let rad = (angle as f64) * core::f64::consts::PI / 180.0;
                let px = (center as f64 + 4.0 * cos_approx(rad)) as usize;
                let py = (center as f64 + 4.0 * sin_approx(rad)) as usize;
                if px < circle_size && py < circle_size {
                    fb.plot_pixel(x + px, y + py, fill_color);
                }
            }
        }

        // Label
        fb.draw_string(x + circle_size + 8, y + 3, &self.label, Theme::TEXT_PRIMARY.to_u32());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.select();
            true
        } else {
            false
        }
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let was_hovered = self.hovered;
        self.hovered = self.rect.contains(x, y);
        was_hovered != self.hovered
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

/// RadioGroup - manages a group of radio buttons
pub struct RadioGroup {
    buttons: Vec<RadioButton>,
    selected_index: Option<usize>,
}

impl RadioGroup {
    pub fn new() -> Self {
        Self {
            buttons: Vec::new(),
            selected_index: None,
        }
    }

    pub fn add_button(&mut self, button: RadioButton) {
        self.buttons.push(button);
    }

    pub fn select(&mut self, index: usize) {
        if index < self.buttons.len() {
            // Deselect all
            for btn in &mut self.buttons {
                btn.set_selected(false);
            }
            // Select one
            self.buttons[index].set_selected(true);
            self.selected_index = Some(index);
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn buttons(&self) -> &Vec<RadioButton> {
        &self.buttons
    }

    pub fn buttons_mut(&mut self) -> &mut Vec<RadioButton> {
        &mut self.buttons
    }
}

impl Default for RadioGroup {
    fn default() -> Self {
        Self::new()
    }
}
