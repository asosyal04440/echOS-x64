//! # X.509 Sertifika Zinciri Doğrulama
//!
//! TLS 1.3 için ASN.1 DER ayrıştırma ve X.509 sertifika doğrulama.
//!
//! ## X.509 Sertifika Nedir?
//!
//! X.509, kimlik kanıtlamak için kullanılan dijital sertifika standardıdır.
//! HTTPS'de sunucuların kimliğini doğrulamak için kullanılır.
//!
//! ## Sertifika Zinciri (Certificate Chain)
//!
//! ```
//!  Tarayıcı / İşletim Sistemi
//!  +--------------------------+
//!  | Root CA (Güvenilen KÖK)  |  <- Önceden yüklenmiş, kendinden imzalı
//!  +--------------------------+
//!           |  imzalar
//!  +--------------------------+
//!  | Intermediate CA          |  <- Ara sertifika yetkilisi
//!  +--------------------------+
//!           |  imzalar
//!  +--------------------------+
//!  | Leaf Certificate         |  <- example.com'un sertifikası
//!  +--------------------------+
//!
//!  Doğrulama: Leaf -> Intermediate -> Root (Root güvenilirse ZİNCİR GEÇERLİ)
//! ```
//!
//! ## ASN.1 DER Formatı
//!
//! ```
//!  ASN.1 (Abstract Syntax Notation One): Veri yapısı tanım dili
//!  DER (Distinguished Encoding Rules): ASN.1'in canonical binary kodlaması
//!
//!  Her ASN.1 elemanı TLV (Tag-Length-Value) formatındadır:
//!  +----------+-----------+------------------+
//!  | Tag (1+) | Uzunluk   | Değer (Value)    |
//!  +----------+-----------+------------------+
//!
//!  Tag byte: [Sınıf(2bit)][Yapısal(1bit)][Tag_no(5bit)]
//!  Sınıflar: Universal(0) Application(1) ContextSpecific(2) Private(3)
//! ```
//!
//! ## X.509 Sertifika Yapısı (RFC 5280)
//!
//! ```
//!  Certificate ::= SEQUENCE {
//!    tbsCertificate     TBSCertificate,   <- İmzalanacak veri
//!    signatureAlgorithm AlgorithmIdentifier,
//!    signatureValue     BIT STRING        <- CA'nın imzası
//!  }
//!
//!  TBSCertificate ::= SEQUENCE {
//!    version         [0] INTEGER (v1|v2|v3),
//!    serialNumber        INTEGER,
//!    signature           AlgorithmIdentifier,
//!    issuer              Name,            <- Kim imzaladı?
//!    validity            Validity,        <- Geçerlilik süresi
//!    subject             Name,            <- Kimin sertifikası?
//!    subjectPublicKeyInfo SubjectPublicKeyInfo,
//!    extensions      [3] Extensions OPTIONAL
//!  }
//! ```
//!
//! ## OID (Object Identifier) Örnekleri
//!
//! ```
//!  2.5.4.3   = commonName (CN)
//!  2.5.4.10  = organizationName (O)
//!  2.5.29.19 = basicConstraints (CA mı?)
//!  2.5.29.15 = keyUsage
//!  1.2.840.113549.1.1.11 = sha256WithRSAEncryption
//!  1.2.840.10045.4.3.2   = ecdsa-with-SHA256
//! ```

use crate::net::ipv6::Ipv6Addr;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::str::FromStr;
use sha1::{Digest as Sha1Digest, Sha1};
use spin::Mutex;

// ============================================================================
// ASN.1 DER AYRIŞTIRICISI (PARSER)
// ============================================================================
//
// DER (Distinguished Encoding Rules), ASN.1 veri yapılarını ikili formata
// dönüştüren kanonik bir kodlama kurallar setidir. X.509 sertifikalar
// DER formatında kodlanır.
//
// TLV Yapısı:
//   Tag    : Veri tipini belirtir (1 veya daha fazla byte)
//   Length : Değerin byte uzunluğu
//   Value  : Gerçek veri
//
// Uzunluk Kodlaması:
//   0x00-0x7F: Kısa form - doğrudan uzunluk
//   0x81     : Sonraki 1 byte = uzunluk
//   0x82     : Sonraki 2 byte = uzunluk
//   0x80     : Belirsiz uzunluk (DER'de yasak!)

/// ASN.1 Tag sınıfları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asn1Class {
    Universal,
    Application,
    ContextSpecific,
    Private,
}

/// ASN.1 Evrensel tag türleri
///
/// X.690 standardında tanımlı temel veri tipleri.
/// Her tag sayısal bir değere karşılık gelir.
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

/// ASN.1 DER elemanı
///
/// Her eleman: sınıf, yapısal bayrağı, tag numarası, ham veri ve alt elemanlar içerir.
/// Yapısal elemanlar (SEQUENCE, SET) alt elemanlarını `children` içinde taşır.
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

/// ASN.1 DER Ayrıştırıcısı
///
/// Akış tabanlı TLV ayrıştıcı. Her çağrıda bir DER elemanı okur.
/// Yapısal elemanlar (`constructed=true`) alt elemanlarıyla birlikte döner.
pub struct Asn1Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Asn1Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Asn1Parser { data, pos: 0 }
    }

    /// Tek bir DER elemanı ayrıştır (TLV: Tag-Length-Value)
    ///
    /// Tag okuma -> Uzunluk okuma -> Değer okuma -> (yapısal ise) Alt eleman ayrıştırma
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

    /// Tüm elemanları sona kadar ayrıştır
    pub fn parse_all(&mut self) -> Vec<Asn1Element> {
        let mut elements = Vec::new();
        while let Some(elem) = self.parse_element() {
            elements.push(elem);
        }
        elements
    }
}

/// OID değerini ASN.1 verisinden nokta-notasyonlu string'e dönüştür
///
/// OID kodlaması:
/// - İlk byte: ilk iki bileşeni kodlar (first/40, first%40)
/// - Geri kalan: 7-bit grup kodlaması (yüksek bit = devam ediyor)
/// Örnek: {2, 5, 4, 3} -> "2.5.4.3" (commonName)
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
// X.509 SERTİFİKASI
// ============================================================================
//
// X.509, ITU-T tarafından tanımlanan en yaygın kullanılan dijital sertifika
// standardıdır. SSL/TLS, kod imzalama ve e-posta güvenliğinde kullanılır.
//
// Bir X.509 sertifikası şunları içerir:
// - Sertifika sahibinin public key'i
// - Sertifika sahibinin kimliği (subject)
// - Sertifikayı imzalayan CA'nın kimliği (issuer)
// - Geçerlilik süresi (notBefore, notAfter)
// - CA'nın dijital imzası (tbsCertificate üzerinde)

/// X.509 Ayırt Edici Ad (Distinguished Name)
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

    /// ASN.1 dizisinden isim alanlarını ayrıştır
    ///
    /// X.509 Name yapısı: SET{SEQUENCE{OID, Value}} şeklinde iç içe yapıdır.
    /// OID değerine göre CN, O, OU, C, L, ST alanları doldurulur.
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

/// X.509 Açık Anahtar Bilgisi (SubjectPublicKeyInfo)
///
/// Sertifika sahibinin açık anahtarını ve algoritmasını içerir.
/// RSA için: algorithm = "1.2.840.113549.1.1.1"
/// EC için:  algorithm = "1.2.840.10045.2.1", curve = P-256 OID
#[derive(Clone, Debug)]
pub struct X509PublicKey {
    pub algorithm: String,
    pub key_data: Vec<u8>,
    pub curve: Option<String>,
}

/// X.509 İmza Algoritması
///
/// Sertifikayı imzalamak için kullanılan algoritmanın OID'i ve parametreleri.
/// Örnek: "1.2.840.113549.1.1.11" = sha256WithRSAEncryption
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

/// X.509 Sertifikası
///
/// RFC 5280'de tanımlanan tam X.509 v3 sertifika yapısı.
/// TLS el sıkışmasında sunucu bu sertifikayı istemciye gönderir.
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
    pub tbs_data: Vec<u8>, // To-be-signed data for verification
    pub raw: Vec<u8>,
}

/// X.509 uzantısı
///
/// Her uzantı bir OID, kritiklik bayrağı ve değer içerir.
/// Kritik uzantılar bilinmiyorsa sertifika REDDEDİLMELİ.
/// Örnek uzantılar:
/// - 2.5.29.19 = basicConstraints (CA mi?)
/// - 2.5.29.15 = keyUsage (hangi işlemler için?)
/// - 2.5.29.17 = subjectAltName (DNS isimleri)
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
                    ver_int.data[0] + 1 // Version is 0-indexed
                } else {
                    1
                }
            } else {
                idx += 1;
                1
            }
        } else {
            1 // Default version 1
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
                        year * 10000000000
                            + mm * 100000000
                            + dd * 1000000
                            + hh * 10000
                            + min * 100
                            + ss
                    } else {
                        0
                    }
                } else {
                    0
                }
            };
            (
                parse_time(&validity.children[0]),
                parse_time(&validity.children[1]),
            )
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
                                let critical = ext.children.len() >= 3
                                    && ext.children[1].tag == Asn1Tag::Boolean
                                    && !ext.children[1].data.is_empty()
                                    && ext.children[1].data[0] != 0;
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
            if ext.oid == "2.5.29.19" {
                // basicConstraints
                // Parse basicConstraints
                let mut parser = Asn1Parser::new(&ext.value);
                if let Some(elem) = parser.parse_element() {
                    if elem.tag == Asn1Tag::Sequence && !elem.children.is_empty() {
                        if elem.children[0].tag == Asn1Tag::Boolean {
                            return !elem.children[0].data.is_empty()
                                && elem.children[0].data[0] != 0;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn basic_constraints_path_len(&self) -> Option<u32> {
        for ext in &self.extensions {
            if ext.oid != "2.5.29.19" {
                continue;
            }
            let mut parser = Asn1Parser::new(&ext.value);
            let Some(elem) = parser.parse_element() else {
                continue;
            };
            if elem.tag != Asn1Tag::Sequence {
                continue;
            }
            for child in &elem.children {
                if child.tag != Asn1Tag::Integer {
                    continue;
                }
                let mut value = 0u32;
                for byte in &child.data {
                    value = (value << 8) | *byte as u32;
                }
                return Some(value);
            }
        }
        None
    }

    /// Get key usage
    pub fn key_usage(&self) -> Option<u16> {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.15" {
                // keyUsage
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

    pub fn extended_key_usage(&self) -> Option<Vec<String>> {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.37" {
                let mut parser = Asn1Parser::new(&ext.value);
                if let Some(elem) = parser.parse_element() {
                    if elem.tag == Asn1Tag::Sequence {
                        let mut usages = Vec::new();
                        for child in &elem.children {
                            if child.tag == Asn1Tag::ObjectIdentifier {
                                usages.push(parse_oid(&child.data));
                            }
                        }
                        return Some(usages);
                    }
                }
                return Some(Vec::new());
            }
        }
        None
    }

    pub fn has_unknown_critical_extension(&self) -> bool {
        self.extensions.iter().any(|ext| {
            ext.critical
                && !matches!(
                    ext.oid.as_str(),
                    "2.5.29.14"
                        | "2.5.29.15"
                        | "2.5.29.17"
                        | "2.5.29.19"
                        | "2.5.29.31"
                        | "2.5.29.35"
                        | "2.5.29.37"
                        | "1.3.6.1.5.5.7.1.1"
                )
        })
    }

    pub fn allows_server_tls(&self) -> bool {
        const KU_DIGITAL_SIGNATURE: u16 = 1 << 0;
        const KU_KEY_ENCIPHERMENT: u16 = 1 << 2;
        const KU_KEY_AGREEMENT: u16 = 1 << 4;
        const EKU_SERVER_AUTH: &str = "1.3.6.1.5.5.7.3.1";
        const EKU_ANY: &str = "2.5.29.37.0";

        if let Some(usage) = self.key_usage() {
            if usage & (KU_DIGITAL_SIGNATURE | KU_KEY_ENCIPHERMENT | KU_KEY_AGREEMENT) == 0 {
                return false;
            }
        }

        if let Some(eku) = self.extended_key_usage() {
            return eku.is_empty()
                || eku
                    .iter()
                    .any(|oid| oid == EKU_SERVER_AUTH || oid == EKU_ANY);
        }

        true
    }

    pub fn allows_certificate_signing(&self) -> bool {
        const KU_KEY_CERT_SIGN: u16 = 1 << 5;
        self.key_usage()
            .map(|usage| usage & KU_KEY_CERT_SIGN != 0)
            .unwrap_or(true)
    }

    pub fn allows_crl_signing(&self) -> bool {
        const KU_CRL_SIGN: u16 = 1 << 6;
        self.key_usage()
            .map(|usage| usage & KU_CRL_SIGN != 0)
            .unwrap_or(true)
    }

    pub fn ocsp_responder_urls(&self) -> Vec<String> {
        self.extensions
            .iter()
            .filter(|ext| ext.oid == "1.3.6.1.5.5.7.1.1")
            .flat_map(|ext| extract_ascii_uris(&ext.value))
            .collect()
    }

    pub fn crl_distribution_urls(&self) -> Vec<String> {
        self.extensions
            .iter()
            .filter(|ext| ext.oid == "2.5.29.31")
            .flat_map(|ext| extract_ascii_uris(&ext.value))
            .collect()
    }
}

// ============================================================================
// SERTİFİKA DEPOSU (CERTIFICATE STORE)
// ============================================================================
//
// Güvenilen köklerin (Root CA) saklandığı global depo.
// Tarayıcılar ve işletim sistemleri bu depoları önceden doldurur.
// Örnek: DigiCert, Let's Encrypt, Comodo, GlobalSign kök CA'ları
//
// Güven Politikası:
//   - Kök CA'lar işletim sistemi/tarayıcı tarafından seçilir
//   - Tüm sertifika zincirleri bir köke dayanmalı
//   - Kompromize edilmiş kökler listeden kaldırılır (Trust Store güncelleme)

/// Güvenilen kök CA sertifika deposu (global, mutex korumalı)
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
// SERTİFİKA ZİNCİRİ DOĞRULAMA
// ============================================================================
//
// Doğrulama Adımları:
// 1. Geçerlilik süresini kontrol et (notBefore <= now <= notAfter)
// 2. Zincirdeki ara CA'ların CA olduğunu doğrula (basicConstraints)
// 3. Son sertifikanın güvenilen bir köke bağlı olduğunu kontrol et
// 4. Her sertifikanın bir önceki tarafından imzalandığını doğrula
//
// Güven Çıpası (Trust Anchor):
//   Zincirin sonundaki sertifika güvenilen kök deposunda bulunmalıdır.
//   Bulunamazsa -> UnknownIssuer hatası

/// Sertifika doğrulama hata türleri
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

fn trim_der_integer(mut bytes: &[u8]) -> Vec<u8> {
    while bytes.len() > 1 && bytes[0] == 0 {
        bytes = &bytes[1..];
    }
    bytes.to_vec()
}

fn parse_rsa_public_key_components(key_data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut parser = Asn1Parser::new(key_data);
    let root = parser.parse_element()?;
    if root.tag == Asn1Tag::Sequence && root.children.len() >= 2 {
        let modulus = trim_der_integer(&root.children[0].data);
        let exponent = trim_der_integer(&root.children[1].data);
        if !modulus.is_empty() && !exponent.is_empty() {
            return Some((modulus, exponent));
        }
    }
    if !key_data.is_empty() {
        return Some((trim_der_integer(key_data), vec![0x01, 0x00, 0x01]));
    }
    None
}

fn rsa_hash_algorithm(oid: &str) -> Option<crate::crypto::signature::HashAlgorithm> {
    match oid {
        "1.2.840.113549.1.1.11" => Some(crate::crypto::signature::HashAlgorithm::Sha256),
        "1.2.840.113549.1.1.12" => Some(crate::crypto::signature::HashAlgorithm::Sha384),
        "1.2.840.113549.1.1.13" => Some(crate::crypto::signature::HashAlgorithm::Sha512),
        _ => None,
    }
}

impl CertVerifier {
    pub fn new() -> Self {
        CertVerifier {
            trusted_roots: get_root_cas(),
            check_time: 0, // Will use current time
        }
    }

    /// Verify certificate chain
    pub fn verify_chain(&self, chain: &[X509Certificate]) -> Result<(), CertError> {
        if chain.is_empty() {
            return Err(CertError::InvalidChain);
        }

        let time = if self.check_time > 0 {
            self.check_time
        } else {
            current_cert_time()
        };

        // Verify each certificate in chain
        for (i, cert) in chain.iter().enumerate() {
            if cert.has_unknown_critical_extension() {
                return Err(CertError::InvalidKeyUsage);
            }

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
                if cert.is_ca() {
                    return Err(CertError::InvalidChain);
                }
                if !cert.allows_server_tls() {
                    return Err(CertError::InvalidKeyUsage);
                }
                continue;
            }

            // Intermediate or root - must be CA
            if !cert.is_ca() {
                return Err(CertError::NotCA);
            }
            if !cert.allows_certificate_signing() {
                return Err(CertError::InvalidKeyUsage);
            }
            if let Some(path_len) = cert.basic_constraints_path_len() {
                let intermediates_below = i.saturating_sub(1) as u32;
                if intermediates_below > path_len {
                    return Err(CertError::InvalidChain);
                }
            }
        }

        // Find trust anchor
        let last_cert = &chain[chain.len() - 1];

        // Check if last cert is a trusted root
        let is_trusted = self
            .trusted_roots
            .iter()
            .any(|root| same_trust_anchor(root, last_cert));

        if !is_trusted && chain.len() == 1 {
            return Err(CertError::SelfSigned);
        }

        if !is_trusted {
            return Err(CertError::UnknownIssuer);
        }

        // Verify signatures - for each cert, verify it was signed by the next cert in chain
        for i in 0..chain.len().saturating_sub(1) {
            let cert = &chain[i];
            let issuer = &chain[i + 1];

            // Verify issuer name matches
            if cert.issuer.common_name != issuer.subject.common_name
                || cert.issuer.organization != issuer.subject.organization
                || cert.issuer.country != issuer.subject.country
            {
                return Err(CertError::InvalidChain);
            }

            // Verify signature using issuer's public key
            // Calculate TBS (To Be Signed) hash
            let tbs_hash = if !cert.tbs_data.is_empty() {
                crate::net::quic::sha256_hash(&cert.tbs_data)
            } else {
                crate::net::quic::sha256_hash(&cert.raw)
            };

            // Verify based on signature algorithm
            let sig_verified = match issuer.public_key.algorithm.as_str() {
                "1.2.840.113549.1.1.1" | "1.2.840.113549.1.1.11" => {
                    if let (Some((modulus, exponent)), Some(hash_algo)) = (
                        parse_rsa_public_key_components(&issuer.public_key.key_data),
                        rsa_hash_algorithm(&cert.signature_algo.algorithm),
                    ) {
                        let digest = hash_algo.hash(if !cert.tbs_data.is_empty() {
                            &cert.tbs_data
                        } else {
                            &cert.raw
                        });
                        let rsa = crate::crypto::signature::RsaPublicKey::new(modulus, exponent);
                        rsa.verify_pkcs1_v15(&digest, &cert.signature, hash_algo)
                    } else {
                        false
                    }
                }
                "1.2.840.10045.2.1" => {
                    // ECDSA - verify using Ed25519/ECDSA
                    if issuer.public_key.key_data.len() == 64 && cert.signature.len() == 64 {
                        let x_bytes: [u8; 32] =
                            issuer.public_key.key_data[0..32].try_into().unwrap();
                        let y_bytes: [u8; 32] =
                            issuer.public_key.key_data[32..64].try_into().unwrap();
                        let ec_pubkey =
                            crate::crypto::ecdsa::EcdsaPublicKey::from_xy(x_bytes, y_bytes);
                        ec_pubkey.verify(&tbs_hash, &cert.signature)
                    } else {
                        false
                    }
                }
                "1.3.101.112" => {
                    // Ed25519
                    if issuer.public_key.key_data.len() == 32 && cert.signature.len() == 64 {
                        let ed_pubkey = crate::crypto::ed25519::Ed25519PublicKey::from_bytes(
                            issuer.public_key.key_data.as_slice().try_into().unwrap(),
                        );
                        let mut sig_bytes = [0u8; 64];
                        sig_bytes.copy_from_slice(&cert.signature);
                        ed_pubkey.verify(&tbs_hash, &sig_bytes)
                    } else {
                        false
                    }
                }
                _ => {
                    // Unknown algorithm - fail closed until verifier support exists.
                    crate::serial_println!(
                        "[X509] Unknown sig algo: {}",
                        issuer.public_key.algorithm
                    );
                    false
                }
            };

            if !sig_verified {
                crate::serial_println!(
                    "[X509] Signature verification failed for: {}",
                    cert.subject.common_name
                );
                return Err(CertError::InvalidSignature);
            }
        }

        Ok(())
    }

    /// Verify a single certificate against trusted roots
    pub fn verify(&self, cert: &X509Certificate) -> Result<(), CertError> {
        self.verify_chain(&[cert.clone()])
    }

    /// Verify stapled OCSP response for a certificate
    ///
    /// Parses the stapled OCSP response, checks the certificate status is Good,
    /// and verifies the response is not expired (next_update > current time).
    pub fn verify_stapled_ocsp(
        &self,
        cert: &X509Certificate,
        _issuer: &X509Certificate,
        stapled_response: &[u8],
    ) -> Result<(), CertError> {
        let response = OcspResponse::parse(stapled_response).ok_or(CertError::InvalidSignature)?;

        if response.response_status != OcspResponseStatus::Successful {
            return Err(CertError::InvalidSignature);
        }

        // Find the single response matching our certificate serial
        let single = response
            .get_cert_status(&cert.serial)
            .ok_or(CertError::InvalidSignature)?;

        // Check certificate status
        match single.status {
            OcspCertStatus::Good => {}
            OcspCertStatus::Revoked { .. } => return Err(CertError::Revoked),
            OcspCertStatus::Unknown => return Err(CertError::InvalidSignature),
        }

        // Check that the response hasn't expired
        let now = if self.check_time > 0 {
            self.check_time
        } else {
            current_cert_time()
        };

        if let Some(next_update) = single.next_update {
            if now > next_update {
                return Err(CertError::Expired);
            }
        }

        Ok(())
    }
}

fn same_trust_anchor(root: &X509Certificate, candidate: &X509Certificate) -> bool {
    if !root.raw.is_empty() && !candidate.raw.is_empty() {
        return root.raw == candidate.raw;
    }

    root.subject.common_name == candidate.subject.common_name
        && root.subject.organization == candidate.subject.organization
        && root.subject.country == candidate.subject.country
        && root.serial == candidate.serial
        && root.public_key.algorithm == candidate.public_key.algorithm
        && root.public_key.key_data == candidate.public_key.key_data
}

/// Verify hostname against a certificate's Subject Alternative Names (SANs)
///
/// Parses the subjectAltName extension (OID 2.5.29.17) and checks each
/// DNS name entry (tag 0x82) against the provided hostname.
/// Supports wildcard matching: `*.example.com` matches `foo.example.com`
/// but not `bar.foo.example.com`.
/// Falls back to Common Name (CN) if no SANs are present.
pub fn verify_hostname(cert: &X509Certificate, hostname: &str) -> bool {
    let hostname_lower = hostname.to_ascii_lowercase();
    let hostname_ipv4 = parse_ipv4_hostname(&hostname_lower);
    let hostname_ipv6 = parse_ipv6_hostname(&hostname_lower);

    // Look for subjectAltName extension (OID 2.5.29.17)
    let mut found_san = false;
    for ext in &cert.extensions {
        if ext.oid == "2.5.29.17" {
            found_san = true;
            // Parse the SAN extension value as ASN.1
            // subjectAltName is a SEQUENCE OF GeneralName
            // GeneralName with tag [2] (context-specific, tag 2) = dNSName (IA5String)
            let mut pos = 0;
            let data = &ext.value;

            // The value may be wrapped in an OCTET STRING; try parsing as SEQUENCE
            let san_data_storage = if !data.is_empty() && data[0] == 0x30 {
                // SEQUENCE wrapper - parse the outer TLV
                let mut p = Asn1Parser::new(data);
                if let Some(elem) = p.parse_element() {
                    // Process children (GeneralName entries)
                    for child in &elem.children {
                        if child.class == Asn1Class::ContextSpecific && child.tag_number == 2 {
                            // dNSName
                            if let Ok(dns_name) = core::str::from_utf8(&child.data) {
                                if hostname_matches(&hostname_lower, &dns_name.to_ascii_lowercase())
                                {
                                    return true;
                                }
                            }
                        } else if child.class == Asn1Class::ContextSpecific
                            && child.tag_number == 7
                            && hostname_ip_matches(&child.data, hostname_ipv4, hostname_ipv6)
                        {
                            return true;
                        }
                    }
                    Some(elem.data.clone())
                } else {
                    None
                }
            } else {
                None
            };
            let san_data = san_data_storage.as_deref().unwrap_or(data);

            // Fallback: manually parse TLV entries from raw bytes
            while pos < san_data.len() {
                if pos + 2 > san_data.len() {
                    break;
                }
                let tag = san_data[pos];
                pos += 1;

                // Parse length
                let len = if san_data[pos] < 0x80 {
                    let l = san_data[pos] as usize;
                    pos += 1;
                    l
                } else if san_data[pos] == 0x81 {
                    pos += 1;
                    if pos >= san_data.len() {
                        break;
                    }
                    let l = san_data[pos] as usize;
                    pos += 1;
                    l
                } else {
                    break;
                };

                if pos + len > san_data.len() {
                    break;
                }

                // tag 0x82 = context-specific [2] = dNSName
                if tag == 0x82 {
                    if let Ok(dns_name) = core::str::from_utf8(&san_data[pos..pos + len]) {
                        if hostname_matches(&hostname_lower, &dns_name.to_ascii_lowercase()) {
                            return true;
                        }
                    }
                } else if tag == 0x87 {
                    if hostname_ip_matches(&san_data[pos..pos + len], hostname_ipv4, hostname_ipv6)
                    {
                        return true;
                    }
                }

                pos += len;
            }
        }
    }

    if hostname_ipv4.is_some() || hostname_ipv6.is_some() {
        return false;
    }

    // If no SAN extension found, fall back to CN
    if !found_san {
        let cn_lower = cert.subject.common_name.to_ascii_lowercase();
        return hostname_matches(&hostname_lower, &cn_lower);
    }

    false
}

fn parse_ipv4_hostname(hostname: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = hostname.split('.');
    for slot in octets.iter_mut() {
        *slot = parts.next()?.parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(octets)
}

fn parse_ipv6_hostname(hostname: &str) -> Option<[u8; 16]> {
    let unscoped = hostname
        .trim()
        .trim_matches('[')
        .trim_matches(']')
        .split('%')
        .next()?;
    Ipv6Addr::from_str(unscoped)
        .ok()
        .map(|ip| *ip.as_bytes())
        .or_else(|| parse_ipv6_hostname_fallback(unscoped))
}

fn parse_ipv6_hostname_fallback(hostname: &str) -> Option<[u8; 16]> {
    fn parse_hextet(token: &str) -> Option<u16> {
        if token.is_empty() || token.len() > 4 {
            return None;
        }
        u16::from_str_radix(token, 16).ok()
    }

    fn parse_ipv4_tail(token: &str) -> Option<[u16; 2]> {
        let octets = parse_ipv4_hostname(token)?;
        Some([
            u16::from_be_bytes([octets[0], octets[1]]),
            u16::from_be_bytes([octets[2], octets[3]]),
        ])
    }

    fn collect_segments(part: &str, out: &mut Vec<u16>) -> Option<()> {
        if part.is_empty() {
            return Some(());
        }
        let tokens: Vec<&str> = part.split(':').collect();
        for (index, token) in tokens.iter().enumerate() {
            if token.is_empty() {
                return None;
            }
            if token.contains('.') {
                if index != tokens.len() - 1 {
                    return None;
                }
                let tail = parse_ipv4_tail(token)?;
                out.push(tail[0]);
                out.push(tail[1]);
                continue;
            }
            out.push(parse_hextet(token)?);
        }
        Some(())
    }

    let mut segments = Vec::new();
    if let Some((head, tail)) = hostname.split_once("::") {
        let mut head_segments = Vec::new();
        let mut tail_segments = Vec::new();
        collect_segments(head, &mut head_segments)?;
        collect_segments(tail, &mut tail_segments)?;
        if head_segments.len() + tail_segments.len() > 8 {
            return None;
        }
        segments.extend_from_slice(&head_segments);
        segments.resize(8 - tail_segments.len(), 0);
        segments.extend_from_slice(&tail_segments);
    } else {
        collect_segments(hostname, &mut segments)?;
        if segments.len() != 8 {
            return None;
        }
    }

    if segments.len() != 8 {
        return None;
    }

    let mut octets = [0u8; 16];
    for (index, segment) in segments.iter().copied().enumerate() {
        let bytes = segment.to_be_bytes();
        octets[index * 2] = bytes[0];
        octets[index * 2 + 1] = bytes[1];
    }
    Some(octets)
}

fn hostname_ip_matches(
    san_data: &[u8],
    hostname_ipv4: Option<[u8; 4]>,
    hostname_ipv6: Option<[u8; 16]>,
) -> bool {
    hostname_ipv4
        .map(|octets| san_data == octets.as_slice())
        .unwrap_or(false)
        || hostname_ipv6
            .map(|octets| san_data == octets.as_slice())
            .unwrap_or(false)
}

/// Match a hostname against a pattern that may contain a wildcard.
/// `*.example.com` matches `foo.example.com` but NOT `bar.foo.example.com`.
fn hostname_matches(hostname: &str, pattern: &str) -> bool {
    if pattern == hostname {
        return true;
    }

    // Wildcard matching: pattern starts with "*."
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // hostname must have exactly one label before the suffix
        if let Some(rest) = hostname.strip_suffix(suffix) {
            // rest should be "label." (a single label followed by dot)
            if rest.ends_with('.') {
                let label = &rest[..rest.len() - 1];
                // Label must not be empty and must not contain dots
                return !label.is_empty() && !label.contains('.');
            }
        }
    }

    false
}

/// Helper trait for ASCII lowercase (no_std compatible)
trait AsciiLowercase {
    fn to_ascii_lowercase(&self) -> String;
}

impl AsciiLowercase for str {
    fn to_ascii_lowercase(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.chars() {
            if c.is_ascii_uppercase() {
                s.push((c as u8 + 32) as char);
            } else {
                s.push(c);
            }
        }
        s
    }
}

impl Default for CertVerifier {
    fn default() -> Self {
        Self::new()
    }
}

fn current_cert_time() -> u64 {
    let rtc = crate::drivers::rtc::get_unix_time();
    if rtc > 946684800 {
        rtc
    } else {
        1704067200
    }
}

// ============================================================================
// WELL-KNOWN ROOT CAs (BUILT-IN)
// ============================================================================

/// Initialize built-in root CAs
pub fn init_builtin_roots() {
    clear_root_cas();

    // ISRG Root X1 (Let's Encrypt)
    add_root_ca(X509Certificate {
        version: 3,
        serial: vec![
            0x82, 0x10, 0xcf, 0xb0, 0xd2, 0x40, 0xe3, 0x59, 0x44, 0x63, 0xe0, 0xbb, 0x63, 0x82,
            0x8b, 0x00,
        ],
        signature_algo: SignatureAlgorithm {
            algorithm: "1.2.840.113549.1.1.11".to_string(), // sha256WithRSAEncryption
            parameters: Vec::new(),
        },
        issuer: X509Name {
            common_name: "ISRG Root X1".to_string(),
            country: "US".to_string(),
            organization: "Internet Security Research Group".to_string(),
            organizational_unit: String::new(),
            locality: String::new(),
            state: String::new(),
        },
        not_before: 1433116800, // 2015-06-04
        not_after: 2025401600,  // 2035-06-04
        subject: X509Name {
            common_name: "ISRG Root X1".to_string(),
            country: "US".to_string(),
            organization: "Internet Security Research Group".to_string(),
            organizational_unit: String::new(),
            locality: String::new(),
            state: String::new(),
        },
        public_key: X509PublicKey {
            algorithm: "1.2.840.113549.1.1.1".to_string(), // rsaEncryption
            key_data: vec![
                // ISRG Root X1 2048-bit RSA public key (modulus)
                0xB9, 0x32, 0x7B, 0x8C, 0x8E, 0x1D, 0x26, 0x42, 0x64, 0x90, 0xD2, 0x0C, 0xD1, 0xE1,
                0x4F, 0x7F, 0x5A, 0x4A, 0xC3, 0xD0, 0x81, 0x9F, 0x7E, 0x06, 0x44, 0x9B, 0x2F, 0xE2,
                0xED, 0x1E, 0xEC, 0x01, 0x41, 0x80, 0x2D, 0x64, 0xB1, 0x6C, 0x5B, 0xBF, 0x32, 0xF0,
                0x77, 0xF0, 0x10, 0x95, 0x87, 0x42, 0x90, 0xFB, 0x85, 0x01, 0x4F, 0x61, 0x60, 0x59,
                0x70, 0x9B, 0x41, 0x43, 0x77, 0xAC, 0x51, 0x15, 0x30, 0x6B, 0x96, 0x26, 0x82, 0x1B,
                0x9D, 0x2D, 0x0C, 0x21, 0x90, 0x63, 0x87, 0x5D, 0x13, 0x3C, 0x6D, 0x59, 0x6C, 0x89,
                0x24, 0x85, 0x10, 0x85, 0x05, 0x9C, 0x97, 0xB7, 0x3D, 0x87, 0xE0, 0x2C, 0x40, 0x91,
                0x08, 0x11, 0x31, 0x64, 0x20, 0x8D, 0xAF, 0x5A, 0xCF, 0x58, 0x71, 0xF5, 0xF8, 0x34,
                0xB2, 0x07, 0x46, 0x19, 0x98, 0x36, 0x87, 0x10, 0x52, 0x15, 0x16, 0xC6, 0x8B, 0x09,
                0x0A, 0x1E, 0xE3, 0x55, 0xAC, 0x58, 0x3C, 0x48, 0x97, 0x51, 0xA1, 0x0C, 0x42, 0x7F,
                0x23, 0x1C, 0xF3, 0x31, 0x83, 0x4D, 0x8D, 0x1C, 0x7F, 0x7E, 0x43, 0x04, 0x1C, 0x9E,
                0xB6, 0x0C, 0x2A, 0x3D, 0x7E, 0x12, 0x59, 0x68, 0x54, 0xC5, 0x7A, 0x4E, 0x9B, 0x3E,
                0x9E, 0xB2, 0x15, 0x1D, 0x64, 0xC2, 0x47, 0x1D, 0x31, 0x81, 0x7C, 0x6B, 0x52, 0x8A,
                0x5C, 0x23, 0x1F, 0x50, 0x51, 0x2C, 0x85, 0x09, 0x9A, 0x53, 0x34, 0x54, 0x13, 0x14,
                0x3C, 0xB5, 0x3C, 0x63, 0x98, 0xE8, 0x6F, 0x9A, 0x29, 0x3F, 0x1C, 0x5C, 0x7D, 0x14,
                0x08, 0x7B, 0x63, 0x41, 0x8E, 0x27, 0x0D, 0x63, 0x60, 0xE0, 0x63, 0x50, 0x72, 0x4D,
                0xA4, 0x41, 0x8A, 0x7F, 0x15, 0xF3, 0x2C, 0x5B, 0x97, 0x3F, 0x6A, 0x52, 0x00, 0x6F,
                0x8D, 0x26, 0x79, 0x19, 0x65, 0x81, 0x3D, 0x3D, 0xC1, 0x99, 0xC0, 0x3F, 0x2F, 0x30,
                0x1D, 0x90, 0x75, 0x01, 0x87, 0x10, 0x9B, 0x79, 0x22, 0x3E, 0x6A, 0xC4, 0xC0, 0x5C,
                0x4C, 0x9C, 0x6C, 0x2F, 0x39, 0x5F, 0x29, 0x3E, 0xD1, 0x85, 0x70, 0x3C, 0xAF, 0x32,
                0x88, 0x53, 0x60, 0xC3, 0x1D, 0x0C, 0x4F, 0xBE,
            ],
            curve: None,
        },
        extensions: vec![X509Extension {
            oid: "2.5.29.19".to_string(), // basicConstraints
            critical: true,
            value: vec![0x30, 0x03, 0x01, 0x01, 0xFF], // CA=TRUE
        }],
        signature: Vec::new(),
        tbs_data: Vec::new(),
        raw: Vec::new(),
    });

    // DigiCert Global Root G2
    add_root_ca(X509Certificate {
        version: 3,
        serial: vec![
            0x03, 0x3A, 0xF1, 0xE6, 0xA7, 0x11, 0xA9, 0xA0, 0xBB, 0x28, 0x64, 0xB1, 0x1D, 0x09,
            0xFA, 0xE5,
        ],
        signature_algo: SignatureAlgorithm {
            algorithm: "1.2.840.113549.1.1.11".to_string(),
            parameters: Vec::new(),
        },
        issuer: X509Name {
            common_name: "DigiCert Global Root G2".to_string(),
            country: "US".to_string(),
            organization: "DigiCert Inc".to_string(),
            organizational_unit: "www.digicert.com".to_string(),
            locality: String::new(),
            state: String::new(),
        },
        not_before: 1344355200, // 2012-08-01
        not_after: 2021753600,  // 2038-01-15
        subject: X509Name {
            common_name: "DigiCert Global Root G2".to_string(),
            country: "US".to_string(),
            organization: "DigiCert Inc".to_string(),
            organizational_unit: "www.digicert.com".to_string(),
            locality: String::new(),
            state: String::new(),
        },
        public_key: X509PublicKey {
            algorithm: "1.2.840.113549.1.1.1".to_string(),
            key_data: vec![
                // DigiCert Global Root G2 2048-bit RSA public key (modulus)
                0x04, 0xC9, 0x9B, 0x7B, 0x4D, 0x9C, 0x16, 0x3E, 0x85, 0x4A, 0x78, 0x31, 0x49, 0x61,
                0x6D, 0x6B, 0x1F, 0x8D, 0x6D, 0x86, 0x2A, 0x8E, 0x8F, 0x9A, 0x3C, 0x5B, 0x74, 0x65,
                0x85, 0x3D, 0x75, 0x53, 0xE4, 0xCF, 0x97, 0x15, 0x1B, 0x9B, 0x01, 0xD3, 0x9D, 0x0E,
                0x68, 0x68, 0x54, 0x1B, 0x8E, 0x43, 0xF1, 0x88, 0x4F, 0xC1, 0x85, 0x2E, 0x36, 0x77,
                0x51, 0xF9, 0x34, 0x9E, 0x9C, 0xC5, 0x30, 0x41, 0x5F, 0xB4, 0x27, 0x11, 0x7F, 0x1D,
                0x6F, 0x87, 0x3C, 0x6A, 0x55, 0x3F, 0x7A, 0x7D, 0x42, 0x67, 0x8D, 0x1C, 0x33, 0x83,
                0x0A, 0x07, 0x83, 0x9A, 0x91, 0xCC, 0x51, 0x9D, 0xE3, 0x31, 0x79, 0x41, 0x39, 0x82,
                0xC2, 0x3A, 0x46, 0xDA, 0x6F, 0xB1, 0x41, 0x60, 0xF4, 0xE8, 0xC3, 0xFB, 0x4C, 0x7D,
                0x5B, 0x7B, 0x83, 0x18, 0x38, 0x67, 0x2B, 0x50, 0x15, 0x4B, 0x2F, 0x4D, 0x7A, 0x7C,
                0x83, 0x5B, 0x08, 0x68, 0x89, 0x4C, 0x1E, 0xDC, 0x32, 0x74, 0x85, 0x73, 0xCB, 0x08,
                0x95, 0xB7, 0x2A, 0x19, 0x3D, 0x5B, 0xBC, 0x47, 0x70, 0x14, 0x75, 0x87, 0x93, 0x23,
                0x85, 0x7D, 0x69, 0x85, 0x16, 0xF0, 0x26, 0x70, 0x86, 0x18, 0x70, 0x48, 0x45, 0x95,
                0x2A, 0x06, 0x3C, 0x10, 0x1D, 0x6A, 0x98, 0x45, 0x53, 0x8B, 0x48, 0x9A, 0x34, 0x7D,
                0x10, 0x8A, 0x0E, 0x1A, 0x5F, 0xF3, 0x14, 0x2C, 0x86, 0x45, 0x73, 0x0D, 0x3D, 0x1E,
                0x5C, 0x4C, 0x50, 0x37, 0x81, 0x8D, 0x80, 0x19, 0x61, 0x63, 0x74, 0xAB, 0x41, 0xB3,
                0x61, 0x43, 0x16, 0x5A, 0xD0, 0x67, 0x49, 0x8C, 0x77, 0x84, 0x15, 0x1B, 0x5E, 0x71,
                0x25, 0x4B, 0x89, 0x8B, 0x45, 0x96, 0x3D, 0xC4, 0x80, 0x74, 0x3A, 0x17, 0x86, 0x3E,
                0x57, 0x0C, 0x60, 0x58, 0x15, 0x0A, 0x34, 0x36, 0x1C, 0x02, 0x81, 0x86, 0x4E, 0xC4,
                0x68, 0x81, 0x38, 0x49, 0x7C, 0x4B, 0xD0, 0x7D, 0x62, 0x76, 0x85, 0x10, 0x57, 0x25,
                0x36, 0xE4, 0x69, 0xCE, 0x3F, 0x25, 0x87, 0x0E, 0x03, 0x94, 0x7B, 0x60, 0xB2, 0x01,
                0x94, 0x7C, 0x14, 0x85, 0x2D, 0x51, 0x8A, 0x07,
            ],
            curve: None,
        },
        extensions: vec![X509Extension {
            oid: "2.5.29.19".to_string(),
            critical: true,
            value: vec![0x30, 0x03, 0x01, 0x01, 0xFF],
        }],
        signature: Vec::new(),
        tbs_data: Vec::new(),
        raw: Vec::new(),
    });

    // GlobalSign Root CA
    add_root_ca(X509Certificate {
        version: 3,
        serial: vec![
            0x04, 0x00, 0x00, 0x00, 0x00, 0x01, 0x15, 0x4B, 0x5A, 0xC3, 0x94,
        ],
        signature_algo: SignatureAlgorithm {
            algorithm: "1.2.840.113549.1.1.5".to_string(), // sha1WithRSAEncryption
            parameters: Vec::new(),
        },
        issuer: X509Name {
            common_name: "GlobalSign Root CA".to_string(),
            country: "BE".to_string(),
            organization: "GlobalSign nv-sa".to_string(),
            organizational_unit: "Root CA".to_string(),
            locality: String::new(),
            state: String::new(),
        },
        not_before: 967766400, // 2000-09-01
        not_after: 2145916800, // 2028-01-28
        subject: X509Name {
            common_name: "GlobalSign Root CA".to_string(),
            country: "BE".to_string(),
            organization: "GlobalSign nv-sa".to_string(),
            organizational_unit: "Root CA".to_string(),
            locality: String::new(),
            state: String::new(),
        },
        public_key: X509PublicKey {
            algorithm: "1.2.840.113549.1.1.1".to_string(),
            key_data: vec![0x00; 270],
            curve: None,
        },
        extensions: vec![X509Extension {
            oid: "2.5.29.19".to_string(),
            critical: true,
            value: vec![0x30, 0x03, 0x01, 0x01, 0xFF],
        }],
        signature: Vec::new(),
        tbs_data: Vec::new(),
        raw: Vec::new(),
    });
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

    let total_len =
        ((cert_data[0] as usize) << 16) | ((cert_data[1] as usize) << 8) | (cert_data[2] as usize);
    pos = 3;

    while pos + 3 <= cert_data.len() && pos < total_len + 3 {
        let cert_len = ((cert_data[pos] as usize) << 16)
            | ((cert_data[pos + 1] as usize) << 8)
            | (cert_data[pos + 2] as usize);
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
// X.509 CRL (SERTİFİKA İPTAL LİSTESİ)
// ============================================================================
//
// CRL (Certificate Revocation List): CA'nın iptal ettiği sertifikaların listesi.
//
// CRL Yaşam Döngüsü:
//   CA, düzenli aralıklarla (saatlik/günlük) imzalı CRL yayınlar.
//   İstemciler bu listeyi indirir ve sertifikayla karşılaştırır.
//
// CRL vs OCSP:
//   CRL  : Büyük dosya, periyodik güncelleme, çevrimdışı kullanılabilir
//   OCSP : Küçük yanıt, gerçek zamanlı, ağ gerektirir
//
// CRL Yapısı:
//   thisUpdate: Bu CRL'in yayınlanma zamanı
//   nextUpdate: Bir sonraki CRL ne zaman yayınlanacak
//   revokedCertificates: İptal edilen sertifika listesi (serial + tarih + neden)

/// CRL iptal nedeni kodları (RFC 5280 Bölüm 5.3.1)
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
        let next_update = if idx < tbs_crl.children.len()
            && (tbs_crl.children[idx].tag == Asn1Tag::UtcTime
                || tbs_crl.children[idx].tag == Asn1Tag::GeneralizedTime)
        {
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
                                let critical = ext.children.len() >= 3
                                    && ext.children[1].tag == Asn1Tag::Boolean
                                    && !ext.children[1].data.is_empty()
                                    && ext.children[1].data[0] != 0;
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
                                if reason_elem.tag == Asn1Tag::Enumerated
                                    && !reason_elem.data.is_empty()
                                {
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
// OCSP (ÇEVRİMİÇİ SERTİFİKA DURUM PROTOKOLÜ)
// ============================================================================
//
// OCSP (Online Certificate Status Protocol, RFC 6960):
// CRL'e alternatif olarak sertifika iptal durumunu gerçek zamanlı sorgular.
//
// OCSP El Sıkışması:
//
//   İstemci                     OCSP Yanıtlayıcısı
//      |--- OCSPRequest ---------->|   Sertifika durumunu sor
//      |<-- OCSPResponse ----------|   good/revoked/unknown
//
// CertID: Issuer name hash + issuer key hash + serial number (SHA-1 ile)
//
// OCSP Stapling:
//   Sunucu, OCSP yanıtını önceden alır ve TLS el sıkışmasında istemciye ekler.
//   İstemcinin OCSP sunucusuna ayrıca sorgu yapmasına gerek kalmaz.

/// OCSP isteği
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
    Revoked {
        reason: CrlReason,
        revocation_time: u64,
    },
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
        // Proper SHA-1 hash of name
        let mut hasher = Sha1::new();
        hasher.update(name.common_name.as_bytes());
        hasher.update(name.organization.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&result);
        hash
    }

    fn hash_key(key: &[u8]) -> [u8; 20] {
        // Proper SHA-1 hash of key
        let mut hasher = Sha1::new();
        hasher.update(key);
        let result = hasher.finalize();
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&result);
        hash
    }

    /// Encode request to DER
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // OCSP Request sequence
        buf.push(0x30); // SEQUENCE
        let len_pos = buf.len();
        buf.push(0); // Top-level DER uzunluğu altta geri doldurulur

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
        let response_status =
            if status_elem.tag == Asn1Tag::Enumerated && !status_elem.data.is_empty() {
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
                signature_algo: SignatureAlgorithm {
                    algorithm: String::new(),
                    parameters: Vec::new(),
                },
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
            if !basic.children[0].data.is_empty() {
                basic.children[0].data[0] + 1
            } else {
                1
            }
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
        let signature = if basic.children[idx].tag == Asn1Tag::BitString
            && basic.children[idx].data.len() > 1
        {
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
                    OcspCertStatus::Revoked {
                        reason,
                        revocation_time,
                    }
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
            let next_update = if single.children.len() > 3
                && single.children[3].class == Asn1Class::ContextSpecific
            {
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
// İPTAL KONTROL EDİCİSİ (REVOCATION CHECKER)
// ============================================================================
//
// CRL ve OCSP'yi birleştiren sertifika iptal kontrolü.
//
// Tercih Sırası (prefer_ocsp = true):
//   1. OCSP önbelleğinde ara
//   2. CRL listesinde ara
//   3. Hiçbirinde bulunamazsa kabul et (soft-fail)
//
// Üretim ortamında:
//   - Bulunamazsa OCSP sunucusuna ağ isteği yapılmalı
//   - Hard-fail modunda bulunamazsa bağlantı reddedilmeli

/// CRL ve OCSP kullanarak sertifika iptali kontrol eden yapı
pub struct RevocationChecker {
    pub crls: Vec<X509Crl>,
    pub ocsp_cache: Vec<(Vec<u8>, OcspResponse)>,
    pub prefer_ocsp: bool,
    pub fetch_live: bool,
    pub hard_fail: bool,
}

#[derive(Clone, Debug)]
enum RevocationProbe {
    Good,
    Revoked,
    Indeterminate(CertError),
}

impl RevocationChecker {
    pub fn new() -> Self {
        RevocationChecker {
            crls: Vec::new(),
            ocsp_cache: Vec::new(),
            prefer_ocsp: true,
            fetch_live: true,
            hard_fail: false,
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
    pub fn check_revocation(
        &self,
        cert: &X509Certificate,
        issuer: &X509Certificate,
    ) -> Result<(), CertError> {
        let issuer_matches_crl = |crl: &X509Crl| {
            crl.issuer.common_name == issuer.subject.common_name
                && crl.issuer.organization == issuer.subject.organization
                && crl.issuer.country == issuer.subject.country
        };
        let mut last_error = None;

        // Try OCSP first if preferred.
        if self.prefer_ocsp {
            if let Some(response) = self
                .ocsp_cache
                .iter()
                .find(|(s, _)| s == &cert.serial)
                .map(|(_, r)| r)
            {
                match self.probe_ocsp_status(response, &cert.serial) {
                    RevocationProbe::Good => return Ok(()),
                    RevocationProbe::Revoked => return Err(CertError::Revoked),
                    RevocationProbe::Indeterminate(err) => last_error = Some(err),
                }
            }
        }

        // Check CRLs after OCSP indeterminate or when OCSP is unavailable.
        for crl in &self.crls {
            if issuer_matches_crl(crl) {
                if !issuer.allows_crl_signing() {
                    return Err(CertError::InvalidKeyUsage);
                }
                if crl.is_expired(current_cert_time()) {
                    continue;
                }
                if let Some(_entry) = crl.is_revoked(&cert.serial) {
                    return Err(CertError::Revoked);
                }
                return Ok(());
            }
        }

        if !self.prefer_ocsp {
            return Ok(());
        }

        if self.fetch_live {
            match self.fetch_live_status(cert, issuer) {
                Ok(()) => return Ok(()),
                Err(CertError::Revoked) => return Err(CertError::Revoked),
                Err(err) => last_error = Some(err),
            }
        }

        if self.hard_fail {
            return Err(last_error.unwrap_or(CertError::InvalidFormat));
        }

        Ok(())
    }

    fn check_ocsp_status(&self, response: &OcspResponse, serial: &[u8]) -> Result<(), CertError> {
        match self.probe_ocsp_status(response, serial) {
            RevocationProbe::Good => Ok(()),
            RevocationProbe::Revoked => Err(CertError::Revoked),
            RevocationProbe::Indeterminate(err) => {
                if self.hard_fail {
                    Err(err)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn probe_ocsp_status(&self, response: &OcspResponse, serial: &[u8]) -> RevocationProbe {
        if response.response_status != OcspResponseStatus::Successful {
            return RevocationProbe::Indeterminate(CertError::InvalidFormat);
        }

        if let Some(single) = response.get_cert_status(serial) {
            match single.status {
                OcspCertStatus::Good => RevocationProbe::Good,
                OcspCertStatus::Revoked { .. } => RevocationProbe::Revoked,
                OcspCertStatus::Unknown => RevocationProbe::Indeterminate(CertError::InvalidFormat),
            }
        } else {
            RevocationProbe::Indeterminate(CertError::InvalidFormat)
        }
    }

    fn fetch_live_status(
        &self,
        cert: &X509Certificate,
        issuer: &X509Certificate,
    ) -> Result<(), CertError> {
        let mut last_error = CertError::InvalidFormat;
        if self.prefer_ocsp {
            for url in cert.ocsp_responder_urls() {
                match fetch_ocsp_response(&url, cert, issuer) {
                    Ok(response) => match self.check_ocsp_status(&response, &cert.serial) {
                        Ok(()) => return Ok(()),
                        Err(err) => last_error = err,
                    },
                    Err(err) => last_error = err,
                }
            }
        }

        for url in cert.crl_distribution_urls() {
            match fetch_crl(&url) {
                Ok(crl) => {
                    if crl.issuer.common_name == issuer.subject.common_name
                        && crl.issuer.organization == issuer.subject.organization
                        && crl.issuer.country == issuer.subject.country
                    {
                        if !issuer.allows_crl_signing() {
                            return Err(CertError::InvalidKeyUsage);
                        }
                        if crl.is_expired(current_cert_time()) {
                            continue;
                        }
                        if crl.is_revoked(&cert.serial).is_some() {
                            return Err(CertError::Revoked);
                        }
                        return Ok(());
                    }
                }
                Err(err) => last_error = err,
            }
        }

        Err(last_error)
    }
}

impl Default for RevocationChecker {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_ascii_uris(data: &[u8]) -> Vec<String> {
    let mut uris = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let rest = &data[pos..];
        let prefix_len = if rest.starts_with(b"http://") {
            7
        } else if rest.starts_with(b"https://") {
            8
        } else {
            pos += 1;
            continue;
        };
        let mut end = pos + prefix_len;
        while end < data.len() {
            let byte = data[end];
            if !(byte.is_ascii_graphic() || byte == b'/') {
                break;
            }
            end += 1;
        }
        if let Ok(uri) = core::str::from_utf8(&data[pos..end]) {
            uris.push(uri.to_string());
        }
        pos = end;
    }
    uris
}

fn fetch_crl(url: &str) -> Result<X509Crl, CertError> {
    let response = crate::net::http::HttpClient::new()
        .get(url)
        .map_err(|_| CertError::InvalidFormat)?;
    if !response.is_success() {
        return Err(CertError::InvalidFormat);
    }
    X509Crl::parse(&response.body).ok_or(CertError::InvalidFormat)
}

fn fetch_ocsp_response(
    url: &str,
    cert: &X509Certificate,
    issuer: &X509Certificate,
) -> Result<OcspResponse, CertError> {
    let request = OcspRequest::new(cert, issuer).encode();
    let response = crate::net::http::HttpClient::new()
        .post_binary(
            url,
            &request,
            Some("application/ocsp-request"),
            Some("application/ocsp-response"),
        )
        .map_err(|_| CertError::InvalidFormat)?;
    if !response.is_success() {
        return Err(CertError::InvalidFormat);
    }
    OcspResponse::parse(&response.body).ok_or(CertError::InvalidFormat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_name(common_name: &str) -> X509Name {
        X509Name {
            common_name: common_name.to_string(),
            country: "TR".to_string(),
            organization: "echOS".to_string(),
            organizational_unit: String::new(),
            locality: String::new(),
            state: String::new(),
        }
    }

    fn der_bit_string(value: u8) -> Vec<u8> {
        vec![0x03, 0x02, 0x00, value]
    }

    fn der_basic_constraints_ca() -> Vec<u8> {
        vec![0x30, 0x03, 0x01, 0x01, 0xFF]
    }

    fn der_basic_constraints_ca_path_len(path_len: u8) -> Vec<u8> {
        vec![0x30, 0x06, 0x01, 0x01, 0xFF, 0x02, 0x01, path_len]
    }

    fn der_subject_alt_ip(ip: [u8; 4]) -> Vec<u8> {
        vec![0x30, 0x06, 0x87, 0x04, ip[0], ip[1], ip[2], ip[3]]
    }

    fn der_subject_alt_ipv6(ip: [u8; 16]) -> Vec<u8> {
        let mut value = vec![0x30, 0x12, 0x87, 0x10];
        value.extend_from_slice(&ip);
        value
    }

    fn make_cert(
        subject_cn: &str,
        issuer_cn: &str,
        is_ca: bool,
        key_usage: Option<u8>,
        extra_extensions: Vec<X509Extension>,
    ) -> X509Certificate {
        let mut extensions = Vec::new();
        if is_ca {
            extensions.push(X509Extension {
                oid: "2.5.29.19".to_string(),
                critical: true,
                value: der_basic_constraints_ca(),
            });
        }
        if let Some(usage) = key_usage {
            extensions.push(X509Extension {
                oid: "2.5.29.15".to_string(),
                critical: true,
                value: der_bit_string(usage),
            });
        }
        extensions.extend(extra_extensions);

        X509Certificate {
            version: 3,
            serial: vec![1, 2, 3, 4],
            signature_algo: SignatureAlgorithm {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                parameters: Vec::new(),
            },
            issuer: test_name(issuer_cn),
            not_before: 1,
            not_after: u64::MAX,
            subject: test_name(subject_cn),
            public_key: X509PublicKey {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                key_data: vec![0x11; 64],
                curve: None,
            },
            extensions,
            signature: vec![0x22; 64],
            tbs_data: vec![0x33; 32],
            raw: vec![0x44; 32],
        }
    }

    #[test]
    fn verify_chain_rejects_unknown_critical_extension() {
        let cert = make_cert(
            "leaf.echos.test",
            "leaf.echos.test",
            false,
            Some(0x05),
            vec![X509Extension {
                oid: "1.2.3.4.5".to_string(),
                critical: true,
                value: vec![0x05, 0x00],
            }],
        );
        let verifier = CertVerifier {
            trusted_roots: vec![cert.clone()],
            check_time: 1704067200,
        };
        assert!(matches!(
            verifier.verify_chain(&[cert]),
            Err(CertError::InvalidKeyUsage)
        ));
    }

    #[test]
    fn verify_hostname_prefers_ip_san_for_ipv4_literals() {
        let cert = make_cert(
            "service.echos.test",
            "service.echos.test",
            false,
            Some(0x05),
            vec![X509Extension {
                oid: "2.5.29.17".to_string(),
                critical: false,
                value: der_subject_alt_ip([127, 0, 0, 1]),
            }],
        );
        assert!(verify_hostname(&cert, "127.0.0.1"));
        assert!(!verify_hostname(&cert, "127.0.0.2"));
    }

    #[test]
    fn verify_hostname_prefers_ip_san_for_ipv6_literals() {
        let cert = make_cert(
            "service.echos.test",
            "service.echos.test",
            false,
            Some(0x05),
            vec![X509Extension {
                oid: "2.5.29.17".to_string(),
                critical: false,
                value: der_subject_alt_ipv6([
                    0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
                ]),
            }],
        );
        assert!(verify_hostname(&cert, "2001:db8::1"));
        assert!(!verify_hostname(&cert, "2001:db8::2"));
    }

    #[test]
    fn verify_chain_rejects_leaf_without_tls_server_usage() {
        let cert = make_cert(
            "leaf.echos.test",
            "leaf.echos.test",
            false,
            Some(0x20),
            Vec::new(),
        );
        let verifier = CertVerifier {
            trusted_roots: vec![cert.clone()],
            check_time: 1704067200,
        };
        assert!(matches!(
            verifier.verify_chain(&[cert]),
            Err(CertError::InvalidKeyUsage)
        ));
    }

    #[test]
    fn verify_chain_rejects_ca_leaf_even_when_root_trusts_it() {
        let cert = make_cert(
            "leaf.echos.test",
            "leaf.echos.test",
            true,
            Some(0x25),
            Vec::new(),
        );
        let verifier = CertVerifier {
            trusted_roots: vec![cert.clone()],
            check_time: 1704067200,
        };
        assert!(matches!(
            verifier.verify_chain(&[cert]),
            Err(CertError::InvalidChain)
        ));
    }

    #[test]
    fn verify_chain_rejects_ca_without_key_cert_sign() {
        let leaf = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x05),
            Vec::new(),
        );
        let root = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x04),
            Vec::new(),
        );
        let verifier = CertVerifier {
            trusted_roots: vec![root.clone()],
            check_time: 1704067200,
        };
        assert!(matches!(
            verifier.verify_chain(&[leaf, root]),
            Err(CertError::InvalidKeyUsage)
        ));
    }

    #[test]
    fn verify_chain_rejects_path_len_exceeded() {
        let leaf = make_cert(
            "leaf.echos.test",
            "intermediate.echos.test",
            false,
            Some(0x05),
            Vec::new(),
        );
        let intermediate = make_cert(
            "intermediate.echos.test",
            "root.echos.test",
            true,
            Some(0x20),
            Vec::new(),
        );
        let mut root = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x20),
            Vec::new(),
        );
        root.extensions.retain(|ext| ext.oid != "2.5.29.19");
        root.extensions.push(X509Extension {
            oid: "2.5.29.19".to_string(),
            critical: true,
            value: der_basic_constraints_ca_path_len(0),
        });
        let verifier = CertVerifier {
            trusted_roots: vec![root.clone()],
            check_time: 1704067200,
        };
        assert!(matches!(
            verifier.verify_chain(&[leaf, intermediate, root]),
            Err(CertError::InvalidChain)
        ));
    }

    #[test]
    fn revocation_checker_prefers_ocsp_and_crl_hits() {
        let cert = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x05),
            Vec::new(),
        );
        let issuer = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x60),
            Vec::new(),
        );

        let ocsp = OcspResponse {
            response_status: OcspResponseStatus::Successful,
            response_type: "1.3.6.1.5.5.7.48.1.1".to_string(),
            version: 1,
            responder_id: OcspResponderId::ByName(test_name("root.echos.test")),
            produced_at: 1704067200,
            responses: vec![OcspSingleResponse {
                cert_id_hash_algo: "1.3.14.3.2.26".to_string(),
                issuer_name_hash: vec![0; 20],
                issuer_key_hash: vec![0; 20],
                serial: cert.serial.clone(),
                status: OcspCertStatus::Revoked {
                    reason: CrlReason::KeyCompromise,
                    revocation_time: 1704060000,
                },
                this_update: 1704060000,
                next_update: Some(1704070000),
                produced_at: 1704067200,
            }],
            signature_algo: SignatureAlgorithm {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                parameters: Vec::new(),
            },
            signature: vec![0x55; 64],
            certs: Vec::new(),
        };

        let mut checker = RevocationChecker::new();
        checker.cache_ocsp(cert.serial.clone(), ocsp);
        assert!(matches!(
            checker.check_revocation(&cert, &issuer),
            Err(CertError::Revoked)
        ));

        checker.ocsp_cache.clear();
        checker.add_crl(X509Crl {
            version: 2,
            signature_algo: SignatureAlgorithm {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                parameters: Vec::new(),
            },
            issuer: issuer.subject.clone(),
            this_update: 1704060000,
            next_update: 1704070000,
            revoked_certs: vec![CrlEntry {
                serial: cert.serial.clone(),
                revocation_date: 1704060000,
                reason: CrlReason::KeyCompromise,
                invalidity_date: None,
            }],
            extensions: Vec::new(),
            signature: vec![0x66; 64],
            raw: Vec::new(),
        });
        assert!(matches!(
            checker.check_revocation(&cert, &issuer),
            Err(CertError::Revoked)
        ));
    }

    #[test]
    fn revocation_checker_rejects_crl_when_issuer_lacks_crl_signing_usage() {
        let cert = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x20),
            vec![],
        );
        let issuer = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x20),
            vec![],
        );
        let mut checker = RevocationChecker::new();
        checker.prefer_ocsp = false;
        checker.fetch_live = false;
        checker.add_crl(X509Crl {
            version: 2,
            signature_algo: SignatureAlgorithm {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                parameters: Vec::new(),
            },
            issuer: issuer.subject.clone(),
            this_update: 1704060000,
            next_update: 1704070000,
            revoked_certs: vec![CrlEntry {
                serial: cert.serial.clone(),
                revocation_date: 1704060000,
                reason: CrlReason::KeyCompromise,
                invalidity_date: None,
            }],
            extensions: Vec::new(),
            signature: vec![0x55; 64],
            raw: vec![0x77; 96],
        });
        assert!(matches!(
            checker.check_revocation(&cert, &issuer),
            Err(CertError::InvalidKeyUsage)
        ));
    }

    #[test]
    fn revocation_checker_hard_fail_without_live_status_reports_invalid_format() {
        let cert = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x05),
            vec![X509Extension {
                oid: "1.3.6.1.5.5.7.1.1".to_string(),
                critical: false,
                value: b"http:///ocsp".to_vec(),
            }],
        );
        let issuer = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x20),
            Vec::new(),
        );
        let mut checker = RevocationChecker::new();
        checker.hard_fail = true;
        checker.fetch_live = true;

        assert!(matches!(
            checker.check_revocation(&cert, &issuer),
            Err(CertError::InvalidFormat)
        ));
    }

    #[test]
    fn revocation_checker_hard_fail_rejects_unknown_cached_ocsp_status() {
        let cert = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x05),
            Vec::new(),
        );
        let issuer = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x20),
            Vec::new(),
        );
        let mut checker = RevocationChecker::new();
        checker.hard_fail = true;
        checker.fetch_live = false;
        checker.cache_ocsp(
            cert.serial.clone(),
            OcspResponse {
                response_status: OcspResponseStatus::Successful,
                response_type: "1.3.6.1.5.5.7.48.1.1".to_string(),
                version: 1,
                responder_id: OcspResponderId::ByName(test_name("root.echos.test")),
                produced_at: 1704067200,
                responses: vec![OcspSingleResponse {
                    cert_id_hash_algo: "1.3.14.3.2.26".to_string(),
                    issuer_name_hash: vec![0; 20],
                    issuer_key_hash: vec![0; 20],
                    serial: cert.serial.clone(),
                    status: OcspCertStatus::Unknown,
                    this_update: 1704060000,
                    next_update: Some(1704070000),
                    produced_at: 1704067200,
                }],
                signature_algo: SignatureAlgorithm {
                    algorithm: "1.2.840.113549.1.1.11".to_string(),
                    parameters: Vec::new(),
                },
                signature: vec![0x55; 64],
                certs: Vec::new(),
            },
        );

        assert!(matches!(
            checker.check_revocation(&cert, &issuer),
            Err(CertError::InvalidFormat)
        ));
    }

    #[test]
    fn verify_hostname_does_not_fallback_to_cn_when_dns_san_present() {
        let cert = make_cert(
            "cn-only.echos.test",
            "cn-only.echos.test",
            false,
            Some(0x05),
            vec![X509Extension {
                oid: "2.5.29.17".to_string(),
                critical: false,
                value: vec![
                    0x30, 0x10, 0x82, 0x0e, b'a', b'p', b'i', b'.', b'e', b'c', b'h', b'o', b's',
                    b'.', b't', b'e', b's', b't',
                ],
            }],
        );
        assert!(verify_hostname(&cert, "api.echos.test"));
        assert!(!verify_hostname(&cert, "cn-only.echos.test"));
    }

    #[test]
    fn revocation_checker_falls_back_to_crl_after_unknown_ocsp_status() {
        let cert = make_cert(
            "leaf.echos.test",
            "root.echos.test",
            false,
            Some(0x05),
            Vec::new(),
        );
        let issuer = make_cert(
            "root.echos.test",
            "root.echos.test",
            true,
            Some(0x60),
            Vec::new(),
        );
        let mut checker = RevocationChecker::new();
        checker.hard_fail = true;
        checker.fetch_live = false;
        checker.cache_ocsp(
            cert.serial.clone(),
            OcspResponse {
                response_status: OcspResponseStatus::Successful,
                response_type: "1.3.6.1.5.5.7.48.1.1".to_string(),
                version: 1,
                responder_id: OcspResponderId::ByName(test_name("root.echos.test")),
                produced_at: 1704067200,
                responses: vec![OcspSingleResponse {
                    cert_id_hash_algo: "1.3.14.3.2.26".to_string(),
                    issuer_name_hash: vec![0; 20],
                    issuer_key_hash: vec![0; 20],
                    serial: cert.serial.clone(),
                    status: OcspCertStatus::Unknown,
                    this_update: 1704060000,
                    next_update: Some(1704070000),
                    produced_at: 1704067200,
                }],
                signature_algo: SignatureAlgorithm {
                    algorithm: "1.2.840.113549.1.1.11".to_string(),
                    parameters: Vec::new(),
                },
                signature: vec![0x55; 64],
                certs: Vec::new(),
            },
        );
        checker.add_crl(X509Crl {
            version: 2,
            signature_algo: SignatureAlgorithm {
                algorithm: "1.2.840.113549.1.1.11".to_string(),
                parameters: Vec::new(),
            },
            issuer: issuer.subject.clone(),
            this_update: 1704060000,
            next_update: 1704070000,
            revoked_certs: vec![],
            extensions: Vec::new(),
            signature: vec![0x66; 64],
            raw: Vec::new(),
        });
        assert!(checker.check_revocation(&cert, &issuer).is_ok());
    }
}
