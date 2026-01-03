//! # echOS TLSF Allocator
//! 
//! TLSF (Two-Level Segregated Fit) heap allocator wrapper.
//! O(1) allocation/deallocation performansı sağlar.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use spin::Mutex;
use rlsf::Tlsf;

/// Thread-safe TLSF allocator wrapper.
pub struct LockedTlsf(Mutex<Option<Tlsf<'static, usize, usize, 32, 32>>>);

unsafe impl Send for LockedTlsf {}
unsafe impl Sync for LockedTlsf {}

impl LockedTlsf {
    /// Yeni boş allocator oluşturur.
    pub const fn new() -> Self {
        LockedTlsf(Mutex::new(None))
    }

    /// Bellek bölgesini allocator'a kaydeder.
    /// 
    /// # Güvenlik
    /// Verilen bellek bölgesi geçerli ve allocator tarafından
    /// kullanılabilir olmalıdır.
    pub unsafe fn insert_free_region_ptr(&self, ptr: *mut u8, size: usize) {
        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();
        
        let slice_ptr = core::ptr::slice_from_raw_parts_mut(ptr, size);
        if let Some(nonnull_slice) = NonNull::new(slice_ptr) {
            tlsf.insert_free_block_ptr(nonnull_slice);
        }
    }
}

unsafe impl GlobalAlloc for LockedTlsf {
    /// Bellek ayırır.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut lock = self.0.lock();
        if lock.is_none() {
            *lock = Some(Tlsf::new());
        }
        let tlsf = lock.as_mut().unwrap();
        
        match tlsf.allocate(layout) {
            Some(ptr) => ptr.as_ptr(),
            None => core::ptr::null_mut(),
        }
    }

    /// Bellek serbest bırakır.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut lock = self.0.lock();
        if let Some(tlsf) = lock.as_mut() {
            if let Some(ptr) = NonNull::new(ptr) {
                tlsf.deallocate(ptr, layout.align());
            }
        }
    }
}
