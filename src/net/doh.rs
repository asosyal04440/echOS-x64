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
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use super::dns::DnsHeader;
use super::dns::DnsRecordType;
use super::http2::{connection_preface, Http2Connection, Http2Frame};
use super::ipv6::Ipv6Addr;
use super::{Ipv4Addr, NetError};

/// DoH içerik türü: DNS mesajlarını ikili (binary) wire formatında taşır
const DNS_MESSAGE_CONTENT_TYPE: &str = "application/dns-message";

/// DoH istemcisi.
///
/// Belirtilen DoH sunucu URL'sine DNS sorguları gönderir.
/// Yanıtlar önbelleklenerek sonraki sorgular hızlandırılır.
pub struct DohClient {
    pub server_url: String,                      // DoH sunucusunun HTTPS URL'si
    pub timeout_ms: u64,                         // Sorgu zaman aşımı (milisaniye)
    pub retry_budget: u8,                        // Ağ/timeout hatalarında yeniden deneme sayısı
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
            retry_budget: 2,
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
    /// HTTP/1.1 isteği TLS üzerinden gönderilir.
    pub fn query_get(&mut self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);
        let encoded = base64url_encode(&dns_query);
        let (host, path) = parse_doh_url(&self.server_url);

        if prefers_native_h2(&host) {
            match self.query_get_native_h2(domain, qtype) {
                Ok(response) => return Ok(response),
                Err(err @ DohError::Timeout) | Err(err @ DohError::NetworkError) => {
                    crate::serial_println!(
                        "[DoH] Native h2 path fell back to HTTP/1.1 for {}: {:?}",
                        host,
                        err
                    );
                }
                Err(err) => return Err(err),
            }
        }

        self.retry_request("GET", &host, || {
            format!(
                "GET {}?dns={} HTTP/1.1\r\nHost: {}\r\nAccept: {}\r\nConnection: close\r\n\r\n",
                path, encoded, host, DNS_MESSAGE_CONTENT_TYPE
            )
            .into_bytes()
        })
    }

    pub fn query_get_native_h2(
        &mut self,
        domain: &str,
        qtype: DnsRecordType,
    ) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);
        let (host, path) = parse_doh_url(&self.server_url);

        let mut last_error = DohError::NetworkError;
        for attempt in 0..=self.retry_budget {
            let (stream_id, request) = self.build_h2_get_request(&host, &path, &dns_query);
            match self.send_https_request_h2(&host, stream_id, &request) {
                Ok(response_body) => {
                    crate::serial_println!(
                        "[DoH] Native h2 response received ({} bytes) on attempt {}",
                        response_body.len(),
                        attempt + 1
                    );
                    return Ok(response_body);
                }
                Err(err @ DohError::Timeout) | Err(err @ DohError::NetworkError) => {
                    last_error = err;
                    crate::serial_println!(
                        "[DoH] Native h2 retry {}/{} for {}",
                        attempt + 1,
                        self.retry_budget + 1,
                        host
                    );
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error)
    }

    /// DNS sorgusunu HTTPS POST yöntemiyle gönderir.
    ///
    /// DNS sorgusu ikili formatta HTTP body olarak gönderilir:
    /// Content-Type: application/dns-message
    ///
    /// HTTP/1.1 isteği TLS üzerinden gönderilir.
    pub fn query_post(&mut self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DohError> {
        let dns_query = Self::build_query(domain, qtype);

        // URL'den host ve path ayrıştır
        let (host, path) = parse_doh_url(&self.server_url);

        // HTTP/1.1 POST isteği oluştur
        self.retry_request("POST", &host, || {
            let header = format!(
                "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept: {}\r\nConnection: close\r\n\r\n",
                path, host, DNS_MESSAGE_CONTENT_TYPE, dns_query.len(), DNS_MESSAGE_CONTENT_TYPE
            );
            let mut request = header.into_bytes();
            request.extend_from_slice(&dns_query);
            request
        })
    }

    fn retry_request<F>(
        &mut self,
        method: &str,
        host: &str,
        mut build_request: F,
    ) -> Result<Vec<u8>, DohError>
    where
        F: FnMut() -> Vec<u8>,
    {
        let mut last_error = DohError::NetworkError;
        for attempt in 0..=self.retry_budget {
            let request = build_request();
            match self.send_https_request(host, &request) {
                Ok(response_body) => {
                    crate::serial_println!(
                        "[DoH] {} response received ({} bytes) on attempt {}",
                        method,
                        response_body.len(),
                        attempt + 1
                    );
                    return Ok(response_body);
                }
                Err(err @ DohError::Timeout) | Err(err @ DohError::NetworkError) => {
                    last_error = err;
                    crate::serial_println!(
                        "[DoH] {} retry {}/{} for {}",
                        method,
                        attempt + 1,
                        self.retry_budget + 1,
                        host
                    );
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error)
    }

    pub fn smoke_a_lookup(&mut self, hostname: &str) -> Result<Ipv4Addr, DohError> {
        let response = self.query_get(hostname, DnsRecordType::A)?;
        let parsed = Self::parse_response(&response)?;
        parsed.get_a().ok_or(DohError::InvalidResponse)
    }

    fn build_h2_get_request(&self, host: &str, path: &str, dns_query: &[u8]) -> (u32, Vec<u8>) {
        let encoded = base64url_encode(dns_query);
        let mut connection = Http2Connection::new();
        let stream_id = connection.create_stream();
        let request_path = format!("{}?dns={}", path, encoded);
        let header_block = connection.build_request(stream_id, "GET", &request_path, host);

        let mut request = connection_preface().to_vec();
        request.extend_from_slice(&Http2Frame::settings(&connection.settings).encode());
        request.extend_from_slice(&Http2Frame::headers(stream_id, header_block, true).encode());
        (stream_id, request)
    }

    fn parse_h2_response(
        &self,
        expected_stream_id: u32,
        response_bytes: &[u8],
    ) -> Result<Vec<u8>, DohError> {
        let mut connection = Http2Connection::new();
        let stream_id = connection.create_stream();
        if stream_id != expected_stream_id {
            return Err(DohError::InvalidResponse);
        }

        let mut cursor = response_bytes;
        while let Some((frame, consumed)) = Http2Frame::decode(cursor) {
            connection
                .process_frame(&frame)
                .map_err(|_| DohError::InvalidResponse)?;
            cursor = &cursor[consumed..];
            if cursor.is_empty() {
                break;
            }
        }

        let stream = connection
            .get_stream(expected_stream_id)
            .ok_or(DohError::InvalidResponse)?;
        if stream.headers.get(":status").map(String::as_str) != Some("200") {
            return Err(DohError::ServerError(500));
        }
        if stream.headers.get("content-type").map(String::as_str) != Some(DNS_MESSAGE_CONTENT_TYPE)
        {
            return Err(DohError::InvalidResponse);
        }
        if stream.data.is_empty() {
            return Err(DohError::InvalidResponse);
        }

        Ok(stream.data.clone())
    }

    /// HTTPS (TLS) üzerinden HTTP isteği gönderir ve yanıt gövdesini döner.
    ///
    /// Adımlar:
    /// 1. Sunucu IP adresini DNS ile çözümle
    /// 2. TCP bağlantısı kur (port 443)
    /// 3. TLS handshake gerçekleştir
    /// 4. HTTP isteğini TLS kaydı olarak gönder
    /// 5. Yanıtı al ve HTTP body'sini ayrıştır
    fn send_https_request(&self, host: &str, request: &[u8]) -> Result<Vec<u8>, DohError> {
        use super::socket::{
            close, connect, recv, send, socket, AddressFamily, Protocol, SocketType,
        };
        use super::{Port, SocketAddr};

        // Sunucu IP adresini çözümle (DoH sunucusu için plain DNS kullanıyoruz)
        let server_ip =
            crate::net::dns::resolve_default(host).map_err(|_| DohError::NetworkError)?;

        // TCP soketi oluştur ve bağlan (HTTPS = port 443)
        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| DohError::NetworkError)?;
        let addr = SocketAddr::new(server_ip, Port(443));
        connect(sock_id, addr).map_err(|_| DohError::NetworkError)?;

        // ── TLS Handshake ─────────────────────────────────────────────
        let mut tls = crate::net::tls::TlsClient::new();

        // ClientHello gönder
        let client_hello = tls.build_client_hello(host);
        let hello_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::Handshake, &client_hello);
        send(sock_id, &hello_record, 0).map_err(|_| DohError::NetworkError)?;

        // ServerHello al
        let mut sh_buf = [0u8; 4096];
        let sh_len = recv(sock_id, &mut sh_buf, 0).map_err(|_| DohError::NetworkError)?;
        if sh_len > 5 {
            let _ = tls.process_server_hello(&sh_buf[5..sh_len]);
        }

        // Kalan handshake mesajlarını al
        let mut hs_buf = [0u8; 8192];
        let hs_len = recv(sock_id, &mut hs_buf, 0).map_err(|_| DohError::NetworkError)?;
        if hs_len > 5 {
            let _ = tls.process_encrypted_extensions(&hs_buf[5..hs_len]);
        }

        tls.complete_handshake();

        if !tls.is_established() {
            let _ = close(sock_id);
            crate::serial_println!("[DoH] TLS handshake failed for {}", host);
            return Err(DohError::NetworkError);
        }
        crate::serial_println!("[DoH] TLS established with {}", host);

        // ── HTTP isteğini gönder ──────────────────────────────────────
        let tls_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::ApplicationData, request);
        send(sock_id, &tls_record, 0).map_err(|_| DohError::NetworkError)?;

        // ── Yanıtı al ──────────────────────────────────────────────
        let mut resp_buf = [0u8; 16384];
        let resp_len = recv(sock_id, &mut resp_buf, 0).map_err(|_| DohError::Timeout)?;
        let _ = close(sock_id);

        if resp_len < 5 {
            return Err(DohError::InvalidResponse);
        }

        // TLS kayıt başlığını atla (5 byte)
        let http_response = &resp_buf[5..resp_len];

        // HTTP yanıtından body'yi çıkar (\r\n\r\n ayırıcısı)
        let body = extract_http_body(http_response);

        if body.is_empty() {
            return Err(DohError::InvalidResponse);
        }

        Ok(body)
    }

    fn send_https_request_h2(
        &self,
        host: &str,
        stream_id: u32,
        request: &[u8],
    ) -> Result<Vec<u8>, DohError> {
        use super::socket::{
            close, connect, recv, send, socket, AddressFamily, Protocol, SocketType,
        };
        use super::{Port, SocketAddr};

        let server_ip =
            crate::net::dns::resolve_default(host).map_err(|_| DohError::NetworkError)?;
        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| DohError::NetworkError)?;
        let addr = SocketAddr::new(server_ip, Port(443));
        connect(sock_id, addr).map_err(|_| DohError::NetworkError)?;

        let mut tls = crate::net::tls::TlsClient::new();
        let client_hello = tls.build_client_hello(host);
        let hello_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::Handshake, &client_hello);
        send(sock_id, &hello_record, 0).map_err(|_| DohError::NetworkError)?;

        let mut sh_buf = [0u8; 4096];
        let sh_len = recv(sock_id, &mut sh_buf, 0).map_err(|_| DohError::NetworkError)?;
        if sh_len > 5 {
            let _ = tls.process_server_hello(&sh_buf[5..sh_len]);
        }

        let mut hs_buf = [0u8; 8192];
        let hs_len = recv(sock_id, &mut hs_buf, 0).map_err(|_| DohError::NetworkError)?;
        if hs_len > 5 {
            let _ = tls.process_encrypted_extensions(&hs_buf[5..hs_len]);
        }

        tls.complete_handshake();
        if !tls.is_established() {
            let _ = close(sock_id);
            return Err(DohError::NetworkError);
        }

        let tls_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::ApplicationData, request);
        send(sock_id, &tls_record, 0).map_err(|_| DohError::NetworkError)?;

        let mut resp_buf = [0u8; 16384];
        let resp_len = recv(sock_id, &mut resp_buf, 0).map_err(|_| DohError::Timeout)?;
        let _ = close(sock_id);
        if resp_len < 5 {
            return Err(DohError::InvalidResponse);
        }

        self.parse_h2_response(stream_id, &resp_buf[5..resp_len])
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
        let _ttl = u32::from_be_bytes([
            data[*offset + 4],
            data[*offset + 5],
            data[*offset + 6],
            data[*offset + 7],
        ]);
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
                Some(IpAddr::V4(Ipv4Addr::from_bytes([
                    rdata[0], rdata[1], rdata[2], rdata[3],
                ])))
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
        let mut resume_offset = None; // Pointer sonrası gerçek akış konumu
        let mut max_jumps = 5; // Döngü önleme: en fazla 5 sıkıştırma atlaması

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
                if resume_offset.is_none() {
                    resume_offset = Some(*offset + 2);
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
        if let Some(saved_offset) = resume_offset {
            *offset = saved_offset;
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
    pub header: DnsHeader,       // DNS başlığı (ID, flags, sayılar)
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
    InvalidResponse,  // Yanıt geçerli DNS formatında değil
    NetworkError,     // Ağ bağlantı hatası
    Timeout,          // Zaman aşımı
    ServerError(u16), // HTTP hata kodu (4xx, 5xx)
}

/// DoH URL'sinden host ve path bileşenlerini ayrıştırır.
///
/// Örnek: "https://cloudflare-dns.com/dns-query" → ("cloudflare-dns.com", "/dns-query")
fn parse_doh_url(url: &str) -> (String, String) {
    // Şema kısmını atla (https://)
    let without_scheme = if let Some(rest) = url.strip_prefix("https://") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else {
        url
    };

    // Host ve path'i ayır
    if let Some(slash_pos) = without_scheme.find('/') {
        let host = &without_scheme[..slash_pos];
        let path = &without_scheme[slash_pos..];
        (String::from(host), String::from(path))
    } else {
        (String::from(without_scheme), String::from("/dns-query"))
    }
}

fn prefers_native_h2(host: &str) -> bool {
    matches!(host, "dns.quad9.net" | "dns.adguard-dns.com")
}

/// HTTP yanıtından body kısmını çıkarır.
///
/// Header ve body arasındaki `\r\n\r\n` ayırıcısını bulur.
fn extract_http_body(data: &[u8]) -> Vec<u8> {
    // \r\n\r\n = [13, 10, 13, 10] ara
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == 13 && data[i + 1] == 10 && data[i + 2] == 13 && data[i + 3] == 10 {
            return data[i + 4..].to_vec();
        }
    }
    // Ayırıcı bulunamazsa tüm veriyi döndür (best effort)
    data.to_vec()
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
        let b1 = if i + 1 < data.len() {
            data[i + 1] as usize
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as usize
        } else {
            0
        };

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
/// İstemci başlatılmamışsa Cloudflare varsayılanı ile başlatılır.
pub fn resolve_doh(domain: &str, qtype: DnsRecordType) -> Result<DnsResponse, DohError> {
    let mut client = DOH_CLIENT.lock();
    if client.is_none() {
        *client = Some(DohClient::cloudflare());
    }
    if let Some(client) = client.as_mut() {
        let response_data = client.query_get(domain, qtype)?;
        DohClient::parse_response(&response_data)
    } else {
        Err(DohError::NetworkError)
    }
}

/// Alan adını güvenli DNS yöntemleri ile çözümler (fallback zinciri).
///
/// Deneme sırası:
/// 1. DoH (DNS over HTTPS) — en yüksek gizlilik, HTTPS trafiğine karışır
/// 2. DoT (DNS over TLS) — şifreli ama ayrı port (853)
/// 3. Plain DNS (UDP port 53) — şifresiz, son çare
///
/// İlk başarılı sonuç döner. Tümü başarısız olursa son hatayı döner.
pub fn resolve_with_fallback(
    hostname: &str,
    record_type: super::dns::DnsRecordType,
) -> Result<super::Ipv4Addr, DohError> {
    // ── 1. DoH dene ───────────────────────────────────────────────────
    crate::serial_println!("[DoH-Fallback] Trying DoH for {}", hostname);
    match resolve_doh(hostname, record_type) {
        Ok(response) => {
            if let Some(ip) = response.get_a() {
                crate::serial_println!("[DoH-Fallback] DoH succeeded for {}", hostname);
                return Ok(ip);
            }
        }
        Err(e) => {
            crate::serial_println!("[DoH-Fallback] DoH failed for {}: {:?}", hostname, e);
        }
    }

    // ── 2. DoT dene ───────────────────────────────────────────────────
    crate::serial_println!("[DoH-Fallback] Trying DoT for {}", hostname);
    match super::dot::resolve_dot_sustained(hostname) {
        Ok(ip) => {
            crate::serial_println!("[DoH-Fallback] DoT succeeded for {}", hostname);
            return Ok(ip);
        }
        Err(e) => {
            crate::serial_println!("[DoH-Fallback] DoT failed for {}: {:?}", hostname, e);
        }
    }

    // ── 3. Plain DNS dene ─────────────────────────────────────────────
    crate::serial_println!("[DoH-Fallback] Trying plain DNS for {}", hostname);
    match super::dns::resolve_default(hostname) {
        Ok(ip) => {
            crate::serial_println!("[DoH-Fallback] Plain DNS succeeded for {}", hostname);
            Ok(ip)
        }
        Err(_) => {
            crate::serial_println!("[DoH-Fallback] All methods failed for {}", hostname);
            Err(DohError::NetworkError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::http2::{Http2Connection, Http2Frame};
    use alloc::collections::BTreeMap;

    fn build_dns_a_answer(domain: &str, ip: [u8; 4]) -> Vec<u8> {
        let mut packet = DohClient::build_query(domain, DnsRecordType::A);
        packet[2] = 0x81;
        packet[3] = 0x80;
        packet[6] = 0x00;
        packet[7] = 0x01;
        packet.extend_from_slice(&[0xC0, 0x0C]);
        packet.extend_from_slice(&1u16.to_be_bytes());
        packet.extend_from_slice(&1u16.to_be_bytes());
        packet.extend_from_slice(&60u32.to_be_bytes());
        packet.extend_from_slice(&4u16.to_be_bytes());
        packet.extend_from_slice(&ip);
        packet
    }

    #[test]
    fn doh_native_h2_request_uses_preface_settings_and_headers() {
        let client = DohClient::quad9();
        let (stream_id, request) =
            client.build_h2_get_request("dns.quad9.net", "/dns-query", &[0x12, 0x34]);

        assert_eq!(stream_id, 1);
        assert!(request.starts_with(connection_preface()));
        let after_preface = &request[connection_preface().len()..];
        let (settings, consumed) = Http2Frame::decode(after_preface).unwrap();
        assert_eq!(settings.frame_type, 0x04);
        let (headers, _) = Http2Frame::decode(&after_preface[consumed..]).unwrap();
        assert_eq!(headers.frame_type, 0x01);
        assert!(headers.is_end_stream());
    }

    #[test]
    fn doh_native_h2_response_preserves_dns_message_body() {
        let client = DohClient::quad9();
        let dns_body = build_dns_a_answer("example.com", [1, 1, 1, 1]);
        let mut encoder = Http2Connection::new();
        let _ = encoder.create_stream();

        let mut headers = BTreeMap::new();
        headers.insert(":status".to_string(), "200".to_string());
        headers.insert(
            "content-type".to_string(),
            DNS_MESSAGE_CONTENT_TYPE.to_string(),
        );

        let mut response = Vec::new();
        response.extend_from_slice(&Http2Frame::settings(&encoder.settings).encode());
        response.extend_from_slice(
            &Http2Frame::headers(1, encoder.encoder.encode(&headers), false).encode(),
        );
        response.extend_from_slice(&Http2Frame::data(1, dns_body.clone(), true).encode());

        let parsed = client.parse_h2_response(1, &response).unwrap();
        assert_eq!(parsed, dns_body);
        let dns = DohClient::parse_response(&parsed).unwrap();
        assert_eq!(dns.get_a(), Some(Ipv4Addr::from_bytes([1, 1, 1, 1])));
    }
}
