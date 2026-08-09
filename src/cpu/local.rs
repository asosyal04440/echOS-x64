//! x86_64 CPU-local state and FS/GS transition contract.
//!
//! Kernel invariant:
//! - while executing Ring 0 Rust, IA32_GS_BASE points at [`CpuData`];
//! - IA32_KERNEL_GS_BASE contains the current task's user GS base;
//! - every Ring 3 boundary performs one symmetric `SWAPGS`.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::registers::model_specific::Msr;

pub const MSR_FS_BASE: u32 = 0xC000_0100;
pub const MSR_GS_BASE: u32 = 0xC000_0101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// Henüz `init` çağrılmadıysa `current_cpu_id` güvenli varsayılan döndürür.
///
/// Boot'un ilk aşamalarında (heap hazır ama SMP/per-CPU verisi kurulmadan
/// önce) IA32_GS_BASE henüz `CpuData`'yı göstermez; `gs:[16]` okuması
/// denetimsiz bellekten (SeaBIOS IVT gibi) çöp üretebilir. Bu flag olmadan
/// örneğin `gdt::current_selectors` çöp CPU kimliğiyle 30 GiB'lik bir
/// `Vec::resize` isteyip önyüklemeyi çökertebilirdi.
static CPU_LOCAL_READY: AtomicBool = AtomicBool::new(false);

/// Layout consumed directly by the syscall entry assembly.
#[repr(C, align(64))]
pub struct CpuData {
    pub user_rsp_scratch: u64,
    pub kernel_stack_top: u64,
    pub cpu_id: u32,
    pub user_rip: u64,
    pub user_rflags: u64,
    pub irq_depth: u64,
}

const _: () = {
    assert!(core::mem::offset_of!(CpuData, user_rsp_scratch) == 0);
    assert!(core::mem::offset_of!(CpuData, kernel_stack_top) == 8);
    assert!(core::mem::offset_of!(CpuData, cpu_id) == 16);
    assert!(core::mem::offset_of!(CpuData, user_rip) == 24);
    assert!(core::mem::offset_of!(CpuData, user_rflags) == 32);
    assert!(core::mem::offset_of!(CpuData, irq_depth) == 40);
    assert!(core::mem::align_of::<CpuData>() == 64);
};

#[inline(always)]
unsafe fn write_msr(msr: u32, value: u64) {
    Msr::new(msr).write(value);
}

#[inline(always)]
unsafe fn read_msr(msr: u32) -> u64 {
    Msr::new(msr).read()
}

/// Establish the Ring 0 GS invariant for the current CPU.
///
/// # Safety
/// `cpu_data` must be a live, permanently mapped, CPU-exclusive allocation.
pub unsafe fn init(cpu_data: *mut CpuData) {
    write_msr(MSR_GS_BASE, cpu_data as u64);
    write_msr(MSR_KERNEL_GS_BASE, 0);
    CPU_LOCAL_READY.store(true, Ordering::Release);
}

#[inline(always)]
pub fn current_cpu_id() -> u32 {
    #[cfg(any(test, target_os = "windows"))]
    {
        return 0;
    }

    #[cfg(not(any(test, target_os = "windows")))]
    {
        if !CPU_LOCAL_READY.load(Ordering::Acquire) {
            return 0;
        }
        let cpu_id: u32;
        unsafe {
            asm!(
                "mov {cpu_id:e}, dword ptr gs:[16]",
                cpu_id = out(reg) cpu_id,
                options(nostack, readonly, preserves_flags),
            );
        }
        cpu_id
    }
}

#[inline]
fn valid_user_base(base: u64) -> bool {
    base == 0 || crate::memory::is_user_address(base)
}

/// Store the user GS shadow without disturbing active kernel GS.
pub fn set_user_gs_base(base: u64) -> bool {
    if !valid_user_base(base) {
        return false;
    }
    unsafe { write_msr(MSR_KERNEL_GS_BASE, base) };
    true
}

/// Read the user GS shadow while active GS remains the kernel CPU-local pointer.
#[inline(always)]
pub fn user_gs_base() -> u64 {
    unsafe { read_msr(MSR_KERNEL_GS_BASE) }
}

pub fn set_user_fs_base(base: u64) -> bool {
    if !valid_user_base(base) {
        return false;
    }
    unsafe { write_msr(MSR_FS_BASE, base) };
    true
}

#[inline(always)]
pub fn user_fs_base() -> u64 {
    unsafe { read_msr(MSR_FS_BASE) }
}

#[inline(always)]
pub fn set_kernel_stack(stack_top: u64) {
    unsafe {
        asm!(
            "mov qword ptr gs:[8], {stack_top}",
            stack_top = in(reg) stack_top,
            options(nostack, preserves_flags),
        );
    }
}

#[inline(always)]
unsafe fn swapgs() {
    asm!("swapgs", options(nostack, preserves_flags));
}

#[inline(always)]
const fn interrupted_user_mode(code_segment: u64) -> bool {
    (code_segment & 3) == 3
}

#[inline(always)]
fn active_gs_is_user(active_gs: u64) -> bool {
    active_gs == 0 || crate::memory::is_user_address(active_gs)
}

/// Restores kernel GS on interrupt entry and reverses the transition on return.
#[must_use = "the guard must remain alive until the interrupt handler returns"]
pub struct KernelGsGuard {
    swapped: bool,
}

impl KernelGsGuard {
    /// Fast IRQ entry: the interrupted CS determines whether the source was Ring 3.
    #[inline(always)]
    pub fn from_interrupted_cs(code_segment: u64) -> Self {
        let swapped = interrupted_user_mode(code_segment);
        if swapped {
            unsafe { swapgs() };
        }
        Self { swapped }
    }

    /// Exception/NMI entry safe across the small CS-change-before-SWAPGS window.
    #[inline]
    pub fn paranoid() -> Self {
        let active_gs = unsafe { read_msr(MSR_GS_BASE) };
        let swapped = active_gs_is_user(active_gs);
        if swapped {
            unsafe { swapgs() };
        }
        Self { swapped }
    }
}

impl Drop for KernelGsGuard {
    #[inline(always)]
    fn drop(&mut self) {
        if self.swapped {
            unsafe { swapgs() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_data_assembly_layout_is_stable() {
        assert_eq!(core::mem::offset_of!(CpuData, cpu_id), 16);
        assert_eq!(core::mem::offset_of!(CpuData, irq_depth), 40);
        assert_eq!(core::mem::size_of::<CpuData>(), 64);
    }

    #[test]
    fn architectural_msr_numbers_are_not_aliased() {
        assert_eq!(MSR_FS_BASE, 0xC000_0100);
        assert_eq!(MSR_GS_BASE, 0xC000_0101);
        assert_eq!(MSR_KERNEL_GS_BASE, 0xC000_0102);
    }

    #[test]
    fn privilege_level_controls_normal_interrupt_swap() {
        assert!(!interrupted_user_mode(0x08));
        assert!(interrupted_user_mode(0x2B));
    }
}
