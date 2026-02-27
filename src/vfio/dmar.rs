//! # ACPI DMAR Tablo Ayrıştırıcı (DMA Remapping)
//!
//! ## ACPI ve DMAR Nedir?
//!
//! **ACPI (Advanced Configuration and Power Interface)**: BIOS/UEFI'nin
//! işletim sistemine donanım yapısını anlatan bir standart. Her ACPI tablosu
//! bellekte 4-byte imzası ve CRC checksum'u ile tanınan SDT yapısında saklanır.
//!
//! **DMAR (DMA Remapping)**: ACPI tablosu ailesi içinde özel bir tablodur.
//! Anakartlaki IOMMU (Input-Output Memory Management Unit) donanımının
//! adreslerini ve kapsadığı PCI cihazlarını listeler.
//!
//! ## Neden Bunu Okumamız Gerekiyor?
//!
//! GPU passthrough için IOMMU donanımına komut göndermemiz gerekir.
//! Komut göndermek için IOMMU'nun MMIO register adresini bilmeliyiz.
//! O adres tam olarak DMAR tablosunda yazıyor.
//!
//! ## Fiziksel Hafızadan Bulma Zinciri:
//!
//! ```text
//!  UEFI firmware
//!       │
//!       │ set_rsdp_address() — boot sırasında kaydedilir
//!       ▼
//!  RSDP (Root System Description Pointer) @ fiziksel adres
//!       │
//!       │ revision >= 2 → XSDT adresi (64-bit)
//!       │ revision  < 2 → RSDT adresi (32-bit)   
//!       ▼
//!  XSDT/RSDT: Tablolar listesi (her biri 4-byte imzayla)
//!       │
//!       │ "DMAR" imzasını ara
//!       ▼
//!  DMAR: host_addr_width + Remapping Structure Dizisi
//!       │
//!       │ DRHD yapılarını parse et
//!       ▼
//!  IommuUnit: mmio_base, segment, devices[]
//! ```
//!
//! ## VT-d Spesifikasyonu
//!
//! Bu kod Intel'in "Virtualization Technology for Directed I/O"
//! (VT-d) Rev. 4.1 spesifikasyonunun §8. bölümünü uygular.
//!
//! ## Table Layout
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │  ACPI SDT Header (36 bytes)                  │
//! │   Signature: "DMAR"                          │
//! ├──────────────────────────────────────────────┤
//! │  host_address_width: u8                      │
//! │  flags:              u8                      │
//! │  reserved:           [10]u8                  │
//! ├──────────────────────────────────────────────┤
//! │  Remapping Structure Array...                │
//! │    DRHD  (type=0)  — Hardware Definition     │
//! │    RMRR  (type=1)  — Reserved Memory Region  │
//! │    ATSR  (type=2)  — Root Port ATS Cap       │
//! │    RHSA  (type=3)  — HW Status Affinity      │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust
//! let units = dmar::parse_dmar_table();
//! // units: Vec<IommuUnit>
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::vec;

use super::{IommuUnit, IommuCapabilities, PciDeviceScope, DeviceScopeType};

// ============================================================================
// DMAR TABLO YAPI OFFSET'LERI
// ============================================================================
//
// ACPI tabloları bellekte sıkıştırılmış (packed) C struct'lar olarak saklanır.
// Rust normal olarak struct alanları arasına padding ekler (hızlı erişim için).
// `repr(C, packed)` ile padding kaldırılır, birebir byte eşlemesi elde edilir.
//
// SDT Header: 36 byte (imza + uzunluk + revizyon + checksum + OEM bilgisi)
// DMAR'a özgü ek: 12 byte (host_addr_width + flags + reserved)
// Toplam: 48 byte sonrası Remapping Structure dizisi başlar.

/// Size of ACPI SDT header
const SDT_HDR_SIZE: usize = 36;
/// DMAR-specific header: 1 (host_addr_width) + 1 (flags) + 10 (reserved) = 12 bytes
const DMAR_HDR_EXTRA: usize = 12;
/// Total DMAR pre-structure offset
const DMAR_DATA_OFFSET: usize = SDT_HDR_SIZE + DMAR_HDR_EXTRA;

/// Remapping structure type IDs (VT-d §8.1)
const RS_TYPE_DRHD: u16 = 0; // DMA Remapping Hardware Unit Definition
const RS_TYPE_RMRR: u16 = 1; // Reserved Memory Region Reporting
const RS_TYPE_ATSR: u16 = 2; // Root Port ATS Capability Reporting
const RS_TYPE_RHSA: u16 = 3; // Remapping Hardware Status Affinity
const RS_TYPE_ANDD: u16 = 4; // ACPI Namespace Device Declaration

/// DRHD flags (§8.3.1)
const DRHD_FLAG_INCLUDE_PCI_ALL: u8 = 0x01;

/// Device scope type IDs (§8.3.1 / §8.5)
const DS_TYPE_ENDPOINT:   u8 = 0x01;
const DS_TYPE_BRIDGE:     u8 = 0x02;
const DS_TYPE_IOAPIC:     u8 = 0x03;
const DS_TYPE_MSI_IOAPIC: u8 = 0x04;

// ============================================================================
// RAW PACKED STRUCTURES  (repr(C, packed) for safe pointer casting)
// ============================================================================

/// ACPI System Descriptor Table header (36 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct AcpiSdtHeader {
    signature:        [u8; 4],  // 0x00
    length:           u32,       // 0x04
    revision:         u8,        // 0x08
    checksum:         u8,        // 0x09
    oem_id:           [u8; 6],  // 0x0A
    oem_table_id:     [u8; 8],  // 0x10
    oem_revision:     u32,       // 0x18
    creator_id:       u32,       // 0x1C
    creator_revision: u32,       // 0x20
}

/// DMAR table (immediately after AcpiSdtHeader)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DmarHeader {
    host_address_width: u8,
    flags:              u8,
    reserved:           [u8; 10],
}

/// Remapping structure common header (4 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RemappingStructHeader {
    struct_type: u16,
    length:      u16,
}

/// DRHD (DMA Remapping Hardware Unit Definition) — §8.3.1
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DrhiHeader {
    struct_type:         u16, // = 0
    length:              u16,
    flags:               u8,
    reserved:            u8,
    segment_number:      u16,
    register_base_addr:  u64,
    // Variable-length device scope array follows
}

/// Device Scope Structure — §8.3.1
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DeviceScopeStructure {
    scope_type:       u8,
    length:           u8,
    reserved:         [u8; 2],
    enumeration_id:   u8,
    start_bus_number: u8,
    // Variable-length Path array follows (pairs of device, function)
}

// ============================================================================
// PARSER
// ============================================================================

/// DMAR tablosunu bul ve ayrıştır. Bulunan IOMMU birimlerini döndürür.
///
/// ## Çalışma Adımları:
/// 1. `crate::acpi::get_rsdp_address()` ile RSDP fiziksel adresini al.
/// 2. HHDM offset'i ekleyerek sanal adrese çevir (HHDM = Higher Half Direct Map).
///    Yani: sanal_adres = fiziksel_adres + hhdm_offset()
///    Bu sayede kernel, fiziksel belleğe doğrudan yazüp okuyabilir.
/// 3. RSDP → XSDT/RSDT → DMAR tablosunu bul.
/// 4. DMAR içindeki DRHD yapılarını ayrıştır.
pub fn parse_dmar_table() -> Vec<IommuUnit> {
    // Retrieve DMAR physical address from the ACPI subsystem
    let dmar_phys = match find_dmar_phys() {
        Some(p) => p,
        None    => {
            crate::serial_println!("[DMAR] DMAR table not found in ACPI (no VT-d hardware?)");
            return Vec::new();
        }
    };

    crate::serial_println!("[DMAR] Found DMAR at phys={:#x}", dmar_phys);

    // HHDM (Higher-Half Direct Map) nedir?
    // Kernel, tüm fiziksel RAM'ı sanal adres uzayının üst yarısına eşler.
    // Örneğin fiziksel 0x2000 → sanal 0xFFFF800000002000
    // Bu sayede kernel herhangi bir fiziksel adresi okumak için
    // sadece HHDM offset eklemesi yeterli — ayrıca map gerekmez.
    let hhdm    = crate::memory::hhdm_offset();
    let va_base = (dmar_phys + hhdm) as *const u8;

    // UNSAFE: DMAR tablosunu ham bellekten ayrıştır.
    // Bu blok güvenli değildir çünkü firmware tarafından doldurulmuş
    // ham fiziksel belleği `repr(C, packed)` pointer'larla okuyoruz.
    unsafe {
        // SDT header'dan toplam tablo uzunluğunu oku
        let hdr       = va_base as *const AcpiSdtHeader;
        let total_len = (*hdr).length as usize;
        if total_len <= DMAR_DATA_OFFSET {
            crate::serial_println!("[DMAR] Tablo çok küçük: {} bayt", total_len);
            return Vec::new();
        }

        let mut units  = Vec::new();
        // Remapping yapıları DMAR_DATA_OFFSET = 48. bayttan başlar
        let mut offset = DMAR_DATA_OFFSET;

        while offset + 4 <= total_len {
            // Remapping structure ortak başlığı: type(u16) + length(u16)
            let rs_hdr    = va_base.add(offset) as *const RemappingStructHeader;
            let rs_type   = (*rs_hdr).struct_type;
            let rs_length = (*rs_hdr).length as usize;
            if rs_length < 4 || offset + rs_length > total_len { break; }

            if rs_type == RS_TYPE_DRHD {
                // DRHD: DMA Remapping Hardware Unit Definition (§8.3.1)
                // Bu yapı bir IOMMU biriminin MMIO adresini ve kapsadığı
                // PCI cihazlarını bildirir.
                let drhd      = va_base.add(offset) as *const DrhiHeader;
                let mmio_base = (*drhd).register_base_addr;
                let segment   = (*drhd).segment_number;
                let catch_all = ((*drhd).flags & DRHD_FLAG_INCLUDE_PCI_ALL) != 0;

                // Device Scope yapıları DRHD header'ından hemen sonra gelir
                let ds_start = offset + core::mem::size_of::<DrhiHeader>();
                let ds_end   = offset + rs_length;
                let mut ds_off = ds_start;
                let mut devices = Vec::new();

                while ds_off + 6 <= ds_end {
                    let ds     = va_base.add(ds_off) as *const DeviceScopeStructure;
                    let ds_len  = (*ds).length as usize;
                    let ds_type = (*ds).scope_type;

                    if ds_len >= 8 {
                        // Path[0] = (device, function) — offset 6'dan başlar
                        let bus      = (*ds).start_bus_number;
                        let dev_fn   = va_base.add(ds_off + 6);
                        let device   = *dev_fn;
                        let function = *dev_fn.add(1);

                        let scope_type = match ds_type {
                            DS_TYPE_ENDPOINT   => DeviceScopeType::PciEndpoint,
                            DS_TYPE_BRIDGE     => DeviceScopeType::PciBridge,
                            DS_TYPE_IOAPIC     => DeviceScopeType::IoapicEp,
                            DS_TYPE_MSI_IOAPIC => DeviceScopeType::MsiCapable,
                            _                  => DeviceScopeType::PciEndpoint,
                        };
                        devices.push(PciDeviceScope { bus, device, function, scope_type });
                    }
                    if ds_len < 6 { break; }
                    ds_off += ds_len;
                }

                let unit = IommuUnit {
                    mmio_base,
                    segment,
                    catch_all,
                    devices,
                    capabilities: IommuCapabilities::default(),
                };
                crate::serial_println!(
                    "[DMAR] DRHD mmio={:#x} seg={} catch_all={} cihaz={}",
                    mmio_base, segment, catch_all, unit.devices.len()
                );
                units.push(unit);
            }

            offset += rs_length;
        }

        crate::serial_println!("[DMAR] {} IOMMU birimi ayrıştırıldı", units.len());
        units
    }
}

/// Locate the DMAR physical address by searching the XSDT (preferred) or RSDT.
fn find_dmar_phys() -> Option<u64> {
    // Try to get the RSDP from the ACPI module's stored address
    let rsdp_phys = crate::acpi::get_rsdp_address();
    if rsdp_phys == 0 { return None; }

    // HHDM offset ile fiziksel adres → sanal adres dönüşümü — her ACPI erişiminde yapılır.
    // RSDP revision >= 2 ise XSDT (64-bit pointer'lar) kullanılır, eski sistemlerde RSDT.
    let hhdm = crate::memory::hhdm_offset();

    unsafe {
        // RSDP'yi sanal adrese çevir ve ham byte pointer olarak oku
        let rsdp_va = (rsdp_phys + hhdm) as *const u8;
        // RSDP yapısı (ACPI §5.2.5):
        //   0x00: Signature[8]  = "RSD PTR "
        //   0x08: Checksum
        //   0x09: OEMID[6]
        //   0x0F: Revision      (0 = ACPI 1.0, ≥2 = ACPI 2.0+)
        //   0x10: RsdtAddress (u32)
        //   0x14: Length (u32, v2+)
        //   0x18: XsdtAddress (u64, v2+)
        let revision = *rsdp_va.add(15);
        if revision >= 2 {
            let xsdt_phys = u64::from_le_bytes(
                core::slice::from_raw_parts(rsdp_va.add(24), 8).try_into().unwrap()
            );
            if let Some(phys) = search_xsdt(xsdt_phys + hhdm, b"DMAR") {
                return Some(phys);
            }
        }

        // Fallback: RSDT (4-byte entries)
        let rsdt_phys = u32::from_le_bytes(
            core::slice::from_raw_parts(rsdp_va.add(16), 4).try_into().unwrap()
        ) as u64;
        search_rsdt(rsdt_phys + hhdm, b"DMAR")
    }
}

/// Walk an XSDT looking for the table with `sig`.
unsafe fn search_xsdt(xsdt_va: u64, sig: &[u8; 4]) -> Option<u64> {
    let hdr  = xsdt_va as *const AcpiSdtHeader;
    let len  = (*hdr).length as usize;
    if len < SDT_HDR_SIZE { return None; }

    let entries = (len - SDT_HDR_SIZE) / 8;
    let entry_ptr = (xsdt_va as usize + SDT_HDR_SIZE) as *const u64;

    for i in 0..entries {
        let table_phys = entry_ptr.add(i).read_unaligned();
        let hhdm = crate::memory::hhdm_offset();
        let table_va   = table_phys + hhdm;
        let table_hdr  = table_va as *const AcpiSdtHeader;
        if &(*table_hdr).signature == sig {
            return Some(table_phys);
        }
    }
    None
}

/// Walk an RSDT (4-byte pointers) looking for the table with `sig`.
unsafe fn search_rsdt(rsdt_va: u64, sig: &[u8; 4]) -> Option<u64> {
    let hdr  = rsdt_va as *const AcpiSdtHeader;
    let len  = (*hdr).length as usize;
    if len < SDT_HDR_SIZE { return None; }

    let entries = (len - SDT_HDR_SIZE) / 4;
    let entry_ptr = (rsdt_va as usize + SDT_HDR_SIZE) as *const u32;

    for i in 0..entries {
        let table_phys = entry_ptr.add(i).read_unaligned() as u64;
        let hhdm = crate::memory::hhdm_offset();
        let table_va   = table_phys + hhdm;
        let table_hdr  = table_va as *const AcpiSdtHeader;
        if &(*table_hdr).signature == sig {
            return Some(table_phys);
        }
    }
    None
}

/// Parse the DMAR table from a virtual address (HHDM mapped).
///
/// Returns a `Vec<IommuUnit>` — one entry per DRHD structure found.
unsafe fn parse_dmar_at_va(va_base: *const u8) -> Vec<IommuUnit> {
    let hdr     = va_base as *const AcpiSdtHeader;
    let total   = (*hdr).length as usize;

    crate::serial_println!(
        "[DMAR] Signature: {:?}  length={} revision={}",
        &(*hdr).signature, total, (*hdr).revision
    );

    // Validate checksum
    if !acpi_checksum_valid(va_base, total) {
        crate::serial_println!("[DMAR] WARNING: DMAR checksum invalid — proceeding anyway");
    }

    // DMAR-specific header follows the SDT header
    let dmar_hdr = va_base.add(SDT_HDR_SIZE) as *const DmarHeader;
    let host_aw  = (*dmar_hdr).host_address_width;
    crate::serial_println!("[DMAR] host_address_width={}", host_aw);

    let mut cursor  = DMAR_DATA_OFFSET;
    let mut units: Vec<IommuUnit> = Vec::new();

    while cursor + 4 <= total {
        let rs_hdr = va_base.add(cursor) as *const RemappingStructHeader;
        let rs_type   = (*rs_hdr).struct_type;
        let rs_length = (*rs_hdr).length as usize;

        if rs_length < 4 || cursor + rs_length > total { break; }

        match rs_type {
            RS_TYPE_DRHD => {
                if let Some(unit) = parse_drhd(va_base.add(cursor), rs_length) {
                    crate::serial_println!(
                        "[DMAR] DRHD: mmio={:#x} seg={} flags={:#x} devices={}",
                        unit.mmio_base, unit.segment, 
                        if unit.catch_all { DRHD_FLAG_INCLUDE_PCI_ALL } else { 0 },
                        unit.devices.len()
                    );
                    units.push(unit);
                }
            }
            RS_TYPE_RMRR => {
                crate::serial_println!("[DMAR] RMRR found (reserved memory region, skipping)");
            }
            RS_TYPE_ATSR => {
                crate::serial_println!("[DMAR] ATSR found (ATS capability, skipping)");
            }
            _ => {
                crate::serial_println!("[DMAR] Unknown remapping structure type={}", rs_type);
            }
        }

        cursor += rs_length;
    }

    // Probe IOMMU capability registers for each discovered unit
    for unit in &mut units {
        read_iommu_capabilities(unit);
    }

    units
}

/// Parse one DRHD structure.
unsafe fn parse_drhd(ptr: *const u8, total_length: usize) -> Option<IommuUnit> {
    if total_length < core::mem::size_of::<DrhiHeader>() { return None; }

    let drhd        = ptr as *const DrhiHeader;
    let flags       = (*drhd).flags;
    let segment     = (*drhd).segment_number;
    let reg_base    = (*drhd).register_base_addr;
    let catch_all   = (flags & DRHD_FLAG_INCLUDE_PCI_ALL) != 0;

    // Parse device scope array
    let scope_offset = core::mem::size_of::<DrhiHeader>();
    let mut cursor   = scope_offset;
    let mut devices  = Vec::new();

    while cursor + core::mem::size_of::<DeviceScopeStructure>() <= total_length {
        let ds     = ptr.add(cursor) as *const DeviceScopeStructure;
        let ds_len = (*ds).length as usize;
        let ds_type = (*ds).scope_type;

        if ds_len < core::mem::size_of::<DeviceScopeStructure>() { break; }

        // Path follows: start_bus + pairs of (device, function)
        // For simplicity, read the first path entry (the device itself, not bridges)
        let path_offset = core::mem::size_of::<DeviceScopeStructure>();
        let start_bus   = (*ds).start_bus_number;

        let (dev, fun) = if cursor + path_offset + 2 <= total_length {
            let dev_byte = *ptr.add(cursor + path_offset);
            let fun_byte = *ptr.add(cursor + path_offset + 1);
            (dev_byte >> 3, dev_byte & 0x7) // PCI device = bits[7:3], function = bits[2:0]
        } else {
            (0, 0)
        };

        let scope_type = match ds_type {
            DS_TYPE_ENDPOINT   => DeviceScopeType::PciEndpoint,
            DS_TYPE_BRIDGE     => DeviceScopeType::PciBridge,
            DS_TYPE_IOAPIC     => DeviceScopeType::IoapicEp,
            DS_TYPE_MSI_IOAPIC => DeviceScopeType::MsiCapable,
            _                   => DeviceScopeType::PciEndpoint,
        };

        devices.push(PciDeviceScope { bus: start_bus, device: dev, function: fun, scope_type });
        cursor += ds_len;
    }

    Some(IommuUnit {
        mmio_base:    reg_base,
        segment,
        catch_all,
        devices,
        capabilities: IommuCapabilities::default(),
    })
}

/// Read IOMMU capability registers (VER, CAP, ECAP) and fill in the
/// `IommuCapabilities` struct.
unsafe fn read_iommu_capabilities(unit: &mut IommuUnit) {
    let hhdm = crate::memory::hhdm_offset();
    let base = unit.mmio_base + hhdm;

    // Version register (32-bit)
    let ver = ((base + VTD_REG_VER as u64) as *const u32).read_volatile();
    unit.capabilities.version = ver;

    // Capability register (64-bit, CAP)
    let cap = ((base + VTD_REG_CAP as u64) as *const u64).read_volatile();

    // ND[2:0]: Number of domains (log2 base: 0=4k, 1=64k, …)
    let nd = (cap & 0x7) as u32;
    unit.capabilities.num_domains = 16 * (1 << nd);

    // AGAW[7:5]: Supported Adjusted Guest Address Widths
    let agaw_bits = ((cap >> 8) & 0x1F) as u8;
    unit.capabilities.agaw = if agaw_bits & 0x04 != 0 { 48 } else { 39 };
    unit.capabilities.supports_4level = (agaw_bits & 0x04) != 0;

    // Extended Capability register (64-bit, ECAP)
    let ecap = ((base + VTD_REG_ECAP as u64) as *const u64).read_volatile();
    // IR bit[3]: Interrupt Remapping support
    unit.capabilities.supports_ir = (ecap & 0x08) != 0;

    crate::serial_println!(
        "[DMAR] CAP: ver={:#x} domains={} agaw={} 4level={} ir={}",
        ver, unit.capabilities.num_domains, unit.capabilities.agaw,
        unit.capabilities.supports_4level, unit.capabilities.supports_ir
    );
}

// ============================================================================
// ACPI CHECKSUM VALIDATION
// ============================================================================

/// ACPI tablosunun checksum'unu doğrular.
///
/// ACPI spesifikasyonuna göre tüm byte'ların toplamı (mod 256) sıfır
/// olmalıdır. Bu sayede BIOS hatası veya bellek bozulması saptanabilir.
/// Checksum hatalıysa genellikle kritik bir sorun yoktur — sadece uyarı verilir.
unsafe fn acpi_checksum_valid(ptr: *const u8, len: usize) -> bool {
    let mut sum: u8 = 0;
    for i in 0..len {
        sum = sum.wrapping_add(*ptr.add(i));
    }
    sum == 0
}

// ============================================================================
// VT-d REGISTER OFFSETS (duplicated here to avoid circular import)
// ============================================================================

const VTD_REG_VER:  u64 = 0x000;
const VTD_REG_CAP:  u64 = 0x008;
const VTD_REG_ECAP: u64 = 0x010;
