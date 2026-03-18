use super::{draw_render_objects, Rect, Widget};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject, RenderObjectKind, TextRunStyle};
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

pub struct Label {
    rect: Rect,
    text: String,
    color: u32,
}

impl Label {
    pub fn new(x: i32, y: i32, text: &str) -> Self {
        Self {
            rect: Rect::new(x, y, (text.len() * 8) as i32, 18),
            text: text.to_string(),
            color: Theme::TEXT_PRIMARY.to_u32(),
        }
    }

    pub fn with_color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.rect.width = (self.text.len() * 8) as i32;
    }
}

impl Widget for Label {
    fn draw(&self, fb: &mut Framebuffer) {
        draw_render_objects(fb, self.bounds(), &self.render_objects());
    }

    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        vec![RenderObject {
            object_id: ((self.rect.x as u64) << 32) ^ self.rect.y as u64,
            bounds: crate::gui::protocol::Rect::new(
                self.rect.x,
                self.rect.y,
                self.rect.width.max(1) as u32,
                self.rect.height.max(1) as u32,
            ),
            clip: None,
            z_index: 0,
            opacity: u8::MAX,
            lane: DamageLane::Text,
            kind: RenderObjectKind::TextRun {
                blob_id: 0,
                text: self.text.clone(),
                color: self.color,
                style: TextRunStyle::Ui,
                max_width: self.rect.width.max(1) as u32,
            },
        }]
    }
}
