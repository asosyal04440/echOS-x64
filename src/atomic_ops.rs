//! # echOS Advanced Atomic Operations Module
//!
//! Tier 1 OS seviyesinde atomic operations
//! Linux atomic operations ile aynı seviyede performans ve güvenlik

use core::sync::atomic::{AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, 
                         AtomicIsize, AtomicPtr, AtomicU16, AtomicU32, AtomicU64, 
                         AtomicU8, AtomicUsize, Ordering};
use alloc::boxed::Box;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};

/// Advanced atomic operations for integers
pub trait AtomicOps<T> {
    /// Atomic add with return value
    fn atomic_add(&self, val: T) -> T;
    
    /// Atomic subtract with return value
    fn atomic_sub(&self, val: T) -> T;
    
    /// Atomic increment with return value
    fn atomic_inc(&self) -> T;
    
    /// Atomic decrement with return value
    fn atomic_dec(&self) -> T;
    
    /// Atomic compare and swap with memory barriers
    fn atomic_compare_exchange(&self, current: T, new: T) -> Result<T, T>;
    
    /// Atomic fetch and add
    fn fetch_add(&self, val: T, order: Ordering) -> T;
    
    /// Atomic fetch and subtract
    fn fetch_sub(&self, val: T, order: Ordering) -> T;
    
    /// Atomic fetch and or
    fn fetch_or(&self, val: T, order: Ordering) -> T;
    
    /// Atomic fetch and and
    fn fetch_and(&self, val: T, order: Ordering) -> T;
    
    /// Atomic fetch and xor
    fn fetch_xor(&self, val: T, order: Ordering) -> T;
}

/// Macro to implement AtomicOps for integer types
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
            
            fn atomic_compare_exchange(&self, current: $primitive_type, new: $primitive_type) -> Result<$primitive_type, $primitive_type> {
                smp_mb();
                let result = self.compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire);
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

// Implement AtomicOps for all integer atomic types
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

/// Advanced atomic operations for pointers
pub trait AtomicPtrOps<T> {
    /// Atomic compare and swap for pointers
    fn atomic_compare_exchange_ptr(&self, current: *mut T, new: *mut T) -> Result<*mut T, *mut T>;
    
    /// Atomic exchange with memory barriers
    fn atomic_exchange(&self, new: *mut T) -> *mut T;
    
    /// Load with acquire semantics
    fn load_acquire(&self) -> *mut T;
    
    /// Store with release semantics
    fn store_release(&self, ptr: *mut T);
    
    /// Update pointer with RCU semantics
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

/// Atomic bit operations
pub trait AtomicBitOps {
    /// Atomic set bit
    fn atomic_set_bit(&self, bit: usize);
    
    /// Atomic clear bit
    fn atomic_clear_bit(&self, bit: usize);
    
    /// Atomic toggle bit
    fn atomic_toggle_bit(&self, bit: usize);
    
    /// Atomic test and set bit
    fn atomic_test_and_set_bit(&self, bit: usize) -> bool;
    
    /// Atomic test and clear bit
    fn atomic_test_and_clear_bit(&self, bit: usize) -> bool;
    
    /// Atomic test bit
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

/// Atomic reference counter
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

/// Atomic flag with memory barriers
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

/// Atomic sequence number generator
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

/// Atomic statistics counter
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

/// Lock-free stack using atomic operations
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
            unsafe { (*new_node).next.store(current_head, Ordering::Relaxed); }
            
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
            // Drain the stack
        }
    }
}

/// Initialize atomic operations subsystem
pub fn init() {
    crate::serial_println!("AtomicOps: Initializing advanced atomic operations");
    
    // Test atomic operations
    test_atomic_operations();
    
    crate::serial_println!("AtomicOps: Advanced atomic operations ready");
}

fn test_atomic_operations() {
    // Test basic atomic operations
    let counter = AtomicU32::new(0);
    counter.atomic_add(10);
    assert_eq!(counter.load(Ordering::Relaxed), 10);
    
    counter.atomic_inc();
    assert_eq!(counter.load(Ordering::Relaxed), 11);
    
    counter.atomic_sub(5);
    assert_eq!(counter.load(Ordering::Relaxed), 6);
    
    // Test bit operations
    let bits = AtomicU32::new(0);
    bits.atomic_set_bit(0);
    assert!(bits.atomic_test_bit(0));
    
    bits.atomic_clear_bit(0);
    assert!(!bits.atomic_test_bit(0));
    
    // Test atomic flag
    let flag = AtomicFlag::new(false);
    assert!(!flag.is_set());
    
    flag.set();
    assert!(flag.is_set());
    
    flag.clear();
    assert!(!flag.is_set());
    
    crate::serial_println!("AtomicOps: All tests passed");
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
}
