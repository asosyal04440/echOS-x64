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

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use ironshim_rs::{
    // Resource isolation
    ResourceManifest, MmioDesc, IoPortDesc,
    // PCI
    PciAddress, PciFunctionDesc, PciBar,
    // Interrupt
    InterruptBudget, InterruptRegistry,
    // DMA
    DmaAllocator, DmaHandle,
    // Audit & Syscall
    AuditSink, AuditEvent,
    SyscallPolicy, SyscallRequest,
    enforce_syscall,
    // Error
    Error as ShimError,
};

use crate::shim_layer::{
    IRONSHIM_DMA, IRONSHIM_IRQ, IRONSHIM_PCI,
    IRONSHIM_AUDIT, IRONSHIM_POLICY,
    EchOsDmaAllocator, EchOsPciConfig,
};

// ============================================================================
// 1. IsolatedDriver — Her sürücüye ait izolasyon state'i
// ============================================================================

/// Bir sürücünün izolasyon bağlamı — ham MMIO/Port verileri (Send+Sync güvenli).
/// `ResourceManifest` `!Send` olduğundan, manifest bilgisini raw olarak tutuyoruz.
pub struct IsolatedDriver {
    /// Sürücü adı (loglama için).
    pub name: String,
    /// İzin verilen MMIO bölgeleri.
    pub mmio_regions: [MmioDesc; 8],
    pub mmio_count: usize,
    /// İzin verilen Port I/O bölgeleri.
    pub port_regions: [IoPortDesc; 8],
    pub port_count: usize,
    /// IRQ budget (max çağrı sayısı).
    pub irq_budget: InterruptBudget,
    /// PCI bus/device/function adresi.
    pub pci_addr: PciAddress,
    /// Bu sürücünün kullandığı IRQ vektörleri.
    pub irq_vectors: [u32; 8],
    pub irq_count: usize,
    /// Aktif mi?
    pub active: bool,
}

/// Her sürücüyü tip düzeyinde ayıran etiket (PhantomData için).
pub struct DriverTag;

// ============================================================================
// 2. Global Sürücü Registry — Yüklü sürücülerin listesi
// ============================================================================

/// Maksimum eşzamanlı izole sürücü sayısı.
const MAX_ISOLATED_DRIVERS: usize = 32;

static ISOLATED_DRIVERS: Mutex<[Option<IsolatedDriver>; MAX_ISOLATED_DRIVERS]> =
    Mutex::new([const { None }; MAX_ISOLATED_DRIVERS]);

/// İzole bir sürücüyü kayıt et. Başarılı olursa slot index'i döner.
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
    crate::serial_println!("[IronShim/Bridge] ERROR: All {} driver slots full", MAX_ISOLATED_DRIVERS);
    Err(ShimError::OutOfMemory)
}

/// Bir sürücüyü kayıttan sil.
pub fn unregister_isolated_driver(slot: usize) -> Result<(), ShimError> {
    let mut slots = ISOLATED_DRIVERS.lock();
    if slot >= MAX_ISOLATED_DRIVERS {
        return Err(ShimError::InvalidAddress);
    }
    if let Some(ref driver) = slots[slot] {
        crate::serial_println!(
            "[IronShim/Bridge] Driver '{}' unregistered from slot {}",
            driver.name, slot
        );
    }
    slots[slot] = None;
    Ok(())
}

/// Kayıtlı sürücü sayısını döner.
pub fn active_driver_count() -> usize {
    ISOLATED_DRIVERS.lock().iter().filter(|s| s.is_some()).count()
}

// ============================================================================
// 3. PCI BAR → Manifest Data Otomatik Oluşturma
// ============================================================================

/// PCI BAR bilgilerinden MMIO ve Port I/O descriptor'larını çıkarır.
/// Dönen değerler doğrudan `IsolatedDriver`'a gömülür.
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

/// MMIO ve Port bilgilerinden bir ResourceManifest oluşturur (runtime doğrulama için).
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
/// ## Güvenlik Adımları
/// 1. IronShim PCI parse ile BAR bilgilerini al
/// 2. BAR'lardan MMIO/Port whitelist çıkar
/// 3. `IsolatedDriver` oluştur (manifest + budget)
/// 4. Registry'ye kaydet
/// 5. Gerçek C FFI `probe()` çağır
/// 6. Audit logla
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
                    dev.vendor_id, dev.device_id,
                    dev.bus, dev.device, dev.function
                );

                // IronShim PCI parse — BAR bilgileri
                let desc = match ironshim_rs::parse_pci_function(
                    &EchOsPciConfig,
                    dev.bus, dev.device, dev.function,
                ) {
                    Ok(d) => d,
                    Err(_) => {
                        crate::serial_println!("[IronShim/Bridge] PCI parse failed, skipping");
                        unsafe { id_ptr = id_ptr.add(1); }
                        continue;
                    }
                };

                // BAR'lardan manifest data çıkar
                let (mmio, mmio_count, ports, port_count) =
                    extract_manifest_from_bars(&desc.bars, desc.bar_len);

                // Manifest doğrulama
                if let Err(e) = build_manifest(mmio, mmio_count, ports, port_count) {
                    crate::serial_println!(
                        "[IronShim/Bridge] Manifest validation failed: {:?}", e
                    );
                    IRONSHIM_AUDIT.record(AuditEvent::ManifestRejected);
                    unsafe { id_ptr = id_ptr.add(1); }
                    continue;
                }

                // IsolatedDriver oluştur ve kaydet
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
                    unsafe { id_ptr = id_ptr.add(1); }
                    continue;
                }

                // Gerçek Linux C FFI probe() çağrısı
                let linux_dev = unsafe {
                    crate::linux_glue::create_pci_dev(dev.bus, dev.device, dev.function)
                };
                if linux_dev.is_null() { break; }

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
                                driver, linux_dev,
                                dev.bus, dev.device, dev.function,
                            );
                        }
                        crate::serial_println!(
                            "[IronShim/Bridge] ✅ '{}' bound to {:02x}:{:02x}.{} (ISOLATED)",
                            driver_name, dev.bus, dev.device, dev.function
                        );
                        IRONSHIM_AUDIT.record(AuditEvent::ManifestValidated);
                        claimed += 1;
                        break;
                    }
                }

                unsafe { crate::linux_glue::destroy_pci_dev(linux_dev); }
            }
            unsafe { id_ptr = id_ptr.add(1); }
        }
    }

    if claimed > 0 { 0 } else { -1 }
}

// ============================================================================
// 5. Güvenli DMA Tahsis — IronShim bounds-checked
// ============================================================================

/// IronShim güvenlik katmanı üzerinden DMA belleği tahsis eder.
pub fn safe_dma_alloc<T>(count: usize) -> Result<DmaHandle<'static, T, EchOsDmaAllocator>, ShimError> {
    let handle = IRONSHIM_DMA.alloc::<T>(count)?;
    crate::serial_println!(
        "[IronShim/Bridge] DMA alloc: {} x {} = {} bytes (phys={:#x})",
        count, core::mem::size_of::<T>(),
        count * core::mem::size_of::<T>(), handle.phys()
    );
    Ok(handle)
}

// ============================================================================
// 6. Güvenli IRQ Kaydı — Budget korumalı
// ============================================================================

/// IronShim budget korumalı IRQ kaydı.
pub fn safe_request_irq(
    irq: u32,
    handler: &'static mut dyn ironshim_rs::InterruptHandler,
    budget: InterruptBudget,
) -> Result<(), ShimError> {
    IRONSHIM_IRQ.register_with_budget(irq, handler, budget)?;
    crate::serial_println!(
        "[IronShim/Bridge] IRQ {} registered (budget: max_calls={}, max_ticks={})",
        irq, budget.max_calls, budget.max_ticks,
    );
    Ok(())
}

// ============================================================================
// 7. Güvenli Syscall Enforce — IronShim + Seccomp
// ============================================================================

/// Bir syscall'u IronShim policy + audit üzerinden doğrula.
pub fn safe_enforce_syscall(number: u32, args: [usize; 6]) -> Result<(), ShimError> {
    let request = SyscallRequest { number, args };
    enforce_syscall(&IRONSHIM_POLICY, &IRONSHIM_AUDIT, &request)
}

// ============================================================================
// 8. Bridge Başlatma
// ============================================================================

/// IronShim Bridge'i başlat. Kernel boot sırasında çağrılır.
pub fn init_ironshim_bridge() {
    crate::serial_println!("╔══════════════════════════════════════════════════╗");
    crate::serial_println!("║   IronShim Bridge — Active                      ║");
    crate::serial_println!("║   Capability-Based Driver Isolation Layer        ║");
    crate::serial_println!("║   DMA Guard │ IRQ Budget │ Manifest Validator    ║");
    crate::serial_println!("╚══════════════════════════════════════════════════╝");
}
