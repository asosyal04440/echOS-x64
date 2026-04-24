use core::arch::asm;
use core::arch::x86_64::__cpuid_count;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use x86_64::registers::model_specific::Msr;

const IA32_SPEC_CTRL: u32 = 0x48;
const IA32_PRED_CMD: u32 = 0x49;
const IA32_ARCH_CAPABILITIES: u32 = 0x10A;
const SPEC_CTRL_IBRS: u64 = 1 << 0;
const SPEC_CTRL_STIBP: u64 = 1 << 1;
const SPEC_CTRL_SSBD: u64 = 1 << 2;
const PRED_CMD_IBPB: u64 = 1;
const BHI_SENTINEL_ROUNDS: usize = 4;
const ARCH_CAP_IBRS_ALL: u64 = 1 << 1;
const ARCH_CAP_MDS_NO: u64 = 1 << 5;
const ARCH_CAP_TAA_NO: u64 = 1 << 8;
const ARCH_CAP_FB_CLEAR: u64 = 1 << 17;
const ARCH_CAP_BHI_NO: u64 = 1 << 20;

static SPEC_CTRL_MASK: AtomicU64 = AtomicU64::new(0);
static IBPB_SUPPORTED: AtomicBool = AtomicBool::new(false);
static MD_CLEAR_REQUIRED: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static MD_CLEAR_SELECTOR: u16 = 0;

#[inline(always)]
fn log_spectre_mitigations(mitigations: SpectreMitigations) {
    #[cfg(debug_assertions)]
    crate::serial_println!(
        "[SEC] Spectre/BHI mitigations: IBRS={} IBPB={} STIBP={} SSBD={} BHB={} eIBRS={} MDS_NO={} TAA_NO={} FB_CLEAR={}",
        mitigations.ibrs,
        mitigations.ibpb,
        mitigations.stibp,
        mitigations.ssbd,
        mitigations.bhi_safe_mode,
        mitigations.enhanced_ibrs,
        mitigations.mds_no,
        mitigations.taa_no,
        mitigations.fb_clear
    );

    #[cfg(not(debug_assertions))]
    let _ = mitigations;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SpectreMitigations {
    pub ibrs: bool,
    pub ibpb: bool,
    pub stibp: bool,
    pub ssbd: bool,
    pub bhi_safe_mode: bool,
    pub enhanced_ibrs: bool,
    pub bhi_no: bool,
    pub mds_no: bool,
    pub taa_no: bool,
    pub fb_clear: bool,
}

pub fn init() {
    let mitigations = detect_mitigations();
    SPEC_CTRL_MASK.store(
        (if mitigations.ibrs { SPEC_CTRL_IBRS } else { 0 })
            | (if mitigations.stibp {
                SPEC_CTRL_STIBP
            } else {
                0
            })
            | (if mitigations.ssbd { SPEC_CTRL_SSBD } else { 0 }),
        Ordering::SeqCst,
    );
    IBPB_SUPPORTED.store(mitigations.ibpb, Ordering::SeqCst);
    MD_CLEAR_REQUIRED.store(
        mitigations.fb_clear && (!mitigations.mds_no || !mitigations.taa_no),
        Ordering::SeqCst,
    );

    if let Some(mask) = spec_ctrl_mask() {
        unsafe {
            let mut spec_ctrl = Msr::new(IA32_SPEC_CTRL);
            let current = spec_ctrl.read();
            spec_ctrl.write(current | mask);
        }
    }

    if mitigations.bhi_safe_mode {
        bhi_entry_barrier();
    }
    if MD_CLEAR_REQUIRED.load(Ordering::SeqCst) {
        md_clear_buffers();
    }

    INITIALIZED.store(true, Ordering::SeqCst);
    log_spectre_mitigations(mitigations);
}

pub fn init_cpu() {
    if let Some(mask) = spec_ctrl_mask() {
        unsafe {
            let mut spec_ctrl = Msr::new(IA32_SPEC_CTRL);
            let current = spec_ctrl.read();
            spec_ctrl.write(current | mask);
        }
    }
    if INITIALIZED.load(Ordering::SeqCst) {
        bhi_entry_barrier();
        if MD_CLEAR_REQUIRED.load(Ordering::Relaxed) {
            md_clear_buffers();
        }
    }
}

#[inline(always)]
pub fn kernel_entry_barrier() {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return;
    }
    bhi_entry_barrier();
    if MD_CLEAR_REQUIRED.load(Ordering::Relaxed) {
        md_clear_buffers();
    }
}

#[inline(always)]
pub fn on_context_switch() {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return;
    }
    if IBPB_SUPPORTED.load(Ordering::Relaxed) {
        unsafe {
            Msr::new(IA32_PRED_CMD).write(PRED_CMD_IBPB);
        }
    }
    bhi_entry_barrier();
    if MD_CLEAR_REQUIRED.load(Ordering::Relaxed) {
        md_clear_buffers();
    }
}

pub fn status() -> SpectreMitigations {
    detect_mitigations()
}

fn detect_mitigations() -> SpectreMitigations {
    let leaf0 = unsafe { __cpuid_count(0, 0) };
    if leaf0.eax < 7 {
        return SpectreMitigations::default();
    }

    let leaf7 = unsafe { __cpuid_count(7, 0) };
    let ibrs_ibpb = (leaf7.edx & (1 << 26)) != 0;
    let stibp = (leaf7.edx & (1 << 27)) != 0;
    let arch_caps = (leaf7.edx & (1 << 29)) != 0;
    let ssbd = (leaf7.edx & (1 << 31)) != 0;
    let arch_capabilities = if arch_caps {
        Some(unsafe { Msr::new(IA32_ARCH_CAPABILITIES).read() })
    } else {
        None
    };
    let enhanced_ibrs = arch_capabilities
        .map(|caps| (caps & ARCH_CAP_IBRS_ALL) != 0)
        .unwrap_or(false);
    let bhi_no = arch_capabilities
        .map(|caps| (caps & ARCH_CAP_BHI_NO) != 0)
        .unwrap_or(false);
    let mds_no = arch_capabilities
        .map(|caps| (caps & ARCH_CAP_MDS_NO) != 0)
        .unwrap_or(false);
    let taa_no = arch_capabilities
        .map(|caps| (caps & ARCH_CAP_TAA_NO) != 0)
        .unwrap_or(false);
    let fb_clear = arch_capabilities
        .map(|caps| (caps & ARCH_CAP_FB_CLEAR) != 0)
        .unwrap_or(false);

    SpectreMitigations {
        ibrs: ibrs_ibpb,
        ibpb: ibrs_ibpb,
        stibp,
        ssbd,
        bhi_safe_mode: bhi_no || ibrs_ibpb || enhanced_ibrs,
        enhanced_ibrs,
        bhi_no,
        mds_no,
        taa_no,
        fb_clear,
    }
}

fn spec_ctrl_mask() -> Option<u64> {
    let mask = SPEC_CTRL_MASK.load(Ordering::Relaxed);
    if mask == 0 {
        None
    } else {
        Some(mask)
    }
}

#[inline(always)]
fn bhi_entry_barrier() {
    for _ in 0..BHI_SENTINEL_ROUNDS {
        unsafe {
            asm!("lfence", options(nomem, nostack, preserves_flags));
        }
    }
}

#[inline(always)]
fn md_clear_buffers() {
    unsafe {
        asm!("verw [{selector}]", selector = in(reg) &MD_CLEAR_SELECTOR, options(readonly, nostack));
    }
}
