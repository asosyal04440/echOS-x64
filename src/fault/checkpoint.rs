//! # Checkpoint (Kontrol Noktası) Sistemi
//!
//! Bilinen kararlı durumlara geri dönüş için sistem durum kaydı (checkpointing).
//! Hata kurtarma sırasında sistemin tutarlı bir duruma geri yüklenmesini sağlar.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// CHECKPOINT YAPISI
// ============================================================================

/// Sistem durum kontrol noktası — belirli bir andaki sistem anlık görüntüsü
#[derive(Clone)]
pub struct Checkpoint {
    /// Kontrol noktası benzersiz kimliği
    pub id: u64,
    /// Kontrol noktası adı
    pub name: String,
    /// Oluşturulma zaman damgası (tick cinsinden)
    pub timestamp: usize,
    /// Kontrol noktasındaki modül durumları
    pub module_states: BTreeMap<String, Vec<u8>>,
    /// Bu kontrol noktası geçerli mi?
    pub valid: bool,
    /// Bütünlük doğrulama sağlama toplamı (checksum)
    pub checksum: u64,
}

impl Checkpoint {
    pub fn new(name: &str) -> Self {
        static CHECKPOINT_ID: AtomicU64 = AtomicU64::new(0);
        
        Self {
            id: CHECKPOINT_ID.fetch_add(1, Ordering::SeqCst),
            name: String::from(name),
            timestamp: crate::task::scheduler::get_ticks(),
            module_states: BTreeMap::new(),
            valid: true,
            checksum: 0,
        }
    }
    
    /// Kontrol noktasına modül durumu ekler ve checksum'ı günceller
    pub fn add_state(&mut self, module: &str, state: Vec<u8>) {
        self.checksum = self.checksum.wrapping_add(
            state.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64))
        );
        self.module_states.insert(String::from(module), state);
    }
    
    /// Kontrol noktasından belirtilen modülün durumunu getirir
    pub fn get_state(&self, module: &str) -> Option<&Vec<u8>> {
        self.module_states.get(module)
    }
    
    /// Checksum kontrolü ile kontrol noktasının bütünlüğünü doğrular
    pub fn verify(&self) -> bool {
        let calculated = self.module_states.values()
            .flat_map(|v| v.iter())
            .fold(0u64, |acc, &b| acc.wrapping_add(b as u64));
        
        calculated == self.checksum
    }
}

// ============================================================================
// CHECKPOINT YÖNETİCİSİ
// ============================================================================

/// Checkpoint yöneticisi — kontrol noktalarını saklar ve yönetir
pub struct CheckpointManager {
    /// Kaydedilmiş kontrol noktaları listesi
    checkpoints: Mutex<Vec<Checkpoint>>,
    /// Saklanacak maksimum kontrol noktası sayısı
    max_checkpoints: AtomicU32,
    /// Otomatik kontrol noktası alımı etkin mi?
    auto_checkpoint: AtomicBool,
    /// Otomatik kontrol noktası aralığı (tick cinsinden)
    checkpoint_interval: AtomicUsize,
    /// Son kontrol noktası zaman damgası
    last_checkpoint: AtomicUsize,
    /// Hata durumunda otomatik kontrol noktası al
    checkpoint_on_fault: AtomicBool,
}

impl CheckpointManager {
    pub const fn new() -> Self {
        Self {
            checkpoints: Mutex::new(Vec::new()),
            max_checkpoints: AtomicU32::new(10),
            auto_checkpoint: AtomicBool::new(false),
            checkpoint_interval: AtomicUsize::new(60000), // 1 dakika
            last_checkpoint: AtomicUsize::new(0),
            checkpoint_on_fault: AtomicBool::new(true),
        }
    }
    
    /// Yeni bir kontrol noktası oluşturur ve kaydeder
    pub fn create(&self, name: &str) -> Checkpoint {
        let mut checkpoint = Checkpoint::new(name);
        
        // Modül durumlarını yakala
        self.capture_states(&mut checkpoint);
        
        // Kontrol noktasını kaydet
        {
            let mut checkpoints = self.checkpoints.lock();
            if checkpoints.len() >= self.max_checkpoints.load(Ordering::SeqCst) as usize {
                checkpoints.remove(0); // En eski kontrol noktasını sil
            }
            checkpoints.push(checkpoint.clone());
        }
        
        self.last_checkpoint.store(checkpoint.timestamp, Ordering::SeqCst);
        
        crate::serial_println!("[CHECKPOINT] Created: {} (id: {})", name, checkpoint.id);
        
        checkpoint
    }
    
    /// Mevcut sistem durumlarını yakalar (zamanlayıcı, bellek, hata istatistikleri)
    fn capture_states(&self, checkpoint: &mut Checkpoint) {
        // Zamanlayıcı durumunu yakala
        let scheduler_state = self.capture_scheduler_state();
        checkpoint.add_state("scheduler", scheduler_state);
        
        // Bellek durumunu yakala
        let memory_state = self.capture_memory_state();
        checkpoint.add_state("memory", memory_state);
        
        // Hata yönetim durumunu yakala
        let fault_state = self.capture_fault_state();
        checkpoint.add_state("fault", fault_state);
    }
    
    fn capture_scheduler_state(&self) -> Vec<u8> {
        let stats = crate::task::scheduler::get_stats();
        // İstatistikleri serileştir (basitleştirilmiş)
        let mut state = Vec::new();
        state.extend_from_slice(&stats.total_tasks.to_le_bytes());
        state.extend_from_slice(&stats.running_tasks.to_le_bytes());
        state.extend_from_slice(&stats.zombie_count.to_le_bytes());
        state
    }
    
    fn capture_memory_state(&self) -> Vec<u8> {
        let free = crate::memory::global_memory_manager()
            .map(|m: &crate::memory::MemoryManager| m.free_frames())
            .unwrap_or(0);
        let total = crate::memory::global_memory_manager()
            .map(|m: &crate::memory::MemoryManager| m.total_frames())
            .unwrap_or(1);
        
        let mut state = Vec::new();
        state.extend_from_slice(&free.to_le_bytes());
        state.extend_from_slice(&total.to_le_bytes());
        state
    }
    
    fn capture_fault_state(&self) -> Vec<u8> {
        let stats = crate::fault::get_stats();
        let mut state = Vec::new();
        state.extend_from_slice(&stats.total_faults.to_le_bytes());
        state.extend_from_slice(&stats.total_recoveries.to_le_bytes());
        state.extend_from_slice(&stats.recovery_level.to_le_bytes());
        state
    }
    
    /// En son oluşturulan kontrol noktasını getirir
    pub fn latest(&self) -> Option<Checkpoint> {
        self.checkpoints.lock().last().cloned()
    }
    
    /// Belirtilen ID ile kontrol noktasını getirir
    pub fn get(&self, id: u64) -> Option<Checkpoint> {
        self.checkpoints.lock().iter().find(|c| c.id == id).cloned()
    }
    
    /// Kontrol noktasına geri döner (sınırlı — çekirdek durumunu tam olarak geri yükleyemez)
    pub fn restore(&self, id: u64) -> bool {
        if let Some(checkpoint) = self.get(id) {
            if !checkpoint.verify() {
                crate::serial_println!("[CHECKPOINT] Checkpoint {} corrupted", id);
                return false;
            }
            
            crate::serial_println!("[CHECKPOINT] Restoring to checkpoint {}", id);
            
            // Sınırlı geri yükleme — çoğunlukla bilgilendirici amaçlı
            // Tam geri yükleme çok daha karmaşık durum yönetimi gerektirir
            
            true
        } else {
            false
        }
    }
    
    /// Tüm kontrol noktalarını listeler
    pub fn list(&self) -> Vec<Checkpoint> {
        self.checkpoints.lock().clone()
    }
    
    /// Tüm kontrol noktalarını siler
    pub fn clear(&self) {
        self.checkpoints.lock().clear();
    }
    
    /// Periyodik kontrol noktası kontrolü — otomatik zamanlama için çağrılır
    pub fn periodic_check(&self) {
        if !self.auto_checkpoint.load(Ordering::SeqCst) {
            return;
        }
        
        let current = crate::task::scheduler::get_ticks();
        let last = self.last_checkpoint.load(Ordering::SeqCst);
        let interval = self.checkpoint_interval.load(Ordering::SeqCst);
        
        if current.saturating_sub(last) >= interval {
            self.create("auto");
        }
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    pub static ref CHECKPOINT_MANAGER: CheckpointManager = CheckpointManager::new();
}

// ============================================================================
// GENEL (PUBLIC) API
// ============================================================================

pub fn create(name: &str) -> Checkpoint {
    CHECKPOINT_MANAGER.create(name)
}

pub fn latest() -> Option<Checkpoint> {
    CHECKPOINT_MANAGER.latest()
}

pub fn get(id: u64) -> Option<Checkpoint> {
    CHECKPOINT_MANAGER.get(id)
}

pub fn restore(id: u64) -> bool {
    CHECKPOINT_MANAGER.restore(id)
}

pub fn list() -> Vec<Checkpoint> {
    CHECKPOINT_MANAGER.list()
}

pub fn clear() {
    CHECKPOINT_MANAGER.clear();
}

pub fn periodic_check() {
    CHECKPOINT_MANAGER.periodic_check();
}

pub fn set_auto_checkpoint(enabled: bool, interval_ticks: usize) {
    CHECKPOINT_MANAGER.auto_checkpoint.store(enabled, Ordering::SeqCst);
    CHECKPOINT_MANAGER.checkpoint_interval.store(interval_ticks, Ordering::SeqCst);
}
