//! # echOS ACPI (Advanced Configuration and Power Interface) Modülü
//!
//! ACPI tablo parsing, power management ve CPU/Memory topology discovery.
//! Minimal ACPICA subset implementasyonu.

use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// RSDP (Root System Description Pointer) signature
const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

/// ACPI tablo signature'ları
const FADT_SIGNATURE: &[u8; 4] = b"FACP";
const MADT_SIGNATURE: &[u8; 4] = b"APIC";
const SRAT_SIGNATURE: &[u8; 4] = b"SRAT";
const SLIT_SIGNATURE: &[u8; 4] = b"SLIT";
#[allow(dead_code)]
const SSDT_SIGNATURE: &[u8; 4] = b"SSDT";
const MCFG_SIGNATURE: &[u8; 4] = b"MCFG";
const DMAR_SIGNATURE: &[u8; 4] = b"DMAR";
const MAX_ACPI_TABLE_SIZE: u32 = 1024 * 1024;
static UEFI_RSDP_ADDRESS: AtomicU64 = AtomicU64::new(0);

/// Global ACPI durumu
pub static ACPI_STATE: Mutex<AcpiState> = Mutex::new(AcpiState::new());

/// ACPI durum yapısı
pub struct AcpiState {
    /// RSDP adresi
    pub rsdp_address: u64,
    /// XSDT (Extended System Description Table) adresi
    pub xsdt_address: u64,
    /// FADT (Fixed ACPI Description Table) adresi
    pub fadt_address: u64,
    /// MADT (Multiple APIC Description Table) adresi
    pub madt_address: u64,
    /// SRAT (System Resource Affinity Table) adresi
    pub srat_address: u64,
    /// SLIT (System Locality Information Table) adresi
    pub slit_address: u64,
    pub mcfg_address: u64,
    pub dmar_address: u64,
    /// Tespit edilen tablolar
    pub tables: Vec<AcpiTable>,
    /// CPU bilgileri
    pub cpu_info: AcpiCpuInfo,
    pub ioapics: Vec<IoApicInfo>,
    pub interrupt_overrides: Vec<InterruptOverride>,
    pub mcfg_entries: Vec<PciEcamInfo>,
    pub dmar_units: Vec<DmarDrhd>,

    // ── Power Management (FADT'den okunur) ──
    /// PM1a Control Block I/O port adresi
    pub pm1a_cnt_blk: u16,
    /// PM1b Control Block I/O port adresi (0 = yok)
    pub pm1b_cnt_blk: u16,
    /// ACPI S5 (shutdown) sleep type A değeri (DSDT \_S5'den okunur)
    pub slp_typ_s5_a: u16,
    /// ACPI S5 sleep type B değeri
    pub slp_typ_s5_b: u16,
    /// ACPI Enable komutu (SMI CMD ile gönderilir)
    pub acpi_enable_cmd: u8,
    /// SMI Command port
    pub smi_cmd_port: u32,
    /// PM1a Event Block adresi
    pub pm1a_evt_blk: u16,
    /// RESET register adresi (Generic Address Structure)
    pub reset_reg_addr: u64,
    /// RESET register adres uzayı (0=bellek, 1=I/O)
    pub reset_reg_space: u8,
    /// RESET register değeri
    pub reset_value: u8,
    /// FADT flags (bit 10 = RESET_REG_SUP)
    pub fadt_flags: u32,
    /// SCI interrupt numarası
    pub sci_interrupt: u16,
    /// FADT başarıyla parse edildi mi?
    pub fadt_parsed: bool,
}

/// ACPI tablo yapısı
#[derive(Debug, Clone)]
pub struct AcpiTable {
    pub signature: [u8; 4],
    pub address: u64,
    pub length: u32,
}

/// ACPI CPU bilgileri
#[derive(Debug, Clone)]
pub struct AcpiCpuInfo {
    /// Toplam CPU sayısı
    pub cpu_count: u32,
    /// BSP (Bootstrap Processor) APIC ID
    pub bsp_apic_id: u32,
    /// APIC base adresi
    pub apic_base: u64,
    /// CPU listesi (APIC ID'ler)
    pub cpu_list: Vec<u32>,
    /// NUMA node bilgileri
    pub numa_nodes: Vec<NumaNode>,
}

#[derive(Debug, Clone)]
pub struct IoApicInfo {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

#[derive(Debug, Clone)]
pub struct InterruptOverride {
    pub bus: u8,
    pub source: u8,
    pub gsi: u32,
    pub flags: u16,
}

#[derive(Debug, Clone)]
pub struct PciEcamInfo {
    pub base_address: u64,
    pub segment_group: u16,
    pub start_bus: u8,
    pub end_bus: u8,
}

#[derive(Debug, Clone)]
pub struct DmarDeviceScope {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Debug, Clone)]
pub struct DmarDrhd {
    pub segment: u16,
    pub register_base: u64,
    pub include_all: bool,
    pub devices: Vec<DmarDeviceScope>,
}

/// NUMA node yapısı
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub node_id: u32,
    pub base_address: u64,
    pub length: u64,
    pub cpu_affinity: Vec<u32>,
}

impl AcpiState {
    /// Yeni ACPI durumu oluşturur
    pub const fn new() -> Self {
        Self {
            rsdp_address: 0,
            xsdt_address: 0,
            fadt_address: 0,
            madt_address: 0,
            srat_address: 0,
            slit_address: 0,
            mcfg_address: 0,
            dmar_address: 0,
            tables: Vec::new(),
            cpu_info: AcpiCpuInfo {
                cpu_count: 1,
                bsp_apic_id: 0,
                apic_base: 0xFEE00000,
                cpu_list: Vec::new(),
                numa_nodes: Vec::new(),
            },
            ioapics: Vec::new(),
            interrupt_overrides: Vec::new(),
            mcfg_entries: Vec::new(),
            dmar_units: Vec::new(),
            pm1a_cnt_blk: 0,
            pm1b_cnt_blk: 0,
            slp_typ_s5_a: 0,
            slp_typ_s5_b: 0,
            acpi_enable_cmd: 0,
            smi_cmd_port: 0,
            pm1a_evt_blk: 0,
            reset_reg_addr: 0,
            reset_reg_space: 0,
            reset_value: 0,
            fadt_flags: 0,
            sci_interrupt: 0,
            fadt_parsed: false,
        }
    }
}

/// RSDP (Root System Description Pointer) yapısı
#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

/// SDT (System Description Table) header yapısı
#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// MADT (Multiple APIC Description Table) yapısı
#[repr(C, packed)]
struct Madt {
    header: SdtHeader,
    local_apic_address: u32,
    flags: u32,
    // Entries follow
}

/// MADT entry tipleri
const MADT_ENTRY_LOCAL_APIC: u8 = 0;
const MADT_ENTRY_IO_APIC: u8 = 1;
const MADT_ENTRY_INTERRUPT_OVERRIDE: u8 = 2;
#[allow(dead_code)]
const MADT_ENTRY_NMI: u8 = 4;
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_APIC_NMI: u8 = 5;
const MADT_ENTRY_LOCAL_APIC_ADDRESS_OVERRIDE: u8 = 6;
#[allow(dead_code)]
const MADT_ENTRY_IO_SAPIC: u8 = 7;
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_SAPIC: u8 = 8;
#[allow(dead_code)]
const MADT_ENTRY_PLATFORM_INTERRUPT: u8 = 9;
const MADT_ENTRY_LOCAL_X2APIC: u8 = 10;
#[allow(dead_code)]
const MADT_ENTRY_LOCAL_X2APIC_NMI: u8 = 11;

/// MADT Local APIC entry
#[repr(C, packed)]
struct MadtLocalApic {
    entry_type: u8,
    length: u8,
    processor_id: u8,
    apic_id: u8,
    flags: u32,
}

/// MADT Local x2APIC entry
#[repr(C, packed)]
struct MadtLocalX2Apic {
    entry_type: u8,
    length: u8,
    reserved: u16,
    x2apic_id: u32,
    flags: u32,
    acpi_processor_uid: u32,
}

#[repr(C, packed)]
struct MadtIoApic {
    entry_type: u8,
    length: u8,
    ioapic_id: u8,
    reserved: u8,
    ioapic_address: u32,
    gsi_base: u32,
}

#[repr(C, packed)]
struct MadtInterruptOverride {
    entry_type: u8,
    length: u8,
    bus: u8,
    source: u8,
    gsi: u32,
    flags: u16,
}

#[repr(C, packed)]
struct MadtLocalApicAddressOverride {
    entry_type: u8,
    length: u8,
    reserved: u16,
    address: u64,
}

#[repr(C, packed)]
struct McfgHeader {
    header: SdtHeader,
    reserved: u64,
}

#[repr(C, packed)]
struct McfgEntry {
    base_address: u64,
    segment_group: u16,
    start_bus: u8,
    end_bus: u8,
    reserved: u32,
}

/// RSDP'yi bul (UEFI veya legacy BIOS)
pub fn find_rsdp() -> Option<u64> {
    // UEFI sistemlerde RSDP Configuration Table'da
    if let Some(uefi_rsdp) = find_rsdp_uefi() {
        return Some(uefi_rsdp);
    }

    // Legacy BIOS: 0xE0000 - 0xFFFFF arasında ara
    find_rsdp_bios()
}

/// UEFI Configuration Table'dan RSDP bul
fn find_rsdp_uefi() -> Option<u64> {
    let addr = UEFI_RSDP_ADDRESS.load(Ordering::Relaxed);
    if addr != 0 {
        return Some(addr);
    }
    None
}

pub fn set_uefi_rsdp_address(address: u64) {
    if address != 0 {
        UEFI_RSDP_ADDRESS.store(address, Ordering::Relaxed);
    }
}

/// Legacy BIOS bölgesinde RSDP ara
fn find_rsdp_bios() -> Option<u64> {
    // EBDA (Extended BIOS Data Area) ve BIOS ROM bölgesinde ara
    let search_areas = [
        (0x000E0000, 0x000FFFFF), // BIOS ROM
        (0x00080000, 0x0009FFFF), // Possible EBDA
    ];

    for (start, end) in search_areas.iter() {
        if let Some(addr) = scan_for_rsdp(*start, *end) {
            return Some(addr);
        }
    }

    None
}

/// Bellek bölgesinde RSDP ara
fn scan_for_rsdp(start: u64, end: u64) -> Option<u64> {
    // 16-byte boundary'lerde ara
    for addr in (start..end).step_by(16) {
        unsafe {
            let ptr = phys_to_virt_ptr::<Rsdp>(addr);

            // Signature kontrolü
            if (*ptr).signature == *RSDP_SIGNATURE {
                // Checksum kontrolü
                if validate_checksum(ptr as *const u8, 20) {
                    return Some(addr);
                }

                // ACPI 2.0+ için extended checksum
                if (*ptr).revision >= 2 {
                    let length = (*ptr).length as usize;
                    if validate_checksum(ptr as *const u8, length) {
                        return Some(addr);
                    }
                }
            }
        }
    }

    None
}

/// Checksum doğrula
fn validate_checksum(data: *const u8, length: usize) -> bool {
    let mut sum: u8 = 0;

    for i in 0..length {
        unsafe {
            sum = sum.wrapping_add(*data.add(i));
        }
    }

    sum == 0
}

fn read_sdt_length(header: *const SdtHeader) -> u32 {
    unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header).length)) }
}

fn read_sdt_signature(header: *const SdtHeader) -> [u8; 4] {
    unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*header).signature)) }
}

/// ACPI tablolarını parse et
pub fn parse_acpi_tables() -> bool {
    crate::serial_println!("Parsing ACPI tables...");

    // RSDP'yi bul
    let rsdp_addr = match find_rsdp() {
        Some(addr) => addr,
        None => {
            crate::serial_println!("ACPI: RSDP not found");
            return false;
        }
    };

    crate::serial_println!("ACPI: RSDP found at 0x{:X}", rsdp_addr);

    let mut state = ACPI_STATE.lock();
    state.rsdp_address = rsdp_addr;

    // RSDP'yi oku
    let rsdp = unsafe { &*phys_to_virt_ptr::<Rsdp>(rsdp_addr) };
    let rsdp_revision = rsdp.revision;
    let rsdp_xsdt = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(rsdp.xsdt_address)) };
    let rsdp_rsdt = rsdp.rsdt_address as u64;

    let mut parsed = false;
    crate::serial_println!(
        "ACPI: RSDP rev={} xsdt=0x{:X} rsdt=0x{:X}",
        rsdp_revision,
        rsdp_xsdt,
        rsdp_rsdt
    );
    if rsdp_revision >= 2 && rsdp_xsdt != 0 && is_canonical_lower_half(rsdp_xsdt) {
        state.xsdt_address = rsdp_xsdt;
        parsed = parse_xsdt(rsdp_xsdt, &mut state);
    }
    if !parsed {
        let rsdt_addr = rsdp_rsdt;
        if rsdt_addr != 0 && is_canonical_lower_half(rsdt_addr) {
            parsed = parse_rsdt(rsdt_addr, &mut state);
        }
    }
    if !parsed {
        crate::serial_println!("ACPI: XSDT/RSDT parse failed");
        return false;
    }

    // CPU bilgilerini çıkar
    extract_cpu_info(&mut state);

    crate::serial_println!("ACPI: Found {} tables", state.tables.len());
    crate::serial_println!("ACPI: {} CPUs detected", state.cpu_info.cpu_count);

    true
}

/// XSDT (Extended System Description Table) parse et
fn parse_xsdt(xsdt_addr: u64, state: &mut AcpiState) -> bool {
    if !is_canonical_lower_half(xsdt_addr) {
        return false;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(xsdt_addr);
    let header = unsafe { &*header_ptr };

    let xsdt_len = read_sdt_length(header_ptr);
    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Invalid XSDT signature={:?} len={}",
            read_sdt_signature(header_ptr),
            xsdt_len
        );
        crate::serial_println!("ACPI: Invalid XSDT");
        return false;
    }

    // Entry sayısı hesapla (64-bit pointers)
    let entry_count = (xsdt_len as usize - mem::size_of::<SdtHeader>()) / 8;
    crate::serial_println!("ACPI: XSDT entries={}", entry_count);

    let entries_base = phys_to_virt(xsdt_addr) + mem::size_of::<SdtHeader>();
    for i in 0..entry_count {
        let entry_addr = entries_base + i * 8;
        let table_addr = unsafe { core::ptr::read_unaligned(entry_addr as *const u64) };
        crate::serial_println!("ACPI: XSDT entry {} addr=0x{:X}", i, table_addr);
        parse_table(table_addr, state);
    }
    true
}

/// RSDT (Root System Description Table) parse et
fn parse_rsdt(rsdt_addr: u64, state: &mut AcpiState) -> bool {
    if !is_canonical_lower_half(rsdt_addr) {
        return false;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(rsdt_addr);
    let header = unsafe { &*header_ptr };

    let rsdt_len = read_sdt_length(header_ptr);
    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Invalid RSDT signature={:?} len={}",
            read_sdt_signature(header_ptr),
            rsdt_len
        );
        crate::serial_println!("ACPI: Invalid RSDT");
        return false;
    }

    // Entry sayısı hesapla (32-bit pointers)
    let entry_count = (rsdt_len as usize - mem::size_of::<SdtHeader>()) / 4;
    crate::serial_println!("ACPI: RSDT entries={}", entry_count);

    let entries_base = phys_to_virt(rsdt_addr) + mem::size_of::<SdtHeader>();
    for i in 0..entry_count {
        let entry_addr = entries_base + i * 4;
        let table_addr = unsafe { core::ptr::read_unaligned(entry_addr as *const u32) } as u64;
        crate::serial_println!("ACPI: RSDT entry {} addr=0x{:X}", i, table_addr);
        parse_table(table_addr, state);
    }
    true
}

/// Tekil tabloyu parse et
fn parse_table(table_addr: u64, state: &mut AcpiState) {
    if !is_canonical_lower_half(table_addr) {
        return;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(table_addr);
    let header = unsafe { &*header_ptr };

    if !validate_table(header_ptr) {
        crate::serial_println!(
            "ACPI: Table invalid sig={:?} len={}",
            read_sdt_signature(header_ptr),
            read_sdt_length(header_ptr)
        );
        return;
    }

    // Tabloyu kaydet
    let signature = read_sdt_signature(header_ptr);
    let length = read_sdt_length(header_ptr);
    let table = AcpiTable {
        signature,
        address: table_addr,
        length,
    };

    state.tables.push(table.clone());
    let sig = core::str::from_utf8(&signature).unwrap_or("????");
    crate::serial_println!("ACPI: Table {} at 0x{:X}", sig, table_addr);

    // Önemli tabloları işaretle
    match &header.signature {
        FADT_SIGNATURE => {
            state.fadt_address = table_addr;
            parse_fadt(table_addr, state);
        }
        MADT_SIGNATURE => {
            state.madt_address = table_addr;
            parse_madt(table_addr, state);
        }
        SRAT_SIGNATURE => {
            state.srat_address = table_addr;
            parse_srat(table_addr, state);
        }
        SLIT_SIGNATURE => {
            state.slit_address = table_addr;
            parse_slit(table_addr, state);
        }
        MCFG_SIGNATURE => {
            state.mcfg_address = table_addr;
            parse_mcfg(table_addr, state);
        }
        DMAR_SIGNATURE => {
            state.dmar_address = table_addr;
            parse_dmar(table_addr, state);
        }
        _ => {}
    }
}

pub fn get_dmar_units() -> Vec<DmarDrhd> {
    ACPI_STATE.lock().dmar_units.clone()
}

fn parse_dmar(table_addr: u64, state: &mut AcpiState) {
    if !is_canonical_lower_half(table_addr) {
        return;
    }
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(table_addr);
    let header = unsafe { &*header_ptr };
    if !validate_table(header_ptr) {
        return;
    }
    let table_len = read_sdt_length(header_ptr) as usize;
    if table_len < mem::size_of::<SdtHeader>() + 12 {
        return;
    }
    let mut offset = mem::size_of::<SdtHeader>() + 12;
    while offset + 4 <= table_len {
        let entry_addr = phys_to_virt(table_addr + offset as u64);
        let entry_type = unsafe { core::ptr::read_unaligned(entry_addr as *const u16) };
        let entry_len = unsafe { core::ptr::read_unaligned((entry_addr + 2) as *const u16) };
        if entry_len < 4 {
            break;
        }
        let entry_len = entry_len as usize;
        if offset + entry_len > table_len {
            break;
        }
        if entry_type == 0 {
            if entry_len >= 16 {
                let flags = unsafe { core::ptr::read_unaligned((entry_addr + 4) as *const u8) };
                let segment = unsafe { core::ptr::read_unaligned((entry_addr + 6) as *const u16) };
                let register_base =
                    unsafe { core::ptr::read_unaligned((entry_addr + 8) as *const u64) };
                let include_all = (flags & 0x1) != 0;
                let mut devices = Vec::new();
                let mut scope_offset = 16usize;
                while scope_offset + 6 <= entry_len {
                    let scope_addr = entry_addr + scope_offset;
                    let scope_len =
                        unsafe { core::ptr::read_unaligned((scope_addr + 1) as *const u8) };
                    if scope_len < 6 {
                        break;
                    }
                    let scope_len = scope_len as usize;
                    if scope_offset + scope_len > entry_len {
                        break;
                    }
                    let start_bus =
                        unsafe { core::ptr::read_unaligned((scope_addr + 5) as *const u8) };
                    if scope_len >= 8 {
                        let path = unsafe {
                            core::slice::from_raw_parts(
                                (scope_addr + 6) as *const u8,
                                scope_len - 6,
                            )
                        };
                        if path.len() >= 2 {
                            let device = path[0];
                            let function = path[1];
                            devices.push(DmarDeviceScope {
                                bus: start_bus,
                                device,
                                function,
                            });
                        }
                    }
                    scope_offset = scope_offset.saturating_add(scope_len);
                }
                state.dmar_units.push(DmarDrhd {
                    segment,
                    register_base,
                    include_all,
                    devices,
                });
            }
        }
        offset = offset.saturating_add(entry_len);
    }
    if !state.dmar_units.is_empty() {
        crate::serial_println!(
            "ACPI: DMAR units={} addr=0x{:X}",
            state.dmar_units.len(),
            table_addr
        );
    }
}

/// Tablo doğrula (signature ve checksum)
fn validate_table(header: *const SdtHeader) -> bool {
    let length = read_sdt_length(header);
    if length < mem::size_of::<SdtHeader>() as u32 || length > MAX_ACPI_TABLE_SIZE {
        return false;
    }
    if read_sdt_signature(header) == [0; 4] {
        return false;
    }
    // Checksum kontrolü
    let data = header as *const u8;
    if !validate_checksum(data, length as usize) {
        return false;
    }
    true
}

fn is_canonical_lower_half(addr: u64) -> bool {
    addr <= 0x000F_FFFF_FFFF_FFFF || addr >= 0xFFF0_0000_0000_0000
}

fn phys_to_virt(addr: u64) -> usize {
    crate::memory::phys_to_virt(addr as usize)
}

fn phys_to_virt_ptr<T>(addr: u64) -> *const T {
    phys_to_virt(addr) as *const T
}

// ============================================================================
// FADT (Fixed ACPI Description Table) — Power Management kayıt defteri
// ACPI Spec 6.5 §5.2.9 — offset'ler sabit, struct minimum 116 byte
// ============================================================================

/// FADT yapısı — sadece ihtiyaç duyulan alanlar
#[repr(C, packed)]
struct Fadt {
    header: SdtHeader,        // 0x00 (36 byte)
    firmware_ctrl: u32,       // 0x24
    dsdt: u32,                // 0x28  ← DSDT adresi (32-bit)
    reserved1: u8,            // 0x2C
    preferred_pm_profile: u8, // 0x2D
    sci_interrupt: u16,       // 0x2E  ← SCI IRQ
    smi_cmd: u32,             // 0x30  ← SMI Command Port
    acpi_enable: u8,          // 0x34  ← ACPI enable komutu
    acpi_disable: u8,         // 0x35
    s4bios_req: u8,           // 0x36
    pstate_cnt: u8,           // 0x37
    pm1a_evt_blk: u32,        // 0x38  ← PM1a Event Block
    pm1b_evt_blk: u32,        // 0x3C
    pm1a_cnt_blk: u32,        // 0x40  ← PM1a Control Block
    pm1b_cnt_blk: u32,        // 0x44  ← PM1b Control Block
    pm2_cnt_blk: u32,         // 0x48
    pm_tmr_blk: u32,          // 0x4C
    gpe0_blk: u32,            // 0x50
    gpe1_blk: u32,            // 0x54
    pm1_evt_len: u8,          // 0x58
    pm1_cnt_len: u8,          // 0x59
    pm2_cnt_len: u8,          // 0x5A
    pm_tmr_len: u8,           // 0x5B
    gpe0_blk_len: u8,         // 0x5C
    gpe1_blk_len: u8,         // 0x5D
    gpe1_base: u8,            // 0x5E
    cst_cnt: u8,              // 0x5F
    p_lvl2_lat: u16,          // 0x60
    p_lvl3_lat: u16,          // 0x62
    flush_size: u16,          // 0x64
    flush_stride: u16,        // 0x66
    duty_offset: u8,          // 0x68
    duty_width: u8,           // 0x69
    day_alarm: u8,            // 0x6A
    month_alarm: u8,          // 0x6B
    century: u8,              // 0x6C
    iapc_boot_arch: u16,      // 0x6D
    reserved2: u8,            // 0x6F
    flags: u32,               // 0x70  ← FADT flags (bit 10 = RESET_REG_SUP)
    // offset 0x74: RESET_REG (Generic Address Structure 12 bytes)
    reset_reg_space: u8,      // 0x74  ← Address space (0=mem, 1=I/O, 2=PCI)
    reset_reg_bit_width: u8,  // 0x75
    reset_reg_bit_offset: u8, // 0x76
    reset_reg_access_size: u8,// 0x77
    reset_reg_addr: u64,      // 0x78  ← RESET register adresi
    reset_value: u8,          // 0x80  ← RESET'e yazılacak değer
}

/// FADT'yi parse et — PM1a/PM1b, RESET_REG, SLP_TYP bilgilerini çıkar
fn parse_fadt(fadt_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Parsing FADT at 0x{:X}", fadt_addr);

    let header_ptr = phys_to_virt_ptr::<SdtHeader>(fadt_addr);
    let fadt_len = read_sdt_length(header_ptr);

    // FADT minimum 116 byte olmalı (ACPI 1.0)
    if fadt_len < 116 {
        crate::serial_println!("ACPI: FADT too small ({}B)", fadt_len);
        return;
    }

    let fadt = unsafe { &*phys_to_virt_ptr::<Fadt>(fadt_addr) };

    // PM1a/PM1b Control Block
    let pm1a = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1a_cnt_blk)) };
    let pm1b = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1b_cnt_blk)) };
    let pm1a_evt = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.pm1a_evt_blk)) };
    let smi_cmd = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.smi_cmd)) };
    let acpi_enable = fadt.acpi_enable;
    let sci_int = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.sci_interrupt)) };
    let flags = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.flags)) };

    state.pm1a_cnt_blk = pm1a as u16;
    state.pm1b_cnt_blk = pm1b as u16;
    state.pm1a_evt_blk = pm1a_evt as u16;
    state.smi_cmd_port = smi_cmd;
    state.acpi_enable_cmd = acpi_enable;
    state.sci_interrupt = sci_int;
    state.fadt_flags = flags;

    crate::serial_println!(
        "ACPI: FADT PM1a_CNT=0x{:X} PM1b_CNT=0x{:X} PM1a_EVT=0x{:X} SCI={}",
        pm1a, pm1b, pm1a_evt, sci_int
    );
    crate::serial_println!(
        "ACPI: FADT SMI_CMD=0x{:X} ACPI_ENABLE=0x{:X} flags=0x{:X}",
        smi_cmd, acpi_enable, flags
    );

    // RESET register (FADT uzunluğu >= 129 byte gerektirir, 0x80+1)
    if fadt_len >= 129 {
        let reset_space = fadt.reset_reg_space;
        let reset_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.reset_reg_addr)) };
        let reset_val = fadt.reset_value;
        state.reset_reg_space = reset_space;
        state.reset_reg_addr = reset_addr;
        state.reset_value = reset_val;

        let reset_supported = (flags >> 10) & 1 == 1;
        crate::serial_println!(
            "ACPI: RESET_REG space={} addr=0x{:X} val=0x{:X} supported={}",
            reset_space, reset_addr, reset_val, reset_supported
        );
    }

    // DSDT'den \_S5 (shutdown) sleep type'ı çıkar
    let dsdt_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(fadt.dsdt)) } as u64;
    if dsdt_addr != 0 && is_canonical_lower_half(dsdt_addr) {
        parse_dsdt_s5(dsdt_addr, state);
    }

    state.fadt_parsed = true;
    crate::serial_println!("ACPI: FADT parsed OK — shutdown/reboot ready");
}

/// DSDT'den \_S5 paketini bul — S5 sleep type değerlerini çıkar
///
/// DSDT AML'de \_S5 şöyle görünür:
/// ```text
/// "_S5_" 0x12 <PkgLength> <NumElements> <BytePrefix> <SLP_TYP_A> ...
/// ```
/// Byte prefix = 0x0A
fn parse_dsdt_s5(dsdt_addr: u64, state: &mut AcpiState) {
    let header_ptr = phys_to_virt_ptr::<SdtHeader>(dsdt_addr);
    let dsdt_len = read_sdt_length(header_ptr) as usize;

    if dsdt_len < mem::size_of::<SdtHeader>() + 4 {
        crate::serial_println!("ACPI: DSDT too small");
        return;
    }

    let dsdt_base = phys_to_virt(dsdt_addr);
    let data_start = dsdt_base + mem::size_of::<SdtHeader>();
    let data_end = dsdt_base + dsdt_len;

    // "\_S5_" byte pattern: 0x5F 0x53 0x35 0x5F veya "_S5_" ASCII
    let s5_sig: [u8; 4] = [b'_', b'S', b'5', b'_'];
    let mut found = false;

    let data_slice = unsafe {
        core::slice::from_raw_parts(data_start as *const u8, data_end - data_start)
    };

    for i in 0..data_slice.len().saturating_sub(20) {
        if data_slice[i..i+4] == s5_sig {
            // \_S5_ bulundu — paket veriyi parse et
            // Format: _S5_ 0x12 PkgLen NumElements 0x0A SLP_TYP_A [0x0A SLP_TYP_B] ...
            let mut offset = i + 4;

            // DefPackage opcode
            if offset < data_slice.len() && data_slice[offset] == 0x12 {
                offset += 1;

                // PkgLength (1-4 byte encoding, basit versiyon: 1 byte)
                if offset < data_slice.len() {
                    let pkg_len_byte = data_slice[offset];
                    if (pkg_len_byte & 0xC0) == 0 {
                        offset += 1; // 1-byte encoding
                    } else {
                        let following = ((pkg_len_byte >> 6) & 3) as usize;
                        offset += 1 + following;
                    }
                }

                // Num Elements
                if offset < data_slice.len() {
                    offset += 1;
                }

                // SLP_TYP_A (0x0A prefix = BytePrefix)
                if offset + 1 < data_slice.len() {
                    let slp_a = if data_slice[offset] == 0x0A {
                        offset += 1;
                        let v = data_slice[offset] as u16;
                        offset += 1;
                        v
                    } else if data_slice[offset] == 0x0B {
                        // WordPrefix
                        offset += 1;
                        let v = u16::from_le_bytes([data_slice[offset], data_slice[offset+1]]);
                        offset += 2;
                        v
                    } else {
                        let v = data_slice[offset] as u16;
                        offset += 1;
                        v
                    };

                    // SLP_TYP_B
                    let slp_b = if offset + 1 < data_slice.len() {
                        if data_slice[offset] == 0x0A {
                            offset += 1;
                            data_slice[offset] as u16
                        } else if data_slice[offset] == 0x0B {
                            offset += 1;
                            u16::from_le_bytes([data_slice[offset], data_slice[offset+1]])
                        } else {
                            data_slice[offset] as u16
                        }
                    } else {
                        slp_a
                    };

                    state.slp_typ_s5_a = slp_a;
                    state.slp_typ_s5_b = slp_b;
                    found = true;

                    crate::serial_println!(
                        "ACPI: DSDT \\_S5 found — SLP_TYP_A={} SLP_TYP_B={}",
                        slp_a, slp_b
                    );
                    break;
                }
            }
        }
    }

    if !found {
        // QEMU i440fx/q35 default: SLP_TYP_A = 0 (piix4) veya 5
        // Fallback: QEMU genelde SLP_TYP=0 veya 5 kullanır
        state.slp_typ_s5_a = 5;
        state.slp_typ_s5_b = 5;
        crate::serial_println!("ACPI: DSDT \\_S5 not found — using QEMU default SLP_TYP=5");
    }
}

/// MADT (Multiple APIC Description Table) parse et
fn parse_madt(madt_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found MADT at 0x{:X}", madt_addr);

    let madt_ptr = phys_to_virt_ptr::<Madt>(madt_addr);
    let madt = unsafe { &*madt_ptr };
    let local_apic_address =
        unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(madt.local_apic_address)) };
    state.cpu_info.apic_base = local_apic_address as u64;

    // MADT entries'leri parse et
    let header_ptr = unsafe { core::ptr::addr_of!((*madt_ptr).header) };
    let madt_len = read_sdt_length(header_ptr) as usize;
    let entries_start = phys_to_virt(madt_addr) + mem::size_of::<Madt>();
    let entries_end = phys_to_virt(madt_addr) + madt_len;

    let mut offset = entries_start;

    while offset < entries_end {
        let entry_type = unsafe { *(offset as *const u8) };
        let entry_length = unsafe { *((offset + 1) as *const u8) } as usize;

        match entry_type {
            MADT_ENTRY_LOCAL_APIC => {
                let entry = unsafe { &*(offset as *const MadtLocalApic) };

                // Copy to local to avoid unaligned reference
                let apic_id = entry.apic_id;
                let flags = entry.flags;

                if (flags & 1) != 0 || (flags & 2) != 0 {
                    // Enabled or Online Capable
                    crate::serial_println!("ACPI: Found APIC ID {} (flags={})", apic_id, flags);
                    state.cpu_info.cpu_list.push(apic_id as u32);
                }
            }

            MADT_ENTRY_LOCAL_X2APIC => {
                let entry = unsafe { &*(offset as *const MadtLocalX2Apic) };

                if (entry.flags & 1) != 0 {
                    // Enabled
                    state.cpu_info.cpu_list.push(entry.x2apic_id);

                    if (entry.flags & 2) != 0 {
                        // BSP
                        state.cpu_info.bsp_apic_id = entry.x2apic_id;
                    }
                }
            }
            MADT_ENTRY_IO_APIC => {
                let entry = unsafe { &*(offset as *const MadtIoApic) };
                state.ioapics.push(IoApicInfo {
                    id: entry.ioapic_id,
                    address: entry.ioapic_address,
                    gsi_base: entry.gsi_base,
                });
            }
            MADT_ENTRY_INTERRUPT_OVERRIDE => {
                let entry = unsafe { &*(offset as *const MadtInterruptOverride) };
                state.interrupt_overrides.push(InterruptOverride {
                    bus: entry.bus,
                    source: entry.source,
                    gsi: entry.gsi,
                    flags: entry.flags,
                });
            }
            MADT_ENTRY_LOCAL_APIC_ADDRESS_OVERRIDE => {
                let entry = unsafe { &*(offset as *const MadtLocalApicAddressOverride) };
                state.cpu_info.apic_base = entry.address;
            }

            _ => {
                // Diğer entry tipleri şimdilik ignore
            }
        }

        offset += entry_length;
    }

    if !state.cpu_info.cpu_list.is_empty() {
        state.cpu_info.cpu_count = state.cpu_info.cpu_list.len() as u32;
    }
}

fn parse_mcfg(mcfg_addr: u64, state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found MCFG at 0x{:X}", mcfg_addr);
    let header = unsafe { &*phys_to_virt_ptr::<McfgHeader>(mcfg_addr) };
    let header_ptr = core::ptr::addr_of!(header.header);
    if !validate_table(header_ptr) {
        return;
    }
    let entries_start = phys_to_virt(mcfg_addr) + mem::size_of::<McfgHeader>();
    let entries_end = phys_to_virt(mcfg_addr) + read_sdt_length(header_ptr) as usize;
    let mut offset = entries_start;
    while offset + mem::size_of::<McfgEntry>() <= entries_end {
        let entry = unsafe { &*(offset as *const McfgEntry) };
        state.mcfg_entries.push(PciEcamInfo {
            base_address: entry.base_address,
            segment_group: entry.segment_group,
            start_bus: entry.start_bus,
            end_bus: entry.end_bus,
        });
        offset += mem::size_of::<McfgEntry>();
    }
}

/// SRAT (System Resource Affinity Table) parse et
fn parse_srat(srat_addr: u64, _state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found SRAT at 0x{:X}", srat_addr);
    // NUMA node bilgilerini parse et
    // Şimdilik basit implementasyon
}

/// SLIT (System Locality Information Table) parse et
fn parse_slit(slit_addr: u64, _state: &mut AcpiState) {
    crate::serial_println!("ACPI: Found SLIT at 0x{:X}", slit_addr);
    // NUMA distance bilgilerini parse et
    // Şimdilik basit implementasyon
}

/// CPU bilgilerini çıkar
fn extract_cpu_info(state: &mut AcpiState) {
    // MADT'den CPU bilgilerini kullan
    if state.cpu_info.cpu_count == 0 {
        // Fallback: CPUID'den bilgi al
        state.cpu_info.cpu_count = 1;
        state.cpu_info.bsp_apic_id = 0;
        state.cpu_info.cpu_list.push(0);
    }
}

/// CPU bilgilerini döndür
pub fn get_cpu_info() -> Option<AcpiCpuInfo> {
    let state = ACPI_STATE.lock();

    if state.cpu_info.cpu_count > 0 {
        Some(state.cpu_info.clone())
    } else {
        None
    }
}

pub fn get_ioapics() -> Vec<IoApicInfo> {
    let state = ACPI_STATE.lock();
    state.ioapics.clone()
}

pub fn get_interrupt_overrides() -> Vec<InterruptOverride> {
    let state = ACPI_STATE.lock();
    state.interrupt_overrides.clone()
}

pub fn get_mcfg_entries() -> Vec<PciEcamInfo> {
    let state = ACPI_STATE.lock();
    state.mcfg_entries.clone()
}

/// ACPI başlatma
pub fn init() -> bool {
    crate::serial_println!("Initializing ACPI...");

    if parse_acpi_tables() {
        crate::serial_println!("ACPI initialized successfully");
        true
    } else {
        crate::serial_println!("ACPI initialization failed, using fallback");
        false
    }
}

// ============================================================================
// Güç Yönetimi — Kapatma / Yeniden Başlatma / Uyku
// ============================================================================

/// ACPI S5 (Soft Off) ile sistemi kapat.
/// QEMU'da bu QEMU'yu "stopped" durumuna sokar (= poweroff).
///
/// # Safety
/// Tüm I/O durdurulmalı ve interrupt'lar kapatılmalı.
pub fn acpi_shutdown() -> ! {
    let state = ACPI_STATE.lock();

    if !state.fadt_parsed || state.pm1a_cnt_blk == 0 {
        crate::serial_println!("[ACPI] FADT not parsed or PM1a_CNT=0 — trying QEMU fallback");
        drop(state);
        qemu_shutdown_fallback();
    }

    let pm1a = state.pm1a_cnt_blk;
    let pm1b = state.pm1b_cnt_blk;
    let mut slp_typ_a = state.slp_typ_s5_a;
    let mut slp_typ_b = state.slp_typ_s5_b;
    drop(state);

    // Faz 2: AML'den _S5 değerini oku — statik parse'ın üzerine yaz
    if let Some((aml_a, aml_b)) = crate::cpu::acpi_aml::get_s5_sleep_type() {
        slp_typ_a = aml_a;
        slp_typ_b = aml_b;
        crate::serial_println!("[ACPI] S5 from AML: SLP_TYP_A={} SLP_TYP_B={}", aml_a, aml_b);
    }

    crate::serial_println!("[ACPI] Shutting down (S5)...");
    crate::serial_println!(
        "[ACPI] PM1a=0x{:X} SLP_TYP_A={} PM1b=0x{:X} SLP_TYP_B={}",
        pm1a, slp_typ_a, pm1b, slp_typ_b
    );

    // Interrupt'ları kapat
    x86_64::instructions::interrupts::disable();

    // PM1a_CNT: SLP_TYP (bit 10-12) | SLP_EN (bit 13)
    let sleep_value_a = (slp_typ_a << 10) | (1 << 13);
    unsafe {
        x86_64::instructions::port::Port::<u16>::new(pm1a).write(sleep_value_a);
    }

    // PM1b_CNT varsa ona da yaz
    if pm1b != 0 {
        let sleep_value_b = (slp_typ_b << 10) | (1 << 13);
        unsafe {
            x86_64::instructions::port::Port::<u16>::new(pm1b).write(sleep_value_b);
        }
    }

    // Bu noktaya ulaşılmamalı — ulaşılırsa fallback
    crate::serial_println!("[ACPI] S5 did not work — trying QEMU fallback");
    qemu_shutdown_fallback();
}

/// QEMU özel shutdown portları (fallback)
fn qemu_shutdown_fallback() -> ! {
    // QEMU ISA debug exit device (isa-debug-exit)
    unsafe {
        // Port 0x604: QEMU ACPI shutdown (piix4/q35)
        x86_64::instructions::port::Port::<u16>::new(0x604).write(0x2000u16);
        // Port 0xB004: Bochs/older QEMU
        x86_64::instructions::port::Port::<u16>::new(0xB004).write(0x2000u16);
    }
    loop {
        x86_64::instructions::hlt();
    }
}

/// ACPI reboot: FADT RESET_REG kullanarak sistemi yeniden başlat.
/// Desteklenmezse keyboard controller (0x64) fallback.
pub fn acpi_reboot() -> ! {
    let state = ACPI_STATE.lock();

    let reset_supported = (state.fadt_flags >> 10) & 1 == 1;

    if state.fadt_parsed && reset_supported && state.reset_reg_addr != 0 {
        let space = state.reset_reg_space;
        let addr = state.reset_reg_addr;
        let value = state.reset_value;
        drop(state);

        crate::serial_println!("[ACPI] Rebooting via RESET_REG (space={} addr=0x{:X} val=0x{:X})", space, addr, value);

        x86_64::instructions::interrupts::disable();

        unsafe {
            match space {
                // System Memory
                0 => {
                    let ptr = phys_to_virt(addr) as *mut u8;
                    core::ptr::write_volatile(ptr, value);
                }
                // System I/O
                1 => {
                    x86_64::instructions::port::Port::<u8>::new(addr as u16).write(value);
                }
                // PCI Configuration Space (bus 0, dev 31, func 0)
                2 => {
                    // PCI config space erişimi — CF8/CFC
                    let pci_addr = 0x8000_0000u32 | ((addr as u32) & 0xFFFF);
                    x86_64::instructions::port::Port::<u32>::new(0xCF8).write(pci_addr);
                    x86_64::instructions::port::Port::<u8>::new(0xCFC).write(value);
                }
                _ => {}
            }
        }
    } else {
        drop(state);
        crate::serial_println!("[ACPI] RESET_REG not available — keyboard controller reset");
    }

    // Fallback: 8042 keyboard controller reset (triple fault tetikler)
    crate::serial_println!("[ACPI] Fallback: keyboard controller reset (0x64)");
    x86_64::instructions::interrupts::disable();
    unsafe {
        // 8042 reset command
        x86_64::instructions::port::Port::<u8>::new(0x64).write(0xFE);
    }

    // Son çare: triple fault
    loop {
        x86_64::instructions::hlt();
    }
}

/// Uyku durumuna geç (S1-S4).
/// Sadece FADT parsed ve PM1a_CNT mevcut olmalı.
pub unsafe fn enter_sleep_state(sleep_state: u8) -> bool {
    if sleep_state < 1 || sleep_state > 4 {
        return false;
    }

    let state = ACPI_STATE.lock();
    if !state.fadt_parsed || state.pm1a_cnt_blk == 0 {
        crate::serial_println!("[ACPI] Cannot enter S{}: FADT not available", sleep_state);
        return false;
    }

    let pm1a = state.pm1a_cnt_blk;
    let pm1b = state.pm1b_cnt_blk;
    drop(state);

    crate::serial_println!("[ACPI] Entering sleep state S{}", sleep_state);

    // SLP_TYP (bit 10-12) | SLP_EN (bit 13)
    let sleep_value = ((sleep_state as u16) << 10) | (1 << 13);

    x86_64::instructions::port::Port::<u16>::new(pm1a).write(sleep_value);

    if pm1b != 0 {
        x86_64::instructions::port::Port::<u16>::new(pm1b).write(sleep_value);
    }

    false
}

/// CPU frekans scaling (P-state değiştirme)
pub fn set_pstate(pstate: u8) -> bool {
    // CPUID leaf 6'dan P-state desteğini kontrol et
    let cpuid_result = crate::cpu::cpuid(6, 0);

    if (cpuid_result.eax & (1 << 1)) == 0 {
        return false; // P-state kontrolü desteklenmiyor
    }

    // MSR 0x199 (IA32_PERF_CTL) ile P-state ayarla
    unsafe {
        use x86_64::registers::model_specific::Msr;
        // MSR yazımı için mut gerekli.
        let mut perf_ctl = Msr::new(0x199);

        // P-state değeri (0 = highest, N = lowest)
        let value = (pstate as u64) & 0xFF;
        perf_ctl.write(value);
    }

    true
}

/// Thermal management
pub struct ThermalZone {
    pub temperature: i32, // Celsius × 10
    pub trip_points: Vec<TripPoint>,
}

pub struct TripPoint {
    pub temperature: i32,
    pub trip_type: u8, // 0: critical, 1: hot, 2: passive, 3: active
}

/// Thermal zone'ları tespit et
pub fn detect_thermal_zones() -> Vec<ThermalZone> {
    let mut zones = Vec::new();

    // ACPI thermal zone'larını ara
    // Şimdilik basit dummy zone
    zones.push(ThermalZone {
        temperature: 450, // 45.0°C
        trip_points: vec![
            TripPoint {
                temperature: 800,
                trip_type: 0,
            }, // Critical 80°C
            TripPoint {
                temperature: 700,
                trip_type: 1,
            }, // Hot 70°C
        ],
    });

    zones
}

/// Battery durumu (laptop'lar için)
pub struct BatteryInfo {
    pub present: bool,
    pub charging: bool,
    pub capacity: u8, // Yüzde
    pub voltage: u16, // mV
}

/// Battery bilgilerini al
pub fn get_battery_info() -> Option<BatteryInfo> {
    // ACPI battery device'larını kontrol et
    // Şimdilik basit implementasyon
    Some(BatteryInfo {
        present: false,
        charging: false,
        capacity: 0,
        voltage: 0,
    })
}

/// ACPI tablo bilgilerini debug için yazdır
pub fn debug_print_tables() {
    let state = ACPI_STATE.lock();

    crate::serial_println!("=== ACPI Debug Info ===");
    crate::serial_println!("RSDP: 0x{:X}", state.rsdp_address);
    crate::serial_println!("XSDT: 0x{:X}", state.xsdt_address);
    crate::serial_println!("FADT: 0x{:X}", state.fadt_address);
    crate::serial_println!("MADT: 0x{:X}", state.madt_address);
    crate::serial_println!("SRAT: 0x{:X}", state.srat_address);
    crate::serial_println!("SLIT: 0x{:X}", state.slit_address);
    crate::serial_println!("MCFG: 0x{:X}", state.mcfg_address);

    crate::serial_println!("CPU Count: {}", state.cpu_info.cpu_count);
    crate::serial_println!("BSP APIC ID: {}", state.cpu_info.bsp_apic_id);
    crate::serial_println!("APIC Base: 0x{:X}", state.cpu_info.apic_base);

    crate::serial_println!("CPU List: {:?}", state.cpu_info.cpu_list);
    crate::serial_println!("Tables found: {}", state.tables.len());

    for table in &state.tables {
        let sig = core::str::from_utf8(&table.signature).unwrap_or("????");
        crate::serial_println!("  {}: 0x{:X} ({} bytes)", sig, table.address, table.length);
    }
}
