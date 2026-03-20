//! # echOS için smoltcp Entegrasyonu
//!
//! Bu modül, [smoltcp](https://github.com/smoltcp-rs/smoltcp) TCP/IP yığınını
//! echOS'a bağlamak için köprü görevi görür.
//!
//! ## smoltcp Nedir?
//!
//! smoltcp, no_std (standart kütüphane olmadan) çalışabilen, tamamen Rust ile
//! yazılmış gömülü TCP/IP yığınıdır. Geleneksel çekirdek ağ yığınlarının aksine
//! kullanıcı alanında çalışabilir ve hiçbir işletim sistemi bağımlılığı yoktur.
//!
//! ## Mimari
//!
//! ```text
//!  Uygulama (HTTP, DNS, TCP soketi)
//!       │
//!  smoltcp soket API'si
//!       │
//!  [Bu modül — smoltcp_driver]  ← köprü katmanı
//!       │
//!  VirtIO-Net sürücüsü (virtio_ffi)
//!       │
//!  QEMU sanal Ethernet
//! ```
//!
//! ## Mevcut Durum
//!
//! Bu modül artık eski smoltcp uyumluluk yüzeyi ile gerçek echOS ağ
//! modülleri arasında köprü görevi görür. Doğrudan `smoltcp::Device`
//! entegrasyonu yapılmış değildir; DHCP, DNS, TCP ve HTTP çağrıları
//! çekirdekteki gerçek netdev/socket/http yollarına yönlendirilir.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const SLIRP_FALLBACK_IP: [u8; 4] = [10, 0, 2, 15];
const SLIRP_FALLBACK_NETMASK: [u8; 4] = [255, 255, 255, 0];
const SLIRP_FALLBACK_GATEWAY: [u8; 4] = [10, 0, 2, 2];
const SLIRP_FALLBACK_DNS: [u8; 4] = [10, 0, 2, 3];

/// Ağ arabiriminin çalışma zamanı durumunu tutan yapı.
///
/// Tüm alanlar `Option<[u8; N]>` türündedir çünkü yapılandırma
/// önyükleme sırasında gerçekleşir; başlangıçta hiçbiri bilinmez.
///
/// - `ip`      : IPv4 adresi (4 bayt, örn. [10, 0, 2, 15])
/// - `gateway` : Varsayılan ağ geçidi (dış ağa çıkış için)
/// - `dns`     : DNS sunucu adresi (alan adı çözümlemesi için)
/// - `mac`     : Ethernet MAC adresi (6 bayt, ARP için gerekli)
pub struct NetInterface {
    pub ip: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
    pub mac: Option<[u8; 6]>,
}

impl NetInterface {
    /// Tüm alanları `None` (yapılandırılmamış) olarak başlatan boş arabirim oluşturur.
    pub fn new() -> Self {
        Self {
            ip: None,
            gateway: None,
            dns: None,
            mac: None,
        }
    }
}

// Global ağ durumu — `lazy_static!` sayesinde çalışma zamanında tek seferlik başlatılır.
// `Mutex<NetInterface>` ile çoklu çekirdek güvenli erişim sağlanır.
use lazy_static::lazy_static;

lazy_static! {
    static ref NET_INTERFACE: Mutex<NetInterface> = Mutex::new(NetInterface::new());
}

/// Global ağ arabirimi referansını döndürür.
///
/// Dönen `&'static Mutex<NetInterface>` sayesinde herhangi bir modül
/// kilidi alarak yapılandırmayı okuyabilir veya değiştirebilir.
pub fn get_interface() -> &'static Mutex<NetInterface> {
    &NET_INTERFACE
}

/// smoltcp arabirimini başlatır.
///
/// Önyükleme sırasında `net::init()` tarafından çağrılır.
/// VirtIO-Net sürücüsü hazırsa MAC adresini alabilir.
/// Gerçek smoltcp entegrasyonunda bu adımda `smoltcp::iface::Interface`
/// nesnesi oluşturulur ve RX/TX tamponları tahsis edilir.
pub fn init() -> bool {
    crate::serial_println!("[smoltcp] Interface initialized");

    // VirtIO-Net'den MAC adresini yansıt
    if crate::drivers::virtio_net::is_initialized() {
        let mac = crate::drivers::virtio_net::get_mac();
        let mut iface = NET_INTERFACE.lock();
        iface.mac = Some(*mac.as_bytes());
        crate::serial_println!(
            "[smoltcp] VirtIO-Net available: mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac.as_bytes()[0],
            mac.as_bytes()[1],
            mac.as_bytes()[2],
            mac.as_bytes()[3],
            mac.as_bytes()[4],
            mac.as_bytes()[5]
        );
    }

    sync_from_kernel_config();

    true
}

fn runtime_config_complete(config: &crate::net::NetworkConfig) -> bool {
    config.ip_addr != [0, 0, 0, 0]
        && config.gateway != [0, 0, 0, 0]
        && !config.dns_servers.is_empty()
}

fn build_slirp_fallback_config() -> crate::net::NetworkConfig {
    let mut config = crate::net::get_config();
    config.ip_addr = SLIRP_FALLBACK_IP;
    config.netmask = SLIRP_FALLBACK_NETMASK;
    config.gateway = SLIRP_FALLBACK_GATEWAY;
    if config.dns_servers.is_empty() {
        config.dns_servers.push(SLIRP_FALLBACK_DNS);
    }
    if config.hostname.is_empty() {
        config.hostname = String::from("echos");
    }
    config
}

fn apply_slirp_fallback() -> bool {
    let ip = crate::net::Ipv4Addr::from_bytes(SLIRP_FALLBACK_IP);
    let netmask = crate::net::Ipv4Addr::from_bytes(SLIRP_FALLBACK_NETMASK);
    let gateway = crate::net::Ipv4Addr::from_bytes(SLIRP_FALLBACK_GATEWAY);
    if crate::net::netdev::configure_static("eth0", ip, netmask, Some(gateway)).is_err() {
        sync_from_kernel_config();
        return false;
    }

    crate::net::set_config(build_slirp_fallback_config());
    sync_from_kernel_config();
    runtime_config_complete(&crate::net::get_config())
}

pub fn bootstrap_runtime_config() -> bool {
    sync_from_kernel_config();
    if runtime_config_complete(&crate::net::get_config()) {
        return true;
    }

    if dhcp_configure() && runtime_config_complete(&crate::net::get_config()) {
        return true;
    }

    if crate::net::get_interface("eth0").is_some() {
        crate::serial_println!(
            "[smoltcp] DHCP incomplete; applying slirp fallback ip=10.0.2.15 gw=10.0.2.2 dns=10.0.2.3"
        );
        return apply_slirp_fallback();
    }

    false
}

pub fn ensure_runtime_network() -> bool {
    crate::net::init();
    bootstrap_runtime_config()
}

/// Ağı DHCP ile yapılandırır.
///
/// Gerçek implementasyonda smoltcp'nin `dhcpv4::Dhcpv4Client` yapısı kullanılır.
/// Şu an QEMU kullanıcı modu ağı (user-mode networking) için bilinen
/// sabit varsayılan adresler (RFC önerisine göre 10.0.2.0/24) atanır.
///
/// QEMU user-mode ağ adresleri:
///   - Konuk IP  : 10.0.2.15
///   - Ağ geçidi : 10.0.2.2  (QEMU dahili slirp/NAT)
///   - DNS       : 10.0.2.3  (QEMU dahili DNS proxy)
pub fn dhcp_configure() -> bool {
    crate::serial_println!("[smoltcp] DHCP configuration started");

    match crate::net::netdev::configure_dhcp("eth0") {
        Ok(config) => {
            let mut iface = NET_INTERFACE.lock();
            iface.ip = Some(config.ip_addr);
            iface.gateway = if config.gateway != [0, 0, 0, 0] {
                Some(config.gateway)
            } else {
                None
            };
            iface.dns = config.dns_servers.first().copied();
            if crate::drivers::virtio_net::is_initialized() {
                iface.mac = Some(*crate::drivers::virtio_net::get_mac().as_bytes());
            }
            crate::serial_println!(
                "[smoltcp] DHCP configured: ip={}.{}.{}.{} gw={}.{}.{}.{} dns={}",
                config.ip_addr[0],
                config.ip_addr[1],
                config.ip_addr[2],
                config.ip_addr[3],
                config.gateway[0],
                config.gateway[1],
                config.gateway[2],
                config.gateway[3],
                iface
                    .dns
                    .map(|dns| format!("{}.{}.{}.{}", dns[0], dns[1], dns[2], dns[3]))
                    .unwrap_or_else(|| String::from("none"))
            );
            true
        }
        Err(err) => {
            crate::serial_println!("[smoltcp] DHCP configuration failed: {:?}", err);
            sync_from_kernel_config();
            false
        }
    }
}

/// Yapılandırılmış IPv4 adresini döndürür.
/// DHCP veya statik yapılandırma yapılmamışsa `None` döner.
pub fn get_ip() -> Option<[u8; 4]> {
    let _ = ensure_runtime_network();
    sync_from_kernel_config();
    NET_INTERFACE.lock().ip
}

/// Varsayılan ağ geçidini döndürür.
/// Ağ geçidi, yerel ağ dışındaki hostlara ulaşmak için kullanılır.
pub fn get_gateway() -> Option<[u8; 4]> {
    let _ = ensure_runtime_network();
    sync_from_kernel_config();
    NET_INTERFACE.lock().gateway
}

/// DNS sunucu adresini döndürür.
/// Bu adres `dns_lookup()` fonksiyonunda sorgu göndermek için kullanılır.
pub fn get_dns() -> Option<[u8; 4]> {
    let _ = ensure_runtime_network();
    sync_from_kernel_config();
    NET_INTERFACE.lock().dns
}

/// Alan adını IPv4 adresine çevirir (DNS çözümlemesi).
///
/// Gerçek implementasyonda smoltcp'nin `dns::DnsSocket` yapısı kullanılarak
/// DNS sunucusuna UDP port 53 üzerinden sorgu gönderilir.
/// Şu an yalnızca "localhost" ve "gateway" için sabit dönüşüm yapılır.
pub fn dns_lookup(hostname: &str) -> Option<[u8; 4]> {
    crate::serial_println!("[smoltcp] DNS lookup: {}", hostname);
    let _ = ensure_runtime_network();

    match hostname {
        "localhost" => Some([127, 0, 0, 1]),
        "gateway" => get_gateway(),
        _ => crate::net::dns::resolve_default(hostname)
            .ok()
            .map(|ip| *ip.as_bytes()),
    }
}

/// Belirtilen IP ve porta TCP bağlantısı kurar.
///
/// Gerçek implementasyonda yapılması gerekenler:
///   1. `smoltcp::socket::tcp::Socket` oluştur.
///   2. Rastgele yerel port seç (ephemeral port: 49152–65535).
///   3. `connect()` çağır; üç yönlü el sıkışması tamamlanana kadar bekle.
pub fn tcp_connect(ip: [u8; 4], port: u16) -> bool {
    let _ = ensure_runtime_network();
    let sock = match crate::net::socket::socket(
        crate::net::socket::AddressFamily::IPV4,
        crate::net::socket::SocketType::STREAM,
        crate::net::socket::Protocol::TCP,
    ) {
        Ok(sock) => sock,
        Err(err) => {
            crate::serial_println!("[smoltcp] TCP socket create failed: {:?}", err);
            return false;
        }
    };

    let addr = crate::net::socket::SocketAddr::new(
        crate::net::Ipv4Addr::from_bytes(ip),
        crate::net::Port(port),
    );
    let connected = crate::net::socket::connect(sock, addr).is_ok();
    let _ = crate::net::socket::close(sock);

    if !connected {
        crate::serial_println!(
            "[smoltcp] TCP connect failed: {}.{}.{}.{}:{}",
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            port
        );
    }

    connected
}

/// Belirtilen URL'ye HTTP GET isteği gönderir ve yanıt gövdesini döndürür.
///
/// Gerçek implementasyonda:
///   1. URL'yi ayrıştır (host + path).
///   2. DNS ile host adresini çöz.
///   3. TCP bağlantısı kur (port 80 veya HTTPS için 443).
///   4. HTTP/1.1 GET isteğini gönder.
///   5. Yanıt başlıklarını ayrıştır, gövdeyi oku.
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let _ = ensure_runtime_network();
    let client = crate::net::http::HttpClient::new();
    client
        .download(url)
        .map_err(|err| format!("HTTP error: {:?}", err))
}

fn sync_from_kernel_config() {
    let config = crate::net::get_config();
    let mut iface = NET_INTERFACE.lock();
    iface.ip = if config.ip_addr != [0, 0, 0, 0] {
        Some(config.ip_addr)
    } else {
        None
    };
    iface.gateway = if config.gateway != [0, 0, 0, 0] {
        Some(config.gateway)
    } else {
        None
    };
    iface.dns = config.dns_servers.first().copied();
    if crate::drivers::virtio_net::is_initialized() {
        iface.mac = Some(*crate::drivers::virtio_net::get_mac().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_complete_requires_ip_gateway_and_dns() {
        let mut config = crate::net::NetworkConfig::new();
        assert!(!runtime_config_complete(&config));
        config.ip_addr = SLIRP_FALLBACK_IP;
        assert!(!runtime_config_complete(&config));
        config.gateway = SLIRP_FALLBACK_GATEWAY;
        assert!(!runtime_config_complete(&config));
        config.dns_servers.push(SLIRP_FALLBACK_DNS);
        assert!(runtime_config_complete(&config));
    }

    #[test]
    fn build_slirp_fallback_config_publishes_dns_and_gateway() {
        crate::net::set_config(crate::net::NetworkConfig::new());
        let config = build_slirp_fallback_config();
        assert_eq!(config.ip_addr, SLIRP_FALLBACK_IP);
        assert_eq!(config.netmask, SLIRP_FALLBACK_NETMASK);
        assert_eq!(config.gateway, SLIRP_FALLBACK_GATEWAY);
        assert_eq!(
            config.dns_servers.first().copied(),
            Some(SLIRP_FALLBACK_DNS)
        );
        assert_eq!(config.hostname, "echos");
    }
}
