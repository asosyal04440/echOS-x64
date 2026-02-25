//! # echOS ScrollBar and Slider Widgets
//!
//! Scroll and value adjustment widgets.

use super::{Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;

/// ScrollBar orientation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// ScrollBar widget
pub struct ScrollBar {
    rect: Rect,
    orientation: Orientation,
    value: usize,
    max_value: usize,
    page_size: usize,
    dragging: bool,
    drag_start: i32,
    hovered: bool,
    on_change: Option<fn(usize)>,
}

impl ScrollBar {
    pub fn new(x: i32, y: i32, width: i32, height: i32, orientation: Orientation) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            orientation,
            value: 0,
            max_value: 100,
            page_size: 10,
            dragging: false,
            drag_start: 0,
            hovered: false,
            on_change: None,
        }
    }

    pub fn with_range(mut self, max: usize, page: usize) -> Self {
        self.max_value = max;
        self.page_size = page;
        self
    }

    pub fn set_value(&mut self, value: usize) {
        self.value = value.min(self.max_value.saturating_sub(self.page_size));
    }

    pub fn value(&self) -> usize {
        self.value
    }

    pub fn with_change_handler(mut self, handler: fn(usize)) -> Self {
        self.on_change = Some(handler);
        self
    }

    fn thumb_size(&self) -> i32 {
        let (track_size, content_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.max_value),
            Orientation::Vertical => (self.rect.height, self.max_value),
        };
        
        if content_size == 0 {
            return track_size;
        }
        
        let thumb = (track_size as usize * self.page_size / content_size).max(20) as i32;
        thumb.min(track_size)
    }

    fn thumb_position(&self) -> i32 {
        let (track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.thumb_size()),
            Orientation::Vertical => (self.rect.height, self.thumb_size()),
        };
        
        let track_range = track_size - thumb_size;
        if self.max_value <= self.page_size {
            return 0;
        }
        
        let max_scroll = self.max_value - self.page_size;
        if max_scroll == 0 {
            return 0;
        }
        
        (track_range as usize * self.value / max_scroll) as i32
    }

    fn thumb_rect(&self) -> Rect {
        let thumb_size = self.thumb_size();
        let thumb_pos = self.thumb_position();
        
        match self.orientation {
            Orientation::Horizontal => Rect::new(
                self.rect.x + thumb_pos,
                self.rect.y,
                thumb_size,
                self.rect.height,
            ),
            Orientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + thumb_pos,
                self.rect.width,
                thumb_size,
            ),
        }
    }

    fn value_from_position(&self, pos: i32) -> usize {
        let (track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => (self.rect.width, self.thumb_size()),
            Orientation::Vertical => (self.rect.height, self.thumb_size()),
        };
        
        let track_range = (track_size - thumb_size) as usize;
        if track_range == 0 {
            return 0;
        }
        
        let max_scroll = self.max_value.saturating_sub(self.page_size);
        let relative_pos = (pos as usize).min(track_range);
        relative_pos * max_scroll / track_range
    }
}

impl Widget for ScrollBar {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;

        // Track background
        fb.draw_rect(x, y, w, h, Theme::BUTTON_BG.to_u32());

        // Thumb
        let thumb = self.thumb_rect();
        let thumb_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::TEXT_SECONDARY.to_u32()
        };
        
        fb.draw_rect(
            thumb.x as usize,
            thumb.y as usize,
            thumb.width as usize,
            thumb.height as usize,
            thumb_color,
        );

        // Arrow buttons
        match self.orientation {
            Orientation::Horizontal => {
                // Left arrow
                fb.draw_rect(x, y, 16, h, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + 4, y + (h - 16) / 2, "<", Theme::TEXT_PRIMARY.to_u32());
                // Right arrow
                fb.draw_rect(x + w - 16, y, 16, h, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + w - 12, y + (h - 16) / 2, ">", Theme::TEXT_PRIMARY.to_u32());
            }
            Orientation::Vertical => {
                // Up arrow
                fb.draw_rect(x, y, w, 16, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + (w - 8) / 2, y + 2, "^", Theme::TEXT_PRIMARY.to_u32());
                // Down arrow
                fb.draw_rect(x, y + h - 16, w, 16, Theme::TITLEBAR_BG.to_u32());
                fb.draw_string(x + (w - 8) / 2, y + h - 14, "v", Theme::TEXT_PRIMARY.to_u32());
            }
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            self.dragging = false;
            return false;
        }

        let thumb = self.thumb_rect();
        
        // Check if clicked on thumb
        if thumb.contains(x, y) {
            self.dragging = true;
            match self.orientation {
                Orientation::Horizontal => self.drag_start = x - thumb.x,
                Orientation::Vertical => self.drag_start = y - thumb.y,
            }
            return true;
        }

        // Check arrow buttons
        match self.orientation {
            Orientation::Horizontal => {
                if x < self.rect.x + 16 {
                    // Left arrow - scroll up
                    if self.value > 0 {
                        self.value -= 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else if x >= self.rect.x + self.rect.width - 16 {
                    // Right arrow - scroll down
                    if self.value < self.max_value.saturating_sub(self.page_size) {
                        self.value += 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else {
                    // Track click - page up/down
                    let thumb_center = self.thumb_position() + self.thumb_size() / 2;
                    let click_pos = x - self.rect.x;
                    if click_pos < thumb_center {
                        self.value = self.value.saturating_sub(self.page_size);
                    } else {
                        self.value = (self.value + self.page_size).min(
                            self.max_value.saturating_sub(self.page_size)
                        );
                    }
                    if let Some(handler) = self.on_change {
                        handler(self.value);
                    }
                }
            }
            Orientation::Vertical => {
                if y < self.rect.y + 16 {
                    // Up arrow
                    if self.value > 0 {
                        self.value -= 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else if y >= self.rect.y + self.rect.height - 16 {
                    // Down arrow
                    if self.value < self.max_value.saturating_sub(self.page_size) {
                        self.value += 1;
                        if let Some(handler) = self.on_change {
                            handler(self.value);
                        }
                    }
                } else {
                    // Track click
                    let thumb_center = self.thumb_position() + self.thumb_size() / 2;
                    let click_pos = y - self.rect.y;
                    if click_pos < thumb_center {
                        self.value = self.value.saturating_sub(self.page_size);
                    } else {
                        self.value = (self.value + self.page_size).min(
                            self.max_value.saturating_sub(self.page_size)
                        );
                    }
                    if let Some(handler) = self.on_change {
                        handler(self.value);
                    }
                }
            }
        }
        true
    }

    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if !self.dragging {
            return false;
        }

        let (delta, track_size, thumb_size) = match self.orientation {
            Orientation::Horizontal => {
                let thumb = self.thumb_rect();
                let new_pos = (thumb.x + dx - self.rect.x).max(0);
                (new_pos, self.rect.width, self.thumb_size())
            }
            Orientation::Vertical => {
                let thumb = self.thumb_rect();
                let new_pos = (thumb.y + dy - self.rect.y).max(0);
                (new_pos, self.rect.height, self.thumb_size())
            }
        };

        let new_value = self.value_from_position(delta);
        if new_value != self.value {
            self.value = new_value;
            if let Some(handler) = self.on_change {
                handler(self.value);
            }
        }
        true
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

/// Slider widget (value selector)
pub struct Slider {
    rect: Rect,
    value: i32,
    min_value: i32,
    max_value: i32,
    step: i32,
    dragging: bool,
    hovered: bool,
    on_change: Option<fn(i32)>,
}

impl Slider {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            value: 0,
            min_value: 0,
            max_value: 100,
            step: 1,
            dragging: false,
            hovered: false,
            on_change: None,
        }
    }

    pub fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min_value = min;
        self.max_value = max;
        self.value = self.value.max(min).min(max);
        self
    }

    pub fn with_step(mut self, step: i32) -> Self {
        self.step = step;
        self
    }

    pub fn set_value(&mut self, value: i32) {
        self.value = value.max(self.min_value).min(self.max_value);
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn with_change_handler(mut self, handler: fn(i32)) -> Self {
        self.on_change = Some(handler);
        self
    }

    fn thumb_position(&self) -> i32 {
        let range = self.max_value - self.min_value;
        if range == 0 {
            return 0;
        }
        let track_width = self.rect.width - 20; // 10px padding each side
        (track_width as i64 * (self.value - self.min_value) as i64 / range as i64) as i32
    }

    fn value_from_position(&self, x: i32) -> i32 {
        let track_width = self.rect.width - 20;
        let relative_x = (x - self.rect.x - 10).max(0);
        let range = self.max_value - self.min_value;
        
        let value = self.min_value + (range as i64 * relative_x as i64 / track_width as i64) as i32;
        
        // Snap to step
        if self.step > 0 {
            ((value - self.min_value + self.step / 2) / self.step) * self.step + self.min_value
        } else {
            value
        }.max(self.min_value).min(self.max_value)
    }
}

impl Widget for Slider {
    fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        let track_y = y + h / 2 - 4;

        // Track background
        fb.draw_rect(x + 10, track_y, w - 20, 8, Theme::BUTTON_BG.to_u32());

        // Filled portion
        let thumb_x = x + 10 + self.thumb_position() as usize;
        fb.draw_rect(x + 10, track_y, thumb_x - x - 10, 8, Theme::ACCENT_PRIMARY.to_u32());

        // Thumb
        let thumb_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else if self.hovered {
            Theme::BUTTON_HOVER.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };
        
        fb.draw_rect(thumb_x - 8, y + 2, 16, h as usize - 4, thumb_color);

        // Value label
        let value_str = alloc::format!("{}", self.value);
        let label_x = x + w - value_str.len() * 8 - 5;
        fb.draw_string(label_x, y + (h - 16) / 2, &value_str, Theme::TEXT_SECONDARY.to_u32());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            self.dragging = true;
            let new_value = self.value_from_position(x);
            if new_value != self.value {
                self.value = new_value;
                if let Some(handler) = self.on_change {
                    handler(self.value);
                }
            }
            true
        } else {
            false
        }
    }

    fn on_drag(&mut self, dx: i32, _dy: i32) -> bool {
        if !self.dragging {
            return false;
        }
        
        // Recalculate value from current thumb position + delta
        let thumb_x = self.rect.x + 10 + self.thumb_position();
        let new_x = thumb_x + dx;
        let new_value = self.value_from_position(new_x);
        
        if new_value != self.value {
            self.value = new_value;
            if let Some(handler) = self.on_change {
                handler(self.value);
            }
        }
        true
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
