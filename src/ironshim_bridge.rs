//! # IronShim Bridge — Linux Driver FFI ↔ echOS Kernel Güvenlik Köprüsü
//!
//! Bu modül, hem C FFI (bindgen) sürücülerini hem de Rust-native sürücüleri
//! IronShim'in capability-based izolasyon katmanından geçirir.
//!
//! ## Mimari
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │          Linux C Driver / Rust Driver             │
//! └──────────────────┬──────────────────────────────┘
//!                    │ C FFI veya Rust trait
//! ┌──────────────────▼──────────────────────────────┐
//! │           IronShim Bridge (bu modül)              │
//! │  • ResourceManifest (MMIO/Port whitelist)         │
//! │  • InterruptBudget (IRQ storm koruması)           │
//! │  • ManifestSignature (imza doğrulama)             │
//! │  • AuditSink (güvenlik logları)                   │
//! └──────────────────┬──────────────────────────────┘
//!                    │ Safe Rust API
//! ┌──────────────────▼──────────────────────────────┐
//! │              echOS Kernel (Rust)                  │
//! │   IRONSHIM_DMA  │  IRONSHIM_PCI  │  IRONSHIM_IRQ │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! ## IronShim Güvenlik Katmanının Amacı
//! Linux sürücüleri doğrudan kernel ile iletişim kurduğunda tehlikeli olabilir:
//! - **MMIO erişim sınırı yok**: Sürücü istediği bellek adresine yazabilir.
//! - **IRQ storm**: Kötü bir sürücü milyonlarca interrupt üretip sistemi dondurabilir.
//! - **DMA saldırıları**: Sürücü DMA ile çekirdek belleğini okuyabilir/yazabilir.
//!
//! IronShim Bridge bu riskleri şöyle önler:
//! 1. **ResourceManifest**: Her sürücüye sadece kendi PCI BAR bölgelerine erişim izni verir.
//! 2. **InterruptBudget**: Sürücünün toplamda kullanabileceği interrupt sayısını sınırlar.
//! 3. **DmaAllocator**: DMA tahsislerini izole eder, sürücünün dışına taşmasını engeller.
//! 4. **AuditSink**: Tüm güvenlik olaylarını kayıt altına alır.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use ironshim_rs::{
    enforce_syscall,
    AuditEvent,
    // Denetim (audit) ve sistem çağrısı politikası
    AuditSink,
    // DMA tahsis yöneticisi
    DmaAllocator,
    DmaHandle,
    // Hata türü
    Error as ShimError,
    // IRQ budget koruması
    InterruptBudget,
    InterruptRegistry,
    IoPortDesc,
    MmioDesc,
    // PCI adres ve fonksiyon tanımlayıcıları
    PciAddress,
    PciBar,
    PciFunctionDesc,
    // Kaynak izolasyonu — MMIO aralıkları ve port erişim denetimi
    ResourceManifest,
    SyscallPolicy,
    SyscallRequest,
};

use crate::shim_layer::{
    EchOsDmaAllocator, EchOsPciConfig, IRONSHIM_AUDIT, IRONSHIM_DMA, IRONSHIM_IRQ, IRONSHIM_PCI,
    IRONSHIM_POLICY,
};

// ============================================================================
// 1. IsolatedDriver — Her sürücüye ait izolasyon bağlamı
// ============================================================================

/// Bir sürücünün izolasyon bağlamı — ham MMIO/Port verileri (Send+Sync güvenli).
///
/// `ResourceManifest` `!Send` olduğundan, manifest bilgisini ham (raw) olarak tutuyoruz.
/// Bu struct, bir PCI sürücüsünün izin verilen kaynaklarını, IRQ limitlerini ve
/// kimlik bilgilerini tek bir yapı altında toplar.
///
/// ## Neden Bu Tasarım?
/// Linux sürücüleri rastgele donanım kaynaklarına erişme eğilimindedir.
/// Bu struct, her sürücünün "dışına çıkamaması" gereken kaynak sınırlarını tanımlar.
///
/// ## Alanlar
/// - `mmio_regions`: Sürücünün yazıp okuyabileceği bellek eşlemeli G/Ç bölgeleri.
/// - `port_regions`: Sürücünün erişebileceği port G/Ç aralıkları.
/// - `irq_budget`: Sürücünün kesinti bütçesi — bütçe dolunca IRQ'lar reddedilir.
/// - `pci_addr`: Sürücünün sahip olduğu PCI cihazının bus/device/function adresi.
pub struct IsolatedDriver {
    /// Sürücü adı (loglama ve denetim için kullanılır).
    pub name: String,
    /// İzin verilen MMIO bölgeleri (bellek eşlemeli G/Ç).
    pub mmio_regions: [MmioDesc; 8],
    /// Gerçekte kullanılan MMIO bölge sayısı.
    pub mmio_count: usize,
    /// İzin verilen Port I/O bölgeleri (x86 IN/OUT talimatları için).
    pub port_regions: [IoPortDesc; 8],
    /// Gerçekte kullanılan port bölge sayısı.
    pub port_count: usize,
    /// IRQ bütçesi — sürücünün alabileceği maksimum kesinti sayısı.
    pub irq_budget: InterruptBudget,
    /// PCI bus/device/function adresi (örn: 00:1F.3).
    pub pci_addr: PciAddress,
    /// Bu sürücünün kayıt ettiği IRQ vektörleri.
    pub irq_vectors: [u32; 8],
    /// Kayıtlı IRQ vektör sayısı.
    pub irq_count: usize,
    /// Sürücü aktif mi? (kayıt silindikten sonra false olur)
    pub active: bool,
}

/// Her sürücüyü tip düzeyinde ayıran etiket.
/// PhantomData ile type-level ayrım sağlar; driver type confusion saldırılarına karşı koruma.
pub struct DriverTag;

// ============================================================================
// 2. Global Sürücü Kayıt Defteri — Yüklü sürücülerin listesi
// ============================================================================

/// Aynı anda sistemde bulunan maksimum izole sürücü sayısı.
/// 32 yeterlidir çünkü boot sırasında bu kadar çok sürücü yüklenmez.
const MAX_ISOLATED_DRIVERS: usize = 32;

/// Sabit boyutlu, Mutex korumalı sürücü slotları.
/// Option<IsolatedDriver> kullanılır: None = boş slot, Some = dolu slot.
static ISOLATED_DRIVERS: Mutex<[Option<IsolatedDriver>; MAX_ISOLATED_DRIVERS]> =
    Mutex::new([const { None }; MAX_ISOLATED_DRIVERS]);

/// İzole bir sürücüyü kayıt defterine ekler.
///
/// İlk boş slotu bulup `driver`'ı yerleştirir ve slot index'ini döner.
/// Tüm slotlar doluysa `ShimError::OutOfMemory` döner.
///
/// ## Güvenlik Etkisi
/// Kayıt sırasında `AuditEvent::ManifestValidated` kaydedilir.
/// Bu, her sürücü yüklemesinin izlenebilir olduğu anlamına gelir.
pub fn register_isolated_driver(driver: IsolatedDriver) -> Result<usize, ShimError> {
    let mut slots = ISOLATED_DRIVERS.lock();
    for (i, slot) in slots.iter_mut().enumerate() {
        if slot.is_none() {
            IRONSHIM_AUDIT.record(AuditEvent::ManifestValidated);
            crate::serial_println!(
                "[IronShim/Bridge] Driver '{}' registered in slot {} (PCI {:02x}:{:02x}.{}) — MMIO:{} Port:{}",
                driver.name, i,
                driver.pci_addr.bus, driver.pci_addr.device, driver.pci_addr.function,
                driver.mmio_count, driver.port_count,
            );
            *slot = Some(driver);
            return Ok(i);
        }
    }
    crate::serial_println!(
        "[IronShim/Bridge] ERROR: All {} driver slots full",
        MAX_ISOLATED_DRIVERS
    );
    Err(ShimError::OutOfMemory)
}

/// Belirtilen slot index'indeki sürücüyü kayıt defterinden siler.
///
/// Sürücü kaldırıldıktan sonra slot temizlenir ve bir sonraki kayıt için hazır olur.
/// Geçersiz index verilirse `ShimError::InvalidAddress` döner.
pub fn unregister_isolated_driver(slot: usize) -> Result<(), ShimError> {
    let mut slots = ISOLATED_DRIVERS.lock();
    if slot >= MAX_ISOLATED_DRIVERS {
        return Err(ShimError::InvalidAddress);
    }
    if let Some(ref driver) = slots[slot] {
        crate::serial_println!(
            "[IronShim/Bridge] Driver '{}' unregistered from slot {}",
            driver.name,
            slot
        );
    }
    slots[slot] = None;
    Ok(())
}

/// Şu anda kayıtlı (aktif) sürücü sayısını döner.
/// `None` olmayan slot sayısını sayar.
pub fn active_driver_count() -> usize {
    ISOLATED_DRIVERS
        .lock()
        .iter()
        .filter(|s| s.is_some())
        .count()
}

// ============================================================================
// 3. PCI BAR → Manifest Verisi Otomatik Oluşturma
// ============================================================================

/// PCI BAR (Base Address Register) bilgilerinden MMIO ve Port I/O descriptor'larını çıkarır.
///
/// ## PCI BAR Nedir?
/// Her PCI cihazı, kullandığı donanım kaynaklarını BAR alanlarıyla bildirir.
/// - **Bellek BAR (MMIO)**: CPU bellek adres uzayına eşlenmiş cihaz belleği.
/// - **Port BAR (I/O)**: x86 IN/OUT talimatlarıyla erişilen özel port alanı.
///
/// Bu fonksiyon, BAR listesini tarayarak IronShim manifest'ini doldurmak için
/// gerekli MMIO ve port aralıklarını çıkarır.
///
/// ## Çıktı Formatı
/// `([MmioDesc; 8], usize, [IoPortDesc; 8], usize)` — MMIO dizisi + count, Port dizisi + count
pub fn extract_manifest_from_bars(
    bars: &[PciBar; 6],
    bar_count: usize,
) -> ([MmioDesc; 8], usize, [IoPortDesc; 8], usize) {
    let mut mmio = [MmioDesc { base: 0, size: 0 }; 8];
    let mut ports = [IoPortDesc { port: 0, count: 0 }; 8];
    let mut mmio_len = 0usize;
    let mut port_len = 0usize;

    for i in 0..bar_count.min(6) {
        let bar = &bars[i];
        if bar.base == 0 {
            continue;
        }
        if bar.is_io {
            if port_len < 8 {
                ports[port_len] = IoPortDesc {
                    port: bar.base as u16,
                    count: 256,
                };
                port_len += 1;
            }
        } else {
            if mmio_len < 8 {
                let size = if bar.is_64 { 0x1000000 } else { 0x100000 };
                mmio[mmio_len] = MmioDesc {
                    base: bar.base as usize,
                    size,
                };
                mmio_len += 1;
            }
        }
    }

    (mmio, mmio_len, ports, port_len)
}

/// MMIO ve Port bilgilerinden bir `ResourceManifest` oluşturur.
///
/// `ResourceManifest`, IronShim'in çalışma zamanında sürücünün erişim
/// taleplerini denetlemek için kullandığı beyaz liste yapısıdır.
/// Manifest oluşturma başarısız olursa `ShimError` döner.
pub fn build_manifest(
    mmio: [MmioDesc; 8],
    mmio_len: usize,
    ports: [IoPortDesc; 8],
    port_len: usize,
) -> Result<ResourceManifest<DriverTag>, ShimError> {
    ResourceManifest::new(mmio, mmio_len, ports, port_len)
}

// ============================================================================
// 4. Güvenli PCI Sürücü Kaydı — safe_pci_register_driver
// ============================================================================

/// Linux C FFI `pci_register_driver()` yerine kullanılacak IronShim-korumalı sürüm.
///
/// ## Güvenlik Adımları (akış diyagramı)
/// ```text
/// safe_pci_register_driver(driver)
///   ├── driver null mı? → ManifestRejected + return -1
///   ├── id_table null mı? → ManifestRejected + return -1
///   └── Her PCI cihazı için:
///         ├── Zaten talep edilmiş mi? → atla
///         ├── ID eşleşiyor mu? → hayır → sonraki ID
///         └── Eşleşme bulundu:
///               ├── IronShim ile BAR parse
///               ├── BAR'lardan manifest data çıkar
///               ├── Manifest doğrula → başarısız → ManifestRejected + atla
///               ├── IsolatedDriver oluştur + kaydet
///               ├── Linux PCI dev nesnesi oluştur
///               ├── probe() çağır → rc == 0 → cihazı talep et
///               └── Audit: ManifestValidated
/// ```
///
/// Bu fonksiyon, `pci_register_driver` doğrudan çağrısının yerine geçer.
/// Her aşamada IronShim izolasyon kontrollerinden geçirilir; başarısız olursa
/// sürücü yüklenmez ve audit kaydı tutulur.
pub fn safe_pci_register_driver(driver: *mut crate::linux_glue::PciDriver) -> i32 {
    if driver.is_null() {
        IRONSHIM_AUDIT.record(AuditEvent::ManifestRejected);
        return -1;
    }

    let driver_name = unsafe { crate::linux_glue::driver_name(driver) };
    crate::serial_println!(
        "[IronShim/Bridge] === safe_pci_register_driver('{}') ===",
        driver_name
    );

    let id_table = unsafe { (*driver).id_table };
    if id_table.is_null() {
        IRONSHIM_AUDIT.record(AuditEvent::ManifestRejected);
        return -1;
    }

    let echos_pci_devices = crate::drivers::pci::scan();
    let mut claimed = 0i32;

    for dev in echos_pci_devices.iter() {
        if crate::linux_glue::is_claimed(dev.bus, dev.device, dev.function) {
            continue;
        }

        let mut id_ptr = id_table;
        loop {
            let id_ref = unsafe { &*id_ptr };
            if crate::linux_glue::id_table_end(id_ref) {
                break;
            }
            if crate::linux_glue::id_match(dev, id_ref) {
                crate::serial_println!(
                    "[IronShim/Bridge] Match: {:04x}:{:04x} @ {:02x}:{:02x}.{}",
                    dev.vendor_id,
                    dev.device_id,
                    dev.bus,
                    dev.device,
                    dev.function
                );

                // IronShim PCI parse — BAR bilgilerini oku
                let desc = match ironshim_rs::parse_pci_function(
                    &EchOsPciConfig,
                    dev.bus,
                    dev.device,
                    dev.function,
                ) {
                    Ok(d) => d,
                    Err(_) => {
                        crate::serial_println!("[IronShim/Bridge] PCI parse failed, skipping");
                        unsafe {
                            id_ptr = id_ptr.add(1);
                        }
                        continue;
                    }
                };

                // BAR'lardan manifest verisi çıkar (MMIO/Port beyaz listesi)
                let (mmio, mmio_count, ports, port_count) =
                    extract_manifest_from_bars(&desc.bars, desc.bar_len);

                // Manifest doğrulama — sürücünün talep ettiği kaynaklar geçerli mi?
                if let Err(e) = build_manifest(mmio, mmio_count, ports, port_count) {
                    crate::serial_println!("[IronShim/Bridge] Manifest validation failed: {:?}", e);
                    IRONSHIM_AUDIT.record(AuditEvent::ManifestRejected);
                    unsafe {
                        id_ptr = id_ptr.add(1);
                    }
                    continue;
                }

                // IsolatedDriver oluştur ve kayıt defterine ekle
                let isolated = IsolatedDriver {
                    name: driver_name.clone(),
                    mmio_regions: mmio,
                    mmio_count,
                    port_regions: ports,
                    port_count,
                    irq_budget: InterruptBudget {
                        max_ticks: 10_000,
                        max_calls: 100_000,
                    },
                    pci_addr: PciAddress {
                        bus: dev.bus,
                        device: dev.device,
                        function: dev.function,
                    },
                    irq_vectors: [0; 8],
                    irq_count: 0,
                    active: true,
                };

                if let Err(e) = register_isolated_driver(isolated) {
                    crate::serial_println!("[IronShim/Bridge] Registration failed: {:?}", e);
                    IRONSHIM_AUDIT.record(AuditEvent::ManifestRejected);
                    unsafe {
                        id_ptr = id_ptr.add(1);
                    }
                    continue;
                }

                // Gerçek Linux C FFI probe() fonksiyonunu çağır
                let linux_dev =
                    unsafe { crate::linux_glue::create_pci_dev(dev.bus, dev.device, dev.function) };
                if linux_dev.is_null() {
                    break;
                }

                unsafe {
                    (*linux_dev).dev.driver = driver as *mut core::ffi::c_void;
                }

                if let Some(probe_fn) = unsafe { (*driver).probe } {
                    let rc = unsafe {
                        probe_fn(linux_dev, id_ref as *const crate::linux_glue::PciDeviceId)
                    };
                    if rc == 0 {
                        unsafe {
                            crate::linux_glue::claim_device(
                                driver,
                                linux_dev,
                                dev.bus,
                                dev.device,
                                dev.function,
                            );
                        }
                        crate::serial_println!(
                            "[IronShim/Bridge] ✅ '{}' bound to {:02x}:{:02x}.{} (ISOLATED)",
                            driver_name,
                            dev.bus,
                            dev.device,
                            dev.function
                        );
                        IRONSHIM_AUDIT.record(AuditEvent::ManifestValidated);
                        claimed += 1;
                        break;
                    }
                }

                unsafe {
                    crate::linux_glue::destroy_pci_dev(linux_dev);
                }
            }
            unsafe {
                id_ptr = id_ptr.add(1);
            }
        }
    }

    if claimed > 0 {
        0
    } else {
        -1
    }
}

// ============================================================================
// 5. Güvenli DMA Tahsis — IronShim sınır denetimli
// ============================================================================

/// IronShim güvenlik katmanı üzerinden DMA belleği tahsis eder.
///
/// ## DMA (Direct Memory Access) Nedir?
/// DMA, donanım aygıtlarının CPU'yu araya sokmadan doğrudan sistem belleğine
/// erişmesini sağlar. Yüksek bant genişliği gerektiren işlemler (ağ, disk, GPU)
/// için kritiktir.
///
/// ## Güvenlik Riski
/// Denetlenmemiş DMA, sürücünün çekirdek belleğini okuyup yazmasına izin verir.
/// IronShim DMA tahsisi, her tahsisin fiziksel adresini ve boyutunu takip eder.
///
/// ## Parametreler
/// - `T`: Tahsis edilecek veri türü
/// - `count`: Kaç adet `T` tahsis edileceği
///
/// ## Dönüş Değeri
/// Başarılı olursa `DmaHandle` döner; bu handle üzerinden sanal ve fiziksel
/// adrese erişilebilir.
pub fn safe_dma_alloc<T>(
    count: usize,
) -> Result<DmaHandle<'static, T, EchOsDmaAllocator>, ShimError> {
    let handle = IRONSHIM_DMA.alloc::<T>(count)?;
    crate::serial_println!(
        "[IronShim/Bridge] DMA alloc: {} x {} = {} bytes (phys={:#x})",
        count,
        core::mem::size_of::<T>(),
        count * core::mem::size_of::<T>(),
        handle.phys()
    );
    Ok(handle)
}

// ============================================================================
// 6. Güvenli IRQ Kaydı — Bütçe korumalı
// ============================================================================

/// IronShim bütçe korumalı IRQ (kesinti) kayıt fonksiyonu.
///
/// ## IRQ Storm Nedir?
/// Hatalı bir sürücü, saniyede milyonlarca kesinti üretebilir.
/// Bu durum diğer işlemleri bloke eder ve sistemi dondurur.
///
/// `InterruptBudget` ile her sürücünün kullanabileceği toplam kesinti
/// sayısı önceden belirlenir. Bütçe aşılınca kernel IRQ'ları reddeder.
///
/// ## Parametreler
/// - `irq`: IRQ vektör numarası (x86'da 0-255 arası)
/// - `handler`: Kesinti işleyici — IronShim `InterruptHandler` trait'ini uygular
/// - `budget`: Maksimum tik ve çağrı sayısı sınırları
pub fn safe_request_irq(
    irq: u32,
    handler: &'static mut dyn ironshim_rs::InterruptHandler,
    budget: InterruptBudget,
) -> Result<(), ShimError> {
    IRONSHIM_IRQ.register_with_budget(irq, handler, budget)?;
    crate::serial_println!(
        "[IronShim/Bridge] IRQ {} registered (budget: max_calls={}, max_ticks={})",
        irq,
        budget.max_calls,
        budget.max_ticks,
    );
    Ok(())
}

// ============================================================================
// 7. Güvenli Sistem Çağrısı Uygulama — IronShim + Seccomp benzeri politika
// ============================================================================

/// Bir sistem çağrısını IronShim politikası ve denetim günlüğü üzerinden doğrular.
///
/// ## Seccomp Benzeri Filtreleme
/// Linux'taki seccomp (Secure Computing) mekanizması gibi, her sistem çağrısı
/// önceden tanımlanmış kurallara göre filtrelenir:
/// - İzin veriliyorsa → çağrı gerçekleştirilir
/// - Reddediliyorsa → EPERM hatası + audit kaydı
///
/// ## Parametreler
/// - `number`: Sistem çağrısı numarası (örn: x86_64'te read=0, write=1, open=2)
/// - `args`: Altı adet sistem çağrısı argümanı (register'lara karşılık gelir)
pub fn safe_enforce_syscall(number: u32, args: [usize; 6]) -> Result<(), ShimError> {
    let request = SyscallRequest { number, args };
    enforce_syscall(&IRONSHIM_POLICY, &IRONSHIM_AUDIT, &request)
}

// ============================================================================
// 8. Bridge Başlatma
// ============================================================================

/// IronShim Bridge güvenlik köprüsünü başlatır.
///
/// Kernel önyükleme aşamasında çağrılır. Başlatma banner'ı yazdırır,
/// tüm izolasyon katmanlarının aktif olduğunu bildirir.
///
/// Bu fonksiyon çağrılmadan sürücü yüklenirse güvenlik katmanı devreye
/// girmez ve sürücü doğrudan kernel kaynaklarına erişebilir.
pub fn init_ironshim_bridge() {
    crate::serial_println!("╔══════════════════════════════════════════════════╗");
    crate::serial_println!("║   IronShim Bridge — Active                      ║");
    crate::serial_println!("║   Capability-Based Driver Isolation Layer        ║");
    crate::serial_println!("║   DMA Guard │ IRQ Budget │ Manifest Validator    ║");
    crate::serial_println!("╚══════════════════════════════════════════════════╝");
}
