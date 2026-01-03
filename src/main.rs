#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
extern crate alloc;

use alloc::format;
use alloc::boxed::Box;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use log::{Level, Metadata, Record};
use uefi::boot::SearchType;
use uefi::{Identify, Status};
use spin::Mutex;
use x86_64::structures::paging::{Mapper, Page, PageTableFlags, Size4KiB, FrameAllocator};
use x86_64::VirtAddr;

// mod memory; // Removed to use library module

// Global Framebuffer for access by kernel tasks
// We use a Mutex<Option> to allow safe sharing (though only GUI task should write)
// The inner type is raw pointer wrapper or similar, but for now we'll store the struct.
// Since Framebuffer uses a mutable reference to GOP, which is a bit tricky with 'static.
// We'll trust that we leaked it properly.
static FRAMEBUFFER: Mutex<Option<&'static mut ech_os::gop::framebuffer::Framebuffer>> = Mutex::new(None);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // 1. Log to Serial (Backup)
    ech_os::serial_println!("PANIC: {}", info);
    
    // 2. Emergency Draw (Visual SOS)
    // We try to grab the lock. If it's deadlocked, we are stuck.
    // In emergency, maybe we should force unlock?
    // Using try_lock is safer? Or just force access via raw ptr if possible.
    // For now, standard lock.
    if let Some(mut fb_guard) = FRAMEBUFFER.try_lock() {
        if let Some(fb) = fb_guard.as_mut() {
             let buffer = fb.buffer_mut();
             let red = 0xFFFF0000;
             for pixel in buffer.iter_mut() {
                 *pixel = red;
             }
        }
    } else {
        // Lock busy? We died while drawing.
        ech_os::serial_println!("Panic: Could not acquire framebuffer lock.");
    }

    loop {
        x86_64::instructions::hlt();
    }
}

struct UefiLogger;

fn str_to_utf16_null_terminated(s: &str) -> alloc::vec::Vec<u16> {
    let mut v: alloc::vec::Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// Enable AVX/AVX2 instructions in CPU (OSFXSR, OSXMMEXCPT, XCR0)
unsafe fn enable_avx() {
    use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
    
    // 1. Enable OSFXSR and OSXMMEXCPT in CR4
    // 1. Enable OSFXSR and OSXMMEXCPT in CR4
    let mut cr4_bits = Cr4::read().bits();
    cr4_bits |= (1 << 9); // OSFXSR
    cr4_bits |= (1 << 10); // OSXMMEXCPT
    cr4_bits |= (1 << 18); // OSXSAVE (Required for xsetbv)
    
    // Safety: we trust bit 18 is supported (CPUID check skipped for speed, but QEMU supports it)
    let cr4 = Cr4Flags::from_bits_truncate(cr4_bits);
    Cr4::write(cr4);
    
    // 2. Enable Monitor Co-processor in CR0
    let mut cr0 = Cr0::read();
    cr0 |= Cr0Flags::MONITOR_COPROCESSOR;
    // Clear EM (Emulation) to avoid #UD on FPU instructions
    // cr0 &= !Cr0Flags::EMULATED; 
    Cr0::write(cr0);
    
    // 3. Enable AVX (Bit 2) and SSE (Bit 1) in XCR0
    // XCR0 (Extended Control Register 0) is accessed via xsetbv.
    // Index 0 = XCR0.
    // Bit 0 = X87 (Must be 1)
    // Bit 1 = SSE (Must be 1 for AVX)
    // Bit 2 = AVX (Enable YMM)
    // Value = 1 | 2 | 4 = 7.
    
    let xcr0 = 0b111; // X87(1) | SSE(1) | AVX(1)
    
    use core::arch::asm;
    asm!(
        "xsetbv",
        in("ecx") 0, // XCR0
        in("eax") xcr0, // Low 32 bits
        in("edx") 0,    // High 32 bits
        options(nostack, preserves_flags)
    );
    
    ech_os::serial_println!("AVX/AVX2 Enabled (XCR0=7)");
}

impl log::Log for UefiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            unsafe {
                if let Some(system_table) = uefi::table::system_table_raw() {
                    let stdout = (*system_table.as_ptr()).stdout;
                    let msg = format!("{}: {}\n", record.level(), record.args());
                    let wide = str_to_utf16_null_terminated(&msg);
                    let _ = ((*stdout).output_string)(stdout, wide.as_ptr());
                }
            }
        }
    }

    fn flush(&self) {}
}

static LOGGER: UefiLogger = UefiLogger;

fn init_logger() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

#[entry]
fn main() -> Status {
    ech_os::serial::init();
    ech_os::serial_println!("echOS boot sequence started...");

    // 1. Memory Init
    let mut memory_manager = match ech_os::memory::init() {
        Ok(manager) => manager,
        Err(_e) => {
            ech_os::serial_println!("Memory init failed");
            panic!("Memory init failed");
        }
    };
    ech_os::serial_println!("Memory initialized.");

    // 2. Paging Init
    // 2. Paging Init (New VMM)
    let mut mapper = unsafe { ech_os::memory::paging::init_virtual_memory(&mut memory_manager) };
    
    // CRITICAL: We switched CR3 in init_virtual_memory.
    // We must update the global KERNEL_PML4_FRAME for the Scheduler.
    unsafe {
        ech_os::memory::KERNEL_PML4_FRAME = Some(x86_64::registers::control::Cr3::read().0);
    }
    
    // 3. Heap Init
    if let Err(e) = ech_os::allocator::init_heap(&mut mapper, &mut memory_manager) {
        ech_os::serial_println!("Heap init failed: {:?}", e);
        panic!("Heap init failed");
    }
    ech_os::serial_println!("Heap initialized.");
    init_logger();

    // 4. Interrupt & CPU Init
    ech_os::cpu::init();
    
    ech_os::serial_println!("Initializing GDT...");
    ech_os::gdt::init();
    ech_os::serial_println!("Initializing IDT...");
    ech_os::interrupts::init(); // Loads IDT & Inits PIC
    
        // --- Phase 6.5: User Mode Test Setup ---
    /*
    {
        ech_os::serial_println!("Setting up User Mode Test...");
        
        // 1. Map User Code Page (0x400000)
        let page_code = Page::<Size4KiB>::containing_address(VirtAddr::new(0x400000));
        let frame_code = memory_manager.allocate_frame().unwrap();
        let flags_code = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        unsafe {
            mapper.map_to(page_code, frame_code, flags_code, &mut memory_manager).unwrap().flush();
        }
        
        // 2. Map User Stack Page (0x500000)
        let page_stack = Page::<Size4KiB>::containing_address(VirtAddr::new(0x500000));
        let frame_stack = memory_manager.allocate_frame().unwrap();
        let flags_stack = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        unsafe {
            mapper.map_to(page_stack, frame_stack, flags_stack, &mut memory_manager).unwrap().flush();
        }
        
        // 3. Write Shellcode to 0x400000
        // Payload: mov rax, 1; syscall; jmp .
        let code_ptr = 0x400000 as *mut u8;
        let payload: [u8; 9] = [
            0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
            0x0F, 0x05                                // syscall
            // jmp . is optional as we expect syscall to print and return? 
            // Better loop: EB FE
        ];
        // Infinite loop instruction: 0xEB 0xFE
        let loop_instr: [u8; 2] = [0xEB, 0xFE];
        
        unsafe {
            for (i, b) in payload.iter().enumerate() {
                *code_ptr.add(i) = *b;
            }
            // Add loop at the end
            *code_ptr.add(payload.len()) = loop_instr[0];
            *code_ptr.add(payload.len() + 1) = loop_instr[1];
        }
        
        ech_os::serial_println!("User Payload written to 0x400000");
    }
    */
    
    /*
    // Switch to APIC
    ech_os::serial_println!("Initializing Local APIC...");
    unsafe { ech_os::drivers::apic::init(); }
    */
    
    ech_os::serial_println!("Initializing Syscall System...");
    ech_os::syscall::init();
    // Do NOT enable interrupts yet, wait for Scheduler

    // 5. GOP Init
    let gop_handles = match uefi::boot::locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID)) {
        Ok(handles) => handles,
        Err(_e) => panic!("GOP locate failed"),
    };

    if !gop_handles.is_empty() {
        let mut gop = match uefi::boot::open_protocol_exclusive::<GraphicsOutput>(gop_handles[0]) {
            Ok(gop) => gop,
            Err(_e) => panic!("GOP open failed"),
        };

        // 5. Framebuffer Create
        let framebuffer = Box::leak(Box::new(ech_os::gop::framebuffer::Framebuffer::new(&mut gop)));
        
        // Save to global mutex
        *FRAMEBUFFER.lock() = Some(framebuffer);
        ech_os::serial_println!("Framebuffer initialized and globally stored.");

    } else {
        panic!("No GOP handles found");
    }

    // 6. Drivers Init
    // Initialize PS/2 keyboard controller
    ech_os::serial_println!("Initializing PS/2 Controller...");
    if ech_os::drivers::ps2::init() {
        ech_os::serial_println!("PS/2 Controller Ready.");
    } else {
        ech_os::serial_println!("WARNING: PS/2 Controller Init Failed!");
    }
    
    // Initialize Mouse Driver
    if ech_os::drivers::mouse::init() {
        ech_os::serial_println!("PS/2 Mouse Ready.");
    } else {
        ech_os::serial_println!("WARNING: PS/2 Mouse Init Failed!");
    }

    // 7. Exit Boot Services
    ech_os::serial_println!("Exiting UEFI Boot Services...");
    let _memory_map = unsafe {
        uefi::boot::exit_boot_services(Some(uefi::boot::MemoryType::LOADER_DATA))
    };
    
    // 8. Re-init Interrupts logic is handled by the pre-ExitBS init. 
    // We just need to ensure PIC is happy.
    // GDT/IDT/PIC are already set. 
    // We do NOT need to call init() again unless we overwrote something.
    // However, enabling interrupts is the key step.
    
    // 8.5. Re-enable Mouse (Send Magic Sequence again)
    // Since we reset, we might need to tell Mouse to stream again.
    ech_os::serial_println!("Re-enabling PS/2 Mouse...");
    ech_os::drivers::mouse::reinit_streaming();
    
    // 9. Initialize Scheduler
    ech_os::serial_println!("Initializing Scheduler...");
    ech_os::task::scheduler::init();
    
    // 10. Spawn Kernel Main Task
    ech_os::serial_println!("Spawning kernel_main...");
    ech_os::task::scheduler::spawn(kernel_main);
    
    // 11. Spawn Prime Cruncher
    ech_os::serial_println!("Spawning Prime Cruncher...");
    ech_os::task::scheduler::spawn_with_priority(prime_cruncher, ech_os::task::Priority::Low, "Cruncher");

    // 12. Enable Interrupts
    ech_os::serial_println!("Enabling Interrupts & Starting System...");
    x86_64::instructions::interrupts::enable();
    
    // 12. Idle Loop (This becomes the Idle Task)
    loop {
        x86_64::instructions::hlt();
    }
}

/// The Main Kernel Task (GUI & Event Loop)
fn kernel_main() -> ! {
    ech_os::serial_println!("Entered kernel_main task! Igniting Graphics Engine...");
    
    loop {
        // We lock the framebuffer and NEVER RELEASE IT.
        // The Compositor owns the screen now.
        // This effectively turns the kernel into a graphical runtime.
        let mut fb_guard = FRAMEBUFFER.lock();
        
        if let Some(fb) = fb_guard.as_mut() {
            // diverge -> !
            // Enable AVX before Engine Start
            // unsafe { enable_avx(); }
            ech_os::gfx::compositor::run(fb);
        } else {
            // Framebuffer not ready? Spin/Sleep.
            ech_os::serial_println!("WARNING: Framebuffer not available in kernel_main!");
            drop(fb_guard); // Release early to retry
            x86_64::instructions::hlt();
        }
    }
}

/// CPU Stress Task: continuously calculates primes
fn prime_cruncher() -> ! {
    let mut n: u64 = 2;
    let mut primes_found = 0;
    
    ech_os::serial_println!("Prime Cruncher STARTED!");
    
    loop {
        if is_prime(n) {
            primes_found += 1;
            // Print every 100th prime to avoid spamming serial too much but prove life
            if primes_found % 100 == 0 {
                 ech_os::serial_println!("Cruncher (Low Prio): Found {} primes. Current: {}", primes_found, n);
            }
        }
        n += 1;
        // NO SLEEP! Pure CPU burn.
        // The scheduler must preempt this.
    }
}

fn is_prime(n: u64) -> bool {
    if n <= 1 { return false; }
    // Inefficient algo to burn more cycles
    for i in 2..n {
        if n % i == 0 { return false; }
    }
    true
}

/// The User Mode Test Task
fn user_test() -> ! {
    ech_os::serial_println!("Task 'UserRoot' starting transition to Ring 3...");
    let entry = x86_64::VirtAddr::new(0x400000);
    // Stack grows down, so top is 0x500000 + 4096 = 0x501000
    let stack = x86_64::VirtAddr::new(0x501000);
    unsafe {
        ech_os::task::user::enter_user_mode(entry, stack);
    }
}