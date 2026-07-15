//! # VRF (Virtual Routing and Forwarding)
//!
//! Linux 4.4+ ile gelen VRF, tek bir fiziksel makinede birden fazla
//! bağımsız routing tablosu (ve adres ailesi) sağlar. Her VRF ayrı bir
//! namespace gibi davranır; yönlendirme kararları VRF'in kendi tablosuna
//! göre verilir.
//!
//! ## VRF vs Network Namespace
//!
//! | Özellik | VRF | Netns |
//! |---------|-----|-------|
//! | İzole routing tablosu | ✓ | ✓ |
//! | İzole arayüz seti | ✗ (tüm arayüzler paylaşılır) | ✓ |
//! | Process izolasyonu | ✗ | ✓ |
//! | Hafif/Cheap | ✓ (sadece FIB) | ✗ (ağır) |
//!
//! VRF'in tipik kullanımı: aynı fiziksel arayüz üzerinden birden fazla
//! müşteri/servis için ayrı routing tabloları (ör. ISP multi-tenant).
//!
//! ## VRF Mimari
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ Routing Decision                            │
//! │   if socket bound to vrf blue → blue FIB   │
//! │   elif socket bound to vrf red → red FIB   │
//! │   else → default (main) FIB                │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## echOS Tasarımı
//!
//! `Vrf` ad + ayrı routing tablosu (clone of RoutingManager state).
//! VRF-aware routing kararı `vrf::lookup_route(vrf_name, dst)` ile.

use super::{Ipv4Addr, Mutex};
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// VRF YAPISI
// ============================================================================

#[derive(Clone, Debug)]
pub struct Vrf {
    pub name: String,
    pub table_id: u32, // Linux: 1-252 (local=255, main=254, default=253)
    pub routes: Vec<VrfRoute>,
    pub interfaces: Vec<String>, // Bu VRF'e bağlı interface'ler
    pub up: bool,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

#[derive(Clone, Debug)]
pub struct VrfRoute {
    pub dst: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub interface: String,
    pub metric: u32,
}

impl Vrf {
    pub fn new(name: String, table_id: u32) -> Self {
        Vrf {
            name,
            table_id,
            routes: Vec::new(),
            interfaces: Vec::new(),
            // Linux: VRF admin-up yöneticinin `ip link set vrfX up` ile
            // açıkça etkinleştirmesi gerekir. echOS: default down.
            up: false,
            rx_packets: 0,
            tx_packets: 0,
        }
    }

    /// Route ekle
    pub fn add_route(&mut self, route: VrfRoute) {
        self.routes.retain(|r| !(r.dst == route.dst && r.mask == route.mask));
        self.routes.push(route);
        // En iyi (en uzun prefix) başa
        self.routes.sort_by(|a, b| {
            let a_bits: u32 = a.mask.0.iter().map(|b| b.count_ones() as u32).sum();
            let b_bits: u32 = b.mask.0.iter().map(|b| b.count_ones() as u32).sum();
            b_bits.cmp(&a_bits)
        });
    }

    /// Route sil
    pub fn del_route(&mut self, dst: Ipv4Addr, mask: Ipv4Addr) -> bool {
        let before = self.routes.len();
        self.routes.retain(|r| !(r.dst == dst && r.mask == mask));
        before != self.routes.len()
    }

    /// LPM (Longest Prefix Match) ile route arama
    pub fn lookup(&self, dst: Ipv4Addr) -> Option<&VrfRoute> {
        // routes zaten en uzun prefix'ten başa sıralı
        self.routes
            .iter()
            .find(|r| {
                let d = u32::from_be_bytes(dst.0);
                let r_dst = u32::from_be_bytes(r.dst.0);
                let r_mask = u32::from_be_bytes(r.mask.0);
                (d & r_mask) == (r_dst & r_mask)
            })
    }

    /// Interface'i VRF'e ata
    pub fn assign_interface(&mut self, iface: &str) {
        if !self.interfaces.iter().any(|i| i == iface) {
            self.interfaces.push(String::from(iface));
        }
    }

    /// Interface'i VRF'ten çıkar
    pub fn unassign_interface(&mut self, iface: &str) {
        self.interfaces.retain(|i| i != iface);
    }
}

// ============================================================================
// KÜRESEL DURUM
// ============================================================================

static VRF_TABLE: Mutex<BTreeMap<String, Vrf>> = Mutex::new(BTreeMap::new());

static VRF_STATS: VrfStats = VrfStats::new();
struct VrfStats {
    vrfs: AtomicU32,
    routes: AtomicU32,
    lookups: AtomicU32,
    misses: AtomicU32,
}
impl VrfStats {
    const fn new() -> Self {
        VrfStats {
            vrfs: AtomicU32::new(0),
            routes: AtomicU32::new(0),
            lookups: AtomicU32::new(0),
            misses: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni VRF oluştur
pub fn create(name: &str, table_id: u32) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    if table.contains_key(name) {
        return Err(VrfError::AlreadyExists);
    }
    table.insert(String::from(name), Vrf::new(String::from(name), table_id));
    VRF_STATS.vrfs.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!("[VRF] created {} (table {})", name, table_id);
    Ok(())
}

/// VRF sil
pub fn destroy(name: &str) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    table.remove(name).ok_or(VrfError::NotFound)?;
    VRF_STATS.vrfs.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// Route ekle
pub fn add_route(
    vrf_name: &str,
    dst: Ipv4Addr,
    mask: Ipv4Addr,
    gw: Ipv4Addr,
    iface: &str,
    metric: u32,
) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    let vrf = table.get_mut(vrf_name).ok_or(VrfError::NotFound)?;
    vrf.add_route(VrfRoute {
        dst,
        mask,
        gateway: gw,
        interface: String::from(iface),
        metric,
    });
    VRF_STATS.routes.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Route sil
pub fn del_route(
    vrf_name: &str,
    dst: Ipv4Addr,
    mask: Ipv4Addr,
) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    let vrf = table.get_mut(vrf_name).ok_or(VrfError::NotFound)?;
    if !vrf.del_route(dst, mask) {
        return Err(VrfError::NotFound);
    }
    Ok(())
}

/// VRF'i admin-up yap (Linux `ip link set vrfX up`)
pub fn set_up(vrf_name: &str, up: bool) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    let vrf = table.get_mut(vrf_name).ok_or(VrfError::NotFound)?;
    vrf.up = up;
    Ok(())
}

/// Route lookup
pub fn lookup_route(vrf_name: &str, dst: Ipv4Addr) -> Option<VrfRoute> {
    let table = VRF_TABLE.lock();
    let vrf = table.get(vrf_name)?;
    VRF_STATS.lookups.fetch_add(1, Ordering::Relaxed);
    let r = vrf.lookup(dst).cloned();
    if r.is_none() {
        VRF_STATS.misses.fetch_add(1, Ordering::Relaxed);
    }
    r
}

/// Interface'i VRF'e ata
pub fn assign_iface(vrf_name: &str, iface: &str) -> Result<(), VrfError> {
    let mut table = VRF_TABLE.lock();
    let vrf = table.get_mut(vrf_name).ok_or(VrfError::NotFound)?;
    vrf.assign_interface(iface);
    Ok(())
}

/// VRF listele
pub fn list() -> Vec<String> {
    VRF_TABLE.lock().keys().cloned().collect()
}

/// Default VRF adı (table 254, Linux "main")
pub const DEFAULT_VRF: &'static str = "main";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrfError {
    NotFound,
    AlreadyExists,
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_match_wins() {
        let mut v = Vrf::new("test".into(), 100);
        // Daha kısa prefix önce eklenir
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 0, 0, 0),
            mask: Ipv4Addr::new(255, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 1, 0, 0),
            mask: Ipv4Addr::new(255, 255, 0, 0),
            gateway: Ipv4Addr::new(10, 1, 0, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        // Sıralama sonrası /16 önce
        assert_eq!(v.routes[0].mask, Ipv4Addr::new(255, 255, 0, 0));
        let r = v.lookup(Ipv4Addr::new(10, 1, 5, 7)).unwrap();
        assert_eq!(r.gateway, Ipv4Addr::new(10, 1, 0, 1));
    }

    #[test]
    fn lookup_returns_none_for_unmatched() {
        let v = Vrf::new("test".into(), 100);
        let r = v.lookup(Ipv4Addr::new(192, 168, 1, 1));
        assert!(r.is_none());
    }

    #[test]
    fn interface_assignment() {
        let mut v = Vrf::new("test".into(), 100);
        v.assign_interface("eth0");
        v.assign_interface("eth1");
        v.unassign_interface("eth0");
        assert_eq!(v.interfaces, vec!["eth1"]);
    }

    #[test]
    fn new_vrf_starts_down() {
        let v = Vrf::new("mgmt".into(), 10);
        assert!(!v.up);
    }

    #[test]
    fn add_route_replaces_existing_with_same_dst_mask() {
        let mut v = Vrf::new("test".into(), 100);
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 0, 0, 0),
            mask: Ipv4Addr::new(255, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 0, 0, 0),
            mask: Ipv4Addr::new(255, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 0, 0, 99), // değişen gateway
            interface: "eth0".into(),
            metric: 5,
        });
        assert_eq!(v.routes.len(), 1);
        assert_eq!(v.routes[0].gateway, Ipv4Addr::new(10, 0, 0, 99));
        assert_eq!(v.routes[0].metric, 5);
    }

    #[test]
    fn del_route_returns_true_when_present() {
        let mut v = Vrf::new("test".into(), 100);
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 0, 0, 0),
            mask: Ipv4Addr::new(255, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        assert!(v.del_route(Ipv4Addr::new(10, 0, 0, 0), Ipv4Addr::new(255, 0, 0, 0)));
        assert!(!v.del_route(Ipv4Addr::new(10, 0, 0, 0), Ipv4Addr::new(255, 0, 0, 0)));
    }

    #[test]
    fn mask_prefix_bits_count_correctly() {
        // /8: 8 bit; /16: 16; /24: 24
        let mut v = Vrf::new("test".into(), 100);
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(10, 0, 0, 0),
            mask: Ipv4Addr::new(255, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 0, 0, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        v.add_route(VrfRoute {
            dst: Ipv4Addr::new(192, 168, 1, 0),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: Ipv4Addr::new(192, 168, 1, 1),
            interface: "eth0".into(),
            metric: 0,
        });
        // /24 önce (24 bit > 8 bit)
        assert_eq!(v.routes[0].mask, Ipv4Addr::new(255, 255, 255, 0));
    }
}

