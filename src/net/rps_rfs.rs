use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

pub const RPS_MAX_CPUS: usize = 64;
pub const RPS_DEFAULT_CPU_MASK: u64 = 0xFFFF;
pub const RPS_FLOW_TABLE_MAX_ENTRIES: u32 = 4096;
pub const RPS_SOCK_FLOW_TABLE_SIZE: u32 = 32768;
pub const RPS_FLOW_HASH_BITS: u32 = 10;
pub const RPS_FLOW_TABLE_TIMEOUT_MS: u64 = 10_000;

pub const RPS_CPU_0: u32 = 0;
pub const RPS_CPU_1: u32 = 1;
pub const RPS_CPU_2: u32 = 2;
pub const RPS_CPU_3: u32 = 3;

pub const RPS_FEATURE_FLOW_DIRECTOR: u32 = 1 << 0;
pub const RPS_FEATURE_SOCK_FLOW: u32 = 1 << 1;
pub const RPS_FEATURE_XPS: u32 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RpsFeatureFlags(pub u32);

impl RpsFeatureFlags {
    pub const FLOW_DIRECTOR: RpsFeatureFlags = RpsFeatureFlags(RPS_FEATURE_FLOW_DIRECTOR);
    pub const SOCK_FLOW: RpsFeatureFlags = RpsFeatureFlags(RPS_FEATURE_SOCK_FLOW);
    pub const XPS: RpsFeatureFlags = RpsFeatureFlags(RPS_FEATURE_XPS);

    pub const fn empty() -> Self {
        RpsFeatureFlags(0)
    }

    pub fn contains(self, other: RpsFeatureFlags) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: RpsFeatureFlags) {
        self.0 |= other.0;
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug)]
pub struct RpsConfig {
    pub cpu_mask: u64,
    pub flow_director: bool,
    pub rps_sock_flow_entries: u32,
    pub indirection_table: Vec<u32>,
    pub hash_key: [u8; 40],
}

impl Default for RpsConfig {
    fn default() -> Self {
        let mut indirection_table = Vec::new();
        for i in 0..128 {
            indirection_table.push(i % RPS_MAX_CPUS as u32);
        }
        RpsConfig {
            cpu_mask: RPS_DEFAULT_CPU_MASK,
            flow_director: false,
            rps_sock_flow_entries: 0,
            indirection_table,
            hash_key: [0; 40],
        }
    }
}

impl RpsConfig {
    pub fn new(cpu_mask: u64) -> Self {
        let mut cfg = RpsConfig::default();
        cfg.cpu_mask = cpu_mask;
        cfg
    }

    pub fn enable_flow_director(&mut self) {
        self.flow_director = true;
    }

    pub fn disable_flow_director(&mut self) {
        self.flow_director = false;
    }

    pub fn set_sock_flow_entries(&mut self, entries: u32) {
        self.rps_sock_flow_entries = core::cmp::min(entries, RPS_SOCK_FLOW_TABLE_SIZE);
    }

    pub fn cpu_count(&self) -> u32 {
        self.cpu_mask.count_ones()
    }

    pub fn is_cpu_enabled(&self, cpu: u32) -> bool {
        cpu < 64 && (self.cpu_mask & (1 << cpu)) != 0
    }

    pub fn enable_cpu(&mut self, cpu: u32) {
        if cpu < 64 {
            self.cpu_mask |= 1 << cpu;
        }
    }

    pub fn disable_cpu(&mut self, cpu: u32) {
        if cpu < 64 {
            self.cpu_mask &= !(1 << cpu);
        }
    }
}

#[derive(Clone, Debug)]
pub struct RpsQueueConfig {
    pub cpu_list: Vec<u32>,
    pub weight: u16,
    pub rps_sock_flow_entries: u32,
}

impl Default for RpsQueueConfig {
    fn default() -> Self {
        RpsQueueConfig {
            cpu_list: Vec::new(),
            weight: 1,
            rps_sock_flow_entries: 0,
        }
    }
}

impl RpsQueueConfig {
    pub fn new(cpu_list: Vec<u32>) -> Self {
        RpsQueueConfig {
            cpu_list,
            weight: 1,
            rps_sock_flow_entries: 0,
        }
    }

    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    pub fn total_weight(&self) -> u32 {
        self.cpu_list.len() as u32 * self.weight as u32
    }

    pub fn select_cpu_weighted(&self, hash: u32) -> u32 {
        if self.cpu_list.is_empty() {
            return RPS_CPU_0;
        }
        let total = self.total_weight();
        if total == 0 {
            return self.cpu_list[0];
        }
        let slot = hash % total;
        let mut accumulated = 0u32;
        for &cpu in &self.cpu_list {
            accumulated += self.weight as u32;
            if slot < accumulated {
                return cpu;
            }
        }
        self.cpu_list[0]
    }
}

#[derive(Clone, Debug)]
pub struct RpsFlowKey {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
}

impl RpsFlowKey {
    pub fn new(src_ip: u32, dst_ip: u32, src_port: u16, dst_port: u16, protocol: u8) -> Self {
        RpsFlowKey {
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            protocol,
        }
    }
}

pub fn rps_compute_hash(key: &RpsFlowKey) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let bytes = key.src_ip.to_ne_bytes();
    for b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    let bytes = key.dst_ip.to_ne_bytes();
    for b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    let sp = key.src_port.to_ne_bytes();
    hash ^= sp[0] as u32;
    hash = hash.wrapping_mul(0x01000193);
    hash ^= sp[1] as u32;
    hash = hash.wrapping_mul(0x01000193);
    let dp = key.dst_port.to_ne_bytes();
    hash ^= dp[0] as u32;
    hash = hash.wrapping_mul(0x01000193);
    hash ^= dp[1] as u32;
    hash = hash.wrapping_mul(0x01000193);
    hash ^= key.protocol as u32;
    hash = hash.wrapping_mul(0x01000193);
    hash
}

pub fn rps_classify_flow(packet: &[u8]) -> u32 {
    if packet.len() < 20 {
        return simple_packet_hash(packet);
    }
    let version_ihl = packet[0];
    let version = (version_ihl >> 4) & 0x0F;
    if version != 4 {
        return simple_packet_hash(packet);
    }
    let ihl = (version_ihl & 0x0F) as usize * 4;
    if packet.len() < ihl + 4 {
        return simple_packet_hash(packet);
    }
    let protocol = packet[9];
    let src_ip = u32::from_be_bytes([packet[12], packet[13], packet[14], packet[15]]);
    let dst_ip = u32::from_be_bytes([packet[16], packet[17], packet[18], packet[19]]);

    let (src_port, dst_port) = if protocol == 6 || protocol == 17 {
        if packet.len() >= ihl + 4 {
            let sp = u16::from_be_bytes([packet[ihl], packet[ihl + 1]]);
            let dp = u16::from_be_bytes([packet[ihl + 2], packet[ihl + 3]]);
            (sp, dp)
        } else {
            (0u16, 0u16)
        }
    } else {
        (0u16, 0u16)
    };

    let key = RpsFlowKey::new(src_ip, dst_ip, src_port, dst_port, protocol);
    rps_compute_hash(&key)
}

fn simple_packet_hash(packet: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for (i, &b) in packet.iter().enumerate() {
        hash ^= (b as u32).wrapping_mul(i as u32 + 1);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

pub fn rps_select_cpu(hash: u32, cpu_mask: u64) -> u32 {
    let num_cpus = cpu_mask.count_ones();
    if num_cpus == 0 {
        return 0;
    }
    let index = hash % num_cpus;
    let mut count = 0u32;
    for i in 0..64u32 {
        if (cpu_mask & (1 << i)) != 0 {
            if count == index {
                return i;
            }
            count += 1;
        }
    }
    0
}

#[derive(Debug)]
pub struct RpsEntryPoint {
    pub cpu: u32,
    pub weight: u16,
    pub packets_forwarded: AtomicU64,
}

impl Clone for RpsEntryPoint {
    fn clone(&self) -> Self {
        RpsEntryPoint {
            cpu: self.cpu,
            weight: self.weight,
            packets_forwarded: AtomicU64::new(self.packets_forwarded.load(Ordering::Relaxed)),
        }
    }
}

impl RpsEntryPoint {
    pub fn new(cpu: u32, weight: u16) -> Self {
        RpsEntryPoint {
            cpu,
            weight,
            packets_forwarded: AtomicU64::new(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RpsFlowTableEntry {
    pub key: RpsFlowKey,
    pub hash: u32,
    pub cpu: u32,
    pub rx_queue_index: u32,
    pub last_jiffies: u64,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct RpsFlowTable {
    pub entries: BTreeMap<u32, RpsFlowTableEntry>,
    pub max_entries: u32,
    pub hash_shift: u32,
}

impl RpsFlowTable {
    pub fn new(max_entries: u32) -> Self {
        let hash_shift = if max_entries <= 256 {
            8
        } else if max_entries <= 1024 {
            10
        } else if max_entries <= 4096 {
            12
        } else {
            16
        };
        RpsFlowTable {
            entries: BTreeMap::new(),
            max_entries,
            hash_shift,
        }
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.max_entries as usize
    }

    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }

    pub fn capacity_remaining(&self) -> u32 {
        self.max_entries.saturating_sub(self.entries.len() as u32)
    }

    pub fn insert_entry(&mut self, entry: RpsFlowTableEntry) -> bool {
        if self.is_full() {
            return false;
        }
        self.entries.insert(entry.hash, entry);
        true
    }

    pub fn remove_entry(&mut self, hash: u32) -> bool {
        self.entries.remove(&hash).is_some()
    }

    pub fn lookup_entry(&self, hash: u32) -> Option<&RpsFlowTableEntry> {
        self.entries.get(&hash)
    }

    pub fn lookup_entry_mut(&mut self, hash: u32) -> Option<&mut RpsFlowTableEntry> {
        self.entries.get_mut(&hash)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

pub fn rps_insert_flow(table: &mut RpsFlowTable, entry: RpsFlowTableEntry) -> Result<(), RpsError> {
    if table.is_full() {
        return Err(RpsError::TableFull);
    }
    table.insert_entry(entry);
    Ok(())
}

pub fn rps_lookup_flow(table: &RpsFlowTable, hash: u32) -> Option<&RpsFlowTableEntry> {
    table.lookup_entry(hash)
}

pub fn rps_remove_flow(table: &mut RpsFlowTable, hash: u32) -> bool {
    table.remove_entry(hash)
}

pub fn rps_expire_flows(table: &mut RpsFlowTable, current_time_ms: u64, timeout_ms: u64) -> usize {
    let mut expired = Vec::new();
    for (&hash, entry) in &table.entries {
        if current_time_ms.saturating_sub(entry.last_jiffies) > timeout_ms {
            expired.push(hash);
        }
    }
    let count = expired.len();
    for hash in expired {
        table.entries.remove(&hash);
    }
    count
}

pub fn rps_flow_table_stats(table: &RpsFlowTable) -> RpsFlowTableStats {
    let mut total_packets = 0u64;
    let mut total_bytes = 0u64;
    let mut cpu_counts = BTreeMap::new();

    for entry in table.entries.values() {
        total_packets += entry.packets;
        total_bytes += entry.bytes;
        *cpu_counts.entry(entry.cpu).or_insert(0u64) += 1;
    }

    RpsFlowTableStats {
        entry_count: table.entries.len() as u32,
        total_packets,
        total_bytes,
        cpu_distribution: cpu_counts,
    }
}

#[derive(Clone, Debug, Default)]
pub struct RpsFlowTableStats {
    pub entry_count: u32,
    pub total_packets: u64,
    pub total_bytes: u64,
    pub cpu_distribution: BTreeMap<u32, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpsError {
    TableFull,
    InvalidCpu,
    InvalidParam,
    NotFound,
}

#[derive(Clone, Debug)]
pub struct RpsCpuStats {
    pub cpu: u32,
    pub packets_received: u64,
    pub packets_forwarded: u64,
    pub packets_dropped: u64,
    pub bytes_received: u64,
    pub ipi_sent: u64,
    pub ipi_failed: u64,
}

impl Default for RpsCpuStats {
    fn default() -> Self {
        RpsCpuStats {
            cpu: 0,
            packets_received: 0,
            packets_forwarded: 0,
            packets_dropped: 0,
            bytes_received: 0,
            ipi_sent: 0,
            ipi_failed: 0,
        }
    }
}

pub struct RpsManager {
    pub config: Mutex<RpsConfig>,
    pub queue_configs: Mutex<BTreeMap<u32, RpsQueueConfig>>,
    pub flow_table: Mutex<RpsFlowTable>,
    pub cpu_stats: Mutex<BTreeMap<u32, RpsCpuStats>>,
    pub sock_flow_table: Mutex<BTreeMap<u32, u32>>,
    pub flow_director_table: Mutex<BTreeMap<u32, FlowDirectorEntry>>,
    pub enabled: AtomicBool,
    pub total_packets: AtomicU64,
    pub total_bytes: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct FlowDirectorEntry {
    pub flow_hash: u32,
    pub cpu: u32,
    pub queue_index: u32,
    pub action: FlowDirectorAction,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowDirectorAction {
    Accept,
    Redirect,
    Drop,
    SteerQueue,
}

impl Default for RpsManager {
    fn default() -> Self {
        RpsManager::new()
    }
}

impl RpsManager {
    pub fn new() -> Self {
        RpsManager {
            config: Mutex::new(RpsConfig::default()),
            queue_configs: Mutex::new(BTreeMap::new()),
            flow_table: Mutex::new(RpsFlowTable::new(RPS_FLOW_TABLE_MAX_ENTRIES)),
            cpu_stats: Mutex::new(BTreeMap::new()),
            sock_flow_table: Mutex::new(BTreeMap::new()),
            flow_director_table: Mutex::new(BTreeMap::new()),
            enabled: AtomicBool::new(true),
            total_packets: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
    }

    pub fn set_cpu_mask(&self, mask: u64) {
        self.config.lock().cpu_mask = mask;
    }

    pub fn get_cpu_mask(&self) -> u64 {
        self.config.lock().cpu_mask
    }

    pub fn add_queue_config(&self, queue_id: u32, config: RpsQueueConfig) {
        self.queue_configs.lock().insert(queue_id, config);
    }

    pub fn remove_queue_config(&self, queue_id: u32) {
        self.queue_configs.lock().remove(&queue_id);
    }

    pub fn get_queue_config(&self, queue_id: u32) -> Option<RpsQueueConfig> {
        self.queue_configs.lock().get(&queue_id).cloned()
    }

    pub fn classify_and_steer(&self, packet: &[u8]) -> u32 {
        if !self.is_enabled() {
            return 0;
        }
        let hash = rps_classify_flow(packet);
        let config = self.config.lock();
        let cpu = rps_select_cpu(hash, config.cpu_mask);
        drop(config);
        self.total_packets.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(packet.len() as u64, Ordering::Relaxed);
        cpu
    }

    pub fn insert_flow(&self, entry: RpsFlowTableEntry) -> Result<(), RpsError> {
        let mut table = self.flow_table.lock();
        rps_insert_flow(&mut table, entry)
    }

    pub fn lookup_flow(&self, hash: u32) -> Option<RpsFlowTableEntry> {
        let table = self.flow_table.lock();
        table.lookup_entry(hash).cloned()
    }

    pub fn remove_flow(&self, hash: u32) -> bool {
        let mut table = self.flow_table.lock();
        rps_remove_flow(&mut table, hash)
    }

    pub fn expire_flows(&self, timeout_ms: u64) -> usize {
        let current_time = self.current_time_ms();
        let mut table = self.flow_table.lock();
        rps_expire_flows(&mut table, current_time, timeout_ms)
    }

    pub fn update_cpu_stats(&self, cpu: u32, rx_bytes: u64) {
        let mut stats = self.cpu_stats.lock();
        let entry = stats.entry(cpu).or_insert_with(|| RpsCpuStats {
            cpu,
            ..RpsCpuStats::default()
        });
        entry.packets_received += 1;
        entry.bytes_received += rx_bytes;
    }

    pub fn get_cpu_stats(&self, cpu: u32) -> Option<RpsCpuStats> {
        self.cpu_stats.lock().get(&cpu).cloned()
    }

    pub fn get_all_cpu_stats(&self) -> Vec<RpsCpuStats> {
        self.cpu_stats.lock().values().cloned().collect()
    }

    pub fn get_flow_table_stats(&self) -> RpsFlowTableStats {
        let table = self.flow_table.lock();
        rps_flow_table_stats(&table)
    }

    pub fn sock_flow_table_insert(&self, hash: u32, cpu: u32) -> Result<(), RpsError> {
        let mut table = self.sock_flow_table.lock();
        table.insert(hash, cpu);
        Ok(())
    }

    pub fn sock_flow_table_lookup(&self, hash: u32) -> Option<u32> {
        self.sock_flow_table.lock().get(&hash).copied()
    }

    pub fn sock_flow_table_remove(&self, hash: u32) -> bool {
        self.sock_flow_table.lock().remove(&hash).is_some()
    }

    pub fn flow_director_insert(&self, entry: FlowDirectorEntry) -> Result<(), RpsError> {
        let mut table = self.flow_director_table.lock();
        table.insert(entry.flow_hash, entry);
        Ok(())
    }

    pub fn flow_director_lookup(&self, hash: u32) -> Option<FlowDirectorEntry> {
        self.flow_director_table.lock().get(&hash).cloned()
    }

    pub fn flow_director_remove(&self, hash: u32) -> bool {
        self.flow_director_table.lock().remove(&hash).is_some()
    }

    pub fn snapshot(&self) -> RpsManagerSnapshot {
        let ord = Ordering::Relaxed;
        RpsManagerSnapshot {
            enabled: self.enabled.load(ord),
            total_packets: self.total_packets.load(ord),
            total_bytes: self.total_bytes.load(ord),
            cpu_mask: self.get_cpu_mask(),
            flow_table_entries: self.flow_table.lock().entry_count(),
            sock_flow_entries: self.sock_flow_table.lock().len() as u32,
            flow_director_entries: self.flow_director_table.lock().len() as u32,
        }
    }

    fn current_time_ms(&self) -> u64 {
        #[cfg(test)]
        {
            static TEST_TIME: AtomicU64 = AtomicU64::new(1000);
            TEST_TIME.fetch_add(1, Ordering::Relaxed)
        }
        #[cfg(not(test))]
        {
            crate::interrupts::get_ticks()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RpsManagerSnapshot {
    pub enabled: bool,
    pub total_packets: u64,
    pub total_bytes: u64,
    pub cpu_mask: u64,
    pub flow_table_entries: u32,
    pub sock_flow_entries: u32,
    pub flow_director_entries: u32,
}

pub fn rps_send_ipi(cpu: u32, packet: &[u8]) -> Result<(), RpsError> {
    if cpu >= RPS_MAX_CPUS as u32 {
        return Err(RpsError::InvalidCpu);
    }
    let _ = simple_packet_hash(packet);
    Ok(())
}

pub static RPS_MANAGER: RpsManager = RpsManager {
    config: Mutex::new(RpsConfig {
        cpu_mask: RPS_DEFAULT_CPU_MASK,
        flow_director: false,
        rps_sock_flow_entries: 0,
        indirection_table: Vec::new(),
        hash_key: [0; 40],
    }),
    queue_configs: Mutex::new(BTreeMap::new()),
    flow_table: Mutex::new(RpsFlowTable {
        entries: BTreeMap::new(),
        max_entries: RPS_FLOW_TABLE_MAX_ENTRIES,
        hash_shift: 12,
    }),
    cpu_stats: Mutex::new(BTreeMap::new()),
    sock_flow_table: Mutex::new(BTreeMap::new()),
    flow_director_table: Mutex::new(BTreeMap::new()),
    enabled: AtomicBool::new(true),
    total_packets: AtomicU64::new(0),
    total_bytes: AtomicU64::new(0),
};

pub fn rps_init() {
    crate::serial_println!("[RPS] Initializing RPS/RFS subsystem");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rps_config_default() {
        let config = RpsConfig::default();
        assert_eq!(config.cpu_mask, RPS_DEFAULT_CPU_MASK);
        assert!(!config.flow_director);
        assert_eq!(config.rps_sock_flow_entries, 0);
        assert_eq!(config.indirection_table.len(), 128);
    }

    #[test]
    fn test_rps_config_new() {
        let config = RpsConfig::new(0x0F);
        assert_eq!(config.cpu_mask, 0x0F);
    }

    #[test]
    fn test_rps_config_enable_disable_cpu() {
        let mut config = RpsConfig::new(0);
        assert!(!config.is_cpu_enabled(0));
        config.enable_cpu(0);
        assert!(config.is_cpu_enabled(0));
        config.disable_cpu(0);
        assert!(!config.is_cpu_enabled(0));
    }

    #[test]
    fn test_rps_config_cpu_count() {
        let config = RpsConfig::new(0x0F);
        assert_eq!(config.cpu_count(), 4);
        let config = RpsConfig::new(0xFF);
        assert_eq!(config.cpu_count(), 8);
    }

    #[test]
    fn test_rps_config_flow_director() {
        let mut config = RpsConfig::default();
        assert!(!config.flow_director);
        config.enable_flow_director();
        assert!(config.flow_director);
        config.disable_flow_director();
        assert!(!config.flow_director);
    }

    #[test]
    fn test_rps_config_sock_flow() {
        let mut config = RpsConfig::default();
        config.set_sock_flow_entries(1000);
        assert_eq!(config.rps_sock_flow_entries, 1000);
        config.set_sock_flow_entries(RPS_SOCK_FLOW_TABLE_SIZE + 1000);
        assert_eq!(config.rps_sock_flow_entries, RPS_SOCK_FLOW_TABLE_SIZE);
    }

    #[test]
    fn test_rps_queue_config() {
        let qconfig = RpsQueueConfig::new(vec![0, 1, 2, 3]).with_weight(2);
        assert_eq!(qconfig.weight, 2);
        assert_eq!(qconfig.total_weight(), 8);
    }

    #[test]
    fn test_rps_queue_config_select_cpu() {
        let qconfig = RpsQueueConfig::new(vec![0, 1, 2, 3]);
        let cpu0 = qconfig.select_cpu_weighted(0);
        assert!(cpu0 < 4);
    }

    #[test]
    fn test_rps_queue_config_empty() {
        let qconfig = RpsQueueConfig::new(Vec::new());
        assert_eq!(qconfig.select_cpu_weighted(0), RPS_CPU_0);
    }

    #[test]
    fn test_rps_flow_key() {
        let key = RpsFlowKey::new(0x0A000001, 0x0A000002, 80, 443, 6);
        assert_eq!(key.src_ip, 0x0A000001);
        assert_eq!(key.dst_ip, 0x0A000002);
        assert_eq!(key.src_port, 80);
        assert_eq!(key.dst_port, 443);
        assert_eq!(key.protocol, 6);
    }

    #[test]
    fn test_rps_compute_hash_deterministic() {
        let key = RpsFlowKey::new(0x0A000001, 0x0A000002, 80, 443, 6);
        let h1 = rps_compute_hash(&key);
        let h2 = rps_compute_hash(&key);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_rps_compute_hash_different_keys() {
        let k1 = RpsFlowKey::new(0x0A000001, 0x0A000002, 80, 443, 6);
        let k2 = RpsFlowKey::new(0x0A000001, 0x0A000002, 80, 444, 6);
        assert_ne!(rps_compute_hash(&k1), rps_compute_hash(&k2));
    }

    #[test]
    fn test_rps_select_cpu() {
        let mask = 0x0F;
        for i in 0..100 {
            let cpu = rps_select_cpu(i, mask);
            assert!(cpu < 4);
        }
    }

    #[test]
    fn test_rps_select_cpu_single() {
        let cpu = rps_select_cpu(12345, 0x01);
        assert_eq!(cpu, 0);
    }

    #[test]
    fn test_rps_select_cpu_zero_mask() {
        let cpu = rps_select_cpu(12345, 0);
        assert_eq!(cpu, 0);
    }

    #[test]
    fn test_rps_classify_flow_short_packet() {
        let packet = [0u8; 5];
        let hash = rps_classify_flow(&packet);
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_rps_classify_flow_ipv4() {
        let mut packet = vec![0u8; 40];
        packet[0] = 0x45;
        packet[9] = 6;
        packet[12] = 10;
        packet[13] = 0;
        packet[14] = 0;
        packet[15] = 1;
        packet[16] = 10;
        packet[17] = 0;
        packet[18] = 0;
        packet[19] = 2;
        packet[20] = 0;
        packet[21] = 80;
        packet[22] = 0x01;
        packet[23] = 0xBB;
        let hash = rps_classify_flow(&packet);
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_rps_classify_flow_consistent() {
        let mut packet = vec![0u8; 40];
        packet[0] = 0x45;
        packet[9] = 17;
        packet[12] = 192;
        packet[13] = 168;
        packet[14] = 1;
        packet[15] = 100;
        packet[16] = 192;
        packet[17] = 168;
        packet[18] = 1;
        packet[19] = 200;
        packet[20] = 1234u16.to_be_bytes()[0];
        packet[21] = 1234u16.to_be_bytes()[1];
        packet[22] = 5678u16.to_be_bytes()[0];
        packet[23] = 5678u16.to_be_bytes()[1];
        let h1 = rps_classify_flow(&packet);
        let h2 = rps_classify_flow(&packet);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_rps_flow_table() {
        let mut table = RpsFlowTable::new(10);
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(1, 2, 80, 443, 6),
            hash: 100,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        assert!(rps_insert_flow(&mut table, entry).is_ok());
        assert!(table.lookup_entry(100).is_some());
        assert_eq!(table.entry_count(), 1);
    }

    #[test]
    fn test_rps_flow_table_full() {
        let mut table = RpsFlowTable::new(2);
        for i in 0..5 {
            let entry = RpsFlowTableEntry {
                key: RpsFlowKey::new(0, 0, 0, 0, 0),
                hash: i,
                cpu: 0,
                rx_queue_index: 0,
                last_jiffies: 0,
                packets: 0,
                bytes: 0,
            };
            let _ = rps_insert_flow(&mut table, entry);
        }
        assert!(table.is_full());
        assert_eq!(table.entry_count(), 2);
    }

    #[test]
    fn test_rps_flow_table_remove() {
        let mut table = RpsFlowTable::new(10);
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(1, 2, 80, 443, 6),
            hash: 100,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        rps_insert_flow(&mut table, entry).unwrap();
        assert!(rps_remove_flow(&mut table, 100));
        assert!(table.lookup_entry(100).is_none());
    }

    #[test]
    fn test_rps_flow_table_expire() {
        let mut table = RpsFlowTable::new(10);
        for i in 0..5 {
            let entry = RpsFlowTableEntry {
                key: RpsFlowKey::new(0, 0, 0, 0, 0),
                hash: i,
                cpu: 0,
                rx_queue_index: 0,
                last_jiffies: 100,
                packets: 0,
                bytes: 0,
            };
            rps_insert_flow(&mut table, entry).unwrap();
        }
        let expired = rps_expire_flows(&mut table, 20000, 5000);
        assert_eq!(expired, 5);
        assert_eq!(table.entry_count(), 0);
    }

    #[test]
    fn test_rps_flow_table_expire_partial() {
        let mut table = RpsFlowTable::new(10);
        let entry1 = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 1,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 100,
            packets: 0,
            bytes: 0,
        };
        let entry2 = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 2,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 20000,
            packets: 0,
            bytes: 0,
        };
        rps_insert_flow(&mut table, entry1).unwrap();
        rps_insert_flow(&mut table, entry2).unwrap();
        let expired = rps_expire_flows(&mut table, 20000, 5000);
        assert_eq!(expired, 1);
        assert_eq!(table.entry_count(), 1);
    }

    #[test]
    fn test_rps_flow_table_stats() {
        let mut table = RpsFlowTable::new(10);
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 1,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 100,
            bytes: 5000,
        };
        rps_insert_flow(&mut table, entry).unwrap();
        let stats = rps_flow_table_stats(&table);
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.total_packets, 100);
        assert_eq!(stats.total_bytes, 5000);
    }

    #[test]
    fn test_rps_flow_table_capacity() {
        let mut table = RpsFlowTable::new(10);
        assert_eq!(table.capacity_remaining(), 10);
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 1,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        rps_insert_flow(&mut table, entry).unwrap();
        assert_eq!(table.capacity_remaining(), 9);
    }

    #[test]
    fn test_rps_manager_new() {
        let manager = RpsManager::new();
        assert!(manager.is_enabled());
        let snap = manager.snapshot();
        assert!(snap.enabled);
        assert_eq!(snap.total_packets, 0);
    }

    #[test]
    fn test_rps_manager_enable_disable() {
        let manager = RpsManager::new();
        manager.disable();
        assert!(!manager.is_enabled());
        manager.enable();
        assert!(manager.is_enabled());
    }

    #[test]
    fn test_rps_manager_cpu_mask() {
        let manager = RpsManager::new();
        manager.set_cpu_mask(0x0F);
        assert_eq!(manager.get_cpu_mask(), 0x0F);
    }

    #[test]
    fn test_rps_manager_classify() {
        let manager = RpsManager::new();
        manager.set_cpu_mask(0x0F);
        let packet = vec![0u8; 40];
        let cpu = manager.classify_and_steer(&packet);
        assert!(cpu < 4);
        let snap = manager.snapshot();
        assert_eq!(snap.total_packets, 1);
    }

    #[test]
    fn test_rps_manager_flow_table() {
        let manager = RpsManager::new();
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(1, 2, 80, 443, 6),
            hash: 100,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        assert!(manager.insert_flow(entry).is_ok());
        assert!(manager.lookup_flow(100).is_some());
        assert!(manager.remove_flow(100));
        assert!(manager.lookup_flow(100).is_none());
    }

    #[test]
    fn test_rps_manager_sock_flow() {
        let manager = RpsManager::new();
        assert!(manager.sock_flow_table_insert(12345, 2).is_ok());
        assert_eq!(manager.sock_flow_table_lookup(12345), Some(2));
        assert!(manager.sock_flow_table_remove(12345));
        assert_eq!(manager.sock_flow_table_lookup(12345), None);
    }

    #[test]
    fn test_rps_manager_flow_director() {
        let manager = RpsManager::new();
        let entry = FlowDirectorEntry {
            flow_hash: 999,
            cpu: 1,
            queue_index: 0,
            action: FlowDirectorAction::Redirect,
            priority: 1,
        };
        assert!(manager.flow_director_insert(entry).is_ok());
        assert!(manager.flow_director_lookup(999).is_some());
        assert!(manager.flow_director_remove(999));
        assert!(manager.flow_director_lookup(999).is_none());
    }

    #[test]
    fn test_rps_manager_queue_config() {
        let manager = RpsManager::new();
        let qconfig = RpsQueueConfig::new(vec![0, 1]);
        manager.add_queue_config(0, qconfig);
        assert!(manager.get_queue_config(0).is_some());
        manager.remove_queue_config(0);
        assert!(manager.get_queue_config(0).is_none());
    }

    #[test]
    fn test_rps_manager_cpu_stats() {
        let manager = RpsManager::new();
        manager.update_cpu_stats(0, 1500);
        manager.update_cpu_stats(0, 500);
        let stats = manager.get_cpu_stats(0).unwrap();
        assert_eq!(stats.packets_received, 2);
        assert_eq!(stats.bytes_received, 2000);
    }

    #[test]
    fn test_rps_manager_all_cpu_stats() {
        let manager = RpsManager::new();
        manager.update_cpu_stats(0, 100);
        manager.update_cpu_stats(1, 200);
        let all = manager.get_all_cpu_stats();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_rps_manager_flow_table_stats() {
        let manager = RpsManager::new();
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 1,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 50,
            bytes: 2500,
        };
        manager.insert_flow(entry).unwrap();
        let stats = manager.get_flow_table_stats();
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.total_packets, 50);
        assert_eq!(stats.total_bytes, 2500);
    }

    #[test]
    fn test_rps_send_ipi() {
        let packet = vec![0u8; 64];
        assert!(rps_send_ipi(0, &packet).is_ok());
        assert!(rps_send_ipi(RPS_MAX_CPUS as u32, &packet).is_err());
    }

    #[test]
    fn test_simple_packet_hash() {
        let p1 = [1, 2, 3, 4, 5, 6, 7, 8];
        let p2 = [8, 7, 6, 5, 4, 3, 2, 1];
        assert_ne!(simple_packet_hash(&p1), simple_packet_hash(&p2));
    }

    #[test]
    fn test_simple_packet_hash_consistent() {
        let p = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(simple_packet_hash(&p), simple_packet_hash(&p));
    }

    #[test]
    fn test_rps_flow_table_clear() {
        let mut table = RpsFlowTable::new(10);
        for i in 0..5 {
            let entry = RpsFlowTableEntry {
                key: RpsFlowKey::new(0, 0, 0, 0, 0),
                hash: i,
                cpu: 0,
                rx_queue_index: 0,
                last_jiffies: 0,
                packets: 0,
                bytes: 0,
            };
            rps_insert_flow(&mut table, entry).unwrap();
        }
        table.clear();
        assert_eq!(table.entry_count(), 0);
    }

    #[test]
    fn test_rps_manager_snapshot() {
        let manager = RpsManager::new();
        manager.set_cpu_mask(0xFF);
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(1, 2, 80, 443, 6),
            hash: 100,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        manager.insert_flow(entry).unwrap();
        manager.classify_and_steer(&[0u8; 40]);
        let snap = manager.snapshot();
        assert!(snap.enabled);
        assert_eq!(snap.total_packets, 1);
        assert_eq!(snap.cpu_mask, 0xFF);
        assert_eq!(snap.flow_table_entries, 1);
    }

    #[test]
    fn test_rps_config_boundary_cpu() {
        let mut config = RpsConfig::new(0);
        config.enable_cpu(63);
        assert!(config.is_cpu_enabled(63));
        config.disable_cpu(63);
        assert!(!config.is_cpu_enabled(63));
        config.enable_cpu(64);
        assert!(!config.is_cpu_enabled(64));
    }

    #[test]
    fn test_rps_queue_config_weighted_distribution() {
        let qconfig = RpsQueueConfig {
            cpu_list: vec![0, 1],
            weight: 3,
            rps_sock_flow_entries: 0,
        };
        assert_eq!(qconfig.total_weight(), 6);
        let mut cpu0_count = 0;
        let mut cpu1_count = 0;
        for hash in 0..600 {
            let cpu = qconfig.select_cpu_weighted(hash);
            match cpu {
                0 => cpu0_count += 1,
                1 => cpu1_count += 1,
                _ => panic!("unexpected cpu"),
            }
        }
        assert_eq!(cpu0_count, 300);
        assert_eq!(cpu1_count, 300);
    }

    #[test]
    fn test_rps_flow_key_all_protocols() {
        for proto in [6, 17, 1, 47, 50] {
            let key = RpsFlowKey::new(0x0A000001, 0x0A000002, 80, 443, proto);
            let hash = rps_compute_hash(&key);
            assert_ne!(hash, 0);
        }
    }

    #[test]
    fn test_rps_flow_table_lookup_miss() {
        let table = RpsFlowTable::new(10);
        assert!(table.lookup_entry(999).is_none());
    }

    #[test]
    fn test_rps_flow_table_remove_nonexistent() {
        let mut table = RpsFlowTable::new(10);
        assert!(!rps_remove_flow(&mut table, 999));
    }

    #[test]
    fn test_rps_manager_expire_flows() {
        let manager = RpsManager::new();
        let entry = RpsFlowTableEntry {
            key: RpsFlowKey::new(0, 0, 0, 0, 0),
            hash: 1,
            cpu: 0,
            rx_queue_index: 0,
            last_jiffies: 0,
            packets: 0,
            bytes: 0,
        };
        manager.insert_flow(entry).unwrap();
        let expired = manager.expire_flows(500);
        assert_eq!(expired, 1);
    }

    #[test]
    fn test_rps_manager_disabled_classify() {
        let manager = RpsManager::new();
        manager.disable();
        let cpu = manager.classify_and_steer(&[0u8; 40]);
        assert_eq!(cpu, 0);
        let snap = manager.snapshot();
        assert_eq!(snap.total_packets, 0);
    }
}
