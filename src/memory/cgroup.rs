//! # Bellek Cgroup'ları
//!
//! Süreç grupları için kaynak kontrolü (cgroups v2 bellek denetleyicisi).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use spin::{Mutex, RwLock};

// ============================================================================
// CGROUP SABİTLERİ
// ============================================================================

/// Maksimum cgroup sayısı
pub const CGROUP_MAX: usize = 4096;
/// Varsayılan bellek limiti (sınırsız)
pub const MEMORY_LIMIT_UNLIMITED: u64 = u64::MAX;
/// Varsayılan oom_score_adj
pub const OOM_SCORE_ADJ_DEFAULT: i32 = 0;
/// OOM skor düzeltme min/maks
pub const OOM_SCORE_ADJ_MIN: i32 = -1000;
pub const OOM_SCORE_ADJ_MAX: i32 = 1000;

// ============================================================================
// BELLEK CGROUP
// ============================================================================

/// Bellek cgroup'u
#[derive(Debug)]
pub struct MemoryCgroup {
    /// Cgroup kimliği
    pub id: u64,
    /// Ad/yol
    pub name: String,
    /// Üst cgroup
    pub parent: Option<u64>,
    /// Alt cgroup'lar
    pub children: Mutex<Vec<u64>>,
    /// Bu cgroup'taki süreçler
    pub processes: Mutex<Vec<u64>>,
    /// Bellek limiti
    pub limit: AtomicU64,
    /// Mevcut kullanım
    pub usage: AtomicU64,
    /// En yüksek kullanım
    pub peak_usage: AtomicU64,
    /// Yumuşak limit
    pub soft_limit: AtomicU64,
    /// Takas limiti
    pub swap_limit: AtomicU64,
    /// Takas kullanımı
    pub swap_usage: AtomicU64,
    /// OOM skor düzeltmesi
    pub oom_score_adj: AtomicI64,
    /// OOM kill etkin
    pub oom_kill_enable: AtomicBool,
    /// Bellek olayları
    pub events: Mutex<MemoryEvents>,
    /// İstatistikler
    pub stats: Mutex<MemoryStats>,
    /// Kök cgroup mu
    pub is_root: bool,
}

/// Bellek olayları
#[derive(Clone, Debug, Default)]
pub struct MemoryEvents {
    /// Düşük olaylar (düşük eşiğin altında)
    pub low: u64,
    /// Yüksek olaylar (yüksek eşiğin üzerinde)
    pub high: u64,
    /// Maks olaylar (limitte)
    pub max: u64,
    /// OOM olayları
    pub oom: u64,
    /// OOM kill olayları
    pub oom_kill: u64,
    /// OOM grup kill olayları
    pub oom_group_kill: u64,
}

/// Bellek istatistikleri
#[derive(Clone, Debug, Default)]
pub struct MemoryStats {
    /// Anonim bellek
    pub anon: u64,
    /// Dosya önbelleği
    pub file: u64,
    /// Çekirdek belleği
    pub kernel: u64,
    /// Çekirdek yığını
    pub kernel_stack: u64,
    /// Sayfa tabloları
    pub pgtable: u64,
    /// Takas önbelleği
    pub swap_cache: u64,
    /// Etkin anonim
    pub active_anon: u64,
    /// Etkin olmayan anonim
    pub inactive_anon: u64,
    /// Etkin dosya
    pub active_file: u64,
    /// Etkin olmayan dosya
    pub inactive_file: u64,
    /// Tahliye edilemez
    pub unevictable: u64,
    /// Geri kazanılabilir slab
    pub slab_reclaimable: u64,
    /// Geri kazanılamaz slab
    pub slab_unreclaimable: u64,
    /// Çalışma kümesi yeniden hata
    pub workingset_refault: u64,
    /// Çalışma kümesi etkinleştirme
    pub workingset_activate: u64,
    /// Çalışma kümesi düğüm geri kazanım
    pub workingset_nodereclaim: u64,
    /// Sayfa hatası sayısı
    pub pgfault: u64,
    /// Büyük hata sayısı
    pub pgmajfault: u64,
    /// Yeniden hata sayısı
    pub refault: u64,
}

impl MemoryCgroup {
    pub fn new(id: u64, name: &str, parent: Option<u64>, is_root: bool) -> Self {
        Self {
            id,
            name: String::from(name),
            parent,
            children: Mutex::new(Vec::new()),
            processes: Mutex::new(Vec::new()),
            limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            usage: AtomicU64::new(0),
            peak_usage: AtomicU64::new(0),
            soft_limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            swap_limit: AtomicU64::new(MEMORY_LIMIT_UNLIMITED),
            swap_usage: AtomicU64::new(0),
            oom_score_adj: AtomicI64::new(OOM_SCORE_ADJ_DEFAULT as i64),
            oom_kill_enable: AtomicBool::new(true),
            events: Mutex::new(MemoryEvents::default()),
            stats: Mutex::new(MemoryStats::default()),
            is_root,
        }
    }

    /// Süreci cgroup'a ekle
    pub fn add_process(&self, pid: u64) {
        let mut procs = self.processes.lock();
        if !procs.contains(&pid) {
            procs.push(pid);
        }
    }

    /// Süreci cgroup'tan kaldır
    pub fn remove_process(&self, pid: u64) {
        self.processes.lock().retain(|&p| p != pid);
    }

    /// Bu cgroup'a bellek yükle
    pub fn charge(&self, bytes: u64) -> Result<(), CgroupError> {
        let limit = self.limit.load(Ordering::SeqCst);
        let current = self.usage.load(Ordering::SeqCst);
        let new_usage = current + bytes;

        // Limiti kontrol et
        if limit != MEMORY_LIMIT_UNLIMITED && new_usage > limit {
            // Bellek limiti aşıldı
            self.events.lock().max += 1;

            // Geri kazanmayı dene
            if self.try_reclaim(bytes) {
                return Ok(());
            }

            // OOM kill'in etkinleştirilip etkinleştirilmediğini kontrol et
            if self.oom_kill_enable.load(Ordering::SeqCst) {
                self.trigger_oom();
            }

            return Err(CgroupError::MemoryLimitExceeded);
        }

        self.usage.store(new_usage, Ordering::SeqCst);

        // En yüksek değeri güncelle
        let peak = self.peak_usage.load(Ordering::SeqCst);
        if new_usage > peak {
            self.peak_usage.store(new_usage, Ordering::SeqCst);
        }

        // Üst cgroup'a yay
        // if let Some(parent_id) = self.parent {
        //     if let Some(parent) = CGROUP_MANAGER.get_cgroup(parent_id) {
        //         parent.charge(bytes);
        //     }
        // }

        Ok(())
    }

    /// Bellek yükünü kaldır
    pub fn uncharge(&self, bytes: u64) {
        let current = self.usage.load(Ordering::SeqCst);
        let new_usage = current.saturating_sub(bytes);
        self.usage.store(new_usage, Ordering::SeqCst);
    }

    /// Belleği geri kazanmaya çalış
    fn try_reclaim(&self, needed: u64) -> bool {
        // Bu cgroup için bellek geri kazanımı tetikle
        // Bellek yönetimini çağırır
        false
    }

    /// OOM kill'i tetikle
    fn trigger_oom(&self) {
        let mut events = self.events.lock();
        events.oom += 1;
        events.oom_kill += 1;

        crate::serial_println!(
            "[CGROUP] OOM kill triggered for cgroup '{}' (usage: {}, limit: {})",
            self.name,
            self.usage.load(Ordering::SeqCst),
            self.limit.load(Ordering::SeqCst)
        );

        // Bu cgroup'un süreçleri için OOM killer'ı çağır
        // crate::memory::oom::oom_kill_cgroup(self);
    }

    /// Bellek limitini ayarla
    pub fn set_limit(&self, limit: u64) {
        self.limit.store(limit, Ordering::SeqCst);

        // Zaten limitin üzerinde miyiz kontrol et
        let usage = self.usage.load(Ordering::SeqCst);
        if limit != MEMORY_LIMIT_UNLIMITED && usage > limit {
            self.try_reclaim(usage - limit);
        }
    }

    /// Süreç sayısını al
    pub fn process_count(&self) -> usize {
        self.processes.lock().len()
    }

    /// Alt cgroup sayısını al
    pub fn child_count(&self) -> usize {
        self.children.lock().len()
    }
}

// ============================================================================
// CGROUP YÖNETİCİSİ
// ============================================================================

/// Cgroup yöneticisi
pub struct CgroupManager {
    /// Tüm cgroup'lar
    cgroups: RwLock<BTreeMap<u64, MemoryCgroup>>,
    /// Sonraki cgroup kimliği
    next_id: AtomicU64,
    /// Kök cgroup
    root_id: AtomicU64,
    /// Süreç-cgroup eşlemesi
    proc_to_cgroup: Mutex<BTreeMap<u64, u64>>,
}

impl CgroupManager {
    pub const fn new() -> Self {
        Self {
            cgroups: RwLock::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
            root_id: AtomicU64::new(0),
            proc_to_cgroup: Mutex::new(BTreeMap::new()),
        }
    }

    /// Kök cgroup ile başlat
    pub fn init(&self) {
        let root = MemoryCgroup::new(0, "/", None, true);
        self.cgroups.write().insert(0, root);
        self.root_id.store(0, Ordering::SeqCst);

        crate::serial_println!("[CGROUP] Initialized root cgroup");
    }

    /// Yeni cgroup oluştur
    pub fn create_cgroup(&self, name: &str, parent_id: u64) -> Result<u64, CgroupError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        // Üst cgroup'un var olduğunu doğrula
        {
            let cgroups = self.cgroups.read();
            if !cgroups.contains_key(&parent_id) {
                return Err(CgroupError::ParentNotFound);
            }
        }

        let cgroup = MemoryCgroup::new(id, name, Some(parent_id), false);

        // Üst cgroup'un alt listesine ekle
        {
            let cgroups = self.cgroups.read();
            if let Some(parent) = cgroups.get(&parent_id) {
                parent.children.lock().push(id);
            }
        }

        self.cgroups.write().insert(id, cgroup);

        crate::serial_println!("[CGROUP] Created cgroup '{}' (id={})", name, id);

        Ok(id)
    }

    /// Cgroup'u kaldır
    pub fn remove_cgroup(&self, id: u64) -> Result<(), CgroupError> {
        let cgroups = self.cgroups.read();

        if let Some(cgroup) = cgroups.get(&id) {
            // Süreç varsa kaldırılamaz
            if cgroup.process_count() > 0 {
                return Err(CgroupError::NotEmpty);
            }

            // Alt cgroup varsa kaldırılamaz
            if cgroup.child_count() > 0 {
                return Err(CgroupError::HasChildren);
            }

            // Üst cgroup'tan kaldır
            if let Some(parent_id) = cgroup.parent {
                if let Some(parent) = cgroups.get(&parent_id) {
                    parent.children.lock().retain(|&c| c != id);
                }
            }
        }

        drop(cgroups);
        self.cgroups.write().remove(&id);

        Ok(())
    }

    /// Kimliğe göre cgroup al
    pub fn get_cgroup(&self, id: u64) -> Option<MemoryCgroup> {
        self.cgroups.read().get(&id).cloned()
    }

    /// Süreci cgroup'a taşı
    pub fn move_process(&self, pid: u64, cgroup_id: u64) -> Result<(), CgroupError> {
        // Eski cgroup'tan kaldır
        if let Some(old_id) = self.proc_to_cgroup.lock().get(&pid).copied() {
            if let Some(old_cgroup) = self.get_cgroup(old_id) {
                old_cgroup.remove_process(pid);
            }
        }

        // Yeni cgroup'a ekle
        let cgroup = self.get_cgroup(cgroup_id).ok_or(CgroupError::NotFound)?;
        cgroup.add_process(pid);
        self.proc_to_cgroup.lock().insert(pid, cgroup_id);

        Ok(())
    }

    /// Süreç için cgroup al
    pub fn get_cgroup_for_process(&self, pid: u64) -> Option<u64> {
        self.proc_to_cgroup.lock().get(&pid).copied()
    }

    /// Süreç için bellek yükle
    pub fn charge_process(&self, pid: u64, bytes: u64) -> Result<(), CgroupError> {
        let cgroup_id = self.get_cgroup_for_process(pid).unwrap_or(0);

        if let Some(cgroup) = self.get_cgroup(cgroup_id) {
            cgroup.charge(bytes)?;
        }

        Ok(())
    }

    /// Süreç için bellek yükünü kaldır
    pub fn uncharge_process(&self, pid: u64, bytes: u64) {
        let cgroup_id = self.get_cgroup_for_process(pid).unwrap_or(0);

        if let Some(cgroup) = self.get_cgroup(cgroup_id) {
            cgroup.uncharge(bytes);
        }
    }

    /// Tüm cgroup'ları listele
    pub fn list_cgroups(&self) -> Vec<(u64, String)> {
        self.cgroups.read()
            .iter()
            .map(|(id, cg)| (*id, cg.name.clone()))
            .collect()
    }
}

// MemoryCgroup için Clone implementasyonu (get_cgroup için gerekli)
impl Clone for MemoryCgroup {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            parent: self.parent,
            children: Mutex::new(self.children.lock().clone()),
            processes: Mutex::new(self.processes.lock().clone()),
            limit: AtomicU64::new(self.limit.load(Ordering::SeqCst)),
            usage: AtomicU64::new(self.usage.load(Ordering::SeqCst)),
            peak_usage: AtomicU64::new(self.peak_usage.load(Ordering::SeqCst)),
            soft_limit: AtomicU64::new(self.soft_limit.load(Ordering::SeqCst)),
            swap_limit: AtomicU64::new(self.swap_limit.load(Ordering::SeqCst)),
            swap_usage: AtomicU64::new(self.swap_usage.load(Ordering::SeqCst)),
            oom_score_adj: AtomicI64::new(self.oom_score_adj.load(Ordering::SeqCst)),
            oom_kill_enable: AtomicBool::new(self.oom_kill_enable.load(Ordering::SeqCst)),
            events: Mutex::new(self.events.lock().clone()),
            stats: Mutex::new(self.stats.lock().clone()),
            is_root: self.is_root,
        }
    }
}

lazy_static::lazy_static! {
    /// Global cgroup yöneticisi
    pub static ref CGROUP_MANAGER: CgroupManager = CgroupManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupError {
    NotFound,
    ParentNotFound,
    NotEmpty,
    HasChildren,
    MemoryLimitExceeded,
    PermissionDenied,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Cgroup alt sistemini başlat
pub fn init() {
    CGROUP_MANAGER.init();
    crate::serial_println!("[CGROUP] Subsystem initialized");
}

/// Cgroup oluştur
pub fn create(name: &str, parent: u64) -> Result<u64, CgroupError> {
    CGROUP_MANAGER.create_cgroup(name, parent)
}

/// Cgroup istatistiklerini al
pub fn get_stats(cgroup_id: u64) -> Option<MemoryStats> {
    CGROUP_MANAGER.get_cgroup(cgroup_id).map(|cg| cg.stats.lock().clone())
}
