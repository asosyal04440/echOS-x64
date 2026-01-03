//! # echOS Paging (Sanal Bellek)
//! 
//! x86_64 Page Table yönetimi.
//! Sanal bellek ilklendirme ve PML4 tablosu oluşturma.

use x86_64::structures::paging::{PageTable, OffsetPageTable, PageTableFlags, PhysFrame, Size4KiB, FrameAllocator};
use x86_64::{PhysAddr, VirtAddr};
use x86_64::registers::control::Cr3;

/// Sanal belleği başlatır ve yeni bir PML4 tablosu oluşturur.
/// 
/// Hem Identity Mapping (Boot için) hem de Higher-Half Mapping (Kernel için) sağlar.
pub unsafe fn init_virtual_memory(allocator: &mut impl FrameAllocator<Size4KiB>) -> OffsetPageTable<'static> {
    // 1. Yeni PML4 tablosu için frame ayır
    let pml4_frame = allocator.allocate_frame().expect("PML4 için frame ayrılamadı!");
    let pml4_addr = pml4_frame.start_address();
    
    // 2. Identity map olduğu için doğrudan erişebiliriz
    let pml4_ptr = pml4_addr.as_u64() as *mut PageTable;
    let pml4_table = &mut *pml4_ptr;
    
    // Tabloyu sıfırla
    pml4_table.zero();

    // 3. Mevcut (eski) PML4 tablosunu kopyala
    // Böylece UEFI ve stack bozulmaz.
    let (old_pml4_frame, _) = Cr3::read();
    let old_pml4_ptr = old_pml4_frame.start_address().as_u64() as *const PageTable;
    let old_pml4_table = &*old_pml4_ptr;
    
    // Alt 256 girdiyi kopyala (User/UEFI alanı)
    for i in 0..256 {
        pml4_table[i] = old_pml4_table[i].clone();
    }

    // 4. Offset Page Table oluştur
    // UEFI identity map sağladığı için offset = 0 ile başlıyoruz.
    // İleride gerçek higher-half offset kullanılabilir.
    let mapper = OffsetPageTable::new(pml4_table, VirtAddr::new(0));
    
    // 5. Yeni Page Table'a geç (CR3 yükle)
    Cr3::write(pml4_frame, x86_64::registers::control::Cr3Flags::empty());

    mapper
}
