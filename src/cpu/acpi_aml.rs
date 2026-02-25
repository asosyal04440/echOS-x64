//! # echOS AML (ACPI Machine Language) Interpreter Modülü
//!
//! `aml` crate (MIT lisanslı) kullanarak DSDT/SSDT tablolarını parse eder,
//! ACPI namespace'i oluşturur ve control method'larını çalıştırır.
//! Bu modül echOS'a Windows seviyesinde ACPI desteği kazandırır.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use aml::{AmlContext, AmlName, AmlValue, DebugVerbosity};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// Global AML Context
// ============================================================================

/// Global AML context — tüm ACPI namespace bu struct içinde yaşar.
/// Mutex ile korunur çünkü method invocation sırasında mutable erişim gerekir.
static AML_CONTEXT: Mutex<Option<AmlContext>> = Mutex::new(None);

/// AML başarıyla başlatıldı mı?
static AML_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// AML başlatıldı mı kontrol et
pub fn is_initialized() -> bool {
    AML_INITIALIZED.load(Ordering::SeqCst)
}

// ============================================================================
// EchOsAmlHandler — aml::Handler trait implementasyonu
// ============================================================================

/// echOS AML Handler — AML interpreter'ın donanıma erişimi için gerekli callback'ler.
///
/// 21 method implemente eder:
/// - Memory R/W (u8, u16, u32, u64) — HHDM üzerinden fiziksel bellek erişimi
/// - I/O Port R/W (u8, u16, u32) — x86 port I/O
/// - PCI Config R/W (u8, u16, u32) — CF8/CFC mekanizması
/// - Fatal error handler
pub struct EchOsAmlHandler;

impl aml::Handler for EchOsAmlHandler {
    // ── Memory Access (fiziksel adres → HHDM sanal adres) ──

    fn read_u8(&self, address: usize) -> u8 {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::read_volatile(virt as *const u8) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::read_volatile(virt as *const u16) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::read_volatile(virt as *const u32) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::read_volatile(virt as *const u64) }
    }

    fn write_u8(&mut self, address: usize, value: u8) {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::write_volatile(virt as *mut u8, value) }
    }

    fn write_u16(&mut self, address: usize, value: u16) {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::write_volatile(virt as *mut u16, value) }
    }

    fn write_u32(&mut self, address: usize, value: u32) {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::write_volatile(virt as *mut u32, value) }
    }

    fn write_u64(&mut self, address: usize, value: u64) {
        let virt = crate::memory::phys_to_virt(address);
        unsafe { core::ptr::write_volatile(virt as *mut u64, value) }
    }

    // ── I/O Port Access ──

    fn read_io_u8(&self, port: u16) -> u8 {
        unsafe { x86_64::instructions::port::Port::<u8>::new(port).read() }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        unsafe { x86_64::instructions::port::Port::<u16>::new(port).read() }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        unsafe { x86_64::instructions::port::Port::<u32>::new(port).read() }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        unsafe { x86_64::instructions::port::Port::<u8>::new(port).write(value) }
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        unsafe { x86_64::instructions::port::Port::<u16>::new(port).write(value) }
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        unsafe { x86_64::instructions::port::Port::<u32>::new(port).write(value) }
    }

    // ── PCI Configuration Space Access (CF8/CFC mekanizması) ──

    fn read_pci_u8(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            let val = x86_64::instructions::port::Port::<u32>::new(0xCFC).read();
            ((val >> ((offset & 3) * 8)) & 0xFF) as u8
        }
    }

    fn read_pci_u16(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            let val = x86_64::instructions::port::Port::<u32>::new(0xCFC).read();
            ((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
        }
    }

    fn read_pci_u32(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            x86_64::instructions::port::Port::<u32>::new(0xCFC).read()
        }
    }

    fn write_pci_u8(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u8,
    ) {
        // Read-modify-write: 32-bit okuyup ilgili byte'ı değiştir
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            let mut val = x86_64::instructions::port::Port::<u32>::new(0xCFC).read();
            let shift = (offset & 3) * 8;
            val &= !(0xFF << shift);
            val |= (value as u32) << shift;
            x86_64::instructions::port::Port::<u32>::new(0xCFC).write(val);
        }
    }

    fn write_pci_u16(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u16,
    ) {
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            let mut val = x86_64::instructions::port::Port::<u32>::new(0xCFC).read();
            let shift = (offset & 2) * 8;
            val &= !(0xFFFF << shift);
            val |= (value as u32) << shift;
            x86_64::instructions::port::Port::<u32>::new(0xCFC).write(val);
        }
    }

    fn write_pci_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        let addr = pci_config_address(segment, bus, device, function, offset);
        unsafe {
            x86_64::instructions::port::Port::<u32>::new(0xCF8).write(addr);
            x86_64::instructions::port::Port::<u32>::new(0xCFC).write(value);
        }
    }

    // ── Fatal Error ──

    fn handle_fatal_error(&self, fatal_type: u8, fatal_code: u32, fatal_arg: u64) {
        crate::serial_println!(
            "[AML] FATAL ERROR: type={} code=0x{:X} arg=0x{:X}",
            fatal_type,
            fatal_code,
            fatal_arg
        );
    }
}

/// PCI CF8 adres hesapla
/// Format: [31:enable] [30:24 reserved] [23:16 bus] [15:11 device] [10:8 function] [7:0 offset]
fn pci_config_address(_segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

// ============================================================================
// AML Context Başlatma
// ============================================================================

/// AML interpreter'ı başlat — DSDT ve tüm SSDT'leri parse et.
///
/// Bu fonksiyon `cpu::init()` sonrasında, ACPI tabloları parse edildikten sonra çağrılmalı.
/// FADT'den DSDT adresini alır, XSDT/RSDT'den SSDT'leri bulur.
pub fn init_aml() {
    crate::serial_println!("[AML] Initializing AML interpreter...");

    // FADT'den DSDT adresi al
    let state = crate::cpu::acpi::ACPI_STATE.lock();

    if state.fadt_address == 0 {
        crate::serial_println!("[AML] FADT not found — cannot initialize AML");
        return;
    }

    // DSDT adresini FADT'den oku
    let fadt_virt = crate::memory::phys_to_virt(state.fadt_address as usize);
    let dsdt_addr = unsafe {
        // FADT offset 0x28 = DSDT (32-bit)
        let dsdt32 = core::ptr::read_unaligned((fadt_virt + 0x28) as *const u32) as u64;

        // FADT uzunluğu >= 148 ise X_DSDT (64-bit, offset 0x8C) tercih et
        let fadt_len = core::ptr::read_unaligned((fadt_virt + 4) as *const u32);
        if fadt_len >= 148 {
            let dsdt64 = core::ptr::read_unaligned((fadt_virt + 0x8C) as *const u64);
            if dsdt64 != 0 { dsdt64 } else { dsdt32 }
        } else {
            dsdt32
        }
    };

    if dsdt_addr == 0 {
        crate::serial_println!("[AML] DSDT address is NULL");
        return;
    }

    crate::serial_println!("[AML] DSDT at 0x{:X}", dsdt_addr);

    // DSDT byte slice oluştur
    let dsdt_virt = crate::memory::phys_to_virt(dsdt_addr as usize);
    let dsdt_len = unsafe { core::ptr::read_unaligned((dsdt_virt + 4) as *const u32) } as usize;

    if dsdt_len < 36 || dsdt_len > 4 * 1024 * 1024 {
        crate::serial_println!("[AML] DSDT length invalid: {}", dsdt_len);
        return;
    }

    let dsdt_bytes = unsafe { core::slice::from_raw_parts(dsdt_virt as *const u8, dsdt_len) };
    crate::serial_println!("[AML] DSDT size: {} bytes", dsdt_len);

    // AmlContext oluştur
    let handler = Box::new(EchOsAmlHandler);
    let mut context = AmlContext::new(handler, DebugVerbosity::None);

    // DSDT parse et
    match context.parse_table(dsdt_bytes) {
        Ok(()) => {
            crate::serial_println!("[AML] DSDT parsed successfully");
        }
        Err(e) => {
            crate::serial_println!("[AML] DSDT parse error: {:?}", e);
            // Hata olsa bile devam et — kısmi namespace kullanılabilir
        }
    }

    // SSDT'leri bul ve parse et
    let tables = state.tables.clone();
    drop(state); // lock'u bırak

    let mut ssdt_count = 0u32;
    for table in &tables {
        if &table.signature == b"SSDT" {
            let ssdt_virt = crate::memory::phys_to_virt(table.address as usize);
            let ssdt_len = table.length as usize;

            if ssdt_len >= 36 && ssdt_len <= 4 * 1024 * 1024 {
                let ssdt_bytes =
                    unsafe { core::slice::from_raw_parts(ssdt_virt as *const u8, ssdt_len) };

                match context.parse_table(ssdt_bytes) {
                    Ok(()) => {
                        ssdt_count += 1;
                        crate::serial_println!(
                            "[AML] SSDT #{} parsed ({} bytes) at 0x{:X}",
                            ssdt_count,
                            ssdt_len,
                            table.address
                        );
                    }
                    Err(e) => {
                        crate::serial_println!(
                            "[AML] SSDT parse error at 0x{:X}: {:?}",
                            table.address,
                            e
                        );
                    }
                }
            }
        }
    }

    // Namespace objelerini başlat (_INI, _STA gibi method'ları çalıştır)
    match context.initialize_objects() {
        Ok(()) => {
            crate::serial_println!("[AML] Namespace objects initialized");
        }
        Err(e) => {
            crate::serial_println!("[AML] Object init error (non-fatal): {:?}", e);
        }
    }

    // Namespace istatistikleri
    let ns = &context.namespace;
    crate::serial_println!(
        "[AML] Namespace ready — DSDT + {} SSDTs loaded",
        ssdt_count
    );

    // Global context'e kaydet
    *AML_CONTEXT.lock() = Some(context);
    AML_INITIALIZED.store(true, Ordering::SeqCst);

    crate::serial_println!(
        "╔══════════════════════════════════════╗"
    );
    crate::serial_println!(
        "║  AML Interpreter: ACTIVE             ║"
    );
    crate::serial_println!(
        "║  Tables: 1 DSDT + {} SSDTs            ║",
        ssdt_count
    );
    crate::serial_println!(
        "╚══════════════════════════════════════╝"
    );
}

// ============================================================================
// AML Method Invocation — Genel API
// ============================================================================

/// ACPI control method'u çalıştır.
///
/// # Örnek
/// ```
/// let result = invoke_method("\\_SB.PCI0._PRT", &[]);
/// let temp = invoke_method("\\_TZ.TZ00._TMP", &[]);
/// ```
pub fn invoke_method(path: &str, args: &[AmlValue]) -> Result<AmlValue, AmlError> {
    let mut ctx = AML_CONTEXT.lock();
    let context = ctx.as_mut().ok_or(AmlError::AmlNotInitialized)?;

    let name = AmlName::from_str(path).map_err(|_| AmlError::InvalidPath)?;

    let aml_args = aml::value::Args::from_list(args.to_vec())
        .map_err(|_| AmlError::InvalidArgs)?;

    context
        .invoke_method(&name, aml_args)
        .map_err(|e| AmlError::AmlCrateError(e))
}

/// ACPI namespace'te bir objeyi oku (invoke_method wrapper — sadece lookup).
/// Eğer path bir method değilse, doğrudan namespace value döner.
pub fn lookup(path: &str) -> Result<AmlValue, AmlError> {
    // invoke_method ile deneriz — method ise çalıştırır, değilse namespace value döner
    invoke_method(path, &[])
}

/// Namespace'teki tüm objeleri serial log'a yaz (debug için).
pub fn debug_dump_namespace() {
    if !is_initialized() {
        crate::serial_println!("[AML] Context not initialized");
        return;
    }

    crate::serial_println!("[AML] === Namespace Dump ===");

    // Bilinen root objeleri kontrol et
    let paths = [
        "\\_SB",
        "\\_TZ",
        "\\_PR",
        "\\_GPE",
        "\\_SI",
        "\\_S0",
        "\\_S3",
        "\\_S4",
        "\\_S5",
    ];

    for path in &paths {
        match lookup(path) {
            Ok(val) => crate::serial_println!("[AML]   {} = {:?}", path, val),
            Err(_) => {} // tanımlı değil — sessizce geç
        }
    }

    crate::serial_println!("[AML] === End Dump ===");
}

// ============================================================================
// Helper: Sleep State evaluation (Faz 2'de genişletilecek)
// ============================================================================

/// ACPI S5 sleep type değerlerini AML'den oku.
/// Fallback: mevcut statik parse sonuçlarını kullan.
pub fn get_s5_sleep_type() -> Option<(u16, u16)> {
    if !is_initialized() {
        return None;
    }

    // \_S5 paketini evaluate et
    match invoke_method("\\_S5", &[]) {
        Ok(AmlValue::Package(elements)) => {
            if elements.len() >= 2 {
                let slp_a = aml_value_to_u64(&elements[0]).unwrap_or(5);
                let slp_b = aml_value_to_u64(&elements[1]).unwrap_or(slp_a);
                crate::serial_println!(
                    "[AML] \\_S5 evaluated: SLP_TYP_A={} SLP_TYP_B={}",
                    slp_a,
                    slp_b
                );
                return Some((slp_a as u16, slp_b as u16));
            }
            None
        }
        Ok(val) => {
            crate::serial_println!("[AML] \\_S5 unexpected type: {:?}", val);
            None
        }
        Err(e) => {
            crate::serial_println!("[AML] \\_S5 evaluation failed: {:?}", e);
            None
        }
    }
}

/// AmlValue'yi u64'e dönüştür
fn aml_value_to_u64(val: &AmlValue) -> Option<u64> {
    match val {
        AmlValue::Integer(n) => Some(*n),
        _ => None,
    }
}

// ============================================================================
// Error tipi
// ============================================================================

/// echOS AML error enum
#[derive(Debug)]
pub enum AmlError {
    /// AML context henüz başlatılmadı
    AmlNotInitialized,
    /// Geçersiz AML path
    InvalidPath,
    /// Geçersiz argümanlar
    InvalidArgs,
    /// `aml` crate hatası
    AmlCrateError(aml::AmlError),
}
