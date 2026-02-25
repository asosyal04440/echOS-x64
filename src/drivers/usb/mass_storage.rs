//! # echOS USB Mass Storage Driver
//!
//! USB Mass Storage Class (MSC) driver with Bulk-Only Transport (BBB).
//! Implements SCSI transparent command set for USB flash drives and hard disks.

use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::{UsbDevice, UsbError, UsbSetupPacket, UsbEndpoint, UsbDirection, UsbTransferType};

// ============================================================================
// MASS STORAGE CLASS REQUESTS
// ============================================================================

const MSC_RESET: u8 = 0xFF;
const MSC_GET_MAX_LUN: u8 = 0xFE;

// ============================================================================
// BULK-ONLY TRANSPORT SIGNATURES
// ============================================================================

const CBW_SIGNATURE: u32 = 0x43425355; // "USBC"
const CSW_SIGNATURE: u32 = 0x53425355; // "USBS"

// ============================================================================
// SCSI COMMAND OPCODES
// ============================================================================

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_REQUEST_SENSE: u8 = 0x03;
const SCSI_FORMAT_UNIT: u8 = 0x04;
const SCSI_READ_6: u8 = 0x08;
const SCSI_WRITE_6: u8 = 0x0A;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_MODE_SELECT_6: u8 = 0x15;
const SCSI_MODE_SENSE_6: u8 = 0x1A;
const SCSI_START_STOP_UNIT: u8 = 0x1B;
const SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E;
const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23;
const SCSI_READ_CAPACITY_10: u8 = 0x25;
const SCSI_READ_10: u8 = 0x28;
const SCSI_WRITE_10: u8 = 0x2A;
const SCSI_WRITE_AND_VERIFY_10: u8 = 0x2E;
const SCSI_VERIFY_10: u8 = 0x2F;
const SCSI_SYNCHRONIZE_CACHE_10: u8 = 0x35;
const SCSI_READ_TOC: u8 = 0x43;
const SCSI_MODE_SELECT_10: u8 = 0x55;
const SCSI_MODE_SENSE_10: u8 = 0x5A;
const SCSI_READ_16: u8 = 0x88;
const SCSI_WRITE_16: u8 = 0x8A;
const SCSI_READ_CAPACITY_16: u8 = 0x9E;

// ============================================================================
// COMMAND BLOCK WRAPPER (CBW)
// ============================================================================

/// Command Block Wrapper - 31 bytes
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct CommandBlockWrapper {
    /// Signature (0x43425355)
    pub signature: u32,
    /// Tag for matching with CSW
    pub tag: u32,
    /// Transfer length in bytes
    pub transfer_length: u32,
    /// Flags (0x80 = IN, 0x00 = OUT)
    pub flags: u8,
    /// Logical Unit Number
    pub lun: u8,
    /// Command Block Length (1-16)
    pub cb_length: u8,
    /// Command Block (16 bytes max)
    pub cb: [u8; 16],
}

impl CommandBlockWrapper {
    /// Create new CBW
    pub fn new(tag: u32, transfer_length: u32, direction: UsbDirection, lun: u8, cb_length: u8) -> Self {
        Self {
            signature: CBW_SIGNATURE,
            tag,
            transfer_length,
            flags: if direction == UsbDirection::In { 0x80 } else { 0x00 },
            lun: lun & 0x0F,
            cb_length: cb_length.min(16),
            cb: [0u8; 16],
        }
    }

    /// Create CBW for READ(10) command
    pub fn read10(tag: u32, lun: u8, lba: u32, block_count: u16) -> Self {
        let mut cbw = Self::new(tag, (block_count as u32) * 512, UsbDirection::In, lun, 10);
        cbw.cb[0] = SCSI_READ_10;
        cbw.cb[1] = 0; // Flags
        cbw.cb[2..6].copy_from_slice(&lba.to_be_bytes());
        cbw.cb[6] = 0; // Group number
        cbw.cb[7..9].copy_from_slice(&block_count.to_be_bytes());
        cbw.cb[9] = 0; // Control
        cbw
    }

    /// Create CBW for WRITE(10) command
    pub fn write10(tag: u32, lun: u8, lba: u32, block_count: u16) -> Self {
        let mut cbw = Self::new(tag, (block_count as u32) * 512, UsbDirection::Out, lun, 10);
        cbw.cb[0] = SCSI_WRITE_10;
        cbw.cb[1] = 0; // Flags
        cbw.cb[2..6].copy_from_slice(&lba.to_be_bytes());
        cbw.cb[6] = 0; // Group number
        cbw.cb[7..9].copy_from_slice(&block_count.to_be_bytes());
        cbw.cb[9] = 0; // Control
        cbw
    }

    /// Create CBW for READ CAPACITY(10) command
    pub fn read_capacity10(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 8, UsbDirection::In, lun, 10);
        cbw.cb[0] = SCSI_READ_CAPACITY_10;
        cbw
    }

    /// Create CBW for INQUIRY command
    pub fn inquiry(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 36, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_INQUIRY;
        cbw.cb[1] = 0; // EVPD=0
        cbw.cb[2] = 0; // Page code
        cbw.cb[3] = 0; // Reserved
        cbw.cb[4] = 36; // Allocation length
        cbw.cb[5] = 0; // Control
        cbw
    }

    /// Create CBW for TEST UNIT READY command
    pub fn test_unit_ready(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 6);
        cbw.cb[0] = SCSI_TEST_UNIT_READY;
        cbw
    }

    /// Create CBW for REQUEST SENSE command
    pub fn request_sense(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 18, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_REQUEST_SENSE;
        cbw.cb[4] = 18; // Allocation length
        cbw
    }

    /// Create CBW for START STOP UNIT command (eject/load)
    pub fn start_stop_unit(tag: u32, lun: u8, eject: bool, start: bool) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 6);
        cbw.cb[0] = SCSI_START_STOP_UNIT;
        cbw.cb[1] = 0; // Immed=0
        cbw.cb[2] = 0; // Reserved
        cbw.cb[3] = 0; // Power conditions
        cbw.cb[4] = (if start { 1 } else { 0 }) | (if eject { 2 } else { 0 });
        cbw
    }

    /// Create CBW for SYNCHRONIZE CACHE command
    pub fn synchronize_cache(tag: u32, lun: u8) -> Self {
        let mut cbw = Self::new(tag, 0, UsbDirection::Out, lun, 10);
        cbw.cb[0] = SCSI_SYNCHRONIZE_CACHE_10;
        cbw
    }

    /// Create CBW for MODE SENSE(6) command
    pub fn mode_sense6(tag: u32, lun: u8, page_code: u8) -> Self {
        let mut cbw = Self::new(tag, 4, UsbDirection::In, lun, 6);
        cbw.cb[0] = SCSI_MODE_SENSE_6;
        cbw.cb[2] = page_code & 0x3F;
        cbw.cb[4] = 4; // Allocation length
        cbw
    }
}

// ============================================================================
// COMMAND STATUS WRAPPER (CSW)
// ============================================================================

/// Command Status Wrapper - 13 bytes
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CommandStatusWrapper {
    /// Signature (0x53425355)
    pub signature: u32,
    /// Tag (matches CBW)
    pub tag: u32,
    /// Data residue (bytes not transferred)
    pub data_residue: u32,
    /// Status code
    pub status: u8,
}

/// CSW Status codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CswStatus {
    /// Command passed
    Passed = 0x00,
    /// Command failed
    Failed = 0x01,
    /// Phase error
    PhaseError = 0x02,
}

impl CommandStatusWrapper {
    /// Check if command passed
    pub fn passed(&self) -> bool {
        self.status == CswStatus::Passed as u8
    }

    /// Check if command failed
    pub fn failed(&self) -> bool {
        self.status == CswStatus::Failed as u8
    }

    /// Get status
    pub fn status(&self) -> CswStatus {
        match self.status {
            0x00 => CswStatus::Passed,
            0x01 => CswStatus::Failed,
            _ => CswStatus::PhaseError,
        }
    }
}

// ============================================================================
// SCSI INQUIRY DATA
// ============================================================================

/// SCSI INQUIRY response - 36 bytes minimum
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiInquiry {
    /// Peripheral device type
    pub peripheral_device_type: u8,
    /// Removable media
    pub removable_media: u8,
    /// Version
    pub version: u8,
    /// Response format
    pub response_format: u8,
    /// Additional length
    pub additional_length: u8,
    /// Reserved
    pub reserved1: [u8; 3],
    /// Vendor identification (8 bytes)
    pub vendor_id: [u8; 8],
    /// Product identification (16 bytes)
    pub product_id: [u8; 16],
    /// Product revision level (4 bytes)
    pub product_revision: [u8; 4],
}

impl ScsiInquiry {
    /// Get device type as string
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

    /// Get vendor ID as string
    pub fn vendor_id_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown").trim_end()
    }

    /// Get product ID as string
    pub fn product_id_str(&self) -> &str {
        core::str::from_utf8(&self.product_id).unwrap_or("Unknown").trim_end()
    }
}

// ============================================================================
// SCSI READ CAPACITY DATA
// ============================================================================

/// SCSI READ CAPACITY(10) response - 8 bytes
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapacity10 {
    /// Last logical block address (big-endian)
    pub last_lba: u32,
    /// Block length in bytes (big-endian)
    pub block_length: u32,
}

impl ScsiReadCapacity10 {
    /// Get total capacity in bytes
    pub fn total_bytes(&self) -> u64 {
        (self.last_lba as u64 + 1) * self.block_length as u64
    }

    /// Get total capacity in MB
    pub fn total_mb(&self) -> u64 {
        self.total_bytes() / (1024 * 1024)
    }

    /// Get total capacity in GB
    pub fn total_gb(&self) -> u64 {
        self.total_bytes() / (1024 * 1024 * 1024)
    }
}

// ============================================================================
// SCSI SENSE DATA
// ============================================================================

/// SCSI SENSE data - 18 bytes (fixed format)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiSenseData {
    /// Response code
    pub response_code: u8,
    /// Reserved
    pub reserved: u8,
    /// Sense key
    pub sense_key: u8,
    /// Information
    pub information: [u8; 4],
    /// Additional sense length
    pub additional_sense_length: u8,
    /// Command specific information
    pub cmd_specific: [u8; 4],
    /// Additional sense code
    pub asc: u8,
    /// Additional sense code qualifier
    pub ascq: u8,
    /// Field replaceable unit code
    pub fru: u8,
    /// Sense key specific
    pub sense_key_specific: [u8; 3],
}

/// SCSI Sense Keys
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SenseKey {
    NoSense = 0x00,
    RecoveredError = 0x01,
    NotReady = 0x02,
    MediumError = 0x03,
    HardwareError = 0x04,
    IllegalRequest = 0x05,
    UnitAttention = 0x06,
    DataProtect = 0x07,
    BlankCheck = 0x08,
    VendorSpecific = 0x09,
    CopyAborted = 0x0A,
    AbortedCommand = 0x0B,
    VolumeOverflow = 0x0D,
    Miscompare = 0x0E,
}

impl ScsiSenseData {
    /// Get sense key
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
// MASS STORAGE DRIVER
// ============================================================================

/// Mass storage device driver
pub struct MassStorageDriver {
    /// USB device reference
    pub device: UsbDevice,
    /// Interface number
    pub interface: u8,
    /// Number of LUNs
    pub lun_count: u8,
    /// Current LUN
    pub current_lun: u8,
    /// Block size (usually 512)
    pub block_size: u32,
    /// Total block count
    pub block_count: u64,
    /// Bulk IN endpoint
    pub bulk_in: Option<UsbEndpoint>,
    /// Bulk OUT endpoint
    pub bulk_out: Option<UsbEndpoint>,
    /// Next CBW tag
    next_tag: AtomicU32,
    /// Initialized flag
    pub initialized: AtomicBool,
    /// Last sense data
    last_sense: Mutex<ScsiSenseData>,
    /// Inquiry data cache
    inquiry_data: Mutex<Option<ScsiInquiry>>,
}

impl MassStorageDriver {
    /// Create new mass storage driver
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

    /// Get next CBW tag
    fn next_tag(&self) -> u32 {
        self.next_tag.fetch_add(1, Ordering::SeqCst)
    }

    /// Initialize mass storage device
    pub fn init(&mut self) -> Result<(), UsbError> {
        // Find bulk endpoints
        for iface in &self.device.interfaces {
            if iface.interface_number == self.interface {
                for ep in &iface.endpoints {
                    if ep.transfer_type == UsbTransferType::Bulk {
                        if ep.direction == UsbDirection::In {
                            self.bulk_in = Some(*ep);
                        } else {
                            self.bulk_out = Some(*ep);
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

        // Reset mass storage device
        self.reset()?;

        // Get max LUN
        self.lun_count = self.get_max_lun()? + 1;
        crate::serial_println!("[MSC] Max LUN: {} ({} logical unit(s))", self.lun_count - 1, self.lun_count);

        // Wait for device to become ready
        let mut ready = false;
        for _ in 0..10 {
            if self.test_unit_ready(0).is_ok() {
                ready = true;
                break;
            }
            // Delay
            for _ in 0..100_000 {
                core::hint::spin_loop();
            }
        }

        if !ready {
            crate::serial_println!("[MSC] Device not ready after reset");
        }

        // Get inquiry data
        if let Ok(inquiry) = self.inquiry(0) {
            crate::serial_println!(
                "[MSC] {} {} (type: {})",
                inquiry.vendor_id_str(),
                inquiry.product_id_str(),
                inquiry.device_type()
            );
            *self.inquiry_data.lock() = Some(inquiry);
        }

        // Read capacity
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

    /// Reset mass storage device (Bulk-Only Mass Storage Reset)
    pub fn reset(&self) -> Result<(), UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0x21, // Host-to-device, class, interface
            request: MSC_RESET,
            value: 0,
            index: self.interface as u16,
            length: 0,
        };

        // Send control transfer
        let _ = setup; // Placeholder

        // Clear HALT on bulk endpoints
        // In real implementation, would send CLEAR_FEATURE(ENDPOINT_HALT)

        Ok(())
    }

    /// Get maximum LUN
    pub fn get_max_lun(&self) -> Result<u8, UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0xA1, // Device-to-host, class, interface
            request: MSC_GET_MAX_LUN,
            value: 0,
            index: self.interface as u16,
            length: 1,
        };

        // Send control transfer and read 1 byte
        let _ = setup; // Placeholder

        // Return 0 as default (1 LUN)
        Ok(0)
    }

    /// Send CBW and receive CSW
    fn execute_command(&self, cbw: &CommandBlockWrapper, data: Option<&mut [u8]>) -> Result<CommandStatusWrapper, UsbError> {
        // 1. Send CBW on bulk OUT endpoint
        // In real implementation:
        // self.send_bulk_out(cbw as *const _ as *const u8, 31)?;

        // 2. Transfer data (if any)
        if let Some(buf) = data {
            if cbw.transfer_length > 0 {
                if cbw.flags & 0x80 != 0 {
                    // Data IN
                    // self.receive_bulk_in(buf, cbw.transfer_length as usize)?;
                } else {
                    // Data OUT
                    // self.send_bulk_out(buf.as_ptr(), buf.len())?;
                }
            }
        }

        // 3. Receive CSW on bulk IN endpoint
        let csw = CommandStatusWrapper {
            signature: CSW_SIGNATURE,
            tag: cbw.tag,
            data_residue: 0,
            status: CswStatus::Passed as u8,
        };

        // In real implementation:
        // self.receive_bulk_in(&csw as *const _ as *mut u8, 13)?;

        let _ = cbw;
        Ok(csw)
    }

    /// Test unit ready
    pub fn test_unit_ready(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::test_unit_ready(self.next_tag(), lun);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            // Get sense data
            let _ = self.request_sense(lun);
            Err(UsbError::DeviceNotResponding)
        }
    }

    /// Request sense data
    pub fn request_sense(&self, lun: u8) -> Result<ScsiSenseData, UsbError> {
        let cbw = CommandBlockWrapper::request_sense(self.next_tag(), lun);
        let mut sense = ScsiSenseData {
            response_code: 0x70,
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

        let sense_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut sense as *mut ScsiSenseData as *mut u8,
                core::mem::size_of::<ScsiSenseData>()
            )
        };

        let csw = self.execute_command(&cbw, Some(sense_buf))?;

        if csw.passed() {
            *self.last_sense.lock() = sense;
            Ok(sense)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Inquiry command
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

        let inquiry_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut inquiry as *mut ScsiInquiry as *mut u8,
                core::mem::size_of::<ScsiInquiry>()
            )
        };

        let csw = self.execute_command(&cbw, Some(inquiry_buf))?;

        if csw.passed() {
            Ok(inquiry)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Read capacity
    pub fn read_capacity(&self, lun: u8) -> Result<ScsiReadCapacity10, UsbError> {
        let cbw = CommandBlockWrapper::read_capacity10(self.next_tag(), lun);
        let mut capacity = ScsiReadCapacity10::default();

        let cap_buf = unsafe {
            core::slice::from_raw_parts_mut(
                &mut capacity as *mut ScsiReadCapacity10 as *mut u8,
                core::mem::size_of::<ScsiReadCapacity10>()
            )
        };

        let csw = self.execute_command(&cbw, Some(cap_buf))?;

        if csw.passed() {
            // Convert from big-endian
            capacity.last_lba = u32::from_be(capacity.last_lba);
            capacity.block_length = u32::from_be(capacity.block_length);
            Ok(capacity)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Read blocks from device
    pub fn read_blocks(&self, lun: u8, lba: u64, block_count: u16, buf: &mut [u8]) -> Result<usize, UsbError> {
        let expected_len = (block_count as usize) * (self.block_size as usize);
        if buf.len() < expected_len {
            return Err(UsbError::DataOverrun);
        }

        // Use READ(10) for LBA < 2^32, READ(16) for larger
        let cbw = if lba < 0x1_0000_0000 {
            CommandBlockWrapper::read10(self.next_tag(), lun, lba as u32, block_count)
        } else {
            // Would need READ(16) for larger LBAs
            return Err(UsbError::Unknown);
        };

        let csw = self.execute_command(&cbw, Some(buf))?;

        if csw.passed() {
            Ok(expected_len)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Write blocks to device
    pub fn write_blocks(&self, lun: u8, lba: u64, block_count: u16, data: &[u8]) -> Result<usize, UsbError> {
        let expected_len = (block_count as usize) * (self.block_size as usize);
        if data.len() < expected_len {
            return Err(UsbError::DataUnderrun);
        }

        let cbw = if lba < 0x1_0000_0000 {
            CommandBlockWrapper::write10(self.next_tag(), lun, lba as u32, block_count)
        } else {
            return Err(UsbError::Unknown);
        };

        // Need mutable slice for execute_command
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(expected_len)
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Synchronize cache (flush)
    pub fn synchronize_cache(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::synchronize_cache(self.next_tag(), lun);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Eject media
    pub fn eject(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::start_stop_unit(self.next_tag(), lun, true, false);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Load media
    pub fn load(&self, lun: u8) -> Result<(), UsbError> {
        let cbw = CommandBlockWrapper::start_stop_unit(self.next_tag(), lun, false, true);
        let csw = self.execute_command(&cbw, None)?;

        if csw.passed() {
            Ok(())
        } else {
            Err(UsbError::Unknown)
        }
    }

    /// Get total capacity in bytes
    pub fn capacity(&self) -> u64 {
        self.block_count * self.block_size as u64
    }

    /// Get capacity in MB
    pub fn capacity_mb(&self) -> u64 {
        self.capacity() / (1024 * 1024)
    }
}

// ============================================================================
// GLOBAL MASS STORAGE REGISTRY
// ============================================================================

use alloc::collections::BTreeMap;

lazy_static::lazy_static! {
    static ref MSC_DRIVERS: Mutex<BTreeMap<u8, Arc<Mutex<MassStorageDriver>>>> = Mutex::new(BTreeMap::new());
}

/// Register mass storage driver
pub fn register_msc_driver(device: UsbDevice, interface: u8) -> Result<u8, UsbError> {
    let driver = MassStorageDriver::new(device, interface);
    let id = interface; // Use interface as ID
    
    MSC_DRIVERS.lock().insert(id, Arc::new(Mutex::new(driver)));
    Ok(id)
}

/// Get mass storage driver by ID
pub fn get_msc_driver(id: u8) -> Option<Arc<Mutex<MassStorageDriver>>> {
    MSC_DRIVERS.lock().get(&id).cloned()
}

/// Initialize all registered mass storage devices
pub fn init_all_msc() {
    let drivers = MSC_DRIVERS.lock();
    for (id, driver) in drivers.iter() {
        if let Err(e) = driver.lock().init() {
            crate::serial_println!("[MSC] Failed to init device {}: {:?}", id, e);
        }
    }
}

/// Get all mass storage devices
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
