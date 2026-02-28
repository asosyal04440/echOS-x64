//! # NVMe Sürücüsü (Non-Volatile Memory Express)
//!
//! NVMe, PCIe veri yoluna doğrudan bağlanan SSD'ler için geliştirilmiş
//! depolama arayüzüdür. Geleneksel ATA/AHCI'ye göre çok daha düşük gecikme
//! ve çok daha yüksek bant genişliği sunar.
//!
//! ## NVMe Mimarisi
//!
//! ```
//!   CPU                            NVMe SSD
//!    |                               |
//!    |---[PCIe x4 Gen4]-------->-----|
//!    |           |                   |
//!    |     [MMIO Registers]    [Flash Controller]
//!    |           |                   |
//!   RAM      [Submission   [NAND Flash Array]
//!              Queues]
//!              [Completion
//!              Queues]
//! ```
//!
//! ## Submission / Completion Kuyrukları
//!
//! NVMe, asenkron kuyruk tabanlı I/O modeli kullanır:
//!
//! ```
//!   CPU -> SQ (Submission Queue) : Komut yazar
//!   CPU -> SQ Doorbell           : "Yeni komut var" zili çalar
//!   NVMe                         : Komutu işler
//!   NVMe -> CQ (Completion Queue): Tamamlama girişi yazar + IRQ gönderir
//!   CPU                          : CQ'yu okur, sonucu alır
//!   CPU -> CQ Doorbell           : "Tamamlamayı gördüm" zili çalar
//! ```
//!
//! ## Namespace Kavramı
//!
//! NVMe, depolama alanını "namespace"lara böler. Her namespace bağımsız bir
//! blok cihazı gibi davranır. nsid=1 genellikle varsayılan ana depolama alanı.
//!
//! ## Admin vs I/O Kuyrukları
//!
//! - Admin Queue (sqid=0, cqid=0): Identify, Create/Delete Queue gibi yönetim komutları
//! - I/O Queues  (sqid>=1):        Read/Write/Flush veri işlemleri
//!
//! Her CPU çekirdeği için ayrı bir I/O kuyruğu oluşturularak çekişme önlenir.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::mem;
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

// ============================================================================
// NVMe SABİTLERİ (CONSTANTS)
// ============================================================================

// PCI sınıf kodları: NVMe'yi diğer depolama kontrolörlerinden ayırır
// class_code=0x01 (Storage), subclass=0x08 (NVM Express)
const PCI_CLASS_STORAGE: u8 = 0x01;   // Depolama kontrolörü
const PCI_SUBCLASS_NVME: u8 = 0x08;   // NVM Express alt sınıfı

// NVMe denetleyici MMIO yazmaç ofseti haritası.
// BAR0'dan okunan fiziksel adrese bu ofsetler eklenerek yazmaçlara erişilir.
// Kaynak: NVM Express Base Specification Revision 1.4

const NVME_CAP: usize = 0x00;    // Controller Capabilities: max queue boyutu, timeout vb.
const NVME_VS: usize = 0x08;     // Version: NVMe spec versiyonu (Major.Minor.Tertiary)
const NVME_INTMS: usize = 0x0C;  // Interrupt Mask Set: IRQ maskesi ekle
const NVME_INTMC: usize = 0x10;  // Interrupt Mask Clear: IRQ maskesi kaldır
const NVME_CC: usize = 0x14;     // Controller Configuration: etkinleştir, kuyruk boyutları
const NVME_CSTS: usize = 0x1C;   // Controller Status: RDY, CFS, SHST
const NVME_NSSR: usize = 0x20;   // NVM Subsystem Reset: 0x4E564D65 yazarak sıfırla
const NVME_AQA: usize = 0x24;    // Admin Queue Attributes: admin SQ/CQ boyutları
const NVME_ASQ: usize = 0x28;    // Admin Submission Queue Base Address (fiziksel)
const NVME_ACQ: usize = 0x30;    // Admin Completion Queue Base Address (fiziksel)

// CAP yazmacı bit alanları (Controller Capabilities Register)
// Bit        Bit Sayısı  Açıklama
// 0-15       16          MQES: Desteklenen maksimum kuyruk girişi (0-base, +1 gerekir)
// 16         1           CQR:  Fiziksel bitişik kuyruk zorunlu mu?
// 17-18      2           AMS:  Desteklenen arbitrasyon mekanizmaları bitmask
// 24-31      8           TO:   Hazır olma zaman aşımı (500ms biriminde)
// 32-35      4           DSTRD: Zil kapı aralığı (4<<DSTRD byte)
// 33         1           NSSRS: NVM Subsystem Reset destekleniyor mu?
// 37-44      8           CSS:  Desteklenen komut setleri (bit 0 = NVM Command Set)
// 48-51      4           MPSMIN: Minimum sayfa boyutu (2^(12+MPSMIN))
// 52-55      4           MPSMAX: Maksimum sayfa boyutu

const CAP_MQES_SHIFT: u64 = 0;    // Max Queue Entries Supported
const CAP_CQR_SHIFT: u64 = 16;    // Contiguous Queues Required
const CAP_AMS_SHIFT: u64 = 17;    // Arbitration Mechanisms Supported
const CAP_TO_SHIFT: u64 = 24;     // Timeout
const CAP_DSTRD_SHIFT: u64 = 32;  // Doorbell Stride
const CAP_NSSRS_SHIFT: u64 = 33;  // NVM Subsystem Reset Supported
const CAP_CSS_SHIFT: u64 = 37;    // Command Sets Supported
const CAP_MPSMIN_SHIFT: u64 = 48; // Memory Page Size Minimum
const CAP_MPSMAX_SHIFT: u64 = 52; // Memory Page Size Maximum

// CC (Controller Configuration) yazmaç bitleri
const CC_EN: u32 = 0x00000001;       // Enable: denetleyiciyi etkinleştir; CSTS.RDY=1 bekle
const CC_CSS_SHIFT: u32 = 4;         // Command Set Selected (0=NVM, 6=Admin Only, 7=I/O CS)
const CC_MPS_SHIFT: u32 = 7;          // Memory Page Size (0 = 4KB = 2^(12+0))
const CC_AMS_SHIFT: u32 = 11;        // Arbitration Mechanism Selected (0=Round Robin)
const CC_SHN_SHIFT: u32 = 14;        // Shutdown Notification (1=Normal, 2=Abrupt)
const CC_IOSQES_SHIFT: u32 = 16;     // I/O SQ Entry Size (2^N byte; 6=64B)
const CC_IOCQES_SHIFT: u32 = 20;     // I/O CQ Entry Size (2^N byte; 4=16B)

// CSTS (Controller Status) yazmaç bitleri
const CSTS_RDY: u32 = 0x00000001;    // Ready: denetleyici komutlara hazır
const CSTS_CFS: u32 = 0x00000002;    // Controller Fatal Status: kritik hata
const CSTS_SHST_SHIFT: u32 = 2;      // Shutdown Status (0=normal, 1=hazırlanıyor, 2=tamamlandı)
const CSTS_NSSRO: u32 = 0x00000008;  // NVM Subsystem Reset Occurred

// NVM Komut Opcode'ları (I/O Queue için)
const OP_FLUSH: u8 = 0x00;                // Volatile önbelleği kalıcı depolamaya yaz
const OP_WRITE: u8 = 0x01;               // LBA'ya veri yaz
const OP_READ: u8 = 0x02;                // LBA'dan veri oku
const OP_WRITE_UNCORRECTABLE: u8 = 0x04; // LBA'yı hatalı olarak işaretle
const OP_COMPARE: u8 = 0x05;             // LBA ile tamponu karşılaştır
const OP_WRITE_ZEROES: u8 = 0x08;        // LBA aralığını sıfırla (donanım hızlandırmalı)
const OP_DATASET_MANAGEMENT: u8 = 0x09;  // TRIM/Discard: boşaltılmış LBA'ları SSD'ye bildir

// Admin Komut Opcode'ları (Admin Queue için)
const OP_ADMIN_DELETE_SQ: u8 = 0x00;     // Submission Queue sil
const OP_ADMIN_CREATE_SQ: u8 = 0x01;     // Submission Queue oluştur
const OP_ADMIN_GET_LOG_PAGE: u8 = 0x02;  // Log sayfasını oku (sağlık, hata, FW istatistikleri)
const OP_ADMIN_DELETE_CQ: u8 = 0x04;     // Completion Queue sil
const OP_ADMIN_CREATE_CQ: u8 = 0x05;     // Completion Queue oluştur
const OP_ADMIN_IDENTIFY: u8 = 0x06;      // Denetleyici/namespace tanımlama verisi al
const OP_ADMIN_SET_FEATURES: u8 = 0x09;  // Özellik ayarla (power, arbitration, vb.)
const OP_ADMIN_GET_FEATURES: u8 = 0x0A;  // Özellik oku
const OP_ADMIN_ASYNC_EVENT: u8 = 0x0C;   // Asenkron olayları kayıt et (health notification)

// Kuyruk boyutları: Admin küçük, I/O büyük tercih edilir
const ADMIN_QUEUE_SIZE: u16 = 32;  // Admin: 32 giriş yeterli (yönetim komutları nadir)
const IO_QUEUE_SIZE: u16 = 256;    // I/O: 256 giriş paralel işlem için

/// Sistem sayfa boyutu (4 KB)
const PAGE_SIZE: usize = 4096;

// ============================================================================
// HATA TÜRLERİ (ERROR TYPES)
// ============================================================================

/// NVMe işlemlerinde dönebilecek hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvmeError {
    NoController,        // Sistemde NVMe denetleyicisi bulunamadı
    ControllerError,     // CSTS.CFS=1: denetleyici kritik hata bildirdi
    Timeout,             // Komut zaman aşımına uğradı (CSTS.RDY beklenirken)
    QueueFull,           // Kuyruk dolu: yeni komut eklenemez
    InvalidNamespace,    // Geçersiz namespace ID (nsid=0 veya aralık dışı)
    DataTransferError,   // DMA/PRP buffer tahsisi hatası
    NotReady,            // Denetleyici henüz hazır değil (init() çağrılmadı)
    FeatureNotSupported, // MSI/MSI-X veya istenen özellik desteklenmiyor
}

// ============================================================================
// DENETLEYİCİ YETENEKLERİ (CONTROLLER CAPABILITIES)
// ============================================================================

// NvmeCapabilities, başlatma sırasında CAP yazmacından bir kez okunur
// ve denetleyicinin desteklediği sınırları tanımlar.

#[derive(Clone, Copy, Debug)]
pub struct NvmeCapabilities {
    pub max_queue_entries: u16,     // Maksimum kuyruk giriş sayısı
    pub contiguous_queues: bool,    // Kuyruklar fiziksel olarak bitişik olmalı mı?
    pub arbitration_mechanisms: u8, // Desteklenen arbitrasyon bitmask (0=RR, 1=WRR, 2=vendor)
    pub timeout_ms: u16,            // Hazır olma zaman aşımı (milisaniye)
    pub doorbell_stride: u16,       // Zil kapı yazmacı aralığı (byte)
    pub nvm_subsystem_reset: bool,  // NVM Subsystem Reset (NSSR) destekleniyor mu?
    pub command_sets: u8,           // Desteklenen komut setleri bitmask
    pub page_size_min: u8,          // Minimum desteklenen sayfa boyutu (2^(12+n))
    pub page_size_max: u8,          // Maksimum desteklenen sayfa boyutu
}

impl NvmeCapabilities {
    /// CAP yazmacından (64-bit) yetenek alanlarını çıkarır
    pub fn parse(cap: u64) -> Self {
        NvmeCapabilities {
            max_queue_entries: ((cap >> CAP_MQES_SHIFT) & 0xFFFF) as u16 + 1,  // 0-base'den 1-base'e
            contiguous_queues: ((cap >> CAP_CQR_SHIFT) & 1) != 0,
            arbitration_mechanisms: ((cap >> CAP_AMS_SHIFT) & 0x7) as u8,
            timeout_ms: ((cap >> CAP_TO_SHIFT) & 0xFF) as u16 * 500,  // 500ms biriminde
            doorbell_stride: (4 << ((cap >> CAP_DSTRD_SHIFT) & 0xF)) as u16,  // 4<<DSTRD byte
            nvm_subsystem_reset: ((cap >> CAP_NSSRS_SHIFT) & 1) != 0,
            command_sets: ((cap >> CAP_CSS_SHIFT) & 0xFF) as u8,
            page_size_min: ((cap >> CAP_MPSMIN_SHIFT) & 0xF) as u8,
            page_size_max: ((cap >> CAP_MPSMAX_SHIFT) & 0xF) as u8,
        }
    }
}

// ============================================================================
// TANIMLAMA VERİSİ (IDENTIFY DATA)
// ============================================================================

// Admin Identify komutu iki türde bilgi döner:
//   CNS=0: Namespace bilgisi (boyut, LBA formatı)
//   CNS=1: Denetleyici bilgisi (model, seri, firmware, yetenek)
//
// Her ikisi de 4096 byte'lık tampon gerektirir ve struct layout C uyumlu olmalıdır.

/// NVMe Identify Controller yapısı (Admin Identify CNS=1)
/// Denetleyicinin model, seri, firmware bilgilerini ve yeteneklerini içerir.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeIdentifyController {
    pub vid: u16,               // PCI Üretici ID (örn. 0x144D Samsung)
    pub ssvid: u16,             // PCI Alt Sistem Üretici ID
    pub serial: [u8; 20],       // Seri numarası (ASCII, boşlukla doldurulmuş)
    pub model: [u8; 40],        // Model numarası (örn. "Samsung SSD 980 PRO")
    pub firmware: [u8; 8],      // Firmware sürümü
    pub rab: u8,                // Önerilen arbitrasyon patlama (Recommended Arbitration Burst)
    pub ieee: [u8; 3],          // IEEE OUI tanımlayıcısı (üretici kodu)
    pub cmic: u8,               // Çok yollu I/O yetenekleri (multi-path capable)
    pub mdts: u8,               // Maksimum veri transfer boyutu (2^MDTS * MPS)
    pub cntlid: u16,            // Denetleyici ID (çok denetleyicili konfigürasyonlarda)
    pub ver: u32,               // NVMe spec versiyonu (bit31-16=Major, 15-8=Minor, 7-0=Tertiary)
    pub rtd3r: u32,             // RTD3 Devam Gecikmesi (microsaniye)
    pub rtd3e: u32,             // RTD3 Giriş Gecikmesi (microsaniye)
    pub oaes: u32,              // Desteklenen isteğe bağlı asenkron olaylar
    pub ctratt: u32,            // Denetleyici özellikleri
    pub rrls: u16,              // Desteklenen okuma kurtarma seviyeleri
    pub cntrltype: u8,          // Denetleyici türü (I/O, Discovery, Admin)
    pub fguid: [u8; 16],        // FRU GUID (küresel benzersiz tanımlayıcı)
    pub crdt1: u16,             // Komut Yeniden Deneme Gecikmesi 1 (100ms biriminde)
    pub crdt2: u16,             // Komut Yeniden Deneme Gecikmesi 2
    pub crdt3: u16,             // Komut Yeniden Deneme Gecikmesi 3
    pub oacs: u16,              // Desteklenen isteğe bağlı admin komutları bitmask
    pub acl: u8,                // İptal komutu limiti (en fazla kaç bekleyen abort olabilir)
    pub aerl: u8,               // Asenkron olay isteği limiti
    pub frmw: u8,               // Firmware güncelleme özellikleri
    pub lpa: u8,                // Log sayfası özellikleri
    pub elpe: u8,               // Hata log sayfası giriş sayısı
    pub npss: u8,               // Desteklenen güç durumu sayısı
    pub avscc: u8,              // Admin satıcıya özgü komut yapılandırması
    pub apsta: u8,              // Otomatik güç durumu geçişi yetenekleri
    pub wctemp: u16,            // Uyarı bileşik sıcaklık eşiği (Kelvin)
    pub cctemp: u16,            // Kritik bileşik sıcaklık eşiği (Kelvin)
    pub mtfa: u16,              // Firmware aktivasyonu için maksimum süre (100ms)
    pub hmpre: u32,             // Tercih edilen ana bellek tamponu boyutu (4KB)
    pub hmmin: u32,             // Minimum ana bellek tamponu boyutu (4KB)
    pub tnvmcap: [u8; 16],      // Toplam NVM kapasitesi (128-bit, byte cinsinden)
    pub unvmcap: [u8; 16],      // Tahsis edilmemiş NVM kapasitesi
    pub rpmbs: u32,             // RPMB desteği (replay protected memory block)
    pub edstt: u16,             // Genişletilmiş cihaz öz test süresi
    pub dsto: u8,               // Cihaz öz test seçenekleri
    pub fwug: u8,               // Firmware güncelleme tanecikliği (4KB biriminde)
    pub kas: u16,               // Keep Alive desteği (timeout periyodu, 100ms)
    pub hctma: u16,             // Isı yönetimi özellikleri
    pub mntmt: u16,             // Minimum ısı yönetimi sıcaklığı (Kelvin)
    pub mxtmt: u16,             // Maksimum ısı yönetimi sıcaklığı (Kelvin)
    pub sanicap: u32,           // Temizleme (sanitize) yetenekleri
    pub hmminds: u32,           // Ana bellek tamponu minimum tanımlayıcı giriş boyutu
    pub hmmaxd: u16,            // Ana bellek tamponu maksimum tanımlayıcı girişi
    pub nsetidmax: u16,         // NVM seti tanımlayıcısı maksimumu
    pub endgidmax: u16,         // Dayanıklılık grubu tanımlayıcısı maksimumu
    pub anatt: u8,              // ANA geçiş süresi
    pub anacap: u8,             // Asimetrik namespace erişim yetenekleri
    pub anagrpmax: u32,         // ANA grup tanımlayıcısı maksimumu
    pub nanagrpid: u32,         // ANA grup tanımlayıcısı sayısı
    pub sqes: u8,               // Gönderme kuyruğu giriş boyutu (alt nibble: min, üst: maks)
    pub cqes: u8,               // Tamamlama kuyruğu giriş boyutu
    pub maxcmd: u16,            // Maksimum bekleyen komut sayısı
    pub nn: u32,                // Namespace sayısı
    pub oncs: u16,              // Desteklenen isteğe bağlı NVM komutları
    pub fuses: u16,             // Birleşik işlem desteği
    pub fna: u8,                // Format NVM özellikleri
    pub vwc: u8,                // Uçucu yazma önbelleği (bit 0 = destekli)
    pub awun: u16,              // Normal koşullarda atomik yazma birimi (0-base, blok sayısı)
    pub awupf: u16,             // Güç kesintisinde atomik yazma birimi
    pub nvscc: u8,              // NVM satıcıya özgü komut yapılandırması
    pub nwpc: u8,               // Namespace yazma koruma yetenekleri
    pub acwu: u16,              // Atomik karşılaştırma ve yazma birimi
    pub sgls: u32,              // Scatter/Gather List desteği
    pub mnan: u32,              // İzin verilen maksimum namespace sayısı
}

impl NvmeIdentifyController {
    /// Seri numarasını temizlenmiş UTF-8 string olarak döner
    pub fn get_serial(&self) -> String {
        String::from_utf8_lossy(&self.serial).trim().to_string()
    }

    /// Model numarasını temizlenmiş UTF-8 string olarak döner
    pub fn get_model(&self) -> String {
        String::from_utf8_lossy(&self.model).trim().to_string()
    }

    /// Firmware revizyonunu temizlenmiş UTF-8 string olarak döner
    pub fn get_firmware(&self) -> String {
        String::from_utf8_lossy(&self.firmware).trim().to_string()
    }

    /// Maksimum gönderme kuyruğu giriş boyutunu döner (byte)
    pub fn get_max_submission_queue_entry_size(&self) -> u8 {
        1 << (self.sqes & 0xF) // Alt nibble: minimum desteklenen boyut
    }

    /// Maksimum tamamlama kuyruğu giriş boyutunu döner (byte)
    pub fn get_max_completion_queue_entry_size(&self) -> u8 {
        1 << (self.cqes & 0xF)
    }

    /// Toplam namespace sayısını döner
    pub fn get_namespace_count(&self) -> u32 {
        self.nn
    }
}

/// NVMe Identify Namespace yapısı (Admin Identify CNS=0)
/// Belirli bir namespace'in boyutunu ve LBA format bilgilerini içerir.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeIdentifyNamespace {
    pub nsze: u64,              // Namespace boyutu (LBA sayısı; toplam depolama kapasitesi)
    pub ncap: u64,              // Namespace kapasitesi (kullanılabilir maksimum LBA)
    pub nuse: u64,              // Namespace kullanımı (şu an kullanılan LBA sayısı)
    pub nsfeat: u8,             // Namespace özellikleri (thin provisioning, atomicity...)
    pub nlbaf: u8,              // LBA formatı sayısı (0-base; +1 = gerçek sayı)
    pub flbas: u8,              // Biçimlendirilmiş LBA boyutu (aktif format indeksi)
    pub mc: u8,                 // Metadata yetenekleri
    pub dpc: u8,                // Uçtan uca veri koruma yetenekleri
    pub dps: u8,                // Veri koruma türü ayarları
    pub nmic: u8,               // Çok yollu I/O yetenekleri
    pub rescap: u8,             // Rezervasyon yetenekleri
    pub fpi: u8,                // Format ilerleme göstergesi
    pub nsattr: u8,             // Namespace özellikleri (yazma korumalı, vb.)
    pub nvmsetid: u16,          // NVM seti tanımlayıcısı
    pub endgid: u16,            // Dayanıklılık grubu tanımlayıcısı
    pub nguid: [u8; 16],        // Namespace küresel benzersiz tanımlayıcısı
    pub eui64: [u8; 8],         // IEEE Genişletilmiş Benzersiz Tanımlayıcı
    pub lbaf: [LbaFormat; 16],  // Desteklenen LBA formatları dizisi (16 olası format)
    pub vs: [u8; 3712],         // Satıcıya özgü alanlar (dolgu)
}

/// LBA Format yapısı: blok boyutu ve performans bilgisi
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LbaFormat {
    pub ms: u16,    // Metadata boyutu (byte; genellikle 0)
    pub lbads: u8,  // LBA veri boyutu (2^LBADS byte; 9=512B, 12=4096B)
    pub rp: u8,     // Göreceli performans (0=en iyi, 3=en kötü)
}

impl NvmeIdentifyNamespace {
    /// Aktif LBA formatından blok boyutunu döner (byte)
    pub fn get_block_size(&self) -> u32 {
        let lbaf_index = (self.flbas & 0xF) as usize; // Alt 4 bit: aktif format indeksi
        if lbaf_index < self.lbaf.len() {
            1u32 << self.lbaf[lbaf_index].lbads // 2^LBADS: genellikle 512 veya 4096
        } else {
            512 // Varsayılan: 512 byte
        }
    }

    /// Toplam blok sayısını döner
    pub fn get_block_count(&self) -> u64 {
        self.nsze // nsze: namespace size in LBAs (0-indexed capacity)
    }

    /// Toplam kapasiteyi byte cinsinden döner
    pub fn get_capacity_bytes(&self) -> u64 {
        self.get_block_count() * self.get_block_size() as u64
    }
}

// ============================================================================
// GÖNDERME KUYRUĞU GİRİŞİ (SUBMISSION QUEUE ENTRY / SQE)
// ============================================================================

// Her NVMe komutu 64 byte'lık Submission Queue Entry (SQE) olarak temsil edilir.
// Yapı sabit: ilk 4 DWord ortak (header), kalan DWord'lar komuta özgü.
//
// SQE Düzeni:
//   DW0: opcode(8) + flags(8) + cid(16)
//   DW1: nsid
//   DW2-3: cdw2-3 (komuta özgü)
//   DW4-5: mptr (metadata pointer; 128-bit)
//   DW6-7: prp1 (Physical Region Page 1; DMA tampon adresi)
//   DW8-9: prp2 (Physical Region Page 2; büyük tamponlar için)
//   DW10-15: cdw10-15 (komuta özgü: LBA, blok sayısı, CNS, vb.)

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCommand {
    pub opcode: u8,    // Komut kodu (OP_READ, OP_WRITE, OP_ADMIN_IDENTIFY...)
    pub flags: u8,     // Komut bayrakları (PRINFO, EF, ...)
    pub cid: u16,      // Command ID: tamamlama eşleştirmesi için benzersiz
    pub nsid: u32,     // Namespace ID (admin komutları için 0)
    pub cdw2: u32,     // Komuta özgü word 2
    pub cdw3: u32,     // Komuta özgü word 3
    pub mptr: u64,     // Metadata pointer (genellikle 0)
    pub prp1: u64,     // PRP1: veri tamponunun fiziksel adresi
    pub prp2: u64,     // PRP2: tampon sayfa sınırını aşıyorsa sonraki sayfa adresi
    pub cdw10: u32,    // Komuta özgü: okuma/yazmada LBA[31:0]
    pub cdw11: u32,    // Komuta özgü: LBA[63:32]
    pub cdw12: u32,    // Komuta özgü: blok sayısı (0-base, NLBA-1)
    pub cdw13: u32,    // Komuta özgü
    pub cdw14: u32,    // Komuta özgü
    pub cdw15: u32,    // Komuta özgü
}

impl NvmeCommand {
    /// Temel komut yapısını sıfır doldurarak oluşturur
    pub fn new(opcode: u8, cid: u16, nsid: u32) -> Self {
        NvmeCommand {
            opcode,
            flags: 0,
            cid,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp1: 0,
            prp2: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// Okuma komutu (OP_READ) oluşturur
    /// lba: başlangıç sektörü, blocks: okunacak sektör sayısı
    pub fn read(cid: u16, nsid: u32, lba: u64, blocks: u16) -> Self {
        let mut cmd = Self::new(OP_READ, cid, nsid);
        cmd.cdw10 = lba as u32;         // LBA'nın alt 32 biti
        cmd.cdw11 = (lba >> 32) as u32; // LBA'nın üst 32 biti
        cmd.cdw12 = (blocks as u32) - 1; // NLBA alanı 0-base (1 blok için 0 yaz)
        cmd
    }

    /// Yazma komutu (OP_WRITE) oluşturur
    pub fn write(cid: u16, nsid: u32, lba: u64, blocks: u16) -> Self {
        let mut cmd = Self::new(OP_WRITE, cid, nsid);
        cmd.cdw10 = lba as u32;
        cmd.cdw11 = (lba >> 32) as u32;
        cmd.cdw12 = (blocks as u32) - 1;
        cmd
    }

    /// Flush komutu (OP_FLUSH): önbelleği kalıcı depolamaya yaz
    pub fn flush(cid: u16, nsid: u32) -> Self {
        Self::new(OP_FLUSH, cid, nsid)
    }

    /// Identify komutu (OP_ADMIN_IDENTIFY): cns seçer ne tanımlanacağını
    /// cns=0: namespace, cns=1: denetleyici
    pub fn identify(cid: u16, cns: u8, nsid: u32) -> Self {
        let mut cmd = Self::new(OP_ADMIN_IDENTIFY, cid, nsid);
        cmd.cdw10 = cns as u32; // CNS (Controller or Namespace Structure)
        cmd
    }

    /// PRP veri tampon adresini ayarlar.
    /// Tampon tek sayfaya sığıyorsa prp1 yeterli; aksi takdirde prp2 gerekir.
    pub fn set_buffer(&mut self, addr: u64, len: usize) {
        self.prp1 = addr;
        // Tampon sayfa sınırını aşıyorsa prp2'yi bir sonraki sayfa başlangıcına ayarla
        let page_offset = addr & 0xFFF; // Sayfa içi ofset (alt 12 bit)
        if page_offset as usize + len > PAGE_SIZE {
            self.prp2 = (addr & !0xFFF) + PAGE_SIZE as u64; // Sonraki 4KB sayfa
        }
    }
}

// ============================================================================
// TAMAMLAMA KUYRUĞU GİRİŞİ (COMPLETION QUEUE ENTRY / CQE)
// ============================================================================

// 16 byte'lık CQE; NVMe SSD tarafından doldurulur:
//   cdw0: komuta özgü tamamlama verisi
//   cdw1: zamanlanmış alan
//   cid:  hangi komutun tamamlandığı (SQE'deki cid ile eşleşmeli)
//   p:    faz biti (phase bit); kuyruğun döngüsel yapısı için
//   sqid: hangi Submission Queue'dan geldiği
//   status: hata kodu veya başarı (bit0 = faz, bit1-14 = status field)

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvmeCompletion {
    pub cid: u16,     // Tamamlanan komutun ID'si (SQE.cid ile eşleşmeli)
    pub p: u16,       // Faz biti (bit 0); CQE geçerliliğini belirler
    pub sqid: u16,    // Komutun geldiği Submission Queue ID
    pub status: u16,  // Durum alanı: bit0=faz, bit1-14=status code, bit15=DNR
    pub cdw0: u32,    // Komuta özgü tamamlama verisi (okumada LBA sayısı vb.)
    pub cdw1: u32,    // Zamanlanmış alan
}

impl NvmeCompletion {
    /// Komutun başarıyla tamamlandığını döner
    /// Status bits[14:1] = 0 ise başarı (bit0 faz, ihmal edilir)
    pub fn is_success(&self) -> bool {
        (self.status & 0xFFFE) == 0
    }

    /// Durum kodunu döner (bit1-8 hata kategorisi ve kodu)
    pub fn get_status(&self) -> u8 {
        (self.status >> 1) as u8
    }

    /// Faz bitini döner (kuyruğun yeni döngüde mi olduğunu belirler)
    pub fn get_phase(&self) -> bool {
        (self.p & 1) != 0
    }
}

// ============================================================================
// NVMe KUYRUK (NVMe QUEUE)
// ============================================================================

// Bir submission + completion kuyruk çiftini tutar.
// Sadece admin kuyruğu için sqid=cqid=0 kullanılır.
// I/O kuyruklarında sqid ve cqid eşit olmak zorunda değil.
//
// Kuyruk doluluk takibi:
//   sq_tail: CPU bir sonraki SQE'yi nereye yazacak
//   sq_head: SSD son olarak nereyi okudu (doorbell'den gelen geri bildirim)
//   cq_head: CPU bir sonraki CQE'yi nereden okuyacak
//   cq_phase: Mevcut döngünün faz değeri; CQE okunup okunmadığını ayırt eder

#[derive(Clone, Debug)]
pub struct NvmeQueue {
    pub sqid: u16,      // Submission Queue ID
    pub cqid: u16,      // Completion Queue ID
    pub size: u16,      // Kuyruk kapasitesi (giriş sayısı)
    pub sq_tail: u16,   // Gönderme kuyruğu kuyruğu (sonraki yazma pozisyonu)
    pub sq_head: u16,   // Gönderme kuyruğu başı (SSD tarafından güncellenir)
    pub cq_head: u16,   // Tamamlama kuyruğu başı (CPU tarafından okunur)
    pub cq_phase: bool, // Tamamlama kuyruğu faz biti (CQE geçerlilik işaretleyici)
    pub sq_addr: u64,   // Gönderme kuyruğunun fiziksel bellek adresi
    pub cq_addr: u64,   // Tamamlama kuyruğunun fiziksel bellek adresi
    pub sq_db: u64,     // Submission Queue Doorbell yazmacı ofseti (MMIO)
    pub cq_db: u64,     // Completion Queue Doorbell yazmacı ofseti (MMIO)
}

impl NvmeQueue {
    /// Yeni kuyruk çifti oluşturur; zil kapı adresleri CAP.DSTRD'e göre hesaplanır
    pub fn new(sqid: u16, cqid: u16, size: u16, sq_addr: u64, cq_addr: u64, db_stride: u16) -> Self {
        NvmeQueue {
            sqid,
            cqid,
            size,
            sq_tail: 0,
            sq_head: 0,
            cq_head: 0,
            cq_phase: true, // İlk döngüde faz = 1
            sq_addr,
            cq_addr,
            // Zil kapı adresi: base_offset=0x1000, her kuyruk için iki register (SQ ve CQ)
            sq_db: 0x1000 + (sqid as u64 * 2 * db_stride as u64),
            cq_db: 0x1000 + (cqid as u64 * 2 + 1) * db_stride as u64,
        }
    }

    /// Kuyruğa komut gönderir; kuyruk doluysa hata döner
    pub fn submit(&mut self, cmd: &NvmeCommand) -> Result<(), NvmeError> {
        // Gerçek uygulamada: sq_addr[sq_tail]'e cmd yazar, sonra doorbell'i çalar
        self.sq_tail = (self.sq_tail + 1) % self.size;
        Ok(())
    }

    /// Tamamlama kuyruğunu yeni tamamlamalar için sorgular
    /// Faz biti eşleşiyorsa yeni tamamlama var demektir
    pub fn poll_completion(&mut self) -> Option<NvmeCompletion> {
        // Gerçek uygulamada: cq_addr[cq_head]'i okur, faz bitini kontrol eder
        None
    }
}

// ============================================================================
// NVMe DENETLEYİCİSİ (NVMe CONTROLLER)
// ============================================================================

// NVMe denetleyici nesnesi: tüm donanım durumunu ve kuyruk yapılarını tutar.
//
//   NvmeController
//     |-- mmio_base: MMIO yazmaçların sanal/fiziksel adresi
//     |-- capabilities: CAP yazmacından okunan yetenek bilgileri
//     |-- identify: Admin Identify komutuyla alınan denetleyici kimliği
//     |-- namespaces: nsid -> NvmeIdentifyNamespace haritası
//     |-- admin_queue: Admin SQ/CQ çifti (yönetim komutları için)
//     +-- io_queues: I/O SQ/CQ çiftleri (veri oku/yaz için)

/// NVMe Denetleyicisi: donanım durumu ve kuyruk yönetimi
#[derive(Clone, Debug)]
pub struct NvmeController {
    pub bus: u8,                                           // PCI bus numarası
    pub device: u8,                                        // PCI cihaz numarası
    pub function: u8,                                      // PCI fonksiyon numarası
    pub mmio_base: u64,                                    // BAR0 MMIO sanal adresi
    pub capabilities: NvmeCapabilities,                    // Denetleyici yetenekleri
    pub identify: Option<NvmeIdentifyController>,          // Denetleyici tanımlama verisi
    pub namespaces: BTreeMap<u32, NvmeIdentifyNamespace>,  // nsid -> namespace bilgisi
    pub admin_queue: Option<NvmeQueue>,                    // Admin kuyruk çifti
    pub io_queues: Vec<NvmeQueue>,                         // I/O kuyruk çiftleri (her CPU için)
    pub next_cid: u16,                                     // Bir sonraki komut ID (1..=65535, döngüsel)
    pub ready: bool,                                       // Denetleyici kullanıma hazır mı?
    /// MSI kesme vektörü (allocate_msi_vector() ile atanır)
    pub irq_vector: Option<u8>,
    /// Komut zaman aşımı (milisaniye; CAP.TO'dan hesaplanır)
    pub timeout_ms: u16,
}

impl NvmeController {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        NvmeController {
            bus,
            device,
            function,
            mmio_base: 0,
            capabilities: unsafe { mem::zeroed() },
            identify: None,
            namespaces: BTreeMap::new(),
            admin_queue: None,
            io_queues: Vec::new(),
            next_cid: 1,
            ready: false,
            irq_vector: None,
            timeout_ms: 5000, // Varsayılan 5 saniyelik zaman aşımı
        }
    }

    /// 32-bit MMIO yazmacı okur (volatile; önbellek atlanır)
    #[inline]
    unsafe fn read_mmio32(&self, offset: usize) -> u32 {
        let addr = (self.mmio_base + offset as u64) as *const u32;
        core::ptr::read_volatile(addr) // volatile: derleyici optimize etmez
    }

    /// 32-bit MMIO yazmacına yazar (volatile; önbellek atlanır)
    #[inline]
    unsafe fn write_mmio32(&self, offset: usize, value: u32) {
        let addr = (self.mmio_base + offset as u64) as *mut u32;
        core::ptr::write_volatile(addr, value);
    }

    /// 64-bit MMIO yazmacı okur
    #[inline]
    unsafe fn read_mmio64(&self, offset: usize) -> u64 {
        let addr = (self.mmio_base + offset as u64) as *const u64;
        core::ptr::read_volatile(addr)
    }

    /// 64-bit MMIO yazmacına yazar
    #[inline]
    unsafe fn write_mmio64(&self, offset: usize, value: u64) {
        let addr = (self.mmio_base + offset as u64) as *mut u64;
        core::ptr::write_volatile(addr, value);
    }

    /// Denetleyiciyi tam donanım iniziyalizasyonuyla başlatır.
    ///
    /// Başlatma sırası:
    /// 1. BAR0'dan MMIO adresini oku ve haritala
    /// 2. CAP yazmacından yetenekleri oku
    /// 3. CC.EN=0 yap (denetleyiciyi devre dışı bırak), CSTS.RDY=0 bekle
    /// 4. Admin kuyruklarını tahsis et ve yapılandır (AQA, ASQ, ACQ)
    /// 5. CC.EN=1 yap (etkinleştir), CSTS.RDY=1 bekle
    /// 6. MSI kesmesini yapılandır
    /// 7. Identify Controller komutu gönder
    /// 8. Namespace'leri keşfet
    pub fn init(&mut self) -> Result<(), NvmeError> {
        // BAR0 MMIO'yu oku (NVMe spec: 64-bit MMIO olmalı)
        let bar = crate::drivers::pci::read_bar_mmio(self.bus, self.device, self.function, 0)
            .ok_or(NvmeError::NoController)?;
        self.mmio_base = bar.base;

        // MMIO bölgesini sayfa tablolarına haritala
        let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
        if !mapped.is_null() {
            self.mmio_base = mapped as u64;
        } else {
            self.mmio_base = crate::memory::active_physical_offset() + bar.base;
        }

        unsafe {
            // CAP yazmacını oku: denetleyici yeteneklerini çıkar
            let cap = self.read_mmio64(NVME_CAP);
            self.capabilities = NvmeCapabilities::parse(cap);
            self.timeout_ms = self.capabilities.timeout_ms;

            crate::serial_println!("[NVMe] CAP: MQES={}, TO={}ms, DSTRD={}",
                self.capabilities.max_queue_entries,
                self.capabilities.timeout_ms,
                self.capabilities.doorbell_stride);

            // Denetleyiciyi devre dışı bırak (CC.EN=0)
            // Bu, admin kuyruk yapılandırması için gereklidir
            self.write_mmio32(NVME_CC, 0);

            // CSTS.RDY=0 olana kadar bekle (disabled onayı)
            let start = crate::task::scheduler::get_ticks();
            loop {
                let csts = self.read_mmio32(NVME_CSTS);
                if (csts & CSTS_RDY) == 0 {
                    break; // Denetleyici devre dışı onaylandı
                }
                if crate::task::scheduler::get_ticks() - start > 1000 {
                    crate::serial_println!("[NVMe] Timeout waiting for disable");
                    break;
                }
            }

            // Admin kuyruklarını oluştur ve yapılandır
            self.setup_admin_queue()?;

            // Denetleyiciyi etkinleştir:
            // - CSS=0:  NVM komut seti seç
            // - MPS=0:  4KB sayfa boyutu (2^(12+0))
            // - AMS=0:  Round-robin arbitrasyon
            // - IOSQES=6: SQ giriş boyutu = 2^6 = 64 byte
            // - IOCQES=4: CQ giriş boyutu = 2^4 = 16 byte
            let cc = CC_EN
                | (0 << CC_CSS_SHIFT)      // NVM command set
                | (0 << CC_MPS_SHIFT)      // 4KB page size (0 = 2^(12+0))
                | (0 << CC_AMS_SHIFT)      // Round robin arbitration
                | (6 << CC_IOSQES_SHIFT)   // 64-byte SQ entry size (2^6)
                | (4 << CC_IOCQES_SHIFT);  // 16-byte CQ entry size (2^4)

            self.write_mmio32(NVME_CC, cc);

            // CSTS.RDY=1 olana kadar bekle (enabled onayı)
            let start = crate::task::scheduler::get_ticks();
            loop {
                let csts = self.read_mmio32(NVME_CSTS);
                if (csts & CSTS_RDY) != 0 {
                    break; // Denetleyici hazır
                }
                if (csts & CSTS_CFS) != 0 {
                    // CSTS.CFS=1: kritik donanım hatası
                    return Err(NvmeError::ControllerError);
                }
                if crate::task::scheduler::get_ticks() as u64 - start as u64 > self.timeout_ms as u64 / 10 {
                    return Err(NvmeError::Timeout);
                }
            }

            // MSI kesmesini yapılandır (polling yerine daha verimli)
            self.setup_interrupts()?;

            // Identify Controller: model/seri/firmware bilgilerini al
            self.identify_controller()?;

            // Tüm namespace'leri keşfet (nsid=1 den nn'e kadar)
            self.discover_namespaces()?;
        }

        self.ready = true;
        crate::serial_println!("[NVMe] Controller initialized: {} namespaces",
            self.namespaces.len());

        if let Some(ref id) = self.identify {
            crate::serial_println!("[NVMe] Model: {}", id.get_model());
            crate::serial_println!("[NVMe] Serial: {}", id.get_serial());
            crate::serial_println!("[NVMe] Firmware: {}", id.get_firmware());
        }

        Ok(())
    }

    /// Admin kuyruk çiftini tahsis eder ve MMIO'ya yazar.
    ///
    /// Admin Queue belleği sayfa hizalı ve sıfır doldurulmuş olmalıdır:
    ///   SQ= ADMIN_QUEUE_SIZE * 64 byte = 2 KB
    ///   CQ= ADMIN_QUEUE_SIZE * 16 byte = 512 byte
    unsafe fn setup_admin_queue(&mut self) -> Result<(), NvmeError> {
        let sq_size = ADMIN_QUEUE_SIZE;
        let cq_size = ADMIN_QUEUE_SIZE;

        // Her kuyruk için kaç sayfa gerektiğini hesapla
        let sq_pages = (sq_size as usize * 64 + PAGE_SIZE - 1) / PAGE_SIZE;
        let cq_pages = (cq_size as usize * 16 + PAGE_SIZE - 1) / PAGE_SIZE;

        // Sayfa hizalı fiziksel bellek tahsis et
        let sq_phys = crate::memory::alloc_phys(sq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let cq_phys = crate::memory::alloc_phys(cq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;

        // Kuyruğu sıfırla (geçersiz/eski giriş kalmasın)
        let sq_virt = (crate::memory::active_physical_offset() + sq_phys) as *mut u8;
        let cq_virt = (crate::memory::active_physical_offset() + cq_phys) as *mut u8;
        core::ptr::write_bytes(sq_virt, 0, sq_pages * PAGE_SIZE);
        core::ptr::write_bytes(cq_virt, 0, cq_pages * PAGE_SIZE);

        // AQA (Admin Queue Attributes) yaz: alt 16 bit = ASQA (SQ boyutu), üst 16 bit = ACQA
        let aqa = ((sq_size - 1) as u32) | (((cq_size - 1) as u32) << 16); // 0-base boyutlar
        self.write_mmio32(NVME_AQA, aqa);

        // Admin kuyruk fiziksel adreslerini yaz
        self.write_mmio64(NVME_ASQ, sq_phys); // Submission Queue fiziksel adresi
        self.write_mmio64(NVME_ACQ, cq_phys); // Completion Queue fiziksel adresi

        // Yazılım kuyruk yapısını oluştur
        let db_stride = self.capabilities.doorbell_stride;
        self.admin_queue = Some(NvmeQueue::new(
            0,  // Admin SQ ID = 0 (spec gereği sabit)
            0,  // Admin CQ ID = 0
            sq_size,
            sq_phys,
            cq_phys,
            db_stride,
        ));

        crate::serial_println!("[NVMe] Admin queue configured (size={})", sq_size);
        Ok(())
    }

    /// MSI (Message Signaled Interrupt) kesmesini yapılandırır.
    /// MSI başarısız olursa polling moduna düşülür.
    unsafe fn setup_interrupts(&mut self) -> Result<(), NvmeError> {
        // Sistem geneli MSI vektörü tahsis et
        let vector = crate::interrupts::allocate_msi_vector(nvme_irq_handler)
            .ok_or(NvmeError::FeatureNotSupported)?;

        self.irq_vector = Some(vector);

        // PCI MSI konfigürasyonunu yaz (Message Address + Message Data)
        let apic_id = crate::cpu::smp::current_cpu_id() as u32;
        if !crate::drivers::pci::configure_pci_interrupt(
            self.bus, self.device, self.function,
            vector, apic_id
        ) {
            crate::serial_println!("[NVMe] MSI configuration failed, using polling");
            self.irq_vector = None;
        } else {
            crate::serial_println!("[NVMe] MSI configured (vector={})", vector);
        }

        Ok(())
    }

    /// Identify Controller komutu gönderir; denetleyici kimliğini doldurur.
    /// 4KB tampon tahsis edilerek Admin Queue'ya gönderilir.
    unsafe fn identify_controller(&mut self) -> Result<(), NvmeError> {
        // 4KB fiziksel tampon tahsis et (Identify tamponu için sayfa hizalı gerekir)
        let buffer_phys = crate::memory::alloc_phys(PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let buffer_virt = (crate::memory::active_physical_offset() + buffer_phys) as *mut u8;
        core::ptr::write_bytes(buffer_virt, 0, PAGE_SIZE);

        // Identify Controller komutu: CNS=1 denetleyici tanımı ister
        let cid = self.get_cid();
        let mut cmd = NvmeCommand::identify(cid, 1, 0);
        cmd.set_buffer(buffer_phys, PAGE_SIZE);

        // Admin kuyruğuna gönder ve tamamlanmasını bekle
        self.submit_admin_command(&cmd)?;

        // Tampon içeriğini NvmeIdentifyController yapısına dönüştür
        let idata = &*(buffer_virt as *const NvmeIdentifyController);
        self.identify = Some(*idata);

        Ok(())
    }

    /// Tüm namespace'leri keşfeder; kapasitesi sıfırdan büyük olanları kaydeder
    unsafe fn discover_namespaces(&mut self) -> Result<(), NvmeError> {
        let nn = self.identify.map(|i| i.nn).unwrap_or(0); // Toplam namespace sayısı

        for nsid in 1..=nn {
            if let Ok(ns) = self.identify_namespace(nsid) {
                if ns.ncap > 0 { // Kullanılabilir kapasitesi olan namespace
                    self.namespaces.insert(nsid, ns);
                    crate::serial_println!("[NVMe] Namespace {}: {} blocks, {} bytes/block",
                        nsid, ns.get_block_count(), ns.get_block_size());
                }
            }
        }

        Ok(())
    }

    /// Belirli bir namespace'i tanımlar (Identify CNS=0)
    unsafe fn identify_namespace(&mut self, nsid: u32) -> Result<NvmeIdentifyNamespace, NvmeError> {
        let buffer_phys = crate::memory::alloc_phys(PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let buffer_virt = (crate::memory::active_physical_offset() + buffer_phys) as *mut u8;
        core::ptr::write_bytes(buffer_virt, 0, PAGE_SIZE);

        let cid = self.get_cid();
        let mut cmd = NvmeCommand::identify(cid, 0, nsid); // CNS=0: namespace bilgisi
        cmd.set_buffer(buffer_phys, PAGE_SIZE);

        self.submit_admin_command(&cmd)?;

        // Fiziksel belleği NvmeIdentifyNamespace yapısına kopyala
        let nsdata = *(buffer_virt as *const NvmeIdentifyNamespace);
        Ok(nsdata)
    }

    /// Admin kuyruğuna komut gönderir ve polling ile tamamlamasını bekler.
    ///
    /// Phase tag mekanizması:
    ///   Kuyruk her dönemde 0 veya 1 arasında geçiş yapar.
    ///   CQE'nin faz biti mevcut beklenen fazla eşleşiyorsa yeni tamamlama var.
    ///   Aksi takdirde eski/stale giriş, yoksay.
    unsafe fn submit_admin_command(&mut self, cmd: &NvmeCommand) -> Result<NvmeCompletion, NvmeError> {
        let queue = self.admin_queue.as_mut().ok_or(NvmeError::NotReady)?;

        // SQE'yi gönderme kuyruğuna yaz (sq_tail pozisyonuna)
        let sq_addr = (crate::memory::active_physical_offset() + queue.sq_addr) as *mut NvmeCommand;
        let sq_entry = &mut *sq_addr.add(queue.sq_tail as usize);
        *sq_entry = *cmd;

        // Bellek bariyeri: yazmanın CPU'nun dışına çıktığından emin ol
        core::sync::atomic::fence(Ordering::SeqCst);

        // SQ Doorbell'i çal: SSD'ye yeni komut olduğunu bildir
        let db_addr = (self.mmio_base as usize + queue.sq_db as usize) as *mut u32;
        let new_tail = (queue.sq_tail + 1) % queue.size;
        core::ptr::write_volatile(db_addr, new_tail as u32); // Yeni kuyruk kuyruğu değerini yaz
        queue.sq_tail = new_tail;

        // Tamamlama kuyruğunu polling ile izle
        let start = crate::task::scheduler::get_ticks();
        loop {
            let cq_addr = (crate::memory::active_physical_offset() + queue.cq_addr) as *const NvmeCompletion;
            let cq_entry = &*cq_addr.add(queue.cq_head as usize);

            // Faz bitini kontrol et: eşleşiyorsa bu CQE yeni (işlenmemiş)
            let phase = (cq_entry.p & 1) != 0;
            if phase == queue.cq_phase {
                let completion = *cq_entry; // CQE'yi kopyala

                // CQ başını ilerlet (sonraki tamamlama buraya yazılacak)
                queue.cq_head = (queue.cq_head + 1) % queue.size;

                // CQ Doorbell'i çal: CPU'nun bu CQE'yi gördüğünü SSD'ye bildir
                let cdb_addr = (self.mmio_base as usize + queue.cq_db as usize) as *mut u32;
                core::ptr::write_volatile(cdb_addr, queue.cq_head as u32);

                // Kuyruk döndüğünde faz bitini tersle (döngüsel kuyruk faz takibi)
                if queue.cq_head == 0 {
                    queue.cq_phase = !queue.cq_phase;
                }

                if !completion.is_success() {
                    crate::serial_println!("[NVMe] Command failed: status={:#x}", completion.status);
                    return Err(NvmeError::ControllerError);
                }

                return Ok(completion);
            }

            // Zaman aşımı kontrolü
            if crate::task::scheduler::get_ticks() as u64 - start as u64 > self.timeout_ms as u64 / 10 {
                return Err(NvmeError::Timeout);
            }

            // MSI modundaysa kesme worker'ını uyandır
            if let Some(vector) = self.irq_vector {
                crate::interrupts::kick_irq_worker();
            }
        }
    }

    /// Bir sonraki komut ID'sini döner ve sayacı ilerletir.
    /// cid döngüsel olarak 1..65535 arasında gider (0 atlanır, spec gereği)
    pub fn get_cid(&mut self) -> u16 {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        if self.next_cid == 0 {
            self.next_cid = 1; // 0'ı atla (bazı denetleyiciler 0'ı geçersiz sayar)
        }
        cid
    }

    /// Namespace'ten blok okur.
    /// lba: başlangıç sektörü, blocks: okunacak sektör sayısı
    pub fn read(&mut self, nsid: u32, lba: u64, blocks: u16, buffer: &mut [u8]) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        let cid = self.get_cid();
        let mut cmd = NvmeCommand::read(cid, nsid, lba, blocks);

        // Tampon fiziksel adresini PRP alanına yaz
        let buffer_phys = crate::memory::virt_to_phys_u64(buffer.as_ptr() as u64);
        cmd.set_buffer(buffer_phys, buffer.len());

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Namespace'e blok yazar
    pub fn write(&mut self, nsid: u32, lba: u64, blocks: u16, buffer: &[u8]) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        let cid = self.get_cid();
        let mut cmd = NvmeCommand::write(cid, nsid, lba, blocks);

        let buffer_phys = crate::memory::virt_to_phys_u64(buffer.as_ptr() as u64);
        cmd.set_buffer(buffer_phys, buffer.len());

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Namespace önbelleğini (volatile write cache) diske yazar
    pub fn flush(&mut self, nsid: u32) -> Result<(), NvmeError> {
        if !self.ready {
            return Err(NvmeError::NotReady);
        }

        let cid = self.get_cid();
        let cmd = NvmeCommand::flush(cid, nsid);

        unsafe {
            self.submit_admin_command(&cmd)?;
        }

        Ok(())
    }

    /// Namespace blok boyutunu döner; namespace bulunamazsa 512 (eski SATA uyumluluğu)
    pub fn get_block_size(&self, nsid: u32) -> u32 {
        self.namespaces.get(&nsid).map(|ns| ns.get_block_size()).unwrap_or(512)
    }

    /// Namespace toplam blok sayısını döner
    pub fn get_block_count(&self, nsid: u32) -> u64 {
        self.namespaces.get(&nsid).map(|ns| ns.get_block_count()).unwrap_or(0)
    }

    /// Namespace toplam kapasitesini byte cinsinden döner
    pub fn get_capacity(&self, nsid: u32) -> u64 {
        self.namespaces.get(&nsid).map(|ns| ns.get_capacity_bytes()).unwrap_or(0)
    }
}

// ============================================================================
// NVMe YÖNETİCİSİ (NVMe MANAGER)
// ============================================================================

// Tüm NVMe denetleyicilerini global olarak depolar.
// Birden fazla NVMe SSD sisteme takılı olabilir.

lazy_static::lazy_static! {
    static ref NVME_CONTROLLERS: Mutex<Vec<NvmeController>> = Mutex::new(Vec::new());
}

/// PCI bus taranarak NVMe denetleyicilerini keşfeder.
/// class_code=0x01, subclass=0x08 olan tüm cihazlar NVMe olarak değerlendirilir.
pub fn discover_nvme_controllers() -> Vec<NvmeController> {
    let mut controllers = Vec::new();

    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == PCI_CLASS_STORAGE && dev.subclass == PCI_SUBCLASS_NVME {
            controllers.push(NvmeController::new(dev.bus, dev.device, dev.function));
        }
    }

    controllers
}

/// NVMe alt sistemini başlatır: keşif + iniziyelizasyon
pub fn init() {
    crate::serial_println!("[NVMe] Initializing NVMe subsystem...");

    let controllers = discover_nvme_controllers();
    let mut nvme_ctrls = NVME_CONTROLLERS.lock();

    for mut ctrl in controllers {
        if ctrl.init().is_ok() {
            nvme_ctrls.push(ctrl);
        }
    }

    crate::serial_println!("[NVMe] Found {} controllers", nvme_ctrls.len());
}

/// Varsayılan (ilk) denetleyiciyi döner; yoksa None
pub fn default_controller() -> Option<NvmeController> {
    NVME_CONTROLLERS.lock().first().cloned()
}

/// Varsayılan denetleyiciden blok okur
pub fn read(nsid: u32, lba: u64, blocks: u16, buffer: &mut [u8]) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.read(nsid, lba, blocks, buffer)
}

/// Varsayılan denetleyiciye blok yazar
pub fn write(nsid: u32, lba: u64, blocks: u16, buffer: &[u8]) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.write(nsid, lba, blocks, buffer)
}

/// Varsayılan denetleyiciyi temizler (önbelleği diske yazar)
pub fn flush(nsid: u32) -> Result<(), NvmeError> {
    let mut controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(NvmeError::NoController)?;
    ctrl.flush(nsid)
}

/// Namespace bilgisini döner: (blok_boyutu, blok_sayısı, kapasite_byte)
pub fn get_namespace_info(nsid: u32) -> Option<(u32, u64, u64)> {
    let controllers = NVME_CONTROLLERS.lock();
    let ctrl = controllers.first()?;
    let ns = ctrl.namespaces.get(&nsid)?;
    Some((ns.get_block_size(), ns.get_block_count(), ns.get_capacity_bytes()))
}

// ============================================================================
// IRQ HANDLER (KESME İŞLEYİCİSİ)
// ============================================================================

/// NVMe MSI kesme işleyicisi.
/// Admin veya I/O komutu tamamlandığında çağrılır.
/// Bekleyen görevleri uyandırır (tam implementasyonda).
fn nvme_irq_handler(vector: u8) {
    crate::serial_println!("[NVMe] IRQ received on vector {}", vector);

    // Tam implementasyonda: bekleyen görevler uyandırılır (kondisyon değişkeni/semaphore)
}

// ============================================================================
// I/O KUYRUK DESTEĞI (I/O QUEUE SUPPORT)
// ============================================================================

/// I/O submission ve completion kuyruk çifti oluşturur.
///
/// Her çekirdek için ayrı kuyruk oluşturularak kilit çakışması önlenir:
///   qid=1 -> CPU 0
///   qid=2 -> CPU 1
///   ...
pub fn create_io_queue(controller: &mut NvmeController, qid: u16, size: u16) -> Result<(), NvmeError> {
    if !controller.ready {
        return Err(NvmeError::NotReady);
    }

    unsafe {
        // I/O kuyruğu için bellek tahsis et
        let sq_pages = (size as usize * 64 + PAGE_SIZE - 1) / PAGE_SIZE;
        let cq_pages = (size as usize * 16 + PAGE_SIZE - 1) / PAGE_SIZE;

        let sq_phys = crate::memory::alloc_phys(sq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;
        let cq_phys = crate::memory::alloc_phys(cq_pages * PAGE_SIZE)
            .ok_or(NvmeError::DataTransferError)?;

        // Kuyruğu sıfırla
        let sq_virt = (crate::memory::active_physical_offset() + sq_phys) as *mut u8;
        let cq_virt = (crate::memory::active_physical_offset() + cq_phys) as *mut u8;
        core::ptr::write_bytes(sq_virt, 0, sq_pages * PAGE_SIZE);
        core::ptr::write_bytes(cq_virt, 0, cq_pages * PAGE_SIZE);

        // Admin komutuyla Completion Queue oluştur:
        // cdw10: QSIZE(0-base) | QID<<16 ; cdw11: PC (Physically Contiguous) = 1
        let mut cmd = NvmeCommand::new(OP_ADMIN_CREATE_CQ, controller.get_cid(), 0);
        cmd.prp1 = cq_phys;
        cmd.cdw10 = ((size - 1) as u32) | ((qid as u32) << 16); // QSIZE | QID
        cmd.cdw11 = 1; // Fiziksel bitişik, kesme yok

        controller.submit_admin_command(&cmd)?;

        // Admin komutuyla Submission Queue oluştur:
        // cdw11: PC=1 | CQID<<16 (bu SQ'nun hangi CQ'ya raporlanacağı)
        let mut cmd = NvmeCommand::new(OP_ADMIN_CREATE_SQ, controller.get_cid(), 0);
        cmd.prp1 = sq_phys;
        cmd.cdw10 = ((size - 1) as u32) | ((qid as u32) << 16);
        cmd.cdw11 = 1 | ((qid as u32) << 16); // PC=1, CQID=qid

        controller.submit_admin_command(&cmd)?;

        // Yazılım kuyruk yapısını denetleyiciye ekle
        let db_stride = controller.capabilities.doorbell_stride;
        controller.io_queues.push(NvmeQueue::new(
            qid,
            qid,
            size,
            sq_phys,
            cq_phys,
            db_stride,
        ));

        crate::serial_println!("[NVMe] I/O queue {} created (size={})", qid, size);
    }

    Ok(())
}

// ============================================================================
// BLOK CİHAZ ARAYÜZİ (BLOCK DEVICE INTERFACE)
// ============================================================================

// NVMe sürücüsünü genel BlockDevice trait'iyle entegre eder.
// Bu sayede dosya sistemi katmanı ATA, VirtIO ve NVMe'yi aynı arayüzle kullanabilir.

use crate::drivers::block::{BlockDevice, BlockDeviceError, BlockDeviceType};

/// NVMe blok cihazı sarmalayıcısı: BlockDevice trait implementasyonu
pub struct NvmeBlockDevice {
    pub controller_idx: usize, // NVME_CONTROLLERS içindeki indeks
    pub nsid: u32,             // Hedef namespace ID
    pub block_size: u32,       // Namespace blok boyutu (byte; genellikle 4096)
    pub block_count: u64,      // Toplam blok sayısı
}

impl NvmeBlockDevice {
    pub fn new(controller_idx: usize, nsid: u32) -> Option<Self> {
        let controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get(controller_idx)?;
        let ns = ctrl.namespaces.get(&nsid)?;

        Some(NvmeBlockDevice {
            controller_idx,
            nsid,
            block_size: ns.get_block_size(),
            block_count: ns.get_block_count(),
        })
    }
}

/// BlockDevice trait implementasyonu: dosya sistemi katmanı bu arayüzü kullanır
impl BlockDevice for NvmeBlockDevice {
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;

        // Tampon boyutuna göre blok sayısını hesapla (en az 1)
        let blocks = (buffer.len() / self.block_size as usize) as u16;
        ctrl.read(self.nsid, lba, blocks.max(1), buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;

        let blocks = (buffer.len() / self.block_size as usize) as u16;
        ctrl.write(self.nsid, lba, blocks.max(1), buffer)
            .map_err(|_| BlockDeviceError::IoError)
    }

    /// Volatile write cache'i temizler; güç kesintisi öncesi çağrılmalıdır
    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        let mut controllers = NVME_CONTROLLERS.lock();
        let ctrl = controllers.get_mut(self.controller_idx)
            .ok_or(BlockDeviceError::DeviceNotFound)?;

        ctrl.flush(self.nsid)
            .map_err(|_| BlockDeviceError::IoError)
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn device_type(&self) -> BlockDeviceType {
        BlockDeviceType::Nvme
    }

    /// Cihaz adını döner (örn. "nvme0n1" = denetleyici 0, namespace 1)
    fn device_name(&self) -> alloc::string::String {
        alloc::format!("nvme{}n{}", self.controller_idx, self.nsid)
    }
}
