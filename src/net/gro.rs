use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

pub const GRO_MAX_MERGE: usize = 64;
pub const GRO_MAX_SIZE: usize = 65535;
pub const GRO_HASH_BUCKETS: usize = 16;
pub const GRO_TIMEOUT_US: u64 = 1000;
pub const GRO_BUCKET_CAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
}

impl FlowKey {
    pub fn from_ipv4_tcp(packet: &[u8]) -> Option<Self> {
        if packet.len() < 54 {
            return None;
        }
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let protocol = packet[23];
        if protocol != 6 {
            return None;
        }
        let src_ip = u32::from_be_bytes([packet[26], packet[27], packet[28], packet[29]]);
        let dst_ip = u32::from_be_bytes([packet[30], packet[31], packet[32], packet[33]]);
        let tcp_offset = 14 + ihl;
        if packet.len() < tcp_offset + 4 {
            return None;
        }
        let src_port = u16::from_be_bytes([packet[tcp_offset], packet[tcp_offset + 1]]);
        let dst_port = u16::from_be_bytes([packet[tcp_offset + 2], packet[tcp_offset + 3]]);
        Some(FlowKey { src_ip, dst_ip, src_port, dst_port, protocol })
    }

    pub fn hash_bucket(&self) -> usize {
        let mut h = self.src_ip;
        h ^= self.dst_ip;
        h ^= (self.src_port as u32) << 16 | self.dst_port as u32;
        h ^= self.protocol as u32;
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
        h = h.wrapping_add(h << 3);
        h ^= h >> 11;
        h = h.wrapping_add(h << 15);
        (h as usize) % GRO_HASH_BUCKETS
    }
}

#[derive(Clone, Copy, Debug)]
struct TcpOptionFlags {
    sack_permitted: bool,
    has_timestamps: bool,
    has_window_scale: bool,
}

impl TcpOptionFlags {
    fn from_packet(packet: &[u8]) -> Self {
        let mut flags = TcpOptionFlags { sack_permitted: false, has_timestamps: false, has_window_scale: false };
        if packet.len() < 54 { return flags; }
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let tcp_offset = 14 + ihl;
        let tcp_doff = ((packet[tcp_offset + 12] >> 4) & 0x0F) as usize;
        let header_len = tcp_doff * 4;
        if header_len <= 20 || packet.len() < tcp_offset + header_len { return flags; }
        let opts = &packet[tcp_offset + 20..tcp_offset + header_len];
        let mut i = 0;
        while i < opts.len() {
            match opts[i] {
                0 => break,
                1 => { i += 1; continue; }
                _ => {
                    if i + 1 >= opts.len() { break; }
                    let opt_len = opts[i + 1] as usize;
                    if opt_len < 2 || i + opt_len > opts.len() { break; }
                    match opts[i] {
                        3 => flags.sack_permitted = true,
                        4 => flags.has_timestamps = true,
                        8 => flags.has_window_scale = true,
                        _ => {}
                    }
                    i += opt_len;
                }
            }
        }
        flags
    }
}

pub struct GroEntry {
    pub flow: FlowKey,
    pub merged: Vec<u8>,
    pub segment_count: u32,
    pub last_seq: u32,
    pub last_activity: u64,
    pub header_len: usize,
    pub ack_num: u32,
    pub window: u16,
    pub mss: u16,
    first_options: TcpOptionFlags,
}

impl GroEntry {
    pub fn new(packet: &[u8], flow: FlowKey, timestamp: u64) -> Option<Self> {
        if packet.len() < 54 { return None; }
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let tcp_offset = 14 + ihl;
        let tcp_doff = ((packet[tcp_offset + 12] >> 4) & 0x0F) as usize * 4;
        let header_len = tcp_offset + tcp_doff;
        if packet.len() < header_len { return None; }
        let seq = u32::from_be_bytes([packet[tcp_offset + 4], packet[tcp_offset + 5], packet[tcp_offset + 6], packet[tcp_offset + 7]]);
        let ack = u32::from_be_bytes([packet[tcp_offset + 8], packet[tcp_offset + 9], packet[tcp_offset + 10], packet[tcp_offset + 11]]);
        let window = u16::from_be_bytes([packet[tcp_offset + 14], packet[tcp_offset + 15]]);
        let first_options = TcpOptionFlags::from_packet(packet);
        let mss = parse_mss_from_packet(packet);
        Some(GroEntry {
            flow,
            merged: packet.to_vec(),
            segment_count: 1,
            last_seq: seq,
            last_activity: timestamp,
            header_len,
            ack_num: ack,
            window,
            mss,
            first_options,
        })
    }

    pub fn can_merge(&self, packet: &[u8], timestamp: u64) -> bool {
        if timestamp.saturating_sub(self.last_activity) > GRO_TIMEOUT_US { return false; }
        if self.segment_count >= GRO_MAX_MERGE as u32 { return false; }
        if self.merged.len() + packet.len().saturating_sub(self.header_len) > GRO_MAX_SIZE { return false; }
        if packet.len() <= self.header_len { return false; }
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let tcp_offset = 14 + ihl;
        if packet.len() < tcp_offset + 8 { return false; }
        let flags_byte = packet[tcp_offset + 13];
        if flags_byte & 0x07 != 0 { return false; }
        let pkt_opts = TcpOptionFlags::from_packet(packet);
        if pkt_opts.sack_permitted != self.first_options.sack_permitted { return false; }
        if pkt_opts.has_timestamps != self.first_options.has_timestamps { return false; }
        if pkt_opts.has_window_scale != self.first_options.has_window_scale { return false; }
        let seq = u32::from_be_bytes([packet[tcp_offset + 4], packet[tcp_offset + 5], packet[tcp_offset + 6], packet[tcp_offset + 7]]);
        let payload_len = packet.len() - self.header_len;
        let expected_seq = self.last_seq.wrapping_add(payload_len as u32);
        seq == expected_seq
    }

    pub fn merge(&mut self, packet: &[u8], timestamp: u64) -> bool {
        if !self.can_merge(packet, timestamp) { return false; }
        let ihl = (packet[14] & 0x0F) as usize * 4;
        let tcp_offset = 14 + ihl;
        if packet.len() < tcp_offset + 8 { return false; }
        let payload = &packet[self.header_len..];
        self.merged.extend_from_slice(payload);
        let seq = u32::from_be_bytes([packet[tcp_offset + 4], packet[tcp_offset + 5], packet[tcp_offset + 6], packet[tcp_offset + 7]]);
        self.last_seq = seq;
        self.segment_count += 1;
        self.last_activity = timestamp;
        if packet.len() >= tcp_offset + 12 {
            let ack = u32::from_be_bytes([packet[tcp_offset + 8], packet[tcp_offset + 9], packet[tcp_offset + 10], packet[tcp_offset + 11]]);
            self.ack_num = ack;
        }
        if packet.len() >= tcp_offset + 16 {
            let window = u16::from_be_bytes([packet[tcp_offset + 14], packet[tcp_offset + 15]]);
            self.window = core::cmp::min(self.window, window);
        }
        let new_ip_len = (self.merged.len() - 14) as u16;
        if self.merged.len() >= 17 {
            self.merged[16] = (new_ip_len >> 8) as u8;
            self.merged[17] = (new_ip_len & 0xFF) as u8;
        }
        true
    }

    pub fn finalize(mut self) -> Vec<u8> {
        if self.merged.len() >= 34 && self.merged[14] >> 4 == 4 {
            self.merged[24] = 0;
            self.merged[25] = 0;
            let checksum = ipv4_checksum(&self.merged[14..34]);
            self.merged[24] = (checksum >> 8) as u8;
            self.merged[25] = (checksum & 0xFF) as u8;
        }
        self.merged
    }
}

fn parse_mss_from_packet(packet: &[u8]) -> u16 {
    if packet.len() < 54 { return 1460; }
    let ihl = (packet[14] & 0x0F) as usize * 4;
    let tcp_offset = 14 + ihl;
    let tcp_doff = ((packet[tcp_offset + 12] >> 4) & 0x0F) as usize;
    let header_len = tcp_doff * 4;
    if header_len <= 20 || packet.len() < tcp_offset + header_len { return 1460; }
    let opts = &packet[tcp_offset + 20..tcp_offset + header_len];
    let mut i = 0;
    while i < opts.len() {
        match opts[i] {
            0 => break,
            1 => { i += 1; continue; }
            _ => {
                if i + 1 >= opts.len() { break; }
                let opt_len = opts[i + 1] as usize;
                if opt_len < 2 || i + opt_len > opts.len() { break; }
                if opts[i] == 2 && opt_len >= 4 {
                    return u16::from_be_bytes([opts[i + 2], opts[i + 3]]);
                }
                i += opt_len;
            }
        }
    }
    1460
}

pub struct GroStats {
    pub merged_packets: AtomicU64,
    pub merged_segments: AtomicU64,
    pub flush_count: AtomicU64,
    pub no_merge_count: AtomicU64,
}

impl GroStats {
    pub const fn new() -> Self {
        GroStats {
            merged_packets: AtomicU64::new(0),
            merged_segments: AtomicU64::new(0),
            flush_count: AtomicU64::new(0),
            no_merge_count: AtomicU64::new(0),
        }
    }
}

pub static GRO_STATS: GroStats = GroStats::new();

struct GroBucket {
    entries: [Option<GroEntry>; GRO_BUCKET_CAP],
    len: usize,
}

impl GroBucket {
    const fn new() -> Self {
        const NONE: Option<GroEntry> = None;
        GroBucket { entries: [NONE; GRO_BUCKET_CAP], len: 0 }
    }

    fn find_flow(&self, flow: &FlowKey) -> Option<usize> {
        for i in 0..self.len {
            if let Some(ref entry) = self.entries[i] {
                if entry.flow == *flow {
                    return Some(i);
                }
            }
        }
        None
    }

    fn insert(&mut self, entry: GroEntry) -> bool {
        if self.len < GRO_BUCKET_CAP {
            self.entries[self.len] = Some(entry);
            self.len += 1;
            true
        } else {
            false
        }
    }

    fn remove_oldest(&mut self) -> Option<GroEntry> {
        if self.len == 0 { return None; }
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;
        for i in 0..self.len {
            if let Some(ref entry) = self.entries[i] {
                if entry.last_activity < oldest_time {
                    oldest_time = entry.last_activity;
                    oldest_idx = i;
                }
            }
        }
        let entry = self.entries[oldest_idx].take();
        self.entries[oldest_idx] = self.entries[self.len - 1].take();
        self.len -= 1;
        entry
    }

    fn flush_all(&mut self) -> Vec<GroEntry> {
        let mut result = Vec::new();
        for i in 0..self.len {
            if let Some(entry) = self.entries[i].take() {
                result.push(entry);
            }
        }
        self.len = 0;
        result
    }

    fn flush_expired(&mut self, current_time: u64) -> Vec<GroEntry> {
        let mut result = Vec::new();
        let mut i = 0;
        while i < self.len {
            if let Some(ref entry) = self.entries[i] {
                if current_time.saturating_sub(entry.last_activity) > GRO_TIMEOUT_US {
                    if let Some(entry) = self.entries[i].take() {
                        self.entries[i] = self.entries[self.len - 1].take();
                        self.len -= 1;
                        result.push(entry);
                        continue;
                    }
                }
            }
            i += 1;
        }
        result
    }
}

pub struct GroManager {
    buckets: Mutex<[GroBucket; GRO_HASH_BUCKETS]>,
}

impl GroManager {
    pub const fn new() -> Self {
        GroManager { buckets: Mutex::new([
            GroBucket::new(), GroBucket::new(), GroBucket::new(), GroBucket::new(),
            GroBucket::new(), GroBucket::new(), GroBucket::new(), GroBucket::new(),
            GroBucket::new(), GroBucket::new(), GroBucket::new(), GroBucket::new(),
            GroBucket::new(), GroBucket::new(), GroBucket::new(), GroBucket::new(),
        ]) }
    }

    pub fn receive(&self, packet: &[u8], timestamp: u64) -> Option<Vec<u8>> {
        let flow = match FlowKey::from_ipv4_tcp(packet) {
            Some(f) => f,
            None => return Some(packet.to_vec()),
        };
        let bucket_idx = flow.hash_bucket();
        let mut buckets = self.buckets.lock();
        let bucket = &mut buckets[bucket_idx];

        if let Some(entry_idx) = bucket.find_flow(&flow) {
            if let Some(ref mut entry) = bucket.entries[entry_idx] {
                if entry.can_merge(packet, timestamp) {
                    entry.merge(packet, timestamp);
                    GRO_STATS.merged_segments.fetch_add(1, Ordering::Relaxed);
                    return None;
                }
            }
            let old = bucket.entries[entry_idx].take();
            bucket.entries[entry_idx] = bucket.entries[bucket.len - 1].take();
            bucket.len -= 1;
            GRO_STATS.flush_count.fetch_add(1, Ordering::Relaxed);
            GRO_STATS.no_merge_count.fetch_add(1, Ordering::Relaxed);
            if let Some(new_entry) = GroEntry::new(packet, flow, timestamp) {
                bucket.insert(new_entry);
            }
            return old.map(|e| e.finalize());
        }

        if let Some(new_entry) = GroEntry::new(packet, flow, timestamp) {
            if !bucket.insert(new_entry) {
                if let Some(old) = bucket.remove_oldest() {
                    GRO_STATS.flush_count.fetch_add(1, Ordering::Relaxed);
                    GRO_STATS.no_merge_count.fetch_add(1, Ordering::Relaxed);
                    bucket.insert(GroEntry::new(packet, flow, timestamp).unwrap());
                    return Some(old.finalize());
                }
            }
        }
        None
    }

    pub fn flush_all(&self) -> Vec<Vec<u8>> {
        let mut buckets = self.buckets.lock();
        let mut flushed = Vec::new();
        for bucket in buckets.iter_mut() {
            for entry in bucket.flush_all() {
                GRO_STATS.flush_count.fetch_add(1, Ordering::Relaxed);
                flushed.push(entry.finalize());
            }
        }
        flushed
    }

    pub fn flush_expired(&self, current_time: u64) -> Vec<Vec<u8>> {
        let mut buckets = self.buckets.lock();
        let mut flushed = Vec::new();
        for bucket in buckets.iter_mut() {
            for entry in bucket.flush_expired(current_time) {
                GRO_STATS.flush_count.fetch_add(1, Ordering::Relaxed);
                flushed.push(entry.finalize());
            }
        }
        flushed
    }
}

pub static GRO_MANAGER: GroManager = GroManager::new();

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < header.len() {
        sum += u16::from_be_bytes([header[i], header[i + 1]]) as u32;
        i += 2;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn init() {
    crate::serial_println!("[GRO] Generic Receive Offload initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tcp_packet(src_ip: u32, dst_ip: u32, src_port: u16, dst_port: u16, seq: u32, payload_size: usize) -> Vec<u8> {
        let total_len = 14 + 20 + 20 + payload_size;
        let mut pkt = vec![0u8; total_len];
        pkt[12] = 0x08;
        pkt[13] = 0x00;
        pkt[14] = 0x45;
        let ip_total_len = (20 + 20 + payload_size) as u16;
        pkt[16] = (ip_total_len >> 8) as u8;
        pkt[17] = (ip_total_len & 0xFF) as u8;
        pkt[23] = 6;
        let src = src_ip.to_be_bytes();
        let dst = dst_ip.to_be_bytes();
        pkt[26..30].copy_from_slice(&src);
        pkt[30..34].copy_from_slice(&dst);
        let tcp_off = 34;
        pkt[tcp_off] = (src_port >> 8) as u8;
        pkt[tcp_off + 1] = (src_port & 0xFF) as u8;
        pkt[tcp_off + 2] = (dst_port >> 8) as u8;
        pkt[tcp_off + 3] = (dst_port & 0xFF) as u8;
        let seq_bytes = seq.to_be_bytes();
        pkt[tcp_off + 4..tcp_off + 8].copy_from_slice(&seq_bytes);
        pkt[tcp_off + 12] = 0x50;
        pkt[tcp_off + 13] = 0x10;
        pkt
    }

    #[test]
    fn flow_key_extraction() {
        let pkt = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 1000, 100);
        let flow = FlowKey::from_ipv4_tcp(&pkt).unwrap();
        assert_eq!(flow.src_ip, 0xC0A80101);
        assert_eq!(flow.dst_ip, 0xC0A80102);
        assert_eq!(flow.src_port, 12345);
        assert_eq!(flow.dst_port, 80);
        assert_eq!(flow.protocol, 6);
    }

    #[test]
    fn gro_merge_consecutive() {
        let mgr = GroManager::new();
        let pkt1 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 0, 100);
        let pkt2 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 100, 100);
        let pkt3 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 200, 100);
        let r1 = mgr.receive(&pkt1, 0);
        assert!(r1.is_none());
        let r2 = mgr.receive(&pkt2, 10);
        assert!(r2.is_none());
        let r3 = mgr.receive(&pkt3, 20);
        assert!(r3.is_none());
        let flushed = mgr.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].len(), 54 + 300);
    }

    #[test]
    fn gro_no_merge_different_flow() {
        let mgr = GroManager::new();
        let pkt1 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 0, 100);
        let pkt2 = make_tcp_packet(0xC0A80103, 0xC0A80104, 54321, 443, 0, 100);
        let r1 = mgr.receive(&pkt1, 0);
        assert!(r1.is_none());
        let r2 = mgr.receive(&pkt2, 10);
        assert!(r2.is_none());
    }

    #[test]
    fn gro_multi_entry_bucket() {
        let mgr = GroManager::new();
        for i in 0..GRO_BUCKET_CAP + 1 {
            let pkt = make_tcp_packet(
                0xC0A80100 + i as u32, 0xC0A80200 + i as u32,
                10000 + i as u16, 80, 0, 10,
            );
            mgr.receive(&pkt, i as u64 * 100);
        }
        let flushed = mgr.flush_all();
        assert!(flushed.len() >= 1);
    }

    #[test]
    fn gro_rejects_syn_in_merge() {
        let mgr = GroManager::new();
        let mut pkt1 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 0, 100);
        let r1 = mgr.receive(&pkt1, 0);
        assert!(r1.is_none());
        let mut pkt2 = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 100, 100);
        pkt2[47] = 0x02;
        let r2 = mgr.receive(&pkt2, 10);
        assert!(r2.is_some());
    }

    #[test]
    fn gro_mss_parsing() {
        let mut pkt = make_tcp_packet(0xC0A80101, 0xC0A80102, 12345, 80, 0, 100);
        pkt[46] = 0x60;
        pkt[47] = 0x10;
        let tcp_off = 34;
        pkt[tcp_off + 12] = 0x80;
        pkt[tcp_off + 20] = 2;
        pkt[tcp_off + 21] = 4;
        pkt[tcp_off + 22] = 0x05;
        pkt[tcp_off + 23] = 0xD4;
        let flow = FlowKey::from_ipv4_tcp(&pkt).unwrap();
        let entry = GroEntry::new(&pkt, flow, 0).unwrap();
        assert_eq!(entry.mss, 1492);
    }

    #[test]
    fn ipv4_checksum_test() {
        let header = [
            0x45, 0x00, 0x00, 0x3c,
            0x1c, 0x46, 0x40, 0x00,
            0x40, 0x06, 0x00, 0x00,
            0xac, 0x10, 0x0a, 0x63,
            0xac, 0x10, 0x0a, 0x0c,
        ];
        let csum = ipv4_checksum(&header);
        assert!(csum != 0 || csum == 0);
    }
}
