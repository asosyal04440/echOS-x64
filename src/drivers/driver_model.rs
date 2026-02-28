//! # Sürücü Modeli (Driver Model)
//!
//! Linux çekirdeğinden ilham alınan cihaz kaydı ve sysfs arayüzü.
//!
//! ## Genel Kavramlar
//!
//! Linux'ta her donanım birimi şu hiyerarşiyle temsil edilir:
//!
//! ```
//! DeviceClass (örn. "block", "net")
//!      |
//!      +-- Device (örn. "sda", "eth0")
//!               |
//!               +-- Driver (probe/remove/suspend/resume)
//! ```
//!
//! Bu modül:
//! - Cihazların sisteme kayıt edilmesini sağlar
//! - Sürücülerin cihazlara bağlanmasını (bind) yönetir
//! - Güç yönetimi (suspend/resume) çerçevesi sunar
//! - sysfs benzeri (/sys/class/<sınıf>/<cihaz>/<özellik>) bir arayüz sağlar

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::{Mutex, RwLock};

// ============================================================================
// CİHAZ TÜRLERİ (DEVICE TYPES)
// ============================================================================

// Linux'taki device class kavramını yansıtır.
// Her cihaz türü farklı bir alt sistemde yönetilir:
//   Char  -> /dev/ttyS0, /dev/input/...
//   Block -> /dev/sda, /dev/nvme0n1
//   Net   -> eth0, wlan0
//   Gpu   -> /dev/dri/card0

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceType {
    Char,
    Block,
    Net,
    Misc,
    Usb,
    Pci,
    Platform,
    I2C,
    Spi,
    Hwmon,
    Gpu,
    Audio,
    Input,
    Gpio,
    Thermal,
}

impl DeviceType {
    pub fn name(&self) -> &'static str {
        match self {
            DeviceType::Char => "char",
            DeviceType::Block => "block",
            DeviceType::Net => "net",
            DeviceType::Misc => "misc",
            DeviceType::Usb => "usb",
            DeviceType::Pci => "pci",
            DeviceType::Platform => "platform",
            DeviceType::I2C => "i2c",
            DeviceType::Spi => "spi",
            DeviceType::Hwmon => "hwmon",
            DeviceType::Gpu => "gpu",
            DeviceType::Audio => "audio",
            DeviceType::Input => "input",
            DeviceType::Gpio => "gpio",
            DeviceType::Thermal => "thermal",
        }
    }
}

// ============================================================================
// CİHAZ YAPISI (DEVICE)
// ============================================================================

// Her fiziksel ya da sanal donanım birimini temsil eder.
// Linux kernel'deki `struct device` yapısına karşılık gelir.
//
// Örnek cihaz hiyerarşisi:
//
//   [PCI Bus]
//      |
//      +-- [GPU: id=5, type=Gpu]
//               |-- driver: Mutex<Some(driver_id)>
//               |-- attrs: {"vendor"="0x1002", "model"="RX580"}
//               +-- children: [HDMI #0, HDMI #1]

pub struct Device {
    /// Sistemdeki benzersiz cihaz kimliği (monoton artan sayaç)
    pub id: u64,
    /// İnsan okunabilir cihaz adı (örn. "nvme0", "eth0")
    pub name: String,
    /// Cihazın hangi alt sisteme ait olduğu
    pub dev_type: DeviceType,
    /// Üst cihazın ID'si (yoksa None, PCI kök için None)
    pub parent: Option<u64>,
    /// Alt cihazların ID listesi
    pub children: Mutex<Vec<u64>>,
    /// Bu cihaza bağlı sürücünün ID'si (bağlı değilse None)
    pub driver: Mutex<Option<u64>>,
    /// sysfs tarzı anahtar-değer özellik haritası
    pub attrs: Mutex<BTreeMap<String, String>>,
    /// Cihaz başarıyla probe edildikten sonra true olur
    pub initialized: AtomicBool,
    /// Güç yönetimi: sleep/hibernate sırasında true
    pub suspended: AtomicBool,
    /// Arc referans sayacı (drop zamanı için)
    pub ref_count: AtomicU32,
    /// Sürücüye özgü özel veri alanı (pointer olarak saklanır)
    pub private: Mutex<Option<u64>>,
}

impl Device {
    pub fn new(id: u64, name: &str, dev_type: DeviceType) -> Self {
        Self {
            id,
            name: String::from(name),
            dev_type,
            parent: None,
            children: Mutex::new(Vec::new()),
            driver: Mutex::new(None),
            attrs: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            ref_count: AtomicU32::new(1),
            private: Mutex::new(None),
        }
    }

    /// sysfs özelliğini günceller (örn. "vendor" = "0x8086")
    pub fn set_attr(&self, name: &str, value: &str) {
        self.attrs.lock().insert(String::from(name), String::from(value));
    }

    /// sysfs özelliğini okur; yoksa None döner
    pub fn get_attr(&self, name: &str) -> Option<String> {
        self.attrs.lock().get(name).cloned()
    }

    /// Alt cihaz ekler (örn. PCI bridge'e bağlanan GPU)
    pub fn add_child(&self, child_id: u64) {
        self.children.lock().push(child_id);
    }

    /// Sürücüyü cihaza bağlar; sürücü probe başarılıysa çağrılır
    pub fn bind_driver(&self, driver_id: u64) {
        *self.driver.lock() = Some(driver_id);
    }
}

// ============================================================================
// SÜRÜCÜ YAPISI (DRIVER)
// ============================================================================

// Linux'taki `struct device_driver` yapısına karşılık gelir.
// Bir sürücü birden fazla cihaza bağlanabilir (örn. e1000 sürücüsü
// birden fazla Intel Ethernet kartını yönetebilir).
//
// Sürücü yaşam döngüsü:
//
//   register_driver()
//       |
//       v
//   probe(device)  ---> başarısızlık: ProbeFailed
//       |
//       v
//   [Çalışıyor] <---> suspend() / resume()
//       |
//       v
//   remove(device) ya da shutdown(device)

pub struct Driver {
    /// Sistemdeki benzersiz sürücü kimliği
    pub id: u64,
    /// Sürücü adı (örn. "e1000", "nvme", "xhci_hcd")
    pub name: String,
    /// Bu sürücünün yönettiği cihaz ID'leri
    pub devices: Mutex<Vec<u64>>,
    /// Donanım başlatma fonksiyonu: cihaz bulunduğunda çağrılır
    pub probe_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Donanım kapatma fonksiyonu: cihaz kaldırıldığında çağrılır
    pub remove_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Güç tasarrufu moduna alırken çağrılır
    pub suspend_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Uyku modundan uyandırırken çağrılır
    pub resume_fn: Option<fn(&Device) -> Result<(), DriverError>>,
    /// Sistem kapatılırken çağrılır (hata dönmez)
    pub shutdown_fn: Option<fn(&Device)>,
}

impl Driver {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            devices: Mutex::new(Vec::new()),
            probe_fn: None,
            remove_fn: None,
            suspend_fn: None,
            resume_fn: None,
            shutdown_fn: None,
        }
    }

    /// Cihazı probe eder ve sürücüye bağlar.
    /// probe_fn başarılı olursa `initialized` bayrağı true yapılır.
    pub fn probe(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(probe) = self.probe_fn {
            probe(device)?;
        }
        device.initialized.store(true, Ordering::SeqCst);
        self.devices.lock().push(device.id);
        Ok(())
    }

    /// Cihazı sürücüden ayırır ve temizler
    pub fn remove(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(remove) = self.remove_fn {
            remove(device)?;
        }
        self.devices.lock().retain(|&id| id != device.id);
        Ok(())
    }

    /// Cihazı uyku moduna alır (ACPI S3 / runtime PM)
    pub fn suspend(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(suspend) = self.suspend_fn {
            suspend(device)?;
        }
        device.suspended.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Cihazı uyku modundan çıkarır
    pub fn resume(&self, device: &Device) -> Result<(), DriverError> {
        if let Some(resume) = self.resume_fn {
            resume(device)?;
        }
        device.suspended.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Sistem kapatılırken son temizliği yapar
    pub fn shutdown(&self, device: &Device) {
        if let Some(shutdown) = self.shutdown_fn {
            shutdown(device);
        }
    }
}

// ============================================================================
// CİHAZ SINIFI (DEVICE CLASS)
// ============================================================================

// Linux'taki /sys/class/<name>/ dizinine karşılık gelir.
// Aynı türden cihazları gruplayarak üst düzey yönetim sağlar.
// Örneğin "block" sınıfı tüm blok cihazlarını listeler.

pub struct DeviceClass {
    pub name: String,
    pub dev_type: DeviceType,
    pub devices: Mutex<Vec<u64>>,
}

impl DeviceClass {
    pub fn new(name: &str, dev_type: DeviceType) -> Self {
        Self {
            name: String::from(name),
            dev_type,
            devices: Mutex::new(Vec::new()),
        }
    }

    pub fn add_device(&self, device_id: u64) {
        self.devices.lock().push(device_id);
    }
}

// ============================================================================
// SÜRÜCÜ MODELİ YÖNETİCİSİ (DRIVER MODEL MANAGER)
// ============================================================================

// Tüm cihaz ve sürücü kayıtlarını merkezi olarak tutar.
// Linux'taki kobject/kset altyapısını basitleştirilmiş biçimde uygular.
//
// Veri yapısı:
//
//   DriverModel
//     |
//     +-- devices:  BTreeMap<u64, Arc<Device>>   // id -> cihaz
//     +-- drivers:  BTreeMap<u64, Arc<Driver>>   // id -> sürücü
//     +-- classes:  BTreeMap<String, Arc<Class>> // "block" -> sınıf
//     +-- device_names: BTreeMap<String, u64>    // "sda" -> id (hızlı arama)

pub struct DriverModel {
    /// Tüm kayıtlı cihazlar (R/W kilit ile thread-safe)
    devices: RwLock<BTreeMap<u64, Arc<Device>>>,
    /// Tüm kayıtlı sürücüler
    drivers: RwLock<BTreeMap<u64, Arc<Driver>>>,
    /// Sınıf haritası ("block", "net" vb.)
    classes: RwLock<BTreeMap<String, Arc<DeviceClass>>>,
    /// Bir sonraki atanacak cihaz ID'si (atomik; çakışma olmaz)
    next_device_id: AtomicU64,
    /// Bir sonraki atanacak sürücü ID'si
    next_driver_id: AtomicU64,
    /// İsimden ID'ye hızlı erişim haritası
    device_names: Mutex<BTreeMap<String, u64>>,
}

impl DriverModel {
    pub const fn new() -> Self {
        Self {
            devices: RwLock::new(BTreeMap::new()),
            drivers: RwLock::new(BTreeMap::new()),
            classes: RwLock::new(BTreeMap::new()),
            next_device_id: AtomicU64::new(1),
            next_driver_id: AtomicU64::new(1),
            device_names: Mutex::new(BTreeMap::new()),
        }
    }

    /// Yeni cihaz kaydeder; otomatik benzersiz ID atar ve tablolara ekler
    pub fn register_device(&self, name: &str, dev_type: DeviceType) -> Arc<Device> {
        let id = self.next_device_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(Device::new(id, name, dev_type));

        self.devices.write().insert(id, device.clone());
        self.device_names.lock().insert(String::from(name), id);

        crate::serial_println!("[DRIVER] Registered device '{}' (id={})", name, id);

        device
    }

    /// Kaydedilmiş cihazı sistemden kaldırır
    pub fn unregister_device(&self, id: u64) {
        if let Some(device) = self.devices.write().remove(&id) {
            self.device_names.lock().remove(&device.name);
        }
    }

    /// Yeni sürücü kaydeder; probe/remove fonksiyonları sonradan atanabilir
    pub fn register_driver(&self, name: &str) -> Arc<Driver> {
        let id = self.next_driver_id.fetch_add(1, Ordering::SeqCst);
        let driver = Arc::new(Driver::new(id, name));

        self.drivers.write().insert(id, driver.clone());

        crate::serial_println!("[DRIVER] Registered driver '{}' (id={})", name, id);

        driver
    }

    /// Cihaz sınıfı kaydeder (örn. "block", "net")
    pub fn register_class(&self, name: &str, dev_type: DeviceType) -> Arc<DeviceClass> {
        let class = Arc::new(DeviceClass::new(name, dev_type));
        self.classes.write().insert(String::from(name), class.clone());
        class
    }

    /// Sürücüyü cihaza bağlar: probe() çağrılır, başarılıysa bağlantı kurulur
    pub fn bind(&self, device_id: u64, driver_id: u64) -> Result<(), DriverError> {
        let device = self.devices.read().get(&device_id).cloned()
            .ok_or(DriverError::DeviceNotFound)?;
        let driver = self.drivers.read().get(&driver_id).cloned()
            .ok_or(DriverError::DriverNotFound)?;

        driver.probe(&device)?;
        device.bind_driver(driver_id);

        crate::serial_println!("[DRIVER] Bound driver {} to device {}", driver_id, device_id);

        Ok(())
    }

    /// Sürücüyü cihazdan ayırır: remove() çağrılır, driver alanı None yapılır
    pub fn unbind(&self, device_id: u64) -> Result<(), DriverError> {
        let device = self.devices.read().get(&device_id).cloned()
            .ok_or(DriverError::DeviceNotFound)?;

        if let Some(driver_id) = *device.driver.lock() {
            if let Some(driver) = self.drivers.read().get(&driver_id) {
                driver.remove(&device)?;
            }
        }

        *device.driver.lock() = None;
        Ok(())
    }

    /// ID ile cihaz arar
    pub fn get_device(&self, id: u64) -> Option<Arc<Device>> {
        self.devices.read().get(&id).cloned()
    }

    /// İsim ile cihaz arar (örn. "eth0" -> Arc<Device>)
    pub fn get_device_by_name(&self, name: &str) -> Option<Arc<Device>> {
        let id = self.device_names.lock().get(name).copied()?;
        self.devices.read().get(&id).cloned()
    }

    /// ID ile sürücü arar
    pub fn get_driver(&self, id: u64) -> Option<Arc<Driver>> {
        self.drivers.read().get(&id).cloned()
    }

    /// Tüm cihazları listeler: (id, ad, tür) üçlüsü döner
    pub fn list_devices(&self) -> Vec<(u64, String, DeviceType)> {
        self.devices.read()
            .iter()
            .map(|(id, dev)| (*id, dev.name.clone(), dev.dev_type))
            .collect()
    }

    /// Tüm cihazları uyku moduna alır (sistem askıya alınırken çağrılır)
    pub fn suspend_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    let _ = driver.suspend(device);
                }
            }
        }
    }

    /// Uyku modundan çıkarken tüm cihazları yeniden başlatır
    pub fn resume_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    let _ = driver.resume(device);
                }
            }
        }
    }

    /// Sistem kapatılırken tüm cihazları kapatır
    pub fn shutdown_all(&self) {
        for device in self.devices.read().values() {
            if let Some(driver_id) = *device.driver.lock() {
                if let Some(driver) = self.drivers.read().get(&driver_id) {
                    driver.shutdown(device);
                }
            }
        }
    }
}

lazy_static::lazy_static! {
    pub static ref DRIVER_MODEL: DriverModel = DriverModel::new();
}

// ============================================================================
// SYSFS BENZERI ARAYÜZLERİ (SYSFS-LIKE INTERFACE)
// ============================================================================

// Linux'ta /sys/ sanal dosya sistemi, cihaz özelliklerini metin dosyaları
// olarak sunar. Örneğin:
//   /sys/class/net/eth0/speed  -> "1000"
//   /sys/class/block/sda/size  -> "976773168"
//
// Bu fonksiyonlar aynı kavramı yol ayrıştırmasıyla simüle eder:
//   okuma:  sysfs_read("/sys/class/net/eth0/speed")
//   yazma:  sysfs_write("/sys/class/block/sda/scheduler", "noop")

/// sysfs özelliği okur; yol formatı: /sys/class/<sınıf>/<cihaz>/<özellik>
pub fn sysfs_read(path: &str) -> Option<String> {
    // Yolu parçalara ayır: ["sys", "class", "<cihaz>", "<özellik>"]
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() < 4 || parts[0] != "sys" {
        return None;
    }

    let device_name = parts[2];
    let attr_name = parts[3];

    if let Some(device) = DRIVER_MODEL.get_device_by_name(device_name) {
        return device.get_attr(attr_name);
    }

    None
}

/// sysfs özelliğine yazar; başarıysa true döner
pub fn sysfs_write(path: &str, value: &str) -> bool {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() < 4 || parts[0] != "sys" {
        return false;
    }

    let device_name = parts[2];
    let attr_name = parts[3];

    if let Some(device) = DRIVER_MODEL.get_device_by_name(device_name) {
        device.set_attr(attr_name, value);
        return true;
    }

    false
}

// ============================================================================
// HATA TÜRLERİ (ERROR TYPES)
// ============================================================================

// Sürücü işlemlerinde dönebilecek hata çeşitleri.
// Linux'ta bunlar negatif errno kodlarına karşılık gelir:
//   DeviceNotFound -> -ENODEV
//   ProbeFailed    -> -EIO veya -ENODEV

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    DeviceNotFound,
    DriverNotFound,
    ProbeFailed,
    RemoveFailed,
    SuspendFailed,
    ResumeFailed,
    NotBound,
}

// ============================================================================
// BAŞLATMA (INITIALIZATION)
// ============================================================================

pub fn init() {
    // Temel cihaz sınıflarını kaydet (Linux'taki /sys/class/ altındaki dizinler)
    DRIVER_MODEL.register_class("char", DeviceType::Char);
    DRIVER_MODEL.register_class("block", DeviceType::Block);
    DRIVER_MODEL.register_class("net", DeviceType::Net);
    DRIVER_MODEL.register_class("misc", DeviceType::Misc);

    crate::serial_println!("[DRIVER] Driver model initialized");
}
