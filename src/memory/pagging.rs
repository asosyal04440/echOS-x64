//! # Sayfalama Kurulumu
//!
//! x86_64 mimarisinde sayfa tablosu başlatma rutinleri.

use core::arch::asm;

pub unsafe fn setup_paging() {
    // 1GB page kurulumu
    static mut PML4: [u64; 512] = [0; 512];
    static mut PDPT: [u64; 512] = [0; 512];
    static mut PD: [u64; 512] = [0; 512];

    unsafe {
        // PD -> 1GB page
        PD[0] = (0x0 & 0xFFFF_FFC0_0000) | (1 << 7) | (1 << 1) | (1 << 0); //temel değer 0 (1<<7) 7.biti 1 yap (1<<2) 2.biti 1 yap 1 de 0.biti 1 yap demek

        // PDPT -> PD
        PDPT[0] = (&raw const PD as *const _ as u64) | (1 << 2) | 1;

        // PML4 -> PDPT
        PML4[0] = (&raw const PDPT as *const _ as u64) | (1 << 2) | 1;

        // CR3'e PML4 tablosunun adresini yaz
        asm!("mov cr3, {}", in(reg) (&raw const PML4 as *const _ as u64));
    }
}