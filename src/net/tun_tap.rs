//! # TUN/TAP — Sanal Ağ Tüneli
//!
//! Linux `tun` ve `tap` sürücüleri, kullanıcı alanı programlarına sanal
//! ağ arayüzü sağlar:
//!
//! - **TUN**: IP seviyesinde (L3) sanal arayüz; kullanıcı alanı IP paketleri
//!   okur/yazar. Tipik: VPN istemcileri, container overlay ağlar.
//! - **TAP**: Ethernet seviyesinde (L2) sanal arayüz; kullanıcı alanı tam
//!   Ethernet çerçeveleri okur/yazar. Tipik: QEMU sanal makine ağı.
//!
//! ## Karşılaştırma
//!
//! | Özellik | TUN | TAP |
//! |---------|-----|-----|
//! | Okunan veri | IP paket | Ethernet frame |
//! | Yazılan veri | IP paket | Ethernet frame |
//! | Tipik kullanım | OpenVPN, WireGuard | QEMU, virtio-net host |
//! | Linux karakter aygıtı | /dev/net/tun | /dev/net/tun |
//!
//! ## Karakter Aygıtı Protokolü
//!
//! `/dev/net/tun` açılır, `ioctl(TUNSETIFF)` ile `ifr_name` ve `ifr_flags`
//! (IFF_TUN | IFF_TAP) ayarlanır. Sonra read/write ile paketler alınır/gönderilir.
//!
//! ## echOS Tasarımı
//!
//! echOS çekirdek alanında çalıştığı için `/dev/net/tun` yerine doğrudan
//! dahili ring buffer kullanılır. Kullanıcı alanı syscall'ları (ileride)
//! bu modüldeki kuyruklara bağlanır.

use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// TUN/TAP TIPLERİ
// ============================================================================

/// TUN/TAP modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunTapMode {
    /// IP seviyesi (L3) sanal arayüz
    Tun = 1,
    /// Ethernet seviyesi (L2) sanal arayüz
    Tap = 2,
}

/// Packet Information (PI) header — Linux `if_tun.h` ile uyumlu
///
/// 4 byte PI header: `flags(u16) | proto(u16)` (big-endian, network order)
/// - flags: 0x0000 = yok, 0x0FF0 = GSO offload
/// - proto: ETH_P_IP (0x0800), ETH_P_IPV6 (0x86DD), vs.
///
/// `IFF_NO_PI` set edilmişse bu header okuma/yazma'da yoktur.
pub const PI_HEADER_SIZE: usize = 4;

/// Linux `IFF_*` flag'leri (sadece echOS kapsamı)
pub mod iff_flags {
    /// TUN cihazı (L3)
    pub const IFF_TUN: u16 = 0x0001;
    /// TAP cihazı (L2)
    pub const IFF_TAP: u16 = 0x0002;
    /// PI (packet info) header ekleme (4 byte)
    pub const IFF_NO_PI: u16 = 0x1000;
}

/// Bir TUN/TAP cihazı
#[derive(Clone, Debug)]
pub struct TunTapDev {
    pub name: String,
    pub mode: TunTapMode,
    /// IFF_NO_PI set edilmişse kullanıcı alanında PI header yok
    pub no_pi: bool,
    /// Kullanıcı alanından çekirdeğe gelen paketler
    pub tx_queue: VecDeque<Vec<u8>>,
    /// Çekirdekten kullanıcı alanına gidecek paketler
    pub rx_queue: VecDeque<Vec<u8>>,
    pub mac: MacAddr,
    pub mtu: u16,
    pub up: bool,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    /// Persisted istatistik
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_drops: u64,
    pub rx_drops: u64,
}

impl TunTapDev {
    pub fn new(name: String, mode: TunTapMode) -> Self {
        TunTapDev {
            name,
            mode,
            // Linux default: PI header VAR; kullanıcı açıkça IFF_NO_PI isterse kapatır.
            no_pi: false,
            tx_queue: VecDeque::new(),
            rx_queue: VecDeque::new(),
            mac: MacAddr([0; 6]),
            mtu: 1500,
            up: false,
            ip: Ipv4Addr::new(0, 0, 0, 0),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            tx_packets: 0,
            rx_packets: 0,
            tx_bytes: 0,
            rx_bytes: 0,
            tx_drops: 0,
            rx_drops: 0,
        }
    }

    /// Linux `IFF_TUN`/`IFF_TAP`/`IFF_NO_PI` flag'lerinden cihaz oluşturur.
    pub fn new_with_flags(name: String, mode: TunTapMode, no_pi: bool) -> Self {
        let mut d = Self::new(name, mode);
        d.no_pi = no_pi;
        d
    }

    /// IFF_NO_PI set edilmişse PI header boyutunu (4 byte) çıkar
    fn mtu_budget(&self) -> usize {
        if self.no_pi {
            self.mtu as usize
        } else {
            (self.mtu as usize).saturating_sub(PI_HEADER_SIZE)
        }
    }

    /// Kullanıcı alanı okuma için — RX kuyruğundan bir frame/paket al
    ///
    /// `no_pi=false` ise (Linux default) frame'in önüne 4 byte PI header
    /// (`flags=0, proto=0x0800 IPv4`) eklenir; kullanıcı alanı
    /// `[4 byte PI][frame]` alır. `no_pi=true` ise sadece frame döner.
    pub fn read(&mut self) -> Option<Vec<u8>> {
        let frame = self.rx_queue.pop_front()?;
        if self.no_pi {
            Some(frame)
        } else {
            let mut out = Vec::with_capacity(PI_HEADER_SIZE + frame.len());
            out.extend_from_slice(&[0u8; 2]);
            out.extend_from_slice(&0x0800u16.to_be_bytes());
            out.extend_from_slice(&frame);
            Some(out)
        }
    }

    /// Kullanıcı alanı yazma — TX kuyruğuna paket ekle (çekirdeğe gönderilmek üzere)
    pub fn write(&mut self, data: Vec<u8>) -> Result<usize, &'static str> {
        if !self.up {
            return Err("device is down");
        }
        if data.len() > self.mtu_budget() {
            self.tx_drops += 1;
            return Err("packet exceeds MTU");
        }
        if self.tx_queue.len() >= MAX_TUNTAP_QUEUE {
            self.tx_drops += 1;
            return Err("tx queue full");
        }
        self.tx_packets += 1;
        self.tx_bytes += data.len() as u64;
        self.tx_queue.push_back(data);
        Ok(0)
    }
}

/// Maksimum kuyruk boyutu
pub const MAX_TUNTAP_QUEUE: usize = 512;

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static TUNTAP_DEVS: Mutex<BTreeMap<String, TunTapDev>> = Mutex::new(BTreeMap::new());

static TUNTAP_STATS: TunTapStats = TunTapStats::new();
struct TunTapStats {
    devices: AtomicU32,
    tun_count: AtomicU32,
    tap_count: AtomicU32,
    packets_in: AtomicU32,
    packets_out: AtomicU32,
}
impl TunTapStats {
    const fn new() -> Self {
        TunTapStats {
            devices: AtomicU32::new(0),
            tun_count: AtomicU32::new(0),
            tap_count: AtomicU32::new(0),
            packets_in: AtomicU32::new(0),
            packets_out: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni TUN/TAP cihazı oluştur
pub fn create(name: &str, mode: TunTapMode) -> Result<(), TunTapError> {
    let mut devs = TUNTAP_DEVS.lock();
    if devs.contains_key(name) {
        return Err(TunTapError::AlreadyExists);
    }
    let mut dev = TunTapDev::new(String::from(name), mode);
    dev.mac = MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    devs.insert(String::from(name), dev);
    TUNTAP_STATS.devices.fetch_add(1, Ordering::Relaxed);
    match mode {
        TunTapMode::Tun => TUNTAP_STATS.tun_count.fetch_add(1, Ordering::Relaxed),
        TunTapMode::Tap => TUNTAP_STATS.tap_count.fetch_add(1, Ordering::Relaxed),
    };
    crate::serial_println!(
        "[TUNTAP] created {} as {:?}",
        name,
        mode
    );
    Ok(())
}

/// Cihazı admin-up yap
pub fn set_up(name: &str, up: bool) -> Result<(), TunTapError> {
    let mut devs = TUNTAP_DEVS.lock();
    let dev = devs.get_mut(name).ok_or(TunTapError::NotFound)?;
    dev.up = up;
    Ok(())
}

/// Cihazı sil
pub fn destroy(name: &str) -> Result<(), TunTapError> {
    let mut devs = TUNTAP_DEVS.lock();
    devs.remove(name).ok_or(TunTapError::NotFound)?;
    TUNTAP_STATS.devices.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// Kullanıcı alanından okuma simülasyonu — RX kuyruğundan al
pub fn user_read(name: &str) -> Option<Vec<u8>> {
    TUNTAP_DEVS.lock().get_mut(name).and_then(|d| d.read())
}

/// Kullanıcı alanından yazma simülasyonu — TX kuyruğuna ekle
pub fn user_write(name: &str, data: Vec<u8>) -> Result<(), TunTapError> {
    TUNTAP_DEVS
        .lock()
        .get_mut(name)
        .ok_or(TunTapError::NotFound)?
        .write(data)
        .map_err(|_| TunTapError::QueueFull)
        .map(|_| ())
}

/// Çekirdek ağ yığınından cihaza paket gönder (RX kuyruğuna)
pub fn kernel_inject(name: &str, packet: Vec<u8>) -> Result<(), TunTapError> {
    let mut devs = TUNTAP_DEVS.lock();
    let dev = devs.get_mut(name).ok_or(TunTapError::NotFound)?;
    if dev.rx_queue.len() >= MAX_TUNTAP_QUEUE {
        dev.rx_drops += 1;
        return Err(TunTapError::QueueFull);
    }
    dev.rx_packets += 1;
    dev.rx_bytes += packet.len() as u64;
    dev.rx_queue.push_back(packet);
    TUNTAP_STATS.packets_in.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Cihazın TX kuyruğundan çekirdek ağ yığınına gönderilecek paketi al
pub fn kernel_read(name: &str) -> Option<Vec<u8>> {
    let mut devs = TUNTAP_DEVS.lock();
    let dev = devs.get_mut(name)?;
    let pkt = dev.tx_queue.pop_front();
    if pkt.is_some() {
        TUNTAP_STATS.packets_out.fetch_add(1, Ordering::Relaxed);
    }
    pkt
}

/// Cihaz istatistiklerini al
pub fn stats(name: &str) -> Option<TunTapDev> {
    TUNTAP_DEVS.lock().get(name).cloned()
}

pub struct TunTapInterface {
    cached_name: String,
    cached_mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
}

impl TunTapInterface {
    pub fn new(name: &str, mac: MacAddr) -> Self {
        TunTapInterface {
            cached_name: name.into(),
            cached_mac: mac,
            ip: Ipv4Addr::new(0, 0, 0, 0),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            up: false,
        }
    }

    pub fn register(name: &str, mode: TunTapMode) -> Result<Arc<Mutex<dyn NetInterface>>, TunTapError> {
        create(name, mode)?;
        let mac = TUNTAP_DEVS.lock().get(name).map(|d| d.mac).unwrap_or(MacAddr([0; 6]));
        let mut iface = TunTapInterface::new(name, mac);
        iface.up = true;
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        // Sync up state to the global dev
        let _ = set_up(name, true);
        Ok(iface_arc)
    }
}

impl NetInterface for TunTapInterface {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn mac(&self) -> MacAddr {
        self.cached_mac
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

    fn set_netmask(&mut self, netmask: Ipv4Addr) {
        self.netmask = netmask;
    }

    fn gateway(&self) -> Option<Ipv4Addr> {
        self.gateway
    }

    fn set_gateway(&mut self, gateway: Ipv4Addr) {
        self.gateway = Some(gateway);
    }

    fn is_up(&self) -> bool {
        self.up
    }

    fn set_up(&mut self, up: bool) {
        self.up = up;
        let mut devs = TUNTAP_DEVS.lock();
        if let Some(dev) = devs.get_mut(&self.cached_name) {
            dev.up = up;
        }
    }

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        let mut devs = TUNTAP_DEVS.lock();
        let dev = devs.get_mut(&self.cached_name).ok_or(NetError::NoInterface)?;
        dev.write(data.to_vec()).map_err(|_| NetError::BufferFull)?;
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        let mut devs = TUNTAP_DEVS.lock();
        let dev = devs.get_mut(&self.cached_name)?;
        dev.rx_queue.pop_front()
    }

    fn stats(&self) -> NetStats {
        let devs = TUNTAP_DEVS.lock();
        if let Some(dev) = devs.get(&self.cached_name) {
            NetStats {
                rx_packets: dev.rx_packets,
                tx_packets: dev.tx_packets,
                rx_bytes: dev.rx_bytes,
                tx_bytes: dev.tx_bytes,
                rx_errors: 0,
                tx_errors: 0,
                rx_dropped: dev.rx_drops,
                tx_dropped: dev.tx_drops,
            }
        } else {
            NetStats {
                rx_packets: 0,
                tx_packets: 0,
                rx_bytes: 0,
                tx_bytes: 0,
                rx_errors: 0,
                tx_errors: 0,
                rx_dropped: 0,
                tx_dropped: 0,
            }
        }
    }

    fn mtu(&self) -> u16 {
        1500
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunTapError {
    NotFound,
    AlreadyExists,
    QueueFull,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_round_trip() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        d.no_pi = true;
        d.write(vec![1, 2, 3, 4]).unwrap();
        let p = d.tx_queue.pop_front().unwrap();
        assert_eq!(p, vec![1, 2, 3, 4]);
    }

    #[test]
    fn kernel_inject_increments_rx() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        d.no_pi = true;
        kernel_inject_into(&mut d, vec![5, 6, 7, 8]);
        let p = d.rx_queue.pop_front().unwrap();
        assert_eq!(p, vec![5, 6, 7, 8]);
        assert_eq!(d.rx_packets, 1);
    }

    fn kernel_inject_into(d: &mut TunTapDev, pkt: Vec<u8>) {
        d.rx_packets += 1;
        d.rx_bytes += pkt.len() as u64;
        d.rx_queue.push_back(pkt);
    }

    #[test]
    fn write_to_down_device_fails() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.no_pi = true;
        // d.up = false
        let r = d.write(vec![1, 2, 3]);
        assert!(r.is_err());
    }

    #[test]
    fn oversized_packet_drops() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        d.no_pi = true;
        d.mtu = 100;
        let big = vec![0u8; 200];
        let r = d.write(big);
        assert!(r.is_err());
        assert_eq!(d.tx_drops, 1);
    }

    #[test]
    fn pi_header_prepended_when_not_no_pi() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        // no_pi default = false (Linux default with IFF_NO_PI unset)
        kernel_inject_into(&mut d, vec![0x45, 0x00, 0x00, 0x14]);
        let p = d.read().unwrap();
        // 4 byte PI: flags(2) | proto(2, BE) | frame
        assert_eq!(p.len(), 4 + 4);
        assert_eq!(&p[0..2], &[0, 0]);
        assert_eq!(u16::from_be_bytes([p[2], p[3]]), 0x0800);
        assert_eq!(&p[4..], &[0x45, 0x00, 0x00, 0x14]);
    }

    #[test]
    fn pi_header_absent_when_no_pi() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        d.no_pi = true;
        kernel_inject_into(&mut d, vec![0x45, 0x00, 0x00, 0x14]);
        let p = d.read().unwrap();
        assert_eq!(p, vec![0x45, 0x00, 0x00, 0x14]);
    }

    #[test]
    fn mtu_budget_excludes_pi_header() {
        let mut d = TunTapDev::new("tun0".into(), TunTapMode::Tun);
        d.up = true;
        d.mtu = 100;
        d.no_pi = true;
        // no_pi=true: 100 byte frame kabul
        d.write(vec![0u8; 100]).unwrap();
        // no_pi=false: mtu-4 = 96 byte kabul
        d.no_pi = false;
        d.write(vec![0u8; 96]).unwrap();
        // 97 byte red
        assert!(d.write(vec![0u8; 97]).is_err());
    }

    #[test]
    fn iff_flags_values() {
        assert_eq!(iff_flags::IFF_TUN, 0x0001);
        assert_eq!(iff_flags::IFF_TAP, 0x0002);
        assert_eq!(iff_flags::IFF_NO_PI, 0x1000);
    }

    #[test]
    fn tuntap_interface_name_and_mac() {
        let iface = TunTapInterface::new("tun0", MacAddr([0x02, 0, 0, 0, 0, 1]));
        assert_eq!(iface.name(), "tun0");
        assert_eq!(iface.mac(), MacAddr([0x02, 0, 0, 0, 0, 1]));
    }

    #[test]
    fn tuntap_interface_ip_config() {
        let mut iface = TunTapInterface::new("tun1", MacAddr([0x02, 0, 0, 0, 0, 2]));
        iface.set_ip(Ipv4Addr::new(10, 0, 0, 5));
        iface.set_netmask(Ipv4Addr::new(255, 255, 255, 0));
        iface.set_gateway(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(iface.ip(), Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(iface.netmask(), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(iface.gateway(), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn tuntap_interface_up_down() {
        let mut iface = TunTapInterface::new("tun2", MacAddr([0x02, 0, 0, 0, 0, 3]));
        assert!(!iface.is_up());
        iface.set_up(true);
        assert!(iface.is_up());
        iface.set_up(false);
        assert!(!iface.is_up());
    }

    #[test]
    fn tuntap_interface_send_down_returns_error() {
        let mut iface = TunTapInterface::new("tun3", MacAddr([0x02, 0, 0, 0, 0, 4]));
        assert_eq!(iface.send(&[1u8; 10]), Err(NetError::NotUp));
    }

    #[test]
    fn tuntap_interface_stats_default_zero() {
        let iface = TunTapInterface::new("tun4", MacAddr([0x02, 0, 0, 0, 0, 5]));
        let stats = iface.stats();
        assert_eq!(stats.rx_packets, 0);
        assert_eq!(stats.tx_packets, 0);
    }

    #[test]
    fn tuntap_interface_mtu() {
        let iface = TunTapInterface::new("tun5", MacAddr([0x02, 0, 0, 0, 0, 6]));
        assert_eq!(iface.mtu(), 1500);
    }
}

