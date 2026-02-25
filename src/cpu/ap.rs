use crate::syscall::{init_cpu_data, CpuData};
use x86_64::VirtAddr;

#[no_mangle]
pub extern "sysv64" fn ap_entry(cpu_data: &'static mut CpuData) -> ! {
    // CRITICAL: Initialize per-CPU data FIRST (required for per-CPU access)
    unsafe {
        init_cpu_data(cpu_data as *mut CpuData);
    }

    let cpu_id = cpu_data.cpu_id;
    
    // CRITICAL ORDER: We must load the GDT BEFORE the IDT!
    // The early boot AP trampoline uses CS=0x18. The kernel GDT uses CS=0x08,
    // and defines 0x18 as the TSS. If IDT is created while CS=0x18, all interrupt
    // gates will point to the TSS selector in the new GDT, causing #GP(0x18) on any interrupt.
    crate::gdt::init_for_cpu(cpu_id, VirtAddr::new(cpu_data.kernel_stack_top));

    unsafe {
        // Manually load IDT for this CPU using the cpu_id from cpu_data
        let idt = crate::interrupts::init_idt_for_cpu(cpu_id);
        idt.load();
    }

    // Now safe to perform operations that might trigger exceptions
    // RAW UART DEBUG: Print 'A' directly to COM1 (0x3f8)
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'A');
    }
    // RAW UART DEBUG: Print 'B' directly to COM1 (0x3f8)
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'B');
    }

    // We already know our CPU ID from cpu_data. 
    // We can get our APIC ID from the global state instead of doing MMIO/MSR reads during early boot.
    let apic_id = {
        let mut id = cpu_id;
        if let Some(state) = crate::cpu::smp::SMP_STATE.try_lock() {
            id = state.cpu_apic_ids.get(cpu_id as usize).copied().unwrap_or(cpu_id);
        }
        id
    };

    // RAW UART DEBUG: Print 'C' directly to COM1 (0x3f8)
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'C');
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'D');
    }
    let _ = crate::apic::lapic::init();
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'E');
    }
    // Set GS_BASE to point to cpu_data for kernel usage (critical for per-cpu data access)
    unsafe {
        use x86_64::registers::model_specific::Msr;
        Msr::new(0xC0000101).write(cpu_data as *mut CpuData as u64);
    }
    // Note: GDT and IDT already loaded at the beginning - no need to call init_per_cpu() again
    // Note: IDT already loaded at the beginning - no need to call init_per_cpu() again
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'G');
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'W');
    }
    crate::cpu::smp::mark_cpu_online(cpu_id, apic_id);
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'H');
    }
    unsafe {
        x86_64::instructions::interrupts::enable();
    }
    unsafe {
        crate::debug::serial::EMERGENCY_SERIAL.write_byte(b'I');
    }
    crate::task::scheduler::idle_loop();
}
