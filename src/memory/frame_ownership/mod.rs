//! # Adaptive Typed Frame Ownership (ATFO) — Refcount Contract
//!
//! This module implements typed physical-frame ownership and reference counting
//! for echOS.  Every 4 KiB physical frame is tracked by a [`FrameTable`] entry
//! carrying a [`FrameFlags`] tag and an [`AtomicU32`] refcount.  The table is
//! accessed through the global [`FRAME_TABLE`] singleton.
//!
//! ---------------------------------------------------------------------------
//! ## Frame Lifecycle States
//!
//! A frame enters the table through one of three constructors:
//! | Constructor | Initial Flags | Initial Refcount | Source |
//! |---|---|---|---|
//! | `init_unique` | UNIQUE | 0 | `UniqueFrame::from_phys` |
//! | `share` (no prior entry) | SHARED | 1 | `SharedAtomicFrame::from_phys`, direct `incref`, or `into_shared` on a non‑tracked frame |
//! | `refcache_init` | REFCACHE | 0 | `UniqueFrame::into_shared_refcache`, or `SharedRefCacheFrame::from_phys` |
//!
//! A frame is released (entry removed from the table) when:
//! * SHARED: `unshare` decrements the count from 1 → 0.
//! * UNIQUE: `unshare` removes unconditionally and returns 0.
//! * ZOMBIE: `unshare` removes and returns 0 (ZOMBIE can only be set by `UniqueFrame::Drop`).
//! * REFCACHE: `refcache_try_remove` succeeds only when the global count is 0.
//!
//! ---------------------------------------------------------------------------
//! ## Refcount Accounting (Normative)
//!
//! Every reference to a frame is owned by exactly one holder.  When the holder
//! releases the reference it MUST perform the corresponding decrement.  No
//! reference is ever double‑accounted.
//!
//! | Holder | Operation (acquire) | Δ | Operation (release) | Δ |
//! |---|---|---|---|---|
//! | Page‑table entry (PTE / PMD / PDPTE) | `share` / `refcache_inc` | **+1** | `unshare` / `refcache_dec` | **−1** |
//! | `SharedAnonPages` / `SharedFilePages` cache | `into_shared` | **+1** | `SharedAtomicFrame::Drop` | **−1** |
//! | `SharedAtomicFrame` handle (clone) | `SharedAtomicFrame::Clone` | **+1** | `SharedAtomicFrame::Drop` | **−1** |
//! | `SharedRefCacheFrame` handle (clone) | `SharedRefCacheFrame::Clone` | **+1** (per‑CPU) | `SharedRefCacheFrame::Drop` | **−1** (per‑CPU) |
//! | Temporary pin (core::mem::forget + page‑table alias) | `SharedAtomicFrame::Clone` + forget | **+1** | `dec_frame_ref` at unmap | **−1** |
//! | Swap cache entry (`Vec<u8>`) | — | **0** (data‑only) | — | **0** |
//! | VFS page cache entry (`Vec<u8>`) | — | **0** (data‑only) | — | **0** |
//! | LRU tracking | — | **0** (no refcount effect) | — | **0** |
//!
//! A **physical frame is freed** (returned to the allocator) exactly when its
//! refcount reaches 0 AND the FRAME_TABLE entry is removed:
//! * SHARED: synchronously in `unshare` or `SharedAtomicFrame::Drop`.
//! * REFCACHE: asynchronously by the per‑CPU refcache epoch‑based review queue
//!   (`refcache::tick` + `refcache_drain`).
//!
//! ---------------------------------------------------------------------------
//! ## Per‑Type Lifecycle Tables
//!
//! ### 1. Anonymous Page (private, `shared_id == 0`)
//!
//! ```
//! Alloc:    allocate_user_frame → init_unique(UNIQUE,ref=0)
//!           frame.fill(0)
//!           frame.leak_as_shared()                → share → SHARED,ref=1   [+1  PTE]
//! Reclaim:  dec_frame_ref(phys)                   → unshare                [−1  PTE]
//!           if ref==0: deallocate
//! Swap-in:  allocate_user_frame → leak_as_shared  → share → SHARED,ref=1   [+1  PTE]
//! Unmap:    dec_frame_ref(phys)                   → unshare                [−1  PTE]
//!           if ref==0: deallocate
//! Teardown: free_user_page_tables → dec_frame_ref → unshare                [−1  PTE]
//! ```
//!
//! ### 2. Anonymous Page (shared, `shared_id != 0` → SHARED_ANON cache)
//!
//! ```
//! Cache hit (map from existing):
//!   shared = SHARED_ANON_PAGES.get(key)           → lookup (no refcount effect)
//!   shared.clone()                                → fetch_add(1) ref=2     [+1  clone]
//!   core::mem::forget(shared.clone())             → leak to PTE            [PTE owns +1]
//!
//! Cache miss (first fault):
//!   allocate_user_frame → init_unique(UNIQUE,ref=0)
//!   frame.fill(0)
//!   frame.into_shared()                           → promote SHARED,ref=1   [+1  cache entry]
//!   cache.insert(key, shared)                     → BTreeMap stores SAF
//!
//! Reclaim (swap-out):
//!   dec_frame_ref(phys)                           → unshare                [−1  PTE]
//!   if ref==0:
//!     SHARED_ANON_PAGES.remove(key)               → SAF::Drop → unshare    [−1  cache entry]
//!     deallocate
//!
//! Unmap:
//!   dec_frame_ref(phys)                           → unshare                [−1  PTE]
//!   if ref==0:
//!     SHARED_ANON_PAGES.remove(key)               → SAF::Drop → unshare    [−1  cache entry]
//!     deallocate
//! ```
//!
//! ### 3. File‑Backed Page (private, `region.shared == false`)
//!
//! ```
//! Map (cache data filled from VFS):
//!   allocate_user_frame → init_unique(UNIQUE,ref=0)
//!   frame.fill(0) + read_cached_file_page → data copied, no ref
//!   frame.leak_as_shared()                       → share → SHARED,ref=1   [+1  PTE]
//! Reclaim:  dec_frame_ref(phys)                  → unshare                [−1  PTE]
//! Unmap:    dec_frame_ref(phys)                  → unshare                [−1  PTE]
//! ```
//!
//! ### 4. File‑Backed Page (shared → SHARED_FILE cache)
//!
//! ```
//! Cache hit (map from existing):
//!   shared = SHARED_FILE_PAGES.get(key)          → lookup
//!   shared.clone()                               → fetch_add(1)           [+1  clone]
//!   core::mem::forget(shared.clone())            → leak to PTE            [PTE owns +1]
//!
//! Cache miss:
//!   allocate_user_frame → init_unique(UNIQUE,ref=0)
//!   frame.fill(0) + read_cached_file_page
//!   frame.into_shared()                          → promote SHARED,ref=1   [+1  cache entry]
//!   cache.insert(key, shared)                    → BTreeMap stores SAF
//!
//! Reclaim / Unmap:
//!   dec_frame_ref(phys)                          → unshare                [−1  PTE]
//!   if ref==0:
//!     SHARED_FILE_PAGES.remove(key)              → SAF::Drop → unshare    [−1  cache entry]
//!     deallocate
//! ```
//!
//! ### 5. COW (Copy‑on‑Write)
//!
//! Two sub‑paths depending on whether the single reference can be upgraded:
//!
//! **Fast path** (`try_upgrade_unique` succeeds — only 1 reference exists):
//! ```
//!   old_phys: SHARED,ref=1 → try_upgrade_unique → UNIQUE,ref=0
//!   update_page_flags_with_split → add WRITABLE
//!   (no allocation, no new frame)
//! ```
//!
//! **Slow path** (copy — refcount > 1 or forced):
//! ```
//!   new_frame = allocate_user_frame               → init_unique(UNIQUE,ref=0)
//!   copy data old→new
//!   mapper.unmap(old_page)                       → PTE removed (no refcount change)
//!   if old_phys is NOT REFCACHE:
//!     drop(SAF::from_phys_inner(old_phys))       → unshare                [−1  old PTE ref]
//!     (if unshare returns 0, deallocate old)
//!   new_frame.into_shared_refcache()             → refcache_init(REFCACHE,ref=0)
//!                                                   + refcache_inc(+1)    [+1  new PTE, per‑CPU]
//!   core::mem::forget(returned SRF)              → leak to PTE
//! Future unmap of new COW page:
//!   dec_frame_ref(new_phys) → REFCACHE → refcache_dec  [−1  per‑CPU, deferred]
//!   refcache::tick → refcache_try_free(global=0) → drain → deallocate
//! ```
//!
//! ### 6. Zero Page (singleton shared read‑only page)
//!
//! echOS maps a single shared read‑only zero page for private anonymous
//! VMAs on first read fault, avoiding per‑process frame allocation.
//!
//! ```text
//! Init:    allocate_contiguous_frames(1) → zero it
//!          frame_ownership::pin_frame(phys) → SHARED,ref=u32::MAX/2
//!
//! Read fault (anon private, cow=true):
//!          map zero page read‑only                              [no refcount change]
//!          register_lru_mapping(zero_pfn)                       [LRU not yet wired]
//!
//! Write fault (COW):
//!          handle_cow_fault → try_upgrade_unique → false        [ref ≫ 1]
//!          allocate new frame, copy zeros, map writable
//!          drop(SAF::from_phys_inner(phys)) → unshare           [−1]
//!          refcount still ≫ 0 → zero page NOT freed
//!
//! Unmap / reclaim:
//!          dec_frame_ref(zero_pfn) → unshare                    [−1]
//!          refcount still ≫ 0 → zero page NOT freed
//! ```
//!
//! The zero page is permanently pinned via `pin_frame` and can never reach
//! refcount 0.  It is allocated once at boot in `init_memory_subsystems`.
//! The separate helper `alloc_zeroed_page` (mod.rs:4460) remains available
//! for transient kernel‑internal allocations; it is unrelated to the shared
//! zero page.
//!
//! ### 7. THP (Transparent Huge Pages) — 2 MiB → 512 sub‑pages
//!
//! ```
//! Allocate 2 MiB region: allocate_contiguous_huge_frame → raw PhysFrame (no init_unique)
//! Zero-fill 2 MiB
//! Map as 2 MiB huge page
//! For each of 512 sub‑pages:
//!   SAF::incref(sub_phys_addr)                      → share → SHARED,ref=1 [+1 × 512  PTE sub‑refs]
//! Unmap / split:
//!   split_huge_page → convert 2 MiB → 512×4 KiB
//!   dec_frame_ref(sub_phys) for each sub‑page       → unshare              [−1 × 512  PTE]
//! Teardown (free_user_page_tables, huge‑page branch):
//!   for k in 0..512:
//!     dec_frame_ref(sub_phys)                       → unshare              [−1 × 512  PTE]
//!     if ref==0: deallocate
//! ```
//!
//! Key difference: THP sub‑pages skip `init_unique` — `incref` inserts each
//! sub‑page directly as SHARED,ref=1 if no prior entry exists.
//!
//! ### 8. Swap Cache
//!
//! The swap cache holds **data buffers** (`Vec<u8>`), not frame references.
//! `swap_take_page` and `swap_store_page` never touch FRAME_TABLE.  The
//! refcount effect at swap‑in/out comes entirely from the caller:
//!
//! * Swap‑in (restore): `allocate_user_frame → leak_as_shared` → +1 (PTE).
//! * Swap‑out (reclaim): `dec_frame_ref` → −1 (PTE removed), then
//!   `swap_store_page` stores the data.
//!
//! ### 9. VFS Page Cache
//!
//! The VFS‑level `PAGE_CACHE` (mod.rs `PageCache`) is a separate data‑only
//! `BTreeMap<(usize,u64), Vec<u8>>`.  It never touches FRAME_TABLE.  Physical
//! frame refcounting for cached file pages is handled through SHARED_FILE_PAGES
//! (Section 4) — the data cache and the frame cache are independent.
//!
//! ---------------------------------------------------------------------------
//! ## Cross‑Cutting: All `share` / `incref` Sites
//!
//! | File | Line | Operation | Context |
//! |---|---|---|---|
//! | `unique.rs` | 56 | `FRAME_TABLE.lock().share(phys)` | `UniqueFrame::into_shared` — UNIQUE→SHARED promote, ref=1 |
//! | `unique.rs` | 69 | `refcache::inc(phys)` | `UniqueFrame::into_shared_refcache` — per‑CPU +1 |
//! | `unique.rs` | 79 | `FRAME_TABLE.lock().share(self.phys)` | `UniqueFrame::leak_as_shared` — leak to PTE |
//! | `shared_atomic.rs` | 17 | `FRAME_TABLE.lock().share(phys)` | `SharedAtomicFrame::from_phys` — constructor |
//! | `shared_atomic.rs` | 47 | `FRAME_TABLE.lock().share(phys)` | `SharedAtomicFrame::incref` — static helper |
//! | `shared_atomic.rs` | 61 | `FRAME_TABLE.lock().share(self.phys)` | `SharedAtomicFrame::Clone` |
//! | `shared_refcache.rs` | 33 | `refcache::inc(self.phys)` | `SharedRefCacheFrame::Clone` — per‑CPU +1 |
//! | `mod.rs` (memory) | 1504 | `SAF::<[u8;4096]>::incref(phys_addr)` | THP sub‑page map — +1 per sub‑page |
//! | `mod.rs` (memory) | 2035 | `SAF::<[u8;4096]>::incref(phys.as_u64())` | mprotect / COW‑clone for each cow+shared page |
//!
//! ## Cross‑Cutting: All `unshare` / `decref` / `dec` Sites
//!
//! | File | Line | Operation | Context |
//! |---|---|---|---|
//! | `mod.rs` (memory) | 2672 | `dec_frame_ref(phys_unmapped)` | LRU reclaim — every evicted page |
//! | `mod.rs` (memory) | 2901 | `dec_frame_ref(phys)` | `unmap_user_range` — VMA unmap |
//! | `mod.rs` (memory) | 2949 | `dec_frame_ref(sub_phys)` | `free_user_page_tables` — THP huge‑page sub‑pages |
//! | `mod.rs` (memory) | 2969 | `dec_frame_ref(leaf_phys)` | `free_user_page_tables` — 4 KiB leaf PTEs |
//! | `mod.rs` (memory) | 3879 | `drop(SAF::from_phys_inner(old_phys))` | COW slow path — old frame ref released |
//! | `shared_atomic.rs` | 71 | `FRAME_TABLE.lock().unshare(self.phys)` | `SharedAtomicFrame::Drop` |
//! | `shared_refcache.rs` | 43 | `refcache::dec(self.phys)` | `SharedRefCacheFrame::Drop` — per‑CPU −1 |
//!
//! ---------------------------------------------------------------------------
//! ## Invariants (enforced by the type system + runtime assertions)
//!
//! 1. **No double free**: every `share` has exactly one matching `unshare`
//!    (or `refcache_inc` → `refcache_dec`).  Violation causes either a
//!    premature deallocation (UAF) or a frame leak.
//! 2. **PTE references are leaked**: when `core::mem::forget(shared.clone())`
//!    is used, the page‑table reference has no Rust RAII guard.  Every unmap
//!    path MUST call `dec_frame_ref` to compensate.
//! 3. **COW‑new frames are REFCACHE‑managed**: the per‑CPU deferred delta
//!    system owns their lifecycle.  Synchronous `dec_frame_ref` for a REFCACHE
//!    frame routes to `refcache_dec` and never deallocates inline.
//! 4. **ZOMBIE frames are deallocated once**: `UniqueFrame::Drop` sets ZOMBIE
//!    synchronously AND calls `deallocate_contiguous_frames`.  Any subsequent
//!    `unshare` on a ZOMBIE entry removes it but does NOT free again.
//! 5. **THP sub‑pages are tracked independently**: each 4 KiB sub‑page of a
//!    2 MiB THP allocation has its own FRAME_TABLE entry.  The huge‑page is
//!    freed only when all 512 sub‑page entries reach 0.
//! 6. **Shared‑cache entries hold one permanent reference**: the
//!    `SharedAtomicFrame` stored in `SHARED_ANON_PAGES` or `SHARED_FILE_PAGES`
//!    keeps refcount ≥ 1 as long as the entry exists.  Removal happens only
//!    when the last PTE reference is released AND the cache entry is dropped.
//!
//! ---------------------------------------------------------------------------
//! ## Safety
//!
//! The `FRAME_TABLE` global is protected by a `spin::Mutex`.  All public
//! functions in this module are safe to call from any context (including
//! interrupt handlers) as long as the caller holds no other lock that could
//! cause a deadlock with FRAME_TABLE.  Per‑CPU refcache operations are
//! lock‑free on the fast path but use unsafe `static mut` access to the CPU
//! slots; these are single‑threaded per CPU and preemption is disabled during
//! refcache operations via `HardIRQGuard` in the interrupt path.

pub(crate) use refcache::RefCacheCpuState;
pub(crate) use shared_atomic::SharedAtomicFrame;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicU32, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

mod refcache;
mod shared_atomic;
mod shared_refcache;
mod unique;

pub(crate) use shared_refcache::SharedRefCacheFrame;
pub(crate) use unique::UniqueFrame;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct FrameFlags: u8 {
        const UNIQUE       = 0b0001;
        const SHARED       = 0b0010;
        const ZOMBIE       = 0b0100;
        const REFCACHE     = 0b1000;
        const FOLIO_HEAD   = 0b0001_0000;
        const FOLIO_TAIL   = 0b0010_0000;
    }
}

#[derive(Debug)]
pub(crate) struct FrameEntry {
    flags: FrameFlags,
    pub(crate) order: u8,
    refcount: AtomicU32,
}

impl FrameEntry {
    fn new_unique() -> Self {
        Self {
            flags: FrameFlags::UNIQUE,
            order: 0,
            refcount: AtomicU32::new(0),
        }
    }

    fn new_shared() -> Self {
        Self {
            flags: FrameFlags::SHARED,
            order: 0,
            refcount: AtomicU32::new(1),
        }
    }

    pub(crate) fn refcount(&self) -> u32 {
        self.refcount.load(Ordering::Relaxed)
    }

    pub(crate) fn folio_order(&self) -> u8 {
        self.order
    }

    pub(crate) fn set_folio_order(&mut self, order: u8) {
        self.order = order;
    }

    fn promote(&mut self) {
        self.flags = FrameFlags::SHARED;
        self.refcount.store(1, Ordering::Relaxed);
    }
}

pub(crate) struct FrameTable {
    entries: BTreeMap<u64, FrameEntry>,
}

impl FrameTable {
    pub(crate) const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn init_unique(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        self.entries.entry(key).or_insert_with(FrameEntry::new_unique);
    }

    pub(crate) fn share(&mut self, phys: u64) -> u32 {
        let key = phys & !(0xFFF);
        match self.entries.get_mut(&key) {
            Some(entry) if entry.flags == FrameFlags::UNIQUE => {
                entry.promote();
                1
            }
            Some(entry) if entry.flags == FrameFlags::SHARED => {
                entry.refcount.fetch_add(1, Ordering::Relaxed);
                entry.refcount.load(Ordering::Relaxed)
            }
            Some(entry) if entry.flags == FrameFlags::ZOMBIE => {
                entry.flags = FrameFlags::SHARED;
                entry.refcount.store(2, Ordering::Relaxed);
                2
            }
            _ => {
                self.entries.entry(key).or_insert_with(FrameEntry::new_shared);
                self.entries[&key].refcount.load(Ordering::Relaxed)
            }
        }
    }

    pub(crate) fn unshare(&mut self, phys: u64) -> u32 {
        let key = phys & !(0xFFF);
        match self.entries.get_mut(&key) {
            Some(entry) if entry.flags == FrameFlags::UNIQUE => {
                let _ = entry.refcount.load(Ordering::Relaxed);
                self.entries.remove(&key);
                0
            }
            Some(entry) if entry.flags == FrameFlags::SHARED => {
                let prev = entry.refcount.fetch_sub(1, Ordering::Relaxed);
                if prev <= 1 {
                    self.entries.remove(&key);
                    0
                } else {
                    prev - 1
                }
            }
            Some(entry) if entry.flags == FrameFlags::ZOMBIE => {
                self.entries.remove(&key);
                0
            }
            _ => 0,
        }
    }

    pub(crate) fn refcount(&self, phys: u64) -> u32 {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.refcount.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub(crate) fn flags(&self, phys: u64) -> FrameFlags {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.flags)
            .unwrap_or(FrameFlags::empty())
    }

    pub(crate) fn is_unique(&self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.flags == FrameFlags::UNIQUE)
            .unwrap_or(false)
    }

    pub(crate) fn is_shared(&self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.flags == FrameFlags::SHARED)
            .unwrap_or(false)
    }

    pub(crate) fn try_upgrade_unique(&mut self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        match self.entries.get_mut(&key) {
            Some(entry) if entry.flags == FrameFlags::SHARED && entry.refcount.load(Ordering::Relaxed) == 1 => {
                entry.flags = FrameFlags::UNIQUE;
                entry.refcount.store(0, Ordering::Relaxed);
                true
            }
            Some(entry) if entry.flags == FrameFlags::SHARED && entry.refcount.load(Ordering::Relaxed) == 0 => {
                self.entries.remove(&key);
                true
            }
            Some(_) => false,
            None => false,
        }
    }

    pub(crate) fn mark_zombie(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.flags = FrameFlags::ZOMBIE;
        }
    }

    /// Pin a frame so it can never be freed.  Sets the entry to SHARED with
    /// an extremely high refcount (`u32::MAX / 2`).  Every subsequent
    /// `unshare` will decrement from this ceiling — it will never reach 0.
    pub(crate) fn pin(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        let refcount = u32::MAX / 2;
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.flags = FrameFlags::SHARED;
                entry.refcount.store(refcount, Ordering::Relaxed);
            }
            None => {
                self.entries.insert(
                    key,
                    FrameEntry {
                        flags: FrameFlags::SHARED,
                        order: 0,
                        refcount: AtomicU32::new(refcount),
                    },
                );
            }
        }
    }

    pub(crate) fn remove(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        self.entries.remove(&key);
    }

    #[allow(dead_code)]
    pub(crate) fn folio_mark_head(&mut self, phys: u64, order: u8) {
        let key = phys & !(0xFFF);
        let entry = self.entries.get_mut(&key);
        if let Some(entry) = entry {
            entry.flags.insert(FrameFlags::FOLIO_HEAD);
            entry.order = order;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn folio_mark_tail(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.flags.insert(FrameFlags::FOLIO_TAIL);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn folio_clear(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.flags.remove(FrameFlags::FOLIO_HEAD);
            entry.flags.remove(FrameFlags::FOLIO_TAIL);
            entry.order = 0;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_folio_head(&self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.flags.contains(FrameFlags::FOLIO_HEAD))
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub(crate) fn is_folio_tail(&self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        self.entries
            .get(&key)
            .map(|e| e.flags.contains(FrameFlags::FOLIO_TAIL))
            .unwrap_or(false)
    }

    pub(crate) fn refcache_init(&mut self, phys: u64) {
        let key = phys & !(0xFFF);
        self.entries.entry(key).or_insert_with(|| FrameEntry {
            flags: FrameFlags::REFCACHE,
            order: 0,
            refcount: AtomicU32::new(0),
        });
    }

    pub(crate) fn refcache_global_add(&mut self, phys: u64, delta: i32) -> u32 {
        let key = phys & !(0xFFF);
        let entry = self.entries.get_mut(&key);
        match entry {
            Some(entry) if entry.flags == FrameFlags::UNIQUE => {
                entry.flags = FrameFlags::REFCACHE;
                let new_cnt = if delta > 0 { 1u32 } else { 0u32 };
                entry.refcount.store(new_cnt, Ordering::Relaxed);
                new_cnt
            }
            Some(entry) if entry.flags == FrameFlags::REFCACHE || entry.flags == FrameFlags::SHARED => {
                let old = entry.refcount.load(Ordering::Relaxed);
                let new = if delta > 0 {
                    old.saturating_add(delta as u32)
                } else {
                    old.saturating_sub((-delta) as u32)
                };
                entry.refcount.store(new, Ordering::Relaxed);
                entry.flags = FrameFlags::REFCACHE;
                new
            }
            Some(entry) if entry.flags == FrameFlags::ZOMBIE => {
                let new_cnt = if delta > 0 { delta as u32 } else { 0u32 };
                entry.refcount.store(new_cnt, Ordering::Relaxed);
                entry.flags = FrameFlags::REFCACHE;
                new_cnt
            }
            _ => {
                let new_cnt = if delta > 0 { delta as u32 } else { 0u32 };
                self.entries.insert(key, FrameEntry {
                    flags: FrameFlags::REFCACHE,
                    order: 0,
                    refcount: AtomicU32::new(new_cnt),
                });
                new_cnt
            }
        }
    }

    pub(crate) fn refcache_try_remove(&mut self, phys: u64) -> bool {
        let key = phys & !(0xFFF);
        match self.entries.get(&key) {
            Some(entry) if entry.flags == FrameFlags::REFCACHE || entry.flags == FrameFlags::SHARED => {
                let cnt = entry.refcount.load(Ordering::Relaxed);
                if cnt == 0 {
                    self.entries.remove(&key);
                    true
                } else {
                    false
                }
            }
            Some(_) => false,
            None => false,
        }
    }
}

lazy_static! {
    pub(crate) static ref FRAME_TABLE: Mutex<FrameTable> = Mutex::new(FrameTable::new());
}

fn frame_key(phys: u64) -> u64 {
    phys & !(0xFFF)
}

pub(crate) fn pin_frame(phys: u64) {
    FRAME_TABLE.lock().pin(phys);
}

pub(crate) fn dec_frame_ref(phys: u64) -> u32 {
    FRAME_TABLE.lock().unshare(phys)
}

pub(crate) fn frame_refcount(phys: u64) -> u32 {
    FRAME_TABLE.lock().refcount(phys)
}

pub(crate) fn frame_flags(phys: u64) -> FrameFlags {
    FRAME_TABLE.lock().flags(phys)
}

pub(crate) fn try_upgrade_unique(phys: u64) -> bool {
    FRAME_TABLE.lock().try_upgrade_unique(phys)
}

pub(crate) fn refcache_flush_delta(phys: u64, delta: i32) -> u32 {
    FRAME_TABLE.lock().refcache_global_add(phys, delta)
}

pub(crate) fn refcache_try_free(phys: u64) -> bool {
    FRAME_TABLE.lock().refcache_try_remove(phys)
}

pub(crate) fn refcache_init_global(phys: u64) {
    FRAME_TABLE.lock().refcache_init(phys);
}

pub(crate) fn refcache_inc(phys: u64) {
    refcache::inc(phys);
}

pub(crate) fn refcache_dec(phys: u64) {
    refcache::dec(phys);
}

pub(crate) fn refcache_tick() {
    refcache::tick();
}

pub(crate) fn refcache_drain() {
    refcache::drain();
}

pub(crate) fn folio_mark_head(phys: u64, order: u8) {
    FRAME_TABLE.lock().folio_mark_head(phys, order);
}

pub(crate) fn folio_mark_tail(phys: u64) {
    FRAME_TABLE.lock().folio_mark_tail(phys);
}

pub(crate) fn folio_clear(phys: u64) {
    FRAME_TABLE.lock().folio_clear(phys);
}

pub(crate) fn is_folio_head(phys: u64) -> bool {
    FRAME_TABLE.lock().is_folio_head(phys)
}

pub(crate) fn is_folio_tail(phys: u64) -> bool {
    FRAME_TABLE.lock().is_folio_tail(phys)
}

pub(crate) fn folio_order(phys: u64) -> u8 {
    let key = phys & !(0xFFF);
    FRAME_TABLE
        .lock()
        .entries
        .get(&key)
        .map(|e| e.order)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FrameTable ────────────────────────────────────────────────

    #[test]
    fn ft_init_unique() {
        let mut ft = FrameTable::new();
        ft.init_unique(0x1000);
        assert!(ft.flags(0x1000).contains(FrameFlags::UNIQUE));
        assert_eq!(ft.refcount(0x1000), 0);
    }

    #[test]
    fn ft_share_promotes_unique() {
        let mut ft = FrameTable::new();
        ft.init_unique(0x1000);
        let cnt = ft.share(0x1000);
        assert_eq!(cnt, 1);
        assert!(ft.flags(0x1000).contains(FrameFlags::SHARED));
    }

    #[test]
    fn ft_share_increment() {
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        assert_eq!(ft.share(0x1000), 2);
        assert_eq!(ft.share(0x1000), 3);
    }

    #[test]
    fn ft_unshare_until_remove() {
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        ft.share(0x1000);
        assert_eq!(ft.unshare(0x1000), 1);
        assert!(ft.flags(0x1000).contains(FrameFlags::SHARED));
        assert_eq!(ft.unshare(0x1000), 0);
        assert_eq!(ft.refcount(0x1000), 0);
    }

    #[test]
    fn ft_try_upgrade_ok() {
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        assert!(ft.try_upgrade_unique(0x1000));
        assert!(ft.flags(0x1000).contains(FrameFlags::UNIQUE));
    }

    #[test]
    fn ft_try_upgrade_ref_gt_1() {
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        ft.share(0x1000);
        assert!(!ft.try_upgrade_unique(0x1000));
    }

    #[test]
    fn ft_try_upgrade_nonexistent() {
        assert!(!FrameTable::new().try_upgrade_unique(0x1000));
    }

    #[test]
    fn ft_try_upgrade_unique_then_share() {
        // try_upgrade_unique leaves ref=0 (UNIQUE). Next share promotes.
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        assert!(ft.try_upgrade_unique(0x1000));
        let cnt = ft.share(0x1000);
        assert_eq!(cnt, 1); // promotes back to SHARED ref=1
    }

    #[test]
    fn ft_mark_zombie() {
        let mut ft = FrameTable::new();
        ft.init_unique(0x1000);
        ft.mark_zombie(0x1000);
        assert!(ft.flags(0x1000).contains(FrameFlags::ZOMBIE));
    }

    #[test]
    fn ft_unshare_zombie_removes() {
        let mut ft = FrameTable::new();
        ft.init_unique(0x1000);
        ft.mark_zombie(0x1000);
        assert_eq!(ft.unshare(0x1000), 0);
        assert_eq!(ft.refcount(0x1000), 0);
    }

    #[test]
    fn ft_refcache_lifecycle() {
        let mut ft = FrameTable::new();
        ft.refcache_init(0x1000);
        assert!(ft.flags(0x1000).contains(FrameFlags::REFCACHE));

        let cnt = ft.refcache_global_add(0x1000, 5);
        assert_eq!(cnt, 5);

        ft.refcache_global_add(0x1000, -3);
        assert_eq!(ft.refcount(0x1000), 2);

        assert!(!ft.refcache_try_remove(0x1000)); // ref=2 → can't remove
        ft.refcache_global_add(0x1000, -2);
        assert_eq!(ft.refcount(0x1000), 0);
        assert!(ft.refcache_try_remove(0x1000));
    }

    #[test]
    fn ft_multiple_frames_independent() {
        let mut ft = FrameTable::new();
        ft.init_unique(0x1000);
        ft.init_unique(0x2000);
        ft.share(0x1000);
        assert_eq!(ft.refcount(0x1000), 1);
        assert_eq!(ft.refcount(0x2000), 0);
        assert!(ft.flags(0x1000).contains(FrameFlags::SHARED));
        assert!(ft.flags(0x2000).contains(FrameFlags::UNIQUE));
    }

    #[test]
    fn ft_share_nonexistent_creates() {
        let mut ft = FrameTable::new();
        let cnt = ft.share(0x1000);
        assert_eq!(cnt, 1);
        assert!(ft.flags(0x1000).contains(FrameFlags::SHARED));
    }

    #[test]
    fn ft_reinit_unique_overwrites_nothing() {
        // init_unique on existing entry does nothing (or_insert_with)
        let mut ft = FrameTable::new();
        ft.share(0x1000);
        ft.init_unique(0x1000); // should be no-op
        assert!(ft.flags(0x1000).contains(FrameFlags::SHARED));
    }

    // ── SharedAtomicFrame static helpers ───────────────────────────

    #[test]
    fn saf_incref_decref() {
        SharedAtomicFrame::<[u8; 4096]>::incref(0x5000);
        let cnt = SharedAtomicFrame::<[u8; 4096]>::decref(0x5000);
        assert_eq!(cnt, 0);
    }

    #[test]
    fn saf_incref_multi() {
        SharedAtomicFrame::<[u8; 4096]>::incref(0x5001);
        SharedAtomicFrame::<[u8; 4096]>::incref(0x5001);
        let cnt = SharedAtomicFrame::<[u8; 4096]>::decref(0x5001);
        assert_eq!(cnt, 1);
        let cnt = SharedAtomicFrame::<[u8; 4096]>::decref(0x5001);
        assert_eq!(cnt, 0);
    }

    // ── Compatibility shim functions ──────────────────────────────
    // Addresses are 0x1000-aligned to avoid page-aligned key collisions.

    #[test]
    fn compat_dec_frame_ref() {
        SharedAtomicFrame::<[u8; 4096]>::incref(0x8000);
        assert_eq!(dec_frame_ref(0x8000), 0);
    }

    #[test]
    fn compat_from_phys_inner_drop() {
        SharedAtomicFrame::<[u8; 4096]>::incref(0x9000);
        SharedAtomicFrame::<[u8; 4096]>::incref(0x9000);
        drop(SharedAtomicFrame::<[u8; 4096]>::from_phys_inner(0x9000));
        assert_eq!(frame_refcount(0x9000), 1);
        drop(SharedAtomicFrame::<[u8; 4096]>::from_phys_inner(0x9000));
        assert_eq!(frame_refcount(0x9000), 0);
    }

    #[test]
    fn compat_frame_refcount() {
        assert_eq!(frame_refcount(0xA000), 0);
        SharedAtomicFrame::<[u8; 4096]>::incref(0xA000);
        assert_eq!(frame_refcount(0xA000), 1);
    }

    #[test]
    fn compat_frame_flags() {
        let flags = frame_flags(0xB000);
        assert!(flags.is_empty());
        SharedAtomicFrame::<[u8; 4096]>::incref(0xB000);
        let flags = frame_flags(0xB000);
        assert!(flags.contains(FrameFlags::SHARED));
    }

    #[test]
    fn compat_try_upgrade_unique() {
        SharedAtomicFrame::<[u8; 4096]>::incref(0xC000);
        assert!(try_upgrade_unique(0xC000));
        assert!(!try_upgrade_unique(0xC000));
    }

    #[test]
    fn ft_pin_never_reaches_zero() {
        let mut ft = FrameTable::new();
        ft.pin(0xD000);
        let initial = ft.refcount(0xD000);
        assert_eq!(initial, u32::MAX / 2);
        assert!(ft.flags(0xD000).contains(FrameFlags::SHARED));
        for _ in 0..1000 {
            let r = ft.unshare(0xD000);
            assert!(r > 0, "refcount dropped to zero during unshare loop");
        }
        let remaining = ft.refcount(0xD000);
        assert!(remaining > 0);
        assert!(ft.flags(0xD000).contains(FrameFlags::SHARED));
    }

    #[test]
    fn ft_pin_try_upgrade_fails() {
        let mut ft = FrameTable::new();
        ft.pin(0xE000);
        assert!(!ft.try_upgrade_unique(0xE000));
    }

    #[test]
    fn ft_pin_via_shared() {
        // pin via share (existing entry) path
        let mut ft = FrameTable::new();
        ft.share(0xF000);
        ft.pin(0xF000); // should overwrite to high refcount
        let cnt = ft.unshare(0xF000);
        assert!(cnt > 0);
    }
}
