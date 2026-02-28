//! # Ethernet Katmanı (Layer 2)
//!
//! Ethernet, yerel alan ağlarında (LAN) veri iletimi için kullanılan
//! en yaygın Layer 2 (Veri Bağlantısı) protokolüdür.
//! IEEE 802.3 standardı ile tanımlanmıştır.
//!
//! ## Ağ Modelinde Ethernet'in Yeri
//!
//! ```text
//! +---------------------------+  OSI Katmanı
//! |  Uygulama (L7)            |  HTTP, DNS, SSH
//! +---------------------------+
//! |  Taşıma (L4)              |  TCP, UDP
//! +---------------------------+
//! |  Ağ (L3)                  |  IP (IPv4/IPv6)
//! +---------------------------+
//! |  Veri Bağlantısı (L2)     |  Ethernet  <-- BU MODÜL
//! +---------------------------+
//! |  Fiziksel (L1)            |  Kablo, RF sinyali
//! +---------------------------+
//! ```
//!
//! ## Ethernet Çerçeve Yapısı (IEEE 802.3)
//!
//! ```text
//! Byte:  0    1    2    3    4    5
//!       +----+----+----+----+----+----+
//!       |      Hedef MAC (6 byte)     |  DST: Paketin gönderileceği cihazın MAC
//!       +----+----+----+----+----+----+
//! Byte:  6    7    8    9   10   11
//!       +----+----+----+----+----+----+
//!       |     Kaynak MAC (6 byte)     |  SRC: Paketi gönderen cihazın MAC
//!       +----+----+----+----+----+----+
//! Byte: 12   13
//!       +----+----+
//!       | EtherType|  Yük protokolü: IPv4=0x0800, ARP=0x0806, IPv6=0x86DD
//!       +----+----+
//! Byte: 14 ....
//!       +----------------------------+
//!       |   Yük (Payload)            |  Üst katman verisi (IP paketi vb.)
//!       |   (46 - 1500 byte)         |
//!       +----------------------------+
//! [Byte: 14+n .. 14+n+3]
//!       +----+----+----+----+
//!       |     FCS (4 byte)   |  Frame Check Sequence: CRC32 hata denetimi
//!       +----+----+----+----+
//!
//! Toplam minimum frame boyutu: 64 byte (14 başlık + 46 min. yük + 4 FCS)
//! Toplam maksimum frame boyutu: 1518 byte (14 başlık + 1500 yük + 4 FCS)
//! ```
//!
//! ## MAC Adresi Türleri
//!
//! ```text
//! Unicast:   XX:XX:XX:XX:XX:XX (en düşük bit = 0)
//!            Tek bir cihaza gönderim
//!
//! Broadcast: FF:FF:FF:FF:FF:FF
//!            Ağdaki tüm cihazlara gönderim (ARP isteği, DHCP Discover vb.)
//!
//! Multicast: XX:XX:XX:XX:XX:XX (en düşük bit = 1)
//!            Belirli bir gruba gönderim (IPv6 Neighbor Discovery vb.)
//! ```
//!
//! ## EtherType Değerleri
//!
//! ```text
//! 0x0800 = IPv4   -- En yaygın: IP paketleri
//! 0x0806 = ARP    -- IP-MAC adres çözümleme
//! 0x86DD = IPv6   -- Yeni nesil IP protokolü
//! 0x8100 = VLAN   -- 802.1Q VLAN etiketleme (4 byte ek başlık)
//! ```

use super::{MacAddr, NetError};

/// Ethernet çerçeve başlığı.
///
/// 14 byte sabit boyutlu başlık: hedef MAC + kaynak MAC + EtherType.
/// Bu başlıktan sonra 46-1500 byte arası yük (payload) gelir.
#[derive(Clone, Copy, Debug)]
pub struct EthernetHeader {
    pub dst: MacAddr,            // Hedef MAC adresi (6 byte)
    pub src: MacAddr,            // Kaynak MAC adresi (6 byte)
    pub ether_type: EtherType,   // Yük protokolü tanımlayıcısı (2 byte)
}

/// Ödünç alınmış yük verisi ile birlikte Ethernet çerçevesi.
///
/// Gerçek bir Ethernet çerçevesi için başlık + yük birleştirilerek gönderilir.
/// `'a` yaşam süresi yük verisinin referansının geçerliliğini garanti eder.
#[derive(Clone, Debug)]
pub struct EthernetFrame<'a> {
    pub header: EthernetHeader,
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    /// Başlıktaki EtherType değerini döner.
    ///
    /// Üst katmanın çerçeve içeriğini nasıl yorumlayacağını belirler:
    /// - IPv4 (0x0800): IP paketi olarak işle
    /// - ARP (0x0806): ARP paketi olarak işle
    pub fn ether_type(&self) -> EtherType {
        self.header.ether_type
    }
}

/// Ethernet protokol türleri (EtherType).
///
/// Bu değer Ethernet başlığının 13. ve 14. byte'larını oluşturur.
/// Alıcı cihaz bu değere bakarak yükü hangi protokola göre işleyeceğini anlar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum EtherType {
    IPV4 = 0x0800,    // IPv4 paketi (en yaygın)
    ARP = 0x0806,     // ARP: IP adresinden MAC adresi çözümleme
    IPV6 = 0x86DD,    // IPv6 paketi
    VLAN = 0x8100,    // IEEE 802.1Q VLAN etiketleme (ek 4 byte başlık içerir)
    UNKNOWN = 0,      // Bilinmeyen veya desteklenmeyen protokol
}

impl EtherType {
    /// 16-bit sayısal değerden EtherType varyantına dönüştürür.
    ///
    /// Tanınmayan değerler için UNKNOWN döner.
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x0800 => EtherType::IPV4,
            0x0806 => EtherType::ARP,
            0x86DD => EtherType::IPV6,
            0x8100 => EtherType::VLAN,
            _ => EtherType::UNKNOWN,
        }
    }

    /// EtherType'ı ağ üzerinde iletim için 16-bit sayıya dönüştürür.
    pub fn to_u16(self) -> u16 {
        self as u16
    }
}

impl EthernetHeader {
    /// Ethernet başlığının sabit boyutu: 14 byte.
    ///
    /// ```text
    /// 6 byte (dst MAC) + 6 byte (src MAC) + 2 byte (EtherType) = 14 byte
    /// ```
    pub const SIZE: usize = 14;

    /// Ham byte dizisinden Ethernet başlığını ayrıştırır.
    ///
    /// Ethernet başlığı ağ byte sırası (big-endian) ile kodlanmıştır.
    /// Veri 14 byte'tan kısa ise InvalidPacket hatası döner.
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::SIZE {
            return Err(NetError::InvalidPacket);
        }

        // Ilk 6 byte: hedef MAC
        let dst = MacAddr::new([data[0], data[1], data[2], data[3], data[4], data[5]]);
        // Sonraki 6 byte: kaynak MAC
        let src = MacAddr::new([data[6], data[7], data[8], data[9], data[10], data[11]]);
        // Son 2 byte: EtherType (big-endian)
        let ether_type = EtherType::from_u16(u16::from_be_bytes([data[12], data[13]]));

        Ok(EthernetHeader { dst, src, ether_type })
    }

    /// Ethernet başlığını byte dizisine seri hale getirir.
    ///
    /// Hedef tampon en az 14 byte olmalıdır.
    /// Tüm değerler ağ byte sırası (big-endian) ile yazılır.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::SIZE {
            return Err(NetError::BufferFull);
        }

        // Hedef MAC'i ilk 6 byte'a yaz
        buf[0..6].copy_from_slice(self.dst.as_bytes());
        // Kaynak MAC'i 7-12. byte'lara yaz
        buf[6..12].copy_from_slice(self.src.as_bytes());
        // EtherType'ı 13-14. byte'lara big-endian olarak yaz
        buf[12..14].copy_from_slice(&self.ether_type.to_u16().to_be_bytes());

        Ok(())
    }

    /// Yeni bir Ethernet başlığı oluşturur.
    pub fn new(dst: MacAddr, src: MacAddr, ether_type: EtherType) -> Self {
        EthernetHeader { dst, src, ether_type }
    }
}

impl<'a> EthernetFrame<'a> {
    /// Gelen ham byte dizisinden Ethernet çerçevesini ayrıştırır.
    ///
    /// Başlık ayrıştırıldıktan sonra geri kalan veriler yük (payload) olarak döner.
    /// Sıfır kopyalama (zero-copy): yük, orijinal tampona referans alır.
    pub fn parse(data: &'a [u8]) -> Result<Self, NetError> {
        let header = EthernetHeader::parse(data)?;
        let payload = &data[EthernetHeader::SIZE..];

        Ok(EthernetFrame { header, payload })
    }

    /// Yeni bir Ethernet çerçevesi oluşturur.
    ///
    /// Bu fonksiyon çerçeveyi fiziksel ağa göndermez;
    /// göndermek için `serialize()` çağrılmalı ve çıktı ağ sürücüsüne verilmelidir.
    pub fn new(dst: MacAddr, src: MacAddr, ether_type: EtherType, payload: &'a [u8]) -> Self {
        EthernetFrame {
            header: EthernetHeader::new(dst, src, ether_type),
            payload,
        }
    }

    /// Çerçeveyi tam olarak byte dizisine yazar.
    ///
    /// Hedef tampona sırasıyla başlık ve yük verisi yazılır.
    /// Dönen değer toplam yazılan byte sayısıdır.
    pub fn serialize(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let total_len = EthernetHeader::SIZE + self.payload.len();
        if buf.len() < total_len {
            return Err(NetError::BufferFull);
        }

        // Başlığı yaz (14 byte)
        self.header.serialize(&mut buf[..EthernetHeader::SIZE])?;
        // Yükü başlığın hemen ardından yaz
        buf[EthernetHeader::SIZE..total_len].copy_from_slice(self.payload);

        Ok(total_len)
    }

    /// Çerçevenin toplam boyutunu döner.
    ///
    /// Başlık (14 byte) + Yük boyutu.
    /// FCS (4 byte CRC) bu boyuta dahil değildir (donanım tarafından eklenir).
    pub fn len(&self) -> usize {
        EthernetHeader::SIZE + self.payload.len()
    }
}

/// Gönderim için Ethernet çerçevesi oluşturur ve tampona yazar.
///
/// Yardımcı fonksiyon: `EthernetFrame::new()` + `serialize()` zincirine kısayol.
/// Dönen değer tampona yazılan toplam byte sayısıdır.
pub fn build_frame(
    dst: MacAddr,
    src: MacAddr,
    ether_type: EtherType,
    payload: &[u8],
    buf: &mut [u8],
) -> Result<usize, NetError> {
    let frame = EthernetFrame::new(dst, src, ether_type, payload);
    frame.serialize(buf)
}
