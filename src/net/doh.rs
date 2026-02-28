//! # DNS over HTTPS (DoH) - HTTPS Üzerinden DNS
//!
//! DoH, DNS sorgularını HTTP/HTTPS protokolü aracılığıyla şifreli olarak
//! iletir. RFC 8484 ile tanımlanmıştır.
//!
//! ## Geleneksel DNS'e Karşı DoH
//!
//! ```text
//! Geleneksel DNS (Port 53, şifresiz):
//!   Uygulama --> DNS Sorgusu (UDP/TCP, düz metin) --> 8.8.8.8:53
//!              [İSS bunu görebilir ve izleyebilir!]
//!
//! DoH (Port 443, şifreli TLS):
//!   Uygulama --> HTTPS POST/GET --> https://cloudflare-dns.com/dns-query
//!              [Şifreli, İSS sadece HTTPS trafiği olduğunu görür]
//! ```
//!
//! ## DoH İki Sorgu Modu (RFC 8484)
//!
//! ```text
//! GET Modu:
//!   GET /dns-query?dns=<Base64URL encoded DNS wire format>
//!   Accept: application/dns-message
//!
//!   Örnek:
//!   GET /dns-query?dns=AAABAAABAAAAAAAA...  HTTP/1.1
//!   Host: cloudflare-dns.com
//!   Accept: application/dns-message
//!
//! POST Modu:
//!   POST /dns-query  HTTP/1.1
//!   Host: cloudflare-dns.com
//!   Content-Type: application/dns-message
//!   Content-Length: <boyut>
//!
//!   [DNS wire format binary verisi]
//! ```
//!
//! ## Base64URL Kodlaması (GET Modu için)
//!
//! ```text
//! DNS wire format binary data --> Base64URL encode (padding yok) --> URL parametresi
//!
//! Normal Base64 karakterleri: A-Z a-z 0-9 + /
//! Base64URL karakterleri:     A-Z a-z 0-9 - _  (URL güvenli, '=' padding yok)
//!
//! Örnek: [0x00, 0x01, ...] --> "AAEB..."
//! ```
//!
//! ## Popüler DoH Sağlayıcıları
//!
//! ```text
//! Cloudflare: https://cloudflare-dns.com/dns-query (1.1.1.1)
//! Google:     https://dns.google/dns-query (8.8.8.8)
//! Quad9:      https://dns.quad9.net/dns-query (9.9.9.9)
//! ```

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

/// DoH içerik türü: DNS mesajlarını ikili (binary) wire formatında taşır
const DNS_MESSAGE_CONTENT_TYPE: &str = "application/dns-message";

/// DoH istemcisi.
///
/// Belirtilen DoH sunucu URL'sine DNS sorguları gönderir.
/// Yanıtlar önbelleklenerek sonraki sorgular hızlandırılır.
pub struct DohClient {
    pub server_url: String,                      // DoH sunucusunun HTTPS URL'si
    pub timeout_ms: u64,                         // Sorgu zaman aşımı (milisaniye)
    pub cache: BTreeMap<String, CachedResponse>, // Önceki yanıtların önbelleği
}

/// Önbelleklenen DoH yanıtı.
///
/// DNS yanıtı ham wire formatında saklanır.
/// `expiry` alanı, önbellekteki yanıtın ne zaman sona ereceğini belirtir.
#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub response: Vec<u8>, // DNS wire format yanıt verisi
    pub expiry: u64,       // Unix zaman damgası: bu zamandan sonra geçersiz
}

impl DohClient {
    /// Belirtilen URL ile yeni bir DoH istemcisi oluşturur.
    pub fn new(server_url: &str) -> Self {
        DohClient {
            server_url: server_url.to_string(),
            timeout_ms: 5000, // Varsayılan 5 saniyelik zaman aşımı
            cache: BTreeMap::new(),
        }
    }

    /// Cloudflare DoH istemcisi oluşturur (https://cloudflare-dns.com/dns-query).
    ///
    /// Cloudflare, DNS gizliliğini ve hızını ön planda tutan bir DoH sağlayıcısıdır.
    pub fn cloudflare() -> Self {
        Self::new("https://cloudflare-dns.com/dns-query")
    }

    /// Google DoH istemcisi oluşturur (https://dns.google/dns-query).
    pub fn google() -> Self {
        Self::new("https://dns.google/dns-query")
    }

    /// Quad9 DoH istemcisi oluşturur (https://dns.quad9.net/dns-query).
    ///
    /// Quad9, zararlı domain engelleme özelliği ile bilinir.
    pub fn quad9() -> Self {
        Self::new("https://dns.quad9.net/dns-query")
    }

    /// DNS sorgu paketini wire formatında (ikili) oluşturur.
    ///
    /// Oluşturulan paket hem GET (Base64URL kodlanarak) hem de
    /// POST (doğrudan body olarak) modunda kullanılabilir.
    ///
    /// Paket yapısı:
    /// ```text
    /// [DNS Header 12 byte] + [Question Section: etiket formatında domain + QTYPE + QCLASS]
    /// ```
    pub fn build_query(domain: &str, qtype: DnsRecordType) -> Vec<u8> {
        let mut query = Vec::new();

        // DNS Header (12 byte): ID=0x1234, RD=1 (özyinelemeli), 1 soru
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

        // Soru bölümü: domain adı etiket formatında kodla
        for label in domain.split('.') {
            if !label.is_empty() {
                query.push(label.len() as u8); // Etiket uzunluğu
                for c in label.chars() {
                    query.push(c as u8);
                }
            }
        }
        query.push(0); // Root label (alan adının sonu)

        // QTYPE: sorgu türü (A=1, AAAA=28 vb.)
        query.push((qtype as u16 >> 8) as u8);
        query.push((qtype as u16 & 0xFF) as u8);

        // QCLASS: IN (Internet) = 1
        query.push(0);
        query.push(1);

        query
    }

    /// DNS sorgusunu HTTPS GET yöntemiyle gönderir.
    ///
    /// DNS sorgusu Base64URL olarak kodlanır ve URL parametresi olarak eklenir:
    /// `GET /dns-query?dns=<base64url>  HTTP/1.1`
    ///
    /// NOT: Bu implementasyon TLS henüz desteklemediğinden hata döner.
    pub fn query_get(&self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);

        // DNS binary verisini Base64 URL kodla (dolgu karakteri '=' olmadan)
        let encoded = base64url_encode(&dns_query);

        // ?dns= parametresiyle URL oluştur
        let url = format!("{}?dns={}", self.server_url, encoded);

        // TODO: Make HTTPS request
        // For now, return error
        Err(DohError::HttpsNotSupported)
    }

    /// DNS sorgusunu HTTPS POST yöntemiyle gönderir.
    ///
    /// DNS sorgusu ikili formatta HTTP body olarak gönderilir:
    /// Content-Type: application/dns-message
    ///
    /// NOT: Bu implementasyon TLS henüz desteklemediğinden hata döner.
    pub fn query_post(&self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);

        // TODO: Make HTTPS POST request
        Err(DohError::HttpsNotSupported)
    }

    /// DoH sunucusundan gelen DNS yanıtını ayrıştırır.
    ///
    /// Yanıt, DNS wire format (ikili) içerir.
    /// Başlık ve yanıt kayıtları ayrıştırılır.
    pub fn parse_response(data: &[u8]) -> Result<DnsResponse, DohError> {
        if data.len() < 12 {
            return Err(DohError::InvalidResponse);
        }

        let header = DnsHeader::parse(data).map_err(|_| DohError::InvalidResponse)?;

        let mut response = DnsResponse {
            header,
            answers: Vec::new(),
        };

        // Soru bölümünü atla (sadece ilerle, içeriği ayrıştırma)
        let mut offset = 12;
        for _ in 0..header.qdcount {
            // Alan adını atla (etiket formatı veya sıkıştırma işaretçisi)
            while offset < data.len() && data[offset] != 0 {
                if (data[offset] & 0xC0) == 0xC0 {
                    offset += 2; // Sıkıştırma işaretçisi: 2 byte
                    break;
                }
                offset += 1 + data[offset] as usize;
            }
            if offset < data.len() && data[offset] == 0 {
                offset += 1; // Root label
            }
            offset += 4; // QTYPE + QCLASS (2+2 byte)
        }

        // Yanıt kayıtlarını ayrıştır
        for _ in 0..header.ancount {
            if offset >= data.len() {
                break;
            }

            let answer = Self::parse_answer(data, &mut offset)?;
            response.answers.push(answer);
        }

        Ok(response)
    }

    /// Tek bir DNS yanıt kaydını (Resource Record) ayrıştırır.
    ///
    /// NAME, TYPE, CLASS, TTL, RDLENGTH, RDATA alanlarını okur.
    /// A ve AAAA kayıtları için IP adresini çıkarır.
    fn parse_answer(data: &[u8], offset: &mut usize) -> Result<DnsAnswer, DohError> {
        // Yanıtın ait olduğu alan adını ayrıştır (sıkıştırma ile)
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

        // Kayıt türüne göre IP adresini çıkar
        let ip = match rtype {
            1 if rdlength == 4 => {
                // A kaydı: 4 byte IPv4 adresi
                Some(IpAddr::V4(Ipv4Addr::from_bytes([rdata[0], rdata[1], rdata[2], rdata[3]])))
            }
            28 if rdlength == 16 => {
                // AAAA kaydı: 16 byte IPv6 adresi
                let mut addr = [0u8; 16];
                addr.copy_from_slice(&rdata);
                Some(IpAddr::V6(Ipv6Addr::new(addr)))
            }
            _ => None, // Diğer kayıt türleri (MX, TXT, CNAME vb.)
        };

        Ok(DnsAnswer {
            name,
            rtype,
            rdata,
            ip,
        })
    }

    /// DNS wire formatındaki alan adını metne çevirir.
    ///
    /// DNS sıkıştırması (0xC0 prefix işaretçileri) desteklenir.
    /// Sonsuz döngüye karşı maksimum 5 atlama sınırı uygulanır.
    fn parse_name(data: &[u8], offset: &mut usize) -> Result<String, DohError> {
        let mut name = String::new();
        let mut jumped = false;    // Sıkıştırma atlama yapıldı mı?
        let mut max_jumps = 5;     // Döngü önleme: en fazla 5 sıkıştırma atlaması
        let original_offset = *offset;

        loop {
            if *offset >= data.len() {
                return Err(DohError::InvalidResponse);
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1; // Root label: alan adı sonu
                break;
            }

            // DNS sıkıştırma işaretçisi (ilk iki bit = 1 1)
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
                    return Err(DohError::InvalidResponse); // Döngü tespit edildi
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
            name.push('.'); // Root zone
        }

        Ok(name)
    }
}

/// IP adresi sarmalayıcısı: IPv4 veya IPv6 adresini tutar.
#[derive(Clone, Debug)]
pub enum IpAddr {
    V4(Ipv4Addr), // 32-bit IPv4 adresi
    V6(Ipv6Addr), // 128-bit IPv6 adresi
}

/// DNS yanıt kaydı (DoH bağlamında).
///
/// İsim, kayıt türü, ham RDATA ve çözümlenmiş IP adresini içerir.
#[derive(Clone, Debug)]
pub struct DnsAnswer {
    pub name: String,       // Yanıtın ait olduğu alan adı
    pub rtype: u16,         // Kayıt türü (1=A, 28=AAAA, 5=CNAME vb.)
    pub rdata: Vec<u8>,     // Ham kayıt verisi
    pub ip: Option<IpAddr>, // Çözümlenmiş IP adresi (sadece A ve AAAA için)
}

/// DoH sorgu yanıtı.
///
/// DNS başlığı ve yanıt kayıtları listesini içerir.
#[derive(Clone, Debug)]
pub struct DnsResponse {
    pub header: DnsHeader,    // DNS başlığı (ID, flags, sayılar)
    pub answers: Vec<DnsAnswer>, // Yanıt kayıtları listesi
}

impl DnsResponse {
    /// Yanıttaki ilk A kaydının IPv4 adresini döner.
    ///
    /// Birden fazla A kaydı varsa yalnızca ilki döner (round-robin için kullanışlı).
    pub fn get_a(&self) -> Option<Ipv4Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V4(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }

    /// Yanıttaki ilk AAAA kaydının IPv6 adresini döner.
    pub fn get_aaaa(&self) -> Option<Ipv6Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V6(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }
}

/// DoH hatası türleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DohError {
    HttpsNotSupported, // TLS/HTTPS henüz desteklenmiyor (TODO)
    InvalidResponse,   // Yanıt geçerli DNS formatında değil
    NetworkError,      // Ağ bağlantı hatası
    Timeout,           // Zaman aşımı
    ServerError(u16),  // HTTP hata kodu (4xx, 5xx)
}

/// Base64 URL kodlaması (dolgusu olmayan).
///
/// RFC 4648 Bölüm 5: URL ve dosya adı güvenli Base64 alfabesi.
/// Normal Base64 '+' ve '/' yerine '-' ve '_' kullanır.
/// DoH GET sorgusunda DNS wire formatını URL'de iletmek için kullanılır.
///
/// ```text
/// Girdi:  [0xAB, 0xCD, 0xEF]
/// Normal Base64: "q83v"
/// URL güvenli:   "q83v" (bu örnekte aynı, özel karakter yoksa)
///
/// Fark: 0xFB = 11111011
///   Normal B64:  '+' (43) yerine '-' (45)
///   Normal B64:  '/' (47) yerine '_' (95)
/// ```
fn base64url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

        // Her 3 byte -> 4 Base64 karakteri (6 bit grupları)
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
    // Dolgu '=' karakterleri eklenmez (RFC 8484 Base64URL without padding)

    result
}

// Global DoH istemcisi: lazy_static ile thread-safe şekilde başlatılır
lazy_static::lazy_static! {
    static ref DOH_CLIENT: Mutex<Option<DohClient>> = Mutex::new(None);
}

/// Global DoH istemcisini başlatır.
///
/// Belirtilen URL bir DoH sunucusuna, örneğin Cloudflare'e işaret etmelidir.
pub fn init_doh(server_url: &str) {
    *DOH_CLIENT.lock() = Some(DohClient::new(server_url));
}

/// Global DoH istemcisi üzerinden alan adını çözümler.
///
/// İstemci başlatılmamışsa hata döner.
/// TLS desteklenene kadar gerçek sorgu yapılamaz.
pub fn resolve_doh(domain: &str, qtype: DnsRecordType) -> Result<DnsResponse, DohError> {
    let client = DOH_CLIENT.lock();
    if let Some(client) = client.as_ref() {
        let response_data = client.query_get(domain, qtype)?;
        DohClient::parse_response(&response_data)
    } else {
        Err(DohError::HttpsNotSupported)
    }
}
