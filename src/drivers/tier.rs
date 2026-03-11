//! # echOS İki Katmanlı Sürücü Kast Sistemi — Tier Classifier
//!
//! PCI cihazlarını performans kritikliğine göre sınıflandırır:
//!
//! ```text
//!  PCI Device Probe
//!        │
//!        ▼
//!  classify_device(vendor, device, class, subclass)
//!        │
//!  ┌─────┴──────┐
//!  │             │
//!  ▼             ▼
//!  TIER 1        TIER 2
//!  ŞAH DAMARI    AMELE
//!  ────────────  ────────────
//!  NVMe SSD      WiFi
//!  100G NIC      Audio (HDA)
//!  GPU/Display   USB (xHCI)
//!                Bluetooth
//!                Serial
//!                PS/2
//!
//!  %100 Native   IronShim Jail
//!  Rust          İzole Thread
//!  Lock-Free     SPSC Ring OUT
//!  io_uring      Blocking OK
//!  MUTEX YASAK   Mutex OK
//! ```
//!
//! ## Kural
//!
//! - **TIER 1**: Gecikme-kritik donanım. Sürücü %100 Native Rust, lock-free
//!   io_uring-native olmalı. Hiçbir `Mutex`, `SpinLock`, veya blocking call
//!   içeremez.
//!
//! - **TIER 2**: Gecikmeye toleranslı donanım. Linux sürücüsü IronShim Jail
//!   içinde izole worker thread'de çalışır. echOS core ile sadece lock-free
//!   SPSC ring buffer üzerinden haberleşir.

/// Sürücü katmanı sınıflandırması
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverTier {
    /// TIER 1: Şah Damarı donanımları — Lock-free native Rust sürücü ZORUNLU
    ///
    /// NVMe, 100G NIC, GPU/Display Controller.
    /// io_uring entegre, zero-copy DMA, per-CPU queue.
    /// Mutex YASAK.
    Tier1Native,

    /// TIER 2: Amele donanımları — IronShim Jail sandbox içinde çalışır
    ///
    /// WiFi, Audio, USB, Bluetooth, Serial.
    /// Linux sürücüsü izole worker thread'de, blocking OK.
    /// echOS core'a sadece lock-free SPSC ring ile bağlı.
    Tier2Jailed,
}

/// Tier sınıflandırma nedeni (diagnostik amaçlı)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierReason {
    /// PCI class/subclass eşleşmesi
    PciClass,
    /// Vendor:Device whitelist'te
    Whitelist,
    /// Kullanıcı tarafından zorlanmış
    ForcedOverride,
    /// Varsayılan (bilinmeyen cihaz)
    Default,
}

/// Sınıflandırma sonucu
#[derive(Clone, Copy, Debug)]
pub struct TierClassification {
    pub tier: DriverTier,
    pub reason: TierReason,
    pub description: &'static str,
}

// ────────────────────────────────────────────────────────────
// PCI Class Codes (https://pci-ids.ucw.cz/read/PD)
// ────────────────────────────────────────────────────────────

/// PCI Base Class: Mass Storage Controller
const PCI_CLASS_STORAGE: u8 = 0x01;
/// PCI Subclass: NVMe (Non-Volatile Memory Controller)
const PCI_SUBCLASS_NVME: u8 = 0x08;

/// PCI Base Class: Network Controller
const PCI_CLASS_NETWORK: u8 = 0x02;
/// PCI Subclass: Ethernet Controller
const PCI_SUBCLASS_ETHERNET: u8 = 0x00;

/// PCI Base Class: Display Controller
const PCI_CLASS_DISPLAY: u8 = 0x03;
/// PCI Subclass: VGA Compatible Controller
const PCI_SUBCLASS_VGA: u8 = 0x00;
/// PCI Subclass: 3D Controller
const PCI_SUBCLASS_3D: u8 = 0x02;

/// PCI Base Class: Multimedia Controller (Audio)
const PCI_CLASS_MULTIMEDIA: u8 = 0x04;

/// PCI Base Class: Serial Bus Controller (USB)
const PCI_CLASS_SERIAL: u8 = 0x0C;

/// PCI Base Class: Wireless Controller
const PCI_CLASS_WIRELESS: u8 = 0x0D;

// ────────────────────────────────────────────────────────────
// TIER 1 Whitelist (Vendor:Device)
// ────────────────────────────────────────────────────────────

/// TIER 1'e zorlanmış Vendor:Device çiftleri
///
/// Bu listedeki cihazlar PCI class'larından bağımsız olarak
/// her zaman TIER 1 (native lock-free) sürücü alır.
static TIER1_WHITELIST: &[(u16, u16, &str)] = &[
    // Intel NVMe
    (0x8086, 0x0953, "Intel P3500/P3600/P3700 NVMe"),
    (0x8086, 0x0A54, "Intel Optane SSD"),
    // Samsung NVMe
    (0x144D, 0xA808, "Samsung 970 EVO Plus"),
    (0x144D, 0xA80A, "Samsung 980 PRO"),
    // Mellanox/NVIDIA ConnectX NIC
    (0x15B3, 0x1017, "Mellanox ConnectX-5 100G"),
    (0x15B3, 0x1019, "Mellanox ConnectX-6 200G"),
    (0x15B3, 0x101B, "Mellanox ConnectX-6 Dx"),
    // Intel E810 NIC
    (0x8086, 0x1592, "Intel E810-C 100G"),
    (0x8086, 0x1593, "Intel E810-XXV 25G"),
    // VirtIO devices (Simics/QEMU test ortamı)
    (0x1AF4, 0x1001, "VirtIO Block Device"),
    (0x1AF4, 0x1000, "VirtIO Network Device"),
    (0x1AF4, 0x1050, "VirtIO GPU"),
];

// ────────────────────────────────────────────────────────────
// Sınıflandırma Fonksiyonu
// ────────────────────────────────────────────────────────────

/// PCI cihazını TIER 1 (native) veya TIER 2 (jail) olarak sınıflandır.
///
/// # Karar Hiyerarşisi
///
/// 1. Whitelist kontrolü (vendor_id + device_id)
/// 2. PCI class/subclass tablosu
/// 3. Varsayılan: TIER 2 (güvenli taraf)
///
/// # Parametreler
///
/// - `vendor_id`: PCI vendor ID (ör: 0x8086 = Intel)
/// - `device_id`: PCI device ID
/// - `class`: PCI base class code
/// - `subclass`: PCI subclass code
pub fn classify_device(
    vendor_id: u16,
    device_id: u16,
    class: u8,
    subclass: u8,
) -> TierClassification {
    // 1. Whitelist kontrolü
    for &(wl_vendor, wl_device, desc) in TIER1_WHITELIST {
        if vendor_id == wl_vendor && device_id == wl_device {
            return TierClassification {
                tier: DriverTier::Tier1Native,
                reason: TierReason::Whitelist,
                description: desc,
            };
        }
    }

    // 2. PCI class/subclass tablosu
    match (class, subclass) {
        // ── TIER 1: Şah Damarı ──

        // NVMe Storage Controller (01:08)
        (PCI_CLASS_STORAGE, PCI_SUBCLASS_NVME) => TierClassification {
            tier: DriverTier::Tier1Native,
            reason: TierReason::PciClass,
            description: "NVMe Storage Controller",
        },

        // Ethernet NIC (02:00) — yüksek bant genişliği, düşük gecikme
        (PCI_CLASS_NETWORK, PCI_SUBCLASS_ETHERNET) => TierClassification {
            tier: DriverTier::Tier1Native,
            reason: TierReason::PciClass,
            description: "Ethernet Network Controller",
        },

        // GPU / Display Controller (03:00, 03:02)
        (PCI_CLASS_DISPLAY, PCI_SUBCLASS_VGA) | (PCI_CLASS_DISPLAY, PCI_SUBCLASS_3D) => {
            TierClassification {
                tier: DriverTier::Tier1Native,
                reason: TierReason::PciClass,
                description: "Display/GPU Controller",
            }
        }

        // ── TIER 2: Amele ──

        // Audio (04:xx) — gecikmeye toleranslı
        (PCI_CLASS_MULTIMEDIA, _) => TierClassification {
            tier: DriverTier::Tier2Jailed,
            reason: TierReason::PciClass,
            description: "Multimedia/Audio Controller",
        },

        // USB / Serial Bus (0C:xx)
        (PCI_CLASS_SERIAL, _) => TierClassification {
            tier: DriverTier::Tier2Jailed,
            reason: TierReason::PciClass,
            description: "Serial Bus Controller (USB/FireWire)",
        },

        // Wireless (0D:xx) — WiFi, Bluetooth
        (PCI_CLASS_WIRELESS, _) => TierClassification {
            tier: DriverTier::Tier2Jailed,
            reason: TierReason::PciClass,
            description: "Wireless Controller (WiFi/BT)",
        },

        // Diğer storage (AHCI, IDE, RAID) — TIER 2
        (PCI_CLASS_STORAGE, _) => TierClassification {
            tier: DriverTier::Tier2Jailed,
            reason: TierReason::PciClass,
            description: "Legacy Storage Controller",
        },

        // ── VARSAYILAN: TIER 2 (güvenli taraf) ──
        _ => TierClassification {
            tier: DriverTier::Tier2Jailed,
            reason: TierReason::Default,
            description: "Unknown Device (default Tier 2)",
        },
    }
}

/// Kullanıcı tarafından belirli bir cihazı TIER 1'e zorla
///
/// Runtime'da whitelist'e ekleme yapar.
/// DİKKAT: TIER 1 sürücü yoksa cihaz çalışmaz!
pub fn force_tier1(_vendor_id: u16, _device_id: u16) -> TierClassification {
    // TODO: Runtime whitelist'e ekleme
    TierClassification {
        tier: DriverTier::Tier1Native,
        reason: TierReason::ForcedOverride,
        description: "User-forced Tier 1",
    }
}

/// Sınıflandırma sonucunu seri porta logla
pub fn log_classification(vendor_id: u16, device_id: u16, result: &TierClassification) {
    let tier_str = match result.tier {
        DriverTier::Tier1Native => "TIER 1 [ŞAH DAMARI]",
        DriverTier::Tier2Jailed => "TIER 2 [AMELE]     ",
    };

    crate::serial_println!(
        "[TIER] {:04X}:{:04X} → {} — {} ({:?})",
        vendor_id,
        device_id,
        tier_str,
        result.description,
        result.reason
    );
}

/// PCI bus taramasından sonra tüm cihazları sınıflandır ve logla
pub fn classify_all_pci_devices() {
    crate::serial_println!("[TIER] ═══════════════════════════════════════════════");
    crate::serial_println!("[TIER]  İki Katmanlı Sürücü Kast Sistemi — Sınıflandırma");
    crate::serial_println!("[TIER] ═══════════════════════════════════════════════");

    // PCI cihazlarını tara ve sınıflandır
    let devices = crate::drivers::pci::scan();
    let mut tier1_count = 0u32;
    let mut tier2_count = 0u32;

    for dev in &devices {
        let result = classify_device(dev.vendor_id, dev.device_id, dev.class_code, dev.subclass);
        log_classification(dev.vendor_id, dev.device_id, &result);

        match result.tier {
            DriverTier::Tier1Native => tier1_count += 1,
            DriverTier::Tier2Jailed => tier2_count += 1,
        }
    }

    crate::serial_println!("[TIER] ───────────────────────────────────────────────");
    crate::serial_println!("[TIER]  TIER 1 (Native Lock-Free): {} cihaz", tier1_count);
    crate::serial_println!("[TIER]  TIER 2 (Jail Sandbox):     {} cihaz", tier2_count);
    crate::serial_println!("[TIER] ═══════════════════════════════════════════════");
}
