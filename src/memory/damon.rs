//! DAMON-benzeri erişim örnekleme ve sıcaklık tahmini.
//!
//! Linux DAMON'daki tam bölge birleştirme/moving-window mantığını kopyalamaz;
//! echOS tarafında düşük maliyetli sayfa-granüler "hint" üretir ve reclaim
//! yoluna sıcak/soğuk sinyali sağlar.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

const DAMON_HEAT_MAX: u16 = 1024;
const HOT_THRESHOLD: u16 = 768;
const WARM_THRESHOLD: u16 = 384;
const ACCESS_BOOST: u16 = 192;
const MISS_PENALTY: u16 = 32;
const REFAULT_BOOST: u16 = 320;
const AGING_INTERVAL_TICKS: u64 = 64;
const STALE_INTERVAL_TICKS: u64 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamonTemperature {
    Hot,
    Warm,
    Cold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamonKey {
    pub space_id: u64,
    pub page_index: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct DamonHint {
    pub key: DamonKey,
    pub node_id: u16,
    pub heat: u16,
    pub idle_ticks: u64,
    pub temperature: DamonTemperature,
}

#[derive(Clone, Copy, Debug)]
struct DamonEntry {
    key: DamonKey,
    node_id: u16,
    heat: u16,
    last_access_tick: u64,
    samples: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DamonStats {
    pub tracked_regions: usize,
    pub hot_regions: usize,
    pub warm_regions: usize,
    pub cold_regions: usize,
    pub total_samples: u64,
    pub age_passes: u64,
    pub refault_boosts: u64,
    pub evictions: u64,
}

struct DamonState {
    entries: BTreeMap<(u64, u64), DamonEntry>,
    total_samples: u64,
    age_passes: u64,
    refault_boosts: u64,
    evictions: u64,
}

impl DamonState {
    const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            total_samples: 0,
            age_passes: 0,
            refault_boosts: 0,
            evictions: 0,
        }
    }

    fn classify(entry: &DamonEntry, now_tick: u64) -> DamonTemperature {
        let idle = now_tick.saturating_sub(entry.last_access_tick);
        if entry.heat >= HOT_THRESHOLD && idle <= STALE_INTERVAL_TICKS {
            DamonTemperature::Hot
        } else if entry.heat >= WARM_THRESHOLD && idle <= STALE_INTERVAL_TICKS.saturating_mul(2) {
            DamonTemperature::Warm
        } else {
            DamonTemperature::Cold
        }
    }

    fn hint(&self, key: DamonKey, now_tick: u64) -> Option<DamonHint> {
        let entry = self.entries.get(&(key.space_id, key.page_index))?;
        Some(DamonHint {
            key,
            node_id: entry.node_id,
            heat: entry.heat,
            idle_ticks: now_tick.saturating_sub(entry.last_access_tick),
            temperature: Self::classify(entry, now_tick),
        })
    }

    fn record_access(
        &mut self,
        key: DamonKey,
        node_id: u16,
        accessed: bool,
        now_tick: u64,
    ) {
        let entry = self.entries.entry((key.space_id, key.page_index)).or_insert(DamonEntry {
            key,
            node_id,
            heat: 0,
            last_access_tick: now_tick,
            samples: 0,
        });
        entry.node_id = node_id;
        entry.samples = entry.samples.saturating_add(1);
        if accessed {
            entry.heat = entry.heat.saturating_add(ACCESS_BOOST).min(DAMON_HEAT_MAX);
            entry.last_access_tick = now_tick;
        } else {
            entry.heat = entry.heat.saturating_sub(MISS_PENALTY);
        }
        self.total_samples = self.total_samples.saturating_add(1);
    }

    fn age(&mut self, now_tick: u64) {
        self.age_passes = self.age_passes.saturating_add(1);
        for entry in self.entries.values_mut() {
            let idle = now_tick.saturating_sub(entry.last_access_tick);
            if idle >= STALE_INTERVAL_TICKS.saturating_mul(4) {
                entry.heat = entry.heat.saturating_sub(ACCESS_BOOST);
            } else if idle >= STALE_INTERVAL_TICKS {
                entry.heat = entry.heat.saturating_sub(MISS_PENALTY.saturating_mul(2));
            }
        }
    }

    fn record_refault(&mut self, key: DamonKey, now_tick: u64) {
        let entry = self.entries.entry((key.space_id, key.page_index)).or_insert(DamonEntry {
            key,
            node_id: 0,
            heat: 0,
            last_access_tick: now_tick,
            samples: 0,
        });
        entry.heat = entry.heat.saturating_add(REFAULT_BOOST).min(DAMON_HEAT_MAX);
        entry.last_access_tick = now_tick;
        self.refault_boosts = self.refault_boosts.saturating_add(1);
    }

    fn record_eviction(&mut self, key: DamonKey) {
        self.evictions = self.evictions.saturating_add(1);
        self.entries.remove(&(key.space_id, key.page_index));
    }

    fn remove(&mut self, key: DamonKey) {
        self.entries.remove(&(key.space_id, key.page_index));
    }

    fn pick_victim(
        &self,
        space_hint: Option<u64>,
        node_hint: Option<u16>,
        now_tick: u64,
    ) -> Option<DamonHint> {
        let mut best: Option<DamonHint> = None;
        for entry in self.entries.values() {
            if let Some(space_id) = space_hint {
                if entry.key.space_id != space_id {
                    continue;
                }
            }
            if let Some(node_id) = node_hint {
                if entry.node_id != node_id {
                    continue;
                }
            }
            let candidate = DamonHint {
                key: entry.key,
                node_id: entry.node_id,
                heat: entry.heat,
                idle_ticks: now_tick.saturating_sub(entry.last_access_tick),
                temperature: Self::classify(entry, now_tick),
            };
            match best {
                None => best = Some(candidate),
                Some(current) => {
                    let better = candidate.temperature == DamonTemperature::Cold
                        && current.temperature != DamonTemperature::Cold
                        || candidate.temperature == current.temperature
                            && (candidate.heat < current.heat
                                || (candidate.heat == current.heat
                                    && candidate.idle_ticks > current.idle_ticks));
                    if better {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    fn stats(&self, now_tick: u64) -> DamonStats {
        let mut hot_regions = 0;
        let mut warm_regions = 0;
        let mut cold_regions = 0;
        for entry in self.entries.values() {
            match Self::classify(entry, now_tick) {
                DamonTemperature::Hot => hot_regions += 1,
                DamonTemperature::Warm => warm_regions += 1,
                DamonTemperature::Cold => cold_regions += 1,
            }
        }
        DamonStats {
            tracked_regions: self.entries.len(),
            hot_regions,
            warm_regions,
            cold_regions,
            total_samples: self.total_samples,
            age_passes: self.age_passes,
            refault_boosts: self.refault_boosts,
            evictions: self.evictions,
        }
    }
}

static DAMON_ENABLED: AtomicBool = AtomicBool::new(true);
static LAST_AGE_TICK: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    static ref DAMON: Mutex<DamonState> = Mutex::new(DamonState::new());
}

pub fn init(enabled: bool) {
    DAMON_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    DAMON_ENABLED.load(Ordering::Acquire)
}

pub fn record_page_access(
    space_id: u64,
    page_index: u64,
    node_id: u16,
    accessed: bool,
    now_tick: u64,
) {
    if !is_enabled() {
        return;
    }
    DAMON.lock().record_access(
        DamonKey {
            space_id,
            page_index,
        },
        node_id,
        accessed,
        now_tick,
    );
}

pub fn age(now_tick: u64) {
    if !is_enabled() {
        return;
    }
    let prev = LAST_AGE_TICK.load(Ordering::Relaxed);
    if now_tick <= prev || now_tick.saturating_sub(prev) < AGING_INTERVAL_TICKS {
        return;
    }
    LAST_AGE_TICK.store(now_tick, Ordering::Relaxed);
    DAMON.lock().age(now_tick);
}

pub fn hint_for_page(space_id: u64, page_index: u64, now_tick: u64) -> Option<DamonHint> {
    if !is_enabled() {
        return None;
    }
    DAMON.lock().hint(
        DamonKey {
            space_id,
            page_index,
        },
        now_tick,
    )
}

pub fn pick_victim(
    space_hint: Option<u64>,
    node_hint: Option<u16>,
    now_tick: u64,
) -> Option<DamonHint> {
    if !is_enabled() {
        return None;
    }
    DAMON.lock().pick_victim(space_hint, node_hint, now_tick)
}

pub fn record_refault(space_id: u64, page_index: u64, now_tick: u64) {
    if !is_enabled() {
        return;
    }
    DAMON.lock().record_refault(
        DamonKey {
            space_id,
            page_index,
        },
        now_tick,
    );
}

pub fn record_eviction(space_id: u64, page_index: u64) {
    if !is_enabled() {
        return;
    }
    DAMON.lock().record_eviction(DamonKey {
        space_id,
        page_index,
    });
}

pub fn remove_page(space_id: u64, page_index: u64) {
    if !is_enabled() {
        return;
    }
    DAMON.lock().remove(DamonKey {
        space_id,
        page_index,
    });
}

pub fn get_stats(now_tick: u64) -> DamonStats {
    DAMON.lock().stats(now_tick)
}
