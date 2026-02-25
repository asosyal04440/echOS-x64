use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;
use x86_64::instructions::port::Port;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
}

#[derive(Debug, Clone)]
pub struct PciBar {
    pub base: u64,
    pub size: u64,
    pub is_64: bool,
}

#[derive(Debug, Clone)]
pub struct PciIoBar {
    pub base: u32,
    pub size: u32,
}

static PCI_MMIO_RESERVATIONS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

fn reserve_mmio_region(base: u64, size: u64) -> bool {
    if size == 0 {
        return false;
    }
    let end = base.saturating_add(size.saturating_sub(1));
    let mut regions = PCI_MMIO_RESERVATIONS.lock();
    for (r_base, r_size) in regions.iter() {
        let r_end = r_base.saturating_add(r_size.saturating_sub(1));
        if base <= r_end && end >= *r_base {
            return false;
        }
    }
    regions.push((base, size));
    true
}

fn linux_pci_priv(
    dev: *mut crate::linux_glue::PciDev,
) -> Option<&'static crate::linux_glue::LinuxPciPriv> {
    if dev.is_null() {
        return None;
    }
    let ptr = unsafe { (*dev).driver_data as *const crate::linux_glue::LinuxPciPriv };
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { &*ptr })
}

pub unsafe fn enable_bus_master(dev: *mut crate::linux_glue::PciDev) -> i32 {
    let priv_data = match linux_pci_priv(dev) {
        Some(data) => data,
        None => return -1,
    };
    let mut command = read_config_dword(priv_data.bus, priv_data.device, priv_data.function, 0x04);
    command |= (1 << 0) | (1 << 1) | (1 << 2);
    write_config_dword(
        priv_data.bus,
        priv_data.device,
        priv_data.function,
        0x04,
        command,
    );
    0
}

pub unsafe fn request_regions(dev: *mut crate::linux_glue::PciDev) -> i32 {
    if dev.is_null() {
        return -1;
    }
    let resources = unsafe { &(*dev).resource };
    let mut has_region = false;
    for res in resources {
        if res.start != 0 && res.end >= res.start {
            let size = res.end.saturating_sub(res.start).saturating_add(1);
            if (res.flags & 0x0000_0200) != 0 && !reserve_mmio_region(res.start, size) {
                return -1;
            }
            has_region = true;
        }
    }
    if has_region {
        0
    } else {
        -1
    }
}

pub fn scan() -> Vec<PciDevice> {
    let entries = crate::cpu::acpi::get_mcfg_entries();
    if !entries.is_empty() {
        scan_ecam(&entries)
    } else {
        scan_legacy()
    }
}

pub fn ecam_base_for_bus(bus: u8) -> Option<u64> {
    let entries = crate::cpu::acpi::get_mcfg_entries();
    for entry in entries {
        if bus >= entry.start_bus && bus <= entry.end_bus {
            return Some(entry.base_address);
        }
    }
    Some(0xE000_0000)
}

pub fn debug_print() {
    let devices = scan();
    for dev in devices {
        crate::serial_println!(
            "PCI {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_code,
            dev.subclass,
            dev.prog_if
        );
        let caps = read_capabilities(dev.bus, dev.device, dev.function);
        if caps.has_msi || caps.has_msix || caps.has_pcie {
            crate::serial_println!(
                "  CAPS MSI={} MSI-X={} PCIe={} msi_off=0x{:02x} msix_off=0x{:02x} pcie_off=0x{:02x}",
                caps.has_msi as u8,
                caps.has_msix as u8,
                caps.has_pcie as u8,
                caps.msi_offset,
                caps.msix_offset,
                caps.pcie_offset
            );
        }
    }
}

pub fn init_driver(dev: &PciDevice) -> bool {
    if dev.class_code == 0x01 && dev.subclass == 0x01 {
        crate::serial_println!(
            "Skipping IDE controller {:02x}:{:02x}.{}",
            dev.bus,
            dev.device,
            dev.function
        );
        return false;
    }
    if dev.vendor_id == 0x1AF4 && (dev.device_id == 0x1001 || dev.device_id == 0x1042) {
        let mut command = read_config_dword(dev.bus, dev.device, dev.function, 0x04);
        command |= (1 << 0) | (1 << 1) | (1 << 2);
        write_config_dword(dev.bus, dev.device, dev.function, 0x04, command);
        crate::serial_println!(
            "PCI CMD ENABLED for Dev {:x}:{:x}",
            dev.vendor_id,
            dev.device_id
        );
        crate::serial_println!("VIRTIO BLK: init transport main tarafından sağlanacak");
        return false;
    }
    false
}

#[derive(Debug, Clone, Copy)]
pub struct PciCapabilityInfo {
    pub has_msi: bool,
    pub has_msix: bool,
    pub has_pcie: bool,
    pub msi_offset: u8,
    pub msix_offset: u8,
    pub pcie_offset: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct MsiCapability {
    pub control: u16,
    pub address: u64,
    pub data: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct MsixCapability {
    pub control: u16,
    pub table_bar: u8,
    pub table_offset: u32,
    pub pba_bar: u8,
    pub pba_offset: u32,
}

fn read_config_word(bus: u8, device: u8, function: u8, offset: u16) -> u16 {
    let value = read_config_dword(bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

fn read_config_byte(bus: u8, device: u8, function: u8, offset: u16) -> u8 {
    let value = read_config_dword(bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

fn write_config_word(bus: u8, device: u8, function: u8, offset: u16, value: u16) {
    let aligned = offset & 0xFFFC;
    let current = read_config_dword(bus, device, function, aligned);
    let shift = ((offset & 2) * 8) as u32;
    let mask = 0xFFFFu32 << shift;
    let new_value = (current & !mask) | ((value as u32) << shift);
    write_config_dword(bus, device, function, aligned, new_value);
}

fn walk_capabilities<F>(bus: u8, device: u8, function: u8, mut f: F)
where
    F: FnMut(u8, u8),
{
    let status = read_config_word(bus, device, function, 0x06);
    if (status & (1 << 4)) == 0 {
        return;
    }
    let mut ptr = read_config_byte(bus, device, function, 0x34);
    let mut guard = 0u8;
    while ptr >= 0x40 && ptr != 0 && guard < 64 {
        let cap_id = read_config_byte(bus, device, function, ptr as u16);
        if cap_id == 0 || cap_id == 0xFF {
            break;
        }
        let next = read_config_byte(bus, device, function, ptr as u16 + 1);
        f(cap_id, ptr);
        if next == 0 || next == ptr {
            break;
        }
        ptr = next;
        guard = guard.wrapping_add(1);
    }
}

pub fn read_capabilities(bus: u8, device: u8, function: u8) -> PciCapabilityInfo {
    let mut info = PciCapabilityInfo {
        has_msi: false,
        has_msix: false,
        has_pcie: false,
        msi_offset: 0,
        msix_offset: 0,
        pcie_offset: 0,
    };
    walk_capabilities(bus, device, function, |cap_id, ptr| match cap_id {
        0x05 => {
            if !info.has_msi {
                info.has_msi = true;
                info.msi_offset = ptr;
            }
        }
        0x11 => {
            if !info.has_msix {
                info.has_msix = true;
                info.msix_offset = ptr;
            }
        }
        0x10 => {
            if !info.has_pcie {
                info.has_pcie = true;
                info.pcie_offset = ptr;
            }
        }
        _ => {}
    });
    info
}

pub fn read_msi(bus: u8, device: u8, function: u8) -> Option<MsiCapability> {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return None;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let addr_low = read_config_dword(bus, device, function, offset + 4) as u64;
    let address = if is_64 {
        let addr_high = read_config_dword(bus, device, function, offset + 8) as u64;
        (addr_high << 32) | addr_low
    } else {
        addr_low
    };
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    let data = read_config_word(bus, device, function, data_offset);
    Some(MsiCapability {
        control,
        address,
        data,
    })
}

pub fn read_msix(bus: u8, device: u8, function: u8) -> Option<MsixCapability> {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return None;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table = read_config_dword(bus, device, function, offset + 4);
    let pba = read_config_dword(bus, device, function, offset + 8);
    let table_bar = (table & 0x7) as u8;
    let table_offset = table & !0x7;
    let pba_bar = (pba & 0x7) as u8;
    let pba_offset = pba & !0x7;
    Some(MsixCapability {
        control,
        table_bar,
        table_offset,
        pba_bar,
        pba_offset,
    })
}

fn msi_message_address(apic_id: u32) -> u64 {
    let mode = crate::apic::lapic::mode();
    let dest = match mode {
        crate::apic::lapic::ApicMode::X2Apic => apic_id as u64,
        _ => (apic_id as u64) & 0xFF,
    };
    0xFEE0_0000u64 | (dest << 12)
}

fn msi_message_data(vector: u8) -> u16 {
    vector as u16
}

pub fn configure_msi(bus: u8, device: u8, function: u8, vector: u8, apic_id: u32) -> bool {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return false;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let address = msi_message_address(apic_id);
    let data = msi_message_data(vector);
    write_config_dword(bus, device, function, offset + 4, address as u32);
    if is_64 {
        write_config_dword(bus, device, function, offset + 8, (address >> 32) as u32);
    }
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    write_config_word(bus, device, function, data_offset, data);
    let mut new_control = control & !0x70;
    new_control |= 1;
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

pub fn configure_msi_multi(
    bus: u8,
    device: u8,
    function: u8,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msi || caps.msi_offset == 0 {
        return false;
    }
    let base_vector = vectors[0];
    for (idx, &vector) in vectors.iter().enumerate() {
        if vector != base_vector.wrapping_add(idx as u8) {
            return false;
        }
    }
    let count = vectors.len() as u16;
    if count == 0 || (count & (count - 1)) != 0 {
        return false;
    }
    let offset = caps.msi_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let is_64 = (control & (1 << 7)) != 0;
    let max_log = ((control >> 1) & 0x7) as u16;
    let req_log = count.trailing_zeros() as u16;
    if req_log > max_log {
        return false;
    }
    let address = msi_message_address(apic_id);
    let data = msi_message_data(base_vector);
    write_config_dword(bus, device, function, offset + 4, address as u32);
    if is_64 {
        write_config_dword(bus, device, function, offset + 8, (address >> 32) as u32);
    }
    let data_offset = if is_64 { offset + 12 } else { offset + 8 };
    write_config_word(bus, device, function, data_offset, data);
    let mut new_control = control & !0x70;
    new_control |= (req_log << 4) & 0x70;
    new_control |= 1;
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

pub fn configure_msix(
    bus: u8,
    device: u8,
    function: u8,
    table_index: u16,
    vector: u8,
    apic_id: u32,
) -> bool {
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return false;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table_size = (control & 0x07FF) as u16 + 1;
    if table_index >= table_size {
        return false;
    }
    let msix = match read_msix(bus, device, function) {
        Some(msix) => msix,
        None => return false,
    };
    let bar = match read_bar_mmio(bus, device, function, msix.table_bar) {
        Some(bar) => bar,
        None => return false,
    };
    let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
    let base = if mapped.is_null() {
        crate::memory::active_physical_offset() + bar.base
    } else {
        mapped as u64
    };
    let entry_addr = base + msix.table_offset as u64 + (table_index as u64) * 16;
    let address = msi_message_address(apic_id);
    let data = msi_message_data(vector) as u32;
    unsafe {
        write_volatile(entry_addr as *mut u32, address as u32);
        write_volatile((entry_addr + 4) as *mut u32, (address >> 32) as u32);
        write_volatile((entry_addr + 8) as *mut u32, data);
        write_volatile((entry_addr + 12) as *mut u32, 0);
    }
    let mut new_control = control | (1 << 15);
    new_control &= !(1 << 14);
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

pub fn configure_msix_table(
    bus: u8,
    device: u8,
    function: u8,
    table_base: u16,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let caps = read_capabilities(bus, device, function);
    if !caps.has_msix || caps.msix_offset == 0 {
        return false;
    }
    let offset = caps.msix_offset as u16;
    let control = read_config_word(bus, device, function, offset + 2);
    let table_size = (control & 0x07FF) as u16 + 1;
    if table_base + vectors.len() as u16 > table_size {
        return false;
    }
    let msix = match read_msix(bus, device, function) {
        Some(msix) => msix,
        None => return false,
    };
    let bar = match read_bar_mmio(bus, device, function, msix.table_bar) {
        Some(bar) => bar,
        None => return false,
    };
    let mapped = crate::memory::map_mmio(bar.base, bar.size as usize);
    let base = if mapped.is_null() {
        crate::memory::active_physical_offset() + bar.base
    } else {
        mapped as u64
    };
    let address = msi_message_address(apic_id);
    for (idx, &vector) in vectors.iter().enumerate() {
        let entry = table_base as u64 + idx as u64;
        let entry_addr = base + msix.table_offset as u64 + entry * 16;
        let data = msi_message_data(vector) as u32;
        unsafe {
            write_volatile(entry_addr as *mut u32, address as u32);
            write_volatile((entry_addr + 4) as *mut u32, (address >> 32) as u32);
            write_volatile((entry_addr + 8) as *mut u32, data);
            write_volatile((entry_addr + 12) as *mut u32, 0);
        }
    }
    let mut new_control = control | (1 << 15);
    new_control &= !(1 << 14);
    write_config_word(bus, device, function, offset + 2, new_control);
    let mut command = read_config_word(bus, device, function, 0x04);
    command |= 1 << 10;
    write_config_word(bus, device, function, 0x04, command);
    true
}

pub fn configure_pci_interrupt(
    bus: u8,
    device: u8,
    function: u8,
    vector: u8,
    apic_id: u32,
) -> bool {
    let policy = crate::interrupts::pci_interrupt_policy();
    let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
    match policy {
        crate::interrupts::PciInterruptPolicy::MsiXPreferred => {
            if configure_msix(bus, device, function, 0, vector, target_apic) {
                return true;
            }
            configure_msi(bus, device, function, vector, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::MsiPreferred => {
            if configure_msi(bus, device, function, vector, target_apic) {
                return true;
            }
            configure_msix(bus, device, function, 0, vector, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::LegacyOnly => false,
    }
}

pub fn configure_pci_interrupts(
    bus: u8,
    device: u8,
    function: u8,
    vectors: &[u8],
    apic_id: u32,
) -> bool {
    if vectors.is_empty() {
        return false;
    }
    let policy = crate::interrupts::pci_interrupt_policy();
    match policy {
        crate::interrupts::PciInterruptPolicy::MsiXPreferred => {
            let mut ok = true;
            for (idx, &vector) in vectors.iter().enumerate() {
                let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
                if !configure_msix(bus, device, function, idx as u16, vector, target_apic) {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
            let target_apic = crate::interrupts::resolve_msi_target(vectors[0], apic_id);
            configure_msi_multi(bus, device, function, vectors, target_apic)
        }
        crate::interrupts::PciInterruptPolicy::MsiPreferred => {
            let target_apic = crate::interrupts::resolve_msi_target(vectors[0], apic_id);
            if configure_msi_multi(bus, device, function, vectors, target_apic) {
                return true;
            }
            let mut ok = true;
            for (idx, &vector) in vectors.iter().enumerate() {
                let target_apic = crate::interrupts::resolve_msi_target(vector, apic_id);
                if !configure_msix(bus, device, function, idx as u16, vector, target_apic) {
                    ok = false;
                    break;
                }
            }
            ok
        }
        crate::interrupts::PciInterruptPolicy::LegacyOnly => false,
    }
}

// PCI config alanını cihazın erişim yöntemine göre okur
pub fn read_config_dword(bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    let aligned = offset & 0xFFFC;
    let entries = crate::cpu::acpi::get_mcfg_entries();
    for entry in entries {
        if bus >= entry.start_bus && bus <= entry.end_bus {
            return read_ecam_dword(entry.base_address, bus, device, function, aligned);
        }
    }
    read_legacy_dword(bus, device, function, aligned as u8)
}

// PCI config alanına cihazın erişim yöntemine göre yazar
pub fn write_config_dword(bus: u8, device: u8, function: u8, offset: u16, value: u32) {
    let aligned = offset & 0xFFFC;
    let entries = crate::cpu::acpi::get_mcfg_entries();
    for entry in entries {
        if bus >= entry.start_bus && bus <= entry.end_bus {
            let address = entry.base_address
                + ((bus as u64) << 20)
                + ((device as u64) << 15)
                + ((function as u64) << 12)
                + ((aligned as u64) & 0xFFC);
            unsafe {
                write_volatile(address as *mut u32, value);
            }
            return;
        }
    }
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((aligned as u32) & 0xFC);
    unsafe {
        let mut addr_port = Port::<u32>::new(0xCF8);
        let mut data_port = Port::<u32>::new(0xCFC);
        addr_port.write(address);
        data_port.write(value);
    }
}

// PCI BAR'ını okuyup MMIO taban ve boyut bilgisini çıkarır
pub fn read_bar_mmio(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<PciBar> {
    if bar_index >= 6 {
        return None;
    }
    let offset = 0x10u16 + (bar_index as u16) * 4;
    let original = read_config_dword(bus, device, function, offset);
    if original == 0 || (original & 0x1) != 0 {
        return None;
    }
    let is_64 = (original & 0x6) == 0x4;
    write_config_dword(bus, device, function, offset, 0xFFFF_FFFF);
    let mask_low = read_config_dword(bus, device, function, offset);
    write_config_dword(bus, device, function, offset, original);
    let masked_low = mask_low & 0xFFFF_FFF0;
    if masked_low == 0 {
        return None;
    }
    let mut base = (original & 0xFFFF_FFF0) as u64;
    let mut size = (!masked_low + 1) as u64;
    if is_64 {
        let offset_high = offset + 4;
        let original_high = read_config_dword(bus, device, function, offset_high);
        write_config_dword(bus, device, function, offset_high, 0xFFFF_FFFF);
        let mask_high = read_config_dword(bus, device, function, offset_high);
        write_config_dword(bus, device, function, offset_high, original_high);
        let mask_full = ((mask_high as u64) << 32) | masked_low as u64;
        if mask_full == 0 {
            return None;
        }
        base |= (original_high as u64) << 32;
        size = (!mask_full + 1) & 0xFFFF_FFFF_FFFF_FFF0;
    } else {
        size &= 0xFFFF_FFF0;
    }
    Some(PciBar { base, size, is_64 })
}

pub fn read_bar_io(bus: u8, device: u8, function: u8, bar_index: u8) -> Option<PciIoBar> {
    if bar_index >= 6 {
        return None;
    }
    let offset = 0x10u16 + (bar_index as u16) * 4;
    let original = read_config_dword(bus, device, function, offset);
    if original == 0 || (original & 0x1) == 0 {
        return None;
    }
    write_config_dword(bus, device, function, offset, 0xFFFF_FFFF);
    let mask = read_config_dword(bus, device, function, offset);
    write_config_dword(bus, device, function, offset, original);
    let masked = mask & 0xFFFF_FFFC;
    if masked == 0 {
        return None;
    }
    let base = original & 0xFFFF_FFFC;
    let size = (!masked).wrapping_add(1) & 0xFFFF_FFFC;
    Some(PciIoBar { base, size })
}

// PCI köprü sınıfı kodları
const PCI_CLASS_BRIDGE: u8 = 0x06;
const PCI_SUBCLASS_PCI_BRIDGE: u8 = 0x04;

fn scan_legacy() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    // Legacy PCI için köprüleri takip ederek tarama yap
    let mut visited = [false; 256];
    scan_bus_legacy_recursive(0, &mut visited, &mut devices);
    devices
}

fn scan_bus_legacy_recursive(bus: u8, visited: &mut [bool; 256], devices: &mut Vec<PciDevice>) {
    // Aynı bus'ın tekrar taranmasını engelle
    if visited[bus as usize] {
        return;
    }
    visited[bus as usize] = true;
    for device in 0u8..32 {
        let vendor_id = read_legacy_word(bus, device, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue;
        }
        let header_type = read_legacy_byte(bus, device, 0, 0x0E);
        let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };
        for function in 0u8..function_count {
            let vendor_id = read_legacy_word(bus, device, function, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }
            let dev = read_device_legacy(bus, device, function);
            crate::serial_println!(
                "PCI SCAN {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if
            );
            if dev.class_code == 0x01 {
                crate::serial_println!(
                    "PCI STORAGE FOUND: Bus/Dev/Fn {:02x}:{:02x}.{} ID={:04x}:{:04x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    dev.vendor_id,
                    dev.device_id
                );
            }
            let is_bridge =
                dev.class_code == PCI_CLASS_BRIDGE && dev.subclass == PCI_SUBCLASS_PCI_BRIDGE;
            devices.push(dev);
            if is_bridge {
                let secondary = read_legacy_byte(bus, device, function, 0x19);
                if secondary != 0 && secondary != 0xFF {
                    scan_bus_legacy_recursive(secondary, visited, devices);
                }
            }
        }
    }
}

fn read_device_legacy(bus: u8, device: u8, function: u8) -> PciDevice {
    let vendor_id = read_legacy_word(bus, device, function, 0x00);
    let device_id = read_legacy_word(bus, device, function, 0x02);
    let prog_if = read_legacy_byte(bus, device, function, 0x09);
    let subclass = read_legacy_byte(bus, device, function, 0x0A);
    let class_code = read_legacy_byte(bus, device, function, 0x0B);
    let header_type = read_legacy_byte(bus, device, function, 0x0E);
    PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
    }
}

fn read_legacy_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_legacy_dword(bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

fn read_legacy_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_legacy_dword(bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

fn read_legacy_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);
    unsafe {
        let mut addr_port = Port::<u32>::new(0xCF8);
        let mut data_port = Port::<u32>::new(0xCFC);
        addr_port.write(address);
        data_port.read()
    }
}

fn scan_ecam(entries: &[crate::cpu::acpi::PciEcamInfo]) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for entry in entries {
        // Her segment için bus taramasını ayrı tut
        let mut visited = [false; 256];
        for bus in entry.start_bus..=entry.end_bus {
            if !visited[bus as usize] {
                scan_bus_ecam_recursive(
                    entry.base_address,
                    entry.start_bus,
                    entry.end_bus,
                    bus,
                    &mut visited,
                    &mut devices,
                );
            }
        }
    }
    devices
}

fn scan_bus_ecam_recursive(
    base: u64,
    start_bus: u8,
    end_bus: u8,
    bus: u8,
    visited: &mut [bool; 256],
    devices: &mut Vec<PciDevice>,
) {
    // ECAM aralığı dışına taşmayı engelle
    if bus < start_bus || bus > end_bus {
        return;
    }
    // Aynı bus'ın tekrar taranmasını engelle
    if visited[bus as usize] {
        return;
    }
    visited[bus as usize] = true;
    for device in 0u8..32 {
        let vendor_id = read_ecam_word(base, bus, device, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue;
        }
        let header_type = read_ecam_byte(base, bus, device, 0, 0x0E);
        let function_count = if header_type & 0x80 != 0 { 8 } else { 1 };
        for function in 0u8..function_count {
            let vendor_id = read_ecam_word(base, bus, device, function, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }
            let dev = read_device_ecam(base, bus, device, function);
            crate::serial_println!(
                "PCI SCAN {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x} subclass={:02x} prog_if={:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.class_code,
                dev.subclass,
                dev.prog_if
            );
            if dev.class_code == 0x01 {
                crate::serial_println!(
                    "PCI STORAGE FOUND: Bus/Dev/Fn {:02x}:{:02x}.{} ID={:04x}:{:04x}",
                    dev.bus,
                    dev.device,
                    dev.function,
                    dev.vendor_id,
                    dev.device_id
                );
            }
            let is_bridge =
                dev.class_code == PCI_CLASS_BRIDGE && dev.subclass == PCI_SUBCLASS_PCI_BRIDGE;
            devices.push(dev);
            if is_bridge {
                let secondary = read_ecam_byte(base, bus, device, function, 0x19);
                if secondary != 0 && secondary != 0xFF {
                    scan_bus_ecam_recursive(base, start_bus, end_bus, secondary, visited, devices);
                }
            }
        }
    }
}

fn read_device_ecam(base: u64, bus: u8, device: u8, function: u8) -> PciDevice {
    let vendor_id = read_ecam_word(base, bus, device, function, 0x00);
    let device_id = read_ecam_word(base, bus, device, function, 0x02);
    let prog_if = read_ecam_byte(base, bus, device, function, 0x09);
    let subclass = read_ecam_byte(base, bus, device, function, 0x0A);
    let class_code = read_ecam_byte(base, bus, device, function, 0x0B);
    let header_type = read_ecam_byte(base, bus, device, function, 0x0E);
    PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
    }
}

fn read_ecam_word(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
    let value = read_ecam_dword(base, bus, device, function, offset);
    let shift = ((offset & 2) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

fn read_ecam_byte(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
    let value = read_ecam_dword(base, bus, device, function, offset);
    let shift = ((offset & 3) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

fn read_ecam_dword(base: u64, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
    let address = base
        + ((bus as u64) << 20)
        + ((device as u64) << 15)
        + ((function as u64) << 12)
        + ((offset as u64) & 0xFFC);
    unsafe { read_volatile(address as *const u32) }
}
