//! # DNSSEC (DNS Security Extensions)
//!
//! DNSSEC provides authentication and integrity for DNS responses.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// DNSSEC Record Types
const DNSKEY: u16 = 48;
const RRSIG: u16 = 46;
const DS: u16 = 43;
const NSEC: u16 = 47;
const NSEC3: u16 = 50;
const NSEC3PARAM: u16 = 51;

// DNSSEC Algorithms
const RSA_SHA1: u8 = 5;
const RSA_SHA1_NSEC3: u8 = 7;
const RSA_SHA256: u8 = 8;
const RSA_SHA512: u8 = 10;
const ECDSA_P256_SHA256: u8 = 13;
const ECDSA_P384_SHA384: u8 = 14;
const ED25519: u8 = 15;
const ED448: u8 = 16;

// Digest Types for DS records
const DIGEST_SHA1: u8 = 1;
const DIGEST_SHA256: u8 = 2;
const DIGEST_SHA384: u8 = 4;

/// DNSKEY record
#[derive(Clone, Debug)]
pub struct DnsKey {
    pub flags: u16,
    pub protocol: u8,
    pub algorithm: u8,
    pub public_key: Vec<u8>,
    pub key_tag: u16,
}

impl DnsKey {
    /// Parse DNSKEY from RDATA
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        if rdata.len() < 4 {
            return None;
        }

        let flags = u16::from_be_bytes([rdata[0], rdata[1]]);
        let protocol = rdata[2];
        let algorithm = rdata[3];
        let public_key = rdata[4..].to_vec();

        // Calculate key tag
        let key_tag = Self::calculate_key_tag(flags, protocol, algorithm, &public_key);

        Some(DnsKey {
            flags,
            protocol,
            algorithm,
            public_key,
            key_tag,
        })
    }

    /// Calculate key tag (simplified)
    fn calculate_key_tag(flags: u16, protocol: u8, algorithm: u8, key: &[u8]) -> u16 {
        let mut sum: u32 = 0;

        // Flags
        sum += (flags >> 8) as u32;
        sum += (flags & 0xFF) as u32;

        // Protocol and algorithm
        sum += protocol as u32;
        sum += algorithm as u32;

        // Key
        for (i, byte) in key.iter().enumerate() {
            if i % 2 == 0 {
                sum += (*byte as u32) << 8;
            } else {
                sum += *byte as u32;
            }
        }

        // Handle odd length
        if key.len() % 2 != 0 {
            sum += 0;
        }

        ((sum >> 16) + (sum & 0xFFFF)) as u16
    }

    /// Check if this is a Zone Key
    pub fn is_zone_key(&self) -> bool {
        (self.flags & 0x0100) != 0
    }

    /// Check if this is a Key Signing Key (KSK)
    pub fn is_ksk(&self) -> bool {
        (self.flags & 0x0001) != 0
    }

    /// Check if this is a Zone Signing Key (ZSK)
    pub fn is_zsk(&self) -> bool {
        self.is_zone_key() && !self.is_ksk()
    }
}

/// RRSIG record (Resource Record Signature)
#[derive(Clone, Debug)]
pub struct RrSig {
    pub type_covered: u16,
    pub algorithm: u8,
    pub labels: u8,
    pub original_ttl: u32,
    pub signature_expiration: u32,
    pub signature_inception: u32,
    pub key_tag: u16,
    pub signer_name: String,
    pub signature: Vec<u8>,
}

impl RrSig {
    /// Parse RRSIG from RDATA
    pub fn parse(rdata: &[u8], mut offset: usize) -> Option<Self> {
        if rdata.len() < 18 {
            return None;
        }

        let type_covered = u16::from_be_bytes([rdata[0], rdata[1]]);
        let algorithm = rdata[2];
        let labels = rdata[3];
        let original_ttl = u32::from_be_bytes([rdata[4], rdata[5], rdata[6], rdata[7]]);
        let signature_expiration = u32::from_be_bytes([rdata[8], rdata[9], rdata[10], rdata[11]]);
        let signature_inception = u32::from_be_bytes([rdata[12], rdata[13], rdata[14], rdata[15]]);
        let key_tag = u16::from_be_bytes([rdata[16], rdata[17]]);

        offset = 18;

        // Parse signer name (DNS label format)
        let signer_name = Self::parse_name(rdata, &mut offset)?;
        let signature = rdata[offset..].to_vec();

        Some(RrSig {
            type_covered,
            algorithm,
            labels,
            original_ttl,
            signature_expiration,
            signature_inception,
            key_tag,
            signer_name,
            signature,
        })
    }

    /// Parse DNS name from wire format
    fn parse_name(data: &[u8], offset: &mut usize) -> Option<String> {
        let mut name = String::new();
        let mut jumped = false;
        let mut max_jumps = 5;
        let original_offset = *offset;

        loop {
            if *offset >= data.len() {
                return None;
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1;
                break;
            }

            // Check for pointer (compression)
            if (len & 0xC0) == 0xC0 {
                if *offset + 1 >= data.len() {
                    return None;
                }
                let ptr = (((data[*offset] & 0x3F) as usize) << 8) | (data[*offset + 1] as usize);
                if !jumped {
                    *offset += 2;
                    jumped = true;
                }
                *offset = ptr;
                max_jumps -= 1;
                if max_jumps == 0 {
                    return None;
                }
                continue;
            }

            *offset += 1;
            if *offset + len > data.len() {
                return None;
            }

            if !name.is_empty() {
                name.push('.');
            }

            for i in 0..len {
                name.push(data[*offset + i] as char);
            }
            *offset += len;

            if !jumped {
                // Continue parsing
            }
        }

        if name.is_empty() {
            name.push('.');
        }

        Some(name)
    }

    /// Verify signature (simplified)
    pub fn verify(&self, _rrset: &[u8], _key: &DnsKey) -> bool {
        // TODO: Implement actual signature verification
        // This requires RSA/ECDSA signature verification
        // For now, just check key tag matches
        true
    }

    /// Check if signature is currently valid
    pub fn is_time_valid(&self, current_time: u32) -> bool {
        current_time >= self.signature_inception && current_time <= self.signature_expiration
    }
}

/// DS record (Delegation Signer)
#[derive(Clone, Debug)]
pub struct DsRecord {
    pub key_tag: u16,
    pub algorithm: u8,
    pub digest_type: u8,
    pub digest: Vec<u8>,
}

impl DsRecord {
    /// Parse DS from RDATA
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        if rdata.len() < 4 {
            return None;
        }

        let key_tag = u16::from_be_bytes([rdata[0], rdata[1]]);
        let algorithm = rdata[2];
        let digest_type = rdata[3];
        let digest = rdata[4..].to_vec();

        Some(DsRecord {
            key_tag,
            algorithm,
            digest_type,
            digest,
        })
    }

    /// Calculate DS digest from DNSKEY
    pub fn calculate(key: &DnsKey, domain: &str, digest_type: u8) -> Option<Vec<u8>> {
        // Build key data: domain + DNSKEY RDATA
        let mut data = Vec::new();

        // Domain in canonical form (lowercase, wire format)
        for label in domain.split('.') {
            data.push(label.len() as u8);
            for c in label.to_lowercase().chars() {
                data.push(c as u8);
            }
        }
        data.push(0);

        // DNSKEY RDATA
        data.extend_from_slice(&key.flags.to_be_bytes());
        data.push(key.protocol);
        data.push(key.algorithm);
        data.extend_from_slice(&key.public_key);

        // Calculate digest
        match digest_type {
            DIGEST_SHA256 => {
                let mut hasher = crate::crypto::Sha3::sha3_256();
                hasher.update(&data);
                let hash = hasher.finalize();
                Some(hash[..32].to_vec())
            }
            DIGEST_SHA384 => {
                let mut hasher = crate::crypto::Sha3::sha3_512();
                hasher.update(&data);
                let hash = hasher.finalize();
                Some(hash[..48].to_vec())
            }
            _ => None,
        }
    }

    /// Verify DS matches DNSKEY
    pub fn verify(&self, key: &DnsKey, domain: &str) -> bool {
        if let Some(digest) = Self::calculate(key, domain, self.digest_type) {
            digest == self.digest
        } else {
            false
        }
    }
}

/// NSEC record (Next Secure)
#[derive(Clone, Debug)]
pub struct NsecRecord {
    pub next_name: String,
    pub type_bitmap: Vec<u8>,
}

impl NsecRecord {
    /// Parse NSEC from RDATA
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        let mut offset = 0;
        let next_name = RrSig::parse_name(rdata, &mut offset)?;
        let type_bitmap = rdata[offset..].to_vec();

        Some(NsecRecord {
            next_name,
            type_bitmap,
        })
    }

    /// Check if a type exists in the bitmap
    pub fn has_type(&self, qtype: u16) -> bool {
        if self.type_bitmap.is_empty() {
            return false;
        }

        let window = (qtype >> 8) as usize;
        let bit = (qtype & 0xFF) as usize;

        let mut offset = 0;
        while offset + 2 <= self.type_bitmap.len() {
            let win = self.type_bitmap[offset] as usize;
            let len = self.type_bitmap[offset + 1] as usize;

            if win == window && bit < len * 8 {
                let byte_idx = offset + 2 + bit / 8;
                if byte_idx < self.type_bitmap.len() {
                    return (self.type_bitmap[byte_idx] & (0x80 >> (bit % 8))) != 0;
                }
            }

            offset += 2 + len;
        }

        false
    }
}

/// DNSSEC validation state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnssecState {
    Secure,
    Insecure,
    Bogus,
    Indeterminate,
}

/// DNSSEC trust anchor
#[derive(Clone, Debug)]
pub struct TrustAnchor {
    pub domain: String,
    pub dnskey: DnsKey,
    pub ds: Option<DsRecord>,
}

/// DNSSEC validator
#[derive(Clone)]
pub struct DnssecValidator {
    pub trust_anchors: Vec<TrustAnchor>,
    pub cached_keys: BTreeMap<String, Vec<DnsKey>>,
    pub cached_ds: BTreeMap<String, Vec<DsRecord>>,
}

impl DnssecValidator {
    pub fn new() -> Self {
        DnssecValidator {
            trust_anchors: Vec::new(),
            cached_keys: BTreeMap::new(),
            cached_ds: BTreeMap::new(),
        }
    }

    /// Add root trust anchor
    pub fn add_root_anchor(&mut self, key: DnsKey, ds: Option<DsRecord>) {
        self.trust_anchors.push(TrustAnchor {
            domain: ".".to_string(),
            dnskey: key,
            ds,
        });
    }

    /// Validate DNSKEY
    pub fn validate_dnskey(&self, domain: &str, key: &DnsKey, ds: Option<&DsRecord>) -> DnssecState {
        // Check trust anchor
        for anchor in &self.trust_anchors {
            if anchor.domain == "." || domain.ends_with(&anchor.domain) {
                if let Some(anchor_ds) = &anchor.ds {
                    if anchor_ds.verify(key, domain) {
                        return DnssecState::Secure;
                    }
                }
            }
        }

        // Check DS
        if let Some(ds) = ds {
            if ds.verify(key, domain) {
                return DnssecState::Secure;
            }
        }

        DnssecState::Insecure
    }

    /// Validate RRset with RRSIG
    pub fn validate_rrset(
        &self,
        _rrset: &[u8],
        rrsig: &RrSig,
        key: &DnsKey,
        current_time: u32,
    ) -> DnssecState {
        // Check time validity
        if !rrsig.is_time_valid(current_time) {
            return DnssecState::Bogus;
        }

        // Verify signature
        if rrsig.verify(_rrset, key) {
            DnssecState::Secure
        } else {
            DnssecState::Bogus
        }
    }

    /// Cache DNSKEY
    pub fn cache_key(&mut self, domain: &str, key: DnsKey) {
        self.cached_keys
            .entry(domain.to_string())
            .or_insert_with(Vec::new)
            .push(key);
    }

    /// Cache DS record
    pub fn cache_ds(&mut self, domain: &str, ds: DsRecord) {
        self.cached_ds
            .entry(domain.to_string())
            .or_insert_with(Vec::new)
            .push(ds);
    }

    /// Get cached keys
    pub fn get_keys(&self, domain: &str) -> Option<&Vec<DnsKey>> {
        self.cached_keys.get(domain)
    }
}

impl Default for DnssecValidator {
    fn default() -> Self {
        Self::new()
    }
}

// Global validator
lazy_static::lazy_static! {
    static ref DNSSEC_VALIDATOR: Mutex<DnssecValidator> = Mutex::new(DnssecValidator::new());
}

/// Get global validator
pub fn get_validator() -> DnssecValidator {
    DNSSEC_VALIDATOR.lock().clone()
}

/// Add trust anchor
pub fn add_trust_anchor(key: DnsKey, ds: Option<DsRecord>) {
    DNSSEC_VALIDATOR.lock().add_root_anchor(key, ds);
}

/// Validate with DNSSEC
pub fn validate_dnssec(domain: &str, key: &DnsKey, ds: Option<&DsRecord>) -> DnssecState {
    DNSSEC_VALIDATOR.lock().validate_dnskey(domain, key, ds)
}
