//! # echOS ACPI Device Manager
//!
//! Faz 4: ACPI namespace'teki cihazları keşfeder ve yönetir.
//! _STA, _PS0/_PS3, _CRS, _HID/_CID method'larını çalıştırır.

use alloc::string::String;
use alloc::vec::Vec;
use aml::AmlValue;

// ============================================================================
// ACPI Device Bilgileri
// ============================================================================

/// ACPI cihaz bilgisi
#[derive(Debug, Clone)]
pub struct AcpiDevice {
    /// ACPI namespace path (örn: "\\_SB.PCI0.SATA")
    pub path: String,
    /// Hardware ID (_HID)
    pub hid: Option<String>,
    /// Compatible ID (_CID)
    pub cid: Option<String>,
    /// Cihaz durumu (_STA) — bit mask
    pub status: u32,
    /// Mevcut güç durumu (D0-D3)
    pub power_state: DevicePowerState,
}

/// Device Power States (ACPI Spec §7.2)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevicePowerState {
    D0,       // Full power
    D1,       // Light sleep
    D2,       // Deeper sleep
    D3Hot,    // Off but still has power
    D3Cold,   // Completely off
    Unknown,
}

/// _STA dönüş değeri bit'leri (ACPI Spec §6.3.7)
pub const STA_PRESENT: u32 = 1 << 0;
pub const STA_ENABLED: u32 = 1 << 1;
pub const STA_VISIBLE: u32 = 1 << 2;
pub const STA_FUNCTIONING: u32 = 1 << 3;
pub const STA_BATTERY: u32 = 1 << 4;

// ============================================================================
// Cihaz Keşfi
// ============================================================================

/// Bilinen ACPI cihaz path'lerini tara ve _STA evaluate et.
pub fn enumerate_devices() -> Vec<AcpiDevice> {
    let mut devices = Vec::new();

    if !crate::cpu::acpi_aml::is_initialized() {
        return devices;
    }

    // Bilinen ACPI cihaz path'leri — QEMU i440fx/q35 ve gerçek donanım
    let known_paths = [
        "\\_SB",
        "\\_SB.PCI0",
        "\\_SB.PCI0.ISA",
        "\\_SB.PCI0.SATA",
        "\\_SB.PCI0.USB0",
        "\\_SB.PCI0.USB1",
        "\\_SB.PCI0.VGA",
        "\\_SB.PCI0.LPC",
        "\\_SB.PCI0.LPC.EC",
        "\\_SB.PCI0.HPET",
        "\\_SB.LID0",
        "\\_SB.SLPB",
        "\\_SB.PWRB",
        "\\_SB.BAT0",
        "\\_SB.AC",
        "\\_SB.FAN0",
    ];

    for path in &known_paths {
        if let Some(dev) = probe_device(path) {
            crate::serial_println!("[ACPI-DEV] Found: {} (status=0x{:X})", path, dev.status);
            devices.push(dev);
        }
    }

    crate::serial_println!("[ACPI-DEV] {} devices enumerated", devices.len());
    devices
}

/// Tek bir cihazı probe et — _STA evaluate et
fn probe_device(path: &str) -> Option<AcpiDevice> {
    // _STA evaluate et
    let sta_path = alloc::format!("{}._STA", path);
    let status = match crate::cpu::acpi_aml::invoke_method(&sta_path, &[]) {
        Ok(AmlValue::Integer(val)) => val as u32,
        _ => 0xF, // _STA yoksa cihaz varsayılan olarak present+enabled+functional
    };

    // Cihaz present değilse atla
    if status & STA_PRESENT == 0 {
        return None;
    }

    // _HID oku
    let hid_path = alloc::format!("{}._HID", path);
    let hid = match crate::cpu::acpi_aml::invoke_method(&hid_path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(eisaid_to_string(val)),
        Ok(AmlValue::String(s)) => Some(s),
        _ => None,
    };

    // _CID oku
    let cid_path = alloc::format!("{}._CID", path);
    let cid = match crate::cpu::acpi_aml::invoke_method(&cid_path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(eisaid_to_string(val)),
        Ok(AmlValue::String(s)) => Some(s),
        _ => None,
    };

    Some(AcpiDevice {
        path: String::from(path),
        hid,
        cid,
        status,
        power_state: DevicePowerState::D0,
    })
}

// ============================================================================
// Device Power Management
// ============================================================================

/// Cihazı belirli bir güç durumuna geçir
pub fn set_device_power(path: &str, state: DevicePowerState) -> bool {
    let method = match state {
        DevicePowerState::D0 => "_PS0",
        DevicePowerState::D1 => "_PS1",
        DevicePowerState::D2 => "_PS2",
        DevicePowerState::D3Hot | DevicePowerState::D3Cold => "_PS3",
        DevicePowerState::Unknown => return false,
    };

    let full_path = alloc::format!("{}.{}", path, method);
    match crate::cpu::acpi_aml::invoke_method(&full_path, &[]) {
        Ok(_) => {
            crate::serial_println!("[ACPI-DEV] {} → {:?}", path, state);
            true
        }
        Err(_) => false,
    }
}

/// Cihazın mevcut kaynaklarını oku (_CRS)
pub fn get_current_resources(path: &str) -> Option<AmlValue> {
    let crs_path = alloc::format!("{}._CRS", path);
    crate::cpu::acpi_aml::invoke_method(&crs_path, &[]).ok()
}

/// Cihaz durumunu oku (_STA)
pub fn get_device_status(path: &str) -> u32 {
    let sta_path = alloc::format!("{}._STA", path);
    match crate::cpu::acpi_aml::invoke_method(&sta_path, &[]) {
        Ok(AmlValue::Integer(val)) => val as u32,
        _ => 0xF,
    }
}

// ============================================================================
// EISA ID Dönüşümü
// ============================================================================

/// Compressed EISA ID'yi insan-okunur string'e çevir
/// Örn: 0x41D00303 → "PNP0303" (PS/2 Keyboard)
fn eisaid_to_string(id: u64) -> String {
    let id = id as u32;
    let c1 = ((id >> 26) & 0x1F) as u8 + b'@';
    let c2 = ((id >> 21) & 0x1F) as u8 + b'@';
    let c3 = ((id >> 16) & 0x1F) as u8 + b'@';
    let n = id & 0xFFFF;
    alloc::format!("{}{}{}{:04X}", c1 as char, c2 as char, c3 as char, n)
}
