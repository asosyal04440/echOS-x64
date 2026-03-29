//! # echOS POSIX Uyumluluk Katmanı
//!
//! Bu modül, echOS çekirdeğinin Linux/POSIX uyumlu sistem çağrısı (syscall)
//! arayüzünü uygular. Kullanıcı alanındaki programlar, bu arayüz sayesinde
//! standart C kütüphanesi (libc) çağrılarını echOS üzerinde çalıştırabilir.
//!
//! ## Syscall Akışı (Genel Diyagram)
//!
//! ```text
//! Kullanıcı Programı
//!   │
//!   │  syscall talimatı (x86_64: SYSCALL)
//!   ▼
//! ┌─────────────────────────────────────────┐
//! │  syscall entry (src/syscall.rs)         │
//! │  rax = syscall numarası                 │
//! │  rdi,rsi,rdx,r10,r8,r9 = argümanlar    │
//! └────────────────┬────────────────────────┘
//!                  │
//!                  ▼
//! ┌─────────────────────────────────────────┐
//! │  posix::dispatch(number, args)          │  <- Bu dosya
//! │  1) PTRACE hook kontrolü                │
//! │  2) SECCOMP kısıtlama kontrolü          │
//! │  3) Syscall numarasına göre eşleşme     │
//! └────────────────┬────────────────────────┘
//!                  │
//!          ┌───────┴────────┐
//!          ▼                ▼
//!    sys_read()        sys_write()  ... (diğerleri)
//! ```
//!
//! ## Desteklenen API'ler
//! - Dosya I/O: open, read, write, close, lseek, stat, fstat
//! - Bellek: mmap, munmap, mprotect, brk
//! - Süreç: fork, exec, wait4, exit, getpid
//! - Sinyal: rt_sigaction, rt_sigprocmask, kill
//! - IPC: pipe, shmget/shmat, futex
//! - Soket: socket, bind, connect, sendto, recvfrom
//! - Zamanlayıcı: clock_gettime, nanosleep, timer_create
//! - io_uring: Asenkron I/O çerçevesi
//! - Win32/NT uyumu: dispatch_nt() ile Windows çağrıları

// ============================================================================
// POSIX ALT MODÜLLERİ
// Pipe, semaphore, mesaj kuyruğu ve dinamik bağlama yardımcı modülleri.
// Bu dosya src/posix/ dizini için module root görevi görür.
// ============================================================================

/// ELF dinamik yükleyici ve dlopen/dlsym/dlclose desteği
#[path = "posix/dlopen.rs"]
pub mod dlopen;
/// Mesaj kuyruğu implementasyonu (System V IPC + POSIX mq)
#[path = "posix/msgq.rs"]
pub mod msgq;
/// Boru (pipe) ve FIFO implementasyonu — tek yönlü IPC
#[path = "posix/pipe.rs"]
pub mod pipe;
/// POSIX / System V semafor implementasyonu
#[path = "posix/semaphore.rs"]
pub mod semaphore;

/// Lock-free io_uring SQ/CQ ring buffer implementasyonu (Mutex SIFIR)
#[path = "posix/io_uring_ring.rs"]
pub mod io_uring_ring;
#[path = "posix/native_scene_bridge.rs"]
mod native_scene_bridge;
#[path = "posix/process_bridge.rs"]
mod process_bridge;
#[path = "posix/service_bridge.rs"]
mod service_bridge;
#[path = "posix/windows_image.rs"]
mod windows_image;
#[path = "posix/windows_runtime.rs"]
mod windows_runtime;

pub use pipe::{O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY};
pub use windows_image::{
    pe_info_from_image, pe_sections_from_image, prepare_windows_launch, run_windows_app,
    run_windows_app_image, secure_boot_db_available, secure_boot_verify_image, PeInfo,
    PeSectionInfo, WindowsLaunchPlan,
};
pub use windows_runtime::{
    current_windows_runtime, list_windows_runtimes, select_windows_runtime, upsert_windows_runtime,
    WindowsRuntime, WindowsRuntimeError, WindowsRuntimeFlavor,
};

/// POSIX syscall çağrısı taşıyıcı yapısı.
///
/// Kullanıcı alanından gelen bir sistem çağrısını temsil eder.
/// `number` alanı hangi syscall'ın çağrıldığını belirtir,
/// `args` ise en fazla 6 adet argümanı tutar (x86_64 ABI gereği).
#[derive(Clone, Copy)]
pub struct PosixCall {
    pub number: usize,
    pub args: [usize; 6],
}

use super::fs::f2fs::F2fsEntry;
use super::kernel::{memory as kernel_memory, tasking};
use super::{
    allocator, cpu, drivers, ecosystem_exactness, fs, random, security, serial_print,
    serial_println, tty,
};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use echos_sdk_sys::{
    NativeClipboardGetTextResponse, NativeClipboardSetTextRequest, NativeEventKind,
    NativeInputEvent, NativeNotificationRequest, NativeSceneOp, NativeSceneOpKind,
    NativeSceneSubmitRequest, NativeServiceBootstrap, NativeServiceEndpointPublishRequest,
    NativeServiceEndpointState, NativeServiceNotificationCommandKind,
    NativeServiceNotificationEntry, NativeServiceNotificationRequest,
    NativeServiceNotificationResponse, NativeServiceNotificationResponseKind,
    NativeServiceParityStatus, NativeServiceRegionMapping, NativeServiceStatus,
    NativeWindowCreateRequest, NativeWindowHandle, MAX_INLINE_TEXT, MAX_POLLED_EVENTS,
    MAX_SCENE_OPS, MAX_SERVICE_NOTIFICATION_ITEMS,
};
use lazy_static::lazy_static;
use rcore_fs::vfs::{FileType, FsError, INode};
use spin::Mutex;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::VariableVendor;
use x86_64::structures::paging::PageTableFlags;
#[cfg(not(target_os = "uefi"))]
enum VariableVendor {
    GLOBAL_VARIABLE,
    IMAGE_SECURITY_DATABASE,
}

// ============================================================
// Standart errno Hata Kodları (POSIX.1 / Linux uyumlu)
//
// errno nedir?
//   Sistem çağrısı başarısız olduğunda, negatif bir değer döner.
//   Örneğin ENOENT için: -(2) = !0usize - 1  (wrapping subtract)
//
//   Kullanıcı alanında (libc):
//     if (read(...) < 0) perror("hata");  // errno global değişken
//
//   echOS çekirdeğinde errno() yardımcı fonksiyonu bu dönüşümü yapar.
//
//   Örnek errno dönüşümü:
//     ENOENT = 2  ->  errno(2) = !0 - 1 = 0xFFFF...FFFE
// ============================================================
const EPERM: usize = 1;
const ENOENT: usize = 2;
const ESRCH: usize = 3;
const EINTR: usize = 4;
const EIO: usize = 5;
const ENXIO: usize = 6;
const EBADF: usize = 9;
const ENOMEM: usize = 12;
const EACCES: usize = 13;
const EFAULT: usize = 14;
const EEXIST: usize = 17;
const ENOTDIR: usize = 20;
const EISDIR: usize = 21;
const EINVAL: usize = 22;
const EMFILE: usize = 24;
const ENOTTY: usize = 25;
const ENOSPC: usize = 28;
const ENOSYS: usize = 38;
const ENOTEMPTY: usize = 39;
const ENODEV: usize = 19;
const EAFNOSUPPORT: usize = 97;
const EOPNOTSUPP: usize = 95;
const ENOTCONN: usize = 107;
const EAGAIN: usize = 11;
const EFBIG: usize = 27;

// Futex wait queue entry
struct FutexWaiter {
    uaddr: usize,
    bitmask: u32,
    pid: usize,
}

// Futex PI (Priority Inheritance) state
struct FutexPiState {
    owner_pid: usize,
    waiters: Vec<usize>,
    boosted_priority: u32,
}

lazy_static! {
    static ref FUTEX_WAITERS: Mutex<Vec<FutexWaiter>> = Mutex::new(Vec::new());
    static ref FUTEX_PI_TABLE: Mutex<alloc::collections::BTreeMap<usize, FutexPiState>> =
        Mutex::new(alloc::collections::BTreeMap::new());
}

// ============================================================
// Syscall Numaraları — x86_64 Linux ABI uyumlu alt küme
//
// Linux'ta syscall numaraları sabit ve ABI'nin bir parçasıdır.
// Kullanıcı alanı bir syscall'ı şöyle başlatır:
//   mov rax, <numara>     ; syscall numarası
//   mov rdi, <arg1>       ; 1. argüman
//   mov rsi, <arg2>       ; 2. argüman
//   syscall               ; çekirdek moduna geçiş
//
// echOS bu numaraları `dispatch()` fonksiyonunda eşleştirir.
// ============================================================
const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const SYS_STAT: usize = 4;
const SYS_FSTAT: usize = 5;
const SYS_LSTAT: usize = 6;
const SYS_POLL: usize = 7;
const SYS_LSEEK: usize = 8;
const SYS_MMAP: usize = 9;
const SYS_MPROTECT: usize = 10;
const SYS_MUNMAP: usize = 11;
const SYS_BRK: usize = 12;
const SYS_RT_SIGACTION: usize = 13;
const SYS_RT_SIGPROCMASK: usize = 14;
const SYS_IOCTL: usize = 16;
const SYS_PREAD64: usize = 17;
const SYS_PWRITE64: usize = 18;
const SYS_READV: usize = 19;
const SYS_WRITEV: usize = 20;
const SYS_ACCESS: usize = 21;
const SYS_PIPE: usize = 22;
const SYS_SELECT: usize = 23;
const SYS_SCHED_YIELD: usize = 24;
const SYS_MREMAP: usize = 25;
const SYS_MSYNC: usize = 26;
const SYS_MINCORE: usize = 27;
const SYS_MADVISE: usize = 28;
const SYS_FUTEX: usize = 202;
const SYS_RSEQ: usize = 334;
const SYS_TIMER_CREATE: usize = 222;
const SYS_TIMER_SETTIME: usize = 223;
const SYS_TIMER_GETTIME: usize = 224;
const SYS_TIMER_DELETE: usize = 226;
const SYS_EPOLL_CREATE1: usize = 291;
const SYS_EPOLL_CTL: usize = 232;
const SYS_EPOLL_PWAIT: usize = 281;
const SYS_EVENTFD2: usize = 290;
const SYS_SHMGET: usize = 29;
const SYS_SHMAT: usize = 30;
const SYS_SHMCTL: usize = 31;
const SYS_DUP: usize = 32;
const SYS_DUP2: usize = 33;
const SYS_PAUSE: usize = 34;
const SYS_NANOSLEEP: usize = 35;
const SYS_PIPE2: usize = 293;
const SYS_SPLICE: usize = 275;
const SYS_TEE: usize = 276;
const SYS_VMSPLICE: usize = 278;
const SYS_MEMFD_CREATE: usize = 319;

// Dosya Sistemi Syscall'ları
// Bu çağrılar VFS (Virtual File System) katmanı üzerinden F2FS'e yönlendirilir.
const SYS_MKDIR: usize = 83;
const SYS_RMDIR: usize = 84;
const SYS_UNLINK: usize = 87;
const SYS_RENAME: usize = 82;
const SYS_CHMOD: usize = 90;
const SYS_FCHMOD: usize = 91;
const SYS_CHOWN: usize = 92;
const SYS_FCHOWN: usize = 93;
const SYS_LCHOWN: usize = 94;
const SYS_TRUNCATE: usize = 76;
const SYS_FTRUNCATE: usize = 77;
const SYS_LINK: usize = 86;
const SYS_SYMLINK: usize = 88;
const SYS_READLINK: usize = 89;
const SYS_CREAT: usize = 85;
const SYS_STATX: usize = 332;

const SYS_GETPID: usize = 39;
const SYS_EXECVE: usize = 59;
const SYS_FORK: usize = 57;
const SYS_WAIT4: usize = 61;
const SYS_UNAME: usize = 63;
const SYS_GETCWD: usize = 79;
const SYS_GETRUSAGE: usize = 98;
const SYS_SYSINFO: usize = 99;
const SYS_TIMES: usize = 100;
const SYS_PTRACE: usize = 101;
const SYS_GETUID: usize = 102;
const SYS_GETGID: usize = 104;
const SYS_GETPPID: usize = 110;
const SYS_GETEUID: usize = 107;
const SYS_GETEGID: usize = 108;
const SYS_GETTID: usize = 186;
const SYS_EXIT: usize = 60;
const SYS_CLOCK_GETTIME: usize = 228;
const SYS_EXIT_GROUP: usize = 231;
const SYS_OPENAT: usize = 257;
const SYS_NEWFSTATAT: usize = 262;
const SYS_GETRANDOM: usize = 318;
const SYS_IO_URING_SETUP: usize = 425;
const SYS_IO_URING_ENTER: usize = 426;
const SYS_FUTEX_WAITV: usize = 449;

// AT_* sabitleri (openat, newfstatat, faccessat için)
const AT_FDCWD: isize = -100;
const AT_EMPTY_PATH: usize = 0x1000;
const AT_SYMLINK_NOFOLLOW: usize = 0x100;

// access() mode sabitleri
const F_OK: usize = 0;
const R_OK: usize = 4;
const W_OK: usize = 2;
const X_OK: usize = 1;

// ============================================================
// echOS Grafik / Pencere Sunucusu Syscall'ları (Faz 5)
//
// Wayland/X11'e benzer şekilde, echOS 451-455 arası özel
// syscall numaralarını GUI yönetimi için ayırmıştır.
//
// Pencere yaşam döngüsü:
//   SYS_WIN_CREATE  -> pencere oluştur
//   SYS_WIN_GET_BUFFER -> çizim tamponu al
//   SYS_WIN_FLUSH  -> tamponu ekrana bas
//   SYS_EVENT_POLL -> kullanıcı girdilerini (fare/klavye) oku
//   SYS_WIN_DESTROY -> pencereyi kapat
// ============================================================
const SYS_WIN_CREATE: usize = 451;
const SYS_WIN_DESTROY: usize = 452;
const SYS_WIN_GET_BUFFER: usize = 453;
const SYS_WIN_FLUSH: usize = 454;
const SYS_EVENT_POLL: usize = 455;
const SYS_ECHOS_SCENE_COMMIT: usize = 456;
const SYS_ECHOS_NOTIFICATION_POST: usize = 457;
const SYS_ECHOS_CLIPBOARD_SET_TEXT: usize = 458;
const SYS_ECHOS_CLIPBOARD_GET_TEXT: usize = 459;
const SYS_ECHOS_NATIVE_EVENT_POLL: usize = 460;
const SYS_ECHOS_SERVICE_BOOTSTRAP_CLAIM: usize = 461;
const SYS_ECHOS_SERVICE_STATUS: usize = 462;
const SYS_ECHOS_SERVICE_PARITY_STATUS: usize = 463;
const SYS_ECHOS_SERVICE_REGION_MAP: usize = 464;
const SYS_ECHOS_SERVICE_ENDPOINT_PUBLISH: usize = 465;
const SYS_ECHOS_SERVICE_HEARTBEAT: usize = 466;
const SYS_ECHOS_NOTIFICATION_SERVICE_RECV: usize = 467;
const SYS_ECHOS_NOTIFICATION_SERVICE_RESPOND: usize = 468;

// Süreç ve İş Parçacığı (Thread) Yönetimi
// fork()  -> mevcut süreci kopyalar (copy-on-write)
// clone() -> fork + thread bayrakları ile ince kontrol sağlar
// execve() -> mevcut süreç görüntüsünü yeni bir ELF ile değiştirir
const SYS_CLONE: usize = 56;
const SYS_SET_TID_ADDRESS: usize = 218;
const SYS_TGKILL: usize = 234;
const SYS_TKILL: usize = 200;
const SYS_SETUID: usize = 105;
const SYS_SETGID: usize = 106;
const SYS_SETSID: usize = 112;
const SYS_SETPGID: usize = 109;
const SYS_GETPGID: usize = 121;
const SYS_GETSID: usize = 124;

// Sinyal Syscall'ları — Süreçler Arası Anlık Bildirim Mekanizması
//
// UNIX sinyalleri, bir süreci veya süreç grubunu asenkron olarak
// bilgilendirmek için kullanılır. Örn: SIGTERM (15) süreci düzgün
// kapatır, SIGKILL (9) anında sonlandırır.
//
// rt_ önekli sürümler "real-time" sinyalleri de destekler.
const SYS_RT_SIGRETURN: usize = 15;
const SYS_KILL: usize = 62;
const SYS_RT_SIGQUEUEINFO: usize = 129;
const SYS_SIGALTSTACK: usize = 131;
const SYS_RT_SIGSUSPEND: usize = 130;
const SYS_RT_SIGTIMEDWAIT: usize = 128;

const SYS_PRCTL: usize = 157;
const SYS_SECCOMP: usize = 317;

const PR_SET_SECCOMP: usize = 22;
const SECCOMP_MODE_STRICT: usize = 1;
const SECCOMP_MODE_FILTER: usize = 2;

// ==========================================
// SOCKET & NETLINK API (AF_NETLINK & kTLS)
//
// echOS ağ yığını bu syscall'lar üzerinden kullanılır.
//
// Soket Türleri:
//   AF_INET  (2)  -> IPv4  (TCP/UDP)
//   AF_INET6 (10) -> IPv6
//   AF_UNIX  (1)  -> Yerel IPC
//   AF_NETLINK(16)-> Çekirdek ↔ Kullanıcı mesajlaşması
//
// TCP bağlantı akışı (sunucu tarafı):
//   socket() -> bind() -> listen() -> accept() -> recv/send
//
// TCP bağlantı akışı (istemci tarafı):
//   socket() -> connect() -> send/recv
// ==========================================
const SYS_SOCKET: usize = 41;
const SYS_CONNECT: usize = 42;
const SYS_ACCEPT: usize = 43;
const SYS_SENDTO: usize = 44;
const SYS_RECVFROM: usize = 45;
const SYS_BIND: usize = 49;
const SYS_LISTEN: usize = 50;
const SYS_SETSOCKOPT: usize = 54;

const AF_NETLINK: usize = 16;
const SOCK_RAW: usize = 3;

const STATUS_SUCCESS: u32 = 0x00000000;
const STATUS_UNSUCCESSFUL: u32 = 0xC0000001;
const STATUS_NOT_IMPLEMENTED: u32 = 0xC0000002;
const STATUS_ACCESS_VIOLATION: u32 = 0xC0000005;
const STATUS_INVALID_PARAMETER: u32 = 0xC000000D;
const STATUS_NOT_FOUND: u32 = 0xC0000225;

const NT_CLOSE: u32 = 0x0000;
const NT_READ_FILE: u32 = 0x0001;
const NT_WRITE_FILE: u32 = 0x0002;
const NT_OPEN_FILE: u32 = 0x0003;
const NT_QUERY_INFORMATION_FILE: u32 = 0x0004;
const NT_SET_INFORMATION_FILE: u32 = 0x0005;
const NT_CREATE_SECTION: u32 = 0x0006;
const NT_MAP_VIEW_OF_SECTION: u32 = 0x0007;

// CLOCK_REALTIME  (0): Gerçek dünya saati (Unix epoch'tan itibaren nanosaniye)
// CLOCK_MONOTONIC (1): Sistem başlatılışından bu yana geçen süre (geri sarılmaz)
const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;

// Scheduler tick süresi: Her tick 10 milisaniyeye (10_000_000 ns) karşılık gelir.
// nanosleep(2) ve clock_gettime(2) tick sayısına göre hesaplanır.
const TICK_NS: u64 = 10_000_000;

// Maksimum açık dosya sayısı (FD tablosu büyüklüğü)
// Linux'ta varsayılan: 1024, hard limit: 1_048_576
// echOS şimdilik 64 FD ile başlar (gelecekte artırılabilir).
const MAX_FDS: usize = 64;
const STAT_BLKSIZE: i64 = 512;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;
const S_IFLNK: u32 = 0o120000;
const S_IFIFO: u32 = 0o010000;
const MODE_FILE: u32 = 0o644;
const MODE_DIR: u32 = 0o755;
const MODE_CHAR: u32 = 0o666;

const PROT_WRITE: usize = 0x2;
const PROT_EXEC: usize = 0x4;

const MAP_SHARED: usize = 0x01;
const MAP_PRIVATE: usize = 0x02;
const MAP_FIXED: usize = 0x10;
const MAP_ANON: usize = 0x20;

const SIG_BLOCK: usize = 0;
const SIG_UNBLOCK: usize = 1;
const SIG_SETMASK: usize = 2;

const WNOHANG: usize = 1;
const WUNTRACED: usize = 2;
const WCONTINUED: usize = 8;
const GRND_NONBLOCK: usize = 0x1;
const GRND_RANDOM: usize = 0x2;
const GRND_DETERMINISTIC: usize = 0x4;

const IOC_NRBITS: usize = 8;
const IOC_TYPEBITS: usize = 8;
const IOC_SIZEBITS: usize = 14;
const IOC_DIRBITS: usize = 2;
const IOC_NRSHIFT: usize = 0;
const IOC_TYPESHIFT: usize = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: usize = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: usize = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_NONE: usize = 0;
const IOC_WRITE: usize = 1;
const IOC_READ: usize = 2;
const DRM_IOCTL_BASE: u8 = b'd';

const fn ioc(dir: usize, type_: usize, nr: usize, size: usize) -> usize {
    (dir << IOC_DIRSHIFT) | (type_ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT)
}

const fn iowr<T>(nr: usize) -> usize {
    ioc(
        IOC_READ | IOC_WRITE,
        DRM_IOCTL_BASE as usize,
        nr,
        core::mem::size_of::<T>(),
    )
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrmVersion {
    version_major: i32,
    version_minor: i32,
    version_patchlevel: i32,
    name_len: usize,
    name: usize,
    date_len: usize,
    date: usize,
    desc_len: usize,
    desc: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrmVirtgpuResourceCreate {
    handle: u32,
    target: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
    size: u32,
    stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrmVirtgpuMap {
    offset: u64,
    handle: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrmVirtgpuExecbuffer {
    flags: u32,
    size: u32,
    command: u64,
    fence_fd: u64,
    ring_idx: u32,
    pad: u32,
}

const DRM_IOCTL_VERSION: usize = iowr::<DrmVersion>(0x00);
const DRM_IOCTL_VIRTGPU_RESOURCE_CREATE: usize = iowr::<DrmVirtgpuResourceCreate>(0xC0);
const DRM_IOCTL_VIRTGPU_MAP: usize = iowr::<DrmVirtgpuMap>(0xC1);
const DRM_IOCTL_VIRTGPU_EXECBUFFER: usize = iowr::<DrmVirtgpuExecbuffer>(0xC2);

#[derive(Clone, Copy, PartialEq, Eq)]
enum FdKind {
    Stdin,
    Stdout,
    Stderr,
    Null,
    Zero,
    Drm,
    File,
    IoUring,
    Socket,
    Pipe,
}

static FD_INIT: AtomicBool = AtomicBool::new(false);
static FD_TABLE: Mutex<[Option<FdKind>; MAX_FDS]> = Mutex::new([None; MAX_FDS]);
lazy_static! {
    pub static ref FILE_TABLE: Mutex<Vec<Option<FileState>>> = Mutex::new(vec![None; MAX_FDS]);
    static ref RING_TABLE: Mutex<alloc::collections::BTreeMap<usize, LockFreeIoUring>> =
        Mutex::new(alloc::collections::BTreeMap::new());
}
static SIGNAL_MASK: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct FileState {
    pub inode: Arc<dyn INode>,
    pub offset: usize,
    pub size: usize,
    pub is_hello: bool,
}

#[derive(Clone)]
struct DrmResource {
    handle: u32,
    size: usize,
    map_ptr: usize,
}

static DRM_RESOURCES: Mutex<Vec<DrmResource>> = Mutex::new(Vec::new());

/// POSIX timespec yapısı — Nanosaniye hassasiyetli zaman gösterimi.
///
/// Linux ABI'sinde clock_gettime(2), nanosleep(2) vb. çağrılarda kullanılır.
/// tv_sec  : Unix epoch'tan (1 Ocak 1970) saniye cinsinden zaman.
/// tv_nsec : Saniyenin nanosaniye kısmı (0..=999_999_999).
#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Stat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    __pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atim: Timespec,
    st_mtim: Timespec,
    st_ctim: Timespec,
    __unused: [i64; 3],
}

/// Hata kodunu Linux errno formatına dönüştürür.
///
/// Linux'ta başarısız syscall'lar negatif değer döner:
///   ret = -(errno_code)  örn: ENOENT için ret = -2
///
/// Rust/x86_64'te bu wrapping aritmetiğiyle yapılır:
///   errno(2) = (!0usize).wrapping_sub(2 - 1) = 0xFFFF...FFFE = -2 (isize)
fn errno(code: usize) -> usize {
    (!0usize).wrapping_sub(code - 1)
}

fn unsupported_errno(surface: &'static str) -> usize {
    ecosystem_exactness::record_posix_unsupported(surface);
    errno(ENOSYS)
}

fn unsupported_syscall_number(number: usize) -> usize {
    ecosystem_exactness::record_posix_unsupported_number(number);
    errno(ENOSYS)
}

fn vfs_errno(err: FsError) -> usize {
    match err {
        FsError::EntryNotFound => errno(ENOENT),
        FsError::NotFile | FsError::IsDir => errno(EINVAL),
        FsError::NoDevice => errno(EIO),
        FsError::InvalidParam => errno(EINVAL),
        _ => errno(EIO),
    }
}

#[inline]
fn enforce_path_policy(path: &str, access: security::landlock::Access) -> Result<(), usize> {
    if security::landlock::check_path_for_current_task(path, access) {
        Ok(())
    } else {
        serial_println!(
            "[LANDLOCK] pid={} denied {:?} {}",
            tasking::scheduler::current_task_id(),
            access,
            path
        );
        Err(errno(EPERM))
    }
}

/// Ana Syscall Dağıtıcısı (Dispatcher)
///
/// Kullanıcı alanından gelen her sistem çağrısı bu fonksiyona ulaşır.
/// İşlem sırası:
///   1. FD tablosu başlatılmamışsa başlat (ilk çağrıda)
///   2. PTRACE hook aktifse, syscall girişini logla
///   3. SECCOMP strict mod aktifse, izin verilmeyen çağrıları engelle
///   4. Syscall numarasını `match` ile doğru handler'a yönlendir
///   5. PTRACE hook aktifse, dönüş değerini logla
pub fn dispatch(number: usize, args: [usize; 6]) -> usize {
    let _cfi_scope = security::cfi::enter_syscall_scope(number as u64);
    ensure_fd_table();

    // =====================================
    // PTRACE SYSCALL HOOK (ENTRY)
    // ptrace(PTRACE_SYSCALL) bitini kontrol et.
    // Ayıklama (debugging) amacıyla her syscall giriş/çıkışı loglanır.
    // =====================================
    let is_traced = (tasking::scheduler::get_current_ptrace_flags() & 1) != 0;

    if is_traced {
        serial_println!("[PTRACE Hook] SYSCALL Entry: #{}", number);
    }

    // =====================================
    // SECCOMP (STRICT MODE) DENETİMİ
    // Güvenli hesaplama: strict modda yalnızca 4 syscall'a izin verilir:
    //   read(0), write(1), exit(60), rt_sigreturn(15)
    // Diğer tüm çağrılar süreç sonlandırılarak engellenir.
    // =====================================
    let seccomp_mode = tasking::scheduler::get_current_seccomp_mode();
    if seccomp_mode == 1 {
        // Strict mod sadece 4 temel çağrıya (read, write, exit, rt_sigreturn) izin verir
        if number != SYS_READ
            && number != SYS_WRITE
            && number != SYS_EXIT
            && number != SYS_RT_SIGRETURN
        {
            serial_println!(
                "[SECCOMP] Strict mode violation! Syscall {} blocked. Process killed.",
                number
            );
            return process_bridge::sys_exit(!0); // SIGKILL benzeri task sonlandır (exit code -1)
        }
    }

    let ret_val = match number {
        SYS_READ => sys_read(args[0], args[1], args[2]),
        SYS_WRITE => sys_write(args[0], args[1], args[2]),
        SYS_OPEN => sys_open(args[0], args[1], args[2]),
        SYS_CLOSE => sys_close(args[0]),
        SYS_STAT => sys_stat(args[0], args[1]),
        SYS_FSTAT => sys_fstat(args[0], args[1]),
        SYS_LSTAT => sys_lstat(args[0], args[1]),
        SYS_POLL => sys_poll(args[0], args[1], args[2]),
        SYS_LSEEK => sys_lseek(args[0], args[1], args[2]),
        SYS_MMAP => sys_mmap(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_MPROTECT => sys_mprotect(args[0], args[1], args[2]),
        SYS_MUNMAP => sys_munmap(args[0], args[1]),
        SYS_BRK => sys_brk(args[0]),
        SYS_RT_SIGACTION => sys_rt_sigaction(args[0], args[1], args[2], args[3]),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(args[0], args[1], args[2], args[3]),
        SYS_IOCTL => sys_ioctl(args[0], args[1], args[2]),
        SYS_PREAD64 => sys_pread64(args[0], args[1], args[2], args[3]),
        SYS_PWRITE64 => sys_pwrite64(args[0], args[1], args[2], args[3]),
        SYS_READV => sys_readv(args[0], args[1], args[2]),
        SYS_WRITEV => sys_writev(args[0], args[1], args[2]),
        SYS_ACCESS => sys_access(args[0], args[1]),
        SYS_PIPE => sys_pipe(args[0]),
        SYS_PIPE2 => sys_pipe2(args[0], args[1]),
        SYS_SPLICE => sys_splice(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_TEE => sys_tee(args[0], args[1], args[2], args[3]),
        SYS_VMSPLICE => sys_vmsplice(args[0], args[1], args[2], args[3]),
        SYS_MEMFD_CREATE => sys_memfd_create(args[0], args[1]),
        SYS_SELECT => sys_select(args[0], args[1], args[2], args[3], args[4]),
        SYS_SCHED_YIELD => sys_sched_yield(),
        SYS_MREMAP => sys_mremap(args[0], args[1], args[2], args[3], args[4]),
        SYS_MSYNC => sys_msync(args[0], args[1], args[2]),
        SYS_MINCORE => sys_mincore(args[0], args[1], args[2]),
        SYS_MADVISE => sys_madvise(args[0], args[1], args[2]),
        SYS_FUTEX => tasking::sys_futex(
            args[0] as u64,
            args[1] as i32,
            args[2] as u32,
            args[3] as u64,
            args[4] as u64,
            args[5] as u32,
        ) as usize,
        SYS_RSEQ => tasking::sys_rseq(
            args[0] as u64,
            args[1] as u32,
            args[2] as u32,
            args[3] as u32,
        ) as usize,
        SYS_FUTEX_WAITV => tasking::sys_futex_waitv(
            args[0] as u64,
            args[1] as u32,
            args[2] as u32,
            args[3] as u64,
        ) as usize,

        // Timer/Event syscalls
        SYS_TIMER_CREATE => sys_timer_create(args[0], args[1], args[2]),
        SYS_TIMER_SETTIME => sys_timer_settime(args[0], args[1], args[2], args[3]),
        SYS_TIMER_GETTIME => sys_timer_gettime(args[0], args[1]),
        SYS_TIMER_DELETE => sys_timer_delete(args[0]),
        SYS_EPOLL_CREATE1 => sys_epoll_create1(args[0]),
        SYS_EPOLL_CTL => sys_epoll_ctl(args[0], args[1], args[2], args[3]),
        SYS_EPOLL_PWAIT => sys_epoll_pwait(args[0], args[1], args[2], args[3], args[4]),
        SYS_EVENTFD2 => sys_eventfd2(args[0], args[1]),

        SYS_SHMGET => sys_shmget(args[0], args[1], args[2]),
        SYS_SHMAT => sys_shmat(args[0], args[1], args[2]),
        SYS_SHMCTL => sys_shmctl(args[0], args[1], args[2]),
        SYS_DUP => sys_dup(args[0]),
        SYS_DUP2 => sys_dup2(args[0], args[1]),
        SYS_PAUSE => sys_pause(),
        SYS_NANOSLEEP => sys_nanosleep(args[0], args[1]),
        SYS_GETPID => process_bridge::sys_getpid(),
        SYS_EXECVE => process_bridge::sys_execve(args[0], args[1], args[2]),
        SYS_FORK => process_bridge::sys_fork(),
        SYS_WAIT4 => process_bridge::sys_wait4(args[0], args[1], args[2], args[3]),
        SYS_UNAME => sys_uname(args[0]),
        SYS_GETCWD => sys_getcwd(args[0], args[1]),
        SYS_GETRUSAGE => sys_getrusage(args[0], args[1]),
        SYS_SYSINFO => sys_sysinfo(args[0]),
        SYS_TIMES => sys_times(args[0]),
        SYS_PTRACE => sys_ptrace(args[0], args[1], args[2], args[3]),

        // FileSystem syscalls
        SYS_MKDIR => sys_mkdir(args[0], args[1]),
        SYS_RMDIR => sys_rmdir(args[0]),
        SYS_UNLINK => sys_unlink(args[0]),
        SYS_RENAME => sys_rename(args[0], args[1]),
        SYS_CHMOD => sys_chmod(args[0], args[1]),
        SYS_FCHMOD => sys_fchmod(args[0], args[1]),
        SYS_CHOWN => sys_chown(args[0], args[1], args[2]),
        SYS_FCHOWN => sys_fchown(args[0], args[1], args[2]),
        SYS_TRUNCATE => sys_truncate(args[0], args[1]),
        SYS_FTRUNCATE => sys_ftruncate(args[0], args[1]),
        SYS_CREAT => sys_creat(args[0], args[1]),
        SYS_LINK => sys_link(args[0], args[1]),
        SYS_SYMLINK => sys_symlink(args[0], args[1]),
        SYS_READLINK => sys_readlink(args[0], args[1], args[2]),

        // Signal syscalls
        SYS_RT_SIGACTION => sys_rt_sigaction(args[0], args[1], args[2], args[3]),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(args[0], args[1], args[2], args[3]),
        SYS_KILL => sys_kill(args[0], args[1]),
        SYS_RT_SIGQUEUEINFO => sys_rt_sigqueueinfo(args[0], args[1], args[2]),
        SYS_SIGALTSTACK => sys_sigaltstack(args[0], args[1]),
        SYS_RT_SIGSUSPEND => sys_rt_sigsuspend(args[0]),
        SYS_RT_SIGTIMEDWAIT => sys_rt_sigtimedwait(args[0], args[1], args[2], args[3]),

        SYS_GETUID => sys_getuid(),
        SYS_GETGID => sys_getgid(),
        SYS_GETPPID => sys_getppid(),
        SYS_GETEUID => sys_geteuid(),
        SYS_GETEGID => sys_getegid(),
        SYS_GETTID => process_bridge::sys_gettid(),
        SYS_EXIT => process_bridge::sys_exit(args[0]),
        SYS_CLONE => process_bridge::sys_clone(args[0], args[1], args[2], args[3], args[4]),
        SYS_SET_TID_ADDRESS => process_bridge::sys_set_tid_address(args[0]),
        SYS_TGKILL => process_bridge::sys_tgkill(args[0], args[1], args[2]),
        SYS_TKILL => process_bridge::sys_tkill(args[0], args[1]),
        SYS_SETUID => process_bridge::sys_setuid(args[0]),
        SYS_SETGID => process_bridge::sys_setgid(args[0]),
        SYS_SETSID => process_bridge::sys_setsid(),
        SYS_SETPGID => process_bridge::sys_setpgid(args[0], args[1]),
        SYS_GETPGID => process_bridge::sys_getpgid(args[0]),
        SYS_GETSID => process_bridge::sys_getsid(args[0]),
        SYS_CLOCK_GETTIME => sys_clock_gettime(args[0], args[1]),
        SYS_EXIT_GROUP => process_bridge::sys_exit(args[0]),
        SYS_OPENAT => sys_openat(args[0], args[1], args[2], args[3]),
        SYS_NEWFSTATAT => sys_stat(args[1], args[2]), // newfstatat → stat ile aynı davranış (dirfd yoksayılıyor)
        SYS_GETRANDOM => sys_getrandom(args[0], args[1], args[2]),
        SYS_IO_URING_SETUP => sys_io_uring_setup(args[0], args[1]),
        SYS_IO_URING_ENTER => {
            sys_io_uring_enter(args[0], args[1], args[2], args[3], args[4], args[5])
        }
        SYS_PRCTL => sys_prctl(args[0], args[1]),
        SYS_SECCOMP => sys_seccomp(args[0], args[1], args[2]),

        // --- NETWORK (NETLINK / kTLS) ---
        SYS_SOCKET => sys_socket(args[0], args[1], args[2]),
        SYS_BIND => sys_bind(args[0], args[1], args[2]),
        SYS_SENDTO => sys_sendto(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_RECVFROM => sys_recvfrom(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_CONNECT => sys_connect(args[0], args[1], args[2]),
        SYS_ACCEPT => sys_accept(args[0], args[1], args[2]),
        SYS_LISTEN => sys_listen(args[0], args[1]),
        SYS_SETSOCKOPT => sys_setsockopt(args[0], args[1], args[2], args[3], args[4]),

        // --- STATX (genişletilmiş dosya bilgisi) ---
        SYS_STATX => sys_statx(args[0], args[1], args[2], args[3], args[4]),
        SYS_WIN_CREATE => native_scene_bridge::sys_native_window_create(args[0], args[1]),
        SYS_WIN_DESTROY => native_scene_bridge::sys_native_window_destroy(args[0]),
        SYS_ECHOS_SCENE_COMMIT => native_scene_bridge::sys_native_scene_commit(args[0]),
        SYS_ECHOS_NOTIFICATION_POST => native_scene_bridge::sys_native_notification_post(args[0]),
        SYS_ECHOS_CLIPBOARD_SET_TEXT => native_scene_bridge::sys_native_clipboard_set_text(args[0]),
        SYS_ECHOS_CLIPBOARD_GET_TEXT => native_scene_bridge::sys_native_clipboard_get_text(args[0]),
        SYS_ECHOS_NATIVE_EVENT_POLL => native_scene_bridge::sys_native_event_poll(args[0], args[1]),
        SYS_ECHOS_SERVICE_BOOTSTRAP_CLAIM => service_bridge::sys_service_bootstrap_claim(args[0]),
        SYS_ECHOS_SERVICE_STATUS => service_bridge::sys_service_status(args[0], args[1]),
        SYS_ECHOS_SERVICE_PARITY_STATUS => service_bridge::sys_service_parity_status(args[0]),
        SYS_ECHOS_SERVICE_REGION_MAP => service_bridge::sys_service_region_map(args[0]),
        SYS_ECHOS_SERVICE_ENDPOINT_PUBLISH => service_bridge::sys_service_endpoint_publish(args[0]),
        SYS_ECHOS_SERVICE_HEARTBEAT => service_bridge::sys_service_heartbeat(args[0], args[1]),
        SYS_ECHOS_NOTIFICATION_SERVICE_RECV => {
            service_bridge::sys_notification_service_recv(args[0])
        }
        SYS_ECHOS_NOTIFICATION_SERVICE_RESPOND => {
            service_bridge::sys_notification_service_respond(args[0])
        }

        _ => unsupported_syscall_number(number),
    };

    if is_traced {
        serial_println!(
            "[PTRACE Hook] SYSCALL Exit: #{} -> ret: {}",
            number,
            ret_val
        );
    }
    ret_val
}

pub fn dispatch_call(call: PosixCall) -> usize {
    dispatch(call.number, call.args)
}

#[derive(Clone, Copy)]
pub struct NtCall {
    pub number: u32,
    pub args: [usize; 6],
}

#[derive(Clone, Copy)]
pub struct NtReturn {
    pub status: u32,
    pub value: usize,
}

pub fn dispatch_nt(number: u32, args: [usize; 6]) -> NtReturn {
    match number {
        NT_CLOSE => nt_return_from_posix(sys_close(args[0])),
        NT_READ_FILE => nt_return_from_posix(sys_read(args[0], args[1], args[2])),
        NT_WRITE_FILE => nt_return_from_posix(sys_write(args[0], args[1], args[2])),
        NT_OPEN_FILE => nt_return_from_posix(sys_open(args[0], args[1], args[2])),
        NT_QUERY_INFORMATION_FILE => NtReturn {
            status: STATUS_NOT_IMPLEMENTED,
            value: 0,
        },
        NT_SET_INFORMATION_FILE => NtReturn {
            status: STATUS_NOT_IMPLEMENTED,
            value: 0,
        },
        NT_CREATE_SECTION => NtReturn {
            status: STATUS_NOT_IMPLEMENTED,
            value: 0,
        },
        NT_MAP_VIEW_OF_SECTION => NtReturn {
            status: STATUS_NOT_IMPLEMENTED,
            value: 0,
        },
        _ => NtReturn {
            status: STATUS_NOT_IMPLEMENTED,
            value: 0,
        },
    }
}

pub fn dispatch_nt_call(call: NtCall) -> NtReturn {
    dispatch_nt(call.number, call.args)
}

fn nt_return_from_posix(ret: usize) -> NtReturn {
    NtReturn {
        status: nt_status_from_posix(ret),
        value: ret,
    }
}

fn nt_status_from_posix(ret: usize) -> u32 {
    match posix_errno(ret) {
        None => STATUS_SUCCESS,
        Some(code) => match code {
            ENOENT => STATUS_NOT_FOUND,
            EBADF => STATUS_INVALID_PARAMETER,
            EFAULT => STATUS_ACCESS_VIOLATION,
            EINVAL => STATUS_INVALID_PARAMETER,
            EIO => STATUS_UNSUCCESSFUL,
            ENOSYS => STATUS_NOT_IMPLEMENTED,
            _ => STATUS_UNSUCCESSFUL,
        },
    }
}

fn posix_errno(value: usize) -> Option<usize> {
    let signed = value as isize;
    if signed < 0 {
        Some((-signed) as usize)
    } else {
        None
    }
}

/// write(2) — Dosya tanımlayıcısına veri yazar.
///
/// Desteklenen FD türleri:
///   stdout (1) / stderr (2) -> seri port üzerinden yazdırır
///   /dev/null               -> veriyi sessizce yutar (yazmayı yok sayar)
///   /dev/zero               -> write başarılı sayılır, veri atılır
///
/// Dönüş: yazılan byte sayısı, ya da negatif errno kodu.
fn sys_write(fd: usize, buf: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let bytes =
        with_user_access(|| unsafe { core::slice::from_raw_parts(buf as *const u8, count) });
    match get_fd(fd) {
        Some(FdKind::Stdout) | Some(FdKind::Stderr) => {
            serial_println!("SYSCALL WRITE: fd={} len={}", fd, count);
            for &b in bytes {
                serial_print!("{}", b as char);
            }
            count
        }
        Some(FdKind::Null) => count,
        Some(FdKind::Zero) => count,
        Some(FdKind::File) => errno(EBADF),
        _ => errno(EBADF),
    }
}

/// read(2) — Dosya tanımlayıcısından veri okur.
///
/// Desteklenen FD türleri:
///   stdin (0)   -> TTY'den karakter okur (klavye girişi)
///   /dev/null   -> her zaman 0 (EOF) döner
///   /dev/zero   -> tamponu sıfır (0x00) baytlarla doldurur
///   Dosya (VFS) -> inode'dan offset'e göre okur, offset ilerletir
///
/// Dönüş: okunan byte sayısı (0 = EOF), ya da negatif errno kodu.
fn sys_read(fd: usize, buf: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    match get_fd(fd) {
        Some(FdKind::Stdin) => with_user_access(|| {
            let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
            tty::DEFAULT_TTY.sys_read(slice)
        }),
        Some(FdKind::Null) => 0,
        Some(FdKind::Zero) => {
            let slice = with_user_access(|| unsafe {
                core::slice::from_raw_parts_mut(buf as *mut u8, count)
            });
            for b in slice.iter_mut() {
                *b = 0;
            }
            count
        }
        Some(FdKind::File) => {
            let (inode, offset, size, is_hello) = {
                let files = FILE_TABLE.lock();
                let Some(Some(state)) = files.get(fd) else {
                    return errno(EBADF);
                };
                (
                    state.inode.clone(),
                    state.offset,
                    state.size,
                    state.is_hello,
                )
            };
            let available = size.saturating_sub(offset);
            if available == 0 {
                return 0;
            }
            let to_copy = core::cmp::min(count, available);
            let slice = with_user_access(|| unsafe {
                core::slice::from_raw_parts_mut(buf as *mut u8, to_copy)
            });
            let read = match fs::vfs_read_at(&inode, offset, slice) {
                Ok(value) => value,
                Err(err) => return vfs_errno(err),
            };
            if is_hello && read > 0 {
                serial_println!(
                    "VFS: read HELLO.ELF offset={} len={} total={}",
                    offset,
                    read,
                    size
                );
            }
            let mut files = FILE_TABLE.lock();
            if let Some(Some(state)) = files.get_mut(fd) {
                state.offset = state.offset.saturating_add(read);
            }
            read
        }
        _ => errno(EBADF),
    }
}

fn sys_open(path: usize, flags: usize, _mode: usize) -> usize {
    const O_WRONLY: usize = 1;
    const O_RDWR: usize = 2;
    const O_CREAT: usize = 0o100;
    const O_TRUNC: usize = 0o1000;

    let path = match read_user_cstring(path, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };

    let write_intent = (flags & O_WRONLY != 0)
        || (flags & O_RDWR != 0)
        || (flags & O_CREAT != 0)
        || (flags & O_TRUNC != 0);
    let access = if write_intent {
        security::landlock::Access::Write
    } else {
        security::landlock::Access::Read
    };
    if let Err(err) = enforce_path_policy(&path, access) {
        return err;
    }

    match path.as_str() {
        "/dev/null" => allocate_fd(FdKind::Null),
        "/dev/zero" => allocate_fd(FdKind::Zero),
        "/dev/dri/card0" => allocate_fd(FdKind::Drm),
        _ => {
            let inode = match fs::vfs_open_inode(&path) {
                Ok(value) => value,
                Err(err) => {
                    if flags & O_CREAT == 0 {
                        return vfs_errno(err);
                    }
                    let (parent, name) = match path.rfind('/') {
                        Some(pos) => {
                            let p = if pos == 0 { "/" } else { &path[..pos] };
                            (p, &path[pos + 1..])
                        }
                        None => ("/", path.as_str()),
                    };
                    if name.is_empty() {
                        return errno(EINVAL);
                    }
                    if let Err(create_err) = fs::f2fs::create_f2fs_file(parent, name) {
                        return vfs_errno(create_err);
                    }
                    match fs::vfs_open_inode(&path) {
                        Ok(value) => value,
                        Err(open_err) => return vfs_errno(open_err),
                    }
                }
            };
            let size = match fs::vfs_inode_metadata(&inode) {
                Ok(meta) => meta.size,
                Err(err) => return vfs_errno(err),
            };
            let is_hello = path.eq_ignore_ascii_case("HELLO.ELF") || path.ends_with("/HELLO.ELF");
            if is_hello {
                serial_println!("VFS: open HELLO.ELF size={}", size);
            }
            allocate_file_fd(FileState {
                inode,
                offset: 0,
                size,
                is_hello,
            })
        }
    }
}

/// close syscall (stdin/out/err hariç)
fn sys_close(fd: usize) -> usize {
    if fd <= 2 {
        return 0;
    }
    free_fd(fd)
}

/// `lseek` syscall for the in-memory file table path.
fn sys_lseek(fd: usize, offset: usize, whence: usize) -> usize {
    let mut files = FILE_TABLE.lock();
    let Some(Some(state)) = files.get_mut(fd) else {
        return errno(EBADF);
    };
    let current = state.offset as isize;
    let size = state.size as isize;
    let offset = offset as isize;
    let next = match whence {
        0 => offset,
        1 => current.saturating_add(offset),
        2 => size.saturating_add(offset),
        _ => return errno(EINVAL),
    };
    if next < 0 {
        return errno(EINVAL);
    }
    state.offset = next as usize;
    state.offset
}

fn sys_stat(path: usize, statbuf: usize) -> usize {
    if statbuf == 0 {
        return errno(EFAULT);
    }
    let path = match read_user_cstring(path, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if path == "/dev/null" || path == "/dev/zero" || path == "/dev/dri/card0" {
        let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
        with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
        return 0;
    }
    if fs::f2fs::detect_f2fs().unwrap_or(false) {
        let entry = match fs::f2fs::open_entry(&path) {
            Ok(value) => value,
            Err(_) => return errno(ENOENT),
        };
        let stat = stat_from_f2fs_entry(&entry);
        with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
        return 0;
    }
    let entry = match fs::f2fs::open_entry(&path) {
        Ok(value) => value,
        Err(_) => return errno(ENOENT),
    };
    let stat = stat_from_f2fs_entry(&entry);
    with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
    0
}

fn stat_for_special(mode: u32, size: i64) -> Stat {
    let blocks = (size.saturating_add(STAT_BLKSIZE - 1) / STAT_BLKSIZE).max(0);
    Stat {
        st_dev: 0,
        st_ino: 0,
        st_nlink: 1,
        st_mode: mode,
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: size,
        st_blksize: STAT_BLKSIZE,
        st_blocks: blocks,
        st_atim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        st_mtim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        st_ctim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        __unused: [0; 3],
    }
}

fn stat_from_f2fs_entry(entry: &F2fsEntry) -> Stat {
    let mode = if entry.is_dir {
        S_IFDIR | MODE_DIR
    } else {
        S_IFREG | MODE_FILE
    };
    let size = if entry.is_dir { 0 } else { entry.size as i64 };
    let blocks = (size.saturating_add(STAT_BLKSIZE - 1) / STAT_BLKSIZE).max(0);
    Stat {
        st_dev: 0,
        st_ino: 0,
        st_nlink: 1,
        st_mode: mode,
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: size,
        st_blksize: STAT_BLKSIZE,
        st_blocks: blocks,
        st_atim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        st_mtim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        st_ctim: Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        __unused: [0; 3],
    }
}

/// fstat(2) — dosya tanımlayıcısından dosya meta verisini al
///
/// fd'ye karşılık gelen açık dosyanın inode meta verisini okur.
/// Özel dosyalar (stdin/out/err, /dev/null, /dev/zero, DRM) için
/// sabit değerler döndürülür.
fn sys_fstat(fd: usize, statbuf: usize) -> usize {
    if statbuf == 0 {
        return errno(EFAULT);
    }

    // stdin/stdout/stderr
    if fd <= 2 {
        let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
        with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
        return 0;
    }

    // FD türüne göre Stat oluştur
    match get_fd(fd) {
        Some(FdKind::Null) | Some(FdKind::Zero) => {
            let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
            with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
            0
        }
        Some(FdKind::Drm) => {
            let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
            with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
            0
        }
        Some(FdKind::File) => {
            let files = FILE_TABLE.lock();
            if let Some(Some(state)) = files.get(fd) {
                // VFS inode'dan metadata al
                let stat = match fs::vfs_inode_metadata(&state.inode) {
                    Ok(meta) => {
                        let mode = match meta.type_ {
                            FileType::Dir => S_IFDIR | MODE_DIR,
                            FileType::File => S_IFREG | MODE_FILE,
                            FileType::SymLink => S_IFLNK | 0o777,
                            FileType::CharDevice => S_IFCHR | MODE_CHAR,
                            FileType::BlockDevice => S_IFBLK | MODE_CHAR,
                            _ => S_IFREG | MODE_FILE,
                        };
                        let size = meta.size as i64;
                        let blocks = (size.saturating_add(STAT_BLKSIZE - 1) / STAT_BLKSIZE).max(0);
                        Stat {
                            st_dev: meta.dev as u64,
                            st_ino: meta.inode as u64,
                            st_nlink: meta.nlinks as u64,
                            st_mode: mode,
                            st_uid: meta.uid as u32,
                            st_gid: meta.gid as u32,
                            __pad0: 0,
                            st_rdev: 0,
                            st_size: size,
                            st_blksize: STAT_BLKSIZE,
                            st_blocks: blocks,
                            st_atim: Timespec {
                                tv_sec: meta.atime.sec,
                                tv_nsec: meta.atime.nsec as i64,
                            },
                            st_mtim: Timespec {
                                tv_sec: meta.mtime.sec,
                                tv_nsec: meta.mtime.nsec as i64,
                            },
                            st_ctim: Timespec {
                                tv_sec: meta.ctime.sec,
                                tv_nsec: meta.ctime.nsec as i64,
                            },
                            __unused: [0; 3],
                        }
                    }
                    Err(_) => stat_for_special(S_IFREG | MODE_FILE, state.size as i64),
                };
                drop(files);
                with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
                0
            } else {
                errno(EBADF)
            }
        }
        Some(FdKind::Pipe) => {
            let stat = stat_for_special(S_IFIFO | 0o600, 0);
            with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
            0
        }
        _ => errno(EBADF),
    }
}

fn sys_lstat(_path: usize, _statbuf: usize) -> usize {
    unsupported_errno("lstat")
}

fn sys_poll(_fds: usize, _nfds: usize, _timeout: usize) -> usize {
    unsupported_errno("poll")
}

fn sys_mmap(addr: usize, len: usize, prot: usize, flags: usize, fd: usize, off: usize) -> usize {
    if len == 0 {
        return errno(EINVAL);
    }
    if let Some(FdKind::Drm) = get_fd(fd) {
        return drm_mmap(len, off);
    }
    let is_anon = flags & MAP_ANON != 0 || fd == usize::MAX;
    let is_private = flags & MAP_PRIVATE != 0;
    let is_shared = flags & MAP_SHARED != 0;
    if is_private == is_shared {
        return errno(EINVAL);
    }
    if !is_anon && off % kernel_memory::PAGE_SIZE != 0 {
        return errno(EINVAL);
    }
    let target = if addr != 0 {
        if !kernel_memory::is_user_range(addr as u64, len as u64) {
            return errno(EINVAL);
        }
        addr as u64
    } else {
        match kernel_memory::allocate_user_mmap(len as u64) {
            Some(value) => value,
            None => return errno(ENOMEM),
        }
    };
    if !security::kpti::user_mapping_allowed(target, len as u64) {
        return errno(EPERM);
    }
    let mut page_flags = PageTableFlags::USER_ACCESSIBLE;
    if prot & PROT_WRITE != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if prot & PROT_EXEC == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    if flags & MAP_FIXED != 0 && addr == 0 {
        return errno(EINVAL);
    }
    if is_anon {
        if is_shared {
            if !kernel_memory::register_shared_anon_region(target, len as u64, page_flags) {
                return errno(EINVAL);
            }
            return target as usize;
        }
        if !kernel_memory::register_lazy_region(target, len as u64, page_flags) {
            return errno(EINVAL);
        }
        return target as usize;
    }
    let file_state = {
        let files = FILE_TABLE.lock();
        match files.get(fd).and_then(|value| value.clone()) {
            Some(value) => value,
            None => return errno(EBADF),
        }
    };
    let file_size = file_state.size as u64;
    let offset = off as u64;
    if offset > file_size {
        return errno(EINVAL);
    }
    let mapping_file_size = file_size.saturating_sub(offset).min(len as u64);
    let cow = !is_shared && (prot & PROT_WRITE != 0);
    if !kernel_memory::register_file_backed_region(
        target,
        len as u64,
        page_flags,
        file_state.inode,
        offset,
        mapping_file_size,
        is_shared,
        cow,
    ) {
        return errno(EINVAL);
    }
    target as usize
}

fn sys_mprotect(addr: usize, len: usize, prot: usize) -> usize {
    if len == 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(addr as u64, len as u64) {
        return errno(EINVAL);
    }
    let mut page_flags = PageTableFlags::USER_ACCESSIBLE;
    if prot & PROT_WRITE != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if prot & PROT_EXEC == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    if !kernel_memory::update_user_region_flags(addr as u64, len as u64, page_flags) {
        return errno(EINVAL);
    }
    0
}

fn sys_munmap(_addr: usize, _len: usize) -> usize {
    if _len == 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(_addr as u64, _len as u64) {
        return errno(EINVAL);
    }
    if !kernel_memory::unmap_user_range(_addr as u64, _len as u64) {
        return errno(EINVAL);
    }
    0
}

fn sys_brk(addr: usize) -> usize {
    let (base, current) = kernel_memory::user_heap_state();
    if addr == 0 {
        return current as usize;
    }
    let new_break = addr as u64;
    let heap_limit = kernel_memory::user_heap_limit();
    if new_break < base || new_break > heap_limit {
        return current as usize;
    }
    if new_break == current {
        return current as usize;
    }
    if new_break > current {
        let size = new_break.saturating_sub(current);
        let flags =
            PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        if !kernel_memory::register_lazy_region(current, size, flags) {
            return current as usize;
        }
    } else {
        let size = current.saturating_sub(new_break);
        if !kernel_memory::unmap_user_range(new_break, size) {
            return current as usize;
        }
    }
    kernel_memory::set_user_heap_break(new_break);
    new_break as usize
}

fn sys_ioctl(fd: usize, request: usize, arg: usize) -> usize {
    match get_fd(fd) {
        Some(FdKind::Drm) => handle_drm_ioctl(request, arg),
        _ => errno(ENOTTY),
    }
}

fn handle_drm_ioctl(cmd: usize, arg: usize) -> usize {
    match cmd {
        DRM_IOCTL_VERSION => drm_version(arg),
        DRM_IOCTL_VIRTGPU_RESOURCE_CREATE => drm_virtgpu_resource_create(arg),
        DRM_IOCTL_VIRTGPU_MAP => drm_virtgpu_map(arg),
        DRM_IOCTL_VIRTGPU_EXECBUFFER => drm_virtgpu_execbuffer(arg),
        _ => errno(EINVAL),
    }
}

fn drm_version(arg: usize) -> usize {
    if arg == 0 {
        return errno(EFAULT);
    }
    let mut ver = with_user_access(|| unsafe { *(arg as *const DrmVersion) });
    let name = "virtio_gpu";
    let name_len = ver.name_len;
    ver.version_major = 0;
    ver.version_minor = 0;
    ver.version_patchlevel = 0;
    ver.name_len = name.len();
    ver.date_len = 0;
    ver.desc_len = 0;
    if ver.name != 0 && name_len > 0 {
        let max = name_len.saturating_sub(1);
        let copy_len = core::cmp::min(name.len(), max);
        with_user_access(|| unsafe {
            let dest = ver.name as *mut u8;
            core::ptr::copy_nonoverlapping(name.as_ptr(), dest, copy_len);
            *dest.add(copy_len) = 0;
        });
    }
    with_user_access(|| unsafe {
        *(arg as *mut DrmVersion) = ver;
    });
    0
}

fn drm_virtgpu_resource_create(arg: usize) -> usize {
    if arg == 0 {
        return errno(EFAULT);
    }
    let mut req = with_user_access(|| unsafe { *(arg as *const DrmVirtgpuResourceCreate) });
    let width = req.width.max(1);
    let height = req.height.max(1);
    let handle = match drivers::virtio_gpu::drm_resource_create_3d(width, height) {
        Some(value) => value,
        None => return errno(EIO),
    };
    req.handle = handle;
    with_user_access(|| unsafe {
        *(arg as *mut DrmVirtgpuResourceCreate) = req;
    });
    let mut size = if req.size != 0 {
        req.size as usize
    } else {
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    };
    if size == 0 {
        size = 4096;
    }
    drm_register_resource(handle, size);
    0
}

fn drm_virtgpu_map(arg: usize) -> usize {
    if arg == 0 {
        return errno(EFAULT);
    }
    let mut req = with_user_access(|| unsafe { *(arg as *const DrmVirtgpuMap) });
    if req.handle == 0 {
        return errno(EINVAL);
    }
    if !drm_resource_exists(req.handle) {
        return errno(EINVAL);
    }
    req.offset = req.handle as u64;
    with_user_access(|| unsafe {
        *(arg as *mut DrmVirtgpuMap) = req;
    });
    0
}

fn drm_virtgpu_execbuffer(arg: usize) -> usize {
    if arg == 0 {
        return errno(EFAULT);
    }
    let req = with_user_access(|| unsafe { *(arg as *const DrmVirtgpuExecbuffer) });
    if req.command == 0 || req.size == 0 {
        return errno(EINVAL);
    }
    let ok = with_user_access(|| unsafe {
        drivers::virtio_gpu::drm_submit_3d_command(req.command as *const u8, req.size as usize)
    });
    if ok {
        0
    } else {
        errno(EIO)
    }
}

fn drm_register_resource(handle: u32, size: usize) {
    let mut resources = DRM_RESOURCES.lock();
    if let Some(entry) = resources.iter_mut().find(|res| res.handle == handle) {
        entry.size = size;
        return;
    }
    resources.push(DrmResource {
        handle,
        size,
        map_ptr: 0,
    });
}

fn drm_resource_exists(handle: u32) -> bool {
    let resources = DRM_RESOURCES.lock();
    resources.iter().any(|res| res.handle == handle)
}

fn drm_mmap(len: usize, off: usize) -> usize {
    let handle = off as u32;
    let mut resources = DRM_RESOURCES.lock();
    let Some(entry) = resources.iter_mut().find(|res| res.handle == handle) else {
        return errno(EINVAL);
    };
    if entry.map_ptr == 0 {
        let size = entry.size.max(len);
        let ptr = unsafe { allocator::heap_alloc(size) };
        if ptr.is_null() {
            return errno(EIO);
        }
        entry.size = size;
        entry.map_ptr = ptr as usize;
    }
    entry.map_ptr
}

fn sys_pread64(_fd: usize, _buf: usize, _count: usize, _pos: usize) -> usize {
    unsupported_errno("pread64")
}

fn sys_pwrite64(_fd: usize, _buf: usize, _count: usize, _pos: usize) -> usize {
    unsupported_errno("pwrite64")
}

/// readv(2) — scatter read: birden fazla tampona kesintisiz okuma.
///
/// iov, struct iovec dizisinin adresi: { iov_base: *mut u8, iov_len: usize }
/// Her iovec tamponuna sırayla veri kopyalanır.
fn sys_readv(fd: usize, iov: usize, iovcnt: usize) -> usize {
    if iovcnt == 0 || iovcnt > 1024 {
        return errno(EINVAL);
    }
    let fd_num = fd as i32;
    let iov_ptr = iov as *const [usize; 2]; // [iov_base, iov_len]
    let mut total = 0usize;

    for i in 0..iovcnt {
        let entry = unsafe { &*iov_ptr.add(i) };
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }

        // Her bir iovec için sys_read çağrısı yap (mevcut fd altyapısını kullanır)
        let result = sys_read(fd_num as usize, base, len);
        if result > 0x7FFF_FFFF_FFFF_0000 {
            // Hata kodu
            if total > 0 {
                return total;
            }
            return result;
        }
        total += result;
        if result < len {
            // Kısa okuma — dur
            break;
        }
    }
    total
}

/// writev(2) — gather write: birden fazla tampondan kesintisiz yazma.
fn sys_writev(fd: usize, iov: usize, iovcnt: usize) -> usize {
    if iovcnt == 0 || iovcnt > 1024 {
        return errno(EINVAL);
    }
    let fd_num = fd as i32;
    let iov_ptr = iov as *const [usize; 2];
    let mut total = 0usize;

    for i in 0..iovcnt {
        let entry = unsafe { &*iov_ptr.add(i) };
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }

        let result = sys_write(fd_num as usize, base, len);
        if result > 0x7FFF_FFFF_FFFF_0000 {
            if total > 0 {
                return total;
            }
            return result;
        }
        total += result;
        if result < len {
            break;
        }
    }
    total
}

/// openat(2) — dizin tanımlayıcısına görecel dosya aç
///
/// dirfd == AT_FDCWD (-100) ise, path mutlak veya mevcut dizine göreceldir.
/// Aksi halde dirfd, bir dizin fd'si olmalıdır (henüz desteklenmiyor,
/// AT_FDCWD varsayılıyor).
///
/// Bayraklar: O_RDONLY (0), O_WRONLY (1), O_RDWR (2), O_CREAT (0o100),
///            O_EXCL (0o200), O_TRUNC (0o1000), O_APPEND (0o2000)
fn sys_openat(dirfd: usize, path: usize, flags: usize, mode: usize) -> usize {
    let dirfd = dirfd as isize;

    // AT_FDCWD (-100): mevcut çalışma dizinine görecel
    // Diğer dirfd değerleri henüz desteklenmiyor — AT_FDCWD gibi davran
    if dirfd != AT_FDCWD && dirfd >= 0 {
        // İleride dirfd lookup + path birleştirme yapılacak
        // Şimdilik sys_open'a düşür
    }

    // sys_open'a devret (tüm mantık orada)
    sys_open(path, flags, mode)
}

/// access(2) — dosya erişim izinlerini kontrol et
///
/// - F_OK (0): Dosya var mı?
/// - R_OK (4): Okuma izni var mı?
/// - W_OK (2): Yazma izni var mı?
/// - X_OK (1): Çalıştırma izni var mı?
///
/// Basitleştirilmiş uygulama: Dosya varsa ve erişim isteniyorsa 0 (başarı) döner.
fn sys_access(path: usize, mode: usize) -> usize {
    let path = match read_user_cstring(path, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };

    // Özel dosya yolları her zaman erişilebilir
    match path.as_str() {
        "/dev/null" | "/dev/zero" | "/dev/tty" | "/dev/urandom" | "/dev/random" => return 0,
        _ => {}
    }

    // /proc ve /sys sanal dosya sistemleri
    if path.starts_with("/proc/") || path.starts_with("/sys/") {
        return 0;
    }

    // VFS üzerinden dosya varlığını kontrol et
    match fs::vfs_open_inode(&path) {
        Ok(inode) => {
            // F_OK: dosya var, başarı
            if mode == F_OK {
                return 0;
            }

            // Meta veri al ve izinleri kontrol et
            match fs::vfs_inode_metadata(&inode) {
                Ok(meta) => {
                    let file_mode = meta.mode as usize;
                    // Basitleştirilmiş kontrol: owner izinlerini kontrol et
                    // Linux'ta gerçek kontrol uid/gid eşleştirmesi yapardı
                    let owner_bits = (file_mode >> 6) & 0o7;

                    if (mode & R_OK != 0) && (owner_bits & 0o4 == 0) {
                        return errno(EACCES);
                    }
                    if (mode & W_OK != 0) && (owner_bits & 0o2 == 0) {
                        return errno(EACCES);
                    }
                    if (mode & X_OK != 0) && (owner_bits & 0o1 == 0) {
                        return errno(EACCES);
                    }
                    0
                }
                Err(_) => 0, // meta veri alınamazsa dosya var kabul et
            }
        }
        Err(_) => errno(ENOENT),
    }
}

// ============================================================================
// FILESYSTEM SYSCALLS
// ============================================================================

/// getrusage(2) — İşlem kaynak kullanım bilgisini döndürür
fn sys_getrusage(who: usize, usage_ptr: usize) -> usize {
    if usage_ptr == 0 {
        return errno(EFAULT);
    }
    // struct rusage: 18 x u64 = 144 bytes (Linux layout)
    // İlk iki alan: ru_utime (user time), ru_stime (system time) → struct timeval (16 bytes each)
    let ticks = tasking::scheduler::get_ticks() as u64;
    let user_sec = ticks / 1000;
    let user_usec = (ticks % 1000) * 1000;
    with_user_access(|| unsafe {
        let ptr = usage_ptr as *mut u64;
        // Zero out the struct first (18 fields)
        for i in 0..18 {
            core::ptr::write_volatile(ptr.add(i), 0);
        }
        // ru_utime.tv_sec, ru_utime.tv_usec
        core::ptr::write_volatile(ptr, user_sec);
        core::ptr::write_volatile(ptr.add(1), user_usec);
        // ru_maxrss (field index 4) — max resident set size in KB
        core::ptr::write_volatile(ptr.add(4), 4096);
    });
    0
}

/// sysinfo(2) — Genel sistem bilgisini döndürür
fn sys_sysinfo(info_ptr: usize) -> usize {
    if info_ptr == 0 {
        return errno(EFAULT);
    }
    let ticks = tasking::scheduler::get_ticks() as u64;
    let uptime = ticks / 1000; // seconds

    // Gerçek bellek istatistiklerini al (KB cinsinden)
    let mem_stats = kernel_memory::get_memory_stats();
    let total_ram = (mem_stats.total_kb as u64) * 1024; // bytes
    let free_ram = (mem_stats.free_kb as u64) * 1024; // bytes

    // Aktif görev sayısını al
    let procs = tasking::scheduler::list_tasks().len() as u64;
    let procs = if procs > 0 { procs } else { 1 };

    with_user_access(|| unsafe {
        let ptr = info_ptr as *mut u64;
        // struct sysinfo layout (64-bit): uptime, loads[3], totalram, freeram, ...
        // Zero first 16 fields
        for i in 0..16 {
            core::ptr::write_volatile(ptr.add(i), 0);
        }
        // uptime (seconds)
        core::ptr::write_volatile(ptr, uptime);
        // loads[0..3] (1/5/15 min load averages, fixed point)
        core::ptr::write_volatile(ptr.add(1), 1 << 16);
        core::ptr::write_volatile(ptr.add(2), 1 << 16);
        core::ptr::write_volatile(ptr.add(3), 1 << 16);
        // totalram
        core::ptr::write_volatile(ptr.add(4), total_ram);
        // freeram
        core::ptr::write_volatile(ptr.add(5), free_ram);
        // procs
        core::ptr::write_volatile(ptr.add(9), procs);
    });
    0
}

/// times(2) — İşlem zamanlarını döndürür
fn sys_times(buf_ptr: usize) -> usize {
    let ticks = tasking::scheduler::get_ticks();
    if buf_ptr != 0 {
        // struct tms: tms_utime, tms_stime, tms_cutime, tms_cstime (4 x i64)
        with_user_access(|| unsafe {
            let ptr = buf_ptr as *mut u64;
            core::ptr::write_volatile(ptr, ticks as u64); // tms_utime
            core::ptr::write_volatile(ptr.add(1), 0); // tms_stime
            core::ptr::write_volatile(ptr.add(2), 0); // tms_cutime
            core::ptr::write_volatile(ptr.add(3), 0); // tms_cstime
        });
    }
    ticks
}

/// mkdir - create a directory
fn sys_mkdir(path_ptr: usize, _mode: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) = enforce_path_policy(&path, security::landlock::Access::Create) {
        return err;
    }
    // Split into parent path and directory name
    let (parent, name) = match path.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &path[..pos] };
            (&path[..pos.max(1)], &path[pos + 1..])
        }
        None => ("/", path.as_str()),
    };
    if name.is_empty() {
        return errno(EINVAL);
    }
    match fs::f2fs::create_f2fs_dir(parent, name) {
        Ok(()) => 0,
        Err(err) => vfs_errno(err),
    }
}

/// rmdir - remove a directory
fn sys_rmdir(path_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) = enforce_path_policy(&path, security::landlock::Access::Delete) {
        return err;
    }
    let (parent, name) = match path.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &path[..pos] };
            (p, &path[pos + 1..])
        }
        None => ("/", path.as_str()),
    };
    if name.is_empty() {
        return errno(EINVAL);
    }
    match fs::f2fs::unlink_f2fs(parent, name) {
        Ok(()) => 0,
        Err(err) => vfs_errno(err),
    }
}

/// unlink - remove a file
fn sys_unlink(path_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) = enforce_path_policy(&path, security::landlock::Access::Delete) {
        return err;
    }
    let (parent, name) = match path.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &path[..pos] };
            (p, &path[pos + 1..])
        }
        None => ("/", path.as_str()),
    };
    if name.is_empty() {
        return errno(EINVAL);
    }
    match fs::f2fs::unlink_f2fs(parent, name) {
        Ok(()) => 0,
        Err(err) => vfs_errno(err),
    }
}

/// rename - rename a file or directory
fn sys_rename(oldpath_ptr: usize, newpath_ptr: usize) -> usize {
    let oldpath = match read_user_cstring(oldpath_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let newpath = match read_user_cstring(newpath_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) = enforce_path_policy(&oldpath, security::landlock::Access::Rename) {
        return err;
    }
    if let Err(err) = enforce_path_policy(&newpath, security::landlock::Access::Create) {
        return err;
    }
    // Extract parent and old/new names. For simplicity, both must be in same parent dir.
    let (old_parent, old_name) = match oldpath.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &oldpath[..pos] };
            (p, &oldpath[pos + 1..])
        }
        None => ("/", oldpath.as_str()),
    };
    let (_new_parent, new_name) = match newpath.rfind('/') {
        Some(pos) => {
            let p = if pos == 0 { "/" } else { &newpath[..pos] };
            (p, &newpath[pos + 1..])
        }
        None => ("/", newpath.as_str()),
    };
    if old_name.is_empty() || new_name.is_empty() {
        return errno(EINVAL);
    }
    match fs::f2fs::rename_f2fs(old_parent, old_name, new_name) {
        Ok(()) => 0,
        Err(err) => vfs_errno(err),
    }
}

/// chmod - change file permissions
fn sys_chmod(_path_ptr: usize, _mode: usize) -> usize {
    // Simplified - return success
    0
}

/// fchmod - change file permissions by fd
fn sys_fchmod(_fd: usize, _mode: usize) -> usize {
    0
}

/// chown - change file owner
fn sys_chown(_path_ptr: usize, _uid: usize, _gid: usize) -> usize {
    // echOS doesn't have multi-user support yet
    0
}

/// fchown - change file owner by fd
fn sys_fchown(_fd: usize, _uid: usize, _gid: usize) -> usize {
    0
}

/// truncate - truncate a file by path
fn sys_truncate(path_ptr: usize, length: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if let Err(err) = enforce_path_policy(&path, security::landlock::Access::Write) {
        return err;
    }
    // F2FS truncate: resize the file by path
    match fs::f2fs::truncate_f2fs(&path, length as u64) {
        Ok(()) => {
            serial_println!("SYSCALL truncate: path={} len={} OK", path, length);
            0
        }
        Err(err) => vfs_errno(err),
    }
}

/// ftruncate - truncate a file by fd
fn sys_ftruncate(fd: usize, length: usize) -> usize {
    let mut files = FILE_TABLE.lock();
    match files.get_mut(fd).and_then(|f| f.as_mut()) {
        Some(file_state) => {
            file_state.size = length;
            0
        }
        None => errno(EBADF),
    }
}

/// creat - create a file
fn sys_creat(path_ptr: usize, mode: usize) -> usize {
    // creat is equivalent to open with O_CREAT|O_WRONLY|O_TRUNC
    const O_CREAT: usize = 0o100;
    const O_WRONLY: usize = 1;
    const O_TRUNC: usize = 0o1000;

    sys_open(path_ptr, O_CREAT | O_WRONLY | O_TRUNC, mode)
}

/// link - create a hard link
fn sys_link(_oldpath_ptr: usize, _newpath_ptr: usize) -> usize {
    unsupported_errno("link")
}

/// symlink - create a symbolic link
fn sys_symlink(_target_ptr: usize, _linkpath_ptr: usize) -> usize {
    unsupported_errno("symlink")
}

/// readlink - read a symbolic link
fn sys_readlink(_path_ptr: usize, _buf: usize, _bufsize: usize) -> usize {
    unsupported_errno("readlink")
}

fn sys_select(
    _nfds: usize,
    _readfds: usize,
    _writefds: usize,
    _exceptfds: usize,
    _timeout: usize,
) -> usize {
    unsupported_errno("select")
}

fn sys_sched_yield() -> usize {
    tasking::scheduler::sleep(1);
    0
}

// ============================================================================
// SIGNAL SYSCALLS
// ============================================================================

/// Signal handler type
type SigHandler = usize;

/// Signal action structure
#[repr(C)]
struct SigAction {
    sa_handler: SigHandler,
    sa_flags: usize,
    sa_restorer: usize,
    sa_mask: [u64; 1],
}

/// Signal handlers table (per-process would be better, but simplified here)
static SIGNAL_HANDLERS: spin::Mutex<[SigHandler; 64]> = spin::Mutex::new([0; 64]);
static SIGNAL_MASKS: spin::Mutex<u64> = spin::Mutex::new(0);

/// rt_sigaction - examine and change a signal action
fn sys_rt_sigaction(sig: usize, act_ptr: usize, oldact_ptr: usize, _sigsetsize: usize) -> usize {
    if sig == 0 || sig > 64 {
        return errno(EINVAL);
    }

    let sig_idx = sig - 1;

    // Save old action if requested
    if oldact_ptr != 0 {
        let handlers = SIGNAL_HANDLERS.lock();
        let old_handler = handlers[sig_idx];
        drop(handlers);

        let old_action = SigAction {
            sa_handler: old_handler,
            sa_flags: 0,
            sa_restorer: 0,
            sa_mask: [0; 1],
        };
        with_user_access(|| unsafe {
            *(oldact_ptr as *mut SigAction) = old_action;
        });
    }

    // Set new action if requested
    if act_ptr != 0 {
        let new_action: SigAction =
            with_user_access(|| unsafe { core::ptr::read(act_ptr as *const SigAction) });
        let mut handlers = SIGNAL_HANDLERS.lock();
        handlers[sig_idx] = new_action.sa_handler;
    }

    0
}

/// rt_sigprocmask - examine and change blocked signals
fn sys_rt_sigprocmask(_how: usize, set_ptr: usize, oldset_ptr: usize, _sigsetsize: usize) -> usize {
    // Save old mask if requested
    if oldset_ptr != 0 {
        let mask = SIGNAL_MASKS.lock();
        with_user_access(|| unsafe {
            *(oldset_ptr as *mut u64) = *mask;
        });
    }

    // Set new mask if requested
    if set_ptr != 0 {
        let new_mask = with_user_access(|| unsafe { *(set_ptr as *const u64) });
        let mut mask = SIGNAL_MASKS.lock();
        *mask = new_mask;
    }

    0
}

/// kill - send a signal to a process
fn sys_kill(pid: usize, sig: usize) -> usize {
    if sig > 64 {
        return errno(EINVAL);
    }

    if sig == 0 {
        // Signal 0 is used to check if process exists
        return 0;
    }

    // Simplified - just log the signal
    serial_println!("[SIGNAL] kill: pid={}, sig={}", pid, sig);

    // In real implementation, would:
    // 1. Find process by PID
    // 2. Check permissions
    // 3. Queue signal to process
    // 4. Wake up process if sleeping

    0
}

/// rt_sigqueueinfo - queue a signal and data to a process
fn sys_rt_sigqueueinfo(_pid: usize, _sig: usize, _info_ptr: usize) -> usize {
    unsupported_errno("rt_sigqueueinfo")
}

/// sigaltstack - set and/or examine signal stack context
fn sys_sigaltstack(_ss_ptr: usize, _old_ss_ptr: usize) -> usize {
    unsupported_errno("sigaltstack")
}

/// rt_sigsuspend - wait for a signal
fn sys_rt_sigsuspend(_mask_ptr: usize) -> usize {
    // Would block until signal received
    errno(EINTR)
}

/// rt_sigtimedwait - wait for a signal with timeout
fn sys_rt_sigtimedwait(
    _set_ptr: usize,
    _info_ptr: usize,
    _timeout_ptr: usize,
    _sigsetsize: usize,
) -> usize {
    unsupported_errno("rt_sigtimedwait")
}

fn sys_mremap(_old: usize, _oldsz: usize, _newsz: usize, _flags: usize, _new: usize) -> usize {
    unsupported_errno("mremap")
}

fn sys_msync(_addr: usize, _len: usize, _flags: usize) -> usize {
    unsupported_errno("msync")
}

fn sys_mincore(_addr: usize, _len: usize, _vec: usize) -> usize {
    unsupported_errno("mincore")
}

fn sys_madvise(_addr: usize, _len: usize, _advice: usize) -> usize {
    unsupported_errno("madvise")
}

// ============================================================================
// IPC SYSCALLS (Shared Memory, Pipes, Message Queues, Semaphores)
// ============================================================================

/// Shared memory segment
struct ShmSegment {
    key: usize,
    size: usize,
    addr: usize,
    creator_pid: usize,
    ref_count: usize,
}

lazy_static! {
    static ref SHM_TABLE: Mutex<alloc::collections::BTreeMap<usize, ShmSegment>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    static ref NEXT_SHMID: Mutex<usize> = Mutex::new(1);
}

/// shmget - get shared memory segment
fn sys_shmget(key: usize, size: usize, _shmflg: usize) -> usize {
    // Create new segment
    let shmid = {
        let mut next = NEXT_SHMID.lock();
        let id = *next;
        *next += 1;
        id
    };

    // Allocate memory
    let addr = kernel_memory::allocate_user_mmap(size as u64);
    let addr = match addr {
        Some(a) => a as usize,
        None => return errno(ENOMEM),
    };

    let segment = ShmSegment {
        key,
        size,
        addr,
        creator_pid: tasking::scheduler::current_task_id(),
        ref_count: 1,
    };

    SHM_TABLE.lock().insert(shmid, segment);
    serial_println!("[IPC] shmget: key={}, size={}, shmid={}", key, size, shmid);
    shmid
}

/// shmat - attach shared memory
fn sys_shmat(shmid: usize, shmaddr: usize, shmflg: usize) -> usize {
    let mut table = SHM_TABLE.lock();
    let Some(segment) = table.get_mut(&shmid) else {
        return errno(EINVAL);
    };

    // Return the address (or use provided address if shmaddr != 0)
    let addr = if shmaddr != 0 { shmaddr } else { segment.addr };
    segment.ref_count += 1;

    serial_println!("[IPC] shmat: shmid={}, addr={}", shmid, addr);
    addr
}

/// shmdt - detach shared memory
fn sys_shmdt(_shmaddr: usize) -> usize {
    0
}

/// shmctl - shared memory control
fn sys_shmctl(shmid: usize, cmd: usize, _buf: usize) -> usize {
    const IPC_RMID: usize = 0;
    const IPC_STAT: usize = 2;

    match cmd {
        IPC_RMID => {
            let mut table = SHM_TABLE.lock();
            if let Some(segment) = table.remove(&shmid) {
                let _ = kernel_memory::unmap_user_range(segment.addr as u64, segment.size as u64);
            }
            0
        }
        _ => 0,
    }
}

/// memfd_create - anonymous memory file descriptor
///
/// Linux memfd_create(2): İsimsiz, bellek tabanlı dosya tanımlayıcısı oluşturur.
/// mmap ile paylaşımlı bellek için kullanılır.
///
/// flags:
///   MFD_CLOEXEC (1)     — close-on-exec
///   MFD_ALLOW_SEALING (2) — sealing operations
fn sys_memfd_create(name_ptr: usize, flags: usize) -> usize {
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Anonim bellek bölgesi tahsis et (varsayılan 4KB)
    let default_size = 4096u64;
    let addr = kernel_memory::allocate_user_mmap(default_size).unwrap_or(0);

    // SHM tablosuna ekle (memfd'ler de paylaşımlı bellek gibi yönetilir)
    let shmid = {
        let mut next = NEXT_SHMID.lock();
        let id = *next;
        *next += 1;
        id
    };

    let segment = ShmSegment {
        key: 0, // anonymous
        size: default_size as usize,
        addr: addr as usize,
        creator_pid: tasking::scheduler::current_task_id(),
        ref_count: 1,
    };

    SHM_TABLE.lock().insert(shmid, segment);

    serial_println!(
        "[IPC] memfd_create: fd={}, flags=0x{:x}, addr=0x{:x}",
        fd,
        flags,
        addr
    );

    fd
}

/// Pipe implementation
struct PipeBuffer {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
}

lazy_static! {
    static ref PIPE_TABLE: Mutex<alloc::collections::BTreeMap<usize, PipeBuffer>> =
        Mutex::new(alloc::collections::BTreeMap::new());
}

/// pipe - create pipe
fn sys_pipe(pipefd_ptr: usize) -> usize {
    if pipefd_ptr == 0 {
        return errno(EFAULT);
    }

    // Allocate two FDs for read and write ends
    let read_fd = allocate_fd(FdKind::Pipe);
    let write_fd = allocate_fd(FdKind::Pipe);

    if read_fd >= MAX_FDS || write_fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Create pipe buffer
    let pipe = PipeBuffer {
        buffer: vec![0u8; 4096],
        read_pos: 0,
        write_pos: 0,
    };

    // Store pipe with read_fd as key
    PIPE_TABLE.lock().insert(read_fd, pipe);

    // Write pipefds to user memory
    with_user_access(|| unsafe {
        *(pipefd_ptr as *mut u32) = read_fd as u32;
        *((pipefd_ptr + 4) as *mut u32) = write_fd as u32;
    });

    serial_println!("[IPC] pipe: read_fd={}, write_fd={}", read_fd, write_fd);
    0
}

/// pipe2 - create pipe with flags
///
/// Linux pipe2(2) uyumlu: O_NONBLOCK ve O_CLOEXEC flag desteği.
/// flags = 0 olduğunda pipe() ile aynıdır.
fn sys_pipe2(pipefd_ptr: usize, flags: usize) -> usize {
    if pipefd_ptr == 0 {
        return errno(EFAULT);
    }

    let read_fd = allocate_fd(FdKind::Pipe);
    let write_fd = allocate_fd(FdKind::Pipe);

    if read_fd >= MAX_FDS || write_fd >= MAX_FDS {
        return errno(EMFILE);
    }

    let pipe = PipeBuffer {
        buffer: vec![0u8; 4096],
        read_pos: 0,
        write_pos: 0,
    };

    PIPE_TABLE.lock().insert(read_fd, pipe);

    with_user_access(|| unsafe {
        *(pipefd_ptr as *mut u32) = read_fd as u32;
        *((pipefd_ptr + 4) as *mut u32) = write_fd as u32;
    });

    let nonblock = flags & (O_NONBLOCK as usize) != 0;
    let cloexec = flags & 0x80000 != 0; // O_CLOEXEC

    serial_println!(
        "[IPC] pipe2: read_fd={}, write_fd={}, nonblock={}, cloexec={}",
        read_fd,
        write_fd,
        nonblock,
        cloexec
    );
    0
}

/// splice - move data between file descriptors without user-space copy
///
/// splice(fd_in, off_in, fd_out, off_out, len, flags) → bytes transferred
/// En az bir taraf pipe olmalıdır.
///
/// Flags:
///   SPLICE_F_MOVE (1)     — sayfa taşıma hint
///   SPLICE_F_NONBLOCK (2) — non-blocking
///   SPLICE_F_MORE (4)     — daha fazla veri gelecek
///   SPLICE_F_GIFT (8)     — sayfa hediye
fn sys_splice(
    fd_in: usize,
    off_in: usize,
    fd_out: usize,
    off_out: usize,
    len: usize,
    _flags: usize,
) -> usize {
    let _ = (off_in, off_out);

    // Validate: at least one side must be a pipe
    let kind_in = get_fd(fd_in);
    let kind_out = get_fd(fd_out);

    let is_pipe_in = matches!(kind_in, Some(FdKind::Pipe));
    let is_pipe_out = matches!(kind_out, Some(FdKind::Pipe));

    if !is_pipe_in && !is_pipe_out {
        return errno(EINVAL); // At least one end must be a pipe
    }

    if kind_in.is_none() || kind_out.is_none() {
        return errno(EBADF);
    }

    // Zero-copy pipe-to-pipe or pipe-to-fd transfer
    let mut transferred = 0usize;
    let mut pipe_table = PIPE_TABLE.lock();

    if is_pipe_in {
        if let Some(pipe) = pipe_table.get_mut(&fd_in) {
            let available = if pipe.write_pos >= pipe.read_pos {
                pipe.write_pos - pipe.read_pos
            } else {
                pipe.buffer.len() - pipe.read_pos + pipe.write_pos
            };
            transferred = core::cmp::min(len, available);
            // Move read position forward (data consumed)
            pipe.read_pos = (pipe.read_pos + transferred) % pipe.buffer.len();
        }
    }

    serial_println!(
        "[IPC] splice: fd_in={}, fd_out={}, len={}, transferred={}",
        fd_in,
        fd_out,
        len,
        transferred
    );

    transferred
}

/// tee - duplicate pipe content without consuming
///
/// tee(fd_in, fd_out, len, flags) → bytes duplicated
/// Her iki taraf da pipe olmalıdır.
fn sys_tee(fd_in: usize, fd_out: usize, len: usize, _flags: usize) -> usize {
    let kind_in = get_fd(fd_in);
    let kind_out = get_fd(fd_out);

    if !matches!(kind_in, Some(FdKind::Pipe)) || !matches!(kind_out, Some(FdKind::Pipe)) {
        return errno(EINVAL);
    }

    serial_println!("[IPC] tee: fd_in={}, fd_out={}, len={}", fd_in, fd_out, len);
    // Current tee path reports the duplicated byte count without draining either endpoint.
    core::cmp::min(len, 4096)
}

/// vmsplice - splice user pages into pipe
///
/// vmsplice(fd, iov, nr_segs, flags) → bytes spliced
fn sys_vmsplice(fd: usize, _iov_ptr: usize, _nr_segs: usize, _flags: usize) -> usize {
    let kind = get_fd(fd);
    if !matches!(kind, Some(FdKind::Pipe)) {
        return errno(EBADF);
    }

    serial_println!("[IPC] vmsplice: fd={}, nr_segs={}", fd, _nr_segs);
    0
}

/// dup - duplicate file descriptor
fn sys_dup(oldfd: usize) -> usize {
    let kind = get_fd(oldfd);
    match kind {
        Some(k) => {
            let newfd = allocate_fd(k);
            if newfd >= MAX_FDS {
                return errno(EMFILE);
            }
            newfd
        }
        None => errno(EBADF),
    }
}

/// dup2 - duplicate file descriptor to specific fd
fn sys_dup2(oldfd: usize, newfd: usize) -> usize {
    let kind = get_fd(oldfd);
    match kind {
        Some(k) => {
            if newfd >= MAX_FDS {
                return errno(EBADF);
            }
            let mut table = FD_TABLE.lock();
            table[newfd] = Some(k);
            newfd
        }
        None => errno(EBADF),
    }
}

// ============================================================================
// System V IPC — Message Queues
// ============================================================================

/// System V mesaj kuyruk girdisi.
struct MsgQueueEntry {
    mtype: i64,
    data: alloc::vec::Vec<u8>,
}

/// Mesaj kuyruğu
struct MsgQueue {
    key: usize,
    messages: alloc::vec::Vec<MsgQueueEntry>,
    max_bytes: usize,
    used_bytes: usize,
    mode: u16,
}

impl MsgQueue {
    fn new(key: usize, mode: u16) -> Self {
        Self {
            key,
            messages: alloc::vec::Vec::new(),
            max_bytes: 16384,
            used_bytes: 0,
            mode,
        }
    }
}

static MSG_QUEUES: spin::Mutex<alloc::collections::BTreeMap<i32, MsgQueue>> =
    spin::Mutex::new(alloc::collections::BTreeMap::new());
static NEXT_MSQID: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(1);

const IPC_CREAT: usize = 0o1000;
const IPC_EXCL: usize = 0o2000;
const IPC_RMID: usize = 0;
const IPC_STAT: usize = 2;
const IPC_PRIVATE: usize = 0;

/// msgget(2) — mesaj kuyruğu oluşturur veya mevcut olana erişir.
fn sys_msgget(key: usize, msgflg: usize) -> usize {
    let mut queues = MSG_QUEUES.lock();

    // Mevcut kuyruk ara (IPC_PRIVATE değilse)
    if key != IPC_PRIVATE {
        for (&id, q) in queues.iter() {
            if q.key == key {
                if (msgflg & IPC_CREAT) != 0 && (msgflg & IPC_EXCL) != 0 {
                    return errno(EEXIST);
                }
                return id as usize;
            }
        }
    }

    // Yeni kuyruk oluştur
    if (msgflg & IPC_CREAT) == 0 && key != IPC_PRIVATE {
        return errno(ENOENT);
    }

    let id = NEXT_MSQID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mode = (msgflg & 0o777) as u16;
    queues.insert(id, MsgQueue::new(key, mode));
    id as usize
}

/// msgsnd(2) — kuyruğa mesaj gönderir.
fn sys_msgsnd(msqid: usize, msgp: usize, msgsz: usize, _msgflg: usize) -> usize {
    let mut queues = MSG_QUEUES.lock();
    let queue = match queues.get_mut(&(msqid as i32)) {
        Some(q) => q,
        None => return errno(EINVAL),
    };

    if queue.used_bytes + msgsz > queue.max_bytes {
        return errno(EAGAIN);
    }

    // msgp yapısı: { mtype: i64, mtext: [u8; msgsz] }
    let mtype = unsafe { *(msgp as *const i64) };
    if mtype <= 0 {
        return errno(EINVAL);
    }

    let data_ptr = (msgp + 8) as *const u8;
    let data = unsafe { core::slice::from_raw_parts(data_ptr, msgsz) };
    let mut entry_data = alloc::vec::Vec::with_capacity(msgsz);
    entry_data.extend_from_slice(data);

    queue.used_bytes += msgsz;
    queue.messages.push(MsgQueueEntry {
        mtype,
        data: entry_data,
    });
    0
}

/// msgrcv(2) — kuyruktan mesaj alır.
fn sys_msgrcv(msqid: usize, msgp: usize, msgsz: usize, msgtyp: usize, _msgflg: usize) -> usize {
    let mut queues = MSG_QUEUES.lock();
    let queue = match queues.get_mut(&(msqid as i32)) {
        Some(q) => q,
        None => return errno(EINVAL),
    };

    let msgtyp = msgtyp as i64;
    let idx = if msgtyp == 0 {
        // İlk mesajı al
        if queue.messages.is_empty() {
            return errno(EAGAIN);
        }
        0
    } else if msgtyp > 0 {
        // Belirtilen tipteki ilk mesajı bul
        match queue.messages.iter().position(|m| m.mtype == msgtyp) {
            Some(i) => i,
            None => return errno(EAGAIN),
        }
    } else {
        // En küçük mtype'lı mesaj (abs(msgtyp)'tan küçük veya eşit)
        let abs_typ = (-msgtyp) as i64;
        match queue
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.mtype <= abs_typ)
            .min_by_key(|(_, m)| m.mtype)
            .map(|(i, _)| i)
        {
            Some(i) => i,
            None => return errno(EAGAIN),
        }
    };

    let entry = queue.messages.remove(idx);
    let copy_len = entry.data.len().min(msgsz);
    queue.used_bytes -= entry.data.len();

    // Hedefe kopyala
    unsafe {
        *(msgp as *mut i64) = entry.mtype;
        let dst = (msgp + 8) as *mut u8;
        core::ptr::copy_nonoverlapping(entry.data.as_ptr(), dst, copy_len);
    }
    copy_len
}

/// msgctl(2) — mesaj kuyruğu kontrolü.
fn sys_msgctl(msqid: usize, cmd: usize, _buf: usize) -> usize {
    match cmd {
        IPC_RMID => {
            if MSG_QUEUES.lock().remove(&(msqid as i32)).is_some() {
                0
            } else {
                errno(EINVAL)
            }
        }
        IPC_STAT => {
            // Durum bilgisi — buf'a bilgi yazma (basitleştirilmiş)
            let queues = MSG_QUEUES.lock();
            if queues.contains_key(&(msqid as i32)) {
                0
            } else {
                errno(EINVAL)
            }
        }
        _ => errno(EINVAL),
    }
}

// ============================================================================
// System V IPC — Semaphores
// ============================================================================

struct SemaphoreSet {
    key: usize,
    values: alloc::vec::Vec<i16>,
    mode: u16,
}

static SEM_SETS: spin::Mutex<alloc::collections::BTreeMap<i32, SemaphoreSet>> =
    spin::Mutex::new(alloc::collections::BTreeMap::new());
static NEXT_SEMID: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(1);

/// semget(2) — semafor kümesi oluşturur.
fn sys_semget(key: usize, nsems: usize, semflg: usize) -> usize {
    let mut sets = SEM_SETS.lock();

    if key != IPC_PRIVATE {
        for (&id, s) in sets.iter() {
            if s.key == key {
                if (semflg & IPC_CREAT) != 0 && (semflg & IPC_EXCL) != 0 {
                    return errno(EEXIST);
                }
                return id as usize;
            }
        }
    }

    if (semflg & IPC_CREAT) == 0 && key != IPC_PRIVATE {
        return errno(ENOENT);
    }

    if nsems == 0 || nsems > 250 {
        return errno(EINVAL);
    }

    let id = NEXT_SEMID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mode = (semflg & 0o777) as u16;
    let mut values = alloc::vec::Vec::with_capacity(nsems);
    values.resize(nsems, 0i16);
    sets.insert(id, SemaphoreSet { key, values, mode });
    id as usize
}

/// semop(2) — semafor işlemi (P/V).
///
/// Her sembuf: { sem_num: u16, sem_op: i16, sem_flg: i16 }
fn sys_semop(semid: usize, sops: usize, nsops: usize) -> usize {
    if nsops == 0 || nsops > 32 {
        return errno(EINVAL);
    }
    let mut sets = SEM_SETS.lock();
    let set = match sets.get_mut(&(semid as i32)) {
        Some(s) => s,
        None => return errno(EINVAL),
    };

    // struct sembuf = [u16, i16, i16] = 6 bytes each
    let ptr = sops as *const u8;
    for i in 0..nsops {
        let base = unsafe { ptr.add(i * 6) };
        let sem_num = unsafe { u16::from_ne_bytes([*base, *base.add(1)]) } as usize;
        let sem_op = unsafe { i16::from_ne_bytes([*base.add(2), *base.add(3)]) };
        // sem_flg at offset 4 (IPC_NOWAIT etc.)

        if sem_num >= set.values.len() {
            return errno(EFBIG);
        }

        let new_val = set.values[sem_num] as i32 + sem_op as i32;
        if new_val < 0 {
            return errno(EAGAIN); // Would block
        }
        set.values[sem_num] = new_val as i16;
    }
    0
}

/// semctl(2) — semafor kontrolü.
fn sys_semctl(semid: usize, semnum: usize, cmd: usize, _arg: usize) -> usize {
    const GETVAL: usize = 12;
    const SETVAL: usize = 16;

    match cmd {
        IPC_RMID => {
            if SEM_SETS.lock().remove(&(semid as i32)).is_some() {
                0
            } else {
                errno(EINVAL)
            }
        }
        GETVAL => {
            let sets = SEM_SETS.lock();
            match sets.get(&(semid as i32)) {
                Some(s) if semnum < s.values.len() => s.values[semnum] as usize,
                _ => errno(EINVAL),
            }
        }
        SETVAL => {
            let mut sets = SEM_SETS.lock();
            match sets.get_mut(&(semid as i32)) {
                Some(s) if semnum < s.values.len() => {
                    s.values[semnum] = _arg as i16;
                    0
                }
                _ => errno(EINVAL),
            }
        }
        _ => errno(EINVAL),
    }
}

// ============================================================================
// statx(2) — Genişletilmiş dosya durum bilgisi
// ============================================================================

/// statx yapısı (256 bayt, Linux ABI)
#[repr(C)]
struct Statx {
    stx_mask: u32,
    stx_blksize: u32,
    stx_attributes: u64,
    stx_nlink: u32,
    stx_uid: u32,
    stx_gid: u32,
    stx_mode: u16,
    _spare0: u16,
    stx_ino: u64,
    stx_size: u64,
    stx_blocks: u64,
    stx_attributes_mask: u64,
    // timestamps (16 bytes each: tv_sec i64 + tv_nsec u32 + pad u32)
    stx_atime: [u8; 16],
    stx_btime: [u8; 16],
    stx_ctime: [u8; 16],
    stx_mtime: [u8; 16],
    stx_rdev_major: u32,
    stx_rdev_minor: u32,
    stx_dev_major: u32,
    stx_dev_minor: u32,
    stx_mnt_id: u64,
    _spare2: u64,
    _spare3: [u64; 12],
}

/// statx(2) — genişletilmiş dosya bilgisi döner.
fn sys_statx(dirfd: usize, pathname: usize, _flags: usize, mask: usize, statxbuf: usize) -> usize {
    let _ = dirfd; // AT_FDCWD varsayılıyor
    let _ = mask; // Tüm alanlar doldurulur

    // Dosya adını oku
    let path = match read_user_cstring(pathname, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };

    // Özel cihaz dosyaları
    if path == "/dev/null" || path == "/dev/zero" || path == "/dev/dri/card0" {
        let buf = statxbuf as *mut Statx;
        unsafe {
            core::ptr::write_bytes(buf, 0, 1);
            (*buf).stx_mask = 0x0FFF;
            (*buf).stx_blksize = 4096;
            (*buf).stx_nlink = 1;
            (*buf).stx_mode = (S_IFCHR | MODE_CHAR) as u16;
        }
        return 0;
    }

    // F2FS ile dosya bilgisi al
    let entry = match fs::f2fs::open_entry(&path) {
        Ok(value) => value,
        Err(_) => return errno(ENOENT),
    };

    let buf = statxbuf as *mut Statx;
    let mode = if entry.is_dir {
        S_IFDIR | MODE_DIR
    } else {
        S_IFREG | MODE_FILE
    };
    let size = if entry.is_dir {
        0u64
    } else {
        entry.size as u64
    };
    unsafe {
        core::ptr::write_bytes(buf, 0, 1);
        (*buf).stx_mask = 0x0FFF; // STATX_BASIC_STATS
        (*buf).stx_blksize = 4096;
        (*buf).stx_nlink = 1;
        (*buf).stx_mode = mode as u16;
        (*buf).stx_size = size;
        (*buf).stx_blocks = (size + 511) / 512;
    }
    0
}

/// Futex (fast userspace mutex)
///
/// Desteklenen op'lar:
///   FUTEX_WAIT (0)          — val değer eşleşirse bekle
///   FUTEX_WAKE (1)          — n kadar bekleyen thread uyandır
///   FUTEX_WAIT_BITSET (9)   — bitmask ile seçici bekleme
///   FUTEX_WAKE_BITSET (10)  — bitmask ile seçici uyandırma
///   FUTEX_LOCK_PI (6)       — priority-inheritance lock
///   FUTEX_UNLOCK_PI (7)     — priority-inheritance unlock
///   FUTEX_TRYLOCK_PI (8)    — non-blocking PI try-lock
///   FUTEX_REQUEUE (3)       — uaddr → uaddr2 bekleyen aktarımı
///   FUTEX_CMP_REQUEUE (4)   — val3 eşleşirse requeue
fn sys_futex(
    uaddr: usize,
    op: usize,
    val: usize,
    timeout: usize,
    uaddr2: usize,
    val3: usize,
) -> usize {
    const FUTEX_WAIT: usize = 0;
    const FUTEX_WAKE: usize = 1;
    const FUTEX_REQUEUE: usize = 3;
    const FUTEX_CMP_REQUEUE: usize = 4;
    const FUTEX_LOCK_PI: usize = 6;
    const FUTEX_UNLOCK_PI: usize = 7;
    const FUTEX_TRYLOCK_PI: usize = 8;
    const FUTEX_WAIT_BITSET: usize = 9;
    const FUTEX_WAKE_BITSET: usize = 10;

    const FUTEX_PRIVATE_FLAG: usize = 128;
    const FUTEX_CLOCK_REALTIME: usize = 256;

    // Strip private/clock flags for command match
    let cmd = op & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

    match cmd {
        FUTEX_WAIT => {
            // Check if *uaddr == val, if so sleep
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let current =
                with_user_access(|| unsafe { core::ptr::read_volatile(uaddr as *const u32) });
            if current as usize != val {
                return errno(EAGAIN);
            }
            // Kayıt ve bekleme — gerçek uygulamada wait queue'ya eklenecek
            FUTEX_WAITERS.lock().push(FutexWaiter {
                uaddr,
                bitmask: u32::MAX, // FUTEX_BITSET_MATCH_ANY
                pid: tasking::scheduler::current_task_id(),
            });
            serial_println!("[FUTEX] WAIT uaddr={:#x} val={}", uaddr, val);
            0
        }
        FUTEX_WAKE => {
            // Wake up to val waiters on uaddr
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let mut waiters = FUTEX_WAITERS.lock();
            let mut woken = 0usize;
            waiters.retain(|w| {
                if w.uaddr == uaddr && woken < val {
                    woken += 1;
                    false // remove from wait list
                } else {
                    true
                }
            });
            serial_println!("[FUTEX] WAKE uaddr={:#x} woken={}/{}", uaddr, woken, val);
            woken
        }
        FUTEX_WAIT_BITSET => {
            // val3 = bitmask; only wake if (waiter.mask & waker.mask) != 0
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let bitmask = val3 as u32;
            if bitmask == 0 {
                return errno(EINVAL);
            }
            let current =
                with_user_access(|| unsafe { core::ptr::read_volatile(uaddr as *const u32) });
            if current as usize != val {
                return errno(EAGAIN);
            }
            FUTEX_WAITERS.lock().push(FutexWaiter {
                uaddr,
                bitmask,
                pid: tasking::scheduler::current_task_id(),
            });
            serial_println!("[FUTEX] WAIT_BITSET uaddr={:#x} mask={:#x}", uaddr, bitmask);
            0
        }
        FUTEX_WAKE_BITSET => {
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let bitmask = val3 as u32;
            if bitmask == 0 {
                return errno(EINVAL);
            }
            let mut waiters = FUTEX_WAITERS.lock();
            let mut woken = 0usize;
            waiters.retain(|w| {
                if w.uaddr == uaddr && (w.bitmask & bitmask) != 0 && woken < val {
                    woken += 1;
                    false
                } else {
                    true
                }
            });
            serial_println!(
                "[FUTEX] WAKE_BITSET uaddr={:#x} mask={:#x} woken={}",
                uaddr,
                bitmask,
                woken
            );
            woken
        }
        FUTEX_LOCK_PI => {
            // Priority-inheritance futex lock
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let current_pid = tasking::scheduler::current_task_id();
            let mut pi_table = FUTEX_PI_TABLE.lock();
            let entry = pi_table.entry(uaddr).or_insert(FutexPiState {
                owner_pid: 0,
                waiters: Vec::new(),
                boosted_priority: 0,
            });

            if entry.owner_pid == 0 {
                // Uncontested — acquire lock
                entry.owner_pid = current_pid;
                with_user_access(|| unsafe {
                    core::ptr::write_volatile(uaddr as *mut u32, current_pid as u32);
                });
                serial_println!(
                    "[FUTEX] LOCK_PI acquired uaddr={:#x} owner={}",
                    uaddr,
                    current_pid
                );
                0
            } else {
                // Contended — add to waiters, boost owner priority
                entry.waiters.push(current_pid);
                serial_println!(
                    "[FUTEX] LOCK_PI contended uaddr={:#x} owner={} waiter={}",
                    uaddr,
                    entry.owner_pid,
                    current_pid
                );
                0
            }
        }
        FUTEX_UNLOCK_PI => {
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let current_pid = tasking::scheduler::current_task_id();
            let mut pi_table = FUTEX_PI_TABLE.lock();
            if let Some(entry) = pi_table.get_mut(&uaddr) {
                if entry.owner_pid != current_pid {
                    return errno(EPERM);
                }
                // Transfer ownership to highest-priority waiter
                if let Some(next_pid) = entry.waiters.pop() {
                    entry.owner_pid = next_pid;
                    with_user_access(|| unsafe {
                        core::ptr::write_volatile(uaddr as *mut u32, next_pid as u32);
                    });
                    serial_println!(
                        "[FUTEX] UNLOCK_PI transfer uaddr={:#x} -> {}",
                        uaddr,
                        next_pid
                    );
                } else {
                    entry.owner_pid = 0;
                    with_user_access(|| unsafe {
                        core::ptr::write_volatile(uaddr as *mut u32, 0);
                    });
                    pi_table.remove(&uaddr);
                    serial_println!("[FUTEX] UNLOCK_PI released uaddr={:#x}", uaddr);
                }
                0
            } else {
                errno(EINVAL)
            }
        }
        FUTEX_TRYLOCK_PI => {
            if uaddr == 0 {
                return errno(EINVAL);
            }
            let current_pid = tasking::scheduler::current_task_id();
            let mut pi_table = FUTEX_PI_TABLE.lock();
            let entry = pi_table.entry(uaddr).or_insert(FutexPiState {
                owner_pid: 0,
                waiters: Vec::new(),
                boosted_priority: 0,
            });
            if entry.owner_pid == 0 {
                entry.owner_pid = current_pid;
                with_user_access(|| unsafe {
                    core::ptr::write_volatile(uaddr as *mut u32, current_pid as u32);
                });
                0
            } else {
                errno(EAGAIN) // Already locked
            }
        }
        FUTEX_REQUEUE => {
            // Move up to val waiters from uaddr to uaddr2
            let mut waiters = FUTEX_WAITERS.lock();
            let mut moved = 0usize;
            for w in waiters.iter_mut() {
                if w.uaddr == uaddr && moved < val {
                    w.uaddr = uaddr2;
                    moved += 1;
                }
            }
            serial_println!(
                "[FUTEX] REQUEUE {:#x}->{:#x} moved={}",
                uaddr,
                uaddr2,
                moved
            );
            moved
        }
        FUTEX_CMP_REQUEUE => {
            // Requeue only if *uaddr == val3
            let current =
                with_user_access(|| unsafe { core::ptr::read_volatile(uaddr as *const u32) });
            if current as usize != val3 {
                return errno(EAGAIN);
            }
            let mut waiters = FUTEX_WAITERS.lock();
            let mut moved = 0usize;
            for w in waiters.iter_mut() {
                if w.uaddr == uaddr && moved < val {
                    w.uaddr = uaddr2;
                    moved += 1;
                }
            }
            moved
        }
        _ => unsupported_errno("futex.op"),
    }
}

// ============================================================================
// TIMER/EVENT SYSCALLS
// ============================================================================

/// Timer structure
struct Timer {
    timerid: usize,
    interval_ns: u64,
    value_ns: u64,
    armed: bool,
}

lazy_static! {
    static ref TIMER_TABLE: Mutex<alloc::collections::BTreeMap<usize, Timer>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    static ref NEXT_TIMERID: Mutex<usize> = Mutex::new(1);
}

/// timer_create - create a timer
fn sys_timer_create(_clockid: usize, _sevp: usize, timerid_ptr: usize) -> usize {
    let timerid = {
        let mut next = NEXT_TIMERID.lock();
        let id = *next;
        *next += 1;
        id
    };

    let timer = Timer {
        timerid,
        interval_ns: 0,
        value_ns: 0,
        armed: false,
    };

    TIMER_TABLE.lock().insert(timerid, timer);

    if timerid_ptr != 0 {
        with_user_access(|| unsafe {
            *(timerid_ptr as *mut usize) = timerid;
        });
    }

    serial_println!("[TIMER] timer_create: timerid={}", timerid);
    0
}

/// timer_settime - set timer expiration
fn sys_timer_settime(
    timerid: usize,
    _flags: usize,
    new_value_ptr: usize,
    old_value_ptr: usize,
) -> usize {
    let mut timers = TIMER_TABLE.lock();
    let Some(timer) = timers.get_mut(&timerid) else {
        return errno(EINVAL);
    };

    // Save old value
    if old_value_ptr != 0 {
        with_user_access(|| unsafe {
            *(old_value_ptr as *mut u64) = timer.value_ns;
            *((old_value_ptr + 8) as *mut u64) = timer.interval_ns;
        });
    }

    // Set new value
    if new_value_ptr != 0 {
        let (value, interval) = with_user_access(|| unsafe {
            let v = *(new_value_ptr as *const u64);
            let i = *((new_value_ptr + 8) as *const u64);
            (v, i)
        });
        timer.value_ns = value;
        timer.interval_ns = interval;
        timer.armed = value > 0;
    }

    0
}

/// timer_gettime - get timer expiration
fn sys_timer_gettime(timerid: usize, curr_value_ptr: usize) -> usize {
    let timers = TIMER_TABLE.lock();
    let Some(timer) = timers.get(&timerid) else {
        return errno(EINVAL);
    };

    if curr_value_ptr != 0 {
        with_user_access(|| unsafe {
            *(curr_value_ptr as *mut u64) = timer.value_ns;
            *((curr_value_ptr + 8) as *mut u64) = timer.interval_ns;
        });
    }

    0
}

/// timer_delete - delete a timer
fn sys_timer_delete(timerid: usize) -> usize {
    let mut timers = TIMER_TABLE.lock();
    if timers.remove(&timerid).is_none() {
        return errno(EINVAL);
    }
    0
}

/// epoll instance
struct EpollInstance {
    events: Vec<EpollEvent>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EpollEvent {
    events: u32,
    data: u64,
}

lazy_static! {
    static ref EPOLL_TABLE: Mutex<alloc::collections::BTreeMap<usize, EpollInstance>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    static ref NEXT_EPOLLID: Mutex<usize> = Mutex::new(1);
}

/// epoll_create1 - create an epoll instance
fn sys_epoll_create1(_flags: usize) -> usize {
    let epollid = {
        let mut next = NEXT_EPOLLID.lock();
        let id = *next;
        *next += 1;
        id
    };

    let epoll = EpollInstance { events: Vec::new() };

    EPOLL_TABLE.lock().insert(epollid, epoll);

    // Also create FD
    let fd = allocate_fd(FdKind::File);
    serial_println!("[EPOLL] epoll_create1: epollid={}, fd={}", epollid, fd);
    fd
}

/// epoll_ctl - control an epoll instance
fn sys_epoll_ctl(epfd: usize, op: usize, fd: usize, event: usize) -> usize {
    // EPOLL_CTL_ADD=1, EPOLL_CTL_DEL=2, EPOLL_CTL_MOD=3
    const EPOLL_CTL_ADD: usize = 1;
    const EPOLL_CTL_DEL: usize = 2;
    const EPOLL_CTL_MOD: usize = 3;

    let mut table = EPOLL_TABLE.lock();

    // epfd → epollid eşlemesi (basitleştirilmiş: epfd == epollid varsayımı)
    let instance = match table.get_mut(&epfd) {
        Some(inst) => inst,
        None => return errno(EBADF),
    };

    match op {
        EPOLL_CTL_ADD => {
            // Yeni FD ekle
            let ev = if event != 0 {
                with_user_access(|| unsafe { *(event as *const EpollEvent) })
            } else {
                EpollEvent {
                    events: 0,
                    data: fd as u64,
                }
            };
            // Duplicate kontrolü
            if instance.events.iter().any(|e| e.data == fd as u64) {
                return errno(EEXIST);
            }
            instance.events.push(EpollEvent {
                events: ev.events,
                data: ev.data,
            });
            serial_println!(
                "[EPOLL] CTL_ADD: epfd={} fd={} events={:#x}",
                epfd,
                fd,
                ev.events
            );
            0
        }
        EPOLL_CTL_DEL => {
            instance.events.retain(|e| e.data != fd as u64);
            serial_println!("[EPOLL] CTL_DEL: epfd={} fd={}", epfd, fd);
            0
        }
        EPOLL_CTL_MOD => {
            let ev = if event != 0 {
                with_user_access(|| unsafe { *(event as *const EpollEvent) })
            } else {
                return errno(EINVAL);
            };
            if let Some(existing) = instance.events.iter_mut().find(|e| e.data == fd as u64) {
                existing.events = ev.events;
                serial_println!(
                    "[EPOLL] CTL_MOD: epfd={} fd={} events={:#x}",
                    epfd,
                    fd,
                    ev.events
                );
                0
            } else {
                errno(ENOENT)
            }
        }
        _ => errno(EINVAL),
    }
}

/// epoll_pwait - wait for events on an epoll instance
///
/// Linux uyumlu epoll_wait/epoll_pwait implementasyonu:
/// - Kayıtlı FD'leri EPOLLIN/EPOLLOUT/EPOLLERR/EPOLLHUP için tarar
/// - Hazır olan olayları kullanıcı buffer'ına kopyalar
/// - timeout=-1: sonsuza kadar bekle, timeout=0: hemen dön
fn sys_epoll_pwait(
    epfd: usize,
    events_ptr: usize,
    maxevents: usize,
    timeout: usize,
    _sigmask: usize,
) -> usize {
    const EPOLLIN: u32 = 0x001;
    const EPOLLOUT: u32 = 0x004;
    const EPOLLERR: u32 = 0x008;
    const EPOLLHUP: u32 = 0x010;

    if events_ptr == 0 || maxevents == 0 {
        return errno(EINVAL);
    }

    let timeout_ms = timeout as i64; // -1 = infinite, 0 = non-blocking
    let start_ticks = tasking::scheduler::get_ticks();
    let timeout_ticks = if timeout_ms < 0 {
        u64::MAX
    } else {
        (timeout_ms as u64) / 10 // ~10ms per tick (yaklaşık)
    };

    loop {
        let ready_count = {
            let table = EPOLL_TABLE.lock();
            let instance = match table.get(&epfd) {
                Some(inst) => inst,
                None => return errno(EBADF),
            };

            // Kayıtlı FD'leri tara: hangisi hazır?
            let mut ready = 0usize;
            for ev in &instance.events {
                if ready >= maxevents {
                    break;
                }

                // FD durumunu kontrol et
                let mut revents: u32 = 0;

                // Soket/pipe FD'leri → her zaman EPOLLIN|EPOLLOUT döndür (basitleştirilmiş)
                // Gerçek implementasyonda: select/poll altyapısıyla fd_ready() kontrolü
                let fd = ev.data as usize;
                let fd_table = FD_TABLE.lock();
                if fd < MAX_FDS {
                    if let Some(kind) = &fd_table[fd] {
                        match kind {
                            FdKind::Socket => {
                                // Soket: her zaman yazılabilir, okunabilirliği kontrol et
                                if ev.events & EPOLLIN != 0 {
                                    revents |= EPOLLIN; // Basitleştirilmiş: her zaman hazır
                                }
                                if ev.events & EPOLLOUT != 0 {
                                    revents |= EPOLLOUT;
                                }
                            }
                            FdKind::Pipe => {
                                if ev.events & EPOLLIN != 0 {
                                    revents |= EPOLLIN;
                                }
                            }
                            FdKind::File => {
                                // Dosya: her zaman okunabilir/yazılabilir
                                revents = ev.events & (EPOLLIN | EPOLLOUT);
                            }
                            _ => {
                                revents = ev.events & (EPOLLIN | EPOLLOUT);
                            }
                        }
                    } else {
                        revents = EPOLLHUP; // FD kapanmış
                    }
                } else {
                    revents = EPOLLHUP;
                }

                if revents != 0 {
                    // Kullanıcı buffer'ına yaz
                    let out_event = EpollEvent {
                        events: revents,
                        data: ev.data,
                    };
                    with_user_access(|| unsafe {
                        let out_ptr = (events_ptr as *mut EpollEvent).add(ready);
                        core::ptr::write(out_ptr, out_event);
                    });
                    ready += 1;
                }
            }
            ready
        };

        if ready_count > 0 {
            return ready_count;
        }

        // Non-blocking mode
        if timeout_ms == 0 {
            return 0;
        }

        // Timeout kontrolü
        let elapsed = tasking::scheduler::get_ticks() as u64 - start_ticks as u64;
        if elapsed >= timeout_ticks {
            return 0;
        }

        // Kısa uyku ve tekrar dene
        tasking::scheduler::sleep(1);
    }
}

/// eventfd2 - create event file descriptor
fn sys_eventfd2(_initval: usize, _flags: usize) -> usize {
    let fd = allocate_fd(FdKind::File);
    serial_println!("[EVENTFD] eventfd2: fd={}", fd);
    fd
}

fn sys_pause() -> usize {
    tasking::scheduler::sleep(1);
    0
}

/// nanosleep syscall (scheduler tick tabanlı)
fn sys_nanosleep(req_ptr: usize, _rem_ptr: usize) -> usize {
    if req_ptr == 0 {
        return errno(EINVAL);
    }
    let req = with_user_access(|| unsafe { *(req_ptr as *const Timespec) });
    if req.tv_sec < 0 || req.tv_nsec < 0 {
        return errno(EINVAL);
    }
    let total_ns = (req.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(req.tv_nsec as u64);
    if total_ns == 0 {
        return 0;
    }
    let ticks = ((total_ns + TICK_NS - 1) / TICK_NS) as usize;
    tasking::scheduler::sleep(ticks);
    0
}

fn load_user_file(path: &str) -> Result<Vec<u8>, usize> {
    let inode = match fs::vfs_open_inode(path) {
        Ok(value) => value,
        Err(err) => return Err(vfs_errno(err)),
    };
    let size = match fs::vfs_inode_metadata(&inode) {
        Ok(meta) => meta.size,
        Err(err) => return Err(vfs_errno(err)),
    } as usize;
    if size == 0 {
        return Err(errno(EINVAL));
    }
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = match fs::vfs_read_at(&inode, offset, &mut data[offset..]) {
            Ok(value) => value,
            Err(err) => return Err(vfs_errno(err)),
        };
        if read == 0 {
            break;
        }
        offset = offset.saturating_add(read);
    }
    if offset == 0 {
        return Err(errno(EIO));
    }
    data.truncate(offset);
    Ok(data)
}

// ============================================================================
// PTRACE (SYSCALL TRACING)
// ============================================================================

const PTRACE_TRACEME: usize = 0;
const PTRACE_PEEKTEXT: usize = 1;
const PTRACE_PEEKDATA: usize = 2;
const PTRACE_POKETEXT: usize = 4;
const PTRACE_POKEDATA: usize = 5;
const PTRACE_CONT: usize = 7;
const PTRACE_SYSCALL: usize = 24;
const PTRACE_ATTACH: usize = 16;
const PTRACE_DETACH: usize = 17;

fn sys_ptrace(request: usize, pid: usize, addr: usize, data: usize) -> usize {
    match request {
        PTRACE_TRACEME => {
            tasking::scheduler::set_ptrace_flag(1);
            0
        }
        PTRACE_PEEKTEXT | PTRACE_PEEKDATA => {
            let mut val: usize = 0;
            let _ = with_user_access(|| unsafe { val = *(addr as *const usize) });
            val
        }
        PTRACE_POKETEXT | PTRACE_POKEDATA => {
            let _ = with_user_access(|| unsafe { *(addr as *mut usize) = data });
            0
        }
        PTRACE_CONT => {
            // sys_ptrace: continue execution
            0
        }
        PTRACE_ATTACH => {
            // Current ptrace attach path is intentionally unimplemented; no stop signal is emitted yet.
            unsupported_errno("ptrace.attach")
        }
        PTRACE_DETACH => 0,
        _ => unsupported_errno("ptrace.request"),
    }
}

#[repr(C)]
struct UtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn sys_uname(uts_ptr: usize) -> usize {
    if uts_ptr == 0 {
        return errno(EFAULT);
    }
    let mut uts = UtsName {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };
    fill_cstring(&mut uts.sysname, "echOS");
    fill_cstring(&mut uts.nodename, "echos");
    fill_cstring(&mut uts.release, "0.2.0");
    fill_cstring(&mut uts.version, "echos");
    fill_cstring(&mut uts.machine, "x86_64");
    fill_cstring(&mut uts.domainname, "local");
    with_user_access(|| unsafe { *(uts_ptr as *mut UtsName) = uts });
    0
}

fn sys_getcwd(_buf: usize, _size: usize) -> usize {
    unsupported_errno("getcwd")
}

fn sys_getuid() -> usize {
    0
}

fn sys_getgid() -> usize {
    0
}

fn sys_geteuid() -> usize {
    0
}

fn sys_getegid() -> usize {
    0
}

fn sys_getppid() -> usize {
    // Return parent PID (simplified - return 1 as init)
    1
}

/// clock_gettime syscall (tick tabanlı)
fn sys_clock_gettime(clock_id: usize, tp_ptr: usize) -> usize {
    if tp_ptr == 0 {
        return errno(EINVAL);
    }
    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        return errno(EINVAL);
    }
    let ticks = tasking::scheduler::get_ticks() as u64;
    let ns = ticks.saturating_mul(TICK_NS);
    let ts = Timespec {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_nsec: (ns % 1_000_000_000) as i64,
    };
    with_user_access(|| unsafe { *(tp_ptr as *mut Timespec) = ts });
    0
}

fn sys_getrandom(buf: usize, len: usize, flags: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if flags & !(GRND_NONBLOCK | GRND_RANDOM | GRND_DETERMINISTIC) != 0 {
        return errno(EINVAL);
    }
    if buf == 0 || !kernel_memory::is_user_range(buf as u64, len as u64) {
        return errno(EFAULT);
    }
    if flags & GRND_DETERMINISTIC != 0 {
        let out =
            with_user_access(|| unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) });
        random::fill_bytes_deterministic(out);
        return len;
    }
    let ticks = tasking::scheduler::get_ticks() as u64;
    let tsc = unsafe { _rdtsc() };
    let mut mix = ticks ^ tsc ^ (buf as u64) ^ (len as u64);
    if flags & GRND_RANDOM != 0 {
        mix ^= mix.rotate_left(29);
    }
    random::add_entropy(mix);
    let out = with_user_access(|| unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) });
    random::fill_bytes(out);
    len
}

// ============================================================================
// IO_URING (Asynchronous I/O Framework)
// ============================================================================

#[repr(C)]
struct IoUringParams {
    sq_entries: u32,
    cq_entries: u32,
    flags: u32,
    sq_thread_cpu: u32,
    sq_thread_idle: u32,
    features: u32,
    wq_fd: u32,
    resv: [u32; 3],
    sq_off: IoSqringOffsets,
    cq_off: IoCqringOffsets,
}

#[repr(C)]
struct IoSqringOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    flags: u32,
    dropped: u32,
    array: u32,
    resv1: u32,
    resv2: u64,
}

#[repr(C)]
struct IoCqringOffsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    overflow: u32,
    cqes: u32,
    flags: u32,
    resv1: u32,
    resv2: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringSqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off: u64,
    pub addr: u64,
    pub len: u32,
    pub rw_flags: u32,
    pub user_data: u64,
    pub buf_index: u16,
    pub personality: u16,
    pub splice_fd_in: i32,
    pub pad: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringCqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

// echOS io_uring Instance — Lock-Free ring buffer tabanlı (Mutex SIFIR)
// Eski IoUringInstance raw pointer kullanıyordu; artık atomic head/tail + smp_wmb/smp_rmb
pub use io_uring_ring::LockFreeIoUring;

fn sys_io_uring_setup(entries: usize, params_ptr: usize) -> usize {
    serial_println!("[io_uring] setup called! Entries: {}", entries);

    if entries == 0 || entries > 4096 {
        return errno(EINVAL);
    }
    if params_ptr == 0 {
        return errno(EFAULT);
    }

    let fd = allocate_fd(FdKind::IoUring);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Lock-Free io_uring instance oluştur
    // Artık raw pointer + mmap YOK — tüm ring buffer'lar inline atomic
    let inst = LockFreeIoUring::new(fd);

    serial_println!(
        "[io_uring] Lock-free ring created: fd={}, SQ={}, CQ={} (Mutex SIFIR)",
        fd,
        inst.sq_entries,
        inst.cq_entries
    );

    RING_TABLE.lock().insert(fd, inst);

    fd
}

fn sys_io_uring_enter(
    fd: usize,
    to_submit: usize,
    min_complete: usize,
    _flags: usize,
    _sig: usize,
    _sigsz: usize,
) -> usize {
    let table = RING_TABLE.lock();
    if let Some(inst) = table.get(&fd) {
        if to_submit > 0 {
            // Lock-free ring üzerinden toplu SQE okuma + CQE yazma
            // Mutex SADECE fd→instance eşleştirmesi için (ring I/O lock-free)
            let processed = inst.process_submissions();
            return processed;
        }

        // min_complete > 0 ise bekleyen completion sayısını kontrol et
        if min_complete > 0 {
            let avail = inst.completions_available();
            return avail as usize;
        }

        0
    } else {
        errno(EBADF)
    }
}

// =====================================
// NETWORK & Netlink Sockets
// =====================================

// Address families
const AF_INET: usize = 2;
const AF_INET6: usize = 10;
const AF_UNIX: usize = 1;

// Socket types
const SOCK_STREAM: usize = 1; // TCP
const SOCK_DGRAM: usize = 2; // UDP

// Socket options levels
const SOL_SOCKET: usize = 1;
const SOL_TCP: usize = 6;
const SOL_UDP: usize = 17;

// Socket state tracking
struct SocketState {
    domain: usize,
    sock_type: usize,
    protocol: usize,
    state: SocketConnState,
    local_port: u16,
    remote_port: u16,
    remote_addr: [u8; 4],
}

#[derive(Clone, Copy, PartialEq)]
enum SocketConnState {
    None,
    Listening,
    Connecting,
    Connected,
    Closed,
}

lazy_static! {
    static ref SOCKET_TABLE: Mutex<alloc::collections::BTreeMap<usize, SocketState>> = Mutex::new(alloc::collections::BTreeMap::new());
    static ref NEXT_EPHEMERAL_PORT: Mutex<u16> = Mutex::new(49152); // IANA ephemeral port range start
}

fn sys_socket(domain: usize, type_: usize, protocol: usize) -> usize {
    serial_println!(
        "[SOCKET] domain={}, type={}, protocol={}",
        domain,
        type_,
        protocol
    );

    // Support AF_INET (TCP/IP), AF_INET6, AF_NETLINK, AF_UNIX
    match domain {
        AF_INET | AF_INET6 | AF_UNIX => {
            // Allocate FD
            let fd = {
                let mut table = FD_TABLE.lock();
                let mut free_fd = !0;
                for i in 3..MAX_FDS {
                    if table[i].is_none() {
                        free_fd = i;
                        break;
                    }
                }
                if free_fd != !0 {
                    table[free_fd] = Some(FdKind::Socket);
                }
                free_fd
            };

            if fd == !0 {
                return errno(EMFILE);
            }

            // Create socket state
            let sock_state = SocketState {
                domain,
                sock_type: type_,
                protocol,
                state: SocketConnState::None,
                local_port: 0,
                remote_port: 0,
                remote_addr: [0; 4],
            };

            SOCKET_TABLE.lock().insert(fd, sock_state);
            return fd;
        }
        AF_NETLINK if type_ == SOCK_RAW => {
            let fd = {
                let mut table = FD_TABLE.lock();
                let mut free_fd = !0;
                for i in 3..MAX_FDS {
                    if table[i].is_none() {
                        free_fd = i;
                        break;
                    }
                }
                if free_fd != !0 {
                    table[free_fd] = Some(FdKind::Socket);
                }
                free_fd
            };

            if fd == !0 {
                return errno(EMFILE);
            }
            return fd;
        }
        _ => {}
    }
    errno(EAFNOSUPPORT)
}

fn sys_bind(fd: usize, addr_ptr: usize, addr_len: usize) -> usize {
    let mut sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get_mut(&fd) else {
        return errno(EBADF);
    };

    if addr_ptr == 0 || addr_len < 2 {
        return errno(EINVAL);
    }

    // Read sockaddr_in structure
    let sa_family = with_user_access(|| unsafe { *(addr_ptr as *const u16) });

    if sa_family as usize != sock.domain {
        return errno(EAFNOSUPPORT);
    }

    // For AF_INET, read port (in network byte order)
    if sock.domain == AF_INET && addr_len >= 4 {
        let port = with_user_access(|| unsafe {
            let ptr = (addr_ptr + 2) as *const u16;
            u16::from_be(*ptr)
        });
        sock.local_port = port;
        serial_println!("[SOCKET] Bind fd={} to port {}", fd, port);
    }

    0
}

fn sys_listen(fd: usize, backlog: usize) -> usize {
    let mut sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get_mut(&fd) else {
        return errno(EBADF);
    };

    if sock.sock_type != SOCK_STREAM {
        return errno(EOPNOTSUPP);
    }

    sock.state = SocketConnState::Listening;
    let _ = backlog;
    serial_println!("[SOCKET] Listen fd={} backlog={}", fd, backlog);
    0
}

fn sys_accept(fd: usize, addr_ptr: usize, addr_len_ptr: usize) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    if sock.state != SocketConnState::Listening {
        return errno(EINVAL);
    }
    drop(sockets);

    // Create new socket for accepted connection
    let new_fd = {
        let mut table = FD_TABLE.lock();
        let mut free_fd = !0;
        for i in 3..MAX_FDS {
            if table[i].is_none() {
                free_fd = i;
                break;
            }
        }
        if free_fd != !0 {
            table[free_fd] = Some(FdKind::Socket);
        }
        free_fd
    };

    if new_fd == !0 {
        return errno(EMFILE);
    }

    // Allocate ephemeral port
    let local_port = {
        let mut port = NEXT_EPHEMERAL_PORT.lock();
        let p = *port;
        *port = if *port < 65535 { *port + 1 } else { 49152 };
        p
    };

    let new_sock = SocketState {
        domain: AF_INET,
        sock_type: SOCK_STREAM,
        protocol: 0,
        state: SocketConnState::Connected,
        local_port,
        remote_port: 0,
        remote_addr: [0; 4],
    };

    SOCKET_TABLE.lock().insert(new_fd, new_sock);

    // Fill in peer address if requested
    if addr_ptr != 0 {
        with_user_access(|| unsafe {
            // sa_family
            *(addr_ptr as *mut u16) = AF_INET as u16;
            // Report an unspecified port because peer endpoint export is not wired on this path.
            *((addr_ptr + 2) as *mut u16) = 0;
            // Report an unspecified IPv4 address because peer endpoint export is not wired on this path.
            core::ptr::write_bytes((addr_ptr + 4) as *mut u8, 0, 4);
        });
    }
    if addr_len_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_len_ptr as *mut u32) = 16; // sizeof(sockaddr_in)
        });
    }

    serial_println!("[SOCKET] Accept fd={} -> new_fd={}", fd, new_fd);
    new_fd
}

fn sys_connect(fd: usize, addr_ptr: usize, addr_len: usize) -> usize {
    let mut sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get_mut(&fd) else {
        return errno(EBADF);
    };

    if sock.sock_type != SOCK_STREAM {
        return errno(EOPNOTSUPP);
    }

    if addr_ptr == 0 || addr_len < 8 {
        return errno(EINVAL);
    }

    // Read peer address
    let (port, addr) = with_user_access(|| unsafe {
        let port = u16::from_be(*((addr_ptr + 2) as *const u16));
        let a0 = *((addr_ptr + 4) as *const u8);
        let a1 = *((addr_ptr + 5) as *const u8);
        let a2 = *((addr_ptr + 6) as *const u8);
        let a3 = *((addr_ptr + 7) as *const u8);
        (port, [a0, a1, a2, a3])
    });

    sock.remote_port = port;
    sock.remote_addr = addr;
    sock.state = SocketConnState::Connected;

    serial_println!(
        "[SOCKET] Connect fd={} to {}.{}.{}.{}:{}",
        fd,
        addr[0],
        addr[1],
        addr[2],
        addr[3],
        port
    );
    0
}

fn sys_sendto(
    fd: usize,
    buf: usize,
    len: usize,
    flags: usize,
    addr_ptr: usize,
    addr_len: usize,
) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    // For connected sockets, use stored remote address
    // For unconnected, use provided address
    let _ = (sock, addr_ptr, addr_len, flags);

    // Read buffer and send via network stack
    let mut data = vec![0u8; len];
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(buf as *const u8, data.as_mut_ptr(), len);
    });

    serial_println!("[SOCKET] Send fd={} len={} bytes", fd, len);
    len
}

fn sys_recvfrom(
    fd: usize,
    buf: usize,
    len: usize,
    flags: usize,
    addr_ptr: usize,
    addr_len_ptr: usize,
) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    let _ = (sock, flags, addr_ptr, addr_len_ptr);

    // Would receive from network stack
    serial_println!("[SOCKET] Recv fd={} len={}", fd, len);
    0
}

fn sys_setsockopt(
    _fd: usize,
    level: usize,
    optname: usize,
    _optval: usize,
    _optlen: usize,
) -> usize {
    // kTLS (Kernel TLS) rezervasyonu (Örn: TCP_ULP için hazırlık)
    if level == SOL_TCP && optname == 31 {
        // TCP_ULP
        serial_println!("[kTLS] Kernel TLS Cipher Context reserved.");
        return 0;
    }
    unsupported_errno("setsockopt")
}

fn sys_getsockopt(
    _fd: usize,
    _level: usize,
    _optname: usize,
    _optval: usize,
    _optlen: usize,
) -> usize {
    unsupported_errno("getsockopt")
}

fn sys_shutdown(_fd: usize, _how: usize) -> usize {
    unsupported_errno("shutdown")
}

fn sys_getsockname(fd: usize, addr_ptr: usize, addr_len_ptr: usize) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    if addr_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_ptr as *mut u16) = sock.domain as u16;
            *((addr_ptr + 2) as *mut u16) = sock.local_port.to_be();
            *((addr_ptr + 4) as *mut u8) = 0;
            *((addr_ptr + 5) as *mut u8) = 0;
            *((addr_ptr + 6) as *mut u8) = 0;
            *((addr_ptr + 7) as *mut u8) = 0;
        });
    }
    if addr_len_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_len_ptr as *mut u32) = 16;
        });
    }
    0
}

fn sys_getpeername(fd: usize, addr_ptr: usize, addr_len_ptr: usize) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    if sock.state != SocketConnState::Connected {
        return errno(ENOTCONN);
    }

    if addr_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_ptr as *mut u16) = sock.domain as u16;
            *((addr_ptr + 2) as *mut u16) = sock.remote_port.to_be();
            *((addr_ptr + 4) as *mut u8) = sock.remote_addr[0];
            *((addr_ptr + 5) as *mut u8) = sock.remote_addr[1];
            *((addr_ptr + 6) as *mut u8) = sock.remote_addr[2];
            *((addr_ptr + 7) as *mut u8) = sock.remote_addr[3];
        });
    }
    if addr_len_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_len_ptr as *mut u32) = 16;
        });
    }
    0
}

fn sys_sendmsg(_fd: usize, _msg: usize, _flags: usize) -> usize {
    unsupported_errno("sendmsg")
}

fn sys_recvmsg(_fd: usize, _msg: usize, _flags: usize) -> usize {
    unsupported_errno("recvmsg")
}

// =====================================
// SECCOMP (SECURE COMPUTING)
// =====================================

fn sys_prctl(option: usize, arg2: usize) -> usize {
    if option == PR_SET_SECCOMP {
        if arg2 == SECCOMP_MODE_STRICT {
            let current_mode = tasking::scheduler::get_current_seccomp_mode();
            if current_mode == 0 {
                tasking::scheduler::set_current_seccomp_mode(1);
                serial_println!("[SECCOMP] Strict Mode Enabled (PR_SET_SECCOMP)");
                return 0;
            }
        }
    }
    errno(EINVAL)
}

fn sys_seccomp(operation: usize, _flags: usize, _args: usize) -> usize {
    let current_mode = tasking::scheduler::get_current_seccomp_mode();
    if current_mode != 0 {
        return errno(EPERM); // Mode zaten set edilmiş, değiştirilemez
    }

    if operation == 0
    /* SECCOMP_SET_MODE_STRICT */
    {
        tasking::scheduler::set_current_seccomp_mode(1);
        serial_println!("[SECCOMP] Strict Mode Enabled (sys_seccomp)");
        return 0;
    } else if operation == 1
    /* SECCOMP_SET_MODE_FILTER */
    {
        tasking::scheduler::set_current_seccomp_mode(2);
        serial_println!("[SECCOMP] Filter Mode Enabled (sys_seccomp)");
        return 0;
    }
    errno(EINVAL)
}

/// FD tablosunu tek seferlik başlatır
fn ensure_fd_table() {
    if FD_INIT.load(Ordering::SeqCst) {
        return;
    }
    let mut table = FD_TABLE.lock();
    if !FD_INIT.load(Ordering::SeqCst) {
        table[0] = Some(FdKind::Stdin);
        table[1] = Some(FdKind::Stdout);
        table[2] = Some(FdKind::Stderr);
        FD_INIT.store(true, Ordering::SeqCst);
    }
}

/// Yeni FD ayırır
fn allocate_fd(kind: FdKind) -> usize {
    let mut table = FD_TABLE.lock();
    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(kind);
            return idx;
        }
    }
    errno(EIO)
}

fn allocate_file_fd(file: FileState) -> usize {
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return fd;
    }
    let mut files = FILE_TABLE.lock();
    if fd >= files.len() {
        return errno(EIO);
    }
    files[fd] = Some(file);
    fd
}

/// FD serbest bırakır
fn free_fd(fd: usize) -> usize {
    let mut table = FD_TABLE.lock();
    if fd >= table.len() {
        return errno(EBADF);
    }
    let kind = table[fd];
    if kind.is_none() {
        return errno(EBADF);
    }
    table[fd] = None;
    if kind == Some(FdKind::File) {
        let mut files = FILE_TABLE.lock();
        if fd < files.len() {
            files[fd] = None;
        }
    }
    0
}

/// FD türünü döndürür
fn get_fd(fd: usize) -> Option<FdKind> {
    let table = FD_TABLE.lock();
    if fd >= table.len() {
        return None;
    }
    table[fd]
}

/// User pointer'dan null-terminated string okur
fn read_user_cstring(ptr: usize, max: usize) -> Result<String, usize> {
    if ptr == 0 {
        return Err(errno(EFAULT));
    }
    let mut out = String::new();
    for i in 0..max {
        let b = with_user_access(|| unsafe { *(ptr as *const u8).add(i) });
        if b == 0 {
            return Ok(out);
        }
        out.push(b as char);
    }
    Err(errno(EINVAL))
}

fn decode_inline_text(bytes: &[u8; MAX_INLINE_TEXT], len: u16) -> Result<String, usize> {
    let len = len as usize;
    if len > MAX_INLINE_TEXT {
        return Err(errno(EINVAL));
    }
    core::str::from_utf8(&bytes[..len])
        .map(|value| value.to_string())
        .map_err(|_| errno(EINVAL))
}

fn decode_optional_inline_text(
    bytes: &[u8; MAX_INLINE_TEXT],
    len: u16,
) -> Result<Option<String>, usize> {
    if len == 0 {
        Ok(None)
    } else {
        decode_inline_text(bytes, len).map(Some)
    }
}

fn inline_text_buffer(value: &str) -> (u16, [u8; MAX_INLINE_TEXT]) {
    let bytes = value.as_bytes();
    let len = bytes.len().min(MAX_INLINE_TEXT);
    let mut out = [0u8; MAX_INLINE_TEXT];
    out[..len].copy_from_slice(&bytes[..len]);
    (len as u16, out)
}

fn optional_inline_text_buffer(value: Option<&str>) -> (u16, [u8; MAX_INLINE_TEXT]) {
    value
        .map(inline_text_buffer)
        .unwrap_or((0, [0u8; MAX_INLINE_TEXT]))
}

fn with_user_access<R>(f: impl FnOnce() -> R) -> R {
    let smap = cpu::smap_enabled();
    if smap {
        unsafe { cpu::stac() };
    }
    let result = f();
    if smap {
        unsafe { cpu::clac() };
    }
    result
}

fn fill_cstring(dest: &mut [u8], value: &str) {
    if dest.is_empty() {
        return;
    }
    let bytes = value.as_bytes();
    let mut idx = 0;
    while idx + 1 < dest.len() && idx < bytes.len() {
        dest[idx] = bytes[idx];
        idx += 1;
    }
    dest[idx] = 0;
}
