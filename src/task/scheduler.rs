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

#![allow(unused)]
#![allow(static_mut_refs)]

use super::task::{ExecutionMode, Priority, Task, TaskContext, TaskId, TaskState};
use super::deque::{Worker, Stealer};
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicUsize, AtomicU32, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::tlb;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::OffsetPageTable;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb, smp_acquire, smp_release};
use x86_64::VirtAddr;

const MAX_CPUS: usize = 8192;

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
        Self { cpu_count: AtomicU32::new(cpu_count) }
    }

    pub fn allocate_task_id(&self) -> TaskId {
        NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub fn spawn(&self, task: Task) {
        let cpu_id = get_current_cpu_id() as usize;
        unsafe {
            if let Some(worker) = WORKERS.get(cpu_id).and_then(|w| w.as_ref()) {
                worker.push(Box::new(task));
            } else if let Some(worker) = WORKERS.get(0).and_then(|w| w.as_ref()) {
                worker.push(Box::new(task));
            } else {
                crate::serial_println!("ERROR: No workers available to spawn task!");
            }
        }
    }
    
    // Zaten box'lanmış görevler için dahili yardımcı (örn. timer'dan gelen görevler)
    pub fn spawn_boxed(&self, task: Box<Task>) {
        let cpu_id = get_current_cpu_id() as usize;
        unsafe {
            if let Some(worker) = WORKERS.get(cpu_id).and_then(|w| w.as_ref()) {
                worker.push(task);
            } else if let Some(worker) = WORKERS.get(0).and_then(|w| w.as_ref()) {
                worker.push(task);
            } else {
                crate::serial_println!("ERROR: No workers available to spawn task!");
            }
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
static mut PER_CPU_CURRENT_TASK: Vec<Option<Box<Task>>> = Vec::new();

static TERMINATED_TASKS: Mutex<Vec<(TaskId, i32)>> = Mutex::new(Vec::new());
static ZOMBIE_TASKS: Mutex<Vec<Box<Task>>> = Mutex::new(Vec::new());

/// Per-CPU idle task'ları
static mut PER_CPU_IDLE_TASK: Vec<Box<Task>> = Vec::new();
static mut PER_CPU_DUMMY_CONTEXT: Vec<TaskContext> = Vec::new();

/// Sistem başlangıcından beri geçen tick sayısı
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

const NICE_0_LOAD: u64 = 1024;
const SCHED_LATENCY_TICKS: u64 = 20;
const MIN_GRANULARITY_TICKS: u64 = 4;
const LOAD_BALANCE_INTERVAL: usize = 100;
const VRUNTIME_NORMALIZE_INTERVAL: usize = 2000;

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
    let cpu_count = crate::cpu::CPU_INFO.lock().topology.logical_count.max(1).min(MAX_CPUS as u32);

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

    crate::serial_println!("SMP Scheduler initialized for {} CPUs (Chase-Lev)", cpu_count);
    crate::random::init(get_ticks() as u32 + 0xDEADBEEF);
    SCHEDULER_READY.store(true, Ordering::SeqCst);
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
    SCHEDULER_READY.store(true, Ordering::SeqCst);
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

pub fn current_user_target() -> Option<(u64, u64)> {
    let cpu_id = get_current_cpu_id();
    unsafe {
        PER_CPU_CURRENT_TASK
            .get(cpu_id as usize)
            .and_then(|t| t.as_ref())
            .and_then(|task| match (task.cold.user_entry, task.cold.user_stack_top) {
                (Some(entry), Some(stack)) => Some((entry, stack)),
                _ => None,
            })
    }
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
        x86_64::instructions::interrupts::enable();
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
    if name == "gpu_test" {
        crate::serial_println!("DEBUG: Task kuyruğa eklendi! PID: {}", task_id);
    }

    task_id
}

/// Sistem tick sayısını döndürür.
pub fn get_ticks() -> usize {
    TICK_COUNT.load(Ordering::Relaxed)
}

pub fn is_ready() -> bool {
    SCHEDULER_READY.load(Ordering::SeqCst)
}

/// Timer interrupt'tan çağrılır. Her tick'te:
/// 1. Tick sayacını artırır
/// 2. Uyuyan task'ları kontrol eder
/// 3. Time slice dolmuşsa schedule() çağırır
pub fn tick() {
    if !SCHEDULER_READY.load(Ordering::SeqCst) {
        return;
    }
    let ticks = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    wake_sleeping_tasks(ticks + 1);
    
    // update_utilization ve normalize_vruntime devre dışı (basitleştirildi)
    
    if should_preempt(ticks as u64) {
        schedule();
    }

    // Load Update (Her 10 tickte bir)
    if ticks % 10 == 0 {
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

    // Load Balance Report (Her 1000 tickte bir - approx 1-3 sec depending on CPU count)
    if ticks % 1000 == 0 {
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

fn calc_time_slice(_cpu_id: u32, weight: u32) -> u64 {
    // Basit sabit slice veya weight bazlı slice
    // Chase-Lev'de total weight hesaplamak zor (global/local kuyruklar)
    // Şimdilik basitçe weight * sabit veriyoruz.
    let base_slice = SCHED_LATENCY_TICKS;
    // weight 1024 -> base_slice
    let slice = base_slice * (weight as u64) / 1024;
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
        task.state = TaskState::Ready;
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
        if let Some(current) = PER_CPU_CURRENT_TASK.get(cpu_id as usize).and_then(|t| t.as_ref()) {
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
        if let Some(current) = PER_CPU_CURRENT_TASK.get(cpu_id as usize).and_then(|t| t.as_ref()) {
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
/// Mevcut CPU ID'sini al (basit implementasyon)
fn get_current_cpu_id() -> u32 {
    crate::cpu::smp::current_cpu_id()
}

/// SMP-aware schedule fonksiyonu
pub fn schedule() {
    if !SCHEDULER_READY.load(Ordering::SeqCst) {
        return;
    }
    let cpu_id = get_current_cpu_id();
    let now = get_ticks() as u64;

    x86_64::instructions::interrupts::without_interrupts(|| {
        // Zamanlama öncesi tam bellek bariyeri
        smp_mb();
        
        unsafe {
            // let mut scheduler = SMP_SCHEDULER.lock(); // Lock'a gerek yok
            let mut next_task_opt: Option<Box<Task>> = None;

            // 4. Context switch hazırlığı
            let old_context_ptr: *mut TaskContext;
            let new_context_ptr: *const TaskContext;

            if let Some(mut current) = PER_CPU_CURRENT_TASK[cpu_id as usize].take() {
                let task_ptr: *mut Task = &mut *current as *mut Task;
                old_context_ptr = unsafe { &mut (*task_ptr).context as *mut TaskContext };

                if current.state == TaskState::Running {
                    let delta = now.saturating_sub(current.last_start);
                    update_task_vruntime(&mut current, delta);
                    current.state = TaskState::Ready;
                }

                match current.state {
                    TaskState::Ready => {
                        if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
                            worker.push(current);
                        }
                    }
                    TaskState::Sleeping { wake_tick } => {
                        SLEEPING_TASKS.lock().schedule(current, wake_tick);
                    }
                    TaskState::Terminated => {
                        TERMINATED_TASKS.lock().push((current.id, current.cold.exit_code.unwrap_or(0)));
                        ZOMBIE_TASKS.lock().push(current);
                    }
                    TaskState::Blocked => {
                        // Baska biri tarafindan handle edilir (örneğin event queue'ye konur).
                    }
                    _ => {}
                }
            } else {
                let idle_task = &mut PER_CPU_IDLE_TASK[cpu_id as usize];
                old_context_ptr = &mut idle_task.context;
            }

            // Yerel worker kuyruğundan almayı dene
            if let Some(worker) = WORKERS.get(cpu_id as usize).and_then(|w| w.as_ref()) {
                next_task_opt = worker.pop();
            }

            // Boşsa iş çalmayı dene
            if next_task_opt.is_none() {
                let cpu_limit = SMP_SCHEDULER.cpu_count.load(Ordering::Relaxed).max(1) as usize;
                let mut best_victim = None;
                let mut max_load = 0;
                
                {
                    if let Some(state) = crate::cpu::smp::SMP_STATE.try_lock() {
                        for cpu in state.per_cpu_data.iter() {
                            if cpu.online && cpu.cpu_id != cpu_id && cpu.load > max_load {
                                max_load = cpu.load;
                                if (cpu.cpu_id as usize) < cpu_limit {
                                    best_victim = Some(cpu.cpu_id as usize);
                                }
                            }
                        }
                    }
                }

                if let Some(victim) = best_victim {
                    if let Some(stealer) = STEALERS.get(victim).and_then(|s| s.as_ref()) {
                        if let Some(task) = stealer.steal() {
                            next_task_opt = Some(task);
                        }
                    }
                }

                if next_task_opt.is_none() {
                    let start_victim = (crate::random::next_u32() as usize) % cpu_limit;
                    
                    for i in 0..cpu_limit {
                        let victim = (start_victim + i) % cpu_limit;
                        
                        if victim != (cpu_id as usize) {
                            if let Some(stealer) = STEALERS.get(victim).and_then(|s: &Option<Stealer<Task>>| s.as_ref()) {
                                if let Some(task) = stealer.steal() {
                                    next_task_opt = Some(task);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // drop(scheduler);

            let mut target_kernel_stack_top: u64;

            if let Some(mut next_task) = next_task_opt {
                next_task.state = TaskState::Running;
                next_task.last_start = now;

                crate::memory::set_active_address_space(next_task.cold.address_space.clone());

                // Sayfa tablosu değişimi (Ring3 task'lar için)
                let current_frame = Cr3::read().0;
                let target_frame = match next_task.cold.mode {
                    ExecutionMode::LegacyRing3 => next_task.cold.page_table.unwrap(),
                    ExecutionMode::NativeRust => crate::memory::KERNEL_PML4_FRAME.unwrap(),
                };
                if target_frame != current_frame {
                    // CR3 değişimi öncesi bellek bariyerleri
                    smp_wmb();
                    Cr3::write(target_frame, Cr3Flags::empty());
                    tlb::flush_all();
                    smp_mb();
                    crate::cpu::smp::send_tlb_shootdown_ipi();
                    smp_rmb();
                }

                PER_CPU_CURRENT_TASK[cpu_id as usize] = Some(next_task);
                new_context_ptr = &PER_CPU_CURRENT_TASK[cpu_id as usize]
                    .as_ref()
                    .unwrap()
                    .context;
                target_kernel_stack_top = PER_CPU_CURRENT_TASK[cpu_id as usize]
                    .as_ref()
                    .unwrap()
                    .kernel_stack_top;
            } else {
                let idle_task = &PER_CPU_IDLE_TASK[cpu_id as usize];
                new_context_ptr = &idle_task.context;
                PER_CPU_CURRENT_TASK[cpu_id as usize] = None;
                crate::memory::set_active_address_space(None);
                target_kernel_stack_top = idle_task.kernel_stack_top;
            }

            crate::gdt::set_kernel_stack(VirtAddr::new(target_kernel_stack_top));
            crate::syscall::set_kernel_stack_for_current_cpu(target_kernel_stack_top);
            
            // Bağlam değişiminden önce son bellek bariyeri
            smp_mb();
            switch_context(old_context_ptr, new_context_ptr);
        }
    });
}

// ============================================================================
// ASSEMBLY: CONTEXT SWITCH
// ============================================================================

/// x86_64 için context switch assembly kodu.
///
/// RDI = eski context pointer (kayıt yapılacak)
/// RSI = yeni context pointer (yüklenecek)
///
/// Callee-saved register'ları ve SSE/FPU durumunu kaydeder/yükler.
global_asm!(
    r#"
.global switch_context
switch_context:
    // Eski context'i kaydet (RDI'ya)
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
    fxsave64 [rdi + 80]      // SSE/FPU durumunu kaydet
    
    // Yeni context'i yükle (RSI'dan)
    fxrstor64 [rsi + 80]     // SSE/FPU durumunu yükle
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

unsafe extern "sysv64" {
    /// Assembly'de tanımlanan context switch fonksiyonu
    fn switch_context(old: *mut TaskContext, new: *const TaskContext);
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
}

/// Tüm task'ları listeler (ps komutu için)
pub fn list_tasks() -> Vec<TaskInfo> {
    let mut tasks = Vec::new();
    
    // Current task'ları al
    x86_64::instructions::interrupts::without_interrupts(|| unsafe {
        for i in 0..PER_CPU_CURRENT_TASK.len() {
            if let Some(task) = &PER_CPU_CURRENT_TASK[i] {
                tasks.push(TaskInfo {
                    pid: task.id,
                    name: task.cold.name,
                    state: task.state,
                    priority: task.hot.priority,
                    cpu_usage: 0, // TODO: CPU kullanımını hesapla
                });
            }
        }
        
        // Zombie task'ları al
        let zombies = ZOMBIE_TASKS.lock();
        for task in zombies.iter() {
            tasks.push(TaskInfo {
                pid: task.id,
                name: task.cold.name,
                state: TaskState::Zombie,
                priority: task.hot.priority,
                cpu_usage: 0,
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
                        crate::serial_println!("[KILL] Task {} terminated by signal {}", pid, signal);
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
