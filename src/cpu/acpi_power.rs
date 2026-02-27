//! # echOS ACPI Power Manager (OSPM)
//!
//! Faz 5: Tam güç yönetimi — S-states, C-states, P-states.
//! _PTS/_WAK, _PSS/_PCT, _CST AML method evaluation.

use alloc::vec::Vec;
use aml::AmlValue;

// ============================================================================
// Uyku Durumu Yönetimi
// ============================================================================

/// Desteklenen uyku durumlarını tespit et (_S0.._S5)
pub fn get_supported_sleep_states() -> Vec<u8> {
    let mut supported = Vec::new();

    if !crate::cpu::acpi_aml::is_initialized() {
        return supported;
    }

    for state in 0..=5 {
        let path = alloc::format!("\\_S{}", state);
        if crate::cpu::acpi_aml::invoke_method(&path, &[]).is_ok() {
            supported.push(state);
        }
    }

    crate::serial_println!("[OSPM] Supported sleep states: {:?}", supported);
    supported
}

/// Uyku durumuna hazırlan — _PTS(sleep_state) çalıştır
/// ACPI Spec §7.4.1: Prepare To Sleep
pub fn prepare_to_sleep(sleep_state: u8) -> bool {
    let arg = AmlValue::Integer(sleep_state as u64);
    match crate::cpu::acpi_aml::invoke_method("\\_PTS", &[arg]) {
        Ok(_) => {
            crate::serial_println!("[OSPM] _PTS({}) OK", sleep_state);
            true
        }
        Err(e) => {
            crate::serial_println!("[OSPM] _PTS({}) failed: {:?}", sleep_state, e);
            false
        }
    }
}

/// Uyku durumundan uyanma — _WAK(sleep_state) çalıştır
/// ACPI Spec §7.4.2: System Wake
pub fn wake_from_sleep(sleep_state: u8) -> bool {
    let arg = AmlValue::Integer(sleep_state as u64);
    match crate::cpu::acpi_aml::invoke_method("\\_WAK", &[arg]) {
        Ok(_) => {
            crate::serial_println!("[OSPM] _WAK({}) OK", sleep_state);
            true
        }
        Err(e) => {
            crate::serial_println!("[OSPM] _WAK({}) failed: {:?}", sleep_state, e);
            false
        }
    }
}

// ============================================================================
// CPU Performance States (P-states) — DVFS
// ============================================================================

/// P-state bilgisi (_PSS dönüş elemanı)
#[derive(Debug, Clone)]
pub struct PStateInfo {
    /// Frekans (MHz)
    pub frequency: u32,
    /// Güç tüketimi (mW)
    pub power: u32,
    /// Geçiş gecikmesi (µs)
    pub transition_latency: u32,
    /// Bus master gecikmesi (µs)
    pub bus_master_latency: u32,
    /// Control register değeri
    pub control: u32,
    /// Status register değeri
    pub status: u32,
}

/// CPU0'ın desteklediği P-state'leri oku (_PSS)
pub fn get_pstate_list() -> Vec<PStateInfo> {
    let mut pstates = Vec::new();

    // \_PR.CPU0._PSS veya \_SB.CPU0._PSS denemesi
    let paths = ["\\_PR.CPU0._PSS", "\\_SB.CPU0._PSS", "\\_PR.C000._PSS"];

    for path in &paths {
        if let Ok(AmlValue::Package(elements)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            for elem in &elements {
                if let AmlValue::Package(fields) = elem {
                    if fields.len() >= 6 {
                        let pstate = PStateInfo {
                            frequency: aml_to_u32(&fields[0]),
                            power: aml_to_u32(&fields[1]),
                            transition_latency: aml_to_u32(&fields[2]),
                            bus_master_latency: aml_to_u32(&fields[3]),
                            control: aml_to_u32(&fields[4]),
                            status: aml_to_u32(&fields[5]),
                        };
                        pstates.push(pstate);
                    }
                }
            }
            if !pstates.is_empty() {
                crate::serial_println!(
                    "[OSPM] {} P-states from {} ({}MHz - {}MHz)",
                    pstates.len(),
                    path,
                    pstates.last().map_or(0, |p| p.frequency),
                    pstates.first().map_or(0, |p| p.frequency),
                );
                break;
            }
        }
    }

    pstates
}

/// P-state'i AML üzerinden değiştir (_PCT Performance Control)
pub fn set_pstate_via_aml(pstate_index: usize) -> bool {
    let pstates = get_pstate_list();
    if pstate_index >= pstates.len() {
        return false;
    }

    let control_value = pstates[pstate_index].control;

    // _PCT ile kontrol register'ını al
    let paths = ["\\_PR.CPU0._PCT", "\\_SB.CPU0._PCT", "\\_PR.C000._PCT"];
    for path in &paths {
        if let Ok(_) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            // MSR yoluyla P-state değiştir (IA32_PERF_CTL = 0x199)
            unsafe {
                let mut msr = x86_64::registers::model_specific::Msr::new(0x199);
                msr.write(control_value as u64);
            }
            crate::serial_println!(
                "[OSPM] P-state → {} ({}MHz, {}mW)",
                pstate_index,
                pstates[pstate_index].frequency,
                pstates[pstate_index].power
            );
            return true;
        }
    }

    false
}

// ============================================================================
// CPU C-states (Idle States)
// ============================================================================

/// C-state bilgisi
#[derive(Debug, Clone)]
pub struct CStateInfo {
    /// C-state tipi (1=C1, 2=C2, 3=C3)
    pub ctype: u8,
    /// Gecikme (µs)
    pub latency: u32,
    /// Güç tüketimi (mW)
    pub power: u32,
}

/// CPU0'ın desteklediği C-state'leri oku (_CST)
pub fn get_cstates() -> Vec<CStateInfo> {
    let mut cstates = Vec::new();

    let paths = ["\\_PR.CPU0._CST", "\\_SB.CPU0._CST", "\\_PR.C000._CST"];

    for path in &paths {
        if let Ok(AmlValue::Package(elements)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            // İlk eleman C-state sayısı
            if elements.len() > 1 {
                for elem in &elements[1..] {
                    if let AmlValue::Package(fields) = elem {
                        if fields.len() >= 3 {
                            let cstate = CStateInfo {
                                ctype: aml_to_u32(&fields[0]) as u8,
                                latency: aml_to_u32(&fields[1]),
                                power: aml_to_u32(&fields[2]),
                            };
                            cstates.push(cstate);
                        }
                    }
                }
            }
            if !cstates.is_empty() {
                crate::serial_println!("[OSPM] {} C-states from {}", cstates.len(), path);
                break;
            }
        }
    }

    cstates
}

// ============================================================================
// Thermal Management (AML-based)
// ============================================================================

/// Termal bölge sıcaklığını oku (_TMP) — Kelvin * 10 döner
pub fn get_temperature(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._TMP", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Kritik sıcaklık eşiği (_CRT) — bu sıcaklıkta sistem kapanır
pub fn get_critical_temp(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._CRT", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Pasif soğutma eşiği (_PSV) — CPU throttle başlatılır
pub fn get_passive_temp(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._PSV", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Aktif soğutma noktaları (_AC0.._AC9) — fan seviyeleri
pub fn get_active_cooling_points(tz_path: &str) -> Vec<u32> {
    let mut points = Vec::new();
    for i in 0..=9 {
        let path = alloc::format!("{}._AC{}", tz_path, i);
        match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
            Ok(AmlValue::Integer(val)) => points.push(val as u32),
            _ => break,
        }
    }
    points
}

// ============================================================================
// Battery (AML-based) — Faz 6
// ============================================================================

/// Batarya bilgisi (_BIF/_BIX)
#[derive(Debug, Clone)]
pub struct BatteryInfoAml {
    pub design_capacity: u32,
    pub last_full_capacity: u32,
    pub design_voltage: u32,
    pub serial_number: Option<alloc::string::String>,
    pub model_number: Option<alloc::string::String>,
}

/// Batarya bilgisini AML ile oku
pub fn get_battery_info_aml() -> Option<BatteryInfoAml> {
    // Önce _BIX (ACPI 4.0+), sonra _BIF (ACPI 1.0)
    let paths = ["\\_SB.BAT0._BIX", "\\_SB.BAT0._BIF", "\\_SB.PCI0.LPC.EC.BAT0._BIF"];

    for path in &paths {
        if let Ok(AmlValue::Package(fields)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            if fields.len() >= 4 {
                return Some(BatteryInfoAml {
                    design_capacity: aml_to_u32(&fields[1]),
                    last_full_capacity: aml_to_u32(&fields[2]),
                    design_voltage: aml_to_u32(&fields[4]),
                    serial_number: aml_to_string(&fields.get(10)),
                    model_number: aml_to_string(&fields.get(9)),
                });
            }
        }
    }
    None
}

/// Batarya durumu (_BST) — şarj durumu, akım, voltaj
#[derive(Debug, Clone)]
pub struct BatteryStatusAml {
    /// Durum (0=full, 1=discharging, 2=charging, 4=critical)
    pub state: u32,
    /// Mevcut akım (mA)
    pub rate: u32,
    /// Kalan kapasite (mAh)
    pub remaining_capacity: u32,
    /// Mevcut voltaj (mV)
    pub voltage: u32,
}

pub fn get_battery_status_aml() -> Option<BatteryStatusAml> {
    let paths = ["\\_SB.BAT0._BST", "\\_SB.PCI0.LPC.EC.BAT0._BST"];

    for path in &paths {
        if let Ok(AmlValue::Package(fields)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            if fields.len() >= 4 {
                return Some(BatteryStatusAml {
                    state: aml_to_u32(&fields[0]),
                    rate: aml_to_u32(&fields[1]),
                    remaining_capacity: aml_to_u32(&fields[2]),
                    voltage: aml_to_u32(&fields[3]),
                });
            }
        }
    }
    None
}

// ============================================================================
// PCI IRQ Routing — Faz 3
// ============================================================================

/// PCI IRQ routing entry
#[derive(Debug, Clone)]
pub struct PciIrqEntry {
    /// PCI device address (upper word = device, lower = function)
    pub address: u64,
    /// PCI interrupt pin (0=INTA, 1=INTB, 2=INTC, 3=INTD)
    pub pin: u8,
    /// GSI (Global System Interrupt) number
    pub gsi: u32,
    /// Source device path (eğer link device üzerinden routing varsa)
    pub source: Option<alloc::string::String>,
}

/// PCI IRQ routing tablosunu AML'den oku (_PRT)
pub fn get_pci_routing_table() -> Vec<PciIrqEntry> {
    let mut entries = Vec::new();

    if !crate::cpu::acpi_aml::is_initialized() {
        return entries;
    }

    let paths = ["\\_SB.PCI0._PRT", "\\_SB.PCI0.PRT0"];

    for path in &paths {
        if let Ok(AmlValue::Package(elements)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            for elem in &elements {
                if let AmlValue::Package(fields) = elem {
                    if fields.len() >= 4 {
                        let address = aml_to_u64(&fields[0]);
                        let pin = aml_to_u32(&fields[1]) as u8;
                        let gsi = aml_to_u32(&fields[3]);

                        entries.push(PciIrqEntry {
                            address,
                            pin,
                            gsi,
                            source: None,
                        });
                    }
                }
            }
            if !entries.is_empty() {
                crate::serial_println!("[OSPM] {} PCI IRQ routing entries from {}", entries.len(), path);
                break;
            }
        }
    }

    entries
}

// ============================================================================
// Yardımcı Fonksiyonlar
// ============================================================================

fn aml_to_u32(val: &AmlValue) -> u32 {
    match val {
        AmlValue::Integer(n) => *n as u32,
        _ => 0,
    }
}

fn aml_to_u64(val: &AmlValue) -> u64 {
    match val {
        AmlValue::Integer(n) => *n,
        _ => 0,
    }
}

fn aml_to_string(val: &Option<&AmlValue>) -> Option<alloc::string::String> {
    match val {
        Some(AmlValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}
