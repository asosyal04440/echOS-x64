//! # Netfilter/iptables
//!
//! Linux-compatible packet filtering and NAT.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NETFILTER CONSTANTS
// ============================================================================

/// Netfilter hooks
pub const NF_INET_PRE_ROUTING: u32 = 0;
pub const NF_INET_LOCAL_IN: u32 = 1;
pub const NF_INET_FORWARD: u32 = 2;
pub const NF_INET_LOCAL_OUT: u32 = 3;
pub const NF_INET_POST_ROUTING: u32 = 4;

/// Netfilter verdicts
pub const NF_DROP: u32 = 0;
pub const NF_ACCEPT: u32 = 1;
pub const NF_STOLEN: u32 = 2;
pub const NF_QUEUE: u32 = 3;
pub const NF_REPEAT: u32 = 4;
pub const NF_STOP: u32 = 5;

/// Protocol families
pub const NFPROTO_UNSPEC: u32 = 0;
pub const NFPROTO_IPV4: u32 = 2;
pub const NFPROTO_IPV6: u32 = 10;

/// iptables targets
pub const IPT_STANDARD_TARGET: &str = "";
pub const IPT_ACCEPT_TARGET: &str = "ACCEPT";
pub const IPT_DROP_TARGET: &str = "DROP";
pub const IPT_RETURN_TARGET: &str = "RETURN";
pub const IPT_QUEUE_TARGET: &str = "QUEUE";
pub const IPT_REJECT_TARGET: &str = "REJECT";
pub const IPT_LOG_TARGET: &str = "LOG";
pub const IPT_MASQUERADE_TARGET: &str = "MASQUERADE";
pub const IPT_DNAT_TARGET: &str = "DNAT";
pub const IPT_SNAT_TARGET: &str = "SNAT";
pub const IPT_REDIRECT_TARGET: &str = "REDIRECT";

/// Table names
pub const IPTABLES_FILTER_TABLE: &str = "filter";
pub const IPTABLES_NAT_TABLE: &str = "nat";
pub const IPTABLES_MANGLE_TABLE: &str = "mangle";
pub const IPTABLES_RAW_TABLE: &str = "raw";
pub const IPTABLES_SECURITY_TABLE: &str = "security";

// ============================================================================
// IPTABLES ENTRY
// ============================================================================

#[derive(Clone, Debug)]
pub struct IptEntry {
    /// Source IP address
    pub src_ip: u32,
    /// Source mask
    pub src_mask: u32,
    /// Destination IP address
    pub dst_ip: u32,
    /// Destination mask
    pub dst_mask: u32,
    /// Input interface
    pub in_iface: String,
    /// Output interface
    pub out_iface: String,
    /// Protocol
    pub proto: u8,
    /// Source port range
    pub src_ports: (u16, u16),
    /// Destination port range
    pub dst_ports: (u16, u16),
    /// TCP flags
    pub tcp_flags: u8,
    /// Match extensions
    pub matches: Vec<IptMatch>,
    /// Target
    pub target: IptTarget,
    /// Packet count
    pub packet_count: AtomicU64,
    /// Byte count
    pub byte_count: AtomicU64,
}

impl IptEntry {
    pub fn new() -> Self {
        Self {
            src_ip: 0,
            src_mask: 0xFFFFFFFF,
            dst_ip: 0,
            dst_mask: 0xFFFFFFFF,
            in_iface: String::new(),
            out_iface: String::new(),
            proto: 0,
            src_ports: (0, 65535),
            dst_ports: (0, 65535),
            tcp_flags: 0,
            matches: Vec::new(),
            target: IptTarget::accept(),
            packet_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
        }
    }

    /// Check if packet matches this entry
    pub fn matches_packet(&self, pkt: &PacketInfo) -> bool {
        // Check source IP
        if (pkt.src_ip & self.src_mask) != (self.src_ip & self.src_mask) {
            return false;
        }
        
        // Check destination IP
        if (pkt.dst_ip & self.dst_mask) != (self.dst_ip & self.dst_mask) {
            return false;
        }
        
        // Check protocol
        if self.proto != 0 && pkt.proto != self.proto {
            return false;
        }
        
        // Check ports
        if pkt.src_port < self.src_ports.0 || pkt.src_port > self.src_ports.1 {
            return false;
        }
        if pkt.dst_port < self.dst_ports.0 || pkt.dst_port > self.dst_ports.1 {
            return false;
        }
        
        // Check interface
        if !self.in_iface.is_empty() && !pkt.in_iface.starts_with(&self.in_iface) {
            return false;
        }
        if !self.out_iface.is_empty() && !pkt.out_iface.starts_with(&self.out_iface) {
            return false;
        }
        
        true
    }
}

#[derive(Clone, Debug)]
pub struct IptMatch {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct IptTarget {
    pub name: String,
    pub verdict: u32,
    pub data: Vec<u8>,
}

impl IptTarget {
    pub fn accept() -> Self {
        Self { name: String::from("ACCEPT"), verdict: NF_ACCEPT, data: Vec::new() }
    }
    
    pub fn drop() -> Self {
        Self { name: String::from("DROP"), verdict: NF_DROP, data: Vec::new() }
    }
    
    pub fn return_() -> Self {
        Self { name: String::from("RETURN"), verdict: 0xFFFFFFFF, data: Vec::new() }
    }
    
    pub fn masquerade() -> Self {
        Self { name: String::from("MASQUERADE"), verdict: NF_ACCEPT, data: Vec::new() }
    }
    
    pub fn snat(ip: u32, port: u16) -> Self {
        Self { 
            name: String::from("SNAT"), 
            verdict: NF_ACCEPT, 
            data: vec![
                (ip & 0xFF) as u8,
                ((ip >> 8) & 0xFF) as u8,
                ((ip >> 16) & 0xFF) as u8,
                ((ip >> 24) & 0xFF) as u8,
                (port & 0xFF) as u8,
                ((port >> 8) & 0xFF) as u8,
            ]
        }
    }
    
    pub fn dnat(ip: u32, port: u16) -> Self {
        Self { 
            name: String::from("DNAT"), 
            verdict: NF_ACCEPT, 
            data: vec![
                (ip & 0xFF) as u8,
                ((ip >> 8) & 0xFF) as u8,
                ((ip >> 16) & 0xFF) as u8,
                ((ip >> 24) & 0xFF) as u8,
                (port & 0xFF) as u8,
                ((port >> 8) & 0xFF) as u8,
            ]
        }
    }
}

// ============================================================================
// IPTABLES CHAIN
// ============================================================================

#[derive(Clone, Debug)]
pub struct IptChain {
    pub name: String,
    pub hook: u32,
    pub policy: u32,
    pub entries: Vec<IptEntry>,
    pub packet_count: AtomicU64,
    pub byte_count: AtomicU64,
}

impl IptChain {
    pub fn new(name: &str, hook: u32, policy: u32) -> Self {
        Self {
            name: String::from(name),
            hook,
            policy,
            entries: Vec::new(),
            packet_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
        }
    }

    /// Add entry to chain
    pub fn add_entry(&mut self, entry: IptEntry) {
        self.entries.push(entry);
    }

    /// Insert entry at position
    pub fn insert_entry(&mut self, entry: IptEntry, pos: usize) {
        if pos <= self.entries.len() {
            self.entries.insert(pos, entry);
        }
    }

    /// Delete entry at position
    pub fn delete_entry(&mut self, pos: usize) -> Option<IptEntry> {
        if pos < self.entries.len() {
            Some(self.entries.remove(pos))
        } else {
            None
        }
    }

    /// Traverse chain for packet
    pub fn traverse(&self, pkt: &mut PacketInfo) -> u32 {
        for entry in &self.entries {
            if entry.matches_packet(pkt) {
                entry.packet_count.fetch_add(1, Ordering::Relaxed);
                entry.byte_count.fetch_add(pkt.len as u64, Ordering::Relaxed);
                
                // Execute target
                return self.execute_target(&entry.target, pkt);
            }
        }
        
        // Return policy
        self.policy
    }

    /// Execute target action
    fn execute_target(&self, target: &IptTarget, pkt: &mut PacketInfo) -> u32 {
        match target.name.as_str() {
            "ACCEPT" => NF_ACCEPT,
            "DROP" => NF_DROP,
            "RETURN" => 0xFFFFFFFF,
            "MASQUERADE" => {
                // NAT: set source to outgoing interface IP
                pkt.new_src_ip = pkt.out_iface_ip;
                NF_ACCEPT
            }
            "SNAT" => {
                if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0], target.data[1], target.data[2], target.data[3]
                    ]);
                    pkt.new_src_ip = ip;
                }
                NF_ACCEPT
            }
            "DNAT" => {
                if target.data.len() >= 6 {
                    let ip = u32::from_le_bytes([
                        target.data[0], target.data[1], target.data[2], target.data[3]
                    ]);
                    pkt.new_dst_ip = ip;
                }
                NF_ACCEPT
            }
            "REJECT" => {
                // Send ICMP/ICMPv6 unreachable
                NF_DROP
            }
            "LOG" => {
                crate::serial_println!(
                    "[IPTABLES] LOG: {}:{} -> {}:{} proto={}",
                    pkt.src_ip, pkt.src_port,
                    pkt.dst_ip, pkt.dst_port,
                    pkt.proto
                );
                NF_ACCEPT
            }
            _ => target.verdict,
        }
    }
}

// ============================================================================
// IPTABLES TABLE
// ============================================================================

#[derive(Clone, Debug)]
pub struct IptTable {
    pub name: String,
    pub chains: BTreeMap<String, IptChain>,
}

impl IptTable {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            chains: BTreeMap::new(),
        }
    }

    /// Add chain
    pub fn add_chain(&mut self, chain: IptChain) {
        self.chains.insert(chain.name.clone(), chain);
    }

    /// Get chain
    pub fn get_chain(&self, name: &str) -> Option<&IptChain> {
        self.chains.get(name)
    }

    /// Get chain mutable
    pub fn get_chain_mut(&mut self, name: &str) -> Option<&mut IptChain> {
        self.chains.get_mut(name)
    }
}

// ============================================================================
// PACKET INFO
// ============================================================================

#[derive(Clone, Debug)]
pub struct PacketInfo {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
    pub in_iface: String,
    pub out_iface: String,
    pub in_iface_ip: u32,
    pub out_iface_ip: u32,
    pub len: usize,
    pub new_src_ip: u32,
    pub new_dst_ip: u32,
    pub new_src_port: u16,
    pub new_dst_port: u16,
    pub conntrack_state: ConntrackState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConntrackState {
    New,
    Established,
    Related,
    Invalid,
}

// ============================================================================
// NETFILTER MANAGER
// ============================================================================

pub struct NetfilterManager {
    tables: Mutex<BTreeMap<String, IptTable>>,
    enabled: AtomicBool,
    stats: Mutex<NetfilterStats>,
}

#[derive(Clone, Debug, Default)]
pub struct NetfilterStats {
    pub packets_processed: u64,
    pub packets_dropped: u64,
    pub packets_accepted: u64,
    pub nat_count: u64,
}

impl NetfilterManager {
    pub const fn new() -> Self {
        Self {
            tables: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
            stats: Mutex::new(NetfilterStats::default()),
        }
    }

    /// Initialize default tables
    pub fn init(&self) {
        // Filter table
        let mut filter = IptTable::new(IPTABLES_FILTER_TABLE);
        filter.add_chain(IptChain::new("INPUT", NF_INET_LOCAL_IN, NF_ACCEPT));
        filter.add_chain(IptChain::new("FORWARD", NF_INET_FORWARD, NF_DROP));
        filter.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables.lock().insert(String::from(IPTABLES_FILTER_TABLE), filter);
        
        // NAT table
        let mut nat = IptTable::new(IPTABLES_NAT_TABLE);
        nat.add_chain(IptChain::new("PREROUTING", NF_INET_PRE_ROUTING, NF_ACCEPT));
        nat.add_chain(IptChain::new("POSTROUTING", NF_INET_POST_ROUTING, NF_ACCEPT));
        nat.add_chain(IptChain::new("OUTPUT", NF_INET_LOCAL_OUT, NF_ACCEPT));
        self.tables.lock().insert(String::from(IPTABLES_NAT_TABLE), nat);
        
        crate::serial_println!("[NETFILTER] Initialized iptables");
    }

    /// Process packet through hooks
    pub fn process_packet(&self, pkt: &mut PacketInfo, hook: u32) -> u32 {
        if !self.enabled.load(Ordering::SeqCst) {
            return NF_ACCEPT;
        }
        
        let mut stats = self.stats.lock();
        stats.packets_processed += 1;
        
        // Process through filter table
        if let Some(table) = self.tables.lock().get(IPTABLES_FILTER_TABLE) {
            for chain in table.chains.values() {
                if chain.hook == hook {
                    let verdict = chain.traverse(pkt);
                    match verdict {
                        NF_ACCEPT => stats.packets_accepted += 1,
                        NF_DROP => stats.packets_dropped += 1,
                        _ => {}
                    }
                    return verdict;
                }
            }
        }
        
        NF_ACCEPT
    }

    /// Add rule
    pub fn add_rule(&self, table: &str, chain: &str, entry: IptEntry) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl.get_chain_mut(chain).ok_or(NetfilterError::ChainNotFound)?;
        chn.add_entry(entry);
        Ok(())
    }

    /// Delete rule
    pub fn delete_rule(&self, table: &str, chain: &str, pos: usize) -> Result<(), NetfilterError> {
        let mut tables = self.tables.lock();
        let tbl = tables.get_mut(table).ok_or(NetfilterError::TableNotFound)?;
        let chn = tbl.get_chain_mut(chain).ok_or(NetfilterError::ChainNotFound)?;
        chn.delete_entry(pos);
        Ok(())
    }

    /// Get statistics
    pub fn get_stats(&self) -> NetfilterStats {
        self.stats.lock().clone()
    }

    /// Enable/disable
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    pub static ref NETFILTER: NetfilterManager = NetfilterManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetfilterError {
    TableNotFound,
    ChainNotFound,
    InvalidRule,
    PermissionDenied,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    NETFILTER.init();
}
