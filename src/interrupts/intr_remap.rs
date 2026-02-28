//! # Interrupt Remapping (Kesme Yönlendirme)
//!
//! Intel VT-d ve AMD-Vi interrupt remapping desteği.
//!
//! ## Interrupt Remapping Nedir?
//!
//! Interrupt Remapping (IR), DMA (Direct Memory Access) saldırılarına
//! karşı güvenlik sağlamak ve sanallaştırma ortamlarında interrupt
//! yönlendirmesini doğru yapmak için kullanılır.
//! Intel VT-d (Virtualization Technology for Directed I/O) özelliğinin
//! bir parçasıdır.
//!
//! Geleneksel interrupt akışı:
//! ```text
//!   PCI Aygıt ──► IOAPIC ──► LAPIC ──► CPU Exception Handler
//! ```
//!
//! Interrupt Remapping ile akış:
//! ```text
//!   PCI Aygıt ──► IOAPIC ──► DMAR Unit ──► IRT Lookup ──► LAPIC ──► CPU
//!                               (VT-d)       (IRTE)
//! ```
//!
//! ## IRTE (Interrupt Remapping Table Entry) Yapısı
//!
//! ```text
//!  127                                64 63                           0
//!  ┌──────────────────────────────────┬──────────────────────────────┐
//!  │  High: SID | SVT | SQ | DestID  │  Low: Vector|DM|TM|RH|FPD|P  │
//!  └──────────────────────────────────┴──────────────────────────────┘
//!    P   (bit  0): Present — giriş geçerli mi?
//!    FPD (bit  1): Fault Processing Disable
//!    DM  (bit  2): Delivery Mode (Fixed=0, LowestPriority=1)
//!    TM  (bit  4): Trigger Mode  (Edge=0, Level=1)
//!    RH  (bit  6): Redirection Hint
//!    Vector[15:8] : Hedef IDT vektörü
//!    DestID       : Hedef APIC ID (hangi CPU'ya gidecek)
//!    SVT          : Source Validation Type (0=none, 1=RID, 2=Bus)
//!    SID          : Source ID (PCI BDF — Bus:Device:Function)
//! ```
//!
//! ## VT-d MMIO Register Haritası
//!
//! ```text
//!  Offset  │ Register   │ Açıklama
//!  ────────┼────────────┼──────────────────────────────────
//!  0x00    │ VER        │ Versiyon
//!  0x08    │ CAP        │ Yetenekler (max page level, vb.)
//!  0x10    │ ECAP       │ Genişletilmiş Yetenekler (IR desteği)
//!  0x18    │ GCMD       │ Global Komut (translation/IR etkinleştir)
//!  0x1C    │ GSTS       │ Global Durum (komut tamamlandı mı?)
//!  0x20    │ RTADDR     │ Root Table Adres
//!  0x28    │ CCMD       │ Context Komutu
//!  0x34    │ FSTS       │ Hata Durumu
//!  0x38    │ FECTL      │ Hata Kontrolü
//!  0xB8    │ IRTA       │ Interrupt Remap Table Adres + boyut
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// INTR REMAPPING CONSTANTS — VT-d MMIO Register offsetleri ve flag bitleri
//
// Intel VT-d spesifikasyonu (bkz. Intel VT-d Architecture Spec Rev 3.x)
// tüm sabitler için kaynak: IOMMU Spec Tablo 10-1 ve 10-2
// ============================================================================

/// Intel VT-d registers
pub const VTD_VER_REG: u32 = 0x00;
pub const VTD_CAP_REG: u32 = 0x08;
pub const VTD_ECAP_REG: u32 = 0x10;
pub const VTD_GCMD_REG: u32 = 0x18;
pub const VTD_GSTS_REG: u32 = 0x1C;
pub const VTD_RTADDR_REG: u32 = 0x20;
pub const VTD_CCMD_REG: u32 = 0x28;
pub const VTD_FSTS_REG: u32 = 0x34;
pub const VTD_FECTL_REG: u32 = 0x38;
pub const VTD_FEDATA_REG: u32 = 0x3C;
pub const VTD_FEADDR_REG: u32 = 0x40;
pub const VTD_FEUADDR_REG: u32 = 0x44;
pub const VTD_AFLOG_REG: u32 = 0x58;
pub const VTD_IVA_REG: u32 = 0x60;
pub const VTD_IRTA_REG: u32 = 0xB8;

/// VT-d capability flags
pub const VTD_CAP_RWBF: u64 = 1 << 4;      // Required Write-Buffer Flushing
pub const VTD_CAP_AFL: u64 = 1 << 3;        // Advanced Fault Logging
pub const VTD_CAP_MGAW_MASK: u64 = 0x3F << 16; // Maximum Guest Address Width
pub const VTD_CAP_SAGAW_MASK: u64 = 0x1F << 8;  // Supported Adjusted Guest Address Width

/// VT-d extended capability flags
pub const VTD_ECAP_IR: u64 = 1 << 3;        // Interrupt Remapping
pub const VTD_ECAP_EIM: u64 = 1 << 4;       // Extended Interrupt Mode
pub const VTD_ECAP_DT: u64 = 1 << 2;        // Device-TLBs

/// Global command register bits
pub const VTD_GCMD_TE: u32 = 1 << 31;       // Translation Enable
pub const VTD_GCMD_SRTP: u32 = 1 << 30;     // Set Root Table Pointer
pub const VTD_GCMD_SFL: u32 = 1 << 29;     // Set Fault Log
pub const VTD_GCMD_EAFL: u32 = 1 << 28;    // Enable Advanced Fault Log
pub const VTD_GCMD_WBF: u32 = 1 << 27;     // Write Buffer Flush
pub const VTD_GCMD_IRE: u32 = 1 << 25;     // Interrupt Remapping Enable
pub const VTD_GCMD_SIRTP: u32 = 1 << 24;  // Set Interrupt Remap Table Pointer

/// Global status register bits
pub const VTD_GSTS_TES: u32 = 1 << 31;
pub const VTD_GSTS_RTPS: u32 = 1 << 30;
pub const VTD_GSTS_FLS: u32 = 1 << 29;
pub const VTD_GSTS_AFLS: u32 = 1 << 28;
pub const VTD_GSTS_WBFS: u32 = 1 << 27;
pub const VTD_GSTS_IRES: u32 = 1 << 25;
pub const VTD_GSTS_IRTPS: u32 = 1 << 24;

/// IRTE (Interrupt Remapping Table Entry) size
pub const IRTE_SIZE: usize = 16;

/// IRTE flags
pub const IRTE_P: u64 = 1 << 0;             // Present
pub const IRTE_FPD: u64 = 1 << 1;           // Fault Processing Disable
pub const IRTE_DM: u64 = 1 << 2;            // Delivery Mode
pub const IRTE_TM: u64 = 1 << 4;            // Trigger Mode
pub const IRTE_RH: u64 = 1 << 6;            // Redirection Hint

/// Source validation types
pub const SVT_NONE: u8 = 0;
pub const SVT_RID: u8 = 1;
pub const SVT_BUS: u8 = 2;

// ============================================================================
// IRTE (Interrupt Remapping Table Entry) — 16 byte'lık tablo girişi
//
// Her IRTE, bir interrupt kaynağını (PCI aygıtı) belirli bir CPU ve
// vektöre yönlendirir. IntrRemapTable içinde dizi olarak tutulur.
// VT-d donanımı bu tabloyu MMIO aracılığıyla okur.
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Irte {
    pub low: u64,
    pub high: u64,
}

impl Irte {
    pub fn new() -> Self {
        Self { low: 0, high: 0 }
    }

    /// Set present
    pub fn set_present(&mut self, present: bool) {
        if present {
            self.low |= IRTE_P;
        } else {
            self.low &= !IRTE_P;
        }
    }

    /// Set vector
    pub fn set_vector(&mut self, vector: u8) {
        self.low = (self.low & !0xFF00) | ((vector as u64) << 8);
    }

    /// Set delivery mode
    pub fn set_delivery_mode(&mut self, mode: u8) {
        self.low = (self.low & !(0x7 << 5)) | ((mode as u64) << 5);
    }

    /// Set trigger mode (0=edge, 1=level)
    pub fn set_trigger_mode(&mut self, level: bool) {
        if level {
            self.low |= IRTE_TM;
        } else {
            self.low &= !IRTE_TM;
        }
    }

    /// Set destination ID
    pub fn set_dest_id(&mut self, dest: u32) {
        self.high = (self.high & !0xFFFF) | (dest as u64);
    }

    /// Set source validation
    pub fn set_source(&mut self, svt: u8, sid: u16, sq: u8) {
        let val = ((svt as u64) << 18) | ((sid as u64) << 32) | ((sq as u64) << 17);
        self.high = (self.high & !(0x3FFFF << 17)) | val;
    }

    /// Set interrupt remap handle (for posted interrupts)
    pub fn set_ir_handle(&mut self, handle: u64) {
        self.high = (self.high & !0xFFFF0000) | (handle << 16);
    }
}

// ============================================================================
// INTERRUPT REMAP TABLE — IRTE girişlerini tutan bellek yapısı
//
// Fiziksel adresi IRTA register'a yazılır. Boyutu 2'nin kuvveti
// olmalıdır (ör: 256 giriş = 4KB). VT-d donanımı interrupt geldiğinde
// bu tabloya bakarak hangi CPU'ya, hangi vektörle göndereceğine karar verir.
// ============================================================================

pub struct IntrRemapTable {
    /// Table entries
    pub entries: Mutex<Vec<Irte>>,
    /// Physical address
    pub phys_addr: AtomicU64,
    /// Size (number of entries)
    pub size: usize,
}

impl IntrRemapTable {
    pub fn new(size: usize) -> Self {
        let mut entries = Vec::with_capacity(size);
        for _ in 0..size {
            entries.push(Irte::new());
        }

        Self {
            entries: Mutex::new(entries),
            phys_addr: AtomicU64::new(0),
            size,
        }
    }

    /// Get entry
    pub fn get_entry(&self, index: usize) -> Option<Irte> {
        if index < self.size {
            self.entries.lock().get(index).copied()
        } else {
            None
        }
    }

    /// Set entry
    pub fn set_entry(&self, index: usize, entry: Irte) -> Result<(), IrError> {
        if index >= self.size {
            return Err(IrError::InvalidIndex);
        }

        self.entries.lock()[index] = entry;
        Ok(())
    }

    /// Allocate free entry
    pub fn allocate_entry(&self) -> Option<usize> {
        let mut entries = self.entries.lock();

        for (i, entry) in entries.iter().enumerate() {
            if entry.low & IRTE_P == 0 {
                return Some(i);
            }
        }

        None
    }
}

// ============================================================================
// INTERRUPT REMAPPING UNIT — Tek bir VT-d donanım birimini temsil eder
//
// Sunucuda birden fazla VT-d birimi olabilir (her PCIe root complex için bir).
// Her birim kendi MMIO alanına, IRT'sine ve hata kuyruğuna sahiptir.
//
// Hayat döngüsü:
//   1. new(id, base_addr)  — Birim nesnesi oluştur
//   2. init()              — CAP/ECAP register'larını oku, IR destekli mi?
//   3. create_irt(size)    — IRT allocate et, IRTA register'a yaz
//   4. enable()            — GCMD.SIRTP → GCMD.IRE → GSTS ile onayla
//   5. program_interrupt() — IRTE'yi doldur (vektör, dest, trigger)
//   6. handle_fault()      — FSTS kontrol et, hata kaydı oluştur
// ============================================================================

pub struct IntrRemapUnit {
    /// Unit ID
    pub id: u32,
    /// Base address (MMIO)
    pub base_addr: u64,
    /// Capability register
    pub cap: AtomicU64,
    /// Extended capability register
    pub ecap: AtomicU64,
    /// Interrupt remap table
    pub irt: Mutex<Option<Arc<IntrRemapTable>>>,
    /// Is enabled
    pub enabled: AtomicBool,
    /// Fault queue
    pub fault_queue: Mutex<Vec<FaultRecord>>,
}

#[derive(Clone, Debug)]
pub struct FaultRecord {
    pub fault_reason: u8,
    pub source_id: u16,
    pub domain_id: u16,
    pub address: u64,
    pub timestamp: u64,
}

impl IntrRemapUnit {
    pub fn new(id: u32, base_addr: u64) -> Self {
        Self {
            id,
            base_addr,
            cap: AtomicU64::new(0),
            ecap: AtomicU64::new(0),
            irt: Mutex::new(None),
            enabled: AtomicBool::new(false),
            fault_queue: Mutex::new(Vec::new()),
        }
    }

    /// Initialize unit
    pub fn init(&self) -> Result<(), IrError> {
        // Read capabilities
        let cap = self.read_reg(VTD_CAP_REG);
        let ecap = self.read_reg(VTD_ECAP_REG);

        self.cap.store(cap, Ordering::SeqCst);
        self.ecap.store(ecap, Ordering::SeqCst);

        // Check if interrupt remapping is supported
        if ecap & VTD_ECAP_IR == 0 {
            return Err(IrError::NotSupported);
        }

        crate::serial_println!("[IR] Unit {} initialized at {:#x}", self.id, self.base_addr);

        Ok(())
    }

    /// Create interrupt remap table
    pub fn create_irt(&self, size: usize) -> Arc<IntrRemapTable> {
        let irt = Arc::new(IntrRemapTable::new(size));

        // Set table pointer
        let addr = irt.phys_addr.load(Ordering::SeqCst);
        let irta = addr | (size.trailing_zeros() as u64);

        self.write_reg(VTD_IRTA_REG, irta);

        *self.irt.lock() = Some(irt.clone());

        irt
    }

    /// Enable interrupt remapping
    pub fn enable(&self) -> Result<(), IrError> {
        // Set interrupt remap table pointer first
        self.write_reg(VTD_GCMD_REG, VTD_GCMD_SIRTP);

        // Wait for completion
        self.wait_status(VTD_GSTS_IRTPS);

        // Enable interrupt remapping
        self.write_reg(VTD_GCMD_REG, VTD_GCMD_IRE);

        // Wait for enable
        self.wait_status(VTD_GSTS_IRES);

        self.enabled.store(true, Ordering::SeqCst);

        crate::serial_println!("[IR] Interrupt remapping enabled for unit {}", self.id);

        Ok(())
    }

    /// Disable interrupt remapping
    pub fn disable(&self) {
        self.write_reg(VTD_GCMD_REG, 0);
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Program interrupt
    pub fn program_interrupt(&self, index: usize, vector: u8, dest: u32, trigger: bool) -> Result<(), IrError> {
        let irt = self.irt.lock();
        let table = irt.as_ref().ok_or(IrError::TableNotSet)?;

        let mut entry = Irte::new();
        entry.set_present(true);
        entry.set_vector(vector);
        entry.set_trigger_mode(trigger);
        entry.set_dest_id(dest);
        entry.set_source(SVT_NONE, 0, 0);

        table.set_entry(index, entry)?;

        Ok(())
    }

    /// Handle fault
    pub fn handle_fault(&self) -> Option<FaultRecord> {
        let fsts = self.read_reg(VTD_FSTS_REG) as u32;

        if fsts & 0x80000000 != 0 {
            // Fault pending
            let reason = ((fsts >> 1) & 0xFF) as u8;
            let source = ((fsts >> 9) & 0xFFFF) as u16;

            let record = FaultRecord {
                fault_reason: reason,
                source_id: source,
                domain_id: 0,
                address: 0,
                timestamp: crate::task::scheduler::get_ticks(),
            };

            // Clear fault
            self.write_reg(VTD_FSTS_REG, 0xFFFFFFFF);

            self.fault_queue.lock().push(record.clone());

            return Some(record);
        }

        None
    }

    /// Read register
    fn read_reg(&self, offset: u32) -> u64 {
        // unsafe {
        //     core::ptr::read_volatile((self.base_addr + offset as u64) as *const u64)
        // }
        0
    }

    /// Write register
    fn write_reg(&self, offset: u32, value: u64) {
        // unsafe {
        //     core::ptr::write_volatile((self.base_addr + offset as u64) as *mut u64, value);
        // }
    }

    /// Wait for status bit
    fn wait_status(&self, bit: u32) {
        // for _ in 0..1000 {
        //     let status = self.read_reg(VTD_GSTS_REG) as u32;
        //     if status & bit != 0 {
        //         return;
        //     }
        // }
    }
}

// ============================================================================
// INTERRUPT REMAPPING MANAGER — Tüm VT-d birimlerini yönetir
//
// Singleton (global INTR_REMAP) olarak kullanılır.
// Birden fazla VT-d birimini BTreeMap ile ID→Arc<Unit> eşleşmesiyle tutar.
// İşletim sistemi yeni bir PCI aygıtı eklendiğinde buraya başvurur:
//
//   INTR_REMAP.register_unit(id, base)   → Yeni VT-d birimini kaydet
//   INTR_REMAP.map_interrupt(...)        → Aygıt için IRTE tahsis et
//   INTR_REMAP.handle_faults()           → Hataları işle (periyodik)
// ============================================================================

pub struct IntrRemapManager {
    /// Remapping units
    pub units: Mutex<BTreeMap<u32, Arc<IntrRemapUnit>>>,
    /// Global interrupt index allocator
    pub next_index: AtomicU32,
    /// Statistics
    pub stats: Mutex<IrStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IrStats {
    pub interrupts_mapped: u64,
    pub faults_handled: u64,
}

impl IntrRemapManager {
    pub const fn new() -> Self {
        Self {
            units: Mutex::new(BTreeMap::new()),
            next_index: AtomicU32::new(0),
            stats: Mutex::new(IrStats::default()),
        }
    }

    /// Register unit
    pub fn register_unit(&self, id: u32, base_addr: u64) -> Result<Arc<IntrRemapUnit>, IrError> {
        let unit = Arc::new(IntrRemapUnit::new(id, base_addr));
        unit.init()?;

        self.units.lock().insert(id, unit.clone());

        Ok(unit)
    }

    /// Get unit
    pub fn get_unit(&self, id: u32) -> Option<Arc<IntrRemapUnit>> {
        self.units.lock().get(&id).cloned()
    }

    /// Allocate interrupt index
    pub fn allocate_index(&self) -> u32 {
        self.next_index.fetch_add(1, Ordering::SeqCst)
    }

    /// Map interrupt
    pub fn map_interrupt(&self, unit_id: u32, vector: u8, dest: u32, trigger: bool) -> Result<u32, IrError> {
        let unit = self.get_unit(unit_id).ok_or(IrError::UnitNotFound)?;

        let index = self.allocate_index();
        unit.program_interrupt(index as usize, vector, dest, trigger)?;

        let mut stats = self.stats.lock();
        stats.interrupts_mapped += 1;

        Ok(index)
    }

    /// Handle faults
    pub fn handle_faults(&self) {
        for unit in self.units.lock().values() {
            while let Some(_fault) = unit.handle_fault() {
                let mut stats = self.stats.lock();
                stats.faults_handled += 1;
            }
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> IrStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref INTR_REMAP: IntrRemapManager = IntrRemapManager::new();
}

// ============================================================================
// ERROR TYPE — VT-d işlemlerinden dönebilecek hata türleri
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrError {
    NotSupported,
    UnitNotFound,
    TableNotSet,
    InvalidIndex,
    TableFull,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[IR] Interrupt remapping manager initialized");
}
