//! # IRQ Domains
//!
//! Hierarchical IRQ domain management.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// IRQ DOMAIN CONSTANTS
// ============================================================================

/// IRQ domain types
pub const IRQ_DOMAIN_TYPE_HIERARCHY: u32 = 0x01;
pub const IRQ_DOMAIN_TYPE_MSI: u32 = 0x02;
pub const IRQ_DOMAIN_TYPE_DMAR: u32 = 0x04;

/// IRQ flags
pub const IRQ_TYPE_NONE: u32 = 0;
pub const IRQ_TYPE_EDGE_RISING: u32 = 1;
pub const IRQ_TYPE_EDGE_FALLING: u32 = 2;
pub const IRQ_TYPE_EDGE_BOTH: u32 = 3;
pub const IRQ_TYPE_LEVEL_HIGH: u32 = 4;
pub const IRQ_TYPE_LEVEL_LOW: u32 = 8;

/// IRQ status flags
pub const IRQ_IRQFLAGS_TRIGGER_MASK: u32 = 0x0F;
pub const IRQ_IRQFLAGS_PER_CPU: u32 = 0x10;
pub const IRQ_IRQFLAGS_NOAUTOEN: u32 = 0x20;

/// IRQ data flags
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
// IRQ DATA
// ============================================================================

pub struct IrqData {
    /// IRQ number
    pub irq: u32,
    /// Hardware IRQ number
    pub hwirq: u32,
    /// Domain
    pub domain: AtomicU32,
    /// State flags
    pub state_flags: AtomicU32,
    /// Trigger type
    pub trigger_type: AtomicU32,
    /// Affinity mask
    pub affinity: AtomicU32,
    /// Chip data
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

    /// Check if IRQ is disabled
    pub fn is_disabled(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_DISABLED != 0
    }

    /// Check if IRQ is masked
    pub fn is_masked(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_MASKED != 0
    }

    /// Check if IRQ is in progress
    pub fn is_in_progress(&self) -> bool {
        self.state_flags.load(Ordering::Relaxed) & IRQD_IRQ_INPROGRESS != 0
    }

    /// Set disabled
    pub fn set_disabled(&self, disabled: bool) {
        if disabled {
            self.state_flags.fetch_or(IRQD_IRQ_DISABLED, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_DISABLED, Ordering::SeqCst);
        }
    }

    /// Set masked
    pub fn set_masked(&self, masked: bool) {
        if masked {
            self.state_flags.fetch_or(IRQD_IRQ_MASKED, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_MASKED, Ordering::SeqCst);
        }
    }

    /// Set in progress
    pub fn set_in_progress(&self, in_progress: bool) {
        if in_progress {
            self.state_flags.fetch_or(IRQD_IRQ_INPROGRESS, Ordering::SeqCst);
        } else {
            self.state_flags.fetch_and(!IRQD_IRQ_INPROGRESS, Ordering::SeqCst);
        }
    }

    /// Set trigger type
    pub fn set_trigger_type(&self, trigger: u32) {
        self.trigger_type.store(trigger, Ordering::SeqCst);
    }

    /// Set affinity
    pub fn set_affinity(&self, mask: u32) {
        self.affinity.store(mask, Ordering::SeqCst);
        self.state_flags.fetch_or(IRQD_AFFINITY_SET, Ordering::SeqCst);
    }
}

// ============================================================================
// IRQ CHIP
// ============================================================================

pub struct IrqChip {
    /// Chip name
    pub name: String,
    /// Acknowledge interrupt
    pub irq_ack: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Mask interrupt
    pub irq_mask: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Unmask interrupt
    pub irq_unmask: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Enable interrupt
    pub irq_enable: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Disable interrupt
    pub irq_disable: Option<Arc<dyn Fn(&IrqData) + Send + Sync>>,
    /// Set affinity
    pub irq_set_affinity: Option<Arc<dyn Fn(&IrqData, u32) -> bool + Send + Sync>>,
    /// Set trigger type
    pub irq_set_type: Option<Arc<dyn Fn(&IrqData, u32) -> i32 + Send + Sync>>,
    /// Set wake
    pub irq_set_wake: Option<Arc<dyn Fn(&IrqData, bool) -> i32 + Send + Sync>>,
    /// EOI (End of Interrupt)
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

    /// Acknowledge
    pub fn ack(&self, data: &IrqData) {
        if let Some(ref ack) = self.irq_ack {
            ack(data);
        }
    }

    /// Mask
    pub fn mask(&self, data: &IrqData) {
        if let Some(ref mask) = self.irq_mask {
            mask(data);
        }
        data.set_masked(true);
    }

    /// Unmask
    pub fn unmask(&self, data: &IrqData) {
        if let Some(ref unmask) = self.irq_unmask {
            unmask(data);
        }
        data.set_masked(false);
    }

    /// Enable
    pub fn enable(&self, data: &IrqData) {
        if let Some(ref enable) = self.irq_enable {
            enable(data);
        }
        data.set_disabled(false);
    }

    /// Disable
    pub fn disable(&self, data: &IrqData) {
        if let Some(ref disable) = self.irq_disable {
            disable(data);
        }
        data.set_disabled(true);
    }

    /// Set affinity
    pub fn set_affinity(&self, data: &IrqData, mask: u32) -> bool {
        if let Some(ref set_affinity) = self.irq_set_affinity {
            if set_affinity(data, mask) {
                data.set_affinity(mask);
                return true;
            }
        }
        false
    }

    /// Set trigger type
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
// IRQ DOMAIN
// ============================================================================

pub struct IrqDomain {
    /// Domain ID
    pub id: u32,
    /// Domain name
    pub name: String,
    /// Domain type
    pub domain_type: u32,
    /// IRQ chip
    pub chip: Mutex<Option<Arc<IrqChip>>>,
    /// Parent domain
    pub parent: AtomicU32,
    /// IRQ data entries
    pub irq_data: Mutex<BTreeMap<u32, Arc<IrqData>>>,
    /// Reverse map (hwirq -> irq)
    pub hwirq_map: Mutex<BTreeMap<u32, u32>>,
    /// Next IRQ number
    pub next_irq: AtomicU32,
    /// Number of IRQs
    pub nr_irqs: AtomicU32,
    /// Is active
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

    /// Set chip
    pub fn set_chip(&self, chip: Arc<IrqChip>) {
        *self.chip.lock() = Some(chip);
    }

    /// Set parent domain
    pub fn set_parent(&self, parent_id: u32) {
        self.parent.store(parent_id, Ordering::SeqCst);
    }

    /// Create mapping
    pub fn create_mapping(&self, hwirq: u32) -> u32 {
        // Check if already mapped
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

    /// Get IRQ data
    pub fn get_irq_data(&self, irq: u32) -> Option<Arc<IrqData>> {
        self.irq_data.lock().get(&irq).cloned()
    }

    /// Get IRQ data by hwirq
    pub fn get_irq_data_by_hwirq(&self, hwirq: u32) -> Option<Arc<IrqData>> {
        let irq = self.hwirq_map.lock().get(&hwirq).copied()?;
        self.irq_data.lock().get(&irq).cloned()
    }

    /// Activate IRQ
    pub fn activate_irq(&self, irq: u32) -> bool {
        if let Some(data) = self.get_irq_data(irq) {
            data.set_disabled(false);
            return true;
        }
        false
    }

    /// Deactivate IRQ
    pub fn deactivate_irq(&self, irq: u32) {
        if let Some(data) = self.get_irq_data(irq) {
            data.set_disabled(true);
        }
    }

    /// Handle IRQ
    pub fn handle_irq(&self, irq: u32) {
        if let Some(data) = self.get_irq_data(irq) {
            if data.is_disabled() || data.is_masked() {
                return;
            }

            data.set_in_progress(true);

            // Get chip and acknowledge
            if let Some(chip) = self.chip.lock().as_ref() {
                chip.ack(&data);
            }

            // Handle the interrupt
            // Would call the handler

            // EOI
            if let Some(chip) = self.chip.lock().as_ref() {
                chip.eoi(&data);
            }

            data.set_in_progress(false);
        }
    }
}

// ============================================================================
// IRQ DOMAIN MANAGER
// ============================================================================

pub struct IrqDomainManager {
    /// Domains
    pub domains: Mutex<BTreeMap<u32, Arc<IrqDomain>>>,
    /// Next domain ID
    pub next_domain_id: AtomicU32,
    /// Linear IRQ to domain map
    pub irq_to_domain: Mutex<BTreeMap<u32, u32>>,
    /// Statistics
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

    /// Create domain
    pub fn create_domain(&self, name: &str, domain_type: u32) -> Arc<IrqDomain> {
        let id = self.next_domain_id.fetch_add(1, Ordering::SeqCst);
        let domain = Arc::new(IrqDomain::new(id, name, domain_type));

        self.domains.lock().insert(id, domain.clone());

        let mut stats = self.stats.lock();
        stats.domains_count += 1;

        crate::serial_println!("[IRQ_DOMAIN] Created domain '{}' (id={})", name, id);

        domain
    }

    /// Get domain
    pub fn get_domain(&self, id: u32) -> Option<Arc<IrqDomain>> {
        self.domains.lock().get(&id).cloned()
    }

    /// Create IRQ mapping
    pub fn create_mapping(&self, domain_id: u32, hwirq: u32) -> Option<u32> {
        let domain = self.get_domain(domain_id)?;
        let irq = domain.create_mapping(hwirq);

        self.irq_to_domain.lock().insert(irq, domain_id);

        let mut stats = self.stats.lock();
        stats.mappings_created += 1;

        Some(irq)
    }

    /// Get IRQ data
    pub fn get_irq_data(&self, irq: u32) -> Option<Arc<IrqData>> {
        let domain_id = self.irq_to_domain.lock().get(&irq).copied()?;
        let domain = self.get_domain(domain_id)?;
        domain.get_irq_data(irq)
    }

    /// Handle IRQ
    pub fn handle_irq(&self, irq: u32) {
        if let Some(domain_id) = self.irq_to_domain.lock().get(&irq).copied() {
            if let Some(domain) = self.get_domain(domain_id) {
                domain.handle_irq(irq);

                let mut stats = self.stats.lock();
                stats.irqs_handled += 1;
            }
        }
    }

    /// Set IRQ affinity
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

    /// Set IRQ trigger type
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

    /// Get statistics
    pub fn get_stats(&self) -> IrqDomainStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref IRQ_DOMAINS: IrqDomainManager = IrqDomainManager::new();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    // Create root domain
    let root = IRQ_DOMAINS.create_domain("root", IRQ_DOMAIN_TYPE_HIERARCHY);

    // Create IOAPIC domain
    let ioapic = IRQ_DOMAINS.create_domain("IOAPIC", IRQ_DOMAIN_TYPE_HIERARCHY);
    ioapic.set_parent(root.id);

    // Create MSI domain
    let msi = IRQ_DOMAINS.create_domain("MSI", IRQ_DOMAIN_TYPE_MSI);
    msi.set_parent(root.id);

    crate::serial_println!("[IRQ_DOMAIN] IRQ domain manager initialized");
}
