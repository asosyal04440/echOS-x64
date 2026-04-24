//! # echOS Görev Zamanlayıcı (Task Scheduler)
//!
//! Bu modül, işletim sisteminin preemptive multitasking desteğini sağlar.
//! Öncelik tabanlı yaşlandırma (Priority-Based Aging) ve
//! Chase-Lev iş çalma (Work Stealing) algoritmasıyla görevleri adil biçimde zamanlar.
//!
//! ## Zamanlayıcı Seçim Mantığı
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────┐
//!  │          ZAMANLAYICI SEÇİM KARAR AKIŞI                  │
//!  │                                                          │
//!  │  Timer Interrupt geldi (tick)                           │
//!  │       ↓                                                  │
//!  │  1. RT görevi var mı? (rt_scheduler)                   │
//!  │     Evet → en yüksek öncelikli RT görevi çalışır       │
//!  │     (SCHED_FIFO: bloke edilene kadar, SCHED_RR: dilime) │
//!  │       ↓ Hayır                                           │
//!  │  2. Yerel Worker kuyruğunda görev var mı?              │
//!  │     Evet → pop() ile al (LIFO — önbellek dostu)        │
//!  │       ↓ Hayır                                           │
//!  │  3. En yüklü CPU'dan iş çal (Work Stealing)            │
//!  │     steal() → başka CPU'nun kuyruğunun başından al     │
//!  │       ↓ Yoksa                                           │
//!  │  4. Boşta görevi çalıştır (idle loop: hlt)             │
//!  └──────────────────────────────────────────────────────────┘
//! ```
//!
//! ## SMP İş Çalma (Work Stealing) Diyagramı
//!
//! ```text
//!  CPU0 [W0:T1,T2,T3]   CPU1 [W1: ]   CPU2 [W2:T4,T5]
//!        pop↑                steal→          pop↑
//!        T3 çalışır      T1 çalınır     T5 çalışır
//! ```
//!
//! ## Bağlam Değişimi (Context Switch)
//!
//! ```text
//!  Eski Görev             Yeni Görev
//!  ──────────              ──────────
//!  R15,R14..RBP  →save→  RSP, RFLAGS
//!  RSP, RFLAGS   ←load←  R15,R14..RBP
//!  SSE/FPU durumu        SSE/FPU durumu
//!  CR3 (sayfa tablosu)   CR3 (sayfa tablosu)
//! ```

use super::deque::{Stealer, Worker};
use super::task::{
    ExecutionMode, Priority, Task, TaskContext, TaskId, TaskState, Win32ThreadState,
};
use crate::cpu::{cpu_slots, epoch::SMP_EPOCH_DOMAIN};
use crate::memory_barriers::{smp_mb, smp_wmb};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{OffsetPageTable, PhysFrame};
use x86_64::VirtAddr;

const MAX_CPUS: usize = 8192;
const MSR_GS_BASE: u32 = 0xC000_0101;

static mut WORKERS: Vec<Option<Worker<Task>>> = Vec::new();
static mut STEALERS: Vec<Option<Stealer<Task>>> = Vec::new();

// Global görev ID sayacı
static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(1);

// ============================================================================
// SMP-AWARE SCHEDULER YAPISI (CHASE-LEV LOCK-FREE WORK STEALING)
// ============================================================================

/// Global SMP scheduler yapısı (Legacy wrapper)
pub struct SmpScheduler {
    cpu_count: AtomicU32,
}

impl SmpScheduler {
    pub fn new(cpu_count: u32) -> Self {
        Self {
            cpu_count: AtomicU32::new(cpu_count),
        }
    }

    pub fn allocate_task_id(&self) -> TaskId {
        NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn spawn(&self, task: Task) {
        let task = Box::new(task);
        let target_cpu = choose_spawn_cpu(&task);
        if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
            publish_worker_load(actual_cpu);
        } else {
            crate::serial_println!("ERROR: No workers available to spawn task!");
        }
    }

    // Zaten box'lanmış görevler için dahili yardımcı (örn. timer'dan gelen görevler)
    pub fn spawn_boxed(&self, task: Box<Task>) {
        let target_cpu = choose_spawn_cpu(&task);
        if let Some(actual_cpu) = enqueue_boxed_task(target_cpu, task) {
            publish_worker_load(actual_cpu);
        } else {
            crate::serial_println!("ERROR: No workers available to spawn task!");
        }
    }
}

fn task_can_run_on_cpu(task: &Task, cpu_id: u32) -> bool {
    task.hot.affinity == 0xFFFF_FFFF || (cpu_id < 32 && (task.hot.affinity & (1u32 << cpu_id)) != 0)
}

fn queued_task_count_usize(cpu_id: usize) -> u32 {
    unsafe {
        WORKERS
            .get(cpu_id)
            .and_then(|w| w.as_ref())
            .map(|worker| worker.len() as u32)
            .unwrap_or(0)
    }
}

pub fn queued_task_count(cpu_id: u32) -> u32 {
    queued_task_count_usize(cpu_id as usize)
}

fn publish_worker_load(cpu_id: usize) {
    crate::cpu::smp::update_cpu_load(cpu_id as u32, queued_task_count_usize(cpu_id));
}

fn choose_spawn_cpu(task: &Task) -> usize {
    let current_cpu = get_current_cpu_id() as usize;
    let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
    let mut best_cpu = current_cpu.min(cpu_limit.saturating_sub(1));
    let mut best_load = queued_task_count_usize(best_cpu);

    for cpu in 0..cpu_limit {
        let cpu_id = cpu as u32;
        if !cpu_slots::is_online(cpu_id) || !task_can_run_on_cpu(task, cpu_id) {
            continue;
        }
        unsafe {
            if WORKERS.get(cpu).and_then(|w| w.as_ref()).is_none() {
                continue;
            }
        }

        let load = queued_task_count_usize(cpu);
        if load < best_load || (load == best_load && cpu == current_cpu) {
            best_cpu = cpu;
            best_load = load;
        }
    }

    best_cpu
}

fn enqueue_boxed_task(target_cpu: usize, task: Box<Task>) -> Option<usize> {
    unsafe {
        if let Some(worker) = WORKERS.get(target_cpu).and_then(|w| w.as_ref()) {
            worker.push(task);
            Some(target_cpu)
        } else if let Some(worker) = WORKERS.get(0).and_then(|w| w.as_ref()) {
            worker.push(task);
            Some(0)
        } else {
            None
        }
    }
}

// ============================================================================
// GLOBAL DURUM
// ============================================================================

use super::timer::TimingWheel;

lazy_static! {
    /// Global SMP Scheduler instance (Lock-Free)
    static ref SMP_SCHEDULER: SmpScheduler = SmpScheduler::new(1);
    /// Uyuyan task'ların listesi (wake_tick ile birlikte) — SMP-safe Mutex ile korunuyor
    /// ARTIK ZAMAN ÇARKI (TIMING WHEEL) KULLANIYORUZ! (O(1) Karmaşıklık)
    static ref SLEEPING_TASKS: Mutex<TimingWheel> = Mutex::new(TimingWheel::new(256));
}

/// CPU başına mevcut görev durumu (CPU ID -> Görev)
pub static mut PER_CPU_CURRENT_TASK: Vec<Option<Box<Task>>> = Vec::new();

static TERMINATED_TASKS: Mutex<Vec<(TaskId, i32)>> = Mutex::new(Vec::new());
static ZOMBIE_TASKS: Mutex<Vec<Box<Task>>> = Mutex::new(Vec::new());

/// Per-CPU idle task'ları
static mut PER_CPU_IDLE_TASK: Vec<Box<Task>> = Vec::new();
static mut PER_CPU_DUMMY_CONTEXT: Vec<TaskContext> = Vec::new();

/// Sistem başlangıcından beri geçen tick sayısı
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);
static SECONDARY_SCHEDULING_ACTIVE: AtomicBool = AtomicBool::new(false);

const NICE_0_LOAD: u64 = 1024;
const SCHED_LATENCY_TICKS: u64 = 20;
const MIN_GRANULARITY_TICKS: u64 = 4;
const LOAD_BALANCE_INTERVAL: usize = 100;
const VRUNTIME_NORMALIZE_INTERVAL: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchedulerPressureClass {
    Normal,
    Elevated,
    Critical,
}

#[derive(Clone, Copy, Debug)]
struct SchedulerPressureSnapshot {
    class: SchedulerPressureClass,
    memory_some_avg10: u64,
    memory_full_avg10: u64,
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Scheduler'ı başlatır.
pub fn init() {
    // Zaten başlatılmış mı kontrol et (örn. smp::init tarafından update_cpu_count çağrıldıysa)
    unsafe {
        if !PER_CPU_IDLE_TASK.is_empty() {
            crate::serial_println!("SMP Scheduler already initialized, skipping");
            return;
        }
    }

    // CPU sayısını al (başlangıçta 1, SMP başlatılınca güncellenecek)
    let cpu_count = crate::cpu::CPU_INFO
        .lock()
        .topology
        .logical_count
        .max(1)
        .min(MAX_CPUS as u32);

    // CPU başına veri yapılarını başlat
    unsafe {
        PER_CPU_CURRENT_TASK = Vec::with_capacity(cpu_count as usize);
        PER_CPU_IDLE_TASK = Vec::with_capacity(cpu_count as usize);
        PER_CPU_DUMMY_CONTEXT = Vec::with_capacity(cpu_count as usize);
        WORKERS = Vec::with_capacity(cpu_count as usize);
        STEALERS = Vec::with_capacity(cpu_count as usize);

        for cpu_id in 0..cpu_count {
            PER_CPU_CURRENT_TASK.push(None);
            PER_CPU_IDLE_TASK.push(Box::new(Task::idle_with_cpu(cpu_id)));
            PER_CPU_DUMMY_CONTEXT.push(TaskContext::new(0, 0));

            let (w, s) = Worker::new();
            WORKERS.push(Some(w));
            STEALERS.push(Some(s));
        }
    }

    // SMP scheduler'ı güncelle
    SMP_SCHEDULER.cpu_count.store(cpu_count, Ordering::Relaxed);

    crate::serial_println!(
        "SMP Scheduler initialized for {} CPUs (Chase-Lev)",
        cpu_count
    );
    crate::random::init(get_ticks() as u32 + 0xDEADBEEF);
    SECONDARY_SCHEDULING_ACTIVE.store(false, Ordering::Release);
    SCHEDULER_READY.store(true, Ordering::Release);
}

/// Belirli bir CPU'nun yük istatistiklerini döndür
pub fn get_cpu_load(cpu_id: u32) -> f32 {
    unsafe {
        if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
            // Worker kuyruğundaki görev sayısına göre yük tahmini
            let queue_len = worker.len() as f32;
            // Normalize: 0-10 arası kuyruk → 0-100%
            (queue_len / 10.0 * 100.0).min(100.0)
        } else {
            0.0
        }
    }
}

/// SMP için CPU sayısını güncelle
pub fn update_cpu_count(cpu_count: u32) {
    let cpu_count = cpu_count.min(MAX_CPUS as u32);
    SMP_SCHEDULER.cpu_count.store(cpu_count, Ordering::Relaxed);

    unsafe {
        if PER_CPU_CURRENT_TASK.len() < cpu_count as usize {
            for cpu_id in PER_CPU_CURRENT_TASK.len() as u32..cpu_count {
                PER_CPU_CURRENT_TASK.push(None);
                PER_CPU_IDLE_TASK.push(Box::new(Task::idle_with_cpu(cpu_id)));
                PER_CPU_DUMMY_CONTEXT.push(TaskContext::new(0, 0));

                let (w, s) = Worker::new();
                WORKERS.push(Some(w));
                STEALERS.push(Some(s));
            }
        }
    }

    crate::serial_println!("Scheduler updated for {} CPUs", cpu_count);
    SCHEDULER_READY.store(true, Ordering::Release);
}

pub fn enable_secondary_scheduling() {
    SECONDARY_SCHEDULING_ACTIVE.store(true, Ordering::Release);
}

pub fn secondary_scheduling_active() -> bool {
    SECONDARY_SCHEDULING_ACTIVE.load(Ordering::Acquire)
}

pub fn current_kernel_stack_top() -> u64 {
    let cpu_id = get_current_cpu_id();
    unsafe {
        if let Some(task) = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
        {
            task.kernel_stack_top
        } else {
            PER_CPU_IDLE_TASK
                .get(cpu_id as usize)
                .map(|t| t.kernel_stack_top)
                .unwrap_or(0)
        }
    }
}

pub fn classify_current_kernel_stack_fault(addr: u64) -> Option<&'static str> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        let task = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())?;
        if addr >= task.hot.kernel_stack_guard_base && addr < task.hot.kernel_stack_bottom {
            Some("KERNEL_STACK_GUARD_PAGE")
        } else {
            None
        }
    }
}

pub fn record_current_stack_pointer(rsp: u64) {
    let cpu_id = get_current_cpu_id();
    unsafe {
        if let Some(task) = PER_CPU_CURRENT_TASK
            .get_mut(cpu_id as usize)
            .and_then(|slot| slot.as_mut())
        {
            if rsp >= task.hot.kernel_stack_bottom
                && rsp <= task.hot.kernel_stack_top
                && rsp < task.hot.kernel_stack_low_watermark
            {
                task.hot.kernel_stack_low_watermark = rsp;
            }
        }
    }
}

pub fn current_kernel_stack_usage() -> Option<(u64, u64)> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        let task = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())?;
        let capacity = task
            .hot
            .kernel_stack_top
            .saturating_sub(task.hot.kernel_stack_bottom);
        let used = task
            .hot
            .kernel_stack_top
            .saturating_sub(task.hot.kernel_stack_low_watermark);
        Some((used, capacity))
    }
}

pub fn current_user_target() -> Option<(u64, u64)> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .and_then(
                |task| match (task.cold.user_entry, task.cold.user_stack_top) {
                    (Some(entry), Some(stack)) => Some((entry, stack)),
                    _ => None,
                },
            )
    }
}

pub fn current_win32_thread_state() -> Option<Win32ThreadState> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .and_then(|task| task.cold.win32)
    }
}

#[inline(always)]
unsafe fn read_user_gs_base() -> u64 {
    Msr::new(MSR_GS_BASE).read()
}

#[inline(always)]
unsafe fn write_user_gs_base(base: u64) {
    Msr::new(MSR_GS_BASE).write(base);
}

pub fn current_task_id() -> TaskId {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .map(|task| task.id)
            .unwrap_or(0)
    }
}

pub fn current_execution_mode() -> Option<ExecutionMode> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .map(|task| task.cold.mode)
    }
}

pub fn current_user_page_table() -> Option<PhysFrame> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .and_then(|task| task.cold.page_table)
    }
}

pub fn current_address_space() -> Option<Arc<Mutex<crate::memory::AddressSpace>>> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .and_then(|task| task.cold.address_space.clone())
    }
}

pub fn task_exists(pid: TaskId) -> bool {
    if pid == 0 {
        return false;
    }
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if PER_CPU_CURRENT_TASK
            .iter()
            .any(|slot| slot.as_ref().map(|task| task.id == pid).unwrap_or(false))
        {
            return true;
        }
        if ZOMBIE_TASKS.lock().iter().any(|task| task.id == pid) {
            return true;
        }
        TERMINATED_TASKS.lock().iter().any(|(id, _)| *id == pid)
    })
}

pub fn fork_current_user_task(user_rip: u64, user_rsp: u64) -> Option<TaskId> {
    if !crate::memory::is_user_address(user_rip) || !crate::memory::is_user_address(user_rsp) {
        return None;
    }
    let cpu_id = get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let current = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())?;
        if current.cold.mode != ExecutionMode::LegacyRing3 {
            return None;
        }
        let priority = current.priority;
        let name = current.cold.name;
        let affinity = current.affinity;
        let address_space = current.cold.address_space.clone()?;
        let cloned_space = crate::memory::clone_address_space_for_cow(&address_space)?;
        let child_pml4 = crate::memory::clone_user_pml4_for_cow()?;
        let child_id = SMP_SCHEDULER.allocate_task_id();
        let mut child = Task::with_priority_and_id(
            crate::task::user::fork_child_start,
            priority,
            name,
            child_id,
        );
        child.cold.mode = ExecutionMode::LegacyRing3;
        child.cold.page_table = Some(child_pml4);
        child.cold.address_space = Some(cloned_space);
        child.cold.user_entry = Some(user_rip);
        child.cold.user_stack_top = Some(user_rsp);
        child.affinity = affinity;
        SMP_SCHEDULER.spawn(child);
        Some(child_id)
    })
}

pub fn idle_loop() -> ! {
    loop {
        let cpu_id = get_current_cpu_id();
        if cpu_id != 0 && !secondary_scheduling_active() {
            x86_64::instructions::interrupts::enable();
            core::hint::spin_loop();
            x86_64::instructions::hlt();
            continue;
        }
        if SCHEDULER_READY.load(Ordering::Acquire) && has_schedulable_work(get_current_cpu_id()) {
            schedule();
            continue;
        }
        x86_64::instructions::interrupts::enable();
        core::hint::spin_loop();
        x86_64::instructions::hlt();
    }
}

/// Normal öncelikle yeni bir task oluşturur ve kuyruğa ekler.
pub fn spawn(entry_point: fn() -> !) -> TaskId {
    spawn_with_priority(entry_point, Priority::Normal, "unnamed")
}

/// Belirtilen öncelikle yeni bir task oluşturur.
pub fn spawn_with_priority(
    entry_point: fn() -> !,
    priority: Priority,
    name: &'static str,
) -> TaskId {
    if name == "gpu_test" {
        crate::serial_println!("DEBUG: Task struct oluşturuluyor...");
    }
    let task_id = SMP_SCHEDULER.allocate_task_id();
    let task = Task::with_priority_and_id(entry_point, priority, name, task_id);

    x86_64::instructions::interrupts::without_interrupts(|| {
        SMP_SCHEDULER.spawn(task);
    });
    let _ = crate::task::ghost::publish_event(crate::task::ghost::MSG_TASK_NEW, task_id as u64, 0);
    if name == "gpu_test" {
        crate::serial_println!("DEBUG: Task kuyruğa eklendi! PID: {}", task_id);
    }

    task_id
}

pub fn spawn_with_priority_in_address_space(
    entry_point: fn() -> !,
    priority: Priority,
    name: &'static str,
    address_space: Option<Arc<Mutex<crate::memory::AddressSpace>>>,
) -> TaskId {
    if name == "gpu_test" {
        crate::serial_println!("DEBUG: Task struct oluÅŸturuluyor...");
    }
    let task_id = SMP_SCHEDULER.allocate_task_id();
    let mut task = Task::with_priority_and_id(entry_point, priority, name, task_id);
    task.cold.address_space = address_space;

    x86_64::instructions::interrupts::without_interrupts(|| {
        SMP_SCHEDULER.spawn(task);
    });
    let _ = crate::task::ghost::publish_event(crate::task::ghost::MSG_TASK_NEW, task_id as u64, 0);
    if name == "gpu_test" {
        crate::serial_println!("DEBUG: Task kuyruÄŸa eklendi! PID: {}", task_id);
    }

    task_id
}

/// Sistem tick sayısını döndürür.
pub fn get_ticks() -> usize {
    TICK_COUNT.load(Ordering::Relaxed)
}

pub fn is_ready() -> bool {
    SCHEDULER_READY.load(Ordering::Acquire)
}

/// Timer interrupt'tan çağrılır. Her tick'te:
/// 1. Tick sayacını artırır
/// 2. Uyuyan task'ları kontrol eder
/// 3. Time slice dolmuşsa schedule() çağırır
pub fn tick() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        return;
    }
    let ticks = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::task::signal::process_alarms(ticks + 1);
    wake_sleeping_tasks(ticks + 1);
    crate::task::futex::check_timeouts();

    // update_utilization ve normalize_vruntime devre dışı (basitleştirildi)

    if should_preempt(ticks as u64) {
        // Doğrudan schedule() çağırmak yerine need_resched bayrağını kur.
        // Bu sayede preempt kontrolü schedule() girişinde yapılır.
        crate::preempt::set_need_resched();
    }

    // Eğer need_resched bayrağı ve preemption etkinse, zamanlayıcıyı çağır
    if crate::preempt::need_resched() && crate::preempt::preemptible() {
        schedule();
    }

    // Load Update — Simics: 1 saniyede bir, bare-metal: 100ms'de bir
    #[cfg(feature = "simics")]
    const LOAD_UPDATE_INTERVAL: usize = 100;
    #[cfg(not(feature = "simics"))]
    const LOAD_UPDATE_INTERVAL: usize = 10;
    if ticks % LOAD_UPDATE_INTERVAL == 0 {
        let cpu_id = get_current_cpu_id();
        let load = unsafe {
            if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
                worker.len() as u32
            } else {
                0
            }
        };
        crate::cpu::smp::update_cpu_load(cpu_id, load);
    }

    // Load Balance Report — Simics: ~100 saniyede bir, bare-metal: ~10 saniyede bir
    #[cfg(feature = "simics")]
    const BALANCE_INTERVAL: usize = 10000;
    #[cfg(not(feature = "simics"))]
    const BALANCE_INTERVAL: usize = 1000;
    if ticks % BALANCE_INTERVAL == 0 {
        crate::cpu::smp::balance_load();
    }
}

fn should_preempt(now: u64) -> bool {
    let cpu_id = get_current_cpu_id();
    unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
        {
            let slice = calc_time_slice(cpu_id, current.weight);
            return now.saturating_sub(current.last_start) >= slice;
        }
    }
    false
}

fn scheduler_pressure_snapshot() -> SchedulerPressureSnapshot {
    let psi = crate::memory::psi::snapshot();
    let class = if psi.full_avg10 >= 350 || psi.some_avg10 >= 700 {
        SchedulerPressureClass::Critical
    } else if psi.full_avg10 >= 150 || psi.some_avg10 >= 400 {
        SchedulerPressureClass::Elevated
    } else {
        SchedulerPressureClass::Normal
    };
    SchedulerPressureSnapshot {
        class,
        memory_some_avg10: psi.some_avg10,
        memory_full_avg10: psi.full_avg10,
    }
}

fn calc_time_slice(_cpu_id: u32, weight: u32) -> u64 {
    // Slice tabanını weight ile başlat, sonra PSI basıncıyla ayarla.
    // Chase-Lev'de total weight hesaplamak zor (global/local kuyruklar)
    // Şimdilik basitçe weight * sabit veriyoruz.
    let base_slice = SCHED_LATENCY_TICKS;
    let mut slice = base_slice * (weight as u64) / 1024;
    let pressure = scheduler_pressure_snapshot();
    match pressure.class {
        SchedulerPressureClass::Normal => {}
        SchedulerPressureClass::Elevated => {
            if weight < 1024 {
                slice = slice.saturating_mul(3) / 4;
            } else {
                slice = slice.saturating_add(1);
            }
        }
        SchedulerPressureClass::Critical => {
            if weight < 1024 {
                slice = slice.saturating_mul(1) / 2;
            } else {
                slice = slice.saturating_mul(5) / 4;
            }
        }
    }
    slice.max(MIN_GRANULARITY_TICKS)
}

fn update_task_vruntime(task: &mut Task, delta_ticks: u64) {
    let weight = task.weight.max(1) as u64;
    let scaled = delta_ticks.saturating_mul(NICE_0_LOAD) / weight;
    task.vruntime = task.vruntime.saturating_add(scaled);
}

/// Uyuyan task'ları kontrol eder, zamanı gelenleri uyandırır.
/// ZAMAN ÇARKI (TIMING WHEEL) KULLANILIR (O(1))
fn wake_sleeping_tasks(_current_tick: usize) {
    // Timing Wheel'ı bir tık ilerlet ve uyananları al
    let tasks = SLEEPING_TASKS.lock().tick();

    // Uyanan task'ları tekrar scheduler'a ekle
    for mut task in tasks {
        let task_id = task.id as u64;
        task.state = TaskState::Ready;
        let _ = crate::task::ghost::publish_event(crate::task::ghost::MSG_TASK_WAKEUP, task_id, 0);
        // spawn_boxed kullan
        SMP_SCHEDULER.spawn_boxed(task);
    }
}

/// Mevcut task'ı belirtilen tick sayısı kadar uyutur.
pub fn sleep(ticks: usize) {
    let cpu_id = get_current_cpu_id();
    let wake_tick = TICK_COUNT.load(Ordering::Relaxed) + ticks;
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK[cpu_id as usize].as_mut() {
            let delta = now.saturating_sub(current.last_start);
            update_task_vruntime(current, delta);
            current.state = TaskState::Sleeping { wake_tick };
        } else {
            crate::serial_println!("WARNING: Idle task attempted to sleep!");
        }
    });

    schedule();
}

/// Mevcut task'ı sonlandırır ve bir daha çalışmaz.
pub fn exit(code: i32) -> ! {
    let cpu_id = get_current_cpu_id();
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK[cpu_id as usize].as_mut() {
            let delta = now.saturating_sub(current.last_start);
            update_task_vruntime(current, delta);
            current.state = TaskState::Terminated;
            current.cold.exit_code = Some(code);
            crate::serial_println!("Task {} '{}' terminated", current.id, current.cold.name);
            // SIGCHLD — üst sürece bildir
            if let Some(parent) = current.cold.parent_pid {
                crate::task::signal::send_signal(parent, crate::task::signal::Signal::SIGCHLD);
            }
        } else {
            crate::serial_println!("ERROR: Idle task attempted to exit!");
        }
    });

    schedule();

    // Bu noktaya asla ulaşılmamalı
    loop {
        x86_64::instructions::hlt();
    }
}

pub fn wait_for_terminated(pid: isize) -> Option<(TaskId, i32)> {
    let mut terminated = TERMINATED_TASKS.lock();
    if pid == -1 {
        if terminated.is_empty() {
            return None;
        }
        return Some(terminated.remove(0));
    }
    if pid <= 0 {
        return None;
    }
    let target = pid as TaskId;
    if let Some(pos) = terminated.iter().position(|(id, _)| *id == target) {
        return Some(terminated.remove(pos));
    }
    None
}

// ============================================================================
// PTRACE (Syscall Tracing) KONTROL ARAYÜZÜ
// ============================================================================
pub fn get_current_ptrace_flags() -> u32 {
    let cpu_id = get_current_cpu_id();
    let mut flags = 0;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
        {
            flags = current.cold.ptrace_flags;
        }
    });
    flags
}

pub fn set_ptrace_flag(flag: u32) {
    let cpu_id = get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize].take() {
            current.cold.ptrace_flags |= flag;
            PER_CPU_CURRENT_TASK[cpu_id as usize] = Some(current);
        }
    });
}

// ============================================================================
// SECCOMP (Secure Computing) KONTROL ARAYÜZÜ
// ============================================================================
pub fn get_current_seccomp_mode() -> u32 {
    let cpu_id = get_current_cpu_id();
    let mut mode = 0;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
        {
            mode = current.cold.seccomp_mode;
        }
    });
    mode
}

pub fn set_current_seccomp_mode(mode: u32) {
    let cpu_id = get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize].take() {
            current.cold.seccomp_mode = mode;
            PER_CPU_CURRENT_TASK[cpu_id as usize] = Some(current);
        }
    });
}

pub fn exec_current_user_image(image: &[u8]) -> Result<(), ()> {
    let address_space = crate::memory::create_address_space(image);
    crate::memory::set_active_address_space(Some(address_space.clone()));
    let user_pml4 = crate::memory::create_user_pml4().ok_or(())?;
    let pml4_phys = user_pml4.start_address().as_u64();
    let phys_offset = crate::memory::active_physical_offset();
    let pml4_virt = VirtAddr::new(phys_offset + pml4_phys);
    let table = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(phys_offset)) };
    let frame_allocator = unsafe { crate::memory::global_memory_manager_mut().ok_or(())? };

    // vDSO sayfasını map et (user read-only)
    if let Err(_) = crate::vdso::map_to_user(&mut mapper) {
        crate::serial_println!("[vDSO] Failed to map vDSO to user space!");
    }

    let user = crate::elf::load_user_elf(image, &mut mapper, frame_allocator).map_err(|_| ())?;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let cpu_id = get_current_cpu_id();
        if let Some(current) = PER_CPU_CURRENT_TASK
            .get_mut(cpu_id as usize)
            .and_then(|t| t.as_mut())
        {
            current.cold.mode = ExecutionMode::LegacyRing3;
            current.cold.page_table = Some(user_pml4);
            current.cold.address_space = Some(address_space.clone());
            current.cold.user_entry = Some(user.entry.as_u64());
            current.cold.user_stack_top = Some(user.stack_top.as_u64());
        }
    });
    unsafe {
        Cr3::write(user_pml4, Cr3Flags::empty());
    }
    unsafe { crate::task::user::enter_user_mode(user.entry, user.stack_top) }
}

pub fn spawn_user_image_task(
    image: &[u8],
    priority: Priority,
    name: &'static str,
) -> Result<TaskId, ()> {
    spawn_user_image_task_with_address_space(image, priority, name).map(|(task_id, _)| task_id)
}

pub fn spawn_user_image_task_with_address_space(
    image: &[u8],
    priority: Priority,
    name: &'static str,
) -> Result<(TaskId, Arc<Mutex<crate::memory::AddressSpace>>), ()> {
    let address_space = crate::memory::create_address_space(image);
    crate::memory::set_active_address_space(Some(address_space.clone()));
    let user_pml4 = match crate::memory::create_user_pml4() {
        Some(frame) => frame,
        None => {
            crate::memory::set_active_address_space(None);
            return Err(());
        }
    };
    let pml4_phys = user_pml4.start_address().as_u64();
    let phys_offset = crate::memory::active_physical_offset();
    let pml4_virt = VirtAddr::new(phys_offset + pml4_phys);
    let table = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
    let mut mapper = unsafe { OffsetPageTable::new(table, VirtAddr::new(phys_offset)) };
    let frame_allocator = match unsafe { crate::memory::global_memory_manager_mut() } {
        Some(allocator) => allocator,
        None => {
            crate::memory::set_active_address_space(None);
            return Err(());
        }
    };

    if crate::vdso::map_to_user(&mut mapper).is_err() {
        crate::serial_println!("[vDSO] Failed to map vDSO to spawned user task");
    }

    let user = match crate::elf::load_user_elf(image, &mut mapper, frame_allocator) {
        Ok(user) => user,
        Err(_) => {
            crate::memory::set_active_address_space(None);
            return Err(());
        }
    };
    crate::memory::set_active_address_space(None);

    let task_id = SMP_SCHEDULER.allocate_task_id();
    let mut task =
        Task::with_priority_and_id(crate::task::user::fork_child_start, priority, name, task_id);
    task.cold.mode = ExecutionMode::LegacyRing3;
    task.cold.page_table = Some(user_pml4);
    task.cold.address_space = Some(address_space.clone());
    task.cold.user_entry = Some(user.entry.as_u64());
    task.cold.user_stack_top = Some(user.stack_top.as_u64());

    x86_64::instructions::interrupts::without_interrupts(|| {
        SMP_SCHEDULER.spawn(task);
    });
    let _ = crate::task::ghost::publish_event(crate::task::ghost::MSG_TASK_NEW, task_id as u64, 0);
    Ok((task_id, address_space))
}

// ============================================================================
// CONTEXT SWITCH
// ============================================================================

/// Ana zamanlama fonksiyonu. Task değişimini gerçekleştirir.
///
/// # İşleyiş
/// 1. Scheduler'dan bir sonraki task'ı al (priority + aging ile)
/// 2. Mevcut task'ın context'ini kaydet
/// 3. Yeni task'ın context'ini yükle
/// 4. Gerekirse sayfa tablosu değiştir (CR3)
/// 5. Assembly switch_context ile CPU register'larını değiştir
/// Mevcut CPU ID'sini SMP katmanından oku.
fn get_current_cpu_id() -> u32 {
    crate::cpu::smp::current_cpu_id()
}

fn has_schedulable_work(cpu_id: u32) -> bool {
    unsafe {
        if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
            if worker.len() > 0 {
                return true;
            }
        }
    }
    choose_victim_cpu(cpu_id).is_some()
}

fn choose_victim_cpu(cpu_id: u32) -> Option<usize> {
    let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
    let pressure = scheduler_pressure_snapshot();
    let current_topology = crate::topology::get_cpu_topology(cpu_id).map(|topology| {
        let guard = topology.read();
        (guard.package_id, guard.core_id, guard.numa_node_id)
    });

    let mut best_victim = None;
    let mut best_score = 0u32;

    for victim in 0..cpu_limit {
        if victim == cpu_id as usize || !cpu_slots::is_online(victim as u32) {
            continue;
        }

        let load = unsafe {
            WORKERS
                .get(victim)
                .and_then(|w| w.as_ref())
                .map(|worker| worker.len() as u32)
                .unwrap_or(0)
        };
        if load == 0 {
            continue;
        }

        let mut score = load.saturating_mul(16);
        if let Some((pkg, core, node)) = current_topology {
            if cpu_slots::numa_node(victim as u32) == node {
                score = score.saturating_add(64);
            } else if matches!(pressure.class, SchedulerPressureClass::Critical) {
                score = score.saturating_sub(24);
            }
            if cpu_slots::package_id(victim as u32) == pkg {
                score = score.saturating_add(48);
            } else if matches!(
                pressure.class,
                SchedulerPressureClass::Elevated | SchedulerPressureClass::Critical
            ) {
                score = score.saturating_sub(12);
            }
            if cpu_slots::core_id(victim as u32) != core {
                score = score.saturating_add(8);
            }
        }
        if matches!(pressure.class, SchedulerPressureClass::Critical) {
            score = score.saturating_add((pressure.memory_some_avg10 / 64) as u32);
            if pressure.memory_full_avg10 >= 350 && load < 2 {
                continue;
            }
        }

        if score > best_score {
            best_score = score;
            best_victim = Some(victim);
        }
    }

    best_victim
}

fn restore_worker_tasks(cpu_index: usize, mut deferred: Vec<Box<Task>>) {
    if let Some(worker) = unsafe { WORKERS.get(cpu_index).and_then(|w| w.as_ref()) } {
        while let Some(task) = deferred.pop() {
            worker.push(task);
        }
    }
}

fn take_task_from_worker_by_id(cpu_index: usize, task_id: TaskId) -> Option<Box<Task>> {
    let worker = unsafe { WORKERS.get(cpu_index).and_then(|w| w.as_ref())? };
    let mut deferred = Vec::new();
    let mut found = None;
    let scan_budget = worker.len();

    for _ in 0..scan_budget {
        let Some(task) = worker.pop() else {
            break;
        };
        if task.id == task_id {
            found = Some(task);
            break;
        }
        deferred.push(task);
    }

    restore_worker_tasks(cpu_index, deferred);
    found
}

fn steal_task_from_victim_by_id(victim: usize, task_id: TaskId) -> Option<Box<Task>> {
    let stealer = unsafe { STEALERS.get(victim).and_then(|s| s.as_ref())? };
    let scan_budget = unsafe {
        WORKERS
            .get(victim)
            .and_then(|w| w.as_ref())
            .map(|worker| worker.len())
            .unwrap_or(0)
    };
    let mut deferred = Vec::new();
    let mut found = None;

    for _ in 0..scan_budget {
        let Some(task) = stealer.steal() else {
            break;
        };
        if task.id == task_id {
            found = Some(task);
            break;
        }
        deferred.push(task);
    }

    restore_worker_tasks(victim, deferred);
    found
}

fn take_committed_policy_task(cpu_id: u32, now_tick: u64) -> Option<Box<Task>> {
    let decision = crate::task::ghost::active_policy(cpu_id, now_tick)?;
    let task_id = decision.task_id as TaskId;

    if let Some(task) = take_task_from_worker_by_id(cpu_id as usize, task_id) {
        let _ =
            crate::task::ghost::note_policy_dispatch(cpu_id, decision.task_id, decision.generation);
        return Some(task);
    }

    if let Some(victim_cpu) = crate::task::ghost::task_cpu_hint(decision.task_id)
        .filter(|victim_cpu| *victim_cpu != cpu_id)
    {
        if let Some(task) = steal_task_from_victim_by_id(victim_cpu as usize, task_id) {
            let _ = crate::task::ghost::note_policy_dispatch(
                cpu_id,
                decision.task_id,
                decision.generation,
            );
            return Some(task);
        }
    }

    None
}

/// SMP-aware schedule fonksiyonu
pub fn schedule() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        return;
    }

    // NMI veya HardIRQ içinden zamanlama yapılamaz
    if crate::preempt::in_nmi() || crate::preempt::in_hardirq() {
        return;
    }

    // Yeniden zamanlama bayrağını temizle
    crate::preempt::clear_need_resched();

    let cpu_id = get_current_cpu_id();
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| {
        let _epoch = SMP_EPOCH_DOMAIN.enter(cpu_id);
        // Zamanlama öncesi tam bellek bariyeri
        smp_mb();

        // RCU durağan durum bildirimi: context switch = quiescent state
        crate::rcu::note_quiescent_state(cpu_id);

        unsafe {
            // let mut scheduler = SMP_SCHEDULER.lock(); // Lock'a gerek yok
            let mut next_task_opt: Option<Box<Task>> = None;

            // 4. Context switch hazırlığı
            let old_context_ptr: *mut TaskContext;
            let new_context_ptr: *const TaskContext;
            let mut old_flags = crate::task::task::TaskFlags::NONE;
            let mut new_flags = crate::task::task::TaskFlags::NONE;
            let mut incoming_user_gs_base = 0u64;

            if let Some(mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize].take() {
                let task_ptr: *mut Task = &mut *current as *mut Task;
                old_context_ptr = unsafe { &mut (*task_ptr).context as *mut TaskContext };
                old_flags = current.hot.flags;
                if current.cold.mode == ExecutionMode::LegacyRing3 {
                    if let Some(state) = current.cold.win32.as_mut() {
                        state.gs_base_shadow = unsafe { read_user_gs_base() };
                    }
                }

                if current.state == TaskState::Running {
                    let delta = now.saturating_sub(current.last_start);
                    update_task_vruntime(&mut current, delta);
                    current.state = TaskState::Ready;
                    let _ = crate::task::ghost::publish_event(
                        crate::task::ghost::MSG_TASK_PREEMPT,
                        current.id as u64,
                        cpu_id as u64,
                    );
                }

                match current.state {
                    TaskState::Ready => {
                        if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref())
                        {
                            worker.push(current);
                        }
                    }
                    TaskState::Sleeping { wake_tick } => {
                        SLEEPING_TASKS.lock().schedule(current, wake_tick);
                    }
                    TaskState::Terminated => {
                        let _ = crate::task::ghost::publish_event(
                            crate::task::ghost::MSG_TASK_DEAD,
                            current.id as u64,
                            current.cold.exit_code.unwrap_or(0) as u64,
                        );
                        TERMINATED_TASKS
                            .lock()
                            .push((current.id, current.cold.exit_code.unwrap_or(0)));
                        ZOMBIE_TASKS.lock().push(current);
                    }
                    TaskState::Blocked => {
                        let _ = crate::task::ghost::publish_event(
                            crate::task::ghost::MSG_TASK_BLOCKED,
                            current.id as u64,
                            cpu_id as u64,
                        );
                        // Baska biri tarafindan handle edilir (örneğin event queue'ye konur).
                    }
                    _ => {}
                }
            } else {
                let idle_task = &mut PER_CPU_IDLE_TASK[cpu_id as usize];
                old_context_ptr = &mut idle_task.context;
                old_flags = idle_task.hot.flags; // idle = NO_FPU
            }

            if next_task_opt.is_none() {
                next_task_opt = take_committed_policy_task(cpu_id, now);
            }

            // Yerel worker kuyruğundan almayı dene
            if next_task_opt.is_none() {
                if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
                    next_task_opt = worker.pop();
                }
            }

            // Boşsa iş çalmayı dene
            if next_task_opt.is_none() {
                let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
                let best_victim = choose_victim_cpu(cpu_id);

                if let Some(victim) = best_victim {
                    if let Some(stealer) = STEALERS.get(victim).and_then(|s| s.as_ref()) {
                        if let Some(task) = stealer.steal() {
                            // Affinity kontrolü: çalınan görev bu CPU'da çalışabilir mi?
                            if task.hot.affinity == 0xFFFFFFFF
                                || (task.hot.affinity & (1 << cpu_id)) != 0
                            {
                                next_task_opt = Some(task);
                            } else {
                                // Affinity uyuşmazlığı — görevi geri koy
                                if let Some(w) = WORKERS.get(victim).and_then(|w| w.as_ref()) {
                                    w.push(task);
                                }
                            }
                        }
                    }
                }

                if next_task_opt.is_none() {
                    let start_victim = (crate::random::next_u32() as usize) % cpu_limit;

                    for i in 0..cpu_limit {
                        let victim = (start_victim + i) % cpu_limit;

                        if victim != (cpu_id as usize) {
                            if let Some(stealer) = STEALERS
                                .get(victim)
                                .and_then(|s: &Option<Stealer<Task>>| s.as_ref())
                            {
                                if let Some(task) = stealer.steal() {
                                    // Affinity kontrolü
                                    if task.hot.affinity == 0xFFFFFFFF
                                        || (task.hot.affinity & (1 << cpu_id)) != 0
                                    {
                                        next_task_opt = Some(task);
                                        break;
                                    } else {
                                        // Geri koy
                                        if let Some(w) =
                                            WORKERS.get(victim).and_then(|w| w.as_ref())
                                        {
                                            w.push(task);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // drop(scheduler);

            let mut target_kernel_stack_top: u64;

            if let Some(mut next_task) = next_task_opt {
                // Sinyal teslimi: Task çalışmaya başlamadan önce bekleyen sinyalleri kontrol et
                if crate::task::signal::has_pending_signals(&next_task.cold.signals) {
                    let task_id = next_task.hot.id;
                    let _ = crate::task::signal::deliver_signals(&next_task.cold.signals, task_id);
                }

                // User task için PCID yoksa tahsis et (0 = kernel PCID rezervli)
                if next_task.cold.mode == ExecutionMode::LegacyRing3
                    && next_task.cold.pcid == 0
                    && crate::memory::paging::pcid_active()
                {
                    next_task.cold.pcid = crate::memory::paging::allocate_pcid();
                }

                next_task.state = TaskState::Running;
                next_task.last_start = now;
                let previous_cpu = next_task.last_cpu;
                next_task.last_cpu = cpu_id;
                if next_task.cold.rseq.registered {
                    next_task.cold.rseq.cpu_id_start = cpu_id;
                    next_task.cold.rseq.cpu_id = cpu_id;
                    next_task.cold.rseq.numa_node = cpu_slots::numa_node(cpu_id);
                    next_task.cold.rseq.event_counter =
                        next_task.cold.rseq.event_counter.saturating_add(1);
                    if previous_cpu != cpu_id {
                        next_task.cold.rseq.abort_count =
                            next_task.cold.rseq.abort_count.saturating_add(1);
                    }
                    crate::task::rseq::sync_user_area(&next_task.cold.rseq);
                }

                crate::memory::set_active_address_space(next_task.cold.address_space.clone());

                // Sayfa tablosu değişimi (Ring3 task'lar için) — PCID ile TLB korumalı
                let current_frame = Cr3::read().0;
                let target_frame = match next_task.cold.mode {
                    ExecutionMode::LegacyRing3 => next_task.cold.page_table.unwrap(),
                    ExecutionMode::NativeRust => crate::memory::KERNEL_PML4_FRAME.unwrap(),
                };
                if target_frame != current_frame {
                    // Tek bariyer + PCID-aware CR3 yükle
                    smp_wmb();
                    let target_pcid = if next_task.cold.mode == ExecutionMode::LegacyRing3 {
                        next_task.cold.pcid
                    } else {
                        0
                    };
                    let noflush = crate::memory::paging::pcid_active() && target_pcid != 0;
                    unsafe {
                        crate::memory::paging::load_cr3_with_pcid(
                            target_frame.start_address(),
                            target_pcid,
                            noflush,
                        );
                    }
                }

                PER_CPU_CURRENT_TASK[cpu_id as usize] = Some(next_task);
                let next_ref = PER_CPU_CURRENT_TASK[cpu_id as usize].as_ref().unwrap();
                new_context_ptr = &next_ref.context;
                target_kernel_stack_top = next_ref.kernel_stack_top;
                new_flags = next_ref.hot.flags;
                incoming_user_gs_base = next_ref
                    .cold
                    .win32
                    .filter(|_| next_ref.cold.mode == ExecutionMode::LegacyRing3)
                    .map(|state| {
                        if state.gs_base_shadow != 0 {
                            state.gs_base_shadow
                        } else {
                            state.teb_base
                        }
                    })
                    .unwrap_or(0);
            } else {
                let idle_task = &PER_CPU_IDLE_TASK[cpu_id as usize];
                new_context_ptr = &idle_task.context;
                PER_CPU_CURRENT_TASK[cpu_id as usize] = None;
                crate::memory::set_active_address_space(None);
                target_kernel_stack_top = idle_task.kernel_stack_top;
                new_flags = idle_task.hot.flags; // idle = NO_FPU
                incoming_user_gs_base = 0;
            }

            crate::gdt::set_kernel_stack(VirtAddr::new(target_kernel_stack_top));
            crate::syscall::set_kernel_stack_for_current_cpu(target_kernel_stack_top);
            unsafe {
                write_user_gs_base(incoming_user_gs_base);
            }

            // FPU mode flags hesapla — asm'e RDX olarak geçirilir
            let mut fpu_mode: u64 = 0;
            if old_flags.contains(crate::task::task::TaskFlags::NO_FPU) {
                fpu_mode |= 1; // bit 0: skip old FPU save
            }
            if new_flags.contains(crate::task::task::TaskFlags::NO_FPU)
                || new_flags.contains(crate::task::task::TaskFlags::FPU_PRISTINE)
            {
                fpu_mode |= 2; // bit 1: skip new FPU restore
            }
            let xsave_caps = crate::cpu::xsave_capabilities();
            if xsave_caps.has_xsave {
                fpu_mode |= 4; // bit 2: use xsave/xrstor family
            }
            if xsave_caps.has_xsaveopt {
                fpu_mode |= 8; // bit 3: prefer xsaveopt over xsave
            }

            // Bağlam değişiminden önce son bellek bariyeri
            smp_mb();
            crate::security::spectre::on_context_switch();
            switch_context(old_context_ptr, new_context_ptr, fpu_mode);

            // Context switch'ten döndük — eski task (biz) tekrar çalışıyor.
            // FPU_PRISTINE bayrağını temizle: bir sonraki switch-in'de FPU restore yapmalı.
            if let Some(ref mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize] {
                current.hot.flags = current
                    .hot
                    .flags
                    .remove(crate::task::task::TaskFlags::FPU_PRISTINE);
            }
            cpu_slots::set_load(
                cpu_id,
                WORKERS
                    .get(cpu_id as usize)
                    .and_then(|w| w.as_ref())
                    .map(|worker| worker.len() as u32)
                    .unwrap_or(0),
            );
        }
        SMP_EPOCH_DOMAIN.leave(cpu_id);
    });
}

// ============================================================================
// ASSEMBLY: CONTEXT SWITCH — Silicon-Assisted Eager FPU
// ============================================================================

/// x86_64 için context switch assembly kodu.
///
/// RDI = eski context pointer (kayıt yapılacak)
/// RSI = yeni context pointer (yüklenecek)
/// RDX = FPU mode flags:
///   bit 0: skip old FPU save (NO_FPU)
///   bit 1: skip new FPU restore (NO_FPU veya FPU_PRISTINE)
///   bit 2: use xsave/xrstor family (XSAVE aktif ise set)
///   bit 3: use xsaveopt64 instead of xsave64
///
/// XSAVE aktifken: xsaveopt64 (hardware lazy — sadece değişen bileşenleri yazar)
/// XSAVE yokken: fxsave64 fallback
#[cfg(not(target_os = "windows"))]
global_asm!(
    r#"
.global switch_context
switch_context:
    // ─── Eski context'i kaydet (RDI'ya) ───
    mov [rdi + 0], r15
    mov [rdi + 8], r14
    mov [rdi + 16], r13
    mov [rdi + 24], r12
    mov [rdi + 32], rbx
    mov [rdi + 40], rbp
    mov [rdi + 48], rsp
    pushfq
    pop rax
    mov [rdi + 56], rax      // RFLAGS
    mov rax, [rsp]
    mov [rdi + 64], rax      // Dönüş adresi (RIP)

    // FPU save — bit 0 set ise atla (NO_FPU)
    test rdx, 1
    jnz .Lskip_old_fpu

    test rdx, 4              // bit 2: XSAVE ailesi aktif mi?
    jnz .Lxsave_old
    fxsave64 [rdi + 128]
    jmp .Lskip_old_fpu
.Lxsave_old:
    // xsaveopt64: EAX:EDX = state component bitmap
    // xsave64/xsaveopt64: EAX:EDX = state component bitmap
    // RDX'i korumam??z laz??m ??? r11'e kaydet (caller-saved, g??venli)
    mov r11, rdx
    mov eax, 0x7              // x87 + SSE + AVX
    xor edx, edx
    test r11, 8              // bit 3: xsaveopt var m???
    jnz .Lxsaveopt_old
    xsave64 [rdi + 128]
    jmp .Lxsave_done
.Lxsaveopt_old:
    xsaveopt64 [rdi + 128]
.Lxsave_done:
    mov rdx, r11              // RDX'i geri y??kle

.Lskip_old_fpu:
    // ─── Yeni context'i yükle (RSI'dan) ───

    // FPU restore — bit 1 set ise atla (NO_FPU veya PRISTINE)
    test rdx, 2
    jnz .Lskip_new_fpu

    test rdx, 4              // bit 2: XSAVE ailesi aktif mi?
    jnz .Lxrstor_new
    fxrstor64 [rsi + 128]
    jmp .Lskip_new_fpu
.Lxrstor_new:
    mov r11, rdx
    mov eax, 0x7
    xor edx, edx
    xrstor64 [rsi + 128]
    mov rdx, r11

.Lskip_new_fpu:
    mov r15, [rsi + 0]
    mov r14, [rsi + 8]
    mov r13, [rsi + 16]
    mov r12, [rsi + 24]
    mov rbx, [rsi + 32]
    mov rbp, [rsi + 40]
    mov rax, [rsi + 56]
    push rax
    popfq                     // RFLAGS
    mov rsp, [rsi + 48]
    mov rax, [rsi + 64]
    jmp rax                   // Yeni task'a atla
"#
);

#[cfg(not(target_os = "windows"))]
unsafe extern "sysv64" {
    /// Assembly'de tanımlanan context switch fonksiyonu.
    /// 3. parametre (fpu_mode): FPU save/restore davranış bayrakları.
    fn switch_context(old: *mut TaskContext, new: *const TaskContext, fpu_mode: u64);
}

#[cfg(target_os = "windows")]
#[no_mangle]
unsafe extern "sysv64" fn switch_context(
    old: *mut TaskContext,
    new: *const TaskContext,
    _fpu_mode: u64,
) {
    // Host-side MSVC test derlemelerinde global_asm! context switch yolu
    // backend hizalama kuralına takıldığı için Rust fallback kullanılır.
    // Bu yol gerçek kernel context switch yerine derleme/test sürekliliği içindir.
    if old.is_null() || new.is_null() {
        return;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(new, old, 1);
    }
}

// ============================================================================
// PROCESS MANAGEMENT (ps, kill, bg, fg)
// ============================================================================

/// Task bilgisi (ps komutu için)
#[derive(Clone)]
pub struct TaskInfo {
    pub pid: TaskId,
    pub name: &'static str,
    pub state: TaskState,
    pub priority: Priority,
    pub cpu_usage: u32,
    pub rss_pages: usize,
    pub swap_pages: usize,
    pub committed_pages: usize,
    pub runtime_ticks: u64,
    pub is_kernel: bool,
    pub children: usize,
}

/// Tüm task'ları listeler (ps komutu için)
pub fn list_tasks() -> Vec<TaskInfo> {
    let mut tasks = Vec::new();
    let now = get_ticks() as u64;

    // Current task'ları al
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = &PER_CPU_CURRENT_TASK[i] {
                let page_counts = task
                    .cold
                    .address_space
                    .as_ref()
                    .map(crate::memory::address_space_page_counts)
                    .unwrap_or_default();
                let running_delta = if task.state == TaskState::Running {
                    now.saturating_sub(task.hot.last_start)
                } else {
                    0
                };
                tasks.push(TaskInfo {
                    pid: task.id,
                    name: task.cold.name,
                    state: task.state,
                    priority: task.hot.priority,
                    cpu_usage: now.saturating_sub(task.hot.last_start).min(u32::MAX as u64) as u32,
                    rss_pages: page_counts.resident_pages(),
                    swap_pages: page_counts.swapped_pages,
                    committed_pages: page_counts.committed_pages,
                    runtime_ticks: task.cold.exec_runtime.saturating_add(running_delta),
                    is_kernel: !matches!(task.cold.mode, ExecutionMode::LegacyRing3)
                        || task.cold.address_space.is_none(),
                    children: task.cold.children.len(),
                });
            }
        }

        // Zombie task'ları al
        let zombies = ZOMBIE_TASKS.lock();
        for task in zombies.iter() {
            let page_counts = task
                .cold
                .address_space
                .as_ref()
                .map(crate::memory::address_space_page_counts)
                .unwrap_or_default();
            tasks.push(TaskInfo {
                pid: task.id,
                name: task.cold.name,
                state: TaskState::Zombie,
                priority: task.hot.priority,
                cpu_usage: 0,
                rss_pages: page_counts.resident_pages(),
                swap_pages: page_counts.swapped_pages,
                committed_pages: page_counts.committed_pages,
                runtime_ticks: task.cold.exec_runtime,
                is_kernel: !matches!(task.cold.mode, ExecutionMode::LegacyRing3)
                    || task.cold.address_space.is_none(),
                children: task.cold.children.len(),
            });
        }
    });

    tasks
}

/// Task'ı PID ile sonlandırır (kill komutu)
pub fn kill_task(pid: TaskId, signal: i32) -> Result<(), &'static str> {
    let mut found = false;

    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        // Current task'ları kontrol et
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = &mut PER_CPU_CURRENT_TASK[i] {
                if task.id == pid {
                    // SIGKILL (9) veya SIGTERM (15)
                    if signal == 9 || signal == 15 {
                        task.state = TaskState::Terminated;
                        task.cold.exit_code = Some(128 + signal);
                        crate::serial_println!(
                            "[KILL] Task {} terminated by signal {}",
                            pid,
                            signal
                        );
                        found = true;
                        break;
                    }
                    // SIGSTOP (19) - suspend
                    if signal == 19 {
                        task.state = TaskState::Stopped;
                        crate::serial_println!("[KILL] Task {} stopped", pid);
                        found = true;
                        break;
                    }
                    // SIGCONT (18) - resume
                    if signal == 18 {
                        if task.state == TaskState::Stopped {
                            task.state = TaskState::Ready;
                            crate::serial_println!("[KILL] Task {} resumed", pid);
                        }
                        found = true;
                        break;
                    }
                }
            }
        }
    });

    if found {
        Ok(())
    } else {
        Err("Task bulunamadi")
    }
}

/// Task'ı durdurur (SIGSTOP/SIGTSTP semantiği)
pub fn stop_task(pid: TaskId) {
    let _ = kill_task(pid, 19); // SIGSTOP
}

/// Durmuş task'ı devam ettirir (SIGCONT semantiği)
pub fn continue_task(pid: TaskId) {
    let _ = kill_task(pid, 18); // SIGCONT
}

/// Mevcut task'ı background'a atar (bg)
pub fn background_current() -> Option<TaskId> {
    let cpu_id = get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK[cpu_id as usize].as_mut() {
            let pid = current.id;
            current.cold.is_background = true;
            crate::serial_println!("[BG] Task {} background'a atandi", pid);
            return Some(pid);
        }
        None
    })
}

/// Ön plandaki (foreground) görevi döndürür — Ctrl+Z job control için.
/// Shell kendisi hariç, çalışan foreground task'ı arar.
pub fn get_foreground_task() -> Option<TaskId> {
    let current_cpu = get_current_cpu_id();
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        // Önce diğer CPU'lardaki foreground task'lara bak
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if i == current_cpu as usize {
                continue;
            }
            if let Some(task) = &PER_CPU_CURRENT_TASK[i] {
                if !task.cold.is_background {
                    return Some(task.id);
                }
            }
        }
        None
    })
}

/// Task'ı foreground'a getirir (fg)
pub fn foreground_task(pid: TaskId) -> Result<(), &'static str> {
    let mut found = false;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = &mut PER_CPU_CURRENT_TASK[i] {
                if task.id == pid {
                    task.cold.is_background = false;
                    crate::serial_println!("[FG] Task {} foreground'a getirildi", pid);
                    found = true;
                    break;
                }
            }
        }
    });
    if found {
        Ok(())
    } else {
        Err("Task bulunamadi")
    }
}

/// Task'ın durumunu getirir
pub fn get_task_state(pid: TaskId) -> Option<TaskState> {
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = &PER_CPU_CURRENT_TASK[i] {
                if task.id == pid {
                    return Some(task.state);
                }
            }
        }
        None
    })
}

/// CPU sayısını döndürür
pub fn get_cpu_count() -> u32 {
    SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed)
}

/// Belirtilen CPU'nun çalışma kuyruğundan bir görev çalar.
/// Hotplug tarafından CPU çevrimdışı yapılırken görev taşıması için kullanılır.
pub fn steal_from_cpu(cpu_id: u32) -> Option<Box<Task>> {
    unsafe {
        if let Some(stealer) = STEALERS.get(cpu_id as usize).and_then(|s| s.as_ref()) {
            stealer.steal()
        } else {
            None
        }
    }
}

/// Belirtilen CPU'nun çalışma kuyruğuna bir görev ekler.
/// Hotplug tarafından görev taşıması için kullanılır.
pub fn push_to_cpu(cpu_id: u32, task: Box<Task>) {
    unsafe {
        if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
            worker.push(task);
        } else if let Some(worker) = WORKERS.get(0).and_then(|w| w.as_ref()) {
            // Hedef CPU'da worker yoksa CPU 0'a gönder
            worker.push(task);
        }
    }
}

/// Scheduler izleme istatistik yapısı
#[derive(Clone, Debug)]
pub struct SchedulerStats {
    pub total_tasks: usize,
    pub running_tasks: usize,
    pub zombie_count: usize,
    pub runnable_tasks: usize,
}

/// Scheduler istatistiklerini döndürür.
pub fn get_stats() -> SchedulerStats {
    let mut total = 0;
    let mut running = 0;
    let mut runnable = 0;

    unsafe {
        for task_opt in PER_CPU_CURRENT_TASK.iter() {
            if task_opt.is_some() {
                total += 1;
                running += 1;
            }
        }

        for worker in WORKERS.iter() {
            if let Some(w) = worker {
                runnable += w.len();
            }
        }
    }

    let zombie_count = ZOMBIE_TASKS.lock().len();

    SchedulerStats {
        total_tasks: total,
        running_tasks: running,
        zombie_count,
        runnable_tasks: runnable,
    }
}

/// Ertelenmiş zamanlayıcı callback'lerini işler (Timer softirq'dan çağrılır).
/// İleride hrtimer wheel ve deferred timer yapıları eklenecektir.
pub fn process_deferred_timers() {
    // Uyuyan görevleri kontrol et — ana tick'ten sonra kaçırılmış olabilir
    let ticks = get_ticks();
    wake_sleeping_tasks(ticks);
}

// ============================================================================
// WAITQUEUE — Bloklanmış görevler için uyandırma mekanizması
// ============================================================================

/// WaitQueue: Bir olay/kaynak üzerinde bekleyen task listesi.
///
/// Linux çekirdeğindeki wait_queue_head_t karşılığıdır.
/// IPC (pipe, channel, semaphore, msgq), futex, TTY I/O gibi her türlü
/// "bir koşul gerçekleşene kadar bekle" senaryosunda kullanılır.
///
/// ```text
///  Task A: wq.sleep()  ──► [WaitQueue: A, B, C]  ◄── Task B: wq.sleep()
///                                   │
///  Kaynak hazır:       wq.wake_one()  →  Task A Ready
///                      wq.wake_all()  →  A,B,C hepsi Ready
/// ```
pub struct WaitQueue {
    waiters: Mutex<Vec<Box<Task>>>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            waiters: Mutex::new(Vec::new()),
        }
    }

    /// Mevcut task'ı bu wait queue üzerinde uyutur.
    /// Task Blocked durumuna geçer ve schedule() çağrılır.
    /// Uyandırıldığında bu fonksiyondan geri döner.
    pub fn sleep(&self) {
        let cpu_id = get_current_cpu_id();
        let now = get_ticks() as u64;

        x86_64::instructions::interrupts::without_interrupts(|| unsafe {
            if let Some(mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize].take() {
                let delta = now.saturating_sub(current.last_start);
                update_task_vruntime(&mut current, delta);
                current.state = TaskState::Blocked;
                self.waiters.lock().push(current);
            }
        });

        schedule();
    }

    /// Bir bekleyeni uyandırır (FIFO sırasıyla).
    /// Uyandırılan task Ready olarak scheduler'a eklenir.
    /// Dönen değer: uyandırılan task sayısı (0 veya 1).
    pub fn wake_one(&self) -> usize {
        let task = self.waiters.lock().pop();
        if let Some(mut t) = task {
            t.state = TaskState::Ready;
            SMP_SCHEDULER.spawn_boxed(t);
            1
        } else {
            0
        }
    }

    /// Tüm bekleyenleri uyandırır.
    /// Dönen değer: uyandırılan task sayısı.
    pub fn wake_all(&self) -> usize {
        let mut waiters = self.waiters.lock();
        let count = waiters.len();
        for mut t in waiters.drain(..) {
            t.state = TaskState::Ready;
            SMP_SCHEDULER.spawn_boxed(t);
        }
        count
    }

    /// Bekleyen task sayısını döndürür.
    pub fn waiter_count(&self) -> usize {
        self.waiters.lock().len()
    }

    /// Bekleyen var mı?
    pub fn has_waiters(&self) -> bool {
        !self.waiters.lock().is_empty()
    }
}

/// Belirtilen task'ı bloklar (Blocked durumuna geçirir) ve schedule() çağırır.
/// Harici bir WaitQueue'ya koymak istemiyorsanız kullanabilirsiniz.
pub fn take_current_blocked_task() -> Option<Box<Task>> {
    let cpu_id = get_current_cpu_id();
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        let mut current = PER_CPU_CURRENT_TASK.get_mut(cpu_id as usize)?.take()?;
        let delta = now.saturating_sub(current.last_start);
        update_task_vruntime(&mut current, delta);
        current.state = TaskState::Blocked;
        Some(current)
    })
}

pub fn wake_blocked_task(mut task: Box<Task>) {
    let task_id = task.id as u64;
    task.state = TaskState::Ready;
    let _ = crate::task::ghost::publish_event(crate::task::ghost::MSG_TASK_WAKEUP, task_id, 0);
    SMP_SCHEDULER.spawn_boxed(task);
}

pub fn spawn_task(task: Task) -> TaskId {
    let task_id = task.id;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SMP_SCHEDULER.spawn(task);
    });
    task_id
}

pub fn block_current_task() {
    let cpu_id = get_current_cpu_id();
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        if let Some(current) = PER_CPU_CURRENT_TASK[cpu_id as usize].as_mut() {
            let delta = now.saturating_sub(current.last_start);
            update_task_vruntime(current, delta);
            current.state = TaskState::Blocked;
        }
    });

    schedule();
}

/// Belirtilen PID'li task'ı Ready yaparak scheduler'a ekler.
/// WaitQueue yerine doğrudan task ID ile uyandırma gerektiğinde kullanılır.
pub fn unblock_task(pid: TaskId) -> bool {
    // Blocked task'ı bulmak zor çünkü schedule() sırasında
    // Blocked task PER_CPU_CURRENT_TASK'tan alınıp hiçbir yere konmaz.
    // Bu fonksiyon yalnızca PER_CPU_CURRENT_TASK'ta bulunan Blocked task'ları
    // Ready yapabilir. WaitQueue kullanımı tercih edilmelidir.
    let mut found = false;
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = PER_CPU_CURRENT_TASK[i].as_mut() {
                if task.id == pid && task.state == TaskState::Blocked {
                    task.state = TaskState::Ready;
                    found = true;
                    break;
                }
            }
        }
    });
    found
}
