//! # echOS Heap Allocator
//!
//! Kernel heap bellek yönetimi.
//! TLSF (Two-Level Segregated Fit) algoritması kullanır.
//! O(1) zaman karmaşıklığı ile allocation/deallocation.

pub mod tlsf;
pub mod stack;
use tlsf::LockedTlsf;

use crate::memory::paging;
use core::alloc::Layout;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

/// Heap başlangıç adresi (sanal bellek)
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Heap boyutu (100 MiB)
pub const HEAP_SIZE: usize = 100 * 1024 * 1024;

/// Global TLSF allocator
#[global_allocator]
pub static ALLOCATOR: LockedTlsf = LockedTlsf::new();

/// Heap bütünlüğünü kontrol et (genel sarmalayıcı)
pub fn check_heap_integrity() -> usize {
    LockedTlsf::check_heap_integrity()
}

/// Allocation istatistiklerini al (genel sarmalayıcı)
pub fn get_alloc_stats() -> tlsf::AllocStats {
    LockedTlsf::get_stats()
}

/// Heap'in başlatılıp başlatılmadığını izlemek için bayrak
static HEAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Heap bellek alanını başlatır.
///
/// Sayfa tablosunda gerekli mapping'leri yapar ve
/// TLSF allocator'a serbest bölgeyi ekler.
///
/// # Parametreler
/// - `mapper`: Sayfa tablosu mapper'ı
/// - `frame_allocator`: Fiziksel frame allocator
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // Zaten başlatıldı mı kontrol et
    if HEAP_INITIALIZED.load(Ordering::Acquire) {
        return Ok(());
    }

    // Heap sayfa aralığını hesapla
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // Her sayfa için fiziksel frame map et
    let map_result: Result<(), MapToError<Size4KiB>> = paging::with_wp_disabled(|| {
        let mut mapped_pages: usize = 0;
        for page in page_range {
            if mapped_pages == 0 {
                crate::serial_println!("[HEAP] mapping start");
                crate::serial_println!("[HEAP] allocating first frame");
            }
            let frame = frame_allocator
                .allocate_frame()
                .ok_or(MapToError::FrameAllocationFailed)?;
            if mapped_pages == 0 {
                crate::serial_println!("[HEAP] allocated first frame");
            }
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            let _ = unsafe { mapper.map_to(page, frame, flags, frame_allocator) }?;
            mapped_pages = mapped_pages.saturating_add(1);
            if mapped_pages == 1 {
                crate::serial_println!("[HEAP] mapped first page");
            }
            if (mapped_pages & 0xFFF) == 0 {
                crate::serial_println!("[HEAP] mapped pages: {}", mapped_pages);
            }
        }
        Ok(())
    });
    map_result?;

    // TLSF allocator'a serbest bölgeyi ekle
    unsafe {
        ALLOCATOR.insert_free_region_ptr(HEAP_START as *mut u8, HEAP_SIZE);
    }

    // Başlatıldı olarak işaretle
    HEAP_INITIALIZED.store(true, Ordering::Release);

    Ok(())
}

pub unsafe fn heap_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }
    let header = core::mem::size_of::<usize>();
    let total = size.saturating_add(header);
    let layout = Layout::from_size_align(total, core::mem::align_of::<usize>()).unwrap();
    let raw = alloc::alloc::alloc(layout);
    if raw.is_null() {
        return ptr::null_mut();
    }
    (raw as *mut usize).write(size);
    raw.add(header)
}

pub unsafe fn heap_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let header = core::mem::size_of::<usize>();
    let raw = ptr.sub(header);
    let size = (raw as *mut usize).read();
    let total = size.saturating_add(header);
    let layout = Layout::from_size_align(total, core::mem::align_of::<usize>()).unwrap();
    alloc::alloc::dealloc(raw, layout);
}
