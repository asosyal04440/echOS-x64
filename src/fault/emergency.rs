//! # Acil Durum Modu (Emergency Mode)
//!
//! Acil durum kapatma ve minimal işlem modu.
//! Sistem kritik bir hatayla karşılaştığında veri kaybını önlemek
//! için güvenli kapatma (safe halt) mekanizması sağlar.

use core::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool, Ordering};

// ============================================================================
// ACİL DURUM DURUMU
// ============================================================================

/// Acil durum modu durum bilgisi
pub struct EmergencyState {
    /// Acil durum modu aktif mi?
    active: AtomicBool,
    /// Acil durum nedeni (metin açıklaması)
    reason: spin::Mutex<Option<alloc::string::String>>,
    /// Acil durum başlangıç zamanı (tick)
    start_time: AtomicUsize,
    /// Kaçıncı acil durum olayı (toplam sayı)
    count: AtomicU64,
    /// Kurtarma denensin mi?
    attempt_recovery: AtomicBool,
}

impl EmergencyState {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            reason: spin::Mutex::new(None),
            start_time: AtomicUsize::new(0),
            count: AtomicU64::new(0),
            attempt_recovery: AtomicBool::new(true),
        }
    }
    
    /// Acil durum moduna girer: dosya sistemlerini senkronize eder ve modülleri devre dışı bırakır
    pub fn enter(&self, reason: &str) {
        if self.active.swap(true, Ordering::SeqCst) {
            return; // Zaten acil durum modunda
        }
        
        self.count.fetch_add(1, Ordering::SeqCst);
        self.start_time.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        *self.reason.lock() = Some(alloc::string::String::from(reason));
        
        crate::serial_println!("[EMERGENCY] === ENTERING EMERGENCY MODE ===");
        crate::serial_println!("[EMERGENCY] Reason: {}", reason);
        
        // Kritik olmayan modülleri devre dışı bırak
        crate::fault::degradation::set_level(crate::fault::severity::RecoveryLevel::Level4);
        
        // Dosya sistemlerini senkronize et
        crate::serial_println!("[EMERGENCY] Dosya sistemleri senkronize ediliyor...");
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // Sistem durumunu günlüklere yaz
        self.log_state();
    }
    
    /// Acil durum modundan çıkar ve normal çalışmaya döner
    pub fn exit(&self) {
        if !self.active.swap(false, Ordering::SeqCst) {
            return; // Zaten acil durum modunda değil
        }
        
        crate::serial_println!("[EMERGENCY] === EXITING EMERGENCY MODE ===");
        
        *self.reason.lock() = None;
        
        // Normal çalışmayı geri yükle
        crate::fault::degradation::set_level(crate::fault::severity::RecoveryLevel::Level0);
    }
    
    /// Mevcut sistem durumunu seri porta yazar (teşhis amaçlı)
    fn log_state(&self) {
        // Bellek durumu
        if let Some(mm) = crate::memory::global_memory_manager() {
            let mm: &crate::memory::MemoryManager = mm;
            let free = mm.free_frames();
            let total = mm.total_frames();
            crate::serial_println!(
                "[EMERGENCY] Memory: {} / {} frames free",
                free,
                total
            );
        }
        
        // CPU durumu
        crate::serial_println!(
            "[EMERGENCY] CPU'lar: {} çevrimici",
            crate::cpu::smp::online_cpu_count()
        );
        
        // Hata istatistikleri
        let stats = crate::fault::get_stats();
        crate::serial_println!(
            "[EMERGENCY] Faults: {} total, {} recoveries, level {}",
            stats.total_faults,
            stats.total_recoveries,
            stats.recovery_level
        );
        
        // Zamanlayıcı durumu
        let sched_stats = crate::task::scheduler::get_stats();
        crate::serial_println!(
            "[EMERGENCY] Tasks: {} total, {} running, {} zombies",
            sched_stats.total_tasks,
            sched_stats.running_tasks,
            sched_stats.zombie_count
        );
    }
    
    /// Acil durum modunun aktif olup olmadığını kontrol eder
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
    
    /// Acil durum nedenini döndürür
    pub fn reason(&self) -> Option<alloc::string::String> {
        self.reason.lock().clone()
    }
    
    /// Acil durum süresini tick cinsinden döndürür
    pub fn duration(&self) -> usize {
        if !self.active.load(Ordering::SeqCst) {
            return 0;
        }
        
        crate::task::scheduler::get_ticks().saturating_sub(
            self.start_time.load(Ordering::SeqCst)
        )
    }
    
    /// Güvenli durdurma — veriyi koruyarak sistemi durdurur
    pub fn safe_halt(&self) -> ! {
        crate::serial_println!("[EMERGENCY] === SAFE HALT ===");
        crate::serial_println!("[EMERGENCY] System is halting safely");
        
        // Son senkronizasyon
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // Kesmeleri devre dışı bırak ve dur
        unsafe {
            x86_64::instructions::interrupts::disable();
            loop {
                x86_64::instructions::hlt();
            }
        }
    }
    
    /// Acil durum yeniden başlatması (reboot)
    pub fn reboot(&self) -> ! {
        crate::serial_println!("[EMERGENCY] === EMERGENCY REBOOT ===");
        
        // Son senkronizasyon
        crate::fault::recovery_modules::fs::emergency_sync();
        
        // ACPI sıfırlamayı dene
        crate::serial_println!("[EMERGENCY] ACPI sıfırlama deneniyor...");
        
        // ACPI başarısız olursa klavye denetleyicisini dene
        // unsafe { ... }
        
        // Hiçbiri işe yaramazsa triple fault ile zorla sıfırla
        crate::serial_println!("[EMERGENCY] Triple fault ile sıfırlama zorlanıyor");
        
        unsafe {
            // Geçersiz IDT yükle ve kesme tetikle
            core::arch::asm!(
                "lidt [{0}]",
                "int 3",
                in(reg) &0u64 as *const u64,
                options(noreturn)
            );
        }
    }
}

// ============================================================================
// GLOBAL ÖRNEK
// ============================================================================

lazy_static::lazy_static! {
    pub static ref EMERGENCY_STATE: EmergencyState = EmergencyState::new();
}

// ============================================================================
// GENEL (PUBLIC) API
// ============================================================================

pub fn enter() {
    EMERGENCY_STATE.enter("System triggered emergency mode");
}

pub fn enter_with_reason(reason: &str) {
    EMERGENCY_STATE.enter(reason);
}

pub fn exit() {
    EMERGENCY_STATE.exit();
}

pub fn is_active() -> bool {
    EMERGENCY_STATE.is_active()
}

pub fn reason() -> Option<alloc::string::String> {
    EMERGENCY_STATE.reason()
}

pub fn duration() -> usize {
    EMERGENCY_STATE.duration()
}

pub fn safe_halt() -> ! {
    EMERGENCY_STATE.safe_halt()
}

pub fn reboot() -> ! {
    EMERGENCY_STATE.reboot()
}
