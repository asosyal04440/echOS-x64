//! # LRO (Large Receive Offload)
//!
//! LRO, NIC veya sürücünün birden fazla küçük TCP segmentini tek büyük
//! bir segmente birleştirerek yukarı katmana (TCP) verim vermesini sağlar.
//!
//! ## LRO vs GRO (Generic Receive Offload)
//!
//! - **LRO**: Donanım (NIC) veya sürücü özel. Patent sorunları var (Linux 2.6 era).
//! - **GRO**: Yazılım genel. Linux 2.6.18+ ile geldi. BSD ve diğer OS'lerde de var.
//! - **TSO**: Gönderim tarafı (TCP → NIC).
//!
//! ## LRO Koşulları
//!
//! Segmentleri birleştirmek için aşağıdakilerin hepsi doğru olmalı:
//! 1. Aynı kaynak ve hedef IP (IPv4 veya IPv6)
//! 2. Aynı kaynak ve hedef port
//! 3. Aynı TCP bayrakları (SYN, FIN, RST hariç)
//! 4. Aynı IP ID (donanım LRO genellikle aynı IP ID kullanır)
//! 5. Segmentin başlangıç seq'i, bir önceki segmentin end_seq'ine eşit
//! 6. Aynı TCP option'lar (timestamps vs.)
//! 7. Timestamp varsa, TSval ardışık ve TSecr eşleşmeli
//!
//! ## echOS Tasarımı
//!
//! `LroAggregator` akış başına TCP segment birleştirici. Maks. 64KB
//! birleştirilmiş segment üretir (Linux LRO_MAX_PG_SIZE).

use super::{Ipv4Addr, Mutex};
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// LRO HEADER (gerekli alanlar)
// ============================================================================

/// LRO birleştirme için gereken TCP/IP header alanları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LroKey {
    pub src_ip: Ipv4Addr,
    pub dst_ip: Ipv4Addr,
    pub src_port: u16,
    pub dst_port: u16,
    /// TCP flags (PSH|ACK tipik olarak)
    pub tcp_flags: u8,
    pub ip_id: u16,
    /// TCP options mask (tüm aktif option'lar eşit olmalı)
    pub tcp_options_mask: u16,
    /// Timestamp option TSval (varsa)
    pub ts_val: u32,
}

/// LRO birikmiş segment
#[derive(Clone, Debug)]
pub struct LroAggregate {
    pub key: LroKey,
    /// Birleştirilmiş veri (TCP payload'lar peş peşe)
    pub data: Vec<u8>,
    /// İlk segmentin IP/TCP header'ından kopyalanan meta
    pub first_seq: u32,
    pub last_seq: u32,
    pub ack: u32,
    pub window: u16,
    pub ip_id: u16,
    /// Birleştirilen segment sayısı
    pub segment_count: u32,
    /// Toplam payload byte sayısı
    pub total_payload_len: u32,
    /// İlk segment geldiği zaman (ticks)
    pub first_seen_ticks: u64,
    /// En son segment
    pub last_seen_ticks: u64,
    /// Checksum bilgisi (NIC offload için)
    pub tcp_csum: u32,
    pub ip_csum: u32,
}

impl LroAggregate {
    pub fn new(key: LroKey, seq: u32, ack: u32, window: u16, data: Vec<u8>, now: u64) -> Self {
        let total = data.len() as u32;
        LroAggregate {
            key,
            data,
            first_seq: seq,
            last_seq: seq.wrapping_add(total),
            ack,
            window,
            ip_id: key.ip_id,
            segment_count: 1,
            total_payload_len: total,
            first_seen_ticks: now,
            last_seen_ticks: now,
            tcp_csum: 0,
            ip_csum: 0,
        }
    }
}

// ============================================================================
// LRO TOPLAYICI
// ============================================================================

/// Maksimum birleştirilmiş segment boyutu (Linux LRO_PAGE_SIZE)
pub const LRO_MAX_LEN: usize = 65536;

/// Maksimum segment sayısı (tek akışta birleştirilebilecek)
pub const LRO_MAX_SEGMENTS: u32 = 32;

#[derive(Debug, Default)]
pub struct LroStats {
    pub aggregated: AtomicU32,
    pub flushed: AtomicU32,
    pub dropped: AtomicU32,
    pub total_bytes_in: AtomicU32,
    pub total_bytes_out: AtomicU32,
}

pub struct LroAggregator {
    pub flows: BTreeMap<u64, LroAggregate>, // key hash → aggregate
    pub stats: LroStats,
    /// Zaman aşımı (ticks, ~1ms)
    pub flush_timeout: u64,
}

impl LroAggregator {
    pub const fn new() -> Self {
        LroAggregator {
            flows: BTreeMap::new(),
            stats: LroStats {
                aggregated: AtomicU32::new(0),
                flushed: AtomicU32::new(0),
                dropped: AtomicU32::new(0),
                total_bytes_in: AtomicU32::new(0),
                total_bytes_out: AtomicU32::new(0),
            },
            flush_timeout: 1000, // 1 saniye
        }
    }

    /// Flow anahtarı hesapla (4-tuple + flags + ip_id)
    fn flow_key(key: &LroKey) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in &key.src_ip.0 {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for &b in &key.dst_ip.0 {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= key.src_port as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= key.dst_port as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= key.tcp_flags as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= key.ip_id as u64;
        h
    }

    /// Gelen segmenti birleştir veya flush et
    ///
    /// `payload`: TCP segmentinin sadece payload'ı (header hariç)
    ///
    /// LRO spec (Linux `tcp_lro`): SYN/FIN/RST bayrakları taşıyan
    /// segmentler ASLA birleştirilmez; flush tetiklemeden reddedilir.
    pub fn aggregate(
        &mut self,
        key: LroKey,
        seq: u32,
        ack: u32,
        window: u16,
        payload: Vec<u8>,
    ) -> Result<Option<LroAggregate>, &'static str> {
        let fk = Self::flow_key(&key);
        let now = crate::interrupts::get_ticks();
        self.stats
            .total_bytes_in
            .fetch_add(payload.len() as u32, Ordering::Relaxed);

        // TCP control bayrakları: SYN(0x02), FIN(0x01), RST(0x04) → reddet
        const CONTROL_FLAGS: u8 = 0x02 | 0x01 | 0x04;
        if key.tcp_flags & CONTROL_FLAGS != 0 {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return Err("LRO rejects SYN/FIN/RST segments");
        }

        // Mevcut akışı kontrol et
        // Merge koşulu: henüz MAX'a ulaşılmamış olmalı
        let can_merge = if let Some(existing) = self.flows.get(&fk) {
            existing.key.tcp_flags == key.tcp_flags
                && existing.key.tcp_options_mask == key.tcp_options_mask
                && existing.last_seq == seq
                && existing.data.len() + payload.len() <= LRO_MAX_LEN
                && existing.segment_count < LRO_MAX_SEGMENTS
        } else {
            false
        };

        if can_merge {
            // Birleştir
            let existing = self.flows.get_mut(&fk).unwrap();
            existing.data.extend_from_slice(&payload);
            existing.last_seq = existing.last_seq.wrapping_add(payload.len() as u32);
            existing.segment_count += 1;
            existing.total_payload_len += payload.len() as u32;
            existing.window = window;
            existing.ack = ack;
            existing.last_seen_ticks = now;
            self.stats.aggregated.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        } else {
            // Flush eski (varsa) ve yeni başlat
            let flushed = self.flows.remove(&fk);
            if let Some(ref f) = flushed {
                self.stats.flushed.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .total_bytes_out
                    .fetch_add(f.total_payload_len, Ordering::Relaxed);
            }
            // Yeni akış ekle
            let new_agg = LroAggregate::new(key, seq, ack, window, payload, now);
            self.flows.insert(fk, new_agg);
            Ok(flushed)
        }
    }

    /// Timeout'a uğramış aggregate'leri flush et
    pub fn flush_timeout_expired(&mut self) -> Vec<LroAggregate> {
        let now = crate::interrupts::get_ticks();
        let mut out = Vec::new();
        let keys: Vec<u64> = self
            .flows
            .iter()
            .filter(|(_, v)| now.saturating_sub(v.last_seen_ticks) > self.flush_timeout)
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            if let Some(agg) = self.flows.remove(&k) {
                self.stats.flushed.fetch_add(1, Ordering::Relaxed);
                out.push(agg);
            }
        }
        out
    }

    /// Tüm akışları flush et
    pub fn flush_all(&mut self) -> Vec<LroAggregate> {
        let mut out = Vec::new();
        let keys: Vec<u64> = self.flows.keys().copied().collect();
        for k in keys {
            if let Some(agg) = self.flows.remove(&k) {
                self.stats.flushed.fetch_add(1, Ordering::Relaxed);
                out.push(agg);
            }
        }
        out
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static LRO_AGGREGATOR: Mutex<LroAggregator> = Mutex::new(LroAggregator::new());

// ============================================================================
// PUBLIC API
// ============================================================================

pub fn aggregate(
    key: LroKey,
    seq: u32,
    ack: u32,
    window: u16,
    payload: Vec<u8>,
) -> Result<Option<LroAggregate>, &'static str> {
    LRO_AGGREGATOR
        .lock()
        .aggregate(key, seq, ack, window, payload)
}

pub fn flush_all() -> Vec<LroAggregate> {
    LRO_AGGREGATOR.lock().flush_all()
}

pub fn flush_timeout_expired() -> Vec<LroAggregate> {
    LRO_AGGREGATOR.lock().flush_timeout_expired()
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key() -> LroKey {
        LroKey {
            src_ip: Ipv4Addr::new(10, 0, 0, 1),
            dst_ip: Ipv4Addr::new(10, 0, 0, 2),
            src_port: 12345,
            dst_port: 443,
            tcp_flags: 0x18, // PSH|ACK
            ip_id: 1,
            tcp_options_mask: 0,
            ts_val: 0,
        }
    }

    #[test]
    fn single_segment_returns_none() {
        let mut agg = LroAggregator::new();
        let key = make_key();
        let r = agg.aggregate(key, 1000, 2000, 65535, vec![1, 2, 3, 4]).unwrap();
        assert!(r.is_none());
        assert_eq!(agg.flows.len(), 1);
    }

    #[test]
    fn contiguous_segments_are_aggregated() {
        let mut agg = LroAggregator::new();
        let key = make_key();
        agg.aggregate(key, 1000, 2000, 65535, vec![1, 2, 3, 4]).unwrap();
        let r = agg.aggregate(key, 1004, 2000, 65535, vec![5, 6, 7, 8]).unwrap();
        assert!(r.is_none());
        let entry = agg.flows.values().next().unwrap();
        assert_eq!(entry.segment_count, 2);
        assert_eq!(entry.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(entry.last_seq, 1008);
    }

    #[test]
    fn out_of_order_segment_flushes() {
        let mut agg = LroAggregator::new();
        let key = make_key();
        agg.aggregate(key, 1000, 2000, 65535, vec![1, 2, 3, 4]).unwrap();
        // Seq 2000 değil 1004 olmalı; burada 2000 = 1004 + 992 gap
        let r = agg.aggregate(key, 2000, 2000, 65535, vec![5, 6, 7, 8]).unwrap();
        assert!(r.is_some(), "gap should flush");
        // Sonra yeni bir akış başlamalı
        assert_eq!(agg.flows.len(), 1);
    }

    #[test]
    fn max_segments_triggers_flush() {
        let mut agg = LroAggregator::new();
        let key = make_key();
        // LRO_MAX_SEGMENTS + 1 segment gönder, sonuncusu flush tetiklemeli
        for i in 0..=(LRO_MAX_SEGMENTS) {
            let seq = 1000 + i * 4;
            let r = agg.aggregate(key, seq, 2000, 65535, vec![0; 4]).unwrap();
            if i == LRO_MAX_SEGMENTS {
                // 33. segment geldiğinde 32 segmentlik aggregate flush olur
                assert!(r.is_some());
            }
        }
    }

    #[test]
    fn syn_segment_is_rejected_and_not_aggregated() {
        let mut agg = LroAggregator::new();
        let mut key = make_key();
        key.tcp_flags = 0x02; // SYN
        let r = agg.aggregate(key, 1000, 2000, 65535, vec![0; 4]);
        assert!(r.is_err(), "SYN must be rejected, got {:?}", r);
        assert_eq!(agg.flows.len(), 0, "SYN must not enter flow table");
    }

    #[test]
    fn fin_segment_is_rejected() {
        let mut agg = LroAggregator::new();
        let mut key = make_key();
        key.tcp_flags = 0x01; // FIN
        assert!(agg.aggregate(key, 1000, 2000, 65535, vec![0; 4]).is_err());
        assert_eq!(agg.flows.len(), 0);
    }

    #[test]
    fn rst_segment_is_rejected() {
        let mut agg = LroAggregator::new();
        let mut key = make_key();
        key.tcp_flags = 0x04; // RST
        assert!(agg.aggregate(key, 1000, 2000, 65535, vec![0; 4]).is_err());
        assert_eq!(agg.flows.len(), 0);
    }

    #[test]
    fn syn_fin_rst_drops_increment_stats() {
        let mut agg = LroAggregator::new();
        let mut key = make_key();
        for flag in [0x02u8, 0x01, 0x04] {
            key.tcp_flags = flag;
            let _ = agg.aggregate(key, 1000, 2000, 65535, vec![0; 4]);
        }
        assert_eq!(agg.stats.dropped.load(Ordering::Relaxed), 3);
    }
}

