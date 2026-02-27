//! # echOS CPU Modülü
//!
//! CPU yapılandırması: GDT, IDT, SSE/AVX ve SMP desteği.

/// Interrupt handlers
pub mod interrupts;

/// TSC (Time Stamp Counter) for high-resolution timing
pub mod tsc;

/// SMP (Symmetric Multi-Processing) desteği
pub mod smp;

/// CPU State Machine ve Affinity
pub mod smp_state;

pub mod ap;

/// ACPI (Advanced Configuration and Power Interface) desteği
pub mod acpi;

/// AML (ACPI Machine Language) interpreter — Windows seviyesinde ACPI
pub mod acpi_aml;

/// ACPI Device Manager — cihaz keşfi ve güç yönetimi
pub mod acpi_device;

/// ACPI Power Manager (OSPM) — P-state/C-state/Thermal/Battery/PCI IRQ
pub mod acpi_power;

/// Embedded Controller (EC) driver — laptop donanım kontrolü
pub mod acpi_ec;

/// SCI + GPE event handler — runtime donanım olayları
pub mod acpi_event;

use core::arch::asm;
use spin::Mutex;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

/// Global CPU bilgisi
pub static CPU_INFO: Mutex<CpuInfo> = Mutex::new(CpuInfo::new());

/// CPU bilgi yapısı
#[derive(Debug)]
pub struct CpuInfo {
    /// Toplam CPU sayısı
    pub total_cpus: u32,
    /// BSP (Bootstrap Processor) APIC ID'si
    pub bsp_apic_id: u32,
    /// CPU vendor (Intel, AMD, vs.)
    pub vendor: CpuVendor,
    /// CPU özellikleri (CPUID leaf 1)
    pub features: u32,
    /// APIC desteği var mı?
    pub has_apic: bool,
    /// x2APIC desteği var mı?
    pub has_x2apic: bool,
    /// TSC-Deadline timer desteği var mı? (CPUID.01H:ECX bit 24)
    pub has_tsc_deadline: bool,
    /// Topoloji bilgisi
    pub topology: CpuTopology,
}

/// CPU vendor türleri
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CpuVendor {
    Intel,
    AMD,
    Unknown,
}

/// CPU topolojisi
#[derive(Debug, Clone)]
pub struct CpuTopology {
    /// SMT (Hyper-Threading) seviyesi
    pub smt_level: u32,
    /// Core seviyesi
    pub core_level: u32,
    /// Package (socket) seviyesi
    pub package_level: u32,
    /// Toplam logical processor sayısı
    pub logical_count: u32,
    /// Toplam core sayısı
    pub core_count: u32,
    /// Toplam package sayısı
    pub package_count: u32,
}

impl CpuInfo {
    /// Yeni CPU bilgisi oluşturur
    pub const fn new() -> Self {
        Self {
            total_cpus: 1,
            bsp_apic_id: 0,
            vendor: CpuVendor::Unknown,
            features: 0,
            has_apic: false,
            has_x2apic: false,
            has_tsc_deadline: false,
            topology: CpuTopology {
                smt_level: 0,
                core_level: 0,
                package_level: 0,
                logical_count: 1,
                core_count: 1,
                package_count: 1,
            },
        }
    }
}

/// CPU özelliklerini etkinleştirir (SSE, AVX, APIC).
pub fn init() {
    detect_cpu();
    crate::serial_println!("CPU: detect_cpu tamam");
    enable_sse();
    crate::serial_println!("CPU: enable_sse tamam");
    enable_page_protections();
    crate::serial_println!("CPU: enable_page_protections tamam");
    crate::memory::paging::init_pcid();
    enable_apic();
    crate::serial_println!("CPU: enable_apic tamam");
    security_status();

    // ACPI ve SMP başlatma
    let (vendor, has_apic, has_x2apic) = {
        let info = CPU_INFO.lock();
        (info.vendor, info.has_apic, info.has_x2apic)
    };
    crate::serial_println!(
        "CPU: Vendor={:?}, APIC={}, x2APIC={}",
        vendor,
        has_apic,
        has_x2apic
    );

    // AML interpreter — DSDT/SSDT parse et, ACPI namespace oluştur
    acpi_aml::init_aml();
}

pub fn has_avx() -> bool {
    let features = cpuid(1, 0);
    let osxsave = (features.ecx & (1 << 27)) != 0;
    let avx = (features.ecx & (1 << 28)) != 0;
    if !(osxsave && avx) {
        return false;
    }
    let xcr0 = unsafe { xgetbv() };
    (xcr0 & 0x6) == 0x6
}

pub fn has_avx2() -> bool {
    if !has_avx() {
        return false;
    }
    let features = cpuid(7, 0);
    (features.ebx & (1 << 5)) != 0
}

pub fn has_sse2() -> bool {
    let features = cpuid(1, 0);
    (features.edx & (1 << 26)) != 0
}

pub fn has_ssse3() -> bool {
    let features = cpuid(1, 0);
    (features.ecx & (1 << 9)) != 0
}

pub fn has_sse41() -> bool {
    let features = cpuid(1, 0);
    (features.ecx & (1 << 19)) != 0
}

pub fn smep_supported() -> bool {
    let features = cpuid(7, 0);
    (features.ebx & (1 << 7)) != 0
}

pub fn smap_supported() -> bool {
    let features = cpuid(7, 0);
    (features.ebx & (1 << 20)) != 0
}

pub fn smap_enabled() -> bool {
    (Cr4::read().bits() & (1 << 21)) != 0
}

pub unsafe fn stac() {
    asm!("stac", options(nomem, nostack, preserves_flags));
}

pub unsafe fn clac() {
    asm!("clac", options(nomem, nostack, preserves_flags));
}

/// CPU bilgilerini tespit eder (CPUID kullanarak)
fn detect_cpu() {
    let mut info = CPU_INFO.lock();

    // CPU vendor tespiti
    let vendor_id = cpuid(0, 0);
    // Vendor imzasını EBX/EDX/ECX baytlarından oluşturuyoruz.
    let mut vendor_str = [0u8; 12];
    vendor_str[0..4].copy_from_slice(&vendor_id.ebx.to_le_bytes());
    vendor_str[4..8].copy_from_slice(&vendor_id.edx.to_le_bytes());
    vendor_str[8..12].copy_from_slice(&vendor_id.ecx.to_le_bytes());

    match &vendor_str {
        b"GenuineIntel" => info.vendor = CpuVendor::Intel,
        b"AuthenticAMD" => info.vendor = CpuVendor::AMD,
        _ => info.vendor = CpuVendor::Unknown,
    }

    // CPU özellikleri (leaf 1)
    let features = cpuid(1, 0);
    info.features = features.ecx;
    info.has_apic = (features.edx & (1 << 9)) != 0; // APIC bit

    // x2APIC kontrolü (leaf 1, ecx bit 21)
    info.has_x2apic = (features.ecx & (1 << 21)) != 0;

    // TSC-Deadline kontrolü (leaf 1, ecx bit 24)
    info.has_tsc_deadline = (features.ecx & (1 << 24)) != 0;

    // BSP APIC ID'si (leaf 1, ebx bits 24-31)
    info.bsp_apic_id = (features.ebx >> 24) & 0xFF;

    // Topoloji tespiti
    detect_topology(&mut info);
}

/// CPU topolojisini tespit eder
fn detect_topology(info: &mut CpuInfo) {
    // Leaf 0Bh (Extended Topology Enumeration) kontrolü
    let has_topology_leaf = cpuid(0, 0).eax >= 0xB;

    if has_topology_leaf && info.vendor == CpuVendor::Intel {
        detect_intel_topology(info);
    } else if info.vendor == CpuVendor::AMD {
        detect_amd_topology(info);
    } else {
        detect_generic_topology(info);
    }
}

/// Intel topolojisi tespiti
fn detect_intel_topology(info: &mut CpuInfo) {
    let mut level = 0;
    let mut logical_count = 0;

    while level < 3 {
        let result = cpuid(0xB, level as u32);
        let level_type = (result.ecx >> 8) & 0xFF;

        match level_type {
            1 => {
                // SMT level
                info.topology.smt_level = level;
                logical_count = result.ebx & 0xFFFF;
            }
            2 => {
                // Core level
                info.topology.core_level = level;
                info.topology.core_count = result.ebx & 0xFFFF;
            }
            _ => {}
        }

        if level_type == 0 {
            break;
        }
        level += 1;
    }

    info.topology.logical_count = logical_count;
    info.topology.package_count = info.topology.core_count / logical_count;
}

/// AMD topolojisi tespiti
fn detect_amd_topology(info: &mut CpuInfo) {
    // AMD için leaf 0x8000001E
    if cpuid(0, 0).eax >= 0x8000001E {
        let result = cpuid(0x8000001E, 0);
        info.topology.core_count = ((result.ecx >> 8) & 0xFF) + 1;
        info.topology.logical_count = (result.ecx & 0xFF) + 1;
    } else {
        detect_generic_topology(info);
    }
}

/// Generic topoloji tespiti
fn detect_generic_topology(info: &mut CpuInfo) {
    // Basit varsayımlar
    info.topology.logical_count = 1;
    info.topology.core_count = 1;
    info.topology.package_count = 1;
}

/// SSE (Streaming SIMD Extensions) talimatlarını etkinleştirir.
fn enable_sse() {
    unsafe {
        // CR0: EM bitini temizle, MP bitini ayarla
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::from_bits_truncate(0x4)); // EM bit
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR); // MP bit
        Cr0::write(cr0);

        // CR4: OSFXSR ve OSXMMEXCPT bitlerini ayarla
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
    crate::serial_println!("SSE Enabled");
}

fn enable_page_protections() {
    unsafe {
        let mut cr0 = Cr0::read();
        cr0.insert(Cr0Flags::WRITE_PROTECT);
        Cr0::write(cr0);
        crate::serial_println!("[SECURITY] CR0.WP (Write Protect) enabled");

        let mut cr4 = Cr4::read();
        if smep_supported() {
            cr4.insert(Cr4Flags::from_bits_truncate(1 << 20));
            crate::serial_println!("[SECURITY] SMEP enabled (CR4 bit 20)");
        } else {
            crate::serial_println!("[SECURITY] SMEP not supported by CPU");
        }
        if smap_supported() {
            cr4.insert(Cr4Flags::from_bits_truncate(1 << 21));
            crate::serial_println!("[SECURITY] SMAP enabled (CR4 bit 21)");
        } else {
            crate::serial_println!("[SECURITY] SMAP not supported by CPU");
        }
        Cr4::write(cr4);
    }
}

/// Boot sonrası güvenlik durum özetini serial log'a yazar.
/// Tüm kritik güvenlik özelliklerinin aktiflik durumunu tek seferde gösterir.
pub fn security_status() {
    let cr0 = Cr0::read();
    let cr4 = Cr4::read();
    let wp = cr0.contains(Cr0Flags::WRITE_PROTECT);
    let smep = (cr4.bits() & (1 << 20)) != 0;
    let smap = (cr4.bits() & (1 << 21)) != 0;

    // NX (No-Execute) bit — IA32_EFER MSR bit 11
    let nx = unsafe {
        use x86_64::registers::model_specific::Msr;
        let efer = Msr::new(0xC000_0080).read();
        (efer & (1 << 11)) != 0
    };

    let pcid = crate::memory::paging::pcid_active();

    crate::serial_println!("╔══════════════════════════════════════╗");
    crate::serial_println!("║     echOS Security Status            ║");
    crate::serial_println!("╠══════════════════════════════════════╣");
    crate::serial_println!("║  CR0.WP (Write Protect): {}        ║", if wp { "  ON" } else { " OFF" });
    crate::serial_println!("║  SMEP (Kernel exec):     {}        ║", if smep { "  ON" } else { " OFF" });
    crate::serial_println!("║  SMAP (Kernel access):   {}        ║", if smap { "  ON" } else { " OFF" });
    crate::serial_println!("║  NX (No-Execute):        {}        ║", if nx { "  ON" } else { " OFF" });
    crate::serial_println!("║  PCID (TLB per-process): {}        ║", if pcid { "  ON" } else { " OFF" });
    crate::serial_println!("║  STAC/CLAC:              available ║");
    crate::serial_println!("║  DMA panic-on-fail:      enforced  ║");
    crate::serial_println!("║  Guard pages:            enforced  ║");
    crate::serial_println!("║  Zone PMM (DMA/Normal):  enforced  ║");
    crate::serial_println!("╚══════════════════════════════════════╝");
}

/// APIC (Advanced Programmable Interrupt Controller) etkinleştirir
fn enable_apic() {
    let info = CPU_INFO.lock();

    if !info.has_apic {
        crate::serial_println!("APIC not supported");
        return;
    }

    unsafe {
        // APIC global enable (IA32_APIC_BASE MSR)
        use x86_64::registers::model_specific::Msr;

        let mut apic_base_msr = Msr::new(0x1B);
        let mut apic_base = apic_base_msr.read();

        // APIC enable bit (bit 11)
        apic_base |= 1 << 11;

        if info.has_x2apic {
            apic_base |= 1 << 10;
            crate::serial_println!("x2APIC Enabled");
        }

        apic_base_msr.write(apic_base);
    }

    crate::serial_println!("APIC Enabled");
}

/// CPUID sonuç yapısı
#[derive(Debug, Clone, Copy)]
struct CpuidResult {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

/// CPUID wrapper fonksiyonu
fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    // LLVM register kısıtına takılmamak için intrinsic kullanılır.
    let result = unsafe { core::arch::x86_64::__cpuid_count(leaf, subleaf) };
    CpuidResult {
        eax: result.eax,
        ebx: result.ebx,
        ecx: result.ecx,
        edx: result.edx,
    }
}

unsafe fn xgetbv() -> u64 {
    let eax: u32;
    let edx: u32;
    core::arch::asm!(
        "xgetbv",
        in("ecx") 0u32,
        out("eax") eax,
        out("edx") edx,
        options(nostack, preserves_flags)
    );
    ((edx as u64) << 32) | (eax as u64)
}

// ─── x86 Port I/O (PCI konfig alanı ve cihaz sorgulama için) ────────────
//
// x86 mimarisinde iki türlü I/O mekanizması vardır:
//
// 1. **MMIO (Memory-Mapped I/O)**: Cihaz register'ları normal bellek gibi
//    adreslenir. Pointer ile erişilir. (Modern GPU, NVMe vs.)
//
// 2. **Port I/O**: 16-bit port adres uzayı (0x0000–0xFFFF). Özel
//    `IN`/`OUT` talimatları ile erişilir. (PCI konfig, PS/2, PIC vs.)
//    Ring-3 bu talimatı doğrudan çalıştiramaz — sadece Ring-0 (kernel).
//
// Bu fonksiyonlar `unsafe` çünkü yanlış port'a yazılmak sistemi çökebilir.
// PCI konfig port'ları (0xCF8/0xCFC) standart ve güvenlidir.

/// x86 I/O portuna 32-bit değer yazar.
///
/// ## Inline Assembly Açıklaması:
///   `out dx, eax`  → x86 "OUT" opcode'u
///   `in("dx")  port`  → port adresi DX register'ına yüklenir
///   `in("eax") val`   → yazılacak değer EAX'e yüklenir
///   `options(nostack, preserves_flags)` → stack değiştirme, flag bozma yok
///
/// # Safety
/// Geçerli bir I/O portu ve güvenli bir yazım olduğundan emin olunmalıdır.
#[inline]
pub unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx")  port,
        in("eax") val,
        options(nostack, preserves_flags)
    );
}

/// Write a 8-bit value to an x86 I/O port.
#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nostack, preserves_flags)
    );
}

/// x86 I/O portundan 32-bit değer okur.
///
/// PCI konfig veri portu (0xCFC) gibi port'lardan cihaz cevabını almak için kullanılır.
/// `outl(0xCF8, adres)` ile hedef seçildikten sonra `inl(0xCFC)` ile veri gelir.
///
/// # Safety
/// Port okunabilir ve yan etkisi kabul edilebilir olmalıdır.
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    core::arch::asm!(
        "in eax, dx",
        out("eax") val,
        in("dx")   port,
        options(nostack, preserves_flags)
    );
    val
}

/// Read a 8-bit value from an x86 I/O port.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") val,
        in("dx")  port,
        options(nostack, preserves_flags)
    );
    val
}
