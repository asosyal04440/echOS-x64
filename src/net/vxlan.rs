//! # VXLAN (Virtual Extensible LAN, RFC 7348)
//!
//! VXLAN, mevcut Layer 3 ağ altyapısı üzerinde Layer 2 segmentlerini
//! genişletmek için kullanılan bir overlay ağ protokolüdür. Veri
//! merkezlerinde ve bulut ağlarında yaygın olarak kullanılır.
//!
//! ## VXLAN Nedir?
//!
//! VXLAN, bir Ethernet frame'i UDP üzerine paketleyerek uzak bir VXLAN
//! Tunnel Endpoint (VTEP) arasında taşır. VTEP'ler, gelen VXLAN paketini
//! alır, içindeki orijinal Ethernet frame'i çıkarır ve hedef LAN'a
//! iletir — bu sayede uzak LAN segmentleri mantıksal olarak tek bir
//! ağ gibi görünür.
//!
//! ## VXLAN Paket Formatı
//!
//! ```text
//! +-----+------+-----+------+-----+-----+-----+-----+
//! | Eth |  IP  | UDP | VXLAN|         Inner         |
//! | hdr | hdr  | hdr | hdr  |   Original L2 frame  |
//! +-----+------+-----+------+-----+-----+-----+-----+
//! Outer UDP dst port: 4789 (IANA)
//! ```
//!
//! ## VXLAN Header (8 byte, RFC 7348 §3.1)
//!
//! ```text
//! 0                   1                   2                   3
//! 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |R|R|R|R|I|R|R|R|            Reserved                           |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                VXLAN Network Identifier (VNI)                 |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                Reserved                                       |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! - **Byte 0**: üst 4 bit reserved, **bit 3 = I (VNI valid; MUST be 1 per RFC 7348 §3.1)**, alt 3 bit reserved
//! - **Bytes 1..=3**: reserved (24 bit)
//! - **Bytes 4..=6**: VNI (24 bit, big-endian)
//! - **Byte 7**: reserved (8 bit)
//! - **VNI (24-bit)**: VXLAN Segment Identifier, 1-16777215
//!
//! ## VTEP (VXLAN Tunnel Endpoint)
//!
//! Her VTEP'in:
//! - Local IP adresi (underlay kaynak)
//! - Bir veya daha fazla uzak VTEP IP adresi (peer list)
//! - Bir VNI
//! - MAC-FDB: hedef MAC → VTEP eşlemesi
//!
//! ## Learning Flooding
//!
//! VXLAN'da hedef MAC tanınmadığında frame, "flood" edilir — tüm peer
//! VTEP'lere gönderilir (multicast veya unicast list).
//!
//! ## echOS Tasarımı
//!
//! `VxlanDev` ad + VNI + peer listesi + MAC-FDB. Çekirdek ağ yığınından
//! `VxlanDev::encap(eth_frame, dest_mac)` ile UDP paketine çevrilir.

use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// VXLAN SABİTLERİ
// ============================================================================

/// IANA tarafından atanmış VXLAN UDP port
pub const VXLAN_UDP_PORT: u16 = 4789;

/// Maksimum VNI (24-bit)
pub const VXLAN_VNI_MAX: u32 = 16_777_215;

/// VXLAN I-flag (RFC 7348 §3.1: VNI valid; MUST be 1 on transmit)
pub const VXLAN_FLAG_I: u8 = 0x08;

/// VXLAN G-flag (Cisco GBP extension, optional)
pub const VXLAN_FLAG_G: u8 = 0x80;

// ============================================================================
// VXLAN HEADER
// ============================================================================

/// VXLAN 8-byte header
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VxlanHeader {
    /// Flags (üst 4 bit: R|R|R|I alt sıra; bayt 0)
    pub flags: u8,
    /// 24-bit VNI (bayt 4..=6)
    pub vni: u32,
}

impl VxlanHeader {
    pub fn new(vni: u32) -> Self {
        VxlanHeader {
            flags: VXLAN_FLAG_I,
            vni: vni & 0x00FF_FFFF,
        }
    }

    /// GBP ile: G|I set
    pub fn new_with_gbp(vni: u32) -> Self {
        VxlanHeader {
            flags: VXLAN_FLAG_I | VXLAN_FLAG_G,
            vni: vni & 0x00FF_FFFF,
        }
    }

    /// I flag set mi? (RFC 7348: receive MUST check)
    pub fn i_flag_set(&self) -> bool {
        (self.flags & VXLAN_FLAG_I) != 0
    }

    /// VXLAN header'ı serileştir (8 byte) — RFC 7348 §3.1 wire format
    ///
    /// Byte 0: flags, bytes 1..=3: reserved=0, bytes 4..=6: VNI big-endian,
    /// byte 7: reserved=0.
    pub fn serialize(&self) -> [u8; 8] {
        let vni_bytes = self.vni.to_be_bytes();
        [
            self.flags,
            0,                // reserved
            0,                // reserved
            0,                // reserved
            vni_bytes[1],     // VNI byte 0 (MSB of 24-bit VNI)
            vni_bytes[2],     // VNI byte 1
            vni_bytes[3],     // VNI byte 2 (LSB of 24-bit VNI)
            0,                // reserved
        ]
    }

    /// 8 byte parse et. RFC 7348: I flag MUST be 1; VNI MUST be != 0.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        if (data[0] & VXLAN_FLAG_I) == 0 {
            return None;
        }
        let vni = ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | (data[6] as u32);
        if vni == 0 {
            return None;
        }
        Some(VxlanHeader {
            flags: data[0],
            vni,
        })
    }
}

// ============================================================================
// VTEP (VXLAN Tunnel Endpoint)
// ============================================================================

/// Bir VTEP tanımı
#[derive(Clone, Debug)]
pub struct VtepPeer {
    pub ip: Ipv4Addr,
    pub udp_port: u16,
}

#[derive(Clone, Debug)]
pub struct VxlanDev {
    pub name: String,
    pub vni: u32,
    pub local_ip: Ipv4Addr,
    pub local_udp_port: u16,
    pub mac: MacAddr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub peers: Vec<VtepPeer>,
    /// MAC-FDB: hedef MAC → VTEP IP
    pub mac_to_vtep: BTreeMap<[u8; 6], Ipv4Addr>,
    pub mtu: u16,
    pub up: bool,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub flood_count: u64,
    /// RX queue for decapsulated frames (filled by underlay handler)
    pub rx_queue: VecDeque<Vec<u8>>,
}

impl VxlanDev {
    pub fn new(name: String, vni: u32, local_ip: Ipv4Addr) -> Self {
        VxlanDev {
            name,
            vni: vni.min(VXLAN_VNI_MAX),
            local_ip,
            local_udp_port: VXLAN_UDP_PORT,
            mac: MacAddr([0x02; 6]),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            peers: Vec::new(),
            mac_to_vtep: BTreeMap::new(),
            mtu: 1450,
            up: false,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            flood_count: 0,
            rx_queue: VecDeque::new(),
        }
    }

    /// Peer VTEP ekle
    pub fn add_peer(&mut self, ip: Ipv4Addr) {
        if !self.peers.iter().any(|p| p.ip == ip) {
            self.peers.push(VtepPeer {
                ip,
                udp_port: VXLAN_UDP_PORT,
            });
        }
    }

    /// MAC-FDB'ye VTEP eşlemesi ekle (öğrenme)
    pub fn learn(&mut self, mac: [u8; 6], vtep: Ipv4Addr) {
        self.mac_to_vtep.insert(mac, vtep);
    }

    /// Bir Ethernet frame'i VXLAN+UDP+IP paketine çevir
    ///
    /// Dönüş: (hedef_vtep_ip, udp_paket_inner_bytes)
    /// udp_paket_inner_bytes = UDP header (src=local, dst=4789) + VXLAN header + original frame
    pub fn encap(&mut self, eth_frame: &[u8]) -> Result<(Ipv4Addr, Vec<u8>), VxlanError> {
        if !self.up {
            return Err(VxlanError::DeviceDown);
        }
        if eth_frame.len() < 14 {
            return Err(VxlanError::InvalidFrame);
        }
        if eth_frame.len() > self.mtu as usize {
            return Err(VxlanError::FrameTooLarge);
        }
        let dst_mac = [eth_frame[0], eth_frame[1], eth_frame[2], eth_frame[3], eth_frame[4], eth_frame[5]];

        // Hedef VTEP seç
        let dest_vtep = if let Some(v) = self.mac_to_vtep.get(&dst_mac) {
            *v
        } else {
            // Flood: ilk peer'a gönder (Linux davranışı: tüm peer'lara unicast flood)
            if self.peers.is_empty() {
                return Err(VxlanError::NoPeers);
            }
            self.flood_count += 1;
            self.peers[0].ip
        };

        // VXLAN header + inner frame
        let vxlan_hdr = VxlanHeader::new(self.vni);
        let mut udp_payload = Vec::with_capacity(8 + eth_frame.len());
        udp_payload.extend_from_slice(&vxlan_hdr.serialize());
        udp_payload.extend_from_slice(eth_frame);

        // UDP header (8 byte) ekle
        let mut udp_pkt = Vec::with_capacity(8 + udp_payload.len());
        udp_pkt.extend_from_slice(&self.local_udp_port.to_be_bytes());
        udp_pkt.extend_from_slice(&VXLAN_UDP_PORT.to_be_bytes());
        // length = 8 (header) + payload
        let udp_len = 8 + udp_payload.len() as u16;
        udp_pkt.extend_from_slice(&udp_len.to_be_bytes());
        udp_pkt.extend_from_slice(&[0u8; 2]); // checksum (UDP/IPv4'te opsiyonel)
        udp_pkt.extend_from_slice(&udp_payload);

        self.tx_packets += 1;
        self.tx_bytes += udp_pkt.len() as u64;

        Ok((dest_vtep, udp_pkt))
    }

    /// Bir UDP payload'ı al, VXLAN header'ı parse et, inner frame'i çıkar
    pub fn decap(&mut self, udp_payload: &[u8]) -> Result<Vec<u8>, VxlanError> {
        if udp_payload.len() < 8 {
            return Err(VxlanError::InvalidFrame);
        }
        let hdr = VxlanHeader::parse(&udp_payload[..8]).ok_or(VxlanError::InvalidFrame)?;
        if hdr.vni != self.vni {
            return Err(VxlanError::VniMismatch);
        }
        let inner = udp_payload[8..].to_vec();

        // Source MAC öğren
        if inner.len() >= 6 {
            // inner SA 6. byte'tan başlar (DA=0..6, SA=6..12)
            let src_mac = [inner[6], inner[7], inner[8], inner[9], inner[10], inner[11]];
            self.mac_to_vtep.entry(src_mac).or_insert(self.local_ip);
        }
        self.rx_packets += 1;
        self.rx_bytes += inner.len() as u64;
        if self.rx_queue.len() < 512 {
            self.rx_queue.push_back(inner.clone());
        }
        Ok(inner)
    }
}

pub struct VxlanInterface {
    dev: Arc<Mutex<VxlanDev>>,
    cached_name: String,
    cached_mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
}

impl VxlanInterface {
    pub fn new(dev: Arc<Mutex<VxlanDev>>) -> Self {
        let cached_name = dev.lock().name.clone();
        let cached_mac = dev.lock().mac;
        let ip = dev.lock().local_ip;
        let netmask = dev.lock().netmask;
        VxlanInterface {
            dev,
            cached_name,
            cached_mac,
            ip,
            netmask,
            gateway: None,
            up: false,
        }
    }

    pub fn register(name: &str, vni: u32, local_ip: Ipv4Addr) -> Result<Arc<Mutex<dyn NetInterface>>, &'static str> {
        let vxlan_dev = VxlanDev::new(name.into(), vni, local_ip);
        let dev_arc = Arc::new(Mutex::new(vxlan_dev));
        let iface = VxlanInterface::new(dev_arc);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        Ok(iface_arc)
    }
}

impl NetInterface for VxlanInterface {
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
        self.dev.lock().local_ip = ip;
    }

    fn netmask(&self) -> Ipv4Addr {
        self.netmask
    }

    fn set_netmask(&mut self, netmask: Ipv4Addr) {
        self.netmask = netmask;
        self.dev.lock().netmask = netmask;
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
        self.dev.lock().up = up;
    }

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        let mut dev = self.dev.lock();
        let (dest_vtep, udp_pkt) = dev.encap(data).map_err(|_| NetError::InvalidPacket)?;
        // Send to underlay: try to find a parent interface with the dest IP
        let iface_name = alloc::format!("vxlan_underlay_{}", dest_vtep);
        if let Some(parent) = get_interface(&iface_name) {
            let mut guard = parent.lock();
            guard.send(&udp_pkt)?;
        }
        drop(dev);
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        // Check for incoming encapsulated packets from underlay interfaces
        // This would normally be called from the UDP socket layer
        // For now, return a placeholder that the underlay UDP handler fills
        let mut dev = self.dev.lock();
        dev.rx_queue.pop_front()
    }

    fn stats(&self) -> NetStats {
        let dev = self.dev.lock();
        NetStats {
            rx_packets: dev.rx_packets,
            tx_packets: dev.tx_packets,
            rx_bytes: dev.rx_bytes,
            tx_bytes: dev.tx_bytes,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
        }
    }

    fn mtu(&self) -> u16 {
        self.dev.lock().mtu
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VxlanError {
    DeviceDown,
    InvalidFrame,
    FrameTooLarge,
    NoPeers,
    VniMismatch,
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static VXLAN_DEVS: Mutex<BTreeMap<String, VxlanDev>> = Mutex::new(BTreeMap::new());

static VXLAN_STATS: VxlanStats = VxlanStats::new();
struct VxlanStats {
    devices: AtomicU32,
    encap_ok: AtomicU32,
    decap_ok: AtomicU32,
    flood_count: AtomicU32,
}
impl VxlanStats {
    const fn new() -> Self {
        VxlanStats {
            devices: AtomicU32::new(0),
            encap_ok: AtomicU32::new(0),
            decap_ok: AtomicU32::new(0),
            flood_count: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni VXLAN cihazı oluştur
pub fn create(name: &str, vni: u32, local_ip: Ipv4Addr) -> Result<(), VxlanError> {
    let mut devs = VXLAN_DEVS.lock();
    if devs.contains_key(name) {
        return Err(VxlanError::InvalidFrame);
    }
    if vni == 0 || vni > VXLAN_VNI_MAX {
        return Err(VxlanError::InvalidFrame);
    }
    devs.insert(String::from(name), VxlanDev::new(String::from(name), vni, local_ip));
    VXLAN_STATS.devices.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Cihazı admin-up yap
pub fn set_up(name: &str, up: bool) -> Result<(), VxlanError> {
    let mut devs = VXLAN_DEVS.lock();
    devs.get_mut(name).ok_or(VxlanError::InvalidFrame)?.up = up;
    Ok(())
}

/// Peer VTEP ekle
pub fn add_peer(name: &str, peer_ip: Ipv4Addr) -> Result<(), VxlanError> {
    let mut devs = VXLAN_DEVS.lock();
    let dev = devs.get_mut(name).ok_or(VxlanError::InvalidFrame)?;
    dev.add_peer(peer_ip);
    Ok(())
}

/// Inner frame → UDP paket
pub fn encap(name: &str, eth_frame: &[u8]) -> Result<(Ipv4Addr, Vec<u8>), VxlanError> {
    let mut devs = VXLAN_DEVS.lock();
    let dev = devs.get_mut(name).ok_or(VxlanError::InvalidFrame)?;
    dev.encap(eth_frame)
}

/// UDP payload → inner frame
pub fn decap(name: &str, udp_payload: &[u8]) -> Result<Vec<u8>, VxlanError> {
    let mut devs = VXLAN_DEVS.lock();
    let dev = devs.get_mut(name).ok_or(VxlanError::InvalidFrame)?;
    dev.decap(udp_payload)
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vxlan_header_round_trip() {
        let h = VxlanHeader::new(100);
        let bytes = h.serialize();
        let parsed = VxlanHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.vni, 100);
        assert_eq!(parsed.flags, VXLAN_FLAG_I);
    }

    #[test]
    fn vni_truncated_to_24_bits() {
        let h = VxlanHeader::new(0x01FF_FFFF);
        assert_eq!(h.vni, 0x00FF_FFFF);
    }

    #[test]
    fn vxlan_header_wire_layout() {
        // RFC 7348 §3.1: byte 0=flags, bytes 1..=3=reserved, bytes 4..=6=VNI, byte 7=reserved
        let h = VxlanHeader::new(0x00AB_CDEF);
        let b = h.serialize();
        assert_eq!(b[0], VXLAN_FLAG_I);
        assert_eq!(b[1], 0);
        assert_eq!(b[2], 0);
        assert_eq!(b[3], 0);
        assert_eq!(b[4], 0xAB);
        assert_eq!(b[5], 0xCD);
        assert_eq!(b[6], 0xEF);
        assert_eq!(b[7], 0);
    }

    #[test]
    fn parse_rejects_zero_i_flag() {
        let mut b = [0u8; 8];
        // I=0: bayt 0 bit 3 clear
        b[0] = 0x00;
        b[4] = 1;
        assert!(VxlanHeader::parse(&b).is_none());
    }

    #[test]
    fn parse_rejects_zero_vni() {
        let mut b = [0u8; 8];
        b[0] = VXLAN_FLAG_I;
        // VNI=0
        assert!(VxlanHeader::parse(&b).is_none());
    }

    #[test]
    fn gbp_flag_round_trip() {
        let h = VxlanHeader::new_with_gbp(42);
        let b = h.serialize();
        assert_eq!(b[0], VXLAN_FLAG_I | VXLAN_FLAG_G);
        let p = VxlanHeader::parse(&b).unwrap();
        assert!(p.i_flag_set());
        assert_eq!(p.vni, 42);
    }

    #[test]
    fn encap_decap_round_trip() {
        let local = Ipv4Addr::new(10, 0, 0, 1);
        let mut dev = VxlanDev::new("vxlan100".into(), 100, local);
        dev.up = true;
        dev.add_peer(Ipv4Addr::new(10, 0, 0, 2));
        let frame = vec![0xFF; 6] // DA
            .into_iter()
            .chain(vec![0xAA; 6]) // SA
            .chain(vec![0x08, 0x00]) // EtherType IPv4
            .chain(vec![1, 2, 3, 4]) // payload
            .collect::<Vec<_>>();
        let (dest, udp_pkt) = dev.encap(&frame).unwrap();
        assert_eq!(dest, Ipv4Addr::new(10, 0, 0, 2));
        // Inner: 6+6+2+4 = 18 byte; UDP header 8 + VXLAN 8 + 18 = 34
        assert_eq!(udp_pkt.len(), 8 + 8 + 18);
        // Decap
        let inner = dev.decap(&udp_pkt[8..]).unwrap();
        assert_eq!(inner, frame);
    }

    #[test]
    fn decap_with_wrong_vni_fails() {
        let local = Ipv4Addr::new(10, 0, 0, 1);
        let mut dev = VxlanDev::new("vxlan100".into(), 100, local);
        let hdr = VxlanHeader::new(200); // farklı VNI
        let mut payload = hdr.serialize().to_vec();
        payload.extend_from_slice(&[0xFF; 14]);
        let r = dev.decap(&payload);
        assert_eq!(r, Err(VxlanError::VniMismatch));
    }

    #[test]
    fn encap_to_learned_mac_does_not_flood() {
        let local = Ipv4Addr::new(10, 0, 0, 1);
        let remote = Ipv4Addr::new(10, 0, 0, 2);
        let mut dev = VxlanDev::new("vxlan100".into(), 100, local);
        dev.up = true;
        dev.add_peer(remote);
        let dst_mac = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01];
        dev.learn(dst_mac, remote);
        let mut frame = vec![0u8; 14];
        frame[..6].copy_from_slice(&dst_mac);
        frame[6..12].copy_from_slice(&[0x02; 6]);
        let (dest, _) = dev.encap(&frame).unwrap();
        assert_eq!(dest, remote);
        assert_eq!(dev.flood_count, 0);
    }

    #[test]
    fn vxlan_interface_name_and_mac() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan0".into(), 100, Ipv4Addr::new(10, 0, 0, 1))));
        let iface = VxlanInterface::new(dev);
        assert_eq!(iface.name(), "vxlan0");
        assert_eq!(iface.ip(), Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn vxlan_interface_ip_config() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan1".into(), 200, Ipv4Addr::new(172, 16, 0, 1))));
        let mut iface = VxlanInterface::new(dev);
        iface.set_ip(Ipv4Addr::new(10, 0, 0, 10));
        iface.set_netmask(Ipv4Addr::new(255, 255, 0, 0));
        iface.set_gateway(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(iface.ip(), Ipv4Addr::new(10, 0, 0, 10));
        assert_eq!(iface.netmask(), Ipv4Addr::new(255, 255, 0, 0));
        assert_eq!(iface.gateway(), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn vxlan_interface_up_down() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan2".into(), 300, Ipv4Addr::new(10, 0, 0, 2))));
        let mut iface = VxlanInterface::new(dev);
        assert!(!iface.is_up());
        iface.set_up(true);
        assert!(iface.is_up());
        iface.set_up(false);
        assert!(!iface.is_up());
    }

    #[test]
    fn vxlan_interface_stats_zero() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan3".into(), 400, Ipv4Addr::new(10, 0, 0, 3))));
        let iface = VxlanInterface::new(dev);
        let stats = iface.stats();
        assert_eq!(stats.rx_packets, 0);
        assert_eq!(stats.tx_packets, 0);
    }

    #[test]
    fn vxlan_interface_mtu() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan4".into(), 500, Ipv4Addr::new(10, 0, 0, 4))));
        let iface = VxlanInterface::new(dev);
        assert_eq!(iface.mtu(), 1450);
    }

    #[test]
    fn vxlan_interface_send_down_returns_error() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan5".into(), 600, Ipv4Addr::new(10, 0, 0, 5))));
        let mut iface = VxlanInterface::new(dev);
        assert_eq!(iface.send(&[1u8; 20]), Err(NetError::NotUp));
    }

    #[test]
    fn vxlan_interface_recv_returns_queued() {
        let dev = Arc::new(Mutex::new(VxlanDev::new("vxlan6".into(), 700, Ipv4Addr::new(10, 0, 0, 6))));
        let mut iface = VxlanInterface::new(dev.clone());
        iface.set_up(true);
        dev.lock().rx_queue.push_back(vec![1, 2, 3]);
        assert_eq!(iface.recv(), Some(vec![1, 2, 3]));
    }
}

