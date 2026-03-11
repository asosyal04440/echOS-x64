//! Input pipeline: jitter filtreleme, ivme eğrisi ve kinetik scroll.

use crate::gui::protocol::Point;
use crate::gui::scroll_physics::ScrollMomentum;
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
