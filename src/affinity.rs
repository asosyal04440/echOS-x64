//! # echOS CPU Affinity ve Zamanlayıcı Entegrasyon Modülü
//!
//! Tier 1 OS seviyesinde CPU affinity ve scheduling entegrasyonu.
//! Linux CPU affinity ile aynı seviyede yetenekler.
//!
//! ## CPU Affinity Nedir?
//! CPU affinity, bir görevin (task) hangi işlemci çekirdeklerinde çalışabileceğini
//! belirleyen mekanizmadır. Doğru affinity ayarları:
//! - Önbellek (cache) verimliliğini artırır
//! - NUMA düğümleri arası bellek erişimini azaltır
//! - Gerçek zamanlı görevlerde gecikmeyi (latency) düşürür

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{preempt_enabled, PreemptDisableGuard};
use crate::rcu::{synchronize_rcu, RcuPtr};
use crate::topology::{
    get_cache_sharing_cpus, get_core_cpus, get_package_cpus, get_system_topology,
};
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// CPU affinity maskesi türü.
/// Her bit, bir CPU çekirdeğini temsil eder.
/// Örneğin: 0b0101 = CPU 0 ve CPU 2 çalıştırılabilir.
/// u64 kullanılarak en fazla 64 CPU çekirdeği desteklenir; genişletmek mümkündür.
pub type CpuMask = u64; // En fazla 64 CPU desteklenir, genişletilebilir

/// CPU affinity politikaları.
/// Her politika, görevin hangi CPU çekirdeklerini tercih ettiğini tanımlar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AffinityPolicy {
    /// Affinity kısıtlaması yok — herhangi bir CPU üzerinde çalışabilir
    Any = 0,
    /// Sadece belirtilen CPU'larda çalışmalı (sabit)
    Fixed = 1,
    /// Belirtilen CPU'ları tercih et, ama başkasında da çalışabilir
    Preferred = 2,
    /// Belirtilen CPU'lardan kaçın
    Avoid = 3,
    /// NUMA düğümüne göre affinity
    Numa = 4,
    /// Önbellek paylaşımına göre affinity
    Cache = 5,
    /// Fiziksel işlemci paketine göre affinity
    Package = 6,
}

/// Görev (task) affinity tanımlayıcısı.
/// Her göreve ait CPU tercihlerini ve istatistiklerini tutar.
/// 64 bayta hizalanmış (align(64)) — cache line boyutuna uygun,
/// böylece yanlış paylaşım (false sharing) önlenir.
#[repr(C, align(64))]
pub struct TaskAffinity {
    /// Görev kimliği
    pub task_id: u64,
    /// Mevcut affinity politikası
    pub policy: AtomicU32, // AffinityPolicy değeri u32 olarak saklanır
    /// CPU affinity maskesi (izin verilen CPU'ların bitmask'i)
    pub cpu_mask: AtomicU64,
    /// Tercih edilen CPU maskesi (Preferred politikası için)
    pub preferred_mask: AtomicU64,
    /// Kaçınılan CPU maskesi (Avoid politikası için)
    pub avoid_mask: AtomicU64,
    /// NUMA düğüm tercihi
    pub numa_node: AtomicU32,
    /// Önbellek seviyesi tercihi
    pub cache_level: AtomicU32,
    /// Paket (soket) tercihi
    pub package_id: AtomicU32,
    /// Bu görevin en son çalıştığı CPU kimliği
    pub last_cpu: AtomicU32,
    /// CPU'lar arası göç sayısı — performans analizi için izlenir
    pub migrations: AtomicU64,
    /// Affinity değişiklik sayısı
    pub affinity_changes: AtomicU64,
    /// Yük dengeleme etkin mi?
    pub load_balance: AtomicBool,
    /// Yapışkan affinity — görevi son kullandığı CPU'da tutmaya çalışır
    pub sticky: AtomicBool,
    /// Yanlış paylaşımı (false sharing) önlemek için dolgu
    _padding: [u8; 64 - 56],
}

impl TaskAffinity {
    /// Yeni bir görev affinity örneği oluşturur.
    /// Varsayılan olarak tüm CPU'lara izin verilir (Any politikası).
    pub fn new(task_id: u64) -> Self {
        Self {
            task_id,
            policy: AtomicU32::new(AffinityPolicy::Any as u32),
            cpu_mask: AtomicU64::new(!0u64), // Tüm CPU'lara izin ver
            preferred_mask: AtomicU64::new(0),
            avoid_mask: AtomicU64::new(0),
            numa_node: AtomicU32::new(0),
            cache_level: AtomicU32::new(0),
            package_id: AtomicU32::new(0),
            last_cpu: AtomicU32::new(0),
            migrations: AtomicU64::new(0),
            affinity_changes: AtomicU64::new(0),
            load_balance: AtomicBool::new(true),
            sticky: AtomicBool::new(true),
            _padding: [0; 64 - 56],
        }
    }

    /// Mevcut affinity politikasını döndürür.
    /// Acquire semantiği kullanılır: bu yükleme, önceki tüm yazmaların görünür
    /// olmasını garanti eder (bellek sıralama güvencesi).
    pub fn get_policy(&self) -> AffinityPolicy {
        match self.policy.load(Ordering::Acquire) {
            0 => AffinityPolicy::Any,
            1 => AffinityPolicy::Fixed,
            2 => AffinityPolicy::Preferred,
            3 => AffinityPolicy::Avoid,
            4 => AffinityPolicy::Numa,
            5 => AffinityPolicy::Cache,
            6 => AffinityPolicy::Package,
            _ => AffinityPolicy::Any,
        }
    }

    /// Affinity politikasını ayarlar.
    /// Release semantiği: bu yazma, sonraki tüm okumalar tarafından görülür.
    /// smp_wmb() ile SMP sistemlerde bellek bariyeri eklenir.
    pub fn set_policy(&self, policy: AffinityPolicy) {
        self.policy.store(policy as u32, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }

    /// CPU maskesini döndürür.
    pub fn get_cpu_mask(&self) -> CpuMask {
        self.cpu_mask.load(Ordering::Acquire)
    }

    /// CPU maskesini ayarlar.
    pub fn set_cpu_mask(&self, mask: CpuMask) {
        self.cpu_mask.store(mask, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }

    /// Tercih edilen CPU maskesini döndürür.
    pub fn get_preferred_mask(&self) -> CpuMask {
        self.preferred_mask.load(Ordering::Acquire)
    }

    /// Tercih edilen CPU maskesini ayarlar.
    pub fn set_preferred_mask(&self, mask: CpuMask) {
        self.preferred_mask.store(mask, Ordering::Release);
        smp_wmb();
    }

    /// Kaçınılan CPU maskesini döndürür.
    pub fn get_avoid_mask(&self) -> CpuMask {
        self.avoid_mask.load(Ordering::Acquire)
    }

    /// Kaçınılan CPU maskesini ayarlar.
    pub fn set_avoid_mask(&self, mask: CpuMask) {
        self.avoid_mask.store(mask, Ordering::Release);
        smp_wmb();
    }

    /// NUMA düğüm tercihini döndürür.
    pub fn get_numa_node(&self) -> u32 {
        self.numa_node.load(Ordering::Acquire)
    }

    /// NUMA düğüm tercihini ayarlar.
    pub fn set_numa_node(&self, node: u32) {
        self.numa_node.store(node, Ordering::Release);
        smp_wmb();
    }

    /// Önbellek seviyesi tercihini döndürür.
    pub fn get_cache_level(&self) -> u32 {
        self.cache_level.load(Ordering::Acquire)
    }

    /// Önbellek seviyesi tercihini ayarlar.
    pub fn set_cache_level(&self, level: u32) {
        self.cache_level.store(level, Ordering::Release);
        smp_wmb();
    }

    /// Paket tercihini döndürür.
    pub fn get_package_id(&self) -> u32 {
        self.package_id.load(Ordering::Acquire)
    }

    /// Paket tercihini ayarlar.
    pub fn set_package_id(&self, package: u32) {
        self.package_id.store(package, Ordering::Release);
        smp_wmb();
    }

    /// Görevin en son çalıştığı CPU'yu döndürür.
    pub fn get_last_cpu(&self) -> u32 {
        self.last_cpu.load(Ordering::Acquire)
    }

    /// Görevin en son çalıştığı CPU'yu günceller.
    /// CPU değişmişse göç sayacını artırır — bu performans analizi için önemlidir.
    pub fn set_last_cpu(&self, cpu: u32) {
        let old_cpu = self.last_cpu.load(Ordering::Acquire);
        if old_cpu != cpu {
            self.last_cpu.store(cpu, Ordering::Release);
            self.migrations.fetch_add(1, Ordering::Relaxed);
            smp_wmb();
        }
    }

    /// Toplam CPU göç sayısını döndürür.
    pub fn get_migration_count(&self) -> u64 {
        self.migrations.load(Ordering::Acquire)
    }

    /// Belirtilen CPU'nun bu görev için uygun olup olmadığını kontrol eder.
    ///
    /// Akış diyagramı:
    /// ```text
    /// is_cpu_allowed(cpu)?
    ///   ├── Any     → her zaman true
    ///   ├── Fixed   → cpu_mask bit'i set mi?
    ///   ├── Preferred → preferred_mask'te var VE avoid_mask'te yok mu?
    ///   ├── Avoid   → avoid_mask bit'i sıfır mı?
    ///   ├── Numa    → CPU, tercih edilen NUMA düğümünde mi?
    ///   ├── Cache   → CPU, son CPU ile önbellek paylaşıyor mu?
    ///   └── Package → CPU, tercih edilen pakette mi?
    /// ```
    pub fn is_cpu_allowed(&self, cpu: u32) -> bool {
        let policy = self.get_policy();
        let cpu_bit = 1u64 << cpu;

        match policy {
            AffinityPolicy::Any => true,
            AffinityPolicy::Fixed => (self.get_cpu_mask() & cpu_bit) != 0,
            AffinityPolicy::Preferred => {
                let preferred = self.get_preferred_mask();
                let avoid = self.get_avoid_mask();
                (preferred & cpu_bit) != 0 || ((preferred == 0) && ((avoid & cpu_bit) == 0))
            }
            AffinityPolicy::Avoid => (self.get_avoid_mask() & cpu_bit) == 0,
            AffinityPolicy::Numa => self.is_numa_cpu(cpu),
            AffinityPolicy::Cache => self.is_cache_cpu(cpu),
            AffinityPolicy::Package => self.is_package_cpu(cpu),
        }
    }

    /// CPU'nun tercih edilen NUMA düğümünde olup olmadığını kontrol eder.
    /// NUMA (Non-Uniform Memory Access): farklı bellek bankalarına erişim süreleri
    /// CPU'ya olan fiziksel mesafeye göre değişir — yerel belleğe erişim daha hızlıdır.
    fn is_numa_cpu(&self, cpu: u32) -> bool {
        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.numa_node_id == self.get_numa_node();
            }
        }
        false
    }

    /// CPU'nun tercih edilen önbellek seviyesini paylaşıp paylaşmadığını kontrol eder.
    /// Önbellek paylaşımı: aynı L2/L3 önbelleğini kullanan CPU'lar arası veri
    /// aktarımı çok daha hızlıdır.
    fn is_cache_cpu(&self, cpu: u32) -> bool {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return true; // Tercih yok — hepsi uygun
        }

        // Son CPU ile önbellek paylaşımını kontrol et
        let last_cpu = self.get_last_cpu();
        if last_cpu == cpu {
            return true;
        }

        if let Some(topology) = get_system_topology() {
            let cache_level_u8 = cache_level.min(u8::MAX as u32) as u8;
            let sharing_cpus = topology.get_cache_sharing_cpus(last_cpu, cache_level_u8);
            return sharing_cpus.contains(&cpu);
        }

        false
    }

    /// CPU'nun tercih edilen fiziksel pakette (sokette) olup olmadığını kontrol eder.
    /// Çok soketli sistemlerde, aynı soketteki CPU'lar arası iletişim daha hızlıdır.
    fn is_package_cpu(&self, cpu: u32) -> bool {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return true; // Tercih yok — hepsi uygun
        }

        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.package_id == package_id;
            }
        }

        false
    }

    /// Bu görev için mevcut CPU'lar arasından en uygun olanı seçer.
    /// Aktif politikaya göre farklı seçim algoritması kullanılır.
    pub fn get_best_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let policy = self.get_policy();

        match policy {
            AffinityPolicy::Any => self.get_any_cpu(available_cpus),
            AffinityPolicy::Fixed => self.get_fixed_cpu(available_cpus),
            AffinityPolicy::Preferred => self.get_preferred_cpu(available_cpus),
            AffinityPolicy::Avoid => self.get_avoid_cpu(available_cpus),
            AffinityPolicy::Numa => self.get_numa_cpu(available_cpus),
            AffinityPolicy::Cache => self.get_cache_cpu(available_cpus),
            AffinityPolicy::Package => self.get_package_cpu(available_cpus),
        }
    }

    /// Herhangi bir uygun CPU seçer.
    /// Sticky (yapışkan) modda önce son CPU'ya bakar — önbellek sıcaklığını korur.
    fn get_any_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) {
                return Some(last_cpu);
            }
        }

        // İlk uygun CPU'yu döndür
        available_cpus.first().copied()
    }

    /// Sabit affinity için CPU seçer — sadece cpu_mask'teki CPU'lar uygun.
    fn get_fixed_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let mask = self.get_cpu_mask();

        // Sticky modda önce son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((mask >> last_cpu) & 1) != 0 {
                return Some(last_cpu);
            }
        }

        // İzin verilen ilk CPU'yu bul
        for &cpu in available_cpus {
            if ((mask >> cpu) & 1) != 0 {
                return Some(cpu);
            }
        }

        None
    }

    /// Tercihli affinity için CPU seçer.
    /// Öncelik sırası: sticky CPU → tercih edilen CPU → kaçınılmayan CPU
    fn get_preferred_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let preferred_mask = self.get_preferred_mask();
        let avoid_mask = self.get_avoid_mask();

        // Sticky modda ve kaçınılmıyorsa önce son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }

        // Tercih edilen CPU'ları dene
        for &cpu in available_cpus {
            if ((preferred_mask >> cpu) & 1) != 0 && ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }

        // Kaçınılmayan CPU'ları dene
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }

        None
    }

    /// Kaçınma affinity için CPU seçer — avoid_mask'te olmayan ilk CPU.
    fn get_avoid_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let avoid_mask = self.get_avoid_mask();

        // Sticky modda ve kaçınılmıyorsa son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }

        // Kaçınılmayan ilk CPU'yu bul
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }

        None
    }

    /// NUMA-farkında CPU seçer — tercih edilen NUMA düğümündeki CPU'ları önceliklendirir.
    /// NUMA düğümünde uygun CPU yoksa herhangi bir CPU'ya geri düşer.
    fn get_numa_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let numa_node = self.get_numa_node();

        // Tercih edilen NUMA düğümündeki CPU'ları bul
        let numa_cpus: Vec<u32> = available_cpus
            .iter()
            .filter(|&&cpu| self.is_numa_cpu(cpu))
            .copied()
            .collect();

        if !numa_cpus.is_empty() {
            return self.get_any_cpu(&numa_cpus);
        }

        // Herhangi bir CPU'ya geri dön
        self.get_any_cpu(available_cpus)
    }

    /// Önbellek-farkında CPU seçer — son CPU ile aynı önbelleği paylaşanları önceliklendirir.
    fn get_cache_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return self.get_any_cpu(available_cpus);
        }

        let last_cpu = self.get_last_cpu();
        let cache_level_u8 = cache_level.min(u8::MAX as u32) as u8;
        let cache_cpus = get_cache_sharing_cpus(last_cpu, cache_level_u8);

        // Önbellek paylaşan CPU'ları bul
        let shared_cpus: Vec<u32> = available_cpus
            .iter()
            .filter(|&&cpu| cache_cpus.contains(&cpu))
            .copied()
            .collect();

        if !shared_cpus.is_empty() {
            return self.get_any_cpu(&shared_cpus);
        }

        // Herhangi bir CPU'ya geri dön
        self.get_any_cpu(available_cpus)
    }

    /// Paket-farkında CPU seçer — aynı fiziksel soketteki CPU'ları önceliklendirir.
    fn get_package_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return self.get_any_cpu(available_cpus);
        }

        let package_cpus = get_package_cpus(self.get_last_cpu());

        // Aynı paketteki CPU'ları bul
        let same_package_cpus: Vec<u32> = available_cpus
            .iter()
            .filter(|&&cpu| package_cpus.contains(&cpu))
            .copied()
            .collect();

        if !same_package_cpus.is_empty() {
            return self.get_any_cpu(&same_package_cpus);
        }

        // Herhangi bir CPU'ya geri dön
        self.get_any_cpu(available_cpus)
    }

    /// Yük dengelemeyi etkinleştirir veya devre dışı bırakır.
    pub fn set_load_balance(&self, enabled: bool) {
        self.load_balance.store(enabled, Ordering::Release);
        smp_wmb();
    }

    /// Yapışkan affinity modunu etkinleştirir veya devre dışı bırakır.
    pub fn set_sticky(&self, enabled: bool) {
        self.sticky.store(enabled, Ordering::Release);
        smp_wmb();
    }

    /// Affinity istatistiklerini bir anlık görüntü olarak döndürür.
    pub fn get_stats(&self) -> AffinityStats {
        AffinityStats {
            policy: self.get_policy(),
            cpu_mask: self.get_cpu_mask(),
            preferred_mask: self.get_preferred_mask(),
            avoid_mask: self.get_avoid_mask(),
            numa_node: self.get_numa_node(),
            cache_level: self.get_cache_level(),
            package_id: self.get_package_id(),
            last_cpu: self.get_last_cpu(),
            migrations: self.get_migration_count(),
            affinity_changes: self.affinity_changes.load(Ordering::Relaxed),
            load_balance: self.load_balance.load(Ordering::Acquire),
            sticky: self.sticky.load(Ordering::Acquire),
        }
    }
}

/// Affinity istatistikleri — bir görevin CPU kullanım geçmişini özetler.
#[derive(Debug, Clone, Copy)]
pub struct AffinityStats {
    pub policy: AffinityPolicy,
    pub cpu_mask: CpuMask,
    pub preferred_mask: CpuMask,
    pub avoid_mask: CpuMask,
    pub numa_node: u32,
    pub cache_level: u32,
    pub package_id: u32,
    pub last_cpu: u32,
    pub migrations: u64,
    pub affinity_changes: u64,
    pub load_balance: bool,
    pub sticky: bool,
}

/// CPU affinity yöneticisi.
/// Tüm görevlerin affinity tanımlayıcılarını ve CPU yük bilgilerini yönetir.
pub struct AffinityManager {
    /// Desteklenen maksimum CPU sayısı
    max_cpus: u32,
    /// Görev affinity tanımlayıcıları (RCU korumalı)
    task_affinities: Vec<RcuPtr<TaskAffinity>>,
    /// Her CPU'nun anlık yük değeri
    cpu_loads: Vec<AtomicU32>,
    /// Genel affinity politikası
    global_policy: AtomicU32, // AffinityPolicy değeri u32 olarak
    /// Yük dengeleme etkin mi?
    load_balance_enabled: AtomicBool,
    /// Göç eşiği — bu yük yüzdesinin üzerindeki CPU'lar aşırı yüklü sayılır
    migration_threshold: AtomicU32,
    /// Yönetici istatistikleri
    stats: AffinityManagerStats,
}

/// Affinity yöneticisi istatistikleri.
#[derive(Debug)]
pub struct AffinityManagerStats {
    pub total_affinities: AtomicU64,
    pub total_migrations: AtomicU64,
    pub load_balancing_events: AtomicU64,
    pub affinity_changes: AtomicU64,
}

impl AffinityManagerStats {
    pub const fn new() -> Self {
        Self {
            total_affinities: AtomicU64::new(0),
            total_migrations: AtomicU64::new(0),
            load_balancing_events: AtomicU64::new(0),
            affinity_changes: AtomicU64::new(0),
        }
    }

    pub fn record_affinity(&self) {
        self.total_affinities.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_migration(&self) {
        self.total_migrations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_load_balance(&self) {
        self.load_balancing_events.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_affinity_change(&self) {
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.total_affinities.load(Ordering::Relaxed),
            self.total_migrations.load(Ordering::Relaxed),
            self.load_balancing_events.load(Ordering::Relaxed),
            self.affinity_changes.load(Ordering::Relaxed),
        )
    }
}

impl AffinityManager {
    /// Yeni bir affinity yöneticisi oluşturur.
    /// Her CPU için sıfır yük ile başlatılır.
    pub fn new(max_cpus: u32) -> Self {
        let mut task_affinities = Vec::new();
        let mut cpu_loads = Vec::new();

        // Her CPU için yük izleyici başlat
        for _ in 0..max_cpus {
            cpu_loads.push(AtomicU32::new(0));
        }
        Self {
            max_cpus,
            task_affinities,
            cpu_loads,
            global_policy: AtomicU32::new(AffinityPolicy::Any as u32),
            load_balance_enabled: AtomicBool::new(true),
            migration_threshold: AtomicU32::new(80), // %80 yük eşiği
            stats: AffinityManagerStats::new(),
        }
    }

    /// Belirtilen görev için yeni bir affinity kaydı oluşturur ve döndürür.
    pub fn create_task_affinity(&mut self, task_id: u64) -> RcuPtr<TaskAffinity> {
        let affinity = Box::new(TaskAffinity::new(task_id));
        let affinity_ptr = RcuPtr::new(Box::into_raw(affinity));

        // Vektörün yeterince büyük olduğundan emin ol
        while self.task_affinities.len() <= task_id as usize {
            self.task_affinities
                .push(RcuPtr::new(core::ptr::null_mut()));
        }

        self.task_affinities[task_id as usize] = affinity_ptr.clone();
        self.stats.record_affinity();

        affinity_ptr
    }

    /// Belirtilen görevin affinity kaydını döndürür.
    pub fn get_task_affinity(&self, task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
        if task_id as usize >= self.task_affinities.len() {
            return None;
        }

        let affinity_ptr = self.task_affinities[task_id as usize].clone();
        if affinity_ptr.read().as_ptr().is_null() {
            return None;
        }

        Some(affinity_ptr)
    }

    /// Belirtilen görevin affinity kaydını kaldırır.
    pub fn remove_task_affinity(&mut self, task_id: u64) {
        if (task_id as usize) < self.task_affinities.len() {
            self.task_affinities[task_id as usize] = RcuPtr::new(core::ptr::null_mut());
        }
    }

    /// Görev için en uygun CPU'yu belirler ve last_cpu'yu günceller.
    pub fn get_best_cpu_for_task(&self, task_id: u64) -> Option<u32> {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return None,
        };

        // Uygun (açık ve aşırı yüklü olmayan) CPU'ları al
        let available_cpus = self.get_available_cpus();

        if available_cpus.is_empty() {
            return None;
        }

        let best_cpu = affinity.read().get_best_cpu(&available_cpus);

        if let Some(cpu) = best_cpu {
            // Son CPU'yu güncelle
            affinity.read().set_last_cpu(cpu);

            // CPU yükünü güncelle
            self.update_cpu_load(cpu, 1);
        }

        best_cpu
    }

    /// Açık ve aşırı yüklü olmayan CPU'ların listesini döndürür.
    fn get_available_cpus(&self) -> Vec<u32> {
        let mut available = Vec::new();

        for cpu_id in 0..self.max_cpus {
            // CPU açık mı kontrol et
            if !self.is_cpu_online(cpu_id) {
                continue;
            }

            // Yük dengeleme etkinse aşırı yüklü CPU'ları atla
            if self.load_balance_enabled.load(Ordering::Acquire) {
                let load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
                let threshold = self.migration_threshold.load(Ordering::Acquire);

                if load > threshold {
                    continue;
                }
            }

            available.push(cpu_id);
        }

        available
    }

    /// Belirtilen CPU'nun açık olup olmadığını kontrol eder.
    fn is_cpu_online(&self, cpu_id: u32) -> bool {
        // Gerçek uygulamada hotplug yöneticisine sorulur.
        // Şimdilik tüm CPU'ların açık olduğunu varsay
        cpu_id < self.max_cpus
    }

    /// CPU yükünü artırır veya azaltır.
    /// delta pozitifse yük artar, negatifse azalır (saturating işlem ile taşma önlenir).
    fn update_cpu_load(&self, cpu_id: u32, delta: i32) {
        if cpu_id as usize >= self.cpu_loads.len() {
            return;
        }

        let current_load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
        let new_load = if delta > 0 {
            current_load.saturating_add(delta as u32)
        } else {
            current_load.saturating_sub((-delta) as u32)
        };

        self.cpu_loads[cpu_id as usize].store(new_load, Ordering::Release);
        smp_wmb();
    }

    /// CPU'lar arasında yük dengesi sağlar.
    ///
    /// Algoritma:
    /// ```text
    /// Tüm CPU'ları tara
    ///   ├── Yük > eşik → aşırı yüklü listesine ekle
    ///   └── Yük < eşik/2 → az yüklü listesine ekle
    /// Aşırı yüklü CPU'lar için taşınabilir görevleri bul
    /// Az yüklü CPU'lara göçür
    /// ```
    pub fn balance_load(&self) {
        if !self.load_balance_enabled.load(Ordering::Acquire) {
            return;
        }

        let threshold = self.migration_threshold.load(Ordering::Acquire);
        let mut overloaded_cpus = Vec::new();
        let mut underloaded_cpus = Vec::new();

        // Aşırı ve az yüklü CPU'ları bul
        for cpu_id in 0..self.max_cpus {
            let load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);

            if load > threshold {
                overloaded_cpus.push((cpu_id, load));
            } else if load < threshold / 2 {
                underloaded_cpus.push(cpu_id);
            }
        }

        // Görevleri aşırı yüklü CPU'lardan az yüklü olanlara taşı
        for &(overloaded_cpu, _) in &overloaded_cpus {
            if underloaded_cpus.is_empty() {
                break;
            }

            // Taşınabilir görevleri bul
            let migratable_tasks = self.find_migratable_tasks(overloaded_cpu);

            for task_id in migratable_tasks {
                if underloaded_cpus.is_empty() {
                    break;
                }

                if let Some(target_cpu) = underloaded_cpus.pop() {
                    if self.migrate_task(task_id, target_cpu) {
                        self.stats.record_load_balance();
                    }
                }
            }
        }
    }

    /// Belirtilen CPU'dan taşınabilecek görevleri bulur.
    fn find_migratable_tasks(&self, cpu_id: u32) -> Vec<u64> {
        let mut migratable = Vec::new();

        // Gerçek uygulamada belirtilen CPU'da çalışan görevler bulunur.
        // Şimdilik boş liste döner
        migratable
    }

    /// Görevi farklı bir CPU'ya taşır.
    /// Affinity politikası hedef CPU'ya izin vermiyorsa göç gerçekleşmez.
    fn migrate_task(&self, task_id: u64, target_cpu: u32) -> bool {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return false,
        };

        // Hedef CPU'nun affinity tarafından izin verilip verilmediğini kontrol et
        if !affinity.read().is_cpu_allowed(target_cpu) {
            return false;
        }

        // CPU yüklerini güncelle
        let current_cpu = affinity.read().get_last_cpu();
        self.update_cpu_load(current_cpu, -1);
        self.update_cpu_load(target_cpu, 1);

        // Affinity bilgisini güncelle
        affinity.read().set_last_cpu(target_cpu);

        // Göç kaydını tut
        affinity.read().migrations.fetch_add(1, Ordering::Relaxed);
        self.stats.record_migration();

        crate::serial_println!(
            "Affinity: Görev {} CPU {}'den CPU {}'e taşındı",
            task_id,
            current_cpu,
            target_cpu
        );

        true
    }

    /// Genel/küresel affinity politikasını ayarlar.
    pub fn set_global_policy(&self, policy: AffinityPolicy) {
        self.global_policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }

    /// Genel affinity politikasını döndürür.
    pub fn get_global_policy(&self) -> AffinityPolicy {
        match self.global_policy.load(Ordering::Acquire) {
            0 => AffinityPolicy::Any,
            1 => AffinityPolicy::Fixed,
            2 => AffinityPolicy::Preferred,
            3 => AffinityPolicy::Avoid,
            4 => AffinityPolicy::Numa,
            5 => AffinityPolicy::Cache,
            6 => AffinityPolicy::Package,
            _ => AffinityPolicy::Any,
        }
    }

    /// Yük dengelemeyi etkinleştirir veya devre dışı bırakır.
    pub fn set_load_balance_enabled(&self, enabled: bool) {
        self.load_balance_enabled.store(enabled, Ordering::Release);
        smp_wmb();
    }

    /// Göç eşiğini ayarlar (0-100 yüzde değeri).
    pub fn set_migration_threshold(&self, threshold: u32) {
        self.migration_threshold.store(threshold, Ordering::Release);
        smp_wmb();
    }

    /// Belirtilen CPU'nun anlık yük değerini döndürür.
    pub fn get_cpu_load(&self, cpu_id: u32) -> Option<u32> {
        if cpu_id as usize >= self.cpu_loads.len() {
            return None;
        }

        Some(self.cpu_loads[cpu_id as usize].load(Ordering::Acquire))
    }

    /// Tüm CPU'ların yük değerlerini (cpu_id, yük) çifti olarak döndürür.
    pub fn get_all_cpu_loads(&self) -> Vec<(u32, u32)> {
        let mut loads = Vec::new();

        for cpu_id in 0..self.max_cpus {
            if let Some(load) = self.get_cpu_load(cpu_id) {
                loads.push((cpu_id, load));
            }
        }

        loads
    }

    /// Yönetici istatistiklerini döndürür.
    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        self.stats.get_stats()
    }
}

/// Global affinity yöneticisi örneği.
/// unsafe kullanımı: çekirdek başlatma sırasında tek thread'den erişilir,
/// sonrasında atomik işlemler ile güvenli erişim sağlanır.
static mut AFFINITY_MANAGER: Option<AffinityManager> = None;
static AFFINITY_INIT: AtomicBool = AtomicBool::new(false);

/// Affinity alt sistemini başlatır.
/// Çift başlatmayı önlemek için atomik flag kontrol edilir.
pub fn init(max_cpus: u32) {
    if AFFINITY_INIT.load(Ordering::Acquire) {
        return;
    }

    crate::serial_println!("Affinity: {} CPU için CPU affinity başlatılıyor", max_cpus);

    let manager = AffinityManager::new(max_cpus);

    unsafe {
        AFFINITY_MANAGER = Some(manager);
    }

    AFFINITY_INIT.store(true, Ordering::Release);
    smp_mb();

    crate::serial_println!("Affinity: CPU affinity başlatıldı");
}

/// Global affinity yöneticisine yalnızca okunur referans döndürür.
pub fn get_manager() -> Option<&'static AffinityManager> {
    if !AFFINITY_INIT.load(Ordering::Acquire) {
        return None;
    }

    unsafe { AFFINITY_MANAGER.as_ref() }
}

/// Kolaylık fonksiyonları — harici modüller için kısa arayüz.
pub fn create_task_affinity(task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
    if let Some(manager) = get_manager() {
        // Gerçek uygulamada mutable erişim gerekir.
        // Şimdilik None döner
        None
    } else {
        None
    }
}

pub fn get_task_affinity(task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
    get_manager()?.get_task_affinity(task_id)
}

pub fn get_best_cpu_for_task(task_id: u64) -> Option<u32> {
    get_manager()?.get_best_cpu_for_task(task_id)
}

pub fn balance_load() {
    if let Some(manager) = get_manager() {
        manager.balance_load();
    }
}

pub fn get_cpu_load(cpu_id: u32) -> Option<u32> {
    get_manager()?.get_cpu_load(cpu_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_affinity() {
        let affinity = TaskAffinity::new(123);

        assert_eq!(affinity.get_policy(), AffinityPolicy::Any);
        assert_eq!(affinity.get_cpu_mask(), !0u64);

        affinity.set_policy(AffinityPolicy::Fixed);
        affinity.set_cpu_mask(0b1010);

        assert_eq!(affinity.get_policy(), AffinityPolicy::Fixed);
        assert_eq!(affinity.get_cpu_mask(), 0b1010);
        assert!(affinity.is_cpu_allowed(1));
        assert!(!affinity.is_cpu_allowed(2));
    }

    #[test]
    fn test_affinity_manager() {
        let manager = AffinityManager::new(4);

        assert_eq!(manager.get_global_policy(), AffinityPolicy::Any);
        assert!(manager.load_balance_enabled.load(Ordering::Acquire));

        manager.set_global_policy(AffinityPolicy::Numa);
        assert_eq!(manager.get_global_policy(), AffinityPolicy::Numa);
    }
}
