//! # echOS ACPI (Advanced Configuration and Power Interface) Modülü
//!
//! ## ACPI Nedir?
//! ACPI (Gelişmiş Yapılandırma ve Güç Arabirimi), Intel ve Microsoft tarafından 1996 yılında
//! tasarlanan, işletim sisteminin donanımı keşfetmesine ve güç yönetimini kontrol etmesine
//! olanak tanıyan bir standarttır. BIOS/firmware ile işletim sistemi arasındaki "köprü" rolünü
//! üstlenir. Sabit kodlanmış donanım bilgisi yerine, firmware içindeki tablolar ve küçük
//! programlar (AML bytecode) aracılığıyla donanım tanımlanır.
//!
//! ## ACPI Tablo Hiyerarşisi
//! ```text
//!  ┌─────────────────────────────────────────────────────────────┐
//!  │  RSDP (Root System Description Pointer)                     │
//!  │  → Bellekte sabit adreste (BIOS ROM / UEFI Config Table)    │
//!  └───────────────┬─────────────────────────────────────────────┘
//!                  │
//!          ┌───────▼────────┐
//!          │  XSDT / RSDT   │  ← Tüm diğer tablolara işaret eder
//!          │  (Root Table)  │
//!          └───┬───┬───┬────┘
//!              │   │   │
//!    ┌─────────▼┐ ┌▼──▼──────────────────────────────────┐
//!    │  FADT    │ │  MADT  │  SRAT  │  MCFG  │  DMAR  ...│
//!    │(Güç Yön.)│ │(APIC)  │(NUMA)  │(PCIe)  │(IOMMU)    │
//!    └────┬─────┘ └────────────────────────────────────────┘
//!         │
//!    ┌────▼─────┐
//!    │   DSDT   │  ← AML bytecode içerir (cihaz tanımları, güç methodları)
//!    └──────────┘
//! ```
//!
//! ## Güç Durumları (Sleep States / S-States)
//! ```text
//!  S0  → Tam Açık   : Sistem çalışıyor, en yüksek güç tüketimi
//!  S1  → Uyku       : CPU durduruldu, önbellek temizlendi, RAM güçlü
//!  S2  → Uyku       : S1'e benzer; CPU kapalı, platform bağımlı
//!  S3  → Askı       : RAM hariç tüm güç kapalı (DRAM Refresh devam)
//!  S4  → Hazırda Bek: RAM içeriği diske yazıldı; bellek de kapalı
//!  S5  → Soft Off   : Sistem kapalı; güç düğmesiyle açılabilir
//! ```
//!
//! ACPI tablo parsing, power management ve CPU/Memory topology discovery.
//! Minimal ACPICA subset implementasyonu.

use crate::boot::context::RsdpAddressKind;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

/// RSDP (Root System Description Pointer) — ACPI'nin bellekteki başlangıç noktası.
/// Bu imza, BIOS ROM bölgesinde veya UEFI yapılandırma tablosunda aranır.
const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

/// ACPI tablo imzaları — her tablonun ilk 4 baytı bu sabit dizelerle eşleşir.
/// Firmware bu tablolara bellek adreslerini XSDT/RSDT üzerinden bildirir.
const FADT_SIGNATURE: &[u8; 4] = b"FACP"; // Fixed ACPI Description Table (Güç yönetimi kayıtları)
const MADT_SIGNATURE: &[u8; 4] = b"APIC"; // Multiple APIC Description Table (CPU/IO APIC listesi)
const SRAT_SIGNATURE: &[u8; 4] = b"SRAT"; // System Resource Affinity Table (NUMA topolojisi)
const SLIT_SIGNATURE: &[u8; 4] = b"SLIT"; // System Locality Information Table (NUMA mesafeleri)
#[allow(dead_code)]
const SSDT_SIGNATURE: &[u8; 4] = b"SSDT"; // Secondary System Description Table (ek AML kod)
const MCFG_SIGNATURE: &[u8; 4] = b"MCFG"; // PCIe Memory-Mapped Config adresi
const DMAR_SIGNATURE: &[u8; 4] = b"DMAR"; // DMA Remapping (Intel VT-d / IOMMU tablosu)
const HPET_SIGNATURE: &[u8; 4] = b"HPET"; // HPET Description Table (yüksek hassasiyetli zamanlayıcı MMIO adresi)

/// Hatalı/devasa tabloların parse edilmesini önlemek için maksimum tablo boyutu (1 MB)
const MAX_ACPI_TABLE_SIZE: u32 = 1024 * 1024;

/// Global ACPI durumu — Mutex ile korunur; herhangi bir CPU'dan güvenli erişim sağlar.
pub static ACPI_STATE: Mutex<AcpiState> = Mutex::new(AcpiState::new());

/// Tüm ACPI tablo parse sonuçlarını ve güç yönetimi parametrelerini tutan ana yapı.
/// Bu yapıdaki veriler `parse_acpi_tables()` tarafından doldurulur ve
/// `acpi_shutdown()`, `acpi_reboot()` gibi fonksiyonlar tarafından kullanılır.
pub struct AcpiState {
    /// RSDP'nin bellekteki fiziksel adresi
    pub rsdp_address: u64,
    /// XSDT (Extended System Description Table) fiziksel adresi — 64-bit pointer listesi
    pub xsdt_address: u64,
    /// FADT (Fixed ACPI Description Table) fiziksel adresi — güç kayıtları burada
    pub fadt_address: u64,
    /// FACS (Firmware ACPI Control Structure) fiziksel adresi — S3 wake vector burada
    pub facs_address: u64,
    /// MADT (Multiple APIC Description Table) fiziksel adresi — CPU APIC ID'leri burada
    pub madt_address: u64,
    /// SRAT (System Resource Affinity Table) fiziksel adresi — NUMA node haritası
    pub srat_address: u64,
    /// SLIT (System Locality Information Table) fiziksel adresi — NUMA mesafe matrisi
    pub slit_address: u64,
    pub mcfg_address: u64,
    pub dmar_address: u64,
    /// Bellekte tespit edilen tüm ACPI tablolarının listesi
    pub tables: Vec<AcpiTable>,
    /// MADT'den çıkarılan CPU bilgileri (APIC ID'ler, sayısı, BSP)
    pub cpu_info: AcpiCpuInfo,
    pub ioapics: Vec<IoApicInfo>,
    pub interrupt_overrides: Vec<InterruptOverride>,
    pub mcfg_entries: Vec<PciEcamInfo>,
    pub dmar_units: Vec<DmarDrhd>,

    // ── Güç Yönetimi Kayıtları (FADT'den okunur) ──
    /// PM1a Kontrol Bloğu I/O port adresi — SLP_TYP ve SLP_EN bitleri buraya yazılır
    pub pm1a_cnt_blk: u16,
    /// PM1b Kontrol Bloğu I/O port adresi (0 = bu blok yok)
    pub pm1b_cnt_blk: u16,
    /// ACPI S3 (Suspend to RAM) uyku türü A değeri — DSDT \_S3 nesnesinden okunur
    pub slp_typ_s3_a: u16,
    /// ACPI S3 uyku türü B değeri — PM1b_CNT'ye yazılır
    pub slp_typ_s3_b: u16,
    /// DSDT içinde \_S3 paketi bulundu mu
    pub slp_typ_s3_valid: bool,
    /// ACPI S4 (Hibernate) uyku türü A değeri — DSDT \_S4 nesnesinden okunur
    pub slp_typ_s4_a: u16,
    /// ACPI S4 uyku türü B değeri — PM1b_CNT'ye yazılır
    pub slp_typ_s4_b: u16,
    /// DSDT içinde \_S4 paketi bulundu mu
    pub slp_typ_s4_valid: bool,
    /// ACPI S5 (Soft Off / kapatma) uyku türü A değeri — DSDT \_S5 nesnesinden okunur
    pub slp_typ_s5_a: u16,
    /// ACPI S5 uyku türü B değeri — PM1b_CNT'ye yazılır
    pub slp_typ_s5_b: u16,
    /// ACPI etkinleştirme komutu — bu değer SMI_CMD portuna yazılarak ACPI modu aktifleştirilir
    pub acpi_enable_cmd: u8,
    /// SMI Komut Portu — ACPI_ENABLE ve ACPI_DISABLE komutları bu port üzerinden gönderilir
    pub smi_cmd_port: u32,
    /// PM1a Olay Bloğu I/O adresi — güç düğmesi, uyku düğmesi olayları buradan okunur
    pub pm1a_evt_blk: u16,
    /// RESET kaydının fiziksel adresi (Generic Address Structure formatında)
    pub reset_reg_addr: u64,
    /// RESET kaydının adres uzayı: 0=sistem belleği, 1=I/O uzayı, 2=PCI yapılandırma uzayı
    pub reset_reg_space: u8,
    /// RESET kaydına yazılacak sihirli değer
    pub reset_value: u8,
    /// FADT bayrakları — bit 10 (RESET_REG_SUP) donanım sıfırlama desteğini gösterir
    pub fadt_flags: u32,
    /// SCI (System Control Interrupt) numarası — ACPI olayları bu IRQ üzerinden gelir
    pub sci_interrupt: u16,
    /// PM Timer I/O port adresi — FADT pm_tmr_blk (3.579545 MHz, 24-bit sayıcı)
    pub pm_tmr_blk: u16,
    /// HPET taban adresi — ACPI HPET tablosundan okunan MMIO adresi
    pub hpet_base: u64,
    /// FADT başarıyla parse edildi mi; false ise güç yönetimi işlemleri fallback kullanır
    pub fadt_parsed: bool,
}

/// Parse edilen bir ACPI tablosunu temsil eden hafif yapı.
/// Gerçek tablo verisi bellekte orijinal adresinde durur; buradan sadece işaret edilir.
#[derive(Debug, Clone)]
pub struct AcpiTable {
    pub signature: [u8; 4],
    pub address: u64,
    pub length: u32,
}

/// MADT'den çıkarılan CPU topoloji bilgileri.
/// Çok çekirdekli/çok işlemcili sistemlerde AP'leri başlatmak için kullanılır.
#[derive(Debug, Clone)]
pub struct AcpiCpuInfo {
    /// Sistemde tespit edilen toplam CPU sayısı (BSP + AP'ler)
    pub cpu_count: u32,
    /// BSP (Bootstrap Processor) APIC ID — işletim sistemini ilk başlatan CPU
    pub bsp_apic_id: u32,
    /// Local APIC'in MMIO taban adresi (varsayılan: 0xFEE00000)
    pub apic_base: u64,
    /// Her CPU'nun APIC ID'sini içeren liste — AP başlatma sırasında kullanılır
    pub cpu_list: Vec<u32>,
    /// NUMA düğüm bilgileri — hangi CPU hangi bellek bankasına yakın
    pub numa_nodes: Vec<NumaNode>,
}

/// Bir IO-APIC biriminin kimlik ve adres bilgisi.
/// Sistemde birden fazla IO-APIC olabilir; her biri farklı IRQ aralıklarını (GSI) yönetir.
#[derive(Debug, Clone)]
pub struct IoApicInfo {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

/// ISA IRQ'larının GSI'ye yeniden eşlenmesini tanımlar.
/// Örneğin: IRQ0 (PIT zamanlayıcı) genelde GSI 2'ye yönlendirilir.
#[derive(Debug, Clone)]
pub struct InterruptOverride {
    pub bus: u8,
    pub source: u8,
    pub gsi: u32,
    pub flags: u16,
}

/// PCIe ECAM (Enhanced Configuration Access Mechanism) bölge tanımı.
/// MCFG tablosundan okunur; PCIe yapılandırma uzayına MMIO ile erişimi sağlar.
#[derive(Debug, Clone)]
pub struct PciEcamInfo {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

/// DMAR tablosu içindeki bir PCI cihazının konum bilgisi (bus:device.function)
#[derive(Debug, Clone)]
pub struct DmarDeviceScope {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// DMAR DRHD (DMA Remapping Hardware Unit Definition) kaydı.
/// Her DRHD, bir VT-d donanım birimini ve onun kapsamındaki cihazları tanımlar.
#[derive(Debug, Clone)]
pub struct DmarDrhd {
    pub segment: u16,
    pub register_base: u64,
    pub include_all: bool,
    pub devices: Vec<DmarDeviceScope>,
}

/// NUMA (Non-Uniform Memory Access) düğüm tanımı.
/// Büyük sunucu sistemlerinde her CPU kümesinin kendi lokal belleği vardır;
/// lokal belleğe erişim, uzak belleğe erişimden çok daha hızlıdır.
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub node_id: u32,
    pub base_address: u64,
    pub length: u64,
    pub cpu_affinity: Vec<u32>,
}

impl AcpiState {
    /// Varsayılan (sıfır) değerlerle yeni bir ACPI durum nesnesi oluşturur.
    /// Bu fonksiyon `const` olduğundan derleme zamanında statik değişken olarak kullanılabilir.
    pub const fn new() -> Self {
        Self {
            rsdp_address: 0,
            xsdt_address: 0,
            fadt_address: 0,
            facs_address: 0,
            madt_address: 0,
            srat_address: 0,
            slit_address: 0,
            mcfg_address: 0,
            dmar_address: 0,
            tables: Vec::new(),
            cpu_info: AcpiCpuInfo {
                cpu_count: 1,
                bsp_apic_id: 0,
                apic_base: 0xFEE00000,
                cpu_list: Vec::new(),
                numa_nodes: Vec::new(),
            },
            ioapics: Vec::new(),
            interrupt_overrides: Vec::new(),
            mcfg_entries: Vec::new(),
            dmar_units: Vec::new(),
            pm1a_cnt_blk: 0,
            pm1b_cnt_blk: 0,
            slp_typ_s3_a: 0,
            slp_typ_s3_b: 0,
            slp_typ_s3_valid: false,
            slp_typ_s4_a: 0,
            slp_typ_s4_b: 0,
            slp_typ_s4_valid: false,
            slp_typ_s5_a: 0,
            slp_typ_s5_b: 0,
            acpi_enable_cmd: 0,
            smi_cmd_port: 0,
            pm1a_evt_blk: 0,
            reset_reg_addr: 0,
            reset_reg_space: 0,
            reset_value: 0,
            fadt_flags: 0,
            sci_interrupt: 0,
            pm_tmr_blk: 0,
            hpet_base: 0,
            fadt_parsed: false,
        }
    }
}

/// RSDP (Root System Description Pointer) ham bellek yapısı.
///
/// ACPI 1.0'da 20 bayt, ACPI 2.0+'da 36 bayt uzunluğundadır.
/// `revision` alanı sürümü belirtir: 0 = ACPI 1.0, 2 = ACPI 2.0+.
/// ACPI 2.0+'da `xsdt_address` kullanılır (64-bit); 1.0'da `rsdt_address` (32-bit).
#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

/// SDT (System Description Table) ortak başlık yapısı.
/// FADT, MADT, SRAT dahil tüm ACPI tablolarının başında bu 36 baytlık başlık bulunur.
/// `checksum` alanı, tablo baytlarının toplamının 0'a eşit olmasını zorunlu kılar.
#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// MADT (Multiple APIC Description Table) tablo başlığı.
/// Bu tablo sistemdeki tüm Local APIC ve IO-APIC birimlerini listeler.
/// Çok CPU'lu sistemlerde SMP başlatma için kritik tablodur.
#[repr(C, packed)]
struct Madt {
    header: SdtHeader,
    local_apic_address: u32,
    flags: u32,
    // Kayıtlar devamında gelir (değişken uzunluklu girişler)
}

/// MADT giriş tipi sabitleri — her giriş farklı bir donanım bileşenini tanımlar.
const MADT_ENTRY_LOCAL_APIC: u8 = 0; // 32-bit APIC ID'li CPU
const MADT_ENTRY_IO_APIC: u8 = 1; // IO-APIC birimi
const MADT_ENTRY_INTERRUPT_OVERRIDE: u8 = 2; // IRQ → GSI yönlendirme
#[allow(dead_code)]
const MADT_ENTRY_NMI: u8 = 4; // Non-Maskable Interrupt kaynağı
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_APIC_NMI: u8 = 5; // CPU'ya bağlı NMI
const MADT_ENTRY_LOCAL_APIC_ADDRESS_OVERRIDE: u8 = 6; // APIC taban adresi geçersizleme
#[allow(dead_code)]
const MADT_ENTRY_IO_SAPIC: u8 = 7; // IA-64 IO-SAPIC
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_SAPIC: u8 = 8; // IA-64 Local SAPIC
#[allow(dead_code)]
const MADT_ENTRY_PLATFORM_INTERRUPT: u8 = 9; // Platform interrupt kaynakları
const MADT_ENTRY_LOCAL_X2APIC: u8 = 10; // x2APIC (>255 CPU desteği için)
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_X2APIC_NMI: u8 = 11; // x2APIC NMI kaynağı

/// Klasik Local APIC giriş yapısı — 32-bit APIC ID (maksimum 255 CPU)
#[repr(C, packed)]
struct MadtLocalApic {
    entry_type: u8,
    length: u8,
    processor_id: u8,
    apic_id: u8,
    flags: u32, // bit 0: enabled, bit 1: online capable
}

/// x2APIC giriş yapısı — 32-bit genişletilmiş APIC ID (255+ CPU desteği)
#[repr(C, packed)]
struct MadtLocalX2Apic {
    entry_type: u8,
    length: u8,
    reserved: u16,
    x2apic_id: u32,
    flags: u32,
    acpi_processor_uid: u32,
}

#[repr(C, packed)]
struct MadtIoApic {
    entry_type: u8,
    length: u8,
    ioapic_id: u8,
    reserved: u8,
    ioapic_address: u32,
    gsi_base: u32,
}

#[repr(C, packed)]
struct MadtInterruptOverride {
    entry_type: u8,
    length: u8,
    bus: u8,
    source: u8,
    gsi: u32,
    flags: u16,
}

#[repr(C, packed)]
struct MadtLocalApicAddressOverride {
    entry_type: u8,
    length: u8,
    reserved: u16,
    address: u64,
}

/// ACPI Generic Address Structure (GAS) — 12 bayt
/// Çeşitli ACPI tablolarında (FADT RESET_REG, HPET BaseAddress, vs.) kullanılır.
#[repr(C, packed)]
struct GenericAddress {
    address_space: u8,
    bit_width: u8,
    bit_offset: u8,
    access_size: u8,
    address: u64,
}

#[repr(C, packed)]
struct McfgHeader {
    header: SdtHeader,
    reserved: u64,
}

#[repr(C, packed)]
struct McfgEntry {
    base_address: u64,
    segment_group: u16,
    start_bus: u8,
    end_bus: u8,
    reserved: u32,
}

/// RSDP'yi bellek içinde arar — önce UEFI yapılandırma tablosuna, ardından eski BIOS bölgesine bakar.
/// UEFI sistemlerde RSDP, EFI_ACPI_TABLE_GUID veya EFI_ACPI_20_TABLE_GUID ile işaretlenmiş
/// konfigürasyon tablosunda yer alır.
pub fn find_rsdp() -> Option<u64> {
    // UEFI sistemlerde RSDP EFI Yapılandırma Tablosundadır
    if let Some(uefi_rsdp) = find_rsdp_uefi() {
        return Some(uefi_rsdp);
    }

    // Eski BIOS: 0xE0000 - 0xFFFFF arasında 16 bayt hizalı noktalarda ara
    find_rsdp_bios()
}

/// UEFI boot aşamasında authoritative state'e kaydedilen RSDP adresini döndürür.
///
/// Wave 1: adres `acpi::publish_rsdp` ile tek authoritative state'te saklanır;
/// bu fonksiyon yalnızca state'e delege eder (ayrı state tutulmaz).
/// Fiziksel adres döndürülür; sanal (HHDM-lineer) adaylar fiziğe çevrilir.
fn find_rsdp_uefi() -> Option<u64> {
    let candidate = crate::acpi::authoritative_rsdp()?;
    Some(match candidate.address_kind {
        RsdpAddressKind::Physical => candidate.address,
        RsdpAddressKind::Virtual => {
            candidate.address.saturating_sub(crate::memory::active_physical_offset())
        }
    })
}

/// Eski BIOS bellein iki kritik bölgesini tarayarak RSDP'yi arar.
/// EBDA (Extended BIOS Data Area) ilk 1KB'ı ve BIOS ROM alanı kontrol edilir.
fn find_rsdp_bios() -> Option<u64> {
    // EBDA (Genişletilmiş BIOS Veri Alanı) ve BIOS ROM bölgelerini tara
    let search_areas = [
        (0x000E0000, 0x000FFFFF), // BIOS ROM — en yaygın konum
        (0x00080000, 0x0009FFFF), // Olası EBDA bölgesi
    ];

    for (start, end) in search_areas.iter() {
        if let Some(addr) = scan_for_rsdp(*start, *end) {
            return Some(addr);
        }
    }

    None
}

/// Verilen bellek aralığını 16 bayt adım aralıklarla tarayarak RSDP'yi arar.
/// ACPI spesifikasyonu RSDP'nin her zaman 16 bayt sınırında hizalanmış olduğunu garanti eder.
fn scan_for_rsdp(start: u64, end: u64) -> Option<u64> {
    // ACPI spec: RSDP her zaman 16 bayt sınırına (paragraph boundary) hizalanmıştır
    for addr in (start..end).step_by(16) {
        unsafe {
            let ptr = phys_to_virt_ptr::<Rsdp>(addr);

            // İmza eşleşmesini kontrol et ("RSD PTR ")
            if (*ptr).signature == *RSDP_SIGNATURE {
                // ACPI 1.0 checksum: ilk 20 baytın toplamı 0 olmalı
                if validate_checksum(ptr as *const u8, 20) {
                    return Some(addr);
                }

                // ACPI 2.0+ için tüm yapı üzerinde genişletilmiş checksum kontrolü
                if (*ptr).revision >= 2 {
                    let length = (*ptr).length as usize;
                    if validate_checksum(ptr as *const u8, length) {
                        return Some(addr);
                    }
                }
            }
        }
    }

    None
}

/// ACPI checksum doğrulama: verilen uzunluktaki tüm baytların aritmetik toplamı 0 olmalıdır.
/// Bu, tabloların bütünlüğünü sağlamak için ACPI spesifikasyonunun temel gereksinimidir.
fn validate_checksum(data: *const u8, length: usize) -> bool {
    let mut sum: u8 = 0;

    for i in 0..length {
        unsafe {
            sum = sum.wrapping_add(*data.add(i));
        }
    }

    sum == 0
}

fn read_sdt_length(header: *const SdtHeader) -> u32 {
    unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header).length)) }
}

fn read_sdt_signature(header: *const SdtHeader) -> [u8; 4] {
    unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header).signature)) }
}

/// Tüm ACPI tablolarını parse eder ve `ACPI_STATE` global değişkenini doldurur.
/// Bu fonksiyon ilk olarak RSDP'yi bulur, oradan XSDT/RSDT'ye, oradan da
/// FADT, MADT, SRAT gibi alt tablolara ulaşır.
pub fn parse_acpi_tables() -> bool {
    crate::serial_println!("Parsing ACPI tables...");

    // Wave 1: authoritative RSDP state'ini signature/checksum/length ile doğrula.
    // Hata durumunda loglanır; find_rsdp yine de state'ten beslenir ve
    // geçersiz adresle parse zaten başarısız olur (eski davranış + net log).
    match crate::acpi::validate_authoritative_rsdp() {
        Ok((rev, len)) => {
            crate::serial_println!("ACPI: RSDP validated (rev={}, len={})", rev, len)
        }
        Err(e) => crate::serial_println!("ACPI: RSDP validation failed: {:?}", e),
    }

    // İlk adım: RSDP'yi bul — bu olmadan ACPI kullanılamaz
    let rsdp_addr = match find_rsdp() {
        Some(addr) => addr,
        None => {
            crate::serial_println!("ACPI: RSDP not found");
            return false;
        }
    };

    crate::serial_println!("ACPI: RSDP found at 0x{:X}", rsdp_addr);

    let mut state = ACPI_STATE.lock();
    state.rsdp_address = rsdp_addr;

    // RSDP yapısını oku ve XSDT/RSDT adresini çıkar
    let rsdp = unsafe { &*phys_to_virt_ptr::<Rsdp>(rsdp_addr) };
    let rsdp_revision = rsdp.revision;
    let rsdp_xsdt = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(rsdp.xsdt_address)) };
    let rsdp_rsdt = rsdp.rsdt_address as u64;

    let mut parsed = false;
    crate::serial_println!(
        "ACPI: RSDP rev={} xsdt=0x{:X} rsdt=0x{:X}",
        rsdp_revision,
        rsdp_xsdt,
        rsdp_rsdt
    );
    // ACPI 2.0+ ise XSDT tercih edilir (64-bit adresler); aksi hâlde RSDT (32-bit adresler)
    if rsdp_revision >= 2 && rsdp_xsdt != 0 && is_canonical_lower_half(rsdp_xsdt) {
        state.xsdt_address = rsdp_xsdt;
        parsed = parse_xsdt(rsdp_xsdt, &mut state);
    }
    if !parsed {
        let rsdt_addr = rsdp_rsdt;
        if rsdt_addr != 0 && is_canonical_lower_half(rsdt_addr) {
            parsed = parse_rsdt(rsdt_addr, &mut state);
        }
    }
    if !parsed {
        crate::serial_println!("ACPI: XSDT/RSDT parse failed");
        return false;
    }

    // MADT'den CPU sayısını ve APIC ID'leri çıkar
    extract_cpu_info(&mut state);

    crate::serial_println!("ACPI: Found {} tables", state.tables.len());
    crate::serial_println!("ACPI: {} CPUs detected", state.cpu_info.cpu_count);

    true
}

/// XSDT (Extended System Description Table) parse eder.
/// XSDT, 8 baytlık (64-bit) tablo adres işaretçileri içerir; bu sayede 4 GB üstü adreslere erişilebilir.
fn parse_xsdt(xsdt_addr: u64, state: &mut AcpiState) -> bool {
    if !is_canonical_lower_half(xsdt_addr) {
        return false;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(xsdt_addr);
    let header = unsafe { &*header_ptr };

    let xsdt_len = read_sdt_length(header_ptr);
    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Invalid XSDT signature={:?} len={}",
            read_sdt_signature(header_ptr),
            xsdt_len
        );
        crate::serial_println!("ACPI: Invalid XSDT");
        return false;
    }

    // XSDT giriş sayısı: (toplam uzunluk - başlık boyutu) / 8 bayt (her giriş 64-bit işaretçi)
    let entry_count = (xsdt_len as usize - mem::size_of::<SdtHeader>()) / 8;
    crate::serial_println!("ACPI: XSDT entries={}", entry_count);

    let entries_base = phys_to_virt(xsdt_addr) + mem::size_of::<SdtHeader>();
    for i in 0..entry_count {
        let entry_addr = entries_base + i * 8;
        let table_addr = unsafe { core::ptr::read_unaligned(entry_addr as *const u64) };
        crate::serial_println!("ACPI: XSDT entry {} addr=0x{:X}", i, table_addr);
        parse_table(table_addr, state);
    }
    true
}

/// RSDT (Root System Description Table) parse eder.
/// Eski ACPI 1.0 tablosudur; 4 baytlık (32-bit) tablo adresi işaretçileri içerir.
/// Günümüzde yalnızca eski BIOS sistemlerde veya XSDT yoksa kullanılır.
fn parse_rsdt(rsdt_addr: u64, state: &mut AcpiState) -> bool {
    if !is_canonical_lower_half(rsdt_addr) {
        return false;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(rsdt_addr);
    let header = unsafe { &*header_ptr };

    let rsdt_len = read_sdt_length(header_ptr);
    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Invalid RSDT signature={:?} len={}",
            read_sdt_signature(header_ptr),
            rsdt_len
        );
        crate::serial_println!("ACPI: Invalid RSDT");
        return false;
    }

    // RSDT giriş sayısı: (toplam uzunluk - başlık boyutu) / 4 bayt (her giriş 32-bit işaretçi)
    let entry_count = (rsdt_len as usize - mem::size_of::<SdtHeader>()) / 4;
    crate::serial_println!("ACPI: RSDT entries={}", entry_count);

    let entries_base = phys_to_virt(rsdt_addr) + mem::size_of::<SdtHeader>();
    for i in 0..entry_count {
        let entry_addr = entries_base + i * 4;
        let table_addr = unsafe { core::ptr::read_unaligned(entry_addr as *const u32) } as u64;
        crate::serial_println!("ACPI: RSDT entry {} addr=0x{:X}", i, table_addr);
        parse_table(table_addr, state);
    }
    true
}

/// Tek bir ACPI tablosunu imzasına göre parse eder ve uygun işleyiciye yönlendirir.
/// Bilinmeyen imzalı tablolar listeye eklenir ama içeriği işlenmez.
fn parse_table(table_addr: u64, state: &mut AcpiState) {
    if !is_canonical_lower_half(table_addr) {
        return;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(table_addr);
    let header = unsafe { &*header_ptr };

    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Table invalid sig={:?} len={}",
            read_sdt_signature(header_ptr),
            read_sdt_length(header_ptr)
        );
        return;
    }

    // Tabloyu genel listeye kaydet
    let signature = read_sdt_signature(header_ptr);
    let length = read_sdt_length(header_ptr);
    let table = AcpiTable {
        signature,
        address: table_addr,
        length,
    };

    state.tables.push(table.clone());
    let sig = core::str::from_utf8(&signature).unwrap_or("????");
    crate::serial_println!("ACPI: Table {} at 0x{:X}", sig, table_addr);

    // İmzaya göre ilgili işleyiciye yönlendir
    match &header.signature {
        FADT_SIGNATURE => {
            state.fadt_address = table_addr;
            parse_fadt(table_addr, state);
        }
        MADT_SIGNATURE => {
            state.madt_address = table_addr;
            parse_madt(table_addr, state);
        }
        SRAT_SIGNATURE => {
            state.srat_address = table_addr;
            parse_srat(table_addr, state);
        }
        SLIT_SIGNATURE => {
            state.slit_address = table_addr;
            parse_slit(table_addr, state);
        }
        MCFG_SIGNATURE => {
            state.mcfg_address = table_addr;
            parse_mcfg(table_addr, state);
        }
        DMAR_SIGNATURE => {
            state.dmar_address = table_addr;
            parse_dmar(table_addr, state);
        }
        HPET_SIGNATURE => {
            parse_hpet(table_addr, state);
        }
        _ => {}
    }
}

pub fn get_dmar_units() -> Vec<DmarDrhd> {
    ACPI_STATE.lock().dmar_units.clone()
}

/// PM Timer (ACPI 3.579545 MHz) I/O port adresini döndürür.
/// TSC kalibrasyonu için kullanılır. 0 dönerse PM Timer mevcut değildir.
pub fn get_pm_tmr_port() -> u16 {
    ACPI_STATE.lock().pm_tmr_blk
}

/// HPET (High Precision Event Timer) MMIO taban adresini döndürür.
/// TSC kalibrasyonu için kullanılır. 0 dönerse HPET mevcut değildir.
pub fn get_hpet_base() -> u64 {
    ACPI_STATE.lock().hpet_base
}

fn parse_dmar(table_addr: u64, state: &mut AcpiState) {
    if !is_canonical_lower_half(table_addr) {
        return;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(table_addr);
    let header = unsafe { &*header_ptr };
    if !validate_table(header_ptr) {
        return;
    }
    let table_len = read_sdt_length(header_ptr) as usize;
    if table_len < mem::size_of::<SdtHeader>() + 12 {
        return;
    }
    let mut offset = mem::size_of::<SdtHeader>() + 12;
    while offset + 4 <= table_len {
        let entry_addr = phys_to_virt(table_addr + offset as u64);
        let entry_type = unsafe { core::ptr::read_unaligned(entry_addr as *const u16) };
        let entry_len = unsafe { core::ptr::read_unaligned((entry_addr + 2) as *const u16) };
        if entry_len < 4 {
            break;
        }
        let entry_len = entry_len as usize;
        if offset + entry_len > table_len {
            break;
        }
        if entry_type == 0 {
            if entry_len >= 16 {
                let flags = unsafe { core::ptr::read_unaligned((entry_addr + 4) as *const u8) };
                let segment = unsafe { core::ptr::read_unaligned((entry_addr + 6) as *const u16) };
                let register_base =
                    unsafe { core::ptr::read_unaligned((entry_addr + 8) as *const u64) };
                let include_all = (flags & 0x1) != 0;
                let mut devices = Vec::new();
                let mut scope_offset = 16usize;
                while scope_offset + 6 <= entry_len {
                    let scope_addr = entry_addr + scope_offset;
                    let scope_len =
                        unsafe { core::ptr::read_unaligned((scope_addr + 1) as *const u8) };
                    if scope_len < 6 {
                        break;
                    }
                    let scope_len = scope_len as usize;
                    if scope_offset + scope_len > entry_len {
                        break;
                    }
                    let start_bus =
                        unsafe { core::ptr::read_unaligned((scope_addr + 5) as *const u8) };
                    if scope_len >= 8 {
                        let path = unsafe {
                            core::slice::from_raw_parts(
                                (scope_addr + 6) as *const u8,
                                scope_len - 6,
                            )
                        };
                        if path.len() >= 2 {
                            let device = path[0];
                            let function = path[1];
                            devices.push(DmarDeviceScope {
                                bus: start_bus,
                                device,
                                function,
                            });
                        }
                    }
                    scope_offset = scope_offset.saturating_add(scope_len);
                }
                state.dmar_units.push(DmarDrhd {
                    segment,
                    register_base,
                    include_all,
                    devices,
                });
            }
        }
        offset = offset.saturating_add(entry_len);
    }
    if !state.dmar_units.is_empty() {
        crate::serial_println!(
            "ACPI: DMAR units={} addr=0x{:X}",
            state.dmar_units.len(),
            table_addr
        );
    }
}

/// ACPI tablosunun geçerliliğini doğrular: imza boş değil, uzunluk makul, checksum doğru.
/// Bu kontroller, bozuk veya kasıtlı olarak hatalı firmware tablolarına karşı koruma sağlar.
fn validate_table(header: *const SdtHeader) -> bool {
    let length = read_sdt_length(header);
    if length < mem::size_of::<SdtHeader>() as u32 || length > MAX_ACPI_TABLE_SIZE {
        return false;
    }
    if read_sdt_signature(header) == [0; 4] {
        return false;
    }
    // Tüm tablo baytlarının toplamı 0 olmalı (ACPI checksum kuralı)
    let data = header as *const u8;
    if !validate_checksum(data, length as usize) {
        return false;
    }
    true
}

fn is_canonical_lower_half(addr: u64) -> bool {
    addr <= 0x000F_FFFF_FFFF_FFFF || addr >= 0xFFF0_0000_0000_0000
}

fn phys_to_virt(addr: u64) -> usize {
    crate::memory::phys_to_virt(addr as usize)
}

fn phys_to_virt_ptr<T>(addr: u64) -> *const T {
    phys_to_virt(addr) as *const T
}

// ============================================================================
// FADT (Fixed ACPI Description Table) — Güç Yönetimi Kayıt Defteri
//
// ACPI Spec 6.5 §5.2.9 — Offsetler sabittir; yapı en az 116 bayttır.
// FADT; güç düğmesi, uyku kontrol blokları ve donanım sıfırlama
// kaydının adres ve değerlerini içerir. Kapatma/yeniden başlatma için
// bu tablodaki adresler ve değerler kritiktir.
// ============================================================================

/// FADT yapısı — yalnızca ihtiyaç duyulan alanlar; ofsetler ACPI spec'e göre sabit.
///
/// ```text
/// Offset  Alan                 Açıklama
/// ------  -------------------  --------------------------------
/// 0x00    header               SDT ortak başlık (36 bayt)
/// 0x24    firmware_ctrl        FACS (Firmware ACPI Control Struct) adresi
/// 0x28    dsdt                 DSDT fiziksel adresi (32-bit)
/// 0x2E    sci_interrupt        SCI (System Control Interrupt) IRQ numarası
/// 0x30    smi_cmd              SMI Komut Portu
/// 0x34    acpi_enable          ACPI modunu etkinleştirme komutu
/// 0x38    pm1a_evt_blk         PM1a Olay Bloğu portu
/// 0x40    pm1a_cnt_blk         PM1a Kontrol Bloğu portu ← kapatma/uyku için
/// 0x44    pm1b_cnt_blk         PM1b Kontrol Bloğu portu (opsiyonel)
/// 0x70    flags                FADT bayrakları (bit10=RESET_REG_SUP)
/// 0x74    reset_reg_*          Donanım sıfırlama kaydı (GAS formatı)
/// 0x80    reset_value          Sıfırlama için kayda yazılacak değer
/// ```
#[repr(C, packed)]
struct Fadt {
    header: SdtHeader,        // 0x00 (36 bayt)
    firmware_ctrl: u32,       // 0x24
    dsdt: u32,                // 0x28  ← DSDT fiziksel adresi (32-bit)
    reserved1: u8,            // 0x2C
    preferred_pm_profile: u8, // 0x2D
    sci_interrupt: u16,       // 0x2E  ← SCI IRQ numarası
    smi_cmd: u32,             // 0x30  ← SMI Komut Portu
    acpi_enable: u8,          // 0x34  ← ACPI etkinleştirme komutu
    acpi_disable: u8,         // 0x35
    s4bios_req: u8,           // 0x36
    pstate_cnt: u8,           // 0x37
    pm1a_evt_blk: u32,        // 0x38  ← PM1a Olay Bloğu
    pm1b_evt_blk: u32,        // 0x3C
    pm1a_cnt_blk: u32,        // 0x40  ← PM1a Kontrol Bloğu
    pm1b_cnt_blk: u32,        // 0x44  ← PM1b Kontrol Bloğu
    pm2_cnt_blk: u32,         // 0x48
    pm_tmr_blk: u32,          // 0x4C
    gpe0_blk: u32,            // 0x50
    gpe1_blk: u32,            // 0x54
    pm1_evt_len: u8,          // 0x58
    pm1_cnt_len: u8,          // 0x59
    pm2_cnt_len: u8,          // 0x5A
    pm_tmr_len: u8,           // 0x5B
    gpe0_blk_len: u8,         // 0x5C
    gpe1_blk_len: u8,         // 0x5D
    gpe1_base: u8,            // 0x5E
    cst_cnt: u8,              // 0x5F
    p_lvl2_lat: u16,          // 0x60
    p_lvl3_lat: u16,          // 0x62
    flush_size: u16,          // 0x64
    flush_stride: u16,        // 0x66
    duty_offset: u8,          // 0x68
    duty_width: u8,           // 0x69
    day_alarm: u8,            // 0x6A
    month_alarm: u8,          // 0x6B
    century: u8,              // 0x6C
    iapc_boot_arch: u16,      // 0x6D
    reserved2: u8,            // 0x6F
    flags: u32,               // 0x70  ← FADT bayrakları (bit 10 = RESET_REG_SUP)
    // Offset 0x74: RESET_REG (Generic Address Structure — 12 bayt)
    reset_reg_space: u8,       // 0x74  ← Adres uzayı (0=bellek, 1=I/O, 2=PCI)
    reset_reg_bit_width: u8,   // 0x75
    reset_reg_bit_offset: u8,  // 0x76
    reset_reg_access_size: u8, // 0x77
    reset_reg_addr: u64,       // 0x78  ← Sıfırlama kaydının fiziksel adresi
    reset_value: u8,           // 0x80  ← Sıfırlama için kayda yazılacak değer
}

/// FADT'yi parse eder ve güç yönetimi parametrelerini `AcpiState`'e aktarır.
/// PM1a/PM1b kontrol/olay blokları, RESET_REG ve SCI numarası bu fonksiyondan gelir.
/// Son olarak DSDT içindeki `\_S5` nesnesi okunarak S5 uyku türü değerleri elde edilir.
fn parse_fadt(fadt_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Parsing FADT at 0x{:X}", fadt_addr);

    let header_ptr = phys_to_virt_ptr::<SdtHeader>(fadt_addr);
    let fadt_len = read_sdt_length(header_ptr);

    // FADT en az 116 bayt olmalıdır (ACPI 1.0 minimum boyutu)
    if fadt_len < 116 {
        crate::serial_println!("ACPI: FADT too small ({}B)", fadt_len);
        return;
    }

    let fadt = unsafe { &*phys_to_virt_ptr::<Fadt>(fadt_addr) };

    // PM1a/PM1b Kontrol Bloklarını oku — kapatma/uyku komutları bu portlara yazılır
    let pm1a = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1a_cnt_blk)) };
    let pm1b = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1b_cnt_blk)) };
    let pm1a_evt = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1a_evt_blk)) };
    let smi_cmd = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.smi_cmd)) };
    let acpi_enable = fadt.acpi_enable;
    let sci_int = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.sci_interrupt)) };
    let flags = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.flags)) };
    let firmware_ctrl =
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.firmware_ctrl)) } as u64;
    let fadt_base = phys_to_virt(fadt_addr);
    let x_firmware_ctrl = if fadt_len >= 0x8C {
        unsafe { core::ptr::read_unaligned((fadt_base + 0x84) as *const u64) }
    } else {
        0
    };

    state.pm1a_cnt_blk = pm1a as u16;
    state.pm1b_cnt_blk = pm1b as u16;
    state.pm1a_evt_blk = pm1a_evt as u16;
    state.facs_address = if x_firmware_ctrl != 0 {
        x_firmware_ctrl
    } else {
        firmware_ctrl
    };
    state.smi_cmd_port = smi_cmd;
    state.acpi_enable_cmd = acpi_enable;
    state.sci_interrupt = sci_int;
    state.fadt_flags = flags;

    // PM Timer portu — TSC kalibrasyonu için kullanılır (3.579545 MHz, 24-bit sayıcı)
    let pm_tmr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm_tmr_blk)) };
    state.pm_tmr_blk = pm_tmr as u16;
    crate::serial_println!("ACPI: FADT PM_TMR_BLK=0x{:X} len={}", pm_tmr, fadt.pm_tmr_len);

    crate::serial_println!(
        "ACPI: FADT PM1a_CNT=0x{:X} PM1b_CNT=0x{:X} PM1a_EVT=0x{:X} FACS=0x{:X} SCI={}",
        pm1a,
        pm1b,
        pm1a_evt,
        state.facs_address,
        sci_int
    );
    crate::serial_println!(
        "ACPI: FADT SMI_CMD=0x{:X} ACPI_ENABLE=0x{:X} flags=0x{:X}",
        smi_cmd,
        acpi_enable,
        flags
    );

    // RESET_REG alanını oku — bu kayda reset_value yazmak sistemi yeniden başlatır.
    // FADT uzunluğu >= 129 bayt ise RESET_REG alanı mevcuttur (ACPI 2.0+).
    if fadt_len >= 129 {
        let reset_space = fadt.reset_reg_space;
        let reset_addr =
            unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.reset_reg_addr)) };
        let reset_val = fadt.reset_value;
        state.reset_reg_space = reset_space;
        state.reset_reg_addr = reset_addr;
        state.reset_value = reset_val;

        let reset_supported = (flags >> 10) & 1 == 1;
        crate::serial_println!(
            "ACPI: RESET_REG space={} addr=0x{:X} val=0x{:X} supported={}",
            reset_space,
            reset_addr,
            reset_val,
            reset_supported
        );
    }

    // DSDT'yi oku ve içindeki \_S3/\_S4/\_S5 paketleri için AML bayt dizisini tara
    let dsdt_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.dsdt)) } as u64;
    if dsdt_addr != 0 && is_canonical_lower_half(dsdt_addr) {
        parse_dsdt_sleep_states(dsdt_addr, state);
    }

    state.fadt_parsed = true;
    crate::serial_println!("ACPI: FADT parsed OK — shutdown/reboot ready");
}

/// DSDT AML bayt dizisini doğrusal tarayarak `\_S3`, `\_S4` ve `\_S5` paketlerini
/// bulur ve uyku türü değerlerini çıkarır. Bu değerler PM1_CNT kaydına yazılır.
///
/// DSDT içinde uyku paketleri şu formatta saklanır:
/// ```text
/// "_Sx_"  0x12  <PkgLength>  <ElemSayısı>  [0x0A <SLP_TYP_A>]  [0x0A <SLP_TYP_B>]
///  ^imza   ^DefPackage opkodu  ^paket uzunluğu  ^eleman sayısı  ^BytePrefix
/// ```
/// `0x0A` = BytePrefix (8-bit değer), `0x0B` = WordPrefix (16-bit değer)
fn parse_dsdt_sleep_states(dsdt_addr: u64, state: &mut AcpiState) {
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(dsdt_addr);
    let dsdt_len = read_sdt_length(header_ptr) as usize;

    if dsdt_len < mem::size_of::<SdtHeader>() + 4 {
        crate::serial_println!("ACPI: DSDT too small");
        return;
    }

    let dsdt_base = phys_to_virt(dsdt_addr);
    let data_start = dsdt_base + mem::size_of::<SdtHeader>();
    let data_end = dsdt_base + dsdt_len;

    // `_S5_` imzasının ASCII/bayt karşılığı: 0x5F 0x53 0x35 0x5F
    let s5_sig: [u8; 4] = [b'_', b'S', b'5', b'_'];
    let mut found = false;

    let data_slice =
        unsafe { core::slice::from_raw_parts(data_start as *const u8, data_end - data_start) };

    if let Some((slp_a, slp_b)) = find_dsdt_sleep_package(data_slice, [b'_', b'S', b'3', b'_']) {
        state.slp_typ_s3_a = slp_a;
        state.slp_typ_s3_b = slp_b;
        state.slp_typ_s3_valid = true;
        crate::serial_println!(
            "ACPI: DSDT \\_S3 found — SLP_TYP_A={} SLP_TYP_B={}",
            slp_a,
            slp_b
        );
    }

    if let Some((slp_a, slp_b)) = find_dsdt_sleep_package(data_slice, [b'_', b'S', b'4', b'_']) {
        state.slp_typ_s4_a = slp_a;
        state.slp_typ_s4_b = slp_b;
        state.slp_typ_s4_valid = true;
        crate::serial_println!(
            "ACPI: DSDT \\_S4 found — SLP_TYP_A={} SLP_TYP_B={}",
            slp_a,
            slp_b
        );
    }

    if let Some((slp_a, slp_b)) = find_dsdt_sleep_package(data_slice, s5_sig) {
        state.slp_typ_s5_a = slp_a;
        state.slp_typ_s5_b = slp_b;
        found = true;
        crate::serial_println!(
            "ACPI: DSDT \\_S5 found — SLP_TYP_A={} SLP_TYP_B={}",
            slp_a,
            slp_b
        );
    }

    if !found {
        // \_S5 bulunamadı; QEMU i440fx/q35 için bilinen varsayılan değerler kullanılıyor.
        // QEMU piix4'te SLP_TYP genellikle 0, q35'te 5'tir; 5 daha güvenli bir varsayılandır.
        state.slp_typ_s5_a = 5;
        state.slp_typ_s5_b = 5;
        crate::serial_println!("ACPI: DSDT \\_S5 not found — using QEMU default SLP_TYP=5");
    }
}

fn find_dsdt_sleep_package(data: &[u8], signature: [u8; 4]) -> Option<(u16, u16)> {
    for i in 0..data.len().saturating_sub(20) {
        if data[i..i + 4] != signature {
            continue;
        }

        let mut offset = i + 4;
        if offset >= data.len() || data[offset] != 0x12 {
            continue;
        }
        offset += 1;
        offset = skip_aml_pkg_length(data, offset)?;
        if offset >= data.len() {
            continue;
        }
        offset += 1;

        let (slp_a, next) = read_aml_integer(data, offset)?;
        let (slp_b, _) = read_aml_integer(data, next).unwrap_or((slp_a, next));
        return Some((slp_a & 0x7, slp_b & 0x7));
    }

    None
}

fn skip_aml_pkg_length(data: &[u8], offset: usize) -> Option<usize> {
    let first = *data.get(offset)?;
    let following = ((first >> 6) & 0x3) as usize;
    offset
        .checked_add(1 + following)
        .filter(|next| *next <= data.len())
}

fn read_aml_integer(data: &[u8], offset: usize) -> Option<(u16, usize)> {
    match *data.get(offset)? {
        0x00 => Some((0, offset + 1)),
        0x01 => Some((1, offset + 1)),
        0x0A => Some((*data.get(offset + 1)? as u16, offset + 2)),
        0x0B => Some((
            u16::from_le_bytes([*data.get(offset + 1)?, *data.get(offset + 2)?]),
            offset + 3,
        )),
        0x0C => Some((
            u32::from_le_bytes([
                *data.get(offset + 1)?,
                *data.get(offset + 2)?,
                *data.get(offset + 3)?,
                *data.get(offset + 4)?,
            ]) as u16,
            offset + 5,
        )),
        value => Some((value as u16, offset + 1)),
    }
}

pub fn dsdt_sleep_type(sleep_state: u8) -> Option<(u16, u16)> {
    let state = ACPI_STATE.lock();
    match sleep_state {
        3 if state.slp_typ_s3_valid => Some((state.slp_typ_s3_a, state.slp_typ_s3_b)),
        4 if state.slp_typ_s4_valid => Some((state.slp_typ_s4_a, state.slp_typ_s4_b)),
        5 if state.fadt_parsed => Some((state.slp_typ_s5_a, state.slp_typ_s5_b)),
        _ => None,
    }
}

pub fn arm_s3_resume_vector() -> bool {
    let facs_address = {
        let state = ACPI_STATE.lock();
        state.facs_address
    };
    if facs_address == 0 || !is_canonical_lower_half(facs_address) {
        crate::serial_println!("[ACPI] S3 blocked: FACS address unavailable");
        return false;
    }

    if !crate::cpu::s3_resume::prepare() {
        crate::serial_println!("[ACPI] S3 blocked: resume trampoline not ready");
        return false;
    }

    let facs_base = phys_to_virt(facs_address);
    let signature = unsafe { core::slice::from_raw_parts(facs_base as *const u8, 4) };
    if signature != b"FACS" {
        crate::serial_println!(
            "[ACPI] S3 blocked: invalid FACS signature at 0x{:X}",
            facs_address
        );
        return false;
    }

    let length = unsafe { core::ptr::read_unaligned((facs_base + 4) as *const u32) };
    if length < 16 {
        crate::serial_println!("[ACPI] S3 blocked: FACS length too small ({})", length);
        return false;
    }

    unsafe {
        core::ptr::write_unaligned(
            (facs_base + 12) as *mut u32,
            crate::cpu::s3_resume::resume_vector_phys(),
        );
        if length >= 32 {
            core::ptr::write_unaligned((facs_base + 24) as *mut u64, 0);
        }
        if length >= 40 {
            let ospm_flags = core::ptr::read_unaligned((facs_base + 36) as *const u32);
            core::ptr::write_unaligned((facs_base + 36) as *mut u32, ospm_flags & !1);
        }
    }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    crate::serial_println!(
        "[ACPI] S3 FACS FirmwareWakingVector armed FACS=0x{:X} vector=0x{:X}",
        facs_address,
        crate::cpu::s3_resume::resume_vector_phys()
    );
    true
}

pub fn s3_resume_vector_ready() -> bool {
    let facs_address = {
        let state = ACPI_STATE.lock();
        state.facs_address
    };
    if facs_address == 0 || !is_canonical_lower_half(facs_address) {
        crate::serial_println!("[ACPI] S3 blocked: FACS address unavailable");
        return false;
    }

    let facs_base = phys_to_virt(facs_address);
    let signature = unsafe { core::slice::from_raw_parts(facs_base as *const u8, 4) };
    if signature != b"FACS" {
        crate::serial_println!(
            "[ACPI] S3 blocked: invalid FACS signature at 0x{:X}",
            facs_address
        );
        return false;
    }

    let length = unsafe { core::ptr::read_unaligned((facs_base + 4) as *const u32) };
    let firmware_waking_vector =
        unsafe { core::ptr::read_unaligned((facs_base + 12) as *const u32) };
    let x_firmware_waking_vector = if length >= 32 {
        unsafe { core::ptr::read_unaligned((facs_base + 24) as *const u64) }
    } else {
        0
    };

    if firmware_waking_vector == 0 && x_firmware_waking_vector == 0 {
        crate::serial_println!(
            "[ACPI] S3 blocked: FACS wake vector is not armed (FACS=0x{:X})",
            facs_address
        );
        return false;
    }

    true
}

/// MADT'yi parse eder; Local APIC, IO-APIC ve kesme yönlendirme bilgilerini çıkarır.
/// Bu bilgiler: AP'leri (Application Processor) başlatmak, IO-APIC'i yapılandırmak ve
/// SMP (Simetrik Çoklu İşlem) altyapısını kurmak için kullanılır.
fn parse_madt(madt_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found MADT at 0x{:X}", madt_addr);

    let madt_ptr = phys_to_virt_ptr::<Madt>(madt_addr);
    let madt = unsafe { &*madt_ptr };
    let local_apic_address =
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(madt.local_apic_address)) };
    state.cpu_info.apic_base = local_apic_address as u64;

    // MADT başlığının hemen ardından değişken uzunluklu girişler gelir
    let header_ptr = unsafe { core::ptr::addr_of!((*madt_ptr).header) };
    let madt_len = read_sdt_length(header_ptr) as usize;
    let entries_start = phys_to_virt(madt_addr) + mem::size_of::<Madt>();
    let entries_end = phys_to_virt(madt_addr) + madt_len;

    let mut offset = entries_start;

    while offset < entries_end {
        let entry_type = unsafe { *(offset as *const u8) };
        let entry_length = unsafe { *((offset + 1) as *const u8) } as usize;

        match entry_type {
            MADT_ENTRY_LOCAL_APIC => {
                let entry = unsafe { &*(offset as *const MadtLocalApic) };

                // Hizasız erişimi önlemek için yerel kopyaya al
                let apic_id = entry.apic_id;
                let flags = entry.flags;

                if (flags & 1) != 0 || (flags & 2) != 0 {
                    // bit 0 = Etkin, bit 1 = Online Capable (başlatılabilir ama henüz kapalı)
                    crate::serial_println!("ACPI: Found APIC ID {} (flags={})", apic_id, flags);
                    state.cpu_info.cpu_list.push(apic_id as u32);
                }
            }

            MADT_ENTRY_LOCAL_X2APIC => {
                let entry = unsafe { &*(offset as *const MadtLocalX2Apic) };
                // Packed struct'tan güvenli kopyalama (hizasız erişim önleme)
                let x2id = entry.x2apic_id;
                let flags = entry.flags;

                if x2id == 0xFFFFFFFF {
                    // Geçersiz APIC ID — firmware bazen boş/devre dışı girişleri
                    // 0xFFFFFFFF ile işaretler. ACPI spec'e göre bu geçersizdir.
                    crate::serial_println!(
                        "ACPI: Skipping x2APIC entry with invalid ID 0xFFFFFFFF"
                    );
                } else if (flags & 1) != 0 || (flags & 2) != 0 {
                    // bit 0 = Etkin, bit 1 = Online Capable
                    crate::serial_println!("ACPI: Found x2APIC ID {} (flags={})", x2id, flags);
                    state.cpu_info.cpu_list.push(x2id);

                    if (flags & 2) != 0 {
                        // Bu BSP (önyükleme işlemcisi)
                        state.cpu_info.bsp_apic_id = x2id;
                    }
                }
            }
            MADT_ENTRY_IO_APIC => {
                let entry = unsafe { &*(offset as *const MadtIoApic) };
                state.ioapics.push(IoApicInfo {
                    id: entry.ioapic_id,
                    address: entry.ioapic_address,
                    gsi_base: entry.gsi_base,
                });
            }
            MADT_ENTRY_INTERRUPT_OVERRIDE => {
                let entry = unsafe { &*(offset as *const MadtInterruptOverride) };
                state.interrupt_overrides.push(InterruptOverride {
                    bus: entry.bus,
                    source: entry.source,
                    gsi: entry.gsi,
                    flags: entry.flags,
                });
            }
            MADT_ENTRY_LOCAL_APIC_ADDRESS_OVERRIDE => {
                let entry = unsafe { &*(offset as *const MadtLocalApicAddressOverride) };
                state.cpu_info.apic_base = entry.address;
            }

            _ => {
                // Diğer giriş tipleri şimdilik işlenmez
            }
        }

        offset += entry_length;
    }

    if !state.cpu_info.cpu_list.is_empty() {
        state.cpu_info.cpu_count = state.cpu_info.cpu_list.len() as u32;
    }
}

/// HPET (High Precision Event Timer) tablosunu parse eder.
///
/// HPET tablosu, yüksek hassasiyetli zamanlayıcının MMIO taban adresini içerir.
/// Bu adres TSC kalibrasyonunda HPET periyodik sayıcısını okumak için kullanılır.
/// Yapı formatı (ACPI 1.0+ / IA-PC HPET spec):
/// ```text
/// 0x00  SdtHeader        (36 bayt)
/// 0x24  Hardware Rev ID  (u8)
/// 0x25  Comparator Count + Legacy IRQ Capable (u8 — alt 4 bit sayıcı, bit5 legacy)
/// 0x26  PCI Vendor ID    (u16)
/// 0x28  Base Address     (Generic Address Structure — 12 bayt)
/// 0x34  Minimum Clock Tick (u16, üretim döngüsü başına minimum tick sayısı)
/// 0x36  Page Protection  (u8, bit0=4K, bit2=64K, vs.)
/// ```
#[repr(C, packed)]
struct HpetTable {
    header: SdtHeader,
    hardware_rev_id: u8,
    info: u8,
    pci_vendor_id: u16,
    base_address: GenericAddress,
    minimum_clock_tick: u16,
    page_protection: u8,
}

fn parse_hpet(hpet_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found HPET table at 0x{:X}", hpet_addr);
    let hpet = unsafe { &*phys_to_virt_ptr::<HpetTable>(hpet_addr) };
    let base_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hpet.base_address.address)) };
    let info_val = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(hpet.info)) };
    let comparators = (info_val & 0x1F) + 1;
    let legacy_irq = (info_val >> 5) & 1 == 1;

    if base_addr != 0 {
        crate::serial_println!(
            "ACPI: HPET base=0x{:X} rev=0x{:X} comparators={} legacy_irq={}",
            base_addr,
            hpet.hardware_rev_id,
            comparators,
            legacy_irq,
        );
        state.hpet_base = base_addr;
    } else {
        crate::serial_println!("ACPI: HPET table found but base address is 0");
    }
}

fn parse_mcfg(mcfg_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found MCFG at 0x{:X}", mcfg_addr);
    let header = unsafe { &*phys_to_virt_ptr::<McfgHeader>(mcfg_addr) };
    let header_ptr = core::ptr::addr_of!(header.header);
    if !validate_table(header_ptr) {
        return;
    }
    let entries_start = phys_to_virt(mcfg_addr) + mem::size_of::<McfgHeader>();
    let entries_end = phys_to_virt(mcfg_addr) + read_sdt_length(header_ptr) as usize;
    let mut offset = entries_start;
    while offset + mem::size_of::<McfgEntry>() <= entries_end {
        let entry = unsafe { &*(offset as *const McfgEntry) };
        state.mcfg_entries.push(PciEcamInfo {
            base_address: entry.base_address,
            segment_group: entry.segment_group,
            start_bus: entry.start_bus,
            end_bus: entry.end_bus,
        });
        offset += mem::size_of::<McfgEntry>();
    }
}

/// SRAT (System Resource Affinity Table) parse eder.
/// Hangi CPU kümesinin hangi bellek aralığına doğrudan erişebildiğini tanımlar (NUMA).
/// Bu bilgi, bellek yöneticisinin lokal NUMA düğümünden bellek ayırmasını sağlar.
fn parse_srat(srat_addr: u64, _state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found SRAT at 0x{:X}", srat_addr);
    // SRAT table discovery is logged here; NUMA affinity extraction is owned by
    // the topology path and must not be inferred from an unparsed table.
}

/// SLIT (System Locality Information Table) parse eder.
/// N×N boyutunda bir matris ile her NUMA düğüm çifti arasındaki göreli erişim gecikmesini verir.
/// Değer düşükse erişim hızlı (lokal), yüksekse yavaştır (uzak).
fn parse_slit(slit_addr: u64, _state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found SLIT at 0x{:X}", slit_addr);
    // SLIT table discovery is logged here; locality distance extraction remains
    // disabled until the NUMA topology owner consumes the matrix explicitly.
}

/// MADT parse sonucuna göre CPU bilgilerini tamamlar.
/// MADT yoksa veya boşsa, BSP (CPU 0) için temel değerler atanır.
fn extract_cpu_info(state: &mut AcpiState) {
    // MADT'den CPU bilgilerini kullan
    if state.cpu_info.cpu_count == 0 {
        // MADT bulunamazsa CPUID'den temel bilgi al (tek işlemcili sistem varsayımı)
        state.cpu_info.cpu_count = 1;
        state.cpu_info.bsp_apic_id = 0;
        state.cpu_info.cpu_list.push(0);
    }
}

/// ACPI'den elde edilen CPU topoloji bilgilerini döndürür.
/// SMP başlatma kodunun AP'leri (yardımcı işlemciler) bulmak için kullandığı bilgiler burada.
pub fn get_cpu_info() -> Option<AcpiCpuInfo> {
    let state = ACPI_STATE.lock();

    if state.cpu_info.cpu_count > 0 {
        Some(state.cpu_info.clone())
    } else {
        None
    }
}

pub fn get_ioapics() -> Vec<IoApicInfo> {
    let state = ACPI_STATE.lock();
    state.ioapics.clone()
}

pub fn get_interrupt_overrides() -> Vec<InterruptOverride> {
    let state = ACPI_STATE.lock();
    state.interrupt_overrides.clone()
}

pub fn get_mcfg_entries() -> Vec<PciEcamInfo> {
    let state = ACPI_STATE.lock();
    state.mcfg_entries.clone()
}

/// ACPI altyapısını başlatır: tabloları parse eder, CPU/güç bilgilerini doldurur.
/// Bu fonksiyon yalnızca bir kez, çekirdek başlatma sürecinde çağrılmalıdır.
pub fn init() -> bool {
    crate::serial_println!("Initializing ACPI...");

    if parse_acpi_tables() {
        crate::serial_println!("ACPI initialized successfully");
        true
    } else {
        crate::serial_println!("ACPI initialization failed, using fallback");
        false
    }
}

// ============================================================================
// Güç Yönetimi — Kapatma / Yeniden Başlatma / Uyku
//
// ACPI güç yönetimi için temel mekanizma:
//   1. PM1a_CNT (ve varsa PM1b_CNT) kontrol kaydına yazılır.
//   2. Kayıt formatı: [15:11 reserved] [12 SLP_EN] [12:10 SLP_TYP] [9:0 diğer]
//   3. SLP_EN bitini set etmek, SLP_TYP'nin gösterdiği uyku durumuna girişi tetikler.
//
//   PM1_CNT bit düzeni:
//   ┌────────────────────────────────────────┐
//   │  15..14  │ 13 SLP_EN │ 12..10 SLP_TYP │
//   └────────────────────────────────────────┘
//   SLP_TYP değeri firmware'e özgüdür; DSDT'deki \_S5 gibi nesnelerden okunur.
// ============================================================================

/// ACPI S5 (Soft Off) güç durumuna geçerek sistemi kapatır.
/// S5 durumunda tüm güç (DRAM dahil) kesilir; yalnızca güç düğmesi ile açılabilir.
/// QEMU'da bu durum "durduruldu" anlamına gelir ve sanal makineyi sonlandırır.
///
/// # Güvenlik Notu
/// Bu fonksiyon çağrılmadan önce tüm G/Ç işlemleri durdurulmalı ve kesmeler kapatılmalıdır.
pub fn acpi_shutdown() -> ! {
    let state = ACPI_STATE.lock();

    if !state.fadt_parsed || state.pm1a_cnt_blk == 0 {
        crate::serial_println!("[ACPI] FADT not parsed or PM1a_CNT=0 — trying QEMU fallback");
        drop(state);
        qemu_shutdown_fallback();
    }

    let pm1a = state.pm1a_cnt_blk;
    let pm1b = state.pm1b_cnt_blk;
    let mut slp_typ_a = state.slp_typ_s5_a;
    let mut slp_typ_b = state.slp_typ_s5_b;
    drop(state);

    // AML interpreter başlatılmışsa, \_S5 değerini AML üzerinden sorgu (daha güvenilir)
    if let Some((aml_a, aml_b)) = crate::cpu::acpi_aml::get_s5_sleep_type() {
        slp_typ_a = aml_a;
        slp_typ_b = aml_b;
        crate::serial_println!(
            "[ACPI] S5 from AML: SLP_TYP_A={} SLP_TYP_B={}",
            aml_a,
            aml_b
        );
    }

    crate::serial_println!("[ACPI] Shutting down (S5)...");
    crate::serial_println!(
        "[ACPI] PM1a=0x{:X} SLP_TYP_A={} PM1b=0x{:X} SLP_TYP_B={}",
        pm1a,
        slp_typ_a,
        pm1b,
        slp_typ_b
    );

    // Kesmeleri devre dışı bırak — kapatma sırasında kesme kabul edilmemeli
    x86_64::instructions::interrupts::disable();

    // PM1a_CNT'ye yaz: SLP_TYP (bit 10-12) | SLP_EN (bit 13)
    // Bu yazma işlemi donanımı S5 durumuna geçirir
    let sleep_value_a = (slp_typ_a << 10) | (1 << 13);
    unsafe {
        x86_64::instructions::port::Port::<u16>::new(pm1a).write(sleep_value_a);
    }

    // PM1b_CNT varsa ona da yaz (bazı platformlar çift kontrol bloğu kullanır)
    if pm1b != 0 {
        let sleep_value_b = (slp_typ_b << 10) | (1 << 13);
        unsafe {
            x86_64::instructions::port::Port::<u16>::new(pm1b).write(sleep_value_b);
        }
    }

    // Bu noktaya ulaşılmamalı; CPU S5 durumuna geçince tüm çalışma durur.
    // Ulaşılırsa QEMU'ya özgü fallback portları denenir.
    crate::serial_println!("[ACPI] S5 did not work — trying QEMU fallback");
    qemu_shutdown_fallback();
}

/// QEMU'ya özgü kapatma portlarını dener (ACPI S5 başarısız olursa yedek yol).
/// Port 0x604 QEMU q35/piix4 ACPI kapatma portudur; 0xB004 daha eski QEMU/Bochs içindir.
fn qemu_shutdown_fallback() -> ! {
    // QEMU ISA debug çıkış aygıtı (isa-debug-exit)
    unsafe {
        // Port 0x604: QEMU ACPI kapatma (piix4/q35 makineleri)
        x86_64::instructions::port::Port::<u16>::new(0x604).write(0x2000u16);
        // Port 0xB004: Bochs/eski QEMU
        x86_64::instructions::port::Port::<u16>::new(0xB004).write(0x2000u16);
    }
    loop {
        x86_64::instructions::hlt();
    }
}

/// ACPI RESET_REG üzerinden sistemi yeniden başlatır.
/// Reset kaydı üç farklı adres uzayında olabilir: sistem belleği, I/O portu veya PCI yapılandırması.
/// RESET_REG desteklenmiyorsa 8042 klavye denetleyicisi üzerinden triple-fault tetiklenir.
pub fn acpi_reboot() -> ! {
    let state = ACPI_STATE.lock();

    let reset_supported = (state.fadt_flags >> 10) & 1 == 1;

    if state.fadt_parsed && reset_supported && state.reset_reg_addr != 0 {
        let space = state.reset_reg_space;
        let addr = state.reset_reg_addr;
        let value = state.reset_value;
        drop(state);

        crate::serial_println!(
            "[ACPI] Rebooting via RESET_REG (space={} addr=0x{:X} val=0x{:X})",
            space,
            addr,
            value
        );

        x86_64::instructions::interrupts::disable();

        unsafe {
            match space {
                // Adres uzayı 0: Sistem Belleği — doğrudan MMIO yazımı
                0 => {
                    let ptr = phys_to_virt(addr) as *mut u8;
                    core::ptr::write_volatile(ptr, value);
                }
                // Adres uzayı 1: Sistem I/O Uzayı — x86 port komutu
                1 => {
                    x86_64::instructions::port::Port::<u8>::new(addr as u16).write(value);
                }
                // Adres uzayı 2: PCI Yapılandırma Uzayı — CF8/CFC mekanizması
                2 => {
                    // PCI yapılandırma uzayı erişimi CF8 (adres) ve CFC (veri) portları aracılığıyla
                    let pci_addr = 0x8000_0000u32 | ((addr as u32) & 0xFFFF);
                    x86_64::instructions::port::Port::<u32>::new(0xCF8).write(pci_addr);
                    x86_64::instructions::port::Port::<u8>::new(0xCFC).write(value);
                }
                _ => {}
            }
        }
    } else {
        drop(state);
        crate::serial_println!("[ACPI] RESET_REG not available — keyboard controller reset");
    }

    // Yedek: 8042 klavye denetleyicisi sıfırlama komutu.
    // 0xFE komutu CPU sıfırlamasını tetikler (triple fault oluşturur).
    crate::serial_println!("[ACPI] Fallback: keyboard controller reset (0x64)");
    x86_64::instructions::interrupts::disable();
    unsafe {
        // 8042 PS/2 denetleyicisi sıfırlama komutu
        x86_64::instructions::port::Port::<u8>::new(0x64).write(0xFE);
    }

    // Son çare: sonsuz döngü — bu noktaya ulaşılmamalı
    loop {
        x86_64::instructions::hlt();
    }
}

/// Belirli bir ACPI uyku durumuna (S1-S4) girer.
/// S1: CPU durdurulur; S3: RAM hariç her şey kapalı; S4: bellek diske yazılmış, tam kapalı.
/// Yalnızca FADT parse edilmişse ve PM1a_CNT mevcutsa çalışır.
pub unsafe fn enter_sleep_state(sleep_state: u8) -> bool {
    if sleep_state < 1 || sleep_state > 4 {
        return false;
    }

    let state = ACPI_STATE.lock();
    if !state.fadt_parsed || state.pm1a_cnt_blk == 0 {
        crate::serial_println!("[ACPI] Cannot enter S{}: FADT not available", sleep_state);
        return false;
    }

    let pm1a = state.pm1a_cnt_blk;
    let pm1b = state.pm1b_cnt_blk;
    drop(state);

    crate::serial_println!("[ACPI] Entering sleep state S{}", sleep_state);

    // SLP_TYP (bit 10-12) | SLP_EN (bit 13) birlikte yazılır
    let sleep_value = ((sleep_state as u16) << 10) | (1 << 13);

    x86_64::instructions::port::Port::<u16>::new(pm1a).write(sleep_value);

    if pm1b != 0 {
        x86_64::instructions::port::Port::<u16>::new(pm1b).write(sleep_value);
    }

    false
}

/// CPU frekans ölçeklendirmesi için P-state (Performance State) değiştirir.
/// IA32_PERF_CTL MSR (0x199) üzerinden hedef P-state yazılır.
/// P-state 0 en yüksek frekansı, N en düşük frekansı temsil eder (DVFS).
pub fn set_pstate(pstate: u8) -> bool {
    // CPUID yaprak 6'da P-state kontrolünün desteklenip desteklenmediğini sorgula
    let cpuid_result = crate::cpu::cpuid(6, 0);

    if (cpuid_result.eax & (1 << 1)) == 0 {
        return false; // P-state donanım kontrolü (IDA/Turbo) desteklenmiyor
    }

    // MSR 0x199 (IA32_PERF_CTL) — Intel SpeedStep P-state kontrol kaydı
    unsafe {
        use x86_64::registers::model_specific::Msr;
        // MSR yazımı için mut gerekli
        let mut perf_ctl = Msr::new(0x199);

        // P-state değeri (0 = en yüksek performans, N = en düşük güç tüketimi)
        let value = (pstate as u64) & 0xFF;
        perf_ctl.write(value);
    }

    true
}

/// Termal bölge (Thermal Zone) yapısı — bir sıcaklık kaynağını ve eşik noktalarını tutar.
pub struct ThermalZone {
    pub temperature: i32, // Santigrat × 10 (örn: 450 = 45.0°C)
    pub trip_points: Vec<TripPoint>,
}

/// Termal eşik noktası — belirli bir sıcaklıkta alınacak eylemi tanımlar.
pub struct TripPoint {
    pub temperature: i32,
    pub trip_type: u8, // 0: kritik (kapat), 1: sıcak (uyar), 2: pasif (kısıtla), 3: aktif (fan)
}

/// Sistemdeki termal bölgeleri listeler.
/// Gerçek implementasyonda ACPI \_TZ namespace'inden termal bölgeler keşfedilir.
pub fn detect_thermal_zones() -> Vec<ThermalZone> {
    let mut zones = Vec::new();

    // ACPI termal bölgelerini ara
    // Şimdilik örnek bir termal bölge ekleniyor
    zones.push(ThermalZone {
        temperature: 450, // 45.0°C
        trip_points: vec![
            TripPoint {
                temperature: 800,
                trip_type: 0,
            }, // Kritik: 80°C — sistem kapatılır
            TripPoint {
                temperature: 700,
                trip_type: 1,
            }, // Sıcak: 70°C — uyarı verilir
        ],
    });

    zones
}

/// Pil (batarya) bilgi yapısı — dizüstü bilgisayarlar için güç kaynağı durumunu tutar.
pub struct BatteryInfo {
    pub present: bool,
    pub charging: bool,
    pub capacity: u8, // Yüzde cinsinden şarj seviyesi
    pub voltage: u16, // Milivolt cinsinden gerilim
}

/// ACPI pil durumunu okur.
/// Gerçek implementasyonda ACPI\_SB.BAT0._BST AML metodu çağrılır.
pub fn get_battery_info() -> Option<BatteryInfo> {
    // No BAT-class ACPI namespace walker is registered in this boot profile.
    // Report "no battery present" instead of fabricating charge telemetry.
    Some(BatteryInfo {
        present: false,
        charging: false,
        capacity: 0,
        voltage: 0,
    })
}

/// Tüm ACPI tablo adreslerini ve CPU bilgilerini seri porta yazdırır.
/// Hata ayıklama ve donanım tanılama için kullanılır.
pub fn debug_print_tables() {
    let state = ACPI_STATE.lock();

    crate::serial_println!("=== ACPI Debug Info ===");
    crate::serial_println!("RSDP: 0x{:X}", state.rsdp_address);
    crate::serial_println!("XSDT: 0x{:X}", state.xsdt_address);
    crate::serial_println!("FADT: 0x{:X}", state.fadt_address);
    crate::serial_println!("MADT: 0x{:X}", state.madt_address);
    crate::serial_println!("SRAT: 0x{:X}", state.srat_address);
    crate::serial_println!("SLIT: 0x{:X}", state.slit_address);
    crate::serial_println!("MCFG: 0x{:X}", state.mcfg_address);

    crate::serial_println!("CPU Count: {}", state.cpu_info.cpu_count);
    crate::serial_println!("BSP APIC ID: {}", state.cpu_info.bsp_apic_id);
    crate::serial_println!("APIC Base: 0x{:X}", state.cpu_info.apic_base);

    crate::serial_println!("CPU List: {:?}", state.cpu_info.cpu_list);
    crate::serial_println!("Tables found: {}", state.tables.len());

    for table in &state.tables {
        let sig = core::str::from_utf8(&table.signature).unwrap_or("????");
        crate::serial_println!("  {}: 0x{:X} ({} bytes)", sig, table.address, table.length);
    }
}
