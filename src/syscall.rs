use core::arch::global_asm;
use x86_64::registers::model_specific::Msr;
use x86_64::VirtAddr;

const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_SFMASK: u32 = 0xC000_0084;
const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

#[repr(C)]
pub struct CpuData {
    pub user_rsp_scratch: u64,
    pub kernel_stack_top: u64,
    pub cpu_id: u32,
    pub user_rip: u64,
    pub user_rflags: u64,
    pub irq_depth: u64,
}

pub const SYSCALL_STACK_SIZE: usize = 40960;
pub fn init() {
    let code_selector = crate::gdt::kernel_code_selector();
    let user_code_selector = crate::gdt::user_code_selector();

    unsafe {
        let mut efer = Msr::new(MSR_EFER);
        let mut efer_val = efer.read();
        efer_val |= 1 << 0; // Bit 0: SCE (SYSCALL/SYSRET enable)
        efer.write(efer_val);

        let kernel_cs = code_selector.0 as u64;
        let user_cs = (user_code_selector.0 | 3) as u64;
        let star_val = ((user_cs - 16) << 48) | (kernel_cs << 32);
        let mut star = Msr::new(MSR_STAR);
        star.write(star_val);

        let syscall_addr = VirtAddr::new(syscall_handler as *const () as usize as u64);
        let mut lstar = Msr::new(MSR_LSTAR);
        lstar.write(syscall_addr.as_u64());

        let mut sfmask = Msr::new(MSR_SFMASK);
        sfmask.write(0x200); // Bit 9: IF (interrupt flag mask)
    }

    crate::serial_println!("Syscall Mechanism Initialized.");
}

pub unsafe fn init_cpu_data(cpu_data: *mut CpuData) {
    let mut kernel_gs_base = Msr::new(MSR_KERNEL_GS_BASE);
    kernel_gs_base.write(cpu_data as u64);
}

pub fn set_kernel_stack_for_current_cpu(stack_top: u64) {
    unsafe {
        let mut msr = Msr::new(MSR_KERNEL_GS_BASE);
        let base = msr.read();
        let data = base as *mut CpuData;
        (*data).kernel_stack_top = stack_top;
    }
}

pub fn current_user_context() -> (u64, u64, u64) {
    unsafe {
        let mut msr = Msr::new(MSR_KERNEL_GS_BASE);
        let base = msr.read();
        let data = base as *const CpuData;
        (
            (*data).user_rsp_scratch,
            (*data).user_rip,
            (*data).user_rflags,
        )
    }
}

global_asm!(
    r#"
.global syscall_handler

.section .text
syscall_handler:
    // SWAPGS: user GS_BASE <-> KERNEL_GS_BASE, per-CPU yapıya geçiş
    swapgs
    // Kullanıcı RSP'yi GS:0 scratch alanına kaydet
    mov qword ptr gs:[0], rsp
    mov qword ptr gs:[24], rcx
    mov qword ptr gs:[32], r11
    // Kernel stack top'u GS:8'den yükle
    mov rsp, qword ptr gs:[8]

    // Register preservation: callee-saved + SYSRET zorunlu RCX/R11
    push rcx
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    // Stack alignment: call öncesi RSP % 16 == 8 olacak şekilde ayarlanır
    mov r11, rdi
    mov rdi, rax
    mov rax, rsi
    mov rsi, r11
    mov r11, rdx
    mov rdx, rax
    mov rcx, r11
    mov r11, r9
    mov r9, r8
    mov r8, r10
    sub rsp, 8
    mov [rsp], r11
    call syscall_dispatcher
    add rsp, 8

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    pop r11
    pop rcx
    
    mov rsp, qword ptr gs:[0]
    swapgs
    sysretq
"#
);

extern "C" {
    fn syscall_handler();
}

#[no_mangle]
pub extern "sysv64" fn syscall_dispatcher(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
) -> i64 {
    let ret = crate::posix::dispatch(
        num as usize,
        [
            a1 as usize,
            a2 as usize,
            a3 as usize,
            a4 as usize,
            a5 as usize,
            a6 as usize,
        ],
    );
    ret as i64
}
