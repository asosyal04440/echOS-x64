//! # I/O APIC Sürücüsü
//!
//! I/O APIC (Input/Output Advanced Programmable Interrupt Controller),
//! donanım kesmelerini belirli CPU'lara yönlendiren MMIO tabanlı bir birimdir.
//! MADT tablosundan elde edilen I/O APIC bilgileriyle başlatılır; her IRQ için
//! redirection table girişleri (vektör, polarite, tetikleme modu, hedef CPU)
//! konfigüre edilir. Kesme kaynak geçersiz kılmaları (ISA override) da uygulanır.
//!
//! ## I/O APIC Genel Mimarisi
//!
//! ```text
//!  Donanım Cihazları          I/O APIC           CPU'lar (LAPIC'ler)
//!  ─────────────────          ────────           ──────────────────
//!  Klavye  (IRQ1) ──────────► [Giriş 1] ──┐
//!  Fare    (IRQ2) ──────────► [Giriş 2]   │    ┌─ CPU0 (LAPIC ID=0)
//!  USB     (IRQ3) ──────────► [Giriş 3]   ├───►├─ CPU1 (LAPIC ID=1)
//!  ...                        ...          │    └─ CPU2 (LAPIC ID=2)
//!  PCIe    (IRQn) ──────────► [Giriş n] ──┘
//!                             └──────────────────────────────────────
//!                              Her IRQ için Redirection Table girişi;
//!                              hedef CPU, vektör numarası, polarite ve
//!                              tetikleme modunu (edge/level) belirler.
//! ```
//!
//! ## Redirection Table Girişi (64-bit)
//!
//! ```text
//!  Bit 63..56  : Hedef APIC ID (hangi CPU?)
//!  Bit 16      : Maske (1=devre dışı, 0=etkin)
//!  Bit 15      : Tetikleme modu (1=level, 0=edge)
//!  Bit 13      : Polarite (1=aktif-düşük, 0=aktif-yüksek)
//!  Bit 10..8   : Teslim modu (000=fixed, 001=lowest prio, ...)
//!  Bit 7..0    : Kesme vektörü (CPU'nun IDT indeksi, 32-255 arası)
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

use crate::acpi::madt::{ApicInfo, InterruptOverride, IoApicInfo};
use crate::memory::{active_physical_offset, map_mmio};

/// I/O APIC versiyon kaydı — max redirection entry sayısını içerir
const IOAPIC_REG_VER: u8 = 0x01;
/// Redirection Table'ın başlangıç register ofseti (her giriş 2 register = 64-bit)
const IOAPIC_REG_REDTBL_BASE: u8 = 0x10;

/// Tek bir I/O APIC birimini temsil eder.
/// Sistemde birden fazla I/O APIC bulunabilir (farklı GSI aralıklarıyla).
struct IoApic {
    id: u8,
    gsi_base: u32,
    mmio_base: u64,
}

/// Tüm I/O APIC birimlerinin küresel durumu.
/// MADT tablosundan derlenen bilgiler burada tutulur.
struct IoApicState {
    ioapics: Vec<IoApic>,
    /// ISA veri yolu IRQ → GSI yönlendirme geçersiz kılmaları (ör. IRQ0 → GSI2)
    overrides: Vec<InterruptOverride>,
    /// BSP (Bootstrap Processor) APIC kimliği — varsayılan hedef CPU
    bsp_apic_id: u8,
    /// IRQ → hedef APIC ID affinite haritası (belirli IRQ'yu belirli CPU'ya yönlendir)
    affinity: BTreeMap<u8, u8>,
    /// IRQ → tetikleme modu geçersiz kılması (edge vs level)
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
    /// Yeni bir I/O APIC nesnesi oluşturur.
    /// MMIO adresi önce map_mmio ile eşleştirilir; başarısız olursa
    /// HHDM (Higher Half Direct Map) ofseti kullanılır.
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

    /// I/O APIC iç registerını okur.
    /// MMIO erişimi: önce IOREGSEL'e (offset 0x00) register no yaz,
    /// sonra IOWIN'den (offset 0x10) değeri oku.
    fn read_reg(&self, reg: u8) -> u32 {
        let reg_sel = self.mmio_base as *mut u32;
        let data = (self.mmio_base + 0x10) as *mut u32;
        unsafe {
            write_volatile(reg_sel, reg as u32);
            read_volatile(data)
        }
    }

    /// I/O APIC iç registerına yazar.
    /// Önce IOREGSEL'e register no yaz, sonra IOWIN'e değeri yaz.
    fn write_reg(&self, reg: u8, value: u32) {
        let reg_sel = self.mmio_base as *mut u32;
        let data = (self.mmio_base + 0x10) as *mut u32;
        unsafe {
            write_volatile(reg_sel, reg as u32);
            write_volatile(data, value);
        }
    }

    /// Bu I/O APIC'in desteklediği redirection entry sayısını döner.
    /// VER registerının bit 23..16 alanı: max_entry – 1.
    fn max_redirection_entries(&self) -> u32 {
        let ver = self.read_reg(IOAPIC_REG_VER);
        ((ver >> 16) & 0xFF) + 1
    }

    /// Belirtilen indeksteki redirection table girişini okur.
    /// Her giriş 64-bit olduğu için iki 32-bit register kullanılır (low, high).
    fn read_redirection(&self, index: u32) -> (u32, u32) {
        let reg = IOAPIC_REG_REDTBL_BASE + (index as u8 * 2);
        let low = self.read_reg(reg);
        let high = self.read_reg(reg + 1);
        (low, high)
    }

    /// Redirection table girişine low ve high 32-bit yarılarını yazar.
    fn write_redirection(&self, index: u32, low: u32, high: u32) {
        let reg = IOAPIC_REG_REDTBL_BASE + (index as u8 * 2);
        self.write_reg(reg, low);
        self.write_reg(reg + 1, high);
    }

    /// Bir redirection table girişinin tüm alanlarını ayarlar.
    ///
    /// - `index`        : Hangi IRQ girişi (0 tabanlı)
    /// - `vector`       : CPU'da tetiklenecek IDT vektörü (32–255)
    /// - `dest_apic_id` : Hedef CPU'nun APIC kimliği
    /// - `polarity_low` : Aktif-düşük polarite (true = düşük, false = yüksek)
    /// - `level_trigger`: Level tetikleme (true = level, false = edge)
    /// - `masked`       : Maske (true = devre dışı)
    fn set_redirection(
        &self,
        index: u32,
        vector: u8,
        dest_apic_id: u8,
        polarity_low: bool,
        level_trigger: bool,
        masked: bool,
    ) {
        if vector == 44 {
            crate::serial_println!(
                "[IOAPIC] Configuring IRQ12 (Vector 44): Index={} Dest={} Masked={}",
                index,
                dest_apic_id,
                masked
            );
        }
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

        // High 32-bit: Hedef APIC ID, bit 63..56 (yani bit 31..24 high word'de)
        let high = (dest_apic_id as u32) << 24;
        self.write_redirection(index, low, high);
    }

    /// Mevcut girişin yalnızca maske bitini değiştirir; diğer alanlar korunur.
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

/// I/O APIC alt sistemini başlatır.
/// MADT bilgilerinden I/O APIC'leri ve ISA override'larını yükler;
/// varsayılan olarak IRQ 0–15'i maskeli (devre dışı) şekilde yapılandırır.
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

    // IRQ 0–15'i maskeli yapılandır (sürücüler enable_irq ile açar)
    for irq in 0u8..=15 {
        configure_irq(&mut state, irq, true);
    }

    true
}

/// Belirtilen IRQ'yu etkinleştirir (maskeyi kaldırır).
pub fn enable_irq(irq: u8) {
    let mut state = IOAPIC_STATE.lock();
    configure_irq(&mut state, irq, false);
}

/// Belirtilen IRQ'yu devre dışı bırakır (maskeler).
pub fn disable_irq(irq: u8) {
    let mut state = IOAPIC_STATE.lock();
    configure_irq(&mut state, irq, true);
}

/// IRQ'yu belirli bir CPU'ya yönlendirir (CPU affinitesi ayarlar).
pub fn set_irq_affinity(irq: u8, apic_id: u8) {
    let mut state = IOAPIC_STATE.lock();
    state.affinity.insert(irq, apic_id);
    configure_irq(&mut state, irq, false);
}

/// IRQ'nun tetikleme modunu ayarlar.
/// `Some(true)` = level, `Some(false)` = edge, `None` = override'ı kaldır.
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

/// Bir IRQ için redirection table girişini tam olarak yapılandırır.
///
/// İşlem akışı:
/// 1. ISA override varsa GSI/polarite/tetikleme çözülür
/// 2. Manuel tetikleme geçersiz kılması varsa uygulanır
/// 3. Uygun I/O APIC ve giriş indeksi bulunur
/// 4. Vektör (IRQ + 32) ve hedef CPU belirlenir; giriş yazılır
fn configure_irq(state: &mut IoApicState, irq: u8, masked: bool) {
    let (gsi, polarity_low, mut level_trigger) = resolve_override(irq, &state.overrides);
    if let Some(override_trigger) = state.trigger_override.get(&irq) {
        level_trigger = *override_trigger;
    }
    if let Some((ioapic, index)) = find_ioapic_for_gsi(&mut state.ioapics, gsi) {
        // x86 ISR vektörleri 0–31 arasında ayrılmış; IRQ vektörleri 32'den başlar
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

/// Verilen GSI'yi hangi I/O APIC'in kapsadığını bulur.
/// Her I/O APIC belirli bir GSI aralığına sahiptir (gsi_base .. gsi_base + max).
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

/// ISA IRQ için MADT override tablosunu tarar.
/// MADT, bazı eski ISA IRQ'larının farklı GSI'ye yönlendirildiğini bildirir
/// (ör. ISA IRQ0 timer → GSI2; ISA IRQ8 RTC → GSI8).
/// Override yoksa varsayılan: GSI=IRQ, aktif-yüksek, edge tetikleme.
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
