//! BSP üzerinde Local APIC başlatma ve kayıt erişimi.
//!
//! x2APIC önceliklidir; desteklenmezse xAPIC MMIO yoluna düşer.
//! TSC-deadline mode destekleniyorsa periodic yerine one-shot deadline kullanır.
//! CPUID leaf 0x01 sonuçları CPU_INFO içinde tutulur.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use x86_64::registers::model_specific::Msr;

use crate::memory::active_physical_offset;

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const IA32_X2APIC_MSR_BASE: u32 = 0x800;

const APIC_REG_TPR: u32 = 0x080;
const APIC_REG_EOI: u32 = 0x0B0;
const APIC_REG_SPURIOUS: u32 = 0x0F0;

const APIC_LVT_TIMER: u32 = 0x320;
const APIC_LVT_THERMAL: u32 = 0x330;
const APIC_LVT_PERF: u32 = 0x340;
const APIC_LVT_LINT0: u32 = 0x350;
const APIC_LVT_LINT1: u32 = 0x360;
const APIC_LVT_ERROR: u32 = 0x370;
const APIC_TIMER_INIT: u32 = 0x380;
const APIC_TIMER_CURRENT: u32 = 0x390;
const APIC_TIMER_DIV: u32 = 0x3E0;

/// IA32_TSC_DEADLINE MSR
const IA32_TSC_DEADLINE_MSR: u32 = 0x6E0;
/// CPUID leaf 1, ECX bit 24 = TSC-Deadline
const CPUID_TSC_DEADLINE_BIT: u32 = 1 << 24;
/// LVT Timer mode: TSC-Deadline = bit 18 set
const LVT_TIMER_TSC_DEADLINE: u32 = 0x40000;

/// TSC-deadline mode aktif mi?
static TSC_DEADLINE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Kalibre edilmiş TSC frekansı (Hz)
static TSC_FREQ_HZ: AtomicU64 = AtomicU64::new(0);

/// LAPIC çalışma modu.
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
    NoApic,
    InvalidBase,
}

static APIC_MODE: AtomicU8 = AtomicU8::new(ApicMode::Disabled as u8);
static XAPIC_MMIO_BASE: AtomicU64 = AtomicU64::new(0);

fn set_mode(mode: ApicMode) {
    APIC_MODE.store(mode as u8, Ordering::SeqCst);
}

pub fn mode() -> ApicMode {
    match APIC_MODE.load(Ordering::SeqCst) {
        2 => ApicMode::X2Apic,
        1 => ApicMode::XApic,
        _ => ApicMode::Disabled,
    }
}

fn x2apic_msr(reg_offset: u32) -> u32 {
    // x2APIC MSR indeksleri: 0x800 + (MMIO offset / 16)
    IA32_X2APIC_MSR_BASE + (reg_offset >> 4)
}

/// LAPIC başlatma ve çalışma modu seçimi.
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
        // Bit 11: APIC global enable
        new_base |= 1 << 11;
        // Bit 10: x2APIC enable
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
        // Bit 11: APIC global enable
        new_base |= 1 << 11;
        // Bit 10: x2APIC disable
        new_base &= !(1 << 10);
        unsafe { Msr::new(IA32_APIC_BASE_MSR).write(new_base) };
        set_mode(ApicMode::XApic);
    }

    common_init();
    Ok(mode())
}

/// Mod bağımsız ortak LAPIC ayarları.
fn common_init() {
    // Spurious Interrupt Vector: 0xFF + enable bit
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

fn init_timer() {
    // TSC-deadline desteğini kontrol et
    if has_tsc_deadline() {
        // TSC-Deadline mode: LVT Timer = vector 32 + TSC-Deadline mode bit
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
        // Fallback: Periodic mode
        write_reg(APIC_TIMER_DIV, 0xB);
        write_reg(APIC_TIMER_INIT, 10_000_000);
        write_reg(APIC_LVT_TIMER, 32 | 0x20000);
        crate::serial_println!("[LAPIC] Periodic timer active (fallback)");
    }
}

/// CPUID ile TSC-deadline desteğini kontrol et
fn has_tsc_deadline() -> bool {
    let info = crate::cpu::CPU_INFO.lock();
    info.has_tsc_deadline
}

/// TSC frekansını kalibre et (CPUID leaf 0x15 veya PIT ile)
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
    write_reg(APIC_TIMER_DIV, 0x03); // Divide by 16
    let tsc_start = unsafe { core::arch::x86_64::_rdtsc() };
    write_reg(APIC_TIMER_INIT, 0xFFFF_FFFF);

    // ~10ms bekle (PIT Channel 2: 1.193182 MHz)
    // 10ms = 11932 PIT ticks
    unsafe {
        let mut pit_cmd = x86_64::instructions::port::Port::<u8>::new(0x43);
        let mut pit_ch2 = x86_64::instructions::port::Port::<u8>::new(0x42);
        pit_cmd.write(0xB0); // Channel 2, lobyte/hibyte, one-shot
        pit_ch2.write((11932 & 0xFF) as u8);
        pit_ch2.write((11932 >> 8) as u8);

        // PIT tamamlanmasını bekle (port 0x61 bit 5)
        let mut gate = x86_64::instructions::port::Port::<u8>::new(0x61);
        let val = gate.read();
        gate.write((val & 0xFC) | 0x01); // Enable speaker gate
        // Basit spin-wait
        for _ in 0..10_000_000 {
            core::hint::spin_loop();
        }
    }

    let current = read_reg(APIC_TIMER_CURRENT);
    let tsc_end = unsafe { core::arch::x86_64::_rdtsc() };
    let elapsed_ticks = 0xFFFF_FFFFu32.wrapping_sub(current);
    let tsc_delta = tsc_end.wrapping_sub(tsc_start);

    // TSC frekansı hesapla
    if elapsed_ticks > 0 {
        // elapsed_ticks = LAPIC ticks (div 16) in ~10ms
        // TSC freq ≈ tsc_delta * 100 (10ms → 1s)
        let freq = tsc_delta * 100;
        TSC_FREQ_HZ.store(freq, Ordering::SeqCst);
    } else {
        // Fallback: 3 GHz varsayım
        TSC_FREQ_HZ.store(3_000_000_000, Ordering::SeqCst);
    }

    // LAPIC timer'ı durdur
    write_reg(APIC_TIMER_INIT, 0);
}

/// TSC-deadline timer'ı arm et — `ticks_from_now` TSC tick sonra fire
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

/// TSC-deadline mode aktif mi?
pub fn is_tsc_deadline() -> bool {
    TSC_DEADLINE_ACTIVE.load(Ordering::SeqCst)
}

/// Kalibre edilmiş TSC frekansı (Hz)
pub fn tsc_frequency() -> u64 {
    TSC_FREQ_HZ.load(Ordering::SeqCst)
}

/// LAPIC kaydını okur.
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

/// End-of-interrupt bildirimi gönderir.
pub fn eoi() {
    write_reg(APIC_REG_EOI, 0);
}
