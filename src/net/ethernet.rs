//! # Ethernet Layer
//!
//! Ethernet frame parsing and construction

use super::{MacAddr, NetError};

/// Ethernet frame header
#[derive(Clone, Copy, Debug)]
pub struct EthernetHeader {
    pub dst: MacAddr,
    pub src: MacAddr,
    pub ether_type: EtherType,
}

/// Ethernet frame with payload
#[derive(Clone, Debug)]
pub struct EthernetFrame<'a> {
    pub header: EthernetHeader,
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    /// Get ether_type from header
    pub fn ether_type(&self) -> EtherType {
        self.header.ether_type
    }
}

/// Ethernet types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum EtherType {
    IPV4 = 0x0800,
    ARP = 0x0806,
    IPV6 = 0x86DD,
    VLAN = 0x8100,
    UNKNOWN = 0,
}

impl EtherType {
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x0800 => EtherType::IPV4,
            0x0806 => EtherType::ARP,
            0x86DD => EtherType::IPV6,
            0x8100 => EtherType::VLAN,
            _ => EtherType::UNKNOWN,
        }
    }
    
    pub fn to_u16(self) -> u16 {
        self as u16
    }
}

impl EthernetHeader {
    /// Header size in bytes
    pub const SIZE: usize = 14;
    
    /// Parse header from bytes
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let dst = MacAddr::new([data[0], data[1], data[2], data[3], data[4], data[5]]);
        let src = MacAddr::new([data[6], data[7], data[8], data[9], data[10], data[11]]);
        let ether_type = EtherType::from_u16(u16::from_be_bytes([data[12], data[13]]));
        
        Ok(EthernetHeader { dst, src, ether_type })
    }
    
    /// Serialize header to bytes
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..6].copy_from_slice(self.dst.as_bytes());
        buf[6..12].copy_from_slice(self.src.as_bytes());
        buf[12..14].copy_from_slice(&self.ether_type.to_u16().to_be_bytes());
        
        Ok(())
    }
    
    /// Create new header
    pub fn new(dst: MacAddr, src: MacAddr, ether_type: EtherType) -> Self {
        EthernetHeader { dst, src, ether_type }
    }
}

impl<'a> EthernetFrame<'a> {
    /// Parse Ethernet frame from bytes
    pub fn parse(data: &'a [u8]) -> Result<Self, NetError> {
        let header = EthernetHeader::parse(data)?;
        let payload = &data[EthernetHeader::SIZE..];
        
        Ok(EthernetFrame { header, payload })
    }
    
    /// Create new frame
    pub fn new(dst: MacAddr, src: MacAddr, ether_type: EtherType, payload: &'a [u8]) -> Self {
        EthernetFrame {
            header: EthernetHeader::new(dst, src, ether_type),
            payload,
        }
    }
    
    /// Serialize frame to bytes
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let total_len = EthernetHeader::SIZE + self.payload.len();
        if buf.len() < total_len {
            return Err(NetError::BufferFull);
        }
        
        self.header.serialize(&mut buf[..EthernetHeader::SIZE])?;
        buf[EthernetHeader::SIZE..total_len].copy_from_slice(self.payload);
        
        Ok(total_len)
    }
    
    /// Get total frame size
    pub fn len(&self) -> usize {
        EthernetHeader::SIZE + self.payload.len()
    }
}

/// Build Ethernet frame for sending
pub fn build_frame(
    dst: MacAddr,
    src: MacAddr,
    ether_type: EtherType,
    payload: &[u8],
    buf: &mut [u8],
) -> Result<usize, NetError> {
    let frame = EthernetFrame::new(dst, src, ether_type, payload);
    frame.serialize(buf)
}
