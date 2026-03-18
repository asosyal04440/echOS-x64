//! # echOS HTTP Ä°stemcisi
//!
//! HTTP/1.1 protokolÃ¼nÃ¼ uygulayan istemci. GET, POST, indirme ve
//! yÃ¶nlendirme takip Ã¶zelliklerini destekler.
//!
//! ## HTTP/1.1 ProtokolÃ¼ Genel BakÄ±ÅŸ
//!
//! HTTP (HyperText Transfer Protocol), istemci-sunucu mimarisinde
//! Ã§alÄ±ÅŸan bir uygulama katmanÄ± protokolÃ¼dÃ¼r. TCP Ã¼zerinde taÅŸÄ±nÄ±r.
//!
//! ## HTTP Ä°stek/YanÄ±t DÃ¶ngÃ¼sÃ¼
//!
//! ```text
//! TarayÄ±cÄ±/Ä°stemci                    Web Sunucusu (Port 80)
//!      |                                      |
//!      |--- TCP SYN ---------------------->   |
//!      |<-- TCP SYN-ACK ------------------- --|
//!      |--- TCP ACK ---------------------->   |  [TCP baÄŸlantÄ±sÄ± kuruldu]
//!      |                                      |
//!      |--- HTTP Ä°stek (Request) ---------->  |
//!      |  GET /index.html HTTP/1.1             |
//!      |  Host: www.example.com               |
//!      |  User-Agent: echOS/1.0               |
//!      |  \r\n\r\n                            |
//!      |                                      |
//!      |<-- HTTP YanÄ±t (Response) ----------- |
//!      |  HTTP/1.1 200 OK                     |
//!      |  Content-Type: text/html             |
//!      |  Content-Length: 1234                |
//!      |  \r\n\r\n                            |
//!      |  [HTML iÃ§eriÄŸi]                      |
//! ```
//!
//! ## HTTP Ä°stek YapÄ±sÄ±
//!
//! ```text
//! [Durum SatÄ±rÄ±]   GET /path?query HTTP/1.1\r\n
//! [BaÅŸlÄ±klar]      Host: example.com\r\n
//!                  User-Agent: echOS/1.0\r\n
//!                  Accept: */*\r\n
//!                  Connection: close\r\n
//!                  \r\n                    <-- BoÅŸ satÄ±r: baÅŸlÄ±k sonu
//! [Veri (isteÄŸe baÄŸlÄ±)]  [POST/PUT iÃ§in istek gÃ¶vdesi]
//! ```
//!
//! ## HTTP YanÄ±t YapÄ±sÄ±
//!
//! ```text
//! [Durum SatÄ±rÄ±]   HTTP/1.1 200 OK\r\n
//! [BaÅŸlÄ±klar]      Content-Type: text/html\r\n
//!                  Content-Length: 1234\r\n
//!                  \r\n                    <-- BoÅŸ satÄ±r (CRLFCRLF): baÅŸlÄ±k sonu
//! [GÃ¶vde]          [HTML/JSON/binary vb. veri]
//!
//! Durum kodu aralÄ±klarÄ±:
//!   1xx = Bilgi (Informational)
//!   2xx = BaÅŸarÄ± (GET: 200 OK, POST: 201 Created)
//!   3xx = YÃ¶nlendirme (301 Moved Permanently, 302 Found)
//!   4xx = Ä°stemci HatasÄ± (400 Bad Request, 404 Not Found)
//!   5xx = Sunucu HatasÄ± (500 Internal Server Error)
//! ```
//!
//! ## Chunked Transfer Encoding
//!
//! ```text
//! Ä°Ã§erik uzunluÄŸu bilinmediÄŸinde (dinamik iÃ§erik) kullanÄ±lÄ±r:
//!
//! HTTP/1.1 200 OK
//! Transfer-Encoding: chunked
//! \r\n
//! 1A\r\n            <- Chunk boyutu: 0x1A = 26 decimal (hex)
//! Body data here...\r\n   <- 26 byte veri
//! 10\r\n            <- Chunk boyutu: 0x10 = 16
//! More data here\r\n       <- 16 byte veri
//! 0\r\n             <- Son chunk: 0 boyut = bitiÅŸ sinyali
//! \r\n
//! ```

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use super::socket::{close, connect, recv, send, socket as socket_create};
use super::socket::{AddressFamily, Protocol, SocketAddr, SocketType};
use super::{Ipv4Addr, NetError, Port};
use crate::net::tls::{wrap_record, ContentType, TlsClient};
use crate::net::x509::{
    init_builtin_roots, parse_certificate_chain, verify_hostname, CertError, CertVerifier,
};

// ============================================================================
// HTTP SABÄ°TLERÄ°
// ============================================================================

/// HTTP standart portu (ÅŸifresiz)
const HTTP_PORT: u16 = 80;
/// HTTPS standart portu (TLS ÅŸifreli)
const HTTPS_PORT: u16 = 443;
/// HTTP yanÄ±t baÅŸlÄ±klarÄ± iÃ§in maksimum tampon boyutu (8 KiB)
const MAX_HEADER_SIZE: usize = 8192;
/// Sonsuz yÃ¶nlendirme dÃ¶ngÃ¼sÃ¼nÃ¼ Ã¶nlemek iÃ§in maksimum yÃ¶nlendirme sayÄ±sÄ±
const MAX_REDIRECTS: u8 = 5;
/// AlÄ±m tamponu boyutu (her recv Ã§aÄŸrÄ±sÄ±nda en fazla bu kadar byte okunur)
const RECV_BUF_SIZE: usize = 4096;
/// VarsayÄ±lan baÄŸlantÄ± zaman aÅŸÄ±mÄ± (30 saniye)
const DEFAULT_TIMEOUT_MS: u64 = 30000;

static X509_ROOTS_READY: AtomicBool = AtomicBool::new(false);

// ============================================================================
// HTTP HATASI
// ============================================================================

/// HTTP istemci hata tÃ¼rleri.
///
/// AÄŸ hatalarÄ±ndan uygulama dÃ¼zeyindeki hatalara kadar tÃ¼m hata durumlarÄ±nÄ± kapsar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    Network(NetError),           // AÄŸ katmanÄ± hatasÄ± (TCP baÄŸlantÄ± kesilmesi vb.)
    InvalidUrl,                  // URL ayrÄ±ÅŸtÄ±rÄ±lamadÄ± (ÅŸema veya host eksik)
    InvalidResponse,             // YanÄ±t geÃ§erli HTTP formatÄ±nda deÄŸil
    ConnectionFailed,            // DNS Ã§Ã¶zÃ¼mleme veya TCP baÄŸlantÄ±sÄ± baÅŸarÄ±sÄ±z
    Timeout,                     // BaÄŸlantÄ± veya veri alÄ±m zaman aÅŸÄ±mÄ±
    TooManyRedirects,            // MAX_REDIRECTS sÄ±nÄ±rÄ± aÅŸÄ±ldÄ±
    NotFound,                    // HTTP 404 Not Found
    ServerError,                 // HTTP 5xx Sunucu HatasÄ±
    ProxyAuthenticationRequired, // Proxy 407 / CONNECT auth gerek
    InvalidHeader,               // BaÅŸlÄ±k UTF-8 geÃ§ersiz veya format hatasÄ±
    ChunkedEncoding,             // Chunked transfer encoding ayrÄ±ÅŸtÄ±rma hatasÄ±
    ContentLength,               // Content-Length baÅŸlÄ±ÄŸÄ± geÃ§ersiz veya eksik
    TlsHandshakeFailed,          // TLS el sikismasi state machine seviyesinde tamamlanamadi
    TlsDecodeFailed,             // TLS certificate/handshake transcript ayristrmasi tamamlanamadi
    TlsCertDateInvalid,          // Sertifika zaman gecerliligi basarisiz
    TlsCertCnInvalid,            // Hostname / SAN / CN eslesmesi basarisiz
    TlsInvalidCa,                // Sertifika zinciri guvenilen CA'ya baglanamadi
    TlsInvalidCertificate,       // Sertifika zinciri yapisi veya imzasi gecersiz
    TlsCertRevoked,              // Sertifika iptal edilmis
    TlsNotSupported,             // TLS handshake/record yolu mevcut taÅŸÄ±yÄ±cÄ± ile tamamlanamadÄ±
}

impl From<NetError> for HttpError {
    fn from(err: NetError) -> Self {
        HttpError::Network(err)
    }
}

fn map_cert_error(err: CertError) -> HttpError {
    match err {
        CertError::Expired | CertError::NotYetValid => HttpError::TlsCertDateInvalid,
        CertError::UnknownIssuer | CertError::SelfSigned => HttpError::TlsInvalidCa,
        CertError::Revoked => HttpError::TlsCertRevoked,
        CertError::InvalidFormat
        | CertError::InvalidSignature
        | CertError::InvalidChain
        | CertError::NotCA
        | CertError::InvalidKeyUsage => HttpError::TlsInvalidCertificate,
    }
}

// ============================================================================
// HTTP METODU
// ============================================================================

/// HTTP istek metodlarÄ±.
///
/// ```text
/// GET    : Kaynak al (yan etkisi yok, Ã¶nbelleklenebilir)
/// POST   : Yeni kaynak oluÅŸtur veya iÅŸlem baÅŸlat
/// PUT    : KaynaÄŸÄ± tamamen gÃ¼ncelle (idempotent)
/// DELETE : KaynaÄŸÄ± sil (idempotent)
/// HEAD   : Sadece baÅŸlÄ±klarÄ± al (GET gibi ama gÃ¶vde yok)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    HEAD,
}

impl HttpMethod {
    /// HTTP metodunun adÄ±nÄ± dÃ¶ner (Ã¶rn. "GET", "POST").
    ///
    /// Ä°stek satÄ±rÄ±nda kullanÄ±lÄ±r: "GET /path HTTP/1.1"
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::HEAD => "HEAD",
        }
    }
}

// ============================================================================
// HTTP BAÅLIKLARI
// ============================================================================

/// HTTP baÅŸlÄ±k koleksiyonu.
///
/// BTreeMap kullanÄ±lÄ±r (no_std uyumlu, alfabetik sÄ±ralÄ±).
/// TÃ¼m baÅŸlÄ±k anahtarlarÄ± kÃ¼Ã§Ã¼k harfe normalleÅŸtirilir (HTTP/1.1 bÃ¼yÃ¼k-kÃ¼Ã§Ã¼k harf duyarsÄ±z).
#[derive(Clone, Debug)]
pub struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

impl HttpHeaders {
    /// VarsayÄ±lan baÅŸlÄ±klar ile yeni bir koleksiyon oluÅŸturur.
    ///
    /// Her HTTP isteÄŸine otomatik eklenen baÅŸlÄ±klar:
    /// - User-Agent: echOS/1.0 (istemci kimliÄŸi)
    /// - Accept: */* (her tÃ¼rlÃ¼ iÃ§erik kabul edilir)
    /// - Connection: close (her istekten sonra baÄŸlantÄ±yÄ± kapat)
    pub fn new() -> Self {
        let mut headers = HttpHeaders {
            headers: BTreeMap::new(),
        };

        // VarsayÄ±lan baÅŸlÄ±klar
        headers.insert("User-Agent", "echOS/1.0");
        headers.insert("Accept", "*/*");
        headers.insert("Connection", "close");

        headers
    }

    /// BaÅŸlÄ±k ekler veya gÃ¼nceller.
    ///
    /// Anahtar kÃ¼Ã§Ã¼k harfe dÃ¶nÃ¼ÅŸtÃ¼rÃ¼lÃ¼r (HTTP baÅŸlÄ±klarÄ± bÃ¼yÃ¼k-kÃ¼Ã§Ã¼k harf duyarsÄ±z).
    pub fn insert(&mut self, key: &str, value: &str) {
        self.headers
            .insert(key.to_string().to_lowercase(), value.to_string());
    }

    /// Belirtilen baÅŸlÄ±ÄŸÄ±n deÄŸerini dÃ¶ner.
    ///
    /// Arama bÃ¼yÃ¼k-kÃ¼Ã§Ã¼k harf duyarsÄ±zdÄ±r: "Content-Type" ve "content-type" aynÄ±dÄ±r.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    /// Belirtilen baÅŸlÄ±ÄŸÄ± kaldÄ±rÄ±r.
    pub fn remove(&mut self, key: &str) {
        self.headers.remove(&key.to_lowercase());
    }

    /// TÃ¼m baÅŸlÄ±klarÄ± HTTP formatÄ±nda metin olarak dÃ¶ner.
    ///
    /// Her baÅŸlÄ±k "Anahtar: DeÄŸer\r\n" formatÄ±nda Ã§Ä±ktÄ±lanÄ±r.
    pub fn to_string(&self) -> String {
        let mut result = String::new();
        for (key, value) in &self.headers {
            result.push_str(key);
            result.push_str(": ");
            result.push_str(value);
            result.push_str("\r\n");
        }
        result
    }
}

impl Default for HttpHeaders {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP URL
// ============================================================================

/// AyrÄ±ÅŸtÄ±rÄ±lmÄ±ÅŸ HTTP URL yapÄ±sÄ±.
///
/// ```text
/// URL BileÅŸenleri:
///   https://user:pass@www.example.com:8080/path?query=val#fragment
///   ^^^^^                                                           ÅŸema (scheme)
///                      ^^^^^^^^^^^^^^^^^^^                          host
///                                         ^^^^                      port
///                                              ^^^^^                path
///                                                    ^^^^^^^^^^^    query
///                                                               ^^^^^^^^ fragment
/// ```
#[derive(Clone, Debug)]
pub struct HttpUrl {
    pub scheme: String,   // Protokol: "http" veya "https"
    pub host: String,     // Sunucu adÄ± veya IP: "www.example.com"
    pub port: u16,        // Port: 80 (http) veya 443 (https) varsayÄ±lan
    pub path: String,     // Kaynak yolu: "/api/v1/data" (varsayÄ±lan "/")
    pub query: String,    // Sorgu parametre: "key=val&foo=bar" (? iÅŸareti dahil deÄŸil)
    pub fragment: String, // ParÃ§a tanÄ±mlayÄ±cÄ±: "section1" (# iÅŸareti dahil deÄŸil)
}

impl HttpUrl {
    /// URL metnini ayrÄ±ÅŸtÄ±rarak HttpUrl yapÄ±sÄ±na dÃ¶nÃ¼ÅŸtÃ¼rÃ¼r.
    ///
    /// Desteklenen formatlar:
    /// - `http://example.com/path`
    /// - `https://example.com:8443/path?query#fragment`
    /// - `//example.com/path` (ÅŸema olmadan, varsayÄ±lan http)
    pub fn parse(url: &str) -> Result<Self, HttpError> {
        // DoÄŸrudan URL ayrÄ±ÅŸtÄ±rÄ±cÄ±
        // Format: scheme://host[:port][/path][?query][#fragment]

        let mut scheme = String::new();
        let mut host = String::new();
        let mut port = 0u16;
        let mut path = String::from("/");
        let mut query = String::new();
        let mut fragment = String::new();

        // ÅemayÄ± ayrÄ±ÅŸtÄ±r (://'dan Ã¶nceki kÄ±sÄ±m)
        let rest = if let Some(idx) = url.find("://") {
            scheme = url[..idx].to_string();
            &url[idx + 3..]
        } else {
            // Åema belirtilmemiÅŸ, varsayÄ±lan http
            scheme = String::from("http");
            url
        };

        // Åemaya gÃ¶re varsayÄ±lan port belirle
        port = if scheme == "https" {
            HTTPS_PORT
        } else {
            HTTP_PORT
        };

        // Host ve port ayrÄ±ÅŸtÄ±r (ilk / karakterine kadar)
        let path_start = rest.find('/').unwrap_or(rest.len());
        let host_port = &rest[..path_start];

        if let Some(idx) = host_port.find(':') {
            host = host_port[..idx].to_string();
            if let Ok(p) = host_port[idx + 1..].parse::<u16>() {
                port = p; // Ã–zel port numarasÄ±
            }
        } else {
            host = host_port.to_string();
        }

        // Path, query ve fragment ayrÄ±ÅŸtÄ±r
        if path_start < rest.len() {
            let path_rest = &rest[path_start..];

            // Fragment bÃ¶lÃ¼mÃ¼nÃ¼ ayÄ±r (# ile baÅŸlar)
            let path_query = if let Some(idx) = path_rest.find('#') {
                fragment = path_rest[idx + 1..].to_string();
                &path_rest[..idx]
            } else {
                path_rest
            };

            // Query bÃ¶lÃ¼mÃ¼nÃ¼ ayÄ±r (? ile baÅŸlar)
            let path_only = if let Some(idx) = path_query.find('?') {
                query = path_query[idx + 1..].to_string();
                &path_query[..idx]
            } else {
                path_query
            };

            path = path_only.to_string();
        }

        if host.is_empty() {
            return Err(HttpError::InvalidUrl);
        }

        Ok(HttpUrl {
            scheme,
            host,
            port,
            path,
            query,
            fragment,
        })
    }

    /// URL'yi tam metin olarak dÃ¶ner.
    ///
    /// Standart portlar (80/443) URL'ye eklenmez.
    pub fn to_url_string(&self) -> String {
        let mut result = String::new();
        result.push_str(&self.scheme);
        result.push_str("://");
        result.push_str(&self.host);

        // Standart olmayan portlar URL'de gÃ¶sterilir
        if (self.scheme == "http" && self.port != HTTP_PORT)
            || (self.scheme == "https" && self.port != HTTPS_PORT)
        {
            result.push(':');
            result.push_str(&self.port.to_string());
        }

        result.push_str(&self.path);

        if !self.query.is_empty() {
            result.push('?');
            result.push_str(&self.query);
        }

        if !self.fragment.is_empty() {
            result.push('#');
            result.push_str(&self.fragment);
        }

        result
    }

    /// Bu URL'nin HTTPS ÅŸemasÄ± kullanÄ±p kullanmadÄ±ÄŸÄ±nÄ± kontrol eder.
    pub fn is_https(&self) -> bool {
        self.scheme == "https"
    }
}

// ============================================================================
// HTTP YANITI
// ============================================================================

/// HTTP yanÄ±t yapÄ±sÄ±.
///
/// Sunucudan alÄ±nan durum kodu, baÅŸlÄ±klar ve gÃ¶vde verisini iÃ§erir.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status_code: u16, // HTTP durum kodu (200=OK, 404=Not Found, 500=Server Error)
    pub status_text: String, // Durum metni ("OK", "Not Found" vb.)
    pub headers: HttpHeaders, // YanÄ±t baÅŸlÄ±klarÄ± (Content-Type, Content-Length vb.)
    pub body: Vec<u8>,    // YanÄ±t gÃ¶vdesi (HTML, JSON, binary vb.)
}

impl HttpResponse {
    pub fn new() -> Self {
        HttpResponse {
            status_code: 0,
            status_text: String::new(),
            headers: HttpHeaders::new(),
            body: Vec::new(),
        }
    }

    /// YanÄ±tÄ±n baÅŸarÄ±lÄ± (2xx) olup olmadÄ±ÄŸÄ±nÄ± kontrol eder.
    ///
    /// 200-299 arasÄ± durum kodlarÄ± baÅŸarÄ± anlamÄ±na gelir.
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }

    /// YanÄ±tÄ±n yÃ¶nlendirme (3xx) iÃ§erip iÃ§ermediÄŸini kontrol eder.
    ///
    /// YÃ¶nlendirmede `Location` baÅŸlÄ±ÄŸÄ±ndan yeni URL alÄ±nmalÄ±dÄ±r.
    pub fn is_redirect(&self) -> bool {
        self.status_code >= 300 && self.status_code < 400
    }

    /// YanÄ±tÄ±n istemci hatasÄ± (4xx) iÃ§erip iÃ§ermediÄŸini kontrol eder.
    ///
    /// 404 Not Found, 401 Unauthorized, 403 Forbidden vb.
    pub fn is_client_error(&self) -> bool {
        self.status_code >= 400 && self.status_code < 500
    }

    /// YanÄ±tÄ±n sunucu hatasÄ± (5xx) iÃ§erip iÃ§ermediÄŸini kontrol eder.
    ///
    /// 500 Internal Server Error, 503 Service Unavailable vb.
    pub fn is_server_error(&self) -> bool {
        self.status_code >= 500 && self.status_code < 600
    }

    /// YanÄ±t gÃ¶vdesini UTF-8 metin olarak dÃ¶ner.
    ///
    /// GeÃ§ersiz byte'lar iÃ§in '?' karakteri kullanÄ±lÄ±r (kayÄ±plÄ± dÃ¶nÃ¼ÅŸtÃ¼rme).
    pub fn body_as_string(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// `Content-Length` baÅŸlÄ±ÄŸÄ±ndan iÃ§erik uzunluÄŸunu okur.
    ///
    /// BaÅŸlÄ±k eksikse veya ayrÄ±ÅŸtÄ±rÄ±lamazsa `None` dÃ¶ner.
    pub fn content_length(&self) -> Option<usize> {
        self.headers
            .get("content-length")
            .and_then(|s| s.parse::<usize>().ok())
    }

    /// `Transfer-Encoding: chunked` baÅŸlÄ±ÄŸÄ±nÄ±n olup olmadÄ±ÄŸÄ±nÄ± kontrol eder.
    ///
    /// Chunked encoding: YanÄ±t gÃ¶vdesi parÃ§alar halinde gelir.
    /// Her parÃ§a Ã¶nce hex boyutunu, ardÄ±ndan veriyi iÃ§erir.
    pub fn is_chunked(&self) -> bool {
        self.headers
            .get("transfer-encoding")
            .map(|s| s.to_lowercase() == "chunked")
            .unwrap_or(false)
    }

    /// YÃ¶nlendirme URL'sini `Location` baÅŸlÄ±ÄŸÄ±ndan okur.
    ///
    /// YalnÄ±zca 3xx yanÄ±tlarda anlamlÄ±dÄ±r.
    pub fn location(&self) -> Option<&str> {
        self.headers.get("location")
    }
}

impl Default for HttpResponse {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP Ä°STEMCÄ°SÄ°
// ============================================================================

/// HTTP/1.1 istemcisi.
///
/// DNS Ã§Ã¶zÃ¼mleme, TCP baÄŸlantÄ±sÄ±, istek gÃ¶nderme ve yanÄ±t ayrÄ±ÅŸtÄ±rmayÄ±
/// birleÅŸtirir. Otomatik yÃ¶nlendirme takibi desteklenir.
pub struct HttpClient {
    timeout_ms: u64,        // BaÄŸlantÄ± ve alÄ±m zaman aÅŸÄ±mÄ± (ms)
    max_redirects: u8,      // Maksimum otomatik yÃ¶nlendirme sayÄ±sÄ±
    follow_redirects: bool, // Otomatik yÃ¶nlendirme takip edilsin mi?
}

impl HttpClient {
    pub fn new() -> Self {
        HttpClient {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_redirects: MAX_REDIRECTS,
            follow_redirects: true,
        }
    }

    /// BaÄŸlantÄ± zaman aÅŸÄ±mÄ±nÄ± milisaniye cinsinden ayarlar.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Otomatik yÃ¶nlendirme takibini etkinleÅŸtirir veya devre dÄ±ÅŸÄ± bÄ±rakÄ±r.
    pub fn set_follow_redirects(&mut self, follow: bool) {
        self.follow_redirects = follow;
    }

    /// HTTP GET isteÄŸi gÃ¶nderir.
    ///
    /// Sunucudan kaynak al. Yan etkisi yok, Ã¶nbelleklenebilir.
    /// DNS Ã§Ã¶zÃ¼mleme -> TCP baÄŸlantÄ± -> Ä°stek -> YanÄ±t ayrÄ±ÅŸtÄ±rma.
    pub fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::GET, url, None, None)
    }

    /// HTTP POST isteÄŸi gÃ¶nderir.
    ///
    /// Sunucuda yeni kaynak oluÅŸtur veya iÅŸlem baÅŸlat.
    /// `body`: Ä°stek gÃ¶vdesi (form verisi, JSON vb.)
    /// `content_type`: Ä°Ã§erik tÃ¼rÃ¼ ("application/json", "application/x-www-form-urlencoded" vb.)
    pub fn post(
        &self,
        url: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::POST, url, Some(body), content_type)
    }

    /// Binary-safe HTTP POST request path.
    pub fn post_binary(
        &self,
        url: &str,
        body: &[u8],
        content_type: Option<&str>,
        accept: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        self.request_binary(HttpMethod::POST, url, Some(body), content_type, accept)
    }

    pub fn request_with_headers(
        &self,
        method: HttpMethod,
        url: &str,
        body: Option<&[u8]>,
        content_type: Option<&str>,
        accept: Option<&str>,
        extra_headers: &[(String, String)],
    ) -> Result<HttpResponse, HttpError> {
        let current_url = HttpUrl::parse(url)?;
        let request_target = build_request_target(&current_url, false);
        let request = self.build_request_bytes(
            method,
            &current_url,
            body,
            content_type,
            accept,
            extra_headers,
            &request_target,
            &current_url.host,
            current_url.port,
        );
        if current_url.is_https() {
            self.send_https_request(&current_url, &request)
        } else {
            let dns_server = super::get_config()
                .dns_servers
                .first()
                .copied()
                .unwrap_or([8, 8, 8, 8]);
            let dns_ip = Ipv4Addr::from_bytes(dns_server);
            let ip = super::dns::resolve(&current_url.host, dns_ip)
                .map_err(|_| HttpError::ConnectionFailed)?;

            let sock_id = socket_create(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)?;
            let addr = SocketAddr::new(ip, Port(current_url.port));
            connect(sock_id, addr)?;
            send(sock_id, &request, 0)?;
            let response = self.receive_response(sock_id)?;
            let _ = close(sock_id);
            Ok(response)
        }
    }

    pub fn request_via_proxy_with_headers(
        &self,
        method: HttpMethod,
        url: &str,
        body: Option<&[u8]>,
        content_type: Option<&str>,
        accept: Option<&str>,
        extra_headers: &[(String, String)],
        proxy_host: &str,
        proxy_port: u16,
    ) -> Result<HttpResponse, HttpError> {
        let current_url = HttpUrl::parse(url)?;
        let request_target = build_request_target(&current_url, true);
        let request = self.build_request_bytes(
            method,
            &current_url,
            body,
            content_type,
            accept,
            extra_headers,
            &request_target,
            &current_url.host,
            current_url.port,
        );
        if current_url.is_https() {
            return self.send_https_request_via_proxy(
                &current_url,
                &request,
                proxy_host,
                proxy_port,
            );
        }
        let sock_id = self.connect_tcp(proxy_host, proxy_port)?;
        send(sock_id, &request, 0)?;
        let response = self.receive_response(sock_id)?;
        let _ = close(sock_id);
        if response.status_code == 407 {
            return Err(HttpError::ProxyAuthenticationRequired);
        }
        Ok(response)
    }

    /// Genel HTTP isteÄŸi gÃ¶nderir.
    ///
    /// TÃ¼m HTTP metodlarÄ± iÃ§in temel uygulama. Åu iÅŸlemleri yapar:
    /// 1. URL ayrÄ±ÅŸtÄ±r
    /// 2. HTTPS ise TLS handshake + HTTP/1.1 over TLS
    /// 3. DNS ile hostname'i IP'ye Ã§evir
    /// 4. TCP soketi oluÅŸtur ve baÄŸlan
    /// 5. HTTP isteÄŸini hazÄ±rla ve gÃ¶nder
    /// 6. YanÄ±tÄ± al ve ayrÄ±ÅŸtÄ±r
    /// 7. YÃ¶nlendirme varsa tekrarla
    pub fn request(
        &self,
        method: HttpMethod,
        url: &str,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        let mut current_url = HttpUrl::parse(url)?;
        let mut redirect_count = 0;

        loop {
            // HTTP istek metnini oluÅŸtur
            let request = self.build_request(method, &current_url, body, content_type);
            let response = if current_url.is_https() {
                self.send_https_request(&current_url, request.as_bytes())?
            } else {
                // DNS ile hostname'i IP adresine Ã§evir
                let dns_server = super::get_config()
                    .dns_servers
                    .first()
                    .copied()
                    .unwrap_or([8, 8, 8, 8]);
                let dns_ip = Ipv4Addr::from_bytes(dns_server);
                let ip = super::dns::resolve(&current_url.host, dns_ip)
                    .map_err(|_| HttpError::ConnectionFailed)?;

                // TCP soketi oluÅŸtur (STREAM = baÄŸlantÄ± yÃ¶nelimli)
                let sock_id =
                    socket_create(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)?;

                // Web sunucusuna baÄŸlan (genellikle port 80)
                let addr = SocketAddr::new(ip, Port(current_url.port));
                connect(sock_id, addr)?;

                // Ä°steÄŸi gÃ¶nder
                send(sock_id, request.as_bytes(), 0)?;

                // YanÄ±tÄ± al ve ayrÄ±ÅŸtÄ±r
                let response = self.receive_response(sock_id)?;

                // Soketi kapat (Connection: close olduÄŸu iÃ§in baÄŸlantÄ± zaten kapatÄ±lacak)
                let _ = close(sock_id);
                response
            };

            // YÃ¶nlendirme mi?
            if response.is_redirect() && self.follow_redirects {
                redirect_count += 1;
                if redirect_count > self.max_redirects {
                    return Err(HttpError::TooManyRedirects);
                }

                if let Some(location) = response.location() {
                    // GÃ¶reli URL desteÄŸi
                    if location.starts_with('/') {
                        current_url.path = location.to_string(); // Mutlak path
                    } else if location.starts_with("http://") || location.starts_with("https://") {
                        current_url = HttpUrl::parse(location)?; // Tam URL
                    } else {
                        // GÃ¶reli URL: mevcut path'e gÃ¶re yorumla
                        current_url.path = location.to_string();
                    }
                    continue; // Yeni URL ile yeniden dene
                }
            }

            return Ok(response);
        }
    }

    fn request_binary(
        &self,
        method: HttpMethod,
        url: &str,
        body: Option<&[u8]>,
        content_type: Option<&str>,
        accept: Option<&str>,
    ) -> Result<HttpResponse, HttpError> {
        let current_url = HttpUrl::parse(url)?;
        self.request_with_headers(method, url, body, content_type, accept, &[])
    }

    fn send_https_request(&self, url: &HttpUrl, request: &[u8]) -> Result<HttpResponse, HttpError> {
        let sock_id = self.connect_tcp(&url.host, url.port)?;
        self.send_https_request_on_socket(sock_id, url, request)
    }

    fn send_https_request_via_proxy(
        &self,
        url: &HttpUrl,
        request: &[u8],
        proxy_host: &str,
        proxy_port: u16,
    ) -> Result<HttpResponse, HttpError> {
        let sock_id = self.connect_tcp(proxy_host, proxy_port)?;
        let proxy_auth_header = extract_proxy_authorization(request)
            .map(|value| alloc::format!("Proxy-Authorization: {}\r\n", value))
            .unwrap_or_default();
        let connect_request = alloc::format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n{}Connection: keep-alive\r\n\r\n",
            url.host,
            url.port,
            url.host,
            url.port,
            proxy_auth_header
        );
        send(sock_id, connect_request.as_bytes(), 0)?;
        let proxy_head = self.receive_response_head(sock_id)?;
        let proxy_text =
            core::str::from_utf8(&proxy_head).map_err(|_| HttpError::InvalidResponse)?;
        let Some(status_line) = proxy_text.lines().next() else {
            let _ = close(sock_id);
            return Err(HttpError::InvalidResponse);
        };
        if !status_line.contains(" 200 ") && !status_line.ends_with(" 200") {
            let _ = close(sock_id);
            if status_line.contains(" 407 ") || status_line.ends_with(" 407") {
                return Err(HttpError::ProxyAuthenticationRequired);
            }
            return Err(HttpError::ConnectionFailed);
        }
        self.send_https_request_on_socket(sock_id, url, request)
    }

    fn send_https_request_on_socket(
        &self,
        sock_id: u32,
        url: &HttpUrl,
        request: &[u8],
    ) -> Result<HttpResponse, HttpError> {
        let mut tls = TlsClient::new();
        let client_hello = tls.build_client_hello(&url.host);
        let hello_record = wrap_record(ContentType::Handshake, &client_hello);
        send(sock_id, &hello_record, 0)?;

        let mut sh_buf = [0u8; 4096];
        let sh_len = recv(sock_id, &mut sh_buf, 0)?;
        if sh_len > 5 {
            tls.process_server_hello(&sh_buf[5..sh_len])
                .map_err(|_| HttpError::ConnectionFailed)?;
        } else {
            let _ = close(sock_id);
            return Err(HttpError::ConnectionFailed);
        }

        let mut hs_buf = [0u8; 8192];
        let hs_len = recv(sock_id, &mut hs_buf, 0)?;
        if hs_len > 5 {
            let handshake_bytes = tls_strip_records(&hs_buf[..hs_len]);
            process_tls_server_handshake_flight(&mut tls, &handshake_bytes, &url.host)?;
        } else {
            let _ = close(sock_id);
            return Err(HttpError::ConnectionFailed);
        }
        tls.complete_handshake();
        if !tls.is_established() {
            let _ = close(sock_id);
            return Err(HttpError::ConnectionFailed);
        }

        let req_record = wrap_record(ContentType::ApplicationData, request);
        send(sock_id, &req_record, 0)?;

        let mut encrypted = Vec::new();
        loop {
            let mut chunk = vec![0u8; RECV_BUF_SIZE];
            let n = recv(sock_id, &mut chunk, 0)?;
            if n == 0 {
                break;
            }
            encrypted.extend_from_slice(&chunk[..n]);
        }
        let _ = close(sock_id);

        let plaintext = tls_strip_records(&encrypted);
        if plaintext.is_empty() {
            return Err(HttpError::InvalidResponse);
        }
        self.parse_response_bytes(&plaintext)
    }

    fn connect_tcp(&self, host: &str, port: u16) -> Result<u32, HttpError> {
        let dns_server = super::get_config()
            .dns_servers
            .first()
            .copied()
            .unwrap_or([8, 8, 8, 8]);
        let dns_ip = Ipv4Addr::from_bytes(dns_server);
        let ip = super::dns::resolve(host, dns_ip).map_err(|_| HttpError::ConnectionFailed)?;

        let sock_id = socket_create(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)?;
        let addr = SocketAddr::new(ip, Port(port));
        connect(sock_id, addr)?;
        Ok(sock_id)
    }

    fn receive_response_head(&self, sock_id: u32) -> Result<Vec<u8>, HttpError> {
        let mut header = Vec::new();
        loop {
            let mut chunk = vec![0u8; RECV_BUF_SIZE];
            let n = recv(sock_id, &mut chunk, 0)?;
            if n == 0 {
                break;
            }
            header.extend_from_slice(&chunk[..n]);
            if header.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        Ok(header)
    }

    /// Belirtilen URL'den dosya indirir.
    ///
    /// GET isteÄŸi gÃ¶nderir ve yanÄ±t gÃ¶vdesini dÃ¶ner.
    /// 404 iÃ§in NotFound, diÄŸer hatalÄ± durum kodlarÄ± iÃ§in ServerError dÃ¶ner.
    pub fn download(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let response = self.get(url)?;

        if !response.is_success() {
            if response.status_code == 404 {
                return Err(HttpError::NotFound);
            }
            return Err(HttpError::ServerError);
        }

        Ok(response.body)
    }

    /// HTTP istek metnini oluÅŸturur.
    ///
    /// RFC 7230'a uygun HTTP/1.1 formatÄ±nda:
    /// ```text
    /// METHOD /path?query HTTP/1.1\r\n
    /// Host: example.com\r\n
    /// Content-Length: <n>\r\n    (POST/PUT iÃ§in)
    /// Content-Type: <type>\r\n   (POST/PUT iÃ§in)
    /// User-Agent: echOS/1.0\r\n
    /// Accept: */*\r\n
    /// Connection: close\r\n
    /// \r\n
    /// [gÃ¶vde verisi]
    /// ```
    fn build_request(
        &self,
        method: HttpMethod,
        url: &HttpUrl,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> String {
        let mut request = String::new();

        // Ä°stek satÄ±rÄ±: METHOD /path?query HTTP/1.1
        let mut path_query = url.path.clone();
        if !url.query.is_empty() {
            path_query.push('?');
            path_query.push_str(&url.query);
        }

        request.push_str(method.as_str());
        request.push(' ');
        request.push_str(&path_query);
        request.push_str(" HTTP/1.1\r\n");

        // Host baÅŸlÄ±ÄŸÄ±: sanal hosting iÃ§in zorunlu (HTTP/1.1)
        request.push_str("Host: ");
        request.push_str(&url.host);
        if url.port != HTTP_PORT && url.port != HTTPS_PORT {
            request.push(':');
            request.push_str(&url.port.to_string());
        }
        request.push_str("\r\n");

        // POST/PUT iÃ§in iÃ§erik baÅŸlÄ±klarÄ±
        if let Some(data) = body {
            request.push_str("Content-Length: ");
            request.push_str(&data.len().to_string());
            request.push_str("\r\n");

            if let Some(ct) = content_type {
                request.push_str("Content-Type: ");
                request.push_str(ct);
                request.push_str("\r\n");
            }
        }

        // Genel baÅŸlÄ±klar
        request.push_str("User-Agent: echOS/1.0\r\n");
        request.push_str("Accept: */*\r\n");
        request.push_str("Connection: close\r\n");

        // BoÅŸ satÄ±r: baÅŸlÄ±klarÄ±n sonu (CRLFCRLF)
        request.push_str("\r\n");

        // Ä°stek gÃ¶vdesi (sadece POST/PUT gibi metodlar iÃ§in)
        if let Some(data) = body {
            // Ä°Ã§erik metin ise doÄŸrudan ekle
            // GerÃ§ek implementasyonda bytes doÄŸruca yazÄ±lmalÄ±
            let body_str = core::str::from_utf8(data).unwrap_or("");
            request.push_str(body_str);
        }

        request
    }

    fn build_request_bytes(
        &self,
        method: HttpMethod,
        url: &HttpUrl,
        body: Option<&[u8]>,
        content_type: Option<&str>,
        accept: Option<&str>,
        extra_headers: &[(String, String)],
        request_target: &str,
        host_header: &str,
        host_port: u16,
    ) -> Vec<u8> {
        let mut request = Vec::new();

        request.extend_from_slice(method.as_str().as_bytes());
        request.extend_from_slice(b" ");
        request.extend_from_slice(request_target.as_bytes());
        request.extend_from_slice(b" HTTP/1.1\r\n");

        request.extend_from_slice(b"Host: ");
        request.extend_from_slice(host_header.as_bytes());
        if host_port != HTTP_PORT && host_port != HTTPS_PORT {
            request.extend_from_slice(b":");
            request.extend_from_slice(host_port.to_string().as_bytes());
        }
        request.extend_from_slice(b"\r\n");

        if let Some(data) = body {
            request.extend_from_slice(b"Content-Length: ");
            request.extend_from_slice(data.len().to_string().as_bytes());
            request.extend_from_slice(b"\r\n");

            if let Some(ct) = content_type {
                request.extend_from_slice(b"Content-Type: ");
                request.extend_from_slice(ct.as_bytes());
                request.extend_from_slice(b"\r\n");
            }
        }

        request.extend_from_slice(b"User-Agent: echOS/1.0\r\n");
        request.extend_from_slice(b"Accept: ");
        request.extend_from_slice(accept.unwrap_or("*/*").as_bytes());
        request.extend_from_slice(b"\r\n");
        for (key, value) in extra_headers {
            request.extend_from_slice(key.as_bytes());
            request.extend_from_slice(b": ");
            request.extend_from_slice(value.as_bytes());
            request.extend_from_slice(b"\r\n");
        }
        request.extend_from_slice(b"Connection: close\r\n\r\n");

        if let Some(data) = body {
            request.extend_from_slice(data);
        }

        request
    }

    /// HTTP yanÄ±tÄ±nÄ± soket Ã¼zerinden alÄ±r ve ayrÄ±ÅŸtÄ±rÄ±r.
    ///
    /// AÅŸamalar:
    /// 1. BaÅŸlÄ±klarÄ± al (CRLFCRLF'e kadar)
    /// 2. Durum satÄ±rÄ± ve baÅŸlÄ±k alanlarÄ±nÄ± ayrÄ±ÅŸtÄ±r
    /// 3. GÃ¶vdeyi al:
    ///    - Chunked: chunk-by-chunk oku
    ///    - Content-Length: tam uzunluk oku
    ///    - BaÄŸlantÄ± kapanana dek: sonsuz oku
    fn receive_response(&self, sock_id: u32) -> Result<HttpResponse, HttpError> {
        let mut response = HttpResponse::new();
        let mut header_buf = vec![0u8; MAX_HEADER_SIZE];
        let mut header_len = 0;

        // BaÅŸlÄ±klarÄ± al (CRLFCRLF = \r\n\r\n sinyaline kadar)
        loop {
            let mut chunk = vec![0u8; RECV_BUF_SIZE];
            let n = recv(sock_id, &mut chunk, 0)?;

            if n == 0 {
                break; // BaÄŸlantÄ± kapandÄ±
            }

            // BaÅŸlÄ±k tamponuna kopyala
            let copy_len = core::cmp::min(n, MAX_HEADER_SIZE - header_len);
            header_buf[header_len..header_len + copy_len].copy_from_slice(&chunk[..copy_len]);
            header_len += copy_len;

            // BaÅŸlÄ±k sonu iÅŸaretÃ§isi bulundu mu? (\r\n\r\n)
            let header_end = find_header_end(&header_buf[..header_len]);
            if header_end.is_some() {
                break;
            }
        }

        // BaÅŸlÄ±k sonu konumunu bul (zorunlu)
        let header_end =
            find_header_end(&header_buf[..header_len]).ok_or(HttpError::InvalidResponse)?;

        let header_str = core::str::from_utf8(&header_buf[..header_end])
            .map_err(|_| HttpError::InvalidHeader)?;

        // BaÅŸlÄ±klarÄ± ayrÄ±ÅŸtÄ±r (durum satÄ±rÄ± + baÅŸlÄ±k alanlarÄ±)
        self.parse_response_headers(header_str, &mut response)?;

        // GÃ¶vde tampondan baÅŸlangÄ±Ã§ konumu: header_end + 4 (\r\n\r\n = 4 byte)
        let body_start = header_end + 4;

        if response.is_chunked() {
            // Chunked transfer encoding: hex boyut + veri bloklarÄ±nÄ± oku
            self.receive_chunked_body(sock_id, &mut response)?;
        } else if let Some(content_len) = response.content_length() {
            // Content-Length bilinÃ§: tam olarak bu kadar byte oku

            // BaÅŸlÄ±k tamponunda zaten gelen gÃ¶vde verisi varsa kopyala
            let initial_body_len = header_len - body_start;
            if initial_body_len > 0 {
                response
                    .body
                    .extend_from_slice(&header_buf[body_start..header_len]);
            }

            // Kalan gÃ¶vdeyi oku
            while response.body.len() < content_len {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break; // BaÄŸlantÄ± kapandÄ±
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        } else {
            // Content-Length yok: baÄŸlantÄ± kapanana dek oku
            response
                .body
                .extend_from_slice(&header_buf[body_start..header_len]);

            loop {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break; // BaÄŸlantÄ± kapandÄ±, gÃ¶vde tamamlandÄ±
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        }

        Ok(response)
    }

    fn parse_response_bytes(&self, bytes: &[u8]) -> Result<HttpResponse, HttpError> {
        let mut response = HttpResponse::new();
        let header_end = find_header_end(bytes).ok_or(HttpError::InvalidResponse)?;
        let header_str =
            core::str::from_utf8(&bytes[..header_end]).map_err(|_| HttpError::InvalidHeader)?;
        self.parse_response_headers(header_str, &mut response)?;
        let body_start = header_end + 4;
        let body = &bytes[body_start..];

        if response.is_chunked() {
            response.body = decode_chunked_body(body)?;
        } else if let Some(content_len) = response.content_length() {
            let take = core::cmp::min(content_len, body.len());
            response.body.extend_from_slice(&body[..take]);
        } else {
            response.body.extend_from_slice(body);
        }

        Ok(response)
    }

    /// HTTP yanÄ±t baÅŸlÄ±klarÄ±nÄ± ayrÄ±ÅŸtÄ±rÄ±r.
    ///
    /// Ä°lk satÄ±r: "HTTP/1.1 200 OK" (durum satÄ±rÄ±)
    /// Sonraki satÄ±rlar: "Anahtar: DeÄŸer" formatÄ±nda baÅŸlÄ±k alanlarÄ±
    fn parse_response_headers(
        &self,
        header_str: &str,
        response: &mut HttpResponse,
    ) -> Result<(), HttpError> {
        let mut lines = header_str.lines();

        // Durum satÄ±rÄ±: "HTTP/1.1 200 OK"
        let status_line = lines.next().ok_or(HttpError::InvalidResponse)?;

        // "HTTP/1.1", "200", "OK" olarak ayÄ±r (en fazla 3 parÃ§a)
        let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return Err(HttpError::InvalidResponse);
        }

        response.status_code = parts[1].parse().map_err(|_| HttpError::InvalidResponse)?;
        response.status_text = parts.get(2).unwrap_or(&"").to_string();

        // BaÅŸlÄ±k alanlarÄ±nÄ± ayrÄ±ÅŸtÄ±r: "Content-Type: text/html"
        for line in lines {
            if line.is_empty() {
                continue;
            }

            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim();
                let value = line[idx + 1..].trim();
                response.headers.insert(key, value);
            }
        }

        Ok(())
    }

    /// Chunked transfer encoding ile kodlanmÄ±ÅŸ gÃ¶vdeyi alÄ±r.
    ///
    /// Her chunk ÅŸu formattadÄ±r:
    /// ```text
    /// <boyut (hex)>\r\n
    /// <veri (boyut byte)>\r\n
    /// ...
    /// 0\r\n          <- Son chunk (boyut = 0 demek bitiÅŸ)
    /// \r\n
    /// ```
    fn receive_chunked_body(
        &self,
        sock_id: u32,
        response: &mut HttpResponse,
    ) -> Result<(), HttpError> {
        loop {
            // Chunk boyutunu oku (hex, \r\n ile biter)
            let mut size_buf = String::new();
            loop {
                let mut byte = [0u8; 1];
                let n = recv(sock_id, &mut byte, 0)?;
                if n == 0 {
                    return Err(HttpError::ChunkedEncoding);
                }

                if byte[0] == b'\n' {
                    break; // SatÄ±r sonu (\n)
                }

                if byte[0] != b'\r' {
                    size_buf.push(byte[0] as char); // hex rakamÄ±nÄ± topla
                }
            }

            // Hex boyutu ayrÄ±ÅŸtÄ±r
            let chunk_size = usize::from_str_radix(size_buf.trim(), 16)
                .map_err(|_| HttpError::ChunkedEncoding)?;

            if chunk_size == 0 {
                // Son chunk: transfer tamamlandÄ±
                break;
            }

            // Chunk verisini oku (tam olarak chunk_size byte)
            let mut remaining = chunk_size;
            while remaining > 0 {
                let mut chunk = vec![0u8; remaining];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    return Err(HttpError::ChunkedEncoding);
                }
                response.body.extend_from_slice(&chunk[..n]);
                remaining -= n;
            }

            // Her chunk'Ä±n sonundaki \r\n'yi tÃ¼ket
            let mut trailer = [0u8; 2];
            recv(sock_id, &mut trailer, 0)?;
        }

        Ok(())
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YARDIMCI FONKSÄ°YONLAR
// ============================================================================

/// HTTP baÅŸlÄ±klarÄ±nÄ±n sonundaki CRLFCRLF (\r\n\r\n) iÅŸaretÃ§isini bulur.
///
/// HTTP/1.1'de baÅŸlÄ±klar ve gÃ¶vde \r\n\r\n ile ayrÄ±lÄ±r.
/// DÃ¶nen deÄŸer: baÅŸlÄ±klarÄ±n bittiÄŸi konum (CRLFCRLF'den Ã¶nceki byte'Ä±n indisi).
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
        {
            return Some(i);
        }
    }
    None
}

fn decode_chunked_body(data: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut cursor = 0usize;
    let mut body = Vec::new();
    while cursor < data.len() {
        let mut line_end = None;
        let search_limit = data.len().saturating_sub(1);
        for i in cursor..search_limit {
            if data[i] == b'\r' && data[i + 1] == b'\n' {
                line_end = Some(i);
                break;
            }
        }
        let Some(line_end) = line_end else {
            return Err(HttpError::ChunkedEncoding);
        };
        let size_str = core::str::from_utf8(&data[cursor..line_end])
            .map_err(|_| HttpError::ChunkedEncoding)?;
        let chunk_size =
            usize::from_str_radix(size_str.trim(), 16).map_err(|_| HttpError::ChunkedEncoding)?;
        cursor = line_end + 2;
        if chunk_size == 0 {
            break;
        }
        let chunk_end = cursor.saturating_add(chunk_size);
        if chunk_end > data.len() {
            return Err(HttpError::ChunkedEncoding);
        }
        body.extend_from_slice(&data[cursor..chunk_end]);
        cursor = chunk_end;
        if cursor + 1 >= data.len() || data[cursor] != b'\r' || data[cursor + 1] != b'\n' {
            return Err(HttpError::ChunkedEncoding);
        }
        cursor += 2;
    }
    Ok(body)
}

fn tls_strip_records(data: &[u8]) -> Vec<u8> {
    let mut cursor = 0usize;
    let mut plaintext = Vec::new();
    while cursor + 5 <= data.len() {
        let record_len = u16::from_be_bytes([data[cursor + 3], data[cursor + 4]]) as usize;
        let record_end = cursor.saturating_add(5).saturating_add(record_len);
        if record_end > data.len() {
            break;
        }
        plaintext.extend_from_slice(&data[cursor + 5..record_end]);
        cursor = record_end;
    }
    if plaintext.is_empty() && data.len() > 5 {
        plaintext.extend_from_slice(&data[5..]);
    }
    plaintext
}

fn ensure_x509_roots() {
    if X509_ROOTS_READY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        init_builtin_roots();
    }
}

fn process_tls_server_handshake_flight(
    tls: &mut TlsClient,
    handshake_bytes: &[u8],
    hostname: &str,
) -> Result<(), HttpError> {
    let mut offset = 0usize;
    let mut saw_finished = false;

    while offset + 4 <= handshake_bytes.len() {
        let msg_len = ((handshake_bytes[offset + 1] as usize) << 16)
            | ((handshake_bytes[offset + 2] as usize) << 8)
            | (handshake_bytes[offset + 3] as usize);
        let msg_end = offset + 4 + msg_len;
        if msg_end > handshake_bytes.len() {
            break;
        }

        let msg = &handshake_bytes[offset..msg_end];
        match handshake_bytes[offset] {
            8 => tls
                .process_encrypted_extensions(msg)
                .map_err(|_| HttpError::TlsHandshakeFailed)?,
            11 => {
                tls.process_certificate(msg)
                    .map_err(|_| HttpError::TlsHandshakeFailed)?;
                verify_tls_server_certificate(&msg[4..], hostname)?;
            }
            15 => tls
                .process_certificate_verify(msg)
                .map_err(|_| HttpError::TlsHandshakeFailed)?,
            20 => {
                tls.process_finished(msg)
                    .map_err(|_| HttpError::TlsHandshakeFailed)?;
                saw_finished = true;
            }
            _ => {}
        }

        offset = msg_end;
    }

    if saw_finished {
        Ok(())
    } else {
        Err(HttpError::TlsHandshakeFailed)
    }
}

fn verify_tls_server_certificate(
    cert_message_body: &[u8],
    hostname: &str,
) -> Result<(), HttpError> {
    ensure_x509_roots();

    if cert_message_body.is_empty() {
        return Err(HttpError::TlsDecodeFailed);
    }

    let request_context_len = cert_message_body[0] as usize;
    let list_offset = 1 + request_context_len;
    if list_offset + 3 > cert_message_body.len() {
        return Err(HttpError::TlsDecodeFailed);
    }

    let cert_list_len = ((cert_message_body[list_offset] as usize) << 16)
        | ((cert_message_body[list_offset + 1] as usize) << 8)
        | (cert_message_body[list_offset + 2] as usize);
    let list_start = list_offset + 3;
    let list_end = list_start
        .saturating_add(cert_list_len)
        .min(cert_message_body.len());
    if list_end <= list_start {
        return Err(HttpError::TlsDecodeFailed);
    }

    let certs = parse_tls13_certificate_entries(&cert_message_body[list_start..list_end]);
    if certs.is_empty() {
        return Err(HttpError::TlsDecodeFailed);
    }

    let verifier = CertVerifier::new();
    verifier.verify_chain(&certs).map_err(map_cert_error)?;

    if certs.len() >= 2 {
        let mut checker = crate::net::x509::RevocationChecker::new();
        if !certs[0].ocsp_responder_urls().is_empty()
            || !certs[0].crl_distribution_urls().is_empty()
        {
            checker.hard_fail = true;
        }
        checker
            .check_revocation(&certs[0], &certs[1])
            .map_err(map_cert_error)?;
    }

    if verify_hostname(&certs[0], hostname) {
        Ok(())
    } else {
        Err(HttpError::TlsCertCnInvalid)
    }
}

fn parse_tls13_certificate_entries(cert_list: &[u8]) -> Vec<crate::net::x509::X509Certificate> {
    let mut certs = Vec::new();
    let mut pos = 0usize;

    while pos + 3 <= cert_list.len() {
        let cert_len = ((cert_list[pos] as usize) << 16)
            | ((cert_list[pos + 1] as usize) << 8)
            | (cert_list[pos + 2] as usize);
        pos += 3;

        if pos + cert_len > cert_list.len() {
            break;
        }

        certs.extend(parse_certificate_chain(&cert_list[pos..pos + cert_len]));
        pos += cert_len;

        if pos + 2 > cert_list.len() {
            break;
        }

        let ext_len = u16::from_be_bytes([cert_list[pos], cert_list[pos + 1]]) as usize;
        pos += 2;
        if pos + ext_len > cert_list.len() {
            break;
        }
        pos += ext_len;
    }

    certs
}

// ============================================================================
// KOLAYLIK FONKSÄ°YONLARI
// ============================================================================

/// Tek satÄ±rda HTTP GET isteÄŸi gÃ¶nderir.
///
/// KullanÄ±m: `http_get("http://example.com/api/data")?`
pub fn http_get(url: &str) -> Result<HttpResponse, HttpError> {
    HttpClient::new().get(url)
}

/// Tek satÄ±rda HTTP POST isteÄŸi gÃ¶nderir.
///
/// KullanÄ±m: `http_post("http://example.com/api", b"{\"key\":\"val\"}")?`
pub fn http_post(url: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
    HttpClient::new().post(url, body, Some("application/octet-stream"))
}

/// Tek satÄ±rda dosya indirir.
///
/// URL'deki kaynaÄŸÄ± indirip ham byte olarak dÃ¶ner.
/// KullanÄ±m: `let data = http_download("http://example.com/file.bin")?`
pub fn http_download(url: &str) -> Result<Vec<u8>, HttpError> {
    HttpClient::new().download(url)
}

fn build_request_target(url: &HttpUrl, absolute_form: bool) -> String {
    if absolute_form {
        return url.to_url_string();
    }

    let mut path_query = url.path.clone();
    if !url.query.is_empty() {
        path_query.push('?');
        path_query.push_str(&url.query);
    }
    path_query
}

fn extract_proxy_authorization(request: &[u8]) -> Option<String> {
    let request_text = core::str::from_utf8(request).ok()?;
    for line in request_text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Proxy-Authorization") {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cert_error_mapping_preserves_tls_failure_class() {
        assert_eq!(
            map_cert_error(CertError::Expired),
            HttpError::TlsCertDateInvalid
        );
        assert_eq!(
            map_cert_error(CertError::NotYetValid),
            HttpError::TlsCertDateInvalid
        );
        assert_eq!(
            map_cert_error(CertError::UnknownIssuer),
            HttpError::TlsInvalidCa
        );
        assert_eq!(
            map_cert_error(CertError::SelfSigned),
            HttpError::TlsInvalidCa
        );
        assert_eq!(
            map_cert_error(CertError::Revoked),
            HttpError::TlsCertRevoked
        );
        assert_eq!(
            map_cert_error(CertError::InvalidSignature),
            HttpError::TlsInvalidCertificate
        );
        assert_eq!(
            map_cert_error(CertError::InvalidChain),
            HttpError::TlsInvalidCertificate
        );
    }

    #[test]
    fn malformed_tls_certificate_message_reports_decode_failure() {
        assert_eq!(
            verify_tls_server_certificate(&[], "example.com"),
            Err(HttpError::TlsDecodeFailed)
        );
        assert_eq!(
            verify_tls_server_certificate(&[0x00, 0x00], "example.com"),
            Err(HttpError::TlsDecodeFailed)
        );
    }

    #[test]
    fn build_request_target_switches_between_origin_and_absolute_form() {
        let url = HttpUrl::parse("http://example.com:8080/api/state?q=1").expect("url");
        assert_eq!(build_request_target(&url, false), "/api/state?q=1");
        assert_eq!(
            build_request_target(&url, true),
            "http://example.com:8080/api/state?q=1"
        );
    }

    #[test]
    fn extract_proxy_authorization_reads_header_value() {
        let raw = b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n\r\n";
        assert_eq!(
            extract_proxy_authorization(raw),
            Some("Basic dXNlcjpwYXNz".to_string())
        );
    }
}
