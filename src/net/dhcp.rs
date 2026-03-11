//! # DHCP İstemcisi (Dynamic Host Configuration Protocol)
//!
//! DHCP, bir ağa bağlanan cihazların IP adresi, alt ağ maskesi, ağ geçidi
//! ve DNS sunucusu bilgilerini otomatik olarak almasını sağlayan protokoldür.
//! RFC 2131 ile tanımlanmıştır. UDP üzerinde çalışır (Port 67/68).
//!
//! ## DORA El Sıkışma Süreci
//!
//! ```text
//! İstemci (Client)                    DHCP Sunucusu (Server)
//!      |                                       |
//!      |--- DISCOVER (broadcast UDP 255.255.255.255:67) --->|
//!      |    "Ağda DHCP sunucusu var mı?"        |
//!      |                                       |
//!      |<--- OFFER (broadcast/unicast) ---------|
//!      |    "Sana 192.168.1.100 verebilirim"   |
//!      |                                       |
//!      |--- REQUEST (broadcast) -------------->|
//!      |    "192.168.1.100'ü istiyorum"        |
//!      |                                       |
//!      |<--- ACK (broadcast/unicast) ----------|
//!      |    "Onaylandı, IP senin!"             |
//!      |                                       |
//! ```
//!
//! ## DHCP Mesaj Yapısı (Minimum 236 Byte + Options)
//!
//! ```text
//! Offset  Uzunluk  Alan
//! ------  -------  -------
//!  0       1       op      (1=istek, 2=yanıt)
//!  1       1       htype   (Donanım türü: 1=Ethernet)
//!  2       1       hlen    (MAC adresi uzunluğu: 6)
//!  3       1       hops    (Ağ geçidi sayısı)
//!  4       4       xid     (Oturum kimliği / transaction ID)
//!  8       2       secs    (Geçen süre)
//! 10       2       flags   (0x8000 = broadcast bayrağı)
//! 12       4       ciaddr  (İstemci IP'si, zaten varsa)
//! 16       4       yiaddr  (Sunucunun teklif ettiği IP "Your IP")
//! 20       4       siaddr  (Sunucu IP'si)
//! 24       4       giaddr  (Ağ geçidi IP'si)
//! 28      16       chaddr  (İstemci MAC adresi, 16 byte'a padded)
//! 44      64       sname   (Sunucu adı, opsiyonel)
//!108     128       file    (Önyükleme dosya adı, opsiyonel)
//!236     var.      options (DHCP seçenekleri, sihirli çerez ile başlar)
//!
//! Options başlangıcı:
//!   0x63 0x82 0x53 0x63  <-- DHCP Sihirli Çerez (Magic Cookie)
//!   53   1    1          <-- Option 53: Mesaj Türü = DISCOVER
//!   55   4    1,3,6,15   <-- Option 55: Parametre İstek Listesi
//!  255                   <-- End option (seçeneklerin sonu)
//! ```
//!
//! ## Kira (Lease) Zaman Çizelgesi
//!
//! ```text
//! 0         T1(~%50)     T2(~%87.5)   Kira Sonu
//! |------------|------------|------------|
//! ^            ^            ^            ^
//! ACK alındı   Yenileme     Yeniden      IP sona
//! (Kira başlar) başlar       bağlanma     erer
//!              (sunucuya    (broadcast
//!              unicast)     REQUEST)
//! ```

use super::socket::{bind, close, recvfrom, sendto, socket};
use super::udp;
use super::{Ipv4Addr, MacAddr, NetError, Port, SocketAddr};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// DHCP istemci portu: istemci bu portu dinler (RFC 2131)
const DHCP_CLIENT_PORT: u16 = 68;
/// DHCP sunucu portu: tüm istek ve yanıtlar bu porta gönderilir (RFC 2131)
const DHCP_SERVER_PORT: u16 = 67;

/// DHCP mesaj türleri (Option 53).
///
/// DORA süreci: Discover -> Offer -> Request -> Ack
/// İptal durumu: Nak (negatif onay)
/// Serbest bırakma: Release
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DhcpMessageType {
    Discover = 1, // İstemci: "Ağda DHCP sunucusu var mı?" (broadcast)
    Offer = 2,    // Sunucu: "Sana şu IP'yi verebilirim"
    Request = 3,  // İstemci: "O IP'yi istiyorum / kira yeniliyorum"
    Decline = 4,  // İstemci: "Teklif ettiğin IP kullanımda, reddediyorum"
    Ack = 5,      // Sunucu: "Onaylandı, IP senin"
    Nak = 6,      // Sunucu: "Reddedildi, yeniden dene"
    Release = 7,  // İstemci: "IP'yi geri veriyorum"
    Inform = 8,   // İstemci: "IP'm var ama diğer konfigürasyona ihtiyacım var"
    Unknown = 0,  // Bilinmeyen tür
}

impl DhcpMessageType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => DhcpMessageType::Discover,
            2 => DhcpMessageType::Offer,
            3 => DhcpMessageType::Request,
            4 => DhcpMessageType::Decline,
            5 => DhcpMessageType::Ack,
            6 => DhcpMessageType::Nak,
            7 => DhcpMessageType::Release,
            8 => DhcpMessageType::Inform,
            _ => DhcpMessageType::Unknown,
        }
    }
}

/// DHCP mesaj yapısı.
///
/// 236 byte sabit başlık + değişken uzunluklu options alanından oluşur.
/// `options` alanı DHCP sihirli çerezini (0x63825363) ve TLV kodlanmış
/// seçenekleri içerir.
#[derive(Clone, Debug)]
pub struct DhcpMessage {
    pub op: u8,           // 1=istemci isteği (BOOTREQUEST), 2=sunucu yanıtı (BOOTREPLY)
    pub htype: u8,        // Donanım türü (1=Ethernet)
    pub hlen: u8,         // Donanım adres uzunluğu (6 = MAC adresi)
    pub hops: u8,         // DHCP relay agent'ların sayısı (tipik 0)
    pub xid: u32,         // Transaction ID: istek/yanıtı eşleştirmek için rastgele sayı
    pub secs: u16,        // IP almaya çalışmaya başladıktan bu yana geçen saniye
    pub flags: u16,       // 0x8000 = Broadcast bayrağı (sunucunun broadcast ile yanıt vermesi)
    pub ciaddr: Ipv4Addr, // Client IP: istemcinin mevcut IP adresi (yenileme için)
    pub yiaddr: Ipv4Addr, // Your IP: sunucunun istemciye teklif ettiği IP
    pub siaddr: Ipv4Addr, // Server IP: bir sonraki önyükleme aşamasının sunucusu
    pub giaddr: Ipv4Addr, // Gateway IP: DHCP relay agent'ın adresi
    pub chaddr: [u8; 16], // Client Hardware Address: istemci MAC adresi (16 byte, padded)
    pub sname: [u8; 64],  // Server Name: opsiyonel sunucu host adı (null terminated)
    pub file: [u8; 128],  // Boot File: opsiyonel önyükleme dosya adı
    pub options: Vec<u8>, // DHCP Seçenekleri: sihirli çerez + TLV kodlanmış seçenekler
}

impl DhcpMessage {
    /// Sabit başlık boyutu (sihirli çerez dahil değil)
    pub const MIN_SIZE: usize = 236;
    /// DHCP sihirli çerezi: her DHCP mesajının options alanı bu 4 byte ile başlar
    pub const MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

    /// Yeni bir DHCP Discover mesajı oluşturur.
    ///
    /// İstemci ağa ilk bağlandığında IP almak için broadcast olarak gönderilir.
    /// Ağdaki tüm DHCP sunucuları bu mesajı alır ve Offer ile yanıt verir.
    pub fn new_discover(mac: MacAddr, xid: u32) -> Self {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(mac.as_bytes());

        let mut options = Vec::new();
        // DHCP sihirli çerezi ile başla (RFC 2131 zorunlu)
        options.extend_from_slice(&Self::MAGIC_COOKIE);
        // Option 53: Mesaj Türü = DISCOVER
        options.push(53); // Option: Message Type
        options.push(1); // Length
        options.push(DhcpMessageType::Discover as u8);
        // Option 61: İstemci Tanımlayıcısı (MAC adresi üzerinden)
        options.push(61); // Option: Client Identifier
        options.push(7); // Length
        options.push(1); // Hardware type: Ethernet
        options.extend_from_slice(mac.as_bytes());
        // Option 55: Parametre İstek Listesi (sunucudan istenen bilgiler)
        options.push(55); // Option: Parameter Request List
        options.push(4); // Length
        options.push(1); // Subnet Mask (alt ağ maskesi)
        options.push(3); // Router (ağ geçidi)
        options.push(6); // DNS Server (isim sunucusu)
        options.push(15); // Domain Name (alan adı)
                          // Option 255: Seçeneklerin sonu
        options.push(255);

        DhcpMessage {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000, // Broadcast bayrağı: sunucu broadcast ile yanıt versin
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            options,
        }
    }

    /// Yeni bir DHCP Request mesajı oluşturur.
    ///
    /// Offer alındıktan sonra gönderilir. İstenen IP ve sunucu IP'si belirtilir.
    /// Broadcast olarak gönderilir ki diğer DHCP sunucuları da tekliflerini geri çeksin.
    pub fn new_request(
        mac: MacAddr,
        xid: u32,
        requested_ip: Ipv4Addr,
        server_ip: Ipv4Addr,
    ) -> Self {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(mac.as_bytes());

        let mut options = Vec::new();
        options.extend_from_slice(&Self::MAGIC_COOKIE);
        // DHCP Mesaj Türü: REQUEST
        options.push(53);
        options.push(1);
        options.push(DhcpMessageType::Request as u8);
        // Option 50: Requested IP Address (istenen IP adresi)
        options.push(50);
        options.push(4);
        options.extend_from_slice(requested_ip.as_bytes());
        // Option 54: Server Identifier (hangi DHCP sunucusundan istendiği)
        options.push(54);
        options.push(4);
        options.extend_from_slice(server_ip.as_bytes());
        // Seçeneklerin sonu
        options.push(255);

        DhcpMessage {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0x8000,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            sname: [0u8; 64],
            file: [0u8; 128],
            options,
        }
    }

    /// Ham byte dizisinden DHCP mesajını ayrıştırır.
    ///
    /// Minimum 236 byte gerektirir. Options alanı sihirli çerezden sonra başlar.
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }

        let op = data[0];
        let htype = data[1];
        let hlen = data[2];
        let hops = data[3];
        let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let secs = u16::from_be_bytes([data[8], data[9]]);
        let flags = u16::from_be_bytes([data[10], data[11]]);
        let ciaddr = Ipv4Addr::from_bytes([data[12], data[13], data[14], data[15]]);
        let yiaddr = Ipv4Addr::from_bytes([data[16], data[17], data[18], data[19]]);
        let siaddr = Ipv4Addr::from_bytes([data[20], data[21], data[22], data[23]]);
        let giaddr = Ipv4Addr::from_bytes([data[24], data[25], data[26], data[27]]);

        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&data[28..44]);

        let mut sname = [0u8; 64];
        sname.copy_from_slice(&data[44..108]);

        let mut file = [0u8; 128];
        file.copy_from_slice(&data[108..236]);

        let options = data[236..].to_vec();

        Ok(DhcpMessage {
            op,
            htype,
            hlen,
            hops,
            xid,
            secs,
            flags,
            ciaddr,
            yiaddr,
            siaddr,
            giaddr,
            chaddr,
            sname,
            file,
            options,
        })
    }

    /// DHCP mesajını byte dizisine seri hale getirir.
    ///
    /// Ağ üzerinden göndermeden önce çağrılır.
    /// Tüm alanlar ağ byte sırası (big-endian) ile yazılır.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::MIN_SIZE + self.options.len());

        buf.push(self.op);
        buf.push(self.htype);
        buf.push(self.hlen);
        buf.push(self.hops);
        buf.extend_from_slice(&self.xid.to_be_bytes());
        buf.extend_from_slice(&self.secs.to_be_bytes());
        buf.extend_from_slice(&self.flags.to_be_bytes());
        buf.extend_from_slice(self.ciaddr.as_bytes());
        buf.extend_from_slice(self.yiaddr.as_bytes());
        buf.extend_from_slice(self.siaddr.as_bytes());
        buf.extend_from_slice(self.giaddr.as_bytes());
        buf.extend_from_slice(&self.chaddr);
        buf.extend_from_slice(&self.sname);
        buf.extend_from_slice(&self.file);
        buf.extend(&self.options);

        buf
    }

    /// Options alanından mesaj türünü (Option 53) okur.
    ///
    /// Options alanı TLV (Type-Length-Value) formatında kodlanmıştır:
    /// - Type (1 byte): Seçenek numarası
    /// - Length (1 byte): Değer uzunluğu
    /// - Value (Length byte): Değer
    pub fn get_message_type(&self) -> DhcpMessageType {
        for i in 0..self.options.len() {
            if self.options[i] == 53 && i + 2 < self.options.len() {
                return DhcpMessageType::from_u8(self.options[i + 2]);
            }
        }
        DhcpMessageType::Unknown
    }

    /// Options alanından belirtilen kod numarasına sahip seçeneği döner.
    ///
    /// TLV formatını okuyarak sihirli çerezi (ilk 4 byte) atlayıp
    /// istenen seçeneğin değerini döner. Seçenek bulunamazsa `None` döner.
    pub fn get_option(&self, code: u8) -> Option<&[u8]> {
        let mut i = 4; // Sihirli çerezi atla (Magic Cookie, 4 byte)
        while i < self.options.len() {
            let opt_code = self.options[i];
            if opt_code == 255 {
                break; // End option: seçeneklerin sonu
            }
            if opt_code == 0 {
                i += 1; // Pad option: tek byte dolgu, atla
                continue;
            }
            if i + 1 >= self.options.len() {
                break;
            }
            let opt_len = self.options[i + 1] as usize;
            if opt_code == code {
                return Some(&self.options[i + 2..i + 2 + opt_len]);
            }
            i += 2 + opt_len; // Sonraki seçeneğe geç
        }
        None
    }
}

// ============================================================================
// DHCP CLIENT (DHCP İSTEMCİSİ)
// ============================================================================
//
// DHCP istemci durumu:
//
//   INIT --> SELECTING --> REQUESTING --> BOUND
//                                          |
//                            <--RENEWING---+
//                           |              |
//                           +--REBINDING-->+
//
// BOUND: Geçerli kira var, normal iletişim
// RENEWING: T1 geçti, sunucudan unicast ile yenileme isteniyor
// REBINDING: T2 geçti, herhangi bir sunucudan broadcast ile yenileme
// INIT: Kira bitti veya NAK alındı, yeniden baştan

static DHCP_SOCKET: Mutex<Option<u32>> = Mutex::new(None);
static DHCP_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// DHCP kira durumu.
///
/// DHCP sunucusundan alınan IP adresi ve ağ konfigürasyon bilgilerini
/// ile kira süre bilgilerini tutar.
#[derive(Clone, Debug)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,               // Atanan IP adresi
    pub subnet_mask: Ipv4Addr,      // Alt ağ maskesi (Option 1)
    pub gateway: Ipv4Addr,          // Varsayılan ağ geçidi (Option 3)
    pub dns_servers: Vec<Ipv4Addr>, // DNS sunucuları (Option 6)
    pub server_ip: Ipv4Addr,        // Kira veren DHCP sunucusunun IP'si (Option 54)
    pub lease_time: u32,            // Toplam kira süresi (Option 51, saniye cinsinden)
    pub renewal_time: u32,          // T1: Yenileme zamanı, tipik olarak kira_süresi * 0.5
    pub rebinding_time: u32,        // T2: Yeniden bağlanma zamanı, tipik olarak kira_süresi * 0.875
    pub obtained_at: u64,           // Kiranın alındığı zaman damgası (sistem saati)
    pub xid: u32,                   // Yenileme işlemleri için orijinal transaction ID
}

impl DhcpLease {
    pub fn new() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            subnet_mask: Ipv4Addr::from_bytes([255, 255, 255, 0]),
            gateway: Ipv4Addr::UNSPECIFIED,
            dns_servers: Vec::new(),
            server_ip: Ipv4Addr::UNSPECIFIED,
            lease_time: 0,
            renewal_time: 0,
            rebinding_time: 0,
            obtained_at: 0,
            xid: 0,
        }
    }

    /// Kiranın geçerli olup olmadığını kontrol eder.
    ///
    /// Geçerli kira: Atanmış bir IP adresi olmalı ve kira süresi > 0.
    pub fn is_valid(&self) -> bool {
        !self.ip.is_unspecified() && self.lease_time > 0
    }

    /// Kiranın yenilenmesi gerekip gerekmediğini kontrol eder (T1 zamanı).
    ///
    /// T1 geçtiyse sunucuya doğrudan unicast Request göndererek yenileme yapılır.
    pub fn needs_renewal(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return false;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.renewal_time as u64
    }

    /// Yeniden bağlanma gerekip gerekmediğini kontrol eder (T2 zamanı).
    ///
    /// T2 geçtiyse herhangi bir DHCP sunucusuna broadcast Request gönderilir.
    pub fn needs_rebinding(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return false;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.rebinding_time as u64
    }

    /// Kiranın süresi dolup dolmadığını kontrol eder.
    ///
    /// Süresi dolmuş kira: IP artık kullanılamaz, INIT'ten yeniden başlanmalı.
    pub fn is_expired(&self, current_time: u64) -> bool {
        if !self.is_valid() {
            return true;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        elapsed >= self.lease_time as u64
    }

    /// Kiranın kalan süresini saniye olarak döner.
    pub fn remaining_time(&self, current_time: u64) -> u32 {
        if !self.is_valid() {
            return 0;
        }
        let elapsed = current_time.saturating_sub(self.obtained_at);
        self.lease_time.saturating_sub(elapsed as u32)
    }
}

impl Default for DhcpLease {
    fn default() -> Self {
        Self::new()
    }
}

/// Global DHCP kira durumu: en son geçerli kira bilgisi saklanır.
static DHCP_LEASE: Mutex<Option<DhcpLease>> = Mutex::new(None);

/// DHCP istemcisini başlatır.
pub fn init() {
    crate::serial_println!("[DHCP] Client initialized");
}

/// Mevcut DHCP kirasını döner.
pub fn get_lease() -> Option<DhcpLease> {
    DHCP_LEASE.lock().clone()
}

/// DHCP Release mesajı oluşturur.
///
/// İstemci IP adresini sunucuya geri verirken gönderilir.
/// RFC 2131: Release mesajı unicast olarak sunucuya gönderilir (broadcast değil).
pub fn new_release(
    mac: MacAddr,
    xid: u32,
    client_ip: Ipv4Addr,
    server_ip: Ipv4Addr,
) -> DhcpMessage {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac.as_bytes());

    let mut options = Vec::new();
    options.extend_from_slice(&DhcpMessage::MAGIC_COOKIE);
    // DHCP Mesaj Türü: RELEASE
    options.push(53);
    options.push(1);
    options.push(DhcpMessageType::Release as u8);
    // Option 54: Hangi sunucuya release gönderildiği
    options.push(54);
    options.push(4);
    options.extend_from_slice(server_ip.as_bytes());
    // Seçeneklerin sonu
    options.push(255);

    DhcpMessage {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0,          // Release unicast'tır, broadcast bayrağı gereksiz
        ciaddr: client_ip, // Release'de mevcut IP adresi ciaddr'a konur
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: server_ip,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        options,
    }
}

/// DHCP Renew isteği oluşturur.
///
/// T1 süresinden sonra kirayı yenilemek için kullanılır.
/// Mevcut IP'nin ciaddr'a konması bu mesajı Discover'dan ayırır.
/// Unicast ile doğrudan kira veren sunucuya gönderilir.
pub fn new_renew(mac: MacAddr, xid: u32, client_ip: Ipv4Addr, server_ip: Ipv4Addr) -> DhcpMessage {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac.as_bytes());

    let mut options = Vec::new();
    options.extend_from_slice(&DhcpMessage::MAGIC_COOKIE);
    // DHCP Mesaj Türü: REQUEST (yenileme için de Request kullanılır)
    options.push(53);
    options.push(1);
    options.push(DhcpMessageType::Request as u8);
    // Option 61: İstemci tanımlayıcısı
    options.push(61);
    options.push(7);
    options.push(1);
    options.extend_from_slice(mac.as_bytes());
    // Option 54: Unicast yenileme için sunucu tanımlayıcısı
    options.push(54);
    options.push(4);
    options.extend_from_slice(server_ip.as_bytes());
    // Seçeneklerin sonu
    options.push(255);

    DhcpMessage {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0,          // Yenileme için broadcast bayrağı yok (unicast)
        ciaddr: client_ip, // Yenilenecek IP adresi
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        options,
    }
}

/// DHCP Discover sürecini başlatır.
///
/// Rastgele bir transaction ID üretir, UDP socketi açar ve
/// broadcast adrese DHCP Discover mesajı gönderir.
pub fn discover() -> Result<(), NetError> {
    let mac = super::default_interface()
        .ok_or(NetError::NoInterface)?
        .lock()
        .mac();

    // UDP soketi oluştur
    let sock_id = socket(
        super::socket::AddressFamily::IPV4,
        super::socket::SocketType::DGRAM,
        super::socket::Protocol::UDP,
    )?;

    // DHCP istemci portuna bağlan (68)
    bind(
        sock_id,
        SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(DHCP_CLIENT_PORT)),
    )?;

    *DHCP_SOCKET.lock() = Some(sock_id);

    // Discover mesajı oluştur (rastgele XID ile)
    let xid = crate::random::rand_u64() as u32;
    let discover = DhcpMessage::new_discover(mac, xid);
    let data = discover.serialize();

    // Broadcast adresine DHCP sunucu portuna gönder
    let dst = SocketAddr::new(Ipv4Addr::BROADCAST, Port(DHCP_SERVER_PORT));
    sendto(sock_id, &data, dst, 0)?;

    crate::serial_println!("[DHCP] Discover sent (xid={:#x})", xid);

    Ok(())
}

/// DHCP sunucusundan gelen yanıtı işler.
///
/// Offer gelirse Request ile devam eder (DORA'nın 3. adımı).
/// Ack gelirse ağ konfigürasyonu tamamlanır ve sonuç döner.
/// Nak gelirse hata döner.
pub fn process_response() -> Result<super::NetworkConfig, NetError> {
    let sock_id = DHCP_SOCKET.lock().ok_or(NetError::ProtocolError)?;

    let mut buf = vec![0u8; 1500];
    let (len, _src) = recvfrom(sock_id, &mut buf, 0)?;

    let msg = DhcpMessage::parse(&buf[..len])?;

    match msg.get_message_type() {
        DhcpMessageType::Offer => {
            crate::serial_println!(
                "[DHCP] Offer received: {}",
                super::socket::format_ipv4(msg.yiaddr)
            );

            // Offer aldık: Request göndererek IP'yi talep et
            let mac = super::default_interface()
                .ok_or(NetError::NoInterface)?
                .lock()
                .mac();

            let request = DhcpMessage::new_request(mac, msg.xid, msg.yiaddr, msg.siaddr);
            let data = request.serialize();

            let dst = SocketAddr::new(Ipv4Addr::BROADCAST, Port(DHCP_SERVER_PORT));
            sendto(sock_id, &data, dst, 0)?;

            crate::serial_println!(
                "[DHCP] Request sent for {}",
                super::socket::format_ipv4(msg.yiaddr)
            );

            Err(NetError::WouldBlock) // ACK bekleniyor
        }
        DhcpMessageType::Ack => {
            crate::serial_println!("[DHCP] ACK received!");

            let mut config = super::NetworkConfig::new();
            config.ip_addr = *msg.yiaddr.as_bytes();

            // Option 1: Alt ağ maskesi
            if let Some(mask) = msg.get_option(1) {
                config.netmask = [mask[0], mask[1], mask[2], mask[3]];
            }

            // Option 3: Varsayılan ağ geçidi
            if let Some(gw) = msg.get_option(3) {
                config.gateway = [gw[0], gw[1], gw[2], gw[3]];
            }

            // Option 6: DNS sunucuları (her biri 4 byte, ardışık)
            if let Some(dns) = msg.get_option(6) {
                for i in (0..dns.len()).step_by(4) {
                    if i + 4 <= dns.len() {
                        config
                            .dns_servers
                            .push([dns[i], dns[i + 1], dns[i + 2], dns[i + 3]]);
                    }
                }
            }

            // ── Kira süresi seçeneklerini ayrıştır ──
            // Option 51: Kira süresi (lease time, 4 bayt big-endian u32 saniye)
            let lease_time: u32 = msg
                .get_option(51)
                .filter(|v| v.len() >= 4)
                .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
                .unwrap_or(86400); // varsayılan 24 saat

            // Option 58: T1 Yenileme süresi (renewal time), yoksa lease_time / 2
            let renewal_time: u32 = msg
                .get_option(58)
                .filter(|v| v.len() >= 4)
                .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
                .unwrap_or(lease_time / 2);

            // Option 59: T2 Yeniden bağlanma süresi (rebinding time), yoksa lease_time * 7 / 8
            let rebinding_time: u32 = msg
                .get_option(59)
                .filter(|v| v.len() >= 4)
                .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
                .unwrap_or(lease_time * 7 / 8);

            crate::serial_println!(
                "[DHCP] Lease: {}s, T1(renew): {}s, T2(rebind): {}s",
                lease_time,
                renewal_time,
                rebinding_time
            );

            // Sunucu IP'si (Option 54)
            let server_ip = msg
                .get_option(54)
                .filter(|v| v.len() >= 4)
                .map(|v| Ipv4Addr::from_bytes([v[0], v[1], v[2], v[3]]))
                .unwrap_or(msg.siaddr);

            // Global kira durumunu güncelle
            {
                let now = crate::interrupts::get_ticks();
                let mut lease_guard = DHCP_LEASE.lock();
                *lease_guard = Some(DhcpLease {
                    ip: msg.yiaddr,
                    subnet_mask: msg
                        .get_option(1)
                        .filter(|v| v.len() >= 4)
                        .map(|v| Ipv4Addr::from_bytes([v[0], v[1], v[2], v[3]]))
                        .unwrap_or(Ipv4Addr::from_bytes([255, 255, 255, 0])),
                    gateway: msg
                        .get_option(3)
                        .filter(|v| v.len() >= 4)
                        .map(|v| Ipv4Addr::from_bytes([v[0], v[1], v[2], v[3]]))
                        .unwrap_or(Ipv4Addr::UNSPECIFIED),
                    dns_servers: {
                        let mut dns_list = Vec::new();
                        if let Some(dns) = msg.get_option(6) {
                            for i in (0..dns.len()).step_by(4) {
                                if i + 4 <= dns.len() {
                                    dns_list.push(Ipv4Addr::from_bytes([
                                        dns[i],
                                        dns[i + 1],
                                        dns[i + 2],
                                        dns[i + 3],
                                    ]));
                                }
                            }
                        }
                        dns_list
                    },
                    server_ip,
                    lease_time,
                    renewal_time,
                    rebinding_time,
                    obtained_at: now,
                    xid: msg.xid,
                });
            }

            DHCP_CONFIGURED.store(true, Ordering::SeqCst);

            // Soketi kapat, DHCP tamamlandı
            close(sock_id)?;
            *DHCP_SOCKET.lock() = None;

            Ok(config)
        }
        DhcpMessageType::Nak => {
            crate::serial_println!("[DHCP] NAK received");
            Err(NetError::ProtocolError) // Sunucu reddetti, yeniden dene
        }
        _ => Err(NetError::WouldBlock), // Beklenmedik mesaj türü
    }
}

/// DHCP yapılandırmasının tamamlanıp tamamlanmadığını kontrol eder.
pub fn is_configured() -> bool {
    DHCP_CONFIGURED.load(Ordering::SeqCst)
}

/// DHCP Rebind isteği oluşturur.
///
/// T2 süresinden sonra kira yenilemek için broadcast olarak gönderilir.
/// `new_renew()`'den farkı: broadcast bayrağı ayarlanır ve sunucu tanımlayıcısı eklenmez.
/// ciaddr = mevcut IP adresi (RFC 2131 §4.3.6).
pub fn new_rebind(mac: MacAddr, xid: u32, client_ip: Ipv4Addr) -> DhcpMessage {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(mac.as_bytes());

    let mut options = Vec::new();
    options.extend_from_slice(&DhcpMessage::MAGIC_COOKIE);
    // DHCP Mesaj Türü: REQUEST (rebind için de Request kullanılır)
    options.push(53);
    options.push(1);
    options.push(DhcpMessageType::Request as u8);
    // Option 61: İstemci tanımlayıcısı
    options.push(61);
    options.push(7);
    options.push(1);
    options.extend_from_slice(mac.as_bytes());
    // NOT: Rebind'de Option 54 (server identifier) EKLENmez — herhangi bir sunucu yanıt verebilir
    // Seçeneklerin sonu
    options.push(255);

    DhcpMessage {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid,
        secs: 0,
        flags: 0x8000,     // Broadcast bayrağı (rebind = broadcast)
        ciaddr: client_ip, // Yenilenecek IP adresi
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        sname: [0u8; 64],
        file: [0u8; 128],
        options,
    }
}

/// DHCP kira zamanlama kontrolünü yapar.
///
/// Periyodik olarak çağrılmalıdır (zamanlama görevinden veya ana döngüden).
///
/// Kontrol sırası:
///   1. Kira süresi dolduysa (`is_expired`) → arayüzü devre dışı işaretle
///   2. T2 geçtiyse (`needs_rebinding`) → broadcast Rebind gönder
///   3. T1 geçtiyse (`needs_renewal`)  → unicast Renew gönder
pub fn dhcp_timer_check() {
    let now = crate::interrupts::get_ticks();

    let lease_opt = {
        let guard = DHCP_LEASE.lock();
        guard.clone()
    };

    let lease = match lease_opt {
        Some(l) if l.is_valid() => l,
        _ => return, // Geçerli kira yok, yapacak bir şey yok
    };

    // 1) Kira süresi doldu mu?
    if lease.is_expired(now) {
        crate::serial_println!("[DHCP] Lease EXPIRED! Marking interface down.");
        DHCP_CONFIGURED.store(false, Ordering::SeqCst);
        // Kirayı geçersiz kıl
        *DHCP_LEASE.lock() = None;
        return;
    }

    // MAC adresini al
    let mac = match super::default_interface() {
        Some(iface) => iface.lock().mac(),
        None => return,
    };

    // 2) T2 geçti mi? → Rebind (broadcast)
    if lease.needs_rebinding(now) {
        crate::serial_println!("[DHCP] T2 expired — sending REBIND (broadcast)");
        let rebind = new_rebind(mac, lease.xid, lease.ip);
        let data = rebind.serialize();
        let dst = SocketAddr::new(Ipv4Addr::BROADCAST, Port(DHCP_SERVER_PORT));
        // Geçici soket oluştur ve gönder
        if let Ok(sock) = socket(
            super::socket::AddressFamily::IPV4,
            super::socket::SocketType::DGRAM,
            super::socket::Protocol::UDP,
        ) {
            let _ = bind(
                sock,
                SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(DHCP_CLIENT_PORT)),
            );
            let _ = sendto(sock, &data, dst, 0);
            let _ = close(sock);
        }
        return;
    }

    // 3) T1 geçti mi? → Renew (unicast)
    if lease.needs_renewal(now) {
        crate::serial_println!(
            "[DHCP] T1 expired — sending RENEW (unicast to {})",
            super::socket::format_ipv4(lease.server_ip)
        );
        let renew = new_renew(mac, lease.xid, lease.ip, lease.server_ip);
        let data = renew.serialize();
        let dst = SocketAddr::new(lease.server_ip, Port(DHCP_SERVER_PORT));
        if let Ok(sock) = socket(
            super::socket::AddressFamily::IPV4,
            super::socket::SocketType::DGRAM,
            super::socket::Protocol::UDP,
        ) {
            let _ = bind(
                sock,
                SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(DHCP_CLIENT_PORT)),
            );
            let _ = sendto(sock, &data, dst, 0);
            let _ = close(sock);
        }
    }
}
