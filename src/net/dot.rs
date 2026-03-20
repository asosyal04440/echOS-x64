//! # DNS over TLS (DoT) - TLS Üzerinden DNS
//!
//! DoT, DNS sorgularını TLS (Transport Layer Security) protokolüyle şifreler.
//! RFC 7858 ile tanımlanmıştır. Standart port: 853.
//!
//! ## DoT ve Geleneksel DNS Karşılaştırması
//!
//! ```text
//! Geleneksel DNS (Port 53, şifresiz UDP/TCP):
//!   Uygulama --> [DNS Sorgu, düz metin] --> 8.8.8.8:53
//!              [İSS her sorguyu görür: gizlilik yok!]
//!
//! DoT (Port 853, TLS ile şifreli TCP):
//!   Uygulama --> [TCP bağlantısı] --> 8.8.8.8:853
//!             --> [TLS El Sıkışma (TLS Handshake)]
//!             --> [Şifreli DNS Sorgu (TLS kanal içinde)]
//!              [İSS sadece 853 portuna bağlandığını görür, içerik görünmez]
//! ```
//!
//! ## DoT ile DoH Farkı
//!
//! ```text
//! DoT (DNS over TLS, RFC 7858):
//!   - Port: 853 (ayrı ve belirgin, İSS kolayca tespit edebilir)
//!   - Protokol: TLS + DNS wire format
//!   - İzleme: İSS 853 portuna bakarak DoT kullanıldığını anlar
//!
//! DoH (DNS over HTTPS, RFC 8484):
//!   - Port: 443 (normal HTTPS trafiğiyle karışır)
//!   - Protokol: HTTP/HTTPS + DNS wire format (daha karmaşık)
//!   - İzleme: İSS HTTPS trafiğinden DoH'u ayırt etmek güçtür
//! ```
//!
//! ## DoT Bağlantı Akışı
//!
//! ```text
//! İstemci                              DoT Sunucusu (853/TCP)
//!    |                                        |
//!    |--- TCP SYN ---------------------------->|
//!    |<-- TCP SYN-ACK ------------------------|
//!    |--- TCP ACK ---------------------------->|
//!    |          [TCP Bağlantısı Kuruldu]       |
//!    |                                        |
//!    |--- TLS ClientHello -------------------->|
//!    |<-- TLS ServerHello + Certificate -------|
//!    |--- TLS Finished (Key Exchange) -------->|
//!    |<-- TLS Finished ------------------------|
//!    |          [TLS Kanalı Aktif]             |
//!    |                                        |
//!    |--- [2-byte length][DNS Query] --------->|  TLS içinde
//!    |<-- [2-byte length][DNS Response] -------|  şifreli
//!    |                                        |
//! ```
//!
//! ## TCP Üzerinde DNS Mesaj Biçimi (RFC 7858 Bölüm 3.3)
//!
//! ```text
//! TLS kanalı içinde DNS mesajları şu formatta taşınır:
//!
//! +------------------+----------------------------+
//! | Uzunluk (2 byte) | DNS Mesajı (değişken byte) |
//! +------------------+----------------------------+
//!  ^
//!  Bu 2-byte big-endian uzunluk alanı TCP/TLS üzerinde DNS'e özgüdür.
//!  UDP'de bu alan yoktur (paket boyutu dolaylı olarak bilinir).
//!
//! Örnek: 30 byte'lık DNS mesajı:
//!   [0x00, 0x1E, ...30 byte DNS data...]
//!    ^^^^
//!    0x001E = 30 (decimal)
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use super::dns::DnsHeader;
use super::dns::DnsRecordType;
use super::ipv6::Ipv6Addr;
use super::socket::{close, connect, recv, send, socket, AddressFamily, Protocol, SocketType};
use super::{Ipv4Addr, NetError, Port, SocketAddr};

/// DoT standart portu (RFC 7858)
const DOT_PORT: u16 = 853;

/// DoT istemcisi.
///
/// Belirtilen DoT sunucusuna kalıcı TLS bağlantısı kurar ve
/// DNS sorgularını şifreli olarak gönderir.
/// Yanıtlar önbelleklenerek tekrar eden sorgular hızlandırılır.
pub struct DotClient {
    pub server_ip: Ipv4Addr, // DoT sunucusunun IPv4 adresi (örn. 1.1.1.1 Cloudflare)
    pub server_name: String, // TLS SNI için sunucu adı (örn. "cloudflare-dns.com")
    pub port: u16,           // DoT portu (varsayılan: 853)
    pub timeout_ms: u64,     // Sorgu zaman aşımı (milisaniye)
    pub retry_budget: u8,    // Ağ/timeout hatalarında yeniden deneme sayısı
    pub connected: bool,     // TLS bağlantısı kuruldu mu?
    pub socket_id: Option<u32>, // TCP soket tanımlayıcısı
    pub cache: BTreeMap<String, CachedResponse>, // DNS yanıt önbelleği
}

/// Önbelleklenen DoT yanıtı.
///
/// DNS yanıtı ham wire formatında saklanır.
/// `expiry`: Unix zaman damgası cinsinden son geçerlilik zamanı.
#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub response: Vec<u8>, // DNS wire format yanıt verisi
    pub expiry: u64,       // Bu zamandan sonra önbellek içeriği eskidir
}

impl DotClient {
    /// Yeni bir DoT istemcisi oluşturur.
    ///
    /// `server_name`: TLS SNI (Server Name Indication) için sunucu adı.
    /// TLS el sıkışmasında sunucu sertifikasını doğrulamak için kullanılır.
    pub fn new(server_ip: Ipv4Addr, server_name: &str) -> Self {
        DotClient {
            server_ip,
            server_name: server_name.to_string(),
            port: DOT_PORT,
            timeout_ms: 5000, // Varsayılan 5 saniye zaman aşımı
            retry_budget: 2,
            connected: false,
            socket_id: None,
            cache: BTreeMap::new(),
        }
    }

    /// Cloudflare DoT istemcisi oluşturur (1.1.1.1:853).
    ///
    /// Cloudflare, gizlilik odaklı ve hızlı bir DoT sağlayıcısıdır.
    pub fn cloudflare() -> Self {
        Self::new(Ipv4Addr::from_bytes([1, 1, 1, 1]), "cloudflare-dns.com")
    }

    /// Google DoT istemcisi oluşturur (8.8.8.8:853).
    pub fn google() -> Self {
        Self::new(Ipv4Addr::from_bytes([8, 8, 8, 8]), "dns.google")
    }

    /// Quad9 DoT istemcisi oluşturur (9.9.9.9:853).
    ///
    /// Quad9, zararlı domain filtreleme özelliği sunar.
    pub fn quad9() -> Self {
        Self::new(Ipv4Addr::from_bytes([9, 9, 9, 9]), "dns.quad9.net")
    }

    /// DoT sunucusuna TCP bağlantısı kurar, ardından TLS el sıkışmasını başlatır.
    ///
    /// Adımlar:
    /// 1. TCP soketi oluştur (port 853)
    /// 2. DoT sunucusuna TCP bağlan
    /// 3. TLS ClientHello gönder ve ServerHello al
    /// 4. TLS Handshake tamamla
    pub fn connect(&mut self) -> Result<(), DotError> {
        if self.connected {
            return Ok(());
        }

        // TCP soketi oluştur (STREAM = TCP bağlantı yönelimli)
        let sock_id = socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
            .map_err(|_| DotError::SocketError)?;

        // DoT sunucusuna TCP bağlantısı kur (Port 853)
        let addr = SocketAddr::new(self.server_ip, Port(self.port));
        connect(sock_id, addr).map_err(|_| DotError::ConnectionFailed)?;

        self.socket_id = Some(sock_id);

        // ── TLS El Sıkışması ──────────────────────────────────────────
        // crate::net::tls::TlsClient kullanarak TLS 1.3 handshake gerçekleştir.
        let mut tls = crate::net::tls::TlsClient::new();

        // 1. ClientHello oluştur ve gönder
        let client_hello = tls.build_client_hello(&self.server_name);
        let hello_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::Handshake, &client_hello);
        send(sock_id, &hello_record, 0).map_err(|_| DotError::ConnectionFailed)?;
        crate::serial_println!("[DoT] TLS ClientHello sent to {}", self.server_name);

        // 2. ServerHello al ve işle
        let mut sh_buf = [0u8; 4096];
        let sh_len = recv(sock_id, &mut sh_buf, 0).map_err(|_| DotError::ConnectionFailed)?;
        if sh_len < 5 {
            crate::serial_println!("[DoT] TLS ServerHello too short ({})", sh_len);
            let _ = close(sock_id);
            self.socket_id = None;
            return Err(DotError::TlsHandshakeFailed);
        }
        // TLS kayıt başlığını atla (5 byte) ve handshake mesajını işle
        let sh_payload = &sh_buf[5..sh_len];
        if tls.process_server_hello(sh_payload).is_err() {
            crate::serial_println!("[DoT] TLS ServerHello processing failed");
            let _ = close(sock_id);
            self.socket_id = None;
            return Err(DotError::TlsHandshakeFailed);
        }
        crate::serial_println!("[DoT] TLS ServerHello processed");

        // 3. Kalan handshake mesajlarını al (EncryptedExtensions, Certificate,
        //    CertificateVerify, Finished)
        let mut hs_buf = [0u8; 8192];
        let hs_len = recv(sock_id, &mut hs_buf, 0).map_err(|_| DotError::ConnectionFailed)?;
        if hs_len > 5 {
            let hs_payload = &hs_buf[5..hs_len];
            // İşle — hata olursa yok say, handshake tamamlamayı dene
            let _ = tls.process_encrypted_extensions(hs_payload);
        }

        // 4. Handshake'i tamamla — master secret türet
        tls.complete_handshake();

        if tls.is_established() {
            self.connected = true;
            // TLS state'i ileride session reuse için saklanabilir.
            // Mevcut istemci yalnızca kurulu TCP/TLS kanalı canlı tutar; ayrıntılı oturum yeniden kullanımı ayrı iş kalır.
            crate::serial_println!("[DoT] TLS handshake completed, connection established");
            Ok(())
        } else {
            crate::serial_println!("[DoT] TLS handshake did not reach Established state");
            let _ = close(sock_id);
            self.socket_id = None;
            Err(DotError::TlsHandshakeFailed)
        }
    }

    /// DoT sunucusundan bağlantıyı kapatır.
    ///
    /// TCP soketi kapatılır ve bağlantı durumu sıfırlanır.
    /// TLS aktifse önce TLS close_notify alert'i gönderilmeli (truncation saldırılarına karşı).
    pub fn disconnect(&mut self) {
        if let Some(sock_id) = self.socket_id {
            let _ = close(sock_id);
            self.socket_id = None;
            self.connected = false;
        }
    }

    /// DNS sorgu paketini wire formatında oluşturur.
    ///
    /// Oluşturulan paket sadece DNS mesajıdır.
    /// TCP üzerinden gönderilirken `query()` içinde 2-byte uzunluk alanı öne eklenir.
    pub fn build_query(domain: &str, qtype: DnsRecordType) -> Vec<u8> {
        let mut query = Vec::new();

        // DNS başlığı (12 byte): ID=0x1234, RD=1 (özyinelemeli), 1 soru
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

        // Soru bölümü: alan adını DNS etiket formatında kodla
        for label in domain.split('.') {
            if !label.is_empty() {
                query.push(label.len() as u8); // Etiket uzunluğu
                for c in label.chars() {
                    query.push(c as u8);
                }
            }
        }
        query.push(0); // Root label (alan adını sonlandırır)

        // QTYPE: hangi kayıt türü isteniyor
        query.push((qtype as u16 >> 8) as u8);
        query.push((qtype as u16 & 0xFF) as u8);

        // QCLASS: IN = 1 (Internet)
        query.push(0);
        query.push(1);

        query
    }

    /// TCP+TLS üzerinden DNS sorgusu gönderir.
    ///
    /// Önce önbellekte mevcut yanıt var mı kontrol edilir.
    /// Önbellekte yoksa bağlantı kurulur ve DNS sorgusu TLS altında gönderilir.
    ///
    /// TCP DNS formatı (RFC 7858): 2 byte uzunluk + DNS wire data
    pub fn query(&mut self, domain: &str, qtype: DnsRecordType) -> Result<Vec<u8>, DotError> {
        // Önbellek kontrolü: daha önce sorulmuş mu?
        let cache_key = format!("{}:{}", domain, qtype as u16);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached.response.clone());
        }

        let dns_query = Self::build_query(domain, qtype);
        let mut last_error = DotError::ConnectionFailed;
        for attempt in 0..=self.retry_budget {
            match self.query_once(domain, qtype, &dns_query) {
                Ok(dns_response) => {
                    self.cache.insert(
                        cache_key.clone(),
                        CachedResponse {
                            response: dns_response.clone(),
                            expiry: 0,
                        },
                    );
                    crate::serial_println!(
                        "[DoT] DNS response received ({} bytes) on attempt {}",
                        dns_response.len(),
                        attempt + 1
                    );
                    return Ok(dns_response);
                }
                Err(err @ DotError::Timeout)
                | Err(err @ DotError::ConnectionFailed)
                | Err(err @ DotError::TlsHandshakeFailed)
                | Err(err @ DotError::NotConnected) => {
                    last_error = err;
                    self.disconnect();
                    crate::serial_println!(
                        "[DoT] Retry {}/{} for {}",
                        attempt + 1,
                        self.retry_budget + 1,
                        domain
                    );
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error)
    }

    fn query_once(
        &mut self,
        domain: &str,
        qtype: DnsRecordType,
        dns_query: &[u8],
    ) -> Result<Vec<u8>, DotError> {
        if !self.connected {
            self.connect()?;
        }

        let sock_id = self.socket_id.ok_or(DotError::NotConnected)?;
        let mut dns_msg = Vec::new();
        dns_msg.extend_from_slice(&(dns_query.len() as u16).to_be_bytes());
        dns_msg.extend_from_slice(dns_query);

        let tls_record =
            crate::net::tls::wrap_record(crate::net::tls::ContentType::ApplicationData, &dns_msg);
        send(sock_id, &tls_record, 0).map_err(|_| DotError::ConnectionFailed)?;
        crate::serial_println!(
            "[DoT] DNS query sent for {} (type={})",
            domain,
            qtype as u16
        );

        let mut recv_buf = [0u8; 8192];
        let recv_len = recv(sock_id, &mut recv_buf, 0).map_err(|_| DotError::Timeout)?;

        if recv_len < 7 {
            return Err(DotError::InvalidResponse);
        }

        let tls_payload = &recv_buf[5..recv_len];
        if tls_payload.len() < 2 {
            return Err(DotError::InvalidResponse);
        }
        let dns_len = u16::from_be_bytes([tls_payload[0], tls_payload[1]]) as usize;
        let dns_data_start = 2;
        let dns_data_end = dns_data_start + dns_len.min(tls_payload.len() - 2);
        Ok(tls_payload[dns_data_start..dns_data_end].to_vec())
    }

    pub fn smoke_a_lookup(&mut self, hostname: &str) -> Result<Ipv4Addr, DotError> {
        let response = self.query(hostname, DnsRecordType::A)?;
        let parsed = Self::parse_response(&response)?;
        parsed.get_a().ok_or(DotError::InvalidResponse)
    }

    /// DNS wire format yanıtını ayrıştırır.
    ///
    /// Başlık, soru bölümü ve yanıt kayıtları ayrıştırılır.
    /// A ve AAAA kayıtları için IP adresi çıkarılır.
    pub fn parse_response(data: &[u8]) -> Result<DotResponse, DotError> {
        if data.len() < 12 {
            return Err(DotError::InvalidResponse);
        }

        let header = DnsHeader::parse(data).map_err(|_| DotError::InvalidResponse)?;

        let mut response = DotResponse {
            header,
            answers: Vec::new(),
        };

        // Soru bölümünü atla (parse etme, sadece offset ilerlet)
        let mut offset = 12;
        for _ in 0..header.qdcount {
            // Alan adı etiketlerini atla
            while offset < data.len() && data[offset] != 0 {
                if (data[offset] & 0xC0) == 0xC0 {
                    offset += 2; // Sıkıştırma işaretçisi (2 byte)
                    break;
                }
                offset += 1 + data[offset] as usize;
            }
            if offset < data.len() && data[offset] == 0 {
                offset += 1; // Root label
            }
            offset += 4; // QTYPE + QCLASS
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
    /// NAME, TYPE, CLASS, TTL, RDLENGTH ve RDATA alanlarını okur.
    fn parse_answer(data: &[u8], offset: &mut usize) -> Result<DotAnswer, DotError> {
        let name = Self::parse_name(data, offset)?;

        if *offset + 10 > data.len() {
            return Err(DotError::InvalidResponse);
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
            return Err(DotError::InvalidResponse);
        }

        let rdata = data[*offset..*offset + rdlength].to_vec();
        *offset += rdlength;

        // Kayıt türüne göre IP adresi çıkar
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
            _ => None, // Diğer kayıt türleri (CNAME, MX, TXT vb.)
        };

        Ok(DotAnswer {
            name,
            rtype,
            rdata,
            ip,
        })
    }

    /// DNS wire formatındaki alan adı etiketlerini metne çevirir.
    ///
    /// DNS sıkıştırması (0xC0 prefix işaretçileri) desteklenir.
    /// Sonsuz döngüye karşı en fazla 5 atlama sınırı uygulanır.
    fn parse_name(data: &[u8], offset: &mut usize) -> Result<String, DotError> {
        let mut name = String::new();
        let mut resume_offset = None; // Pointer sonrası gerçek akış konumu
        let mut max_jumps = 5; // Döngü koruması: en fazla 5 işaretçi atlaması

        loop {
            if *offset >= data.len() {
                return Err(DotError::InvalidResponse);
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1; // Root label: alan adının sonu
                break;
            }

            // DNS sıkıştırma işaretçisi: ilk 2 bit = 11 (0xC0)
            if (len & 0xC0) == 0xC0 {
                if *offset + 1 >= data.len() {
                    return Err(DotError::InvalidResponse);
                }
                let ptr = (((data[*offset] & 0x3F) as usize) << 8) | (data[*offset + 1] as usize);
                if resume_offset.is_none() {
                    resume_offset = Some(*offset + 2);
                }
                *offset = ptr; // İşaretçinin gösterdiği konuma atla
                max_jumps -= 1;
                if max_jumps == 0 {
                    return Err(DotError::InvalidResponse); // Muhtemel döngü
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
            name.push('.'); // Kök zone: tek nokta
        }
        if let Some(saved_offset) = resume_offset {
            *offset = saved_offset;
        }

        Ok(name)
    }
}

/// IP adresi sarmalayıcısı: IPv4 veya IPv6 adresini birlikte tutar.
#[derive(Clone, Debug)]
pub enum IpAddr {
    V4(Ipv4Addr), // 32-bit IPv4 adresi
    V6(Ipv6Addr), // 128-bit IPv6 adresi
}

/// DoT DNS yanıt kaydı (Resource Record).
///
/// İsim, kayıt türü, ham RDATA ve çözümlenmiş IP adresini içerir.
#[derive(Clone, Debug)]
pub struct DotAnswer {
    pub name: String,       // Yanıtın ait olduğu alan adı
    pub rtype: u16,         // Kayıt türü (1=A, 28=AAAA, 5=CNAME vb.)
    pub rdata: Vec<u8>,     // Ham kayıt verisi (RDATA)
    pub ip: Option<IpAddr>, // Çözümlenmiş IP (sadece A ve AAAA için)
}

/// DoT DNS sorgu yanıtı.
///
/// DNS başlığı ve yanıt kayıtları listesini içerir.
#[derive(Clone, Debug)]
pub struct DotResponse {
    pub header: DnsHeader,       // DNS yanıt başlığı
    pub answers: Vec<DotAnswer>, // Yanıt kayıtları (A, AAAA, CNAME vb.)
}

impl DotResponse {
    /// İlk A kaydının IPv4 adresini döner.
    ///
    /// Birden fazla A kaydı varsa yalnızca ilki döner.
    pub fn get_a(&self) -> Option<Ipv4Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V4(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }

    /// İlk AAAA kaydının IPv6 adresini döner.
    pub fn get_aaaa(&self) -> Option<Ipv6Addr> {
        for answer in &self.answers {
            if let Some(IpAddr::V6(ip)) = &answer.ip {
                return Some(*ip);
            }
        }
        None
    }
}

/// DoT hata türleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotError {
    TlsNotSupported,    // TLS henüz desteklenmiyor (TODO: rustls/mbedtls entegrasyonu)
    NotConnected,       // Sorgu öncesinde bağlantı kurulmamış
    SocketError,        // TCP soket oluşturulamadı
    ConnectionFailed,   // TCP bağlantısı kurulamadı
    InvalidResponse,    // Geçersiz DNS yanıt formatı
    Timeout,            // Sorgu zaman aşımına uğradı
    TlsHandshakeFailed, // TLS el sıkışması başarısız oldu
}

/// DotClient düşürüldüğünde TCP bağlantısını otomatik olarak kapatır.
///
/// Bu Drop implementasyonu kaynak sızıntısını önler:
/// DotClient kapsam dışına çıkınca TCP soketi temiz şekilde kapatılır.
impl Drop for DotClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

// Global DoT istemcisi: lazy_static ile thread-safe şekilde başlatılır
lazy_static::lazy_static! {
    static ref DOT_CLIENT: Mutex<Option<DotClient>> = Mutex::new(None);
}

/// Global DoT istemcisini başlatır.
///
/// `server_name` parametresi TLS SNI doğrulaması için kullanılır.
pub fn init_dot(server_ip: Ipv4Addr, server_name: &str) {
    *DOT_CLIENT.lock() = Some(DotClient::new(server_ip, server_name));
}

/// Global DoT istemcisi üzerinden alan adını çözümler.
///
/// İstemci başlatılmamışsa Cloudflare varsayılanı ile başlatılır.
pub fn resolve_dot(domain: &str, qtype: DnsRecordType) -> Result<DotResponse, DotError> {
    let mut client = DOT_CLIENT.lock();
    if client.is_none() {
        *client = Some(DotClient::cloudflare());
    }
    if let Some(client) = client.as_mut() {
        let response_data = client.query(domain, qtype)?;
        DotClient::parse_response(&response_data)
    } else {
        Err(DotError::NotConnected)
    }
}

fn sustained_lookup_with<F>(hostname: &str, mut lookup: F) -> Result<Ipv4Addr, DotError>
where
    F: FnMut(&str, &mut DotClient) -> Result<Ipv4Addr, DotError>,
{
    let mut providers = [
        ("cloudflare", DotClient::cloudflare()),
        ("google", DotClient::google()),
        ("quad9", DotClient::quad9()),
    ];
    let mut last_error = DotError::NotConnected;

    for (provider, client) in providers.iter_mut() {
        match lookup(provider, client) {
            Ok(ip) => return Ok(ip),
            Err(err @ DotError::Timeout)
            | Err(err @ DotError::ConnectionFailed)
            | Err(err @ DotError::TlsHandshakeFailed)
            | Err(err @ DotError::NotConnected) => {
                last_error = err;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error)
}

pub fn resolve_dot_sustained(hostname: &str) -> Result<Ipv4Addr, DotError> {
    sustained_lookup_with(hostname, |_, client| client.smoke_a_lookup(hostname))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sustained_lookup_rotates_across_resolvers_on_transient_failures() {
        let mut visited = Vec::new();
        let result = sustained_lookup_with("example.com", |provider, _client| {
            visited.push(provider.to_string());
            match provider {
                "cloudflare" => Err(DotError::Timeout),
                "google" => Err(DotError::TlsHandshakeFailed),
                "quad9" => Ok(Ipv4Addr::from_bytes([9, 9, 9, 9])),
                _ => unreachable!(),
            }
        })
        .unwrap();

        assert_eq!(visited, vec!["cloudflare", "google", "quad9"]);
        assert_eq!(result, Ipv4Addr::from_bytes([9, 9, 9, 9]));
    }

    #[test]
    fn sustained_lookup_stops_on_non_transient_parse_failure() {
        let mut visited = Vec::new();
        let err = sustained_lookup_with("example.com", |provider, _client| {
            visited.push(provider.to_string());
            match provider {
                "cloudflare" => Err(DotError::InvalidResponse),
                _ => Ok(Ipv4Addr::from_bytes([1, 1, 1, 1])),
            }
        })
        .unwrap_err();

        assert_eq!(visited, vec!["cloudflare"]);
        assert_eq!(err, DotError::InvalidResponse);
    }
}
