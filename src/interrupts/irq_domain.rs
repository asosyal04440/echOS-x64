//! # IRQ Alanları
//!
//! Hiyerarşik IRQ alan yönetimi.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// IRQ ALAN SABİTLERİ
// ============================================================================

/// IRQ alan türleri
pub const IRQ_DOMAIN_TYPE_HIERARCHY: u32 = 0x01;
pub const IRQ_DOMAIN_TYPE_MSI: u32 = 0x02;
pub const IRQ_DOMAIN_TYPE_DMAR: u32 = 0x04;

/// IRQ bayrakları
pub const IRQ_TYPE_NONE: u32 = 0;
pub const IRQ_TYPE_EDGE_RISING: u32 = 1;
pub const IRQ_TYPE_EDGE_FALLING: u32 = 2;
pub const IRQ_TYPE_EDGE_BOTH: u32 = 3;
pub const IRQ_TYPE_LEVEL_HIGH: u32 = 4;
pub const IRQ_TYPE_LEVEL_LOW: u32 = 8;

/// IRQ durum bayrakları
pub const IRQ_IRQFLAGS_TRIGGER_MASK: u32 = 0x0F;
pub const IRQ_IRQFLAGS_PER_CPU: u32 = 0x10;
pub const IRQ_IRQFLAGS_NOAUTOEN: u32 = 0x20;

/// IRQ veri bayrakları
pub const IRQD_TRIGGER_MASK: u32 = 0x0F;
pub const IRQD_LEVEL: u32 = 0x10;
pub const IRQD_PER_CPU: u32 = 0x20;
pub const IRQD_NOAUTOEN: u32 = 0x40;
pub const IRQD_MOVE_PCNTXT: u32 = 0x80;
pub const IRQD_IRQ_DISABLED: u32 = 0x100;
pub const IRQD_IRQ_MASKED: u32 = 0x200;
pub const IRQD_IRQ_INPROGRESS: u32 = 0x400;
pub const IRQD_WAKEUP_STATE: u32 = 0x800;
pub const IRQD_AFFINITY_SET: u32 = 0x1000;

// ============================================================================
// IRQ VERİSİ
// ============================================================================

pub struct IrqData {
    /// IRQ numarası
    pub irq: u32,
    /// Donanım IRQ numarası
    pub hwirq: u32,
    /// Alan
    pub domain: AtomicU32,
    /// Durum bayrakları
    pub state_flags: AtomicU32,
    /// Tetikleyici türü
    pub trigger_type: AtomicU32,
    /// Benzeşim maskesi
    pub affinity: AtomicU32,
    /// Çip verisi
    pub chip_data: Mutex<Option<Arc<dyn ChipData>>>,
}

pub trait ChipData: Send + Sync {
    fn as_any(&self) -> &dyn core::any::Any;
}

impl IrqData {
    pub fn new(irq: u32, hwirq: u32) -> Self {
        Self {
            irq,
            hwirq,
            domain: AtomicU32::new(0),
            state_flags: AtomicU32::new(IRQD_IRQ_DISABLED),
            trigger_type: AtomicU32::new(IRQ_TYPE_NONE),
            affinity: AtomicU32::new(0xFFFFFFFF),
            chip_data: Mutex::new(None),
        }
    }

    /// IRQ devre dışı mı kontrol et
    pub fn is_disabled(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_DISABLED != 0
    }

    /// IRQ maskelenmiş mi kontrol et
    pub fn is_masked(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_MASKED != 0
    }

    /// IRQ işlemde mi kontrol et
    pub fn is_in_progress(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_INPROGRESS != 0
    }

    /// Devre dışı ayarla
    pub fn set_disabled(&self, disabled: bool) {
        if disabled {
            self.state_flags.fetch_or(IRQD_IRQ_DISABLED, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_DISABLED, Ordering::SeqCst);
        }
    }

    /// Maskelenmiş ayarla
    pub fn set_masked(&self, masked: bool) {
        if masked {
            self.state_flags.fetch_or(IRQD_IRQ_MASKED, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_MASKED, Ordering::SeqCst);
        }
    }

    /// İşlemde ayarla
    pub fn set_in_progress(&self, in_progress: bool) {
        if in_progress {
            self.state_flags.fetch_or(IRQD_IRQ_INPROGRESS, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_INPROGRESS, Ordering::SeqCst);
        }
    }

    /// Tetikleyici türü ayarla
    pub fn set_trigger_type(&self, trigger: u32) {
        self.trigger_type.store(trigger, Ordering::SeqCst);
    }

    /// Benzeşim ayarla
    pub fn set_affinity(&self, mask: u32) {
        self.affinity.store(mask, Ordering::SeqCst);
        self.state_flags.fetch_or(IRQD_AFFINITY_SET, Ordering::SeqCst);
    }
}

// ============================================================================
// IRQ ÇİPİ
// ============================================================================

pub struct IrqChip {
    /// Çip adı
    pub name: String,
    /// Kesmeyi onayla
    pub irq_ack: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Kesmeyi maskele
    pub irq_mask: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Kesme maskesini kaldır
    pub irq_unmask: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Kesmeyi etkinleştir
    pub irq_enable: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Kesmeyi devre dışı bırak
    pub irq_disable: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Benzeşim ayarla
    pub irq_set_affinity: Option<Arc<dyn Fn(&IrqData, u32) -> bool + Send + Sync>>,
    /// Tetikleyici türü ayarla
    pub irq_set_type: Option<Arc<dyn Fn(&IrqData, u32) -> i32 + Send + Sync>>,
    /// Uyanma ayarla
    pub irq_set_wake: Option<Arc<dyn Fn(&IrqData, bool) -> i32 + Send + Sync>>,
    /// EOI (Kesme Sonu)
    pub irq_eoi: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
}

impl IrqChip {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            irq_ack: None,
            irq_mask: None,
            irq_unmask: None,
            irq_enable: None,
            irq_disable: None,
            irq_set_affinity: None,
            irq_set_type: None,
            irq_set_wake: None,
            irq_eoi: None,
        }
    }

    /// Onayla
    pub fn ack(&self, data: &IrqData) {
        if let Some(ref ack) = self.irq_ack {
            ack(data);
        }
    }

    /// Maskele
    pub fn mask(&self, data: &IrqData) {
        if let Some(ref mask) = self.irq_mask {
            mask(data);
        }
        data.set_masked(true);
    }

    /// Maskesini kaldır
    pub fn unmask(&self, data: &IrqData) {
        if let Some(ref unmask) = self.irq_unmask {
            unmask(data);
        }
        data.set_masked(false);
    }

    /// Etkinleştir
    pub fn enable(&self, data: &IrqData) {
        if let Some(ref enable) = self.irq_enable {
            enable(data);
        }
        data.set_disabled(false);
    }

    /// Devre dışı bırak
    pub fn disable(&self, data: &IrqData) {
        if let Some(ref disable) = self.irq_disable {
            disable(data);
        }
        data.set_disabled(true);
    }

    /// Benzeşim ayarla
    pub fn set_affinity(&self, data: &IrqData, mask: u32) -> bool {
        if let Some(ref set_affinity) = self.irq_set_affinity {
            if set_affinity(data, mask) {
                data.set_affinity(mask);
                return true;
            }
        }
        false
    }

    /// Tetikleyici türü ayarla
    pub fn set_type(&self, data: &IrqData, trigger: u32) -> i32 {
        if let Some(ref set_type) = self.irq_set_type {
            let ret = set_type(data, trigger);
            if ret == 0 {
                data.set_trigger_type(trigger);
            }
            return ret;
        }
        -1
    }

    /// EOI
    pub fn eoi(&self, data: &IrqData) {
        if let Some(ref eoi) = self.irq_eoi {
            eoi(data);
        }
    }
}

// ============================================================================
// IRQ ALANI
// ============================================================================

pub struct IrqDomain {
    /// Alan kimliği
    pub id: u32,
    /// Alan adı
    pub name: String,
    /// Alan türü
    pub domain_type: u32,
    /// IRQ çipi
    pub chip: Mutex<Option<Arc<IrqChip>>>,
    /// Üst alan
    pub parent: AtomicU32,
    /// IRQ veri girdileri
    pub irq_data: Mutex<BTreeMap<u32, Arc<IrqData>>>,
    /// Ters eşleme (hwirq -> irq)
    pub hwirq_map: Mutex<BTreeMap<u32, u32>>,
    /// Sonraki IRQ numarası
    pub next_irq: AtomicU32,
    /// IRQ sayısı
    pub nr_irqs: AtomicU32,
    /// Aktif mi
    pub active: AtomicBool,
}

impl IrqDomain {
    pub fn new(id: u32, name: &str, domain_type: u32) -> Self {
        Self {
            id,
            name: String::from(name),
            domain_type,
            chip: Mutex::new(None),
            parent: AtomicU32::new(0),
            irq_data: Mutex::new(BTreeMap::new()),
            hwirq_map: Mutex::new(BTreeMap::new()),
            next_irq: AtomicU32::new(0),
            nr_irqs: AtomicU32::new(0),
            active: AtomicBool::new(true),
        }
    }

    /// Çip ayarla
    pub fn set_chip(&self, chip: Arc<IrqChip>) {
        *self.chip.lock() = Some(chip);
    }

    /// Üst alanı ayarla
    pub fn set_parent(&self, parent_id: u32) {
        self.parent.store(parent_id, Ordering::SeqCst);
    }

    /// Eşleme oluştur
    pub fn create_mapping(&self, hwirq: u32) -> u32 {
        // Zaten eşlenmiş mi kontrol et
        if let Some(&irq) = self.hwirq_map.lock().get(&hwirq) {
            return irq;
        }

        let irq = self.next_irq.fetch_add(1, Ordering::SeqCst);
        let data = Arc::new(IrqData::new(irq, hwirq));
        data.domain.store(self.id, Ordering::SeqCst);

        self.irq_data.lock().insert(irq, data.clone());
        self.hwirq_map.lock().insert(hwirq, irq);

        self.nr_irqs.fetch_add(1, Ordering::SeqCst);

        irq
    }

    /// IRQ verisini al
    pub fn get_irq_data(&self, irq: u32) -> Option<Arc<IrqData>> {
        self.irq_data.lock().get(&irq).cloned()
    }

    /// hwirq'a göre IRQ verisini al
    pub fn get_irq_data_by_hwirq(&self, hwirq: u32) -> Option<Arc<IrqData>> {
        let irq = self.hwirq_map.lock().get(&hwirq).copied()?;
        self.irq_data.lock().get(&irq).cloned()
    }

    /// IRQ'yu etkinleştir
    pub fn activate_irq(&self, irq: u32) -> bool {
        if let Some(data) = self.get_irq_data(irq) {
            data.set_disabled(false);
            return true;
        }
        false
    }

    /// IRQ'yu devre dışı bırak
    pub fn deactivate_irq(&self, irq: u32) {
        if let Some(data) = self.get_irq_data(irq) {
            data.set_disabled(true);
        }
    }

    /// IRQ'yu işle
    pub fn handle_irq(&self, irq: u32) {
        if let Some(data) = self.get_irq_data(irq) {
            if data.is_disabled() || data.is_masked() {
                return;
            }

            data.set_in_progress(true);

            // Çip al ve onayla
            if let Some(chip) = self.chip.lock().as_ref() {
                chip.ack(&data);
            }

            // Kesmeyi işle
            // Handler çağrılacak

            // EOI
            if let Some(chip) = self.chip.lock().as_ref() {
                chip.eoi(&data);
            }

            data.set_in_progress(false);
        }
    }
}

// ============================================================================
// IRQ ALAN YÖNETİCİSİ
// ============================================================================

pub struct IrqDomainManager {
    /// Alanlar
    pub domains: Mutex<BTreeMap<u32, Arc<IrqDomain>>>,
    /// Sonraki alan kimliği
    pub next_domain_id: AtomicU32,
    /// Doğrusal IRQ'dan alana eşleme
    pub irq_to_domain: Mutex<BTreeMap<u32, u32>>,
    /// İstatistikler
    pub stats: Mutex<IrqDomainStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IrqDomainStats {
    pub domains_count: u32,
    pub mappings_created: u64,
    pub irqs_handled: u64,
}

impl IrqDomainManager {
    pub const fn new() -> Self {
        Self {
            domains: Mutex::new(BTreeMap::new()),
            next_domain_id: AtomicU32::new(1),
            irq_to_domain: Mutex::new(BTreeMap::new()),
            stats: Mutex::new(IrqDomainStats::default()),
        }
    }

    /// Alan oluştur
    pub fn create_domain(&self, name: &str, domain_type: u32) -> Arc<IrqDomain> {
        let id = self.next_domain_id.fetch_add(1, Ordering::SeqCst);
        let domain = Arc::new(IrqDomain::new(id, name, domain_type));

        self.domains.lock().insert(id, domain.clone());

        let mut stats = self.stats.lock();
        stats.domains_count += 1;

        crate::serial_println!("[IRQ_DOMAIN] Created domain '{}' (id={})", name, id);

        domain
    }

    /// Alanı al
    pub fn get_domain(&self, id: u32) -> Option<Arc<IrqDomain>> {
        self.domains.lock().get(&id).cloned()
    }

    /// IRQ eşlemesi oluştur
    pub fn create_mapping(&self, domain_id: u32, hwirq: u32) -> Option<u32> {
        let domain = self.get_domain(domain_id)?;
        let irq = domain.create_mapping(hwirq);

        self.irq_to_domain.lock().insert(irq, domain_id);

        let mut stats = self.stats.lock();
        stats.mappings_created += 1;

        Some(irq)
    }

    /// IRQ verisini al
    pub fn get_irq_data(&self, irq: u32) -> Option<Arc<IrqData>> {
        let domain_id = self.irq_to_domain.lock().get(&irq).copied()?;
        let domain = self.get_domain(domain_id)?;
        domain.get_irq_data(irq)
    }

    /// IRQ'yu işle
    pub fn handle_irq(&self, irq: u32) {
        if let Some(domain_id) = self.irq_to_domain.lock().get(&irq).copied() {
            if let Some(domain) = self.get_domain(domain_id) {
                domain.handle_irq(irq);

                let mut stats = self.stats.lock();
                stats.irqs_handled += 1;
            }
        }
    }

    /// IRQ benzeşimi ayarla
    pub fn set_affinity(&self, irq: u32, mask: u32) -> bool {
        if let Some(data) = self.get_irq_data(irq) {
            let domain_id = data.domain.load(Ordering::Relaxed);
            if let Some(domain) = self.get_domain(domain_id) {
                if let Some(chip) = domain.chip.lock().as_ref() {
                    return chip.set_affinity(&data, mask);
                }
            }
        }
        false
    }

    /// IRQ tetikleyici türünü ayarla
    pub fn set_trigger_type(&self, irq: u32, trigger: u32) -> i32 {
        if let Some(data) = self.get_irq_data(irq) {
            let domain_id = data.domain.load(Ordering::Relaxed);
            if let Some(domain) = self.get_domain(domain_id) {
                if let Some(chip) = domain.chip.lock().as_ref() {
                    return chip.set_type(&data, trigger);
                }
            }
        }
        -1
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> IrqDomainStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref IRQ_DOMAINS: IrqDomainManager = IrqDomainManager::new();
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    // Kök alanı oluştur
    let root = IRQ_DOMAINS.create_domain("root", IRQ_DOMAIN_TYPE_HIERARCHY);

    // IOAPIC alanı oluştur
    let ioapic = IRQ_DOMAINS.create_domain("IOAPIC", IRQ_DOMAIN_TYPE_HIERARCHY);
    ioapic.set_parent(root.id);

    // MSI alanı oluştur
    let msi = IRQ_DOMAINS.create_domain("MSI", IRQ_DOMAIN_TYPE_MSI);
    msi.set_parent(root.id);

    crate::serial_println!("[IRQ_DOMAIN] IRQ domain manager initialized");
}
