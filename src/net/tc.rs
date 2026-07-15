//! # Traffic Control (tc) — Queueing Discipline Framework
//!
//! Linux `tc` komutunun eşdeğeri. Paketleri kuyruklama, şekillendirme ve
//! sıralama disiplinleri (qdisc) ile yönetir.
//!
//! ## Neden qdisc gerekli?
//!
//! Tek bantli FIFO kuyrukta paketler yalnız geliş sırasına göre gönderilir. Bu:
//! - Bufferbloat'a neden olur (çok paket birikir, latency artar)
//! - QoS verilemez (tüm paketler eşit öncelikli)
//! - Shaping yapılamaz (bandwidth garanti verilemez)
//!
//! ## Desteklenen qdisc'ler
//!
//! | qdisc      | Tür          | Açıklama                            |
//! |------------|--------------|--------------------------------------|
//! | pfifo_fast | Classless    | Varsayılan 3-band priority FIFO     |
//! | fq_codel   | Classless    | Flow-based fair queueing + CoDel AQM|
//!
//! Kaynak: Linux kernel tc subsystem, fq_codel paper (K. Nichols, V. Jacobson)

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// QDISC TYPES
// ============================================================================

/// Qdisc türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QdiscKind {
    /// pfifo_fast — 3-band priority FIFO (Linux varsayılan)
    PfifoFast,
    /// fq_codel — Flow-based fair queueing + CoDel AQM
    FqCodel,
    /// noop — paket at (loopback gibi)
    Noop,
}

// ============================================================================
// PACKET (Qdisc'e giren paket)
// ============================================================================

/// Qdisc'e enqueue edilen paket
#[derive(Clone, Debug)]
pub struct TcPacket {
    /// Paket verisi
    pub data: Vec<u8>,
    
    /// Öncelik (0 = en yüksek, 7 = en düşük)
    /// IPv4 TOS field'dan türetilir
    pub priority: u8,
    
    /// Flow hash (5-tuple hash, fq_codel için)
    pub flow_hash: u32,
    
    /// Enqueue zamanı (TSC ticks veya monotonic clock)
    pub enqueue_time: u64,
    
    /// Paket boyutu (bytes)
    pub len: u32,
}

impl TcPacket {
    pub fn new(data: Vec<u8>, priority: u8, flow_hash: u32, timestamp: u64) -> Self {
        let len = data.len() as u32;
        TcPacket {
            data,
            priority,
            flow_hash,
            enqueue_time: timestamp,
            len,
        }
    }
    
    /// IPv4 TOS/DSCP field'dan öncelik çıkar
    pub fn priority_from_tos(tos: u8) -> u8 {
        // Linux pfifo_fast: TOS bit 1-4 → band 0-2
        let dscp = tos >> 2;
        match dscp {
            0..=7 => 1,     // Normal → band 1
            8..=15 => 0,    // High priority → band 0
            16..=31 => 1,   // Normal
            32..=47 => 0,   // EF (Expedited Forwarding) → band 0
            48..=63 => 2,   // Low priority → band 2
            _ => 1,
        }
    }
}

// ============================================================================
// QDISC STATISTICS
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct QdiscStats {
    pub enqueue_count: u64,
    pub dequeue_count: u64,
    pub drop_count: u64,
    pub requeue_count: u64,
    pub backlog_bytes: u64,
    pub backlog_packets: u64,
    pub overlimits: u64,
}

// ============================================================================
// PFIFO_FAST (3-band Priority FIFO)
// ============================================================================

/// pfifo_fast — Linux'un varsayılan qdisc'i
///
/// 3 priority band:
/// - Band 0: Yüksek öncelik (interactive, EF)
/// - Band 1: Normal (bulk, BE)
/// - Band 2: Düşük öncelik (scavenger)
///
/// Dequeue her zaman en düşük numaralı boş-olmayan band'tan yapılır.
pub struct PfifoFast {
    bands: [VecDeque<TcPacket>; 3],
    limit: usize,
    priomap: [u8; 16],
    stats: QdiscStats,
}

impl PfifoFast {
    pub fn new(limit: usize) -> Self {
        PfifoFast {
            bands: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            limit,
            priomap: [1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
            stats: QdiscStats::default(),
        }
    }
    
    pub fn enqueue(&mut self, pkt: TcPacket) -> Result<(), TcPacket> {
        let total_len: usize = self.bands.iter().map(|b| b.len()).sum();
        if total_len >= self.limit {
            self.stats.drop_count += 1;
            return Err(pkt);
        }
        
        let band = if pkt.data.len() >= 15 {
            let tos = pkt.data[14 + 1];
            let priomap_idx = ((tos >> 4) & 0x0F) as usize;
            self.priomap[priomap_idx].min(2) as usize
        } else {
            (pkt.priority as usize).min(2)
        };
        self.stats.backlog_bytes += pkt.len as u64;
        self.stats.backlog_packets += 1;
        self.stats.enqueue_count += 1;
        self.bands[band].push_back(pkt);
        Ok(())
    }
    
    pub fn dequeue(&mut self) -> Option<TcPacket> {
        for band in &mut self.bands {
            if let Some(pkt) = band.pop_front() {
                self.stats.backlog_bytes -= pkt.len as u64;
                self.stats.backlog_packets -= 1;
                self.stats.dequeue_count += 1;
                return Some(pkt);
            }
        }
        None
    }
    
    pub fn len(&self) -> usize {
        self.bands.iter().map(|b| b.len()).sum()
    }
    
    pub fn is_empty(&self) -> bool {
        self.bands.iter().all(|b| b.is_empty())
    }
    
    pub fn stats(&self) -> &QdiscStats {
        &self.stats
    }
}

// ============================================================================
// FQ_CODEL (Fair Queueing + Controlled Delay)
// ============================================================================

/// CoDel parametreleri
const CODEL_TARGET: u64 = 5_000;       // 5ms target latency (microseconds)
const CODEL_INTERVAL: u64 = 100_000;   // 100ms interval (microseconds)
const FQ_CODEL_FLOWS: usize = 1024;    // Flow sayısı (hash bucket)
const FQ_CODEL_LIMIT: usize = 10240;   // Maksimum toplam paket

/// Tek bir flow'un kuyruğu
struct FqCodelFlow {
    queue: VecDeque<TcPacket>,
    deficit: i32,
    dropping: bool,
    drop_next: u64,
    first_above_time: u64,
    count: u32,
    last_count: u32,
    in_new_flows: bool,
    in_old_flows: bool,
}

impl FqCodelFlow {
    fn new() -> Self {
        FqCodelFlow {
            queue: VecDeque::new(),
            deficit: 0,
            dropping: false,
            drop_next: 0,
            first_above_time: 0,
            count: 0,
            last_count: 0,
            in_new_flows: false,
            in_old_flows: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlowListKind {
    New,
    Old,
}

/// fq_codel — Flow-based fair queueing with CoDel AQM
///
/// Linux fq_codel qdisc'inin implementasyonu.
/// - Her flow (5-tuple hash) ayrı kuyruk
/// - DRR (Deficit Round Robin) ile fair scheduling
/// - CoDel AQM ile bufferbloat önleme
pub struct FqCodel {
    flows: Vec<FqCodelFlow>,
    new_flows: VecDeque<usize>,
    old_flows: VecDeque<usize>,
    limit: usize,
    total_packets: usize,
    quantum: u32,  // Her round'da her flow'a verilecek byte miktarı
    stats: QdiscStats,
    codel_target: u64,
    codel_interval: u64,
}

impl FqCodel {
    pub fn new() -> Self {
        let flows = (0..FQ_CODEL_FLOWS).map(|_| FqCodelFlow::new()).collect();
        
        FqCodel {
            flows,
            new_flows: VecDeque::new(),
            old_flows: VecDeque::new(),
            limit: FQ_CODEL_LIMIT,
            total_packets: 0,
            quantum: 1514,  // ~1 MTU
            stats: QdiscStats::default(),
            codel_target: CODEL_TARGET,
            codel_interval: CODEL_INTERVAL,
        }
    }
    
    /// Paket enqueue et
    pub fn enqueue(&mut self, pkt: TcPacket) -> Result<(), TcPacket> {
        if self.total_packets >= self.limit {
            // Queue full — en büyük flow'dan drop
            self.drop_from_largest_flow();
            self.stats.drop_count += 1;
            self.stats.overlimits += 1;
        }
        
        let flow_idx = (pkt.flow_hash as usize) % FQ_CODEL_FLOWS;
        let pkt_len = pkt.len as u64;
        let was_empty = self.flows[flow_idx].queue.is_empty();

        if was_empty {
            self.flows[flow_idx].deficit = self.quantum as i32;
            self.enqueue_flow(flow_idx, FlowListKind::New);
        }

        self.flows[flow_idx].queue.push_back(pkt);
        self.total_packets += 1;
        self.stats.enqueue_count += 1;
        self.stats.backlog_packets += 1;
        self.stats.backlog_bytes += pkt_len;
        
        Ok(())
    }
    
    /// Paket dequeue et (DRR + CoDel)
    pub fn dequeue(&mut self, now: u64) -> Option<TcPacket> {
        if let Some(pkt) = self.dequeue_from_list(FlowListKind::New, now) {
            return Some(pkt);
        }
        self.dequeue_from_list(FlowListKind::Old, now)
    }
    
    fn dequeue_from_list(&mut self, list_kind: FlowListKind, now: u64) -> Option<TcPacket> {
        loop {
            let flow_idx = {
                let list = self.flow_list_mut(list_kind);
                if list.is_empty() {
                    return None;
                }
                list.pop_front().unwrap()
            };
            self.set_flow_membership(flow_idx, list_kind, false);

            if self.flows[flow_idx].queue.is_empty() {
                self.reset_idle_flow(flow_idx);
                continue;
            }

            if self.flows[flow_idx].deficit <= 0 {
                self.flows[flow_idx].deficit += self.quantum as i32;
                self.enqueue_flow(flow_idx, FlowListKind::Old);
                continue;
            }

            if let Some(pkt) = self.codel_dequeue(flow_idx, now) {
                self.flows[flow_idx].deficit -= pkt.len as i32;
                if self.flows[flow_idx].queue.is_empty() {
                    self.reset_idle_flow(flow_idx);
                    return Some(pkt);
                }
                if self.flows[flow_idx].deficit > 0 {
                    self.enqueue_flow(flow_idx, list_kind);
                } else {
                    self.flows[flow_idx].deficit += self.quantum as i32;
                    self.enqueue_flow(flow_idx, FlowListKind::Old);
                }
                return Some(pkt);
            }

            self.reset_idle_flow(flow_idx);
        }
    }
    
    /// CoDel control law: interval / sqrt(count)
    fn control_law(&self, count: u32) -> u64 {
        let c = count.max(1) as f64;
        let inv_sqrt = 1.0 / libm::sqrt(c);
        (self.codel_interval as f64 * inv_sqrt) as u64
    }

    fn flow_list_mut(&mut self, list_kind: FlowListKind) -> &mut VecDeque<usize> {
        match list_kind {
            FlowListKind::New => &mut self.new_flows,
            FlowListKind::Old => &mut self.old_flows,
        }
    }

    fn enqueue_flow(&mut self, flow_idx: usize, list_kind: FlowListKind) {
        if self.flow_in_list(flow_idx, list_kind) {
            return;
        }
        self.flow_list_mut(list_kind).push_back(flow_idx);
        self.set_flow_membership(flow_idx, list_kind, true);
    }

    fn flow_in_list(&self, flow_idx: usize, list_kind: FlowListKind) -> bool {
        match list_kind {
            FlowListKind::New => self.flows[flow_idx].in_new_flows,
            FlowListKind::Old => self.flows[flow_idx].in_old_flows,
        }
    }

    fn set_flow_membership(&mut self, flow_idx: usize, list_kind: FlowListKind, present: bool) {
        match list_kind {
            FlowListKind::New => self.flows[flow_idx].in_new_flows = present,
            FlowListKind::Old => self.flows[flow_idx].in_old_flows = present,
        }
    }

    fn reset_idle_flow(&mut self, flow_idx: usize) {
        let flow = &mut self.flows[flow_idx];
        flow.deficit = 0;
        flow.dropping = false;
        flow.drop_next = 0;
        flow.first_above_time = 0;
        flow.count = 0;
        flow.last_count = 0;
        flow.in_new_flows = false;
        flow.in_old_flows = false;
    }

    fn codel_dequeue(&mut self, flow_idx: usize, now: u64) -> Option<TcPacket> {
        loop {
            let pkt = self.flows[flow_idx].queue.pop_front()?;
            let sojourn = now.saturating_sub(pkt.enqueue_time);

            self.total_packets = self.total_packets.saturating_sub(1);
            self.stats.backlog_packets = self.stats.backlog_packets.saturating_sub(1);
            self.stats.backlog_bytes = self.stats.backlog_bytes.saturating_sub(pkt.len as u64);

            if sojourn > self.codel_target {
                if self.flows[flow_idx].first_above_time == 0 {
                    self.flows[flow_idx].first_above_time = now + self.codel_interval;
                } else if now >= self.flows[flow_idx].first_above_time {
                    if self.flows[flow_idx].dropping && now >= self.flows[flow_idx].drop_next {
                        self.stats.drop_count += 1;
                        self.flows[flow_idx].count += 1;
                        let next_drop = self.control_law(self.flows[flow_idx].count);
                        self.flows[flow_idx].drop_next = now + next_drop;
                        continue;
                    }
                    if !self.flows[flow_idx].dropping {
                        self.flows[flow_idx].dropping = true;
                        self.flows[flow_idx].count = if self.flows[flow_idx].count > 2 {
                            self.flows[flow_idx].count - 2
                        } else {
                            1
                        };
                        let next_drop = self.control_law(self.flows[flow_idx].count);
                        self.flows[flow_idx].drop_next = now + next_drop;
                    }
                }
            } else {
                self.flows[flow_idx].first_above_time = 0;
                if self.flows[flow_idx].dropping {
                    self.flows[flow_idx].dropping = false;
                    self.flows[flow_idx].last_count = self.flows[flow_idx].count;
                    self.flows[flow_idx].count = 0;
                }
            }

            self.stats.dequeue_count += 1;
            return Some(pkt);
        }
    }
    
    /// En büyük flow'dan bir paket drop et (limit aşıldığında)
    fn drop_from_largest_flow(&mut self) {
        let mut max_idx = 0;
        let mut max_len = 0;
        
        for (i, flow) in self.flows.iter().enumerate() {
            if flow.queue.len() > max_len {
                max_len = flow.queue.len();
                max_idx = i;
            }
        }
        
        if max_len > 0 {
            if let Some(pkt) = self.flows[max_idx].queue.pop_front() {
                self.total_packets -= 1;
                self.stats.backlog_packets = self.stats.backlog_packets.saturating_sub(1);
                self.stats.backlog_bytes = self.stats.backlog_bytes.saturating_sub(pkt.len as u64);
                if self.flows[max_idx].queue.is_empty() {
                    self.reset_idle_flow(max_idx);
                }
            }
        }
    }
    
    pub fn len(&self) -> usize {
        self.total_packets
    }
    
    pub fn is_empty(&self) -> bool {
        self.total_packets == 0
    }
    
    pub fn stats(&self) -> &QdiscStats {
        &self.stats
    }
}

// ============================================================================
// QDISC WRAPPER (Enum dispatch)
// ============================================================================

/// Qdisc wrapper — farklı qdisc türlerini tek enum'da toplar
pub enum Qdisc {
    PfifoFast(PfifoFast),
    FqCodel(FqCodel),
    Noop,
}

impl Qdisc {
    pub fn new(kind: QdiscKind) -> Self {
        match kind {
            QdiscKind::PfifoFast => Qdisc::PfifoFast(PfifoFast::new(1000)),
            QdiscKind::FqCodel => Qdisc::FqCodel(FqCodel::new()),
            QdiscKind::Noop => Qdisc::Noop,
        }
    }
    
    pub fn enqueue(&mut self, pkt: TcPacket) -> Result<(), TcPacket> {
        match self {
            Qdisc::PfifoFast(q) => q.enqueue(pkt),
            Qdisc::FqCodel(q) => q.enqueue(pkt),
            Qdisc::Noop => {
                // Noop: paket at
                Err(pkt)
            }
        }
    }
    
    pub fn dequeue(&mut self, now: u64) -> Option<TcPacket> {
        match self {
            Qdisc::PfifoFast(q) => q.dequeue(),
            Qdisc::FqCodel(q) => q.dequeue(now),
            Qdisc::Noop => None,
        }
    }
    
    pub fn len(&self) -> usize {
        match self {
            Qdisc::PfifoFast(q) => q.len(),
            Qdisc::FqCodel(q) => q.len(),
            Qdisc::Noop => 0,
        }
    }
    
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    pub fn kind(&self) -> QdiscKind {
        match self {
            Qdisc::PfifoFast(_) => QdiscKind::PfifoFast,
            Qdisc::FqCodel(_) => QdiscKind::FqCodel,
            Qdisc::Noop => QdiscKind::Noop,
        }
    }

    pub fn stats(&self) -> QdiscStats {
        match self {
            Qdisc::PfifoFast(q) => q.stats.clone(),
            Qdisc::FqCodel(q) => q.stats.clone(),
            Qdisc::Noop => QdiscStats::default(),
        }
    }
}

// ============================================================================
// TC MANAGER (Per-interface qdisc yönetimi)
// ============================================================================

/// Traffic Control Manager — her interface için qdisc yönetir
pub struct TcManager {
    /// Interface → Qdisc mapping
    qdiscs: Mutex<BTreeMap<String, Qdisc>>,
}

impl TcManager {
    pub fn new() -> Self {
        TcManager {
            qdiscs: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Interface için qdisc ata
    pub fn set_qdisc(&self, iface: &str, kind: QdiscKind) {
        let mut qdiscs = self.qdiscs.lock();
        qdiscs.insert(String::from(iface), Qdisc::new(kind));
    }
    
    /// Interface'in qdisc'ini al
    pub fn get_qdisc(&self, iface: &str) -> Option<QdiscKind> {
        let qdiscs = self.qdiscs.lock();
        qdiscs.get(iface).map(|q| q.kind())
    }
    
    /// Paket enqueue
    pub fn enqueue(&self, iface: &str, pkt: TcPacket) -> Result<(), TcPacket> {
        let mut qdiscs = self.qdiscs.lock();
        if let Some(qdisc) = qdiscs.get_mut(iface) {
            qdisc.enqueue(pkt)
        } else {
            // Default: pfifo_fast
            let mut qdisc = Qdisc::new(QdiscKind::PfifoFast);
            let result = qdisc.enqueue(pkt);
            qdiscs.insert(String::from(iface), qdisc);
            result
        }
    }
    
    /// Paket dequeue
    pub fn dequeue(&self, iface: &str, now: u64) -> Option<TcPacket> {
        let mut qdiscs = self.qdiscs.lock();
        qdiscs.get_mut(iface)?.dequeue(now)
    }

    /// Dump all qdiscs: returns (iface_name, qdisc_kind, stats)
    pub fn dump_all(&self) -> Vec<(String, QdiscKind, QdiscStats)> {
        let qdiscs = self.qdiscs.lock();
        let mut result = Vec::with_capacity(qdiscs.len());
        for (name, q) in qdiscs.iter() {
            result.push((name.clone(), q.kind(), q.stats()));
        }
        result
    }

    /// Check if an interface has a qdisc configured
    pub fn has_qdisc(&self, iface: &str) -> bool {
        self.qdiscs.lock().contains_key(iface)
    }
}

/// Global TC manager
pub static TC_MANAGER: TcManager = TcManager {
    qdiscs: Mutex::new(BTreeMap::new()),
};

// ============================================================================
// INIT
// ============================================================================

pub fn init() {
    crate::serial_println!("[TC] Traffic control framework initialized (pfifo_fast + fq_codel)");
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn pfifo_fast_priority_ordering() {
        let mut q = PfifoFast::new(100);
        
        // Düşük öncelikli paketi önce ekle
        q.enqueue(TcPacket::new(vec![1; 5], 2, 0, 0)).unwrap();
        // Yüksek öncelikli paketi sonra ekle
        q.enqueue(TcPacket::new(vec![2; 5], 0, 0, 0)).unwrap();
        // Normal öncelik
        q.enqueue(TcPacket::new(vec![3; 5], 1, 0, 0)).unwrap();
        
        // Dequeue: yüksek öncelik (0) → normal (1) → düşük (2)
        let p1 = q.dequeue().unwrap();
        assert_eq!(p1.data[0], 2);  // Band 0
        
        let p2 = q.dequeue().unwrap();
        assert_eq!(p2.data[0], 3);  // Band 1
        
        let p3 = q.dequeue().unwrap();
        assert_eq!(p3.data[0], 1);  // Band 2
    }
    
    #[test]
    fn pfifo_fast_limit() {
        let mut q = PfifoFast::new(2);
        
        assert!(q.enqueue(TcPacket::new(vec![1; 64], 0, 0, 0)).is_ok());
        assert!(q.enqueue(TcPacket::new(vec![2; 64], 0, 0, 0)).is_ok());
        assert!(q.enqueue(TcPacket::new(vec![3; 64], 0, 0, 0)).is_err()); // Full
        
        assert_eq!(q.stats().drop_count, 1);
    }
    
    #[test]
    fn fq_codel_basic_enqueue_dequeue() {
        let mut q = FqCodel::new();
        
        q.enqueue(TcPacket::new(vec![1; 64], 0, 100, 0)).unwrap();
        q.enqueue(TcPacket::new(vec![2; 64], 0, 200, 100)).unwrap();
        
        assert_eq!(q.len(), 2);
        
        let p1 = q.dequeue(200).unwrap();
        assert_eq!(p1.len, 64);
        
        assert_eq!(q.len(), 1);
    }
    
    #[test]
    fn fq_codel_fairness() {
        let mut q = FqCodel::new();
        
        // Flow 1: 10 paket
        for i in 0..10u64 {
            q.enqueue(TcPacket::new(vec![i as u8; 64], 0, 1, i * 100)).unwrap();
        }

        // Flow 2: 10 paket
        for i in 0..10u64 {
            q.enqueue(TcPacket::new(vec![(i as u8).wrapping_add(100); 64], 0, 2, i * 100)).unwrap();
        }
        
        // Dequeue: flow'lar arası adil (DRR)
        let mut flow1_count = 0;
        let mut flow2_count = 0;
        
        for _ in 0..20 {
            if let Some(pkt) = q.dequeue(20_000) {
                if pkt.data[0] < 100 {
                    flow1_count += 1;
                } else {
                    flow2_count += 1;
                }
            }
        }
        
        // Tüm paketler tüketilmeli
        assert_eq!(q.len(), 0);
        assert_eq!(flow1_count, 10);
        assert_eq!(flow2_count, 10);
    }

    #[test]
    fn fq_codel_tracks_backlog_bytes() {
        let mut q = FqCodel::new();
        q.enqueue(TcPacket::new(vec![1; 128], 0, 7, 0)).unwrap();
        q.enqueue(TcPacket::new(vec![2; 64], 0, 8, 10)).unwrap();
        assert_eq!(q.stats().backlog_packets, 2);
        assert_eq!(q.stats().backlog_bytes, 192);
        let _ = q.dequeue(100).unwrap();
        assert_eq!(q.stats().backlog_packets, 1);
        assert_eq!(q.stats().backlog_bytes, 64);
    }

    #[test]
    fn fq_codel_does_not_duplicate_active_flow_entries() {
        let mut q = FqCodel::new();
        q.enqueue(TcPacket::new(vec![1; 64], 0, 42, 0)).unwrap();
        q.enqueue(TcPacket::new(vec![2; 64], 0, 42, 10)).unwrap();
        q.enqueue(TcPacket::new(vec![3; 64], 0, 42, 20)).unwrap();

        assert_eq!(q.new_flows.len(), 1);
        assert!(q.old_flows.is_empty());

        let _ = q.dequeue(1_000).unwrap();
        assert_eq!(q.new_flows.len(), 1);
        assert!(q.old_flows.is_empty());
    }
    
    #[test]
    fn tc_manager_set_and_enqueue() {
        let mgr = TcManager::new();
        
        mgr.set_qdisc("eth0", QdiscKind::FqCodel);
        assert_eq!(mgr.get_qdisc("eth0"), Some(QdiscKind::FqCodel));
        
        let pkt = TcPacket::new(vec![1; 64], 0, 0, 0);
        assert!(mgr.enqueue("eth0", pkt).is_ok());
        
        let dequeued = mgr.dequeue("eth0", 100);
        assert!(dequeued.is_some());
    }
    
    #[test]
    fn qdisc_kind_dispatch() {
        let mut q = Qdisc::new(QdiscKind::PfifoFast);
        assert_eq!(q.kind(), QdiscKind::PfifoFast);
        assert!(q.is_empty());
        
        q.enqueue(TcPacket::new(vec![1; 64], 0, 0, 0)).unwrap();
        assert_eq!(q.len(), 1);
        
        let pkt = q.dequeue(0).unwrap();
        assert_eq!(pkt.len, 64);
    }
    
    #[test]
    fn priority_from_tos() {
        assert_eq!(TcPacket::priority_from_tos(0x00), 1);  // Normal
        assert_eq!(TcPacket::priority_from_tos(0x28), 0);  // EF (DSCP 10)
        assert_eq!(TcPacket::priority_from_tos(0xC0), 2);  // Low priority
    }
}
