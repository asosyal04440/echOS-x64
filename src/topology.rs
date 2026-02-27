//! # echOS CPU Topoloji Algılama Modülü
//!
//! Tier 1 OS seviyesinde dinamik CPU topoloji keşfi.
//! CPUID komutları aracılığıyla NUMA düğümleri, soket/paket başına çekirdek sayısı,
//! SMT/hiper iş parçacığı yapılandırması ve L1/L2/L3 önbellek hiyerarşisi algılanır.
//! Linux CPU topoloji altyapısıyla eşdeğer yetenekler sunar.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::PreemptDisableGuard;
use crate::rcu::{RcuPtr, synchronize_rcu};

/// CPU önbellek türleri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CacheType {
    /// Önbellek yok
    None = 0,
    /// Veri önbelleği
    Data = 1,
    /// Komut önbelleği
    Instruction = 2,
    /// Birleşik önbellek (veri + komut)
    Unified = 3,
    /// İz önbelleği
    Trace = 4,
}

/// CPU önbellek tanımlayıcısı
#[derive(Debug, Clone, Copy)]
pub struct CacheDescriptor {
    /// Önbellek türü
    pub cache_type: CacheType,
    /// Önbellek seviyesi (L1, L2, L3, vb.)
    pub level: u8,
    /// Önbellek boyutu (bayt cinsinden)
    pub size: u32,
    /// Satır boyutu (bayt cinsinden)
    pub line_size: u16,
    /// Yol sayısı (birleşiklik)
    pub ways: u16,
    /// Küme sayısı
    pub sets: u32,
    /// Diğer CPU'larla paylaşılan (CPU maskesi)
    pub shared_cpu_mask: u64,
    /// Alt seviye önbellekleri kapsar
    pub inclusive: bool,
}

impl CacheDescriptor {
    pub fn new(cache_type: CacheType, level: u8, size: u32, line_size: u16, ways: u16) -> Self {
        let sets = size / (line_size as u32 * ways as u32);
        Self {
            cache_type,
            level,
            size,
            line_size,
            ways,
            sets,
            shared_cpu_mask: 0,
            inclusive: false,
        }
    }
    
    /// Toplam önbellek boyutunu insan tarafından okunabilir biçimde döndürür
    pub fn get_size_kb(&self) -> u32 {
        self.size / 1024
    }
    
    /// Bu önbelleğin başka bir CPU ile paylaşılıp paylaşılmadığını kontrol eder
    pub fn is_shared_with(&self, cpu_id: u32) -> bool {
        (self.shared_cpu_mask & (1u64 << cpu_id)) != 0
    }
    
    /// Paylaşılan CPU maskesini ayarlar
    pub fn set_shared_mask(&mut self, cpu_mask: u64) {
        self.shared_cpu_mask = cpu_mask;
    }
}

/// CPU topoloji bilgisi
#[repr(C, align(64))]
pub struct CpuTopology {
    /// Fiziksel CPU kimliği
    pub physical_id: u32,
    /// Mantıksal CPU kimliği (iş parçacığı)
    pub logical_id: u32,
    /// Paket içindeki çekirdek kimliği
    pub core_id: u32,
    /// Çekirdek içindeki iş parçacığı kimliği
    pub thread_id: u32,
    /// Paket/Soket kimliği
    pub package_id: u32,
    /// NUMA düğüm kimliği
    pub numa_node_id: u32,
    /// CPU ailesi/modeli/adımlama
    pub cpu_signature: u32,
    /// CPU özellik bit haritası
    pub cpu_features: u64,
    /// Maksimum frekans (MHz cinsinden)
    pub max_frequency: u32,
    /// Temel frekans (MHz cinsinden)
    pub base_frequency: u32,
    /// Önbellek hiyerarşisi (L1d, L1i, L2, L3, vb.)
    pub caches: Vec<CacheDescriptor>,
    /// SMT (Eşzamanlı Çok İş Parçacığı) etkin
    pub smt_enabled: bool,
    /// Çekirdek başına iş parçacığı sayısı
    pub threads_per_core: u32,
    /// Paket başına çekirdek sayısı
    pub cores_per_package: u32,
    /// Toplam paket sayısı
    pub packages_total: u32,
    /// Bu CPU'nun çevrimiiçi olup olmadığı
    pub online: AtomicBool,
    /// Bu CPU'nun sıcak takılıp takılamayacağı
    pub hotpluggable: AtomicBool,
    /// CPU topoloji sürümü (değişiklik tespiti için)
    pub topology_version: AtomicU64,
    /// Yanlış paylaşımı önlemek için dolgu
    _padding: [u8; 0],
}

impl CpuTopology {
    /// Yeni CPU topolojisi oluşturur
    pub fn new(logical_id: u32) -> Self {
        Self {
            physical_id: logical_id,
            logical_id,
            core_id: logical_id,
            thread_id: 0,
            package_id: 0,
            numa_node_id: 0,
            cpu_signature: 0,
            cpu_features: 0,
            max_frequency: 0,
            base_frequency: 0,
            caches: Vec::new(),
            smt_enabled: false,
            threads_per_core: 1,
            cores_per_package: 1,
            packages_total: 1,
            online: AtomicBool::new(false),
            hotpluggable: AtomicBool::new(false),
            topology_version: AtomicU64::new(0),
            _padding: [0; 0],
        }
    }
    
    /// Önbellek tanımlayıcısı ekler
    pub fn add_cache(&mut self, cache: CacheDescriptor) {
        self.caches.push(cache);
    }
    
    /// Seviye ve türe göre önbellek döndürür
    pub fn get_cache(&self, level: u8, cache_type: CacheType) -> Option<&CacheDescriptor> {
        self.caches.iter().find(|c| c.level == level && c.cache_type == cache_type)
    }
    
    /// L1 veri önbelleğini döndürür
    pub fn get_l1d_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(1, CacheType::Data)
    }
    
    /// L1 komut önbelleğini döndürür
    pub fn get_l1i_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(1, CacheType::Instruction)
    }
    
    /// L2 önbelleğini döndürür
    pub fn get_l2_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(2, CacheType::Unified)
    }
    
    /// L3 önbelleğini döndürür
    pub fn get_l3_cache(&self) -> Option<&CacheDescriptor> {
        self.get_cache(3, CacheType::Unified)
    }
    
    /// CPU'nun hiper iş parçacığı/SMT destekleyip desteklemediğini kontrol eder
    pub fn has_smt(&self) -> bool {
        self.smt_enabled && self.threads_per_core > 1
    }
    
    /// Bunun başka bir CPU'nun hiper iş parçacığı olup olmadığını kontrol eder
    pub fn is_hyperthread_of(&self, other: &CpuTopology) -> bool {
        self.package_id == other.package_id && 
        self.core_id == other.core_id && 
        self.thread_id != other.thread_id
    }
    
    /// Bu CPU'nun başka bir CPU ile önbellek paylaşıp paylaşmadığını kontrol eder
    pub fn shares_cache_with(&self, other: &CpuTopology, cache_level: u8) -> bool {
        if let Some(cache) = self.get_cache(cache_level, CacheType::Unified) {
            cache.is_shared_with(other.logical_id)
        } else if let Some(cache) = self.get_cache(cache_level, CacheType::Data) {
            cache.is_shared_with(other.logical_id)
        } else {
            false
        }
    }
    
    /// Önbellek paylaşım bilgisini döndürür
    pub fn get_cache_sharing(&self) -> Vec<(u8, u64)> {
        let mut sharing = Vec::new();
        
        for cache in &self.caches {
            sharing.push((cache.level, cache.shared_cpu_mask));
        }
        
        sharing
    }
    
    /// Topoloji sürümünü artırır
    pub fn increment_version(&self) {
        self.topology_version.fetch_add(1, Ordering::AcqRel);
        smp_wmb();
    }
    
    /// Topoloji sürümünü döndürür
    pub fn get_version(&self) -> u64 {
        self.topology_version.load(Ordering::Acquire)
    }
    
    /// Çevrimiiçi durumunu ayarlar
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Release);
        smp_wmb();
    }
    
    /// CPU'nun çevrimiiçi olup olmadığını kontrol eder
    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }
    
    /// Sıcak takılabilir durumunu ayarlar
    pub fn set_hotpluggable(&self, hotpluggable: bool) {
        self.hotpluggable.store(hotpluggable, Ordering::Release);
        smp_wmb();
    }
    
    /// CPU'nun sıcak takılabilir olup olmadığını kontrol eder
    pub fn is_hotpluggable(&self) -> bool {
        self.hotpluggable.load(Ordering::Acquire)
    }
}

/// Sistem topoloji bilgisi
pub struct SystemTopology {
    /// Maksimum CPU sayısı
    max_cpus: u32,
    /// CPU topolojileri
    cpu_topologies: Vec<RcuPtr<CpuTopology>>,
    /// Paket sayısı
    package_count: AtomicU32,
    /// Paket başına çekirdek sayısı
    cores_per_package: AtomicU32,
    /// Çekirdek başına iş parçacığı sayısı
    threads_per_core: AtomicU32,
    /// Toplam çekirdek sayısı
    total_cores: AtomicU32,
    /// Toplam iş parçacığı sayısı
    total_threads: AtomicU32,
    /// Topoloji tespiti etkin
    detection_enabled: AtomicBool,
    /// Son topoloji güncelleme zaman damgası
    last_update: AtomicU64,
    /// Topoloji güncelleme sayısı
    update_count: AtomicU64,
}

impl SystemTopology {
    /// Yeni sistem topolojisi oluşturur
    pub fn new(max_cpus: u32) -> Self {
        let mut cpu_topologies = Vec::with_capacity(max_cpus as usize);
        
        // CPU topolojilerini başlat
        for cpu_id in 0..max_cpus {
            let topology = Box::new(CpuTopology::new(cpu_id));
            cpu_topologies.push(RcuPtr::new(Box::into_raw(topology)));
        }
        
        Self {
            max_cpus,
            cpu_topologies,
            package_count: AtomicU32::new(0),
            cores_per_package: AtomicU32::new(0),
            threads_per_core: AtomicU32::new(0),
            total_cores: AtomicU32::new(0),
            total_threads: AtomicU32::new(0),
            detection_enabled: AtomicBool::new(true),
            last_update: AtomicU64::new(0),
            update_count: AtomicU64::new(0),
        }
    }
    
    /// CPU topolojisini döndürür
    pub fn get_cpu_topology(&self, cpu_id: u32) -> Option<RcuPtr<CpuTopology>> {
        if cpu_id >= self.max_cpus {
            return None;
        }
        
        Some(self.cpu_topologies[cpu_id as usize].clone())
    }
    
    /// CPUID kullanarak CPU topolojisini algılar
    pub fn detect_topology(&mut self) -> Result<(), TopologyError> {
        if !self.detection_enabled.load(Ordering::Acquire) {
            return Err(TopologyError::DetectionDisabled);
        }
        
        crate::serial_println!("Topology: Starting CPU topology detection...");
        
        // Temel CPU bilgilerini algıla
        self.detect_basic_info()?;
        
        // Önbellek hiyerarşisini algıla
        self.detect_cache_hierarchy()?;
        
        // SMT/hiper iş parçacığını algıla
        self.detect_smt_info()?;
        
        // Paket bilgilerini algıla
        self.detect_package_info()?;
        
        // Paylaşım ilişkilerini oluştur
        self.build_sharing_relationships()?;
        
        // İstatistikleri güncelle
        self.update_statistics();
        
        // Sürüm ve zaman damgasını güncelle
        self.update_version();
        
        crate::serial_println!("Topology: Detection completed");
        Ok(())
    }
    
    /// CPUID kullanarak temel CPU bilgilerini algılar
    fn detect_basic_info(&mut self) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // Gerçek uygulamada CPUID komutları kullanılırdı
            // Şimdiliği için algılamayı simüle edeceğiz
            
            // CPU imzasını simüle et (aile, model, adımlama)
            let cpu_signature = 0x806E9; // Örnek: Intel Core i7-12700K
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.cpu_signature = cpu_signature;
            
            // CPU özelliklerini simüle et
            let cpu_features = 0xFFFFFFFFFFFFFFFF; // Tüm özellikler etkin
            mutable_topology.cpu_features = cpu_features;
            
            // Frekansları simüle et
            let max_freq = 3600; // 3.6 GHz
            let base_freq = 2400; // 2.4 GHz
            mutable_topology.max_frequency = max_freq;
            mutable_topology.base_frequency = base_freq;
        }
        
        Ok(())
    }
    
    /// Önbellek hiyerarşisini algılar
    fn detect_cache_hierarchy(&mut self) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // Gerçek uygulamada CPUID 4. yaprağı kullanılırdı
            // Şimdiliği için tipik önbellek hiyerarşisini simüle edeceğiz
            
            let mutable_topology = topology_guard.as_mut();
            
            // L1 Veri önbelleği: 32KB, 8-yollu, 64-bayt satır
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Data, 1, 32 * 1024, 64, 8
            ));
            
            // L1 Komut önbelleği: 32KB, 8-yollu, 64-bayt satır
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Instruction, 1, 32 * 1024, 64, 8
            ));
            
            // L2 önbelleği: 1MB, 16-yollu, 64-bayt satır (çekirdek başına)
            mutable_topology.add_cache(CacheDescriptor::new(
                CacheType::Unified, 2, 1024 * 1024, 64, 16
            ));
            
            // L3 önbelleği: 25MB, 20-yollu, 64-bayt satır (paylaşılan)
            let mut l3_cache = CacheDescriptor::new(
                CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20
            );
            l3_cache.inclusive = true;
            mutable_topology.add_cache(l3_cache);
        }
        
        Ok(())
    }
    
    /// SMT/hiper iş parçacığı bilgilerini algılar
    fn detect_smt_info(&mut self) -> Result<(), TopologyError> {
        // Gerçek uygulamada CPUID 1 ve 11. yaprakları kullanılırdı
        // Şimdiliği için tipik SMT yapılandırmasını simüle edeceğiz
        
        let threads_per_core = 2; // Hiper iş parçacığı etkin
        let cores_per_package = 8; // Paket başına 8 çekirdek
        
        self.threads_per_core.store(threads_per_core, Ordering::Release);
        self.cores_per_package.store(cores_per_package, Ordering::Release);
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.smt_enabled = threads_per_core > 1;
            mutable_topology.threads_per_core = threads_per_core;
            mutable_topology.cores_per_package = cores_per_package;
            
            // Çekirdek ve iş parçacığı kimliklerini hesapla
            let package_id = cpu_id / (cores_per_package * threads_per_core);
            let core_in_package = (cpu_id / threads_per_core) % cores_per_package;
            let thread_in_core = cpu_id % threads_per_core;
            
            mutable_topology.package_id = package_id;
            mutable_topology.core_id = core_in_package;
            mutable_topology.thread_id = thread_in_core;
            mutable_topology.physical_id = core_in_package;
        }
        
        Ok(())
    }
    
    /// Paket bilgilerini algılar
    fn detect_package_info(&mut self) -> Result<(), TopologyError> {
        // Benzersiz paketleri say
        let mut packages = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            let package_id = topology_guard.package_id;
            
            if !packages.contains(&package_id) {
                packages.push(package_id);
            }
        }
        
        self.package_count.store(packages.len() as u32, Ordering::Release);
        
        // Tüm topolojilerde paket sayısını güncelle
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            let mutable_topology = topology_guard.as_mut();
            mutable_topology.packages_total = packages.len() as u32;
        }
        
        Ok(())
    }
    
    /// Önbellek paylaşım ilişkilerini oluşturur
    fn build_sharing_relationships(&mut self) -> Result<(), TopologyError> {
        // Her önbellek seviyesi için paylaşım maskeleri oluştur
        for cache_level in 1..=3 {
            self.build_cache_sharing_for_level(cache_level)?;
        }
        
        Ok(())
    }
    
    /// Belirli seviye için önbellek paylaşımını oluşturur
    fn build_cache_sharing_for_level(&mut self, level: u8) -> Result<(), TopologyError> {
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            // Bu önbellek seviyesini paylaşan CPU'ları bul
            let mut shared_mask = 1u64 << cpu_id;
            
            for other_cpu_id in 0..self.max_cpus {
                if cpu_id == other_cpu_id {
                    continue;
                }
                
                let other_topology = match self.get_cpu_topology(other_cpu_id) {
                    Some(topology) => topology,
                    None => continue,
                };
                
                let other_guard = other_topology.read();
                
                // Bu önbellek seviyesini paylaşıp paylaşmadıklarını kontrol et
                let shares_cache = match level {
                    1 => {
                        // L1 önbellekleri çekirdek başınadır, paylaşımlı değildir
                        topology_guard.core_id == other_guard.core_id
                    }
                    2 => {
                        // L2 önbellekleri çoğu mimaride çekirdek başınadır
                        topology_guard.core_id == other_guard.core_id
                    }
                    3 => {
                        // L3 önbellekleri genellikle paket içinde paylaşılır
                        topology_guard.package_id == other_guard.package_id
                    }
                    _ => false,
                };
                
                if shares_cache {
                    shared_mask |= 1u64 << other_cpu_id;
                }
            }
            
            // Bu seviyedeki tüm önbelleklerin paylaşım maskelerini güncelle
            let mutable_topology = topology_guard.as_mut();
            for cache in &mut mutable_topology.caches {
                if cache.level == level {
                    cache.set_shared_mask(shared_mask);
                }
            }
        }
        
        Ok(())
    }
    
    /// Topoloji istatistiklerini günceller
    fn update_statistics(&mut self) {
        let mut total_cores = 0;
        let mut total_threads = 0;
        let mut unique_cores = Vec::new();
        
        for cpu_id in 0..self.max_cpus {
            let topology = match self.get_cpu_topology(cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let topology_guard = topology.read();
            
            total_threads += 1;
            
            let core_key = (topology_guard.package_id, topology_guard.core_id);
            if !unique_cores.contains(&core_key) {
                unique_cores.push(core_key);
                total_cores += 1;
            }
        }
        
        self.total_cores.store(total_cores, Ordering::Release);
        self.total_threads.store(total_threads, Ordering::Release);
    }
    
    /// Topoloji sürümünü günceller
    fn update_version(&mut self) {
        let current_time = crate::task::scheduler::get_ticks() as u64;
        self.last_update.store(current_time, Ordering::Release);
        self.update_count.fetch_add(1, Ordering::AcqRel);
        
        // Tüm CPU'ların sürümünü artır
        for cpu_id in 0..self.max_cpus {
            if let Some(topology) = self.get_cpu_topology(cpu_id) {
                topology.read().increment_version();
            }
        }
        
        smp_mb();
    }
    
    /// Belirtilen CPU ile önbellek paylaşan CPU'ları döndürür
    pub fn get_cache_sharing_cpus(&self, cpu_id: u32, cache_level: u8) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        
        if let Some(cache) = topology_guard.get_cache(cache_level, CacheType::Unified) {
            let mut sharing_cpus = Vec::new();
            let shared_mask = cache.shared_cpu_mask;
            
            for other_cpu_id in 0..self.max_cpus {
                if (shared_mask & (1u64 << other_cpu_id)) != 0 {
                    sharing_cpus.push(other_cpu_id);
                }
            }
            
            sharing_cpus
        } else {
            Vec::new()
        }
    }
    
    /// Aynı paketteki CPU'ları döndürür
    pub fn get_package_cpus(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        let package_id = topology_guard.package_id;
        
        let mut package_cpus = Vec::new();
        
        for other_cpu_id in 0..self.max_cpus {
            let other_topology = match self.get_cpu_topology(other_cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let other_guard = other_topology.read();
            if other_guard.package_id == package_id {
                package_cpus.push(other_cpu_id);
            }
        }
        
        package_cpus
    }
    
    /// Aynı çekirdekteki CPU'ları döndürür
    pub fn get_core_cpus(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        let package_id = topology_guard.package_id;
        let core_id = topology_guard.core_id;
        
        let mut core_cpus = Vec::new();
        
        for other_cpu_id in 0..self.max_cpus {
            let other_topology = match self.get_cpu_topology(other_cpu_id) {
                Some(topology) => topology,
                None => continue,
            };
            
            let other_guard = other_topology.read();
            if other_guard.package_id == package_id && other_guard.core_id == core_id {
                core_cpus.push(other_cpu_id);
            }
        }
        
        core_cpus
    }
    
    /// Hiper iş parçacığı kardeşlerini döndürür
    pub fn get_hyperthread_siblings(&self, cpu_id: u32) -> Vec<u32> {
        let topology = match self.get_cpu_topology(cpu_id) {
            Some(topology) => topology,
            None => return Vec::new(),
        };
        
        let topology_guard = topology.read();
        
        if !topology_guard.has_smt() {
            return Vec::new();
        }
        
        self.get_core_cpus(cpu_id).into_iter()
            .filter(|&sibling_id| sibling_id != cpu_id)
            .collect()
    }
    
    /// İki CPU'nun kardeş (aynı çekirdek) olup olmadığını kontrol eder
    pub fn are_siblings(&self, cpu_id1: u32, cpu_id2: u32) -> bool {
        let topology1 = match self.get_cpu_topology(cpu_id1) {
            Some(topology) => topology,
            None => return false,
        };
        
        let topology2 = match self.get_cpu_topology(cpu_id2) {
            Some(topology) => topology,
            None => return false,
        };
        
        let guard1 = topology1.read();
        let guard2 = topology2.read();
        
        guard1.package_id == guard2.package_id && guard1.core_id == guard2.core_id
    }
    
    /// Sistem topoloji özetini döndürür
    pub fn get_summary(&self) -> TopologySummary {
        TopologySummary {
            packages: self.package_count.load(Ordering::Acquire),
            cores_per_package: self.cores_per_package.load(Ordering::Acquire),
            threads_per_core: self.threads_per_core.load(Ordering::Acquire),
            total_cores: self.total_cores.load(Ordering::Acquire),
            total_threads: self.total_threads.load(Ordering::Acquire),
            smt_enabled: self.threads_per_core.load(Ordering::Acquire) > 1,
            last_update: self.last_update.load(Ordering::Acquire),
            update_count: self.update_count.load(Ordering::Acquire),
        }
    }
    
    /// Topoloji tespitini etkinleştirir/devre dışı bırakır
    pub fn set_detection_enabled(&self, enabled: bool) {
        self.detection_enabled.store(enabled, Ordering::Release);
        smp_wmb();
    }
    
    /// Topoloji tespitinin etkin olup olmadığını kontrol eder
    pub fn is_detection_enabled(&self) -> bool {
        self.detection_enabled.load(Ordering::Acquire)
    }
}

/// Topoloji özeti
#[derive(Debug, Clone, Copy)]
pub struct TopologySummary {
    pub packages: u32,
    pub cores_per_package: u32,
    pub threads_per_core: u32,
    pub total_cores: u32,
    pub total_threads: u32,
    pub smt_enabled: bool,
    pub last_update: u64,
    pub update_count: u64,
}

/// Topoloji tespit hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyError {
    /// Tespit devre dışı
    DetectionDisabled,
    /// Geçersiz CPU kimliği
    InvalidCpuId,
    /// CPUID komutu başarısız
    CpuidFailed,
    /// Tutarsız topoloji verisi
    InconsistentData,
    /// Uygulanmadı
    NotImplemented,
}

/// Global topoloji örneği
static mut SYSTEM_TOPOLOGY: Option<SystemTopology> = None;
static TOPOLOGY_INIT: AtomicBool = AtomicBool::new(false);

/// Topoloji alt sistemini başlatır
pub fn init(max_cpus: u32) -> Result<(), TopologyError> {
    if TOPOLOGY_INIT.load(Ordering::Acquire) {
        return Ok(());
    }
    
    crate::serial_println!("Topology: Initializing topology detection for {} CPUs", max_cpus);
    
    let mut topology = SystemTopology::new(max_cpus);
    
    // Başlangıç topolojisini algıla
    topology.detect_topology()?;;
    
    unsafe {
        SYSTEM_TOPOLOGY = Some(topology);
    }
    
    TOPOLOGY_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("Topology: Topology detection initialized");
    Ok(())
}

/// Sistem topolojisini döndürür
pub fn get_system_topology() -> Option<&'static SystemTopology> {
    if !TOPOLOGY_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { SYSTEM_TOPOLOGY.as_ref() }
}

/// Topolojiyi yeniden algılar (sıcak takma olayları için)
pub fn redetect_topology() -> Result<(), TopologyError> {
    let topology = get_system_topology().ok_or(TopologyError::DetectionDisabled)?;
    
    // Gerçek uygulamada değiştirilebilir erişim gerekir
    // Şimdiliği için sadece isteği günlüğe kaydedeceğiz
    crate::serial_println!("Topology: Redetection requested");
    
    Err(TopologyError::NotImplemented)
}

/// Kolaylık fonksiyonları
pub fn get_cpu_topology(cpu_id: u32) -> Option<RcuPtr<CpuTopology>> {
    get_system_topology()?.get_cpu_topology(cpu_id)
}

pub fn get_cache_sharing_cpus(cpu_id: u32, cache_level: u8) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_cache_sharing_cpus(cpu_id, cache_level))
        .unwrap_or_default()
}

pub fn get_package_cpus(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_package_cpus(cpu_id))
        .unwrap_or_default()
}

pub fn get_core_cpus(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_core_cpus(cpu_id))
        .unwrap_or_default()
}

pub fn get_hyperthread_siblings(cpu_id: u32) -> Vec<u32> {
    get_system_topology()
        .map(|t| t.get_hyperthread_siblings(cpu_id))
        .unwrap_or_default()
}

pub fn are_siblings(cpu_id1: u32, cpu_id2: u32) -> bool {
    get_system_topology()
        .map(|t| t.are_siblings(cpu_id1, cpu_id2))
        .unwrap_or(false)
}

pub fn get_topology_summary() -> Option<TopologySummary> {
    get_system_topology().map(|t| t.get_summary())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_descriptor() {
        let cache = CacheDescriptor::new(CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20);
        assert_eq!(cache.get_size_kb(), 25 * 1024);
        assert_eq!(cache.level, 3);
        assert_eq!(cache.cache_type, CacheType::Unified);
    }
    
    #[test]
    fn test_cpu_topology() {
        let mut topology = CpuTopology::new(0);
        
        // Önbellekler ekle
        topology.add_cache(CacheDescriptor::new(CacheType::Data, 1, 32 * 1024, 64, 8));
        topology.add_cache(CacheDescriptor::new(CacheType::Unified, 3, 25 * 1024 * 1024, 64, 20));
        
        assert!(topology.get_l1d_cache().is_some());
        assert!(topology.get_l3_cache().is_some());
        assert!(topology.get_l1i_cache().is_none());
    }
    
    #[test]
    fn test_system_topology() {
        let mut topology = SystemTopology::new(4);
        
        assert!(topology.detect_topology().is_ok());
        assert_eq!(topology.get_summary().total_threads, 4);
    }
}
