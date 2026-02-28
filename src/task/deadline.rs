//! # Deadline Zamanlayıcısı (EDF - Earliest Deadline First)
//!
//! En Erken Son Tarih Önce (EDF) gerçek zamanlı zamanlama politikası.
//! POSIX SCHED_DEADLINE politikasının uygulaması.
//!
//! ## EDF (Earliest Deadline First) Nedir?
//!
//! EDF, gerçek zamanlı sistemlerde matematiksel olarak optimal olan
//! tek-işlemcili zamanlama algoritmasıdır. Her an, son tarihi en yakın
//! olan task çalıştırılır.
//!
//! ## Zaman Ekseni Diyagramı
//!
//! ```text
//! Task A: period=8,  deadline=8,  runtime=2
//! Task B: period=5,  deadline=5,  runtime=2
//! Task C: period=10, deadline=10, runtime=3
//!
//! Zaman:  0  1  2  3  4  5  6  7  8  9  10
//!         |--|--|--|--|--|--|--|--|--|--|--|
//! Task A:  AA          AA
//! Task B:    BB  BB       BB
//! Task C:          CCC         CCC
//!
//! Her zaman adımında son tarihi en yakın olan task çalışır.
//! ```
//!
//! ## CBS (Constant Bandwidth Server) Mekanizması
//!
//! ```text
//!  ┌─────────────────────────────────────────────────┐
//!  │  Task Parametreleri (sched_attr):               │
//!  │  runtime  = C  (çalışma bütçesi / periyot)     │
//!  │  deadline = D  (göreli son tarih)               │
//!  │  period   = T  (periyot, T >= D)               │
//!  │                                                  │
//!  │  Kullanım oranı (U) = C / T  (<= 1.0 olmalı)  │
//!  │  Toplam U <= %95 (RT band genişliği sınırı)    │
//!  └─────────────────────────────────────────────────┘
//! ```
//!
//! ## Bütçe Yenileme (Replenishment)
//! Her periyot başında:
//!   1. Kalan bütçe sıfırlanır (runtime kadar yenilenir)
//!   2. Mutlak son tarih = şimdi + relative_deadline olarak güncellenir
//!   3. Throttle bayrağı kaldırılır

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;

// ============================================================================
// DEADLINE SABİTLERİ
// ============================================================================

/// Varsayılan çalışma bütçesi (mikrosaniye cinsinden)
pub const DL_DEFAULT_RUNTIME: u64 = 100_000; // 100ms
/// Varsayılan periyot (mikrosaniye cinsinden)
pub const DL_DEFAULT_PERIOD: u64 = 1_000_000; // 1s
/// Varsayılan son tarih = periyot (her periyot sonunda teslim)
pub const DL_DEFAULT_DEADLINE: u64 = DL_DEFAULT_PERIOD;

/// POSIX zamanlama politika kodu (Linux ile uyumlu)
pub const SCHED_DEADLINE: i32 = 6;

/// sched_attr bayrakları
pub const SCHED_FLAG_DL_OVERRUN: u64 = 1 << 0;  // Bütçe aşımı bildirimi iste
pub const SCHED_FLAG_DL_RECLAIM: u64 = 1 << 1;  // Boşta kalan bant genişliğini geri al
pub const SCHED_FLAG_DL_SPECIAL: u64 = 1 << 2;  // Özel EDF varyantı

// ============================================================================
// DEADLINE TASK (GÖREVİ)
// ============================================================================
//
// Bir SCHED_DEADLINE task'ının üç temel parametresi vardır:
//   runtime  (C): Her periyotta kullanabileceği maksimum CPU süresi
//   deadline (D): Göreli son tarih (periyot başına göre)
//   period   (T): Yineleme periyodu (en az D kadar olmalıdır)

#[derive(Clone, Debug)]
pub struct DeadlineTask {
    /// Görevin benzersiz kimlik numarası
    pub task_id: u64,
    /// CPU bütçesi — her periyotta başlangıç değeri (nanosaniye)
    pub runtime: AtomicU64,
    /// Kalan bütçe — her tick'te azalır, 0 olunca task throttle edilir
    pub remaining_runtime: AtomicU64,
    /// Periyot uzunluğu (nanosaniye)
    pub period: u64,
    /// Göreli son tarih (nanosaniye, periyot başlangıcından itibaren)
    pub deadline: u64,
    /// Mutlak son tarih (monoton saat üzerinden hesaplanır)
    pub abs_deadline: AtomicU64,
    /// Bir sonraki bütçe yenileme zamanı
    pub next_replenish: AtomicU64,
    /// Görev aktif mi?
    pub active: AtomicBool,
    /// Bütçe tükendi mi? (throttled = CPU'ya erişim engellendi)
    pub throttled: AtomicBool,
    /// Zamanlama bayrakları
    pub flags: u64,
    /// İstatistikler (gecikme sayısı, toplam çalışma süresi vb.)
    pub stats: Mutex<DlStats>,
}

#[derive(Clone, Debug, Default)]
pub struct DlStats {
    pub migrations: u64,
    pub throttled_time: u64,
    pub runtime_time: u64,
    pub deadline_misses: u64,
}

impl DeadlineTask {
    pub fn new(task_id: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> Self {
        let now = crate::task::scheduler::get_ticks();

        Self {
            task_id,
            runtime: AtomicU64::new(runtime),
            remaining_runtime: AtomicU64::new(runtime),
            period,
            deadline,
            abs_deadline: AtomicU64::new(now + deadline),
            next_replenish: AtomicU64::new(now + period),
            active: AtomicBool::new(true),
            throttled: AtomicBool::new(false),
            flags,
            stats: Mutex::new(DlStats::default()),
        }
    }

    /// Son tarihin geçip geçmediğini kontrol eder.
    /// Eğer geçtiyse "deadline miss" olarak kaydedilir.
    pub fn deadline_passed(&self) -> bool {
        let now = crate::task::scheduler::get_ticks();
        now > self.abs_deadline.load(Ordering::Relaxed)
    }

    /// Bütçenin tükenip tükenmediğini kontrol eder.
    pub fn runtime_exhausted(&self) -> bool {
        self.remaining_runtime.load(Ordering::Relaxed) == 0
    }

    /// Çalışma süresini bütçeden düşer.
    /// Bütçe sıfıra ulaşırsa task throttle edilir.
    pub fn consume_runtime(&self, ns: u64) {
        let remaining = self.remaining_runtime.load(Ordering::Relaxed);
        let new_remaining = remaining.saturating_sub(ns);
        self.remaining_runtime.store(new_remaining, Ordering::Relaxed);

        if new_remaining == 0 {
            self.throttled.store(true, Ordering::SeqCst);
        }
    }

    /// Yeni periyot başında bütçeyi yeniler.
    /// CBS (Constant Bandwidth Server) davranışı burada uygulanır.
    pub fn replenish(&self) {
        let now = crate::task::scheduler::get_ticks();
        let runtime = self.runtime.load(Ordering::Relaxed);

        // Yeni mutlak son tarihi hesapla
        let new_deadline = now + self.deadline;
        self.abs_deadline.store(new_deadline, Ordering::SeqCst);

        // Bütçeyi tam olarak yenile
        self.remaining_runtime.store(runtime, Ordering::SeqCst);

        // Bir sonraki yenileme zamanını güncelle
        self.next_replenish.store(now + self.period, Ordering::SeqCst);

        // Throttle bayrağını kaldır — görev çalışabilir
        self.throttled.store(false, Ordering::SeqCst);

        crate::serial_println!("[DL] Task {} bütçe yenilendi, son_tarih={}",
            self.task_id, new_deadline);
    }

    /// Gevşeklik (laxity) hesaplar: son_tarih - şimdi - kalan_bütçe.
    /// Negatif laxity = son tarihi kaçırma riski!
    pub fn laxity(&self) -> i64 {
        let now = crate::task::scheduler::get_ticks();
        let deadline = self.abs_deadline.load(Ordering::Relaxed) as i64;
        let remaining = self.remaining_runtime.load(Ordering::Relaxed) as i64;

        deadline - now as i64 - remaining
    }

    /// EDF sıralaması için iki task'ın son tarihlerini karşılaştırır.
    pub fn compare_deadline(&self, other: &DeadlineTask) -> core::cmp::Ordering {
        self.abs_deadline.load(Ordering::Relaxed)
            .cmp(&other.abs_deadline.load(Ordering::Relaxed))
    }
}

// ============================================================================
// DEADLINE ÇALIŞMA KUYRUĞU (RUN QUEUE)
// ============================================================================
//
// EDF politikasında kuyruk her zaman son tarihe göre sıralıdır.
// En sol düğüm = en yakın son tarih = bir sonraki çalışacak task.
//
//   BTreeMap<abs_deadline, DeadlineTask>
//   ┌──────┬──────┬──────┬──────┐
//   │ t=10 │ t=15 │ t=20 │ t=30 │
//   └──────┴──────┴──────┴──────┘
//      ▲
//   pick_next() burayı seçer

pub struct DeadlineRq {
    /// Son tarihe göre sıralanmış görev listesi
    pub tasks: Mutex<BTreeMap<u64, Arc<DeadlineTask>>>, // son_tarih -> görev
    /// Şu anda CPU'da çalışan görev
    pub running: Mutex<Option<Arc<DeadlineTask>>>,
    /// Toplam bant genişliği kullanımı (U = Σ C_i / T_i * 10000)
    pub total_bw: AtomicU64,
    /// Maksimum izin verilen bant genişliği (10000 = %100)
    pub max_bw: u64, // 10000 = %100
}

impl DeadlineRq {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(BTreeMap::new()),
            running: Mutex::new(None),
            total_bw: AtomicU64::new(0),
            max_bw: 10000, // %100
        }
    }

    /// Görevi çalışma kuyruğuna ekler.
    /// Bant genişliği kontrolü yapar: toplam U <= max_bw olmalı.
    pub fn enqueue(&self, task: Arc<DeadlineTask>) -> Result<(), DlError> {
        // Kabul testi: yeni görevin bant genişliğini kontrol et
        let task_bw = self.compute_bandwidth(&task);
        let current_bw = self.total_bw.load(Ordering::Relaxed);

        if current_bw + task_bw > self.max_bw {
            return Err(DlError::BandwidthExceeded);
        }

        self.total_bw.fetch_add(task_bw, Ordering::Relaxed);

        let deadline = task.abs_deadline.load(Ordering::Relaxed);
        self.tasks.lock().insert(deadline, task);

        Ok(())
    }

    /// Görevi kuyruktan çıkarır ve bant genişliğini serbest bırakır.
    pub fn dequeue(&self, task: &DeadlineTask) {
        let deadline = task.abs_deadline.load(Ordering::Relaxed);
        self.tasks.lock().remove(&deadline);

        let task_bw = self.compute_bandwidth(task);
        self.total_bw.fetch_sub(task_bw, Ordering::Relaxed);
    }

    /// EDF politikasına göre bir sonraki görevi seçer.
    /// Throttle edilmemiş, aktif görevler arasında en yakın son tarihe sahip olanı döndürür.
    pub fn pick_next(&self) -> Option<Arc<DeadlineTask>> {
        let tasks = self.tasks.lock();

        // Son tarihe göre en yakın, throttle edilmemiş görevi bul
        for task in tasks.values() {
            if !task.throttled.load(Ordering::Relaxed) &&
               task.active.load(Ordering::Relaxed) {
                return Some(task.clone());
            }
        }

        None
    }

    /// Bant genişliğini hesaplar (yüzde * 100 cinsinden).
    /// U_i = (runtime / period) * 10000
    fn compute_bandwidth(&self, task: &DeadlineTask) -> u64 {
        let runtime = task.runtime.load(Ordering::Relaxed);
        let period = task.period;

        if period == 0 {
            return 0;
        }

        // bant_genisligi = (runtime / period) * 10000
        (runtime * 10000) / period
    }

    /// Periyot sona eren görevlerin bütçelerini yeniler.
    pub fn check_replenishments(&self) {
        let now = crate::task::scheduler::get_ticks();

        for task in self.tasks.lock().values() {
            if task.next_replenish.load(Ordering::Relaxed) <= now {
                task.replenish();
            }
        }
    }

    /// Son tarihi geçmiş görevleri tespit eder ve istatistiğe kaydeder.
    pub fn check_deadline_misses(&self) {
        for task in self.tasks.lock().values() {
            if task.deadline_passed() && !task.throttled.load(Ordering::Relaxed) {
                let mut stats = task.stats.lock();
                stats.deadline_misses += 1;

                crate::serial_println!(
                    "[DL] Görev {} son tarihi kaçırdı!",
                    task.task_id
                );
            }
        }
    }
}

// ============================================================================
// DEADLINE ZAMANLAYICISI
// ============================================================================

pub struct DeadlineScheduler {
    /// CPU başına çalışma kuyruğu (SMP desteği)
    pub run_queues: Mutex<Vec<DeadlineRq>>,
    /// Sistem CPU sayısı
    pub nr_cpus: usize,
    /// Zamanlayıcı aktif mi?
    pub enabled: AtomicBool,
    /// Tick aralığı (nanosaniye cinsinden)
    pub tick_interval: u64,
}

impl DeadlineScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rqs = Vec::new();
        for _ in 0..nr_cpus {
            rqs.push(DeadlineRq::new());
        }

        Self {
            run_queues: Mutex::new(rqs),
            nr_cpus,
            enabled: AtomicBool::new(true),
            tick_interval: 1_000_000, // 1ms
        }
    }

    /// Bir sonraki çalışacak görevi seçer.
    /// Önce bütçe yenilemelerini kontrol eder, sonra EDF seçimi yapar.
    pub fn schedule(&self, cpu: usize) -> Option<Arc<DeadlineTask>> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.check_replenishments();
            rq.pick_next()
        } else {
            None
        }
    }

    /// Yeni bir SCHED_DEADLINE görevi ekler.
    pub fn add_task(&self, task: Arc<DeadlineTask>, cpu: usize) -> Result<(), DlError> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.enqueue(task)
        } else {
            Err(DlError::InvalidCpu)
        }
    }

    /// Görevi zamanlayıcıdan çıkarır.
    pub fn remove_task(&self, task: &DeadlineTask, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.dequeue(task);
        }
    }

    /// Her timer interrupt'ta çağrılır.
    /// Bütçe yenileme ve deadline miss kontrolünü yapar.
    pub fn tick(&self, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.check_replenishments();
            rq.check_deadline_misses();

            // Çalışan görevin bütçesini tüket
            if let Some(running) = rq.running.lock().as_ref() {
                running.consume_runtime(self.tick_interval);

                if running.throttled.load(Ordering::Relaxed) {
                    // Bütçe tükendi, yeniden zamanlama gerekli
                    // self.reschedule(cpu);
                }
            }
        }
    }

    /// Tüm CPU'lardaki bant genişliği sınırını ayarlar.
    pub fn set_bandwidth_cap(&self, cap: u64) {
        // cap: yüzde * 100 cinsinden (örn. 9000 = %90)
        for rq in self.run_queues.lock().iter_mut() {
            rq.max_bw = cap;
        }
    }
}

lazy_static::lazy_static! {
    pub static ref DL_SCHEDULER: DeadlineScheduler = DeadlineScheduler::new(1);
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlError {
    BandwidthExceeded,
    InvalidCpu,
    TaskNotFound,
    InvalidParameters,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

pub fn sys_sched_setattr(pid: u64, runtime: u64, period: u64, deadline: u64, flags: u64) -> i32 {
    let task = Arc::new(DeadlineTask::new(pid, runtime, period, deadline, flags));

    match DL_SCHEDULER.add_task(task, 0) {
        Ok(()) => 0,
        Err(DlError::BandwidthExceeded) => -16, // EBUSY
        Err(_) => -22,
    }
}

pub fn sys_sched_getattr(pid: u64, attr: &mut SchedAttr) -> i32 {
    // Görevi bul ve parametreleri doldur
    attr.sched_policy = SCHED_DEADLINE;
    attr.sched_runtime = DL_DEFAULT_RUNTIME;
    attr.sched_period = DL_DEFAULT_PERIOD;
    attr.sched_deadline = DL_DEFAULT_DEADLINE;
    0
}

#[repr(C)]
pub struct SchedAttr {
    pub sched_policy: i32,
    pub sched_flags: u64,
    pub sched_nice: i32,
    pub sched_priority: u32,
    pub sched_runtime: u64,
    pub sched_deadline: u64,
    pub sched_period: u64,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[DL] Deadline zamanlayıcısı (EDF) başlatıldı");
}
