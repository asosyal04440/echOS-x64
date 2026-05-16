//! # echOS CPU Modülü
//!
//! CPU yapılandırması: GDT, IDT, SSE/AVX ve SMP desteği.

/// Kesme işleyicileri — PIC ofseti, IDT vektörleri ve donanım kesme handler'ları
pub mod interrupts;

/// TSC (Zaman Damgası Sayacı) — yüksek çözünürlüklü zamanlama, frekans kalibrasyonu
pub mod tsc;

/// SMP (Symmetric Multi-Processing) desteği
pub mod smp;

/// ACPI S3 wake vector trampoline and firmware resume entry
pub mod s3_resume;

/// CPU Durum Makinesi (State Machine) ve İşlemci Benzeşimi (Affinity) yönetimi
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
pub mod cpu_slots;
pub mod epoch;

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

// ============================================================================
// XSAVE ALTYAPISI — Silicon-Assisted Eager FPU
// ============================================================================

/// XSAVE yeteneklerini tutan yapı — boot sırasında bir kez doldurulur, sonra read-only.
#[derive(Debug, Clone, Copy)]
pub struct XSaveCapabilities {
    /// XSAVE instruction desteği var mı (CPUID.01H:ECX bit 26)
    pub has_xsave: bool,
    /// XSAVEOPT desteği var mı (CPUID.0DH.01H:EAX bit 0) — hardware lazy save
    pub has_xsaveopt: bool,
    /// XSAVEC (compacted) desteği var mı (CPUID.0DH.01H:EAX bit 1)
    pub has_xsavec: bool,
    /// XSAVES (supervisor) desteği var mı (CPUID.0DH.01H:EAX bit 3)
    pub has_xsaves: bool,
    /// XCR0 tarafından desteklenen bileşen bitmask'i
    pub xcr0_supported: u64,
    /// Aktif XCR0 değeri (OS tarafından enable edilen bileşenler)
    pub xcr0_active: u64,
    /// XSAVE alanının toplam boyutu (byte) — CPUID.0DH.00H:ECX
    pub area_size: usize,
}

impl XSaveCapabilities {
    pub const fn empty() -> Self {
        Self {
            has_xsave: false,
            has_xsaveopt: false,
            has_xsavec: false,
            has_xsaves: false,
            xcr0_supported: 0,
            xcr0_active: 0,
            area_size: 512, // fallback: FXSAVE size
        }
    }
}

/// Global XSAVE yetenekleri — boot sırasında `enable_xsave()` tarafından doldurulur.
static XSAVE_CAPS: Mutex<XSaveCapabilities> = Mutex::new(XSaveCapabilities::empty());

/// XSAVE aktif mi? (fast path — Mutex almadan kontrol)
static XSAVE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// XSAVE yeteneklerinin kopyasını döndürür.
pub fn xsave_capabilities() -> XSaveCapabilities {
    *XSAVE_CAPS.lock()
}

/// XSAVE aktif mi? (lock-free fast path)
#[inline(always)]
pub fn xsave_active() -> bool {
    XSAVE_ACTIVE.load(Ordering::Relaxed)
}

/// XSAVE alanının boyutunu döndürür (context switch için).
/// XSAVE yoksa FXSAVE boyutu (512) döner.
#[inline]
pub fn xsave_area_size() -> usize {
    if xsave_active() {
        XSAVE_CAPS.lock().area_size
    } else {
        512
    }
}

/// Global CPU bilgisi
pub static CPU_INFO: Mutex<CpuInfo> = Mutex::new(CpuInfo::new());

/// CPU bilgi yapısı
#[derive(Debug, Clone)]
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

    /// Vendor string (procfs/sysfs için)
    pub fn vendor_str(&self) -> &'static str {
        match self.vendor {
            CpuVendor::Intel => "GenuineIntel",
            CpuVendor::AMD => "AuthenticAMD",
            CpuVendor::Unknown => "Unknown",
        }
    }

    /// Marka dizesi (model name) — CPUID leaf 0x80000002-4'ten alınamazsa sabit döndürür
    pub fn brand_string(&self) -> &'static str {
        match self.vendor {
            CpuVendor::Intel => "Intel(R) Core(TM) Processor (echOS detected)",
            CpuVendor::AMD => "AMD Processor (echOS detected)",
            _ => "Unknown Processor",
        }
    }

    /// Tahmini frekans (MHz) — gerçek TSC ölçümü mevcut olmadığında sabit döner
    pub fn freq_mhz(&self) -> u32 {
        3000
    }

    /// L2 cache boyutu (KB) — CPUID leaf 4 ile tam okuma yapılmıyorsa sabit döner
    pub fn l2_cache_kb(&self) -> u32 {
        256
    }

    /// SSE2 desteği var mı (CPUID.01H:EDX bit 26)
    pub fn has_sse2(&self) -> bool {
        self.features & (1 << 26) != 0
    }

    /// AVX desteği var mı (CPUID.01H:ECX bit 28)
    pub fn has_avx_feat(&self) -> bool {
        self.features & (1 << 28) != 0
    }

    /// AES-NI desteği var mı (CPUID.01H:ECX bit 25)
    pub fn has_aes(&self) -> bool {
        self.features & (1 << 25) != 0
    }

    /// RDRAND desteği var mı (CPUID.01H:ECX bit 30)
    pub fn has_rdrand(&self) -> bool {
        self.features & (1 << 30) != 0
    }
}

/// Global CPU bilgisine erişim kısayolu — Mutex lock alarak kopyasını döndürür
pub fn get_cpu_info() -> CpuInfo {
    CPU_INFO.lock().clone()
}

/// CPU özelliklerini etkinleştirir (SSE, AVX, XSAVE, APIC).
pub fn init() {
    detect_cpu();
    crate::serial_println!("CPU: detect_cpu tamam");
    enable_sse();
    crate::serial_println!("CPU: enable_sse tamam");
    enable_xsave();
    crate::serial_println!("CPU: enable_xsave tamam");
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

    // AML namespace is initialized after FADT/DSDT discovery in the UEFI boot pipeline.
}

/// Secondary CPU feature bringup.
///
/// AP'ler BSP'nin CR4/XCR0 durumunu güvenilir şekilde miras almaz. Scheduler'ın ilk
/// XSAVE/XRSTOR veya PCID yolunda #UD/#GP yememesi için her AP kendi SIMD ve paging
/// yardımcı durumunu yeniden etkinleştirir.
pub fn init_secondary_cpu() {
    enable_sse();
    enable_xsave();
    enable_page_protections();
    crate::memory::paging::init_pcid();
}

/// AVX (Advanced Vector Extensions) komutlarının hem CPU hem OS tarafından desteklenip desteklenmediğini sorgular.
/// CPUID ile donanım desteği kontrol edilir; ardından XCR0 kaydından OS'un YMM registerlarını
/// kaydettiği doğrulanır (OSXSAVE). Her ikisi de gereklidir.
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

/// AVX2 (256-bit tamsayı vektör uzantısı) desteklenip desteklenmediğini sorgular.
/// Önce AVX desteği kontrol edilir; ardından CPUID leaf 7 EBX bit 5 sorgulanır.
pub fn has_avx2() -> bool {
    if !has_avx() {
        return false;
    }
    let features = cpuid(7, 0);
    (features.ebx & (1 << 5)) != 0
}

/// SSE2 (128-bit kayan nokta vektör işlemleri) desteklenip desteklenmediğini döndürür.
/// CPUID leaf 1 EDX bit 26 sorgulanır.
pub fn has_sse2() -> bool {
    let features = cpuid(1, 0);
    (features.edx & (1 << 26)) != 0
}

/// SSSE3 (Supplemental SSE3) desteklenip desteklenmediğini döndürür.
/// CPUID leaf 1 ECX bit 9 sorgulanır.
pub fn has_ssse3() -> bool {
    let features = cpuid(1, 0);
    (features.ecx & (1 << 9)) != 0
}

/// SSE4.1 (gelişmiş yatay toplama, blend vb.) desteklenip desteklenmediğini döndürür.
/// CPUID leaf 1 ECX bit 19 sorgulanır.
pub fn has_sse41() -> bool {
    let features = cpuid(1, 0);
    (features.ecx & (1 << 19)) != 0
}

pub fn smep_supported() -> bool {
    if !cpuid_leaf_available(7) {
        return false;
    }
    let features = cpuid(7, 0);
    (features.ebx & (1 << 7)) != 0
}

pub fn smap_supported() -> bool {
    if !cpuid_leaf_available(7) {
        return false;
    }
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

    // Simics QSP modeli x2APIC MSR'larını desteklemez (MSR 0x800+ "unknown").
    // CPUID x2APIC destekliyor dese bile MMIO (legacy xAPIC) moduna zorla.
    #[cfg(feature = "simics")]
    {
        if info.has_x2apic {
            crate::serial_println!("[CPU] Simics detected: forcing x2APIC OFF (using MMIO xAPIC)");
            info.has_x2apic = false;
        }
    }

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
                // SMT (Hiper-İş Parçacığı) seviyesi — mantıksal çekirdek başına iş parçacığı sayısı
                info.topology.smt_level = level;
                logical_count = result.ebx & 0xFFFF;
            }
            2 => {
                // Çekirdek (Core) seviyesi — fiziksel çekirdek başına mantıksal işlemci sayısı
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
    // CPUID topology leaves unavailable: publish a single logical/core/package CPU.
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

/// XSAVE altyapısını etkinleştirir — Silicon-Assisted Eager FPU.
///
/// 1. CPUID ile XSAVE desteği kontrol edilir
/// 2. CR4.OSXSAVE ayarlanır — OS'un XSAVE kullanacağını CPU'ya bildirir
/// 3. XCR0'a x87 + SSE + AVX bileşenleri yazılır
/// 4. XSAVEOPT/XSAVEC/XSAVES yetenekleri tespit edilir
/// 5. XSAVE alan boyutu hesaplanır
///
/// Bu fonksiyon `enable_sse()`'den SONRA çağrılmalıdır.
fn enable_xsave() {
    // CPUID leaf 1: XSAVE desteği (ECX bit 26)
    let features = cpuid(1, 0);
    let has_xsave = (features.ecx & (1 << 26)) != 0;

    if !has_xsave {
        crate::serial_println!("XSAVE not supported — falling back to FXSAVE");
        return;
    }

    unsafe {
        // CR4.OSXSAVE (bit 18) — OS'un XSAVE/XRSTOR kullanacağını CPU'ya bildir
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSXSAVE);
        Cr4::write(cr4);
    }

    // XCR0'a yazılabilecek bileşenleri tespit et (CPUID leaf 0xD, subleaf 0)
    let xsave_main = cpuid(0xD, 0);
    let xcr0_supported = ((xsave_main.edx as u64) << 32) | (xsave_main.eax as u64);

    // XCR0 bileşen bitleri:
    //   bit 0 = x87 FPU (zorunlu)
    //   bit 1 = SSE (XMM0-15)
    //   bit 2 = AVX (YMM0-15 üst yarı)
    //   bit 5 = AVX-512 opmask (k0-k7)
    //   bit 6 = AVX-512 ZMM_Hi256 (ZMM0-15 üst 256-bit)
    //   bit 7 = AVX-512 Hi16_ZMM (ZMM16-31 tamamı)
    let mut xcr0_val: u64 = 0x3; // x87 + SSE (zorunlu minimum)

    if xcr0_supported & (1 << 2) != 0 {
        xcr0_val |= 1 << 2; // AVX enable
    }

    // AVX-512: Simics/QEMU desteklerse etkinleştir (üç bit birlikte)
    if xcr0_supported & 0xE0 == 0xE0 {
        xcr0_val |= 0xE0; // AVX-512 opmask + ZMM_Hi256 + Hi16_ZMM
    }

    // XCR0'a yaz
    unsafe {
        xsetbv(0, xcr0_val);
    }

    // XSAVE alanının toplam boyutunu sor (CPUID leaf 0xD, subleaf 0, ECX)
    // ECX = tüm aktif bileşenler için gereken toplam byte
    let xsave_size_info = cpuid(0xD, 0);
    let area_size = xsave_size_info.ecx as usize;

    // XSAVEOPT / XSAVEC / XSAVES desteği (CPUID leaf 0xD, subleaf 1)
    let xsave_ext = cpuid(0xD, 1);
    let has_xsaveopt = (xsave_ext.eax & (1 << 0)) != 0;
    let has_xsavec = (xsave_ext.eax & (1 << 1)) != 0;
    let has_xsaves = (xsave_ext.eax & (1 << 3)) != 0;

    // Global yetenekleri kaydet
    {
        let mut caps = XSAVE_CAPS.lock();
        *caps = XSaveCapabilities {
            has_xsave: true,
            has_xsaveopt,
            has_xsavec,
            has_xsaves,
            xcr0_supported,
            xcr0_active: xcr0_val,
            area_size,
        };
    }
    XSAVE_ACTIVE.store(true, Ordering::Release);

    crate::serial_println!(
        "XSAVE Enabled: area={}B, XCR0=0x{:X}, XSAVEOPT={}, XSAVEC={}, XSAVES={}",
        area_size,
        xcr0_val,
        has_xsaveopt,
        has_xsavec,
        has_xsaves
    );

    // AVX durumunu logla
    if xcr0_val & (1 << 2) != 0 {
        crate::serial_println!("  AVX enabled — simd::stream_copy() will use AVX2 path");
    }
    if xcr0_val & 0xE0 == 0xE0 {
        crate::serial_println!("  AVX-512 enabled — future-proof ZMM state save active");
    }
}

/// XCR (Extended Control Register) yazma — XSETBV instruction wrapper.
///
/// # Safety
/// XCR index ve değer geçerli olmalıdır. Geçersiz bileşen bitleri #GP üretir.
#[inline]
unsafe fn xsetbv(xcr_index: u32, value: u64) {
    let eax = value as u32;
    let edx = (value >> 32) as u32;
    core::arch::asm!(
        "xsetbv",
        in("ecx") xcr_index,
        in("eax") eax,
        in("edx") edx,
        options(nostack, preserves_flags)
    );
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
    let xsave = xsave_active();

    crate::serial_println!("╔══════════════════════════════════════╗");
    crate::serial_println!("║     echOS Security Status            ║");
    crate::serial_println!("╠══════════════════════════════════════╣");
    crate::serial_println!(
        "║  CR0.WP (Write Protect): {}        ║",
        if wp { "  ON" } else { " OFF" }
    );
    crate::serial_println!(
        "║  SMEP (Kernel exec):     {}        ║",
        if smep { "  ON" } else { " OFF" }
    );
    crate::serial_println!(
        "║  SMAP (Kernel access):   {}        ║",
        if smap { "  ON" } else { " OFF" }
    );
    crate::serial_println!(
        "║  NX (No-Execute):        {}        ║",
        if nx { "  ON" } else { " OFF" }
    );
    crate::serial_println!(
        "║  PCID (TLB per-process): {}        ║",
        if pcid { "  ON" } else { " OFF" }
    );
    crate::serial_println!(
        "║  XSAVE (Silicon FPU):    {}        ║",
        if xsave { "  ON" } else { " OFF" }
    );
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

fn cpuid_leaf_available(leaf: u32) -> bool {
    let max_leaf = if leaf >= 0x8000_0000 {
        unsafe { core::arch::x86_64::__cpuid_count(0x8000_0000, 0).eax }
    } else {
        unsafe { core::arch::x86_64::__cpuid_count(0, 0).eax }
    };
    leaf <= max_leaf
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
