//! # echOS IRQ Chip + Domain Abstraction
//!
//! Linux `kernel/irq/chip.c` + `kernel/irq/irqdomain.c` karşılığı.
//! Birden fazla interrupt controller'ı tek bir soyut katman altında birleştirir.

use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// IrqChip Trait — Linux struct irq_chip
// ============================================================================

/// Interrupt controller soyutlaması.
/// Her interrupt controller (PIC, I/O APIC, MSI) bu trait'i implemente eder.
pub trait IrqChip: Send + Sync {
    /// Chip adı (debug için)
    fn name(&self) -> &'static str;

    /// Interrupt'ı onayla (acknowledge)
    fn irq_ack(&self, irq: u32);

    /// Interrupt'ı maskele (devre dışı bırak)
    fn irq_mask(&self, irq: u32);

    /// Interrupt maskesini kaldır (etkinleştir)
    fn irq_unmask(&self, irq: u32);

    /// EOI gönder
    fn irq_eoi(&self, irq: u32);

    /// Trigger tipi ayarla (edge/level)
    fn irq_set_type(&self, _irq: u32, _level_trigger: bool) -> bool {
        false // varsayılan: desteklenmiyor
    }

    /// IRQ affinite ayarla
    fn irq_set_affinity(&self, _irq: u32, _apic_id: u8) -> bool {
        false
    }

    /// IRQ'yu tamamen devre dışı bırak
    fn irq_disable(&self, irq: u32) {
        self.irq_mask(irq);
    }

    /// IRQ'yu etkinleştir
    fn irq_enable(&self, irq: u32) {
        self.irq_unmask(irq);
    }
}

// ============================================================================
// PIC Chip
// ============================================================================

/// 8259 PIC interrupt controller
pub struct PicChip;

impl IrqChip for PicChip {
    fn name(&self) -> &'static str {
        "8259-PIC"
    }

    fn irq_ack(&self, irq: u32) {
        unsafe {
            crate::interrupts::pic::PICS
                .lock()
                .notify_end_of_interrupt(irq as u8 + 32);
        }
    }

    fn irq_mask(&self, irq: u32) {
        // PIC mask register'a yaz
        let (port, bit) = if irq < 8 {
            (0x21u16, irq as u8)
        } else {
            (0xA1u16, (irq - 8) as u8)
        };
        unsafe {
            let mut p = x86_64::instructions::port::Port::<u8>::new(port);
            let val = p.read();
            p.write(val | (1 << bit));
        }
    }

    fn irq_unmask(&self, irq: u32) {
        let (port, bit) = if irq < 8 {
            (0x21u16, irq as u8)
        } else {
            (0xA1u16, (irq - 8) as u8)
        };
        unsafe {
            let mut p = x86_64::instructions::port::Port::<u8>::new(port);
            let val = p.read();
            p.write(val & !(1 << bit));
        }
    }

    fn irq_eoi(&self, irq: u32) {
        self.irq_ack(irq);
    }
}

// ============================================================================
// I/O APIC Chip
// ============================================================================

/// I/O APIC interrupt controller
pub struct IoApicChip;

impl IrqChip for IoApicChip {
    fn name(&self) -> &'static str {
        "IO-APIC"
    }

    fn irq_ack(&self, _irq: u32) {
        // I/O APIC edge-triggered: LAPIC EOI yeterli
        crate::apic::lapic::eoi();
    }

    fn irq_mask(&self, irq: u32) {
        crate::apic::ioapic::disable_irq(irq as u8);
    }

    fn irq_unmask(&self, irq: u32) {
        crate::apic::ioapic::enable_irq(irq as u8);
    }

    fn irq_eoi(&self, _irq: u32) {
        crate::apic::lapic::eoi();
    }

    fn irq_set_type(&self, irq: u32, level_trigger: bool) -> bool {
        crate::apic::ioapic::set_irq_trigger_mode(irq as u8, Some(level_trigger));
        true
    }

    fn irq_set_affinity(&self, irq: u32, apic_id: u8) -> bool {
        crate::apic::ioapic::set_irq_affinity(irq as u8, apic_id);
        true
    }
}

// ============================================================================
// MSI Chip
// ============================================================================

/// MSI/MSI-X interrupt controller (virtual)
pub struct MsiChip;

impl IrqChip for MsiChip {
    fn name(&self) -> &'static str {
        "MSI"
    }

    fn irq_ack(&self, _irq: u32) {
        // MSI: sadece LAPIC EOI
        crate::apic::lapic::eoi();
    }

    fn irq_mask(&self, _irq: u32) {
        // MSI mask: PCI config space üzerinden
        // TODO: PCI MSI mask bit
    }

    fn irq_unmask(&self, _irq: u32) {
        // TODO: PCI MSI unmask
    }

    fn irq_eoi(&self, _irq: u32) {
        crate::apic::lapic::eoi();
    }
}

// ============================================================================
// IRQ Domain — Linux irqdomain.c
// ============================================================================

/// IRQ domain — bir interrupt controller'ın yönettiği IRQ aralığı
pub struct IrqDomain {
    /// Domain adı
    pub name: &'static str,
    /// Yönetilen chip
    pub chip: &'static dyn IrqChip,
    /// HW IRQ → Linux IRQ mapping
    pub hwirq_base: u32,
    /// Toplam IRQ sayısı
    pub size: u32,
}

/// Global domain listesi
static IRQ_DOMAINS: Mutex<Vec<IrqDomain>> = Mutex::new(Vec::new());

/// Active chip (mevcut interrupt controller)
static ACTIVE_CHIP: Mutex<Option<&'static dyn IrqChip>> = Mutex::new(None);

/// Yeni domain kaydet
pub fn register_domain(domain: IrqDomain) {
    crate::serial_println!(
        "[IRQ-CHIP] Domain '{}' registered: chip={} hwirq_base={} size={}",
        domain.name,
        domain.chip.name(),
        domain.hwirq_base,
        domain.size
    );
    IRQ_DOMAINS.lock().push(domain);
}

/// Aktif chip'i set et
pub fn set_active_chip(chip: &'static dyn IrqChip) {
    *ACTIVE_CHIP.lock() = Some(chip);
    crate::serial_println!("[IRQ-CHIP] Active chip: {}", chip.name());
}

/// Aktif chip'ten EOI gönder
pub fn chip_eoi(irq: u32) {
    if let Some(chip) = *ACTIVE_CHIP.lock() {
        chip.irq_eoi(irq);
    }
}

/// Aktif chip'ten mask
pub fn chip_mask(irq: u32) {
    if let Some(chip) = *ACTIVE_CHIP.lock() {
        chip.irq_mask(irq);
    }
}

/// Aktif chip'ten unmask
pub fn chip_unmask(irq: u32) {
    if let Some(chip) = *ACTIVE_CHIP.lock() {
        chip.irq_unmask(irq);
    }
}

/// IRQ numarasından domain bul
pub fn find_domain_for_irq(irq: u32) -> Option<usize> {
    let domains = IRQ_DOMAINS.lock();
    for (i, domain) in domains.iter().enumerate() {
        if irq >= domain.hwirq_base && irq < domain.hwirq_base + domain.size {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// Statik chip instance'ları
// ============================================================================

/// Global PIC chip
pub static PIC_CHIP: PicChip = PicChip;
/// Global I/O APIC chip
pub static IOAPIC_CHIP: IoApicChip = IoApicChip;
/// Global MSI chip
pub static MSI_CHIP: MsiChip = MsiChip;

/// IRQ chip subsystemini başlat
pub fn init() {
    // I/O APIC varsa onu kullan, yoksa PIC
    if crate::interrupts::ioapic_enabled() {
        set_active_chip(&IOAPIC_CHIP);
        register_domain(IrqDomain {
            name: "ioapic0",
            chip: &IOAPIC_CHIP,
            hwirq_base: 0,
            size: 24,
        });
    } else {
        set_active_chip(&PIC_CHIP);
        register_domain(IrqDomain {
            name: "pic",
            chip: &PIC_CHIP,
            hwirq_base: 0,
            size: 16,
        });
    }

    // MSI domain her zaman kayıt ol
    register_domain(IrqDomain {
        name: "msi",
        chip: &MSI_CHIP,
        hwirq_base: 64,
        size: 192,
    });

    crate::serial_println!("[IRQ-CHIP] Subsystem initialized");
}
