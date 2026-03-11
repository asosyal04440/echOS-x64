//! # echOS ACPI Cihaz Yöneticisi
//!
//! ## ACPI Cihaz Modeli
//! ACPI namespace'te her donanım bileşeni bir "cihaz" nesnesiyle temsil edilir.
//! Bu nesneler birkaç standart AML metoda sahiptir:
//!
//! ```text
//! Cihaz Metotları:
//!   _HID  → Hardware ID (Donanım Kimliği) — örn: "PNP0303" = PS/2 klavye
//!   _CID  → Compatible ID (Uyumlu Kimlik) — alternatif kimlikler
//!   _STA  → Status (Durum) — cihazın etkin/mevcut olup olmadığı (bit maskesi)
//!   _CRS  → Current Resource Settings — IRQ, I/O port, bellek bölgesi gibi kaynaklar
//!   _PRS  → Possible Resource Settings — cihazın desteklediği kaynak alternatifleri
//!   _PS0  → Power Set D0 — cihazı tam güç durumuna geçir
//!   _PS3  → Power Set D3 — cihazı uyku/kapalı durumuna geçir
//!   _INI  → Initialize — cihaz başlatma prosedürü
//! ```
//!
//! ## Cihaz Güç Durumları (D-States)
//! ```text
//!   D0 → Tam Güç    : Cihaz tam hızda çalışıyor
//!   D1 → Hafif Uyku : Platform/cihaz bağımlı; bazı güç tasarrufu
//!   D2 → Derin Uyku : D1'den daha fazla güç tasarrufu; daha yüksek gecikme
//!   D3Hot → Bağlı-Kapalı : Güç hattı aktif ama cihaz kapalı; hızlı uyandırma
//!   D3Cold → Tam Kapalı  : Hiç güç yok; yalnızca çıkarma/takma ile uyandırma
//! ```
//!
//! ACPI Faz 4: ACPI namespace'teki cihazları keşfeder ve yönetir.
//! _STA, _PS0/_PS3, _CRS, _HID/_CID metotlarını çalıştırır.

use alloc::string::String;
use alloc::vec::Vec;
use aml::AmlValue;

// ============================================================================
// ACPI Cihaz Bilgileri
// ============================================================================

/// Keşfedilen bir ACPI cihazının tüm bilgilerini tutan yapı.
/// Her cihaz ACPI namespace'teki konumuyla (yolu) tanımlanır.
#[derive(Debug, Clone)]
pub struct AcpiDevice {
    /// ACPI namespace yolu — cihazın namespace içindeki tam adresi
    /// Örn: `\\_SB.PCI0.SATA`, `\\_SB.LID0`
    pub path: String,
    /// Donanım Kimliği (_HID) — PnP veya ACPI kimlik dizesi
    /// Örn: "PNP0303" (PS/2 klavye), "ACPI0003" (güç adaptörü)
    pub hid: Option<String>,
    /// Uyumlu Kimlik (_CID) — ek sürücü uyumluluğu için alternatif kimlik
    pub cid: Option<String>,
    /// Cihaz durumu (_STA dönüş değeri) — aşağıdaki sabitlerle yorumlanır
    pub status: u32,
    /// Mevcut cihaz güç durumu (D0-D3Cold)
    pub power_state: DevicePowerState,
}

/// ACPI Cihaz Güç Durumları (ACPI Spec §7.2).
///
/// Her cihaz, bağımsız olarak bir D-state'te bulunabilir.
/// S-state geçişleri (sistem geneli uyku) sırasında tüm cihazlar uygun D-state'e geçirilir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevicePowerState {
    D0,      // Tam güç — cihaz aktif ve tam işlevsel
    D1,      // Hafif uyku — hafif güç tasarrufu, düşük gecikme
    D2,      // Derin uyku — daha fazla güç tasarrufu
    D3Hot,   // Yazılım kapalı — güç hattı aktif; hızlı uyandırma mümkün
    D3Cold,  // Fiziksel kapalı — güç hattı kesildi; en yüksek tasarruf
    Unknown, // Durum bilinmiyor
}

/// `_STA` metot dönüş değerinin bit maskesi tanımları (ACPI Spec §6.3.7).
///
/// ```text
///   Bit 0: Cihaz fiziksel olarak mevcut mu?
///   Bit 1: Cihaz etkin (I/O deşifre, kesme etkin) mi?
///   Bit 2: UI'da görünür (Cihaz Yöneticisi'nde gösterilsin mi) mi?
///   Bit 3: Tamamen işlevsel mi?
///   Bit 4: Pil durumu göstergesi mi? (yalnızca pil aygıtları)
/// ```
pub const STA_PRESENT: u32 = 1 << 0; // Cihaz fiziksel olarak var
pub const STA_ENABLED: u32 = 1 << 1; // Cihaz etkin ve adres çözme aktif
pub const STA_VISIBLE: u32 = 1 << 2; // Kullanıcı arayüzünde görünür
pub const STA_FUNCTIONING: u32 = 1 << 3; // Cihaz tamamen işlevsel
pub const STA_BATTERY: u32 = 1 << 4; // Batarya durumunu göster

// ============================================================================
// Cihaz Keşfi
// ============================================================================

/// Bilinen ACPI cihaz yollarını tarar, her biri için `_STA` değerlendirir
/// ve mevcut olan cihazların listesini döndürür.
///
/// Gerçek bir ACPI sürücüsü namespace'i özyinelemeli (recursive) gezer;
/// bu implementasyon QEMU i440fx/q35 ve yaygın gerçek donanım için sabit yolları kullanır.
pub fn enumerate_devices() -> Vec<AcpiDevice> {
    let mut devices = Vec::new();

    if !crate::cpu::acpi_aml::is_initialized() {
        return devices;
    }

    // Yaygın ACPI cihaz yolları — QEMU i440fx/q35 ve gerçek dizüstü/masaüstü donanım
    let known_paths = [
        "\\_SB",             // System Bus kök nesnesi
        "\\_SB.PCI0",        // PCI kök köprüsü (Host Bridge)
        "\\_SB.PCI0.ISA",    // ISA köprüsü
        "\\_SB.PCI0.SATA",   // SATA denetleyicisi
        "\\_SB.PCI0.USB0",   // USB denetleyicisi 0
        "\\_SB.PCI0.USB1",   // USB denetleyicisi 1
        "\\_SB.PCI0.VGA",    // Ekran kartı
        "\\_SB.PCI0.LPC",    // LPC (Low Pin Count) köprüsü
        "\\_SB.PCI0.LPC.EC", // Embedded Controller (EC) — dizüstülerde fan/batarya
        "\\_SB.PCI0.HPET",   // High Precision Event Timer
        "\\_SB.LID0",        // Kapak anahtarı (dizüstü)
        "\\_SB.SLPB",        // Uyku düğmesi
        "\\_SB.PWRB",        // Güç düğmesi
        "\\_SB.BAT0",        // Birincil batarya
        "\\_SB.AC",          // AC güç adaptörü
        "\\_SB.FAN0",        // Fan denetleyicisi
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

/// Tek bir cihazı yoklar: `_STA` çalıştırır ve cihaz mevcutsa `_HID`/`_CID` okur.
///
/// `_STA` metodu 0xF (1111 ikili) döndürürse cihaz tamamen aktif demektir.
/// Metot yoksa da cihaz varsayılan olarak mevcut ve etkin kabul edilir.
fn probe_device(path: &str) -> Option<AcpiDevice> {
    // _STA (Status) — cihazın mevcut olup olmadığını öğren
    let sta_path = alloc::format!("{}._STA", path);
    let status = match crate::cpu::acpi_aml::invoke_method(&sta_path, &[]) {
        Ok(AmlValue::Integer(val)) => val as u32,
        // _STA metodu tanımlanmamışsa cihaz varsayılan olarak mevcut + etkin + işlevsel
        _ => 0xF,
    };

    // STA_PRESENT biti set değilse cihaz fiziksel olarak yok — atla
    if status & STA_PRESENT == 0 {
        return None;
    }

    // _HID (Hardware ID) — PnP kimliği tam sayı veya dize olabilir
    let hid_path = alloc::format!("{}._HID", path);
    let hid = match crate::cpu::acpi_aml::invoke_method(&hid_path, &[]) {
        Ok(AmlValue::Integer(val)) => Some(eisaid_to_string(val)), // Sıkıştırılmış EISA ID
        Ok(AmlValue::String(s)) => Some(s),                        // ACPI ID dizesi
        _ => None,
    };

    // _CID (Compatible ID) — ek uyumluluk kimliği; sürücü eşleştirme için kullanılır
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
        power_state: DevicePowerState::D0, // Başlangıçta tam güç varsayılır
    })
}

// ============================================================================
// Cihaz Güç Yönetimi
// ============================================================================

/// Belirtilen cihazı hedef güç durumuna geçirir.
///
/// Güç durumu geçişi, ACPI namespace'teki ilgili `_PSx` metodu çalıştırılarak yapılır.
/// `_PS0` → D0 (tam güç), `_PS3` → D3 (kapalı) için kullanılır.
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

/// Cihazın mevcut kaynak ayarlarını okur (`_CRS` — Current Resource Settings).
///
/// Dönen `AmlValue::Buffer`, IRQ, I/O port veya bellek bölgesi gibi
/// cihaz kaynaklarını tanımlayan Kaynak Tanımlayıcı (Resource Descriptor) baytları içerir.
pub fn get_current_resources(path: &str) -> Option<AmlValue> {
    let crs_path = alloc::format!("{}._CRS", path);
    crate::cpu::acpi_aml::invoke_method(&crs_path, &[]).ok()
}

/// Cihaz durumunu `_STA` metodu ile okur.
/// Metot tanımlı değilse 0xF (tam işlevsel) döndürülür.
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

/// Sıkıştırılmış EISA ID'yi okunabilir PnP kimlik dizesine dönüştürür.
///
/// EISA ID formatı: ilk 3 karakter üreticiden (5-bit harfler), son 4 karakter ürün (hex).
/// ```text
/// Örn: 0x41D00303 → "PNP0303" (PS/2 Klavye Denetleyicisi)
///      0x41D00F03 → "PNP0F03" (PS/2 Fare)
///      0x41D00501 → "PNP0501" (NS16550 Seri Port)
/// ```
///
/// Dönüşüm algoritması: Her 5-bit blok, ASCII 'A'-'Z' aralığına '@' (0x40) eklenerek eşlenir.
fn eisaid_to_string(id: u64) -> String {
    let id = id as u32;
    let c1 = ((id >> 26) & 0x1F) as u8 + b'@'; // Üretici kodu, 1. harf
    let c2 = ((id >> 21) & 0x1F) as u8 + b'@'; // Üretici kodu, 2. harf
    let c3 = ((id >> 16) & 0x1F) as u8 + b'@'; // Üretici kodu, 3. harf
    let n = id & 0xFFFF; // Ürün numarası (4 hex rakam)
    alloc::format!("{}{}{}{:04X}", c1 as char, c2 as char, c3 as char, n)
}
