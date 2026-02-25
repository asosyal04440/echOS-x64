//! # echOS USB CDC Driver
//!
//! Communication Device Class for serial/ethernet over USB.

use super::{UsbDevice, UsbError, UsbClass};
use alloc::vec::Vec;

/// CDC device type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdcType {
    Serial,
    Ethernet,
    Wireless,
    NetworkControl,
}

/// CDC device
#[derive(Clone, Debug)]
pub struct CdcDevice {
    pub device: UsbDevice,
    pub cdc_type: CdcType,
    pub control_interface: u8,
    pub data_interface: u8,
    pub in_endpoint: u8,
    pub out_endpoint: u8,
    pub mac_address: [u8; 6],
    pub rx_buffer: Vec<u8>,
    pub tx_buffer: Vec<u8>,
}

impl CdcDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        CdcDevice {
            device,
            cdc_type: CdcType::Serial,
            control_interface: control_if,
            data_interface: data_if,
            in_endpoint: 0,
            out_endpoint: 0,
            mac_address: [0; 6],
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
        }
    }

    /// Set line coding (baud rate, stop bits, parity)
    pub fn set_line_coding(&mut self, baud_rate: u32, stop_bits: u8, parity: u8, data_bits: u8) -> Result<(), UsbError> {
        // Line coding structure: 7 bytes
        // dwDTERate (4), bCharFormat (1), bParityType (1), bDataBits (1)
        let _line_coding = [
            (baud_rate & 0xFF) as u8,
            ((baud_rate >> 8) & 0xFF) as u8,
            ((baud_rate >> 16) & 0xFF) as u8,
            ((baud_rate >> 24) & 0xFF) as u8,
            stop_bits,
            parity,
            data_bits,
        ];
        
        // TODO: Send SET_LINE_CODING control request
        Ok(())
    }

    /// Set control line state (DTR, RTS)
    pub fn set_control_line_state(&mut self, dtr: bool, rts: bool) -> Result<(), UsbError> {
        let value = (dtr as u16) | ((rts as u16) << 1);
        let _ = value;
        // TODO: Send SET_CONTROL_LINE_STATE control request
        Ok(())
    }

    /// Send data
    pub fn send(&mut self, data: &[u8]) -> Result<usize, UsbError> {
        // TODO: Implement USB bulk out transfer
        self.tx_buffer.extend_from_slice(data);
        Ok(data.len())
    }

    /// Receive data
    pub fn receive(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        if self.rx_buffer.is_empty() {
            return Ok(0);
        }
        
        let len = buf.len().min(self.rx_buffer.len());
        buf[..len].copy_from_slice(&self.rx_buffer[..len]);
        self.rx_buffer.drain(..len);
        Ok(len)
    }

    /// Check if data available
    pub fn data_available(&self) -> bool {
        !self.rx_buffer.is_empty()
    }
}

/// CDC-ECM (Ethernet Control Model) device
#[derive(Clone, Debug)]
pub struct CdcEcmDevice {
    pub cdc: CdcDevice,
    pub ethernet_statistics: EthernetStatistics,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EthernetStatistics {
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}

impl CdcEcmDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        let mut cdc = CdcDevice::new(device, control_if, data_if);
        cdc.cdc_type = CdcType::Ethernet;
        
        CdcEcmDevice {
            cdc,
            ethernet_statistics: EthernetStatistics::default(),
        }
    }

    /// Send Ethernet frame
    pub fn send_frame(&mut self, frame: &[u8]) -> Result<usize, UsbError> {
        // CDC-ECM requires length prefix for frames
        let len = frame.len() as u16;
        let mut packet = Vec::with_capacity(frame.len() + 4);
        packet.push((len & 0xFF) as u8);
        packet.push(((len >> 8) & 0xFF) as u8);
        packet.extend_from_slice(frame);
        
        self.cdc.send(&packet)?;
        self.ethernet_statistics.tx_packets += 1;
        self.ethernet_statistics.tx_bytes += frame.len() as u64;
        Ok(frame.len())
    }

    /// Receive Ethernet frame
    pub fn receive_frame(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        let mut temp_buf = [0u8; 2048];
        let len = self.cdc.receive(&mut temp_buf)?;
        
        if len < 2 {
            return Ok(0);
        }
        
        let frame_len = temp_buf[0] as usize | ((temp_buf[1] as usize) << 8);
        let frame_len = frame_len.min(buf.len()).min(len - 2);
        
        buf[..frame_len].copy_from_slice(&temp_buf[2..2 + frame_len]);
        self.ethernet_statistics.rx_packets += 1;
        self.ethernet_statistics.rx_bytes += frame_len as u64;
        Ok(frame_len)
    }

    /// Get MAC address
    pub fn mac_address(&self) -> [u8; 6] {
        self.cdc.mac_address
    }

    /// Set MAC address (from descriptor)
    pub fn set_mac_address(&mut self, mac: [u8; 6]) {
        self.cdc.mac_address = mac;
    }
}

/// CDC-ACM (Abstract Control Model) device for serial
#[derive(Clone, Debug)]
pub struct CdcAcmDevice {
    pub cdc: CdcDevice,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: u8,
}

impl CdcAcmDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        let mut cdc = CdcDevice::new(device, control_if, data_if);
        cdc.cdc_type = CdcType::Serial;
        
        CdcAcmDevice {
            cdc,
            baud_rate: 115200,
            data_bits: 8,
            stop_bits: 1,
            parity: 0,
        }
    }

    /// Configure serial parameters
    pub fn configure(&mut self, baud_rate: u32, data_bits: u8, stop_bits: u8, parity: u8) -> Result<(), UsbError> {
        self.cdc.set_line_coding(baud_rate, stop_bits, parity, data_bits)?;
        self.baud_rate = baud_rate;
        self.data_bits = data_bits;
        self.stop_bits = stop_bits;
        self.parity = parity;
        Ok(())
    }

    /// Write data to serial
    pub fn write(&mut self, data: &[u8]) -> Result<usize, UsbError> {
        self.cdc.send(data)
    }

    /// Read data from serial
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        self.cdc.receive(buf)
    }

    /// Set DTR (Data Terminal Ready)
    pub fn set_dtr(&mut self, state: bool) -> Result<(), UsbError> {
        self.cdc.set_control_line_state(state, false)
    }

    /// Set RTS (Request To Send)
    pub fn set_rts(&mut self, state: bool) -> Result<(), UsbError> {
        self.cdc.set_control_line_state(false, state)
    }
}

/// Find CDC devices
pub fn find_cdc_devices(devices: &[UsbDevice]) -> Vec<CdcDevice> {
    let mut cdc_devices = Vec::new();
    
    for device in devices {
        // Look for CDC interfaces
        let mut control_if: Option<u8> = None;
        let mut data_if: Option<u8> = None;
        
        for iface in &device.interfaces {
            if iface.class == UsbClass::CdcControl {
                control_if = Some(iface.interface_number);
            } else if iface.class == UsbClass::CdcData {
                data_if = Some(iface.interface_number);
            }
        }
        
        if let (Some(ctrl), Some(data)) = (control_if, data_if) {
            cdc_devices.push(CdcDevice::new(device.clone(), ctrl, data));
        }
    }
    
    cdc_devices
}
