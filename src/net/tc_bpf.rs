//! # tc-bpf (Traffic Control BPF) Classifier
//!
//! Linux TC (Traffic Control) altyapısının eBPF entegrasyonu.
//! Ingress ve egress yönünde eBPF programları ile paket filtreleme sağlar.
//!
//! ## TC Pipeline
//!
//! ```text
//!  Gelen Paket (RX)
//!      │
//!  [tc ingress filter]  ← eBPF program burada çalışır
//!      │                   TC_ACT_OK    → stack'e devam
//!      │                   TC_ACT_SHOT  → paket düşürülür
//!      │                   TC_ACT_STOLEN → paket alınır, stack'e devam etmez
//!      ▼
//!  Netfilter / Routing / Uygulama
//!      │
//!  [tc egress filter]   ← eBPF program burada çalışır
//!      │
//!  ▼
//!  Giden Paket (TX)
//! ```
//!
//! ## Kullanım
//!
//! ```text
//! // tc qdisc add dev eth0 clsact
//! // tc filter add dev eth0 ingress bpf da obj prog.o sec classifier
//! // tc filter add dev eth0 egress bpf da obj prog.o sec classifier
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::ebpf::{EbpfVm, EbpfError};

// ============================================================================
// TC CONSTANTS (Linux uyumlu)
// ============================================================================

/// TC karar kodları (action codes)
pub const TC_ACT_OK: i32 = 0;        // Paketi kabul et, bir sonraki kurala geç
pub const TC_ACT_RECLASSIFY: i32 = 1; // Yeniden sınıflandır
pub const TC_ACT_SHOT: i32 = 2;       // Paketi düşür (drop)
pub const TC_ACT_PIPE: i32 = 3;       // Bir sonraki filter'a ilet
pub const TC_ACT_STOLEN: i32 = 4;     // Paketi al, TX yapma
pub const TC_ACT_REDIRECT: i32 = 7;   // Farklı device'a yönlendir
pub const TC_ACT_REPEAT: i32 = 8;     // Aynı filter'ı tekrar çalıştır

/// TC yön (direction)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcDirection {
    Ingress,
    Egress,
}

/// TC program sonucu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcVerdict {
    /// TC_ACT_* kodu
    pub action: i32,
    /// Yeniden sınıflandırma class ID (TC_ACT_RECLASSIFY için)
pub classid: u32,
}

impl TcVerdict {
    pub const fn ok() -> Self {
        TcVerdict { action: TC_ACT_OK, classid: 0 }
    }
    pub const fn shot() -> Self {
        TcVerdict { action: TC_ACT_SHOT, classid: 0 }
    }
    pub const fn stolen() -> Self {
        TcVerdict { action: TC_ACT_STOLEN, classid: 0 }
    }
    pub const fn redirect() -> Self {
        TcVerdict { action: TC_ACT_REDIRECT, classid: 0 }
    }
}

// ============================================================================
// TC BPF FILTER
// ============================================================================

/// TC'ye atanmış bir eBPF program
#[derive(Clone)]
pub struct TcBpfFilter {
    /// Program ID (unique)
    pub prog_id: u32,
    /// Program adı
    pub prog_name: String,
    /// Priority (düşük = önce çalışır)
    pub priority: u32,
    /// Protocol filtresi (0 = tümü, ETH_P_IP = 0x0800, vb.)
    pub protocol: u16,
    /// eBPF program bytecode
    program: Vec<u64>,
    /// Program tipi (SCHED_CLS veya SCHED_ACT)
    pub prog_type: u32,
    /// Enabled/disabled
    pub enabled: bool,
}

impl TcBpfFilter {
    /// Filtreyi bir paket üzerinde çalıştır
    pub fn classify(&self, packet: &[u8]) -> Result<TcVerdict, EbpfError> {
        if !self.enabled {
            return Ok(TcVerdict::ok());
        }

        let mut vm = EbpfVm::new(self.program.clone(), self.prog_type);

        // Paket başlangıç adresini context olarak kullan
        // (gerçek implementasyonda packet metadata + pointer verilir)
        let result = vm.execute(packet.as_ptr())?;

        Ok(TcVerdict {
            action: result as i32,
            classid: 0,
        })
    }
}

// ============================================================================
// TC BPF CLASSIFIER (per-interface)
// ============================================================================

/// Interface başına tc-bpf classifier yapısı
pub struct TcBpfClassifier {
    /// Interface adı
    pub iface_name: String,
    /// Ingress filtreleri (öncelik sıralı)
    ingress_filters: Vec<TcBpfFilter>,
    /// Egress filtreleri (öncelik sıralı)
    egress_filters: Vec<TcBpfFilter>,
    /// İstatistikler
    pub stats: TcClassifierStats,
}

/// TC classifier istatistikleri
#[derive(Clone, Debug, Default)]
pub struct TcClassifierStats {
    pub ingress_classified: u64,
    pub egress_classified: u64,
    pub ingress_dropped: u64,
    pub egress_dropped: u64,
    pub ingress_passed: u64,
    pub egress_passed: u64,
}

impl TcBpfClassifier {
    pub fn new(iface_name: &str) -> Self {
        TcBpfClassifier {
            iface_name: String::from(iface_name),
            ingress_filters: Vec::new(),
            egress_filters: Vec::new(),
            stats: TcClassifierStats::default(),
        }
    }

    /// Ingress filtre ekle
    pub fn add_ingress_filter(&mut self, filter: TcBpfFilter) {
        self.ingress_filters.push(filter);
        self.ingress_filters.sort_by_key(|f| f.priority);
    }

    /// Egress filtre ekle
    pub fn add_egress_filter(&mut self, filter: TcBpfFilter) {
        self.egress_filters.push(filter);
        self.egress_filters.sort_by_key(|f| f.priority);
    }

    /// Ingress filtre kaldır
    pub fn remove_filter(&mut self, prog_id: u32, direction: TcDirection) -> bool {
        let filters = match direction {
            TcDirection::Ingress => &mut self.ingress_filters,
            TcDirection::Egress => &mut self.egress_filters,
        };
        let len_before = filters.len();
        filters.retain(|f| f.prog_id != prog_id);
        filters.len() < len_before
    }

    /// Ingress paketi sınıflandır
    ///
    /// Filtreler öncelik sırasıyla çalıştırılır.
    /// İlk TC_ACT_SHOT/STOLEN dönen filtre paketi durdurur.
    /// TC_ACT_OK → sonraki filtreye geç.
    pub fn classify_ingress(&mut self, packet: &[u8]) -> TcVerdict {
        self.stats.ingress_classified += 1;

        for filter in &self.ingress_filters {
            if !filter.enabled {
                continue;
            }
            // Protocol filtresi
            if filter.protocol != 0 && packet.len() >= 14 {
                let pkt_proto = u16::from_be_bytes([packet[12], packet[13]]);
                if pkt_proto != filter.protocol {
                    continue;
                }
            }

            match filter.classify(packet) {
                Ok(verdict) => {
                    match verdict.action {
                        TC_ACT_OK | TC_ACT_PIPE | TC_ACT_RECLASSIFY => {
                            // Sonraki filtreye geç
                            self.stats.ingress_passed += 1;
                            continue;
                        }
                        TC_ACT_SHOT => {
                            self.stats.ingress_dropped += 1;
                            return verdict;
                        }
                        TC_ACT_STOLEN => {
                            return verdict;
                        }
                        TC_ACT_REDIRECT => {
                            return verdict;
                        }
                        _ => {
                            return verdict;
                        }
                    }
                }
                Err(_) => {
                    // Hata → kabul et (fail-open)
                    continue;
                }
            }
        }

        // Hiçbir filtre düşürmedi → kabul
        TcVerdict::ok()
    }

    /// Egress paketi sınıflandır
    pub fn classify_egress(&mut self, packet: &[u8]) -> TcVerdict {
        self.stats.egress_classified += 1;

        for filter in &self.egress_filters {
            if !filter.enabled {
                continue;
            }
            if filter.protocol != 0 && packet.len() >= 14 {
                let pkt_proto = u16::from_be_bytes([packet[12], packet[13]]);
                if pkt_proto != filter.protocol {
                    continue;
                }
            }

            match filter.classify(packet) {
                Ok(verdict) => {
                    match verdict.action {
                        TC_ACT_OK | TC_ACT_PIPE | TC_ACT_RECLASSIFY => {
                            self.stats.egress_passed += 1;
                            continue;
                        }
                        TC_ACT_SHOT => {
                            self.stats.egress_dropped += 1;
                            return verdict;
                        }
                        TC_ACT_STOLEN | TC_ACT_REDIRECT => {
                            return verdict;
                        }
                        _ => return verdict,
                    }
                }
                Err(_) => continue,
            }
        }

        TcVerdict::ok()
    }

    /// Filtre sayısını döner
    pub fn filter_count(&self, direction: TcDirection) -> usize {
        match direction {
            TcDirection::Ingress => self.ingress_filters.len(),
            TcDirection::Egress => self.egress_filters.len(),
        }
    }
}

// ============================================================================
// GLOBAL TC REGISTRY
// ============================================================================

/// Global TC classifier registry (interface name → classifier)
static TC_REGISTRY: Mutex<BTreeMap<String, TcBpfClassifier>> = Mutex::new(BTreeMap::new());

/// Sonraki program ID
static NEXT_PROG_ID: AtomicU32 = AtomicU32::new(1);

/// Interface için TC classifier oluştur (yoksa)
pub fn ensure_classifier(iface_name: &str) {
    let mut registry = TC_REGISTRY.lock();
    if !registry.contains_key(iface_name) {
        registry.insert(
            String::from(iface_name),
            TcBpfClassifier::new(iface_name),
        );
    }
}

/// Ingress/egress filtre ata
pub fn attach_tc_prog(
    iface_name: &str,
    direction: TcDirection,
    prog_name: &str,
    priority: u32,
    protocol: u16,
    program: Vec<u64>,
    prog_type: u32,
) -> u32 {
    let mut registry = TC_REGISTRY.lock();
    ensure_classifier_locked(&mut registry, iface_name);

    let prog_id = NEXT_PROG_ID.fetch_add(1, Ordering::Relaxed);
    let filter = TcBpfFilter {
        prog_id,
        prog_name: String::from(prog_name),
        priority,
        protocol,
        program,
        prog_type,
        enabled: true,
    };

    if let Some(classifier) = registry.get_mut(iface_name) {
        match direction {
            TcDirection::Ingress => classifier.add_ingress_filter(filter),
            TcDirection::Egress => classifier.add_egress_filter(filter),
        }
    }

    prog_id
}

fn ensure_classifier_locked(registry: &mut BTreeMap<String, TcBpfClassifier>, iface_name: &str) {
    if !registry.contains_key(iface_name) {
        registry.insert(
            String::from(iface_name),
            TcBpfClassifier::new(iface_name),
        );
    }
}

/// Filtre kaldır
pub fn detach_tc_prog(iface_name: &str, direction: TcDirection, prog_id: u32) -> bool {
    let mut registry = TC_REGISTRY.lock();
    if let Some(classifier) = registry.get_mut(iface_name) {
        classifier.remove_filter(prog_id, direction)
    } else {
        false
    }
}

/// Ingress filtreleme (RX pipeline'da çağrılır)
pub fn classify_ingress(iface_name: &str, packet: &[u8]) -> TcVerdict {
    let mut registry = TC_REGISTRY.lock();
    if let Some(classifier) = registry.get_mut(iface_name) {
        classifier.classify_ingress(packet)
    } else {
        TcVerdict::ok()
    }
}

/// Egress filtreleme (TX pipeline'da çağrılır)
pub fn classify_egress(iface_name: &str, packet: &[u8]) -> TcVerdict {
    let mut registry = TC_REGISTRY.lock();
    if let Some(classifier) = registry.get_mut(iface_name) {
        classifier.classify_egress(packet)
    } else {
        TcVerdict::ok()
    }
}

/// Interface istatistiklerini getir
pub fn get_stats(iface_name: &str) -> Option<TcClassifierStats> {
    let registry = TC_REGISTRY.lock();
    registry.get(iface_name).map(|c| c.stats.clone())
}

/// Filtre listesini getir
pub fn list_filters(iface_name: &str, direction: TcDirection) -> Vec<(u32, String, u32, bool)> {
    let registry = TC_REGISTRY.lock();
    if let Some(classifier) = registry.get(iface_name) {
        let filters = match direction {
            TcDirection::Ingress => &classifier.ingress_filters,
            TcDirection::Egress => &classifier.egress_filters,
        };
        filters.iter().map(|f| (f.prog_id, f.prog_name.clone(), f.priority, f.enabled)).collect()
    } else {
        Vec::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_verdict_actions() {
        assert_eq!(TcVerdict::ok().action, TC_ACT_OK);
        assert_eq!(TcVerdict::shot().action, TC_ACT_SHOT);
        assert_eq!(TcVerdict::stolen().action, TC_ACT_STOLEN);
        assert_eq!(TcVerdict::redirect().action, TC_ACT_REDIRECT);
    }

    #[test]
    fn tc_attach_and_detach_filter() {
        let prog_id = attach_tc_prog(
            "eth0",
            TcDirection::Ingress,
            "test_filter",
            10,
            0,
            vec![0x95u64], // BPF_EXIT instruction
            3, // SCHED_CLS
        );
        assert!(prog_id > 0);

        let filters = list_filters("eth0", TcDirection::Ingress);
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].0, prog_id);

        assert!(detach_tc_prog("eth0", TcDirection::Ingress, prog_id));
        let filters = list_filters("eth0", TcDirection::Ingress);
        assert_eq!(filters.len(), 0);
    }

    #[test]
    fn tc_empty_classifier_passes() {
        ensure_classifier("eth1");
        let verdict = classify_ingress("eth1", &[0u8; 64]);
        assert_eq!(verdict.action, TC_ACT_OK);

        let verdict = classify_egress("eth1", &[0u8; 64]);
        assert_eq!(verdict.action, TC_ACT_OK);
    }

    #[test]
    fn tc_classifier_stats_increment() {
        ensure_classifier("eth2");
        classify_ingress("eth2", &[0u8; 64]);
        classify_ingress("eth2", &[0u8; 64]);
        classify_egress("eth2", &[0u8; 64]);

        let stats = get_stats("eth2").unwrap();
        assert_eq!(stats.ingress_classified, 2);
        assert_eq!(stats.egress_classified, 1);
    }

    #[test]
    fn tc_priority_ordering() {
        // Düşük priority önce çalışır
        attach_tc_prog("eth3", TcDirection::Ingress, "low", 10, 0, vec![], 3);
        attach_tc_prog("eth3", TcDirection::Ingress, "high", 100, 0, vec![], 3);
        attach_tc_prog("eth3", TcDirection::Ingress, "mid", 50, 0, vec![], 3);

        let filters = list_filters("eth3", TcDirection::Ingress);
        assert_eq!(filters.len(), 3);
        assert_eq!(filters[0].1, "low");
        assert_eq!(filters[1].1, "mid");
        assert_eq!(filters[2].1, "high");
    }
}
