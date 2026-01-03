use core::arch::asm;
use x86_64::VirtAddr;

/// Jumps to User Mode using IRETQ.
/// This function never returns.
/// 
/// # Safety
/// This function executes `iretq` which changes privilege level.
/// The caller must ensure that the provided `entry_point` and `user_stack_top` are valid
/// virtual addresses accessible in User Mode.
pub unsafe fn enter_user_mode(entry_point: VirtAddr, user_stack_top: VirtAddr) -> ! {
    let user_cs = crate::gdt::GDT.1.user_code_selector.0;
    let user_ds = crate::gdt::GDT.1.user_data_selector.0;
    
    // RPL = 3 (Ring 3)
    let user_cs = user_cs | 3;
    let user_ds = user_ds | 3;
    
    // RFLAGS: Interrupts enabled (bit 9), Reserved (bit 1) -> 0x202
    // IOPL=0 (User cannot use IO ports generally)
    let rflags: u64 = 0x202;
    
    // IRETQ expects stack: [SS, RSP, RFLAGS, CS, RIP]
    asm!(
        "mov ds, {ds_val:r}", // Also set DS/ES to user data segment
        "mov es, {ds_val:r}",
        "push {ss}",
        "push {rsp}",
        "push {rflags}",
        "push {cs}",
        "push {rip}",
        "iretq",
        ds_val = in(reg) user_ds,
        ss = in(reg) user_ds,  // u64
        rsp = in(reg) user_stack_top.as_u64(), 
        rflags = in(reg) rflags,
        cs = in(reg) user_cs,   // u64
        rip = in(reg) entry_point.as_u64(),
        options(noreturn)
    );
}
