//! # Çekirdek Stack Ayırıcısı
//!
//! Bu modül, çekirdek iş parçacıkları için fiziksel olarak bitişik bellek
//! sayfalarından stack (yığın) alanı ayıran `KernelStackAllocator`'ı içerir.
//!
//! Global heap ayırıcısını (bump/linked-list) atlatarak doğrudan Fiziksel Bellek
//! Yöneticisi'nden (PMM) sayfa tahsis eder. HHDM (Higher Half Direct Map) aracılığıyla
//! fiziksel adresleri sanal adrese çevirir.

use crate::memory::{
    allocate_contiguous_frames, deallocate_contiguous_frames, map_kernel_stack_pages,
    remap_kernel_guard_page, unmap_kernel_guard_page, unmap_kernel_stack_pages,
};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::slice;
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Kernel Stack Allocator.
///
/// Kernel stack'leri için PMM'den (Fiziksel Bellek Yöneticisi) doğrudan
/// bitişik fiziksel bellek sayfaları ayırır. Global heap allocator'ı atlatarak
/// fragmantasyonu önler.
///
/// Belleğe erişmek için doğrudan eşleme (Fiziksel Adres + Offset) kullanır.
/// Bu teknik HHDM (Higher Half Direct Map) olarak bilinir: tüm fiziksel bellek
/// sabit bir sanal adres ofseti ile erişilebilir durumdadır.
///
/// ## Stack Bellek Düzeni:
/// ```
/// Fiziksel: [frame0][frame1]...[frameN]  (bitişik N sayfa)
///                |
///          + PHYSICAL_MEMORY_OFFSET
///                |
///                v
/// Sanal:   [ptr] --> kullanılabilir stack alanı (N * 4096 byte)
///                                                ^-- stack tepesi (top)
/// ```
///
/// ## Neden Heap Yerine PMM?
/// Kernel stack'leri sabitlenmiş boyutlarda olup uzun ömürlüdür.
/// Heap allocator'dan alınmaları gereksiz fragmentasyon ve kilitleme
/// gecikmesine yol açar. PMM ile doğrudan sayfa tahsisi daha hızlı
/// ve öngörülebilirdir.
#[derive(Debug)]
pub struct KernelStack {
    ptr: NonNull<u8>,
    phys_start: PhysAddr,
    pages: usize,
    guard_pages: usize,
    layout: core::alloc::Layout, // İlerideki hizalama ihtiyaçları için saklanır; şu an kullanılmıyor
}

// KernelStack belleğin sahipliğini aldığı için Send + Sync güvenli kabul edilir.
// Farklı CPU'lar farklı stack'lere güvenle erişebilir.
unsafe impl Send for KernelStack {}
unsafe impl Sync for KernelStack {}

impl KernelStack {
    /// Belirtilen boyutta (byte cinsinden) yeni bir Kernel Stack ayırır.
    ///
    /// Boyut, sayfa sınırlarına (4096 byte = 4 KiB) yukarı yuvarlanır.
    /// Örneğin 5000 byte istenirse 2 sayfa (8192 byte) tahsis edilir.
    ///
    /// ## Tahsis Adımları:
    /// ```
    /// new(size_in_bytes)
    ///      |
    ///      v
    /// pages = (size + 4095) / 4096   (sayfa sayısına yukarı yuvarla)
    ///      |
    ///      v
    /// PMM'den bitişik fiziksel frame'ler al
    ///      |
    ///      v
    /// Fiziksel adres + PHYSICAL_MEMORY_OFFSET = sanal adres
    ///      |
    ///      v
    /// Güvenlik için sıfırla (write_bytes 0)
    ///      |
    ///      v
    /// KernelStack döndür
    /// ```
    pub fn new(size_in_bytes: usize) -> Option<Self> {
        if size_in_bytes == 0 {
            return None;
        }

        // Sayfa sayısına yuvarla: her zaman tam sayfa tahsis edilir
        let pages = (size_in_bytes + 4095) / 4096;

        let frame = allocate_contiguous_frames(pages)?;

        let phys_addr = frame.start_address();
        let virt_addr = match map_kernel_stack_pages(phys_addr.as_u64(), pages) {
            Some(value) => value,
            None => {
                deallocate_contiguous_frames(frame, pages);
                return None;
            }
        };

        let ptr = NonNull::new(virt_addr as *mut u8)?;

        // Güvenlik ve deterministik davranış için belleği sıfırla.
        // Sıfırlanmamış stack, eski verilerden kaynaklanan güvenlik açıklarına
        // (information leak) yol açabilir.
        unsafe {
            ptr::write_bytes(ptr.as_ptr(), 0, pages * 4096);
        }

        let stack_bytes = pages.checked_mul(4096)?;
        let layout = core::alloc::Layout::from_size_align(stack_bytes, 4096).ok()?;

        Some(Self {
            ptr,
            phys_start: phys_addr,
            pages,
            guard_pages: 0,
            layout,
        })
    }

    pub fn enable_guard_pages(&mut self, guard_pages: usize) -> bool {
        if guard_pages == 0 {
            self.guard_pages = 0;
            return true;
        }
        if guard_pages >= self.pages {
            return false;
        }

        let guard_bytes = guard_pages.saturating_mul(4096);
        for page_idx in 0..guard_pages {
            let guard_virt = self.ptr.as_ptr() as u64 + (page_idx * 4096) as u64;
            if !unmap_kernel_guard_page(guard_virt) {
                for rollback_idx in 0..page_idx {
                    let rollback_virt = self.ptr.as_ptr() as u64 + (rollback_idx * 4096) as u64;
                    let rollback_phys = self.phys_addr().as_u64() + (rollback_idx * 4096) as u64;
                    let _ = remap_kernel_guard_page(rollback_virt, rollback_phys);
                }
                return false;
            }
        }
        self.guard_pages = guard_pages;
        debug_assert!(guard_bytes < self.len());
        true
    }

    pub fn guard_pages(&self) -> usize {
        self.guard_pages
    }

    pub fn usable_ptr(&self) -> *const u8 {
        unsafe { self.ptr.as_ptr().add(self.guard_pages * 4096) }
    }

    pub fn usable_mut_ptr(&mut self) -> *mut u8 {
        unsafe { self.ptr.as_ptr().add(self.guard_pages * 4096) }
    }

    pub fn usable_len(&self) -> usize {
        self.len().saturating_sub(self.guard_pages * 4096)
    }

    /// Stack'in fiziksel adresini döndürür.
    pub fn phys_addr(&self) -> PhysAddr {
        self.phys_start
    }

    /// Stack başlangıcının sanal adresini (immutable pointer) döndürür.
    ///
    /// Not: Stack x86-64'te yukarıdan aşağıya büyür. Stack tepesi (RSP),
    /// başlangıç + boyut adresinden başlar ve aşağı doğru ilerler.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Stack başlangıcının sanal adresini (mutable pointer) döndürür.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Stack'in byte cinsinden boyutunu döndürür.
    ///
    /// Her zaman 4096'nın katıdır (sayfa hizalı).
    pub fn len(&self) -> usize {
        self.pages * 4096
    }
}

/// KernelStack düşürüldüğünde (drop) fiziksel frame'leri PMM'ye iade eder.
///
/// Rust'ın sahiplik sistemi sayesinde stack sızıntısı (stack leak) önlenir:
/// KernelStack scope dışına çıktığında otomatik olarak bellek serbest bırakılır.
impl Drop for KernelStack {
    fn drop(&mut self) {
        if !unmap_kernel_stack_pages(self.ptr.as_ptr() as u64, self.pages) {
            crate::serial_println!(
                "[STACK] stack VA unmap incomplete; physical frames still returned phys={:#x}",
                self.phys_start.as_u64()
            );
        }
        let start_frame = PhysFrame::containing_address(self.phys_start);
        deallocate_contiguous_frames(start_frame, self.pages);
    }
}

/// KernelStack'i byte dilimi (slice) olarak kullanmayı sağlar.
///
/// `deref` ile `&KernelStack` -> `&[u8]` dönüşümü yapılır.
impl Deref for KernelStack {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.usable_ptr(), self.usable_len()) }
    }
}

/// KernelStack'i mutable byte dilimi olarak kullanmayı sağlar.
impl DerefMut for KernelStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.usable_mut_ptr(), self.usable_len()) }
    }
}

/// KernelStack'i klonlar: derin kopya (deep copy) yapılır.
///
/// Sığ kopya (shallow copy) burada anlamsız olur çünkü her iki kopya da
/// aynı fiziksel belleğe işaret ederdi ve drop sırasında çift serbest bırakma
/// (double free) hatası oluşurdu. Bu nedenle yeni fiziksel frame'ler tahsis
/// edilerek içerik kopyalanır.
impl KernelStack {
    pub fn try_clone_stack(&self) -> Option<Self> {
        // Sahip olduğumuz belleği kopyalamak için derin kopya gereklidir.
        let mut new_stack = Self::new(self.len())?;
        if self.guard_pages != 0 && !new_stack.enable_guard_pages(self.guard_pages) {
            return None;
        }
        unsafe {
            ptr::copy_nonoverlapping(
                self.usable_ptr(),
                new_stack.usable_mut_ptr(),
                self.usable_len(),
            );
        }
        Some(new_stack)
    }
}
