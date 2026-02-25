//! # DNS over HTTPS (DoH)
//!
//! RFC 8484 - DNS queries over HTTPS

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use super::dns::DnsHeader;
use super::dns::DnsRecordType;
use super::{Ipv4Addr, NetError};
use super::ipv6::Ipv6Addr;

/// DoH Content-Type
const DNS_MESSAGE_CONTENT_TYPE: &str = "application/dns-message";

/// DoH Client
pub struct DohClient {
    pub server_url: String,
    pub timeout_ms: u64,
    pub cache: BTreeMap<String, CachedResponse>,
}

/// Cached DoH response
#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub response: Vec<u8>,
    pub expiry: u64,
}

impl DohClient {
    /// Create new DoH client
    pub fn new(server_url: &str) -> Self {
        DohClient {
            server_url: server_url.to_string(),
            timeout_ms: 5000,
            cache: BTreeMap::new(),
        }
    }

    /// Create with common providers
    pub fn cloudflare() -> Self {
        Self::new("https://cloudflare-dns.com/dns-query")
    }

    pub fn google() -> Self {
        Self::new("https://dns.google/dns-query")
    }

    pub fn quad9() -> Self {
        Self::new("https://dns.quad9.net/dns-query")
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
        // Domain name (labels)
        for label in domain.split('.') {
            if !label.is_empty() {
                query.push(label.len() as u8);
                for c in label.chars() {
                    query.push(c as u8);
                }
            }
        }
        query.push(0); // Root label

        // QTYPE
        query.push((qtype as u16 >> 8) as u8);
        query.push((qtype as u16 & 0xFF) as u8);

        // QCLASS (IN = 1)
        query.push(0);
        query.push(1);

        query
    }

    /// Send DNS query over HTTPS (GET method)
    pub fn query_get(&self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);

        // Base64 URL encode (without padding)
        let encoded = base64url_encode(&dns_query);

        // Build URL
        let url = format!("{}?dns={}", self.server_url, encoded);

        // TODO: Make HTTPS request
        // For now, return error
        Err(DohError::HttpsNotSupported)
    }

    /// Send DNS query over HTTPS (POST method)
    pub fn query_post(&self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);

        // TODO: Make HTTPS POST request
        Err(DohError::HttpsNotSupported)
    }

    /// Parse DNS response
    pub fn parse_response(data: &[u8]) -> Result<DnsResponse, DohError> {
        if data.len() < 12 {
            return Err(DohError::InvalidResponse);
        }

        let header = DnsHeader::parse(data).map_err(|_| DohError::InvalidResponse)?;

        let mut response = DnsResponse {
            header,
            answers: Vec::new(),
        };

        // Parse questions (skip)
        let mut offset = 12;
        for _ in 0..header.qdcount {
            // Skip name
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
            offset += 4; // QTYPE + QCLASS
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

    fn parse_answer(data: &[u8], offset: &mut usize) -> Result<DnsAnswer, DohError> {
        // Parse name (might be compressed)
        let name = Self::parse_name(data, offset)?;

        if *offset + 10 > data.len() {
            return Err(DohError::InvalidResponse);
        }

        let rtype = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
        let _rclass = u16::from_be_bytes([data[*offset + 2], data[*offset + 3]]);
        let _ttl = u32::from_be_bytes([data[*offset + 4], data[*offset + 5], data[*offset + 6], data[*offset + 7]]);
        let rdlength = u16::from_be_bytes([data[*offset + 8], data[*offset + 9]]) as usize;
        *offset += 10;

        if *offset + rdlength > data.len() {
            return Err(DohError::InvalidResponse);
        }

        let rdata = data[*offset..*offset + rdlength].to_vec();
        *offset += rdlength;

        // Parse IP address from rdata
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

        Ok(DnsAnswer {
            name,
            rtype,
            rdata,
            ip,
        })
    }

    fn parse_name(data: &[u8], offset: &mut usize) -> Result<String, DohError> {
        let mut name = String::new();
        let mut jumped = false;
        let mut max_jumps = 5;
        let original_offset = *offset;

        loop {
            if *offset >= data.len() {
                return Err(DohError::InvalidResponse);
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1;
                break;
            }

            // Check for pointer
            if (len & 0xC0) == 0xC0 {
                if *offset + 1 >= data.len() {
                    return Err(DohError::InvalidResponse);
                }
                let ptr = (((data[*offset] & 0x3F) as usize) << 8) | (data[*offset + 1] as usize);
                if !jumped {
                    *offset += 2;
                    jumped = true;
                }
                *offset = ptr;
                max_jumps -= 1;
                if max_jumps == 0 {
                    return Err(DohError::InvalidResponse);
                }
                continue;
            }

            *offset += 1;
            if *offset + len > data.len() {
                return Err(DohError::InvalidResponse);
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

#[derive(Clone, Debug)]
pub enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

/// DNS Answer
#[derive(Clone, Debug)]
pub struct DnsAnswer {
    pub name: String,
    pub rtype: u16,
    pub rdata: Vec<u8>,
    pub ip: Option<IpAddr>,
}

/// DNS Response
#[derive(Clone, Debug)]
pub struct DnsResponse {
    pub header: DnsHeader,
    pub answers: Vec<DnsAnswer>,
}

impl DnsResponse {
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

/// DoH Error
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DohError {
    HttpsNotSupported,
    InvalidResponse,
    NetworkError,
    Timeout,
    ServerError(u16),
}

/// Base64 URL encode (without padding)
fn base64url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0F) << 2) | (b2 >> 6)] as char);
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[b2 & 0x3F] as char);
        }

        i += 3;
    }

    result
}

// Global DoH client
lazy_static::lazy_static! {
    static ref DOH_CLIENT: Mutex<Option<DohClient>> = Mutex::new(None);
}

/// Initialize DoH client
pub fn init_doh(server_url: &str) {
    *DOH_CLIENT.lock() = Some(DohClient::new(server_url));
}

/// Resolve domain using DoH
pub fn resolve_doh(domain: &str, qtype: DnsRecordType) -> Result<DnsResponse, DohError> {
    let client = DOH_CLIENT.lock();
    if let Some(client) = client.as_ref() {
        let response_data = client.query_get(domain, qtype)?;
        DohClient::parse_response(&response_data)
    } else {
        Err(DohError::HttpsNotSupported)
    }
}
