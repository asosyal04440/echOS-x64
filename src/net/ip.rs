//! # IP Katmanı (IPv4)
//!
//! IPv4 paket ayrıştırma, oluşturma ve yönlendirme.
//!
//! ## IPv4 Paket Yapısı (minimum 20 bayt başlık)
//!
//! ```
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! ┌─────────┬─────────┬─────────────────┬─────────────────────────────┐
//! │ Versiyon│   IHL   │ DSCP        │ECN│        Toplam Uzunluk        │
//! │  (4bit) │  (4bit) │  (6bit)     │(2)│           (16 bit)          │
//! ├─────────────────────────────────┼──┬──┬─────────────────────────────┤
//! │          Kimlik (16 bit)        │R │DF│MF│   Parça Ofseti (13 bit)  │
//! ├─────────────────┬───────────────┴──┴──┴──────────────────────────────┤
//! │  TTL (8 bit)    │ Protokol (8 bit)  │       Başlık Sağlama Toplamı   │
//! ├─────────────────┴──────────────────┴───────────────────────────────┤
//! │                      Kaynak IP Adresi (32 bit)                     │
//! ├───────────────────────────────────────────────────────────────────┤
//! │                      Hedef IP Adresi (32 bit)                      │
//! └───────────────────────────────────────────────────────────────────┘
//!
//! Alan Açıklamaları:
//! - Versiyon: IPv4 için 4
//! - IHL: Başlık uzunluğu 32-bit word sayısı cinsinden (minimum 5 = 20 bayt)
//! - DSCP/ECN: Servis kalitesi (QoS) ve tıkanıklık bildirimi
//! - Toplam Uzunluk: Başlık + veri (bayt)
//! - Kimlik + Bayraklar + Parça Ofseti: IP parçalanması için
//! - TTL: Paket ömrü (her yönlendiricide 1 azalır, 0'da düşürülür)
//! - Protokol: Üst katman (1=ICMP, 6=TCP, 17=UDP)
//! - Sağlama Toplamı: Yalnızca başlık için (veri dahil değil)
//! ```
//!
//! ## IP Yönlendirme Kararı
//!
//! ```
//!  Gönderilecek paket
//!         │
//!         ▼
//!  Hedef aynı alt ağda mı?
//!  (dst_ip & maske == local_ip & maske)
//!         │
//!    Evet │         Hayır
//!         ▼           ▼
//!  Doğrudan ilet    Varsayılan ağ geçidine gönder
//!  (ARP ile MAC)    (gateway IP → ARP → MAC)
//! ```

use super::{Ipv4Addr, NetError, local_ip};
use alloc::vec::Vec;

/// IP protokol numaraları
///
/// IPv4 başlığındaki `protocol` alanı, paketin taşıdığı üst katman protokolünü belirtir.
/// Tam liste IANA tarafından yönetilir; en yaygın olanlar burada tanımlıdır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IpProtocol {
    ICMP = 1,    // İnternet Kontrol Mesaj Protokolü (ping, hata bildirimleri)
    TCP = 6,     // İletim Kontrolü Protokolü (güvenilir, bağlantı tabanlı)
    UDP = 17,    // Kullanıcı Datagram Protokolü (hızlı, bağlantısız)
    UNKNOWN = 0, // Tanınmayan protokol
}

impl IpProtocol {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => IpProtocol::ICMP,
            6 => IpProtocol::TCP,
            17 => IpProtocol::UDP,
            _ => IpProtocol::UNKNOWN,
        }
    }
}

/// IPv4 başlığı (minimum 20 bayt)
///
/// `ihl` alanı başlığın 32-bit word sayısını belirtir.
/// `ihl == 5` → 20 bayt (seçeneksiz standart başlık)
/// `ihl > 5`  → IP seçenekleri mevcut (en fazla 60 bayt)
///
/// Sağlama toplamı hesabı: başlık baytları 16-bit word'lere bölünür,
/// tümleyen toplamı alınır, sonuç 0xFFFF olmalıdır (doğrulama).
#[derive(Clone, Copy, Debug)]
pub struct Ipv4Header {
    pub version: u8,           // 4 bits, should be 4
    pub ihl: u8,               // 4 bits, header length in 32-bit words
    pub dscp: u8,              // 6 bits
    pub ecn: u8,               // 2 bits
    pub total_length: u16,
    pub identification: u16,
    pub flags: u8,             // 3 bits
    pub fragment_offset: u16,  // 13 bits
    pub ttl: u8,
    pub protocol: IpProtocol,
    pub checksum: u16,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
}

impl Ipv4Header {
    /// Minimum IPv4 başlık boyutu (seçeneksiz): 20 bayt
    pub const MIN_SIZE: usize = 20;

    /// Bayt dizisinden IPv4 başlığını ayrıştırır.
    ///
    /// Ayrıştırma adımları:
    /// 1. En az 20 bayt var mı?
    /// 2. Versiyon 4 mü?
    /// 3. IHL >= 5 mi? (geçersiz başlık boyutu)
    /// 4. Sağlama toplamı doğru mu?
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }

        let version = (data[0] >> 4) & 0x0F;
        if version != 4 {
            return Err(NetError::InvalidPacket);
        }

        let ihl = data[0] & 0x0F;
        if ihl < 5 {
            return Err(NetError::InvalidPacket);
        }

        let dscp = (data[1] >> 2) & 0x3F;
        let ecn = data[1] & 0x03;
        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        // Bayraklar 3 bit, parça ofseti 13 bit: birlikte 2 bayt
        let flags = (data[6] >> 5) & 0x07;
        let fragment_offset = u16::from_be_bytes([data[6] & 0x1F, data[7]]);
        let ttl = data[8];
        let protocol = IpProtocol::from_u8(data[9]);
        let checksum = u16::from_be_bytes([data[10], data[11]]);
        let src = Ipv4Addr::from_bytes([data[12], data[13], data[14], data[15]]);
        let dst = Ipv4Addr::from_bytes([data[16], data[17], data[18], data[19]]);

        // Verify checksum
        let header_len = (ihl as usize) * 4;
        if data.len() < header_len {
            return Err(NetError::InvalidPacket);
        }

        // Sağlama toplamı doğrulaması: tüm başlık üzerinde hesapla; sonuç 0 olmalı
        let computed_checksum = compute_checksum(&data[..header_len]);
        if computed_checksum != 0 {
            return Err(NetError::ChecksumError);
        }

        Ok(Ipv4Header {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            ttl,
            protocol,
            checksum,
            src,
            dst,
        })
    }

    /// IPv4 başlığını bayt dizisine yazar.
    ///
    /// Sağlama toplamı alanı sıfır bırakılır; `Ipv4Packet::serialize()` hesaplar.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        let header_len = (self.ihl as usize) * 4;
        if buf.len() < header_len {
            return Err(NetError::BufferFull);
        }

        // İlk bayt: versiyon (4 bit) + IHL (4 bit) birleştirilmiş
        buf[0] = (self.version << 4) | self.ihl;
        // İkinci bayt: DSCP (6 bit) + ECN (2 bit)
        buf[1] = (self.dscp << 2) | self.ecn;
        buf[2..4].copy_from_slice(&self.total_length.to_be_bytes());
        buf[4..6].copy_from_slice(&self.identification.to_be_bytes());
        // Bayraklar + parça ofseti birleştirilmiş
        buf[6] = (self.flags << 5) | ((self.fragment_offset >> 8) as u8 & 0x1F);
        buf[7] = self.fragment_offset as u8;
        buf[8] = self.ttl;
        buf[9] = self.protocol as u8;
        // Sağlama toplamı: şimdilik sıfır, sonra hesaplanacak
        buf[10..12].copy_from_slice(&self.checksum.to_be_bytes());
        buf[12..16].copy_from_slice(self.src.as_bytes());
        buf[16..20].copy_from_slice(self.dst.as_bytes());

        Ok(())
    }

    /// Varsayılan değerlerle yeni bir IPv4 başlığı oluşturur.
    ///
    /// - TTL: 64 (Linux varsayılanı)
    /// - Bayraklar: 0x02 = DF (Don't Fragment - parçalama)
    /// - Sağlama toplamı: 0 (serialize sırasında hesaplanır)
    pub fn new(src: Ipv4Addr, dst: Ipv4Addr, protocol: IpProtocol, total_length: u16) -> Self {
        Ipv4Header {
            version: 4,
            ihl: 5,     // Seçeneksiz: 5 × 4 = 20 bayt
            dscp: 0,
            ecn: 0,
            total_length,
            identification: 0,
            flags: 2,   // DF bayrağı: parçalama yapma
            fragment_offset: 0,
            ttl: 64,    // Linux/BSD varsayılan TTL
            protocol,
            checksum: 0,
            src,
            dst,
        }
    }

    /// Başlık uzunluğunu bayt cinsinden döner (IHL × 4).
    pub fn header_len(&self) -> usize {
        (self.ihl as usize) * 4
    }

    /// Yük (payload) uzunluğunu döner (toplam_uzunluk - başlık_uzunluğu).
    pub fn payload_len(&self) -> usize {
        self.total_length as usize - self.header_len()
    }
}

/// Yük dahil IPv4 paketi
///
/// `payload` alanı başlık sonrasındaki ham veriyi işaret eder.
/// Yaşam süresi (lifetime) `'a`, paketin kaynak tampona bağlı olduğunu gösterir.
#[derive(Clone, Debug)]
pub struct Ipv4Packet<'a> {
    pub header: Ipv4Header,
    pub payload: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    /// Bayt dizisinden IPv4 paketi ayrıştırır.
    ///
    /// Başlık başarıyla ayrıştırıldıktan sonra yük dilimi hesaplanır.
    /// `total_length` alandaki değer dışındaki baytlar görmezden gelinir.
    pub fn parse(data: &'a [u8]) -> Result<Self, NetError> {
        let header = Ipv4Header::parse(data)?;
        let header_len = header.header_len();

        if data.len() < header.total_length as usize {
            return Err(NetError::InvalidPacket);
        }

        // Yük = başlık_sonundan → total_length'a kadar
        let payload = &data[header_len..header.total_length as usize];

        Ok(Ipv4Packet { header, payload })
    }

    /// Yeni bir IPv4 paketi oluşturur.
    ///
    /// `total_length` otomatik hesaplanır: başlık + yük boyutu.
    pub fn new(src: Ipv4Addr, dst: Ipv4Addr, protocol: IpProtocol, payload: &'a [u8]) -> Self {
        let total_length = (Ipv4Header::MIN_SIZE + payload.len()) as u16;
        let header = Ipv4Header::new(src, dst, protocol, total_length);
        Ipv4Packet { header, payload }
    }

    /// Paketi baytlara serileştirir ve doğru sağlama toplamını hesaplar.
    ///
    /// Sağlama toplamı hesabı:
    /// 1. Başlığı sıfır checksum ile yaz
    /// 2. Yükü başlık arkasına kopyala
    /// 3. Başlık baytları üzerinde sağlama toplamı hesapla
    /// 4. Sağlama toplamını [10..12] konumuna yaz
    ///
    /// Başarılı olursa yazılan toplam bayt sayısını döner.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let total_len = self.header.header_len() + self.payload.len();
        if buf.len() < total_len {
            return Err(NetError::BufferFull);
        }

        // Serialize header with zero checksum
        let mut header = self.header;
        header.checksum = 0;
        header.serialize(buf)?;

        // Copy payload
        buf[self.header.header_len()..total_len].copy_from_slice(self.payload);

        // Compute and set checksum
        let checksum = compute_checksum(&buf[..self.header.header_len()]);
        buf[10..12].copy_from_slice(&checksum.to_be_bytes());

        Ok(total_len)
    }
}

/// IP sağlama toplamı hesabı (tümleyen - one's complement)
///
/// RFC 791'de tanımlı algoritma:
/// 1. Veriyi 16-bit word'lere böl ve topla
/// 2. Taşan bitleri (carry) toplama ekle (fold carries)
/// 3. Sonucun tümleyen tersi sağlama toplamıdır
///
/// Doğrulama: sağlama toplamı dahil tüm başlık üzerinde hesap yapılırsa sonuç 0 olmalı.
///
/// ```
/// Örnek: 0x4500 + 0x0034 + 0xFFFF = 0x14534
///        carry fold: 0x4534 + 0x0001 = 0x4535
///        NOT(0x4535) = 0xBACA ← sağlama toplamı
/// ```
pub fn compute_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sum 16-bit words
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            // Tek bayt kaldıysa yüksek bayta yerleştir, düşük bayt sıfır
            (chunk[0] as u16) << 8
        };
        sum += word as u32;
    }

    // Fold carries: 32-bit toplamın yüksek 16 bitini düşük 16 bite ekle
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // One's complement (tümleyen tersi)
    !(sum as u16)
}

/// Gelen IPv4 paketini işler ve doğru protokol katmanına yönlendirir.
///
/// İşleme adımları:
/// 1. Paketi ayrıştır (başlık + sağlama toplamı doğrulama)
/// 2. Hedef IP bizim mi? (unicast, broadcast veya multicast)
/// 3. Protokol alanına göre ICMP, TCP veya UDP'ye yönlendir
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    let packet = Ipv4Packet::parse(data)?;

    // Check if destination is us
    let local = local_ip();
    if packet.header.dst != local &&
       !packet.header.dst.is_broadcast() &&
       !packet.header.dst.is_multicast() {
        // Not for us, drop
        return Ok(());
    }

    // Dispatch by protocol
    match packet.header.protocol {
        IpProtocol::ICMP => {
            icmp_process(&packet)?;
        }
        IpProtocol::TCP => {
            super::tcp::process_packet(&packet)?;
        }
        IpProtocol::UDP => {
            super::udp::process_packet(&packet)?;
        }
        _ => {
            // Unknown protocol
        }
    }

    Ok(())
}

/// Gönderilmek üzere IPv4 paketi oluşturur.
///
/// Kaynak IP otomatik olarak yerel IP'den alınır.
/// Başarılı olursa `buf`'a yazılan bayt sayısını döner.
pub fn build_packet(
    dst: Ipv4Addr,
    protocol: IpProtocol,
    payload: &[u8],
    buf: &mut [u8],
) -> Result<usize, NetError> {
    let src = local_ip();
    let packet = Ipv4Packet::new(src, dst, protocol, payload);
    packet.serialize(buf)
}

/// Hedef IP için bir sonraki atlama (next hop) adresini belirler.
///
/// ```
/// Hedef IP
///    │
///    ├── Aynı alt ağda? → Hedef IP'yi doğrudan döner
///    │   (dst & maske == local & maske)
///    │
///    └── Farklı alt ağ? → Varsayılan ağ geçidini döner
///        (yapılandırılmışsa)
/// ```
pub fn route(dst: Ipv4Addr) -> Option<Ipv4Addr> {
    let local = local_ip();

    // Same subnet - direct delivery
    if is_same_subnet(local, dst) {
        return Some(dst);
    }

    // Different subnet - use gateway
    let config = super::get_config();
    if config.gateway != [0, 0, 0, 0] {
        return Some(Ipv4Addr::from_bytes(config.gateway));
    }

    None
}

/// İki IP adresinin aynı alt ağda olup olmadığını kontrol eder.
///
/// Alt ağ maskesi ağ yapılandırmasından alınır.
/// Her iki IP de maskeyle AND'lendikten sonra eşitse aynı ağdalar.
pub fn is_same_subnet(a: Ipv4Addr, b: Ipv4Addr) -> bool {
    let config = super::get_config();
    let mask = Ipv4Addr::from_bytes(config.netmask);

    (a.to_u32() & mask.to_u32()) == (b.to_u32() & mask.to_u32())
}

/// ICMP işlemi (taslak)
///
/// Gerçek bir uygulama şunları yapardı:
/// - ICMP Echo Request'e Echo Reply göndermek
/// - Destination Unreachable mesajı üretmek
/// - Time Exceeded mesajı üretmek (traceroute için)
///
/// Şu anlık sadece seri porta kayıt yazar.
pub fn icmp_process(packet: &Ipv4Packet) -> Result<(), NetError> {
    // TODO: Implement ICMP echo reply
    crate::serial_println!(
        "[NET] ICMP packet from {}: {} bytes",
        packet.header.src,
        packet.payload.len()
    );
    Ok(())
}
