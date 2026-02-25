//! # echOS Task Modülü
//!
//! Preemptive multitasking altyapısı.
//! Task yapısı, scheduler ve context switch.

/// Task yapısı ve context
pub mod task;

/// Priority-based aging scheduler
pub mod scheduler;

/// Real-Time Scheduler (SCHED_FIFO/RR)
pub mod rt_scheduler;

/// User mode task desteği
pub mod user;

/// Asenkron Worker Havuzu (io_uring)
pub mod worker;

/// Zaman Çarkı (Timing Wheel) - Yüksek performanslı timer yönetimi
pub mod timer;

/// Chase-Lev Lock-Free Deque
pub mod deque;

/// Google GhOSt (Global Herbert Operating System Threading) Benzeri Userspace Scheduling
pub mod ghost;

/// Signal Handling and Job Control
pub mod signal;

/// Futex and pthread support
pub mod futex;

pub use scheduler::{
    exit, get_ticks, init as init_scheduler, schedule, sleep, spawn, spawn_with_priority,
    list_tasks, kill_task, background_current, foreground_task, get_task_state, get_cpu_count,
    TaskInfo,
};
pub use task::Priority;
pub use task::TaskState;
pub use signal::{Signal, SignalHandlers, JobManager, Job, JobState, JOB_MANAGER,
    SigAction, SigInfo, SignalAction, SignalDisposition, SignalError,
    SIG_DFL, SIG_IGN, SA_NOCLDSTOP, SA_NOCLDWAIT, SA_SIGINFO, SA_RESTART, 
    SA_NODEFER, SA_RESETHAND, SA_ONSTACK,
    SIG_BLOCK, SIG_UNBLOCK, SIG_SETMASK,
    sys_sigaction, sys_sigprocmask, sys_sigpending, sys_sigsuspend, sys_kill,
    sys_raise, sys_pause, sys_alarm, deliver_signals, has_pending_signals,
    send_signal, send_signal_pgroup, send_signal_all, generate_terminal_signal,
};
pub use rt_scheduler::{
    SchedPolicy, RtSchedParam, RtRunQueue, RtTaskInfo,
    RT_PRIO_MIN, RT_PRIO_MAX, RR_DEFAULT_TIMESLICE,
    has_rt_tasks, rt_task_count, pick_next_rt_task,
    set_sched_param, get_sched_param, is_rt_task,
    get_task_priority, get_task_policy,
};
pub use futex::{
    FUTEX_WAIT, FUTEX_WAKE, FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
    FUTEX_REQUEUE, FUTEX_CMP_REQUEUE, FUTEX_LOCK_PI, FUTEX_UNLOCK_PI,
    FUTEX_PRIVATE_FLAG, FUTEX_CLOCK_REALTIME,
    CLONE_VM, CLONE_FS, CLONE_FILES, CLONE_SIGHAND, CLONE_THREAD,
    CLONE_VFORK, CLONE_SETTLS, CLONE_PARENT_SETTID, CLONE_CHILD_CLEARTID,
    sys_futex, sys_clone, sys_set_robust_list, sys_get_robust_list, sys_set_tid_address,
    FutexStats, get_stats as get_futex_stats, wake_all_at_address,
};
