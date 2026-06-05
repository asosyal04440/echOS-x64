//! # echOS Kullanıcı Modu Yürütme (User Mode Task Execution)
//!
//! Bu modül, çekirdek modundan (Ring 0) kullanıcı moduna (Ring 3) geçiş
//! mekanizmasını sağlar. IRETQ komutu ile ayrıcalık seviyesi değiştirilir.
//!
//! ## Ring 0 → Ring 3 Geçiş Mekanizması (IRETQ)
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────┐
//!  │             IRETQ İLE KULLANICI MODUNA GEÇİŞ                │
//!  │                                                              │
//!  │  Kernel Stack (Ring 0):                                      │
//!  │  ┌───────────┐                                               │
//!  │  │   SS      │ ← Kullanıcı veri segmenti (RPL=3)            │
//!  │  │   RSP     │ ← Kullanıcı yığın işaretçisi (stack_top)     │
//!  │  │  RFLAGS   │ ← 0x202 (IF=1, IOPL=0)                      │
//!  │  │   CS      │ ← Kullanıcı kod segmenti (RPL=3)             │
//!  │  │   RIP     │ ← Kullanıcı giriş noktası (entry_point)      │
//!  │  └───────────┘                                               │
//!  │       ↓ iretq                                                │
//!  │  CPU: Ring 0 → Ring 3 geçişi gerçekleşir                    │
//!  │  Segment register'ları kullanıcı segmentlerine ayarlanır     │
//!  │  RSP = user_stack_top, RIP = entry_point                     │
//!  └──────────────────────────────────────────────────────────────┘
//!
//!  NOT: IOPL=0 ile kullanıcı modu, IN/OUT gibi G/Ç komutlarını
//!       çalıştıramaz. Sistem çağrısı (SYSCALL) kullanması gerekir.
//! ```

use super::super::{elf, kernel, serial_println};
use core::arch::asm;
use kernel::{arch, memory as kernel_memory, tasking};
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Size4KiB};
use x86_64::VirtAddr;

const MSR_GS_BASE: u32 = 0xC000_0101;

/// IRETQ ile Ring 3'e geçiş yapar.
/// Bu fonksiyon geri dönmez.
///
/// # Güvenlik
/// `iretq` ayrıcalık seviyesi değiştirir.
/// `entry_point` ve `user_stack_top` kullanıcı modunda erişilebilir olmalıdır.
pub unsafe fn enter_user_mode(entry_point: VirtAddr, user_stack_top: VirtAddr) -> ! {
    let kernel_stack_top = tasking::scheduler::current_kernel_stack_top();
    let kernel_stack_top = VirtAddr::new(kernel_stack_top);
    arch::gdt::set_kernel_stack(kernel_stack_top);
    arch::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top.as_u64());
    let user_cs = arch::gdt::user_code_selector().0 | 3;
    let user_ds = arch::gdt::user_data_selector().0 | 3;
    let rflags: u64 = 0x202;

    let user_pml4 = match tasking::scheduler::current_user_page_table() {
        Some(user_pml4) => user_pml4,
        None => {
            serial_println!(
                "[FATAL] enter_user_mode: NO USER PML4! RIP={:#x} RSP={:#x}",
                entry_point.as_u64(),
                user_stack_top.as_u64()
            );
            loop {
                x86_64::instructions::hlt();
            }
        }
    };

    let user_pml4_addr = user_pml4.start_address().as_u64();
    let old_cr3 = Cr3::read().0.start_address().as_u64();

    serial_println!(
        "SWITCHING TO USER MODE now... RIP={:#x} RSP={:#x} CR3:{:#x}->{:#x}",
        entry_point.as_u64(),
        user_stack_top.as_u64(),
        old_cr3,
        user_pml4_addr
    );

    asm!(
        // Load user data segments (GDT accessible via kernel page table)
        "mov ds, {ds_val:r}",
        "mov es, {ds_val:r}",
        // Build iretq stack frame on kernel stack (kernel page table active)
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        // CR3 switch LAST — kernel code at low-half addresses is mapped in
        // user PML4 (PD[0..7] without USER_ACCESSIBLE) for this trampoline.
        "mov cr3, {cr3_val}",
        "iretq",
        cr3_val = in(reg) user_pml4_addr,
        ds_val = in(reg) user_ds,
        ss = in(reg) u64::from(user_ds),
        rsp = in(reg) user_stack_top.as_u64(),
        rflags = in(reg) rflags,
        cs = in(reg) u64::from(user_cs),
        rip = in(reg) entry_point.as_u64(),
        options(noreturn)
    );
}

pub unsafe fn enter_user_mode_with_ret(
    entry_point: VirtAddr,
    user_stack_top: VirtAddr,
    ret: u64,
) -> ! {
    let kernel_stack_top = tasking::scheduler::current_kernel_stack_top();
    let kernel_stack_top_va = VirtAddr::new(kernel_stack_top);
    arch::gdt::set_kernel_stack(kernel_stack_top_va);
    arch::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top);

    let user_cs = arch::gdt::user_code_selector().0 | 3;
    let user_ds = arch::gdt::user_data_selector().0 | 3;
    let rflags: u64 = 0x202;

    let user_pml4 = match tasking::scheduler::current_user_page_table() {
        Some(user_pml4) => user_pml4,
        None => {
            serial_println!(
                "[FATAL] enter_user_mode_with_ret: NO USER PML4 for PID={:?}! User-space will #PF on first instruction.",
                tasking::scheduler::current_task_id()
            );
            loop {
                x86_64::instructions::hlt();
            }
        }
    };

    let user_pml4_addr = user_pml4.start_address().as_u64();
    let old_cr3 = Cr3::read().0.start_address().as_u64();

    let mut gdtr = x86_64::instructions::tables::DescriptorTablePointer {
        limit: 0,
        base: VirtAddr::new(0),
    };
    let mut idtr = x86_64::instructions::tables::DescriptorTablePointer {
        limit: 0,
        base: VirtAddr::new(0),
    };
    let mut tr_sel: u16 = 0;
    unsafe {
        asm!("sgdt [{}]", in(reg) &mut gdtr);
        asm!("sidt [{}]", in(reg) &mut idtr);
        asm!("str {:x}", out(reg) tr_sel);
    }
    let tss_rsp0 = arch::gdt::current_tss_rsp0();

    serial_println!(
        "[PRE_IRETQ] GDTR={:#x}+{} IDTR={:#x}+{} TR={:#x} TSS.RSP0={:#x}",
        gdtr.base.as_u64(),
        gdtr.limit,
        idtr.base.as_u64(),
        idtr.limit,
        tr_sel,
        tss_rsp0
    );
    crate::debug_diag!(
        "[PRE_IRETQ] CS={:#x} SS={:#x} RIP={:#x} RSP={:#x} RFLAGS={:#x}",
        user_cs,
        user_ds,
        entry_point.as_u64(),
        user_stack_top.as_u64(),
        rflags
    );

    crate::debug_diag!(
        "[SHELL_TEST] enter_user_mode_with_ret: Before CR3 switch: GDTR.base={:#x} GDTR.limit={} IDTR.base={:#x} IDTR.limit={} TR={:#x} TSS.RSP0={:#x}",
        gdtr.base.as_u64(),
        gdtr.limit,
        idtr.base.as_u64(),
        idtr.limit,
        tr_sel,
        tss_rsp0
    );
    crate::debug_diag!(
        "[SHELL_TEST] enter_user_mode_with_ret: switching CR3 to {:#x}, RIP={:#x} RSP={:#x}",
        user_pml4_addr,
        entry_point.as_u64(),
        user_stack_top.as_u64()
    );

    crate::debug_diag!(
        "[SHELL_TEST] enter_user_mode_with_ret: Switching CR3 and executing iretq with RIP={:#x} RSP={:#x} CS={:#x} SS={:#x} RAX={:#x}...",
        entry_point.as_u64(),
        user_stack_top.as_u64(),
        user_cs,
        user_ds,
        ret
    );

    asm!(
        // Load user data segments (GDT accessible via kernel page table)
        "mov ds, {ds_val:r}",
        "mov es, {ds_val:r}",
        // Build iretq stack frame on kernel stack (kernel page table active)
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        // Debugcon marker: 'I' = about to CR3+iretq
        "mov dx, 0xe9",
        "mov al, 0x49",
        "out dx, al",
        // CR3 switch LAST — kernel code at low-half addresses is mapped in
        // user PML4 (PD[0..7] without USER_ACCESSIBLE) for this trampoline.
        // After this, only iretq can execute (already fetched/accessible).
        "mov cr3, {cr3_val}",
        // Debugcon marker: 'J' = CR3 switched, about to iretq
        "mov dx, 0xe9",
        "mov al, 0x4a",
        "out dx, al",
        "iretq",
        cr3_val = in(reg) user_pml4_addr,
        ds_val = in(reg) user_ds,
        ss = in(reg) u64::from(user_ds),
        rsp = in(reg) user_stack_top.as_u64(),
        rflags = in(reg) rflags,
        cs = in(reg) u64::from(user_cs),
        rip = in(reg) entry_point.as_u64(),
        in("rax") ret,
        options(noreturn)
    );
}

pub unsafe fn enter_win32_user_mode(thread: tasking::task::Win32ThreadState, rax_seed: u64) -> ! {
    let kernel_stack_top = tasking::scheduler::current_kernel_stack_top();
    let kernel_stack_top = VirtAddr::new(kernel_stack_top);
    arch::gdt::set_kernel_stack(kernel_stack_top);
    arch::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top.as_u64());
    Msr::new(MSR_GS_BASE).write(thread.teb_base);
    let user_cs = arch::gdt::user_code_selector().0 | 3;
    let user_ds = arch::gdt::user_data_selector().0 | 3;
    let rflags: u64 = 0x202;

    asm!(
        "mov ds, {ds_val:r}",
        "mov es, {ds_val:r}",
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        "iretq",
        in("rax") rax_seed,
        in("rcx") thread.initial_rcx,
        ds_val = in(reg) user_ds,
        ss = in(reg) u64::from(user_ds),
        rsp = in(reg) thread.user_stack_top,
        rflags = in(reg) rflags,
        cs = in(reg) u64::from(user_cs),
        rip = in(reg) thread.entry_rip,
        options(noreturn)
    );
}

pub fn fork_child_start() -> ! {
    crate::debug_diag!("[DIAG] fork_child_start entered, PID={:?}", tasking::scheduler::current_task_id());
    crate::debug_diag!("[SHELL_TEST] fork_child_start entered");
    {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let cr3 = unsafe { Cr3::read().0.start_address().as_u64() };
        crate::debug_diag!(
            "[SHELL_TEST] fork_child_start: CPU={} PID={:?} CR3={:#x}",
            cpu_id,
            tasking::scheduler::current_task_id(),
            cr3
        );
    }
    if let Some((entry, stack)) = tasking::scheduler::current_user_target() {
        crate::debug_diag!("[DIAG] fork_child_start: user_entry={:#x} user_stack={:#x} — calling enter_user_mode_with_ret", entry, stack);
        crate::debug_diag!(
            "[SHELL_TEST] fork_child_start: entry={:#x} stack={:#x} — calling enter_user_mode_with_ret",
            entry,
            stack
        );
        unsafe { enter_user_mode_with_ret(VirtAddr::new(entry), VirtAddr::new(stack), 0) }
    }
    crate::debug_diag!("[DIAG] fork_child_start: NO user_target, idling");
    tasking::scheduler::idle_loop()
}

/// ELF imajını yükler ve Ring 3'e geçer.
pub fn enter_user_elf(
    image: &[u8],
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), elf::ElfError> {
    let address_space = kernel_memory::create_address_space(image);
    kernel_memory::set_active_address_space(Some(address_space));
    let user = elf::load_user_elf(image, mapper, frame_allocator)?;
    unsafe { enter_user_mode(user.entry, user.stack_top) }
}

/// Global bellek yöneticisi ile ELF'i yükler ve Ring 3'e geçer.
pub fn enter_user_elf_from_image(image: &[u8]) -> Result<(), elf::ElfError> {
    let frame_allocator =
        unsafe { kernel_memory::global_memory_manager_mut().ok_or(elf::ElfError::Unsupported)? };
    let address_space = kernel_memory::create_address_space(image);
    kernel_memory::set_active_address_space(Some(address_space));
    let user_pml4 = kernel_memory::create_user_pml4().ok_or(elf::ElfError::Unsupported)?;
    let pml4_phys = user_pml4.start_address().as_u64();
    let phys_offset = kernel_memory::active_physical_offset();
    let pml4_virt = VirtAddr::new(phys_offset + pml4_phys);
    let table = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(phys_offset)) };
    let user = elf::load_user_elf(image, &mut mapper, frame_allocator)?;
    unsafe {
        Cr3::write(user_pml4, Cr3Flags::empty());
    }
    unsafe { enter_user_mode(user.entry, user.stack_top) }
}
