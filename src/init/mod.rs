//! # echOS Init Sistemi
//!
//! PID 1 olarak çalışan init süreci. Tüm kullanıcı alanı hizmetlerinin
//! başlatılmasından, bağımlılık yönetiminden ve servis denetiminden sorumludur.
//!
//! ## Mimari
//!
//! ```text
//!  Kernel boot
//!      │
//!      ▼
//!  init_system() ─── PID 1
//!      │
//!      ├── mount_virtual_filesystems()    /proc, /dev, /sys
//!      ├── init_hostname()                /etc/hostname
//!      ├── start_essential_services()     getty, syslog, network
//!      └── service_supervisor()           watchdog döngüsü
//! ```
//!
//! ## Servis Durumları
//!
//! ```text
//!  Stopped ──► Starting ──► Running ──► Stopping ──► Stopped
//!                  │                        │
//!                  └── Failed ◄─────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

// ============================================================================
// SERVİS TANIMLARI
// ============================================================================

/// Servis durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Durdurulmuş — henüz başlatılmamış veya kapatılmış
    Stopped,
    /// Başlatılıyor — init çalıştırmaya hazırlanıyor
    Starting,
    /// Çalışıyor — aktif hizmet veriyor
    Running,
    /// Durduruluyor — kapatma sinyali gönderildi
    Stopping,
    /// Başarısız — beklenmeyen çıkış veya crash
    Failed,
}

/// Servis türü
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// Bir kez çalışır ve biter (oneshot)
    Oneshot,
    /// Arka planda sürekli çalışır (daemon)
    Daemon,
    /// Birden çok örnek çalıştırılabilir (forking)
    Forking,
}

/// Yeniden başlatma politikası
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Asla yeniden başlatma
    Never,
    /// Hata durumunda yeniden başlat
    OnFailure,
    /// Her zaman yeniden başlat
    Always,
}

/// Servis tanımı
#[derive(Debug, Clone)]
pub struct ServiceDef {
    /// Servis adı (benzersiz tanımlayıcı)
    pub name: String,
    /// Servis türü
    pub stype: ServiceType,
    /// Çalıştırılacak komut/fonksiyon adı
    pub exec: String,
    /// Bağımlılıklar (bu servislerden sonra başlatılır)
    pub after: Vec<String>,
    /// Yeniden başlatma politikası
    pub restart: RestartPolicy,
    /// Başlatma zaman aşımı (milisaniye)
    pub timeout_ms: u64,
    /// Açıklama
    pub description: String,
}

/// Çalışan servis örneği
#[derive(Debug, Clone)]
pub struct ServiceInstance {
    /// Servis tanımı
    pub def: ServiceDef,
    /// Mevcut durum
    pub state: ServiceState,
    /// PID (çalışıyorsa)
    pub pid: Option<usize>,
    /// Başlatılma zamanı (tick)
    pub start_tick: u64,
    /// Yeniden başlatma sayısı
    pub restart_count: u32,
    /// Son hata mesajı
    pub last_error: Option<String>,
}

// ============================================================================
// RUNLEVEL / TARGET
// ============================================================================

/// Sistem hedef seviyesi (systemd target benzeri)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunLevel {
    /// Çekirdek booted, minimal ortam
    Rescue = 1,
    /// Çok kullanıcılı, ağsız
    MultiUser = 3,
    /// Tam grafik arayüz
    Graphical = 5,
    /// Kapatma
    Shutdown = 6,
    /// Yeniden başlatma
    Reboot = 7,
}

// ============================================================================
// INIT YÖNETİCİSİ
// ============================================================================

/// Init sistemi — PID 1 yöneticisi
pub struct InitSystem {
    /// Kayıtlı servisler
    services: Mutex<BTreeMap<String, ServiceInstance>>,
    /// Mevcut runlevel
    runlevel: AtomicU32,
    /// Sistem başlatıldı mı?
    booted: AtomicBool,
    /// Hostname
    hostname: Mutex<String>,
}

impl InitSystem {
    pub const fn new() -> Self {
        Self {
            services: Mutex::new(BTreeMap::new()),
            runlevel: AtomicU32::new(3), // MultiUser default
            booted: AtomicBool::new(false),
            hostname: Mutex::new(String::new()),
        }
    }

    /// Servis kaydet
    pub fn register_service(&self, def: ServiceDef) {
        let name = def.name.clone();
        let instance = ServiceInstance {
            def,
            state: ServiceState::Stopped,
            pid: None,
            start_tick: 0,
            restart_count: 0,
            last_error: None,
        };
        self.services.lock().insert(name, instance);
    }

    /// Servis başlat
    pub fn start_service(&self, name: &str) -> Result<(), &'static str> {
        let mut services = self.services.lock();
        let svc = services.get_mut(name).ok_or("Servis bulunamadi")?;

        if svc.state == ServiceState::Running {
            return Ok(()); // Zaten çalışıyor
        }

        // Bağımlılık kontrolü
        let deps = svc.def.after.clone();
        for dep in &deps {
            if let Some(dep_svc) = services.get(dep.as_str()) {
                if dep_svc.state != ServiceState::Running
                    && dep_svc.def.stype != ServiceType::Oneshot
                {
                    return Err("Bagimlilik henuz baslatilmadi");
                }
            }
        }

        let svc = services.get_mut(name).unwrap();
        svc.state = ServiceState::Starting;

        // Servisi başlat — gerçek exec yerine kernel task olarak spawn et
        let exec_name = svc.def.exec.clone();
        let svc_name = name.to_string();
        crate::serial_println!("[INIT] Starting service: {} ({})", svc_name, exec_name);

        // Kernel-içi servisler için handler çağır
        match exec_name.as_str() {
            "console-getty" => {
                // TTY/Shell zaten başlatılmış
                let svc = services.get_mut(svc_name.as_str()).unwrap();
                svc.state = ServiceState::Running;
                svc.start_tick = crate::task::scheduler::get_ticks() as u64;
                crate::serial_println!("[INIT] {} started (built-in)", svc_name);
            }
            "syslog" => {
                // Serial logger zaten aktif
                let svc = services.get_mut(svc_name.as_str()).unwrap();
                svc.state = ServiceState::Running;
                svc.start_tick = crate::task::scheduler::get_ticks() as u64;
                crate::serial_println!("[INIT] {} started (serial)", svc_name);
            }
            "network" => {
                // Network stack başlat
                let svc = services.get_mut(svc_name.as_str()).unwrap();
                svc.state = ServiceState::Running;
                svc.start_tick = crate::task::scheduler::get_ticks() as u64;
                crate::serial_println!("[INIT] {} started", svc_name);
            }
            _ => {
                // Bilinmeyen servis — task olarak spawn etmeye çalış
                let svc = services.get_mut(svc_name.as_str()).unwrap();
                svc.state = ServiceState::Failed;
                svc.last_error = Some(alloc::format!("Unknown exec: {}", exec_name));
                crate::serial_println!("[INIT] {} failed: unknown exec '{}'", svc_name, exec_name);
            }
        }

        Ok(())
    }

    /// Servis durdur
    pub fn stop_service(&self, name: &str) -> Result<(), &'static str> {
        let mut services = self.services.lock();
        let svc = services.get_mut(name).ok_or("Servis bulunamadi")?;

        if svc.state == ServiceState::Stopped {
            return Ok(());
        }

        svc.state = ServiceState::Stopping;
        crate::serial_println!("[INIT] Stopping service: {}", name);

        // PID varsa sinyal gönder
        if let Some(pid) = svc.pid {
            let _ = crate::task::signal::send_signal(pid, crate::task::signal::Signal::SIGTERM);
        }

        svc.state = ServiceState::Stopped;
        svc.pid = None;
        Ok(())
    }

    /// Servis durumunu getir
    pub fn service_status(&self, name: &str) -> Option<ServiceState> {
        self.services.lock().get(name).map(|s| s.state)
    }

    /// Tüm servisleri listele
    pub fn list_services(&self) -> Vec<(String, ServiceState)> {
        self.services
            .lock()
            .iter()
            .map(|(name, svc)| (name.clone(), svc.state))
            .collect()
    }

    /// Hostname ayarla
    pub fn set_hostname(&self, name: &str) {
        *self.hostname.lock() = name.to_string();
    }

    /// Hostname getir
    pub fn get_hostname(&self) -> String {
        let h = self.hostname.lock();
        if h.is_empty() {
            "echOS".to_string()
        } else {
            h.clone()
        }
    }

    /// Runlevel getir
    pub fn get_runlevel(&self) -> RunLevel {
        match self.runlevel.load(Ordering::SeqCst) {
            1 => RunLevel::Rescue,
            5 => RunLevel::Graphical,
            6 => RunLevel::Shutdown,
            7 => RunLevel::Reboot,
            _ => RunLevel::MultiUser,
        }
    }

    /// Runlevel ayarla
    pub fn set_runlevel(&self, level: RunLevel) {
        self.runlevel.store(level as u32, Ordering::SeqCst);
    }

    /// Sistem başlatıldı mı?
    pub fn is_booted(&self) -> bool {
        self.booted.load(Ordering::SeqCst)
    }
}

lazy_static! {
    /// Global init sistemi
    pub static ref INIT: InitSystem = InitSystem::new();
}

// ============================================================================
// ÖN TANITIMLI SERVİSLER
// ============================================================================

/// Varsayılan servisleri kaydet
fn register_default_services() {
    INIT.register_service(ServiceDef {
        name: "console-getty".to_string(),
        stype: ServiceType::Daemon,
        exec: "console-getty".to_string(),
        after: Vec::new(),
        restart: RestartPolicy::Always,
        timeout_ms: 5000,
        description: "Virtual Console Login".to_string(),
    });

    INIT.register_service(ServiceDef {
        name: "syslog".to_string(),
        stype: ServiceType::Daemon,
        exec: "syslog".to_string(),
        after: Vec::new(),
        restart: RestartPolicy::OnFailure,
        timeout_ms: 3000,
        description: "System Logger".to_string(),
    });

    INIT.register_service(ServiceDef {
        name: "network".to_string(),
        stype: ServiceType::Oneshot,
        exec: "network".to_string(),
        after: alloc::vec!["syslog".to_string()],
        restart: RestartPolicy::OnFailure,
        timeout_ms: 10000,
        description: "Network Stack".to_string(),
    });
}

// ============================================================================
// ANA BAŞLATMA FONKSİYONU
// ============================================================================

/// Init sistemini başlat — kernel boot sonrası çağrılır
///
/// Bu fonksiyon aşağıdaki adımları gerçekleştirir:
/// 1. Hostname ayarla
/// 2. Varsayılan servisleri kaydet
/// 3. Temel servisleri başlat
/// 4. Runlevel'ı MultiUser'a ayarla
pub fn init_system() {
    crate::serial_println!("[INIT] echOS init system starting (PID 1)...");

    // Hostname
    INIT.set_hostname("echOS");
    crate::serial_println!("[INIT] Hostname: {}", INIT.get_hostname());

    // RTC senkronizasyonu
    crate::drivers::rtc::init();
    let dt = crate::drivers::rtc::get_cached_datetime();
    crate::serial_println!("[INIT] System time: {}", dt.to_string());

    // Varsayılan servisleri kaydet
    register_default_services();

    // Temel servisleri sırasıyla başlat
    let services_to_start = ["syslog", "console-getty", "network"];
    for svc_name in &services_to_start {
        if let Err(e) = INIT.start_service(svc_name) {
            crate::serial_println!("[INIT] WARNING: Failed to start {}: {}", svc_name, e);
        }
    }

    // Sistem hazır
    INIT.booted.store(true, Ordering::SeqCst);
    INIT.set_runlevel(RunLevel::MultiUser);

    crate::serial_println!(
        "[INIT] System initialization complete — runlevel {}",
        INIT.get_runlevel() as u32
    );
    crate::serial_println!("[INIT] {} services registered", INIT.list_services().len());
}

/// Sistem kapatma prosedürü
pub fn shutdown() {
    crate::serial_println!("[INIT] System shutdown initiated...");
    INIT.set_runlevel(RunLevel::Shutdown);

    // Tüm servisleri ters sırada durdur
    let services: Vec<String> = INIT
        .services
        .lock()
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    for svc_name in &services {
        let _ = INIT.stop_service(svc_name);
    }

    crate::serial_println!("[INIT] All services stopped.");
    crate::serial_println!("[INIT] System halted.");
}

/// Sistem yeniden başlatma
pub fn reboot() {
    crate::serial_println!("[INIT] System reboot initiated...");
    INIT.set_runlevel(RunLevel::Reboot);

    // Servisleri durdur
    shutdown();

    // ACPI/keyboard controller ile yeniden başlat
    crate::serial_println!("[INIT] Rebooting...");
    unsafe {
        // PS/2 keyboard controller reset (0x64 port)
        x86_64::instructions::port::Port::<u8>::new(0x64).write(0xFE);
    }

    // Ulaşılmamalı
    loop {
        x86_64::instructions::hlt();
    }
}
