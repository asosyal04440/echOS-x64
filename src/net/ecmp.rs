//! # ECMP (Equal-Cost Multi-Path) Routing
//!
//! ECMP, aynı hedefe birden fazla eşit maliyetli yol olduğunda yükü
//! bu yollar arasında dağıtır. RFC 2992 (ECMP for IPv4/v6) ile tanımlıdır.
//!
//! ## Neden ECMP?
//!
//! - **Throughput artışı**: 2 link × 1 Gbps = 2 Gbps efektif
//! - **Yedeklilik**: Bir link down olursa diğeri trafik alır
//! - **Maliyet**: Daha fazla link = daha az over-subscription
//!
//! ## Hash Algoritmaları
//!
//! - **Per-flow**: 5-tuple hash (src_ip, dst_ip, src_port, dst_port, proto)
//!   → aynı flow hep aynı yola → paket reorder yok
//! - **Per-packet**: round-robin → max throughput ama reorder riski
//!
//! Linux varsayılanı: per-flow (Thibault hash, RFC 2991).
//!
//! ## Routing Kararı
//!
//! ```text
//! packet → hash(src,dst,sport,dport,proto) % nexthop_count → nexthop[idx]
//! ```
//!
//! ## echOS Tasarımı
//!
//! `EcmpGroup` ad + hedef network + nexthop listesi.
//! `EcmpGroup::select(packet)` ile nexthop seçilir.

use super::{Ipv4Addr, Mutex};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// NEXTHOP
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nexthop {
    pub gw: Ipv4Addr,
    pub interface_idx: u32,
    pub weight: u32, // Ağırlık (toplam ağırlık içindeki payı)
    pub up: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcmpHashMode {
    /// 5-tuple hash (src_ip, dst_ip, src_port, dst_port, proto)
    PerFlow = 0,
    /// Paket bazlı round-robin
    PerPacket = 1,
    /// Sadece src_ip + dst_ip
    PerSrcDst = 2,
}

// ============================================================================
// ECMP GROUP
// ============================================================================

#[derive(Clone, Debug)]
pub struct EcmpGroup {
    pub id: u32,
    pub name: String,
    pub destination: Ipv4Addr,
    pub destination_mask: Ipv4Addr,
    pub nexthops: Vec<Nexthop>,
    pub hash_mode: EcmpHashMode,
    /// Per-packet round-robin sayaç
    pub rr_counter: u32,
    /// İstatistik
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
}

impl EcmpGroup {
    pub fn new(id: u32, name: String, dst: Ipv4Addr, mask: Ipv4Addr) -> Self {
        EcmpGroup {
            id,
            name,
            destination: dst,
            destination_mask: mask,
            nexthops: Vec::new(),
            hash_mode: EcmpHashMode::PerFlow,
            rr_counter: 0,
            lookups: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Nexthop ekle
    pub fn add_nexthop(&mut self, nh: Nexthop) {
        if !self.nexthops.iter().any(|n| n.gw == nh.gw) {
            self.nexthops.push(nh);
        }
    }

    /// FNV-1a 32-bit tek byte adımı
    #[inline]
    fn fnv1a_step(h: u32, b: u8) -> u32 {
        (h ^ b as u32).wrapping_mul(0x01000193)
    }

    /// 5-tuple hash (FNV-1a 32-bit, byte-by-byte) — per-flow mod için
    ///
    /// Standart FNV-1a: `h = (h ^ byte) * FNV_PRIME`. Tüm alanlar (IP'ler,
    /// port'lar, proto) byte byte hashing'e sokulur; böylece farklı
    /// implementasyonlarla aynı sonuç üretilir.
    fn hash_5tuple(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, sport: u16, dport: u16, proto: u8) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for &b in &src_ip.0 {
            h = Self::fnv1a_step(h, b);
        }
        for &b in &dst_ip.0 {
            h = Self::fnv1a_step(h, b);
        }
        for &b in &sport.to_be_bytes() {
            h = Self::fnv1a_step(h, b);
        }
        for &b in &dport.to_be_bytes() {
            h = Self::fnv1a_step(h, b);
        }
        h = Self::fnv1a_step(h, proto);
        h
    }

    /// 2-tuple (src, dst) hash — `PerSrcDst` modu için
    fn hash_srcdst(src_ip: Ipv4Addr, dst_ip: Ipv4Addr) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for &b in &src_ip.0 {
            h = Self::fnv1a_step(h, b);
        }
        for &b in &dst_ip.0 {
            h = Self::fnv1a_step(h, b);
        }
        h
    }

    /// Paket için nexthop seç
    pub fn select(
        &mut self,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
        sport: u16,
        dport: u16,
        proto: u8,
    ) -> Option<Nexthop> {
        self.lookups += 1;
        let active: Vec<&Nexthop> = self.nexthops.iter().filter(|n| n.up).collect();
        if active.is_empty() {
            self.misses += 1;
            return None;
        }

        let idx = match self.hash_mode {
            EcmpHashMode::PerFlow => {
                let h = Self::hash_5tuple(src_ip, dst_ip, sport, dport, proto);
                (h as usize) % active.len()
            }
            EcmpHashMode::PerSrcDst => {
                let h = Self::hash_srcdst(src_ip, dst_ip);
                (h as usize) % active.len()
            }
            EcmpHashMode::PerPacket => {
                let i = (self.rr_counter as usize) % active.len();
                self.rr_counter = self.rr_counter.wrapping_add(1);
                i
            }
        };
        self.hits += 1;
        Some(*active[idx])
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static ECMP_GROUPS: Mutex<BTreeMap<u32, EcmpGroup>> = Mutex::new(BTreeMap::new());
static ECMP_BY_NAME: Mutex<BTreeMap<String, u32>> = Mutex::new(BTreeMap::new());

static ECMP_STATS: EcmpStats = EcmpStats::new();
struct EcmpStats {
    groups: AtomicU32,
    nexthops: AtomicU32,
    selections: AtomicU32,
}
impl EcmpStats {
    const fn new() -> Self {
        EcmpStats {
            groups: AtomicU32::new(0),
            nexthops: AtomicU32::new(0),
            selections: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni ECMP grubu oluştur
pub fn create_group(id: u32, name: &str, dst: Ipv4Addr, mask: Ipv4Addr) -> Result<(), EcmpError> {
    let mut groups = ECMP_GROUPS.lock();
    if groups.contains_key(&id) {
        return Err(EcmpError::AlreadyExists);
    }
    groups.insert(id, EcmpGroup::new(id, String::from(name), dst, mask));
    ECMP_BY_NAME.lock().insert(String::from(name), id);
    ECMP_STATS.groups.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Nexthop ekle
pub fn add_nexthop(group_id: u32, gw: Ipv4Addr, ifidx: u32, weight: u32) -> Result<(), EcmpError> {
    let mut groups = ECMP_GROUPS.lock();
    let g = groups.get_mut(&group_id).ok_or(EcmpError::NotFound)?;
    g.add_nexthop(Nexthop {
        gw,
        interface_idx: ifidx,
        weight,
        up: true,
    });
    ECMP_STATS.nexthops.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Paket için nexthop seç
pub fn select(
    group_id: u32,
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    sport: u16,
    dport: u16,
    proto: u8,
) -> Result<Nexthop, EcmpError> {
    let mut groups = ECMP_GROUPS.lock();
    let g = groups.get_mut(&group_id).ok_or(EcmpError::NotFound)?;
    let result = g.select(src_ip, dst_ip, sport, dport, proto);
    ECMP_STATS.selections.fetch_add(1, Ordering::Relaxed);
    result.ok_or(EcmpError::NoActiveNexthop)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcmpError {
    NotFound,
    AlreadyExists,
    NoActiveNexthop,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_flow_same_flow_same_nexthop() {
        let mut g = EcmpGroup::new(
            1,
            "default".into(),
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(0, 0, 0, 0),
        );
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 1),
            interface_idx: 0,
            weight: 1,
            up: true,
        });
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 2),
            interface_idx: 1,
            weight: 1,
            up: true,
        });
        let s = Ipv4Addr::new(192, 168, 1, 5);
        let d = Ipv4Addr::new(8, 8, 8, 8);
        let n1 = g.select(s, d, 12345, 443, 6).unwrap();
        let n2 = g.select(s, d, 12345, 443, 6).unwrap();
        assert_eq!(n1, n2, "same flow must map to same nexthop");
    }

    #[test]
    fn per_packet_round_robin() {
        let mut g = EcmpGroup::new(
            1,
            "default".into(),
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(0, 0, 0, 0),
        );
        g.hash_mode = EcmpHashMode::PerPacket;
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 1),
            interface_idx: 0,
            weight: 1,
            up: true,
        });
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 2),
            interface_idx: 1,
            weight: 1,
            up: true,
        });
        let s = Ipv4Addr::new(1, 1, 1, 1);
        let d = Ipv4Addr::new(2, 2, 2, 2);
        let n0 = g.select(s, d, 0, 0, 6).unwrap();
        let n1 = g.select(s, d, 0, 0, 6).unwrap();
        let n2 = g.select(s, d, 0, 0, 6).unwrap();
        // Round robin: 0, 1, 0
        assert_eq!(n0.gw, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(n1.gw, Ipv4Addr::new(10, 0, 0, 2));
        assert_eq!(n2.gw, Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn down_nexthop_skipped() {
        let mut g = EcmpGroup::new(
            1,
            "default".into(),
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(0, 0, 0, 0),
        );
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 1),
            interface_idx: 0,
            weight: 1,
            up: false, // down
        });
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 2),
            interface_idx: 1,
            weight: 1,
            up: true,
        });
        let s = Ipv4Addr::new(1, 1, 1, 1);
        let d = Ipv4Addr::new(2, 2, 2, 2);
        let n = g.select(s, d, 0, 0, 6).unwrap();
        assert_eq!(n.gw, Ipv4Addr::new(10, 0, 0, 2));
    }

    #[test]
    fn all_nexthops_down_returns_none() {
        let mut g = EcmpGroup::new(
            1,
            "default".into(),
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(0, 0, 0, 0),
        );
        g.add_nexthop(Nexthop {
            gw: Ipv4Addr::new(10, 0, 0, 1),
            interface_idx: 0,
            weight: 1,
            up: false,
        });
        let r = g.select(Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(2, 2, 2, 2), 0, 0, 6);
        assert!(r.is_none());
    }

    #[test]
    fn fnv1a_byte_by_byte_matches_reference() {
        // Reference: integration test'teki local implementasyon
        let mut h_ref: u32 = 0x811C9DC5;
        for &b in [10u8, 0, 0, 1].iter().chain([10u8, 0, 0, 2].iter()) {
            h_ref ^= b as u32;
            h_ref = h_ref.wrapping_mul(0x01000193);
        }
        for w in &[1234u16, 80u16] {
            for &b in w.to_be_bytes().iter() {
                h_ref ^= b as u32;
                h_ref = h_ref.wrapping_mul(0x01000193);
            }
        }
        h_ref ^= 6u32;
        h_ref = h_ref.wrapping_mul(0x01000193);

        let h_prod = EcmpGroup::hash_5tuple(
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 2),
            1234,
            80,
            6,
        );
        assert_eq!(h_ref, h_prod);
    }

    #[test]
    fn fnv1a_different_dport_different_hash() {
        let h1 = EcmpGroup::hash_5tuple(
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 2),
            1234,
            80,
            6,
        );
        let h2 = EcmpGroup::hash_5tuple(
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 2),
            1234,
            81,
            6,
        );
        assert_ne!(h1, h2);
    }
}
