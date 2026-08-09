use core::sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use core::cmp::{max, min};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use rcore_fs::vfs::INode;
use x86_64::structures::paging::PageTableFlags;
use spin::Mutex;

pub(crate) const MAX_HEIGHT: usize = 16;
const FRACTION_NUM: u32 = 1;
const FRACTION_DEN: u32 = 2;

// ── CILS flag‑bit encoding ─────────────────────────────────────
// Low 2 bits of an AtomicPtr<Node> carry flags:
//   bit 0 (0b01) = LOCKED   (level‑0: interval lock)
//   bit 0 (0b01) = DELETED  (level > 0: Harris‑style logical deletion)
//   bit 1 (0b10) = INVALIDATED (node replaced by a Swap)
const PTR_MASK: usize = !0b11;
const LOCKED: usize = 0b01;
const INVALIDATED: usize = 0b10;

#[inline]
fn ptr_with_flag(ptr: *mut Node, flag: usize) -> *mut Node {
    ((ptr as usize) | flag) as *mut Node
}

#[inline]
fn flag_of(ptr: *mut Node) -> usize {
    (ptr as usize) & !PTR_MASK
}

#[inline]
pub(crate) fn ptr_strip(ptr: *mut Node) -> *mut Node {
    ((ptr as usize) & PTR_MASK) as *mut Node
}

#[inline]
fn is_locked(ptr: *mut Node) -> bool {
    flag_of(ptr) & LOCKED != 0
}

#[inline]
fn is_invalidated(ptr: *mut Node) -> bool {
    flag_of(ptr) & INVALIDATED != 0
}

#[inline]
fn is_unmarked(ptr: *mut Node) -> bool {
    flag_of(ptr) == 0
}

#[derive(Clone)]
pub enum VmaKind {
    Anonymous { id: u64 },
    Image { seg_start: u64, file_offset: u64, file_size: u64 },
    File { inode: Arc<dyn INode>, file_offset: u64, file_size: u64 },
}

#[derive(Clone)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub flags: PageTableFlags,
    pub kind: VmaKind,
    pub cow: bool,
    pub shared: bool,
    pub locked: bool,
}

impl Vma {
    pub fn kind_anonymous_id(&self) -> Option<u64> {
        match &self.kind {
            VmaKind::Anonymous { id } => Some(*id),
            _ => None,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, VmaKind::File { .. })
    }

    pub fn is_image(&self) -> bool {
        matches!(self.kind, VmaKind::Image { .. })
    }

    pub fn can_merge_with(&self, other: &Vma) -> bool {
        if self.end != other.start {
            return false;
        }
        if self.flags != other.flags || self.cow != other.cow || self.shared != other.shared || self.locked != other.locked {
            return false;
        }
        match (&self.kind, &other.kind) {
            (VmaKind::Anonymous { id: lid }, VmaKind::Anonymous { id: rid }) => lid == rid,
            (VmaKind::Image { seg_start: ls, file_offset: lo, .. },
             VmaKind::Image { seg_start: rs, file_offset: ro, .. }) => {
                let expected_off = lo.saturating_add(self.end.saturating_sub(self.start));
                ls == rs && *ro == expected_off
            }
            (VmaKind::File { inode: li, file_offset: lo, .. },
             VmaKind::File { inode: ri, file_offset: ro, .. }) => {
                let expected_off = lo.saturating_add(self.end.saturating_sub(self.start));
                Arc::ptr_eq(li, ri) && *ro == expected_off
            }
            _ => false,
        }
    }
}

/// A single skip‑list node.
///
/// Fields are `pub(crate)` so that the CILS concurrent‑reader module
/// (`cils.rs`) can traverse the list safely under RCU.
pub(crate) struct Node {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) vma: Vma,
    pub(crate) height: u8,
    pub(crate) next: [AtomicPtr<Node>; MAX_HEIGHT],
}

impl Node {
    pub(crate) fn new(vma: Vma, height: u8) -> *mut Self {
        let start = vma.start;
        let end = vma.end;
        let node = Box::new(Node {
            start,
            end,
            vma,
            height,
            next: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_HEIGHT],
        });
        Box::into_raw(node)
    }

    unsafe fn free_node(target: *mut Self) {
        if !target.is_null() {
            drop(Box::from_raw(target));
        }
    }

    /// Load `next[level]` (Relaxed), strip flag bits.
    /// Safe for single‑threaded or mutex‑protected use.
    pub(crate) fn next_ptr(&self, level: usize) -> *mut Node {
        ptr_strip(self.next[level].load(Ordering::Relaxed))
    }

    fn set_next(&self, level: usize, node: *mut Node) {
        self.next[level].store(node, Ordering::Relaxed);
    }

    /// Load `next[level]` with `Acquire` ordering, strip flag bits — used by RCU readers.
    pub(crate) fn next_acquire(&self, level: usize) -> *mut Node {
        ptr_strip(self.next[level].load(Ordering::Acquire))
    }

    /// Store `next[level]` with `Release` ordering — used by writers
    /// to publish a node atomically.
    pub(crate) fn set_next_release(&self, level: usize, node: *mut Node) {
        self.next[level].store(node, Ordering::Release);
    }

    // ── Raw (flag‑preserving) loads — for concurrent Lock/Swap ──

    /// Load raw `next[level]` (Relaxed) without stripping flags.
    pub(crate) fn next_raw_relaxed(&self, level: usize) -> *mut Node {
        self.next[level].load(Ordering::Relaxed)
    }

    /// Load raw `next[level]` (Acquire) without stripping flags.
    pub(crate) fn next_raw_acquire(&self, level: usize) -> *mut Node {
        self.next[level].load(Ordering::Acquire)
    }

    /// Compare‑and‑swap `next[level]` (AcqRel).
    pub(crate) fn cas_next(&self, level: usize, old: *mut Node, new: *mut Node) -> bool {
        self.next[level]
            .compare_exchange(old, new, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

pub struct VmaMap {
    pub(crate) head: Box<Node>,
    height: AtomicU8,
    len: AtomicUsize,
    /// Unlinked nodes awaiting an RCU grace period before deallocation.
    /// Stored as `usize` (not `*mut u8`) so that `VmaMap` remains `Send`.
    /// `Mutex` provides interior mutability for concurrent Lock/Swap paths.
    retired: Mutex<Vec<usize>>,
}

impl VmaMap {
    pub fn new() -> Self {
        VmaMap {
            head: Box::new(Node {
                start: 0, end: 0,
                vma: Vma {
                    start: 0, end: 0,
                    flags: PageTableFlags::empty(),
                    kind: VmaKind::Anonymous { id: 0 },
                    cow: false, shared: false, locked: false,
                },
                height: MAX_HEIGHT as u8,
                next: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_HEIGHT],
            }),
            height: AtomicU8::new(1),
            len: AtomicUsize::new(0),
            retired: Mutex::new(Vec::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return a raw pointer to the sentinel head node (never freed).
    /// Used by the CILS concurrent‑reader path.
    pub fn sentinel_ptr(&self) -> *const Node {
        &*self.head as *const Node
    }

    /// Return the current top‑level height.
    pub fn height(&self) -> usize {
        self.height.load(Ordering::Relaxed) as usize
    }

    /// Drain accumulated retired nodes (as `usize` addresses).
    ///
    /// The returned list should be passed to [`cils::reclaim_retired`]
    /// after releasing the `AddressSpace` lock.
    pub fn drain_retired(&self) -> Vec<usize> {
        core::mem::take(&mut *self.retired.lock())
    }

    /// Deallocate a single retired node.
    ///
    /// Must be called **after** an RCU grace period has elapsed so that
    /// no reader holds a reference.
    pub fn free_retired(ptr: *mut Node) {
        if !ptr.is_null() {
            unsafe { Node::free_node(ptr); }
        }
    }

    fn random_height() -> u8 {
        let mut h = 1u8;
        while h < MAX_HEIGHT as u8 {
            let r = random_u32_bounded(FRACTION_DEN);
            if r < FRACTION_NUM { h += 1; } else { break; }
        }
        h
    }

    fn find_preds(&self, addr: u64) -> [*mut Node; MAX_HEIGHT] {
        let mut preds = [core::ptr::null_mut(); MAX_HEIGHT];
        let h = self.height.load(Ordering::Relaxed);
        let mut x: *mut Node = self.head.as_ref() as *const Node as *mut Node;
        unsafe {
            for i in (0..h as usize).rev() {
                let mut next = (*x).next_ptr(i);
                while !next.is_null() && (*next).start <= addr {
                    x = next;
                    next = (*x).next_ptr(i);
                }
                preds[i] = x;
            }
            for i in h as usize..MAX_HEIGHT {
                preds[i] = self.head.as_ref() as *const Node as *mut Node;
            }
        }
        preds
    }

    pub fn find(&self, addr: u64) -> Option<Vma> {
        let preds = self.find_preds(addr);
        unsafe {
            let node = preds[0];
            let head = &*self.head as *const Node as *mut Node;
            if !node.is_null() && node != head {
                let r = &*node;
                if r.start <= addr && addr < r.end {
                    return Some(r.vma.clone());
                }
            }
        }
        None
    }

    pub fn overlaps(&self, start: u64, end: u64) -> bool {
        if end <= start { return false; }
        let preds = self.find_preds(start);
        unsafe {
            let mut cur = (*preds[0]).next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.start >= end { break; }
                if end > node.start && start < node.end { return true; }
                cur = node.next_ptr(0);
            }
        }
        false
    }

    pub fn insert(&self, vma: Vma) -> bool {
        if vma.end <= vma.start { return false; }
        let start = vma.start;
        let end = vma.end;

        let guard = self.lock_interval(start, end);
        // Check for actual overlap (not just adjacency)
        if guard.locked.len() > 1 {
            let mut has_overlap = false;
            unsafe {
                for &node in &guard.locked[1..] {
                    let n = &*node;
                    if n.start < end && n.end > start {
                        has_overlap = true;
                        break;
                    }
                }
            }
            if has_overlap {
                self.unlock_interval(guard);
                return false;
            }
        }

        let height = Self::random_height();
        let ptr = Node::new(vma, height);
        let h = self.height.load(Ordering::Relaxed);
        if height > h as u8 {
            self.height.store(height, Ordering::Relaxed);
        }

        self.swap_interval(guard, alloc::vec![ptr]);

        // Best-effort merge: lock the predecessor's anchor, swap pred for a
        // merged node, then unlink the successor so only the merged node remains.
        let head_ptr = self.head.as_ref() as *const Node as *mut Node;
        unsafe {
            let mut found_pred: *mut Node = head_ptr;
            let mut walk = (*found_pred).next_acquire(0);
            while !walk.is_null() && walk != ptr {
                found_pred = walk;
                walk = (*walk).next_acquire(0);
            }

            // Merge with predecessor
            if found_pred != head_ptr {
                let pred_vma = &(*found_pred).vma;
                let cur_vma = &(*ptr).vma;
                if pred_vma.can_merge_with(cur_vma) {
                    let merged_vma = Vma {
                        start: pred_vma.start,
                        end: cur_vma.end,
                        flags: pred_vma.flags,
                        kind: pred_vma.kind.clone(),
                        cow: pred_vma.cow && cur_vma.cow,
                        shared: pred_vma.shared && cur_vma.shared,
                        locked: pred_vma.locked && cur_vma.locked,
                    };
                    let merged = Node::new(merged_vma, Self::random_height());

                    // Step 1: lock and swap found_pred for merged
                    // Lock [found_pred.start, found_pred.end) so that old_nodes = [found_pred]
                    let mg = self.lock_interval(pred_vma.start, pred_vma.end);
                    if mg.locked.len() >= 2 {
                        let old = &mg.locked[1..];
                        if old.len() == 1 && (*old[0]).start == pred_vma.start {
                            self.swap_interval(mg, alloc::vec![merged]);
                            // Now: anchor → merged → ptr → succ
                            // Step 2: unlink ptr (it is covered by merged)
                            self.len.fetch_sub(1, Ordering::Relaxed);
                            let p_succ = (*ptr).next_ptr(0);
                            // Mark higher levels DELETED for ptr
                            for i in (1..(*ptr).height as usize).rev() {
                                let p_raw = (*ptr).next_raw_relaxed(i);
                                let p_target = ptr_strip(p_raw);
                                let del = ptr_with_flag(p_target, LOCKED); // DELETED bit
                                (*ptr).cas_next(i, p_raw, del);
                            }
                            (*merged).set_next_release(0, p_succ);
                            self.push_retired(ptr);
                            return true;
                        }
                    }
                    self.unlock_interval(mg);
                }
            }

            // Merge with successor
            let succ_ptr = (*ptr).next_acquire(0);
            if !succ_ptr.is_null() {
                let cur_vma = &(*ptr).vma;
                let succ_vma = &(*succ_ptr).vma;
                if cur_vma.can_merge_with(succ_vma) {
                    let merged_vma = Vma {
                        start: cur_vma.start,
                        end: succ_vma.end,
                        flags: cur_vma.flags,
                        kind: cur_vma.kind.clone(),
                        cow: cur_vma.cow && succ_vma.cow,
                        shared: cur_vma.shared && succ_vma.shared,
                        locked: cur_vma.locked && succ_vma.locked,
                    };
                    let merged = Node::new(merged_vma, Self::random_height());

                    // Step 1: lock and swap ptr for merged
                    let mg = self.lock_interval(cur_vma.start, cur_vma.end);
                    if mg.locked.len() >= 2 {
                        let old = &mg.locked[1..];
                        if old.len() == 1 && (*old[0]).start == cur_vma.start {
                            self.swap_interval(mg, alloc::vec![merged]);
                            // Now: anchor → merged → succ
                            // Step 2: unlink succ (it is covered by merged)
                            // succ may have been retired by swap_interval if it was in old_nodes,
                            // but succ was NOT in old_nodes (only ptr was).  Unlink it.
                            let s_next = (*succ_ptr).next_ptr(0);
                            for i in (1..(*succ_ptr).height as usize).rev() {
                                let s_raw = (*succ_ptr).next_raw_relaxed(i);
                                let s_target = ptr_strip(s_raw);
                                let del = ptr_with_flag(s_target, LOCKED);
                                (*succ_ptr).cas_next(i, s_raw, del);
                            }
                            (*merged).set_next_release(0, s_next);
                            self.len.fetch_sub(1, Ordering::Relaxed);
                            self.push_retired(succ_ptr);
                            return true;
                        }
                    }
                    self.unlock_interval(mg);
                }
            }
        }

        true
    }

    pub fn remove(&self, start: u64, end: u64) {
        if end <= start { return; }
        let guard = self.lock_interval(start, end);
        if guard.locked.len() <= 1 {
            self.unlock_interval(guard);
            return;
        }
        self.swap_interval(guard, alloc::vec![]);
    }

    pub fn remove_overlapping(&self, start: u64, end: u64) {
        self.remove(start, end);
    }

    pub fn update_flags(&self, start: u64, end: u64, flags: PageTableFlags) {
        if end <= start { return; }
        let guard = self.lock_interval(start, end);
        let old_nodes = &guard.locked[1..];
        let mut new_nodes: Vec<*mut Node> = Vec::new();
        unsafe {
            for &node in old_nodes {
                let n = &*node;
                if n.start < start {
                    let mut left = n.vma.clone();
                    left.end = start;
                    new_nodes.push(Node::new(left, Self::random_height()));
                }
                let mid_start = max(n.start, start);
                let mid_end = min(n.end, end);
                if mid_start < mid_end {
                    let mut mid = n.vma.clone();
                    mid.start = mid_start;
                    mid.end = mid_end;
                    mid.flags = flags;
                    new_nodes.push(Node::new(mid, Self::random_height()));
                }
                if n.end > end {
                    let mut right = n.vma.clone();
                    right.start = end;
                    new_nodes.push(Node::new(right, Self::random_height()));
                }
            }
        }
        self.swap_interval(guard, new_nodes);
    }

    pub fn insert_or_update(&self, start: u64, end: u64, mut vma: Vma) -> bool {
        if end <= start { return false; }
        let guard = self.lock_interval(start, end);
        let old_nodes = &guard.locked[1..];
        let mut new_nodes: Vec<*mut Node> = Vec::new();
        unsafe {
            for &node in old_nodes {
                let n = &*node;
                if n.start < start {
                    let mut left = n.vma.clone();
                    left.end = start;
                    new_nodes.push(Node::new(left, Self::random_height()));
                }
                if n.end > end {
                    let mut right = n.vma.clone();
                    right.start = end;
                    new_nodes.push(Node::new(right, Self::random_height()));
                }
            }
        }
        vma.start = start;
        vma.end = end;
        new_nodes.push(Node::new(vma, Self::random_height()));
        self.swap_interval(guard, new_nodes);
        true
    }

    /// Insert a VMA without checking for overlap or merge.
    /// Used by `Clone::clone` (single-threaded context).
    fn insert_skip_merge(&mut self, vma: Vma) {
        let start = vma.start;
        let end = vma.end;
        if end <= start { return; }
        let preds = self.find_preds(start);
        let height = Self::random_height();
        let ptr = Node::new(vma, height);
        unsafe {
            for i in 0..height as usize {
                (*ptr).set_next(i, (*preds[i]).next_ptr(i));
                (*preds[i]).set_next(i, ptr);
            }
        }
        let h = self.height.load(Ordering::Relaxed);
        if height > h { self.height.store(height, Ordering::Relaxed); }
        self.len.fetch_add(1, Ordering::Relaxed);
    }

    pub fn find_overlapping(&self, start: u64, end: u64) -> Vec<Vma> {
        if end <= start { return Vec::new(); }
        let preds = self.find_preds(start);
        let mut result = Vec::new();
        unsafe {
            let mut cur = (*preds[0]).next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.start >= end { break; }
                if node.end > start && node.start < end {
                    result.push(node.vma.clone());
                }
                cur = node.next_ptr(0);
            }
        }
        result
    }

    pub fn find_cow_regions(&self) -> Vec<Vma> {
        let mut result = Vec::new();
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.vma.cow || node.vma.shared {
                    result.push(node.vma.clone());
                }
                cur = node.next_ptr(0);
            }
        }
        result
    }

    pub fn mark_cow(&mut self) {
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                let node = &mut *cur;
                if !node.vma.shared && node.vma.flags.contains(PageTableFlags::WRITABLE) {
                    node.vma.cow = true;
                }
                cur = node.next_ptr(0);
            }
        }
    }

    pub fn committed_pages(&self) -> usize {
        let page_size = 4096u64;
        let mut total = 0usize;
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                let bytes = node.end.saturating_sub(node.start);
                total = total.saturating_add(
                    ((bytes.saturating_add(page_size - 1)) / page_size) as usize
                );
                cur = node.next_ptr(0);
            }
        }
        total
    }

    pub fn iter<'a>(&'a self) -> VmaIter<'a> {
        VmaIter {
            cur: self.head.next_ptr(0),
            _marker: core::marker::PhantomData,
        }
    }

    pub fn collect_all(&self) -> Vec<Vma> {
        let mut result = Vec::with_capacity(self.len());
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                result.push((*cur).vma.clone());
                cur = (*cur).next_ptr(0);
            }
        }
        result
    }

    pub fn collect_filtered(&self, mut f: impl FnMut(&Vma) -> bool) -> Vec<Vma> {
        let mut result = Vec::new();
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                let vma = &(*cur).vma;
                if f(vma) {
                    result.push(vma.clone());
                }
                cur = (*cur).next_ptr(0);
            }
        }
        result
    }

    pub fn clear(&mut self) {
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                let next = (*cur).next_ptr(0);
                Node::free_node(cur);
                cur = next;
            }
            // Free any retired-but-not-freed nodes.
            let retired = core::mem::take(&mut *self.retired.lock());
            for &addr in &retired {
                if addr != 0 {
                    Node::free_node(addr as *mut Node);
                }
            }
            for i in 0..MAX_HEIGHT {
                self.head.set_next(i, core::ptr::null_mut());
            }
        }
        self.height.store(1, Ordering::Relaxed);
        self.len.store(0, Ordering::Relaxed);
    }

}

pub struct VmaIter<'a> {
    cur: *mut Node,
    _marker: core::marker::PhantomData<&'a Vma>,
}

impl<'a> Iterator for VmaIter<'a> {
    type Item = &'a Vma;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur.is_null() {
            return None;
        }
        unsafe {
            let node = &*self.cur;
            let result = &node.vma;
            self.cur = node.next_ptr(0);
            Some(result)
        }
    }
}

impl Drop for VmaMap {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Clone for VmaMap {
    fn clone(&self) -> Self {
        let mut new_map = VmaMap::new();
        let vmas = self.collect_all();
        for v in vmas {
            new_map.insert_skip_merge(v);
        }
        new_map
    }
}

impl VmaMap {
    fn push_retired(&self, ptr: *mut Node) {
        if !ptr.is_null() {
            self.retired.lock().push(ptr as usize);
        }
    }
}

// ── Concurrent Lock / Swap ────────────────────────────────────
//
// Linearisation points (all atomic):
//   1. lock_interval:   pred.next[0] CAS from clean → LOCKED (line in LockInternal)
//   2. swap_interval:   pred.next[0] Release‑store from LOCKED → new head  (§4.1, Fig 18 line 75/77)
//   3. unlock_interval: pred.next[0] OR node.next[0] CAS from LOCKED → clean
//
// These primitives allow non‑overlapping interval operations to
// execute in parallel while overlapping operations are serialised
// by the interval lock.

/// The return type of [`VmaMap::lock_interval`] — holds locked nodes
/// and must be consumed by [`VmaMap::swap_interval`] or
/// [`VmaMap::unlock_interval`].
pub(crate) struct IntervalGuard {
    pub(crate) pred: *mut Node,
    pub(crate) locked: Vec<*mut Node>,
}

impl VmaMap {
    /// Concurrent predecessor/successor search (cf. GetPredSucc in Fig 18).
    ///
    /// Returns `(preds, succs)` for all `MAX_HEIGHT` levels.
    /// Handles DELETED (help‑unlink), LOCKED (treat as valid), and
    /// INVALIDATED (restart) flags on `next[i]` pointers.
    fn get_pred_succ_concurrent(&self, start: u64, end: u64) -> ([*mut Node; MAX_HEIGHT], [*mut Node; MAX_HEIGHT]) {
        loop {
            let mut preds = [core::ptr::null_mut(); MAX_HEIGHT];
            let mut succs = [core::ptr::null_mut(); MAX_HEIGHT];
            let h = self.height.load(Ordering::Relaxed);
            let mut x: *mut Node = self.head.as_ref() as *const Node as *mut Node;
            let head = x;
            let mut ok = true;

            unsafe {
                'levels: for i in (0..MAX_HEIGHT).rev() {
                    if i >= h as usize {
                        preds[i] = head;
                        succs[i] = core::ptr::null_mut();
                        continue;
                    }

                    let mut prev = x;
                    let mut raw = (*prev).next_raw_relaxed(i);
                    let mut node = ptr_strip(raw);

                    loop {
                        // Phase 1: advance `prev` past nodes with end <= start
                        // (i.e. nodes entirely before the target range).
                        while !node.is_null() {
                            let n_raw = (*node).next_raw_acquire(i);
                            let n_flag = flag_of(n_raw);
                            if n_flag == INVALIDATED {
                                ok = false;
                                break 'levels;
                            }
                            let n_next = ptr_strip(n_raw);

                            if n_flag != 0 && n_flag != LOCKED {
                                // DELETED at level > 0: help unlink
                                let cas_raw = ptr_with_flag(n_next, n_flag);
                                if !(*prev).cas_next(i, raw, cas_raw) {
                                    ok = false;
                                    break 'levels;
                                }
                                raw = (*prev).next_raw_relaxed(i);
                                node = ptr_strip(raw);
                                continue;
                            }

                            if n_flag == LOCKED {
                                // LOCKED → may be DELETED at higher levels.
                                // Treat as valid for pred-search.
                                break;
                            }

                            // Advance pred while node is entirely before [start, end)
                            if (*node).end <= start {
                                prev = node;
                                raw = (*prev).next_raw_relaxed(i);
                                node = ptr_strip(raw);
                                continue;
                            }

                            // node.end > start → it overlaps or is the first
                            // non-gap node.  Stop the pred advance.
                            break;
                        }

                        // Phase 2: collect nodes that overlap [start, end)
                        while !node.is_null() {
                            let next_raw = (*node).next_raw_acquire(i);
                            let next_flag = flag_of(next_raw);
                            if next_flag == INVALIDATED {
                                ok = false;
                                break 'levels;
                            }
                            let next_node = ptr_strip(next_raw);

                            // node.start >= end → past the range → stop
                            if (*node).start >= end {
                                break;
                            }

                            if next_node.is_null() || (*next_node).start >= end {
                                break;
                            }
                            // Gap or adjacency check for level-0
                            if i == 0 && (*next_node).end <= start {
                                break;
                            }

                            // Advance unless DELETED
                            if next_flag != 0 && next_flag != LOCKED {
                                if !(*node).cas_next(i, next_raw, ptr_with_flag(next_node, next_flag)) {
                                    ok = false;
                                    break 'levels;
                                }
                                continue;
                            }

                            prev = node;
                            raw = next_raw;
                            node = next_node;
                        }

                        if node.is_null() || (*node).start >= end {
                            break;
                        }
                        if i == 0 && (*node).end <= start {
                            // Gap before this node — advance pred and retry
                            prev = node;
                            raw = (*node).next_raw_relaxed(i);
                            node = ptr_strip(raw);
                            continue;
                        }
                        break;
                    }

                    preds[i] = prev;
                    succs[i] = node;
                    x = prev;
                }

                // Fill higher levels with head
                for i in h as usize..MAX_HEIGHT {
                    preds[i] = head;
                    succs[i] = core::ptr::null_mut();
                }
            }

            if ok {
                return (preds, succs);
            }
            // restart
        }
    }

    /// Lock the interval `[start, end]` (cf. Lock in Fig 18).
    ///
    /// Returns an `IntervalGuard` holding the locked predecessor and
    /// all locked overlapping nodes.  The caller **must** call either
    /// [`swap_interval`] or [`unlock_interval`] to release.
    ///
    /// **Linearisation point:** the successful CAS of `pred.next[0]`
    /// from clean to `LOCKED`.
    pub(crate) fn lock_interval(&self, start: u64, end: u64) -> IntervalGuard {
        loop {
            let (preds, _) = self.get_pred_succ_concurrent(start, end);
            let mut node = preds[0];
            let head = self.head.as_ref() as *const Node as *mut Node;

            // Lock the predecessor's level-0 next pointer
            unsafe {
                let raw = (*node).next_raw_relaxed(0);
                if is_locked(raw) || is_invalidated(raw) {
                    // must retry from scratch
                    continue;
                }
                if flag_of(raw) != 0 {
                    // DELETED at level 0 — retry
                    continue;
                }
                let locked = ptr_with_flag(ptr_strip(raw), LOCKED);
                if !(*node).cas_next(0, raw, locked) {
                    continue; // CAS failed, retry
                }
            }

            let mut locked_nodes: Vec<*mut Node> = Vec::new();
            locked_nodes.push(node); // predecessor
            let mut pred = node;

            unsafe {
                'walk: loop {
                    let raw = (*pred).next_raw_acquire(0);
                    let next = ptr_strip(raw);
                    if next.is_null() {
                        break;
                    }
                    let next_node = &*next;
                    if next_node.start >= end {
                        break;
                    }

                    // Try to lock `next`
                    let next_raw = (*next).next_raw_relaxed(0);
                    if is_invalidated(next_raw) {
                        // Node invalidated — must restart
                        // Unlock everything
                        Self::unlock_list(&locked_nodes);
                        continue; // retry whole lock_interval
                    }
                    if flag_of(next_raw) != 0 && !is_locked(next_raw) {
                        // DELETED at level 0 — skip by advancing pred
                        // Mark predecessor as pointing to what follows
                        let n_next = ptr_strip(next_raw);
                        let flagged = ptr_with_flag(n_next, LOCKED);
                        (*pred).cas_next(0, raw, flagged);
                        // relock the same pred
                        continue;
                    }

                    let lock_target = ptr_with_flag(ptr_strip(next_raw), LOCKED);
                    if !(*next).cas_next(0, next_raw, lock_target) {
                        // CAS failed — read next again
                        continue;
                    }

                    locked_nodes.push(next);

                    // Check if this locked node is actually before our interval
                    // (dynamic locking — see paper §4.2, Fig 7a)
                    if next_node.end <= start {
                        // The predecessor is now this node; unlock the old one
                        let old_pred = locked_nodes[0];
                        Self::unlock_node(old_pred);
                        locked_nodes.remove(0);
                        pred = next;
                        continue 'walk;
                    }

                    pred = next;
                }
            }

            return IntervalGuard {
                pred: locked_nodes[0],
                locked: locked_nodes,
            };
        }
    }

    /// Unlock all locked nodes (no swap — just release).
    pub(crate) fn unlock_interval(&self, guard: IntervalGuard) {
        Self::unlock_list(&guard.locked);
    }

    fn unlock_list(nodes: &[*mut Node]) {
        unsafe {
            for &node in nodes {
                Self::unlock_node(node);
            }
        }
    }

    /// Clear the LOCKED flag on `node.next[0]`.
    fn unlock_node(node: *mut Node) {
        unsafe {
            loop {
                let raw = (*node).next_raw_relaxed(0);
                let clean = ptr_strip(raw);
                if (*node).cas_next(0, raw, clean) {
                    break;
                }
            }
        }
    }

    /// Atomically swap old nodes for new nodes (cf. Swap in Fig 18).
    ///
    /// `guard` must come from [`lock_interval`].  After this call the
    /// guard is consumed and all nodes are unlocked.
    ///
    /// **Linearisation point:** the `Release`‑store to `pred.next[0]`
    /// which simultaneously:
    /// 1. makes old nodes unreachable at level-0,
    /// 2. makes new nodes reachable,
    /// 3. clears the LOCKED flag on the predecessor.
    pub(crate) fn swap_interval(&self, guard: IntervalGuard, new_nodes: Vec<*mut Node>) {
        let pred = guard.pred;
        let old_nodes = &guard.locked[1..]; // skip predecessor

        unsafe {
            // 1. Decrement height of old nodes (mark skip‑links DELETED)
            for &node in old_nodes {
                let h = (*node).height;
                for i in (1..h as usize).rev() {
                    loop {
                        let raw = (*node).next_raw_relaxed(i);
                        let target = ptr_strip(raw);
                        let marked = ptr_with_flag(target, LOCKED); // DELETED bit
                        if (*node).cas_next(i, raw, marked) {
                            break;
                        }
                    }
                }
            }

            // 2. Unlink old nodes from higher levels
            for &node in old_nodes {
                let h = (*node).height;
                for i in (1..h as usize).rev() {
                    let mut x: *mut Node = self.head.as_ref() as *const Node as *mut Node;
                    loop {
                        let x_raw = (*x).next_raw_relaxed(i);
                        let x_next = ptr_strip(x_raw);
                        if x_next.is_null() || x_next == node {
                            if !x_next.is_null() {
                                // found it — unlink
                                let succ_raw = (*node).next_raw_relaxed(i);
                                let succ = ptr_strip(succ_raw);
                                if (*x).cas_next(i, x_raw, succ) {
                                    break;
                                }
                                // CAS failed, retry from head
                                x = self.head.as_ref() as *const Node as *mut Node;
                                continue;
                            }
                            break;
                        }
                        x = x_next;
                    }
                }
            }

            // 3. Wire new nodes together (forward links at all levels)
            if !new_nodes.is_empty() {
                let max_h = unsafe {
                    new_nodes.iter()
                        .map(|n| (**n).height as usize)
                        .max()
                        .unwrap_or(0)
                };

                for i in 0..max_h {
                    for w in new_nodes.windows(2) {
                        let n0 = w[0];
                        let n1 = w[1];
                        if i < unsafe { (&*n1).height as usize } {
                            unsafe { (&*n0).set_next(i, n1); }
                        }
                    }
                    // Last new node points to old successor
                    if let Some(&last) = new_nodes.last() {
                        if i < unsafe { (*last).height as usize } {
                            let succ = unsafe { (*pred).next_raw_acquire(0) }; // old l0 successor = first old node
                            let mut s = ptr_strip(succ);
                            // walk past old nodes to find real successor
                            for _ in 0..old_nodes.len() {
                                if !s.is_null() {
                                    s = unsafe { (*s).next_ptr(i) };
                                }
                            }
                            unsafe { (&*last).set_next(i, s); }
                        }
                    }
                }

                // 4. Commit: pred.next[0] → new_nodes[0] (Release, clears LOCKED)
                self.len.fetch_sub(old_nodes.len(), Ordering::Relaxed);
                self.len.fetch_add(new_nodes.len(), Ordering::Relaxed);
                (*pred).set_next_release(0, new_nodes[0]);
            } else {
                // No new nodes — pred points past old nodes
                self.len.fetch_sub(old_nodes.len(), Ordering::Relaxed);
                let mut succ = ptr_strip((*pred).next_raw_relaxed(0));
                for _ in 0..old_nodes.len() {
                    if !succ.is_null() {
                        succ = (*succ).next_ptr(0);
                    }
                }
                (*pred).set_next_release(0, succ);
            }

            // 5. Mark old nodes INVALIDATED
            for &node in old_nodes {
                let raw = (*node).next_raw_relaxed(0);
                let target = ptr_strip(raw);
                let invalidated = ptr_with_flag(target, INVALIDATED);
                (*node).set_next_release(0, invalidated);
            }

            // 6. Retire old nodes
            for &node in old_nodes {
                self.push_retired(node);
            }
        }
    }
}

fn random_u32_bounded(bound: u32) -> u32 {
    use core::sync::atomic::AtomicU64;
    static SEED: AtomicU64 = AtomicU64::new(0x123456789abcdef);
    let old = SEED.fetch_add(0x9e3779b97f4a7c15, Ordering::Relaxed);
    let mut z = old.wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z = z ^ (z >> 31);
    (z as u32) % bound
}
