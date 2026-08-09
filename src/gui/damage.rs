//! Dirty region tracker with tile-grid compaction and adaptive redraw cost.

use crate::gui::protocol::Rect;
use alloc::vec;
use alloc::vec::Vec;

const TILE_SIZE: i32 = 64;
const MAX_DAMAGE_REGIONS: usize = 24;

/// Base threshold: prefer partial redraw when partial_cost < 78% of full_cost.
const FULL_REDRAW_ALPHA_NUM: u32 = 78;
const FULL_REDRAW_ALPHA_DEN: u32 = 100;

/// Base cost weights (starting point for adaptive EWMA calibration).
const BASE_TILE_COST_WEIGHT: u32 = 4096;
const BASE_BATCH_COST_WEIGHT: u32 = 768;

/// Adaptive bounds.
const MIN_TILE_COST_WEIGHT: u32 = 2048;
const MAX_TILE_COST_WEIGHT: u32 = 8192;
const MIN_BATCH_COST_WEIGHT: u32 = 384;
const MAX_BATCH_COST_WEIGHT: u32 = 1536;

/// EWMA smoothing: new = (sample * NUM + old * (DEN - NUM)) / DEN
const EWMA_NUM: u32 = 1;
const EWMA_DEN: u32 = 32;

/// Density thresholds (percent * 100). When EWMA density crosses these,
/// the cost weights are adjusted to prefer the right redraw strategy.
const DENSITY_HIGH_THRESH: u32 = 5000; // 50.00%
const DENSITY_LOW_THRESH: u32 = 1500;  // 15.00%
const WEIGHT_ADJUST_NUM: u32 = 1;
const WEIGHT_ADJUST_DEN: u32 = 8;

/// Minimum overlap ratio to justify a merge in normalize.
const MERGE_OVERLAP_RATIO_NUM: u32 = 15;
const MERGE_OVERLAP_RATIO_DEN: u32 = 100;

pub struct DamageTracker {
    full_redraw: bool,
    regions: Vec<Rect>,
    /// EWMA-smoothed density estimate (screen_percent * 100).
    density_ewma: u32,
    /// Adaptive cost coefficients.
    tile_cost_weight: u32,
    batch_cost_weight: u32,
    /// Reusable tile occupancy buffer (avoids per-call allocation).
    tile_buf: Vec<bool>,
    tile_buf_w: usize,
    tile_buf_h: usize,
}

impl DamageTracker {
    pub fn new() -> Self {
        Self {
            full_redraw: true,
            regions: Vec::new(),
            density_ewma: 0,
            tile_cost_weight: BASE_TILE_COST_WEIGHT,
            batch_cost_weight: BASE_BATCH_COST_WEIGHT,
            tile_buf: Vec::new(),
            tile_buf_w: 0,
            tile_buf_h: 0,
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

        let mut compacted = tile_compact_with_buf(&out, screen, &mut self.tile_buf, &mut self.tile_buf_w, &mut self.tile_buf_h);
        if compacted.is_empty() {
            compacted.push(screen);
        }

        let partial_cost = adaptive_partial_cost(&compacted, self.tile_cost_weight, self.batch_cost_weight);
        let full_cost = full_redraw_cost(screen);

        if partial_cost * FULL_REDRAW_ALPHA_DEN as u64 > full_cost * FULL_REDRAW_ALPHA_NUM as u64 {
            return vec![screen];
        }

        self.update_weights(&compacted, screen, partial_cost, full_cost);
        compacted
    }

    fn update_weights(&mut self, compacted: &[Rect], screen: Rect, partial_cost: u64, full_cost: u64) {
        let screen_area = screen.width as u64 * screen.height as u64;
        if screen_area == 0 {
            return;
        }

        let damaged_area: u64 = compacted
            .iter()
            .map(|r| r.width as u64 * r.height as u64)
            .sum();
        let sample_density = (damaged_area * 10000 / screen_area).min(10000) as u32;

        self.density_ewma = self
            .density_ewma
            .wrapping_mul(EWMA_DEN - EWMA_NUM)
            .wrapping_add(sample_density.wrapping_mul(EWMA_NUM))
            / EWMA_DEN;

        let headroom_ratio = FULL_REDRAW_ALPHA_DEN as u64 * partial_cost / full_cost.max(1);
        let at_threshold = headroom_ratio >= (FULL_REDRAW_ALPHA_NUM as u64 * 85 / 100);
        let near_threshold = headroom_ratio >= (FULL_REDRAW_ALPHA_NUM as u64 * 70 / 100);

        if self.density_ewma > DENSITY_HIGH_THRESH && at_threshold {
            let adjust = (MAX_TILE_COST_WEIGHT - self.tile_cost_weight) * WEIGHT_ADJUST_NUM / WEIGHT_ADJUST_DEN;
            self.tile_cost_weight = (self.tile_cost_weight + adjust).min(MAX_TILE_COST_WEIGHT);
            let adjust = (MAX_BATCH_COST_WEIGHT - self.batch_cost_weight) * WEIGHT_ADJUST_NUM / WEIGHT_ADJUST_DEN;
            self.batch_cost_weight = (self.batch_cost_weight + adjust).min(MAX_BATCH_COST_WEIGHT);
        } else if self.density_ewma < DENSITY_LOW_THRESH && !near_threshold {
            let adjust = (self.tile_cost_weight - MIN_TILE_COST_WEIGHT) * WEIGHT_ADJUST_NUM / WEIGHT_ADJUST_DEN;
            self.tile_cost_weight = self.tile_cost_weight.saturating_sub(adjust).max(MIN_TILE_COST_WEIGHT);
            let adjust = (self.batch_cost_weight - MIN_BATCH_COST_WEIGHT) * WEIGHT_ADJUST_NUM / WEIGHT_ADJUST_DEN;
            self.batch_cost_weight = self.batch_cost_weight.saturating_sub(adjust).max(MIN_BATCH_COST_WEIGHT);
        }
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
                if should_merge(candidate, self.regions[other]) {
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

fn tile_compact_with_buf(
    regions: &[Rect],
    screen: Rect,
    buf: &mut Vec<bool>,
    buf_w: &mut usize,
    buf_h: &mut usize,
) -> Vec<Rect> {
    if screen.is_empty() {
        return Vec::new();
    }

    let tiles_x = ((screen.width as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as usize;
    let tiles_y = ((screen.height as i32 + TILE_SIZE - 1) / TILE_SIZE).max(1) as usize;
    let tile_count = tiles_x.saturating_mul(tiles_y);

    if tile_count > *buf_w * *buf_h {
        buf.resize(tile_count, false);
        *buf_w = tiles_x;
        *buf_h = tiles_y;
    }
    let occupied = buf;
    occupied.fill(false);

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
                    if existing.x == rect.x
                        && existing.width == rect.width
                        && existing.bottom() == rect.y
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

fn adaptive_partial_cost(regions: &[Rect], tile_weight: u32, batch_weight: u32) -> u64 {
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
    damaged_pixels
        + damaged_tiles * tile_weight as u64
        + regions.len() as u64 * batch_weight as u64
}

fn full_redraw_cost(screen: Rect) -> u64 {
    screen.width as u64 * screen.height as u64
}

/// Return true when merging `a` and `b` saves rendering work (significant overlap).
fn should_merge(a: Rect, b: Rect) -> bool {
    if !a.intersects(&b) {
        let expanded = Rect::new(
            a.x.saturating_sub(1),
            a.y.saturating_sub(1),
            a.width.saturating_add(2),
            a.height.saturating_add(2),
        );
        if !expanded.intersects(&b) {
            return false;
        }
        return true;
    }

    let area_a = a.width as u64 * a.height as u64;
    let area_b = b.width as u64 * b.height as u64;
    let min_area = area_a.min(area_b);
    if min_area == 0 {
        return true;
    }

    let intersection = a.intersection(&b).unwrap();
    let overlap_area = intersection.width as u64 * intersection.height as u64;
    overlap_area * MERGE_OVERLAP_RATIO_DEN as u64
        >= min_area * MERGE_OVERLAP_RATIO_NUM as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker_without_full_redraw() -> DamageTracker {
        DamageTracker {
            full_redraw: false,
            regions: Vec::new(),
            density_ewma: 0,
            tile_cost_weight: BASE_TILE_COST_WEIGHT,
            batch_cost_weight: BASE_BATCH_COST_WEIGHT,
            tile_buf: Vec::new(),
            tile_buf_w: 0,
            tile_buf_h: 0,
        }
    }

    #[test]
    fn take_coalesces_dense_damage_into_tiles_instead_of_full_redraw() {
        let mut tracker = tracker_without_full_redraw();
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
        let mut tracker = tracker_without_full_redraw();
        for y in 0..6 {
            for x in 0..8 {
                tracker.mark_rect(Rect::new(x * 34, y * 22, 18, 10));
            }
        }

        let damage = tracker.take(Rect::new(0, 0, 320, 200));
        assert_eq!(damage, vec![Rect::new(0, 0, 320, 200)]);
    }
}
