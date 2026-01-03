//! # echOS Interrupt Descriptor Table (IDT)
//! 
//! CPU exception ve interrupt handler'larını tanımlar.
//! Double fault ve page fault gibi kritik hataları yakalar.

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use crate::cpu::gdt;
use crate::serial_println;
use spin::Lazy;

/// Global IDT nesnesi
pub static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    
    // Exception Handlers
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault.set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    
    // Hardware Interrupt Handlers
    idt[crate::cpu::interrupts::InterruptIndex::Timer.as_u8()]
        .set_handler_fn(crate::cpu::interrupts::timer_interrupt_handler);
    idt[crate::cpu::interrupts::InterruptIndex::Keyboard.as_u8()]
        .set_handler_fn(crate::cpu::interrupts::keyboard_interrupt_handler);

    idt
});

/// IDT'yi CPU'ya yükler.
pub fn init() {
    IDT.load();
}

/// Breakpoint Exception (#BP) handler
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

/// Double Fault Exception (#DF) handler.
/// Kurtarılamaz hatadır, panic ile sistemi durdurur.
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// General Protection Fault (#GP) handler.
extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64)
{
    serial_println!("EXCEPTION: GENERAL PROTECTION FAULT\nError Code: {}\n{:#?}", error_code, stack_frame);
    loop {}
}

use x86_64::structures::idt::PageFaultErrorCode;

/// Page Fault Exception (#PF) handler.
/// Hatalı bellek erişimlerinde tetiklenir.
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    serial_println!("EXCEPTION: PAGE FAULT");
    serial_println!("Erişilen Adres: {:?}", Cr2::read());
    serial_println!("Hata Kodu: {:?}", error_code);
    serial_println!("{:#?}", stack_frame);
    loop {}
}
