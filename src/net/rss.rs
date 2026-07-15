//! # RSS (Receive Side Scaling) — Çok Çekirdekli NIC Alım Dağıtımı
//!
//! RSS, modern NIC'lerin (Intel ixgbe, mlx5 vb.) gelen paketleri birden
//! fazla RX kuyruğuna dağıtmak için kullandığı donanım özelliğidir.
//! Her kuyruk farklı bir CPU çekirdeğine bağlanır; bu sayede ağ
//! throughput'u lineer olarak ölçeklenir.
//!
//! ## RSS Nasıl Çalışır?
//!
//! ```text
//!         ┌──────────┐
//!         │   NIC    │
//!         │  RSS HW  │
//!         └────┬─────┘
//!              │  Toeplitz hash (src_ip, dst_ip, src_port, dst_port, proto)
//!              ▼
//!   ┌────┬────┬────┬────┐
//!   │RX0 │RX1 │RX2 │RX3 │  (her biri farklı CPU'ya IRQ verir)
//!   └────┴────┴────┴────┘
//! ```
//!
//! ## Toeplitz Hash (Microsoft RSS Spec)
//!
//! NIC'ler genellikle Toeplitz hash kullanır:
//!
//! ```text
//! hash = Σ byte[i] × key[byte_index]
//! ```
//!
//! `key` 40 byte (320 bit) uzunluğunda, NIC'e programlanır. Aynı key
//! ile aynı flow aynı kuyruğa düşer.
//!
//! ## Indirection Table (RETA)
//!
//! 128 veya 512 girişli tablo: hash[0..7] % 128 → RETA[hash] → RX kuyruğu.
//!
//! ## echOS Tasarımı
//!
//! `RssConfig`: key (40 byte), indirection table, hash tipleri.
//! `compute_hash(flow)` → Toeplitz hash, ardından `select_queue` ile RX kuyruğu seç.

use super::Ipv4Addr;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// RSS SABİTLERİ
// ============================================================================

/// RSS key uzunluğu (byte)
pub const RSS_KEY_LEN: usize = 40;

/// Varsayılan indirection table giriş sayısı
pub const RSS_RETA_SIZE: usize = 128;

/// Maksimum RX kuyruğu sayısı
pub const RSS_MAX_QUEUES: usize = 16;

/// Hash tipleri — hangi header alanları hash'e girsin
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RssHashType(u8);

impl RssHashType {
    pub const IPV4: u8 = 0x01;
    pub const IPV4_TCP: u8 = 0x02;
    pub const IPV4_UDP: u8 = 0x04;
    pub const IPV6: u8 = 0x08;
    pub const IPV6_TCP: u8 = 0x10;
    pub const IPV6_UDP: u8 = 0x20;

    pub fn new(types: u8) -> Self {
        RssHashType(types)
    }

    pub fn contains(self, t: u8) -> bool {
        (self.0 & t) != 0
    }
}

// ============================================================================
// FLOW TANIMI
// ============================================================================

/// Bir paketin 5-tuple'ı (RSS hash girdisi)
#[derive(Clone, Copy, Debug)]
pub struct RssFlow {
    pub src_ip: Ipv4Addr,
    pub dst_ip: Ipv4Addr,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8, // 6 = TCP, 17 = UDP
}

// ============================================================================
// RSS YAPILANDIRMASI
// ============================================================================

#[derive(Debug)]
pub struct RssConfig {
    pub key: [u8; RSS_KEY_LEN],
    pub reta: Vec<u8>,           // RETA[i] = RX kuyruğu ID (0..num_queues-1)
    pub num_queues: u32,
    pub hash_types: RssHashType,
    pub enable: bool,
    pub indirection_table_size: usize,
    /// Her kuyruğa atanan flow sayısı (istatistik)
    pub queue_flow_count: Vec<AtomicU32>,
}

impl RssConfig {
    /// Yeni RSS config — varsayılan key
    pub fn new(num_queues: u32) -> Self {
        let num_queues = num_queues.min(RSS_MAX_QUEUES as u32) as usize;
        let mut reta = Vec::with_capacity(RSS_RETA_SIZE);
        for i in 0..RSS_RETA_SIZE {
            reta.push((i % num_queues) as u8);
        }
        let mut key = [0u8; RSS_KEY_LEN];
        // Deterministik anahtar: golden ratio karıştırma ile uniform dağılım
        for (i, b) in key.iter_mut().enumerate() {
            *b = ((i as u32).wrapping_mul(0x9e3779b1) >> 24) as u8;
        }
        let queue_flow_count = (0..num_queues).map(|_| AtomicU32::new(0)).collect();
        RssConfig {
            key,
            reta,
            num_queues: num_queues as u32,
            hash_types: RssHashType::new(
                RssHashType::IPV4 | RssHashType::IPV4_TCP | RssHashType::IPV4_UDP,
            ),
            enable: true,
            indirection_table_size: RSS_RETA_SIZE,
            queue_flow_count,
        }
    }

    /// Key'i kullanıcı tarafından sağlanan 40 byte ile değiştir
    pub fn set_key(&mut self, key: [u8; RSS_KEY_LEN]) {
        self.key = key;
    }

    /// RETA girişini güncelle
    pub fn set_reta(&mut self, index: usize, queue: u8) -> Result<(), &'static str> {
        if index >= self.reta.len() {
            return Err("RETA index out of range");
        }
        if (queue as u32) >= self.num_queues {
            return Err("queue ID out of range");
        }
        self.reta[index] = queue;
        Ok(())
    }

    /// Toeplitz hash hesapla — Microsoft RSS Spec.
    ///
    /// Spec: K 40-byte anahtar, sonuç 32-bit. Her input bit için
    /// (MSB to LSB): eğer bit = 1, result ^= u32::from_le_bytes(K[0..4])
    /// yani K'nin ilk 4 byte'ı LE u32 olarak; ardından K <<= 1 (40 byte
    /// üzerinde MSB-first 1 bit shift).
    pub fn toeplitz_hash(&self, data: &[u8]) -> u32 {
        let mut k = self.key;
        let mut result: u32 = 0;
        for &byte in data {
            for bit in 0..8u32 {
                if (byte >> (7 - bit)) & 1 == 1 {
                    let k_val = u32::from_le_bytes([k[0], k[1], k[2], k[3]]);
                    result ^= k_val;
                }
                // K <<= 1 (40 byte üzerinde MSB-first, byte 0 MSB)
                let mut carry: u8 = 0;
                for i in 0..RSS_KEY_LEN {
                    let new_carry = k[i] >> 7;
                    k[i] = (k[i] << 1) | carry;
                    carry = new_carry;
                }
            }
        }
        result
    }

    /// RETA indeksini hesapla (hash'in alt 7 veya 9 bit'i)
    pub fn reta_index(&self, hash: u32) -> usize {
        let bits = (self.indirection_table_size as u32).trailing_zeros();
        let mask = (1u32 << bits) - 1;
        (hash & mask) as usize
    }

    /// Flow → RX kuyruğu seç
    pub fn select_queue(&self, flow: &RssFlow) -> u32 {
        if !self.enable || self.num_queues == 0 {
            return 0;
        }
        // 5-tuple'ı byte dizisine serialize
        let mut data = Vec::with_capacity(13);
        data.extend_from_slice(&flow.src_ip.0);
        data.extend_from_slice(&flow.dst_ip.0);
        data.extend_from_slice(&flow.src_port.to_be_bytes());
        data.extend_from_slice(&flow.dst_port.to_be_bytes());
        data.push(flow.proto);

        let hash = self.toeplitz_hash(&data);
        let idx = self.reta_index(hash);
        self.reta[idx] as u32
    }

    /// Kuyruk kullanım istatistiğini güncelle
    pub fn record_queue_hit(&self, queue_id: u32) {
        if (queue_id as usize) < self.queue_flow_count.len() {
            self.queue_flow_count[queue_id as usize].fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reta_default_cycles_queues() {
        let c = RssConfig::new(4);
        assert_eq!(c.reta[0], 0);
        assert_eq!(c.reta[1], 1);
        assert_eq!(c.reta[2], 2);
        assert_eq!(c.reta[3], 3);
        assert_eq!(c.reta[4], 0); // wrap
    }

    #[test]
    fn reta_set_within_bounds() {
        let mut c = RssConfig::new(4);
        c.set_reta(5, 2).unwrap();
        assert_eq!(c.reta[5], 2);
    }

    #[test]
    fn reta_set_out_of_queue_fails() {
        let mut c = RssConfig::new(4);
        assert!(c.set_reta(0, 10).is_err());
    }

    #[test]
    fn select_queue_returns_valid_index() {
        let c = RssConfig::new(4);
        let flow = RssFlow {
            src_ip: Ipv4Addr::new(192, 168, 1, 5),
            dst_ip: Ipv4Addr::new(8, 8, 8, 8),
            src_port: 12345,
            dst_port: 443,
            proto: 6,
        };
        let q = c.select_queue(&flow);
        assert!(q < 4);
    }

    #[test]
    fn same_flow_maps_to_same_queue() {
        let c = RssConfig::new(8);
        let flow = RssFlow {
            src_ip: Ipv4Addr::new(192, 168, 1, 5),
            dst_ip: Ipv4Addr::new(8, 8, 8, 8),
            src_port: 12345,
            dst_port: 443,
            proto: 6,
        };
        let q1 = c.select_queue(&flow);
        let q2 = c.select_queue(&flow);
        assert_eq!(q1, q2, "same flow must go to same queue");
    }

    #[test]
    fn hash_type_contains() {
        let t = RssHashType::new(RssHashType::IPV4 | RssHashType::IPV4_TCP);
        assert!(t.contains(RssHashType::IPV4));
        assert!(t.contains(RssHashType::IPV4_TCP));
        assert!(!t.contains(RssHashType::IPV6));
    }

    #[test]
    fn toeplitz_all_zero_key_all_zero_input_yields_zero() {
        let mut c = RssConfig::new(1);
        c.key = [0u8; RSS_KEY_LEN];
        let h = c.toeplitz_hash(&[0u8; 16]);
        assert_eq!(h, 0);
    }

    #[test]
    fn toeplitz_deterministic_for_same_input() {
        let c = RssConfig::new(4);
        let data = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x06];
        let h1 = c.toeplitz_hash(&data);
        let h2 = c.toeplitz_hash(&data);
        assert_eq!(h1, h2, "Toeplitz must be deterministic");
    }

    #[test]
    fn toeplitz_first_bit_set_xors_first_k_window() {
        // K = [0x01, 0, 0, 0, 0, ...]. İlk input bit set olduğunda
        // result = K[0..4] LE = u32::from_le_bytes([0x01, 0, 0, 0]) = 0x01.
        let mut c = RssConfig::new(1);
        c.key = [0u8; RSS_KEY_LEN];
        c.key[0] = 0x01;
        let h = c.toeplitz_hash(&[0x80, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(h, 0x01, "input bit 0 set with K[0]=0x01 must give 0x01");
    }

    #[test]
    fn toeplitz_different_keys_produce_different_hashes() {
        let mut c1 = RssConfig::new(4);
        c1.key[0] = 0xAA;
        let mut c2 = RssConfig::new(4);
        c2.key[0] = 0x55;
        let data = [0xde, 0xad, 0xbe, 0xef];
        let h1 = c1.toeplitz_hash(&data);
        let h2 = c2.toeplitz_hash(&data);
        assert_ne!(h1, h2, "different keys must produce different hashes");
    }

    #[test]
    fn toeplitz_shift_advances_k_after_each_bit() {
        // K = [0x01, 0, 0, 0, 0, ...]. Input bits 0 ve 8 set (0x80 0x80).
        // Bit 0 set: K[0..4] = [0x01, 0, 0, 0] → LE u32 = 0x01. result = 0x01.
        //   K shifted by 8 bits → K = [0, 0x01, 0, 0, 0, ...] (8 bits processed).
        // Bit 8 set: K[0..4] = [0, 0x01, 0, 0] → LE u32 = 0x100. result = 0x01 ^ 0x100 = 0x101.
        let mut c = RssConfig::new(1);
        c.key = [0u8; RSS_KEY_LEN];
        c.key[0] = 0x01;
        let h = c.toeplitz_hash(&[0x80, 0x80, 0, 0, 0, 0, 0, 0]);
        assert_eq!(h, 0x101, "K must shift left by 1 per input bit");
    }
}
