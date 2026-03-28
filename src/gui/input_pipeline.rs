//! Input pipeline: jitter filtreleme, ivme eğrisi ve kinetik scroll.

use crate::gui::protocol::Point;
use crate::gui::scroll_physics::ScrollMomentum;
use crate::gui::widgets::{
    ElementId, EventPhase, EventResult, FocusPolicy, RenderBox, WidgetEvent,
};
use alloc::vec;
use alloc::vec::Vec;
use libm::{powf, roundf};

pub struct InputPipeline {
    pointer_deadzone: i32,
    pointer_accel: f32,
    max_delta: i32,
    scroll_momentum: ScrollMomentum,
}

impl InputPipeline {
    pub fn new() -> Self {
        Self {
            pointer_deadzone: 1,
            pointer_accel: 1.12,
            max_delta: 256,
            scroll_momentum: ScrollMomentum::new(),
        }
    }

    pub fn filter_pointer_delta(&self, raw: Point) -> Point {
        let mut dx = if raw.x.abs() <= self.pointer_deadzone {
            0.0
        } else {
            raw.x as f32
        };
        let mut dy = if raw.y.abs() <= self.pointer_deadzone {
            0.0
        } else {
            raw.y as f32
        };

        dx = dx.signum() * powf(dx.abs(), self.pointer_accel);
        dy = dy.signum() * powf(dy.abs(), self.pointer_accel);

        Point::new(
            (roundf(dx) as i32).clamp(-self.max_delta, self.max_delta),
            (roundf(dy) as i32).clamp(-self.max_delta, self.max_delta),
        )
    }

    pub fn feed_scroll_notch(&mut self, raw_notch: i32) -> i32 {
        if raw_notch == 0 {
            return 0;
        }
        self.scroll_momentum.add(0.0, raw_notch as f32);
        raw_notch.clamp(-16, 16)
    }

    pub fn poll_kinetic_scroll(&mut self, dt_sec: f32) -> Option<Point> {
        let (dx, dy) = self.scroll_momentum.update(dt_sec);
        let px = roundf(dx) as i32;
        let py = roundf(dy) as i32;
        if px == 0 && py == 0 {
            return None;
        }
        Some(Point::new(px, py))
    }
}

impl Default for InputPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureKind {
    Tap,
    Drag,
    Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerCaptureToken {
    pub element_id: ElementId,
}

#[derive(Clone, Debug, Default)]
pub struct FocusTree {
    ordered: Vec<ElementId>,
    focused: Option<ElementId>,
}

impl FocusTree {
    pub fn rebuild(&mut self, render_boxes: &[RenderBox]) {
        self.ordered = render_boxes
            .iter()
            .filter(|entry| entry.focus_policy != FocusPolicy::None)
            .map(|entry| entry.element_id)
            .collect();
        if self
            .focused
            .map(|id| self.ordered.iter().any(|candidate| *candidate == id))
            != Some(true)
        {
            self.focused = self.ordered.first().copied();
        }
    }

    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    pub fn set_focused(&mut self, element_id: Option<ElementId>) {
        self.focused = element_id;
    }

    pub fn advance(&mut self) -> Option<ElementId> {
        let Some(current) = self.focused else {
            self.focused = self.ordered.first().copied();
            return self.focused;
        };
        if self.ordered.is_empty() {
            return None;
        }
        let index = self
            .ordered
            .iter()
            .position(|candidate| *candidate == current)
            .unwrap_or(0);
        let next = self.ordered.get((index + 1) % self.ordered.len()).copied();
        self.focused = next;
        next
    }
}

#[derive(Clone, Debug, Default)]
pub struct GestureArena {
    active: Option<(ElementId, GestureKind)>,
}

impl GestureArena {
    pub fn claim(&mut self, element_id: ElementId, kind: GestureKind) -> bool {
        if self.active.is_none() || self.active == Some((element_id, kind)) {
            self.active = Some((element_id, kind));
            true
        } else {
            false
        }
    }

    pub fn active(&self) -> Option<(ElementId, GestureKind)> {
        self.active
    }

    pub fn clear(&mut self) {
        self.active = None;
    }
}

#[derive(Clone, Debug, Default)]
pub struct InputRoute {
    pub capture: Vec<ElementId>,
    pub target: Option<ElementId>,
    pub bubble: Vec<ElementId>,
}

pub struct InputRouter {
    capture: Option<PointerCaptureToken>,
}

impl InputRouter {
    pub fn new() -> Self {
        Self { capture: None }
    }

    pub fn pointer_capture(&self) -> Option<PointerCaptureToken> {
        self.capture
    }

    pub fn route_pointer(&self, render_boxes: &[RenderBox], point: Point) -> InputRoute {
        if let Some(capture) = self.capture {
            return InputRoute {
                capture: vec![capture.element_id],
                target: Some(capture.element_id),
                bubble: vec![capture.element_id],
            };
        }

        let target = render_boxes
            .iter()
            .rev()
            .find(|entry| entry.bounds.contains(point.x, point.y))
            .map(|entry| entry.element_id);
        InputRoute {
            capture: target.into_iter().collect(),
            target,
            bubble: target.into_iter().collect(),
        }
    }

    pub fn dispatch(
        &mut self,
        render_boxes: &[RenderBox],
        point: Point,
        event: WidgetEvent,
        mut handler: impl FnMut(ElementId, EventPhase, WidgetEvent) -> EventResult,
    ) -> EventResult {
        let route = self.route_pointer(render_boxes, point);
        let mut result = EventResult::default();
        for element_id in route.capture.iter().copied() {
            result = merge_event_results(result, handler(element_id, EventPhase::Capture, event));
        }
        if let Some(target) = route.target {
            result = merge_event_results(result, handler(target, EventPhase::Target, event));
        }
        for element_id in route.bubble.iter().copied() {
            result = merge_event_results(result, handler(element_id, EventPhase::Bubble, event));
        }
        if result.capture_pointer {
            self.capture = route
                .target
                .map(|element_id| PointerCaptureToken { element_id });
        }
        if result.release_pointer {
            self.capture = None;
        }
        result
    }
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn merge_event_results(mut left: EventResult, right: EventResult) -> EventResult {
    left.handled |= right.handled;
    left.request_focus |= right.request_focus;
    left.capture_pointer |= right.capture_pointer;
    left.release_pointer |= right.release_pointer;
    left.needs_redraw |= right.needs_redraw;
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::widgets::Rect;

    fn render_box(element_id: ElementId, x: i32, y: i32) -> RenderBox {
        RenderBox {
            element_id,
            widget_id: element_id,
            bounds: Rect::new(x, y, 32, 32),
            z_index: element_id as u32,
            focus_policy: FocusPolicy::Click,
            render_objects: Vec::new(),
            semantics: Vec::new(),
            children: Vec::new(),
        }
    }

    #[test]
    fn focus_tree_cycles_focus_order() {
        let mut focus = FocusTree::default();
        focus.rebuild(&[render_box(1, 0, 0), render_box(2, 40, 0)]);
        assert_eq!(focus.focused(), Some(1));
        assert_eq!(focus.advance(), Some(2));
        assert_eq!(focus.advance(), Some(1));
    }

    #[test]
    fn input_router_keeps_pointer_capture_until_release() {
        let mut router = InputRouter::new();
        let render_boxes = [render_box(1, 0, 0), render_box(2, 8, 8)];
        let down = router.dispatch(
            &render_boxes,
            Point::new(12, 12),
            WidgetEvent::PointerDown(Point::new(12, 12)),
            |_, phase, _| EventResult {
                handled: true,
                capture_pointer: phase == EventPhase::Target,
                ..EventResult::default()
            },
        );
        assert!(down.capture_pointer);
        assert_eq!(
            router.pointer_capture(),
            Some(PointerCaptureToken { element_id: 2 })
        );

        let moved = router.route_pointer(&render_boxes, Point::new(1, 1));
        assert_eq!(moved.target, Some(2));

        let up = router.dispatch(
            &render_boxes,
            Point::new(1, 1),
            WidgetEvent::PointerUp(Point::new(1, 1)),
            |_, _, _| EventResult {
                release_pointer: true,
                ..EventResult::default()
            },
        );
        assert!(up.release_pointer);
        assert_eq!(router.pointer_capture(), None);
    }
}
