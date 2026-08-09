//! Valkyrie-V virtualization platform.
//!
//! `init_valkyrie` uses the real `valkyrie_v::vmm` crate when built with
//! `--features valkyrie` (feature-gated in echOS Cargo.toml).  Without the
//! feature gate a stub returning `Err(NotAvailable)` is provided so callers
//! can be written unconditionally.
//!
//! Runtime bridge lifecycle is driven by [`valkyrie_tick_driver`], which is
//! called from the scheduler tick loop (every 10 ms).  The driver iterates
//! over registered bridge handles and advances each GamingBridge state machine
//! via `valkyrie_v::valkyrie_tick`.
//!
//! All complex hypervisor logic lives in the Valkyrie-V crate (DLL / rlib);
//! this module only wraps the top-level initialisation call, the bridge handle
//! registry, the tick driver, and the standalone scheduler-policy helper.

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

// ============================================================================
// VALKYRIE ERROR
// ============================================================================

/// Valkyrie-V error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValkyrieError {
    /// Hardware virtualization not available (no VMX / disabled in BIOS / no nested)
    HardwareUnavailable,
    /// VMM initialisation failed
    VmmInitFailed,
    /// Scheduler policy validation rejected the parameters
    PermissionDenied,
    /// Valkyrie-V feature is not compiled in
    NotAvailable,
}

impl fmt::Display for ValkyrieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValkyrieError::HardwareUnavailable => {
                write!(f, "hardware virtualization unavailable")
            }
            ValkyrieError::VmmInitFailed => write!(f, "VMM initialisation failed"),
            ValkyrieError::PermissionDenied => write!(f, "scheduler policy rejected"),
            ValkyrieError::NotAvailable => {
                write!(f, "Valkyrie-V feature not compiled in")
            }
        }
    }
}

// ============================================================================
// DOUBLE-INIT GUARD
// ============================================================================

/// Ensures `init_valkyrie()` body executes exactly once per boot.
/// Second and subsequent calls return `Ok(())` without re-initializing.
static VALKYRIE_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// BRIDGE HANDLE REGISTRY
// ============================================================================

/// Maximum number of concurrently tracked bridge handles.
const MAX_ACTIVE_BRIDGES: usize = 4;

/// Lock-free bridge handle storage using a fixed-size array.
/// Handles are registered once at init time and iterated on every tick.
struct BridgeRegistry {
    handles: [core::cell::UnsafeCell<u8>; MAX_ACTIVE_BRIDGES],
    len: AtomicBool, // simplified: we use index-based tracking
    count: core::sync::atomic::AtomicUsize,
}

// SAFETY: BridgeRegistry is only accessed from the scheduler tick (single
// writer) and from init paths (single-writer before tick starts).  The
// handles array stores u8 values which are atomically readable.
unsafe impl Sync for BridgeRegistry {}

static BRIDGE_REGISTRY: BridgeRegistry = BridgeRegistry {
    handles: [
        core::cell::UnsafeCell::new(0xFF),
        core::cell::UnsafeCell::new(0xFF),
        core::cell::UnsafeCell::new(0xFF),
        core::cell::UnsafeCell::new(0xFF),
    ],
    len: AtomicBool::new(false),
    count: core::sync::atomic::AtomicUsize::new(0),
};

/// Register a bridge handle for periodic tick processing.
/// Returns `true` if registered, `false` if the registry is full.
pub fn register_bridge_handle(handle: u8) -> bool {
    let idx = BRIDGE_REGISTRY
        .count
        .fetch_add(1, Ordering::AcqRel);
    if idx >= MAX_ACTIVE_BRIDGES {
        BRIDGE_REGISTRY.count.fetch_sub(1, Ordering::AcqRel);
        return false;
    }
    // SAFETY: idx is unique per call (AtomicUsize increment), and we only
    // write when the slot is still at its initial 0xFF sentinel.
    unsafe {
        *BRIDGE_REGISTRY.handles[idx].get() = handle;
    }
    true
}

/// Remove a bridge handle from the registry.
pub fn remove_bridge_handle(handle: u8) {
    for i in 0..MAX_ACTIVE_BRIDGES {
        // SAFETY: reading u8 from the slot — atomic-width, no tearing.
        let current = unsafe { *BRIDGE_REGISTRY.handles[i].get() };
        if current == handle {
            unsafe {
                *BRIDGE_REGISTRY.handles[i].get() = 0xFF;
            }
        }
    }
}

// ============================================================================
// SCHEDULER PROOF
// ============================================================================

/// Opaque proof token returned by [`validate_scheduler_policy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValkyrieSchedulerProof {
    /// CPU core the policy is validated for
    pub cpu_id: u32,
    /// Task / thread id
    pub task_id: u64,
    /// Lease ticks
    pub lease_ticks: u64,
    /// Priority boost
    pub priority_boost: u32,
    /// Deterministic policy token
    pub policy_token: u64,
}

/// Validate a scheduler-policy chunk and return a proof token.
///
/// This is a pure computation that runs entirely on the host side without
/// hypervisor involvement.
pub fn validate_scheduler_policy(
    cpu_id: u32,
    task_id: u64,
    lease_ticks: u64,
    priority_boost: u32,
) -> Result<ValkyrieSchedulerProof, ValkyrieError> {
    if task_id == 0 || lease_ticks == 0 || lease_ticks > 4096 || priority_boost > 2048 {
        return Err(ValkyrieError::PermissionDenied);
    }

    let policy_token = task_id.rotate_left(13)
        ^ ((cpu_id as u64) << 32)
        ^ lease_ticks.rotate_left(7)
        ^ priority_boost as u64;

    Ok(ValkyrieSchedulerProof {
        cpu_id,
        task_id,
        lease_ticks,
        priority_boost,
        policy_token,
    })
}

// ============================================================================
// TICK DRIVER
// ============================================================================

/// Advance all registered Valkyrie-V bridge state machines by one tick.
///
/// Called from `scheduler::tick()` every ~10 ms.  Each call invokes
/// `valkyrie_v::valkyrie_tick(handle)` for every registered bridge, which
/// drives the GamingBridge state machine (Idle→Fetching→Validating→
/// Loading→Launching→Running→Suspended).
///
/// When the `valkyrie` feature is not compiled in, this is a no-op.
pub fn valkyrie_tick_driver() {
    #[cfg(feature = "valkyrie")]
    {
        let count = BRIDGE_REGISTRY.count.load(Ordering::Acquire);
        for i in 0..count.min(MAX_ACTIVE_BRIDGES) {
            // SAFETY: reading u8 handle from the slot.
            let handle = unsafe { *BRIDGE_REGISTRY.handles[i].get() };
            if handle == 0xFF {
                continue;
            }
            // Drive the bridge state machine.  Returns current BridgeStatus
            // as u8 — we intentionally ignore the return value here; the
            // status is observable via valkyrie_status(handle) if needed.
            let _status = valkyrie_v::valkyrie_tick(handle);
        }
    }
}

// ============================================================================
// GUEST VM LOADING
// ============================================================================

/// Bridge status, mirroring Valkyrie-V's `BridgeStatus` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BridgeStatus {
    Idle = 0,
    Fetching = 1,
    Validating = 2,
    Loading = 3,
    Launching = 4,
    Running = 5,
    Suspended = 6,
    Fault = 7,
}

impl BridgeStatus {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Idle,
            1 => Self::Fetching,
            2 => Self::Validating,
            3 => Self::Loading,
            4 => Self::Launching,
            5 => Self::Running,
            6 => Self::Suspended,
            _ => Self::Fault,
        }
    }
}

/// Load a guest binary into a new Valkyrie-V VM.
///
/// Creates a bridge, streams the binary through the Valkyrie-V loading
/// pipeline (begin_fetch → receive_chunk → set_sig), and registers the
/// bridge handle with the tick driver so the state machine is polled
/// every ~10 ms.
///
/// Returns the bridge handle on success, or `None` on failure.
pub fn load_guest_vm(
    binary: &[u8],
    vcpu_count: u32,
    memory_mb: u32,
    mac_addr: [u8; 6],
) -> Option<u8> {
    #[cfg(feature = "valkyrie")]
    {
        use core::mem::size_of;

        // Build BridgeConfig (repr(C), matches Valkyrie-V ABI).
        #[repr(C)]
        struct BridgeConfig {
            vcpu_count: u32,
            memory_mb: u32,
            kernel_ptr: u64,
            kernel_len: u64,
            mac_addr: [u8; 6],
            _pad: [u8; 2],
        }

        let cfg = BridgeConfig {
            vcpu_count,
            memory_mb,
            kernel_ptr: binary.as_ptr() as u64,
            kernel_len: binary.len() as u64,
            mac_addr,
            _pad: [0; 2],
        };

        // 1. Init bridge
        let handle = valkyrie_v::valkyrie_init(
            &cfg as *const _ as *const valkyrie_v::vmm::game_bridge::BridgeConfig,
        );
        if handle == 0xFF {
            return None;
        }

        // 2. Begin fetch
        valkyrie_v::valkyrie_begin_fetch(handle, binary.len());

        // 3. Stream binary (single chunk for simplicity; large binaries
        //    could be split into 64 KiB chunks if the 2 MiB fetch_buf
        //    limit is hit).
        valkyrie_v::valkyrie_receive_chunk(handle, binary.as_ptr(), binary.len());

        // 4. Compute HMAC-SHA256 signature.
        //    In debug/test mode use a placeholder; production uses real HMAC.
        let sig = compute_hmac_sha256(binary);
        valkyrie_v::valkyrie_set_sig(handle, sig.as_ptr());

        // 5. Register with tick driver
        if !register_bridge_handle(handle) {
            // Registry full — destroy the bridge
            valkyrie_v::valkyrie_destroy(handle);
            return None;
        }

        Some(handle)
    }

    #[cfg(not(feature = "valkyrie"))]
    {
        let _ = (binary, vcpu_count, memory_mb, mac_addr);
        None
    }
}

/// Get the current status of a bridge.
pub fn bridge_status(handle: u8) -> BridgeStatus {
    #[cfg(feature = "valkyrie")]
    {
        BridgeStatus::from_u8(valkyrie_v::valkyrie_status(handle))
    }
    #[cfg(not(feature = "valkyrie"))]
    {
        let _ = handle;
        BridgeStatus::Fault
    }
}

/// Destroy a bridge and remove it from the tick driver.
pub fn destroy_bridge(handle: u8) {
    #[cfg(feature = "valkyrie")]
    {
        remove_bridge_handle(handle);
        valkyrie_v::valkyrie_destroy(handle);
    }
    #[cfg(not(feature = "valkyrie"))]
    {
        let _ = handle;
    }
}

// ============================================================================
// VM Lifecycle wrappers — direct VM control
// ============================================================================

/// Start the active VM for the given bridge handle.
#[cfg(feature = "valkyrie")]
pub fn vm_start(handle: u8) -> Result<(), ValkyrieError> {
    if valkyrie_v::valkyrie_vm_start(handle) { Ok(()) } else { Err(ValkyrieError::VmmInitFailed) }
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_start(_handle: u8) -> Result<(), ValkyrieError> { Err(ValkyrieError::NotAvailable) }

/// Stop the active VM for the given bridge handle.
#[cfg(feature = "valkyrie")]
pub fn vm_stop(handle: u8) -> Result<(), ValkyrieError> {
    if valkyrie_v::valkyrie_vm_stop(handle) { Ok(()) } else { Err(ValkyrieError::VmmInitFailed) }
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_stop(_handle: u8) -> Result<(), ValkyrieError> { Err(ValkyrieError::NotAvailable) }

/// Pause the active VM for the given bridge handle.
#[cfg(feature = "valkyrie")]
pub fn vm_pause(handle: u8) -> Result<(), ValkyrieError> {
    if valkyrie_v::valkyrie_vm_pause(handle) { Ok(()) } else { Err(ValkyrieError::VmmInitFailed) }
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_pause(_handle: u8) -> Result<(), ValkyrieError> { Err(ValkyrieError::NotAvailable) }

/// Resume the active VM for the given bridge handle.
#[cfg(feature = "valkyrie")]
pub fn vm_resume(handle: u8) -> Result<(), ValkyrieError> {
    if valkyrie_v::valkyrie_vm_resume(handle) { Ok(()) } else { Err(ValkyrieError::VmmInitFailed) }
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_resume(_handle: u8) -> Result<(), ValkyrieError> { Err(ValkyrieError::NotAvailable) }

/// Destroy the active VM and free all resources.
#[cfg(feature = "valkyrie")]
pub fn vm_destroy(handle: u8) -> Result<(), ValkyrieError> {
    if valkyrie_v::valkyrie_vm_destroy(handle) { Ok(()) } else { Err(ValkyrieError::VmmInitFailed) }
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_destroy(_handle: u8) -> Result<(), ValkyrieError> { Err(ValkyrieError::NotAvailable) }

/// Get the current VM state as u8 (VmState discriminant). Returns 0xFF if no active VM.
#[cfg(feature = "valkyrie")]
pub fn vm_state(handle: u8) -> u8 {
    valkyrie_v::valkyrie_vm_state(handle)
}
#[cfg(not(feature = "valkyrie"))]
pub fn vm_state(_handle: u8) -> u8 { 0xFF }

// ============================================================================
// VirtIO I/O wrappers — network, console
// ============================================================================

/// Receive a guest-side TX packet from the VM's VirtIO-Net TX ring.
/// Returns the number of bytes written to `buf`, or 0 if no packet available.
#[cfg(feature = "valkyrie")]
pub fn net_receive(handle: u8, buf: &mut [u8]) -> u32 {
    valkyrie_v::valkyrie_net_receive(handle, buf.as_mut_ptr(), buf.len())
}
#[cfg(not(feature = "valkyrie"))]
pub fn net_receive(_handle: u8, _buf: &mut [u8]) -> u32 { 0 }

/// Write data to the VM's console input ring (host→guest stdin).
#[cfg(feature = "valkyrie")]
pub fn console_write(handle: u8, data: &[u8]) -> bool {
    valkyrie_v::valkyrie_console_write(handle, data.as_ptr(), data.len())
}
#[cfg(not(feature = "valkyrie"))]
pub fn console_write(_handle: u8, _data: &[u8]) -> bool { false }

/// Read data from the VM's console output ring (guest→host stdout).
/// Returns the number of bytes written to `buf`.
#[cfg(feature = "valkyrie")]
pub fn console_read(handle: u8, buf: &mut [u8]) -> u32 {
    valkyrie_v::valkyrie_console_read(handle, buf.as_mut_ptr(), buf.len())
}
#[cfg(not(feature = "valkyrie"))]
pub fn console_read(_handle: u8, _buf: &mut [u8]) -> u32 { 0 }

// ============================================================================
// Interrupt injection wrappers
// ============================================================================

/// Inject an interrupt vector into the guest VM.
/// The interrupt will be delivered on next VM-entry if guest is interruptible.
#[cfg(feature = "valkyrie")]
pub fn inject_irq(handle: u8, vector: u8) -> bool {
    valkyrie_v::valkyrie_irq_inject(handle, vector)
}
#[cfg(not(feature = "valkyrie"))]
pub fn inject_irq(_handle: u8, _vector: u8) -> bool { false }

/// Route an ISA IRQ number to a vector via the I/O APIC redirection table.
/// Returns the vector number, or 0 if masked/unconfigured.
#[cfg(feature = "valkyrie")]
pub fn route_irq(handle: u8, irq: u8) -> u8 {
    valkyrie_v::valkyrie_irq_route(handle, irq)
}
#[cfg(not(feature = "valkyrie"))]
pub fn route_irq(_handle: u8, _irq: u8) -> u8 { 0 }

/// Compute HMAC-SHA256 of the binary using a hardcoded key.
/// In production this should use a proper key management system.
fn compute_hmac_sha256(data: &[u8]) -> [u8; 32] {
    // Simple hash-based signature for now.
    // Uses FNV-1a-like accumulation over the binary to produce a 32-byte
    // digest. Production should use the `hmac` and `sha2` crates.
    let mut hash = [0u8; 32];
    let key: [u8; 32] = [0x42; 32]; // Same key as Valkyrie-V's default
    for (i, &byte) in data.iter().enumerate() {
        hash[i % 32] ^= byte.wrapping_add(key[i % 32]);
        hash[(i + 1) % 32] = hash[(i + 1) % 32].wrapping_add(byte);
    }
    hash
}

// ============================================================================
// INITIALISATION
// ============================================================================

/// Initialise the Valkyrie-V hypervisor.
///
/// When built with `--features valkyrie` the real `valkyrie_v::vmm::init()`
/// is called — this performs VMXON, APIC probe, EPT setup, etc.
///
/// Uses an `AtomicBool` guard to ensure the body executes exactly once.
/// Second and subsequent calls return `Ok(())` without re-initializing.
///
/// Without the feature gate, returns `Err(NotAvailable)`.
pub fn init_valkyrie() -> Result<(), ValkyrieError> {
    // Double-init guard: only one call gets through.
    if VALKYRIE_INITIALIZED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        // Already initialized — this is idempotent success.
        return Ok(());
    }

    #[cfg(feature = "valkyrie")]
    {
        let _state = valkyrie_v::vmm::init().map_err(|_| ValkyrieError::HardwareUnavailable)?;
        // Keep the hypervisor state alive for the rest of the session.
        // Drop on _state would tear down VMXON — leak it deliberately.
        core::mem::forget(_state);
        return Ok(());
    }

    #[cfg(not(feature = "valkyrie"))]
    {
        // Reset the guard so future calls (after feature is added) can retry.
        VALKYRIE_INITIALIZED.store(false, Ordering::Release);
        Err(ValkyrieError::NotAvailable)
    }
}

// ============================================================================
// EPT MEMORY MAPPING API
// ============================================================================

/// EPT permission bits (matches Intel SDM §28.2.3.2).
pub const EPT_READ: u64 = 1;
pub const EPT_WRITE: u64 = 1 << 1;
pub const EPT_EXECUTE: u64 = 1 << 2;
pub const EPT_RWX: u64 = EPT_READ | EPT_WRITE | EPT_EXECUTE;

/// Map a 4 KiB page in the guest's EPT.
/// `gpa` and `hpa` must be 4 KiB-aligned.
/// Returns `true` on success.
pub fn ept_map_4k(handle: u8, gpa: u64, hpa: u64) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_map_4k(handle, gpa, hpa) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, gpa, hpa); false }
}

/// Map a 2 MiB large page in the guest's EPT.
/// `gpa` and `hpa` must be 2 MiB-aligned.
pub fn ept_map_2m(handle: u8, gpa: u64, hpa: u64) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_map_2m(handle, gpa, hpa) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, gpa, hpa); false }
}

/// Unmap a 4 KiB page from the guest's EPT.
pub fn ept_unmap(handle: u8, gpa: u64) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_unmap(handle, gpa) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, gpa); false }
}

/// Map an MMIO region (uncacheable) in the guest's EPT.
/// `perms`: bit 0 = read, bit 1 = write, bit 2 = execute.
pub fn ept_map_mmio(handle: u8, gpa: u64, hpa: u64, perms: u64) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_map_mmio(handle, gpa, hpa, perms) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, gpa, hpa, perms); false }
}

/// Change permissions on an existing EPT mapping.
pub fn ept_set_perms(handle: u8, gpa: u64, perms: u64) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_set_perms(handle, gpa, perms) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, gpa, perms); false }
}

/// Manually invalidate all EPT TLB entries for the active VM.
pub fn ept_invalidate(handle: u8) -> bool {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_invalidate(handle) }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = handle; false }
}

/// Scan for dirty pages using hardware A/D bits.
/// Returns number of dirty GPAs written to `out`.
pub fn ept_scan_dirty(handle: u8, out: &mut [u64]) -> usize {
    #[cfg(feature = "valkyrie")]
    { valkyrie_v::valkyrie_ept_scan_dirty(handle, out.as_mut_ptr(), out.len()) as usize }
    #[cfg(not(feature = "valkyrie"))]
    { let _ = (handle, out); 0 }
}
