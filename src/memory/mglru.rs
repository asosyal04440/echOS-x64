//! Multi-Gen LRU (MGLRU) — sıcak/soğuk sayfa sınıflandırması.
//!
//! Bu modül, klasik active/inactive LRU'ya ek olarak sayfaları nesiller
//! (generation) üzerinden takip eder. Amaç:
//! 1. Hot/cold ayrımını erişim geri-bildirimiyle yapmak
//! 2. Reclaim sırasında en eski nesilden başlamak
//! 3. Refault durumunda sayfayı hızlıca sıcak nesle taşımak

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

const MGLRU_GENERATIONS: u64 = 8;
const HOT_REF_THRESHOLD: u16 = 3;
const COLD_EVICTION_AGE: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MglruPageKey {
    pub space_id: u64,
    pub page_index: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MglruVictim {
    pub key: MglruPageKey,
    pub node_id: u16,
    pub generation: u64,
    pub hot_score: u16,
}

#[derive(Clone, Copy, Debug)]
struct MglruEntry {
    key: MglruPageKey,
    node_id: u16,
    generation: u64,
    access_count: u16,
    last_access_tick: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MglruStats {
    pub generations: u64,
    pub tracked_pages: usize,
    pub current_generation: u64,
    pub promotions: u64,
    pub demotions: u64,
    pub evictions: u64,
    pub refault_promotions: u64,
}

struct MglruState {
    current_generation: u64,
    entries: BTreeMap<(u64, u64), MglruEntry>,
    by_generation: BTreeMap<u64, Vec<(u64, u64)>>,
    promotions: u64,
    demotions: u64,
    evictions: u64,
    refault_promotions: u64,
}

impl MglruState {
    fn new() -> Self {
        Self {
            current_generation: 1,
            entries: BTreeMap::new(),
            by_generation: BTreeMap::new(),
            promotions: 0,
            demotions: 0,
            evictions: 0,
            refault_promotions: 0,
        }
    }

    fn generation_slot(generation: u64) -> u64 {
        generation % MGLRU_GENERATIONS
    }

    fn detach_from_generation(&mut self, key: (u64, u64), generation: u64) {
        let slot = Self::generation_slot(generation);
        if let Some(bucket) = self.by_generation.get_mut(&slot) {
            if let Some(idx) = bucket.iter().position(|k| *k == key) {
                bucket.swap_remove(idx);
            }
        }
    }

    fn attach_to_generation(&mut self, key: (u64, u64), generation: u64) {
        let slot = Self::generation_slot(generation);
        self.by_generation.entry(slot).or_default().push(key);
    }

    fn set_generation(&mut self, key: (u64, u64), new_generation: u64, now_tick: u64) {
        let (old_generation, access_count) = match self.entries.get(&key) {
            Some(entry) => (entry.generation, entry.access_count),
            None => return,
        };

        if old_generation != new_generation {
            self.detach_from_generation(key, old_generation);
            self.attach_to_generation(key, new_generation);
        }

        if let Some(entry) = self.entries.get_mut(&key) {
            entry.generation = new_generation;
            entry.last_access_tick = now_tick;
            entry.access_count = access_count;
        }
    }

    fn on_access(&mut self, key: MglruPageKey, node_id: u16, accessed_bit: bool, now_tick: u64) {
        let map_key = (key.space_id, key.page_index);
        if let Some(mut entry) = self.entries.get(&map_key).copied() {
            if accessed_bit {
                entry.access_count = entry.access_count.saturating_add(1);
                entry.last_access_tick = now_tick;
                if entry.access_count >= HOT_REF_THRESHOLD {
                    let target = self.current_generation;
                    if entry.generation != target {
                        self.promotions = self.promotions.saturating_add(1);
                        self.detach_from_generation(map_key, entry.generation);
                        entry.generation = target;
                        self.attach_to_generation(map_key, entry.generation);
                    }
                    entry.access_count = HOT_REF_THRESHOLD;
                }
            } else {
                let age = self.current_generation.saturating_sub(entry.generation);
                if age > COLD_EVICTION_AGE && entry.access_count > 0 {
                    entry.access_count -= 1;
                }
            }
            self.entries.insert(map_key, entry);
            return;
        }

        let generation = if accessed_bit {
            self.current_generation
        } else {
            self.current_generation.saturating_sub(1)
        };
        let entry = MglruEntry {
            key,
            node_id,
            generation,
            access_count: if accessed_bit { 1 } else { 0 },
            last_access_tick: now_tick,
        };
        self.entries.insert(map_key, entry);
        self.attach_to_generation(map_key, generation);
    }

    fn age_tick(&mut self, now_tick: u64) {
        let next_generation = self.current_generation.saturating_add(1);
        self.current_generation = next_generation.max(1);
        let mut to_demote: Vec<(u64, u64, u64)> = Vec::new();
        for (k, entry) in self.entries.iter() {
            let idle = now_tick.saturating_sub(entry.last_access_tick);
            if idle > 2048 && entry.generation + 1 < self.current_generation {
                to_demote.push((k.0, k.1, entry.generation));
            }
        }
        for (space_id, page_index, old_generation) in to_demote {
            let key = (space_id, page_index);
            let new_generation = old_generation.saturating_sub(1);
            self.demotions = self.demotions.saturating_add(1);
            self.set_generation(key, new_generation, now_tick);
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.access_count = entry.access_count.saturating_sub(1);
            }
        }
    }

    fn remove_page(&mut self, key: MglruPageKey) {
        let map_key = (key.space_id, key.page_index);
        if let Some(entry) = self.entries.remove(&map_key) {
            self.detach_from_generation(map_key, entry.generation);
        }
    }

    fn record_refault(&mut self, key: MglruPageKey, now_tick: u64) {
        let map_key = (key.space_id, key.page_index);
        if let Some(entry) = self.entries.get_mut(&map_key) {
            entry.generation = self.current_generation;
            entry.access_count = HOT_REF_THRESHOLD;
            entry.last_access_tick = now_tick;
            self.refault_promotions = self.refault_promotions.saturating_add(1);
            return;
        }
        self.on_access(key, 0, true, now_tick);
    }

    fn record_eviction(&mut self, key: MglruPageKey) {
        self.evictions = self.evictions.saturating_add(1);
        self.remove_page(key);
    }

    fn pick_victim(&self, space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
        if self.entries.is_empty() {
            return None;
        }

        let mut best: Option<MglruVictim> = None;
        for entry in self.entries.values() {
            if let Some(space) = space_hint {
                if entry.key.space_id != space {
                    continue;
                }
            }
            if let Some(node) = node_hint {
                if entry.node_id != node {
                    continue;
                }
            }
            let candidate = MglruVictim {
                key: entry.key,
                node_id: entry.node_id,
                generation: entry.generation,
                hot_score: entry.access_count,
            };
            match best {
                None => best = Some(candidate),
                Some(curr) => {
                    if candidate.generation < curr.generation
                        || (candidate.generation == curr.generation
                            && candidate.hot_score < curr.hot_score)
                    {
                        best = Some(candidate);
                    }
                }
            }
        }
        best
    }

    fn stats(&self) -> MglruStats {
        MglruStats {
            generations: MGLRU_GENERATIONS,
            tracked_pages: self.entries.len(),
            current_generation: self.current_generation,
            promotions: self.promotions,
            demotions: self.demotions,
            evictions: self.evictions,
            refault_promotions: self.refault_promotions,
        }
    }
}

static MGLRU_ENABLED: AtomicBool = AtomicBool::new(true);
static MGLRU_LAST_AGE_TICK: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    static ref MGLRU: Mutex<MglruState> = Mutex::new(MglruState::new());
}

pub fn init(enabled: bool) {
    MGLRU_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    MGLRU_ENABLED.load(Ordering::Acquire)
}

pub fn record_page_access(
    space_id: u64,
    page_index: u64,
    node_id: u16,
    accessed_bit: bool,
    now_tick: u64,
) {
    if !is_enabled() {
        return;
    }
    MGLRU.lock().on_access(
        MglruPageKey {
            space_id,
            page_index,
        },
        node_id,
        accessed_bit,
        now_tick,
    );
}

pub fn age_generations(now_tick: u64) {
    if !is_enabled() {
        return;
    }
    let prev = MGLRU_LAST_AGE_TICK.load(Ordering::Relaxed);
    if now_tick <= prev || now_tick.saturating_sub(prev) < 64 {
        return;
    }
    MGLRU_LAST_AGE_TICK.store(now_tick, Ordering::Relaxed);
    MGLRU.lock().age_tick(now_tick);
}

pub fn record_refault(space_id: u64, page_index: u64, now_tick: u64) {
    if !is_enabled() {
        return;
    }
    MGLRU.lock().record_refault(
        MglruPageKey {
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
    MGLRU.lock().record_eviction(MglruPageKey {
        space_id,
        page_index,
    });
}

pub fn remove_page(space_id: u64, page_index: u64) {
    if !is_enabled() {
        return;
    }
    MGLRU.lock().remove_page(MglruPageKey {
        space_id,
        page_index,
    });
}

pub fn pick_victim(space_hint: Option<u64>, node_hint: Option<u16>) -> Option<MglruVictim> {
    if !is_enabled() {
        return None;
    }
    MGLRU.lock().pick_victim(space_hint, node_hint)
}

pub fn get_stats() -> MglruStats {
    MGLRU.lock().stats()
}
