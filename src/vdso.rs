//! # Linux Seviyesinde vDSO (Virtual Dynamically Shared Object - Sanal Dinamik Paylaşılan Nesne)
//!
//! vDSO, sistem çağrısı (syscall) gecikmesini (overhead) sıfıra indirmek için çekirdek tarafından
//! sağlanan ve kullanıcı alanına salt okunur (read-only) olarak eşlenen (mapped) bellek sayfasıdır.
//!
//! ## Neden vDSO?
//! `gettimeofday`, `clock_gettime` gibi sık çağrılan sistem çağrıları için
//! her seferinde çekirdek moduna geçiş (SYSCALL/SYSRET) çok maliyetlidir.
//! vDSO, güncel zaman verisini kullanıcı alanında erişilebilir kılarak
//! bu sistem çağrılarını kernel'e geçmeden tamamlar.
//!
//! ## Seqlock Mekanizması
//! ```ascii
//! Çekirdek (yazıcı):
//!   seq++  [tek]  --> veri yaz --> seq++ [çift]
//!
//! Kullanıcı (okuyucu):
//!   seq1 = seq_count
//!   veri oku
//!   seq2 = seq_count
//!   seq1 != seq2 ise → güncelleme oldu, tekrar oku
//!   seq1 tek ise    → yazım devam ediyor, tekrar oku
//! ```

use crate::memory::PAGE_SIZE;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// Kullanıcı alanında vDSO'nun eşleneceği sabit sanal adres.
///
/// Adres kullanıcı alanının üst sınırına yakın seçilir (Linux benzeri).
/// Örnekten: `0x0000_7FFF_FFFF_E000`
pub const VDSO_USER_BASE: u64 = 0x0000_7FFF_FFFF_E000;

/// Kullanıcı alanına eşlenecek vDSO veri yapısı.
///
/// `repr(C)` zorunludur: kullanıcı alanı kodu (C veya Rust) sabit ofsetlerle alanlara erişir.
/// Tüm alanlar atomik çünkü okuyucu-yazıcı eşzamanlılığı kilitsiz yönetilir.
#[repr(C)]
#[derive(Debug)]
pub struct VdsoData {
    /// Çekirdeğin güncellediği gerçek zamanlı saat saniyesi (RTC seconds).
    pub rtc_sec: core::sync::atomic::AtomicU64,
    /// Çekirdeğin güncellediği nanosaniye bölümü (0..999_999_999).
    pub rtc_nsec: core::sync::atomic::AtomicU64,
    /// TSC → nanosaniye dönüşümü için kaydırma (shift) değeri.
    pub tsc_shift: core::sync::atomic::AtomicU32,
    /// TSC → nanosaniye dönüşümü için çarpan (multiplier) değeri.
    pub tsc_mult: core::sync::atomic::AtomicU32,
    /// Açılış anından itibaren TSC ofseti; zaman hesaplamasında kullanılır.
    pub tsc_offset: core::sync::atomic::AtomicU64,
    /// Seqlock sayacı; okuyucuların güncelleme çakışmasını tespit etmesini sağlar.
    /// Tek değer: yazım devam ediyor. Çift değer: veri tutarlı.
    pub seq_count: core::sync::atomic::AtomicU32,
    /// `getcpu` sistem çağrısı için mevcut CPU kimlik numarası.
    pub cpu: core::sync::atomic::AtomicU32,
    /// `getcpu` sistem çağrısı için NUMA düğüm kimlik numarası.
    pub node: core::sync::atomic::AtomicU32,
}

/// vDSO için tahsis edilmiş fiziksel çerçeve (physical frame).
/// `unsafe` çünkü global değişken; yalnızca `init()` tarafından yazılır.
static mut VDSO_PHYS_FRAME: Option<PhysFrame> = None;

/// vDSO belleğine çekirdek tarafı erişim için sanal adres.
/// `unsafe` çünkü global değişken; yalnızca `init()` tarafından yazılır.
static mut VDSO_KERNEL_VIRT: Option<VirtAddr> = None;

/// Çekirdek başlatmasında çağrılan `vdso::init()`.
///
/// Adımlar:
/// 1. Bir fiziksel sayfa çerçevesi tahsis et.
/// 2. Fiziksel adresi çekirdek sanal adres alanına eşle.
/// 3. Sayfayı sıfırla (tüm alanlar sıfır ile başlatılır).
/// 4. İlk zaman değerlerini yaz.
pub fn init() {
    // 1 sayfalık physical frame allocate et
    let mut allocator = unsafe { crate::memory::global_memory_manager_mut().unwrap() };
    let frame = allocator
        .allocate_frame()
        .expect("vDSO frame allocate edilemedi!");
    unsafe {
        VDSO_PHYS_FRAME = Some(frame);
    }

    // Kernel sanal adresini hesapla (phys_offset ekleyerek)
    let phys_offset = crate::memory::active_physical_offset();
    let virt_addr = VirtAddr::new(phys_offset + frame.start_address().as_u64());

    unsafe {
        VDSO_KERNEL_VIRT = Some(virt_addr);
        // Belleği sıfırla: tüm `VdsoData` alanları sıfır değerle başlasın
        core::ptr::write_bytes(virt_addr.as_mut_ptr::<u8>(), 0, PAGE_SIZE as usize);
    }

    // İlk değerleri gir
    update_time(0, 0);

    crate::serial_println!(
        "[vDSO] Initialized at mapped phys: {:#x}",
        frame.start_address().as_u64()
    );
}

/// Zamanlayıcı tick'i geldiğinde çekirdek tarafından çağrılır; zaman verilerini günceller.
///
/// Seqlock protokolü uygulanır:
/// - `seq_count` önce tek sayıya (+1) çekilir → yazım başladı.
/// - Veri yazılır.
/// - `seq_count` çift sayıya (+2) çekilir → yazım bitti.
/// Kullanıcı tarafı seq sayacı tek ise veya başlangıç/bitiş değerleri farklıysa tekrar okur.
pub fn update_time(sec: u64, nsec: u64) {
    if let Some(virt_addr) = unsafe { VDSO_KERNEL_VIRT } {
        let vdso = unsafe { &*(virt_addr.as_ptr::<VdsoData>()) };

        // Seqlock yazma başlangıcı: sayacı tek yap (okuyuculara "yazım devam" sinyali)
        let seq = vdso.seq_count.load(Ordering::Relaxed);
        vdso.seq_count.store(seq + 1, Ordering::Release);

        // Zaman verilerini güncelle
        vdso.rtc_sec.store(sec, Ordering::Relaxed);
        vdso.rtc_nsec.store(nsec, Ordering::Relaxed);

        // Seqlock yazma bitişi: sayacı çift yap (okuyuculara "veri tutarlı" sinyali)
        vdso.seq_count.store(seq + 2, Ordering::Release);
    }
}

/// vDSO sayfasını kullanıcı sürecine Salt Okunur (Read-Only) olarak eşler.
///
/// Sayfa tablosu bayrakları: `PRESENT | USER_ACCESSIBLE` (WRITABLE YOK).
/// Kullanıcı kodu yalnızca okuyabilir; yazma girişimi sayfa hatası (page fault) üretir.
pub fn map_to_user(mapper: &mut impl Mapper<Size4KiB>) -> Result<(), ()> {
    let frame = unsafe { VDSO_PHYS_FRAME.ok_or(())? };
    let page = Page::containing_address(VirtAddr::new(VDSO_USER_BASE));

    // Yalnızca okuma (User Accessible + Present, WRITABLE YOKTUR!)
    let flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

    let mut allocator = unsafe { crate::memory::global_memory_manager_mut().ok_or(())? };

    unsafe {
        mapper
            .map_to(page, frame, flags, allocator)
            .map_err(|_| ())?
            .flush();
    }

    Ok(())
}
