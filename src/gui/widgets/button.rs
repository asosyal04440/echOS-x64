use super::{
    draw_render_objects, AccessRole, AccessState, AccessibilityInfo, FocusPolicy, Rect, Widget,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject, RenderObjectKind, TextRunStyle};
use crate::gui::theme::{ButtonRole, Theme, ThemeMode};
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

pub struct Button<'a> {
    rect: Rect,
    text: &'a str,
    role: ButtonRole,
    hovered: bool,
    pressed: bool,
    enabled: bool,
    focused: bool,
    on_click_fn: Option<fn()>,
}

impl<'a> Button<'a> {
    pub fn new(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self {
            rect: Rect::new(
                x,
                y,
                width.max(Theme::MIN_HIT_WIDTH),
                height.max(Theme::MIN_HIT_HEIGHT),
            ),
            text,
            role: ButtonRole::Secondary,
            hovered: false,
            pressed: false,
            enabled: true,
            focused: false,
            on_click_fn: None,
        }
    }

    pub fn primary(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self::new(x, y, width, height, text).with_role(ButtonRole::Primary)
    }

    pub fn tertiary(x: i32, y: i32, width: i32, height: i32, text: &'a str) -> Self {
        Self::new(x, y, width, height, text).with_role(ButtonRole::Tertiary)
    }

    pub fn with_role(mut self, role: ButtonRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_on_click(mut self, cb: fn()) -> Self {
        self.on_click_fn = Some(cb);
        self
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl<'a> Widget for Button<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        draw_render_objects(fb, self.bounds(), &self.render_objects());
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.enabled || !self.rect.contains(x, y) {
            return false;
        }
        if let Some(cb) = self.on_click_fn {
            cb();
        }
        self.pressed = !self.pressed;
        true
    }

    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let previous = self.hovered;
        self.hovered = self.rect.contains(x, y);
        previous != self.hovered
    }

    fn on_key(&mut self, key: char, _modifiers: u8, _scancode: u8) -> bool {
        if !self.enabled || !self.focused {
            return false;
        }
        if key == '\n' || key == ' ' {
            if let Some(cb) = self.on_click_fn {
                cb();
            }
            self.pressed = !self.pressed;
            return true;
        }
        false
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

    fn can_focus(&self) -> bool {
        self.enabled
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.enabled {
            FocusPolicy::Strong
        } else {
            FocusPolicy::None
        }
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.focused {
            state = state.with(AccessState::FOCUSED);
        }
        if !self.enabled {
            state = state.with(AccessState::DISABLED);
        }
        AccessibilityInfo {
            role: AccessRole::Button,
            label: self.text,
            value: if self.pressed { "pressed" } else { "" },
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        let mode = ThemeMode::Dark;
        let fill = if !self.enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else {
            Theme::button_fill(self.role, mode, self.pressed, self.hovered)
        };
        let border = if self.focused {
            Theme::INPUT_FOCUS.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        let text_c = if !self.enabled {
            Theme::TEXT_DISABLED.to_u32()
        } else {
            Theme::button_text(self.role, mode)
        };
        let bounds = crate::gui::protocol::Rect::new(
            self.rect.x,
            self.rect.y,
            self.rect.width.max(0) as u32,
            self.rect.height.max(0) as u32,
        );
        let text_bounds = crate::gui::protocol::Rect::new(
            self.rect.x + 10,
            self.rect.y + ((self.rect.height - 18).max(0) / 2),
            (self.rect.width - 20).max(1) as u32,
            18,
        );

        vec![
            RenderObject {
                object_id: ((self.rect.x as u64) << 32) ^ self.rect.y as u64,
                bounds,
                clip: None,
                z_index: 0,
                opacity: u8::MAX,
                lane: DamageLane::Window,
                kind: RenderObjectKind::SolidRect {
                    color: fill,
                    corner_radius: 6,
                },
            },
            RenderObject {
                object_id: 0x1000_0000_0000_0000u64
                    ^ (((self.rect.x as u64) << 32) ^ self.rect.y as u64),
                bounds: crate::gui::protocol::Rect::new(self.rect.x, self.rect.y, bounds.width, 1),
                clip: None,
                z_index: 1,
                opacity: u8::MAX,
                lane: DamageLane::Window,
                kind: RenderObjectKind::SolidRect {
                    color: border,
                    corner_radius: 0,
                },
            },
            RenderObject {
                object_id: 0x2000_0000_0000_0000u64
                    ^ (((self.rect.x as u64) << 32) ^ self.rect.y as u64),
                bounds: text_bounds,
                clip: None,
                z_index: 2,
                opacity: u8::MAX,
                lane: DamageLane::Text,
                kind: RenderObjectKind::TextRun {
                    blob_id: 0,
                    text: self.text.to_string(),
                    color: text_c,
                    style: TextRunStyle::Ui,
                    max_width: text_bounds.width.max(1),
                },
            },
        ]
    }
}
