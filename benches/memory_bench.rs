#![cfg(not(target_os = "none"))]
//! Bellek Tahsis Benchmark Takımı - Fibonacci Buddy ile Linux Buddy ve Windows Heap karşılaştırması
//!
//! Bu modül, echOS'un Fibonacci Buddy bellek yöneticisinin performansını ölçer.
//!
//! Fibonacci Buddy Algoritması nedir?
//!   Klasik ikili buddy sisteminde bloklar 2^n boyutundadır (1KB, 2KB, 4KB...).
//!   Fibonacci buddy'de ise Fibonacci dizisi kullanılır: 1, 2, 3, 5, 8, 13, 21...
//!   Bu sayede küçük boyutlu tahsisler için daha az iç parçalanma (internal fragmentation) oluşur.
//!
//!   Bellek havuzu yapısı (örnek 64MB):
//!     ┌────────────────────────────────────────┐
//!     │  Buddy çiftleri:                       │
//!     │  fib(k)   + fib(k-1) = fib(k+1)       │
//!     │  [blok A] + [blok B] = [üst blok]      │
//!     └────────────────────────────────────────┘

#![feature(test)]
extern crate test;

use ech_os::memory::fibonacci_buddy::FibonacciBuddyAllocator;
use test::Bencher;
use x86_64::PhysAddr;

/// `bench_fibonacci_buddy_allocation`: Karışık boyutlarda bellek tahsis hızını ölçer.
///
/// Bu benchmark, tahsis yöneticisinin küçük, orta ve büyük talepler için
/// ne kadar hızlı uygun blok bulduğunu ölçer. Üç farklı boyut kademesi:
///   - Küçük : 1–256 bayt    (1000 adet) → yoğun tahsis senaryosu
///   - Orta  : 1–4KB         (100 adet)  → tipik kullanım
///   - Büyük : 4KB–64KB      (10 adet)   → sayfa boyutlu tahsisler
#[bench]
fn bench_fibonacci_buddy_allocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);

        // Küçük tahsisler (1-256 bayt)
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 256) + 1;
            let _ = allocator.allocate(size);
        }

        // Orta boyutlu tahsisler (1-4KB)
        for _ in 0..100 {
            let size = (test::black_box(rand::random::<usize>()) % 4096) + 1;
            let _ = allocator.allocate(size);
        }

        // Büyük boyutlu tahsisler (4KB-64KB)
        for _ in 0..10 {
            let size = (test::black_box(rand::random::<usize>()) % 65536) + 4096;
            let _ = allocator.allocate(size);
        }
    });
}

/// `bench_fibonacci_buddy_fragmentation`: Dış parçalanma (external fragmentation) oranını ölçer.
///
/// Parçalanma, tahsis yöneticisinin boş belleğini küçük parçalara bölmesi sonucu
/// büyük ardışık tahsislerin başarısız olmasıdır.
///
/// Bu benchmark, karma boyutlu 5000 tahsis yaparak parçalanmayı zorlar ve
/// dış parçalanma oranını hesaplar:
///   dış_parçalanma = 1.0 - (toplam_tahsis / toplam_bellek)
///
/// Parçalanma senaryosu (i döngüsüne göre boyut seçimi):
///   i % 5 == 0  → Büyük blok  (4KB–64KB)
///   i % 3 == 0  → Orta blok   (1B–4KB)
///   diğer       → Küçük blok  (1B–256B)
#[bench]
fn bench_fibonacci_buddy_fragmentation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 128; // 128MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Karma boyutlu tahsislerle belleği parçala
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

/// `bench_fibonacci_buddy_deallocation`: Bellek serbest bırakma ve birleştirme hızını ölçer.
///
/// Buddy sisteminin en önemli özelliği, iki komşu "kardeş" blok serbest bırakıldığında
/// otomatik olarak birleştirilmesidir (coalescing). Bu birleştirme işlemi
/// dış parçalanmayı azaltır ancak ek hesaplama gerektirir.
///
/// Bu benchmark şunu ölçer:
///   1. 1000 karma boyutlu tahsis yap
///   2. En büyük bloktan başlayarak sırayla serbest bırak (worst-case birleştirme)
///
/// Serbest bırakma sırası (max-by_key stratejisi):
///   En büyük blok önce serbest bırakılır → buddy birleştirme maximuma zorlanır
#[bench]
fn bench_fibonacci_buddy_deallocation(b: &mut Bencher) {
    const MEMORY_SIZE: usize = 1024 * 1024 * 64; // 64MB
    let base_addr = PhysAddr::new(0x1000000);

    b.iter(|| {
        let mut allocator = FibonacciBuddyAllocator::new(base_addr, MEMORY_SIZE);
        let mut allocations = Vec::new();

        // Karma boyutlarda tahsis yap
        for _ in 0..1000 {
            let size = (test::black_box(rand::random::<usize>()) % 8192) + 1;
            if let Some(addr) = allocator.allocate(size) {
                allocations.push((addr, size));
            }
        }

        // Rastgele sırada (en büyükten küçüğe) serbest bırak
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
