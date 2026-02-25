use core::arch::asm;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Size4KiB};
use x86_64::VirtAddr;

/// IRETQ ile Ring 3'e geçiş yapar.
/// Bu fonksiyon geri dönmez.
///
/// # Güvenlik
/// `iretq` ayrıcalık seviyesi değiştirir.
/// `entry_point` ve `user_stack_top` kullanıcı modunda erişilebilir olmalıdır.
pub unsafe fn enter_user_mode(entry_point: VirtAddr, user_stack_top: VirtAddr) -> ! {
    let kernel_stack_top = crate::task::scheduler::current_kernel_stack_top();
    let kernel_stack_top = VirtAddr::new(kernel_stack_top);
    crate::gdt::set_kernel_stack(kernel_stack_top);
    crate::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top.as_u64());
    let user_cs = crate::gdt::user_code_selector().0;
    let user_ds = crate::gdt::user_data_selector().0;

    // RPL = 3 (Ring 3)
    let user_cs = user_cs | 3;
    let user_ds = user_ds | 3;

    // RFLAGS: Interrupt açık (bit 9) ve reserved (bit 1) -> 0x202
    // IOPL=0 (kullanıcı modunda IO portu kullanımını engeller)
    let rflags: u64 = 0x202;
    crate::serial_println!(
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
    let kernel_stack_top = crate::task::scheduler::current_kernel_stack_top();
    let kernel_stack_top = VirtAddr::new(kernel_stack_top);
    crate::gdt::set_kernel_stack(kernel_stack_top);
    crate::syscall::set_kernel_stack_for_current_cpu(kernel_stack_top.as_u64());
    let user_cs = crate::gdt::user_code_selector().0;
    let user_ds = crate::gdt::user_data_selector().0;

    let user_cs = user_cs | 3;
    let user_ds = user_ds | 3;

    let rflags: u64 = 0x202;
    crate::serial_println!(
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

pub fn fork_child_start() -> ! {
    if let Some((entry, stack)) = crate::task::scheduler::current_user_target() {
        unsafe { enter_user_mode_with_ret(VirtAddr::new(entry), VirtAddr::new(stack), 0) }
    }
    crate::task::scheduler::idle_loop()
}

/// ELF imajını yükler ve Ring 3'e geçer.
pub fn enter_user_elf(
    image: &[u8],
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), crate::elf::ElfError> {
    let address_space = crate::memory::create_address_space(image);
    crate::memory::set_active_address_space(Some(address_space));
    let user = crate::elf::load_user_elf(image, mapper, frame_allocator)?;
    unsafe { enter_user_mode(user.entry, user.stack_top) }
}

/// Global bellek yöneticisi ile ELF'i yükler ve Ring 3'e geçer.
pub fn enter_user_elf_from_image(image: &[u8]) -> Result<(), crate::elf::ElfError> {
    let frame_allocator = unsafe {
        crate::memory::global_memory_manager_mut().ok_or(crate::elf::ElfError::Unsupported)?
    };
    let address_space = crate::memory::create_address_space(image);
    crate::memory::set_active_address_space(Some(address_space));
    let user_pml4 = crate::memory::create_user_pml4().ok_or(crate::elf::ElfError::Unsupported)?;
    let pml4_phys = user_pml4.start_address().as_u64();
    let phys_offset = crate::memory::active_physical_offset();
    let pml4_virt = VirtAddr::new(phys_offset + pml4_phys);
    let table = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(phys_offset)) };
    let user = crate::elf::load_user_elf(image, &mut mapper, frame_allocator)?;
    unsafe {
        Cr3::write(user_pml4, Cr3Flags::empty());
    }
    unsafe { enter_user_mode(user.entry, user.stack_top) }
}
