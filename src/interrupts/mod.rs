//! # echOS Interrupt Yönetimi
//! 
//! Bu modül, x86_64 exception ve hardware interrupt'larını yönetir.
//! IDT (Interrupt Descriptor Table) ve PIC (Programmable Interrupt Controller) yapılandırması.

pub mod idt;
pub mod pic;

use x86_64::structures::idt::InterruptDescriptorTable;
use lazy_static::lazy_static;
use spin::Mutex;

// ============================================================================
// IDT YAPISI
// ============================================================================

lazy_static! {
    /// Global Interrupt Descriptor Table.
    /// Exception ve IRQ handler'larını içerir.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // Exception handlers (CPU hataları)
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        
        unsafe {
            // Double Fault için özel stack (IST)
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
            
            // Page Fault için özel stack
            idt.page_fault.set_handler_fn(page_fault_handler)
                .set_stack_index(crate::gdt::PAGE_FAULT_IST_INDEX);
                
            // General Protection Fault için özel stack
            idt.general_protection_fault.set_handler_fn(general_protection_fault_handler)
                .set_stack_index(crate::gdt::GENERAL_PROTECTION_IST_INDEX);
        }
        
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        
        // Hardware interrupts (IRQs)
        idt[32].set_handler_fn(timer_interrupt_handler);    // IRQ0 - Timer
        idt[33].set_handler_fn(keyboard_interrupt_handler); // IRQ1 - Keyboard
        idt[44].set_handler_fn(mouse_interrupt_handler);    // IRQ12 - Mouse
        
        idt
    };
}

/// Interrupt sistemini başlatır.
/// IDT'yi yükler ve PIC'i yapılandırır.
pub fn init() {
    IDT.load();
    pic::init();
    crate::serial_println!("Interrupts initialized");
}

// ============================================================================
// EXCEPTION HANDLERS (CPU Hataları)
// ============================================================================

use x86_64::structures::idt::InterruptStackFrame;

/// Sıfıra bölme hatası (Divide by Zero)
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: DIVIDE ERROR\n{:#?}", stack_frame);
}

/// Debug breakpoint (INT 3)
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

use x86_64::structures::idt::PageFaultErrorCode;

/// Sayfa hatası (Page Fault)
/// User mode'da oluşursa task sonlandırılır, kernel mode'da panic.
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    
    let cs = stack_frame.code_segment;
    if (cs.0 & 3) == 3 {
        // User mode hatası - task'ı öldür
        crate::serial_println!("Page Fault in User Mode (Addr: {:?}). Killing task.", Cr2::read());
        crate::task::scheduler::exit();
    } else {
        // Kernel mode hatası - panic
        crate::serial_println!("EXCEPTION: PAGE FAULT");
        crate::serial_println!("Accessed Address: {:?}", Cr2::read());
        crate::serial_println!("Error Code: {:?}", error_code);
        crate::serial_println!("{:#?}", stack_frame);
        panic!("Page fault");
    }
}

/// Çift hata (Double Fault) - kurtarılamaz
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// Genel koruma hatası (General Protection Fault)
extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    if (cs.0 & 3) == 3 {
        // User mode hatası - task'ı öldür
        crate::serial_println!("GPF in User Mode (RIP={:#x}). Killing task.", stack_frame.instruction_pointer);
        crate::task::scheduler::exit();
    } else {
        panic!("EXCEPTION: GENERAL PROTECTION FAULT (code: {})\n{:#?}", error_code, stack_frame);
    }
}

/// Geçersiz opcode hatası (#UD)
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    crate::serial_println!("EXCEPTION: INVALID OPCODE (#UD) at RIP={:#x}", rip);
    crate::serial_println!("{:#?}", stack_frame);
    panic!("Invalid Opcode");
}

// ============================================================================
// HARDWARE INTERRUPT HANDLERS (IRQs)
// ============================================================================

/// Sistem tick sayacı
static TICKS: Mutex<u64> = Mutex::new(0);

/// Timer interrupt handler (IRQ0)
/// Her tick'te scheduler'ı çağırır.
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    *TICKS.lock() += 1;
    
    // Scheduler'a tick bildir
    crate::task::scheduler::tick();
    
    unsafe {
        pic::PICS.lock().notify_end_of_interrupt(32);
    }
}

// Keyboard state
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

lazy_static! {
    /// PS/2 Keyboard decoder
    static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    };
}

/// Keyboard interrupt handler (IRQ1)
/// Scancode'u decode eder ve input queue'ya ekler.
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    let result = KEYBOARD.lock().add_byte(scancode);
    match result {
        Ok(Some(key_event)) => {
            if let Some(key) = KEYBOARD.lock().process_keyevent(key_event) {
                use crate::drivers::input::InputEvent;
                crate::drivers::input::push_event(InputEvent::Keyboard(key));
            }
        }
        Ok(None) => {}
        Err(_) => {}
    }
    
    unsafe {
        pic::PICS.lock().notify_end_of_interrupt(33);
    }
}

/// Mouse interrupt handler (IRQ12)
/// Raw byte'ı input queue'ya ekler (Fast-Path).
extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    use crate::drivers::input::InputEvent;
    
    let mut data = Port::<u8>::new(0x60);
    let byte = unsafe { data.read() };
    
    crate::drivers::input::push_event(InputEvent::MouseByte(byte));
    
    unsafe {
        pic::PICS.lock().notify_end_of_interrupt(44);
    }
}

/// Toplam geçen tick sayısını döndürür.
pub fn get_ticks() -> u64 {
    *TICKS.lock()
}
