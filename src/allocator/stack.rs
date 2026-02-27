//! # Çekirdek Yığını (Kernel Stack) Ayırıcı
//!
//! PMM (Fiziksel Bellek Yöneticisi) üzerinden doğrudan ardışık fiziksel
//! bellek sayfaları ayırır. Global heap allocator'ı devre dışı bırakarak
//! parçalanmayı (fragmantasyonu) önler.
//! Belleğe erişmek için doğrudan eşleme (Fiziksel Adres + Offset) kullanır.

use crate::memory::{global_memory_manager_mut, PHYSICAL_MEMORY_OFFSET};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Çekirdek Yığın (Stack) Ayırıcı.
///
/// PMM'den doğrudan ardışık fiziksel bellek sayfaları ayırır,
/// parçalanmayı önlemek için global heap allocator'ı atlar.
///
/// Belleğe erişmek için doğrudan eşleme (Fiziksel Adres + Offset) kullanır.
#[derive(Debug)]
pub struct KernelStack {
    ptr: NonNull<u8>,
    pages: usize,
    layout: core::alloc::Layout, // Kullanılmıyor, gelecekteki hizalama için saklandı
}

// Belleğin sahibi olduğu için Send + Sync
unsafe impl Send for KernelStack {}
unsafe impl Sync for KernelStack {}

impl KernelStack {
    /// Verilen boyutta (bayt cinsinden) yeni bir Kernel Stack ayırır.
    /// Boyut sayfa sınırlarına yuvarlanır.
    pub fn new(size_in_bytes: usize) -> Option<Self> {
        if size_in_bytes == 0 {
            return None;
        }

        let pages = (size_in_bytes + 4095) / 4096;

        let mm = unsafe { global_memory_manager_mut() }?;
        let frame = mm.allocate_contiguous_frames(pages)?;

        let phys_addr = frame.start_address();
        let virt_addr = phys_addr.as_u64() + PHYSICAL_MEMORY_OFFSET;

        let ptr = NonNull::new(virt_addr as *mut u8)?;

        // Güvenlik ve belirlilik için belleği sıfırla
        unsafe {
            ptr::write_bytes(ptr.as_ptr(), 0, pages * 4096);
        }

        Some(Self {
            ptr,
            pages,
            layout: core::alloc::Layout::from_size_align(pages * 4096, 4096).unwrap(),
        })
    }

    /// Yığının fiziksel adresini döndürür.
    pub fn phys_addr(&self) -> PhysAddr {
        let virt_addr = self.ptr.as_ptr() as u64;

        if virt_addr >= PHYSICAL_MEMORY_OFFSET {
            // HHDM eşlemeli yığın: doğrudan offset hesaplaması kullan
            PhysAddr::new(virt_addr - PHYSICAL_MEMORY_OFFSET)
        } else {
            // Heap'te ayrılan yığın: sayfa tablosu çevirisi kullan
            use x86_64::VirtAddr;
            crate::memory::paging::translate_addr(VirtAddr::new(virt_addr))
                .expect("KernelStack virtual address is not mapped")
        }
    }

    /// Yığının sanal başlangıç adresini döndürür.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Yığının değiştirilebilir sanal başlangıç adresini döndürür.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Yığının bayt cinsinden boyutunu döndürür.
    pub fn len(&self) -> usize {
        self.pages * 4096
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let mm = unsafe { global_memory_manager_mut() };
        if let Some(mm) = mm {
            let start_frame = PhysFrame::containing_address(self.phys_addr());
            mm.deallocate_contiguous_frames(start_frame, self.pages);
        }
    }
}

impl Deref for KernelStack {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

impl DerefMut for KernelStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

impl Clone for KernelStack {
    fn clone(&self) -> Self {
        // Belleğin sahibi olduğumuz için derin kopya gerekli
        let new_stack = Self::new(self.len()).expect("Failed to allocate stack clone");
        unsafe {
            ptr::copy_nonoverlapping(self.as_ptr(), new_stack.ptr.as_ptr(), self.len());
        }
        new_stack
    }
}
