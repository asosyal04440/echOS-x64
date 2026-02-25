//! # ARP Protocol
//!
//! Address Resolution Protocol for MAC resolution

use super::{MacAddr, Ipv4Addr, NetError, local_ip};
use super::ethernet::{EtherType, EthernetFrame};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// ARP header
#[derive(Clone, Copy, Debug)]
pub struct ArpHeader {
    pub htype: u16,      // Hardware type (1 = Ethernet)
    pub ptype: u16,      // Protocol type (0x0800 = IPv4)
    pub hlen: u8,        // Hardware address length (6)
    pub plen: u8,        // Protocol address length (4)
    pub oper: ArpOperation,
    pub sha: MacAddr,    // Sender hardware address
    pub spa: Ipv4Addr,   // Sender protocol address
    pub tha: MacAddr,    // Target hardware address
    pub tpa: Ipv4Addr,   // Target protocol address
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ArpOperation {
    Request = 1,
    Reply = 2,
    Unknown = 0,
}

impl ArpOperation {
    pub fn from_u16(val: u16) -> Self {
        match val {
            1 => ArpOperation::Request,
            2 => ArpOperation::Reply,
            _ => ArpOperation::Unknown,
        }
    }
}

impl ArpHeader {
    pub const SIZE: usize = 28;
    
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let htype = u16::from_be_bytes([data[0], data[1]]);
        let ptype = u16::from_be_bytes([data[2], data[3]]);
        let hlen = data[4];
        let plen = data[5];
        let oper = ArpOperation::from_u16(u16::from_be_bytes([data[6], data[7]]));
        
        let sha = MacAddr::new([data[8], data[9], data[10], data[11], data[12], data[13]]);
        let spa = Ipv4Addr::from_bytes([data[14], data[15], data[16], data[17]]);
        let tha = MacAddr::new([data[18], data[19], data[20], data[21], data[22], data[23]]);
        let tpa = Ipv4Addr::from_bytes([data[24], data[25], data[26], data[27]]);
        
        Ok(ArpHeader {
            htype, ptype, hlen, plen, oper, sha, spa, tha, tpa,
        })
    }
    
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..2].copy_from_slice(&self.htype.to_be_bytes());
        buf[2..4].copy_from_slice(&self.ptype.to_be_bytes());
        buf[4] = self.hlen;
        buf[5] = self.plen;
        buf[6..8].copy_from_slice(&(self.oper as u16).to_be_bytes());
        buf[8..14].copy_from_slice(self.sha.as_bytes());
        buf[14..18].copy_from_slice(self.spa.as_bytes());
        buf[18..24].copy_from_slice(self.tha.as_bytes());
        buf[24..28].copy_from_slice(self.tpa.as_bytes());
        
        Ok(())
    }
    
    pub fn new_request(sha: MacAddr, spa: Ipv4Addr, tpa: Ipv4Addr) -> Self {
        ArpHeader {
            htype: 1,
            ptype: 0x0800,
            hlen: 6,
            plen: 4,
            oper: ArpOperation::Request,
            sha,
            spa,
            tha: MacAddr::ZERO,
            tpa,
        }
    }
    
    pub fn new_reply(sha: MacAddr, spa: Ipv4Addr, tha: MacAddr, tpa: Ipv4Addr) -> Self {
        ArpHeader {
            htype: 1,
            ptype: 0x0800,
            hlen: 6,
            plen: 4,
            oper: ArpOperation::Reply,
            sha,
            spa,
            tha,
            tpa,
        }
    }
}

// ============================================================================
// ARP CACHE
// ============================================================================

static ARP_CACHE: Mutex<BTreeMap<u32, MacAddr>> = Mutex::new(BTreeMap::new());
static ARP_PENDING: Mutex<BTreeMap<u32, Vec<Vec<u8>>>> = Mutex::new(BTreeMap::new());

/// Initialize ARP subsystem
pub fn init() {
    crate::serial_println!("[ARP] Initialized");
}

/// Resolve IP to MAC address
pub fn resolve(ip: Ipv4Addr) -> Option<MacAddr> {
    ARP_CACHE.lock().get(&ip.to_u32()).copied()
}

/// Get ARP table entries
pub fn get_table() -> Vec<(Ipv4Addr, MacAddr)> {
    let cache = ARP_CACHE.lock();
    cache.iter()
        .map(|(&ip, &mac)| (Ipv4Addr::from_u32(ip), mac))
        .collect()
}

/// Add entry to ARP cache
pub fn add_entry(ip: Ipv4Addr, mac: MacAddr) {
    ARP_CACHE.lock().insert(ip.to_u32(), mac);
    
    // Send pending packets
    let mut pending = ARP_PENDING.lock();
    if let Some(packets) = pending.remove(&ip.to_u32()) {
        drop(pending);
        
        for packet in packets {
            // Resend with resolved MAC
            let _ = send_to_ip(ip, &packet);
        }
    }
}

/// Send ARP request
pub fn send_request(tpa: Ipv4Addr) -> Result<(), NetError> {
    let iface = super::default_interface().ok_or(NetError::NoInterface)?;
    let mut iface = iface.lock();
    
    let sha = iface.mac();
    let spa = iface.ip();
    
    let arp = ArpHeader::new_request(sha, spa, tpa);
    let mut buf = alloc::vec![0u8; ArpHeader::SIZE];
    arp.serialize(&mut buf)?;
    
    // Build Ethernet frame with broadcast
    let mut frame_buf = alloc::vec![0u8; 1514];
    let frame = EthernetFrame::new(
        MacAddr::BROADCAST,
        sha,
        EtherType::ARP,
        &buf,
    );
    let len = frame.serialize(&mut frame_buf)?;
    
    iface.send(&frame_buf[..len])?;
    
    crate::serial_println!("[ARP] Request: Who has {}?", 
        super::socket::format_ipv4(tpa));
    
    Ok(())
}

/// Send packet to IP (resolve MAC first)
pub fn send_to_ip(ip: Ipv4Addr, data: &[u8]) -> Result<(), NetError> {
    // Check if we need routing
    let next_hop = super::ip::route(ip).unwrap_or(ip);
    
    // Check ARP cache
    if let Some(mac) = resolve(next_hop) {
        let iface = super::default_interface().ok_or(NetError::NoInterface)?;
        let mut iface = iface.lock();
        
        let mut frame_buf = alloc::vec![0u8; 1514];
        let frame = EthernetFrame::new(
            mac,
            iface.mac(),
            EtherType::IPV4,
            data,
        );
        let len = frame.serialize(&mut frame_buf)?;
        
        iface.send(&frame_buf[..len])?;
        Ok(())
    } else {
        // Queue packet and send ARP request
        ARP_PENDING.lock().entry(next_hop.to_u32()).or_default().push(data.to_vec());
        send_request(next_hop)?;
        Err(NetError::WouldBlock)
    }
}

/// Process incoming ARP packet
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    let arp = ArpHeader::parse(data)?;
    
    // Update cache with sender info
    add_entry(arp.spa, arp.sha);
    
    // Check if this is for us
    let local = local_ip();
    if arp.tpa == local {
        match arp.oper {
            ArpOperation::Request => {
                // Send reply
                let iface = super::default_interface().ok_or(NetError::NoInterface)?;
                let mut iface = iface.lock();
                
                let reply = ArpHeader::new_reply(
                    iface.mac(),
                    local,
                    arp.sha,
                    arp.spa,
                );
                
                let mut buf = alloc::vec![0u8; ArpHeader::SIZE];
                reply.serialize(&mut buf)?;
                
                let mut frame_buf = alloc::vec![0u8; 1514];
                let frame = EthernetFrame::new(
                    arp.sha,
                    iface.mac(),
                    EtherType::ARP,
                    &buf,
                );
                let len = frame.serialize(&mut frame_buf)?;
                
                iface.send(&frame_buf[..len])?;
                
                crate::serial_println!("[ARP] Reply: {} is at {:?}", 
                    super::socket::format_ipv4(local), iface.mac());
            }
            ArpOperation::Reply => {
                crate::serial_println!("[ARP] Reply: {} is at {:?}", 
                    super::socket::format_ipv4(arp.spa), arp.sha);
            }
            _ => {}
        }
    }
    
    Ok(())
}

/// Get ARP cache entries
pub fn get_cache() -> Vec<(Ipv4Addr, MacAddr)> {
    ARP_CACHE.lock()
        .iter()
        .map(|(&ip, &mac)| (Ipv4Addr::from_u32(ip), mac))
        .collect()
}
