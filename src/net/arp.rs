//! # ARP Protokolü (Address Resolution Protocol)
//!
//! ARP, ağ katmanı (Layer 3) IP adreslerini veri bağlantı katmanı (Layer 2)
//! MAC adreslerine çevirmek için kullanılan temel bir protokoldür.
//! RFC 826 ile tanımlanmıştır.
//!
//! ## Ağ Katmanları ve ARP'nin Yeri
//!
//! ```text
//! +---------------------+
//! |  Uygulama Katmanı   |  <-- HTTP, DNS, SSH vb.
//! +---------------------+
//! |  Taşıma Katmanı     |  <-- TCP, UDP
//! +---------------------+
//! |  Ağ Katmanı  (L3)   |  <-- IP (IPv4/IPv6)
//! +---------------------+
//! |  Veri Bağlantısı(L2)|  <-- Ethernet + ARP <-- BU KATMAN
//! +---------------------+
//! |  Fiziksel Katman    |  <-- Kablo, WiFi sinyali
//! +---------------------+
//! ```
//!
//! ## ARP Paket Yapısı (28 Byte)
//!
//! ```text
//! Byte:  0    1    2    3    4    5    6    7
//!       +----+----+----+----+----+----+----+----+
//!       | HTYPE(2)|  PTYPE(2) |HLEN|PLEN|OPER(2)|
//!       +----+----+----+----+----+----+----+----+
//! Byte:  8    9   10   11   12   13
//!       +----+----+----+----+----+----+
//!       |     SHA (Gönderen MAC, 6B)  |
//!       +----+----+----+----+----+----+
//! Byte: 14   15   16   17
//!       +----+----+----+----+
//!       |  SPA (Gönderen IP)|
//!       +----+----+----+----+
//! Byte: 18   19   20   21   22   23
//!       +----+----+----+----+----+----+
//!       |     THA (Hedef MAC, 6B)     |
//!       +----+----+----+----+----+----+
//! Byte: 24   25   26   27
//!       +----+----+----+----+
//!       |   TPA (Hedef IP)  |
//!       +----+----+----+----+
//!
//! HTYPE: Donanım türü (1 = Ethernet)
//! PTYPE: Protokol türü (0x0800 = IPv4)
//! HLEN : Donanım adres uzunluğu (6 byte, MAC adresi)
//! PLEN : Protokol adres uzunluğu (4 byte, IPv4)
//! OPER : İşlem (1 = İstek, 2 = Yanıt)
//! SHA  : Sender Hardware Address (Gönderenin MAC adresi)
//! SPA  : Sender Protocol Address (Gönderenin IP adresi)
//! THA  : Target Hardware Address (Hedefin MAC adresi)
//! TPA  : Target Protocol Address (Hedefin IP adresi)
//! ```
//!
//! ## ARP İstek/Yanıt Akışı
//!
//! ```text
//! Bilgisayar A (192.168.1.10)       Bilgisayar B (192.168.1.20)
//!        |                                    |
//!        |  "192.168.1.20'nin MAC'i nedir?"   |
//!        |------- ARP REQUEST (broadcast) --->|
//!        |        (THA = FF:FF:FF:FF:FF:FF)   |
//!        |                                    |
//!        |  "Benim MAC'im AA:BB:CC:DD:EE:FF"  |
//!        |<------- ARP REPLY (unicast) --------|
//!        |                                    |
//!        | (A, B'nin MAC adresini önbelleğe   |
//!        |  kaydeder ve iletişimi başlatır)    |
//! ```

use super::{MacAddr, Ipv4Addr, NetError, local_ip};
use super::ethernet::{EtherType, EthernetFrame};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// ARP başlık yapısı.
///
/// Bir ARP paketinin tüm alanlarını içerir.
/// Hem istek (Request) hem de yanıt (Reply) paketleri için kullanılır.
///
/// ```text
/// İstek paketinde THA=00:00:00:00:00:00 (bilinmiyor)
/// Yanıt paketinde THA, SPA sahibinin gerçek MAC adresidir.
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ArpHeader {
    pub htype: u16,      // Donanım türü: 1 = Ethernet (IEEE 802.3)
    pub ptype: u16,      // Protokol türü: 0x0800 = IPv4
    pub hlen: u8,        // Donanım adres uzunluğu: Ethernet için 6 byte (MAC)
    pub plen: u8,        // Protokol adres uzunluğu: IPv4 için 4 byte
    pub oper: ArpOperation,
    pub sha: MacAddr,    // Sender Hardware Address: Gönderenin MAC adresi
    pub spa: Ipv4Addr,   // Sender Protocol Address: Gönderenin IP adresi
    pub tha: MacAddr,    // Target Hardware Address: Hedefin MAC adresi (istek'te sıfır)
    pub tpa: Ipv4Addr,   // Target Protocol Address: Hedefin IP adresi (çözümlenmek istenen)
}

/// ARP işlem kodu.
///
/// ARP paketi bir istek mi (kim bu IP?) yoksa yanıt mı (bu IP bende!) olduğunu belirtir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ArpOperation {
    Request = 1,  // ARP isteği: "Bu IP kimin?" sorusu (broadcast ile gönderilir)
    Reply = 2,    // ARP yanıtı: "Bu IP benim!" cevabı (unicast ile gönderilir)
    Unknown = 0,  // Bilinmeyen işlem kodu
}

impl ArpOperation {
    pub fn from_u16(val: u16) -> Self {
        match val {
            1 => ArpOperation::Request,
            2 => ArpOperation::Reply,
            _ => ArpOperation::Unknown,
        }
    }
}

impl ArpHeader {
    /// ARP paketi sabit boyutu: 28 byte.
    ///
    /// Ethernet için: 2+2+1+1+2 + 6+4+6+4 = 28 byte
    pub const SIZE: usize = 28;

    /// Ham byte dizisinden ARP başlığını ayrıştırır.
    ///
    /// Ağ byte sırası (big-endian) kullanılarak okuma yapılır.
    /// Gelen veri 28 byte'tan kısa ise hata döner.
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }

        let htype = u16::from_be_bytes([data[0], data[1]]);
        let ptype = u16::from_be_bytes([data[2], data[3]]);
        let hlen = data[4];
        let plen = data[5];
        let oper = ArpOperation::from_u16(u16::from_be_bytes([data[6], data[7]]));

        let sha = MacAddr::new([data[8], data[9], data[10], data[11], data[12], data[13]]);
        let spa = Ipv4Addr::from_bytes([data[14], data[15], data[16], data[17]]);
        let tha = MacAddr::new([data[18], data[19], data[20], data[21], data[22], data[23]]);
        let tpa = Ipv4Addr::from_bytes([data[24], data[25], data[26], data[27]]);

        Ok(ArpHeader {
            htype, ptype, hlen, plen, oper, sha, spa, tha, tpa,
        })
    }

    /// ARP başlığını byte dizisine seri hale getirir.
    ///
    /// Tüm çok-byte alanlar ağ byte sırasına (big-endian) dönüştürülür.
    /// Hedef tampon 28 byte'tan küçük ise hata verir.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }

        buf[0..2].copy_from_slice(&self.htype.to_be_bytes());
        buf[2..4].copy_from_slice(&self.ptype.to_be_bytes());
        buf[4] = self.hlen;
        buf[5] = self.plen;
        buf[6..8].copy_from_slice(&(self.oper as u16).to_be_bytes());
        buf[8..14].copy_from_slice(self.sha.as_bytes());
        buf[14..18].copy_from_slice(self.spa.as_bytes());
        buf[18..24].copy_from_slice(self.tha.as_bytes());
        buf[24..28].copy_from_slice(self.tpa.as_bytes());

        Ok(())
    }

    /// Yeni bir ARP isteği (Request) paketi oluşturur.
    ///
    /// Hedef MAC adresi (THA) bilinmediği için sıfır olarak ayarlanır.
    /// Bu paket broadcast (FF:FF:FF:FF:FF:FF) olarak gönderilmelidir.
    pub fn new_request(sha: MacAddr, spa: Ipv4Addr, tpa: Ipv4Addr) -> Self {
        ArpHeader {
            htype: 1,
            ptype: 0x0800,
            hlen: 6,
            plen: 4,
            oper: ArpOperation::Request,
            sha,
            spa,
            tha: MacAddr::ZERO,  // Hedef MAC henüz bilinmiyor, sıfır gönderilir
            tpa,
        }
    }

    /// Yeni bir ARP yanıtı (Reply) paketi oluşturur.
    ///
    /// İstek yapan cihaza kendi MAC adresimizi bildiririz.
    /// Bu paket unicast (isteği gönderenin MAC adresine) olarak gönderilmelidir.
    pub fn new_reply(sha: MacAddr, spa: Ipv4Addr, tha: MacAddr, tpa: Ipv4Addr) -> Self {
        ArpHeader {
            htype: 1,
            ptype: 0x0800,
            hlen: 6,
            plen: 4,
            oper: ArpOperation::Reply,
            sha,
            spa,
            tha,
            tpa,
        }
    }
}

// ============================================================================
// ARP CACHE (ARP ÖNBELLEĞI)
// ============================================================================
//
// ARP önbelleği, daha önce çözümlenmiş IP->MAC eşleşmelerini saklar.
// Böylece her iletişimde ARP isteği göndermemize gerek kalmaz.
//
// Önbellek yapısı:
//   IP Adresi (u32)  -->  MAC Adresi (6 byte)
//
//   Örnek:
//   192.168.1.1   -->  AA:BB:CC:DD:EE:01
//   192.168.1.20  -->  AA:BB:CC:DD:EE:FF
//
// ARP_PENDING: MAC adresi henüz çözümlenmemiş IP'ler için bekleyen
// paketleri tutar. MAC adresi öğrenilince bu paketler otomatik gönderilir.

static ARP_CACHE: Mutex<BTreeMap<u32, MacAddr>> = Mutex::new(BTreeMap::new());
static ARP_PENDING: Mutex<BTreeMap<u32, Vec<Vec<u8>>>> = Mutex::new(BTreeMap::new());

/// ARP alt sistemini başlatır.
///
/// Sistem başlangıcında çağrılmalıdır. Önbellek ve bekleyen paket
/// kuyruğu bu aşamada boş olarak hazırlanır.
pub fn init() {
    crate::serial_println!("[ARP] Initialized");
}

/// Belirtilen IP adresine karşılık gelen MAC adresini önbellekten arar.
///
/// Önbellekte kayıt varsa MAC adresini döner, yoksa `None` döner.
/// `None` durumunda ARP isteği gönderilmeli ve yanıt beklenmeli.
pub fn resolve(ip: Ipv4Addr) -> Option<MacAddr> {
    ARP_CACHE.lock().get(&ip.to_u32()).copied()
}

/// ARP tablosundaki tüm kayıtları döner.
///
/// Ağ yönetimi ve hata ayıklama amacıyla önbellekteki tüm
/// IP->MAC eşleşmelerinin listesini verir.
pub fn get_table() -> Vec<(Ipv4Addr, MacAddr)> {
    let cache = ARP_CACHE.lock();
    cache.iter()
        .map(|(&ip, &mac)| (Ipv4Addr::from_u32(ip), mac))
        .collect()
}

/// ARP önbelleğine yeni bir IP->MAC eşleşmesi ekler.
///
/// Bu fonksiyon şunları yapar:
/// 1. Yeni eşleşmeyi önbelleğe kaydeder.
/// 2. Bu IP için bekleyen paket varsa hepsini gönderir.
///
/// ARP yanıtı alındığında veya başka bir kaynaktan MAC öğrenildiğinde çağrılır.
pub fn add_entry(ip: Ipv4Addr, mac: MacAddr) {
    ARP_CACHE.lock().insert(ip.to_u32(), mac);

    // MAC adresi öğrenilince bu IP için bekleyen paketleri gönder
    let mut pending = ARP_PENDING.lock();
    if let Some(packets) = pending.remove(&ip.to_u32()) {
        drop(pending);

        for packet in packets {
            // Çözümlenen MAC adresiyle yeniden gönder
            let _ = send_to_ip(ip, &packet);
        }
    }
}

/// Belirtilen IP adresi için ARP isteği (Request) gönderir.
///
/// Broadcast Ethernet çerçevesi (FF:FF:FF:FF:FF:FF) oluşturur ve
/// yerel ağ üzerindeki tüm cihazlara gönderir.
/// Hedef IP sahibi olan cihaz ARP yanıtı ile MAC adresini bildirir.
pub fn send_request(tpa: Ipv4Addr) -> Result<(), NetError> {
    let iface = super::default_interface().ok_or(NetError::NoInterface)?;
    let mut iface = iface.lock();

    let sha = iface.mac();
    let spa = iface.ip();

    let arp = ArpHeader::new_request(sha, spa, tpa);
    let mut buf = alloc::vec![0u8; ArpHeader::SIZE];
    arp.serialize(&mut buf)?;

    // Broadcast MAC adresiyle Ethernet çerçevesi oluştur
    let mut frame_buf = alloc::vec![0u8; 1514];
    let frame = EthernetFrame::new(
        MacAddr::BROADCAST,
        sha,
        EtherType::ARP,
        &buf,
    );
    let len = frame.serialize(&mut frame_buf)?;

    iface.send(&frame_buf[..len])?;

    crate::serial_println!("[ARP] Request: Who has {}?",
        super::socket::format_ipv4(tpa));

    Ok(())
}

/// Belirtilen IP adresine veri paketi gönderir; gerekirse MAC çözümlemesi yapar.
///
/// Akış şeması:
/// ```text
/// send_to_ip(ip, data)
///       |
///       v
/// ARP önbelleğinde MAC var mı?
///   +-EVET---> Ethernet çerçevesi oluştur ve gönder
///   |
///   +--HAYIR-> Paketi bekleyen kuyruğuna al
///              ARP isteği gönder
///              WouldBlock hatası döndür
///              (MAC gelince paket otomatik gönderilecek)
/// ```
pub fn send_to_ip(ip: Ipv4Addr, data: &[u8]) -> Result<(), NetError> {
    // Yönlendirme tablosuna göre sonraki atlamayı (next hop) belirle
    let next_hop = super::ip::route(ip).unwrap_or(ip);

    // ARP önbelleğini kontrol et
    if let Some(mac) = resolve(next_hop) {
        let iface = super::default_interface().ok_or(NetError::NoInterface)?;
        let mut iface = iface.lock();

        let mut frame_buf = alloc::vec![0u8; 1514];
        let frame = EthernetFrame::new(
            mac,
            iface.mac(),
            EtherType::IPV4,
            data,
        );
        let len = frame.serialize(&mut frame_buf)?;

        iface.send(&frame_buf[..len])?;
        Ok(())
    } else {
        // MAC bilinmiyor: paketi kuyruğa al ve ARP isteği gönder
        ARP_PENDING.lock().entry(next_hop.to_u32()).or_default().push(data.to_vec());
        send_request(next_hop)?;
        Err(NetError::WouldBlock)
    }
}

/// Gelen ARP paketini işler.
///
/// Her ARP paketinden gönderen cihazın MAC ve IP bilgisi öğrenilir
/// (ücretsiz önbellek güncellemesi). Eğer paket bize yönelikse:
/// - İstek ise: MAC adresimizi içeren yanıt gönderilir.
/// - Yanıt ise: Önbellek zaten güncellenmiştir, sadece log basılır.
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    let arp = ArpHeader::parse(data)?;

    // Gönderici bilgisini önbelleğe kaydet (ücretsiz ARP öğrenimi)
    add_entry(arp.spa, arp.sha);

    // Bu paket bize mi yönelik?
    let local = local_ip();
    if arp.tpa == local {
        match arp.oper {
            ArpOperation::Request => {
                // Bize ARP isteği geldi: MAC adresimizle yanıt ver
                let iface = super::default_interface().ok_or(NetError::NoInterface)?;
                let mut iface = iface.lock();

                let reply = ArpHeader::new_reply(
                    iface.mac(),
                    local,
                    arp.sha,
                    arp.spa,
                );

                let mut buf = alloc::vec![0u8; ArpHeader::SIZE];
                reply.serialize(&mut buf)?;

                let mut frame_buf = alloc::vec![0u8; 1514];
                let frame = EthernetFrame::new(
                    arp.sha,
                    iface.mac(),
                    EtherType::ARP,
                    &buf,
                );
                let len = frame.serialize(&mut frame_buf)?;

                iface.send(&frame_buf[..len])?;

                crate::serial_println!("[ARP] Reply: {} is at {:?}",
                    super::socket::format_ipv4(local), iface.mac());
            }
            ArpOperation::Reply => {
                // Bize ARP yanıtı geldi: add_entry ile önbellek güncellendi
                crate::serial_println!("[ARP] Reply: {} is at {:?}",
                    super::socket::format_ipv4(arp.spa), arp.sha);
            }
            _ => {}
        }
    }

    Ok(())
}

/// ARP önbelleğini döner (get_table ile aynı işlev, alternatif isim).
pub fn get_cache() -> Vec<(Ipv4Addr, MacAddr)> {
    ARP_CACHE.lock()
        .iter()
        .map(|(&ip, &mac)| (Ipv4Addr::from_u32(ip), mac))
        .collect()
}
