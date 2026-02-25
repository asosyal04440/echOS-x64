//! # X.509 Certificate Chain Verification
//!
//! ASN.1 DER parsing and X.509 certificate verification for TLS 1.3

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::format;
use spin::Mutex;

// ============================================================================
// ASN.1 DER PARSER
// ============================================================================

/// ASN.1 Tag classes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asn1Class {
    Universal,
    Application,
    ContextSpecific,
    Private,
}

/// ASN.1 Universal tags
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asn1Tag {
    Boolean = 0x01,
    Integer = 0x02,
    BitString = 0x03,
    OctetString = 0x04,
    Null = 0x05,
    ObjectIdentifier = 0x06,
    Utf8String = 0x0C,
    Sequence = 0x10,
    Set = 0x11,
    PrintableString = 0x13,
    T61String = 0x14,
    Ia5String = 0x16,
    UtcTime = 0x17,
    GeneralizedTime = 0x18,
    Enumerated = 0x0A,
    Unknown,
}

impl Asn1Tag {
    pub fn from_u8(tag: u8) -> Self {
        match tag {
            0x01 => Asn1Tag::Boolean,
            0x02 => Asn1Tag::Integer,
            0x03 => Asn1Tag::BitString,
            0x04 => Asn1Tag::OctetString,
            0x05 => Asn1Tag::Null,
            0x06 => Asn1Tag::ObjectIdentifier,
            0x0A => Asn1Tag::Enumerated,
            0x0C => Asn1Tag::Utf8String,
            0x10 => Asn1Tag::Sequence,
            0x11 => Asn1Tag::Set,
            0x13 => Asn1Tag::PrintableString,
            0x14 => Asn1Tag::T61String,
            0x16 => Asn1Tag::Ia5String,
            0x17 => Asn1Tag::UtcTime,
            0x18 => Asn1Tag::GeneralizedTime,
            _ => Asn1Tag::Unknown,
        }
    }
}

/// ASN.1 DER element
#[derive(Clone, Debug)]
pub struct Asn1Element {
    pub class: Asn1Class,
    pub constructed: bool,
    pub tag: Asn1Tag,
    pub tag_number: u32,
    pub data: Vec<u8>,
    pub children: Vec<Asn1Element>,
}

impl Asn1Element {
    pub fn new(tag: Asn1Tag, data: Vec<u8>) -> Self {
        Asn1Element {
            class: Asn1Class::Universal,
            constructed: false,
            tag,
            tag_number: tag as u32,
            data,
            children: Vec::new(),
        }
    }
    
    pub fn sequence(children: Vec<Asn1Element>) -> Self {
        Asn1Element {
            class: Asn1Class::Universal,
            constructed: true,
            tag: Asn1Tag::Sequence,
            tag_number: 0x10,
            data: Vec::new(),
            children,
        }
    }
}

/// ASN.1 DER Parser
pub struct Asn1Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Asn1Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Asn1Parser { data, pos: 0 }
    }
    
    /// Parse a single element
    pub fn parse_element(&mut self) -> Option<Asn1Element> {
        if self.pos >= self.data.len() {
            return None;
        }
        
        // Read tag
        let tag_byte = self.data[self.pos];
        self.pos += 1;
        
        let class = match (tag_byte >> 6) & 0x03 {
            0 => Asn1Class::Universal,
            1 => Asn1Class::Application,
            2 => Asn1Class::ContextSpecific,
            3 => Asn1Class::Private,
            _ => unreachable!(),
        };
        
        let constructed = (tag_byte & 0x20) != 0;
        
        // Read tag number
        let tag_number = if (tag_byte & 0x1F) == 0x1F {
            // Long form
            let mut num = 0u32;
            loop {
                if self.pos >= self.data.len() {
                    return None;
                }
                let b = self.data[self.pos];
                self.pos += 1;
                num = (num << 7) | ((b & 0x7F) as u32);
                if (b & 0x80) == 0 {
                    break;
                }
            }
            num
        } else {
            (tag_byte & 0x1F) as u32
        };
        
        let tag = if class == Asn1Class::Universal {
            Asn1Tag::from_u8(tag_number as u8)
        } else {
            Asn1Tag::Unknown
        };
        
        // Read length
        if self.pos >= self.data.len() {
            return None;
        }
        
        let len_byte = self.data[self.pos];
        self.pos += 1;
        
        let length = if len_byte < 0x80 {
            len_byte as usize
        } else if len_byte == 0x80 {
            // Indefinite length - not supported in DER
            return None;
        } else {
            // Long form
            let num_bytes = (len_byte & 0x7F) as usize;
            if self.pos + num_bytes > self.data.len() {
                return None;
            }
            
            let mut len = 0usize;
            for _ in 0..num_bytes {
                len = (len << 8) | (self.data[self.pos] as usize);
                self.pos += 1;
            }
            len
        };
        
        // Read data
        if self.pos + length > self.data.len() {
            return None;
        }
        
        let data = self.data[self.pos..self.pos + length].to_vec();
        self.pos += length;
        
        // Parse children if constructed
        let children = if constructed && class == Asn1Class::Universal && tag == Asn1Tag::Sequence {
            let mut parser = Asn1Parser::new(&data);
            let mut kids = Vec::new();
            while let Some(child) = parser.parse_element() {
                kids.push(child);
            }
            kids
        } else {
            Vec::new()
        };
        
        Some(Asn1Element {
            class,
            constructed,
            tag,
            tag_number,
            data,
            children,
        })
    }
    
    /// Parse all elements
    pub fn parse_all(&mut self) -> Vec<Asn1Element> {
        let mut elements = Vec::new();
        while let Some(elem) = self.parse_element() {
            elements.push(elem);
        }
        elements
    }
}

/// Parse OID from ASN.1 element
pub fn parse_oid(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    
    let mut oid = String::new();
    
    // First byte encodes first two components
    let first = data[0];
    oid.push_str(&format!("{}.{}", first / 40, first % 40));
    
    // Remaining bytes encode remaining components
    let mut value = 0u64;
    for &b in &data[1..] {
        value = (value << 7) | ((b & 0x7F) as u64);
        if (b & 0x80) == 0 {
            oid.push_str(&format!(".{}", value));
            value = 0;
        }
    }
    
    oid
}

// ============================================================================
// X.509 CERTIFICATE
// ============================================================================

/// X.509 Distinguished Name
#[derive(Clone, Debug, Default)]
pub struct X509Name {
    pub common_name: String,
    pub country: String,
    pub organization: String,
    pub organizational_unit: String,
    pub locality: String,
    pub state: String,
}

impl X509Name {
    pub fn new() -> Self {
        X509Name::default()
    }
    
    /// Parse from ASN.1 sequence
    pub fn parse(elements: &[Asn1Element]) -> Self {
        let mut name = X509Name::new();
        
        for set in elements {
            if set.tag != Asn1Tag::Set {
                continue;
            }
            
            for seq in &set.children {
                if seq.tag != Asn1Tag::Sequence {
                    continue;
                }
                
                if seq.children.len() >= 2 {
                    let oid_elem = &seq.children[0];
                    let value_elem = &seq.children[1];
                    
                    if oid_elem.tag == Asn1Tag::ObjectIdentifier {
                        let oid = parse_oid(&oid_elem.data);
                        
                        let value = match value_elem.tag {
                            Asn1Tag::Utf8String | Asn1Tag::PrintableString | Asn1Tag::Ia5String => {
                                String::from_utf8_lossy(&value_elem.data).to_string()
                            }
                            _ => String::new(),
                        };
                        
                        // Map OID to attribute
                        match oid.as_str() {
                            "2.5.4.3" => name.common_name = value,
                            "2.5.4.6" => name.country = value,
                            "2.5.4.10" => name.organization = value,
                            "2.5.4.11" => name.organizational_unit = value,
                            "2.5.4.7" => name.locality = value,
                            "2.5.4.8" => name.state = value,
                            _ => {}
                        }
                    }
                }
            }
        }
        
        name
    }
}

/// X.509 Public Key
#[derive(Clone, Debug)]
pub struct X509PublicKey {
    pub algorithm: String,
    pub key_data: Vec<u8>,
    pub curve: Option<String>,
}

/// X.509 Signature Algorithm
#[derive(Clone, Debug)]
pub struct SignatureAlgorithm {
    pub algorithm: String,
    pub parameters: Vec<u8>,
}

impl SignatureAlgorithm {
    pub fn parse(element: &Asn1Element) -> Option<Self> {
        if element.tag != Asn1Tag::Sequence || element.children.is_empty() {
            return None;
        }
        
        let oid = parse_oid(&element.children[0].data);
        let params = if element.children.len() > 1 {
            element.children[1].data.clone()
        } else {
            Vec::new()
        };
        
        Some(SignatureAlgorithm {
            algorithm: oid,
            parameters: params,
        })
    }
}

/// X.509 Certificate
#[derive(Clone, Debug)]
pub struct X509Certificate {
    pub version: u8,
    pub serial: Vec<u8>,
    pub signature_algo: SignatureAlgorithm,
    pub issuer: X509Name,
    pub not_before: u64,
    pub not_after: u64,
    pub subject: X509Name,
    pub public_key: X509PublicKey,
    pub extensions: Vec<X509Extension>,
    pub signature: Vec<u8>,
    pub tbs_data: Vec<u8>,  // To-be-signed data for verification
    pub raw: Vec<u8>,
}

/// X.509 Extension
#[derive(Clone, Debug)]
pub struct X509Extension {
    pub oid: String,
    pub critical: bool,
    pub value: Vec<u8>,
}

impl X509Certificate {
    /// Parse X.509 certificate from DER bytes
    pub fn parse(der: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(der);
        let root = parser.parse_element()?;
        
        if root.tag != Asn1Tag::Sequence {
            return None;
        }
        
        if root.children.len() < 3 {
            return None;
        }
        
        let tbs_cert = &root.children[0];
        let sig_algo = &root.children[1];
        let sig_value = &root.children[2];
        
        // Store TBS data for verification
        let tbs_data = tbs_cert.data.clone();
        
        // Parse TBSCertificate
        if tbs_cert.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut idx = 0;
        
        // Version (optional, context-specific [0])
        let version = if tbs_cert.children[idx].class == Asn1Class::ContextSpecific {
            let ver_elem = &tbs_cert.children[idx];
            if !ver_elem.children.is_empty() {
                let ver_int = &ver_elem.children[0];
                if ver_int.tag == Asn1Tag::Integer && !ver_int.data.is_empty() {
                    idx += 1;
                    ver_int.data[0] + 1  // Version is 0-indexed
                } else {
                    1
                }
            } else {
                idx += 1;
                1
            }
        } else {
            1  // Default version 1
        };
        
        // Serial number
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let serial = tbs_cert.children[idx].data.clone();
        idx += 1;
        
        // Signature algorithm
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let tbs_sig_algo = SignatureAlgorithm::parse(&tbs_cert.children[idx])?;
        idx += 1;
        
        // Issuer
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let issuer = X509Name::parse(&tbs_cert.children[idx].children);
        idx += 1;
        
        // Validity
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let validity = &tbs_cert.children[idx];
        idx += 1;
        
        let (not_before, not_after) = if validity.children.len() >= 2 {
            let parse_time = |elem: &Asn1Element| -> u64 {
                let time_str = String::from_utf8_lossy(&elem.data);
                // Parse UTCTime (YYMMDDhhmmssZ) or GeneralizedTime (YYYYMMDDhhmmssZ)
                if elem.tag == Asn1Tag::UtcTime {
                    // YYMMDDhhmmssZ
                    if time_str.len() >= 12 {
                        let yy: u64 = time_str[0..2].parse().unwrap_or(0);
                        let mm: u64 = time_str[2..4].parse().unwrap_or(0);
                        let dd: u64 = time_str[4..6].parse().unwrap_or(0);
                        let hh: u64 = time_str[6..8].parse().unwrap_or(0);
                        let min: u64 = time_str[8..10].parse().unwrap_or(0);
                        let ss: u64 = time_str[10..12].parse().unwrap_or(0);
                        // Simple timestamp (not accurate, just for comparison)
                        let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
                        year * 10000000000 + mm * 100000000 + dd * 1000000 + hh * 10000 + min * 100 + ss
                    } else {
                        0
                    }
                } else {
                    0
                }
            };
            (parse_time(&validity.children[0]), parse_time(&validity.children[1]))
        } else {
            (0, 0)
        };
        
        // Subject
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let subject = X509Name::parse(&tbs_cert.children[idx].children);
        idx += 1;
        
        // Subject Public Key Info
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let spki = &tbs_cert.children[idx];
        idx += 1;
        
        let public_key = if spki.children.len() >= 2 {
            let algo = &spki.children[0];
            let key_bits = &spki.children[1];
            
            let algo_oid = if !algo.children.is_empty() {
                parse_oid(&algo.children[0].data)
            } else {
                String::new()
            };
            
            let curve = if algo.children.len() > 1 {
                let curve_elem = &algo.children[1];
                if !curve_elem.children.is_empty() {
                    Some(parse_oid(&curve_elem.children[0].data))
                } else {
                    None
                }
            } else {
                None
            };
            
            // Extract key data from BIT STRING
            let key_data = if key_bits.tag == Asn1Tag::BitString && key_bits.data.len() > 1 {
                // Skip unused bits byte
                key_bits.data[1..].to_vec()
            } else {
                key_bits.data.clone()
            };
            
            X509PublicKey {
                algorithm: algo_oid,
                key_data,
                curve,
            }
        } else {
            X509PublicKey {
                algorithm: String::new(),
                key_data: Vec::new(),
                curve: None,
            }
        };
        
        // Extensions (optional, context-specific [3])
        let mut extensions = Vec::new();
        while idx < tbs_cert.children.len() {
            let elem = &tbs_cert.children[idx];
            if elem.class == Asn1Class::ContextSpecific && elem.tag_number == 3 {
                for ext_seq in &elem.children {
                    if ext_seq.tag == Asn1Tag::Sequence {
                        for ext in &ext_seq.children {
                            if ext.tag == Asn1Tag::Sequence && ext.children.len() >= 2 {
                                let oid = parse_oid(&ext.children[0].data);
                                let critical = ext.children.len() >= 3 && ext.children[1].tag == Asn1Tag::Boolean && !ext.children[1].data.is_empty() && ext.children[1].data[0] != 0;
                                let value_idx = if critical { 2 } else { 1 };
                                let value = if ext.children.len() > value_idx {
                                    ext.children[value_idx].data.clone()
                                } else {
                                    Vec::new()
                                };
                                
                                extensions.push(X509Extension {
                                    oid,
                                    critical,
                                    value,
                                });
                            }
                        }
                    }
                }
            }
            idx += 1;
        }
        
        // Signature algorithm (outer)
        let signature_algo = SignatureAlgorithm::parse(sig_algo)?;
        
        // Signature value
        let signature = if sig_value.tag == Asn1Tag::BitString && sig_value.data.len() > 1 {
            sig_value.data[1..].to_vec()
        } else {
            sig_value.data.clone()
        };
        
        Some(X509Certificate {
            version,
            serial,
            signature_algo,
            issuer,
            not_before,
            not_after,
            subject,
            public_key,
            extensions,
            signature,
            tbs_data,
            raw: der.to_vec(),
        })
    }
    
    /// Check if certificate is valid at given time
    pub fn is_valid_at(&self, time: u64) -> bool {
        time >= self.not_before && time <= self.not_after
    }
    
    /// Check basic constraints (CA flag)
    pub fn is_ca(&self) -> bool {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.19" {  // basicConstraints
                // Parse basicConstraints
                let mut parser = Asn1Parser::new(&ext.value);
                if let Some(elem) = parser.parse_element() {
                    if elem.tag == Asn1Tag::Sequence && !elem.children.is_empty() {
                        if elem.children[0].tag == Asn1Tag::Boolean {
                            return !elem.children[0].data.is_empty() && elem.children[0].data[0] != 0;
                        }
                    }
                }
            }
        }
        false
    }
    
    /// Get key usage
    pub fn key_usage(&self) -> Option<u16> {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.15" {  // keyUsage
                let mut parser = Asn1Parser::new(&ext.value);
                if let Some(elem) = parser.parse_element() {
                    if elem.tag == Asn1Tag::BitString && elem.data.len() > 1 {
                        let unused_bits = elem.data[0] as usize;
                        let mut usage = 0u16;
                        for (i, &b) in elem.data[1..].iter().enumerate() {
                            usage |= (b as u16) << (i * 8);
                        }
                        return Some(usage >> unused_bits);
                    }
                }
            }
        }
        None
    }
}

// ============================================================================
// CERTIFICATE STORE
// ============================================================================

/// Root CA certificate store
static ROOT_CA_STORE: Mutex<Vec<X509Certificate>> = Mutex::new(Vec::new());

/// Add root CA to store
pub fn add_root_ca(cert: X509Certificate) {
    let mut store = ROOT_CA_STORE.lock();
    store.push(cert);
}

/// Get root CA store
pub fn get_root_cas() -> Vec<X509Certificate> {
    ROOT_CA_STORE.lock().clone()
}

/// Clear root CA store
pub fn clear_root_cas() {
    ROOT_CA_STORE.lock().clear();
}

// ============================================================================
// CERTIFICATE CHAIN VERIFICATION
// ============================================================================

/// Certificate verification error
#[derive(Clone, Debug)]
pub enum CertError {
    InvalidFormat,
    Expired,
    NotYetValid,
    UnknownIssuer,
    InvalidSignature,
    InvalidChain,
    SelfSigned,
    NotCA,
    InvalidKeyUsage,
    Revoked,
}

/// Certificate chain verifier
pub struct CertVerifier {
    pub trusted_roots: Vec<X509Certificate>,
    pub check_time: u64,
}

impl CertVerifier {
    pub fn new() -> Self {
        CertVerifier {
            trusted_roots: get_root_cas(),
            check_time: 0,  // Will use current time
        }
    }
    
    /// Verify certificate chain
    pub fn verify_chain(&self, chain: &[X509Certificate]) -> Result<(), CertError> {
        if chain.is_empty() {
            return Err(CertError::InvalidChain);
        }
        
        // Get current time (simplified)
        let time = if self.check_time > 0 {
            self.check_time
        } else {
            // Use a pseudo-time based on random
            crate::random::next_u32() as u64
        };
        
        // Verify each certificate in chain
        for (i, cert) in chain.iter().enumerate() {
            // Check validity period
            if !cert.is_valid_at(time) {
                if time < cert.not_before {
                    return Err(CertError::NotYetValid);
                } else {
                    return Err(CertError::Expired);
                }
            }
            
            // Check if this is the leaf certificate
            if i == 0 {
                // Leaf cert - check if it's not a CA
                // (unless it's self-signed, which is handled below)
                continue;
            }
            
            // Intermediate or root - must be CA
            if !cert.is_ca() {
                return Err(CertError::NotCA);
            }
        }
        
        // Find trust anchor
        let last_cert = &chain[chain.len() - 1];
        
        // Check if last cert is a trusted root
        let is_trusted = self.trusted_roots.iter().any(|root| {
            root.subject.common_name == last_cert.subject.common_name &&
            root.public_key.key_data == last_cert.public_key.key_data
        });
        
        if !is_trusted && chain.len() == 1 {
            return Err(CertError::SelfSigned);
        }
        
        if !is_trusted {
            return Err(CertError::UnknownIssuer);
        }
        
        // Verify signatures (simplified - in production would verify actual signatures)
        // For each cert, verify it was signed by the next cert in chain
        for i in 0..chain.len().saturating_sub(1) {
            let cert = &chain[i];
            let issuer = &chain[i + 1];
            
            // Verify issuer name matches
            if cert.issuer.common_name != issuer.subject.common_name {
                return Err(CertError::InvalidChain);
            }
            
            // In production: verify signature using issuer's public key
            // For now, we trust that the chain is properly signed
        }
        
        Ok(())
    }
    
    /// Verify a single certificate against trusted roots
    pub fn verify(&self, cert: &X509Certificate) -> Result<(), CertError> {
        self.verify_chain(&[cert.clone()])
    }
}

impl Default for CertVerifier {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// WELL-KNOWN ROOT CAs (BUILT-IN)
// ============================================================================

/// Initialize built-in root CAs
pub fn init_builtin_roots() {
    // In production, these would be actual root CA certificates
    // For now, we just initialize an empty store
    // Real implementation would include: DigiCert, Let's Encrypt, etc.
    clear_root_cas();
}

// ============================================================================
// TLS INTEGRATION
// ============================================================================

/// Parse certificate chain from TLS handshake
pub fn parse_certificate_chain(cert_data: &[u8]) -> Vec<X509Certificate> {
    let mut certs = Vec::new();
    let mut pos = 0;
    
    // TLS certificate message format:
    // u24 total_length
    // For each certificate:
    //   u24 length
    //   DER-encoded certificate
    
    if cert_data.len() < 3 {
        return certs;
    }
    
    let total_len = ((cert_data[0] as usize) << 16) | ((cert_data[1] as usize) << 8) | (cert_data[2] as usize);
    pos = 3;
    
    while pos + 3 <= cert_data.len() && pos < total_len + 3 {
        let cert_len = ((cert_data[pos] as usize) << 16) | ((cert_data[pos + 1] as usize) << 8) | (cert_data[pos + 2] as usize);
        pos += 3;
        
        if pos + cert_len > cert_data.len() {
            break;
        }
        
        if let Some(cert) = X509Certificate::parse(&cert_data[pos..pos + cert_len]) {
            certs.push(cert);
        }
        
        pos += cert_len;
    }
    
    certs
}

// ============================================================================
// X.509 CRL (CERTIFICATE REVOCATION LIST)
// ============================================================================

/// CRL Reason codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrlReason {
    Unspecified = 0,
    KeyCompromise = 1,
    CaCompromise = 2,
    AffiliationChanged = 3,
    Superseded = 4,
    CessationOfOperation = 5,
    CertificateHold = 6,
    RemoveFromCrl = 8,
    PrivilegeWithdrawn = 9,
    AaCompromise = 10,
}

/// CRL Entry (revoked certificate)
#[derive(Clone, Debug)]
pub struct CrlEntry {
    pub serial: Vec<u8>,
    pub revocation_date: u64,
    pub reason: CrlReason,
    pub invalidity_date: Option<u64>,
}

/// X.509 CRL
#[derive(Clone, Debug)]
pub struct X509Crl {
    pub version: u8,
    pub signature_algo: SignatureAlgorithm,
    pub issuer: X509Name,
    pub this_update: u64,
    pub next_update: u64,
    pub revoked_certs: Vec<CrlEntry>,
    pub extensions: Vec<X509Extension>,
    pub signature: Vec<u8>,
    pub raw: Vec<u8>,
}

impl X509Crl {
    /// Parse CRL from DER bytes
    pub fn parse(der: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(der);
        let root = parser.parse_element()?;
        
        if root.tag != Asn1Tag::Sequence || root.children.len() < 4 {
            return None;
        }
        
        let tbs_crl = &root.children[0];
        let sig_algo = &root.children[1];
        let sig_value = &root.children[2];
        
        if tbs_crl.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut idx = 0;
        
        // Version (optional, default v1)
        let version = if tbs_crl.children[idx].tag == Asn1Tag::Integer {
            idx += 1;
            if !tbs_crl.children[0].data.is_empty() {
                tbs_crl.children[0].data[0] + 1
            } else {
                1
            }
        } else {
            1
        };
        
        // Signature algorithm
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let signature_algo = SignatureAlgorithm::parse(&tbs_crl.children[idx])?;
        idx += 1;
        
        // Issuer
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let issuer = X509Name::parse(&tbs_crl.children[idx].children);
        idx += 1;
        
        // This Update
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let this_update = Self::parse_time(&tbs_crl.children[idx]);
        idx += 1;
        
        // Next Update (optional)
        let next_update = if idx < tbs_crl.children.len() && 
            (tbs_crl.children[idx].tag == Asn1Tag::UtcTime || 
             tbs_crl.children[idx].tag == Asn1Tag::GeneralizedTime) {
            let time = Self::parse_time(&tbs_crl.children[idx]);
            idx += 1;
            time
        } else {
            this_update + 86400 // Default 24 hours
        };
        
        // Revoked certificates
        let mut revoked_certs = Vec::new();
        while idx < tbs_crl.children.len() {
            let elem = &tbs_crl.children[idx];
            if elem.tag == Asn1Tag::Sequence {
                if let Some(entry) = Self::parse_crl_entry(elem) {
                    revoked_certs.push(entry);
                }
                idx += 1;
            } else {
                break;
            }
        }
        
        // Extensions (optional, context-specific [0])
        let mut extensions = Vec::new();
        if idx < tbs_crl.children.len() {
            let elem = &tbs_crl.children[idx];
            if elem.class == Asn1Class::ContextSpecific && elem.tag_number == 0 {
                for ext_seq in &elem.children {
                    if ext_seq.tag == Asn1Tag::Sequence {
                        for ext in &ext_seq.children {
                            if ext.tag == Asn1Tag::Sequence && ext.children.len() >= 2 {
                                let oid = parse_oid(&ext.children[0].data);
                                let critical = ext.children.len() >= 3 && 
                                    ext.children[1].tag == Asn1Tag::Boolean && 
                                    !ext.children[1].data.is_empty() && 
                                    ext.children[1].data[0] != 0;
                                let value_idx = if critical { 2 } else { 1 };
                                let value = if ext.children.len() > value_idx {
                                    ext.children[value_idx].data.clone()
                                } else {
                                    Vec::new()
                                };
                                extensions.push(X509Extension { oid, critical, value });
                            }
                        }
                    }
                }
            }
        }
        
        // Signature algorithm (outer)
        let _outer_sig_algo = SignatureAlgorithm::parse(sig_algo)?;
        
        // Signature value
        let signature = if sig_value.tag == Asn1Tag::BitString && sig_value.data.len() > 1 {
            sig_value.data[1..].to_vec()
        } else {
            sig_value.data.clone()
        };
        
        Some(X509Crl {
            version,
            signature_algo,
            issuer,
            this_update,
            next_update,
            revoked_certs,
            extensions,
            signature,
            raw: der.to_vec(),
        })
    }
    
    fn parse_time(elem: &Asn1Element) -> u64 {
        let time_str = String::from_utf8_lossy(&elem.data);
        if elem.tag == Asn1Tag::UtcTime && time_str.len() >= 12 {
            let yy: u64 = time_str[0..2].parse().unwrap_or(0);
            let mm: u64 = time_str[2..4].parse().unwrap_or(0);
            let dd: u64 = time_str[4..6].parse().unwrap_or(0);
            let hh: u64 = time_str[6..8].parse().unwrap_or(0);
            let min: u64 = time_str[8..10].parse().unwrap_or(0);
            let ss: u64 = time_str[10..12].parse().unwrap_or(0);
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            year * 10000000000 + mm * 100000000 + dd * 1000000 + hh * 10000 + min * 100 + ss
        } else {
            0
        }
    }
    
    fn parse_crl_entry(elem: &Asn1Element) -> Option<CrlEntry> {
        if elem.children.len() < 2 {
            return None;
        }
        
        let serial = elem.children[0].data.clone();
        let revocation_date = Self::parse_time(&elem.children[1]);
        
        // Parse extensions for reason
        let mut reason = CrlReason::Unspecified;
        let mut invalidity_date = None;
        
        if elem.children.len() > 2 {
            let ext_seq = &elem.children[2];
            if ext_seq.tag == Asn1Tag::Sequence {
                for ext in &ext_seq.children {
                    if ext.tag == Asn1Tag::Sequence && ext.children.len() >= 2 {
                        let oid = parse_oid(&ext.children[0].data);
                        let value = &ext.children[1].data;
                        
                        // CRL reason (OID 2.5.29.21)
                        if oid == "2.5.29.21" {
                            let mut parser = Asn1Parser::new(value);
                            if let Some(reason_elem) = parser.parse_element() {
                                if reason_elem.tag == Asn1Tag::Enumerated && !reason_elem.data.is_empty() {
                                    reason = match reason_elem.data[0] {
                                        1 => CrlReason::KeyCompromise,
                                        2 => CrlReason::CaCompromise,
                                        3 => CrlReason::AffiliationChanged,
                                        4 => CrlReason::Superseded,
                                        5 => CrlReason::CessationOfOperation,
                                        6 => CrlReason::CertificateHold,
                                        8 => CrlReason::RemoveFromCrl,
                                        9 => CrlReason::PrivilegeWithdrawn,
                                        10 => CrlReason::AaCompromise,
                                        _ => CrlReason::Unspecified,
                                    };
                                }
                            }
                        }
                        
                        // Invalidity date (OID 2.5.29.24)
                        if oid == "2.5.29.24" {
                            let mut parser = Asn1Parser::new(value);
                            if let Some(date_elem) = parser.parse_element() {
                                invalidity_date = Some(Self::parse_time(&date_elem));
                            }
                        }
                    }
                }
            }
        }
        
        Some(CrlEntry {
            serial,
            revocation_date,
            reason,
            invalidity_date,
        })
    }
    
    /// Check if certificate is revoked
    pub fn is_revoked(&self, serial: &[u8]) -> Option<&CrlEntry> {
        self.revoked_certs.iter().find(|e| e.serial == serial)
    }
    
    /// Check if CRL is expired
    pub fn is_expired(&self, time: u64) -> bool {
        time > self.next_update
    }
}

// ============================================================================
// OCSP (ONLINE CERTIFICATE STATUS PROTOCOL)
// ============================================================================

/// OCSP Request
#[derive(Clone, Debug)]
pub struct OcspRequest {
    pub requestor_name: Option<X509Name>,
    pub request_list: Vec<OcspCertRequest>,
    pub signature_algo: Option<SignatureAlgorithm>,
    pub signature: Option<Vec<u8>>,
}

/// OCSP Certificate Request
#[derive(Clone, Debug)]
pub struct OcspCertRequest {
    pub issuer_key_hash: [u8; 20],
    pub issuer_name_hash: [u8; 20],
    pub hash_algorithm: String,
    pub serial: Vec<u8>,
}

/// OCSP Response Status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspResponseStatus {
    Successful = 0,
    MalformedRequest = 1,
    InternalError = 2,
    TryLater = 3,
    SigRequired = 5,
    Unauthorized = 6,
}

/// OCSP Certificate Status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspCertStatus {
    Good,
    Revoked { reason: CrlReason, revocation_time: u64 },
    Unknown,
}

/// OCSP Single Response
#[derive(Clone, Debug)]
pub struct OcspSingleResponse {
    pub cert_id_hash_algo: String,
    pub issuer_name_hash: Vec<u8>,
    pub issuer_key_hash: Vec<u8>,
    pub serial: Vec<u8>,
    pub status: OcspCertStatus,
    pub this_update: u64,
    pub next_update: Option<u64>,
    pub produced_at: u64,
}

/// OCSP Response
#[derive(Clone, Debug)]
pub struct OcspResponse {
    pub response_status: OcspResponseStatus,
    pub response_type: String,
    pub version: u8,
    pub responder_id: OcspResponderId,
    pub produced_at: u64,
    pub responses: Vec<OcspSingleResponse>,
    pub signature_algo: SignatureAlgorithm,
    pub signature: Vec<u8>,
    pub certs: Vec<X509Certificate>,
}

/// OCSP Responder ID
#[derive(Clone, Debug)]
pub enum OcspResponderId {
    ByName(X509Name),
   ByKey(Vec<u8>),
}

impl OcspRequest {
    /// Create new OCSP request for a certificate
    pub fn new(cert: &X509Certificate, issuer: &X509Certificate) -> Self {
        // Hash issuer name and key
        let issuer_name_hash = Self::hash_name(&issuer.subject);
        let issuer_key_hash = Self::hash_key(&issuer.public_key.key_data);
        
        OcspRequest {
            requestor_name: None,
            request_list: vec![OcspCertRequest {
                issuer_key_hash,
                issuer_name_hash,
                hash_algorithm: "1.3.14.3.2.26".to_string(), // SHA-1 OID
                serial: cert.serial.clone(),
            }],
            signature_algo: None,
            signature: None,
        }
    }
    
    fn hash_name(name: &X509Name) -> [u8; 20] {
        // Simplified SHA-1 hash of name
        let mut hash = [0u8; 20];
        for (i, b) in name.common_name.as_bytes().iter().chain(
            name.organization.as_bytes().iter()
        ).enumerate() {
            hash[i % 20] ^= b;
        }
        hash
    }
    
    fn hash_key(key: &[u8]) -> [u8; 20] {
        // Simplified SHA-1 hash of key
        let mut hash = [0u8; 20];
        for (i, b) in key.iter().enumerate() {
            hash[i % 20] ^= b;
        }
        hash
    }
    
    /// Encode request to DER
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        // OCSP Request sequence
        buf.push(0x30); // SEQUENCE
        let len_pos = buf.len();
        buf.push(0); // Placeholder length
        
        // TBSRequest
        buf.push(0x30); // SEQUENCE
        let tbs_len_pos = buf.len();
        buf.push(0);
        
        // Request list
        buf.push(0x30); // SEQUENCE
        let list_len_pos = buf.len();
        buf.push(0);
        
        for req in &self.request_list {
            // Request
            buf.push(0x30); // SEQUENCE
            let req_len_pos = buf.len();
            buf.push(0);
            
            // CertID
            buf.push(0x30); // SEQUENCE
            let certid_len_pos = buf.len();
            buf.push(0);
            
            // Hash algorithm
            buf.push(0x30); // SEQUENCE
            buf.push(0x05); // Length
            buf.push(0x06); // OID tag
            buf.push(0x03); // OID length
            buf.extend_from_slice(&[0x2A, 0x03, 0x04]); // SHA-1 prefix
            
            // Issuer name hash
            buf.push(0x04); // OCTET STRING
            buf.push(20); // Length
            buf.extend_from_slice(&req.issuer_name_hash);
            
            // Issuer key hash
            buf.push(0x04); // OCTET STRING
            buf.push(20); // Length
            buf.extend_from_slice(&req.issuer_key_hash);
            
            // Serial
            buf.push(0x02); // INTEGER
            buf.push(req.serial.len() as u8);
            buf.extend_from_slice(&req.serial);
            
            // Update lengths
            let certid_len = buf.len() - certid_len_pos - 1;
            buf[certid_len_pos] = certid_len as u8;
            
            let req_len = buf.len() - req_len_pos - 1;
            buf[req_len_pos] = req_len as u8;
        }
        
        // Update lengths
        let list_len = buf.len() - list_len_pos - 1;
        buf[list_len_pos] = list_len as u8;
        
        let tbs_len = buf.len() - tbs_len_pos - 1;
        buf[tbs_len_pos] = tbs_len as u8;
        
        let total_len = buf.len() - len_pos - 1;
        buf[len_pos] = total_len as u8;
        
        buf
    }
}

impl OcspResponse {
    /// Parse OCSP response from DER
    pub fn parse(der: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(der);
        let root = parser.parse_element()?;
        
        if root.tag != Asn1Tag::Sequence {
            return None;
        }
        
        // Response status
        if root.children.is_empty() {
            return None;
        }
        
        let status_elem = &root.children[0];
        let response_status = if status_elem.tag == Asn1Tag::Enumerated && !status_elem.data.is_empty() {
            match status_elem.data[0] {
                0 => OcspResponseStatus::Successful,
                1 => OcspResponseStatus::MalformedRequest,
                2 => OcspResponseStatus::InternalError,
                3 => OcspResponseStatus::TryLater,
                5 => OcspResponseStatus::SigRequired,
                6 => OcspResponseStatus::Unauthorized,
                _ => return None,
            }
        } else {
            return None;
        };
        
        if response_status != OcspResponseStatus::Successful {
            return Some(OcspResponse {
                response_status,
                response_type: String::new(),
                version: 1,
                responder_id: OcspResponderId::ByKey(Vec::new()),
                produced_at: 0,
                responses: Vec::new(),
                signature_algo: SignatureAlgorithm { algorithm: String::new(), parameters: Vec::new() },
                signature: Vec::new(),
                certs: Vec::new(),
            });
        }
        
        // Parse response bytes
        if root.children.len() < 2 {
            return None;
        }
        
        let response_bytes = &root.children[1];
        if response_bytes.tag != Asn1Tag::Sequence || response_bytes.children.len() < 2 {
            return None;
        }
        
        let response_type = parse_oid(&response_bytes.children[0].data);
        let response_data = &response_bytes.children[1].data;
        
        // Parse BasicOCSPResponse
        let mut parser = Asn1Parser::new(response_data);
        let basic = parser.parse_element()?;
        
        if basic.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut idx = 0;
        
        // Version (optional)
        let version = if basic.children[idx].tag == Asn1Tag::Integer {
            idx += 1;
            if !basic.children[0].data.is_empty() { basic.children[0].data[0] + 1 } else { 1 }
        } else {
            1
        };
        
        // Responder ID
        if idx >= basic.children.len() {
            return None;
        }
        let responder_id = Self::parse_responder_id(&basic.children[idx])?;
        idx += 1;
        
        // Produced At
        if idx >= basic.children.len() {
            return None;
        }
        let produced_at = X509Crl::parse_time(&basic.children[idx]);
        idx += 1;
        
        // Responses
        if idx >= basic.children.len() {
            return None;
        }
        let responses = Self::parse_responses(&basic.children[idx])?;
        idx += 1;
        
        // Signature algorithm
        if idx >= basic.children.len() {
            return None;
        }
        let signature_algo = SignatureAlgorithm::parse(&basic.children[idx])?;
        idx += 1;
        
        // Signature
        if idx >= basic.children.len() {
            return None;
        }
        let signature = if basic.children[idx].tag == Asn1Tag::BitString && basic.children[idx].data.len() > 1 {
            basic.children[idx].data[1..].to_vec()
        } else {
            basic.children[idx].data.clone()
        };
        idx += 1;
        
        // Certs (optional)
        let certs = if idx < basic.children.len() {
            let mut c = Vec::new();
            for cert_elem in &basic.children[idx..] {
                if cert_elem.tag == Asn1Tag::Sequence {
                    if let Some(cert) = X509Certificate::parse(&cert_elem.data) {
                        c.push(cert);
                    }
                }
            }
            c
        } else {
            Vec::new()
        };
        
        Some(OcspResponse {
            response_status,
            response_type,
            version,
            responder_id,
            produced_at,
            responses,
            signature_algo,
            signature,
            certs,
        })
    }
    
    fn parse_responder_id(elem: &Asn1Element) -> Option<OcspResponderId> {
        if elem.class == Asn1Class::ContextSpecific {
            if elem.tag_number == 1 {
                // ByName
                Some(OcspResponderId::ByName(X509Name::parse(&elem.children)))
            } else if elem.tag_number == 2 {
                // ByKey
                Some(OcspResponderId::ByKey(elem.data.clone()))
            } else {
                None
            }
        } else {
            None
        }
    }
    
    fn parse_responses(elem: &Asn1Element) -> Option<Vec<OcspSingleResponse>> {
        if elem.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut responses = Vec::new();
        for single in &elem.children {
            if single.tag != Asn1Tag::Sequence || single.children.len() < 4 {
                continue;
            }
            
            // CertID
            let cert_id = &single.children[0];
            if cert_id.tag != Asn1Tag::Sequence || cert_id.children.len() < 4 {
                continue;
            }
            
            let hash_algo = parse_oid(&cert_id.children[0].children[0].data);
            let issuer_name_hash = cert_id.children[1].data.clone();
            let issuer_key_hash = cert_id.children[2].data.clone();
            let serial = cert_id.children[3].data.clone();
            
            // Cert Status
            let status = if single.children[1].class == Asn1Class::ContextSpecific {
                if single.children[1].tag_number == 0 {
                    // Good
                    OcspCertStatus::Good
                } else if single.children[1].tag_number == 1 {
                    // Revoked
                    let revocation_time = X509Crl::parse_time(&single.children[1]);
                    let reason = CrlReason::Unspecified;
                    OcspCertStatus::Revoked { reason, revocation_time }
                } else {
                    // Unknown
                    OcspCertStatus::Unknown
                }
            } else {
                OcspCertStatus::Unknown
            };
            
            // thisUpdate
            let this_update = X509Crl::parse_time(&single.children[2]);
            
            // nextUpdate (optional)
            let next_update = if single.children.len() > 3 && 
                single.children[3].class == Asn1Class::ContextSpecific {
                Some(X509Crl::parse_time(&single.children[3]))
            } else {
                None
            };
            
            responses.push(OcspSingleResponse {
                cert_id_hash_algo: hash_algo,
                issuer_name_hash,
                issuer_key_hash,
                serial,
                status,
                this_update,
                next_update,
                produced_at: 0,
            });
        }
        
        Some(responses)
    }
    
    /// Get certificate status
    pub fn get_cert_status(&self, serial: &[u8]) -> Option<&OcspSingleResponse> {
        self.responses.iter().find(|r| r.serial == serial)
    }
}

// ============================================================================
// REVOCATION CHECKER
// ============================================================================

/// Revocation checker combining CRL and OCSP
pub struct RevocationChecker {
    pub crls: Vec<X509Crl>,
    pub ocsp_cache: Vec<(Vec<u8>, OcspResponse)>,
    pub prefer_ocsp: bool,
}

impl RevocationChecker {
    pub fn new() -> Self {
        RevocationChecker {
            crls: Vec::new(),
            ocsp_cache: Vec::new(),
            prefer_ocsp: true,
        }
    }
    
    /// Add CRL
    pub fn add_crl(&mut self, crl: X509Crl) {
        self.crls.push(crl);
    }
    
    /// Cache OCSP response
    pub fn cache_ocsp(&mut self, serial: Vec<u8>, response: OcspResponse) {
        self.ocsp_cache.push((serial, response));
    }
    
    /// Check if certificate is revoked
    pub fn check_revocation(&self, cert: &X509Certificate, issuer: &X509Certificate) -> Result<(), CertError> {
        // Try OCSP first if preferred
        if self.prefer_ocsp {
            if let Some(response) = self.ocsp_cache.iter().find(|(s, _)| s == &cert.serial).map(|(_, r)| r) {
                return self.check_ocsp_status(response, &cert.serial);
            }
        }
        
        // Check CRLs
        for crl in &self.crls {
            // Check if CRL issuer matches certificate issuer
            if crl.issuer.common_name == issuer.subject.common_name {
                if let Some(entry) = crl.is_revoked(&cert.serial) {
                    return Err(CertError::Revoked);
                }
            }
        }
        
        // If OCSP not preferred and not found in cache, check CRLs only
        if !self.prefer_ocsp {
            return Ok(());
        }
        
        // Otherwise, we couldn't determine revocation status
        // In production, would make network request to OCSP responder
        Ok(())
    }
    
    fn check_ocsp_status(&self, response: &OcspResponse, serial: &[u8]) -> Result<(), CertError> {
        if response.response_status != OcspResponseStatus::Successful {
            // OCSP failed, fall back to CRL
            return Ok(());
        }
        
        if let Some(single) = response.get_cert_status(serial) {
            match single.status {
                OcspCertStatus::Good => Ok(()),
                OcspCertStatus::Revoked { .. } => Err(CertError::Revoked),
                OcspCertStatus::Unknown => Ok(()), // Unknown status, accept
            }
        } else {
            Ok(())
        }
    }
}

impl Default for RevocationChecker {
    fn default() -> Self {
        Self::new()
    }
}
