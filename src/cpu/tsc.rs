//! # TSC (Time Stamp Counter)
//!
//! High-resolution timing using CPU timestamp counter

use core::arch::asm;

/// Read TSC value
pub fn read() -> u64 {
    let low: u32;
    let high: u32;
    
    unsafe {
        asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack)
        );
    }
    
    ((high as u64) << 32) | (low as u64)
}

/// Read TSC in nanoseconds (assumes CPU frequency is known)
/// This is a simplified version - real implementation needs calibration
pub fn read_ns() -> u64 {
    // Assume ~3GHz CPU for now - this should be calibrated properly
    let tsc = read();
    // Convert to nanoseconds (assuming 3GHz = 3 cycles per ns)
    tsc / 3
}

/// Read TSC in microseconds
pub fn read_us() -> u64 {
    read_ns() / 1000
}

/// Read TSC in milliseconds
pub fn read_ms() -> u64 {
    read_us() / 1000
}

/// Get CPU frequency in Hz (placeholder - needs calibration)
pub fn cpu_frequency() -> u64 {
    // Placeholder: 3 GHz
    3_000_000_000
}

/// Calibrate TSC against PIT (Programmable Interval Timer)
/// Returns TSC frequency in Hz
pub fn calibrate() -> u64 {
    // This is a simplified calibration
    // Real implementation would use PIT or HPET
    3_000_000_000
}
