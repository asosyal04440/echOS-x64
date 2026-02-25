//! # Block Device Abstraction
//!
//! Generic block device interface for storage drivers

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// Block device error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceError {
    /// Device not found
    DeviceNotFound,
    /// I/O error
    IoError,
    /// Invalid sector
    InvalidSector,
    /// Device busy
    DeviceBusy,
    /// Write protected
    WriteProtected,
    /// Timeout
    Timeout,
    /// Unknown error
    Unknown,
}

/// Block device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceType {
    /// Hard disk drive
    Hdd,
    /// Solid state drive
    Ssd,
    /// USB mass storage
    Usb,
    /// Virtual disk (virtio, etc.)
    Virtual,
    /// CD/DVD drive
    Optical,
    /// NVMe drive
    Nvme,
    /// Unknown type
    Unknown,
}

/// Block device trait
pub trait BlockDevice: Send {
    /// Read a single block
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError>;
    
    /// Write a single block
    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError>;
    
    /// Get block size in bytes
    fn block_size(&self) -> u32;
    
    /// Get total block count
    fn block_count(&self) -> u64;
    
    /// Get device name
    fn device_name(&self) -> String;
    
    /// Get device type
    fn device_type(&self) -> BlockDeviceType;
    
    /// Check if device is read-only
    fn is_read_only(&self) -> bool {
        false
    }
    
    /// Flush write cache
    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        Ok(())
    }
    
    /// Read multiple sectors (convenience method)
    fn read_sectors(&mut self, lba: u64, count: u32) -> Result<Vec<u8>, BlockDeviceError> {
        let block_size = self.block_size() as usize;
        let mut buffer = vec![0u8; count as usize * block_size];
        for i in 0..count as u64 {
            let offset = (i as usize) * block_size;
            self.read_block(lba + i, &mut buffer[offset..offset + block_size])?;
        }
        Ok(buffer)
    }
    
    /// Write multiple sectors (convenience method)
    fn write_sectors(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockDeviceError> {
        let block_size = self.block_size() as usize;
        if data.len() % block_size != 0 {
            return Err(BlockDeviceError::IoError);
        }
        let count = data.len() / block_size;
        for i in 0..count {
            let offset = i * block_size;
            self.write_block(lba + i as u64, &data[offset..offset + block_size])?;
        }
        Ok(())
    }
    
    /// Get device capacity in sectors
    fn capacity(&self) -> u64 {
        self.block_count()
    }
    
    /// Get device name as &str
    fn name(&self) -> &str {
        "block device"
    }
}
