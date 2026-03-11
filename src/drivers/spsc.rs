//! # SPSC Lock-Free Ring Buffer
//! 
//! Single-Producer Single-Consumer queue for zero-latency input pipelines.
//! No Mutex, no locking. Optimized for cacheline boundaries.

use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::boxed::Box;
use alloc::vec::Vec;

pub struct SpscQueue<T, const N: usize> {
    buffer: Box<[Option<T>; N]>,
    head: AtomicUsize, // Consumer index
    tail: AtomicUsize, // Producer index
}

impl<T, const N: usize> SpscQueue<T, N> {
    pub fn new() -> Self {
        // Power of two check for bitmasking
        assert!(N.is_power_of_two(), "SPSC Queue size must be power of two");
        
        let mut v: Vec<Option<T>> = Vec::with_capacity(N);
        for _ in 0..N { v.push(None); }
        let buffer: Box<[Option<T>]> = v.into_boxed_slice();
        
        // Safety: We ensure N is power of two and indices are managed correctly.
        let buffer_ptr = Box::into_raw(buffer) as *mut [Option<T>; N];
        let buffer = unsafe { Box::from_raw(buffer_ptr) };

        Self {
            buffer,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: Push an item into the queue.
    /// Returns Err if full.
    pub fn push(&self, item: T) -> Result<(), T> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        
        if tail.wrapping_sub(head) >= N {
            return Err(item); // Full
        }

        // Safety: Only one producer, so tail is stable.
        // We use raw pointer access to bypass borrow checker for the buffer.
        let slot = unsafe {
            let ptr = self.buffer.as_ptr() as *mut Option<T>;
            &mut *ptr.add(tail & (N - 1))
        };
        
        *slot = Some(item);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Consumer: Pop an item from the queue.
    /// Returns None if empty.
    pub fn pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        if head == tail {
            return None; // Empty
        }

        let slot = unsafe {
            let ptr = self.buffer.as_ptr() as *mut Option<T>;
            &mut *ptr.add(head & (N - 1))
        };

        let item = slot.take();
        self.head.store(head.wrapping_add(1), Ordering::Release);
        item
    }
}

// SpscQueue is Send/Sync if T is Send
unsafe impl<T: Send, const N: usize> Send for SpscQueue<T, N> {}
unsafe impl<T: Send, const N: usize> Sync for SpscQueue<T, N> {}
