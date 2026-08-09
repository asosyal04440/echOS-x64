//! # Folio / Compound-Page Abstraction Layer
//!
//! A `Folio` is a typed handle for a physically-contiguous run of 2<sup>order</sup>
//! 4 KiB frames.  The metadata (head/tail flags, order) is stored in the
//! [`FrameTable`] entry of the head frame, so a non‑folio frame carries zero
//! extra overhead.
//!
//! ```
//!                    ┌─── head frame ───┐
//!                    │ FrameFlags::SHARED │
//!                    │ │ FOLIO_HEAD      │
//!                    │ order = 9         │
//!                    │ refcount = N      │
//!                    └───────────────────┘
//!                    ┌─── tail frame 0 ──┐
//!                    │ │ FOLIO_TAIL      │
//!                    │ refcount = 1      │
//!                    └───────────────────┘
//!                    ┌─── tail frame 1 ──┐
//!                    │ │ FOLIO_TAIL      │
//!                    │ refcount = 1      │
//!                    └───────────────────┘
//!                    ...
//! ```
//!
//! Reference counting remains **per‑frame** — each 4 KiB sub‑page carries its
//! own `AtomicU32` in the `FrameTable`.  The folio is an *organisational* unit
//! used by the page‑cache, writeback, and THP paths; it does not introduce a
//! new refcount domain.
//!
//! ## Integration Points
//!
//! | Subsystem | How to use |
//! |-----------|-----------|
//! | THP | `folio_register(base, order)` after the first `incref`; `folio_unregister` on split |
//! | Page cache | `page_cache_insert_folio` / `page_cache_folio_data` for folio‑sized entries |
//! | Writeback | `WritebackEntry` gains an optional `folio` field; `process_writeback_budget` writes the full range |
//! | LRU | A single `LruEntry` can represent an entire folio (future work) |
//!
//! ## Safety
//!
//! `Folio::from_head_phys` checks the `FOLIO_HEAD` flag, so it is safe to
//! construct from any page‑aligned physical address.  All other operations
//! assume the folio metadata is consistent — this is guaranteed by the
//! `folio_register` / `folio_unregister` pairing.

use crate::memory::frame_ownership::{self, SharedAtomicFrame};
use crate::memory::{allocate_contiguous_frames, deallocate_contiguous_frames};
use core::fmt;
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Maximum order for a folio.  9 → 2⁹ = 512 pages = 2 MiB (matching THP).
pub const MAX_FOLIO_ORDER: u8 = 9;

/// EchOS equivalent of Linux's `struct address_space *`.
///
/// This is an opaque cookie that the VFS/page‑cache layer interprets as a
/// mapping context (inode + backing store).  Low bits MAY encode flags
/// (reserved for future use — currently zero).
///
/// A zero `Mapping` (the default) means "no mapping" or "unknown mapping".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping(pub u64);

/// Folio — a handle for a physically‑contiguous group of 2<sup>order</sup>
/// 4 KiB frames.
///
/// This is the echOS equivalent of Linux's `struct folio`.  Unlike Linux,
/// the refcount remains **per‑frame** (stored in `FrameTable`), and the
/// head‑frame metadata (flags, order) is stored in the `FrameEntry`.
///
/// Fields `mapping`, `index`, and `private` mirror Linux's `struct folio`
/// and carry the same semantics:
///
/// | Field     | Linux equivalent   | Purpose |
/// |-----------|-------------------|---------|
/// | `mapping` | `folio->mapping`   | Opaque mapping cookie; VFS layer interprets it |
/// | `index`   | `folio->index`     | Offset within the mapping, in PAGE_SIZE units |
/// | `private` | `folio->private`   | Filesystem per‑folio private data |
///
/// # Non‑Copy
///
/// `Folio` is **not** `Copy` — this matches the Linux API where a folio carries
/// ownership semantics.  Use `.clone()` when an explicit duplicate is needed.
#[derive(Clone)]
pub struct Folio {
    head_phys: u64,
    /// Opaque mapping cookie (Linux: `folio->mapping`).
    /// Zero (default) means unassociated / anonymous.
    pub mapping: Mapping,
    /// Offset within the mapping, in PAGE_SIZE units (Linux: `folio->index`).
    pub index: u64,
    /// Filesystem per‑folio private data (Linux: `folio->private`).
    pub private: u64,
}

impl Folio {
    /// Construct a `Folio` from a physical address that is (or is inside) a
    /// registered folio.  Returns `None` if `phys` does not point into any
    /// known folio.
    ///
    /// The returned `Folio` has zero/default `mapping`, `index`, and `private`.
    /// Use `.with_meta()` to attach mapping metadata when available.
    ///
    /// Fast path: if `phys` *is* the head frame the lookup is trivial.
    /// Slow path: walks through the `FrameTable` to discover the head — this
    /// is O(nr_frames) in the worst case so consider caching the head address.
    pub fn from_phys(phys: u64) -> Option<Self> {
        let aligned = phys & !(0xFFF);
        if frame_ownership::is_folio_head(aligned) {
            return Some(Self::from_head_phys_impl(aligned));
        }
        if frame_ownership::is_folio_tail(aligned) {
            let order = frame_ownership::folio_order(aligned);
            if order == 0 {
                return None;
            }
            let nr = 1usize << order;
            for i in 0..nr {
                let candidate = (aligned & !((nr as u64 - 1) << 12)) + (i as u64) * 4096;
                if frame_ownership::is_folio_head(candidate) {
                    return Some(Self::from_head_phys_impl(candidate));
                }
            }
        }
        None
    }

    /// Construct a `Folio` directly from a known head‑frame physical address.
    /// Only succeeds if the frame carries the `FOLIO_HEAD` flag.
    ///
    /// The returned `Folio` has zero/default `mapping`, `index`, and `private`.
    pub fn from_head_phys(phys: u64) -> Option<Self> {
        let aligned = phys & !(0xFFF);
        if frame_ownership::is_folio_head(aligned) {
            Some(Self::from_head_phys_impl(aligned))
        } else {
            None
        }
    }

    /// Internal: construct from an aligned head address (no flag check).
    fn from_head_phys_impl(aligned: u64) -> Self {
        Self {
            head_phys: aligned,
            mapping: Mapping(0),
            index: 0,
            private: 0,
        }
    }

    /// Attach mapping metadata (mapping cookie, index, private) to this folio.
    /// Returns self so it can be used as a builder: `Folio::from_phys(p)?.with_meta(...)`.
    pub fn with_meta(mut self, mapping: Mapping, index: u64, private: u64) -> Self {
        self.mapping = mapping;
        self.index = index;
        self.private = private;
        self
    }

    /// Allocate a new folio of `order` pages (2<sup>order</sup> × 4 KiB).
    ///
    /// This does **not** change any refcount — the caller must call `incref`
    /// on each sub‑page before mapping (see the THP integration in `mod.rs`).
    /// Returns `None` if the contiguous allocation fails or `order` is too
    /// large.
    pub fn alloc(order: u8) -> Option<Self> {
        if order > MAX_FOLIO_ORDER {
            return None;
        }
        let nr_pages = 1usize << order;
        let frame = allocate_contiguous_frames(nr_pages)?;
        let base = frame.start_address().as_u64();
        for i in 0..nr_pages {
            let phys = base + (i as u64) * 4096;
            SharedAtomicFrame::<[u8; 4096]>::incref(phys);
            if i == 0 {
                frame_ownership::folio_mark_head(phys, order);
            } else {
                frame_ownership::folio_mark_tail(phys);
            }
        }
        Some(Self {
            head_phys: base,
            mapping: Mapping(0),
            index: 0,
            private: 0,
        })
    }

    /// Free the folio — drops the refcount on every sub‑page and clears
    /// folio metadata.  The caller must ensure no PTE or other reference
    /// still exists.
    pub unsafe fn free(self) {
        let nr = self.nr_pages();
        for i in 0..nr {
            let phys = self.page_phys(i);
            frame_ownership::folio_clear(phys);
            let _ = frame_ownership::dec_frame_ref(phys);
        }
    }

    // ── Accessors ─────────────────────────────────────────────────

    /// Physical address of the head frame.
    pub fn head_phys(&self) -> u64 {
        self.head_phys
    }

    /// Order of this folio — 2<sup>order</sup> pages in total.
    pub fn order(&self) -> u8 {
        if self.head_phys == 0 {
            return 0;
        }
        frame_ownership::folio_order(self.head_phys)
    }

    /// Number of 4 KiB pages in this folio.
    pub fn nr_pages(&self) -> usize {
        1usize << self.order()
    }

    /// Total size in bytes.
    pub fn size(&self) -> usize {
        self.nr_pages() * 4096
    }

    /// Physical address of the `idx`-th sub‑page (0‑based).
    pub fn page_phys(&self, idx: usize) -> u64 {
        self.head_phys + (idx as u64) * 4096
    }

    /// Which sub‑page index within this folio corresponds to `sub_phys`?
    /// Returns `None` if `sub_phys` is outside the folio.
    pub fn page_idx(&self, sub_phys: u64) -> Option<usize> {
        let aligned = sub_phys & !(0xFFF);
        let offset = aligned.wrapping_sub(self.head_phys);
        if offset % 4096 == 0 {
            let idx = (offset / 4096) as usize;
            if idx < self.nr_pages() {
                return Some(idx);
            }
        }
        None
    }

    /// Does this folio cover the frame at `phys`?
    pub fn contains(&self, phys: u64) -> bool {
        let aligned = phys & !(0xFFF);
        let end = self.head_phys + (self.nr_pages() as u64) * 4096;
        aligned >= self.head_phys && aligned < end
    }

    // ── Linux `folio_get` / `folio_put` wrappers ─────────────────

    /// Increment the refcount on the head frame of this folio.
    ///
    /// This is the echOS equivalent of Linux's `folio_get()`.  Note that
    /// unlike Linux, echOS maintains per‑frame refcounts, so this only
    /// affects the head frame.  For bulk operations across all sub‑pages,
    /// use [`Folio::incref_all`].
    pub fn get(&self) {
        SharedAtomicFrame::<[u8; 4096]>::incref(self.head_phys);
    }

    /// Decrement the refcount on the head frame of this folio.
    ///
    /// EchOS equivalent of Linux's `folio_put()`.  Per‑frame refcount
    /// semantics — see [`Folio::get`].
    pub fn put(&self) {
        let _ = frame_ownership::dec_frame_ref(self.head_phys);
    }

    // ── Bulk refcount helpers ──────────────────────────────────────

    /// Increment the refcount on every sub‑page of this folio.
    /// Use when establishing N new PTE references (one per sub‑page).
    pub fn incref_all(&self) {
        for i in 0..self.nr_pages() {
            SharedAtomicFrame::<[u8; 4096]>::incref(self.page_phys(i));
        }
    }

    /// Decrement the refcount on every sub‑page of this folio.
    pub fn decref_all(&self) {
        for i in 0..self.nr_pages() {
            let _ = frame_ownership::dec_frame_ref(self.page_phys(i));
        }
    }

    // ── Registration helpers (used by THP / splitting) ────────────

    /// Register an already‑allocated 2<sup>order</sup>-page region as a
    /// folio.  The caller **must** have already called `incref` on every
    /// sub‑page (or otherwise ensured a `FrameTable` entry exists) —
    /// typically done by the THP mapping path.
    ///
    /// Returns `None` if the head frame already carries folio metadata.
    pub fn register(base: u64, order: u8) -> Option<Self> {
        let aligned = base & !(0xFFF);
        if frame_ownership::is_folio_head(aligned) {
            return None;
        }
        let nr = 1usize << order;
        for i in 0..nr {
            let phys = aligned + (i as u64) * 4096;
            if i == 0 {
                frame_ownership::folio_mark_head(phys, order);
            } else {
                frame_ownership::folio_mark_tail(phys);
            }
        }
        Some(Self::from_head_phys_impl(aligned))
    }

    /// Unregister the folio — clear `FOLIO_HEAD` / `FOLIO_TAIL` flags on
    /// every sub‑page and reset their orders to 0.  Typically called from
    /// `split_huge_page`.
    pub fn unregister(&self) {
        for i in 0..self.nr_pages() {
            frame_ownership::folio_clear(self.page_phys(i));
        }
    }
}

/// Zero-fill every sub‑page of the folio using the HHDM mapping.
pub fn folio_zero(folio: &Folio) {
    let nr = folio.nr_pages();
    let hhdm = crate::memory::active_physical_offset();
    for i in 0..nr {
        let page_phys = folio.page_phys(i);
        let dst = (hhdm + page_phys) as *mut u8;
        unsafe {
            core::ptr::write_bytes(dst, 0, 4096);
        }
    }
}

// ── Convenience functions ──────────────────────────────

/// Mark a contiguous run of frames as a folio.  See [`Folio::register`].
pub fn folio_register(base: u64, order: u8) -> Option<Folio> {
    Folio::register(base, order)
}

/// Clear folio metadata on every frame that belongs to the folio at `base`.
/// See [`Folio::unregister`].
pub fn folio_unregister(base: u64) {
    if let Some(folio) = Folio::from_head_phys(base) {
        folio.unregister();
    }
}

/// Return the head physical address of the folio containing `phys`, or
/// `phys` itself if no folio metadata is found.
///
/// This is the echOS equivalent of Linuxʼs `compound_head()`.
pub fn folio_head(phys: u64) -> u64 {
    let aligned = phys & !(0xFFF);
    if frame_ownership::is_folio_head(aligned) {
        return aligned;
    }
    if frame_ownership::is_folio_tail(aligned) {
        if let Some(folio) = Folio::from_phys(phys) {
            return folio.head_phys();
        }
    }
    aligned
}

/// Return the folio order for the frame at `phys`.  Returns 0 if the
/// frame is not part of a folio.
pub fn folio_order(phys: u64) -> u8 {
    frame_ownership::folio_order(phys & !(0xFFF))
}

/// Return the number of pages in the folio containing `phys` (1 for
/// non‑folio frames).
pub fn folio_nr_pages(phys: u64) -> usize {
    let order = folio_order(phys);
    if order == 0 {
        1
    } else {
        1usize << order
    }
}

// ── Trait impls ──────────────────────────────────────────

impl fmt::Debug for Folio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Folio")
            .field("head_phys", &format_args!("{:#x}", self.head_phys))
            .field("order", &self.order())
            .field("nr_pages", &self.nr_pages())
            .field("mapping", &self.mapping.0)
            .field("index", &self.index)
            .field("private", &self.private)
            .finish()
    }
}

impl PartialEq for Folio {
    fn eq(&self, other: &Self) -> bool {
        self.head_phys == other.head_phys
    }
}

impl Eq for Folio {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::frame_ownership;

    fn make_folio(base: u64, order: u8) -> Folio {
        let nr = 1usize << order;
        for i in 0..nr {
            let phys = base + (i as u64) * 4096;
            SharedAtomicFrame::<[u8; 4096]>::incref(phys);
        }
        frame_ownership::folio_mark_head(base, order);
        for i in 1..nr {
            frame_ownership::folio_mark_tail(base + (i as u64) * 4096);
        }
        Folio::from_head_phys_impl(base)
    }

    fn cleanup_folio(base: u64, order: u8) {
        let nr = 1usize << order;
        for i in 0..nr {
            let phys = base + (i as u64) * 4096;
            frame_ownership::folio_clear(phys);
            let _ = frame_ownership::dec_frame_ref(phys);
        }
    }

    #[test]
    fn folio_register_unregister_roundtrip() {
        let base = 0x1_0000_0000u64;
        let order = 3;
        let nr = 1usize << order;
        for i in 0..nr {
            SharedAtomicFrame::<[u8; 4096]>::incref(base + (i as u64) * 4096);
        }
        let folio = folio_register(base, order).expect("register");
        assert_eq!(folio.order(), order);
        assert_eq!(folio.nr_pages(), nr);
        assert!(frame_ownership::is_folio_head(base));
        assert!(frame_ownership::is_folio_tail(base + 4096));
        assert_eq!(folio.page_phys(0), base);
        assert_eq!(folio.page_phys(1), base + 4096);
        assert_eq!(folio.page_phys(nr - 1), base + (nr as u64 - 1) * 4096);
        assert!(folio.contains(base));
        assert!(folio.contains(base + (nr as u64 - 1) * 4096));
        assert!(!folio.contains(base + (nr as u64) * 4096));
        assert_eq!(folio.page_idx(base), Some(0));
        assert_eq!(folio.page_idx(base + 4096), Some(1));
        assert_eq!(folio.page_idx(base + (nr as u64 - 1) * 4096), Some(nr - 1));
        assert_eq!(folio.page_idx(base + (nr as u64) * 4096), None);
        folio_unregister(base);
        assert!(!frame_ownership::is_folio_head(base));
        assert!(!frame_ownership::is_folio_tail(base + 4096));
        for i in 0..nr {
            let _ = frame_ownership::dec_frame_ref(base + (i as u64) * 4096);
        }
    }

    #[test]
    fn folio_from_head_phys_ok() {
        let base = 0x2_0000_0000u64;
        SharedAtomicFrame::<[u8; 4096]>::incref(base);
        frame_ownership::folio_mark_head(base, 2);
        let folio = Folio::from_head_phys(base).expect("from_head_phys");
        assert_eq!(folio.order(), 2);
        assert_eq!(folio.nr_pages(), 4);
        frame_ownership::folio_clear(base);
        let _ = frame_ownership::dec_frame_ref(base);
    }

    #[test]
    fn folio_from_phys_tail() {
        let base = 0x3_0000_0000u64;
        let order = 2;
        let nr = 4;
        for i in 0..nr {
            SharedAtomicFrame::<[u8; 4096]>::incref(base + (i as u64) * 4096);
        }
        frame_ownership::folio_mark_head(base, order);
        for i in 1..nr {
            frame_ownership::folio_mark_tail(base + (i as u64) * 4096);
        }
        let folio = Folio::from_phys(base + 8192).expect("from tail phys");
        assert_eq!(folio.head_phys(), base);
        assert_eq!(folio.order(), order);

        for i in 0..nr {
            frame_ownership::folio_clear(base + (i as u64) * 4096);
            let _ = frame_ownership::dec_frame_ref(base + (i as u64) * 4096);
        }
    }

    #[test]
    fn folio_head_helper() {
        let base = 0x4_0000_0000u64;
        SharedAtomicFrame::<[u8; 4096]>::incref(base);
        SharedAtomicFrame::<[u8; 4096]>::incref(base + 4096);
        frame_ownership::folio_mark_head(base, 1);
        frame_ownership::folio_mark_tail(base + 4096);
        assert_eq!(folio_head(base), base);
        assert_eq!(folio_head(base + 4096), base);
        assert_eq!(folio_head(base + 8192), base + 8192);
        assert_eq!(folio_order(base), 1);
        assert_eq!(folio_order(base + 4096), 1);
        assert_eq!(folio_order(base + 8192), 0);
        assert_eq!(folio_nr_pages(base), 2);
        assert_eq!(folio_nr_pages(base + 4096), 2);
        assert_eq!(folio_nr_pages(base + 8192), 1);
        frame_ownership::folio_clear(base);
        frame_ownership::folio_clear(base + 4096);
        let _ = frame_ownership::dec_frame_ref(base);
        let _ = frame_ownership::dec_frame_ref(base + 4096);
    }

    #[test]
    fn folio_size_and_order_zero() {
        let base = 0x5_0000_0000u64;
        SharedAtomicFrame::<[u8; 4096]>::incref(base);
        frame_ownership::folio_mark_head(base, 0);
        let folio = Folio::from_head_phys(base).unwrap();
        assert_eq!(folio.nr_pages(), 1);
        assert_eq!(folio.size(), 4096);
        assert_eq!(folio.order(), 0);
        frame_ownership::folio_clear(base);
        let _ = frame_ownership::dec_frame_ref(base);
    }

    #[test]
    fn folio_meta_fields() {
        let base = 0x6_0000_0000u64;
        SharedAtomicFrame::<[u8; 4096]>::incref(base);
        frame_ownership::folio_mark_head(base, 0);
        let folio = Folio::from_head_phys(base)
            .unwrap()
            .with_meta(Mapping(42), 128, 999);
        assert_eq!(folio.mapping.0, 42);
        assert_eq!(folio.index, 128);
        assert_eq!(folio.private, 999);
        frame_ownership::folio_clear(base);
        let _ = frame_ownership::dec_frame_ref(base);
    }

    #[test]
    fn folio_get_put() {
        let base = 0x7_0000_0000u64;
        SharedAtomicFrame::<[u8; 4096]>::incref(base);
        frame_ownership::folio_mark_head(base, 0);
        let folio = Folio::from_head_phys(base).unwrap();
        let before = frame_ownership::frame_refcount(base);
        folio.get();
        assert_eq!(frame_ownership::frame_refcount(base), before + 1);
        folio.put();
        assert_eq!(frame_ownership::frame_refcount(base), before);
        frame_ownership::folio_clear(base);
        let _ = frame_ownership::dec_frame_ref(base);
    }
}
