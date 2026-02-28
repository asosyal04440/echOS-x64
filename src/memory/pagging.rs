//! # Erken Önyükleme Sayfalama Kurulumu
//!
//! x86_64 4 seviyeli sayfa tablosu başlatma (önyükleme aşaması).
//!
//! ## x86_64 4 Seviyeli Sayfa Tablosu Yapısı
//!
//! ```
//! Sanal Adres (64-bit):
//!  63      48 47    39 38    30 29    21 20     12 11      0
//!  ┌─────────┬────────┬────────┬────────┬─────────┬────────┐
//!  │ İşaret  │ PML4   │  PDPT  │   PD   │   PT    │ Ofset  │
//!  │ uzatma  │ indeks │ indeks │ indeks │ indeks  │        │
//!  └─────────┴────────┴────────┴────────┴─────────┴────────┘
//!    16 bit    9 bit    9 bit    9 bit    9 bit     12 bit
//!
//! PML4 [512 giriş] → PDPT [512 giriş] → PD [512 giriş] → PT [512 giriş] → Sayfa (4 KB)
//!                                                  └──► 2 MB büyük sayfa (PS bit = 1)
//!                                    └──► 1 GB büyük sayfa (PS bit = 1)
//! ```
//!
//! ## Bu Modülün Yaptıkları
//!
//! `setup_paging()` erken önyükleme için en basit eşlemeyi kurar:
//!
//! ```
//! 1. PD[0] = 0x0 | HUGE_PAGE(bit7) | WRITABLE(bit1) | PRESENT(bit0)
//!    → Fiziksel 0x0 adresini 2 MB büyük sayfa ile eşle
//!
//! 2. PDPT[0] = &PD | WRITABLE | PRESENT
//!    → PDPT ilk girişi PD'ye işaret eder
//!
//! 3. PML4[0] = &PDPT | WRITABLE | PRESENT
//!    → PML4 ilk girişi PDPT'ye işaret eder
//!
//! 4. CR3 = &PML4
//!    → İşlemci yeni sayfa tablosunu etkinleştirir
//! ```
//!
//! ## Önemli: Bu Sadece Bootstrap İçindir
//!
//! Bu kurulum yalnızca ilk MB'lara erişim sağlar.
//! Tam çekirdek sayfa tablosu `mod.rs::init_paging()` tarafından kurulur.
//!
//! ## İlgili Modüller:
//! - `paging.rs`: HHDM tabanlı sayfa tablosu yardımcıları (`translate_addr`, `map_page`)
//! - `mod.rs`: `init_paging()` — tam çekirdek sayfa tablosu kurulumu

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