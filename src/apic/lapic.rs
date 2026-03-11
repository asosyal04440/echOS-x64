//! # Local APIC (LAPIC) Sürücüsü
//!
//! BSP üzerinde Local APIC başlatma ve kayıt erişimi.
//!
//! x2APIC önceliklidir; desteklenmezse xAPIC MMIO yoluna düşer.
//! TSC-deadline mode destekleniyorsa periodic yerine one-shot deadline kullanır.
//! CPUID leaf 0x01 sonuçları CPU_INFO içinde tutulur.
//!
//! ## LAPIC Nedir?
//!
//! Her CPU çekirdeğine gömülü bir kesme denetleyicisidir.
//! Yerel timer, IPI (CPU'lar arası kesme) ve donanım kesmelerinin teslimini yönetir.
//!
//! ```text
//!  ┌─────────────────────────────────────────────┐
//!  │                CPU Çekirdeği                 │
//!  │                                              │
//!  │  ┌──────────────────────────────────────┐   │
//!  │  │             L A P I C                │   │
//!  │  │                                      │   │
//!  │  │  ┌──────────┐  ┌─────────────────┐  │   │
//!  │  │  │  Timer   │  │  LVT Register   │  │   │
//!  │  │  │ (TSC/PIT)│  │  (6 giriş)     │  │   │
//!  │  │  └──────────┘  └─────────────────┘  │   │
//!  │  │  ┌──────────┐  ┌─────────────────┐  │   │
//!  │  │  │   EOI    │  │ Spurious Vector │  │   │
//!  │  │  │ Register │  │   (0xFF)        │  │   │
//!  │  │  └──────────┘  └─────────────────┘  │   │
//!  │  └──────────────────────────────────────┘   │
//!  └─────────────────────────────────────────────┘
//! ```
//!
//! ## xAPIC vs x2APIC
//!
//! ```text
//!  xAPIC  : MMIO tabanlı (bellek üzerinden register erişimi, 0xFEE00000 fiziksel)
//!  x2APIC : MSR tabanlı  (daha hızlı, 64-bit APIC ID, MSR 0x800 + offset/16)
//! ```
//!
//! ## TSC-Deadline Timer
//!
//! ```text
//!  TSC (Time Stamp Counter) sürekli artar.
//!  IA32_TSC_DEADLINE MSR'a bir değer yazılır.
//!  TSC o değere ulaştığında LAPIC timer kesmesi tetiklenir.
//!  Bu yöntem periyodik timer'dan daha hassas one-shot zamanlama sağlar.
//! ```

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use x86_64::registers::model_specific::Msr;

use crate::memory::active_physical_offset;

/// IA32_APIC_BASE MSR adresi — APIC taban adresini ve etkinleştirme bitlerini içerir
const IA32_APIC_BASE_MSR: u32 = 0x1B;
/// x2APIC MSR taban adresi — APIC register'ları bu MSR bloğuna eşlenir
const IA32_X2APIC_MSR_BASE: u32 = 0x800;

/// Görev Öncelik Kaydı — TPR=0 → tüm kesmeler kabul edilir
const APIC_REG_TPR: u32 = 0x080;
/// End-of-Interrupt kaydı — işleyici sonunda 0 yazarak APIC'e bildirim gönderilir
const APIC_REG_EOI: u32 = 0x0B0;
/// Spurious Interrupt Vector kaydı — bit 8: APIC etkinleştirme; bit 7..0: yalın-kesme vektörü
const APIC_REG_SPURIOUS: u32 = 0x0F0;

/// LVT Timer kaydı — hangi vektörün tetikleneceği ve mod belirlenir
const APIC_LVT_TIMER: u32 = 0x320;
/// LVT Termal Sensör kaydı
const APIC_LVT_THERMAL: u32 = 0x330;
/// LVT Performans İzleyici kaydı
const APIC_LVT_PERF: u32 = 0x340;
/// LVT LINT0 kaydı — yerel kesme 0 (genellikle 8259 PIC bağlantısı)
const APIC_LVT_LINT0: u32 = 0x350;
/// LVT LINT1 kaydı — yerel kesme 1 (genellikle NMI)
const APIC_LVT_LINT1: u32 = 0x360;
/// LVT Hata kaydı — APIC dahili hata kesmesi
const APIC_LVT_ERROR: u32 = 0x370;
/// Timer başlangıç sayacı — periyodik modda bu değerden geri sayar
const APIC_TIMER_INIT: u32 = 0x380;
/// Timer mevcut sayacı — anlık sayaç değerini okur
const APIC_TIMER_CURRENT: u32 = 0x390;
/// Timer bölen kaydı — sayacın hızını belirler (1/2/4/.../128)
const APIC_TIMER_DIV: u32 = 0x3E0;

/// IA32_TSC_DEADLINE MSR adresi — TSC-Deadline modunda son teslim tarihini bu MSR'a yazar
const IA32_TSC_DEADLINE_MSR: u32 = 0x6E0;
/// CPUID leaf 1, ECX bit 24 = TSC-Deadline destekleniyorsa bu bit 1 olur
const CPUID_TSC_DEADLINE_BIT: u32 = 1 << 24;
/// LVT Timer modu: TSC-Deadline için bit 18 set edilir, periyodik yerine tek seferlik zamanlama
const LVT_TIMER_TSC_DEADLINE: u32 = 0x40000;

/// TSC-deadline mode aktif mi?
static TSC_DEADLINE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Kalibre edilmiş TSC frekansı (Hz)
static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(0);

/// LAPIC çalışma modu.
///
/// ```text
///  Disabled : APIC devre dışı (eski sistemler veya başlatılmamış durum)
///  XApic    : MMIO tabanlı erişim (xAPIC modu, Intel P4+)
///  X2Apic   : MSR tabanlı erişim (x2APIC modu, Intel Nehalem+, daha hızlı)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ApicMode {
    X2Apic = 2,
    XApic = 1,
    Disabled = 0,
}

/// LAPIC başlatma hataları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApicInitError {
    /// CPU'da APIC donanımı bulunmuyor
    NoApic,
    /// IA32_APIC_BASE MSR'daki fiziksel adres geçersiz (0)
    InvalidBase,
}

static APIC_MODE: AtomicU8 = AtomicU8::new(ApicMode::Disabled as u8);
static XAPIC_MMIO_BASE: AtomicU64 = AtomicU64::new(0);

fn set_mode(mode: ApicMode) {
    APIC_MODE.store(mode as u8, Ordering::SeqCst);
}

/// Mevcut LAPIC çalışma modunu döner.
pub fn mode() -> ApicMode {
    match APIC_MODE.load(Ordering::SeqCst) {
        2 => ApicMode::X2Apic,
        1 => ApicMode::XApic,
        _ => ApicMode::Disabled,
    }
}

/// xAPIC MMIO register ofsetini x2APIC MSR adresine çevirir.
/// x2APIC MSR indeksleri: 0x800 + (MMIO offset / 16)
fn x2apic_msr(reg_offset: u32) -> u32 {
    IA32_X2APIC_MSR_BASE + (reg_offset >> 4)
}

/// LAPIC'i başlatır ve uygun çalışma modunu seçer.
///
/// Mod seçim önceliği: x2APIC > xAPIC > Hata
/// x2APIC için IA32_APIC_BASE MSR'da bit 10 (x2APIC enable) ve bit 11 (global enable) set edilir.
pub fn init() -> Result<ApicMode, ApicInitError> {
    // CPUID leaf 0x01 sonucu CPU_INFO içinde saklanır.
    let (has_x2apic, has_xapic) = {
        let info = crate::cpu::CPU_INFO.lock();
        (info.has_x2apic, info.has_apic)
    };

    if !has_x2apic && !has_xapic {
        set_mode(ApicMode::Disabled);
        return Err(ApicInitError::NoApic);
    }

    let apic_base = unsafe { Msr::new(IA32_APIC_BASE_MSR).read() };
    // IA32_APIC_BASE bits 12-35: fiziksel taban adresi
    let phys_base = apic_base & 0xFFFF_F000;
    if phys_base == 0 {
        set_mode(ApicMode::Disabled);
        return Err(ApicInitError::InvalidBase);
    }

    if has_x2apic {
        let mut new_base = apic_base;
        // Bit 11: APIC global etkinleştirme — IA32_APIC_BASE MSR'da bu bit 1 olmalı
        new_base |= 1 << 11;
        // Bit 10: x2APIC modunu etkinleştir — MSR tabanlı erişime geçiş sağlar
        new_base |= 1 << 10;
        unsafe { Msr::new(IA32_APIC_BASE_MSR).write(new_base) };
        set_mode(ApicMode::X2Apic);
    } else {
        // HHDM tüm fiziksel adresleri sabit bir offset ile sanala taşır.
        // Bu sayede APIC MMIO bölgesi de PHYSICAL_MEMORY_OFFSET ile erişilebilir.
        let mapped = crate::memory::map_mmio(phys_base, 0x1000);
        let virt_base = if mapped.is_null() {
            active_physical_offset() + phys_base
        } else {
            mapped as u64
        };
        XAPIC_MMIO_BASE.store(virt_base, Ordering::SeqCst);

        let mut new_base = apic_base;
        // Bit 11: APIC global etkinleştirme — xAPIC modu için de gerekli
        new_base |= 1 << 11;
        // Bit 10: x2APIC'yi devre dışı bırak — MMIO tabanlı xAPIC modunda kal
        new_base &= !(1 << 10);
        unsafe { Msr::new(IA32_APIC_BASE_MSR).write(new_base) };
        set_mode(ApicMode::XApic);
    }

    common_init();
    Ok(mode())
}

/// Mod bağımsız ortak LAPIC ayarları.
///
/// Yapılan işlemler:
/// 1. Spurious Interrupt Vector ayarlanır (APIC etkinleştirilir)
/// 2. TPR sıfırlanır (tüm önceliklerdeki kesmeler kabul edilir)
/// 3. LVT kayıtlarındaki bekleyen durumlar okunarak temizlenir
/// 4. Timer başlatılır
fn common_init() {
    // Spurious Interrupt (yalın kesme) vektörü: 0xFF + etkinleştirme biti (bit 8)
    // Yalın kesme: donanım bir kesme gönderir ama CPU almadan önce geri çekilir;
    // bu durumda CPU 0xFF vektörüne dal ve sadece EOI gönder.
    write_reg(APIC_REG_SPURIOUS, 0xFF | (1 << 8));
    // TPR: 0 (tüm interrupt'ları kabul et)
    write_reg(APIC_REG_TPR, 0);
    // LVT'leri okuyarak olası bekleyen durumları temizle
    let _ = read_reg(APIC_LVT_TIMER);
    let _ = read_reg(APIC_LVT_THERMAL);
    let _ = read_reg(APIC_LVT_PERF);
    let _ = read_reg(APIC_LVT_LINT0);
    let _ = read_reg(APIC_LVT_LINT1);
    let _ = read_reg(APIC_LVT_ERROR);
    init_timer();
}

/// LAPIC timer'ını başlatır.
/// TSC-Deadline destekliyorsa onu tercih eder; yoksa periyodik moda geçer.
///
/// ```text
///  TSC-Deadline Modu:
///    TSC ─────────────────────────────────────────► zaman
///               ▲                 ▲
///               │                 │
///     arm (MSR'a deadline yaz)  TSC == deadline → kesme!
///
///  Periyodik Mod:
///    APIC_TIMER_INIT → geri say → 0 → kesme → tekrar INIT'ten başla
/// ```
fn init_timer() {
    // TSC-deadline desteğini kontrol et
    // Simics'te TSC-Deadline MSR yazımları AP'de #GP oluşturabilir — periyodik moda düş
    #[cfg(feature = "simics")]
    let use_tsc_deadline = false;
    #[cfg(not(feature = "simics"))]
    let use_tsc_deadline = has_tsc_deadline();

    if use_tsc_deadline {
        // TSC-Deadline modu: LVT Timer = vektör 32 + TSC-Deadline mod biti
        write_reg(APIC_LVT_TIMER, 32 | LVT_TIMER_TSC_DEADLINE);
        TSC_DEADLINE_ACTIVE.store(true, Ordering::SeqCst);
        calibrate_tsc();
        // İlk deadline'ı arm et (10ms sonra)
        let freq = tsc_frequency();
        if freq > 0 {
            let deadline_ticks = freq / 100; // 10ms
            deadline_arm(deadline_ticks);
        }
        crate::serial_println!(
            "[LAPIC] TSC-Deadline timer active (TSC freq: {} MHz)",
            tsc_frequency() / 1_000_000
        );
    } else {
        // Geri düş: Periyodik mod (TSC-Deadline yoksa)
        // Önce TSC ile LAPIC timer frekansını kalibre et
        calibrate_tsc();
        write_reg(APIC_TIMER_DIV, 0xB); // Bölen = 1

        // LAPIC frekansını ölç: 10ms boyunca LAPIC timer'ın kaç tick yediğine bak
        // Kısa ölçüm: max değerle başlat, TSC ile 10ms bekle
        write_reg(APIC_LVT_TIMER, 32 | (1 << 16)); // Masked
        write_reg(APIC_TIMER_INIT, 0xFFFF_FFFF);
        let tsc_start = unsafe { core::arch::x86_64::_rdtsc() };
        let tsc_freq = tsc_frequency();
        let tsc_10ms = tsc_freq / 100; // 10ms in TSC ticks
        while (unsafe { core::arch::x86_64::_rdtsc() } - tsc_start) < tsc_10ms {
            core::hint::spin_loop();
        }
        let current = read_reg(APIC_TIMER_CURRENT);
        let lapic_ticks_10ms = 0xFFFF_FFFFu32.wrapping_sub(current);

        // Timer'ı durdur, sonra kalibre edilmiş değerle kur
        write_reg(APIC_TIMER_INIT, 0);

        let init_count = if lapic_ticks_10ms > 100 {
            lapic_ticks_10ms
        } else {
            10_000_000u32 // Kalibrasyon başarısız, varsayılan
        };

        write_reg(APIC_TIMER_INIT, init_count);
        write_reg(APIC_LVT_TIMER, 32 | 0x20000); // Periodic, vector 32
        crate::serial_println!(
            "[LAPIC] Periodic timer active (calibrated: {} ticks/10ms)",
            init_count
        );
    }
}

/// CPUID ile TSC-deadline desteğini kontrol et.
/// Bu bilgi başlatma sırasında CPU_INFO'ya kaydedilir.
fn has_tsc_deadline() -> bool {
    let info = crate::cpu::CPU_INFO.lock();
    info.has_tsc_deadline
}

/// TSC (Time Stamp Counter) frekansını kalibre eder.
///
/// Yöntem 1 (tercihli): CPUID leaf 0x15 — Intel Skylake ve sonrası işlemcilerde
///   doğrudan TSC/referans frekansı oranı bildirilen bir CPUID yaprağı.
///   Formül: freq = ECX * EBX / EAX
///
/// Yöntem 2 (geri düş): PIT (Programmable Interval Timer) ile ~10ms ölçüm.
///   PIT 1.193182 MHz'de çalışır, 11932 tick ≈ 10ms.
///   Bu sürede okunan TSC delta'sı × 100 = yaklaşık TSC Hz.
fn calibrate_tsc() {
    // Yöntem 1: CPUID leaf 0x15 (Intel Skylake+)
    let cpuid_result = unsafe { core::arch::x86_64::__cpuid(0x15) };
    if cpuid_result.ebx != 0 && cpuid_result.ecx != 0 {
        let freq = (cpuid_result.ecx as u64) * (cpuid_result.ebx as u64)
            / (cpuid_result.eax as u64).max(1);
        TSC_FREQ_HZ.store(freq, Ordering::SeqCst);
        return;
    }

    // Yöntem 2: LAPIC timer ile kaba kalibrasyon
    // Divider = 16, 10ms PIT ile ölç
    write_reg(APIC_TIMER_DIV, 0x03); // Böl 16 (divider = 16)
    let tsc_start = unsafe { core::arch::x86_64::_rdtsc() };
    write_reg(APIC_TIMER_INIT, 0xFFFF_FFFF);

    // ~10ms bekle (PIT Channel 2: 1.193182 MHz)
    // 10ms = 11932 PIT ticks
    unsafe {
        let mut pit_cmd = x86_64::instructions::port::Port::<u8>::new(0x43);
        let mut pit_ch2 = x86_64::instructions::port::Port::<u8>::new(0x42);
        pit_cmd.write(0xB0); // Kanal 2, lobyte/hibyte, tek-seferlik mod
        pit_ch2.write((11932 & 0xFF) as u8);
        pit_ch2.write((11932 >> 8) as u8);

        // PIT tamamlanmasını bekle (port 0x61 bit 5)
        let mut gate = x86_64::instructions::port::Port::<u8>::new(0x61);
        let val = gate.read();
        gate.write((val & 0xFC) | 0x01); // Höparlör kapısını etkinleştir (speaker gate)
                                         // Basit spin-wait döngüsü
        for _ in 0..10_000_000 {
            core::hint::spin_loop();
        }
    }

    let current = read_reg(APIC_TIMER_CURRENT);
    let tsc_end = unsafe { core::arch::x86_64::_rdtsc() };
    let elapsed_ticks = 0xFFFF_FFFFu32.wrapping_sub(current);
    let tsc_delta = tsc_end.wrapping_sub(tsc_start);

    // TSC frekansını hesapla
    if elapsed_ticks > 0 {
        // elapsed_ticks = LAPIC tick sayısı (bölen 16) yaklaşık 10ms'de
        // TSC frekansı ≈ tsc_delta * 100 (10ms → 1s ölçeği)
        let freq = tsc_delta * 100;
        TSC_FREQ_HZ.store(freq, Ordering::SeqCst);
    } else {
        // CPUID leaf 0x16 ile işlemci frekansını almayı dene
        let max_leaf = unsafe { core::arch::x86_64::__cpuid(0) }.eax;
        let freq = if max_leaf >= 0x16 {
            let cpuid16 = unsafe { core::arch::x86_64::__cpuid_count(0x16, 0) };
            if cpuid16.eax > 0 {
                // EAX = base frequency in MHz
                (cpuid16.eax as u64) * 1_000_000
            } else {
                3_000_000_000u64 // CPUID boş dönerse 3 GHz
            }
        } else {
            3_000_000_000u64 // Leaf 0x16 desteklenmiyorsa 3 GHz
        };
        crate::serial_println!(
            "[TSC] PIT calibration failed, using CPUID/fallback: {} Hz",
            freq
        );
        TSC_FREQ_HZ.store(freq, Ordering::SeqCst);
    }

    // LAPIC timer'ı durdur
    write_reg(APIC_TIMER_INIT, 0);
}

/// TSC-deadline zamanlayıcısını arm eder.
/// `ticks_from_now` TSC tick sonra LAPIC timer kesmesi tetiklenir.
/// IA32_TSC_DEADLINE MSR'a (mevcut_TSC + ticks_from_now) yazılır.
pub fn deadline_arm(ticks_from_now: u64) {
    if !TSC_DEADLINE_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let current_tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let deadline = current_tsc + ticks_from_now;
    unsafe {
        Msr::new(IA32_TSC_DEADLINE_MSR).write(deadline);
    }
}

pub fn mask_timer() {
    write_reg(APIC_LVT_TIMER, read_reg(APIC_LVT_TIMER) | (1 << 16));
    write_reg(APIC_TIMER_INIT, 0);
    if TSC_DEADLINE_ACTIVE.swap(false, Ordering::SeqCst) {
        unsafe {
            Msr::new(IA32_TSC_DEADLINE_MSR).write(0);
        }
    }
}

/// TSC-deadline modunun aktif olup olmadığını döner.
pub fn is_tsc_deadline() -> bool {
    TSC_DEADLINE_ACTIVE.load(Ordering::SeqCst)
}

/// Kalibre edilmiş TSC frekansını Hz cinsinden döner.
pub fn tsc_frequency() -> u64 {
    TSC_FREQ_HZ.load(Ordering::SeqCst)
}

/// LAPIC kaydını okur.
///
/// x2APIC modunda MSR okuma kullanılır (daha hızlı, `rdmsr` talimatı).
/// xAPIC modunda MMIO okuma kullanılır (bellek eşlemeli register).
pub fn read_reg(reg_offset: u32) -> u32 {
    match mode() {
        ApicMode::X2Apic => {
            let msr = x2apic_msr(reg_offset);
            unsafe { Msr::new(msr).read() as u32 }
        }
        ApicMode::XApic => {
            let base = XAPIC_MMIO_BASE.load(Ordering::SeqCst);
            if base == 0 {
                return 0;
            }
            let ptr = (base + reg_offset as u64) as *const u32;
            unsafe { read_volatile(ptr) }
        }
        ApicMode::Disabled => 0,
    }
}

/// LAPIC kaydına yazar.
///
/// x2APIC modunda MSR yazma kullanılır (`wrmsr` talimatı).
/// xAPIC modunda MMIO yazma kullanılır (bellek eşlemeli register).
pub fn write_reg(reg_offset: u32, value: u32) {
    match mode() {
        ApicMode::X2Apic => {
            let msr = x2apic_msr(reg_offset);
            unsafe { Msr::new(msr).write(value as u64) };
        }
        ApicMode::XApic => {
            let base = XAPIC_MMIO_BASE.load(Ordering::SeqCst);
            if base == 0 {
                return;
            }
            let ptr = (base + reg_offset as u64) as *mut u32;
            unsafe { write_volatile(ptr, value) };
        }
        ApicMode::Disabled => {}
    }
}

/// End-of-Interrupt (EOI) bildirimi gönderir.
///
/// Her kesme işleyicisinin sonunda çağrılmalıdır.
/// EOI yazılmazsa LAPIC aynı öncelikteki bir sonraki kesmeyi teslim etmez.
/// APIC_REG_EOI kaydına sıfır yazmak yeterlidir.
pub fn eoi() {
    write_reg(APIC_REG_EOI, 0);
}
