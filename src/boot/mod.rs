//! # echOS Boot Bilgisi
//!
//! UEFI boot sürecinden kernel'e aktarılan bilgiler.
//! Bellek haritası ve fiziksel offset içerir.

pub mod safety;

use crate::gop::framebuffer::Framebuffer;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use uefi::table::boot::MemoryMap;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::{ResetType, Time};
#[cfg(target_os = "uefi")]
use uefi::Status;

#[repr(C)]
pub struct BootInfo {
    pub magic: u64,
    pub version: u32,
    pub size: u32,
    pub memory_map: Option<MemoryMap<'static>>,
    pub physical_memory_offset: u64,
    pub hhdm_offset: u64,
    pub framebuffer: Option<Framebuffer>,
    pub rsdp_address: u64,
    pub system_table: u64,
    pub runtime_services: usize,
    pub secure_boot: bool,
    pub cmdline_ptr: u64,
    pub cmdline_len: u64,
    pub image_size: u64,
    pub image_hash: [u8; 32],
}

pub const BOOTINFO_MAGIC: u64 = u64::from_le_bytes(*b"ECHBOOT0");
pub const BOOTINFO_VERSION: u32 = 2;

static SECURE_BOOT: AtomicBool = AtomicBool::new(false);
static RUNTIME_SERVICES: AtomicUsize = AtomicUsize::new(0);

/// Global framebuffer erişimi - shell ve diğer bileşenler için
static GLOBAL_FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);

/// Terminal cursor pozisyonu
static TERM_CURSOR_X: AtomicUsize = AtomicUsize::new(0);
static TERM_CURSOR_Y: AtomicUsize = AtomicUsize::new(0);

/// Terminal renkleri
pub const TERM_FG_COLOR: u32 = 0x00FF00; // Yeşil
pub const TERM_BG_COLOR: u32 = 0x000000; // Siyah

/// Global framebuffer'ı kaydet
pub fn set_global_framebuffer(fb: Framebuffer) {
    *GLOBAL_FRAMEBUFFER.lock() = Some(fb);
}

/// Global framebuffer'a erişim
pub fn get_global_framebuffer() -> Option<Framebuffer> {
    GLOBAL_FRAMEBUFFER.lock().clone()
}

/// Framebuffer'a thread-safe yazı yaz
pub fn term_print(s: &str) {
    // Interrupts'ları disable et - deadlock önlemek için
    let flags = x86_64::registers::rflags::read().bits();
    x86_64::instructions::interrupts::disable();
    
    {
        let mut guard = GLOBAL_FRAMEBUFFER.lock();
        if let Some(ref mut fb) = *guard {
            let mut cursor_x = TERM_CURSOR_X.load(Ordering::Relaxed);
            let mut cursor_y = TERM_CURSOR_Y.load(Ordering::Relaxed);
            
            let char_width = 8;
            let char_height = 16;
            let max_x = fb.width.saturating_sub(char_width);
            let max_y = fb.height.saturating_sub(char_height);
            
            for c in s.chars() {
                if c == '\n' {
                    cursor_x = 0;
                    cursor_y += char_height;
                } else if c == '\r' {
                    cursor_x = 0;
                } else if c == '\t' {
                    cursor_x = (cursor_x + 32) & !31; // 4 karakterlik tab
                    if cursor_x > max_x {
                        cursor_x = 0;
                        cursor_y += char_height;
                    }
                } else if c == '\x08' {
                    // Backspace
                    if cursor_x >= char_width {
                        cursor_x -= char_width;
                        // Silmek için boşluk yaz
                        fb.draw_char(cursor_x, cursor_y, ' ', TERM_BG_COLOR);
                    }
                } else {
                    if cursor_x > max_x {
                        cursor_x = 0;
                        cursor_y += char_height;
                    }
                    
                    if cursor_y > max_y {
                        // Scroll up
                        fb.scroll_up(char_height);
                        cursor_y = max_y.saturating_sub(char_height) + 1;
                        // Yeni satırı temizle
                        for x in (0..fb.width).step_by(char_width) {
                            fb.draw_string(x, cursor_y, "        ", TERM_BG_COLOR);
                        }
                    }
                    
                    fb.draw_char(cursor_x, cursor_y, c, TERM_FG_COLOR);
                    cursor_x += char_width;
                }
            }
            
            TERM_CURSOR_X.store(cursor_x, Ordering::Relaxed);
            TERM_CURSOR_Y.store(cursor_y, Ordering::Relaxed);
        }
    }
    
    // Interrupt durumunu geri yükle
    if (flags & (1 << 9)) != 0 {
        x86_64::instructions::interrupts::enable();
    }
}

/// Terminal'i temizle
pub fn term_clear() {
    // Interrupts'ları disable et - deadlock önlemek için
    let flags = x86_64::registers::rflags::read().bits();
    x86_64::instructions::interrupts::disable();
    
    {
        let mut guard = GLOBAL_FRAMEBUFFER.lock();
        if let Some(ref mut fb) = *guard {
            fb.clear(TERM_BG_COLOR);
            TERM_CURSOR_X.store(0, Ordering::Relaxed);
            TERM_CURSOR_Y.store(0, Ordering::Relaxed);
        }
    }
    
    // Interrupt durumunu geri yükle
    if (flags & (1 << 9)) != 0 {
        x86_64::instructions::interrupts::enable();
    }
}

pub fn set_secure_boot(enabled: bool) {
    SECURE_BOOT.store(enabled, Ordering::SeqCst);
}

pub fn secure_boot_enabled() -> bool {
    SECURE_BOOT.load(Ordering::SeqCst)
}

pub fn set_runtime_services(ptr: usize) {
    RUNTIME_SERVICES.store(ptr, Ordering::SeqCst);
}

#[cfg(target_os = "uefi")]
pub fn runtime_services_ptr() -> Option<*mut uefi::table::runtime::RuntimeServices> {
    let ptr = RUNTIME_SERVICES.load(Ordering::SeqCst);
    if ptr == 0 {
        None
    } else {
        Some(ptr as *mut uefi::table::runtime::RuntimeServices)
    }
}

#[cfg(target_os = "uefi")]
pub fn runtime_services() -> Option<&'static uefi::table::runtime::RuntimeServices> {
    runtime_services_ptr().map(|ptr| unsafe { &*ptr })
}

#[cfg(target_os = "uefi")]
pub fn get_uefi_time() -> Result<Time, Status> {
    let runtime = runtime_services().ok_or(Status::UNSUPPORTED)?;
    runtime.get_time().map_err(|err| err.status())
}

#[cfg(target_os = "uefi")]
pub fn set_uefi_time(time: &Time) -> Result<(), Status> {
    let runtime = runtime_services_ptr().ok_or(Status::UNSUPPORTED)?;
    unsafe { (&mut *runtime).set_time(time) }.map_err(|err| err.status())
}

#[cfg(target_os = "uefi")]
pub fn verify_uefi_runtime_services() -> Result<(), Status> {
    get_uefi_time().map(|_| ())
}

#[cfg(target_os = "uefi")]
pub fn reset_uefi_system(reset_type: ResetType, status: Status) -> ! {
    if let Some(runtime) = runtime_services_ptr() {
        unsafe { (&mut *runtime).reset(reset_type, status, None) };
    }
    loop {}
}

#[cfg(not(target_os = "uefi"))]
pub fn runtime_services() -> Option<&'static ()> {
    None
}
