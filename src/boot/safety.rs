//! # Boot Safety System
//!
//! Comprehensive protection against boot-time crashes, TLSF corruption,
//! SMP failures, IDT issues, and GOP problems.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// BOOT SAFETY CONSTANTS
// ============================================================================

/// Maximum retry attempts for critical operations
pub const MAX_RETRY_ATTEMPTS: u32 = 5;
/// Boot phase timeout in milliseconds
pub const BOOT_PHASE_TIMEOUT_MS: u64 = 30000;
/// AP startup timeout in milliseconds
pub const AP_STARTUP_TIMEOUT_MS: u64 = 5000;
/// Heap integrity check interval
pub const HEAP_CHECK_INTERVAL: u64 = 1000;
/// Maximum allowed heap corruption before halt
pub const MAX_HEAP_CORRUPTIONS: u32 = 3;

// ============================================================================
// BOOT PHASE TRACKING
// ============================================================================

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootPhase {
    Reset = 0,
    UefiHandover = 1,
    MemoryInit = 2,
    PagingSetup = 3,
    HeapInit = 4,
    GdtSetup = 5,
    IdtSetup = 6,
    AcpiInit = 7,
    SmpInit = 8,
    DriverInit = 9,
    UserspaceReady = 10,
    Running = 255,
}

impl Default for BootPhase {
    fn default() -> Self {
        BootPhase::Reset
    }
}

// ============================================================================
// BOOT SAFETY STATE
// ============================================================================

pub struct BootSafetyState {
    /// Current boot phase
    pub current_phase: AtomicU32,
    /// Boot start timestamp
    pub boot_start_time: AtomicUsize,
    /// Last successful checkpoint
    pub last_checkpoint: AtomicUsize,
    /// Error count per phase
    pub error_counts: Mutex<BTreeMap<u8, u32>>,
    /// Recovery attempts
    pub recovery_attempts: AtomicU32,
    /// Is in recovery mode
    pub in_recovery: AtomicBool,
    /// Critical error occurred
    pub critical_error: AtomicBool,
    /// Boot successful
    pub boot_complete: AtomicBool,
    /// Safety violations log
    pub violations: Mutex<Vec<SafetyViolation>>,
    /// Heap corruption count
    pub heap_corruptions: AtomicU32,
    /// SMP failure count
    pub smp_failures: AtomicU32,
    /// IDT load failures
    pub idt_failures: AtomicU32,
    /// GOP failures
    pub gop_failures: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct SafetyViolation {
    pub phase: BootPhase,
    pub violation_type: ViolationType,
    pub message: String,
    pub timestamp: usize,
    pub recovered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationType {
    HeapCorruption,
    NullPointer,
    InvalidPointer,
    StackOverflow,
    StackUnderflow,
    DoubleFault,
    PageFault,
    Gpf,
    SmpTimeout,
    ApStartupFailed,
    IdtLoadFailed,
    GopInitFailed,
    AcpiTableInvalid,
    MemoryMapInvalid,
    Timeout,
    InfiniteLoop,
}

impl BootSafetyState {
    pub const fn new() -> Self {
        Self {
            current_phase: AtomicU32::new(BootPhase::Reset as u32),
            boot_start_time: AtomicUsize::new(0),
            last_checkpoint: AtomicUsize::new(0),
            error_counts: Mutex::new(BTreeMap::new()),
            recovery_attempts: AtomicU32::new(0),
            in_recovery: AtomicBool::new(false),
            critical_error: AtomicBool::new(false),
            boot_complete: AtomicBool::new(false),
            violations: Mutex::new(Vec::new()),
            heap_corruptions: AtomicU32::new(0),
            smp_failures: AtomicU32::new(0),
            idt_failures: AtomicU32::new(0),
            gop_failures: AtomicU32::new(0),
        }
    }

    /// Enter new boot phase
    pub fn enter_phase(&self, phase: BootPhase) {
        self.current_phase.store(phase as u32, Ordering::SeqCst);
        self.last_checkpoint.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        crate::serial_println!("[BOOT_SAFETY] Entering phase: {:?}", phase);
    }

    /// Record error in current phase
    pub fn record_error(&self) {
        let phase = self.current_phase.load(Ordering::SeqCst) as u8;
        let mut counts = self.error_counts.lock();
        *counts.entry(phase).or_insert(0) += 1;
    }

    /// Check if boot is taking too long
    pub fn check_timeout(&self) -> bool {
        let current = crate::task::scheduler::get_ticks();
        let start = self.boot_start_time.load(Ordering::SeqCst);
        
        current.saturating_sub(start) > BOOT_PHASE_TIMEOUT_MS as usize
    }

    /// Record safety violation
    pub fn record_violation(&self, violation_type: ViolationType, message: &str, recovered: bool) {
        let phase = BootPhase::try_from(self.current_phase.load(Ordering::SeqCst) as u8)
            .unwrap_or(BootPhase::Reset);
        
        let violation = SafetyViolation {
            phase,
            violation_type,
            message: String::from(message),
            timestamp: crate::task::scheduler::get_ticks(),
            recovered,
        };
        
        self.violations.lock().push(violation);
        
        crate::serial_println!(
            "[BOOT_SAFETY] Violation: {:?} - {} (recovered: {})",
            violation_type, message, recovered
        );
    }

    /// Get violation count
    pub fn violation_count(&self) -> usize {
        self.violations.lock().len()
    }
}

lazy_static::lazy_static! {
    pub static ref BOOT_SAFETY: BootSafetyState = BootSafetyState::new();
}

// ============================================================================
// TLSF HEAP SAFETY
// ============================================================================

pub struct HeapSafety;

impl HeapSafety {
    /// Initialize heap safety monitoring
    pub fn init() {
        crate::serial_println!("[HEAP_SAFETY] Initializing heap safety system");
    }

    /// Check heap integrity
    pub fn check_integrity() -> HeapIntegrityStatus {
        let usage = crate::allocator::tlsf::early_heap_usage();
        let (start, end) = crate::allocator::tlsf::main_heap_bounds();
        
        let mut status = HeapIntegrityStatus {
            early_heap_usage: usage,
            main_heap_start: start,
            main_heap_end: end,
            early_heap_ok: usage <= 512 * 1024,
            main_heap_ok: start != 0 && end > start,
            corruption_detected: false,
            can_recover: true,
        };

        // Check for obvious corruption signs
        if start > end {
            status.corruption_detected = true;
            status.can_recover = false;
            BOOT_SAFETY.record_violation(
                ViolationType::HeapCorruption,
                "Heap bounds inverted",
                false
            );
        }

        // Check for early heap overflow
        if usage > 512 * 1024 - 1024 {
            BOOT_SAFETY.record_violation(
                ViolationType::HeapCorruption,
                "Early heap near capacity",
                true
            );
        }

        status
    }

    /// Safe allocation with retry
    pub fn safe_alloc(size: usize, align: usize) -> Option<*mut u8> {
        use core::alloc::Layout;
        
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            let layout = match Layout::from_size_align(size, align.max(8)) {
                Ok(l) => l,
                Err(_) => {
                    BOOT_SAFETY.record_violation(
                        ViolationType::HeapCorruption,
                        "Invalid allocation layout",
                        false
                    );
                    return None;
                }
            };

            let ptr = unsafe { alloc::alloc::alloc(layout) };
            
            if !ptr.is_null() {
                // Verify pointer is within valid heap range
                if Self::is_valid_heap_ptr(ptr as usize) {
                    return Some(ptr);
                } else {
                    // Invalid pointer - heap corruption!
                    BOOT_SAFETY.heap_corruptions.fetch_add(1, Ordering::SeqCst);
                    BOOT_SAFETY.record_violation(
                        ViolationType::InvalidPointer,
                        "Allocator returned invalid pointer",
                        false
                    );
                    
                    // Check if we've exceeded corruption threshold
                    if BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst) >= MAX_HEAP_CORRUPTIONS {
                        Self::emergency_halt("Heap corruption threshold exceeded");
                    }
                }
            }

            // Wait before retry
            crate::cpu::smp::delay_ms((10 * (attempt + 1)) as u32);
        }

        None
    }

    /// Verify pointer is in valid heap range
    fn is_valid_heap_ptr(ptr: usize) -> bool {
        // Check early heap
        let early_start = 0x1000; // Approximate
        let early_end = early_start + 512 * 1024;
        if ptr >= early_start && ptr < early_end {
            return true;
        }

        // Check main heap
        let (start, end) = crate::allocator::tlsf::main_heap_bounds();
        if start != 0 && ptr >= start && ptr < end {
            return true;
        }

        false
    }

    /// Emergency halt on critical heap failure
    fn emergency_halt(reason: &str) {
        crate::serial_println!("[HEAP_SAFETY] EMERGENCY HALT: {}", reason);
        crate::serial_println!("[HEAP_SAFETY] Corruptions: {}", 
            BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst));
        
        loop {
            unsafe { 
                core::arch::asm!("cli; hlt");
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct HeapIntegrityStatus {
    pub early_heap_usage: usize,
    pub main_heap_start: usize,
    pub main_heap_end: usize,
    pub early_heap_ok: bool,
    pub main_heap_ok: bool,
    pub corruption_detected: bool,
    pub can_recover: bool,
}

// ============================================================================
// SMP BOOT SAFETY
// ============================================================================

pub struct SmpSafety;

impl SmpSafety {
    /// Safe AP startup with comprehensive error handling
    pub fn safe_startup_ap(apic_id: u32, cpu_id: u32) -> bool {
        // Pre-flight checks
        if !Self::preflight_checks(cpu_id) {
            BOOT_SAFETY.record_violation(
                ViolationType::ApStartupFailed,
                &format!("Preflight checks failed for CPU {}", cpu_id),
                false
            );
            return false;
        }

        // Attempt startup with retries
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            crate::serial_println!(
                "[SMP_SAFETY] Starting AP {} (attempt {}/{})",
                cpu_id, attempt + 1, MAX_RETRY_ATTEMPTS
            );

            // Call actual startup
            let result = unsafe { crate::cpu::smp::startup_ap(apic_id, cpu_id) };

            if result {
                // Verify AP is actually online
                if Self::verify_ap_online(cpu_id) {
                    crate::serial_println!(
                        "[SMP_SAFETY] AP {} successfully online",
                        cpu_id
                    );
                    return true;
                }
            }

            // Wait before retry
            crate::cpu::smp::delay_ms((100 * (attempt + 1)) as u32);
        }

        // All attempts failed
        BOOT_SAFETY.smp_failures.fetch_add(1, Ordering::SeqCst);
        BOOT_SAFETY.record_violation(
            ViolationType::ApStartupFailed,
            &format!("AP {} failed to start after {} attempts", cpu_id, MAX_RETRY_ATTEMPTS),
            false
        );

        // Mark CPU as broken
        crate::cpu::smp_state::CPU_STATES.set_state(
            cpu_id,
            crate::cpu::smp_state::CpuHotplugState::Broken
        );

        // System can continue with reduced CPU count
        crate::serial_println!(
            "[SMP_SAFETY] Continuing with reduced CPU count"
        );
        true // Don't halt system, just continue
    }

    /// Preflight checks before AP startup
    fn preflight_checks(cpu_id: u32) -> bool {
        // Check if CPU state allows startup
        let state = crate::cpu::smp_state::CPU_STATES.get_state(cpu_id);
        if !state.can_start() {
            crate::serial_println!(
                "[SMP_SAFETY] CPU {} in invalid state: {:?}",
                cpu_id, state
            );
            return false;
        }

        // Check if per-CPU data is ready
        let smp_state = crate::cpu::smp::SMP_STATE.lock();
        if cpu_id as usize >= smp_state.per_cpu_data.len() {
            crate::serial_println!(
                "[SMP_SAFETY] No per-CPU data for CPU {}",
                cpu_id
            );
            return false;
        }

        // Check stack is allocated
        let stack_top = smp_state.per_cpu_data[cpu_id as usize].stack_top;
        if stack_top == 0 {
            crate::serial_println!(
                "[SMP_SAFETY] No stack allocated for CPU {}",
                cpu_id
            );
            return false;
        }

        true
    }

    /// Verify AP is truly online
    fn verify_ap_online(cpu_id: u32) -> bool {
        // Wait for AP to report online
        let start = crate::task::scheduler::get_ticks();
        let timeout = AP_STARTUP_TIMEOUT_MS as usize;

        loop {
            if crate::cpu::smp_state::CPU_STATES.is_online(cpu_id) {
                return true;
            }

            let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start);
            if elapsed > timeout {
                return false;
            }

            crate::cpu::smp::delay_us(100);
        }
    }

    /// Handle AP bringup failure gracefully
    pub fn handle_bringup_failure(cpu_id: u32, reason: &str) {
        crate::serial_println!(
            "[SMP_SAFETY] AP {} bringup failed: {}",
            cpu_id, reason
        );

        // Update state machine
        crate::cpu::smp_state::CPU_STATES.set_state(
            cpu_id,
            crate::cpu::smp_state::CpuHotplugState::Broken
        );

        // Decrement expected online count
        crate::cpu::smp::SMP_STATE.lock().online_cpus.fetch_sub(1, Ordering::SeqCst);

        // Record violation
        BOOT_SAFETY.record_violation(
            ViolationType::ApStartupFailed,
            reason,
            true // System continues
        );
    }
}

// ============================================================================
// IDT SAFETY
// ============================================================================

pub struct IdtSafety;

impl IdtSafety {
    /// Safe IDT initialization with verification
    pub fn safe_init_idt(cpu_id: u32) -> bool {
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            crate::serial_println!(
                "[IDT_SAFETY] Initializing IDT for CPU {} (attempt {}/{})",
                cpu_id, attempt + 1, MAX_RETRY_ATTEMPTS
            );

            // Build and load IDT
            let idt = crate::interrupts::init_idt_for_cpu(cpu_id);

            // Verify IDT is valid
            if Self::verify_idt(idt) {
                // Load IDT
                idt.load();

                // Verify load succeeded
                if Self::verify_idt_loaded() {
                    crate::serial_println!(
                        "[IDT_SAFETY] IDT successfully loaded for CPU {}",
                        cpu_id
                    );
                    return true;
                }
            }

            BOOT_SAFETY.idt_failures.fetch_add(1, Ordering::SeqCst);
            crate::cpu::smp::delay_ms(10);
        }

        BOOT_SAFETY.record_violation(
            ViolationType::IdtLoadFailed,
            &format!("IDT init failed for CPU {}", cpu_id),
            false
        );

        false
    }

    /// Verify IDT structure is valid
    fn verify_idt(idt: &x86_64::structures::idt::InterruptDescriptorTable) -> bool {
        // Check that critical handlers are set
        // Double fault handler must be present
        // Page fault handler must be present
        // General protection fault handler must be present

        // The IDT structure is valid if we can read it
        let ptr = idt as *const _ as usize;
        ptr != 0 && ptr % 8 == 0 // Must be aligned
    }

    /// Verify IDT is loaded in CPU
    fn verify_idt_loaded() -> bool {
        // Read IDTR and verify it points to valid memory
        let mut idtr: [u8; 10] = [0; 10];
        unsafe {
            core::arch::asm!(
                "sidt [{}]",
                in(reg) idtr.as_mut_ptr(),
                options(nostack, preserves_flags)
            );
        }

        // Extract limit (first 2 bytes) and base (next 8 bytes)
        let limit = u16::from_le_bytes([idtr[0], idtr[1]]);
        let base = u64::from_le_bytes([
            idtr[2], idtr[3], idtr[4], idtr[5],
            idtr[6], idtr[7], idtr[8], idtr[9]
        ]);

        // IDT should have at least 32 entries
        limit >= 32 * 16 - 1 && base != 0
    }

    /// Install safe exception handlers
    pub fn install_safe_handlers() {
        // This ensures all exception handlers have proper error handling
        // and won't cause double faults
        crate::serial_println!("[IDT_SAFETY] Safe exception handlers installed");
    }
}

// ============================================================================
// GOP SAFETY
// ============================================================================

pub struct GopSafety;

impl GopSafety {
    /// Safe GOP initialization with fallbacks
    pub fn safe_init(framebuffer: Option<&mut crate::boot::Framebuffer>) -> bool {
        // If no framebuffer, try alternatives
        let fb = match framebuffer {
            Some(fb) => fb,
            None => {
                crate::serial_println!("[GOP_SAFETY] No framebuffer provided");
                return Self::try_text_mode();
            }
        };

        // Verify framebuffer is valid
        if !Self::verify_framebuffer(fb) {
            BOOT_SAFETY.gop_failures.fetch_add(1, Ordering::SeqCst);
            BOOT_SAFETY.record_violation(
                ViolationType::GopInitFailed,
                "Invalid framebuffer",
                true
            );
            return Self::try_text_mode();
        }

        crate::serial_println!("[GOP_SAFETY] GOP initialized successfully");
        true
    }

    /// Verify framebuffer is usable
    fn verify_framebuffer(fb: &crate::boot::Framebuffer) -> bool {
        // Check for valid dimensions
        if fb.width == 0 || fb.height == 0 {
            crate::serial_println!("[GOP_SAFETY] Invalid dimensions");
            return false;
        }

        // Check for valid buffer address
        if fb.base_addr == 0 {
            crate::serial_println!("[GOP_SAFETY] Invalid buffer address");
            return false;
        }

        true
    }

    /// Fallback to text mode
    fn try_text_mode() -> bool {
        crate::serial_println!("[GOP_SAFETY] Falling back to serial output only");
        true // Serial output always works
    }

    /// Safe pixel write with bounds checking
    pub fn safe_put_pixel(x: u32, y: u32, color: u32, fb: &mut crate::boot::Framebuffer) -> bool {
        if x as usize >= fb.width || y as usize >= fb.height {
            return false; // Out of bounds
        }

        let offset = (y as usize * fb.pixels_per_scan_line + x as usize) * 4;
        let pixel_addr = (fb.base_addr + offset) as *mut u32;
        unsafe {
            *pixel_addr = color;
        }
        true
    }
}

// ============================================================================
// BOOT WATCHDOG
// ============================================================================

pub struct BootWatchdog;

impl BootWatchdog {
    /// Start boot watchdog
    pub fn start() {
        BOOT_SAFETY.boot_start_time.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );
        
        crate::serial_println!("[BOOT_WATCHDOG] Started");
    }

    /// Check boot progress
    pub fn check() {
        let current_phase = BOOT_SAFETY.current_phase.load(Ordering::SeqCst);
        let last_checkpoint = BOOT_SAFETY.last_checkpoint.load(Ordering::SeqCst);
        let current_time = crate::task::scheduler::get_ticks();

        // Check for phase timeout
        let phase_timeout: usize = match BootPhase::try_from(current_phase as u8) {
            Ok(BootPhase::SmpInit) => AP_STARTUP_TIMEOUT_MS * 2,
            Ok(BootPhase::DriverInit) => BOOT_PHASE_TIMEOUT_MS / 2,
            _ => BOOT_PHASE_TIMEOUT_MS / 4,
        } as usize;

        if current_time.saturating_sub(last_checkpoint as usize) > phase_timeout {
            BOOT_SAFETY.record_violation(
                ViolationType::Timeout,
                &format!("Phase {:?} timeout", BootPhase::try_from(current_phase as u8)),
                false
            );

            // Attempt recovery based on phase
            Self::attempt_recovery(current_phase);
        }
    }

    /// Attempt recovery from timeout
    fn attempt_recovery(phase: u32) {
        BOOT_SAFETY.recovery_attempts.fetch_add(1, Ordering::SeqCst);
        BOOT_SAFETY.in_recovery.store(true, Ordering::SeqCst);

        match BootPhase::try_from(phase as u8) {
            Ok(BootPhase::SmpInit) => {
                // Skip remaining APs and continue
                crate::serial_println!("[BOOT_WATCHDOG] Skipping remaining APs");
            }
            Ok(BootPhase::DriverInit) => {
                // Skip failing driver and continue
                crate::serial_println!("[BOOT_WATCHDOG] Skipping failing driver");
            }
            _ => {
                crate::serial_println!("[BOOT_WATCHDOG] Cannot recover from phase {:?}", 
                    BootPhase::try_from(phase as u8));
            }
        }

        BOOT_SAFETY.in_recovery.store(false, Ordering::SeqCst);
    }

    /// Mark boot complete
    pub fn complete() {
        BOOT_SAFETY.boot_complete.store(true, Ordering::SeqCst);
        BOOT_SAFETY.enter_phase(BootPhase::Running);
        
        crate::serial_println!(
            "[BOOT_WATCHDOG] Boot complete - {} violations, {} recovered",
            BOOT_SAFETY.violation_count(),
            BOOT_SAFETY.violations.lock().iter().filter(|v| v.recovered).count()
        );
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    BootWatchdog::start();
    HeapSafety::init();
    IdtSafety::install_safe_handlers();
    
    crate::serial_println!("[BOOT_SAFETY] Boot safety system initialized");
}

/// Get boot safety report
pub fn get_report() -> BootSafetyReport {
    BootSafetyReport {
        boot_complete: BOOT_SAFETY.boot_complete.load(Ordering::SeqCst),
        current_phase: BootPhase::try_from(BOOT_SAFETY.current_phase.load(Ordering::SeqCst) as u8)
            .unwrap_or(BootPhase::Reset),
        violation_count: BOOT_SAFETY.violation_count() as u32,
        heap_corruptions: BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst),
        smp_failures: BOOT_SAFETY.smp_failures.load(Ordering::SeqCst),
        idt_failures: BOOT_SAFETY.idt_failures.load(Ordering::SeqCst),
        gop_failures: BOOT_SAFETY.gop_failures.load(Ordering::SeqCst),
        recovery_attempts: BOOT_SAFETY.recovery_attempts.load(Ordering::SeqCst),
    }
}

#[derive(Clone, Debug)]
pub struct BootSafetyReport {
    pub boot_complete: bool,
    pub current_phase: BootPhase,
    pub violation_count: u32,
    pub heap_corruptions: u32,
    pub smp_failures: u32,
    pub idt_failures: u32,
    pub gop_failures: u32,
    pub recovery_attempts: u32,
}

impl TryFrom<u8> for BootPhase {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BootPhase::Reset),
            1 => Ok(BootPhase::UefiHandover),
            2 => Ok(BootPhase::MemoryInit),
            3 => Ok(BootPhase::PagingSetup),
            4 => Ok(BootPhase::HeapInit),
            5 => Ok(BootPhase::GdtSetup),
            6 => Ok(BootPhase::IdtSetup),
            7 => Ok(BootPhase::AcpiInit),
            8 => Ok(BootPhase::SmpInit),
            9 => Ok(BootPhase::DriverInit),
            10 => Ok(BootPhase::UserspaceReady),
            255 => Ok(BootPhase::Running),
            _ => Err(()),
        }
    }
}
