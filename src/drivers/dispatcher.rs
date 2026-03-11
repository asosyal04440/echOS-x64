//! # Jail↔Core Dispatcher — TIER 1/TIER 2 Otomatik Yönlendirici
//!
//! PCI cihazlarını tarar, tier.rs ile sınıflandırır ve uygun sürücü
//! modeline yönlendirir:
//!
//! - TIER 1 (NVMe, NIC, GPU): Doğrudan async trait ile core'a bağlanır
//! - TIER 2 (WiFi, Audio, USB, BT): Jail worker thread'e yönlendirilir
//!
//! ```text
//! ┌──────────────┐    ┌─────────────────┐    ┌──────────────────┐
//! │ PCI Scan     │───►│ tier::classify() │───►│ TIER 1: native   │
//! │              │    │                  │    │ AsyncBlockDevice │
//! └──────────────┘    │                  │    ├──────────────────┤
//!                     │                  │───►│ TIER 2: jail     │
//!                     └─────────────────┘    │ JailWorker       │
//!                                            └──────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use ironshim_rs::{InterruptBudget, PciAddress};
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DriverFamily {
    Gpu,
    Nic,
    Nvme,
    Hid,
    Audio,
    Usb,
    Other,
}

impl core::fmt::Display for DriverFamily {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            DriverFamily::Gpu => "gpu",
            DriverFamily::Nic => "nic",
            DriverFamily::Nvme => "nvme",
            DriverFamily::Hid => "hid",
            DriverFamily::Audio => "audio",
            DriverFamily::Usb => "usb",
            DriverFamily::Other => "other",
        };
        write!(f, "{}", name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverOnboardingProfile {
    pub family: DriverFamily,
    pub preferred_tier: DriverTier,
    pub requires_els_source_compat: bool,
    pub requires_ironshim_manifest: bool,
    pub compile_profile: &'static str,
    pub parallel_lane: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverCompatibilityRow {
    pub family: DriverFamily,
    pub discovered: u32,
    pub active: u32,
    pub failed: u32,
    pub disabled: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverResourceManifest {
    pub mmio_regions: u8,
    pub port_regions: u8,
    pub irq_budget: u32,
    pub dma_budget_pages: u32,
    pub signed_policy: bool,
    pub isolated_slot: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverActivationError {
    Disabled,
    MissingCompileProfile,
    MissingManifest,
    InvalidManifest,
    UnsignedRejected,
    PciParseFailed,
    IronShimRegistrationFailed,
}

// ============================================================================
// SÜRÜCÜ KAYDI
// ============================================================================

/// Sürücü kaydı: hangi tier'da, hangi modülle
#[derive(Clone, Debug)]
pub struct DriverRegistration {
    /// Benzersiz sürücü ID
    pub driver_id: u32,
    /// PCI vendor:device
    pub vendor_id: u16,
    pub device_id: u16,
    /// PCI class:subclass
    pub class_code: u8,
    pub subclass: u8,
    /// Sürücü adı
    pub name: String,
    /// Tier seviyesi
    pub tier: DriverTier,
    /// Geniş eLS + IronShim onboarding politikası
    pub onboarding: DriverOnboardingProfile,
    /// Durum
    pub state: DriverState,
    /// Bağımlılıklar (diğer driver_id'ler)
    pub dependencies: Vec<u32>,
    /// Runtime config
    pub config: DriverConfig,
    /// IronShim enforced resource manifest summary.
    pub resource_manifest: Option<DriverResourceManifest>,
    /// Last activation failure reason.
    pub last_error: Option<DriverActivationError>,
}

// ============================================================================
// SÜRÜCÜ YAPILANDIRMASI (H18 — Runtime Config)
// ============================================================================

/// Sürücü çalışma zamanı yapılandırması.
///
/// Tier override, kaynak limitleri ve davranış bayrakları.
#[derive(Clone, Debug)]
pub struct DriverConfig {
    /// Tier'ı elle geçersiz kıl (None = otomatik sınıflandırma)
    pub tier_override: Option<DriverTier>,
    /// Sürücünün kullanabileceği maksimum bellek (sayfa sayısı, 0 = sınırsız)
    pub max_memory_pages: u32,
    /// Sürücünün kullanabileceği maksimum IRQ sayısı
    pub max_irqs: u8,
    /// Otomatik yeniden başlatma denemeleri
    pub restart_attempts: u8,
    /// Sürücüyü devre dışı bırak
    pub disabled: bool,
    /// Manifest imzası gerekli mi?
    pub require_signed: bool,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            tier_override: None,
            max_memory_pages: 0,
            max_irqs: 4,
            restart_attempts: 3,
            disabled: false,
            require_signed: false,
        }
    }
}

// ============================================================================
// SÜRÜCÜ BAĞIMLILIKLARI (H18 — Dependency Tracking)
// ============================================================================

/// Sürücü bağımlılık kaydı.
///
/// Topolojik sıralama ile boot sırası belirlenir.
#[derive(Clone, Debug)]
pub struct DriverDependency {
    /// Bağımlı sürücü (dependent)
    pub driver_id: u32,
    /// Bağımlı olduğu sürücü (dependency)
    pub depends_on: u32,
    /// İsteğe bağlı mı (true ise, bağımlılık yoksa da başlatılır)
    pub optional: bool,
}

/// Boot sırası hesaplamak için topolojik sıralama.
///
/// Kahn's algorithm: in-degree tabanlı BFS.
pub fn resolve_boot_order(drivers: &[DriverRegistration]) -> Vec<u32> {
    let deps = DEPENDENCY_GRAPH.lock();
    let mut in_degree: BTreeMap<u32, usize> = BTreeMap::new();
    let mut adj: BTreeMap<u32, Vec<u32>> = BTreeMap::new();

    // Tüm sürücüleri grafiğe ekle
    for drv in drivers {
        in_degree.entry(drv.driver_id).or_insert(0);
        adj.entry(drv.driver_id).or_insert_with(Vec::new);
    }

    // Bağımlılık kenarlarını ekle
    for dep in deps.iter() {
        adj.entry(dep.depends_on)
            .or_insert_with(Vec::new)
            .push(dep.driver_id);
        *in_degree.entry(dep.driver_id).or_insert(0) += 1;
    }

    // BFS — in-degree 0 olanlardan başla
    let mut queue: Vec<u32> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut order = Vec::new();

    while let Some(node) = queue.pop() {
        order.push(node);
        if let Some(neighbors) = adj.get(&node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(&next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(next);
                    }
                }
            }
        }
    }

    order
}

/// Bağımlılık ekler.
pub fn add_dependency(driver_id: u32, depends_on: u32, optional: bool) {
    let dep = DriverDependency {
        driver_id,
        depends_on,
        optional,
    };
    DEPENDENCY_GRAPH.lock().push(dep);

    // Registry'deki bağımlılık listesini de güncelle
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        if !reg.dependencies.contains(&depends_on) {
            reg.dependencies.push(depends_on);
        }
    }

    crate::serial_println!(
        "[Dispatcher] Dependency: driver {} → depends on {} (optional={})",
        driver_id,
        depends_on,
        optional
    );
}

/// Runtime tier override uygular.
pub fn override_tier(driver_id: u32, new_tier: DriverTier) -> bool {
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        let old = reg.tier;
        reg.tier = new_tier;
        reg.config.tier_override = Some(new_tier);
        crate::serial_println!(
            "[Dispatcher] Tier override: driver {} '{}' {:?} → {:?}",
            driver_id,
            reg.name,
            old,
            new_tier
        );
        true
    } else {
        false
    }
}

/// Sürücü yapılandırmasını günceller.
pub fn update_config(driver_id: u32, config: DriverConfig) -> bool {
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        reg.config = config;
        true
    } else {
        false
    }
}

/// Sürücü tier seviyesi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverTier {
    /// TIER 1: Lock-free native driver (NVMe, NIC, GPU)
    Tier1Native,
    /// TIER 2: Jail sandbox driver (WiFi, Audio, USB, BT)
    Tier2Jail,
    /// Bilinmeyen cihaz — henüz sınıflandırılmamış
    Unknown,
}

impl core::fmt::Display for DriverTier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DriverTier::Tier1Native => write!(f, "TIER1-Native"),
            DriverTier::Tier2Jail => write!(f, "TIER2-Jail"),
            DriverTier::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Sürücü durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverState {
    /// Keşfedildi, henüz başlatılmadı
    Discovered,
    /// Başlatılıyor
    Initializing,
    /// Aktif ve çalışıyor
    Active,
    /// Hata ile durdu
    Failed,
    /// Kullanıcı tarafından devre dışı bırakıldı
    Disabled,
}

fn driver_family_for_class(class_code: u8, subclass: u8) -> DriverFamily {
    match (class_code, subclass) {
        (0x03, _) => DriverFamily::Gpu,
        (0x02, 0x00) => DriverFamily::Nic,
        (0x01, 0x08) => DriverFamily::Nvme,
        (0x09, _) => DriverFamily::Hid,
        (0x04, _) => DriverFamily::Audio,
        (0x0C, 0x03) | (0x0C, 0x05) => DriverFamily::Usb,
        _ => DriverFamily::Other,
    }
}

pub fn onboarding_profile_for(class_code: u8, subclass: u8) -> DriverOnboardingProfile {
    let family = driver_family_for_class(class_code, subclass);
    let preferred_tier = classify_device(class_code, subclass);
    let (compile_profile, parallel_lane) = match family {
        DriverFamily::Gpu => ("els-gpu-native", "gpu"),
        DriverFamily::Nic => ("els-nic-native", "nic"),
        DriverFamily::Nvme => ("els-nvme-native", "nvme"),
        DriverFamily::Hid => ("els-hid-shim", "hid"),
        DriverFamily::Audio => ("els-audio-shim", "audio"),
        DriverFamily::Usb => ("els-usb-shim", "usb"),
        DriverFamily::Other => ("els-generic-shim", "generic"),
    };
    DriverOnboardingProfile {
        family,
        preferred_tier,
        requires_els_source_compat: true,
        requires_ironshim_manifest: true,
        compile_profile,
        parallel_lane,
    }
}

fn default_config_for_profile(profile: DriverOnboardingProfile) -> DriverConfig {
    let mut cfg = DriverConfig::default();
    cfg.require_signed = profile.requires_ironshim_manifest;
    cfg.max_irqs = if profile.preferred_tier == DriverTier::Tier1Native {
        16
    } else {
        4
    };
    cfg.max_memory_pages = if profile.preferred_tier == DriverTier::Tier1Native {
        0
    } else {
        8192
    };
    cfg
}

/// Sonraki driver ID
static NEXT_DRIVER_ID: AtomicU32 = AtomicU32::new(1);

lazy_static::lazy_static! {
    /// Tüm kayıtlı sürücülerin merkezi kaydı
    static ref DRIVER_REGISTRY: Mutex<BTreeMap<u32, DriverRegistration>> = Mutex::new(BTreeMap::new());

    /// Bağımlılık grafiği
    static ref DEPENDENCY_GRAPH: Mutex<Vec<DriverDependency>> = Mutex::new(Vec::new());
}

/// PCI cihazını tier.rs ile sınıflandırır ve uygun sürücü modeline yönlendirir.
pub fn dispatch_device(
    vendor_id: u16,
    device_id: u16,
    class_code: u8,
    subclass: u8,
    bus: u8,
    device: u8,
    function: u8,
) -> u32 {
    let driver_id = NEXT_DRIVER_ID.fetch_add(1, Ordering::Relaxed);

    // Tier + onboarding profili birlikte belirlenir.
    let classified_tier = classify_device(class_code, subclass);
    let onboarding = onboarding_profile_for(class_code, subclass);
    let tier = if classified_tier == DriverTier::Unknown {
        onboarding.preferred_tier
    } else {
        classified_tier
    };

    let name = match tier {
        DriverTier::Tier1Native => match (class_code, subclass) {
            (0x01, 0x08) => String::from("nvme-native"),
            (0x02, _) => String::from("nic-native"),
            (0x03, _) => String::from("gpu-native"),
            _ => alloc::format!("native-{:02x}{:02x}", class_code, subclass),
        },
        DriverTier::Tier2Jail => match (class_code, subclass) {
            (0x02, 0x80) => String::from("wifi-jail"),
            (0x04, _) => String::from("audio-jail"),
            (0x0C, 0x03) => String::from("usb-jail"),
            (0x0D, _) => String::from("bluetooth-jail"),
            _ => alloc::format!("jail-{:02x}{:02x}", class_code, subclass),
        },
        DriverTier::Unknown => {
            alloc::format!("unknown-{:02x}{:02x}", class_code, subclass)
        }
    };

    let reg = DriverRegistration {
        driver_id,
        vendor_id,
        device_id,
        class_code,
        subclass,
        name: name.clone(),
        tier,
        onboarding,
        state: DriverState::Discovered,
        dependencies: Vec::new(),
        config: default_config_for_profile(onboarding),
        resource_manifest: None,
        last_error: None,
    };

    DRIVER_REGISTRY.lock().insert(driver_id, reg);

    crate::serial_println!(
        "[Dispatcher] Device {:04x}:{:04x} (class={:02x}:{:02x}) → {:?} '{}' [id={}] lane={} ironshim={} els={}",
        vendor_id,
        device_id,
        class_code,
        subclass,
        tier,
        name,
        driver_id,
        onboarding.parallel_lane,
        onboarding.requires_ironshim_manifest,
        onboarding.requires_els_source_compat
    );

    // Tier'a göre başlatma
    match tier {
        DriverTier::Tier1Native => {
            init_tier1_driver(driver_id, class_code, subclass, bus, device, function);
        }
        DriverTier::Tier2Jail => {
            init_tier2_driver(driver_id, class_code, subclass, bus, device, function);
        }
        DriverTier::Unknown => {
            crate::serial_println!(
                "[Dispatcher] Skipping unknown device {:04x}:{:04x}",
                vendor_id,
                device_id
            );
        }
    }

    driver_id
}

/// Cihazı tier'a göre sınıflandırır
fn classify_device(class_code: u8, subclass: u8) -> DriverTier {
    match (class_code, subclass) {
        // TIER 1: Yüksek performanslı, lock-free native driver'lar
        (0x01, 0x08) => DriverTier::Tier1Native, // NVMe
        (0x02, 0x00) => DriverTier::Tier1Native, // Ethernet NIC
        (0x03, _) => DriverTier::Tier1Native,    // GPU/Display

        // TIER 2: İzole sandbox driver'lar
        (0x02, 0x80) => DriverTier::Tier2Jail, // WiFi
        (0x04, _) => DriverTier::Tier2Jail,    // Audio
        (0x0C, 0x03) => DriverTier::Tier2Jail, // USB (xHCI)
        (0x0C, 0x05) => DriverTier::Tier2Jail, // USB (type-C)
        (0x0D, _) => DriverTier::Tier2Jail,    // Wireless (BT)
        (0x08, _) => DriverTier::Tier2Jail,    // System peripherals
        (0x0C, 0x09) => DriverTier::Tier2Jail, // I2C

        // Bilinmeyen
        _ => DriverTier::Unknown,
    }
}

fn fail_driver(driver_id: u32, error: DriverActivationError) {
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        reg.state = DriverState::Failed;
        reg.last_error = Some(error);
    }
}

fn prepare_activation_manifest(
    driver_id: u32,
    bus: u8,
    device: u8,
    function: u8,
) -> Result<DriverResourceManifest, DriverActivationError> {
    let reg = DRIVER_REGISTRY
        .lock()
        .get(&driver_id)
        .cloned()
        .ok_or(DriverActivationError::MissingManifest)?;

    if reg.config.disabled {
        return Err(DriverActivationError::Disabled);
    }
    if reg.onboarding.compile_profile.is_empty() {
        return Err(DriverActivationError::MissingCompileProfile);
    }
    if reg.onboarding.requires_ironshim_manifest
        && !crate::security::anti_cheat::signed_driver_policy_enabled()
        && reg.config.require_signed
    {
        crate::security::anti_cheat::record_runtime_violation(
            crate::security::anti_cheat::RuntimeViolation::UnsignedDriverLoad,
            "driver activation rejected by signed-driver policy",
        );
        return Err(DriverActivationError::UnsignedRejected);
    }

    let desc = ironshim_rs::parse_pci_function(&crate::shim_layer::EchOsPciConfig, bus, device, function)
        .map_err(|_| DriverActivationError::PciParseFailed)?;
    let (mmio, mmio_count, ports, port_count) =
        crate::ironshim_bridge::extract_manifest_from_bars(&desc.bars, desc.bar_len);
    if mmio_count == 0 && port_count == 0 {
        crate::security::anti_cheat::record_runtime_violation(
            crate::security::anti_cheat::RuntimeViolation::UnsignedDriverLoad,
            "driver activation missing IronShim resource manifest",
        );
        return Err(DriverActivationError::MissingManifest);
    }
    if crate::ironshim_bridge::build_manifest(mmio, mmio_count, ports, port_count).is_err() {
        crate::security::anti_cheat::record_runtime_violation(
            crate::security::anti_cheat::RuntimeViolation::CallbackTamper,
            "driver manifest validation rejected malformed callback/resource contract",
        );
        return Err(DriverActivationError::InvalidManifest);
    }

    let isolated = crate::ironshim_bridge::IsolatedDriver {
        name: reg.name.clone(),
        mmio_regions: mmio,
        mmio_count,
        port_regions: ports,
        port_count,
        irq_budget: InterruptBudget {
            max_ticks: 10_000,
            max_calls: reg.config.max_irqs.max(1) as u32 * 1_024,
        },
        pci_addr: PciAddress {
            bus,
            device,
            function,
        },
        irq_vectors: [0; 8],
        irq_count: 0,
        active: true,
    };
    let slot = match crate::ironshim_bridge::register_isolated_driver(isolated) {
        Ok(slot) => slot,
        Err(_) => {
            crate::security::anti_cheat::record_runtime_violation(
                crate::security::anti_cheat::RuntimeViolation::CallbackTamper,
                "driver IronShim registration failed during callback isolation",
            );
            return Err(DriverActivationError::IronShimRegistrationFailed);
        }
    };

    Ok(DriverResourceManifest {
        mmio_regions: mmio_count as u8,
        port_regions: port_count as u8,
        irq_budget: reg.config.max_irqs.max(1) as u32,
        dma_budget_pages: reg.config.max_memory_pages,
        signed_policy: reg.config.require_signed,
        isolated_slot: Some(slot as u16),
    })
}

/// TIER 1 native sürücüsünü başlatır
fn init_tier1_driver(
    driver_id: u32,
    class_code: u8,
    subclass: u8,
    bus: u8,
    device: u8,
    function: u8,
) {
    update_state(driver_id, DriverState::Initializing);

    let manifest = match prepare_activation_manifest(driver_id, bus, device, function) {
        Ok(manifest) => manifest,
        Err(err) => {
            fail_driver(driver_id, err);
            return;
        }
    };
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        reg.resource_manifest = Some(manifest);
        reg.last_error = None;
    }

    match (class_code, subclass) {
        (0x01, 0x08) => {
            crate::serial_println!(
                "[Dispatcher] TIER 1 NVMe: bus={} dev={} func={} → AsyncBlockDevice",
                bus,
                device,
                function
            );
            // NVMe init zaten nvme::init() içinde yapılıyor
        }
        (0x02, 0x00) => {
            crate::serial_println!(
                "[Dispatcher] TIER 1 NIC: bus={} dev={} func={} → AsyncNetDevice",
                bus,
                device,
                function
            );
        }
        (0x03, _) => {
            crate::serial_println!(
                "[Dispatcher] TIER 1 GPU: bus={} dev={} func={} → AsyncGpuDevice",
                bus,
                device,
                function
            );
        }
        _ => {}
    }

    update_state(driver_id, DriverState::Active);
}

/// TIER 2 jail sürücüsünü başlatır
fn init_tier2_driver(
    driver_id: u32,
    class_code: u8,
    subclass: u8,
    bus: u8,
    device: u8,
    function: u8,
) {
    update_state(driver_id, DriverState::Initializing);

    let manifest = match prepare_activation_manifest(driver_id, bus, device, function) {
        Ok(manifest) => manifest,
        Err(err) => {
            fail_driver(driver_id, err);
            return;
        }
    };
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        reg.resource_manifest = Some(manifest);
        reg.last_error = None;
    }

    // Jail oluştur ve kaydet
    let jail_id = driver_id as u16;
    let name = match (class_code, subclass) {
        (0x02, 0x80) => "wifi-jail",
        (0x04, _) => "audio-jail",
        (0x0C, 0x03) => "usb-jail",
        (0x0D, _) => "bt-jail",
        _ => "generic-jail",
    };

    super::jail::register_jail(jail_id, name, class_code, subclass);

    crate::serial_println!(
        "[Dispatcher] TIER 2: Created jail '{}' (id={}) for class={:02x}:{:02x}",
        name,
        jail_id,
        class_code,
        subclass
    );

    update_state(driver_id, DriverState::Active);
}

/// Sürücü durumunu günceller
fn update_state(driver_id: u32, state: DriverState) {
    if let Some(reg) = DRIVER_REGISTRY.lock().get_mut(&driver_id) {
        reg.state = state;
    }
}

/// Boot sırasında tüm PCI cihazlarını tarar ve dispatcher'a kaydeder.
pub fn scan_and_dispatch() {
    crate::serial_println!("[Dispatcher] Scanning PCI bus for devices...");

    let devices = crate::drivers::pci::scan();
    let mut tier1_count = 0u32;
    let mut tier2_count = 0u32;
    let mut unknown_count = 0u32;

    for dev in &devices {
        let id = dispatch_device(
            dev.vendor_id,
            dev.device_id,
            dev.class_code,
            dev.subclass,
            dev.bus,
            dev.device,
            dev.function,
        );

        let tier = classify_device(dev.class_code, dev.subclass);
        match tier {
            DriverTier::Tier1Native => tier1_count += 1,
            DriverTier::Tier2Jail => tier2_count += 1,
            DriverTier::Unknown => unknown_count += 1,
        }
    }

    crate::serial_println!(
        "[Dispatcher] Scan complete: {} devices ({} TIER1, {} TIER2, {} unknown)",
        devices.len(),
        tier1_count,
        tier2_count,
        unknown_count
    );
}

/// Kayıtlı tüm sürücüleri listeler
pub fn list_drivers() -> Vec<DriverRegistration> {
    DRIVER_REGISTRY.lock().values().cloned().collect()
}

/// Belirli bir sürücüyü döner
pub fn get_driver(driver_id: u32) -> Option<DriverRegistration> {
    DRIVER_REGISTRY.lock().get(&driver_id).cloned()
}

/// Sınıf bazlı uyumluluk matrisi.
pub fn compatibility_matrix() -> Vec<DriverCompatibilityRow> {
    let registry = DRIVER_REGISTRY.lock();
    let mut rows: BTreeMap<DriverFamily, DriverCompatibilityRow> = BTreeMap::new();

    for reg in registry.values() {
        let row = rows.entry(reg.onboarding.family).or_insert(DriverCompatibilityRow {
            family: reg.onboarding.family,
            discovered: 0,
            active: 0,
            failed: 0,
            disabled: 0,
        });
        row.discovered = row.discovered.saturating_add(1);
        match reg.state {
            DriverState::Active => row.active = row.active.saturating_add(1),
            DriverState::Failed => row.failed = row.failed.saturating_add(1),
            DriverState::Disabled => row.disabled = row.disabled.saturating_add(1),
            DriverState::Discovered | DriverState::Initializing => {}
        }
    }

    rows.values().copied().collect()
}

pub fn compatibility_matrix_report() -> String {
    use alloc::format;
    let rows = compatibility_matrix();
    let mut out = String::from("=== Driver Compatibility Matrix ===\n\n");
    out.push_str("family     discovered active failed disabled\n");
    out.push_str("-------------------------------------------\n");
    for row in rows.iter() {
        out.push_str(&format!(
            "{:<10} {:<10} {:<6} {:<6} {}\n",
            row.family, row.discovered, row.active, row.failed, row.disabled
        ));
    }
    out
}

/// Dispatcher alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[Dispatcher] Two-tier driver dispatch system initialized");
    crate::serial_println!("[Dispatcher]   TIER 1 (native): NVMe, NIC, GPU → lock-free async");
    crate::serial_println!("[Dispatcher]   TIER 2 (jail):   WiFi, Audio, USB, BT → SPSC sandbox");
}

/// Tier dashboard — tüm sürücülerin tier, durum ve config bilgisini döner.
pub fn tier_dashboard() -> String {
    use alloc::format;
    let registry = DRIVER_REGISTRY.lock();
    let mut out = String::from("=== Driver Tier Dashboard ===\n\n");
    out.push_str(&format!("Total drivers: {}\n\n", registry.len()));
    out.push_str(&format!(
        "{:<6} {:<20} {:<14} {:<8} {:<12} {:<12} {}\n",
        "ID", "Name", "Tier", "Family", "State", "Override", "Deps"
    ));
    out.push_str(&format!("{}\n", "-".repeat(90)));

    for (_, reg) in registry.iter() {
        let override_str = match reg.config.tier_override {
            Some(t) => alloc::format!("{}", t),
            None => String::from("auto"),
        };
        let dep_str = if reg.dependencies.is_empty() {
            String::from("-")
        } else {
            let ids: Vec<String> = reg
                .dependencies
                .iter()
                .map(|d| alloc::format!("{}", d))
                .collect();
            ids.join(",")
        };
        out.push_str(&format!(
            "{:<6} {:<20} {:<14} {:<8} {:<12?} {:<12} {}\n",
            reg.driver_id,
            reg.name,
            alloc::format!("{}", reg.tier),
            reg.onboarding.family,
            reg.state,
            override_str,
            dep_str
        ));
    }
    out
}

/// Belirli bir sürücü için detaylı bilgi döner.
pub fn driver_detail(driver_id: u32) -> Option<String> {
    use alloc::format;
    let registry = DRIVER_REGISTRY.lock();
    let reg = registry.get(&driver_id)?;
    let mut out = format!("Driver #{}\n", reg.driver_id);
    out.push_str(&format!("  Name:      {}\n", reg.name));
    out.push_str(&format!(
        "  Vendor:    {:04x}:{:04x}\n",
        reg.vendor_id, reg.device_id
    ));
    out.push_str(&format!(
        "  Class:     {:02x}:{:02x}\n",
        reg.class_code, reg.subclass
    ));
    out.push_str(&format!("  Tier:      {}\n", reg.tier));
    out.push_str(&format!("  Family:    {}\n", reg.onboarding.family));
    out.push_str(&format!(
        "  Lane:      {}\n",
        reg.onboarding.parallel_lane
    ));
    out.push_str(&format!(
        "  eLS src:   {}\n",
        reg.onboarding.requires_els_source_compat
    ));
    out.push_str(&format!(
        "  IronShim:  {}\n",
        reg.onboarding.requires_ironshim_manifest
    ));
    out.push_str(&format!("  State:     {:?}\n", reg.state));
    out.push_str(&format!("  Override:  {:?}\n", reg.config.tier_override));
    out.push_str(&format!(
        "  Max mem:   {} pages\n",
        reg.config.max_memory_pages
    ));
    out.push_str(&format!("  Max IRQs:  {}\n", reg.config.max_irqs));
    out.push_str(&format!("  Restarts:  {}\n", reg.config.restart_attempts));
    out.push_str(&format!("  Disabled:  {}\n", reg.config.disabled));
    out.push_str(&format!("  Signed:    {}\n", reg.config.require_signed));
    out.push_str(&format!("  Last err:  {:?}\n", reg.last_error));
    if let Some(manifest) = reg.resource_manifest {
        out.push_str(&format!(
            "  Manifest:  mmio={} port={} irq_budget={} dma_pages={} signed={} slot={:?}\n",
            manifest.mmio_regions,
            manifest.port_regions,
            manifest.irq_budget,
            manifest.dma_budget_pages,
            manifest.signed_policy,
            manifest.isolated_slot
        ));
    }
    if !reg.dependencies.is_empty() {
        out.push_str(&format!("  Deps:      {:?}\n", reg.dependencies));
    }
    Some(out)
}
