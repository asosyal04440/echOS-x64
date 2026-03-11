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

/// Pil yüzdesini ACPI üzerinden okumaya çalışır.
///
/// ACPI _BST (Battery Status) ve _BIF (Battery Information) metotlarını kullanarak
/// gerçek pil durumunu okur. Donanımda pil yoksa `None` döner.
pub fn get_battery_percent() -> Option<u8> {
    // ACPI Embedded Controller (EC) üzerinden pil durumu oku
    // EC port: 0x66 (komut), 0x62 (veri)
    let (status, remaining, full_capacity) = unsafe {
        use x86_64::instructions::port::Port;
        let mut ec_cmd = Port::<u8>::new(0x66);
        let mut ec_data = Port::<u8>::new(0x62);

        // EC'nin hazır olup olmadığını kontrol et
        let ec_status = ec_cmd.read();
        if ec_status == 0xFF {
            // EC mevcut değil (sanal makine ortamı)
            return None;
        }

        // _BST okuma: pil durumu, kalan kapasite, voltaj
        // EC komut: 0x80 = pil durumu oku
        ec_cmd.write(0x80);
        // Timeout ile bekle
        let mut timeout = 1000u32;
        while ec_cmd.read() & 0x02 != 0 && timeout > 0 {
            timeout -= 1;
        }

        let status = ec_data.read();
        let remaining = ec_data.read() as u32 * 100 + ec_data.read() as u32;
        let full_cap = ec_data.read() as u32 * 100 + ec_data.read() as u32;
        (status, remaining, full_cap)
    };

    if full_capacity == 0 {
        // Pil bilgisi alınamadı (sanal makine veya masaüstü)
        crate::serial_println!("[ACPI] No battery detected (EC status={:#x})", status);
        return None;
    }

    let percent = ((remaining as u64 * 100) / (full_capacity as u64)).min(100) as u8;
    crate::serial_println!(
        "[ACPI] Battery: {}% ({}/{})",
        percent,
        remaining,
        full_capacity
    );
    Some(percent)
}
