//! # echOS Kesme (Interrupt) TanÄ±mlarÄ±
//!
//! DonanÄ±m kesme iÅŸleyicileri, PIC ofseti sabitleri ve vektÃ¶r dizin enum'u.

use crate::platform::interrupt_abi::KernelTrapFrameView;
use pic8259::ChainedPics;
use spin::Mutex;

#[cfg(any(target_os = "none", target_os = "uefi"))]
use x86_64::structures::idt::InterruptStackFrame;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

fn timer_interrupt_logic(_frame: KernelTrapFrameView) {
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

fn keyboard_interrupt_logic(_frame: KernelTrapFrameView) {
    #[cfg(any(target_os = "none", target_os = "uefi"))]
    {
        use x86_64::instructions::port::Port;

        let mut port = Port::new(0x60);
        let _scancode: u8 = unsafe { port.read() };
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub extern "x86-interrupt" fn timer_interrupt_handler(stack_frame: InterruptStackFrame) {
    let _gs_guard = crate::cpu::local::KernelGsGuard::from_interrupted_cs(stack_frame.code_segment);
    timer_interrupt_logic(KernelTrapFrameView::from(&stack_frame));
}

#[cfg(not(any(target_os = "none", target_os = "uefi")))]
pub fn timer_interrupt_handler_host() {
    timer_interrupt_logic(KernelTrapFrameView::host_inert());
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub extern "x86-interrupt" fn keyboard_interrupt_handler(stack_frame: InterruptStackFrame) {
    let _gs_guard = crate::cpu::local::KernelGsGuard::from_interrupted_cs(stack_frame.code_segment);
    keyboard_interrupt_logic(KernelTrapFrameView::from(&stack_frame));
}

#[cfg(not(any(target_os = "none", target_os = "uefi")))]
pub fn keyboard_interrupt_handler_host() {
    keyboard_interrupt_logic(KernelTrapFrameView::host_inert());
}
