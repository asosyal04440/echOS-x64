//! # DNS Client
//!
//! Simple DNS resolver with caching

use super::{Ipv4Addr, Port, SocketAddr, NetError};
use super::udp;
use super::socket::{socket, bind, sendto, recvfrom, close};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use alloc::collections::BTreeMap;
use spin::Mutex;

/// DNS server port
const DNS_PORT: u16 = 53;

/// DNS record types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsRecordType {
    A = 1,      // IPv4 address
    NS = 2,     // Name server
    CNAME = 5,  // Canonical name
    SOA = 6,    // Start of authority
    PTR = 12,   // Pointer
    MX = 15,    // Mail exchange
    TXT = 16,   // Text
    AAAA = 28,  // IPv6 address
    SRV = 33,   // Service
}

impl DnsRecordType {
    pub fn from_u16(v: u16) -> Self {
        match v {
            1 => DnsRecordType::A,
            2 => DnsRecordType::NS,
            5 => DnsRecordType::CNAME,
            6 => DnsRecordType::SOA,
            12 => DnsRecordType::PTR,
            15 => DnsRecordType::MX,
            16 => DnsRecordType::TXT,
            28 => DnsRecordType::AAAA,
            33 => DnsRecordType::SRV,
            _ => DnsRecordType::A,
        }
    }
}

/// DNS header
#[derive(Clone, Copy, Debug)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qdcount: u16,
    pub ancount: u16,
    pub nscount: u16,
    pub arcount: u16,
}

impl DnsHeader {
    pub const SIZE: usize = 12;
    
    pub fn new_query(id: u16) -> Self {
        DnsHeader {
            id,
            flags: 0x0100, // Standard query, recursion desired
            qdcount: 1,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }
    
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        Ok(DnsHeader {
            id: u16::from_be_bytes([data[0], data[1]]),
            flags: u16::from_be_bytes([data[2], data[3]]),
            qdcount: u16::from_be_bytes([data[4], data[5]]),
            ancount: u16::from_be_bytes([data[6], data[7]]),
            nscount: u16::from_be_bytes([data[8], data[9]]),
            arcount: u16::from_be_bytes([data[10], data[11]]),
        })
    }
    
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..2].copy_from_slice(&self.id.to_be_bytes());
        buf[2..4].copy_from_slice(&self.flags.to_be_bytes());
        buf[4..6].copy_from_slice(&self.qdcount.to_be_bytes());
        buf[6..8].copy_from_slice(&self.ancount.to_be_bytes());
        buf[8..10].copy_from_slice(&self.nscount.to_be_bytes());
        buf[10..12].copy_from_slice(&self.arcount.to_be_bytes());
        
        Ok(())
    }
    
    pub fn is_response(&self) -> bool {
        self.flags & 0x8000 != 0
    }
    
    pub fn is_valid(&self) -> bool {
        self.flags & 0x000F == 0 // No error
    }
}

/// DNS question
#[derive(Clone, Debug)]
pub struct DnsQuestion {
    pub name: String,
    pub qtype: u16,  // 1 = A record
    pub qclass: u16, // 1 = IN
}

impl DnsQuestion {
    pub fn new(name: &str) -> Self {
        DnsQuestion {
            name: String::from(name),
            qtype: 1,
            qclass: 1,
        }
    }
    
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let mut offset = 0;
        
        // Encode domain name
        for part in self.name.split('.') {
            if part.is_empty() {
                continue;
            }
            let bytes = part.as_bytes();
            if offset + 1 + bytes.len() >= buf.len() {
                return Err(NetError::BufferFull);
            }
            buf[offset] = bytes.len() as u8;
            offset += 1;
            buf[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }
        
        // Null terminator
        buf[offset] = 0;
        offset += 1;
        
        // Type and class
        if offset + 4 >= buf.len() {
            return Err(NetError::BufferFull);
        }
        buf[offset..offset + 2].copy_from_slice(&self.qtype.to_be_bytes());
        buf[offset + 2..offset + 4].copy_from_slice(&self.qclass.to_be_bytes());
        offset += 4;
        
        Ok(offset)
    }
}

/// DNS answer
#[derive(Clone, Debug)]
pub struct DnsAnswer {
    pub name: String,
    pub atype: u16,
    pub aclass: u16,
    pub ttl: u32,
    pub data: Vec<u8>,
}

impl DnsAnswer {
    pub fn parse(data: &[u8], offset: usize) -> Result<(Self, usize), NetError> {
        let mut pos = offset;
        
        // Parse name (handle compression)
        let name = Self::parse_name(data, &mut pos)?;
        
        if pos + 10 > data.len() {
            return Err(NetError::InvalidPacket);
        }
        
        let atype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let aclass = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
        let ttl = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let rdlength = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;
        
        if pos + rdlength > data.len() {
            return Err(NetError::InvalidPacket);
        }
        
        let answer_data = data[pos..pos + rdlength].to_vec();
        pos += rdlength;
        
        Ok((DnsAnswer {
            name,
            atype,
            aclass,
            ttl,
            data: answer_data,
        }, pos))
    }
    
    fn parse_name(data: &[u8], pos: &mut usize) -> Result<String, NetError> {
        let mut name = String::new();
        let mut jumped = false;
        let mut jumped_pos = 0;
        
        loop {
            let len = data[*pos] as usize;
            *pos += 1;
            
            if len == 0 {
                break;
            }
            
            // Handle compression
            if (len & 0xC0) == 0xC0 {
                if !jumped {
                    jumped_pos = *pos + 1;
                    jumped = true;
                }
                let offset = ((len & 0x3F) << 8) | (data[*pos] as usize);
                *pos = offset;
                continue;
            }
            
            if *pos + len > data.len() {
                return Err(NetError::InvalidPacket);
            }
            
            if !name.is_empty() {
                name.push('.');
            }
            
            for i in 0..len {
                name.push(data[*pos + i] as char);
            }
            *pos += len;
        }
        
        if jumped {
            *pos = jumped_pos;
        }
        
        Ok(name)
    }
    
    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        if self.atype == 1 && self.data.len() == 4 {
            Some(Ipv4Addr::from_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else {
            None
        }
    }
}

// ============================================================================
// DNS RESOLVER
// ============================================================================

static DNS_SOCKET: Mutex<Option<u32>> = Mutex::new(None);

/// DNS cache entry
#[derive(Clone, Debug)]
pub struct DnsCacheEntry {
    pub name: String,
    pub record_type: DnsRecordType,
    pub data: Vec<u8>,
    pub ttl: u32,
    pub obtained_at: u64,
}

impl DnsCacheEntry {
    pub fn is_expired(&self, current_time: u64) -> bool {
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.ttl as u64
    }
    
    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        if self.record_type == DnsRecordType::A && self.data.len() == 4 {
            Some(Ipv4Addr::from_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else {
            None
        }
    }
    
    pub fn as_ipv6(&self) -> Option<[u8; 16]> {
        if self.record_type == DnsRecordType::AAAA && self.data.len() == 16 {
            let mut addr = [0u8; 16];
            addr.copy_from_slice(&self.data);
            Some(addr)
        } else {
            None
        }
    }
    
    pub fn as_cname(&self) -> Option<&str> {
        if self.record_type == DnsRecordType::CNAME {
            // CNAME data is a domain name
            Some(core::str::from_utf8(&self.data).unwrap_or(""))
        } else {
            None
        }
    }
}

/// DNS cache (key = "name:type")
static DNS_CACHE: Mutex<BTreeMap<String, DnsCacheEntry>> = Mutex::new(BTreeMap::new());

/// Initialize DNS resolver
pub fn init() {
    crate::serial_println!("[DNS] Resolver initialized with cache");
}

/// Get cached DNS entry
pub fn get_cached(name: &str, record_type: DnsRecordType, current_time: u64) -> Option<DnsCacheEntry> {
    let key = format!("{}:{}", name, record_type as u16);
    let cache = DNS_CACHE.lock();
    if let Some(entry) = cache.get(&key) {
        if !entry.is_expired(current_time) {
            return Some(entry.clone());
        }
    }
    None
}

/// Add entry to DNS cache
pub fn cache_entry(entry: DnsCacheEntry, current_time: u64) {
    let key = format!("{}:{}", entry.name, entry.record_type as u16);
    let mut cache = DNS_CACHE.lock();
    cache.insert(key, DnsCacheEntry {
        obtained_at: current_time,
        ..entry
    });
}

/// Clear DNS cache
pub fn clear_cache() {
    DNS_CACHE.lock().clear();
}

/// Get cache size
pub fn cache_size() -> usize {
    DNS_CACHE.lock().len()
}

/// Resolve hostname to IP address
pub fn resolve(hostname: &str, dns_server: Ipv4Addr) -> Result<Ipv4Addr, NetError> {
    // Create UDP socket
    let sock_id = socket(
        super::socket::AddressFamily::IPV4,
        super::socket::SocketType::DGRAM,
        super::socket::Protocol::UDP,
    )?;
    
    // Bind to ephemeral port
    bind(sock_id, SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(0)))?;
    
    // Build DNS query
    let id = crate::random::rand_u64() as u16;
    let header = DnsHeader::new_query(id);
    let question = DnsQuestion::new(hostname);
    
    let mut buf = vec![0u8; 512];
    header.serialize(&mut buf)?;
    let q_offset = DnsHeader::SIZE;
    let q_len = question.serialize(&mut buf[q_offset..])?;
    let total_len = q_offset + q_len;
    
    // Send query
    let dst = SocketAddr::new(dns_server, Port(DNS_PORT));
    sendto(sock_id, &buf[..total_len], dst, 0)?;
    
    crate::serial_println!("[DNS] Query sent for {}", hostname);
    
    // Receive response
    let mut resp_buf = vec![0u8; 512];
    let (len, _) = recvfrom(sock_id, &mut resp_buf, 0)?;
    
    close(sock_id)?;
    
    // Parse response
    let resp_header = DnsHeader::parse(&resp_buf)?;
    
    if !resp_header.is_response() || !resp_header.is_valid() {
        return Err(NetError::ProtocolError);
    }
    
    // Skip questions
    let mut offset = DnsHeader::SIZE;
    for _ in 0..resp_header.qdcount {
        // Skip name
        while offset < len && resp_buf[offset] != 0 {
            let lbl_len = resp_buf[offset] as usize;
            if lbl_len & 0xC0 == 0xC0 {
                offset += 2;
                break;
            }
            offset += 1 + lbl_len;
        }
        if offset < len && resp_buf[offset] == 0 {
            offset += 1;
        }
        offset += 4; // type + class
    }
    
    // Parse answers
    for _ in 0..resp_header.ancount {
        let (answer, new_offset) = DnsAnswer::parse(&resp_buf, offset)?;
        offset = new_offset;
        
        if let Some(ip) = answer.as_ipv4() {
            crate::serial_println!("[DNS] {} -> {}", hostname, super::socket::format_ipv4(ip));
            return Ok(ip);
        }
    }
    
    Err(NetError::HostUnreachable)
}

/// Resolve using default DNS server
pub fn resolve_default(hostname: &str) -> Result<Ipv4Addr, NetError> {
    let config = super::get_config();
    
    if let Some(dns) = config.dns_servers.first() {
        resolve(hostname, Ipv4Addr::from_bytes(*dns))
    } else {
        // Try Google DNS
        resolve(hostname, Ipv4Addr::new(8, 8, 8, 8))
    }
}
