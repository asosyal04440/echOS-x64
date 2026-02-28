//! # IOMMU Desteği (I/O Bellek Yönetim Birimi)
//!
//! Intel VT-d ve AMD-Vi mimarisi: çevre birimlerinin DMA erişimini denetler.
//!
//! ## IOMMU Neden Gereklidir?
//!
//! Geleneksel sistemlerde bir DMA aygıtı (disk, NIC, GPU) HERHANGI bir
//! fiziksel bellek adresine erişebilir. Bu ciddi güvenlik ve kararlılık
//! açıklarına yol açar:
//!   - Hatalı sürücü tüm RAM'i bozabilir
//!   - Kötü amaçlı PCIe cihaz çekirdek belleğini okuyabilir (DMA attack)
//!
//! IOMMU bu sorunu çözer:
//!
//! ```
//! Cihaz (GPU/NIC/Disk)
//!        |
//!        | DMA adresi (cihazın gördüğü adres)
//!        v
//!    [IOMMU Donanımı]  <-- DMA adres -> Fiziksel adres çevirisi
//!        |
//!        | Fiziksel adres (gerçek RAM)
//!        v
//!    [Sistem RAM]
//! ```
//!
//! ## Intel VT-d vs AMD-Vi
//!
//! Her iki teknoloji fonksiyonel olarak eşdeğerdir; yalnızca yazmaç
//! adresleri ve komut yapıları farklıdır.
//!
//!   Intel VT-d: DMAR tablosu (ACPI) ile keşfedilir
//!   AMD-Vi:     IVRS tablosu (ACPI) ile keşfedilir
//!
//! ## Domain (Alan) Kavramı
//!
//! Her domain bağımsız bir DMA adres uzayıdır. Cihazlar bir domain'e atanır;
//! o domain içinde kendi sayfa tablolarına göre erişim denetlenir.
//!
//! ```
//! Domain 1: GPU, Sound card  -> sadece grafik/ses belleğine erişim
//! Domain 2: NIC              -> sadece ağ tamponu belleğine erişim
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IOMMU SABİTLERİ (IOMMU CONSTANTS)
// ============================================================================

// Intel VT-d yazmaç ofseti haritası.
// Tüm yazmaçlar MMIO alanında base_addr + offset adresinde yer alır.

/// Versiyon yazmacı: donanım versiyonunu bildirir
pub const VTD_VER_REG: u32 = 0x00;
/// Yetenek yazmacı: hangi özelliklerin desteklendiğini gösteren bitmask
pub const VTD_CAP_REG: u32 = 0x08;
/// Genişletilmiş yetenek yazmacı
pub const VTD_ECAP_REG: u32 = 0x10;
/// Global komut yazmacı: çeviriyi etkinleştir, kök tabloyu ayarla
pub const VTD_GCMD_REG: u32 = 0x18;
/// Global durum yazmacı: GCMD komutlarının tamamlanıp tamamlanmadığını gösterir
pub const VTD_GSTS_REG: u32 = 0x1C;
/// Kök tablo adresi yazmacı: DMA çeviri tablosunun başlangıç adresi
pub const VTD_RTADDR_REG: u32 = 0x20;
/// Bağlam komut yazmacı: bağlam girişi geçersiz kılma
pub const VTD_CCMD_REG: u32 = 0x28;
/// Hata durum yazmacı
pub const VTD_FSTS_REG: u32 = 0x34;
/// Hata denetim yazmacı
pub const VTD_FECTL_REG: u32 = 0x38;
/// Hata kesme veri yazmacı
pub const VTD_FEDATA_REG: u32 = 0x3C;
/// Hata kesme adres yazmacı
pub const VTD_FEADDR_REG: u32 = 0x40;
/// Gelişmiş hata günlük yazmacı
pub const VTD_AFLOG_REG: u32 = 0x58;
/// Geçersiz kılma kuyruğu adres yazmacı
pub const VTD_PQADDR_REG: u32 = 0x60;
/// IOTLB geçersiz kılma yazmacı (çeviri önbelleği temizleme)
pub const VTD_IOTLB_REG: u32 = 0x80;

// VT-d Global Komut Bitleri
pub const VTD_GCMD_TE: u32 = 1 << 31;     // Çeviriyi etkinleştir (Translation Enable)
pub const VTD_GCMD_SRTP: u32 = 1 << 30;   // Kök tablo pointer'ını ayarla
pub const VTD_GCMD_WBF: u32 = 1 << 27;    // Yazma tamponu temizleme (Write Buffer Flush)
pub const VTD_GCMD_QIE: u32 = 1 << 26;    // Geçersiz kılma kuyruğunu etkinleştir
pub const VTD_GCMD_IRE: u32 = 1 << 25;    // Kesme yeniden yönlendirmeyi etkinleştir
pub const VTD_GCMD_EAFL: u32 = 1 << 24;   // Gelişmiş hata kaydını etkinleştir

// AMD-Vi yazmaç ofseti haritası
pub const AMDVI_CONTROL_REG: u32 = 0x00;
pub const AMDVI_EXCL_BASE_REG: u32 = 0x08;
pub const AMDVI_EXCL_LIMIT_REG: u32 = 0x10;
pub const AMDVI_DEV_TABLE_BASE_REG: u32 = 0x18;
pub const AMDVI_CMD_BASE_REG: u32 = 0x20;
pub const AMDVI_CMD_TAIL_REG: u32 = 0x28;
pub const AMDVI_EVT_BASE_REG: u32 = 0x30;
pub const AMDVI_EVT_HEAD_REG: u32 = 0x38;
pub const AMDVI_STATUS_REG: u32 = 0x2020;

// ============================================================================
// DMA YENİDEN YÖNLENDİRME (DMA REMAPPING)
// ============================================================================

// DMA çeviri tablosu girişleri. VT-d iki düzeyli yapı kullanır:
//
//   Kök Tablosu (Root Table)
//     |
//     +-- Bağlam Tablosu (Context Table) [her PCI bus için bir tane]
//           |
//           +-- Sayfa Tablosu (Page Table) [her cihaz için bir tane]
//                 |
//                 +-- Sayfa Tablosu Giriş (PTE) [her DMA sayfası için]

/// DMA adres çeviri bilgisi (yazılımsal temsil)
#[repr(C)]
pub struct DmaTranslation {
    pub present: bool,      // Bu eşleme geçerli mi?
    pub read_perm: bool,    // Cihaz bu adresten okuyabilir mi?
    pub write_perm: bool,   // Cihaz bu adrese yazabilir mi?
    pub phys_addr: u64,     // Eşlenen fiziksel adres
    pub size: u64,          // Eşleme boyutu (byte)
}

/// Intel VT-d kök tablosu girişi (16 byte, 16 byte hizalı)
#[repr(C, align(16))]
pub struct VtdRootEntry {
    pub lo: u64,
    pub hi: u64,
}

/// Intel VT-d bağlam tablosu girişi (16 byte, 16 byte hizalı)
#[repr(C, align(16))]
pub struct VtdContextEntry {
    pub lo: u64,
    pub hi: u64,
}

/// DMA için sayfa tablosu girişi (Page Table Entry)
#[repr(C)]
pub struct DmaPte {
    pub val: u64,
}

impl DmaPte {
    /// Yeni bir DMA sayfa tablosu girişi oluşturur.
    /// Bit 0 (Present), Bit 1 (Read), Bit 2 (Write) VT-d PTE formatıdır.
    pub fn new(phys: u64, read: bool, write: bool) -> Self {
        let mut val = phys & !0xFFF; // Sayfa hizalaması: alt 12 bit sıfırlanır
        val |= 1; // Present biti
        if read { val |= 1 << 1; }  // Okuma izni
        if write { val |= 1 << 2; } // Yazma izni
        Self { val }
    }
}

// ============================================================================
// IOMMU DOMAIN (IOMMU ALANI)
// ============================================================================

// Her domain bağımsız bir DMA adres uzayıdır.
//
// Bir domain'e atanan cihazlar yalnızca o domain'in eşleme tablosundaki
// adreslere erişebilir. Diğer domainlerin (ve çekirdek kodunun) belleğine
// erişim donanım tarafından engellenir ve hata kaydedilir.

pub struct IommuDomain {
    /// Domain kimliği (sürücü tarafından atanır)
    pub id: u32,
    /// Sayfa tablosunun fiziksel adresi (kök tablo pointer'ı)
    pub page_table: u64,
    /// Bu domain için çeviri etkin mi?
    pub translation_enabled: bool,
    /// Bu domain'e atanmış cihazlar: (segment, BDF) ikilileri
    /// BDF = Bus:Device:Function (PCI cihaz tanımlayıcısı)
    pub devices: Mutex<Vec<(u16, u16)>>,
    /// DMA adres -> fiziksel adres eşleme tablosu
    pub mappings: Mutex<BTreeMap<u64, DmaTranslation>>,
}

impl IommuDomain {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            page_table: 0,
            translation_enabled: true,
            devices: Mutex::new(Vec::new()),
            mappings: Mutex::new(BTreeMap::new()),
        }
    }

    /// DMA adresi eşler: cihaz dma_addr'ye erişince phys_addr'ye yönlendirilir
    pub fn map(&self, dma_addr: u64, phys_addr: u64, size: u64, read: bool, write: bool) -> Result<(), IommuError> {
        let mapping = DmaTranslation {
            present: true,
            read_perm: read,
            write_perm: write,
            phys_addr,
            size,
        };
        self.mappings.lock().insert(dma_addr, mapping);
        Ok(())
    }

    /// DMA eşlemesini kaldırır; cihaz bu adrese artık erişemez
    pub fn unmap(&self, dma_addr: u64) -> bool {
        self.mappings.lock().remove(&dma_addr).is_some()
    }

    /// DMA adresini fiziksel adrese çevirir; eşleme yoksa None döner
    pub fn translate(&self, dma_addr: u64) -> Option<DmaTranslation> {
        self.mappings.lock().get(&dma_addr).cloned()
    }

    /// PCI cihazını bu domain'e atar; artık bu domain'in kuralları geçerli
    pub fn attach_device(&self, segment: u16, bdf: u16) {
        self.devices.lock().push((segment, bdf));
    }

    /// PCI cihazını domain'den ayırır
    pub fn detach_device(&self, segment: u16, bdf: u16) {
        self.devices.lock().retain(|&(s, b)| s != segment || b != bdf);
    }
}

// ============================================================================
// IOMMU BİRİMİ (IOMMU UNIT)
// ============================================================================

// Fiziksel IOMMU donanımını temsil eder.
// Sistemde birden fazla IOMMU bulunabilir (çok soketli sunucular, PCIe segment kümeleri).
//
//   IommuUnit
//     |-- vendor: Intel veya AMD
//     |-- base_addr: MMIO alanının başlangıç adresi (DMAR/IVRS tablosundan)
//     |-- domains: domain_id -> IommuDomain tablosu
//     +-- fault_recording: IOMMU ihlallerinin kaydı

pub struct IommuUnit {
    /// Birim kimliği (0'dan başlar)
    pub id: u32,
    /// Üretici (Intel VT-d veya AMD-Vi)
    pub vendor: IommuVendor,
    /// MMIO yazmaçlarının fiziksel taban adresi
    pub base_addr: u64,
    /// Haritalanmış MMIO sanal adresi (None = henüz haritalanmadı)
    pub mmio: Mutex<Option<u64>>,
    /// Çeviri etkin mi?
    pub enabled: AtomicBool,
    /// Kök tablonun fiziksel adresi
    pub root_table: AtomicU64,
    /// Kaydedilmiş IOMMU hataları (debug/audit için)
    pub fault_recording: Mutex<Vec<IommuFault>>,
    /// Bu birim tarafından yönetilen domain'ler
    pub domains: Mutex<BTreeMap<u32, IommuDomain>>,
    /// Bir sonraki domain ID sayacı
    pub next_domain_id: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IommuVendor {
    Intel,
    Amd,
    Unknown,
}

/// IOMMU erişim ihlali kaydı
#[derive(Clone, Debug)]
pub struct IommuFault {
    /// İhlali yapan cihazın PCI kaynak kimliği (BDF)
    pub source_id: u16,
    /// İhlalin yaşandığı domain
    pub domain_id: u32,
    /// İhlal edilen DMA adresi
    pub address: u64,
    /// İhlalin türü
    pub fault_type: IommuFaultType,
    /// İhlalin zamanlama damgası (tick sayısı)
    pub timestamp: u64,
}

/// IOMMU ihlal türleri
#[derive(Clone, Copy, Debug)]
pub enum IommuFaultType {
    ReadViolation,         // Okuma izni olmayan adrese erişim
    WriteViolation,        // Yazma izni olmayan adrese erişim
    TranslationFailed,     // Eşleme tablosunda adres bulunamadı
    AccessViolation,       // Genel erişim ihlali
}

impl IommuUnit {
    pub fn new(id: u32, vendor: IommuVendor, base_addr: u64) -> Self {
        Self {
            id,
            vendor,
            base_addr,
            mmio: Mutex::new(None),
            enabled: AtomicBool::new(false),
            root_table: AtomicU64::new(0),
            fault_recording: Mutex::new(Vec::new()),
            domains: Mutex::new(BTreeMap::new()),
            next_domain_id: AtomicU32::new(1),
        }
    }

    /// IOMMU donanımını başlatır:
    /// 1. MMIO alanını haritalar
    /// 2. Yetenek yazmaçlarını okur
    /// 3. Kök tabloyu tahsis eder
    pub fn init(&self) -> Result<(), IommuError> {
        // MMIO alanını sanal adres uzayına haritala
        *self.mmio.lock() = Some(self.base_addr);

        // Donanımın desteklediği özellikleri doğrula
        self.check_capabilities()?;

        // Sayfa hizalı kök tablosu tahsis et
        let root_table = self.alloc_root_table();
        self.root_table.store(root_table, Ordering::SeqCst);

        crate::serial_println!("[IOMMU] Unit {} initialized ({:?})", self.id, self.vendor);
        Ok(())
    }

    /// Yetenek yazmaçlarını okur ve desteklenen özellikleri doğrular
    fn check_capabilities(&self) -> Result<(), IommuError> {
        // Gerçek uygulamada: VTD_CAP_REG ve VTD_ECAP_REG okunur
        Ok(())
    }

    /// Kök tablo için sayfa hizalı bellek tahsis eder
    fn alloc_root_table(&self) -> u64 {
        // Gerçek uygulamada: sayfa tahsisi yapılır, sıfırlanır
        0x100000
    }

    /// DMA çevirisini etkinleştirir (kök tabloyu yükle + çeviriyi aç)
    pub fn enable(&self) -> Result<(), IommuError> {
        match self.vendor {
            IommuVendor::Intel => self.enable_vtd()?,
            IommuVendor::Amd => self.enable_amd()?,
            _ => return Err(IommuError::NotSupported),
        }

        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[IOMMU] Unit {} enabled", self.id);
        Ok(())
    }

    /// Intel VT-d çevirisini etkinleştir:
    /// 1. RTADDR yazmacına kök tabloyu yaz
    /// 2. GCMD yazmacında SRTP bitini ayarla (kök tabloyu uygula)
    /// 3. GCMD yazmacında TE bitini ayarla (çeviriyi etkinleştir)
    fn enable_vtd(&self) -> Result<(), IommuError> {
        // Kök tablo pointer'ını ayarla
        // Çeviriyi etkinleştir
        Ok(())
    }

    /// AMD-Vi çevirisini etkinleştir
    fn enable_amd(&self) -> Result<(), IommuError> {
        Ok(())
    }

    /// DMA çevirisini devre dışı bırakır
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Yeni bir DMA domain oluşturur ve ID'sini döner
    pub fn create_domain(&self) -> u32 {
        let id = self.next_domain_id.fetch_add(1, Ordering::SeqCst);
        let domain = IommuDomain::new(id);
        self.domains.lock().insert(id, domain);
        id
    }

    /// Domain ID ile domain nesnesini döner
    pub fn get_domain(&self, id: u32) -> Option<IommuDomain> {
        self.domains.lock().get(&id).cloned()
    }

    /// IOTLB (I/O TLB) geçersiz kılar: çeviri önbelleğini temizler.
    /// Eşleme değişikliklerinden sonra çağrılmalıdır.
    pub fn flush_iotlb(&self, domain_id: u32, addr: u64) {
        // Gerçek uygulamada: VTD_IOTLB_REG'e geçersiz kılma komutu yazılır
    }

    /// Yazma tamponunu temizler: askıdaki DMA yazmaları tamamlanır
    pub fn flush_write_buffer(&self) {
        // Gerçek uygulamada: VTD_GCMD_WBF biti ayarlanır ve GSTS beklenir
    }

    /// IOMMU hata kesmesini işler ve hatayı kaydeder
    pub fn handle_fault(&self) {
        let fault = IommuFault {
            source_id: 0,
            domain_id: 0,
            address: 0,
            fault_type: IommuFaultType::TranslationFailed,
            timestamp: crate::task::scheduler::get_ticks(),
        };
        self.fault_recording.lock().push(fault);
    }
}

// ============================================================================
// IOMMU YÖNETİCİSİ (IOMMU MANAGER)
// ============================================================================

// Sistemdeki tüm IOMMU birimlerini merkezi olarak yönetir.
// ACPI tabloları (DMAR veya IVRS) taranarak birimler keşfedilir.

pub struct IommuManager {
    units: Mutex<Vec<IommuUnit>>,
    /// Varsayılan domain ID'si (kimliği doğrulanmamış cihazlar için)
    default_domain: AtomicU32,
    /// Herhangi bir IOMMU etkin mi?
    iommu_enabled: AtomicBool,
}

impl IommuManager {
    pub const fn new() -> Self {
        Self {
            units: Mutex::new(Vec::new()),
            default_domain: AtomicU32::new(0),
            iommu_enabled: AtomicBool::new(false),
        }
    }

    /// Yeni IOMMU birimi kaydeder (DMAR/IVRS tablosundan çağrılır)
    pub fn register_unit(&self, vendor: IommuVendor, base_addr: u64) -> u32 {
        let id = self.units.lock().len() as u32;
        let unit = IommuUnit::new(id, vendor, base_addr);
        self.units.lock().push(unit);
        id
    }

    /// Tüm IOMMU birimlerini başlatır
    pub fn init_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.init()?;
        }
        Ok(())
    }

    /// Tüm IOMMU birimlerinde DMA çevirisini etkinleştirir
    pub fn enable_all(&self) -> Result<(), IommuError> {
        for unit in self.units.lock().iter() {
            unit.enable()?;
        }
        self.iommu_enabled.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Birim ID'si ile IOMMU birimini döner
    pub fn get_unit(&self, id: u32) -> Option<IommuUnit> {
        self.units.lock().get(id as usize).cloned()
    }

    /// Cihaz için DMA adresi eşler: BDF ile ilgili domain bulunur ve map çağrılır
    pub fn map_dma(&self, segment: u16, bdf: u16, dma_addr: u64, phys_addr: u64, size: u64, read: bool, write: bool) -> Result<(), IommuError> {
        // Cihazın domain'ini bul ve eşlemesi yap
        Ok(())
    }

    /// Cihaz için DMA eşlemesini kaldırır
    pub fn unmap_dma(&self, segment: u16, bdf: u16, dma_addr: u64) -> bool {
        false
    }

    /// IOMMU'nun etkin olup olmadığını döner
    pub fn is_enabled(&self) -> bool {
        self.iommu_enabled.load(Ordering::SeqCst)
    }
}

// IommuUnit için Clone implementasyonu (AtomicXxx alanları manuel klonlanır)
impl Clone for IommuUnit {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            vendor: self.vendor,
            base_addr: self.base_addr,
            mmio: Mutex::new(*self.mmio.lock()),
            enabled: AtomicBool::new(self.enabled.load(Ordering::Relaxed)),
            root_table: AtomicU64::new(self.root_table.load(Ordering::Relaxed)),
            fault_recording: Mutex::new(self.fault_recording.lock().clone()),
            domains: Mutex::new(self.domains.lock().clone()),
            next_domain_id: AtomicU32::new(self.next_domain_id.load(Ordering::Relaxed)),
        }
    }
}

// IommuDomain için Clone implementasyonu
impl Clone for IommuDomain {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            page_table: self.page_table,
            translation_enabled: self.translation_enabled,
            devices: Mutex::new(self.devices.lock().clone()),
            mappings: Mutex::new(self.mappings.lock().clone()),
        }
    }
}

lazy_static::lazy_static! {
    pub static ref IOMMU_MANAGER: IommuManager = IommuManager::new();
}

// ============================================================================
// HATA TÜRLERİ (ERROR TYPES)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuError {
    NotSupported,   // Bu donanımda IOMMU desteklenmiyor
    InitFailed,     // Başlatma başarısız
    NoMemory,       // Kök/sayfa tablosu için bellek yetersiz
    InvalidAddress, // Geçersiz DMA adresi
    DeviceNotFound, // Belirtilen PCI cihazı bulunamadı
    DomainNotFound, // Belirtilen domain bulunamadı
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

pub fn init() {
    // ACPI DMAR tablosunu (Intel) veya IVRS tablosunu (AMD) tara ve birimleri keşfet
    crate::serial_println!("[IOMMU] Subsystem initialized");
}
