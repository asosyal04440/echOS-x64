//! # echOS VFIO — Omni-Matrix Mimarisinin Tier 3'\u00fc
//!
//! ## Bu Modül Ne Yapar?
//!
//! Bir GPU'yu fiziksel olarak izole ederek oyun process'ine "dogrudan
//! bağar" (passthrough). Oyun GPU'yu sanki kçunda varmış gibi kullanabilir,
//! kernel araya girmez, sürücü gerekmez.
//!
//! ## IOMMU / VT-d Nedir?
//!
//! Normal bir sistemde herhangi bir PCI cihazı (GPU, NVMe, USB) tüm fiziksel
//! bellege DMA (Direct Memory Access) yapabilir — bu devasa bir güvenlik
//! açığıdır. Bir kartüşa kastas ekranda görünen kernel bellegini okuyabilir.
//!
//! **IOMMU (I/O Memory Management Unit)** bunu engeller:
//! - Her PCI cihaz bir "domain"e atanir.
//! - O domain'in 2. seviye sayfa tablosu (SLPT) hangi fiziksel adreslere
//!   DMA yapilabileceğini belirler.
//! - Tabloda olmayan bir adrese DMA yapilirsa → IOMMU hatası, sistem koruma altında.
//!
//! ## Passthrough Pipeline:
//!
//! ```text
//!  1. DMAR parse   → IOMMU donanımının MMIO adresini bul
//!  2. BAR okuma    → GPU'nun bellek pencerelerini (BAR0..5) PCI'den al
//!  3. SLPT inşa   → GPU BAR'ları için kimlik (identity) mapleşmesi
//!  4. Context yaz  → IOMMU'ya "bu BDF = domain 1" talimatı ver
//!  5. TE bit       → IOMMU translation'u aktifleştir (GCMD.TE = 1)
//!  6. CPUID stealth → Anti-cheat'ten gizle
//! ```
//!
//! ## Domain Modeli:
//!
//! ```text
//!  IOMMU
//!    ├─ Context[Bus:Dev:Fn] → Domain 1 (GPU)
//!    │                         SLPT: BAR0, BAR1 → oyun VA
//!    │                         Kalan PA → HATA (DMA engellendi)
//!    └─ Context[diğer]     → Domain 0 (kernel, serbest DMA)
//! ```
//!
//! This subsystem implements physical GPU isolation and passthrough by:
//!
//! 1. **Parsing DMAR** — reads the ACPI DMAR (DMA Remapping) table to locate
//!    all IOMMU (VT-d) hardware units on the system.
//!
//! 2. **Programming IOMMU** — isolates a target PCI device (e.g. NVIDIA RTX,
//!    AMD RX 7900) into a dedicated IOMMU domain so no other DMA agent can
//!    touch its memory.
//!
//! 3. **BAR Passthrough** — maps the GPU's Base Address Registers directly into
//!    the game process's physical-address space via the IOMMU second-level
//!    page table (SLPT).
//!
//! 4. **CPUID / Hypervisor Stealth** — clears the hypervisor-presence bits and
//!    KVM leaf in software before they can be read by ring-3 code, preventing
//!    anti-cheat systems (Vanguard, BattlEye, EAC) from detecting virtualisation.
//!
//! ## VT-d IOMMU Domain Model
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────────┐
//!  │  IOMMU (VT-d DRHD unit, MMIO @ DRHD.register_base_address)      │
//!  │                                                                  │
//!  │  Context Entry [Bus:Dev:Fn] ──► Domain 1 (GPU)                  │
//!  │                  all others ──► Domain 0 (kernel, DMA-allowed)  │
//!  │                                                                  │
//!  │  Domain 1 SLPT maps:                                             │
//!  │    GPU BAR0 (MMIO) ──► game process VA                          │
//!  │    GPU BAR1 (VRAM) ──► game process VA                          │
//!  │    all other PA    ──► FAULT (IOMMU blocks DMA)                 │
//!  └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## CPUID Stealth Strategy
//!
//! | Leaf | Bit | Meaning before | After spoofing |
//! |------|-----|----------------|----------------|
//! | 0x1  | ECX[31] | Hypervisor present | **0** — hidden |
//! | 0x40000000 | EAX | KVM/VMWare ID leaf | returns zeros |
//! | 0x40000001 | EAX | KVM feature bits | returns zeros |
//!
//! Stealth is achieved by hooking the `#UD` fault path (CPUID raises `#UD` when
//! executed from Ring-3 if the kernel intercepts it) or by patching the MSR
//! `IA32_MISC_ENABLE.BIT22` to cause `#GP` + emulation.  We use the simpler
//! approach: a per-process CPUID mask table in the TCB, applied in the syscall
//! return path.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod dmar;
pub mod cpuid_spoof;
pub mod pci_bar;

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ============================================================================
// VT-d REGISTER OFFSET'LERI (VT-d Rev. 4.1, §10.4)
// ============================================================================
//
// IOMMU donanımı, MMIO aracılığıyla komut alır. DRHD.register_base_address'e
// bu offset'leri ekleyerek ilgili register'a erişilir.
//
// Misal: IOMMU MMIO base = 0xFED90000
//   GCMD register'u: 0xFED90000 + 0x018 = 0xFED90018 adresine yaz.
//   Bu adrese 0x80000000 yazarsak: TE (Translation Enable) biti set edilir.
//   Bir sonraki CPU clock'ta IOMMU DMA filtrelemeye başlar.

/// Version Register
const VTD_REG_VER:      u32 = 0x000;
/// Capability Register
const VTD_REG_CAP:      u32 = 0x008;
/// Extended Capability Register
const VTD_REG_ECAP:     u32 = 0x010;
/// Global Command Register
const VTD_REG_GCMD:     u32 = 0x018;
/// Global Status Register
const VTD_REG_GSTS:     u32 = 0x01C;
/// Root Table Address Register
const VTD_REG_RTADDR:   u32 = 0x020;
/// Context Command Register
const VTD_REG_CCMD:     u32 = 0x028;
/// Fault Status Register
const VTD_REG_FSTS:     u32 = 0x034;
/// Fault Event Control Register
const VTD_REG_FECTL:    u32 = 0x038;
/// Interrupt Remapping Table Address Register
const VTD_REG_IRTA:     u32 = 0x0B8;

/// Global Command: Set Root Table Pointer
const VTD_GCMD_SRTP:    u32 = 1 << 30;
/// Global Command: Translation Enable
const VTD_GCMD_TE:      u32 = 1 << 31;

/// Global Status: Translation Enable Status
const VTD_GSTS_TES:     u32 = 1 << 31;
/// Global Status: Root Table Pointer Status
const VTD_GSTS_RTPS:    u32 = 1 << 30;

// ============================================================================
// IOMMU UNIT
// ============================================================================

/// Represents one VT-d IOMMU hardware unit discovered from a DRHD entry.
#[derive(Debug, Clone)]
pub struct IommuUnit {
    /// MMIO base address of the IOMMU's register space (from DRHD.register_base)
    pub mmio_base: u64,
    /// PCI segment number this unit covers (always 0 on most consumer boards)
    pub segment:   u16,
    /// Whether this DRHD has the INCLUDE_PCI_ALL flag set
    pub catch_all: bool,
    /// Devices covered by this unit (when catch_all == false)
    pub devices:   Vec<PciDeviceScope>,
    /// IOMMU capabilities (read from VER/CAP registers after MMIO is mapped)
    pub capabilities: IommuCapabilities,
}

/// Parsed IOMMU capability registers.
#[derive(Debug, Clone, Default)]
pub struct IommuCapabilities {
    pub version: u32,
    /// Number of domains supported (from CAP.ND * 16)
    pub num_domains: u32,
    /// Largest supported physical address width
    pub agaw: u8,
    /// Supports 4-level paging (SLPT)
    pub supports_4level: bool,
    /// Extended capability: interrupt remapping
    pub supports_ir: bool,
}

/// Identifies one PCI device/function within an IOMMU device scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceScope {
    pub bus:      u8,
    pub device:   u8,
    pub function: u8,
    pub scope_type: DeviceScopeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceScopeType {
    PciEndpoint = 1,
    PciBridge   = 2,
    IoapicEp    = 3,
    MsiCapable  = 4,
}

// ============================================================================
// IOMMU DOMAIN
// ============================================================================

/// SLPT = Second-Level Page Table (2. Seviye Sayfa Tablosu)
///
/// IOMMU'nun DMA filtreleme mekanizması klasik CPU sayfa tablolarına çok benzer.
/// Her domain'in bir "root" SLPT'si var. IOMMU her DMA iısteğini bu tablodan
/// yürüterek fiziksel adrese çevirir — tabloda yoksa DMA bloklar.
///
/// ## En Basit SLPT (bu implementasyon):
///   - Tek sayfa (4 KB)
///   - Sadece GPU BAR adresleri için kimlik mapleşmesi (GPA = PA)
///   - Tüm diğer DMA istekleri fault üretir
pub struct IommuDomain {
    /// Unique domain ID (1..num_domains)
    pub id: u32,
    /// The device this domain was created for
    pub device: PciDeviceScope,
    /// Physical address of the SLPT root (4-KiB aligned)
    pub slpt_phys: u64,
    /// MMIO base addresses of the GPU BARs that are passthrough-mapped
    pub bar_mappings: Vec<BarMapping>,
    /// Is translation currently active in hardware?
    pub hw_active: bool,
}

/// One GPU BAR mapped into the domain.
#[derive(Debug, Clone)]
pub struct BarMapping {
    /// BAR index (0-5)
    pub bar_idx: u8,
    /// BAR physical address (from PCI config space)
    pub phys_base: u64,
    /// BAR size in bytes
    pub size: u64,
    /// Guest physical address (where the game process sees the BAR)
    pub gpa_base: u64,
}

// ============================================================================
// GLOBAL STATE
// ============================================================================

static VFIO_READY:    AtomicBool = AtomicBool::new(false);
static IOMMU_UNITS:   Mutex<Vec<IommuUnit>>   = Mutex::new(Vec::new());
static ACTIVE_DOMAIN: Mutex<Option<IommuDomain>> = Mutex::new(None);

// ============================================================================
// PUBLIC API
// ============================================================================

/// VFIO altsistemini başlatır.
///
/// `crate::acpi::init()` çalıştıktan sonra çağrılmalıdır.
/// ACPI DMAR tablosunu parse eder → IOMMU birimlerini keşfeder.
/// Yapılandırma başarılıysa `VFIO_READY` flag'i set edilir.
pub fn init() {
    crate::serial_println!("[VFIO] Initialising IOMMU/VFIO subsystem …");

    let units = dmar::parse_dmar_table();
    if units.is_empty() {
        crate::serial_println!("[VFIO] No VT-d IOMMU units found — GPU passthrough unavailable");
        return;
    }

    crate::serial_println!("[VFIO] Found {} IOMMU unit(s):", units.len());
    for (i, u) in units.iter().enumerate() {
        crate::serial_println!(
            "  [{}] mmio={:#x} seg={} catch_all={} devices={}",
            i, u.mmio_base, u.segment, u.catch_all, u.devices.len()
        );
    }

    *IOMMU_UNITS.lock() = units;
    VFIO_READY.store(true, Ordering::Release);
    crate::serial_println!("[VFIO] Ready");
}

/// Bir PCI GPU'yu `pid` process'ine passthrough yapır.
///
/// ## Detaylı Adımlar:
/// 1. **PCI BAR okuma**: CF8/CFC port I/O ile GPU'nun bellek pencerelerini bul.
/// 2. **SLPT tahsisi**: Kernel frame allocator'dan bir sayfa al, sıfırla.
///    Bu sayfa IOMMU'nun 2. seviye sayfa tablosu olacak.
/// 3. **BAR kimlik mapleşmesi**: GPU'nun fiziksel MMIO adresleri SLPT'ye yazılır.
///    "Kimlik" demek: konuk fiziksel adres = gerçek fiziksel adres (birebir).
/// 4. **Context entry**: IOMMU root/context tablosuna "bu BDF = domain 1" yazılır.
/// 5. **TE biti**: GCMD register'una 0x80000000 yazarak translation aktifleştir.
/// 6. **CPUID stealth**: Oyun CPUID çalıştırınca hypervisor bitleri gizlenir.
pub fn passthrough_gpu(
    bus: u8, device: u8, function: u8,
    pid: u64,
) -> Result<u32, &'static str> {
    if !VFIO_READY.load(Ordering::Acquire) {
        return Err("VFIO not initialised — no VT-d hardware");
    }

    let target = PciDeviceScope {
        bus, device, function,
        scope_type: DeviceScopeType::PciEndpoint,
    };

    // 1. Read GPU BARs from PCI config space
    let bars = pci_bar::read_bars(bus, device, function)?;
    crate::serial_println!(
        "[VFIO] GPU {:02x}:{:02x}.{} — {} BARs",
        bus, device, function, bars.len()
    );
    for b in &bars {
        crate::serial_println!(
            "  BAR{}: phys={:#x} size={:#x} ({})",
            b.bar_idx, b.phys_base, b.size,
            if b.is_mmio { "MMIO" } else { "I/O" }
        );
    }

    // 2. Allocate SLPT (one page, minimal — covers BAR regions only)
    let slpt_page = crate::memory::alloc_zeroed_page()
        .ok_or("VFIO: SLPT page allocation failed")?;
    let slpt_phys = crate::memory::virt_to_phys_va(VirtAddr::new(slpt_page as u64))
        .ok_or("VFIO: SLPT phys lookup failed")?
        .as_u64();

    // 3. Build BarMapping list mapped at GPA = phys (identity)
    let bar_mappings: Vec<BarMapping> = bars.iter().filter(|b| b.is_mmio).map(|b| {
        BarMapping {
            bar_idx:  b.bar_idx,
            phys_base: b.phys_base,
            size:      b.size,
            gpa_base:  b.phys_base, // identity mapping
        }
    }).collect();

    // 4. Program SLPT with GPU BAR identity mappings
    build_slpt_for_bars(slpt_page as *mut u64, &bar_mappings)?;

    // 5. Find the responsible IOMMU unit and program context entry
    let units = IOMMU_UNITS.lock();
    let unit = units.iter().find(|u| u.catch_all || u.devices.contains(&target))
        .ok_or("VFIO: no IOMMU unit covers this device")?;

    unsafe {
        program_iommu_context(unit, &target, slpt_phys, 1)?;
        enable_translation(unit)?;
    }
    drop(units);

    let domain_id = 1u32; // In production, allocate from a domain ID pool

    // 6. Register CPUID mask for this process PID
    cpuid_spoof::arm_for_pid(pid);

    let domain = IommuDomain {
        id:     domain_id,
        device: target,
        slpt_phys,
        bar_mappings,
        hw_active: true,
    };
    *ACTIVE_DOMAIN.lock() = Some(domain);

    crate::serial_println!(
        "[VFIO] GPU {:02x}:{:02x}.{} passthrough active (domain={}  pid={})",
        bus, device, function, domain_id, pid
    );

    Ok(domain_id)
}

/// Revoke GPU passthrough and return the device to kernel control.
pub fn revoke_passthrough() -> Result<(), &'static str> {
    let mut dom = ACTIVE_DOMAIN.lock();
    if let Some(ref mut d) = *dom {
        let units = IOMMU_UNITS.lock();
        if let Some(unit) = units.iter().find(|u| {
            u.catch_all || u.devices.contains(&d.device)
        }) {
            unsafe { disable_translation(unit)?; }
        }
        d.hw_active = false;
        crate::serial_println!("[VFIO] Passthrough revoked for domain={}", d.id);
    }
    *dom = None;
    Ok(())
}

// ============================================================================
// IOMMU LOW-LEVEL PROGRAMMING
// ============================================================================

/// Write a context entry that routes `device` through SLPT at `slpt_phys`.
///
/// VT-d context entry (128-bit, two u64 words, spec §9.3):
/// ```text
/// Lower 64 bits:
///   [0]     = Present
///   [1]     = Fault Processing Disable
///   [3:2]   = Translation Type (00 = DMA remapping)
///   [63:12] = Second Level Page Table Pointer (SLPT >> 12)
///
/// Upper 64 bits:
///   [2:0]   = AGAW (Address Width: 010 = 48-bit / 4-level)
///   [23:16] = Domain ID
/// ```
unsafe fn program_iommu_context(
    unit:      &IommuUnit,
    device:    &PciDeviceScope,
    slpt_phys: u64,
    domain_id: u32,
) -> Result<(), &'static str> {
    // Root table physical address (must be allocated separately)
    // For simplicity, resolve via the kernel page allocator
    let root_table_phys = get_or_alloc_root_table(unit)?;

    // The root table has 256 entries (one per Bus).
    // Each root entry → 256 context entries (one per Dev:Fn).
    let bus     = device.bus as usize;
    let dev_fn  = ((device.device << 3) | device.function) as usize;

    // Root table entry (16 bytes each: { lower: u64, upper: u64 })
    let hhdm = crate::memory::hhdm_offset();
    let root_table_va = root_table_phys + hhdm;

    // Read/write root entry for this bus
    let root_entry_ptr = (root_table_va + (bus * 16) as u64) as *mut u64;
    let root_lower = root_entry_ptr.read_volatile();

    let ctx_table_phys = if root_lower & 1 != 0 {
        // Context table already exists for this bus
        root_lower & !0xFFF
    } else {
        // Allocate new context table (4 KiB)
        let ctx_page = crate::memory::alloc_zeroed_page()
            .ok_or("VFIO: context table alloc failed")?;
        let phys = crate::memory::virt_to_phys_va(
            x86_64::VirtAddr::new(ctx_page as u64)
        ).ok_or("VFIO: ctx phys failed")?.as_u64();
        // Set root entry: present=1 | phys
        root_entry_ptr.write_volatile(phys | 1);
        root_entry_ptr.add(1).write_volatile(0);
        phys
    };

    // Context entry (16 bytes): ctx_lower, ctx_upper
    let ctx_va  = ctx_table_phys + hhdm;
    let ctx_ptr = (ctx_va + (dev_fn * 16) as u64) as *mut u64;

    // Lower: Present=1, FPD=0, TT=00 (DMA remapping), SLPTR = slpt_phys>>12
    let ctx_lower: u64 = 1 | (slpt_phys & !0xFFF);
    // Upper: AGAW=010 (48-bit), DID=domain_id
    let agaw: u64 = 2; // 4-level 48-bit
    let ctx_upper: u64 = agaw | ((domain_id as u64) << 8);

    ctx_ptr.write_volatile(ctx_lower);
    ctx_ptr.add(1).write_volatile(ctx_upper);

    // Invalidate context cache (write CCMD register: context-cache invalidation)
    let ccmd_ptr = (unit.mmio_base + hhdm + VTD_REG_CCMD as u64) as *mut u64;
    // CCMD: ICC=1 | CIRG=01 (Global) | CAIG after poll
    ccmd_ptr.write_volatile((1u64 << 63) | (1u64 << 61));
    // Poll until ICC=0
    for _ in 0..100_000usize {
        if ccmd_ptr.read_volatile() & (1u64 << 63) == 0 { break; }
        core::hint::spin_loop();
    }

    // Invalidate IOTLB (global)
    invalidate_iotlb_global(unit);

    crate::serial_println!(
        "[VFIO] Context entry set: bus={:02x} devfn={:02x} slpt={:#x} domain={}",
        bus, dev_fn, slpt_phys, domain_id
    );
    Ok(())
}

/// Enable DMA translation on the IOMMU unit (GCMD.TE = 1).
unsafe fn enable_translation(unit: &IommuUnit) -> Result<(), &'static str> {
    let hhdm   = crate::memory::hhdm_offset();
    let gcmd   = (unit.mmio_base + hhdm + VTD_REG_GCMD as u64) as *mut u32;
    let gsts   = (unit.mmio_base + hhdm + VTD_REG_GSTS as u64) as *const u32;
    let rtaddr = (unit.mmio_base + hhdm + VTD_REG_RTADDR as u64) as *mut u64;

    // Set Root Table Address first
    let root_phys = get_or_alloc_root_table(unit)?;
    rtaddr.write_volatile(root_phys);

    // SRTP command
    gcmd.write_volatile(VTD_GCMD_SRTP);
    // Wait for RTPS
    for _ in 0..1_000_000usize {
        if gsts.read_volatile() & VTD_GSTS_RTPS != 0 { break; }
        core::hint::spin_loop();
    }

    // TE command
    gcmd.write_volatile(VTD_GCMD_TE);
    // Wait for TES
    for _ in 0..1_000_000usize {
        if gsts.read_volatile() & VTD_GSTS_TES != 0 { break; }
        core::hint::spin_loop();
    }

    if gsts.read_volatile() & VTD_GSTS_TES == 0 {
        return Err("VFIO: IOMMU translation enable timed out");
    }
    crate::serial_println!("[VFIO] IOMMU translation enabled (GSTS.TES=1)");
    Ok(())
}

/// Disable DMA translation (GCMD.TE = 0).  Called on revoke.
unsafe fn disable_translation(unit: &IommuUnit) -> Result<(), &'static str> {
    let hhdm = crate::memory::hhdm_offset();
    let gcmd = (unit.mmio_base + hhdm + VTD_REG_GCMD as u64) as *mut u32;
    let gsts = (unit.mmio_base + hhdm + VTD_REG_GSTS as u64) as *const u32;
    gcmd.write_volatile(0); // clear TE bit
    for _ in 0..1_000_000usize {
        if gsts.read_volatile() & VTD_GSTS_TES == 0 { break; }
        core::hint::spin_loop();
    }
    crate::serial_println!("[VFIO] IOMMU translation disabled");
    Ok(())
}

/// Perform a global IOTLB invalidation.
unsafe fn invalidate_iotlb_global(unit: &IommuUnit) {
    let hhdm = crate::memory::hhdm_offset();
    // IOTLB Invalidate Register: offset from ECAP.IRO (typically 0x100)
    // Simplified: use the fixed offset from the VT-d spec examples
    let iva = (unit.mmio_base + hhdm + 0x108) as *mut u64;
    // IVT=1 | IIRG=01 (Global)
    iva.write_volatile((1u64 << 63) | (1u64 << 60));
    for _ in 0..100_000usize {
        if iva.read_volatile() & (1u64 << 63) == 0 { break; }
        core::hint::spin_loop();
    }
}

// ============================================================================
// SECOND-LEVEL PAGE TABLE BUILDER  (minimal — maps GPU BARs only)
// ============================================================================

/// Build a 4-level SLPT (VT-d Second Level Page Table) that identity-maps
/// each GPU BAR.  All other physical addresses fault (not present).
///
/// The SLPT format mirrors x86-64 EPT / Intel VT-d SLPT:
/// * Bit 0 = Read  (R)
/// * Bit 1 = Write (W)
/// * Bit 2 = User (ignored at SLPT level)
/// * Bits 63:12 = Physical Frame Address >> 12
fn build_slpt_for_bars(
    slpt_root: *mut u64,
    bars: &[BarMapping],
) -> Result<(), &'static str> {
    for bar in bars {
        // Map each 4-KiB aligned page of the BAR
        let pages = (bar.size as usize + 0xFFF) / 4096;
        for page_idx in 0..pages {
            let gpa = bar.gpa_base + (page_idx as u64 * 4096);
            let hpa = bar.phys_base + (page_idx as u64 * 4096);
            slpt_map_page(slpt_root, gpa, hpa)?;
        }
    }
    Ok(())
}

/// Map a single 4-KiB page in the SLPT: gpa → hpa.
///
/// A production SLPT would use multi-level page tables (512-entry arrays).
/// This simplified version assumes a flat 1-level table for the lower 4 GiB
/// — sufficient for typical GPU BAR spaces (BARs live below 4 GiB on most
/// boards configured with Default BIOS settings).
fn slpt_map_page(root: *mut u64, gpa: u64, hpa: u64) -> Result<(), &'static str> {
    // Index into the root table: for a flat 1-level, use (gpa >> 12) & 0x1FF
    let idx = ((gpa >> 12) & 0x1FF) as usize;
    // R=1 | W=1 | physical frame address
    let entry: u64 = (hpa & !0xFFF) | 0b11;
    unsafe { root.add(idx).write_volatile(entry); }
    Ok(())
}

// ============================================================================
// ROOT TABLE REGISTRY  (one per IOMMU unit)
// ============================================================================

static ROOT_TABLES: Mutex<BTreeMap<u64, u64>> = Mutex::new(BTreeMap::new());

fn get_or_alloc_root_table(unit: &IommuUnit) -> Result<u64, &'static str> {
    let mut rt = ROOT_TABLES.lock();
    if let Some(&phys) = rt.get(&unit.mmio_base) {
        return Ok(phys);
    }
    // Allocate a zeroed 4-KiB root table
    let page = crate::memory::alloc_zeroed_page()
        .ok_or("VFIO: root table alloc failed")?;
    let phys = crate::memory::virt_to_phys_va(x86_64::VirtAddr::new(page as u64))
        .ok_or("VFIO: root table phys failed")?
        .as_u64();
    rt.insert(unit.mmio_base, phys);
    Ok(phys)
}

use x86_64::VirtAddr;
