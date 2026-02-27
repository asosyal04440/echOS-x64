//! # echOS ACPI Modülü
//!
//! ACPI (Advanced Configuration and Power Interface) tablolarını bulma ve okuma.
//! UEFI config tablosundan RSDP adresini alır.

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

static RSDP_PHYS: AtomicU64 = AtomicU64::new(0);
pub static APIC_INFO: Mutex<madt::ApicInfo> = Mutex::new(madt::ApicInfo::empty());

/// RSDP fiziksel adresini çekirdek genelinde paylaşılan atomik değişkene yaz.
///
/// UEFI Boot Services `get_config_table()` ile ACPI2_GUID'e sahip tablo
/// bulunduğunda çağrılır. `SeqCst` sıralama kullanılır çünkü bu değer
/// ikincil CPU'lar başlamadan önce tamamen görünür olmalıdır.
pub fn set_rsdp_address(rsdp_phys: u64) {
    if rsdp_phys != 0 {
        RSDP_PHYS.store(rsdp_phys, Ordering::SeqCst);
    }
}

/// RSDP'nin fiziksel adresini döndür, henüz ayarlanmadıysa 0 döner.
///
/// ## Neden AtomicU64 + SeqCst?
///
/// RSDP adresi `Init` domain'inde yalnızca bir kez yazılır, üzerine yazılmaz.
/// Ancak:
///   - Farklı CPU çekirdekleri (SMP) bu değeri okuyabilir.
///   - Compiler, `static mut` okuma/yazmada yeniden sıralama yapabilir.
///
/// `AtomicU64` + `SeqCst` ile hem donanım hemde derleyici bellek bariyeri
/// garanti edilir → okuma her zaman en güncel adresi döndürür.
///
/// Örnek kullanım:
/// ```
/// let rsdp = acpi::get_rsdp_address(); // → 0xE0000 (BIOS) veya 0x7FE9000 (UEFI)
/// ```
pub fn get_rsdp_address() -> u64 {
    RSDP_PHYS.load(Ordering::SeqCst)
}

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

pub fn get_apic_info() -> madt::ApicInfo {
    APIC_INFO.lock().clone()
}

#[derive(Clone, Copy)]
struct HhdmAcpiHandler;

impl AcpiHandler for HhdmAcpiHandler {
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
