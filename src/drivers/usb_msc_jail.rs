//! # USB Mass Storage Jail — TIER 2 USB MSC İzole Sürücüsü
//!
//! USB Mass Storage Class (MSC) cihazları TIER 2 jail sandbox ortamında çalışır.
//! BBB (Bulk-Only Transport) protokolü ile SCSI komutları yürütür.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────┐   SPSC Ring   ┌───────────────┐  USB Bulk  ┌──────────┐
//! │ VFS / Block │◄─────────────►│ USB MSC Jail  │───────────►│ USB Drive│
//! │ Layer       │  JailChannel  │ (Tier 2)      │  IN/OUT    │ (Flash,  │
//! └─────────────┘               └───────────────┘            │  HDD)    │
//!                                                            └──────────┘
//! ```
//!
//! ## Protokol
//!
//! BBB (Bulk-Only Bulk) transport:
//! 1. CBW (Command Block Wrapper) → Bulk-OUT endpoint
//! 2. Data Stage → Bulk-IN veya Bulk-OUT
//! 3. CSW (Command Status Wrapper) ← Bulk-IN endpoint
//!
//! ## SCSI Commands
//!
//! - INQUIRY (0x12)
//! - TEST UNIT READY (0x00)
//! - READ CAPACITY (0x25)
//! - READ(10) (0x28)
//! - WRITE(10) (0x2A)
//! - REQUEST SENSE (0x03)
//! - MODE SENSE (0x1A)

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// BBB Protocol Constants
// ============================================================================

/// CBW signature: "USBC" (0x43425355)
const CBW_SIGNATURE: u32 = 0x43425355;

/// CSW signature: "USBS" (0x53425355)
const CSW_SIGNATURE: u32 = 0x53425355;

/// CBW total size (31 bytes)
const CBW_SIZE: usize = 31;

/// CSW total size (13 bytes)
const CSW_SIZE: usize = 13;

// ============================================================================
// SCSI Command Opcodes
// ============================================================================

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_REQUEST_SENSE: u8 = 0x03;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_MODE_SENSE_6: u8 = 0x1A;
const SCSI_READ_CAPACITY_10: u8 = 0x25;
const SCSI_READ_10: u8 = 0x28;
const SCSI_WRITE_10: u8 = 0x2A;
const SCSI_SYNCHRONIZE_CACHE: u8 = 0x35;

/// CSW Status
const CSW_STATUS_GOOD: u8 = 0;
const CSW_STATUS_FAILED: u8 = 1;
const CSW_STATUS_PHASE_ERROR: u8 = 2;

// ============================================================================
// BBB Structures
// ============================================================================

/// Command Block Wrapper (CBW) — host → device
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CommandBlockWrapper {
    /// Signature: 0x43425355 ("USBC")
    pub signature: u32,
    /// Tag: host-assigned unique command ID
    pub tag: u32,
    /// Data transfer length
    pub data_transfer_length: u32,
    /// Flags: bit 7 = direction (0=OUT, 1=IN)
    pub flags: u8,
    /// LUN (logical unit number)
    pub lun: u8,
    /// SCSI command length (6, 10, 12, or 16)
    pub cb_length: u8,
    /// SCSI command block (16 bytes max)
    pub cb: [u8; 16],
}

impl CommandBlockWrapper {
    pub fn new(tag: u32, data_len: u32, direction_in: bool, lun: u8, cb: &[u8]) -> Self {
        let mut cbw = Self {
            signature: CBW_SIGNATURE,
            tag,
            data_transfer_length: data_len,
            flags: if direction_in { 0x80 } else { 0x00 },
            lun,
            cb_length: cb.len().min(16) as u8,
            cb: [0u8; 16],
        };
        let copy_len = cb.len().min(16);
        cbw.cb[..copy_len].copy_from_slice(&cb[..copy_len]);
        cbw
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; CBW_SIZE] {
        let mut buf = [0u8; CBW_SIZE];
        buf[0..4].copy_from_slice(&self.signature.to_le_bytes());
        buf[4..8].copy_from_slice(&self.tag.to_le_bytes());
        buf[8..12].copy_from_slice(&self.data_transfer_length.to_le_bytes());
        buf[12] = self.flags;
        buf[13] = self.lun;
        buf[14] = self.cb_length;
        buf[15..31].copy_from_slice(&self.cb);
        buf
    }
}

/// Command Status Wrapper (CSW) — device → host
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CommandStatusWrapper {
    /// Signature: 0x53425355 ("USBS")
    pub signature: u32,
    /// Tag: matching CBW tag
    pub tag: u32,
    /// Data residue (bytes not transferred)
    pub data_residue: u32,
    /// Status (0=good, 1=failed, 2=phase error)
    pub status: u8,
}

impl CommandStatusWrapper {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < CSW_SIZE {
            return None;
        }
        let sig = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if sig != CSW_SIGNATURE {
            return None;
        }
        Some(Self {
            signature: sig,
            tag: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            data_residue: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            status: data[12],
        })
    }

    pub fn is_good(&self) -> bool {
        self.status == CSW_STATUS_GOOD
    }
}

// ============================================================================
// USB MSC Device Info
// ============================================================================

/// SCSI INQUIRY response bilgileri
#[derive(Clone, Debug)]
pub struct ScsiInquiryData {
    /// Peripheral device type (0=disk, 5=CD-ROM, etc.)
    pub device_type: u8,
    /// Removable media flag
    pub removable: bool,
    /// Vendor identification (8 bytes)
    pub vendor: String,
    /// Product identification (16 bytes)
    pub product: String,
    /// Product revision (4 bytes)
    pub revision: String,
}

/// USB MSC cihaz bilgisi
#[derive(Clone, Debug)]
pub struct MscDeviceInfo {
    /// USB slot/address
    pub usb_address: u8,
    /// Interface number
    pub interface_num: u8,
    /// Bulk-IN endpoint
    pub ep_in: u8,
    /// Bulk-OUT endpoint
    pub ep_out: u8,
    /// Max LUN
    pub max_lun: u8,
    /// SCSI inquiry data
    pub inquiry: Option<ScsiInquiryData>,
    /// Disk kapasitesi (blok sayısı)
    pub block_count: u64,
    /// Blok boyutu (genellikle 512)
    pub block_size: u32,
    /// Yazılabilir mi?
    pub writable: bool,
}

// ============================================================================
// USB MSC Jail Controller
// ============================================================================

/// USB Mass Storage Jail sürücüsü
pub struct UsbMscJail {
    /// Cihaz listesi
    devices: Vec<MscDeviceInfo>,
    /// Sonraki CBW tag
    next_tag: AtomicU32,
    /// Controller hazır mı?
    ready: AtomicBool,
    /// Jail ID
    pub jail_id: u32,
}

impl UsbMscJail {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            next_tag: AtomicU32::new(1),
            ready: AtomicBool::new(false),
            jail_id: 0,
        }
    }

    /// Yeni tag üretir
    fn alloc_tag(&self) -> u32 {
        self.next_tag.fetch_add(1, Ordering::Relaxed)
    }

    /// SCSI INQUIRY komutu oluşturur
    pub fn build_inquiry_cbw(&self, lun: u8) -> CommandBlockWrapper {
        let cmd = [SCSI_INQUIRY, 0, 0, 0, 36, 0]; // allocation length = 36
        CommandBlockWrapper::new(self.alloc_tag(), 36, true, lun, &cmd)
    }

    /// TEST UNIT READY komutu oluşturur
    pub fn build_test_unit_ready_cbw(&self, lun: u8) -> CommandBlockWrapper {
        let cmd = [SCSI_TEST_UNIT_READY, 0, 0, 0, 0, 0];
        CommandBlockWrapper::new(self.alloc_tag(), 0, false, lun, &cmd)
    }

    /// READ CAPACITY(10) komutu oluşturur
    pub fn build_read_capacity_cbw(&self, lun: u8) -> CommandBlockWrapper {
        let cmd = [SCSI_READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        CommandBlockWrapper::new(self.alloc_tag(), 8, true, lun, &cmd)
    }

    /// READ(10) komutu oluşturur
    pub fn build_read10_cbw(&self, lun: u8, lba: u32, block_count: u16) -> CommandBlockWrapper {
        let lba_bytes = lba.to_be_bytes();
        let count_bytes = block_count.to_be_bytes();
        let cmd = [
            SCSI_READ_10,
            0,
            lba_bytes[0],
            lba_bytes[1],
            lba_bytes[2],
            lba_bytes[3],
            0,
            count_bytes[0],
            count_bytes[1],
            0,
        ];
        let data_len = block_count as u32 * 512;
        CommandBlockWrapper::new(self.alloc_tag(), data_len, true, lun, &cmd)
    }

    /// WRITE(10) komutu oluşturur
    pub fn build_write10_cbw(&self, lun: u8, lba: u32, block_count: u16) -> CommandBlockWrapper {
        let lba_bytes = lba.to_be_bytes();
        let count_bytes = block_count.to_be_bytes();
        let cmd = [
            SCSI_WRITE_10,
            0,
            lba_bytes[0],
            lba_bytes[1],
            lba_bytes[2],
            lba_bytes[3],
            0,
            count_bytes[0],
            count_bytes[1],
            0,
        ];
        let data_len = block_count as u32 * 512;
        CommandBlockWrapper::new(self.alloc_tag(), data_len, false, lun, &cmd)
    }

    /// INQUIRY response parse eder (36 bayt)
    pub fn parse_inquiry_response(&self, data: &[u8]) -> Option<ScsiInquiryData> {
        if data.len() < 36 {
            return None;
        }

        let vendor = core::str::from_utf8(&data[8..16])
            .unwrap_or("")
            .trim()
            .into();
        let product = core::str::from_utf8(&data[16..32])
            .unwrap_or("")
            .trim()
            .into();
        let revision = core::str::from_utf8(&data[32..36])
            .unwrap_or("")
            .trim()
            .into();

        Some(ScsiInquiryData {
            device_type: data[0] & 0x1F,
            removable: data[1] & 0x80 != 0,
            vendor,
            product,
            revision,
        })
    }

    /// READ CAPACITY response parse eder (8 bayt)
    pub fn parse_read_capacity(&self, data: &[u8]) -> Option<(u64, u32)> {
        if data.len() < 8 {
            return None;
        }
        let last_lba = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let block_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        Some((last_lba as u64 + 1, block_size))
    }

    /// Cihaz ekler
    pub fn add_device(&mut self, info: MscDeviceInfo) {
        crate::serial_println!(
            "[USB-MSC-Jail] Device added: addr={}, capacity={}MB",
            info.usb_address,
            (info.block_count * info.block_size as u64) / (1024 * 1024)
        );
        self.devices.push(info);
    }

    /// Cihaz sayısı
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Tüm cihazları listeler
    pub fn list_devices(&self) -> &[MscDeviceInfo] {
        &self.devices
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    /// USB MSC Jail controller
    static ref MSC_JAIL: Mutex<UsbMscJail> = Mutex::new(UsbMscJail::new());
}

/// USB MSC Jail sürücüsünü başlatır
pub fn init() {
    crate::serial_println!("[USB-MSC-Jail] TIER 2 USB Mass Storage Jail driver initialized");
}

/// Cihaz sayısı
pub fn device_count() -> usize {
    MSC_JAIL.lock().device_count()
}
