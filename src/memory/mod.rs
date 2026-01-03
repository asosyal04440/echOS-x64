//! # echOS Bellek Yönetimi
//! 
//! Bu modül, fiziksel ve sanal bellek yönetimini sağlar.
//! UEFI memory map'i kullanarak sayfa tablosu ve frame allocation yapar.

use uefi::boot::MemoryType;
use uefi::Error;
use uefi::mem::memory_map::{MemoryMapOwned, MemoryMapIter, MemoryMap};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB, OffsetPageTable, PageTable};
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;

pub mod pmm;
pub mod paging;

// ============================================================================
// MEMORY MANAGER
// ============================================================================

/// Ana bellek yöneticisi.
/// UEFI memory map ve bitmap-based PMM kullanır.
pub struct MemoryManager {
    /// UEFI'den alınan bellek haritası
    memory_map: MemoryMapOwned,
    /// Bitmap tabanlı fiziksel bellek yöneticisi
    pmm: pmm::BitmapPmm,
}

impl MemoryManager {
    /// Yeni bir MemoryManager oluşturur.
    /// 
    /// # Parametreler
    /// - `memory_map`: UEFI'den alınan bellek haritası
    pub fn new(memory_map: MemoryMapOwned) -> Self {
        let mut pmm = pmm::BitmapPmm::empty();
        unsafe {
            pmm.init(memory_map.entries());
        }

        MemoryManager {
            memory_map,
            pmm,
        }
    }

    /// UEFI bellek haritası üzerinde iterator döndürür.
    #[allow(dead_code)]
    pub fn get_memory_map(&self) -> MemoryMapIter<'_> {
        self.memory_map.entries()
    }
}

/// x86_64 FrameAllocator trait implementasyonu.
/// Scheduler ve paging sistemi için gerekli.
unsafe impl FrameAllocator<Size4KiB> for MemoryManager {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.pmm.allocate_frame()
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Bellek yöneticisini başlatır.
pub fn init() -> Result<MemoryManager, Error> {
    let memory_map = uefi::boot::memory_map(MemoryType::LOADER_DATA)?;
    Ok(MemoryManager::new(memory_map))
}

/// Kernel'in PML4 frame'i (scheduler context switch için)
pub static mut KERNEL_PML4_FRAME: Option<PhysFrame> = None;

/// Sayfa tablosunu başlatır.
/// 
/// # Güvenlik
/// Bu fonksiyon fiziksel belleğin identity-mapped olduğunu varsayar.
/// UEFI için offset genellikle 0'dır.
/// 
/// # Parametreler
/// - `physical_memory_offset`: Fiziksel-sanal adres farkı
pub unsafe fn init_paging(physical_memory_offset: u64) -> OffsetPageTable<'static> {
    let (level_4_table_frame, _) = Cr3::read();
    
    // Kernel PML4'ü scheduler için kaydet
    KERNEL_PML4_FRAME = Some(level_4_table_frame);

    let phys = level_4_table_frame.start_address();
    let virt = VirtAddr::new(physical_memory_offset + phys.as_u64());
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    let level_4_table = &mut *page_table_ptr;
    OffsetPageTable::new(level_4_table, VirtAddr::new(physical_memory_offset))
}