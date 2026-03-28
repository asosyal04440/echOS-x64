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

use super::{local_ip, Ipv4Addr, NetError};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// IP FRAGMENT REASSEMBLY (IP PARÇA BİRLEŞTİRME)
// ============================================================================
//
// IP paketleri MTU'dan büyükse parçalanır. Alıcı taraf parçaları birleştirmelidir.
// RFC 791: Parça birleştirme zaman aşımı 15-60 saniye arası olmalıdır.
//
// FragmentKey: (src_ip, dst_ip, identification, protocol) 4'lüsü ile
// aynı orijinal pakete ait parçalar eşleştirilir.

/// Parça birleştirme tablosu anahtarı
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FragmentKey {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub identification: u16,
    pub protocol: u8,
}

/// Parça birleştirme tablosu girişi
pub struct FragmentEntry {
    /// Birleştirme tamponu — parçalar burada toplanır
    pub buffer: Vec<u8>,
    /// Hangi bayt ofsetlerinin alındığını izleyen bitmask (offset/8 bazında)
    pub received_mask: Vec<bool>,
    /// Toplam yük uzunluğu (son parça alındığında belirlenir)
    pub total_len: Option<u16>,
    /// Girişin oluşturulma zamanı (tick cinsinden)
    pub timestamp: u64,
}

/// Küresel parça birleştirme tablosu
static FRAGMENT_TABLE: Mutex<BTreeMap<FragmentKey, FragmentEntry>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IcmpEchoKey {
    peer: Ipv4Addr,
    identifier: u16,
    sequence: u16,
}

static ICMP_ECHO_OUTSTANDING: Mutex<BTreeMap<IcmpEchoKey, u64>> = Mutex::new(BTreeMap::new());
static ICMP_ECHO_REPLIES: Mutex<BTreeMap<IcmpEchoKey, u32>> = Mutex::new(BTreeMap::new());

/// Parça birleştirme zaman aşımı (15 saniye — tick cinsinden, ~1 tick/saniye varsayımı)
const FRAGMENT_TIMEOUT_TICKS: u64 = 15;

/// Gelen IP parçasını birleştirme tablosuna ekler.
///
/// Tüm parçalar tamamlandığında birleştirilmiş yükü `Some(Vec<u8>)` olarak döner.
/// Henüz eksik parça varsa `None` döner.
pub fn reassemble_fragment(header: &Ipv4Header, payload: &[u8]) -> Option<Vec<u8>> {
    let now = crate::interrupts::get_ticks();
    let key = FragmentKey {
        src_ip: header.src.to_u32(),
        dst_ip: header.dst.to_u32(),
        identification: header.identification,
        protocol: header.protocol as u8,
    };

    let offset_bytes = (header.fragment_offset as usize) * 8;
    let mf = (header.flags & 0x01) != 0; // More Fragments bayrağı

    let mut table = FRAGMENT_TABLE.lock();
    let entry = table.entry(key).or_insert_with(|| FragmentEntry {
        buffer: vec![0u8; 65535],
        received_mask: vec![false; 8192], // 65535/8 + 1
        total_len: None,
        timestamp: now,
    });

    // Parça verisini tampona kopyala
    let end = offset_bytes + payload.len();
    if end > entry.buffer.len() {
        return None; // Tampon sınırı aşıldı
    }
    entry.buffer[offset_bytes..end].copy_from_slice(payload);

    // Alınan ofsetleri işaretle (8 byte bloklar halinde)
    let block_start = offset_bytes / 8;
    let block_end = (end + 7) / 8;
    for i in block_start..block_end.min(entry.received_mask.len()) {
        entry.received_mask[i] = true;
    }

    // Son parçaysa toplam uzunluğu belirle
    if !mf {
        entry.total_len = Some(end as u16);
    }

    // Tüm parçalar tamam mı kontrol et
    if let Some(total) = entry.total_len {
        let total_blocks = (total as usize + 7) / 8;
        let complete = entry.received_mask[..total_blocks].iter().all(|&b| b);
        if complete {
            let result = entry.buffer[..total as usize].to_vec();
            table.remove(&key);
            return Some(result);
        }
    }

    None
}

/// Parça birleştirme tablosundaki süresi dolmuş girişleri temizler.
///
/// RFC 791 uyarınca 15 saniyeden eski parçalar kaldırılır.
/// Periyodik olarak (timer tick'ten veya ağ işleme döngüsünden) çağrılmalıdır.
pub fn fragment_gc() {
    let now = crate::interrupts::get_ticks();
    let mut table = FRAGMENT_TABLE.lock();
    table.retain(|_key, entry| now.wrapping_sub(entry.timestamp) < FRAGMENT_TIMEOUT_TICKS);
}

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
    pub version: u8, // 4 bits, should be 4
    pub ihl: u8,     // 4 bits, header length in 32-bit words
    pub dscp: u8,    // 6 bits
    pub ecn: u8,     // 2 bits
    pub total_length: u16,
    pub identification: u16,
    pub flags: u8,            // 3 bits
    pub fragment_offset: u16, // 13 bits
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
            ihl: 5, // Seçeneksiz: 5 × 4 = 20 bayt
            dscp: 0,
            ecn: 0,
            total_length,
            identification: 0,
            flags: 2, // DF bayrağı: parçalama yapma
            fragment_offset: 0,
            ttl: 64, // Linux/BSD varsayılan TTL
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
    let mut filtered_buf = data.to_vec();
    let prerouting_verdict = super::netfilter::process_ipv4_packet(
        &mut filtered_buf,
        super::netfilter::NF_INET_PRE_ROUTING,
        Some("eth0"),
        None,
    )?;
    if prerouting_verdict == super::netfilter::NF_DROP {
        return Ok(());
    }

    let mut packet = Ipv4Packet::parse(&filtered_buf)?;

    // ── Source Route Rejection (RFC 7126) ──
    // IHL > 5 demek IP seçenekleri mevcut; LSRR (0x83) ve SSRR (0x89) güvenlik riski.
    if packet.header.ihl > 5 {
        let header_len = packet.header.header_len();
        let options = &data[Ipv4Header::MIN_SIZE..header_len];
        let mut idx = 0;
        while idx < options.len() {
            let opt_type = options[idx];
            // End of Options List
            if opt_type == 0 {
                break;
            }
            // No-Operation
            if opt_type == 1 {
                idx += 1;
                continue;
            }
            // LSRR (0x83) veya SSRR (0x89) → paketi düşür
            if opt_type == 0x83 || opt_type == 0x89 {
                crate::serial_println!(
                    "[IP] Source route option 0x{:02X} rejected (RFC 7126)",
                    opt_type
                );
                return Ok(());
            }
            // Diğer seçenekler: uzunluk alanını oku ve atla
            if idx + 1 >= options.len() {
                break;
            }
            let opt_len = options[idx + 1] as usize;
            if opt_len < 2 {
                break;
            }
            idx += opt_len;
        }
    }

    // Check if destination is us
    let local = local_ip();
    if packet.header.dst != local
        && !packet.header.dst.is_broadcast()
        && !packet.header.dst.is_multicast()
    {
        // ── TTL Decrement on Forward ──
        // Yönlendirme durumunda TTL'i azalt; 0'a düşerse paketi sil.
        let mut fwd_data = filtered_buf.clone();
        let ttl = fwd_data[8];
        if ttl <= 1 {
            crate::serial_println!(
                "[IP] TTL expired for packet from {} -> {}",
                packet.header.src,
                packet.header.dst
            );
            // ICMP Time Exceeded (type 11) gönder — traceroute için gerekli
            send_icmp_time_exceeded(&packet.header, &fwd_data[..20]);
            return Ok(());
        }
        fwd_data[8] = ttl - 1;
        // Başlık sağlama toplamını yeniden hesapla
        let hdr_len = packet.header.header_len();
        fwd_data[10] = 0;
        fwd_data[11] = 0;
        let new_cksum = compute_checksum(&fwd_data[..hdr_len]);
        fwd_data[10..12].copy_from_slice(&new_cksum.to_be_bytes());
        // Not for us — forward would happen here; for now just drop.
        let forward_verdict = super::netfilter::process_ipv4_packet(
            &mut fwd_data,
            super::netfilter::NF_INET_FORWARD,
            Some("eth0"),
            None,
        )?;
        if forward_verdict == super::netfilter::NF_DROP {
            return Ok(());
        }
        return Ok(());
    }

    // ── Fragment Reassembly ──
    // Parçalanmış paketleri birleştir: MF bayrağı set veya fragment_offset > 0
    let mf = (packet.header.flags & 0x01) != 0;
    if mf || packet.header.fragment_offset > 0 {
        if let Some(reassembled) = reassemble_fragment(&packet.header, packet.payload) {
            let mut raw_buf = vec![0u8; packet.header.header_len() + reassembled.len()];
            let mut raw_header = packet.header;
            raw_header.flags = 0;
            raw_header.fragment_offset = 0;
            raw_header.total_length = (raw_header.header_len() + reassembled.len()) as u16;
            let raw_packet = Ipv4Packet {
                header: raw_header,
                payload: &reassembled,
            };
            if let Ok(len) = raw_packet.serialize(&mut raw_buf) {
                super::socket::deliver_raw_ipv4(&raw_buf[..len], &raw_header);
            }
            // Birleştirilmiş yükle protokol dağıtımı yap
            dispatch_protocol(&packet.header, &reassembled)?;
        }
        // Henüz tamamlanmamış — sessizce bekle
        return Ok(());
    }

    let local_in_verdict = super::netfilter::process_ipv4_packet(
        &mut filtered_buf,
        super::netfilter::NF_INET_LOCAL_IN,
        Some("eth0"),
        None,
    )?;
    if local_in_verdict == super::netfilter::NF_DROP {
        return Ok(());
    }
    packet = Ipv4Packet::parse(&filtered_buf)?;

    super::socket::deliver_raw_ipv4(&filtered_buf, &packet.header);

    // Parçalanmamış paket — doğrudan protokol dağıtımı
    dispatch_protocol(&packet.header, packet.payload)?;
    Ok(())
}

/// Protokol alanına göre paketi ilgili üst katmana yönlendirir.
fn dispatch_protocol(header: &Ipv4Header, payload: &[u8]) -> Result<(), NetError> {
    // Geçici Ipv4Packet oluştur (dispatch için)
    let pkt = Ipv4Packet {
        header: *header,
        payload,
    };
    match header.protocol {
        IpProtocol::ICMP => {
            icmp_process(&pkt)?;
        }
        IpProtocol::TCP => {
            super::tcp::process_packet(&pkt)?;
        }
        IpProtocol::UDP => {
            super::udp::process_packet(&pkt)?;
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
/// ICMP paket tipleri
const ICMP_TYPE_ECHO_REPLY: u8 = 0;
const ICMP_TYPE_ECHO_REQUEST: u8 = 8;
const ICMP_TYPE_TIME_EXCEEDED: u8 = 11; // TTL=0 veya fragment reassembly timeout

pub fn send_icmp_echo_request(
    dest: Ipv4Addr,
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Result<(), NetError> {
    let mut icmp_buf = Vec::with_capacity(8 + payload.len());
    icmp_buf.push(ICMP_TYPE_ECHO_REQUEST);
    icmp_buf.push(0);
    icmp_buf.extend_from_slice(&[0u8; 2]);
    icmp_buf.extend_from_slice(&identifier.to_be_bytes());
    icmp_buf.extend_from_slice(&sequence.to_be_bytes());
    icmp_buf.extend_from_slice(payload);

    let checksum = compute_checksum(&icmp_buf);
    icmp_buf[2..4].copy_from_slice(&checksum.to_be_bytes());

    let mut ip_buf = vec![0u8; 1500];
    let len = build_packet(dest, IpProtocol::ICMP, &icmp_buf, &mut ip_buf)?;
    match super::arp::send_to_ip(dest, &ip_buf[..len]) {
        Ok(()) | Err(NetError::WouldBlock) => {}
        Err(err) => return Err(err),
    }

    ICMP_ECHO_OUTSTANDING.lock().insert(
        IcmpEchoKey {
            peer: dest,
            identifier,
            sequence,
        },
        crate::interrupts::get_ticks(),
    );

    Ok(())
}

pub fn take_icmp_echo_reply(dest: Ipv4Addr, identifier: u16, sequence: u16) -> Option<u32> {
    ICMP_ECHO_REPLIES.lock().remove(&IcmpEchoKey {
        peer: dest,
        identifier,
        sequence,
    })
}

pub fn cancel_icmp_echo_request(dest: Ipv4Addr, identifier: u16, sequence: u16) {
    ICMP_ECHO_OUTSTANDING.lock().remove(&IcmpEchoKey {
        peer: dest,
        identifier,
        sequence,
    });
}

/// ICMP Time Exceeded mesajı gönderir (RFC 792).
///
/// Kullanım:
///   - traceroute aracı için gerekli
///   - TTL=0 olduğunda kaynak IP'ye "paketiniz yolda öldü" bildirimi
///
/// Format:
///   Type (1) = 11
///   Code (1) = 0 (TTL exceeded in transit) or 1 (fragment reassembly time exceeded)
///   Checksum (2)
///   Unused (4) = 0
///   Original IP header + first 8 bytes of original payload
pub fn send_icmp_time_exceeded(orig_ip_hdr: &Ipv4Header, orig_payload: &[u8]) {
    // ICMP Time Exceeded mesajı oluştur
    let mut icmp_buf = [0u8; 28]; // 8 byte ICMP header + 20 byte IP header

    // ICMP header
    icmp_buf[0] = ICMP_TYPE_TIME_EXCEEDED;
    icmp_buf[1] = 0; // Code 0: TTL exceeded in transit
    icmp_buf[2..4].copy_from_slice(&[0u8; 2]); // Checksum will be calculated
    icmp_buf[4..8].copy_from_slice(&[0u8; 4]); // Unused

    // Orijinal IP header'ın ilk 20 byte'ını ekle (RFC 792)
    let ip_header_bytes = [
        ((orig_ip_hdr.version << 4) | orig_ip_hdr.ihl),
        ((orig_ip_hdr.dscp << 2) | orig_ip_hdr.ecn),
        orig_ip_hdr.total_length.to_be_bytes()[0],
        orig_ip_hdr.total_length.to_be_bytes()[1],
        orig_ip_hdr.identification.to_be_bytes()[0],
        orig_ip_hdr.identification.to_be_bytes()[1],
        (((orig_ip_hdr.flags as u16) << 5) | ((orig_ip_hdr.fragment_offset >> 8) & 0x1F)) as u8,
        orig_ip_hdr.fragment_offset.to_be_bytes()[1],
        orig_ip_hdr.ttl,
        orig_ip_hdr.protocol as u8,
        orig_ip_hdr.checksum.to_be_bytes()[0],
        orig_ip_hdr.checksum.to_be_bytes()[1],
        orig_ip_hdr.src.0[0],
        orig_ip_hdr.src.0[1],
        orig_ip_hdr.src.0[2],
        orig_ip_hdr.src.0[3],
        orig_ip_hdr.dst.0[0],
        orig_ip_hdr.dst.0[1],
        orig_ip_hdr.dst.0[2],
        orig_ip_hdr.dst.0[3],
    ];
    icmp_buf[8..28].copy_from_slice(&ip_header_bytes);

    // ICMP checksum hesapla
    let cksum = compute_checksum(&icmp_buf);
    icmp_buf[2..4].copy_from_slice(&cksum.to_be_bytes());

    // IP paketi oluştur: hedef = orijinal kaynak, kaynak = biz
    let mut ip_buf = vec![0u8; 64];
    match build_packet(orig_ip_hdr.src, IpProtocol::ICMP, &icmp_buf, &mut ip_buf) {
        Ok(len) => match super::arp::send_to_ip(orig_ip_hdr.src, &ip_buf[..len]) {
            Ok(()) | Err(NetError::WouldBlock) => {
                crate::serial_println!(
                    "[ICMP] Time Exceeded sent to {} (orig src: {})",
                    orig_ip_hdr.src,
                    orig_ip_hdr.dst
                );
            }
            Err(e) => {
                crate::serial_println!("[ICMP] Failed to send Time Exceeded: {:?}", e);
            }
        },
        Err(e) => {
            crate::serial_println!("[ICMP] Failed to build Time Exceeded packet: {:?}", e);
        }
    }
}

pub fn icmp_process(packet: &Ipv4Packet) -> Result<(), NetError> {
    if packet.payload.len() < 8 {
        crate::serial_println!("[ICMP] Paket çok kısa: {} bayt", packet.payload.len());
        return Err(NetError::InvalidPacket);
    }

    let icmp_type = packet.payload[0];
    let icmp_code = packet.payload[1];
    // payload[2..4] = checksum, payload[4..6] = identifier, payload[6..8] = sequence

    crate::serial_println!(
        "[ICMP] packet from {}: type={} code={} len={}",
        packet.header.src,
        icmp_type,
        icmp_code,
        packet.payload.len()
    );

    if icmp_type == ICMP_TYPE_ECHO_REQUEST && icmp_code == 0 {
        // ── ICMP Echo Reply oluştur ──
        // Tip=0, Kod=0, aynı tanımlayıcı/sıra/veri korunur
        let mut reply_payload = packet.payload.to_vec();
        reply_payload[0] = ICMP_TYPE_ECHO_REPLY; // type = 0 (echo reply)
        reply_payload[1] = 0; // code = 0
                              // Checksum alanını sıfırla ve yeniden hesapla
        reply_payload[2] = 0;
        reply_payload[3] = 0;
        let cksum = compute_checksum(&reply_payload);
        reply_payload[2..4].copy_from_slice(&cksum.to_be_bytes());

        // IP paketi oluştur ve gönder: hedef = orijinal kaynak, kaynak = biz
        let mut ip_buf = vec![0u8; 1500];
        let len = build_packet(
            packet.header.src,
            IpProtocol::ICMP,
            &reply_payload,
            &mut ip_buf,
        )?;

        match super::arp::send_to_ip(packet.header.src, &ip_buf[..len]) {
            Ok(()) | Err(NetError::WouldBlock) => {}
            Err(err) => return Err(err),
        }

        crate::serial_println!("[ICMP] Echo Reply sent to {}", packet.header.src);
    } else if icmp_type == ICMP_TYPE_ECHO_REPLY && icmp_code == 0 && packet.payload.len() >= 8 {
        let identifier = u16::from_be_bytes([packet.payload[4], packet.payload[5]]);
        let sequence = u16::from_be_bytes([packet.payload[6], packet.payload[7]]);
        let key = IcmpEchoKey {
            peer: packet.header.src,
            identifier,
            sequence,
        };
        let now = crate::interrupts::get_ticks();
        let rtt = if let Some(sent_tick) = ICMP_ECHO_OUTSTANDING.lock().remove(&key) {
            now.saturating_sub(sent_tick) as u32
        } else {
            0
        };
        ICMP_ECHO_REPLIES.lock().insert(key, rtt);
        crate::serial_println!(
            "[ICMP] Echo Reply received from {} id={} seq={} rtt={}",
            packet.header.src,
            identifier,
            sequence,
            rtt
        );
    }

    Ok(())
}
