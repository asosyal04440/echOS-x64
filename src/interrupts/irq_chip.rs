//! # echOS IRQ Chip + Domain Abstraction
//!
//! Linux `kernel/irq/chip.c` + `kernel/irq/irqdomain.c` karşılığı.
//! Birden fazla interrupt controller'ı tek bir soyut katman altında birleştirir.
//!
//! ## Interrupt Controller Hiyerarşisi
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────┐
//!  │               IrqChip Trait (Soyut Katman)               │
//!  │        irq_ack | irq_mask | irq_unmask | irq_eoi         │
//!  └────────────┬──────────────────┬────────────┬─────────────┘
//!               │                  │            │
//!               ▼                  ▼            ▼
//!          PicChip            IoApicChip     MsiChip
//!        (8259 PIC)          (I/O APIC)    (PCI MSI/X)
//!          │                    │               │
//!          ▼                    ▼               ▼
//!     Port 0x20/0xA0     IOAPIC MMIO      LAPIC MSI Msg
//!     (master/slave)    redirect tbl     (addr+data yaz)
//! ```
//!
//! ## Neden Soyutlama?
//!
//! Eski PC sistemlerde 8259 PIC kullanılırken, modern sistemler
//! I/O APIC ve LAPIC ile çalışır. PCI Express aygıtları ise MSI
//! (Message Signaled Interrupts) kullanır. Bu trait sayesinde
//! üst katman hangisi olduğunu bilmeden interrupt yönetebilir.
//!
//! ## IRQ Yaşam Döngüsü
//!
//! ```text
//!  Aygıt sürücüsü               IrqChip             Donanım
//!  ──────────────               ───────              ──────
//!  request_irq(vector, handler) ──►  irq_unmask(irq) ──► IRQ hattı aktif
//!                                                        │
//!  interrupt gelir ◄─────────────────────────────────────┘
//!       │
//!       ▼
//!  handler(vector) çalışır
//!       │
//!       ▼
//!  irq_eoi(irq)  ──►  PIC/LAPIC EOI gönder
//! ```

use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// IrqChip Trait — Linux struct irq_chip karşılığı
//
// Her interrupt controller bu trait'i implemente eder.
// Trait metotlarının anlamları:
//   irq_ack()      : CPU, interrupt'ı aldığını onaylar ("acknowledge")
//   irq_mask()     : Controller seviyesinde IRQ'yu sustur (kapat)
//   irq_unmask()   : Controller seviyesinde IRQ'yu aç
//   irq_eoi()      : "End of Interrupt" — handler bitti, yeni interrupt alınabilir
//   irq_set_type() : Edge / Level tetikleme modunu ayarla
//   irq_set_affinity(): Hangi CPU'ya yönlendirilecek (SMP)
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
// PIC Chip — Intel 8259A Programmable Interrupt Controller
//
// İki adet 8259 çipten oluşur: Master (IRQ 0-7) ve Slave (IRQ 8-15).
// Slave, Master'ın IRQ2 hattına bağlıdır (cascade/zincir).
// Port adresleri:
//   Master: komut=0x20, veri(mask)=0x21
//   Slave:  komut=0xA0, veri(mask)=0xA1
// Bit maskesi: 1=maskelenmiş(kapalı), 0=aktif
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
// I/O APIC Chip — Advanced Programmable Interrupt Controller
//
// Modern x86_64 sistemlerin interrupt controller'ı.
// MMIO (Memory-Mapped I/O) üzerinden programlanır.
// Her IRQ için bir "redirection table entry" (RTE) bulunur:
//   RTE = {vektör, teslim modu, maskeleme, hedef APIC ID}
//
// I/O APIC edge-triggered interrupt'ları için LAPIC EOI yeterlidir.
// Level-triggered için I/O APIC'e ek olarak level clear gerekebilir.
//
// Avantajları (PIC'e göre):
//   • 24'e kadar IRQ (PIC: 15)
//   • SMP: Her IRQ farklı CPU'ya yönlendirilebilir (affinity)
//   • MSI ile birlikte kullanılabilir
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
// MSI Chip — Message Signaled Interrupts (Mesaj Tabanlı Kesmeler)
//
// MSI/MSI-X, PCI Express aygıtlarının geleneksel pin-tabanlı IRQ yerine
// bellek yazma işlemiyle (memory write) interrupt göndermesidir.
//
// MSI mesajı yapısı:
//   Hedef Adres: 0xFEEE_XXXX  (LAPIC MSI adres formatı)
//   Hedef Veri : {trigger, level, delivery_mode, vector}
//
// Avantajları:
//   • Pin paylaşımı yok (her aygıt kendi vektörüne sahip)
//   • MSI-X: Aygıt başına 2048'e kadar bağımsız kesme
//   • CPU affinity kolayca ayarlanır
//   • Level-triggered sorunları yok (hepsi edge)
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
// IRQ Domain — Linux irqdomain.c karşılığı
//
// IRQ Domain, bir interrupt controller'ın yönettiği IRQ aralığını
// ve bu aralığa ait chip'i bir arada tutan yapıdır.
//
// Örnek domain yapısı:
//   PIC domain   : hwirq_base=0, size=16, chip=PicChip
//   IOAPIC domain: hwirq_base=0, size=24, chip=IoApicChip
//   MSI domain   : hwirq_base=64, size=192, chip=MsiChip
//
// Domain'ler IRQ_DOMAINS listesinde tutulur. Bir IRQ numarası
// find_domain_for_irq() ile hangi domain'e ait olduğu bulunur.
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
// Statik chip instance'ları — 'static ömürlü global controller nesneleri
//
// Rust'ta trait nesneleri referans olarak tutulduğundan 'static gerekir.
// Bu instance'lar compile-time'da oluşturulur, heap allocator gerekmez.
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
