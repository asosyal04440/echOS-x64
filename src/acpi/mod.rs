//! # echOS ACPI Modülü
//! 
//! ACPI (Advanced Configuration and Power Interface) tablolarını bulma ve okuma.
//! UEFI config tablosundan RSDP adresini alır.

use uefi::table::cfg::{ACPI_GUID, ACPI2_GUID, ConfigTableEntry};
use log::info;

/// UEFI config tablosundan ACPI RSDP (Root System Description Pointer) adresini bulur.
/// 
/// Önce ACPI 2.0 RSDP arar, bulamazsa ACPI 1.0'a düşer.
/// 
/// # Dönüş
/// - `Some(adres)`: RSDP'nin fiziksel bellek adresi
/// - `None`: Hiçbir ACPI tablosu bulunamadı
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
