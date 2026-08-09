use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

const DCACHE_HASH_BITS: usize = 13;
const DCACHE_HASH_SIZE: usize = 1 << DCACHE_HASH_BITS;
const DCACHE_MAX_ENTRIES: usize = 32768;
const DCACHE_SHRINK_BATCH: usize = 1024;
const DCACHE_STALE_CLEANUP_RATIO: usize = 4;

#[derive(Clone)]
pub struct Dentry {
    pub name: String,
    pub parent_ino: u64,
    pub ino: u64,
    pub is_dir: bool,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct HashKey(u64);

fn hash_key(parent_ino: u64, name: &str) -> HashKey {
    let mut h: u64 = parent_ino.wrapping_mul(33);
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    HashKey(h)
}

pub struct Dcache {
    entries: Vec<Option<Rc<Dentry>>>,
    hash_table: [Vec<usize>; DCACHE_HASH_SIZE],
    lru: Vec<usize>,
    lru_index: BTreeMap<usize, usize>,
    count: usize,
    stale_count: usize,
}

unsafe impl Send for Dcache {}

impl Dcache {
    pub const fn new() -> Self {
        const EMPTY_VEC: Vec<usize> = Vec::new();
        Self {
            entries: Vec::new(),
            hash_table: [EMPTY_VEC; DCACHE_HASH_SIZE],
            lru: Vec::new(),
            lru_index: BTreeMap::new(),
            count: 0,
            stale_count: 0,
        }
    }

    fn hash_bucket(key: HashKey) -> usize {
        (key.0 as usize) & (DCACHE_HASH_SIZE - 1)
    }

    pub fn lookup(&mut self, parent_ino: u64, name: &str) -> Option<Rc<Dentry>> {
        let key = hash_key(parent_ino, name);
        let bucket_idx = Self::hash_bucket(key);
        let found = {
            self.hash_table[bucket_idx].iter().find(|&&idx| {
                self.entries[idx]
                    .as_ref()
                    .is_some_and(|d| d.parent_ino == parent_ino && d.name == name)
            }).copied()
        };
        if let Some(idx) = found {
            self.touch_lru(idx);
            self.entries[idx].clone()
        } else {
            None
        }
    }

    pub fn alloc(&mut self, dentry: Dentry) -> Rc<Dentry> {
        if self.count >= DCACHE_MAX_ENTRIES {
            self.shrink(DCACHE_SHRINK_BATCH);
        }
        let idx = self.entries.len();
        let key = hash_key(dentry.parent_ino, &dentry.name);
        let bucket = Self::hash_bucket(key);
        let rc = Rc::new(dentry);
        self.entries.push(Some(rc.clone()));
        self.hash_table[bucket].push(idx);
        self.lru.push(idx);
        self.lru_index.insert(idx, self.lru.len() - 1);
        self.count += 1;
        rc
    }

    pub fn delete(&mut self, parent_ino: u64, name: &str) {
        let key = hash_key(parent_ino, name);
        let bucket = Self::hash_bucket(key);
        let mut found = None;
        for (i, &idx) in self.hash_table[bucket].iter().enumerate() {
            if let Some(ref dentry) = self.entries[idx] {
                if dentry.parent_ino == parent_ino && dentry.name == name {
                    found = Some((i, idx));
                    break;
                }
            }
        }
        if let Some((bi, idx)) = found {
            self.hash_table[bucket].remove(bi);
            self.entries[idx] = None;
            self.stale_count += 1;
            if let Some(&lru_pos) = self.lru_index.get(&idx) {
                self.lru.swap_remove(lru_pos);
                if lru_pos < self.lru.len() {
                    if let Some(&swapped) = self.lru.get(lru_pos) {
                        self.lru_index.insert(swapped, lru_pos);
                    }
                }
            }
            self.lru_index.remove(&idx);
            self.count -= 1;
            self.maybe_compact();
        }
    }

    pub fn rename(&mut self, old_parent: u64, old_name: &str, new_parent: u64, new_name: &str) {
        if old_parent == new_parent && old_name == new_name {
            return;
        }

        let old_key = hash_key(old_parent, old_name);
        let old_bucket = Self::hash_bucket(old_key);

        let found_idx = match self.remove_from_bucket(old_bucket, old_parent, old_name) {
            Some(idx) => idx,
            None => return,
        };

        if let Some(rc) = self.entries[found_idx].as_ref() {
            let new_key = hash_key(new_parent, new_name);
            let new_bucket = Self::hash_bucket(new_key);

                let ptr = Rc::as_ptr(rc) as *mut Dentry;
            unsafe {
                (*ptr).parent_ino = new_parent;
                let mut new_name_str = String::from(new_name);
                core::mem::swap(&mut (*ptr).name, &mut new_name_str);
            }

            self.hash_table[new_bucket].push(found_idx);
        }
    }

    fn remove_from_bucket(&mut self, bucket: usize, parent_ino: u64, name: &str) -> Option<usize> {
        for bi in 0..self.hash_table[bucket].len() {
            let idx = self.hash_table[bucket][bi];
            if let Some(ref dentry) = self.entries[idx] {
                if dentry.parent_ino == parent_ino && dentry.name == name {
                    self.hash_table[bucket].remove(bi);
                    return Some(idx);
                }
            }
        }
        None
    }

    fn steal(&mut self, parent_ino: u64, name: &str) -> Option<Rc<Dentry>> {
        let key = hash_key(parent_ino, name);
        let bucket = Self::hash_bucket(key);
        for bi in 0..self.hash_table[bucket].len() {
            let idx = self.hash_table[bucket][bi];
            if let Some(ref dentry) = self.entries[idx] {
                if dentry.parent_ino == parent_ino && dentry.name == name {
                    let d = self.entries[idx].take();
                    self.hash_table[bucket].remove(bi);
                    self.stale_count += 1;
                    self.lru_index.remove(&idx);
                    self.count -= 1;
                    return d;
                }
            }
        }
        None
    }

    fn touch_lru(&mut self, idx: usize) {
        if let Some(&pos) = self.lru_index.get(&idx) {
            self.lru.swap_remove(pos);
            if pos < self.lru.len() {
                let swapped = self.lru[pos];
                self.lru_index.insert(swapped, pos);
            }
            self.lru.push(idx);
            self.lru_index.insert(idx, self.lru.len() - 1);
        }
    }

    pub fn shrink(&mut self, target: usize) {
        let mut evicted = 0;
        while self.count > target && !self.lru.is_empty() {
            let idx = self.lru.swap_remove(0);
            self.lru_index.remove(&idx);
            if self.entries[idx].is_some() {
                self.entries[idx] = None;
                self.stale_count += 1;
                self.count -= 1;
                evicted += 1;
            }
        }
        if evicted > 0 && self.should_compact() {
            self.compact();
        }
    }

    fn should_compact(&self) -> bool {
        let cap = self.entries.len();
        cap > 0 && self.stale_count > cap / DCACHE_STALE_CLEANUP_RATIO
    }

    fn maybe_compact(&mut self) {
        if self.should_compact() {
            self.compact();
        }
    }

    fn compact(&mut self) {
        let live: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].is_some())
            .collect();
        if live.len() == self.entries.len() {
            self.stale_count = 0;
            return;
        }
        let old_len = self.entries.len();
        for (new_idx, &old_idx) in live.iter().enumerate() {
            if new_idx != old_idx {
                self.entries.swap(new_idx, old_idx);
            }
        }
        self.entries.truncate(self.count);

        let mut remap = alloc::vec![None; old_len];
        for (new_idx, &old_idx) in live.iter().enumerate() {
            remap[old_idx] = Some(new_idx);
        }

        for bucket in self.hash_table.iter_mut() {
            for idx in bucket.iter_mut() {
                if let Some(&new_idx) = remap[*idx].as_ref() {
                    *idx = new_idx;
                }
            }
        }

        let mut new_lru = Vec::with_capacity(self.count);
        let mut new_lru_index = BTreeMap::new();
        for (pos, &idx) in live.iter().enumerate() {
            new_lru.push(idx);
            new_lru_index.insert(idx, pos);
        }
        self.lru = new_lru;
        self.lru_index = new_lru_index;
        self.stale_count = 0;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        for bucket in self.hash_table.iter_mut() {
            bucket.clear();
        }
        self.lru.clear();
        self.lru_index.clear();
        self.count = 0;
        self.stale_count = 0;
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

impl fmt::Debug for Dcache {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Dcache(entries={}, stale={})", self.count, self.stale_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dentry(name: &str, parent_ino: u64, ino: u64) -> Dentry {
        Dentry {
            name: String::from(name),
            parent_ino,
            ino,
            is_dir: false,
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: 0,
            generation: 0,
        }
    }

    #[test]
    fn test_lookup_alloc_delete_cycle() {
        let mut cache = Dcache::new();
        let d = dentry("file.txt", 1, 100);
        let rc = cache.alloc(d);
        assert_eq!(rc.name, "file.txt");
        assert_eq!(rc.ino, 100);
        assert_eq!(cache.len(), 1);

        let found = cache.lookup(1, "file.txt");
        assert!(found.is_some());
        assert_eq!(found.unwrap().ino, 100);

        cache.delete(1, "file.txt");
        assert_eq!(cache.len(), 0);
        assert!(cache.lookup(1, "file.txt").is_none());
    }

    #[test]
    fn test_lookup_returns_rc() {
        let mut cache = Dcache::new();
        let d = dentry("shared.txt", 1, 200);
        let rc1 = cache.alloc(d);
        let rc2 = cache.lookup(1, "shared.txt").unwrap();
        assert_eq!(rc1.ino, rc2.ino);
        assert_eq!(Rc::as_ptr(&rc1), Rc::as_ptr(&rc2));
    }

    #[test]
    fn test_lookup_cache_hit_updates_lru() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("a", 1, 10));
        cache.alloc(dentry("b", 1, 20));
        let last = cache.lru[cache.lru.len() - 1];
        cache.lookup(1, "a");
        assert_eq!(cache.lru[cache.lru.len() - 1], last);
    }

    #[test]
    fn test_lookup_miss_returns_none() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("a", 1, 10));
        assert!(cache.lookup(1, "nonexistent").is_none());
        assert!(cache.lookup(2, "a").is_none());
    }

    #[test]
    fn test_alloc_triggers_shrink_at_capacity() {
        let mut cache = Dcache::new();
        for i in 0..DCACHE_MAX_ENTRIES {
            let name = alloc::format!("file_{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        assert_eq!(cache.len(), DCACHE_MAX_ENTRIES);
        cache.alloc(dentry("overflow", 1, 9999));
        assert!(cache.len() <= DCACHE_MAX_ENTRIES);
    }

    #[test]
    fn test_delete_nonexistent() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("a", 1, 10));
        cache.delete(1, "nonexistent");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_rename_basic() {
        let mut cache = Dcache::new();
        let d = dentry("old_name", 1, 42);
        let rc = cache.alloc(d);
        let orig_ino = rc.ino;

        cache.rename(1, "old_name", 1, "new_name");
        assert_eq!(cache.len(), 1);

        assert!(cache.lookup(1, "old_name").is_none());
        let found = cache.lookup(1, "new_name");
        assert!(found.is_some());
        assert_eq!(found.unwrap().ino, orig_ino);
    }

    #[test]
    fn test_rename_cross_parent() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("file", 1, 42));
        cache.rename(1, "file", 2, "moved_file");

        assert!(cache.lookup(1, "file").is_none());
        let found = cache.lookup(2, "moved_file");
        assert!(found.is_some());
        assert_eq!(found.unwrap().ino, 42);
    }

    #[test]
    fn test_rename_same_name_noop() {
        let mut cache = Dcache::new();
        let d = dentry("file", 1, 42);
        let rc1 = cache.alloc(d);
        let ptr_before = Rc::as_ptr(&rc1);

        cache.rename(1, "file", 1, "file");
        assert_eq!(cache.len(), 1);

        let rc2 = cache.lookup(1, "file").unwrap();
        assert_eq!(Rc::as_ptr(&rc2), ptr_before);
    }

    #[test]
    fn test_rename_nonexistent() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("a", 1, 10));
        cache.rename(1, "nonexistent", 1, "b");
        assert_eq!(cache.len(), 1);
        assert!(cache.lookup(1, "a").is_some());
    }

    #[test]
    fn test_rename_atomic_invariant() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("old", 1, 42));
        cache.alloc(dentry("other", 1, 99));

        let before = cache.lookup(1, "other").unwrap();
        cache.rename(1, "old", 1, "new");
        let after = cache.lookup(1, "other").unwrap();
        assert_eq!(Rc::as_ptr(&before), Rc::as_ptr(&after));
    }

    #[test]
    fn test_lookup_after_delete() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("a", 1, 10));
        cache.delete(1, "a");
        assert!(cache.lookup(1, "a").is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_shrink_evicts_lru() {
        let mut cache = Dcache::new();
        for i in 0..100 {
            let name = alloc::format!("f{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        assert_eq!(cache.len(), 100);
        cache.shrink(50);
        assert_eq!(cache.len(), 50);
    }

    #[test]
    fn test_shrink_no_op_when_under_target() {
        let mut cache = Dcache::new();
        for i in 0..10 {
            let name = alloc::format!("f{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        cache.shrink(50);
        assert_eq!(cache.len(), 10);
    }

    #[test]
    fn test_shrink_stale_entries_skipped() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("keep", 1, 10));
        cache.alloc(dentry("evict", 1, 20));
        cache.delete(1, "evict");

        let stale_before = cache.stale_count;
        assert!(stale_before > 0);

        cache.shrink(1);
        assert_eq!(cache.len(), 1);
        assert!(cache.lookup(1, "keep").is_some());
    }

    #[test]
    fn test_multiple_alloc_different_buckets() {
        let mut cache = Dcache::new();
        for i in 0..100 {
            let name = alloc::format!("{}", i);
            cache.alloc(dentry(&name, i as u64, i as u64));
        }
        assert_eq!(cache.len(), 100);
        for i in 0..100 {
            let name = alloc::format!("{}", i);
            let found = cache.lookup(i as u64, &name);
            assert!(found.is_some(), "missing entry {}", i);
        }
    }

    #[test]
    fn test_compact_reclaims_stale_entries() {
        let mut cache = Dcache::new();
        for i in 0..20 {
            let name = alloc::format!("f{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        for i in 0..10 {
            let name = alloc::format!("f{}", i);
            cache.delete(1, &name);
        }
        let stale = cache.stale_count;
        assert!(stale > 0);
        let old_cap = cache.entries.len();
        cache.compact();
        assert_eq!(cache.stale_count, 0);
        assert!(cache.entries.len() < old_cap);
        assert_eq!(cache.len(), 10);
    }

    #[test]
    fn test_clear_resets_all_state() {
        let mut cache = Dcache::new();
        for i in 0..50 {
            let name = alloc::format!("f{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.stale_count, 0);
        assert_eq!(cache.entries.len(), 0);
        assert!(cache.lookup(1, "f0").is_none());
    }

    #[test]
    fn test_compact_no_stale_is_noop() {
        let mut cache = Dcache::new();
        for i in 0..10 {
            let name = alloc::format!("f{}", i);
            cache.alloc(dentry(&name, 1, i as u64));
        }
        let old_cap = cache.entries.len();
        cache.compact();
        assert_eq!(cache.entries.len(), old_cap);
        assert_eq!(cache.stale_count, 0);
    }

    #[test]
    fn test_rename_to_existing_replaces() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("old", 1, 42));
        cache.alloc(dentry("existing", 1, 99));

        cache.rename(1, "old", 1, "existing");
        assert!(cache.lookup(1, "old").is_none());
        let found = cache.lookup(1, "existing").unwrap();
        assert_eq!(found.ino, 42);
    }

    #[test]
    fn test_steal_removes_completely() {
        let mut cache = Dcache::new();
        cache.alloc(dentry("victim", 1, 42));
        let stolen = cache.steal(1, "victim");
        assert!(stolen.is_some());
        assert_eq!(stolen.unwrap().ino, 42);
        assert_eq!(cache.len(), 0);
        assert!(cache.lookup(1, "victim").is_none());
    }
}
