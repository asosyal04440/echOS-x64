//! Dirty region tracker for week-2 partial redraws.

use crate::gui::protocol::Rect;
use alloc::vec::Vec;

const MAX_DAMAGE_REGIONS: usize = 24;

pub struct DamageTracker {
    full_redraw: bool,
    regions: Vec<Rect>,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self {
            full_redraw: true,
            regions: Vec::new(),
        }
    }

    pub fn mark_full(&mut self) {
        self.full_redraw = true;
        self.regions.clear();
    }

    pub fn mark_rect(&mut self, rect: Rect) {
        if rect.is_empty() || self.full_redraw {
            return;
        }

        self.regions.push(rect);
        self.normalize();
    }

    pub fn mark_rects(&mut self, rects: &[Rect]) {
        for rect in rects {
            self.mark_rect(*rect);
            if self.full_redraw {
                return;
            }
        }
    }

    pub fn take(&mut self, screen: Rect) -> Vec<Rect> {
        if self.full_redraw {
            self.full_redraw = false;
            self.regions.clear();
            return alloc::vec![screen];
        }

        let mut out = core::mem::take(&mut self.regions);
        out.retain(|rect| !rect.is_empty());
        out
    }

    pub fn has_damage(&self) -> bool {
        self.full_redraw || !self.regions.is_empty()
    }

    fn normalize(&mut self) {
        if self.regions.len() > MAX_DAMAGE_REGIONS {
            self.mark_full();
            return;
        }

        let mut index = 0;
        while index < self.regions.len() {
            let mut candidate = self.regions[index];
            let mut merged = false;
            let mut other = index + 1;
            while other < self.regions.len() {
                if touches(candidate, self.regions[other]) {
                    candidate = candidate.union(&self.regions[other]);
                    self.regions.swap_remove(other);
                    merged = true;
                } else {
                    other += 1;
                }
            }

            self.regions[index] = candidate;
            if !merged {
                index += 1;
            }
        }
    }
}

fn touches(a: Rect, b: Rect) -> bool {
    let expanded = Rect::new(
        a.x.saturating_sub(1),
        a.y.saturating_sub(1),
        a.width.saturating_add(2),
        a.height.saturating_add(2),
    );
    expanded.intersects(&b) || a.intersects(&b)
}
