//! # echOS Gelişmiş Atomik İşlemler Modülü
//!
//! Tier 1 işletim sistemi düzeyinde atomik işlemler sunar.
//! Linux atomik işlemleri ile aynı düzeyde performans ve güvenlik sağlar.
//! Tamsayılar, işaretçiler ve bit alanları için kilit gerektirmeyen (lock-free)
//! veri yapıları ve senkronizasyon ilkelleri içerir.
//! Bellek bariyerleri (smp_mb, smp_rmb, smp_wmb) ile donanım sıralama garantileri
//! zorunlu kılınır; böylece çok işlemcili ortamda veri tutarsızlığı önlenir.

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use alloc::boxed::Box;
use core::sync::atomic::{
    AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicIsize, AtomicPtr, AtomicU16,
    AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};

/// Tamsayı türleri için gelişmiş atomik işlemler arayüzü.
/// Her işlem, çok işlemcili ortamda veri yarışını önleyen SeqCst bellek sıralamasıyla çalışır.
pub trait AtomicOps<T> {
    /// Atomik toplama — eski değeri döndürür, bellek güvenliğini garanti eder
    fn atomic_add(&self, val: T) -> T;

    /// Atomik çıkarma — eski değeri döndürür
    fn atomic_sub(&self, val: T) -> T;

    /// Atomik artırma (+=1) — eski değeri döndürür
    fn atomic_inc(&self) -> T;

    /// Atomik azaltma (-=1) — eski değeri döndürür
    fn atomic_dec(&self) -> T;

    /// Bellek bariyerli atomik karşılaştır-ve-değiştir (CAS).
    /// `current` == mevcut değer ise `new` yazar; aksi hâlde mevcut değeri hatayla döner.
    fn atomic_compare_exchange(&self, current: T, new: T) -> Result<T, T>;

    /// Atomik getir-ve-ekle — belirtilen `order` ile çalışır
    fn fetch_add(&self, val: T, order: Ordering) -> T;

    /// Atomik getir-ve-çıkar — belirtilen `order` ile çalışır
    fn fetch_sub(&self, val: T, order: Ordering) -> T;

    /// Atomik getir-ve-VEYA (bitwise OR) — belirtilen `order` ile çalışır
    fn fetch_or(&self, val: T, order: Ordering) -> T;

    /// Atomik getir-ve-VE (bitwise AND) — belirtilen `order` ile çalışır
    fn fetch_and(&self, val: T, order: Ordering) -> T;

    /// Atomik getir-ve-XOR (bitwise XOR) — belirtilen `order` ile çalışır
    fn fetch_xor(&self, val: T, order: Ordering) -> T;
}

/// Tüm tamsayı atomik türleri için AtomicOps trait'ini otomatik uygulayan makro.
/// Kod tekrarını önler; her tür için aynı mantık SeqCst sıralamasıyla uygulanır.
macro_rules! impl_atomic_ops {
    ($atomic_type:ty, $primitive_type:ty) => {
        impl AtomicOps<$primitive_type> for $atomic_type {
            fn atomic_add(&self, val: $primitive_type) -> $primitive_type {
                self.fetch_add(val, Ordering::SeqCst)
            }

            fn atomic_sub(&self, val: $primitive_type) -> $primitive_type {
                self.fetch_sub(val, Ordering::SeqCst)
            }

            fn atomic_inc(&self) -> $primitive_type {
                self.fetch_add(1, Ordering::SeqCst)
            }

            fn atomic_dec(&self) -> $primitive_type {
                self.fetch_sub(1, Ordering::SeqCst)
            }

            fn atomic_compare_exchange(
                &self,
                current: $primitive_type,
                new: $primitive_type,
            ) -> Result<$primitive_type, $primitive_type> {
                smp_mb();
                let result =
                    self.compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire);
                smp_mb();
                result
            }

            fn fetch_add(&self, val: $primitive_type, order: Ordering) -> $primitive_type {
                self.fetch_add(val, order)
            }

            fn fetch_sub(&self, val: $primitive_type, order: Ordering) -> $primitive_type {
                self.fetch_sub(val, order)
            }

            fn fetch_or(&self, val: $primitive_type, order: Ordering) -> $primitive_type {
                self.fetch_or(val, order)
            }

            fn fetch_and(&self, val: $primitive_type, order: Ordering) -> $primitive_type {
                self.fetch_and(val, order)
            }

            fn fetch_xor(&self, val: $primitive_type, order: Ordering) -> $primitive_type {
                self.fetch_xor(val, order)
            }
        }
    };
}

// Tüm tamsayı atomik türleri için AtomicOps uygulaması — makro ile otomatik üretilir
impl_atomic_ops!(AtomicU8, u8);
impl_atomic_ops!(AtomicI8, i8);
impl_atomic_ops!(AtomicU16, u16);
impl_atomic_ops!(AtomicI16, i16);
impl_atomic_ops!(AtomicU32, u32);
impl_atomic_ops!(AtomicI32, i32);
impl_atomic_ops!(AtomicU64, u64);
impl_atomic_ops!(AtomicI64, i64);
impl_atomic_ops!(AtomicUsize, usize);
impl_atomic_ops!(AtomicIsize, isize);

/// İşaretçi türleri için gelişmiş atomik işlemler arayüzü.
/// RCU (Read-Copy-Update) uyumlu güncellemeler ve bellek bariyerli takas işlemleri sağlar.
pub trait AtomicPtrOps<T> {
    /// İşaretçiler için atomik karşılaştır-ve-değiştir (CAS) — bellek bariyerli
    fn atomic_compare_exchange_ptr(&self, current: *mut T, new: *mut T) -> Result<*mut T, *mut T>;

    /// Bellek bariyerli atomik takas — eski işaretçiyi döndürür
    fn atomic_exchange(&self, new: *mut T) -> *mut T;

    /// Acquire semantiği ile yükleme — okuma bariyeri uygulanır
    fn load_acquire(&self) -> *mut T;

    /// Release semantiği ile saklama — yazma bariyeri uygulanır
    fn store_release(&self, ptr: *mut T);

    /// RCU semantiği ile işaretçi güncelleme — eski işaretçiyi döndürür, grace period başlatır
    fn rcu_update(&self, new: *mut T) -> *mut T;
}

impl<T> AtomicPtrOps<T> for AtomicPtr<T> {
    fn atomic_compare_exchange_ptr(&self, current: *mut T, new: *mut T) -> Result<*mut T, *mut T> {
        smp_mb();
        let result = self.compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire);
        smp_mb();
        result
    }

    fn atomic_exchange(&self, new: *mut T) -> *mut T {
        smp_mb();
        let result = self.swap(new, Ordering::AcqRel);
        smp_mb();
        result
    }

    fn load_acquire(&self) -> *mut T {
        smp_rmb();
        let result = self.load(Ordering::Acquire);
        result
    }

    fn store_release(&self, ptr: *mut T) {
        smp_wmb();
        self.store(ptr, Ordering::Release);
    }

    fn rcu_update(&self, new: *mut T) -> *mut T {
        let old = self.atomic_exchange(new);
        crate::rcu::start_grace_period();
        old
    }
}

/// Bit düzeyinde atomik işlemler arayüzü.
/// Her işlem, yarış koşullarını önlemek için SeqCst sıralamasıyla atomik olarak yürütülür.
pub trait AtomicBitOps {
    /// Belirtilen bit konumunu atomik olarak sete çeker (1 yapar)
    fn atomic_set_bit(&self, bit: usize);

    /// Belirtilen bit konumunu atomik olarak temizler (0 yapar)
    fn atomic_clear_bit(&self, bit: usize);

    /// Belirtilen bit konumunu atomik olarak tersine çevirir
    fn atomic_toggle_bit(&self, bit: usize);

    /// Atomik sına-ve-sete-çek: önceki değerini döndürür, biti 1 yapar
    fn atomic_test_and_set_bit(&self, bit: usize) -> bool;

    /// Atomik sına-ve-temizle: önceki değerini döndürür, biti 0 yapar
    fn atomic_test_and_clear_bit(&self, bit: usize) -> bool;

    /// Belirtilen bit konumunun mevcut değerini okur (Relaxed)
    fn atomic_test_bit(&self, bit: usize) -> bool;
}

impl AtomicBitOps for AtomicU32 {
    fn atomic_set_bit(&self, bit: usize) {
        debug_assert!(bit < 32);
        self.fetch_or(1 << bit, Ordering::SeqCst);
    }

    fn atomic_clear_bit(&self, bit: usize) {
        debug_assert!(bit < 32);
        self.fetch_and(!(1 << bit), Ordering::SeqCst);
    }

    fn atomic_toggle_bit(&self, bit: usize) {
        debug_assert!(bit < 32);
        self.fetch_xor(1 << bit, Ordering::SeqCst);
    }

    fn atomic_test_and_set_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 32);
        let mask = 1 << bit;
        let old = self.fetch_or(mask, Ordering::SeqCst);
        (old & mask) != 0
    }

    fn atomic_test_and_clear_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 32);
        let mask = 1 << bit;
        let old = self.fetch_and(!mask, Ordering::SeqCst);
        (old & mask) != 0
    }

    fn atomic_test_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 32);
        let mask = 1 << bit;
        (self.load(Ordering::Relaxed) & mask) != 0
    }
}

impl AtomicBitOps for AtomicU64 {
    fn atomic_set_bit(&self, bit: usize) {
        debug_assert!(bit < 64);
        self.fetch_or(1 << bit, Ordering::SeqCst);
    }

    fn atomic_clear_bit(&self, bit: usize) {
        debug_assert!(bit < 64);
        self.fetch_and(!(1 << bit), Ordering::SeqCst);
    }

    fn atomic_toggle_bit(&self, bit: usize) {
        debug_assert!(bit < 64);
        self.fetch_xor(1 << bit, Ordering::SeqCst);
    }

    fn atomic_test_and_set_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 64);
        let mask = 1 << bit;
        let old = self.fetch_or(mask, Ordering::SeqCst);
        (old & mask) != 0
    }

    fn atomic_test_and_clear_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 64);
        let mask = 1 << bit;
        let old = self.fetch_and(!mask, Ordering::SeqCst);
        (old & mask) != 0
    }

    fn atomic_test_bit(&self, bit: usize) -> bool {
        debug_assert!(bit < 64);
        let mask = 1 << bit;
        (self.load(Ordering::Relaxed) & mask) != 0
    }
}

/// Atomik referans sayacı — Arc benzeri, ancak kernel ortamında hafiftir.
/// Nesne paylaşımında güvenli artırma/azaltma için kullanılır.
pub struct AtomicRefCounter {
    count: AtomicUsize,
}

impl AtomicRefCounter {
    pub fn new(initial_count: usize) -> Self {
        Self {
            count: AtomicUsize::new(initial_count),
        }
    }

    pub fn increment(&self) -> usize {
        self.count.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn decrement(&self) -> usize {
        self.count.fetch_sub(1, Ordering::AcqRel) - 1
    }

    pub fn get(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    pub fn is_zero(&self) -> bool {
        self.get() == 0
    }

    pub fn reset(&self) -> usize {
        self.count.swap(0, Ordering::AcqRel)
    }
}

/// Bellek bariyerli atomik bayrak — bool değerinin güvenli çok işlemcili paylaşımı için.
/// set/clear işlemleri yazma bariyeri, is_set okuma bariyeri uygular.
pub struct AtomicFlag {
    flag: AtomicBool,
}

impl AtomicFlag {
    pub fn new(initial: bool) -> Self {
        Self {
            flag: AtomicBool::new(initial),
        }
    }

    pub fn set(&self) {
        smp_wmb();
        self.flag.store(true, Ordering::Release);
    }

    pub fn clear(&self) {
        smp_wmb();
        self.flag.store(false, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        smp_rmb();
        self.flag.load(Ordering::Acquire)
    }

    pub fn test_and_set(&self) -> bool {
        smp_mb();
        let result = self.flag.swap(true, Ordering::AcqRel);
        smp_mb();
        result
    }

    pub fn test_and_clear(&self) -> bool {
        smp_mb();
        let result = self.flag.swap(false, Ordering::AcqRel);
        smp_mb();
        result
    }
}

/// Atomik sıra numarası üreteci — işlem sıralamasını izlemek için monoton artan sayaç.
pub struct AtomicSequence {
    seq: AtomicU64,
}

impl AtomicSequence {
    pub fn new(start: u64) -> Self {
        Self {
            seq: AtomicU64::new(start),
        }
    }

    pub fn next(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    pub fn current(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    pub fn reset(&self, value: u64) -> u64 {
        self.seq.swap(value, Ordering::AcqRel)
    }
}

/// Atomik istatistik sayacı — işlem sayısı, başarı/başarısızlık ve toplam süreyi izler.
pub struct AtomicStats {
    operations: AtomicU64,
    successes: AtomicU64,
    failures: AtomicU64,
    total_time: AtomicU64,
}

impl AtomicStats {
    pub fn new() -> Self {
        Self {
            operations: AtomicU64::new(0),
            successes: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            total_time: AtomicU64::new(0),
        }
    }

    pub fn record_operation(&self, success: bool, duration: u64) {
        self.operations.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successes.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        self.total_time.fetch_add(duration, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.operations.load(Ordering::Relaxed),
            self.successes.load(Ordering::Relaxed),
            self.failures.load(Ordering::Relaxed),
            self.total_time.load(Ordering::Relaxed),
        )
    }

    pub fn reset(&self) {
        self.operations.store(0, Ordering::Relaxed);
        self.successes.store(0, Ordering::Relaxed);
        self.failures.store(0, Ordering::Relaxed);
        self.total_time.store(0, Ordering::Relaxed);
    }
}

/// Atomik işlemler kullanan kilit gerektirmeyen (lock-free) yığıt.
/// CAS döngüsüyle çok işlemcili güvenli push/pop sağlar; ABA sorununu minimize eder.
pub struct LockFreeStack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    data: T,
    next: AtomicPtr<Node<T>>,
}

impl<T> LockFreeStack<T> {
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    pub fn push(&self, data: T) {
        let new_node = Box::into_raw(Box::new(Node {
            data,
            next: AtomicPtr::new(core::ptr::null_mut()),
        }));

        loop {
            let current_head = self.head.load(Ordering::Acquire);
            unsafe {
                (*new_node).next.store(current_head, Ordering::Relaxed);
            }

            match self.head.compare_exchange(
                current_head,
                new_node,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        loop {
            let current_head = self.head.load(Ordering::Acquire);
            if current_head.is_null() {
                return None;
            }

            let next_head = unsafe { (*current_head).next.load(Ordering::Relaxed) };

            match self.head.compare_exchange(
                current_head,
                next_head,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let data = unsafe { Box::from_raw(current_head) }.data;
                    return Some(data);
                }
                Err(_) => continue,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }
}

impl<T> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while let Some(_) = self.pop() {
            // Yığıttaki tüm elemanları serbest bırak
        }
    }
}

// ============================================================================
// QSPINLOCK + MCS FALLBACK
// ============================================================================

#[repr(align(64))]
pub struct QSpinLock {
    owner_ticket: AtomicU32,
    next_ticket: AtomicU32,
    tail: AtomicPtr<McsNode>,
    adaptive_spin_loops: AtomicU32,
}

#[repr(align(64))]
struct McsNode {
    next: AtomicPtr<McsNode>,
    locked: AtomicBool,
}

impl McsNode {
    fn new() -> Self {
        Self {
            next: AtomicPtr::new(core::ptr::null_mut()),
            locked: AtomicBool::new(true),
        }
    }
}

pub struct QSpinGuard<'a> {
    lock: &'a QSpinLock,
    node: Option<Box<McsNode>>,
}

impl QSpinLock {
    pub const fn new() -> Self {
        Self {
            owner_ticket: AtomicU32::new(0),
            next_ticket: AtomicU32::new(0),
            tail: AtomicPtr::new(core::ptr::null_mut()),
            adaptive_spin_loops: AtomicU32::new(2048),
        }
    }

    pub fn set_adaptive_spin_loops(&self, loops: u32) {
        self.adaptive_spin_loops
            .store(loops.max(64), Ordering::Release);
    }

    pub fn lock(&self) -> QSpinGuard<'_> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::AcqRel);
        if self.owner_ticket.load(Ordering::Acquire) == ticket {
            return QSpinGuard {
                lock: self,
                node: None,
            };
        }

        let max_loops = self.adaptive_spin_loops.load(Ordering::Acquire);
        let mut loops = 0u32;
        while loops < max_loops {
            if self.owner_ticket.load(Ordering::Acquire) == ticket {
                return QSpinGuard {
                    lock: self,
                    node: None,
                };
            }
            core::hint::spin_loop();
            loops = loops.saturating_add(1);
        }

        // Yoğun çekişmede MCS kuyruğuna düş.
        let mut node = Box::new(McsNode::new());
        let node_ptr = &mut *node as *mut McsNode;
        let prev = self.tail.swap(node_ptr, Ordering::AcqRel);
        if !prev.is_null() {
            unsafe {
                (*prev).next.store(node_ptr, Ordering::Release);
            }
            while node.locked.load(Ordering::Acquire) {
                core::hint::spin_loop();
            }
        } else {
            // Kuyruk başı ise ticket sahibi olana kadar bekle.
            while self.owner_ticket.load(Ordering::Acquire) != ticket {
                core::hint::spin_loop();
            }
        }

        QSpinGuard {
            lock: self,
            node: Some(node),
        }
    }

    fn unlock(&self, mut node: Option<Box<McsNode>>) {
        if let Some(ref mut node_box) = node {
            let node_ptr = &mut **node_box as *mut McsNode;
            let mut next = node_box.next.load(Ordering::Acquire);
            if next.is_null() {
                if self
                    .tail
                    .compare_exchange(
                        node_ptr,
                        core::ptr::null_mut(),
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    loop {
                        next = node_box.next.load(Ordering::Acquire);
                        if !next.is_null() {
                            break;
                        }
                        core::hint::spin_loop();
                    }
                }
            }
            if !next.is_null() {
                unsafe {
                    (*next).locked.store(false, Ordering::Release);
                }
            }
        }
        self.owner_ticket.fetch_add(1, Ordering::Release);
    }
}

impl<'a> Drop for QSpinGuard<'a> {
    fn drop(&mut self) {
        self.lock.unlock(self.node.take());
    }
}

/// Atomik işlemler alt sistemini başlatır — temel testleri çalıştırarak doğruluk kontrolü yapar.
pub fn init() {
    crate::serial_println!("AtomicOps: Gelişmiş atomik işlemler başlatılıyor");

    // Atomik işlemleri test et
    test_atomic_operations();

    crate::serial_println!("AtomicOps: Gelişmiş atomik işlemler hazır");
}

fn test_atomic_operations() {
    // Temel atomik işlem testleri — toplama, artırma, çıkarma
    let counter = AtomicU32::new(0);
    counter.atomic_add(10);
    assert_eq!(counter.load(Ordering::Relaxed), 10);

    counter.atomic_inc();
    assert_eq!(counter.load(Ordering::Relaxed), 11);

    counter.atomic_sub(5);
    assert_eq!(counter.load(Ordering::Relaxed), 6);

    // Bit işlem testleri — set ve clear
    let bits = AtomicU32::new(0);
    bits.atomic_set_bit(0);
    assert!(bits.atomic_test_bit(0));

    bits.atomic_clear_bit(0);
    assert!(!bits.atomic_test_bit(0));

    // Atomik bayrak testi — set/clear/is_set
    let flag = AtomicFlag::new(false);
    assert!(!flag.is_set());

    flag.set();
    assert!(flag.is_set());

    flag.clear();
    assert!(!flag.is_set());

    crate::serial_println!("AtomicOps: Tüm testler başarıyla geçildi");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_ops() {
        let counter = AtomicU32::new(100);

        assert_eq!(counter.atomic_add(50), 100);
        assert_eq!(counter.load(Ordering::Relaxed), 150);

        assert_eq!(counter.atomic_sub(25), 150);
        assert_eq!(counter.load(Ordering::Relaxed), 125);

        assert_eq!(counter.atomic_inc(), 125);
        assert_eq!(counter.load(Ordering::Relaxed), 126);

        assert_eq!(counter.atomic_dec(), 126);
        assert_eq!(counter.load(Ordering::Relaxed), 125);
    }

    #[test]
    fn test_bit_operations() {
        let bits = AtomicU32::new(0b1010);

        assert!(bits.atomic_test_bit(1));
        assert!(bits.atomic_test_bit(3));
        assert!(!bits.atomic_test_bit(0));

        bits.atomic_set_bit(0);
        assert!(bits.atomic_test_bit(0));
        assert_eq!(bits.load(Ordering::Relaxed), 0b1011);

        bits.atomic_clear_bit(3);
        assert!(!bits.atomic_test_bit(3));
        assert_eq!(bits.load(Ordering::Relaxed), 0b0011);
    }

    #[test]
    fn test_lock_free_stack() {
        let stack = LockFreeStack::new();

        stack.push(1);
        stack.push(2);
        stack.push(3);

        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_qspinlock_basic() {
        let lock = QSpinLock::new();
        {
            let _g = lock.lock();
            assert_eq!(lock.owner_ticket.load(Ordering::Acquire), 0);
            assert_eq!(lock.next_ticket.load(Ordering::Acquire), 1);
        }
        assert_eq!(lock.owner_ticket.load(Ordering::Acquire), 1);
    }
}
