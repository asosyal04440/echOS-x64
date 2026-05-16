//! # echOS AML (ACPI Machine Language) Yorumlayıcı Modülü
//!
//! ## AML Nedir?
//! AML (ACPI Machine Language — ACPI Makine Dili), ACPI spesifikasyonunun tanımladığı
//! sıkıştırılmış bir bytecode formatıdır. Tıpkı Java'nın bytecode'u gibi platform bağımsızdır.
//! BIOS/UEFI firmware, donanım tanımlarını ve güç yönetimi prosedürlerini DSDT (Differentiated
//! System Description Table) ile isteğe bağlı SSDT tablolarına AML olarak gömülü biçimde sunar.
//!
//! ## AML Namespace Yapısı
//! ```text
//!  \_SB          → System Bus (tüm donanım cihazları burada)
//!    └─ PCI0     → PCI kök köprüsü (IRQ routing, _PRT)
//!         ├─ ISA → ISA köprüsü
//!         └─ EC  → Embedded Controller (dizüstü fan/batarya)
//!  \_TZ          → Thermal Zone (sıcaklık bölgeleri, fan kontrolü)
//!  \_PR          → Processor (CPU P-state / C-state tanımları)
//!  \_GPE         → General Purpose Event handler'ları
//!  \_S0.._S5     → Desteklenen uyku durumu tanımları
//!  \_PTS         → Prepare To Sleep (uyku öncesi çağrılır)
//!  \_WAK         → Wake (uyanma sonrası çağrılır)
//! ```
//!
//! ## Bu Modülün Görevi
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
// Global AML Bağlamı
// ============================================================================

/// Global AML bağlamı — tüm ACPI namespace bu yapı içinde yaşar.
/// Mutex ile korunur; method çağrısı (invocation) sırasında mutable erişim gerektiğinden
/// herhangi bir CPU'dan güvenli erişim sağlanır.
static AML_CONTEXT: Mutex<Option<AmlContext>> = Mutex::new(None);

/// AML yorumlayıcısının başarıyla başlatılıp başlatılmadığını atom olarak izler.
static AML_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// AML yorumlayıcısının hazır olup olmadığını döndürür.
/// Diğer modüller bu fonksiyonu kontrol ederek AML çağrısı yapabilirler.
pub fn is_initialized() -> bool {
    AML_INITIALIZED.load(Ordering::SeqCst)
}

// ============================================================================
// EchOsAmlHandler — aml::Handler trait implementasyonu
// ============================================================================

/// echOS AML Yöneticisi — AML yorumlayıcısının donanıma erişimi için gereken geri çağırım (callback) yapısı.
///
/// `aml` crate AML baytcodunu yorumlarken donanıma ya da platforma özgü işlemler için
/// bu trait'in metotlarını çağırır. echOS bu metotları şu şekilde uygular:
///
/// - Bellek Okuma/Yazma (u8, u16, u32, u64) — HHDM üzerinden fiziksel belleğe erişim
/// - G/Ç Port Okuma/Yazma (u8, u16, u32) — x86 `in`/`out` komutları
/// - PCI Yapılandırma Okuma/Yazma (u8, u16, u32) — CF8/CFC mekanizması
/// - Ölümcül hata yöneticisi
pub struct EchOsAmlHandler;

impl aml::Handler for EchOsAmlHandler {
    // ── Bellek Erişimi (fiziksel adres → HHDM sanal adres dönüşümü ile) ──
    // HHDM (Higher Half Direct Map) ile fiziksel adresler yüksek yarı sanal adreslere örtülür.
    // Örn: fiziksel 0x1000 → sanal 0xFFFF_8000_0000_1000 (HHDM offset eklenerek)

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

    // ── G/Ç Port Erişimi ──
    // x86 mimarisinde I/O uzayı bellek uzayından ayrıdır; `in`/`out` komutlarıyla erişilir.
    // Port adresi 16 bit (0x0000-0xFFFF); erişim genişliği 8, 16 veya 32 bit olabilir.

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

    // ── PCI Yapılandırma Uzayı Erişimi (CF8/CFC mekanizması) ──
    //
    // x86'da PCI yapılandırma uzayına iki port üzerinden erişilir:
    //   CF8h (CONFIG_ADDRESS) → 32-bit adres kaydı
    //   CFCh (CONFIG_DATA)    → 32-bit veri kaydı
    //
    // Adres formatı:
    //   ┌───────────────────────────────────────────────────┐
    //   │ 31:Enable │ 30:24 Saklı │ 23:16 Bus │ 15:11 Dev  │
    //   │ 10:8 Fonk │  7:2 Offset │  1:0 Sıfır             │
    //   └───────────────────────────────────────────────────┘

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
        // Oku-değiştir-yaz (Read-Modify-Write): 32-bit okuyup yalnızca ilgili baytı güncelle
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

    // ── Ölümcül Hata Yöneticisi ──

    fn handle_fatal_error(&self, fatal_type: u8, fatal_code: u32, fatal_arg: u64) {
        crate::serial_println!(
            "[AML] ÖLÜMCÜL HATA: tip={} kod=0x{:X} arg=0x{:X}",
            fatal_type,
            fatal_code,
            fatal_arg
        );
    }
}

/// PCI CF8 adres değeri hesaplar.
///
/// Format:
/// ```text
/// [31]    Enable bit — her zaman 1 olmalı
/// [30:24] Saklı (sıfır)
/// [23:16] Bus numarası (0-255)
/// [15:11] Cihaz numarası (0-31)
/// [10:8]  Fonksiyon numarası (0-7)
/// [7:0]   Register offset (4 bayt hizalı, bit 1:0 = 0)
/// ```
fn pci_config_address(_segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

// ============================================================================
// AML Bağlamı Başlatma
// ============================================================================

/// AML yorumlayıcısını başlatır — DSDT ve bulunan tüm SSDT tablolarını parse eder.
///
/// Bu fonksiyon `cpu::init()` sonrasında, ACPI tabloları parse edildikten sonra çağrılmalıdır.
/// Başlatma adımları:
/// 1. FADT'den DSDT fiziksel adresini oku (32-bit veya 64-bit X_DSDT alanı)
/// 2. DSDT baytlarını AmlContext'e yükle ve parse et
/// 3. Tüm SSDT tablolarını sırayla parse et (cihaz ve P-state tanımları burada)
/// 4. `initialize_objects()` ile _INI, _STA gibi namespace nesnelerini başlat
pub fn init_aml() {
    if is_initialized() {
        crate::serial_println!("[AML] Interpreter already initialized");
        return;
    }

    crate::serial_println!("[AML] Initializing AML interpreter...");

    // FADT'den DSDT adresini al
    let state = crate::cpu::acpi::ACPI_STATE.lock();

    if state.fadt_address == 0 {
        crate::serial_println!("[AML] FADT not found; AML init deferred");
        return;
    }

    // DSDT adresini FADT'den oku — iki olası alan var:
    //   FADT offset 0x28 → DSDT (32-bit, ACPI 1.0 uyumlu)
    //   FADT offset 0x8C → X_DSDT (64-bit, ACPI 2.0+ tercih edilen alan)
    let fadt_virt = crate::memory::phys_to_virt(state.fadt_address as usize);
    let dsdt_addr = unsafe {
        // FADT offset 0x28 = DSDT (32-bit)
        let dsdt32 = core::ptr::read_unaligned((fadt_virt + 0x28) as *const u32) as u64;

        // FADT uzunluğu >= 148 ise X_DSDT (64-bit, offset 0x8C) tercih et
        let fadt_len = core::ptr::read_unaligned((fadt_virt + 4) as *const u32);
        if fadt_len >= 148 {
            let dsdt64 = core::ptr::read_unaligned((fadt_virt + 0x8C) as *const u64);
            if dsdt64 != 0 {
                dsdt64
            } else {
                dsdt32
            }
        } else {
            dsdt32
        }
    };

    if dsdt_addr == 0 {
        crate::serial_println!("[AML] DSDT address is NULL");
        return;
    }

    crate::serial_println!("[AML] DSDT at 0x{:X}", dsdt_addr);

    // DSDT'yi ham bayt dilimi (slice) olarak hazırla
    let dsdt_virt = crate::memory::phys_to_virt(dsdt_addr as usize);
    let dsdt_len = unsafe { core::ptr::read_unaligned((dsdt_virt + 4) as *const u32) } as usize;

    if dsdt_len < 36 || dsdt_len > 4 * 1024 * 1024 {
        crate::serial_println!("[AML] DSDT length invalid: {}", dsdt_len);
        return;
    }

    let dsdt_table = unsafe { core::slice::from_raw_parts(dsdt_virt as *const u8, dsdt_len) };
    let dsdt_bytes = &dsdt_table[36..];
    crate::serial_println!("[AML] DSDT size: {} bytes", dsdt_len);

    // AmlContext oluştur — DebugVerbosity::None ile ayrıntılı AML log'u kapatılır
    let handler = Box::new(EchOsAmlHandler);
    let mut context = AmlContext::new(handler, DebugVerbosity::None);

    // DSDT'yi parse et ve namespace'e yükle
    match context.parse_table(dsdt_bytes) {
        Ok(()) => {
            crate::serial_println!("[AML] DSDT parsed successfully");
        }
        Err(e) => {
            crate::serial_println!("[AML] DSDT parse error: {:?}", e);
            // Parse hatası ölümcül değil; kısmi namespace yine de kullanışlı olabilir
        }
    }

    // Tüm SSDT tablolarını bul ve parse et
    // SSDT = Secondary System Description Table; ek cihaz tanımları ve P-state tabloları içerir
    let tables = state.tables.clone();
    drop(state); // Kilit serbest bırakılır; sonraki AML çağrıları için gerekli

    let mut ssdt_count = 0u32;
    for table in &tables {
        if &table.signature == b"SSDT" {
            let ssdt_virt = crate::memory::phys_to_virt(table.address as usize);
            let ssdt_len = table.length as usize;

            if ssdt_len >= 36 && ssdt_len <= 4 * 1024 * 1024 {
                let ssdt_table =
                    unsafe { core::slice::from_raw_parts(ssdt_virt as *const u8, ssdt_len) };
                let ssdt_bytes = &ssdt_table[36..];

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

    // Namespace nesnelerini başlat: _INI (başlatma), _STA (durum) gibi metotlar çalıştırılır.
    // Bu adım cihazların hazır duruma geçmesi için gereklidir.
    match context.initialize_objects() {
        Ok(()) => {
            crate::serial_println!("[AML] Namespace objects initialized");
        }
        Err(e) => {
            crate::serial_println!("[AML] Object init error (non-fatal): {:?}", e);
        }
    }

    // Namespace istatistiklerini kaydı için referans al (henüz kullanılmıyor)
    let ns = &context.namespace;
    crate::serial_println!("[AML] Namespace ready — DSDT + {} SSDTs loaded", ssdt_count);

    // Global bağlama kaydet ve başlatıldı bayrağını set et
    *AML_CONTEXT.lock() = Some(context);
    AML_INITIALIZED.store(true, Ordering::SeqCst);

    crate::serial_println!("╔══════════════════════════════════════╗");
    crate::serial_println!("║  AML Interpreter: ACTIVE             ║");
    crate::serial_println!("║  Tables: 1 DSDT + {} SSDTs            ║", ssdt_count);
    crate::serial_println!("╚══════════════════════════════════════╝");
}

// ============================================================================
// AML Metot Çağrısı — Genel API
// ============================================================================

/// Bir ACPI kontrol metodunu çalıştırır ve sonucunu döndürür.
///
/// AML metodları DSDT/SSDT içinde tanımlanmış küçük programlardır.
/// İşletim sistemi bu metodları çağırarak donanımı yönetir.
///
/// # Örnekler
/// ```
/// // PCI IRQ yönlendirme tablosunu oku
/// let prt = invoke_method("\\_SB.PCI0._PRT", &[]);
/// // Termal bölge sıcaklığını oku (Kelvin * 10 döner, örn: 3232 = 323.2K = 50.2°C)
/// let temp = invoke_method("\\_TZ.TZ00._TMP", &[]);
/// ```
pub fn invoke_method(path: &str, args: &[AmlValue]) -> Result<AmlValue, AmlError> {
    let mut ctx = AML_CONTEXT.lock();
    let context = ctx.as_mut().ok_or(AmlError::AmlNotInitialized)?;

    let name = AmlName::from_str(path).map_err(|_| AmlError::InvalidPath)?;

    let aml_args = aml::value::Args::from_list(args.to_vec()).map_err(|_| AmlError::InvalidArgs)?;

    context
        .invoke_method(&name, aml_args)
        .map_err(|e| AmlError::AmlCrateError(e))
}

/// ACPI namespace'teki bir nesneyi okur.
/// Belirtilen yol bir metot ise çalıştırır; değer ise doğrudan döndürür.
pub fn lookup(path: &str) -> Result<AmlValue, AmlError> {
    let mut ctx = AML_CONTEXT.lock();
    let context = ctx.as_mut().ok_or(AmlError::AmlNotInitialized)?;
    let name = AmlName::from_str(path).map_err(|_| AmlError::InvalidPath)?;
    context
        .namespace
        .get_by_path(&name)
        .cloned()
        .map_err(AmlError::AmlCrateError)
}

/// Namespace'teki bilinen tüm kök nesneleri seri porta yazdırır (hata ayıklama için).
pub fn debug_dump_namespace() {
    if !is_initialized() {
        crate::serial_println!("[AML] Context not initialized");
        return;
    }

    crate::serial_println!("[AML] === Namespace Dump ===");

    // Bilinen ACPI kök nesneleri — bunların varlığı donanım özelliklerini gösterir
    let paths = [
        "\\_SB",  // System Bus
        "\\_TZ",  // Thermal Zone
        "\\_PR",  // Processor (P-state/C-state tanımları)
        "\\_GPE", // General Purpose Events
        "\\_SI",  // System Indicators
        "\\_S0",  // S0 uyku durumu tanımı
        "\\_S3",  // S3 (Askı/Suspend to RAM) tanımı
        "\\_S4",  // S4 (Hazırda Bekletme) tanımı
        "\\_S5",  // S5 (Soft Off/Kapatma) tanımı
    ];

    for path in &paths {
        match lookup(path) {
            Ok(val) => crate::serial_println!("[AML]   {} = {:?}", path, val),
            Err(_) => {} // Bu nesne tanımlanmamış — sessizce geç
        }
    }

    crate::serial_println!("[AML] === End Dump ===");
}

// ============================================================================
// Yardımcı: Uyku Durumu Değerlendirmesi
// ============================================================================

fn get_sleep_type(path: &str, fallback: u16) -> Option<(u16, u16)> {
    if !is_initialized() {
        return None;
    }

    // \_Sx is usually a named Package object, not a control method. Read the
    // namespace value first; only fall back to method invocation for firmware
    // that implements it as executable AML.
    let value = lookup(path).or_else(|_| invoke_method(path, &[]));
    match value {
        Ok(AmlValue::Package(elements)) => {
            if elements.len() >= 2 {
                let slp_a = aml_value_to_u64(&elements[0]).unwrap_or(fallback as u64);
                let slp_b = aml_value_to_u64(&elements[1]).unwrap_or(slp_a);
                crate::serial_println!(
                    "[AML] {} evaluated: SLP_TYP_A={} SLP_TYP_B={}",
                    path,
                    slp_a,
                    slp_b
                );
                return Some((slp_a as u16, slp_b as u16));
            }
            None
        }
        Ok(val) => {
            crate::serial_println!("[AML] {} unexpected type: {:?}", path, val);
            None
        }
        Err(e) => {
            crate::serial_println!("[AML] {} evaluation failed: {:?}", path, e);
            None
        }
    }
}

/// ACPI S3 (Suspend to RAM) uyku türü değerlerini AML namespace üzerinden okur.
pub fn get_s3_sleep_type() -> Option<(u16, u16)> {
    get_sleep_type("\\_S3", 3)
}

/// ACPI S4 (Hibernate) uyku türü değerlerini AML namespace üzerinden okur.
pub fn get_s4_sleep_type() -> Option<(u16, u16)> {
    get_sleep_type("\\_S4", 4)
}

/// ACPI S5 (Soft Off) uyku türü değerlerini AML namespace üzerinden okur.
/// Bu değerler PM1_CNT kaydına yazılarak sistemin S5 durumuna girmesi sağlanır.
/// AML başlatılmamışsa `None` döner; çağıran kod statik parse sonucuna geri döner.
pub fn get_s5_sleep_type() -> Option<(u16, u16)> {
    get_sleep_type("\\_S5", 5)
}

/// AmlValue'yu u64 tam sayısına dönüştürür.
/// Yalnızca `AmlValue::Integer` varyantı için tanımlıdır; diğerleri `None` döndürür.
fn aml_value_to_u64(val: &AmlValue) -> Option<u64> {
    match val {
        AmlValue::Integer(n) => Some(*n),
        _ => None,
    }
}

// ============================================================================
// Hata Türü
// ============================================================================

/// echOS AML hata numaralandırması.
/// AML metot çağrıları başarısız olduğunda bu hata türleri döndürülür.
#[derive(Debug)]
pub enum AmlError {
    /// AML bağlamı henüz başlatılmadı — `init_aml()` önce çağrılmalı
    AmlNotInitialized,
    /// Geçersiz AML namespace yolu (örn: yanlış biçim veya geçersiz karakter)
    InvalidPath,
    /// Metoda geçilen argüman listesi uyumsuz
    InvalidArgs,
    /// `aml` crate'in döndürdüğü düşük seviyeli AML yorumlayıcı hatası
    AmlCrateError(aml::AmlError),
}
