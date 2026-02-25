//! # UDP Protocol
//!
//! UDP datagram handling

use super::{Ipv4Addr, Port, NetError, allocate_socket_id};
use super::ip::{IpProtocol, Ipv4Packet};
use super::socket::SocketAddr;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use spin::Mutex;

/// UDP header (8 bytes)
#[derive(Clone, Copy, Debug)]
pub struct UdpHeader {
    pub src_port: Port,
    pub dst_port: Port,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub const SIZE: usize = 8;
    
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let src_port = Port(u16::from_be_bytes([data[0], data[1]]));
        let dst_port = Port(u16::from_be_bytes([data[2], data[3]]));
        let length = u16::from_be_bytes([data[4], data[5]]);
        let checksum = u16::from_be_bytes([data[6], data[7]]);
        
        Ok(UdpHeader {
            src_port,
            dst_port,
            length,
            checksum,
        })
    }
    
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..2].copy_from_slice(&self.src_port.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.0.to_be_bytes());
        buf[4..6].copy_from_slice(&self.length.to_be_bytes());
        buf[6..8].copy_from_slice(&self.checksum.to_be_bytes());
        
        Ok(())
    }
    
    pub fn new(src_port: Port, dst_port: Port, length: u16) -> Self {
        UdpHeader {
            src_port,
            dst_port,
            length,
            checksum: 0,
        }
    }
}

/// Compute UDP checksum
pub fn compute_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    
    // Pseudo-header
    sum += u16::from_be_bytes([src_ip.0[0], src_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([src_ip.0[2], src_ip.0[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[0], dst_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[2], dst_ip.0[3]]) as u32;
    sum += 17u32; // UDP protocol number
    sum += segment.len() as u32;
    
    // UDP segment
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    
    // Odd byte
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    
    // Fold carries
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    // One's complement
    // For UDP, 0 means "no checksum" - if result is 0, return 0xFFFF
    let result = !(sum as u16);
    if result == 0 { 0xFFFF } else { result }
}

/// Verify UDP checksum
pub fn verify_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> bool {
    // 0 checksum means "not checked" for IPv4 UDP
    if segment.len() >= 8 {
        let checksum = u16::from_be_bytes([segment[6], segment[7]]);
        if checksum == 0 {
            return true;
        }
    }
    compute_checksum(src_ip, dst_ip, segment) == 0
}

/// UDP socket
#[derive(Clone, Debug)]
pub struct UdpSocket {
    pub id: u32,
    pub local: SocketAddr,
    pub rx_buffer: Vec<(SocketAddr, Vec<u8>)>,
}

impl UdpSocket {
    pub fn new() -> Self {
        UdpSocket {
            id: allocate_socket_id(),
            local: SocketAddr::default(),
            rx_buffer: Vec::new(),
        }
    }
    
    pub fn bind(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        self.local = addr;
        Ok(())
    }
    
    pub fn send_to(&mut self, data: &[u8], dst: SocketAddr) -> Result<usize, NetError> {
        let header = UdpHeader::new(
            self.local.port,
            dst.port,
            (UdpHeader::SIZE + data.len()) as u16,
        );
        
        let mut segment = vec![0u8; UdpHeader::SIZE + data.len()];
        header.serialize(&mut segment)?;
        segment[UdpHeader::SIZE..].copy_from_slice(data);
        
        let mut ip_buf = vec![0u8; 1500];
        let len = super::ip::build_packet(dst.ip, IpProtocol::UDP, &segment, &mut ip_buf)?;
        
        super::send_packet(&ip_buf[..len])?;
        
        Ok(data.len())
    }
    
    pub fn recv_from(&mut self, buf: &mut [u8]) -> Result<(usize, SocketAddr), NetError> {
        if self.rx_buffer.is_empty() {
            return Err(NetError::WouldBlock);
        }
        
        let (src, data) = self.rx_buffer.remove(0);
        let len = buf.len().min(data.len());
        buf[..len].copy_from_slice(&data[..len]);
        
        Ok((len, src))
    }
}

// ============================================================================
// UDP MANAGER
// ============================================================================

static UDP_SOCKETS: Mutex<BTreeMap<u32, Box<UdpSocket>>> = Mutex::new(BTreeMap::new());
static UDP_BINDINGS: Mutex<BTreeMap<Port, u32>> = Mutex::new(BTreeMap::new());

pub fn init() {
    crate::serial_println!("[UDP] Initialized");
}

pub fn create_socket() -> u32 {
    let sock = UdpSocket::new();
    let id = sock.id;
    UDP_SOCKETS.lock().insert(id, Box::new(sock));
    id
}

pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    sock.bind(addr)?;
    
    UDP_BINDINGS.lock().insert(addr.port, socket_id);
    
    Ok(())
}

pub fn send_to(socket_id: u32, data: &[u8], dst: SocketAddr) -> Result<usize, NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    sock.send_to(data, dst)
}

pub fn recv_from(socket_id: u32, buf: &mut [u8]) -> Result<(usize, SocketAddr), NetError> {
    let mut socks = UDP_SOCKETS.lock();
    let sock = socks.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    sock.recv_from(buf)
}

pub fn close(socket_id: u32) {
    if let Some(sock) = UDP_SOCKETS.lock().remove(&socket_id) {
        UDP_BINDINGS.lock().remove(&sock.local.port);
    }
}

/// Get socket by ID (for event checking)
pub fn get_socket(socket_id: u32) -> Option<UdpSocket> {
    let socks = UDP_SOCKETS.lock();
    socks.get(&socket_id).map(|s| (**s).clone())
}

/// Get all sockets (for ss utility)
pub fn get_all_sockets() -> Vec<UdpSocket> {
    let socks = UDP_SOCKETS.lock();
    socks.values().map(|s| (**s).clone()).collect()
}

pub fn process_packet(ip_packet: &Ipv4Packet) -> Result<(), NetError> {
    let udp_header = UdpHeader::parse(ip_packet.payload)?;
    let data = &ip_packet.payload[UdpHeader::SIZE..udp_header.length as usize];
    
    let src = SocketAddr::new(ip_packet.header.src, udp_header.src_port);
    
    // Find socket by destination port
    let bindings = UDP_BINDINGS.lock();
    if let Some(&socket_id) = bindings.get(&udp_header.dst_port) {
        drop(bindings);
        
        let mut socks = UDP_SOCKETS.lock();
        if let Some(sock) = socks.get_mut(&socket_id) {
            sock.rx_buffer.push((src, data.to_vec()));
        }
    }
    
    Ok(())
}
