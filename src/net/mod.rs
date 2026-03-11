//! # echOS Ağ Yığını
//!
//! Katman-1 (OS seviyesi) TCP/IP implementasyonu — POSIX soket API uyumlu.
//!
//! ## Ağ Yığını Katman Mimarisi
//!
//! ```text
//! +----------------------------------------------+
//! |           Uygulama Katmanı                   |
//! |   (HTTP, DNS, TLS, WebSocket, QUIC...)       |
//! +----------------------------------------------+
//! |         Taşıma Katmanı (Transport)           |
//! |        TCP (bağlantılı) | UDP (bağlantısız) |
//! +----------------------------------------------+
//! |           Ağ Katmanı (Network)               |
//! |     IPv4 / IPv6 | ICMP | ARP | DHCP         |
//! +----------------------------------------------+
//! |         Veri Bağlantı Katmanı (Data Link)    |
//! |          Ethernet | MAC Adresleme            |
//! +----------------------------------------------+
//! |          Fiziksel Katman / Sürücü            |
//! |    VirtIO-Net | Loopback (lo) | smoltcp      |
//! +----------------------------------------------+
//! ```
//!
//! ## Alt Modüller
//!
//! | Modül           | Açıklama                                      |
//! |-----------------|-----------------------------------------------|
//! | `socket`        | POSIX soket API (socket/bind/connect/send...) |
//! | `tcp`           | TCP bağlantı yönetimi, durum makinesi         |
//! | `udp`           | UDP datagram gönderme/alma                    |
//! | `ip`            | IPv4 paket işleme                             |
//! | `ipv6`          | IPv6, ICMPv6, NDP, SLAAC, DHCPv6             |
//! | `ethernet`      | Ethernet çerçeve işleme, EtherType            |
//! | `arp`           | ARP protokolü (IP→MAC çözümleme)              |
//! | `dhcp`          | DHCPv4 istemci                                |
//! | `dns`           | DNS çözümleme                                 |
//! | `dnssec`        | DNSSEC doğrulama                              |
//! | `doh`           | DNS over HTTPS                                |
//! | `dot`           | DNS over TLS                                  |
//! | `netdev`        | Ağ aygıt sürücüleri (VirtIO-Net, loopback)   |
//! | `http`          | HTTP/1.1 istemci/sunucu                       |
//! | `http2`         | HTTP/2 multiplexing                           |
//! | `websocket`     | WebSocket protokolü                           |
//! | `smoltcp_driver`| smoltcp TCP/IP kütüphanesi entegrasyonu       |
//! | `tls`           | TLS 1.2/1.3 şifreleme katmanı                |
//! | `io_uring`      | Linux io_uring uyumlu async I/O API           |
//! | `x509`          | X.509 sertifika işleme                        |
//! | `quic`          | QUIC protokolü (UDP üzeri TLS)                |
//! | `bluetooth_le_audio` | Bluetooth 5.2 LE Audio (LC3, ISO channels) |
//! | `zero_copy`     | Sıfır kopya tampon yönetimi                   |
//! | `netfilter`     | iptables/netfilter paket filtreleme           |

pub mod arp;
pub mod bluetooth_le_audio;
pub mod dhcp;
pub mod cni;
pub mod dns;
pub mod dnssec;
pub mod doh;
pub mod dot;
pub mod ebpf;
pub mod ethernet;
pub mod grpc;
pub mod http;
pub mod http2;
pub mod http3;
pub mod io_uring;
pub mod io_uring_nvme;
pub mod ip;
pub mod ipv6;
pub mod netdev;
pub mod netfilter;
pub mod quic;
pub mod smoltcp_driver;
pub mod socket;
pub mod tcp;
pub mod test_stack;
pub mod tls;
pub mod udp;
pub mod unix_socket;
pub mod websocket;
pub mod wireguard;
pub mod x509;
pub mod zero_copy;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// AĞ YAPILANDIRMASI (NETWORK CONFIGURATION)
// ============================================================================
//
// Sistem genelinde tek bir ağ yapılandırması tutulur.
// DHCP veya statik yapılandırma ile doldurulur.

/// Maksimum soket sayısı
const MAX_SOCKETS: usize = 4096;

/// Ağ tamponu boyutu (MTU + Ethernet başlığı)
const NET_BUF_SIZE: usize = 1514; // MTU + Ethernet header

/// Küresel ağ yapılandırması (Mutex ile korumalı)
static NET_CONFIG: Mutex<NetworkConfig> = Mutex::new(NetworkConfig {
    ip_addr: [0, 0, 0, 0],
    netmask: [255, 255, 255, 0],
    gateway: [0, 0, 0, 0],
    dns_servers: Vec::new(),
    hostname: String::new(),
});

/// Ağ yapılandırması — IP, ağ maskesi, ağ geçidi ve DNS
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    pub ip_addr: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns_servers: Vec<[u8; 4]>,
    pub hostname: String,
}

impl NetworkConfig {
    pub fn new() -> Self {
        Self {
            ip_addr: [0, 0, 0, 0],
            netmask: [255, 255, 255, 0],
            gateway: [0, 0, 0, 0],
            dns_servers: Vec::new(),
            hostname: String::from("echos"),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.ip_addr != [0, 0, 0, 0]
    }
}

// ============================================================================
// MAC ADRESİ (Media Access Control Address)
// ============================================================================
//
// MAC adresi, ağ arayüz kartına üretici tarafından atanan 48-bitlik
// donanım adresidir. Ethernet çerçevelerinde kaynak ve hedef tanımlamasında
// kullanılır. Format: AA:BB:CC:DD:EE:FF (6 oktet, hex)

/// MAC Adresi (6 bayt = 48 bit)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xFF; 6]);
    pub const ZERO: MacAddr = MacAddr([0x00; 6]);

    pub fn new(bytes: [u8; 6]) -> Self {
        MacAddr(bytes)
    }

    pub fn from_bytes(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8) -> Self {
        MacAddr([a, b, c, d, e, f])
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 6]
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }

    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl Default for MacAddr {
    fn default() -> Self {
        MacAddr::ZERO
    }
}

// ============================================================================
// IP ADRESİ (IPv4)
// ============================================================================
//
// IPv4 adresi 32 bittir ve genellikle dört oktet noktalı gösterim ile yazılır.
// Örnek: 192.168.1.1 (0xC0.0xA8.0x01.0x01)
//
// Sınıflar (CIDR öncesi, tarihsel):
//   A: 0.0.0.0/8     (büyük ağlar, 10.x.x.x özel)
//   B: 128.0.0.0/16  (orta ağlar, 172.16-31.x.x özel)
//   C: 192.0.0.0/24  (küçük ağlar, 192.168.x.x özel)
//   D: 224.0.0.0/4   (multicast)

/// IPv4 Adresi (4 bayt = 32 bit)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const UNSPECIFIED: Ipv4Addr = Ipv4Addr([0, 0, 0, 0]); // 0.0.0.0 belirsiz
    pub const BROADCAST: Ipv4Addr = Ipv4Addr([255, 255, 255, 255]); // 255.255.255.255 yayın
    pub const LOCALHOST: Ipv4Addr = Ipv4Addr([127, 0, 0, 1]); // 127.0.0.1 geri döngü

    pub fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Ipv4Addr([a, b, c, d])
    }

    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        Ipv4Addr(bytes)
    }

    pub fn is_unspecified(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }

    pub fn is_private(&self) -> bool {
        // RFC 1918 özel adres aralıkları:
        self.0[0] == 10 ||                                               // 10.0.0.0/8
        // 172.16.0.0/12
        (self.0[0] == 172 && self.0[1] >= 16 && self.0[1] <= 31) ||
        // 192.168.0.0/16
        (self.0[0] == 192 && self.0[1] == 168)
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == [255, 255, 255, 255]
    }

    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    pub fn to_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub fn from_u32(val: u32) -> Self {
        Ipv4Addr(val.to_be_bytes())
    }
}

impl Default for Ipv4Addr {
    fn default() -> Self {
        Ipv4Addr::UNSPECIFIED
    }
}

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

// ============================================================================
// PORT NUMARASI
// ============================================================================
//
// Port, aynı IP adresi üzerindeki farklı servisleri ayırt etmek için kullanılan
// 16-bit (0-65535) tanımlayıcıdır.
//
// Port Aralıkları:
//   0 - 1023  : Sistem portları (iyi bilinen servisler, root yetkisi gerektirir)
//   1024-49151 : Kayıtlı portlar (IANA tarafından atanmış)
//   49152-65535 : Dinamik/kısa ömürlü portlar (istemci tarafı geçici bağlantılar)

/// Ağ portu (16 bit)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port(pub u16);

impl Port {
    pub const HTTP: Port = Port(80); // Hiper Metin Transfer Protokolü
    pub const HTTPS: Port = Port(443); // HTTP Güvenli (TLS üzeri HTTP)
    pub const SSH: Port = Port(22); // Güvenli Kabuk (Secure Shell)
    pub const DNS: Port = Port(53); // Alan Adı Sistemi
    pub const DHCP_CLIENT: Port = Port(68); // DHCP İstemci portu
    pub const DHCP_SERVER: Port = Port(67); // DHCP Sunucu portu

    pub fn new(port: u16) -> Self {
        Port(port)
    }

    pub fn is_system(&self) -> bool {
        self.0 < 1024
    }

    pub fn is_dynamic(&self) -> bool {
        self.0 >= 49152
    }

    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

// ============================================================================
// SOKET ADRESİ (SOCKET ADDRESS)
// ============================================================================
//
// Bir soket addresi = IP adresi + Port numarası birleşimidir.
// TCP/UDP bağlantılarında kaynak ve hedef uç noktaları tanımlar.
// Örnek: 192.168.1.1:8080

/// Soket adresi (IP + Port)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketAddr {
    pub ip: Ipv4Addr,
    pub port: Port,
}

impl SocketAddr {
    pub fn new(ip: Ipv4Addr, port: Port) -> Self {
        SocketAddr { ip, port }
    }

    pub fn unspecified(port: Port) -> Self {
        SocketAddr {
            ip: Ipv4Addr::UNSPECIFIED,
            port,
        }
    }
}

impl Default for SocketAddr {
    fn default() -> Self {
        SocketAddr::unspecified(Port(0))
    }
}

// ============================================================================
// AĞ ARABIRIM KATMANI (NETWORK INTERFACE)
// ============================================================================
//
// Her ağ arabirimi (eth0, lo, wlan0 vb.) bu trait'i implemente eder.
// Sürücüler, bu soyutlama katmanı sayesinde protokol yığınından bağımsız hale gelir.
//
//   Protokol Yığını
//       │
//       ▼
//   NetInterface (trait) ← VirtioNetInterface
//                        ← LoopbackInterface

/// Ağ arabirimi istatistikleri (gelen/giden paket, bayt, hata sayaçları)
#[derive(Clone, Debug, Default)]
pub struct NetStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

/// Ağ arabirimi trait'i — tüm sürücüler bu arayüzü uygulamalıdır
pub trait NetInterface: Send + Sync {
    /// Arabirim adını döndürür (örn. "eth0", "lo")
    fn name(&self) -> &str;

    /// MAC adresini döndürür
    fn mac(&self) -> MacAddr;

    /// Atanmış IPv4 adresini döndürür
    fn ip(&self) -> Ipv4Addr;

    /// IPv4 adresini ayarlar
    fn set_ip(&mut self, ip: Ipv4Addr);

    /// Ağ maskesini döndürür (örn. 255.255.255.0)
    fn netmask(&self) -> Ipv4Addr;

    /// Ağ maskesini ayarlar
    fn set_netmask(&mut self, mask: Ipv4Addr);

    /// Varsayılan ağ geçidini döndürür
    fn gateway(&self) -> Option<Ipv4Addr>;

    /// Varsayılan ağ geçidini ayarlar
    fn set_gateway(&mut self, gw: Ipv4Addr);

    /// Arabirimin aktif (up) olup olmadığını döndürür
    fn is_up(&self) -> bool;

    /// Arabirimi aktif (up=true) veya pasif (up=false) yapar
    fn set_up(&mut self, up: bool);

    /// Ham Ethernet çerçevesi gönderir
    fn send(&mut self, data: &[u8]) -> Result<(), NetError>;

    /// Ham Ethernet çerçevesi alır (engellemesiz/non-blocking)
    fn recv(&mut self) -> Option<Vec<u8>>;

    /// Arabirim istatistiklerini döndürür
    fn stats(&self) -> NetStats;

    /// Maksimum İletim Birimi (MTU) — varsayılan 1500 bayt
    fn mtu(&self) -> u16 {
        1500
    }
}

/// Ağ işlemi hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetError {
    NoInterface,        // Ağ arabirimi bulunamadı
    NotUp,              // Arabirim aktif değil
    BufferFull,         // Gönderme tamponu dolu
    BufferEmpty,        // Alma tamponu boş
    InvalidPacket,      // Geçersiz paket formatı
    InvalidFd,          // Geçersiz soket tanımlayıcısı
    InvalidParam,       // Geçersiz parametre
    ChecksumError,      // Sağlama toplamı hatası
    Timeout,            // Zaman aşımı
    ConnectionRefused,  // Bağlantı reddedildi
    ConnectionReset,    // Bağlantı sıfırlandı
    ConnectionClosed,   // Bağlantı kapatıldı
    WouldBlock,         // Engellemesiz modda işlem tamamlanamadı
    AddrInUse,          // Adres/port zaten kullanımda
    AddrNotAvailable,   // Adres mevcut değil
    NetworkUnreachable, // Ağa erişilemiyor
    HostUnreachable,    // Uzak makineye erişilemiyor
    ProtocolError,      // Protokol hatası
    NotSupported,       // Desteklenmeyen işlem
    NotConnected,       // Soket bağlı değil
    Unknown,            // Bilinmeyen hata
}

// ============================================================================
// AĞ YÖNETİCİSİ (NETWORK MANAGER)
// ============================================================================
//
// Tüm ağ arabirimlerini merkezi olarak yönetir.
// Arabirim kayıt, çözümleme ve varsayılan arabirim seçimi buradan yapılır.
//
//   NET_INTERFACES : Kayıtlı arabirimler listesi (Mutex<Vec<Arc<Mutex<dyn NetInterface>>>>)
//   NET_INITIALIZED: Ağ yığınının başlatılıp başlatılmadığını belirtir (AtomicBool)
//   NEXT_SOCKET_ID : Bir sonraki benzersiz soket kimliği (AtomicU32)

/// Kayıtlı ağ arabirimlerinin küresel listesi
static NET_INTERFACES: Mutex<Vec<Arc<Mutex<dyn NetInterface>>>> = Mutex::new(Vec::new());
/// Ağ yığınının başlatılma durumu (çift başlatmayı önler)
static NET_INITIALIZED: AtomicBool = AtomicBool::new(false);
static NEXT_SOCKET_ID: AtomicU32 = AtomicU32::new(1);

/// Ağ yığınını başlatır (çift çağrı güvenlidir)
pub fn init() {
    if NET_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    crate::serial_println!("[NET] Initializing networking stack...");

    // Initialize network device drivers
    netdev::init();

    // Initialize protocols
    arp::init();
    tcp::init();
    udp::init();
    dhcp::init();
    dns::init();
    ipv6::init();
    netfilter::init();

    // High-performance datapaths
    ebpf::init();
    zero_copy::init();
    io_uring::init();
    io_uring_nvme::init();

    // Modern transport/security protocols
    http3::init();
    wireguard::init();
    grpc::init();

    crate::serial_println!("[NET] Networking stack initialized");
}

/// Yeni bir ağ arabirimini sisteme kaydeder
pub fn register_interface(iface: Arc<Mutex<dyn NetInterface>>) {
    let mut interfaces = NET_INTERFACES.lock();
    interfaces.push(iface);
    crate::serial_println!(
        "[NET] Interface registered: {}",
        interfaces.last().unwrap().lock().name()
    );
}

/// Ada göre ağ arabirimini bulur ve döndürür
pub fn get_interface(name: &str) -> Option<Arc<Mutex<dyn NetInterface>>> {
    let interfaces = NET_INTERFACES.lock();
    for iface in interfaces.iter() {
        if iface.lock().name() == name {
            return Some(iface.clone());
        }
    }
    None
}

/// Varsayılan ağ arabirimini döndürür (listede ilk kayıtlı arabirim)
pub fn default_interface() -> Option<Arc<Mutex<dyn NetInterface>>> {
    let interfaces = NET_INTERFACES.lock();
    if !interfaces.is_empty() {
        Some(interfaces[0].clone())
    } else {
        None
    }
}

/// Yeni benzersiz soket kimliği ayırır ve döndürür
pub fn allocate_socket_id() -> u32 {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

/// Ağın yapılandırılıp yapılandırılmadığını kontrol eder (IP atanmış mı?)
pub fn is_configured() -> bool {
    NET_CONFIG.lock().is_configured()
}

/// Mevcut ağ yapılandırmasının bir kopyasını döndürür
pub fn get_config() -> NetworkConfig {
    NET_CONFIG.lock().clone()
}

/// Ağ yapılandırmasını günceller (DHCP yanıtı veya statik ayar sonrası)
pub fn set_config(config: NetworkConfig) {
    let mut cfg = NET_CONFIG.lock();
    *cfg = config;
}

/// Sistemin yerel IPv4 adresini döndürür
pub fn local_ip() -> Ipv4Addr {
    Ipv4Addr::from_bytes(NET_CONFIG.lock().ip_addr)
}

/// Gelen Ethernet çerçevesini işler ve uygun protokole yönlendirir
pub fn process_packet(data: &[u8]) -> Result<(), NetError> {
    // Ethernet çerçevesini ayrıştır
    let frame = ethernet::EthernetFrame::parse(data)?;

    // EtherType'a göre yönlendir
    match frame.ether_type() {
        ethernet::EtherType::ARP => {
            arp::process_packet(&frame.payload)?;
        }
        ethernet::EtherType::IPV4 => {
            ip::process_packet(&frame.payload)?;
        }
        _ => {
            // Bilinmeyen protokol, paketi düşür
        }
    }

    Ok(())
}

/// Paketi varsayılan ağ arabirimi üzerinden gönderir
pub fn send_packet(data: &[u8]) -> Result<(), NetError> {
    let iface = default_interface().ok_or(NetError::NoInterface)?;
    iface.lock().send(data)?;
    Ok(())
}

// ============================================================================
// AĞ ARAÇLARI (ss, nc, traceroute, ping, ifconfig)
// ============================================================================
//
// Bu bölüm, kullanıcı kabuk komutlarını desteklemek için üst düzey ağ araçları
// sunar. Her araç ilgili protokol modülleri üzerine inşa edilmiştir.
//
//  Araç         Komut     Açıklama
//  --------     ------    ----------------------------------------
//  ss           ss -t     Aktif soketleri listeler (netstat yerine)
//  nc           nc        Netcat: TCP bağlantısı kur veya dinle
//  traceroute   tracert   Hedefe giden yol üzerindeki atlama noktaları
//  ping         ping      ICMP Echo Request/Reply ile gecikme ölçümü
//  arp          arp -n    ARP tablosunu görüntüler
//  ifconfig     ifconfig  Ağ arabirimi bilgilerini görüntüler

/// Soket istatistikleri girdisi (`ss` komutu için)
#[derive(Clone, Debug)]
pub struct SocketStats {
    pub id: u32,
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub rx_bytes: usize,
    pub tx_bytes: usize,
}

/// Tüm aktif soketlerin istatistiklerini döndürür (`ss` komutu)
pub fn get_socket_stats() -> Vec<SocketStats> {
    let mut stats = Vec::new();

    // TCP bağlantılarını listele
    let tcp_conns = tcp::get_all_connections();
    for conn in tcp_conns {
        let state_str = match conn.state {
            tcp::TcpState::Closed => "CLOSED",
            tcp::TcpState::Listen => "LISTEN",
            tcp::TcpState::SynSent => "SYN-SENT",
            tcp::TcpState::SynReceived => "SYN-RECV",
            tcp::TcpState::Established => "ESTAB",
            tcp::TcpState::FinWait1 => "FIN-WAIT1",
            tcp::TcpState::FinWait2 => "FIN-WAIT2",
            tcp::TcpState::CloseWait => "CLOSE-WAIT",
            tcp::TcpState::Closing => "CLOSING",
            tcp::TcpState::LastAck => "LAST-ACK",
            tcp::TcpState::TimeWait => "TIME-WAIT",
        };

        stats.push(SocketStats {
            id: conn.id,
            proto: String::from("tcp"),
            local: format!(
                "{}:{}",
                socket::format_ipv4(conn.local.ip),
                conn.local.port.0
            ),
            remote: format!(
                "{}:{}",
                socket::format_ipv4(conn.remote.ip),
                conn.remote.port.0
            ),
            state: String::from(state_str),
            rx_bytes: conn.rx_buffer.len(),
            tx_bytes: conn.tx_buffer.len(),
        });
    }

    // UDP soketlerini listele
    let udp_socks = udp::get_all_sockets();
    for sock in udp_socks {
        stats.push(SocketStats {
            id: sock.id,
            proto: String::from("udp"),
            local: format!(
                "{}:{}",
                socket::format_ipv4(sock.local.ip),
                sock.local.port.0
            ),
            remote: String::from("*:*"),
            state: String::from(" "),
            rx_bytes: sock.rx_buffer.iter().map(|(_, v)| v.len()).sum(),
            tx_bytes: 0,
        });
    }

    stats
}

/// Netcat — uzak sunucuya TCP bağlantısı kurar (`nc host port`)
pub fn nc_connect(host: &str, port: u16) -> Result<u32, NetError> {
    let dns_server = get_config()
        .dns_servers
        .first()
        .map(|ip| Ipv4Addr::from_bytes(*ip))
        .ok_or(NetError::NetworkUnreachable)?;

    let ip = dns::resolve(host, dns_server)?;
    let addr = SocketAddr::new(ip, Port(port));

    let sock = socket::socket(
        socket::AddressFamily::IPV4,
        socket::SocketType::STREAM,
        socket::Protocol::TCP,
    )?;

    socket::connect(sock, addr)?;
    Ok(sock)
}

/// Netcat — bağlı sokete veri gönderir
pub fn nc_send(sock: u32, data: &[u8]) -> Result<usize, NetError> {
    socket::send(sock, data, 0)
}

/// Netcat — soketten veri alır
pub fn nc_recv(sock: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    socket::recv(sock, buf, 0)
}

/// Netcat — dinleme modu (`nc -l port`) — belirtilen portta bağlantı bekler
pub fn nc_listen(port: u16) -> Result<u32, NetError> {
    let sock = socket::socket(
        socket::AddressFamily::IPV4,
        socket::SocketType::STREAM,
        socket::Protocol::TCP,
    )?;

    socket::bind(sock, SocketAddr::new(Ipv4Addr::UNSPECIFIED, Port(port)))?;
    socket::listen(sock, 1)?;
    Ok(sock)
}

/// Netcat — gelen bağlantıyı kabul eder
pub fn nc_accept(sock: u32) -> Result<(u32, SocketAddr), NetError> {
    socket::accept(sock)
}

/// Traceroute atlama noktası bilgisi
#[derive(Clone, Debug)]
pub struct TracerouteHop {
    pub hop: u8,
    pub ip: Ipv4Addr,
    pub rtt_ms: u32,
    pub reached: bool,
}

/// Hedefe giden yolu TTL artırarak keşfeder (Traceroute)
pub fn traceroute(dest: Ipv4Addr, max_hops: u8) -> Result<Vec<TracerouteHop>, NetError> {
    let mut hops = Vec::new();

    // Yönlendirme için ağ geçidini al
    let config = get_config();
    let gateway = Ipv4Addr::from_bytes(config.gateway);

    // Basit traceroute simülasyonu
    // Gerçek implementasyonunda artan TTL ile ICMP/UDP paketleri gönderilir
    for ttl in 1..=max_hops {
        // Hedefe ulaşıp ulaşmadığımızı kontrol et
        if ttl == 1 {
            // İlk atlama genellikle ağ geçididir (yerel router)
            if !gateway.is_unspecified() {
                hops.push(TracerouteHop {
                    hop: ttl,
                    ip: gateway,
                    rtt_ms: 1,
                    reached: false,
                });
            }
        } else if ttl == max_hops || ttl >= 16 {
            // Hedefe ulaşıldığı varsayılır (gerçek impl: ICMP Echo Reply bekle)
            hops.push(TracerouteHop {
                hop: ttl,
                ip: dest,
                rtt_ms: ttl as u32 * 10,
                reached: true,
            });
            break;
        } else {
            // Ara atlama noktası (simüle edilmiş, gerçek değil)
            // Gerçek implementasyonda ICMP Time Exceeded yanıtları ayrıştırılır
            let hop_ip = Ipv4Addr::from_bytes([gateway.0[0], gateway.0[1], gateway.0[2], ttl]);
            hops.push(TracerouteHop {
                hop: ttl,
                ip: hop_ip,
                rtt_ms: ttl as u32 * 5,
                reached: false,
            });
        }
    }

    Ok(hops)
}

/// Ping — ICMP Echo Request/Reply ile gecikme ölçer (`ping ip count`)
pub fn ping(dest: Ipv4Addr, count: u8) -> Result<Vec<(u32, bool)>, NetError> {
    let mut results = Vec::new();

    // Ping simülasyonu — gerçek implementasyonda ICMP Echo Request/Reply kullanılır
    for i in 0..count {
        // RTT simüle et (gerçekte: ICMP yanıt zamanı ölçülür)
        let rtt = 5 + (i as u32 * 2);
        let success = i < count - 1; // Paket kaybı simülasyonu

        results.push((rtt, success));
    }

    Ok(results)
}

/// ARP tablosunu döndürür (`arp -n`)
pub fn get_arp_table() -> Vec<(Ipv4Addr, MacAddr)> {
    arp::get_table()
}

/// Sistemdeki tüm ağ arabirimlerini döndürür (`ifconfig`)
pub fn get_interfaces() -> Vec<InterfaceInfo> {
    let mut interfaces = Vec::new();

    if let Some(iface) = default_interface() {
        let netdev = iface.lock();
        interfaces.push(InterfaceInfo {
            name: String::from("eth0"),
            mac: netdev.mac(),
            ip: local_ip(),
            netmask: Ipv4Addr::from_bytes(get_config().netmask),
            gateway: Ipv4Addr::from_bytes(get_config().gateway),
            mtu: 1500,
            up: true,
        });
    }

    interfaces
}

/// Ağ arabirimi bilgisi (`ifconfig` çıktısı için)
#[derive(Clone, Debug)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub mtu: usize,
    pub up: bool,
}

// Public exports
pub use arp::*;
pub use bluetooth_le_audio::*;
pub use dhcp::*;
pub use dns::*;
pub use ethernet::*;
pub use ip::*;
pub use ipv6::*;
pub use quic::*;
pub use socket::*;
pub use tcp::*;
pub use tls::*;
pub use udp::*;
