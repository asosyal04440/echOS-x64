//! # echOS Boot Bilgisi
//!
//! UEFI boot sürecinden kernel'e aktarılan bilgiler.
//! Bellek haritası ve fiziksel offset içerir.
//!
//! ## Boot Bilgisi Aktarım Mekanizması:
//!
//! UEFI firmware, kernel'i yükledikten sonra kontrol CPU'ya devredilir.
//! Bu esnada kernel çeşitli bilgilere ihtiyaç duyar:
//!
//! ```
//!  UEFI Firmware
//!       |
//!       | BootInfo pointer (R/W register veya ABI parametresi)
//!       v
//!  Kernel entry point
//!       |
//!       v
//!  BootInfo yapısı:
//!  +------------------+------------------+
//!  | magic: "ECHBOOT0"| version: 2       |
//!  +------------------+------------------+
//!  | memory_map       | phys_mem_offset   |
//!  +------------------+------------------+
//!  | framebuffer      | rsdp_address      |
//!  +------------------+------------------+
//!  | ...              |                   |
//!  +------------------+------------------+
//! ```
//!
//! `magic` alanı, struct'ın geçerli olduğunu doğrular.
//! `version` alanı, geriye dönük uyumluluk için kullanılır.

pub mod appliance;
pub mod safety;

use crate::gop::framebuffer::Framebuffer;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use uefi::table::boot::MemoryMap;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::{ResetType, Time};
#[cfg(target_os = "uefi")]
use uefi::Status;

/// UEFI'den kernel'e aktarılan boot bilgisi yapısı.
///
/// `#[repr(C)]` ile C ABI uyumlu bellek düzeni garanti edilir;
/// bu struct hem UEFI loader hem de Rust kernel tarafından erişilir.
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

/// Boot bilgisi sihirli sayısı: "ECHBOOT0" ASCII karakter dizisi.
///
/// Kernel, BootInfo pointer'ını okumadan önce bu değeri kontrol ederek
/// bellek bozulmasını veya yanlış pointer'ı tespit edebilir.
pub const BOOTINFO_MAGIC: u64 = u64::from_le_bytes(*b"ECHBOOT0");

/// Boot bilgisi protokol versiyonu.
///
/// UEFI loader ve kernel aynı versiyonu desteklediğinde iletişim güvenlidir.
/// Versiyon uyumsuzluğunda kernel boot'u reddedebilir.
pub const BOOTINFO_VERSION: u32 = 2;

/// Secure Boot durumunu atomik olarak saklayan global bayrak.
static SECURE_BOOT: AtomicBool = AtomicBool::new(false);

/// UEFI Runtime Services pointer'ını atomik olarak saklayan global.
static RUNTIME_SERVICES: AtomicUsize = AtomicUsize::new(0);

/// Global framebuffer erişimi — shell ve diğer bileşenler için.
///
/// Mutex ile korunur: eşzamanlı yazma işlemleri sıralanır.
static GLOBAL_FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);

/// Terminal imleç (cursor) X konumu — piksel cinsinden
static TERM_CURSOR_X: AtomicUsize = AtomicUsize::new(0);
/// Terminal imleç (cursor) Y konumu — piksel cinsinden
static TERM_CURSOR_Y: AtomicUsize = AtomicUsize::new(0);

/// Terminal ön plan rengi: parlak yeşil (0x00FF00 = RGB)
pub const TERM_FG_COLOR: u32 = 0x00FF00; // Yeşil
/// Terminal arka plan rengi: siyah (0x000000 = RGB)
pub const TERM_BG_COLOR: u32 = 0x000000; // Siyah

/// Global framebuffer'ı kaydeder.
///
/// Bu fonksiyon UEFI handover sonrasında bir kez çağrılır.
/// Framebuffer `GLOBAL_FRAMEBUFFER` mutexine taşınır.
pub fn set_global_framebuffer(fb: Framebuffer) {
    *GLOBAL_FRAMEBUFFER.lock() = Some(fb);
}

/// Global framebuffer'ın bir kopyasını döndürür.
///
/// `Framebuffer` kopyalanabilir (Clone) olduğundan kilidi kısa süre tutar.
pub fn get_global_framebuffer() -> Option<Framebuffer> {
    GLOBAL_FRAMEBUFFER.lock().clone()
}

/// Framebuffer'a iş parçacığı güvenli metin yazar.
///
/// Karakter işleme akışı:
/// ```
/// '\n' --> cursor_x = 0, cursor_y += char_height
/// '\r' --> cursor_x = 0
/// '\t' --> cursor_x'i 32-pixel sınırına hizala (4 karakterlik tab)
/// '\x08' (Backspace) --> cursor_x bir geriye, önceki hücreyi sil
/// diğer --> glif çiz, cursor_x += char_width
/// ```
///
/// Ekran dolduğunda içerik `char_height` piksel yukarı kaydırılır (scroll).
/// Kesintiler (interrupt) kilitlenmeyi önlemek için geçici olarak devre dışı bırakılır.
pub fn term_print(s: &str) {
    // Kesintileri devre dışı bırak: mutex kilidi alınırken başka bir kesinti
    // aynı mutex'i almaya çalışırsa kilitlenme (deadlock) oluşabilir.
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
                    cursor_x = (cursor_x + 32) & !31; // 4 karakterlik tab (32 piksel hizalama)
                    if cursor_x > max_x {
                        cursor_x = 0;
                        cursor_y += char_height;
                    }
                } else if c == '\x08' {
                    // Geri al (Backspace) — imleçi bir karakter geri çekerek önceki hücreyi temizler
                    if cursor_x >= char_width {
                        cursor_x -= char_width;
                        // Silmek için boşluk yaz (arka plan rengiyle üstünü örtme)
                        fb.draw_char(cursor_x, cursor_y, ' ', TERM_BG_COLOR);
                    }
                } else {
                    if cursor_x > max_x {
                        cursor_x = 0;
                        cursor_y += char_height;
                    }

                    if cursor_y > max_y {
                        // Ekranı yukarı kaydır: tüm içeriği char_height piksel yukarı taşı
                        fb.scroll_up(char_height);
                        cursor_y = max_y.saturating_sub(char_height) + 1;
                        // Yeni en alt satırı temizle (kaydırma sonrasında eski içeriği kapat)
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

    // Kesinti durumunu geri yükle: RFLAGS IF (Interrupt Flag) biti kontrol edilir.
    // Bit 9 = 1 ise kesintiler çağrı öncesinde etkinmiş demektir.
    if (flags & (1 << 9)) != 0 {
        x86_64::instructions::interrupts::enable();
    }
}

/// Terminali temizler: tüm ekranı arka plan rengiyle doldurur ve imleci (0,0)'a taşır.
pub fn term_clear() {
    // Kesintileri geçici olarak devre dışı bırak (deadlock önlemi)
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

    // Kesinti durumunu geri yükle
    if (flags & (1 << 9)) != 0 {
        x86_64::instructions::interrupts::enable();
    }
}

/// Secure Boot durumunu ayarlar.
///
/// UEFI loader tarafından boot zamanında çağrılır.
pub fn set_secure_boot(enabled: bool) {
    SECURE_BOOT.store(enabled, Ordering::SeqCst);
}

/// Secure Boot'un etkin olup olmadığını döndürür.
pub fn secure_boot_enabled() -> bool {
    SECURE_BOOT.load(Ordering::SeqCst)
}

/// UEFI Runtime Services pointer'ını saklar.
///
/// Pointer, UEFI handover sırasında kernel'e geçirilir ve ileride
/// `get_uefi_time`, `reset_uefi_system` gibi fonksiyonlarda kullanılır.
pub fn set_runtime_services(ptr: usize) {
    RUNTIME_SERVICES.store(ptr, Ordering::SeqCst);
}

/// UEFI Runtime Services ham pointer'ını döndürür.
///
/// Pointer ayarlanmamışsa (0) `None` döner.
#[cfg(target_os = "uefi")]
pub fn runtime_services_ptr() -> Option<*mut uefi::table::runtime::RuntimeServices> {
    let ptr = RUNTIME_SERVICES.load(Ordering::SeqCst);
    if ptr == 0 {
        None
    } else {
        Some(ptr as *mut uefi::table::runtime::RuntimeServices)
    }
}

/// UEFI Runtime Services referansını döndürür.
///
/// Pointer geçerliyse `'static` ömürlü bir referans döner.
#[cfg(target_os = "uefi")]
pub fn runtime_services() -> Option<&'static uefi::table::runtime::RuntimeServices> {
    runtime_services_ptr().map(|ptr| unsafe { &*ptr })
}

/// UEFI üzerinden mevcut zamanı okur.
#[cfg(target_os = "uefi")]
pub fn get_uefi_time() -> Result<Time, Status> {
    let runtime = runtime_services().ok_or(Status::UNSUPPORTED)?;
    runtime.get_time().map_err(|err| err.status())
}

/// UEFI üzerinden sistem saatini ayarlar.
#[cfg(target_os = "uefi")]
pub fn set_uefi_time(time: &Time) -> Result<(), Status> {
    let runtime = runtime_services_ptr().ok_or(Status::UNSUPPORTED)?;
    unsafe { (&mut *runtime).set_time(time) }.map_err(|err| err.status())
}

/// UEFI Runtime Services'in çalışır durumda olduğunu doğrular.
///
/// Zaman okuma işlemini test amaçlı olarak yapar; başarılıysa servisler hazırdır.
#[cfg(target_os = "uefi")]
pub fn verify_uefi_runtime_services() -> Result<(), Status> {
    get_uefi_time().map(|_| ())
}

/// UEFI üzerinden sistemi yeniden başlatır veya kapatır.
///
/// Bu fonksiyon geri dönmez (`!`): UEFI reset çağrısı başarısız olursa
/// sonsuz döngüde bekler (halt).
#[cfg(target_os = "uefi")]
pub fn reset_uefi_system(reset_type: ResetType, status: Status) -> ! {
    if let Some(runtime) = runtime_services_ptr() {
        unsafe { (&mut *runtime).reset(reset_type, status, None) };
    }
    loop {}
}

/// UEFI olmayan ortamlar için yedek: Runtime Services her zaman `None` döner.
#[cfg(not(target_os = "uefi"))]
pub fn runtime_services() -> Option<&'static ()> {
    None
}
