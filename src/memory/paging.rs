//! HHDM tabanlı sayfalama yardımcıları.
//!
//! Bu dosya, VMM kurulumunda kullanılacak küçük ve güvenli yardımcılar sunar.

use x86_64::registers::control::{Cr0, Cr0Flags, Cr3};
use x86_64::structures::paging::mapper::{MapToError, MapperFlush, Translate, UnmapError};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

use super::active_physical_offset;

pub fn map_page<A>(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut A,
    virt: VirtAddr,
    phys: PhysAddr,
    flags: PageTableFlags,
) -> Result<MapperFlush<Size4KiB>, MapToError<Size4KiB>>
where
    A: FrameAllocator<Size4KiB>,
{
    let page = Page::containing_address(virt);
    let frame = PhysFrame::containing_address(phys);
    unsafe { mapper.map_to(page, frame, flags, frame_allocator) }.map_err(|err| {
        crate::serial_println!(
            "[MEMORY] map_page failed virt={:#x} phys={:#x} err={:?}",
            virt.as_u64(),
            phys.as_u64(),
            err
        );
        err
    })
}

pub fn unmap_page(
    mapper: &mut impl Mapper<Size4KiB>,
    virt: VirtAddr,
) -> Result<PhysFrame<Size4KiB>, UnmapError> {
    let page = Page::containing_address(virt);
    let (frame, flush) = mapper.unmap(page).map_err(|err| {
        crate::serial_println!(
            "[MEMORY] unmap_page failed virt={:#x} err={:?}",
            virt.as_u64(),
            err
        );
        err
    })?;
    flush.flush();
    Ok(frame)
}

pub fn translate_addr(virt: VirtAddr) -> Option<PhysAddr> {
    let (pml4_frame, _) = Cr3::read();
    let mut frame = pml4_frame;

    let p4_index = virt.p4_index();
    let p3_index = virt.p3_index();
    let p2_index = virt.p2_index();
    let p1_index = virt.p1_index();

    let p4_table = unsafe { phys_table(frame) };
    let p4_entry = &p4_table[p4_index];
    if !p4_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    frame = PhysFrame::containing_address(p4_entry.addr());

    let p3_table = unsafe { phys_table(frame) };
    let p3_entry = &p3_table[p3_index];
    if !p3_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        let phys = p3_entry.addr().as_u64() + (virt.as_u64() & 0x3FFF_FFFF);
        return Some(PhysAddr::new(phys));
    }
    frame = PhysFrame::containing_address(p3_entry.addr());

    let p2_table = unsafe { phys_table(frame) };
    let p2_entry = &p2_table[p2_index];
    if !p2_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }
    if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
        let phys = p2_entry.addr().as_u64() + (virt.as_u64() & 0x1F_FFFF);
        return Some(PhysAddr::new(phys));
    }
    frame = PhysFrame::containing_address(p2_entry.addr());

    let p1_table = unsafe { phys_table(frame) };
    let p1_entry = &p1_table[p1_index];
    if !p1_entry.flags().contains(PageTableFlags::PRESENT) {
        return None;
    }

    let phys = p1_entry.addr().as_u64() + (virt.as_u64() & 0xFFF);
    Some(PhysAddr::new(phys))
}

pub fn translate_effective_flags(virt: VirtAddr) -> Option<PageTableFlags> {
    let (pml4_frame, _) = Cr3::read();
    let mut frame = pml4_frame;
    let mut effective_user = true;
    let mut effective_writable = true;
    let mut effective_present = true;
    let mut effective_nx = false;

    let p4_index = virt.p4_index();
    let p3_index = virt.p3_index();
    let p2_index = virt.p2_index();
    let p1_index = virt.p1_index();

    let p4_table = unsafe { phys_table(frame) };
    let p4_entry = &p4_table[p4_index];
    let p4_flags = p4_entry.flags();
    if !p4_flags.contains(PageTableFlags::PRESENT) {
        return None;
    }
    effective_user &= p4_flags.contains(PageTableFlags::USER_ACCESSIBLE);
    effective_writable &= p4_flags.contains(PageTableFlags::WRITABLE);
    effective_present &= p4_flags.contains(PageTableFlags::PRESENT);
    effective_nx |= p4_flags.contains(PageTableFlags::NO_EXECUTE);
    frame = PhysFrame::containing_address(p4_entry.addr());

    let p3_table = unsafe { phys_table(frame) };
    let p3_entry = &p3_table[p3_index];
    let p3_flags = p3_entry.flags();
    if !p3_flags.contains(PageTableFlags::PRESENT) {
        return None;
    }
    effective_user &= p3_flags.contains(PageTableFlags::USER_ACCESSIBLE);
    effective_writable &= p3_flags.contains(PageTableFlags::WRITABLE);
    effective_present &= p3_flags.contains(PageTableFlags::PRESENT);
    effective_nx |= p3_flags.contains(PageTableFlags::NO_EXECUTE);
    if p3_flags.contains(PageTableFlags::HUGE_PAGE) {
        let mut flags = PageTableFlags::empty();
        if effective_present {
            flags.insert(PageTableFlags::PRESENT);
        }
        if effective_user {
            flags.insert(PageTableFlags::USER_ACCESSIBLE);
        }
        if effective_writable {
            flags.insert(PageTableFlags::WRITABLE);
        }
        if effective_nx {
            flags.insert(PageTableFlags::NO_EXECUTE);
        }
        return Some(flags);
    }
    frame = PhysFrame::containing_address(p3_entry.addr());

    let p2_table = unsafe { phys_table(frame) };
    let p2_entry = &p2_table[p2_index];
    let p2_flags = p2_entry.flags();
    if !p2_flags.contains(PageTableFlags::PRESENT) {
        return None;
    }
    effective_user &= p2_flags.contains(PageTableFlags::USER_ACCESSIBLE);
    effective_writable &= p2_flags.contains(PageTableFlags::WRITABLE);
    effective_present &= p2_flags.contains(PageTableFlags::PRESENT);
    effective_nx |= p2_flags.contains(PageTableFlags::NO_EXECUTE);
    if p2_flags.contains(PageTableFlags::HUGE_PAGE) {
        let mut flags = PageTableFlags::empty();
        if effective_present {
            flags.insert(PageTableFlags::PRESENT);
        }
        if effective_user {
            flags.insert(PageTableFlags::USER_ACCESSIBLE);
        }
        if effective_writable {
            flags.insert(PageTableFlags::WRITABLE);
        }
        if effective_nx {
            flags.insert(PageTableFlags::NO_EXECUTE);
        }
        return Some(flags);
    }
    frame = PhysFrame::containing_address(p2_entry.addr());

    let p1_table = unsafe { phys_table(frame) };
    let p1_entry = &p1_table[p1_index];
    let p1_flags = p1_entry.flags();
    if !p1_flags.contains(PageTableFlags::PRESENT) {
        return None;
    }
    effective_user &= p1_flags.contains(PageTableFlags::USER_ACCESSIBLE);
    effective_writable &= p1_flags.contains(PageTableFlags::WRITABLE);
    effective_present &= p1_flags.contains(PageTableFlags::PRESENT);
    effective_nx |= p1_flags.contains(PageTableFlags::NO_EXECUTE);

    let mut flags = PageTableFlags::empty();
    if effective_present {
        flags.insert(PageTableFlags::PRESENT);
    }
    if effective_user {
        flags.insert(PageTableFlags::USER_ACCESSIBLE);
    }
    if effective_writable {
        flags.insert(PageTableFlags::WRITABLE);
    }
    if effective_nx {
        flags.insert(PageTableFlags::NO_EXECUTE);
    }
    Some(flags)
}

pub fn verify_idempotent_mapping(
    mapper: &(impl Mapper<Size4KiB> + Translate),
    page: Page<Size4KiB>,
    expected_frame: PhysFrame<Size4KiB>,
) -> bool {
    let virt = page.start_address();
    match mapper.translate_addr(virt) {
        Some(current) => current == expected_frame.start_address(),
        None => false,
    }
}

pub fn with_wp_disabled<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let mut cr0 = Cr0::read();
    let had_wp = cr0.contains(Cr0Flags::WRITE_PROTECT);
    if had_wp {
        cr0.remove(Cr0Flags::WRITE_PROTECT);
        unsafe { Cr0::write(cr0) };
    }

    let result = f();

    if had_wp {
        let mut cr0_restore = Cr0::read();
        cr0_restore.insert(Cr0Flags::WRITE_PROTECT);
        unsafe { Cr0::write(cr0_restore) };
    }

    result
}

unsafe fn phys_table(frame: PhysFrame) -> &'static PageTable {
    let phys = frame.start_address().as_u64();
    let virt = VirtAddr::new(active_physical_offset() + phys);
    &*virt.as_ptr()
}

// ============================================================================
// PCID (Process Context Identifiers) — TLB flush azaltımı
// Linux: arch/x86/mm/tlb.c, CONFIG_X86_PCID referans
// ============================================================================

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

/// Maksimum PCID değeri (12-bit, 0–4095). PCID 0 kernel için ayrılır.
const PCID_MAX: u16 = 4095;
const PCID_KERNEL: u16 = 0;

/// Bir sonraki tahsis edilecek PCID.
static NEXT_PCID: AtomicU16 = AtomicU16::new(1);

/// PCID desteği aktif mi (CPUID + CR4.PCIDE kontrol sonrası).
static PCID_ENABLED: AtomicBool = AtomicBool::new(false);

/// CPUID ile PCID desteğini kontrol eder ve CR4.PCIDE bitini aktifler.
/// Boot sırasında bir kez çağrılmalıdır.
pub fn init_pcid() {
    // CPUID.(EAX=01h):ECX[17] = PCID desteği
    let cpuid = unsafe { core::arch::x86_64::__cpuid(0x01) };
    let pcid_supported = (cpuid.ecx >> 17) & 1 == 1;

    if !pcid_supported {
        crate::serial_println!("[PCID] CPU does not support PCID — disabled");
        return;
    }

    // CR4.PCIDE (bit 17) aktifle
    unsafe {
        let cr4 = x86_64::registers::control::Cr4::read();
        let new_cr4 = cr4 | x86_64::registers::control::Cr4Flags::PCID;
        x86_64::registers::control::Cr4::write(new_cr4);
    }

    PCID_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[PCID] PCID enabled — TLB flush optimizations active");
}

/// PCID desteği aktif mi?
pub fn pcid_active() -> bool {
    PCID_ENABLED.load(Ordering::Relaxed)
}

/// Yeni bir PCID tahsis et. Taşma durumunda wrap-around yapar (TLB flush gerektirir).
pub fn allocate_pcid() -> u16 {
    let pcid = NEXT_PCID.fetch_add(1, Ordering::Relaxed);
    if pcid > PCID_MAX {
        // Wrap-around: PCID'ler tükendi, 1'den başla (0 = kernel)
        NEXT_PCID.store(2, Ordering::Relaxed);
        1
    } else {
        pcid
    }
}

/// CR3'ü PCID ile yükle. `noflush = true` ise TLB flush yapılmaz.
///
/// CR3 formatı (PCID aktifken):
///   - Bit 63: NOFLUSH — 1 ise mevcut PCID TLB entry'leri korunur
///   - Bit 11:0: PCID değeri
///   - Bit 51:12: PML4 fiziksel adresi
///
/// # Safety
/// Geçersiz CR3 tüm sistemi çökertir.
pub unsafe fn load_cr3_with_pcid(pml4_phys: PhysAddr, pcid: u16, noflush: bool) {
    if !pcid_active() {
        // PCID yoksa düz CR3 yükle
        let frame = PhysFrame::containing_address(pml4_phys);
        x86_64::registers::control::Cr3::write(frame, x86_64::registers::control::Cr3Flags::empty());
        return;
    }

    let mut cr3_val = pml4_phys.as_u64() & !0xFFF; // PML4 adresi (sayfa hizalı)
    cr3_val |= (pcid & 0xFFF) as u64;               // PCID (12-bit)
    if noflush {
        cr3_val |= 1u64 << 63;                       // NOFLUSH bit
    }

    core::arch::asm!("mov cr3, {}", in(reg) cr3_val, options(nostack, preserves_flags));
}

/// Kernel PCID'sini döndürür (her zaman 0).
pub fn kernel_pcid() -> u16 {
    PCID_KERNEL
}
