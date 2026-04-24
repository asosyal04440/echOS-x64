//! # Control-Flow Integrity (CFI) Guard
//!
//! Donanım IBT/CET bilgilerini algılar ve yazılım tarafında shadow-call-stack doğrulaması uygular.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

static CFI_ENABLED: AtomicBool = AtomicBool::new(false);
static HW_IBT: AtomicBool = AtomicBool::new(false);
static HW_SHSTK: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref INDIRECT_TARGETS: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
    static ref SHADOW_STACKS: Mutex<BTreeMap<usize, Vec<u64>>> = Mutex::new(BTreeMap::new());
}

#[cfg(target_arch = "x86_64")]
fn detect_hw_cet() -> (bool, bool) {
    let leaf7 = unsafe { core::arch::x86_64::__cpuid_count(7, 0) };
    let ibt = (leaf7.edx & (1 << 20)) != 0;
    let shstk = (leaf7.ecx & (1 << 7)) != 0;
    (ibt, shstk)
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_hw_cet() -> (bool, bool) {
    (false, false)
}

fn scope_token(pid: usize, tag: u64) -> u64 {
    let t = crate::interrupts::get_ticks() as u64;
    let mut x = (pid as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ tag ^ t;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^ (x >> 33)
}

pub struct CfiScope {
    pid: usize,
    token: u64,
}

impl Drop for CfiScope {
    fn drop(&mut self) {
        if !CFI_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let mut stacks = SHADOW_STACKS.lock();
        let Some(stack) = stacks.get_mut(&self.pid) else {
            return;
        };
        let top = stack.pop();
        if top != Some(self.token) {
            crate::serial_println!(
                "[CFI] Shadow stack mismatch pid={} expected={:#x} got={:?}",
                self.pid,
                self.token,
                top
            );
        }
    }
}

pub fn init() {
    let (ibt, shstk) = detect_hw_cet();
    HW_IBT.store(ibt, Ordering::Relaxed);
    HW_SHSTK.store(shstk, Ordering::Relaxed);
    CFI_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!(
        "[CFI] enabled (IBT hw={}, ShadowStack hw={}, software shadow stack active)",
        ibt,
        shstk
    );
}

pub fn is_enabled() -> bool {
    CFI_ENABLED.load(Ordering::SeqCst)
}

pub fn register_indirect_target(addr: u64) {
    INDIRECT_TARGETS.lock().insert(addr);
}

pub fn validate_indirect_target(addr: u64) -> bool {
    if !is_enabled() {
        return true;
    }
    INDIRECT_TARGETS.lock().contains(&addr)
}

pub fn enter_syscall_scope(sysno: u64) -> CfiScope {
    let pid = crate::task::scheduler::current_task_id();
    let token = scope_token(pid, sysno);
    if is_enabled() {
        SHADOW_STACKS.lock().entry(pid).or_default().push(token);
    }
    CfiScope { pid, token }
}
