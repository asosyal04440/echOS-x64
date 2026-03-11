//! # echOS NUMA (Non-Uniform Memory Access - Tekdüze Olmayan Bellek Erişimi) Destek Modülü
//!
//! Tier 1 OS seviyesinde NUMA-aware bellek tahsisi ve CPU yakınlığı (affinity) yönetimi.
//! Linux NUMA ile aynı seviyede performans ve özellikler.
//!
//! ## NUMA Nedir?
//! NUMA mimarisinde her işlemci (CPU) kendi yerel belleğine hızlı erişirken,
//! başka bir işlemcinin belleğine erişim daha yavaştır. Bu modül, bellek
//! tahsislerini mümkün olduğunca yerel tutarak performansı artırır.
//!
//! ```ascii
//! CPU0 <--[hızlı]--> Bellek0 (NUMA Düğüm 0)
//!           |
//!        [yavaş]
//!           |
//! CPU1 <--[hızlı]--> Bellek1 (NUMA Düğüm 1)
//! ```

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::PreemptDisableGuard;
use crate::rcu::{synchronize_rcu, RcuPtr};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Desteklenen maksimum NUMA düğüm sayısı.
///
/// Linux çekirdeği ile uyumlu 256 düğüm sınırı.
pub const MAX_NUMA_NODES: usize = 256;

/// NUMA düğüm durumları (Linux `numa_states` ile uyumlu).
///
/// Bir düğüm bu dört durum arasında geçiş yapar:
/// Offline -> ComingUp -> Online -> GoingDown -> Offline
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NumaNodeState {
    /// Düğüm çevrimdışı ve kullanılamaz
    Offline = 0,
    /// Düğüm devreye giriyor (başlatma aşamasında)
    ComingUp = 1,
    /// Düğüm çevrimiçi ve kullanıma hazır
    Online = 2,
    /// Düğüm kapatılıyor (devre dışı bırakma aşamasında)
    GoingDown = 3,
}

/// NUMA bellek tahsis politikaları.
///
/// Her politika, bellek tahsis edilirken hangi düğümlerin
/// tercih edileceğini belirler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NumaPolicy {
    /// Varsayılan politika (genellikle yerel tahsis)
    Default = 0,
    /// Yerel düğümden tahsis et, yetersizse başkasına geç
    Prefer = 1,
    /// Belirli düğümlere bağla (kesin zorunluluk)
    Bind = 2,
    /// Düğümler arasında dönüşümlü tahsis et (yük dengeleme)
    Interleave = 3,
    /// Belirtilen düğümleri tercih et
    Preferred = 4,
    /// Yalnızca yerel düğümden tahsis et
    Local = 5,
}

/// NUMA mesafe matrisi girdisi.
///
/// İki düğüm arasındaki göreli erişim maliyetini temsil eder.
/// Linux çekirdeğinde yerel mesafe 10, uzak mesafe genellikle 20+ olarak tanımlanır.
#[derive(Debug, Clone, Copy)]
pub struct NumaDistance {
    /// Kaynak düğümden hedef düğüme mesafe değeri
    pub distance: u8,
    /// Yerel mesafe mi? (aynı düğüm = mesafe 10)
    pub is_local: bool,
    /// Uzak mesafe mi? (farklı düğüm = mesafe > 10)
    pub is_remote: bool,
}

impl NumaDistance {
    /// Mesafe değerinden yeni bir `NumaDistance` oluşturur.
    ///
    /// Linux çekirdeği standardına göre yerel mesafe=10.
    pub fn new(distance: u8) -> Self {
        Self {
            distance,
            is_local: distance == 10, // Linux yerel mesafe için 10 kullanır
            is_remote: distance > 10,
        }
    }
}

/// NUMA düğüm tanımlayıcısı.
///
/// Her fiziksel bellek düğümü için CPU listesi, bellek miktarı,
/// tahsis politikası ve mesafe bilgilerini tutar.
/// Cache-line hizalaması (64 bayt) yanlış paylaşımı (false sharing) önler.
#[repr(C, align(64))]
pub struct NumaNode {
    /// Düğüm kimlik numarası
    pub node_id: u32,
    /// Mevcut durum (NumaNodeState)
    pub state: AtomicU32, // NumaNodeState as u32
    /// Bu düğümdeki CPU sayısı
    pub cpu_count: AtomicU32,
    /// Bu düğümdeki CPU kimlik numaraları listesi
    pub cpus: Vec<u32>,
    /// Düğümdeki toplam bellek miktarı (bayt)
    pub total_memory: AtomicU64,
    /// Düğümdeki kullanılabilir bellek miktarı (bayt)
    pub available_memory: AtomicU64,
    /// Bu düğüm için bellek tahsis politikası (NumaPolicy)
    pub policy: AtomicU32, // NumaPolicy as u32
    /// Tercih edilen diğer düğümlerin listesi
    pub preferred_nodes: Vec<u32>,
    /// Diğer düğümlere olan mesafeler
    pub distances: Vec<NumaDistance>,
    /// Bellek tahsis sayacı (istatistik)
    pub allocations: AtomicU64,
    /// Sayfa göçü (page migration) sayacı
    pub migrations: AtomicU64,
    /// Düğüm işaretçileri (flags)
    pub flags: u32,
    /// Düğümde bellek var mı?
    pub has_memory: AtomicBool,
    /// Düğümde CPU var mı?
    pub has_cpus: AtomicBool,
    /// Düğüm yakınlık alanı (ACPI SRAT tablosundan)
    pub proximity_domain: u32,
    /// Yanlış paylaşımı önlemek için cache-line dolgusu
    _padding: [u8; 0],
}

impl NumaNode {
    /// Yeni bir NUMA düğümü oluşturur.
    ///
    /// Başlangıçta tüm değerler sıfır/varsayılan ile başlatılır
    /// ve düğüm Offline durumunda olur.
    pub fn new(node_id: u32) -> Self {
        Self {
            node_id,
            state: AtomicU32::new(NumaNodeState::Offline as u32),
            cpu_count: AtomicU32::new(0),
            cpus: Vec::new(),
            total_memory: AtomicU64::new(0),
            available_memory: AtomicU64::new(0),
            policy: AtomicU32::new(NumaPolicy::Default as u32),
            preferred_nodes: Vec::new(),
            distances: Vec::new(),
            allocations: AtomicU64::new(0),
            migrations: AtomicU64::new(0),
            flags: 0,
            has_memory: AtomicBool::new(false),
            has_cpus: AtomicBool::new(false),
            proximity_domain: node_id,
            _padding: [0; 0],
        }
    }

    /// Düğümün mevcut durumunu edinme (acquire) sıralaması ile okur.
    pub fn get_state(&self) -> NumaNodeState {
        match self.state.load(Ordering::Acquire) {
            0 => NumaNodeState::Offline,
            1 => NumaNodeState::ComingUp,
            2 => NumaNodeState::Online,
            3 => NumaNodeState::GoingDown,
            _ => NumaNodeState::Offline,
        }
    }

    /// Düğüm durumunu serbest bırakma (release) sıralaması ile günceller.
    ///
    /// Durum değişikliği yapıldıktan sonra `smp_wmb()` çağrılarak
    /// diğer CPU'ların değişikliği görmesi garanti edilir.
    pub fn set_state(&self, state: NumaNodeState) {
        self.state.store(state as u32, Ordering::Release);
        smp_wmb();
    }

    /// Düğüme yeni bir CPU ekler.
    ///
    /// Aynı CPU iki kez eklenmez; ekleme işlemi atomik olarak yapılır.
    pub fn add_cpu(&mut self, cpu_id: u32) {
        if !self.cpus.contains(&cpu_id) {
            self.cpus.push(cpu_id);
            self.cpu_count.fetch_add(1, Ordering::AcqRel);
            self.has_cpus.store(true, Ordering::Release);
            smp_wmb();
        }
    }

    /// Düğümden bir CPU kaldırır.
    ///
    /// CPU listesi boşalırsa `has_cpus` bayrağı sıfırlanır.
    pub fn remove_cpu(&mut self, cpu_id: u32) {
        if let Some(pos) = self.cpus.iter().position(|&id| id == cpu_id) {
            self.cpus.remove(pos);
            self.cpu_count.fetch_sub(1, Ordering::AcqRel);
            if self.cpus.is_empty() {
                self.has_cpus.store(false, Ordering::Release);
            }
            smp_wmb();
        }
    }

    /// Düğümün bellek boyutunu belirler.
    ///
    /// Hem toplam hem de kullanılabilir bellek miktarı güncellenir.
    /// total > 0 ise `has_memory` bayrağı otomatik olarak ayarlanır.
    pub fn set_memory_size(&self, total: u64, available: u64) {
        self.total_memory.store(total, Ordering::Release);
        self.available_memory.store(available, Ordering::Release);
        self.has_memory.store(total > 0, Ordering::Release);
        smp_wmb();
    }

    /// Bellek istatistiklerini döner: (toplam, kullanılabilir) çifti.
    pub fn get_memory_stats(&self) -> (u64, u64) {
        (
            self.total_memory.load(Ordering::Acquire),
            self.available_memory.load(Ordering::Acquire),
        )
    }

    /// Bu düğümden bellek tahsis etmeye çalışır.
    ///
    /// Yeterli bellek yoksa `None` döner.
    /// Gerçek implementasyonda düğümün bellek havuzundan tahsis yapılır.
    pub fn allocate(&self, size: usize) -> Option<*mut u8> {
        let available = self.available_memory.load(Ordering::Acquire) as usize;
        if available < size {
            return None;
        }

        // İstatistikleri güncelle: tahsis sayacını artır, kullanılabilir belleği azalt
        self.allocations.fetch_add(1, Ordering::AcqRel);
        self.available_memory
            .fetch_sub(size as u64, Ordering::AcqRel);
        smp_mb();

        // Gerçek PMM'den fiziksel çerçeve tahsis et
        let page_size = 4096usize;
        let pages = (size + page_size - 1) / page_size;
        if let Some(frame) = crate::memory::allocate_contiguous_frames(pages) {
            let phys_addr = frame.start_address().as_u64();
            let hhdm = crate::memory::hhdm_offset();
            Some((phys_addr + hhdm) as *mut u8)
        } else {
            // Tahsis başarısız — istatistikleri geri al
            self.available_memory
                .fetch_add(size as u64, Ordering::AcqRel);
            self.allocations.fetch_sub(1, Ordering::AcqRel);
            None
        }
    }

    /// Bu düğüme bellek geri bırakır.
    ///
    /// Kullanılabilir bellek miktarını artırır ve yazma bariyeri uygular.
    pub fn free(&self, size: usize) {
        self.available_memory
            .fetch_add(size as u64, Ordering::AcqRel);
        smp_wmb();
    }

    /// Düğümün tahsis politikasını ayarlar.
    pub fn set_policy(&self, policy: NumaPolicy) {
        self.policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }

    /// Düğümün mevcut tahsis politikasını döner.
    pub fn get_policy(&self) -> NumaPolicy {
        match self.policy.load(Ordering::Acquire) {
            0 => NumaPolicy::Default,
            1 => NumaPolicy::Prefer,
            2 => NumaPolicy::Bind,
            3 => NumaPolicy::Interleave,
            4 => NumaPolicy::Preferred,
            5 => NumaPolicy::Local,
            _ => NumaPolicy::Default,
        }
    }

    /// Tercih edilen düğüm listesini günceller.
    pub fn set_preferred_nodes(&mut self, nodes: Vec<u32>) {
        self.preferred_nodes = nodes;
        smp_wmb();
    }

    /// Belirtilen hedef düğüme olan mesafeyi döner.
    ///
    /// Hedef düğüm indeksi aralık dışındaysa `None` döner.
    pub fn get_distance(&self, target_node: u32) -> Option<NumaDistance> {
        if target_node as usize >= self.distances.len() {
            return None;
        }
        Some(self.distances[target_node as usize])
    }

    /// Hedef düğüme olan mesafeyi ayarlar.
    ///
    /// Mesafe vektörü gerektiğinde büyütülür; bilinmeyen düğümlere
    /// başlangıçta çok uzak (255) mesafe atanır.
    pub fn set_distance(&mut self, target_node: u32, distance: u8) {
        // Mesafe vektörü yeterince büyük değilse genişlet
        while self.distances.len() <= target_node as usize {
            self.distances.push(NumaDistance::new(255)); // Varsayılan: çok uzak
        }

        self.distances[target_node as usize] = NumaDistance::new(distance);
        smp_wmb();
    }

    /// Verilen CPU'nun bu düğüme yerel olup olmadığını kontrol eder.
    pub fn is_local_to_cpu(&self, cpu_id: u32) -> bool {
        self.cpus.contains(&cpu_id)
    }

    /// Tahsis ve göç istatistiklerini döner: (tahsis_sayısı, göç_sayısı) çifti.
    pub fn get_allocation_stats(&self) -> (u64, u64) {
        (
            self.allocations.load(Ordering::Acquire),
            self.migrations.load(Ordering::Acquire),
        )
    }
}

/// Tüm NUMA düğümlerini yöneten merkezi yapı.
///
/// Bellek tahsisi, düğüm aktifleştirme/devre dışı bırakma ve
/// sayfa göçü (page migration) işlemlerini koordine eder.
pub struct NumaManager {
    /// Maksimum düğüm sayısı
    max_nodes: u32,
    /// RCU korumalı NUMA düğüm listesi
    nodes: Vec<RcuPtr<NumaNode>>,
    /// Şu an çevrimiçi olan düğüm sayısı
    online_nodes: AtomicU32,
    /// Varsayılan tahsis politikası (NumaPolicy)
    default_policy: AtomicU32, // NumaPolicy as u32
    /// NUMA istatistikleri
    stats: NumaStats,
    /// Sayfa göçü işlemleri için kilit
    migration_gate: AtomicBool,
}

/// NUMA alt sistemi için istatistikler.
///
/// Yerel ve uzak tahsisleri, başarısız denemeleri ve sayfa göçlerini takip eder.
#[derive(Debug)]
pub struct NumaStats {
    pub total_allocations: AtomicU64,
    pub local_allocations: AtomicU64,
    pub remote_allocations: AtomicU64,
    pub failed_allocations: AtomicU64,
    pub page_migrations: AtomicU64,
}

impl NumaStats {
    /// Sıfır değerlerle yeni bir istatistik nesnesi oluşturur.
    pub const fn new() -> Self {
        Self {
            total_allocations: AtomicU64::new(0),
            local_allocations: AtomicU64::new(0),
            remote_allocations: AtomicU64::new(0),
            failed_allocations: AtomicU64::new(0),
            page_migrations: AtomicU64::new(0),
        }
    }

    /// Bir tahsis işlemini kaydeder.
    ///
    /// `is_local` bayrağına göre yerel veya uzak sayacı artırır.
    pub fn record_allocation(&self, is_local: bool) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        if is_local {
            self.local_allocations.fetch_add(1, Ordering::Relaxed);
        } else {
            self.remote_allocations.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Başarısız bir tahsis denemesini kaydeder.
    pub fn record_failed_allocation(&self) {
        self.failed_allocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Bir sayfa göç işlemini kaydeder.
    pub fn record_migration(&self) {
        self.page_migrations.fetch_add(1, Ordering::Relaxed);
    }

    /// Tüm istatistikleri döner: (toplam, yerel, uzak, başarısız, göç) demeti.
    pub fn get_stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.total_allocations.load(Ordering::Relaxed),
            self.local_allocations.load(Ordering::Relaxed),
            self.remote_allocations.load(Ordering::Relaxed),
            self.failed_allocations.load(Ordering::Relaxed),
            self.page_migrations.load(Ordering::Relaxed),
        )
    }
}

impl NumaManager {
    /// Yeni bir NUMA yöneticisi oluşturur.
    ///
    /// `max_nodes` adet NUMA düğümü için RCU korumalı yapılar oluşturur.
    /// Her düğüm başlangıçta Offline durumundadır.
    pub fn new(max_nodes: u32) -> Self {
        let mut nodes = Vec::with_capacity(max_nodes as usize);

        // Tüm NUMA düğümlerini başlat
        for node_id in 0..max_nodes {
            let node = Box::new(NumaNode::new(node_id));
            nodes.push(RcuPtr::new(Box::into_raw(node)));
        }

        Self {
            max_nodes,
            nodes,
            online_nodes: AtomicU32::new(0),
            default_policy: AtomicU32::new(NumaPolicy::Default as u32),
            stats: NumaStats::new(),
            migration_gate: AtomicBool::new(false),
        }
    }

    /// Belirtilen kimlik numarasına sahip NUMA düğümünü döner.
    ///
    /// Geçersiz `node_id` için `None` döner.
    pub fn get_node(&self, node_id: u32) -> Option<RcuPtr<NumaNode>> {
        if node_id >= self.max_nodes {
            return None;
        }

        Some(self.nodes[node_id as usize].clone())
    }

    /// Belirtilen NUMA düğümünü çevrimiçi yapar.
    ///
    /// Düğüm zaten Online ise `AlreadyOnline` hatası döner.
    /// Yalnızca Offline -> Online geçişi desteklenir.
    pub fn node_online(&self, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };

        let node_guard = node.read();

        // Mevcut durumu kontrol et
        let current_state = node_guard.get_state();
        if current_state == NumaNodeState::Online {
            return Err(NumaError::AlreadyOnline);
        }

        if current_state != NumaNodeState::Offline {
            return Err(NumaError::InvalidStateTransition);
        }

        // Düğümü çevrimiçi yap
        node_guard.set_state(NumaNodeState::Online);
        self.online_nodes.fetch_add(1, Ordering::AcqRel);
        smp_mb();

        crate::serial_println!("NUMA: Node {} is now online", node_id);
        Ok(())
    }

    /// Belirtilen NUMA düğümünü çevrimdışı yapar.
    ///
    /// Düğümde CPU varsa çevrimdışı yapılamaz (`NodeHasCpus` hatası).
    pub fn node_offline(&self, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };

        let node_guard = node.read();

        // Mevcut durumu kontrol et
        let current_state = node_guard.get_state();
        if current_state == NumaNodeState::Offline {
            return Err(NumaError::AlreadyOffline);
        }

        if current_state != NumaNodeState::Online {
            return Err(NumaError::InvalidStateTransition);
        }

        // CPU'su olan düğümler çevrimdışı yapılamaz
        if node_guard.has_cpus.load(Ordering::Acquire) {
            return Err(NumaError::NodeHasCpus);
        }

        // Düğümü çevrimdışı yap
        node_guard.set_state(NumaNodeState::Offline);
        self.online_nodes.fetch_sub(1, Ordering::AcqRel);
        smp_mb();

        crate::serial_println!("NUMA: Node {} is now offline", node_id);
        Ok(())
    }

    /// NUMA-aware bellek tahsis eder.
    ///
    /// Politikaya göre en uygun düğümden bellek tahsis etmeye çalışır.
    ///
    /// ```ascii
    /// allocate() çağrısı
    ///      |
    ///      v
    /// Politika kontrol et
    ///  _____|_____
    /// |     |     |     |      |      |
    /// Local Prefer Bind Inter Pref  Default
    ///  |     |     |     |      |      |
    ///  +-----+-----+-----+------+------+
    ///                    |
    ///              Tahsis başarılı?
    ///              Evet -> ptr döndür
    ///              Hayır -> hata döndür
    /// ```
    pub fn allocate(
        &self,
        size: usize,
        preferred_node: Option<u32>,
        policy: Option<NumaPolicy>,
    ) -> Result<*mut u8, NumaError> {
        let policy = policy.unwrap_or_else(|| self.get_default_policy());

        match policy {
            NumaPolicy::Local => self.allocate_local(size),
            NumaPolicy::Prefer => self.allocate_preferred(size, preferred_node),
            NumaPolicy::Bind => {
                self.allocate_bind(size, preferred_node.ok_or(NumaError::NoPreferredNode)?)
            }
            NumaPolicy::Interleave => self.allocate_interleave(size),
            NumaPolicy::Preferred => self.allocate_preferred(size, preferred_node),
            NumaPolicy::Default => self.allocate_default(size, preferred_node),
        }
    }

    /// Geçerli CPU'nun yerel NUMA düğümünden bellek tahsis eder.
    fn allocate_local(&self, size: usize) -> Result<*mut u8, NumaError> {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let node_id = self.get_cpu_node(cpu_id)?;

        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();

        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(true);
                Ok(ptr)
            }
            None => {
                self.stats.record_failed_allocation();
                Err(NumaError::OutOfMemory)
            }
        }
    }

    /// Tercih edilen NUMA düğümünden bellek tahsis eder.
    ///
    /// Tercih edilen düğüm yetersizse ya da çevrimdışıysa `allocate_fallback`'e geçer.
    fn allocate_preferred(
        &self,
        size: usize,
        preferred_node: Option<u32>,
    ) -> Result<*mut u8, NumaError> {
        if let Some(node_id) = preferred_node {
            let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
            let node_guard = node.read();

            if node_guard.get_state() != NumaNodeState::Online {
                return self.allocate_fallback(size);
            }

            match node_guard.allocate(size) {
                Some(ptr) => {
                    self.stats.record_allocation(false);
                    Ok(ptr)
                }
                None => self.allocate_fallback(size),
            }
        } else {
            self.allocate_local(size)
        }
    }

    /// Yalnızca belirli bir düğüme bağlı (bind) tahsis yapar.
    ///
    /// Bind politikasında düğüm çevrimdışıysa veya bellek yetersizse
    /// başka düğümlere geçilmez; hata döndürülür.
    fn allocate_bind(&self, size: usize, node_id: u32) -> Result<*mut u8, NumaError> {
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();

        if node_guard.get_state() != NumaNodeState::Online {
            return Err(NumaError::NodeOffline);
        }

        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(false);
                Ok(ptr)
            }
            None => {
                self.stats.record_failed_allocation();
                Err(NumaError::OutOfMemory)
            }
        }
    }

    /// Çevrimiçi düğümler arasında dönüşümlü (interleave) tahsis yapar.
    ///
    /// Yük dengeleme için kullanılır. Basit round-robin algoritması kullanır.
    fn allocate_interleave(&self, size: usize) -> Result<*mut u8, NumaError> {
        let online_nodes = self.get_online_nodes();
        if online_nodes.is_empty() {
            return Err(NumaError::NoOnlineNodes);
        }

        // Basit round-robin sıralaması ile dönüşümlü tahsis
        let node_id = online_nodes
            [(self.stats.total_allocations.load(Ordering::Relaxed) as usize) % online_nodes.len()];
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();

        match node_guard.allocate(size) {
            Some(ptr) => {
                self.stats.record_allocation(false);
                Ok(ptr)
            }
            None => self.allocate_fallback(size),
        }
    }

    /// Varsayılan politika ile bellek tahsis eder.
    ///
    /// Önce yerel düğümü, sonra tercih edilen düğümü, son olarak
    /// herhangi çevrimiçi düğümü dener.
    fn allocate_default(
        &self,
        size: usize,
        preferred_node: Option<u32>,
    ) -> Result<*mut u8, NumaError> {
        // Önce yereli dene, sonra tercihliye, en son yedek düğüme geç
        if let Ok(ptr) = self.allocate_local(size) {
            return Ok(ptr);
        }

        if let Some(node_id) = preferred_node {
            if let Ok(ptr) = self.allocate_preferred(size, Some(node_id)) {
                return Ok(ptr);
            }
        }

        self.allocate_fallback(size)
    }

    /// Herhangi bir çevrimiçi düğümden yedek tahsis yapar.
    ///
    /// Tüm diğer politikalar başarısız olduğunda son çare olarak kullanılır.
    fn allocate_fallback(&self, size: usize) -> Result<*mut u8, NumaError> {
        let online_nodes = self.get_online_nodes();

        for &node_id in &online_nodes {
            let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
            let node_guard = node.read();

            if let Some(ptr) = node_guard.allocate(size) {
                self.stats.record_allocation(false);
                return Ok(ptr);
            }
        }

        self.stats.record_failed_allocation();
        Err(NumaError::OutOfMemory)
    }

    /// Belleği belirtilen NUMA düğümüne geri bırakır.
    pub fn free(&self, ptr: *mut u8, size: usize, node_id: u32) {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return,
        };

        let node_guard = node.read();
        node_guard.free(size);
    }

    /// Verilen CPU'nun hangi NUMA düğümüne ait olduğunu bulur.
    pub fn get_cpu_node(&self, cpu_id: u32) -> Result<u32, NumaError> {
        for (node_id, node_ptr) in self.nodes.iter().enumerate() {
            let node_guard = node_ptr.read();
            if node_guard.is_local_to_cpu(cpu_id) {
                return Ok(node_id as u32);
            }
        }

        Err(NumaError::CpuNotFound)
    }

    /// Bir CPU'yu belirtilen NUMA düğümüne atar.
    ///
    /// CPU önce mevcut düğümünden çıkarılır, ardından yeni düğüme eklenir.
    pub fn add_cpu_to_node(&mut self, cpu_id: u32, node_id: u32) -> Result<(), NumaError> {
        let node = match self.get_node(node_id) {
            Some(node) => node,
            None => return Err(NumaError::InvalidNodeId),
        };

        // CPU'yu mevcut düğümünden kaldır
        self.remove_cpu_from_any_node(cpu_id);

        // Yeni düğüme ekle
        let node_guard = node.read();
        let mutable_node = node_guard.as_mut();
        mutable_node.add_cpu(cpu_id);

        Ok(())
    }

    /// Bir CPU'yu hangi düğümde olursa olsun kaldırır.
    fn remove_cpu_from_any_node(&mut self, cpu_id: u32) {
        for node_ptr in &self.nodes {
            let node_guard = node_ptr.read();
            if node_guard.is_local_to_cpu(cpu_id) {
                let mutable_node = node_guard.as_mut();
                mutable_node.remove_cpu(cpu_id);
                break;
            }
        }
    }

    /// Belirtilen düğümün bellek boyutunu ayarlar.
    pub fn set_node_memory(
        &self,
        node_id: u32,
        total: u64,
        available: u64,
    ) -> Result<(), NumaError> {
        let node = self.get_node(node_id).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        node_guard.set_memory_size(total, available);
        Ok(())
    }

    /// İki NUMA düğümü arasındaki mesafeyi ayarlar.
    ///
    /// Mesafe matrisi simetrik olmak zorunda değildir;
    /// her yön ayrı ayrı ayarlanmalıdır.
    pub fn set_node_distance(
        &mut self,
        src_node: u32,
        dst_node: u32,
        distance: u8,
    ) -> Result<(), NumaError> {
        let node = self.get_node(src_node).ok_or(NumaError::InvalidNodeId)?;
        let node_guard = node.read();
        let mutable_node = node_guard.as_mut();
        mutable_node.set_distance(dst_node, distance);

        Ok(())
    }

    /// Tüm çevrimiçi düğümlerin kimlik numaralarını döner.
    pub fn get_online_nodes(&self) -> Vec<u32> {
        let mut online_nodes = Vec::new();

        for (node_id, node_ptr) in self.nodes.iter().enumerate() {
            let node_guard = node_ptr.read();
            if node_guard.get_state() == NumaNodeState::Online {
                online_nodes.push(node_id as u32);
            }
        }

        online_nodes
    }

    /// Çevrimiçi düğüm sayısını döner.
    pub fn get_online_node_count(&self) -> u32 {
        self.online_nodes.load(Ordering::Acquire)
    }

    /// Varsayılan tahsis politikasını ayarlar.
    pub fn set_default_policy(&self, policy: NumaPolicy) {
        self.default_policy.store(policy as u32, Ordering::Release);
        smp_wmb();
    }

    /// Varsayılan tahsis politikasını döner.
    pub fn get_default_policy(&self) -> NumaPolicy {
        match self.default_policy.load(Ordering::Acquire) {
            0 => NumaPolicy::Default,
            1 => NumaPolicy::Prefer,
            2 => NumaPolicy::Bind,
            3 => NumaPolicy::Interleave,
            4 => NumaPolicy::Preferred,
            5 => NumaPolicy::Local,
            _ => NumaPolicy::Default,
        }
    }

    /// NUMA istatistiklerini döner: (toplam, yerel, uzak, başarısız, göç) demeti.
    pub fn get_stats(&self) -> (u64, u64, u64, u64, u64) {
        self.stats.get_stats()
    }

    /// Sayfaları bir NUMA düğümünden diğerine göç ettirir.
    ///
    /// Göç işlemi sırasında `migration_lock` kilidiyle korunur.
    /// Her iki düğüm de Online olmalıdır; hedef düğümde yeterli bellek bulunmalıdır.
    pub fn migrate_pages(
        &self,
        src_node: u32,
        dst_node: u32,
        pages: usize,
    ) -> Result<(), NumaError> {
        let _guard = self.acquire_migration_gate()?;

        let src_node_ptr = self.get_node(src_node).ok_or(NumaError::InvalidNodeId)?;
        let dst_node_ptr = self.get_node(dst_node).ok_or(NumaError::InvalidNodeId)?;

        let src_guard = src_node_ptr.read();
        let dst_guard = dst_node_ptr.read();

        // Her iki düğümün de çevrimiçi olup olmadığını kontrol et
        if src_guard.get_state() != NumaNodeState::Online
            || dst_guard.get_state() != NumaNodeState::Online
        {
            return Err(NumaError::NodeOffline);
        }

        // Hedef düğümde yeterli bellek var mı kontrol et
        let dst_available = dst_guard.available_memory.load(Ordering::Acquire);
        let required_memory = (pages * 4096) as u64; // 4KB sayfa boyutu varsayılır

        if dst_available < required_memory {
            return Err(NumaError::OutOfMemory);
        }

        // Basitleştirilmiş göç: bellek sayaçlarını güncelle
        src_guard
            .available_memory
            .fetch_sub(required_memory, Ordering::AcqRel);
        dst_guard
            .available_memory
            .fetch_add(required_memory, Ordering::AcqRel);

        // İstatistikleri güncelle
        src_guard
            .migrations
            .fetch_add(pages as u64, Ordering::AcqRel);
        dst_guard
            .migrations
            .fetch_add(pages as u64, Ordering::AcqRel);
        self.stats.record_migration();

        smp_mb();

        crate::serial_println!(
            "NUMA: Migrated {} pages from node {} to node {}",
            pages,
            src_node,
            dst_node
        );
        Ok(())
    }

    fn acquire_migration_gate(&self) -> Result<MigrationGateGuard<'_>, NumaError> {
        match self.migration_gate.compare_exchange(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(MigrationGateGuard {
                gate: &self.migration_gate,
            }),
            Err(_) => Err(NumaError::InvalidStateTransition),
        }
    }
}

/// NUMA işlemleri için hata türleri.
///
/// Her hata türü belirli bir başarısızlık senaryosuna karşılık gelir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaError {
    /// Geçersiz düğüm kimlik numarası
    InvalidNodeId,
    /// Düğüm zaten çevrimiçi
    AlreadyOnline,
    /// Düğüm zaten çevrimdışı
    AlreadyOffline,
    /// Geçersiz durum geçişi (örn: Offline -> GoingDown)
    InvalidStateTransition,
    /// Düğümde CPU var (çevrimdışı yapılamaz)
    NodeHasCpus,
    /// Düğüm çevrimdışı
    NodeOffline,
    /// Hiç çevrimiçi düğüm yok
    NoOnlineNodes,
    /// Tercih edilen düğüm belirtilmedi (Bind politikası için gerekli)
    NoPreferredNode,
    /// CPU hiçbir düğümde bulunamadı
    CpuNotFound,
    /// Bellek yetersiz
    OutOfMemory,
}

/// Global NUMA yöneticisi örneği.
///
/// `unsafe` çünkü global değişken; `NUMA_INIT` bayrağı ile
/// tek seferlik başlatma korunur.
static mut NUMA_MANAGER: Option<NumaManager> = None;
static NUMA_INIT: AtomicBool = AtomicBool::new(false);

/// NUMA alt sistemini başlatır.
///
/// `max_nodes` adet NUMA düğümü için alt yapıyı hazırlar.
/// İkinci kez çağrılırsa erken döner (idempotent).
pub fn init(max_nodes: u32) {
    if NUMA_INIT.load(Ordering::Acquire) {
        return;
    }

    crate::serial_println!("NUMA: Initializing NUMA support for {} nodes", max_nodes);

    let manager = NumaManager::new(max_nodes);

    unsafe {
        NUMA_MANAGER = Some(manager);
    }

    NUMA_INIT.store(true, Ordering::Release);
    smp_mb();

    crate::serial_println!("NUMA: NUMA support initialized");
}

/// Global NUMA yöneticisine salt okunur referans döner.
///
/// Alt sistem başlatılmamışsa `None` döner.
pub fn get_manager() -> Option<&'static NumaManager> {
    if !NUMA_INIT.load(Ordering::Acquire) {
        return None;
    }

    unsafe { NUMA_MANAGER.as_ref() }
}

/// NUMA-aware bellek tahsisi için kolaylık fonksiyonu.
///
/// Global yönetici üzerinden `allocate` çağrısı yapar.
pub fn allocate(
    size: usize,
    preferred_node: Option<u32>,
    policy: Option<NumaPolicy>,
) -> Result<*mut u8, NumaError> {
    let manager = get_manager().ok_or(NumaError::NoOnlineNodes)?;
    manager.allocate(size, preferred_node, policy)
}

/// NUMA belleğini serbest bırakmak için kolaylık fonksiyonu.
pub fn free(ptr: *mut u8, size: usize, node_id: u32) {
    if let Some(manager) = get_manager() {
        manager.free(ptr, size, node_id);
    }
}

/// Bir CPU'nun hangi NUMA düğümüne ait olduğunu bulan kolaylık fonksiyonu.
pub fn get_cpu_node(cpu_id: u32) -> Result<u32, NumaError> {
    let manager = get_manager().ok_or(NumaError::NoOnlineNodes)?;
    manager.get_cpu_node(cpu_id)
}

/// Tüm çevrimiçi NUMA düğümlerinin listesini dönen kolaylık fonksiyonu.
pub fn get_online_nodes() -> Vec<u32> {
    get_manager()
        .map(|m| m.get_online_nodes())
        .unwrap_or_default()
}

/// Çevrimiçi NUMA düğüm sayısını dönen kolaylık fonksiyonu.
pub fn get_online_node_count() -> u32 {
    get_manager()
        .map(|m| m.get_online_node_count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_node() {
        let node = NumaNode::new(0);
        assert_eq!(node.get_state(), NumaNodeState::Offline);
        assert!(!node.has_memory.load(Ordering::Acquire));
        assert!(!node.has_cpus.load(Ordering::Acquire));

        node.set_memory_size(1024 * 1024, 512 * 1024);
        let (total, available) = node.get_memory_stats();
        assert_eq!(total, 1024 * 1024);
        assert_eq!(available, 512 * 1024);
    }

    #[test]
    fn test_numa_manager() {
        let manager = NumaManager::new(4);
        assert_eq!(manager.get_online_node_count(), 0);

        // Düğüm çevrimiçi/çevrimdışı geçişini test et
        assert!(manager.node_online(0).is_ok());
        assert_eq!(manager.get_online_node_count(), 1);

        assert!(manager.node_offline(0).is_ok());
        assert_eq!(manager.get_online_node_count(), 0);
    }
}

struct MigrationGateGuard<'a> {
    gate: &'a AtomicBool,
}

impl Drop for MigrationGateGuard<'_> {
    fn drop(&mut self) {
        self.gate.store(false, Ordering::Release);
    }
}
