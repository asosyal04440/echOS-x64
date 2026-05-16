//! # Linux Driver Onboarding — echOS Linux Sürücü Uyumluluk Katmanı
//!
//! Linux çekirdek sürücülerinin echOS'a onboard edilmesi için gerekli
//! profilleme, ABI çeviri ve yaşam döngüsü yönetimi.
//!
//! ## Linux Sürücü Modeli → echOS Çevirisi
//!
//! ```text
//! Linux                          echOS
//! ──────────────────────────    ──────────────────────────
//! struct pci_driver          →  LinuxDriverProfile
//!   .probe()                 →  DriverLifecycle::bind()
//!   .remove()                →  DriverLifecycle::unbind()
//!   .id_table                →  DeviceIdMatchTable
//!   .suspend()/.resume()     →  PowerState::Suspend/Resume
//! devm_kmalloc()             →  DevresArena::alloc()
//! devm_ioremap()             →  DevresArena::iomap()
//! devm_request_irq()         →  DevresArena::request_irq()
//! driver_bind/unbind (sysfs) →  DriverLifecycle::bind/unbind()
//! VFIO container/group       →  IommuGroup isolation
//! ```
//!
//! ## Desteklenen Sürücü Sınıfları (CONFIG_STANDALONE filtresi)
//!
//! | Sınıf | Durum | Firmware Gerekli mi? |
//! |---|---|---|
//! | NVMe (class 0x01/0x08) | ✅ Supported | Hayır |
//! | AHCI (class 0x01/0x06) | ✅ Supported | Hayır |
//! | Ethernet NIC (class 0x02/0x00) | ✅ Supported | Hayır (çoğu) |
//! | USB xHCI (class 0x0C/0x03) | ✅ Supported | Hayır |
//! | HID (class 0x03/0x00) | ✅ Supported | Hayır |
//! | Serial 8250 (class 0x07/0x00) | ✅ Supported | Hayır |
//! | GPU (class 0x03) | ⚠️ Partial | Evet (VBIOS/microcode) |
//! | WiFi (class 0x02/0x80) | ❌ Unsupported | Evet (vendor firmware) |
//! | Bluetooth (class 0x0D) | ❌ Unsupported | Evet (firmware) |
//! | DVB/Media (class 0x04/0x01) | ❌ Unsupported | Evet (demod firmware) |
//! | Staging drivers | ❌ Unsupported | Çeşitli |
//!
//! ## Kaynaklar
//! - Linux Driver Model docs (docs.kernel.org/driver-api/)
//! - Linux VFIO docs (Documentation/driver-api/vfio.rst)
//! - NetBSD rump kernels (https://www.netbsd.org/docs/rump/)
//! - CONFIG_STANDALONE, CONFIG_BROKEN kernel config analizi

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::dispatcher::{DriverFamily, DriverState, DriverTier};

// ============================================================================
// Linux Sürücü Destek Durumu
// ============================================================================

/// Linux sürücüsünün echOS'taki destek durumu.
///
/// Linux'ta "compile ediyor" ≠ "çalışıyor". Bu enum bu ayrımı açıkça belirtir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxDriverSupport {
    /// Tam destek: firmware gerektirmez, standart protokol, test edilmiş
    Supported,
    /// Kısmi destek: çalışıyor ama sınırlamalar var (örn. GPU: 2D sadece)
    Partial { limitation: &'static str },
    /// Derleniyor ama çalışmıyor: firmware bağımlılığı veya eksik altyapı
    CompileOnly { reason: &'static str },
    /// Açıkça desteklenmiyor: vendor-specific, kapalı kaynak, veya çok kompleks
    Unsupported { reason: &'static str },
}

impl LinuxDriverSupport {
    pub fn is_usable(&self) -> bool {
        match self {
            Self::Supported => true,
            Self::Partial { .. } => true,
            Self::CompileOnly { .. } => false,
            Self::Unsupported { .. } => false,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial { .. } => "partial",
            Self::CompileOnly { .. } => "compile-only",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

// ============================================================================
// Linux PCI Driver Profile (struct pci_driver → echOS)
// ============================================================================

/// Linux `struct pci_device_id` eşleşme girdisi.
///
/// Linux'ta: `vendor, device, subvendor, subdevice, class, class_mask, driver_data`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinuxDeviceId {
    pub vendor: u16,
    pub device: u16,
    pub subvendor: u16,
    pub subdevice: u16,
    pub class: u32,
    pub class_mask: u32,
}

impl LinuxDeviceId {
    /// PCI_ANY_ID wildcard
    pub const PCI_ANY_ID: u16 = 0xFFFF;
    pub const PCI_ANY_CLASS: u32 = 0xFFFFFFFF;

    pub const fn vendor_device(vendor: u16, device: u16) -> Self {
        Self {
            vendor,
            device,
            subvendor: Self::PCI_ANY_ID,
            subdevice: Self::PCI_ANY_ID,
            class: Self::PCI_ANY_CLASS,
            class_mask: 0,
        }
    }

    pub const fn class_match(class: u8, subclass: u8) -> Self {
        Self {
            vendor: Self::PCI_ANY_ID,
            device: Self::PCI_ANY_ID,
            subvendor: Self::PCI_ANY_ID,
            subdevice: Self::PCI_ANY_ID,
            class: ((class as u32) << 16) | ((subclass as u32) << 8),
            class_mask: 0x00FFFF00,
        }
    }

    pub const fn vendor_class_match(vendor: u16, class: u8, subclass: u8) -> Self {
        Self {
            vendor,
            device: Self::PCI_ANY_ID,
            subvendor: Self::PCI_ANY_ID,
            subdevice: Self::PCI_ANY_ID,
            class: ((class as u32) << 16) | ((subclass as u32) << 8),
            class_mask: 0x00FFFF00,
        }
    }

    /// Linux `pci_match_id()` benzeri eşleşme kontrolü
    pub fn matches(&self, vendor: u16, device: u16, class: u8, subclass: u8) -> bool {
        if self.vendor != Self::PCI_ANY_ID && self.vendor != vendor {
            return false;
        }
        if self.device != Self::PCI_ANY_ID && self.device != device {
            return false;
        }
        if self.class_mask != 0 {
            let dev_class = ((class as u32) << 16) | ((subclass as u32) << 8);
            if (dev_class & self.class_mask) != (self.class & self.class_mask) {
                return false;
            }
        }
        true
    }
}

/// Linux `struct pci_driver` → echOS profili.
///
/// Linux'taki pci_driver alanlarını echOS eşdeğerlerine çevirir.
#[derive(Clone, Debug)]
pub struct LinuxDriverProfile {
    /// Sürücü adı (Linux `pci_driver.name`)
    pub name: &'static str,
    /// Destek durumu (supported/partial/compile-only/unsupported)
    pub support: LinuxDriverSupport,
    /// PCI device ID tablosu (Linux `pci_driver.id_table`)
    pub id_table: &'static [LinuxDeviceId],
    /// echOS tier karşılığı
    pub echos_tier: DriverTier,
    /// echOS driver family karşılığı
    pub echos_family: DriverFamily,
    /// Firmware gerekli mi? (Linux `CONFIG_FW_LOADER` bağımlılığı)
    pub requires_firmware: bool,
    /// Firmware dosya adı (varsa)
    pub firmware_name: Option<&'static str>,
    /// ABI çeviri notu
    pub abi_notes: &'static str,
    /// "Compile ediyor" ama "çalışıyor" mu?
    pub actually_works: bool,
    /// Bilinen sorunlar
    pub known_issues: &'static str,
}

impl LinuxDriverProfile {
    /// Linux `pci_match_device()` benzeri: bu profil verilen cihazla eşleşiyor mu?
    pub fn matches_device(&self, vendor: u16, device: u16, class: u8, subclass: u8) -> bool {
        for id in self.id_table {
            if id.matches(vendor, device, class, subclass) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// Linux Sürücü Veritabanı (static)
// ============================================================================

/// NVMe sürücüleri — Linux `drivers/nvme/host/pci.c`
static NVME_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "nvme",
    support: LinuxDriverSupport::Supported,
    id_table: &[LinuxDeviceId::class_match(0x01, 0x08)],
    echos_tier: DriverTier::Tier1Native,
    echos_family: DriverFamily::Nvme,
    requires_firmware: false,
    firmware_name: None,
    abi_notes: "pci_driver.probe → nvme_probe, BAR0 MMIO + MSI-X",
    actually_works: true,
    known_issues: "Yok",
}];

/// AHCI sürücüleri — Linux `drivers/ata/ahci.c`
static AHCI_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "ahci",
    support: LinuxDriverSupport::Supported,
    id_table: &[LinuxDeviceId::class_match(0x01, 0x06)],
    echos_tier: DriverTier::Tier1Native,
    echos_family: DriverFamily::Nvme, // block storage
    requires_firmware: false,
    firmware_name: None,
    abi_notes: "pci_driver.probe → ahci_init_one, BAR5 (ABAR) MMIO",
    actually_works: true,
    known_issues: "Yok",
}];

/// Ethernet NIC sürücüleri — Linux `drivers/net/ethernet/`
static NIC_PROFILES: &[LinuxDriverProfile] = &[
    LinuxDriverProfile {
        name: "e1000e",
        support: LinuxDriverSupport::Supported,
        id_table: &[
            LinuxDeviceId::vendor_device(0x8086, 0x10D3), // 82574L
            LinuxDeviceId::vendor_device(0x8086, 0x10F6), // 82573L
            LinuxDeviceId::vendor_device(0x8086, 0x1502), // 82579LM
            LinuxDeviceId::vendor_device(0x8086, 0x1503), // 82579V
        ],
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Nic,
        requires_firmware: false,
        firmware_name: None,
        abi_notes: "pci_driver.probe → e1000_probe, BAR0 MMIO + MSI",
        actually_works: true,
        known_issues: "Yok",
    },
    LinuxDriverProfile {
        name: "igb",
        support: LinuxDriverSupport::Supported,
        id_table: &[
            LinuxDeviceId::vendor_device(0x8086, 0x150E), // i350
            LinuxDeviceId::vendor_device(0x8086, 0x1521), // i350 2-port
        ],
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Nic,
        requires_firmware: false,
        firmware_name: None,
        abi_notes: "pci_driver.probe → igb_probe, BAR0 MMIO + MSI-X",
        actually_works: true,
        known_issues: "Yok",
    },
    LinuxDriverProfile {
        name: "r8169",
        support: LinuxDriverSupport::Supported,
        id_table: &[LinuxDeviceId::vendor_device(0x10EC, 0x8168)],
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Nic,
        requires_firmware: false,
        firmware_name: None,
        abi_notes: "pci_driver.probe → rtl_init_one, BAR0 MMIO",
        actually_works: true,
        known_issues: "Bazı revizyonlarda firmware gerekebilir",
    },
];

/// USB xHCI sürücüleri — Linux `drivers/usb/host/xhci-pci.c`
static USB_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "xhci_hcd",
    support: LinuxDriverSupport::Supported,
    id_table: &[LinuxDeviceId::class_match(0x0C, 0x03)],
    echos_tier: DriverTier::Tier2Jail,
    echos_family: DriverFamily::Usb,
    requires_firmware: false,
    firmware_name: None,
    abi_notes: "pci_driver.probe → xhci_pci_probe, BAR0 MMIO, devm_* yoğun",
    actually_works: true,
    known_issues: "Jail isolation gerektirir",
}];

/// HID sürücüleri — Linux `drivers/hid/`
static HID_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "hid-generic",
    support: LinuxDriverSupport::Supported,
    id_table: &[LinuxDeviceId::class_match(0x03, 0x00)],
    echos_tier: DriverTier::Tier2Jail,
    echos_family: DriverFamily::Hid,
    requires_firmware: false,
    firmware_name: None,
    abi_notes: "hid_driver.probe → hid_probe, USB/HID report descriptor parse",
    actually_works: true,
    known_issues: "Yok",
}];

/// GPU sürücüleri — Linux `drivers/gpu/drm/`
static GPU_PROFILES: &[LinuxDriverProfile] = &[
    LinuxDriverProfile {
        name: "i915",
        support: LinuxDriverSupport::Partial {
            limitation: "2D sadece, 3D için GuC/HuC firmware gerekli",
        },
        id_table: &[LinuxDeviceId::vendor_class_match(0x8086, 0x03, 0x00)], // Intel display controller
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Gpu,
        requires_firmware: true,
        firmware_name: Some("i915/guc_*.bin"),
        abi_notes: "pci_driver.probe → i915_driver_load, DRM/KMS + GEM + fence",
        actually_works: false,
        known_issues: "Firmware yükleme altyapısı gerekli, GuC/HuC olmadan 3D yok",
    },
    LinuxDriverProfile {
        name: "amdgpu",
        support: LinuxDriverSupport::Partial {
            limitation: "2D sadece, 3D için PSP/SMC firmware gerekli",
        },
        id_table: &[LinuxDeviceId::vendor_class_match(0x1002, 0x03, 0x00)], // AMD display controller
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Gpu,
        requires_firmware: true,
        firmware_name: Some("amdgpu/*.bin"),
        abi_notes: "pci_driver.probe → amdgpu_driver_load_kms, DRM/KMS + amdkfd",
        actually_works: false,
        known_issues: "Firmware yükleme altyapısı gerekli, PSP/SMC olmadan 3D yok",
    },
    LinuxDriverProfile {
        name: "nouveau",
        support: LinuxDriverSupport::Partial {
            limitation: "2D + temel 3D, GSP firmware olmadan sınırlı",
        },
        id_table: &[LinuxDeviceId::vendor_class_match(0x10DE, 0x03, 0x00)], // NVIDIA display controller
        echos_tier: DriverTier::Tier1Native,
        echos_family: DriverFamily::Gpu,
        requires_firmware: true,
        firmware_name: Some("nouveau/*.bin"),
        abi_notes: "pci_driver.probe → nouveau_drm_load, DRM/KMS + TTM",
        actually_works: false,
        known_issues: "Firmware yükleme altyapısı gerekli, GSP olmadan sınırlı",
    },
];

/// WiFi sürücüleri — Linux `drivers/net/wireless/`
static WIFI_PROFILES: &[LinuxDriverProfile] = &[
    LinuxDriverProfile {
        name: "iwlwifi",
        support: LinuxDriverSupport::CompileOnly {
            reason: "Intel firmware (.ucode) gerekli, cfg80211/mac80211 altyapısı karmaşık",
        },
        id_table: &[
            LinuxDeviceId::vendor_class_match(0x8086, 0x02, 0x80), // Intel network/other controller
        ],
        echos_tier: DriverTier::Tier2Jail,
        echos_family: DriverFamily::Other,
        requires_firmware: true,
        firmware_name: Some("iwlwifi-*.ucode"),
        abi_notes: "pci_driver.probe → iwl_pci_probe, firmware loading + mac80211",
        actually_works: false,
        known_issues: "Firmware yükleme + mac80211 stack gerekli",
    },
    LinuxDriverProfile {
        name: "ath9k",
        support: LinuxDriverSupport::CompileOnly {
            reason: "Atheros firmware (.fw) gerekli, bazı cihazlar firmware'siz çalışır",
        },
        id_table: &[
            LinuxDeviceId::vendor_device(0x168C, 0x0000), // Atheros WiFi
        ],
        echos_tier: DriverTier::Tier2Jail,
        echos_family: DriverFamily::Other,
        requires_firmware: true,
        firmware_name: Some("ath9k/*.fw"),
        abi_notes: "pci_driver.probe → ath9k_init_device, ath_common + mac80211",
        actually_works: false,
        known_issues: "Firmware + mac80211 stack gerekli",
    },
];

/// Bluetooth sürücüleri — Linux `drivers/bluetooth/`
static BT_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "btusb",
    support: LinuxDriverSupport::CompileOnly {
        reason: "Çoğu chipset firmware (.hcd/.bin) gerekli",
    },
    id_table: &[
        LinuxDeviceId::vendor_device(0x8087, 0x0000), // Intel BT
        LinuxDeviceId::vendor_device(0x0A5C, 0x0000), // Broadcom BT
    ],
    echos_tier: DriverTier::Tier2Jail,
    echos_family: DriverFamily::Other,
    requires_firmware: true,
    firmware_name: Some("intel/*.hcd"),
    abi_notes: "usb_driver.probe → btusb_probe, USB HCI + firmware loading",
    actually_works: false,
    known_issues: "Firmware yükleme gerekli",
}];

/// Staging sürücüleri — Linux `drivers/staging/`
static STAGING_PROFILES: &[LinuxDriverProfile] = &[LinuxDriverProfile {
    name: "staging",
    support: LinuxDriverSupport::Unsupported {
        reason: "drivers/staging/ — tamamlanmamış, bakımsız veya bilinen sorunları var",
    },
    id_table: &[],
    echos_tier: DriverTier::Unknown,
    echos_family: DriverFamily::Other,
    requires_firmware: false,
    firmware_name: None,
    abi_notes: "N/A — staging sürücüleri desteklenmiyor",
    actually_works: false,
    known_issues: "Tamamlanmamış, bakımsız, veya CONFIG_BROKEN",
}];

/// Tüm Linux sürücü profillerini döner
pub fn all_linux_driver_profiles() -> &'static [&'static [LinuxDriverProfile]] {
    static ALL_PROFILES: &[&[LinuxDriverProfile]] = &[
        &NVME_PROFILES,
        &AHCI_PROFILES,
        &NIC_PROFILES,
        &USB_PROFILES,
        &GPU_PROFILES,
        &HID_PROFILES,
        &WIFI_PROFILES,
        &BT_PROFILES,
        &STAGING_PROFILES,
    ];
    ALL_PROFILES
}

/// Verilen PCI cihazı için en uygun Linux sürücü profilini bulur
pub fn find_linux_driver(
    vendor: u16,
    device: u16,
    class: u8,
    subclass: u8,
) -> Option<&'static LinuxDriverProfile> {
    for profiles in all_linux_driver_profiles() {
        for profile in *profiles {
            if profile.matches_device(vendor, device, class, subclass) {
                return Some(profile);
            }
        }
    }
    None
}

// ============================================================================
// ABI/API Çeviri Tablosu
// ============================================================================

/// Linux API → echOS eşdeğeri.
///
/// Bu tablo, Linux sürücülerinin echOS'a portlanması sırasında
/// hangi Linux API'lerinin hangi echOS API'lerine çevrileceğini belirtir.
#[derive(Clone, Copy, Debug)]
pub struct LinuxAbiTranslation {
    /// Linux API adı
    pub linux_api: &'static str,
    /// echOS eşdeğeri
    pub echos_equivalent: &'static str,
    /// Notlar
    pub notes: &'static str,
}

/// Linux → echOS ABI çeviri tablosu
pub const LINUX_ABI_TRANSLATIONS: &[LinuxAbiTranslation] = &[
    // struct pci_driver alanları
    LinuxAbiTranslation {
        linux_api: "struct pci_driver.name",
        echos_equivalent: "LinuxDriverProfile.name",
        notes: "Sürücü adı, loglama ve sysfs için kullanılır",
    },
    LinuxAbiTranslation {
        linux_api: "struct pci_driver.id_table",
        echos_equivalent: "LinuxDriverProfile.id_table",
        notes: "PCI device ID eşleşme tablosu",
    },
    LinuxAbiTranslation {
        linux_api: "pci_driver.probe(pdev, id)",
        echos_equivalent: "DriverLifecycle::bind(driver_id, pci_addr)",
        notes: "Cihaz bağlandığında çağrılır, BAR map, IRQ register",
    },
    LinuxAbiTranslation {
        linux_api: "pci_driver.remove(pdev)",
        echos_equivalent: "DriverLifecycle::unbind(driver_id)",
        notes: "Cihaz ayrıldığında çağrılır, tüm devres kaynakları serbest",
    },
    LinuxAbiTranslation {
        linux_api: "pci_driver.suspend(pdev, state)",
        echos_equivalent: "PowerState::Suspend",
        notes: "ACPI S3/S4 suspend, BAR durumu kaydedilir",
    },
    LinuxAbiTranslation {
        linux_api: "pci_driver.resume(pdev)",
        echos_equivalent: "PowerState::Resume",
        notes: "ACPI S3/S4 resume, BAR durumu geri yüklenir",
    },
    LinuxAbiTranslation {
        linux_api: "pci_driver.shutdown(pdev)",
        echos_equivalent: "DriverLifecycle::shutdown(driver_id)",
        notes: "Kapatma sırasında çağrılır",
    },
    // devm_* managed resource API'leri
    LinuxAbiTranslation {
        linux_api: "devm_kmalloc(dev, size, gfp)",
        echos_equivalent: "DevresArena::alloc(size)",
        notes: "Cihaz yaşam döngüsüne bağlı bellek tahsisi",
    },
    LinuxAbiTranslation {
        linux_api: "devm_kzalloc(dev, size, gfp)",
        echos_equivalent: "DevresArena::alloc_zeroed(size)",
        notes: "Sıfırlanmış bellek tahsisi",
    },
    LinuxAbiTranslation {
        linux_api: "devm_ioremap(dev, offset, size)",
        echos_equivalent: "DevresArena::iomap(phys_addr, size)",
        notes: "MMIO mapping, unbind'de otomatik unmmap",
    },
    LinuxAbiTranslation {
        linux_api: "devm_ioremap_resource(dev, res)",
        echos_equivalent: "DevresArena::iomap_resource(bar_idx)",
        notes: "BAR request + ioremap composite",
    },
    LinuxAbiTranslation {
        linux_api: "devm_request_irq(dev, irq, handler, flags, name, dev_id)",
        echos_equivalent: "DevresArena::request_irq(irq, handler)",
        notes: "IRQ registration, unbind'de otomatik free",
    },
    LinuxAbiTranslation {
        linux_api: "devm_kstrdup(dev, s, gfp)",
        echos_equivalent: "DevresArena::strdup(s)",
        notes: "Yönetilen string kopyalama",
    },
    LinuxAbiTranslation {
        linux_api: "dmam_alloc_coherent(dev, size, dma_handle, gfp)",
        echos_equivalent: "DevresArena::dma_alloc_coherent(size)",
        notes: "Coherent DMA allocation, unbind'de otomatik free",
    },
    LinuxAbiTranslation {
        linux_api: "pcim_enable_device(pdev)",
        echos_equivalent: "DevresArena::pci_enable()",
        notes: "PCI device enable, unbind'de otomatik disable",
    },
    LinuxAbiTranslation {
        linux_api: "pcim_iomap_regions(pdev, mask, name)",
        echos_equivalent: "DevresArena::iomap_regions(mask)",
        notes: "Çoklu BAR request + iomap",
    },
    // PCI core API'leri
    LinuxAbiTranslation {
        linux_api: "pci_read_config_word(pdev, offset, *val)",
        echos_equivalent: "PciConfig::read_word(bus, dev, func, offset)",
        notes: "PCI config space okuma",
    },
    LinuxAbiTranslation {
        linux_api: "pci_write_config_dword(pdev, offset, val)",
        echos_equivalent: "PciConfig::write_dword(bus, dev, func, offset, val)",
        notes: "PCI config space yazma",
    },
    LinuxAbiTranslation {
        linux_api: "pci_enable_device(pdev)",
        echos_equivalent: "PciDevice::enable()",
        notes: "PCI device enable (MEM + IO + BUS_MASTER)",
    },
    LinuxAbiTranslation {
        linux_api: "pci_set_master(pdev)",
        echos_equivalent: "PciDevice::set_bus_master(true)",
        notes: "Bus master enable",
    },
    LinuxAbiTranslation {
        linux_api: "pci_request_regions(pdev, name)",
        echos_equivalent: "PciDevice::request_regions(name)",
        notes: "BAR bölge rezervasyonu",
    },
    LinuxAbiTranslation {
        linux_api: "pci_iomap(pdev, bar, maxlen)",
        echos_equivalent: "PciDevice::map_bar(bar, maxlen)",
        notes: "BAR MMIO mapping",
    },
    // IRQ API'leri
    LinuxAbiTranslation {
        linux_api: "request_irq(irq, handler, flags, name, dev_id)",
        echos_equivalent: "IrqManager::register(irq, handler)",
        notes: "IRQ handler kaydı",
    },
    LinuxAbiTranslation {
        linux_api: "free_irq(irq, dev_id)",
        echos_equivalent: "IrqManager::unregister(irq, dev_id)",
        notes: "IRQ handler serbest bırakma",
    },
    LinuxAbiTranslation {
        linux_api: "enable_irq(irq)",
        echos_equivalent: "IrqManager::enable(irq)",
        notes: "IRQ enable",
    },
    LinuxAbiTranslation {
        linux_api: "disable_irq(irq)",
        echos_equivalent: "IrqManager::disable(irq)",
        notes: "IRQ disable",
    },
    // DMA API'leri
    LinuxAbiTranslation {
        linux_api: "dma_alloc_coherent(dev, size, dma_handle, gfp)",
        echos_equivalent: "DmaAllocator::alloc_coherent(size)",
        notes: "Coherent DMA buffer allocation",
    },
    LinuxAbiTranslation {
        linux_api: "dma_free_coherent(dev, size, vaddr, dma_handle)",
        echos_equivalent: "DmaAllocator::free_coherent(vaddr)",
        notes: "Coherent DMA buffer free",
    },
    LinuxAbiTranslation {
        linux_api: "dma_map_single(dev, ptr, size, dir)",
        echos_equivalent: "DmaAllocator::map_single(ptr, size, dir)",
        notes: "Single buffer DMA mapping",
    },
    LinuxAbiTranslation {
        linux_api: "dma_unmap_single(dev, addr, size, dir)",
        echos_equivalent: "DmaAllocator::unmap_single(addr, size, dir)",
        notes: "Single buffer DMA unmap",
    },
    // VFIO isolation
    LinuxAbiTranslation {
        linux_api: "vfio_container_get_group_fd(container, group_num)",
        echos_equivalent: "IommuGroup::open(group_num)",
        notes: "IOMMU group FD alma",
    },
    LinuxAbiTranslation {
        linux_api: "vfio_group_get_device_fd(group, device_name)",
        echos_equivalent: "IommuGroup::get_device_fd(device_name)",
        notes: "Device FD alma",
    },
    LinuxAbiTranslation {
        linux_api: "VFIO_IOMMU_MAP_DMA ioctl",
        echos_equivalent: "IommuManager::map_dma(iova, phys, size)",
        notes: "DMA mapping through IOMMU",
    },
];

// ============================================================================
// Devres Arena (Linux devm_* managed resource emulation)
// ============================================================================

/// Linux `devres` giriş tipi.
///
/// Her giriş bir kaynak türü ve serbest bırakma fonksiyonu içerir.
/// Unbind sırasında tüm girişler ters sırada serbest bırakılır.
#[derive(Clone, Debug)]
pub enum DevresEntry {
    /// Yönetilen bellek tahsisi (devm_kmalloc)
    Memory { ptr: usize, size: usize },
    /// Yönetilen MMIO mapping (devm_ioremap)
    Iomem {
        virt_addr: usize,
        phys_addr: u64,
        size: usize,
    },
    /// Yönetilen IRQ kaydı (devm_request_irq)
    Irq { irq: u32, handler_name: String },
    /// Yönetilen DMA buffer (dmam_alloc_coherent)
    DmaCoherent {
        virt_addr: usize,
        dma_addr: u64,
        size: usize,
    },
    /// Yönetilen PCI enable (pcim_enable_device)
    PciEnabled,
    /// Yönetilen BAR mapping (pcim_iomap_regions)
    BarMapped {
        bar_idx: u8,
        virt_addr: usize,
        size: usize,
    },
    /// Özel serbest bırakma fonksiyonu
    Custom { name: String, data: usize },
}

impl DevresEntry {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Memory { .. } => "memory",
            Self::Iomem { .. } => "iomem",
            Self::Irq { .. } => "irq",
            Self::DmaCoherent { .. } => "dma_coherent",
            Self::PciEnabled => "pci_enabled",
            Self::BarMapped { .. } => "bar_mapped",
            Self::Custom { .. } => "custom",
        }
    }
}

/// Linux `devres` arena'sı.
///
/// Cihaz yaşam döngüsüne bağlı kaynak yönetimi.
/// Unbind sırasında tüm kaynaklar ters sırada serbest bırakılır.
#[derive(Debug)]
pub struct DevresArena {
    pub entries: Vec<DevresEntry>,
    pub total_allocated: usize,
    memory_backing: Vec<Vec<u8>>,
}

impl DevresArena {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            total_allocated: 0,
            memory_backing: Vec::new(),
        }
    }

    /// devm_kmalloc benzeri: yönetilen bellek tahsisi
    pub fn alloc(&mut self, size: usize) -> usize {
        self.alloc_with_fill(size, 0xA5)
    }

    fn alloc_with_fill(&mut self, size: usize, fill: u8) -> usize {
        let mut backing = vec![fill; size.max(1)];
        let ptr = backing.as_mut_ptr() as usize;
        self.memory_backing.push(backing);
        self.entries.push(DevresEntry::Memory { ptr, size });
        self.total_allocated += size;
        ptr
    }

    /// devm_kzalloc benzeri: sıfırlanmış yönetilen bellek tahsisi
    pub fn alloc_zeroed(&mut self, size: usize) -> usize {
        self.alloc_with_fill(size, 0)
    }

    /// devm_ioremap benzeri: yönetilen MMIO mapping
    pub fn iomap(&mut self, phys_addr: u64, size: usize) -> usize {
        let virt_addr = 0xF000_0000usize + self.total_allocated;
        self.entries.push(DevresEntry::Iomem {
            virt_addr,
            phys_addr,
            size,
        });
        self.total_allocated += size;
        virt_addr
    }

    /// devm_request_irq benzeri: yönetilen IRQ kaydı
    pub fn request_irq(&mut self, irq: u32, handler_name: &str) {
        self.entries.push(DevresEntry::Irq {
            irq,
            handler_name: String::from(handler_name),
        });
    }

    /// dmam_alloc_coherent benzeri: yönetilen coherent DMA allocation
    pub fn dma_alloc_coherent(&mut self, size: usize) -> (usize, u64) {
        let virt_addr = 0xC000_0000usize + self.total_allocated;
        let dma_addr = 0x1000_0000u64 + self.total_allocated as u64;
        self.entries.push(DevresEntry::DmaCoherent {
            virt_addr,
            dma_addr,
            size,
        });
        self.total_allocated += size;
        (virt_addr, dma_addr)
    }

    /// pcim_enable_device benzeri: yönetilen PCI enable
    pub fn pci_enable(&mut self) {
        self.entries.push(DevresEntry::PciEnabled);
    }

    /// pcim_iomap_regions benzeri: yönetilen BAR mapping
    pub fn iomap_region(&mut self, bar_idx: u8, phys_addr: u64, size: usize) -> usize {
        let virt_addr = 0xF000_0000usize + self.total_allocated;
        self.entries.push(DevresEntry::BarMapped {
            bar_idx,
            virt_addr,
            size,
        });
        self.total_allocated += size;
        virt_addr
    }

    /// Unbind: tüm kaynakları ters sırada serbest bırak
    pub fn release_all(&mut self) -> Vec<String> {
        let mut log = Vec::new();
        for entry in self.entries.drain(..).rev() {
            match &entry {
                DevresEntry::Memory { ptr, size } => {
                    log.push(format!(
                        "  devm_kfree(memory): ptr={:#x} size={}",
                        ptr, size
                    ));
                }
                DevresEntry::Iomem {
                    virt_addr,
                    phys_addr,
                    size,
                } => {
                    log.push(format!(
                        "  devm_iounmap(iomem): virt={:#x} phys={:#x} size={}",
                        virt_addr, phys_addr, size
                    ));
                }
                DevresEntry::Irq { irq, handler_name } => {
                    log.push(format!(
                        "  devm_free_irq: irq={} handler={}",
                        irq, handler_name
                    ));
                }
                DevresEntry::DmaCoherent {
                    virt_addr,
                    dma_addr,
                    size,
                } => {
                    log.push(format!(
                        "  dmam_free_coherent: virt={:#x} dma={:#x} size={}",
                        virt_addr, dma_addr, size
                    ));
                }
                DevresEntry::PciEnabled => {
                    log.push(String::from("  pcim_disable_device"));
                }
                DevresEntry::BarMapped {
                    bar_idx,
                    virt_addr,
                    size,
                } => {
                    log.push(format!(
                        "  pcim_iounmap: bar={} virt={:#x} size={}",
                        bar_idx, virt_addr, size
                    ));
                }
                DevresEntry::Custom { name, .. } => {
                    log.push(format!("  devres_release: {}", name));
                }
            }
        }
        self.total_allocated = 0;
        self.memory_backing.clear();
        log
    }

    /// Kayıt sayısı
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Toplam tahsis edilen bellek
    pub fn total_bytes(&self) -> usize {
        self.total_allocated
    }
}

impl Clone for DevresArena {
    fn clone(&self) -> Self {
        let mut memory_iter = self.memory_backing.iter();
        let mut cloned = Self {
            entries: Vec::with_capacity(self.entries.len()),
            total_allocated: self.total_allocated,
            memory_backing: Vec::with_capacity(self.memory_backing.len()),
        };

        for entry in &self.entries {
            match entry {
                DevresEntry::Memory { size, .. } => {
                    let alloc_len = (*size).max(1);
                    let mut backing = memory_iter
                        .next()
                        .cloned()
                        .unwrap_or_else(|| vec![0; alloc_len]);
                    backing.resize(alloc_len, 0);
                    let ptr = backing.as_mut_ptr() as usize;
                    cloned.memory_backing.push(backing);
                    cloned
                        .entries
                        .push(DevresEntry::Memory { ptr, size: *size });
                }
                other => cloned.entries.push(other.clone()),
            }
        }

        cloned
    }
}

// ============================================================================
// Driver Lifecycle Manager (bind/unbind/quarantine)
// ============================================================================

/// Sürücü yaşam döngüsü durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    /// Keşfedildi, henüz bağlanmadı
    Discovered,
    /// Bağlanıyor (probe aşaması)
    Probing,
    /// Bağlı ve aktif
    Bound,
    /// Bağlantı kesiliyor
    Unbinding,
    /// Bağlantı kesildi
    Unbound,
    /// Karantinada (hata nedeniyle izole)
    Quarantined,
    /// Başarısız
    Failed,
}

/// Sürücü yaşam döngüsü olayı.
#[derive(Clone, Debug)]
pub enum LifecycleEvent {
    BindStarted,
    BindCompleted,
    BindFailed { reason: String },
    UnbindStarted,
    UnbindCompleted,
    Quarantined { reason: String, crash_count: u32 },
    Restarted { attempt: u32 },
}

/// Sürücü yaşam döngüsü yöneticisi.
///
/// Linux driver bind/unbind modelini taklit eder:
/// - `bind()` → probe → resources allocated → active
/// - `unbind()` → remove → devres released → inactive
/// - `quarantine()` → isolate → restart policy
#[derive(Clone, Debug)]
pub struct DriverLifecycle {
    pub driver_id: u32,
    pub state: LifecycleState,
    pub devres: DevresArena,
    pub bind_count: u32,
    pub unbind_count: u32,
    pub crash_count: u32,
    pub max_restart_attempts: u32,
    pub last_error: Option<String>,
    pub events: Vec<LifecycleEvent>,
}

impl DriverLifecycle {
    pub fn new(driver_id: u32) -> Self {
        Self {
            driver_id,
            state: LifecycleState::Discovered,
            devres: DevresArena::new(),
            bind_count: 0,
            unbind_count: 0,
            crash_count: 0,
            max_restart_attempts: 3,
            last_error: None,
            events: Vec::new(),
        }
    }

    /// Linux `driver_bind()` benzeri: sürücüyü cihaza bağla
    pub fn bind(&mut self) -> Result<(), String> {
        if self.state != LifecycleState::Discovered && self.state != LifecycleState::Unbound {
            return Err(format!("Cannot bind: state is {:?}", self.state));
        }

        self.state = LifecycleState::Probing;
        self.events.push(LifecycleEvent::BindStarted);

        // Probe aşaması: kaynak tahsisi (devm_* simülasyonu)
        // Gerçek implementasyonda burada pci_driver.probe() çağrılır

        self.state = LifecycleState::Bound;
        self.bind_count += 1;
        self.events.push(LifecycleEvent::BindCompleted);

        Ok(())
    }

    /// Linux `driver_unbind()` benzeri: sürücüyü cihazdan ayır
    pub fn unbind(&mut self) -> Result<Vec<String>, String> {
        if self.state != LifecycleState::Bound {
            return Err(format!("Cannot unbind: state is {:?}", self.state));
        }

        self.state = LifecycleState::Unbinding;
        self.events.push(LifecycleEvent::UnbindStarted);

        // Remove aşaması: tüm devres kaynaklarını serbest bırak
        let release_log = self.devres.release_all();

        self.state = LifecycleState::Unbound;
        self.unbind_count += 1;
        self.events.push(LifecycleEvent::UnbindCompleted);

        Ok(release_log)
    }

    /// Sürücüyü karantinaya al (hata nedeniyle izole)
    pub fn quarantine(&mut self, reason: &str) {
        self.state = LifecycleState::Quarantined;
        self.crash_count += 1;
        self.last_error = Some(reason.to_string());
        self.events.push(LifecycleEvent::Quarantined {
            reason: reason.to_string(),
            crash_count: self.crash_count,
        });
    }

    /// Karantinadan çıkar ve yeniden başlat
    pub fn restart(&mut self) -> Result<(), String> {
        if self.state != LifecycleState::Quarantined && self.state != LifecycleState::Failed {
            return Err(format!("Cannot restart: state is {:?}", self.state));
        }

        if self.crash_count >= self.max_restart_attempts {
            return Err(format!(
                "Max restart attempts exceeded: {} > {}",
                self.crash_count, self.max_restart_attempts
            ));
        }

        self.state = LifecycleState::Discovered;
        self.devres = DevresArena::new();
        self.events.push(LifecycleEvent::Restarted {
            attempt: self.crash_count,
        });

        self.bind()
    }

    /// Cihaz çökmesini kaydet
    pub fn record_crash(&mut self, reason: &str) {
        self.crash_count += 1;
        self.last_error = Some(reason.to_string());
        if self.crash_count >= self.max_restart_attempts {
            self.state = LifecycleState::Failed;
        } else {
            self.state = LifecycleState::Quarantined;
        }
        self.events.push(LifecycleEvent::Quarantined {
            reason: reason.to_string(),
            crash_count: self.crash_count,
        });
    }

    /// Durum raporu
    pub fn status_report(&self) -> String {
        format!(
            "Driver #{}: state={:?} binds={} unbinds={} crashes={} devres_entries={} devres_bytes={}",
            self.driver_id,
            self.state,
            self.bind_count,
            self.unbind_count,
            self.crash_count,
            self.devres.entry_count(),
            self.devres.total_bytes()
        )
    }
}

// ============================================================================
// Capability Matrix (compile vs working distinction)
// ============================================================================

/// Linux sürücü yetenek matrisi satırı.
#[derive(Clone, Debug)]
pub struct LinuxCapabilityRow {
    pub driver_name: &'static str,
    pub class: u8,
    pub subclass: u8,
    pub support: LinuxDriverSupport,
    pub requires_firmware: bool,
    pub actually_works: bool,
    pub tier: DriverTier,
    pub abi_notes: &'static str,
    pub known_issues: &'static str,
}

/// Linux sürücü yetenek matrisini oluştur
pub fn linux_capability_matrix() -> Vec<LinuxCapabilityRow> {
    let mut rows = Vec::new();

    for profiles in all_linux_driver_profiles() {
        for profile in *profiles {
            // id_table'daki her giriş için bir satır
            for id in profile.id_table {
                let (class, subclass) = if id.class_mask != 0 {
                    ((id.class >> 16) as u8, ((id.class >> 8) & 0xFF) as u8)
                } else {
                    (0, 0)
                };

                rows.push(LinuxCapabilityRow {
                    driver_name: profile.name,
                    class,
                    subclass,
                    support: profile.support,
                    requires_firmware: profile.requires_firmware,
                    actually_works: profile.actually_works,
                    tier: profile.echos_tier,
                    abi_notes: profile.abi_notes,
                    known_issues: profile.known_issues,
                });
            }
        }
    }

    rows
}

/// Yetenek matrisini okunabilir formatta raporla
pub fn linux_capability_report() -> String {
    let rows = linux_capability_matrix();
    let mut out = String::from("=== Linux Driver Capability Matrix ===\n\n");
    out.push_str(&format!(
        "{:<16} {:<8} {:<14} {:<10} {:<8} {:<10}\n",
        "Driver", "Class", "Support", "Firmware", "Works", "Tier"
    ));
    out.push_str(&"-".repeat(70));
    out.push('\n');

    for row in &rows {
        let tier_str = format!("{:?}", row.tier);
        out.push_str(&format!(
            "{:<16} {:02x}/{:<02x} {:<14} {:<10} {:<8} {:<10}\n",
            row.driver_name,
            row.class,
            row.subclass,
            row.support.label(),
            if row.requires_firmware { "yes" } else { "no" },
            if row.actually_works { "yes" } else { "no" },
            tier_str,
        ));
    }

    out.push('\n');
    out.push_str(&format!("Total drivers: {}\n", rows.len()));

    let supported = rows.iter().filter(|r| r.actually_works).count();
    let compile_only = rows
        .iter()
        .filter(|r| matches!(r.support, LinuxDriverSupport::CompileOnly { .. }))
        .count();
    let unsupported = rows
        .iter()
        .filter(|r| matches!(r.support, LinuxDriverSupport::Unsupported { .. }))
        .count();
    let needs_fw = rows.iter().filter(|r| r.requires_firmware).count();

    out.push_str(&format!(
        "Supported: {}, Compile-only: {}, Unsupported: {}, Needs firmware: {}\n",
        supported, compile_only, unsupported, needs_fw
    ));

    out
}

// ============================================================================
// Global Lifecycle Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref LIFECYCLE_REGISTRY: Mutex<BTreeMap<u32, DriverLifecycle>> =
        Mutex::new(BTreeMap::new());
}

/// Sürücü yaşam döngüsü kaydını al veya oluştur
pub fn get_or_create_lifecycle(driver_id: u32) -> DriverLifecycle {
    let mut registry = LIFECYCLE_REGISTRY.lock();
    registry
        .entry(driver_id)
        .or_insert_with(|| DriverLifecycle::new(driver_id))
        .clone()
}

/// Sürücü yaşam döngüsünü güncelle
pub fn update_lifecycle(driver_id: u32, lifecycle: DriverLifecycle) {
    LIFECYCLE_REGISTRY.lock().insert(driver_id, lifecycle);
}

/// Tüm yaşam döngüsü kayıtlarını raporla
pub fn lifecycle_report() -> String {
    let registry = LIFECYCLE_REGISTRY.lock();
    let mut out = String::from("=== Driver Lifecycle Report ===\n\n");

    for (id, lc) in registry.iter() {
        out.push_str(&format!("Driver #{}: {:?}\n", id, lc.state));
        out.push_str(&format!(
            "  binds={} unbinds={} crashes={}\n",
            lc.bind_count, lc.unbind_count, lc.crash_count
        ));
        out.push_str(&format!(
            "  devres: {} entries, {} bytes\n",
            lc.devres.entry_count(),
            lc.devres.total_bytes()
        ));
        if let Some(ref err) = lc.last_error {
            out.push_str(&format!("  last error: {}\n", err));
        }
        out.push('\n');
    }

    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_device_id_vendor_device_match() {
        let id = LinuxDeviceId::vendor_device(0x8086, 0x10D3);
        assert!(id.matches(0x8086, 0x10D3, 0x02, 0x00));
        assert!(!id.matches(0x8086, 0x10F6, 0x02, 0x00));
        assert!(!id.matches(0x10EC, 0x8168, 0x02, 0x00));
    }

    #[test]
    fn linux_device_id_class_match() {
        let id = LinuxDeviceId::class_match(0x01, 0x08);
        assert!(id.matches(0x8086, 0x0000, 0x01, 0x08));
        assert!(id.matches(0x144D, 0xA808, 0x01, 0x08));
        assert!(!id.matches(0x8086, 0x10D3, 0x02, 0x00));
    }

    #[test]
    fn linux_device_id_wildcard() {
        let id = LinuxDeviceId::vendor_device(0x8086, LinuxDeviceId::PCI_ANY_ID);
        assert!(id.matches(0x8086, 0x10D3, 0x02, 0x00));
        assert!(id.matches(0x8086, 0x1502, 0x0C, 0x03));
        assert!(!id.matches(0x10EC, 0x8168, 0x02, 0x00));
    }

    #[test]
    fn find_linux_driver_nvme() {
        let profile = find_linux_driver(0x8086, 0x0000, 0x01, 0x08);
        assert!(profile.is_some());
        let p = profile.unwrap();
        assert_eq!(p.name, "nvme");
        assert!(matches!(p.support, LinuxDriverSupport::Supported));
        assert!(!p.requires_firmware);
        assert!(p.actually_works);
    }

    #[test]
    fn find_linux_driver_e1000e() {
        let profile = find_linux_driver(0x8086, 0x10D3, 0x02, 0x00);
        assert!(profile.is_some());
        let p = profile.unwrap();
        assert_eq!(p.name, "e1000e");
        assert!(matches!(p.support, LinuxDriverSupport::Supported));
        assert!(!p.requires_firmware);
        assert!(p.actually_works);
    }

    #[test]
    fn find_linux_driver_i915_partial() {
        let profile = find_linux_driver(0x8086, 0x1234, 0x03, 0x00);
        assert!(profile.is_some());
        let p = profile.unwrap();
        assert_eq!(p.name, "i915");
        assert!(matches!(p.support, LinuxDriverSupport::Partial { .. }));
        assert!(p.requires_firmware);
        assert!(!p.actually_works);
    }

    #[test]
    fn find_linux_driver_iwlwifi_compile_only() {
        let profile = find_linux_driver(0x8086, 0x4234, 0x02, 0x80);
        assert!(profile.is_some());
        let p = profile.unwrap();
        assert_eq!(p.name, "iwlwifi");
        assert!(matches!(p.support, LinuxDriverSupport::CompileOnly { .. }));
        assert!(p.requires_firmware);
        assert!(!p.actually_works);
    }

    #[test]
    fn linux_driver_support_labels() {
        assert_eq!(LinuxDriverSupport::Supported.label(), "supported");
        assert_eq!(
            LinuxDriverSupport::CompileOnly { reason: "test" }.label(),
            "compile-only"
        );
        assert_eq!(
            LinuxDriverSupport::Unsupported { reason: "test" }.label(),
            "unsupported"
        );
        assert_eq!(
            LinuxDriverSupport::Partial { limitation: "test" }.label(),
            "partial"
        );
    }

    #[test]
    fn linux_driver_support_is_usable() {
        assert!(LinuxDriverSupport::Supported.is_usable());
        assert!(LinuxDriverSupport::Partial { limitation: "test" }.is_usable());
        assert!(!LinuxDriverSupport::CompileOnly { reason: "test" }.is_usable());
        assert!(!LinuxDriverSupport::Unsupported { reason: "test" }.is_usable());
    }

    #[test]
    fn devres_alloc_and_release() {
        let mut arena = DevresArena::new();

        let ptr = arena.alloc(128);
        assert_ne!(ptr, 0);
        assert_eq!(arena.entry_count(), 1);

        let virt = arena.iomap(0xFEC0_0000, 4096);
        assert_ne!(virt, 0);
        assert_eq!(arena.entry_count(), 2);

        arena.request_irq(42, "test_handler");
        assert_eq!(arena.entry_count(), 3);

        let log = arena.release_all();
        assert_eq!(log.len(), 3);
        assert_eq!(arena.entry_count(), 0);
        assert_eq!(arena.total_bytes(), 0);

        // Check reverse order
        assert!(log[0].contains("irq"));
        assert!(log[1].contains("iomem"));
        assert!(log[2].contains("memory"));
    }

    #[test]
    fn devres_dma_coherent() {
        let mut arena = DevresArena::new();
        let (virt, dma) = arena.dma_alloc_coherent(4096);
        assert_ne!(virt, 0);
        assert_ne!(dma, 0);
        assert_eq!(arena.entry_count(), 1);

        let log = arena.release_all();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("dmam_free_coherent"));
    }

    #[test]
    fn driver_lifecycle_bind_unbind() {
        let mut lc = DriverLifecycle::new(1);
        assert_eq!(lc.state, LifecycleState::Discovered);

        // Bind
        lc.bind().unwrap();
        assert_eq!(lc.state, LifecycleState::Bound);
        assert_eq!(lc.bind_count, 1);

        // Allocate some devres during bind
        lc.devres.alloc(256);
        lc.devres.request_irq(33, "nic_irq");

        // Unbind
        let log = lc.unbind().unwrap();
        assert_eq!(lc.state, LifecycleState::Unbound);
        assert_eq!(lc.unbind_count, 1);
        assert_eq!(log.len(), 2); // 2 devres entries released
    }

    #[test]
    fn driver_lifecycle_quarantine_restart() {
        let mut lc = DriverLifecycle::new(1);
        lc.bind().unwrap();

        // Crash
        lc.record_crash("DMA timeout");
        assert_eq!(lc.state, LifecycleState::Quarantined);
        assert_eq!(lc.crash_count, 1);

        // Restart
        lc.restart().unwrap();
        assert_eq!(lc.state, LifecycleState::Bound);
        assert_eq!(lc.bind_count, 2);
    }

    #[test]
    fn driver_lifecycle_max_restart_exceeded() {
        let mut lc = DriverLifecycle::new(1);
        lc.max_restart_attempts = 2;

        // Crash twice
        lc.record_crash("error1");
        lc.record_crash("error2");
        assert_eq!(lc.state, LifecycleState::Failed);

        // Restart should fail
        assert!(lc.restart().is_err());
    }

    #[test]
    fn driver_lifecycle_cannot_bind_when_bound() {
        let mut lc = DriverLifecycle::new(1);
        lc.bind().unwrap();
        assert!(lc.bind().is_err());
    }

    #[test]
    fn driver_lifecycle_cannot_unbind_when_not_bound() {
        let mut lc = DriverLifecycle::new(1);
        assert!(lc.unbind().is_err());
    }

    #[test]
    fn linux_capability_matrix_has_entries() {
        let rows = linux_capability_matrix();
        assert!(!rows.is_empty());

        // Should have at least NVMe, NIC, USB entries
        let supported_count = rows.iter().filter(|r| r.actually_works).count();
        assert!(supported_count >= 3);
    }

    #[test]
    fn linux_capability_report_generates() {
        let report = linux_capability_report();
        assert!(report.contains("Linux Driver Capability Matrix"));
        assert!(report.contains("Total drivers:"));
        assert!(report.contains("Supported:"));
    }

    #[test]
    fn abi_translation_table_complete() {
        // Should have translations for key Linux APIs
        let has_pci_driver = LINUX_ABI_TRANSLATIONS
            .iter()
            .any(|t| t.linux_api.contains("pci_driver"));
        let has_devm = LINUX_ABI_TRANSLATIONS
            .iter()
            .any(|t| t.linux_api.starts_with("devm_"));
        let has_dma = LINUX_ABI_TRANSLATIONS
            .iter()
            .any(|t| t.linux_api.starts_with("dma_"));

        assert!(has_pci_driver);
        assert!(has_devm);
        assert!(has_dma);
    }
}
