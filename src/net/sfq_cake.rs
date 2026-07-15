use super::NetError;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

const SFQ_DEFAULT_QUANTUM: u32 = 1514;
const SFQ_DEFAULT_PERTURB_INTERVAL: u32 = 10;
const SFQ_MAX_BUCKETS: usize = 1024;
const SFQ_MIN_BUCKETS: usize = 16;

const CAKE_DEFAULT_BANDWIDTH: u32 = 1_000_000;
const CAKE_MAX_TINS: usize = 8;
const CAKE_DFLT_TARGET_US: u32 = 5_000;
const CAKE_DFLT_INTERVAL_US: u32 = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffServMode {
    BestEffort,
    DiffServ3,
    DiffServ4,
    DiffServ8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowMode {
    None,
    Host,
    Flow,
    DualHost,
    DualFlow,
    TripleHost,
    TripleFlow,
}

pub struct SfqQueue {
    buckets: VecDeque<Vec<u8>>,
    quantum: u32,
    perturbation: u32,
    perturb_interval: u32,
    perturb_counter: u32,
    tail: usize,
    flow_count: u32,
    max_buckets: usize,
}

impl SfqQueue {
    pub fn new(quantum: u32, perturb_interval: u32, max_buckets: usize) -> Self {
        let nbuckets = max_buckets.clamp(SFQ_MIN_BUCKETS, SFQ_MAX_BUCKETS);
        let mut buckets = VecDeque::with_capacity(nbuckets);
        for _ in 0..nbuckets {
            buckets.push_back(Vec::new());
        }
        SfqQueue {
            buckets,
            quantum,
            perturbation: 0,
            perturb_interval,
            perturb_counter: 0,
            tail: 0,
            flow_count: 0,
            max_buckets: nbuckets,
        }
    }

    pub fn enqueue(&mut self, packet: Vec<u8>) -> Result<(), NetError> {
        if self.total_len() >= self.max_buckets * 64 {
            return Err(NetError::BufferFull);
        }
        let hash = sfq_flow_hash_with_perturbation(&packet, self.perturbation);
        let bucket = (hash as usize) % self.max_buckets;
        self.buckets[bucket].extend_from_slice(&packet);
        self.flow_count += 1;
        self.perturb_counter += 1;
        if self.perturb_counter >= self.perturb_interval {
            self.perturbation = self.perturbation.wrapping_add(0x9E3779B9);
            self.perturb_counter = 0;
        }
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<Vec<u8>> {
        let nbuckets = self.max_buckets;
        for _ in 0..nbuckets {
            self.tail = (self.tail + 1) % nbuckets;
            if !self.buckets[self.tail].is_empty() {
                let pkt = self.buckets[self.tail].clone();
                self.buckets[self.tail].clear();
                self.flow_count = self.flow_count.saturating_sub(1);
                return Some(pkt);
            }
        }
        None
    }

    pub fn total_len(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.iter().all(|b| b.is_empty())
    }

    pub fn quantum(&self) -> u32 {
        self.quantum
    }

    pub fn set_quantum(&mut self, quantum: u32) {
        self.quantum = quantum;
    }

    pub fn perturbation(&self) -> u32 {
        self.perturbation
    }

    pub fn flow_count(&self) -> u32 {
        self.flow_count
    }
}

pub fn sfq_init(quantum: u32, perturb_interval: u32) -> SfqQueue {
    SfqQueue::new(quantum, perturb_interval, SFQ_MAX_BUCKETS)
}

pub fn sfq_enqueue(queue: &mut SfqQueue, packet: Vec<u8>) -> Result<(), NetError> {
    queue.enqueue(packet)
}

pub fn sfq_dequeue(queue: &mut SfqQueue) -> Option<Vec<u8>> {
    queue.dequeue()
}

pub fn sfq_flow_hash(packet: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &b in packet.iter() {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn sfq_flow_hash_with_perturbation(packet: &[u8], perturbation: u32) -> u32 {
    sfq_flow_hash(packet) ^ perturbation
}

#[derive(Clone, Debug)]
pub struct CakeTin {
    pub rate_bytes_per_sec: u32,
    pub target_us: u32,
    pub interval_us: u32,
    pub threshold_us: u32,
    pub diffserv_mode: DiffServMode,
    pub flow_mode: FlowMode,
    pub backlog: u32,
    pub max_ptime: u32,
    pub deficit: i32,
    pub spare: u32,
    /// Gerçek paket kuyruğu (hollow fix: eskiden sadece backlog sayacıydı)
    pub packets: VecDeque<Vec<u8>>,
}

impl CakeTin {
    pub fn new(diffserv: DiffServMode, flow: FlowMode, bandwidth: u32) -> Self {
        let target = cake_bandwidth_to_target(bandwidth);
        CakeTin {
            rate_bytes_per_sec: bandwidth / 8,
            target_us: target,
            interval_us: CAKE_DFLT_INTERVAL_US,
            threshold_us: 0,
            diffserv_mode: diffserv,
            flow_mode: flow,
            backlog: 0,
            max_ptime: 0,
            deficit: 0,
            spare: 0,
            packets: VecDeque::new(),
        }
    }
}

pub struct CakeQueue {
    pub tins: Vec<CakeTin>,
    pub bandwidth: u32,
    pub target_us: u32,
    pub interval_us: u32,
    pub overhead: u16,
    pub tin_idx: usize,
    pub time_next_packet: u64,
    pub now: u64,
}

impl CakeQueue {
    pub fn new(bandwidth: u32, diffserv: DiffServMode) -> Self {
        let target = cake_bandwidth_to_target(bandwidth);
        let mut tins = Vec::new();
        match diffserv {
            DiffServMode::BestEffort => {
                tins.push(CakeTin::new(diffserv, FlowMode::Flow, bandwidth));
            }
            DiffServMode::DiffServ3 => {
                let third = bandwidth / 3;
                tins.push(CakeTin::new(DiffServMode::DiffServ3, FlowMode::Flow, third));
                tins.push(CakeTin::new(DiffServMode::DiffServ3, FlowMode::Flow, third));
                tins.push(CakeTin::new(DiffServMode::DiffServ3, FlowMode::Flow, third));
            }
            DiffServMode::DiffServ4 => {
                let quarter = bandwidth / 4;
                tins.push(CakeTin::new(DiffServMode::DiffServ4, FlowMode::Flow, quarter));
                tins.push(CakeTin::new(DiffServMode::DiffServ4, FlowMode::Flow, quarter));
                tins.push(CakeTin::new(DiffServMode::DiffServ4, FlowMode::Flow, quarter));
                tins.push(CakeTin::new(DiffServMode::DiffServ4, FlowMode::Flow, quarter));
            }
            DiffServMode::DiffServ8 => {
                let eighth = bandwidth / 8;
                for _ in 0..8 {
                    tins.push(CakeTin::new(DiffServMode::DiffServ8, FlowMode::Flow, eighth));
                }
            }
        }
        CakeQueue {
            tins,
            bandwidth,
            target_us: target,
            interval_us: CAKE_DFLT_INTERVAL_US,
            overhead: 0,
            tin_idx: 0,
            time_next_packet: 0,
            now: 0,
        }
    }

    pub fn enqueue(&mut self, packet: Vec<u8>) -> Result<(), NetError> {
        let tin_idx = self.classify_packet(&packet);
        if tin_idx >= self.tins.len() {
            return Err(NetError::InvalidPacket);
        }
        let tin = &mut self.tins[tin_idx];
        let len = packet.len() as u32;
        tin.packets.push_back(packet);
        tin.backlog += len;
        if tin.backlog > 1024 * 1024 {
            return Err(NetError::BufferFull);
        }
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<Vec<u8>> {
        let tin_count = self.tins.len();
        if tin_count == 0 {
            return None;
        }
        for _ in 0..tin_count {
            let idx = self.tin_idx;
            self.tin_idx = (self.tin_idx + 1) % tin_count;
            let tin = &mut self.tins[idx];
            if let Some(pkt) = tin.packets.pop_front() {
                let len = pkt.len() as u32;
                tin.backlog = tin.backlog.saturating_sub(len);
                return Some(pkt);
            }
        }
        None
    }

    fn classify_packet(&self, packet: &[u8]) -> usize {
        if self.tins.len() <= 1 {
            return 0;
        }
        let dscp = if packet.len() >= 1 {
            packet[0]
        } else {
            0
        };
        let ecn = dscp & 0x03;
        let dscp_val = dscp >> 2;
        match self.tins[0].diffserv_mode {
            DiffServMode::BestEffort => 0,
            DiffServMode::DiffServ3 => match dscp_val {
                0..=7 => 2,
                8..=39 => 1,
                _ => 0,
            },
            DiffServMode::DiffServ4 => match dscp_val {
                0..=7 => 3,
                8..=15 => 2,
                16..=31 => 1,
                _ => 0,
            },
            DiffServMode::DiffServ8 => {
                let idx = ((dscp_val >> 3) as usize).min(self.tins.len() - 1);
                idx
            }
        }
    }

    pub fn total_backlog(&self) -> u32 {
        self.tins.iter().map(|t| t.backlog).sum()
    }

    pub fn tin_count(&self) -> usize {
        self.tins.len()
    }
}

pub fn cake_init(bandwidth: u32, diffserv_mode: DiffServMode) -> CakeQueue {
    CakeQueue::new(bandwidth, diffserv_mode)
}

pub fn cake_enqueue(queue: &mut CakeQueue, packet: Vec<u8>) -> Result<(), NetError> {
    queue.enqueue(packet)
}

pub fn cake_dequeue(queue: &mut CakeQueue) -> Option<Vec<u8>> {
    queue.dequeue()
}

pub fn cake_bandwidth_to_target(bandwidth_bps: u32) -> u32 {
    if bandwidth_bps >= 5_000_000 {
        CAKE_DFLT_TARGET_US
    } else if bandwidth_bps >= 1_000_000 {
        10_000
    } else if bandwidth_bps >= 100_000 {
        50_000
    } else {
        100_000
    }
}

pub fn cake_overhead_adjusted(bandwidth: u32, overhead: u16) -> u32 {
    if overhead == 0 {
        return bandwidth;
    }
    let mtu = 1500u32 + overhead as u32;
    let factor = (mtu + 64) * 8;
    let adjusted = bandwidth.saturating_sub(factor * (bandwidth / (mtu * 8 / 10)));
    adjusted.max(1)
}

pub fn cake_flow_mode_to_uint(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::None => 0,
        FlowMode::Host => 1,
        FlowMode::Flow => 2,
        FlowMode::DualHost => 3,
        FlowMode::DualFlow => 4,
        FlowMode::TripleHost => 5,
        FlowMode::TripleFlow => 6,
    }
}

pub fn cake_diffserv_to_uint(mode: DiffServMode) -> u8 {
    match mode {
        DiffServMode::BestEffort => 0,
        DiffServMode::DiffServ3 => 1,
        DiffServMode::DiffServ4 => 2,
        DiffServMode::DiffServ8 => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sfq_init() {
        let q = sfq_init(1514, 10);
        assert_eq!(q.quantum(), 1514);
        assert!(q.is_empty());
        assert_eq!(q.flow_count(), 0);
    }

    #[test]
    fn test_sfq_enqueue_dequeue() {
        let mut q = sfq_init(1514, 10);
        let pkt1 = vec![1, 2, 3, 4];
        let pkt2 = vec![5, 6, 7, 8];
        sfq_enqueue(&mut q, pkt1.clone()).unwrap();
        sfq_enqueue(&mut q, pkt2.clone()).unwrap();
        assert_eq!(q.total_len(), 8);
        let d1 = sfq_dequeue(&mut q).unwrap();
        let d2 = sfq_dequeue(&mut q).unwrap();
        assert!(d1 == pkt1 || d1 == pkt2);
        assert!(d2 == pkt1 || d2 == pkt2);
        assert_ne!(d1, d2);
        assert!(sfq_dequeue(&mut q).is_none());
    }

    #[test]
    fn test_sfq_flow_hash() {
        let h1 = sfq_flow_hash(&[1, 2, 3, 4]);
        let h2 = sfq_flow_hash(&[1, 2, 3, 5]);
        assert_ne!(h1, h2);
        let h3 = sfq_flow_hash(&[1, 2, 3, 4]);
        assert_eq!(h1, h3);
    }

    #[test]
    fn test_sfq_buffer_full() {
        let mut q = SfqQueue::new(1514, 10, 16);
        for _ in 0..1024 {
            let _ = q.enqueue(vec![0u8; 64]);
        }
        assert!(q.enqueue(vec![0u8; 64]).is_err());
    }

    #[test]
    fn test_cake_init_best_effort() {
        let q = cake_init(10_000_000, DiffServMode::BestEffort);
        assert_eq!(q.tins.len(), 1);
        assert_eq!(q.bandwidth, 10_000_000);
        assert_eq!(q.target_us, 5_000);
    }

    #[test]
    fn test_cake_init_diffserv3() {
        let q = cake_init(30_000_000, DiffServMode::DiffServ3);
        assert_eq!(q.tins.len(), 3);
    }

    #[test]
    fn test_cake_init_diffserv4() {
        let q = cake_init(40_000_000, DiffServMode::DiffServ4);
        assert_eq!(q.tins.len(), 4);
    }

    #[test]
    fn test_cake_init_diffserv8() {
        let q = cake_init(80_000_000, DiffServMode::DiffServ8);
        assert_eq!(q.tins.len(), 8);
    }

    #[test]
    fn test_cake_bandwidth_to_target() {
        assert_eq!(cake_bandwidth_to_target(10_000_000), 5_000);
        assert_eq!(cake_bandwidth_to_target(5_000_000), 5_000);
        assert_eq!(cake_bandwidth_to_target(2_000_000), 10_000);
        assert_eq!(cake_bandwidth_to_target(500_000), 50_000);
        assert_eq!(cake_bandwidth_to_target(50_000), 100_000);
    }

    #[test]
    fn test_cake_enqueue_dequeue() {
        let mut q = cake_init(10_000_000, DiffServMode::BestEffort);
        let pkt = vec![0xAA; 100];
        cake_enqueue(&mut q, pkt).unwrap();
        assert_eq!(q.total_backlog(), 100);
        let d = cake_dequeue(&mut q).unwrap();
        assert_eq!(d.len(), 100);
        assert_eq!(q.total_backlog(), 0);
    }

    #[test]
    fn test_cake_flow_mode_conversion() {
        assert_eq!(cake_flow_mode_to_uint(FlowMode::None), 0);
        assert_eq!(cake_flow_mode_to_uint(FlowMode::Flow), 2);
        assert_eq!(cake_flow_mode_to_uint(FlowMode::TripleFlow), 6);
    }

    #[test]
    fn test_cake_diffserv_conversion() {
        assert_eq!(cake_diffserv_to_uint(DiffServMode::BestEffort), 0);
        assert_eq!(cake_diffserv_to_uint(DiffServMode::DiffServ3), 1);
        assert_eq!(cake_diffserv_to_uint(DiffServMode::DiffServ8), 3);
    }

    #[test]
    fn test_cake_overhead_adjusted() {
        let adj = cake_overhead_adjusted(1_000_000, 0);
        assert_eq!(adj, 1_000_000);
        let adj2 = cake_overhead_adjusted(1_000_000, 32);
        assert!(adj2 < 1_000_000);
    }

    #[test]
    fn test_sfq_perturbation() {
        let mut q = sfq_init(1514, 2);
        let initial_pert = q.perturbation();
        let pkt = vec![0u8; 64];
        let _ = sfq_enqueue(&mut q, pkt);
        let _ = sfq_dequeue(&mut q);
        let _ = sfq_enqueue(&mut q, vec![0u8; 64]);
        assert_ne!(q.perturbation(), initial_pert);
    }
}
