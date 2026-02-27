//! # echOS CPU Affinite ve Zamanlama Entegrasyon Modulu
//!
//! Tier 1 OS seviyesinde CPU affinite ve scheduling entegrasyonu.
//! sched_setaffinity benzeri bir arayuz ile gorevlerin hangi CPU'larda
//! calisabilecegi belirlenir; NUMA, onbellek ve paket farkindaliği desteklenir.
//! Linux CPU affinity ile aynı seviyede yetenekler

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{PreemptDisableGuard, preempt_enabled};
use crate::rcu::{RcuPtr, synchronize_rcu};
use crate::topology::{get_system_topology, get_cache_sharing_cpus, get_package_cpus, get_core_cpus};

/// CPU affinite maskesi turu -- hangi CPU'larin izinli olduğunu bit konumlarıyla ifade eder.
/// Bit N = 1 ise CPU N izinlidir (en dusuk bit = CPU 0).
pub type CpuMask = u64; // 64 CPU'ya kadar destek; genisletilebilir

/// CPU affinite politikaları -- bir gorevin CPU secimini hangi kurala gore yapacagını belirler.
/// Linux'ta sched_setaffinity ile yapılan cagrılar bu politikalara denk duser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AffinityPolicy {
    /// Affinite kısıtlaması yok -- gorev herhangi bir CPU'da calısabilir (varsayılan)
    Any = 0,
    /// Gorev yalnızca belirtilen CPU'larda calısmalı -- gercek zamanlı gorevler icin idealdir
    Fixed = 1,
    /// Belirli CPU'lar tercih edilir; ancak gerekirse baskalarında da calısabilir
    Preferred = 2,
    /// Belirtilen CPU'lardan kacınılır -- yuk dagılımında kritik CPU'ları rezerve etmek icin
    Avoid = 3,
    /// NUMA farkında affinite -- bellek erisim gecikmesini azaltmak icin aynı NUMA dugumunde kal
    Numa = 4,
    /// Onbellek farkında affinite -- L2/L3 onbellegini paylasan CPU'lar tercih edilir
    Cache = 5,
    /// Paket farkında affinite -- aynı soket/paket icindeki CPU'lar tercih edilir
    Package = 6,
}

/// Gorev affinite tanımlayıcısı -- bir gorevin CPU affinite politikasını,
/// izin verilen/tercih edilen/kacınılan CPU maskelerini ve calısma istatistiklerini tutar.
/// Islemci basına bir ornek bulunur; satır boyutu hizalamasıyla yanlıs paylasim onlenir.
#[repr(C, align(64))]
pub struct TaskAffinity {
    /// Gorev kimliği -- scheduler tarafından atanan benzersiz task ID
    pub task_id: u64,
    /// Mevcut affinite politikası -- hangi CPU secim kuralının gecerli olduğunu gosterir
    pub policy: AtomicU32, // AffinityPolicy u32 olarak atomik saklanır
    /// CPU affinite maskesi -- izinli CPU'ların bit maskesi; bit N = CPU N izinli
    pub cpu_mask: AtomicU64,
    /// Tercih edilen CPU maskesi -- Preferred politikasında once denenen CPU'lar
    pub preferred_mask: AtomicU64,
    /// Kacınılacak CPU maskesi -- Avoid politikasında atlanan CPU'lar
    pub avoid_mask: AtomicU64,
    /// NUMA dugumu tercihi -- hangi NUMA dugumunde calısılmak istendiğini belirtir
    pub numa_node: AtomicU32,
    /// Onbellek seviyesi tercihi -- paylasilan onbellek seviyesi (0=yok, 1=L1, 2=L2, 3=L3)
    pub cache_level: AtomicU32,
    /// Paket tercihi -- tercih edilen soket/paket kimliği
    pub package_id: AtomicU32,
    /// Bu gorevin en son calıstıgı CPU -- yapıskan affinite hesaplamalarında kullanılır
    pub last_cpu: AtomicU32,
    /// Goc sayısı -- gorevin kac kez farklı bir CPU'ya tasındıgını sayar
    pub migrations: AtomicU64,
    /// Affinite degisim sayısı -- politika veya maske kac kez guncellendi
    pub affinity_changes: AtomicU64,
    /// Yuk dengeleme etkin mi -- false ise gorev hicbir zaman baska CPU'ya tasinmaz
    pub load_balance: AtomicBool,
    /// Yapıskan affinite -- true ise gorev mumkunse son calıstıgı CPU'yu tercih eder
    pub sticky: AtomicBool,
    /// Yanlıs paylasimi onlemek icin satır dolgusu -- farklı gorev kayıtları aynı onbellek satırına dusmez
    _padding: [u8; 64 - 56],
}

impl TaskAffinity {
    /// Yeni gorev affinitesi olusturur; baslangicta tum CPU'lara izin verilir (Any politikası)
    pub fn new(task_id: u64) -> Self {
        Self {
            task_id,
            policy: AtomicU32::new(AffinityPolicy::Any as u32),
            cpu_mask: AtomicU64::new(!0u64), // Tum CPU'lara izin verilir; tum bitler 1
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
    
    /// Mevcut affinite politikasını dondurur -- atomik okuma ile veri tutarlılıği saglanır
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
    
    /// Affinite politikasını ayarlar ve degisim sayıcısını arttırır;
    /// smp_wmb() ile diger cekirdeklerin yeni politikayı gormesi garanti altına alınır
    pub fn set_policy(&self, policy: AffinityPolicy) {
        self.policy.store(policy as u32, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }
    
    /// CPU maskesini dondurur -- hangi CPU'lara izin verildiğini bit duzeninde gosterir
    pub fn get_cpu_mask(&self) -> CpuMask {
        self.cpu_mask.load(Ordering::Acquire)
    }
    
    /// CPU maskesini ayarlar -- sched_setaffinity cagrısının dogrudan karsiligıdır
    pub fn set_cpu_mask(&self, mask: CpuMask) {
        self.cpu_mask.store(mask, Ordering::Release);
        self.affinity_changes.fetch_add(1, Ordering::Relaxed);
        smp_wmb();
    }
    
    /// Tercih edilen CPU maskesini dondurur -- Preferred politikasında once denenen CPU'lar
    pub fn get_preferred_mask(&self) -> CpuMask {
        self.preferred_mask.load(Ordering::Acquire)
    }
    
    /// Tercih edilen CPU maskesini ayarlar -- zorunlu degil, oncelikli CPU listesi
    pub fn set_preferred_mask(&self, mask: CpuMask) {
        self.preferred_mask.store(mask, Ordering::Release);
        smp_wmb();
    }
    
    /// Kacınılacak CPU maskesini dondurur -- bu CPU'lar mumkunse secilmez
    pub fn get_avoid_mask(&self) -> CpuMask {
        self.avoid_mask.load(Ordering::Acquire)
    }
    
    /// Kacınılacak CPU maskesini ayarlar -- kritik is akısları icin bazı CPU'ları devre dısı bırakmak icin kullanılır
    pub fn set_avoid_mask(&self, mask: CpuMask) {
        self.avoid_mask.store(mask, Ordering::Release);
        smp_wmb();
    }
    
    /// NUMA dugumu tercihini dondurur -- bellek lokalitesini korumak icin kullanılır
    pub fn get_numa_node(&self) -> u32 {
        self.numa_node.load(Ordering::Acquire)
    }
    
    /// NUMA dugumu tercihini ayarlar -- farklı NUMA dugumlerine erisim uzak bellek gecikmesi dogurur
    pub fn set_numa_node(&self, node: u32) {
        self.numa_node.store(node, Ordering::Release);
        smp_wmb();
    }
    
    /// Onbellek seviyesi tercihini dondurur (0=yok, 1=L1, 2=L2, 3=L3)
    pub fn get_cache_level(&self) -> u32 {
        self.cache_level.load(Ordering::Acquire)
    }
    
    /// Onbellek seviyesi tercihini ayarlar -- aynı L3 onbellegini paylasan CPU'larda calismak icin 3 sec
    pub fn set_cache_level(&self, level: u32) {
        self.cache_level.store(level, Ordering::Release);
        smp_wmb();
    }
    
    /// Paket tercihini dondurur -- aynı fiziksel sokette kalmak gecikmeyi azaltır
    pub fn get_package_id(&self) -> u32 {
        self.package_id.load(Ordering::Acquire)
    }
    
    /// Paket tercihini ayarlar -- gorevi belirli bir fiziksel sokete baglar
    pub fn set_package_id(&self, package: u32) {
        self.package_id.store(package, Ordering::Release);
        smp_wmb();
    }
    
    /// Son calısılan CPU'yu dondurur -- yapıskan affinite ve gecis istatistiklerinde kullanılır
    pub fn get_last_cpu(&self) -> u32 {
        self.last_cpu.load(Ordering::Acquire)
    }
    
    /// Son calısılan CPU'yu gunceller; CPU degismisse goc sayıcısı arttırılır
    pub fn set_last_cpu(&self, cpu: u32) {
        let old_cpu = self.last_cpu.load(Ordering::Acquire);
        if old_cpu != cpu {
            self.last_cpu.store(cpu, Ordering::Release);
            self.migrations.fetch_add(1, Ordering::Relaxed);
            smp_wmb();
        }
    }
    
    /// Goc sayısını dondurur -- yuksek deger affinite politikasının yetersiz kaldigını gosterebilir
    pub fn get_migration_count(&self) -> u64 {
        self.migrations.load(Ordering::Acquire)
    }
    
    /// CPU'nun affinite kuralları tarafından izin verilip verilmediğini kontrol eder.
    /// Politikaya gore farklı maskeler ve topoloji bilgisi kullanılarak karar verilir.
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
    
    /// CPU'nun tercih edilen NUMA dugumune ait olup olmadıgını kontrol eder.
    /// NUMA dugumler arası gecis uzak bellege erisim (remote memory access) demektir.
    fn is_numa_cpu(&self, cpu: u32) -> bool {
        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.numa_node_id == self.get_numa_node();
            }
        }
        false
    }
    
    /// CPU'nun tercih edilen onbellek seviyesini paylasip paylasmadıgını kontrol eder.
    /// Ayni L3 onbellegini paylasan CPU'lar arasi gecis bellek gecikmesini minimize eder.
    fn is_cache_cpu(&self, cpu: u32) -> bool {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return true; // Tercih belirtilmemis; tum CPU'lara izin ver
        }
        
        // Bu CPU'nun son calısılan CPU ile aynı onbellek havuzunu paylasip paylasmadıgını kontrol et
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
    
    /// CPU'nun tercih edilen pakette (sokette) olup olmadıgını kontrol eder.
    /// Aynı fiziksel soketteki CPU'lar arasi iletisim daha duş uk gecikmeli olmaktadır.
    fn is_package_cpu(&self, cpu: u32) -> bool {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return true; // Tercih belirtilmemis; tum CPU'lara izin ver
        }
        
        if let Some(topology) = get_system_topology() {
            if let Some(cpu_topology) = topology.get_cpu_topology(cpu) {
                let guard = cpu_topology.read();
                return guard.package_id == package_id;
            }
        }
        
        false
    }
    
    /// Bu gorev icin en iyi CPU'yu secip dondurur; politikaya gore farklı secim stratejileri uygulanır
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
    
    /// Herhangi bir musait CPU'yu dondurur; sticky ise son CPU once denenir
    fn get_any_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) {
                return Some(last_cpu);
            }
        }
        
        // Listedeki ilk musait CPU'yu dondur
        available_cpus.first().copied()
    }
    
    /// Sabit affinite politikası icin CPU sec -- yalnızca cpu_mask'te isaretli CPU'lar gecerlidir
    fn get_fixed_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let mask = self.get_cpu_mask();
        
        // Sticky ise once son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((mask >> last_cpu) & 1) != 0 {
                return Some(last_cpu);
            }
        }
        
        // Maskede isaretli ilk CPU'yu bul
        for &cpu in available_cpus {
            if ((mask >> cpu) & 1) != 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// Tercihli affinite politikası icin CPU sec -- once tercihli, sonra kacınılmayan, en son herhangi biri
    fn get_preferred_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let preferred_mask = self.get_preferred_mask();
        let avoid_mask = self.get_avoid_mask();
        
        // Sticky ise ve kacınılmıyorsa once son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }
        
        // Tercihli CPU'ları dene (avoid maskesinde olmayan)
        for &cpu in available_cpus {
            if ((preferred_mask >> cpu) & 1) != 0 && ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        // Kacınılmayan herhangi bir CPU'yu dene
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// Kacınma affinite politikası icin CPU sec -- avoid maskesinde olmayan CPU'lar tercih edilir
    fn get_avoid_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let avoid_mask = self.get_avoid_mask();
        
        // Sticky ise ve kacınılmıyorsa once son CPU'yu dene
        if self.sticky.load(Ordering::Acquire) {
            let last_cpu = self.get_last_cpu();
            if available_cpus.contains(&last_cpu) && ((avoid_mask >> last_cpu) & 1) == 0 {
                return Some(last_cpu);
            }
        }
        
        // Kacınılmayan ilk CPU'yu bul
        for &cpu in available_cpus {
            if ((avoid_mask >> cpu) & 1) == 0 {
                return Some(cpu);
            }
        }
        
        None
    }
    
    /// NUMA farkında CPU sec -- once tercih edilen NUMA dugumundeki CPU'lar denenir
    fn get_numa_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let numa_node = self.get_numa_node();
        
        // Tercih edilen NUMA dugumundeki CPU'ları bul
        let numa_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| self.is_numa_cpu(cpu))
            .copied()
            .collect();
        
        if !numa_cpus.is_empty() {
            return self.get_any_cpu(&numa_cpus);
        }
        
        // NUMA eslesme yoksa herhangi bir CPU'ya don
        self.get_any_cpu(available_cpus)
    }
    
    /// Onbellek farkında CPU sec -- son CPU ile aynı onbellek havuzunu paylasan CPU'lar tercih edilir
    fn get_cache_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let cache_level = self.get_cache_level();
        if cache_level == 0 {
            return self.get_any_cpu(available_cpus);
        }
        
        let last_cpu = self.get_last_cpu();
        let cache_level_u8 = cache_level.min(u8::MAX as u32) as u8;
        let cache_cpus = get_cache_sharing_cpus(last_cpu, cache_level_u8);
        
        // Ayni onbellek havuzunu paylasan CPU'ları bul
        let shared_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| cache_cpus.contains(&cpu))
            .copied()
            .collect();
        
        if !shared_cpus.is_empty() {
            return self.get_any_cpu(&shared_cpus);
        }
        
        // Eslesme yoksa herhangi bir CPU'ya don
        self.get_any_cpu(available_cpus)
    }
    
    /// Paket farkında CPU sec -- ayni fiziksel soketteki CPU'lar onceliklidir
    fn get_package_cpu(&self, available_cpus: &[u32]) -> Option<u32> {
        let package_id = self.get_package_id();
        if package_id == 0 {
            return self.get_any_cpu(available_cpus);
        }
        
        let package_cpus = get_package_cpus(self.get_last_cpu());
        
        // Ayni paketteki CPU'ları bul -- bu CPU'lar arasi L3 paylasimi ve QPI/UPI gecikmeleri daha dusuktur
        let same_package_cpus: Vec<u32> = available_cpus.iter()
            .filter(|&&cpu| package_cpus.contains(&cpu))
            .copied()
            .collect();
        
        if !same_package_cpus.is_empty() {
            return self.get_any_cpu(&same_package_cpus);
        }
        
        // Ayni pakette CPU yoksa herhangi birine don
        self.get_any_cpu(available_cpus)
    }
    
    /// Yuk dengelemeyi etkinlestirir/devre dısı bırakır;
    /// devre dısı bırakılirsa gorev hicbir zaman baska CPU'ya tasinmaz
    pub fn set_load_balance(&self, enabled: bool) {
        self.load_balance.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Yapıskan affiniteyi ayarlar -- true ise gorev mumkunse son calıstıgı CPU'ya donmeye calısır;
    /// bu, onbellek ısınmasını (cache warm-up) koruyarak performansı artırır
    pub fn set_sticky(&self, enabled: bool) {
        self.sticky.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Affinite istatistiklerini dondurur -- politika, maskeler, goc sayısı ve diger metrikler
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

/// Affinite istatistik kaydı -- bir gorevin affinite politikası ve calısma gecmisinin anlik goruntusunu tutar
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

/// CPU affinite yoneticisi -- sistemdeki tum gorevlerin affinite politikalarını merkezi olarak yonetir.
/// Yuk dengeleme tetiklemesi, gorev gecisi ve CPU yuk izleme bu yapiyla gerceklestirilir.
pub struct AffinityManager {
    /// Sistemdeki maksimum CPU sayısı -- basılatma sırasında topolojiden alinir
    max_cpus: u32,
    /// Gorev affinite tanımlayıcıları -- gorev kimligine gore indekslenir
    task_affinities: Vec<RcuPtr<TaskAffinity>>,
    /// CPU yuk izleme -- her CPU'nun mevcut gorev yogunlugunu tutar
    cpu_loads: Vec<AtomicU32>,
    /// Genel affinite politikası -- tum gorevlere uygulanabilecek varsayilan politika
    global_policy: AtomicU32, // AffinityPolicy u32 olarak atomik saklanır
    /// Yuk dengeleme etkin mi -- false ise hicbir gorev baska CPU'ya tasinmaz
    load_balance_enabled: AtomicBool,
    /// Gecis esligi -- bu yuzdeyi asan CPU'lardaki gorevler tasinır
    migration_threshold: AtomicU32,
    /// Istatistikler -- affinite olaylarının sayisal ozetini tutar
    stats: AffinityManagerStats,
}

/// Affinite yoneticisi istatistikleri -- toplu affinite aktivitesinin sayisal takibi
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
    /// Yeni affinite yoneticisi olusturur; baslangicta tum CPU'lar icin yuk sayaclari sıfırlanır
    pub fn new(max_cpus: u32) -> Self {
        let mut task_affinities = Vec::new();
        let mut cpu_loads = Vec::new();
        
        // Her CPU icin yuk sayıcıları basılat -- yuk dengeleme kararlarında kullanılır
        for _ in 0..max_cpus {
            cpu_loads.push(AtomicU32::new(0));
        }
        
        Self {
            max_cpus,
            task_affinities,
            cpu_loads,
            global_policy: AtomicU32::new(AffinityPolicy::Any as u32),
            load_balance_enabled: AtomicBool::new(true),
            migration_threshold: AtomicU32::new(80), // %80 yuk esligi -- bu degerin ustundeki CPU'lardan gorev tasinir
            stats: AffinityManagerStats::new(),
        }
    }
    
    /// Gorev affinitesi olusturur -- yeni bir gorev icin varsayilan affinite kaydı basılatır
    pub fn create_task_affinity(&mut self, task_id: u64) -> RcuPtr<TaskAffinity> {
        let affinity = Box::new(TaskAffinity::new(task_id));
        let affinity_ptr = RcuPtr::new(Box::into_raw(affinity));
        
        // Vektoru gorev kimligini karsilayacak buyuklukte genislet
        while self.task_affinities.len() <= task_id as usize {
            self.task_affinities.push(RcuPtr::new(core::ptr::null_mut()));
        }
        
        self.task_affinities[task_id as usize] = affinity_ptr.clone();
        self.stats.record_affinity();
        
        affinity_ptr
    }
    
    /// Gorev affinitesini dondurur -- gorev kimligine gore RCU korumalı affinite isaretcisini al
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
    
    /// Gorev affinitesini siler -- gorev sonlandirildıginda kaynagi serbest bırakmak icin cagir
    pub fn remove_task_affinity(&mut self, task_id: u64) {
        if (task_id as usize) < self.task_affinities.len() {
            self.task_affinities[task_id as usize] = RcuPtr::new(core::ptr::null_mut());
        }
    }
    
    /// Gorev icin en iyi CPU'yu bulur -- affinite politikası, CPU yukleri ve topoloji bilgisi kullanılır
    pub fn get_best_cpu_for_task(&self, task_id: u64) -> Option<u32> {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return None,
        };
        
        // Musait CPU'ları getir (actif ve asirı yuklu olmayan)
        let available_cpus = self.get_available_cpus();
        
        if available_cpus.is_empty() {
            return None;
        }
        
        let best_cpu = affinity.read().get_best_cpu(&available_cpus);
        
        if let Some(cpu) = best_cpu {
            // Son calısılan CPU kaydını guncelle
            affinity.read().set_last_cpu(cpu);
            
            // CPU yukunu arttır -- bu gorev artık bu CPU'ya atandı
            self.update_cpu_load(cpu, 1);
        }
        
        best_cpu
    }
    
    /// Musait CPU'ları dondurur -- acik ve asirı yuklu olmayan CPU'lar secim havuzuna girer
    fn get_available_cpus(&self) -> Vec<u32> {
        let mut available = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            // CPU'nun acik olup olmadıgını kontrol et
            if !self.is_cpu_online(cpu_id) {
                continue;
            }
            
            // Yuk dengeleme aktifse CPU'nun asirı yuklu olup olmadıgını kontrol et
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
    
    /// CPU'nun acik (online) olup olmadıgını kontrol eder.
    /// Gercek uygulamada hotplug yoneticisiyle entegre edilir.
    fn is_cpu_online(&self, cpu_id: u32) -> bool {
        // Hotplug yoneticisiyle denetlenmeli; simdilik tum CPU'lar acik kabul edilir
        cpu_id < self.max_cpus
    }
    
    /// CPU yukunu gunceller -- pozitif delta ekleme, negatif delta cıkarma anlamına gelir
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
    
    /// CPU'lar arası yuku dengeler -- asirı yuklu CPU'lardan az yuklu olanlara gorev tasinir.
    /// Linux'taki load_balance() fonksiyonuna benzer sekilde calısır.
    pub fn balance_load(&self) {
        if !self.load_balance_enabled.load(Ordering::Acquire) {
            return;
        }
        
        let threshold = self.migration_threshold.load(Ordering::Acquire);
        let mut overloaded_cpus = Vec::new();
        let mut underloaded_cpus = Vec::new();
        
        // Asirı yuklu ve az yuklu CPU'ları bul -- tasinım kararı buradan verilir
        for cpu_id in 0..self.max_cpus {
            let load = self.cpu_loads[cpu_id as usize].load(Ordering::Acquire);
            
            if load > threshold {
                overloaded_cpus.push((cpu_id, load));
            } else if load < threshold / 2 {
                underloaded_cpus.push(cpu_id);
            }
        }
        
        // Asirı yuklu CPU'lardan az yuklu olanlara gorevleri tasin
        for &(overloaded_cpu, _) in &overloaded_cpus {
            if underloaded_cpus.is_empty() {
                break;
            }
            
            // Tasinabilecek gorevleri bul -- affinite izin veriyorsa gecis gerceklestirilir
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
    
    /// Belirli bir CPU'dan tasinabilecek gorevleri bulur.
    /// Gercek uygulamada o CPU'da calısan gorevler zamanlayicidan sorgulanır.
    fn find_migratable_tasks(&self, cpu_id: u32) -> Vec<u64> {
        let mut migratable = Vec::new();
        
        // O CPU'da calısan gorevler zamanlayicidan alinmali; su an bos liste dondur
        migratable
    }
    
    /// Gorevi farklı bir CPU'ya tasinır -- affinite politikası uygunsa gorev gecisi gerceklestirilir.
    /// task_migration_notify() cagrılarıyla benzer semantige sahiptir.
    fn migrate_task(&self, task_id: u64, target_cpu: u32) -> bool {
        let affinity = match self.get_task_affinity(task_id) {
            Some(affinity) => affinity,
            None => return false,
        };
        
        // Gorevin hedef CPU'da calısıp calısamayacagını kontrol et
        if !affinity.read().is_cpu_allowed(target_cpu) {
            return false;
        }
        
        // CPU yuklerini guncelle -- kaynak CPU'dan cıkar, hedef CPU'ya ekle
        let current_cpu = affinity.read().get_last_cpu();
        self.update_cpu_load(current_cpu, -1);
        self.update_cpu_load(target_cpu, 1);
        
        // Affinite kaydini guncelle -- gorev bir sonraki zamanlama turunda yeni CPU'ya atanir
        affinity.read().set_last_cpu(target_cpu);
        
        // Goc olayini kaydet -- istatistik ve hata ayıklama icin
        affinity.read().migrations.fetch_add(1, Ordering::Relaxed);
        self.stats.record_migration();
        
        crate::serial_println!("Affinity: Migrated task {} from CPU {} to CPU {}", 
            task_id, current_cpu, target_cpu);
        
        true
    }
    
    /// Genel affinite politikasını ayarlar -- tum yeni gorevler bu politikayla baslatilabilir
    pub fn set_global_policy(&self, policy: AffinityPolicy) {
        self.global_policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Genel affinite politikasını dondurur
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
    
    /// Yuk dengelemeyi etkinlestirir veya devre dısı bırakır;
    /// devre dısında hicbir gorev otomatik olarak tasinmaz
    pub fn set_load_balance_enabled(&self, enabled: bool) {
        self.load_balance_enabled.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Gecis esligini ayarlar -- bu yuzdeyi asan yuk, gorev tasinimini tetikler
    pub fn set_migration_threshold(&self, threshold: u32) {
        self.migration_threshold.store(threshold, Ordering::Release);
        smp_wmb();
    }
    
    /// Belirtilen CPU'nun mevcut yukunu dondurur
    pub fn get_cpu_load(&self, cpu_id: u32) -> Option<u32> {
        if cpu_id as usize >= self.cpu_loads.len() {
            return None;
        }
        
        Some(self.cpu_loads[cpu_id as usize].load(Ordering::Acquire))
    }
    
    /// Tum CPU'lar icin (cpu_id, yuk) cifti listesini dondurur
    pub fn get_all_cpu_loads(&self) -> Vec<(u32, u32)> {
        let mut loads = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            if let Some(load) = self.get_cpu_load(cpu_id) {
                loads.push((cpu_id, load));
            }
        }
        
        loads
    }
    
    /// Yonetici istatistiklerini dondurur: (toplam_affinite, toplam_goc, yuk_dengeleme, affinite_degisim)
    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        self.stats.get_stats()
    }
}

/// Global affinite yonetici ornegi -- tum gorevlerin affinite bilgisi buradan erisilen tek ornekle yonetilir
static mut AFFINITY_MANAGER: Option<AffinityManager> = None;
static AFFINITY_INIT: AtomicBool = AtomicBool::new(false);

/// CPU affinite alt sistemini baslatir -- maksimum CPU sayısı icin AffinityManager ornegi olusturulur.
/// Bu fonksiyon topoloji baslatma (topology::init) sonrasında cagrilmalidir.
pub fn init(max_cpus: u32) {
    if AFFINITY_INIT.load(Ordering::Acquire) {
        return;
    }
    
    crate::serial_println!("Affinity: Initializing CPU affinity for {} CPUs", max_cpus);
    
    let manager = AffinityManager::new(max_cpus);
    
    unsafe {
        AFFINITY_MANAGER = Some(manager);
    }
    
    AFFINITY_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("Affinity: CPU affinity initialized");
}

/// Global affinite yoneticisine erisim saglar; baslatılmamissa None dondurur
pub fn get_manager() -> Option<&'static AffinityManager> {
    if !AFFINITY_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { AFFINITY_MANAGER.as_ref() }
}

/// Kolaylik fonksiyonları -- modul duzeyi API'si, calisan koda yoneticiye dogrudan erisim gerektirmez
pub fn create_task_affinity(task_id: u64) -> Option<RcuPtr<TaskAffinity>> {
    if let Some(manager) = get_manager() {
        // Gercek uygulamada degistirilebilir erisim gerekir; su an None dondurulur
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
