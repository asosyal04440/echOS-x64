//! Cacheline-aligned SPSC ring for zero-copy desktop command/event transport.

use alloc::vec;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
pub struct SharedRing<T> {
    head: AtomicUsize,
    tail: AtomicUsize,
    mask: usize,
    slots: Vec<MaybeUninit<T>>,
}

impl<T> SharedRing<T> {
    pub fn with_capacity_pow2(capacity: usize) -> Self {
        let cap = capacity.max(2).next_power_of_two();
        let mut slots = Vec::with_capacity(cap);
        slots.resize_with(cap, MaybeUninit::uninit);
        Self {
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            mask: cap - 1,
            slots,
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let next_tail = tail.wrapping_add(1);
        let head = self.head.load(Ordering::Acquire);
        if next_tail.wrapping_sub(head) > self.capacity() {
            return Err(value);
        }
        self.slots[tail & self.mask].write(value);
        self.tail.store(next_tail, Ordering::Release);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let value = unsafe { self.slots[head & self.mask].as_ptr().read() };
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    pub fn drain(&mut self, max_items: usize) -> Vec<T> {
        let mut out = vec![];
        let limit = max_items.max(1);
        while out.len() < limit {
            match self.pop() {
                Some(item) => out.push(item),
                None => break,
            }
        }
        out
    }

    pub fn clear(&mut self) {
        while self.pop().is_some() {}
    }
}
