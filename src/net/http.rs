//! # echOS HTTP İstemcisi
//!
//! HTTP/1.1 protokolünü uygulayan istemci. GET, POST, indirme ve
//! yönlendirme takip özelliklerini destekler.
//!
//! ## HTTP/1.1 Protokolü Genel Bakış
//!
//! HTTP (HyperText Transfer Protocol), istemci-sunucu mimarisinde
//! çalışan bir uygulama katmanı protokolüdür. TCP üzerinde taşınır.
//!
//! ## HTTP İstek/Yanıt Döngüsü
//!
//! ```text
//! Tarayıcı/İstemci                    Web Sunucusu (Port 80)
//!      |                                      |
//!      |--- TCP SYN ---------------------->   |
//!      |<-- TCP SYN-ACK ------------------- --|
//!      |--- TCP ACK ---------------------->   |  [TCP bağlantısı kuruldu]
//!      |                                      |
//!      |--- HTTP İstek (Request) ---------->  |
//!      |  GET /index.html HTTP/1.1             |
//!      |  Host: www.example.com               |
//!      |  User-Agent: echOS/1.0               |
//!      |  \r\n\r\n                            |
//!      |                                      |
//!      |<-- HTTP Yanıt (Response) ----------- |
//!      |  HTTP/1.1 200 OK                     |
//!      |  Content-Type: text/html             |
//!      |  Content-Length: 1234                |
//!      |  \r\n\r\n                            |
//!      |  [HTML içeriği]                      |
//! ```
//!
//! ## HTTP İstek Yapısı
//!
//! ```text
//! [Durum Satırı]   GET /path?query HTTP/1.1\r\n
//! [Başlıklar]      Host: example.com\r\n
//!                  User-Agent: echOS/1.0\r\n
//!                  Accept: */*\r\n
//!                  Connection: close\r\n
//!                  \r\n                    <-- Boş satır: başlık sonu
//! [Veri (isteğe bağlı)]  [POST/PUT için istek gövdesi]
//! ```
//!
//! ## HTTP Yanıt Yapısı
//!
//! ```text
//! [Durum Satırı]   HTTP/1.1 200 OK\r\n
//! [Başlıklar]      Content-Type: text/html\r\n
//!                  Content-Length: 1234\r\n
//!                  \r\n                    <-- Boş satır (CRLFCRLF): başlık sonu
//! [Gövde]          [HTML/JSON/binary vb. veri]
//!
//! Durum kodu aralıkları:
//!   1xx = Bilgi (Informational)
//!   2xx = Başarı (GET: 200 OK, POST: 201 Created)
//!   3xx = Yönlendirme (301 Moved Permanently, 302 Found)
//!   4xx = İstemci Hatası (400 Bad Request, 404 Not Found)
//!   5xx = Sunucu Hatası (500 Internal Server Error)
//! ```
//!
//! ## Chunked Transfer Encoding
//!
//! ```text
//! İçerik uzunluğu bilinmediğinde (dinamik içerik) kullanılır:
//!
//! HTTP/1.1 200 OK
//! Transfer-Encoding: chunked
//! \r\n
//! 1A\r\n            <- Chunk boyutu: 0x1A = 26 decimal (hex)
//! Body data here...\r\n   <- 26 byte veri
//! 10\r\n            <- Chunk boyutu: 0x10 = 16
//! More data here\r\n       <- 16 byte veri
//! 0\r\n             <- Son chunk: 0 boyut = bitiş sinyali
//! \r\n
//! ```

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::borrow::ToOwned;
use spin::Mutex;

use super::{NetError, Ipv4Addr, Port};
use super::socket::{SocketAddr, SocketType, AddressFamily, Protocol};
use super::socket::{socket as socket_create, connect, send, recv, close};

// ============================================================================
// HTTP SABİTLERİ
// ============================================================================

/// HTTP standart portu (şifresiz)
const HTTP_PORT: u16 = 80;
/// HTTPS standart portu (TLS şifreli)
const HTTPS_PORT: u16 = 443;
/// HTTP yanıt başlıkları için maksimum tampon boyutu (8 KiB)
const MAX_HEADER_SIZE: usize = 8192;
/// Sonsuz yönlendirme döngüsünü önlemek için maksimum yönlendirme sayısı
const MAX_REDIRECTS: u8 = 5;
/// Alım tamponu boyutu (her recv çağrısında en fazla bu kadar byte okunur)
const RECV_BUF_SIZE: usize = 4096;
/// Varsayılan bağlantı zaman aşımı (30 saniye)
const DEFAULT_TIMEOUT_MS: u64 = 30000;

// ============================================================================
// HTTP HATASI
// ============================================================================

/// HTTP istemci hata türleri.
///
/// Ağ hatalarından uygulama düzeyindeki hatalara kadar tüm hata durumlarını kapsar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    Network(NetError),  // Ağ katmanı hatası (TCP bağlantı kesilmesi vb.)
    InvalidUrl,         // URL ayrıştırılamadı (şema veya host eksik)
    InvalidResponse,    // Yanıt geçerli HTTP formatında değil
    ConnectionFailed,   // DNS çözümleme veya TCP bağlantısı başarısız
    Timeout,            // Bağlantı veya veri alım zaman aşımı
    TooManyRedirects,   // MAX_REDIRECTS sınırı aşıldı
    NotFound,           // HTTP 404 Not Found
    ServerError,        // HTTP 5xx Sunucu Hatası
    InvalidHeader,      // Başlık UTF-8 geçersiz veya format hatası
    ChunkedEncoding,    // Chunked transfer encoding ayrıştırma hatası
    ContentLength,      // Content-Length başlığı geçersiz veya eksik
    TlsNotSupported,    // HTTPS/TLS henüz desteklenmiyor (TODO)
}

impl From<NetError> for HttpError {
    fn from(err: NetError) -> Self {
        HttpError::Network(err)
    }
}

// ============================================================================
// HTTP METODU
// ============================================================================

/// HTTP istek metodları.
///
/// ```text
/// GET    : Kaynak al (yan etkisi yok, önbelleklenebilir)
/// POST   : Yeni kaynak oluştur veya işlem başlat
/// PUT    : Kaynağı tamamen güncelle (idempotent)
/// DELETE : Kaynağı sil (idempotent)
/// HEAD   : Sadece başlıkları al (GET gibi ama gövde yok)
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
    /// HTTP metodunun adını döner (örn. "GET", "POST").
    ///
    /// İstek satırında kullanılır: "GET /path HTTP/1.1"
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
// HTTP BAŞLIKLARI
// ============================================================================

/// HTTP başlık koleksiyonu.
///
/// BTreeMap kullanılır (no_std uyumlu, alfabetik sıralı).
/// Tüm başlık anahtarları küçük harfe normalleştirilir (HTTP/1.1 büyük-küçük harf duyarsız).
#[derive(Clone, Debug)]
pub struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

impl HttpHeaders {
    /// Varsayılan başlıklar ile yeni bir koleksiyon oluşturur.
    ///
    /// Her HTTP isteğine otomatik eklenen başlıklar:
    /// - User-Agent: echOS/1.0 (istemci kimliği)
    /// - Accept: */* (her türlü içerik kabul edilir)
    /// - Connection: close (her istekten sonra bağlantıyı kapat)
    pub fn new() -> Self {
        let mut headers = HttpHeaders {
            headers: BTreeMap::new(),
        };

        // Varsayılan başlıklar
        headers.insert("User-Agent", "echOS/1.0");
        headers.insert("Accept", "*/*");
        headers.insert("Connection", "close");

        headers
    }

    /// Başlık ekler veya günceller.
    ///
    /// Anahtar küçük harfe dönüştürülür (HTTP başlıkları büyük-küçük harf duyarsız).
    pub fn insert(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_string().to_lowercase(), value.to_string());
    }

    /// Belirtilen başlığın değerini döner.
    ///
    /// Arama büyük-küçük harf duyarsızdır: "Content-Type" ve "content-type" aynıdır.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    /// Belirtilen başlığı kaldırır.
    pub fn remove(&mut self, key: &str) {
        self.headers.remove(&key.to_lowercase());
    }

    /// Tüm başlıkları HTTP formatında metin olarak döner.
    ///
    /// Her başlık "Anahtar: Değer\r\n" formatında çıktılanır.
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

/// Ayrıştırılmış HTTP URL yapısı.
///
/// ```text
/// URL Bileşenleri:
///   https://user:pass@www.example.com:8080/path?query=val#fragment
///   ^^^^^                                                           şema (scheme)
///                      ^^^^^^^^^^^^^^^^^^^                          host
///                                         ^^^^                      port
///                                              ^^^^^                path
///                                                    ^^^^^^^^^^^    query
///                                                               ^^^^^^^^ fragment
/// ```
#[derive(Clone, Debug)]
pub struct HttpUrl {
    pub scheme: String,   // Protokol: "http" veya "https"
    pub host: String,     // Sunucu adı veya IP: "www.example.com"
    pub port: u16,        // Port: 80 (http) veya 443 (https) varsayılan
    pub path: String,     // Kaynak yolu: "/api/v1/data" (varsayılan "/")
    pub query: String,    // Sorgu parametre: "key=val&foo=bar" (? işareti dahil değil)
    pub fragment: String, // Parça tanımlayıcı: "section1" (# işareti dahil değil)
}

impl HttpUrl {
    /// URL metnini ayrıştırarak HttpUrl yapısına dönüştürür.
    ///
    /// Desteklenen formatlar:
    /// - `http://example.com/path`
    /// - `https://example.com:8443/path?query#fragment`
    /// - `//example.com/path` (şema olmadan, varsayılan http)
    pub fn parse(url: &str) -> Result<Self, HttpError> {
        // Basit URL ayrıştırıcı
        // Format: scheme://host[:port][/path][?query][#fragment]

        let mut scheme = String::new();
        let mut host = String::new();
        let mut port = 0u16;
        let mut path = String::from("/");
        let mut query = String::new();
        let mut fragment = String::new();

        // Şemayı ayrıştır (://'dan önceki kısım)
        let rest = if let Some(idx) = url.find("://") {
            scheme = url[..idx].to_string();
            &url[idx + 3..]
        } else {
            // Şema belirtilmemiş, varsayılan http
            scheme = String::from("http");
            url
        };

        // Şemaya göre varsayılan port belirle
        port = if scheme == "https" { HTTPS_PORT } else { HTTP_PORT };

        // Host ve port ayrıştır (ilk / karakterine kadar)
        let path_start = rest.find('/').unwrap_or(rest.len());
        let host_port = &rest[..path_start];

        if let Some(idx) = host_port.find(':') {
            host = host_port[..idx].to_string();
            if let Ok(p) = host_port[idx + 1..].parse::<u16>() {
                port = p; // Özel port numarası
            }
        } else {
            host = host_port.to_string();
        }

        // Path, query ve fragment ayrıştır
        if path_start < rest.len() {
            let path_rest = &rest[path_start..];

            // Fragment bölümünü ayır (# ile başlar)
            let path_query = if let Some(idx) = path_rest.find('#') {
                fragment = path_rest[idx + 1..].to_string();
                &path_rest[..idx]
            } else {
                path_rest
            };

            // Query bölümünü ayır (? ile başlar)
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

    /// URL'yi tam metin olarak döner.
    ///
    /// Standart portlar (80/443) URL'ye eklenmez.
    pub fn to_url_string(&self) -> String {
        let mut result = String::new();
        result.push_str(&self.scheme);
        result.push_str("://");
        result.push_str(&self.host);

        // Standart olmayan portlar URL'de gösterilir
        if (self.scheme == "http" && self.port != HTTP_PORT) ||
           (self.scheme == "https" && self.port != HTTPS_PORT) {
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

    /// Bu URL'nin HTTPS şeması kullanıp kullanmadığını kontrol eder.
    ///
    /// HTTPS ise TLS gerektirir. Mevcut implementasyonda TLS desteklenmez.
    pub fn is_https(&self) -> bool {
        self.scheme == "https"
    }
}

// ============================================================================
// HTTP YANITI
// ============================================================================

/// HTTP yanıt yapısı.
///
/// Sunucudan alınan durum kodu, başlıklar ve gövde verisini içerir.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status_code: u16,    // HTTP durum kodu (200=OK, 404=Not Found, 500=Server Error)
    pub status_text: String, // Durum metni ("OK", "Not Found" vb.)
    pub headers: HttpHeaders,// Yanıt başlıkları (Content-Type, Content-Length vb.)
    pub body: Vec<u8>,       // Yanıt gövdesi (HTML, JSON, binary vb.)
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

    /// Yanıtın başarılı (2xx) olup olmadığını kontrol eder.
    ///
    /// 200-299 arası durum kodları başarı anlamına gelir.
    pub fn is_success(&self) -> bool {
        self.status_code >= 200 && self.status_code < 300
    }

    /// Yanıtın yönlendirme (3xx) içerip içermediğini kontrol eder.
    ///
    /// Yönlendirmede `Location` başlığından yeni URL alınmalıdır.
    pub fn is_redirect(&self) -> bool {
        self.status_code >= 300 && self.status_code < 400
    }

    /// Yanıtın istemci hatası (4xx) içerip içermediğini kontrol eder.
    ///
    /// 404 Not Found, 401 Unauthorized, 403 Forbidden vb.
    pub fn is_client_error(&self) -> bool {
        self.status_code >= 400 && self.status_code < 500
    }

    /// Yanıtın sunucu hatası (5xx) içerip içermediğini kontrol eder.
    ///
    /// 500 Internal Server Error, 503 Service Unavailable vb.
    pub fn is_server_error(&self) -> bool {
        self.status_code >= 500 && self.status_code < 600
    }

    /// Yanıt gövdesini UTF-8 metin olarak döner.
    ///
    /// Geçersiz byte'lar için '?' karakteri kullanılır (kayıplı dönüştürme).
    pub fn body_as_string(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// `Content-Length` başlığından içerik uzunluğunu okur.
    ///
    /// Başlık eksikse veya ayrıştırılamazsa `None` döner.
    pub fn content_length(&self) -> Option<usize> {
        self.headers.get("content-length")
            .and_then(|s| s.parse::<usize>().ok())
    }

    /// `Transfer-Encoding: chunked` başlığının olup olmadığını kontrol eder.
    ///
    /// Chunked encoding: Yanıt gövdesi parçalar halinde gelir.
    /// Her parça önce hex boyutunu, ardından veriyi içerir.
    pub fn is_chunked(&self) -> bool {
        self.headers.get("transfer-encoding")
            .map(|s| s.to_lowercase() == "chunked")
            .unwrap_or(false)
    }

    /// Yönlendirme URL'sini `Location` başlığından okur.
    ///
    /// Yalnızca 3xx yanıtlarda anlamlıdır.
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
// HTTP İSTEMCİSİ
// ============================================================================

/// HTTP/1.1 istemcisi.
///
/// DNS çözümleme, TCP bağlantısı, istek gönderme ve yanıt ayrıştırmayı
/// birleştirir. Otomatik yönlendirme takibi desteklenir.
pub struct HttpClient {
    timeout_ms: u64,       // Bağlantı ve alım zaman aşımı (ms)
    max_redirects: u8,     // Maksimum otomatik yönlendirme sayısı
    follow_redirects: bool,// Otomatik yönlendirme takip edilsin mi?
}

impl HttpClient {
    pub fn new() -> Self {
        HttpClient {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_redirects: MAX_REDIRECTS,
            follow_redirects: true,
        }
    }

    /// Bağlantı zaman aşımını milisaniye cinsinden ayarlar.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Otomatik yönlendirme takibini etkinleştirir veya devre dışı bırakır.
    pub fn set_follow_redirects(&mut self, follow: bool) {
        self.follow_redirects = follow;
    }

    /// HTTP GET isteği gönderir.
    ///
    /// Sunucudan kaynak al. Yan etkisi yok, önbelleklenebilir.
    /// DNS çözümleme -> TCP bağlantı -> İstek -> Yanıt ayrıştırma.
    pub fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::GET, url, None, None)
    }

    /// HTTP POST isteği gönderir.
    ///
    /// Sunucuda yeni kaynak oluştur veya işlem başlat.
    /// `body`: İstek gövdesi (form verisi, JSON vb.)
    /// `content_type`: İçerik türü ("application/json", "application/x-www-form-urlencoded" vb.)
    pub fn post(&self, url: &str, body: &[u8], content_type: Option<&str>) -> Result<HttpResponse, HttpError> {
        self.request(HttpMethod::POST, url, Some(body), content_type)
    }

    /// Genel HTTP isteği gönderir.
    ///
    /// Tüm HTTP metodları için temel uygulama. Şu işlemleri yapar:
    /// 1. URL ayrıştır
    /// 2. HTTPS kontrolü (desteklenmez)
    /// 3. DNS ile hostname'i IP'ye çevir
    /// 4. TCP soketi oluştur ve bağlan
    /// 5. HTTP isteğini hazırla ve gönder
    /// 6. Yanıtı al ve ayrıştır
    /// 7. Yönlendirme varsa tekrarla
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
            // HTTPS henüz desteklenmiyor (TLS implementasyonu gerekli)
            if current_url.is_https() {
                return Err(HttpError::TlsNotSupported);
            }

            // DNS ile hostname'i IP adresine çevir
            let dns_server = super::get_config().dns_servers.first()
                .copied()
                .unwrap_or([8, 8, 8, 8]);
            let dns_ip = Ipv4Addr::from_bytes(dns_server);
            let ip = super::dns::resolve(&current_url.host, dns_ip)
                .map_err(|_| HttpError::ConnectionFailed)?;

            // TCP soketi oluştur (STREAM = bağlantı yönelimli)
            let sock_id = socket_create(
                AddressFamily::IPV4,
                SocketType::STREAM,
                Protocol::TCP,
            )?;

            // Web sunucusuna bağlan (genellikle port 80)
            let addr = SocketAddr::new(ip, Port(current_url.port));
            connect(sock_id, addr)?;

            // HTTP istek metnini oluştur
            let request = self.build_request(method, &current_url, body, content_type);

            // İsteği gönder
            send(sock_id, request.as_bytes(), 0)?;

            // Yanıtı al ve ayrıştır
            let response = self.receive_response(sock_id)?;

            // Soketi kapat (Connection: close olduğu için bağlantı zaten kapatılacak)
            let _ = close(sock_id);

            // Yönlendirme mi?
            if response.is_redirect() && self.follow_redirects {
                redirect_count += 1;
                if redirect_count > self.max_redirects {
                    return Err(HttpError::TooManyRedirects);
                }

                if let Some(location) = response.location() {
                    // Göreli URL desteği
                    if location.starts_with('/') {
                        current_url.path = location.to_string(); // Mutlak path
                    } else if location.starts_with("http://") || location.starts_with("https://") {
                        current_url = HttpUrl::parse(location)?; // Tam URL
                    } else {
                        // Göreli URL: mevcut path'e göre yorumla
                        current_url.path = location.to_string();
                    }
                    continue; // Yeni URL ile yeniden dene
                }
            }

            return Ok(response);
        }
    }

    /// Belirtilen URL'den dosya indirir.
    ///
    /// GET isteği gönderir ve yanıt gövdesini döner.
    /// 404 için NotFound, diğer hatalı durum kodları için ServerError döner.
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

    /// HTTP istek metnini oluşturur.
    ///
    /// RFC 7230'a uygun HTTP/1.1 formatında:
    /// ```text
    /// METHOD /path?query HTTP/1.1\r\n
    /// Host: example.com\r\n
    /// Content-Length: <n>\r\n    (POST/PUT için)
    /// Content-Type: <type>\r\n   (POST/PUT için)
    /// User-Agent: echOS/1.0\r\n
    /// Accept: */*\r\n
    /// Connection: close\r\n
    /// \r\n
    /// [gövde verisi]
    /// ```
    fn build_request(
        &self,
        method: HttpMethod,
        url: &HttpUrl,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> String {
        let mut request = String::new();

        // İstek satırı: METHOD /path?query HTTP/1.1
        let mut path_query = url.path.clone();
        if !url.query.is_empty() {
            path_query.push('?');
            path_query.push_str(&url.query);
        }

        request.push_str(method.as_str());
        request.push(' ');
        request.push_str(&path_query);
        request.push_str(" HTTP/1.1\r\n");

        // Host başlığı: sanal hosting için zorunlu (HTTP/1.1)
        request.push_str("Host: ");
        request.push_str(&url.host);
        if url.port != HTTP_PORT && url.port != HTTPS_PORT {
            request.push(':');
            request.push_str(&url.port.to_string());
        }
        request.push_str("\r\n");

        // POST/PUT için içerik başlıkları
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

        // Genel başlıklar
        request.push_str("User-Agent: echOS/1.0\r\n");
        request.push_str("Accept: */*\r\n");
        request.push_str("Connection: close\r\n");

        // Boş satır: başlıkların sonu (CRLFCRLF)
        request.push_str("\r\n");

        // İstek gövdesi (sadece POST/PUT gibi metodlar için)
        if let Some(data) = body {
            // İçerik metin ise doğrudan ekle
            // Gerçek implementasyonda bytes doğruca yazılmalı
            let body_str = core::str::from_utf8(data).unwrap_or("");
            request.push_str(body_str);
        }

        request
    }

    /// HTTP yanıtını soket üzerinden alır ve ayrıştırır.
    ///
    /// Aşamalar:
    /// 1. Başlıkları al (CRLFCRLF'e kadar)
    /// 2. Durum satırı ve başlık alanlarını ayrıştır
    /// 3. Gövdeyi al:
    ///    - Chunked: chunk-by-chunk oku
    ///    - Content-Length: tam uzunluk oku
    ///    - Bağlantı kapanana dek: sonsuz oku
    fn receive_response(&self, sock_id: u32) -> Result<HttpResponse, HttpError> {
        let mut response = HttpResponse::new();
        let mut header_buf = vec![0u8; MAX_HEADER_SIZE];
        let mut header_len = 0;

        // Başlıkları al (CRLFCRLF = \r\n\r\n sinyaline kadar)
        loop {
            let mut chunk = vec![0u8; RECV_BUF_SIZE];
            let n = recv(sock_id, &mut chunk, 0)?;

            if n == 0 {
                break; // Bağlantı kapandı
            }

            // Başlık tamponuna kopyala
            let copy_len = core::cmp::min(n, MAX_HEADER_SIZE - header_len);
            header_buf[header_len..header_len + copy_len].copy_from_slice(&chunk[..copy_len]);
            header_len += copy_len;

            // Başlık sonu işaretçisi bulundu mu? (\r\n\r\n)
            let header_end = find_header_end(&header_buf[..header_len]);
            if header_end.is_some() {
                break;
            }
        }

        // Başlık sonu konumunu bul (zorunlu)
        let header_end = find_header_end(&header_buf[..header_len])
            .ok_or(HttpError::InvalidResponse)?;

        let header_str = core::str::from_utf8(&header_buf[..header_end])
            .map_err(|_| HttpError::InvalidHeader)?;

        // Başlıkları ayrıştır (durum satırı + başlık alanları)
        self.parse_response_headers(header_str, &mut response)?;

        // Gövde tampondan başlangıç konumu: header_end + 4 (\r\n\r\n = 4 byte)
        let body_start = header_end + 4;

        if response.is_chunked() {
            // Chunked transfer encoding: hex boyut + veri bloklarını oku
            self.receive_chunked_body(sock_id, &mut response)?;
        } else if let Some(content_len) = response.content_length() {
            // Content-Length bilinç: tam olarak bu kadar byte oku

            // Başlık tamponunda zaten gelen gövde verisi varsa kopyala
            let initial_body_len = header_len - body_start;
            if initial_body_len > 0 {
                response.body.extend_from_slice(&header_buf[body_start..header_len]);
            }

            // Kalan gövdeyi oku
            while response.body.len() < content_len {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break; // Bağlantı kapandı
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        } else {
            // Content-Length yok: bağlantı kapanana dek oku
            response.body.extend_from_slice(&header_buf[body_start..header_len]);

            loop {
                let mut chunk = vec![0u8; RECV_BUF_SIZE];
                let n = recv(sock_id, &mut chunk, 0)?;
                if n == 0 {
                    break; // Bağlantı kapandı, gövde tamamlandı
                }
                response.body.extend_from_slice(&chunk[..n]);
            }
        }

        Ok(response)
    }

    /// HTTP yanıt başlıklarını ayrıştırır.
    ///
    /// İlk satır: "HTTP/1.1 200 OK" (durum satırı)
    /// Sonraki satırlar: "Anahtar: Değer" formatında başlık alanları
    fn parse_response_headers(&self, header_str: &str, response: &mut HttpResponse) -> Result<(), HttpError> {
        let mut lines = header_str.lines();

        // Durum satırı: "HTTP/1.1 200 OK"
        let status_line = lines.next().ok_or(HttpError::InvalidResponse)?;

        // "HTTP/1.1", "200", "OK" olarak ayır (en fazla 3 parça)
        let parts: Vec<&str> = status_line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return Err(HttpError::InvalidResponse);
        }

        response.status_code = parts[1].parse()
            .map_err(|_| HttpError::InvalidResponse)?;
        response.status_text = parts.get(2).unwrap_or(&"").to_string();

        // Başlık alanlarını ayrıştır: "Content-Type: text/html"
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

    /// Chunked transfer encoding ile kodlanmış gövdeyi alır.
    ///
    /// Her chunk şu formattadır:
    /// ```text
    /// <boyut (hex)>\r\n
    /// <veri (boyut byte)>\r\n
    /// ...
    /// 0\r\n          <- Son chunk (boyut = 0 demek bitiş)
    /// \r\n
    /// ```
    fn receive_chunked_body(&self, sock_id: u32, response: &mut HttpResponse) -> Result<(), HttpError> {
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
                    break; // Satır sonu (\n)
                }

                if byte[0] != b'\r' {
                    size_buf.push(byte[0] as char); // hex rakamını topla
                }
            }

            // Hex boyutu ayrıştır
            let chunk_size = usize::from_str_radix(size_buf.trim(), 16)
                .map_err(|_| HttpError::ChunkedEncoding)?;

            if chunk_size == 0 {
                // Son chunk: transfer tamamlandı
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

            // Her chunk'ın sonundaki \r\n'yi tüket
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
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// HTTP başlıklarının sonundaki CRLFCRLF (\r\n\r\n) işaretçisini bulur.
///
/// HTTP/1.1'de başlıklar ve gövde \r\n\r\n ile ayrılır.
/// Dönen değer: başlıkların bittiği konum (CRLFCRLF'den önceki byte'ın indisi).
fn find_header_end(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// KOLAYLIK FONKSİYONLARI
// ============================================================================

/// Tek satırda HTTP GET isteği gönderir.
///
/// Kullanım: `http_get("http://example.com/api/data")?`
pub fn http_get(url: &str) -> Result<HttpResponse, HttpError> {
    HttpClient::new().get(url)
}

/// Tek satırda HTTP POST isteği gönderir.
///
/// Kullanım: `http_post("http://example.com/api", b"{\"key\":\"val\"}")?`
pub fn http_post(url: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
    HttpClient::new().post(url, body, Some("application/octet-stream"))
}

/// Tek satırda dosya indirir.
///
/// URL'deki kaynağı indirip ham byte olarak döner.
/// Kullanım: `let data = http_download("http://example.com/file.bin")?`
pub fn http_download(url: &str) -> Result<Vec<u8>, HttpError> {
    HttpClient::new().download(url)
}
