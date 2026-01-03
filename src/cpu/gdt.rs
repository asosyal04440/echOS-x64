//! # echOS Global Descriptor Table (GDT)
//! 
//! Segmentasyon tablosu. Modern x86_64'te segmentasyon neredeyse kullanılmasa da,
//! Kernel/User geçişleri ve TSS (Task State Segment) yüklemek için GDT zorunludur.
//! Özellikle Double Fault handler için stack switch (IST) mekanizması TSS gerektirir.

use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::instructions::segmentation::Segment;
use x86_64::instructions::segmentation::{CS, DS, ES, SS, FS, GS};
use spin::Lazy;

/// Double Fault Interrupt Stack Table (IST) indeksi
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// GDT Selektörlerini tutan yapı
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
}

/// Global GDT nesnesi (Lazy initialization)
pub static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    // Kernel Code Segment
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    // Kernel Data Segment
    let data_selector = gdt.append(Descriptor::kernel_data_segment());
    
    // TODO: TSS (Task State Segment) eklemesi yapılmalı (Double Fault stack switch için)
    
    (gdt, Selectors { code_selector, data_selector })
});

/// GDT'yi yükler ve segment registerlarını günceller.
pub fn init() {
    GDT.0.load();
    unsafe {
        // Kod Segmenti (CS) güncelle
        CS::set_reg(GDT.1.code_selector);
        // Veri Segmentleri güncelle
        DS::set_reg(GDT.1.data_selector);
        ES::set_reg(GDT.1.data_selector);
        SS::set_reg(GDT.1.data_selector);
         FS::set_reg(GDT.1.data_selector);
        GS::set_reg(GDT.1.data_selector);
    }
}
