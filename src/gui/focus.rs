//! Input focus registry for week-1 desktop bootstrap.

use crate::gui::protocol::AppId;
use alloc::vec::Vec;

pub struct FocusManager {
    focused: Option<AppId>,
    stack: Vec<AppId>,
}

impl FocusManager {
    pub fn new() -> Self {
        Self {
            focused: None,
            stack: Vec::new(),
        }
    }

    pub fn register_app(&mut self, app_id: AppId) {
        if !self.stack.contains(&app_id) {
            self.stack.push(app_id);
        }
        if self.focused.is_none() {
            self.focused = Some(app_id);
        }
    }

    pub fn unregister_app(&mut self, app_id: AppId) {
        self.stack.retain(|id| *id != app_id);
        if self.focused == Some(app_id) {
            self.focused = self.stack.last().copied();
        }
    }

    pub fn request_focus(&mut self, app_id: AppId) -> bool {
        if !self.stack.contains(&app_id) {
            return false;
        }
        self.stack.retain(|id| *id != app_id);
        self.stack.push(app_id);
        self.focused = Some(app_id);
        true
    }

    pub fn release_focus(&mut self, app_id: AppId) {
        if self.focused != Some(app_id) {
            return;
        }
        self.stack.retain(|id| *id != app_id);
        self.focused = self.stack.last().copied();
    }

    pub fn focused_app(&self) -> Option<AppId> {
        self.focused
    }
}
