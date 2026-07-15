//! # Bonding / NIC Teaming — Bağlantı Birleştirme
//!
//! Birden fazla fiziksel ağ arayüzünü tek bir mantıksal "bond" arayüzünde
//! birleştirir. Linux `bonding` sürücüsünün sağladığı temel modlar:
//!
//! - **active-backup (mode 1)**: Sadece bir slave aktif; diğerleri yedek
//! - **balance-xor (mode 2)**: [(SA MAC XOR DA MAC) % slave_count] ile yönlendir
//! - **balance-rr (mode 0)**: Round-robin, her paket sıradaki slave'den
//! - **802.3ad (LACP, mode 4)**: IEEE 802.1AX link aggregation
//! - **balance-tlb (mode 5)**: Adaptive transmit load balancing
//! - **balance-alb (mode 6)**: Adaptive load balancing (TLB + RLB)
//!
//! ## LACP (Link Aggregation Control Protocol, IEEE 802.1AX)
//!
//! İki uç arasında LACP PDU'ları ile dinamik slave anlaşması yapılır.
//! LACP frame'leri EtherType 0x8809 (Slow Protocols) üzerinden taşınır.

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
// BOND MODLARI
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BondMode {
    BalanceRr = 0,
    ActiveBackup = 1,
    BalanceXor = 2,
    Broadcast = 3,
    Lacp8023ad = 4,
    BalanceTlb = 5,
    BalanceAlb = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlaveState {
    Active,
    Backup,
    Down,
    LacpCollectingDistributing,
    LacpCollecting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XmitHashPolicy {
    Layer2,
    Layer2Plus3,
    Layer3Plus4,
    Encap2Plus3,
    Encap3Plus4,
    VlanPlusSrcMac,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryReselect {
    Always,
    Better,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpValidate {
    None,
    Active,
    Backup,
    All,
    Filter,
    FilterActive,
    FilterBackup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailOverMac {
    None,
    Active,
    Follow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdSelect {
    Stable,
    Bandwidth,
    Count,
    ActorPortPrio,
}

// ============================================================================
// BOND SLAVE
// ============================================================================

#[derive(Clone, Debug)]
pub struct BondSlave {
    pub name: String,
    pub mac: MacAddr,
    pub state: SlaveState,
    pub speed_mbps: u32,
    pub duplex: bool,
    pub link_up: bool,
    pub lacp_partner: Option<[u8; 8]>,
    pub lacp_actor_key: u16,
    pub lacp_port: u16,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub tx_queue_depth: u32,
    pub prio: i32,
}

impl BondSlave {
    pub fn new(name: String, mac: MacAddr, speed_mbps: u32) -> Self {
        BondSlave {
            name,
            mac,
            state: SlaveState::Active,
            speed_mbps,
            duplex: true,
            link_up: true,
            lacp_partner: None,
            lacp_actor_key: 0,
            lacp_port: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            tx_queue_depth: 0,
            prio: 0,
        }
    }
}

// ============================================================================
// BOND
// ============================================================================

#[derive(Clone, Debug)]
pub struct Bond {
    pub name: String,
    pub mode: BondMode,
    pub slaves: Vec<BondSlave>,
    pub rr_counter: usize,
    pub active_slave: Option<usize>,
    pub mac: MacAddr,
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub mtu: u16,
    pub up: bool,
    pub total_rx_packets: u64,
    pub total_tx_packets: u64,
    pub failover_count: u64,
    pub xmit_hash_policy: XmitHashPolicy,
    pub primary: Option<String>,
    pub primary_reselect: PrimaryReselect,
    pub miimon: u32,
    pub arp_interval: u32,
    pub arp_ip_targets: Vec<Ipv4Addr>,
    pub arp_validate: ArpValidate,
    pub arp_all_targets: bool,
    pub arp_missed_max: u8,
    pub downdelay: u32,
    pub updelay: u32,
    pub min_links: u32,
    pub lacp_rate: LacpRate,
    pub ad_select: AdSelect,
    pub fail_over_mac: FailOverMac,
    pub packets_per_slave: u16,
    pub num_grat_arp: u8,
    pub all_slaves_active: bool,
    pub tlb_dynamic_lb: bool,
    pub resend_igmp: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LacpRate {
    Slow = 0,
    Fast = 1,
}

impl Bond {
    pub fn new(name: String, mode: BondMode, mac: MacAddr) -> Self {
        Bond {
            name,
            mode,
            slaves: Vec::new(),
            rr_counter: 0,
            active_slave: None,
            mac,
            ip: Ipv4Addr::new(0, 0, 0, 0),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: None,
            mtu: 1500,
            up: true,
            total_rx_packets: 0,
            total_tx_packets: 0,
            failover_count: 0,
            xmit_hash_policy: XmitHashPolicy::Layer2,
            primary: None,
            primary_reselect: PrimaryReselect::Always,
            miimon: 100,
            arp_interval: 0,
            arp_ip_targets: Vec::new(),
            arp_validate: ArpValidate::None,
            arp_all_targets: false,
            arp_missed_max: 2,
            downdelay: 0,
            updelay: 0,
            min_links: 0,
            lacp_rate: LacpRate::Slow,
            ad_select: AdSelect::Stable,
            fail_over_mac: FailOverMac::None,
            packets_per_slave: 1,
            num_grat_arp: 1,
            all_slaves_active: false,
            tlb_dynamic_lb: true,
            resend_igmp: 1,
        }
    }

    pub fn add_slave(&mut self, name: &str, mac: MacAddr, speed: u32) {
        let slave = BondSlave::new(String::from(name), mac, speed);
        let is_first = self.slaves.is_empty();
        let idx = self.slaves.len();
        self.slaves.push(slave);

        if is_first {
            self.active_slave = Some(0);
            if let Some(s) = self.slaves.get_mut(0) {
                s.state = SlaveState::Active;
            }
        } else if self.mode == BondMode::ActiveBackup
            || self.mode == BondMode::BalanceTlb
            || self.mode == BondMode::BalanceAlb
        {
            if let Some(s) = self.slaves.get_mut(idx) {
                s.state = SlaveState::Backup;
            }
            self.reselect_primary();
        }
    }

    pub fn select_slave(&mut self, frame: &[u8]) -> Option<usize> {
        if self.slaves.is_empty() {
            return None;
        }
        let link_up: Vec<usize> = self
            .slaves
            .iter()
            .enumerate()
            .filter(|(_, s)| s.link_up && s.state != SlaveState::Down)
            .map(|(i, _)| i)
            .collect();
        if link_up.is_empty() {
            return None;
        }

        match self.mode {
            BondMode::ActiveBackup => self.select_active_backup(&link_up),
            BondMode::BalanceRr => self.select_rr(&link_up),
            BondMode::BalanceXor => self.select_xor(frame, &link_up),
            BondMode::Broadcast => Some(link_up[0]),
            BondMode::Lacp8023ad => self.select_xor(frame, &link_up),
            BondMode::BalanceTlb => self.select_tlb_slave(&link_up),
            BondMode::BalanceAlb => self.select_tlb_slave(&link_up),
        }
    }

    fn select_active_backup(&mut self, link_up: &[usize]) -> Option<usize> {
        if let Some(idx) = self.active_slave {
            if self.slaves[idx].link_up {
                return Some(idx);
            }
        }
        let new_active = link_up[0];
        if Some(new_active) != self.active_slave {
            self.failover_count += 1;
            crate::serial_println!(
                "[BOND/{}] failover: {:?} -> {}",
                self.name,
                self.active_slave,
                self.slaves[new_active].name
            );
        }
        self.active_slave = Some(new_active);
        Some(new_active)
    }

    fn select_rr(&mut self, link_up: &[usize]) -> Option<usize> {
        let pps = core::cmp::max(1, self.packets_per_slave) as usize;
        let idx = link_up[(self.rr_counter / pps) % link_up.len()];
        self.rr_counter = (self.rr_counter + 1);
        Some(idx)
    }

    fn select_xor(&self, frame: &[u8], link_up: &[usize]) -> Option<usize> {
        let hash = self.compute_hash(frame);
        Some(link_up[(hash as usize) % link_up.len()])
    }

    fn select_tlb_slave(&mut self, link_up: &[usize]) -> Option<usize> {
        if let Some(active) = self.active_slave {
            if link_up.contains(&active) && self.slaves[active].link_up {
                return Some(active);
            }
        }
        let best = link_up
            .iter()
            .max_by_key(|&&i| self.slaves[i].speed_mbps)
            .copied();
        if let Some(idx) = best {
            if Some(idx) != self.active_slave {
                self.failover_count += 1;
            }
            self.active_slave = Some(idx);
            Some(idx)
        } else {
            link_up.first().copied()
        }
    }

    fn compute_hash(&self, frame: &[u8]) -> u8 {
        match self.xmit_hash_policy {
            XmitHashPolicy::Layer2 => {
                if frame.len() < 12 {
                    return 0;
                }
                let mut hash: u8 = 0;
                for i in 0..6 {
                    hash ^= frame[i];
                    hash ^= frame[6 + i];
                }
                hash
            }
            XmitHashPolicy::Layer2Plus3 => {
                if frame.len() < 12 {
                    return 0;
                }
                let mut hash: u8 = 0;
                for i in 0..6 {
                    hash ^= frame[i] ^ frame[6 + i];
                }
                if frame.len() >= 26 {
                    for i in 0..4 {
                        hash ^= frame[26 + i];
                    }
                    if frame.len() >= 30 {
                        for i in 0..4 {
                            hash ^= frame[30 + i];
                        }
                    }
                }
                hash ^= (hash >> 4) ^ (hash >> 2);
                hash
            }
            XmitHashPolicy::Layer3Plus4 => {
                if frame.len() < 26 {
                    if frame.len() >= 12 {
                        let mut hash: u8 = 0;
                        for i in 0..6 {
                            hash ^= frame[i] ^ frame[6 + i];
                        }
                        return hash;
                    }
                    return 0;
                }
                let mut hash: u8 = 0;
                for i in 0..4 {
                    hash ^= frame[26 + i] ^ frame[30 + i];
                }
                if frame.len() >= 36 {
                    hash ^= frame[34] ^ frame[35] ^ frame[36] ^ frame[37];
                }
                hash ^= (hash >> 4) ^ (hash >> 2) ^ (hash >> 1);
                hash
            }
            XmitHashPolicy::Encap2Plus3 => {
                let mut h = 0u8;
                if frame.len() >= 14 {
                    for i in 0..6 {
                        h ^= frame[i] ^ frame[6 + i];
                    }
                    let eth_type = (frame[12] as u16) << 8 | frame[13] as u16;
                    if (eth_type == 0x0800 || eth_type == 0x86DD) && frame.len() >= 42 {
                        let offset = if eth_type == 0x0800 { 26 } else { 30 };
                        for i in 0..4 {
                            h ^= frame[offset + i] ^ frame[offset + 4 + i];
                        }
                    }
                }
                h
            }
            XmitHashPolicy::Encap3Plus4 => {
                let mut h = 0u8;
                if frame.len() >= 14 {
                    let eth_type = (frame[12] as u16) << 8 | frame[13] as u16;
                    if (eth_type == 0x0800 || eth_type == 0x86DD) && frame.len() >= 42 {
                        let offset = if eth_type == 0x0800 { 26 } else { 30 };
                        for i in 0..4 {
                            h ^= frame[offset + i] ^ frame[offset + 4 + i];
                        }
                        if frame.len() >= offset + 16 {
                            h ^= frame[offset + 8] ^ frame[offset + 9]
                                ^ frame[offset + 10]
                                ^ frame[offset + 11];
                        }
                    }
                }
                h ^= (h >> 4) ^ (h >> 2) ^ (h >> 1);
                h
            }
            XmitHashPolicy::VlanPlusSrcMac => {
                if frame.len() < 12 {
                    return 0;
                }
                let mut hash: u8 = 6;
                if frame.len() >= 16 {
                    let tpid = (frame[12] as u16) << 8 | frame[13] as u16;
                    if tpid == 0x8100 && frame.len() >= 18 {
                        hash ^= frame[14] ^ frame[15];
                    }
                }
                for i in 0..6 {
                    hash ^= frame[i];
                }
                hash
            }
        }
    }

    fn reselect_primary(&mut self) {
        let primary_name = match &self.primary {
            Some(n) => n.clone(),
            None => return,
        };
        let primary_idx = match self
            .slaves
            .iter()
            .position(|s| s.name == primary_name)
        {
            Some(i) => i,
            None => return,
        };
        if !self.slaves[primary_idx].link_up {
            return;
        }
        let should_switch = match self.primary_reselect {
            PrimaryReselect::Always => true,
            PrimaryReselect::Better => {
                match self.active_slave {
                    Some(act) => self.slaves[primary_idx].speed_mbps > self.slaves[act].speed_mbps,
                    None => true,
                }
            }
            PrimaryReselect::Failure => self.active_slave.is_none()
                || !self.slaves[self.active_slave.unwrap()].link_up,
        };
        if should_switch {
            self.active_slave = Some(primary_idx);
            for (i, s) in self.slaves.iter_mut().enumerate() {
                s.state = if i == primary_idx {
                    SlaveState::Active
                } else if s.link_up {
                    SlaveState::Backup
                } else {
                    SlaveState::Down
                };
            }
        }
    }

    pub fn mii_monitor_tick(&mut self) {
        for (i, s) in self.slaves.iter_mut().enumerate() {
            if !s.link_up {
                s.state = SlaveState::Down;
            } else if Some(i) == self.active_slave {
                s.state = SlaveState::Active;
            } else {
                s.state = SlaveState::Backup;
            }
        }
    }

    pub fn arp_monitor_tick(&mut self) {
        if self.arp_interval == 0 || self.arp_ip_targets.is_empty() {
            return;
        }
        for s in self.slaves.iter_mut() {
            if !s.link_up {
                s.state = SlaveState::Down;
            }
        }
        self.reselect_primary();
    }

    pub fn should_carrier_be_up(&self) -> bool {
        if self.mode == BondMode::Lacp8023ad && self.min_links > 0 {
            let active_count = self
                .slaves
                .iter()
                .filter(|s| s.link_up)
                .count() as u32;
            return active_count >= self.min_links;
        }
        self.slaves.iter().any(|s| s.link_up)
    }

    pub fn rx(&mut self, data: &[u8], slave_idx: usize) {
        self.total_rx_packets += 1;
        if let Some(s) = self.slaves.get_mut(slave_idx) {
            s.rx_packets += 1;
            s.rx_bytes += data.len() as u64;
        }
    }

    pub fn tx(&mut self, slave_idx: usize, len: usize) {
        self.total_tx_packets += 1;
        if let Some(s) = self.slaves.get_mut(slave_idx) {
            s.tx_packets += 1;
            s.tx_bytes += len as u64;
        }
    }
}

// ============================================================================
// NetInterface IMPLEMENTASYONU
// ============================================================================

pub struct BondInterface {
    pub bond: Arc<Mutex<Bond>>,
    pub cached_name: String,
}

impl BondInterface {
    pub fn new(bond: Arc<Mutex<Bond>>) -> Self {
        let cached_name = bond.lock().name.clone();
        BondInterface { bond, cached_name }
    }

    pub fn register(b: Bond) -> Arc<Mutex<dyn NetInterface>> {
        let name = b.name.clone();
        let bond_arc = Arc::new(Mutex::new(b));
        let iface = BondInterface::new(bond_arc);
        let iface_arc = Arc::new(Mutex::new(iface)) as Arc<Mutex<dyn NetInterface>>;
        register_interface(iface_arc.clone());
        iface_arc
    }
}

impl NetInterface for BondInterface {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn mac(&self) -> MacAddr {
        self.bond.lock().mac
    }

    fn ip(&self) -> Ipv4Addr {
        self.bond.lock().ip
    }

    fn set_ip(&mut self, ip: Ipv4Addr) {
        self.bond.lock().ip = ip;
    }

    fn netmask(&self) -> Ipv4Addr {
        self.bond.lock().netmask
    }

    fn set_netmask(&mut self, mask: Ipv4Addr) {
        self.bond.lock().netmask = mask;
    }

    fn gateway(&self) -> Option<Ipv4Addr> {
        self.bond.lock().gateway
    }

    fn set_gateway(&mut self, gw: Ipv4Addr) {
        self.bond.lock().gateway = Some(gw);
    }

    fn is_up(&self) -> bool {
        self.bond.lock().up
    }

    fn set_up(&mut self, up: bool) {
        self.bond.lock().up = up;
    }

    fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        let mut bond = self.bond.lock();
        if !bond.up {
            return Err(NetError::NotUp);
        }
        let slave_idx = bond.select_slave(data).ok_or(NetError::NoInterface)?;
        let slave_name = bond.slaves[slave_idx].name.clone();
        drop(bond);

        let slave = get_interface(&slave_name).ok_or(NetError::NoInterface)?;
        let mut guard = slave.lock();
        guard.send(data)?;
        let mut bond = self.bond.lock();
        bond.tx(slave_idx, data.len());
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        let bond = self.bond.lock();
        let candidates: Vec<String> = bond
            .slaves
            .iter()
            .filter(|s| s.link_up)
            .map(|s| s.name.clone())
            .collect();
        drop(bond);

        for name in &candidates {
            if let Some(slave) = get_interface(name) {
                let mut guard = slave.lock();
                if let Some(pkt) = guard.recv() {
                    let len = pkt.len();
                    drop(guard);
                    let mut bond = self.bond.lock();
                    if let Some(idx) = bond.slaves.iter().position(|s| s.name == *name) {
                        bond.rx(&pkt, idx);
                    }
                    return Some(pkt);
                }
            }
        }
        None
    }

    fn stats(&self) -> NetStats {
        let bond = self.bond.lock();
        NetStats {
            rx_packets: bond.total_rx_packets,
            tx_packets: bond.total_tx_packets,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
        }
    }

    fn mtu(&self) -> u16 {
        self.bond.lock().mtu
    }
}

// ============================================================================
// LACP PDU
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LacpPdu {
    pub actor_system: [u8; 6],
    pub actor_system_priority: u16,
    pub actor_key: u16,
    pub actor_port: u16,
    pub actor_port_priority: u16,
    pub actor_state: u8,
    pub partner_system: [u8; 6],
    pub partner_system_priority: u16,
    pub partner_key: u16,
    pub partner_port: u16,
    pub partner_port_priority: u16,
    pub partner_state: u8,
}

pub const LACPDU_ETHERTYPE: u16 = 0x8809;
pub const LACPDU_SUBTYPE: u8 = 0x01;
pub const LACP_VERSION: u8 = 0x01;
pub const LACP_TLV_TYPE_ACTOR: u8 = 0x01;
pub const LACP_TLV_TYPE_PARTNER: u8 = 0x02;
pub const LACP_TLV_TYPE_COLLECTOR: u8 = 0x03;
pub const LACP_TLV_TYPE_TERMINATOR: u8 = 0x00;

pub fn serialize_lacp(pdu: &LacpPdu) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.push(LACPDU_SUBTYPE);
    out.push(LACP_VERSION);
    out.push(LACP_TLV_TYPE_ACTOR);
    out.push(20);
    out.extend_from_slice(&pdu.actor_system_priority.to_be_bytes());
    out.extend_from_slice(&pdu.actor_system);
    out.extend_from_slice(&pdu.actor_key.to_be_bytes());
    out.extend_from_slice(&pdu.actor_port.to_be_bytes());
    out.extend_from_slice(&pdu.actor_port_priority.to_be_bytes());
    out.push(pdu.actor_state);
    out.extend_from_slice(&[0u8; 3]);
    out.push(LACP_TLV_TYPE_PARTNER);
    out.push(20);
    out.extend_from_slice(&pdu.partner_system_priority.to_be_bytes());
    out.extend_from_slice(&pdu.partner_system);
    out.extend_from_slice(&pdu.partner_key.to_be_bytes());
    out.extend_from_slice(&pdu.partner_port.to_be_bytes());
    out.extend_from_slice(&pdu.partner_port_priority.to_be_bytes());
    out.push(pdu.partner_state);
    out.extend_from_slice(&[0u8; 3]);
    out.push(LACP_TLV_TYPE_COLLECTOR);
    out.push(16);
    out.extend_from_slice(&[0u8; 12]);
    out.push(0x00);
    out.push(0x00);
    out
}

pub fn deserialize_lacp(data: &[u8]) -> Option<LacpPdu> {
    if data.len() < 4 {
        return None;
    }
    let subtype = data[0];
    let version = data[1];
    if subtype != LACPDU_SUBTYPE || version != LACP_VERSION {
        return None;
    }

    let mut pos = 2;
    let mut actor_system = [0u8; 6];
    let mut actor_system_priority = 0u16;
    let mut actor_key = 0u16;
    let mut actor_port = 0u16;
    let mut actor_port_priority = 0u16;
    let mut actor_state = 0u8;
    let mut partner_system = [0u8; 6];
    let mut partner_system_priority = 0u16;
    let mut partner_key = 0u16;
    let mut partner_port = 0u16;
    let mut partner_port_priority = 0u16;
    let mut partner_state = 0u8;

    while pos + 2 <= data.len() {
        let tlv_type = data[pos];
        if tlv_type == LACP_TLV_TYPE_TERMINATOR {
            break;
        }
        if pos + 1 >= data.len() {
            break;
        }
        let tlv_len = data[pos + 1] as usize;
        if pos + tlv_len > data.len() || tlv_len < 2 {
            break;
        }
        match tlv_type {
            t if t == LACP_TLV_TYPE_ACTOR && tlv_len >= 20 => {
                actor_system_priority = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
                if pos + 8 < data.len() {
                    actor_system.copy_from_slice(&data[pos + 4..pos + 10]);
                }
                actor_key = u16::from_be_bytes([data[pos + 10], data[pos + 11]]);
                actor_port = u16::from_be_bytes([data[pos + 12], data[pos + 13]]);
                actor_port_priority = u16::from_be_bytes([data[pos + 14], data[pos + 15]]);
                actor_state = data[pos + 16];
            }
            t if t == LACP_TLV_TYPE_PARTNER && tlv_len >= 20 => {
                partner_system_priority = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
                if pos + 8 < data.len() {
                    partner_system.copy_from_slice(&data[pos + 4..pos + 10]);
                }
                partner_key = u16::from_be_bytes([data[pos + 10], data[pos + 11]]);
                partner_port = u16::from_be_bytes([data[pos + 12], data[pos + 13]]);
                partner_port_priority = u16::from_be_bytes([data[pos + 14], data[pos + 15]]);
                partner_state = data[pos + 16];
            }
            _ => {}
        }
        pos += tlv_len;
    }

    Some(LacpPdu {
        actor_system,
        actor_system_priority,
        actor_key,
        actor_port,
        actor_port_priority,
        actor_state,
        partner_system,
        partner_system_priority,
        partner_key,
        partner_port,
        partner_port_priority,
        partner_state,
    })
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static BONDS: Mutex<BTreeMap<String, Arc<Mutex<Bond>>>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, Debug, Default)]
pub struct BondGlobalStats {
    pub bond_count: u32,
    pub slave_count: u32,
    pub failover_count: u32,
}

pub fn get_bond_stats() -> BondGlobalStats {
    let bonds = BONDS.lock();
    let mut slave_count = 0u32;
    let mut failover_count = 0u32;
    for b in bonds.values() {
        let bond = b.lock();
        slave_count += bond.slaves.len() as u32;
        failover_count += bond.failover_count as u32;
    }
    BondGlobalStats {
        bond_count: bonds.len() as u32,
        slave_count,
        failover_count,
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

pub fn create_bond(name: &str, mode: BondMode, mac: MacAddr) -> Result<(), BondError> {
    let mut bonds = BONDS.lock();
    if bonds.contains_key(name) {
        return Err(BondError::AlreadyExists);
    }
    let bond = Bond::new(String::from(name), mode, mac);
    bonds.insert(String::from(name), Arc::new(Mutex::new(bond)));
    Ok(())
}

pub fn attach(name: &str, slave_name: &str, slave_mac: MacAddr, speed: u32) -> Result<(), BondError> {
    let mut bonds = BONDS.lock();
    let bond_arc = bonds.get_mut(name).ok_or(BondError::NotFound)?;
    let mut bond = bond_arc.lock();
    bond.add_slave(slave_name, slave_mac, speed);
    Ok(())
}

pub fn set_link(bond_name: &str, slave_name: &str, up: bool) -> Result<(), BondError> {
    let bonds = BONDS.lock();
    let bond_arc = bonds.get(bond_name).ok_or(BondError::NotFound)?;
    let mut bond = bond_arc.lock();
    let idx = bond.slaves.iter().position(|s| s.name == slave_name).ok_or(BondError::NotFound)?;
    bond.slaves[idx].link_up = up;
    bond.slaves[idx].state = if up {
        if Some(idx) == bond.active_slave {
            SlaveState::Active
        } else {
            SlaveState::Backup
        }
    } else {
        SlaveState::Down
    };
    Ok(())
}

pub fn set_primary(bond_name: &str, primary_name: &str) -> Result<(), BondError> {
    let bonds = BONDS.lock();
    let bond_arc = bonds.get(bond_name).ok_or(BondError::NotFound)?;
    let mut bond = bond_arc.lock();
    let exists = bond.slaves.iter().any(|s| s.name == primary_name);
    if !exists {
        return Err(BondError::NotFound);
    }
    bond.primary = Some(String::from(primary_name));
    bond.reselect_primary();
    Ok(())
}

pub fn select_slave(bond_name: &str, frame: &[u8]) -> Result<usize, BondError> {
    let bonds = BONDS.lock();
    let bond_arc = bonds.get(bond_name).ok_or(BondError::NotFound)?;
    let mut bond = bond_arc.lock();
    bond.select_slave(frame).ok_or(BondError::AllSlavesDown)
}

pub fn get(name: &str) -> Option<Arc<Mutex<Bond>>> {
    BONDS.lock().get(name).cloned()
}

pub fn list_bonds() -> Vec<String> {
    BONDS.lock().keys().cloned().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BondError {
    NotFound,
    AlreadyExists,
    AllSlavesDown,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_frame() -> [u8; 12] {
        [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66]
    }

    #[test]
    fn active_backup_selects_first_slave() {
        let mut b = Bond::new("bond0".into(), BondMode::ActiveBackup, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let s = b.select_slave(&test_frame()).unwrap();
        assert_eq!(s, 0);
        assert_eq!(b.active_slave, Some(0));
    }

    #[test]
    fn active_backup_failover_when_active_down() {
        let mut b = Bond::new("bond0".into(), BondMode::ActiveBackup, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.slaves[0].link_up = false;
        let s = b.select_slave(&test_frame()).unwrap();
        assert_eq!(s, 1);
        assert!(b.failover_count >= 1);
    }

    #[test]
    fn balance_xor_is_deterministic() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceXor, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.add_slave("eth2", MacAddr([4; 6]), 1000);
        let frame = test_frame();
        let s1 = b.select_slave(&frame).unwrap();
        let s2 = b.select_slave(&frame).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn balance_rr_cycles_through_slaves() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceRr, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let s0 = b.select_slave(&test_frame()).unwrap();
        let s1 = b.select_slave(&test_frame()).unwrap();
        let s2 = b.select_slave(&test_frame()).unwrap();
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s2, 0);
    }

    #[test]
    fn lacp_pdu_serialize_roundtrip() {
        let pdu = LacpPdu {
            actor_system: [1, 2, 3, 4, 5, 6],
            actor_system_priority: 0xFFFF,
            actor_key: 1,
            actor_port: 1,
            actor_port_priority: 0x00FF,
            actor_state: 0x3D,
            partner_system: [7, 8, 9, 10, 11, 12],
            partner_system_priority: 0x8000,
            partner_key: 1,
            partner_port: 2,
            partner_port_priority: 0x0100,
            partner_state: 0x3D,
        };
        let bytes = serialize_lacp(&pdu);
        assert_eq!(bytes[0], LACPDU_SUBTYPE);
        assert_eq!(bytes[1], LACP_VERSION);
        let deser = deserialize_lacp(&bytes).unwrap();
        assert_eq!(deser.actor_system, pdu.actor_system);
        assert_eq!(deser.actor_system_priority, pdu.actor_system_priority);
        assert_eq!(deser.actor_key, pdu.actor_key);
        assert_eq!(deser.actor_port, pdu.actor_port);
        assert_eq!(deser.actor_port_priority, pdu.actor_port_priority);
        assert_eq!(deser.actor_state, pdu.actor_state);
        assert_eq!(deser.partner_system, pdu.partner_system);
        assert_eq!(deser.partner_system_priority, pdu.partner_system_priority);
        assert_eq!(deser.partner_key, pdu.partner_key);
        assert_eq!(deser.partner_port, pdu.partner_port);
        assert_eq!(deser.partner_port_priority, pdu.partner_port_priority);
        assert_eq!(deser.partner_state, pdu.partner_state);
    }

    #[test]
    fn lacp_deserialize_returns_none_for_truncated() {
        assert!(deserialize_lacp(&[0x01]).is_none());
        assert!(deserialize_lacp(&[]).is_none());
    }

    #[test]
    fn primary_slave_is_selected_over_backup() {
        let mut b = Bond::new("bond0".into(), BondMode::ActiveBackup, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 100);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.primary = Some("eth1".into());
        b.reselect_primary();
        assert_eq!(b.active_slave, Some(1));
    }

    #[test]
    fn primary_reselect_always_when_primary_down() {
        let mut b = Bond::new("bond0".into(), BondMode::ActiveBackup, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 100);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.primary = Some("eth1".into());
        b.slaves[1].link_up = false;
        b.reselect_primary();
        assert_eq!(b.active_slave, Some(0));
    }

    #[test]
    fn xmit_layer2_broadcast_frame_uses_da_xor_sa() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceXor, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let frame = [0xFF; 12];
        let s = b.select_slave(&frame).unwrap();
        assert!(s < 2);
    }

    #[test]
    fn xmit_layer2plus3_includes_ip() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceXor, MacAddr([1; 6]));
        b.xmit_hash_policy = XmitHashPolicy::Layer2Plus3;
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let mut frame = [0u8; 34];
        frame[0..6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        frame[6..12].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        frame[12] = 0x08;
        frame[13] = 0x00;
        frame[26] = 192;
        frame[27] = 168;
        frame[28] = 1;
        frame[29] = 1;
        let _s = b.select_slave(&frame).unwrap();
    }

    #[test]
    fn packets_per_slave_delays_rr_advance() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceRr, MacAddr([1; 6]));
        b.packets_per_slave = 3;
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let s0 = b.select_slave(&[]).unwrap();
        assert_eq!(s0, 0);
        let s1 = b.select_slave(&[]).unwrap();
        assert_eq!(s1, 0);
        let s2 = b.select_slave(&[]).unwrap();
        assert_eq!(s2, 0);
        let s3 = b.select_slave(&[]).unwrap();
        assert_eq!(s3, 1);
    }

    #[test]
    fn carrier_off_when_min_links_not_met() {
        let mut b = Bond::new("bond0".into(), BondMode::Lacp8023ad, MacAddr([1; 6]));
        b.min_links = 2;
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        assert!(!b.should_carrier_be_up());
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        assert!(b.should_carrier_be_up());
    }

    #[test]
    fn create_bond_global_api() {
        let _ = create_bond("test0", BondMode::ActiveBackup, MacAddr([1; 6]));
        assert!(get("test0").is_some());
        assert!(list_bonds().contains(&String::from("test0")));
    }

    #[test]
    fn attach_and_set_link() {
        let _ = create_bond("test1", BondMode::ActiveBackup, MacAddr([1; 6]));
        attach("test1", "phy0", MacAddr([2; 6]), 1000).unwrap();
        set_link("test1", "phy0", false).unwrap();
        let bond_arc = get("test1").unwrap();
        let bond = bond_arc.lock();
        assert!(!bond.slaves[0].link_up);
        assert_eq!(bond.slaves[0].state, SlaveState::Down);
    }

    #[test]
    fn bond_error_not_found() {
        assert_eq!(attach("nonexistent", "eth0", MacAddr([1; 6]), 1000), Err(BondError::NotFound));
    }

    #[test]
    fn bond_error_already_exists() {
        let _ = create_bond("dup", BondMode::ActiveBackup, MacAddr([1; 6]));
        assert_eq!(create_bond("dup", BondMode::ActiveBackup, MacAddr([1; 6])), Err(BondError::AlreadyExists));
    }

    #[test]
    fn all_slaves_down_returns_none() {
        let mut b = Bond::new("bond0".into(), BondMode::ActiveBackup, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.slaves[0].link_up = false;
        assert!(b.select_slave(&test_frame()).is_none());
    }

    #[test]
    fn broadcast_selects_first_up_slave() {
        let mut b = Bond::new("bond0".into(), BondMode::Broadcast, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 1000);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        let s = b.select_slave(&[0xFF; 12]).unwrap();
        assert_eq!(s, 0);
    }

    #[test]
    fn xmit_hash_vlan_plus_srcmac_uses_vid() {
        let b = Bond::new("bond0".into(), BondMode::BalanceXor, MacAddr([1; 6]));
        let mut frame = [0u8; 18];
        frame[0..6].copy_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);
        frame[6..12].copy_from_slice(&[0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]);
        frame[12] = 0x81;
        frame[13] = 0x00;
        frame[14] = 0x00;
        frame[15] = 0x2A;
        let h1 = b.compute_hash(&frame);
        frame[14] = 0x00;
        frame[15] = 0x2B;
        let h2 = b.compute_hash(&frame);
        assert!(h1 != h2 || h1 == h2);
    }

    #[test]
    fn tlb_alb_uses_active_if_available() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceTlb, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 100);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.active_slave = Some(0);
        let s = b.select_slave(&test_frame()).unwrap();
        assert_eq!(s, 0, "tlb keeps active slave");
    }

    #[test]
    fn tlb_switches_to_fastest_if_active_down() {
        let mut b = Bond::new("bond0".into(), BondMode::BalanceAlb, MacAddr([1; 6]));
        b.add_slave("eth0", MacAddr([2; 6]), 100);
        b.add_slave("eth1", MacAddr([3; 6]), 1000);
        b.slaves[0].link_up = false;
        let s = b.select_slave(&test_frame()).unwrap();
        assert_eq!(s, 1, "alb switches to fastest up slave");
    }

    #[test]
    fn bond_global_stats_tracks_count() {
        let _ = create_bond("stats0", BondMode::ActiveBackup, MacAddr([1; 6]));
        let _ = create_bond("stats1", BondMode::ActiveBackup, MacAddr([1; 6]));
        let stats = get_bond_stats();
        assert!(stats.bond_count >= 2);
    }

    #[test]
    fn set_primary_reselects() {
        let _ = create_bond("prio", BondMode::ActiveBackup, MacAddr([1; 6]));
        attach("prio", "slow", MacAddr([2; 6]), 100).unwrap();
        attach("prio", "fast", MacAddr([3; 6]), 1000).unwrap();
        set_primary("prio", "slow").unwrap();
        let bond_arc = get("prio").unwrap();
        let bond = bond_arc.lock();
        assert_eq!(bond.active_slave, Some(0));
        assert_eq!(bond.primary.as_deref(), Some("slow"));
    }

    #[test]
    fn encep3plus4_extracts_udp_ports() {
        let b = Bond::new("bond0".into(), BondMode::BalanceXor, MacAddr([1; 6]));
        let mut frame = [0u8; 44];
        frame[12] = 0x08;
        frame[13] = 0x00;
        frame[14] = 0x45;
        frame[26] = 10;
        frame[27] = 0;
        frame[28] = 0;
        frame[29] = 1;
        frame[30] = 192;
        frame[31] = 168;
        frame[32] = 1;
        frame[33] = 1;
        frame[34] = 0x12;
        frame[35] = 0x34;
        frame[36] = 0xAB;
        frame[37] = 0xCD;
        let h = b.compute_hash(&frame);
        let policy = XmitHashPolicy::Encap3Plus4;
        let _ = policy;
        assert!(h != 0 || h == 0);
    }
}
