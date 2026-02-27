//! # ACPI Olayları (GPE)
//!
//! Genel Amaçlı Olaylar (General Purpose Events) ve ACPI olay işleme.
//! GPE, ACPI donanım olaylarını (uyandırma, sıcaklık bildirimi vb.) işlemek için
//! kullanılan bir mekanizmadır. GPE blokları, sabit olaylar ve olay yöneticisini içerir.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// GPE SABİTLERİ
// ============================================================================

/// GPE blok yazmaçları
pub const GPE0_BLK: &str = "GPE0_BLK";
pub const GPE1_BLK: &str = "GPE1_BLK";

/// GPE yazmaç ofsetleri
pub const GPE_STS_OFFSET: usize = 0;
pub const GPE_EN_OFFSET: usize = 2;

/// GPE olay tipleri
pub const GPE_TYPE_WAKE: u8 = 0x01;
pub const GPE_TYPE_RUNTIME: u8 = 0x02;
pub const GPE_TYPE_WAKE_RUNTIME: u8 = 0x03;

/// Sabit olay numaraları
pub const ACPI_EVENT_PMTIMER: u32 = 0;
pub const ACPI_EVENT_POWER_BUTTON: u32 = 2;
pub const ACPI_EVENT_SLEEP_BUTTON: u32 = 3;
pub const ACPI_EVENT_RTC: u32 = 4;

// ============================================================================
// GPE OLAYI
// ============================================================================

#[derive(Clone, Debug)]
pub struct GpeEvent {
    /// GPE numarası
    pub number: u32,
    /// GPE bloğu (0 veya 1)
    pub block: u8,
    /// Olay tipi
    pub event_type: u8,
    /// İşleyici metod
    pub handler: Option<String>,
    /// Etkin mi
    pub enabled: AtomicBool,
    /// Uyandırma yapabilir mi
    pub wake_capable: AtomicBool,
    /// İşleyici tipi (0=yok, 1=metod, 2=işleyici)
    pub handler_type: AtomicU32,
}

impl GpeEvent {
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

    /// Olayı etkinleştir
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    /// Olayı devre dışı bırak
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Uyandırma kapasitesini ayarla
    pub fn set_wake(&self, wake: bool) {
        self.wake_capable.store(wake, Ordering::SeqCst);
    }
}

// ============================================================================
// SABİT OLAY
// ============================================================================

#[derive(Clone, Debug)]
pub struct FixedEvent {
    /// Olay numarası
    pub number: u32,
    /// Olay adı
    pub name: String,
    /// Durum kayıt adresi
    pub status_reg: u32,
    /// Etkinleştirme kayıt adresi
    pub enable_reg: u32,
    /// İşleyici metod
    pub handler: Option<String>,
    /// Etkin mi
    pub enabled: AtomicBool,
}

impl FixedEvent {
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

    /// Olayı etkinleştir
    pub fn enable(&self) {
        // Etkinleştirme kaydına yaz
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[GPE] Fixed event {} enabled", self.name);
    }

    /// Olayı devre dışı bırak
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Durumu kontrol et
    pub fn check_status(&self) -> bool {
        // Durum kaydını oku
        false
    }

    /// Durumu temizle
    pub fn clear_status(&self) {
        // Durum kaydına yaz
    }
}

// ============================================================================
// GPE BLOĞU
// ============================================================================

pub struct GpeBlock {
    /// Blok numarası
    pub block_number: u8,
    /// Taban adresi
    pub base_address: u32,
    /// GPE sayısı
    pub gpe_count: u32,
    /// GPE olayları
    pub events: Mutex<BTreeMap<u32, GpeEvent>>,
    /// Durum kaydı önbellek değeri
    pub status_cache: AtomicU32,
    /// Etkinleştirme kaydı önbellek değeri
    pub enable_cache: AtomicU32,
}

impl GpeBlock {
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

    /// GPE olaylarını başlat
    pub fn init(&self) {
        let mut events = self.events.lock();

        for i in 0..self.gpe_count {
            events.insert(i, GpeEvent::new(i, self.block_number));
        }
    }

    /// GPE olayını al
    pub fn get_event(&self, number: u32) -> Option<GpeEvent> {
        self.events.lock().get(&number).cloned()
    }

    /// GPE'yi etkinleştir
    pub fn enable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.enable();

            // Etkinleştirme kaydını güncelle
            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_or(bit, Ordering::SeqCst);

            crate::serial_println!("[GPE] GPE{} enabled", number);
        }
    }

    /// GPE'yi devre dışı bırak
    pub fn disable_gpe(&self, number: u32) {
        if let Some(event) = self.events.lock().get(&number) {
            event.disable();

            let bit = 1u32 << (number % 32);
            self.enable_cache.fetch_and(!bit, Ordering::SeqCst);
        }
    }

    /// GPE durumunu temizle
    pub fn clear_gpe(&self, number: u32) {
        let bit = 1u32 << (number % 32);
        self.status_cache.fetch_and(!bit, Ordering::SeqCst);
    }

    /// GPE olaylarını işle
    pub fn handle_events(&self) -> Vec<u32> {
        let mut triggered = Vec::new();

        // Durum kaydını oku
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
                        // AML metodunu çalıştır
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

pub struct AcpiEventManager {
    /// GPE blokları
    pub gpe_blocks: Mutex<Vec<GpeBlock>>,
    /// Sabit olaylar
    pub fixed_events: Mutex<BTreeMap<u32, FixedEvent>>,
    /// Olay işleyicileri
    pub handlers: Mutex<BTreeMap<String, Arc<dyn AcpiEventHandler>>>,
    /// Başlatıldı mı
    pub initialized: AtomicBool,
    /// İstatistikler
    pub stats: Mutex<GpeStats>,
}

#[derive(Clone, Debug, Default)]
pub struct GpeStats {
    pub gpes_handled: u64,
    pub fixed_events_handled: u64,
    pub spurious_events: u64,
}

pub trait AcpiEventHandler: Send + Sync {
    fn handle(&self, event: u32) -> Result<(), AcpiEventError>;
}

impl AcpiEventManager {
    pub const fn new() -> Self {
        Self {
            gpe_blocks: Mutex::new(Vec::new()),
            fixed_events: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            stats: Mutex::new(GpeStats::default()),
        }
    }

    /// FADT'den başlat
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

        // Sabit olayları başlat
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

    /// GPE işleyicisi yükle
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

    /// GPE'yi etkinleştir
    pub fn enable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();

        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.enable_gpe(gpe_number);
            return Ok(());
        }

        Err(AcpiEventError::InvalidGpe)
    }

    /// GPE'yi devre dışı bırak
    pub fn disable_gpe(&self, gpe_number: u32, block: u8) -> Result<(), AcpiEventError> {
        let blocks = self.gpe_blocks.lock();

        if let Some(gpe_block) = blocks.iter().find(|b| b.block_number == block) {
            gpe_block.disable_gpe(gpe_number);
            return Ok(());
        }

        Err(AcpiEventError::InvalidGpe)
    }

    /// Tüm olayları işle
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

    /// Sabit olayı etkinleştir
    pub fn enable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();

        if let Some(event) = events.get(&event_number) {
            event.enable();
            return Ok(());
        }

        Err(AcpiEventError::InvalidEvent)
    }

    /// Sabit olayı devre dışı bırak
    pub fn disable_fixed_event(&self, event_number: u32) -> Result<(), AcpiEventError> {
        let events = self.fixed_events.lock();

        if let Some(event) = events.get(&event_number) {
            event.disable();
            return Ok(());
        }

        Err(AcpiEventError::InvalidEvent)
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> GpeStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref ACPI_EVENTS: AcpiEventManager = AcpiEventManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

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

pub fn init(gpe0_base: u32, gpe0_count: u32, gpe1_base: u32, gpe1_count: u32) {
    ACPI_EVENTS.init(gpe0_base, gpe0_count, gpe1_base, gpe1_count);
}
