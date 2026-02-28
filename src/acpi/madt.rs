//! # ACPI MADT (Multiple APIC Description Table) Modülü
//!
//! MADT, sistemdeki APIC (Advanced Programmable Interrupt Controller) yapılandırmasını
//! açıklar. Bu modül MADT'tan APIC bilgilerini çıkararak echOS kesme altyapısına aktarır.
//!
//! ## APIC Hiyerarşisi
//! ```ascii
//! MADT Tablosu
//!    |
//!    ├── Local APIC adresi (her CPU için)
//!    ├── I/O APIC(lar) — donanım kesmelerini yönetir
//!    └── Kesme Geçişleri (ISO) — IRQ -> GSI eşlemesi
//! ```

use acpi::platform::interrupt::{Apic, Polarity, TriggerMode};
use alloc::vec::Vec;

/// APIC yapılandırma özeti — MADT'tan çıkarılan tüm APIC bilgilerini içerir.
#[derive(Clone, Debug)]
pub struct ApicInfo {
    /// Yerel APIC fiziksel bellek adresi (tüm CPU'lar için ortak)
    pub local_apic_address: u64,
    /// APIC bayrakları (bit 0: eski PIC'ler de mevcut)
    pub flags: u32,
    /// Sistemdeki I/O APIC'lerin listesi
    pub io_apics: Vec<IoApicInfo>,
    /// ISA IRQ -> GSI kesme geçiş (override) kayıtları
    pub interrupt_overrides: Vec<InterruptOverride>,
}

/// Tek bir I/O APIC bilgisi.
///
/// Her I/O APIC belirli bir GSI (Global System Interrupt) aralığını yönetir.
#[derive(Clone, Debug)]
pub struct IoApicInfo {
    /// I/O APIC kimlik numarası
    pub id: u8,
    /// I/O APIC MMIO adresi
    pub address: u64,
    /// Bu I/O APIC'in başladığı GSI tabanı
    pub gsi_base: u32,
}

/// ISA IRQ -> GSI kesme geçiş kaydı.
///
/// Bazı ISA kesmeleri PC/AT standardından farklı GSI numaralarına veya
/// farklı tetikleme modlarına (kenar/seviye, aktif-yüksek/aktif-düşük) sahiptir.
/// Bu yapı bu farklılıkları kodlar.
#[derive(Clone, Debug)]
pub struct InterruptOverride {
    /// Kaynak veri yolu (genellikle 0 = ISA)
    pub bus: u8,
    /// Kaynak ISA IRQ numarası
    pub source: u8,
    /// Hedef Global System Interrupt numarası
    pub gsi: u32,
    /// Bayraklar: bit 0:1 = polarite, bit 2:3 = tetikleme modu
    pub flags: u16,
}

impl ApicInfo {
    /// Boş bir `ApicInfo` oluşturur — henüz MADT ayrıştırılmadan önce kullanılır.
    pub const fn empty() -> Self {
        Self {
            local_apic_address: 0,
            flags: 0,
            io_apics: Vec::new(),
            interrupt_overrides: Vec::new(),
        }
    }
}

/// `acpi` kütüphanesinin `Apic` yapısından `ApicInfo`'ya dönüştürme yapar.
///
/// I/O APIC'leri ve kesme geçiş kayıtlarını yineleyerek echOS formatına çevirir.
/// Polarite ve tetikleme modu bit alanlarına birleştirilerek `flags` alanına kodlanır.
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
