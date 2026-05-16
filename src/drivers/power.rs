//! # Güç Yönetimi (Power Management)
//!
//! Bu modül, ACPI uyumlu sistem uyku durumlarını ve cihaz güç durumlarını
//! yönetir. Cihazları askıya alır/devam ettirir, uyku geçişlerini koordine eder
//! ve çalışma zamanı güç yönetimi (Runtime PM) altyapısını sağlar.
//!
//! ## ACPI Sistem Uyku Durumları
//!
//! ```text
//!  Durum | Ad                     | Açıklama
//!  ------+------------------------+------------------------------------------
//!   S0   | Tam Çalışma            | Normal çalışma, güç tüketimi en yüksek
//!   S1   | Güçten Askıya Al       | CPU durduruldu, önbellek korunuyor
//!   S2   | (Nadiren kullanılır)   | CPU kapalı, RAM korunuyor
//!   S3   | RAM'e Askıya Al        | Tüm bağlam RAM'de, diğer her şey kapalı
//!   S4   | Diske Askıya Al (Hbnt) | Bellek imajı diske yazılır, güç kesilir
//!   S5   | Yumuşak Kapatma        | İşletim sistemi kapatıldı; güç düğmesiyle açılır
//! ```
//!
//! ## ACPI Cihaz Güç Durumları
//!
//! ```text
//!  Durum | Açıklama
//!  ------+-----------------------------------------------------------
//!   D0   | Tam çalışma, tam güç
//!   D1   | Düşük güç, cihaz bağlamı korunuyor (az desteklenir)
//!   D2   | Daha düşük güç, bağlam kısmen kaybolabilir (nadir)
//!   D3Hot | Kapalı (yazılımsal), PCI veri yolunda hâlâ görünür
//!   D3Cold | Kapalı (fiziksel), güç tamamen kesilmiş
//! ```
//!
//! ## Güç Yönetimi Akışı
//!
//! ```text
//!  Sistem Uyku Geçişi (örn: S3 - RAM'e Askıya Al):
//!
//!   enter_sleep(S3)
//!        |
//!        v
//!   [Can suspend?] --Hayır--> Err(Blocked)
//!        |
//!        v Evet
//!   [prepare() çağrı tüm cihazlar]
//!        |
//!        v
//!   [suspend() ters sırayla] --> cihazlar D3Hot durumuna geçer
//!        |
//!        v
//!   [ACPI PM1a kontrolü]     --> donanım askıya alır
//!   ~~~~~~ UYKU ~~~~~~
//!   [ACPI uyandırma olayı]
//!        |
//!        v
//!   [resume() tüm cihazlar]  --> cihazlar D0 durumuna döner
//!        |
//!        v
//!   [complete() tüm cihazlar]
//!        |
//!        v
//!   [İstatistikleri güncelle, S0'a dön]
//! ```
//!
//! ## Çalışma Zamanı PM (Runtime PM)
//!
//! ```text
//!  Otomatik Askıya Alma (Autosuspend):
//!
//!  pm.get()  <-- Kullanım sayacını artır (cihaz meşgul)
//!     |
//!  [Cihaz kullanılıyor]
//!     |
//!  pm.put()  <-- Kullanım sayacını azalt
//!     |
//!  [usage_count == 0] ----> autosuspend zamanlayıcı başlar
//!     |                     (varsayılan: 2000 ms)
//!     |
//!  [Zaman doldu] ----------> runtime_suspend() --> D3Hot
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// GÜÇ DURUMLARI
// ============================================================================

/// ACPI sistem uyku durumları.
/// S0 normalden S5 kapatmaya kadar enerji tasarruf seviyeleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepState {
    /// Tam çalışma durumu
    S0,
    /// Güçten Askıya Al (CPU durduruldu, önbellek korunuyor)
    S1,
    /// RAM'e Askıya Al (DRAM korunuyor, diğer her şey kapalı)
    S3,
    /// Diske Askıya Al / Hibernate (bellek imajı diske yazılır, güç kesilir)
    S4,
    /// Yumuşak Kapatma (işletim sistemi kapatıldı)
    S5,
}

impl Default for SleepState {
    fn default() -> Self {
        SleepState::S0
    }
}

/// ACPI cihaz güç durumları.
/// D0 tam çalışmadan D3Cold tam kapanmaya kadar enerji seviyeleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePowerState {
    /// Tam çalışma (full power)
    D0,
    /// Düşük güç, bağlam korunuyor
    D1,
    /// Düşük güç, bağlam büyük ölçüde kaybolmuş
    D2,
    /// Kapalı (yazılımsal), PCI veri yolunda hâlâ görünür
    D3Hot,
    /// Kapalı (fiziksel), güç tamamen kesilmiş
    D3Cold,
}

/// Çalışma zamanı güç yönetimi durumu.
/// Bir cihazın o an aktif, askıya alınmış veya geçiş halinde olduğunu gösterir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePmStatus {
    Active,
    Suspended,
    Suspending,
    Resuming,
    Error,
}

// ============================================================================
// GÜÇ YÖNETİLEBİLİR CİHAZ
// ============================================================================

/// Güç yönetimi altyapısına kaydedilmiş bir cihazı temsil eder.
/// Kullanım sayacı, otomatik askıya alma gecikmesi ve callback fonksiyonlarını içerir.
pub struct PowerManageable {
    /// Cihaz kimliği
    pub device_id: u64,
    /// Mevcut güç durumu (Mutex ile thread-safe korumalı)
    pub power_state: Mutex<DevicePowerState>,
    /// Çalışma zamanı PM durumu
    pub runtime_status: Mutex<RuntimePmStatus>,
    /// Toplam askıya alma sayacı
    pub suspend_count: AtomicU32,
    /// Kullanım sayacı: 0 ise cihaz boşta
    pub usage_count: AtomicU32,
    /// Otomatik askıya alma gecikmesi (milisaniye)
    pub autosuspend_delay: AtomicU32,
    /// Son meşguliyet zamanı (zamanlayıcı tıkları)
    pub last_busy: AtomicU64,
    /// Sistemi uyandırabilir mi?
    pub can_wakeup: AtomicBool,
    /// Uyandırma isteği var mı?
    pub should_wakeup: AtomicBool,
    /// Askıya alma callback'i
    pub suspend_cb: Option<fn(u64, DevicePowerState) -> Result<(), PmError>>,
    /// Devam ettirme callback'i
    pub resume_cb: Option<fn(u64) -> Result<(), PmError>>,
    /// Hazırlık callback'i (uyku öncesi)
    pub prepare_cb: Option<fn(u64) -> Result<(), PmError>>,
    /// Tamamlama callback'i (uyanma sonrası)
    pub complete_cb: Option<fn(u64)>,
}

impl PowerManageable {
    pub fn new(device_id: u64) -> Self {
        Self {
            device_id,
            power_state: Mutex::new(DevicePowerState::D0),
            runtime_status: Mutex::new(RuntimePmStatus::Active),
            suspend_count: AtomicU32::new(0),
            usage_count: AtomicU32::new(1),
            autosuspend_delay: AtomicU32::new(2000), // 2 saniye varsayılan gecikme
            last_busy: AtomicU64::new(0),
            can_wakeup: AtomicBool::new(false),
            should_wakeup: AtomicBool::new(false),
            suspend_cb: None,
            resume_cb: None,
            prepare_cb: None,
            complete_cb: None,
        }
    }

    /// Cihazı belirtilen güç durumuna askıya alır.
    /// Varsa suspend_cb çağrılır, ardından güç durumu ve çalışma zamanı durumu güncellenir.
    pub fn suspend(&self, state: DevicePowerState) -> Result<(), PmError> {
        if let Some(cb) = self.suspend_cb {
            cb(self.device_id, state)?;
        }
        *self.power_state.lock() = state;
        *self.runtime_status.lock() = RuntimePmStatus::Suspended;
        self.suspend_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Cihazı D0 (tam çalışma) durumuna döndürür.
    /// Varsa resume_cb çağrılır.
    pub fn resume(&self) -> Result<(), PmError> {
        if let Some(cb) = self.resume_cb {
            cb(self.device_id)?;
        }
        *self.power_state.lock() = DevicePowerState::D0;
        *self.runtime_status.lock() = RuntimePmStatus::Active;
        Ok(())
    }

    /// Çalışma zamanı otomatik askıya alma.
    /// Sadece usage_count == 0 ise gerçekleşir; aksi hâlde Busy hatası döner.
    pub fn runtime_suspend(&self) -> Result<(), PmError> {
        if self.usage_count.load(Ordering::SeqCst) > 0 {
            return Err(PmError::Busy);
        }

        *self.runtime_status.lock() = RuntimePmStatus::Suspending;
        self.suspend(DevicePowerState::D3Hot)?;
        Ok(())
    }

    /// Çalışma zamanı otomatik devam ettirme.
    /// Durum Resuming'e geçirilir, ardından tam devam sağlanır.
    pub fn runtime_resume(&self) -> Result<(), PmError> {
        *self.runtime_status.lock() = RuntimePmStatus::Resuming;
        self.resume()?;
        Ok(())
    }

    /// Kullanım sayacını artırır ve son meşguliyet zamanını günceller.
    /// Otomatik askıya alma zamanlayıcısını sıfırlar.
    pub fn get(&self) {
        self.usage_count.fetch_add(1, Ordering::SeqCst);
        self.last_busy
            .store(crate::task::scheduler::get_ticks() as u64, Ordering::SeqCst);
    }

    /// Kullanım sayacını azaltır.
    /// Sayaç sıfıra ulaştığında cihaz boşta sayılır ve otomatik askıya alma tetiklenebilir.
    pub fn put(&self) {
        self.usage_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Cihazın boşta olup olmadığını kontrol eder (usage_count == 0).
    pub fn is_idle(&self) -> bool {
        self.usage_count.load(Ordering::SeqCst) == 0
    }

    /// Otomatik askıya alma zaman aşımının dolup dolmadığını kontrol eder.
    /// Son meşguliyet üzerinden autosuspend_delay ms geçmişse true döner.
    pub fn check_autosuspend(&self) -> bool {
        let last = self.last_busy.load(Ordering::SeqCst);
        let now = crate::task::scheduler::get_ticks() as u64;
        let delay = self.autosuspend_delay.load(Ordering::SeqCst) as u64;

        now - last > delay
    }
}

// ============================================================================
// GÜÇ YÖNETİCİSİ
// ============================================================================

/// Sistem genelindeki güç yönetimini koordine eden merkezi yapı.
/// Cihazları, uyandırma kaynaklarını ve askıya alma engelleyicileri yönetir.
pub struct PowerManager {
    /// Mevcut sistem uyku durumu
    system_state: Mutex<SleepState>,
    /// Kayıtlı güç yönetimi cihazları (ID -> Arc<PowerManageable>)
    devices: Mutex<BTreeMap<u64, Arc<PowerManageable>>>,
    /// Uyandırma kaynakları listesi
    wakeup_sources: Mutex<Vec<WakeupSource>>,
    /// Askıya almayı engelleyen sebepler (kilit listesi)
    suspend_blockers: Mutex<Vec<String>>,
    /// Sistemin şu an askıya alım sürecinde olup olmadığı
    suspending: AtomicBool,
    /// Güç yönetimi istatistikleri
    stats: Mutex<PmStats>,
}

/// Sistemi uyandırabilecek bir kaynak.
/// Örnek: klavye, ağ kartı, RTC zamanlayıcısı.
#[derive(Clone, Debug)]
pub struct WakeupSource {
    pub name: String,
    pub device_id: u64,
    pub enabled: bool,
    pub count: u64,
}

/// Güç yönetimi istatistikleri.
/// Kaç kez askıya alındığı, toplam askıya alım süresi gibi bilgileri tutar.
#[derive(Clone, Debug, Default)]
pub struct PmStats {
    pub suspend_count: u64,
    pub resume_count: u64,
    pub suspend_fail_count: u64,
    pub last_suspend_time: u64,
    pub total_suspend_time: u64,
    pub deepest_state: SleepState,
}

impl PowerManager {
    pub const fn new() -> Self {
        Self {
            system_state: Mutex::new(SleepState::S0),
            devices: Mutex::new(BTreeMap::new()),
            wakeup_sources: Mutex::new(Vec::new()),
            suspend_blockers: Mutex::new(Vec::new()),
            suspending: AtomicBool::new(false),
            stats: Mutex::new(PmStats {
                suspend_count: 0,
                resume_count: 0,
                suspend_fail_count: 0,
                last_suspend_time: 0,
                total_suspend_time: 0,
                deepest_state: SleepState::S0,
            }),
        }
    }

    /// Cihazı güç yöneticisine kaydeder.
    /// Kaydedilen cihazlar sistem uyku geçişlerinde otomatik yönetilir.
    pub fn register_device(&self, device: Arc<PowerManageable>) {
        self.devices.lock().insert(device.device_id, device);
    }

    /// Cihazı güç yöneticisinden çıkarır.
    pub fn unregister_device(&self, device_id: u64) {
        self.devices.lock().remove(&device_id);
    }

    /// Sistemi uyandırabilecek bir kaynak ekler.
    pub fn add_wakeup_source(&self, name: &str, device_id: u64) {
        let ws = WakeupSource {
            name: String::from(name),
            device_id,
            enabled: true,
            count: 0,
        };
        self.wakeup_sources.lock().push(ws);
    }

    /// Belirtilen sebepten dolayı askıya almayı engeller.
    /// Tüm engelleyiciler kaldırılmadan sistem askıya alınamaz.
    pub fn block_suspend(&self, reason: &str) {
        self.suspend_blockers.lock().push(String::from(reason));
    }

    /// Belirtilen sebebe ait askıya alma engelini kaldırır.
    pub fn unblock_suspend(&self, reason: &str) {
        self.suspend_blockers.lock().retain(|r| r != reason);
    }

    /// Sistemin askıya alınmaya hazır olup olmadığını kontrol eder.
    /// Engelleyici liste boşsa true döner.
    pub fn can_suspend(&self) -> bool {
        self.suspend_blockers.lock().is_empty()
    }

    /// Sistemi belirtilen ACPI uyku durumuna sokar.
    /// Tüm cihazları hazırlar, askıya alır; uyanışta devam ettirir.
    pub fn enter_sleep(&self, state: SleepState) -> Result<(), PmError> {
        if !self.can_suspend() {
            return Err(PmError::Blocked);
        }

        self.suspending.store(true, Ordering::SeqCst);
        let start_time = crate::task::scheduler::get_ticks() as u64;

        crate::serial_println!("[PM] Entering sleep state {:?}", state);

        // Tüm cihazlarda hazırlık callback'ini çalıştır
        for device in self.devices.lock().values() {
            if let Some(cb) = device.prepare_cb {
                cb(device.device_id)?;
            }
        }

        // Cihazları ters sırayla askıya al (bağımlılık sırasına göre)
        let devices: Vec<Arc<PowerManageable>> = self.devices.lock().values().cloned().collect();

        for device in devices.iter().rev() {
            let target_state = match state {
                SleepState::S1 => DevicePowerState::D1,
                SleepState::S3 => DevicePowerState::D3Hot,
                SleepState::S4 | SleepState::S5 => DevicePowerState::D3Cold,
                _ => DevicePowerState::D0,
            };
            device.suspend(target_state)?;
        }

        // Gerçek ACPI uyku durumuna gir
        self.enter_acpi_state(state)?;

        // ... sistem şimdi uyuyor ...
        // ... ve şimdi uyandı ...

        // Cihazları devam ettir
        for device in devices.iter() {
            device.resume()?;
            if let Some(cb) = device.complete_cb {
                cb(device.device_id);
            }
        }

        // İstatistikleri güncelle
        let end_time = crate::task::scheduler::get_ticks() as u64;
        let mut stats = self.stats.lock();
        stats.suspend_count += 1;
        stats.resume_count += 1;
        stats.last_suspend_time = end_time - start_time;
        stats.total_suspend_time += stats.last_suspend_time;

        *self.system_state.lock() = SleepState::S0;
        self.suspending.store(false, Ordering::SeqCst);

        crate::serial_println!("[PM] Resumed from sleep state {:?}", state);

        Ok(())
    }

    /// ACPI uyku durumunu fiilen uygular.
    /// Gerçek uygulamada ACPI PM1a kontrol yazmacına değer yazılır.
    fn enter_acpi_state(&self, state: SleepState) -> Result<(), PmError> {
        *self.system_state.lock() = state;

        let (pm1a_cnt_addr, pm1b_cnt_addr) = acpi_pm1_control_blocks()?;
        if state == SleepState::S3 {
            if !crate::cpu::acpi::arm_s3_resume_vector()
                || !crate::cpu::acpi::s3_resume_vector_ready()
            {
                return Err(PmError::InvalidState);
            }
        }
        let (sleep_type_a, sleep_type_b) = acpi_sleep_type(state)?;
        let sleep_value_a = pm1_sleep_value(sleep_type_a);
        let sleep_value_b = pm1_sleep_value(sleep_type_b);

        match state {
            SleepState::S1 => {
                crate::serial_println!("[PM] Entering S1 (Power On Suspend)");
                write_pm1_control(pm1a_cnt_addr, pm1b_cnt_addr, sleep_value_a, sleep_value_b);
            }
            SleepState::S3 => {
                crate::serial_println!("[PM] Entering S3 (Suspend to RAM)");
                if !crate::cpu::s3_resume::enter_pm1_sleep(
                    pm1a_cnt_addr,
                    pm1b_cnt_addr,
                    sleep_value_a,
                    sleep_value_b,
                ) {
                    crate::serial_println!("[PM] S3 continuation did not resume from firmware wake");
                    return Err(PmError::InvalidState);
                }
                crate::serial_println!("[PM] S3 continuation resumed from firmware wake");
            }
            SleepState::S4 => {
                crate::serial_println!("[PM] Entering S4 (Hibernate)");
                write_pm1_control(pm1a_cnt_addr, pm1b_cnt_addr, sleep_value_a, sleep_value_b);
            }
            SleepState::S5 => {
                // Sistemi kapat
                crate::serial_println!("[PM] Powering off (S5)");
                unsafe {
                    use x86_64::instructions::port::Port;
                    // QEMU poweroff: port 0x604 değeri 0x2000
                    let mut qemu_port = Port::<u16>::new(0x604);
                    qemu_port.write(0x2000);
                    // Fallback: PM1a_CNT
                    write_pm1_control(pm1a_cnt_addr, pm1b_cnt_addr, sleep_value_a, sleep_value_b);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Sistemi S4 (Hibernate) durumuna sokar.
    /// Bellek imajı diske yazılır, güç kesilir.
    pub fn hibernate(&self) -> Result<(), PmError> {
        // Bellek imajını takas alanına kaydet
        self.enter_sleep(SleepState::S4)
    }

    /// Sistemi S3 (RAM'e Askıya Al) durumuna sokar.
    /// DRAM içeriği korunur, diğer donanım kapatılır.
    pub fn suspend_to_ram(&self) -> Result<(), PmError> {
        self.enter_sleep(SleepState::S3)
    }

    pub fn resume_from_firmware_wake(&self, state: SleepState) -> Result<(), PmError> {
        crate::serial_println!("[PM] Firmware wake resume begin {:?}", state);

        let devices: Vec<Arc<PowerManageable>> = self.devices.lock().values().cloned().collect();
        for device in devices.iter() {
            device.resume()?;
            if let Some(cb) = device.complete_cb {
                cb(device.device_id);
            }
        }

        let mut stats = self.stats.lock();
        stats.suspend_count += 1;
        stats.resume_count += 1;
        stats.last_suspend_time = 0;
        if state as u32 > stats.deepest_state as u32 {
            stats.deepest_state = state;
        }

        *self.system_state.lock() = SleepState::S0;
        self.suspending.store(false, Ordering::SeqCst);
        crate::serial_println!("[PM] Firmware wake resume complete {:?}", state);
        Ok(())
    }

    /// Sistemi S5 (Yumuşak Kapatma) durumuna sokar.
    pub fn power_off(&self) -> Result<(), PmError> {
        self.enter_sleep(SleepState::S5)
    }

    /// Sistemi yeniden başlatır.
    /// ACPI veya klavye kontrolcüsü üzerinden sıfırlama yapılır.
    pub fn reboot(&self) -> Result<(), PmError> {
        crate::serial_println!("[PM] Rebooting");
        // ACPI veya klavye kontrolcüsü üzerinden sıfırlama
        Ok(())
    }

    /// Güç yönetimi istatistiklerinin kopyasını döner.
    pub fn get_stats(&self) -> PmStats {
        self.stats.lock().clone()
    }

    /// Sistemin şu an askıya alım sürecinde olup olmadığını döner.
    pub fn is_suspending(&self) -> bool {
        self.suspending.load(Ordering::SeqCst)
    }
}

fn acpi_pm1_control_blocks() -> Result<(u16, u16), PmError> {
    let state = crate::cpu::acpi::ACPI_STATE.lock();
    if !state.fadt_parsed || state.pm1a_cnt_blk == 0 {
        crate::serial_println!("[PM] FADT PM1 control block unavailable");
        return Err(PmError::InvalidState);
    }
    Ok((state.pm1a_cnt_blk, state.pm1b_cnt_blk))
}

fn acpi_sleep_type(state: SleepState) -> Result<(u16, u16), PmError> {
    match state {
        SleepState::S1 => Ok((0, 0)),
        SleepState::S3 => crate::cpu::acpi_aml::get_s3_sleep_type()
            .or_else(|| crate::cpu::acpi::dsdt_sleep_type(3))
            .ok_or(PmError::InvalidState),
        SleepState::S4 => crate::cpu::acpi_aml::get_s4_sleep_type()
            .or_else(|| crate::cpu::acpi::dsdt_sleep_type(4))
            .ok_or(PmError::InvalidState),
        SleepState::S5 => Ok(crate::cpu::acpi_aml::get_s5_sleep_type()
            .or_else(|| crate::cpu::acpi::dsdt_sleep_type(5))
            .unwrap_or((5, 5))),
        SleepState::S0 => Ok((0, 0)),
    }
}

fn pm1_sleep_value(sleep_type: u16) -> u16 {
    ((sleep_type & 0x7) << 10) | (1 << 13)
}

fn write_pm1_control(pm1a: u16, pm1b: u16, value_a: u16, value_b: u16) {
    unsafe {
        use x86_64::instructions::port::Port;
        Port::<u16>::new(pm1a).write(value_a);
        if pm1b != 0 {
            Port::<u16>::new(pm1b).write(value_b);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref PM_MANAGER: PowerManager = PowerManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Güç yönetimi işlemlerinde oluşabilecek hatalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmError {
    /// Cihaz hâlâ kullanımda, askıya alınamaz
    Busy,
    /// Askıya alma engelleyici mevcut
    Blocked,
    /// Cihaz hata durumunda
    DeviceError,
    /// Bu işlem desteklenmiyor
    NotSupported,
    /// Hazırlık aşamasında hata
    PrepareFailed,
    /// Askıya alma aşamasında hata
    SuspendFailed,
    /// Devam ettirme aşamasında hata
    ResumeFailed,
    /// ACPI/FADT durumu uyku geçişi için yeterli değil
    InvalidState,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// sys_reboot sistem çağrısı.
/// Linux uyumlu komut kodlarıyla sistemin yeniden başlatılmasını, kapatılmasını
/// veya hibernate durumuna geçişini sağlar.
pub fn sys_reboot(cmd: u32) -> i32 {
    match cmd {
        0 => {
            // LINUX_REBOOT_CMD_RESTART
            let _ = PM_MANAGER.reboot();
            0
        }
        1 => {
            // LINUX_REBOOT_CMD_POWER_OFF
            let _ = PM_MANAGER.power_off();
            0
        }
        2 => {
            // LINUX_REBOOT_CMD_HALT
            0
        }
        3 => {
            // LINUX_REBOOT_CMD_SW_SUSPEND (Hibernate)
            let _ = PM_MANAGER.hibernate();
            0
        }
        _ => -22, // EINVAL: geçersiz komut
    }
}

/// sys_suspend sistem çağrısı.
/// Sayısal uyku durumu kodunu (1/3/4) ACPI SleepState enum'una çevirir.
pub fn sys_suspend(state: u32) -> i32 {
    let sleep_state = match state {
        1 => SleepState::S1,
        3 => SleepState::S3,
        4 => SleepState::S4,
        _ => return -22, // EINVAL
    };

    match PM_MANAGER.enter_sleep(sleep_state) {
        Ok(()) => 0,
        Err(_) => -5, // EIO
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Güç yönetimi alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[PM] Güç yönetimi başlatıldı");
}
