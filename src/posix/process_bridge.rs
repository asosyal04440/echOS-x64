use super::super::{syscall, task};

pub(super) fn sys_getpid() -> usize {
    task::scheduler::current_task_id()
}

pub(super) fn sys_execve(path_ptr: usize, _argv: usize, _envp: usize) -> usize {
    let path = match super::read_user_cstring(path_ptr, 256) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let image = match super::load_user_file(&path) {
        Ok(value) => value,
        Err(err) => return err,
    };
    match task::scheduler::exec_current_user_image(&image) {
        Ok(()) => 0,
        Err(()) => super::errno(super::EIO),
    }
}

pub(super) fn sys_fork() -> usize {
    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();
    match task::scheduler::fork_current_user_task(user_rip, user_rsp) {
        Some(pid) => pid,
        None => super::errno(super::ENOSYS),
    }
}

pub(super) fn sys_wait4(pid: usize, status: usize, options: usize, _rusage: usize) -> usize {
    let pid = pid as isize;
    let nohang = options & super::WNOHANG != 0;
    let _wuntraced = options & super::WUNTRACED != 0;
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
        if nohang {
            return 0;
        }
        task::scheduler::sleep(1);
    }
}

pub(super) fn sys_gettid() -> usize {
    task::scheduler::current_task_id()
}

pub(super) fn sys_clone(
    flags: usize,
    child_stack: usize,
    ptid: usize,
    ctid: usize,
    _newtls: usize,
) -> usize {
    const CLONE_VM: usize = 0x00000100;
    const CLONE_PARENT_SETTID: usize = 0x00100000;
    const CLONE_CHILD_CLEARTID: usize = 0x00200000;

    let (user_rsp, user_rip, _user_rflags) = syscall::current_user_context();
    let is_thread = (flags & CLONE_VM) != 0;
    let new_rsp = if is_thread && child_stack != 0 {
        child_stack
    } else {
        user_rsp as usize
    };

    match task::scheduler::fork_current_user_task(user_rip, new_rsp as u64) {
        Some(pid) => {
            if (flags & CLONE_PARENT_SETTID) != 0 && ptid != 0 {
                if let Err(err) = super::write_user(ptid, pid) {
                    return err;
                }
            }
            if (flags & CLONE_CHILD_CLEARTID) != 0 && ctid != 0 {
                let _ = ctid;
            }
            pid
        }
        None => super::errno(super::ENOSYS),
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
    task::scheduler::current_task_id()
}

pub(super) fn sys_setpgid(pid: usize, pgid: usize) -> usize {
    let _ = (pid, pgid);
    0
}

pub(super) fn sys_getpgid(pid: usize) -> usize {
    if pid == 0 {
        task::scheduler::current_task_id()
    } else {
        pid
    }
}

pub(super) fn sys_getsid(pid: usize) -> usize {
    if pid == 0 {
        task::scheduler::current_task_id()
    } else {
        pid
    }
}

pub(super) fn sys_exit(code: usize) -> usize {
    task::scheduler::exit(code as i32);
}
