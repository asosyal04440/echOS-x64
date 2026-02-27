/// POSIX syscall çağrısı taşıyıcı yapısı.
#[derive(Clone, Copy)]
pub struct PosixCall {
    pub number: usize,
    pub args: [usize; 6],
}

use crate::fs::f2fs::F2fsEntry;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::_rdtsc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use lazy_static::lazy_static;
use rcore_fs::vfs::{FsError, INode};
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::{BigUint, RsaPublicKey};
use sha2::{Digest, Sha256};
use signature::Verifier;
use spin::Mutex;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::{VariableAttributes, VariableVendor};
#[cfg(target_os = "uefi")]
use uefi::CStr16;
use x86_64::structures::paging::PageTableFlags;
#[cfg(not(target_os = "uefi"))]
enum VariableVendor {
    GLOBAL_VARIABLE,
    IMAGE_SECURITY_DATABASE,
}

// Standart errno kodları
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

// Syscall numaraları (x86_64 Linux uyumlu alt küme)
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

// FileSystem syscalls
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
const SYS_NEWFSTATAT: usize = 262;
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
const SYS_GETRANDOM: usize = 318;
const SYS_IO_URING_SETUP: usize = 425;
const SYS_IO_URING_ENTER: usize = 426;

// echOS — Grafik / Pencere Sunucusu (Faz 5)
const SYS_WIN_CREATE:     usize = 451;
const SYS_WIN_DESTROY:    usize = 452;
const SYS_WIN_GET_BUFFER: usize = 453;
const SYS_WIN_FLUSH:      usize = 454;
const SYS_EVENT_POLL:     usize = 455;

// Process/Thread management
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

// Signal syscalls (additional)
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

// clock_gettime saat tipleri
const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;

// Scheduler tick süresi (ns)
const TICK_NS: u64 = 10_000_000;

// Maksimum açık dosya sayısı
const MAX_FDS: usize = 64;
const STAT_BLKSIZE: i64 = 512;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFCHR: u32 = 0o020000;
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
    static ref FILE_TABLE: Mutex<Vec<Option<FileState>>> = Mutex::new(vec![None; MAX_FDS]);
    static ref RING_TABLE: Mutex<alloc::collections::BTreeMap<usize, IoUringInstance>> = Mutex::new(alloc::collections::BTreeMap::new());
}
static SIGNAL_MASK: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct FileState {
    inode: Arc<dyn INode>,
    offset: usize,
    size: usize,
    is_hello: bool,
}

#[derive(Clone)]
struct DrmResource {
    handle: u32,
    size: usize,
    map_ptr: usize,
}

static DRM_RESOURCES: Mutex<Vec<DrmResource>> = Mutex::new(Vec::new());

/// POSIX timespec yapısı
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

/// Hata kodunu Linux errno dönüşüne çevirir.
fn errno(code: usize) -> usize {
    (!0usize).wrapping_sub(code - 1)
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

/// Syscall dispatcher
pub fn dispatch(number: usize, args: [usize; 6]) -> usize {
    ensure_fd_table();

    // =====================================
    // PTRACE SYSCALL HOOK (ENTRY)
    // =====================================
    let is_traced = (crate::task::scheduler::get_current_ptrace_flags() & 1) != 0;

    if is_traced {
        crate::serial_println!("[PTRACE Hook] SYSCALL Entry: #{}", number);
    }

    // =====================================
    // SECCOMP (STRICT MODE) DENETİMİ
    // =====================================
    let seccomp_mode = crate::task::scheduler::get_current_seccomp_mode();
    if seccomp_mode == 1 {
        // Strict mod sadece 4 temel çağrıya (read, write, exit, rt_sigreturn) izin verir
        if number != SYS_READ && number != SYS_WRITE && number != SYS_EXIT && number != SYS_RT_SIGRETURN {
            crate::serial_println!("[SECCOMP] Strict mode violation! Syscall {} blocked. Process killed.", number);
            return sys_exit(!0); // SIGKILL benzeri task sonlandır (exit code -1)
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
        SYS_SELECT => sys_select(args[0], args[1], args[2], args[3], args[4]),
        SYS_SCHED_YIELD => sys_sched_yield(),
        SYS_MREMAP => sys_mremap(args[0], args[1], args[2], args[3], args[4]),
        SYS_MSYNC => sys_msync(args[0], args[1], args[2]),
        SYS_MINCORE => sys_mincore(args[0], args[1], args[2]),
        SYS_MADVISE => sys_madvise(args[0], args[1], args[2]),
        SYS_FUTEX => sys_futex(args[0], args[1], args[2], args[3], args[4], args[5]),
        
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
        SYS_GETPID => sys_getpid(),
        SYS_EXECVE => sys_execve(args[0], args[1], args[2]),
        SYS_FORK => sys_fork(),
        SYS_WAIT4 => sys_wait4(args[0], args[1], args[2], args[3]),
        SYS_UNAME => sys_uname(args[0]),
        SYS_GETCWD => sys_getcwd(args[0], args[1]),
        SYS_GETRUSAGE => 0, // unimplemented
        SYS_SYSINFO => 0,   // unimplemented
        SYS_TIMES => 0,     // unimplemented
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
        SYS_GETTID => sys_gettid(),
        SYS_EXIT => sys_exit(args[0]),
        SYS_CLONE => sys_clone(args[0], args[1], args[2], args[3], args[4]),
        SYS_SET_TID_ADDRESS => sys_set_tid_address(args[0]),
        SYS_TGKILL => sys_tgkill(args[0], args[1], args[2]),
        SYS_TKILL => sys_tkill(args[0], args[1]),
        SYS_SETUID => sys_setuid(args[0]),
        SYS_SETGID => sys_setgid(args[0]),
        SYS_SETSID => sys_setsid(),
        SYS_SETPGID => sys_setpgid(args[0], args[1]),
        SYS_GETPGID => sys_getpgid(args[0]),
        SYS_GETSID => sys_getsid(args[0]),
        SYS_CLOCK_GETTIME => sys_clock_gettime(args[0], args[1]),
        SYS_EXIT_GROUP => sys_exit(args[0]),
        SYS_GETRANDOM => sys_getrandom(args[0], args[1], args[2]),
        SYS_IO_URING_SETUP => sys_io_uring_setup(args[0], args[1]),
        SYS_IO_URING_ENTER => sys_io_uring_enter(args[0], args[1], args[2], args[3], args[4], args[5]),
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

        // --- WINDOW SERVER (echOS Faz 5) ---
        SYS_WIN_CREATE     => crate::gui::win_server::sys_win_create(
                                  args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_WIN_DESTROY    => crate::gui::win_server::sys_win_destroy(
                                  args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_WIN_GET_BUFFER => crate::gui::win_server::sys_win_get_buffer(
                                  args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_WIN_FLUSH      => crate::gui::win_server::sys_win_flush(
                                  args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_EVENT_POLL     => crate::gui::win_server::sys_event_poll(
                                  args[0], args[1], args[2], args[3], args[4], args[5]),

        _ => errno(ENOSYS),
    };

    if is_traced {
        crate::serial_println!("[PTRACE Hook] SYSCALL Exit: #{} -> ret: {}", number, ret_val);
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

/// write syscall (stdout/stderr + /dev/null + /dev/zero)
fn sys_write(fd: usize, buf: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let bytes =
        with_user_access(|| unsafe { core::slice::from_raw_parts(buf as *const u8, count) });
    match get_fd(fd) {
        Some(FdKind::Stdout) | Some(FdKind::Stderr) => {
            crate::serial_println!("SYSCALL WRITE: fd={} len={}", fd, count);
            for &b in bytes {
                crate::serial_print!("{}", b as char);
            }
            count
        }
        Some(FdKind::Null) => count,
        Some(FdKind::Zero) => count,
        Some(FdKind::File) => errno(EBADF),
        _ => errno(EBADF),
    }
}

/// read syscall (stdin + /dev/zero)
fn sys_read(fd: usize, buf: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    match get_fd(fd) {
        Some(FdKind::Stdin) => {
            with_user_access(|| {
                let slice = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, count) };
                crate::tty::DEFAULT_TTY.sys_read(slice)
            })
        },
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
            let read = match crate::fs::vfs_read_at(&inode, offset, slice) {
                Ok(value) => value,
                Err(err) => return vfs_errno(err),
            };
            if is_hello && read > 0 {
                crate::serial_println!(
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

fn sys_open(path: usize, _flags: usize, _mode: usize) -> usize {
    let path = match read_user_cstring(path, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    match path.as_str() {
        "/dev/null" => allocate_fd(FdKind::Null),
        "/dev/zero" => allocate_fd(FdKind::Zero),
        "/dev/dri/card0" => allocate_fd(FdKind::Drm),
        _ => {
            let inode = match crate::fs::vfs_open_inode(&path) {
                Ok(value) => value,
                Err(err) => return vfs_errno(err),
            };
            let size = match crate::fs::vfs_inode_metadata(&inode) {
                Ok(meta) => meta.size,
                Err(err) => return vfs_errno(err),
            };
            let is_hello = path.eq_ignore_ascii_case("HELLO.ELF") || path.ends_with("/HELLO.ELF");
            if is_hello {
                crate::serial_println!("VFS: open HELLO.ELF size={}", size);
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

/// lseek syscall (stub)
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
    if crate::fs::f2fs::detect_f2fs().unwrap_or(false) {
        let entry = match crate::fs::f2fs::open_entry(&path) {
            Ok(value) => value,
            Err(_) => return errno(ENOENT),
        };
        let stat = stat_from_f2fs_entry(&entry);
        with_user_access(|| unsafe { *(statbuf as *mut Stat) = stat });
        return 0;
    }
    let entry = match crate::fs::f2fs::open_entry(&path) {
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

fn sys_fstat(_fd: usize, _statbuf: usize) -> usize {
    errno(ENOSYS)
}

fn sys_lstat(_path: usize, _statbuf: usize) -> usize {
    errno(ENOSYS)
}

fn sys_poll(_fds: usize, _nfds: usize, _timeout: usize) -> usize {
    errno(ENOSYS)
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
    if !is_anon && off % crate::memory::PAGE_SIZE != 0 {
        return errno(EINVAL);
    }
    let target = if addr != 0 {
        if !crate::memory::is_user_range(addr as u64, len as u64) {
            return errno(EINVAL);
        }
        addr as u64
    } else {
        match crate::memory::allocate_user_mmap(len as u64) {
            Some(value) => value,
            None => return errno(ENOMEM),
        }
    };
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
            if !crate::memory::register_shared_anon_region(target, len as u64, page_flags) {
                return errno(EINVAL);
            }
            return target as usize;
        }
        if !crate::memory::register_lazy_region(target, len as u64, page_flags) {
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
    if !crate::memory::register_file_backed_region(
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
    if !crate::memory::is_user_range(addr as u64, len as u64) {
        return errno(EINVAL);
    }
    let mut page_flags = PageTableFlags::USER_ACCESSIBLE;
    if prot & PROT_WRITE != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if prot & PROT_EXEC == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    if !crate::memory::update_user_region_flags(addr as u64, len as u64, page_flags) {
        return errno(EINVAL);
    }
    0
}

fn sys_munmap(_addr: usize, _len: usize) -> usize {
    if _len == 0 {
        return errno(EINVAL);
    }
    if !crate::memory::is_user_range(_addr as u64, _len as u64) {
        return errno(EINVAL);
    }
    if !crate::memory::unmap_user_range(_addr as u64, _len as u64) {
        return errno(EINVAL);
    }
    0
}

fn sys_brk(addr: usize) -> usize {
    let (base, current) = crate::memory::user_heap_state();
    if addr == 0 {
        return current as usize;
    }
    let new_break = addr as u64;
    let heap_limit = crate::memory::user_heap_limit();
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
        if !crate::memory::register_lazy_region(current, size, flags) {
            return current as usize;
        }
    } else {
        let size = current.saturating_sub(new_break);
        if !crate::memory::unmap_user_range(new_break, size) {
            return current as usize;
        }
    }
    crate::memory::set_user_heap_break(new_break);
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
    let handle = match crate::drivers::virtio_gpu::drm_resource_create_3d(width, height) {
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
        crate::drivers::virtio_gpu::drm_submit_3d_command(
            req.command as *const u8,
            req.size as usize,
        )
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
        let ptr = unsafe { crate::allocator::heap_alloc(size) };
        if ptr.is_null() {
            return errno(EIO);
        }
        entry.size = size;
        entry.map_ptr = ptr as usize;
    }
    entry.map_ptr
}

fn sys_pread64(_fd: usize, _buf: usize, _count: usize, _pos: usize) -> usize {
    errno(ENOSYS)
}

fn sys_pwrite64(_fd: usize, _buf: usize, _count: usize, _pos: usize) -> usize {
    errno(ENOSYS)
}

fn sys_readv(_fd: usize, _iov: usize, _iovcnt: usize) -> usize {
    errno(ENOSYS)
}

fn sys_writev(_fd: usize, _iov: usize, _iovcnt: usize) -> usize {
    errno(ENOSYS)
}

fn sys_access(_path: usize, _mode: usize) -> usize {
    errno(ENOSYS)
}

// ============================================================================
// FILESYSTEM SYSCALLS
// ============================================================================

/// mkdir - create a directory
fn sys_mkdir(_path_ptr: usize, _mode: usize) -> usize {
    // TODO: Implement with F2FS when VFS layer is ready
    errno(ENOSYS)
}

/// rmdir - remove a directory
fn sys_rmdir(_path_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// unlink - remove a file
fn sys_unlink(_path_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// rename - rename a file or directory
fn sys_rename(_oldpath_ptr: usize, _newpath_ptr: usize) -> usize {
    errno(ENOSYS)
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

/// truncate - truncate a file
fn sys_truncate(_path_ptr: usize, _length: usize) -> usize {
    errno(ENOSYS)
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
    errno(ENOSYS)
}

/// symlink - create a symbolic link
fn sys_symlink(_target_ptr: usize, _linkpath_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// readlink - read a symbolic link
fn sys_readlink(_path_ptr: usize, _buf: usize, _bufsize: usize) -> usize {
    errno(ENOSYS)
}

fn sys_select(
    _nfds: usize,
    _readfds: usize,
    _writefds: usize,
    _exceptfds: usize,
    _timeout: usize,
) -> usize {
    errno(ENOSYS)
}

fn sys_sched_yield() -> usize {
    crate::task::scheduler::sleep(1);
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
        let new_action: SigAction = with_user_access(|| unsafe { 
            core::ptr::read(act_ptr as *const SigAction) 
        });
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
    crate::serial_println!("[SIGNAL] kill: pid={}, sig={}", pid, sig);
    
    // In real implementation, would:
    // 1. Find process by PID
    // 2. Check permissions
    // 3. Queue signal to process
    // 4. Wake up process if sleeping
    
    0
}

/// rt_sigqueueinfo - queue a signal and data to a process
fn sys_rt_sigqueueinfo(_pid: usize, _sig: usize, _info_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// sigaltstack - set and/or examine signal stack context
fn sys_sigaltstack(_ss_ptr: usize, _old_ss_ptr: usize) -> usize {
    errno(ENOSYS)
}

/// rt_sigsuspend - wait for a signal
fn sys_rt_sigsuspend(_mask_ptr: usize) -> usize {
    // Would block until signal received
    errno(EINTR)
}

/// rt_sigtimedwait - wait for a signal with timeout
fn sys_rt_sigtimedwait(_set_ptr: usize, _info_ptr: usize, _timeout_ptr: usize, _sigsetsize: usize) -> usize {
    errno(ENOSYS)
}

fn sys_mremap(_old: usize, _oldsz: usize, _newsz: usize, _flags: usize, _new: usize) -> usize {
    errno(ENOSYS)
}

fn sys_msync(_addr: usize, _len: usize, _flags: usize) -> usize {
    errno(ENOSYS)
}

fn sys_mincore(_addr: usize, _len: usize, _vec: usize) -> usize {
    errno(ENOSYS)
}

fn sys_madvise(_addr: usize, _len: usize, _advice: usize) -> usize {
    errno(ENOSYS)
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
    static ref SHM_TABLE: Mutex<alloc::collections::BTreeMap<usize, ShmSegment>> = Mutex::new(alloc::collections::BTreeMap::new());
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
    let addr = crate::memory::allocate_user_mmap(size as u64);
    let addr = match addr {
        Some(a) => a as usize,
        None => return errno(ENOMEM),
    };
    
    let segment = ShmSegment {
        key,
        size,
        addr,
        creator_pid: crate::task::scheduler::current_task_id(),
        ref_count: 1,
    };
    
    SHM_TABLE.lock().insert(shmid, segment);
    crate::serial_println!("[IPC] shmget: key={}, size={}, shmid={}", key, size, shmid);
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
    
    crate::serial_println!("[IPC] shmat: shmid={}, addr={}", shmid, addr);
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
                let _ = crate::memory::unmap_user_range(segment.addr as u64, segment.size as u64);
            }
            0
        }
        _ => 0,
    }
}

/// Pipe implementation
struct PipeBuffer {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
}

lazy_static! {
    static ref PIPE_TABLE: Mutex<alloc::collections::BTreeMap<usize, PipeBuffer>> = Mutex::new(alloc::collections::BTreeMap::new());
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
    
    crate::serial_println!("[IPC] pipe: read_fd={}, write_fd={}", read_fd, write_fd);
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

/// Message queue stubs
fn sys_msgget(_key: usize, _msgflg: usize) -> usize {
    errno(ENOSYS)
}

fn sys_msgsnd(_msqid: usize, _msgp: usize, _msgsz: usize, _msgflg: usize) -> usize {
    errno(ENOSYS)
}

fn sys_msgrcv(_msqid: usize, _msgp: usize, _msgsz: usize, _msgtyp: usize, _msgflg: usize) -> usize {
    errno(ENOSYS)
}

fn sys_msgctl(_msqid: usize, _cmd: usize, _buf: usize) -> usize {
    errno(ENOSYS)
}

/// Semaphore stubs
fn sys_semget(_key: usize, _nsems: usize, _semflg: usize) -> usize {
    errno(ENOSYS)
}

fn sys_semop(_semid: usize, _sops: usize, _nsops: usize) -> usize {
    errno(ENOSYS)
}

fn sys_semctl(_semid: usize, _semnum: usize, _cmd: usize, _arg: usize) -> usize {
    errno(ENOSYS)
}

/// Futex (fast userspace mutex)
fn sys_futex(_uaddr: usize, _op: usize, _val: usize, _timeout: usize, _uaddr2: usize, _val3: usize) -> usize {
    errno(ENOSYS)
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
    static ref TIMER_TABLE: Mutex<alloc::collections::BTreeMap<usize, Timer>> = Mutex::new(alloc::collections::BTreeMap::new());
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
    
    crate::serial_println!("[TIMER] timer_create: timerid={}", timerid);
    0
}

/// timer_settime - set timer expiration
fn sys_timer_settime(timerid: usize, _flags: usize, new_value_ptr: usize, old_value_ptr: usize) -> usize {
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
struct EpollEvent {
    events: u32,
    data: u64,
}

lazy_static! {
    static ref EPOLL_TABLE: Mutex<alloc::collections::BTreeMap<usize, EpollInstance>> = Mutex::new(alloc::collections::BTreeMap::new());
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
    
    let epoll = EpollInstance {
        events: Vec::new(),
    };
    
    EPOLL_TABLE.lock().insert(epollid, epoll);
    
    // Also create FD
    let fd = allocate_fd(FdKind::File);
    crate::serial_println!("[EPOLL] epoll_create1: epollid={}, fd={}", epollid, fd);
    fd
}

/// epoll_ctl - control an epoll instance
fn sys_epoll_ctl(_epfd: usize, _op: usize, _fd: usize, _event: usize) -> usize {
    // Add/modify/delete file descriptor from epoll interest list
    0
}

/// epoll_pwait - wait for events
fn sys_epoll_pwait(_epfd: usize, events_ptr: usize, maxevents: usize, _timeout: usize, _sigmask: usize) -> usize {
    // Would block until events available
    if events_ptr != 0 {
        // Return empty events for now
        let _ = maxevents;
    }
    0
}

/// eventfd2 - create event file descriptor
fn sys_eventfd2(_initval: usize, _flags: usize) -> usize {
    let fd = allocate_fd(FdKind::File);
    crate::serial_println!("[EVENTFD] eventfd2: fd={}", fd);
    fd
}

fn sys_pause() -> usize {
    crate::task::scheduler::sleep(1);
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
    crate::task::scheduler::sleep(ticks);
    0
}

fn load_user_file(path: &str) -> Result<Vec<u8>, usize> {
    let inode = match crate::fs::vfs_open_inode(path) {
        Ok(value) => value,
        Err(err) => return Err(vfs_errno(err)),
    };
    let size = match crate::fs::vfs_inode_metadata(&inode) {
        Ok(meta) => meta.size,
        Err(err) => return Err(vfs_errno(err)),
    } as usize;
    if size == 0 {
        return Err(errno(EINVAL));
    }
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = match crate::fs::vfs_read_at(&inode, offset, &mut data[offset..]) {
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

fn sys_getpid() -> usize {
    crate::task::scheduler::current_task_id()
}

fn sys_execve(path_ptr: usize, _argv: usize, _envp: usize) -> usize {
    let path = match read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let image = match load_user_file(&path) {
        Ok(value) => value,
        Err(err) => return err,
    };
    match crate::task::scheduler::exec_current_user_image(&image) {
        Ok(()) => 0,
        Err(()) => errno(EIO),
    }
}

fn sys_fork() -> usize {
    let (user_rsp, user_rip, _user_rflags) = crate::syscall::current_user_context();
    match crate::task::scheduler::fork_current_user_task(user_rip, user_rsp) {
        Some(pid) => pid,
        None => errno(ENOSYS),
    }
}

fn sys_wait4(pid: usize, status: usize, options: usize, _rusage: usize) -> usize {
    let pid = pid as isize;
    let nohang = options & WNOHANG != 0;
    loop {
        if let Some((tid, code)) = crate::task::scheduler::wait_for_terminated(pid) {
            if status != 0 {
                let value = ((code as u32 & 0xff) << 8) as i32;
                with_user_access(|| unsafe { *(status as *mut i32) = value });
            }
            return tid as usize;
        }
        if nohang {
            return 0;
        }
        crate::task::scheduler::sleep(1);
    }
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
            crate::task::scheduler::set_ptrace_flag(1);
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
            // Şimdilik stub, ileride SIGSTOP gönderilecek
            errno(ENOSYS)
        }
        PTRACE_DETACH => {
            0
        }
        _ => errno(ENOSYS),
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
    errno(ENOSYS)
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

fn sys_gettid() -> usize {
    crate::task::scheduler::current_task_id()
}

/// clone syscall - create child process or thread
fn sys_clone(flags: usize, child_stack: usize, ptid: usize, ctid: usize, newtls: usize) -> usize {
    // Clone flags
    const CLONE_VM: usize = 0x00000100;      // Share memory space
    const CLONE_PARENT_SETTID: usize = 0x00100000;
    const CLONE_CHILD_CLEARTID: usize = 0x00200000;
    
    let (user_rsp, user_rip, _user_rflags) = crate::syscall::current_user_context();
    
    // Determine if this is a thread or process creation
    let is_thread = (flags & CLONE_VM) != 0;
    
    // For threads, use provided stack; for processes, copy current stack
    let new_rsp = if is_thread && child_stack != 0 {
        child_stack
    } else {
        user_rsp as usize
    };
    
    // Use fork for process creation, simplified clone for threads
    match crate::task::scheduler::fork_current_user_task(user_rip, new_rsp as u64) {
        Some(pid) => {
            // Set parent TID if requested
            if (flags & CLONE_PARENT_SETTID) != 0 && ptid != 0 {
                with_user_access(|| unsafe { *(ptid as *mut usize) = pid });
            }
            // Set child TID if requested  
            if (flags & CLONE_CHILD_CLEARTID) != 0 && ctid != 0 {
                // Store ctid for later clearing on exit (simplified)
                let _ = ctid;
            }
            pid
        }
        None => errno(ENOSYS),
    }
}

/// set_tid_address - set pointer to thread ID
fn sys_set_tid_address(tidptr: usize) -> usize {
    // Simplified - just return current TID
    let _ = tidptr;
    crate::task::scheduler::current_task_id()
}

/// tgkill - send signal to thread
fn sys_tgkill(_tgid: usize, _tid: usize, sig: usize) -> usize {
    if sig == 0 {
        return 0; // Signal 0 is used to check existence
    }
    if sig > 64 {
        return errno(EINVAL);
    }
    
    // Simplified - just return success
    0
}

/// tkill - send signal to thread
fn sys_tkill(_tid: usize, sig: usize) -> usize {
    if sig == 0 {
        return 0;
    }
    if sig > 64 {
        return errno(EINVAL);
    }
    
    // Simplified - just return success
    0
}

/// setuid - set user ID
fn sys_setuid(uid: usize) -> usize {
    // In echOS, we don't have real user management yet
    // Just return success for compatibility
    let _ = uid;
    0
}

/// setgid - set group ID  
fn sys_setgid(gid: usize) -> usize {
    let _ = gid;
    0
}

/// setsid - create new session
fn sys_setsid() -> usize {
    // Return new session ID (same as PGID)
    crate::task::scheduler::current_task_id()
}

/// setpgid - set process group ID
fn sys_setpgid(pid: usize, pgid: usize) -> usize {
    // Simplified - just return success
    let _ = (pid, pgid);
    0
}

/// getpgid - get process group ID
fn sys_getpgid(pid: usize) -> usize {
    if pid == 0 {
        crate::task::scheduler::current_task_id()
    } else {
        pid
    }
}

/// getsid - get session ID
fn sys_getsid(pid: usize) -> usize {
    if pid == 0 {
        crate::task::scheduler::current_task_id()
    } else {
        pid
    }
}

/// exit/exit_group syscall
fn sys_exit(code: usize) -> usize {
    crate::task::scheduler::exit(code as i32);
}

/// clock_gettime syscall (tick tabanlı)
fn sys_clock_gettime(clock_id: usize, tp_ptr: usize) -> usize {
    if tp_ptr == 0 {
        return errno(EINVAL);
    }
    if clock_id != CLOCK_REALTIME && clock_id != CLOCK_MONOTONIC {
        return errno(EINVAL);
    }
    let ticks = crate::task::scheduler::get_ticks() as u64;
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
    if buf == 0 || !crate::memory::is_user_range(buf as u64, len as u64) {
        return errno(EFAULT);
    }
    if flags & GRND_DETERMINISTIC != 0 {
        let out =
            with_user_access(|| unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) });
        crate::random::fill_bytes_deterministic(out);
        return len;
    }
    let ticks = crate::task::scheduler::get_ticks() as u64;
    let tsc = unsafe { _rdtsc() };
    let mut mix = ticks ^ tsc ^ (buf as u64) ^ (len as u64);
    if flags & GRND_RANDOM != 0 {
        mix ^= mix.rotate_left(29);
    }
    crate::random::add_entropy(mix);
    let out = with_user_access(|| unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, len) });
    crate::random::fill_bytes(out);
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

// echOS io_uring Instance Yapısı (Kernel İçi Tutulan Dosya Karşılığı)
pub struct IoUringInstance {
    pub sq_entries: u32,
    pub cq_entries: u32,
    pub sqe_array: usize, // User map pointer
    pub cqe_array: usize, // User map pointer
}

fn sys_io_uring_setup(entries: usize, params_ptr: usize) -> usize {
    crate::serial_println!("[io_uring] setup called! Entries: {}", entries);
    
    if entries == 0 || entries > 4096 {
        return errno(EINVAL);
    }
    if params_ptr == 0 {
        return errno(EFAULT);
    }

    // 1. Kernel'da iki frame tahsis et (Submission ve Completion)
    // Gerçekte mmap ile share edileceği için allocate_user_mmap
    let sq_size = entries as u64 * core::mem::size_of::<IoUringSqe>() as u64;
    let cq_size = entries as u64 * core::mem::size_of::<IoUringCqe>() as u64;
    
    // Güvenliği için 1 Page'e (4096) yuvarla
    let alloc_sq = x86_64::align_up(sq_size, 4096);
    let alloc_cq = x86_64::align_up(cq_size, 4096);

    let sq_ptr = match crate::memory::allocate_user_mmap(alloc_sq) {
        Some(ptr) => ptr,
        None => return errno(ENOMEM),
    };

    let cq_ptr = match crate::memory::allocate_user_mmap(alloc_cq) {
        Some(ptr) => ptr,
        None => return errno(ENOMEM),
    };

    let fd = allocate_fd(FdKind::IoUring);
    if fd >= MAX_FDS {
        return errno(EMFILE);
    }

    // 2. Instance nesnesi kaydet
    let inst = IoUringInstance {
        sq_entries: entries as u32,
        cq_entries: entries as u32,
        sqe_array: sq_ptr as usize,
        cqe_array: cq_ptr as usize,
    };
    RING_TABLE.lock().insert(fd, inst);

    // TODO: parameters struct içerisine offset'leri geri ver ki user space bilsin.

    fd
}

fn sys_io_uring_enter(fd: usize, to_submit: usize, min_complete: usize, _flags: usize, _sig: usize, _sigsz: usize) -> usize {
    let mut table = RING_TABLE.lock();
    if let Some(inst) = table.get_mut(&fd) {
        if to_submit > 0 {
            // Submission Queue (SQ) pointer'ına user alanından dönüştürerek git
            // Gerçekte head/tail index'leriyle çalışır, şimdilik mock tarama
            let sqes = unsafe { core::slice::from_raw_parts(inst.sqe_array as *const IoUringSqe, inst.sq_entries as usize) };
            let cqes = unsafe { core::slice::from_raw_parts_mut(inst.cqe_array as *mut IoUringCqe, inst.cq_entries as usize) };
            
            // "to_submit" sayısı kadar SQE oku, opcode'unu icra et
            let mut submitted = 0;
            for i in 0..to_submit {
                if i >= inst.sq_entries as usize { break; }
                
                let sqe = &sqes[i];
                let opcode = sqe.opcode;
                let user_data = sqe.user_data;
                
                let cqe_ptr_usize = inst.cqe_array;
                let cqe_index = i;

                // --- KERNEL WORKER THREAD DİSPATCH (ASENKRON) ---
                // Gerçek io_uring, iş yükünü kernel thread havuzuna böler.
                crate::task::worker::spawn_work(move || {
                    let res = match opcode {
                        // IORING_OP_NOP (0)
                        0 => 0, 
                        // IORING_OP_READV / WRITEV vb opcode'lar buraya gelecek...
                        _ => -38, // -ENOSYS
                    };
                    
                    // Asenkron işlem bittiğinde CQ (Completion) event'i atılır
                    unsafe {
                        let cqe_ptr = cqe_ptr_usize as *mut IoUringCqe;
                        let cqe = &mut *cqe_ptr.add(cqe_index);
                        cqe.user_data = user_data;
                        cqe.res = res;
                        cqe.flags = 0;
                    }
                });
                
                submitted += 1;
            }
            return submitted;
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
const SOCK_STREAM: usize = 1;  // TCP
const SOCK_DGRAM: usize = 2;   // UDP

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
    crate::serial_println!("[SOCKET] domain={}, type={}, protocol={}", domain, type_, protocol);
    
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
        crate::serial_println!("[SOCKET] Bind fd={} to port {}", fd, port);
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
    crate::serial_println!("[SOCKET] Listen fd={} backlog={}", fd, backlog);
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
            // port (placeholder)
            *((addr_ptr + 2) as *mut u16) = 0;
            // addr (placeholder)
            core::ptr::write_bytes((addr_ptr + 4) as *mut u8, 0, 4);
        });
    }
    if addr_len_ptr != 0 {
        with_user_access(|| unsafe {
            *(addr_len_ptr as *mut u32) = 16; // sizeof(sockaddr_in)
        });
    }
    
    crate::serial_println!("[SOCKET] Accept fd={} -> new_fd={}", fd, new_fd);
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
    
    crate::serial_println!("[SOCKET] Connect fd={} to {}.{}.{}.{}:{}", 
        fd, addr[0], addr[1], addr[2], addr[3], port);
    0
}

fn sys_sendto(fd: usize, buf: usize, len: usize, flags: usize, addr_ptr: usize, addr_len: usize) -> usize {
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
    
    crate::serial_println!("[SOCKET] Send fd={} len={} bytes", fd, len);
    len
}

fn sys_recvfrom(fd: usize, buf: usize, len: usize, flags: usize, addr_ptr: usize, addr_len_ptr: usize) -> usize {
    let sockets = SOCKET_TABLE.lock();
    let Some(sock) = sockets.get(&fd) else {
        return errno(EBADF);
    };
    
    let _ = (sock, flags, addr_ptr, addr_len_ptr);
    
    // Would receive from network stack
    crate::serial_println!("[SOCKET] Recv fd={} len={}", fd, len);
    0
}

fn sys_setsockopt(_fd: usize, level: usize, optname: usize, _optval: usize, _optlen: usize) -> usize {
    // kTLS (Kernel TLS) rezervasyonu (Örn: TCP_ULP için hazırlık)
    if level == SOL_TCP && optname == 31 { // TCP_ULP
        crate::serial_println!("[kTLS] Kernel TLS Cipher Context reserved.");
        return 0;
    }
    errno(ENOSYS)
}

fn sys_getsockopt(_fd: usize, _level: usize, _optname: usize, _optval: usize, _optlen: usize) -> usize {
    errno(ENOSYS)
}

fn sys_shutdown(_fd: usize, _how: usize) -> usize {
    errno(ENOSYS)
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
    errno(ENOSYS)
}

fn sys_recvmsg(_fd: usize, _msg: usize, _flags: usize) -> usize {
    errno(ENOSYS)
}

// =====================================
// SECCOMP (SECURE COMPUTING)
// =====================================

fn sys_prctl(option: usize, arg2: usize) -> usize {
    if option == PR_SET_SECCOMP {
        if arg2 == SECCOMP_MODE_STRICT {
            let current_mode = crate::task::scheduler::get_current_seccomp_mode();
            if current_mode == 0 {
                crate::task::scheduler::set_current_seccomp_mode(1);
                crate::serial_println!("[SECCOMP] Strict Mode Enabled (PR_SET_SECCOMP)");
                return 0;
            }
        }
    }
    errno(EINVAL)
}

fn sys_seccomp(operation: usize, _flags: usize, _args: usize) -> usize {
    let current_mode = crate::task::scheduler::get_current_seccomp_mode();
    if current_mode != 0 {
        return errno(EPERM); // Mode zaten set edilmiş, değiştirilemez
    }
    
    if operation == 0 /* SECCOMP_SET_MODE_STRICT */ {
        crate::task::scheduler::set_current_seccomp_mode(1);
        crate::serial_println!("[SECCOMP] Strict Mode Enabled (sys_seccomp)");
        return 0;
    } else if operation == 1 /* SECCOMP_SET_MODE_FILTER */ {
        crate::task::scheduler::set_current_seccomp_mode(2);
        crate::serial_println!("[SECCOMP] Filter Mode Enabled (sys_seccomp)");
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

fn with_user_access<R>(f: impl FnOnce() -> R) -> R {
    let smap = crate::cpu::smap_enabled();
    if smap {
        unsafe { crate::cpu::stac() };
    }
    let result = f();
    if smap {
        unsafe { crate::cpu::clac() };
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WineRuntimeKind {
    Wine,
    Proton,
}

#[derive(Clone, Debug)]
pub struct WineRuntime {
    pub name: String,
    pub root_path: String,
    pub kind: WineRuntimeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WineRuntimeError {
    NotFound,
    Invalid,
    NotSupported,
    SecureBootViolation,
}

static WINE_RUNTIME_REGISTRY: Mutex<Vec<WineRuntime>> = Mutex::new(Vec::new());
static WINE_RUNTIME_ACTIVE: Mutex<Option<usize>> = Mutex::new(None);

pub fn upsert_wine_runtime(
    name: &str,
    root_path: &str,
    kind: WineRuntimeKind,
) -> Result<usize, WineRuntimeError> {
    if name.trim().is_empty() || root_path.trim().is_empty() {
        return Err(WineRuntimeError::Invalid);
    }
    let mut registry = WINE_RUNTIME_REGISTRY.lock();
    if let Some((idx, runtime)) = registry
        .iter_mut()
        .enumerate()
        .find(|(_, r)| r.name == name)
    {
        runtime.root_path = root_path.to_string();
        runtime.kind = kind;
        *WINE_RUNTIME_ACTIVE.lock() = Some(idx);
        return Ok(idx);
    }
    registry.push(WineRuntime {
        name: name.to_string(),
        root_path: root_path.to_string(),
        kind,
    });
    let idx = registry.len() - 1;
    *WINE_RUNTIME_ACTIVE.lock() = Some(idx);
    Ok(idx)
}

pub fn list_wine_runtimes() -> Vec<WineRuntime> {
    WINE_RUNTIME_REGISTRY.lock().clone()
}

pub fn select_wine_runtime(name: &str) -> Result<(), WineRuntimeError> {
    let idx = WINE_RUNTIME_REGISTRY
        .lock()
        .iter()
        .position(|runtime| runtime.name == name)
        .ok_or(WineRuntimeError::NotFound)?;
    *WINE_RUNTIME_ACTIVE.lock() = Some(idx);
    Ok(())
}

pub fn current_wine_runtime() -> Option<WineRuntime> {
    let idx = *WINE_RUNTIME_ACTIVE.lock();
    let registry = WINE_RUNTIME_REGISTRY.lock();
    idx.and_then(|value| registry.get(value).cloned())
}

pub fn run_windows_app(path: &str) -> Result<(), WineRuntimeError> {
    if path.trim().is_empty() {
        return Err(WineRuntimeError::Invalid);
    }
    current_wine_runtime().ok_or(WineRuntimeError::NotFound)?;
    let image = load_windows_image(path)?;
    run_windows_app_image(&image)
}

fn load_windows_image(path: &str) -> Result<Vec<u8>, WineRuntimeError> {
    let inode = crate::fs::vfs_open_inode(path).map_err(|_| WineRuntimeError::Invalid)?;
    let size = crate::fs::vfs_inode_metadata(&inode)
        .map_err(|_| WineRuntimeError::Invalid)?
        .size;
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = crate::fs::vfs_read_at(&inode, offset, &mut data[offset..])
            .map_err(|_| WineRuntimeError::Invalid)?;
        if read == 0 {
            break;
        }
        offset += read;
    }
    data.truncate(offset);
    Ok(data)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeInfo {
    pub is_64: bool,
    pub machine: u16,
    pub section_count: u16,
    pub entry_rva: u32,
    pub image_base: u64,
    pub subsystem: u16,
}

#[derive(Clone, Debug)]
pub struct PeImage {
    pub info: PeInfo,
    pub image: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PeSectionInfo {
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_pointer: u32,
    pub raw_size: u32,
}

#[derive(Clone, Debug)]
pub struct WineLaunchPlan {
    pub runtime: WineRuntime,
    pub pe_info: PeInfo,
}

pub fn pe_info_from_image(image: &[u8]) -> Result<PeInfo, WineRuntimeError> {
    parse_pe(image).map_err(|_| WineRuntimeError::Invalid)
}

pub fn prepare_windows_launch(image: &[u8]) -> Result<WineLaunchPlan, WineRuntimeError> {
    let runtime = current_wine_runtime().ok_or(WineRuntimeError::NotFound)?;
    if crate::boot::secure_boot_enabled() {
        verify_authenticode(image).map_err(|_| WineRuntimeError::SecureBootViolation)?;
    }
    let pe_info = parse_pe(image).map_err(|_| WineRuntimeError::Invalid)?;
    Ok(WineLaunchPlan { runtime, pe_info })
}

pub fn pe_sections_from_image(image: &[u8]) -> Result<Vec<PeSectionInfo>, WineRuntimeError> {
    let meta = parse_pe_meta(image).map_err(|_| WineRuntimeError::Invalid)?;
    Ok(meta
        .sections
        .into_iter()
        .map(|section| PeSectionInfo {
            name: section.name,
            virtual_address: section.virtual_address,
            virtual_size: section.virtual_size,
            raw_pointer: section.raw_pointer,
            raw_size: section.raw_size,
        })
        .collect())
}

pub fn run_windows_app_image(image: &[u8]) -> Result<(), WineRuntimeError> {
    let runtime = current_wine_runtime().ok_or(WineRuntimeError::NotFound)?;
    if crate::boot::secure_boot_enabled() {
        verify_authenticode(image).map_err(|_| WineRuntimeError::SecureBootViolation)?;
    }
    let loaded = load_pe_image(image).map_err(|_| WineRuntimeError::Invalid)?;
    crate::serial_println!(
        "wine launch runtime={} entry=0x{:08x} image_base=0x{:016x} pe64={}",
        runtime.name,
        loaded.info.entry_rva,
        loaded.info.image_base,
        loaded.info.is_64
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PeError {
    Invalid,
    OutOfBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SecureBootError {
    Invalid,
    OutOfBounds,
    NotSigned,
    SignatureInvalid,
    Revoked,
    MissingDb,
    RuntimeUnavailable,
    Unsupported,
}

impl From<PeError> for SecureBootError {
    fn from(value: PeError) -> Self {
        match value {
            PeError::Invalid => SecureBootError::Invalid,
            PeError::OutOfBounds => SecureBootError::OutOfBounds,
        }
    }
}

struct PeLayout {
    optional_offset: usize,
    optional_size: usize,
    is_64: bool,
    cert_table_offset: u32,
    cert_table_size: u32,
}

struct Pkcs7Info {
    econtent: Vec<u8>,
    content_digest: Vec<u8>,
    signed_attrs_der: Vec<u8>,
    signed_attrs_digest: Vec<u8>,
    signature: Vec<u8>,
    signer_issuer: Vec<u8>,
    signer_serial: Vec<u8>,
    certs: Vec<Vec<u8>>,
}

struct SignatureDatabase {
    hashes: Vec<[u8; 32]>,
    certs: Vec<Vec<u8>>,
}

struct CertDetails {
    issuer: Vec<u8>,
    subject: Vec<u8>,
    tbs: Vec<u8>,
    signature: Vec<u8>,
}

const OID_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];
const OID_MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04];
const OID_CONTENT_TYPE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x03];
const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_SHA256_WITH_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const EFI_CERT_SHA256: [u8; 16] = [
    0x26, 0x16, 0xc4, 0xc1, 0x4c, 0x50, 0x92, 0x40, 0xac, 0xa9, 0x41, 0xf9, 0x36, 0x93, 0x43, 0x28,
];
const EFI_CERT_X509: [u8; 16] = [
    0xa1, 0x59, 0xc0, 0xa5, 0xe4, 0x94, 0xa7, 0x4a, 0x87, 0xb5, 0xab, 0x15, 0x5c, 0x2b, 0xf0, 0x72,
];

fn verify_authenticode(image: &[u8]) -> Result<(), SecureBootError> {
    let layout = parse_pe_layout(image)?;
    if layout.cert_table_size == 0 || layout.cert_table_offset == 0 {
        return Err(SecureBootError::NotSigned);
    }
    let pkcs7 = extract_pkcs7(image, &layout)?;
    let info = parse_pkcs7(&pkcs7)?;
    let file_hash = compute_authenticode_hash(image, &layout)?;
    if info.content_digest.len() != file_hash.len() || info.content_digest != file_hash {
        return Err(SecureBootError::SignatureInvalid);
    }
    let econtent_hash = sha256_hash(&info.econtent);
    if info.signed_attrs_digest.len() != econtent_hash.len()
        || info.signed_attrs_digest != econtent_hash
    {
        return Err(SecureBootError::SignatureInvalid);
    }
    let signer_cert = select_signer_cert(&info)?;
    let rsa_key = extract_rsa_public_key(&signer_cert)?;
    let sig = RsaSignature::try_from(info.signature.as_slice())
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let verifying_key = VerifyingKey::<Sha256>::new(rsa_key);
    verifying_key
        .verify(&info.signed_attrs_der, &sig)
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let (_pk, _kek, db, dbx) = read_secure_boot_keys()?;
    if dbx.matches_hash(&file_hash) || dbx.matches_cert(&signer_cert) {
        return Err(SecureBootError::Revoked);
    }
    if db.matches_hash(&file_hash) {
        return Ok(());
    }
    if db.matches_cert(&signer_cert) {
        return Ok(());
    }
    if verify_cert_chain_to_db(&signer_cert, &info.certs, &db, &dbx)? {
        Ok(())
    } else {
        Err(SecureBootError::SignatureInvalid)
    }
}

fn parse_pe_layout(image: &[u8]) -> Result<PeLayout, SecureBootError> {
    if image.len() < 64 {
        return Err(SecureBootError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(SecureBootError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(SecureBootError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if optional_size < 64 {
        return Err(SecureBootError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let is_64 = match magic {
        0x20B => true,
        0x10B => false,
        _ => return Err(SecureBootError::Invalid),
    };
    let data_dir_offset = optional_offset + if is_64 { 112 } else { 96 };
    let optional_end = optional_offset + optional_size;
    let security_index = 4usize;
    let security_offset = data_dir_offset + security_index * 8;
    let mut cert_table_offset = 0u32;
    let mut cert_table_size = 0u32;
    if security_offset + 8 <= optional_end {
        cert_table_offset = read_u32(image, security_offset)?;
        cert_table_size = read_u32(image, security_offset + 4)?;
    }
    Ok(PeLayout {
        optional_offset,
        optional_size,
        is_64,
        cert_table_offset,
        cert_table_size,
    })
}

fn compute_authenticode_hash(image: &[u8], layout: &PeLayout) -> Result<Vec<u8>, SecureBootError> {
    let checksum_offset = layout.optional_offset + 64;
    if checksum_offset + 4 > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    let data_dir_offset = layout.optional_offset + if layout.is_64 { 112 } else { 96 };
    let security_entry_offset = data_dir_offset + 4 * 8;
    if security_entry_offset + 8 > layout.optional_offset + layout.optional_size {
        return Err(SecureBootError::OutOfBounds);
    }
    let mut hasher = Sha256::new();
    hasher.update(&image[..checksum_offset]);
    hasher.update(&[0u8; 4]);
    let mut pos = checksum_offset + 4;
    if pos < security_entry_offset {
        hasher.update(&image[pos..security_entry_offset]);
    }
    pos = security_entry_offset + 8;
    let cert_offset = layout.cert_table_offset as usize;
    let cert_size = layout.cert_table_size as usize;
    if cert_offset == 0 || cert_size == 0 {
        if pos < image.len() {
            hasher.update(&image[pos..]);
        }
        let digest = hasher.finalize();
        return Ok(digest.to_vec());
    }
    if cert_offset > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if pos < cert_offset {
        hasher.update(&image[pos..cert_offset]);
    }
    let cert_end = cert_offset.saturating_add(cert_size).min(image.len());
    pos = cert_end;
    if pos < image.len() {
        hasher.update(&image[pos..]);
    }
    let digest = hasher.finalize();
    Ok(digest.to_vec())
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn extract_pkcs7(image: &[u8], layout: &PeLayout) -> Result<Vec<u8>, SecureBootError> {
    let cert_offset = layout.cert_table_offset as usize;
    let cert_size = layout.cert_table_size as usize;
    if cert_offset == 0 || cert_size == 0 {
        return Err(SecureBootError::NotSigned);
    }
    if cert_offset + cert_size > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    let mut pos = cert_offset;
    let end = cert_offset + cert_size;
    while pos + 8 <= end {
        let length = read_u32(image, pos)? as usize;
        if length < 8 || pos + length > end {
            return Err(SecureBootError::OutOfBounds);
        }
        let cert_type = read_u16(image, pos + 6)?;
        if cert_type == 0x0002 {
            return Ok(image[pos + 8..pos + length].to_vec());
        }
        let aligned = (length + 7) & !7;
        pos = pos.saturating_add(aligned);
    }
    Err(SecureBootError::NotSigned)
}

fn parse_pkcs7(data: &[u8]) -> Result<Pkcs7Info, SecureBootError> {
    let (_, seq, _, _) = der_expect_tag(data, 0x30)?;
    let mut rest = seq;
    let (_, oid, rest_after_oid, _) = der_expect_tag(rest, 0x06)?;
    if !oid_eq(oid, OID_SIGNED_DATA) {
        return Err(SecureBootError::Unsupported);
    }
    rest = rest_after_oid;
    let (_, signed_data_wrapper, _, _) = der_expect_tag(rest, 0xa0)?;
    let (_, signed_data, _, _) = der_expect_tag(signed_data_wrapper, 0x30)?;
    let mut sd = signed_data;
    sd = der_skip_tlv(sd)?;
    sd = der_skip_tlv(sd)?;
    let (_, encap, rest_after_encap, _) = der_expect_tag(sd, 0x30)?;
    let econtent = parse_encap_content(encap)?;
    let mut sd_rest = rest_after_encap;
    let mut certs = Vec::new();
    if let Some(tag) = sd_rest.first().copied() {
        if tag == 0xa0 {
            let (_, cert_block, rest_after_certs, _) = der_expect_tag(sd_rest, 0xa0)?;
            certs = parse_certificate_set(cert_block)?;
            sd_rest = rest_after_certs;
        }
    }
    let (_, signer_set, _, _) = der_expect_tag(sd_rest, 0x31)?;
    let signer_info = parse_first_signer_info(signer_set)?;
    let content_digest = parse_spc_indirect_data(&econtent)?;
    Ok(Pkcs7Info {
        econtent,
        content_digest,
        signed_attrs_der: signer_info.signed_attrs_der,
        signed_attrs_digest: signer_info.signed_attrs_digest,
        signature: signer_info.signature,
        signer_issuer: signer_info.signer_issuer,
        signer_serial: signer_info.signer_serial,
        certs,
    })
}

fn parse_encap_content(encap: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let mut rest = encap;
    let (_, _, rest_after_oid, _) = der_expect_tag(rest, 0x06)?;
    rest = rest_after_oid;
    let (_, content_wrapper, _, _) = der_expect_tag(rest, 0xa0)?;
    let (_, content_bytes, _, _) = der_expect_tag(content_wrapper, 0x04)?;
    Ok(content_bytes.to_vec())
}

fn parse_spc_indirect_data(content: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let (_, seq, _, _) = der_expect_tag(content, 0x30)?;
    let mut rest = seq;
    rest = der_skip_tlv(rest)?;
    let (_, digest_info, _, _) = der_expect_tag(rest, 0x30)?;
    let mut di = digest_info;
    let (_, alg_seq, rest_after_alg, _) = der_expect_tag(di, 0x30)?;
    let (_, alg_oid, _, _) = der_expect_tag(alg_seq, 0x06)?;
    if !oid_eq(alg_oid, OID_SHA256) {
        return Err(SecureBootError::Unsupported);
    }
    let (_, digest_bytes, _, _) = der_expect_tag(rest_after_alg, 0x04)?;
    Ok(digest_bytes.to_vec())
}

struct SignerInfo {
    signed_attrs_der: Vec<u8>,
    signed_attrs_digest: Vec<u8>,
    signature: Vec<u8>,
    signer_issuer: Vec<u8>,
    signer_serial: Vec<u8>,
}

fn parse_first_signer_info(data: &[u8]) -> Result<SignerInfo, SecureBootError> {
    let (_, signer_info_der, _, _) = der_expect_tag(data, 0x30)?;
    let mut rest = signer_info_der;
    rest = der_skip_tlv(rest)?;
    let (_, sid, rest_after_sid, _) = der_expect_tag(rest, 0x30)?;
    let (issuer, serial) = parse_signer_sid(sid)?;
    rest = rest_after_sid;
    rest = der_skip_tlv(rest)?;
    let (tag, attrs_value, rest_after_attrs, _) = der_read_tlv(rest)?;
    let signed_attrs_der = if tag == 0xa0 {
        build_set_der(attrs_value)
    } else {
        return Err(SecureBootError::Invalid);
    };
    let signed_attrs_digest = parse_signed_attrs_digest(attrs_value)?;
    rest = rest_after_attrs;
    rest = der_skip_tlv(rest)?;
    let (_, signature_bytes, _, _) = der_expect_tag(rest, 0x04)?;
    Ok(SignerInfo {
        signed_attrs_der,
        signed_attrs_digest,
        signature: signature_bytes.to_vec(),
        signer_issuer: issuer,
        signer_serial: serial,
    })
}

fn parse_signer_sid(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let mut rest = data;
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(rest)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let issuer = issuer_full.to_vec();
    let (_, serial_value, _, _) = der_expect_tag(rest_after_issuer, 0x02)?;
    Ok((issuer, serial_value.to_vec()))
}

fn parse_signed_attrs_digest(attrs: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let mut rest = attrs;
    let mut digest = None;
    while !rest.is_empty() {
        let (_, attr_seq, new_rest, _) = der_expect_tag(rest, 0x30)?;
        rest = new_rest;
        let mut attr_rest = attr_seq;
        let (_, oid, rest_after_oid, _) = der_expect_tag(attr_rest, 0x06)?;
        attr_rest = rest_after_oid;
        let (_, set_val, _, _) = der_expect_tag(attr_rest, 0x31)?;
        if oid_eq(oid, OID_MESSAGE_DIGEST) {
            let (_, digest_bytes, _, _) = der_expect_tag(set_val, 0x04)?;
            digest = Some(digest_bytes.to_vec());
        } else if oid_eq(oid, OID_CONTENT_TYPE) {
            let _ = der_expect_tag(set_val, 0x06)?;
        }
    }
    digest.ok_or(SecureBootError::Invalid)
}

fn parse_certificate_set(data: &[u8]) -> Result<Vec<Vec<u8>>, SecureBootError> {
    let mut certs = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (tag, _, new_rest, full) = der_read_tlv(rest)?;
        if tag == 0x30 {
            certs.push(full.to_vec());
        }
        rest = new_rest;
    }
    Ok(certs)
}

fn select_signer_cert(info: &Pkcs7Info) -> Result<Vec<u8>, SecureBootError> {
    for cert in &info.certs {
        if let Ok((issuer, serial, _)) = parse_cert_issuer_serial_key(cert) {
            if issuer == info.signer_issuer && serial == info.signer_serial {
                return Ok(cert.clone());
            }
        }
    }
    Err(SecureBootError::SignatureInvalid)
}

fn verify_cert_chain_to_db(
    signer_cert: &[u8],
    intermediates: &[Vec<u8>],
    db: &SignatureDatabase,
    dbx: &SignatureDatabase,
) -> Result<bool, SecureBootError> {
    let mut current = signer_cert.to_vec();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut depth = 0usize;
    loop {
        depth += 1;
        if depth > 8 {
            return Ok(false);
        }
        if dbx.matches_cert(&current) {
            return Err(SecureBootError::Revoked);
        }
        if db.matches_cert(&current) {
            return Ok(true);
        }
        let (issuer, subject) = parse_cert_subject_issuer(&current)?;
        if seen.iter().any(|s| s.as_slice() == subject.as_slice()) {
            return Ok(false);
        }
        seen.push(subject);
        let issuer_cert = find_cert_by_subject(&issuer, intermediates)
            .or_else(|| find_cert_by_subject(&issuer, &db.certs));
        let issuer_cert = match issuer_cert {
            Some(cert) => cert,
            None => return Ok(false),
        };
        verify_cert_signature(&current, issuer_cert)?;
        current = issuer_cert.clone();
    }
}

fn find_cert_by_subject<'a>(subject: &[u8], certs: &'a [Vec<u8>]) -> Option<&'a Vec<u8>> {
    for cert in certs {
        if let Ok((_, cert_subject)) = parse_cert_subject_issuer(cert) {
            if cert_subject == subject {
                return Some(cert);
            }
        }
    }
    None
}

fn verify_cert_signature(child_cert: &[u8], issuer_cert: &[u8]) -> Result<(), SecureBootError> {
    let details = parse_cert_details(child_cert)?;
    let rsa_key = extract_rsa_public_key(issuer_cert)?;
    let sig = RsaSignature::try_from(details.signature.as_slice())
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let verifying_key = VerifyingKey::<Sha256>::new(rsa_key);
    verifying_key
        .verify(&details.tbs, &sig)
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    Ok(())
}

fn parse_cert_details(cert: &[u8]) -> Result<CertDetails, SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (tbs_tag, tbs_value, rest_after_tbs, tbs_full) = der_read_tlv(cert_seq)?;
    if tbs_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let (issuer, subject) = parse_tbs_issuer_subject(tbs_value)?;
    let (_, sig_alg, rest_after_sig_alg, _) = der_expect_tag(rest_after_tbs, 0x30)?;
    let (_, sig_oid, _, _) = der_expect_tag(sig_alg, 0x06)?;
    if !oid_eq(sig_oid, OID_SHA256_WITH_RSA) {
        return Err(SecureBootError::Unsupported);
    }
    let (_, sig_bits, _, _) = der_expect_tag(rest_after_sig_alg, 0x03)?;
    if sig_bits.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    Ok(CertDetails {
        issuer,
        subject,
        tbs: tbs_full.to_vec(),
        signature: sig_bits[1..].to_vec(),
    })
}

fn parse_cert_subject_issuer(cert: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (tbs_tag, tbs_value, _, _) = der_read_tlv(cert_seq)?;
    if tbs_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    parse_tbs_issuer_subject(tbs_value)
}

fn parse_tbs_issuer_subject(tbs: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let mut tbs_rest = tbs;
    if let Some(tag) = tbs_rest.first().copied() {
        if tag == 0xa0 {
            tbs_rest = der_skip_tlv(tbs_rest)?;
        }
    }
    let (_, _, rest_after_serial, _) = der_expect_tag(tbs_rest, 0x02)?;
    tbs_rest = rest_after_serial;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(tbs_rest)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let issuer = issuer_full.to_vec();
    tbs_rest = rest_after_issuer;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (subject_tag, _, rest_after_subject, subject_full) = der_read_tlv(tbs_rest)?;
    if subject_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let subject = subject_full.to_vec();
    let _ = rest_after_subject;
    Ok((issuer, subject))
}

fn extract_rsa_public_key(cert: &[u8]) -> Result<RsaPublicKey, SecureBootError> {
    let (_, _, key) = parse_cert_issuer_serial_key(cert)?;
    Ok(key)
}

fn parse_cert_issuer_serial_key(
    cert: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, RsaPublicKey), SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (_, tbs, _, _) = der_expect_tag(cert_seq, 0x30)?;
    let mut tbs_rest = tbs;
    if let Some(tag) = tbs_rest.first().copied() {
        if tag == 0xa0 {
            tbs_rest = der_skip_tlv(tbs_rest)?;
        }
    }
    let (_, serial, rest_after_serial, _) = der_expect_tag(tbs_rest, 0x02)?;
    tbs_rest = rest_after_serial;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(tbs_rest)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let issuer = issuer_full.to_vec();
    tbs_rest = rest_after_issuer;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (_, spki, _, _) = der_expect_tag(tbs_rest, 0x30)?;
    let mut spki_rest = spki;
    let (_, alg_seq, rest_after_alg, _) = der_expect_tag(spki_rest, 0x30)?;
    let (_, alg_oid, _, _) = der_expect_tag(alg_seq, 0x06)?;
    if !oid_eq(alg_oid, OID_RSA_ENCRYPTION) {
        return Err(SecureBootError::Unsupported);
    }
    spki_rest = rest_after_alg;
    let (_, bit_string, _, _) = der_expect_tag(spki_rest, 0x03)?;
    if bit_string.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    let rsapub = &bit_string[1..];
    let (_, rsa_seq, _, _) = der_expect_tag(rsapub, 0x30)?;
    let mut rsa_rest = rsa_seq;
    let (_, modulus, rest_after_modulus, _) = der_expect_tag(rsa_rest, 0x02)?;
    rsa_rest = rest_after_modulus;
    let (_, exponent, _, _) = der_expect_tag(rsa_rest, 0x02)?;
    let n = BigUint::from_bytes_be(modulus);
    let e = BigUint::from_bytes_be(exponent);
    let key = RsaPublicKey::new(n, e).map_err(|_| SecureBootError::Invalid)?;
    Ok((issuer, serial.to_vec(), key))
}

fn der_read_tlv<'a>(
    input: &'a [u8],
) -> Result<(u8, &'a [u8], &'a [u8], &'a [u8]), SecureBootError> {
    if input.len() < 2 {
        return Err(SecureBootError::Invalid);
    }
    let tag = input[0];
    let (len, len_len) = der_read_len(&input[1..])?;
    let header = 1 + len_len;
    if input.len() < header + len {
        return Err(SecureBootError::OutOfBounds);
    }
    let value = &input[header..header + len];
    let rest = &input[header + len..];
    let full = &input[..header + len];
    Ok((tag, value, rest, full))
}

fn der_expect_tag<'a>(
    input: &'a [u8],
    expected: u8,
) -> Result<(u8, &'a [u8], &'a [u8], &'a [u8]), SecureBootError> {
    let (tag, value, rest, full) = der_read_tlv(input)?;
    if tag != expected {
        return Err(SecureBootError::Invalid);
    }
    Ok((tag, value, rest, full))
}

fn der_read_len(input: &[u8]) -> Result<(usize, usize), SecureBootError> {
    if input.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    let first = input[0];
    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 4 || input.len() < 1 + count {
        return Err(SecureBootError::Invalid);
    }
    let mut len = 0usize;
    for i in 0..count {
        len = (len << 8) | input[1 + i] as usize;
    }
    Ok((len, 1 + count))
}

fn der_skip_tlv<'a>(input: &'a [u8]) -> Result<&'a [u8], SecureBootError> {
    let (_, _, rest, _) = der_read_tlv(input)?;
    Ok(rest)
}

fn build_set_der(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + content.len());
    out.push(0x31);
    der_write_len(content.len(), &mut out);
    out.extend_from_slice(content);
    out
}

fn der_write_len(len: usize, out: &mut Vec<u8>) {
    if len < 0x80 {
        out.push(len as u8);
        return;
    }
    let mut tmp = [0u8; 4];
    let mut idx = 4;
    let mut val = len;
    while val > 0 {
        idx -= 1;
        tmp[idx] = (val & 0xff) as u8;
        val >>= 8;
    }
    let count = 4 - idx;
    out.push(0x80 | (count as u8));
    out.extend_from_slice(&tmp[idx..]);
}

fn oid_eq(value: &[u8], oid: &[u8]) -> bool {
    value == oid
}

pub fn secure_boot_db_available() -> bool {
    if !crate::boot::secure_boot_enabled() {
        return true;
    }
    read_secure_boot_keys().is_ok()
}

pub fn secure_boot_verify_image(image: &[u8]) -> bool {
    if !crate::boot::secure_boot_enabled() {
        return true;
    }
    verify_authenticode(image).is_ok()
}

fn read_secure_boot_keys() -> Result<
    (
        SignatureDatabase,
        SignatureDatabase,
        SignatureDatabase,
        SignatureDatabase,
    ),
    SecureBootError,
> {
    let pk = read_signature_database("PK", VariableVendor::GLOBAL_VARIABLE)?;
    let kek = read_signature_database("KEK", VariableVendor::GLOBAL_VARIABLE)?;
    let db = read_signature_database("db", VariableVendor::IMAGE_SECURITY_DATABASE)?;
    let dbx = read_signature_database("dbx", VariableVendor::IMAGE_SECURITY_DATABASE)?;
    if pk.hashes.is_empty() && pk.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    if kek.hashes.is_empty() && kek.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    if db.hashes.is_empty() && db.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    Ok((pk, kek, db, dbx))
}

fn read_signature_database(
    name: &'static str,
    vendor: VariableVendor,
) -> Result<SignatureDatabase, SecureBootError> {
    let data = read_uefi_variable(name, vendor)?;
    parse_signature_database(&data)
}

#[cfg(target_os = "uefi")]
fn uefi_variable_allowed(name: &str) -> bool {
    matches!(name, "PK" | "KEK" | "db" | "dbx")
}

#[cfg(target_os = "uefi")]
fn uefi_variable_attributes(name: &str) -> VariableAttributes {
    let _ = name;
    VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS
}

#[cfg(target_os = "uefi")]
fn read_uefi_variable(
    name: &'static str,
    vendor: VariableVendor,
) -> Result<Vec<u8>, SecureBootError> {
    if crate::boot::secure_boot_enabled() && !uefi_variable_allowed(name) {
        return Err(SecureBootError::Unsupported);
    }
    let runtime_services =
        crate::boot::runtime_services().ok_or(SecureBootError::RuntimeUnavailable)?;
    let mut buf = vec![0u16; name.encode_utf16().count() + 1];
    let var_name =
        CStr16::from_str_with_buf(name, &mut buf).map_err(|_| SecureBootError::Invalid)?;
    let (data, _) = runtime_services
        .get_variable_boxed(var_name, &vendor)
        .map_err(|_| SecureBootError::MissingDb)?;
    Ok(data.into_vec())
}

#[cfg(target_os = "uefi")]
#[allow(dead_code)]
fn write_uefi_variable(
    name: &'static str,
    vendor: VariableVendor,
    data: &[u8],
) -> Result<(), SecureBootError> {
    if crate::boot::secure_boot_enabled() && !uefi_variable_allowed(name) {
        return Err(SecureBootError::Unsupported);
    }
    let runtime_services =
        crate::boot::runtime_services().ok_or(SecureBootError::RuntimeUnavailable)?;
    let mut buf = vec![0u16; name.encode_utf16().count() + 1];
    let var_name =
        CStr16::from_str_with_buf(name, &mut buf).map_err(|_| SecureBootError::Invalid)?;
    let attributes = uefi_variable_attributes(name);
    runtime_services
        .set_variable(var_name, &vendor, attributes, data)
        .map_err(|_| SecureBootError::Invalid)?;
    Ok(())
}

#[cfg(not(target_os = "uefi"))]
fn read_uefi_variable(
    _name: &'static str,
    _vendor: VariableVendor,
) -> Result<Vec<u8>, SecureBootError> {
    Err(SecureBootError::RuntimeUnavailable)
}

#[cfg(not(target_os = "uefi"))]
#[allow(dead_code)]
fn write_uefi_variable(
    _name: &'static str,
    _vendor: VariableVendor,
    _data: &[u8],
) -> Result<(), SecureBootError> {
    Err(SecureBootError::RuntimeUnavailable)
}

fn parse_signature_database(data: &[u8]) -> Result<SignatureDatabase, SecureBootError> {
    let mut hashes = Vec::new();
    let mut certs = Vec::new();
    let mut offset = 0usize;
    while offset + 28 <= data.len() {
        let sig_type = &data[offset..offset + 16];
        let list_size = read_u32(data, offset + 16)? as usize;
        let header_size = read_u32(data, offset + 20)? as usize;
        let sig_size = read_u32(data, offset + 24)? as usize;
        if list_size < 28 || sig_size < 16 || offset + list_size > data.len() {
            return Err(SecureBootError::OutOfBounds);
        }
        let mut entry = offset + 28 + header_size;
        let list_end = offset + list_size;
        while entry + sig_size <= list_end {
            let sig_data = &data[entry + 16..entry + sig_size];
            if sig_type == EFI_CERT_SHA256.as_slice() {
                if sig_data.len() == 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(sig_data);
                    hashes.push(hash);
                }
            } else if sig_type == EFI_CERT_X509.as_slice() {
                certs.push(sig_data.to_vec());
            }
            entry += sig_size;
        }
        offset += list_size;
    }
    Ok(SignatureDatabase { hashes, certs })
}

impl SignatureDatabase {
    fn matches_hash(&self, hash: &[u8]) -> bool {
        if hash.len() != 32 {
            return false;
        }
        self.hashes.iter().any(|h| h.as_slice() == hash)
    }

    fn matches_cert(&self, cert: &[u8]) -> bool {
        self.certs.iter().any(|c| c.as_slice() == cert)
    }
}

fn load_pe_image(image: &[u8]) -> Result<PeImage, PeError> {
    let meta = parse_pe_meta(image)?;
    let size_of_image = meta.size_of_image as usize;
    if size_of_image == 0 || size_of_image > 256 * 1024 * 1024 {
        return Err(PeError::OutOfBounds);
    }
    let mut loaded = vec![0u8; size_of_image];
    let header_bytes = meta
        .size_of_headers
        .min(image.len() as u32)
        .min(size_of_image as u32) as usize;
    loaded[..header_bytes].copy_from_slice(&image[..header_bytes]);
    for section in meta.sections {
        if section.raw_size == 0 {
            continue;
        }
        let virt_start = section.virtual_address as usize;
        let raw_start = section.raw_pointer as usize;
        let raw_size = section.raw_size as usize;
        let virt_size = section.virtual_size as usize;
        let copy_size = if virt_size == 0 {
            raw_size
        } else {
            raw_size.min(virt_size)
        };
        if raw_start + raw_size > image.len() {
            return Err(PeError::OutOfBounds);
        }
        if virt_start + copy_size > loaded.len() {
            return Err(PeError::OutOfBounds);
        }
        loaded[virt_start..virt_start + copy_size]
            .copy_from_slice(&image[raw_start..raw_start + copy_size]);
    }
    Ok(PeImage {
        info: meta.info,
        image: loaded,
    })
}

struct PeSection {
    name: String,
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

struct PeMeta {
    info: PeInfo,
    size_of_image: u32,
    size_of_headers: u32,
    sections: Vec<PeSection>,
    cert_table_size: u32,
}

fn parse_pe_meta(image: &[u8]) -> Result<PeMeta, PeError> {
    if image.len() < 64 {
        return Err(PeError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(PeError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(PeError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let machine = read_u16(image, coff_offset)?;
    let section_count = read_u16(image, coff_offset + 2)?;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if optional_size < 64 {
        return Err(PeError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let (is_64, entry_rva, image_base, subsystem) = match magic {
        0x20B => {
            if optional_size < 112 {
                return Err(PeError::Invalid);
            }
            let entry_rva = read_u32(image, optional_offset + 16)?;
            let image_base = read_u64(image, optional_offset + 24)?;
            let subsystem = read_u16(image, optional_offset + 68)?;
            (true, entry_rva, image_base, subsystem)
        }
        0x10B => {
            if optional_size < 96 {
                return Err(PeError::Invalid);
            }
            let entry_rva = read_u32(image, optional_offset + 16)?;
            let image_base = read_u32(image, optional_offset + 28)? as u64;
            let subsystem = read_u16(image, optional_offset + 68)?;
            (false, entry_rva, image_base, subsystem)
        }
        _ => return Err(PeError::Invalid),
    };
    let size_of_image = read_u32(image, optional_offset + 56)?;
    let size_of_headers = read_u32(image, optional_offset + 60)?;
    let section_table = optional_offset + optional_size;
    let mut sections = Vec::new();
    let total_section_bytes = section_count as usize * 40;
    if section_table + total_section_bytes > image.len() {
        return Err(PeError::OutOfBounds);
    }
    for idx in 0..section_count as usize {
        let offset = section_table + idx * 40;
        let mut name = String::new();
        let name_end = (offset + 8).min(image.len());
        for &b in &image[offset..name_end] {
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        let virtual_size = read_u32(image, offset + 8)?;
        let virtual_address = read_u32(image, offset + 12)?;
        let raw_size = read_u32(image, offset + 16)?;
        let raw_pointer = read_u32(image, offset + 20)?;
        if virtual_address
            .checked_add(virtual_size)
            .map(|end| end as usize <= size_of_image as usize)
            != Some(true)
        {
            return Err(PeError::OutOfBounds);
        }
        sections.push(PeSection {
            name,
            virtual_address,
            virtual_size,
            raw_pointer,
            raw_size,
        });
    }
    let data_dir_offset = optional_offset + if is_64 { 112 } else { 96 };
    let mut cert_table_size = 0u32;
    let optional_end = optional_offset + optional_size;
    let security_index = 4usize;
    let security_offset = data_dir_offset + security_index * 8;
    if security_offset + 8 <= optional_end {
        let _ = read_u32(image, security_offset)?;
        cert_table_size = read_u32(image, security_offset + 4)?;
    }
    Ok(PeMeta {
        info: PeInfo {
            is_64,
            machine,
            section_count,
            entry_rva,
            image_base,
            subsystem,
        },
        size_of_image,
        size_of_headers,
        sections,
        cert_table_size,
    })
}

fn parse_pe(image: &[u8]) -> Result<PeInfo, PeError> {
    if image.len() < 64 {
        return Err(PeError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(PeError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(PeError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let machine = read_u16(image, coff_offset)?;
    let section_count = read_u16(image, coff_offset + 2)?;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if optional_size < 2 {
        return Err(PeError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let (is_64, entry_rva, image_base, subsystem) = match magic {
        0x20B => {
            if optional_size < 112 {
                return Err(PeError::Invalid);
            }
            let entry_rva = read_u32(image, optional_offset + 16)?;
            let image_base = read_u64(image, optional_offset + 24)?;
            let subsystem = read_u16(image, optional_offset + 68)?;
            (true, entry_rva, image_base, subsystem)
        }
        0x10B => {
            if optional_size < 96 {
                return Err(PeError::Invalid);
            }
            let entry_rva = read_u32(image, optional_offset + 16)?;
            let image_base = read_u32(image, optional_offset + 28)? as u64;
            let subsystem = read_u16(image, optional_offset + 68)?;
            (false, entry_rva, image_base, subsystem)
        }
        _ => return Err(PeError::Invalid),
    };
    Ok(PeInfo {
        is_64,
        machine,
        section_count,
        entry_rva,
        image_base,
        subsystem,
    })
}

fn read_u16(image: &[u8], offset: usize) -> Result<u16, PeError> {
    if offset + 2 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u16::from_le_bytes([image[offset], image[offset + 1]]))
}

fn read_u32(image: &[u8], offset: usize) -> Result<u32, PeError> {
    if offset + 4 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u32::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
    ]))
}

fn read_u64(image: &[u8], offset: usize) -> Result<u64, PeError> {
    if offset + 8 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u64::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
        image[offset + 4],
        image[offset + 5],
        image[offset + 6],
        image[offset + 7],
    ]))
}
