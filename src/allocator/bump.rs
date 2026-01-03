//! # echOS Bump Allocator
//! 
//! Basit ve hızlı bir doğrusal (linear) bellek ayırıcı.
//! Bellek iadesi (deallocation) desteklemez, sadece ileriye doğru büyür.
//! Küçük ve kısa ömürlü kernel projeleri veya boot aşaması için uygundur.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

/// Bump Allocator yapısı.
pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
}

impl BumpAllocator {
    /// Yeni boş bir Bump Allocator oluşturur.
    pub const fn new() -> Self {
        BumpAllocator {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Allocator'ı verilen heap aralığı ile başlatır.
    /// 
    /// # Güvenlik
    /// Çağıran kişi, verilen bellek aralığının kullanımda olmadığından emin olmalıdır.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    /// Bellek ayırma işlemi.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let alloc_start = align_up(self.next, layout.align());
        let alloc_end = match alloc_start.checked_add(layout.size()) {
            Some(end) => end,
            None => return ptr::null_mut(),
        };

        if alloc_end > self.heap_end {
            ptr::null_mut() // Bellek yetersiz
        } else {
            let next = alloc_end;
            let allocations = self.allocations + 1;
            
            // `self` immutable referans olduğu için ve `Mutex` ile sarmalanmadığı varsayımıyla (GlobalAlloc için genelde wrapper olur)
            // burada iç mutability için raw pointer kullanıyoruz. Normalde LockedHeap wrapper'ı bunu halleder.
            // Ancak bu basit implementasyonda doğrudan modifikasyon yapılıyor.
            let self_ptr = self as *const Self as *mut Self;
            unsafe {
                (*self_ptr).next = next;
                (*self_ptr).allocations = allocations;
            }
            
            alloc_start as *mut u8
        }
    }

    /// Bellek iade işlemi.
    /// Bump allocator spesifik blokları serbest bırakamaz.
    /// Ancak tüm allocasyonlar bittiğinde sayacı sıfırlayabilir.
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        let self_ptr = self as *const Self as *mut Self;
        unsafe {
            (*self_ptr).allocations = self.allocations.saturating_sub(1);
            
            // Eğer tüm objeler silindiyse başa sarabiliriz.
            if self.allocations == 0 {
                (*self_ptr).next = self.heap_start;
            }
        }
    }
}

/// Adresi verilen hizalamaya (align) göre yukarı yuvarlar.
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
