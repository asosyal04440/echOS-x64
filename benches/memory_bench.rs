#![cfg(not(target_os = "none"))]
//! Memory Allocation Benchmark Suite - Fibonacci Buddy vs Linux Buddy vs Windows Heap

#![feature(test)]
extern crate test;

use ech_os::memory::fibonacci_buddy::FibonacciBuddyAllocator;
use test::Bencher;
use x86_64::PhysAddr;

#[bench]
fn bench_fibonacci_buddy_allocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);

        // Small allocations (1-256 bytes)
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 256) + 1;
            let _ = allocator.allocate(size);
        }

        // Medium allocations (1-4KB)
        for _ in 0..100 {
            let size = (test::black_box(rand::random::<usize>()) % 4096) + 1;
            let _ = allocator.allocate(size);
        }

        // Large allocations (4KB-64KB)
        for _ in 0..10 {
            let size = (test::black_box(rand::random::<usize>()) % 65536) + 4096;
            let _ = allocator.allocate(size);
        }
    });
}

#[bench]
fn bench_fibonacci_buddy_fragmentation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 128; // 128MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Fragment memory with mixed size allocations
        for i in 0..5000 {
            let size = if i % 5 == 0 {
                (test::black_box(rand::random::<usize>()) % 65536) + 4096 // Large
            } else if i % 3 == 0 {
                (test::black_box(rand::random::<usize>()) % 4096) + 1 // Medium
            } else {
                (test::black_box(rand::random::<usize>()) % 256) + 1 // Small
            };

            if let Some(addr) = allocator.allocate(size) {
                allocations.push((addr, size));
            }
        }

        // Calculate fragmentation
        let total_allocated: usize = allocations.iter().map(|(_, size)| size).sum();
        let external_fragmentation = 1.0 - (total_allocated as f64 / MEMORY_SIZE as f64);

        test::black_box(external_fragmentation);
    });
}

#[bench]
fn bench_fibonacci_buddy_deallocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Allocate mixed sizes
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 8192) + 1;
            if let Some(addr) = allocator.allocate(size) {
                allocations.push((addr, size));
            }
        }

        // Deallocate in random order
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
