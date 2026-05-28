use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

const ICACHE_MAX_ENTRIES: usize = 16384;
const ICACHE_SHRINK_BATCH: usize = 512;

#[derive(Clone, Debug)]
pub struct CachedInode {
    pub ino: u64,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
    pub nlink: u32,
    pub generation: u64,
    pub is_dir: bool,
    pub blocks: u64,
    pub rdev: u64,
}

pub struct Icache {
    entries: BTreeMap<u64, CachedInode>,
    lru: Vec<u64>,
    lru_index: BTreeMap<u64, usize>,
    count: usize,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl Icache {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            lru: Vec::new(),
            lru_index: BTreeMap::new(),
            count: 0,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn iget(&mut self, ino: u64) -> Option<CachedInode> {
        if self.entries.contains_key(&ino) {
            self.touch_lru(ino);
            self.hits.fetch_add(1, Ordering::Relaxed);
            self.entries.get(&ino).cloned()
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    pub fn insert(&mut self, inode: CachedInode) {
        let ino = inode.ino;
        if self.entries.contains_key(&ino) {
            return;
        }
        if self.count >= ICACHE_MAX_ENTRIES {
            self.shrink(self.count.saturating_sub(ICACHE_SHRINK_BATCH));
        }
        self.entries.insert(ino, inode);
        self.lru.push(ino);
        self.lru_index.insert(ino, self.lru.len() - 1);
        self.count = self.entries.len();
    }

    pub fn update(&mut self, inode: CachedInode) {
        let ino = inode.ino;
        if self.entries.contains_key(&ino) {
            self.entries.insert(ino, inode);
            self.touch_lru(ino);
        }
    }

    pub fn remove(&mut self, ino: u64) -> bool {
        if self.entries.remove(&ino).is_some() {
            if let Some(&pos) = self.lru_index.get(&ino) {
                self.lru.swap_remove(pos);
                if pos < self.lru.len() {
                    if let Some(&swapped) = self.lru.get(pos) {
                        self.lru_index.insert(swapped, pos);
                    }
                }
                self.lru_index.remove(&ino);
            }
            self.count = self.entries.len();
            true
        } else {
            false
        }
    }

    pub fn contains(&self, ino: u64) -> bool {
        self.entries.contains_key(&ino)
    }

    fn touch_lru(&mut self, ino: u64) {
        if let Some(&pos) = self.lru_index.get(&ino) {
            self.lru.swap_remove(pos);
            if pos < self.lru.len() {
                let swapped = self.lru[pos];
                self.lru_index.insert(swapped, pos);
            }
            self.lru.push(ino);
            self.lru_index.insert(ino, self.lru.len() - 1);
        }
    }

    pub fn shrink(&mut self, target: usize) {
        while self.count > target && !self.lru.is_empty() {
            let ino = self.lru.remove(0);
            self.lru_index.remove(&ino);
            self.entries.remove(&ino);
            self.count = self.entries.len();
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.lru_index.clear();
        self.count = 0;
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits.load(Ordering::Relaxed), self.misses.load(Ordering::Relaxed))
    }
}
