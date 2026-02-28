//! # Linux Glue — Linux Sürücü ABI Kopyası
//!
//! Bu modül, Linux çekirdeğinin C yapılarını Rust'ta yeniden tanımlar.
//! Amacı, orijinal Linux PCI/DRM sürücü kaynak kodunu hiç değiştirmeden
//! echOS üzerinde derlenip çalıştırılabilir kılmaktır.
//!
//! ## Temel Kavram: ABI Uyumluluğu
//! Linux sürücüleri, çekirdeğin belirli C yapılarını belirli bellek
//! düzeninde (offset) bekler. Bu modül, tam olarak aynı bellek düzenini
//! `#[repr(C)]` ile yeniden üretir — böylece C ile derlenen sürücü kodu
//! bu yapılara doğru offsetlerde erişir.
//!
//! ## Veri Akışı
//! ```text
//! Linux C Sürücüsü (.ko)
//!   │
//!   ├── pci_register_driver(&pci_drv) ─────────────────── C FFI
//!   │       │
//!   │       ▼
//!   │   linux_glue::pci_register_driver()
//!   │       │
//!   │       ├── echOS PCI tarama → PciDevice listesi
//!   │       ├── ID eşleştirme (vendor/device)
//!   │       ├── create_pci_dev() → Linux PciDev nesnesi oluştur
//!   │       ├── probe(dev, id) → Sürücü başlatma
//!   │       └── claim_device() → Cihazı talep et
//!   │
//!   └── pci_unregister_driver() → remove() çağrısı
//! ```
//!
//! ## Güvenlik Notu
//! Bu modül `unsafe` kod içerir — Linux'un ham pointer tabanlı arayüzleri
//! zorunlu kılar. Her `unsafe` blok belirtilmiş gerekçesiyle kullanılır.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void, CStr};
use spin::Mutex;

// ============================================================================
// LINUX ÇEKIRDEĞI C YAPILARI — ABI UYUMLU YENİDEN TANIM
// ============================================================================

/// Linux çekirdeği `struct device` — tüm cihaz türlerinin temel yapısı.
///
/// Linux'ta PciDev, UsbDev vb. tüm yapılar ilk alan olarak `Device` içerir.
/// Bu "kalıtım benzeri" bir C desenidir: parent pointer'ı aracılığıyla
/// cihaz ağacı (device tree) oluşturulur.
///
/// ## Bellek Düzeni (offset'ler)
/// - offset 0x00: `parent` — üst cihaz pointer'ı
/// - offset 0x08: `driver`  — bu cihazı yöneten sürücü
/// - offset 0x10: `driver_data` — sürücüye özel özel veri
#[repr(C)]
pub struct Device {
    pub parent: *mut Device,      // offset = 0x00
    pub driver: *mut c_void,      // offset = 0x08
    pub driver_data: *mut c_void, // offset = 0x10
}

/// Linux PCI kaynak tanımlayıcısı — bir BAR'ın start/end adres ve bayraklarını tutar.
///
/// `IORESOURCE_MEM` bayrağı bellek BAR'ı, `IORESOURCE_IO` port BAR'ı gösterir.
/// Linux sürücüleri `pci_resource_start(dev, bar_index)` ile bu yapıya erişir.
#[repr(C)]
pub struct PciResource {
    /// BAR'ın başlangıç fiziksel adresi
    pub start: u64,
    /// BAR'ın bitiş fiziksel adresi (dahil)
    pub end: u64,
    /// Kaynak bayrakları: IORESOURCE_MEM, IORESOURCE_IO, vs.
    pub flags: u64,
}

/// Linux çekirdeği `struct pci_dev` — PCI cihaz tanımlayıcısı.
///
/// ## İçerdiği Bilgiler
/// - Cihazın temel kimleği (vendor, device, class, revision)
/// - Alt sistem kimleği (subsystem_vendor, subsystem_device)
/// - BAR (Base Address Register) kaynak dizisi: 6 adet BAR
/// - Sürücüye özel veri pointer'ı
///
/// ## Önemli Bellek Hizalaması
/// `#[repr(C)]` ile tanımlanmıştır; her alan Linux'taki C yapısıyla
/// aynı offset'e düşmelidir. Yanlış hizalama sürücüyü çökertir.
#[repr(C)]
pub struct PciDev {
    /// Temel cihaz nesnesi — ilk alan olmalı (Linux tasarım gereği)
    pub dev: Device,           // offset = 0x00
    /// PCI cihazının üretici (vendor) kimliği — örn: 0x8086 = Intel
    pub vendor: u16,           // offset = 0x18
    /// PCI cihazının ürün (device) kimliği
    pub device: u16,           // offset = 0x1A
    /// Alt sistem üretici kimliği
    pub subsystem_vendor: u16, // offset = 0x1C
    /// Alt sistem ürün kimliği
    pub subsystem_device: u16, // offset = 0x1E
    /// Sınıf kodu: 3 baytlık hiyerarşik kod (class/subclass/progif)
    pub class: u32,            // offset = 0x20
    /// Revizyon kodu — donanım versiyonu
    pub revision: u8,          // offset = 0x24
    /// Hizalama dolgusu — yapı boyutunu 4'ün katına tamamlar
    pub _pad0: [u8; 3],
    /// 6 adet BAR (Base Address Register) — MMIO ve Port I/O aralıkları
    pub resource: [PciResource; 6], // offset = 0x28
    /// Sürücüye özel veri pointer'ı — `pci_set_drvdata()` / `pci_get_drvdata()` ile yönetilir
    pub driver_data: *mut c_void,   // offset = 0xB8
}

/// Linux PCI cihaz kimlik tablosu girişi — sürücünün desteklediği cihazlar.
///
/// Her sürücü, desteklediği cihazların `vendor:device` çiftlerini içeren
/// bir `PciDeviceId` tablosu ile birlikte gelir. Tablo `{0,0,...}` ile biter.
///
/// `0xFFFF` değeri "joker" (wildcard) anlamına gelir — herhangi bir değerle eşleşir.
#[repr(C)]
pub struct PciDeviceId {
    /// Üretici kimliği (0xFFFF = joker)
    pub vendor: u16,
    /// Ürün kimliği (0xFFFF = joker)
    pub device: u16,
    /// Alt sistem üretici kimliği (0xFFFF = joker)
    pub subvendor: u16,
    /// Alt sistem ürün kimliği (0xFFFF = joker)
    pub subdevice: u16,
    /// Sınıf kodu (class_mask ile birlikte kullanılır)
    pub class: u32,
    /// Sınıf maskesi — hangi bitlerin class karşılaştırmasına katılacağını belirler
    pub class_mask: u32,
    /// Sürücüye özel veri — probe() içinde kullanılmak üzere
    pub driver_data: usize,
}

/// Linux PCI `probe` fonksiyon işaretçi türü.
/// Bir cihaz eşleştiğinde çağrılır. 0 döndürürse cihaz başarıyla talep edilmiştir.
pub type PciProbeFn = Option<unsafe extern "C" fn(dev: *mut PciDev, id: *const PciDeviceId) -> i32>;

/// Linux PCI `remove` fonksiyon işaretçi türü.
/// Bir cihaz sistemden çıkarıldığında veya sürücü kaldırıldığında çağrılır.
pub type PciRemoveFn = Option<unsafe extern "C" fn(dev: *mut PciDev)>;

/// Linux çekirdeği `struct pci_driver` — bir PCI sürücüsünün tanımı.
///
/// Her Linux PCI sürücüsü bu yapıyı doldurur ve `pci_register_driver()` ile kaydeder.
/// `id_table` boş bir girişle ({vendor:0, device:0, ...}) sonlandırılmalıdır.
#[repr(C)]
pub struct PciDriver {
    /// Sürücü adı — `/sys/bus/pci/drivers/` altında görünen isim
    pub name: *const c_char,
    /// Desteklenen cihaz kimlikleri tablosu — null ile bitmez, son giriş {0,0,...}
    pub id_table: *const PciDeviceId,
    /// Cihaz bulunduğunda çağrılan fonksiyon pointer'ı
    pub probe: PciProbeFn,
    /// Cihaz kaldırıldığında çağrılan fonksiyon pointer'ı
    pub remove: PciRemoveFn,
}

/// Bir sürücü tarafından talep edilmiş (claimed) PCI cihaz kaydı.
/// CLAIMED_DEVICES listesinde tutulur; aynı cihazın iki kez talep edilmesini önler.
struct ClaimedDevice {
    /// PCI bus numarası
    pub bus: u8,
    /// PCI device numarası (slot)
    pub device: u8,
    /// PCI function numarası
    pub function: u8,
    /// Sürücü pointer'ı (usize olarak saklanır — raw pointer yerine)
    pub driver: usize,
    /// PciDev pointer'ı (usize olarak saklanır)
    pub dev: usize,
    /// Sürücü adı (loglama için)
    pub name: String,
}

/// Talep edilmiş PCI cihazlarının global listesi.
/// Mutex ile korunur — sürücü yükleme ve kaldırma işlemleri çok iş parçacıklıdır.
static CLAIMED_DEVICES: Mutex<Vec<ClaimedDevice>> = Mutex::new(Vec::new());

/// echOS PCI cihazını Linux sürücüsüne bağlamak için kullanılan özel veri.
///
/// `PciDev.driver_data` alanına işaret eder.
/// Sürücü `pci_get_drvdata()` ile bu yapıya erişip bus/device/function bilgisini alır.
#[repr(C)]
pub struct LinuxPciPriv {
    /// PCI bus numarası
    pub bus: u8,
    /// PCI device (slot) numarası
    pub device: u8,
    /// PCI function numarası
    pub function: u8,
    /// Yapı boyutunu 8 bayta hizalamak için dolgu
    pub _pad: [u8; 5],
}

/// Bir `PciDev` nesnesine bus/device/function bilgisi ekler.
///
/// ## Nasıl Çalışır?
/// Heap'ten `LinuxPciPriv` tahsis eder ve pointer'ı `PciDev.driver_data`'ya yazar.
/// Sürücü daha sonra bu pointer'a `pci_get_drvdata(dev)` ile erişir.
///
/// ## Güvenlik
/// `dev` null ise false döner. Tahsis başarısız olursa da false döner.
pub unsafe fn attach_pci_bdf(dev: *mut PciDev, bus: u8, device: u8, function: u8) -> bool {
    if dev.is_null() {
        return false;
    }
    let size = core::mem::size_of::<LinuxPciPriv>();
    let ptr = crate::allocator::heap_alloc(size) as *mut LinuxPciPriv;
    if ptr.is_null() {
        return false;
    }
    ptr.write(LinuxPciPriv {
        bus,
        device,
        function,
        _pad: [0; 5],
    });
    (*dev).driver_data = ptr as *mut c_void;
    true
}

/// IORESOURCE_MEM bayrağı — PCI kaynağının bellek eşlemeli G/Ç (MMIO) olduğunu gösterir.
/// Linux kaynak bayrakları include/linux/ioport.h içinde tanımlıdır.
const IORESOURCE_MEM: u64 = 0x0000_0200;

/// Bir PCI cihazı için Linux `PciDev` nesnesi oluşturur.
///
/// ## Yapılan İşlemler (akış diyagramı)
/// ```text
/// create_pci_dev(bus, device, function)
///   ├── PCI config space oku (vendor:device)
///   │     └── 0xFFFF_FFFF → null döndür (cihaz yok)
///   ├── class_rev oku → class_code, subclass, prog_if, revision
///   ├── subsystem oku → subsystem_vendor, subsystem_device
///   ├── PciDev struct oluştur (config değerleriyle doldur)
///   ├── Her BAR için:
///   │     ├── read_bar_mmio() → BAR base ve size
///   │     └── PciResource olarak ekle (IORESOURCE_MEM bayrağıyla)
///   ├── Heap'ten PciDev tahsis et ve yaz
///   └── attach_pci_bdf() → driver_data'ya BDF ekle
/// ```
///
/// Dönen pointer heap'te tahsis edilmiştir; `destroy_pci_dev()` ile serbest bırakılmalıdır.
pub(crate) unsafe fn create_pci_dev(bus: u8, device: u8, function: u8) -> *mut PciDev {
    let id = crate::drivers::pci::read_config_dword(bus, device, function, 0x00);
    if id == 0xFFFF_FFFF {
        return core::ptr::null_mut();
    }
    let class_rev = crate::drivers::pci::read_config_dword(bus, device, function, 0x08);
    let subsys = crate::drivers::pci::read_config_dword(bus, device, function, 0x2C);
    let class_code = ((class_rev >> 24) & 0xFF) as u8;
    let subclass = ((class_rev >> 16) & 0xFF) as u8;
    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
    let revision = (class_rev & 0xFF) as u8;
    let mut dev = PciDev {
        dev: Device {
            parent: core::ptr::null_mut(),
            driver: core::ptr::null_mut(),
            driver_data: core::ptr::null_mut(),
        },
        vendor: (id & 0xFFFF) as u16,
        device: ((id >> 16) & 0xFFFF) as u16,
        subsystem_vendor: (subsys & 0xFFFF) as u16,
        subsystem_device: ((subsys >> 16) & 0xFFFF) as u16,
        class: ((class_code as u32) << 16) | ((subclass as u32) << 8) | (prog_if as u32),
        revision,
        _pad0: [0; 3],
        resource: core::mem::zeroed(),
        driver_data: core::ptr::null_mut(),
    };
    for bar_index in 0..6u8 {
        if let Some(bar) = crate::drivers::pci::read_bar_mmio(bus, device, function, bar_index) {
            if bar.size == 0 {
                continue;
            }
            let end = bar.base.saturating_add(bar.size.saturating_sub(1));
            dev.resource[bar_index as usize] = PciResource {
                start: bar.base,
                end,
                flags: IORESOURCE_MEM,
            };
        }
    }
    let ptr = crate::allocator::heap_alloc(core::mem::size_of::<PciDev>()) as *mut PciDev;
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    ptr.write(dev);
    attach_pci_bdf(ptr, bus, device, function);
    ptr
}

/// Heap'teki bir `PciDev` nesnesini ve ilişkili `driver_data`'yı serbest bırakır.
///
/// `create_pci_dev` ile oluşturulan nesneler için kullanılır.
/// Sürücü probe başarısız olursa veya sürücü kaldırılırsa çağrılır.
pub(crate) unsafe fn destroy_pci_dev(dev: *mut PciDev) {
    if dev.is_null() {
        return;
    }
    let data = (*dev).driver_data as *mut u8;
    if !data.is_null() {
        crate::allocator::heap_free(data);
    }
    crate::allocator::heap_free(dev as *mut u8);
}

/// PCI kimlik tablosunun sonuna ulaşılıp ulaşılmadığını kontrol eder.
///
/// Linux sözleşmesine göre, `vendor == 0 && device == 0` olan giriş
/// tablonun sonunu gösterir. Bu, C'deki null-terminated string'e benzer.
pub(crate) fn id_table_end(id: &PciDeviceId) -> bool {
    id.vendor == 0 && id.device == 0
}

/// Bir echOS PCI cihazının, sürücü ID tablosundaki bir girişle eşleşip eşleşmediğini kontrol eder.
///
/// ## Eşleştirme Kuralları
/// - `vendor != 0xFFFF` ise: cihazın vendor ID'si eşleşmeli
/// - `device != 0xFFFF` ise: cihazın device ID'si eşleşmeli
/// - `class_mask != 0` ise: `(dev_class & class_mask) == (id.class & class_mask)` sağlanmalı
///
/// `0xFFFF` joker değeridir — o alana bakılmaz.
pub(crate) fn id_match(dev: &crate::drivers::pci::PciDevice, id: &PciDeviceId) -> bool {
    if id.vendor != 0xFFFF && id.vendor != dev.vendor_id {
        return false;
    }
    if id.device != 0xFFFF && id.device != dev.device_id {
        return false;
    }
    if id.class_mask != 0 {
        let dev_class =
            ((dev.class_code as u32) << 16) | ((dev.subclass as u32) << 8) | (dev.prog_if as u32);
        if (dev_class & id.class_mask) != (id.class & id.class_mask) {
            return false;
        }
    }
    true
}

/// Belirtilen PCI cihazının zaten bir sürücü tarafından talep edilip edilmediğini kontrol eder.
///
/// `bus:device.function` üçlüsü ile eşleşen bir kayıt varsa `true` döner.
/// Bu kontrol, aynı cihazın birden fazla sürücü tarafından talep edilmesini önler.
pub(crate) fn is_claimed(bus: u8, device: u8, function: u8) -> bool {
    let claimed = CLAIMED_DEVICES.lock();
    claimed
        .iter()
        .any(|entry| entry.bus == bus && entry.device == device && entry.function == function)
}

/// Bir PCI cihazını belirtilen sürücü adına kayıt eder.
///
/// `probe()` başarılı olduğunda çağrılır. Cihaz, `CLAIMED_DEVICES` listesine eklenir
/// ve bir daha `probe` denilmez.
pub(crate) fn claim_device(driver: *mut PciDriver, dev: *mut PciDev, bus: u8, device: u8, function: u8) {
    let mut claimed = CLAIMED_DEVICES.lock();
    let name = driver_name(driver);
    claimed.push(ClaimedDevice {
        bus,
        device,
        function,
        driver: driver as usize,
        dev: dev as usize,
        name,
    });
}

/// Linux PCI sürücü kayıt fonksiyonu — C ABI ile dışa aktarılır.
///
/// Linux sürücüsü `pci_register_driver(&drv)` çağırdığında bu fonksiyon tetiklenir.
///
/// ## Akış
/// ```text
/// pci_register_driver(driver)
///   ├── driver veya id_table null → -1
///   └── Her PCI cihazı için:
///         ├── Zaten talep edilmiş → atla
///         └── ID eşleşiyor mu?
///               ├── Evet → create_pci_dev() + probe()
///               │           ├── rc == 0 → claim_device(), claimed++
///               │           └── rc != 0 → destroy_pci_dev()
///               └── Hayır → sonraki ID
/// claimed > 0 → 0 döner, aksi hâlde -1
/// ```
///
/// `#[no_mangle]`: Rust isim düzenlemesi (name mangling) devre dışı bırakılır;
/// C kütüphane linkleme için gereklidir.
#[no_mangle]
pub unsafe extern "C" fn pci_register_driver(driver: *mut PciDriver) -> i32 {
    if driver.is_null() {
        return -1;
    }
    let id_table = (*driver).id_table;
    if id_table.is_null() {
        return -1;
    }
    let devices = crate::drivers::pci::scan();
    let mut claimed = 0;
    for dev in devices {
        if is_claimed(dev.bus, dev.device, dev.function) {
            continue;
        }
        let mut id_ptr = id_table;
        loop {
            let id_ref = &*id_ptr;
            if id_table_end(id_ref) {
                break;
            }
            if id_match(&dev, id_ref) {
                let linux_dev = create_pci_dev(dev.bus, dev.device, dev.function);
                if linux_dev.is_null() {
                    break;
                }
                (*linux_dev).dev.driver = driver as *mut c_void;
                if let Some(probe) = (*driver).probe {
                    let rc = probe(linux_dev, id_ref as *const PciDeviceId);
                    if rc == 0 {
                        claim_device(driver, linux_dev, dev.bus, dev.device, dev.function);
                        crate::shim_layer::printk(
                            b"pci_register_driver: bound device\n\0".as_ptr() as *const c_char,
                        );
                        claimed += 1;
                        break;
                    }
                }
                destroy_pci_dev(linux_dev);
            }
            id_ptr = id_ptr.add(1);
        }
    }
    if claimed > 0 {
        0
    } else {
        -1
    }
}

/// Linux PCI sürücü kayıt silme fonksiyonu — C ABI ile dışa aktarılır.
///
/// Sürücü kaldırıldığında `remove()` fonksiyonunu her talep edilmiş cihaz için
/// çağırır ve cihaz nesnelerini heap'ten serbest bırakır.
///
/// `swap_remove`: VecDeque değil Vec kullandığı için sıra korunmaz,
/// ancak O(1) ile silme yapılır (son elemanla yer değiştirilir).
#[no_mangle]
pub unsafe extern "C" fn pci_unregister_driver(driver: *mut PciDriver) {
    if driver.is_null() {
        return;
    }
    let mut claimed = CLAIMED_DEVICES.lock();
    let mut i = 0usize;
    while i < claimed.len() {
        if claimed[i].driver == driver as usize {
            let dev = claimed[i].dev as *mut PciDev;
            if let Some(remove) = (*driver).remove {
                remove(dev);
            }
            destroy_pci_dev(dev);
            claimed.swap_remove(i);
            continue;
        }
        i += 1;
    }
}

/// Bir `PciDriver` pointer'ından sürücü adını çıkarır.
///
/// `driver.name` alanı C string (null terminated) pointer'ıdır.
/// `CStr::from_ptr` ile Rust string'e dönüştürülür.
/// Geçersiz pointer veya null ise `"unknown"` döner.
pub(crate) fn driver_name(driver: *mut PciDriver) -> String {
    if driver.is_null() {
        return String::from("unknown");
    }
    let name_ptr = unsafe { (*driver).name };
    if name_ptr.is_null() {
        return String::from("unknown");
    }
    unsafe { String::from(CStr::from_ptr(name_ptr).to_str().unwrap_or("unknown")) }
}

/// Aktif sürücü adlarının listesini döner (tekrar içermez).
///
/// Hata ayıklama ve sürücü envanter yönetimi için kullanılır.
pub fn list_driver_names() -> Vec<String> {
    let claimed = CLAIMED_DEVICES.lock();
    let mut names: Vec<String> = Vec::new();
    for entry in claimed.iter() {
        if !names.iter().any(|n| n == &entry.name) {
            names.push(entry.name.clone());
        }
    }
    names
}

/// Tüm aktif sürücüleri ve her sürücünün talep ettiği cihaz sayısını seri porta yazdırır.
///
/// Kernel başlatma günlüklerinde veya hata ayıklama sırasında sürücü durumu
/// hızlıca gözlemlenmek istendiğinde çağrılır.
pub fn debug_dump_drivers() {
    let claimed = CLAIMED_DEVICES.lock();
    let mut names: Vec<String> = Vec::new();
    for entry in claimed.iter() {
        if !names.iter().any(|n| n == &entry.name) {
            names.push(entry.name.clone());
        }
    }
    for name in names.iter() {
        let mut count = 0usize;
        for entry in claimed.iter() {
            if &entry.name == name {
                count += 1;
            }
        }
        crate::serial_println!("driver={} devices={}", name, count);
    }
}

// ============================================================================
// VirtIO GPU SÜRÜCÜSÜ — PCI üzerinden GPU başlatma
// ============================================================================

/// VirtIO GPU PCI probe fonksiyonu — cihaz bulunduğunda çağrılır.
///
/// VirtIO GPU, QEMU/KVM sanallaştırma ortamlarında kullanılan sanal GPU'dur.
/// Vendor ID: 0x1AF4 (Red Hat / VirtIO), Device ID: 0x1050
/// Başarılı olursa 0, başarısız olursa -1 döner.
unsafe extern "C" fn virtio_gpu_probe(_dev: *mut PciDev, _id: *const PciDeviceId) -> i32 {
    if crate::drivers::virtio_gpu::init_from_pci(_dev) {
        0
    } else {
        -1
    }
}

/// VirtIO GPU sürücüsünün desteklediği PCI cihaz kimlik tablosu.
///
/// İlk giriş: VirtIO GPU (Vendor=0x1AF4, Device=0x1050).
/// İkinci giriş: Tablo sonu işaretçisi (vendor=0, device=0).
static VIRTIO_GPU_ID_TABLE: [PciDeviceId; 2] = [
    PciDeviceId {
        vendor: 0x1AF4,    // Red Hat VirtIO üretici kimliği
        device: 0x1050,    // VirtIO GPU ürün kimliği
        subvendor: 0xFFFF, // Alt sistem üretici — joker (herhangi biri)
        subdevice: 0xFFFF, // Alt sistem ürün — joker (herhangi biri)
        class: 0,
        class_mask: 0,
        driver_data: 0,
    },
    PciDeviceId {
        vendor: 0,  // Tablo sonu işaretçisi — vendor=0, device=0
        device: 0,
        subvendor: 0,
        subdevice: 0,
        class: 0,
        class_mask: 0,
        driver_data: 0,
    },
];

/// VirtIO GPU PCI sürücü yapısı — statik olarak tanımlanmıştır.
///
/// `static mut` gereklidir: `pci_register_driver` C FFI ile `*mut PciDriver` pointer'ı alır.
/// Güvenlik: bu pointer yalnızca init() sırasında tek kez kullanılır.
static mut VIRTIO_GPU_DRIVER: PciDriver = PciDriver {
    name: b"virtio_gpu\0".as_ptr() as *const c_char,
    id_table: VIRTIO_GPU_ID_TABLE.as_ptr(),
    probe: Some(virtio_gpu_probe),
    remove: None,
};

/// Linux glue katmanını başlatır; VirtIO GPU sürücüsünü PCI sistemine kaydeder.
///
/// Kernel önyükleme sırasında çağrılır. VirtIO GPU cihazı PCI taramasında
/// bulunursa `virtio_gpu_probe()` otomatik olarak tetiklenir.
pub fn init() {
    unsafe {
        let _ = pci_register_driver(&raw mut VIRTIO_GPU_DRIVER as *mut PciDriver);
    }
}

// ============================================================================
// SOYUT DOSYA SİSTEMİ VE DRM YAPILARI
// ============================================================================

/// Soyut inode (dosya sistemi nodu) — DRM ve dosya işlemleri için kullanılır.
///
/// `_opaque: [u8; 0]` sıfır boyutlu yer tutucu (ZST olmayan opaque type).
/// Bu sayede Rust, bu tür pointer'ları `c_void` yerine tip güvenli kullanabilir.
#[repr(C)]
pub struct Inode {
    pub _opaque: [u8; 0],
}

/// Soyut dosya nesnesi — `open()`/`read()`/`write()`/`ioctl()` için kullanılır.
#[repr(C)]
pub struct File {
    pub _opaque: [u8; 0],
}

/// DRM (Direct Rendering Manager) dosya nesnesi — GPU kaynak yönetimi için.
///
/// DRM, kullanıcı alanının GPU ile doğrudan iletişim kurmasına izin veren
/// Linux kernel alt sistemidir. Her GPU bağlantısı bir `DrmFile` ile temsil edilir.
#[repr(C)]
pub struct DrmFile {
    pub _opaque: [u8; 0],
}

/// DRM sürücüsünün yükleme (load) fonksiyon türü.
/// GPU başlatıldığında çağrılır; donanım başlatma ve kaynak tahsisi burada yapılır.
pub type DrmLoadFn = Option<unsafe extern "C" fn(dev: *mut Device, flags: u32) -> i32>;

/// DRM sürücüsünün kaldırma (unload) fonksiyon türü.
pub type DrmUnloadFn = Option<unsafe extern "C" fn(dev: *mut Device)>;

/// DRM IOCTL işleyici fonksiyon türü.
/// Kullanıcı alanından gelen GPU komutları (buffer oluşturma, fence, vs.) bu yolla işlenir.
pub type DrmIoctlFn =
    Option<unsafe extern "C" fn(dev: *mut Device, data: *mut c_void, file: *mut DrmFile) -> i32>;

/// Linux `struct file_operations` — bir karakter aygıtının dosya işlem tablosu.
///
/// Her `open`, `read`, `write`, `ioctl` gibi sistem çağrısı için
/// bir fonksiyon pointer'ı içerir. NULL pointer = "işlem desteklenmiyor".
#[repr(C)]
pub struct FileOperations {
    pub owner: *mut c_void, // offset = 0x00 — modül sahipliği (MODULE_THIS)
    pub open: Option<unsafe extern "C" fn(inode: *mut Inode, file: *mut File) -> i32>, // offset = 0x08
    pub release: Option<unsafe extern "C" fn(inode: *mut Inode, file: *mut File) -> i32>, // offset = 0x10
    pub read: Option<
        unsafe extern "C" fn(file: *mut File, buf: *mut u8, len: usize, offset: *mut i64) -> isize,
    >, // offset = 0x18
    pub write: Option<
        unsafe extern "C" fn(
            file: *mut File,
            buf: *const u8,
            len: usize,
            offset: *mut i64,
        ) -> isize,
    >, // offset = 0x20
    pub unlocked_ioctl: Option<unsafe extern "C" fn(file: *mut File, cmd: u32, arg: usize) -> i32>, // offset = 0x28
}

/// Linux DRM sürücü tanımlayıcı yapısı.
///
/// DRM sürücüleri bu yapıyı doldurarak `drm_dev_register()` ile kaydedilebilir.
/// `fops` alanı, kullanıcı alanı dosya sistemi işlemlerini tanımlar.
#[repr(C)]
pub struct DrmDriver {
    /// DRM major versiyon numarası
    pub major: i32,
    /// DRM minor versiyon numarası
    pub minor: i32,
    /// GPU başlatma fonksiyonu
    pub load: DrmLoadFn,     // offset = 0x08
    /// GPU kaldırma fonksiyonu
    pub unload: DrmUnloadFn, // offset = 0x10
    /// IOCTL işleyici
    pub ioctl: DrmIoctlFn,   // offset = 0x18
    /// Dosya operasyonları tablosu pointer'ı
    pub fops: *const FileOperations,
    /// Sürücü adı
    pub name: *const c_char,
}

// ============================================================================
// BELLEK DÜZENİ DOĞRULAMA MAKROLARI
// ============================================================================

/// Bir alanın yapı içindeki bellek offsetini döner.
///
/// ## Kullanım
/// ```rust
/// let off = offset_of!(PciDev, vendor);  // İdeal: 0x18
/// ```
///
/// ## Nasıl Çalışır?
/// Null pointer'dan (`0x0`) sanal bir yapı örneği türetilir ve
/// alanın adresi hesaplanır. Bu "pointer aritmetiği triki" C'deki
/// `offsetof()` makrosuna eşdeğerdir ve compile-time güvenlidir.
#[macro_export]
macro_rules! offset_of {
    ($ty:ty, $field:ident) => {{
        let base = core::ptr::null::<$ty>();
        unsafe { &(*base).$field as *const _ as usize }
    }};
}

/// Bir yapının boyutunu ve alan offsetlerini derleme zamanında doğrular.
///
/// Linux ABI uyumluluğu kritiktir — yanlış offset sürücüyü anında çökertir.
/// Bu makro, tanımlanan yapı düzeninin beklenenle uyuştuğunu garanti eder.
///
/// ## Kullanım Örneği
/// ```rust
/// verify_layout!(PciDev, 0xC0,
///     vendor => 0x18,
///     device => 0x1A,
///     resource => 0x28,
/// );
/// ```
#[macro_export]
macro_rules! verify_layout {
    ($ty:ty, $size:expr, $( $field:ident => $offset:expr ),+ $(,)?) => {{
        assert_eq!(core::mem::size_of::<$ty>(), $size);
        $(assert_eq!($crate::offset_of!($ty, $field), $offset);)+
    }};
}
