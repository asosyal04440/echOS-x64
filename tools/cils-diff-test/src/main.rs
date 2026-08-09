use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::cmp::{max, min};
// use std::collections::BTreeMap;

// ============================================================
// Minimal VMA type
// ============================================================

#[derive(Clone, Debug, PartialEq)]
struct Vma {
    start: u64,
    end: u64,
    flags: u16,
    kind: u8,   // 0=anon, 1=file, 2=image
    cow: bool,
    shared: bool,
}

impl Vma {
    fn anon(start: u64, end: u64, flags: u16) -> Self {
        Vma { start, end, flags, kind: 0, cow: false, shared: false }
    }

    fn can_merge_with(&self, other: &Vma) -> bool {
        self.end == other.start
            && self.flags == other.flags
            && self.cow == other.cow
            && self.shared == other.shared
            && self.kind == other.kind
    }
}

// ============================================================
// Common trait for both backends
// ============================================================

#[derive(Debug, Clone)]
struct Snapshot {
    vmas: Vec<Vma>,
    committed: usize,
    len: usize,
}

trait VmaBackend {
    fn insert(&mut self, vma: Vma) -> bool;
    fn find(&self, addr: u64) -> Option<Vma>;
    fn remove(&mut self, start: u64, end: u64);
    fn remove_overlapping(&mut self, start: u64, end: u64);
    fn update_flags(&mut self, start: u64, end: u64, flags: u16);
    fn find_overlapping(&self, start: u64, end: u64) -> Vec<Vma>;
    fn overlaps(&self, start: u64, end: u64) -> bool;
    fn committed_pages(&self) -> usize;
    fn collect_all(&self) -> Vec<Vma>;
    fn len(&self) -> usize;
    fn mark_cow(&mut self);
    fn snapshot(&self) -> Snapshot {
        let vmas = self.collect_all();
        Snapshot { committed: self.committed_pages(), len: self.len(), vmas }
    }
}

// ============================================================
// LEGACY: Vec-backed backend (original behavior)
// ============================================================

struct VecBackend {
    vmas: Vec<Vma>,
}

impl VecBackend {
    fn new() -> Self {
        VecBackend { vmas: Vec::new() }
    }

    fn merge_adjacent(&mut self) {
        if self.vmas.len() <= 1 { return; }
        self.vmas.sort_by_key(|v| v.start);
        let mut merged: Vec<Vma> = Vec::with_capacity(self.vmas.len());
        for vma in self.vmas.iter().cloned() {
            if let Some(last) = merged.last_mut() {
                if last.can_merge_with(&vma) {
                    last.end = vma.end;
                    continue;
                }
            }
            merged.push(vma);
        }
        self.vmas = merged;
    }
}

impl VmaBackend for VecBackend {
    fn insert(&mut self, vma: Vma) -> bool {
        if vma.end <= vma.start { return false; }
        let idx = self.vmas.iter().position(|item| item.start > vma.start)
            .unwrap_or(self.vmas.len());
        if idx > 0 {
            let prev = &self.vmas[idx - 1];
            if vma.start < prev.end { return false; }
        }
        if idx < self.vmas.len() {
            let next = &self.vmas[idx];
            if vma.end > next.start { return false; }
        }
        self.vmas.insert(idx, vma);
        self.merge_adjacent();
        true
    }

    fn find(&self, addr: u64) -> Option<Vma> {
        self.vmas.iter().find(|r| addr >= r.start && addr < r.end).cloned()
    }

    fn remove(&mut self, start: u64, end: u64) {
        self.vmas.retain(|r| !(r.start == start && r.end == end));
    }

    fn remove_overlapping(&mut self, start: u64, end: u64) {
        if end <= start { return; }
        let mut next = Vec::with_capacity(self.vmas.len());
        for region in &self.vmas {
            if end <= region.start || start >= region.end {
                next.push(region.clone());
                continue;
            }
            if start > region.start {
                let mut left = region.clone();
                left.end = start;
                next.push(left);
            }
            if end < region.end {
                let mut right = region.clone();
                right.start = end;
                next.push(right);
            }
        }
        self.vmas = next;
    }

    fn update_flags(&mut self, start: u64, end: u64, flags: u16) {
        if end <= start { return; }
        let mut next = Vec::with_capacity(self.vmas.len().saturating_add(2));
        for region in &self.vmas {
            if end <= region.start || start >= region.end {
                next.push(region.clone());
                continue;
            }
            if start > region.start {
                let mut left = region.clone();
                left.end = start;
                next.push(left);
            }
            let mid_start = region.start.max(start);
            let mid_end = region.end.min(end);
            if mid_start < mid_end {
                let mut mid = region.clone();
                mid.start = mid_start;
                mid.end = mid_end;
                mid.flags = flags;
                next.push(mid);
            }
            if end < region.end {
                let mut right = region.clone();
                right.start = end;
                next.push(right);
            }
        }
        self.vmas = next;
        self.merge_adjacent();
    }

    fn find_overlapping(&self, start: u64, end: u64) -> Vec<Vma> {
        if end <= start { return Vec::new(); }
        self.vmas.iter()
            .filter(|r| end > r.start && start < r.end)
            .cloned()
            .collect()
    }

    fn overlaps(&self, start: u64, end: u64) -> bool {
        if end <= start { return false; }
        self.vmas.iter().any(|r| end > r.start && start < r.end)
    }

    fn committed_pages(&self) -> usize {
        let page_size = 4096u64;
        self.vmas.iter().map(|r| {
            ((r.end.saturating_sub(r.start).saturating_add(page_size - 1)) / page_size) as usize
        }).sum()
    }

    fn collect_all(&self) -> Vec<Vma> { self.vmas.clone() }
    fn len(&self) -> usize { self.vmas.len() }

    fn mark_cow(&mut self) {
        for vma in &mut self.vmas {
            if !vma.shared && (vma.flags & 0x2) != 0 {
                vma.cow = true;
            }
        }
    }
}

// ============================================================
// NEW: SkipList-backed VmaMap (algorithm from SOSP '25 KAIST)
// ============================================================

const MAX_HEIGHT: usize = 16;

struct Node {
    start: u64,
    end: u64,
    vma: Vma,
    height: u8,
    next: [AtomicPtr<Node>; MAX_HEIGHT],
}

impl Node {
    fn new(vma: Vma, height: u8) -> Box<Self> {
        let start = vma.start;
        let end = vma.end;
        Box::new(Node {
            start, end, vma, height,
            next: [const { AtomicPtr::new(std::ptr::null_mut()) }; MAX_HEIGHT],
        })
    }

    fn next_ptr(&self, level: usize) -> *mut Node {
        self.next[level].load(Ordering::Relaxed)
    }

    fn set_next(&self, level: usize, node: *mut Node) {
        self.next[level].store(node, Ordering::Relaxed);
    }
}

struct VmaMap {
    head: Box<Node>,
    height: AtomicU8,
    len: usize,
}

fn random_height() -> u8 {
    let mut h = 1u8;
    while h < MAX_HEIGHT as u8 {
        if fastrand::u32(0..2) == 0 { h += 1; } else { break; }
    }
    h
}

impl VmaMap {
    fn new() -> Self {
        VmaMap {
            head: Node::new(Vma { start: 0, end: 0, flags: 0, kind: 0, cow: false, shared: false }, MAX_HEIGHT as u8),
            height: AtomicU8::new(1),
            len: 0,
        }
    }

    fn find_preds(&self, addr: u64) -> [*mut Node; MAX_HEIGHT] {
        let mut preds = [std::ptr::null_mut(); MAX_HEIGHT];
        let h = self.height.load(Ordering::Relaxed);
        let head_ptr = self.head.as_ref() as *const Node as *mut Node;
        let mut x = head_ptr;
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
                preds[i] = head_ptr;
            }
        }
        preds
    }

    fn maybe_merge_around(&mut self, addr: u64) {
        let preds = self.find_preds(addr.saturating_sub(1));
        unsafe {
            let cur_ptr = (*preds[0]).next_ptr(0);
            if cur_ptr.is_null() { return; }
            let prev_ptr = preds[0];
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if prev_ptr != head_ptr {
                let prev = &mut *prev_ptr;
                let cur = &*cur_ptr;
                if prev.vma.can_merge_with(&cur.vma) {
                    prev.end = cur.end;
                    prev.vma.end = cur.end;
                    self.unlink_node(cur_ptr);
                    // after backward merge, check forward merge too
                    let next_ptr = prev.next_ptr(0);
                    if !next_ptr.is_null() {
                        let next = &*next_ptr;
                        if prev.vma.can_merge_with(&next.vma) {
                            prev.end = next.end;
                            prev.vma.end = next.end;
                            self.unlink_node(next_ptr);
                        }
                    }
                    return;
                }
            }
            let cur = &*cur_ptr;
            let next_ptr = cur.next_ptr(0);
            if !next_ptr.is_null() {
                let next = &*next_ptr;
                if cur.vma.can_merge_with(&next.vma) {
                    let cur_mut = &mut *cur_ptr;
                    cur_mut.end = next.end;
                    cur_mut.vma.end = next.end;
                    self.unlink_node(next_ptr);
                }
            }
        }
    }

    fn unlink_node(&mut self, target: *mut Node) {
        let h = self.height.load(Ordering::Relaxed);
        let _ = h; // suppress unused warning
        unsafe {
            let height = (*target).height;
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            for i in 0..height as usize {
                let mut x = head_ptr;
                while !x.is_null() && (*x).next_ptr(i) != target {
                    x = (*x).next_ptr(i);
                }
                if !x.is_null() {
                    let succ = (*target).next_ptr(i);
                    (*x).set_next(i, succ);
                }
            }
        }
        self.len -= 1;
    }

    fn insert_skip_merge(&mut self, vma: Vma) {
        let start = vma.start;
        if vma.end <= start { return; }
        let preds = self.find_preds(start);
        let height = random_height();
        let node = Node::new(vma, height);
        let ptr: *mut Node = node.as_ref() as *const Node as *mut Node;
        unsafe {
            for i in 0..height as usize {
                (*ptr).set_next(i, (*preds[i]).next_ptr(i));
                (*preds[i]).set_next(i, ptr);
            }
            std::mem::forget(node);
        }
        let h = self.height.load(Ordering::Relaxed);
        if height > h { self.height.store(height, Ordering::Relaxed); }
        self.len += 1;
        self.maybe_merge_around(start);
    }
}

impl VmaBackend for VmaMap {
    fn insert(&mut self, vma: Vma) -> bool {
        if vma.end <= vma.start { return false; }
        let start = vma.start;
        let end = vma.end;
        let preds = self.find_preds(start);
        unsafe {
            let next_node = (*preds[0]).next_ptr(0);
            if !next_node.is_null() {
                let next = &*next_node;
                if next.start == start || (start < next.end && next.start < end) {
                    return false;
                }
            }
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let prev = &*preds[0];
                if start < prev.end { return false; }
            }
        }
        let height = random_height();
        let node = Node::new(vma, height);
        let ptr: *mut Node = node.as_ref() as *const Node as *mut Node;
        unsafe {
            for i in 0..height as usize {
                (*ptr).set_next(i, (*preds[i]).next_ptr(i));
                (*preds[i]).set_next(i, ptr);
            }
            std::mem::forget(node);
        }
        let h = self.height.load(Ordering::Relaxed);
        if height > h { self.height.store(height, Ordering::Relaxed); }
        self.len += 1;
        self.maybe_merge_around(start);
        true
    }

    fn find(&self, addr: u64) -> Option<Vma> {
        let preds = self.find_preds(addr);
        unsafe {
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start <= addr && addr < node.end {
                    return Some(node.vma.clone());
                }
            }
            let candidate = (*preds[0]).next_ptr(0);
            if !candidate.is_null() {
                let node = &*candidate;
                if node.start <= addr && addr < node.end {
                    return Some(node.vma.clone());
                }
            }
        }
        None
    }

    fn remove(&mut self, start: u64, end: u64) {
        if end <= start { return; }
        let preds = self.find_preds(start);
        unsafe {
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start == start && node.end == end {
                    self.unlink_node(preds[0]);
                    return;
                }
            }
            let mut cur = (*preds[0]).next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.start >= end { break; }
                if node.start == start && node.end == end {
                    self.unlink_node(cur);
                    return;
                }
                cur = node.next_ptr(0);
            }
        }
    }

    fn remove_overlapping(&mut self, start: u64, end: u64) {
        if end <= start { return; }
        let preds = self.find_preds(start);
        unsafe {
            let mut todo: Vec<*mut Node> = Vec::new();
            let mut clip_left: Option<Vma> = None;
            let mut clip_right: Option<Vma> = None;
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start < end && node.end > start {
                    if node.start < start {
                        let mut left = node.vma.clone();
                        left.end = start;
                        clip_left = Some(left);
                    }
                    if node.end > end {
                        let mut right = node.vma.clone();
                        right.start = end;
                        clip_right = Some(right);
                    }
                    todo.push(preds[0]);
                }
            }
            let mut cur = (*preds[0]).next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.start >= end { break; }
                if node.end > start && node.start < end {
                    if node.end > end {
                        let mut right = node.vma.clone();
                        right.start = end;
                        clip_right = Some(right);
                    }
                    todo.push(cur);
                }
                cur = node.next_ptr(0);
            }
            for &n in &todo { self.unlink_node(n); }
            if let Some(left) = clip_left { self.insert_skip_merge(left); }
            if let Some(right) = clip_right { self.insert_skip_merge(right); }
        }
    }

    fn update_flags(&mut self, start: u64, end: u64, flags: u16) {
        if end <= start { return; }
        let preds = self.find_preds(start);
        unsafe {
            let mut todo_remove: Vec<*mut Node> = Vec::new();
            let mut todo_insert: Vec<Vma> = Vec::new();
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start < end && node.end > start {
                    if node.start < start {
                        let mut left = node.vma.clone();
                        left.end = start;
                        todo_insert.push(left);
                    }
                    let mid_start = max(node.start, start);
                    let mid_end = min(node.end, end);
                    if mid_start < mid_end {
                        let mut mid = node.vma.clone();
                        mid.start = mid_start;
                        mid.end = mid_end;
                        mid.flags = flags;
                        todo_insert.push(mid);
                    }
                    if node.end > end {
                        let mut right = node.vma.clone();
                        right.start = end;
                        todo_insert.push(right);
                    }
                    todo_remove.push(preds[0]);
                }
            }
            let mut cur = (*preds[0]).next_ptr(0);
            while !cur.is_null() {
                let node = &*cur;
                if node.start >= end { break; }
                if node.end > start && node.start < end {
                    let mid_start = max(node.start, start);
                    let mid_end = min(node.end, end);
                    if mid_start < mid_end {
                        let mut mid = node.vma.clone();
                        mid.start = mid_start;
                        mid.end = mid_end;
                        mid.flags = flags;
                        todo_insert.push(mid);
                    }
                    if node.end > end {
                        let mut right = node.vma.clone();
                        right.start = end;
                        todo_insert.push(right);
                    }
                    todo_remove.push(cur);
                }
                cur = node.next_ptr(0);
            }
            for &n in &todo_remove { self.unlink_node(n); }
            for v in todo_insert { self.insert_skip_merge(v); }
        }
    }

    fn find_overlapping(&self, start: u64, end: u64) -> Vec<Vma> {
        if end <= start { return Vec::new(); }
        let preds = self.find_preds(start);
        let mut result = Vec::new();
        unsafe {
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start < end && node.end > start {
                    result.push(node.vma.clone());
                }
            }
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

    fn overlaps(&self, start: u64, end: u64) -> bool {
        if end <= start { return false; }
        let preds = self.find_preds(start);
        unsafe {
            let head_ptr = self.head.as_ref() as *const Node as *mut Node;
            if preds[0] != head_ptr {
                let node = &*preds[0];
                if node.start < end && node.end > start { return true; }
            }
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

    fn committed_pages(&self) -> usize {
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

    fn collect_all(&self) -> Vec<Vma> {
        let mut result = Vec::new();
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                result.push((*cur).vma.clone());
                cur = (*cur).next_ptr(0);
            }
        }
        result
    }

    fn len(&self) -> usize { self.len }

    fn mark_cow(&mut self) {
        unsafe {
            let mut cur = self.head.next_ptr(0);
            while !cur.is_null() {
                if !(*cur).vma.shared && ((*cur).vma.flags & 0x2) != 0 {
                    (*cur).vma.cow = true;
                }
                cur = (*cur).next_ptr(0);
            }
        }
    }
}

impl Drop for VmaMap {
    fn drop(&mut self) {
        let mut cur = self.head.next_ptr(0);
        while !cur.is_null() {
            unsafe {
                let next = (*cur).next_ptr(0);
                let _ = Box::from_raw(cur);
                cur = next;
            }
        }
    }
}

// ============================================================
// Delta comparison
// ============================================================

#[derive(Debug)]
struct Mismatch {
    op_idx: usize,
    op_desc: String,
    left: Snapshot,
    right: Snapshot,
}

fn compare_snapshots(a: &Snapshot, b: &Snapshot, op_idx: usize, op_desc: &str) -> Option<Mismatch> {
    if a.vmas.len() != b.vmas.len() || a.committed != b.committed || a.len != b.len {
        return Some(Mismatch {
            op_idx, op_desc: op_desc.to_string(),
            left: a.clone(), right: b.clone(),
        });
    }
    for (i, (va, vb)) in a.vmas.iter().zip(b.vmas.iter()).enumerate() {
        if va.start != vb.start || va.end != vb.end
            || va.flags != vb.flags || va.cow != vb.cow
            || va.shared != vb.shared || va.kind != vb.kind
        {
            return Some(Mismatch {
                op_idx, op_desc: op_desc.to_string(),
                left: a.clone(), right: b.clone(),
            });
        }
    }
    None
}

// ============================================================
// Operation generation
// ============================================================

#[derive(Clone, Debug)]
enum Op {
    Insert(u64, u64, u16),         // start, end, flags
    Find(u64),                      // address
    Remove(u64, u64),              // exact range
    RemoveOverlapping(u64, u64),   // clip range
    UpdateFlags(u64, u64, u16),    // mprotect
    Overlaps(u64, u64),            // check
    MarkCow,
}

fn gen_ops(rng: &mut fastrand::Rng, count: usize, space_end: u64) -> Vec<Op> {
    let mut ops = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = rng.u32(0..8);
        match kind {
            0..=3 => {  // insert
                let a = rng.u64(0x1000..space_end);
                let b = a + rng.u64(0x1000..0x10000);
                let flags = 1u16 | (if rng.bool() { 2 } else { 0 }) | (if rng.bool() { 4 } else { 0 });
                ops.push(Op::Insert(a, b, flags));
            }
            4 => {  // find
                let a = rng.u64(0x1000..space_end);
                ops.push(Op::Find(a));
            }
            5 => {  // remove exact
                let a = rng.u64(0x1000..space_end);
                let b = a + rng.u64(0x1000..0x8000);
                ops.push(Op::Remove(a, b));
            }
            6 => {  // remove_overlapping / partial munmap
                let a = rng.u64(0x1000..space_end);
                let b = a + rng.u64(0x1000..0x10000);
                ops.push(Op::RemoveOverlapping(a, b));
            }
            7 => {  // update flags / mprotect
                let a = rng.u64(0x1000..space_end);
                let b = a + rng.u64(0x1000..0x10000);
                let flags = 1u16 | (if rng.bool() { 2 } else { 0 }) | (if rng.bool() { 4 } else { 0 });
                ops.push(Op::UpdateFlags(a, b, flags));
            }
            _ => {}
        }
    }
    // always end with mark_cow
    ops.push(Op::MarkCow);
    ops
}

// ============================================================
// Main test runner
// ============================================================

fn run_ops(ops: &[Op], backend: &mut dyn VmaBackend) -> Vec<Snapshot> {
    let mut snaps = Vec::with_capacity(ops.len() + 1);
    snaps.push(backend.snapshot());
    for op in ops {
        match op {
            Op::Insert(s, e, f) => { backend.insert(Vma::anon(*s, *e, *f)); }
            Op::Find(a) => { backend.find(*a); }
            Op::Remove(s, e) => { backend.remove(*s, *e); }
            Op::RemoveOverlapping(s, e) => { backend.remove_overlapping(*s, *e); }
            Op::UpdateFlags(s, e, f) => { backend.update_flags(*s, *e, *f); }
            Op::Overlaps(s, e) => { backend.overlaps(*s, *e); }
            Op::MarkCow => { backend.mark_cow(); }
        }
        snaps.push(backend.snapshot());
    }
    snaps
}

fn test_sequence(ops: &[Op], seed: u64) -> Option<Mismatch> {
    let mut vec_backend = VecBackend::new();
    let mut cils_backend = VmaMap::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Insert(s, e, f) => {
                let r1 = vec_backend.insert(Vma::anon(*s, *e, *f));
                let r2 = cils_backend.insert(Vma::anon(*s, *e, *f));
                if r1 != r2 {
                    return Some(Mismatch {
                        op_idx: i, op_desc: format!("Insert({:#x},{:#x},{}) ret={}/{}", s, e, f, r1, r2),
                        left: vec_backend.snapshot(), right: cils_backend.snapshot(),
                    });
                }
            }
            Op::Find(a) => {
                let r1 = vec_backend.find(*a);
                let r2 = cils_backend.find(*a);
                if r1 != r2 {
                    return Some(Mismatch {
                        op_idx: i, op_desc: format!("Find({:#x})", a),
                        left: vec_backend.snapshot(), right: cils_backend.snapshot(),
                    });
                }
            }
            Op::Remove(s, e) => {
                vec_backend.remove(*s, *e);
                cils_backend.remove(*s, *e);
            }
            Op::RemoveOverlapping(s, e) => {
                vec_backend.remove_overlapping(*s, *e);
                cils_backend.remove_overlapping(*s, *e);
            }
            Op::UpdateFlags(s, e, f) => {
                vec_backend.update_flags(*s, *e, *f);
                cils_backend.update_flags(*s, *e, *f);
            }
            Op::Overlaps(s, e) => {
                let r1 = vec_backend.overlaps(*s, *e);
                let r2 = cils_backend.overlaps(*s, *e);
                if r1 != r2 {
                    return Some(Mismatch {
                        op_idx: i, op_desc: format!("Overlaps({:#x},{:#x}) ret={}/{}", s, e, r1, r2),
                        left: vec_backend.snapshot(), right: cils_backend.snapshot(),
                    });
                }
            }
            Op::MarkCow => {
                vec_backend.mark_cow();
                cils_backend.mark_cow();
            }
        }

        let s1 = vec_backend.snapshot();
        let s2 = cils_backend.snapshot();
        if let Some(mm) = compare_snapshots(&s1, &s2, i, &format!("{:?}", op)) {
            return Some(mm);
        }
    }

    // Final state must be identical
    let s1 = vec_backend.snapshot();
    let s2 = cils_backend.snapshot();
    compare_snapshots(&s1, &s2, ops.len(), "final")
}

// ============================================================
// Edge case scenarios
// ============================================================

fn edge_case_adjacent_inserts() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x2000, 0x7),
        Op::Insert(0x2000, 0x3000, 0x7),  // same flags → should merge
        Op::Insert(0x3000, 0x4000, 0x3),  // different flags → no merge
        Op::Insert(0x4000, 0x5000, 0x3),  // same as prev → should merge with 0x3000-0x4000
        Op::Find(0x1500),
        Op::Find(0x2500),
        Op::MarkCow,
    ];
    test_sequence(&ops, 0)
}

fn edge_case_overlap_rejection() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x5000, 0x7),
        Op::Insert(0x2000, 0x3000, 0x7),  // overlap → reject
        Op::Insert(0x5000, 0x6000, 0x7),  // adjacent → merge
        Op::Insert(0x1000, 0x5000, 0x3),  // exact overlap → reject (different flags)
        Op::Overlaps(0x1000, 0x5000),
        Op::Overlaps(0x6000, 0x7000),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_partial_unmap() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x10000, 0x7),
        Op::RemoveOverlapping(0x4000, 0x8000),  // clip middle
        Op::Find(0x2000),
        Op::Find(0x6000),  // now gap
        Op::Find(0x9000),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_flags_update() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x10000, 0x7),
        Op::UpdateFlags(0x4000, 0x8000, 0x3),  // partial mprotect
        Op::Find(0x2000),
        Op::Find(0x6000),
        Op::Find(0x9000),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_split_then_merge() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x10000, 0x7),
        Op::RemoveOverlapping(0x4000, 0x6000),
        Op::RemoveOverlapping(0x6000, 0x8000),
        Op::Insert(0x4000, 0x8000, 0x7),  // should merge back
        Op::Find(0x5000),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_cow() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x5000, 0x7),  // writable
        Op::Insert(0x5000, 0x9000, 0x1),  // non-writable
        Op::MarkCow,
        Op::Find(0x2000),
        Op::Find(0x6000),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_empty_ops() -> Option<Mismatch> {
    test_sequence(&[], 0)
}

fn edge_case_single_page() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x2000, 0x7),
        Op::Remove(0x1000, 0x2000),
        Op::Insert(0x1000, 0x2000, 0x7),
    ];
    test_sequence(&ops, 0)
}

fn edge_case_many_adjacent() -> Option<Mismatch> {
    let mut ops = Vec::new();
    for i in 0..100u64 {
        let s = 0x1000 + i * 0x1000;
        let e = s + 0x1000;
        let flags = if i % 2 == 0 { 0x7 } else { 0x3 };
        ops.push(Op::Insert(s, e, flags));
    }
    ops.push(Op::MarkCow);
    test_sequence(&ops, 0)
}

fn edge_case_mprotect_multi() -> Option<Mismatch> {
    let ops = vec![
        Op::Insert(0x1000, 0x5000, 0x7),
        Op::Insert(0x5000, 0x9000, 0x3),
        Op::Insert(0x9000, 0xD000, 0x7),
        Op::UpdateFlags(0x3000, 0xB000, 0x5),  // spans 3 VMAs
    ];
    test_sequence(&ops, 0)
}

// ============================================================
// main
// ============================================================

fn main() {
    let seeds: Vec<u64> = if let Ok(s) = std::env::var("CILS_SEED") {
        s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
    } else {
        (0..100).collect()
    };

    let edge_cases: Vec<(&str, fn() -> Option<Mismatch>)> = vec![
        ("adjacent_inserts", edge_case_adjacent_inserts),
        ("overlap_rejection", edge_case_overlap_rejection),
        ("partial_unmap", edge_case_partial_unmap),
        ("flags_update", edge_case_flags_update),
        ("split_then_merge", edge_case_split_then_merge),
        ("cow", edge_case_cow),
        ("empty_ops", edge_case_empty_ops),
        ("single_page", edge_case_single_page),
        ("many_adjacent", edge_case_many_adjacent),
        ("mprotect_multi", edge_case_mprotect_multi),
    ];

    let mut failures = 0;
    let mut total = 0;

    // Edge cases
    for (name, test_fn) in &edge_cases {
        total += 1;
        match test_fn() {
            None => println!("  PASS  {name}"),
            Some(mm) => {
                failures += 1;
                println!("  FAIL  {name}");
                println!("        op[{}]: {}", mm.op_idx, mm.op_desc);
                println!("        VEC  committed={} len={}", mm.left.committed, mm.left.len);
                for v in &mm.left.vmas {
                    println!("          [{:#x},{:#x}) f={} cow={}", v.start, v.end, v.flags, v.cow as u8);
                }
                println!("        CILS committed={} len={}", mm.right.committed, mm.right.len);
                for v in &mm.right.vmas {
                    println!("          [{:#x},{:#x}) f={} cow={}", v.start, v.end, v.flags, v.cow as u8);
                }
            }
        }
    }

    // Random sequences at multiple seeds
    for &seed in &seeds {
        let mut rng = fastrand::Rng::with_seed(seed);
        let space_end = 0x1_0000_0000u64;
        let ops = gen_ops(&mut rng, 500, space_end);
        total += 1;
        if let Some(mm) = test_sequence(&ops, seed) {
            failures += 1;
            println!("  FAIL  seed={seed}  op[{}]: {}", mm.op_idx, mm.op_desc);
        } else {
            if seed < 5 || seed % 10 == 0 {
                println!("  PASS  seed={seed}");
            }
        }
    }

    println!("\n─── Results ───");
    println!("  Total:  {total}");
    println!("  Passed: {}", total - failures);
    println!("  Failed: {failures}");

    if failures > 0 {
        std::process::exit(1);
    }
}
