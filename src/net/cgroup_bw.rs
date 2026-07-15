use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

pub const CGROUP_DEFAULT_CLASSID: u32 = 0;
pub const CGROOT_CLASSID: u32 = 1;
pub const CGROUP_MAX_CLASSES: u32 = 65536;
pub const CGROUP_MIN_BURST: u32 = 1024;
pub const CGROUP_MAX_BURST: u32 = 10_000_000;
pub const CGROUP_MAX_RATE: u32 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgroupEgressAction {
    Transmit,
    Drop,
    Requeue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CgroupClassError {
    ClassNotFound,
    ClassAlreadyExists,
    InvalidRate,
    InvalidBurst,
    QueueFull,
    TokenBucketEmpty,
    InvalidParam,
}

#[derive(Clone, Debug)]
pub struct CgroupClassify {
    pub classid: u32,
    pub prio: u16,
    pub rate_bytes_per_sec: u32,
    pub burst_bytes: u32,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub packets_dropped: u64,
    pub bytes_dropped: u64,
    pub rate_est_bytes_per_sec: u32,
}

impl Default for CgroupClassify {
    fn default() -> Self {
        CgroupClassify {
            classid: 0,
            prio: 0,
            rate_bytes_per_sec: 0,
            burst_bytes: 0,
            packets_sent: 0,
            bytes_sent: 0,
            packets_dropped: 0,
            bytes_dropped: 0,
            rate_est_bytes_per_sec: 0,
        }
    }
}

impl CgroupClassify {
    pub fn new(classid: u32, prio: u16, rate_bytes_per_sec: u32, burst_bytes: u32) -> Self {
        CgroupClassify {
            classid,
            prio,
            rate_bytes_per_sec,
            burst_bytes,
            packets_sent: 0,
            bytes_sent: 0,
            packets_dropped: 0,
            bytes_dropped: 0,
            rate_est_bytes_per_sec: 0,
        }
    }

    pub fn update_stats_sent(&mut self, bytes: u32) {
        self.packets_sent += 1;
        self.bytes_sent += bytes as u64;
    }

    pub fn update_stats_dropped(&mut self, bytes: u32) {
        self.packets_dropped += 1;
        self.bytes_dropped += bytes as u64;
    }

    pub fn update_rate_estimation(&mut self, bytes_per_sec: u32) {
        self.rate_est_bytes_per_sec = bytes_per_sec;
    }
}

#[derive(Clone, Debug)]
pub struct TokenBucket {
    pub tokens: i64,
    pub rate: u32,
    pub burst: u32,
    pub last_update: u64,
    pub total_consumed: u64,
    pub total_refilled: u64,
    pub drops: u64,
}

impl Default for TokenBucket {
    fn default() -> Self {
        TokenBucket {
            tokens: 0,
            rate: 0,
            burst: 0,
            last_update: 0,
            total_consumed: 0,
            total_refilled: 0,
            drops: 0,
        }
    }
}

impl TokenBucket {
    pub fn new(rate: u32, burst: u32, current_time_ms: u64) -> Self {
        TokenBucket {
            tokens: if rate == 0 { 0 } else { burst as i64 },
            rate,
            burst,
            last_update: current_time_ms,
            total_consumed: 0,
            total_refilled: 0,
            drops: 0,
        }
    }

    pub fn available_tokens(&self) -> i64 {
        self.tokens
    }

    pub fn is_empty(&self) -> bool {
        self.tokens <= 0
    }

    pub fn utilization_pct(&self) -> u32 {
        if self.burst == 0 {
            return 0;
        }
        ((self.burst as i64 - self.tokens) * 100 / self.burst as i64).max(0) as u32
    }
}

pub fn token_bucket_init(bucket: &mut TokenBucket, rate: u32, burst: u32) {
    bucket.rate = rate;
    bucket.burst = burst;
    bucket.tokens = burst as i64;
}

pub fn token_bucket_refill(bucket: &mut TokenBucket, current_time_ms: u64) {
    if bucket.rate == 0 {
        return;
    }
    let elapsed = current_time_ms.saturating_sub(bucket.last_update);
    if elapsed == 0 {
        return;
    }
    let refill = (bucket.rate as u64) * (elapsed as u64) / 1000;
    if refill > 0 {
        bucket.tokens = core::cmp::min(
            bucket.tokens.saturating_add(refill as i64),
            bucket.burst as i64,
        );
        bucket.total_refilled += refill;
        bucket.last_update = current_time_ms;
    }
}

pub fn token_bucket_consume(bucket: &mut TokenBucket, bytes: u32, current_time_ms: u64) -> bool {
    token_bucket_refill(bucket, current_time_ms);
    if bucket.tokens >= bytes as i64 {
        bucket.tokens -= bytes as i64;
        bucket.total_consumed += bytes as u64;
        true
    } else {
        bucket.drops += 1;
        false
    }
}

pub fn token_bucket_peek(bucket: &TokenBucket, bytes: u32, current_time_ms: u64) -> bool {
    let elapsed = current_time_ms.saturating_sub(bucket.last_update);
    let refill = (bucket.rate as u64) * (elapsed as u64) / 1000;
    let available = bucket.tokens.saturating_add(refill as i64);
    available >= bytes as i64
}

#[derive(Clone, Debug)]
pub struct CgroupPacket {
    pub data: Vec<u8>,
    pub classid: u32,
    pub priority: u16,
    pub enqueue_time: u64,
    pub pkt_len: u32,
}

impl CgroupPacket {
    pub fn new(data: Vec<u8>, classid: u32, priority: u16, timestamp: u64) -> Self {
        let pkt_len = data.len() as u32;
        CgroupPacket {
            data,
            classid,
            priority,
            enqueue_time: timestamp,
            pkt_len,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CgroupClassStats {
    pub classid: u32,
    pub bytes_sent: u64,
    pub packets_sent: u64,
    pub bytes_dropped: u64,
    pub packets_dropped: u64,
    pub tokens_available: i64,
    pub rate_bytes_per_sec: u32,
    pub burst_bytes: u32,
    pub rate_est_bytes_per_sec: u32,
}

pub struct CgroupClassQueue {
    pub classes: BTreeMap<u32, CgroupClassify>,
    pub buckets: BTreeMap<u32, TokenBucket>,
    pub default_classid: u32,
    pub queues: BTreeMap<u32, VecDeque<CgroupPacket>>,
    pub total_enqueued: AtomicU64,
    pub total_dequeued: AtomicU64,
    pub total_dropped: AtomicU64,
    pub max_queue_depth: u32,
    pub enabled: AtomicBool,
}

impl Default for CgroupClassQueue {
    fn default() -> Self {
        CgroupClassQueue::new()
    }
}

impl CgroupClassQueue {
    pub fn new() -> Self {
        CgroupClassQueue {
            classes: BTreeMap::new(),
            buckets: BTreeMap::new(),
            default_classid: CGROUP_DEFAULT_CLASSID,
            queues: BTreeMap::new(),
            total_enqueued: AtomicU64::new(0),
            total_dequeued: AtomicU64::new(0),
            total_dropped: AtomicU64::new(0),
            max_queue_depth: 1024,
            enabled: AtomicBool::new(true),
        }
    }

    pub fn with_max_queue_depth(mut self, depth: u32) -> Self {
        self.max_queue_depth = depth;
        self
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

    pub fn set_default_class(&mut self, classid: u32) {
        self.default_classid = classid;
    }

    pub fn add_class(&mut self, class: CgroupClassify, current_time_ms: u64) {
        let bucket = TokenBucket::new(class.rate_bytes_per_sec, class.burst_bytes, current_time_ms);
        self.buckets.insert(class.classid, bucket);
        self.classes.insert(class.classid, class);
    }

    pub fn remove_class(&mut self, classid: u32) -> bool {
        self.classes.remove(&classid).is_some() | self.buckets.remove(&classid).is_some() | self.queues.remove(&classid).is_some()
    }

    pub fn class_exists(&self, classid: u32) -> bool {
        self.classes.contains_key(&classid)
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    pub fn get_class(&self, classid: u32) -> Option<&CgroupClassify> {
        self.classes.get(&classid)
    }

    pub fn get_class_mut(&mut self, classid: u32) -> Option<&mut CgroupClassify> {
        self.classes.get_mut(&classid)
    }

    pub fn get_bucket(&self, classid: u32) -> Option<&TokenBucket> {
        self.buckets.get(&classid)
    }

    pub fn get_bucket_mut(&mut self, classid: u32) -> Option<&mut TokenBucket> {
        self.buckets.get_mut(&classid)
    }

    pub fn queue_depth(&self, classid: u32) -> u32 {
        self.queues.get(&classid).map_or(0, |q| q.len() as u32)
    }

    pub fn total_queue_depth(&self) -> u32 {
        self.queues.values().map(|q| q.len() as u32).sum()
    }

    pub fn is_queue_full(&self, classid: u32) -> bool {
        self.queues
            .get(&classid)
            .map_or(false, |q| q.len() >= self.max_queue_depth as usize)
    }
}

pub fn cgroup_init() {
    crate::serial_println!("[CGROUP] Initializing cgroup bandwidth controller");
}

pub fn cgroup_create_class(
    queue: &Mutex<CgroupClassQueue>,
    classid: u32,
    prio: u16,
    rate: u32,
    burst: u32,
) -> Result<(), CgroupClassError> {
    if rate == 0 || rate > CGROUP_MAX_RATE {
        return Err(CgroupClassError::InvalidRate);
    }
    if burst < CGROUP_MIN_BURST || burst > CGROUP_MAX_BURST {
        return Err(CgroupClassError::InvalidBurst);
    }
    let mut q = queue.lock();
    if q.class_exists(classid) {
        return Err(CgroupClassError::ClassAlreadyExists);
    }
    let current_time = current_time_ms();
    let class = CgroupClassify::new(classid, prio, rate, burst);
    q.add_class(class, current_time);
    Ok(())
}

pub fn cgroup_destroy_class(
    queue: &Mutex<CgroupClassQueue>,
    classid: u32,
) -> Result<(), CgroupClassError> {
    let mut q = queue.lock();
    if !q.remove_class(classid) {
        return Err(CgroupClassError::ClassNotFound);
    }
    Ok(())
}

pub fn cgroup_classify_packet(queue: &Mutex<CgroupClassQueue>, packet: &[u8]) -> u32 {
    if packet.len() < 20 {
        let q = queue.lock();
        return q.default_classid;
    }
    let version_ihl = packet[0];
    let version = (version_ihl >> 4) & 0x0F;
    if version != 4 {
        let q = queue.lock();
        return q.default_classid;
    }
    let tos = packet[1];
    let dscp = (tos >> 2) & 0x3F;
    let ecn = tos & 0x03;

    let q = queue.lock();
    for (&classid, class) in &q.classes {
        if ecn != 0 && class.prio == 0 {
            return classid;
        }
        if dscp >= 46 && class.prio == 0 {
            return classid;
        }
        if dscp >= 34 && class.prio <= 1 {
            return classid;
        }
        if dscp >= 16 && class.prio <= 2 {
            return classid;
        }
    }
    q.default_classid
}

pub fn cgroup_egress_enqueue(
    queue: &Mutex<CgroupClassQueue>,
    packet: CgroupPacket,
    classid: u32,
) -> Result<CgroupEgressAction, CgroupClassError> {
    let mut q = queue.lock();
    if !q.is_enabled() {
        return Ok(CgroupEgressAction::Transmit);
    }
    if !q.class_exists(classid) && classid != q.default_classid {
        return Err(CgroupClassError::ClassNotFound);
    }
    if q.is_queue_full(classid) {
        return Ok(CgroupEgressAction::Drop);
    }
    let current_time = current_time_ms();
    if let Some(bucket) = q.buckets.get_mut(&classid) {
        if !token_bucket_consume(bucket, packet.pkt_len, current_time) {
            return Ok(CgroupEgressAction::Requeue);
        }
    }
    let queue_entry = q.queues.entry(classid).or_insert_with(VecDeque::new);
    queue_entry.push_back(packet);
    q.total_enqueued.fetch_add(1, Ordering::Relaxed);
    Ok(CgroupEgressAction::Transmit)
}

pub fn cgroup_egress_dequeue(
    queue: &Mutex<CgroupClassQueue>,
) -> Option<(CgroupPacket, u32)> {
    let mut q = queue.lock();
    if !q.is_enabled() {
        return None;
    }
    let current_time = current_time_ms();

    let mut classes_sorted: Vec<u32> = q.classes.keys().copied().collect();
    classes_sorted.sort_by_key(|id| q.classes.get(id).map_or(u16::MAX, |c| c.prio));

    for &classid in &classes_sorted {
        let pkt_len = match q.queues.get(&classid) {
            Some(qe) => qe.front().map(|p| p.pkt_len),
            None => continue,
        };
        let pkt_len = match pkt_len {
            Some(l) => l,
            None => continue,
        };
        let allowed = match q.buckets.get_mut(&classid) {
            Some(bucket) => token_bucket_consume(bucket, pkt_len, current_time),
            None => true,
        };
        if allowed {
            if let Some(pkt) = q.queues.get_mut(&classid).and_then(|qe| qe.pop_front()) {
                q.total_dequeued.fetch_add(1, Ordering::Relaxed);
                if let Some(class) = q.classes.get_mut(&classid) {
                    class.update_stats_sent(pkt_len);
                }
                return Some((pkt, classid));
            }
        } else {
            q.total_dropped.fetch_add(1, Ordering::Relaxed);
            if let Some(class) = q.classes.get_mut(&classid) {
                class.update_stats_dropped(pkt_len);
            }
            if let Some(qe) = q.queues.get_mut(&classid) {
                let _ = qe.pop_front();
            }
            return None;
        }
    }
    None
}

pub fn cgroup_set_class_rate(
    queue: &Mutex<CgroupClassQueue>,
    classid: u32,
    rate: u32,
    burst: u32,
) -> Result<(), CgroupClassError> {
    if rate == 0 || rate > CGROUP_MAX_RATE {
        return Err(CgroupClassError::InvalidRate);
    }
    if burst < CGROUP_MIN_BURST || burst > CGROUP_MAX_BURST {
        return Err(CgroupClassError::InvalidBurst);
    }
    let mut q = queue.lock();
    if let Some(class) = q.classes.get_mut(&classid) {
        class.rate_bytes_per_sec = rate;
        class.burst_bytes = burst;
    } else {
        return Err(CgroupClassError::ClassNotFound);
    }
    if let Some(bucket) = q.buckets.get_mut(&classid) {
        let current_time = current_time_ms();
        token_bucket_refill(bucket, current_time);
        bucket.rate = rate;
        bucket.burst = burst;
        if bucket.tokens > burst as i64 {
            bucket.tokens = burst as i64;
        }
    }
    Ok(())
}

pub fn cgroup_get_class_stats(
    queue: &Mutex<CgroupClassQueue>,
    classid: u32,
) -> Option<CgroupClassStats> {
    let q = queue.lock();
    let class = q.classes.get(&classid)?;
    let bucket = q.buckets.get(&classid);
    Some(CgroupClassStats {
        classid,
        bytes_sent: class.bytes_sent,
        packets_sent: class.packets_sent,
        bytes_dropped: class.bytes_dropped,
        packets_dropped: class.packets_dropped,
        tokens_available: bucket.map_or(0, |b| b.tokens),
        rate_bytes_per_sec: class.rate_bytes_per_sec,
        burst_bytes: class.burst_bytes,
        rate_est_bytes_per_sec: class.rate_est_bytes_per_sec,
    })
}

pub fn cgroup_get_all_class_stats(queue: &Mutex<CgroupClassQueue>) -> Vec<CgroupClassStats> {
    let q = queue.lock();
    let mut stats = Vec::new();
    for (&classid, class) in &q.classes {
        let bucket = q.buckets.get(&classid);
        stats.push(CgroupClassStats {
            classid,
            bytes_sent: class.bytes_sent,
            packets_sent: class.packets_sent,
            bytes_dropped: class.bytes_dropped,
            packets_dropped: class.packets_dropped,
            tokens_available: bucket.map_or(0, |b| b.tokens),
            rate_bytes_per_sec: class.rate_bytes_per_sec,
            burst_bytes: class.burst_bytes,
            rate_est_bytes_per_sec: class.rate_est_bytes_per_sec,
        });
    }
    stats
}

pub fn cgroup_refill_all_buckets(queue: &Mutex<CgroupClassQueue>, current_time_ms: u64) {
    let mut q = queue.lock();
    for (_, bucket) in q.buckets.iter_mut() {
        token_bucket_refill(bucket, current_time_ms);
    }
}

pub fn cgroup_drain_queue(queue: &Mutex<CgroupClassQueue>, classid: u32, max_packets: u32) -> Vec<CgroupPacket> {
    let mut q = queue.lock();
    let mut drained = Vec::new();
    if let Some(q_entry) = q.queues.get_mut(&classid) {
        for _ in 0..max_packets {
            if let Some(pkt) = q_entry.pop_front() {
                drained.push(pkt);
            } else {
                break;
            }
        }
    }
    drained
}

pub fn cgroup_purge_all(queue: &Mutex<CgroupClassQueue>) {
    let mut q = queue.lock();
    for (_, q_entry) in q.queues.iter_mut() {
        q_entry.clear();
    }
}

pub fn cgroup_snapshot(queue: &Mutex<CgroupClassQueue>) -> CgroupQueueSnapshot {
    let q = queue.lock();
    let ord = Ordering::Relaxed;
    CgroupQueueSnapshot {
        enabled: q.is_enabled(),
        class_count: q.class_count() as u32,
        total_enqueued: q.total_enqueued.load(ord),
        total_dequeued: q.total_dequeued.load(ord),
        total_dropped: q.total_dropped.load(ord),
        total_queue_depth: q.total_queue_depth(),
        default_classid: q.default_classid,
    }
}

#[derive(Clone, Debug, Default)]
pub struct CgroupQueueSnapshot {
    pub enabled: bool,
    pub class_count: u32,
    pub total_enqueued: u64,
    pub total_dequeued: u64,
    pub total_dropped: u64,
    pub total_queue_depth: u32,
    pub default_classid: u32,
}

pub static CGROUP_CLASS_QUEUE: Mutex<CgroupClassQueue> = Mutex::new(CgroupClassQueue {
    classes: BTreeMap::new(),
    buckets: BTreeMap::new(),
    default_classid: CGROUP_DEFAULT_CLASSID,
    queues: BTreeMap::new(),
    total_enqueued: AtomicU64::new(0),
    total_dequeued: AtomicU64::new(0),
    total_dropped: AtomicU64::new(0),
    max_queue_depth: 1024,
    enabled: AtomicBool::new(true),
});

fn current_time_ms() -> u64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(len: usize) -> Vec<u8> {
        let mut pkt = vec![0u8; len];
        pkt[0] = 0x45;
        pkt[1] = 0x00;
        if len > 9 {
            pkt[9] = 6;
        }
        pkt
    }

    fn make_queue_with_class(classid: u32, rate: u32, burst: u32) -> Mutex<CgroupClassQueue> {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, classid, 0, rate, burst).unwrap();
        queue
    }

    #[test]
    fn test_token_bucket_init() {
        let mut bucket = TokenBucket::default();
        token_bucket_init(&mut bucket, 1000, 2048);
        assert_eq!(bucket.rate, 1000);
        assert_eq!(bucket.burst, 2048);
        assert_eq!(bucket.tokens, 2048);
    }

    #[test]
    fn test_token_bucket_consume_ok() {
        let mut bucket = TokenBucket::new(1000, 2048, 0);
        assert!(token_bucket_consume(&mut bucket, 500, 0));
        assert_eq!(bucket.tokens, 1548);
        assert_eq!(bucket.total_consumed, 500);
    }

    #[test]
    fn test_token_bucket_consume_fail() {
        let mut bucket = TokenBucket::new(100, 200, 0);
        assert!(!token_bucket_consume(&mut bucket, 300, 0));
        assert_eq!(bucket.drops, 1);
        assert_eq!(bucket.tokens, 200);
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(1000, 2000, 0);
        token_bucket_consume(&mut bucket, 1500, 0);
        assert_eq!(bucket.tokens, 500);
        token_bucket_refill(&mut bucket, 1000);
        let expected = 500 + (1000 * 1000 / 1000);
        assert_eq!(bucket.tokens, expected);
    }

    #[test]
    fn test_token_bucket_refill_capped() {
        let mut bucket = TokenBucket::new(10000, 2000, 0);
        token_bucket_consume(&mut bucket, 100, 0);
        assert_eq!(bucket.tokens, 1900);
        token_bucket_refill(&mut bucket, 5000);
        assert!(bucket.tokens <= 2000);
    }

    #[test]
    fn test_token_bucket_peek() {
        let bucket = TokenBucket::new(1000, 2000, 0);
        assert!(token_bucket_peek(&bucket, 1000, 0));
        assert!(token_bucket_peek(&bucket, 2000, 0));
        assert!(!token_bucket_peek(&bucket, 3000, 0));
    }

    #[test]
    fn test_token_bucket_utilization() {
        let mut bucket = TokenBucket::new(1000, 1000, 0);
        assert_eq!(bucket.utilization_pct(), 0);
        token_bucket_consume(&mut bucket, 500, 0);
        assert_eq!(bucket.utilization_pct(), 50);
    }

    #[test]
    fn test_token_bucket_utilization_zero_burst() {
        let bucket = TokenBucket::new(1000, 0, 0);
        assert_eq!(bucket.utilization_pct(), 0);
    }

    #[test]
    fn test_token_bucket_is_empty() {
        let mut bucket = TokenBucket::new(100, 200, 0);
        assert!(!bucket.is_empty());
        token_bucket_consume(&mut bucket, 200, 0);
        assert!(bucket.is_empty());
    }

    #[test]
    fn test_cgroup_create_class() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert!(cgroup_create_class(&queue, 1, 0, 1000, 2048).is_ok());
        assert!(queue.lock().class_exists(1));
    }

    #[test]
    fn test_cgroup_create_class_duplicate() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        assert_eq!(
            cgroup_create_class(&queue, 1, 0, 1000, 2048),
            Err(CgroupClassError::ClassAlreadyExists)
        );
    }

    #[test]
    fn test_cgroup_create_class_invalid_rate() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert_eq!(
            cgroup_create_class(&queue, 1, 0, 0, 2048),
            Err(CgroupClassError::InvalidRate)
        );
        assert_eq!(
            cgroup_create_class(&queue, 1, 0, CGROUP_MAX_RATE + 1, 2048),
            Err(CgroupClassError::InvalidRate)
        );
    }

    #[test]
    fn test_cgroup_create_class_invalid_burst() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert_eq!(
            cgroup_create_class(&queue, 1, 0, 1000, 100),
            Err(CgroupClassError::InvalidBurst)
        );
        assert_eq!(
            cgroup_create_class(&queue, 1, 0, 1000, CGROUP_MAX_BURST + 1),
            Err(CgroupClassError::InvalidBurst)
        );
    }

    #[test]
    fn test_cgroup_destroy_class() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        assert!(cgroup_destroy_class(&queue, 1).is_ok());
        assert!(!queue.lock().class_exists(1));
    }

    #[test]
    fn test_cgroup_destroy_class_not_found() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert_eq!(
            cgroup_destroy_class(&queue, 999),
            Err(CgroupClassError::ClassNotFound)
        );
    }

    #[test]
    fn test_cgroup_set_class_rate() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        assert!(cgroup_set_class_rate(&queue, 1, 5000, 4096).is_ok());
        let class = queue.lock().get_class(1).cloned().unwrap();
        assert_eq!(class.rate_bytes_per_sec, 5000);
        assert_eq!(class.burst_bytes, 4096);
    }

    #[test]
    fn test_cgroup_set_class_rate_not_found() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert_eq!(
            cgroup_set_class_rate(&queue, 999, 1000, 2048),
            Err(CgroupClassError::ClassNotFound)
        );
    }

    #[test]
    fn test_cgroup_set_class_rate_invalid() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        assert_eq!(
            cgroup_set_class_rate(&queue, 1, 0, 2048),
            Err(CgroupClassError::InvalidRate)
        );
    }

    #[test]
    fn test_cgroup_classify_packet_short() {
        let queue = Mutex::new(CgroupClassQueue::new());
        let pkt = make_packet(5);
        let classid = cgroup_classify_packet(&queue, &pkt);
        assert_eq!(classid, CGROUP_DEFAULT_CLASSID);
    }

    #[test]
    fn test_cgroup_classify_packet_ipv6() {
        let queue = Mutex::new(CgroupClassQueue::new());
        let mut pkt = vec![0u8; 40];
        pkt[0] = 0x60;
        let classid = cgroup_classify_packet(&queue, &pkt);
        assert_eq!(classid, CGROUP_DEFAULT_CLASSID);
    }

    #[test]
    fn test_cgroup_classify_packet_dscp() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 10, 0, 10000, 20480).unwrap();
        cgroup_create_class(&queue, 20, 1, 5000, 10240).unwrap();
        cgroup_create_class(&queue, 30, 2, 1000, 2048).unwrap();
        let mut pkt = make_packet(40);
        pkt[1] = 0xB8;
        let classid = cgroup_classify_packet(&queue, &pkt);
        assert!(queue.lock().class_exists(classid));
    }

    #[test]
    fn test_cgroup_egress_enqueue_dequeue() {
        let queue = make_queue_with_class(1, 1_000_000, 204800);
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let action = cgroup_egress_enqueue(&queue, pkt, 1).unwrap();
        assert_eq!(action, CgroupEgressAction::Transmit);
        let result = cgroup_egress_dequeue(&queue);
        assert!(result.is_some());
        let (pkt, classid) = result.unwrap();
        assert_eq!(classid, 1);
        assert_eq!(pkt.pkt_len, 64);
    }

    #[test]
    fn test_cgroup_egress_enqueue_drop() {
        let queue = Mutex::new(CgroupClassQueue::new().with_max_queue_depth(2));
        cgroup_create_class(&queue, 1, 0, 1, CGROUP_MIN_BURST).unwrap();
        let p1 = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let p2 = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let p3 = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, p1, 1).unwrap();
        let _ = cgroup_egress_enqueue(&queue, p2, 1).unwrap();
        let action = cgroup_egress_enqueue(&queue, p3, 1).unwrap();
        assert_eq!(action, CgroupEgressAction::Drop);
    }

    #[test]
    fn test_cgroup_egress_enqueue_requeue() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 100, CGROUP_MIN_BURST).unwrap();
        for _ in 0..20 {
            let pkt = CgroupPacket::new(make_packet(100), 1, 0, 0);
            let _ = cgroup_egress_enqueue(&queue, pkt, 1).unwrap();
        }
        let big_pkt = CgroupPacket::new(make_packet(300), 1, 0, 0);
        let action = cgroup_egress_enqueue(&queue, big_pkt, 1).unwrap();
        assert_eq!(action, CgroupEgressAction::Requeue);
    }

    #[test]
    fn test_cgroup_egress_enqueue_disabled() {
        let queue = Mutex::new(CgroupClassQueue::new());
        queue.lock().disable();
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let action = cgroup_egress_enqueue(&queue, pkt, 1).unwrap();
        assert_eq!(action, CgroupEgressAction::Transmit);
    }

    #[test]
    fn test_cgroup_egress_enqueue_nonexistent_class() {
        let queue = Mutex::new(CgroupClassQueue::new());
        let pkt = CgroupPacket::new(make_packet(64), 999, 0, 0);
        assert_eq!(
            cgroup_egress_enqueue(&queue, pkt, 999),
            Err(CgroupClassError::ClassNotFound)
        );
    }

    #[test]
    fn test_cgroup_egress_dequeue_empty() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert!(cgroup_egress_dequeue(&queue).is_none());
    }

    #[test]
    fn test_cgroup_egress_dequeue_disabled() {
        let queue = make_queue_with_class(1, 1_000_000, 204800);
        queue.lock().disable();
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1).unwrap();
        assert!(cgroup_egress_dequeue(&queue).is_none());
    }

    #[test]
    fn test_cgroup_get_class_stats() {
        let queue = make_queue_with_class(1, 1000, 2048);
        let stats = cgroup_get_class_stats(&queue, 1).unwrap();
        assert_eq!(stats.classid, 1);
        assert_eq!(stats.rate_bytes_per_sec, 1000);
        assert_eq!(stats.burst_bytes, 2048);
        assert_eq!(stats.bytes_sent, 0);
    }

    #[test]
    fn test_cgroup_get_class_stats_not_found() {
        let queue = Mutex::new(CgroupClassQueue::new());
        assert!(cgroup_get_class_stats(&queue, 999).is_none());
    }

    #[test]
    fn test_cgroup_get_all_class_stats() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        cgroup_create_class(&queue, 2, 1, 2000, 4096).unwrap();
        let stats = cgroup_get_all_class_stats(&queue);
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_cgroup_refill_all_buckets() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        let mut q = queue.lock();
        let bucket = q.get_bucket_mut(1).unwrap();
        bucket.tokens = 0;
        drop(q);
        cgroup_refill_all_buckets(&queue, 2000);
        let q = queue.lock();
        let bucket = q.get_bucket(1).unwrap();
        assert!(bucket.tokens > 0);
    }

    #[test]
    fn test_cgroup_drain_queue() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        for i in 0..5 {
            let pkt = CgroupPacket::new(make_packet(64), 1, 0, i);
            let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        }
        let drained = cgroup_drain_queue(&queue, 1, 3);
        assert_eq!(drained.len(), 3);
        assert_eq!(queue.lock().queue_depth(1), 2);
    }

    #[test]
    fn test_cgroup_purge_all() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        for i in 0..5 {
            let pkt = CgroupPacket::new(make_packet(64), 1, 0, i);
            let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        }
        cgroup_purge_all(&queue);
        assert_eq!(queue.lock().queue_depth(1), 0);
    }

    #[test]
    fn test_cgroup_snapshot() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        let snap = cgroup_snapshot(&queue);
        assert!(snap.enabled);
        assert_eq!(snap.class_count, 1);
        assert_eq!(snap.total_enqueued, 0);
    }

    #[test]
    fn test_cgroup_queue_depth() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        assert_eq!(queue.lock().queue_depth(1), 0);
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        assert_eq!(queue.lock().queue_depth(1), 1);
    }

    #[test]
    fn test_cgroup_total_queue_depth() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        cgroup_create_class(&queue, 2, 1, 1000, 2048).unwrap();
        let p1 = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let p2 = CgroupPacket::new(make_packet(64), 2, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, p1, 1);
        let _ = cgroup_egress_enqueue(&queue, p2, 2);
        assert_eq!(queue.lock().total_queue_depth(), 2);
    }

    #[test]
    fn test_cgroup_enable_disable() {
        let queue = Mutex::new(CgroupClassQueue::new());
        queue.lock().disable();
        assert!(!queue.lock().is_enabled());
        queue.lock().enable();
        assert!(queue.lock().is_enabled());
    }

    #[test]
    fn test_cgroup_set_default_class() {
        let queue = Mutex::new(CgroupClassQueue::new());
        queue.lock().set_default_class(100);
        assert_eq!(queue.lock().default_classid, 100);
    }

    #[test]
    fn test_cgroup_stats_after_dequeue() {
        let queue = make_queue_with_class(1, 1_000_000, 204800);
        let pkt = CgroupPacket::new(make_packet(128), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1).unwrap();
        let _ = cgroup_egress_dequeue(&queue);
        let stats = cgroup_get_class_stats(&queue, 1).unwrap();
        assert_eq!(stats.packets_sent, 1);
        assert_eq!(stats.bytes_sent, 128);
    }

    #[test]
    fn test_cgroup_token_bucket_rate_change() {
        let queue = make_queue_with_class(1, 1000, 2048);
        cgroup_set_class_rate(&queue, 1, 5000, 4096).unwrap();
        let q = queue.lock();
        let class = q.get_class(1).unwrap();
        assert_eq!(class.rate_bytes_per_sec, 5000);
        assert_eq!(class.burst_bytes, 4096);
        let bucket = q.get_bucket(1).unwrap();
        assert_eq!(bucket.rate, 5000);
        assert_eq!(bucket.burst, 4096);
    }

    #[test]
    fn test_cgroup_packet_new() {
        let data = make_packet(64);
        let pkt = CgroupPacket::new(data, 1, 2, 1000);
        assert_eq!(pkt.classid, 1);
        assert_eq!(pkt.priority, 2);
        assert_eq!(pkt.enqueue_time, 1000);
        assert_eq!(pkt.pkt_len, 64);
    }

    #[test]
    fn test_cgroup_classify_packet_ecn() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 10, 0, 10000, 20480).unwrap();
        let mut pkt = make_packet(40);
        pkt[1] = 0x01;
        let classid = cgroup_classify_packet(&queue, &pkt);
        assert!(queue.lock().class_exists(classid));
    }

    #[test]
    fn test_cgroup_classify_packet_default() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 10, 3, 10000, 20480).unwrap();
        let pkt = make_packet(40);
        let classid = cgroup_classify_packet(&queue, &pkt);
        assert_eq!(classid, CGROUP_DEFAULT_CLASSID);
    }

    #[test]
    fn test_cgroup_egress_multiple_classes_priority() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 10, 0, 1_000_000, 204800).unwrap();
        cgroup_create_class(&queue, 20, 1, 1_000_000, 204800).unwrap();
        let p1 = CgroupPacket::new(make_packet(64), 10, 0, 0);
        let p2 = CgroupPacket::new(make_packet(64), 20, 1, 0);
        let _ = cgroup_egress_enqueue(&queue, p1, 10);
        let _ = cgroup_egress_enqueue(&queue, p2, 20);
        let (pkt, classid) = cgroup_egress_dequeue(&queue).unwrap();
        assert_eq!(classid, 10);
        assert_eq!(pkt.classid, 10);
    }

    #[test]
    fn test_cgroup_token_bucket_consume_exact() {
        let mut bucket = TokenBucket::new(1000, 1000, 0);
        assert!(token_bucket_consume(&mut bucket, 1000, 0));
        assert_eq!(bucket.tokens, 0);
    }

    #[test]
    fn test_cgroup_token_bucket_refill_zero_rate() {
        let mut bucket = TokenBucket::new(0, 1000, 0);
        token_bucket_refill(&mut bucket, 1000);
        assert_eq!(bucket.tokens, 0);
    }

    #[test]
    fn test_cgroup_token_bucket_refill_zero_elapsed() {
        let mut bucket = TokenBucket::new(1000, 1000, 1000);
        token_bucket_consume(&mut bucket, 500, 1000);
        token_bucket_refill(&mut bucket, 1000);
        assert_eq!(bucket.tokens, 500);
    }

    #[test]
    fn test_cgroup_class_update_stats() {
        let mut class = CgroupClassify::new(1, 0, 1000, 2048);
        class.update_stats_sent(100);
        class.update_stats_sent(200);
        class.update_stats_dropped(50);
        assert_eq!(class.packets_sent, 2);
        assert_eq!(class.bytes_sent, 300);
        assert_eq!(class.packets_dropped, 1);
        assert_eq!(class.bytes_dropped, 50);
    }

    #[test]
    fn test_cgroup_queue_full_per_class() {
        let queue = Mutex::new(CgroupClassQueue::new().with_max_queue_depth(1));
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        cgroup_create_class(&queue, 2, 0, 1000, 2048).unwrap();
        let p1 = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let p2 = CgroupPacket::new(make_packet(64), 2, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, p1, 1);
        let _ = cgroup_egress_enqueue(&queue, p2, 2);
        assert!(queue.lock().is_queue_full(1));
        assert!(queue.lock().is_queue_full(2));
    }

    #[test]
    fn test_cgroup_get_all_stats_populated() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        let _ = cgroup_egress_dequeue(&queue);
        let stats = cgroup_get_all_class_stats(&queue);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].bytes_sent, 64);
    }

    #[test]
    fn test_cgroup_drain_queue_more_than_available() {
        let queue = Mutex::new(CgroupClassQueue::new());
        cgroup_create_class(&queue, 1, 0, 1000, 2048).unwrap();
        let pkt = CgroupPacket::new(make_packet(64), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        let drained = cgroup_drain_queue(&queue, 1, 100);
        assert_eq!(drained.len(), 1);
    }

    #[test]
    fn test_cgroup_drain_queue_nonexistent() {
        let queue = Mutex::new(CgroupClassQueue::new());
        let drained = cgroup_drain_queue(&queue, 999, 10);
        assert!(drained.is_empty());
    }

    #[test]
    fn test_cgroup_token_bucket_consume_and_refill_cycle() {
        let mut bucket = TokenBucket::new(1000, 2000, 0);
        for _ in 0..4 {
            assert!(token_bucket_consume(&mut bucket, 500, 0));
        }
        assert_eq!(bucket.tokens, 0);
        assert!(!token_bucket_consume(&mut bucket, 500, 0));
        assert_eq!(bucket.tokens, 0);
        token_bucket_refill(&mut bucket, 1000);
        assert!(bucket.tokens > 0);
    }

    #[test]
    fn test_cgroup_stats_tokens_available() {
        let queue = make_queue_with_class(1, 1000, 2048);
        let pkt = CgroupPacket::new(make_packet(500), 1, 0, 0);
        let _ = cgroup_egress_enqueue(&queue, pkt, 1);
        let _ = cgroup_egress_dequeue(&queue);
        let stats = cgroup_get_class_stats(&queue, 1).unwrap();
        assert!(stats.tokens_available < 2048);
    }
}
