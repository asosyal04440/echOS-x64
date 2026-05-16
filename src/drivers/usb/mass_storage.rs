//! # echOS USB Yığın Depolama Sürücüsü (Mass Storage)
//!
//! USB Yığın Depolama Sınıfı (MSC) sürücüsü, Bulk-Only Transport (BBB) protokolü
//! ve SCSI şeffaf komut kümesi kullanarak USB flash bellek ve sabit disklere erişimi sağlar.
//!
//! ## BBB Aktarım Katmanı
//!
//! ```
//!  ┌──────────────────────────────────────────────────────┐
//!  │  Uygulama: read_blocks() / write_blocks()            │
//!  ├──────────────────────────────────────────────────────┤
//!  │  MassStorageDriver: init → inquiry → read_capacity  │
//!  ├──────────────────────────────────────────────────────┤
//!  │  BBB Aktarım: CBW → Data → CSW döngüsü              │
//!  ├──────────────────────────────────────────────────────┤
//!  │  USB Bulk OUT (Komut+Veri)  ↔  USB Bulk IN (Yanıt)  │
//!  └──────────────────────────────────────────────────────┘
//! ```
//!
//! ## Bulk-Only Transport (BBB) Protokolü
//!
//! Her işlem üç aşamadan oluşur:
//!
//! **Aşama 1 — CBW (Command Block Wrapper, 31 byte):**
//! ```
//! Host → Cihaz (Bulk OUT):
//!   [dCBWSignature: 0x43425355 "USBC"]  4 byte
//!   [dCBWTag: etiket]                    4 byte
//!   [dCBWDataTransferLength]             4 byte
//!   [bmCBWFlags: 0x80=IN, 0x00=OUT]     1 byte
//!   [bCBWLUN: 0-15]                      1 byte
//!   [bCBWCBLength: 1-16]                1 byte
//!   [CBWCB: SCSI komutu]                16 byte
//! Toplam: 31 byte
//! ```
//!
//! **Aşama 2 — Veri Aktarımı (CBWFlags'e göre):**
//! ```
//! cbm.flags & 0x80 = 1 → Data IN:  Cihaz → Host (Bulk IN)
//! cbm.flags & 0x80 = 0 → Data OUT: Host  → Cihaz (Bulk OUT)
//! dCBWDataTransferLength = 0 → Veri yok, CSW'ye geç
//! ```
//!
//! **Aşama 3 — CSW (Command Status Wrapper, 13 byte):**
//! ```
//! Cihaz → Host (Bulk IN):
//!   [dCSWSignature: 0x53425355 "USBS"]  4 byte
//!   [dCSWTag: CBW etiketi ile eşleşmeli] 4 byte
//!   [dCSWDataResidue: aktarılamamış byte]4 byte
//!   [bCSWStatus: 0=Geçti, 1=Başarısız]  1 byte
//! Toplam: 13 byte
//! ```
//!
//! ## SCSI Komut Düzeyi
//!
//! MSC, SCSI saydamlık komut kümesi kullanır.
//! Yaygın komutlar:
//! - `INQUIRY (0x12)`: Üretici + model bilgisi
//! - `TEST_UNIT_READY (0x00)`: Cihaz hazır mı?
//! - `READ_CAPACITY_10 (0x25)`: Disk boyutu (blok sayısı + blok boyu)
//! - `READ_10 (0x28)`: LBA adresinden veri oku
//! - `WRITE_10 (0x2A)`: LBA adresine veri yaz
//! - `REQUEST_SENSE (0x03)`: Hata detayları (18 byte sense data)
//!
//! ## LUN (Logical Unit Number)
//!
//! Tek bir USB cihazı birden fazla mantıksal birim içerebilir.
//! Örneğin, çoklu kart okuyucu: SD, CF, MS slotları → ayrı LUN'lar.
//! `GET_MAX_LUN` isteğiyle maksimum LUN sayısı sorgulanır.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::{UsbDevice, UsbDirection, UsbEndpoint, UsbError, UsbSetupPacket, UsbTransferType};

// ============================================================================
// YIĞIN DEPOLAMA SINIF İSTEKLERİ
// USB kontrol aktarımıyla gönderilen MSC sınıfına özgü komutlar
// ============================================================================

/// Toplu sıfırlama: Bulk-Only Mass Storage Reset
/// Hem host hem cihaz durumunu default haline getirir
const MSC_RESET: u8 = 0xFF;

/// Maksimum LUN sorgula: Cihazın kaç mantıksal birimi olduğunu öğren
/// Yanıt: 1 byte (max LUN değeri; 0 = tek LUN)
const MSC_GET_MAX_LUN: u8 = 0xFE;

// ============================================================================
// TOPLU YALNIZCA TAŞIMA İMZALARI (Bulk-Only Transport Signatures)
// Komut/durum sarmalayıcılarda doğrulama baytları
// ============================================================================

/// CBW imzası: ASCII "USBC" = 0x43425355
/// Host, her komut bloğunu bu 4-byte imzayla açar.
const CBW_SIGNATURE: u32 = 0x43425355; // "USBC"

/// CSW imzası: ASCII "USBS" = 0x53425355
/// Cihaz, her durum yanıtını bu 4-byte imzayla başlatır.
const CSW_SIGNATURE: u32 = 0x53425355; // "USBS"

// ============================================================================
// SCSI KOMUT OPCODE'LARI
// Komut Blok Alanı'nın (CBWCB) ilk byte: komut tipi
// ============================================================================

const SCSI_TEST_UNIT_READY: u8 = 0x00; // Cihaz hazır mı?
const SCSI_REQUEST_SENSE: u8 = 0x03; // Hata detay bilgisi
const SCSI_FORMAT_UNIT: u8 = 0x04; // Birim biçimlendir
const SCSI_READ_6: u8 = 0x08; // Oku (6-byte, eski format)
const SCSI_WRITE_6: u8 = 0x0A; // Yaz (6-byte, eski format)
const SCSI_INQUIRY: u8 = 0x12; // Cihaz tanımlama
const SCSI_MODE_SELECT_6: u8 = 0x15; // Mod seç (6-byte)
const SCSI_MODE_SENSE_6: u8 = 0x1A; // Mod anlamlandır (6-byte)
const SCSI_START_STOP_UNIT: u8 = 0x1B; // Başlat/durdur (çıkar/yükle)
const SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E; // Ortam çıkarılmasını engelle
const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23; // Biçim kapasitelerini oku
const SCSI_READ_CAPACITY_10: u8 = 0x25; // Kapasite oku (10-byte)
const SCSI_READ_10: u8 = 0x28; // Oku (10-byte, standart)
const SCSI_WRITE_10: u8 = 0x2A; // Yaz (10-byte, standart)
const SCSI_WRITE_AND_VERIFY_10: u8 = 0x2E; // Yaz ve doğrula
const SCSI_VERIFY_10: u8 = 0x2F; // Doğrula
const SCSI_SYNCHRONIZE_CACHE_10: u8 = 0x35; // Önbelleği senkronize et (flush)
const SCSI_READ_TOC: u8 = 0x43; // İçindekiler tablosu oku (CD-ROM)
const SCSI_MODE_SELECT_10: u8 = 0x55; // Mod seç (10-byte)
const SCSI_MODE_SENSE_10: u8 = 0x5A; // Mod anlamlandır (10-byte)
const SCSI_READ_16: u8 = 0x88; // Oku (16-byte, büyük LBA)
const SCSI_WRITE_16: u8 = 0x8A; // Yaz (16-byte, büyük LBA)
const SCSI_READ_CAPACITY_16: u8 = 0x9E; // Kapasite oku (16-byte, 2TB+)

// ============================================================================
// KOMUT BLOK SARMAYICI (Command Block Wrapper - CBW)
// Host'tan cihaza gönderilen 31-byte komut paketi
// ============================================================================

/// Komut Blok Sarmalayıcı (CBW) - 31 byte.
///
/// ## Yapı Ayrıntıları
///
/// `#[repr(C, packed)]`: USB üzerinden gönderilen bayt sıralaması C ABI ile
/// birebir eşleşmelidir. Padding olmamalı; yoksa imza yanlış konuma düşer.
///
/// ## İmza Doğrulaması
///
/// Cihaz, `dCBWSignature = 0x43425355` ("USBC") değerini doğrular.
/// Farklı olursa CBWCB alanı göz ardı edilir (protokol hatası).
///
/// ## Etiket Eşleştirme
///
/// `tag` alanı, CBW ve CSW'yi eşleştirmek için kullanılır.
/// CSW'deki `dCSWTag` CBW'deki `dCBWTag` ile aynı olmalıdır.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CommandBlockWrapper {
    /// Sabit imza: 0x43425355 ("USBC")
    pub signature: u32,
    /// Komut etiketi: CBW ve CSW eşleştirmesi için özgün sayı
    pub tag: u32,
    /// Aktarımın toplam byte sayısı (0 ise veri aşaması yok)
    pub transfer_length: u32,
    /// Yön bayrağı: 0x80=Data IN (cihazdan), 0x00=Data OUT (cihaza)
    pub flags: u8,
    /// Mantıksal Birim Numarası (Logical Unit Number, 0..15)
    pub lun: u8,
    /// SCSI komut bloğu uzunluğu (1..16 byte)
    pub cb_length: u8,
    /// SCSI komut bloğu (örn. READ_10, WRITE_10; 16 byte maks.)
    pub cb: [u8; 16],
}

impl CommandBlockWrapper {
    /// Yeni CBW oluşturur.
    ///
    /// `direction=In` → flags=0x80 (cihaz → host veri akışı)
    /// `direction=Out` → flags=0x00 (host → cihaz veri akışı)
    /// `lun & 0x0F`: LUN 4 bit, üst 4 bit sıfır olmalı
    pub fn new(
        tag: u32,
        transfer_length: u32,
        direction: UsbDirection,
        lun: u8,
        cb_length: u8,
    ) -> Self {
        Self {
            signature: CBW_SIGNATURE,
            tag,
            transfer_length,
            flags: if direction == UsbDirection::In {
                0x80
            } else {
                0x00
            },
            lun: lun & 0x0F,
            cb_length: cb_length.min(16),
            cb: [0u8; 16],
        }
    }

    /// READ(10) komutu için CBW oluşturur.
    ///
    /// ## CBWCB Formatı (READ_10, 10 byte)
    /// ```
    /// cb[0] = 0x28 (READ_10 opcode)
    /// cb[1] = 0x00 (bayraklar)
    /// cb[2..6] = LBA (big-endian 32-bit)
    /// cb[6] = 0x00 (grup numarası)
    /// cb[7..9] = blok sayısı (big-endian 16-bit)
    /// cb[9] = 0x00 (kontrol)
    /// ```
    /// Transfer uzunluğu: `block_count * 512` byte (standart sektör boyutu)
    pub fn read10(tag: u32, lun: u8, lba: u32, block_count: u16) -> Self {
        let mut cbw = Self::new(tag, (block_count as u32) * 512, UsbDirection::In, lun, 10);
        cbw.cb[0] = SCSI_READ_10;
        cbw.cb[1] = 0; // Bayraklar
        cbw.cb[2..6].copy_from_slice(&lba.to_be_bytes()); // LBA big-endian
        cbw.cb[6] = 0; // Grup numarası
        cbw.cb[7..9].copy_from_slice(&block_count.to_be_bytes()); // Blok sayısı big-endian
        cbw.cb[9] = 0; // Kontrol
        cbw
    }

    /// WRITE(10) komutu için CBW oluşturur.
    ///
    /// READ_10 ile aynı formattadır; fark: direction=OUT ve opcode=0x2A.
    pub fn write10(tag: u32, lun: u8, lba: u32, block_count: u16) -> Self {
        let mut cbw = Self::new(tag, (block_count as u32) * 512, UsbDirection::Out, lun, 10);
        cbw.cb[0] = SCSI_WRITE_10;
        cbw.cb[1] = 0; // Bayraklar
        cbw.cb[2..6].copy_from_slice(&lba.to_be_bytes());
        cbw.cb[6] = 0; // Grup numarası
        cbw.cb[7..9].copy_from_slice(&block_count.to_be_bytes());
        cbw.cb[9] = 0; // Kontrol
        cbw
    }

    /// READ_CAPACITY(10) komutu için CBW oluşturur.
    ///
    /// Yanıt: 8 byte
    /// - Son LBA (4 byte, big-endian)
    /// - Blok boyutu (4 byte, big-endian)
    pub fn read_capacity10(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 8, UsbDirection::In, lun, 10);
        cbw.cb[0] = SCSI_READ_CAPACITY_10;
        cbw
    }

    /// INQUIRY komutu için CBW oluşturur.
    ///
    /// Yanıt: 36 byte minimum (ScsiInquiry yapısı)
    /// - Üretici adı (8 byte)
    /// - Ürün adı (16 byte)
    /// - Revizyon (4 byte)
    pub fn inquiry(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 36, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_INQUIRY;
        cbw.cb[1] = 0; // EVPD=0 (Vital Product Data yoktur)
        cbw.cb[2] = 0; // Sayfa kodu (EVPD=0 ise kullanılmaz)
        cbw.cb[3] = 0; // Rezerve
        cbw.cb[4] = 36; // Ayrılan boyut (Allocation Length)
        cbw.cb[5] = 0; // Kontrol
        cbw
    }

    /// TEST_UNIT_READY komutu için CBW oluşturur.
    ///
    /// Veri aktarımı yoktur (transfer_length=0, direction=OUT).
    /// CSW status=0 ise cihaz hazır; 1 ise REQUEST_SENSE ile hata detayı alınır.
    pub fn test_unit_ready(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 6);
        cbw.cb[0] = SCSI_TEST_UNIT_READY;
        cbw
    }

    /// REQUEST_SENSE komutu için CBW oluşturur.
    ///
    /// Bir önceki başarısız komutun hata bilgisini alır.
    /// Yanıt: 18 byte (ScsiSenseData yapısı)
    pub fn request_sense(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 18, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_REQUEST_SENSE;
        cbw.cb[4] = 18; // Ayrılan boyut
        cbw
    }

    /// START_STOP_UNIT komutu için CBW oluşturur.
    ///
    /// `eject=true, start=false` → Ortamı çıkar (optik disk kapağını aç)
    /// `eject=false, start=true` → Ortamı yükle/başlat
    /// `cb[4]` bit0=Start, bit1=LoEj (Load/Eject)
    pub fn start_stop_unit(tag: u32, lun: u8, eject: bool, start: bool) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 6);
        cbw.cb[0] = SCSI_START_STOP_UNIT;
        cbw.cb[1] = 0; // Immed=0 (komut tamamlanana kadar bekle)
        cbw.cb[2] = 0; // Rezerve
        cbw.cb[3] = 0; // Güç koşulları
        cbw.cb[4] = (if start { 1 } else { 0 }) | (if eject { 2 } else { 0 });
        cbw
    }

    /// SYNCHRONIZE_CACHE(10) komutu için CBW oluşturur.
    ///
    /// Yazma önbelleğini diske boşaltır (flush).
    /// Cihaz kapatılmadan veya media çıkarılmadan önce çağrılmalıdır.
    pub fn synchronize_cache(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 10);
        cbw.cb[0] = SCSI_SYNCHRONIZE_CACHE_10;
        cbw
    }

    /// MODE_SENSE(6) komutu için CBW oluşturur.
    ///
    /// `page_code`: Hangi mod sayfası alınacak?
    /// Yanıt: 4 byte (mod başlığı)
    pub fn mode_sense6(tag: u32, lun: u8, page_code: u8) -> Self {
        let mut cbw = Self::new(tag, 4, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_MODE_SENSE_6;
        cbw.cb[2] = page_code & 0x3F; // Sayfa kodu (bit6=PC, bit5-0=sayfa)
        cbw.cb[4] = 4; // Ayrılan boyut
        cbw
    }

    pub fn to_bot_bytes(&self) -> [u8; 31] {
        let signature = self.signature;
        let tag = self.tag;
        let transfer_length = self.transfer_length;
        let flags = self.flags;
        let lun = self.lun;
        let cb_length = self.cb_length;
        let cb = self.cb;

        let mut bytes = [0u8; 31];
        bytes[0..4].copy_from_slice(&signature.to_le_bytes());
        bytes[4..8].copy_from_slice(&tag.to_le_bytes());
        bytes[8..12].copy_from_slice(&transfer_length.to_le_bytes());
        bytes[12] = flags;
        bytes[13] = lun & 0x0F;
        bytes[14] = cb_length.min(16);
        bytes[15..31].copy_from_slice(&cb);
        bytes
    }
}

// ============================================================================
// KOMUT DURUM SARMAYICI (Command Status Wrapper - CSW)
// Cihazdan hoste gelen 13-byte durum yanıtı
// ============================================================================

/// Komut Durum Sarmalayıcı (CSW) - 13 byte.
///
/// Her CBW'ye karşılık cihaz bir CSW gönderir.
/// `dCSWTag` CBW'deki `dCBWTag` ile eşleşmeli; eşleşmiyorsa protokol hatası.
/// `dCSWDataResidue`: İstenilen byte sayısı ile aktarılan byte arasındaki fark.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandStatusWrapper {
    /// Sabit imza: 0x53425355 ("USBS")
    pub signature: u32,
    /// Etiket: gönderilen CBW'nin tag değeriyle eşleşmeli
    pub tag: u32,
    /// Veri artığı: transfer_length - gerçekte aktarılan (byte)
    pub data_residue: u32,
    /// Durum kodu: 0=Geçti, 1=Başarısız, 2=Faz Hatası
    pub status: u8,
}

/// CSW durum kodları.
///
/// `Passed`: Komut başarıyla tamamlandı.
/// `Failed`: Komut başarısız; `REQUEST_SENSE` ile hata detayı alınabilir.
/// `PhaseError`: Protokol faz hatası; cihaz sıfırlanmalı (MSC_RESET).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CswStatus {
    /// Komut başarıyla tamamlandı
    Passed = 0x00,
    /// Komut başarısız (REQUEST_SENSE ile hata detayı alınabilir)
    Failed = 0x01,
    /// Faz hatası: cihaz sıfırlanmalı
    PhaseError = 0x02,
}

impl CommandStatusWrapper {
    pub fn signature(&self) -> u32 {
        self.signature
    }

    pub fn tag(&self) -> u32 {
        self.tag
    }

    pub fn data_residue(&self) -> u32 {
        self.data_residue
    }

    /// Komut geçti mi? (status == 0)
    pub fn passed(&self) -> bool {
        self.status == CswStatus::Passed as u8
    }

    /// Komut başarısız mı? (status == 1)
    pub fn failed(&self) -> bool {
        self.status == CswStatus::Failed as u8
    }

    /// Durum kodunu döndürür.
    pub fn status(&self) -> CswStatus {
        match self.status {
            0x00 => CswStatus::Passed,
            0x01 => CswStatus::Failed,
            _ => CswStatus::PhaseError,
        }
    }
}

fn parse_csw_bytes(
    bytes: &[u8; 13],
    expected_tag: u32,
    expected_transfer_length: u32,
) -> Result<CommandStatusWrapper, UsbError> {
    let signature = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let tag = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let data_residue = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let status = bytes[12];

    if signature != CSW_SIGNATURE || tag != expected_tag {
        return Err(UsbError::TransferError);
    }
    if data_residue > expected_transfer_length {
        return Err(UsbError::DataOverrun);
    }
    if status > CswStatus::PhaseError as u8 {
        return Err(UsbError::TransferError);
    }

    Ok(CommandStatusWrapper {
        signature,
        tag,
        data_residue,
        status,
    })
}

// ============================================================================
// SCSI INQUIRY VERİSİ
// Cihaz kimliği ve yetenekleri (minimum 36 byte)
// ============================================================================

/// SCSI INQUIRY yanıtı - en az 36 byte.
///
/// ## Alan Açıklamaları
///
/// `peripheral_device_type` bit4-0: Cihaz tipi (0x00=disk, 0x05=CD-ROM, vb.)
/// `removable_media` bit7: 1 ise çıkarılabilir ortam (USB flash bellek gibi)
/// `version`: SCSI standart sürümü (0x04=SPC-2, 0x05=SPC-3)
/// `vendor_id` (8 byte): Üretici adı ASCII, sağdan boşlukla doldurulmuş
/// `product_id` (16 byte): Ürün adı ASCII
/// `product_revision` (4 byte): Revizyon kodu ASCII
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiInquiry {
    /// Çevre birimi cihaz tipi (bit4-0: tür, bit6-5: niteleyici)
    pub peripheral_device_type: u8,
    /// Ortam çıkarılabilir mi? (bit7=1 ise evet)
    pub removable_media: u8,
    /// SCSI sürüm uyumluluk seviyesi
    pub version: u8,
    /// Yanıt veri formatı (0x02: SCSI-2 uyumlu)
    pub response_format: u8,
    /// Ek veri uzunluğu (bu bayttan itibaren, N-4)
    pub additional_length: u8,
    /// Rezerve (3 byte)
    pub reserved1: [u8; 3],
    /// Üretici kimliği - ASCII, 8 byte, sağ-hizalı boşluklu
    pub vendor_id: [u8; 8],
    /// Ürün kimliği - ASCII, 16 byte, sağ-hizalı boşluklu
    pub product_id: [u8; 16],
    /// Ürün revizyon seviyesi - ASCII, 4 byte
    pub product_revision: [u8; 4],
}

impl ScsiInquiry {
    /// Cihaz tipini insan okunabilir dize olarak döndürür.
    ///
    /// `peripheral_device_type & 0x1F` → alt 5 bit tipi belirtir.
    pub fn device_type(&self) -> &'static str {
        match self.peripheral_device_type & 0x1F {
            0x00 => "Direct Access (SBC)",
            0x01 => "Sequential Access (SSC)",
            0x02 => "Printer",
            0x03 => "Processor",
            0x04 => "Write Once (SBC)",
            0x05 => "CD-ROM (MMC)",
            0x06 => "Scanner",
            0x07 => "Optical Memory (SBC)",
            0x08 => "Medium Changer",
            0x09 => "Communications",
            0x0A => "ASC IT8",
            0x0B => "ASC IT8",
            0x0C => "Array Controller",
            0x0D => "Enclosure Services",
            0x0E => "Simplified Direct Access",
            0x0F => "Optical Card",
            0x10 => "Object Based Storage",
            0x11 => "Automation/Drive Interface",
            0x1E => "Well Known Logical Unit",
            0x1F => "Unknown",
            _ => "Reserved",
        }
    }

    /// Üretici kimliğini sağ boşlukları kırparak döndürür.
    ///
    /// `trim_end()`: ASCII alanlarında sağ boşluk dolgusu standart bir uygulamadır.
    pub fn vendor_id_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id)
            .unwrap_or("Unknown")
            .trim_end()
    }

    /// Ürün kimliğini sağ boşlukları kırparak döndürür.
    pub fn product_id_str(&self) -> &str {
        core::str::from_utf8(&self.product_id)
            .unwrap_or("Unknown")
            .trim_end()
    }
}

// ============================================================================
// SCSI READ CAPACITY VERİSİ
// Disk kapasitesi (son LBA + blok boyutu)
// ============================================================================

/// SCSI READ_CAPACITY(10) yanıtı - 8 byte.
///
/// ## Big-Endian Uyarısı
///
/// USB cihazlar bu yanıtı big-endian formatında gönderir.
/// `u32::from_be()` ile little-endian'a çevrilmeli.
///
/// ## Hesaplama Örneği
///
/// `last_lba=0x00EFFFFF, block_length=0x200`:
/// Toplam blok: 0x00F00000 = 15_728_640
/// Toplam byte: 15_728_640 × 512 = ~8 GB
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapacity10 {
    /// Son mantıksal blok adresi (big-endian; dönüşüm gerekli)
    pub last_lba: u32,
    /// Blok boyutu (byte; genellikle 512)
    pub block_length: u32,
}

impl ScsiReadCapacity10 {
    /// Toplam kapasiteyi byte cinsinden hesaplar.
    ///
    /// Formül: `(last_lba + 1) * block_length`
    /// +1: son LBA dahil, 0'dan başlayan indeks.
    pub fn total_bytes(&self) -> u64 {
        (self.last_lba as u64 + 1) * self.block_length as u64
    }

    /// Toplam kapasiteyi MB (Mebibyte, 1024^2) cinsinden döndürür.
    pub fn total_mb(&self) -> u64 {
        self.total_bytes() / (1024 * 1024)
    }

    /// Toplam kapasiteyi GB (Gibibyte, 1024^3) cinsinden döndürür.
    pub fn total_gb(&self) -> u64 {
        self.total_bytes() / (1024 * 1024 * 1024)
    }
}

// ============================================================================
// SCSI SENSE VERİSİ
// Hata detay bilgisi (REQUEST_SENSE yanıtı, 18 byte sabit format)
// ============================================================================

/// SCSI Sense verisi - 18 byte (sabit format, response_code=0x70/0x71).
///
/// ## Hata Kodlama Hiyerarşisi
///
/// ```
/// sense_key (4 bit): Genel hata kategorisi (SenseKey enum)
///     ↓
/// asc (1 byte): Ek His Kodu (Additional Sense Code)
///     ↓
/// ascq (1 byte): Ek His Kodu Niteleyicisi (ASCQ)
/// ```
///
/// Örnek: sense_key=0x03 (Medium Error), asc=0x11, ascq=0x00 → "Unrecovered Read Error"
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiSenseData {
    /// Yanıt kodu: 0x70=geçerli, 0x71=ertelenmiş hata
    pub response_code: u8,
    /// Rezerve
    pub reserved: u8,
    /// His anahtarı (bit3-0: SenseKey; bit6-4: ILI, EOM, Filemark; bit7: Valid)
    pub sense_key: u8,
    /// Bilgi alanı (4 byte; Valid=1 ise mantıksal blok adresi veya başka bilgi)
    pub information: [u8; 4],
    /// Ek veri uzunluğu (bu bayttan itibaren: N - 7)
    pub additional_sense_length: u8,
    /// Komuta özgü bilgi (4 byte)
    pub cmd_specific: [u8; 4],
    /// Ek His Kodu (Additional Sense Code - ASC)
    pub asc: u8,
    /// Ek His Kodu Niteleyicisi (ASCQ)
    pub ascq: u8,
    /// Değiştirilebilir Birim Kodu (FRU - Field Replaceable Unit)
    pub fru: u8,
    /// His anahtarı özgü bilgi (3 byte)
    pub sense_key_specific: [u8; 3],
}

/// SCSI His Anahtarı (Sense Key) - genel hata kategorisini belirtir.
///
/// Bu değer `sense_key & 0x0F` ile okunur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SenseKey {
    /// Hata yok (başarılı veya bilgi niteliğinde)
    NoSense = 0x00,
    /// Kurtarılan hata (komut tamamlandı, ancak uyarı var)
    RecoveredError = 0x01,
    /// Hazır değil (cihaz komutu şu an kabul edemiyor)
    NotReady = 0x02,
    /// Ortam hatası (okuma/yazma hatası, bozuk sektör)
    MediumError = 0x03,
    /// Donanım hatası (mekanik arıza, elektronik sorun)
    HardwareError = 0x04,
    /// Geçersiz istek (desteklenmeyen komut, hatalı parametre)
    IllegalRequest = 0x05,
    /// Birim dikkati (ortam değişti, sıfırlama yapıldı)
    UnitAttention = 0x06,
    /// Veri koruması (yazma korumalı ortam)
    DataProtect = 0x07,
    /// Boş kontrol (ortam sonuna ulaşıldı)
    BlankCheck = 0x08,
    /// Üreticiye özgü hata kodu
    VendorSpecific = 0x09,
    /// Kopyalama durduruldu
    CopyAborted = 0x0A,
    /// Komut durduruldu (host tarafından veya dahili hata)
    AbortedCommand = 0x0B,
    /// Birim taşması (yazma kapasitesi aşıldı)
    VolumeOverflow = 0x0D,
    /// Karşılaştırma uyuşmazlığı (doğrulama başarısız)
    Miscompare = 0x0E,
}

impl ScsiSenseData {
    /// His anahtarını `SenseKey` enum'una dönüştürerek döndürür.
    ///
    /// `& 0x0F`: Alt 4 bit his anahtarıdır; üst bitler ILI, EOM, Filemark, Valid.
    pub fn sense_key(&self) -> SenseKey {
        match self.sense_key & 0x0F {
            0x00 => SenseKey::NoSense,
            0x01 => SenseKey::RecoveredError,
            0x02 => SenseKey::NotReady,
            0x03 => SenseKey::MediumError,
            0x04 => SenseKey::HardwareError,
            0x05 => SenseKey::IllegalRequest,
            0x06 => SenseKey::UnitAttention,
            0x07 => SenseKey::DataProtect,
            0x08 => SenseKey::BlankCheck,
            0x09 => SenseKey::VendorSpecific,
            0x0A => SenseKey::CopyAborted,
            0x0B => SenseKey::AbortedCommand,
            0x0D => SenseKey::VolumeOverflow,
            0x0E => SenseKey::Miscompare,
            _ => SenseKey::NoSense,
        }
    }
}

// ============================================================================
// YIĞIN DEPOLAMA SÜRÜCÜSÜ
// Cihaz başlatma, BBB aktarım yönetimi ve SCSI komut gönderimi
// ============================================================================

/// Yığın depolama cihazı sürücüsü.
///
/// ## Yaşam Döngüsü
///
/// ```
/// MassStorageDriver::new() → init() → [hazır]
///     ↓ init() adımları:
///   1. Bulk IN + Bulk OUT endpoint'leri bul
///   2. MSC_RESET (cihaz ve host aynı duruma gelir)
///   3. GET_MAX_LUN (kaç LUN var?)
///   4. TEST_UNIT_READY × 10 (cihaz hazır olana kadar bekle)
///   5. INQUIRY (üretici/model bilgisi)
///   6. READ_CAPACITY_10 (disk boyutu)
///   7. initialized = true
/// ```
///
/// ## Etiket Yönetimi
///
/// `next_tag: AtomicU32` her CBW için artan ve özgün bir etiket sağlar.
/// Atomik sayaç çok çekirdekli kullanım için thread-safe'dir.
pub struct MassStorageDriver {
    /// USB cihaz referansı
    pub device: UsbDevice,
    /// Arabirim numarası (MSC arabirimi)
    pub interface: u8,
    /// Toplam LUN sayısı (1 + GET_MAX_LUN yanıtı)
    pub lun_count: u8,
    /// Şu anda seçili LUN numarası
    pub current_lun: u8,
    /// Blok boyutu (genellikle 512 byte)
    pub block_size: u32,
    /// Toplam blok sayısı (READ_CAPACITY'den)
    pub block_count: u64,
    /// Bulk IN uç noktası (cihazdan veri alma)
    pub bulk_in: Option<UsbEndpoint>,
    /// Bulk OUT uç noktası (cihaza komut/veri gönderme)
    pub bulk_out: Option<UsbEndpoint>,
    /// Bir sonraki CBW etiketi (AtomicU32: thread-safe sayaç)
    next_tag: AtomicU32,
    /// Cihaz başlatıldı mı?
    pub initialized: AtomicBool,
    /// En son REQUEST_SENSE yanıtı
    last_sense: Mutex<ScsiSenseData>,
    /// INQUIRY verisinin önbelleği
    inquiry_data: Mutex<Option<ScsiInquiry>>,
}

enum BotDataBuffer<'a> {
    In(&'a mut [u8]),
    Out(&'a [u8]),
}

impl MassStorageDriver {
    /// Yeni sürücü örneği oluşturur.
    ///
    /// Varsayılan değerler: lun_count=1, block_size=512 byte.
    /// `next_tag`: 1'den başlar (0 rezerve, bazı cihazlar reddedebilir).
    pub fn new(device: UsbDevice, interface: u8) -> Self {
        Self {
            device,
            interface,
            lun_count: 1,
            current_lun: 0,
            block_size: 512,
            block_count: 0,
            bulk_in: None,
            bulk_out: None,
            next_tag: AtomicU32::new(1),
            initialized: AtomicBool::new(false),
            last_sense: Mutex::new(ScsiSenseData {
                response_code: 0,
                reserved: 0,
                sense_key: 0,
                information: [0; 4],
                additional_sense_length: 0,
                cmd_specific: [0; 4],
                asc: 0,
                ascq: 0,
                fru: 0,
                sense_key_specific: [0; 3],
            }),
            inquiry_data: Mutex::new(None),
        }
    }

    /// Bir sonraki CBW etiketi alır (atomik artırma).
    ///
    /// `fetch_add(1, SeqCst)`: Mevcut değeri döndür, ardından 1 artır.
    /// SeqCst: En güçlü bellek sıralaması; çok çekirdekli sistemlerde kesin sıra.
    fn next_tag(&self) -> u32 {
        self.next_tag.fetch_add(1, Ordering::SeqCst)
    }

    /// USB yığın depolama cihazını başlatır.
    ///
    /// Adım sırası kritiktir:
    /// - Sıfırlamadan önce endpoint bulunmalı
    /// - LUN sorgusu sıfırlamadan sonra yapılmalı
    /// - Cihaz hazırlık kontrolü LUN tespitinden sonra yapılmalı
    pub fn init(&mut self) -> Result<(), UsbError> {
        // Arabirim uç noktalarını bul (Bulk IN + Bulk OUT)
        for iface in &self.device.interfaces {
            if iface.interface_number == self.interface {
                for ep in &iface.endpoints {
                    if ep.transfer_type == UsbTransferType::Bulk {
                        if ep.direction == UsbDirection::In {
                            self.bulk_in = Some(*ep); // Veri alma
                        } else {
                            self.bulk_out = Some(*ep); // Komut/veri gönderme
                        }
                    }
                }
                break;
            }
        }

        if self.bulk_in.is_none() || self.bulk_out.is_none() {
            crate::serial_println!("[MSC] No bulk endpoints found");
            return Err(UsbError::NoDevice);
        }

        // Bulk-Only Mass Storage Reset gönder
        self.reset()?;

        // LUN sayısını sorgula (GET_MAX_LUN + 1)
        self.lun_count = self.get_max_lun()? + 1;
        crate::serial_println!(
            "[MSC] Max LUN: {} ({} logical unit(s))",
            self.lun_count - 1,
            self.lun_count
        );

        // Cihazın hazır hale gelmesini bekle (ilk güç açık sonrası ortam yükleme gecikmesi)
        let mut ready = false;
        for _ in 0..10 {
            if self.test_unit_ready(0).is_ok() {
                ready = true;
                break;
            }
            // Kısa gecikme (busy-wait)
            for _ in 0..100_000 {
                core::hint::spin_loop();
            }
        }

        if !ready {
            crate::serial_println!("[MSC] Device not ready after reset");
        }

        // INQUIRY ile cihaz kimliğini al
        if let Ok(inquiry) = self.inquiry(0) {
            crate::serial_println!(
                "[MSC] {} {} (type: {})",
                inquiry.vendor_id_str(),
                inquiry.product_id_str(),
                inquiry.device_type()
            );
            *self.inquiry_data.lock() = Some(inquiry);
        }

        // READ_CAPACITY ile disk boyutunu öğren
        if let Ok(capacity) = self.read_capacity(0) {
            self.block_size = capacity.block_length;
            self.block_count = (capacity.last_lba as u64) + 1;
            crate::serial_println!(
                "[MSC] Capacity: {} MB ({} blocks x {} bytes)",
                capacity.total_mb(),
                self.block_count,
                self.block_size
            );
        }

        self.initialized.store(true, Ordering::SeqCst);
        crate::serial_println!("[MSC] Device initialized");
        Ok(())
    }

    /// Bulk-Only Mass Storage Reset gönderir.
    ///
    /// Setup paketi:
    /// - `request_type=0x21`: Host→Device, Sınıf, Arabirim
    /// - `request=0xFF`: MSC_RESET
    /// - `value=0, index=arabirim, length=0`
    ///
    /// Ardından Bulk IN ve Bulk OUT endpoint HALT durumları temizlenmeli.
    pub fn reset(&self) -> Result<(), UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0x21, // Host'tan cihaza, sınıf, arabirim
            request: MSC_RESET,
            value: 0,
            index: self.interface as u16,
            length: 0,
        };
        self.device.control_transfer(setup, None)?;

        // BOT reset sonrası Bulk IN/Bulk OUT halt clear (CLEAR_FEATURE ENDPOINT_HALT).
        if let Some(ep) = self.bulk_in {
            self.clear_endpoint_halt(ep.address)?;
        }
        if let Some(ep) = self.bulk_out {
            self.clear_endpoint_halt(ep.address)?;
        }
        Ok(())
    }

    /// Maksimum LUN değerini sorgular.
    ///
    /// Setup paketi:
    /// - `request_type=0xA1`: Device→Host, Sınıf, Arabirim
    /// - `request=0xFE`: GET_MAX_LUN
    /// - `length=1`: 1 byte yanıt (max LUN değeri)
    ///
    /// USB belleği genellikle 0 döndürür (tek LUN).
    pub fn get_max_lun(&self) -> Result<u8, UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0xA1, // Device'tan host'a, sınıf, arabirim
            request: MSC_GET_MAX_LUN,
            value: 0,
            index: self.interface as u16,
            length: 1,
        };
        let mut max_lun = [0u8; 1];
        match self.device.control_transfer(setup, Some(&mut max_lun)) {
            Ok(()) => Ok(max_lun[0]),
            // BOT spesinde bazı cihazlar GET_MAX_LUN'u desteklemez ve STALL döner.
            Err(UsbError::Stall) => Ok(0),
            Err(err) => Err(err),
        }
    }

    fn clear_endpoint_halt(&self, endpoint_address: u8) -> Result<(), UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0x02, // Host->Device | Standard | Endpoint
            request: 0x01,      // CLEAR_FEATURE
            value: 0,           // ENDPOINT_HALT
            index: endpoint_address as u16,
            length: 0,
        };
        self.device.control_transfer(setup, None)
    }

    /// CBW gönderir, veri transferi yapar ve CSW alır.
    ///
    /// ## BBB Şelale Protokolü
    ///
    /// 1. CBW → Bulk OUT (31 byte sabit uzunluk)
    /// 2. Veri aktarımı (cbw.flags'e göre yön belirlenir):
    ///    - flags & 0x80 = 1 → Bulk IN (cihazdan veri al)
    ///    - flags & 0x80 = 0 → Bulk OUT (cihaza veri gönder)
    /// 3. CSW ← Bulk IN (13 byte sabit uzunluk)
    ///
    /// Gerçek uygulamada her adım Bulk transfer ile gerçekleştirilir.
    fn execute_command(
        &self,
        cbw: &CommandBlockWrapper,
        data: Option<BotDataBuffer<'_>>,
    ) -> Result<CommandStatusWrapper, UsbError> {
        let bulk_out = self.bulk_out.ok_or(UsbError::NoDevice)?;
        let bulk_in = self.bulk_in.ok_or(UsbError::NoDevice)?;
        let transfer_length = cbw.transfer_length;
        let transfer_len = transfer_length as usize;
        let data_in = (cbw.flags & 0x80) != 0;
        let tag = cbw.tag;

        let cbw_bytes = cbw.to_bot_bytes();
        let cbw_sent = self
            .device
            .bulk_transfer_out(bulk_out, &cbw_bytes, "MSC BOT CBW")?;
        if cbw_sent != cbw_bytes.len() {
            return Err(UsbError::DataUnderrun);
        }

        if transfer_len != 0 {
            match (data_in, data) {
                (true, Some(BotDataBuffer::In(buf))) if buf.len() >= transfer_len => {
                    let _ = self.device.bulk_transfer_in(
                        bulk_in,
                        &mut buf[..transfer_len],
                        "MSC BOT data IN",
                    )?;
                }
                (false, Some(BotDataBuffer::Out(buf))) if buf.len() >= transfer_len => {
                    let _ = self.device.bulk_transfer_out(
                        bulk_out,
                        &buf[..transfer_len],
                        "MSC BOT data OUT",
                    )?;
                }
                _ => return Err(UsbError::DataUnderrun),
            }
        }

        let mut csw_bytes = [0u8; 13];
        let csw_len = self
            .device
            .bulk_transfer_in(bulk_in, &mut csw_bytes, "MSC BOT CSW")?;
        if csw_len != csw_bytes.len() {
            return Err(UsbError::DataUnderrun);
        }
        let csw = parse_csw_bytes(&csw_bytes, tag, transfer_length)?;
        if csw.status() == CswStatus::PhaseError {
            let _ = self.reset();
            return Err(UsbError::TransferError);
        }
        Ok(csw)
    }

    /// Cihazın hazır olup olmadığını test eder.
    ///
    /// CSW status=Passed → hazır
    /// CSW status=Failed → henüz hazır değil; REQUEST_SENSE ile detay alınabilir
    pub fn test_unit_ready(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::test_unit_ready(self.next_tag(), lun);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            // Hata detayını al (sessizce yoksay)
            let _ = self.request_sense(lun);
            Err(UsbError::DeviceNotResponding)
        }
    }

    /// Sense verisini alır (hata detayı).
    ///
    /// `unsafe core::slice::from_raw_parts_mut`: ScsiSenseData yapısını
    /// doğrudan ham byte tamponu olarak kullanmak için güvensiz erişim gerekir.
    /// `#[repr(C, packed)]` bu dönüşümü tanımsız davranıştan kurtarır.
    pub fn request_sense(&self, lun: u8) -> Result<ScsiSenseData, UsbError> {
        let cbw = CommandBlockWrapper::request_sense(self.next_tag(), lun);
        let mut sense = ScsiSenseData {
            response_code: 0x70, // Sabit format, geçerli veri
            reserved: 0,
            sense_key: 0,
            information: [0; 4],
            additional_sense_length: 10,
            cmd_specific: [0; 4],
            asc: 0,
            ascq: 0,
            fru: 0,
            sense_key_specific: [0; 3],
        };

        // ScsiSenseData yapısını ham byte tamponu olarak kullan
        let sense_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut sense as *mut ScsiSenseData as *mut u8,
                core::mem::size_of::<ScsiSenseData>(),
            )
        };

        let csw = self.execute_command(&cbw, Some(BotDataBuffer::In(sense_buf)))?;

        if csw.passed() {
            *self.last_sense.lock() = sense;
            Ok(sense)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// INQUIRY komutunu çalıştırır (cihaz kimliği).
    ///
    /// Dönen `ScsiInquiry` yapısı `vendor_id_str()` ve `product_id_str()`
    /// metodlarıyla insan okunabilir metin olarak erişilebilir.
    pub fn inquiry(&self, lun: u8) -> Result<ScsiInquiry, UsbError> {
        let cbw = CommandBlockWrapper::inquiry(self.next_tag(), lun);
        let mut inquiry = ScsiInquiry {
            peripheral_device_type: 0,
            removable_media: 0,
            version: 0,
            response_format: 0,
            additional_length: 0,
            reserved1: [0; 3],
            vendor_id: [0; 8],
            product_id: [0; 16],
            product_revision: [0; 4],
        };

        // ScsiInquiry yapısını ham byte tamponu olarak kullan
        let inquiry_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut inquiry as *mut ScsiInquiry as *mut u8,
                core::mem::size_of::<ScsiInquiry>(),
            )
        };

        let csw = self.execute_command(&cbw, Some(BotDataBuffer::In(inquiry_buf)))?;

        if csw.passed() {
            Ok(inquiry)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// READ_CAPACITY(10) komutunu çalıştırır.
    ///
    /// ## Big-Endian Dönüşümü
    ///
    /// USB cihazları big-endian gönderir; x86 little-endian kullanır.
    /// `u32::from_be(x)` byte sırasını tersine çevirir.
    pub fn read_capacity(&self, lun: u8) -> Result<ScsiReadCapacity10, UsbError> {
        let cbw = CommandBlockWrapper::read_capacity10(self.next_tag(), lun);
        let mut capacity = ScsiReadCapacity10::default();

        let cap_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut capacity as *mut ScsiReadCapacity10 as *mut u8,
                core::mem::size_of::<ScsiReadCapacity10>(),
            )
        };

        let csw = self.execute_command(&cbw, Some(BotDataBuffer::In(cap_buf)))?;

        if csw.passed() {
            // Big-endian → little-endian dönüşümü
            capacity.last_lba = u32::from_be(capacity.last_lba);
            capacity.block_length = u32::from_be(capacity.block_length);
            Ok(capacity)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Diskten bloklar okur.
    ///
    /// LBA < 2^32 için READ(10) kullanılır.
    /// Büyük diskler için READ(16) (4TB+) gerekir; şu anda desteklenmiyor.
    /// `expected_len = block_count * block_size`: okuma tamponu yeterli boyutta olmalı.
    pub fn read_blocks(
        &self,
        lun: u8,
        lba: u64,
        block_count: u16,
        buf: &mut [u8],
    ) -> Result<usize, UsbError> {
        let expected_len = (block_count as usize) * (self.block_size as usize);
        if buf.len() < expected_len {
            return Err(UsbError::DataOverrun);
        }

        // 2^32 < LBA kontrolü (READ_10 sınırı)
        let cbw = if lba < 0x1_0000_0000 {
            CommandBlockWrapper::read10(self.next_tag(), lun, lba as u32, block_count)
        } else {
            // Büyük LBA: READ(16) gerekli (henüz uygulanmadı)
            return Err(UsbError::Unknown);
        };

        let csw = self.execute_command(&cbw, Some(BotDataBuffer::In(buf)))?;

        if csw.passed() && csw.data_residue() == 0 {
            Ok(expected_len)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Diske bloklar yazar.
    ///
    /// LBA < 2^32 için WRITE(10) kullanılır.
    /// Veri tamponu en az `block_count * block_size` byte olmalıdır.
    pub fn write_blocks(
        &self,
        lun: u8,
        lba: u64,
        block_count: u16,
        data: &[u8],
    ) -> Result<usize, UsbError> {
        let expected_len = (block_count as usize) * (self.block_size as usize);
        if data.len() < expected_len {
            return Err(UsbError::DataUnderrun);
        }

        let cbw = if lba < 0x1_0000_0000 {
            CommandBlockWrapper::write10(self.next_tag(), lun, lba as u32, block_count)
        } else {
            return Err(UsbError::Unknown);
        };

        let csw = self.execute_command(&cbw, Some(BotDataBuffer::Out(&data[..expected_len])))?;

        if csw.passed() && csw.data_residue() == 0 {
            Ok(expected_len)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Yazma önbelleğini diske boşaltır (SYNCHRONIZE_CACHE).
    ///
    /// Cihaz kapatılmadan veya mesafe çıkarılmadan önce çağrılmalıdır.
    pub fn synchronize_cache(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::synchronize_cache(self.next_tag(), lun);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Optik disk ortamını çıkarır (START_STOP_UNIT eject=true).
    pub fn eject(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::start_stop_unit(self.next_tag(), lun, true, false);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Optik disk ortamını yükler (START_STOP_UNIT start=true).
    pub fn load(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::start_stop_unit(self.next_tag(), lun, false, true);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Toplam kapasiteyi byte olarak döndürür.
    pub fn capacity(&self) -> u64 {
        self.block_count * self.block_size as u64
    }

    /// Toplam kapasiteyi MB olarak döndürür.
    pub fn capacity_mb(&self) -> u64 {
        self.capacity() / (1024 * 1024)
    }
}

// ============================================================================
// GLOBAL YIĞIN DEPOLAMA KAYIT DEFTERİ
// BTreeMap: arabirim numarasına göre sıralı sürücü erişimi
// ============================================================================

use alloc::collections::BTreeMap;

/// Global MSC sürücü kayıt defteri.
///
/// `BTreeMap<arabirim_no, Arc<Mutex<MassStorageDriver>>>`:
/// - `BTreeMap`: sıralı anahtar erişimi, arabirim numarasına göre
/// - `Arc`: paylaşımlı sahiplik (dosya sistemi + sürücü yöneticisi)
/// - `Mutex<MassStorageDriver>`: tek anda bir erişimci
lazy_static::lazy_static! {
    static ref MSC_DRIVERS: Mutex<BTreeMap<u8, Arc<Mutex<MassStorageDriver>>>> = Mutex::new(BTreeMap::new());
}

/// MSC sürücüsü kaydeder; arabirim numarasını kimlik olarak döndürür.
pub fn register_msc_driver(device: UsbDevice, interface: u8) -> Result<u8, UsbError> {
    let driver = MassStorageDriver::new(device, interface);
    let id = interface; // Arabirim numarasını kimlik olarak kullan

    MSC_DRIVERS.lock().insert(id, Arc::new(Mutex::new(driver)));
    Ok(id)
}

/// Kimliğe göre MSC sürücüsü döndürür.
///
/// `cloned()`: Arc referans sayacını artırır; kilitlenme olmaz.
pub fn get_msc_driver(id: u8) -> Option<Arc<Mutex<MassStorageDriver>>> {
    MSC_DRIVERS.lock().get(&id).cloned()
}

/// Tüm kayıtlı MSC cihazlarını başlatır.
///
/// Başlatma başarısız olan cihazlar seri porta hata mesajıyla raporlanır.
pub fn init_all_msc() {
    let drivers = MSC_DRIVERS.lock();
    for (id, driver) in drivers.iter() {
        if let Err(e) = driver.lock().init() {
            crate::serial_println!("[MSC] Failed to init device {}: {:?}", id, e);
        }
    }
}

/// Başlatılmış tüm MSC cihazlarını ve kapasitelerini döndürür.
///
/// `initialized.load(SeqCst)`: Başlatılmış cihazlar filtrelenir.
/// Dönen `ScsiReadCapacity10` yapısından `total_mb()` ile kapasite hesaplanabilir.
pub fn get_all_msc() -> Vec<(u8, ScsiReadCapacity10)> {
    let mut devices = Vec::new();
    let drivers = MSC_DRIVERS.lock();

    for (id, driver) in drivers.iter() {
        let d = driver.lock();
        if d.initialized.load(Ordering::SeqCst) {
            let cap = ScsiReadCapacity10 {
                last_lba: (d.block_count - 1) as u32,
                block_length: d.block_size,
            };
            devices.push((*id, cap));
        }
    }

    devices
}

#[cfg(test)]
mod tests {
    use super::{
        parse_csw_bytes, CommandBlockWrapper, CswStatus, MassStorageDriver, CSW_SIGNATURE,
    };
    use crate::drivers::usb::{UsbClass, UsbDevice, UsbError, UsbSpeed};
    use alloc::vec::Vec;

    fn driver_fixture() -> MassStorageDriver {
        MassStorageDriver::new(
            UsbDevice {
                address: 11,
                port: 1,
                speed: UsbSpeed::High,
                controller_bus: 0,
                controller_device: 20,
                controller_function: 0,
                descriptor: None,
                interfaces: Vec::new(),
                device_class: UsbClass::MassStorage,
            },
            0,
        )
    }

    #[test]
    fn reset_uses_control_transfer_path() {
        let driver = driver_fixture();
        assert_eq!(driver.reset(), Err(UsbError::NoDevice));
    }

    #[test]
    fn get_max_lun_uses_control_transfer_path() {
        let driver = driver_fixture();
        assert_eq!(driver.get_max_lun(), Err(UsbError::NoDevice));
    }

    #[test]
    fn cbw_serialization_matches_bot_wire_layout() {
        let mut cbw = CommandBlockWrapper::inquiry(0xAABB_CCDD, 3);
        cbw.cb[15] = 0xEE;
        let bytes = cbw.to_bot_bytes();

        assert_eq!(bytes.len(), 31);
        assert_eq!(&bytes[0..4], &0x4342_5355u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0xAABB_CCDDu32.to_le_bytes());
        assert_eq!(&bytes[8..12], &36u32.to_le_bytes());
        assert_eq!(bytes[12], 0x80);
        assert_eq!(bytes[13], 3);
        assert_eq!(bytes[14], 6);
        assert_eq!(bytes[15], 0x12);
        assert_eq!(bytes[30], 0xEE);
    }

    #[test]
    fn csw_parser_rejects_bad_signature_and_tag() {
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&7u32.to_le_bytes());
        csw[8..12].copy_from_slice(&0u32.to_le_bytes());
        csw[12] = CswStatus::Passed as u8;

        assert!(parse_csw_bytes(&csw, 7, 0).unwrap().passed());
        csw[0] ^= 0xFF;
        assert_eq!(
            parse_csw_bytes(&csw, 7, 0).unwrap_err(),
            UsbError::TransferError
        );
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        assert_eq!(
            parse_csw_bytes(&csw, 8, 0).unwrap_err(),
            UsbError::TransferError
        );
    }

    #[test]
    fn csw_parser_rejects_impossible_residue() {
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&9u32.to_le_bytes());
        csw[8..12].copy_from_slice(&37u32.to_le_bytes());
        csw[12] = CswStatus::Passed as u8;

        assert_eq!(
            parse_csw_bytes(&csw, 9, 36).unwrap_err(),
            UsbError::DataOverrun
        );
    }

    #[test]
    fn execute_command_requires_real_bulk_endpoints() {
        let driver = driver_fixture();
        let cbw = CommandBlockWrapper::test_unit_ready(1, 0);
        assert_eq!(
            driver.execute_command(&cbw, None).unwrap_err(),
            UsbError::NoDevice
        );
    }

    // ========================================================================
    // MSC MEDIA SMOKE TEST (field gate closure)
    // MSC BOT spec: INQUIRY → READ_CAPACITY → READ_10 sequence
    // ========================================================================

    #[test]
    fn cbw_inquiry_command_serialization() {
        // INQUIRY command per SPC-4: opcode=0x12, allocation_length=36
        let cbw = CommandBlockWrapper::inquiry(0xDEAD_BEEF, 0);
        let bytes = cbw.to_bot_bytes();

        assert_eq!(bytes.len(), 31);
        // CBW signature
        assert_eq!(&bytes[0..4], &0x4342_5355u32.to_le_bytes());
        // Tag
        assert_eq!(&bytes[4..8], &0xDEAD_BEEFu32.to_le_bytes());
        // Data transfer length = 36 bytes
        assert_eq!(&bytes[8..12], &36u32.to_le_bytes());
        // Direction = Device-to-Host (bit 7 set)
        assert_eq!(bytes[12], 0x80);
        // LUN = 0
        assert_eq!(bytes[13] & 0x0F, 0);
        // CBWCB length = 6 (INQUIRY)
        assert_eq!(bytes[14], 6);
        // SCSI INQUIRY opcode
        assert_eq!(bytes[15], 0x12);
        // Allocation length
        assert_eq!(bytes[19], 36);
    }

    #[test]
    fn cbw_read_capacity_command_serialization() {
        // READ CAPACITY (10) per SBC-4: opcode=0x25
        let cbw = CommandBlockWrapper::read_capacity10(0xCAFE_BABE, 0);
        let bytes = cbw.to_bot_bytes();

        assert_eq!(bytes.len(), 31);
        assert_eq!(&bytes[4..8], &0xCAFE_BABEu32.to_le_bytes());
        // Data transfer length = 8 bytes (LBA + block size)
        assert_eq!(&bytes[8..12], &8u32.to_le_bytes());
        // Direction = Device-to-Host
        assert_eq!(bytes[12], 0x80);
        // CBWCB length = 10
        assert_eq!(bytes[14], 10);
        // SCSI READ CAPACITY (10) opcode
        assert_eq!(bytes[15], 0x25);
    }

    #[test]
    fn cbw_read10_command_serialization() {
        // READ (10) per SBC-4: opcode=0x28
        let cbw = CommandBlockWrapper::read10(0xFACE_FEED, 0, 0x1000, 8);
        let bytes = cbw.to_bot_bytes();

        assert_eq!(bytes.len(), 31);
        assert_eq!(&bytes[4..8], &0xFACE_FEEDu32.to_le_bytes());
        // Data transfer length = 8 sectors * 512 bytes = 4096
        assert_eq!(&bytes[8..12], &4096u32.to_le_bytes());
        // Direction = Device-to-Host
        assert_eq!(bytes[12], 0x80);
        // CBWCB length = 10
        assert_eq!(bytes[14], 10);
        // SCSI READ (10) opcode
        assert_eq!(bytes[15], 0x28);
        // LBA = 0x1000 (big-endian in CDB)
        assert_eq!(bytes[17], 0x00);
        assert_eq!(bytes[18], 0x00);
        assert_eq!(bytes[19], 0x10);
        assert_eq!(bytes[20], 0x00);
        // Transfer length = 8
        assert_eq!(bytes[22], 0x00);
        assert_eq!(bytes[23], 8);
    }

    #[test]
    fn csw_status_all_variants() {
        // Test all CSW status values per MSC BOT spec
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());

        // CswStatus::Passed = 0x00
        csw[4..8].copy_from_slice(&1u32.to_le_bytes());
        csw[8..12].copy_from_slice(&0u32.to_le_bytes());
        csw[12] = 0x00;
        let result = parse_csw_bytes(&csw, 1, 0).unwrap();
        assert!(result.passed());

        // CswStatus::Failed = 0x01
        csw[12] = 0x01;
        let result = parse_csw_bytes(&csw, 1, 0).unwrap();
        assert!(!result.passed());

        // CswStatus::PhaseError = 0x02
        csw[12] = 0x02;
        let result = parse_csw_bytes(&csw, 1, 0).unwrap();
        assert!(!result.passed());
    }

    #[test]
    fn media_enumeration_sequence_wire_format() {
        // Simulate full media enumeration: INQUIRY → READ_CAPACITY → READ_10
        // This validates the wire format of each command in sequence

        // Step 1: INQUIRY (36 bytes response)
        let inquiry_cbw = CommandBlockWrapper::inquiry(1, 0);
        let inquiry_bytes = inquiry_cbw.to_bot_bytes();
        assert_eq!(inquiry_bytes[15], 0x12); // INQUIRY opcode
        assert_eq!(inquiry_bytes[19], 36); // Allocation length

        // Step 2: READ CAPACITY (8 bytes response)
        let read_cap_cbw = CommandBlockWrapper::read_capacity10(2, 0);
        let read_cap_bytes = read_cap_cbw.to_bot_bytes();
        assert_eq!(read_cap_bytes[15], 0x25); // READ CAPACITY opcode
        assert_eq!(&read_cap_bytes[8..12], &8u32.to_le_bytes());

        // Step 3: READ (10) - read first sector
        let read_cbw = CommandBlockWrapper::read10(3, 0, 0, 1);
        let read_bytes = read_cbw.to_bot_bytes();
        assert_eq!(read_bytes[15], 0x28); // READ (10) opcode
        assert_eq!(&read_bytes[8..12], &512u32.to_le_bytes()); // 1 sector * 512 bytes

        // Verify tag uniqueness across sequence
        assert_ne!(&inquiry_bytes[4..8], &read_cap_bytes[4..8]);
        assert_ne!(&read_cap_bytes[4..8], &read_bytes[4..8]);
        assert_ne!(&inquiry_bytes[4..8], &read_bytes[4..8]);
    }
}
