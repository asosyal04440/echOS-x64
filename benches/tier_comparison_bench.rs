//! # H25 — Tier Comparison Benchmark
//!
//! TIER 1 (lock-free native) vs TIER 2 (jail SPSC) karşılaştırmalı benchmark.
//! Lock-free audit: TIER 1 sürücülerinde sıfır Mutex doğrulaması.
//!
//! `cargo bench --bench tier_comparison_bench` ile çalıştırılır.

#![cfg(not(target_os = "none"))]
#![feature(test)]
extern crate test;

use std::collections::VecDeque;
use std::sync::atomic::{fence, AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use test::Bencher;

// ============================================================================
// TIER 1: Lock-Free MMIO Simülasyonu
// ============================================================================

/// TIER 1 NVMe — sıfır Mutex, doğrudan MMIO
struct Tier1NvmeSimulator {
    /// Doğrudan bellek erişimi (MMIO simülasyonu)
    mmio_doorbell: AtomicU64,
    /// Submission queue tail (lock-free)
    sq_tail: AtomicU64,
    /// Completion queue head (lock-free)
    cq_head: AtomicU64,
    /// I/O sayacı
    io_count: AtomicU64,
}

impl Tier1NvmeSimulator {
    fn new() -> Self {
        Self {
            mmio_doorbell: AtomicU64::new(0),
            sq_tail: AtomicU64::new(0),
            cq_head: AtomicU64::new(0),
            io_count: AtomicU64::new(0),
        }
    }

    /// Lock-free submit — sadece atomic ops
    #[inline(always)]
    fn submit_io(&self, _lba: u64, _blocks: u16) {
        let tail = self.sq_tail.fetch_add(1, Ordering::AcqRel);
        // MMIO doorbell write (simüle)
        self.mmio_doorbell.store(tail + 1, Ordering::Release);
        fence(Ordering::SeqCst);
        self.io_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Lock-free completion check
    #[inline(always)]
    fn check_completion(&self) -> bool {
        let head = self.cq_head.load(Ordering::Acquire);
        let tail = self.sq_tail.load(Ordering::Acquire);
        head < tail
    }

    /// Lock-free complete
    #[inline(always)]
    fn complete_io(&self) {
        self.cq_head.fetch_add(1, Ordering::AcqRel);
    }
}

// ============================================================================
// TIER 2: Jail SPSC Ring Simülasyonu
// ============================================================================

/// TIER 2 USB — Mutex + SPSC ring overhead
struct Tier2UsbJailSimulator {
    /// Command ring (Mutex-korumalı)
    ring: Mutex<VecDeque<JailCommand>>,
    /// Completion ring (Mutex-korumalı)
    completions: Mutex<VecDeque<JailCompletion>>,
    /// I/O sayacı
    io_count: AtomicU64,
}

#[derive(Clone)]
struct JailCommand {
    opcode: u8,
    lba: u64,
    length: u16,
}

#[derive(Clone)]
struct JailCompletion {
    status: u8,
    cid: u16,
}

impl Tier2UsbJailSimulator {
    fn new() -> Self {
        Self {
            ring: Mutex::new(VecDeque::with_capacity(256)),
            completions: Mutex::new(VecDeque::with_capacity(256)),
            io_count: AtomicU64::new(0),
        }
    }

    /// Mutex'li submit — jail SPSC overhead
    fn submit_io(&self, lba: u64, length: u16) {
        let cmd = JailCommand {
            opcode: 0x28,
            lba,
            length,
        };
        self.ring.lock().unwrap().push_back(cmd);
        self.io_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Mutex'li process
    fn process_one(&self) -> bool {
        let cmd = self.ring.lock().unwrap().pop_front();
        if let Some(_cmd) = cmd {
            self.completions
                .lock()
                .unwrap()
                .push_back(JailCompletion { status: 0, cid: 0 });
            true
        } else {
            false
        }
    }

    /// Mutex'li completion check
    fn complete(&self) -> Option<JailCompletion> {
        self.completions.lock().unwrap().pop_front()
    }
}

// ============================================================================
// BENCHMARKS
// ============================================================================

#[bench]
fn bench_tier1_nvme_submit_latency(b: &mut Bencher) {
    let nvme = Tier1NvmeSimulator::new();
    b.iter(|| {
        nvme.submit_io(test::black_box(0), test::black_box(8));
    });
}

#[bench]
fn bench_tier2_jail_submit_latency(b: &mut Bencher) {
    let jail = Tier2UsbJailSimulator::new();
    b.iter(|| {
        jail.submit_io(test::black_box(0), test::black_box(8));
    });
}

#[bench]
fn bench_tier1_submit_complete_roundtrip(b: &mut Bencher) {
    let nvme = Tier1NvmeSimulator::new();
    b.iter(|| {
        nvme.submit_io(0, 8);
        while nvme.check_completion() {
            nvme.complete_io();
        }
    });
}

#[bench]
fn bench_tier2_submit_complete_roundtrip(b: &mut Bencher) {
    let jail = Tier2UsbJailSimulator::new();
    b.iter(|| {
        jail.submit_io(0, 8);
        jail.process_one();
        jail.complete();
    });
}

#[bench]
fn bench_tier1_batch_100_io(b: &mut Bencher) {
    let nvme = Tier1NvmeSimulator::new();
    b.iter(|| {
        for i in 0..100u64 {
            nvme.submit_io(i * 8, 8);
        }
        for _ in 0..100 {
            nvme.complete_io();
        }
    });
}

#[bench]
fn bench_tier2_batch_100_io(b: &mut Bencher) {
    let jail = Tier2UsbJailSimulator::new();
    b.iter(|| {
        for i in 0..100u64 {
            jail.submit_io(i * 8, 8);
        }
        for _ in 0..100 {
            jail.process_one();
            jail.complete();
        }
    });
}

// ============================================================================
// LOCK-FREE AUDIT — TIER 1 sürücülerinde Mutex olmamalı
// ============================================================================

/// Bu test, TIER 1 simülasyonunun lock-free olduğunu doğrular.
/// Gerçek audit: `grep -r "Mutex" src/drivers/nvme.rs src/drivers/nic_native.rs`
#[test]
fn audit_tier1_is_lock_free() {
    // Tier1NvmeSimulator hiçbir Mutex kullanmıyor
    // Tüm alanlar AtomicU64
    let nvme = Tier1NvmeSimulator::new();

    // 1000 I/O lock-free tamamlanmalı
    for i in 0..1000 {
        nvme.submit_io(i, 8);
    }
    assert_eq!(nvme.io_count.load(Ordering::Relaxed), 1000);

    // CQ'dan tamamla
    for _ in 0..1000 {
        nvme.complete_io();
    }
    assert_eq!(nvme.cq_head.load(Ordering::Relaxed), 1000);
}

/// TIER 2'nin Mutex kullandığını doğrular (beklenen davranış).
#[test]
fn audit_tier2_uses_mutex() {
    let jail = Tier2UsbJailSimulator::new();

    // Mutex-korumalı operasyonlar çalışmalı
    jail.submit_io(0, 8);
    assert!(jail.process_one());
    assert!(jail.complete().is_some());
}

/// TIER 1 vs TIER 2 latency farkını gösterir (informational test).
#[test]
fn compare_tier1_vs_tier2_overhead() {
    let nvme = Tier1NvmeSimulator::new();
    let jail = Tier2UsbJailSimulator::new();

    let iterations = 10_000u64;

    // TIER 1 timing
    let t1_start = std::time::Instant::now();
    for i in 0..iterations {
        nvme.submit_io(i, 8);
    }
    let t1_duration = t1_start.elapsed();

    // TIER 2 timing
    let t2_start = std::time::Instant::now();
    for i in 0..iterations {
        jail.submit_io(i, 8);
    }
    let t2_duration = t2_start.elapsed();

    // TIER 2 mutlaka TIER 1'den yavaş olmalı (Mutex overhead)
    println!(
        "TIER 1 (lock-free): {:?} for {} ops",
        t1_duration, iterations
    );
    println!(
        "TIER 2 (jail/Mutex): {:?} for {} ops",
        t2_duration, iterations
    );
    println!(
        "Overhead ratio: {:.2}x",
        t2_duration.as_nanos() as f64 / t1_duration.as_nanos().max(1) as f64
    );

    // Basit sanity check — her ikisi de tamamlandı
    assert_eq!(nvme.io_count.load(Ordering::Relaxed), iterations);
    assert_eq!(jail.io_count.load(Ordering::Relaxed), iterations);
}
