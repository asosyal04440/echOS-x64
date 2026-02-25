//! # Power Management S3/S4
//!
//! Suspend to RAM (S3) and Suspend to Disk (S4) support.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SLEEP STATE CONSTANTS
// ============================================================================

/// Sleep states
pub const ACPI_STATE_S0: u8 = 0;  // Working
pub const ACPI_STATE_S1: u8 = 1;  // Sleeping (Processor Context Maintained)
pub const ACPI_STATE_S2: u8 = 2;  // Sleeping (Processor Context Lost)
pub const ACPI_STATE_S3: u8 = 3;  // Suspend to RAM
pub const ACPI_STATE_S4: u8 = 4;  // Suspend to Disk
pub const ACPI_STATE_S5: u8 = 5;  // Soft Off

/// Sleep type values (from FADT)
pub const SLEEP_TYPE_S0: u8 = 0;
pub const SLEEP_TYPE_S1: u8 = 1;
pub const SLEEP_TYPE_S2: u8 = 2;
pub const SLEEP_TYPE_S3: u8 = 3;
pub const SLEEP_TYPE_S4: u8 = 4;
pub const SLEEP_TYPE_S5: u8 = 5;

/// PM1 control register bits
pub const PM1_SLP_TYP_SHIFT: u16 = 10;
pub const PM1_SLP_EN: u16 = 0x2000;
pub const PM1_SLP_TYP_MASK: u16 = 0x1C00;

/// PM1 status register bits
pub const PM1_WAK_STS: u16 = 0x8000;
pub const PM1_PWRBTN_STS: u16 = 0x0100;
pub const PM1_RTC_STS: u16 = 0x0400;

// ============================================================================
// SLEEP STATE INFO
// ============================================================================

#[derive(Clone, Debug)]
pub struct SleepStateInfo {
    /// State number
    pub state: u8,
    /// Sleep type A
    pub sleep_type_a: u8,
    /// Sleep type B
    pub sleep_type_b: u8,
    /// Supported
    pub supported: bool,
    /// Wake vector address
    pub wake_vector: u64,
    /// Wake vector for S3
    pub wake_vector_s3: u64,
    /// Wake vector for S4
    pub wake_vector_s4: u64,
}

impl SleepStateInfo {
    pub fn new(state: u8) -> Self {
        Self {
            state,
            sleep_type_a: 0,
            sleep_type_b: 0,
            supported: false,
            wake_vector: 0,
            wake_vector_s3: 0,
            wake_vector_s4: 0,
        }
    }
}

// ============================================================================
// SUSPEND CONTEXT
// ============================================================================

#[derive(Clone, Debug)]
pub struct SuspendContext {
    /// Processor state
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub efer: u64,
    /// General purpose registers
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    /// RIP
    pub rip: u64,
    pub rflags: u64,
    /// IDT
    pub idtr: (u64, u16),
    /// GDT
    pub gdtr: (u64, u16),
    /// Page tables
    pub pml4: u64,
    /// LAPIC state
    pub lapic_id: u32,
    pub lapic_timer: u64,
    /// FPU/SSE state
    pub fpu_state: [u8; 512],
}

impl SuspendContext {
    pub fn new() -> Self {
        Self {
            cr0: 0, cr2: 0, cr3: 0, cr4: 0, efer: 0,
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0, rsp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            rip: 0, rflags: 0,
            idtr: (0, 0),
            gdtr: (0, 0),
            pml4: 0,
            lapic_id: 0,
            lapic_timer: 0,
            fpu_state: [0u8; 512],
        }
    }

    /// Save current state
    pub fn save(&mut self) {
        // Would save actual CPU state
        // unsafe {
        //     core::arch::asm!(
        //         "mov {0}, cr0",
        //         "mov {1}, cr3",
        //         "mov {2}, cr4",
        //         out(reg) self.cr0,
        //         out(reg) self.cr3,
        //         out(reg) self.cr4,
        //     );
        // }
    }

    /// Restore saved state
    pub fn restore(&self) {
        // Would restore actual CPU state
    }
}

// ============================================================================
// SWAP HEADER (for S4)
// ============================================================================

#[repr(C)]
#[derive(Clone, Debug)]
pub struct SwapHeader {
    /// Magic
    pub magic: u32,        // "SUSP"
    /// Version
    pub version: u32,
    /// Image size
    pub image_size: u64,
    /// Page count
    pub page_count: u64,
    /// Checksum
    pub checksum: u32,
    /// Timestamp
    pub timestamp: u64,
    /// Resume device
    pub resume_device: u64,
    /// Original boot parameters
    pub boot_params: [u8; 4096],
}

impl SwapHeader {
    pub fn new() -> Self {
        Self {
            magic: 0x50535553, // "SUSP"
            version: 1,
            image_size: 0,
            page_count: 0,
            checksum: 0,
            timestamp: 0,
            resume_device: 0,
            boot_params: [0u8; 4096],
        }
    }
}

// ============================================================================
// POWER STATE MANAGER
// ============================================================================

pub struct PowerStateManager {
    /// Sleep state info
    pub sleep_states: Mutex<BTreeMap<u8, SleepStateInfo>>,
    /// PM1a control block address
    pub pm1a_cnt_blk: AtomicU64,
    /// PM1b control block address
    pub pm1b_cnt_blk: AtomicU64,
    /// PM1a status block address
    pub pm1a_evt_blk: AtomicU64,
    /// PM1b status block address
    pub pm1b_evt_blk: AtomicU64,
    /// Suspend context for each CPU
    pub suspend_contexts: Mutex<Vec<SuspendContext>>,
    /// Is suspended
    pub suspended: AtomicBool,
    /// Current state
    pub current_state: AtomicU32,
    /// Swap device for S4
    pub swap_device: Mutex<Option<String>>,
    /// Statistics
    pub stats: Mutex<PmStats>,
}

#[derive(Clone, Debug, Default)]
pub struct PmStats {
    pub s3_entries: u64,
    pub s3_exits: u64,
    pub s4_entries: u64,
    pub s4_exits: u64,
    pub wake_events: u64,
}

impl PowerStateManager {
    pub const fn new() -> Self {
        Self {
            sleep_states: Mutex::new(BTreeMap::new()),
            pm1a_cnt_blk: AtomicU64::new(0),
            pm1b_cnt_blk: AtomicU64::new(0),
            pm1a_evt_blk: AtomicU64::new(0),
            pm1b_evt_blk: AtomicU64::new(0),
            suspend_contexts: Mutex::new(Vec::new()),
            suspended: AtomicBool::new(false),
            current_state: AtomicU32::new(ACPI_STATE_S0 as u32),
            swap_device: Mutex::new(None),
            stats: Mutex::new(PmStats::default()),
        }
    }

    /// Initialize from FADT
    pub fn init(&self, pm1a_cnt: u64, pm1b_cnt: u64, pm1a_evt: u64, pm1b_evt: u64) {
        self.pm1a_cnt_blk.store(pm1a_cnt, Ordering::SeqCst);
        self.pm1b_cnt_blk.store(pm1b_cnt, Ordering::SeqCst);
        self.pm1a_evt_blk.store(pm1a_evt, Ordering::SeqCst);
        self.pm1b_evt_blk.store(pm1b_evt, Ordering::SeqCst);

        // Initialize sleep states
        let mut states = self.sleep_states.lock();
        for i in 1..=5 {
            states.insert(i, SleepStateInfo::new(i));
        }

        // Mark supported states (would read from FADT)
        if let Some(s3) = states.get_mut(&3) {
            s3.supported = true;
        }
        if let Some(s4) = states.get_mut(&4) {
            s4.supported = true;
        }
        if let Some(s5) = states.get_mut(&5) {
            s5.supported = true;
        }

        crate::serial_println!("[PM] Power state manager initialized");
    }

    /// Enter sleep state
    pub fn enter_state(&self, state: u8) -> Result<(), PmError> {
        let states = self.sleep_states.lock();
        let info = states.get(&state).ok_or(PmError::UnsupportedState)?;

        if !info.supported {
            return Err(PmError::UnsupportedState);
        }

        crate::serial_println!("[PM] Entering S{}", state);

        // Prepare for sleep
        self.prepare_sleep(state)?;

        // Save context
        self.save_context()?;

        // For S4, write to swap
        if state == ACPI_STATE_S4 {
            self.write_suspend_image()?;
        }

        // Enter sleep
        self.enter_sleep(info)?;

        // We should not reach here for S3/S4
        // For S1, we continue here after wake
        self.wake_from_sleep(state)?;

        Ok(())
    }

    /// Prepare for sleep
    fn prepare_sleep(&self, state: u8) -> Result<(), PmError> {
        // Freeze processes
        crate::serial_println!("[PM] Freezing processes for S{}", state);

        // Suspend devices
        crate::serial_println!("[PM] Suspending devices");

        // Disable interrupts
        // x86_64::instructions::interrupts::disable();

        // Save LAPIC state
        // Save HPET state
        // Save other hardware state

        Ok(())
    }

    /// Save CPU context
    fn save_context(&self) -> Result<(), PmError> {
        let mut contexts = self.suspend_contexts.lock();

        // For each CPU, save context
        contexts.clear();
        contexts.push(SuspendContext::new());

        // Save context for CPU 0
        contexts[0].save();

        Ok(())
    }

    /// Write suspend image to swap (S4)
    fn write_suspend_image(&self) -> Result<(), PmError> {
        crate::serial_println!("[PM] Writing suspend image to disk");

        let mut header = SwapHeader::new();
        header.timestamp = crate::task::scheduler::get_ticks();

        // Calculate image size
        // Write memory pages to swap
        // Write header

        let mut stats = self.stats.lock();
        stats.s4_entries += 1;

        Ok(())
    }

    /// Enter sleep state
    fn enter_sleep(&self, info: &SleepStateInfo) -> Result<(), PmError> {
        let pm1a_cnt = self.pm1a_cnt_blk.load(Ordering::SeqCst);
        let pm1b_cnt = self.pm1b_cnt_blk.load(Ordering::SeqCst);

        // Write sleep type to PM1 control registers
        let sleep_type_a = (info.sleep_type_a as u16) << PM1_SLP_TYP_SHIFT;
        let sleep_type_b = (info.sleep_type_b as u16) << PM1_SLP_TYP_SHIFT;

        // PM1a_CNT_BLK
        // Write sleep_type_a | PM1_SLP_EN

        // PM1b_CNT_BLK (if exists)
        if pm1b_cnt != 0 {
            // Write sleep_type_b | PM1_SLP_EN
        }

        // Wait for sleep
        // unsafe { core::arch::asm!("hlt"); }

        self.suspended.store(true, Ordering::SeqCst);
        self.current_state.store(info.state as u32, Ordering::SeqCst);

        if info.state == ACPI_STATE_S3 {
            let mut stats = self.stats.lock();
            stats.s3_entries += 1;
        }

        Ok(())
    }

    /// Wake from sleep
    fn wake_from_sleep(&self, state: u8) -> Result<(), PmError> {
        crate::serial_println!("[PM] Waking from S{}", state);

        // Restore context
        let contexts = self.suspend_contexts.lock();
        if !contexts.is_empty() {
            contexts[0].restore();
        }

        // Restore devices
        crate::serial_println!("[PM] Resuming devices");

        // Thaw processes
        crate::serial_println!("[PM] Thawing processes");

        self.suspended.store(false, Ordering::SeqCst);
        self.current_state.store(ACPI_STATE_S0 as u32, Ordering::SeqCst);

        // Update statistics
        let mut stats = self.stats.lock();
        stats.wake_events += 1;
        if state == ACPI_STATE_S3 {
            stats.s3_exits += 1;
        } else if state == ACPI_STATE_S4 {
            stats.s4_exits += 1;
        }

        Ok(())
    }

    /// Check wake events
    pub fn check_wake_events(&self) -> Vec<u16> {
        let mut events = Vec::new();

        let pm1a_evt = self.pm1a_evt_blk.load(Ordering::SeqCst);

        // Read PM1 status register
        // let status: u16 = unsafe { core::ptr::read_volatile(pm1a_evt as *const u16) };

        // Check wake status
        // if status & PM1_WAK_STS != 0 { events.push(PM1_WAK_STS); }
        // if status & PM1_PWRBTN_STS != 0 { events.push(PM1_PWRBTN_STS); }
        // if status & PM1_RTC_STS != 0 { events.push(PM1_RTC_STS); }

        events
    }

    /// Set swap device for S4
    pub fn set_swap_device(&self, device: &str) {
        *self.swap_device.lock() = Some(String::from(device));
    }

    /// Get supported states
    pub fn get_supported_states(&self) -> Vec<u8> {
        self.sleep_states.lock()
            .iter()
            .filter(|(_, info)| info.supported)
            .map(|(state, _)| *state)
            .collect()
    }

    /// Get statistics
    pub fn get_stats(&self) -> PmStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref PM_STATE: PowerStateManager = PowerStateManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmError {
    UnsupportedState,
    DeviceSuspendFailed,
    ImageWriteFailed,
    ImageReadFailed,
    WakeFailed,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_suspend(state: u8) -> i32 {
    match PM_STATE.enter_state(state) {
        Ok(()) => 0,
        Err(PmError::UnsupportedState) => -22,
        Err(_) => -5,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init(pm1a_cnt: u64, pm1b_cnt: u64, pm1a_evt: u64, pm1b_evt: u64) {
    PM_STATE.init(pm1a_cnt, pm1b_cnt, pm1a_evt, pm1b_evt);
}
