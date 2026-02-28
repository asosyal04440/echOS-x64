//! # Çekirdek Stack Ayırıcısı
//!
//! Bu modül, çekirdek iş parçacıkları için fiziksel olarak bitişik bellek
//! sayfalarından stack (yığın) alanı ayıran `KernelStackAllocator`'ı içerir.
//!
//! Global heap ayırıcısını (bump/linked-list) atlatarak doğrudan Fiziksel Bellek
//! Yöneticisi'nden (PMM) sayfa tahsis eder. HHDM (Higher Half Direct Map) aracılığıyla
//! fiziksel adresleri sanal adrese çevirir.

use crate::memory::{global_memory_manager_mut, PHYSICAL_MEMORY_OFFSET};
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
    pages: usize,
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

        let mm = unsafe { global_memory_manager_mut() }?;
        let frame = mm.allocate_contiguous_frames(pages)?;

        let phys_addr = frame.start_address();
        // HHDM: fiziksel adrese sabit offset eklenerek sanal adres elde edilir
        let virt_addr = phys_addr.as_u64() + PHYSICAL_MEMORY_OFFSET;

        let ptr = NonNull::new(virt_addr as *mut u8)?;

        // Güvenlik ve deterministik davranış için belleği sıfırla.
        // Sıfırlanmamış stack, eski verilerden kaynaklanan güvenlik açıklarına
        // (information leak) yol açabilir.
        unsafe {
            ptr::write_bytes(ptr.as_ptr(), 0, pages * 4096);
        }

        Some(Self {
            ptr,
            pages,
            layout: core::alloc::Layout::from_size_align(pages * 4096, 4096).unwrap(),
        })
    }

    /// Stack'in fiziksel adresini döndürür.
    ///
    /// İki durumu ele alır:
    /// - HHDM eşlemeli stack: doğrudan offset çıkarma ile fiziksel adres hesaplanır.
    /// - Heap'ten ayrılan stack: sayfa tablosu çevirisi (translate_addr) kullanılır.
    pub fn phys_addr(&self) -> PhysAddr {
        let virt_addr = self.ptr.as_ptr() as u64;

        if virt_addr >= PHYSICAL_MEMORY_OFFSET {
            // HHDM eşlemeli stack: sanal adres - offset = fiziksel adres
            PhysAddr::new(virt_addr - PHYSICAL_MEMORY_OFFSET)
        } else {
            // Heap'ten ayrılan stack: sayfa tablosu çevirisi gerekli
            use x86_64::VirtAddr;
            crate::memory::paging::translate_addr(VirtAddr::new(virt_addr))
                .expect("KernelStack sanal adresi eşlenmemiş")
        }
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
        let mm = unsafe { global_memory_manager_mut() };
        if let Some(mm) = mm {
            let start_frame = PhysFrame::containing_address(self.phys_addr());
            mm.deallocate_contiguous_frames(start_frame, self.pages);
        }
    }
}

/// KernelStack'i byte dilimi (slice) olarak kullanmayı sağlar.
///
/// `deref` ile `&KernelStack` -> `&[u8]` dönüşümü yapılır.
impl Deref for KernelStack {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

/// KernelStack'i mutable byte dilimi olarak kullanmayı sağlar.
impl DerefMut for KernelStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.pages * 4096) }
    }
}

/// KernelStack'i klonlar: derin kopya (deep copy) yapılır.
///
/// Sığ kopya (shallow copy) burada anlamsız olur çünkü her iki kopya da
/// aynı fiziksel belleğe işaret ederdi ve drop sırasında çift serbest bırakma
/// (double free) hatası oluşurdu. Bu nedenle yeni fiziksel frame'ler tahsis
/// edilerek içerik kopyalanır.
impl Clone for KernelStack {
    fn clone(&self) -> Self {
        // Sahip olduğumuz belleği kopyalamak için derin kopya gereklidir
        let new_stack = Self::new(self.len()).expect("Stack klonu için bellek ayrılamadı");
        unsafe {
            ptr::copy_nonoverlapping(self.as_ptr(), new_stack.ptr.as_ptr(), self.len());
        }
        new_stack
    }
}
