//! # Ring 3 Syscall Wrapper'ları (echshell bağımsız)
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

// ============================================================================
// DÜŞÜK SEVİYE SYSCALL ÇAĞRI MEKANİZMASI
// ============================================================================

#[inline(always)]
pub unsafe fn raw_syscall(nr: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize) -> isize {
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
        out("rcx") _,
        out("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
fn check_ret(ret: isize) -> Result<usize, i32> {
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(-ret as i32)
    }
}

// ============================================================================
// DOSYA SİSTEMİ
// ============================================================================

pub fn sys_open(path: &str, flags: u32) -> Result<usize, i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(2 /*SYS_OPEN*/, path_c.as_ptr() as usize, flags as usize, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_close(fd: usize) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(3 /*SYS_CLOSE*/, fd, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_read(fd: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(0 /*SYS_READ*/, fd, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_write(fd: usize, buf: &[u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(1 /*SYS_WRITE*/, fd, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_getdents64(fd: usize, buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(217 /*SYS_GETDENTS64*/, fd, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_chdir(path: &str) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(80 /*SYS_CHDIR*/, path_c.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

#[allow(dead_code)]
pub fn sys_getcwd(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(79 /*SYS_GETCWD*/, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

// ============================================================================
// SÜREÇ YÖNETİMİ
// ============================================================================

pub fn sys_getpid() -> usize {
    let ret = unsafe { raw_syscall(39 /*SYS_GETPID*/, 0, 0, 0, 0, 0, 0) };
    ret as usize
}

#[allow(dead_code)]
pub fn sys_getppid() -> usize {
    let ret = unsafe { raw_syscall(110 /*SYS_GETPPID*/, 0, 0, 0, 0, 0, 0) };
    ret as usize
}

pub fn sys_fork() -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(57 /*SYS_FORK*/, 0, 0, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_execve(path: &str, argv: &[&str], envp: &[&str]) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);

    let mut argv_ptrs: alloc::vec::Vec<usize> = argv.iter().map(|a| a.as_ptr() as usize).collect();
    argv_ptrs.push(0);

    let mut envp_ptrs: alloc::vec::Vec<usize> = envp.iter().map(|e| e.as_ptr() as usize).collect();
    envp_ptrs.push(0);

    let ret = unsafe { raw_syscall(
        59 /*SYS_EXECVE*/,
        path_c.as_ptr() as usize,
        argv_ptrs.as_ptr() as usize,
        envp_ptrs.as_ptr() as usize,
        0, 0, 0
    ) };
    check_ret(ret).map(|_| ())
}

pub fn sys_exit(code: i32) -> ! {
    unsafe { raw_syscall(60 /*SYS_EXIT*/, code as usize, 0, 0, 0, 0, 0) };
    loop {}
}

pub fn sys_wait4(pid: isize, status: &mut i32, options: u32) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(
        61 /*SYS_WAIT4*/,
        pid as usize,
        status as *mut i32 as usize,
        options as usize,
        0, 0, 0
    ) };
    check_ret(ret)
}

pub fn sys_clock_gettime(clock_id: usize, tp: &mut [usize; 2]) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(228 /*SYS_CLOCK_GETTIME*/, clock_id, tp.as_ptr() as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

// ============================================================================
// echOS ÖZEL SYSCALL'LARI
// ============================================================================

pub fn sys_eon_list_tasks(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(500, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

#[allow(dead_code)]
pub fn sys_eon_keyboard_read() -> Result<u8, i32> {
    let ret = unsafe { raw_syscall(501, 0, 0, 0, 0, 0, 0) };
    if ret >= 0 { Ok(ret as u8) } else { Err(-ret as i32) }
}

pub fn sys_eon_term_clear() -> Result<(), i32> {
    let ret = unsafe { raw_syscall(502, 0, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_eon_memory_stats(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(503, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_eon_get_hostname(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(541, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_eon_rtc_datetime(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(550, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_dup2(old_fd: usize, new_fd: usize) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(33 /*SYS_DUP2*/, old_fd, new_fd, 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_pipe(fds: &mut [usize; 2]) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(22 /*SYS_PIPE*/, fds.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_getuid() -> u32 {
    let ret = unsafe { raw_syscall(102 /*SYS_GETUID*/, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_getgid() -> u32 {
    let ret = unsafe { raw_syscall(104 /*SYS_GETGID*/, 0, 0, 0, 0, 0, 0) };
    ret as u32
}

pub fn sys_time() -> i64 {
    let ret = unsafe { raw_syscall(201 /*SYS_TIME*/, 0, 0, 0, 0, 0, 0) };
    ret as i64
}

pub fn sys_nanosleep(secs: u64, nanos: u64) -> Result<(), i32> {
    let req = [secs as usize, nanos as usize];
    let ret = unsafe { raw_syscall(35 /*SYS_NANOSLEEP*/, req.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_eon_set_hostname(name: &str) -> Result<(), i32> {
    let name_c = alloc::format!("{}\0", name);
    let ret = unsafe { raw_syscall(540, name_c.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_eon_net_config(buf: &mut [u8]) -> Result<usize, i32> {
    let ret = unsafe { raw_syscall(509, buf.as_ptr() as usize, buf.len(), 0, 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_chmod(path: &str, mode: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(90 /*SYS_CHMOD*/, path_c.as_ptr() as usize, mode as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_chown(path: &str, uid: u32, gid: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(92 /*SYS_CHOWN*/, path_c.as_ptr() as usize, uid as usize, gid as usize, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_kill(pid: usize, sig: i32) -> Result<(), i32> {
    let ret = unsafe { raw_syscall(62 /*SYS_KILL*/, pid, sig as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_link(target: &str, link: &str) -> Result<(), i32> {
    let target_c = alloc::format!("{}\0", target);
    let link_c = alloc::format!("{}\0", link);
    let ret = unsafe { raw_syscall(86 /*SYS_LINK*/, target_c.as_ptr() as usize, link_c.as_ptr() as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_symlink(target: &str, link: &str) -> Result<(), i32> {
    let target_c = alloc::format!("{}\0", target);
    let link_c = alloc::format!("{}\0", link);
    let ret = unsafe { raw_syscall(88 /*SYS_SYMLINK*/, target_c.as_ptr() as usize, link_c.as_ptr() as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_readlink(path: &str, buf: &mut [u8]) -> Result<usize, i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(89 /*SYS_READLINK*/, path_c.as_ptr() as usize, buf.as_ptr() as usize, buf.len(), 0, 0, 0) };
    check_ret(ret)
}

pub fn sys_truncate(path: &str, size: u64) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(76 /*SYS_TRUNCATE*/, path_c.as_ptr() as usize, size as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_mkdir(path: &str, mode: u32) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(83 /*SYS_MKDIR*/, path_c.as_ptr() as usize, mode as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_rename(old: &str, new: &str) -> Result<(), i32> {
    let old_c = alloc::format!("{}\0", old);
    let new_c = alloc::format!("{}\0", new);
    let ret = unsafe { raw_syscall(82 /*SYS_RENAME*/, old_c.as_ptr() as usize, new_c.as_ptr() as usize, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}

pub fn sys_unlink(path: &str) -> Result<(), i32> {
    let path_c = alloc::format!("{}\0", path);
    let ret = unsafe { raw_syscall(87 /*SYS_UNLINK*/, path_c.as_ptr() as usize, 0, 0, 0, 0, 0) };
    check_ret(ret).map(|_| ())
}
