//! # echOS Görev Modülü
//!
//! Preemptive (önleyici) çoklu görev altyapısı.
//! Görev yapısı, zamanlayıcı ve bağlam değişimi (context switch) içerir.
//!
//! ## Zamanlayıcı Seçim Hiyerarşisi
//!
//! ```text
//!  RT Görevler (öncelik 1-99)         Normal Görevler (nice -20..+19)
//!  ┌──────────────────────┐           ┌──────────────────────────────┐
//!  │  rt_scheduler.rs     │           │  scheduler.rs (CFS benzeri)  │
//!  │  SCHED_FIFO          │  >önce>   │  vruntime bazlı seçim        │
//!  │  SCHED_RR            │           │  Work-stealing deque         │
//!  └──────────────────────┘           └──────────────────────────────┘
//!        ▲                                       ▲
//!        │ yüksek öncelik önce çalışır           │ en düşük vruntime önce
//!        └───────────────────────────────────────┘
//! ```

/// Görev yapısı ve bağlam (context)
pub mod task;

/// Öncelik + yaşlandırma (aging) tabanlı zamanlayıcı
pub mod scheduler;

pub mod eas;
pub mod eevdf;
/// Gerçek Zamanlı Zamanlayıcı (SCHED_FIFO / SCHED_RR)
pub mod rt_scheduler;

/// Kullanıcı modu görev desteği (Ring3)
pub mod user;

/// Asenkron İşçi Havuzu (io_uring benzeri)
pub mod worker;

/// Zaman Çarkı (Timing Wheel) — Yüksek performanslı O(1) zamanlayıcı yönetimi
pub mod timer;

/// Chase-Lev Kilit-Serbest (Lock-Free) Çift Uçlu Kuyruk — İş Çalma (Work Stealing)
pub mod deque;

/// Google GhOSt Benzeri Kullanıcı Alanı Zamanlayıcısı
pub mod ghost;

/// Sinyal İşleme ve İş Kontrolü (Job Control)
pub mod signal;

/// Futex ve pthread desteği (hızlı kullanıcı alanı mutex)
pub mod futex;
pub mod rseq;

/// Cgroups v2 — kaynak kontrol grubu yönetimi
pub mod cgroup_v2;

pub use futex::{
    get_stats as get_futex_stats, sys_clone, sys_futex, sys_futex_waitv, sys_get_robust_list,
    sys_set_robust_list, sys_set_tid_address, wait_on_address, wake_all_at_address,
    wake_by_address_all, wake_by_address_single, FutexStats, CLONE_CHILD_CLEARTID, CLONE_FILES,
    CLONE_FS, CLONE_PARENT_SETTID, CLONE_SETTLS, CLONE_SIGHAND, CLONE_THREAD, CLONE_VFORK,
    CLONE_VM, FUTEX_CLOCK_REALTIME, FUTEX_CMP_REQUEUE, FUTEX_LOCK_PI, FUTEX_PRIVATE_FLAG,
    FUTEX_REQUEUE, FUTEX_UNLOCK_PI, FUTEX_WAIT, FUTEX_WAIT_BITSET, FUTEX_WAKE, FUTEX_WAKE_BITSET,
};
pub use rseq::{sys_rseq, RseqUserArea};
pub use rt_scheduler::{
    get_sched_param, get_task_policy, get_task_priority, has_rt_tasks, is_rt_task,
    pick_next_rt_task, rt_task_count, set_sched_param, RtRunQueue, RtSchedParam, RtTaskInfo,
    SchedPolicy, RR_DEFAULT_TIMESLICE, RT_PRIO_MAX, RT_PRIO_MIN,
};
pub use scheduler::{
    background_current, exit, foreground_task, get_cpu_count, get_task_state, get_ticks,
    init as init_scheduler, kill_task, list_tasks, schedule, sleep, spawn, spawn_with_priority,
    TaskInfo,
};
pub use signal::{
    deliver_signals, generate_terminal_signal, has_pending_signals, send_signal, send_signal_all,
    send_signal_pgroup, sys_alarm, sys_kill, sys_pause, sys_raise, sys_sigaction, sys_sigpending,
    sys_sigprocmask, sys_sigsuspend, Job, JobManager, JobState, SigAction, SigInfo, Signal,
    SignalAction, SignalDisposition, SignalError, SignalHandlers, JOB_MANAGER, SA_NOCLDSTOP,
    SA_NOCLDWAIT, SA_NODEFER, SA_ONSTACK, SA_RESETHAND, SA_RESTART, SA_SIGINFO, SIG_BLOCK, SIG_DFL,
    SIG_IGN, SIG_SETMASK, SIG_UNBLOCK,
};
pub use task::Priority;
pub use task::TaskState;
