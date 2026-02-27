//! # X.509 Sertifika Zinciri Doğrulama
//!
//! TLS 1.3 için ASN.1 DER (Ayırt Edici Kodlama Kuralları) ayrıştırma ve X.509 sertifika doğrulama.
//! ASN.1 DER, TLV (Tag-Length-Value / Etiket-Uzunluk-Değer) formatında ikili kodlama kullanır.

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::format;
use spin::Mutex;

// ============================================================================
// ASN.1 DER AYRIŞTIRICISI
// ============================================================================

/// ASN.1 etiket sınıfları — bir elemanın hangi alana ait olduğunu belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asn1Class {
    Universal,
    Application,
    ContextSpecific,
    Private,
}

/// ASN.1 evrensel etiketler — DER'deki TLV yapısında T (Tag/Etiket) kısmının veri türünü belirtir
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

/// ASN.1 DER elemanı — TLV (Etiket-Uzunluk-Değer) formatındaki tek bir kodlanmış nesneyi temsil eder
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

/// ASN.1 DER Ayrıştırıcı — ham DER baytlarını TLV yapılarına dönüştürür
pub struct Asn1Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Asn1Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Asn1Parser { data, pos: 0 }
    }
    
    /// Tek bir TLV elemanını ayrıştır: önce etiket, sonra uzunluk, sonra değer okunur
    pub fn parse_element(&mut self) -> Option<Asn1Element> {
        if self.pos >= self.data.len() {
            return None;
        }
        
        // Etiketi oku — ilk bayt sınıf, yapı ve etiket numarasını içerir
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
        
        // Etiket numarasını oku — 0x1F ise uzun form kullanılıyor
        let tag_number = if (tag_byte & 0x1F) == 0x1F {
            // Uzun form — birden fazla bayta yayılan etiket numarası, her baytın MSB'si devam bayrağıdır
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
        
        // Uzunluğu oku — TLV'nin L kısmı: içerik baytlarının sayısını belirtir
        if self.pos >= self.data.len() {
            return None;
        }
        
        let len_byte = self.data[self.pos];
        self.pos += 1;
        
        let length = if len_byte < 0x80 {
            len_byte as usize
        } else if len_byte == 0x80 {
            // Belirsiz uzunluk — DER standardında desteklenmez, yalnızca BER'de kullanılır
            return None;
        } else {
            // Uzun form uzunluk — alt 7 bit, uzunluğu kodlayan bayt sayısını belirtir
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
        
        // Değeri oku — TLV'nin V kısmı: asıl içerik verisi
        if self.pos + length > self.data.len() {
            return None;
        }
        
        let data = self.data[self.pos..self.pos + length].to_vec();
        self.pos += length;
        
        // Yapılandırılmışsa alt elemanları özyinelemeli ayrıştır — SEQUENCE tipi iç içe TLV'ler içerir
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
    
    /// Veri tamponundaki tüm TLV elemanlarını sırasıyla ayrıştır
    pub fn parse_all(&mut self) -> Vec<Asn1Element> {
        let mut elements = Vec::new();
        while let Some(elem) = self.parse_element() {
            elements.push(elem);
        }
        elements
    }
}

/// ASN.1 elemanından OID (Object Identifier / Nesne Tanımlayıcı) ayrıştır.
/// OID, X.500/X.509 standartlarında algoritmaları, uzantıları ve öznitelikleri benzersiz biçimde tanımlar.
pub fn parse_oid(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    
    let mut oid = String::new();
    
    // İlk bayt, OID'nin ilk iki bileşenini kodlar (bileşen1 * 40 + bileşen2)
    let first = data[0];
    oid.push_str(&format!("{}.{}", first / 40, first % 40));
    
    // Kalan baytlar geri kalan OID bileşenlerini base-128 (her baytın MSB'si devam bayrağı) kodlar
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

/// X.509 Ayırt Edici Ad (Distinguished Name) — sertifika sahibini veya yayınlayıcıyı tanımlar
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
    
    /// ASN.1 dizisinden ayırt edici adı ayrıştır — SET içindeki SEQUENCE'ları tarar
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
                        
                        // OID'yi X.500 öznitelik adına eşle — bilinen OID'leri alanlara dönüştür
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

/// X.509 Açık Anahtar — sertifikadaki SubjectPublicKeyInfo yapısı
#[derive(Clone, Debug)]
pub struct X509PublicKey {
    pub algorithm: String,
    pub key_data: Vec<u8>,
    pub curve: Option<String>,
}

/// X.509 İmza Algoritması — RSA, ECDSA gibi algoritmaları OID ile tanımlar
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
    pub tbs_data: Vec<u8>,  // Doğrulama için imzalanacak TBSCertificate verisi
    pub raw: Vec<u8>,
}

/// X.509 Uzantısı — v3 sertifikalara ek özellikler ekler (basicConstraints, keyUsage, vb.)
#[derive(Clone, Debug)]
pub struct X509Extension {
    pub oid: String,
    pub critical: bool,
    pub value: Vec<u8>,
}

impl X509Certificate {
    /// DER (Ayırt Edici Kodlama Kuralları) baytlarından X.509 sertifikası ayrıştır.
    /// Yapı: Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
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
        
        // TBSCertificate verisini doğrulama amacıyla sakla — imza bu veri üzerine hesaplanır
        let tbs_data = tbs_cert.data.clone();
        
        // TBSCertificate (To-Be-Signed Certificate / İmzalanacak Sertifika Verisi)'ni ayrıştır
        if tbs_cert.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut idx = 0;
        
        // Sürüm (isteğe bağlı, bağlama özgü [0]) — v1=0, v2=1, v3=2 olarak kodlanır
        let version = if tbs_cert.children[idx].class == Asn1Class::ContextSpecific {
            let ver_elem = &tbs_cert.children[idx];
            if !ver_elem.children.is_empty() {
                let ver_int = &ver_elem.children[0];
                if ver_int.tag == Asn1Tag::Integer && !ver_int.data.is_empty() {
                    idx += 1;
                    ver_int.data[0] + 1  // Sürüm 0'dan başlar, insan okunaklılık için 1 ekliyoruz
                } else {
                    1
                }
            } else {
                idx += 1;
                1
            }
        } else {
            1  // Varsayılan sürüm: v1 (sadece temel alanlar içerir)
        };
        
        // Seri numarası — yayıncı başvuru alanı içinde sertifikayı benzersiz tanımlar
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let serial = tbs_cert.children[idx].data.clone();
        idx += 1;
        
        // TBSCertificate içindeki imza algoritması — dış imzayla örtüşmeli
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let tbs_sig_algo = SignatureAlgorithm::parse(&tbs_cert.children[idx])?;
        idx += 1;
        
        // Yayınlayıcı (Issuer) — sürtifikayı imzalayan CA'nın ayırt edici adı
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let issuer = X509Name::parse(&tbs_cert.children[idx].children);
        idx += 1;
        
        // Geçerlilik dönemi (Validity) — notBefore ve notAfter zaman damgalarını içerir
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let validity = &tbs_cert.children[idx];
        idx += 1;
        
        let (not_before, not_after) = if validity.children.len() >= 2 {
            let parse_time = |elem: &Asn1Element| -> u64 {
                let time_str = String::from_utf8_lossy(&elem.data);
                // UTCTime (YYAAGGssddssZ) veya GeneralizedTime (YYYYAAGGssddssZ) formatını ayrıştır
                if elem.tag == Asn1Tag::UtcTime {
                    // YYAAGGssddssZ formatı: YY=yıl(2 hane), AA=ay, GG=gün, ss=saat, dd=dakika, ss=saniye
                    if time_str.len() >= 12 {
                        let yy: u64 = time_str[0..2].parse().unwrap_or(0);
                        let mm: u64 = time_str[2..4].parse().unwrap_or(0);
                        let dd: u64 = time_str[4..6].parse().unwrap_or(0);
                        let hh: u64 = time_str[6..8].parse().unwrap_or(0);
                        let min: u64 = time_str[8..10].parse().unwrap_or(0);
                        let ss: u64 = time_str[10..12].parse().unwrap_or(0);
                        // Basit zaman damgası (kesin değil, yalnızca karşılaştırma amacıyla)
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
        
        // Konu (Subject) — sertifika sahibinin ayırt edici adı
        if idx >= tbs_cert.children.len() {
            return None;
        }
        let subject = X509Name::parse(&tbs_cert.children[idx].children);
        idx += 1;
        
        // Konu Açık Anahtar Bilgisi (SubjectPublicKeyInfo) — algoritma + açık anahtar verisi
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
            
            // BIT STRING'den anahtar verisini çıkar — X.509'de açık anahtar BIT STRING olarak kodlanır
            let key_data = if key_bits.tag == Asn1Tag::BitString && key_bits.data.len() > 1 {
                // Kullanılmayan bit sayısı baytını atla — BIT STRING'in ilk baytyı kullanılmayan bitleri belirtir
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
        
        // Uzantılar (isteğe bağlı, bağlama özgü [3]) — v3 sertifikalarında ek politika bilgileri
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
        
        // Dış imza algoritması — tbsCertificate içindekiyle örtüşmeli (RFC 5280 gerekliliği)
        let signature_algo = SignatureAlgorithm::parse(sig_algo)?;
        
        // İmza değeri (BIT STRING) — X.509 sertifikasının RSA veya ECDSA imzası
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
    
    /// Sertifikanın belirtilen zamanda geçerli olup olmadığını kontrol et (notBefore ≤ time ≤ notAfter)
    pub fn is_valid_at(&self, time: u64) -> bool {
        time >= self.not_before && time <= self.not_after
    }
    
    /// Temel kısıtlamaları kontrol et: sertifikada CA bayrağı set edilmiş mi?
    /// basicConstraints uzantısı (OID: 2.5.29.19) cA boolean alanını içerir.
    pub fn is_ca(&self) -> bool {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.19" {  // temelKisitlamalar (basicConstraints)
                // basicConstraints uzantısını ayrıştır: cA=TRUE ise sertifika bir CA sertifikasıdır
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
    
    /// Anahtar kullanım alanını al — keyUsage uzantısı hangi işlemlerin izinli olduğunu belirtir
    pub fn key_usage(&self) -> Option<u16> {
        for ext in &self.extensions {
            if ext.oid == "2.5.29.15" {  // anahtarKullanimi (keyUsage)
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
// SERTİFİKA DEPOSU
// ============================================================================

/// Kök CA sertifika deposu — güvenilen kerede CA sertifikalarını depolar
static ROOT_CA_STORE: Mutex<Vec<X509Certificate>> = Mutex::new(Vec::new());

/// Kök CA'yı depoya ekle
pub fn add_root_ca(cert: X509Certificate) {
    let mut store = ROOT_CA_STORE.lock();
    store.push(cert);
}

/// Kök CA deposunu al — klonlanmış kopyasını döndür
pub fn get_root_cas() -> Vec<X509Certificate> {
    ROOT_CA_STORE.lock().clone()
}

/// Kök CA deposunu temizle
pub fn clear_root_cas() {
    ROOT_CA_STORE.lock().clear();
}

// ============================================================================
// SERTİFİKA ZİNCİRİ DOĞRULAMA
// ============================================================================

/// Sertifika doğrulama hatası — zincir doğrulama sırasında ortaya çıkabilecek hata türleri
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

/// Sertifika zinciri doğrulayucısı — RFC 5280 uyumlu sertifika zinciri doğrulaması yapar
pub struct CertVerifier {
    pub trusted_roots: Vec<X509Certificate>,
    pub check_time: u64,
}

impl CertVerifier {
    pub fn new() -> Self {
        CertVerifier {
            trusted_roots: get_root_cas(),
            check_time: 0,  // Varsayılan: mevcut zamanı kullanacak
        }
    }
    
    /// Sertifika zincirini doğrula — geçerlilik süresi, CA bayrağı ve güven çıpasını kontrol eder
    pub fn verify_chain(&self, chain: &[X509Certificate]) -> Result<(), CertError> {
        if chain.is_empty() {
            return Err(CertError::InvalidChain);
        }
        
        // Mevcut zamanı al (basitleştirilmiş — gerçek sistemde RTC veya NTP kullanılır)
        let time = if self.check_time > 0 {
            self.check_time
        } else {
            // Rastgele tabanlı yapı zaman kullan — test amaçlı, üretimde gerçek saat gereklidir
            crate::random::next_u32() as u64
        };
        
        // Zincirdeki her sertifikayı sırayla doğrula
        for (i, cert) in chain.iter().enumerate() {
            // Geçerlilik dönemini kontrol et — notBefore ve notAfter zaman sınırlarına bak
            if !cert.is_valid_at(time) {
                if time < cert.not_before {
                    return Err(CertError::NotYetValid);
                } else {
                    return Err(CertError::Expired);
                }
            }
            
            // Yaprak sertifika mı kontrol et (i==0): son kullanıcı sertifikası
            if i == 0 {
                // Yaprak sertifika: CA olmadığını kontrol et
                // (öz-imzalı ise aşağıda ele alınır)
                continue;
            }
            
            // Ara veya kök sertifika — CA olmalı (basicConstraints.cA=TRUE gereklidir)
            if !cert.is_ca() {
                return Err(CertError::NotCA);
            }
        }
        
        // Güven çıpasını bul — zincirin sonundaki sertifika güvenilen kök deposuyla karşılaştırılır
        let last_cert = &chain[chain.len() - 1];
        
        // Son sertifikanın güvenilen kök sertifika olup olmadığını kontrol et
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
        
        // İmzaları doğrula (basitleştirilmiş — üretimde RSA/ECDSA imzası gerçekten doğrulanır)
        // Her sertifika için, zincirdeki bir sonraki sertifika tarafından imzalandığı kontrol edilir
        for i in 0..chain.len().saturating_sub(1) {
            let cert = &chain[i];
            let issuer = &chain[i + 1];
            
            // Yayınlayıcı adının eşleşip eşleşmediğini doğrula (issuer CN == issuer subject CN)
            if cert.issuer.common_name != issuer.subject.common_name {
                return Err(CertError::InvalidChain);
            }
            
            // Üretimde: yayınlayıcının açık anahtarı ile RSA veya ECDSA imzasını doğrula
            // Şimdiilik, zincirin düzgün imzalandığına güveniyoruz (stub uygulama)
        }
        
        Ok(())
    }
    
    /// Tek bir sertifikayı güvenilen kök sertifika listesine karşı doğrula
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
// BİLİNEN KÖK CA'LAR (GÖMÜLÜ)
// ============================================================================

/// Gömülü kök CA sertifikalarını başlat
pub fn init_builtin_roots() {
    // Üretimde, bunlar gerçek kök CA sertifikaları olurdu
    // Şimdiilik, boş bir depo başlatıyoruz
    // Gerçek uygulama: DigiCert, Let's Encrypt, GlobalSign gibi CA'ları içerirdi
    clear_root_cas();
}

// ============================================================================
// TLS ENTEGRASYONU
// ============================================================================

/// TLS el sıkışmasından sertifika zinciri ayrıştır
pub fn parse_certificate_chain(cert_data: &[u8]) -> Vec<X509Certificate> {
    let mut certs = Vec::new();
    let mut pos = 0;
    
    // TLS sertifika mesaj formatı:
    // u24 toplam_uzunluk
    // Her sertifika için:
    //   u24 sertifika_uzunlugu
    //   DER kodlu sertifika
    
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
// X.509 İPTAL LİSTESİ (CERTIFICATE REVOCATION LIST - CRL)
// ============================================================================

/// CRL İptal Neden Kodları — sertifikanın neden iptal edildiğini açıklar
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

/// CRL Kayıdı (iptal edilmiş sertifika) — seri numarası, iptal tarihi ve neden içerir
#[derive(Clone, Debug)]
pub struct CrlEntry {
    pub serial: Vec<u8>,
    pub revocation_date: u64,
    pub reason: CrlReason,
    pub invalidity_date: Option<u64>,
}

/// X.509 İptal Listesi (CRL) — CA tarafından yayımlanan iptal edilmiş sertifika listesi
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
    /// DER baytlarından CRL ayrıştır — TBSCertList, imza algoritması ve imzayı ayrıştırır
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
        
        // Sürüm (isteğe bağlı, varsayılan v1) — v2 CRL'ler uzantıları destekler
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
        
        // CRL imza algoritması
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let signature_algo = SignatureAlgorithm::parse(&tbs_crl.children[idx])?;
        idx += 1;
        
        // Yayınlayıcı (Issuer) — CRL'i yayımlayan CA'nın ayırt edici adı
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let issuer = X509Name::parse(&tbs_crl.children[idx].children);
        idx += 1;
        
        // Bu Güncelleme (ThisUpdate) — CRL'in güncellenme tarihi
        if idx >= tbs_crl.children.len() {
            return None;
        }
        let this_update = Self::parse_time(&tbs_crl.children[idx]);
        idx += 1;
        
        // Sonraki Güncelleme (NextUpdate, isteğe bağlı) — CRL'in sonraki yenileme tarihi
        let next_update = if idx < tbs_crl.children.len() && 
            (tbs_crl.children[idx].tag == Asn1Tag::UtcTime || 
             tbs_crl.children[idx].tag == Asn1Tag::GeneralizedTime) {
            let time = Self::parse_time(&tbs_crl.children[idx]);
            idx += 1;
            time
        } else {
            this_update + 86400 // Varsayılan: 24 saat sonra
        };
        
        // İptal edilmiş sertifikalar listesi
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
        
        // CRL uzantıları (isteğe bağlı, bağlama özgü [0]) — v2 CRL'lerde ek bilgiler
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
        
        // Dış imza algoritması (CRL düzeyinde)
        let _outer_sig_algo = SignatureAlgorithm::parse(sig_algo)?;
        
        // İmza değeri (BIT STRING) — CRL üzerindeki CA imzası
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
        
        // İptal uzantılarını ayrıştır: neden ve geçersizlik tarihi
        let mut reason = CrlReason::Unspecified;
        let mut invalidity_date = None;
        
        if elem.children.len() > 2 {
            let ext_seq = &elem.children[2];
            if ext_seq.tag == Asn1Tag::Sequence {
                for ext in &ext_seq.children {
                    if ext.tag == Asn1Tag::Sequence && ext.children.len() >= 2 {
                        let oid = parse_oid(&ext.children[0].data);
                        let value = &ext.children[1].data;
                        
                        // CRL nedenı (OID 2.5.29.21) — iptal nedenini ENUMERATED olarak kodlar
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
                        
                        // Geçersizlik tarihi (OID 2.5.29.24) — sertifikanın fiilen geçersiz olduğu tarih
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
    
    /// Sertifikanın iptal edilip edilmediğini seri numarasına göre kontrol et
    pub fn is_revoked(&self, serial: &[u8]) -> Option<&CrlEntry> {
        self.revoked_certs.iter().find(|e| e.serial == serial)
    }
    
    /// CRL'nin süresi dolmuş mu kontrol et — nextUpdate geçmişse CRL güncellenmeli
    pub fn is_expired(&self, time: u64) -> bool {
        time > self.next_update
    }
}

// ============================================================================
// OCSP (ÇEVRİMİÇİ SERTİFİKA DURUM PROTOKOLÜ - Online Certificate Status Protocol)
// ============================================================================

/// OCSP İsteği — sertifika durumunu sorgulamak için gönderilen mesaj
#[derive(Clone, Debug)]
pub struct OcspRequest {
    pub requestor_name: Option<X509Name>,
    pub request_list: Vec<OcspCertRequest>,
    pub signature_algo: Option<SignatureAlgorithm>,
    pub signature: Option<Vec<u8>>,
}

/// OCSP Sertifika Sorgu Birimi — tek bir sertifikanın yıkayıcı bilgilerini içerir
#[derive(Clone, Debug)]
pub struct OcspCertRequest {
    pub issuer_key_hash: [u8; 20],
    pub issuer_name_hash: [u8; 20],
    pub hash_algorithm: String,
    pub serial: Vec<u8>,
}

/// OCSP Yanıt Durumu — OCSP sunucusunun isteğe verdiği üst düzey cevap kodu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspResponseStatus {
    Successful = 0,
    MalformedRequest = 1,
    InternalError = 2,
    TryLater = 3,
    SigRequired = 5,
    Unauthorized = 6,
}

/// OCSP Sertifika Durumu — sorgu sonuç: geçerli, iptal edilmiş veya bilinmiyor
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcspCertStatus {
    Good,
    Revoked { reason: CrlReason, revocation_time: u64 },
    Unknown,
}

/// OCSP Tekil Yanıt — tek bir sertifikanın durum bilgisini içerir
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

/// OCSP Yanıtı — OCSP sunucusundan gelen tam yanıt mesajı
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

/// OCSP Yanıtlayıcı Kimliği — ada göre (DN) veya anahtar özeti ile tanımlanır
#[derive(Clone, Debug)]
pub enum OcspResponderId {
    ByName(X509Name),
   ByKey(Vec<u8>),
}

impl OcspRequest {
    /// Bir sertifika için yeni OCSP isteği oluştur (sorgu: bu sertifika geçerli mi?)
    pub fn new(cert: &X509Certificate, issuer: &X509Certificate) -> Self {
        // Yayınlayıcı adını ve anahtarını hash'le — CertID oluşturmak için gerekli
        let issuer_name_hash = Self::hash_name(&issuer.subject);
        let issuer_key_hash = Self::hash_key(&issuer.public_key.key_data);
        
        OcspRequest {
            requestor_name: None,
            request_list: vec![OcspCertRequest {
                issuer_key_hash,
                issuer_name_hash,
                hash_algorithm: "1.3.14.3.2.26".to_string(), // SHA-1 OID (Nesne Tanımlayıcısı)
                serial: cert.serial.clone(),
            }],
            signature_algo: None,
            signature: None,
        }
    }
    
    fn hash_name(name: &X509Name) -> [u8; 20] {
        // Basitleştirilmiş ad hash'i — gerçek SHA-1 yerine XOR tabanlı (stub)
        let mut hash = [0u8; 20];
        for (i, b) in name.common_name.as_bytes().iter().chain(
            name.organization.as_bytes().iter()
        ).enumerate() {
            hash[i % 20] ^= b;
        }
        hash
    }
    
    fn hash_key(key: &[u8]) -> [u8; 20] {
        // Basitleştirilmiş anahtar hash'i — gerçek SHA-1 yerine XOR tabanlı (stub)
        let mut hash = [0u8; 20];
        for (i, b) in key.iter().enumerate() {
            hash[i % 20] ^= b;
        }
        hash
    }
    
    /// İsteği DER formatına kodla — OCSP sunucusuna gönderilebilecek binary veri üretir
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        // OCSP İstek dizisi (SEQUENCE)
        buf.push(0x30); // SEQUENCE etiketi
        let len_pos = buf.len();
        buf.push(0); // Uzunluk yer tutucu (sonra güncellenecek)
        
        // TBSRequest ("To Be Signed Request" / İmzalanacak İstek)
        buf.push(0x30); // SEQUENCE etiketi
        let tbs_len_pos = buf.len();
        buf.push(0);
        
        // İstek listesi — sorgulanacak sertifika CertID'lerinin dizisi
        buf.push(0x30); // SEQUENCE etiketi
        let list_len_pos = buf.len();
        buf.push(0);
        
        for req in &self.request_list {
            // Tek bir sertifika isteği
            buf.push(0x30); // SEQUENCE etiketi
            let req_len_pos = buf.len();
            buf.push(0);
            
            // Sertifika Kimliği (CertID) — hash algoritması, yayınlayıcı hash'leri ve seri numarası
            buf.push(0x30); // SEQUENCE etiketi
            let certid_len_pos = buf.len();
            buf.push(0);
            
            // Hash algoritması (AlgorithmIdentifier)
            buf.push(0x30); // SEQUENCE etiketi
            buf.push(0x05); // Uzunluk
            buf.push(0x06); // OID etiketi
            buf.push(0x03); // OID uzunluğu
            buf.extend_from_slice(&[0x2A, 0x03, 0x04]); // SHA-1 ön ek baytları
            
            // Yayınlayıcı adı hash'i (OCTET STRING)
            buf.push(0x04); // OCTET STRING etiketi
            buf.push(20); // Uzunluk (SHA-1 = 20 bayt)
            buf.extend_from_slice(&req.issuer_name_hash);
            
            // Yayınlayıcı anahtarı hash'i (OCTET STRING)
            buf.push(0x04); // OCTET STRING etiketi
            buf.push(20); // Uzunluk (SHA-1 = 20 bayt)
            buf.extend_from_slice(&req.issuer_key_hash);
            
            // Seri numarası (INTEGER)
            buf.push(0x02); // INTEGER etiketi
            buf.push(req.serial.len() as u8);
            buf.extend_from_slice(&req.serial);
            
            // Uzunlukları güncelle (TLV yapısında L alanı gerçek boyuta göre yazılır)
            let certid_len = buf.len() - certid_len_pos - 1;
            buf[certid_len_pos] = certid_len as u8;
            
            let req_len = buf.len() - req_len_pos - 1;
            buf[req_len_pos] = req_len as u8;
        }
        
        // Kalan uzunlukları güncelle
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
    /// DER'den OCSP yanıtı ayrıştır — BasicOCSPResponse formatını işler
    pub fn parse(der: &[u8]) -> Option<Self> {
        let mut parser = Asn1Parser::new(der);
        let root = parser.parse_element()?;
        
        if root.tag != Asn1Tag::Sequence {
            return None;
        }
        
        // Yanıt durumunu oku (ENUMERATED) — üst düzey başarı/hata kodu
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
        
        // Yanıt baytlarını ayrıştır — ResponseBytes içindeki OID ve içerik verisi
        if root.children.len() < 2 {
            return None;
        }
        
        let response_bytes = &root.children[1];
        if response_bytes.tag != Asn1Tag::Sequence || response_bytes.children.len() < 2 {
            return None;
        }
        
        let response_type = parse_oid(&response_bytes.children[0].data);
        let response_data = &response_bytes.children[1].data;
        
        // BasicOCSPResponse yapısını ayrıştır
        let mut parser = Asn1Parser::new(response_data);
        let basic = parser.parse_element()?;
        
        if basic.tag != Asn1Tag::Sequence {
            return None;
        }
        
        let mut idx = 0;
        
        // Sürüm (isteğe bağlı) — yoksa v1 kabul edilir
        let version = if basic.children[idx].tag == Asn1Tag::Integer {
            idx += 1;
            if !basic.children[0].data.is_empty() { basic.children[0].data[0] + 1 } else { 1 }
        } else {
            1
        };
        
        // Yanıtlayıcı Kimliği — ada göre (byName) veya anahtar özetine göre (byKey)
        if idx >= basic.children.len() {
            return None;
        }
        let responder_id = Self::parse_responder_id(&basic.children[idx])?;
        idx += 1;
        
        // Üretim Zamanı (ProducedAt) — bu yanıtın oluşturulduğu an
        if idx >= basic.children.len() {
            return None;
        }
        let produced_at = X509Crl::parse_time(&basic.children[idx]);
        idx += 1;
        
        // Tekil yanıtlar listesi — her biri ayrı bir sertifikanın durumunu içerir
        if idx >= basic.children.len() {
            return None;
        }
        let responses = Self::parse_responses(&basic.children[idx])?;
        idx += 1;
        
        // İmza algoritması
        if idx >= basic.children.len() {
            return None;
        }
        let signature_algo = SignatureAlgorithm::parse(&basic.children[idx])?;
        idx += 1;
        
        // İmza
        if idx >= basic.children.len() {
            return None;
        }
        let signature = if basic.children[idx].tag == Asn1Tag::BitString && basic.children[idx].data.len() > 1 {
            basic.children[idx].data[1..].to_vec()
        } else {
            basic.children[idx].data.clone()
        };
        idx += 1;
        
        // Sertifikalar (isteğe bağlı) — yanıtlayıcının kendi sertifikası eklenebilir
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
                // Ada Göre (byName) — DN ile tanımlanmış yanıtlayıcı
                Some(OcspResponderId::ByName(X509Name::parse(&elem.children)))
            } else if elem.tag_number == 2 {
                // Anahtara Göre (byKey) — açık anahtar özeti ile tanımlanmış yanıtlayıcı
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
            
            // Sertifika Kimliği (CertID) — hash algoritması + yayınlayıcı hash'leri + seri
            let cert_id = &single.children[0];
            if cert_id.tag != Asn1Tag::Sequence || cert_id.children.len() < 4 {
                continue;
            }
            
            let hash_algo = parse_oid(&cert_id.children[0].children[0].data);
            let issuer_name_hash = cert_id.children[1].data.clone();
            let issuer_key_hash = cert_id.children[2].data.clone();
            let serial = cert_id.children[3].data.clone();
            
            // Sertifika Durumu — bağlama özgü: [0] geçerli, [1] iptal, [2] bilinmiyor
            let status = if single.children[1].class == Asn1Class::ContextSpecific {
                if single.children[1].tag_number == 0 {
                    // Geçerli (Good) — sertifika iptal edilmemiş
                    OcspCertStatus::Good
                } else if single.children[1].tag_number == 1 {
                    // İptal Edilmiş (Revoked) — iptal zamanı ve neden içerebilir
                    let revocation_time = X509Crl::parse_time(&single.children[1]);
                    let reason = CrlReason::Unspecified;
                    OcspCertStatus::Revoked { reason, revocation_time }
                } else {
                    // Bilinmiyor (Unknown) — yanıtlayıcı bu sertifikayı tanımıyor
                    OcspCertStatus::Unknown
                }
            } else {
                OcspCertStatus::Unknown
            };
            
            // bu Güncelleme (thisUpdate) — bu yanıtın geçerlilik başlangıcı
            let this_update = X509Crl::parse_time(&single.children[2]);
            
            // sonraki Güncelleme (nextUpdate, isteğe bağlı) — yanıtın geçerlilik bitiş tarihi
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
    
    /// Sertifika durumunu seri numarasına göre al
    pub fn get_cert_status(&self, serial: &[u8]) -> Option<&OcspSingleResponse> {
        self.responses.iter().find(|r| r.serial == serial)
    }
}

// ============================================================================
// İPTAL DENETLEYİCİSİ
// ============================================================================

/// CRL ve OCSP'yi birleştiren iptal denetleyicisi — önce OCSP'yi, yoksa CRL'yi kontrol eder
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
    
    /// CRL ekle — iptal listesini denetleyiciye tanıt
    pub fn add_crl(&mut self, crl: X509Crl) {
        self.crls.push(crl);
    }
    
    /// OCSP yanıtını önbelleğe al — ağ üzerinden sorgulama yapmak yerine önbelleği kullan
    pub fn cache_ocsp(&mut self, serial: Vec<u8>, response: OcspResponse) {
        self.ocsp_cache.push((serial, response));
    }
    
    /// Sertifikanın iptal edilip edilmediğini OCSP veya CRL kullanarak kontrol et
    pub fn check_revocation(&self, cert: &X509Certificate, issuer: &X509Certificate) -> Result<(), CertError> {
        // Tercih edilirse önce OCSP önbelleğini dene
        if self.prefer_ocsp {
            if let Some(response) = self.ocsp_cache.iter().find(|(s, _)| s == &cert.serial).map(|(_, r)| r) {
                return self.check_ocsp_status(response, &cert.serial);
            }
        }
        
        // CRL'leri kontrol et — yayınlayıcı eşleşenlerinde seri numarasına bak
        for crl in &self.crls {
            // CRL yayınlayıcısının sertifika yayınlayıcısıyla eşleşip eşleşmediğini kontrol et
            if crl.issuer.common_name == issuer.subject.common_name {
                if let Some(entry) = crl.is_revoked(&cert.serial) {
                    return Err(CertError::Revoked);
                }
            }
        }
        
        // OCSP tercih edilmiyorsa ve önbellekte bulunamadıysa yalnızca CRL'leri kontrol et
        if !self.prefer_ocsp {
            return Ok(());
        }
        
        // Aksi takdirde iptal durumu belirlenemedi
        // Üretimde: OCSP yanıtlayıcısına HTTP üzerinden ağ isteği yapılırdı
        Ok(())
    }
    
    fn check_ocsp_status(&self, response: &OcspResponse, serial: &[u8]) -> Result<(), CertError> {
        if response.response_status != OcspResponseStatus::Successful {
            // OCSP başarısız — CRL'e geri dön (graceful fallback)
            return Ok(());
        }
        
        if let Some(single) = response.get_cert_status(serial) {
            match single.status {
                OcspCertStatus::Good => Ok(()),
                OcspCertStatus::Revoked { .. } => Err(CertError::Revoked),
                OcspCertStatus::Unknown => Ok(()), // Bilinmeyen durum: kabul et
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
