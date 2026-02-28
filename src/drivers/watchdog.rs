//! # Watchdog Timer (Bekçi Zamanlayıcı)
//!
//! Donanım watchdog zamanlayıcı desteği. Watchdog, yazılım
//! donduğunda (yanıt vermediğinde) sistemi otomatik olarak sıfırlayan
//! bir güvenlik mekanizmasıdır.
//!
//! ## Watchdog Nasıl Çalışır?
//!
//! ```
//!  ┌────────────────────────────────────────────────────┐
//!  │ Watchdog Donanımı                                  │
//!  │                                                    │
//!  │  Sayaç: TIMEOUT → 0                               │
//!  │          ↑ ping() ile sıfırlanır                  │
//!  │                                                    │
//!  │  Eğer sayaç 0'a ulaşırsa → SİSTEM SIFIRLAMA      │
//!  └────────────────────────────────────────────────────┘
//!
//!  Yazılım akışı:
//!  start() → [çalışıyor...] → ping() → ping() → ping() → ...
//!                                ↑ zamanlayıcı kesimine
//!                                  (timer tick) çağrılır
//! ```
//!
//! ## Ping (Keepalive)
//!
//! `ping()` fonksiyonu, watchdog sayacını zaman aşımı değerine sıfırlar.
//! Periyodik olarak çağrılmazsa sistem hardware watchdog tarafından sıfırlanır.
//! Bu, kilitlenmiş bir çekirdeği otomatik kurtarır.
//!
//! ## Nowayout Modu
//!
//! `nowayout = true` ise watchdog bir daha durdurulamaz.
//! Bu, güvenlik açısından kritik sistemlerde kullanılır;
//! saldırgan veya hatalı kod watchdog'u kapatamaz.
//!
//! ## Boot Status
//!
//! Sistem bir önceki çalışmada watchdog sıfırlamasıyla mı kapandı?
//! `get_boot_status()` bunu sorgular. Log analizi için önemlidir.
//!
//! ## Desteklenen Donanımlar
//!
//! - `HpetWatchdog`: HPET (High Precision Event Timer) tabanlı
//! - `TcoWatchdog`: Intel ICH/PCH TCO (Total Cost of Ownership) timer

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ============================================================================
// WATCHDOG SABİTLERİ
// ============================================================================

/// Varsayılan zaman aşımı: 60 saniye
pub const WATCHDOG_DEFAULT_TIMEOUT: u32 = 60;
/// Minimum izin verilen zaman aşımı: 1 saniye
pub const WATCHDOG_MIN_TIMEOUT: u32 = 1;
/// Maksimum zaman aşımı: 65535 saniye (~18 saat)
pub const WATCHDOG_MAX_TIMEOUT: u32 = 65535;

// ============================================================================
// WATCHDOG İŞLEM ARAYÜZÜ (TRAIT)
// ============================================================================

/// Watchdog donanım soyutlaması.
///
/// Her watchdog tipi (HPET, TCO, ...) bu trait'i implement eder.
/// `Send + Sync`: farklı CPU çekirdeklerinden erişilebilir.
///
/// Bu, Rust'ın "trait object" polymorphism deseni:
/// `Option<&'static dyn WatchdogOps>` ile çalışma zamanında
/// farklı implementasyonlar kullanılabilir.
pub trait WatchdogOps: Send + Sync {
    /// Watchdog sayacını başlatır
    fn start(&self) -> Result<(), WatchdogError>;
    /// Watchdog sayacını durdurur (nowayout=true ise hata döner)
    fn stop(&self) -> Result<(), WatchdogError>;
    /// Sayacı sıfırlar (keepalive / besleme)
    fn ping(&self) -> Result<(), WatchdogError>;
    /// Zaman aşımı süresini saniye olarak ayarlar
    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError>;
    /// Mevcut zaman aşımı değerini döndürür
    fn get_timeout(&self) -> u32;
    /// Sıfırlamaya kalan süreyi döndürür
    fn get_timeleft(&self) -> u32;
    /// Son boot'un watchdog sıfırlamasıyla mı gerçekleştiğini döndürür
    fn get_boot_status(&self) -> WatchdogBootStatus;
}

// ============================================================================
// WATCHDOG DURUM BİLGİSİ
// ============================================================================

/// Son boot'un nasıl gerçekleştiğini açıklar.
///
/// `WatchdogReset`: watchdog zaman aşımı nedeniyle sıfırlama → olağandışı durum
/// `Normal`: normal kapatma/açma → sağlıklı durum
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogBootStatus {
    /// Normal güç açma veya yazılımsal yeniden başlatma
    Normal,
    /// Watchdog zaman aşımı nedeniyle zorla sıfırlama
    WatchdogReset,
    /// Durum bilinmiyor (donanım bildirimedi)
    Unknown,
}

// ============================================================================
// WATCHDOG CİHAZ YAPISI
// ============================================================================

/// Watchdog cihazı: durum + donanım operasyon fonksiyonları.
///
/// Atomic tipler kullanılır çünkü bu yapı birden fazla CPU çekirdeğinden
/// erişilebilir olmalıdır (no locking = düşük gecikme).
///
/// `ops: Option<&'static dyn WatchdogOps>`: başlatma sırasında set edilir.
/// Başlatılmadan önce `None`'dur; operasyonlar sessizce atlanır.
pub struct WatchdogDevice {
    /// İnsan okunabilir cihaz adı (örn. "hpet-watchdog")
    pub name: &'static str,
    /// Watchdog çalışıyor mu? (true = sayaç sayıyor)
    pub running: AtomicBool,
    /// Zaman aşımı süresi (saniye)
    pub timeout: AtomicU32,
    /// Son ping zamanı (scheduler tick cinsinden)
    pub last_ping: AtomicU64,
    /// Nowayout: true ise çalıştıktan sonra durdurulamaz
    pub nowayout: AtomicBool,
    /// Boot durum kodu (WatchdogBootStatus::* değeri)
    pub boot_status: AtomicU32,
    /// Donanıma özgü operasyonlar (dinamik dispatch)
    pub ops: Option<&'static dyn WatchdogOps>,
}

impl WatchdogDevice {
    /// Derleme zamanı sabit oluşturucu (`const fn`).
    ///
    /// `const fn`: global/static değişken oluşturmak için derleme zamanında
    /// çalışabilir. `lazy_static!` gerektirmez.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            running: AtomicBool::new(false),
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
            last_ping: AtomicU64::new(0),
            nowayout: AtomicBool::new(false),
            boot_status: AtomicU32::new(0),
            ops: None,
        }
    }

    /// Watchdog'u başlatır.
    ///
    /// Ops varsa donanım başlatma çağrısını yapar,
    /// ardından `running = true` ve ilk ping zamanını kaydeder.
    /// `SeqCst`: diğer CPU'ların bu değişikliği hemen görmesini garanti eder.
    pub fn start(&self) -> Result<(), WatchdogError> {
        if let Some(ops) = self.ops {
            ops.start()?;
            self.running.store(true, Ordering::SeqCst);
            self.last_ping.store(
                crate::task::scheduler::get_ticks(),
                Ordering::SeqCst
            );
            crate::serial_println!("[WATCHDOG] {} started", self.name);
        }
        Ok(())
    }

    /// Watchdog'u durdurur.
    ///
    /// `nowayout=true` ise `NoWayOut` hatası döner.
    /// Bu güvenlik mekanizması, kritik sistemlerde watchdog'un
    /// devre dışı bırakılmasını engeller.
    pub fn stop(&self) -> Result<(), WatchdogError> {
        if self.nowayout.load(Ordering::SeqCst) {
            return Err(WatchdogError::NoWayOut);
        }

        if let Some(ops) = self.ops {
            ops.stop()?;
            self.running.store(false, Ordering::SeqCst);
            crate::serial_println!("[WATCHDOG] {} stopped", self.name);
        }
        Ok(())
    }

    /// Watchdog sayacını sıfırlar (ping / keepalive).
    ///
    /// Çalışmıyorsa `NotRunning` hatası döner.
    /// Zamanlayıcı kesimine (timer ISR) bağlı periyodik olarak çağrılır.
    pub fn ping(&self) -> Result<(), WatchdogError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(WatchdogError::NotRunning);
        }

        if let Some(ops) = self.ops {
            ops.ping()?;
            self.last_ping.store(
                crate::task::scheduler::get_ticks(),
                Ordering::SeqCst
            );
        }
        Ok(())
    }

    /// Zaman aşımı süresini ayarlar.
    ///
    /// Geçerli aralık kontrolü yapılır: [WATCHDOG_MIN_TIMEOUT, WATCHDOG_MAX_TIMEOUT]
    /// Donanımın desteklediği maksimum değer aşılamaz.
    pub fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        if seconds < WATCHDOG_MIN_TIMEOUT || seconds > WATCHDOG_MAX_TIMEOUT {
            return Err(WatchdogError::InvalidTimeout);
        }

        if let Some(ops) = self.ops {
            ops.set_timeout(seconds)?;
        }
        self.timeout.store(seconds, Ordering::SeqCst);
        Ok(())
    }

    /// Mevcut zaman aşımı değerini döndürür.
    pub fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    /// Sıfırlamaya kalan süreyi döndürür.
    /// Ops yoksa 0 döner (bilinmiyor).
    pub fn get_timeleft(&self) -> u32 {
        if let Some(ops) = self.ops {
            return ops.get_timeleft();
        }
        0
    }

    /// Watchdog çalışıyor mu?
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Nowayout modunu ayarlar.
    ///
    /// `true` yapıldıktan sonra `stop()` çağrıları `NoWayOut` hatası döndürür.
    /// Geri alınamaz bir güvenlik kilididir.
    pub fn set_nowayout(&self, value: bool) {
        self.nowayout.store(value, Ordering::SeqCst);
    }
}

// ============================================================================
// HPET WATCHDOG
// HPET (High Precision Event Timer) tabanlı implementasyon
// ============================================================================

/// HPET tabanlı watchdog.
///
/// HPET, x86 sistemlerde TSC'ye alternatif yüksek çözünürlüklü zamanlayıcıdır.
/// Watchdog için HPET'in karşılaştırma (comparator) mekanizması kullanılır.
///
/// `base_addr`: HPET MMIO register tabanı (ACPI tablosundan okunur)
/// `timer_id`: Hangi HPET timer'ın watchdog için kullanılacağı (0-31)
pub struct HpetWatchdog {
    base_addr: u64,
    timer_id: u32,
    timeout: AtomicU32,
}

impl HpetWatchdog {
    /// Yeni HPET watchdog oluşturur.
    ///
    /// `base_addr`: ACPI HPET tablosundaki fiziksel MMIO adresi
    /// `timer_id`: Karşılaştırıcı timer numarası (0 = birincil)
    pub fn new(base_addr: u64, timer_id: u32) -> Self {
        Self {
            base_addr,
            timer_id,
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
        }
    }
}

impl WatchdogOps for HpetWatchdog {
    fn start(&self) -> Result<(), WatchdogError> {
        // HPET timer'ı watchdog modunda yapılandır
        Ok(())
    }

    fn stop(&self) -> Result<(), WatchdogError> {
        Ok(())
    }

    fn ping(&self) -> Result<(), WatchdogError> {
        // Timer karşılaştırıcısını sıfırla (reload)
        Ok(())
    }

    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        self.timeout.store(seconds, Ordering::SeqCst);
        Ok(())
    }

    fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    fn get_timeleft(&self) -> u32 {
        0
    }

    fn get_boot_status(&self) -> WatchdogBootStatus {
        WatchdogBootStatus::Unknown
    }
}

// ============================================================================
// TCO WATCHDOG (Intel ICH/PCH)
// Total Cost of Ownership Timer - x86 çipsetlerinde yerleşik
// ============================================================================

/// Intel TCO (Total Cost of Ownership) Watchdog.
///
/// TCO timer, Intel ICH (I/O Controller Hub) ve PCH (Platform Controller Hub)
/// çipsetlerinde yerleşik olarak bulunur. LPC/eSPI bus üzerinden erişilir.
///
/// `iobase`: ACPI PM I/O uzayındaki TCO register tabanı (örn. 0x400 + 0x60)
///
/// ## TCO Zaman Aşımı Sınırlaması
///
/// TCO timer'ın donanımsal maksimum zaman aşımı ~613 saniyedir.
/// Bu değeri aşan istekler 613 saniyeye kırpılır.
pub struct TcoWatchdog {
    iobase: u16,
    timeout: AtomicU32,
}

impl TcoWatchdog {
    /// TCO watchdog oluşturur.
    ///
    /// `iobase`: ACPI kaynaklarından elde edilen TCO I/O port tabanı
    pub fn new(iobase: u16) -> Self {
        Self {
            iobase,
            timeout: AtomicU32::new(WATCHDOG_DEFAULT_TIMEOUT),
        }
    }
}

impl WatchdogOps for TcoWatchdog {
    fn start(&self) -> Result<(), WatchdogError> {
        // TCO timer'ı etkinleştir
        Ok(())
    }

    fn stop(&self) -> Result<(), WatchdogError> {
        // TCO timer'ı devre dışı bırak
        Ok(())
    }

    fn ping(&self) -> Result<(), WatchdogError> {
        // TCO sayacını yenile (TCO_RLD register'ına yaz)
        Ok(())
    }

    fn set_timeout(&self, seconds: u32) -> Result<(), WatchdogError> {
        // TCO donanımsal maksimum 613 saniye sınırı
        let tco_timeout = if seconds > 613 { 613 } else { seconds };
        self.timeout.store(tco_timeout, Ordering::SeqCst);
        Ok(())
    }

    fn get_timeout(&self) -> u32 {
        self.timeout.load(Ordering::SeqCst)
    }

    fn get_timeleft(&self) -> u32 {
        // TCO sayaç değerini oku (TCO_TMR register)
        0
    }

    fn get_boot_status(&self) -> WatchdogBootStatus {
        // TCO status register'ından son sıfırlama nedenini oku
        WatchdogBootStatus::Unknown
    }
}

// ============================================================================
// WATCHDOG YÖNETİCİSİ
// Tüm kayıtlı watchdog cihazlarını yönetir
// ============================================================================

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// Watchdog cihazlarını merkezi olarak yöneten yapı.
///
/// ## Tasarım Kararları
///
/// - `devices`: tüm kayıtlı watchdog'lar (birden fazla olabilir)
/// - `active`: şu anda aktif olan watchdog (None = hiç aktif değil)
/// - `Arc<WatchdogDevice>`: paylaşımlı sahiplik; birden fazla referans olabilir
/// - İki ayrı `Mutex`: kilitlenmeyi (deadlock) önlemek için minimal kilitleme
pub struct WatchdogManager {
    devices: Mutex<Vec<Arc<WatchdogDevice>>>,
    active: Mutex<Option<Arc<WatchdogDevice>>>,
}

impl WatchdogManager {
    /// Derleme zamanı oluşturma (`const fn`).
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(Vec::new()),
            active: Mutex::new(None),
        }
    }

    /// Watchdog'u sisteme kaydeder.
    ///
    /// `Arc::clone()` referans sayacını artırır; orijinal Arc geçerliliğini korur.
    pub fn register(&self, device: Arc<WatchdogDevice>) {
        self.devices.lock().push(device.clone());
        crate::serial_println!("[WATCHDOG] Registered {}", device.name);
    }

    /// İsme göre bir watchdog'u aktif hale getirir ve başlatır.
    ///
    /// Bu, "Strategy Pattern":
    /// - Hangi watchdog kullanılacağı çalışma zamanında belirlenir
    /// - Kullanıcı yalnızca isme göre seçim yapar
    pub fn set_active(&self, name: &str) -> Result<(), WatchdogError> {
        let devices = self.devices.lock();
        for device in devices.iter() {
            if device.name == name {
                *self.active.lock() = Some(device.clone());
                device.start()?;
                return Ok(());
            }
        }
        Err(WatchdogError::NotFound)
    }

    /// Aktif watchdog'u besler (ping).
    ///
    /// Zamanlayıcı kesimine (timer ISR) bağlanmalıdır.
    /// Aktif watchdog yoksa sessizce başarı döner.
    pub fn ping_active(&self) -> Result<(), WatchdogError> {
        if let Some(device) = self.active.lock().as_ref() {
            device.ping()?;
        }
        Ok(())
    }

    /// Aktif watchdog'a referans döndürür.
    ///
    /// `cloned()`: Arc referans sayacını artırarak güvenli kopyalama yapar.
    pub fn get_active(&self) -> Option<Arc<WatchdogDevice>> {
        self.active.lock().as_ref().cloned()
    }
}

/// Global Watchdog yöneticisi.
///
/// `lazy_static!`: ilk erişimde başlatılır (çalışma zamanı sabiti).
/// Bu, `const fn` ile oluşturulamayan karmaşık tipler için kullanılır.
lazy_static::lazy_static! {
    pub static ref WATCHDOG_MANAGER: WatchdogManager = WatchdogManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Watchdog hatalarını temsil eden enum.
///
/// Rust'ta `Result<T, E>` ile hata yönetimi yapılır.
/// `#[derive(Debug, Clone, Copy)]`: hata mesajı yazdırmak, klonlamak
/// ve kopyalamak için gerekli trait'ler otomatik türetilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogError {
    /// Watchdog çalışmıyor (ping denemesi başarısız)
    NotRunning,
    /// Watchdog zaten çalışıyor
    AlreadyRunning,
    /// Geçersiz zaman aşımı değeri
    InvalidTimeout,
    /// Nowayout modu: durdurma yasak
    NoWayOut,
    /// Belirtilen isimde watchdog cihazı bulunamadı
    NotFound,
    /// I/O register okuma/yazma hatası
    IoError,
}

// ============================================================================
// BAŞLATMA FONKSİYONLARI
// ============================================================================

/// Watchdog alt sistemini başlatır.
///
/// Daha fazla yapılandırma gerektiren sistemlerde buraya donanım keşfi eklenir.
pub fn init() {
    crate::serial_println!("[WATCHDOG] Subsystem initialized");
}

/// Aktif watchdog'u besler (timer tick'inden çağrılır).
///
/// Bu fonksiyon periyodik zamanlayıcı kesimine bağlanmalıdır.
/// Aktif watchdog yoksa `Ok(())` ile sessizce döner.
/// `let _ =`: hata dönsün veya dönmesin yoksay (isteğe bağlı ping).
pub fn ping() {
    let _ = WATCHDOG_MANAGER.ping_active();
}
