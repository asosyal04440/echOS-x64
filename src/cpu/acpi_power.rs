//! # echOS ACPI Güç Yöneticisi (OSPM — OS-directed Power Management)
//!
//! ## OSPM Nedir?
//! OSPM (Operating System-directed Power Management), ACPI modelinde işletim sisteminin
//! güç yönetimini doğrudan kontrol ettiği mimaridir. BIOS/firmware pasif bir rol üstlenerek
//! yalnızca AML metodlarla politika sunar; gerçek kararları işletim sistemi verir.
//!
//! ## Güç Durumu Hiyerarşisi
//! ```text
//!  ┌──────────────────────────────────────────────────────────────────────┐
//!  │                     ACPI Güç Durumları                              │
//!  ├─────────────┬────────────────────────────────────────────────────────┤
//!  │  S-States   │  Sistem geneli uyku durumları                          │
//!  │  (Global)   │  S0:Çalışıyor │ S3:Askı │ S4:Hazırda Bek │ S5:Kapalı  │
//!  ├─────────────┼────────────────────────────────────────────────────────┤
//!  │  C-States   │  CPU boşta durum yönetimi                              │
//!  │  (CPU-lokal)│  C0:Çalışıyor │ C1:HLT │ C2:Stop-Grant │ C3:Sleep     │
//!  ├─────────────┼────────────────────────────────────────────────────────┤
//!  │  P-States   │  CPU performans/frekans ölçeklendirmesi (DVFS)         │
//!  │  (CPU-lokal)│  P0:Maks Frekans → Pn:Min Frekans (güç tasarrufu)     │
//!  ├─────────────┼────────────────────────────────────────────────────────┤
//!  │  D-States   │  Bireysel ACPI cihazlarının güç durumları              │
//!  │  (Cihaz)    │  D0:Tam Güç │ D1/D2:Uyku │ D3Hot/D3Cold:Kapalı        │
//!  └─────────────┴────────────────────────────────────────────────────────┘
//! ```
//!
//! ## P-State (Performance State) — DVFS
//! DVFS (Dynamic Voltage and Frequency Scaling) ile CPU frekansı ve gerilimi
//! çalışma yüküne göre dinamik olarak ayarlanır.
//! ```text
//!  P0 →  3.5 GHz, 1.2V  (maksimum performans)
//!  P1 →  2.8 GHz, 1.1V
//!  P2 →  2.1 GHz, 1.0V
//!  P3 →  1.4 GHz, 0.9V  (minimum güç tüketimi)
//! ```
//!
//! ## C-State (CPU Idle State) — Boşta Güç Yönetimi
//! ```text
//!  C0 → CPU çalışıyor (normal işlem)
//!  C1 → HLT komutu ile durduruldu; ilk kesmede anında uyandırma
//!  C2 → PLT (Stop-Grant) durumu; C1'den daha derin, daha fazla güç tasarrufu
//!  C3 → Sleep durumu; L2 önbellek flush edilebilir; daha yüksek gecikme
//! ```
//!
//! Faz 5: Tam güç yönetimi — S-states, C-states, P-states.
//! _PTS/_WAK, _PSS/_PCT, _CST AML metot değerlendirmesi.

use alloc::vec::Vec;
use aml::AmlValue;

// ============================================================================
// Uyku Durumu Yönetimi (S-States)
// ============================================================================

/// Firmware tarafından desteklenen ACPI uyku durumlarını tespit eder (`\_S0` — `\_S5`).
///
/// Firmware'in her uyku durumu için AML namespace'te `\_Sn` nesnesi olması gerekir.
/// Bu nesneyi evaluate etmek mümkünse durum destekleniyor demektir.
pub fn get_supported_sleep_states() -> Vec<u8> {
    let mut supported = Vec::new();

    if !crate::cpu::acpi_aml::is_initialized() {
        return supported;
    }

    // _S0.._S5 arasında her uyku durumunu dene
    for state in 0..=5 {
        let path = alloc::format!("\\_S{}", state);
        if crate::cpu::acpi_aml::invoke_method(&path, &[]).is_ok() {
            supported.push(state);
        }
    }

    crate::serial_println!("[OSPM] Supported sleep states: {:?}", supported);
    supported
}

/// Uyku durumuna hazırlan — `\_PTS(sleep_state)` AML metodunu çalıştırır.
///
/// ACPI Spec §7.4.1: `_PTS` (Prepare To Sleep), uyku girişinden önce firmware'in
/// donanımı uyku moduna hazırlaması için çağrılır. Aygıt sürücüleri bu sırada
/// bağlamlarını kaydeder, G/Ç işlemlerini durdurur.
///
/// `sleep_state`: 1=S1, 3=S3, 4=S4, 5=S5
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

/// Uyku durumundan uyandıktan sonra — `\_WAK(sleep_state)` AML metodunu çalıştırır.
///
/// ACPI Spec §7.4.2: `_WAK` (System Wake), uyku modundan çıkıldıktan sonra
/// firmware'in donanımı yeniden aktif duruma geçirmesi için çağrılır.
/// Aygıt sürücüleri bu sırada bağlamlarını geri yükler.
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
// CPU Performans Durumları (P-States) — DVFS
// ============================================================================

/// Tek bir CPU P-state'inin tüm özelliklerini içeren yapı.
///
/// `_PSS` (Performance Supported States) metodu bu yapı listesini döndürür.
/// P0 = en yüksek frekans (en fazla güç), Pn = en düşük frekans (en az güç).
#[derive(Debug, Clone)]
pub struct PStateInfo {
    /// CPU çalışma frekansı (MHz cinsinden)
    pub frequency: u32,
    /// Tahmini güç tüketimi (miliwatt cinsinden)
    pub power: u32,
    /// Bu P-state'e geçiş gecikmesi (mikrosaniye)
    pub transition_latency: u32,
    /// Bus master gecikmesi (mikrosaniye) — C3 ve benzeri durumlarla etkileşim
    pub bus_master_latency: u32,
    /// Performans kontrol register değeri — IA32_PERF_CTL MSR'a yazılır
    pub control: u32,
    /// Beklenen durum değeri — IA32_PERF_STATUS MSR'dan doğrulama için okunur
    pub status: u32,
}

/// CPU'nun desteklediği P-state listesini `_PSS` AML metodundan okur.
///
/// `_PSS` (Performance Supported States) paketi, her P-state için 6 elemanlı
/// bir alt paket içerir. Birden fazla CPU namespace yolu denenir
/// (farklı firmware'ler farklı yollar kullanır).
pub fn get_pstate_list() -> Vec<PStateInfo> {
    let mut pstates = Vec::new();

    // Farklı firmware'lerin kullandığı CPU namespace yolları
    let paths = ["\\_PR.CPU0._PSS", "\\_SB.CPU0._PSS", "\\_PR.C000._PSS"];

    for path in &paths {
        if let Ok(AmlValue::Package(elements)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            for elem in &elements {
                if let AmlValue::Package(fields) = elem {
                    if fields.len() >= 6 {
                        let pstate = PStateInfo {
                            frequency:          aml_to_u32(&fields[0]),
                            power:              aml_to_u32(&fields[1]),
                            transition_latency: aml_to_u32(&fields[2]),
                            bus_master_latency: aml_to_u32(&fields[3]),
                            control:            aml_to_u32(&fields[4]),
                            status:             aml_to_u32(&fields[5]),
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

/// AML `_PCT` (Performance Control) yolu üzerinden P-state değiştirir.
///
/// `_PCT` metodu performans kontrol register'ının adresini ve tipini döndürür.
/// Bu implementasyon MSR yolunu kullanır: IA32_PERF_CTL (0x199) Intel CPU'larda
/// frekans/gerilim geçişlerini yazılımdan kontrol etmeye olanak tanır.
pub fn set_pstate_via_aml(pstate_index: usize) -> bool {
    let pstates = get_pstate_list();
    if pstate_index >= pstates.len() {
        return false;
    }

    let control_value = pstates[pstate_index].control;

    // _PCT (Performance Control) kaydını al — MSR tabanlı kontrol yaygın
    let paths = ["\\_PR.CPU0._PCT", "\\_SB.CPU0._PCT", "\\_PR.C000._PCT"];
    for path in &paths {
        if let Ok(_) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            // MSR 0x199 = IA32_PERF_CTL — Intel SpeedStep/Enhanced SpeedStep P-state kontrolü
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
// CPU Boşta Durumları (C-States)
// ============================================================================

/// Tek bir C-state'in özelliklerini tutan yapı.
///
/// C-state'ler CPU boşta durum yönetimi için kullanılır;
/// daha derin C-state daha düşük güç ama daha yüksek uyandırma gecikmesi demektir.
#[derive(Debug, Clone)]
pub struct CStateInfo {
    /// C-state türü: 1=C1 (HLT), 2=C2 (Stop-Grant), 3=C3 (Sleep)
    pub ctype: u8,
    /// Bu C-state'ten çıkış gecikmesi (mikrosaniye)
    pub latency: u32,
    /// Bu C-state'de tahmini güç tüketimi (miliwatt)
    pub power: u32,
}

/// CPU'nun desteklediği C-state listesini `_CST` AML metodundan okur.
///
/// `_CST` ilk elemanı C-state sayısı olan bir paket döndürür;
/// devamındakiler her C-state için tanımlayıcı alt paketlerdir.
pub fn get_cstates() -> Vec<CStateInfo> {
    let mut cstates = Vec::new();

    let paths = ["\\_PR.CPU0._CST", "\\_SB.CPU0._CST", "\\_PR.C000._CST"];

    for path in &paths {
        if let Ok(AmlValue::Package(elements)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            // İlk eleman toplam C-state sayısını belirtir; gerçek veriler 1. indeksten başlar
            if elements.len() > 1 {
                for elem in &elements[1..] {
                    if let AmlValue::Package(fields) = elem {
                        if fields.len() >= 3 {
                            let cstate = CStateInfo {
                                ctype:   aml_to_u32(&fields[0]) as u8,
                                latency: aml_to_u32(&fields[1]),
                                power:   aml_to_u32(&fields[2]),
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
// Termal Yönetim (AML tabanlı)
// ============================================================================

/// Termal bölgenin mevcut sıcaklığını `_TMP` AML metoduyla okur.
///
/// Dönüş değeri Kelvin × 10 cinsindendir.
/// Örn: 3232 → 323.2 K → 50.2°C (Kelvin'den Celsius: K - 273.15)
pub fn get_temperature(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._TMP", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Kritik sıcaklık eşiğini `_CRT` AML metoduyla okur.
///
/// Bu sıcaklık aşıldığında işletim sistemi sistemi acil kapatmalıdır.
/// Kelvin × 10 cinsinden döner.
pub fn get_critical_temp(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._CRT", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Pasif soğutma eşiğini `_PSV` AML metoduyla okur.
///
/// Bu sıcaklık aşıldığında işletim sistemi CPU'yu kısıtlamalıdır (throttle).
/// Pasif soğutma, fan kullanmadan yalnızca CPU frekansını düşürerek çalışır.
pub fn get_passive_temp(tz_path: &str) -> Option<u32> {
    let path = alloc::format!("{}._PSV", tz_path);
    match crate::cpu::acpi_aml::invoke_method(&path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(val as u32),
        _ => None,
    }
}

/// Aktif soğutma (fan) eşik noktalarını `_AC0`.`_AC9` AML metodlarıyla okur.
///
/// Daha düşük indeksli `_ACx`, daha yüksek fan hızını tetikler.
/// `_AC0` en agresif soğutma, `_AC9` en sessiz soğutma eşiğidir.
/// Kelvin × 10 cinsinden döner; ilk bulunan metot yoksa döngü kesilir.
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
// Batarya Yönetimi (AML tabanlı) — Faz 6
// ============================================================================

/// AML üzerinden okunan batarya sabit bilgileri yapısı.
///
/// `_BIX` (ACPI 4.0+ genişletilmiş) veya `_BIF` (ACPI 1.0 uyumlu) metodundan okunur.
/// Bu bilgiler fabrika verilerini içerir; sürekli değişmez.
#[derive(Debug, Clone)]
pub struct BatteryInfoAml {
    /// Tasarım kapasitesi (mAh veya mWh — `_BIF[1]`)
    pub design_capacity: u32,
    /// Son tam şarj kapasitesi (mAh veya mWh — `_BIF[2]`)
    pub last_full_capacity: u32,
    /// Tasarım gerilimi (mV — `_BIF[4]`)
    pub design_voltage: u32,
    /// Batarya seri numarası (varsa)
    pub serial_number: Option<alloc::string::String>,
    /// Batarya model adı (varsa)
    pub model_number: Option<alloc::string::String>,
}

/// Batarya sabit bilgilerini AML üzerinden okur.
///
/// Önce ACPI 4.0+ `_BIX` denenir; bulunamazsa geriye uyumlu `_BIF` kullanılır.
/// Farklı platformlardaki EC yolları için birden fazla namespace yolu denenir.
pub fn get_battery_info_aml() -> Option<BatteryInfoAml> {
    // Önce _BIX (ACPI 4.0+), sonra _BIF (ACPI 1.0) denenir
    let paths = ["\\_SB.BAT0._BIX", "\\_SB.BAT0._BIF", "\\_SB.PCI0.LPC.EC.BAT0._BIF"];

    for path in &paths {
        if let Ok(AmlValue::Package(fields)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            if fields.len() >= 4 {
                return Some(BatteryInfoAml {
                    design_capacity:    aml_to_u32(&fields[1]),
                    last_full_capacity: aml_to_u32(&fields[2]),
                    design_voltage:     aml_to_u32(&fields[4]),
                    serial_number: aml_to_string(&fields.get(10)),
                    model_number:  aml_to_string(&fields.get(9)),
                });
            }
        }
    }
    None
}

/// AML üzerinden okunan anlık batarya durumu yapısı.
///
/// `_BST` (Battery Status) metodu tarafından döndürülür; sık sık sorgulanabilir.
#[derive(Debug, Clone)]
pub struct BatteryStatusAml {
    /// Batarya durumu bayrakları:
    /// 0=tam dolu, 1=boşalıyor (discharging), 2=şarj oluyor (charging), 4=kritik seviye
    pub state: u32,
    /// Anlık şarj/deşarj akımı (mA veya mW)
    pub rate: u32,
    /// Kalan kapasite (mAh veya mWh)
    pub remaining_capacity: u32,
    /// Anlık uçbirim gerilimi (mV)
    pub voltage: u32,
}

/// Batarya anlık durumunu `_BST` AML metoduyla okur.
pub fn get_battery_status_aml() -> Option<BatteryStatusAml> {
    let paths = ["\\_SB.BAT0._BST", "\\_SB.PCI0.LPC.EC.BAT0._BST"];

    for path in &paths {
        if let Ok(AmlValue::Package(fields)) = crate::cpu::acpi_aml::invoke_method(path, &[]) {
            if fields.len() >= 4 {
                return Some(BatteryStatusAml {
                    state:              aml_to_u32(&fields[0]),
                    rate:               aml_to_u32(&fields[1]),
                    remaining_capacity: aml_to_u32(&fields[2]),
                    voltage:            aml_to_u32(&fields[3]),
                });
            }
        }
    }
    None
}

// ============================================================================
// PCI IRQ Yönlendirme — Faz 3
// ============================================================================

/// PCI IRQ yönlendirme tablosu girişi.
///
/// `_PRT` (PCI Routing Table) metodu bu girişlerin listesini döndürür.
/// Her PCI cihazının interrupt pinlerinin hangi GSI'ye (Global System Interrupt) bağlandığını tanımlar.
#[derive(Debug, Clone)]
pub struct PciIrqEntry {
    /// PCI cihaz adresi: üst sözcük = cihaz numarası, alt sözcük = fonksiyon numarası
    pub address: u64,
    /// PCI interrupt pin: 0=INTA#, 1=INTB#, 2=INTC#, 3=INTD#
    pub pin: u8,
    /// Hedef GSI (Global System Interrupt) numarası
    pub gsi: u32,
    /// Interrupt link cihaz yolu — yönlendirme link cihazı üzerinden yapılıyorsa dolu
    pub source: Option<alloc::string::String>,
}

/// PCI IRQ yönlendirme tablosunu `_PRT` AML metodundan okur.
///
/// Bu tablo, PCI cihazlarının kesme bağlantılarını yapılandırmak için kullanılır.
/// PCI kök köprüsü (`\_SB.PCI0`) altında bulunur.
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
                        let pin  = aml_to_u32(&fields[1]) as u8;
                        let gsi  = aml_to_u32(&fields[3]);

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
// Yardımcı Dönüşüm Fonksiyonları
// ============================================================================

/// `AmlValue::Integer` varyantını `u32`'ye dönüştürür; diğer türler için 0 döner.
fn aml_to_u32(val: &AmlValue) -> u32 {
    match val {
        AmlValue::Integer(n) => *n as u32,
        _ => 0,
    }
}

/// `AmlValue::Integer` varyantını `u64`'e dönüştürür; diğer türler için 0 döner.
fn aml_to_u64(val: &AmlValue) -> u64 {
    match val {
        AmlValue::Integer(n) => *n,
        _ => 0,
    }
}

/// `AmlValue::String` varyantını `Option<String>`'e dönüştürür.
fn aml_to_string(val: &Option<&AmlValue>) -> Option<alloc::string::String> {
    match val {
        Some(AmlValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}
