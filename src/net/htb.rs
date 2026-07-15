//! # HTB (Hierarchical Token Bucket) — Linux tc qdisc
//!
//! HTB, hiyerarşik token bucket ile sınıflandırma ve shaping yapar.
//! Linux'ta `tc qdisc add dev eth0 root handle 1: htb` ile yapılandırılır.
//!
//! ## HTB Kavramsal Model
//!
//! HTB'de her sınıf (class) bir token bucket'tır ve hiyerarşik olarak
//! düzenlenir. Kök (root) sınıf, alt sınıfları kontrol eder. Her sınıfın
//! rate (garanti), ceil (maksimum) ve priority özellikleri vardır.
//!
//! ```text
//!                root (1:1)
//!                /          \
//!        class 1:10        class 1:20
//!        (rate=1Mbps,      (rate=2Mbps,
//!         ceil=2Mbps)       ceil=2Mbps)
//!         |                |
//!       filter            filter
//!        |                |
//!     packets          packets
//! ```
//!
//! ## Rate, Ceil, Burst
//!
//! - **rate**: Garanti edilen bant genişliği (CIR, Committed Information Rate)
//! - **ceil**: Maksimum bant genişliği (MIR, Maximum Information Rate)
//! - **burst**: Bucket büyüklüğü (piksel burst için izin verilen byte)
//! - **cburst**: ceil bucket büyüklüğü
//! - **priority**: Düşük değer = yüksek öncelik
//!
//! ## Sınıflandırma (Classification)
//!
//! HTB kendi başına sınıflandırma yapmaz. Paketlerin hangi sınıfa
//! gönderileceğine `tc filter` (u32, fw, mark vs.) karar verir.
//! echOS'ta `HtbFilter` ile (src/dst IP) filtreleme yapılır.
//!
//! ## Borrow Mekanizması
//!
//! Bir sınıf rate'inden az kullanıyorsa, fazlası kardeşlerine
//! "ödünç verilebilir" (ceil sınırına kadar). Bu sayede toplam
//! bant genişliği dinamik olarak paylaşılır.

use super::{Ipv4Addr, Mutex};
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// HTB SINIF
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HtbClassId {
    pub major: u16, // 1
    pub minor: u16, // 10
}

impl HtbClassId {
    pub const fn new(major: u16, minor: u16) -> Self {
        HtbClassId { major, minor }
    }
    pub fn to_u32(self) -> u32 {
        ((self.major as u32) << 16) | (self.minor as u32)
    }
}

#[derive(Clone, Debug)]
pub struct HtbClass {
    pub id: HtbClassId,
    pub parent: Option<HtbClassId>,
    pub children: Vec<HtbClassId>,
    pub rate_bps: u64,    // Garanti (CIR)
    pub ceil_bps: u64,    // Maksimum (MIR)
    pub burst_bytes: u32, // Burst tolerance
    pub cburst_bytes: u32,
    pub priority: u8, // 0-7 (0 = en yüksek)
    /// Token bucket — şu anki token sayısı (byte)
    pub tokens: i64,
    pub ceil_tokens: i64,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub packets_dropped: u64,
    pub bytes_dropped: u64,
    pub last_refill_ticks: u64,
}

impl HtbClass {
    pub fn new(
        id: HtbClassId,
        parent: Option<HtbClassId>,
        rate_bps: u64,
        ceil_bps: u64,
        burst_bytes: u32,
        cburst_bytes: u32,
        priority: u8,
    ) -> Self {
        HtbClass {
            id,
            parent,
            children: Vec::new(),
            rate_bps,
            ceil_bps,
            burst_bytes,
            cburst_bytes,
            priority,
            tokens: burst_bytes as i64,
            ceil_tokens: cburst_bytes as i64,
            packets_sent: 0,
            bytes_sent: 0,
            packets_dropped: 0,
            bytes_dropped: 0,
            last_refill_ticks: 0,
        }
    }

    /// Token bucket refill — şu anki zamana kadar token ekle
    pub fn refill(&mut self, now_ticks: u64) {
        if self.last_refill_ticks == 0 {
            self.last_refill_ticks = now_ticks;
            return;
        }
        let elapsed_ms = now_ticks.saturating_sub(self.last_refill_ticks);
        if elapsed_ms == 0 {
            return;
        }
        // tokens_per_ms = rate_bps / 8 / 1000
        let rate_bytes_per_ms = self.rate_bps / 8 / 1000;
        let ceil_bytes_per_ms = self.ceil_bps / 8 / 1000;
        let new_tokens = (elapsed_ms as u64).saturating_mul(rate_bytes_per_ms) as i64;
        let new_ceil = (elapsed_ms as u64).saturating_mul(ceil_bytes_per_ms) as i64;
        self.tokens = (self.tokens + new_tokens).min(self.burst_bytes as i64);
        self.ceil_tokens = (self.ceil_tokens + new_ceil).min(self.cburst_bytes as i64);
        self.last_refill_ticks = now_ticks;
    }

    /// Paket gönderimi simüle et
    ///
    /// HTB semantiği: paket hem `tokens` (rate bucket) hem `ceil_tokens`
    /// (ceil cap) için yeterli bütçeye sahip olmalı. Biri yetersizse drop.
    /// `tokens` rate kadar, `ceil_tokens` ceil kadar replenishment alır;
    /// ikisi de time-based refill ile dolar.
    ///
    /// `size`: paket boyutu (byte)
    /// Dönüş: `true` = gönderildi, `false` = drop
    pub fn try_send(&mut self, size: usize) -> bool {
        self.refill(crate::interrupts::get_ticks());
        let s = size as i64;
        if self.tokens >= s && self.ceil_tokens >= s {
            self.tokens -= s;
            self.ceil_tokens -= s;
            self.packets_sent += 1;
            self.bytes_sent += size as u64;
            true
        } else {
            self.packets_dropped += 1;
            self.bytes_dropped += size as u64;
            false
        }
    }
}

// ============================================================================
// HTB FİLTRE
// ============================================================================

#[derive(Clone, Debug)]
pub enum HtbFilter {
    /// Kaynak IP'ye göre
    SrcIp(Ipv4Addr),
    /// Hedef IP'ye göre
    DstIp(Ipv4Addr),
    /// Hedef porta göre
    DstPort(u16),
    /// Tüm paketler (default route)
    All,
}

#[derive(Clone, Debug)]
pub struct HtbClassifyRule {
    pub filter: HtbFilter,
    pub class_id: HtbClassId,
}

// ============================================================================
// HTB QDISC
// ============================================================================

#[derive(Clone, Debug)]
pub struct HtbQdisc {
    pub handle: HtbClassId, // Root class
    pub default_class: HtbClassId,
    pub classes: BTreeMap<u32, HtbClass>,
    pub rules: Vec<HtbClassifyRule>,
    pub mtu: u32,
    pub packets_classified: u64,
    pub packets_dropped: u64,
    pub bytes_classified: u64,
    pub bytes_dropped: u64,
}

impl HtbQdisc {
    pub fn new(handle: HtbClassId, default_class: HtbClassId, mtu: u32) -> Self {
        let mut classes = BTreeMap::new();
        let root = HtbClass::new(handle, None, 0, 0, 0, 0, 7);
        classes.insert(handle.to_u32(), root);
        HtbQdisc {
            handle,
            default_class,
            classes,
            rules: Vec::new(),
            mtu,
            packets_classified: 0,
            packets_dropped: 0,
            bytes_classified: 0,
            bytes_dropped: 0,
        }
    }

    /// Root sınıfı ekle (veya ilk kurulum)
    pub fn add_class(&mut self, cls: HtbClass) -> Result<(), &'static str> {
        let key = cls.id.to_u32();
        if self.classes.contains_key(&key) {
            return Err("class exists");
        }
        // Parent'a çocuk olarak ekle
        if let Some(p) = cls.parent {
            if let Some(parent) = self.classes.get_mut(&p.to_u32()) {
                parent.children.push(cls.id);
            }
        }
        self.classes.insert(key, cls);
        Ok(())
    }

    pub fn add_filter(&mut self, filter: HtbFilter, class_id: HtbClassId) {
        self.rules.push(HtbClassifyRule { filter, class_id });
    }

    /// Paket için sınıf seç (classification)
    pub fn classify(
        &self,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
        dst_port: u16,
    ) -> HtbClassId {
        for rule in &self.rules {
            match rule.filter {
                HtbFilter::SrcIp(ip) if ip == src_ip => return rule.class_id,
                HtbFilter::DstIp(ip) if ip == dst_ip => return rule.class_id,
                HtbFilter::DstPort(p) if p == dst_port => return rule.class_id,
                HtbFilter::All => return rule.class_id,
                _ => continue,
            }
        }
        self.default_class
    }

    /// Bir paketi qdisc'ten geçir
    pub fn enqueue(
        &mut self,
        size: usize,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
        dst_port: u16,
    ) -> Result<(), &'static str> {
        let class_id = self.classify(src_ip, dst_ip, dst_port);
        let key = class_id.to_u32();
        let cls = self.classes.get_mut(&key).ok_or("class not found")?;
        if cls.try_send(size) {
            self.packets_classified += 1;
            self.bytes_classified += size as u64;
            Ok(())
        } else {
            self.packets_dropped += 1;
            self.bytes_dropped += size as u64;
            Err("dropped by HTB")
        }
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static HTB_QDISCS: Mutex<BTreeMap<String, HtbQdisc>> = Mutex::new(BTreeMap::new());

static HTB_STATS: HtbStats = HtbStats::new();
struct HtbStats {
    qdiscs: AtomicU32,
    classes: AtomicU32,
    packets: AtomicU32,
    drops: AtomicU32,
}
impl HtbStats {
    const fn new() -> Self {
        HtbStats {
            qdiscs: AtomicU32::new(0),
            classes: AtomicU32::new(0),
            packets: AtomicU32::new(0),
            drops: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni HTB qdisc oluştur
pub fn create_qdisc(name: &str, default_class: HtbClassId) -> Result<(), &'static str> {
    let mut qdiscs = HTB_QDISCS.lock();
    if qdiscs.contains_key(name) {
        return Err("qdisc exists");
    }
    let q = HtbQdisc::new(HtbClassId::new(1, 1), default_class, 1500);
    qdiscs.insert(String::from(name), q);
    HTB_STATS.qdiscs.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// HTB sınıfı ekle
pub fn add_class(
    qdisc_name: &str,
    id: HtbClassId,
    parent: Option<HtbClassId>,
    rate_bps: u64,
    ceil_bps: u64,
    burst: u32,
    cburst: u32,
    priority: u8,
) -> Result<(), &'static str> {
    let mut qdiscs = HTB_QDISCS.lock();
    let q = qdiscs.get_mut(qdisc_name).ok_or("qdisc not found")?;
    let cls = HtbClass::new(id, parent, rate_bps, ceil_bps, burst, cburst, priority);
    q.add_class(cls)?;
    HTB_STATS.classes.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Filtre ekle
pub fn add_filter(qdisc_name: &str, filter: HtbFilter, class_id: HtbClassId) -> Result<(), &'static str> {
    let mut qdiscs = HTB_QDISCS.lock();
    let q = qdiscs.get_mut(qdisc_name).ok_or("qdisc not found")?;
    q.add_filter(filter, class_id);
    Ok(())
}

/// Paket HTB'den geçir
pub fn enqueue(
    qdisc_name: &str,
    size: usize,
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    dst_port: u16,
) -> Result<(), &'static str> {
    let mut qdiscs = HTB_QDISCS.lock();
    let q = qdiscs.get_mut(qdisc_name).ok_or("qdisc not found")?;
    q.enqueue(size, src_ip, dst_ip, dst_port)
}

/// Qdisc istatistiklerini al
pub fn get(name: &str) -> Option<HtbQdisc> {
    HTB_QDISCS.lock().get(name).cloned()
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_id_to_u32_round_trip() {
        let id = HtbClassId::new(1, 10);
        assert_eq!(id.to_u32(), 0x0001_000A);
    }

    #[test]
    fn enqueue_within_rate_passes() {
        let mut q = HtbQdisc::new(HtbClassId::new(1, 1), HtbClassId::new(1, 10), 1500);
        q.add_class(HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            1_000_000,    // 1 Mbps
            1_000_000,
            1500,
            1500,
            0,
        ))
        .unwrap();
        // İlk paket burst'tan gönderilebilir
        let r = q.enqueue(500, Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(0, 0, 0, 0), 0);
        assert!(r.is_ok());
    }

    #[test]
    fn filter_classifies_by_dst_port() {
        let mut q = HtbQdisc::new(HtbClassId::new(1, 1), HtbClassId::new(1, 10), 1500);
        q.add_class(HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            1_000_000,
            1_000_000,
            1500,
            1500,
            0,
        ))
        .unwrap();
        q.add_filter(HtbFilter::DstPort(443), HtbClassId::new(1, 10));
        let id = q.classify(
            Ipv4Addr::new(1, 1, 1, 1),
            Ipv4Addr::new(2, 2, 2, 2),
            443,
        );
        assert_eq!(id, HtbClassId::new(1, 10));
    }

    #[test]
    fn unclassified_packet_uses_default() {
        let q = HtbQdisc::new(HtbClassId::new(1, 1), HtbClassId::new(1, 20), 1500);
        let id = q.classify(
            Ipv4Addr::new(1, 1, 1, 1),
            Ipv4Addr::new(2, 2, 2, 2),
            9999,
        );
        assert_eq!(id, HtbClassId::new(1, 20));
    }

    #[test]
    fn try_send_drops_when_ceil_insufficient() {
        // rate=1Mbps, ceil=2Mbps. burst=1500, cburst=200.
        // İlk 200 byte gider (cburst=200); sonraki 1500 byte gönderilemez
        // (ceil_tokens yetersiz).
        let mut c = HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            1_000_000,
            2_000_000,
            1500,
            200, // küçük cburst: ceil'i hızla tüket
            0,
        );
        // ceil_tokens başlangıçta 200, tokens 1500
        assert!(c.try_send(150)); // ceil_tokens=50, tokens=1350
        // 100 byte göndermek iste: ceil_tokens(50) < 100, drop
        assert!(!c.try_send(100));
        assert_eq!(c.packets_dropped, 1);
    }

    #[test]
    fn try_send_drops_when_rate_tokens_insufficient() {
        // rate=1Mbps ama burst=100, ceil yüksek.
        // İlk 100 byte gider; 50 byte gönderilemez.
        let mut c = HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            1_000_000,
            2_000_000,
            100, // küçük burst
            1500,
            0,
        );
        assert!(c.try_send(80));
        // 30 byte: tokens(20) < 30, drop
        assert!(!c.try_send(30));
        assert_eq!(c.packets_dropped, 1);
    }

    #[test]
    fn refill_adds_proportional_to_elapsed_time() {
        // 8 Mbps = 1_000_000 byte/s = 1000 byte/ms. 1 ms elapsed → 1000 byte.
        let mut c = HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            8_000_000,
            8_000_000,
            10_000, // büyük burst, cap'e takılmasın
            10_000,
            0,
        );
        c.tokens = 0;
        c.ceil_tokens = 0;
        c.last_refill_ticks = 50; // başlangıç referans noktası
        c.refill(51); // 1 ms elapsed
        assert_eq!(c.tokens, 1000);
        assert_eq!(c.ceil_tokens, 1000);
    }

    #[test]
    fn refill_caps_at_burst() {
        // 8 Mbps = 1000 byte/ms. 100 ms elapsed → 100_000 byte, capped at burst=200.
        let mut c = HtbClass::new(
            HtbClassId::new(1, 10),
            Some(HtbClassId::new(1, 1)),
            8_000_000,
            8_000_000,
            200,  // küçük burst
            200,
            0,
        );
        c.tokens = 0;
        c.ceil_tokens = 0;
        c.last_refill_ticks = 50; // başlangıç referans noktası
        c.refill(150); // 100 ms elapsed
        assert_eq!(c.tokens, 200);
        assert_eq!(c.ceil_tokens, 200);
    }
}

