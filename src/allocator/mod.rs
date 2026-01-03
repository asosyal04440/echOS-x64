//! # echOS Heap Allocator
//! 
//! Kernel heap bellek yönetimi.
//! TLSF (Two-Level Segregated Fit) algoritması kullanır.
//! O(1) zaman karmaşıklığı ile allocation/deallocation.

pub mod tlsf;
use tlsf::LockedTlsf;

use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Size4KiB, Page, PageTableFlags,
    },
    VirtAddr,
};

/// Heap başlangıç adresi (sanal bellek)
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Heap boyutu (100 MiB)
pub const HEAP_SIZE: usize = 100 * 1024 * 1024;

/// Global TLSF allocator
#[global_allocator]
static ALLOCATOR: LockedTlsf = LockedTlsf::new();

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
    // Heap sayfa aralığını hesapla
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // Her sayfa için fiziksel frame map et
    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    // TLSF allocator'a serbest bölgeyi ekle
    unsafe {
        ALLOCATOR.insert_free_region_ptr(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}
