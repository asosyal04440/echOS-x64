use super::super::{syscall, task};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub(super) fn sys_getpid() -> usize {
    // Linux: getpid() Thread Group ID (TGID) döndürür
    // CLONE_THREAD kullanıldığında tüm thread'ler aynı tgid'ye sahiptir
    get_current_tgid()
}

pub(super) fn sys_gettid() -> usize {
    // Linux: gettid() gerçek task ID (unique per thread) döndürür
    task::scheduler::current_task_id()
}

/// Current task'ın TGID'sini döndür
pub fn get_current_tgid() -> usize {
    let cpu_id = crate::cpu::smp::get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(task) = task::scheduler::PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
        {
            task.cold.tgid
        } else {
            task::scheduler::current_task_id()
        }
    })
}

fn read_user_string_array(ptr: usize) -> Result<Vec<String>, usize> {
    if ptr == 0 {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let max_args = 256;
    for i in 0..max_args {
        let pointer_addr = ptr + i * core::mem::size_of::<usize>();
        super::validate_user_ptr(pointer_addr)?;
        let str_ptr: usize = super::read_user(pointer_addr)?;
        if str_ptr == 0 {
            break;
        }
        let s = super::read_user_cstring(str_ptr, 4096)?;
        result.push(s);
    }
    Ok(result)
}

const SHEBANG_RECURSION_LIMIT: usize = 4;

enum ExecTarget {
    Elf(Vec<u8>),
    Shebang {
        interpreter: String,
        opt_arg: Option<String>,
        script_path: String,
    },
    Unknown,
}

fn classify_file(file_data: &[u8]) -> ExecTarget {
    if file_data.len() >= 2 && file_data[0] == b'#' && file_data[1] == b'!' {
        let first_line_end = file_data.iter().position(|&b| b == b'\n').unwrap_or(file_data.len());
        let line = &file_data[2..first_line_end];
        let line_str = alloc::string::String::from_utf8_lossy(line);
        let trimmed = line_str.trim();
        let mut parts = trimmed.splitn(2, |c: char| c == ' ' || c == '\t');
        let interpreter = parts.next().unwrap_or("").trim();
        let opt_arg = parts.next().map(|s| alloc::string::String::from(s.trim())).filter(|s| !s.is_empty());
        if interpreter.is_empty() {
            ExecTarget::Unknown
        } else {
            ExecTarget::Shebang {
                interpreter: alloc::string::String::from(interpreter),
                opt_arg,
                script_path: String::new(),
            }
        }
    } else if file_data.len() >= 4
        && file_data[0] == 0x7f
        && file_data[1] == b'E'
        && file_data[2] == b'L'
        && file_data[3] == b'F'
    {
        ExecTarget::Elf(file_data.to_vec())
    } else {
        ExecTarget::Unknown
    }
}

fn exec_with_shebang(
    interpreter: &str,
    opt_arg: Option<&str>,
    script_path: &str,
    original_argv: &[String],
    envp: &[String],
    depth: usize,
) -> usize {
    if depth >= SHEBANG_RECURSION_LIMIT {
        return super::errno(super::ELOOP);
    }
    let interp_data = match super::load_user_file(interpreter) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut new_argv = Vec::new();
    new_argv.push(interpreter.to_string());
    if let Some(arg) = opt_arg {
        if !arg.is_empty() {
            new_argv.push(arg.to_string());
        }
    }
    new_argv.push(script_path.to_string());
    for arg in original_argv.iter().skip(1) {
        new_argv.push(arg.clone());
    }
    exec_internal(interpreter, &new_argv, envp, &interp_data, depth + 1)
}

fn exec_internal(
    path: &str,
    argv: &[String],
    envp: &[String],
    file_data: &[u8],
    depth: usize,
) -> usize {
    match classify_file(file_data) {
        ExecTarget::Elf(_) => {
            match task::scheduler::exec_current_user_image_with_args(file_data, argv, envp) {
                Ok(()) => 0,
                Err(()) => super::errno(super::EIO),
            }
        }
        ExecTarget::Shebang { interpreter, opt_arg, script_path: _ } => {
            exec_with_shebang(&interpreter, opt_arg.as_deref(), path, argv, envp, depth)
        }
        ExecTarget::Unknown => super::errno(super::ENOEXEC),
    }
}

pub(super) fn sys_execve(path_ptr: usize, argv_ptr: usize, envp_ptr: usize) -> usize {
    let path = match super::read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let argv = match read_user_string_array(argv_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let envp = match read_user_string_array(envp_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let image = match super::load_user_file(&path) {
        Ok(value) => value,
        Err(err) => return err,
    };
    exec_internal(&path, &argv, &envp, &image, 0)
}

pub(super) fn sys_execveat(
    dirfd: usize,
    path_ptr: usize,
    argv_ptr: usize,
    envp_ptr: usize,
    _flags: usize,
) -> usize {
    const AT_FDCWD: usize = -100isize as usize;
    let path = if path_ptr == 0 {
        return super::errno(super::EINVAL);
    } else {
        match super::read_user_cstring(path_ptr, 256) {
            Ok(value) => value,
            Err(err) => return err,
        }
    };
    let resolved_path = if dirfd == AT_FDCWD || path.starts_with('/') {
        path.clone()
    } else {
        alloc::format!("/proc/self/fd/{}{}", dirfd, path)
    };
    let argv = match read_user_string_array(argv_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let envp = match read_user_string_array(envp_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let image = match super::load_user_file(&resolved_path) {
        Ok(value) => value,
        Err(err) => return err,
    };
    exec_internal(&resolved_path, &argv, &envp, &image, 0)
}

pub(super) fn sys_vfork() -> usize {
    // vfork(2) = clone(CLONE_VM | CLONE_VFORK | SIGCHLD)
    // Linux man page: "The calling thread is suspended until the child terminates
    // (either normally, by calling _exit(2), or abnormally, after delivery of a fatal signal),
    // or it makes a call to execve(2). Until that point, the child shares all memory
    // with its parent, including the stack."
    // Parent stack pointer must not be modified by child.
    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();
    let flags = crate::task::task::CLONE_VM
        | crate::task::task::CLONE_VFORK
        | crate::task::task::EXIT_SIGNAL_SIGCHLD;

    // fork_current_user_task_with_flags() CLONE_VFORK看見ると vfork_wait_child を作成する
    let child_pid = match task::scheduler::fork_current_user_task_with_flags(
        user_rip,
        user_rsp as usize as u64,
        flags,
        0, 0, 0, 0,
    ) {
        Some(pid) => pid,
        None => return super::errno(super::ENOMEM),
    };

    // Parent'ın vfork_wait_child gate'ini oku ve onunla block ol
    // Gate fork_current_user_task_with_flags içinde parent.cold.vfork_wait_child'a kaydedildi
    let parent_gate = x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let cpu_id = crate::cpu::smp::get_current_cpu_id();
        if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
            .get_mut(cpu_id as usize)
            .and_then(|t| t.as_mut())
        {
            current.cold.vfork_wait_child.clone()
        } else {
            None
        }
    });

    if let Some(gate) = parent_gate {
        task::scheduler::block_current_task_vfork(gate);
    }

    child_pid
}

pub(super) fn sys_fork() -> usize {
    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();
    // fork() = clone(SIGCHLD) — Linux: glibc fork wrapper'ı clone(SIGCHLD) çağırır
    // fork her zaman COW kullanır (CLONE_VM yok), exit_signal = SIGCHLD
    match task::scheduler::fork_current_user_task_with_flags(
        user_rip,
        user_rsp as usize as u64,
        crate::task::task::EXIT_SIGNAL_SIGCHLD, // flags = SIGCHLD (exit signal)
        0, 0, 0, 0,
    ) {
        Some(pid) => pid,
        None => super::errno(super::ENOMEM),
    }
}

pub(super) fn sys_wait4(pid: usize, status: usize, options: usize, _rusage: usize) -> usize {
    let pid = pid as isize;
    let nohang = options & super::WNOHANG != 0;
    let wuntraced = options & super::WUNTRACED != 0;
    loop {
        if let Some((tid, code)) = task::scheduler::wait_for_terminated(pid) {
            if status != 0 {
                let value = if code > 128 {
                    let sig = (code - 128) as u32 & 0x7F;
                    sig as i32
                } else {
                    ((code as u32 & 0xFF) << 8) as i32
                };
                if let Err(err) = super::write_user(status, value) {
                    return err;
                }
            }
            return tid as usize;
        }
        if wuntraced {
            if let Some((tid, code)) = task::scheduler::wait_for_stopped(pid) {
                if status != 0 {
                    let value = (code as i32 & 0xFF) << 8 | 0x7F;
                    if let Err(err) = super::write_user(status, value) {
                        return err;
                    }
                }
                return tid as usize;
            }
        }
        if nohang {
            return 0;
        }
        task::scheduler::sleep(1);
    }
}

pub(super) fn sys_clone(
    flags: usize,
    child_stack: usize,
    ptid: usize,
    ctid: usize,
    newtls: usize,
) -> usize {
    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();
    let effective_rsp = if child_stack != 0 { child_stack } else { user_rsp as usize };

    match task::scheduler::fork_current_user_task_with_flags(
        user_rip,
        effective_rsp as u64,
        flags,
        child_stack,
        ptid,
        ctid,
        newtls,
    ) {
        Some(pid) => pid,
        None => super::errno(super::ENOMEM),
    }
}

/// clone3(2) — struct clone_args ile yeni süreç/iş parçacığı oluşturur
/// Linux: clone3(cl_args, size) — clone_args yapısı:
///   flags, pidfd, child_tid, parent_tid, exit_signal, stack, stack_size, tls,
///   set_tid, set_tid_size, cgroup
/// Toplam: 11 alan × 8 byte = 88 byte
#[repr(C)]
#[derive(Clone, Copy)]
struct CloneArgs {
    flags: u64,
    pidfd: u64,
    child_tid: u64,
    parent_tid: u64,
    exit_signal: u64,
    stack: u64,
    stack_size: u64,
    tls: u64,
    set_tid: u64,
    set_tid_size: u64,
    cgroup: u64,
}

pub(super) fn sys_clone3(args_ptr: usize, size: usize) -> usize {
    // EINVAL: size must be >= sizeof(struct clone_args)
    if size < core::mem::size_of::<CloneArgs>() {
        return super::errno(super::EINVAL);
    }

    // Validate user pointer
    if super::validate_user_range(args_ptr, core::mem::size_of::<CloneArgs>()).is_err() {
        return super::errno(super::EFAULT);
    }

    // Read clone_args from userspace
    let args: CloneArgs = super::read_user(args_ptr).unwrap_or(CloneArgs {
        flags: 0,
        pidfd: 0,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: 0,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: 0,
    });

    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();

    // CLONE_NEWPID, CLONE_NEWUSER, CLONE_NEWNET vb. namespace'leri desteklemiyoruz
    let unsupported_ns = task::task::CLONE_NEWPID as u64
        | task::task::CLONE_NEWUSER as u64
        | task::task::CLONE_NEWNET as u64
        | task::task::CLONE_NEWIPC as u64
        | task::task::CLONE_NEWUTS as u64
        | task::task::CLONE_NEWNS as u64;
    if args.flags & unsupported_ns != 0 {
        return super::errno(super::ENOSYS);
    }

    // set_tid ve cgroup desteklenmiyor
    if args.set_tid != 0 || args.set_tid_size != 0 || args.cgroup != 0 {
        return super::errno(super::ENOSYS);
    }

    // exit_signal: clone3'te flags'in low byte'ı DEĞİL, ayrı bir alan
    // Linux: flags & 0xFF yerine exit_signal alanı kullanılır
    // flags'e exit_signal'ı ekle (fork_current_user_task_with_flags low byte'ı okur)
    let effective_flags = if args.exit_signal != 0 {
        (args.flags & !0xFF) | (args.exit_signal & 0xFF)
    } else {
        args.flags
    };

    // stack: clone3'te stack ve stack_size ayrı alanlar
    let effective_stack = if args.stack != 0 {
        args.stack as usize
    } else {
        user_rsp as usize
    };

    // CLONE_PIDFD: pidfd alloke et ve kullanıcıya yaz
    if args.pidfd != 0 && (effective_flags & task::task::CLONE_PIDFD as u64) != 0 {
        // pidfd alloc — minimum destek: sadece return value olarak child_pid
        // Gerçek pidfd_alloc pidfd_open benzeri bir dosya tanımlayıcı oluşturur
        crate::serial_println!("[CLONE3] CLONE_PIDFD istendi, pidfd_addr={:#x}", args.pidfd);
    }

    match task::scheduler::fork_current_user_task_with_flags(
        user_rip,
        effective_stack as u64,
        effective_flags as usize,
        args.stack as usize,
        args.parent_tid as usize,
        args.child_tid as usize,
        args.tls as usize,
    ) {
        Some(pid) => pid,
        None => super::errno(super::ENOMEM),
    }
}

pub(super) fn sys_set_tid_address(tidptr: usize) -> usize {
    let _ = tidptr;
    task::scheduler::current_task_id()
}

pub(super) fn sys_tgkill(_tgid: usize, _tid: usize, sig: usize) -> usize {
    if sig == 0 {
        return 0;
    }
    if sig > 64 {
        return super::errno(super::EINVAL);
    }
    0
}

pub(super) fn sys_tkill(_tid: usize, sig: usize) -> usize {
    if sig == 0 {
        return 0;
    }
    if sig > 64 {
        return super::errno(super::EINVAL);
    }
    0
}

pub(super) fn sys_setuid(uid: usize) -> usize {
    let _ = uid;
    0
}

pub(super) fn sys_setgid(gid: usize) -> usize {
    let _ = gid;
    0
}

pub(super) fn sys_setsid() -> usize {
    let pid = task::scheduler::current_task_id();
    let mut result = Ok(());
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
            .get(crate::cpu::smp::get_current_cpu_id() as usize)
            .and_then(|t| t.as_ref())
        {
            if current.cold.sid == pid {
                result = Err(());
            }
        }
    });
    if result.is_err() {
        return super::errno(super::EPERM);
    }
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
            .get_mut(crate::cpu::smp::get_current_cpu_id() as usize)
            .and_then(|t| t.as_mut())
        {
            current.cold.sid = pid;
            current.cold.pgid = pid;
        }
    });
    pid
}

pub(super) fn sys_setpgid(pid: usize, pgid: usize) -> usize {
    let target_pid = if pid == 0 {
        task::scheduler::current_task_id()
    } else {
        pid
    };
    let new_pgid = if pgid == 0 { target_pid } else { pgid };
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if target_pid == task::scheduler::current_task_id() {
            if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
                .get_mut(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_mut())
            {
                current.cold.pgid = new_pgid;
            }
        }
    });
    0
}

pub(super) fn sys_getpgid(pid: usize) -> usize {
    if pid == 0 {
        let mut pgid = 0;
        x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
            {
                pgid = current.cold.pgid;
            }
        });
        pgid
    } else {
        pid
    }
}

pub(super) fn sys_getsid(pid: usize) -> usize {
    if pid == 0 {
        let mut sid = 0;
        x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            if let Some(current) = task::scheduler::PER_CPU_CURRENT_TASK
                .get(crate::cpu::smp::get_current_cpu_id() as usize)
                .and_then(|t| t.as_ref())
            {
                sid = current.cold.sid;
            }
        });
        sid
    } else {
        pid
    }
}

pub(super) fn sys_exit(code: usize) -> usize {
    task::scheduler::exit(code as i32);
}
