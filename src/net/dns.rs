//! # DNS İstemcisi (Domain Name System)
//!
//! DNS, alan adlarını (örn. "google.com") IP adreslerine çeviren dağıtık
//! bir veritabanı sistemidir. RFC 1034 ve RFC 1035 ile tanımlanmıştır.
//! UDP ve TCP üzerinde Port 53'te çalışır.
//!
//! ## DNS Paket Yapısı
//!
//! ```text
//! +---------------------------+  <-- Başlık (12 byte, sabit)
//! |  ID (2 byte)              |
//! +---------------------------+
//! |  Flags (2 byte)           |  QR | Opcode | AA | TC | RD | RA | Z | RCODE
//! +---------------------------+
//! |  QDCOUNT (2 byte)         |  Soru bölümündeki kayıt sayısı
//! +---------------------------+
//! |  ANCOUNT (2 byte)         |  Yanıt bölümündeki kayıt sayısı
//! +---------------------------+
//! |  NSCOUNT (2 byte)         |  Yetki bölümündeki kayıt sayısı
//! +---------------------------+
//! |  ARCOUNT (2 byte)         |  Ek bölümündeki kayıt sayısı
//! +---------------------------+  <-- Soru Bölümü (değişken uzunluk)
//! |  QNAME (değişken)         |  Alan adı (etiket formatında)
//! +---------------------------+
//! |  QTYPE (2 byte)           |  Sorgu türü: 1=A, 28=AAAA, 5=CNAME...
//! +---------------------------+
//! |  QCLASS (2 byte)          |  Sınıf: 1=IN (Internet)
//! +---------------------------+  <-- Yanıt Bölümü (değişken)
//! |  Resource Records         |
//! +---------------------------+
//! ```
//!
//! ## DNS Alan Adı Kodlaması (Wire Format)
//!
//! ```text
//! "www.example.com" şu şekilde kodlanır:
//!
//!  Byte:  0    1    2    3    4    5    6    7    8    9   10   11   12   13   14   15
//!       +----+----+----+----+----+----+----+----+----+----+----+----+----+----+----+----+
//!       | 03 | 'w'| 'w'| 'w'| 07 | 'e'| 'x'| 'a'| 'm'| 'p'| 'l'| 'e'| 03 | 'c'| 'o'| 'm'|
//!       +----+----+----+----+----+----+----+----+----+----+----+----+----+----+----+----+
//!         ^              ^              domain              ^         ^
//!         3 (uzunluk)   7 (uzunluk)                       3         sonra NULL byte (0x00)
//! ```
//!
//! ## DNS Sorgu/Yanıt Akışı
//!
//! ```text
//! Uygulama              DNS İstemcisi           DNS Sunucusu (8.8.8.8:53)
//!    |                       |                          |
//!    |-- resolve("google.com")|                          |
//!    |                       |--- UDP Sorgu (ID=0xABCD)->|
//!    |                       |   [Header][Question]      |
//!    |                       |                          |
//!    |                       |<-- UDP Yanıt (ID=0xABCD)--|
//!    |                       |   [Header][Answer: A/IP] |
//!    |<-- 142.250.185.46 ----|                          |
//! ```
//!
//! ## DNS Önbellekleme Stratejisi
//!
//! ```text
//! resolve("google.com") çağrısı:
//!       |
//!       v
//! DNS önbelleğini kontrol et (key = "google.com:1")
//!   +--HIT ve süresi dolmamış--> Önbellekten IP döndür (hızlı)
//!   |
//!   +--MISS veya süresi dolmuş-> UDP sorgusu gönder
//!                                 Yanıtı al ve önbelleğe kaydet (TTL ile)
//!                                 IP'yi döndür
//! ```

use super::socket::{bind, close, recvfrom, sendto, socket};
use super::udp;
use super::{Ipv4Addr, NetError, Port, SocketAddr};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

/// DNS sunucu portu: hem UDP hem TCP'de kullanılır
const DNS_PORT: u16 = 53;
// Fail-closed bound to break cyclic DNS compression pointers.
const MAX_DNS_COMPRESSION_JUMPS: usize = 10;
const MAX_DNS_LABEL_LEN: usize = 63;
const MAX_DNS_NAME_LEN: usize = 255;

/// DNS kayıt türleri (Resource Record Types).
///
/// Her kayıt türü farklı bir ağ bilgisini depolar.
/// İstemciler sorgu yaparken hangi türde kayıt istediklerini belirtir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsRecordType {
    A = 1,     // IPv4 adresi (32 bit)
    NS = 2,    // İsim sunucusu (Name Server): Bu domain için yetkili sunucu
    CNAME = 5, // Kanonik isim (Alias): Bir domain adı başka bir domain'e işaret eder
    SOA = 6,   // Yetki başlangıcı (Start of Authority): Zone hakkında bilgi
    PTR = 12,  // Ters sorgu (Pointer): IP'den domain adına (reverse DNS)
    MX = 15,   // Posta sunucusu (Mail Exchange)
    TXT = 16,  // Metin kaydı (Text): SPF, DKIM vb. için kullanılır
    AAAA = 28, // IPv6 adresi (128 bit)
    SRV = 33,  // Servis kaydı: Belirli servisler için port/öncelik bilgisi
}

impl DnsRecordType {
    pub fn from_u16(v: u16) -> Self {
        match v {
            1 => DnsRecordType::A,
            2 => DnsRecordType::NS,
            5 => DnsRecordType::CNAME,
            6 => DnsRecordType::SOA,
            12 => DnsRecordType::PTR,
            15 => DnsRecordType::MX,
            16 => DnsRecordType::TXT,
            28 => DnsRecordType::AAAA,
            33 => DnsRecordType::SRV,
            _ => DnsRecordType::A,
        }
    }
}

/// DNS başlık yapısı (12 byte, sabit boyut).
///
/// ```text
/// Flags alanı bit düzenlemesi:
///  15 14 13 12 11  10   9    8    7    6  5  4  3  2  1  0
/// +--+--+--+--+--+----+---+----+----+---+--+--+--+--+--+--+
/// |QR| Opcode  |AA| TC| RD| RA |  Z |     RCODE            |
/// +--+--+--+--+--+----+---+----+----+---+--+--+--+--+--+--+
///  QR: 0=Sorgu, 1=Yanıt
///  RD: Recursive Desired (özyinelemeli sorgu iste)
///  RA: Recursive Available (sunucu özyinelemeli sorgu yapabilir)
///  RCODE: 0=Hata yok, 3=NXDOMAIN (alan adı yok)
/// ```
#[derive(Clone, Copy, Debug)]
pub struct DnsHeader {
    pub id: u16,      // Sorgu/yanıt eşleştirmek için benzersiz kimlik
    pub flags: u16,   // Bayraklar: QR, Opcode, AA, TC, RD, RA, Z, RCODE
    pub qdcount: u16, // Soru bölümü kayıt sayısı
    pub ancount: u16, // Yanıt bölümü kayıt sayısı
    pub nscount: u16, // Yetki bölümü kayıt sayısı (name server)
    pub arcount: u16, // Ek bölümü kayıt sayısı (additional)
}

impl DnsHeader {
    /// DNS başlığının sabit boyutu: 12 byte
    pub const SIZE: usize = 12;

    /// Yeni bir DNS sorgu başlığı oluşturur.
    ///
    /// - QR=0 (sorgu)
    /// - RD=1 (özyinelemeli sorgu iste, sunucu bizim adımıza araştırır)
    /// - 1 soru, 0 yanıt
    pub fn new_query(id: u16) -> Self {
        DnsHeader {
            id,
            flags: 0x0100, // RD=1: özyinelemeli sorgu bayrağı etkin
            qdcount: 1,    // Tek soru var
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }

    /// Ham byte dizisinden DNS başlığını ayrıştırır (12 byte big-endian).
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }

        Ok(DnsHeader {
            id: u16::from_be_bytes([data[0], data[1]]),
            flags: u16::from_be_bytes([data[2], data[3]]),
            qdcount: u16::from_be_bytes([data[4], data[5]]),
            ancount: u16::from_be_bytes([data[6], data[7]]),
            nscount: u16::from_be_bytes([data[8], data[9]]),
            arcount: u16::from_be_bytes([data[10], data[11]]),
        })
    }

    /// DNS başlığını byte dizisine yazar (12 byte big-endian).
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }

        buf[0..2].copy_from_slice(&self.id.to_be_bytes());
        buf[2..4].copy_from_slice(&self.flags.to_be_bytes());
        buf[4..6].copy_from_slice(&self.qdcount.to_be_bytes());
        buf[6..8].copy_from_slice(&self.ancount.to_be_bytes());
        buf[8..10].copy_from_slice(&self.nscount.to_be_bytes());
        buf[10..12].copy_from_slice(&self.arcount.to_be_bytes());

        Ok(())
    }

    /// Bu paketin yanıt (response) mı olduğunu kontrol eder.
    ///
    /// Flags alanının en yüksek biti (QR biti) 1 ise yanıt paketini belirtir.
    pub fn is_response(&self) -> bool {
        self.flags & 0x8000 != 0
    }

    /// Yanıt kodunun başarılı (NOERROR = 0) olduğunu kontrol eder.
    ///
    /// RCODE = 0 ise hata yok.
    /// RCODE = 1 = Format hatası
    /// RCODE = 2 = Sunucu hatası
    /// RCODE = 3 = NXDOMAIN (alan adı mevcut değil)
    pub fn is_valid(&self) -> bool {
        self.flags & 0x000F == 0 // Alt 4 bit: RCODE
    }
}

/// DNS soru kaydı.
///
/// Sorgulanan alan adını, kayıt türünü ve sınıfını içerir.
/// Her DNS sorgusunda en az bir soru kaydı bulunur.
#[derive(Clone, Debug)]
pub struct DnsQuestion {
    pub name: String,
    pub qtype: u16,  // Sorgu türü: 1=A (IPv4), 28=AAAA (IPv6), 15=MX vb.
    pub qclass: u16, // Sorgu sınıfı: 1=IN (Internet)
}

impl DnsQuestion {
    /// Belirtilen alan adı için A kaydı sorgulayan bir soru oluşturur.
    pub fn new(name: &str) -> Self {
        DnsQuestion {
            name: String::from(name),
            qtype: 1,  // A kaydı (IPv4)
            qclass: 1, // IN (Internet)
        }
    }

    /// Soru kaydını DNS wire format (ağ formatı) olarak yazar.
    ///
    /// Alan adı etiket formatında kodlanır:
    /// "www.example.com" -> \x03www\x07example\x03com\x00
    ///
    /// Dönen değer yazılan byte sayısıdır.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let mut offset = 0;

        // Alan adını etiket formatına çevir (her parçayı uzunluk + baytlar ile yaz)
        for part in self.name.split('.') {
            if part.is_empty() {
                continue;
            }
            let bytes = part.as_bytes();
            if offset + 1 + bytes.len() >= buf.len() {
                return Err(NetError::BufferFull);
            }
            buf[offset] = bytes.len() as u8; // Etiket uzunluğu
            offset += 1;
            buf[offset..offset + bytes.len()].copy_from_slice(bytes);
            offset += bytes.len();
        }

        // Root label: 0 byte (alan adının sonu)
        buf[offset] = 0;
        offset += 1;

        // QTYPE ve QCLASS alanlarını yaz
        if offset + 4 >= buf.len() {
            return Err(NetError::BufferFull);
        }
        buf[offset..offset + 2].copy_from_slice(&self.qtype.to_be_bytes());
        buf[offset + 2..offset + 4].copy_from_slice(&self.qclass.to_be_bytes());
        offset += 4;

        Ok(offset)
    }
}

/// DNS yanıt kaydı (Resource Record).
///
/// ```text
/// DNS Resource Record Yapısı:
///  NAME    : Alan adı (etiket veya sıkıştırma işaretçisi)
///  TYPE    : Kayıt türü (2 byte)
///  CLASS   : Sınıf (2 byte, genelde IN=1)
///  TTL     : Önbellekte tutma süresi (4 byte, saniye)
///  RDLENGTH: Veri uzunluğu (2 byte)
///  RDATA   : Asıl veri (RDLENGTH byte)
///            A kaydı için: 4 byte IPv4
///            AAAA için: 16 byte IPv6
///            CNAME için: etiket formatında alan adı
/// ```
#[derive(Clone, Debug)]
pub struct DnsAnswer {
    pub name: String,  // Yanıtın ait olduğu alan adı
    pub atype: u16,    // Kayıt türü (A=1, AAAA=28 vb.)
    pub aclass: u16,   // Sınıf (IN=1)
    pub ttl: u32,      // Time-To-Live: önbellekte kaç saniye tutulacağı
    pub data: Vec<u8>, // Ham kayıt verisi (RDATA)
}

impl DnsAnswer {
    /// DNS yanıt kaydını belirtilen offset'ten itibaren ayrıştırır.
    ///
    /// DNS sıkıştırmasını (pointer) destekler. İşaretçi (0xC0 ile başlayan)
    /// başka bir konumdaki alan adını gösterir.
    /// Dönen değer: (yanıt, yeni offset)
    pub fn parse(data: &[u8], offset: usize) -> Result<(Self, usize), NetError> {
        let mut pos = offset;

        // Alan adını ayrıştır (sıkıştırma desteğiyle)
        let name = Self::parse_name(data, &mut pos)?;

        if pos + 10 > data.len() {
            return Err(NetError::InvalidPacket);
        }

        let atype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let aclass = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
        let ttl = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let rdlength = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;

        if pos + rdlength > data.len() {
            return Err(NetError::InvalidPacket);
        }

        let answer_data = data[pos..pos + rdlength].to_vec();
        pos += rdlength;

        Ok((
            DnsAnswer {
                name,
                atype,
                aclass,
                ttl,
                data: answer_data,
            },
            pos,
        ))
    }

    /// DNS wire formatındaki alan adını metne çevirir.
    ///
    /// DNS sıkıştırması: 0xC0 prefix'li byte bir işaretçidir.
    /// İşaretçi paketin başka bir bölümündeki alan adına atlar,
    /// bu sayede tekrarlayan alan adları için bant genişliği tasarrufu sağlanır.
    fn parse_name(data: &[u8], pos: &mut usize) -> Result<String, NetError> {
        parse_dns_name(data, pos)
    }

    /// Kayıt A tipi ise IPv4 adresini döner.
    ///
    /// A kaydının RDATA alanı tam olarak 4 byte olmalıdır.
    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        if self.atype == 1 && self.data.len() == 4 {
            Some(Ipv4Addr::from_bytes([
                self.data[0],
                self.data[1],
                self.data[2],
                self.data[3],
            ]))
        } else {
            None
        }
    }
}

// ============================================================================
// DNS RESOLVER (DNS ÇÖZÜMLEYICI)
// ============================================================================
//
// DNS çözümleyici, alan adlarını IP adreslerine çevirir.
// Önbellek sayesinde aynı sorgu kısa süre içinde tekrar gelirse
// ağ trafiği oluşturmadan yanıtlanır.
//
// Önbellek anahtarı: "alan_adı:kayıt_türü"
// Örnek: "google.com:1" (A kaydı), "google.com:28" (AAAA kaydı)

static DNS_SOCKET: Mutex<Option<u32>> = Mutex::new(None);

/// DNS önbellek girdisi.
///
/// Her girdi bir kayıt türüne ait veriyi ve ne zaman alındığını saklar.
/// TTL (Time-To-Live) süresi dolmadan girdi geçerli kabul edilir.
#[derive(Clone, Debug)]
pub struct DnsCacheEntry {
    pub name: String,               // Alan adı
    pub record_type: DnsRecordType, // Kayıt türü (A, AAAA, CNAME vb.)
    pub data: Vec<u8>,              // Ham kayıt verisi
    pub ttl: u32,                   // Saniye cinsinden geçerlilik süresi
    pub obtained_at: u64,           // Alındığı zaman damgası
}

impl DnsCacheEntry {
    /// TTL süresinin dolup dolmadığını kontrol eder.
    ///
    /// `current_time - obtained_at >= ttl` ise kayıt eskidir.
    pub fn is_expired(&self, current_time: u64) -> bool {
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.ttl as u64
    }

    /// A kaydı ise IPv4 adresini döner.
    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        if self.record_type == DnsRecordType::A && self.data.len() == 4 {
            Some(Ipv4Addr::from_bytes([
                self.data[0],
                self.data[1],
                self.data[2],
                self.data[3],
            ]))
        } else {
            None
        }
    }

    /// AAAA kaydı ise IPv6 adresini döner (16 byte).
    pub fn as_ipv6(&self) -> Option<[u8; 16]> {
        if self.record_type == DnsRecordType::AAAA && self.data.len() == 16 {
            let mut addr = [0u8; 16];
            addr.copy_from_slice(&self.data);
            Some(addr)
        } else {
            None
        }
    }

    /// CNAME kaydı ise takma adı (alias) string olarak döner.
    pub fn as_cname(&self) -> Option<&str> {
        if self.record_type == DnsRecordType::CNAME {
            // CNAME data is a domain name
            Some(core::str::from_utf8(&self.data).unwrap_or(""))
        } else {
            None
        }
    }
}

/// DNS önbelleği (anahtar = "isim:tür").
///
/// BTreeMap kullanımı: deterministik sıralama ve no_std uyumu için.
static DNS_CACHE: Mutex<BTreeMap<String, DnsCacheEntry>> = Mutex::new(BTreeMap::new());

/// DNS çözümleyiciyi başlatır.
pub fn init() {
    crate::serial_println!("[DNS] Resolver initialized with cache");
}

/// DNS önbelleğinden belirtilen kayıt türü için girdi arar.
///
/// Girdi varsa ve TTL dolmamışsa döner, yoksa veya süresi geçmişse `None` döner.
pub fn get_cached(
    name: &str,
    record_type: DnsRecordType,
    current_time: u64,
) -> Option<DnsCacheEntry> {
    let key = format!("{}:{}", name, record_type as u16);
    let cache = DNS_CACHE.lock();
    if let Some(entry) = cache.get(&key) {
        if !entry.is_expired(current_time) {
            return Some(entry.clone());
        }
    }
    None
}

/// DNS önbelleğine yeni bir girdi ekler.
///
/// Aynı anahtar için mevcut girdi varsa üzerine yazar.
/// `obtained_at` alanı geçerli zaman damgasıyla doldurulur.
pub fn cache_entry(entry: DnsCacheEntry, current_time: u64) {
    let key = format!("{}:{}", entry.name, entry.record_type as u16);
    let mut cache = DNS_CACHE.lock();
    cache.insert(
        key,
        DnsCacheEntry {
            obtained_at: current_time,
            ..entry
        },
    );
}

/// Tüm DNS önbelleğini temizler.
pub fn clear_cache() {
    DNS_CACHE.lock().clear();
}

/// DNS önbelleğindeki girdi sayısını döner.
pub fn cache_size() -> usize {
    DNS_CACHE.lock().len()
}

/// Bir alan adını DNS sunucusuna sorarak IPv4 adresine çözümler.
///
/// Çalışma mantığı:
/// 1. Önce DNS önbelleğini kontrol et — geçerli girdi varsa hemen döndür
/// 2. Önbellekte yoksa UDP soketi oluştur ve geçici porta bağlan
/// 3. DNS sorgusu oluştur (Header + Question)
/// 4. DNS sunucusuna gönder (UDP port 53)
/// 5. Yanıtı al ve ayrıştır
/// 6. CNAME zinciri varsa takip et (maks. 8 atlama)
/// 7. Sonucu önbelleğe kaydet ve ilk A kaydını döndür
pub fn resolve(hostname: &str, dns_server: Ipv4Addr) -> Result<Ipv4Addr, NetError> {
    resolve_with_depth(hostname, dns_server, 0)
}

/// CNAME zinciri takibinde sonsuz döngüyü önlemek için maksimum derinlik.
const MAX_CNAME_DEPTH: u8 = 8;

/// İç çözümleme fonksiyonu — CNAME takibi için derinlik sayacı içerir.
fn resolve_with_depth(
    hostname: &str,
    dns_server: Ipv4Addr,
    depth: u8,
) -> Result<Ipv4Addr, NetError> {
    if depth >= MAX_CNAME_DEPTH {
        crate::serial_println!(
            "[DNS] CNAME chain depth limit ({}) exceeded for {}",
            MAX_CNAME_DEPTH,
            hostname
        );
        return Err(NetError::ProtocolError);
    }

    // ── 1. Önbellek kontrolü ──────────────────────────────────────────
    let current_time = crate::interrupts::get_ticks();
    if let Some(cached) = get_cached(hostname, DnsRecordType::A, current_time) {
        if let Some(ip) = cached.as_ipv4() {
            crate::serial_println!(
                "[DNS] Cache hit for {} -> {}",
                hostname,
                super::socket::format_ipv4(ip)
            );
            return Ok(ip);
        }
    }

    // ── 2. UDP soketi oluştur ─────────────────────────────────────────
    let sock_id = socket(
        super::socket::AddressFamily::IPV4,
        super::socket::SocketType::DGRAM,
        super::socket::Protocol::UDP,
    )?;

    // Geçici porta bağlan (0 = sistem otomatik port atar)
    bind(sock_id, SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(0)))?;

    // DNS sorgusu oluştur
    let (id, secure_id) = crate::random::secure_u16();
    if !secure_id {
        crate::serial_println!(
            "[DNS] secure RNG unavailable; query id uses entropy-mixed fallback"
        );
    }
    let header = DnsHeader::new_query(id);
    let question = DnsQuestion::new(hostname);

    let mut buf = vec![0u8; 512]; // DNS mesajları 512 byte ile sınırlıdır (UDP için)
    header.serialize(&mut buf)?;
    let q_offset = DnsHeader::SIZE;
    let q_len = question.serialize(&mut buf[q_offset..])?;
    let total_len = q_offset + q_len;

    // DNS sunucusuna sorgu gönder (UDP port 53)
    let dst = SocketAddr::new(dns_server, Port(DNS_PORT));
    sendto(sock_id, &buf[..total_len], dst, 0)?;

    crate::serial_println!("[DNS] Query sent for {} (depth={})", hostname, depth);

    // Yanıtı al
    let mut resp_buf = vec![0u8; 512];
    let (len, _) = recvfrom(sock_id, &mut resp_buf, 0)?;
    let resp_data = &resp_buf[..len];

    close(sock_id)?;

    // Yanıt başlığını ayrıştır
    let resp_header = DnsHeader::parse(resp_data)?;

    if resp_header.id != id || !resp_header.is_response() || !resp_header.is_valid() {
        return Err(NetError::ProtocolError);
    }

    // Soru bölümünü atla (ayrıştırma yapılmıyor, sadece ilerle)
    let mut offset = DnsHeader::SIZE;
    for _ in 0..resp_header.qdcount {
        parse_dns_name(resp_data, &mut offset)?;
        if offset + 4 > len {
            return Err(NetError::InvalidPacket);
        }
        offset += 4;
    }

    // ── 3. Yanıt kayıtlarını ayrıştır ─────────────────────────────────
    // İlk geçişte A kaydı arıyoruz; bulamazsak CNAME takip ediyoruz.
    let mut cname_target: Option<String> = None;

    for _ in 0..resp_header.ancount {
        let (answer, new_offset) = DnsAnswer::parse(resp_data, offset)?;
        offset = new_offset;

        // A kaydı bulundu — önbelleğe kaydet ve döndür
        if let Some(ip) = answer.as_ipv4() {
            let ts = crate::interrupts::get_ticks();
            cache_entry(
                DnsCacheEntry {
                    name: String::from(hostname),
                    record_type: DnsRecordType::A,
                    data: answer.data.clone(),
                    ttl: answer.ttl,
                    obtained_at: ts,
                },
                ts,
            );
            crate::serial_println!(
                "[DNS] {} -> {} (TTL={}s)",
                hostname,
                super::socket::format_ipv4(ip),
                answer.ttl
            );
            return Ok(ip);
        }

        // CNAME kaydı — hedef alan adını çıkar
        if answer.atype == DnsRecordType::CNAME as u16 && cname_target.is_none() {
            // CNAME RDATA'sı wire-format alan adıdır; ayrıştır
            if let Ok(target) = parse_name_from_rdata(
                &resp_buf,
                answer.data.as_slice(),
                offset.saturating_sub(answer.data.len()),
            ) {
                crate::serial_println!("[DNS] CNAME {} -> {}", hostname, target);
                // CNAME'i de önbelleğe al
                let ts = crate::interrupts::get_ticks();
                cache_entry(
                    DnsCacheEntry {
                        name: String::from(hostname),
                        record_type: DnsRecordType::CNAME,
                        data: target.as_bytes().to_vec(),
                        ttl: answer.ttl,
                        obtained_at: ts,
                    },
                    ts,
                );
                cname_target = Some(target);
            }
        }
    }

    // ── 4. CNAME takibi ───────────────────────────────────────────────
    if let Some(target) = cname_target {
        return resolve_with_depth(&target, dns_server, depth + 1);
    }

    Err(NetError::HostUnreachable) // A kaydı bulunamadı
}

/// DNS wire-format RDATA içindeki alan adını metne çevirir.
///
/// CNAME RDATA'sı, tam paketin sıkıştırma işaretçileri ile birlikte
/// kodlanmış bir alan adı içerir. Bu yardımcı fonksiyon bunu çözer.
fn parse_name_from_rdata(
    full_packet: &[u8],
    _rdata: &[u8],
    rdata_offset: usize,
) -> Result<String, NetError> {
    let mut pos = rdata_offset;
    let parsed = parse_dns_name(full_packet, &mut pos)?;
    if parsed.is_empty() {
        return Err(NetError::InvalidPacket);
    }
    Ok(parsed)
}

fn parse_dns_name(data: &[u8], pos: &mut usize) -> Result<String, NetError> {
    let mut cursor = *pos;
    let mut name = String::new();
    let mut jumped = false;
    let mut jumped_pos = 0usize;
    let mut jumps = 0usize;

    loop {
        if cursor >= data.len() {
            return Err(NetError::InvalidPacket);
        }

        let len = data[cursor] as usize;
        cursor += 1;

        if len == 0 {
            break;
        }

        if (len & 0xC0) == 0xC0 {
            if cursor >= data.len() || jumps >= MAX_DNS_COMPRESSION_JUMPS {
                return Err(NetError::InvalidPacket);
            }
            let offset = ((len & 0x3F) << 8) | data[cursor] as usize;
            if offset >= data.len() {
                return Err(NetError::InvalidPacket);
            }
            if !jumped {
                jumped_pos = cursor + 1;
                jumped = true;
            }
            cursor = offset;
            jumps += 1;
            continue;
        }

        if (len & 0xC0) != 0 || len > MAX_DNS_LABEL_LEN || cursor + len > data.len() {
            return Err(NetError::InvalidPacket);
        }

        let next_len = name.len() + len + usize::from(!name.is_empty());
        if next_len > MAX_DNS_NAME_LEN {
            return Err(NetError::InvalidPacket);
        }

        if !name.is_empty() {
            name.push('.');
        }
        for &byte in &data[cursor..cursor + len] {
            name.push(byte as char);
        }
        cursor += len;
    }

    *pos = if jumped { jumped_pos } else { cursor };
    Ok(name)
}

/// Varsayılan DNS sunucusunu kullanarak alan adını çözümler.
///
/// DHCP ile alınan DNS sunucusunu dener, yoksa Google DNS (8.8.8.8) kullanır.
pub fn resolve_default(hostname: &str) -> Result<Ipv4Addr, NetError> {
    let config = super::get_config();

    if let Some(dns) = config.dns_servers.first() {
        resolve(hostname, Ipv4Addr::from_bytes(*dns))
    } else {
        // Fallback: Google Public DNS
        resolve(hostname, Ipv4Addr::new(8, 8, 8, 8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_query_id_uses_secure_u16_range() {
        let mut observed_nonzero = false;
        for _ in 0..64 {
            let (id, _) = crate::random::secure_u16();
            if id != 0 {
                observed_nonzero = true;
            }
        }
        assert!(observed_nonzero);
    }

    #[test]
    fn parse_dns_name_supports_valid_compression_pointer() {
        let packet = [
            0x03, b'w', b'w', b'w', 0xC0, 0x06, 0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
            0x03, b'c', b'o', b'm', 0x00,
        ];
        let mut pos = 0usize;

        let parsed = parse_dns_name(&packet, &mut pos).expect("valid compressed name");

        assert_eq!(parsed, "www.example.com");
        assert_eq!(pos, 6);
    }

    #[test]
    fn parse_dns_name_rejects_self_referential_pointer_loop() {
        let packet = [0xC0, 0x00];
        let mut pos = 0usize;

        assert!(matches!(
            parse_dns_name(&packet, &mut pos),
            Err(NetError::InvalidPacket)
        ));
    }

    #[test]
    fn parse_dns_name_rejects_mutual_pointer_loop() {
        let packet = [0xC0, 0x02, 0xC0, 0x00];
        let mut pos = 0usize;

        assert!(matches!(
            parse_dns_name(&packet, &mut pos),
            Err(NetError::InvalidPacket)
        ));
    }

    #[test]
    fn parse_name_from_rdata_rejects_pointer_loop() {
        let packet = [0xC0, 0x00];

        assert!(matches!(
            parse_name_from_rdata(&packet, &packet, 0),
            Err(NetError::InvalidPacket)
        ));
    }

    #[test]
    fn parse_dns_name_rejects_out_of_bounds_start_cursor() {
        let packet = [0x00u8];
        let mut pos = packet.len();

        assert!(matches!(
            parse_dns_name(&packet, &mut pos),
            Err(NetError::InvalidPacket)
        ));
    }
}
