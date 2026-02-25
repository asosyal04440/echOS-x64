//! # echOS PCI Root for VirtIO
//!
//! PciRoot wrapper for virtio_drivers using PIO (Port I/O) mode

use virtio_drivers::transport::pci::bus::{Cam, PciRoot};

/// Create a PciRoot instance using PIO mode
/// This uses the standard PCI configuration address (0xCF8) and data (0xCFC) ports
pub fn create_pci_root() -> PciRoot {
    // SAFETY: PIO mode doesn't require MMIO base, we use port I/O
    // The PciRoot with Cam::Pio will use x86 port I/O for config access
    unsafe { PciRoot::new(core::ptr::null_mut(), Cam::Pio) }
}

/// Enable Bus Master and Memory Space for device
pub fn enable_device(bus: u8, device: u8, function: u8) {
    // Read current command register
    let command = super::pci::read_config_dword(bus, device, function, 0x04);
    
    // Enable Bus Master, Memory Space, and I/O Space
    let new_command = command | (1 << 0) | (1 << 1) | (1 << 2);
    
    super::pci::write_config_dword(bus, device, function, 0x04, new_command);
}

/// Get BAR (Base Address Register) info
pub fn get_bar(bus: u8, device: u8, function: u8, bar_index: u8) -> (u64, u64) {
    let bar_offset = 0x10 + (bar_index as u16 * 4);
    
    // Read original BAR value
    let original = super::pci::read_config_dword(bus, device, function, bar_offset);
    
    // Write all 1s to get size
    super::pci::write_config_dword(bus, device, function, bar_offset, 0xFFFFFFFF);
    let size_response = super::pci::read_config_dword(bus, device, function, bar_offset);
    
    // Restore original value
    super::pci::write_config_dword(bus, device, function, bar_offset, original);
    
    // Calculate base and size
    let is_io = (original & 1) != 0;
    let is_64bit = !is_io && ((original >> 1) & 3) == 2;
    
    let base = if is_io {
        (original as u64) & 0xFFFFFFFC
    } else if is_64bit {
        // Read next BAR for upper 32 bits
        let upper = if bar_index < 5 {
            super::pci::read_config_dword(bus, device, function, bar_offset + 4)
        } else {
            0
        };
        ((original as u64) & 0xFFFFFFF0) | ((upper as u64) << 32)
    } else {
        (original as u64) & 0xFFFFFFF0
    };
    
    let size = if is_io {
        let mask = !(size_response as u64 & 0xFFFFFFFC);
        mask + 1
    } else if is_64bit {
        let upper_size = if bar_index < 5 {
            let next_offset = bar_offset + 4;
            super::pci::write_config_dword(bus, device, function, next_offset, 0xFFFFFFFF);
            let upper_resp = super::pci::read_config_dword(bus, device, function, next_offset);
            super::pci::write_config_dword(bus, device, function, next_offset, 
                super::pci::read_config_dword(bus, device, function, bar_offset + 4));
            upper_resp
        } else {
            0
        };
        let mask = !(((size_response as u64) & 0xFFFFFFF0) | ((upper_size as u64) << 32));
        mask + 1
    } else {
        let mask = !((size_response as u64) & 0xFFFFFFF0);
        mask + 1
    };
    
    (base, size)
}
