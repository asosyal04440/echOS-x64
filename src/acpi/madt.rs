//! # MADT (Multiple APIC Description Table) Ayrıştırıcı
//!
//! ACPI MADT tablosunu ayrıştırarak APIC yapılandırmasını çıkarır.
//! MADT tablosu, sistemdeki tüm APIC (yerel ve G/Ç APIC) bilgilerini
//! ve kesme yönlendirme geçersiz kılmalarını (interrupt source overrides) içerir.
//! Bu bilgiler çok işlemcili başlatma (SMP) için kullanılır.

use acpi::platform::interrupt::{Apic, Polarity, TriggerMode};
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct ApicInfo {
    pub local_apic_address: u64,
    pub flags: u32,
    pub io_apics: Vec<IoApicInfo>,
    pub interrupt_overrides: Vec<InterruptOverride>,
}

#[derive(Clone, Debug)]
pub struct IoApicInfo {
    pub id: u8,
    pub address: u64,
    pub gsi_base: u32,
}

#[derive(Clone, Debug)]
pub struct InterruptOverride {
    pub bus: u8,
    pub source: u8,
    pub gsi: u32,
    pub flags: u16,
}

impl ApicInfo {
    pub const fn empty() -> Self {
        Self {
            local_apic_address: 0,
            flags: 0,
            io_apics: Vec::new(),
            interrupt_overrides: Vec::new(),
        }
    }
}

pub fn from_apic(apic: &Apic) -> ApicInfo {
    let io_apics = apic
        .io_apics
        .iter()
        .map(|ioapic| IoApicInfo {
            id: ioapic.id,
            address: ioapic.address as u64,
            gsi_base: ioapic.global_system_interrupt_base,
        })
        .collect();
    let interrupt_overrides = apic
        .interrupt_source_overrides
        .iter()
        .map(|iso| {
            let polarity_bits = match iso.polarity {
                Polarity::SameAsBus => 0,
                Polarity::ActiveHigh => 1,
                Polarity::ActiveLow => 3,
            };
            let trigger_bits = match iso.trigger_mode {
                TriggerMode::SameAsBus => 0,
                TriggerMode::Edge => 1,
                TriggerMode::Level => 3,
            };
            let flags = polarity_bits | (trigger_bits << 2);
            InterruptOverride {
                bus: 0,
                source: iso.isa_source,
                gsi: iso.global_system_interrupt,
                flags,
            }
        })
        .collect();

    ApicInfo {
        local_apic_address: apic.local_apic_address,
        flags: if apic.also_has_legacy_pics { 1 } else { 0 },
        io_apics,
        interrupt_overrides,
    }
}
