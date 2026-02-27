#![cfg(not(target_os = "none"))]
//! Bellek Tahsis Kıyaslama Paketi - Fibonacci Buddy vs Linux Buddy vs Windows Heap
//!
//! Bu modül; echOS'un Fibonacci Buddy tahsis algoritmasının küçük/orta/büyük
//! tahsis senaryolarında ve hafıza parçalanması (fragmentation) durumlarında
//! performansını ölçen kıyaslama fonksiyonlarını içerir.

#![feature(test)]
extern crate test;

use ech_os::memory::fibonacci_buddy::FibonacciBuddyAllocator;
use test::Bencher;
use x86_64::PhysAddr;

#[bench]
fn bench_fibonacci_buddy_allocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64 MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);

        // Küçük tahsisler (1-256 bayt)
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 256) + 1;
            let _ = allocator.allocate(size);
        }

        // Orta tahsisler (1-4 KB)
        for _ in 0..100 {
            let size = (test::black_box(rand::random::<usize>()) % 4096) + 1;
            let _ = allocator.allocate(size);
        }

        // Büyük tahsisler (4 KB - 64 KB)
        for _ in 0..10 {
            let size = (test::black_box(rand::random::<usize>()) % 65536) + 4096;
            let _ = allocator.allocate(size);
        }
    });
}

#[bench]
fn bench_fibonacci_buddy_fragmentation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 128; // 128 MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Karışık boyutlu tahsislerle belleği parçala
        for i in 0..5000 {
            let size = if i % 5 == 0 {
                (test::black_box(rand::random::<usize>()) % 65536) + 4096 // Büyük
            } else if i % 3 == 0 {
                (test::black_box(rand::random::<usize>()) % 4096) + 1 // Orta
            } else {
                (test::black_box(rand::random::<usize>()) % 256) + 1 // Küçük
            };

            if let Some(addr) = allocator.allocate(size) {
                allocations.push((addr, size));
            }
        }

        // Parçalanma oranını hesapla
        let total_allocated: usize = allocations.iter().map(|(_, size)| size).sum();
        let external_fragmentation = 1.0 - (total_allocated as f64 / MEMORY_SIZE as f64);

        test::black_box(external_fragmentation);
    });
}

#[bench]
fn bench_fibonacci_buddy_deallocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64 MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Karışık boyutlarda tahsis yap
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 8192) + 1;
            if let Some(addr) = allocator.allocate(size) {
                allocations.push((addr, size));
            }
        }

        // Rastgele sırayla serbest bırak
        while let Some((idx, _)) = allocations
            .iter()
            .enumerate()
            .max_by_key(|(_, (_, size))| *size)
        {
            let (addr, size) = allocations.remove(idx);
            allocator.deallocate(addr, size);
        }
    });
}
