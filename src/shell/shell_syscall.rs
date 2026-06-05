//! # Ring 3 Syscall Wrapper'ları
//!
//! Shell Ring 3'te (user mode) çalıştığında kernel fonksiyonlarını
//! doğrudan çağıramaz. Bu modül, her kernel fonksiyonu için bir
//! syscall wrapper sağlar.
//!
//! x86_64 Linux Syscall Calling Convention:
//! - rax: syscall numarası
//! - rdi: arg1
//! - rsi: arg2
//! - rdx: arg3
//! - r10: arg4
//! - r8:  arg5
//! - r9:  arg6
//! - Dönüş: rax (başarı: pozitif, hata: negatif errno)

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ============================================================================
// SYSCALL NUMARALARI — POSIX/Linux uyumlu
// ============================================================================

// Dosya Sistemi
const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const SYS_STAT: usize = 4;
const SYS_FSTAT: usize = 5;
const SYS_LSTAT: usize = 6;
const SYS_LSEEK: usize = 8;
const SYS_MKDIR: usize = 83;
const SYS_UNLINK: usize = 87;
const SYS_RENAME: usize = 82;
const SYS_CHMOD: usize = 90;
const SYS_CHOWN: usize = 92;
const SYS_TRUNCATE: usize = 76;
const SYS_LINK: usize = 86;
const SYS_SYMLINK: usize = 88;
const SYS_READLINK: usize = 89;
const SYS_GETDENTS64: usize = 217;
const SYS_ACCESS: usize = 21;
const SYS_PIPE: usize = 22;
const SYS_DUP2: usize = 33;
const SYS_UMASK: usize = 60;
const SYS_MOUNT: usize = 165;
const SYS_UMOUNT2: usize = 166;
const SYS_STATFS: usize = 137;

// Süreç Yönetimi
const SYS_GETPID: usize = 39;
const SYS_GETPPID: usize = 110;
const SYS_FORK: usize = 57;
const SYS_EXECVE: usize = 59;
const SYS_EXIT: usize = 60;
const SYS_WAIT4: usize = 61;
const SYS_KILL: usize = 62;
const SYS_NANOSLEEP: usize = 35;
const SYS_GETUID: usize = 102;
const SYS_GETGID: usize = 104;
const SYS_GETEUID: usize = 107;
const SYS_GETEGID: usize = 108;
const SYS_GETTID: usize = 186;
const SYS_SETPGID: usize = 109;
const SYS_GETPGID: usize = 121;
const SYS_SETSID: usize = 112;
const SYS_GETSID: usize = 124;

// Sinyal
const SYS_RT_SIGACTION: usize = 13;
const SYS_RT_SIGPROCMASK: usize = 14;

// Terminal / IOCTL
const SYS_IOCTL: usize = 16;
const SYS_GETCWD: usize = 79;
const SYS_CHDIR: usize = 80;

// Bellek / Sistem
const SYS_SYSINFO: usize = 99;
const SYS_CLOCK_GETTIME: usize = 228;
const SYS_UNAME: usize = 63;

// Özel echOS syscall'ları (Linux ile çakışmayan aralık)
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

// ============================================================================
// DÜŞÜK SEVİYE SYSCALL ÇAĞRI MEKANİZMASI
// ============================================================================

/// x86_64 `syscall` talimatı ile çekirdeğe geçiş yapar.
/// Linux ABI: rax=num, rdi=arg1, rsi=arg2, rdx=arg3, r10=arg4, r8=arg5, r9=arg6
#[inline(always)]
unsafe fn raw_syscall(
    nr: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> isize {
    let ret: isize;
    core::arch::asm!(
        "syscall",
        inlateout("rax") nr => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        options(nostack, preserves_flags)
    );
    ret
}

/// Hata kodunu kontrol et — negatif ise errno döndür
#[inline(always)]
fn check_ret(ret: isize) -> Result<usize, i32> {
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(-ret as i32)
    }
}

// ============================================================================
// DOSYA SİSTEMİ SYSCALL'LARI
// ============================================================================

pub fn sys_open(path: &str, flags: u32) -> Result<usize, i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_OPEN,
            path_c.as_ptr() as usize,
            flags as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

pub fn sys_close(fd: usize) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_CLOSE, fd, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_read(fd: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_READ, fd, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_write(fd: usize, buf: &[u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_lseek(fd: usize, offset: isize, whence: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_LSEEK, fd, offset as usize, whence, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_mkdir(path: &str, mode: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_MKDIR,
            path_c.as_ptr() as usize,
            mode as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_unlink(path: &str) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(SYS_UNLINK, path_c.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_rename(old: &str, new: &str) -> Result<(), i32> {
    let old_c = alloc::format!("{}\0", old);
    let new_c = alloc::format!("{}\0", new);
    let ret = unsafe {
        raw_syscall(
            SYS_RENAME,
            old_c.as_ptr() as usize,
            new_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_chmod(path: &str, mode: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_CHMOD,
            path_c.as_ptr() as usize,
            mode as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_chown(path: &str, uid: u32, gid: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_CHOWN,
            path_c.as_ptr() as usize,
            uid as usize,
            gid as usize,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_truncate(path: &str, size: u64) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_TRUNCATE,
            path_c.as_ptr() as usize,
            size as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_link(target: &str, link: &str) -> Result<(), i32> {
    let target_c = alloc::format!("{}\0", target);
    let link_c = alloc::format!("{}\0", link);
    let ret = unsafe {
        raw_syscall(
            SYS_LINK,
            target_c.as_ptr() as usize,
            link_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_symlink(target: &str, link: &str) -> Result<(), i32> {
    let target_c = alloc::format!("{}\0", target);
    let link_c = alloc::format!("{}\0", link);
    let ret = unsafe {
        raw_syscall(
            SYS_SYMLINK,
            target_c.as_ptr() as usize,
            link_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_readlink(path: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_READLINK,
            path_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

pub fn sys_access(path: &str, mode: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_ACCESS,
            path_c.as_ptr() as usize,
            mode as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_pipe(fds: &mut [usize; 2]) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_PIPE, fds.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_dup2(old_fd: usize, new_fd: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_DUP2, old_fd, new_fd, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_getdents64(fd: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_GETDENTS64,
            fd,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

pub fn sys_statfs(path: &str, buf: &mut [u8]) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            SYS_STATFS,
            path_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_mount(source: &str, target: &str, fstype: &str, flags: u32) -> Result<(), i32> {
    let source_c = alloc::format!("{}\0", source);
    let target_c = alloc::format!("{}\0", target);
    let fstype_c = alloc::format!("{}\0", fstype);
    let ret = unsafe {
        raw_syscall(
            SYS_MOUNT,
            source_c.as_ptr() as usize,
            target_c.as_ptr() as usize,
            fstype_c.as_ptr() as usize,
            flags as usize,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_umount2(target: &str, flags: u32) -> Result<(), i32> {
    let target_c = alloc::format!("{}\0", target);
    let ret = unsafe {
        raw_syscall(
            SYS_UMOUNT2,
            target_c.as_ptr() as usize,
            flags as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_umask(mask: u32) -> u32 {
    let ret = unsafe { raw_syscall(SYS_UMASK, mask as usize, 0, 0, 0, 0, 0) };
    ret as u32
}

// ============================================================================
// SÜREÇ YÖNETİMİ SYSCALL'LARI
// ============================================================================

pub fn sys_getpid() -> usize {
    let ret = unsafe { raw_syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0) };
    ret as usize
}

pub fn sys_getppid() -> usize {
    let ret = unsafe { raw_syscall(SYS_GETPPID, 0, 0, 0, 0, 0, 0) };
    ret as usize
}

pub fn sys_getuid() -> u32 {
    let ret = unsafe { raw_syscall(SYS_GETUID, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_getgid() -> u32 {
    let ret = unsafe { raw_syscall(SYS_GETGID, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_geteuid() -> u32 {
    let ret = unsafe { raw_syscall(SYS_GETEUID, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_getegid() -> u32 {
    let ret = unsafe { raw_syscall(SYS_GETEGID, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_fork() -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_FORK, 0, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_execve(path: &str, argv: &[&str], envp: &[&str]) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);

    // argv array — NULL-terminated pointer array
    let mut argv_ptrs: Vec<usize> = argv.iter().map(|a| a.as_ptr() as usize).collect();
    argv_ptrs.push(0); // NULL terminator

    // envp array — NULL-terminated pointer array
    let mut envp_ptrs: Vec<usize> = envp.iter().map(|e| e.as_ptr() as usize).collect();
    envp_ptrs.push(0); // NULL terminator

    let ret = unsafe {
        raw_syscall(
            SYS_EXECVE,
            path_c.as_ptr() as usize,
            argv_ptrs.as_ptr() as usize,
            envp_ptrs.as_ptr() as usize,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_exit(code: i32) -> ! {
    unsafe { raw_syscall(SYS_EXIT, code as usize, 0, 0, 0, 0, 0) };
    loop {}
}

pub fn sys_wait4(pid: isize, status: &mut i32, options: u32) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_WAIT4,
            pid as usize,
            status as *mut i32 as usize,
            options as usize,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

pub fn sys_kill(pid: usize, sig: i32) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_KILL, pid, sig as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_nanosleep(secs: u64, nanos: u64) -> Result<(), i32> {
    let req = [secs as usize, nanos as usize];
    let ret = unsafe { raw_syscall(SYS_NANOSLEEP, req.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_getcwd(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_GETCWD, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_chdir(path: &str) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(SYS_CHDIR, path_c.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_setsid() -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_SETSID, 0, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_setpgid(pid: usize, pgid: usize) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_SETPGID, pid, pgid, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_getpgid(pid: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_GETPGID, pid, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_getsid(pid: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_GETSID, pid, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_gettid() -> usize {
    let ret = unsafe { raw_syscall(SYS_GETTID, 0, 0, 0, 0, 0, 0) };
    ret as usize
}

// ============================================================================
// SİNYAL SYSCALL'LARI
// ============================================================================

pub fn sys_rt_sigaction(
    signum: usize,
    act: usize,
    oldact: usize,
    sigsetsize: usize,
) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_RT_SIGACTION, signum, act, oldact, sigsetsize, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_rt_sigprocmask(
    how: usize,
    set: usize,
    oldset: usize,
    sigsetsize: usize,
) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_RT_SIGPROCMASK, how, set, oldset, sigsetsize, 0, 0) };
    check_ret(ret).map(|_| ())
}

// ============================================================================
// TERMİNAL / IOCTL SYSCALL'LARI
// ============================================================================

pub fn sys_ioctl(fd: usize, request: usize, arg: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_IOCTL, fd, request, arg, 0, 0, 0) };
    check_ret(ret)
}

// ============================================================================
// SİSTEM BİLGİSİ SYSCALL'LARI
// ============================================================================

pub fn sys_clock_gettime(clock_id: usize, tp: &mut [usize; 2]) -> Result<(), i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_CLOCK_GETTIME,
            clock_id,
            tp.as_ptr() as usize,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

pub fn sys_uname(buf: &mut [u8]) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_UNAME, buf.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_sysinfo(buf: &mut [u8]) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_SYSINFO, buf.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

// ============================================================================
// echOS ÖZEL SYSCALL'LARI
// ============================================================================

/// Çalışan task listesini al
pub fn sys_eon_list_tasks(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_LIST_TASKS,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Klavyeden tuş oku (non-blocking)
pub fn sys_eon_keyboard_read() -> Result<u8, i32> {
    let ret = unsafe { raw_syscall(SYS_EON_KEYBOARD_READ, 0, 0, 0, 0, 0, 0) };
    if ret >= 0 {
        Ok(ret as u8)
    } else {
        Err(-ret as i32)
    }
}

/// Ekranı temizle
pub fn sys_eon_term_clear() -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_EON_TERM_CLEAR, 0, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

/// Bellek istatistiklerini al
pub fn sys_eon_memory_stats(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_MEMORY_STATS,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// ELF binary'yi user mode'da çalıştır
pub fn sys_eon_spawn_elf(data: &[u8], priority: u32) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_SPAWN_ELF,
            data.as_ptr() as usize,
            data.len(),
            priority as usize,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Foreground process group ID'yi al
pub fn sys_eon_get_foreground() -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(SYS_EON_GET_FOREGROUND, 0, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

/// Foreground process group ID'yi ayarla
pub fn sys_eon_set_foreground(pgid: usize) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(SYS_EON_SET_FOREGROUND, pgid, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

/// Mount tablosunu listele
pub fn sys_eon_mount_list(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_MOUNT_LIST,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Sürücü listesini al
pub fn sys_eon_driver_list(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_DRIVER_LIST,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Ağ yapılandırmasını al
pub fn sys_eon_net_config(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_NET_CONFIG,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Sistem kapatma
pub fn sys_eon_shutdown() -> ! {
    unsafe { raw_syscall(SYS_EON_SHUTDOWN, 0, 0, 0, 0, 0, 0) };
    loop {}
}

/// Sistem yeniden başlatma
pub fn sys_eon_reboot() -> ! {
    unsafe { raw_syscall(SYS_EON_REBOOT, 0, 0, 0, 0, 0, 0) };
    loop {}
}

/// IPC mesajı gönder/al
pub fn sys_eon_ipc_send(
    service_id: usize,
    request: &[u8],
    response: &mut [u8],
) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            SYS_EON_IPC_SEND,
            service_id,
            request.as_ptr() as usize,
            request.len(),
            response.as_ptr() as usize,
            response.len(),
            0,
        )
    };
    check_ret(ret)
}

// ============================================================================
// IPC — PACKAGE REGISTRY SYSCALL'LARI
// ============================================================================

/// Paket kur (install)
pub fn sys_eon_ipc_pkg_install(path: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe {
        raw_syscall(
            520, // SYS_EON_IPC_PKG_INSTALL
            path_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Paket kaldır (remove)
pub fn sys_eon_ipc_pkg_remove(name: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            521, // SYS_EON_IPC_PKG_REMOVE
            name_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Paket listesi al
pub fn sys_eon_ipc_pkg_list(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            522, // SYS_EON_IPC_PKG_LIST
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Paket bilgisi al
pub fn sys_eon_ipc_pkg_info(name: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            523, // SYS_EON_IPC_PKG_INFO
            name_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Paket ara
pub fn sys_eon_ipc_pkg_search(term: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let term_c = alloc::format!("{}\0", term);
    let ret = unsafe {
        raw_syscall(
            524, // SYS_EON_IPC_PKG_SEARCH
            term_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Paket imzasını doğrula
pub fn sys_eon_ipc_pkg_verify(name: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            525, // SYS_EON_IPC_PKG_VERIFY
            name_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

// ============================================================================
// IPC — UPDATE INSTALLER SYSCALL'LARI
// ============================================================================

/// Update index'ini incele
pub fn sys_eon_ipc_update_inspect(locator: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let locator_c = alloc::format!("{}\0", locator);
    let ret = unsafe {
        raw_syscall(
            530, // SYS_EON_IPC_UPDATE_INSPECT
            locator_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Update'yi uygula
pub fn sys_eon_ipc_update_apply(locator: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let locator_c = alloc::format!("{}\0", locator);
    let ret = unsafe {
        raw_syscall(
            531, // SYS_EON_IPC_UPDATE_APPLY
            locator_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Update durumunu al
pub fn sys_eon_ipc_update_status(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            532, // SYS_EON_IPC_UPDATE_STATUS
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

// ============================================================================
// INIT / SERVİS SYSCALL'LARI
// ============================================================================

/// Hostname ayarla
pub fn sys_eon_set_hostname(name: &str) -> Result<(), i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            540, // SYS_EON_SET_HOSTNAME
            name_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| ())
}

/// Hostname al
pub fn sys_eon_get_hostname(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            541, // SYS_EON_GET_HOSTNAME
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Servisleri listele
pub fn sys_eon_list_services(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            542, // SYS_EON_LIST_SERVICES
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}

/// Servis başlat
pub fn sys_eon_start_service(name: &str) -> Result<String, i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            543, // SYS_EON_START_SERVICE
            name_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| alloc::format!("Service '{}' started", name))
}

/// Servis durdur
pub fn sys_eon_stop_service(name: &str) -> Result<String, i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe {
        raw_syscall(
            544, // SYS_EON_STOP_SERVICE
            name_c.as_ptr() as usize,
            0,
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret).map(|_| alloc::format!("Service '{}' stopped", name))
}

/// Servis durumunu al
pub fn sys_eon_service_status(name: &str) -> Result<String, i32> {
    let name_c = alloc::format!("{}\0", name);
    let mut buf = alloc::vec![0u8; 256];
    let ret = unsafe {
        raw_syscall(
            545, // SYS_EON_SERVICE_STATUS
            name_c.as_ptr() as usize,
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
        )
    };
    match check_ret(ret) {
        Ok(n) => core::str::from_utf8(&buf[..n])
            .map(|s| s.to_string())
            .map_err(|_| -84),
        Err(e) => Err(e),
    }
}

// ============================================================================
// SÜRÜCÜ (DRIVER) SYSCALL'LARI
// ============================================================================

/// RTC tarih/saat bilgisini al
pub fn sys_eon_rtc_datetime(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe {
        raw_syscall(
            550, // SYS_EON_RTC_DATETIME
            buf.as_ptr() as usize,
            buf.len(),
            0,
            0,
            0,
            0,
        )
    };
    check_ret(ret)
}
