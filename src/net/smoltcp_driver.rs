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
//! Bu modül şu an smoltcp'yi doğrudan kullanmayan bir **stub** (iskelet)
//! katmanıdır. Gerçek entegrasyon için smoltcp'nin `Device` trait'inin
//! VirtIO-Net üzerinde implemente edilmesi gerekir.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

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

    // VirtIO-Net'den MAC adresi al
    if crate::drivers::virtio_net::is_initialized() {
        crate::serial_println!("[smoltcp] VirtIO-Net available");
    }

    true
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

    // TODO: smoltcp DHCP client kullan
    // Şimdilik QEMU user-mode network için varsayılan IP
    let mut iface = NET_INTERFACE.lock();
    iface.ip = Some([10, 0, 2, 15]); // QEMU default guest IP
    iface.gateway = Some([10, 0, 2, 2]); // QEMU default gateway
    iface.dns = Some([10, 0, 2, 3]); // QEMU default DNS

    crate::serial_println!("[smoltcp] DHCP configured: IP={:?}", iface.ip);
    true
}

/// Yapılandırılmış IPv4 adresini döndürür.
/// DHCP veya statik yapılandırma yapılmamışsa `None` döner.
pub fn get_ip() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().ip
}

/// Varsayılan ağ geçidini döndürür.
/// Ağ geçidi, yerel ağ dışındaki hostlara ulaşmak için kullanılır.
pub fn get_gateway() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().gateway
}

/// DNS sunucu adresini döndürür.
/// Bu adres `dns_lookup()` fonksiyonunda sorgu göndermek için kullanılır.
pub fn get_dns() -> Option<[u8; 4]> {
    NET_INTERFACE.lock().dns
}

/// Alan adını IPv4 adresine çevirir (DNS çözümlemesi).
///
/// Gerçek implementasyonda smoltcp'nin `dns::DnsSocket` yapısı kullanılarak
/// DNS sunucusuna UDP port 53 üzerinden sorgu gönderilir.
/// Şu an yalnızca "localhost" ve "gateway" için sabit dönüşüm yapılır.
pub fn dns_lookup(hostname: &str) -> Option<[u8; 4]> {
    crate::serial_println!("[smoltcp] DNS lookup: {}", hostname);

    // TODO: smoltcp DNS socket kullan
    // Şimdilik bilinen hostlar için hardcoded
    match hostname {
        "localhost" => Some([127, 0, 0, 1]),
        "gateway" => get_gateway(),
        _ => {
            crate::serial_println!("[smoltcp] DNS: unknown host");
            None
        }
    }
}

/// Belirtilen IP ve porta TCP bağlantısı kurar.
///
/// Gerçek implementasyonda yapılması gerekenler:
///   1. `smoltcp::socket::tcp::Socket` oluştur.
///   2. Rastgele yerel port seç (ephemeral port: 49152–65535).
///   3. `connect()` çağır; üç yönlü el sıkışması tamamlanana kadar bekle.
pub fn tcp_connect(_ip: [u8; 4], _port: u16) -> bool {
    // TODO: smoltcp TCP socket
    crate::serial_println!("[smoltcp] TCP connect not implemented");
    false
}

/// Belirtilen URL'ye HTTP GET isteği gönderir ve yanıt gövdesini döndürür.
///
/// Gerçek implementasyonda:
///   1. URL'yi ayrıştır (host + path).
///   2. DNS ile host adresini çöz.
///   3. TCP bağlantısı kur (port 80 veya HTTPS için 443).
///   4. HTTP/1.1 GET isteğini gönder.
///   5. Yanıt başlıklarını ayrıştır, gövdeyi oku.
pub fn http_get(_url: &str) -> Result<Vec<u8>, String> {
    // TODO: smoltcp HTTP
    Err(String::from("HTTP not implemented"))
}
