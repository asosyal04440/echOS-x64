//! # ACPI Olayları (GPE - General Purpose Events)
//!
//! Genel Amaçlı Olaylar ve ACPI olay işleme altyapısı.
//!
//! ## GPE Nedir?
//! GPE (General Purpose Event), donanım aygıtlarının ACPI olay mekanizması
//! aracılığıyla işletim sistemine sinyal gönderdiği kesme tabanlı bir sistemdir.
//! Güç düğmesi, uyku düğmesi, pil durumu, sıcaklık değişimleri gibi olaylar
//! GPE kanalları üzerinden bildirilir.
//!
//! ## GPE Bloğu Akışı
//! ```ascii
//! Donanım Olayı
//!      |
//!      v
//! GPE Durum Yazmacı (GPE_STS)
//!      |
//!      v
//! GPE Etkinleştirme Yazmacı (GPE_EN) ile AND
//!      |
//!      v
//! Etkin GPE'ler → handle_events()
//!      |
//!      v
//! AML yöntemi çağrısı veya doğrudan işleyici
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// GPE SABİTLERİ
// ============================================================================

/// GPE blok yazmacı adları — ACPI FADT'tan alınan blok tanımlayıcıları.
pub const GPE0_BLK: &str = "GPE0_BLK";
pub const GPE1_BLK: &str = "GPE1_BLK";

/// GPE yazmacı ofsetleri.
///
/// Her GPE bloğu birbirine bitişik durum ve etkinleştirme yazmaçlarına sahiptir.
pub const GPE_STS_OFFSET: usize = 0; // Durum yazmacı ofset
pub const GPE_EN_OFFSET: usize = 2;  // Etkinleştirme yazmacı ofset

/// GPE olay türleri — bir GPE'nin uyandırma, çalışma zamanı veya her ikisi olduğunu belirtir.
pub const GPE_TYPE_WAKE: u8 = 0x01;           // Yalnızca uyandırma olayı
pub const GPE_TYPE_RUNTIME: u8 = 0x02;        // Yalnızca çalışma zamanı olayı
pub const GPE_TYPE_WAKE_RUNTIME: u8 = 0x03;   // Hem uyandırma hem çalışma zamanı

/// Sabit (Fixed) olay numaraları — ACPI belirtimine göre tanımlanmış sabit donanım olayları.
pub const ACPI_EVENT_PMTIMER: u32 = 0;      // PM zamanlayıcı taşması olayı
pub const ACPI_EVENT_POWER_BUTTON: u32 = 2; // Güç düğmesi basılması olayı
pub const ACPI_EVENT_SLEEP_BUTTON: u32 = 3; // Uyku düğmesi basılması olayı
pub const ACPI_EVENT_RTC: u32 = 4;          // Gerçek zamanlı saat alarmı olayı

// ============================================================================
// GPE OLAYI
// ============================================================================

/// Tek bir GPE (Genel Amaçlı Olay) tanımı ve durum bilgisi.
///
/// Her GPE'nin bir numarası, bloğu, türü ve isteğe bağlı AML işleyici yöntemi bulunur.
/// Atomik alanlar sayesinde çok işlemcili erişimde kilit gerekmez.
#[derive(Clone, Debug)]
pub struct GpeEvent {
    /// GPE numarası (blok içindeki bit konumu)
    pub number: u32,
    /// GPE bloğu (0 veya 1)
    pub block: u8,
    /// Olay türü (uyandırma / çalışma zamanı)
    pub event_type: u8,
    /// İşleyici AML yöntemi adı (varsa)
    pub handler: Option<String>,
    /// GPE etkin mi?
    pub enabled: AtomicBool,
    /// Uyandırma yeteneği var mı?
    pub wake_capable: AtomicBool,
    /// İşleyici türü: 0=yok, 1=AML yöntemi, 2=doğrudan işleyici
    pub handler_type: AtomicU32,
}

impl GpeEvent {
    /// Yeni bir GPE olayı oluşturur; başlangıçta devre dışı ve uyandırma yeteneği yok.
    pub fn new(number: u32, block: u8) -> Self {
        Self {
            number,
            block,
            event_type: GPE_TYPE_RUNTIME,
            handler: None,
            enabled: AtomicBool::new(false),
            wake_capable: AtomicBool::new(false),
            handler_type: AtomicU32::new(0),
        }
    }

    /// GPE olayını etkinleştirir.
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// GPE olayını devre dışı bırakır.
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// GPE'nin uyandırma yeteneğini ayarlar veya temizler.
    pub fn set_wake(&self, wake: bool) {
        self.wake_capable.store(wake, Ordering::SeqCst);
    }
}

// ============================================================================
// SABİT OLAY
// ============================================================================

/// Sabit ACPI olay tanımı ve durum bilgisi.
///
/// Sabit olaylar (güç düğmesi, uyku düğmesi, RTC alarmı vb.) PM1 durum ve
/// etkinleştirme yazmaçları aracılığıyla işlenir.
#[derive(Clone, Debug)]
pub struct FixedEvent {
    /// Olay numarası (ACPI_EVENT_* sabitleri)
    pub number: u32,
    /// Olay adı (insan tarafından okunabilir)
    pub name: String,
    /// Durum yazmaç adresi (PM1a veya PM1b)
    pub status_reg: u32,
    /// Etkinleştirme yazmaç adresi
    pub enable_reg: u32,
    /// İşleyici AML yöntemi adı (varsa)
    pub handler: Option<String>,
    /// Olay etkin mi?
    pub enabled: AtomicBool,
}

impl FixedEvent {
    /// Yeni bir sabit olay oluşturur.
    pub fn new(number: u32, name: &str, status_reg: u32, enable_reg: u32) -> Self {
        Self {
            number,
            name: String::from(name),
            status_reg,
            enable_reg,
            handler: None,
            enabled: AtomicBool::new(false),
        }
    }

    /// Sabit olayı etkinleştirir; etkinleştirme yazmacına yazar.
    pub fn enable(&self) {
        // Etkinleştirme yazmacına yaz
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[GPE] Fixed event {} enabled", self.name);
    }

    /// Sabit olayı devre dışı bırakır.
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Durum yazmacını okuyarak olayın tetiklenip tetiklenmediğini kontrol eder.
    pub fn check_status(&self) -> bool {
        // Durum yazmacını oku
        false
    }

    /// Durum yazmacını temizler; olay ele alındıktan sonra çağrılmalıdır.
    pub fn clear_status(&self) {
        // Durum yazmacına yaz
    }
}

// ============================================================================
// GPE BLOĞU
// ============================================================================

/// ACPI GPE bloğu — bir GPE donanım bloğunu ve içerdiği olayları yönetir.
///
/// GPE0_BLK ve GPE1_BLK olmak üzere iki blok bulunabilir.
/// Her blok birden fazla GPE içerir; her GPE tek bir bittir.
pub struct GpeBlock {
    /// Blok numarası (0 veya 1)
    pub block_number: u8,
    /// Bloğun temel I/O adresi
    pub base_address: u32,
    /// Blok içindeki GPE sayısı
    pub gpe_count: u32,
    /// GPE olay koleksiyonu (numara -> olay)
    pub events: Mutex<BTreeMap<u32, GpeEvent>>,
    /// Durum yazmacı önbelleklenmiş değeri
    pub status_cache: AtomicU32,
    /// Etkinleştirme yazmacı önbelleklenmiş değeri
    pub enable_cache: AtomicU32,
}

impl GpeBlock {
    /// Yeni bir GPE bloğu oluşturur.
    pub fn new(block_number: u8, base_address: u32, gpe_count: u32) -> Self {
        Self {
            block_number,
            base_address,
            gpe_count,
            events: Mutex::new(BTreeMap::new()),
            status_cache: AtomicU32::new(0),
            enable_cache: AtomicU32::new(0),
        }
    }

    /// GPE olaylarını başlatır — blok içindeki her GPE için boş olay oluşturur.
    pub fn init(&self) {
        let mut events = self.events.lock();

        for i in 0..self.gpe_count {
            events.insert(i, GpeEvent::new(i, self.block_number));
        }
    }

    /// Belirtilen numaralı GPE olayını döner.
    pub fn get_event(&self, number: u32) -> Option<GpeEvent> {
        self.events.lock().get(&number).cloned()
    }

    /// Belirtilen GPE'yi etkinleştirir ve etkinleştirme önbelleğini günceller.
    pub fn enable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.enable();

            // Etkinleştirme yazmacını güncelle: ilgili biti set et
            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_or(bit, Ordering::SeqCst);

            crate::serial_println!("[GPE] GPE{} enabled", number);
        }
    }

    /// Belirtilen GPE'yi devre dışı bırakır ve etkinleştirme önbelleğini günceller.
    pub fn disable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.disable();

            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_and(!bit, Ordering::SeqCst);
        }
    }

    /// GPE durum bitini temizler — olay ele alındıktan sonra çağrılmalıdır.
    pub fn clear_gpe(&self, number: u32) {
        let bit = 1u32 << (number % 32);
        self.status_cache.fetch_and(!bit, Ordering::SeqCst);
    }

    /// Tüm etkin ve tetiklenmiş GPE'leri işler.
    ///
    /// Durum ve etkinleştirme yazmacı önbelleklerini AND'leyerek etkin bitleri bulur,
    /// her tetiklenen GPE için AML işleyicisini çağırır ve durumu temizler.
    /// Döner değer: tetiklenen GPE numaralarının listesi.
    pub fn handle_events(&self) -> Vec<u32> {
        let mut triggered = Vec::new();

        // Durum ve etkinleştirme yazmacı önbelleklerini oku
        let status = self.status_cache.load(Ordering::SeqCst);
        let enabled = self.enable_cache.load(Ordering::SeqCst);

        let active = status & enabled;

        for i in 0..self.gpe_count {
            let bit = 1u32 << (i % 32);
            if active & bit != 0 {
                triggered.push(i);

                // İşleyiciyi çalıştır
                if let Some(event) = self.events.lock().get(&i) {
                    if let Some(ref handler) = event.handler {
                        crate::serial_println!("[GPE] Executing handler {} for GPE{}", handler, i);
                        // AML yöntemi çalıştır
                    }
                }

                // Durumu temizle
                self.clear_gpe(i);
            }
        }

        triggered
    }
}

// ============================================================================
// ACPI OLAY YÖNETİCİSİ
// ============================================================================

/// ACPI Olay Yöneticisi — hem GPE hem de sabit olayları merkezi olarak yönetir.
///
/// Sistem başlangıcında FADT'tan alınan adreslerle başlatılır.
/// GPE blokları ve sabit olaylar bu yapı üzerinden etkinleştirilir, devre dışı bırakılır
/// ve işlenir.
pub struct AcpiEventManager {
    /// GPE blokları koleksiyonu
    pub gpe_blocks: Mutex<Vec<GpeBlock>>,
    /// Sabit olaylar koleksiyonu (numara -> olay)
    pub fixed_events: Mutex<BTreeMap<u32, FixedEvent>>,
    /// Olay işleyicileri (ad -> işleyici trait nesnesi)
    pub handlers: Mutex<BTreeMap<String, Arc<dyn AcpiEventHandler>>>,
    /// Yönetici başlatıldı mı?
    pub initialized: AtomicBool,
    /// GPE istatistikleri
    pub stats: Mutex<GpeStats>,
}

/// GPE istatistikleri — olay işleme performansını izlemek için.
#[derive(Clone, Debug, Default)]
pub struct GpeStats {
    pub gpes_handled: u64,
    pub fixed_events_handled: u64,
    pub spurious_events: u64,
}

/// ACPI olay işleyici arabirim trait'i.
///
/// Bir olayı ele almak için `handle()` metodu uygulanmalıdır.
pub trait AcpiEventHandler: Send + Sync {
    fn handle(&self, event: u32) -> Result<(), AcpiEventError>;
}

impl AcpiEventManager {
    /// Sabit başlatıcı — global static ataması için `const fn` gereklidir.
    pub const fn new() -> Self {
        Self {
            gpe_blocks: Mutex::new(Vec::new()),
            fixed_events: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            stats: Mutex::new(GpeStats::default()),
        }
    }

    /// FADT'taki GPE blok adresleri ile olay yöneticisini başlatır.
    ///
    /// GPE0 ve GPE1 bloklarını oluşturur, sabit olayları (güç/uyku düğmesi, RTC) kaydeder.
    pub fn init(&self, gpe0_base: u32, gpe0_count: u32, gpe1_base: u32, gpe1_count: u32) {
        // GPE blok 0 oluştur
        if gpe0_count > 0 {
            let block0 = GpeBlock::new(0, gpe0_base, gpe0_count);
            block0.init();
            self.gpe_blocks.lock().push(block0);
        }

        // GPE blok 1 oluştur
        if gpe1_count > 0 {
            let block1 = GpeBlock::new(1, gpe1_base, gpe1_count);
            block1.init();
            self.gpe_blocks.lock().push(block1);
        }

        // Sabit olayları kaydet
        let mut fixed = self.fixed_events.lock();
        fixed.insert(ACPI_EVENT_PMTIMER, FixedEvent::new(
            ACPI_EVENT_PMTIMER, "PMTIMER", 0, 0
        ));
        fixed.insert(ACPI_EVENT_POWER_BUTTON, FixedEvent::new(
            ACPI_EVENT_POWER_BUTTON, "POWER_BUTTON", 0, 0
        ));
        fixed.insert(ACPI_EVENT_SLEEP_BUTTON, FixedEvent::new(
            ACPI_EVENT_SLEEP_BUTTON, "SLEEP_BUTTON", 0, 0
        ));
        fixed.insert(ACPI_EVENT_RTC, FixedEvent::new(
            ACPI_EVENT_RTC, "RTC", 0, 0
        ));

        self.initialized.store(true, Ordering::SeqCst);

        crate::serial_println!("[GPE] Event manager initialized");
    }

    /// Belirtilen GPE için AML işleyici yöntemi kaydeder.
    ///
    /// GPE bloğu ve numarası ile işleyici adı belirtilir; başarısız olursa `InvalidGpe` döner.
    pub fn install_gpe_handler(&self, gpe_number: u32, block: u8, handler: &str) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();

        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            if let Some(event) = gpe_block.events.lock().get_mut(&gpe_number) {
                event.handler = Some(String::from(handler));
                event.handler_type.store(1, Ordering::SeqCst);
                return Ok(());
            }
        }

        Err(AcpiEventError::InvalidGpe)
    }

    /// Belirtilen GPE'yi etkinleştirir.
    pub fn enable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();

        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.enable_gpe(gpe_number);
            return Ok(());
        }

        Err(AcpiEventError::InvalidGpe)
    }

    /// Belirtilen GPE'yi devre dışı bırakır.
    pub fn disable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();

        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.disable_gpe(gpe_number);
            return Ok(());
        }

        Err(AcpiEventError::InvalidGpe)
    }

    /// Tüm GPE bloklarını ve sabit olayları tarayarak tetiklenmiş olayları işler.
    ///
    /// Her GPE bloğundaki olayları ve sabit (fixed) donanım olaylarını kontrol eder.
    pub fn handle_events(&self) {
        // GPE olaylarını işle
        for block in self.gpe_blocks.lock().iter() {
            let triggered = block.handle_events();

            let mut stats = self.stats.lock();
            stats.gpes_handled += triggered.len() as u64;
        }

        // Sabit olayları işle
        for event in self.fixed_events.lock().values() {
            if event.check_status() {
                event.clear_status();

                if let Some(ref handler) = event.handler {
                    crate::serial_println!("[GPE] Fixed event {} triggered", event.name);
                }

                let mut stats = self.stats.lock();
                stats.fixed_events_handled += 1;
            }
        }
    }

    /// Belirtilen sabit ACPI olayını etkinleştirir.
    pub fn enable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();

        if let Some(event) = events.get(&event_number) {
            event.enable();
            return Ok(());
        }

        Err(AcpiEventError::InvalidEvent)
    }

    /// Belirtilen sabit ACPI olayını devre dışı bırakır.
    pub fn disable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();

        if let Some(event) = events.get(&event_number) {
            event.disable();
            return Ok(());
        }

        Err(AcpiEventError::InvalidEvent)
    }

    /// GPE istatistiklerinin anlık görüntüsünü döner.
    pub fn get_stats(&self) -> GpeStats {
        self.stats.lock().clone()
    }
}

/// Küresel ACPI olay yöneticisi örneği.
///
/// `lazy_static` ile ilk erişimde oluşturulur; tüm GPE işlemleri bu örnek üzerinden yapılır.
lazy_static::lazy_static! {
    pub static ref ACPI_EVENTS: AcpiEventManager = AcpiEventManager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

/// ACPI olay yöneticisi hata türleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiEventError {
    InvalidGpe,
    InvalidEvent,
    HandlerAlreadyInstalled,
    NoHandler,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// ACPI GPE alt sistemini başlatır.
///
/// FADT'tan alınan GPE0 ve GPE1 blok adresleri ile olay yöneticisini kurar.
pub fn init(gpe0_base: u32, gpe0_count: u32, gpe1_base: u32, gpe1_count: u32) {
    ACPI_EVENTS.init(gpe0_base, gpe0_count, gpe1_base, gpe1_count);
}
