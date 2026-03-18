//! Dirty region tracker with tile-grid compaction.

use crate::gui::protocol::Rect;
use alloc::vec;
use alloc::vec::Vec;

const TILE_SIZE: i32 = 64;
const MAX_DAMAGE_REGIONS: usize = 24;
const FULL_REDRAW_ALPHA_NUM: u32 = 78;
const FULL_REDRAW_ALPHA_DEN: u32 = 100;
const TILE_COST_WEIGHT: u32 = 4096;
const BATCH_COST_WEIGHT: u32 = 768;

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
        if self.regions.len() <= MAX_DAMAGE_REGIONS {
            self.normalize();
        }
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
            return vec![screen];
        }

        let mut out = core::mem::take(&mut self.regions);
        out.retain(|rect| !rect.is_empty());
        if out.len() <= MAX_DAMAGE_REGIONS {
            return out;
        }

        let mut compacted = tile_compact(&out, screen);
        if compacted.is_empty() {
            compacted.push(screen);
        }
        if partial_redraw_cost(&compacted) * FULL_REDRAW_ALPHA_DEN as u64
            > full_redraw_cost(screen) * FULL_REDRAW_ALPHA_NUM as u64
        {
            return vec![screen];
        }
        compacted
    }

    pub fn has_damage(&self) -> bool {
        self.full_redraw || !self.regions.is_empty()
    }

    fn normalize(&mut self) {
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

fn tile_compact(regions: &[Rect], screen: Rect) -> Vec<Rect> {
    if screen.is_empty() {
        return Vec::new();
    }

    let tiles_x = ((screen.width as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as usize;
    let tiles_y = ((screen.height as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as usize;
    let mut occupied = vec![false; tiles_x.saturating_mul(tiles_y)];

    for rect in regions.iter().filter_map(|rect| rect.intersection(&screen)) {
        let start_x = ((rect.x - screen.x).max(0) / TILE_SIZE) as usize;
        let end_x = ((rect.right() - screen.x - 1).max(0) / TILE_SIZE) as usize;
        let start_y = ((rect.y - screen.y).max(0) / TILE_SIZE) as usize;
        let end_y = ((rect.bottom() - screen.y - 1).max(0) / TILE_SIZE) as usize;
        for ty in start_y..=end_y.min(tiles_y - 1) {
            let row = ty * tiles_x;
            for tx in start_x..=end_x.min(tiles_x - 1) {
                occupied[row + tx] = true;
            }
        }
    }

    let mut output = Vec::new();
    let mut row_runs = vec![None::<Rect>; tiles_x];
    for ty in 0..tiles_y {
        let mut tx = 0;
        while tx < tiles_x {
            if !occupied[ty * tiles_x + tx] {
                tx += 1;
                continue;
            }

            let start = tx;
            while tx < tiles_x && occupied[ty * tiles_x + tx] {
                tx += 1;
            }

            let rect = tile_span_rect(screen, start, tx - start, ty);
            let mut extended = false;
            for slot in row_runs.iter_mut() {
                if let Some(existing) = slot.as_mut() {
                    if existing.x == rect.x && existing.width == rect.width && existing.bottom() == rect.y
                    {
                        existing.height = existing.height.saturating_add(rect.height);
                        extended = true;
                        break;
                    }
                }
            }
            if extended {
                continue;
            }

            if let Some(slot) = row_runs.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(rect);
            } else {
                output.extend(row_runs.iter_mut().filter_map(|slot| slot.take()));
                row_runs[0] = Some(rect);
            }
        }

        let next_row_start = screen.y + ((ty as i32 + 1) * TILE_SIZE);
        for slot in row_runs.iter_mut() {
            let should_flush = match slot {
                Some(rect) => rect.bottom() < next_row_start,
                None => false,
            };
            if should_flush {
                if let Some(rect) = slot.take() {
                    output.push(rect);
                }
            }
        }
    }

    output.extend(row_runs.into_iter().flatten());
    output.sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
    output.dedup();
    output
}

fn tile_span_rect(screen: Rect, start_x: usize, tile_count: usize, tile_y: usize) -> Rect {
    let x = screen.x + start_x as i32 * TILE_SIZE;
    let y = screen.y + tile_y as i32 * TILE_SIZE;
    let width = ((tile_count as i32 * TILE_SIZE).min(screen.right() - x)).max(0) as u32;
    let height = (TILE_SIZE.min(screen.bottom() - y)).max(0) as u32;
    Rect::new(x, y, width, height)
}

fn partial_redraw_cost(regions: &[Rect]) -> u64 {
    let damaged_pixels = regions
        .iter()
        .map(|rect| rect.width as u64 * rect.height as u64)
        .sum::<u64>();
    let damaged_tiles = regions
        .iter()
        .map(|rect| {
            let tiles_x = ((rect.width as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as u64;
            let tiles_y = ((rect.height as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as u64;
            tiles_x * tiles_y
        })
        .sum::<u64>();
    damaged_pixels + damaged_tiles * TILE_COST_WEIGHT as u64 + regions.len() as u64 * BATCH_COST_WEIGHT as u64
}

fn full_redraw_cost(screen: Rect) -> u64 {
    screen.width as u64 * screen.height as u64
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_coalesces_dense_damage_into_tiles_instead_of_full_redraw() {
        let mut tracker = DamageTracker {
            full_redraw: false,
            regions: Vec::new(),
        };
        for index in 0..40 {
            tracker.mark_rect(Rect::new(index * 4, 0, 2, 2));
        }

        let damage = tracker.take(Rect::new(0, 0, 256, 128));
        assert!(!damage.is_empty());
        assert!(damage.len() < 10);
        assert!(damage.iter().all(|rect| !rect.is_empty()));
    }

    #[test]
    fn take_returns_full_screen_only_for_explicit_full_redraw() {
        let mut tracker = DamageTracker::new();
        let damage = tracker.take(Rect::new(0, 0, 320, 200));
        assert_eq!(damage, vec![Rect::new(0, 0, 320, 200)]);
    }

    #[test]
    fn take_promotes_dense_fragmented_damage_to_full_screen_when_cost_exceeds_threshold() {
        let mut tracker = DamageTracker {
            full_redraw: false,
            regions: Vec::new(),
        };
        for y in 0..6 {
            for x in 0..8 {
                tracker.mark_rect(Rect::new(x * 34, y * 22, 18, 10));
            }
        }

        let damage = tracker.take(Rect::new(0, 0, 320, 200));
        assert_eq!(damage, vec![Rect::new(0, 0, 320, 200)]);
    }
}
