//! # DNS over TLS (DoT)
//!
//! RFC 7858 - DNS queries over TLS

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use super::dns::DnsHeader;
use super::dns::DnsRecordType;
use super::{Ipv4Addr, NetError, Port, SocketAddr};
use super::ipv6::Ipv6Addr;
use super::socket::{socket, connect, send, recv, close, AddressFamily, SocketType, Protocol};

/// DoT port
const DOT_PORT: u16 = 853;

/// DoT Client
pub struct DotClient {
    pub server_ip: Ipv4Addr,
    pub server_name: String,
    pub port: u16,
    pub timeout_ms: u64,
    pub connected: bool,
    pub socket_id: Option<u32>,
    pub cache: BTreeMap<String, CachedResponse>,
}

/// Cached DoT response
#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub response: Vec<u8>,
    pub expiry: u64,
}

impl DotClient {
    /// Create new DoT client
    pub fn new(server_ip: Ipv4Addr, server_name: &str) -> Self {
        DotClient {
            server_ip,
            server_name: server_name.to_string(),
            port: DOT_PORT,
            timeout_ms: 5000,
            connected: false,
            socket_id: None,
            cache: BTreeMap::new(),
        }
    }

    /// Create with common providers
    pub fn cloudflare() -> Self {
        Self::new(Ipv4Addr::from_bytes([1, 1, 1, 1]), "cloudflare-dns.com")
    }

    pub fn google() -> Self {
        Self::new(Ipv4Addr::from_bytes([8, 8, 8, 8]), "dns.google")
    }

    pub fn quad9() -> Self {
        Self::new(Ipv4Addr::from_bytes([9, 9, 9, 9]), "dns.quad9.net")
    }

    /// Connect to DoT server
    pub fn connect(&mut self) -> Result<(), DotError> {
        if self.connected {
            return Ok(());
        }

        // Create TCP socket
        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| DotError::SocketError)?;

        // Connect to server
        let addr = SocketAddr::new(self.server_ip, Port(self.port));
        connect(sock_id, addr).map_err(|_| DotError::ConnectionFailed)?;

        // TODO: Perform TLS handshake
        // This requires TLS implementation
        // For now, we'll mark as needing TLS

        self.socket_id = Some(sock_id);
        // self.connected = true; // Only set after TLS handshake

        Err(DotError::TlsNotSupported)
    }

    /// Disconnect from server
    pub fn disconnect(&mut self) {
        if let Some(sock_id) = self.socket_id {
            let _ = close(sock_id);
            self.socket_id = None;
            self.connected = false;
        }
    }

    /// Build DNS query wire format
    pub fn build_query(domain: &str, qtype: DnsRecordType) -> Vec<u8> {
        let mut query = Vec::new();

        // DNS Header (12 bytes)
        let header = DnsHeader::new_query(0x1234);
        query.push((header.id >> 8) as u8);
        query.push((header.id & 0xFF) as u8);
        query.push((header.flags >> 8) as u8);
        query.push((header.flags & 0xFF) as u8);
        query.push((header.qdcount >> 8) as u8);
        query.push((header.qdcount & 0xFF) as u8);
        query.push((header.ancount >> 8) as u8);
        query.push((header.ancount & 0xFF) as u8);
        query.push((header.nscount >> 8) as u8);
        query.push((header.nscount & 0xFF) as u8);
        query.push((header.arcount >> 8) as u8);
        query.push((header.arcount & 0xFF) as u8);

        // Question section
        for label in domain.split('.') {
            if !label.is_empty() {
                query.push(label.len() as u8);
                for c in label.chars() {
                    query.push(c as u8);
                }
            }
        }
        query.push(0);

        // QTYPE
        query.push((qtype as u16 >> 8) as u8);
        query.push((qtype as u16 & 0xFF) as u8);

        // QCLASS (IN = 1)
        query.push(0);
        query.push(1);

        query
    }

    /// Send DNS query over TLS
    pub fn query(&mut self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DotError> {
        // Check cache
        let cache_key = format!("{}:{}", domain, qtype as u16);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached.response.clone());
        }

        // Ensure connected
        if !self.connected {
            self.connect()?;
        }

        let sock_id = self.socket_id.ok_or(DotError::NotConnected)?;

        // Build DNS query
        let dns_query = Self::build_query(domain, qtype);

        // DoT uses 2-byte length prefix over TCP
        let mut tls_record = Vec::new();
        tls_record.extend_from_slice(&(dns_query.len() as u16).to_be_bytes());
        tls_record.extend_from_slice(&dns_query);

        // TODO: Encrypt with TLS before sending
        // For now, return error
        Err(DotError::TlsNotSupported)
    }

    /// Parse DNS response
    pub fn parse_response(data: &[u8]) -> Result<DotResponse, DotError> {
        if data.len() < 12 {
            return Err(DotError::InvalidResponse);
        }

        let header = DnsHeader::parse(data).map_err(|_| DotError::InvalidResponse)?;

        let mut response = DotResponse {
            header,
            answers: Vec::new(),
        };

        // Parse questions (skip)
        let mut offset = 12;
        for _ in 0..header.qdcount {
            while offset < data.len() && data[offset] != 0 {
                if (data[offset] & 0xC0) == 0xC0 {
                    offset += 2;
                    break;
                }
                offset += 1 + data[offset] as usize;
            }
            if offset < data.len() && data[offset] == 0 {
                offset += 1;
            }
            offset += 4;
        }

        // Parse answers
        for _ in 0..header.ancount {
            if offset >= data.len() {
                break;
            }
            let answer = Self::parse_answer(data, &mut offset)?;
            response.answers.push(answer);
        }

        Ok(response)
    }

    fn parse_answer(data: &[u8], offset: &mut usize) -> Result<DotAnswer, DotError> {
        let name = Self::parse_name(data, offset)?;

        if *offset + 10 > data.len() {
            return Err(DotError::InvalidResponse);
        }

        let rtype = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
        let _rclass = u16::from_be_bytes([data[*offset + 2], data[*offset + 3]]);
        let _ttl = u32::from_be_bytes([data[*offset + 4], data[*offset + 5], data[*offset + 6], data[*offset + 7]]);
        let rdlength = u16::from_be_bytes([data[*offset + 8], data[*offset + 9]]) as usize;
        *offset += 10;

        if *offset + rdlength > data.len() {
            return Err(DotError::InvalidResponse);
        }

        let rdata = data[*offset..*offset + rdlength].to_vec();
        *offset += rdlength;

        let ip = match rtype {
            1 if rdlength == 4 => {
                Some(IpAddr::V4(Ipv4Addr::from_bytes([rdata[0], rdata[1], rdata[2], rdata[3]])))
            }
            28 if rdlength == 16 => {
                let mut addr = [0u8; 16];
                addr.copy_from_slice(&rdata);
                Some(IpAddr::V6(Ipv6Addr::new(addr)))
            }
            _ => None,
        };

        Ok(DotAnswer {
            name,
            rtype,
            rdata,
            ip,
        })
    }

    fn parse_name(data: &[u8], offset: &mut usize) -> Result<String, DotError> {
        let mut name = String::new();
        let mut jumped = false;
        let mut max_jumps = 5;

        loop {
            if *offset >= data.len() {
                return Err(DotError::InvalidResponse);
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1;
                break;
            }

            if (len & 0xC0) == 0xC0 {
                if *offset + 1 >= data.len() {
                    return Err(DotError::InvalidResponse);
                }
                let ptr = (((data[*offset] & 0x3F) as usize) << 8) | (data[*offset + 1] as usize);
                if !jumped {
                    *offset += 2;
                    jumped = true;
                }
                *offset = ptr;
                max_jumps -= 1;
                if max_jumps == 0 {
                    return Err(DotError::InvalidResponse);
                }
                continue;
            }

            *offset += 1;
            if *offset + len > data.len() {
                return Err(DotError::InvalidResponse);
            }

            if !name.is_empty() {
                name.push('.');
            }

            for i in 0..len {
                name.push(data[*offset + i] as char);
            }
            *offset += len;
        }

        if name.is_empty() {
            name.push('.');
        }

        Ok(name)
    }
}

/// IP Address wrapper
#[derive(Clone, Debug)]
pub enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

/// DNS Answer
#[derive(Clone, Debug)]
pub struct DotAnswer {
    pub name: String,
    pub rtype: u16,
    pub rdata: Vec<u8>,
    pub ip: Option<IpAddr>,
}

/// DNS Response
#[derive(Clone, Debug)]
pub struct DotResponse {
    pub header: DnsHeader,
    pub answers: Vec<DotAnswer>,
}

impl DotResponse {
    /// Get first A record IP
    pub fn get_a(&self) -> Option<Ipv4Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V4(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }

    /// Get first AAAA record IP
    pub fn get_aaaa(&self) -> Option<Ipv6Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V6(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }
}

/// DoT Error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotError {
    TlsNotSupported,
    NotConnected,
    SocketError,
    ConnectionFailed,
    InvalidResponse,
    Timeout,
    TlsHandshakeFailed,
}

impl Drop for DotClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

// Global DoT client
lazy_static::lazy_static! {
    static ref DOT_CLIENT: Mutex<Option<DotClient>> = Mutex::new(None);
}

/// Initialize DoT client
pub fn init_dot(server_ip: Ipv4Addr, server_name: &str) {
    *DOT_CLIENT.lock() = Some(DotClient::new(server_ip, server_name));
}

/// Resolve domain using DoT
pub fn resolve_dot(domain: &str, qtype: DnsRecordType) -> Result<DotResponse, DotError> {
    let mut client = DOT_CLIENT.lock();
    if let Some(client) = client.as_mut() {
        let response_data = client.query(domain, qtype)?;
        DotClient::parse_response(&response_data)
    } else {
        Err(DotError::NotConnected)
    }
}
