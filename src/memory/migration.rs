use alloc::sync::Arc;
use alloc::vec::Vec;
use x86_64::structures::paging::{PageTable, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use super::frame_ownership::{self, SharedAtomicFrame};
use super::rmap;
use super::{active_physical_offset, deallocate_contiguous_frames, get_pt_lock, PAGE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationError {
    NoRmapEntries,
    PteChanged,
    PteIsHuge,
}

fn remote_read_pte(pml4_phys: u64, vaddr: u64) -> Option<(PhysAddr, PageTableFlags)> {
    let hhdm = active_physical_offset();
    let pml4_v = VirtAddr::new(hhdm + pml4_phys);
    let pml4 = unsafe { &*pml4_v.as_ptr::<PageTable>() };
    let pml4e = &pml4[(vaddr >> 39) as usize & 0x1FF];
    if pml4e.is_unused() {
        return None;
    }
    let pdpt_v = VirtAddr::new(hhdm + pml4e.addr().as_u64());
    let pdpt = unsafe { &*pdpt_v.as_ptr::<PageTable>() };
    let pdpte = &pdpt[(vaddr >> 30) as usize & 0x1FF];
    if pdpte.is_unused() || pdpte.flags().contains(PageTableFlags::HUGE_PAGE) {
        return None;
    }
    let pd_v = VirtAddr::new(hhdm + pdpte.addr().as_u64());
    let pd = unsafe { &*pd_v.as_ptr::<PageTable>() };
    let pde = &pd[(vaddr >> 21) as usize & 0x1FF];
    if pde.is_unused() || pde.flags().contains(PageTableFlags::HUGE_PAGE) {
        return None;
    }
    let pt_v = VirtAddr::new(hhdm + pde.addr().as_u64());
    let pt = unsafe { &*pt_v.as_ptr::<PageTable>() };
    let pte = &pt[(vaddr >> 12) as usize & 0x1FF];
    if pte.is_unused() {
        return None;
    }
    Some((pte.addr(), pte.flags()))
}

fn remote_write_pte(pml4_phys: u64, vaddr: u64, new_phys: PhysAddr, flags: PageTableFlags) {
    let hhdm = active_physical_offset();
    let pml4_v = VirtAddr::new(hhdm + pml4_phys);
    let pml4 = unsafe { &mut *pml4_v.as_mut_ptr::<PageTable>() };
    let pdpt_v = VirtAddr::new(hhdm + pml4[(vaddr >> 39) as usize & 0x1FF].addr().as_u64());
    let pdpt = unsafe { &mut *pdpt_v.as_mut_ptr::<PageTable>() };
    let pd_v = VirtAddr::new(
        hhdm + pdpt[(vaddr >> 30) as usize & 0x1FF]
            .addr()
            .as_u64(),
    );
    let pd = unsafe { &mut *pd_v.as_mut_ptr::<PageTable>() };
    let pt_v = VirtAddr::new(
        hhdm + pd[(vaddr >> 21) as usize & 0x1FF]
            .addr()
            .as_u64(),
    );
    let pt = unsafe { &mut *pt_v.as_mut_ptr::<PageTable>() };
    let pte = &mut pt[(vaddr >> 12) as usize & 0x1FF];
    pte.set_addr(new_phys, flags);
}

pub fn migrate_page(src_phys: u64, dst_phys: u64) -> Result<(), MigrationError> {
    let entries = rmap::rmap_lookup(src_phys);
    if entries.is_empty() {
        return Err(MigrationError::NoRmapEntries);
    }
    let n = entries.len();

    for entry in &entries {
        let (pte_phys, _flags) =
            remote_read_pte(entry.pml4, entry.virt).ok_or(MigrationError::PteChanged)?;
        if pte_phys.as_u64() & !(0xFFF) != src_phys {
            return Err(MigrationError::PteChanged);
        }
    }

    let hhdm = active_physical_offset();
    unsafe {
        core::ptr::copy_nonoverlapping::<u8>(
            (hhdm + src_phys) as *const u8,
            (hhdm + dst_phys) as *mut u8,
            PAGE_SIZE,
        );
    }

    let mut space_ids: Vec<u64> = entries.iter().map(|e| e.space_id).collect();
    space_ids.sort();
    space_ids.dedup();
    let locked_arcs: Vec<Arc<spin::Mutex<()>>> =
        space_ids.iter().map(|sid| get_pt_lock(*sid)).collect();
    let mut locked_guards: Vec<spin::MutexGuard<()>> =
        locked_arcs.iter().map(|a| a.lock()).collect();

    for entry in &entries {
        if let Some((_, flags)) = remote_read_pte(entry.pml4, entry.virt) {
            remote_write_pte(entry.pml4, entry.virt, PhysAddr::new(dst_phys), flags);
        }
    }

    for _ in 0..n {
        SharedAtomicFrame::<[u8; 4096]>::incref(dst_phys);
    }
    for _ in 0..n {
        let _ = frame_ownership::dec_frame_ref(src_phys);
    }

    rmap::rmap_replace_page(src_phys, dst_phys);

    for entry in &entries {
        super::lru_update_phys(entry.space_id, entry.virt / (PAGE_SIZE as u64), dst_phys);
    }

    crate::cpu::smp::tlb_defer_shootdown();

    Ok(())
}

pub fn migrate_page_alloc(src_phys: u64) -> Option<u64> {
    let dst_frame = super::allocate_contiguous_frames(1)?;
    let dst_phys = dst_frame.start_address().as_u64();

    let hhdm = active_physical_offset();
    unsafe {
        core::ptr::write_bytes((hhdm + dst_phys) as *mut u8, 0, PAGE_SIZE);
    }

    match migrate_page(src_phys, dst_phys) {
        Ok(()) => Some(dst_phys),
        Err(e) => {
            crate::serial_println!(
                "[MIGRATION] migrate_page_alloc({:#x}) failed: {:?}",
                src_phys,
                e
            );
            let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(dst_phys));
            deallocate_contiguous_frames(frame, 1);
            None
        }
    }
}
