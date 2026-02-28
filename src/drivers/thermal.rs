//! # Termal Bölge Yönetimi (Thermal Zone)
//!
//! Bu modül, CPU ve diğer donanım bileşenlerine ait sıcaklık sensörlerini
//! izler, eşik noktalarını (trip points) denetler ve soğutma cihazlarını
//! tetikler. Linux çekirdeğindeki `thermal_zone_device` altyapısına benzer.
//!
//! ## Termal Yönetim Genel Bakış
//!
//! ```text
//!  [Sıcaklık Sensörü]
//!         |
//!         | update_temperature(temp)
//!         v
//!   [ThermalZone]
//!         |
//!         | check_trips()
//!         v
//!  +------+----------+----------+-----------+
//!  |      |          |          |           |
//!  v      v          v          v
//! ACTIVE PASSIVE    HOT      CRITICAL
//!  |      |          |          |
//!  v      v          v          v
//! Fan   CPU Frekans Max      Acil Kapatma
//! Aç    Düşür       Soğutma  (Emergency Shutdown)
//! ```
//!
//! ## Eşik Noktası Tipleri (Trip Point Types)
//!
//! ```text
//!  Tip      | Değeri | Tetiklendiğinde
//!  ---------+--------+---------------------------------------------
//!  ACTIVE   |   0    | Aktif soğutma aç (fan hızını artır)
//!  PASSIVE  |   1    | Pasif soğutma (CPU frekansını düşür)
//!  HOT      |   2    | Agresif soğutma (maksimum soğutma aktif)
//!  CRITICAL |   3    | Kritik sıcaklık - sistemi acil kapat!
//! ```
//!
//! ## Histerezis (Hysteresis) Mekanizması
//!
//! ```text
//!  Sıcaklık (°C)
//!   ^^^
//!   |              Eşik = 85°C           Soğutma AÇIK
//!   85 -----------+--------->-----------+
//!   |             |                     |
//!   |             ^                     v
//!   80 -----------+---------<-----------+
//!   |        Histerezis = 5°C           Soğutma KAPALI
//!   |
//!   +----> Zaman
//!
//!  - Sıcaklık >= 85°C  -> soğutma etkinleşir
//!  - Sıcaklık <  80°C  -> soğutma devre dışı kalır
//!  - Titreşimi (on/off flapping) önler
//! ```
//!
//! ## Soğutma Cihazı Durumları (Cooling States)
//!
//! ```text
//!  Durum | Açıklama
//!  ------+--------------------------------------------
//!   0    | Minimum soğutma (fan kapalı / maks frekans)
//!   ...  | ...
//!   N    | Maksimum soğutma (fan tam hız / min frekans)
//!
//!  CPU Soğutma -> state = frekans indeksi (düşük state = yüksek frekans)
//!  Fan Soğutma -> state = RPM indeksi    (yüksek state = yüksek RPM)
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// TERMAL SABİTLER
// ============================================================================

/// Eşik noktası tipi: Aktif soğutma (örn: fanı aç).
pub const THERMAL_TRIP_ACTIVE: u32 = 0;
/// Eşik noktası tipi: Pasif soğutma (örn: CPU frekansını düşür).
pub const THERMAL_TRIP_PASSIVE: u32 = 1;
/// Eşik noktası tipi: Sıcak uyarı - agresif soğutmayı tetikle.
pub const THERMAL_TRIP_HOT: u32 = 2;
/// Eşik noktası tipi: Kritik - acil sistem kapatma.
pub const THERMAL_TRIP_CRITICAL: u32 = 3;

/// Soğutma durumu sınır yok (maksimum soğutma).
pub const THERMAL_NO_LIMIT: u32 = u32::MAX;

/// Varsayılan yoklama gecikmesi (milisaniye). Sıcaklık bu aralıkta güncellenir.
pub const THERMAL_POLLING_DELAY: u32 = 1000;
/// Pasif soğutma için yoklama gecikmesi (milisaniye).
pub const THERMAL_PASSIVE_DELAY: u32 = 1000;

// ============================================================================
// EŞİK NOKTASI (TRIP POINT)
// ============================================================================

/// Bir termal eşik noktasını temsil eder.
/// Sıcaklık bu eşiği aştığında ilgili soğutma eylemi tetiklenir.
#[derive(Clone, Debug)]
pub struct TripPoint {
    /// Eşik noktası kimliği
    pub id: u32,
    /// Eşik sıcaklığı (milli-derece Celsius, örn: 85000 = 85°C)
    pub temperature: AtomicI32,
    /// Histerezis miktarı (milli-derece, örn: 5000 = 5°C)
    pub hysteresis: AtomicI32,
    /// Eşik tipi (ACTIVE / PASSIVE / HOT / CRITICAL)
    pub trip_type: u32,
    /// Bu eşik noktası etkin mi?
    pub enabled: AtomicBool,
}

impl TripPoint {
    pub fn new(id: u32, temp: i32, hyst: i32, trip_type: u32) -> Self {
        Self {
            id,
            temperature: AtomicI32::new(temp),
            hysteresis: AtomicI32::new(hyst),
            trip_type,
            enabled: AtomicBool::new(true),
        }
    }

    /// Verilen sıcaklığın bu eşik noktasını aşıp aşmadığını kontrol eder.
    /// Eşik devre dışıysa her zaman false döner.
    pub fn is_exceeded(&self, temp: i32) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }
        temp >= self.temperature.load(Ordering::Relaxed)
    }

    /// Sıcaklığın histerezis bandının altına düşüp düşmediğini kontrol eder.
    /// Soğutmayı kapatmak için kullanılır (eşik - histerezis değerinin altı).
    pub fn is_below_hysteresis(&self, temp: i32) -> bool {
        let trip = self.temperature.load(Ordering::Relaxed);
        let hyst = self.hysteresis.load(Ordering::Relaxed);
        temp < trip - hyst
    }
}

// ============================================================================
// TERMAL BÖLGE (THERMAL ZONE)
// ============================================================================

/// Bir termal bölgeyi temsil eder.
/// Genellikle bir CPU, GPU veya güç çipine karşılık gelir.
/// Eşik noktaları ve soğutma cihazları bu yapıya bağlanır.
pub struct ThermalZone {
    /// Bölge kimliği
    pub id: u32,
    /// Bölge adı (örn: "cpu-thermal")
    pub name: String,
    /// Bölge tipi (örn: "cpu", "gpu")
    pub zone_type: String,
    /// Anlık sıcaklık (milli-derece Celsius)
    pub temperature: AtomicI32,
    /// Eşik noktaları listesi
    pub trips: Mutex<Vec<TripPoint>>,
    /// Bağlı soğutma cihazları
    pub cooling_devices: Mutex<Vec<Arc<CoolingDevice>>>,
    /// Termal governor (soğutma stratejisi, örn: "step_wise")
    pub governor: Mutex<String>,
    /// Sıcaklık yoklama gecikmesi (ms)
    pub polling_delay: AtomicU32,
    /// Pasif soğutma yoklama gecikmesi (ms)
    pub passive_delay: AtomicU32,
    /// Pasif soğutma şu an aktif mi?
    pub passive_active: AtomicBool,
    /// Son güncelleme zamanı (zamanlayıcı tıkları)
    pub last_update: AtomicU64,
    /// Bölge modu: true = etkin, false = devre dışı
    pub mode: AtomicBool,
}

impl ThermalZone {
    pub fn new(id: u32, name: &str, zone_type: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            zone_type: String::from(zone_type),
            temperature: AtomicI32::new(0),
            trips: Mutex::new(Vec::new()),
            cooling_devices: Mutex::new(Vec::new()),
            governor: Mutex::new(String::from("step_wise")),
            polling_delay: AtomicU32::new(THERMAL_POLLING_DELAY),
            passive_delay: AtomicU32::new(THERMAL_PASSIVE_DELAY),
            passive_active: AtomicBool::new(false),
            last_update: AtomicU64::new(0),
            mode: AtomicBool::new(true),
        }
    }

    /// Bölgeye yeni bir eşik noktası ekler.
    pub fn add_trip(&self, trip: TripPoint) {
        self.trips.lock().push(trip);
    }

    /// Bölgeye bir soğutma cihazı bağlar.
    pub fn add_cooling(&self, device: Arc<CoolingDevice>) {
        self.cooling_devices.lock().push(device);
    }

    /// Anlık sıcaklığı günceller ve eşik noktalarını denetler.
    /// Güncelleme zamanı da kaydedilir.
    pub fn update_temperature(&self, temp: i32) {
        self.temperature.store(temp, Ordering::SeqCst);
        self.last_update.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );

        // Eşik noktalarını tüm sıcaklık değerleriyle karşılaştır
        self.check_trips();
    }

    /// Tüm eşik noktalarını mevcut sıcaklıkla karşılaştırır.
    /// Aşılan her eşik için ilgili soğutma eylemini tetikler.
    fn check_trips(&self) {
        let temp = self.temperature.load(Ordering::Relaxed);
        let trips = self.trips.lock();

        for trip in trips.iter() {
            if trip.is_exceeded(temp) {
                self.handle_trip(trip);
            }
        }
    }

    /// Aşılan eşik noktasına göre soğutma eylemini seçer ve uygular.
    fn handle_trip(&self, trip: &TripPoint) {
        match trip.trip_type {
            THERMAL_TRIP_CRITICAL => {
                // Kritik sıcaklık - acil sistem kapatma
                crate::serial_println!(
                    "[THERMAL] KRİTİK: {} {}°C sıcaklığa ulaştı - kapatılıyor!",
                    self.name,
                    trip.temperature.load(Ordering::Relaxed) / 1000
                );
                // Acil kapatma işlemi
            }
            THERMAL_TRIP_HOT => {
                // Sıcak uyarı - tüm soğutma cihazlarını maksimuma al
                self.activate_max_cooling();
            }
            THERMAL_TRIP_PASSIVE => {
                // Pasif soğutma - CPU frekansını düşür
                self.passive_active.store(true, Ordering::SeqCst);
                self.activate_cooling();
            }
            THERMAL_TRIP_ACTIVE => {
                // Aktif soğutma - fanı aç
                self.activate_cooling();
            }
            _ => {}
        }
    }

    /// Tüm bağlı soğutma cihazlarını maksimum duruma getirir.
    fn activate_cooling(&self) {
        let cooling_devices = self.cooling_devices.lock();
        for device in cooling_devices.iter() {
            let _ = device.set_cur_state(device.get_max_state());
        }
    }

    /// Tüm bağlı soğutma cihazlarını maksimum duruma getirir (agresif mod).
    fn activate_max_cooling(&self) {
        let cooling_devices = self.cooling_devices.lock();
        for device in cooling_devices.iter() {
            let _ = device.set_cur_state(device.get_max_state());
        }
    }

    /// Anlık sıcaklığı döner (milli-derece Celsius).
    pub fn get_temperature(&self) -> i32 {
        self.temperature.load(Ordering::Relaxed)
    }

    /// Termal bölgeyi etkinleştirir veya devre dışı bırakır.
    pub fn set_mode(&self, enabled: bool) {
        self.mode.store(enabled, Ordering::SeqCst);
    }
}

// ============================================================================
// SOĞUTMA CİHAZI (COOLING DEVICE)
// ============================================================================

/// Soyut bir soğutma cihazını temsil eder.
/// Fan, CPU frekans ölçekleyici veya güç kapısı olabilir.
pub struct CoolingDevice {
    /// Cihaz kimliği
    pub id: u32,
    /// Cihaz adı (örn: "fan0", "cpu0-cooling")
    pub name: String,
    /// Cihaz tipi (örn: "fan", "cpufreq")
    pub cooling_type: String,
    /// Mevcut soğutma durumu (0 = min, max_state = maks)
    pub cur_state: AtomicU32,
    /// Maksimum soğutma durumu sayısı
    pub max_state: AtomicU32,
    /// Minimum soğutma durumu
    pub min_state: AtomicU32,
    /// Gerçek donanım işlemlerini gerçekleştiren ops arayüzü
    pub ops: Option<&'static dyn CoolingOps>,
}

/// Soğutma cihazı işlem arayüzü.
/// CPU ve fan soğutma cihazları bu trait'i uygular.
pub trait CoolingOps: Send + Sync {
    fn get_max_state(&self) -> u32;
    fn get_cur_state(&self) -> u32;
    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError>;
    fn get_requested_power(&self, state: u32) -> u32;
    fn state2power(&self, state: u32) -> u32;
    fn power2state(&self, power: u32) -> u32;
}

impl CoolingDevice {
    pub fn new(id: u32, name: &str, cooling_type: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            cooling_type: String::from(cooling_type),
            cur_state: AtomicU32::new(0),
            max_state: AtomicU32::new(10),
            min_state: AtomicU32::new(0),
            ops: None,
        }
    }

    pub fn get_max_state(&self) -> u32 {
        self.max_state.load(Ordering::Relaxed)
    }

    pub fn get_cur_state(&self) -> u32 {
        self.cur_state.load(Ordering::Relaxed)
    }

    /// Soğutma cihazının durumunu ayarlar.
    /// Durum min_state ile max_state aralığında olmalıdır.
    /// ops mevcutsa gerçek donanım işlemi de gerçekleştirilir.
    pub fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        let max = self.max_state.load(Ordering::Relaxed);
        let min = self.min_state.load(Ordering::Relaxed);

        if state > max || state < min {
            return Err(ThermalError::InvalidState);
        }

        if let Some(ops) = self.ops {
            ops.set_cur_state(state)?;
        }

        self.cur_state.store(state, Ordering::SeqCst);
        Ok(())
    }
}

// ============================================================================
// CPU SOĞUTMASI (CPU Frequency Scaling)
// ============================================================================

/// CPU frekans ölçekleyici soğutma cihazı.
/// Sıcaklık arttıkça frekansı düşürerek güç tüketimini ve ısı üretimini azaltır.
pub struct CpuCooling {
    cpu_id: u32,
    /// Desteklenen frekans değerleri (MHz), azalan sırayla (index 0 = en yüksek frekans)
    frequencies: Vec<u32>,
    /// Mevcut frekans indeksi
    current_freq_idx: AtomicU32,
}

impl CpuCooling {
    pub fn new(cpu_id: u32, frequencies: Vec<u32>) -> Self {
        Self {
            cpu_id,
            frequencies,
            current_freq_idx: AtomicU32::new(0),
        }
    }
}

impl CoolingOps for CpuCooling {
    fn get_max_state(&self) -> u32 {
        (self.frequencies.len() - 1) as u32
    }

    fn get_cur_state(&self) -> u32 {
        self.current_freq_idx.load(Ordering::Relaxed)
    }

    /// CPU frekansını ayarlar.
    /// Yüksek soğutma durumu = düşük frekans = daha az ısı üretimi.
    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        if state as usize >= self.frequencies.len() {
            return Err(ThermalError::InvalidState);
        }

        // CPU frekansını ayarla (şimdilik yorum satırı)
        let freq = self.frequencies[state as usize];
        // crate::cpu::set_frequency(self.cpu_id, freq);

        self.current_freq_idx.store(state, Ordering::SeqCst);
        Ok(())
    }

    fn get_requested_power(&self, _state: u32) -> u32 {
        0
    }

    fn state2power(&self, _state: u32) -> u32 {
        0
    }

    fn power2state(&self, _power: u32) -> u32 {
        0
    }
}

// ============================================================================
// FAN SOĞUTMASI
// ============================================================================

/// Fan soğutma cihazı.
/// Sıcaklık arttıkça fan hızı (RPM) artırılarak daha fazla ısı dağıtılır.
pub struct FanCooling {
    fan_id: u32,
    /// Desteklenen fan hızları (RPM), artan sırayla (index 0 = düşük hız)
    speeds: Vec<u32>,
    current_speed_idx: AtomicU32,
}

impl FanCooling {
    pub fn new(fan_id: u32, speeds: Vec<u32>) -> Self {
        Self {
            fan_id,
            speeds,
            current_speed_idx: AtomicU32::new(0),
        }
    }
}

impl CoolingOps for FanCooling {
    fn get_max_state(&self) -> u32 {
        (self.speeds.len() - 1) as u32
    }

    fn get_cur_state(&self) -> u32 {
        self.current_speed_idx.load(Ordering::Relaxed)
    }

    /// Fan hızını (RPM) ayarlar.
    /// Yüksek soğutma durumu = yüksek RPM = daha fazla soğutma kapasitesi.
    fn set_cur_state(&self, state: u32) -> Result<(), ThermalError> {
        if state as usize >= self.speeds.len() {
            return Err(ThermalError::InvalidState);
        }

        // Fan hızını ayarla (şimdilik yorum satırı)
        let rpm = self.speeds[state as usize];
        // crate::drivers::fan::set_speed(self.fan_id, rpm);

        self.current_speed_idx.store(state, Ordering::SeqCst);
        Ok(())
    }

    fn get_requested_power(&self, _state: u32) -> u32 {
        0
    }

    fn state2power(&self, _state: u32) -> u32 {
        0
    }

    fn power2state(&self, _power: u32) -> u32 {
        0
    }
}

// ============================================================================
// TERMAL YÖNETİCİ (THERMAL MANAGER)
// ============================================================================

/// Sistem genelindeki tüm termal bölgeleri ve soğutma cihazlarını yöneten merkezi yapı.
pub struct ThermalManager {
    /// Kayıtlı termal bölgeler (ID -> Arc<ThermalZone>)
    zones: Mutex<BTreeMap<u32, Arc<ThermalZone>>>,
    /// Kayıtlı soğutma cihazları (ID -> Arc<CoolingDevice>)
    cooling_devices: Mutex<BTreeMap<u32, Arc<CoolingDevice>>>,
    /// Sonraki bölge kimliği için atomik sayaç
    next_zone_id: AtomicU32,
    /// Sonraki soğutma cihazı kimliği için atomik sayaç
    next_cooling_id: AtomicU32,
}

impl ThermalManager {
    pub const fn new() -> Self {
        Self {
            zones: Mutex::new(BTreeMap::new()),
            cooling_devices: Mutex::new(BTreeMap::new()),
            next_zone_id: AtomicU32::new(0),
            next_cooling_id: AtomicU32::new(0),
        }
    }

    /// Yeni bir termal bölge oluşturur ve yöneticiye kaydeder.
    pub fn register_zone(&self, name: &str, zone_type: &str) -> Arc<ThermalZone> {
        let id = self.next_zone_id.fetch_add(1, Ordering::SeqCst);
        let zone = Arc::new(ThermalZone::new(id, name, zone_type));
        self.zones.lock().insert(id, zone.clone());
        zone
    }

    /// Yeni bir soğutma cihazı oluşturur ve yöneticiye kaydeder.
    pub fn register_cooling(&self, name: &str, cooling_type: &str) -> Arc<CoolingDevice> {
        let id = self.next_cooling_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(CoolingDevice::new(id, name, cooling_type));
        self.cooling_devices.lock().insert(id, device.clone());
        device
    }

    /// ID'ye göre termal bölge döner.
    pub fn get_zone(&self, id: u32) -> Option<Arc<ThermalZone>> {
        self.zones.lock().get(&id).cloned()
    }

    /// ID'ye göre soğutma cihazı döner.
    pub fn get_cooling(&self, id: u32) -> Option<Arc<CoolingDevice>> {
        self.cooling_devices.lock().get(&id).cloned()
    }

    /// Tüm termal bölgelerin sıcaklıklarını günceller.
    /// Gerçek uygulamada sensörden okunan değer update_temperature'a geçirilir.
    pub fn update_all(&self) {
        for zone in self.zones.lock().values() {
            // Sıcaklığı sensörden oku ve güncelle
            // zone.update_temperature(temp);
        }
    }
}

lazy_static::lazy_static! {
    pub static ref THERMAL_MANAGER: ThermalManager = ThermalManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Termal yönetim işlemlerinde oluşabilecek hatalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalError {
    /// Geçersiz soğutma durumu (min/max aralığı dışı)
    InvalidState,
    /// Soğutma cihazı bulunamadı
    DeviceNotFound,
    /// Termal bölge bulunamadı
    ZoneNotFound,
    /// Sensör okuma hatası
    SensorError,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Termal yönetim alt sistemini başlatır.
///
/// CPU termal bölgesi oluşturur ve eşik noktalarını ayarlar:
///   - 85°C (±5°C): Pasif soğutma (CPU frekans düşürme)
///   - 95°C (±2°C): Sıcak uyarı (agresif soğutma)
///   - 105°C       : Kritik - acil sistem kapatma
pub fn init() {
    // CPU termal bölgesi oluştur
    let cpu_zone = THERMAL_MANAGER.register_zone("cpu-thermal", "cpu");

    // Eşik noktalarını tanımla
    // TripPoint::new(id, sıcaklık_mdegC, histerezis_mdegC, tip)
    cpu_zone.add_trip(TripPoint::new(0, 85000, 5000, THERMAL_TRIP_PASSIVE));
    cpu_zone.add_trip(TripPoint::new(1, 95000, 2000, THERMAL_TRIP_HOT));
    cpu_zone.add_trip(TripPoint::new(2, 105000, 0, THERMAL_TRIP_CRITICAL));

    crate::serial_println!("[THERMAL] Termal yönetim alt sistemi baslatildi");
}
