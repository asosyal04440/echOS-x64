use crate::gui::protocol::{
    FrameTicket, InvalidationReason, InvalidationTarget, SceneInvalidation,
};
use alloc::vec::Vec;

const TARGET_COUNT: usize = 14;

fn target_index(target: InvalidationTarget) -> usize {
    match target {
        InvalidationTarget::TopBar => 0,
        InvalidationTarget::Dock => 1,
        InvalidationTarget::Launcher => 2,
        InvalidationTarget::Overview => 3,
        InvalidationTarget::QuickSettings => 4,
        InvalidationTarget::CommandPalette => 5,
        InvalidationTarget::NotificationCenter => 6,
        InvalidationTarget::Dialog => 7,
        InvalidationTarget::ContextMenu => 8,
        InvalidationTarget::Switcher => 9,
        InvalidationTarget::LockScreen => 10,
        InvalidationTarget::WorkspaceViewport => 11,
        InvalidationTarget::Cursor => 12,
        InvalidationTarget::Wallpaper => 13,
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShellInvalidationState {
    pending_mask: u16,
    pending: Vec<SceneInvalidation>,
    next_frame_ticket: FrameTicket,
}

#[derive(Clone, Debug)]
pub struct ShellFramePlan {
    pub frame_ticket: FrameTicket,
    pub pending: Vec<SceneInvalidation>,
    mask: u16,
}

impl ShellInvalidationState {
    pub fn new() -> Self {
        Self {
            pending_mask: 0,
            pending: Vec::new(),
            next_frame_ticket: 1,
        }
    }

    pub fn bootstrap_shell() -> Self {
        let mut state = Self::new();
        for target in [
            InvalidationTarget::TopBar,
            InvalidationTarget::Dock,
            InvalidationTarget::Launcher,
            InvalidationTarget::Overview,
            InvalidationTarget::QuickSettings,
            InvalidationTarget::CommandPalette,
            InvalidationTarget::NotificationCenter,
            InvalidationTarget::Dialog,
            InvalidationTarget::ContextMenu,
            InvalidationTarget::Switcher,
            InvalidationTarget::LockScreen,
            InvalidationTarget::WorkspaceViewport,
            InvalidationTarget::Wallpaper,
        ] {
            state.mark(target, InvalidationReason::StateChanged);
        }
        state
    }

    pub fn mark(&mut self, target: InvalidationTarget, reason: InvalidationReason) {
        let bit = 1u16 << target_index(target).min(TARGET_COUNT - 1);
        self.pending_mask |= bit;
        if !self
            .pending
            .iter()
            .any(|entry| entry.target == target && entry.reason == reason)
        {
            self.pending.push(SceneInvalidation { target, reason });
        }
    }

    pub fn mark_many(&mut self, targets: &[InvalidationTarget], reason: InvalidationReason) {
        for target in targets.iter().copied() {
            self.mark(target, reason);
        }
    }

    pub fn take_frame_plan(&mut self) -> Option<ShellFramePlan> {
        if self.pending_mask == 0 {
            return None;
        }
        let frame_ticket = self.next_frame_ticket;
        self.next_frame_ticket = self.next_frame_ticket.saturating_add(1);
        Some(ShellFramePlan {
            frame_ticket,
            pending: core::mem::take(&mut self.pending),
            mask: core::mem::take(&mut self.pending_mask),
        })
    }
}

impl ShellFramePlan {
    pub fn touches(&self, target: InvalidationTarget) -> bool {
        let bit = 1u16 << target_index(target).min(TARGET_COUNT - 1);
        self.mask & bit != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidation_coalesces_targets_and_advances_frame_ticket() {
        let mut state = ShellInvalidationState::new();
        state.mark(InvalidationTarget::TopBar, InvalidationReason::StateChanged);
        state.mark(InvalidationTarget::TopBar, InvalidationReason::FocusChanged);
        state.mark(
            InvalidationTarget::QuickSettings,
            InvalidationReason::AnimationAdvanced,
        );

        let frame = state.take_frame_plan().expect("expected frame plan");
        assert_eq!(frame.frame_ticket, 1);
        assert!(frame.touches(InvalidationTarget::TopBar));
        assert!(frame.touches(InvalidationTarget::QuickSettings));
        assert_eq!(frame.pending.len(), 3);
        assert!(state.take_frame_plan().is_none());
    }
}
