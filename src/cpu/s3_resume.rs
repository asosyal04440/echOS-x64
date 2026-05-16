use core::arch::global_asm;
use core::sync::atomic::{compiler_fence, AtomicBool, AtomicU32, AtomicU64, Ordering};
use x86_64::registers::control::Cr3;

const S3_RESUME_ADDR: u32 = 0x8000;
const S3_RESUME_SIZE: usize = 4096;
const S3_RESUME_STACK_SIZE: usize = 16 * 1024;
const S3_RESUME_MAGIC: u64 = 0x5343_4852_4553_3345;

#[repr(C, packed)]
struct S3ResumeData {
    pml4_phys: u64,
    entry: u64,
    stack_top: u64,
    continuation_rsp: u64,
    continuation_rip: u64,
    continuation_magic: u64,
}

#[repr(align(16))]
struct S3ResumeStack([u8; S3_RESUME_STACK_SIZE]);

#[cfg(not(target_os = "windows"))]
global_asm!(include_str!("s3_resume.asm"));

#[cfg(not(target_os = "windows"))]
extern "C" {
    static s3_resume_begin: u8;
    static s3_resume_end: u8;
    static s3_resume_data: u8;
    fn s3_enter_pm1_and_wait(
        pm1a: u16,
        pm1b: u16,
        value_a: u16,
        value_b: u16,
        data: *mut S3ResumeData,
    ) -> u64;
}

#[cfg(target_os = "windows")]
#[no_mangle]
static s3_resume_begin: u8 = 0;
#[cfg(target_os = "windows")]
#[no_mangle]
static s3_resume_end: u8 = 0;
#[cfg(target_os = "windows")]
#[no_mangle]
static s3_resume_data: u8 = 0;

static S3_RESUME_ARMED: AtomicBool = AtomicBool::new(false);
static S3_RESUME_CONTINUED: AtomicBool = AtomicBool::new(false);
static S3_RESUME_COUNT: AtomicU32 = AtomicU32::new(0);
static S3_RESUME_LAST_PML4: AtomicU64 = AtomicU64::new(0);
static S3_RESUME_LAST_CONTINUATION: AtomicU64 = AtomicU64::new(0);
static mut S3_RESUME_STACK: S3ResumeStack = S3ResumeStack([0; S3_RESUME_STACK_SIZE]);

pub fn resume_vector_phys() -> u32 {
    S3_RESUME_ADDR
}

pub fn resume_count() -> u32 {
    S3_RESUME_COUNT.load(Ordering::Acquire)
}

pub fn is_armed() -> bool {
    S3_RESUME_ARMED.load(Ordering::Acquire)
}

pub fn take_continuation_resume() -> bool {
    S3_RESUME_CONTINUED.swap(false, Ordering::AcqRel)
}

pub fn prepare() -> bool {
    #[cfg(target_os = "windows")]
    {
        false
    }

    #[cfg(not(target_os = "windows"))]
    unsafe {
        let src = &s3_resume_begin as *const u8;
        let size =
            (&s3_resume_end as *const u8 as usize) - (&s3_resume_begin as *const u8 as usize);
        if size == 0 || size > S3_RESUME_SIZE {
            crate::serial_println!("[S3] resume trampoline size invalid: {}", size);
            S3_RESUME_ARMED.store(false, Ordering::Release);
            return false;
        }

        let mut pml4_phys = crate::memory::KERNEL_PML4_PHYS;
        if pml4_phys == 0 {
            let (pml4_frame, _) = Cr3::read();
            pml4_phys = pml4_frame.start_address().as_u64();
        }
        if pml4_phys == 0 || pml4_phys > u32::MAX as u64 {
            crate::serial_println!(
                "[S3] resume trampoline requires PML4 below 4GiB, got {:#x}",
                pml4_phys
            );
            S3_RESUME_ARMED.store(false, Ordering::Release);
            return false;
        }

        if !crate::memory::map_identity(S3_RESUME_ADDR as u64, S3_RESUME_SIZE) {
            crate::serial_println!("[S3] failed to identity-map resume trampoline");
            S3_RESUME_ARMED.store(false, Ordering::Release);
            return false;
        }
        crate::gdt::prepare_bsp_resume_gdt();

        let dest = (crate::memory::active_physical_offset() + S3_RESUME_ADDR as u64) as *mut u8;
        core::ptr::write_bytes(dest, 0, S3_RESUME_SIZE);
        core::ptr::copy_nonoverlapping(src, dest, size);

        let start = &s3_resume_begin as *const u8 as usize;
        let data = &s3_resume_data as *const u8 as usize;
        let data_offset = data - start;
        let data_ptr = dest.add(data_offset) as *mut S3ResumeData;
        let stack_top =
            (&raw const S3_RESUME_STACK.0 as *const u8 as u64) + S3_RESUME_STACK_SIZE as u64;
        (*data_ptr).pml4_phys = pml4_phys;
        (*data_ptr).entry = s3_resume_entry as usize as u64;
        (*data_ptr).stack_top = stack_top;
        (*data_ptr).continuation_rsp = 0;
        (*data_ptr).continuation_rip = 0;
        (*data_ptr).continuation_magic = 0;

        S3_RESUME_LAST_PML4.store(pml4_phys, Ordering::Release);
        S3_RESUME_CONTINUED.store(false, Ordering::Release);
        compiler_fence(Ordering::SeqCst);
        S3_RESUME_ARMED.store(true, Ordering::Release);
        crate::serial_println!(
            "[S3] resume trampoline armed phys={:#x} pml4={:#x} stack={:#x}",
            S3_RESUME_ADDR,
            pml4_phys,
            stack_top
        );
        true
    }
}

pub fn enter_pm1_sleep(pm1a: u16, pm1b: u16, value_a: u16, value_b: u16) -> bool {
    #[cfg(target_os = "windows")]
    {
        let _ = (pm1a, pm1b, value_a, value_b);
        false
    }

    #[cfg(not(target_os = "windows"))]
    unsafe {
        if !S3_RESUME_ARMED.load(Ordering::Acquire) {
            crate::serial_println!("[S3] PM1 entry refused: resume vector is not armed");
            return false;
        }

        let src_start = &s3_resume_begin as *const u8 as usize;
        let data = &s3_resume_data as *const u8 as usize;
        let data_offset = data - src_start;
        let data_ptr = (crate::memory::active_physical_offset()
            + S3_RESUME_ADDR as u64
            + data_offset as u64) as *mut S3ResumeData;

        compiler_fence(Ordering::SeqCst);
        let rc = s3_enter_pm1_and_wait(pm1a, pm1b, value_a, value_b, data_ptr);
        compiler_fence(Ordering::SeqCst);

        let rip = (*data_ptr).continuation_rip;
        S3_RESUME_LAST_CONTINUATION.store(rip, Ordering::Release);
        S3_RESUME_ARMED.store(false, Ordering::Release);
        if rc == S3_RESUME_MAGIC {
            S3_RESUME_COUNT.fetch_add(1, Ordering::AcqRel);
            S3_RESUME_CONTINUED.store(true, Ordering::Release);
            crate::serial_println!("[S3] continuation resumed rip={:#x}", rip);
            true
        } else {
            false
        }
    }
}

pub fn restore_bsp_descriptor_tables_after_resume() {
    crate::gdt::reload_bsp_after_resume();
    crate::interrupts::reload_bsp_idt_after_resume();
    compiler_fence(Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn s3_resume_entry() -> ! {
    S3_RESUME_COUNT.fetch_add(1, Ordering::AcqRel);
    S3_RESUME_ARMED.store(false, Ordering::Release);
    crate::serial_println!(
        "[S3] firmware wake vector reached pml4={:#x} continuation={:#x}",
        S3_RESUME_LAST_PML4.load(Ordering::Acquire),
        S3_RESUME_LAST_CONTINUATION.load(Ordering::Acquire)
    );

    restore_bsp_descriptor_tables_after_resume();

    if let Err(err) = crate::drivers::power::PM_MANAGER
        .resume_from_firmware_wake(crate::drivers::power::SleepState::S3)
    {
        crate::serial_println!("[SMOKE] suspend-resume fail: {:?}", err);
    } else if let Err(err) = crate::power::system_resume_from_firmware_wake() {
        crate::serial_println!("[SMOKE] suspend-resume fail: {:?}", err);
    } else {
        crate::serial_println!("[SMOKE] suspend-resume ok");
    }

    loop {
        #[cfg(not(any(test, target_os = "windows")))]
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
        #[cfg(any(test, target_os = "windows"))]
        core::hint::spin_loop();
    }
}
