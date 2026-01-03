use x86_64::registers::model_specific::{Efer, EferFlags, LStar, Star, SFMask};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;
use x86_64::instructions::segmentation::Segment;
use x86_64::structures::gdt::SegmentSelector;

pub fn init() {
    let code_selector = crate::gdt::GDT.1.code_selector;
    let data_selector = crate::gdt::GDT.1.data_selector;
    let user_code_selector = crate::gdt::GDT.1.user_code_selector;
    let user_data_selector = crate::gdt::GDT.1.user_data_selector;

    unsafe {
        // 1. Enable System Call Extensions (SCE)
        Efer::update(|flags| {
            flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        });

        // 2. Set STAR register
        // CS (User Code) must be Base + 16 (0x10)
        // SS (User Data) must be Base + 8  (0x08)
        
        // GDT layout in src/gdt.rs:
        // [1] Kernel Code
        // [2] Kernel Data
        // [3] TSS
        // [5] User Data  -> Index 5
        // [6] User Code  -> Index 6
        
        // For Sysret:
        // Base = User Data Index (5) - 1 = 4.
        // CS = Base + 16 = 4*8 + 16 = 32+16 = 48 (Index 6). Checking logic:
        // x86_64 logic for segments: Base Selector (16 bits) << 32 in MSR.
        // The MSR value is actually strictly bits 48-63 = SYSRET CS, bits 32-47 = SYSCALL CS.
        // No, Star::write handles the shift.
        // But the selectors we pass determine the value.
        // We need `user_base_selector` such that:
        //   user_base + 16 = user_code
        //   user_base + 8  = user_data
        
        // user_data.0 (raw) = 5 * 8 = 40.
        // user_code.0 (raw) = 6 * 8 = 48.
        
        // If user_base.0 = 40 - 8 = 32 (Index 4).
        // Then user_base + 8 = 40 (User Data). Correct.
        // Then user_base + 16 = 48 (User Code). Correct.
        
        let user_base_val = user_data_selector.0 - 8;
        let user_base_selector = SegmentSelector(user_base_val);
        
        // Kernel Base:
        // SYSCALL loads CS from Bits 32-47. And SS from Bits 32-47 + 8.
        // So Kernel CS = Base.
        // Kernel SS = Base + 8.
        // Kernel Code = Index 1 (8).
        // Kernel Data = Index 2 (16).
        // Base = 8.
        // Base + 8 = 16. Correct.
        
        Star::write(
            user_base_selector,
            user_base_selector,
            code_selector,
            data_selector,
        ).unwrap_or_else(|e| {
             crate::serial_println!("Syscall Star::write failed: {:?} (Selectors: UserDataIdx={}, UserCodeIdx={}, BaseIdx={})", 
                e, user_data_selector.index(), user_code_selector.index(), user_base_selector.index());
             
             // UNWRAP FAILED? USE THE HAMMER.
             // Manually write to MSR 0xC0000081 (STAR)
             // Bits 0-31:  Target EIP (Legacy - Unused in Long Mode)
             // Bits 32-47: Kernel CS (Target for SYSCALL) -> code_selector
             // Bits 48-63: User CS Base (Target for SYSRET) -> user_base_selector
             
             use x86_64::registers::model_specific::Msr;
             let mut star_msr = Msr::new(0xC0000081);
             
             let kernel_base = code_selector.0 as u64;
             let user_base = user_base_selector.0 as u64;
             
             let val = (user_base << 48) | (kernel_base << 32);
             
             crate::serial_println!("Forcing STAR MSR Write: {:#016x}", val);
             star_msr.write(val);
        });

        // 3. Set LSTAR register (Entry Point)
        let syscall_addr = VirtAddr::new(syscall_handler as *const () as usize as u64);
        LStar::write(syscall_addr);

        // 4. Set FMASK register (Flags to clear on syscall)
        // Clear Interrupt flag (IF) to disable interrupts on entry
        SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
    }
    
    crate::serial_println!("Syscall Mechanism Initialized.");
}

// Global storage for stack switching (Single Core Only!)
// In a multicore system, this must be per-cpu (GS_BASE).
use core::arch::global_asm;

global_asm!(r#"
.global syscall_handler
.section .bss
    .align 16
    global_user_rsp_backup: .skip 8
    global_kernel_stack: .skip 40960
    global_kernel_stack_top:

.section .text
syscall_handler:
    // SYSCALL entry point
    // RCX = return RIP
    // R11 = legacy RFLAGS
    // Kernel Mode active (Ring 0), but RSP is User Stack!
    
    // 1. Save User Stack Pointer
    mov [global_user_rsp_backup + rip], rsp
    
    // 2. Switch to Kernel Stack
    lea rsp, [global_kernel_stack_top + rip]
    
    // 3. Save User Context (RCX=RIP, R11=RFLAGS)
    push rcx // Return RIP
    push r11 // RFLAGS
    push rbp
    
    // 4. Setup Argument handling (System V AMD64 ABI)
    // User: RDI, RSI, RDX, R10, R8, R9
    // Kernel: RDI, RSI, RDX, RCX, R8, R9  (R10 -> RCX)
    mov rcx, r10 
    
    // 5. Call Rust Dispatcher
    call syscall_rust_dispatcher
    
    // 6. Restore Context
    pop rbp
    pop r11
    pop rcx
    
    // 7. Restore User Stack
    mov rsp, [global_user_rsp_backup + rip]
    
    // 8. Return to User Mode (Ring 3)
    sysretq
"#);

extern "C" {
    fn syscall_handler();
}

#[no_mangle]
extern "C" fn syscall_rust_dispatcher() {
    crate::serial_println!("SYSCALL TRIGGERED! (In Ring 0)");
}
