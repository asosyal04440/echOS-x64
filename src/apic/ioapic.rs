use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

use crate::acpi::madt::{ApicInfo, InterruptOverride, IoApicInfo};
use crate::memory::{active_physical_offset, map_mmio};

const IOAPIC_REG_VER: u8 = 0x01;
const IOAPIC_REG_REDTBL_BASE: u8 = 0x10;

struct IoApic {
    id: u8,
    gsi_base: u32,
    mmio_base: u64,
}

struct IoApicState {
    ioapics: Vec<IoApic>,
    overrides: Vec<InterruptOverride>,
    bsp_apic_id: u8,
    affinity: BTreeMap<u8, u8>,
    trigger_override: BTreeMap<u8, bool>,
}

static IOAPIC_STATE: Mutex<IoApicState> = Mutex::new(IoApicState {
    ioapics: Vec::new(),
    overrides: Vec::new(),
    bsp_apic_id: 0,
    affinity: BTreeMap::new(),
    trigger_override: BTreeMap::new(),
});

impl IoApic {
    fn new(info: &IoApicInfo) -> Self {
        let mapped = map_mmio(info.address, 0x20);
        let mmio_base = if mapped.is_null() {
            active_physical_offset() + info.address
        } else {
            mapped as u64
        };
        Self {
            id: info.id,
            gsi_base: info.gsi_base,
            mmio_base,
        }
    }

    fn read_reg(&self, reg: u8) -> u32 {
        let reg_sel = self.mmio_base as *mut u32;
        let data = (self.mmio_base + 0x10) as *mut u32;
        unsafe {
            write_volatile(reg_sel, reg as u32);
            read_volatile(data)
        }
    }

    fn write_reg(&self, reg: u8, value: u32) {
        let reg_sel = self.mmio_base as *mut u32;
        let data = (self.mmio_base + 0x10) as *mut u32;
        unsafe {
            write_volatile(reg_sel, reg as u32);
            write_volatile(data, value);
        }
    }

    fn max_redirection_entries(&self) -> u32 {
        let ver = self.read_reg(IOAPIC_REG_VER);
        ((ver >> 16) & 0xFF) + 1
    }

    fn read_redirection(&self, index: u32) -> (u32, u32) {
        let reg = IOAPIC_REG_REDTBL_BASE + (index as u8 * 2);
        let low = self.read_reg(reg);
        let high = self.read_reg(reg + 1);
        (low, high)
    }

    fn write_redirection(&self, index: u32, low: u32, high: u32) {
        let reg = IOAPIC_REG_REDTBL_BASE + (index as u8 * 2);
        self.write_reg(reg, low);
        self.write_reg(reg + 1, high);
    }

    fn set_redirection(
        &self,
        index: u32,
        vector: u8,
        dest_apic_id: u8,
        polarity_low: bool,
        level_trigger: bool,
        masked: bool,
    ) {
        let mut low = vector as u32;
        if polarity_low {
            low |= 1 << 13;
        }
        if level_trigger {
            low |= 1 << 15;
        }
        if masked {
            low |= 1 << 16;
        }

        let high = (dest_apic_id as u32) << 24;
        self.write_redirection(index, low, high);
    }

    fn set_mask(&self, index: u32, masked: bool) {
        let (mut low, high) = self.read_redirection(index);
        if masked {
            low |= 1 << 16;
        } else {
            low &= !(1 << 16);
        }
        self.write_redirection(index, low, high);
    }
}

pub fn init(info: &ApicInfo, bsp_apic_id: u8) -> bool {
    let mut state = IOAPIC_STATE.lock();
    state.ioapics = info.io_apics.iter().map(IoApic::new).collect();
    state.overrides = info.interrupt_overrides.clone();
    state.bsp_apic_id = bsp_apic_id;
    state.affinity.clear();
    state.trigger_override.clear();

    if state.ioapics.is_empty() {
        return false;
    }

    for irq in 0u8..=15 {
        configure_irq(&mut state, irq, true);
    }

    true
}

pub fn enable_irq(irq: u8) {
    let mut state = IOAPIC_STATE.lock();
    configure_irq(&mut state, irq, false);
}

pub fn disable_irq(irq: u8) {
    let mut state = IOAPIC_STATE.lock();
    configure_irq(&mut state, irq, true);
}

pub fn set_irq_affinity(irq: u8, apic_id: u8) {
    let mut state = IOAPIC_STATE.lock();
    state.affinity.insert(irq, apic_id);
    configure_irq(&mut state, irq, false);
}

pub fn set_irq_trigger_mode(irq: u8, level_trigger: Option<bool>) {
    let mut state = IOAPIC_STATE.lock();
    match level_trigger {
        Some(value) => {
            state.trigger_override.insert(irq, value);
        }
        None => {
            state.trigger_override.remove(&irq);
        }
    }
    configure_irq(&mut state, irq, false);
}

fn configure_irq(state: &mut IoApicState, irq: u8, masked: bool) {
    let (gsi, polarity_low, mut level_trigger) = resolve_override(irq, &state.overrides);
    if let Some(override_trigger) = state.trigger_override.get(&irq) {
        level_trigger = *override_trigger;
    }
    if let Some((ioapic, index)) = find_ioapic_for_gsi(&mut state.ioapics, gsi) {
        let vector = 32u8.wrapping_add(irq);
        let dest_apic_id = state
            .affinity
            .get(&irq)
            .copied()
            .unwrap_or(state.bsp_apic_id);
        ioapic.set_redirection(
            index,
            vector,
            dest_apic_id,
            polarity_low,
            level_trigger,
            masked,
        );
    }
}

fn find_ioapic_for_gsi(ioapics: &mut [IoApic], gsi: u32) -> Option<(&mut IoApic, u32)> {
    for ioapic in ioapics.iter_mut() {
        let base = ioapic.gsi_base;
        let max = ioapic.max_redirection_entries();
        let end = base + max;
        if gsi >= base && gsi < end {
            return Some((ioapic, gsi - base));
        }
    }
    None
}

fn resolve_override(irq: u8, overrides: &[InterruptOverride]) -> (u32, bool, bool) {
    for entry in overrides {
        if entry.bus == 0 && entry.source == irq {
            let polarity = entry.flags & 0b11;
            let trigger = (entry.flags >> 2) & 0b11;
            let polarity_low = polarity == 3;
            let level_trigger = trigger == 3;
            return (entry.gsi, polarity_low, level_trigger);
        }
    }
    (irq as u32, false, false)
}
