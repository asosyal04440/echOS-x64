//! # echOS Sistem Çağrısı (Syscall) Mekanizması Modülü
//!
//! x86_64 `SYSCALL/SYSRET` talimat çifti üzerine kurulu sistem çağrısı altyapısı.
//! Linux POSIX uyumlu `syscall` çağrı kuralı (calling convention) kullanılır.
//!
//! ## SYSCALL/SYSRET Nedir?
//! Kullanıcı modundan çekirdek moduna geçişi sağlayan özel x86_64 talimatları:
//! - `SYSCALL`: Kullanıcı modundan çekirdeğe atlar, RIP'i MSR_LSTAR'dan yükler.
//! - `SYSRET`: Çekirdekten kullanıcı moduna döner, RIP ve RFLAGS'ı geri yükler.
//!
//! ## Sistem Çağrısı Akışı
//! ```ascii
//! Kullanıcı kodu
//!      |  (SYSCALL talimatı)
//!      v
//! syscall_handler  [assembly]
//!   1. SWAPGS      <-- Kullanıcı GS_BASE <-> Çekirdek GS_BASE değiş tokuşu
//!   2. Kullanıcı RSP'yi kaydet (GS:0)
//!   3. Çekirdek yığınına geç (GS:8)
//!   4. Kaydedici kaydet (push rbx, rbp, r12-r15)
//!   5. Argümanları çevir (rax-rsi-rdi-rdx-rcx-r8-r9)
//!      |  (call syscall_dispatcher)
//!      v
//! syscall_dispatcher  [Rust]
//!   -> crate::posix::dispatch(num, args)
//!   <- ret: i64
//!      |  (dönüş; pop + SWAPGS + SYSRETQ)
//!      v
//! Kullanıcı kodu devam eder
//! ```

use core::arch::global_asm;
use x86_64::registers::model_specific::Msr;
use x86_64::VirtAddr;

/// EFER MSR: Genişletilmiş Özellik Enable Register.
/// SCE (SYSCALL Enable) bitini içerir.
const MSR_EFER: u32 = 0xC000_0080;
/// STAR MSR: Segment seçici veritabanı; CS ve SS değerlerini SYSCALL/SYSRET için depolar.
const MSR_STAR: u32 = 0xC000_0081;
/// LSTAR MSR: Long mode SYSCALL hedef adresini (RIP) depolar.
const MSR_LSTAR: u32 = 0xC000_0082;
/// SFMASK MSR: SYSCALL sırasında temizlenecek RFLAGS bitlerini belirler.
const MSR_SFMASK: u32 = 0xC000_0084;
/// KERNEL_GS_BASE MSR: SWAPGS talimatı için çekirdek GS tabanını depolar.
const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// CPU başına sistem çağrısı geçiş verisi yapısı.
///
/// `repr(C)` zorunludur: assembly kodu sabit ofsetlerle bu alanlara erişir.
///
/// GS segment tabanlı erişim haritası:
/// - GS:0  -> `user_rsp_scratch`  (kullanıcı RSP geçici deposu)
/// - GS:8  -> `kernel_stack_top`  (çekirdek yığın tepe adresi)
/// - GS:16 -> `cpu_id`            (CPU kimlik numarası, u32 = 4 bayt)
/// - GS:24 -> `user_rip`          (kullanıcı RIP; SYSCALL sonrası RCX)
/// - GS:32 -> `user_rflags`       (kullanıcı RFLAGS; SYSCALL sonrası R11)
/// - GS:40 -> `irq_depth`         (kesme derinliği sayacı)
#[repr(C)]
pub struct CpuData {
    pub user_rsp_scratch: u64,
    pub kernel_stack_top: u64,
    pub cpu_id: u32,
    pub user_rip: u64,
    pub user_rflags: u64,
    pub irq_depth: u64,
}

/// Sistem çağrısı başına ayrılan çekirdek yığını boyutu (40 KiB).
///
/// Her sistem çağrısı iç içe geçebileceğinden yeterince büyük tutulur.
pub const SYSCALL_STACK_SIZE: usize = 40960;

/// Sistem çağrısı mekanizmasını başlatır.
///
/// MSR'lara gerekli değerleri yazar:
/// - `MSR_EFER`: SCE bitini set ederek SYSCALL/SYSRET'i etkinleştirir.
/// - `MSR_STAR`: Çekirdek ve kullanıcı segment seçicilerini yapılandırır.
/// - `MSR_LSTAR`: Sistem çağrısı giriş noktasının adresini yazar.
/// - `MSR_SFMASK`: SYSCALL sırasında kesme bayrağı (IF) temizlenir.
pub fn init() {
    let code_selector = crate::gdt::kernel_code_selector();
    let user_code_selector = crate::gdt::user_code_selector();

    unsafe {
        let mut efer = Msr::new(MSR_EFER);
        let mut efer_val = efer.read();
        efer_val |= 1 << 0; // Bit 0: SCE (SYSCALL/SYSRET etkinleştirme)
        efer.write(efer_val);

        let kernel_cs = code_selector.0 as u64;
        let user_cs = (user_code_selector.0 | 3) as u64;
        // STAR: üst 32 bit ring3 CS, bit 47:32 ring0 CS
        let star_val = ((user_cs - 16) << 48) | (kernel_cs << 32);
        let mut star = Msr::new(MSR_STAR);
        star.write(star_val);

        // LSTAR: SYSCALL gelince RIP bu adresteki assembly işleyiciye atlar
        let syscall_addr = VirtAddr::new(syscall_handler as *const () as usize as u64);
        let mut lstar = Msr::new(MSR_LSTAR);
        lstar.write(syscall_addr.as_u64());

        let mut sfmask = Msr::new(MSR_SFMASK);
        sfmask.write(0x200); // Bit 9: IF (kesme bayrağı maskesi); SYSCALL sırasında kesme kapalı kalır
    }

    crate::serial_println!("Syscall Mechanism Initialized.");
}

/// Geçerli CPU için `CpuData` işaretçisini KERNEL_GS_BASE MSR'a yazar.
///
/// SWAPGS sonrası GS segmentinin `CpuData` yapısını göstermesi için
/// her CPU'nun başlangıcında çağrılması gerekir.
pub unsafe fn init_cpu_data(cpu_data: *mut CpuData) {
    let mut kernel_gs_base = Msr::new(MSR_KERNEL_GS_BASE);
    kernel_gs_base.write(cpu_data as u64);
}

/// Geçerli CPU'nun çekirdek yığın tepe adresini günceller.
///
/// Yeni bir görev (task) zamanlandığında çağrılmalıdır; böylece SYSCALL
/// sırasında doğru çekirdek yığını kullanılır.
pub fn set_kernel_stack_for_current_cpu(stack_top: u64) {
    unsafe {
        let mut msr = Msr::new(MSR_KERNEL_GS_BASE);
        let base = msr.read();
        let data = base as *mut CpuData;
        // GS:8 pozisyonunu (kernel_stack_top) güncelle
        (*data).kernel_stack_top = stack_top;
    }
}

/// Geçerli CPU'nun kullanıcı bağlamını (RSP, RIP, RFLAGS) okur.
///
/// Sistem çağrısından dönerken veya sinyal işlerken kullanıcı durumunu
/// incelemek için kullanılır. `(user_rsp, user_rip, user_rflags)` demeti döner.
pub fn current_user_context() -> (u64, u64, u64) {
    unsafe {
        let mut msr = Msr::new(MSR_KERNEL_GS_BASE);
        let base = msr.read();
        let data = base as *const CpuData;
        (
            (*data).user_rsp_scratch,
            (*data).user_rip,
            (*data).user_rflags,
        )
    }
}

/// x86_64 sistem çağrısı assembly giriş noktası.
///
/// SYSCALL talimatı tarafından doğrudan çağrılan düşük seviye giriş noktası.
/// Kullanıcı bağlamını kaydeder, argümanları yeniden düzenler, Rust dağıtıcısını çağırır.
global_asm!(
    r#"
.global syscall_handler

.section .text
syscall_handler:
    // SWAPGS: user GS_BASE <-> KERNEL_GS_BASE, per-CPU yapıya geçiş
    swapgs
    // Kullanıcı RSP'yi GS:0 scratch alanına kaydet
    mov qword ptr gs:[0], rsp
    mov qword ptr gs:[24], rcx
    mov qword ptr gs:[32], r11
    // Kernel stack top'u GS:8'den yükle
    mov rsp, qword ptr gs:[8]

    // Register preservation: callee-saved + SYSRET zorunlu RCX/R11
    push rcx
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    // Stack alignment: call öncesi RSP % 16 == 8 olacak şekilde ayarlanır
    mov r11, rdi
    mov rdi, rax
    mov rax, rsi
    mov rsi, r11
    mov r11, rdx
    mov rdx, rax
    mov rcx, r11
    mov r11, r9
    mov r9, r8
    mov r8, r10
    sub rsp, 8
    mov [rsp], r11
    call syscall_dispatcher
    add rsp, 8

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    pop r11
    pop rcx

    mov rsp, qword ptr gs:[0]
    swapgs
    sysretq
"#
);

/// Sistem çağrısı assembly giriş noktasının dış bildirimi.
extern "C" {
    fn syscall_handler();
}

/// Rust tarafındaki sistem çağrısı dağıtıcısı.
///
/// Assembly giriş noktasının `call syscall_dispatcher` komutuyla çağrısına karşılık gelir.
/// Syscall numarasını ve altı argümanı alarak `crate::posix::dispatch`'e iletir.
/// Dönüş değeri `rax` kaydedicisi aracılığıyla kullanıcıya geri döner.
#[no_mangle]
pub extern "sysv64" fn syscall_dispatcher(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
) -> i64 {
    let ret = crate::posix::dispatch(
        num as usize,
        [
            a1 as usize,
            a2 as usize,
            a3 as usize,
            a4 as usize,
            a5 as usize,
            a6 as usize,
        ],
    );
    ret as i64
}
