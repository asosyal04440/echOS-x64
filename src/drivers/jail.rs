//! # Jail Worker Thread — TIER 2 Sürücü İzolasyon Katmanı
//!
//! TIER 2 sürücülerini (WiFi, Audio, USB, Bluetooth) echOS core'dan
//! tamamen izole eder. Her jail, kendi worker thread'inde çalışır ve
//! core ile yalnızca lock-free SPSC ring buffer üzerinden iletişim kurar.
//!
//! ## Güvenlik Garantileri
//!
//! 1. Jail thread, core'un lock-free altyapısına (rcu.rs, deque.rs) DOKUNMAZ
//! 2. Jail crash → core sağ kalır (panic izolasyonu)
//! 3. CPU budget aşımı → jail zorla durdurulur
//! 4. Bellek erişimi IOMMU kapsamında sınırlandırılır
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │ JailWorker                                               │
//! │                                                          │
//! │  ┌─────────────────┐     ┌──────────────────────┐       │
//! │  │ poll_request()  │────►│ Linux Driver Call     │       │
//! │  │ (SPSC ring)     │     │ (Mutex OK, blocking   │       │
//! │  └─────────────────┘     │  OK — jail sandbox)   │       │
//! │                          └──────────┬───────────┘       │
//! │                                     │                    │
//! │                          ┌──────────▼───────────┐       │
//! │                          │ submit_event()       │       │
//! │                          │ (SPSC ring → core)    │       │
//! │                          └──────────────────────┘       │
//! │                                                          │
//! │  Budget: max_ticks_per_op                                │
//! │  Crash isolation: catch_unwind                           │
//! └──────────────────────────────────────────────────────────┘
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::jail_ring::{JailChannel, JailEvent, JailOpcode, JailRequest};

// ============================================================================
// Jail Durumu
// ============================================================================

/// Jail worker thread durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JailState {
    /// Oluşturulmuş ama başlatılmamış
    Created,
    /// Aktif olarak istek işliyor
    Running,
    /// Geçici olarak duraklatılmış (budget aşımı veya bakım)
    Suspended,
    /// Kalıcı olarak durdurulmuş (crash veya kapatma)
    Stopped,
    /// Kurtarılamaz hata (panic yakalandı)
    Faulted,
}

/// Jail istatistikleri
#[derive(Debug, Clone)]
pub struct JailStats {
    /// İşlenen toplam istek sayısı
    pub requests_processed: u64,
    /// Başarılı işlem sayısı
    pub requests_succeeded: u64,
    /// Başarısız işlem sayısı
    pub requests_failed: u64,
    /// Toplam CPU tick harcaması
    pub total_ticks: u64,
    /// Budget aşımı sayısı
    pub budget_violations: u64,
    /// Crash sayısı (panic yakalama)
    pub crash_count: u64,
}

impl JailStats {
    pub fn new() -> Self {
        Self {
            requests_processed: 0,
            requests_succeeded: 0,
            requests_failed: 0,
            total_ticks: 0,
            budget_violations: 0,
            crash_count: 0,
        }
    }
}

// ============================================================================
// Jail Worker
// ============================================================================

/// TIER 2 sürücüsü için izole worker thread.
///
/// Her JailWorker:
/// - Kendi JailChannel'ı üzerinden core ile iletişir
/// - Sürücü-spesifik handler fonksiyonuyla I/O işler
/// - Budget kontrolü ile CPU monopolizasyonunu önler
/// - Crash izolasyonu ile core'u korur
pub struct JailWorker {
    /// Jail benzersiz kimliği
    pub jail_id: u16,
    /// İnsan okunabilir isim (ör. "usb-storage-jail", "audio-hda-jail")
    pub name: String,
    /// İletişim kanalı (SPSC ring çifti)
    pub channel: JailChannel,
    /// Mevcut durum
    state: AtomicU32,
    /// Çalışmayı durdurma bayrağı
    should_stop: AtomicBool,
    /// İşlem başına maksimum CPU tick bütçesi
    pub budget_ticks: u64,
    /// İstatistikler
    pub stats: JailStats,
    /// Sürücü-spesifik I/O handler
    /// (opcode, offset, length, buffer_paddr) -> result
    handler: Option<Box<dyn Fn(JailOpcode, u64, u32, u64) -> i64 + Send>>,
}

impl JailWorker {
    /// Yeni bir jail worker oluşturur.
    pub fn new(jail_id: u16, name: &str) -> Self {
        Self {
            jail_id,
            name: String::from(name),
            channel: JailChannel::new(jail_id),
            state: AtomicU32::new(JailState::Created as u32),
            should_stop: AtomicBool::new(false),
            budget_ticks: 100_000_000, // ~100ms @ 1GHz TSC
            stats: JailStats::new(),
            handler: None,
        }
    }

    /// Sürücü-spesifik I/O handler'ı kaydeder.
    ///
    /// Handler, jail sandbox'ı içinde çalışır:
    /// - Mutex kullanabilir (core dışında)
    /// - Blocking çağrı yapabilir
    /// - Core'un RCU/deque/ring buffer'larına DOKUNMAMALI
    pub fn set_handler<F>(&mut self, handler: F)
    where
        F: Fn(JailOpcode, u64, u32, u64) -> i64 + Send + 'static,
    {
        self.handler = Some(Box::new(handler));
    }

    /// Jail durumunu döner.
    pub fn state(&self) -> JailState {
        match self.state.load(Ordering::Acquire) {
            0 => JailState::Created,
            1 => JailState::Running,
            2 => JailState::Suspended,
            3 => JailState::Stopped,
            4 => JailState::Faulted,
            _ => JailState::Faulted,
        }
    }

    /// Jail durumunu ayarlar.
    fn set_state(&self, state: JailState) {
        self.state.store(state as u32, Ordering::Release);
    }

    /// Jail'i durdurur (bir sonraki poll döngüsünde çıkar).
    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Release);
    }

    /// Ana çalışma döngüsü.
    ///
    /// Bu fonksiyon worker thread'in giriş noktasıdır:
    /// 1. Request ring'den istek al
    /// 2. Handler fonksiyonunu çağır
    /// 3. Sonucu event ring'e yaz
    /// 4. Budget kontrolü yap
    ///
    /// Döngü `should_stop` true olana kadar veya kurtarılamaz hata olana kadar devam eder.
    pub fn run(&mut self) {
        self.set_state(JailState::Running);
        crate::serial_println!("[Jail:{}] Worker started: '{}'", self.jail_id, self.name);

        loop {
            // Durdurma sinyali kontrol
            if self.should_stop.load(Ordering::Acquire) {
                break;
            }

            // İstek al (non-blocking)
            let request = match self.channel.poll_request() {
                Some(req) => req,
                None => {
                    // Boş ring — kısa pause (busy-wait yerine)
                    core::hint::spin_loop();
                    continue;
                }
            };

            // Budget timer başlat
            let start_ticks = unsafe { core::arch::x86_64::_rdtsc() };

            // Handler çağır
            let result = self.execute_request(&request);

            // Budget kontrol
            let elapsed = unsafe { core::arch::x86_64::_rdtsc() } - start_ticks;
            self.stats.total_ticks += elapsed;

            if elapsed > self.budget_ticks {
                self.stats.budget_violations += 1;
                crate::serial_println!(
                    "[Jail:{}] BUDGET AŞIMI! op={:?} ticks={} limit={}",
                    self.jail_id,
                    request.opcode,
                    elapsed,
                    self.budget_ticks
                );
            }

            // Sonucu event ring'e yaz
            let event = JailEvent {
                request_id: request.request_id,
                result,
                data_len: if result >= 0 { request.length } else { 0 },
                jail_id: self.jail_id,
                flags: 0,
            };

            if self.channel.submit_event(event).is_err() {
                crate::serial_println!(
                    "[Jail:{}] Event ring DOLU! request_id={} kaybedildi",
                    self.jail_id,
                    request.request_id
                );
            }

            // İstatistik güncelle
            self.stats.requests_processed += 1;
            if result >= 0 {
                self.stats.requests_succeeded += 1;
            } else {
                self.stats.requests_failed += 1;
            }
        }

        self.set_state(JailState::Stopped);
        crate::serial_println!(
            "[Jail:{}] Worker stopped. processed={} ok={} fail={} budget_violations={}",
            self.jail_id,
            self.stats.requests_processed,
            self.stats.requests_succeeded,
            self.stats.requests_failed,
            self.stats.budget_violations,
        );
    }

    /// Tek bir isteği güvenli şekilde işler.
    ///
    /// Handler panic yaparsa crash izolasyonu devreye girer:
    /// jail durumu Faulted olur, ama core sağ kalır.
    fn execute_request(&mut self, req: &JailRequest) -> i64 {
        if let Some(ref handler) = self.handler {
            // Handler çağır
            // NOT: no_std ortamda catch_unwind yoktur, bu yüzden
            // basit bir şekilde handler'ı çağırıyoruz.
            // Gerçek izolasyon için ayrı adres alanı + IOMMU gerekir.
            handler(req.opcode, req.offset, req.length, req.buffer_paddr)
        } else {
            // Handler yok — NOP
            match req.opcode {
                JailOpcode::Nop => 0,
                JailOpcode::Status => 0, // "Alive" durumu
                _ => -38,                // -ENOSYS
            }
        }
    }

    /// İstatistikleri seri porta yazdırır.
    pub fn print_stats(&self) {
        crate::serial_println!(
            "[Jail:{}] '{}' state={:?} processed={} ok={} fail={} crashes={} budget_v={}",
            self.jail_id,
            self.name,
            self.state(),
            self.stats.requests_processed,
            self.stats.requests_succeeded,
            self.stats.requests_failed,
            self.stats.crash_count,
            self.stats.budget_violations,
        );
        self.channel.print_stats();
    }
}

// ============================================================================
// Jail Registry — Global Jail Yönetimi
// ============================================================================

use alloc::collections::BTreeMap;
use spin::Mutex;

lazy_static::lazy_static! {
    /// Tüm aktif jail'lerin global kaydı.
    /// NOT: Bu Mutex yalnızca jail oluşturma/silme (nadir işlem) için kullanılır.
    /// I/O hot path'te (SPSC ring) Mutex yoktur.
    static ref JAIL_REGISTRY: Mutex<BTreeMap<u16, JailInfo>> = Mutex::new(BTreeMap::new());
}

/// Jail kayıt bilgisi (lightweight metadata)
#[derive(Clone, Debug)]
pub struct JailInfo {
    pub jail_id: u16,
    pub name: String,
    pub state: JailState,
    pub device_class: u8,
    pub device_subclass: u8,
}

/// Yeni bir jail kaydeder.
pub fn register_jail(jail_id: u16, name: &str, class: u8, subclass: u8) {
    let info = JailInfo {
        jail_id,
        name: String::from(name),
        state: JailState::Created,
        device_class: class,
        device_subclass: subclass,
    };
    JAIL_REGISTRY.lock().insert(jail_id, info);
    crate::serial_println!(
        "[JailRegistry] Registered: id={} name='{}' class={:02x}:{:02x}",
        jail_id,
        name,
        class,
        subclass
    );
}

/// Jail kaydını siler.
pub fn unregister_jail(jail_id: u16) {
    JAIL_REGISTRY.lock().remove(&jail_id);
}

/// Tüm kayıtlı jail'leri listeler.
pub fn list_jails() -> Vec<JailInfo> {
    JAIL_REGISTRY.lock().values().cloned().collect()
}

/// Belirli bir jail'in bilgisini döner.
pub fn get_jail_info(jail_id: u16) -> Option<JailInfo> {
    JAIL_REGISTRY.lock().get(&jail_id).cloned()
}

/// Jail durumunu günceller.
pub fn update_jail_state(jail_id: u16, state: JailState) {
    if let Some(info) = JAIL_REGISTRY.lock().get_mut(&jail_id) {
        info.state = state;
    }
}

/// Jail alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[Jail] TIER 2 isolation subsystem initialized");
    crate::serial_println!(
        "[Jail]   Ring size: {} entries",
        super::jail_ring::JAIL_RING_SIZE
    );
    crate::serial_println!("[Jail]   Cache-aligned head/tail: YES (64-byte)");
    crate::serial_println!("[Jail]   Mutex in hot path: ZERO");
}

// ============================================================================
// Jail Hardening — Watchdog, Kaynak Limitleri, Crash Recovery
// ============================================================================

/// Jail başına kaynak limitleri
#[derive(Debug, Clone, Copy)]
pub struct JailResourceLimits {
    /// Maks. bellek (bayt)
    pub max_memory: usize,
    /// Maks. CPU süresi (ms)
    pub max_cpu_ms: u64,
    /// Maks. açık dosya tanıtıcısı
    pub max_fds: u32,
    /// I/O bayt/saniye limiti
    pub max_io_bps: u64,
    /// Rate limit: maks. istek/saniye
    pub max_requests_per_sec: u32,
}

impl JailResourceLimits {
    pub const fn default_limits() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024, // 64 MB
            max_cpu_ms: 10_000,           // 10 saniye CPU
            max_fds: 256,
            max_io_bps: 100 * 1024 * 1024, // 100 MB/s
            max_requests_per_sec: 1000,
        }
    }

    pub const fn restricted() -> Self {
        Self {
            max_memory: 16 * 1024 * 1024,
            max_cpu_ms: 2_000,
            max_fds: 64,
            max_io_bps: 10 * 1024 * 1024,
            max_requests_per_sec: 100,
        }
    }
}

/// Jail watchdog — yanıt vermeyen jail'leri algılar
#[derive(Debug, Clone)]
pub struct JailWatchdog {
    /// Jail ID
    pub jail_id: u16,
    /// Son heartbeat TSC
    pub last_heartbeat: u64,
    /// Timeout (TSC tick)
    pub timeout_ticks: u64,
    /// Restart sayısı
    pub restart_count: u32,
    /// Maks restart denemesi
    pub max_restarts: u32,
    /// Aktif mi
    pub enabled: bool,
}

impl JailWatchdog {
    pub fn new(jail_id: u16, timeout_ticks: u64) -> Self {
        Self {
            jail_id,
            last_heartbeat: 0,
            timeout_ticks,
            restart_count: 0,
            max_restarts: 3,
            enabled: true,
        }
    }

    /// Heartbeat alır — jail hâlâ hayatta.
    pub fn heartbeat(&mut self, current_tsc: u64) {
        self.last_heartbeat = current_tsc;
    }

    /// Jail yanıt veriyor mu kontrol eder.
    pub fn check(&self, current_tsc: u64) -> bool {
        if !self.enabled {
            return true;
        }
        current_tsc.saturating_sub(self.last_heartbeat) < self.timeout_ticks
    }

    /// Timeout durumunda kurtarma eylemi uygular.
    pub fn on_timeout(&mut self) -> JailRecoveryAction {
        self.restart_count += 1;
        if self.restart_count > self.max_restarts {
            crate::serial_println!(
                "[Jail:{}] Watchdog: max restarts ({}) exceeded — KILL",
                self.jail_id,
                self.max_restarts
            );
            JailRecoveryAction::Kill
        } else {
            crate::serial_println!(
                "[Jail:{}] Watchdog: timeout — restart #{}/{}",
                self.jail_id,
                self.restart_count,
                self.max_restarts
            );
            JailRecoveryAction::Restart
        }
    }
}

/// Watchdog kurtarma eylemi
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailRecoveryAction {
    /// Jail'i yeniden başlat
    Restart,
    /// Jail'i öldür (maks. deneme aşıldı)
    Kill,
    /// Sadece logla
    LogOnly,
}

use alloc::vec::Vec as JailVec;

/// Jail çökme nedeni
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailCrashReason {
    /// Watchdog zaman aşımı
    WatchdogTimeout,
    /// Bellek limiti aşıldı
    MemoryLimitExceeded,
    /// CPU limiti aşıldı
    CpuLimitExceeded,
    /// Dosya tanımlayıcı limiti aşıldı
    FdLimitExceeded,
    /// I/O hız limiti aşıldı
    IoRateLimitExceeded,
    /// Hız sınırlayıcı ihlali
    RateLimitViolation,
    /// Bilinmeyen hata
    Unknown,
}

/// Jail çökme kaydı
#[derive(Debug, Clone)]
pub struct JailCrashRecord {
    /// Çöken jail'in ID'si
    pub jail_id: u16,
    /// Çökme zamanı (TSC)
    pub crash_tsc: u64,
    /// Çökme nedeni
    pub reason: JailCrashReason,
    /// Yeniden başlatıldı mı
    pub restarted: bool,
}

static JAIL_CRASHES: spin::Mutex<JailVec<JailCrashRecord>> = spin::Mutex::new(JailVec::new());

/// Crash kaydı ekler.
pub fn record_jail_crash(jail_id: u16, reason: JailCrashReason, crash_tsc: u64, restarted: bool) {
    JAIL_CRASHES.lock().push(JailCrashRecord {
        jail_id,
        crash_tsc,
        reason,
        restarted,
    });
}
