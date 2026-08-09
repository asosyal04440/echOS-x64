//! # Linux Seviyesinde vDSO (Virtual Dynamically Shared Object)
//!
//! vDSO, sistem çağrısı (syscall) gecikmesini sıfıra indirmek için çekirdek tarafından
//! sağlanan ve kullanıcı alanına salt okunur (read-only) olarak eşlenen bellek sayfasıdır.
//!
//! ## Neden vDSO?
//!
//! `clock_gettime`, `gettimeofday`, `getcpu` gibi sık çağrılan sistem çağrıları için
//! her seferinde çekirdek moduna geçiş (SYSCALL/SYSRET) çok maliyetlidir.
//! vDSO, güncel zaman verisini kullanıcı alanında erişilebilir kılarak
//! bu sistem çağrılarını kernel'e geçmeden tamamlar.
//!
//! ## Kullanıcı Alanı ABI
//!
//! Sayfa `VDSO_USER_BASE` (0x0000_7FFF_FFFF_E000) adresine eşlenir.
//! Kullanıcı kodu `VdsoData` struct'ını bu adreste okuyarak zaman bilgisine erişir:
//!
//! ```c
//! struct ech_vdso_data *vdso = (struct ech_vdso_data *)0x00007fffffffe000;
//!
//! // clock_gettime(CLOCK_MONOTONIC) — seqlock korumalı okuma
//! uint32_t seq1, seq2;
//! uint64_t base_sec, base_ns, epoch_tsc;
//! do {
//!     seq1 = atomic_load(&vdso->seq_count);
//!     base_sec = atomic_load(&vdso->rtc_sec);
//!     base_ns  = atomic_load(&vdso->rtc_nsec);
//!     epoch_tsc = atomic_load(&vdso->tsc_epoch);
//!     seq2 = atomic_load(&vdso->seq_count);
//! } while (seq1 != seq2 || (seq1 & 1));
//!
//! // clock_mode == VDSO_CLOCKMODE_NONE → fallback syscall
//! if (atomic_load(&vdso->clock_mode) != VDSO_CLOCKMODE_TSC) {
//!     return syscall(SYS_clock_gettime, clk, ts);
//! }
//!
//! uint64_t now_tsc = __rdtsc();
//! uint64_t delta_ns;
//! if (now_tsc > epoch_tsc) {
//!     uint64_t ticks = now_tsc - epoch_tsc;
//!     delta_ns = (ticks * (uint64_t)vdso->tsc_mult) >> vdso->tsc_shift;
//! } else {
//!     delta_ns = 0;
//! }
//!
//! uint64_t total_ns = base_ns + delta_ns;
//! ts->tv_sec  = base_sec + total_ns / 1000000000;
//! ts->tv_nsec = total_ns % 1000000000;
//! ```
//!
//! `tsc_mult`, `tsc_shift` boot'ta bir kez kalibre edilir.
//! `tsc_epoch` her tick'te `update_time()` tarafından güncellenir.
//! Kullanıcı alanı `epoch_tsc` ile `rdtsc()` arasındaki farkı alarak
//! tick altı (sub-tick) hassasiyet elde eder.
//!
//! ## Seqlock Mekanizması
//!
//! ```text
//! Çekirdek (yazıcı):
//!   seq++  [tek]  --> rtc_sec/rtc_nsec/tsc_epoch yaz --> seq++ [çift]
//!
//! Kullanıcı (okuyucu):
//!   seq1 = seq_count
//!   veri oku (sec, nsec, epoch)
//!   seq2 = seq_count
//!   seq1 != seq2 ise → güncelleme oldu, tekrar oku
//!   seq1 tek ise    → yazım devam ediyor, tekrar oku
//! ```

use crate::apic::lapic;
use crate::memory::PAGE_SIZE;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

/// vDSO clock mode: TSC tabanlı yüksek çözünürlük (başarılı).
pub const VDSO_CLOCKMODE_TSC: u32 = 0;
/// vDSO clock mode: hiçbiri — kullanıcı alanı fallback syscall yapmalı.
pub const VDSO_CLOCKMODE_NONE: u32 = 1;

/// Kullanıcı alanında vDSO'nun eşleneceği sabit sanal adres.
pub const VDSO_USER_BASE: u64 = 0x0000_7FFF_FFFF_E000;

/// Kullanıcı alanına eşlenecek vDSO veri yapısı.
///
/// `repr(C)` zorunludur: kullanıcı alanı kodu sabit ofsetlerle alanlara erişir.
/// Tüm alanlar atomik çünkü okuyucu-yazıcı eşzamanlılığı kilitsiz yönetilir.
#[repr(C)]
#[derive(Debug)]
pub struct VdsoData {
    /// Gerçek zamanlı saat — saniye (seqlock koruması altında).
    pub rtc_sec: AtomicU64,
    /// Gerçek zamanlı saat — nanosaniye (0..999_999_999, seqlock koruması altında).
    pub rtc_nsec: AtomicU64,

    /// TSC → nanosaniye dönüşümü için kaydırma (shift) değeri.
    /// Boot'ta bir kez kalibre edilir, sonra değişmez.
    pub tsc_shift: AtomicU32,
    /// TSC → nanosaniye dönüşümü için çarpan (multiplier) değeri.
    /// Boot'ta bir kez kalibre edilir, sonra değişmez.
    pub tsc_mult: AtomicU32,

    /// `rtc_sec`/`rtc_nsec` ile eşleşen TSC değeri (epoch).
    /// Kullanıcı: `delta_ns = ((rdtsc() - tsc_epoch) * tsc_mult) >> tsc_shift`
    /// Her tick'te güncellenir (seqlock koruması altında).
    pub tsc_epoch: AtomicU64,

    /// Seqlock sayacı; okuyucuların güncelleme çakışmasını tespit etmesini sağlar.
    /// Tek değer: yazım devam ediyor. Çift değer: veri tutarlı.
    pub seq_count: AtomicU32,

    /// Kullanılan clock mode: VDSO_CLOCKMODE_TSC (0) veya VDSO_CLOCKMODE_NONE (1).
    /// Linux vDSO ABI uyumluluğu. Kullanıcı alanı kodu önce burayı kontrol eder:
    /// `VDSO_CLOCKMODE_NONE` → fallback syscall.
    pub clock_mode: AtomicU32,

    /// `getcpu` için mevcut CPU kimlik numarası.
    pub cpu: AtomicU32,
    /// `getcpu` için NUMA düğüm kimlik numarası.
    pub node: AtomicU32,
}

/// Varsayılan: tüm alanlar 0.
impl Default for VdsoData {
    fn default() -> Self {
        Self {
            rtc_sec: AtomicU64::new(0),
            rtc_nsec: AtomicU64::new(0),
            tsc_shift: AtomicU32::new(0),
            tsc_mult: AtomicU32::new(0),
            tsc_epoch: AtomicU64::new(0),
            seq_count: AtomicU32::new(0),
            clock_mode: AtomicU32::new(0),
            cpu: AtomicU32::new(0),
            node: AtomicU32::new(0),
        }
    }
}

/// vDSO için tahsis edilmiş fiziksel çerçeve.
static mut VDSO_PHYS_FRAME: Option<PhysFrame> = None;

/// vDSO belleğine çekirdek tarafı erişim için sanal adres.
static mut VDSO_KERNEL_VIRT: Option<VirtAddr> = None;

/// Boot'ta TSC'den kalibre edilen çarpan (mult).
static VDSO_TSC_MULT: AtomicU32 = AtomicU32::new(0);
/// Boot'ta TSC'den kalibre edilen kaydırma (shift).
static VDSO_TSC_SHIFT: AtomicU32 = AtomicU32::new(0);

// ── TSC çarpan hesaplama ──────────────────────────────

/// TSC frekansından mult/shift hesaplar.
///
/// `ns = (tsc_delta * mult) >> shift`
///
/// Algoritma: shift=32'den başla, mult'un hem >0 hem de <2^32
/// olmasını garanti et. Bu, Linux `clocksource_cyc2ns()` ile aynı
/// multiply-and-shift yaklaşımıdır.
fn tsc_calibrate(hz: u64) -> (u32, u32) {
    const NS_PER_SEC: u64 = 1_000_000_000;
    let hz = hz.max(1);
    let mut shift: u32 = 32;
    while shift > 0 && (NS_PER_SEC << shift) / hz >= (1u64 << 32) {
        shift -= 1;
    }
    while (NS_PER_SEC << shift) / hz == 0 {
        shift += 1;
    }
    let mult = ((NS_PER_SEC << shift) / hz) as u32;
    (mult, shift)
}

// ── Başlatma ──────────────────────────────────────────

/// vDSO başlatma hatası.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdsoInitError {
    MemoryManagerUnavailable,
    FrameAllocationFailed,
}

/// Çekirdek başlatmasında çağrılan `vdso::init()`.
///
/// Adımlar:
/// 1. Bir fiziksel sayfa çerçevesi tahsis et.
/// 2. Fiziksel adresi çekirdek sanal adres alanına eşle.
/// 3. TSC mult/shift kalibre et.
/// 4. İlk zaman değerlerini yaz.
pub fn init() -> Result<(), VdsoInitError> {
    let mut allocator = unsafe {
        crate::memory::global_memory_manager_mut().ok_or(VdsoInitError::MemoryManagerUnavailable)?
    };
    let frame = allocator
        .allocate_frame()
        .ok_or(VdsoInitError::FrameAllocationFailed)?;

    let phys_addr = frame.start_address().as_u64();
    let phys_offset = crate::memory::active_physical_offset();
    let virt_addr = VirtAddr::new(phys_offset + phys_addr);

    unsafe {
        VDSO_PHYS_FRAME = Some(frame);
        VDSO_KERNEL_VIRT = Some(virt_addr);
        // Belleği sıfırla
        core::ptr::write_bytes(virt_addr.as_mut_ptr::<u8>(), 0, PAGE_SIZE as usize);
    }

    // TSC kalibrasyonu
    let hz = lapic::tsc_frequency();
    let (mult, shift) = tsc_calibrate(hz);
    VDSO_TSC_MULT.store(mult, Ordering::Relaxed);
    VDSO_TSC_SHIFT.store(shift, Ordering::Relaxed);

    let vdso = unsafe { &*(virt_addr.as_ptr::<VdsoData>()) };
    vdso.tsc_mult.store(mult, Ordering::Relaxed);
    vdso.tsc_shift.store(shift, Ordering::Relaxed);
    vdso.clock_mode.store(VDSO_CLOCKMODE_TSC, Ordering::Relaxed);

    // İlk epoch TSC'sini kaydet
    let boot_tsc = unsafe { core::arch::x86_64::_rdtsc() };
    vdso.tsc_epoch.store(boot_tsc, Ordering::Relaxed);

    // rtc_sec/rtc_nsec zaten 0 (sıfırlanmış sayfa)

    crate::serial_println!(
        "[vDSO] Initialized at phys {:#x}, TSC {} Hz → mult={} shift={}",
        phys_addr,
        hz,
        mult,
        shift,
    );
    Ok(())
}

// ── Zaman güncelleme (timer tick'ten) ─────────────────

/// Zamanlayıcı tick'i geldiğinde çekirdek tarafından çağrılır.
///
/// TSC epoch'u ilerletir, rtc_sec/rtc_nsec'i günceller, cpu/node yazar.
/// Seqlock protokolü uygulanır.
///
/// `ns_since_boot` — tick sayacından hesaplanan yaklaşık nanosaniye.
/// TSC ile tick arasındaki fark (varsa) epoch'a yansıtılır.
pub fn update_time(ns_since_boot: u64) {
    if let Some(virt_addr) = unsafe { VDSO_KERNEL_VIRT } {
        let vdso = unsafe { &*(virt_addr.as_ptr::<VdsoData>()) };
        let now_tsc = unsafe { core::arch::x86_64::_rdtsc() };

        // Taban zaman = tick tabanlı ns (referans)
        let base_sec = ns_since_boot / 1_000_000_000;
        let base_ns = ns_since_boot % 1_000_000_000;

        // Seqlock yazma başlangıcı
        let seq = vdso.seq_count.load(Ordering::Relaxed);
        vdso.seq_count.store(seq.wrapping_add(1), Ordering::Release);

        vdso.rtc_sec.store(base_sec, Ordering::Relaxed);
        vdso.rtc_nsec.store(base_ns, Ordering::Relaxed);
        vdso.tsc_epoch.store(now_tsc, Ordering::Relaxed);

        // CPU / node güncelle
        let cpu_id = crate::cpu::smp::get_current_cpu_id();
        vdso.cpu.store(cpu_id as u32, Ordering::Relaxed);
        vdso.node.store(0, Ordering::Relaxed); // echOS henüz NUMA yok

        // Seqlock yazma bitişi
        vdso.seq_count.store(seq.wrapping_add(2), Ordering::Release);
    }
}

// ── Kernel içi zaman okuyucu ──────────────────────────

/// vDSO verilerini kullanarak tick üstü hassas zamanı döndürür.
///
/// `(sec, nsec)` — monotonic boot zamanı.
/// Eğer vDSO henüz başlatılmamışsa ticks*TICK_NS baz alınır.
pub fn get_time_ns() -> (u64, u64) {
    if let Some(virt_addr) = unsafe { VDSO_KERNEL_VIRT } {
        let vdso = unsafe { &*(virt_addr.as_ptr::<VdsoData>()) };
        let mult = VDSO_TSC_MULT.load(Ordering::Relaxed);
        let shift = VDSO_TSC_SHIFT.load(Ordering::Relaxed);
        let now_tsc = unsafe { core::arch::x86_64::_rdtsc() };

        // Seqlock korumalı oku
        let (base_sec, base_ns, epoch_tsc) = loop {
            let seq1 = vdso.seq_count.load(Ordering::Acquire);
            let s = vdso.rtc_sec.load(Ordering::Relaxed);
            let ns = vdso.rtc_nsec.load(Ordering::Relaxed);
            let ep = vdso.tsc_epoch.load(Ordering::Relaxed);
            let seq2 = vdso.seq_count.load(Ordering::Acquire);
            if seq1 == seq2 && (seq1 & 1) == 0 {
                break (s, ns, ep);
            }
            // Seqlock çakışması — hafif spin
            core::hint::spin_loop();
        };

        let tsc_delta = if now_tsc > epoch_tsc {
            now_tsc - epoch_tsc
        } else {
            0
        };

        let tsc_ns = if mult != 0 && shift < 64 {
            ((tsc_delta as u128) * (mult as u128)) >> shift
        } else {
            0u128
        };

        let total_ns = (base_ns as u128).saturating_add(tsc_ns);
        let carry_sec = total_ns / 1_000_000_000;
        let final_ns = total_ns % 1_000_000_000;
        (base_sec + carry_sec as u64, final_ns as u64)
    } else {
        // vDSO yok — ticks*TICK_NS fallback
        let ticks = crate::interrupts::get_ticks();
        let ns = ticks * 10_000_000; // 1 tick = 10ms
        (ns / 1_000_000_000, ns % 1_000_000_000)
    }
}

/// vDSO verilerini kullanarak `ts`'yi (Timespec) doldurur.
pub fn get_time_timespec() -> crate::posix::Timespec {
    let (sec, nsec) = get_time_ns();
    crate::posix::Timespec {
        tv_sec: sec as i64,
        tv_nsec: nsec as i64,
    }
}

// ── Kullanıcı alanına eşleme ──────────────────────────

/// vDSO sayfasını kullanıcı sürecine salt okunur olarak eşler.
pub fn map_to_user(mapper: &mut impl Mapper<Size4KiB>) -> Result<(), ()> {
    let frame = unsafe { VDSO_PHYS_FRAME.ok_or(())? };
    let page = Page::containing_address(VirtAddr::new(VDSO_USER_BASE));
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::NO_CACHE; // WC — seqlock okuyucuları için tutarlılık
    let mut allocator = unsafe { crate::memory::global_memory_manager_mut().ok_or(())? };
    unsafe {
        mapper
            .map_to(page, frame, flags, allocator)
            .map_err(|_| ())?
            .flush();
    }
    Ok(())
}

/// vDSO sayfasını kullanıcı sürecinden kaldırır (process exit).
pub fn unmap_from_user(mapper: &mut impl Mapper<Size4KiB>) -> Result<(), ()> {
    let page = Page::containing_address(VirtAddr::new(VDSO_USER_BASE));
    let (_, flush) = mapper.unmap(page).map_err(|_| ())?;
    flush.flush();
    Ok(())
}

// ── Testler ───────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsc_calibrate_produces_valid_factors() {
        // 2 GHz → mult > 0, shift reasonable
        let (mult, shift) = tsc_calibrate(2_000_000_000);
        assert!(mult > 0, "mult must be > 0 for 2GHz");
        assert!(shift < 64, "shift must be reasonable");

        // 3 GHz
        let (mult3, shift3) = tsc_calibrate(3_000_000_000);
        assert!(mult3 > 0);
        assert!(shift3 < 64);

        // 1 GHz — edge case
        let (mult1, shift1) = tsc_calibrate(1_000_000_000);
        assert!(mult1 > 0);
        assert!(shift1 < 64);

        // Consistency check: delta_ns should be approximately correct
        for hz in [
            1_000_000_000u64,
            2_000_000_000,
            3_000_000_000,
            4_000_000_000,
        ] {
            let (m, s) = tsc_calibrate(hz);
            let delta_tsc = hz; // 1 saniyelik TSC delta
            let ns = ((delta_tsc as u128) * (m as u128)) >> s;
            let error_pct = if ns > 1_000_000_000 {
                ((ns - 1_000_000_000) * 100) / 1_000_000_000
            } else {
                ((1_000_000_000 - ns) * 100) / 1_000_000_000
            };
            assert!(
                error_pct < 1,
                "TSC→ns error too large for {} Hz: {}% (got {} ns, expected 1e9)",
                hz,
                error_pct,
                ns
            );
        }
    }

    #[test]
    fn tsc_calibrate_edge_cases() {
        // Very low freq (hypothetical)
        let (mult, shift) = tsc_calibrate(1_000_000);
        assert!(mult > 0);
        assert!(u64::from(mult) <= u32::MAX as u64);
        assert!(shift < 64);

        // Freq = 0 (should not panic, hz.max(1) guards)
        let (mult0, shift0) = tsc_calibrate(0);
        assert!(mult0 > 0 || shift0 > 0);
    }
}
