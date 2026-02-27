//! # Kurtarma Motoru (Recovery Engine)
//!
//! Merkezi kurtarma koordinasyonu ve aksiyon yürUtmesi.
//! Her hata türü için strateji belirler: birincil, yedek ve son çare eylemleri.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

use super::{Fault, FaultSource, FaultType};
use super::severity::{Severity, RecoveryResult};

// ============================================================================
// KURTARMA EYLEMLERİ
// ============================================================================

/// Yürütülecek kurtarma eylemi
#[derive(Clone, Debug)]
pub enum RecoveryAction {
    /// Eylem gerekmez
    None,
    /// Hatayı yalnızca günlülere yaz ve devam et
    LogOnly,
    /// Modülü sıfırla
    ResetModule(String),
    /// Modülü devre dışı bırak
    DisableModule(String),
    /// Yedek moda geç
    FallbackMode(String),
    /// Belirtilen görevi sonlandır
    KillTask(u64),
    /// Bellek sayfalarını serbest bırak
    FreeMemory(usize),
    /// Dosya sistemini senkronize et
    SyncFilesystem,
    /// Acil senkronizasyon ve durdurma
    EmergencyHalt,
    /// Yeniden başlatı
    Reboot,
}

/// Bir hata türü için kurtarma stratejisi
pub struct RecoveryStrategy {
    /// Birincil kurtarma eylemi
    pub primary: RecoveryAction,
    /// Birincil başarısız olursa yedek eylem
    pub fallback: Option<RecoveryAction>,
    /// Son çare eylemi
    pub last_resort: RecoveryAction,
    /// Maksimum deneme sayısı
    pub max_attempts: u32,
    /// Zaman aşımı (tick cinsinden)
    pub timeout_ticks: u64,
}

impl RecoveryStrategy {
    pub fn new(primary: RecoveryAction, fallback: Option<RecoveryAction>, last_resort: RecoveryAction) -> Self {
        Self {
            primary,
            fallback,
            last_resort,
            max_attempts: 3,
            timeout_ticks: 1000,
        }
    }
    
    /// Hata türüne göre uygun stratejiyi döndürür
    pub fn for_fault(fault: &Fault) -> Self {
        match &fault.fault_type {
            // Bellek hataları
            FaultType::HeapCorruption => Self::new(
                RecoveryAction::LogOnly, // Gerçek kurtarma mümkün değil
                None,
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::OutOfMemory => Self::new(
                RecoveryAction::FreeMemory(64),
                Some(RecoveryAction::KillTask(0)), // En büyük görevi sonlandır
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::DoubleFree | FaultType::UseAfterFree => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::DisableModule("memory".into()),
            ),
            
            // CPU/SMP hataları
            FaultType::ApStartupFailed => Self::new(
                RecoveryAction::LogOnly, // Zaten SMP güvenliği tarafından yönetildi
                None,
                RecoveryAction::LogOnly,
            ),
            FaultType::TlbShootdownTimeout => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::ResetModule("smp".into()),
            ),
            
            // Kesme (interrupt) hataları
            FaultType::IrqStorm => Self::new(
                RecoveryAction::DisableModule("irq_source".into()),
                None,
                RecoveryAction::LogOnly,
            ),
            FaultType::HandlerTimeout => Self::new(
                RecoveryAction::ResetModule("interrupts".into()),
                None,
                RecoveryAction::DisableModule("interrupts".into()),
            ),
            
            // Zamanlayıcı hataları
            FaultType::RunQueueCorruption => Self::new(
                RecoveryAction::EmergencyHalt,
                None,
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::TaskLeak => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::LogOnly,
            ),
            
            // Sürücsü hataları
            FaultType::DeviceTimeout | FaultType::DeviceError => Self::new(
                RecoveryAction::ResetModule("driver".into()),
                Some(RecoveryAction::DisableModule("driver".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Dosya sistemi hataları
            FaultType::MetadataCorruption => Self::new(
                RecoveryAction::SyncFilesystem,
                Some(RecoveryAction::DisableModule("filesystem".into())),
                RecoveryAction::EmergencyHalt,
            ),
            FaultType::IoError => Self::new(
                RecoveryAction::LogOnly,
                Some(RecoveryAction::DisableModule("filesystem".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Ağ (network) hataları
            FaultType::ConnectionReset | FaultType::StackCorruption => Self::new(
                RecoveryAction::ResetModule("network".into()),
                Some(RecoveryAction::DisableModule("network".into())),
                RecoveryAction::LogOnly,
            ),
            
            // Güvenlik hataları
            FaultType::CanaryMismatch => Self::new(
                RecoveryAction::EmergencyHalt, // Olası istismar (exploit)
                None,
                RecoveryAction::EmergencyHalt,
            ),
            
            // Varsayılan
            _ => Self::new(
                RecoveryAction::LogOnly,
                None,
                RecoveryAction::LogOnly,
            ),
        }
    }
}

// ============================================================================
// KURTARMA MOTORU
// ============================================================================

/// Kurtarma motorunun durumu
pub struct RecoveryEngine {
    /// Hata başına kurtarma deneme sayısı
    attempts: Mutex<BTreeMap<u64, u32>>,
    /// Aktif kurtarmalar
    active: Mutex<Vec<u64>>,
    /// Kurtarma etkin mi?
    enabled: AtomicBool,
    /// Toplam kurtarma deneme sayısı
    total_attempts: AtomicU32,
    /// Başarılı kurtarmalar
    successful: AtomicU32,
    /// Başarısız kurtarmalar
    failed: AtomicU32,
}

impl RecoveryEngine {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(BTreeMap::new()),
            active: Mutex::new(Vec::new()),
            enabled: AtomicBool::new(true),
            total_attempts: AtomicU32::new(0),
            successful: AtomicU32::new(0),
            failed: AtomicU32::new(0),
        }
    }
    
    /// Attempt recovery for a fault
    pub fn recover(&self, fault: &Fault) -> RecoveryResult {
        if !self.enabled.load(Ordering::SeqCst) {
            return RecoveryResult::Failed;
        }
        
        // Check if already being recovered
        if self.active.lock().contains(&fault.id.0) {
            return RecoveryResult::Failed;
        }
        
        // Aktif olarak işaretle
        self.active.lock().push(fault.id.0);
        
        // Stratejiyi belirle
        let strategy = RecoveryStrategy::for_fault(fault);
        
        // Deneme sayısını kontrol et
        let attempts = *self.attempts.lock().get(&fault.id.0).unwrap_or(&0);
        if attempts >= strategy.max_attempts {
            self.active.lock().retain(|&id| id != fault.id.0);
            return RecoveryResult::Failed;
        }
        
        // Increment attempts
        self.attempts.lock().insert(fault.id.0, attempts + 1);
        self.total_attempts.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!(
            "[RECOVERY] Attempting recovery for fault #{:?} (attempt {}/{})",
            fault.id, attempts + 1, strategy.max_attempts
        );
        
        // Execute primary action
        let result = self.execute_action(&strategy.primary, fault);
        
        if result.is_success() {
            self.successful.fetch_add(1, Ordering::SeqCst);
            self.active.lock().retain(|&id| id != fault.id.0);
            return result;
        }
        
        // Yedek eylemi dene
        if let Some(fallback) = &strategy.fallback {
            crate::serial_println!("[RECOVERY] Birincil başarısız, yedek deneniyor");
            let result = self.execute_action(fallback, fault);
            if result.is_success() {
                self.successful.fetch_add(1, Ordering::SeqCst);
                self.active.lock().retain(|&id| id != fault.id.0);
                return result;
            }
        }
        
        // Son çareyi yürüt
        crate::serial_println!("[RECOVERY] Tüm denemeler başarısız, son çare yürütülüyor");
        let result = self.execute_action(&strategy.last_resort, fault);
        
        if !result.is_success() {
            self.failed.fetch_add(1, Ordering::SeqCst);
        }
        
        self.active.lock().retain(|&id| id != fault.id.0);
        result
    }
    
    /// Kurtarma eylemini yürütür
    fn execute_action(&self, action: &RecoveryAction, fault: &Fault) -> RecoveryResult {
        match action {
            RecoveryAction::None => RecoveryResult::Recovered,
            
            RecoveryAction::LogOnly => {
                crate::serial_println!(
                    "[RECOVERY] Logged fault: {:?} - {}",
                    fault.fault_type, fault.message
                );
                RecoveryResult::Recovered
            }
            
            RecoveryAction::ResetModule(module) => {
                crate::serial_println!("[RECOVERY] Resetting module: {}", module);
                self.reset_module(module)
            }
            
            RecoveryAction::DisableModule(module) => {
                crate::serial_println!("[RECOVERY] Disabling module: {}", module);
                self.disable_module(module)
            }
            
            RecoveryAction::FallbackMode(mode) => {
                crate::serial_println!("[RECOVERY] Entering fallback mode: {}", mode);
                self.enter_fallback(mode)
            }
            
            RecoveryAction::KillTask(task_id) => {
                crate::serial_println!("[RECOVERY] Killing task: {}", task_id);
                self.kill_task(*task_id)
            }
            
            RecoveryAction::FreeMemory(pages) => {
                crate::serial_println!("[RECOVERY] Freeing {} pages", pages);
                self.free_memory(*pages)
            }
            
            RecoveryAction::SyncFilesystem => {
                crate::serial_println!("[RECOVERY] Syncing filesystem");
                self.sync_filesystem()
            }
            
            RecoveryAction::EmergencyHalt => {
                crate::serial_println!("[RECOVERY] EMERGENCY HALT");
                self.emergency_halt()
            }
            
            RecoveryAction::Reboot => {
                crate::serial_println!("[RECOVERY] Rebooting system");
                self.reboot()
            }
        }
    }
    
    /// Modülü sıfırlar
    fn reset_module(&self, module: &str) -> RecoveryResult {
        match module {
            "network" => {
                // Ağ yığınını sıfırla
                crate::serial_println!("[RECOVERY] Ağ yığını sıfırlama henüz uygulanmadı");
                RecoveryResult::Degraded
            }
            "driver" => {
                // Sürücsü kurtarma modülü tarafından yönetilir
                RecoveryResult::Degraded
            }
            "interrupts" => {
                // IDT'yi yeniden başlat
                crate::serial_println!("[RECOVERY] IDT sıfırlaması güvenli değil, bozunmuş mod");
                RecoveryResult::Degraded
            }
            _ => RecoveryResult::Failed,
        }
    }
    
    /// Modülü devre dışı bırakır
    fn disable_module(&self, module: &str) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] Module {} disabled", module);
        RecoveryResult::Degraded
    }
    
    /// Yedek moda geçer
    fn enter_fallback(&self, _mode: &str) -> RecoveryResult {
        RecoveryResult::Degraded
    }
    
    /// Bir görevi sonlandırır
    fn kill_task(&self, task_id: u64) -> RecoveryResult {
        if task_id == 0 {
            // En fazla bellek kullanan görevi bul
            crate::serial_println!("[RECOVERY] OOM: En büyük görev sonlandırılacak");
        }
        RecoveryResult::Recovered
    }
    
    /// Bellek sayfalarını serbest bırakır (OOM kurtarma)
    fn free_memory(&self, pages: usize) -> RecoveryResult {
        crate::memory::reclaim_pages_global(pages);
        RecoveryResult::Recovered
    }
    
    /// Dosya sistemini senkronize eder
    fn sync_filesystem(&self) -> RecoveryResult {
        // Acil senkronizasyon
        crate::serial_println!("[RECOVERY] Dosya sistemi senkronizasyonu denendi");
        RecoveryResult::Recovered
    }
    
    /// Acil durdurma (kurtarılamaz hata)
    fn emergency_halt(&self) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] === EMERGENCY HALT ===");
        crate::serial_println!("[RECOVERY] System halted due to unrecoverable fault");
        
        // Kesmeleri devre dışı bırak ve dur
        unsafe {
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
    }
    
    /// Sistemi yeniden başlatır
    fn reboot(&self) -> RecoveryResult {
        crate::serial_println!("[RECOVERY] Sistem yeniden başlatılması başlatıldı");
        // ACPI veya klavye denetleyicisiyle yeniden başlat
        RecoveryResult::RequiresReboot
    }
    
    /// Kurtarma motorunu etkinleştirir/devre dışı bırakır
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    
    /// Kurtarma istatistiklerini döndürür
    pub fn stats(&self) -> RecoveryStats {
        RecoveryStats {
            total_attempts: self.total_attempts.load(Ordering::SeqCst),
            successful: self.successful.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            active_count: self.active.lock().len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RecoveryStats {
    pub total_attempts: u32,
    pub successful: u32,
    pub failed: u32,
    pub active_count: usize,
}

// ============================================================================
// BAŞLAŞMA
// ============================================================================

lazy_static::lazy_static! {
    static ref RECOVERY_ENGINE: RecoveryEngine = RecoveryEngine::new();
}

pub fn init() {
    crate::serial_println!("[RECOVERY] Kurtarma motoru başlatıldı");
}

pub fn attempt_recovery(fault: &Fault) -> RecoveryResult {
    RECOVERY_ENGINE.recover(fault)
}

pub fn get_stats() -> RecoveryStats {
    RECOVERY_ENGINE.stats()
}

pub fn set_enabled(enabled: bool) {
    RECOVERY_ENGINE.set_enabled(enabled);
}
