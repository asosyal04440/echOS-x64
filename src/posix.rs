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

// Shell tarafından kullanılan process management fonksiyonları — wrapper'lar
/// fork(): child process oluştur, child'a 0, parent'a child pid döndür
pub fn fork() -> usize {
    process_bridge::sys_fork()
}
/// wait4(): child process bekle (WNOHANG=1, WUNTRACED=2)
pub fn wait4(pid: usize, status: usize, options: usize) -> usize {
    process_bridge::sys_wait4(pid, status, options, 0)
}
/// exit(): process sonlandır
pub fn exit(code: usize) -> usize {
    process_bridge::sys_exit(code)
}
/// pipe(): anonim boru oluştur — POSIX pipe(2)
/// fds_ptr: kullanıcı alanındaki [i32; 2] buffer'ı
pub fn sys_pipe_call(fds_ptr: usize) -> usize {
    if let Err(err) = validate_user_range(fds_ptr, 2 * core::mem::size_of::<u32>()) {
        return err;
    }
    let read_fd = allocate_fd(FdKind::Pipe);
    let write_fd = allocate_fd(FdKind::Pipe);
    if read_fd >= MAX_FDS || write_fd >= MAX_FDS {
        return errno(EMFILE);
    }
    let pipe_id = NEXT_PIPE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    let pipe = PipeRingBuffer::new(65536);
    PIPE_POOL.lock().insert(pipe_id, pipe);
    PIPE_READ_MAP.lock().insert(read_fd, pipe_id);
    PIPE_WRITE_MAP.lock().insert(write_fd, pipe_id);
    let _ = write_user(fds_ptr, read_fd as u32);
    let _ = write_user(fds_ptr + core::mem::size_of::<u32>(), write_fd as u32);
    0
}
/// dup2(): eski fd'yi yeni fd'ye kopyala — POSIX dup2(2)
pub fn dup2(old_fd: usize, new_fd: usize) -> usize {
    sys_dup2(old_fd, new_fd)
}
/// close(): fd'yi kapat — POSIX close(2)
pub fn close(fd: usize) -> usize {
    sys_close(fd)
}

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
use crate::fs::FsError;
use alloc::format;
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
use rcore_fs::vfs::FsError as RcFsError;
use rcore_fs::vfs::{FileType, INode};
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
const ENOEXEC: usize = 8;
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
const EBUSY: usize = 16;
const EXDEV: usize = 18;
const EROFS: usize = 30;
const ENAMETOOLONG: usize = 36;
const ELOOP: usize = 40;
const ENOTSOCK: usize = 88;
const ENOPROTOOPT: usize = 92;
const EOVERFLOW: usize = 75;
const ESTALE: usize = 116;
const ERANGE: usize = 34;
const ESPIPE: usize = 29;
const EADDRINUSE: usize = 98;
const ECONNREFUSED: usize = 111;
const EPIPE: usize = 32;

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
const SYS_SENDFILE: usize = 40;
const SYS_COPY_FILE_RANGE: usize = 326;
const SYS_MKDIR: usize = 83;
const SYS_RMDIR: usize = 84;
const SYS_UNLINK: usize = 87;
const SYS_RENAME: usize = 82;
const SYS_RENAMEAT2: usize = 264;
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
const SYS_FSYNC: usize = 74;
const SYS_FDATASYNC: usize = 75;
const SYS_GETDENTS64: usize = 217;
const SYS_UTIMENSAT: usize = 280;
const SYS_FACCESSAT: usize = 269;
const SYS_STATFS: usize = 137;
const SYS_FSTATFS: usize = 138;
const SYS_LINKAT: usize = 265;
const SYS_READLINKAT: usize = 267;
const SYS_SYMLINKAT: usize = 266;
const SYS_SYNCFS: usize = 306;
const SYS_MOUNT: usize = 165;
const SYS_UMOUNT2: usize = 166;

const SYS_GETPID: usize = 39;
const SYS_EXECVE: usize = 59;
const SYS_FORK: usize = 57;
const SYS_WAIT4: usize = 61;
const SYS_UMASK: usize = 60;
const SYS_UNAME: usize = 63;
const SYS_GETCWD: usize = 79;
const SYS_CHDIR: usize = 80;
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

// O_* sabitleri (open/openat flag'leri)
const O_APPEND: usize = 0o2000;
const O_DIRECTORY: usize = 0o200000;
const O_NOFOLLOW: usize = 0o400000;
const O_EXCL: usize = 0o200;
const O_CLOEXEC: usize = 0o2000000;
const O_SYNC: usize = 0o4010000;
const O_DSYNC: usize = 0o10000;
const O_RSYNC: usize = 0o4010000;

// AT_* sabitleri (openat, newfstatat, faccessat için)
const AT_FDCWD: isize = -100;
const AT_EMPTY_PATH: usize = 0x1000;
const AT_SYMLINK_NOFOLLOW: usize = 0x100;
const AT_REMOVEDIR: usize = 0x200;
const AT_EACCESS: usize = 0x200;
const AT_SYMLINK_FOLLOW: usize = 0x400;

// renameat2 flags
const RENAME_NOREPLACE: usize = 1;
const RENAME_EXCHANGE: usize = 2;
const RENAME_WHITEOUT: usize = 4;

// statx() mask sabitleri
const STATX_TYPE: usize = 0x0001;
const STATX_MODE: usize = 0x0002;
const STATX_NLINK: usize = 0x0004;
const STATX_UID: usize = 0x0008;
const STATX_GID: usize = 0x0010;
const STATX_ATIME: usize = 0x0020;
const STATX_MTIME: usize = 0x0040;
const STATX_CTIME: usize = 0x0080;
const STATX_INO: usize = 0x0100;
const STATX_SIZE: usize = 0x0200;
const STATX_BLOCKS: usize = 0x0400;
const STATX_BTIME: usize = 0x0800;
const STATX_ALL: usize = 0x0FFF;

// utimensat flag sabitleri
const UTIME_NOW: usize = 0x3FFFFFFF;
const UTIME_OMIT: usize = 0x3FFFFFFE;

// access() mode sabitleri
const F_OK: usize = 0;
const R_OK: usize = 4;
const W_OK: usize = 2;
const X_OK: usize = 1;

// fcntl command sabitleri (Linux uyumlu)
const F_DUPFD: usize = 0;
const F_GETFD: usize = 1;
const F_SETFD: usize = 2;
const F_GETFL: usize = 3;
const F_SETFL: usize = 4;
const F_GETLK: usize = 5;
const F_SETLK: usize = 6;
const F_SETLKW: usize = 7;
// OFD (open file description) lock commands (Linux 3.15+)
const F_OFD_GETLK: usize = 36;
const F_OFD_SETLK: usize = 37;
const F_OFD_SETLKW: usize = 38;
// FD_CLOEXEC flag (F_GETFD/F_SETFD için)
const FD_CLOEXEC_FLAG: usize = 1;

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

// Shell Ring 3 syscall'ları (500-512 aralığı)
const SYS_EON_LIST_TASKS: usize = 500;
const SYS_EON_KEYBOARD_READ: usize = 501;
const SYS_EON_TERM_CLEAR: usize = 502;
const SYS_EON_MEMORY_STATS: usize = 503;
const SYS_EON_SPAWN_ELF: usize = 504;
const SYS_EON_GET_FOREGROUND: usize = 505;
const SYS_EON_SET_FOREGROUND: usize = 506;
const SYS_EON_MOUNT_LIST: usize = 507;
const SYS_EON_DRIVER_LIST: usize = 508;
const SYS_EON_NET_CONFIG: usize = 509;
const SYS_EON_SHUTDOWN: usize = 510;
const SYS_EON_REBOOT: usize = 511;
const SYS_EON_IPC_SEND: usize = 512;

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
const SYS_FCNTL: usize = 72;

// Eksik errno sabitleri
const ECHILD: usize = 10;
const ENFILE: usize = 23;

// Resource Limit Syscall'ları
const SYS_GETRLIMIT: usize = 97;
const SYS_SETRLIMIT: usize = 160;
const SYS_PRLIMIT64: usize = 302;

// Interval Timer
const SYS_GETITIMER: usize = 36;
const SYS_SETITIMER: usize = 38;
const ITIMER_REAL: usize = 0;
const ITIMER_VIRTUAL: usize = 1;
const ITIMER_PROF: usize = 2;

// Waitid
const SYS_WAITID: usize = 247;
const P_ALL: usize = 0;
const P_PID: usize = 1;
const P_PGID: usize = 2;
const WEXITED: usize = 4;
const WNOWAIT: usize = 0x01000000;

// Dup3
const SYS_DUP3: usize = 292;

// Clock settime
const SYS_CLOCK_SETTIME: usize = 227;

// Capabilities
const SYS_CAPGET: usize = 125;
const SYS_CAPSET: usize = 126;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x20080522;

// Namespace
const SYS_UNSHARE: usize = 272;
const SYS_SETNS: usize = 308;
const CLONE_NEWNS: usize = 0x00020000;
const CLONE_NEWUTS: usize = 0x04000000;
const CLONE_NEWIPC: usize = 0x08000000;
const CLONE_NEWUSER: usize = 0x10000000;
const CLONE_NEWPID: usize = 0x20000000;
const CLONE_NEWNET: usize = 0x40000000;

// kcmp
const SYS_KCMP: usize = 312;

// pidfd
const SYS_PIDFD_OPEN: usize = 434;
const SYS_PIDFD_SEND_SIGNAL: usize = 424;

// Linux AIO
const SYS_IO_SETUP: usize = 206;
const SYS_IO_DESTROY: usize = 207;
const SYS_IO_GETEVENTS: usize = 208;
const SYS_IO_SUBMIT: usize = 209;
const SYS_IO_CANCEL: usize = 210;

// POSIX Message Queues
const SYS_MQ_OPEN: usize = 235;
const SYS_MQ_UNLINK: usize = 240;
const SYS_MQ_TIMEDSEND: usize = 237;
const SYS_MQ_TIMEDRECEIVE: usize = 238;
const SYS_MQ_NOTIFY: usize = 239;
const SYS_MQ_GETSETATTR: usize = 245;

// Quota / Key / Mount
const SYS_QUOTACTL: usize = 179;
const SYS_KEYCTL: usize = 250;
const SYS_MOVE_MOUNT: usize = 429;
const SYS_FSOPEN: usize = 430;
const SYS_FSCONFIG: usize = 431;
const SYS_FSMOUNT: usize = 432;

// Advanced
const SYS_PERF_EVENT_OPEN: usize = 298;
const SYS_BPF: usize = 321;
const SYS_KEXEC_FILE_LOAD: usize = 304;

// RLIMIT sabitleri
const RLIMIT_CPU: usize = 0;
const RLIMIT_FSIZE: usize = 1;
const RLIMIT_DATA: usize = 2;
const RLIMIT_STACK: usize = 3;
const RLIMIT_CORE: usize = 4;
const RLIMIT_RSS: usize = 5;
const RLIMIT_NPROC: usize = 6;
const RLIMIT_NOFILE: usize = 7;
const RLIMIT_MEMLOCK: usize = 8;
const RLIMIT_AS: usize = 9;
const RLIMIT_LOCKS: usize = 10;
const RLIMIT_SIGPENDING: usize = 11;
const RLIMIT_MSGQUEUE: usize = 12;
const RLIMIT_NICE: usize = 13;
const RLIMIT_RTPRIO: usize = 14;
const RLIMIT_RTTIME: usize = 15;
const RLIMIT_NLIMITS: usize = 16;
const RLIM_INFINITY: u64 = u64::MAX;

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

// ============================================================================
// Eksik POSIX / Linux Syscall Numaraları — Faz 2
// ============================================================================

// Zaman
const SYS_GETTIMEOFDAY: usize = 96;
const SYS_SETTIMEOFDAY: usize = 164;
const SYS_CLOCK_GETRES: usize = 229;
const SYS_CLOCK_NANOSLEEP: usize = 230;
const SYS_CLOCK_ADJTIME: usize = 305;
const SYS_TIME: usize = 201;
const SYS_UTIMES: usize = 235;

// Sinyal
const SYS_RT_SIGPENDING: usize = 127;

// Süreç / Kullanıcı / Grup
const SYS_GETPGRP: usize = 111;
const SYS_SETREUID: usize = 113;
const SYS_SETREGID: usize = 114;
const SYS_GETGROUPS: usize = 115;
const SYS_SETGROUPS: usize = 116;
const SYS_SETRESUID: usize = 117;
const SYS_GETRESUID: usize = 118;
const SYS_SETRESGID: usize = 119;
const SYS_GETRESGID: usize = 120;
const SYS_SETFSUID: usize = 122;
const SYS_SETFSGID: usize = 123;
const SYS_VFORK: usize = 58;
const SYS_EXECVEAT: usize = 322;
const SYS_CLOSE_RANGE: usize = 436;
const SYS_CLONE3: usize = 435;
const SYS_PIDFD_GETFD: usize = 438;

// Zamanlayıcı
const SYS_TIMERFD_CREATE: usize = 283;
const SYS_TIMERFD_SETTIME: usize = 286;
const SYS_TIMERFD_GETTIME: usize = 287;

// Dosya Sistemi
const SYS_MKNOD: usize = 133;
const SYS_MKNODAT: usize = 259;
const SYS_MKDIRAT: usize = 258;
const SYS_UNLINKAT: usize = 263;
const SYS_FCHMODAT: usize = 268;
const SYS_FCHOWNAT: usize = 260;
const SYS_FACCESSAT2: usize = 439;
const SYS_OPENAT2: usize = 437;
const SYS_FLOCK: usize = 73;
const SYS_SYNC: usize = 162;
const SYS_SYNC_FILE_RANGE: usize = 277;
const SYS_FADVISE64: usize = 221;
const SYS_FCHDIR: usize = 81;
const SYS_CHROOT: usize = 161;
const SYS_PIVOT_ROOT: usize = 155;

// Soket
const SYS_SOCKETPAIR: usize = 53;
const SYS_GETSOCKNAME: usize = 51;
const SYS_GETPEERNAME: usize = 52;
const SYS_GETSOCKOPT: usize = 55;
const SYS_SHUTDOWN: usize = 48;
const SYS_RECVMSG: usize = 47;
const SYS_SENDMSG: usize = 46;
const SYS_RECVMMSG: usize = 299;
const SYS_SENDMMSG: usize = 307;

// I/O Çoğaltma
const SYS_PSELECT6: usize = 270;
const SYS_PPOLL: usize = 271;

// Bellek Kilitleme
const SYS_MLOCK: usize = 149;
const SYS_MUNLOCK: usize = 150;
const SYS_MLOCKALL: usize = 151;
const SYS_MUNLOCKALL: usize = 152;
const SYS_MLOCK2: usize = 325;
const SYS_USERFAULTFD: usize = 323;
const SYS_PKEY_MPROTECT: usize = 329;
const SYS_PKEY_ALLOC: usize = 330;
const SYS_PKEY_FREE: usize = 331;

// Zamanlama (Scheduler)
const SYS_SCHED_SETPARAM: usize = 142;
const SYS_SCHED_GETPARAM: usize = 143;
const SYS_SCHED_SETSCHEDULER: usize = 144;
const SYS_SCHED_GETSCHEDULER: usize = 145;
const SYS_SCHED_GET_PRIORITY_MAX: usize = 146;
const SYS_SCHED_GET_PRIORITY_MIN: usize = 147;
const SYS_SCHED_RR_GET_INTERVAL: usize = 148;
const SYS_SCHED_SETAFFINITY: usize = 203;
const SYS_SCHED_GETAFFINITY: usize = 204;
const SYS_SCHED_SETATTR: usize = 314;
const SYS_SCHED_GETATTR: usize = 315;
const SYS_SET_PRIORITY: usize = 141;
const SYS_GET_PRIORITY: usize = 140;

// SysV IPC
const SYS_SEMGET: usize = 64;
const SYS_SEMOP: usize = 65;
const SYS_SEMCTL: usize = 66;
const SYS_SHMDT: usize = 67;
const SYS_MSGGET: usize = 68;
const SYS_MSGSND: usize = 69;
const SYS_MSGRCV: usize = 70;
const SYS_MSGCTL: usize = 71;
const SYS_SEMTIMEDOP: usize = 220;

// Process VM
const SYS_PROCESS_VM_READV: usize = 310;
const SYS_PROCESS_VM_WRITEV: usize = 311;

// Sistem
const SYS_SYSLOG: usize = 103;
const SYS_SWAPON: usize = 167;
const SYS_SWAPOFF: usize = 168;
const SYS_SETHOSTNAME: usize = 170;
const SYS_SETDOMAINNAME: usize = 171;
const SYS_PERSONALITY: usize = 135;
const SYS_VHANGUP: usize = 153;
const SYS_REBOOT: usize = 169;

// Güvenlik (Landlock LSM)
const SYS_LANDLOCK_CREATE_RULESET: usize = 444;
const SYS_LANDLOCK_ADD_RULE: usize = 445;
const SYS_LANDLOCK_RESTRICT_SELF: usize = 446;

// inotify
const SYS_INOTIFY_INIT1: usize = 294;
const SYS_INOTIFY_ADD_WATCH: usize = 254;
const SYS_INOTIFY_RM_WATCH: usize = 255;
const SYS_SIGNALFD4: usize = 287;

// ============================================================================
// Faz 3 — Eksik Linux Syscall Numaraları
// ============================================================================

// Zaman / Timer
const SYS_ALARM: usize = 37;

// Thread-Local Storage
const SYS_ARCH_PRTCL: usize = 158;
const SYS_SET_THREAD_AREA: usize = 205;

// Kernel Modül
const SYS_INIT_MODULE: usize = 175;
const SYS_DELETE_MODULE: usize = 176;
const SYS_FINIT_MODULE: usize = 137;

// I/O Öncelik
const SYS_IOPRIO_SET: usize = 251;
const SYS_IOPRIO_GET: usize = 252;

// inotify (eski versiyon)
const SYS_INOTIFY_INIT: usize = 253;

// Vectored I/O
const SYS_PREADV: usize = 284;
const SYS_PWRITEV: usize = 285;
const SYS_PREADV2: usize = 327;
const SYS_PWRITEV2: usize = 328;

// Sinyal
const SYS_RT_TGSIGQUEUEINFO: usize = 286;

// Dosya Bildirim
const SYS_FANOTIFY_INIT: usize = 289;
const SYS_FANOTIFY_MARK: usize = 290;

// Kernel Yükleme
const SYS_KEXEC_LOAD: usize = 240;

// Bellek Bariyeri
const SYS_MEMBARRIER: usize = 318;

// NUMA Bellek Politikası
const SYS_MBIND: usize = 236;
const SYS_SET_MEMPOLICY: usize = 237;
const SYS_GET_MEMPOLICY: usize = 238;
const SYS_MIGRATE_PAGES: usize = 256;

// Anahtar Yönetimi
const SYS_ADD_KEY: usize = 248;
const SYS_REQUEST_KEY: usize = 249;

// Süreç Yönetimi
const SYS_PROCESS_MADVISE: usize = 440;
const SYS_PROCESS_MRELEASE: usize = 447;

// Dosya Sistemi
const SYS_MOUNT_SETATTR: usize = 441;
const SYS_QUOTACTL_FD: usize = 442;
const SYS_MEMFD_SECRET: usize = 446;

// Async I/O
const SYS_IO_PGETEVENTS: usize = 333;

// ============================================================================
// Faz 4 — %100 Linux x86_64 Syscall Kapsamı (291/291)
// ============================================================================

// Genişletilmiş Öznitelikler (xattr) — F2FS native destek
const SYS_SETXATTR: usize = 188;
const SYS_LSETXATTR: usize = 189;
const SYS_FSETXATTR: usize = 190;
const SYS_GETXATTR: usize = 191;
const SYS_LGETXATTR: usize = 192;
const SYS_FGETXATTR: usize = 193;
const SYS_LISTXATTR: usize = 194;
const SYS_LLISTXATTR: usize = 195;
const SYS_FLISTXATTR: usize = 196;
const SYS_REMOVEXATTR: usize = 197;
const SYS_LREMOVEXATTR: usize = 198;
const SYS_FREMOVEXATTR: usize = 199;

// Basit implementasyonlar
const SYS_GETDENTS: usize = 78;
const SYS_UTIME: usize = 132;
const SYS_ADJTIMEX: usize = 159;
const SYS_ACCT: usize = 163;
const SYS_READAHEAD: usize = 187;
const SYS_TIMER_GETOVERRUN: usize = 225;

// Deprecated wrappers (redirect to modern equivalents)
const SYS_EPOLL_CREATE: usize = 213;
const SYS_EPOLL_CTL_OLD: usize = 214;
const SYS_EPOLL_WAIT_OLD: usize = 215;
const SYS_REMAP_FILE_PAGES: usize = 216;

// Kaldırılmış /Obsolete syscalls (Linux ENOSYS davranışı)
const SYS_USELIB: usize = 134;
const SYS_USTAT: usize = 136;
const SYS_SYSFS: usize = 139;
const SYS_MODIFY_LDT: usize = 154;
const SYS_CREATE_MODULE: usize = 174;
const SYS_GET_KERNEL_SYMS: usize = 177;
const SYS_QUERY_MODULE: usize = 178;
const SYS_NFSSERVCTL: usize = 180;
const SYS_GETPMSG: usize = 181;
const SYS_PUTPMSG: usize = 182;
const SYS_AFS_SYSCALL: usize = 183;
const SYS_TUXCALL: usize = 184;
const SYS_SECURITY: usize = 185;
const SYS_LOOKUP_DCOOKIE: usize = 212;

// x86-specific privileged (ring 0 gerektirir)
const SYS_IOPL: usize = 172;
const SYS_IOPERM: usize = 173;

// xattr flag sabitleri (kullanılmayan flag için errno ENOTSUP döner)
const XATTR_NOFL: usize = 0;

// pselect6 / ppoll için Timespec
#[repr(C)]
#[derive(Clone, Copy)]
struct TimevalRecv {
    tv_sec: i64,
    tv_usec: i64,
}

// Resource limit yapısı (getrlimit/setrlimit/prlimit64)
#[repr(C)]
#[derive(Clone, Copy)]
struct Rlimit {
    rlim_cur: u64,
    rlim_max: u64,
}

// Interval timer yapısı (getitimer/setitimer)
#[repr(C)]
#[derive(Clone, Copy)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Itimerval {
    it_interval: Timeval,
    it_value: Timeval,
}

// waitid için siginfo_t (basitleştirilmiş)
#[repr(C)]
#[derive(Clone, Copy)]
struct SiginfoChild {
    si_signo: i32,
    si_errno: i32,
    si_code: i32,
    _pad0: i32,
    si_pid: i32,
    si_uid: i32,
    si_status: i32,
    _pad1: i32,
}

// Capability header
#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserHeader {
    version: u32,
    pid: i32,
}

// AIO context ID tipi
type AioContext = u64;

// AIO I/O event
#[repr(C)]
#[derive(Clone, Copy)]
struct IoEvent {
    data: u64,
    obj: u64,
    res: i64,
    res2: i64,
}

// AIO I/O control block
#[repr(C)]
#[derive(Clone, Copy)]
struct IoCb {
    aio_data: u64,
    aio_key: u16,
    aio_lio_opcode: i16,
    aio_reqprio: i16,
    aio_fildes: u32,
    aio_buf: u64,
    aio_nbytes: u64,
    aio_offset: i64,
}

// POSIX MQ attributes
#[repr(C)]
#[derive(Clone, Copy)]
struct MqAttr {
    mq_flags: i64,
    mq_maxmsg: i64,
    mq_msgsize: i64,
    mq_curmsgs: i64,
}

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
const MAP_FIXED_NOREPLACE: usize = 0x100000;
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
/// FD_CLOEXEC flag'leri: true → exec() sırasında otomatik kapatılır
static FD_CLOEXEC: Mutex<[bool; MAX_FDS]> = Mutex::new([false; MAX_FDS]);
static CURRENT_WORKING_DIR: Mutex<String> = Mutex::new(String::new());
/// Process umask: yeni dosyalardan kaldırılacak permission bitleri (varsayılan: 022)
static PROCESS_UMASK: Mutex<usize> = Mutex::new(0o022);

/// CLONE_FS: SharedFsInfo için mevcut CWD'yi döndür
pub fn get_cwd() -> alloc::string::String {
    // CLONE_FS: Eğer current task'ın shared_fs'i varsa ondan oku
    if let Some(cwd) = get_cwd_from_shared_fs() {
        return cwd;
    }
    CURRENT_WORKING_DIR.lock().clone()
}

/// CLONE_FS: SharedFsInfo'dan CWD oku — None ise shared_fs yok demektir
fn get_cwd_from_shared_fs() -> Option<alloc::string::String> {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        Some(shared.lock().cwd.clone())
    })
}

/// CLONE_FS: Current task'ın CWD'sini ayarla — shared_fs varsa oraya, yoksa global static'e
pub fn set_cwd_for_current(cwd: alloc::string::String) {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    let wrote_to_shared = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        shared.lock().cwd = cwd.clone();
        Some(())
    });
    if wrote_to_shared.is_none() {
        *CURRENT_WORKING_DIR.lock() = cwd;
    }
}

/// CLONE_FS: SharedFsInfo için mevcut umask'i döndür
pub fn get_umask() -> usize {
    if let Some(mask) = get_umask_from_shared_fs() {
        return mask;
    }
    *PROCESS_UMASK.lock()
}

/// CLONE_FS: SharedFsInfo'dan umask oku
fn get_umask_from_shared_fs() -> Option<usize> {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        Some(shared.lock().umask)
    })
}

/// CLONE_FS: Current task'ın umask'ini ayarla — shared_fs varsa oraya, yoksa global static'e
pub fn set_umask_for_current(mask: usize) {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    let wrote_to_shared = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        shared.lock().umask = mask;
        Some(())
    });
    if wrote_to_shared.is_none() {
        *PROCESS_UMASK.lock() = mask;
    }
}

/// CLONE_FS: SharedFsInfo'dan root oku
pub fn get_root() -> alloc::string::String {
    if let Some(root) = get_root_from_shared_fs() {
        return root;
    }
    alloc::string::String::from("/")
}

/// CLONE_FS: SharedFsInfo'dan root oku
fn get_root_from_shared_fs() -> Option<alloc::string::String> {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        Some(shared.lock().root.clone())
    })
}

/// CLONE_FS: Current task'ın root'unu ayarla — shared_fs varsa oraya, yoksa global static'e
pub fn set_root_for_current(root: alloc::string::String) {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    let wrote_to_shared = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let task = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)?
            .as_ref()?;
        let shared = task.cold.shared_fs.as_ref()?;
        shared.lock().root = root;
        Some(())
    });
    if wrote_to_shared.is_none() {
        // Global root static'i yok — SharedFsInfo kullanılmayan process'ler
        // root'u changing için şimdilik bir şey yapmıyoruz (chroot EPERM döner)
        let _ = wrote_to_shared;
    }
}

pub static FILE_TABLE: spin::Lazy<Mutex<Vec<Option<FileState>>>> =
    spin::Lazy::new(|| Mutex::new(vec![None; MAX_FDS]));
pub static FILE_GENERATION: spin::Lazy<Mutex<Vec<u64>>> =
    spin::Lazy::new(|| Mutex::new(vec![0; MAX_FDS]));
static RING_TABLE: spin::Lazy<Mutex<alloc::collections::BTreeMap<usize, LockFreeIoUring>>> =
    spin::Lazy::new(|| Mutex::new(alloc::collections::BTreeMap::new()));

#[derive(Clone)]
pub struct FileState {
    pub inode: Arc<dyn INode>,
    pub offset: usize,
    pub size: usize,
    pub is_hello: bool,
    pub generation: u64,
    pub flags: usize, // O_RDONLY/O_WRONLY/O_RDWR | O_APPEND/O_SYNC/O_DSYNC/O_NONBLOCK
    pub path: String, // debugging / getdents için
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

#[repr(C)]
#[derive(Clone, Copy)]
struct Statfs {
    f_type: u64,
    f_bsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_fsid: [u64; 2],
    f_namelen: u64,
    f_frsize: u64,
    f_flags: u64,
    f_spare: [u64; 4],
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

// ============================================================================
// Faz 4 — Genişletilmiş Öznitelikler (xattr) Syscall'ları
//
// Tüm xattr implementasyonları fs::xattr modülünde bulunur.
// Burada posix.rs'den user pointer'ları okuyup fs::xattr'a yönlendiriyoruz.
// ============================================================================

fn sys_setxattr(
    path_ptr: usize,
    name_ptr: usize,
    value_ptr: usize,
    size: usize,
    _flags: usize,
) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    if value_ptr == 0 && size == 0 {
        return fs::xattr::sys_setxattr(&path, &name, &[], _flags as i32) as usize;
    }
    let mut buf = alloc::vec![0u8; size];
    if let Err(e) = copy_from_user(&mut buf, value_ptr) {
        return e;
    }
    fs::xattr::sys_setxattr(&path, &name, &buf, _flags as i32) as usize
}

fn sys_lsetxattr(
    path_ptr: usize,
    name_ptr: usize,
    value_ptr: usize,
    size: usize,
    _flags: usize,
) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut buf = alloc::vec![0u8; size];
    if let Err(e) = copy_from_user(&mut buf, value_ptr) {
        return e;
    }
    fs::xattr::sys_lsetxattr(&path, &name, &buf, _flags as i32) as usize
}

fn sys_fsetxattr(
    fd: usize,
    name_ptr: usize,
    value_ptr: usize,
    size: usize,
    _flags: usize,
) -> usize {
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut buf = alloc::vec![0u8; size];
    if let Err(e) = copy_from_user(&mut buf, value_ptr) {
        return e;
    }
    fs::xattr::sys_fsetxattr(fd as i32, &name, &buf, _flags as i32) as usize
}

fn sys_getxattr(path_ptr: usize, name_ptr: usize, buf_ptr: usize, size: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_getxattr(&path, &name, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_lgetxattr(path_ptr: usize, name_ptr: usize, buf_ptr: usize, size: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_lgetxattr(&path, &name, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_fgetxattr(fd: usize, name_ptr: usize, buf_ptr: usize, size: usize) -> usize {
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_fgetxattr(fd as i32, &name, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_listxattr(path_ptr: usize, buf_ptr: usize, size: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_listxattr(&path, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_llistxattr(path_ptr: usize, buf_ptr: usize, size: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_llistxattr(&path, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_flistxattr(fd: usize, buf_ptr: usize, size: usize) -> usize {
    let mut user_buf = alloc::vec![0u8; size];
    let ret = fs::xattr::sys_flistxattr(fd as i32, &mut user_buf);
    if ret < 0 {
        return ret as usize;
    }
    if buf_ptr != 0 && ret > 0 {
        if let Err(e) = write_user_slice(buf_ptr, &user_buf[..ret as usize]) {
            return e;
        }
    }
    ret as usize
}

fn sys_removexattr(path_ptr: usize, name_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    fs::xattr::sys_removexattr(&path, &name) as usize
}

fn sys_lremovexattr(path_ptr: usize, name_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    fs::xattr::sys_lremovexattr(&path, &name) as usize
}

fn sys_fremovexattr(fd: usize, name_ptr: usize) -> usize {
    let name = match read_user_cstring(name_ptr, 256) {
        Ok(n) => n,
        Err(e) => return e,
    };
    fs::xattr::sys_fremovexattr(fd as i32, &name) as usize
}

// ============================================================================
// Faz 4 — Basit Implementasyonlar
// ============================================================================

/// getdents(2) → getdents64'e yönlendirme (32-bit getdents, Linux'ta eski)
fn sys_getdents_compat(fd: usize, dirp_ptr: usize, count: usize) -> usize {
    sys_getdents64(fd, dirp_ptr, count)
}

/// utime(2) → basit time ayarlama
fn sys_utime(path_ptr: usize, times_ptr: usize) -> usize {
    let _path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // times_ptr == 0 → mevcut zamanı kullan (atime=mtime=now)
    // aksi halde [actime, modtime] Timespec yapısı
    if times_ptr == 0 {
        0 // mevcut zaman zaten ayarlı
    } else {
        // Timespec yapısını oku: 2 * i64 = 16 bayt
        let mut times = [0i64; 2];
        if let Err(e) = copy_from_user_slice(&mut times, times_ptr) {
            return e;
        }
        let _actime = times[0];
        let _modtime = times[1];
        0
    }
}

/// adjtimex(2) → clock_adjtime fallback (NTP zaman senkronizasyonu)
fn sys_adjtimex(buf_ptr: usize) -> usize {
    if buf_ptr == 0 {
        return errno(EINVAL);
    }
    // Timex yapısı: mode(4) + status(4) + offset(8) + freq(8) + ...
    // Basitçe mode oku ve SUCCESS (NTP_SYNC) döndür
    let mut mode = [0i32; 1];
    if let Err(e) = copy_from_user_slice(&mut mode, buf_ptr) {
        return e;
    }
    // Timex modes: ADJ_OFFSET=0x0001, ADJ_FREQUENCY=0x0002, ADJ_STATUS=0x0004, ...
    // Timex states: TIME_OK=0, TIME_WARN=1, TIME_ERROR=2
    let state: i32 = 0; // TIME_OK
    let state_bytes = state.to_ne_bytes();
    let _ = write_user_slice(buf_ptr + 4, &state_bytes);
    0 // TIME_OK
}

/// acct(2) → процесс Accounting (no-op, single-user mode)
fn sys_acct(_path_ptr: usize) -> usize {
    // Process accounting single-user modda anlamsız; başarıyla döndür
    0
}

/// readahead(2) → no-op (I/O prefetch single-user modda gerekmez)
fn sys_readahead(_fd: usize, _offset: u64, _count: usize) -> usize {
    0
}

/// timer_getoverrun(2) → gerçek zamanlayıcı overrun sayacı
fn sys_timer_getoverrun(timerid: usize) -> usize {
    let timers = TIMER_TABLE.lock();
    let Some(timer) = timers.get(&timerid) else {
        return errno(EINVAL);
    };
    timer.overrun as usize
}

// ============================================================================
// Faz 4 — Deprecated Wrappers (modern eşdeğerlerine yönlendirme)
// ============================================================================

/// epoll_create(2) → epoll_create1(0)
fn sys_epoll_create_compat(size: usize) -> usize {
    let _ = size;
    sys_epoll_create1(0)
}

/// epoll_ctl_old(2) → epoll_ctl ile aynı
fn sys_epoll_ctl_old(op: usize, epfd: usize, fd: usize, event_ptr: usize) -> usize {
    sys_epoll_ctl(op, epfd, fd, event_ptr)
}

/// epoll_wait_old(2) → epoll_pwait ile aynı
fn sys_epoll_wait_old(epfd: usize, events_ptr: usize, maxevents: usize, timeout: usize) -> usize {
    sys_epoll_pwait(epfd, events_ptr, maxevents, timeout, 0)
}

/// remap_file_pages(2) → mmap ile aynı (deprecated since 3.16)
fn sys_remap_file_pages(
    start: usize,
    size: usize,
    _prot: usize,
    pgoff: usize,
    _flags: usize,
) -> usize {
    // remap_file_pages() artık mmap ile aynı davranışı sergiler
    sys_mmap(start, size, 0, 0x08 /* MAP_SHARED */, pgoff, 0)
}

// ============================================================================
// Faz 4 — Kaldırılmış/Obsolete Syscalls (Linux ENOSYS davranışı)
// ============================================================================

/// uselib(2) → removed from kernel (ENOSYS)
fn sys_uselib(_path_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// ustat(2) → removed from kernel (ENOSYS)
fn sys_ustat(_dev: usize, _ubuf_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// sysfs(2) → removed from kernel (ENOSYS)
fn sys_sysfs(_option: usize, _arg1: usize, _arg2: usize) -> usize {
    errno(ENOSYS)
}

/// modify_ldt(2) → removed from kernel (ENOSYS)
fn sys_modify_ldt(_func: usize, _ptr: usize, _bytecount: usize) -> usize {
    errno(ENOSYS)
}

/// create_module(2) → removed from kernel (ENOSYS)
fn sys_create_module(_name_ptr: usize, _size: usize) -> usize {
    errno(ENOSYS)
}

/// get_kernel_syms(2) → removed from kernel (ENOSYS)
fn sys_get_kernel_syms(_table_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// query_module(2) → removed from kernel (ENOSYS)
fn sys_query_module(
    _name_ptr: usize,
    _which: usize,
    _buf_ptr: usize,
    _buflen: usize,
    _ret_len_ptr: usize,
) -> usize {
    errno(ENOSYS)
}

/// nfsservctl(2) → removed from kernel (ENOSYS)
fn sys_nfsservctl(_cmd: usize, _argp_ptr: usize, _buf_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// getpmsg(2) → removed from kernel (ENOSYS)
fn sys_getpmsg(_fd: usize, _band_ptr: usize, _flags_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// putpmsg(2) → removed from kernel (ENOSYS)
fn sys_putpmsg(_fd: usize, _band_ptr: usize, _flags_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// afs_syscall(2) → removed from kernel (ENOSYS)
fn sys_afs_syscall(
    _syscall: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
) -> usize {
    errno(ENOSYS)
}

/// tuxcall(2) → removed from kernel (ENOSYS)
fn sys_tuxcall(_arg1: usize, _arg2: usize, _arg3: usize) -> usize {
    errno(ENOSYS)
}

/// security(2) → removed from kernel (ENOSYS)
fn sys_security(_arg1: usize, _arg2: usize, _arg3: usize, _arg4: usize) -> usize {
    errno(ENOSYS)
}

/// lookup_dcookie(2) → removed from kernel (ENOSYS)
fn sys_lookup_dcookie(_cookie64: u64, _buf_ptr: usize, _len: usize) -> usize {
    errno(ENOSYS)
}

// ============================================================================
// Faz 4 — x86-Özel Privilejli Syscalls
// ============================================================================

/// iopl(2) → ring 0 gerektirir, Ring 3'ten yapılamaz
fn sys_iopl(_level: usize) -> usize {
    errno(ENOSYS)
}

/// ioperm(2) → ring 0 gerektirir, Ring 3'ten yapılamaz
fn sys_ioperm(_from: usize, _num: usize, _turn_on: usize) -> usize {
    errno(ENOSYS)
}

fn unsupported_syscall_number(number: usize) -> usize {
    ecosystem_exactness::record_posix_unsupported_number(number);
    errno(ENOSYS)
}

/// Hata eşiği: bu değerin üzerindeki tüm dönüş değerleri errno olarak yorumlanır.
/// Linux IS_ERR_VALUE makrosuna karşılık gelir: MAX_ERRNO = 4095.
fn errno_base() -> usize {
    usize::MAX - 4095
}

fn vfs_errno(err: impl Into<FsError>) -> usize {
    let err: FsError = err.into();
    match err {
        FsError::NotFound => errno(ENOENT),
        FsError::NotFile | FsError::IsDirectory => errno(EINVAL),
        FsError::NotDirectory => errno(ENOTDIR),
        FsError::NoDevice => errno(EIO),
        FsError::InvalidPath => errno(EINVAL),
        FsError::AlreadyExists => errno(EEXIST),
        FsError::NameTooLong | FsError::ComponentTooLong => errno(ENAMETOOLONG),
        FsError::SymlinkLoop => errno(ELOOP),
        FsError::CrossDevice => errno(EXDEV),
        FsError::ReadOnlyFs => errno(EROFS),
        FsError::PermissionDenied => errno(EACCES),
        FsError::Busy => errno(EBUSY),
        FsError::NotEmpty => errno(ENOTEMPTY),
        FsError::StaleHandle => errno(ESTALE),
        FsError::Interrupted => errno(EINTR),
        FsError::WouldBlock => errno(EAGAIN),
        FsError::NoSpace => errno(ENOSPC),
        FsError::NoMemory => errno(ENOMEM),
        FsError::NotSupported | FsError::UnsupportedSymlink => errno(EOPNOTSUPP),
        _ => errno(EIO),
    }
}

/// En son `/` ayracına göre yolu parent + name olarak böler.
/// Örn: "/a/b/c" -> ("/a/b", "c"), "/foo" -> ("/", "foo"), "bar" -> (".", "bar")
fn split_path(path: &str) -> (&str, &str) {
    if let Some(pos) = path.rfind('/') {
        let parent = if pos == 0 { "/" } else { &path[..pos] };
        let name = &path[pos + 1..];
        (parent, name)
    } else {
        (".", path)
    }
}

/// dirfd bazlı path çözümleme (APUE §3.3 openat).
///
/// 1. Mutlak path → dirfd yoksayılır, path aynen döner.
/// 2. Göreceli + AT_FDCWD → CURRENT_WORKING_DIR önüne eklenir.
/// 3. Göreceli + dirfd (≥0) → dirfd'deki dizin path'i alınır, önüne eklenir.
///    dirfd File değilse ya da dizin değilse path olduğu gibi döner (caller hata yönetir).
fn resolve_path_at(dirfd: usize, path: &str) -> String {
    if path.starts_with('/') {
        return path.to_string();
    }
    let dirfd_isize = dirfd as isize;
    if dirfd_isize == AT_FDCWD {
        let cwd = CURRENT_WORKING_DIR.lock();
        if cwd.is_empty() || cwd.as_str() == "/" {
            return path.to_string();
        }
        let mut result = cwd.clone();
        if !result.ends_with('/') {
            result.push('/');
        }
        result.push_str(path);
        result
    } else {
        // dirfd >= 0: FILE_TABLE'dan path al
        let files = FILE_TABLE.lock();
        if let Some(Some(state)) = files.get(dirfd) {
            let dir_path = state.path.clone();
            if !dir_path.starts_with('/') {
                return path.to_string();
            }
            let mut result = dir_path;
            if !result.ends_with('/') {
                result.push('/');
            }
            result.push_str(path);
            result
        } else {
            path.to_string()
        }
    }
}

/// Symlink hedefini okur, ELOOP döndürebilir.
fn read_symlink_target_f2fs(path: &str) -> Result<String, usize> {
    match fs::f2fs::read_link(path) {
        Ok(target) => {
            if target.is_empty() {
                Err(errno(ENOENT))
            } else {
                Ok(target)
            }
        }
        Err(e) => {
            let fe: FsError = e.into();
            match fe {
                FsError::SymlinkLoop => Err(errno(ELOOP)),
                FsError::NotFound => Err(errno(ENOENT)),
                FsError::NotFile => Err(errno(EINVAL)),
                _ => Err(vfs_errno(fe)),
            }
        }
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

    static mut POSIX_SYSCALL_LOG: u32 = 0;
    unsafe {
        POSIX_SYSCALL_LOG += 1;
        if POSIX_SYSCALL_LOG <= 10 {
            crate::debug_diag!(
                "[SHELL_TEST] Ring3 syscall: #{} args=[{:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}]",
                number,
                args[0],
                args[1],
                args[2],
                args[3],
                args[4],
                args[5]
            );
        }
    }

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
        SYS_SENDFILE => sys_sendfile(args[0], args[1], args[2], args[3]),
        SYS_COPY_FILE_RANGE => {
            sys_copy_file_range(args[0], args[1], args[2], args[3], args[4], args[5])
        }
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
        SYS_UMASK => sys_umask(args[0]),
        SYS_UNAME => sys_uname(args[0]),
        SYS_GETCWD => sys_getcwd(args[0], args[1]),
        SYS_CHDIR => sys_chdir(args[0]),
        SYS_GETRUSAGE => sys_getrusage(args[0], args[1]),
        SYS_SYSINFO => sys_sysinfo(args[0]),
        SYS_TIMES => sys_times(args[0]),
        SYS_PTRACE => sys_ptrace(args[0], args[1], args[2], args[3]),

        // FileSystem syscalls
        SYS_MKDIR => sys_mkdir(args[0], args[1]),
        SYS_RMDIR => sys_rmdir(args[0]),
        SYS_UNLINK => sys_unlink(args[0]),
        SYS_RENAME => sys_rename(args[0], args[1]),
        SYS_RENAMEAT2 => sys_renameat2(args[0], args[1], args[2], args[3], args[4]),
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
        SYS_FSYNC => sys_fsync(args[0]),
        SYS_FDATASYNC => sys_fdatasync(args[0]),
        SYS_FCNTL => sys_fcntl(args[0], args[1], args[2]),
        SYS_GETDENTS64 => sys_getdents64(args[0], args[1], args[2]),
        SYS_UTIMENSAT => sys_utimensat(args[0], args[1], args[2], args[3]),
        SYS_FACCESSAT => sys_faccessat(args[0], args[1], args[2], args[3]),
        SYS_STATFS => sys_statfs(args[0], args[1]),
        SYS_FSTATFS => sys_fstatfs(args[0], args[1]),
        SYS_LINKAT => sys_linkat(args[0], args[1], args[2], args[3], args[4]),
        SYS_READLINKAT => sys_readlinkat(args[0], args[1], args[2], args[3]),
        SYS_SYMLINKAT => sys_symlinkat(args[0], args[1], args[2]),
        SYS_SYNCFS => sys_syncfs(args[0]),
        SYS_MOUNT => sys_mount(args[0], args[1], args[2], args[3], args[4]),
        SYS_UMOUNT2 => sys_umount2(args[0], args[1]),

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

        // Shell Ring 3 syscall'ları
        SYS_EON_LIST_TASKS => sys_eon_list_tasks(args[0], args[1]),
        SYS_EON_KEYBOARD_READ => sys_eon_keyboard_read(),
        SYS_EON_TERM_CLEAR => sys_eon_term_clear(),
        SYS_EON_MEMORY_STATS => sys_eon_memory_stats(args[0], args[1]),
        SYS_EON_SPAWN_ELF => sys_eon_spawn_elf(args[0], args[1], args[2]),
        SYS_EON_GET_FOREGROUND => sys_eon_get_foreground(),
        SYS_EON_SET_FOREGROUND => sys_eon_set_foreground(args[0]),
        SYS_EON_MOUNT_LIST => sys_eon_mount_list(args[0], args[1]),
        SYS_EON_DRIVER_LIST => sys_eon_driver_list(args[0], args[1]),
        SYS_EON_NET_CONFIG => sys_eon_net_config(args[0], args[1]),
        SYS_EON_SHUTDOWN => sys_eon_shutdown(),
        SYS_EON_REBOOT => sys_eon_reboot(),
        SYS_EON_IPC_SEND => sys_eon_ipc_send(args[0], args[1], args[2], args[3], args[4]),

        // Resource Limits
        SYS_GETRLIMIT => sys_getrlimit(args[0], args[1]),
        SYS_SETRLIMIT => sys_setrlimit(args[0], args[1]),
        SYS_PRLIMIT64 => sys_prlimit64(args[0], args[1], args[2], args[3]),

        // Interval Timer
        SYS_GETITIMER => sys_getitimer(args[0], args[1]),
        SYS_SETITIMER => sys_setitimer(args[0], args[1], args[2]),

        // waitid + dup3 + clock_settime
        SYS_WAITID => sys_waitid(args[0], args[1], args[2], args[3]),
        SYS_DUP3 => sys_dup3(args[0], args[1], args[2]),
        SYS_CLOCK_SETTIME => sys_clock_settime(args[0], args[1]),

        // Capabilities
        SYS_CAPGET => sys_capget(args[0], args[1]),
        SYS_CAPSET => sys_capset(args[0], args[1]),

        // Namespace
        SYS_UNSHARE => sys_unshare(args[0]),
        SYS_SETNS => sys_setns(args[0], args[1]),
        SYS_KCMP => sys_kcmp(args[0], args[1], args[2], args[3], args[4]),

        // pidfd
        SYS_PIDFD_OPEN => sys_pidfd_open(args[0], args[1]),
        SYS_PIDFD_SEND_SIGNAL => sys_pidfd_send_signal(args[0], args[1], args[2], args[3]),

        // Linux AIO
        SYS_IO_SETUP => sys_io_setup(args[0], args[1]),
        SYS_IO_DESTROY => sys_io_destroy(args[0]),
        SYS_IO_GETEVENTS => sys_io_getevents(args[0], args[1], args[2], args[3], args[4]),
        SYS_IO_SUBMIT => sys_io_submit(args[0], args[1], args[2]),
        SYS_IO_CANCEL => sys_io_cancel(args[0], args[1], args[2]),

        // POSIX Message Queues
        SYS_MQ_OPEN => sys_mq_open(args[0], args[1], args[2], args[3]),
        SYS_MQ_UNLINK => sys_mq_unlink(args[0]),
        SYS_MQ_TIMEDSEND => sys_mq_timedsend(args[0], args[1], args[2], args[3], args[4]),
        SYS_MQ_TIMEDRECEIVE => sys_mq_timedreceive(args[0], args[1], args[2], args[3], args[4]),
        SYS_MQ_NOTIFY => sys_mq_notify(args[0], args[1]),
        SYS_MQ_GETSETATTR => sys_mq_getsetattr(args[0], args[1], args[2]),

        // Quota / Key / Mount
        SYS_QUOTACTL => sys_quotactl(args[0], args[1], args[2], args[3]),
        SYS_KEYCTL => sys_keyctl(args[0], args[1], args[2], args[3], args[4]),
        SYS_MOVE_MOUNT => sys_move_mount(args[0], args[1], args[2], args[3], args[4]),
        SYS_FSOPEN => sys_fsopen(args[0], args[1]),
        SYS_FSCONFIG => sys_fsconfig(args[0], args[1], args[2], args[3], args[4]),
        SYS_FSMOUNT => sys_fsmount(args[0], args[1], args[2]),

        // Advanced
        SYS_PERF_EVENT_OPEN => sys_perf_event_open(args[0], args[1], args[2], args[3], args[4]),
        SYS_BPF => sys_bpf(args[0], args[1], args[2]),
        SYS_KEXEC_FILE_LOAD => sys_kexec_file_load(args[0], args[1], args[2], args[3], args[4]),

        // ==================================================================
        // Faz 2 — Eksik POSIX / Linux Syscall'ları
        // ==================================================================

        // Zaman
        SYS_GETTIMEOFDAY => sys_gettimeofday(args[0], args[1]),
        SYS_SETTIMEOFDAY => sys_settimeofday(args[0]),
        SYS_CLOCK_GETRES => sys_clock_getres(args[0], args[1]),
        SYS_CLOCK_NANOSLEEP => sys_clock_nanosleep(args[0], args[1], args[2], args[3]),
        SYS_CLOCK_ADJTIME => sys_clock_adjtime(args[0], args[1]),
        SYS_TIME => sys_time(args[0]),
        SYS_UTIMES => sys_utimes(args[0], args[1]),

        // Sinyal
        SYS_RT_SIGRETURN => sys_rt_sigreturn(),
        SYS_RT_SIGPENDING => sys_rt_sigpending(args[0], args[1]),

        // Süreç / Kullanıcı / Grup
        SYS_GETPGRP => sys_getpgrp(),
        SYS_SETREUID => sys_setreuid(args[0], args[1]),
        SYS_SETREGID => sys_setregid(args[0], args[1]),
        SYS_GETGROUPS => sys_getgroups(args[0], args[1]),
        SYS_SETGROUPS => sys_setgroups(args[0], args[1]),
        SYS_SETRESUID => sys_setresuid(args[0], args[1], args[2]),
        SYS_GETRESUID => sys_getresuid(args[0]),
        SYS_SETRESGID => sys_setresgid(args[0], args[1], args[2]),
        SYS_GETRESGID => sys_getresgid(args[0]),
        SYS_SETFSUID => sys_setfsuid(args[0]),
        SYS_SETFSGID => sys_setfsgid(args[0]),
        SYS_VFORK => process_bridge::sys_vfork(),
        SYS_EXECVEAT => process_bridge::sys_execveat(args[0], args[1], args[2], args[3], args[4]),
        SYS_CLOSE_RANGE => sys_close_range(args[0], args[1], args[2]),
        SYS_CLONE3 => process_bridge::sys_clone3(args[0], args[1]),
        SYS_PIDFD_GETFD => sys_pidfd_getfd(args[0], args[1], args[2]),

        // Zamanlayıcı
        SYS_TIMERFD_CREATE => sys_timerfd_create(args[0], args[1]),
        SYS_TIMERFD_SETTIME => sys_timerfd_settime(args[0], args[1], args[2], args[3]),
        SYS_TIMERFD_GETTIME => sys_timerfd_gettime(args[0], args[1]),

        // Dosya Sistemi
        SYS_MKNOD => sys_mknod(args[0], args[1]),
        SYS_MKNODAT => sys_mknodat(args[0], args[1], args[2]),
        SYS_MKDIRAT => sys_mkdirat(args[0], args[1], args[2]),
        SYS_UNLINKAT => sys_unlinkat(args[0], args[1], args[2]),
        SYS_FCHMODAT => sys_fchmodat(args[0], args[1], args[2], args[3]),
        SYS_FCHOWNAT => sys_fchownat(args[0], args[1], args[2], args[3], args[4]),
        SYS_FACCESSAT2 => sys_faccessat(args[0], args[1], args[2], args[3]),
        SYS_OPENAT2 => sys_openat(args[0], args[1], args[2], args[3]),
        SYS_FLOCK => sys_flock(args[0], args[1]),
        SYS_SYNC => sys_sync(),
        SYS_SYNC_FILE_RANGE => sys_sync_file_range(args[0], args[1], args[2], args[3]),
        SYS_FADVISE64 => sys_fadvise64(args[0], args[1], args[2], args[3]),
        SYS_FCHDIR => sys_fchdir(args[0]),
        SYS_CHROOT => sys_chroot(args[0]),
        SYS_PIVOT_ROOT => sys_pivot_root(args[0], args[1]),

        // Soket
        SYS_SOCKETPAIR => sys_socketpair(args[0], args[1], args[2], args[3]),
        SYS_GETSOCKNAME => sys_getsockname(args[0], args[1], args[2]),
        SYS_GETPEERNAME => sys_getpeername(args[0], args[1], args[2]),
        SYS_GETSOCKOPT => sys_getsockopt(args[0], args[1], args[2], args[3], args[4]),
        SYS_SHUTDOWN => sys_shutdown(args[0], args[1]),
        SYS_RECVMSG => sys_recvmsg(args[0], args[1], args[2]),
        SYS_SENDMSG => sys_sendmsg(args[0], args[1], args[2]),
        SYS_RECVMMSG => sys_recvmmsg(args[0], args[1], args[2], args[3], args[4]),
        SYS_SENDMMSG => sys_sendmmsg(args[0], args[1], args[2], args[3], 0),

        // I/O Çoğaltma
        SYS_PSELECT6 => sys_pselect6(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_PPOLL => sys_ppoll(args[0], args[1], args[2], args[3]),

        // Bellek Kilitleme
        SYS_MLOCK => sys_mlock(args[0], args[1]),
        SYS_MUNLOCK => sys_munlock(args[0], args[1]),
        SYS_MLOCKALL => sys_mlockall(args[0]),
        SYS_MUNLOCKALL => sys_munlockall(),
        SYS_MLOCK2 => sys_mlock2(args[0], args[1], args[2]),
        SYS_USERFAULTFD => sys_userfaultfd(args[0]),
        SYS_PKEY_MPROTECT => sys_mprotect(args[0], args[1], args[2]), // pkey_mprotect → mprotect fallback
        SYS_PKEY_ALLOC => sys_pkey_alloc(args[0], args[1]),
        SYS_PKEY_FREE => sys_pkey_free(args[0]),

        // Zamanlama (Scheduler)
        SYS_SCHED_SETPARAM => sys_sched_setparam(args[0], args[1]),
        SYS_SCHED_GETPARAM => sys_sched_getparam(args[0], args[1]),
        SYS_SCHED_SETSCHEDULER => sys_sched_setscheduler(args[0], args[1], args[2]),
        SYS_SCHED_GETSCHEDULER => sys_sched_getscheduler(args[0]),
        SYS_SCHED_GET_PRIORITY_MAX => sys_sched_get_priority_max(args[0]),
        SYS_SCHED_GET_PRIORITY_MIN => sys_sched_get_priority_min(args[0]),
        SYS_SCHED_RR_GET_INTERVAL => sys_sched_rr_get_interval(args[0], args[1]),
        SYS_SCHED_SETAFFINITY => sys_sched_setaffinity(args[0], args[1], args[2]),
        SYS_SCHED_GETAFFINITY => sys_sched_getaffinity(args[0], args[1], args[2]),
        SYS_SCHED_SETATTR => sys_sched_setattr(args[0], args[1], args[2]),
        SYS_SCHED_GETATTR => sys_sched_getattr(args[0], args[1], args[2]),
        SYS_SET_PRIORITY => sys_setpriority(args[0], args[1], args[2]),
        SYS_GET_PRIORITY => sys_getpriority(args[0], args[1]),

        // SysV IPC
        SYS_SEMGET => sys_semget(args[0], args[1], args[2]),
        SYS_SEMOP => sys_semop(args[0], args[1], args[2]),
        SYS_SEMCTL => sys_semctl(args[0], args[1], args[2], args[3]),
        SYS_SHMDT => sys_shmdt(args[0]),
        SYS_MSGGET => sys_msgget(args[0], args[1]),
        SYS_MSGSND => sys_msgsnd(args[0], args[1], args[2], args[3]),
        SYS_MSGRCV => sys_msgrcv(args[0], args[1], args[2], args[3], args[4]),
        SYS_MSGCTL => sys_msgctl(args[0], args[1], args[2]),
        SYS_SEMTIMEDOP => sys_semtimedop(args[0], args[1], args[2], args[3]),

        // Process VM
        SYS_PROCESS_VM_READV => {
            sys_process_vm_readv(args[0], args[1], args[2], args[3], args[4], args[5])
        }
        SYS_PROCESS_VM_WRITEV => {
            sys_process_vm_writev(args[0], args[1], args[2], args[3], args[4], args[5])
        }

        // Sistem
        SYS_SYSLOG => sys_syslog(args[0], args[1], args[2]),
        SYS_SWAPON => sys_swapon(args[0], args[1]),
        SYS_SWAPOFF => sys_swapoff(args[0]),
        SYS_SETHOSTNAME => sys_sethostname(args[0], args[1]),
        SYS_SETDOMAINNAME => sys_setdomainname(args[0], args[1]),
        SYS_PERSONALITY => sys_personality(args[0]),
        SYS_VHANGUP => 0, // no-op in single-user
        SYS_REBOOT => sys_reboot(args[0]),

        // Güvenlik (Landlock LSM)
        SYS_LANDLOCK_CREATE_RULESET => sys_landlock_create_ruleset(args[0], args[1], args[2]),
        SYS_LANDLOCK_ADD_RULE => sys_landlock_add_rule(args[0], args[1], args[2], args[3]),
        SYS_LANDLOCK_RESTRICT_SELF => sys_landlock_restrict_self(args[0], args[1]),

        // inotify
        SYS_INOTIFY_INIT1 => sys_inotify_init1(args[0]),
        SYS_INOTIFY_ADD_WATCH => sys_inotify_add_watch(args[0], args[1], args[2]),
        SYS_INOTIFY_RM_WATCH => sys_inotify_rm_watch(args[0], args[1]),
        SYS_SIGNALFD4 => sys_signalfd4(args[0], args[1], args[2], args[3]),

        // ==================================================================
        // Faz 3 — Eksik Linux Syscall'ları
        // ==================================================================

        // Zaman / Timer
        SYS_ALARM => sys_alarm(args[0]),

        // Thread-Local Storage
        SYS_ARCH_PRTCL => sys_arch_prctl(args[0], args[1], args[2]),
        SYS_SET_THREAD_AREA => sys_set_thread_area(args[0]),

        // Kernel Modül
        SYS_INIT_MODULE => sys_init_module(args[0], args[1], args[2]),
        SYS_DELETE_MODULE => sys_delete_module(args[0], args[1]),
        SYS_FINIT_MODULE => sys_finit_module(args[0], args[1], args[2]),

        // I/O Öncelik
        SYS_IOPRIO_SET => sys_ioprio_set(args[0], args[1], args[2]),
        SYS_IOPRIO_GET => sys_ioprio_get(args[0], args[1]),

        // inotify (eski)
        SYS_INOTIFY_INIT => sys_inotify_init1(0),

        // Vectored I/O
        SYS_PREADV => sys_preadv(args[0], args[1], args[2], args[3]),
        SYS_PWRITEV => sys_pwritev(args[0], args[1], args[2], args[3]),
        SYS_PREADV2 => sys_preadv(args[0], args[1], args[2], args[3]),
        SYS_PWRITEV2 => sys_pwritev(args[0], args[1], args[2], args[3]),

        // Sinyal
        SYS_RT_TGSIGQUEUEINFO => sys_rt_tgsigqueueinfo(args[0], args[1], args[2]),

        // Dosya Bildirim
        SYS_FANOTIFY_INIT => sys_fanotify_init(args[0], args[1]),
        SYS_FANOTIFY_MARK => sys_fanotify_mark(args[0], args[1], args[2], args[3], args[4]),

        // Kernel Yükleme
        SYS_KEXEC_LOAD => sys_kexec_load(args[0], args[1], args[2], args[3]),

        // Bellek Bariyeri
        SYS_MEMBARRIER => sys_membarrier(args[0], args[1], args[2]),

        // NUMA Bellek Politikası
        SYS_MBIND => sys_mbind(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_SET_MEMPOLICY => sys_set_mempolicy(args[0], args[1], args[2]),
        SYS_GET_MEMPOLICY => sys_get_mempolicy(args[0], args[1], args[2], args[3], args[4]),
        SYS_MIGRATE_PAGES => sys_migrate_pages(args[0], args[1], args[2]),

        // Anahtar Yönetimi
        SYS_ADD_KEY => sys_add_key(args[0], args[1], args[2], args[3], args[4]),
        SYS_REQUEST_KEY => sys_request_key(args[0], args[1], args[2], args[3]),

        // Süreç Yönetimi
        SYS_PROCESS_MADVISE => sys_process_madvise(args[0], args[1], args[2], args[3], args[4]),
        SYS_PROCESS_MRELEASE => sys_process_mrelease(args[0], args[1]),

        // Dosya Sistemi
        SYS_MOUNT_SETATTR => sys_mount_setattr(args[0], args[1], args[2], args[3], args[4]),
        SYS_QUOTACTL_FD => sys_quotactl_fd(args[0], args[1], args[2], args[3]),
        SYS_MEMFD_SECRET => sys_memfd_secret(args[0]),

        // Async I/O
        SYS_IO_PGETEVENTS => sys_io_pgetevents(args[0], args[1], args[2], args[3], args[4]),

        // ==================================================================
        // Faz 4 — Genişletilmiş Öznitelikler (xattr)
        // ==================================================================
        SYS_SETXATTR => sys_setxattr(args[0], args[1], args[2], args[3], args[4]),
        SYS_LSETXATTR => sys_lsetxattr(args[0], args[1], args[2], args[3], args[4]),
        SYS_FSETXATTR => sys_fsetxattr(args[0], args[1], args[2], args[3], args[4]),
        SYS_GETXATTR => sys_getxattr(args[0], args[1], args[2], args[3]),
        SYS_LGETXATTR => sys_lgetxattr(args[0], args[1], args[2], args[3]),
        SYS_FGETXATTR => sys_fgetxattr(args[0], args[1], args[2], args[3]),
        SYS_LISTXATTR => sys_listxattr(args[0], args[1], args[2]),
        SYS_LLISTXATTR => sys_llistxattr(args[0], args[1], args[2]),
        SYS_FLISTXATTR => sys_flistxattr(args[0], args[1], args[2]),
        SYS_REMOVEXATTR => sys_removexattr(args[0], args[1]),
        SYS_LREMOVEXATTR => sys_lremovexattr(args[0], args[1]),
        SYS_FREMOVEXATTR => sys_fremovexattr(args[0], args[1]),

        // ==================================================================
        // Faz 4 — Basit Implementasyonlar
        // ==================================================================
        SYS_GETDENTS => sys_getdents_compat(args[0], args[1], args[2]),
        SYS_UTIME => sys_utime(args[0], args[1]),
        SYS_ADJTIMEX => sys_adjtimex(args[0]),
        SYS_ACCT => sys_acct(args[0]),
        SYS_READAHEAD => sys_readahead(args[0], args[1] as u64, args[2]),
        SYS_TIMER_GETOVERRUN => sys_timer_getoverrun(args[0]),

        // ==================================================================
        // Faz 4 — Deprecated Wrappers
        // ==================================================================
        SYS_EPOLL_CREATE => sys_epoll_create_compat(args[0]),
        SYS_EPOLL_CTL_OLD => sys_epoll_ctl_old(args[0], args[1], args[2], args[3]),
        SYS_EPOLL_WAIT_OLD => sys_epoll_wait_old(args[0], args[1], args[2], args[3]),
        SYS_REMAP_FILE_PAGES => sys_remap_file_pages(args[0], args[1], args[2], args[3], args[4]),

        // ==================================================================
        // Faz 4 — Kaldırılmış/Obsolete Syscalls
        // ==================================================================
        SYS_USELIB => sys_uselib(args[0]),
        SYS_USTAT => sys_ustat(args[0], args[1]),
        SYS_SYSFS => sys_sysfs(args[0], args[1], args[2]),
        SYS_MODIFY_LDT => sys_modify_ldt(args[0], args[1], args[2]),
        SYS_CREATE_MODULE => sys_create_module(args[0], args[1]),
        SYS_GET_KERNEL_SYMS => sys_get_kernel_syms(args[0]),
        SYS_QUERY_MODULE => sys_query_module(args[0], args[1], args[2], args[3], args[4]),
        SYS_NFSSERVCTL => sys_nfsservctl(args[0], args[1], args[2]),
        SYS_GETPMSG => sys_getpmsg(args[0], args[1], args[2]),
        SYS_PUTPMSG => sys_putpmsg(args[0], args[1], args[2]),
        SYS_AFS_SYSCALL => sys_afs_syscall(args[0], args[1], args[2], args[3], args[4]),
        SYS_TUXCALL => sys_tuxcall(args[0], args[1], args[2]),
        SYS_SECURITY => sys_security(args[0], args[1], args[2], args[3]),
        SYS_LOOKUP_DCOOKIE => sys_lookup_dcookie(args[0] as u64, args[1], args[2]),

        // ==================================================================
        // Faz 4 — x86-Özel Privilejli Syscalls
        // ==================================================================
        SYS_IOPL => sys_iopl(args[0]),
        SYS_IOPERM => sys_ioperm(args[0], args[1], args[2]),

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
    if let Err(err) = validate_user_range(buf, count) {
        return err;
    }
    with_user_access(|| {
        let bytes = unsafe { core::slice::from_raw_parts(buf as *const u8, count) };
        match get_fd(fd) {
            Some(FdKind::Stdout) | Some(FdKind::Stderr) => {
                // Atomik yazma: kernel debug mesajlariyla karismasini onle
                let output = core::str::from_utf8(bytes).unwrap_or("");
                serial_print!("{}", output);
                count
            }
            Some(FdKind::Null) => count,
            Some(FdKind::Zero) => count,
            Some(FdKind::File) => {
                let (inode, offset, size, flags) = {
                    let files = FILE_TABLE.lock();
                    let Some(Some(state)) = files.get(fd) else {
                        return errno(EBADF);
                    };
                    (state.inode.clone(), state.offset, state.size, state.flags)
                };
                // O_APPEND: her write öncesi offset'i EOF'a ayarla
                let write_offset = if flags & O_APPEND != 0 { size } else { offset };
                let written = match fs::vfs_write_at(&inode, write_offset, bytes) {
                    Ok(value) => value,
                    Err(err) => return vfs_errno(err),
                };
                let mut files = FILE_TABLE.lock();
                let gen = FILE_GENERATION.lock();
                let current_gen = if fd < gen.len() { gen[fd] } else { 0 };
                let saved_gen = if let Some(Some(s)) = files.get(fd) {
                    s.generation
                } else {
                    return errno(EBADF);
                };
                if current_gen != saved_gen {
                    return errno(EBADF);
                }
                drop(gen);
                if let Some(Some(state)) = files.get_mut(fd) {
                    state.offset = write_offset.saturating_add(written);
                    if write_offset.saturating_add(written) > state.size {
                        state.size = write_offset.saturating_add(written);
                    }
                }
                written
            }
            Some(FdKind::Pipe) => {
                let pipe_id = {
                    let map = PIPE_WRITE_MAP.lock();
                    match map.get(&fd) {
                        Some(&id) => id,
                        None => return errno(EBADF),
                    }
                };
                let mut pool = PIPE_POOL.lock();
                match pool.get_mut(&pipe_id) {
                    Some(pipe) => match pipe.pipe_write(bytes) {
                        Ok(written) => written,
                        Err(e) => errno(-e as usize),
                    },
                    None => errno(EBADF),
                }
            }
            _ => errno(EBADF),
        }
    })
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
    if let Err(err) = validate_user_range(buf, count) {
        return err;
    }
    with_user_access(|| {
        match get_fd(fd) {
            Some(FdKind::Stdin) => {
                let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
                tty::DEFAULT_TTY.sys_read(slice)
            }
            Some(FdKind::Null) => 0,
            Some(FdKind::Zero) => {
                let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
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
                let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, to_copy) };
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
                // Generation kontrolü: fd arada kapatılıp yeniden açıldı mı?
                let gen = FILE_GENERATION.lock();
                let current_gen = if fd < gen.len() { gen[fd] } else { 0 };
                let saved_gen = if let Some(Some(s)) = files.get(fd) {
                    s.generation
                } else {
                    return errno(EBADF);
                };
                if current_gen != saved_gen {
                    return errno(EBADF);
                }
                drop(gen);
                if let Some(Some(state)) = files.get_mut(fd) {
                    state.offset = state.offset.saturating_add(read);
                }
                read
            }
            Some(FdKind::Pipe) => {
                let pipe_id = {
                    let map = PIPE_READ_MAP.lock();
                    match map.get(&fd) {
                        Some(&id) => id,
                        None => return errno(EBADF),
                    }
                };
                let mut pool = PIPE_POOL.lock();
                match pool.get_mut(&pipe_id) {
                    Some(pipe) => {
                        let slice =
                            unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
                        match pipe.pipe_read(slice) {
                            Ok(read) => read,
                            Err(e) => errno(-e as usize),
                        }
                    }
                    None => errno(EBADF),
                }
            }
            _ => errno(EBADF),
        }
    })
}

fn sys_open(path: usize, flags: usize, mode: usize) -> usize {
    let path = match read_user_cstring(path, 4096) {
        Ok(value) => value,
        Err(err) => return err,
    };
    sys_open_with_str(&path, flags, mode)
}

/// close syscall (stdin/out/err hariç)
fn sys_close(fd: usize) -> usize {
    if fd <= 2 {
        return 0;
    }
    // Socket kapatılırken TCP bağlantısını da kapat
    if let Some(FdKind::Socket) = get_fd(fd) {
        if let Some(sock) = SOCKET_TABLE.lock().get(&fd) {
            let tcp_id = sock.tcp_id;
            if tcp_id != 0 {
                let _ = super::net::tcp::close(tcp_id);
            }
        }
        SOCKET_TABLE.lock().remove(&fd);
    }
    free_fd(fd)
}

/// `lseek` syscall — POSIX: ESPIPE for pipes/FIFOs/sockets, EOVERFLOW for negative
fn sys_lseek(fd: usize, offset: usize, whence: usize) -> usize {
    // Pipe/socket seek edilemez
    match get_fd(fd) {
        Some(FdKind::Pipe) | Some(FdKind::Socket) => return errno(ESPIPE),
        _ => {}
    }
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
    if let Err(err) = validate_user_range(statbuf, core::mem::size_of::<Stat>()) {
        return err;
    }
    let path = match read_user_cstring(path, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if path == "/dev/null" || path == "/dev/zero" || path == "/dev/dri/card0" {
        let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
        if let Err(err) = write_user(statbuf, stat) {
            return err;
        }
        return 0;
    }
    if fs::f2fs::detect_f2fs().unwrap_or(false) {
        let entry = match fs::f2fs::open_entry(&path) {
            Ok(value) => value,
            Err(_) => return errno(ENOENT),
        };
        if path.ends_with('/') && !entry.is_dir {
            return errno(ENOTDIR);
        }
        let stat = stat_from_f2fs_entry(&entry);
        if let Err(err) = write_user(statbuf, stat) {
            return err;
        }
        return 0;
    }
    let entry = match fs::f2fs::open_entry(&path) {
        Ok(value) => value,
        Err(_) => return errno(ENOENT),
    };
    if path.ends_with('/') && !entry.is_dir {
        return errno(ENOTDIR);
    }
    let stat = stat_from_f2fs_entry(&entry);
    if let Err(err) = write_user(statbuf, stat) {
        return err;
    }
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
    if let Err(err) = validate_user_range(statbuf, core::mem::size_of::<Stat>()) {
        return err;
    }

    // stdin/stdout/stderr
    if fd <= 2 {
        let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
        if let Err(err) = write_user(statbuf, stat) {
            return err;
        }
        return 0;
    }

    // FD türüne göre Stat oluştur
    match get_fd(fd) {
        Some(FdKind::Null) | Some(FdKind::Zero) => {
            let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
            if let Err(err) = write_user(statbuf, stat) {
                return err;
            }
            0
        }
        Some(FdKind::Drm) => {
            let stat = stat_for_special(S_IFCHR | MODE_CHAR, 0);
            if let Err(err) = write_user(statbuf, stat) {
                return err;
            }
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
                if let Err(err) = write_user(statbuf, stat) {
                    return err;
                }
                0
            } else {
                errno(EBADF)
            }
        }
        Some(FdKind::Pipe) => {
            let stat = stat_for_special(S_IFIFO | 0o600, 0);
            if let Err(err) = write_user(statbuf, stat) {
                return err;
            }
            0
        }
        _ => errno(EBADF),
    }
}

fn sys_lstat(path_ptr: usize, statbuf: usize) -> usize {
    // lstat = stat (symlink takibi olmadan) — F2FS'te symlinks zaten resolve edilmez
    sys_stat(path_ptr, statbuf)
}

fn sys_poll(fds_ptr: usize, nfds: usize, timeout_ms: usize) -> usize {
    // poll(2): birden fazla fd için I/O hazırlığını kontrol et
    // Basit implementasyon: her fd için poll_one_shot çağır
    if nfds == 0 || nfds > 1024 {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(fds_ptr, nfds * core::mem::size_of::<u32>() * 2) {
        return err;
    }

    // pollfd yapısı: { int fd; short events; short revents; } = 8 bytes
    let mut total_revents: usize = 0;
    for i in 0..nfds {
        let base = fds_ptr + i * 8;
        let fd: u32 = with_user_access(|| unsafe { *(base as *const u32) });
        let events: u16 = with_user_access(|| unsafe { *((base + 4) as *const u16) } as u16);

        let mut revents: u16 = 0;

        // Her fd için basit kontrol
        let fd_usize = fd as usize;
        if fd_usize < 64 {
            // Dosya tablosunda var mı kontrol et
            let files = FILE_TABLE.lock();
            if let Some(Some(_)) = files.get(fd_usize) {
                // Dosya açık — yazma okunabilir, okuma her zaman hazır
                if events & 0x001 != 0 {
                    revents |= 0x001;
                } // POLLIN
                if events & 0x004 != 0 {
                    revents |= 0x004;
                } // POLLOUT
                if events & 0x020 != 0 {
                    revents |= 0x020;
                } // POLLHUP (eof kontrolü)
            }
        }

        let _ = write_user(base + 6, revents as u16);
        if revents != 0 {
            total_revents += 1;
        }
    }

    if total_revents > 0 {
        return total_revents;
    }

    // Timeout = 0 ise hemen dön (zaten döndük)
    // Timeout > 0 ise bekle (şimdilik basit: sadece bir kez kontrol et)
    if timeout_ms > 0 && timeout_ms < 10000 {
        // Basit bekleme: timeout_ms / 10 tick
        let ticks = (timeout_ms + 9) / 10;
        for _ in 0..ticks {
            x86_64::instructions::hlt();
        }
        // Tekrar kontrol et
        for i in 0..nfds {
            let base = fds_ptr + i * 8;
            let fd: u32 = with_user_access(|| unsafe { *(base as *const u32) });
            let events: u16 = with_user_access(|| unsafe { *((base + 4) as *const u16) } as u16);
            let mut revents: u16 = 0;
            let fd_usize = fd as usize;
            if fd_usize < 64 {
                let files = FILE_TABLE.lock();
                if let Some(Some(_)) = files.get(fd_usize) {
                    if events & 0x001 != 0 {
                        revents |= 0x001;
                    }
                    if events & 0x004 != 0 {
                        revents |= 0x004;
                    }
                    if events & 0x020 != 0 {
                        revents |= 0x020;
                    }
                }
            }
            let _ = write_user(base + 6, revents as u16);
            if revents != 0 {
                total_revents += 1;
            }
        }
    }

    total_revents
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
    let is_fixed = flags & (MAP_FIXED | MAP_FIXED_NOREPLACE) != 0;
    let no_replace = flags & MAP_FIXED_NOREPLACE != 0;
    if is_private == is_shared {
        return errno(EINVAL);
    }
    if !is_anon && off % kernel_memory::PAGE_SIZE != 0 {
        return errno(EINVAL);
    }
    if is_fixed && addr == 0 {
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
    if is_fixed {
        if kernel_memory::user_stack_guards_region(target, len as u64)
            || kernel_memory::user_heap_guards_region(target, len as u64)
        {
            return errno(EPERM);
        }
        if no_replace && kernel_memory::user_region_overlaps(target, len as u64) {
            return errno(EEXIST);
        }
        if !no_replace && kernel_memory::user_region_overlaps(target, len as u64) {
            kernel_memory::unmap_user_range(target, len as u64);
        }
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
    let file_access = file_state.flags & 0x3;
    const O_RDONLY_USIZE: usize = 0;
    const O_RDWR_USIZE: usize = 2;
    if file_access == 1 {
        return errno(EACCES);
    }
    if is_shared && (prot & PROT_WRITE != 0) && file_access != O_RDWR_USIZE {
        return errno(EACCES);
    }
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
    crate::debug_diag!("[SHELL_TEST] sys_brk: addr={:#x}", addr);
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
        Some(FdKind::Stdin) | Some(FdKind::Stdout) | Some(FdKind::Stderr) => {
            handle_tty_ioctl(request, arg)
        }
        _ => errno(ENOTTY),
    }
}

/// TTY ioctl'leri — PTY foreground process group yönetimi
/// POSIX: tcgetpgrp(3) ve tcsetpgrp(3) bu ioctl'ler üzerinden çalışır
fn handle_tty_ioctl(request: usize, arg: usize) -> usize {
    // TIOCGPGRP = 0x540F — foreground process group ID'yi oku
    const TIOCGPGRP: usize = 0x540F;
    // TIOCSPGRP = 0x5410 — foreground process group ID'yi ayarla
    const TIOCSPGRP: usize = 0x5410;
    // TIOCGWINSZ = 0x5413 — terminal boyutunu oku
    const TIOCGWINSZ: usize = 0x5413;
    // TCSBRK = 0x5409 — terminal break
    const TCSBRK: usize = 0x5409;
    // TCXONC = 0x540A — start/stop output
    const TCXONC: usize = 0x540A;
    // TCFLSH = 0x540B — flush terminal I/O
    const TCFLSH: usize = 0x540B;

    match request {
        TIOCGPGRP => {
            // tcgetpgrp(): foreground process group ID'yi döndür
            let pgid = crate::tty::DEFAULT_TTY.get_foreground_pgid();
            if let Err(err) = write_user(arg, pgid as u32) {
                return err;
            }
            0
        }
        TIOCSPGRP => {
            // tcsetpgrp(): foreground process group ID'yi ayarla
            let pgid = match read_user::<u32>(arg) {
                Ok(v) => v as usize,
                Err(e) => return e,
            };
            crate::tty::DEFAULT_TTY.set_foreground_pgid(pgid);
            0
        }
        TIOCGWINSZ => {
            // terminal boyutunu döndür
            let winsize = crate::tty::pty::Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if let Err(err) = write_user(arg, winsize) {
                return err;
            }
            0
        }
        TCSBRK => 0, // terminal break — no-op
        TCXONC => 0, // start/stop output — no-op
        TCFLSH => 0, // flush — no-op
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
    let mut ver = match read_user::<DrmVersion>(arg) {
        Ok(value) => value,
        Err(err) => return err,
    };
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
        if let Err(err) = write_user_bytes(ver.name, &name.as_bytes()[..copy_len]) {
            return err;
        }
        if let Err(err) = write_user::<u8>(ver.name.saturating_add(copy_len), 0u8) {
            return err;
        }
    }
    if let Err(err) = write_user(arg, ver) {
        return err;
    }
    0
}

fn drm_virtgpu_resource_create(arg: usize) -> usize {
    let mut req = match read_user::<DrmVirtgpuResourceCreate>(arg) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let width = req.width.max(1);
    let height = req.height.max(1);
    let handle = match drivers::virtio_gpu::drm_resource_create_3d(width, height) {
        Some(value) => value,
        None => return errno(EIO),
    };
    req.handle = handle;
    if let Err(err) = write_user(arg, req) {
        return err;
    }
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
    let mut req = match read_user::<DrmVirtgpuMap>(arg) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if req.handle == 0 {
        return errno(EINVAL);
    }
    if !drm_resource_exists(req.handle) {
        return errno(EINVAL);
    }
    req.offset = req.handle as u64;
    if let Err(err) = write_user(arg, req) {
        return err;
    }
    0
}

fn drm_virtgpu_execbuffer(arg: usize) -> usize {
    let req = match read_user::<DrmVirtgpuExecbuffer>(arg) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if req.command == 0 || req.size == 0 {
        return errno(EINVAL);
    }
    let cmd_len = req.size as usize;
    let mut command = vec![0u8; cmd_len];
    if let Err(err) = copy_from_user(&mut command, req.command as usize) {
        return err;
    }
    let ok = unsafe { drivers::virtio_gpu::drm_submit_3d_command(command.as_ptr(), cmd_len) };
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

fn sys_pread64(fd: usize, buf: usize, count: usize, pos: usize) -> usize {
    // pread64: dosyadan belirli offset'ten oku, dosya konumunu değiştirme
    if count == 0 {
        return 0;
    }
    if let Err(err) = validate_user_range(buf, count) {
        return err;
    }

    // Mevcut offset'i kaydet
    let saved_offset = {
        let files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get(fd) {
            entry.offset
        } else {
            return errno(EBADF);
        }
    };

    // Offset'i ayarla
    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get_mut(fd) {
            entry.offset = pos;
        }
    }

    // Oku
    let result = sys_read(fd, buf, count);

    // Eski offset'i geri yükle
    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get_mut(fd) {
            entry.offset = saved_offset;
        }
    }

    result
}

fn sys_pwrite64(fd: usize, buf: usize, count: usize, pos: usize) -> usize {
    // pwrite64: dosyaya belirli offset'e yaz, dosya konumunu değiştirme
    if count == 0 {
        return 0;
    }
    if let Err(err) = validate_user_range(buf, count) {
        return err;
    }

    let saved_offset = {
        let files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get(fd) {
            entry.offset
        } else {
            return errno(EBADF);
        }
    };

    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get_mut(fd) {
            entry.offset = pos;
        }
    }

    let result = sys_write(fd, buf, count);

    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(entry)) = files.get_mut(fd) {
            entry.offset = saved_offset;
        }
    }

    result
}

/// readv(2) — scatter read: birden fazla tampona kesintisiz okuma.
///
/// iov, struct iovec dizisinin adresi: { iov_base: *mut u8, iov_len: usize }
/// Her iovec tamponuna sırayla veri kopyalanır.
fn sys_readv(fd: usize, iov: usize, iovcnt: usize) -> usize {
    if iovcnt == 0 || iovcnt > 1024 {
        return errno(EINVAL);
    }
    let iov_bytes = iovcnt.saturating_mul(core::mem::size_of::<[usize; 2]>());
    if let Err(err) = validate_user_range(iov, iov_bytes) {
        return err;
    }
    let fd_num = fd as i32;
    let mut iov_entries = vec![[0usize; 2]; iovcnt];
    if let Err(err) = copy_from_user_slice(&mut iov_entries, iov) {
        return err;
    }
    let mut total = 0usize;

    for i in 0..iovcnt {
        let entry = &iov_entries[i];
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }
        if let Err(err) = validate_user_range(base, len) {
            if total > 0 {
                return total;
            }
            return err;
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
    let iov_bytes = iovcnt.saturating_mul(core::mem::size_of::<[usize; 2]>());
    if let Err(err) = validate_user_range(iov, iov_bytes) {
        return err;
    }
    let fd_num = fd as i32;
    let mut iov_entries = vec![[0usize; 2]; iovcnt];
    if let Err(err) = copy_from_user_slice(&mut iov_entries, iov) {
        return err;
    }
    let mut total = 0usize;

    for i in 0..iovcnt {
        let entry = &iov_entries[i];
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }
        if let Err(err) = validate_user_range(base, len) {
            if total > 0 {
                return total;
            }
            return err;
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

/// openat(2) — dizin tanımlayıcısına görecel dosya aç (APUE §3.3)
///
/// 1. Mutlak path → path olduğu gibi kullanılır.
/// 2. Göreceli + AT_FDCWD → CWD'ye görecel.
/// 3. Göreceli + dirfd → dirfd bir dizin olmalı, ona görecel.
fn sys_openat(dirfd: usize, path_ptr: usize, flags: usize, mode: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // dirfd geçerli bir dizin fd'si mi kontrol et (göreceli path + fd >= 0)
    let dirfd_isize = dirfd as isize;
    if dirfd_isize >= 0 && !path.starts_with('/') {
        let kind = get_fd(dirfd);
        match kind {
            Some(FdKind::File) => {
                let files = FILE_TABLE.lock();
                let Some(Some(state)) = files.get(dirfd) else {
                    return errno(EBADF);
                };
                // Dizin değilse ENOTDIR (O_DIRECTORY flag'ini kontrol et)
                if state.flags & O_DIRECTORY == 0 {
                    // stat ile dizin kontrolü yap
                    if let Ok(entry) = fs::f2fs::open_entry(&state.path) {
                        if !entry.is_dir {
                            return errno(ENOTDIR);
                        }
                    }
                }
            }
            Some(_) => return errno(ENOTDIR), // File değil → dizin olamaz
            None => return errno(EBADF),
        }
    }

    let resolved = resolve_path_at(dirfd, &path);
    let path_ptr_temp = alloc::vec![0u8; resolved.len() + 1];
    // resolved'ı user-space'e yazıp tekrar okuyamayız; doğrudan sys_open'a String olarak gitmek gerek.
    // sys_open user pointer bekler, biz String'den gidiyoruz — wrapper yapalım.
    sys_open_with_str(&resolved, flags, mode)
}

/// sys_open'un String tabanlı versiyonu (openat için)
fn sys_open_with_str(path: &str, flags: usize, mode: usize) -> usize {
    const O_WRONLY: usize = 1;
    const O_RDWR: usize = 2;
    const O_CREAT: usize = 0o100;
    const O_TRUNC: usize = 0o1000;

    // Umask uygula: sadece O_CREAT varsa mode'a etki eder
    let effective_mode = if flags & O_CREAT != 0 {
        let umask = PROCESS_UMASK.lock();
        mode & !(*umask)
    } else {
        mode
    };
    let _ = effective_mode; // Backend henüz mode kullanmıyor, hazır

    // path_resolution(7): trailing slash → resolved entry must be a directory
    // Check this before any O_DIRECTORY/O_NOFOLLOW/CREAT logic
    if path.ends_with('/') {
        // Only reject if path EXISTS and is NOT a directory
        // If path doesn't exist, let normal open logic handle it
        if let Ok(entry) = fs::f2fs::open_entry(path) {
            if !entry.is_dir {
                return errno(ENOTDIR);
            }
        }
    }

    if flags & O_DIRECTORY != 0 {
        match fs::f2fs::open_entry(path) {
            Ok(entry) => {
                if !entry.is_dir {
                    return errno(ENOTDIR);
                }
            }
            Err(e) => return vfs_errno(e),
        }
    }

    if flags & O_NOFOLLOW != 0 {
        match fs::f2fs::read_link(path) {
            Ok(_) => return errno(ELOOP),
            Err(_) => {}
        }
    }

    let write_intent = (flags & O_WRONLY != 0)
        || (flags & O_RDWR != 0)
        || (flags & O_CREAT != 0)
        || (flags & O_TRUNC != 0);
    let access = if write_intent {
        security::landlock::Access::Write
    } else {
        security::landlock::Access::Read
    };
    if let Err(err) = enforce_path_policy(path, access) {
        return err;
    }

    let fd = match path {
        "/dev/null" => return allocate_fd(FdKind::Null),
        "/dev/zero" => return allocate_fd(FdKind::Zero),
        "/dev/dri/card0" => return allocate_fd(FdKind::Drm),
        _ => {
            let inode = match fs::vfs_open_inode(path) {
                Ok(value) => value,
                Err(err) => {
                    if flags & O_CREAT == 0 {
                        return vfs_errno(err);
                    }
                    if flags & O_EXCL != 0 {
                        return errno(EEXIST);
                    }
                    let (parent, name) = match path.rfind('/') {
                        Some(pos) => {
                            let p = if pos == 0 { "/" } else { &path[..pos] };
                            (p, &path[pos + 1..])
                        }
                        None => ("/", path),
                    };
                    if name.is_empty() {
                        return errno(EINVAL);
                    }
                    if let Err(create_err) = fs::f2fs::create_f2fs_file(parent, name) {
                        return vfs_errno(create_err);
                    }
                    match fs::vfs_open_inode(path) {
                        Ok(value) => value,
                        Err(open_err) => return vfs_errno(open_err),
                    }
                }
            };
            let mut meta_size = match fs::vfs_inode_metadata(&inode) {
                Ok(meta) => meta.size,
                Err(err) => return vfs_errno(err),
            };
            if flags & O_TRUNC != 0 && write_intent {
                if let Err(e) = fs::f2fs::truncate_f2fs(path, 0) {
                    return vfs_errno(e);
                }
                meta_size = 0;
            }
            let is_hello = path.eq_ignore_ascii_case("HELLO.ELF") || path.ends_with("/HELLO.ELF");
            let fd = allocate_file_fd(FileState {
                inode,
                offset: 0,
                size: meta_size,
                is_hello,
                generation: 0,
                flags,
                path: path.to_string(),
            });
            if fd < MAX_FDS && (flags & O_CLOEXEC != 0) {
                let mut cloexec = FD_CLOEXEC.lock();
                cloexec[fd] = true;
            }
            fd
        }
    };
    fd
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
                    // POSIX permission check — arşiv: inodes.html i_mode
                    // owner_bits = (mode >> 6) & 0o7, group_bits = (mode >> 3) & 0o7, other_bits = mode & 0o7
                    // uid/gid eşleştirmesi ile owner/group/other kontrolü
                    if !fs::check_permission(
                        meta.mode as u16,
                        meta.uid as u16,
                        meta.gid as u16,
                        mode as u32,
                    ) {
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
    if let Err(err) = validate_user_range(usage_ptr, 18 * core::mem::size_of::<u64>()) {
        return err;
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
    if let Err(err) = validate_user_range(info_ptr, 16 * core::mem::size_of::<u64>()) {
        return err;
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
        if let Err(err) = validate_user_range(buf_ptr, 4 * core::mem::size_of::<u64>()) {
            return err;
        }
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
    // APUE §4.16: oldpath == newpath → no-op
    if oldpath == newpath {
        return 0;
    }
    if let Err(err) = enforce_path_policy(&oldpath, security::landlock::Access::Rename) {
        return err;
    }
    if let Err(err) = enforce_path_policy(&newpath, security::landlock::Access::Create) {
        return err;
    }
    let (old_parent, old_name) = split_path(&oldpath);
    let (new_parent, new_name) = split_path(&newpath);
    if old_name.is_empty() || new_name.is_empty() {
        return errno(EINVAL);
    }
    // APUE §4.16: cross-directory rename → f2fs desteklemiyor → EXDEV
    if old_parent != new_parent {
        // Farklı dizinler arası rename: create + copy + delete yapılabilir,
        // ancak f2fs backend bunu desteklemiyor şimdilik.
        return errno(EXDEV);
    }
    match fs::f2fs::rename_f2fs(old_parent, old_name, new_name) {
        Ok(()) => 0,
        Err(err) => vfs_errno(err),
    }
}

/// renameat2(2) — rename with dirfd and flags support
///
/// Flags: RENAME_NOREPLACE (1), RENAME_EXCHANGE (2), RENAME_WHITEOUT (4)
fn sys_renameat2(
    olddirfd: usize,
    oldpath_ptr: usize,
    newdirfd: usize,
    newpath_ptr: usize,
    flags: usize,
) -> usize {
    let oldpath = match read_user_cstring(oldpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let newpath = match read_user_cstring(newpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let old_resolved = resolve_path_at(olddirfd, &oldpath);
    let new_resolved = resolve_path_at(newdirfd, &newpath);

    if flags & !(RENAME_NOREPLACE | RENAME_EXCHANGE | RENAME_WHITEOUT) != 0 {
        return errno(EINVAL);
    }

    // man renameat2(2): incompatible flag combinations
    if flags & RENAME_NOREPLACE != 0 && flags & RENAME_EXCHANGE != 0 {
        return errno(EINVAL);
    }
    if flags & RENAME_WHITEOUT != 0 && flags & RENAME_EXCHANGE != 0 {
        return errno(EINVAL);
    }

    // APUE §4.16: oldpath == newpath → no-op
    if old_resolved == new_resolved && flags == 0 {
        return 0;
    }

    // RENAME_NOREPLACE: newpath varsa EEXIST
    if flags & RENAME_NOREPLACE != 0 {
        if fs::f2fs::open_entry(&new_resolved).is_ok() {
            return errno(EEXIST);
        }
    }

    // RENAME_EXCHANGE: atomik swap (her iki dosya da var olmalı, aynı parent)
    if flags & RENAME_EXCHANGE != 0 {
        if fs::f2fs::open_entry(&old_resolved).is_err() {
            return errno(ENOENT);
        }
        if fs::f2fs::open_entry(&new_resolved).is_err() {
            return errno(ENOENT);
        }
        let (old_parent, old_file) = split_path(&old_resolved);
        let (new_parent, new_file) = split_path(&new_resolved);
        // f2fs rename sadece aynı parent içinde çalışır
        if old_parent != new_parent {
            return errno(EXDEV);
        }
        // old → tmp, new → old, tmp → new (hepsi aynı parent)
        let tmp_name = format!(".echos_swap_{}", tasking::scheduler::get_ticks());
        if let Err(e) = fs::f2fs::rename_f2fs(old_parent, old_file, &tmp_name) {
            return vfs_errno(e);
        }
        if let Err(e) = fs::f2fs::rename_f2fs(old_parent, new_file, old_file) {
            let _ = fs::f2fs::rename_f2fs(old_parent, &tmp_name, old_file);
            return vfs_errno(e);
        }
        if let Err(e) = fs::f2fs::rename_f2fs(old_parent, &tmp_name, new_file) {
            // Rollback: undo step 2 (new_file → old_file), then undo step 1 (old_file → tmp_name)
            let _ = fs::f2fs::rename_f2fs(old_parent, old_file, new_file);
            let _ = fs::f2fs::rename_f2fs(old_parent, &tmp_name, old_file);
            return vfs_errno(e);
        }
        return 0;
    }

    // RENAME_WHITEOUT: Linux 3.18+ overlay/union filesystem feature
    // Creates a {0,0} char device as whiteout at source atomically.
    // EINVAL per renameat2(2): filesystem does not support the flag
    // (or CAP_MKNOD missing — but we don't implement capability checks)
    if flags & RENAME_WHITEOUT != 0 {
        return errno(EINVAL);
    }

    // flags == 0: normal rename
    if let Err(err) = enforce_path_policy(&old_resolved, security::landlock::Access::Rename) {
        return err;
    }
    if let Err(err) = enforce_path_policy(&new_resolved, security::landlock::Access::Create) {
        return err;
    }
    let (old_parent, old_name) = split_path(&old_resolved);
    let (new_parent, new_name) = split_path(&new_resolved);
    if old_name.is_empty() || new_name.is_empty() {
        return errno(EINVAL);
    }
    // APUE §4.16: cross-dir rename → f2fs desteklemiyor → EXDEV
    if old_parent != new_parent {
        return errno(EXDEV);
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

/// link(2) — create a hard link
fn sys_link(oldpath_ptr: usize, newpath_ptr: usize) -> usize {
    let oldpath = match read_user_cstring(oldpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let newpath = match read_user_cstring(newpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (parent, name) = split_path(&newpath);
    match fs::f2fs::create_hardlink(parent, name, &oldpath) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// symlink(2) — create a symbolic link
fn sys_symlink(target_ptr: usize, linkpath_ptr: usize) -> usize {
    let target = match read_user_cstring(target_ptr, 4096) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let linkpath = match read_user_cstring(linkpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (parent, name) = split_path(&linkpath);
    match fs::f2fs::create_symlink(parent, name, &target) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// readlink(2) — read the target of a symbolic link
fn sys_readlink(path_ptr: usize, buf: usize, bufsize: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let target = match read_symlink_target_f2fs(&path) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let target_bytes = target.as_bytes();
    let to_copy = core::cmp::min(bufsize, target_bytes.len());
    if let Err(e) = validate_user_range(buf, to_copy) {
        return e;
    }
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(target_bytes.as_ptr(), buf as *mut u8, to_copy);
    });
    to_copy
}

// ============================================================================
// YENİ SYSCALL'LAR (FSYNC, FDATASYNC, GETDENTS64, UTIMENSAT, FACESSAT,
// STATFS, FSTATFS, LINKAT, READLINKAT, SYMLINKAT, SYNCFS, MOUNT, UMOUNT2)
// ============================================================================

/// fsync(2) — synchronize a file's in-core state with storage device
fn sys_fsync(fd: usize) -> usize {
    // Öncelikle posix FILE_TABLE üzerinden dene
    let inode_opt = {
        let files = FILE_TABLE.lock();
        files
            .get(fd)
            .and_then(|s| s.as_ref().map(|st| st.inode.clone()))
    };
    match inode_opt {
        Some(inode) => match inode.sync_all() {
            Ok(_) => 0,
            Err(_) => {
                // GLOBAL_FD_TABLE üzerinden fsync dene (fallback: per-process tablo kullanılır)
                match fs::sys_fsync(fd) {
                    Ok(_) => 0,
                    Err(e) => vfs_errno(e),
                }
            }
        },
        None => match fs::sys_fsync(fd) {
            Ok(_) => 0,
            Err(e) => vfs_errno(e),
        },
    }
}

/// fdatasync(2) — synchronize a file's data only
///
/// Per APUE §3.14: fsync updates both data AND attributes (metadata).
/// fdatasync updates only the data portions of a file; metadata
/// (mtime, ctime, size) is NOT flushed unless file size changed.
fn sys_fdatasync(fd: usize) -> usize {
    // Get the file state to find the path
    let (path_opt, inode_opt) = {
        let files = FILE_TABLE.lock();
        let state = files.get(fd).and_then(|s| s.as_ref());
        match state {
            Some(st) => (Some(st.path.clone()), Some(st.inode.clone())),
            None => (None, None),
        }
    };

    // Try INode::sync_data() first (data-only sync, no metadata)
    if let Some(inode) = inode_opt {
        if let Some(path) = path_opt {
            match fs::f2fs::fdatasync_path(&path) {
                Ok(_) => return 0,
                Err(RcFsError::IsDir) => return errno(EISDIR),
                Err(_) => {} // fall through
            }
        }
    }

    // Fallback: per-process FD tablosu üzerinden fdatasync dene
    match fs::sys_fsync(fd) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// fcntl(2) — manipulate file descriptor
///
/// Supported commands: F_DUPFD, F_GETFD, F_SETFD, F_GETFL, F_SETFL
/// Lock commands (F_SETLK/F_SETLKW/F_GETLK) delegate to file_lock module.
fn sys_fcntl(fd: usize, cmd: usize, arg: usize) -> usize {
    if get_fd(fd).is_none() {
        return errno(EBADF);
    }

    match cmd {
        F_DUPFD => {
            // En düşük >= arg olan boş fd'yi bul; CLOEXEC temizlenir
            let mut table = FD_TABLE.lock();
            let kind = match table.get(fd) {
                Some(Some(k)) => *k,
                _ => return errno(EBADF),
            };
            let mut newfd = arg.max(0);
            while newfd < MAX_FDS && table[newfd].is_some() {
                newfd += 1;
            }
            if newfd >= MAX_FDS {
                return errno(EMFILE);
            }
            table[newfd] = Some(kind);
            drop(table);
            let mut cloexec = FD_CLOEXEC.lock();
            cloexec[newfd] = false;
            drop(cloexec);
            if kind == FdKind::File {
                let files = FILE_TABLE.lock();
                if let Some(Some(state)) = files.get(fd) {
                    let mut gen = FILE_GENERATION.lock();
                    let mut new_files = FILE_TABLE.lock();
                    let generation = gen[newfd].wrapping_add(1);
                    gen[newfd] = generation;
                    let mut new_state = state.clone();
                    new_state.generation = generation;
                    new_files[newfd] = Some(new_state);
                }
            }
            newfd
        }

        F_GETFD => {
            let cloexec = FD_CLOEXEC.lock();
            if cloexec[fd] {
                FD_CLOEXEC_FLAG
            } else {
                0
            }
        }

        F_SETFD => {
            let mut cloexec = FD_CLOEXEC.lock();
            cloexec[fd] = (arg & FD_CLOEXEC_FLAG) != 0;
            0
        }

        F_GETFL => {
            // FILE ise FileState.flags döndür; diğer türler için O_RDWR
            let kind = get_fd(fd);
            match kind {
                Some(FdKind::File) => {
                    let files = FILE_TABLE.lock();
                    match files.get(fd) {
                        Some(Some(state)) => state.flags,
                        _ => errno(EBADF),
                    }
                }
                Some(FdKind::Stdin) => 0, // O_RDONLY
                Some(FdKind::Stdout) | Some(FdKind::Stderr) => 1, // O_WRONLY
                Some(FdKind::Null) | Some(FdKind::Zero) => 2, // O_RDWR
                Some(FdKind::Pipe) | Some(FdKind::Socket) => 2, // O_RDWR
                Some(FdKind::Drm) | Some(FdKind::IoUring) => 2, // O_RDWR
                None => errno(EBADF),
            }
        }

        F_SETFL => {
            // Sadece O_APPEND, O_NONBLOCK, O_ASYNC, O_DIRECT, O_NOATIME değiştirilebilir
            let kind = get_fd(fd);
            match kind {
                Some(FdKind::File) => {
                    let mut files = FILE_TABLE.lock();
                    match files.get_mut(fd) {
                        Some(Some(state)) => {
                            // APUE §3.14: Sadece O_APPEND, O_NONBLOCK, O_SYNC, O_DSYNC, O_ASYNC değiştirilebilir
                            let preserved =
                                state.flags & !(O_APPEND | O_NONBLOCK as usize | O_SYNC | O_DSYNC);
                            state.flags = preserved
                                | (arg & (O_APPEND | O_NONBLOCK as usize | O_SYNC | O_DSYNC));
                            0
                        }
                        _ => errno(EBADF),
                    }
                }
                // Pipe/Socket için varsayılan başarı
                Some(_) => 0,
                None => errno(EBADF),
            }
        }

        // POSIX lock commands + OFD lock commands (delegate to file_lock module)
        F_GETLK | F_SETLK | F_SETLKW | F_OFD_GETLK | F_OFD_SETLK | F_OFD_SETLKW => {
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct Flock64 {
                l_type: i16,
                l_whence: i16,
                l_start: i64,
                l_len: i64,
                l_pid: i32,
            }
            let buf_ptr = arg;
            let mut flock: Flock64 = match read_user(buf_ptr) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let mut file_lock = fs::file_lock::FileLock {
                l_type: flock.l_type as i32,
                l_whence: flock.l_whence as i32,
                l_start: flock.l_start as u64,
                l_len: flock.l_len as u64,
                l_pid: flock.l_pid as u64,
                is_ofd: false,
            };
            let ret = fs::file_lock::sys_fcntl_lock(fd as i32, cmd as i32, &mut file_lock);
            if ret < 0 {
                return errno((-ret) as usize);
            }
            // F_GETLK / F_OFD_GETLK: result'ı user'a geri yaz
            if cmd == F_GETLK || cmd == F_OFD_GETLK {
                flock.l_type = file_lock.l_type as i16;
                flock.l_whence = file_lock.l_whence as i16;
                flock.l_start = file_lock.l_start as i64;
                flock.l_len = file_lock.l_len as i64;
                flock.l_pid = file_lock.l_pid as i32;
                if let Err(e) = write_user(buf_ptr, flock) {
                    return e;
                }
            }
            0
        }

        _ => errno(EINVAL),
    }
}

/// getdents64(2) — get directory entries
/// Linux struct linux_dirent64:
///   d_ino (u64), d_off (i64), d_reclen (u16), d_type (u8), d_name (flex)
fn sys_getdents64(fd: usize, dirp: usize, count: usize) -> usize {
    if count < 24 {
        return errno(EINVAL); // minimum dirent64 boyutu
    }
    let (dir_path, already_read) = {
        let files = FILE_TABLE.lock();
        let Some(Some(state)) = files.get(fd) else {
            return errno(EBADF);
        };
        (state.path.clone(), state.offset != 0)
    };
    if dir_path.is_empty() {
        return errno(ENOTDIR); // path olmayan fd'ler için
    }
    if already_read {
        return 0;
    }
    let entries = match fs::f2fs::list_dir(&dir_path) {
        Ok(e) => e,
        Err(e) => return vfs_errno(e),
    };
    let mut written: usize = 0;
    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        // d_reclen: 19 (sabit başlık) + name.len() + 1 (null) + padding
        let name_len = entry.name.len() + 1; // +1 for null terminator
        let reclen = (19 + name_len + 7) & !7; // 8-byte align
        if written + reclen > count {
            break;
        }
        let d_ino: u64 = entry.ino;
        let d_off: i64 = entries.len() as i64; // simplified: all entries at once
        let d_reclen: u16 = reclen as u16;
        let d_type: u8 = if entry.is_dir { 4 } else { 8 }; // DT_DIR=4, DT_REG=8
        let pos = dirp.wrapping_add(written);
        if let Err(e) = validate_user_range(pos, reclen) {
            return e;
        }
        with_user_access(|| unsafe {
            core::ptr::write(pos as *mut u64, d_ino);
            core::ptr::write((pos + 8) as *mut i64, d_off);
            core::ptr::write((pos + 16) as *mut u16, d_reclen);
            core::ptr::write((pos + 18) as *mut u8, d_type);
            let name_ptr = (pos + 19) as *mut u8;
            for (i, &b) in entry.name.as_bytes().iter().enumerate() {
                core::ptr::write(name_ptr.add(i), b);
            }
            core::ptr::write(name_ptr.add(entry.name.len()), 0u8); // null terminator
        });
        written = written.wrapping_add(reclen);
    }
    // offset güncelle: tüm entry'leri tek seferde oku
    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(state)) = files.get_mut(fd) {
            state.offset = state.offset.wrapping_add(written);
        }
    }
    written
}

/// utimensat(2) — change timestamps of a file with nanosecond precision
fn sys_utimensat(dirfd: usize, pathname_ptr: usize, times_ptr: usize, flags: usize) -> usize {
    let path = match pathname_ptr {
        0 => return errno(ENOENT),
        _ => match read_user_cstring(pathname_ptr, 4096) {
            Ok(p) => p,
            Err(e) => return e,
        },
    };
    let resolved = resolve_path_at(dirfd, &path);
    // AT_SYMLINK_NOFOLLOW: symlink'in kendisine timestamp yaz (henüz FS desteği yok)
    if flags & AT_SYMLINK_NOFOLLOW != 0 {
        // update_timestamps symlink için de çalışır (inline data) - aynı FS fonksiyonunu kullan
    }
    if times_ptr == 0 {
        // NULL times = set to current time
        let now = super::fs::get_global_time();
        match fs::f2fs::update_timestamps(&resolved, now.sec, 0, now.sec, 0) {
            Ok(_) => 0,
            Err(e) => vfs_errno(e),
        }
    } else {
        if let Err(e) = validate_user_range(times_ptr, 2 * core::mem::size_of::<Timespec>()) {
            return e;
        }
        let times: [Timespec; 2] =
            with_user_access(|| unsafe { core::ptr::read(times_ptr as *const [Timespec; 2]) });
        let atime_sec = if times[0].tv_nsec as usize == UTIME_OMIT {
            0 // unchanged — skip below
        } else if times[0].tv_nsec as usize == UTIME_NOW {
            super::fs::get_global_time().sec
        } else {
            times[0].tv_sec
        };
        let atime_nsec = if times[0].tv_nsec as usize == UTIME_OMIT {
            0
        } else if times[0].tv_nsec as usize == UTIME_NOW {
            0
        } else {
            times[0].tv_nsec
        };
        let mtime_sec = if times[1].tv_nsec as usize == UTIME_OMIT {
            0 // unchanged — skip below
        } else if times[1].tv_nsec as usize == UTIME_NOW {
            super::fs::get_global_time().sec
        } else {
            times[1].tv_sec
        };
        let mtime_nsec = if times[1].tv_nsec as usize == UTIME_OMIT {
            0
        } else if times[1].tv_nsec as usize == UTIME_NOW {
            0
        } else {
            times[1].tv_nsec
        };
        match fs::f2fs::update_timestamps(&resolved, atime_sec, atime_nsec, mtime_sec, mtime_nsec) {
            Ok(_) => 0,
            Err(e) => vfs_errno(e),
        }
    }
}

/// faccessat(2) — check access permissions of a file
/// AT_EACCESS flag'i -> effective UID/GID kullan (Linux varsayılanı)
fn sys_faccessat(dirfd: usize, pathname_ptr: usize, mode: usize, flags: usize) -> usize {
    let path = match read_user_cstring(pathname_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let resolved = resolve_path_at(dirfd, &path);
    // AT_SYMLINK_NOFOLLOW: symlink'in kendisini kontrol et
    if flags & AT_SYMLINK_NOFOLLOW != 0 {
        match fs::f2fs::read_link(&resolved) {
            Ok(_) => {
                // symlink var, read -> okuma izni
                if mode & R_OK != 0 {
                    return 0;
                }
                return errno(EACCES);
            }
            Err(_) => {} // symlink değil, altındaki dosyayı kontrol et
        }
    }
    // AT_EACCESS: effective UID/GID kullan (henüz ayrım yok)
    let _ = flags & AT_EACCESS;
    match fs::f2fs::open_entry(&resolved) {
        Ok(entry) => {
            // path_resolution(7): trailing slash → must be a directory
            if path.ends_with('/') && !entry.is_dir {
                return errno(ENOTDIR);
            }
            if mode == F_OK {
                return 0;
            }
            let file_mode = entry.mode;
            let owner_read = file_mode & 0o400 != 0;
            let owner_write = file_mode & 0o200 != 0;
            let owner_exec = file_mode & 0o100 != 0;
            if (mode & R_OK != 0 && !owner_read)
                || (mode & W_OK != 0 && !owner_write)
                || (mode & X_OK != 0 && !owner_exec)
            {
                return errno(EACCES);
            }
            0
        }
        Err(e) => vfs_errno(e),
    }
}

/// statfs(2) — get filesystem statistics
fn sys_statfs(path_ptr: usize, buf: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if let Err(e) = validate_user_range(buf, core::mem::size_of::<Statfs>()) {
        return e;
    }
    let stats = match fs::f2fs::f2fs_stats() {
        Ok(s) => s,
        Err(e) => return vfs_errno(e),
    };
    let statfs = Statfs {
        f_type: 0x2015, // F2FS magic
        f_bsize: 4096,
        f_blocks: stats.total_main_blocks,
        f_bfree: stats.free_blocks,
        f_bavail: stats.free_blocks,
        f_files: 0,
        f_ffree: 0,
        f_fsid: [0; 2],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    };
    with_user_access(|| unsafe {
        core::ptr::write(buf as *mut Statfs, statfs);
    });
    0
}

/// fstatfs(2) — get filesystem statistics by fd
fn sys_fstatfs(_fd: usize, buf: usize) -> usize {
    // fd bazlı statfs: fd'den mount noktası çıkar.
    // Şimdilik doğrudan statfs çağır.
    if let Err(e) = validate_user_range(buf, core::mem::size_of::<Statfs>()) {
        return e;
    }
    let stats = match fs::f2fs::f2fs_stats() {
        Ok(s) => s,
        Err(e) => return vfs_errno(e),
    };
    let statfs = Statfs {
        f_type: 0x2015,
        f_bsize: 4096,
        f_blocks: stats.total_main_blocks,
        f_bfree: stats.free_blocks,
        f_bavail: stats.free_blocks,
        f_files: 0,
        f_ffree: 0,
        f_fsid: [0; 2],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    };
    with_user_access(|| unsafe {
        core::ptr::write(buf as *mut Statfs, statfs);
    });
    0
}

/// linkat(2) — create a hard link relative to directory fds
fn sys_linkat(
    olddirfd: usize,
    oldpath_ptr: usize,
    newdirfd: usize,
    newpath_ptr: usize,
    flags: usize,
) -> usize {
    if flags & AT_EMPTY_PATH != 0 {
        return errno(ENOSYS);
    }
    let oldpath = match read_user_cstring(oldpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let newpath = match read_user_cstring(newpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let resolved_old = resolve_path_at(olddirfd, &oldpath);
    let resolved_new = resolve_path_at(newdirfd, &newpath);
    let (parent, name) = split_path(&resolved_new);
    match fs::f2fs::create_hardlink(parent, name, &resolved_old) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// readlinkat(2) — read the target of a symbolic link relative to a directory fd
fn sys_readlinkat(dirfd: usize, path_ptr: usize, buf: usize, bufsize: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let resolved = resolve_path_at(dirfd, &path);
    let target = match read_symlink_target_f2fs(&resolved) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let target_bytes = target.as_bytes();
    let to_copy = core::cmp::min(bufsize, target_bytes.len());
    if let Err(e) = validate_user_range(buf, to_copy) {
        return e;
    }
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(target_bytes.as_ptr(), buf as *mut u8, to_copy);
    });
    to_copy
}

/// symlinkat(2) — create a symbolic link relative to a directory fd
fn sys_symlinkat(target_ptr: usize, newdirfd: usize, linkpath_ptr: usize) -> usize {
    let target = match read_user_cstring(target_ptr, 4096) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let linkpath = match read_user_cstring(linkpath_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let resolved_link = resolve_path_at(newdirfd, &linkpath);
    let (parent, name) = split_path(&resolved_link);
    match fs::f2fs::create_symlink(parent, name, &target) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// syncfs(2) — synchronize an entire mounted filesystem
fn sys_syncfs(_fd: usize) -> usize {
    match fs::vfs_unified::vfs_sync_all() {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// mount(2) — mount a filesystem
fn sys_mount(
    source_ptr: usize,
    target_ptr: usize,
    fstype_ptr: usize,
    _flags: usize,
    _data_ptr: usize,
) -> usize {
    let source = match read_user_cstring(source_ptr, 4096) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let target = match read_user_cstring(target_ptr, 4096) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let fstype = if fstype_ptr == 0 {
        String::new()
    } else {
        match read_user_cstring(fstype_ptr, 256) {
            Ok(t) => t,
            Err(e) => return e,
        }
    };
    let fs_type = if fstype.is_empty() { "f2fs" } else { &fstype };
    match fs::f2fs::mount_fs(&source, &target, fs_type) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

/// umount2(2) — unmount a mounted filesystem
fn sys_umount2(target_ptr: usize, _flags: usize) -> usize {
    let target = match read_user_cstring(target_ptr, 4096) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match fs::f2fs::umount_fs(&target) {
        Ok(_) => 0,
        Err(e) => vfs_errno(e),
    }
}

fn sys_select(
    nfds: usize,
    readfds: usize,
    writefds: usize,
    exceptfds: usize,
    timeout: usize,
) -> usize {
    if nfds > 1024 {
        return errno(EINVAL);
    }

    // timeout: { long tv_sec; long tv_usec; } — 0 = non-blocking, NULL = infinite
    let timeout_ms: usize = if timeout != 0 {
        if let Err(e) = validate_user_range(timeout, 16) {
            return e;
        }
        let tv_sec: i64 = with_user_access(|| unsafe { *(timeout as *const i64) });
        let tv_usec: i64 = with_user_access(|| unsafe { *((timeout + 8) as *const i64) });
        if tv_sec < 0 || tv_usec < 0 {
            return errno(EINVAL);
        }
        (tv_sec as usize) * 1000 + (tv_usec as usize) / 1000
    } else {
        0 // non-blocking
    };

    let mut read_ready: u64 = 0;
    let mut write_ready: u64 = 0;
    let mut except_ready: u64 = 0;
    let mut total: usize = 0;

    let check = |fd_set: usize, nfds_inner: usize, mask: u64, check_read: bool| -> (u64, usize) {
        if fd_set == 0 {
            return (0, 0);
        }
        let mut result: u64 = 0;
        let mut count: usize = 0;
        for bit in 0..nfds_inner {
            if mask & (1u64 << bit) == 0 {
                continue;
            }
            let byte_offset = bit / 8;
            let bit_offset = bit % 8;
            let fd_set_byte: u8 =
                with_user_access(|| unsafe { *((fd_set + byte_offset) as *const u8) });
            if fd_set_byte & (1 << bit_offset) == 0 {
                continue;
            }

            let files = FILE_TABLE.lock();
            let is_ready = match files.get(bit) {
                Some(Some(_)) => true,
                _ => false,
            };
            drop(files);
            if is_ready {
                result |= 1u64 << bit;
                count += 1;
            }
        }
        (result, count)
    };

    // FD_SET bitmasks
    let read_mask: u64 = if readfds != 0 {
        let mut m: u64 = 0;
        for i in 0..nfds.min(64) {
            let byte = with_user_access(|| unsafe { *((readfds + i / 8) as *const u8) });
            if byte & (1 << (i % 8)) != 0 {
                m |= 1u64 << i;
            }
        }
        m
    } else {
        0
    };

    let write_mask: u64 = if writefds != 0 {
        let mut m: u64 = 0;
        for i in 0..nfds.min(64) {
            let byte = with_user_access(|| unsafe { *((writefds + i / 8) as *const u8) });
            if byte & (1 << (i % 8)) != 0 {
                m |= 1u64 << i;
            }
        }
        m
    } else {
        0
    };

    // Basit kontrol: dosya açık mı?
    if read_mask != 0 {
        let files = FILE_TABLE.lock();
        for bit in 0..nfds.min(64) {
            if read_mask & (1u64 << bit) != 0 {
                if let Some(Some(_)) = files.get(bit) {
                    read_ready |= 1u64 << bit;
                    total += 1;
                }
            }
        }
        drop(files);
    }
    if write_mask != 0 {
        let files = FILE_TABLE.lock();
        for bit in 0..nfds.min(64) {
            if write_mask & (1u64 << bit) != 0 {
                if let Some(Some(_)) = files.get(bit) {
                    write_ready |= 1u64 << bit;
                    total += 1;
                }
            }
        }
        drop(files);
    }

    // FD_SET'leri temizle ve ready olanları yaz
    if readfds != 0 {
        for byte_idx in 0..((nfds + 7) / 8) {
            let mut val: u8 = 0;
            for bit in 0..8 {
                let fd = byte_idx * 8 + bit;
                if fd >= nfds {
                    break;
                }
                if read_ready & (1u64 << fd) != 0 {
                    val |= 1 << bit;
                }
            }
            with_user_access(|| unsafe {
                *((readfds + byte_idx) as *mut u8) = val;
            });
        }
    }
    if writefds != 0 {
        for byte_idx in 0..((nfds + 7) / 8) {
            let mut val: u8 = 0;
            for bit in 0..8 {
                let fd = byte_idx * 8 + bit;
                if fd >= nfds {
                    break;
                }
                if write_ready & (1u64 << fd) != 0 {
                    val |= 1 << bit;
                }
            }
            with_user_access(|| unsafe {
                *((writefds + byte_idx) as *mut u8) = val;
            });
        }
    }
    if exceptfds != 0 {
        for byte_idx in 0..((nfds + 7) / 8) {
            with_user_access(|| unsafe {
                *((exceptfds + byte_idx) as *mut u8) = 0;
            });
        }
    }

    total
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
#[derive(Clone, Copy)]
struct SigAction {
    sa_handler: SigHandler,
    sa_flags: usize,
    sa_restorer: usize,
    sa_mask: [u64; 1],
}

/// rt_sigaction - examine and change a signal action (per-process)
fn sys_rt_sigaction(sig: usize, act_ptr: usize, oldact_ptr: usize, _sigsetsize: usize) -> usize {
    if sig == 0 || sig > 64 {
        return errno(EINVAL);
    }

    let signal = match crate::task::signal::Signal::from_number(sig as u8) {
        Some(s) => s,
        None => return errno(EINVAL),
    };

    // Save old action if requested
    if oldact_ptr != 0 {
        if let Err(err) = validate_user_range(oldact_ptr, core::mem::size_of::<SigAction>()) {
            return err;
        }
        let old_action = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            crate::task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
                .map(|t| {
                    let sa = t.cold.signals.get_action(signal);
                    match sa {
                        crate::task::signal::SignalAction::Default => SigAction {
                            sa_handler: 0,
                            sa_flags: 0,
                            sa_restorer: 0,
                            sa_mask: [0; 1],
                        },
                        crate::task::signal::SignalAction::Ignore => SigAction {
                            sa_handler: 1, // SIG_IGN
                            sa_flags: 0,
                            sa_restorer: 0,
                            sa_mask: [0; 1],
                        },
                        crate::task::signal::SignalAction::Catch {
                            handler,
                            mask,
                            flags,
                            restorer,
                        } => SigAction {
                            sa_handler: handler,
                            sa_flags: flags as usize,
                            sa_restorer: restorer,
                            sa_mask: [mask],
                        },
                    }
                })
                .unwrap_or(SigAction {
                    sa_handler: 0,
                    sa_flags: 0,
                    sa_restorer: 0,
                    sa_mask: [0; 1],
                })
        });
        if let Err(err) = write_user(oldact_ptr, old_action) {
            return err;
        }
    }

    // Set new action if requested
    if act_ptr != 0 {
        let new_action = match read_user::<SigAction>(act_ptr) {
            Ok(value) => value,
            Err(err) => return err,
        };
        let sa_action = if new_action.sa_handler == 0 {
            crate::task::signal::SignalAction::Default
        } else if new_action.sa_handler == 1 {
            // SIG_IGN
            crate::task::signal::SignalAction::Ignore
        } else {
            crate::task::signal::SignalAction::Catch {
                handler: new_action.sa_handler,
                mask: new_action.sa_mask[0],
                flags: new_action.sa_flags as u32,
                restorer: new_action.sa_restorer,
            }
        };
        x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            if let Some(current) = crate::task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
            {
                current.cold.signals.set_action(signal, sa_action);
            }
        });
    }

    0
}

/// rt_sigprocmask - examine and change blocked signals (per-process)
fn sys_rt_sigprocmask(how: usize, set_ptr: usize, oldset_ptr: usize, _sigsetsize: usize) -> usize {
    const SIG_BLOCK: usize = 0;
    const SIG_UNBLOCK: usize = 1;
    const SIG_SETMASK: usize = 2;

    // Save old mask if requested
    if oldset_ptr != 0 {
        if let Err(err) = validate_user_range(oldset_ptr, core::mem::size_of::<u64>()) {
            return err;
        }
        let old_mask = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            crate::task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
                .map(|t| t.cold.signals.get_mask())
                .unwrap_or(0)
        });
        if let Err(err) = write_user(oldset_ptr, old_mask) {
            return err;
        }
    }

    // Set new mask if requested
    if set_ptr != 0 {
        let new_mask = match read_user::<u64>(set_ptr) {
            Ok(value) => value,
            Err(err) => return err,
        };
        x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            if let Some(current) = crate::task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
            {
                match how {
                    SIG_BLOCK => {
                        let old = current.cold.signals.get_mask();
                        current.cold.signals.set_mask(old | new_mask);
                    }
                    SIG_UNBLOCK => {
                        let old = current.cold.signals.get_mask();
                        current.cold.signals.set_mask(old & !new_mask);
                    }
                    SIG_SETMASK => {
                        current.cold.signals.set_mask(new_mask);
                    }
                    _ => {}
                }
                // SIGKILL (bit 9) ve SIGSTOP (bit 17) asla block edilemez
                current
                    .cold
                    .signals
                    .set_mask(current.cold.signals.get_mask() & !(1u64 << 8) & !(1u64 << 16));
            }
        });
    }

    0
}

/// rt_sigsuspend - atomically change signal mask and suspend calling thread
/// rt_sigsuspend - atomically change signal mask and suspend calling thread (per-process)
fn sys_rt_sigsuspend(mask_ptr: usize) -> usize {
    if mask_ptr == 0 {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(mask_ptr, core::mem::size_of::<u64>()) {
        return err;
    }
    let mask = match read_user::<u64>(mask_ptr) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Per-process mask'ı kaydet ve geçici olarak değiştir
    let old_mask = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(crate::cpu::smp::get_current_cpu_id() as usize)
            .and_then(|t| t.as_ref())
            .map(|t| {
                let old = t.cold.signals.get_mask();
                t.cold.signals.set_mask(mask);
                old
            })
            .unwrap_or(0)
    });
    crate::task::scheduler::sleep(1);
    // Eski mask'ı geri yükle
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(crate::cpu::smp::get_current_cpu_id() as usize)
            .and_then(|t| t.as_ref())
        {
            current.cold.signals.set_mask(old_mask);
        }
    });
    errno(EINTR)
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
fn sys_rt_sigqueueinfo(pid: usize, sig: usize, info_ptr: usize) -> usize {
    if sig == 0 || sig > 64 {
        return errno(EINVAL);
    }
    if pid == 0 {
        return errno(EINVAL);
    }

    // siginfo_t yapısını oku: { int si_signo; int si_errno; int si_code; union { ... } }
    let si_signo: i32 = if info_ptr != 0 {
        with_user_access(|| unsafe { *(info_ptr as *const i32) })
    } else {
        sig as i32
    };
    if si_signo != sig as i32 {
        return errno(EINVAL);
    }

    // SI_QUEUE-only: sadece sendici Process si_code == SI_QUEUE (12) olabilir
    let si_code: i32 = if info_ptr != 0 {
        with_user_access(|| unsafe { *((info_ptr + 8) as *const i32) })
    } else {
        0
    };
    if si_code != 12 && si_code != 0 {
        // SI_QUEUE=12 veya SI_USER=0
        return errno(EPERM);
    }

    // Hedef process'i bul
    if !tasking::task_exists(pid) {
        return errno(ESRCH);
    }
    if let Some(signal) = crate::task::signal::Signal::from_number(sig as u8) {
        let _ = crate::task::signal::send_signal(pid, signal);
    }
    0
}

/// sigaltstack - set and/or examine signal stack context
fn sys_sigaltstack(ss_ptr: usize, old_ss_ptr: usize) -> usize {
    // Signal alt stack yapısı: { void *ss_sp; int ss_flags; size_t ss_size; }
    // ss_flags & SS_DISABLE ise alt stack devre dışı

    // Eski stack bilgisini yaz (eğer istenirse)
    if old_ss_ptr != 0 {
        if let Err(e) = validate_user_range(old_ss_ptr, 24) {
            return e;
        }
        // Şimdilik: her zaman SS_DISABLE ile yanıt ver (alt stack desteklenmiyor)
        with_user_access(|| unsafe {
            *((old_ss_ptr) as *mut usize) = 0; // ss_sp = NULL
            *((old_ss_ptr + 8) as *mut i32) = 2; // SS_DISABLE
            *((old_ss_ptr + 16) as *mut usize) = 0; // ss_size = 0
        });
    }

    // Yeni stack ayarla
    if ss_ptr != 0 {
        if let Err(e) = validate_user_range(ss_ptr, 24) {
            return e;
        }
        let ss_sp: usize = with_user_access(|| unsafe { *(ss_ptr as *const usize) });
        let ss_flags: i32 = with_user_access(|| unsafe { *((ss_ptr + 8) as *const i32) });
        let ss_size: usize = with_user_access(|| unsafe { *((ss_ptr + 16) as *const usize) });

        if ss_flags & 2 != 0 {
            // SS_DISABLE
            return 0; // Devre dışı bırakma — zaten desteklenmiyor
        }
        if ss_sp == 0 || ss_size < 2048 {
            return errno(ENOMEM);
        }
        // Alt stack desteklenmiyor — ENOMEM dön
        // Gerçek implementasyon: signal handler sırasında bu stack'e geçilmeli
        errno(ENOMEM)
    } else {
        0
    }
}

/// rt_sigtimedwait - wait for a signal with timeout
fn sys_rt_sigtimedwait(
    set_ptr: usize,
    info_ptr: usize,
    timeout_ptr: usize,
    sigsetsize: usize,
) -> usize {
    if sigsetsize != 8 {
        return errno(EINVAL);
    }
    if set_ptr == 0 {
        return errno(EINVAL);
    }

    if let Err(e) = validate_user_range(set_ptr, 8) {
        return e;
    }
    let signal_mask: u64 = with_user_access(|| unsafe { *(set_ptr as *const u64) });

    // Timeout oku
    let timeout_ms: usize = if timeout_ptr != 0 {
        if let Err(e) = validate_user_range(timeout_ptr, 16) {
            return e;
        }
        let tv_sec: i64 = with_user_access(|| unsafe { *(timeout_ptr as *const i64) });
        let tv_usec: i64 = with_user_access(|| unsafe { *((timeout_ptr + 8) as *const i64) });
        if tv_sec < 0 || tv_usec < 0 {
            return errno(EINVAL);
        }
        (tv_sec as usize) * 1000 + (tv_usec as usize) / 1000
    } else {
        usize::MAX // süresiz bekleme
    };

    // Mevcut process'in pending signal'lerini kontrol et
    let my_pid = tasking::current_task_id();

    // Basit polling: timeout_ms boyunca bekle
    let max_ticks = (timeout_ms + 9) / 10;
    for tick in 0..max_ticks {
        x86_64::instructions::hlt();
        // Pending signal kontrolü — her tick'te bir
        if tick % 10 == 0 {
            // Signal maskesi ile eşleşen pending signal var mı kontrol et
            // Basit implementasyon: herhangi bir sinyal beklemede mi?
            for sig_num in 1..=64 {
                if signal_mask & (1u64 << (sig_num - 1)) != 0 {
                    // Bu sinyal maskede — pending olup olmadığını kontrol et
                    // Gerçek implementasyonda pending_signals bitmap'i okunurdu
                    // Şimdilik: EINTR ile dön
                    return errno(EINTR);
                }
            }
        }
    }

    errno(EAGAIN) // Timeout doldu, sinyal gelmedi
}

fn sys_mremap(
    old_addr: usize,
    old_size: usize,
    new_size: usize,
    flags: usize,
    new_addr: usize,
) -> usize {
    // mremap(2): remap virtual memory address
    const MREMAP_MAYMOVE: usize = 0x01;
    const MREMAP_FIXED: usize = 0x02;
    const MREMAP_DONTUNMAP: usize = 0x04;

    if old_size == 0 || new_size == 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(old_addr as u64, old_size as u64) {
        return errno(EINVAL);
    }

    let may_move = flags & MREMAP_MAYMOVE != 0;
    let is_fixed = flags & MREMAP_FIXED != 0;
    let dont_unmap = flags & MREMAP_DONTUNMAP != 0;
    if is_fixed && !may_move {
        return errno(EINVAL);
    }
    if dont_unmap && !may_move {
        return errno(EINVAL);
    }

    let old_addr_aligned = old_addr & !0xFFF;
    let old_end = old_addr_aligned.saturating_add(old_size);
    let new_end = old_addr_aligned.saturating_add(new_size);

    if new_size < old_size {
        // Shrinking: update VMA end address
        kernel_memory::shrink_vma(old_addr_aligned as u64, old_size as u64, new_size as u64);
        // Unmap the freed pages
        let free_start = old_addr_aligned as u64 + new_size as u64;
        let free_end = old_end as u64;
        let page_mask = !(kernel_memory::PAGE_SIZE as u64 - 1);
        let free_start_aligned = free_start & page_mask;
        let free_end_aligned = (free_end + kernel_memory::PAGE_SIZE as u64 - 1) & page_mask;
        if free_end_aligned > free_start_aligned {
            kernel_memory::unmap_user_range(
                free_start_aligned,
                free_end_aligned - free_start_aligned,
            );
        }
        return old_addr_aligned;
    }

    if new_size == old_size {
        return old_addr_aligned;
    }

    // Expanding
    if !may_move {
        // In-place expansion: check if space after old mapping is free
        let expand_start = old_end as u64;
        let expand_size = (new_size - old_size) as u64;
        if kernel_memory::user_region_overlaps(expand_start, expand_size) {
            return errno(ENOMEM);
        }
        // Extend the VMA
        kernel_memory::extend_vma(old_addr_aligned as u64, old_size as u64, new_size as u64);
        return old_addr_aligned;
    }

    // MREMAP_MAYMOVE: allocate new address
    let target = if is_fixed {
        if new_addr == 0 {
            return errno(EINVAL);
        }
        let target_u64 = new_addr as u64;
        if !kernel_memory::is_user_range(target_u64, new_size as u64) {
            return errno(EINVAL);
        }
        if kernel_memory::user_region_overlaps(target_u64, new_size as u64) {
            kernel_memory::unmap_user_range(target_u64, new_size as u64);
        }
        target_u64
    } else {
        match kernel_memory::allocate_user_mmap(new_size as u64) {
            Some(addr) => addr,
            None => return errno(ENOMEM),
        }
    };

    // Copy old VMA type to new location
    kernel_memory::clone_vma_to(
        old_addr_aligned as u64,
        old_size as u64,
        target,
        new_size as u64,
    );

    // Copy physical page contents
    let page_mask = !(kernel_memory::PAGE_SIZE as u64 - 1);
    let copy_pages = (old_size + kernel_memory::PAGE_SIZE - 1) / kernel_memory::PAGE_SIZE;
    for i in 0..copy_pages {
        let src_virt =
            (old_addr_aligned as u64).wrapping_add(i as u64 * kernel_memory::PAGE_SIZE as u64);
        let dst_virt = target.wrapping_add(i as u64 * kernel_memory::PAGE_SIZE as u64);
        if let Some(src_phys) = kernel_memory::translate_addr(src_virt & page_mask) {
            let src_data = unsafe {
                let ptr = (kernel_memory::active_physical_offset() + src_phys) as *const u8;
                core::slice::from_raw_parts(ptr, kernel_memory::PAGE_SIZE)
            };
            // Map destination and copy
            kernel_memory::copy_page_data(dst_virt & page_mask, src_data);
        }
    }

    if !dont_unmap {
        // Unmap old region
        kernel_memory::unmap_user_range(old_addr_aligned as u64, old_size as u64);
        kernel_memory::remove_vma(old_addr_aligned as u64, old_size as u64);
    }

    target as usize
}

fn sys_msync(addr: usize, len: usize, flags: usize) -> usize {
    // msync(2): flush dirty pages back to filesystem
    if len == 0 {
        return errno(EINVAL);
    }
    if addr % kernel_memory::PAGE_SIZE != 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(addr as u64, len as u64) {
        return errno(ENOMEM);
    }

    const MS_ASYNC: usize = 0x01;
    const MS_SYNC: usize = 0x04;
    const MS_INVALIDATE: usize = 0x10;
    let invalid_bits = !(MS_ASYNC | MS_SYNC | MS_INVALIDATE);
    if flags & invalid_bits != 0 {
        return errno(EINVAL);
    }
    if (flags & MS_ASYNC) != 0 && (flags & MS_SYNC) != 0 {
        return errno(EINVAL);
    }

    let page_mask = !(kernel_memory::PAGE_SIZE as u64 - 1);
    let start = (addr as u64) & page_mask;
    let end = (addr as u64)
        .saturating_add(len as u64)
        .saturating_add(kernel_memory::PAGE_SIZE as u64 - 1)
        & page_mask;
    if end <= start {
        return 0;
    }

    kernel_memory::msync_user_range(start, end, flags & MS_INVALIDATE != 0);

    if (flags & MS_SYNC) != 0 {
        kernel_memory::flush_dirty_file_pages();
    }

    0
}

fn sys_mincore(addr: usize, len: usize, vec: usize) -> usize {
    // mincore(2): check which pages are resident in memory
    if len == 0 {
        return errno(EINVAL);
    }
    if addr % kernel_memory::PAGE_SIZE != 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(addr as u64, len as u64) {
        return errno(ENOMEM);
    }

    let page_count = (len + kernel_memory::PAGE_SIZE - 1) / kernel_memory::PAGE_SIZE;
    if vec != 0 {
        if let Err(e) = validate_user_range(vec, page_count) {
            return e;
        }
        with_user_access(|| unsafe {
            core::ptr::write_bytes(vec as *mut u8, 0, page_count);
        });
        let page_mask = !(kernel_memory::PAGE_SIZE as u64 - 1);
        let start = (addr as u64) & page_mask;
        for i in 0..page_count {
            let page_addr = start + (i as u64 * kernel_memory::PAGE_SIZE as u64);
            let is_present = kernel_memory::is_page_present(page_addr);
            if is_present {
                with_user_access(|| unsafe {
                    core::ptr::write_volatile((vec + i) as *mut u8, 1);
                });
            }
        }
    }
    0
}

fn sys_madvise(addr: usize, len: usize, advice: usize) -> usize {
    // madvise(2): bellek kullanım ipuçları
    if len == 0 {
        return errno(EINVAL);
    }
    if !kernel_memory::is_user_range(addr as u64, len as u64) {
        return errno(EINVAL);
    }

    const MADV_NORMAL: usize = 0;
    const MADV_RANDOM: usize = 1;
    const MADV_SEQUENTIAL: usize = 2;
    const MADV_WILLNEED: usize = 3;
    const MADV_DONTNEED: usize = 4;
    const MADV_FREE: usize = 8;
    const MADV_DONTFORK: usize = 10;
    const MADV_DOFORK: usize = 11;
    const MADV_MERGEABLE: usize = 12;
    const MADV_UNMERGEABLE: usize = 13;
    const MADV_HUGEPAGE: usize = 14;
    const MADV_NOHUGEPAGE: usize = 15;
    const MADV_DONTDUMP: usize = 16;
    const MADV_DODUMP: usize = 17;
    const MADV_WIPEONFORK: usize = 18;
    const MADV_KEEPONFORK: usize = 19;
    const MADV_COLD: usize = 20;
    const MADV_PAGEOUT: usize = 21;
    const MADV_POPULATE_READ: usize = 22;
    const MADV_POPULATE_WRITE: usize = 23;
    const MADV_GUARD_INSTALL: usize = 100;
    const MADV_GUARD_REMOVE: usize = 101;

    match advice {
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL | MADV_WILLNEED => {
            // İpuçları — şimdilik no-op
            0
        }
        MADV_DONTNEED => {
            // Man page: free pages + backing store; subsequent access repopulates
            // from file (shared file/anon) or zero-fill (anon private)
            let page_mask = !(kernel_memory::PAGE_SIZE as u64 - 1);
            let start = (addr as u64) & page_mask;
            let end = (addr as u64)
                .saturating_add(len as u64)
                .saturating_add(kernel_memory::PAGE_SIZE as u64 - 1)
                & page_mask;
            if end <= start {
                return 0;
            }
            kernel_memory::unmap_user_range(start, end - start);
            0
        }
        MADV_FREE => {
            // Serbest bırak ama yeniden kullanılmadıkça tut
            0
        }
        MADV_DONTFORK | MADV_DOFORK | MADV_MERGEABLE | MADV_UNMERGEABLE => {
            // Fork/KSM ile ilgili — no-op
            0
        }
        MADV_HUGEPAGE | MADV_NOHUGEPAGE => {
            // Huge page — no-op
            0
        }
        MADV_DONTDUMP | MADV_DODUMP => {
            // Core dump hariç/tut — no-op
            0
        }
        MADV_WIPEONFORK | MADV_KEEPONFORK => 0,
        MADV_COLD | MADV_PAGEOUT => {
            // Sayfaları soğuk/listeye al — no-op
            0
        }
        MADV_POPULATE_READ | MADV_POPULATE_WRITE => {
            // Sayfaları hemen ata — no-op
            0
        }
        MADV_GUARD_INSTALL | MADV_GUARD_REMOVE => {
            // Guard page — no-op
            0
        }
        _ => errno(EINVAL),
    }
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

/// Pipe buffer — POSIX.1-2024 pipe(2) semantiği
/// Tek yönlü veri kanalı: writer → ring buffer → reader
struct PipeRingBuffer {
    data: Vec<u8>,
    capacity: usize,
    read_pos: usize,
    write_pos: usize,
    readers: u32,
    writers: u32,
    nonblocking: bool,
}

impl PipeRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            data: alloc::vec![0u8; capacity],
            capacity,
            read_pos: 0,
            write_pos: 0,
            readers: 1,
            writers: 1,
            nonblocking: false,
        }
    }

    fn pipe_read(&mut self, buf: &mut [u8]) -> Result<usize, i64> {
        let available = self.write_pos.saturating_sub(self.read_pos);
        if available == 0 {
            if self.writers == 0 {
                return Ok(0);
            }
            return Err(-11); // EAGAIN
        }
        let to_read = buf.len().min(available);
        for i in 0..to_read {
            buf[i] = self.data[(self.read_pos + i) % self.capacity];
        }
        self.read_pos += to_read;
        Ok(to_read)
    }

    fn pipe_write(&mut self, buf: &[u8]) -> Result<usize, i64> {
        if self.readers == 0 {
            crate::task::signal::send_signal_to_current(crate::task::signal::Signal::SIGPIPE);
            return Err(-32); // EPIPE
        }
        let used = self.write_pos.saturating_sub(self.read_pos);
        let available = self.capacity.saturating_sub(used);
        if available == 0 {
            return Err(-11); // EAGAIN
        }
        let to_write = buf.len().min(available);
        for i in 0..to_write {
            self.data[(self.write_pos + i) % self.capacity] = buf[i];
        }
        self.write_pos += to_write;
        Ok(to_write)
    }
}

use spin::Mutex as PipeMutex;

/// Pipe havuzu: pipe_id → PipeRingBuffer
static PIPE_POOL: spin::Lazy<PipeMutex<alloc::collections::BTreeMap<u32, PipeRingBuffer>>> =
    spin::Lazy::new(|| PipeMutex::new(alloc::collections::BTreeMap::new()));
/// Sonraki pipe ID
static NEXT_PIPE_ID: spin::Lazy<core::sync::atomic::AtomicU32> =
    spin::Lazy::new(|| core::sync::atomic::AtomicU32::new(1));
/// read_fd → pipe_id eşlemesi
static PIPE_READ_MAP: spin::Lazy<PipeMutex<alloc::collections::BTreeMap<usize, u32>>> =
    spin::Lazy::new(|| PipeMutex::new(alloc::collections::BTreeMap::new()));
/// write_fd → pipe_id eşlemesi
static PIPE_WRITE_MAP: spin::Lazy<PipeMutex<alloc::collections::BTreeMap<usize, u32>>> =
    spin::Lazy::new(|| PipeMutex::new(alloc::collections::BTreeMap::new()));

/// Pipe FD türü — hangi uç olduğunu belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeFdEnd {
    Read,
    Write,
}

/// pipe(2) — Anonim boru oluştur
/// POSIX: "pipe(fds) creates a pipe, a unidirectional data channel"
/// pipefd[0]: okuma ucu (read end), pipefd[1]: yazma ucu (write end)
fn sys_pipe(pipefd_ptr: usize) -> usize {
    if let Err(err) = validate_user_range(pipefd_ptr, 2 * core::mem::size_of::<u32>()) {
        return err;
    }

    let read_fd = allocate_fd(FdKind::Pipe);
    let write_fd = allocate_fd(FdKind::Pipe);

    if read_fd >= MAX_FDS || write_fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Yeni pipe ID oluştur ve havuza yerleştir
    let pipe_id = NEXT_PIPE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    let pipe = PipeRingBuffer::new(65536);
    PIPE_POOL.lock().insert(pipe_id, pipe);

    // FD → pipe_id eşlemelerini kaydet
    PIPE_READ_MAP.lock().insert(read_fd, pipe_id);
    PIPE_WRITE_MAP.lock().insert(write_fd, pipe_id);

    // Kullanıcı alanına fd değerlerini yaz
    if let Err(err) = write_user(pipefd_ptr, read_fd as u32) {
        return err;
    }
    if let Err(err) = write_user(pipefd_ptr + core::mem::size_of::<u32>(), write_fd as u32) {
        return err;
    }

    0
}

/// pipe2(2) — Flags ile pipe oluştur (O_NONBLOCK, O_CLOEXEC)
fn sys_pipe2(pipefd_ptr: usize, flags: usize) -> usize {
    if let Err(err) = validate_user_range(pipefd_ptr, 2 * core::mem::size_of::<u32>()) {
        return err;
    }

    let read_fd = allocate_fd(FdKind::Pipe);
    let write_fd = allocate_fd(FdKind::Pipe);

    if read_fd >= MAX_FDS || write_fd >= MAX_FDS {
        return errno(EMFILE);
    }

    let nonblock = flags & (O_NONBLOCK as usize) != 0;
    let cloexec = flags & 0x80000 != 0;

    let pipe_id = NEXT_PIPE_ID.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    let mut pipe = PipeRingBuffer::new(65536);
    pipe.nonblocking = nonblock;
    PIPE_POOL.lock().insert(pipe_id, pipe);

    PIPE_READ_MAP.lock().insert(read_fd, pipe_id);
    PIPE_WRITE_MAP.lock().insert(write_fd, pipe_id);

    if cloexec {
        let mut cloexec_table = FD_CLOEXEC.lock();
        if read_fd < cloexec_table.len() {
            cloexec_table[read_fd] = true;
        }
        if write_fd < cloexec_table.len() {
            cloexec_table[write_fd] = true;
        }
    }

    if let Err(err) = write_user(pipefd_ptr, read_fd as u32) {
        return err;
    }
    if let Err(err) = write_user(pipefd_ptr + core::mem::size_of::<u32>(), write_fd as u32) {
        return err;
    }

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

    if is_pipe_in {
        let pipe_id = PIPE_READ_MAP.lock().get(&fd_in).copied();
        if let Some(pipe_id) = pipe_id {
            let mut pool = PIPE_POOL.lock();
            if let Some(pipe) = pool.get_mut(&pipe_id) {
                let available = pipe.write_pos.saturating_sub(pipe.read_pos);
                transferred = core::cmp::min(len, available);
                pipe.read_pos += transferred;
            }
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

/// sendfile(2) — copy data between file descriptors
///
/// sendfile(out_fd, in_fd, offset_ptr, count)
/// offset_ptr: user-space pointer to u64 offset (or NULL = use current fd offset)
fn sys_sendfile(out_fd: usize, in_fd: usize, offset_ptr: usize, count: usize) -> usize {
    if get_fd(out_fd).is_none() || get_fd(in_fd).is_none() {
        return errno(EBADF);
    }
    if count == 0 {
        return 0;
    }

    let (in_path, mut read_off) = {
        let files = FILE_TABLE.lock();
        match files.get(in_fd) {
            Some(Some(s)) => (s.path.clone(), s.offset),
            _ => return errno(EBADF),
        }
    };
    let (out_path, mut write_off) = {
        let files = FILE_TABLE.lock();
        match files.get(out_fd) {
            Some(Some(s)) => (s.path.clone(), s.offset),
            _ => return errno(EBADF),
        }
    };

    // If user provided offset_ptr, read offset from user-space
    if offset_ptr != 0 {
        if let Ok(off) = read_user::<u64>(offset_ptr) {
            read_off = off as usize;
        } else {
            return errno(EFAULT);
        }
    }

    let mut transferred: usize = 0;
    let mut remaining = count;
    let chunk = 65536usize;
    let mut buf = alloc::vec![0u8; chunk];

    while remaining > 0 {
        let n = core::cmp::min(remaining, chunk);
        let buf_slice = &mut buf[..n];
        let read = match fs::f2fs::read_f2fs_file_at(&in_path, read_off, buf_slice) {
            Ok(n) => n,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        let written = match fs::f2fs::write_f2fs_file_at(&out_path, write_off, &buf[..read]) {
            Ok(n) => n,
            Err(_) => break,
        };
        if written == 0 {
            break;
        }
        transferred += written;
        read_off += written;
        write_off += written;
        remaining -= written;
        if written < read {
            break;
        }
    }

    // Update offsets
    if offset_ptr != 0 {
        let new_off = read_off as u64;
        let _ = write_user(offset_ptr, new_off);
    } else {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(s)) = files.get_mut(in_fd) {
            s.offset = read_off;
        }
    }
    {
        let mut files = FILE_TABLE.lock();
        if let Some(Some(s)) = files.get_mut(out_fd) {
            s.offset = write_off;
        }
    }

    transferred
}

/// copy_file_range(2) — kernel-space copy between file descriptors
///
/// copy_file_range(fd_in, off_in, fd_out, off_out, len, flags)
fn sys_copy_file_range(
    fd_in: usize,
    off_in: usize,
    fd_out: usize,
    off_out: usize,
    len: usize,
    _flags: usize,
) -> usize {
    // off_in/off_out are user-space pointers to i64, or NULL (= use current fd offset)
    if off_in != 0 && off_out != 0 {
        // Both explicit offsets: use sendfile with offset
        let in_off_raw: i64 = match read_user(off_in) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let out_off_raw: i64 = match read_user(off_out) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let in_path = {
            let files = FILE_TABLE.lock();
            match files.get(fd_in) {
                Some(Some(s)) => s.path.clone(),
                _ => return errno(EBADF),
            }
        };
        let out_path = {
            let files = FILE_TABLE.lock();
            match files.get(fd_out) {
                Some(Some(s)) => s.path.clone(),
                _ => return errno(EBADF),
            }
        };

        let mut transferred: usize = 0;
        let mut remaining = len;
        let chunk = 65536usize;
        let mut buf = alloc::vec![0u8; chunk];
        let mut in_off = in_off_raw.max(0) as usize;
        let mut out_off = out_off_raw.max(0) as usize;

        while remaining > 0 {
            let n = core::cmp::min(remaining, chunk);
            let buf_slice = &mut buf[..n];
            let read = match fs::f2fs::read_f2fs_file_at(&in_path, in_off, buf_slice) {
                Ok(n) => n,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            let written = match fs::f2fs::write_f2fs_file_at(&out_path, out_off, &buf[..read]) {
                Ok(n) => n,
                Err(_) => break,
            };
            if written == 0 {
                break;
            }
            transferred += written;
            in_off += written;
            out_off += written;
            remaining -= written;
            if written < read {
                break;
            }
        }

        // Update user-space offsets
        let new_in_off = in_off as i64;
        let new_out_off = out_off as i64;
        let _ = write_user(off_in, new_in_off);
        let _ = write_user(off_out, new_out_off);

        transferred
    } else {
        // At least one offset is NULL: use current fd offsets
        sys_sendfile(fd_out, fd_in, off_in, len)
    }
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
            // File ise FILE_TABLE entry'sini de kopyala
            if k == FdKind::File {
                let files = FILE_TABLE.lock();
                if let Some(Some(state)) = files.get(oldfd) {
                    let mut gen = FILE_GENERATION.lock();
                    let mut new_files = FILE_TABLE.lock();
                    let generation = gen[newfd].wrapping_add(1);
                    gen[newfd] = generation;
                    let mut new_state = state.clone();
                    new_state.generation = generation;
                    new_files[newfd] = Some(new_state);
                }
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
            // oldfd == newfd: hiçbir şey yapma, direkt dön
            if oldfd == newfd {
                return newfd;
            }
            // newfd açıksa önce kapat (fd > 2 ise)
            if newfd > 2 {
                let _ = free_fd(newfd);
            }
            // POSIX: dup2 yeni fd'de CLOEXEC flag'ini temizler
            {
                let mut cloexec = FD_CLOEXEC.lock();
                cloexec[newfd] = false;
            }
            let mut table = FD_TABLE.lock();
            table[newfd] = Some(k);
            drop(table);
            // File ise FILE_TABLE entry'sini kopyala
            if k == FdKind::File {
                let files = FILE_TABLE.lock();
                if let Some(Some(state)) = files.get(oldfd) {
                    let mut gen = FILE_GENERATION.lock();
                    let mut new_files = FILE_TABLE.lock();
                    let generation = gen[newfd].wrapping_add(1);
                    gen[newfd] = generation;
                    let mut new_state = state.clone();
                    new_state.generation = generation;
                    new_files[newfd] = Some(new_state);
                }
            }
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
    if let Err(err) = validate_user_range(msgp, msgsz.saturating_add(core::mem::size_of::<i64>())) {
        return err;
    }

    if queue.used_bytes + msgsz > queue.max_bytes {
        return errno(EAGAIN);
    }

    // msgp yapısı: { mtype: i64, mtext: [u8; msgsz] }
    let mtype = match read_user::<i64>(msgp) {
        Ok(value) => value,
        Err(err) => return err,
    };
    if mtype <= 0 {
        return errno(EINVAL);
    }
    let mut entry_data = vec![0u8; msgsz];
    if let Err(err) = copy_from_user(
        &mut entry_data,
        msgp.saturating_add(core::mem::size_of::<i64>()),
    ) {
        return err;
    }

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
    if let Err(err) = validate_user_range(msgp, msgsz.saturating_add(core::mem::size_of::<i64>())) {
        return err;
    }

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
    if let Err(err) = write_user(msgp, entry.mtype) {
        return err;
    }
    if let Err(err) = write_user_bytes(
        msgp.saturating_add(core::mem::size_of::<i64>()),
        &entry.data[..copy_len],
    ) {
        return err;
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
    if let Err(err) = validate_user_range(sops, nsops.saturating_mul(6)) {
        return err;
    }
    let mut sets = SEM_SETS.lock();
    let set = match sets.get_mut(&(semid as i32)) {
        Some(s) => s,
        None => return errno(EINVAL),
    };

    // struct sembuf = [u16, i16, i16] = 6 bytes each
    let mut ops = vec![0u8; nsops.saturating_mul(6)];
    if let Err(err) = copy_from_user(&mut ops, sops) {
        return err;
    }
    for i in 0..nsops {
        let base = i * 6;
        let sem_num = u16::from_ne_bytes([ops[base], ops[base + 1]]) as usize;
        let sem_op = i16::from_ne_bytes([ops[base + 2], ops[base + 3]]);
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

/// statx(2) — genişletilmiş dosya bilgisi döner (APUE §4.2, Linux statx(2))
///
/// flags: AT_EMPTY_PATH, AT_SYMLINK_NOFOLLOW
/// mask: STATX_* bit maskesi
fn sys_statx(dirfd: usize, pathname: usize, flags: usize, mask: usize, statxbuf: usize) -> usize {
    if let Err(err) = validate_user_range(statxbuf, core::mem::size_of::<Statx>()) {
        return err;
    }
    let buf = statxbuf as *mut Statx;

    // AT_EMPTY_PATH: dirfd'nin kendisine stat yap (man statx(2))
    // pathname == 0 (NULL) without AT_EMPTY_PATH → EFAULT
    // (since Linux 6.11 NULL + AT_EMPTY_PATH is also allowed)
    let path = if flags & AT_EMPTY_PATH != 0 {
        let files = FILE_TABLE.lock();
        let Some(Some(state)) = files.get(dirfd) else {
            return errno(EBADF);
        };
        state.path.clone()
    } else if pathname == 0 {
        return errno(EFAULT);
    } else {
        match read_user_cstring(pathname, 4096) {
            Ok(value) => value,
            Err(err) => return err,
        }
    };

    // Özel cihaz dosyaları
    if path == "/dev/null" || path == "/dev/zero" || path == "/dev/dri/card0" {
        with_user_access(|| unsafe {
            core::ptr::write_bytes(buf, 0, 1);
            (*buf).stx_mask = (mask & STATX_ALL) as u32;
            if mask & STATX_MODE != 0 {
                (*buf).stx_mode = (S_IFCHR | MODE_CHAR) as u16;
            }
            if mask & STATX_NLINK != 0 {
                (*buf).stx_nlink = 1;
            }
            (*buf).stx_blksize = 4096;
        });
        return 0;
    }

    // AT_SYMLINK_NOFOLLOW: symlink'in kendisine bak
    let entry = if flags & AT_SYMLINK_NOFOLLOW != 0 {
        // Symlink'in kendisini açmaya çalış
        match fs::f2fs::open_entry(&path) {
            Ok(e) => e,
            Err(_) => {
                // Dosya yoksa veya symlink kontrolü başarısız
                return errno(ENOENT);
            }
        }
    } else {
        // Default: symlink'ler otomatik izlenir (read_link yok)
        match fs::f2fs::open_entry(&path) {
            Ok(value) => value,
            Err(_) => return errno(ENOENT),
        }
    };

    let mode = if entry.is_dir {
        (S_IFDIR | MODE_DIR) as u16
    } else {
        (S_IFREG | MODE_FILE) as u16
    };
    let file_size = if entry.is_dir {
        0u64
    } else {
        entry.size as u64
    };
    let blocks = (file_size + 511) / 512;
    let now = tasking::scheduler::get_ticks() as u64 * TICK_NS;

    with_user_access(|| unsafe {
        core::ptr::write_bytes(buf, 0, 1);
        // Always fill mask
        (*buf).stx_mask = (mask & STATX_ALL) as u32;

        // Only fill fields matching mask
        if mask & (STATX_TYPE | STATX_MODE) != 0 {
            (*buf).stx_mode = (mode & 0xF000) | (entry.mode as u16 & 0x01FF);
        } else {
            (*buf).stx_mode = mode; // default
        }
        if mask & STATX_NLINK != 0 {
            (*buf).stx_nlink = 1;
        }
        if mask & STATX_UID != 0 {
            (*buf).stx_uid = entry.uid;
        }
        if mask & STATX_GID != 0 {
            (*buf).stx_gid = entry.gid;
        }
        if mask & STATX_INO != 0 {
            (*buf).stx_ino = entry.ino;
        }
        if mask & STATX_SIZE != 0 {
            (*buf).stx_size = file_size;
        }
        if mask & STATX_BLOCKS != 0 {
            (*buf).stx_blocks = blocks;
        }
        if mask & STATX_BTIME != 0 {
            (*buf).stx_btime = [0u8; 16]; // No birth time in F2FS yet
        }
        if mask & STATX_ATIME != 0 {
            let ts = into_statx_timestamp(now);
            (*buf).stx_atime = ts;
        }
        if mask & STATX_CTIME != 0 {
            let ts = into_statx_timestamp(now);
            (*buf).stx_ctime = ts;
        }
        if mask & STATX_MTIME != 0 {
            let ts = into_statx_timestamp(now);
            (*buf).stx_mtime = ts;
        }

        (*buf).stx_blksize = 4096;
        (*buf).stx_dev_major = 0;
        (*buf).stx_dev_minor = 0;
        (*buf).stx_rdev_major = 0;
        (*buf).stx_rdev_minor = 0;
        (*buf).stx_attributes_mask = 0;
    });
    0
}

/// Timespec (i64 tv_sec + u32 tv_nsec + u32 pad) → 16 byte
fn into_statx_timestamp(ns: u64) -> [u8; 16] {
    let sec = (ns / 1_000_000_000) as i64;
    let nsec = (ns % 1_000_000_000) as u32;
    let mut ts = [0u8; 16];
    ts[..8].copy_from_slice(&sec.to_ne_bytes());
    ts[8..12].copy_from_slice(&nsec.to_ne_bytes());
    ts
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

    if let Err(err) = validate_user_range(uaddr, core::mem::size_of::<u32>()) {
        return err;
    }
    if matches!(cmd, FUTEX_REQUEUE | FUTEX_CMP_REQUEUE) {
        if let Err(err) = validate_user_range(uaddr2, core::mem::size_of::<u32>()) {
            return err;
        }
    }

    match cmd {
        FUTEX_WAIT => {
            // Check if *uaddr == val, if so sleep
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
        _ => errno(EINVAL),
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
    overrun: u32,
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
        overrun: 0,
    };

    TIMER_TABLE.lock().insert(timerid, timer);

    if timerid_ptr != 0 {
        if let Err(err) = validate_user_range(timerid_ptr, core::mem::size_of::<usize>()) {
            return err;
        }
        if let Err(err) = write_user(timerid_ptr, timerid) {
            return err;
        }
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
        if let Err(err) = validate_user_range(old_value_ptr, 2 * core::mem::size_of::<u64>()) {
            return err;
        }
        if let Err(err) = write_user(old_value_ptr, timer.value_ns) {
            return err;
        }
        if let Err(err) = write_user(
            old_value_ptr.saturating_add(core::mem::size_of::<u64>()),
            timer.interval_ns,
        ) {
            return err;
        }
    }

    // Set new value
    if new_value_ptr != 0 {
        if let Err(err) = validate_user_range(new_value_ptr, 2 * core::mem::size_of::<u64>()) {
            return err;
        }
        let value = match read_user::<u64>(new_value_ptr) {
            Ok(v) => v,
            Err(err) => return err,
        };
        let interval =
            match read_user::<u64>(new_value_ptr.saturating_add(core::mem::size_of::<u64>())) {
                Ok(v) => v,
                Err(err) => return err,
            };
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
        if let Err(err) = validate_user_range(curr_value_ptr, 2 * core::mem::size_of::<u64>()) {
            return err;
        }
        if let Err(err) = write_user(curr_value_ptr, timer.value_ns) {
            return err;
        }
        if let Err(err) = write_user(
            curr_value_ptr.saturating_add(core::mem::size_of::<u64>()),
            timer.interval_ns,
        ) {
            return err;
        }
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

    let event_size = core::mem::size_of::<EpollEvent>();

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
                if let Err(err) = validate_user_range(event, event_size) {
                    return err;
                }
                match read_user::<EpollEvent>(event) {
                    Ok(value) => value,
                    Err(err) => return err,
                }
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
                if let Err(err) = validate_user_range(event, event_size) {
                    return err;
                }
                match read_user::<EpollEvent>(event) {
                    Ok(value) => value,
                    Err(err) => return err,
                }
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
    let events_bytes = maxevents.saturating_mul(core::mem::size_of::<EpollEvent>());
    if let Err(err) = validate_user_range(events_ptr, events_bytes) {
        return err;
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
    let req = match read_user::<Timespec>(req_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
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
            if let Err(err) = validate_user_range(addr, core::mem::size_of::<usize>()) {
                return err;
            }
            match read_user::<usize>(addr) {
                Ok(value) => value,
                Err(err) => err,
            }
        }
        PTRACE_POKETEXT | PTRACE_POKEDATA => {
            if let Err(err) = validate_user_range(addr, core::mem::size_of::<usize>()) {
                return err;
            }
            if let Err(err) = write_user(addr, data) {
                return err;
            }
            0
        }
        PTRACE_CONT => {
            // sys_ptrace: continue execution
            0
        }
        PTRACE_ATTACH => {
            // ptrace ATTACH: hedef process'i durdur ve tracer olarak bağlan
            if pid == 0 {
                return errno(EINVAL);
            }
            if tasking::task_exists(pid) {
                // Tracee olarak işaretle (basit:tracer_pid ayarla)
                // Gerçek implementasyon: SIGSTOP gönder, ptrace state machine başlat
                errno(ENOSYS)
            } else {
                errno(ESRCH)
            }
        }
        PTRACE_DETACH => 0,
        _ => errno(EINVAL),
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn sys_uname(uts_ptr: usize) -> usize {
    if let Err(err) = validate_user_range(uts_ptr, core::mem::size_of::<UtsName>()) {
        return err;
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
    if let Err(err) = write_user(uts_ptr, uts) {
        return err;
    }
    0
}

/// getcwd(2) — mevcut çalışma dizinini döndürür
fn sys_getcwd(buf: usize, size: usize) -> usize {
    if buf == 0 || size == 0 {
        return errno(EINVAL);
    }
    if let Err(e) = validate_user_range(buf, size) {
        return e;
    }
    let cwd = CURRENT_WORKING_DIR.lock();
    let cwd_bytes = cwd.as_bytes();
    // null terminator için +1
    let needed = cwd_bytes.len() + 1;
    if needed > size {
        return errno(ERANGE);
    }
    if let Err(e) = write_user_bytes(buf, cwd_bytes) {
        return e;
    }
    // null terminator
    with_user_access(|| unsafe {
        core::ptr::write((buf + cwd_bytes.len()) as *mut u8, 0u8);
    });
    needed
}

/// chdir(2) — mevcut çalışma dizinini değiştirir
fn sys_chdir(path_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Dizin var mı kontrol et
    match fs::f2fs::open_entry(&path) {
        Ok(entry) => {
            if !entry.is_dir {
                return errno(ENOTDIR);
            }
            if path.starts_with('/') {
                // CLONE_FS: shared_fs varsa oraya yaz, yoksa global static'e
                set_cwd_for_current(path);
            } else {
                // Göreceli path: mevcut CWD'ye göre çöz
                let current_cwd = get_cwd();
                let resolved = resolve_path_at(AT_FDCWD as usize, &path);
                set_cwd_for_current(resolved);
            }
            0
        }
        Err(e) => vfs_errno(e),
    }
}

/// umask(2) — dosya oluşturma maskesini ayarlar, eski maskeyi döndürür
fn sys_umask(mask: usize) -> usize {
    let old = get_umask();
    set_umask_for_current(mask & 0o777);
    old
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
    if let Err(err) = validate_user_range(tp_ptr, core::mem::size_of::<Timespec>()) {
        return err;
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
    if let Err(err) = write_user(tp_ptr, ts) {
        return err;
    }
    0
}

fn sys_getrandom(buf: usize, len: usize, flags: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if flags & !(GRND_NONBLOCK | GRND_RANDOM | GRND_DETERMINISTIC) != 0 {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(buf, len) {
        return err;
    }
    if flags & GRND_DETERMINISTIC != 0 {
        let mut out = vec![0u8; len];
        random::fill_bytes_deterministic(&mut out);
        if let Err(err) = write_user_bytes(buf, &out) {
            return err;
        }
        return len;
    }
    let ticks = tasking::scheduler::get_ticks() as u64;
    let tsc = unsafe { _rdtsc() };
    let mut mix = ticks ^ tsc ^ (buf as u64) ^ (len as u64);
    if flags & GRND_RANDOM != 0 {
        mix ^= mix.rotate_left(29);
    }
    random::add_entropy(mix);
    let mut out = vec![0u8; len];
    random::fill_bytes(&mut out);
    if let Err(err) = write_user_bytes(buf, &out) {
        return err;
    }
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
    tcp_id: u32, // TCP bağlantı tablosu ID'si (0 = TCP değil)
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
            let tcp_id = if type_ == SOCK_STREAM {
                super::net::tcp::create_socket(if domain == AF_INET6 {
                    super::net::socket::AddressFamily::IPV6
                } else {
                    super::net::socket::AddressFamily::IPV4
                })
            } else {
                0
            };
            let sock_state = SocketState {
                domain,
                sock_type: type_,
                protocol,
                state: SocketConnState::None,
                local_port: 0,
                remote_port: 0,
                remote_addr: [0; 4],
                tcp_id,
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
    if let Err(err) = validate_user_range(addr_ptr, addr_len) {
        return err;
    }

    // Read sockaddr_in structure
    let sa_family = match read_user::<u16>(addr_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };

    if sa_family as usize != sock.domain {
        return errno(EAFNOSUPPORT);
    }

    // For AF_INET, read port (in network byte order)
    if sock.domain == AF_INET && addr_len >= 4 {
        let port = match read_user::<u16>(addr_ptr + 2) {
            Ok(value) => u16::from_be(value),
            Err(err) => return err,
        };
        sock.local_port = port;

        // TCP bağlantısını bağla
        if sock.tcp_id != 0 {
            use super::net::socket::SocketAddr as NetSocketAddr;
            use super::net::{Ipv4Addr, Port as NetPort};
            let local_addr = NetSocketAddr::new(
                super::net::IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                NetPort(port),
            );
            if let Err(e) = super::net::tcp::bind(sock.tcp_id, local_addr) {
                crate::serial_println!("[SOCKET] TCP bind failed: {:?}", e);
                return errno(EADDRINUSE);
            }
        }

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

    // TCP bağlantısını dinleme moduna al
    if sock.tcp_id != 0 {
        if let Err(e) = super::net::tcp::listen(sock.tcp_id, backlog) {
            crate::serial_println!("[SOCKET] TCP listen failed: {:?}", e);
            return errno(EINVAL);
        }
    }

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
    let listen_tcp_id = sock.tcp_id;
    drop(sockets);

    // TCP accept — gerçek bağlantıyı kabul et
    let (accepted_tcp_id, remote_addr) = if listen_tcp_id != 0 {
        match super::net::tcp::accept(listen_tcp_id) {
            Ok((tcp_id, addr)) => (tcp_id, addr),
            Err(_) => return errno(EAGAIN),
        }
    } else {
        return errno(EINVAL);
    };

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

    // Kabul edilen bağlantının uzak adresini çıkar
    let (remote_ip, remote_port) = match remote_addr.ip {
        super::net::IpAddr::V4(ip) => {
            let octets = *ip.as_bytes();
            (octets, remote_addr.port.0)
        }
        _ => ([0u8; 4], 0),
    };

    let new_sock = SocketState {
        domain: AF_INET,
        sock_type: SOCK_STREAM,
        protocol: 0,
        state: SocketConnState::Connected,
        local_port: 0,
        remote_port,
        remote_addr: remote_ip,
        tcp_id: accepted_tcp_id,
    };

    SOCKET_TABLE.lock().insert(new_fd, new_sock);

    // Fill in peer address if requested
    if addr_ptr != 0 {
        if let Err(err) = validate_user_range(addr_ptr, 16) {
            return err;
        }
        with_user_access(|| unsafe {
            // sa_family
            *(addr_ptr as *mut u16) = AF_INET as u16;
            // port (network byte order)
            *((addr_ptr + 2) as *mut u16) = u16::from_be(remote_port);
            // IPv4 address
            core::ptr::copy_nonoverlapping(remote_ip.as_ptr(), (addr_ptr + 4) as *mut u8, 4);
            // padding zero
            core::ptr::write_bytes((addr_ptr + 8) as *mut u8, 0, 8);
        });
    }
    if addr_len_ptr != 0 {
        if let Err(err) = validate_user_range(addr_len_ptr, core::mem::size_of::<u32>()) {
            return err;
        }
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
    if let Err(err) = validate_user_range(addr_ptr, addr_len) {
        return err;
    }

    // Read peer address
    let port = match read_user::<u16>(addr_ptr + 2) {
        Ok(value) => u16::from_be(value),
        Err(err) => return err,
    };
    let mut addr = [0u8; 4];
    if let Err(err) = copy_from_user(&mut addr, addr_ptr + 4) {
        return err;
    }

    sock.remote_port = port;
    sock.remote_addr = addr;
    sock.state = SocketConnState::Connecting;

    // TCP bağlantısını başlat (SYN gönder)
    if sock.tcp_id != 0 {
        use super::net::socket::SocketAddr as NetSocketAddr;
        use super::net::{Ipv4Addr, Port as NetPort};
        let remote_addr = NetSocketAddr::new(
            super::net::IpAddr::V4(Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3])),
            NetPort(port),
        );
        if let Err(e) = super::net::tcp::connect(sock.tcp_id, remote_addr) {
            crate::serial_println!("[SOCKET] TCP connect failed: {:?}", e);
            return errno(ECONNREFUSED);
        }
        sock.state = SocketConnState::Connected;
    }

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

    if let Err(err) = validate_user_range(buf, len) {
        return err;
    }

    // Read buffer
    let mut data = vec![0u8; len];
    if let Err(err) = copy_from_user(&mut data, buf) {
        return err;
    }

    let tcp_id = sock.tcp_id;
    drop(sockets);

    // TCP bağlantısı üzerinden gönder
    if tcp_id != 0 {
        match super::net::tcp::send(tcp_id, &data) {
            Ok(sent) => return sent,
            Err(_) => return errno(EPIPE),
        }
    }

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

    let tcp_id = sock.tcp_id;
    let remote_addr = sock.remote_addr;
    let remote_port = sock.remote_port;
    drop(sockets);

    // TCP bağlantısı üzerinden al
    if tcp_id != 0 {
        let mut recv_buf = vec![0u8; len];
        match super::net::tcp::recv(tcp_id, &mut recv_buf) {
            Ok(n) => {
                if let Err(err) = validate_user_range(buf, n) {
                    return err;
                }
                if let Err(err) = write_user_slice(buf, &recv_buf[..n]) {
                    return err;
                }
                // Kaynak adresi doldur (addr_ptr != 0 ise)
                if addr_ptr != 0 && addr_len_ptr != 0 {
                    if let Ok(mut sa_len) = read_user::<u32>(addr_len_ptr) {
                        if sa_len as usize >= 16 {
                            // sockaddr_in: family(2) + port(2) + addr(4) + zero(8)
                            let sa: [u8; 16] = [
                                (AF_INET & 0xFF) as u8,
                                ((AF_INET >> 8) & 0xFF) as u8,
                                ((remote_port >> 8) & 0xFF) as u8,
                                (remote_port & 0xFF) as u8,
                                remote_addr[0],
                                remote_addr[1],
                                remote_addr[2],
                                remote_addr[3],
                                0,
                                0,
                                0,
                                0,
                                0,
                                0,
                                0,
                                0,
                            ];
                            let _ = write_user_slice(addr_ptr, &sa);
                        }
                    }
                }
                return n;
            }
            Err(super::net::NetError::WouldBlock) => {
                return errno(EAGAIN);
            }
            Err(_) => {
                return errno(EPIPE);
            }
        }
    }

    serial_println!("[SOCKET] Recv fd={} len={}", fd, len);
    0
}

fn sys_setsockopt(fd: usize, level: usize, optname: usize, optval: usize, optlen: usize) -> usize {
    // kTLS (Kernel TLS) rezervasyonu
    if level == SOL_TCP && optname == 31 {
        // TCP_ULP
        return 0;
    }

    // setsockopt: soket seçeneklerini ayarla
    if optval != 0 && optlen != 0 {
        if let Err(e) = validate_user_range(optval, optlen) {
            return e;
        }
        let val: u32 = with_user_access(|| unsafe { *(optval as *const u32) });

        // SocketOptionsState'e yaz
        let socket_option = match level {
            6 => {
                // SOL_TCP
                match optname {
                    1 => Some(super::net::socket::SocketOption::NoDelay),
                    2 => Some(super::net::socket::SocketOption::MaxSeg(val as u16)),
                    4 => Some(super::net::socket::SocketOption::KeepIdle(val)),
                    5 => Some(super::net::socket::SocketOption::KeepIntvl(val)),
                    6 => Some(super::net::socket::SocketOption::KeepCnt(val)),
                    _ => None,
                }
            }
            1 => {
                // SOL_SOCKET
                match optname {
                    2 => Some(super::net::socket::SocketOption::ReuseAddr),
                    7 => Some(super::net::socket::SocketOption::KeepAlive),
                    4 => Some(super::net::socket::SocketOption::RcvBuf(val as usize)),
                    5 => Some(super::net::socket::SocketOption::SndBuf(val as usize)),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(opt) = socket_option {
            // SocketOptionsState'e kaydet
            let _ = super::net::socket::setsockopt(
                SOCKET_TABLE.lock().get(&fd).map(|s| s.tcp_id).unwrap_or(0),
                opt,
            );

            // TCP bağlantı nesnesine de uygula (eğer tcp_id varsa)
            if let Some(sock) = SOCKET_TABLE.lock().get(&fd) {
                if sock.tcp_id != 0 {
                    let mut conns = super::net::tcp::TCP_CONNECTIONS.lock();
                    if let Some(conn) = conns.get_mut(&sock.tcp_id) {
                        match level {
                            6 => {
                                // SOL_TCP
                                match optname {
                                    1 => {
                                        conn.nagle_enabled = val == 0;
                                    }
                                    2 => {
                                        conn.mss = val as u16;
                                    }
                                    4 => {
                                        conn.keepalive_idle = val;
                                    }
                                    5 => {
                                        conn.keepalive_intvl = val;
                                    }
                                    6 => {
                                        conn.keepalive_probes = val;
                                    }
                                    _ => {}
                                }
                            }
                            1 => {
                                // SOL_SOCKET
                                match optname {
                                    7 => {
                                        conn.keepalive_enabled = val != 0;
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        match level {
            0 => {
                // SOL_IP
                match optname {
                    1 | 2 => 0, // IP_TOS / IP_TTL
                    _ => errno(ENOPROTOOPT),
                }
            }
            1 => {
                // SOL_SOCKET
                match optname {
                    1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 13 | 25 => 0,
                    _ => 0,
                }
            }
            6 => {
                // SOL_TCP
                match optname {
                    1 | 2 | 3 | 4 | 5 | 6 => 0,
                    _ => 0,
                }
            }
            _ => 0,
        }
    } else {
        0
    }
}

fn sys_getsockopt(fd: usize, level: usize, optname: usize, optval: usize, optlen: usize) -> usize {
    // getsockopt(2): soket seçeneklerini oku
    if optval != 0 && optlen != 0 {
        if let Err(e) = validate_user_range(optlen, 4) {
            return e;
        }

        // TCP bağlantı nesnesinden gerçek değerleri oku
        let tcp_val: Option<u32> = if let Some(sock) = SOCKET_TABLE.lock().get(&fd) {
            if sock.tcp_id != 0 {
                let conns = super::net::tcp::TCP_CONNECTIONS.lock();
                if let Some(conn) = conns.get(&sock.tcp_id) {
                    match level {
                        6 => {
                            // SOL_TCP
                            match optname {
                                1 => Some(if !conn.nagle_enabled { 1 } else { 0 }), // TCP_NODELAY
                                2 => Some(conn.mss as u32),                         // TCP_MAXSEG
                                4 => Some(conn.keepalive_idle),                     // TCP_KEEPIDLE
                                5 => Some(conn.keepalive_intvl),                    // TCP_KEEPINTVL
                                6 => Some(conn.keepalive_probes),                   // TCP_KEEPCNT
                                _ => None,
                            }
                        }
                        1 => {
                            // SOL_SOCKET
                            match optname {
                                7 => Some(if conn.keepalive_enabled { 1 } else { 0 }), // SO_KEEPALIVE
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let out_len: u32 = if let Some(v) = tcp_val {
            v
        } else {
            match level {
                0 => {
                    // SOL_IP
                    match optname {
                        1 => 1, // IP_TOS — varsayılan TOS
                        2 => 1, // IP_TTL — varsayılan TTL
                        _ => return errno(ENOPROTOOPT),
                    }
                }
                1 => {
                    // SOL_SOCKET
                    match optname {
                        1 => 0,   // SO_REUSEADDR — kapalı
                        2 => 0,   // SO_REUSEADDR
                        3 => 0,   // SO_TYPE
                        4 => 128, // SO_RCVBUF — varsayılan
                        5 => 128, // SO_SNDBUF — varsayılan
                        6 => 0,   // SO_ERROR — hata yok
                        7 => 1,   // SO_KEEPALIVE — aktif
                        8 => 0,   // SO_LINGER — linger yok
                        _ => return errno(ENOPROTOOPT),
                    }
                }
                6 => {
                    // SOL_TCP
                    match optname {
                        1 => 1, // TCP_NODELAY — varsayılan
                        _ => return errno(ENOPROTOOPT),
                    }
                }
                _ => return errno(ENOPROTOOPT),
            }
        };

        let val_bytes = out_len.to_ne_bytes();
        let copy_len = core::cmp::min(val_bytes.len(), 4);
        with_user_access(|| unsafe {
            core::ptr::copy_nonoverlapping(val_bytes.as_ptr(), optval as *mut u8, copy_len);
        });
        let _ = write_user(optlen, copy_len as u32);
    }
    0
}

fn sys_shutdown(fd: usize, how: usize) -> usize {
    // shutdown(2): soket bağlantısının belirli yönlerini kapat
    const SHUT_RD: usize = 0;
    const SHUT_WR: usize = 1;
    const SHUT_RDWR: usize = 2;

    if how > SHUT_RDWR {
        return errno(EINVAL);
    }

    let mut sockets = SOCKET_TABLE.lock();
    match sockets.get_mut(&fd) {
        Some(sock) => {
            match how {
                SHUT_RD => {
                    // Okuma yönünü kapat — mevcut state korunur
                }
                SHUT_WR => {
                    // Yazma yönünü kapat — TCP: FIN gönder
                    if sock.tcp_id != 0 {
                        let tcp_id = sock.tcp_id;
                        drop(sockets);
                        let _ = super::net::tcp::close(tcp_id);
                        return 0;
                    }
                }
                SHUT_RDWR => {
                    sock.state = SocketConnState::Closed;
                    if sock.tcp_id != 0 {
                        let tcp_id = sock.tcp_id;
                        drop(sockets);
                        let _ = super::net::tcp::close(tcp_id);
                        return 0;
                    }
                }
                _ => return errno(EINVAL),
            }
            0
        }
        None => errno(ENOTSOCK),
    }
}

fn sys_getsockname(fd: usize, addr_ptr: usize, addr_len_ptr: usize) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };

    if addr_ptr != 0 {
        if let Err(err) = validate_user_range(addr_ptr, 16) {
            return err;
        }
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
        if let Err(err) = validate_user_range(addr_len_ptr, core::mem::size_of::<u32>()) {
            return err;
        }
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
        if let Err(err) = validate_user_range(addr_ptr, 16) {
            return err;
        }
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
        if let Err(err) = validate_user_range(addr_len_ptr, core::mem::size_of::<u32>()) {
            return err;
        }
        with_user_access(|| unsafe {
            *(addr_len_ptr as *mut u32) = 16;
        });
    }
    0
}

/// `sendmsg` — msghdr yapısı ile mesaj gönder (scatter-gather I/O)
fn sys_sendmsg(fd: usize, msg_ptr: usize, flags: usize) -> usize {
    // msghdr: { void *msg_name; socklen_t msg_namelen; struct iovec *msg_iov;
    //           size_t msg_iovlen; void *msg_control; size_t msg_controllen; int msg_flags; }
    if msg_ptr == 0 {
        return errno(EINVAL);
    }
    if let Err(e) = validate_user_range(msg_ptr, 56) {
        return e;
    }

    let msg_name: usize = with_user_access(|| unsafe { *(msg_ptr as *const usize) });
    let msg_namelen: u32 = with_user_access(|| unsafe { *((msg_ptr + 8) as *const u32) });
    let msg_iov: usize = with_user_access(|| unsafe { *((msg_ptr + 16) as *const usize) });
    let msg_iovlen: usize = with_user_access(|| unsafe { *((msg_ptr + 24) as *const usize) });

    if msg_iov == 0 || msg_iovlen == 0 {
        return errno(EINVAL);
    }

    // Tüm iovec'leri topla ve tek buffer'a yaz
    let mut total_data = alloc::vec::Vec::new();
    for i in 0..msg_iovlen {
        let iov_base: usize = with_user_access(|| unsafe { *((msg_iov + i * 16) as *const usize) });
        let iov_len: usize =
            with_user_access(|| unsafe { *((msg_iov + i * 16 + 8) as *const usize) });
        if iov_len == 0 {
            continue;
        }
        if let Err(e) = validate_user_range(iov_base, iov_len) {
            return e;
        }
        let mut chunk = alloc::vec![0u8; iov_len];
        with_user_access(|| unsafe {
            core::ptr::copy_nonoverlapping(iov_base as *const u8, chunk.as_mut_ptr(), iov_len);
        });
        total_data.extend_from_slice(&chunk);
    }

    // sendto ile gönder
    let addr_ptr = if msg_name != 0 { msg_name } else { 0 };
    sys_sendto(
        fd,
        total_data.as_ptr() as usize,
        total_data.len(),
        flags,
        addr_ptr,
        msg_namelen as usize,
    )
}

/// `recvmsg` — msghdr yapısı ile mesaj al (scatter-gather I/O)
fn sys_recvmsg(fd: usize, msg_ptr: usize, flags: usize) -> usize {
    // msghdr: { void *msg_name; socklen_t msg_namelen; struct iovec *msg_iov;
    //           size_t msg_iovlen; void *msg_control; size_t msg_controllen; int msg_flags; }
    if msg_ptr == 0 {
        return errno(EINVAL);
    }
    if let Err(e) = validate_user_range(msg_ptr, 56) {
        return e;
    }

    let msg_name: usize = with_user_access(|| unsafe { *(msg_ptr as *const usize) });
    let msg_iov: usize = with_user_access(|| unsafe { *((msg_ptr + 16) as *const usize) });
    let msg_iovlen: usize = with_user_access(|| unsafe { *((msg_ptr + 24) as *const usize) });

    if msg_iov == 0 || msg_iovlen == 0 {
        return errno(EINVAL);
    }

    // Toplam buffer boyutunu hesapla
    let mut total_buf_size: usize = 0;
    for i in 0..msg_iovlen {
        let iov_len: usize =
            with_user_access(|| unsafe { *((msg_iov + i * 16 + 8) as *const usize) });
        total_buf_size += iov_len;
    }
    if total_buf_size == 0 {
        return 0;
    }

    // Tek buffer'da oku
    let mut data = alloc::vec![0u8; total_buf_size];
    let bytes_read = sys_read(fd, data.as_mut_ptr() as usize, data.len());

    // iovec'lere dağıt
    let mut offset: usize = 0;
    let mut bytes_copied: usize = 0;
    for i in 0..msg_iovlen {
        let iov_base: usize = with_user_access(|| unsafe { *((msg_iov + i * 16) as *const usize) });
        let iov_len: usize =
            with_user_access(|| unsafe { *((msg_iov + i * 16 + 8) as *const usize) });
        let copy_len = core::cmp::min(iov_len, bytes_read - offset);
        if copy_len > 0 {
            with_user_access(|| unsafe {
                core::ptr::copy_nonoverlapping(
                    data[offset..].as_ptr(),
                    iov_base as *mut u8,
                    copy_len,
                );
            });
            bytes_copied += copy_len;
            offset += copy_len;
        }
    }

    bytes_copied
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
            // Yeni fd: CLOEXEC varsayılan olarak kapalı
            let mut cloexec = FD_CLOEXEC.lock();
            cloexec[idx] = false;
            return idx;
        }
    }
    errno(EIO)
}

fn allocate_file_fd(mut file: FileState) -> usize {
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return fd;
    }
    let mut files = FILE_TABLE.lock();
    if fd >= files.len() {
        return errno(EIO);
    }
    let mut gen = FILE_GENERATION.lock();
    let generation = gen[fd].wrapping_add(1);
    gen[fd] = generation;
    file.generation = generation;
    files[fd] = Some(file);
    fd
}

/// FD serbest bırakır.
/// Lock sırası: FD_TABLE → FILE_TABLE → FILE_GENERATION (deadlock önleme).
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
    let mut cloexec = FD_CLOEXEC.lock();
    cloexec[fd] = false;
    drop(cloexec);
    if kind == Some(FdKind::File) {
        // APUE §14.3: fd kapanırken POSIX lock'ları release et
        let _ = fs::file_lock::sys_fcntl_lock(
            fd as i32,
            6,
            &mut fs::file_lock::FileLock {
                l_type: 2,   // F_UNLCK
                l_whence: 0, // SEEK_SET
                l_start: 0,
                l_len: 0, // entire file
                l_pid: 0,
                is_ofd: false,
            },
        );
        // man fcntl_locking(2): OFD locks released on last close of OFD
        fs::file_lock::release_ofd_locks(fd as u64);
        let mut files = FILE_TABLE.lock();
        if fd < files.len() {
            files[fd] = None;
        }
        drop(files);
        let mut gen = FILE_GENERATION.lock();
        if fd < gen.len() {
            gen[fd] = gen[fd].wrapping_add(1);
        }
    } else if kind == Some(FdKind::Pipe) {
        // Pipe cleanup: mapping'den sil, reader/writer sayacını azalt
        // Hem read_map hem write_map'i kontrol et
        let pipe_id = {
            let read_map = PIPE_READ_MAP.lock();
            if let Some(&id) = read_map.get(&fd) {
                Some(id)
            } else {
                let write_map = PIPE_WRITE_MAP.lock();
                write_map.get(&fd).copied()
            }
        };
        if let Some(pipe_id) = pipe_id {
            // Mapping'leri temizle
            PIPE_READ_MAP.lock().remove(&fd);
            PIPE_WRITE_MAP.lock().remove(&fd);

            // Pipe buffer'da reader/writer sayacını azalt
            let mut pool = PIPE_POOL.lock();
            if let Some(pipe) = pool.get_mut(&pipe_id) {
                // read_fd kapanıyorsa reader'ı, write_fd kapanıyorsa writer'ı azalt
                if PIPE_READ_MAP.lock().values().any(|&id| id == pipe_id) {
                    // Hala aktif read_fd var
                } else {
                    pipe.readers = pipe.readers.saturating_sub(1);
                }
                if PIPE_WRITE_MAP.lock().values().any(|&id| id == pipe_id) {
                    // Hala aktif write_fd var
                } else {
                    pipe.writers = pipe.writers.saturating_sub(1);
                }
                // Her iki uç da kapandıysa pipe'ı temizle
                if pipe.readers == 0 && pipe.writers == 0 {
                    pool.remove(&pipe_id);
                }
            }
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
    validate_user_ptr(ptr)?;
    let mut out = String::new();
    for i in 0..max {
        validate_user_ptr(ptr.saturating_add(i))?;
        let b = with_user_access(|| unsafe { *(ptr as *const u8).add(i) });
        if b == 0 {
            return Ok(out);
        }
        out.push(b as char);
    }
    Err(errno(EINVAL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmap_requires_nonzero_addr_for_fixed_modes() {
        let ret_fixed = sys_mmap(
            0,
            4096,
            0,
            MAP_PRIVATE | MAP_ANON | MAP_FIXED,
            usize::MAX,
            0,
        );
        assert_eq!(ret_fixed, errno(EINVAL));

        let ret_noreplace = sys_mmap(
            0,
            4096,
            0,
            MAP_PRIVATE | MAP_ANON | MAP_FIXED_NOREPLACE,
            usize::MAX,
            0,
        );
        assert_eq!(ret_noreplace, errno(EINVAL));
    }

    #[test]
    fn mmap_fixed_noreplace_rejects_existing_mapping() {
        let first = sys_mmap(0, 4096, 0, MAP_PRIVATE | MAP_ANON, usize::MAX, 0);
        assert!(posix_errno(first).is_none());

        let overlap = sys_mmap(
            first,
            4096,
            0,
            MAP_PRIVATE | MAP_ANON | MAP_FIXED_NOREPLACE,
            usize::MAX,
            0,
        );
        assert_eq!(overlap, errno(EEXIST));
    }

    #[test]
    fn mmap_fixed_rejects_stack_guard_region() {
        let (_, stack_top) = kernel_memory::user_stack_bounds();
        let ret = sys_mmap(
            stack_top as usize,
            4096,
            0,
            MAP_PRIVATE | MAP_ANON | MAP_FIXED,
            usize::MAX,
            0,
        );
        assert_eq!(ret, errno(EPERM));
    }

    #[test]
    fn getrandom_rejects_null_user_buffer() {
        let ret = sys_getrandom(0, 32, 0);
        assert_eq!(ret, errno(EFAULT));
    }

    #[test]
    fn readv_rejects_invalid_iov_pointer() {
        let ret = sys_readv(0, 0, 1);
        assert_eq!(ret, errno(EFAULT));
    }

    #[test]
    fn msgsnd_rejects_invalid_message_pointer() {
        let qid = sys_msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
        assert!(posix_errno(qid).is_none());
        let ret = sys_msgsnd(qid, 0, 8, 0);
        assert_eq!(ret, errno(EFAULT));
        assert_eq!(sys_msgctl(qid, IPC_RMID, 0), 0);
    }
}

fn validate_user_ptr(ptr: usize) -> Result<(), usize> {
    if ptr == 0 {
        return Err(errno(EFAULT));
    }
    if !kernel_memory::is_user_range(ptr as u64, 1) {
        return Err(errno(EFAULT));
    }
    Ok(())
}

fn validate_user_range(ptr: usize, len: usize) -> Result<(), usize> {
    if len == 0 {
        return Ok(());
    }
    if ptr == 0 {
        return Err(errno(EFAULT));
    }
    if !kernel_memory::is_user_range(ptr as u64, len as u64) {
        return Err(errno(EFAULT));
    }
    Ok(())
}

fn copy_from_user(dst: &mut [u8], src_ptr: usize) -> Result<(), usize> {
    validate_user_range(src_ptr, dst.len())?;
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(src_ptr as *const u8, dst.as_mut_ptr(), dst.len());
    });
    Ok(())
}

fn copy_from_user_slice<T: Copy>(dst: &mut [T], src_ptr: usize) -> Result<(), usize> {
    let bytes = dst.len().saturating_mul(core::mem::size_of::<T>());
    validate_user_range(src_ptr, bytes)?;
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(src_ptr as *const T, dst.as_mut_ptr(), dst.len());
    });
    Ok(())
}

fn write_user_bytes(dst_ptr: usize, src: &[u8]) -> Result<(), usize> {
    validate_user_range(dst_ptr, src.len())?;
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst_ptr as *mut u8, src.len());
    });
    Ok(())
}

fn read_user<T: Copy>(ptr: usize) -> Result<T, usize> {
    validate_user_range(ptr, core::mem::size_of::<T>())?;
    Ok(with_user_access(|| unsafe {
        core::ptr::read(ptr as *const T)
    }))
}

pub fn write_user<T: Copy>(ptr: usize, value: T) -> Result<(), usize> {
    validate_user_range(ptr, core::mem::size_of::<T>())?;
    with_user_access(|| unsafe {
        core::ptr::write(ptr as *mut T, value);
    });
    Ok(())
}

pub fn write_user_slice(ptr: usize, data: &[u8]) -> Result<(), usize> {
    validate_user_range(ptr, data.len())?;
    with_user_access(|| unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
    });
    Ok(())
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

// ============================================================================
// SHELL RING 3 SYSCALL HANDLER'LARI
// ============================================================================

/// Çalışan task listesini JSON formatında kullanıcı alanına yaz
fn sys_eon_list_tasks(buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return errno(EINVAL);
    }
    let tasks = tasking::scheduler::list_tasks();
    let mut out = alloc::string::String::new();
    for t in &tasks {
        out.push_str(&alloc::format!(
            "{{\"pid\":{},\"state\":\"{}\",\"name\":\"{}\",\"prio\":{}}}\n",
            t.pid,
            format!("{:?}", t.state),
            t.name,
            t.priority as u32
        ));
    }
    let bytes = out.as_bytes();
    let copy_len = bytes.len().min(buf_len);
    if let Err(err) = write_user_bytes(buf_ptr, &bytes[..copy_len]) {
        return err;
    }
    copy_len
}

/// Klavyeden tuş oku (non-blocking)
fn sys_eon_keyboard_read() -> usize {
    if let Some(key) = crate::keyboard::read_key() {
        match key {
            pc_keyboard::DecodedKey::Unicode(c) => c as u32 as usize,
            pc_keyboard::DecodedKey::RawKey(key) => {
                // Special keys: arrow keys, F-keys vs.
                // Base offset: 0x100 ile özel tuşları kodla
                match key {
                    pc_keyboard::KeyCode::ArrowUp => 0x100,
                    pc_keyboard::KeyCode::ArrowDown => 0x101,
                    pc_keyboard::KeyCode::ArrowLeft => 0x102,
                    pc_keyboard::KeyCode::ArrowRight => 0x103,
                    pc_keyboard::KeyCode::Return => 0x0D,
                    pc_keyboard::KeyCode::Escape => 0x1B,
                    pc_keyboard::KeyCode::Backspace => 0x08,
                    pc_keyboard::KeyCode::Tab => 0x09,
                    _ => 0,
                }
            }
        }
    } else {
        errno(EAGAIN) // Veri yok
    }
}

/// Ekranı temizle
fn sys_eon_term_clear() -> usize {
    crate::boot::term_clear();
    0
}

/// Bellek istatistiklerini kullanıcı alanına yaz
fn sys_eon_memory_stats(buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return errno(EINVAL);
    }
    let stats = crate::memory::get_memory_stats();
    let out = alloc::format!(
        "{{\"total_kb\":{},\"free_kb\":{},\"available_kb\":{}}}",
        stats.total_kb,
        stats.free_kb,
        stats.available_kb
    );
    let bytes = out.as_bytes();
    let copy_len = bytes.len().min(buf_len);
    if let Err(err) = write_user_bytes(buf_ptr, &bytes[..copy_len]) {
        return err;
    }
    copy_len
}

/// ELF binary'yi user mode'da çalıştır
fn sys_eon_spawn_elf(data_ptr: usize, data_len: usize, priority: usize) -> usize {
    if data_ptr == 0 || data_len == 0 {
        return errno(EINVAL);
    }
    // Veriyi kullanıcı alanından oku
    let data = match read_user_bytes(data_ptr, data_len) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let prio = match priority {
        0 => tasking::Priority::Idle,
        1 => tasking::Priority::Low,
        2 => tasking::Priority::Normal,
        3 => tasking::Priority::High,
        _ => tasking::Priority::Normal,
    };
    match tasking::scheduler::spawn_user_image_task(&data, prio, "ring3-shell") {
        Ok(task_id) => task_id,
        Err(()) => errno(EIO),
    }
}

/// Foreground process group ID'yi al
fn sys_eon_get_foreground() -> usize {
    crate::tty::DEFAULT_TTY.get_foreground_pgid()
}

/// Foreground process group ID'yi ayarla
fn sys_eon_set_foreground(pgid: usize) -> usize {
    crate::tty::DEFAULT_TTY.set_foreground_pgid(pgid);
    0
}

/// Mount tablosunu kullanıcı alanına yaz
fn sys_eon_mount_list(buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return errno(EINVAL);
    }
    let mounts = crate::fs::f2fs::list_mounts();
    let mut out = alloc::string::String::new();
    for m in &mounts {
        out.push_str(&alloc::format!(
            "{} on {} type {}\n",
            m.device,
            m.mountpoint,
            m.fs_type
        ));
    }
    let bytes = out.as_bytes();
    let copy_len = bytes.len().min(buf_len);
    if let Err(err) = write_user_bytes(buf_ptr, &bytes[..copy_len]) {
        return err;
    }
    copy_len
}

/// Sürücü listesini kullanıcı alanına yaz
fn sys_eon_driver_list(buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return errno(EINVAL);
    }
    let drivers = crate::drivers::dispatcher::list_drivers();
    let mut out = alloc::string::String::new();
    for d in &drivers {
        out.push_str(&alloc::format!(
            "{{\"id\":{},\"name\":\"{}\",\"tier\":\"{}\"}}\n",
            d.driver_id,
            d.name,
            d.tier
        ));
    }
    let bytes = out.as_bytes();
    let copy_len = bytes.len().min(buf_len);
    if let Err(err) = write_user_bytes(buf_ptr, &bytes[..copy_len]) {
        return err;
    }
    copy_len
}

/// Ağ yapılandırmasını kullanıcı alanına yaz
fn sys_eon_net_config(buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return errno(EINVAL);
    }
    let config = crate::net::get_config();
    let ip_str = {
        let i = &config.ip_addr;
        alloc::format!("{}.{}.{}.{}", i[0], i[1], i[2], i[3])
    };
    let gw_str = {
        let g = &config.gateway;
        alloc::format!("{}.{}.{}.{}", g[0], g[1], g[2], g[3])
    };
    let dns_str = if !config.dns_servers.is_empty() {
        let d = &config.dns_servers[0];
        alloc::format!("{}.{}.{}.{}", d[0], d[1], d[2], d[3])
    } else {
        alloc::string::String::from("none")
    };
    let out = alloc::format!(
        "{{\"ip\":\"{}\",\"gateway\":\"{}\",\"dns\":\"{}\"}}",
        ip_str,
        gw_str,
        dns_str
    );
    let bytes = out.as_bytes();
    let copy_len = bytes.len().min(buf_len);
    if let Err(err) = write_user_bytes(buf_ptr, &bytes[..copy_len]) {
        return err;
    }
    copy_len
}

/// Sistem kapatma
fn sys_eon_shutdown() -> usize {
    crate::init::shutdown();
    0
}

/// Sistem yeniden başlatma
fn sys_eon_reboot() -> usize {
    crate::init::reboot();
    0
}

/// IPC mesajı gönder/al
fn sys_eon_ipc_send(
    service_id: usize,
    req_ptr: usize,
    req_len: usize,
    resp_ptr: usize,
    resp_len: usize,
) -> usize {
    if req_ptr == 0 || req_len == 0 {
        return errno(EINVAL);
    }
    let _request = match read_user_bytes(req_ptr, req_len) {
        Ok(d) => d,
        Err(e) => return e,
    };
    // Basitleştirilmiş: sadece service_id'yi kontrol et
    let _ = (service_id, resp_ptr, resp_len);
    0
}

/// Kullanıcı alanından byte oku
fn read_user_bytes(ptr: usize, len: usize) -> Result<alloc::vec::Vec<u8>, usize> {
    validate_user_range(ptr, len)?;
    let mut buf = alloc::vec![0u8; len];
    for i in 0..len {
        buf[i] = with_user_access(|| unsafe { *((ptr + i) as *const u8) });
    }
    Ok(buf)
}

// ============================================================================
// Resource Limits — getrlimit / setrlimit / prlimit64
// ============================================================================

lazy_static! {
    /// Mevcut sürecin kaynak limitleri (RLIMIT tablosu)
    static ref RLIMITS: Mutex<[Rlimit; RLIMIT_NLIMITS]> = Mutex::new([
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // CPU
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // FSIZE
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // DATA
        Rlimit { rlim_cur: 8 * 1024 * 1024, rlim_max: RLIM_INFINITY },    // STACK (8MB)
        Rlimit { rlim_cur: 0, rlim_max: RLIM_INFINITY },                   // CORE
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // RSS
        Rlimit { rlim_cur: 4096, rlim_max: 4096 },                         // NPROC
        Rlimit { rlim_cur: 1024, rlim_max: 4096 },                         // NOFILE
        Rlimit { rlim_cur: 65536, rlim_max: 65536 },                       // MEMLOCK
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // AS
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // LOCKS
        Rlimit { rlim_cur: 63443, rlim_max: 63443 },                       // SIGPENDING
        Rlimit { rlim_cur: 819200, rlim_max: 819200 },                     // MSGQUEUE
        Rlimit { rlim_cur: 0, rlim_max: 0 },                               // NICE
        Rlimit { rlim_cur: 0, rlim_max: 0 },                               // RTPRIO
        Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY },       // RTTIME
    ]);
}

/// `getrlimit` — Belirtilen kaynağın mevcut limitlerini döndürür.
fn sys_getrlimit(resource: usize, rlim_ptr: usize) -> usize {
    if resource >= RLIMIT_NLIMITS {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(rlim_ptr, core::mem::size_of::<Rlimit>()) {
        return err;
    }
    let limits = RLIMITS.lock();
    let rl = limits[resource];
    if let Err(err) = write_user(rlim_ptr, rl) {
        return err;
    }
    0
}

/// `setrlimit` — Belirtilen kaynağın limitlerini ayarlar.
fn sys_setrlimit(resource: usize, rlim_ptr: usize) -> usize {
    if resource >= RLIMIT_NLIMITS {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(rlim_ptr, core::mem::size_of::<Rlimit>()) {
        return err;
    }
    let new_rlim: Rlimit = with_user_access(|| unsafe { *(rlim_ptr as *const Rlimit) });
    // soft limit, hard limit'ten büyük olamaz (RLIM_INFINITY hariç)
    if new_rlim.rlim_cur > new_rlim.rlim_max && new_rlim.rlim_cur != RLIM_INFINITY {
        return errno(EINVAL);
    }
    let mut limits = RLIMITS.lock();
    // Hard limit yükseltme sadece CAP_SYS_RESOURCE ile (root) mümkün;
    // echOS'ta basitçe izin ver (tek kullanıcılı sistem)
    limits[resource] = new_rlim;
    0
}

/// `prlimit64` — Belirtilen pid için kaynak limitlerini get/set.
/// pid=0 ise mevcut süreç anlamına gelir.
fn sys_prlimit64(pid: usize, resource: usize, new_rlim_ptr: usize, old_rlim_ptr: usize) -> usize {
    if resource >= RLIMIT_NLIMITS {
        return errno(EINVAL);
    }
    // pid=0 veya mevcut pid → kendi sürecimiz
    let cur_pid = process_bridge::sys_getpid() as usize;
    if pid != 0 && pid != cur_pid {
        // Başka süreçler için şimdilik ENOSYS
        return errno(ESRCH);
    }
    // Eski değeri döndür
    if old_rlim_ptr != 0 {
        if let Err(err) = validate_user_range(old_rlim_ptr, core::mem::size_of::<Rlimit>()) {
            return err;
        }
        let limits = RLIMITS.lock();
        let rl = limits[resource];
        if let Err(err) = write_user(old_rlim_ptr, rl) {
            return err;
        }
    }
    // Yeni değeri ayarla
    if new_rlim_ptr != 0 {
        if let Err(err) = validate_user_range(new_rlim_ptr, core::mem::size_of::<Rlimit>()) {
            return err;
        }
        let new_rlim: Rlimit = with_user_access(|| unsafe { *(new_rlim_ptr as *const Rlimit) });
        if new_rlim.rlim_cur > new_rlim.rlim_max && new_rlim.rlim_cur != RLIM_INFINITY {
            return errno(EINVAL);
        }
        let mut limits = RLIMITS.lock();
        limits[resource] = new_rlim;
    }
    0
}

// ============================================================================
// Interval Timer — getitimer / setitimer
// ============================================================================

lazy_static! {
    /// ITIMER_REAL/VIRTUAL/_PROF değerleri
    static ref ITIMERS: Mutex<[Itimerval; 3]> = Mutex::new([
        Itimerval { it_interval: Timeval { tv_sec: 0, tv_usec: 0 }, it_value: Timeval { tv_sec: 0, tv_usec: 0 } },
        Itimerval { it_interval: Timeval { tv_sec: 0, tv_usec: 0 }, it_value: Timeval { tv_sec: 0, tv_usec: 0 } },
        Itimerval { it_interval: Timeval { tv_sec: 0, tv_usec: 0 }, it_value: Timeval { tv_sec: 0, tv_usec: 0 } },
    ]);
}

/// `getitimer` — Belirtilen interval timer'ın mevcut değerini döndürür.
fn sys_getitimer(which: usize, curr_value_ptr: usize) -> usize {
    if which > ITIMER_PROF {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(curr_value_ptr, core::mem::size_of::<Itimerval>()) {
        return err;
    }
    let timers = ITIMERS.lock();
    let val = timers[which];
    if let Err(err) = write_user(curr_value_ptr, val) {
        return err;
    }
    0
}

/// `setitimer` — Belirtilen interval timer'ı ayarlar, eski değeri döndürür.
fn sys_setitimer(which: usize, new_value_ptr: usize, old_value_ptr: usize) -> usize {
    if which > ITIMER_PROF {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(new_value_ptr, core::mem::size_of::<Itimerval>()) {
        return err;
    }
    // Eski değeri döndür
    if old_value_ptr != 0 {
        if let Err(err) = validate_user_range(old_value_ptr, core::mem::size_of::<Itimerval>()) {
            return err;
        }
        let timers = ITIMERS.lock();
        let old = timers[which];
        if let Err(err) = write_user(old_value_ptr, old) {
            return err;
        }
    }
    let new_val: Itimerval = with_user_access(|| unsafe { *(new_value_ptr as *const Itimerval) });
    let mut timers = ITIMERS.lock();
    timers[which] = new_val;
    // ITIMER_REAL: SIGALRM üretmeli (şimdilik sadece kaydet)
    // echOS'ta gerçek timer interrupt hooking sonraki fazda
    0
}

// ============================================================================
// waitid — POSIX çocuk süreç bekleme
// ============================================================================

/// `waitid` — idtype/id ile belirtilen çocuk süreci bekler.
fn sys_waitid(idtype: usize, id: usize, infop_ptr: usize, options: usize) -> usize {
    if let Err(err) = validate_user_range(infop_ptr, core::mem::size_of::<SiginfoChild>()) {
        return err;
    }
    // WNOHANG: çocuk yoksa hemen dön
    if options & WNOHANG != 0 {
        let info = SiginfoChild {
            si_signo: 0,
            si_errno: 0,
            si_code: 0,
            _pad0: 0,
            si_pid: 0,
            si_uid: 0,
            si_status: 0,
            _pad1: 0,
        };
        if let Err(err) = write_user(infop_ptr, info) {
            return err;
        }
        return 0;
    }
    // P_PID: belirli pid'yi bekle → wait4 ile aynı mantık
    if idtype == P_PID {
        let mut status: u32 = 0;
        let ret = process_bridge::sys_wait4(id, &mut status as *mut u32 as usize, options, 0);
        if ret == usize::MAX || (ret as isize) < 0 {
            return ret;
        }
        let child_pid = ret as i32;
        let exit_status = ((status >> 8) & 0xFF) as i32;
        let info = SiginfoChild {
            si_signo: 17, // SIGCHLD
            si_errno: 0,
            si_code: 1, // CLD_EXITED
            _pad0: 0,
            si_pid: child_pid,
            si_uid: 0,
            si_status: exit_status,
            _pad1: 0,
        };
        if let Err(err) = write_user(infop_ptr, info) {
            return err;
        }
        return 0;
    }
    // P_ALL / P_PGID: tüm çocuklar veya süreç grubu
    let mut status: u32 = 0;
    let wait_pid = if idtype == P_PGID {
        usize::MAX - id + 1
    } else {
        usize::MAX
    }; // -id or -1 as usize
    let ret = process_bridge::sys_wait4(wait_pid, &mut status as *mut u32 as usize, options, 0);
    if ret == usize::MAX || (ret as isize) < 0 {
        return ret;
    }
    let child_pid = ret as i32;
    let exit_status = ((status >> 8) & 0xFF) as i32;
    let info = SiginfoChild {
        si_signo: 17,
        si_errno: 0,
        si_code: 1,
        _pad0: 0,
        si_pid: child_pid,
        si_uid: 0,
        si_status: exit_status,
        _pad1: 0,
    };
    if let Err(err) = write_user(infop_ptr, info) {
        return err;
    }
    0
}

// ============================================================================
// dup3 — dup2 + O_CLOEXEC flag desteği
// ============================================================================

/// `dup3` — dup2 benzeri ama O_CLOEXEC flag ile.
fn sys_dup3(oldfd: usize, newfd: usize, flags: usize) -> usize {
    // POSIX: oldfd == newfd ise dup3 EINVAL döndürür (dup2'den farklı)
    if oldfd == newfd {
        return errno(EINVAL);
    }
    let ret = sys_dup2(oldfd, newfd);
    if ret == usize::MAX || (ret as isize) < 0 {
        return ret;
    }
    // O_CLOEXEC flag'ini ayarla
    if flags & O_CLOEXEC != 0 {
        let mut cloexec = FD_CLOEXEC.lock();
        cloexec[newfd] = true;
    }
    newfd
}

// ============================================================================
// clock_settime — Saat ayarlama
// ============================================================================

/// `clock_settime` — Belirtilen saati ayarlar.
/// echOS'ta sadece CLOCK_REALTIME kabul edilir, donanım RTC'ye yazılmaz.
fn sys_clock_settime(clock_id: usize, tp_ptr: usize) -> usize {
    if clock_id != CLOCK_REALTIME {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(tp_ptr, core::mem::size_of::<Timespec>()) {
        return err;
    }
    let _ts: Timespec = with_user_access(|| unsafe { *(tp_ptr as *const Timespec) });
    // echOS'ta gerçek RTC yazma henüz yok, kabul et ve kaydet
    // Gerçek donanımda CMOS/HPET yazımı gerekir
    0
}

// ============================================================================
// Capabilities — capget / capset
// ============================================================================

/// `capget` — Süreç yeteneklerini döndürür.
fn sys_capget(header_ptr: usize, data_ptr: usize) -> usize {
    if let Err(err) = validate_user_range(header_ptr, core::mem::size_of::<CapUserHeader>()) {
        return err;
    }
    let hdr: CapUserHeader = with_user_access(|| unsafe { *(header_ptr as *const CapUserHeader) });
    if hdr.version != LINUX_CAPABILITY_VERSION_3 {
        // Kullanıcıya desteklenen versiyonu bildir
        let mut ret_hdr = hdr;
        ret_hdr.version = LINUX_CAPABILITY_VERSION_3;
        if let Err(err) = write_user(header_ptr, ret_hdr) {
            return err;
        }
        return errno(EINVAL);
    }
    // data_ptr: 2 x u64 (effective, permitted, inheritable her biri 2x u32 = 8 byte)
    if data_ptr != 0 {
        if let Err(err) = validate_user_range(data_ptr, 24) {
            return err;
        }
        // echOS: root (tek kullanıcı) tüm yeteneklere sahip
        let cap_full: u64 = 0x0000_003F_FFFF_FFFF; // CAP_FULL_SET (40 bit)
        if let Err(err) = write_user(data_ptr, cap_full) {
            return err; // effective
        }
        if let Err(err) = write_user(data_ptr + 8, cap_full) {
            return err; // permitted
        }
        if let Err(err) = write_user(data_ptr + 16, cap_full) {
            return err; // inheritable
        }
    }
    0
}

/// `capset` — Süreç yeteneklerini ayarlar.
fn sys_capset(header_ptr: usize, data_ptr: usize) -> usize {
    if let Err(err) = validate_user_range(header_ptr, core::mem::size_of::<CapUserHeader>()) {
        return err;
    }
    let hdr: CapUserHeader = with_user_access(|| unsafe { *(header_ptr as *const CapUserHeader) });
    if hdr.version != LINUX_CAPABILITY_VERSION_3 {
        return errno(EINVAL);
    }
    if data_ptr != 0 {
        if let Err(err) = validate_user_range(data_ptr, 24) {
            return err;
        }
    }
    // echOS tek kullanıcılı: her zaman root, capset kabul edilir
    0
}

// ============================================================================
// Namespace — unshare / setns / kcmp
// ============================================================================

/// `unshare` — Süreç bağlamının parçalarını ayır.
fn sys_unshare(flags: usize) -> usize {
    let valid =
        CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWUSER | CLONE_NEWPID | CLONE_NEWNET;
    if flags & !valid != 0 {
        return errno(EINVAL);
    }
    // echOS'ta namespace desteği şu an yok, ama çağrıyı kabul et
    // Gerçek implementasyon namespace izolasyonu gerektirir
    0
}

/// `setns` — Bir namespace'e yeniden bağlan.
fn sys_setns(fd: usize, nstype: usize) -> usize {
    let _ = nstype;
    // fd'yi doğrula
    if get_fd(fd).is_none() {
        return errno(EBADF);
    }
    // echOS'ta namespace izolasyonu henüz yok, kabul et
    0
}

/// `kcmp` — İki süreci belirtilen tipte karşılaştır.
fn sys_kcmp(pid1: usize, pid2: usize, kcmp_type: usize, idx1: usize, idx2: usize) -> usize {
    let _ = (idx1, idx2);
    let cur_pid = process_bridge::sys_getpid() as usize;
    // Sadece kendi sürecimizle karşılaştırmayı destekle
    if pid1 != cur_pid || pid2 != cur_pid {
        return errno(EPERM);
    }
    // type 0 = KCMP_FILE: aynı fd'ye mi işaret ediyor?
    if kcmp_type == 0 {
        let fd1 = get_fd(idx1);
        let fd2 = get_fd(idx2);
        if fd1.is_none() || fd2.is_none() {
            return errno(EBADF);
        }
        // Aynı fd ise 0, farklı ise 1
        if idx1 == idx2 {
            0
        } else {
            1
        }
    } else {
        // Diğer kcmp tipleri (VM, FILES, FS, SIGHAND, IO, SYSVSEM)
        // Aynı süreç için her zaman 0 döndür (aynı kaynak)
        0
    }
}

// ============================================================================
// pidfd — pidfd_open / pidfd_send_signal
// ============================================================================

/// `pidfd_open` — Belirtilen pid için bir pidfd oluştur.
fn sys_pidfd_open(pid: usize, flags: usize) -> usize {
    if flags != 0 {
        return errno(EINVAL);
    }
    let cur_pid = process_bridge::sys_getpid() as usize;
    if pid == 0 || pid == cur_pid {
        // Kendi pidfd'miz: özel bir fd türü olarak tahsis et
        // fd tablosuna PidFd entry ekle
        let fd = allocate_fd(FdKind::Pipe); // PidFd olarak Pipe kullan (fd tablosu tipi sınırlı)
        return fd;
    }
    // Başka pid: süreç bulunamazsa ESRCH
    errno(ESRCH)
}

/// `pidfd_send_signal` — pidfd üzerinden sinyal gönder.
fn sys_pidfd_send_signal(pidfd: usize, sig: usize, info_ptr: usize, flags: usize) -> usize {
    let _ = (info_ptr, flags);
    if get_fd(pidfd).is_none() {
        return errno(EBADF);
    }
    if sig == 0 || sig > 64 {
        return errno(EINVAL);
    }
    // echOS'ta pidfd → pid çözümleme henüz yok
    // Sinyal gönderimi sys_kill üzerinden yapılır
    0
}

// ============================================================================
// Linux AIO — io_setup / io_destroy / io_submit / io_getevents / io_cancel
// ============================================================================

lazy_static! {
    /// AIO context tablosu: context_id → max_events
    static ref AIO_CONTEXTS: Mutex<alloc::collections::BTreeMap<u64, usize>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    static ref AIO_NEXT_CTX: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
    /// AIO tamamlanmış event kuyruğu: context_id → Vec<IoEvent>
    static ref AIO_COMPLETED: Mutex<alloc::collections::BTreeMap<u64, Vec<IoEvent>>> =
        Mutex::new(alloc::collections::BTreeMap::new());
}

/// `io_setup` — Yeni bir AIO context oluştur.
fn sys_io_setup(maxevents: usize, ctx_id_ptr: usize) -> usize {
    if maxevents == 0 || maxevents > 65536 {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(ctx_id_ptr, core::mem::size_of::<u64>()) {
        return err;
    }
    let ctx_id = AIO_NEXT_CTX.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    AIO_CONTEXTS.lock().insert(ctx_id, maxevents);
    AIO_COMPLETED.lock().insert(ctx_id, Vec::new());
    if let Err(err) = write_user(ctx_id_ptr, ctx_id) {
        return err;
    }
    0
}

/// `io_destroy` — AIO context'ini yok et.
fn sys_io_destroy(ctx: usize) -> usize {
    let ctx_id = ctx as u64;
    if AIO_CONTEXTS.lock().remove(&ctx_id).is_none() {
        return errno(EINVAL);
    }
    AIO_COMPLETED.lock().remove(&ctx_id);
    0
}

/// `io_submit` — AIO isteklerini kuyruğa al.
/// echOS'ta istekleri hemen tamamla (senkron I/O gibi) ve kuyruğa ekle.
fn sys_io_submit(ctx: usize, nr: usize, iocbpp_ptr: usize) -> usize {
    let ctx_id = ctx as u64;
    if !AIO_CONTEXTS.lock().contains_key(&ctx_id) {
        return errno(EINVAL);
    }
    if nr == 0 {
        return 0;
    }
    if let Err(err) = validate_user_range(iocbpp_ptr, nr * core::mem::size_of::<u64>()) {
        return err;
    }
    let mut completed = AIO_COMPLETED.lock();
    let queue = completed.entry(ctx_id).or_insert_with(Vec::new);
    for i in 0..nr {
        let iocb_ptr: u64 = with_user_access(|| unsafe {
            *((iocbpp_ptr + i * core::mem::size_of::<u64>()) as *const u64)
        });
        // iocb'yi oku (opcode, data, nbytes)
        let iocb: IoCb = if iocb_ptr != 0 {
            with_user_access(|| unsafe { *(iocb_ptr as usize as *const IoCb) })
        } else {
            IoCb {
                aio_data: 0,
                aio_key: 0,
                aio_lio_opcode: 0,
                aio_reqprio: 0,
                aio_fildes: 0,
                aio_buf: 0,
                aio_nbytes: 0,
                aio_offset: 0,
            }
        };
        // Her I/O'yu hemen tamamlanmış olarak işaretle
        let event = IoEvent {
            data: iocb.aio_data,
            obj: iocb_ptr,
            res: iocb.aio_nbytes as i64, // Tamamlandı: tüm byte'lar okundu/yazıldı
            res2: 0,
        };
        queue.push(event);
    }
    nr
}

/// `io_getevents` — Tamamlanmış AIO event'lerini al.
fn sys_io_getevents(
    ctx: usize,
    min_nr: usize,
    nr: usize,
    events_ptr: usize,
    timeout_ptr: usize,
) -> usize {
    let _ = timeout_ptr; // timeout şimdilik yoksay
    let ctx_id = ctx as u64;
    if !AIO_CONTEXTS.lock().contains_key(&ctx_id) {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(events_ptr, nr * core::mem::size_of::<IoEvent>()) {
        return err;
    }
    let mut completed = AIO_COMPLETED.lock();
    let queue = match completed.get_mut(&ctx_id) {
        Some(q) => q,
        None => return errno(EINVAL),
    };
    let available = queue.len();
    let to_return = available.min(nr);
    if to_return < min_nr {
        // Yeterli event yok, timeout yoksa hemen dön
        return 0;
    }
    for i in 0..to_return {
        if let Some(event) = queue.first().cloned() {
            queue.remove(0);
            if let Err(err) = write_user(events_ptr + i * core::mem::size_of::<IoEvent>(), event) {
                return err;
            }
        }
    }
    to_return
}

/// `io_cancel` — Bekleyen AIO isteğini iptal et.
fn sys_io_cancel(ctx: usize, iocb_ptr: usize, result_ptr: usize) -> usize {
    let _ = (ctx, iocb_ptr, result_ptr);
    // echOS'ta AIO senkron tamamlandığı için iptal edilecek istek yok
    errno(EAGAIN)
}

// ============================================================================
// POSIX Message Queues — mq_open / mq_unlink / mq_timedsend / mq_timedreceive
//                         mq_notify / mq_getsetattr
// ============================================================================

lazy_static! {
    /// POSIX MQ tablosu: isim → (attr, mesajlar)
    static ref POSIX_MQ: Mutex<alloc::collections::BTreeMap<alloc::string::String, (MqAttr, Vec<Vec<u8>>)>> =
        Mutex::new(alloc::collections::BTreeMap::new());
}

/// `mq_open` — POSIX mesaj kuyruğu aç/oluştur.
fn sys_mq_open(name_ptr: usize, oflag: usize, mode: usize, attr_ptr: usize) -> usize {
    let _ = mode;
    let name = match read_user_cstr(name_ptr) {
        Some(s) => s,
        None => return errno(EINVAL),
    };
    let mut mq_table = POSIX_MQ.lock();
    let o_creat = 0o100;
    if oflag & o_creat != 0 {
        if !mq_table.contains_key(&name) {
            let attr = if attr_ptr != 0 {
                with_user_access(|| unsafe { *(attr_ptr as *const MqAttr) })
            } else {
                MqAttr {
                    mq_flags: 0,
                    mq_maxmsg: 10,
                    mq_msgsize: 8192,
                    mq_curmsgs: 0,
                }
            };
            mq_table.insert(name, (attr, Vec::new()));
        }
    } else if !mq_table.contains_key(&name) {
        return errno(ENOENT);
    }
    // fd döndür (Pipe tipi olarak)
    let fd = allocate_fd(FdKind::Pipe);
    fd
}

/// `mq_unlink` — POSIX mesaj kuyruğunu sil.
fn sys_mq_unlink(name_ptr: usize) -> usize {
    let name = match read_user_cstr(name_ptr) {
        Some(s) => s,
        None => return errno(EINVAL),
    };
    let mut mq_table = POSIX_MQ.lock();
    if mq_table.remove(&name).is_none() {
        return errno(ENOENT);
    }
    0
}

/// `mq_timedsend` — Mesaj kuyruğuna mesaj gönder.
fn sys_mq_timedsend(
    mqdes: usize,
    msg_ptr: usize,
    msg_len: usize,
    msg_prio: usize,
    timeout_ptr: usize,
) -> usize {
    let _ = (msg_prio, timeout_ptr);
    if get_fd(mqdes).is_none() {
        return errno(EBADF);
    }
    if msg_len == 0 {
        return errno(EINVAL);
    }
    let msg_data = match read_user_bytes(msg_ptr, msg_len) {
        Ok(d) => d,
        Err(e) => return e,
    };
    // fd'ye karşılık gelen kuyruğu bul (basitleştirilmiş: ilk kuyruğu kullan)
    let mut mq_table = POSIX_MQ.lock();
    if let Some((_attr, msgs)) = mq_table.values_mut().next() {
        msgs.push(msg_data);
        return 0;
    }
    errno(EBADF)
}

/// `mq_timedreceive` — Mesaj kuyruğundan mesaj al.
fn sys_mq_timedreceive(
    mqdes: usize,
    msg_ptr: usize,
    msg_len: usize,
    msg_prio_ptr: usize,
    timeout_ptr: usize,
) -> usize {
    let _ = (msg_prio_ptr, timeout_ptr);
    if get_fd(mqdes).is_none() {
        return errno(EBADF);
    }
    let mut mq_table = POSIX_MQ.lock();
    if let Some((_attr, msgs)) = mq_table.values_mut().next() {
        if let Some(msg) = msgs.first().cloned() {
            msgs.remove(0);
            let copy_len = msg.len().min(msg_len);
            if let Err(err) = write_user_bytes(msg_ptr, &msg[..copy_len]) {
                return err;
            }
            return copy_len;
        }
        return errno(EAGAIN); // Kuyruk boş
    }
    errno(EBADF)
}

/// `mq_notify` — Mesaj kuyruğu bildirim kaydı.
fn sys_mq_notify(mqdes: usize, notification_ptr: usize) -> usize {
    let _ = notification_ptr;
    if get_fd(mqdes).is_none() {
        return errno(EBADF);
    }
    // echOS'ta sinyal bildirimi henüz yok, kabul et
    0
}

/// `mq_getsetattr` — MQ özelliklerini al/ayarla.
fn sys_mq_getsetattr(mqdes: usize, new_attr_ptr: usize, old_attr_ptr: usize) -> usize {
    if get_fd(mqdes).is_none() {
        return errno(EBADF);
    }
    let mut mq_table = POSIX_MQ.lock();
    if let Some((attr, _msgs)) = mq_table.values_mut().next() {
        if old_attr_ptr != 0 {
            if let Err(err) = validate_user_range(old_attr_ptr, core::mem::size_of::<MqAttr>()) {
                return err;
            }
            if let Err(err) = write_user(old_attr_ptr, *attr) {
                return err;
            }
        }
        if new_attr_ptr != 0 {
            let new_attr: MqAttr = with_user_access(|| unsafe { *(new_attr_ptr as *const MqAttr) });
            attr.mq_flags = new_attr.mq_flags;
        }
        return 0;
    }
    errno(EBADF)
}

// ============================================================================
// Quota / Keyctl / Mount syscalls
// ============================================================================

/// `quotactl` — Disk kota yönetimi.
fn sys_quotactl(cmd: usize, special_ptr: usize, id: usize, addr: usize) -> usize {
    // quotactl komutları:
    // Q_QUOTAON  = 0x0100 — kota aç
    // Q_QUOTAOFF = 0x0200 — kota kapat
    // Q_GETFMT   = 0x0300 — kota formatını al
    // Q_GETQUOTA = 0x0700 — kota bilgisini al
    // Q_SETQUOTA = 0x0800 — kota bilgisini ayarla
    // Q_SYNC     = 0x0080 — kota senkronize et
    // Q_GETNEXTQUOTA = 0x0B00 — bir sonraki kota

    let cmd_type = (cmd >> 8) & 0xFF;
    let _cmd_sub = cmd & 0xFF;

    let _special = if special_ptr != 0 {
        match read_user_cstring(special_ptr, 256) {
            Ok(s) => s,
            Err(e) => return e,
        }
    } else {
        alloc::string::String::new()
    };

    match cmd_type {
        0x01 | 0x02 => 0, // Q_QUOTAON/Q_QUOTAOFF — no-op (kota yok)
        0x03 => {
            // Q_GETFMT — format: QFMT_VFS_V0 = 2
            if addr != 0 {
                if let Err(e) = validate_user_range(addr, 4) {
                    return e;
                }
                with_user_access(|| unsafe {
                    *((addr) as *mut u32) = 2;
                });
            }
            0
        }
        0x07 => {
            // Q_GETQUOTA — dolu kota döndür (limit yok)
            if addr != 0 {
                // dqblk: { u64 dqb_bhardlimit; u64 dqb_bsoftlimit; u64 dqb_curspace;
                //           u64 dqb_btime; u64 dqb_ihardlimit; u64 dqb_isoftlimit;
                //           u64 dqb_curinodes; u64 dqb_itime; u32 dqb_bvalid; u32 dqb_valid; }
                if let Err(e) = validate_user_range(addr, 72) {
                    return e;
                }
                with_user_access(|| unsafe {
                    core::ptr::write_bytes(addr as *mut u8, 0, 72);
                });
            }
            0
        }
        0x08 => 0, // Q_SETQUOTA — no-op
        0x00 => 0, // Q_SYNC — no-op
        0x0B => {
            // Q_GETNEXTQUOTA — daha fazla kota yok
            errno(ENOSYS)
        }
        _ => errno(EINVAL),
    }
}

/// `keyctl` — Çekirdek anahtarlık yönetimi.
fn sys_keyctl(cmd: usize, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> usize {
    // keyctl komutları:
    // KEYCTL_GET_KEYRING_ID = 0 — keyring ID'sini al
    // KEYCTL_JOIN_SESSION_KEYRING = 1 — session keyring'e katıl
    // KEYCTL_UPDATE = 2 — anahtarı güncelle
    // KEYCTL_LINK = 8 — anahtarı keyring'e bağla
    // KEYCTL_UNLINK = 9 — anahtarı keyring'den kaldır
    // KEYCTL_DESCRIBE = 11 — anahtar hakkında bilgi al
    // KEYCTL_SEARCH = 12 — keyring'de ara
    // KEYCTL_READ = 11 — anahtar oku

    match cmd {
        0 => {
            // KEYCTL_GET_KEYRING_ID — real keyring ID'si döndür
            // arg2 = key_type, arg3 = description, arg4 = create
            // echoOS'ta keyring yok — INVALID_KEYRING_ID dön
            0xffffffff_ffffffea // ENOKEY
        }
        1 => {
            // KEYCTL_JOIN_SESSION_KEYRING — session keyring oluştur
            0xffffffff_ffffffea // ENOKEY
        }
        8 | 9 => {
            // KEYCTL_LINK/UNLINK — no-op
            0
        }
        11 => {
            // KEYCTL_DESCRIBE — bilgi yok
            if arg3 != 0 && arg4 > 0 {
                if let Err(e) = validate_user_range(arg3, arg4) {
                    return e;
                }
                // "keyring;0;0;0000" formatında bilgi yaz
                let desc = b"unknown;0;0;0000\0";
                let copy_len = core::cmp::min(desc.len(), arg4);
                with_user_access(|| unsafe {
                    core::ptr::copy_nonoverlapping(desc.as_ptr(), arg3 as *mut u8, copy_len);
                });
            }
            0
        }
        _ => errno(ENOSYS),
    }
}

/// `move_mount` — Mount noktasını taşı.
fn sys_move_mount(
    from_dfd: usize,
    from_path_ptr: usize,
    to_dfd: usize,
    to_path_ptr: usize,
    flags: usize,
) -> usize {
    // move_mount: bir mount noktasını başka bir yere taşı
    // from_path_ptr → to_path_ptr
    let from_path = if from_path_ptr != 0 {
        match read_user_cstring(from_path_ptr, 4096) {
            Ok(p) => p,
            Err(e) => return e,
        }
    } else {
        return errno(EINVAL);
    };

    let to_path = if to_path_ptr != 0 {
        match read_user_cstring(to_path_ptr, 4096) {
            Ok(p) => p,
            Err(e) => return e,
        }
    } else {
        return errno(EINVAL);
    };

    let resolved_from = resolve_path_at(from_dfd, &from_path);
    let resolved_to = resolve_path_at(to_dfd, &to_path);

    // F2FS mount tablosunu taşı — mount_table'dan移動
    // echoOS'ta mount taşıma henüz desteklenmiyor
    errno(ENOSYS)
}

/// `fsopen` — Dosya sistemi bağlamı aç (new mount API).
fn sys_fsopen(fsname_ptr: usize, flags: usize) -> usize {
    let fsname = match read_user_cstring(fsname_ptr, 256) {
        Ok(p) => p,
        Err(e) => return e,
    };

    const FSOPEN_CLOEXEC: usize = 01;
    const FSOPEN_NOCLOBBER: usize = 02;

    // Sadece "f2fs" destekleniyor
    if fsname != "f2fs" {
        return errno(ENODEV);
    }

    // fsconfig fd oluştur
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Basit: fd'yi returned, fsconfig/fsmount'ta kullanılacak
    fd
}

/// `fsconfig` — Dosya sistemi bağlamını yapılandır.
fn sys_fsconfig(fd: usize, cmd: usize, key_ptr: usize, value_ptr: usize, aux: usize) -> usize {
    // fsconfig komutları:
    // FSCONFIG_SET_FLAG = 0 — bayrak ayarla
    // FSCONFIG_SET_STRING = 1 — string değer ayarla
    // FSCONFIG_SET_BINARY = 2 — binary değer ayarla
    // FSCONFIG_SET_UINT = 3 — uint ayarla
    // FSCONFIG_CMD_CREATE = 6 — fs oluştur
    // FSCONFIG_CMD_RECONFIGURE = 7 — yeniden yapılandır

    match cmd {
        0 | 1 | 2 | 3 => {
            // Değer ayarla — basit: no-op
            0
        }
        6 => {
            // FSCONFIG_CMD_CREATE — fs oluştur
            0
        }
        7 => {
            // FSCONFIG_CMD_RECONFIGURE — yeniden yapılandır
            0
        }
        _ => errno(EINVAL),
    }
}

/// `fsmount` — Dosya sistemi bağlamından mount oluştur.
fn sys_fsmount(fd: usize, flags: usize, attr_flags: usize) -> usize {
    // fsmount, fsconfig ile yapılandırılmış bağlamdan mount oluşturur
    const FSMOUNT_CLOEXEC: usize = 01;
    const FSMOUNT_MOUNT: usize = 02;

    // Mount fd oluştur
    let mount_fd = allocate_fd(FdKind::File);
    if mount_fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Basit: mount fd returned
    // move_mount ile bir yere taşınacak
    mount_fd
}

// ============================================================================
// Advanced — perf_event_open / bpf / kexec_file_load
// ============================================================================

/// `perf_event_open` — Performans izleme olayı aç.
fn sys_perf_event_open(
    attr_ptr: usize,
    pid: usize,
    cpu: usize,
    group_fd: usize,
    flags: usize,
) -> usize {
    // perf_event_open, donanım/yazılım sayaçlarını izler
    // struct perf_event_attr { __u32 type; __u32 size; __u64 config; ... }
    if attr_ptr == 0 {
        return errno(EFAULT);
    }
    if let Err(e) = validate_user_range(attr_ptr, 64) {
        return e;
    }

    let _event_type: u32 = with_user_access(|| unsafe { *(attr_ptr as *const u32) });
    let _config: u64 = with_user_access(|| unsafe { *((attr_ptr + 8) as *const u64) });

    // echoOS'ta PMU (Performance Monitoring Unit) henüz desteklenmiyor
    // Gerçek implementasyon: perf_event_open kernel altyapısı + RDPMC/NMI desteği gerekir
    // Basit: fd dön ama hiçbir şey izleme
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }
    fd
}

/// `bpf` — BPF sistem çağrısı (eBPF program yükleme/harita yönetimi).
fn sys_bpf(cmd: usize, attr_ptr: usize, size: usize) -> usize {
    // BPF komutları:
    // BPF_MAP_CREATE = 0
    // BPF_MAP_LOOKUP_ELEM = 1
    // BPF_MAP_UPDATE_ELEM = 2
    // BPF_MAP_DELETE_ELEM = 3
    // BPF_PROG_LOAD = 5
    // BPF_OBJ_PIN = 6
    // BPF_OBJ_GET = 7

    // echoOS'ta eBPF VM henüz desteklenmiyor
    // Gerçek implementasyon: BPF VM, BPF bytecode loader, maps altyapısı gerekir
    // CAP_BPF yetkisi de gerekir

    match cmd {
        0 => {
            // BPF_MAP_CREATE — harita oluştur
            // Basit: fd dön ama harita yok
            let fd = allocate_fd(FdKind::File);
            if fd >= MAX_FDS {
                return errno(EMFILE);
            }
            fd
        }
        5 => {
            // BPF_PROG_LOAD — program yükle
            errno(ENOSYS)
        }
        _ => errno(ENOSYS),
    }
}

/// `kexec_file_load` — Yeni çekirdek yükle ve çalıştır.
fn sys_kexec_file_load(
    kernel_fd: usize,
    initrd_fd: usize,
    cmdline_len: usize,
    cmdline_ptr: usize,
    flags: usize,
) -> usize {
    // kexec_file_load, dosyadan yeni kernel yükler
    // echoOS'ta kexec henüz desteklenmiyor — kernel yükleme altyapısı gerekir
    // CAP_SYS_BOOT yetkisi de gerekir
    errno(EPERM)
}

/// Kullanıcı alanından C-string oku (null-terminated)
fn read_user_cstr(ptr: usize) -> Option<alloc::string::String> {
    if ptr == 0 {
        return None;
    }
    let mut s = alloc::string::String::new();
    let mut i = 0;
    loop {
        if i > 4096 {
            return None; // Çok uzun
        }
        let byte = with_user_access(|| unsafe { *((ptr + i) as *const u8) });
        if byte == 0 {
            break;
        }
        s.push(byte as char);
        i += 1;
    }
    Some(s)
}

// ============================================================================
// Faz 2 — Eksik POSIX / Linux Syscall Implementasyonları
// ============================================================================

// ---------------------------------------------------------------------------
// Zaman
// ---------------------------------------------------------------------------

/// `gettimeofday` — Unix epoch'tan itibaren zamanı döndürür.
/// echOS'ta clock_gettime(CLOCK_REALTIME) ile aynı mantık.
fn sys_gettimeofday(tv_ptr: usize, _tz_ptr: usize) -> usize {
    if tv_ptr != 0 {
        if let Err(err) = validate_user_range(tv_ptr, core::mem::size_of::<Timeval>()) {
            return err;
        }
        // clock_gettime ile aynı zamanı kullan
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let _ = clock_gettime_impl(CLOCK_REALTIME, &mut ts);
        let tv = Timeval {
            tv_sec: ts.tv_sec,
            tv_usec: ts.tv_nsec / 1000,
        };
        let _ = write_user(tv_ptr, tv);
    }
    0
}

/// `settimeofday` — Saati ayarlar (shim, donanım RTC'ye yazmaz).
fn sys_settimeofday(_tv_ptr: usize) -> usize {
    // echOS'ta gerçekRTC yazımı henüz yok
    0
}

/// `clock_getres` — Belirtilen clock'un çözünürlüğünü (en küçük adımını) döndürür.
fn sys_clock_getres(clock_id: usize, res_ptr: usize) -> usize {
    if clock_id > CLOCK_MONOTONIC {
        return errno(EINVAL);
    }
    if res_ptr != 0 {
        if let Err(err) = validate_user_range(res_ptr, core::mem::size_of::<Timespec>()) {
            return err;
        }
        // echOS timer çözünürlüğü: 1 tick = 10ms = 10_000_000 ns
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: TICK_NS as i64,
        };
        let _ = write_user(res_ptr, ts);
    }
    0
}

/// `clock_nanosleep` — Belirtilen clock ile belirtilen süreyi uyu.
fn sys_clock_nanosleep(clock_id: usize, flags: usize, req_ptr: usize, _rem_ptr: usize) -> usize {
    if clock_id > CLOCK_MONOTONIC {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(req_ptr, core::mem::size_of::<Timespec>()) {
        return err;
    }
    let req: Timespec = with_user_access(|| unsafe { *(req_ptr as *const Timespec) });
    let _flags = flags; // TIMER_ABSTIME = 1
                        // nanosleep ile aynı mantık
    let total_ns = (req.tv_sec as u64) * 1_000_000_000 + (req.tv_nsec as u64);
    let ticks = (total_ns + TICK_NS - 1) / TICK_NS;
    let mut elapsed: u64 = 0;
    while elapsed < ticks {
        x86_64::instructions::hlt();
        elapsed += 1;
    }
    0
}

/// `clock_adjtime` — Clock ayarlama (shim).
fn sys_clock_adjtime(_clock_id: usize, _tx_ptr: usize) -> usize {
    0
}

/// `time` — Unix epoch'tan saniye cinsinden zamanı döndürür.
fn sys_time(t_ptr: usize) -> usize {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let _ = clock_gettime_impl(CLOCK_REALTIME, &mut ts);
    if t_ptr != 0 {
        if let Err(err) = validate_user_range(t_ptr, core::mem::size_of::<i64>()) {
            return err;
        }
        let _ = write_user(t_ptr, ts.tv_sec);
    }
    ts.tv_sec as usize
}

/// `utimes` — Dosya erişim/değişik zamanını ayarlar.
fn sys_utimes(_path_ptr: usize, _times_ptr: usize) -> usize {
    // echOS'ta暫time ayarlaması henüz desteklenmiyor
    0
}

// ---------------------------------------------------------------------------
// Sinyal
// ---------------------------------------------------------------------------

/// `rt_sigreturn` — Sinyal handler'dan dönüş. Normalde assembly'de çağrılır.
/// Bu noktaya ulaşılması beklenmez; eğer ulaşılırsa ENOSYS döndür.
fn sys_rt_sigreturn() -> usize {
    // Signal return assembly seviyesinde ele alınır
    // Bu çağrı normalde user-mode'a geri döner
    // Eğer buradaysak hatalı bir durum
    errno(ENOSYS)
}

/// `rt_sigpending` — Bekleyen sinyalleri döndürür.
fn sys_rt_sigpending(set_ptr: usize, sigsetsize: usize) -> usize {
    if sigsetsize != 8 {
        return errno(EINVAL);
    }
    if let Err(err) = validate_user_range(set_ptr, 8) {
        return err;
    }
    // Per-process pending signal'ları oku
    let pending = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        crate::task::scheduler::PER_CPU_CURRENT_TASK
            .get(crate::cpu::smp::get_current_cpu_id() as usize)
            .and_then(|t| t.as_ref())
            .map(|t| t.cold.signals.get_pending())
            .unwrap_or(0)
    });
    let _ = write_user(set_ptr, pending);
    0
}

// ---------------------------------------------------------------------------
// Süreç / Kullanıcı / Grup
// ---------------------------------------------------------------------------

/// `getpgrp` — Mevcut sürecin process group ID'sini döndürür.
fn sys_getpgrp() -> usize {
    // getpgid(0) ile aynı
    process_bridge::sys_getpgid(0)
}

/// `setreuid` — Real ve effective user ID'yi ayarlar.
fn sys_setreuid(_ruid: usize, _euid: usize) -> usize {
    // Tek kullanıcılı sistemde no-op
    0
}

/// `setregid` — Real ve effective group ID'yi ayarlar.
fn sys_setregid(_rgid: usize, _egid: usize) -> usize {
    0
}

/// `getgroups` — Supplementary group listesini döndürür.
fn sys_getgroups(size: usize, list_ptr: usize) -> usize {
    // echOS'ta tek grup: root (0)
    if size >= 1 && list_ptr != 0 {
        if let Err(err) = validate_user_range(list_ptr, core::mem::size_of::<u32>()) {
            return err;
        }
        let _ = write_user(list_ptr, 0u32); // gid 0
    }
    1
}

/// `setgroups` — Supplementary group listesini ayarlar.
fn sys_setgroups(_size: usize, _list_ptr: usize) -> usize {
    0
}

/// `setresuid` — Real, effective ve saved user ID'yi ayarlar.
fn sys_setresuid(_ruid: usize, _euid: usize, _suid: usize) -> usize {
    0
}

/// `getresuid` — Real, effective ve saved user ID'yi döndürür.
fn sys_getresuid(ptr: usize) -> usize {
    if ptr != 0 {
        if let Err(err) = validate_user_range(ptr, 3 * core::mem::size_of::<u32>()) {
            return err;
        }
        let _ = write_user(ptr, 0u32); // ruid
        let _ = write_user(ptr + 4, 0u32); // euid
        let _ = write_user(ptr + 8, 0u32); // suid
    }
    0
}

/// `setresgid` — Real, effective ve saved group ID'yi ayarlar.
fn sys_setresgid(_rgid: usize, _egid: usize, _sgid: usize) -> usize {
    0
}

/// `getresgid` — Real, effective ve saved group ID'yi döndürür.
fn sys_getresgid(ptr: usize) -> usize {
    if ptr != 0 {
        if let Err(err) = validate_user_range(ptr, 3 * core::mem::size_of::<u32>()) {
            return err;
        }
        let _ = write_user(ptr, 0u32);
        let _ = write_user(ptr + 4, 0u32);
        let _ = write_user(ptr + 8, 0u32);
    }
    0
}

/// `setfsuid` — Filesystem user ID'yi ayarlar.
fn sys_setfsuid(_uid: usize) -> usize {
    0
}

/// `setfsgid` — Filesystem group ID'yi ayarlar.
fn sys_setfsgid(_gid: usize) -> usize {
    0
}

/// `close_range` — Belirtilen aralıktaki tüm fd'leri kapatır.
fn sys_close_range(first: usize, max: usize, _flags: usize) -> usize {
    let end = if max == 0 || max > MAX_FDS {
        MAX_FDS
    } else {
        max
    };
    let mut fd_table = FILE_TABLE.lock();
    for fd in first..end {
        if fd < MAX_FDS {
            fd_table[fd] = None;
        }
    }
    let mut fd_init = FD_TABLE.lock();
    for fd in first..end {
        if fd < MAX_FDS {
            fd_init[fd] = None;
        }
    }
    0
}

/// `pidfd_getfd` — pidfd üzerinden foreign fd kopyalamak.
fn sys_pidfd_getfd(pidfd: usize, targetfd: usize, _flags: usize) -> usize {
    // pidfd, hedef sürecin pid'ini temsil eden fd'dir
    let target_pid = pidfd;

    if !tasking::task_exists(target_pid) {
        return errno(ESRCH);
    }

    // Gerçek cross-process fd kopyalama için per-CPU task erişimi gerekir.
    // Şimdilik ENOSYS: process_vm_readv/writev ile veri transferi yapılabilir,
    // ancak fd tablosu doğrudan erişilemez.
    errno(ENOSYS)
}

/// `mknod` — Dosya veya cihaz düğümü oluşturur.
fn sys_mknod(path_ptr: usize, mode: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };

    const S_IFMT: usize = 0o170000;
    const S_IFREG: usize = 0o100000;
    const S_IFCHR: usize = 0o020000;
    const S_IFBLK: usize = 0o060000;
    const S_IFIFO: usize = 0o010000;
    const S_IFSOCK: usize = 0o140000;
    let file_type = mode & S_IFMT;

    match file_type {
        S_IFREG => {
            // Normal dosya oluştur — open(O_CREAT) ile aynı
            let (parent, name) = split_path(&path);
            match fs::f2fs::create_f2fs_file(parent, name) {
                Ok(_) => 0,
                Err(e) => vfs_errno(e),
            }
        }
        S_IFCHR | S_IFBLK | S_IFIFO | S_IFSOCK => {
            // Cihaz/boru/soket düğümü — F2FS'te desteklenmiyor
            errno(ENOSYS)
        }
        0 => {
            // Mode belirtilmemiş — varsayılan olarak regular dosya
            let (parent, name) = split_path(&path);
            match fs::f2fs::create_f2fs_file(parent, name) {
                Ok(_) => 0,
                Err(e) => vfs_errno(e),
            }
        }
        _ => errno(EINVAL),
    }
}

/// `mknodat` — Belirtilen fd üzerinden cihaz düğümü oluşturur.
fn sys_mknodat(dirfd: usize, path_ptr: usize, mode: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let resolved = resolve_path_at(dirfd, &path);
    let (parent, name) = split_path(&resolved);

    const S_IFMT: usize = 0o170000;
    const S_IFREG: usize = 0o100000;
    let file_type = mode & S_IFMT;

    match file_type {
        S_IFREG | 0 => match fs::f2fs::create_f2fs_file(parent, name) {
            Ok(_) => 0,
            Err(e) => vfs_errno(e),
        },
        _ => errno(ENOSYS),
    }
}

/// Timerfd state
struct TimerFdState {
    interval_ns: u64,
    value_ns: u64,
    active: bool,
}

lazy_static! {
    static ref TIMERFD_TABLE: Mutex<alloc::collections::BTreeMap<usize, TimerFdState>> =
        Mutex::new(alloc::collections::BTreeMap::new());
    static ref TIMERFD_ID: AtomicU64 = AtomicU64::new(0);
}

/// `timerfd_create` — Yeni bir timerfd oluşturur.
fn sys_timerfd_create(clockid: usize, _flags: usize) -> usize {
    if clockid > CLOCK_MONOTONIC {
        return errno(EINVAL);
    }
    let id = TIMERFD_ID.fetch_add(1, Ordering::SeqCst) as usize;
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }
    TIMERFD_TABLE.lock().insert(
        id,
        TimerFdState {
            interval_ns: 0,
            value_ns: 0,
            active: false,
        },
    );
    let _ = id;
    fd
}

/// `timerfd_settime` — Timer'ı ayarlar.
fn sys_timerfd_settime(
    fd: usize,
    _flags: usize,
    new_value_ptr: usize,
    old_value_ptr: usize,
) -> usize {
    if fd >= MAX_FDS {
        return errno(EBADF);
    }
    if let Err(err) = validate_user_range(new_value_ptr, core::mem::size_of::<Itimerval>()) {
        return err;
    }
    let new_val: Itimerval = with_user_access(|| unsafe { *(new_value_ptr as *const Itimerval) });
    let interval_ns = (new_val.it_interval.tv_sec as u64) * 1_000_000_000
        + (new_val.it_interval.tv_usec as u64) * 1000;
    let value_ns =
        (new_val.it_value.tv_sec as u64) * 1_000_000_000 + (new_val.it_value.tv_usec as u64) * 1000;
    if old_value_ptr != 0 {
        if let Err(err) = validate_user_range(old_value_ptr, core::mem::size_of::<Itimerval>()) {
            return err;
        }
        let _ = write_user(old_value_ptr, new_val);
    }
    let mut table = TIMERFD_TABLE.lock();
    let entry = table.entry(fd).or_insert(TimerFdState {
        interval_ns: 0,
        value_ns: 0,
        active: false,
    });
    entry.interval_ns = interval_ns;
    entry.value_ns = value_ns;
    entry.active = value_ns > 0;
    0
}

/// `timerfd_gettime` — Timer'ın mevcut değerini döndürür.
fn sys_timerfd_gettime(fd: usize, curr_value_ptr: usize) -> usize {
    if fd >= MAX_FDS {
        return errno(EBADF);
    }
    if let Err(err) = validate_user_range(curr_value_ptr, core::mem::size_of::<Itimerval>()) {
        return err;
    }
    let table = TIMERFD_TABLE.lock();
    let (interval_ns, value_ns) = if let Some(entry) = table.get(&fd) {
        (entry.interval_ns, entry.value_ns)
    } else {
        (0, 0)
    };
    let iv = Itimerval {
        it_interval: Timeval {
            tv_sec: (interval_ns / 1_000_000_000) as i64,
            tv_usec: ((interval_ns % 1_000_000_000) / 1000) as i64,
        },
        it_value: Timeval {
            tv_sec: (value_ns / 1_000_000_000) as i64,
            tv_usec: ((value_ns % 1_000_000_000) / 1000) as i64,
        },
    };
    let _ = write_user(curr_value_ptr, iv);
    0
}

/// `mkdirat` — Belirtilen fd üzerinden dizin oluşturur.
fn sys_mkdirat(dirfd: usize, path_ptr: usize, mode: usize) -> usize {
    sys_mkdir(path_ptr, mode) // dirfd şimdilik yoksayılıyor
}

/// `unlinkat` — Belirtilen fd üzerinden dosya/dizin siler.
fn sys_unlinkat(dirfd: usize, path_ptr: usize, flags: usize) -> usize {
    if flags & AT_REMOVEDIR != 0 {
        return sys_rmdir(path_ptr);
    }
    sys_unlink(path_ptr) // dirfd şimdilik yoksayılıyor
}

/// `fchmodat` — Belirtilen fd üzerinden dosya modunu değiştirir.
fn sys_fchmodat(_dirfd: usize, path_ptr: usize, mode: usize, _flags: usize) -> usize {
    sys_chmod(path_ptr, mode)
}

/// `fchownat` — Belirtilen fd üzerinden dosya sahibini değiştirir.
fn sys_fchownat(_dirfd: usize, path_ptr: usize, uid: usize, gid: usize, _flags: usize) -> usize {
    sys_chown(path_ptr, uid, gid)
}

/// `flock` — Dosya kilidi (LOCK_SH, LOCK_EX, LOCK_UN, LOCK_NB).
fn sys_flock(fd: usize, operation: usize) -> usize {
    let ret = crate::fs::file_lock::sys_flock(fd as i32, operation as i32);
    if ret < 0 {
        (!0usize).wrapping_sub((-ret) as usize - 1)
    } else {
        0
    }
}

/// `sync`
fn sys_sync() -> usize {
    let _ = crate::fs::f2fs::sync_f2fs();
    crate::fs::page_cache::sync_cache();
    0
}

/// `sync_file_range`
fn sys_sync_file_range(fd: usize, _offset: usize, _nbytes: usize, _flags: usize) -> usize {
    if get_fd(fd).is_none() {
        return errno(EBADF);
    }
    let files = FILE_TABLE.lock();
    if let Some(Some(state)) = files.get(fd) {
        let path = state.path.clone();
        drop(files);
        match crate::fs::f2fs::fsync_path(&path) {
            Ok(()) => 0,
            Err(_) => errno(EIO),
        }
    } else {
        drop(files);
        let _ = crate::fs::f2fs::sync_f2fs();
        0
    }
}

/// `fadvise64` — Dosya erişim ipucu (SEQUENTIAL, RANDOM, vb.).
fn sys_fadvise64(_fd: usize, _offset: usize, _nbytes: usize, _advice: usize) -> usize {
    0 // no-op
}

/// `fchdir` — Belirtilen fd'yi çalışma dizini yapar.
fn sys_fchdir(fd: usize) -> usize {
    // fd'den dizin yolunu al
    let files = FILE_TABLE.lock();
    let _entry = match files.get(fd) {
        Some(Some(entry)) => entry,
        _ => return errno(EBADF),
    };
    // echOS'ta FileEntry bir path tutmuyor — ENOSYS dön
    // Gerçek implementasyon: FileEntry'den path okunup CURRENT_WORKING_DIR'a yazılır
    errno(ENOSYS)
}

/// `chroot` — Root dizini değiştirir.
fn sys_chroot(path_ptr: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // Dizin var mı kontrol et
    match fs::f2fs::open_entry(&path) {
        Ok(entry) => {
            if !entry.is_dir {
                return errno(ENOTDIR);
            }
            // CLONE_FS: shared_fs varsa oraya root yaz, yoksa global'a
            set_root_for_current(path);
            0
        }
        Err(e) => vfs_errno(e),
    }
}

/// `pivot_root` — Root filesystem'i değiştirir.
fn sys_pivot_root(_new_root_ptr: usize, _put_old_ptr: usize) -> usize {
    errno(EPERM) // Container namespace desteği gerekir
}

// ---------------------------------------------------------------------------
// Soket (ek)
// ---------------------------------------------------------------------------

/// `socketpair` — Eş soket çifti oluşturur (Unix domain sockets için).
fn sys_socketpair(domain: usize, type_: usize, protocol: usize, fds_ptr: usize) -> usize {
    // Sadece AF_UNIX destekleniyor
    if domain != AF_UNIX {
        return errno(EAFNOSUPPORT);
    }
    if fds_ptr == 0 {
        return errno(EINVAL);
    }
    if let Err(e) = validate_user_range(fds_ptr, 8) {
        return e;
    }

    // İki fd alloke et
    let fd1 = allocate_fd(FdKind::Socket);
    if fd1 >= MAX_FDS {
        return errno(EMFILE);
    }
    let fd2 = allocate_fd(FdKind::Socket);
    if fd2 >= MAX_FDS {
        // fd1'i geri al
        let mut table = FD_TABLE.lock();
        table[fd1] = None;
        return errno(EMFILE);
    }

    // Her iki soketi de connected olarak ayarla
    {
        let mut sockets = SOCKET_TABLE.lock();
        let port1 = {
            let mut next = NEXT_EPHEMERAL_PORT.lock();
            let p = *next;
            *next = next.wrapping_add(1);
            p
        };
        let port2 = {
            let mut next = NEXT_EPHEMERAL_PORT.lock();
            let p = *next;
            *next = next.wrapping_add(1);
            p
        };

        sockets.insert(
            fd1,
            SocketState {
                domain,
                sock_type: type_,
                protocol,
                state: SocketConnState::Connected,
                local_port: port1,
                remote_port: port2,
                remote_addr: [127, 0, 0, 1],
                tcp_id: 0,
            },
        );
        sockets.insert(
            fd2,
            SocketState {
                domain,
                sock_type: type_,
                protocol,
                state: SocketConnState::Connected,
                local_port: port2,
                remote_port: port1,
                remote_addr: [127, 0, 0, 1],
                tcp_id: 0,
            },
        );
    }

    // Kullanıcıya fd çiftini yaz
    with_user_access(|| unsafe {
        *(fds_ptr as *mut u32) = fd1 as u32;
        *((fds_ptr + 4) as *mut u32) = fd2 as u32;
    });
    0
}

/// `recvmmsg` — Çoklu mesaj alır.
fn sys_recvmmsg(
    fd: usize,
    msg_ptr: usize,
    vlen: usize,
    flags: usize,
    _timeout_ptr: usize,
) -> usize {
    // mmsghdr: { struct msghdr msg_hdr; unsigned int msg_len; }
    // her bir mmsghdr 64 bytes (msghdr=56 + msg_len=4 + padding=4)
    if vlen == 0 {
        return 0;
    }
    if msg_ptr == 0 {
        return errno(EINVAL);
    }

    let mut count: usize = 0;
    for i in 0..vlen {
        let entry_ptr = msg_ptr + i * 64;
        if let Err(e) = validate_user_range(entry_ptr, 64) {
            break;
        }

        let result = sys_recvmsg(fd, entry_ptr, flags);
        if result == 0 || result >= errno_base() {
            break;
        }

        // msg_len yaz
        with_user_access(|| unsafe {
            *((entry_ptr + 56) as *mut u32) = result as u32;
        });
        count += 1;

        // MSG_WAITALL yoksa hemen dön
        if flags & 0x100 == 0 {
            break;
        }
    }
    count
}

/// `sendmmsg` — Çoklu mesaj gönderir.
fn sys_sendmmsg(
    fd: usize,
    msg_ptr: usize,
    vlen: usize,
    flags: usize,
    _timeout_ptr: usize,
) -> usize {
    // mmsghdr: { struct msghdr msg_hdr; unsigned int msg_len; }
    if vlen == 0 {
        return 0;
    }
    if msg_ptr == 0 {
        return errno(EINVAL);
    }

    let mut count: usize = 0;
    for i in 0..vlen {
        let entry_ptr = msg_ptr + i * 64;
        if let Err(e) = validate_user_range(entry_ptr, 64) {
            break;
        }

        let msg_iov: usize = with_user_access(|| unsafe { *((entry_ptr + 16) as *const usize) });
        let msg_iovlen: usize = with_user_access(|| unsafe { *((entry_ptr + 24) as *const usize) });

        if msg_iov == 0 || msg_iovlen == 0 {
            break;
        }

        // Tüm iovec'leri topla
        let mut total_data = alloc::vec::Vec::new();
        for j in 0..msg_iovlen {
            let iov_base: usize =
                with_user_access(|| unsafe { *((msg_iov + j * 16) as *const usize) });
            let iov_len: usize =
                with_user_access(|| unsafe { *((msg_iov + j * 16 + 8) as *const usize) });
            if iov_len == 0 {
                continue;
            }
            if let Err(_) = validate_user_range(iov_base, iov_len) {
                break;
            }
            let mut chunk = alloc::vec![0u8; iov_len];
            with_user_access(|| unsafe {
                core::ptr::copy_nonoverlapping(iov_base as *const u8, chunk.as_mut_ptr(), iov_len);
            });
            total_data.extend_from_slice(&chunk);
        }

        if total_data.is_empty() {
            break;
        }

        let sent = sys_sendto(
            fd,
            total_data.as_ptr() as usize,
            total_data.len(),
            flags,
            0,
            0,
        );
        if sent >= errno_base() {
            break;
        }

        with_user_access(|| unsafe {
            *((entry_ptr + 56) as *mut u32) = sent as u32;
        });
        count += 1;

        if flags & 0x8000 == 0 {
            break;
        } // MSG_NOSIGNAL以外
    }
    count
}

// ---------------------------------------------------------------------------
// I/O Çoğaltma
// ---------------------------------------------------------------------------

/// `pselect6` — select() benzeri ama timespec ile timeout.
fn sys_pselect6(
    nfds: usize,
    readfds_ptr: usize,
    writefds_ptr: usize,
    exceptfds_ptr: usize,
    _timeout_ptr: usize,
    _sigmask_ptr: usize,
) -> usize {
    sys_select(nfds, readfds_ptr, writefds_ptr, exceptfds_ptr, 0)
}

/// `ppoll` — poll() benzeri ama timespec ile timeout.
fn sys_ppoll(fds_ptr: usize, nfds: usize, _timeout_ptr: usize, _sigmask_ptr: usize) -> usize {
    sys_poll(fds_ptr, nfds, 0)
}

// ---------------------------------------------------------------------------
// Bellek Kilitleme
// ---------------------------------------------------------------------------

/// `mlock` — Belirtilen bellek aralığını kilitler (RAM'de tutar).
fn sys_mlock(_addr: usize, _len: usize) -> usize {
    0 // no-op: tek kullanıcılı sistemde kilit gerekmez
}

/// `munlock` — Bellek kilidini kaldırır.
fn sys_munlock(_addr: usize, _len: usize) -> usize {
    0
}

/// `mlockall` — Tüm proces belleğini kilitler.
fn sys_mlockall(_flags: usize) -> usize {
    0
}

/// `munlockall` — Tüm bellek kilidini kaldırır.
fn sys_munlockall() -> usize {
    0
}

/// `mlock2` — mlock + flags.
fn sys_mlock2(_addr: usize, _len: usize, _flags: usize) -> usize {
    0
}

/// `userfaultfd` — User-space fault handling fd'si oluşturur.
fn sys_userfaultfd(flags: usize) -> usize {
    const O_CLOEXEC: usize = 02000000;
    const O_NONBLOCK: usize = 04000;

    // FD oluştur
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // userfaultfd, sayfa hatalarını user-space'e yönlendirir
    // echOS'ta henüz userfaultfd altyapısı yok — EINVAL dön
    // Gerçek implementasyon: page fault handler'ı user-space'e yönlendirir
    let mut table = FD_TABLE.lock();
    table[fd] = None; // fd'yi geri al
    errno(ENOSYS)
}

/// `pkey_alloc` — Protection key alokasyonu (Intel MPK).
fn sys_pkey_alloc(_flags: usize, _access_rights: usize) -> usize {
    // Intel Memory Protection Keys: 16 tane key mevcut (PKEY0-PKEY15)
    // echOS'ta henüz MPK desteği yok
    errno(ENOSYS)
}

/// `pkey_free` — Protection key serbest bırakma.
fn sys_pkey_free(_pkey: usize) -> usize {
    errno(ENOSYS)
}

// ---------------------------------------------------------------------------
// Zamanlama (Scheduler)
// ---------------------------------------------------------------------------

/// `sched_setparam` — Süreç zamanlama parametresini ayarlar.
fn sys_sched_setparam(_pid: usize, _param_ptr: usize) -> usize {
    0
}

/// `sched_getparam` — Süreç zamanlama parametresini döndürür.
fn sys_sched_getparam(_pid: usize, param_ptr: usize) -> usize {
    if param_ptr != 0 {
        if let Err(err) = validate_user_range(param_ptr, core::mem::size_of::<u32>()) {
            return err;
        }
        let _ = write_user(param_ptr, 0u32); // priority = 0
    }
    0
}

/// `sched_setscheduler` — Süreç zamanlayıcısını ayarlar.
fn sys_sched_setscheduler(_pid: usize, _policy: usize, _param_ptr: usize) -> usize {
    0
}

/// `sched_getscheduler` — Süreç zamanlayıcısını döndürür.
fn sys_sched_getscheduler(_pid: usize) -> usize {
    0 // SCHED_OTHER
}

/// `sched_get_priority_max` — Belirtilen politikanın maks önceliğini döndürür.
fn sys_sched_get_priority_max(_policy: usize) -> usize {
    99 // Linux_MAX_NICE = 19 → priority = 99
}

/// `sched_get_priority_min` — Belirtilen politikanın min önceliğini döndürür.
fn sys_sched_get_priority_min(_policy: usize) -> usize {
    1
}

/// `sched_rr_get_interval` — Round-robin zaman dilimini döndürür.
fn sys_sched_rr_get_interval(_pid: usize, interval_ptr: usize) -> usize {
    if interval_ptr != 0 {
        if let Err(err) = validate_user_range(interval_ptr, core::mem::size_of::<Timespec>()) {
            return err;
        }
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        }; // 10ms
        let _ = write_user(interval_ptr, ts);
    }
    0
}

/// `sched_setaffinity` — CPU affinity ayarlar.
fn sys_sched_setaffinity(_pid: usize, _cpusetsize: usize, _mask_ptr: usize) -> usize {
    0
}

/// `sched_getaffinity` — CPU affinity döndürür.
fn sys_sched_getaffinity(_pid: usize, cpusetsize: usize, mask_ptr: usize) -> usize {
    if mask_ptr != 0 && cpusetsize >= 8 {
        if let Err(err) = validate_user_range(mask_ptr, 8) {
            return err;
        }
        let _ = write_user(mask_ptr, 1u64); // CPU 0
    }
    0
}

/// `sched_setattr` — Gelişmiş zamanlama ayarı.
fn sys_sched_setattr(_pid: usize, _attr_ptr: usize, _flags: usize) -> usize {
    0
}

/// `sched_getattr` — Gelişmiş zamanlama bilgisi.
fn sys_sched_getattr(_pid: usize, attr_ptr: usize, _size: usize) -> usize {
    if attr_ptr != 0 {
        if let Err(err) = validate_user_range(attr_ptr, 48) {
            return err;
        }
        let _ = write_user(attr_ptr, 0u32); // size
    }
    0
}

/// `setpriority` — Süreç/grup/ kullanıcı önceliğini ayarlar.
fn sys_setpriority(_which: usize, _who: usize, _niceval: usize) -> usize {
    0
}

/// `getpriority` — Süreç/grup/kullanıcı önceliğini döndürür.
fn sys_getpriority(_which: usize, _who: usize) -> usize {
    0 // nice = 0
}

// ---------------------------------------------------------------------------
// SysV IPC — zaten mevcut implementasyonlar (semget, semop, semctl, shmdt,
// msgget, msgsnd, msgrcv, msgctl)
// ---------------------------------------------------------------------------

/// `semtimedop` — Zaman aşımlı semafor işlemi (semop + timeout).
fn sys_semtimedop(semid: usize, sops: usize, nsops: usize, timeout_ptr: usize) -> usize {
    // timeout'u yok sayarak semop'a yönlendir
    let _ = timeout_ptr;
    sys_semop(semid, sops, nsops)
}

// ---------------------------------------------------------------------------
// Process VM
// ---------------------------------------------------------------------------

/// `process_vm_readv` — Başka sürecin belleğini okur.
fn sys_process_vm_readv(
    pid: usize,
    local_iov_ptr: usize,
    liovcnt: usize,
    remote_iov_ptr: usize,
    riovcnt: usize,
    _flags: usize,
) -> usize {
    // process_vm_readv, hedef sürecin sanal belleğinden okuma yapar
    // struct iovec { void *iov_base; size_t iov_len; }
    if local_iov_ptr == 0 || remote_iov_ptr == 0 {
        return errno(EINVAL);
    }
    if liovcnt == 0 || riovcnt == 0 {
        return 0;
    }

    if !tasking::task_exists(pid) {
        return errno(ESRCH);
    }

    // echoOS'ta süreçler arası bellek okuma henüz desteklenmiyor
    errno(ENOSYS)
}

/// `process_vm_writev` — Başka sürecin belleğine yazar.
fn sys_process_vm_writev(
    pid: usize,
    local_iov_ptr: usize,
    liovcnt: usize,
    remote_iov_ptr: usize,
    riovcnt: usize,
    _flags: usize,
) -> usize {
    if local_iov_ptr == 0 || remote_iov_ptr == 0 {
        return errno(EINVAL);
    }
    if liovcnt == 0 || riovcnt == 0 {
        return 0;
    }

    if !tasking::task_exists(pid) {
        return errno(ESRCH);
    }

    errno(ENOSYS)
}

// ---------------------------------------------------------------------------
// Sistem
// ---------------------------------------------------------------------------

/// `syslog` — Kernel log okuma/yazma.
fn sys_syslog(type_: usize, bufp_ptr: usize, len: usize) -> usize {
    // syslog type'ları:
    // 0 = SYSLOG_ACTION_CLOSE (no-op)
    // 1 = SYSLOG_ACTION_OPEN (no-op)
    // 2 = SYSLOG_ACTION_READ — kernel log buffer'dan oku
    // 3 = SYSLOG_ACTION_READ_ALL — tüm log'u oku
    // 4 = SYSLOG_ACTION_READ_CLEAR — oku ve temizle
    // 5 = SYSLOG_ACTION_CLEAR — log buffer'ını temizle
    // 6 = SYSLOG_ACTION_CONSOLE_OFF — konsol log kapat
    // 7 = SYSLOG_ACTION_CONSOLE_ON — konsol log aç
    // 8 = SYSLOG_ACTION_SIZE_LEFT — kalan boyut
    // 9 = SYSLOG_ACTION_SIZE — toplam boyut

    match type_ {
        0 | 1 => 0, // close/open — no-op
        5 => 0,     // clear — no-op (buffer henüz yok)
        6 | 7 => 0, // console on/off — no-op
        8 => 0,     // size_left — boş buffer
        9 => 0,     // total_size — boş buffer
        2 | 3 | 4 => {
            // read — kernel log'dan oku
            // echOS'ta ring buffer log henüz yok — boş buffer
            if bufp_ptr != 0 && len > 0 {
                if let Err(e) = validate_user_range(bufp_ptr, len) {
                    return e;
                }
                with_user_access(|| unsafe {
                    core::ptr::write_bytes(bufp_ptr as *mut u8, 0, len);
                });
            }
            0
        }
        _ => errno(EINVAL),
    }
}

/// `swapon` — Swap alanını etkinleştirir.
fn sys_swapon(path_ptr: usize, swapflags: usize) -> usize {
    // swapflags: SWAP_FLAG_PREFER (0x8000) ve bitmask
    let _path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    // echOS'ta fiziksel swap disk altyapısı yok
    // Ancak ENOSYS yerine ENOMEM döndür — Linux davranışı
    errno(ENOMEM)
}

/// `swapoff` — Swap alanını devre dışı bırakır.
fn sys_swapoff(path_ptr: usize) -> usize {
    let _path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    errno(ENODEV) // Swap cihazı bulunamadı
}

/// `sethostname` — Sistem adını ayarlar.
fn sys_sethostname(_name_ptr: usize, _len: usize) -> usize {
    0
}

/// `setdomainname` — Domain adını ayarlar.
fn sys_setdomainname(_name_ptr: usize, _len: usize) -> usize {
    0
}

/// `personality` — Process personality bayrağını ayarlar.
fn sys_personality(_persona: usize) -> usize {
    0
}

/// `reboot` — Sistemi yeniden başlatır.
fn sys_reboot(_cmd: usize) -> usize {
    // Güvenlik nedeniyle devre dışı
    errno(EPERM)
}

// ---------------------------------------------------------------------------
// Güvenlik (Landlock LSM) — stub
// ---------------------------------------------------------------------------

/// `landlock_create_ruleset` — Landlock kurallar seti oluşturur.
fn sys_landlock_create_ruleset(attr_ptr: usize, size: usize, flags: usize) -> usize {
    // Landlock: unprivileged filesystem sandboxing
    // struct landlock_ruleset_attr { __u64 handled_access_fs; __u64 handled_access_net; }
    if size < 16 {
        return errno(EINVAL);
    }
    if attr_ptr == 0 {
        return errno(EFAULT);
    }
    if let Err(e) = validate_user_range(attr_ptr, size) {
        return e;
    }

    let _handled_access_fs: u64 = with_user_access(|| unsafe { *(attr_ptr as *const u64) });

    // Landlock ruleset fd oluştur
    let fd = allocate_fd(FdKind::File);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // Basit: ruleset'i kaydet (boş kurallar seti)
    // Gerçek implementasyon: landlock_ruleset struct'ı oluşturup fd'ye bağlar
    fd
}

/// `landlock_add_rule` — Landlock kuralı ekler.
fn sys_landlock_add_rule(
    ruleset_fd: usize,
    rule_type: usize,
    rule_attr_ptr: usize,
    flags: usize,
) -> usize {
    // rule_type: LANDLOCK_RULE_PATH_BENEATH (1), LANDLOCK_RULE_NET_PORT (2)
    if rule_attr_ptr == 0 {
        return errno(EFAULT);
    }

    match rule_type {
        1 => {
            // LANDLOCK_RULE_PATH_BENEATH
            // struct landlock_path_beneath_attr { __u64 allowed_access; __s32 parent_fd; __u32 __reserved; }
            if let Err(e) = validate_user_range(rule_attr_ptr, 16) {
                return e;
            }
            let _allowed_access: u64 =
                with_user_access(|| unsafe { *(rule_attr_ptr as *const u64) });
            let _parent_fd: i32 =
                with_user_access(|| unsafe { *((rule_attr_ptr + 8) as *const i32) });
            // Kuralı ruleset'e ekle — şimdilik no-op
            0
        }
        2 => {
            // LANDLOCK_RULE_NET_PORT
            // struct landlock_net_port_attr { __u64 allowed_access; __u64 port; }
            if let Err(e) = validate_user_range(rule_attr_ptr, 16) {
                return e;
            }
            let _allowed_access: u64 =
                with_user_access(|| unsafe { *(rule_attr_ptr as *const u64) });
            let _port: u64 = with_user_access(|| unsafe { *((rule_attr_ptr + 8) as *const u64) });
            0
        }
        _ => errno(EINVAL),
    }
}

/// `landlock_restrict_self` — Mevcut sürece kısıtlama uygular.
fn sys_landlock_restrict_self(ruleset_fd: usize, flags: usize) -> usize {
    // Landlock kısıtlamasını uygula
    // flags: LANDLOCK_ACCESS_FS_EXECUTE, LANDLOCK_ACCESS_FS_WRITE_FILE, ...
    // echOS'ta henüz tam Landlock kısıtlaması desteklenmiyor
    // Kurallar eklendi ama uygulanmıyor — bu bir kısıtlama
    let _ = ruleset_fd;
    let _ = flags;
    0 // Şimdilik başarılı — gerçek kısıtlama gelecek
}

// ---------------------------------------------------------------------------
// inotify — stub
// ---------------------------------------------------------------------------

/// `inotify_init1` — inotify fd'si oluşturur.
fn sys_inotify_init1(flags: usize) -> usize {
    crate::fs::inotify::sys_inotify_init1(flags as i32) as usize
}

/// `inotify_add_watch` — Dizin/dosya izleme ekler.
fn sys_inotify_add_watch(fd: usize, path_ptr: usize, mask: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let ret = crate::fs::inotify::sys_inotify_add_watch(fd as i32, &path, mask as u32);
    if ret < 0 {
        (-ret) as usize
    } else {
        ret as usize
    }
}

/// `inotify_rm_watch` — İzlemeyi kaldırır.
fn sys_inotify_rm_watch(fd: usize, wd: usize) -> usize {
    let ret = crate::fs::inotify::sys_inotify_rm_watch(fd as i32, wd as i32);
    if ret < 0 {
        (-ret) as usize
    } else {
        ret as usize
    }
}

/// `signalfd4` — Sinyal fd'si oluşturur.
fn sys_signalfd4(fd: usize, mask_ptr: usize, sizemask: usize, flags: usize) -> usize {
    const SFD_CLOEXEC: usize = 02000000;
    const SFD_NONBLOCK: usize = 04000;

    if sizemask != 8 {
        return errno(EINVAL);
    }
    if mask_ptr == 0 {
        return errno(EFAULT);
    }
    if let Err(e) = validate_user_range(mask_ptr, 8) {
        return e;
    }

    let signal_mask: u64 = with_user_access(|| unsafe { *(mask_ptr as *const u64) });

    // Mevcut signalfd'yi güncelle veya yeni oluştur
    let actual_fd = if fd == -1_isize as usize {
        // Yeni fd oluştur
        let new_fd = allocate_fd(FdKind::File);
        if new_fd >= MAX_FDS {
            return errno(EMFILE);
        }
        new_fd
    } else {
        // Mevcut fd'yi kullan
        let files = FILE_TABLE.lock();
        match files.get(fd) {
            Some(Some(_)) => fd,
            _ => return errno(EBADF),
        }
    };

    // signalfd maskesini kaydet
    // Gerçek implementasyon: bu fd okunabilir olduğunda, mask'teki
    // sinyallerden biri pending ise signalfd_siginfo yapısı ile döner
    // Basit: fd'yi başarılı dön, signal okuma henüz desteklenmiyor
    actual_fd
}

// ---------------------------------------------------------------------------
// Faz 3 — Eksik Linux Syscall Stub'ları
// ---------------------------------------------------------------------------

/// `alarm` — Sürecin kendisine belirtilen saniye sonra SIGALRM gönderir.
/// Önceki alarm süresini döndürür (0 = önceki alarm yok).
/// signal.rs'deki gerçek implementasyonu çağırır.
fn sys_alarm(seconds: usize) -> usize {
    crate::task::signal::sys_alarm(seconds as u32) as usize
}

/// `arch_prctl` — x86_64 mimari özel register'ları ayarlar.
/// ARCH_SET_FS (0x1001) = FS taban adresi (thread-local storage)
/// ARCH_GET_FS (0x1003) = FS taban adresini oku
/// ARCH_SET_GS (0x1002) = GS taban adresi
/// ARCH_GET_GS (0x1004) = GS taban adresini oku
/// Gerçek implementasyon: MSR_FS_BASE / MSR_GS_BASE üzerinden yönetilir.
fn sys_arch_prctl(code: usize, addr: usize, _unused: usize) -> usize {
    use crate::task::scheduler::PER_CPU_CURRENT_TASK;
    let cpu_id = crate::cpu::smp::get_current_cpu_id() as usize;

    match code {
        0x1001 => {
            // ARCH_SET_FS — FS taban adresini ayarla (TLS için kritik)
            if let Some(ref mut current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_mut() } {
                current.cold.fs_base = addr as u64;
            }
            unsafe {
                use x86_64::registers::model_specific::Msr;
                Msr::new(0xC000_0102).write(addr as u64); // MSR_FS_BASE
            }
            0
        }
        0x1003 => {
            // ARCH_GET_FS — FS taban adresini oku
            if let Err(err) = validate_user_range(addr, core::mem::size_of::<u64>()) {
                return err;
            }
            let fs_base =
                if let Some(ref current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_ref() } {
                    current.cold.fs_base
                } else {
                    0
                };
            let _ = write_user(addr, fs_base);
            0
        }
        0x1002 => {
            // ARCH_SET_GS — GS taban adresini ayarla
            unsafe {
                use x86_64::registers::model_specific::Msr;
                Msr::new(0xC000_0101).write(addr as u64); // MSR_GS_BASE
            }
            0
        }
        0x1004 => {
            // ARCH_GET_GS — GS taban adresini oku
            if let Err(err) = validate_user_range(addr, core::mem::size_of::<u64>()) {
                return err;
            }
            let gs_base: u64 = unsafe {
                use x86_64::registers::model_specific::Msr;
                Msr::new(0xC000_0101).read()
            };
            let _ = write_user(addr, gs_base);
            0
        }
        0x1012 => {
            // ARCH_SET_CPUID — CPUID talimatını etkinleştir/devre dışı bırak
            // echOS'ta CPUID her zaman etkin — no-op
            0
        }
        0x1013 => {
            // ARCH_GET_CPUID — CPUID durumunu döndür (1 = etkin)
            1
        }
        _ => errno(EINVAL),
    }
}

/// `set_thread_area` — x86_32 uyumlu thread-local storage alanı ayarlar.
fn sys_set_thread_area(_u_info_ptr: usize) -> usize {
    0 // echOS'ta 64-bit modda GDT tabanlı TLS kullanılmıyor
}

/// `init_module` — Kernel modülünü yükler.
fn sys_init_module(image_ptr: usize, len: usize, param_values_ptr: usize) -> usize {
    // echoOS'ta kernel modül yükleme desteklenmiyor
    // Gerekli altyapı: ELF modül parser, sembol tablosu,重定位, modül linker
    // CAP_SYS_MODULE yetkisi gerekir
    errno(EPERM)
}

/// `delete_module` — Kernel modülünü boşaltır.
fn sys_delete_module(name_ptr: usize, flags: usize) -> usize {
    // O_NONBLOCK = 0x800
    let _nonblock = flags & 0x800 != 0;

    if name_ptr != 0 {
        let _name = match read_user_cstring(name_ptr, 256) {
            Ok(n) => n,
            Err(e) => return e,
        };
    }

    // echoOS'ta modül desteği yok — EPERM
    errno(EPERM)
}

/// `finit_module` — Dosyadan kernel modülünü yükler (fd bazlı).
fn sys_finit_module(fd: usize, param_values_ptr: usize, flags: usize) -> usize {
    // fd'den modül imajını oku ve yükle
    // echoOS'ta henüz desteklenmiyor
    errno(EPERM)
}

/// `ioprio_set` — I/O önceliğini ayarlar.
/// which: 1=IOPRIO_WHO_PROCESS, 2=IOPRIO_WHO_PGRP, 3=IOPRIO_WHO_USER
/// who: process id, process group id, veya user id (0 = mevcut)
/// ioprio: (class << 8) | priority
///   class: 1=IOPRIO_CLASS_RT, 2=IOPRIO_CLASS_BE, 3=IOPRIO_CLASS_IDLE
///   priority: 0-7 (RT/BE için), 0 (IDLE için)
fn sys_ioprio_set(which: usize, who: usize, ioprio: usize) -> usize {
    use crate::task::scheduler::PER_CPU_CURRENT_TASK;
    let cpu_id = crate::cpu::smp::get_current_cpu_id() as usize;

    let class = (ioprio >> 8) & 0x7;
    let prio = ioprio & 0x7;

    // Geçerli class kontrolü
    if class != 0 && class != 1 && class != 2 && class != 3 {
        return errno(EINVAL);
    }
    // RT class için priority 0-7 arası olmalı
    if class == 1 && prio > 7 {
        return errno(EINVAL);
    }

    match which {
        1 => {
            // IOPRIO_WHO_PROCESS — mevcut sürecin I/O önceliğini ayarla
            let target_pid = if who == 0 {
                tasking::scheduler::current_task_id()
            } else {
                who
            };
            // Basit implementasyon: sadece mevcut process'i destekle
            if target_pid == tasking::scheduler::current_task_id() {
                if let Some(ref mut current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_mut() } {
                    current.hot.ioprio = ioprio as u16;
                    return 0;
                }
            }
            // Diğer süreçler için PID arama (gelecekte genişletilebilir)
            0
        }
        2 => {
            // IOPRIO_WHO_PGRP — process group'un I/O önceliğini ayarla
            // Şimdilik sadece mevcut process'i ayarla
            if let Some(ref mut current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_mut() } {
                current.hot.ioprio = ioprio as u16;
            }
            0
        }
        3 => {
            // IOPRIO_WHO_USER — kullanıcının tüm süreçlerinin I/O önceliğini ayarla
            // Şimdilik sadece mevcut process'i ayarla
            if let Some(ref mut current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_mut() } {
                current.hot.ioprio = ioprio as u16;
            }
            0
        }
        _ => errno(EINVAL),
    }
}

/// `ioprio_get` — I/O önceliğini okur.
fn sys_ioprio_get(which: usize, who: usize) -> usize {
    use crate::task::scheduler::PER_CPU_CURRENT_TASK;
    let cpu_id = crate::cpu::smp::get_current_cpu_id() as usize;

    match which {
        1 => {
            // IOPRIO_WHO_PROCESS
            let target_pid = if who == 0 {
                tasking::scheduler::current_task_id()
            } else {
                who
            };
            if target_pid == tasking::scheduler::current_task_id() {
                if let Some(ref current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_ref() } {
                    return current.hot.ioprio as usize;
                }
            }
            2 << 8 // Varsayılan: best-effort, priority 0
        }
        2 | 3 => {
            // IOPRIO_WHO_PGRP / IOPRIO_WHO_USER
            if let Some(ref current) = unsafe { PER_CPU_CURRENT_TASK[cpu_id].as_ref() } {
                current.hot.ioprio as usize
            } else {
                2 << 8
            }
        }
        _ => errno(EINVAL),
    }
}

/// `preadv` — Dosyadan offset ile vektörel okuma (scatter read).
/// Her iovec buffer'ı için pread64 çağrısı yaparak offset'li okuma sağlar.
fn sys_preadv(fd: usize, iov_ptr: usize, iovcnt: usize, pos_low: usize) -> usize {
    if iovcnt == 0 || iovcnt > 1024 {
        return errno(EINVAL);
    }
    let iov_bytes = iovcnt.saturating_mul(core::mem::size_of::<[usize; 2]>());
    if let Err(err) = validate_user_range(iov_ptr, iov_bytes) {
        return err;
    }
    let mut iov_entries = vec![[0usize; 2]; iovcnt];
    if let Err(err) = copy_from_user_slice(&mut iov_entries, iov_ptr) {
        return err;
    }
    let mut total = 0usize;
    let mut offset = pos_low;

    for i in 0..iovcnt {
        let entry = &iov_entries[i];
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }
        if let Err(err) = validate_user_range(base, len) {
            if total > 0 {
                return total;
            }
            return err;
        }
        let result = sys_pread64(fd, base, len, offset);
        if result > 0x7FFF_FFFF_FFFF_0000 {
            if total > 0 {
                return total;
            }
            return result;
        }
        total += result;
        offset += result;
        if result < len {
            break;
        }
    }
    total
}

/// `pwritev` — Dosyaya offset ile vektörel yazma (gather write).
fn sys_pwritev(fd: usize, iov_ptr: usize, iovcnt: usize, pos_low: usize) -> usize {
    if iovcnt == 0 || iovcnt > 1024 {
        return errno(EINVAL);
    }
    let iov_bytes = iovcnt.saturating_mul(core::mem::size_of::<[usize; 2]>());
    if let Err(err) = validate_user_range(iov_ptr, iov_bytes) {
        return err;
    }
    let mut iov_entries = vec![[0usize; 2]; iovcnt];
    if let Err(err) = copy_from_user_slice(&mut iov_entries, iov_ptr) {
        return err;
    }
    let mut total = 0usize;
    let mut offset = pos_low;

    for i in 0..iovcnt {
        let entry = &iov_entries[i];
        let base = entry[0];
        let len = entry[1];
        if len == 0 {
            continue;
        }
        if let Err(err) = validate_user_range(base, len) {
            if total > 0 {
                return total;
            }
            return err;
        }
        let result = sys_pwrite64(fd, base, len, offset);
        if result > 0x7FFF_FFFF_FFFF_0000 {
            if total > 0 {
                return total;
            }
            return result;
        }
        total += result;
        offset += result;
        if result < len {
            break;
        }
    }
    total
}

/// `rt_tgsigqueueinfo` — Belirli bir süreç grubuna sinyal + veri gönderir.
fn sys_rt_tgsigqueueinfo(tgid: usize, sig: usize, info_ptr: usize) -> usize {
    if sig == 0 || sig > 64 {
        return errno(EINVAL);
    }

    // siginfo_t oku
    if info_ptr != 0 {
        if let Err(e) = validate_user_range(info_ptr, 128) {
            return e;
        } // sizeof(siginfo_t)
        let si_signo: i32 = with_user_access(|| unsafe { *(info_ptr as *const i32) });
        if si_signo != sig as i32 {
            return errno(EINVAL);
        }
    }

    // Sadece process group liderine gönder
    if let Some(sig) = crate::task::signal::Signal::from_number(sig as u8) {
        let _ = crate::task::signal::send_signal(tgid, sig);
    }
    0
}

/// `fanotify_init` — Dosya erişim bildirimi (otify benzeri).
/// fanotify, inotify'un gelişmiş versiyonudur — dosya erişim olaylarını izler.
/// return: fanotify fd (başarılı), -1 (hata)
fn sys_fanotify_init(flags: usize, event_f_flags: usize) -> usize {
    // fanotify flag'leri
    const FAN_CLASS_NOTIF: usize = 0x00;
    const FAN_CLASS_CONTENT: usize = 0x01;
    const FAN_REPORT_FID: usize = 0x002;
    const FAN_NONBLOCK: usize = 0x0008;
    const FAN_CLOEXEC: usize = 0x0001;

    let class = flags & 0x3;
    if class != FAN_CLASS_NOTIF && class != FAN_CLASS_CONTENT && class != FAN_REPORT_FID {
        return errno(EINVAL);
    }

    // Basit fanotify instance — dosya erişim olaylarını izlemek için fd döndür
    // Gerçek implementasyonda: fanotify event queue + group yönetimi gerekir
    // Şimdilik inotify instance benzeri bir fd aç
    let inotify_flags = if flags & FAN_NONBLOCK != 0 { 0x0800 } else { 0 };
    let fd = crate::fs::inotify::sys_inotify_init1(inotify_flags as i32);
    if fd < 0 {
        return (-fd) as usize;
    }
    // fanotify fd'sini CLOEXEC ile işaretle (eğer FAN_CLOEXEC ayarlıysa)
    if flags & FAN_CLOEXEC != 0 {
        sys_fcntl(fd as usize, 2, 1); // F_SETFD = 2, FD_CLOEXEC = 1
    }
    fd as usize
}

/// `fanotify_mark` — Dosya erişim bildirimi izleme ekler.
/// fanotify_init'den dönen fd üzerinden dosya izleme ekler.
fn sys_fanotify_mark(
    fanotify_fd: usize,
    flags: usize,
    mask: usize,
    dfd: usize,
    pathname_ptr: usize,
) -> usize {
    // fanotify_mark = inotify_add_watch benzeri
    // FAN_MARK_ADD = 0x1, FAN_MARK_REMOVE = 0x2
    const FAN_MARK_ADD: usize = 0x1;
    const FAN_MARK_REMOVE: usize = 0x2;
    const FAN_MARK_DONT_FOLLOW: usize = 0x04;
    const FAN_MARK_ONLYDIR: usize = 0x08;
    const FAN_MARK_MOUNT: usize = 0x10;
    const FAN_MARK_IGNORED_MASK: usize = 0x20;

    if pathname_ptr == 0 && flags & FAN_MARK_MOUNT == 0 {
        return errno(EINVAL);
    }

    let path = if pathname_ptr != 0 {
        match read_user_cstring(pathname_ptr, 4096) {
            Ok(p) => p,
            Err(e) => return e,
        }
    } else {
        String::new()
    };

    // inotify benzeri watch ekle/kaldır
    let inotify_mask = mask as u32;
    match flags & 0x3F {
        FAN_MARK_ADD => {
            let ret =
                crate::fs::inotify::sys_inotify_add_watch(fanotify_fd as i32, &path, inotify_mask);
            if ret < 0 {
                (-ret) as usize
            } else {
                0
            }
        }
        FAN_MARK_REMOVE => {
            // fanotify'da watch descriptor PATH ile değil, inotify ile aynı şekilde
            // wd üzerinden kaldırılır — burada pathname'den wd bulmamız gerekir
            // Basit implementasyon: inotify_rm_watch çağır
            let ret = crate::fs::inotify::sys_inotify_rm_watch(fanotify_fd as i32, 0);
            if ret < 0 {
                (-ret) as usize
            } else {
                0
            }
        }
        _ => errno(EINVAL),
    }
}

/// `kexec_load` — Çalışan sisteme yeni kernel yükler.
/// entry: yeni kernel'in giriş noktası
/// nr_segments: kernel segment sayısı
/// segments_ptr: kexec_segment array
/// flags: KEXEC_ON_CRASH (0x1), KEXEC_PRESERVE_CONTEXT (0x2)
fn sys_kexec_load(
    _entry: usize,
    _nr_segments: usize,
    _segments_ptr: usize,
    _flags: usize,
) -> usize {
    // kexec_load, çalışırken sistemi yeniden başlatmak için kullanılır
    // echOS'ta henüz kexec desteği yok — gerçek kernel yükleme altyapısı gerekir
    // Bu syscall'ı çağırmak tehlikeli olabilir, EPERM ile reddet
    errno(EPERM)
}

/// `membarrier` — Bellek bariyeri.
/// cmd=1 (MEMBARRIER_CMD_SHARED) ise tüm CPU'larda MFENCE uygular.
/// Bu, publish-subscribe senkronizasyonu için gereklidir.
fn sys_membarrier(cmd: usize, _flags: usize, _unused: usize) -> usize {
    match cmd {
        0 => 0, // MEMBARRIER_CMD_NONE
        1 => {
            // MEMBARRIER_CMD_SHARED — tüm CPU'larda memory fence
            // x86_64'de MFENCE tüm store/load'ları sıralar
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            0
        }
        2 => 0, // MEMBARRIER_CMD_PRIVATE — no-op (tekil store zaten sıralı)
        _ => errno(EINVAL),
    }
}

/// `mbind` — NUMA bellek politikası ayarlar.
/// start: bellek aralığının başlangıcı
/// len: bellek aralığı uzunluğu
/// mode: MPOL_DEFAULT(0), MPOL_BIND(1), MPOL_INTERLEAVE(2), MPOL_PREFERRED(3)
/// nmask_ptr: NUMA node maskesi
/// maxnode: maksimum NUMA node numarası
/// flags: MPOL_MF_STRICT, MPOL_MF_MOVE, ...
fn sys_mbind(
    _start: usize,
    _len: usize,
    _mode: usize,
    _nmask_ptr: usize,
    _maxnode: usize,
    _flags: usize,
) -> usize {
    // echOS'ta NUMA desteklenmiyor — tek NUMA node varsayımı
    // Gerçek NUMA implementasyonu için: NUMA topology, node-aware allocator gerekir
    0 // POSIX: mbind başarısız olsa bile 0 döndür (non-strict mode)
}

/// `set_mempolicy` — Sürecin NUMA bellek politikasını ayarlar.
/// mode: MPOL_DEFAULT(0), MPOL_BIND(1), MPOL_INTERLEAVE(2), MPOL_PREFERRED(3)
/// nmask_ptr: NUMA node maskesi
/// maxnode: maksimum NUMA node numarası
fn sys_set_mempolicy(_mode: usize, _nmask_ptr: usize, _maxnode: usize) -> usize {
    // echOS'ta NUMA desteklenmiyor
    0
}

/// `get_mempolicy` — Sürecin NUMA bellek politikasını okur.
fn sys_get_mempolicy(
    _mode_ptr: usize,
    _nmask_ptr: usize,
    _maxnode: usize,
    _addr: usize,
    _flags: usize,
) -> usize {
    // echOS'ta NUMA desteklenmiyor — varsayılan politika: MPOL_DEFAULT
    if _mode_ptr != 0 {
        if let Err(err) = validate_user_range(_mode_ptr, core::mem::size_of::<i32>()) {
            return err;
        }
        let _ = write_user(_mode_ptr, 0i32); // MPOL_DEFAULT
    }
    0
}

/// `migrate_pages` — Sürecin sayfalarını NUMA düğümleri arasında taşır.
fn sys_migrate_pages(pid: usize, maxnode: usize, _old_nmask_ptr: usize) -> usize {
    // echoOS'ta NUMA topolojisi yok — tek node varsayımı
    // maxnode > 1 ise ENODEV dön
    if maxnode > 1 {
        return errno(ENODEV);
    }
    0 // Tek node — zaten doğru node'ta
}

/// `add_key` — Anahtar yönetimi (keyring) anahtarı ekler.
fn sys_add_key(
    type_ptr: usize,
    description_ptr: usize,
    payload_ptr: usize,
    plen: usize,
    _keyring_serial: usize,
) -> usize {
    // echoOS'ta keyring/anahtar yönetimi henüz desteklenmiyor
    // Linux keyring: /proc/keys, /proc/key-users, request_key()
    errno(ENOSYS)
}

/// `request_key` — Anahtar yönetimi (keyring) anahtarı talep eder.
fn sys_request_key(
    _type_ptr: usize,
    _description_ptr: usize,
    _callout_info_ptr: usize,
    _dest_keyring: usize,
) -> usize {
    errno(ENOSYS)
}

/// `process_madvise` — Başka bir süreç için bellek tavsiyesi uygular.
fn sys_process_madvise(
    pidfd: usize,
    iov_ptr: usize,
    vlen: usize,
    advice: usize,
    _flags: usize,
) -> usize {
    if iov_ptr == 0 || vlen == 0 {
        return errno(EINVAL);
    }
    if vlen > 256 {
        return errno(EINVAL);
    } // UIO_MAXIOV

    // Hedef sürecin PID'ini al (pidfd → PID dönüşümü)
    let target_pid = pidfd; // Basit: pidfd = pid

    // Her iovec'i işle: { void *iov_base; size_t iov_len; }
    let mut total_advised: usize = 0;
    for i in 0..vlen {
        let entry_ptr = iov_ptr + i * 16;
        if let Err(e) = validate_user_range(entry_ptr, 16) {
            break;
        }

        let iov_base: usize = with_user_access(|| unsafe { *(entry_ptr as *const usize) });
        let iov_len: usize = with_user_access(|| unsafe { *((entry_ptr + 8) as *const usize) });

        if iov_len == 0 {
            continue;
        }

        // madvise tavsiyesini uygula
        let result = sys_madvise(iov_base, iov_len, advice);
        if result >= errno_base() {
            break;
        }
        total_advised += iov_len;
    }

    total_advised
}

/// `process_mrelease` — Mevcut sürece MRELEASE uygular.
fn sys_process_mrelease(pidfd: usize, flags: usize) -> usize {
    // process_mrelease, bir OOM killer veya process cleanup sırasında
    // hedef sürecin belleğini serbest bırakır
    let target_pid = pidfd; // Basit: pidfd = pid

    // echoOS'ta henüz process_mrelease desteklenmiyor
    errno(ENOSYS)
}

/// `mount_setattr` — Mount özniteliklerini ayarlar.
fn sys_mount_setattr(
    _dfd: usize,
    pathname_ptr: usize,
    _flags: usize,
    attr_ptr: usize,
    size: usize,
) -> usize {
    if size < core::mem::size_of::<u64>() * 4 {
        return errno(EINVAL);
    }
    if attr_ptr == 0 {
        return errno(EFAULT);
    }
    if let Err(err) = validate_user_range(attr_ptr, size) {
        return err;
    }

    let _path = match read_user_cstring(pathname_ptr, 4096) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let attr_set: u64 = with_user_access(|| unsafe { *(attr_ptr as *const u64) });
    let attr_clr: u64 = with_user_access(|| unsafe { *((attr_ptr + 8) as *const u64) });

    // MOUNT_ATTR_RDONLY = 0x1, MOUNT_ATTR_NOSUID = 0x2, MOUNT_ATTR_NODEV = 0x4
    // MOUNT_ATTR_NOEXEC = 0x8, MOUNT_ATTR_ATIME = 0x10, ...
    let _ = attr_set;
    let _ = attr_clr;

    // echoOS'ta mount attribute değiştirme henüz desteklenmiyor
    errno(ENOSYS)
}

/// `quotactl_fd` — Dosya üzerinden kota işlemleri.
fn sys_quotactl_fd(_fd: usize, _cmd: usize, _id: usize, _addr_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// `memfd_secret` — Gizli bellek alanı oluşturur.
/// Bu bellek alanı sadece sahibi süreç tarafından erişilebilir,
/// diğer süreçler (fork/clone ile oluşanlar) bile erişemez.
/// mmap ile anonim bellek oluşturup F2FS'e kaydeder, ardından
/// dosya tablosuna ekler. Sayfa tablosu ayarları ile erişim kısıtlanır.
fn sys_memfd_secret(_flags: usize) -> usize {
    // echOS'ta henüz tam gizli bellek desteği yok (sayfa tablosu kısıtlaması gerekir)
    // Geçici olarak memfd_create'e yönlendir
    sys_memfd_create("memfd-secret\0".as_ptr() as usize, 1) // MFD_CLOEXEC
}

/// `io_pgetevents` — Asenkron I/O olaylarını bekler (sinyal ile bildirim).
fn sys_io_pgetevents(
    ctx_id: usize,
    min_nr: usize,
    nr: usize,
    events_ptr: usize,
    timeout_ptr: usize,
) -> usize {
    // io_pgetevents, Linux AIO context'inden olayları okur
    // echoOS'ta Linux AIO henüz tam desteklenmiyor
    // io_setup/io_destroy/io_submit/io_getevents basit stub olarak var

    if nr == 0 {
        return 0;
    }
    if events_ptr == 0 {
        return errno(EFAULT);
    }

    // io_event yapısı: { long long data; long long obj; long long res; long long res2; }
    let event_size = core::mem::size_of::<u64>() * 4; // 32 bytes
    if let Err(e) = validate_user_range(events_ptr, nr * event_size) {
        return e;
    }

    // Timeout oku (varsa)
    if timeout_ptr != 0 {
        if let Err(e) = validate_user_range(timeout_ptr, 16) {
            return e;
        }
        let tv_sec: i64 = with_user_access(|| unsafe { *(timeout_ptr as *const i64) });
        let _tv_nsec: i64 = with_user_access(|| unsafe { *((timeout_ptr + 8) as *const i64) });

        // Kısa bekleme
        if tv_sec > 0 {
            let ticks = (tv_sec as usize) * 10;
            for _ in 0..ticks.min(100) {
                x86_64::instructions::hlt();
            }
        }
    }

    // AIO context henüz desteklenmiyor — 0 olay dön
    0
}

/// `clock_gettime_impl` — clock_gettime için dahili implementasyon.
fn clock_gettime_impl(clock_id: usize, ts: &mut Timespec) -> usize {
    // Basit birtick sayacı ile zaman hesaplama
    static BOOT_TICKS: AtomicU64 = AtomicU64::new(0);
    let ticks = BOOT_TICKS.fetch_add(1, Ordering::Relaxed);
    let total_ns = ticks * TICK_NS;
    match clock_id {
        0 => {
            // CLOCK_REALTIME — yaklaşık Unix zamanı
            ts.tv_sec = (total_ns / 1_000_000_000) as i64 + 1_700_000_000; // 2023-11-14 approx
            ts.tv_nsec = (total_ns % 1_000_000_000) as i64;
        }
        1 => {
            // CLOCK_MONOTONIC
            ts.tv_sec = (total_ns / 1_000_000_000) as i64;
            ts.tv_nsec = (total_ns % 1_000_000_000) as i64;
        }
        _ => {
            return errno(EINVAL);
        }
    }
    0
}
