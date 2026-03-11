//! # CFS (Completely Fair Scheduler) - echOS Implementasyonu
//!
//! Linux CFS ile aynı mantıkta çalışan, adil ve verimli process scheduler.
//! Red-black tree tabanlı virtual runtime hesaplaması ile CPU zamanını adil dağıtır.
//!
//! ## CFS Nedir?
//!
//! CFS, process'lere CPU zamanını "tamamen adil" bir şekilde dağıtan
//! modern bir scheduling algoritmasıdır. Her process'in virtual runtime'ı
//! tutularak en az çalışanı seçer.
//!
//! ## CFS Mekanizması
//!
//! ```text
//! Virtual Runtime (vruntime):
//! Process A: vruntime = 1000ms
//! Process B: vruntime = 800ms  <- En az çalışan, bir sonraki seçilecek
//! Process C: vruntime = 1200ms
//!
//! Red-Black Tree:
//!         (800) Process B
//!        /        \
//!   (1000) A     (1200) C
//! ```
//!
//! ## Özellikler
//! - O(1) enqueue/dequeue (red-black tree)
//! - Adil CPU zamanı dağılımı
//! - Nice değer desteği (öncelik)
//! - CPU affinity desteği
//! - Real-time process desteği

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// CFS SABİTLERİ
// ============================================================================

/// Scheduler tick frekansı (Hz)
pub const SCHED_HZ: u64 = 1000;

/// Bir tick'in süresi (nanosaniye)
pub const SCHED_TICK_NS: u64 = 1_000_000_000 / SCHED_HZ;

/// Minimum granularity (ns)
pub const MIN_GRANULARITY_NS: u64 = 1_000_000; // 1ms

/// Varsayılan timeslice (ns)
pub const DEFAULT_TIMESLICE_NS: u64 = 10_000_000; // 10ms

/// Nice değer aralığı
pub const MIN_NICE: i8 = -20;
pub const MAX_NICE: i8 = 19;
pub const NICE_WIDTH: i8 = MAX_NICE - MIN_NICE + 1;

/// Nice değerine göre timeslice çarpanı
pub const NICE_TO_TIMESLICE_FACTOR: f64 = 1.25;

/// CFS process durumları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfsTaskState {
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
}

/// CFS hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfsError {
    /// Geçersiz process ID
    InvalidPid,
    /// Process bulunamadı
    TaskNotFound,
    /// Zaten çalışıyor
    AlreadyRunning,
    /// İzin hatası
    PermissionDenied,
    /// Bellek yetersiz
    OutOfMemory,
}

// ============================================================================
// CFS TASK STRUCTURE
// ============================================================================

/// CFS task yapısı
#[derive(Clone, Debug)]
pub struct CfsTask {
    /// Process ID
    pub pid: u32,
    /// Process adı
    pub name: String,
    /// Nice değeri (-20 to 19)
    pub nice: i8,
    /// Virtual runtime
    pub vruntime: u64,
    /// Actual runtime
    pub runtime: u64,
    /// Timeslice (ns)
    pub timeslice: u64,
    /// Durum
    pub state: CfsTaskState,
    /// CPU affinity mask
    pub cpu_mask: u64,
    /// Real-time mi?
    pub is_realtime: bool,
    /// Real-time priority (0-99)
    pub rt_priority: u8,
    /// Son çalıştığı CPU
    pub last_cpu: u32,
    /// Oluşturulma zamanı
    pub create_time: u64,
    /// Kuyrukta mı?
    pub queued: bool,
    /// Red-black tree node (placeholder)
    pub rb_node: Option<RbNode>,
}

/// Red-black tree node (placeholder)
#[derive(Clone, Debug)]
pub struct RbNode {
    pub key: u64,
    pub color: RbColor,
    pub left: Option<Box<RbNode>>,
    pub right: Option<Box<RbNode>>,
    pub parent: Option<*mut RbNode>,
}

/// Red-black tree renkleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RbColor {
    Red,
    Black,
}

impl CfsTask {
    /// Yeni CFS task oluştur
    pub fn new(pid: u32, name: &str, nice: i8) -> Self {
        let timeslice = Self::calculate_timeslice(nice);
        
        Self {
            pid,
            name: name.to_string(),
            nice,
            vruntime: 0,
            runtime: 0,
            timeslice,
            state: CfsTaskState::Runnable,
            cpu_mask: u64::MAX, // Tüm CPU'larda çalışabilir
            is_realtime: false,
            rt_priority: 0,
            last_cpu: 0,
            create_time: crate::interrupts::get_ticks(),
            queued: false,
            rb_node: None,
        }
    }
    
    /// Nice değerine göre timeslice hesapla
    fn calculate_timeslice(nice: i8) -> u64 {
        let nice_offset = nice - MIN_NICE;
        let factor = NICE_TO_TIMESLICE_FACTOR.powi(nice_offset as i32);
        (DEFAULT_TIMESLICE_NS as f64 / factor) as u64
    }
    
    /// Virtual runtime'ı güncelle
    pub fn update_vruntime(&mut self, delta_ns: u64) {
        self.runtime += delta_ns;
        
        // Nice değerine göre vruntime'ı ayarla
        let weight = Self::nice_to_weight(self.nice);
        let vruntime_delta = (delta_ns as f64 * 1024.0 / weight) as u64;
        self.vruntime += vruntime_delta;
    }
    
    /// Nice değerine göre ağırlık hesapla
    fn nice_to_weight(nice: i8) -> f64 {
        // Linux CFS weight table
        let weights = [
            88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14978, 11916, 9548,
            7620, 6100, 4904, 3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423,
            335, 272, 215, 172, 137, 110, 87, 70, 56, 45, 36, 29, 23, 18, 15, 12, 10, 8, 7, 5, 4
        ];
        
        let index = (nice - MIN_NICE) as usize;
        if index < weights.len() {
            weights[index] as f64
        } else {
            1024.0 // Default weight
        }
    }
    
    /// CPU affinity kontrol et
    pub fn can_run_on_cpu(&self, cpu_id: u32) -> bool {
        (self.cpu_mask >> cpu_id) & 1 == 1
    }
    
    /// CPU affinity ayarla
    pub fn set_cpu_affinity(&mut self, cpu_mask: u64) {
        self.cpu_mask = cpu_mask;
    }
    
    /// Real-time yap
    pub fn make_realtime(&mut self, rt_priority: u8) {
        self.is_realtime = true;
        self.rt_priority = rt_priority;
        self.timeslice = 0; // Real-time process'lerin timeslice'ı yok
    }
    
    /// Normal process yap
    pub fn make_normal(&mut self) {
        self.is_realtime = false;
        self.rt_priority = 0;
        self.timeslice = Self::calculate_timeslice(self.nice);
    }
}

// ============================================================================
// CFS RUNQUEUE
// ============================================================================

/// CFS runqueue (her CPU için)
pub struct CfsRunQueue {
    /// CPU ID
    pub cpu_id: u32,
    /// Red-black tree (vruntime'a göre sıralı)
    pub rb_tree: Mutex<BTreeMap<u64, Arc<Mutex<CfsTask>>>>,
    /// Toplam task sayısı
    pub nr_running: AtomicUsize,
    /// Minimum vruntime
    pub min_vruntime: AtomicU64,
    /// Mevcut çalışan task
    pub current: Mutex<Option<Arc<Mutex<CfsTask>>>>,
    /// CPU aktif mi?
    pub active: AtomicBool,
    /// Load average
    pub load: AtomicU64,
}

impl CfsRunQueue {
    /// Yeni runqueue oluştur
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            rb_tree: Mutex::new(BTreeMap::new()),
            nr_running: AtomicUsize::new(0),
            min_vruntime: AtomicU64::new(0),
            current: Mutex::new(None),
            active: AtomicBool::new(true),
            load: AtomicU64::new(0),
        }
    }
    
    /// Task'ı kuyruğa ekle
    pub fn enqueue(&self, task: Arc<Mutex<CfsTask>>) -> Result<(), CfsError> {
        let task_data = task.lock();
        
        if task_data.queued {
            return Err(CfsError::AlreadyRunning);
        }
        
        let vruntime = task_data.vruntime;
        drop(task_data);
        
        // Red-black tree'ye ekle
        let mut rb_tree = self.rb_tree.lock();
        rb_tree.insert(vruntime, task.clone());
        
        self.nr_running.fetch_add(1, Ordering::SeqCst);
        
        // Task'ı güncelle
        task.lock().queued = true;
        
        crate::serial_println!(
            "[CFS] Enqueued task {} (pid {}) on CPU {} with vruntime {}",
            task.lock().name,
            task.lock().pid,
            self.cpu_id,
            vruntime
        );
        
        Ok(())
    }
    
    /// Task'ı kuyruktan al
    pub fn dequeue(&self, task: Arc<Mutex<CfsTask>>) -> Result<(), CfsError> {
        let task_data = task.lock();
        
        if !task_data.queued {
            return Err(CfsError::TaskNotFound);
        }
        
        let vruntime = task_data.vruntime;
        drop(task_data);
        
        // Red-black tree'den çıkar
        let mut rb_tree = self.rb_tree.lock();
        rb_tree.remove(&vruntime);
        
        self.nr_running.fetch_sub(1, Ordering::SeqCst);
        
        // Task'ı güncelle
        task.lock().queued = false;
        
        crate::serial_println!(
            "[CFS] Dequeued task {} (pid {}) from CPU {}",
            task.lock().name,
            task.lock().pid,
            self.cpu_id
        );
        
        Ok(())
    }
    
    /// Bir sonraki task'ı seç
    pub fn pick_next_task(&self) -> Option<Arc<Mutex<CfsTask>>> {
        let rb_tree = self.rb_tree.lock();
        
        if rb_tree.is_empty() {
            return None;
        }
        
        // En düşük vruntime'a sahip task'ı al
        let (vruntime, task) = rb_tree.iter().next().unwrap();
        
        crate::serial_println!(
            "[CFS] Picked task {} (pid {}) with vruntime {} on CPU {}",
            task.lock().name,
            task.lock().pid,
            vruntime,
            self.cpu_id
        );
        
        Some(task.clone())
    }
    
    /// Task'ı çalıştır
    pub fn schedule(&self) -> Option<Arc<Mutex<CfsTask>>> {
        let next_task = self.pick_next_task();
        
        if let Some(ref task) = next_task {
            // Mevcut task'ı güncelle
            let mut current = self.current.lock();
            *current = Some(task.clone());
            
            // Task durumunu güncelle
            task.lock().state = CfsTaskState::Running;
            task.lock().last_cpu = self.cpu_id;
        }
        
        next_task
    }
    
    /// Context switch yap
    pub fn context_switch(&self, prev_task: Option<Arc<Mutex<CfsTask>>>, next_task: Arc<Mutex<CfsTask>>) {
        if let Some(ref prev) = prev_task {
            // Önceki task'ı güncelle
            let mut prev_data = prev.lock();
            prev_data.state = CfsTaskState::Runnable;
            
            // Virtual runtime'ı güncelle
            let now = crate::interrupts::get_ticks();
            let delta = now * SCHED_TICK_NS - prev_data.runtime;
            prev_data.update_vruntime(delta);
            
            // Tekrar kuyruğa ekle
            drop(prev_data);
            let _ = self.enqueue(prev.clone());
        }
        
        // Yeni task'ı çalıştır
        self.schedule();
    }
    
    /// Load'u hesapla
    pub fn update_load(&self) {
        let nr_running = self.nr_running.load(Ordering::SeqCst) as u64;
        let load = nr_running * 1000; // Load scale factor
        self.load.store(load, Ordering::SeqCst);
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> CfsRunQueueStats {
        CfsRunQueueStats {
            cpu_id: self.cpu_id,
            nr_running: self.nr_running.load(Ordering::SeqCst),
            min_vruntime: self.min_vruntime.load(Ordering::SeqCst),
            load: self.load.load(Ordering::SeqCst),
            active: self.active.load(Ordering::SeqCst),
        }
    }
}

/// CFS runqueue istatistikleri
#[derive(Clone, Debug)]
pub struct CfsRunQueueStats {
    pub cpu_id: u32,
    pub nr_running: usize,
    pub min_vruntime: u64,
    pub load: u64,
    pub active: bool,
}

// ============================================================================
// CFS SCHEDULER
// ============================================================================

/// CFS scheduler (tüm CPU'ları yönetir)
pub struct CfsScheduler {
    /// CPU runqueue'ları
    pub runqueues: Vec<CfsRunQueue>,
    /// Tüm task'lar
    pub tasks: Mutex<BTreeMap<u32, Arc<Mutex<CfsTask>>>>,
    /// Scheduler aktif mi?
    pub active: AtomicBool,
    /// Toplam CPU sayısı
    pub nr_cpus: u32,
    /// Global min_vruntime
    pub global_min_vruntime: AtomicU64,
    /// Scheduler tick sayacı
    pub tick_count: AtomicU64,
}

impl CfsScheduler {
    /// Yeni CFS scheduler oluştur
    pub fn new(nr_cpus: u32) -> Self {
        let mut runqueues = Vec::with_capacity(nr_cpus as usize);
        
        for cpu_id in 0..nr_cpus {
            runqueues.push(CfsRunQueue::new(cpu_id));
        }
        
        Self {
            runqueues,
            tasks: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
            nr_cpus,
            global_min_vruntime: AtomicU64::new(0),
            tick_count: AtomicU64::new(0),
        }
    }
    
    /// Scheduler'ı başlat
    pub fn start(&self) {
        self.active.store(true, Ordering::SeqCst);
        crate::serial_println!("[CFS] CFS Scheduler started with {} CPUs", self.nr_cpus);
    }
    
    /// Scheduler'ı durdur
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        crate::serial_println!("[CFS] CFS Scheduler stopped");
    }
    
    /// Yeni task ekle
    pub fn add_task(&self, task: CfsTask) -> Result<(), CfsError> {
        let pid = task.pid;
        let task_arc = Arc::new(Mutex::new(task));
        
        // Task'ı global listeye ekle
        self.tasks.lock().insert(pid, task_arc.clone());
        
        // En az yüklü CPU'yu seç
        let target_cpu = self.select_cpu_for_task(&task_arc);
        
        // Runqueue'ya ekle
        self.runqueues[target_cpu as usize].enqueue(task_arc)?;
        
        crate::serial_println!("[CFS] Added task {} (pid {}) to CPU {}", 
            task_arc.lock().name, pid, target_cpu);
        
        Ok(())
    }
    
    /// Task'ı kaldır
    pub fn remove_task(&self, pid: u32) -> Result<(), CfsError> {
        let tasks = self.tasks.lock();
        let task = tasks.get(&pid).ok_or(CfsError::TaskNotFound)?.clone();
        drop(tasks);
        
        // Hangi CPU'da olduğunu bul ve kaldır
        let task_data = task.lock();
        let last_cpu = task_data.last_cpu;
        drop(task_data);
        
        self.runqueues[last_cpu as usize].dequeue(task)?;
        
        // Global listeden kaldır
        self.tasks.lock().remove(&pid);
        
        crate::serial_println!("[CFS] Removed task {} (pid {})", pid, pid);
        
        Ok(())
    }
    
    /// Task için CPU seç
    fn select_cpu_for_task(&self, task: &Arc<Mutex<CfsTask>>) -> u32 {
        let task_data = task.lock();
        let cpu_mask = task_data.cpu_mask;
        drop(task_data);
        
        let mut best_cpu = 0;
        let mut min_load = u64::MAX;
        
        // Uygun CPU'lar arasından en az yüklü olanı seç
        for (cpu_id, runqueue) in self.runqueues.iter().enumerate() {
            if !runqueue.active.load(Ordering::SeqCst) {
                continue;
            }
            
            if ((cpu_mask >> cpu_id) & 1) == 0 {
                continue; // CPU affinity uymuyor
            }
            
            let load = runqueue.load.load(Ordering::SeqCst);
            if load < min_load {
                min_load = load;
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
                
                // Real-time task'leri etkileme
                if task_data.is_realtime {
                    continue;
                }
                
                // Timeslice dolmuş mu kontrol et
                let now = crate::interrupts::get_ticks();
                let elapsed = now * SCHED_TICK_NS - task_data.runtime;
                
                if elapsed >= task_data.timeslice {
                    crate::serial_println!(
                        "[CFS] Task {} (pid {}) timeslice expired, rescheduling",
                        task_data.name,
                        task_data.pid
                    );
                    
                    // Task'ı yeniden schedule et
                    task_data.state = CfsTaskState::Runnable;
                    drop(task_data);
                    
                    // Context switch
                    if let Some(next_task) = runqueue.pick_next_task() {
                        runqueue.context_switch(Some(current_task.clone()), next_task);
                    }
                }
            }
        }
        
        // Load'ları güncelle
        for runqueue in &self.runqueues {
            runqueue.update_load();
        }
        
        // Global min_vruntime'ı güncelle
        self.update_global_min_vruntime();
    }
    
    /// Global min_vruntime'ı güncelle
    fn update_global_min_vruntime(&self) {
        let mut min_vruntime = u64::MAX;
        
        for runqueue in &self.runqueues {
            let queue_min = runqueue.min_vruntime.load(Ordering::SeqCst);
            if queue_min < min_vruntime {
                min_vruntime = queue_min;
            }
        }
        
        self.global_min_vruntime.store(min_vruntime, Ordering::SeqCst);
    }
    
    /// Load balancing
    pub fn load_balance(&self) {
        crate::serial_println!("[CFS] Load balancing");
        
        let mut total_load = 0;
        let mut active_cpus = 0;
        
        // Toplam load'u hesapla
        for runqueue in &self.runqueues {
            if runqueue.active.load(Ordering::SeqCst) {
                total_load += runqueue.load.load(Ordering::SeqCst);
                active_cpus += 1;
            }
        }
        
        if active_cpus == 0 {
            return;
        }
        
        let avg_load = total_load / active_cpus as u64;
        
        // Overloaded CPU'lardan task'ları taşı
        for runqueue in &self.runqueues {
            let load = runqueue.load.load(Ordering::SeqCst);
            
            if load > avg_load * 2 {
                // Bu CPU overloaded, task'ları taşı
                self.migrate_tasks_from_cpu(runqueue.cpu_id);
            }
        }
    }
    
    /// CPU'dan task'ları taşı
    fn migrate_tasks_from_cpu(&self, cpu_id: u32) {
        let runqueue = &self.runqueues[cpu_id as usize];
        
        // En son eklenen task'ı al ve taşı
        if let Some((_, task)) = runqueue.rb_tree.lock().iter().rev().next() {
            let task_clone = task.clone();
            
            // Eski CPU'dan kaldır
            let _ = runqueue.dequeue(task_clone.clone());
            
            // Yeni CPU seç ve ekle
            let target_cpu = self.select_cpu_for_task(&task_clone);
            if target_cpu != cpu_id {
                self.runqueues[target_cpu as usize].enqueue(task_clone.clone()).ok();
                
                crate::serial_println!(
                    "[CFS] Migrated task {} (pid {}) from CPU {} to CPU {}",
                    task_clone.lock().name,
                    task_clone.lock().pid,
                    cpu_id,
                    target_cpu
                );
            }
        }
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> CfsSchedulerStats {
        let mut runqueue_stats = Vec::new();
        
        for runqueue in &self.runqueues {
            runqueue_stats.push(runqueue.get_stats());
        }
        
        CfsSchedulerStats {
            nr_cpus: self.nr_cpus,
            active: self.active.load(Ordering::SeqCst),
            global_min_vruntime: self.global_min_vruntime.load(Ordering::SeqCst),
            tick_count: self.tick_count.load(Ordering::SeqCst),
            runqueues: runqueue_stats,
        }
    }
}

/// CFS scheduler istatistikleri
#[derive(Clone, Debug)]
pub struct CfsSchedulerStats {
    pub nr_cpus: u32,
    pub active: bool,
    pub global_min_vruntime: u64,
    pub tick_count: u64,
    pub runqueues: Vec<CfsRunQueueStats>,
}

// ============================================================================
// GLOBAL CFS SCHEDULER
// ============================================================================

/// Global CFS scheduler
static mut CFS_SCHEDULER: Option<CfsScheduler> = None;
static CFS_SCHEDULER_INIT: AtomicBool = AtomicBool::new(false);

/// CFS scheduler'ı al
pub fn get_scheduler() -> &'static CfsScheduler {
    unsafe {
        CFS_SCHEDULER.as_ref().unwrap()
    }
}

/// CFS scheduler'ı başlat
pub fn init_cfs(nr_cpus: u32) -> Result<(), CfsError> {
    if CFS_SCHEDULER_INIT.load(Ordering::SeqCst) {
        return Ok(());
    }
    
    unsafe {
        CFS_SCHEDULER = Some(CfsScheduler::new(nr_cpus));
    }
    
    CFS_SCHEDULER_INIT.store(true, Ordering::SeqCst);
    get_scheduler().start();
    
    crate::serial_println!("[CFS] CFS Scheduler initialized with {} CPUs", nr_cpus);
    
    Ok(())
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Yeni process oluştur
pub fn create_process(pid: u32, name: &str, nice: i8) -> Result<(), CfsError> {
    let task = CfsTask::new(pid, name, nice);
    get_scheduler().add_task(task)
}

/// Process'i sonlandır
pub fn terminate_process(pid: u32) -> Result<(), CfsError> {
    get_scheduler().remove_task(pid)
}

/// Process nice değerini değiştir
pub fn set_process_nice(pid: u32, nice: i8) -> Result<(), CfsError> {
    let tasks = get_scheduler().tasks.lock();
    let task = tasks.get(&pid).ok_or(CfsError::TaskNotFound)?.clone();
    drop(tasks);
    
    let mut task_data = task.lock();
    task_data.nice = nice;
    task_data.timeslice = CfsTask::calculate_timeslice(nice);
    
    crate::serial_println!("[CFS] Set nice {} for process {} (pid {})", nice, task_data.name, pid);
    
    Ok(())
}

/// CPU affinity ayarla
pub fn set_process_affinity(pid: u32, cpu_mask: u64) -> Result<(), CfsError> {
    let tasks = get_scheduler().tasks.lock();
    let task = tasks.get(&pid).ok_or(CfsError::TaskNotFound)?.clone();
    drop(tasks);
    
    task.lock().set_cpu_affinity(cpu_mask);
    
    crate::serial_println!("[CFS] Set CPU affinity 0x{:x} for process {} (pid {})", cpu_mask, task.lock().name, pid);
    
    Ok(())
}

/// Scheduler istatistiklerini göster
pub fn print_scheduler_stats() {
    let stats = get_scheduler().get_stats();
    
    crate::serial_println!("[CFS] Scheduler Statistics:");
    crate::serial_println!("  CPUs: {}", stats.nr_cpus);
    crate::serial_println!("  Active: {}", stats.active);
    crate::serial_println!("  Global min_vruntime: {}", stats.global_min_vruntime);
    crate::serial_println!("  Tick count: {}", stats.tick_count);
    
    for rq_stats in &stats.runqueues {
        crate::serial_println!(
            "  CPU {}: {} running, load {}, min_vruntime {}",
            rq_stats.cpu_id,
            rq_stats.nr_running,
            rq_stats.load,
            rq_stats.min_vruntime
        );
    }
}

/// CFS testi
pub fn test_cfs() -> Result<(), CfsError> {
    crate::serial_println!("[CFS] Testing CFS Scheduler");
    
    // Test process'leri oluştur
    create_process(1001, "test_process_1", 0)?;  // Normal nice
    create_process(1002, "test_process_2", -10)? // High priority
    create_process(1003, "test_process_3", 10)?;  // Low priority
    
    // Nice değerlerini değiştir
    set_process_nice(1001, 5)?;
    set_process_nice(1002, -5)?;
    
    // CPU affinity ayarla
    set_process_affinity(1001, 0b0001)?; // Sadece CPU 0
    set_process_affinity(1002, 0b0010)?; // Sadece CPU 1
    
    // Birkaç scheduler tick çalıştır
    for _ in 0..10 {
        get_scheduler().scheduler_tick();
    }
    
    // Load balancing
    get_scheduler().load_balance();
    
    // İstatistikleri göster
    print_scheduler_stats();
    
    // Process'leri temizle
    terminate_process(1001)?;
    terminate_process(1002)?;
    terminate_process(1003)?;
    
    crate::serial_println!("[CFS] CFS test completed");
    
    Ok(())
}
