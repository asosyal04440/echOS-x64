//! # echOS ACPI Modülü
//!
//! ACPI (Advanced Configuration and Power Interface) tablolarını bulma ve okuma.
//! UEFI config tablosundan RSDP adresini alır.
//!
//! ## ACPI Başlatma Akışı
//! ```ascii
//! UEFI Önyükleyici
//!      |
//!      v
//! find_acpi_table() → RSDP fiziksel adresi
//!      |
//!      v
//! set_rsdp_address() → RSDP_PHYS atomik değişkeni
//!      |
//!      v
//! init() → AcpiTables::from_rsdp()
//!      |
//!      v
//! platform_info() → InterruptModel::Apic
//!      |
//!      v
//! APIC_INFO → madt::from_apic()
//! ```

use acpi::platform::interrupt::InterruptModel;
use acpi::{AcpiHandler, AcpiTables, PhysicalMapping};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

#[cfg(target_os = "uefi")]
use log::info;
#[cfg(target_os = "uefi")]
use uefi::table::cfg::{ConfigTableEntry, ACPI2_GUID, ACPI_GUID};

pub mod madt;

/// RSDP (Root System Description Pointer) fiziksel bellek adresi.
///
/// UEFI önyükleyiciden alınan adres burada saklanır; `init()` tarafından okunur.
static RSDP_PHYS: AtomicU64 = AtomicU64::new(0);

/// Küresel APIC yapılandırma bilgisi.
///
/// `init()` başarılı olduğunda MADT'tan çıkarılan APIC bilgisi burada saklanır.
pub static APIC_INFO: Mutex<madt::ApicInfo> = Mutex::new(madt::ApicInfo::empty());

/// RSDP fiziksel adresini kaydeder.
///
/// Sıfır olmayan adresler kabul edilir; sıfır geçilirse işlem yapılmaz.
pub fn set_rsdp_address(rsdp_phys: u64) {
    if rsdp_phys != 0 {
        RSDP_PHYS.store(rsdp_phys, Ordering::SeqCst);
    }
}

/// ACPI alt sistemini başlatır.
///
/// `RSDP_PHYS` adresinden ACPI tablolarını ayrıştırır ve APIC bilgisini çıkarır.
/// Başarılıysa `true`, başarısızsa `false` döner.
pub fn init() -> bool {
    let rsdp = RSDP_PHYS.load(Ordering::SeqCst);
    if rsdp == 0 {
        return false;
    }

    let handler = HhdmAcpiHandler;
    let tables = unsafe { AcpiTables::from_rsdp(handler, rsdp as usize) };
    let tables = match tables {
        Ok(tables) => tables,
        Err(_) => return false,
    };

    let platform_info = match tables.platform_info() {
        Ok(info) => info,
        Err(_) => return false,
    };

    match platform_info.interrupt_model {
        InterruptModel::Apic(apic) => {
            *APIC_INFO.lock() = madt::from_apic(&apic);
            true
        }
        _ => false,
    }
}

/// Küresel APIC yapılandırma bilgisinin klonunu döner.
pub fn get_apic_info() -> madt::ApicInfo {
    APIC_INFO.lock().clone()
}

/// HHDM (Higher Half Direct Map) tabanlı ACPI bellek eşleyici.
///
/// Fiziksel adresleri HHDM ofsetiyle sanal adrese çevirerek ACPI tablolarına
/// erişim sağlar. `AcpiHandler` trait'ini uygular.
#[derive(Clone, Copy)]
struct HhdmAcpiHandler;

impl AcpiHandler for HhdmAcpiHandler {
    /// Fiziksel bellek bölgesini sanal adres alanına eşler.
    ///
    /// HHDM ofseti eklenerek fiziksel adres sanal adrese dönüştürülür.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address =
            (crate::memory::active_physical_offset() + physical_address as u64) as *mut T;
        let virtual_address = NonNull::new(virtual_address).unwrap();
        PhysicalMapping::new(physical_address, virtual_address, size, size, *self)
    }

    /// Fiziksel bellek bölgesinin eşlemesini kaldırır.
    ///
    /// HHDM tabanlı eşleme için temizleme gerekmez; boş bırakılır.
    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}
}

/// UEFI config tablosundan ACPI RSDP (Root System Description Pointer) adresini bulur.
///
/// Önce ACPI 2.0 RSDP arar, bulamazsa ACPI 1.0'a düşer.
///
/// # Dönüş
/// - `Some(adres)`: RSDP'nin fiziksel bellek adresi
/// - `None`: Hiçbir ACPI tablosu bulunamadı
#[cfg(target_os = "uefi")]
pub fn find_acpi_table(config_entries: &[ConfigTableEntry]) -> Option<usize> {
    // Önce ACPI 2.0 ara (daha yeni ve kapsamlı)
    if let Some(entry) = config_entries.iter().find(|entry| entry.guid == ACPI2_GUID) {
        info!("ACPI 2.0 RSDP found at {:?}", entry.address);
        return Some(entry.address as usize);
    }

    // Bulunamazsa ACPI 1.0'a düş
    if let Some(entry) = config_entries.iter().find(|entry| entry.guid == ACPI_GUID) {
        info!("ACPI 1.0 RSDP found at {:?}", entry.address);
        return Some(entry.address as usize);
    }

    None
}
