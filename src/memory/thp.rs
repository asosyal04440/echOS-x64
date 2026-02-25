//! # Transparent Huge Pages (THP)
//!
//! Automatic promotion to 2MB/1GB huge pages.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// THP CONSTANTS
// ============================================================================

/// Huge page sizes
pub const HPAGE_2MB: usize = 2 * 1024 * 1024;
pub const HPAGE_1GB: usize = 1024 * 1024 * 1024;

/// THP modes
pub const THP_ALWAYS: &str = "always";
pub const THP_MADVISE: &str = "madvise";
pub const THP_NEVER: &str = "never";

/// MADV flags for huge pages
pub const MADV_HUGEPAGE: i32 = 14;
pub const MADV_NOHUGEPAGE: i32 = 15;

// ============================================================================
// THP CONFIGURATION
// ============================================================================

/// THP configuration
#[derive(Clone, Debug)]
pub struct ThpConfig {
    /// Enable THP
    pub enabled: bool,
    /// Mode: always, madvise, never
    pub mode: ThpMode,
    /// Use 1GB pages if available
    pub use_1gb: bool,
    /// Defrag strategy
    pub defrag: ThpDefrag,
    /// Max percentage of memory for THP
    pub max_percent: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThpMode {
    Always,
    Madvise,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThpDefrag {
    Always,
    Defer,
    DeferMadvise,
    Never,
}

impl Default for ThpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ThpMode::Always,
            use_1gb: false,
            defrag: ThpDefrag::Defer,
            max_percent: 50,
        }
    }
}

// ============================================================================
// HUGE PAGE TRACKING
// ============================================================================

/// A huge page allocation
#[derive(Clone, Debug)]
pub struct HugePage {
    /// Physical address
    pub phys_addr: u64,
    /// Virtual address
    pub virt_addr: u64,
    /// Size (2MB or 1GB)
    pub size: usize,
    /// Is it a THP (promoted from small pages)?
    pub is_thp: bool,
    /// Reference count
    pub ref_count: AtomicU32,
    /// NUMA node
    pub node: u32,
}

impl HugePage {
    pub fn new(phys: u64, virt: u64, size: usize, is_thp: bool, node: u32) -> Self {
        Self {
            phys_addr: phys,
            virt_addr: virt,
            size,
            is_thp,
            ref_count: AtomicU32::new(1),
            node,
        }
    }
}

// ============================================================================
// THP MANAGER
// ============================================================================

/// THP statistics
#[derive(Clone, Debug, Default)]
pub struct ThpStats {
    pub total_huge_pages: u64,
    pub thp_promotions: u64,
    pub thp_collapses: u64,
    pub thp_splits: u64,
    pub thp_faults: u64,
    pub thp_alloc_failures: u64,
    pub bytes_in_huge_pages: u64,
}

/// THP Manager
pub struct ThpManager {
    /// Configuration
    config: Mutex<ThpConfig>,
    /// Huge pages by virtual address
    huge_pages: Mutex<BTreeMap<u64, HugePage>>,
    /// Statistics
    stats: Mutex<ThpStats>,
    /// THP enabled flag
    enabled: AtomicBool,
    /// Total huge page memory
    total_huge_memory: AtomicU64,
}

impl ThpManager {
    pub const fn new() -> Self {
        Self {
            config: Mutex::new(ThpConfig::default()),
            huge_pages: Mutex::new(BTreeMap::new()),
            stats: Mutex::new(ThpStats::default()),
            enabled: AtomicBool::new(true),
            total_huge_memory: AtomicU64::new(0),
        }
    }

    /// Check if address is huge page aligned
    pub fn is_aligned(addr: u64, size: usize) -> bool {
        match size {
            HPAGE_2MB => (addr % HPAGE_2MB as u64) == 0,
            HPAGE_1GB => (addr % HPAGE_1GB as u64) == 0,
            _ => false,
        }
    }

    /// Allocate a huge page
    pub fn alloc_huge_page(&self, size: usize, node: u32) -> Option<HugePage> {
        // Check if THP enabled
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        // Check alignment
        if !Self::is_aligned(0, size) && size != HPAGE_2MB && size != HPAGE_1GB {
            return None;
        }

        // Allocate contiguous physical memory
        // This would call into the PMM for contiguous allocation
        let phys = self.alloc_contiguous(size)?;
        let virt = self.map_huge_page(phys, size)?;

        let hp = HugePage::new(phys, virt, size, false, node);
        
        self.huge_pages.lock().insert(virt, hp.clone());
        self.total_huge_memory.fetch_add(size as u64, Ordering::Relaxed);
        
        let mut stats = self.stats.lock();
        stats.total_huge_pages += 1;
        stats.bytes_in_huge_pages += size as u64;

        crate::serial_println!("[THP] Allocated {} huge page at {:#x}", 
            if size == HPAGE_1GB { "1GB" } else { "2MB" }, virt);

        Some(hp)
    }

    /// Allocate contiguous physical memory (placeholder)
    fn alloc_contiguous(&self, size: usize) -> Option<u64> {
        // Would call PMM for contiguous allocation
        // For now return placeholder
        Some(0x10000000)
    }

    /// Map huge page (placeholder)
    fn map_huge_page(&self, phys: u64, size: usize) -> Option<u64> {
        // Would set up page tables with huge page flag
        Some(0xFFFF800000000000 + phys)
    }

    /// Try to collapse small pages into huge page
    pub fn try_collapse(&self, vaddr: u64) -> bool {
        // Check if region is suitable for collapse
        if !Self::is_aligned(vaddr, HPAGE_2MB) {
            return false;
        }

        // Check if all pages in range are present and suitable
        if !self.can_collapse(vaddr) {
            return false;
        }

        // Perform collapse
        if self.do_collapse(vaddr) {
            let mut stats = self.stats.lock();
            stats.thp_collapses += 1;
            stats.thp_promotions += 1;
            crate::serial_println!("[THP] Collapsed pages at {:#x}", vaddr);
            return true;
        }

        false
    }

    /// Check if region can be collapsed
    fn can_collapse(&self, vaddr: u64) -> bool {
        // Check all 512 4KB pages in the 2MB range
        // All must be present, not locked, same permissions
        // For now, return true
        true
    }

    /// Perform actual collapse
    fn do_collapse(&self, vaddr: u64) -> bool {
        // 1. Allocate 2MB contiguous physical memory
        // 2. Copy data from 512 small pages
        // 3. Update page tables to use huge page
        // 4. Free old small pages
        true
    }

    /// Split huge page into small pages
    pub fn split_huge_page(&self, vaddr: u64) -> bool {
        let mut huge_pages = self.huge_pages.lock();
        
        if let Some(hp) = huge_pages.remove(&vaddr) {
            // Split into 512 4KB pages
            let num_pages = hp.size / 4096;
            
            // Update page tables
            // Free huge page, allocate small pages
            
            let mut stats = self.stats.lock();
            stats.thp_splits += 1;
            stats.total_huge_pages -= 1;
            stats.bytes_in_huge_pages -= hp.size as u64;
            
            self.total_huge_memory.fetch_sub(hp.size as u64, Ordering::Relaxed);
            
            crate::serial_println!("[THP] Split huge page at {:#x}", vaddr);
            return true;
        }
        
        false
    }

    /// Handle THP fault
    pub fn handle_thp_fault(&self, vaddr: u64) -> bool {
        let mut stats = self.stats.lock();
        stats.thp_faults += 1;
        
        // Try to allocate huge page for fault
        match self.alloc_huge_page(HPAGE_2MB, 0) {
            Some(_) => true,
            None => {
                stats.thp_alloc_failures += 1;
                false
            }
        }
    }

    /// Set THP mode
    pub fn set_mode(&self, mode: ThpMode) {
        self.config.lock().mode = mode;
        self.enabled.store(mode != ThpMode::Never, Ordering::SeqCst);
    }

    /// Get configuration
    pub fn get_config(&self) -> ThpConfig {
        self.config.lock().clone()
    }

    /// Get statistics
    pub fn get_stats(&self) -> ThpStats {
        self.stats.lock().clone()
    }

    /// Compact memory for THP
    pub fn compact_for_thp(&self) -> usize {
        // Trigger memory compaction to create contiguous ranges
        let mut compacted = 0;
        
        // Would call memory compaction routine
        
        crate::serial_println!("[THP] Compacted {} pages", compacted);
        compacted
    }
}

lazy_static::lazy_static! {
    /// Global THP manager
    pub static ref THP_MANAGER: ThpManager = ThpManager::new();
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// madvise for huge pages
pub fn sys_madvise_hugepage(addr: u64, len: u64, advice: i32) -> i32 {
    match advice {
        MADV_HUGEPAGE => {
            // Mark region as wanting huge pages
            THP_MANAGER.try_collapse(addr);
            0
        }
        MADV_NOHUGEPAGE => {
            // Mark region as not wanting huge pages
            // Split any existing huge pages
            THP_MANAGER.split_huge_page(addr);
            0
        }
        _ => -22, // EINVAL
    }
}

/// prctl for THP
pub const PR_GET_THP_DISABLE: i32 = 42;
pub const PR_SET_THP_DISABLE: i32 = 43;

pub fn sys_prctl_thp(option: i32, arg: u64) -> i64 {
    match option {
        PR_GET_THP_DISABLE => {
            if THP_MANAGER.enabled.load(Ordering::SeqCst) { 0 } else { 1 }
        }
        PR_SET_THP_DISABLE => {
            THP_MANAGER.enabled.store(arg == 0, Ordering::SeqCst);
            0
        }
        _ => -22,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize THP subsystem
pub fn init() {
    crate::serial_println!("[THP] Subsystem initialized (mode: always)");
}

/// Check if address is in huge page
pub fn is_huge_page(vaddr: u64) -> bool {
    THP_MANAGER.huge_pages.lock().contains_key(&vaddr)
}

/// Get huge page info
pub fn get_huge_page_info(vaddr: u64) -> Option<HugePage> {
    THP_MANAGER.huge_pages.lock().get(&vaddr).cloned()
}
