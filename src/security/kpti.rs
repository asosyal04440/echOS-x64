//! # Kernel Page Table Isolation (KPTI) Guard Rails
//!
//! Tam CR3 ayrıştırması yerine, kullanıcı eşlemelerinde hassas aralıkların
//! dışarı sızmasını engelleyen izolasyon denetimi uygular.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

#[derive(Clone, Copy, Debug)]
struct SensitiveRange {
    start: u64,
    end: u64,
}

static KPTI_ENABLED: AtomicBool = AtomicBool::new(false);
static MELTDOWN_GUARD: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref SENSITIVE_RANGES: Mutex<Vec<SensitiveRange>> = Mutex::new(Vec::new());
}

#[cfg(target_arch = "x86_64")]
fn vendor_is_intel() -> bool {
    let leaf0 = unsafe { core::arch::x86_64::__cpuid(0) };
    let mut vendor = [0u8; 12];
    vendor[..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
    &vendor == b"GenuineIntel"
}

#[cfg(not(target_arch = "x86_64"))]
fn vendor_is_intel() -> bool {
    false
}

pub fn init() {
    let meltdown_guard = vendor_is_intel();
    MELTDOWN_GUARD.store(meltdown_guard, Ordering::Relaxed);
    KPTI_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[KPTI] enabled (meltdown_guard={})", meltdown_guard);
}

pub fn is_enabled() -> bool {
    KPTI_ENABLED.load(Ordering::SeqCst)
}

pub fn register_sensitive_range(start: u64, size: u64) {
    if size == 0 {
        return;
    }
    let end = start.saturating_add(size);
    let mut ranges = SENSITIVE_RANGES.lock();
    ranges.push(SensitiveRange { start, end });
}

fn overlaps(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

pub fn user_mapping_allowed(start: u64, size: u64) -> bool {
    if !is_enabled() || size == 0 {
        return true;
    }

    let end = start.saturating_add(size);
    for range in SENSITIVE_RANGES.lock().iter() {
        if overlaps(start, end, range.start, range.end) {
            return false;
        }
    }
    true
}
