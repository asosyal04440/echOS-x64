//! # IP Layer (IPv4)
//!
//! IPv4 packet parsing, construction, and routing

use super::{Ipv4Addr, NetError, local_ip};
use alloc::vec::Vec;

/// IP protocol numbers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IpProtocol {
    ICMP = 1,
    TCP = 6,
    UDP = 17,
    UNKNOWN = 0,
}

impl IpProtocol {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => IpProtocol::ICMP,
            6 => IpProtocol::TCP,
            17 => IpProtocol::UDP,
            _ => IpProtocol::UNKNOWN,
        }
    }
}

/// IPv4 header (20 bytes minimum)
#[derive(Clone, Copy, Debug)]
pub struct Ipv4Header {
    pub version: u8,           // 4 bits, should be 4
    pub ihl: u8,               // 4 bits, header length in 32-bit words
    pub dscp: u8,              // 6 bits
    pub ecn: u8,               // 2 bits
    pub total_length: u16,
    pub identification: u16,
    pub flags: u8,             // 3 bits
    pub fragment_offset: u16,  // 13 bits
    pub ttl: u8,
    pub protocol: IpProtocol,
    pub checksum: u16,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
}

impl Ipv4Header {
    /// Minimum header size
    pub const MIN_SIZE: usize = 20;
    
    /// Parse header from bytes
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let version = (data[0] >> 4) & 0x0F;
        if version != 4 {
            return Err(NetError::InvalidPacket);
        }
        
        let ihl = data[0] & 0x0F;
        if ihl < 5 {
            return Err(NetError::InvalidPacket);
        }
        
        let dscp = (data[1] >> 2) & 0x3F;
        let ecn = data[1] & 0x03;
        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags = (data[6] >> 5) & 0x07;
        let fragment_offset = u16::from_be_bytes([data[6] & 0x1F, data[7]]);
        let ttl = data[8];
        let protocol = IpProtocol::from_u8(data[9]);
        let checksum = u16::from_be_bytes([data[10], data[11]]);
        let src = Ipv4Addr::from_bytes([data[12], data[13], data[14], data[15]]);
        let dst = Ipv4Addr::from_bytes([data[16], data[17], data[18], data[19]]);
        
        // Verify checksum
        let header_len = (ihl as usize) * 4;
        if data.len() < header_len {
            return Err(NetError::InvalidPacket);
        }
        
        let computed_checksum = compute_checksum(&data[..header_len]);
        if computed_checksum != 0 {
            return Err(NetError::ChecksumError);
        }
        
        Ok(Ipv4Header {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            ttl,
            protocol,
            checksum,
            src,
            dst,
        })
    }
    
    /// Serialize header to bytes
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        let header_len = (self.ihl as usize) * 4;
        if buf.len() < header_len {
            return Err(NetError::BufferFull);
        }
        
        buf[0] = (self.version << 4) | self.ihl;
        buf[1] = (self.dscp << 2) | self.ecn;
        buf[2..4].copy_from_slice(&self.total_length.to_be_bytes());
        buf[4..6].copy_from_slice(&self.identification.to_be_bytes());
        buf[6] = (self.flags << 5) | ((self.fragment_offset >> 8) as u8 & 0x1F);
        buf[7] = self.fragment_offset as u8;
        buf[8] = self.ttl;
        buf[9] = self.protocol as u8;
        buf[10..12].copy_from_slice(&self.checksum.to_be_bytes());
        buf[12..16].copy_from_slice(self.src.as_bytes());
        buf[16..20].copy_from_slice(self.dst.as_bytes());
        
        Ok(())
    }
    
    /// Create new header with defaults
    pub fn new(src: Ipv4Addr, dst: Ipv4Addr, protocol: IpProtocol, total_length: u16) -> Self {
        Ipv4Header {
            version: 4,
            ihl: 5,
            dscp: 0,
            ecn: 0,
            total_length,
            identification: 0,
            flags: 2, // Don't fragment
            fragment_offset: 0,
            ttl: 64,
            protocol,
            checksum: 0,
            src,
            dst,
        }
    }
    
    /// Get header length in bytes
    pub fn header_len(&self) -> usize {
        (self.ihl as usize) * 4
    }
    
    /// Get payload length
    pub fn payload_len(&self) -> usize {
        self.total_length as usize - self.header_len()
    }
}

/// IPv4 packet with payload
#[derive(Clone, Debug)]
pub struct Ipv4Packet<'a> {
    pub header: Ipv4Header,
    pub payload: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    /// Parse packet from bytes
    pub fn parse(data: &'a [u8]) -> Result<Self, NetError> {
        let header = Ipv4Header::parse(data)?;
        let header_len = header.header_len();
        
        if data.len() < header.total_length as usize {
            return Err(NetError::InvalidPacket);
        }
        
        let payload = &data[header_len..header.total_length as usize];
        
        Ok(Ipv4Packet { header, payload })
    }
    
    /// Create new packet
    pub fn new(src: Ipv4Addr, dst: Ipv4Addr, protocol: IpProtocol, payload: &'a [u8]) -> Self {
        let total_length = (Ipv4Header::MIN_SIZE + payload.len()) as u16;
        let header = Ipv4Header::new(src, dst, protocol, total_length);
        Ipv4Packet { header, payload }
    }
    
    /// Serialize packet to bytes (computes checksum)
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let total_len = self.header.header_len() + self.payload.len();
        if buf.len() < total_len {
            return Err(NetError::BufferFull);
        }
        
        // Serialize header with zero checksum
        let mut header = self.header;
        header.checksum = 0;
        header.serialize(buf)?;
        
        // Copy payload
        buf[self.header.header_len()..total_len].copy_from_slice(self.payload);
        
        // Compute and set checksum
        let checksum = compute_checksum(&buf[..self.header.header_len()]);
        buf[10..12].copy_from_slice(&checksum.to_be_bytes());
        
        Ok(total_len)
    }
}

/// Compute IP checksum (one's complement)
pub fn compute_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    
    // Sum 16-bit words
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            (chunk[0] as u16) << 8
        };
        sum += word as u32;
    }
    
    // Fold carries
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    // One's complement
    !(sum as u16)
}

/// Process incoming IP packet
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    let packet = Ipv4Packet::parse(data)?;
    
    // Check if destination is us
    let local = local_ip();
    if packet.header.dst != local && 
       !packet.header.dst.is_broadcast() &&
       !packet.header.dst.is_multicast() {
        // Not for us, drop
        return Ok(());
    }
    
    // Dispatch by protocol
    match packet.header.protocol {
        IpProtocol::ICMP => {
            icmp_process(&packet)?;
        }
        IpProtocol::TCP => {
            super::tcp::process_packet(&packet)?;
        }
        IpProtocol::UDP => {
            super::udp::process_packet(&packet)?;
        }
        _ => {
            // Unknown protocol
        }
    }
    
    Ok(())
}

/// Build IP packet for sending
pub fn build_packet(
    dst: Ipv4Addr,
    protocol: IpProtocol,
    payload: &[u8],
    buf: &mut [u8],
) -> Result<usize, NetError> {
    let src = local_ip();
    let packet = Ipv4Packet::new(src, dst, protocol, payload);
    packet.serialize(buf)
}

/// Route IP address to determine next hop
pub fn route(dst: Ipv4Addr) -> Option<Ipv4Addr> {
    let local = local_ip();
    
    // Same subnet - direct delivery
    if is_same_subnet(local, dst) {
        return Some(dst);
    }
    
    // Different subnet - use gateway
    let config = super::get_config();
    if config.gateway != [0, 0, 0, 0] {
        return Some(Ipv4Addr::from_bytes(config.gateway));
    }
    
    None
}

/// Check if two IPs are in the same subnet
pub fn is_same_subnet(a: Ipv4Addr, b: Ipv4Addr) -> bool {
    let config = super::get_config();
    let mask = Ipv4Addr::from_bytes(config.netmask);
    
    (a.to_u32() & mask.to_u32()) == (b.to_u32() & mask.to_u32())
}

/// ICMP processing (stub)
pub fn icmp_process(packet: &Ipv4Packet) -> Result<(), NetError> {
    // TODO: Implement ICMP echo reply
    crate::serial_println!(
        "[NET] ICMP packet from {}: {} bytes",
        packet.header.src,
        packet.payload.len()
    );
    Ok(())
}
