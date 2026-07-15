//! # VLAN (IEEE 802.1Q) — Sanal Yerel Ağ Etiketleme
//!
//! 802.1Q, Ethernet çerçevelerinin içine 4 baytlık VLAN tag ekleyerek tek
//! bir fiziksel ağ üzerinde birden fazla mantıksal ağ (VLAN) tanımlamayı
//! sağlar.
//!
//! ## 802.1Q Frame Formatı
//!
//! ```text
//! +---------+-----------+------+-----+----+----+----+----+
//! | DA (6B) | SA (6B)   | TPID | TCI | Len| Pay| FCS|    ← 802.1Q tagged
//! +---------+-----------+------+-----+----+----+----+----+
//!                   TPID = 0x8100   TCI: PCP(3)|DEI(1)|VID(12)
//! ```
//!
//! ## TCI (Tag Control Information)
//!
//! - **PCP (3 bit)** : Priority Code Point (0-7). 802.1p QoS için.
//! - **DEI (1 bit)** : Drop Eligible Indicator. Yoğunlukta atılabilir.
//! - **VID (12 bit)**: VLAN ID (0-4095). 0 = sadece tag var, 1 = varsayılan,
//!   4095 = ayrılmış.
//!
//! ## Untagged vs Tagged
//!
//! - **Access port** : Tek VLAN, frame'ler tagsız gelir/gider
//! - **Trunk port** : Birden çok VLAN, frame'ler 802.1Q taglı taşınır
//!
//! ## Q-in-Q (802.1ad) — Stacked VLAN
//!
//! İki VLAN tag iç içe geçer: dış etiket (S-Tag, TPID 0x88A8) hizmet sağlayıcıya,
//! iç etiket (C-Tag, TPID 0x8100) müşteriye ait. ethOS'ta temel 802.1Q
//! desteklenir; Q-in-Q genişletme noktası TPID değiştirilerek sağlanır.
//!
//! ## echOS Tasarımı
//!
//! `VlanTable` global bir BTreeMap: (vlan_id, interface_name) -> VlanDev
//! Her VlanDev kendi MAC/IP/MTU'sunu tutar. Gelen frame'lerdeki tag
//! `VlanTable.lookup(vid)` ile aygıta yönlendirilir.

use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// 802.1Q SABİTLERİ
// ============================================================================

/// 802.1Q TPID (Tag Protocol Identifier)
pub const ETH_P_8021Q: u16 = 0x8100;

/// 802.1ad S-Tag TPID (Provider Bridge)
pub const ETH_P_8021AD: u16 = 0x88A8;

/// Maksimum VLAN sayısı
pub const MAX_VLANS: usize = 4096;

/// VLAN etiket yapısı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VlanTag {
    /// VLAN ID (0-4095)
    pub vid: u16,
    /// Priority Code Point (0-7, 802.1p)
    pub pcp: u8,
    /// Drop Eligible Indicator (0-1)
    pub dei: bool,
}

impl VlanTag {
    pub const fn new(vid: u16, pcp: u8, dei: bool) -> Self {
        VlanTag {
            vid: vid & 0x0FFF,
            pcp: pcp & 0x07,
            dei,
        }
    }

    /// TCI byte'larına serialize (2 byte big-endian)
    /// Format: PCP(3) | DEI(1) | VID(12)
    pub fn to_tci(&self) -> [u8; 2] {
        let tci: u16 = ((self.pcp as u16) << 13) | ((self.dei as u16) << 12) | (self.vid & 0x0FFF);
        tci.to_be_bytes()
    }

    /// TCI byte'larından parse
    pub fn from_tci(tci: [u8; 2]) -> Self {
        let v = u16::from_be_bytes(tci);
        VlanTag {
            pcp: ((v >> 13) & 0x07) as u8,
            dei: ((v >> 12) & 0x01) != 0,
            vid: v & 0x0FFF,
        }
    }

    /// 802.1Q tag ekle (4 byte)
    pub fn serialize(&self) -> [u8; 4] {
        let mut out = [0u8; 4];
        out[0..2].copy_from_slice(&ETH_P_8021Q.to_be_bytes());
        out[2..4].copy_from_slice(&self.to_tci());
        out
    }
}

// ============================================================================
// VLAN AYGIT DURUMU
// ============================================================================

/// Bir VLAN arayüzü (ör. "eth0.100" — ana arayüz eth0, VLAN 100)
#[derive(Clone, Debug)]
pub struct VlanDev {
    /// Arayüz adı (ör. "eth0.100")
    pub name: String,
    /// Ana (parent) arayüz adı (ör. "eth0")
    pub parent: String,
    pub tag: VlanTag,
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub mtu: u16,
    pub up: bool,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_tagged: u64,
    pub rx_untagged: u64,
}

impl VlanDev {
    pub fn new(parent: String, tag: VlanTag, name: String) -> Self {
        VlanDev {
            name,
            parent,
            tag,
            mac: MacAddr([0; 6]),
            ip: Ipv4Addr::UNSPECIFIED,
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            mtu: 1500,
            up: false,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_tagged: 0,
            rx_untagged: 0,
        }
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

/// VLAN tablosu: (parent_name, vid) -> VlanDev
static VLAN_TABLE: Mutex<BTreeMap<(String, u16), VlanDev>> = Mutex::new(BTreeMap::new());

/// VLAN istatistikleri
static VLAN_STATS: VlanStats = VlanStats::new();

struct VlanStats {
    total_devices: AtomicU32,
    tagged_inserts: AtomicU32,
    tagged_drops: AtomicU32,
}

impl VlanStats {
    const fn new() -> Self {
        VlanStats {
            total_devices: AtomicU32::new(0),
            tagged_inserts: AtomicU32::new(0),
            tagged_drops: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni VLAN arayüzü oluştur
///
/// `parent`: ana arayüz adı (ör. "eth0")
/// `vid`: VLAN ID (1-4094)
/// `pcp`: 802.1p priority (varsayılan 0)
pub fn create(parent: &str, vid: u16, pcp: u8) -> Result<String, VlanError> {
    if vid == 0 || vid >= 4095 {
        return Err(VlanError::InvalidVlanId);
    }
    let mut table = VLAN_TABLE.lock();
    if table.len() >= MAX_VLANS {
        return Err(VlanError::TableFull);
    }
    let key = (String::from(parent), vid);
    if table.contains_key(&key) {
        return Err(VlanError::AlreadyExists);
    }
    let name = alloc::format!("{}.{}", parent, vid);
    let tag = VlanTag::new(vid, pcp, false);
    let dev = VlanDev::new(String::from(parent), tag, name.clone());
    table.insert(key, dev);
    VLAN_STATS.total_devices.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[VLAN] created {}", name);
    Ok(name)
}

/// VLAN arayüzünü sil
pub fn destroy(parent: &str, vid: u16) -> Result<(), VlanError> {
    let mut table = VLAN_TABLE.lock();
    let key = (String::from(parent), vid);
    table.remove(&key).ok_or(VlanError::NotFound)?;
    VLAN_STATS.total_devices.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// VLAN arayüzünü admin-up yap
pub fn set_up(parent: &str, vid: u16, up: bool) -> Result<(), VlanError> {
    let mut table = VLAN_TABLE.lock();
    let key = (String::from(parent), vid);
    let dev = table.get_mut(&key).ok_or(VlanError::NotFound)?;
    dev.up = up;
    Ok(())
}

/// VLAN arayüzünü getir
pub fn get(parent: &str, vid: u16) -> Option<VlanDev> {
    let table = VLAN_TABLE.lock();
    let key = (String::from(parent), vid);
    table.get(&key).cloned()
}

/// Belirli bir VLAN'daki tüm arayüzleri listele
pub fn list_by_vid(vid: u16) -> Vec<String> {
    VLAN_TABLE
        .lock()
        .values()
        .filter(|d| d.tag.vid == vid)
        .map(|d| d.name.clone())
        .collect()
}

/// Tüm VLAN'ları listele
pub fn list_all() -> Vec<VlanDev> {
    VLAN_TABLE.lock().values().cloned().collect()
}

/// Gelen bir Ethernet çerçevesini VLAN tablosuna göre ayrıştır
///
/// Eğer frame tagged ise TCI parse edilir, ilgili VlanDev bulunur ve
/// tag stripped hâlde teslim edilir.
pub fn ingress(parent: &str, frame: &[u8]) -> Result<Option<VlanDev>, VlanError> {
    if frame.len() < 14 {
        return Ok(None);
    }
    let tpid = u16::from_be_bytes([frame[12], frame[13]]);
    if tpid != ETH_P_8021Q {
        // Untagged — access port
        let table = VLAN_TABLE.lock();
        let key = (String::from(parent), 1); // PVID 1 (varsayılan VLAN)
        if let Some(dev) = table.get(&key) {
            let mut d = dev.clone();
            d.rx_untagged += 1;
            d.rx_packets += 1;
            d.rx_bytes += frame.len() as u64;
            return Ok(Some(d));
        }
        return Ok(None);
    }
    if frame.len() < 18 {
        return Ok(None);
    }
    let tag = VlanTag::from_tci([frame[14], frame[15]]);
    let table = VLAN_TABLE.lock();
    let key = (String::from(parent), tag.vid);
    let dev = table.get(&key).cloned();
    if let Some(mut d) = dev {
        d.rx_tagged += 1;
        d.rx_packets += 1;
        d.rx_bytes += frame.len() as u64;
        VLAN_STATS.tagged_inserts.fetch_add(1, Ordering::Relaxed);
        return Ok(Some(d));
    }
    VLAN_STATS.tagged_drops.fetch_add(1, Ordering::Relaxed);
    Ok(None)
}

/// VLAN tagged Ethernet frame oluştur (DA + SA + VLAN + EtherType + payload)
///
/// Eğer çıkış tagged ise tam 802.1Q header eklenir; untagged ise sadece
/// EtherType yazılır.
pub fn build_frame(mac_dst: MacAddr, mac_src: MacAddr, tag: Option<VlanTag>, ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(14 + 4 + payload.len());
    frame.extend_from_slice(&mac_dst.0);
    frame.extend_from_slice(&mac_src.0);
    if let Some(t) = tag {
        frame.extend_from_slice(&t.serialize());
    }
    frame.extend_from_slice(&ethertype.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

pub struct VlanInterface {
    parent: String,
    pub vid: u16,
    cached_name: String,
    cached_mac: MacAddr,
    ip: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Option<Ipv4Addr>,
    up: bool,
}

impl VlanInterface {
    pub fn new(parent: &str, vid: u16, name: &str, mac: MacAddr) -> Self {
        VlanInterface {
            parent: parent.into(),
            vid,
            cached_name: name.into(),
            cached_mac: mac,
            ip: Ipv4Addr::UNSPECIFIED,
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            up: false,
        }
    }

    pub fn register(parent: &str, vid: u16, name: &str) -> Result<Arc<Mutex<dyn NetInterface>>, &'static str> {
        let mac = get_interface(parent)
            .map(|p| p.lock().mac())
            .unwrap_or(MacAddr([0; 6]));
        let iface = VlanInterface::new(parent, vid, name, mac);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        // Update VlanDev MAC from parent
        if let Ok(name_str) = create(parent, vid, 0) {
            if let Some(dev) = get(parent, vid) {
                if vid > 0 && vid < 4095 {
                    let mut table = VLAN_TABLE.lock();
                    let key = (String::from(parent), vid);
                    if let Some(entry) = table.get_mut(&key) {
                        entry.mac = mac;
                    }
                }
            }
            let _ = name_str;
        }
        Ok(iface_arc)
    }
}

impl NetInterface for VlanInterface {
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
        let key = (self.parent.clone(), self.vid);
        if let Some(dev) = VLAN_TABLE.lock().get_mut(&key) {
            dev.ip = ip;
        }
    }

    fn netmask(&self) -> Ipv4Addr {
        self.netmask
    }

    fn set_netmask(&mut self, netmask: Ipv4Addr) {
        self.netmask = netmask;
        let key = (self.parent.clone(), self.vid);
        if let Some(dev) = VLAN_TABLE.lock().get_mut(&key) {
            dev.netmask = netmask;
        }
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
        let key = (self.parent.clone(), self.vid);
        if let Some(dev) = VLAN_TABLE.lock().get_mut(&key) {
            dev.up = up;
        }
    }

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.up {
            return Err(NetError::NotUp);
        }
        if data.len() < 14 {
            return Err(NetError::InvalidPacket);
        }
        let parent = get_interface(&self.parent).ok_or(NetError::NoInterface)?;
        let mac_dst = MacAddr([data[0], data[1], data[2], data[3], data[4], data[5]]);
        let mac_src = MacAddr([data[6], data[7], data[8], data[9], data[10], data[11]]);
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        let payload = if data.len() > 14 { &data[14..] } else { &[] };
        let tag = VlanTag::new(self.vid, 0, false);
        let vlan_frame = build_frame(mac_dst, mac_src, Some(tag), ethertype, payload);
        let mut guard = parent.lock();
        let result = guard.send(&vlan_frame);
        if result.is_ok() {
            let key = (self.parent.clone(), self.vid);
            if let Some(dev) = VLAN_TABLE.lock().get_mut(&key) {
                dev.tx_packets += 1;
                dev.tx_bytes += data.len() as u64;
            }
        }
        result
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        if !self.up {
            return None;
        }
        let parent = get_interface(&self.parent)?;
        let mut guard = parent.lock();
        let frame = guard.recv()?;
        if frame.len() < 18 {
            return None;
        }
        let tpid = u16::from_be_bytes([frame[12], frame[13]]);
        if tpid != ETH_P_8021Q {
            return None;
        }
        let tag = VlanTag::from_tci([frame[14], frame[15]]);
        if tag.vid != self.vid {
            return None;
        }
        // Strip VLAN tag: rebuild frame without the 4-byte tag
        let mut stripped = Vec::with_capacity(frame.len() - 4);
        stripped.extend_from_slice(&frame[0..12]);
        stripped.extend_from_slice(&frame[16..]);
        let key = (self.parent.clone(), self.vid);
        if let Some(dev) = VLAN_TABLE.lock().get_mut(&key) {
            dev.rx_packets += 1;
            dev.rx_bytes += frame.len() as u64;
            dev.rx_tagged += 1;
        }
        Some(stripped)
    }

    fn stats(&self) -> NetStats {
        let key = (self.parent.clone(), self.vid);
        if let Some(dev) = VLAN_TABLE.lock().get(&key) {
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
pub enum VlanError {
    InvalidVlanId,
    TableFull,
    AlreadyExists,
    NotFound,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tci_round_trip_preserves_pcp_dei_vid() {
        let tag = VlanTag::new(100, 5, true);
        let tci = tag.to_tci();
        let back = VlanTag::from_tci(tci);
        assert_eq!(back.vid, 100);
        assert_eq!(back.pcp, 5);
        assert_eq!(back.dei, true);
    }

    #[test]
    fn vid_clamped_to_12_bits() {
        let tag = VlanTag::new(0xFFFF, 7, true);
        assert_eq!(tag.vid, 0x0FFF);
        let tag2 = VlanTag::new(0x1234, 8, false);
        assert_eq!(tag2.pcp, 0); // 8 mod 8
    }

    #[test]
    fn serialize_contains_8100_tpid() {
        let tag = VlanTag::new(42, 0, false);
        let bytes = tag.serialize();
        assert_eq!(u16::from_be_bytes([bytes[0], bytes[1]]), 0x8100);
        // VID 42 = 0x02A → 0x00, 0x2A
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0x2A);
    }

    #[test]
    fn build_frame_with_tag_has_8100_at_offset_12() {
        let tag = VlanTag::new(10, 0, false);
        let frame = build_frame(
            MacAddr([0xff; 6]),
            MacAddr([0x01; 6]),
            Some(tag),
            0x0800,
            &[1, 2, 3, 4],
        );
        assert_eq!(frame.len(), 14 + 4 + 4);
        let tpid = u16::from_be_bytes([frame[12], frame[13]]);
        assert_eq!(tpid, 0x8100);
        let ethertype = u16::from_be_bytes([frame[16], frame[17]]);
        assert_eq!(ethertype, 0x0800);
        assert_eq!(&frame[18..22], &[1, 2, 3, 4]);
    }

    #[test]
    fn vlan_interface_create_and_name() {
        let iface = VlanInterface::new("eth0", 100, "eth0.100", MacAddr([0x02, 0, 0, 0, 0, 1]));
        assert_eq!(iface.name(), "eth0.100");
        assert_eq!(iface.mac(), MacAddr([0x02, 0, 0, 0, 0, 1]));
    }

    #[test]
    fn vlan_interface_ip_config() {
        let mut iface = VlanInterface::new("eth0", 200, "eth0.200", MacAddr([0x02, 0, 0, 0, 0, 2]));
        iface.set_ip(Ipv4Addr::new(10, 0, 0, 20));
        iface.set_netmask(Ipv4Addr::new(255, 255, 255, 0));
        iface.set_gateway(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(iface.ip(), Ipv4Addr::new(10, 0, 0, 20));
        assert_eq!(iface.netmask(), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(iface.gateway(), Some(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn vlan_interface_up_down() {
        let mut iface = VlanInterface::new("eth0", 300, "eth0.300", MacAddr([0x02, 0, 0, 0, 0, 3]));
        assert!(!iface.is_up());
        iface.set_up(true);
        assert!(iface.is_up());
        iface.set_up(false);
        assert!(!iface.is_up());
    }

    #[test]
    fn vlan_interface_send_down_returns_error() {
        let mut iface = VlanInterface::new("eth0", 400, "eth0.400", MacAddr([0x02, 0, 0, 0, 0, 4]));
        iface.set_up(false);
        let frame = [0u8; 20];
        assert_eq!(iface.send(&frame), Err(NetError::NotUp));
    }

    #[test]
    fn vlan_interface_short_send_returns_error() {
        let mut iface = VlanInterface::new("eth0", 500, "eth0.500", MacAddr([0x02, 0, 0, 0, 0, 5]));
        iface.set_up(true);
        let frame = [0u8; 5];
        assert_eq!(iface.send(&frame), Err(NetError::InvalidPacket));
    }

    #[test]
    fn vlan_interface_stats_default_zero() {
        let iface = VlanInterface::new("eth0", 600, "eth0.600", MacAddr([0x02, 0, 0, 0, 0, 6]));
        let stats = iface.stats();
        assert_eq!(stats.rx_packets, 0);
        assert_eq!(stats.tx_packets, 0);
    }

    #[test]
    fn vlan_interface_mtu_default() {
        let iface = VlanInterface::new("eth0", 700, "eth0.700", MacAddr([0x02, 0, 0, 0, 0, 7]));
        assert_eq!(iface.mtu(), 1500);
    }
}
