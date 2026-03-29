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
    let user_cs = arch::gdt::user_code_selector().0;
    let user_ds = arch::gdt::user_data_selector().0;

    // RPL = 3 (Ring 3 — kullanıcı ayrıcalık seviyesi)
    let user_cs = user_cs | 3;
    let user_ds = user_ds | 3;

    // RFLAGS: Kesme bayrağı açık (bit 9) ve rezerv bit (bit 1) → 0x202
    // IOPL=0: kullanıcı modunda doğrudan G/Ç portu erişimini engeller
    let rflags: u64 = 0x202;
    serial_println!(
        "SWITCHING TO USER MODE now... RIP={:#x} RSP={:#x}",
        entry_point.as_u64(),
        user_stack_top.as_u64()
    );

    // IRETQ stack düzeni: [SS, RSP, RFLAGS, CS, RIP]
    asm!(
        "mov ds, {ds_val:r}", // DS/ES kullanıcı data segmenti olur
        "mov es, {ds_val:r}",
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        "iretq",
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
    let kernel_stack_top = VirtAddr::new(kernel_stack_top);
    arch::gdt::set_kernel_stack(kernel_stack_top);
    arch::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top.as_u64());
    let user_cs = arch::gdt::user_code_selector().0;
    let user_ds = arch::gdt::user_data_selector().0;

    let user_cs = user_cs | 3;
    let user_ds = user_ds | 3;

    let rflags: u64 = 0x202;
    serial_println!(
        "SWITCHING TO USER MODE now... RIP={:#x} RSP={:#x}",
        entry_point.as_u64(),
        user_stack_top.as_u64()
    );

    asm!(
        "mov rax, {ret}",
        "mov ds, {ds_val:r}",
        "mov es, {ds_val:r}",
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        "iretq",
        ds_val = in(reg) user_ds,
        ss = in(reg) u64::from(user_ds),
        rsp = in(reg) user_stack_top.as_u64(),
        rflags = in(reg) rflags,
        cs = in(reg) u64::from(user_cs),
        rip = in(reg) entry_point.as_u64(),
        ret = in(reg) ret,
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
        "mov rax, {rax_seed}",
        "mov rcx, {rcx_seed}",
        "mov ds, {ds_val:r}",
        "mov es, {ds_val:r}",
        "push {ss:r}",
        "push {rsp}",
        "push {rflags}",
        "push {cs:r}",
        "push {rip}",
        "iretq",
        rax_seed = in(reg) rax_seed,
        rcx_seed = in(reg) thread.initial_rcx,
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
    if let Some((entry, stack)) = tasking::scheduler::current_user_target() {
        unsafe { enter_user_mode_with_ret(VirtAddr::new(entry), VirtAddr::new(stack), 0) }
    }
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
