//! # L2 Bridge ve Spanning Tree Protocol (STP / IEEE 802.1D)
//!
//! ## L2 Bridge Nedir?
//!
//! L2 Bridge, iki veya daha fazla Ethernet segmentini birleştirir ve
//! MAC adreslerine bakarak hangi frame'i hangi porta ileteceğine karar
//! verir. Router gibi IP'ye bakmaz; tamamen Layer 2 seviyesinde çalışır.
//!
//! ## Bridge Bileşenleri
//!
//! - **FDB (Forwarding Database)**: MAC adresi → port eşlemesi
//!   (ör. aa:bb:cc:dd:ee:ff → port 2)
//! - **Öğrenme (Learning)**: Gelen frame'in SA'sından port öğrenilir
//! - **Yönlendirme (Forwarding)**: Hedef MAC FDB'de varsa o porta gönderilir
//! - **Flooding**: FDB'de yoksa tüm portlara gönderilir
//! - **Aging**: Eski FDB girdileri 300s sonra silinir
//!
//! ## Spanning Tree Protocol (STP, IEEE 802.1D)
//!
//! Ağ topolojisinde döngüleri (loops) önlemek için kullanılır. Redundant
//! linkler olsa bile aktif topoloji bir ağaçtır.
//!
//! ### BPDU (Bridge Protocol Data Unit)
//!
//! Bridge'ler arasında iki tür BPDU değiş tokuş edilir:
//! - **Configuration BPDU** (Tip 0x00): Root bridge seçimi ve yol maliyeti
//! - **TCN BPDU** (Tip 0x80): Topoloji değişikliği bildirimi
//!
//! ### Root Bridge Seçimi
//!
//! Bridge'ler kendi `Bridge ID` (8 byte: priority 4 byte + MAC 6 byte) ile
//! en küçük Bridge ID'ye sahip bridge'i root seçer. Eşitlikte MAC adresi
//! tie-breaker olur.
//!
//! ### Port Rolleri
//!
//! - **Root Port**: Root bridge'e en kısa yol
//! - **Designated Port**: Segment üzerinde en iyi bridge'in portu
//! - **Blocked/Alternate Port**: Yedek port, frame iletmez
//!
//! ### Port Durumları
//!
//! - **Disabled** → **Blocking** → **Listening** → **Learning** → **Forwarding**
//!
//! ## echOS Tasarımı
//!
//! `Bridge` yapısı: bridge adı, port listesi, FDB, STP durumu.
//! `BridgedFrame(frame, in_port, bridge)` ile frame'ler işlenir.

use super::{
    get_interface, register_interface, Ipv4Addr, MacAddr, NetError, NetInterface, NetStats,
};
use super::Mutex;
use alloc::sync::Arc;
use alloc::vec;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// STP SABİTLERİ (IEEE 802.1D-2004)
// ============================================================================

/// Configuration BPDU tipi
pub const BPDU_TYPE_CONFIG: u8 = 0x00;
/// TCN (Topology Change Notification) BPDU tipi
pub const BPDU_TYPE_TCN: u8 = 0x80;

/// RSTP (Rapid Spanning Tree Protocol) BPDU tipi
pub const BPDU_TYPE_RSTP: u8 = 0x02;

/// Default STP timer değerleri (Linux `IFLA_BR_*` default'larına uygun)
/// USER_HZ = 256 olduğunda saniye = timer / 256
pub const STP_DEFAULT_HELLO_TIME: u16 = 2 * 256;       // 2 saniye
pub const STP_DEFAULT_MAX_AGE: u16 = 20 * 256;         // 20 saniye
pub const STP_DEFAULT_FORWARD_DELAY: u16 = 15 * 256;   // 15 saniye
pub const STP_DEFAULT_AGEING_TIME: u32 = 300;          // 300 saniye (FDB aging)

/// STP port durumları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StpState {
    Disabled,
    Blocking,
    Listening,
    Learning,
    Forwarding,
}

/// STP port rolleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StpRole {
    Disabled,
    Root,
    Designated,
    Alternate,
    Backup,
}

/// STP BPDU yapısı (IEEE 802.1D Configuration BPDU, 35 byte)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bpdu {
    pub protocol_id: u16,    // 0x0000
    pub version: u8,          // 0 (STP), 2 (RSTP)
    pub bpdu_type: u8,        // 0x00 config, 0x80 TCN
    pub flags: u8,
    pub root_id: [u8; 8],     // Root bridge ID (4B priority+ext_sysid + 6B MAC; high 4 bits = priority)
    pub root_path_cost: u32,
    pub bridge_id: [u8; 8],   // Bu BPDU'yu gönderen bridge'in ID'si
    pub port_id: u16,         // 8 bit priority + 8 bit port numarası
    pub message_age: u16,     // 1/256 saniye
    pub max_age: u16,         // 1/256 saniye, default 20s = 5120
    pub hello_time: u16,      // 1/256 saniye, default 2s = 512
    pub forward_delay: u16,   // 1/256 saniye, default 15s = 3840
}

/// BPDU bayrağı bit'leri (802.1D)
pub mod bpdu_flags {
    /// Topology Change: topoloji değişti
    pub const TC: u8 = 0b0000_0001;
    /// Topology Change Acknowledgment: TCA
    pub const TCA: u8 = 0b0000_0010;
}

impl Bpdu {
    /// 35 byte Configuration BPDU'yu parse et
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 35 {
            return None;
        }
        let protocol_id = u16::from_be_bytes([data[0], data[1]]);
        if protocol_id != 0 {
            return None;
        }
        let version = data[2];
        let bpdu_type = data[3];
        let flags = data[4];
        let mut root_id = [0u8; 8];
        root_id.copy_from_slice(&data[5..13]);
        let root_path_cost = u32::from_be_bytes([data[13], data[14], data[15], data[16]]);
        let mut bridge_id = [0u8; 8];
        bridge_id.copy_from_slice(&data[17..25]);
        let port_id = u16::from_be_bytes([data[25], data[26]]);
        let message_age_raw = u16::from_be_bytes([data[27], data[28]]);
        let max_age_raw = u16::from_be_bytes([data[29], data[30]]);
        let hello_time_raw = u16::from_be_bytes([data[31], data[32]]);
        let forward_delay_raw = u16::from_be_bytes([data[33], data[34]]);

        Some(Bpdu {
            protocol_id,
            version,
            bpdu_type,
            flags,
            root_id,
            root_path_cost,
            bridge_id,
            port_id,
            message_age: message_age_raw,
            max_age: max_age_raw,
            hello_time: hello_time_raw,
            forward_delay: forward_delay_raw,
        })
    }

    /// Configuration BPDU'yu serileştir
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(35);
        out.extend_from_slice(&self.protocol_id.to_be_bytes());
        out.push(self.version);
        out.push(self.bpdu_type);
        out.push(self.flags);
        out.extend_from_slice(&self.root_id);
        out.extend_from_slice(&self.root_path_cost.to_be_bytes());
        out.extend_from_slice(&self.bridge_id);
        out.extend_from_slice(&self.port_id.to_be_bytes());
        out.extend_from_slice(&self.message_age.to_be_bytes());
        out.extend_from_slice(&self.max_age.to_be_bytes());
        out.extend_from_slice(&self.hello_time.to_be_bytes());
        out.extend_from_slice(&self.forward_delay.to_be_bytes());
        out
    }
}

/// Bridge ID oluştur: priority (4 bit) + extended system ID (12 bit) + MAC (48 bit) = 64 bit
///
/// IEEE 802.1D §9.2.5: priority field = 4 high bits of first byte; lower 12 bits
/// form the extended system ID (genelde VLAN ID, 0 varsayılan).
///
/// `priority` 0-15 (4 bit), default 8. Fonksiyon kendisi (priority << 12) formatına çevirir.
pub fn make_bridge_id(priority: u16, mac: MacAddr) -> [u8; 8] {
    let priority_field: u16 = (priority & 0x0F) << 12;
    let mut id = [0u8; 8];
    id[0..2].copy_from_slice(&priority_field.to_be_bytes());
    id[2..8].copy_from_slice(&mac.0);
    id
}

/// Bridge ID'nin 4-bit öncelik alanını döndürür (0-15).
pub fn bridge_id_priority(id: &[u8; 8]) -> u16 {
    (u16::from_be_bytes([id[0], id[1]]) >> 12) & 0x0F
}

/// Bridge ID'nin extended system ID alanını döndürür (0-4095, default 0).
pub fn bridge_id_ext_sysid(id: &[u8; 8]) -> u16 {
    u16::from_be_bytes([id[0], id[1]]) & 0x0FFF
}

/// Bridge ID'nin MAC adresini döndürür.
pub fn bridge_id_mac(id: &[u8; 8]) -> MacAddr {
    MacAddr([id[2], id[3], id[4], id[5], id[6], id[7]])
}

/// İki bridge ID'yi karşılaştır: önce 4-bit priority (düşük = iyi), sonra MAC (düşük = iyi).
pub fn bridge_id_less(a: &[u8; 8], b: &[u8; 8]) -> bool {
    let pa = bridge_id_priority(a);
    let pb = bridge_id_priority(b);
    if pa != pb {
        return pa < pb;
    }
    bridge_id_mac(a).0 < bridge_id_mac(b).0
}

/// Port ID: 8 bit port priority (default 0x80 = 128) + 8 bit port numarası
pub fn make_port_id(port_priority: u8, port_number: u8) -> u16 {
    ((port_priority as u16) << 8) | (port_number as u16)
}

pub fn port_id_priority(port_id: u16) -> u8 {
    (port_id >> 8) as u8
}

pub fn port_id_number(port_id: u16) -> u8 {
    (port_id & 0xFF) as u8
}

// ============================================================================
// BRIDGE PORT
// ============================================================================

#[derive(Clone, Debug)]
pub struct BridgePort {
    pub name: String,
    pub stp_state: StpState,
    pub stp_role: StpRole,
    pub designated_root: [u8; 8],
    pub designated_cost: u32,
    pub designated_bridge: [u8; 8],
    pub designated_port: u16,
    pub forward_delay_timer: u32,
    pub message_age_timer: u32,
    pub priority: u8,
    pub cost: u32,
    pub unicast_flood: bool,
    pub broadcast_flood: bool,
    pub multicast_flood: bool,
    pub isolated: bool,
    pub learning: bool,
    pub guard: bool,
    pub protect: bool,
}

impl BridgePort {
    pub fn new(name: String) -> Self {
        BridgePort {
            name,
            stp_state: StpState::Blocking,
            stp_role: StpRole::Disabled,
            designated_root: [0; 8],
            designated_cost: 0,
            designated_bridge: [0; 8],
            designated_port: 0,
            forward_delay_timer: 0,
            message_age_timer: 0,
            priority: 128,
            cost: 4,
            unicast_flood: true,
            broadcast_flood: true,
            multicast_flood: true,
            isolated: false,
            learning: true,
            guard: false,
            protect: false,
        }
    }
}

// ============================================================================
// FDB GİRDİSİ
// ============================================================================

#[derive(Clone, Debug)]
pub struct FdbEntry {
    pub port: String,
    pub last_seen_ticks: u64,
}

#[derive(Clone, Debug, Default)]
pub struct FdbTable {
    entries: BTreeMap<MacAddr, FdbEntry>,
}

impl FdbTable {
    pub fn new() -> Self {
        FdbTable::default()
    }
    pub fn learn(&mut self, mac: MacAddr, port: &str, now: u64) {
        self.entries.insert(
            mac,
            FdbEntry {
                port: String::from(port),
                last_seen_ticks: now,
            },
        );
    }
    pub fn lookup(&self, mac: &MacAddr) -> Option<&FdbEntry> {
        self.entries.get(mac)
    }
    pub fn age(&mut self, now: u64, max_age_ticks: u64) {
        self.entries
            .retain(|_, e| now.saturating_sub(e.last_seen_ticks) < max_age_ticks);
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> Vec<(MacAddr, FdbEntry)> {
        self.entries.iter().map(|(k, v)| (*k, v.clone())).collect()
    }
}

// ============================================================================
// BRIDGE
// ============================================================================

#[derive(Clone, Debug)]
pub struct Bridge {
    pub name: String,
    pub bridge_id: [u8; 8],
    pub priority: u16,
    pub mac: MacAddr,
    pub ports: Vec<BridgePort>,
    pub fdb: FdbTable,
    /// Forwarding database aging süresi (saniye, default 300s = Linux)
    pub aging_time: u32,
    pub root_port: Option<usize>,
    pub hello_time: u16,
    pub forward_delay: u16,
    pub max_age: u16,
    pub rx_frames: u64,
    pub tx_frames: u64,
    pub dropped_frames: u64,
    pub stp_topology_changes: u64,
}

impl Bridge {
    pub fn new(name: String, priority: u16, mac: MacAddr) -> Self {
        let bridge_id = make_bridge_id(priority, mac);
        Bridge {
            name,
            bridge_id,
            priority,
            mac,
            ports: Vec::new(),
            fdb: FdbTable::new(),
            aging_time: STP_DEFAULT_AGEING_TIME,
            root_port: None,
            hello_time: STP_DEFAULT_HELLO_TIME,
            forward_delay: STP_DEFAULT_FORWARD_DELAY,
            max_age: STP_DEFAULT_MAX_AGE,
            rx_frames: 0,
            tx_frames: 0,
            dropped_frames: 0,
            stp_topology_changes: 0,
        }
    }

    pub fn add_port(&mut self, port_name: &str) {
        self.ports.push(BridgePort::new(String::from(port_name)));
    }

    /// Bir frame'i bridge'e sok. Sonuç: (yönlendirilecek port adları)
    ///
    /// `frame` tam Ethernet çerçevesi (DA + SA + ...).
    /// `in_port` frame'in geldiği port.
    pub fn process(&mut self, frame: &[u8], in_port: &str) -> Vec<String> {
        if frame.len() < 14 {
            self.dropped_frames += 1;
            return Vec::new();
        }
        self.rx_frames += 1;
        let dst = MacAddr([frame[0], frame[1], frame[2], frame[3], frame[4], frame[5]]);
        let src = MacAddr([frame[6], frame[7], frame[8], frame[9], frame[10], frame[11]]);
        let now = crate::interrupts::get_ticks();

        let in_idx = self.ports.iter().position(|p| p.name == in_port);
        let in_isolated = in_idx.is_some() && self.ports[in_idx.unwrap()].isolated;

        // 1) Source MAC öğren (if learning enabled on in_port)
        if let Some(idx) = in_idx {
            if self.ports[idx].learning {
                self.fdb.learn(src, in_port, now);
            }
        }

        // 2) Hedef portu belirle
        let mut out_ports: Vec<String> = Vec::new();
        if let Some(entry) = self.fdb.lookup(&dst) {
            if entry.port != in_port {
                if let Some(p) = self.ports.iter().find(|p| p.name == entry.port) {
                    if p.stp_state == StpState::Forwarding && !self.is_port_isolated_from(in_port, p, in_isolated) {
                        out_ports.push(entry.port.clone());
                    }
                }
            }
        } else {
            let is_broadcast = dst.0 == [0xFF; 6];
            for p in &self.ports {
                if p.name == in_port || p.stp_state != StpState::Forwarding {
                    continue;
                }
                if is_broadcast && !p.broadcast_flood {
                    continue;
                }
                if !is_broadcast && !p.unicast_flood {
                    continue;
                }
                if self.is_port_isolated_from(in_port, p, in_isolated) {
                    continue;
                }
                out_ports.push(p.name.clone());
            }
        }
        self.tx_frames += out_ports.len() as u64;
        out_ports
    }

    fn is_port_isolated_from(&self, in_port: &str, p: &BridgePort, in_isolated: bool) -> bool {
        if !in_isolated && !p.isolated {
            return false;
        }
        if in_isolated && p.isolated {
            return true;
        }
        if in_isolated && !p.isolated {
            return false;
        }
        true
    }

    /// BPDU geldi — STP durumunu güncelle
    pub fn handle_bpdu(&mut self, bpdu: &Bpdu, from_port: &str) {
        if bpdu.bpdu_type == BPDU_TYPE_TCN {
            self.stp_topology_changes += 1;
            crate::serial_println!("[BRIDGE/{}] TCN from {}", self.name, from_port);
            return;
        }
        // Configuration BPDU
        if bridge_id_less(&bpdu.root_id, &self.bridge_id) {
            // Gelen BPDU'daki root ID, bizimkinden daha iyi.
            // Bu bridge artık root değil; BPDU'yu gönderen port root port olur.
            // designated_* alanlar BPDU'dan alınır; bridge_id veya priority
            // alanları değişmez (kendi bridge_id'miz sabit kalır).
            for p in self.ports.iter_mut() {
                if p.name == from_port {
                    p.designated_root.copy_from_slice(&bpdu.root_id);
                    p.designated_cost = bpdu.root_path_cost;
                    p.designated_bridge.copy_from_slice(&bpdu.bridge_id);
                    p.designated_port = bpdu.port_id;
                    p.stp_role = StpRole::Root;
                    p.stp_state = StpState::Forwarding;
                } else {
                    p.stp_state = StpState::Blocking;
                }
            }
            self.root_port = self.ports.iter().position(|p| p.name == from_port);
            self.stp_topology_changes += 1;
        }
    }
}

// ============================================================================
// BRIDGE INTERFACE (NetInterface wrapper)
// ============================================================================

pub struct BridgeInterface {
    bridge: Arc<Mutex<Bridge>>,
    cached_name: String,
}

impl BridgeInterface {
    pub fn new(bridge: Arc<Mutex<Bridge>>) -> Self {
        let cached_name = bridge.lock().name.clone();
        BridgeInterface { bridge, cached_name }
    }

    pub fn register(b: Bridge) -> Arc<Mutex<dyn NetInterface>> {
        let name = b.name.clone();
        let bridge_arc = Arc::new(Mutex::new(b));
        let iface = BridgeInterface::new(bridge_arc);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        iface_arc
    }
}

impl NetInterface for BridgeInterface {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn mac(&self) -> MacAddr {
        self.bridge.lock().mac
    }

    fn ip(&self) -> Ipv4Addr {
        Ipv4Addr::new(0, 0, 0, 0)
    }

    fn set_ip(&mut self, _ip: Ipv4Addr) {}

    fn netmask(&self) -> Ipv4Addr {
        Ipv4Addr::new(255, 255, 255, 0)
    }

    fn set_netmask(&mut self, _mask: Ipv4Addr) {}

    fn gateway(&self) -> Option<Ipv4Addr> {
        None
    }

    fn set_gateway(&mut self, _gw: Ipv4Addr) {}

    fn is_up(&self) -> bool {
        self.bridge.lock().ports.iter().any(|p| p.stp_state == StpState::Forwarding)
    }

    fn set_up(&mut self, _up: bool) {}

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        let mut bridge = self.bridge.lock();
        let fwd: Vec<String> = bridge.process(data, "__bridge_send");
        drop(bridge);
        for port_name in &fwd {
            if let Some(slave) = get_interface(port_name) {
                let mut guard = slave.lock();
                let _ = guard.send(data);
            }
        }
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        let bridge = self.bridge.lock();
        let candidates: Vec<String> = bridge
            .ports
            .iter()
            .filter(|p| p.stp_state == StpState::Forwarding)
            .map(|p| p.name.clone())
            .collect();
        drop(bridge);
        for name in &candidates {
            if let Some(slave) = get_interface(name) {
                let mut guard = slave.lock();
                if let Some(pkt) = guard.recv() {
                    return Some(pkt);
                }
            }
        }
        None
    }

    fn stats(&self) -> NetStats {
        let bridge = self.bridge.lock();
        NetStats {
            rx_packets: bridge.rx_frames,
            tx_packets: bridge.tx_frames,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: bridge.dropped_frames,
            tx_dropped: 0,
        }
    }

    fn mtu(&self) -> u16 {
        1500
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static BRIDGES: Mutex<BTreeMap<String, Arc<Mutex<Bridge>>>> = Mutex::new(BTreeMap::new());

static BRIDGE_STATS: BridgeStats = BridgeStats::new();
struct BridgeStats {
    bridges: AtomicU32,
    fdb_entries: AtomicU32,
    bpdus: AtomicU32,
}
impl BridgeStats {
    const fn new() -> Self {
        BridgeStats {
            bridges: AtomicU32::new(0),
            fdb_entries: AtomicU32::new(0),
            bpdus: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni bridge oluştur
pub fn create_bridge(name: &str, priority: u16, mac: MacAddr) -> Result<(), BridgeError> {
    let mut bridges = BRIDGES.lock();
    if bridges.contains_key(name) {
        return Err(BridgeError::AlreadyExists);
    }
    let bridge = Bridge::new(String::from(name), priority, mac);
    bridges.insert(String::from(name), Arc::new(Mutex::new(bridge)));
    BRIDGE_STATS.bridges.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[BRIDGE] created {}", name);
    Ok(())
}

/// Bridge'e port ekle
pub fn add_port(bridge_name: &str, port_name: &str) -> Result<(), BridgeError> {
    let mut bridges = BRIDGES.lock();
    let bridge_arc = bridges.get_mut(bridge_name).ok_or(BridgeError::NotFound)?;
    let mut bridge = bridge_arc.lock();
    bridge.add_port(port_name);
    Ok(())
}

/// Frame'i bridge'e işle
pub fn process_frame(bridge_name: &str, frame: &[u8], in_port: &str) -> Result<Vec<String>, BridgeError> {
    let bridges = BRIDGES.lock();
    let bridge_arc = bridges.get(bridge_name).ok_or(BridgeError::NotFound)?;
    let mut bridge = bridge_arc.lock();
    Ok(bridge.process(frame, in_port))
}

/// BPDU işle
pub fn handle_bpdu(bridge_name: &str, bpdu: &Bpdu, from_port: &str) -> Result<(), BridgeError> {
    let bridges = BRIDGES.lock();
    let bridge_arc = bridges.get(bridge_name).ok_or(BridgeError::NotFound)?;
    let mut bridge = bridge_arc.lock();
    BRIDGE_STATS.bpdus.fetch_add(1, Ordering::Relaxed);
    bridge.handle_bpdu(bpdu, from_port);
    Ok(())
}

/// Bridge istatistiklerini al
pub fn get(bridge_name: &str) -> Option<Arc<Mutex<Bridge>>> {
    BRIDGES.lock().get(bridge_name).cloned()
}

/// Tüm bridge'leri listele
pub fn list_bridges() -> Vec<String> {
    BRIDGES.lock().keys().cloned().collect()
}

/// FDB tablosunu al
pub fn fdb(bridge_name: &str) -> Option<Vec<(MacAddr, String)>> {
    let bridges = BRIDGES.lock();
    let bridge_arc = bridges.get(bridge_name)?;
    let bridge = bridge_arc.lock();
    Some(
        bridge
            .fdb
            .entries()
            .into_iter()
            .map(|(mac, e)| (mac, e.port))
            .collect(),
    )
}

/// Periyodik aging — her saniye çağrılmalı
pub fn age_all() {
    let now = crate::interrupts::get_ticks();
    let bridges = BRIDGES.lock();
    let mut total = 0u32;
    for b in bridges.values() {
        let mut bridge = b.lock();
        let aging_ticks = (bridge.aging_time as u64) * 1000;
        bridge.fdb.age(now, aging_ticks);
        total += bridge.fdb.len() as u32;
    }
    BRIDGE_STATS.fdb_entries.store(total, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    NotFound,
    AlreadyExists,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_id_compares_by_priority_first() {
        let a = make_bridge_id(8, MacAddr([0xFF; 6]));
        let b = make_bridge_id(4, MacAddr([0x00; 6]));
        assert!(bridge_id_less(&b, &a));
    }

    #[test]
    fn bridge_id_mac_breaks_tie() {
        let a = make_bridge_id(8, MacAddr([0x01, 0, 0, 0, 0, 0]));
        let b = make_bridge_id(8, MacAddr([0x02, 0, 0, 0, 0, 0]));
        assert!(bridge_id_less(&a, &b));
    }

    #[test]
    fn bridge_id_priority_field_is_4_bits() {
        let c = make_bridge_id(0x1F, MacAddr([0; 6]));
        assert_eq!(bridge_id_priority(&c), 0x0F);
        let p0 = make_bridge_id(0, MacAddr([0; 6]));
        let p8 = make_bridge_id(8, MacAddr([0; 6]));
        let p15 = make_bridge_id(15, MacAddr([0; 6]));
        assert_eq!(bridge_id_priority(&p0), 0);
        assert_eq!(bridge_id_priority(&p8), 8);
        assert_eq!(bridge_id_priority(&p15), 15);
        assert!(bridge_id_less(&p0, &p8));
        assert!(bridge_id_less(&p8, &p15));
    }

    #[test]
    fn bpdu_parse_round_trip() {
        let bpdu = Bpdu {
            protocol_id: 0,
            version: 0,
            bpdu_type: BPDU_TYPE_CONFIG,
            flags: 0,
            root_id: make_bridge_id(8, MacAddr([1, 2, 3, 4, 5, 6])),
            root_path_cost: 4,
            bridge_id: make_bridge_id(0x9000, MacAddr([7, 8, 9, 10, 11, 12])),
            port_id: 0x8001,
            message_age: 0,
            max_age: 5120,
            hello_time: 512,
            forward_delay: 3840,
        };
        let bytes = bpdu.serialize();
        let parsed = Bpdu::parse(&bytes).expect("must parse");
        assert_eq!(parsed.bpdu_type, BPDU_TYPE_CONFIG);
        assert_eq!(parsed.root_path_cost, 4);
        assert_eq!(parsed.max_age, 5120);
    }

    #[test]
    fn fdb_learn_and_lookup() {
        let mut fdb = FdbTable::new();
        let mac = MacAddr([1, 2, 3, 4, 5, 6]);
        fdb.learn(mac, "eth0", 1000);
        assert_eq!(fdb.len(), 1);
        assert_eq!(fdb.lookup(&mac).unwrap().port, "eth0");
    }

    #[test]
    fn fdb_aging_removes_old_entries() {
        let mut fdb = FdbTable::new();
        let mac = MacAddr([1, 2, 3, 4, 5, 6]);
        fdb.learn(mac, "eth0", 1000);
        fdb.age(2000, 500);
        assert_eq!(fdb.len(), 0);
    }

    #[test]
    fn bridge_floods_unknown_destination() {
        let mut bridge = Bridge::new("br0".into(), 8, MacAddr([1, 2, 3, 4, 5, 6]));
        bridge.add_port("eth0");
        bridge.add_port("eth1");
        for p in bridge.ports.iter_mut() {
            p.stp_state = StpState::Forwarding;
        }
        let frame = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
                    0x08, 0x00];
        let out = bridge.process(&frame, "eth0");
        assert_eq!(out, vec!["eth1"]);
    }

    #[test]
    fn bridge_forwards_known_destination() {
        let mut bridge = Bridge::new("br0".into(), 8, MacAddr([1, 2, 3, 4, 5, 6]));
        bridge.add_port("eth0");
        bridge.add_port("eth1");
        for p in bridge.ports.iter_mut() {
            p.stp_state = StpState::Forwarding;
        }
        let dst = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let src = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut frame = [0u8; 14];
        frame[0..6].copy_from_slice(&dst);
        frame[6..12].copy_from_slice(&src);
        frame[12] = 0x08;
        frame[13] = 0x00;
        bridge.process(&frame, "eth0");
        let frame2 = [dst[0], dst[1], dst[2], dst[3], dst[4], dst[5],
                      0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
                      0x08, 0x00];
        let out = bridge.process(&frame2, "eth1");
        assert_eq!(out, vec!["eth0"]);
    }

    #[test]
    fn bridge_does_not_forward_back_to_in_port() {
        let mut bridge = Bridge::new("br0".into(), 8, MacAddr([1, 2, 3, 4, 5, 6]));
        bridge.add_port("eth0");
        bridge.add_port("eth1");
        for p in bridge.ports.iter_mut() {
            p.stp_state = StpState::Forwarding;
        }
        let dst = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let src = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut frame = [0u8; 14];
        frame[0..6].copy_from_slice(&dst);
        frame[6..12].copy_from_slice(&src);
        frame[12] = 0x08;
        frame[13] = 0x00;
        let out = bridge.process(&frame, "eth0");
        assert!(!out.contains(&String::from("eth0")));
        assert!(out.contains(&String::from("eth1")));
    }

    #[test]
    fn bridge_learning_creates_fdb_entry() {
        let mut bridge = Bridge::new("br0".into(), 8, MacAddr([1, 2, 3, 4, 5, 6]));
        bridge.add_port("eth0");
        bridge.add_port("eth1");
        for p in bridge.ports.iter_mut() {
            p.stp_state = StpState::Forwarding;
        }
        let src_mac = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut frame = [0u8; 14];
        frame[6..12].copy_from_slice(&src_mac);
        frame[12] = 0x08;
        frame[13] = 0x00;
        bridge.process(&frame, "eth1");
        assert!(bridge.fdb.lookup(&MacAddr(src_mac)).is_some());
    }

    #[test]
    fn create_bridge_global_api() {
        let _ = create_bridge("test_br", 8, MacAddr([1, 2, 3, 4, 5, 6]));
        assert!(get("test_br").is_some());
        assert!(list_bridges().contains(&String::from("test_br")));
    }

    #[test]
    fn add_port_to_bridge() {
        let _ = create_bridge("br_port_test", 8, MacAddr([1; 6]));
        add_port("br_port_test", "eth0").unwrap();
        let bridge_arc = get("br_port_test").unwrap();
        let bridge = bridge_arc.lock();
        assert_eq!(bridge.ports.len(), 1);
        assert_eq!(bridge.ports[0].name, "eth0");
    }

    #[test]
    fn bridge_error_not_found() {
        assert_eq!(add_port("nonexistent", "eth0"), Err(BridgeError::NotFound));
    }

    #[test]
    fn bridge_error_already_exists() {
        let _ = create_bridge("dup_br", 8, MacAddr([1; 6]));
        assert_eq!(create_bridge("dup_br", 8, MacAddr([1; 6])), Err(BridgeError::AlreadyExists));
    }

    #[test]
    fn bridge_flood_does_not_include_isolated_ports() {
        let mut bridge = Bridge::new("br0".into(), 8, MacAddr([1, 2, 3, 4, 5, 6]));
        bridge.add_port("eth0");
        bridge.add_port("eth1");
        bridge.add_port("eth2");
        for p in bridge.ports.iter_mut() {
            p.stp_state = StpState::Forwarding;
        }
        bridge.ports[1].isolated = true;
        let frame = [0xFF; 14];
        let out = bridge.process(&frame, "eth0");
        assert!(!out.contains(&String::from("eth0")));
        assert!(!out.contains(&String::from("eth1")));
        assert!(out.contains(&String::from("eth2")));
    }

    #[test]
    fn bpdu_parse_invalid_returns_none() {
        assert!(Bpdu::parse(&[0; 4]).is_none());
        assert!(Bpdu::parse(&[]).is_none());
        let mut bad = [0u8; 35];
        bad[0] = 0x01;
        assert!(Bpdu::parse(&bad).is_none());
    }

    #[test]
    fn fdb_entries_returned_correctly() {
        let mut fdb = FdbTable::new();
        fdb.learn(MacAddr([1; 6]), "eth0", 0);
        fdb.learn(MacAddr([2; 6]), "eth1", 0);
        let entries = fdb.entries();
        assert_eq!(entries.len(), 2);
    }
}

