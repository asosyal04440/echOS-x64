use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

const DCACHE_HASH_BITS: usize = 13;
const DCACHE_HASH_SIZE: usize = 1 << DCACHE_HASH_BITS;
const DCACHE_MAX_ENTRIES: usize = 32768;
const DCACHE_SHRINK_BATCH: usize = 1024;

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
    entries: Vec<Option<Dentry>>,
    hash_table: [Vec<usize>; DCACHE_HASH_SIZE],
    lru: Vec<usize>,
    lru_index: BTreeMap<usize, usize>,
    count: usize,
    access_counter: u64,
}

impl Dcache {
    pub const fn new() -> Self {
        const EMPTY_VEC: Vec<usize> = Vec::new();
        Self {
            entries: Vec::new(),
            hash_table: [EMPTY_VEC; DCACHE_HASH_SIZE],
            lru: Vec::new(),
            lru_index: BTreeMap::new(),
            count: 0,
            access_counter: 0,
        }
    }

    fn hash_bucket(key: HashKey) -> usize {
        (key.0 as usize) & (DCACHE_HASH_SIZE - 1)
    }

    pub fn lookup(&mut self, parent_ino: u64, name: &str) -> Option<Dentry> {
        let key = hash_key(parent_ino, name);
        let bucket = Self::hash_bucket(key);
        let indices: Vec<usize> = self.hash_table[bucket].clone();
        let found = indices.iter().find(|&&idx| {
            self.entries[idx].as_ref().is_some_and(|d| d.parent_ino == parent_ino && d.name == name)
        }).copied();
        if let Some(idx) = found {
            self.touch_lru(idx);
            self.entries[idx].clone()
        } else {
            None
        }
    }

    pub fn alloc(&mut self, dentry: Dentry) -> usize {
        if self.count >= DCACHE_MAX_ENTRIES {
            self.shrink(DCACHE_SHRINK_BATCH);
        }
        let idx = self.entries.len();
        let key = hash_key(dentry.parent_ino, &dentry.name);
        let bucket = Self::hash_bucket(key);
        self.entries.push(Some(dentry));
        self.hash_table[bucket].push(idx);
        self.lru.push(idx);
        self.lru_index.insert(idx, self.lru.len() - 1);
        self.count += 1;
        idx
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
        }
    }

    pub fn rename(&mut self, old_parent: u64, old_name: &str, new_parent: u64, new_name: &str) {
        self.delete(old_parent, old_name);
        if let Some(mut dentry) = self.steal(old_parent, old_name) {
            dentry.parent_ino = new_parent;
            dentry.name = String::from(new_name);
            self.alloc(dentry);
        }
    }

    fn steal(&mut self, parent_ino: u64, name: &str) -> Option<Dentry> {
        let key = hash_key(parent_ino, name);
        let bucket = Self::hash_bucket(key);
        for bi in 0..self.hash_table[bucket].len() {
            let idx = self.hash_table[bucket][bi];
            if let Some(ref dentry) = self.entries[idx] {
                if dentry.parent_ino == parent_ino && dentry.name == name {
                    let d = self.entries[idx].take();
                    self.hash_table[bucket].remove(bi);
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
            let idx = self.lru.remove(0);
            self.lru_index.remove(&idx);
            if let Some(dentry) = self.entries[idx].take() {
                let key = hash_key(dentry.parent_ino, &dentry.name);
                let bucket = Self::hash_bucket(key);
                self.hash_table[bucket].retain(|&i| i != idx);
                self.count -= 1;
                evicted += 1;
            }
        }
        if evicted > 0 {
            self.compact();
        }
    }

    fn compact(&mut self) {
        let mut live: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].is_some())
            .collect();
        if live.len() == self.count {
            return;
        }
        let old_len = self.entries.len();
        for (new_idx, &old_idx) in live.iter().enumerate() {
            if new_idx != old_idx {
                self.entries.swap(new_idx, old_idx);
            }
        }
        self.entries.truncate(self.count);
        for bucket in self.hash_table.iter_mut() {
            for idx in bucket.iter_mut() {
                if let Some(pos) = live.iter().position(|&x| x == *idx) {
                    *idx = pos;
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
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        for bucket in self.hash_table.iter_mut() {
            bucket.clear();
        }
        self.lru.clear();
        self.lru_index.clear();
        self.count = 0;
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

impl fmt::Debug for Dcache {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Dcache(entries={})", self.count)
    }
}
