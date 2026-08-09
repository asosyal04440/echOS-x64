#![cfg(not(target_os = "none"))]
//! AddressSpace throughput benchmarks
//!
//! Measures the impact of the Phase 2 refactor
//! (Arc<Mutex<AddressSpace>> → Arc<RwLock<AddressSpace>>).
//!
//! - bench_address_space_read_lock: pure read‑lock acquisition + field access
//! - bench_address_space_write_create: write‑lock + allocation (create_empty_address_space)
//! - bench_address_space_read_complex: read‑lock + cross‑subsystem page counts

#![feature(test)]
extern crate test;

use test::Bencher;

/// Read‑lock acquisition latency.
/// Measures `space.read().id` — the cheapest possible read‑lock operation.
#[bench]
fn bench_address_space_read_lock(b: &mut Bencher) {
    let space = ech_os::memory::create_empty_address_space();

    b.iter(|| {
        let _id = test::black_box(ech_os::memory::address_space_id(&space));
    });
}

/// Write‑lock + allocation throughput.
/// `create_empty_address_space` acquires a write lock, allocates an
/// `AddressSpace` on the heap, and returns it wrapped in `Arc<RwLock<>>`.
#[bench]
fn bench_address_space_write_create(b: &mut Bencher) {
    b.iter(|| {
        let _space = test::black_box(ech_os::memory::create_empty_address_space());
    });
}

/// Complex read‑lock path.
/// `address_space_page_counts` acquires a read lock on the AddressSpace then
/// also touches LRU (rank 6) and SWAP (rank 7) — this is a realistic proxy
/// for the combined read path used by page‑fault accounting and procfs.
#[bench]
fn bench_address_space_read_complex(b: &mut Bencher) {
    let space = ech_os::memory::create_empty_address_space();

    b.iter(|| {
        let _counts = test::black_box(ech_os::memory::address_space_page_counts(&space));
    });
}
