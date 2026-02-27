//! # echOS Kullanıcı Alanı Çalışma Zamanı Kütüphanesi
//!
//! echOS'ta kullanıcı alanı programları için minimal çalışma zamanı.
//! Syscall sarmalayıcıları, temel G/Ç, bellek tahsisi ve süreç kontrolü sağlar.

#![no_std]
#![feature(asm)]
#![feature(llvm_asm)]

use core::arch::asm;
use core::panic::PanicInfo;

// ============================================================================
// SİSTEM ÇAĞRISI NUMARALARI (x86_64 Linux ABI uyumlu)
// ============================================================================

pub const SYS_READ: usize = 0;
pub const SYS_WRITE: usize = 1;
pub const SYS_OPEN: usize = 2;
pub const SYS_CLOSE: usize = 3;
pub const SYS_STAT: usize = 4;
pub const SYS_FSTAT: usize = 5;
pub const SYS_LSEEK: usize = 8;
pub const SYS_MMAP: usize = 9;
pub const SYS_MPROTECT: usize = 10;
pub const SYS_MUNMAP: usize = 11;
pub const SYS_BRK: usize = 12;
pub const SYS_IOCTL: usize = 16;
pub const SYS_DUP: usize = 32;
pub const SYS_DUP2: usize = 33;
pub const SYS_GETPID: usize = 39;
pub const SYS_SOCKET: usize = 41;
pub const SYS_CONNECT: usize = 42;
pub const SYS_ACCEPT: usize = 43;
pub const SYS_SENDTO: usize = 44;
pub const SYS_RECVFROM: usize = 45;
pub const SYS_SHUTDOWN: usize = 48;
pub const SYS_BIND: usize = 49;
pub const SYS_LISTEN: usize = 50;
pub const SYS_GETSOCKNAME: usize = 51;
pub const SYS_GETPEERNAME: usize = 52;
pub const SYS_SOCKETPAIR: usize = 53;
pub const SYS_SETSOCKOPT: usize = 54;
pub const SYS_GETSOCKOPT: usize = 55;
pub const SYS_FORK: usize = 57;
pub const SYS_EXECVE: usize = 59;
pub const SYS_EXIT: usize = 60;
pub const SYS_WAIT4: usize = 61;
pub const SYS_KILL: usize = 62;
pub const SYS_UNAME: usize = 63;
pub const SYS_FCNTL: usize = 72;
pub const SYS_FSYNC: usize = 74;
pub const SYS_FTRUNCATE: usize = 77;
pub const SYS_GETCWD: usize = 79;
pub const SYS_CHDIR: usize = 80;
pub const SYS_MKDIR: usize = 83;
pub const SYS_RMDIR: usize = 84;
pub const SYS_UNLINK: usize = 87;
pub const SYS_SYMLINK: usize = 88;
pub const SYS_READLINK: usize = 89;
pub const SYS_CHMOD: usize = 90;
pub const SYS_GETUID: usize = 102;
pub const SYS_GETGID: usize = 104;
pub const SYS_SETUID: usize = 105;
pub const SYS_SETGID: usize = 106;
pub const SYS_GETPPID: usize = 110;
pub const SYS_SCHED_YIELD: usize = 24;
pub const SYS_NANOSLEEP: usize = 35;
pub const SYS_CLOCK_GETTIME: usize = 228;
pub const SYS_CLOCK_GETRES: usize = 229;
pub const SYS_GETTIMEOFDAY: usize = 96;
pub const SYS_TIME: usize = 201;
pub const SYS_FUTEX: usize = 202;
pub const SYS_SET_TID_ADDRESS: usize = 218;
pub const SYS_GET_TID: usize = 224;

// ============================================================================
// SİSTEM ÇAĞRISI SARMALAYICILARI
// ============================================================================

/// 0 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall0(n: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 1 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall1(n: usize, a: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 2 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall2(n: usize, a: usize, b: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 3 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall3(n: usize, a: usize, b: usize, c: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 4 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall4(n: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        in("r10") d as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 5 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall5(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        in("r10") d as isize,
        in("r8") e as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

/// 6 argümanlı ham syscall
#[inline(always)]
pub unsafe fn syscall6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        in("r10") d as isize,
        in("r8") e as isize,
        in("r9") f as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

// ============================================================================
// HATA KODLARI
// ============================================================================

pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const ENXIO: i32 = 6;
pub const EBADF: i32 = 9;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;
pub const ENOSPC: i32 = 28;
pub const ENOSYS: i32 = 38;

/// Syscall dönüş değerini Result'a çevir
#[inline]
pub fn result(ret: isize) -> Result<usize, i32> {
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(-ret as i32)
    }
}

// ============================================================================
// DOSYA TANITICI (FILE DESCRIPTOR) İŞLEMLERİ
// ============================================================================

/// Dosya tanıtıcı türü
pub type Fd = usize;

/// Standart dosya tanıtıcıları
pub const STDIN: Fd = 0;
pub const STDOUT: Fd = 1;
pub const STDERR: Fd = 2;

/// Dosya tanıtıcısından oku
pub fn read(fd: Fd, buf: &mut [u8]) -> Result<usize, i32> {
    unsafe {
        result(syscall3(SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len()))
    }
}

/// Dosya tanıtıcısına yaz
pub fn write(fd: Fd, buf: &[u8]) -> Result<usize, i32> {
    unsafe {
        result(syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()))
    }
}

/// Tüm baytları yaz
pub fn write_all(fd: Fd, buf: &[u8]) -> Result<(), i32> {
    let mut offset = 0;
    while offset < buf.len() {
        match write(fd, &buf[offset..]) {
            Ok(n) => offset += n,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Metin dizisi yaz
pub fn write_str(fd: Fd, s: &str) -> Result<(), i32> {
    write_all(fd, s.as_bytes())
}

/// Standart çıktıya yazdır
pub fn print(s: &str) {
    let _ = write_str(STDOUT, s);
}

/// Standart çıktıya satır sonu ile yazdır
pub fn println(s: &str) {
    print(s);
    print("\n");
}

/// Dosya tanıtıcısını kapat
pub fn close(fd: Fd) -> Result<(), i32> {
    unsafe {
        result(syscall1(SYS_CLOSE, fd))?;
        Ok(())
    }
}

/// Dosya tanıtıcısını çoğalt
pub fn dup(fd: Fd) -> Result<Fd, i32> {
    unsafe {
        result(syscall1(SYS_DUP, fd))
    }
}

/// Belirli bir fd'ye çoğalt
pub fn dup2(old: Fd, new: Fd) -> Result<Fd, i32> {
    unsafe {
        result(syscall2(SYS_DUP2, old, new))
    }
}

// ============================================================================
// DOSYA İŞLEMLERİ
// ============================================================================

/// Açma bayrakları
pub const O_RDONLY: usize = 0;
pub const O_WRONLY: usize = 1;
pub const O_RDWR: usize = 2;
pub const O_CREAT: usize = 64;
pub const O_EXCL: usize = 128;
pub const O_TRUNC: usize = 512;
pub const O_APPEND: usize = 1024;

/// Dosya aç
pub fn open(path: &str, flags: usize, mode: usize) -> Result<Fd, i32> {
    unsafe {
        result(syscall3(SYS_OPEN, path.as_ptr() as usize, flags, mode))
    }
}

/// Konum belirleme yönleri
pub const SEEK_SET: usize = 0;
pub const SEEK_CUR: usize = 1;
pub const SEEK_END: usize = 2;

/// Dosyada konum değiştir
pub fn lseek(fd: Fd, offset: i64, whence: usize) -> Result<i64, i32> {
    unsafe {
        result(syscall3(SYS_LSEEK, fd, offset as usize, whence)).map(|r| r as i64)
    }
}

// ============================================================================
// SÜREÇ İŞLEMLERİ
// ============================================================================

/// Süreç kimliğini al
pub fn getpid() -> usize {
    unsafe { syscall0(SYS_GETPID) as usize }
}

/// Üst süreç kimliğini al
pub fn getppid() -> usize {
    unsafe { syscall0(SYS_GETPPID) as usize }
}

/// Süreçten çık
pub fn exit(code: i32) -> ! {
    unsafe {
        let _ = syscall1(SYS_EXIT, code as usize);
    }
    loop {
        unsafe { asm!("hlt") }
    }
}

/// İşlemciyi bırak (yield)
pub fn sched_yield() {
    unsafe {
        let _ = syscall0(SYS_SCHED_YIELD);
    }
}

/// Süreç çatalla (fork)
pub fn fork() -> Result<usize, i32> {
    unsafe {
        result(syscall0(SYS_FORK))
    }
}

/// Program çalıştır
pub fn execve(path: &str, argv: &[&str], envp: &[&str]) -> Result<(), i32> {
    // Bu karmaşık - argv/envp dizileri oluşturulması gerekir
    // Şimdilik basitleştirilmiş sürüm
    unsafe {
        result(syscall3(
            SYS_EXECVE,
            path.as_ptr() as usize,
            argv.as_ptr() as usize,
            envp.as_ptr() as usize,
        ))?;
    }
    Ok(())
}

/// Alt süreç için bekle
pub fn wait4(pid: usize, status: &mut i32, options: usize) -> Result<usize, i32> {
    unsafe {
        result(syscall4(SYS_WAIT4, pid, status as *mut i32 as usize, options, 0))
    }
}

// ============================================================================
// BELLEK İŞLEMLERİ
// ============================================================================

/// Bellek koruma bayrakları
pub const PROT_NONE: usize = 0;
pub const PROT_READ: usize = 1;
pub const PROT_WRITE: usize = 2;
pub const PROT_EXEC: usize = 4;

/// Eşleme bayrakları
pub const MAP_SHARED: usize = 0x01;
pub const MAP_PRIVATE: usize = 0x02;
pub const MAP_FIXED: usize = 0x10;
pub const MAP_ANONYMOUS: usize = 0x20;

/// Bellek eşle
pub fn mmap(addr: usize, len: usize, prot: usize, flags: usize, fd: Fd, offset: usize) -> Result<usize, i32> {
    unsafe {
        result(syscall6(SYS_MMAP, addr, len, prot, flags, fd, offset))
    }
}

/// Bellek eşlemesini kaldır
pub fn munmap(addr: usize, len: usize) -> Result<(), i32> {
    unsafe {
        result(syscall2(SYS_MUNMAP, addr, len))?;
        Ok(())
    }
}

/// Bellek korumasını değiştir
pub fn mprotect(addr: usize, len: usize, prot: usize) -> Result<(), i32> {
    unsafe {
        result(syscall3(SYS_MPROTECT, addr, len, prot))?;
        Ok(())
    }
}

/// Heap kesim noktasını değiştir
pub fn brk(addr: usize) -> Result<usize, i32> {
    unsafe {
        result(syscall1(SYS_BRK, addr))
    }
}

// ============================================================================
// DİZİN İŞLEMLERİ
// ============================================================================

/// Dizin oluştur
pub fn mkdir(path: &str, mode: usize) -> Result<(), i32> {
    unsafe {
        result(syscall2(SYS_MKDIR, path.as_ptr() as usize, mode))?;
        Ok(())
    }
}

/// Dizini kaldır
pub fn rmdir(path: &str) -> Result<(), i32> {
    unsafe {
        result(syscall1(SYS_RMDIR, path.as_ptr() as usize))?;
        Ok(())
    }
}

/// Dizin değiştir
pub fn chdir(path: &str) -> Result<(), i32> {
    unsafe {
        result(syscall1(SYS_CHDIR, path.as_ptr() as usize))?;
        Ok(())
    }
}

/// Geçerli dizini al
pub fn getcwd(buf: &mut [u8]) -> Result<usize, i32> {
    unsafe {
        result(syscall2(SYS_GETCWD, buf.as_mut_ptr() as usize, buf.len()))
    }
}

/// Dosya bağlantısını kaldır
pub fn unlink(path: &str) -> Result<(), i32> {
    unsafe {
        result(syscall1(SYS_UNLINK, path.as_ptr() as usize))?;
        Ok(())
    }
}

// ============================================================================
// ZAMAN İŞLEMLERİ
// ============================================================================

/// Saat kimliği
pub const CLOCK_REALTIME: usize = 0;
pub const CLOCK_MONOTONIC: usize = 1;

/// Zaman yapısı
#[repr(C)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Zaman al
pub fn clock_gettime(clock: usize) -> Result<Timespec, i32> {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe {
        result(syscall2(SYS_CLOCK_GETTIME, clock, &mut ts as *mut Timespec as usize))?;
    }
    Ok(ts)
}

/// Nanosaniye uyku
pub fn nanosleep(req: &Timespec, rem: Option<&mut Timespec>) -> Result<(), i32> {
    unsafe {
        result(syscall2(
            SYS_NANOSLEEP,
            req as *const Timespec as usize,
            rem.map_or(0, |r| r as *mut Timespec as usize),
        ))?;
        Ok(())
    }
}

/// Milisaniye bekleme
pub fn sleep_ms(ms: u64) {
    let ts = Timespec {
        tv_sec: (ms / 1000) as i64,
        tv_nsec: ((ms % 1000) * 1_000_000) as i64,
    };
    let _ = nanosleep(&ts, None);
}

// ============================================================================
// METİN DİZESİ YARDIMCI FONKSİYONLARI
// ============================================================================

/// Metinleri karşılaştır
pub fn strcmp(a: &str, b: &str) -> i32 {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let min_len = a_bytes.len().min(b_bytes.len());

    for i in 0..min_len {
        let diff = a_bytes[i] as i32 - b_bytes[i] as i32;
        if diff != 0 {
            return diff;
        }
    }

    a_bytes.len() as i32 - b_bytes.len() as i32
}

/// Metnin belirtilen önek ile başlayıp başlamadığını kontrol et
pub fn starts_with(s: &str, prefix: &str) -> bool {
    s.as_bytes().starts_with(prefix.as_bytes())
}

/// Metnin belirtilen sonek ile bitip bitmediğini kontrol et
pub fn ends_with(s: &str, suffix: &str) -> bool {
    s.as_bytes().ends_with(suffix.as_bytes())
}

/// Boşlukları kırp
pub fn trim(s: &str) -> &str {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b != b' ').unwrap_or(0);
    let end = bytes.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);
    if start >= end {
        ""
    } else {
        core::str::from_utf8(&bytes[start..end]).unwrap_or("")
    }
}

// ============================================================================
// PANİK İŞLEYİCİSİ
// ============================================================================

#[panic_handler]
pub fn panic(_info: &PanicInfo) -> ! {
    print("\n[KULLANICI_ALANI] PANİK: ");
    if let Some(s) = _info.payload().downcast_ref::<&str>() {
        print(s);
    }
    print("\n");
    exit(127);
}

// ============================================================================
// GENEL TAHSİS EDİCİ (Basit Bump Tahsis Edici)
// ============================================================================

use core::alloc::{GlobalAlloc, Layout};

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        static mut HEAP: [u8; 1024 * 1024] = [0; 1024 * 1024]; // 1 MB heap
        static mut OFFSET: usize = 0;

        let align = layout.align();
        let size = layout.size();

        let base = HEAP.as_ptr() as usize;
        let current = base + OFFSET;
        let aligned = (current + align - 1) & !(align - 1);
        let new_offset = aligned - base + size;

        if new_offset > HEAP.len() {
            return core::ptr::null_mut();
        }

        OFFSET = new_offset;
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump tahsis edici bellek serbest bırakmaz
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

// ============================================================================
// ALLOC KÜTÜPHANESİ YENİDEN İHRAÇLARI
// ============================================================================

extern crate alloc;

pub use alloc::string::String;
pub use alloc::vec::Vec;
pub use alloc::boxed::Box;
pub use alloc::format;
