//! # Ağ Aygıt Sürücüleri (Network Device Drivers)
//!
//! VirtIO-Net ve Loopback arabirim sürücüleri.
//!
//! ## Bu Modüldeki Arabirimler
//!
//! ```text
//! +--------------------------+     +--------------------------+
//! |   LoopbackInterface      |     |   VirtioNetInterface     |
//! |   (lo: 127.0.0.1/8)     |     |   (eth0: DHCP)           |
//! |                          |     |                          |
//! |  send() → rx_queue       |     |  send() → VirtIO TX ring |
//! |  recv() ← rx_queue.pop  |     |  recv() ← VirtIO RX ring |
//! +--------------------------+     +--------------------------+
//!          |                                  |
//!          +------ NetInterface (trait) -------+
//!                         |
//!                  Protocol Stack
//! ```
//!
//! ## VirtIO-Net Nedir?
//!
//! VirtIO-Net, QEMU/KVM sanallaştırma ortamında sanal ağ kartı sürücüsüdür.
//! Gerçek donanım yerine ortak bir "sanal kuyruk" (virtqueue) mekanizması
//! kullanarak ana makine ile ağ paketlerini paylaşır.
//!
//! ## Loopback Arabirim Nedir?
//!
//! Geri döngü arabirimi (loopback), gönderilen paketleri doğrudan alma
//! kuyruğuna yazar. Ağ kartı gerektirmez. Genellikle 127.0.0.1 ile bilinir.

use super::{MacAddr, Ipv4Addr, NetInterface, NetError, NetStats, register_interface};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// GERİ DÖNGÜ ARABİRİMİ (LOOPBACK INTERFACE)
// ============================================================================
//
// Loopback arabirimi Linux'taki `lo` arayüzüne karşılık gelir.
// Gönderilen her paket anında alma kuyruğuna düşer — ağ donanımı gerekmez.
//
//   Uygulama
//      │  send("127.0.0.1")
//      ▼
//   rx_queue.push(data)    ← gönderilen veri kuyruğa eklenir
//      │
//      ▼
//   recv() → rx_queue.pop  ← aynı uygulama veya başka bir süreç alır

/// Geri döngü arabirimi (127.0.0.1, 8-bit ağ maskesi 255.0.0.0)
pub struct LoopbackInterface {
    name: String,
    mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    up: bool,
    rx_queue: Vec<Vec<u8>>,
    stats: NetStats,
}

impl LoopbackInterface {
    pub fn new() -> Self {
        LoopbackInterface {
            name: String::from("lo"),
            mac: MacAddr::ZERO,
            ip: Ipv4Addr::LOCALHOST,
            netmask: Ipv4Addr::new(255, 0, 0, 0),
            up: true,
            rx_queue: Vec::new(),
            stats: NetStats::default(),
        }
    }
}

impl NetInterface for LoopbackInterface {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn mac(&self) -> MacAddr {
        self.mac
    }
    
    fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    
    fn set_ip(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }
    
    fn netmask(&self) -> Ipv4Addr {
        self.netmask
    }
    
    fn set_netmask(&mut self, mask: Ipv4Addr) {
        self.netmask = mask;
    }
    
    fn gateway(&self) -> Option<Ipv4Addr> {
        None
    }
    
    fn set_gateway(&mut self, _gw: Ipv4Addr) {
        // Loopback arabiriminin ağ geçidi olmaz
    }
    
    fn is_up(&self) -> bool {
        self.up
    }
    
    fn set_up(&mut self, up: bool) {
        self.up = up;
    }
    
    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        
        // Loopback: gönderilen veriyi alma kuyruğuna ekle (geri döngü mantığı)
        self.rx_queue.push(data.to_vec());
        self.stats.tx_packets += 1;
        self.stats.tx_bytes += data.len() as u64;
        
        Ok(())
    }
    
    fn recv(&mut self) -> Option<Vec<u8>> {
        if let Some(data) = self.rx_queue.pop() {
            self.stats.rx_packets += 1;
            self.stats.rx_bytes += data.len() as u64;
            return Some(data);
        }
        None
    }
    
    fn stats(&self) -> NetStats {
        self.stats.clone()
    }
    
    fn mtu(&self) -> u16 {
        65535
    }
}

// ============================================================================
// VİRTIO-NET ARABİRİMİ (VIRTIO-NET INTERFACE)
// ============================================================================
//
// VirtIO (Virtual I/O), QEMU ve KVM gibi sanallaştırma katmanlarında
// aygıt iletişimini standartlaştıran bir OASIS spesifikasyonudur.
//
// VirtIO-Net'in çalışma mantığı:
//   1. Misafir çekirdek (echOS), TX-virtqueue'ya Ethernet çerçevesi yazar.
//   2. Hipervisora "kick" (zil sinyali) gönderilir.
//   3. Hipervisor paketi gerçek/sanal ağa iletir.
//   4. Gelen paketler RX-virtqueue'ya kopyalanır; misafir çekirdek keser
//      alır ve `recv()` ile okur.
//
// Bu implementasyon şu an bir "stub" (iskelet) durumundadır.
// Gerçek gönderme/alma için `src/drivers/virtio_ffi` entegrasyonu gerekir.

/// VirtIO-Net arabirimi (iskelet - virtio sürücü entegrasyonu gerektirir)
///
/// Alanlar:
/// - `name`    : Arabirim adı; "eth0" Linux/POSIX standardıyla uyumludur
/// - `mac`     : 6 baytlık donanım adresi; QEMU'da 52:54:00:xx:xx:xx formatı
/// - `ip`      : DHCP ile atanır; başlangıçta 0.0.0.0 (UNSPECIFIED)
/// - `netmask` : Varsayılan /24 maskesi (255.255.255.0)
/// - `gateway` : Dış ağa çıkış noktası; DHCP ile öğrenilir
/// - `up`      : false başlar; DHCP veya statik yapılandırmadan sonra true olur
/// - `stats`   : TX/RX istatistik sayaçları (hata ayıklama ve izleme için)
pub struct VirtioNetInterface {
    name: String,
    mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
    stats: NetStats,
    // TODO: Add virtio queue pointers
}

impl VirtioNetInterface {
    /// Yeni bir VirtIO-Net arabirimi oluşturur.
    ///
    /// `mac` parametresi hipervisor tarafından belirlenen donanım adresidir.
    /// Arabirim başlangıçta `up = false` ve `ip = 0.0.0.0` durumundadır;
    /// `configure_dhcp()` veya `configure_static()` çağrıldıktan sonra aktif olur.
    pub fn new(mac: MacAddr) -> Self {
        VirtioNetInterface {
            name: String::from("eth0"),
            mac,
            ip: Ipv4Addr::UNSPECIFIED,
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            up: false,
            stats: NetStats::default(),
        }
    }
}

impl NetInterface for VirtioNetInterface {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn mac(&self) -> MacAddr {
        self.mac
    }
    
    fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    
    fn set_ip(&mut self, ip: Ipv4Addr) {
        self.ip = ip;
    }
    
    fn netmask(&self) -> Ipv4Addr {
        self.netmask
    }
    
    fn set_netmask(&mut self, mask: Ipv4Addr) {
        self.netmask = mask;
    }
    
    fn gateway(&self) -> Option<Ipv4Addr> {
        self.gateway
    }
    
    fn set_gateway(&mut self, gw: Ipv4Addr) {
        self.gateway = Some(gw);
    }
    
    fn is_up(&self) -> bool {
        self.up
    }
    
    fn set_up(&mut self, up: bool) {
        self.up = up;
    }
    
    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        
        // TODO: Implement virtio-net TX
        // This requires integration with virtio_ffi module
        
        self.stats.tx_packets += 1;
        self.stats.tx_bytes += data.len() as u64;
        
        crate::serial_println!("[NET] TX {} bytes on {}", data.len(), self.name);
        
        Ok(())
    }
    
    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        
        // TODO: Implement virtio-net RX
        // This requires integration with virtio_ffi module
        
        None
    }
    
    fn stats(&self) -> NetStats {
        self.stats.clone()
    }
    
    fn mtu(&self) -> u16 {
        1500
    }
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================
//
// Çekirdek önyükleme sürecinde ağ alt sistemi bu bölümle hazırlanır.
// Her arabirim `Arc<Mutex<T>>` ile sarılarak global listeye eklenir:
//   - `Arc`   : Atomik referans sayacı — Rust'da çoklu sahiplik sağlar.
//   - `Mutex` : Karşılıklı dışlama kilidi — aynı anda yalnızca bir görevin
//               arabirime erişmesini garantiler; çok çekirdekli güvenlik.

/// Ağ aygıtlarını başlatır ve global arabirim listesine kaydeder.
///
/// loopback önce eklenir; daha sonra VirtIO-Net iskelet arabirimi eklenir.
/// MAC adresi şimdilik sabit kodlanmıştır; gerçek implementasyonda
/// hipervisordan PCI yapılandırma alanı okunarak öğrenilmelidir.
pub fn init() {
    crate::serial_println!("[NETDEV] Initializing network devices...");

    // Create loopback interface
    let lo = Arc::new(Mutex::new(LoopbackInterface::new()));
    register_interface(lo);

    // Try to create VirtIO-Net interface
    // TODO: Detect virtio-net device and get MAC
    let eth0 = Arc::new(Mutex::new(VirtioNetInterface::new(MacAddr::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]))));
    register_interface(eth0);

    crate::serial_println!("[NETDEV] Network devices initialized");
}

/// DHCP ile otomatik ağ yapılandırması yapar.
///
/// DHCP (Dynamic Host Configuration Protocol) 4 adımlı el sıkışması:
///   1. DISCOVER : Broadcast — "Ağda DHCP sunucusu var mı?"
///   2. OFFER    : Sunucu → istemci IP teklifi sunar.
///   3. REQUEST  : İstemci teklifi onaylar.
///   4. ACK      : Sunucu IP, maske ve gateway'i kesinleştirir.
///
/// Bu implementasyon basitleştirilmiş döngüyle yanıt bekler (~100 deneme).
/// Gerçek bir çekirdekte bu işlem async/await veya IRQ tabanlı olmalıdır.
pub fn configure_dhcp(iface_name: &str) -> Result<super::NetworkConfig, NetError> {
    let iface = super::get_interface(iface_name).ok_or(NetError::NoInterface)?;
    
    {
        let mut iface = iface.lock();
        iface.set_up(true);
    }
    
    // Start DHCP discovery
    super::dhcp::discover()?;
    
    // Wait for response (simplified - should be async)
    for _ in 0..100 {
        if let Ok(config) = super::dhcp::process_response() {
            // Apply configuration
            let mut iface = iface.lock();
            iface.set_ip(Ipv4Addr::from_bytes(config.ip_addr));
            iface.set_netmask(Ipv4Addr::from_bytes(config.netmask));
            if config.gateway != [0, 0, 0, 0] {
                iface.set_gateway(Ipv4Addr::from_bytes(config.gateway));
            }
            
            super::set_config(config.clone());
            
            return Ok(config);
        }
        
        // Small delay
        for _ in 0..10000 {
            core::hint::spin_loop();
        }
    }
    
    Err(NetError::Timeout)
}

/// Arabirimi statik IP ile yapılandırır.
///
/// DHCP kullanılamadığında (sunucu yok, ağ bağlantısı yok) IP adresi,
/// ağ maskesi ve ağ geçidi elle belirtilir. Tüm değerler hem arabirim
/// nesnesine hem de global `NetworkConfig` yapısına yazılır.
///
/// Örnek CIDR gösterimi:
///   ip = 192.168.1.5, netmask = 255.255.255.0  →  192.168.1.5/24
pub fn configure_static(
    iface_name: &str,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
) -> Result<(), NetError> {
    let iface = super::get_interface(iface_name).ok_or(NetError::NoInterface)?;
    
    let mut iface = iface.lock();
    iface.set_ip(ip);
    iface.set_netmask(netmask);
    if let Some(gw) = gateway {
        iface.set_gateway(gw);
    }
    iface.set_up(true);
    
    let mut config = super::get_config();
    config.ip_addr = *ip.as_bytes();
    config.netmask = *netmask.as_bytes();
    if let Some(gw) = gateway {
        config.gateway = *gw.as_bytes();
    }
    super::set_config(config);
    
    crate::serial_println!("[NETDEV] {} configured: {}/{}", 
        iface_name, 
        super::socket::format_ipv4(ip),
        super::socket::format_ipv4(netmask));
    
    Ok(())
}
