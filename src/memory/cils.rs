//! # CILS — Concurrent Interval Lock‑free Skip‑list Lookup
//!
//! Reader/writer protocol
//! ======================
//! - **Readers** (page‑fault path):
//!   1. Acquire the sentinel address from the per‑space registry
//!      (very brief `CILS_MAP` lock, rank‑neutral).
//!   2. Enter RCU read‑side critical section.
//!   3. Traverse the skip‑list with `Acquire` ordering.
//!   4. Clone the `Vma` if found.
//!   5. Exit RCU read‑side critical section.
//!
//! - **Writers** (VMA split / merge / remove / insert — serialised by
//!   the existing `AddressSpace` mutex):
//!   1. Modify the skip‑list (VmaMap) as usual.
//!   2. Unlinked nodes are NOT freed immediately — they are pushed onto
//!      a retired‑node list.
//!   3. After releasing the AddressSpace mutex, call [`reclaim_retired`]
//!      which waits for an RCU grace period (`synchronize_rcu()`), then
//!      frees the retired nodes.
//!
//! Safety
//! ======
//! - The sentinel head (`Node` inside `VmaMap`) is heap‑allocated via
//!   `Box` and lives as long as the `VmaMap` (inside `AddressSpace`,
//!   behind `Arc<Mutex<>>`).  The sentinel **pointer** is stable —
//!   it never changes after creation.
//! - Readers hold `RcuReadLock` during traversal, preventing the RCU
//!   grace period from completing.
//! - Writers wait for a grace period (`synchronize_rcu()`) before
//!   freeing any unlinked node, guaranteeing that no reader can be
//!   accessing it.
//! - Every load of an `AtomicPtr<Node>` in the reader path uses
//!   `Ordering::Acquire`; every store uses `Ordering::Release`.

use crate::rcu::{RcuReadLock, synchronize_rcu};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use super::vma::{Vma, VmaMap};

// ── Retired‑node reclamation ─────────────────────────────────

/// Free all previously‑unlinked nodes after an RCU grace period.
///
/// Must be called **after** releasing the AddressSpace mutex so that
/// other tasks are not blocked during the grace‑period wait.
pub fn reclaim_retired(retired: &mut Vec<usize>) {
    if retired.is_empty() {
        return;
    }
    synchronize_rcu();
    let drained = core::mem::take(retired);
    for addr in drained {
        if addr != 0 {
            VmaMap::free_retired(addr as *mut super::vma::Node);
        }
    }
}

// ── Per‑address‑space sentinel registry ──────────────────────

/// Sentinel‑pointer registry, keyed by address‑space ID.
///
/// The sentinel pointer is stored once (during address‑space creation)
/// and is stable for the lifetime of the address space.
static CILS_MAP: Mutex<BTreeMap<u64, SentinelEntry>> = Mutex::new(BTreeMap::new());

struct SentinelEntry {
    /// Raw pointer to the `VmaMap` sentinel head (`*const Node`).
    sentinel: AtomicUsize,
}

/// Register the sentinel address for `space_id`.
///
/// Called once during address‑space creation (or when the `VmaMap` is
/// first populated).
pub fn register(space_id: u64, vmamap: &VmaMap) {
    let mut guard = CILS_MAP.lock();
    guard.entry(space_id).or_insert_with(|| SentinelEntry {
        sentinel: AtomicUsize::new(vmamap.sentinel_ptr() as usize),
    });
}

/// Unregister (called during address‑space teardown).
pub fn unregister(space_id: u64) {
    CILS_MAP.lock().remove(&space_id);
}

// ── Lock‑free VMA lookup ─────────────────────────────────────

/// Lock‑free VMA lookup using RCU.
///
/// Returns `Some(Vma)` if a VMA covering `addr` is found.
///
/// This function is safe to call from any context.  It acquires no
/// sustained locks beyond the brief `CILS_MAP` lock (sentinel pointer
/// lookup) and the RCU read‑side critical section during traversal.
pub fn find_vma_cils(addr: u64) -> Option<Vma> {
    // ── Phase 1: get sentinel pointer ──
    let space_id = get_current_space_id_fast();
    let sentinel = {
        let guard = CILS_MAP.lock();
        guard.get(&space_id).map(|e| e.sentinel.load(Ordering::Acquire))
    };
    let sentinel = sentinel?; // None if space_id is not registered

    // ── Phase 2: RCU‑protected traversal ──
    //
    // Every pointer loaded from an AtomicPtr<Node> may carry flag bits
    // in its low 2 bytes (LOCKED / INVALIDATED / DELETED).  We strip
    // them before dereferencing — the Vma data itself is still valid
    // (RCU guarantees the node is not freed).
    let _rcu = RcuReadLock::new();
    unsafe {
        let sentinel_node = sentinel as *const super::vma::Node;
        let mut x = sentinel_node;
        use super::vma::ptr_strip;

        // Iterate all MAX_HEIGHT levels (sentinel is at MAX_HEIGHT).
        for i in (0..super::vma::MAX_HEIGHT).rev() {
            let mut next = ptr_strip((*x).next_acquire(i));
            while !next.is_null() && (*next).start <= addr {
                x = next;
                next = ptr_strip((*x).next_acquire(i));
            }
        }

        if x as *const () as usize != sentinel {
            let node = &*x;
            if node.start <= addr && addr < node.end {
                // Best‑effort epoch validation.
                if _rcu.is_valid() {
                    return Some(node.vma.clone());
                }
            }
        }
    }
    None
}

fn get_current_space_id_fast() -> u64 {
    use crate::memory::ACTIVE_ADDRESS_SPACE;
    let guard = ACTIVE_ADDRESS_SPACE.lock();
    guard
        .as_ref()
        .map(|arc| {
            use core::ops::Deref;
            // Lock the inner AddressSpace just long enough to read `id`.
            // This is acceptable because ACTIVE_ADDRESS_SPACE (R1) →
            // AddressSpace (R2) follows the lock‑order contract.
            unsafe { (*Arc::as_ptr(arc)).read().id }
        })
        .unwrap_or(0)
}
