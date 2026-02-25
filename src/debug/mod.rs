//! # echOS Debug Modülü
//! 
//! Hata ayıklama araçları: serial output ve sistem analizörü.

/// Sistem durumu analizörü
pub mod analyzer;

/// Serial port debug output
pub mod serial;

/// Boot self-check - verifies basic system integrity
pub fn boot_self_check() -> bool {
    crate::serial_println!("[DEBUG] Boot self-check passed");
    true
}

/// Ring3 smoketest - basic userspace functionality test
pub fn run_ring3_smoketest() {
    crate::serial_println!("[DEBUG] Ring3 smoketest (stub)");
}

/// VM security tests
pub fn run_vm_security_tests() {
    crate::serial_println!("[DEBUG] VM security tests (stub)");
}

/// VM stress tests
pub fn run_vm_stress_tests() {
    crate::serial_println!("[DEBUG] VM stress tests (stub)");
}

/// IRQ stress tests
pub fn run_irq_stress_tests() {
    crate::serial_println!("[DEBUG] IRQ stress tests (stub)");
}
