use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RmapEntry {
    pub space_id: u64,
    pub virt: u64,
    pub pml4: u64,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct TtuFlags: u32 {
        const SKIP_CURRENT  = 1 << 0;
        const IGNORE_MLOCK  = 1 << 1;
        const SYNC          = 1 << 2;
        const SPLIT_HUGE    = 1 << 3;
    }
}

struct RmapTable {
    entries: BTreeMap<u64, Vec<RmapEntry>>,
}

impl RmapTable {
    const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    fn insert(&mut self, phys: u64, space_id: u64, virt: u64, pml4: u64) {
        let key = phys & !(0xFFF);
        let entry = RmapEntry { space_id, virt, pml4 };
        self.entries.entry(key).or_default().push(entry);
    }

    fn remove(&mut self, phys: u64, space_id: u64, virt: u64) {
        let key = phys & !(0xFFF);
        if let Some(vec) = self.entries.get_mut(&key) {
            vec.retain(|e| !(e.space_id == space_id && e.virt == virt));
            if vec.is_empty() {
                self.entries.remove(&key);
            }
        }
    }

    fn remove_all_for_phys(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        self.entries.remove(&key);
    }

    fn lookup(&self, phys: u64) -> Vec<RmapEntry> {
        let key = phys & !(0xFFF);
        self.entries.get(&key).cloned().unwrap_or_default()
    }

    fn cleanup_space(&mut self, space_id: u64) {
        self.entries.retain(|_key, vec| {
            vec.retain(|e| e.space_id != space_id);
            !vec.is_empty()
        });
    }

    #[allow(dead_code)]
    fn remove_all_for_space(&mut self, phys: u64, space_id: u64) {
        let key = phys & !(0xFFF);
        if let Some(vec) = self.entries.get_mut(&key) {
            vec.retain(|e| e.space_id != space_id);
            if vec.is_empty() {
                self.entries.remove(&key);
            }
        }
    }

    fn total_entries(&self) -> usize {
        self.entries.values().map(|v| v.len()).sum()
    }
}

use lazy_static::lazy_static;
lazy_static! {
    static ref RMAP_TABLE: Mutex<RmapTable> = Mutex::new(RmapTable::new());
}

pub fn rmap_insert(phys: u64, space_id: u64, virt: u64, pml4: u64) {
    RMAP_TABLE.lock().insert(phys, space_id, virt, pml4);
}

pub fn rmap_remove(phys: u64, space_id: u64, virt: u64) {
    RMAP_TABLE.lock().remove(phys, space_id, virt);
}

pub fn rmap_remove_all_for_phys(phys: u64) {
    RMAP_TABLE.lock().remove_all_for_phys(phys);
}

pub fn rmap_lookup(phys: u64) -> Vec<RmapEntry> {
    RMAP_TABLE.lock().lookup(phys)
}

pub fn rmap_cleanup_space(space_id: u64) {
    RMAP_TABLE.lock().cleanup_space(space_id);
}

pub fn rmap_replace_page(old_phys: u64, new_phys: u64) -> Vec<RmapEntry> {
    let mut table = RMAP_TABLE.lock();
    let old_key = old_phys & !(0xFFF);
    let new_key = new_phys & !(0xFFF);
    if let Some(entries) = table.entries.remove(&old_key) {
        table
            .entries
            .entry(new_key)
            .or_default()
            .extend(entries.clone());
        entries
    } else {
        Vec::new()
    }
}

#[allow(dead_code)]
pub fn rmap_total_entries() -> usize {
    RMAP_TABLE.lock().total_entries()
}
