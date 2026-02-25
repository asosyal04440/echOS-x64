//! # Linux-Level vDSO (Virtual Dynamically Shared Object)
//!
//! vDSO, sistem çağrısı overhead'ini sıfıra indirmek için kullanılan
//! kernel tarafından sağlanan ve user-space'e read-only donanım maplenen memory sayfasıdır.

use crate::memory::PAGE_SIZE;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

/// Kullanıcı alanında vDSO'nun mapleneceği sabit sanal adres.
/// (Örn: 0x0000_7FFF_FFFF_E000)
pub const VDSO_USER_BASE: u64 = 0x0000_7FFF_FFFF_E000;

/// User-space'e maplenecek veri yapısı.
/// Kesinlikle `repr(C)` olmalı ki C/Rust uygulamaları bu offsetleri bilsin.
#[repr(C)]
#[derive(Debug)]
pub struct VdsoData {
    /// Kernel'in güncellediği RTC saniyesi
    pub rtc_sec: core::sync::atomic::AtomicU64,
    /// Kernel'in güncellediği nanosaniye
    pub rtc_nsec: core::sync::atomic::AtomicU64,
    /// TSC kalibrasyon shift değeri
    pub tsc_shift: core::sync::atomic::AtomicU32,
    /// TSC kalibrasyon multiplier değeri
    pub tsc_mult: core::sync::atomic::AtomicU32,
    /// Boot anından itibaren TSC ofseti
    pub tsc_offset: core::sync::atomic::AtomicU64,
    /// Seqcount (okuma sırasında güncelleme çakışmasını önlemek için)
    pub seq_count: core::sync::atomic::AtomicU32,
    /// Geçerli İşlemcinin (CPU) ID'si (getcpu syscall için)
    pub cpu: core::sync::atomic::AtomicU32,
    pub node: core::sync::atomic::AtomicU32,
}

static mut VDSO_PHYS_FRAME: Option<PhysFrame> = None;
static mut VDSO_KERNEL_VIRT: Option<VirtAddr> = None;

/// Kernel başlarken çağrılacak `vdso::init()`
pub fn init() {
    // 1 sayfalık physical frame allocate et
    let mut allocator = unsafe { crate::memory::global_memory_manager_mut().unwrap() };
    let frame = allocator.allocate_frame().expect("vDSO frame allocate edilemedi!");
    unsafe {
        VDSO_PHYS_FRAME = Some(frame);
    }

    // Kernel sanal adresini hesapla (phys_offset ekleyerek)
    let phys_offset = crate::memory::active_physical_offset();
    let virt_addr = VirtAddr::new(phys_offset + frame.start_address().as_u64());

    unsafe {
        VDSO_KERNEL_VIRT = Some(virt_addr);
        // Belleği sıfırla
        core::ptr::write_bytes(virt_addr.as_mut_ptr::<u8>(), 0, PAGE_SIZE as usize);
    }

    // İlk değerleri gir
    update_time(0, 0);

    crate::serial_println!("[vDSO] Initialized at mapped phys: {:#x}", frame.start_address().as_u64());
}

/// Zamanlayıcı, Kernel'den her tick geldiğinde bu veriyi günceller
pub fn update_time(sec: u64, nsec: u64) {
    if let Some(virt_addr) = unsafe { VDSO_KERNEL_VIRT } {
        let vdso = unsafe { &*(virt_addr.as_ptr::<VdsoData>()) };
        
        // Seqlock write_begin
        let seq = vdso.seq_count.load(Ordering::Relaxed);
        vdso.seq_count.store(seq + 1, Ordering::Release);
        
        vdso.rtc_sec.store(sec, Ordering::Relaxed);
        vdso.rtc_nsec.store(nsec, Ordering::Relaxed);
        
        // Seqlock write_end
        vdso.seq_count.store(seq + 2, Ordering::Release);
    }
}

/// vDSO sayfasını kullanıcı process'ine (Read-Only) mapler
pub fn map_to_user(mapper: &mut impl Mapper<Size4KiB>) -> Result<(), ()> {
    let frame = unsafe { VDSO_PHYS_FRAME.ok_or(())? };
    let page = Page::containing_address(VirtAddr::new(VDSO_USER_BASE));
    
    // Yalnızca okuma (User Accessible + Present, WRITABLE YOKTUR!)
    let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    
    let mut allocator = unsafe { crate::memory::global_memory_manager_mut().ok_or(())? };
    
    unsafe {
        mapper.map_to(page, frame, flags, allocator)
            .map_err(|_| ())?
            .flush();
    }
    
    Ok(())
}
