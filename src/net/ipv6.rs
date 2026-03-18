//! # IPv6 Protokolü (Internet Protocol version 6)
//!
//! IPv6 başlık yapısı ve adres işleme.
//!
//! ## IPv6 Nedir?
//!
//! IPv4'ün 32-bit adres alanı tükenmesiyle geliştirilmiş, 128-bit adres
//! uzayına sahip yeni nesil internet protokolüdür. RFC 2460 ile tanımlanmıştır.
//!
//! ## IPv6 Başlık Yapısı (40 bayt sabit boyut)
//!
//! ```text
//! 0                   1                   2                   3
//! 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |Version| Traf. Cls |           Flow Label                      |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |         Payload Length        |  Next Header  |   Hop Limit   |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                                                               |
//! +                     Kaynak IPv6 Adresi                        +
//! |                       (128 bit)                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                                                               |
//! +                    Hedef IPv6 Adresi                          +
//! |                       (128 bit)                               |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! ## IPv6 Adres Türleri
//!
//! ```text
//! Unicast   : Tek bir arabirime ait adres
//! Multicast : Bir grup arabirime ait adres (ff00::/8)
//! Anycast   : En yakın arabirime yönlendirilen adres
//!
//! Özel Adresler:
//!   ::           = 0.0.0.0 (belirsiz / unspecified)
//!   ::1          = 127.0.0.1 (geri döngü / loopback)
//!   fe80::/10    = Bağlantı-yerel (link-local)
//!   fc00::/7     = Benzersiz-yerel (unique-local, RFC 4193)
//!   2000::/3     = Global unicast
//!   ::ffff:0:0/96 = IPv4 eşlemeli (IPv4-mapped)
//! ```
//!
//! ## Bu Modüldeki İçerikler
//!
//! - `Ipv6Addr`           : 128-bit IPv6 adres yapısı
//! - `Ipv6Header`         : 40 baytlık sabit başlık
//! - `Ipv6NextHeader`     : Sonraki başlık türleri (TCP=6, UDP=17, ICMPv6=58)
//! - `Icmpv6Type`         : ICMPv6 mesaj tipleri (NDP, ping...)
//! - `RouterSolicitation` : Yönlendirici Talep (RS) mesajı
//! - `RouterAdvertisement`: Yönlendirici Duyuru (RA) mesajı
//! - `SlaacState`         : Durumsuz Adres Otokonfigürasyonu
//! - `Dhcpv6Client`       : DHCPv6 istemci durumu
//! - `NeighborSolicitation`: Komşu Bulma (NDP) protokolü

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::str::FromStr;
use spin::Mutex;

// ============================================================================
// IPv6 ADRESİ
// ============================================================================
//
// IPv6 adresi 128 bit uzunluğundadır ve genellikle 8 adet 16-bit blok
// olarak gösterilir. Bloklar birbirinden ':' ile ayrılır.
//
// Örnek:  2001:0db8:85a3:0000:0000:8a2e:0370:7334
//
// Kısaltma kuralları:
//   1) Baştaki sıfırlar atılabilir: 0db8 → db8
//   2) Ardışık sıfır blokları '::' ile gösterilir (yalnızca bir kez):
//      2001:db8::1  (2001:0db8:0000:0000:0000:0000:0000:0001)

/// IPv6 adresi (128 bit = 16 bayt)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    /// Belirsiz (unspecified) adres `::` — IPv4'teki 0.0.0.0 karşılığı
    pub const UNSPECIFIED: Self = Ipv6Addr([0; 16]);

    /// Geri döngü (loopback) adresi `::1` — IPv4'teki 127.0.0.1 karşılığı
    pub const LOOPBACK: Self = Ipv6Addr([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    /// Bağlantı-yerel (link-local) ön eki `fe80::/64`
    /// Bu adresler yalnızca aynı fiziksel bağlantı (link) üzerinde geçerlidir.
    pub const LINK_LOCAL_PREFIX: [u8; 8] = [0xfe, 0x80, 0, 0, 0, 0, 0, 0];

    /// Bayt dizisinden yeni bir IPv6 adresi oluşturur
    pub const fn new(bytes: [u8; 16]) -> Self {
        Ipv6Addr(bytes)
    }

    /// 8 adet 16-bit segment (grup) dizisinden IPv6 adresi oluşturur.
    /// Her segment big-endian (büyük-önce) byte sırasıyla 2 bayta dönüştürülür.
    pub const fn from_segments(segments: [u16; 8]) -> Self {
        Ipv6Addr([
            (segments[0] >> 8) as u8,
            (segments[0] & 0xFF) as u8,
            (segments[1] >> 8) as u8,
            (segments[1] & 0xFF) as u8,
            (segments[2] >> 8) as u8,
            (segments[2] & 0xFF) as u8,
            (segments[3] >> 8) as u8,
            (segments[3] & 0xFF) as u8,
            (segments[4] >> 8) as u8,
            (segments[4] & 0xFF) as u8,
            (segments[5] >> 8) as u8,
            (segments[5] & 0xFF) as u8,
            (segments[6] >> 8) as u8,
            (segments[6] & 0xFF) as u8,
            (segments[7] >> 8) as u8,
            (segments[7] & 0xFF) as u8,
        ])
    }

    /// Adresin ham bayt dizisini döndürür (16 bayt)
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Adresi 8 adet 16-bit segmente (gruba) dönüştürür.
    /// Örnek: 2001:db8::1 → [0x2001, 0x0db8, 0, 0, 0, 0, 0, 1]
    pub fn segments(&self) -> [u16; 8] {
        [
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
            u16::from_be_bytes([self.0[4], self.0[5]]),
            u16::from_be_bytes([self.0[6], self.0[7]]),
            u16::from_be_bytes([self.0[8], self.0[9]]),
            u16::from_be_bytes([self.0[10], self.0[11]]),
            u16::from_be_bytes([self.0[12], self.0[13]]),
            u16::from_be_bytes([self.0[14], self.0[15]]),
        ]
    }

    /// Adresin belirsiz `::` olup olmadığını kontrol eder
    pub fn is_unspecified(&self) -> bool {
        self.0 == [0; 16]
    }

    /// Adresin geri döngü `::1` olup olmadığını kontrol eder
    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }

    /// Bağlantı-yerel adres `fe80::/10` olup olmadığını kontrol eder.
    /// fe80:: - febf:: aralığındaki adresler bağlantı-yereldir.
    pub fn is_link_local(&self) -> bool {
        (self.0[0] & 0xFF) == 0xFE && (self.0[1] & 0xC0) == 0x80
    }

    /// Benzersiz-yerel adres `fc00::/7` olup olmadığını kontrol eder.
    /// RFC 4193 ile tanımlanmış, özel ağlar için kullanılır (IPv4'teki RFC 1918 gibi).
    pub fn is_unique_local(&self) -> bool {
        (self.0[0] & 0xFE) == 0xFC
    }

    /// Global unicast adres `2000::/3` olup olmadığını kontrol eder.
    /// İnternette yönlendirilebilen genel adreslerdir.
    pub fn is_global(&self) -> bool {
        // Global unicast: 2000::/3 (en yüksek 3 bit = 001)
        (self.0[0] & 0xE0) == 0x20
    }

    /// Çok noktaya yayın (multicast) adresi `ff00::/8` olup olmadığını kontrol eder.
    /// İlk bayt 0xFF ise multicast'tir.
    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xFF
    }

    /// IPv4-eşlemeli adres `::ffff:0:0/96` olup olmadığını kontrol eder.
    /// IPv4 ve IPv6 geçiş mekanizmasında kullanılır.
    /// Örnek: ::ffff:192.168.1.1
    pub fn is_ipv4_mapped(&self) -> bool {
        self.0[0..10] == [0; 10] && self.0[10..12] == [0xFF, 0xFF]
    }

    /// IPv4-eşlemeli ise karşılık gelen IPv4 adresini döndürür, değilse `None`
    pub fn to_ipv4_mapped(&self) -> Option<super::Ipv4Addr> {
        if self.is_ipv4_mapped() {
            Some(super::Ipv4Addr::from_bytes([
                self.0[12], self.0[13], self.0[14], self.0[15],
            ]))
        } else {
            None
        }
    }

    /// Bir IPv4 adresinden IPv4-eşlemeli IPv6 adresi oluşturur.
    /// Sonuç: `::ffff:a.b.c.d` formatında bir adres
    pub fn from_ipv4_mapped(ipv4: super::Ipv4Addr) -> Self {
        let bytes = ipv4.as_bytes();
        Ipv6Addr([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, bytes[0], bytes[1], bytes[2], bytes[3],
        ])
    }

    /// Bağlantı-yerel adresler için kapsam kimliği (scope ID) döndürür.
    /// Scope ID, hangi ağ arayüzü üzerinden erişileceğini belirtir.
    pub fn scope_id(&self) -> Option<u32> {
        if self.is_link_local() {
            Some(u32::from_be_bytes([
                self.0[12], self.0[13], self.0[14], self.0[15],
            ]))
        } else {
            None
        }
    }

    /// Adresi insan okunabilir IPv6 metin formatına çevirir.
    /// RFC 5952'ye göre çift kolon `::` sıkıştırması uygulanır.
    pub fn to_string(&self) -> String {
        if self.is_ipv4_mapped() {
            if let Some(ipv4) = self.to_ipv4_mapped() {
                return format!(
                    "::ffff:{}.{}.{}.{}",
                    ipv4.0[0], ipv4.0[1], ipv4.0[2], ipv4.0[3]
                );
            }
        }

        if self.is_loopback() {
            return String::from("::1");
        }

        if self.is_unspecified() {
            return String::from("::");
        }

        let segments = self.segments();

        // RFC 5952 §4.2: `::` için ardışık sıfır bloklarından en uzununun başlangıcını bul.
        let mut longest_start = 0;
        let mut longest_len = 0;
        let mut current_start = 0;
        let mut current_len = 0;

        for i in 0..8 {
            if segments[i] == 0 {
                if current_len == 0 {
                    current_start = i;
                }
                current_len += 1;
                if current_len > longest_len {
                    longest_len = current_len;
                    longest_start = current_start;
                }
            } else {
                current_len = 0;
            }
        }

        let mut result = String::new();
        let mut i = 0;

        while i < 8 {
            if i == longest_start && longest_len > 1 {
                if i == 0 {
                    result.push(':');
                }
                result.push(':');
                i += longest_len;
            } else {
                if i > 0 {
                    result.push(':');
                }
                result.push_str(&format!("{:x}", segments[i]));
                i += 1;
            }
        }

        result
    }
}

impl FromStr for Ipv6Addr {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut segments = [0u16; 8];
        let mut seg_idx = 0;
        let mut double_colon_pos = None;
        let mut parts = s.split(':');
        let mut before_double_colon = alloc::vec::Vec::new();
        let mut after_double_colon = alloc::vec::Vec::new();
        let mut found_double_colon = false;

        // Handle special case of leading ::
        let chars: alloc::vec::Vec<char> = s.chars().collect();
        let mut idx = 0;

        while idx < chars.len() {
            if chars[idx] == ':' {
                if idx + 1 < chars.len() && chars[idx + 1] == ':' {
                    found_double_colon = true;
                    double_colon_pos = Some(before_double_colon.len());
                    idx += 2;
                    // Parse rest after ::
                    while idx < chars.len() {
                        let mut hex_str = String::new();
                        while idx < chars.len() && chars[idx] != ':' {
                            hex_str.push(chars[idx]);
                            idx += 1;
                        }
                        if !hex_str.is_empty() {
                            if let Ok(val) = u16::from_str_radix(&hex_str, 16) {
                                after_double_colon.push(val);
                            }
                        }
                        idx += 1;
                    }
                    break;
                } else {
                    idx += 1;
                }
            } else {
                let mut hex_str = String::new();
                while idx < chars.len() && chars[idx] != ':' {
                    hex_str.push(chars[idx]);
                    idx += 1;
                }
                if let Ok(val) = u16::from_str_radix(&hex_str, 16) {
                    before_double_colon.push(val);
                }
                idx += 1;
            }
        }

        if !found_double_colon {
            // No ::, must have exactly 8 segments
            let all_parts: alloc::vec::Vec<u16> = s
                .split(':')
                .filter(|p| !p.is_empty())
                .filter_map(|p| u16::from_str_radix(p, 16).ok())
                .collect();

            if all_parts.len() != 8 {
                return Err(());
            }
            segments.copy_from_slice(&all_parts);
        } else {
            let before_len = before_double_colon.len();
            let after_len = after_double_colon.len();
            let zero_count = 8 - before_len - after_len;

            if zero_count == 0 && before_len + after_len != 8 {
                return Err(());
            }

            for (i, &val) in before_double_colon.iter().enumerate() {
                segments[i] = val;
            }
            for (i, &val) in after_double_colon.iter().enumerate() {
                segments[before_len + zero_count + i] = val;
            }
        }

        Ok(Ipv6Addr::from_segments(segments))
    }
}

impl Default for Ipv6Addr {
    fn default() -> Self {
        Self::UNSPECIFIED
    }
}

// ============================================================================
// IPv6 BAŞLIĞI (HEADER)
// ============================================================================
//
// IPv6 başlığı daima 40 bayttır (IPv4'ün değişken uzunluklu başlığının aksine).
// Uzantı başlıkları (extension headers) ayrı paketler olarak eklenir.
//
// Başlık alanları:
//   version        : Daima 6
//   traffic_class  : Hizmet kalitesi (QoS) için 8 bit
//   flow_label     : Akış tanımlaması için 20 bit (QoS, multipath)
//   payload_len    : Başlık sonrasındaki veri boyutu (bayt)
//   next_header    : Sonraki başlık türü (IPv4'teki protocol alanı gibi)
//   hop_limit      : İzin verilen maksimum yönlendirici sayısı (IPv4 TTL gibi)

/// IPv6 başlığı (40 bayt sabit boyut)
#[derive(Clone, Copy, Debug)]
pub struct Ipv6Header {
    pub version: u8,       // 4 bit, daima 6
    pub traffic_class: u8, // 8 bit, trafik sınıfı (QoS)
    pub flow_label: u32,   // 20 bit, akış etiketi
    pub payload_len: u16,  // Yük uzunluğu (başlık hariç, bayt cinsinden)
    pub next_header: u8,   // Sonraki başlık türü (IPv4 protocol alanı gibi)
    pub hop_limit: u8,     // Maksimum atlama sayısı (IPv4 TTL gibi)
    pub src: Ipv6Addr,
    pub dst: Ipv6Addr,
}

impl Ipv6Header {
    pub const SIZE: usize = 40;

    pub fn new(src: Ipv6Addr, dst: Ipv6Addr, next_header: u8, payload_len: u16) -> Self {
        Ipv6Header {
            version: 6,
            traffic_class: 0,
            flow_label: 0,
            payload_len,
            next_header,
            hop_limit: 64,
            src,
            dst,
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self, super::NetError> {
        if data.len() < Self::SIZE {
            return Err(super::NetError::InvalidPacket);
        }

        let version = (data[0] >> 4) & 0x0F;
        if version != 6 {
            return Err(super::NetError::InvalidPacket);
        }

        let traffic_class = ((data[0] & 0x0F) << 4) | ((data[1] >> 4) & 0x0F);
        let flow_label =
            ((data[1] as u32 & 0x0F) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);

        let payload_len = u16::from_be_bytes([data[4], data[5]]);
        let next_header = data[6];
        let hop_limit = data[7];

        let mut src_bytes = [0u8; 16];
        src_bytes.copy_from_slice(&data[8..24]);
        let src = Ipv6Addr(src_bytes);

        let mut dst_bytes = [0u8; 16];
        dst_bytes.copy_from_slice(&data[24..40]);
        let dst = Ipv6Addr(dst_bytes);

        Ok(Ipv6Header {
            version,
            traffic_class,
            flow_label,
            payload_len,
            next_header,
            hop_limit,
            src,
            dst,
        })
    }

    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), super::NetError> {
        if buf.len() < Self::SIZE {
            return Err(super::NetError::BufferFull);
        }

        // Version (4 bit) + Traffic class (8 bit) + Flow label (20 bit)
        // İlk 32-bit sözcük bit düzeyinde paketleme:
        //   [7:4] = version, [3:0][15:12] = traffic_class, [11:0] = flow_label
        buf[0] = (self.version << 4) | ((self.traffic_class >> 4) & 0x0F);
        buf[1] = ((self.traffic_class & 0x0F) << 4) | ((self.flow_label >> 16) as u8 & 0x0F);
        buf[2] = (self.flow_label >> 8) as u8;
        buf[3] = self.flow_label as u8;

        buf[4..6].copy_from_slice(&self.payload_len.to_be_bytes());
        buf[6] = self.next_header;
        buf[7] = self.hop_limit;

        buf[8..24].copy_from_slice(&self.src.0);
        buf[24..40].copy_from_slice(&self.dst.0);

        Ok(())
    }
}

/// IPv6 sonraki başlık (next header) türleri.
/// Bu alan IPv4'teki "protocol" alanının karşılığıdır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ipv6NextHeader {
    HopByHop = 0,
    Tcp = 6,
    Udp = 17,
    Icmpv6 = 58,
    NoNextHeader = 59,
    DestinationOptions = 60,
    Fragment = 44,
    Authentication = 51,
    EncapsulatingSecurityPayload = 50,
}

impl Ipv6NextHeader {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Ipv6NextHeader::HopByHop,
            6 => Ipv6NextHeader::Tcp,
            17 => Ipv6NextHeader::Udp,
            58 => Ipv6NextHeader::Icmpv6,
            59 => Ipv6NextHeader::NoNextHeader,
            60 => Ipv6NextHeader::DestinationOptions,
            44 => Ipv6NextHeader::Fragment,
            51 => Ipv6NextHeader::Authentication,
            50 => Ipv6NextHeader::EncapsulatingSecurityPayload,
            _ => Ipv6NextHeader::HopByHop,
        }
    }
}

// ============================================================================
// IPv6 PAKETİ
// ============================================================================
//
// Bir IPv6 paketi = 40 bayt sabit başlık + değişken uzunluklu yük (payload)
// IPv4'teki parçalama (fragmentation) başlığa gömülü değil, uzantı başlığı olarak ayrıdır.

/// IPv6 paketi (başlık + yük)
#[derive(Clone, Debug)]
pub struct Ipv6Packet {
    pub header: Ipv6Header,
    pub payload: alloc::vec::Vec<u8>,
}

impl Ipv6Packet {
    pub fn new(header: Ipv6Header, payload: &[u8]) -> Self {
        Ipv6Packet {
            header,
            payload: alloc::vec::Vec::from(payload),
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self, super::NetError> {
        let header = Ipv6Header::parse(data)?;
        let payload_start = Ipv6Header::SIZE;
        let payload_end = payload_start + header.payload_len as usize;

        if payload_end > data.len() {
            return Err(super::NetError::InvalidPacket);
        }

        Ok(Ipv6Packet {
            header,
            payload: alloc::vec::Vec::from(&data[payload_start..payload_end]),
        })
    }

    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec![0u8; Ipv6Header::SIZE + self.payload.len()];
        self.header.serialize(&mut buf).ok();
        buf[Ipv6Header::SIZE..].copy_from_slice(&self.payload);
        buf
    }

    pub fn total_len(&self) -> usize {
        Ipv6Header::SIZE + self.payload.len()
    }
}

// ============================================================================
// IPv6 UZANTI BAŞLIKLARI (EXTENSION HEADERS)
// ============================================================================
//
// IPv6, seçenekler için sabit başlık içine alan koymak yerine uzantı başlıkları
// kullanır. Her uzantı başlığı kendi "next_header" alanıyla bir sonrakine işaret eder.
//
// Uzantı Başlığı Zinciri:
//   IPv6 Header → HopByHop Opt. → Routing → Fragment → Dest. Opt. → TCP/UDP
//
// Uzantı başlıkları parse/serialize düzeyinde kapsanır; tam route/fragment parity ayrı çalışmadır.

/// Atlama-Atlama (Hop-by-Hop) Seçenekleri başlığı.
/// Rota üzerindeki her yönlendirici bu başlığı işlemek zorundadır.
#[derive(Clone, Debug)]
pub struct HopByHopHeader {
    pub next_header: u8,
    pub hdr_ext_len: u8,
    pub options: alloc::vec::Vec<u8>,
}

impl HopByHopHeader {
    /// Hop-by-Hop uzantı başlığını ham bayt dizisinden ayrıştırır.
    ///
    /// Dönüş: `(ayrıştırılmış başlık, tüketilen bayt sayısı)` veya `None`.
    /// `hdr_ext_len` alanı 8 baytlık birimler cinsindedir; toplam boyut = (hdr_ext_len + 1) * 8.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 2 {
            return None;
        }
        let next_header = data[0];
        let hdr_ext_len = data[1];
        // Toplam uzantı başlığı boyutu: (hdr_ext_len + 1) * 8 bayt
        let total_len = (hdr_ext_len as usize + 1) * 8;
        if data.len() < total_len {
            return None;
        }
        // İlk 2 bayt (next_header + hdr_ext_len) sonrasındaki kalan baytlar seçenek verisidir
        let options = data[2..total_len].to_vec();
        Some((
            HopByHopHeader {
                next_header,
                hdr_ext_len,
                options,
            },
            total_len,
        ))
    }

    /// Hop-by-Hop uzantı başlığını bayt dizisine seri hale getirir.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(self.next_header);
        buf.push(self.hdr_ext_len);
        buf.extend_from_slice(&self.options);
        // Toplam boyutu (hdr_ext_len + 1) * 8 olacak şekilde sıfırla doldur
        let total_len = (self.hdr_ext_len as usize + 1) * 8;
        while buf.len() < total_len {
            buf.push(0);
        }
        buf
    }
}

/// Parçalama (Fragment) başlığı.
/// IPv6'da parçalama yalnızca kaynak tarafından yapılır; yönlendiriciler parçalamaz.
#[derive(Clone, Debug)]
pub struct FragmentHeader {
    pub next_header: u8,
    pub fragment_offset: u16, // 13 bit, parça ofseti (8 baytlık birimler halinde)
    pub more_fragments: bool,
    pub identification: u32,
}

impl FragmentHeader {
    /// Fragment başlığının sabit boyutu: 8 bayt
    pub const SIZE: usize = 8;

    /// Fragment başlığını ham bayt dizisinden ayrıştırır.
    ///
    /// Fragment başlığı formatı (8 bayt):
    ///   [0]     next_header
    ///   [1]     reserved
    ///   [2..4]  fragment_offset (13 bit) | res (2 bit) | M flag (1 bit)
    ///   [4..8]  identification (32 bit)
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < Self::SIZE {
            return None;
        }
        let next_header = data[0];
        // data[1] reserved
        let frag_field = u16::from_be_bytes([data[2], data[3]]);
        let fragment_offset = frag_field >> 3; // Üst 13 bit
        let more_fragments = (frag_field & 0x01) != 0; // En düşük bit = M bayrağı
        let identification = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        Some((
            FragmentHeader {
                next_header,
                fragment_offset,
                more_fragments,
                identification,
            },
            Self::SIZE,
        ))
    }

    /// Fragment başlığını bayt dizisine seri hale getirir (8 bayt).
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.push(self.next_header);
        buf.push(0); // reserved
        let frag_field: u16 =
            (self.fragment_offset << 3) | (if self.more_fragments { 1 } else { 0 });
        buf.extend_from_slice(&frag_field.to_be_bytes());
        buf.extend_from_slice(&self.identification.to_be_bytes());
        buf
    }
}

/// Uzantı başlık zincirini yürüyerek son yükün (payload) türünü ve ofsetini bulur.
///
/// `data`: IPv6 yükünün başlangıcı (IPv6 sabit başlığından sonraki ilk bayt).
/// `start_next_header`: IPv6 sabit başlığının `next_header` alanı.
///
/// Dönüş: `(son_next_header, yüke_olan_ofset)` — yani gerçek üst katman
/// protokolünün (TCP/UDP/ICMPv6) türü ve `data` içindeki başlangıç konumu.
pub fn walk_extension_headers(data: &[u8], start_next_header: u8) -> (u8, usize) {
    let mut nh = start_next_header;
    let mut offset: usize = 0;

    loop {
        match nh {
            // Hop-by-Hop (0), Destination Options (60), Routing (43)
            0 | 60 | 43 => {
                if offset + 2 > data.len() {
                    break;
                }
                let ext_next = data[offset];
                let ext_len = data[offset + 1] as usize;
                let total = (ext_len + 1) * 8;
                if offset + total > data.len() {
                    break;
                }
                nh = ext_next;
                offset += total;
            }
            // Fragment Header (44) — sabit 8 bayt
            44 => {
                if offset + FragmentHeader::SIZE > data.len() {
                    break;
                }
                nh = data[offset]; // next_header
                offset += FragmentHeader::SIZE;
            }
            // Authentication Header (51)
            51 => {
                if offset + 2 > data.len() {
                    break;
                }
                let ext_next = data[offset];
                let ext_len = data[offset + 1] as usize;
                let total = (ext_len + 2) * 4;
                if offset + total > data.len() {
                    break;
                }
                nh = ext_next;
                offset += total;
            }
            // NoNextHeader (59) veya üst katman protokolü (TCP=6, UDP=17, ICMPv6=58, …)
            _ => break,
        }
    }

    (nh, offset)
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================
//
// EUI-64: MAC adresinden 64-bit arabirim tanımlayıcısı oluşturma yöntemi.
//
// MAC adresi (48 bit):   XX:XX:XX:YY:YY:YY
//                                 ↓
// EUI-64 (64 bit):       XX:XX:XX:FF:FE:YY:YY:YY
//   + 7. bitin tersine çevrilmesi (universal/local bit)
//
// Bu tanımlayıcı, link-local ve SLAAC global adreslerinin ikinci yarısını oluşturur.

/// MAC adresinden EUI-64 yöntemiyle bağlantı-yerel (link-local) IPv6 adresi üretir.
/// Sonuç: `fe80::` ön eki + EUI-64 arabirim tanımlayıcısı
pub fn link_local_from_mac(mac: super::MacAddr) -> Ipv6Addr {
    let bytes = mac.as_bytes();

    // EUI-64: Ortaya FF:FE ekle (48-bit → 64-bit genişletme)
    let mut addr = [0u8; 16];
    addr[0] = 0xFE;
    addr[1] = 0x80;
    // Bytes 2-7 sıfır (link-local prefix zaten fe80:: olduğu için)
    addr[8] = bytes[0] ^ 0x02; // Universal/local bitini tersine çevir (EUI-64 standardı)
    addr[9] = bytes[1];
    addr[10] = bytes[2];
    addr[11] = 0xFF;
    addr[12] = 0xFE;
    addr[13] = bytes[3];
    addr[14] = bytes[4];
    addr[15] = bytes[5];

    Ipv6Addr(addr)
}

/// Bir IPv6 adresi için talep-düğüm (solicited-node) çok noktaya yayın adresi üretir.
/// Komşu Bulma Protokolü'nde (NDP) ARP'nin IPv6 karşılığı olarak kullanılır.
/// Format: `ff02::1:ff00:0/104` ön eki + adresin son 24 biti
pub fn solicited_node_multicast(addr: &Ipv6Addr) -> Ipv6Addr {
    Ipv6Addr([
        0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xFF, addr.0[13], addr.0[14], addr.0[15],
    ])
}

// ============================================================================
// ICMPv6 (IPv6 İçin İnternet Kontrol Mesaj Protokolü)
// ============================================================================
//
// ICMPv6, IPv6'nın ayrılmaz bir parçasıdır (RFC 4443). IPv4'teki ICMP, ARP ve
// IGMP protokollerinin birleşimi gibi düşünülebilir.
//
// Kullanım alanları:
//   - Hata bildirimi (Destination Unreachable, Time Exceeded...)
//   - Echo (ping) istekleri
//   - Komşu Bulma Protokolü / NDP (ARP'nin IPv6 karşılığı)
//   - Yönlendirici Bulma (Router Discovery)
//   - Çok noktaya yayın dinleyici keşfi (MLD)
//
// ICMPv6 mesaj yapısı:
//   Tür (1B) | Kod (1B) | Sağlama Toplamı (2B) | Tipe özgü veri
//
// Türler:
//   1-127  : Hata mesajları
//   128-255: Bilgi mesajları (Echo, NDP, MLD...)

/// ICMPv6 mesaj türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icmpv6Type {
    DestinationUnreachable = 1,
    PacketTooBig = 2,
    TimeExceeded = 3,
    ParameterProblem = 4,
    EchoRequest = 128,
    EchoReply = 129,
    RouterSolicitation = 133,
    RouterAdvertisement = 134,
    NeighborSolicitation = 135,
    NeighborAdvertisement = 136,
    Redirect = 137,
}

impl Icmpv6Type {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Icmpv6Type::DestinationUnreachable),
            2 => Some(Icmpv6Type::PacketTooBig),
            3 => Some(Icmpv6Type::TimeExceeded),
            4 => Some(Icmpv6Type::ParameterProblem),
            128 => Some(Icmpv6Type::EchoRequest),
            129 => Some(Icmpv6Type::EchoReply),
            133 => Some(Icmpv6Type::RouterSolicitation),
            134 => Some(Icmpv6Type::RouterAdvertisement),
            135 => Some(Icmpv6Type::NeighborSolicitation),
            136 => Some(Icmpv6Type::NeighborAdvertisement),
            137 => Some(Icmpv6Type::Redirect),
            _ => None,
        }
    }
}

/// ICMPv6 genel başlık alanları (tüm ICMPv6 mesajlarında ortak)
#[derive(Clone, Debug)]
pub struct Icmpv6Header {
    pub msg_type: Icmpv6Type,
    pub code: u8,
    pub checksum: u16,
}

/// ICMPv6 Yönlendirici Talep (Router Solicitation - RS) mesajı.
/// Bir istemci ağa bağlandığında yönlendiricileri keşfetmek için gönderilir.
/// Hedef: `ff02::2` (tüm yönlendiriciler multicast adresi)
#[derive(Clone, Debug)]
pub struct RouterSolicitation {
    pub header: Icmpv6Header,
    /// Kaynak bağlantı katmanı adresi seçeneği (isteğe bağlı, genellikle MAC adresi)
    pub source_link_addr: Option<[u8; 6]>,
}

impl RouterSolicitation {
    pub fn new(source_mac: Option<super::MacAddr>) -> Self {
        RouterSolicitation {
            header: Icmpv6Header {
                msg_type: Icmpv6Type::RouterSolicitation,
                code: 0,
                checksum: 0,
            },
            source_link_addr: source_mac.map(|m| *m.as_bytes()),
        }
    }

    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();

        // ICMPv6 başlığı serileştir
        buf.push(self.header.msg_type as u8);
        buf.push(self.header.code);
        buf.extend_from_slice(&self.header.checksum.to_be_bytes());

        // Ayrılmış (Reserved) 4 bayt
        buf.extend_from_slice(&[0u8; 4]);

        // Kaynak bağlantı katmanı adresi seçeneği (tip 1)
        if let Some(mac) = &self.source_link_addr {
            buf.push(1); // Seçenek tipi: Source Link-Layer Address
            buf.push(1); // Seçenek uzunluğu (8 baytlık birimler halinde: 1 = 8 bayt)
            buf.extend_from_slice(mac);
            buf.extend_from_slice(&[0u8; 2]); // Hizalama için dolgu (padding)
        }

        buf
    }
}

/// ICMPv6 Yönlendirici Duyurusu (Router Advertisement - RA) mesajı.
/// Yönlendiriciler periyodik ya da RS'e yanıt olarak bu mesajı gönderir.
/// İçeriği: ön ek bilgisi, MTU, atlama sınırı, DNS sunucuları, DHCPv6 işaretleri
#[derive(Clone, Debug)]
pub struct RouterAdvertisement {
    pub header: Icmpv6Header,
    /// Yeni oluşturulan paketler için varsayılan atlama sınırı (hop limit)
    pub hop_limit: u8,
    /// Bayraklar: M=Yönetimli (Managed/DHCPv6), O=Diğer yapılandırma (Other)
    pub flags: u8,
    /// Yönlendirici ömrü (saniye cinsinden, 0 ise artık varsayılan yönlendirici değildir)
    pub router_lifetime: u16,
    /// Komşunun erişilebilir kalma süresi (milisaniye)
    pub reachable_time: u32,
    /// Komşu talep yeniden gönderme aralığı (milisaniye)
    pub retransmit_timer: u32,
    /// Ön ek bilgisi seçenekleri (SLAAC için)
    pub prefixes: alloc::vec::Vec<PrefixInfo>,
    /// RDNSS seçeneğindeki DNS sunucuları
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// Bağlantı MTU değeri (varsa)
    pub mtu: Option<u32>,
}

/// RA içindeki Ön Ek Bilgisi (Prefix Information) seçeneği.
/// SLAAC adresi üretme ve yönlendirme kararları için kullanılır.
#[derive(Clone, Debug)]
pub struct PrefixInfo {
    pub prefix: Ipv6Addr,
    pub prefix_len: u8,
    pub on_link: bool,
    pub autonomous: bool,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
}

impl RouterAdvertisement {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }

        let msg_type = Icmpv6Type::from_u8(data[0])?;
        if msg_type != Icmpv6Type::RouterAdvertisement {
            return None;
        }

        let mut ra = RouterAdvertisement {
            header: Icmpv6Header {
                msg_type,
                code: data[1],
                checksum: u16::from_be_bytes([data[2], data[3]]),
            },
            hop_limit: data[4],
            flags: data[5],
            router_lifetime: u16::from_be_bytes([data[6], data[7]]),
            reachable_time: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            retransmit_timer: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            prefixes: alloc::vec::Vec::new(),
            dns_servers: alloc::vec::Vec::new(),
            mtu: None,
        };

        // RA seçeneklerini ayrıştır
        let mut offset = 16;
        while offset + 2 <= data.len() {
            let opt_type = data[offset];
            let opt_len = data[offset + 1] as usize * 8;

            if offset + opt_len > data.len() {
                break;
            }

            match opt_type {
                1 => {
                    // Kaynak bağlantı katmanı adresi seçeneği (Source Link-Layer Address)
                }
                3 => {
                    // Ön Ek Bilgisi seçeneği (Prefix Information)
                    if opt_len >= 32 {
                        let prefix_len = data[offset + 2];
                        let flags = data[offset + 3];
                        let valid_lifetime = u32::from_be_bytes([
                            data[offset + 4],
                            data[offset + 5],
                            data[offset + 6],
                            data[offset + 7],
                        ]);
                        let preferred_lifetime = u32::from_be_bytes([
                            data[offset + 8],
                            data[offset + 9],
                            data[offset + 10],
                            data[offset + 11],
                        ]);

                        let mut prefix = [0u8; 16];
                        prefix.copy_from_slice(&data[offset + 16..offset + 32]);

                        ra.prefixes.push(PrefixInfo {
                            prefix: Ipv6Addr(prefix),
                            prefix_len,
                            on_link: (flags & 0x80) != 0,
                            autonomous: (flags & 0x40) != 0,
                            valid_lifetime,
                            preferred_lifetime,
                        });
                    }
                }
                5 => {
                    // MTU seçeneği (RFC 4861 §4.6.4)
                    if opt_len >= 8 {
                        ra.mtu = Some(u32::from_be_bytes([
                            data[offset + 4],
                            data[offset + 5],
                            data[offset + 6],
                            data[offset + 7],
                        ]));
                    }
                }
                25 => {
                    // RDNSS: Özyinelemeli DNS Sunucusu seçeneği (RFC 8106)
                    // Her DNS sunucusu 16 bayt (1 IPv6 adresi)
                    if opt_len >= 24 {
                        let num_servers = (opt_len - 8) / 16;
                        for i in 0..num_servers {
                            let start = offset + 8 + i * 16;
                            if start + 16 <= data.len() {
                                let mut addr = [0u8; 16];
                                addr.copy_from_slice(&data[start..start + 16]);
                                ra.dns_servers.push(Ipv6Addr(addr));
                            }
                        }
                    }
                }
                _ => {}
            }

            offset += opt_len;
        }

        Some(ra)
    }

    /// M (Managed) bayrağı: Ayarlanmışsa adresler DHCPv6 ile alınır (SLAAC yerine)
    pub fn use_dhcpv6(&self) -> bool {
        (self.flags & 0x80) != 0
    }

    /// O (Other) bayrağı: Ayarlanmışsa DNS/NTP gibi diğer yapılandırma için DHCPv6 kullanılır
    pub fn use_dhcpv6_other(&self) -> bool {
        (self.flags & 0x40) != 0
    }
}

// ============================================================================
// SLAAC (Durumsuz Adres Otokonfigürasyonu - Stateless Address Autoconfiguration)
// ============================================================================
//
// SLAAC, RFC 4862'de tanımlanmıştır. Cihazların DHCPv6 sunucusu olmadan
// otomatik olarak IPv6 adresi edinmesini sağlar.
//
// SLAAC Akışı:
//
//   1. Cihaz açılır → Link-Local adres üretilir (fe80::/64 + EUI-64)
//   2. DAD (Duplicate Address Detection) yapılır → adres çakışıyor mu?
//   3. Yönlendirici Talebi (RS) gönderilir → ff02::2 hedefli
//   4. Yönlendirici Duyurusu (RA) alınır ← yönlendiriciden
//   5. RA'daki AutoConf ön eki + EUI-64 → Global adres oluşturulur
//   6. Adres DAD ile doğrulanır
//
//   Cihaz        Bağlantı        Yönlendirici
//    |               |                 |
//    |-- RS -------->|---------------->|
//    |               |          RA <---|
//    |<-- RA --------|-----------------|
//    | (Global adres üret)             |
//    |                                 |

/// SLAAC durum nesnesi — bir ağ arayüzünün IPv6 adres durumunu tutar
#[derive(Clone, Debug)]
pub struct SlaacState {
    /// Bağlantı-yerel adres (fe80::/64)
    pub link_local: Ipv6Addr,
    /// Global adresler (RA ön eki + EUI-64 ile oluşturulur)
    pub global_addresses: alloc::vec::Vec<SlaacAddress>,
    /// Varsayılan ağ geçidi (RA'nın kaynak adresi)
    pub default_gateway: Option<Ipv6Addr>,
    /// RA'dan öğrenilen DNS sunucuları
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// Bağlantı MTU değeri (varsayılan 1500)
    pub mtu: u32,
    /// Yönlendirici ömrü (saniye)
    pub router_lifetime: u32,
}

/// SLAAC ile oluşturulmuş tek bir IPv6 adresi ve yaşam süresi bilgisi
#[derive(Clone, Debug)]
pub struct SlaacAddress {
    pub address: Ipv6Addr,
    pub prefix_len: u8,
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
    pub created_at: u64,
}

impl SlaacState {
    pub fn new(mac: super::MacAddr) -> Self {
        SlaacState {
            link_local: link_local_from_mac(mac),
            global_addresses: alloc::vec::Vec::new(),
            default_gateway: None,
            dns_servers: alloc::vec::Vec::new(),
            mtu: 1500,
            router_lifetime: 0,
        }
    }

    /// RA'dan gelen ön ek ve MAC adresinden SLAAC global adresi üretir.
    /// Algoritma: ön ek (64 bit) + EUI-64 arabirim tanımlayıcısı (64 bit)
    pub fn generate_address(prefix: &Ipv6Addr, prefix_len: u8, mac: super::MacAddr) -> Ipv6Addr {
        let mac_bytes = mac.as_bytes();

        // EUI-64 arabirim tanımlayıcısı oluştur
        let mut interface_id = [0u8; 8];
        interface_id[0] = mac_bytes[0] ^ 0x02; // Universal/local bitini tersine çevir
        interface_id[1] = mac_bytes[1];
        interface_id[2] = mac_bytes[2];
        interface_id[3] = 0xFF;
        interface_id[4] = 0xFE;
        interface_id[5] = mac_bytes[3];
        interface_id[6] = mac_bytes[4];
        interface_id[7] = mac_bytes[5];

        // Ön ek ve arabirim tanımlayıcısını birleştir
        let mut addr = [0u8; 16];

        // Ön eki kopyala (prefix_len bit kadar)
        let prefix_bytes = (prefix_len as usize + 7) / 8;
        for i in 0..prefix_bytes.min(8) {
            addr[i] = prefix.0[i];
        }

        // Arabirim tanımlayıcısını (EUI-64) sonuna ekle (bayt 8-15)
        for i in 0..8 {
            addr[8 + i] = interface_id[i];
        }

        Ipv6Addr(addr)
    }

    /// Yönlendirici Duyurusu'nu (RA) işler; adres ve yapılandırmayı günceller
    pub fn process_ra(&mut self, ra: &RouterAdvertisement, mac: super::MacAddr, current_time: u64) {
        // Varsayılan ağ geçidini güncelle (RA kaynağı, IPv6 başlığından alınır)
        if ra.router_lifetime > 0 {
            // Ağ geçidi adresi RA'nın kaynak IPv6 adresinden alınır (burada üst katmandan geçmeli)
            self.router_lifetime = ra.router_lifetime as u32;
        }

        // MTU'yu güncelle
        if let Some(mtu) = ra.mtu {
            self.mtu = mtu;
        }

        // DNS sunucularını güncelle
        self.dns_servers = ra.dns_servers.clone();

        // Otomatik yapılandırma (A bayrağı) ön ekleri için SLAAC adresi üret
        for prefix in &ra.prefixes {
            if prefix.autonomous {
                // EUI-64 + ön ek ile global adres oluştur
                let addr = Self::generate_address(&prefix.prefix, prefix.prefix_len, mac);

                // Bu adres zaten kayıtlıysa yaşam sürelerini güncelle
                let existing = self.global_addresses.iter_mut().find(|a| a.address == addr);

                if let Some(existing) = existing {
                    // Mevcut adresin yaşam sürelerini yenile
                    existing.valid_lifetime = prefix.valid_lifetime;
                    existing.preferred_lifetime = prefix.preferred_lifetime;
                } else {
                    // Yeni adresi listeye ekle
                    self.global_addresses.push(SlaacAddress {
                        address: addr,
                        prefix_len: prefix.prefix_len,
                        valid_lifetime: prefix.valid_lifetime,
                        preferred_lifetime: prefix.preferred_lifetime,
                        created_at: current_time,
                    });
                }
            }
        }
    }

    /// Adresin tercih edilen (preferred) durumda olup olmadığını kontrol eder.
    /// Tercih süresi (preferred_lifetime) dolmamışsa adres tercih edilir.
    pub fn is_preferred(&self, addr: &Ipv6Addr, current_time: u64) -> bool {
        for slaac_addr in &self.global_addresses {
            if &slaac_addr.address == addr {
                let elapsed = current_time - slaac_addr.created_at;
                return elapsed < slaac_addr.preferred_lifetime as u64;
            }
        }
        false
    }

    /// Adresin geçerli (valid) olup olmadığını kontrol eder.
    /// Geçerli süre (valid_lifetime) dolmamışsa adres hâlâ kullanılabilir.
    pub fn is_valid(&self, addr: &Ipv6Addr, current_time: u64) -> bool {
        for slaac_addr in &self.global_addresses {
            if &slaac_addr.address == addr {
                let elapsed = current_time - slaac_addr.created_at;
                return elapsed < slaac_addr.valid_lifetime as u64;
            }
        }
        false
    }

    /// Geçerli süresi (valid_lifetime) dolmuş adresleri listeden temizler
    pub fn expire_addresses(&mut self, current_time: u64) {
        self.global_addresses.retain(|addr| {
            let elapsed = current_time - addr.created_at;
            elapsed < addr.valid_lifetime as u64
        });
    }
}

// ============================================================================
// DHCPv6 (IPv6 için Dinamik Ana Makine Yapılandırma Protokolü)
// ============================================================================
//
// DHCPv6 (RFC 3315), IPv6 ağlarında cihazlara otomatik IP adresi, DNS sunucusu
// ve diğer yapılandırma bilgilerini atayan protokoldür.
//
// SLAAC'tan farkı: Merkezi sunucudan yönetilen adres ataması yapar.
//
// DHCPv6 Mesaj Akışı (4 adımlı):
//
//   İstemci                            Sunucu
//     |                                   |
//     |------- Solicit (Talep) ---------->|
//     |<------ Advertise (Duyuru) --------|
//     |------- Request (İstek) ---------->|
//     |<------ Reply (Yanıt) -------------|
//     |       (IP atanmış!)               |
//
// Hızlı mod (Rapid Commit seçeneğiyle 2 adım):
//
//   İstemci                            Sunucu
//     |                                   |
//     |------- Solicit + RapidCommit --->|
//     |<------ Reply (/w address) --------|
//
// DUID (DHCP Unique Identifier): Her DHCPv6 istemcisinin kalıcı kimliği.
//   Tür 1: DUID-LLT (Link-Layer + Zaman)
//   Tür 2: DUID-EN  (Kurumsal Numara)
//   Tür 3: DUID-LL  (Yalnızca MAC — bu implementasyonda kullanılan)
//
// IA_NA (Identity Association for Non-temporary Addresses):
//   Sunucunun istemciye atadığı geçici olmayan adres grubu
//   T1 (Yenileme Zamanı) dolunca istemci sunucuya Renew gönderir
//   T2 (Yeniden Bağlama Zamanı) dolunca Rebind gönderir

/// DHCPv6 mesaj türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dhcpv6MessageType {
    Solicit = 1,
    Advertise = 2,
    Request = 3,
    Confirm = 4,
    Renew = 5,
    Rebind = 6,
    Reply = 7,
    Release = 8,
    Decline = 9,
    Reconfigure = 10,
    InformationRequest = 11,
    RelayForw = 12,
    RelayRepl = 13,
}

/// DHCPv6 seçenek kodları (RFC 3315 §22)
pub const DHCPV6_OPT_CLIENTID: u16 = 1; // İstemci DUID
pub const DHCPV6_OPT_SERVERID: u16 = 2; // Sunucu DUID
pub const DHCPV6_OPT_IA_NA: u16 = 3; // Kalıcı Olmayan Adres Grubu
pub const DHCPV6_OPT_IA_TA: u16 = 4; // Geçici Adres Grubu
pub const DHCPV6_OPT_IAADDR: u16 = 5; // IA içindeki adres
pub const DHCPV6_OPT_ORO: u16 = 6; // Seçenek İstek Listesi
pub const DHCPV6_OPT_PREFERENCE: u16 = 7; // Sunucu tercihi (0-255)
pub const DHCPV6_OPT_ELAPSED_TIME: u16 = 8; // Geçen süre (1/100 saniye)
pub const DHCPV6_OPT_RELAY_MSG: u16 = 9; // Röle mesajı
pub const DHCPV6_OPT_STATUS_CODE: u16 = 13; // Durum kodu
pub const DHCPV6_OPT_RAPID_COMMIT: u16 = 14; // Hızlı onay (2 adımlı el sıkışma)
pub const DHCPV6_OPT_USER_CLASS: u16 = 15; // Kullanıcı sınıfı
pub const DHCPV6_OPT_VENDOR_CLASS: u16 = 16; // Satıcı sınıfı
pub const DHCPV6_OPT_DNS_SERVERS: u16 = 23; // DNS sunucuları (RFC 3646)
pub const DHCPV6_OPT_DOMAIN_LIST: u16 = 24; // Alan adı arama listesi
pub const DHCPV6_OPT_IA_PD: u16 = 25; // Ön ek Delegasyonu
pub const DHCPV6_OPT_IA_PREFIX: u16 = 26; // IA_PD içindeki ön ek

/// DHCPv6 istemci durum makinesi ve yapılandırma verisi
#[derive(Clone, Debug)]
pub struct Dhcpv6Client {
    /// İstemci DUID (DHCP Benzersiz Tanımlayıcısı) — her cihazın kalıcı kimliği
    pub duid: [u8; 14],
    /// İşlem Kimliği (24 bit): istek/yanıt eşleştirme için kullanılır
    pub transaction_id: u32,
    /// Sunucu DUID (Solicit/Reply akışından öğrenilir)
    pub server_duid: Option<alloc::vec::Vec<u8>>,
    /// Atanan IPv6 adresleri listesi
    pub addresses: alloc::vec::Vec<Dhcpv6Address>,
    /// Sunucudan alınan DNS sunucuları
    pub dns_servers: alloc::vec::Vec<Ipv6Addr>,
    /// Alan adı arama listesi
    pub domains: alloc::vec::Vec<String>,
    /// İstemci durum makinesi
    pub state: Dhcpv6State,
    /// T1 yenileme zamanlayıcısı (saniye): dolunca Renew gönderilir
    pub t1: u32,
    /// T2 yeniden bağlama zamanlayıcısı (saniye): dolunca Rebind gönderilir
    pub t2: u32,
    /// Tercih edilen yaşam süresi (saniye)
    pub preferred_lifetime: u32,
    /// Geçerli yaşam süresi (saniye)
    pub valid_lifetime: u32,
}

/// DHCPv6 sunucusundan atanmış tek bir IPv6 adresi ve yaşam süresi bilgisi
#[derive(Clone, Debug)]
pub struct Dhcpv6Address {
    pub address: Ipv6Addr,
    pub prefix_len: u8,
    pub preferred_lifetime: u32,
    pub valid_lifetime: u32,
}

/// DHCPv6 istemci durum makinesi durumları (RFC 3315 §5.1)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dhcpv6State {
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
    Released,
}

impl Dhcpv6Client {
    pub fn new(mac: super::MacAddr) -> Self {
        // MAC adresinden DUID-LL (Link-Layer DUID) üret
        // Format: [tip(2B)][donanım tipi(2B)][MAC(6B)][zaman(4B)]
        let mut duid = [0u8; 14];
        duid[0] = 0; // DUID tipi: Link-Layer (LL)
        duid[1] = 1;
        duid[2] = 0; // Donanım tipi: Ethernet
        duid[3] = 1;
        duid[4..10].copy_from_slice(mac.as_bytes());
        duid[10..14].copy_from_slice(&[0, 0, 0, 1]); // Zaman bilgisi (sabit 1)

        Dhcpv6Client {
            duid,
            transaction_id: 0,
            server_duid: None,
            addresses: alloc::vec::Vec::new(),
            dns_servers: alloc::vec::Vec::new(),
            domains: alloc::vec::Vec::new(),
            state: Dhcpv6State::Init,
            t1: 0,
            t2: 0,
            preferred_lifetime: 0,
            valid_lifetime: 0,
        }
    }

    /// Yeni bir işlem kimliği (transaction ID) üretir. Her mesaj akışı için taze bir ID gerekir.
    pub fn new_transaction_id(&mut self) -> u32 {
        // Zamanlayıcı sayacından 24-bit rasgele değer üret
        self.transaction_id = crate::interrupts::get_ticks() as u32 & 0xFFFFFF;
        self.transaction_id
    }

    /// DHCPv6 Solicit (Talep) mesajı oluşturur — sunucu keşfi için gönderilir
    pub fn build_solicit(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();

        // Mesaj tipi: Solicit
        buf.push(Dhcpv6MessageType::Solicit as u8);

        // İşlem Kimliği (24 bit, büyük bajt önce)
        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);

        // İstemci DUID seçeneği
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);

        // Hızlı onay seçeneği (Rapid Commit — 2 adımlı el sıkışma için)
        buf.extend_from_slice(&DHCPV6_OPT_RAPID_COMMIT.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());

        // Seçenek İstek Listesi (ORO — DNS sunucuları iste)
        buf.extend_from_slice(&DHCPV6_OPT_ORO.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DNS_SERVERS.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DOMAIN_LIST.to_be_bytes());

        // Geçen süre seçeneği (0 = ilk deneme)
        buf.extend_from_slice(&DHCPV6_OPT_ELAPSED_TIME.to_be_bytes());
        buf.extend_from_slice(&2u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes()); // 0 ms geçti

        buf
    }

    /// DHCPv6 Request (İstek) mesajı oluşturur — Advertise'a yanıt olarak belirli sunucudan IP ister
    pub fn build_request(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();

        // Mesaj tipi: Request
        buf.push(Dhcpv6MessageType::Request as u8);

        // İşlem Kimliği
        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);

        // İstemci DUID seçeneği
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);

        // Sunucu DUID seçeneği (Advertise'dan öğrenildi)
        if let Some(server_duid) = &self.server_duid {
            buf.extend_from_slice(&DHCPV6_OPT_SERVERID.to_be_bytes());
            buf.extend_from_slice(&(server_duid.len() as u16).to_be_bytes());
            buf.extend_from_slice(server_duid);
        }

        // ORO (Seçenek İstek Listesi)
        buf.extend_from_slice(&DHCPV6_OPT_ORO.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DNS_SERVERS.to_be_bytes());
        buf.extend_from_slice(&DHCPV6_OPT_DOMAIN_LIST.to_be_bytes());

        // Geçen süre
        buf.extend_from_slice(&DHCPV6_OPT_ELAPSED_TIME.to_be_bytes());
        buf.extend_from_slice(&2u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());

        buf
    }

    /// DHCPv6 Renew (Yenileme) mesajı oluşturur — T1 süresi dolunca gönderilir
    pub fn build_renew(&mut self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();

        buf.push(Dhcpv6MessageType::Renew as u8);

        let tid = self.new_transaction_id();
        buf.push(((tid >> 16) & 0xFF) as u8);
        buf.push(((tid >> 8) & 0xFF) as u8);
        buf.push((tid & 0xFF) as u8);

        // İstemci DUID
        buf.extend_from_slice(&DHCPV6_OPT_CLIENTID.to_be_bytes());
        buf.extend_from_slice(&(self.duid.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.duid);

        // Sunucu DUID
        if let Some(server_duid) = &self.server_duid {
            buf.extend_from_slice(&DHCPV6_OPT_SERVERID.to_be_bytes());
            buf.extend_from_slice(&(server_duid.len() as u16).to_be_bytes());
            buf.extend_from_slice(server_duid);
        }

        buf
    }

    /// DHCPv6 Reply (Yanıt) mesajını ayrıştırır; başarıyla işlenirse `true` döndürür
    pub fn parse_reply(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }

        // Mesaj tipini kontrol et
        if data[0] != Dhcpv6MessageType::Reply as u8 {
            return false;
        }

        // İşlem kimliğini doğrula
        if data.len() < 4 {
            return false;
        }

        let tid = ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
        if tid != self.transaction_id {
            return false;
        }

        // Seçenekleri ayrıştır
        let mut offset = 4;
        while offset + 4 <= data.len() {
            let opt_code = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let opt_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;

            if offset + 4 + opt_len > data.len() {
                break;
            }

            let opt_data = &data[offset + 4..offset + 4 + opt_len];

            match opt_code {
                DHCPV6_OPT_SERVERID => {
                    self.server_duid = Some(opt_data.to_vec());
                }
                DHCPV6_OPT_IA_NA => {
                    // IA_NA (Geçici Olmayan Adres Grubu) — T1/T2 ve adres bilgisi içerir
                    if opt_len >= 12 {
                        let t1 = u32::from_be_bytes([
                            opt_data[4],
                            opt_data[5],
                            opt_data[6],
                            opt_data[7],
                        ]);
                        let t2 = u32::from_be_bytes([
                            opt_data[8],
                            opt_data[9],
                            opt_data[10],
                            opt_data[11],
                        ]);
                        self.t1 = t1;
                        self.t2 = t2;

                        // IA_NA içindeki IAADDR seçeneklerini ayrıştır
                        let mut ia_offset = 12;
                        while ia_offset + 4 <= opt_data.len() {
                            let ia_opt_code =
                                u16::from_be_bytes([opt_data[ia_offset], opt_data[ia_offset + 1]]);
                            let ia_opt_len = u16::from_be_bytes([
                                opt_data[ia_offset + 2],
                                opt_data[ia_offset + 3],
                            ]) as usize;

                            if ia_opt_code == DHCPV6_OPT_IAADDR && ia_opt_len >= 24 {
                                let mut addr = [0u8; 16];
                                addr.copy_from_slice(&opt_data[ia_offset + 4..ia_offset + 20]);
                                let preferred = u32::from_be_bytes([
                                    opt_data[ia_offset + 20],
                                    opt_data[ia_offset + 21],
                                    opt_data[ia_offset + 22],
                                    opt_data[ia_offset + 23],
                                ]);
                                let valid = u32::from_be_bytes([
                                    opt_data[ia_offset + 24],
                                    opt_data[ia_offset + 25],
                                    opt_data[ia_offset + 26],
                                    opt_data[ia_offset + 27],
                                ]);

                                self.addresses.push(Dhcpv6Address {
                                    address: Ipv6Addr(addr),
                                    prefix_len: 64, // Default
                                    preferred_lifetime: preferred,
                                    valid_lifetime: valid,
                                });
                            }

                            ia_offset += 4 + ia_opt_len;
                        }
                    }
                }
                DHCPV6_OPT_DNS_SERVERS => {
                    // DNS sunucuları: her biri 16 bayt IPv6 adresi
                    for i in (0..opt_len).step_by(16) {
                        if i + 16 <= opt_len {
                            let mut addr = [0u8; 16];
                            addr.copy_from_slice(&opt_data[i..i + 16]);
                            self.dns_servers.push(Ipv6Addr(addr));
                        }
                    }
                }
                DHCPV6_OPT_DOMAIN_LIST => {
                    // Alan adı arama listesi — şimdilik ham baytlar olarak saklanıyor
                    // Simplified: just store as bytes
                }
                _ => {}
            }

            offset += 4 + opt_len;
        }

        self.state = Dhcpv6State::Bound;
        true
    }
}

// ============================================================================
// IPv6 KOMŞU BULMA PROTOKOLÜ (NDP - Neighbor Discovery Protocol)
// ============================================================================
//
// NDP, RFC 4861'de tanımlanmıştır. IPv4'teki ARP protokolünün yerini alır.
// ICMPv6 mesajları üzerine inşa edilmiştir.
//
// NDP'nin Görevleri:
//   1) Adres Çözümleme  : IPv6 → MAC (ARP yerine NS/NA kullanılır)
//   2) Yönlendirici Keşfi: RS/RA mesajları ile varsayılan ağ geçidi bulunur
//   3) Ön Ek Keşfi     : RA'dan ağ ön ekleri öğrenilir (SLAAC için)
//   4) DAD             : Yinelenen Adres Algılama (adres çakışması tespiti)
//   5) NUD             : Komşu Erişilebilirlik Tespiti
//
// Komşu Çözümleme Akışı (ARP'ye benzer):
//
//   A (fe80::1)          Ağ          B (fe80::2)
//       |                 |               |
//       |=== NS =========>|==============>|  "fe80::2 kimdir?"
//       |                 |   NA <========|  "Ben! MAC: 52:54:00:xx:xx:xx"
//       |<================|               |
//       | (B'nin MAC'i öğrenildi)         |
//
// Komşu Önbellek Durumları:
//   INCOMPLETE → REACHABLE → STALE → DELAY → PROBE → (temizle)

/// Komşu önbellek girdisi — IPv6 adres ↔ MAC adres eşleşmesini tutar
#[derive(Clone, Debug)]
pub struct NeighborEntry {
    pub ip: Ipv6Addr,
    pub mac: super::MacAddr,
    pub is_router: bool,
    pub state: NeighborState,
    pub created_at: u64,
    pub last_used: u64,
}

/// Komşu önbellek girdisinin durumu (RFC 4861 §7.3.2)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeighborState {
    Incomplete,
    Reachable,
    Stale,
    Delay,
    Probe,
}

/// Komşu Talep (Neighbor Solicitation - NS) mesajı.
/// "Bu IPv6 adresinin MAC'i nedir?" sorusunu ağa yayar.
/// Hedef: talep-düğüm multicast adresi (`ff02::1:ffXX:XXXX`)
#[derive(Clone, Debug)]
pub struct NeighborSolicitation {
    pub header: Icmpv6Header,
    pub target_addr: Ipv6Addr,
    pub source_link_addr: Option<[u8; 6]>,
}

impl NeighborSolicitation {
    pub fn new(target: Ipv6Addr, source_mac: Option<super::MacAddr>) -> Self {
        NeighborSolicitation {
            header: Icmpv6Header {
                msg_type: Icmpv6Type::NeighborSolicitation,
                code: 0,
                checksum: 0,
            },
            target_addr: target,
            source_link_addr: source_mac.map(|m| *m.as_bytes()),
        }
    }

    pub fn serialize(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();

        buf.push(self.header.msg_type as u8);
        buf.push(self.header.code);
        buf.extend_from_slice(&self.header.checksum.to_be_bytes());

        // Ayrılmış (Reserved)
        buf.extend_from_slice(&[0u8; 4]);

        // Hedef adres (sorgulanmak istenen IPv6 adresi)
        buf.extend_from_slice(&self.target_addr.0);

        // Kaynak bağlantı katmanı adresi seçeneği
        if let Some(mac) = &self.source_link_addr {
            buf.push(1); // Seçenek tipi
            buf.push(1); // Seçenek uzunluğu
            buf.extend_from_slice(mac);
            buf.extend_from_slice(&[0u8; 2]);
        }

        buf
    }
}

/// Komşu Duyurusu (Neighbor Advertisement - NA) mesajı.
/// NS'e yanıt olarak "Bu adres bende, MAC'im şu" bilgisini içerir.
#[derive(Clone, Debug)]
pub struct NeighborAdvertisement {
    pub header: Icmpv6Header,
    pub target_addr: Ipv6Addr,
    pub target_link_addr: [u8; 6],
    pub router: bool,
    pub solicited: bool,
    pub override_flag: bool,
}

impl NeighborAdvertisement {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }

        let msg_type = Icmpv6Type::from_u8(data[0])?;
        if msg_type != Icmpv6Type::NeighborAdvertisement {
            return None;
        }

        let flags = data[4];

        let mut target_addr = [0u8; 16];
        target_addr.copy_from_slice(&data[8..24]);

        let mut target_link_addr = None;

        // NA seçeneklerini ayrıştır (hedef bağlantı katmanı adresi ararız)
        let mut offset = 24;
        while offset + 2 <= data.len() {
            let opt_type = data[offset];
            let opt_len = data[offset + 1] as usize * 8;

            if opt_type == 2 && opt_len >= 8 && offset + 8 <= data.len() {
                target_link_addr = Some([
                    data[offset + 2],
                    data[offset + 3],
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]);
            }

            offset += opt_len;
        }

        Some(NeighborAdvertisement {
            header: Icmpv6Header {
                msg_type,
                code: data[1],
                checksum: u16::from_be_bytes([data[2], data[3]]),
            },
            target_addr: Ipv6Addr(target_addr),
            target_link_addr: target_link_addr?,
            router: (flags & 0x80) != 0,
            solicited: (flags & 0x40) != 0,
            override_flag: (flags & 0x20) != 0,
        })
    }
}

// ============================================================================
// ICMPv6 İŞLEME VE SAĞLAMA TOPLAMI
// ============================================================================

/// ICMPv6 sağlama toplamını (checksum) hesaplar.
///
/// IPv6 sözde başlık (pseudo-header) kullanılarak RFC 4443 §2.3'e göre hesaplanır:
///   Kaynak Adresi (16B) + Hedef Adresi (16B) + ICMPv6 Uzunluk (4B) + Sıfır (3B) + Next Header=58 (1B)
/// Ardından ICMPv6 mesajının tamamı toplamlanır.
pub fn compute_icmpv6_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, payload: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Kaynak adres (16 bayt = 8 × 16-bit sözcük)
    for chunk in src.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    // Hedef adres
    for chunk in dst.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }

    // ICMPv6 yük uzunluğu (32-bit, big-endian)
    let len = payload.len() as u32;
    sum += (len >> 16) as u32;
    sum += (len & 0xFFFF) as u32;

    // Next Header = 58 (ICMPv6)
    sum += 58u32;

    // ICMPv6 verisi (16-bit sözcükler halinde)
    let mut i = 0;
    while i + 1 < payload.len() {
        sum += u16::from_be_bytes([payload[i], payload[i + 1]]) as u32;
        i += 2;
    }
    // Tek bayt kaldıysa
    if i < payload.len() {
        sum += (payload[i] as u32) << 8;
    }

    // Taşmaları katla
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

/// Gelen ICMPv6 paketini işler.
///
/// - EchoRequest (128): EchoReply (129) oluşturur, sağlama toplamını hesaplar ve geri gönderir.
/// - RouterSolicitation (133) / NeighborSolicitation (135) / NeighborAdvertisement (136):
///   NDP işleyicisine yönlendirir.
pub fn process_icmpv6(ipv6_src: &Ipv6Addr, ipv6_dst: &Ipv6Addr, icmpv6_payload: &[u8]) {
    if icmpv6_payload.len() < 4 {
        crate::serial_println!("[ICMPv6] Payload too short ({}B)", icmpv6_payload.len());
        return;
    }

    let msg_type = icmpv6_payload[0];
    let _code = icmpv6_payload[1];
    // icmpv6_payload[2..4] = checksum (alınan)

    match msg_type {
        // ── EchoRequest (128) → EchoReply (129) ──
        128 => {
            crate::serial_println!("[ICMPv6] EchoRequest from {:?}", ipv6_src);

            // Yanıt: type=129, code=0, checksum=0 (hesaplanacak), gövde aynı
            let mut reply = icmpv6_payload.to_vec();
            reply[0] = 129; // EchoReply
            reply[1] = 0;
            // Checksum alanını sıfırla, sonra yeniden hesapla
            reply[2] = 0;
            reply[3] = 0;

            // Kaynak ↔ hedef ters çevrilir
            let cksum = compute_icmpv6_checksum(ipv6_dst, ipv6_src, &reply);
            reply[2] = (cksum >> 8) as u8;
            reply[3] = (cksum & 0xFF) as u8;

            // IPv6 paketi oluştur ve gönder
            let reply_header = Ipv6Header::new(
                *ipv6_dst, // kaynak artık bizim adresimiz
                *ipv6_src, // hedef orijinal kaynağa
                58,        // ICMPv6
                reply.len() as u16,
            );
            let pkt = Ipv6Packet::new(reply_header, &reply);
            let serialized = pkt.serialize();
            let current_tsc = crate::interrupts::get_ticks();
            let (next_hop, dst_mac) = if let Some(mac) = neighbor_lookup(ipv6_src) {
                (*ipv6_src, mac)
            } else if let Some((router_addr, mac)) = select_next_hop(ipv6_src, current_tsc) {
                (router_addr, mac)
            } else {
                let _ = send_neighbor_solicitation(ipv6_src);
                crate::serial_println!(
                    "[ICMPv6] EchoReply pending neighbor resolution for {:?}",
                    ipv6_src
                );
                return;
            };
            let Some(iface) = super::default_interface() else {
                crate::serial_println!("[ICMPv6] EchoReply failed: no default interface");
                return;
            };
            let mut iface = iface.lock();
            let frame = super::ethernet::EthernetFrame::new(
                dst_mac,
                iface.mac(),
                super::ethernet::EtherType::IPV6,
                &serialized,
            );
            let mut frame_buf = alloc::vec![0u8; 1514];
            match frame.serialize(&mut frame_buf) {
                Ok(len) => match iface.send(&frame_buf[..len]) {
                    Ok(()) => {
                        crate::serial_println!(
                            "[ICMPv6] EchoReply sent to {:?} via {:?} ({}B)",
                            ipv6_src,
                            next_hop,
                            reply.len()
                        );
                    }
                    Err(err) => {
                        crate::serial_println!(
                            "[ICMPv6] EchoReply transmit failed for {:?}: {:?}",
                            ipv6_src,
                            err
                        );
                    }
                },
                Err(err) => {
                    crate::serial_println!(
                        "[ICMPv6] EchoReply frame build failed for {:?}: {:?}",
                        ipv6_src,
                        err
                    );
                }
            }
        }

        // ── RouterSolicitation (133) ──
        133 => {
            crate::serial_println!("[ICMPv6/NDP] RouterSolicitation from {:?}", ipv6_src);
            // Sunucu tarafı RS işleme (RA gönderme) bu çekirdekte opsiyonel
        }

        // ── NeighborSolicitation (135) ──
        134 => {
            crate::serial_println!("[ICMPv6/NDP] RouterAdvertisement from {:?}", ipv6_src);
            if let Some(ra) = RouterAdvertisement::parse(icmpv6_payload) {
                let mut router_mac = None;
                let mut offset = 16usize;
                while offset + 2 <= icmpv6_payload.len() {
                    let opt_type = icmpv6_payload[offset];
                    let opt_len = icmpv6_payload[offset + 1] as usize * 8;
                    if opt_len == 0 || offset + opt_len > icmpv6_payload.len() {
                        break;
                    }
                    if opt_type == 1 && opt_len >= 8 {
                        router_mac = Some(super::MacAddr::new([
                            icmpv6_payload[offset + 2],
                            icmpv6_payload[offset + 3],
                            icmpv6_payload[offset + 4],
                            icmpv6_payload[offset + 5],
                            icmpv6_payload[offset + 6],
                            icmpv6_payload[offset + 7],
                        ]));
                        break;
                    }
                    offset += opt_len;
                }

                if let Some(mac) = router_mac {
                    neighbor_update(*ipv6_src, mac);
                    if ra.router_lifetime > 0 {
                        add_default_router(
                            *ipv6_src,
                            ra.router_lifetime as u64,
                            crate::interrupts::get_ticks(),
                            *mac.as_bytes(),
                        );
                    }
                }

                crate::serial_println!(
                    "[ICMPv6/NDP] RA accepted: lifetime={}s prefixes={} dns={}",
                    ra.router_lifetime,
                    ra.prefixes.len(),
                    ra.dns_servers.len()
                );
            }
        }

        135 => {
            crate::serial_println!("[ICMPv6/NDP] NeighborSolicitation from {:?}", ipv6_src);
            // Hedef adres (payload[8..24]) bizim adreslerimizden biri mi kontrol et
            if icmpv6_payload.len() >= 24 {
                let mut target = [0u8; 16];
                target.copy_from_slice(&icmpv6_payload[8..24]);
                let target_addr = Ipv6Addr(target);
                crate::serial_println!("[ICMPv6/NDP] NS target={:?}", target_addr);
                // Komşu önbelleğini güncelle (kaynak MAC varsa)
                if icmpv6_payload.len() >= 32 && icmpv6_payload[24] == 1 {
                    let mac = super::MacAddr::new([
                        icmpv6_payload[26],
                        icmpv6_payload[27],
                        icmpv6_payload[28],
                        icmpv6_payload[29],
                        icmpv6_payload[30],
                        icmpv6_payload[31],
                    ]);
                    neighbor_update(*ipv6_src, mac);
                }
            }
        }

        // ── NeighborAdvertisement (136) ──
        136 => {
            crate::serial_println!("[ICMPv6/NDP] NeighborAdvertisement from {:?}", ipv6_src);
            if let Some(na) = NeighborAdvertisement::parse(icmpv6_payload) {
                let mac = super::MacAddr::new(na.target_link_addr);
                neighbor_update(na.target_addr, mac);
                crate::serial_println!(
                    "[ICMPv6/NDP] Neighbor cache updated: {:?} -> {:?}",
                    na.target_addr,
                    mac
                );
            }
        }

        _ => {
            crate::serial_println!("[ICMPv6] Unknown type {} from {:?}", msg_type, ipv6_src);
        }
    }
}

pub fn send_neighbor_solicitation(target: &Ipv6Addr) -> Result<(), super::NetError> {
    let iface = super::default_interface().ok_or(super::NetError::NoInterface)?;
    let mut iface = iface.lock();
    let src_addr = link_local_from_mac(iface.mac());
    let dst_addr = solicited_node_multicast(target);

    let mut payload = NeighborSolicitation::new(*target, Some(iface.mac())).serialize();
    let checksum = compute_icmpv6_checksum(&src_addr, &dst_addr, &payload);
    payload[2] = (checksum >> 8) as u8;
    payload[3] = (checksum & 0xff) as u8;

    let packet = Ipv6Packet::new(
        Ipv6Header::new(src_addr, dst_addr, 58, payload.len() as u16),
        &payload,
    );
    let serialized = packet.serialize();
    let frame = super::ethernet::EthernetFrame::new(
        ipv6_multicast_mac(&dst_addr),
        iface.mac(),
        super::ethernet::EtherType::IPV6,
        &serialized,
    );
    let mut frame_buf = alloc::vec![0u8; 1514];
    let len = frame.serialize(&mut frame_buf)?;
    iface.send(&frame_buf[..len])?;
    crate::serial_println!(
        "[ICMPv6/NDP] NeighborSolicitation sent for {:?} via {:?}",
        target,
        dst_addr
    );
    Ok(())
}

pub fn send_router_solicitation() -> Result<(), super::NetError> {
    let iface = super::default_interface().ok_or(super::NetError::NoInterface)?;
    let mut iface = iface.lock();
    let src_addr = link_local_from_mac(iface.mac());
    let dst_addr = Ipv6Addr::new([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02]);

    let mut payload = RouterSolicitation::new(Some(iface.mac())).serialize();
    let checksum = compute_icmpv6_checksum(&src_addr, &dst_addr, &payload);
    payload[2] = (checksum >> 8) as u8;
    payload[3] = (checksum & 0xff) as u8;

    let packet = Ipv6Packet::new(
        Ipv6Header::new(src_addr, dst_addr, 58, payload.len() as u16),
        &payload,
    );
    let serialized = packet.serialize();
    let frame = super::ethernet::EthernetFrame::new(
        super::MacAddr::new([0x33, 0x33, 0x00, 0x00, 0x00, 0x02]),
        iface.mac(),
        super::ethernet::EtherType::IPV6,
        &serialized,
    );
    let mut frame_buf = alloc::vec![0u8; 1514];
    let len = frame.serialize(&mut frame_buf)?;
    iface.send(&frame_buf[..len])?;
    crate::serial_println!("[ICMPv6/NDP] RouterSolicitation sent");
    Ok(())
}

fn ipv6_multicast_mac(addr: &Ipv6Addr) -> super::MacAddr {
    super::MacAddr::new([0x33, 0x33, addr.0[12], addr.0[13], addr.0[14], addr.0[15]])
}

pub fn process_packet(data: &[u8]) -> Result<(), super::NetError> {
    let packet = Ipv6Packet::parse(data)?;
    let (next_header, payload_offset) =
        walk_extension_headers(&packet.payload, packet.header.next_header);
    if payload_offset > packet.payload.len() {
        return Err(super::NetError::InvalidPacket);
    }

    match next_header {
        58 => {
            process_icmpv6(
                &packet.header.src,
                &packet.header.dst,
                &packet.payload[payload_offset..],
            );
            Ok(())
        }
        _ => {
            crate::serial_println!(
                "[IPv6] Unsupported next header {} from {:?} to {:?}",
                next_header,
                packet.header.src,
                packet.header.dst
            );
            Ok(())
        }
    }
}

// ============================================================================
// GLOBAL KOMŞU ÖNBELLEĞİ (NEIGHBOR CACHE)
// ============================================================================
//
// IPv6 adres → MAC adres eşleşmelerini tutan global önbellek.
// NDP Neighbor Solicitation / Advertisement akışıyla doldurulur.

/// Global komşu önbelleği — tüm çekirdek genelinde paylaşılır
static NEIGHBOR_CACHE: Mutex<BTreeMap<Ipv6Addr, NeighborCacheEntry>> = Mutex::new(BTreeMap::new());

/// Komşu önbelleğinde verilen IPv6 adresi için MAC adresini arar.
///
/// Yalnızca `Reachable` veya `Stale` durumundaki girişlerin MAC'ini döndürür.
pub fn neighbor_lookup(addr: &Ipv6Addr) -> Option<super::MacAddr> {
    let cache = NEIGHBOR_CACHE.lock();
    cache.get(addr).and_then(|entry| match entry.state {
        NudState::Reachable | NudState::Stale | NudState::Delay | NudState::Probe => {
            entry.link_addr.map(|la| super::MacAddr::new(la))
        }
        _ => None,
    })
}

/// Komşu önbelleğini günceller veya yeni giriş ekler.
///
/// Giriş zaten varsa MAC ve durumu güncellenir; yoksa yeni `Reachable` giriş oluşturulur.
pub fn neighbor_update(addr: Ipv6Addr, mac: super::MacAddr) {
    let mut cache = NEIGHBOR_CACHE.lock();
    let now = crate::interrupts::get_ticks();
    let entry = cache
        .entry(addr)
        .or_insert_with(|| NeighborCacheEntry::new(addr));
    entry.confirm(*mac.as_bytes(), now);
}

/// Komşu önbelleğindeki bayat (stale) girişleri temizler.
///
/// `max_age_ticks` süresinden daha eski olan ve artık `Reachable` olmayan girişleri siler.
/// Varsayılan olarak 300 saniyelik (yaklaşık) bir eşik kullanılır.
pub fn neighbor_gc() {
    let mut cache = NEIGHBOR_CACHE.lock();
    let now = crate::interrupts::get_ticks();
    // ~300 saniye (tahminî, tick hızına bağlı — çoğu çekirdekte 1 tick ≈ 10ms)
    let max_age: u64 = 30_000;
    cache.retain(|_addr, entry| {
        let age = now.saturating_sub(entry.last_confirmed_tsc);
        // Reachable girişleri tut; diğerlerini yaş kontrolüyle at
        match entry.state {
            NudState::Reachable => true,
            NudState::Failed => false,
            _ => age < max_age,
        }
    });
}

/// IPv6 modülünü başlatır
pub fn init() {
    crate::serial_println!("[IPv6] Module initialized");
    let _ = send_router_solicitation();
}

// ============================================================================
// NDP — Neighbor Discovery Protocol (RFC 4861/4862) Tamamlama
// ============================================================================
//
// Router Solicitation/Advertisement, SLAAC, DAD, NUD durum makinesi

/// NUD (Neighbor Unreachability Detection) durumu — RFC 4861 §7.3
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudState {
    Incomplete,
    Reachable,
    Stale,
    Delay,
    Probe,
    Failed,
}

/// Komşu önbellek kaydı — NUD durum makinesi ile yönetilir.
#[derive(Debug, Clone)]
pub struct NeighborCacheEntry {
    pub ip: Ipv6Addr,
    pub link_addr: Option<[u8; 6]>,
    pub state: NudState,
    pub last_confirmed_tsc: u64,
    pub probes_sent: u32,
}

impl NeighborCacheEntry {
    pub fn new(ip: Ipv6Addr) -> Self {
        Self {
            ip,
            link_addr: None,
            state: NudState::Incomplete,
            last_confirmed_tsc: 0,
            probes_sent: 0,
        }
    }

    /// Komşu ulaşılabilir olarak işaretler.
    pub fn confirm(&mut self, link_addr: [u8; 6], current_tsc: u64) {
        self.link_addr = Some(link_addr);
        self.state = NudState::Reachable;
        self.last_confirmed_tsc = current_tsc;
        self.probes_sent = 0;
    }

    /// NUD durum geçişini kontrol eder.
    pub fn check_reachability(&mut self, current_tsc: u64, reachable_time_ticks: u64) -> NudState {
        match self.state {
            NudState::Reachable => {
                if current_tsc.saturating_sub(self.last_confirmed_tsc) > reachable_time_ticks {
                    self.state = NudState::Stale;
                }
            }
            NudState::Delay => {
                // Delay süresi dolduysa Probe'a geç
                self.state = NudState::Probe;
                self.probes_sent = 0;
            }
            NudState::Probe => {
                if self.probes_sent >= 3 {
                    self.state = NudState::Failed;
                }
            }
            _ => {}
        }
        self.state
    }

    /// Probe gönderildi bildirir.
    pub fn probe_sent(&mut self) {
        self.probes_sent += 1;
    }
}

/// SLAAC (Stateless Address Autoconfiguration) — RFC 4862
///
/// RA'dan alınan prefix + EUI-64 interface ID ile küresel adres üretir.
pub fn slaac_generate_address(prefix: &[u8; 16], prefix_len: u8, mac: &[u8; 6]) -> Ipv6Addr {
    let mut addr = [0u8; 16];
    // Prefix kopyala
    let prefix_bytes = (prefix_len as usize + 7) / 8;
    for i in 0..prefix_bytes.min(16) {
        addr[i] = prefix[i];
    }
    // EUI-64 interface ID (RFC 4291)
    addr[8] = mac[0] ^ 0x02; // Universal/Local bit ters çevir
    addr[9] = mac[1];
    addr[10] = mac[2];
    addr[11] = 0xFF;
    addr[12] = 0xFE;
    addr[13] = mac[3];
    addr[14] = mac[4];
    addr[15] = mac[5];
    Ipv6Addr(addr)
}

/// DAD (Duplicate Address Detection) durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DadState {
    /// DAD başlamadı
    NotStarted,
    /// DAD devam ediyor (NS gönderildi, NA bekleniyor)
    InProgress { probes_remaining: u8 },
    /// Adres benzersiz — kullanılabilir
    Unique,
    /// Çakışma tespit edildi
    Duplicate,
}

/// DAD işlemini başlatır.
///
/// `tentative_addr` için solicited-node multicast grubuna NS gönderilir.
/// RFC 4862, varsayılan DupAddrDetectTransmits = 1.
pub fn dad_start(tentative_addr: &Ipv6Addr) -> DadState {
    let ns_sent = send_neighbor_solicitation(tentative_addr).is_ok();
    crate::serial_println!(
        "[NDP/DAD] Starting DAD for {:?}, NS sent={}",
        tentative_addr,
        ns_sent
    );
    DadState::InProgress {
        probes_remaining: 0,
    }
}

/// Default Router listesi
static DEFAULT_ROUTERS: spin::Mutex<Vec<DefaultRouter>> = spin::Mutex::new(Vec::new());

/// Default Router kaydı
#[derive(Debug, Clone)]
pub struct DefaultRouter {
    /// Router'ın link-local adresi
    pub addr: Ipv6Addr,
    /// Geçerlilik süresi bitişi (TSC)
    pub expiry_tsc: u64,
    /// MAC adresi
    pub link_addr: [u8; 6],
}

/// Router Advertisement'tan varsayılan yönlendirici ekler.
pub fn add_default_router(addr: Ipv6Addr, lifetime_tsc: u64, current_tsc: u64, link_addr: [u8; 6]) {
    let mut routers = DEFAULT_ROUTERS.lock();
    // Mevcut mu kontrol
    for r in routers.iter_mut() {
        if r.addr.0 == addr.0 {
            r.expiry_tsc = current_tsc + lifetime_tsc;
            r.link_addr = link_addr;
            return;
        }
    }
    routers.push(DefaultRouter {
        addr,
        expiry_tsc: current_tsc + lifetime_tsc,
        link_addr,
    });
}

/// Süresi dolmuş yönlendiricileri temizler.
pub fn gc_routers(current_tsc: u64) -> usize {
    let mut routers = DEFAULT_ROUTERS.lock();
    let before = routers.len();
    routers.retain(|r| r.expiry_tsc > current_tsc);
    before - routers.len()
}

/// Varsayılan yönlendirici sayısı.
pub fn default_router_count() -> usize {
    DEFAULT_ROUTERS.lock().len()
}

/// Returns the best-known next hop and link-layer destination for an IPv6 target.
pub fn select_next_hop(dest: &Ipv6Addr, current_tsc: u64) -> Option<(Ipv6Addr, super::MacAddr)> {
    if dest.is_multicast() {
        return Some((
            *dest,
            super::MacAddr::new([0x33, 0x33, dest.0[12], dest.0[13], dest.0[14], dest.0[15]]),
        ));
    }

    if let Some(mac) = neighbor_lookup(dest) {
        return Some((*dest, mac));
    }

    if dest.is_link_local() {
        return None;
    }

    gc_routers(current_tsc);
    DEFAULT_ROUTERS
        .lock()
        .iter()
        .find(|router| router.expiry_tsc > current_tsc)
        .map(|router| (router.addr, super::MacAddr::new(router.link_addr)))
}
