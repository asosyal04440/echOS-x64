//! # Real-Time Scheduler - echOS Implementasyonu
//!
//! POSIX.1b compliant real-time scheduling with SCHED_FIFO ve SCHED_RR.
//! Deterministik davranış ve düşük gecikme için tasarlanmıştır.
//!
//! ## Real-Time Scheduling Nedir?
//!
//! Real-time scheduler, zaman kısıtlı uygulamalar için deterministik
//! davranış sağlayan scheduling algoritmasıdır. Normal scheduler'dan
//! daha yüksek önceliğe sahiptir.
//!
//! ## Scheduling Politikaları
//!
//! ```text
//! SCHED_FIFO:
//! - Preemptive: yüksek öncelik düşük önceliği preempt eder
//! - Time-slicing yok: process kendi sonlandırana kadar çalışır
//! - Aynı öncelikte FIFO sırası
//!
//! SCHED_RR:
//! - Preemptive: yüksek öncelik düşük önceliği preempt eder
//! - Time-slicing var: quantum süresi dolunca sonraki geçer
//! - Aynı öncelikte round-robin
//! ```
//!
//! ## Özellikler
//! - POSIX.1b uyumlu
//! - 0-99 arası real-time priority
//! - SCHED_FIFO ve SCHED_RR desteği
//! - CPU affinity desteği
//! - Deadline scheduling (gelecek)

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use super::cfs::{CfsTask, CfsTaskState};

// ============================================================================
// REAL-TIME SCHEDULER SABİTLERİ
// ============================================================================

/// Real-time priority aralığı
pub const RT_PRIORITY_MIN: u8 = 0;
pub const RT_PRIORITY_MAX: u8 = 99;

/// SCHED_RR için varsayılan quantum (ms)
pub const DEFAULT_RR_QUANTUM_MS: u32 = 10;

/// Scheduling politikaları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtSchedulingPolicy {
    /// Normal scheduling (CFS)
    Normal,
    /// FIFO real-time scheduling
    Fifo,
    /// Round-robin real-time scheduling
    RoundRobin,
    /// Deadline scheduling (gelecek)
    Deadline,
}

/// Real-time task durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtTaskState {
    /// Çalışmaya hazır
    Runnable,
    /// Çalışıyor
    Running,
    /// Beklemede (I/O, sleep)
    Sleeping,
    /// Durdu (stopped)
    Stopped,
    /// Öldü (zombie)
    Zombie,
    /// Suspend edildi
    Suspended,
}

/// Real-time hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtSchedulerError {
    /// Geçersiz politika
    InvalidPolicy,
    /// Geçersiz priority
    InvalidPriority,
    /// Process bulunamadı
    TaskNotFound,
    /// İzin hatası
    PermissionDenied,
    /// Zaten gerçek zamanlı
    AlreadyRealtime,
    /// Bellek yetersiz
    OutOfMemory,
}

// ============================================================================
// REAL-TIME TASK STRUCTURE
// ============================================================================

/// Real-time task yapısı
#[derive(Clone, Debug)]
pub struct RtTask {
    /// Process ID
    pub pid: u32,
    /// Process adı
    pub name: String,
    /// Scheduling politikası
    pub policy: RtSchedulingPolicy,
    /// Real-time priority (0-99)
    pub rt_priority: u8,
    /// Durum
    pub state: RtTaskState,
    /// CPU affinity mask
    pub cpu_mask: u64,
    /// Son çalıştığı CPU
    pub last_cpu: u32,
    /// Oluşturulma zamanı
    pub create_time: u64,
    /// Toplam çalışma zamanı
    pub total_runtime: u64,
    /// Quantum başlangıç zamanı (SCHED_RR için)
    pub quantum_start: u64,
    /// Kalan quantum (SCHED_RR için)
    pub remaining_quantum: u32,
    /// Kuyrukta mı?
    pub queued: bool,
    /// Preempt edildi mi?
    pub preempted: bool,
}

impl RtTask {
    /// Yeni real-time task oluştur
    pub fn new(pid: u32, name: &str, policy: RtSchedulingPolicy, rt_priority: u8) -> Result<Self, RtSchedulerError> {
        if rt_priority > RT_PRIORITY_MAX {
            return Err(RtSchedulerError::InvalidPriority);
        }
        
        let remaining_quantum = match policy {
            RtSchedulingPolicy::RoundRobin => DEFAULT_RR_QUANTUM_MS,
            _ => 0,
        };
        
        Ok(Self {
            pid,
            name: name.to_string(),
            policy,
            rt_priority,
            state: RtTaskState::Runnable,
            cpu_mask: u64::MAX, // Tüm CPU'larda çalışabilir
            last_cpu: 0,
            create_time: crate::interrupts::get_ticks(),
            total_runtime: 0,
            quantum_start: crate::interrupts::get_ticks(),
            remaining_quantum,
            queued: false,
            preempted: false,
        })
    }
    
    /// Priority'yi güncelle
    pub fn set_priority(&mut self, new_priority: u8) -> Result<(), RtSchedulerError> {
        if new_priority > RT_PRIORITY_MAX {
            return Err(RtSchedulerError::InvalidPriority);
        }
        
        self.rt_priority = new_priority;
        Ok(())
    }
    
    /// Politikayı güncelle
    pub fn set_policy(&mut self, new_policy: RtSchedulingPolicy) -> Result<(), RtSchedulerError> {
        self.policy = new_policy;
        
        // SCHED_RR için quantum'u sıfırla
        if new_policy == RtSchedulingPolicy::RoundRobin {
            self.remaining_quantum = DEFAULT_RR_QUANTUM_MS;
            self.quantum_start = crate::interrupts::get_ticks();
        } else {
            self.remaining_quantum = 0;
        }
        
        Ok(())
    }
    
    /// CPU affinity kontrol et
    pub fn can_run_on_cpu(&self, cpu_id: u32) -> bool {
        (self.cpu_mask >> cpu_id) & 1 == 1
    }
    
    /// CPU affinity ayarla
    pub fn set_cpu_affinity(&mut self, cpu_mask: u64) {
        self.cpu_mask = cpu_mask;
    }
    
    /// Quantum'u güncelle (SCHED_RR için)
    pub fn update_quantum(&mut self) -> bool {
        if self.policy != RtSchedulingPolicy::RoundRobin {
            return false;
        }
        
        let now = crate::interrupts::get_ticks();
        let elapsed = (now - self.quantum_start) * 10; // ticks to ms (placeholder)
        
        if elapsed >= self.remaining_quantum as u64 {
            // Quantum doldu, sıradaki geç
            self.remaining_quantum = DEFAULT_RR_QUANTUM_MS;
            self.quantum_start = now;
            return true;
        }
        
        false
    }
    
    /// Runtime'ı güncelle
    pub fn update_runtime(&mut self, delta_ns: u64) {
        self.total_runtime += delta_ns;
    }
}

// ============================================================================
// REAL-TIME RUNQUEUE
// ============================================================================

/// Real-time runqueue (her CPU için)
pub struct RtRunQueue {
    /// CPU ID
    pub cpu_id: u32,
    /// FIFO kuyrukları (priority'ye göre)
    pub fifo_queues: Mutex<Vec<VecDeque<Arc<Mutex<RtTask>>>>>,
    /// Round-robin kuyrukları (priority'ye göre)
    pub rr_queues: Mutex<Vec<VecDeque<Arc<Mutex<RtTask>>>>>,
    /// Toplam task sayısı
    pub nr_running: AtomicUsize,
    /// Mevcut çalışan task
    pub current: Mutex<Option<Arc<Mutex<RtTask>>>>,
    /// CPU aktif mi?
    pub active: AtomicBool,
    /// Preempt sayısı
    pub preempt_count: AtomicU64,
}

impl RtRunQueue {
    /// Yeni real-time runqueue oluştur
    pub fn new(cpu_id: u32) -> Self {
        let mut fifo_queues = Vec::with_capacity(100);
        let mut rr_queues = Vec::with_capacity(100);
        
        // Her priority için ayrı kuyruk oluştur
        for _ in 0..=RT_PRIORITY_MAX {
            fifo_queues.push(VecDeque::new());
            rr_queues.push(VecDeque::new());
        }
        
        Self {
            cpu_id,
            fifo_queues: Mutex::new(fifo_queues),
            rr_queues: Mutex::new(rr_queues),
            nr_running: AtomicUsize::new(0),
            current: Mutex::new(None),
            active: AtomicBool::new(true),
            preempt_count: AtomicU64::new(0),
        }
    }
    
    /// Task'ı kuyruğa ekle
    pub fn enqueue(&self, task: Arc<Mutex<RtTask>>) -> Result<(), RtSchedulerError> {
        let task_data = task.lock();
        
        if task_data.queued {
            return Err(RtSchedulerError::TaskNotFound);
        }
        
        let priority = task_data.rt_priority as usize;
        let policy = task_data.policy;
        
        drop(task_data);
        
        // Politikaya göre uygun kuyruğa ekle
        match policy {
            RtSchedulingPolicy::Fifo => {
                let mut queues = self.fifo_queues.lock();
                queues[priority].push_back(task.clone());
            }
            RtSchedulingPolicy::RoundRobin => {
                let mut queues = self.rr_queues.lock();
                queues[priority].push_back(task.clone());
            }
            _ => return Err(RtSchedulerError::InvalidPolicy),
        }
        
        self.nr_running.fetch_add(1, Ordering::SeqCst);
        
        // Task'ı güncelle
        task.lock().queued = true;
        
        crate::serial_println!(
            "[RT] Enqueued RT task {} (pid {}) with priority {} on CPU {}",
            task.lock().name,
            task.lock().pid,
            priority,
            self.cpu_id
        );
        
        Ok(())
    }
    
    /// Task'ı kuyruktan al
    pub fn dequeue(&self, task: Arc<Mutex<RtTask>>) -> Result<(), RtSchedulerError> {
        let task_data = task.lock();
        
        if !task_data.queued {
            return Err(RtSchedulerError::TaskNotFound);
        }
        
        let priority = task_data.rt_priority as usize;
        let policy = task_data.policy;
        
        drop(task_data);
        
        // Politikaya göre uygun kuyruktan çıkar
        match policy {
            RtSchedulingPolicy::Fifo => {
                let mut queues = self.fifo_queues.lock();
                queues[priority].retain(|t| t.lock().pid != task.lock().pid);
            }
            RtSchedulingPolicy::RoundRobin => {
                let mut queues = self.rr_queues.lock();
                queues[priority].retain(|t| t.lock().pid != task.lock().pid);
            }
            _ => return Err(RtSchedulerError::InvalidPolicy),
        }
        
        self.nr_running.fetch_sub(1, Ordering::SeqCst);
        
        // Task'ı güncelle
        task.lock().queued = false;
        
        crate::serial_println!(
            "[RT] Dequeued RT task {} (pid {}) from CPU {}",
            task.lock().name,
            task.lock().pid,
            self.cpu_id
        );
        
        Ok(())
    }
    
    /// Bir sonraki task'ı seç
    pub fn pick_next_task(&self) -> Option<Arc<Mutex<RtTask>>> {
        // En yüksek priority'den başla
        for priority in (0..=RT_PRIORITY_MAX).rev() {
            // Önce FIFO kuyruğunu kontrol et
            {
                let fifo_queues = self.fifo_queues.lock();
                if let Some(task) = fifo_queues[priority].front() {
                    crate::serial_println!(
                        "[RT] Picked FIFO task {} (pid {}) with priority {} on CPU {}",
                        task.lock().name,
                        task.lock().pid,
                        priority,
                        self.cpu_id
                    );
                    return Some(task.clone());
                }
            }
            
            // Sonra RR kuyruğunu kontrol et
            {
                let rr_queues = self.rr_queues.lock();
                if let Some(task) = rr_queues[priority].front() {
                    crate::serial_println!(
                        "[RT] Picked RR task {} (pid {}) with priority {} on CPU {}",
                        task.lock().name,
                        task.lock().pid,
                        priority,
                        self.cpu_id
                    );
                    return Some(task.clone());
                }
            }
        }
        
        None
    }
    
    /// Task'ı çalıştır
    pub fn schedule(&self) -> Option<Arc<Mutex<RtTask>>> {
        let next_task = self.pick_next_task();
        
        if let Some(ref task) = next_task {
            // Mevcut task'ı güncelle
            let mut current = self.current.lock();
            *current = Some(task.clone());
            
            // Task durumunu güncelle
            task.lock().state = RtTaskState::Running;
            task.lock().last_cpu = self.cpu_id;
        }
        
        next_task
    }
    
    /// Preempt kontrolü
    pub fn check_preempt(&self, new_task: &Arc<Mutex<RtTask>>) -> bool {
        let current_task = self.current.lock();
        
        if let Some(ref current) = *current_task {
            let current_data = current.lock();
            let new_data = new_task.lock();
            
            // Yeni task'ın priority'si daha yüksek mi?
            if new_data.rt_priority > current_data.rt_priority {
                drop(current_data);
                drop(new_data);
                
                // Preempt sayısını artır
                self.preempt_count.fetch_add(1, Ordering::SeqCst);
                
                crate::serial_println!(
                    "[RT] Preempting task {} (pid {}) with task {} (pid {})",
                    current.lock().name,
                    current.lock().pid,
                    new_task.lock().name,
                    new_task.lock().pid
                );
                
                return true;
            }
        }
        
        false
    }
    
    /// Context switch yap
    pub fn context_switch(&self, prev_task: Option<Arc<Mutex<RtTask>>>, next_task: Arc<Mutex<RtTask>>) {
        if let Some(ref prev) = prev_task {
            // Önceki task'ı güncelle
            let mut prev_data = prev.lock();
            
            if prev_data.policy == RtSchedulingPolicy::RoundRobin {
                // Quantum kontrolü
                if prev_data.update_quantum() {
                    // Quantum doldu, kuyruğun sonuna ekle
                    drop(prev_data);
                    let _ = self.dequeue(prev.clone());
                    let _ = self.enqueue(prev.clone());
                } else {
                    // Quantum dolmadı, kuyruğun başına ekle
                    drop(prev_data);
                    let _ = self.dequeue(prev.clone());
                    let _ = self.enqueue(prev.clone());
                }
            } else {
                // FIFO, kuyruğun başına ekle
                drop(prev_data);
                let _ = self.dequeue(prev.clone());
                let _ = self.enqueue(prev.clone());
            }
        }
        
        // Yeni task'ı çalıştır
        self.schedule();
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> RtRunQueueStats {
        RtRunQueueStats {
            cpu_id: self.cpu_id,
            nr_running: self.nr_running.load(Ordering::SeqCst),
            active: self.active.load(Ordering::SeqCst),
            preempt_count: self.preempt_count.load(Ordering::SeqCst),
        }
    }
}

/// Real-time runqueue istatistikleri
#[derive(Clone, Debug)]
pub struct RtRunQueueStats {
    pub cpu_id: u32,
    pub nr_running: usize,
    pub active: bool,
    pub preempt_count: u64,
}

// ============================================================================
// REAL-TIME SCHEDULER
// ============================================================================

/// Real-time scheduler
pub struct RtScheduler {
    /// CPU runqueue'ları
    pub runqueues: Vec<RtRunQueue>,
    /// Tüm RT task'lar
    pub tasks: Mutex<BTreeMap<u32, Arc<Mutex<RtTask>>>>,
    /// Scheduler aktif mi?
    pub active: AtomicBool,
    /// Toplam CPU sayısı
    pub nr_cpus: u32,
    /// Scheduler tick sayacı
    pub tick_count: AtomicU64,
}

impl RtScheduler {
    /// Yeni real-time scheduler oluştur
    pub fn new(nr_cpus: u32) -> Self {
        let mut runqueues = Vec::with_capacity(nr_cpus as usize);
        
        for cpu_id in 0..nr_cpus {
            runqueues.push(RtRunQueue::new(cpu_id));
        }
        
        Self {
            runqueues,
            tasks: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
            nr_cpus,
            tick_count: AtomicU64::new(0),
        }
    }
    
    /// Scheduler'ı başlat
    pub fn start(&self) {
        self.active.store(true, Ordering::SeqCst);
        crate::serial_println!("[RT] Real-time Scheduler started with {} CPUs", self.nr_cpus);
    }
    
    /// Scheduler'ı durdur
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        crate::serial_println!("[RT] Real-time Scheduler stopped");
    }
    
    /// Yeni RT task ekle
    pub fn add_task(&self, task: RtTask) -> Result<(), RtSchedulerError> {
        let pid = task.pid;
        let task_arc = Arc::new(Mutex::new(task));
        
        // Task'ı global listeye ekle
        self.tasks.lock().insert(pid, task_arc.clone());
        
        // En az yüklü CPU'yu seç
        let target_cpu = self.select_cpu_for_task(&task_arc);
        
        // Runqueue'ya ekle
        self.runqueues[target_cpu as usize].enqueue(task_arc)?;
        
        crate::serial_println!("[RT] Added RT task {} (pid {}) to CPU {}", 
            task_arc.lock().name, pid, target_cpu);
        
        Ok(())
    }
    
    /// Task'ı kaldır
    pub fn remove_task(&self, pid: u32) -> Result<(), RtSchedulerError> {
        let tasks = self.tasks.lock();
        let task = tasks.get(&pid).ok_or(RtSchedulerError::TaskNotFound)?.clone();
        drop(tasks);
        
        // Hangi CPU'da olduğunu bul ve kaldır
        let task_data = task.lock();
        let last_cpu = task_data.last_cpu;
        drop(task_data);
        
        self.runqueues[last_cpu as usize].dequeue(task)?;
        
        // Global listeden kaldır
        self.tasks.lock().remove(&pid);
        
        crate::serial_println!("[RT] Removed RT task {} (pid {})", pid, pid);
        
        Ok(())
    }
    
    /// Task için CPU seç
    fn select_cpu_for_task(&self, task: &Arc<Mutex<RtTask>>) -> u32 {
        let task_data = task.lock();
        let cpu_mask = task_data.cpu_mask;
        let priority = task_data.rt_priority;
        drop(task_data);
        
        let mut best_cpu = 0;
        let mut min_running = usize::MAX;
        
        // Uygun CPU'lar arasından en az çalışanı seç
        for (cpu_id, runqueue) in self.runqueues.iter().enumerate() {
            if !runqueue.active.load(Ordering::SeqCst) {
                continue;
            }
            
            if ((cpu_mask >> cpu_id) & 1) == 0 {
                continue; // CPU affinity uymuyor
            }
            
            let nr_running = runqueue.nr_running.load(Ordering::SeqCst);
            
            // High priority task'lar için daha az yüklü CPU tercih et
            if nr_running < min_running {
                min_running = nr_running;
                best_cpu = cpu_id as u32;
            }
        }
        
        best_cpu
    }
    
    /// Scheduler tick
    pub fn scheduler_tick(&self) {
        if !self.active.load(Ordering::SeqCst) {
            return;
        }
        
        let tick_count = self.tick_count.fetch_add(1, Ordering::SeqCst);
        
        // Her CPU için tick işle
        for runqueue in &self.runqueues {
            if let Some(current_task) = runqueue.current.lock().clone() {
                let mut task_data = current_task.lock();
                
                // Runtime'ı güncelle
                let now = crate::interrupts::get_ticks();
                let delta = now * 10_000_000 - task_data.total_runtime; // ns
                task_data.update_runtime(delta);
                
                // SCHED_RR için quantum kontrolü
                if task_data.policy == RtSchedulingPolicy::RoundRobin {
                    if task_data.update_quantum() {
                        crate::serial_println!(
                            "[RT] Task {} (pid {}) quantum expired, rescheduling",
                            task_data.name,
                            task_data.pid
                        );
                        
                        // Task'ı yeniden schedule et
                        task_data.state = RtTaskState::Runnable;
                        drop(task_data);
                        
                        // Context switch
                        if let Some(next_task) = runqueue.pick_next_task() {
                            runqueue.context_switch(Some(current_task.clone()), next_task);
                        }
                    }
                }
            }
        }
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> RtSchedulerStats {
        let mut runqueue_stats = Vec::new();
        
        for runqueue in &self.runqueues {
            runqueue_stats.push(runqueue.get_stats());
        }
        
        RtSchedulerStats {
            nr_cpus: self.nr_cpus,
            active: self.active.load(Ordering::SeqCst),
            tick_count: self.tick_count.load(Ordering::SeqCst),
            runqueues: runqueue_stats,
        }
    }
}

/// Real-time scheduler istatistikleri
#[derive(Clone, Debug)]
pub struct RtSchedulerStats {
    pub nr_cpus: u32,
    pub active: bool,
    pub tick_count: u64,
    pub runqueues: Vec<RtRunQueueStats>,
}

// ============================================================================
// GLOBAL REAL-TIME SCHEDULER
// ============================================================================

/// Global real-time scheduler
static mut RT_SCHEDULER: Option<RtScheduler> = None;
static RT_SCHEDULER_INIT: AtomicBool = AtomicBool::new(false);

/// Real-time scheduler'ı al
pub fn get_scheduler() -> &'static RtScheduler {
    unsafe {
        RT_SCHEDULER.as_ref().unwrap()
    }
}

/// Real-time scheduler'ı başlat
pub fn init_rt_scheduler(nr_cpus: u32) -> Result<(), RtSchedulerError> {
    if RT_SCHEDULER_INIT.load(Ordering::SeqCst) {
        return Ok(());
    }
    
    unsafe {
        RT_SCHEDULER = Some(RtScheduler::new(nr_cpus));
    }
    
    RT_SCHEDULER_INIT.store(true, Ordering::SeqCst);
    get_scheduler().start();
    
    crate::serial_println!("[RT] Real-time Scheduler initialized with {} CPUs", nr_cpus);
    
    Ok(())
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni real-time process oluştur
pub fn create_rt_process(pid: u32, name: &str, policy: RtSchedulingPolicy, priority: u8) -> Result<(), RtSchedulerError> {
    let task = RtTask::new(pid, name, policy, priority)?;
    get_scheduler().add_task(task)
}

/// Process'i sonlandır
pub fn terminate_rt_process(pid: u32) -> Result<(), RtSchedulerError> {
    get_scheduler().remove_task(pid)
}

/// Process priority'sini değiştir
pub fn set_rt_priority(pid: u32, new_priority: u8) -> Result<(), RtSchedulerError> {
    let tasks = get_scheduler().tasks.lock();
    let task = tasks.get(&pid).ok_or(RtSchedulerError::TaskNotFound)?.clone();
    drop(tasks);
    
    task.lock().set_priority(new_priority)?;
    
    crate::serial_println!("[RT] Set RT priority {} for process {} (pid {})", new_priority, task.lock().name, pid);
    
    Ok(())
}

/// Process scheduling politikasını değiştir
pub fn set_rt_policy(pid: u32, new_policy: RtSchedulingPolicy) -> Result<(), RtSchedulerError> {
    let tasks = get_scheduler().tasks.lock();
    let task = tasks.get(&pid).ok_or(RtSchedulerError::TaskNotFound)?.clone();
    drop(tasks);
    
    task.lock().set_policy(new_policy)?;
    
    crate::serial_println!("[RT] Set RT policy {:?} for process {} (pid {})", new_policy, task.lock().name, pid);
    
    Ok(())
}

/// CPU affinity ayarla
pub fn set_rt_affinity(pid: u32, cpu_mask: u64) -> Result<(), RtSchedulerError> {
    let tasks = get_scheduler().tasks.lock();
    let task = tasks.get(&pid).ok_or(RtSchedulerError::TaskNotFound)?.clone();
    drop(tasks);
    
    task.lock().set_cpu_affinity(cpu_mask);
    
    crate::serial_println!("[RT] Set CPU affinity 0x{:x} for RT process {} (pid {})", cpu_mask, task.lock().name, pid);
    
    Ok(())
}

/// Real-time scheduler istatistiklerini göster
pub fn print_rt_stats() {
    let stats = get_scheduler().get_stats();
    
    crate::serial_println!("[RT] Real-time Scheduler Statistics:");
    crate::serial_println!("  CPUs: {}", stats.nr_cpus);
    crate::serial_println!("  Active: {}", stats.active);
    crate::serial_println!("  Tick count: {}", stats.tick_count);
    
    for rq_stats in &stats.runqueues {
        crate::serial_println!(
            "  CPU {}: {} running, {} preempts",
            rq_stats.cpu_id,
            rq_stats.nr_running,
            rq_stats.preempt_count
        );
    }
}

/// Real-time scheduler testi
pub fn test_rt_scheduler() -> Result<(), RtSchedulerError> {
    crate::serial_println!("[RT] Testing Real-time Scheduler");
    
    // Test process'leri oluştur
    create_rt_process(2001, "rt_fifo_1", RtSchedulingPolicy::Fifo, 80)?;  // High priority FIFO
    create_rt_process(2002, "rt_fifo_2", RtSchedulingPolicy::Fifo, 70)?;  // Medium priority FIFO
    create_rt_process(2003, "rt_rr_1", RtSchedulingPolicy::RoundRobin, 90)?; // High priority RR
    create_rt_process(2004, "rt_rr_2", RtSchedulingPolicy::RoundRobin, 60)?; // Low priority RR
    
    // Priority'leri değiştir
    set_rt_priority(2002, 75)?;
    set_rt_priority(2004, 65)?;
    
    // Politikaları değiştir
    set_rt_policy(2001, RtSchedulingPolicy::RoundRobin)?;
    set_rt_policy(2003, RtSchedulingPolicy::Fifo)?;
    
    // CPU affinity ayarla
    set_rt_affinity(2001, 0b0001)?; // Sadece CPU 0
    set_rt_affinity(2002, 0b0010)?; // Sadece CPU 1
    
    // Birkaç scheduler tick çalıştır
    for _ in 0..20 {
        get_scheduler().scheduler_tick();
    }
    
    // İstatistikleri göster
    print_rt_stats();
    
    // Process'leri temizle
    terminate_rt_process(2001)?;
    terminate_rt_process(2002)?;
    terminate_rt_process(2003)?;
    terminate_rt_process(2004)?;
    
    crate::serial_println!("[RT] Real-time scheduler test completed");
    
    Ok(())
}
