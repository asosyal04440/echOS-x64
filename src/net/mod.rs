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
//! | `napi`          | NAPI polling framework (interrupt coalescing)  |
//! | `gso`           | TSO/GSO/UFO segmentation offload              |
//! | `gro`           | GRO receive offload (paket birleştirme)       |
//! | `checksum`      | Checksum offload (HW/SW)                      |
//! | `routing`       | Policy routing + FIB trie (LPM)               |
//! | `tc`            | Traffic control / qdisc (pfifo_fast, fq_codel)|

pub mod arp;
pub mod bluetooth_le_audio;
pub mod bond;
pub mod bridge;
pub mod cgroup_bw;
pub mod checksum;
pub mod cni;
pub mod devlink_genl;
pub mod dhcp;
pub mod dns;
pub mod dnssec;
pub mod doh;
pub mod dot;
pub mod dscp;
pub mod ebpf;
pub mod ebpf_maps;
pub mod tc_bpf;
pub mod ecmp;
pub mod ethernet;
pub mod ethtool;
pub mod ethtool_genl;
pub mod gre;
pub mod geneve;
pub mod gro;
pub mod grpc;
pub mod gso;
pub mod htb;
pub mod hw_timestamping;
pub mod http;
pub mod http2;
pub mod http3;
pub mod http_sys;
pub mod igmp;
pub mod io_uring;
pub mod ip;
pub mod ipsec;
pub mod ipv6;
pub mod ipv6_transition;
pub mod ipvlan;
pub mod lro;
pub mod macvlan;
pub mod mld;
pub mod mmsg;
pub mod mptcp;
pub mod pim;
pub mod napi;
pub mod net_device;
pub mod netdev;
pub mod netfilter;
pub mod nf_conntrack;
pub mod nftables;
pub mod netns;
pub mod quic;
pub mod routing;
pub mod routing_protocols;
pub mod rps_rfs;
pub mod rss;
pub mod sfq_cake;
pub mod sendfile;
pub mod smoltcp_driver;
pub mod sock_diag;
pub mod netlink;
pub mod socket;
pub mod tcp;
pub mod tc;
pub mod tcp_cork;
pub mod tcp_info;
pub mod tcp_metrics_genl;
pub mod wireguard_genl;
pub mod mptcp_pm_genl;
pub mod net_shaper_genl;
pub mod ovpn_genl;
mod wireless;
pub mod handshake_genl;
pub mod nl80211_genl;
pub mod test_stack;
pub mod tls;
pub mod tun_tap;
pub mod udp;
pub mod unix_socket;
pub mod veth;
pub mod vlan;
pub mod vrf;
pub mod vxlan;
pub mod websocket;
pub mod wireguard;
pub mod x509;
pub mod zero_copy;
pub mod af_packet;
pub mod snmp_agent;
pub mod sctp;
pub mod smb;
pub mod port_knock;

use af_packet::deliver_frame;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IpAddr {
    V4(Ipv4Addr),
    V6(ipv6::Ipv6Addr),
}

impl IpAddr {
    pub fn is_unspecified(&self) -> bool {
        match self {
            IpAddr::V4(ip) => ip.is_unspecified(),
            IpAddr::V6(ip) => ip.is_unspecified(),
        }
    }

    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        match self {
            IpAddr::V4(ip) => Some(*ip),
            IpAddr::V6(ip) => ip.to_ipv4_mapped(),
        }
    }

    pub fn as_ipv6(&self) -> Option<ipv6::Ipv6Addr> {
        match self {
            IpAddr::V4(ip) => Some(ipv6::Ipv6Addr::from_ipv4_mapped(*ip)),
            IpAddr::V6(ip) => Some(*ip),
        }
    }

    pub fn family(&self) -> crate::net::socket::AddressFamily {
        match self {
            IpAddr::V4(_) => crate::net::socket::AddressFamily::IPV4,
            IpAddr::V6(_) => crate::net::socket::AddressFamily::IPV6,
        }
    }
}

impl Default for IpAddr {
    fn default() -> Self {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    }
}

impl From<Ipv4Addr> for IpAddr {
    fn from(value: Ipv4Addr) -> Self {
        IpAddr::V4(value)
    }
}

impl From<ipv6::Ipv6Addr> for IpAddr {
    fn from(value: ipv6::Ipv6Addr) -> Self {
        IpAddr::V6(value)
    }
}

impl core::fmt::Display for IpAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IpAddr::V4(ip) => write!(f, "{}", ip),
            IpAddr::V6(ip) => write!(f, "{}", ip.to_string()),
        }
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
    pub ip: IpAddr,
    pub port: Port,
}

impl SocketAddr {
    pub fn new<T: Into<IpAddr>>(ip: T, port: Port) -> Self {
        SocketAddr {
            ip: ip.into(),
            port,
        }
    }

    pub fn unspecified(port: Port) -> Self {
        SocketAddr {
            ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port,
        }
    }

    pub fn unspecified_v6(port: Port) -> Self {
        SocketAddr {
            ip: IpAddr::V6(ipv6::Ipv6Addr::UNSPECIFIED),
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

// ============================================================================
// GENİŞLETİLMİŞ NIC İSTATİSTİKLERİ (Per-NIC Extended Stats)
// ============================================================================
//
// /proc/net/dev benzeri ayrıntılı istatistikler.
// Her NIC için rx/tx tarafında detaylı hata ve başarı sayaçları.

/// Genişletilmiş NIC istatistikleri (/proc/net/dev parity)
#[derive(Clone, Debug, Default)]
pub struct ExtendedNicStats {
    // --- RX tarafı ---
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    pub rx_fifo_errors: u64,
    pub rx_frame_errors: u64,
    pub rx_compressed: u64,
    pub rx_multicast: u64,
    pub rx_crc_errors: u64,
    pub rx_length_errors: u64,
    pub rx_over_errors: u64,
    pub rx_missed_errors: u64,
    // --- TX tarafı ---
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
    pub tx_fifo_errors: u64,
    pub tx_carrier_errors: u64,
    pub tx_compressed: u64,
    pub tx_aborted_errors: u64,
    pub tx_window_errors: u64,
    pub tx_heartbeat_errors: u64,
    // --- Offload sayaçları ---
    pub tx_gso_segments: u64,
    pub tx_gso_bytes: u64,
    pub rx_gro_merged: u64,
    pub rx_gro_bytes: u64,
    pub tx_checksum_offloaded: u64,
    pub rx_checksum_offloaded: u64,
    // --- NAPI ---
    pub napi_polls: u64,
    pub napi_budget_exhausted: u64,
}

impl ExtendedNicStats {
    pub fn from_basic(stats: &NetStats) -> Self {
        ExtendedNicStats {
            rx_packets: stats.rx_packets,
            rx_bytes: stats.rx_bytes,
            rx_errors: stats.rx_errors,
            rx_dropped: stats.rx_dropped,
            tx_packets: stats.tx_packets,
            tx_bytes: stats.tx_bytes,
            tx_errors: stats.tx_errors,
            tx_dropped: stats.tx_dropped,
            ..Default::default()
        }
    }
}

// ============================================================================
// GLOBAL AĞ HATA SAYAÇLARI (SNMP-benzeri Network Error Counters)
// ============================================================================
//
// /proc/net/snmp benzeri global hata sayaçları.
// Tüm protokollerden gelen hatalar burada toplanır.

/// SNMP-benzeri global IP hata sayaçları
pub struct IpErrorCounters {
    pub in_receives: AtomicU64,
    pub in_hdr_errors: AtomicU64,
    pub in_addr_errors: AtomicU64,
    pub in_discards: AtomicU64,
    pub in_delivers: AtomicU64,
    pub out_requests: AtomicU64,
    pub out_discards: AtomicU64,
    pub out_no_routes: AtomicU64,
    pub in_unknown_protos: AtomicU64,
    pub reasm_timeout: AtomicU64,
    pub reasm_reqds: AtomicU64,
    pub reasm_oks: AtomicU64,
    pub reasm_fails: AtomicU64,
    pub frag_oks: AtomicU64,
    pub frag_fails: AtomicU64,
    pub frag_creates: AtomicU64,
}

impl IpErrorCounters {
    pub const fn new() -> Self {
        IpErrorCounters {
            in_receives: AtomicU64::new(0),
            in_hdr_errors: AtomicU64::new(0),
            in_addr_errors: AtomicU64::new(0),
            in_discards: AtomicU64::new(0),
            in_delivers: AtomicU64::new(0),
            out_requests: AtomicU64::new(0),
            out_discards: AtomicU64::new(0),
            out_no_routes: AtomicU64::new(0),
            in_unknown_protos: AtomicU64::new(0),
            reasm_timeout: AtomicU64::new(0),
            reasm_reqds: AtomicU64::new(0),
            reasm_oks: AtomicU64::new(0),
            reasm_fails: AtomicU64::new(0),
            frag_oks: AtomicU64::new(0),
            frag_fails: AtomicU64::new(0),
            frag_creates: AtomicU64::new(0),
        }
    }
}

/// SNMP-benzeri global TCP hata sayaçları
pub struct TcpErrorCounters {
    pub active_opens: AtomicU64,
    pub passive_opens: AtomicU64,
    pub in_segs: AtomicU64,
    pub out_segs: AtomicU64,
    pub retrans_segs: AtomicU64,
    pub in_errs: AtomicU64,
    pub out_rsts: AtomicU64,
    pub attempt_fails: AtomicU64,
    pub estab_resets: AtomicU64,
    pub curr_estab: AtomicU64,
    pub syn_cookies_sent: AtomicU64,
    pub syn_cookies_recv: AtomicU64,
    pub syn_cookies_failed: AtomicU64,
}

impl TcpErrorCounters {
    pub const fn new() -> Self {
        TcpErrorCounters {
            active_opens: AtomicU64::new(0),
            passive_opens: AtomicU64::new(0),
            in_segs: AtomicU64::new(0),
            out_segs: AtomicU64::new(0),
            retrans_segs: AtomicU64::new(0),
            in_errs: AtomicU64::new(0),
            out_rsts: AtomicU64::new(0),
            attempt_fails: AtomicU64::new(0),
            estab_resets: AtomicU64::new(0),
            curr_estab: AtomicU64::new(0),
            syn_cookies_sent: AtomicU64::new(0),
            syn_cookies_recv: AtomicU64::new(0),
            syn_cookies_failed: AtomicU64::new(0),
        }
    }
}

/// Multicast (IGMP/MLD) sayaçları
pub struct MulticastCounters {
    pub igmp_queries: AtomicU64,
    pub igmp_reports_sent: AtomicU64,
    pub igmp_leaves_sent: AtomicU64,
    pub igmp_joins: AtomicU64,
    pub igmp_leaves: AtomicU64,
    pub mld_queries: AtomicU64,
    pub mld_reports_sent: AtomicU64,
    pub mld_dones_sent: AtomicU64,
    pub mld_joins: AtomicU64,
    pub mld_leaves: AtomicU64,
    pub multicast_packets_in: AtomicU64,
    pub multicast_packets_out: AtomicU64,
    pub multicast_bytes_in: AtomicU64,
    pub multicast_bytes_out: AtomicU64,
}

impl MulticastCounters {
    pub const fn new() -> Self {
        MulticastCounters {
            igmp_queries: AtomicU64::new(0),
            igmp_reports_sent: AtomicU64::new(0),
            igmp_leaves_sent: AtomicU64::new(0),
            igmp_joins: AtomicU64::new(0),
            igmp_leaves: AtomicU64::new(0),
            mld_queries: AtomicU64::new(0),
            mld_reports_sent: AtomicU64::new(0),
            mld_dones_sent: AtomicU64::new(0),
            mld_joins: AtomicU64::new(0),
            mld_leaves: AtomicU64::new(0),
            multicast_packets_in: AtomicU64::new(0),
            multicast_packets_out: AtomicU64::new(0),
            multicast_bytes_in: AtomicU64::new(0),
            multicast_bytes_out: AtomicU64::new(0),
        }
    }
}

/// SNMP-benzeri global ICMP hata sayaçları
pub struct IcmpErrorCounters {
    pub in_msgs: AtomicU64,
    pub in_errors: AtomicU64,
    pub out_msgs: AtomicU64,
    pub out_errors: AtomicU64,
}

impl IcmpErrorCounters {
    pub const fn new() -> Self {
        IcmpErrorCounters {
            in_msgs: AtomicU64::new(0),
            in_errors: AtomicU64::new(0),
            out_msgs: AtomicU64::new(0),
            out_errors: AtomicU64::new(0),
        }
    }
}

/// SNMP-benzeri global UDP hata sayaçları
pub struct UdpErrorCounters {
    pub in_datagrams: AtomicU64,
    pub out_datagrams: AtomicU64,
    pub in_errors: AtomicU64,
    pub no_ports: AtomicU64,
    pub rcv_buf_errors: AtomicU64,
    pub snd_buf_errors: AtomicU64,
}

impl UdpErrorCounters {
    pub const fn new() -> Self {
        UdpErrorCounters {
            in_datagrams: AtomicU64::new(0),
            out_datagrams: AtomicU64::new(0),
            in_errors: AtomicU64::new(0),
            no_ports: AtomicU64::new(0),
            rcv_buf_errors: AtomicU64::new(0),
            snd_buf_errors: AtomicU64::new(0),
        }
    }
}

/// Global ağ hata sayaçları topluluğu
pub struct GlobalNetCounters {
    pub ip: IpErrorCounters,
    pub tcp: TcpErrorCounters,
    pub udp: UdpErrorCounters,
    pub icmp: IcmpErrorCounters,
    pub multicast: MulticastCounters,
}

impl GlobalNetCounters {
    pub const fn new() -> Self {
        GlobalNetCounters {
            ip: IpErrorCounters::new(),
            tcp: TcpErrorCounters::new(),
            udp: UdpErrorCounters::new(),
            icmp: IcmpErrorCounters::new(),
            multicast: MulticastCounters::new(),
        }
    }
}

/// Küresel ağ hata sayaçları
pub static NET_COUNTERS: GlobalNetCounters = GlobalNetCounters::new();

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
    InvalidArg,         // Geçersiz argüman
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
    smoltcp_driver::init();
    ipv6::init();
    netfilter::init();

    // High-performance datapaths
    napi::init();
    gso::init();
    gro::init();
    checksum::init();
    ebpf::init();
    zero_copy::init();
    io_uring::init();

    // NetDevice abstraction
    net_device::init();

    // Routing and traffic control
    routing::init();
    tc::init();

    // Modern transport/security protocols
    http3::init();
    wireguard::init();
    grpc::init();
    let _ = smoltcp_driver::bootstrap_runtime_config();

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
    #[cfg(any(test, target_os = "windows"))]
    {
        for iface in interfaces.iter() {
            let guard = iface.lock();
            if guard.name() == "lo" && guard.is_up() {
                drop(guard);
                return Some(iface.clone());
            }
        }
    }

    for iface in interfaces.iter() {
        let guard = iface.lock();
        if guard.name() != "lo" && guard.is_up() {
            drop(guard);
            return Some(iface.clone());
        }
    }

    if let Some(loopback) = interfaces.iter().find(|iface| iface.lock().name() == "lo") {
        return Some(loopback.clone());
    }

    interfaces.first().cloned()
}

/// Yeni benzersiz soket kimliği ayırır ve döndürür
pub fn allocate_socket_id() -> u32 {
    NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn ensure_loopback_interface_for_tests() {
    if get_interface("lo").is_some() {
        return;
    }

    register_interface(Arc::new(Mutex::new(netdev::LoopbackInterface::new())));
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
    match ebpf::filter_ingress_packet(data) {
        Ok(true) => {
            deliver_frame(data);
        }
        Ok(false) => {
            crate::serial_println!("[NET] eBPF ingress filter dropped frame ({}B)", data.len());
            return Ok(());
        }
        Err(err) => {
            crate::serial_println!("[NET] eBPF ingress filter error: {:?}", err);
            return Err(NetError::ProtocolError);
        }
    }

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
        ethernet::EtherType::IPV6 => {
            ipv6::process_packet(&frame.payload)?;
        }
        _ => {
            // Bilinmeyen protokol, paketi düşür
        }
    }

    Ok(())
}

/// Paketi varsayılan ağ arabirimi üzerinden gönderir
pub fn send_packet(data: &[u8]) -> Result<(), NetError> {
    let iface = match default_interface() {
        Some(i) => i,
        None => {
            if ip::Ipv4Packet::parse(data).is_ok() {
                NET_COUNTERS.ip.out_no_routes.fetch_add(1, Ordering::Relaxed);
            }
            return Err(NetError::NoInterface);
        }
    };
    let mut tx_buf = data.to_vec();
    let iface_name = {
        let guard = iface.lock();
        String::from(guard.name())
    };
    if ip::Ipv4Packet::parse(&tx_buf).is_ok() {
        NET_COUNTERS.ip.out_requests.fetch_add(1, Ordering::Relaxed);
        let local_out = netfilter::process_ipv4_packet(
            &mut tx_buf,
            netfilter::NF_INET_LOCAL_OUT,
            None,
            Some(iface_name.as_str()),
        )?;
        if local_out == netfilter::NF_DROP {
            NET_COUNTERS.ip.out_discards.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        let post_routing = netfilter::process_ipv4_packet(
            &mut tx_buf,
            netfilter::NF_INET_POST_ROUTING,
            None,
            Some(iface_name.as_str()),
        )?;
        if post_routing == netfilter::NF_DROP {
            NET_COUNTERS.ip.out_discards.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    } else if ipv6::Ipv6Packet::parse(&tx_buf).is_ok() {
        let local_out = netfilter::process_ipv6_packet(
            &mut tx_buf,
            netfilter::NF_INET_LOCAL_OUT,
            None,
            Some(iface_name.as_str()),
        )?;
        if local_out == netfilter::NF_DROP {
            return Ok(());
        }
        let post_routing = netfilter::process_ipv6_packet(
            &mut tx_buf,
            netfilter::NF_INET_POST_ROUTING,
            None,
            Some(iface_name.as_str()),
        )?;
        if post_routing == netfilter::NF_DROP {
            return Ok(());
        }
    }
    iface.lock().send(&tx_buf)?;
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
                socket::format_ipaddr(conn.local.ip),
                conn.local.port.0
            ),
            remote: format!(
                "{}:{}",
                socket::format_ipaddr(conn.remote.ip),
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
                socket::format_ipaddr(sock.local.ip),
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

    // Traceroute simülasyonu: artan TTL ile ICMP/UDP paketleri gönderilir.
    // Gerçek ICMP/UDP probing + timeout yönetimi ileride eklenecek.
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
    ping_real(dest, count)
}

/// Gercek ICMP Echo Request/Reply yolu ile ping dener.
///
/// Bu yol, varsayilan aktif arabirimden paket alimi yaparak Echo Reply
/// paketlerini bekler. Timeout durumunda `success=false` ile dÃ¶ner.
pub fn ping_real(dest: Ipv4Addr, count: u8) -> Result<Vec<(u32, bool)>, NetError> {
    let mut results = Vec::new();
    let identifier = (crate::interrupts::get_ticks() as u16) ^ 0xEC10;
    let timeout_ticks = 250u64;

    for i in 0..count {
        let sequence = i as u16;
        ip::send_icmp_echo_request(dest, identifier, sequence, b"echOS-ping")?;

        let deadline = crate::interrupts::get_ticks().saturating_add(timeout_ticks);
        let mut delivered = false;

        loop {
            if let Some(rtt) = ip::take_icmp_echo_reply(dest, identifier, sequence) {
                results.push((rtt, true));
                delivered = true;
                break;
            }

            if let Some(iface) = default_interface() {
                if let Some(frame) = iface.lock().recv() {
                    let _ = process_packet(&frame);
                    continue;
                }
            }

            if crate::interrupts::get_ticks() >= deadline {
                break;
            }

            core::hint::spin_loop();
        }

        if !delivered {
            ip::cancel_icmp_echo_request(dest, identifier, sequence);
            results.push((timeout_ticks as u32, false));
        }
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

/// Tüm arabirimlerin genişletilmiş istatistiklerini döndürür (`/proc/net/dev` benzeri)
pub fn get_all_extended_stats() -> Vec<(String, ExtendedNicStats)> {
    let interfaces = NET_INTERFACES.lock();
    let mut result = Vec::new();
    for iface in interfaces.iter() {
        let guard = iface.lock();
        let name = String::from(guard.name());
        let basic = guard.stats();
        let ext = ExtendedNicStats::from_basic(&basic);
        result.push((name, ext));
    }
    result
}

/// Belirli bir arabirimin genişletilmiş istatistiklerini döndürür
pub fn get_extended_stats(iface_name: &str) -> Option<ExtendedNicStats> {
    let iface = get_interface(iface_name)?;
    let guard = iface.lock();
    let basic = guard.stats();
    Some(ExtendedNicStats::from_basic(&basic))
}

/// Global ağ hata sayaçlarını okunabilir formatta döndürür (`/proc/net/snmp` benzeri)
pub fn get_snmp_counters() -> SnmpSnapshot {
    let ord = Ordering::Relaxed;
    SnmpSnapshot {
        ip_in_receives: NET_COUNTERS.ip.in_receives.load(ord),
        ip_in_hdr_errors: NET_COUNTERS.ip.in_hdr_errors.load(ord),
        ip_in_addr_errors: NET_COUNTERS.ip.in_addr_errors.load(ord),
        ip_in_discards: NET_COUNTERS.ip.in_discards.load(ord),
        ip_in_delivers: NET_COUNTERS.ip.in_delivers.load(ord),
        ip_in_unknown_protos: NET_COUNTERS.ip.in_unknown_protos.load(ord),
        ip_out_requests: NET_COUNTERS.ip.out_requests.load(ord),
        ip_out_discards: NET_COUNTERS.ip.out_discards.load(ord),
        ip_out_no_routes: NET_COUNTERS.ip.out_no_routes.load(ord),
        tcp_active_opens: NET_COUNTERS.tcp.active_opens.load(ord),
        tcp_passive_opens: NET_COUNTERS.tcp.passive_opens.load(ord),
        tcp_attempt_fails: NET_COUNTERS.tcp.attempt_fails.load(ord),
        tcp_estab_resets: NET_COUNTERS.tcp.estab_resets.load(ord),
        tcp_curr_estab: NET_COUNTERS.tcp.curr_estab.load(ord),
        tcp_in_segs: NET_COUNTERS.tcp.in_segs.load(ord),
        tcp_out_segs: NET_COUNTERS.tcp.out_segs.load(ord),
        tcp_retrans_segs: NET_COUNTERS.tcp.retrans_segs.load(ord),
        tcp_in_errs: NET_COUNTERS.tcp.in_errs.load(ord),
        tcp_out_rsts: NET_COUNTERS.tcp.out_rsts.load(ord),
        tcp_syn_cookies_sent: NET_COUNTERS.tcp.syn_cookies_sent.load(ord),
        tcp_syn_cookies_recv: NET_COUNTERS.tcp.syn_cookies_recv.load(ord),
        tcp_syn_cookies_failed: NET_COUNTERS.tcp.syn_cookies_failed.load(ord),
        udp_in_datagrams: NET_COUNTERS.udp.in_datagrams.load(ord),
        udp_out_datagrams: NET_COUNTERS.udp.out_datagrams.load(ord),
        udp_in_errors: NET_COUNTERS.udp.in_errors.load(ord),
        udp_no_ports: NET_COUNTERS.udp.no_ports.load(ord),
        udp_rcv_buf_errors: NET_COUNTERS.udp.rcv_buf_errors.load(ord),
        udp_snd_buf_errors: NET_COUNTERS.udp.snd_buf_errors.load(ord),
        icmp_in_msgs: NET_COUNTERS.icmp.in_msgs.load(ord),
        icmp_in_errors: NET_COUNTERS.icmp.in_errors.load(ord),
        icmp_out_msgs: NET_COUNTERS.icmp.out_msgs.load(ord),
        icmp_out_errors: NET_COUNTERS.icmp.out_errors.load(ord),
    }
}

/// Anlık SNMP sayacı görüntüsü (read-only snapshot)
#[derive(Clone, Debug, Default)]
pub struct SnmpSnapshot {
    pub ip_in_receives: u64,
    pub ip_in_hdr_errors: u64,
    pub ip_in_addr_errors: u64,
    pub ip_in_discards: u64,
    pub ip_in_delivers: u64,
    pub ip_in_unknown_protos: u64,
    pub ip_out_requests: u64,
    pub ip_out_discards: u64,
    pub ip_out_no_routes: u64,
    pub tcp_active_opens: u64,
    pub tcp_passive_opens: u64,
    pub tcp_attempt_fails: u64,
    pub tcp_estab_resets: u64,
    pub tcp_curr_estab: u64,
    pub tcp_in_segs: u64,
    pub tcp_out_segs: u64,
    pub tcp_retrans_segs: u64,
    pub tcp_in_errs: u64,
    pub tcp_out_rsts: u64,
    pub tcp_syn_cookies_sent: u64,
    pub tcp_syn_cookies_recv: u64,
    pub tcp_syn_cookies_failed: u64,
    pub udp_in_datagrams: u64,
    pub udp_out_datagrams: u64,
    pub udp_in_errors: u64,
    pub udp_no_ports: u64,
    pub udp_rcv_buf_errors: u64,
    pub udp_snd_buf_errors: u64,
    pub icmp_in_msgs: u64,
    pub icmp_in_errors: u64,
    pub icmp_out_msgs: u64,
    pub icmp_out_errors: u64,
}

/// `/proc/net/snmp` formatında IP/ICMP/TCP/UDP sayaç satırları üretir.
pub fn format_proc_net_snmp(snap: &SnmpSnapshot) -> Vec<u8> {
    let mut out = Vec::new();
    let ip_line = format!(
        "Ip: {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
        snap.ip_in_receives, snap.ip_in_hdr_errors, snap.ip_in_addr_errors,
        snap.ip_in_discards, 0u64, 0u64, 0u64, 0u64,
        snap.ip_in_unknown_protos, snap.ip_in_delivers,
        snap.ip_out_requests, snap.ip_out_discards, snap.ip_out_no_routes,
        0u64,
    );
    out.extend_from_slice(ip_line.as_bytes());
    let icmp_line = format!(
        "Icmp: {} {} {} {}\n",
        snap.icmp_in_msgs, snap.icmp_in_errors,
        snap.icmp_out_msgs, snap.icmp_out_errors,
    );
    out.extend_from_slice(icmp_line.as_bytes());
    let tcp_line = format!(
        "Tcp: {} {} {} {} {} {} {} {} {} {}\n",
        snap.tcp_active_opens, snap.tcp_passive_opens,
        snap.tcp_attempt_fails, snap.tcp_estab_resets,
        snap.tcp_curr_estab,
        snap.tcp_in_segs, snap.tcp_out_segs,
        snap.tcp_retrans_segs, snap.tcp_in_errs,
        snap.tcp_out_rsts,
    );
    out.extend_from_slice(tcp_line.as_bytes());
    let udp_line = format!(
        "Udp: {} {} {} {} {} {}\n",
        snap.udp_in_datagrams, snap.udp_no_ports,
        snap.udp_in_errors, snap.udp_out_datagrams,
        snap.udp_rcv_buf_errors, snap.udp_snd_buf_errors,
    );
    out.extend_from_slice(udp_line.as_bytes());
    out
}

/// `/proc/net/tcp` satırına karşılık gelen özet TCP girdisi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcNetTcpEntry {
    pub sl: usize,
    pub local_address_hex: String,
    pub rem_address_hex: String,
    pub state_hex: String,
    pub tx_queue_hex: String,
    pub rx_queue_hex: String,
    pub uid: u32,
    pub inode: u32,
}

/// `/proc/net/sockstat` özet görünümü.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SockStatSnapshot {
    pub sockets_used: usize,
    pub tcp_inuse: usize,
    pub tcp_timewait: usize,
    pub tcp_listen: usize,
    pub udp_inuse: usize,
    pub raw_inuse: usize,
    pub frag_inuse: usize,
}

fn format_ipv4_proc_hex(ip: Ipv4Addr, port: Port) -> String {
    let o = ip.0;
    format!(
        "{:02X}{:02X}{:02X}{:02X}:{:04X}",
        o[3], o[2], o[1], o[0], port.0
    )
}

fn tcp_state_proc_hex(state: tcp::TcpState) -> &'static str {
    match state {
        tcp::TcpState::Established => "01",
        tcp::TcpState::SynSent => "02",
        tcp::TcpState::SynReceived => "03",
        tcp::TcpState::FinWait1 => "04",
        tcp::TcpState::FinWait2 => "05",
        tcp::TcpState::TimeWait => "06",
        tcp::TcpState::Closed => "07",
        tcp::TcpState::CloseWait => "08",
        tcp::TcpState::LastAck => "09",
        tcp::TcpState::Listen => "0A",
        tcp::TcpState::Closing => "0B",
    }
}

/// Etkin TCP bağlantılarını `/proc/net/tcp` benzeri satırlara dönüştürür.
pub fn get_proc_net_tcp_entries() -> Vec<ProcNetTcpEntry> {
    let mut entries = Vec::new();
    let tcp_conns = tcp::get_all_connections();

    for (sl, conn) in tcp_conns.into_iter().enumerate() {
        let (local_ip, remote_ip) = match (conn.local.ip, conn.remote.ip) {
            (IpAddr::V4(local), IpAddr::V4(remote)) => (local, remote),
            _ => continue,
        };

        entries.push(ProcNetTcpEntry {
            sl,
            local_address_hex: format_ipv4_proc_hex(local_ip, conn.local.port),
            rem_address_hex: format_ipv4_proc_hex(remote_ip, conn.remote.port),
            state_hex: String::from(tcp_state_proc_hex(conn.state)),
            tx_queue_hex: format!("{:08X}", conn.tx_buffer.len()),
            rx_queue_hex: format!("{:08X}", conn.rx_buffer.len()),
            uid: 0,
            inode: conn.id,
        });
    }

    entries
}

/// `/proc/net/tcp` benzeri metinsel görünüm üretir.
pub fn render_proc_net_tcp() -> String {
    let mut out = String::from(
        "  sl  local_address rem_address   st tx_queue rx_queue uid inode\n",
    );

    for entry in get_proc_net_tcp_entries() {
        out.push_str(&format!(
            "{:4}: {} {} {} {} {} {:>3} {}\n",
            entry.sl,
            entry.local_address_hex,
            entry.rem_address_hex,
            entry.state_hex,
            entry.tx_queue_hex,
            entry.rx_queue_hex,
            entry.uid,
            entry.inode
        ));
    }

    out
}

/// Etkin soketleri `/proc/net/sockstat` benzeri özet sayaçlara indirger.
pub fn get_sockstat_snapshot() -> SockStatSnapshot {
    let tcp_conns = tcp::get_all_connections();
    let udp_socks = udp::get_all_sockets();
    let raw_inuse = socket::raw_socket_count();

    let mut snapshot = SockStatSnapshot {
        sockets_used: tcp_conns.len() + udp_socks.len() + raw_inuse,
        tcp_inuse: tcp_conns
            .iter()
            .filter(|conn| conn.state != tcp::TcpState::Closed && conn.state != tcp::TcpState::Listen)
            .count(),
        tcp_timewait: tcp_conns
            .iter()
            .filter(|conn| conn.state == tcp::TcpState::TimeWait)
            .count(),
        tcp_listen: tcp_conns
            .iter()
            .filter(|conn| conn.state == tcp::TcpState::Listen)
            .count(),
        udp_inuse: udp_socks.len(),
        raw_inuse,
        frag_inuse: 0,
    };

    if snapshot.sockets_used < snapshot.tcp_inuse + snapshot.udp_inuse + snapshot.raw_inuse {
        snapshot.sockets_used = snapshot.tcp_inuse + snapshot.udp_inuse + snapshot.raw_inuse;
    }

    snapshot
}

/// `/proc/net/sockstat` benzeri metinsel görünüm üretir.
pub fn render_sockstat() -> String {
    let snapshot = get_sockstat_snapshot();
    format!(
        "sockets: used {}\nTCP: inuse {} tw {} alloc {} mem 0\nUDP: inuse {} mem 0\nRAW: inuse {}\nFRAG: inuse {} memory 0\n",
        snapshot.sockets_used,
        snapshot.tcp_inuse,
        snapshot.tcp_timewait,
        snapshot.tcp_inuse + snapshot.tcp_listen,
        snapshot.udp_inuse,
        snapshot.raw_inuse,
        snapshot.frag_inuse
    )
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
pub use ipsec::*;
pub use ipv6::*;
pub use quic::*;
pub use socket::*;
pub use tcp::*;
pub use tls::*;
pub use udp::*;
