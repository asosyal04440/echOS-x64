//! # CFS (Completely Fair Scheduler - Tamamen Adil Zamanlayıcı)
//!
//! Linux çekirdeğinden ilham alınan, sanal çalışma zamanı (vruntime) tabanlı
//! adil zamanlama algoritması.
//!
//! ## Temel Fikir
//! Her task bir "sanal saat"e (vruntime) sahiptir. Zamanlayıcı her zaman
//! en düşük vruntime değerine sahip task'ı çalıştırır. Bu sayede tüm
//! task'lar CPU zamanından "eşit" pay alır.
//!
//! ## CFS Red-Black Tree (Kırmızı-Siyah Ağaç) Yapısı
//!
//! ```text
//!                    [vruntime=50]  <-- En yüksek öncelik (kök)
//!                   /              \
//!          [vruntime=30]        [vruntime=80]
//!          /          \
//!  [vruntime=10]  [vruntime=40]
//!       ^
//!  pick_next() bu task'ı seçer (en sol yaprak = en küçük vruntime)
//! ```
//!
//! ## vruntime Hesaplama
//! ```
//! vruntime_delta = (gercek_sure * NICE_0_WEIGHT) / task_weight
//! ```
//! Ağır (yüksek öncelikli) task'lar daha az vruntime biriktirerek
//! daha sık seçilir.
//!
//! ## nice Değeri ve Ağırlık İlişkisi
//! ```text
//! nice -20  ->  weight ~3121  (en yüksek öncelik)
//! nice   0  ->  weight  1024  (varsayılan)
//! nice +19  ->  weight   15   (en düşük öncelik)
//! ```
//! Her nice seviyesi ağırlığı yaklaşık %25 değiştirir.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// CFS SABİTLERİ
// ============================================================================

/// Varsayılan zaman dilimi (mikrosaniye cinsinden)
pub const CFS_DEFAULT_SLICE: u64 = 1_000_000; // 1ms
/// Minimum granülarite — bir task bu süreden az çalışmaz
pub const CFS_MIN_GRANULARITY: u64 = 1_000_000; // 1ms
/// Uyandırma granülaritesi — yeni uyanan task'ın preempt edebilmesi için minimum vruntime farkı
pub const CFS_WAKEUP_GRANULARITY: u64 = 1_000_000;
/// nice=0 için referans ağırlık (Linux'ta aynı değer kullanılır)
pub const CFS_NICE_0_WEIGHT: u64 = 1024;
/// Yük ortalaması periyodu (PELT algoritması için)
pub const CFS_LOAD_AVG_PERIOD: u64 = 32;
/// PELT yarı ömrü — yük ortalamasının %50'ye düşmesi için gereken ms
pub const CFS_PELT_HALF_LIFE: u64 = 32; // 32ms

/// nice değerinden ağırlık hesaplar.
///
/// Her nice seviyesi ağırlığı yaklaşık %25 artırır veya azaltır.
pub fn nice_to_weight(nice: i32) -> u64 {
    let weight = CFS_NICE_0_WEIGHT as i64;
    let delta = nice as i64;

    // Her nice seviyesi ağırlığı ~%25 oranında değiştirir
    let factor = 1.25_f64.powi(delta.abs() as i32);

    if delta > 0 {
        (weight as f64 / factor) as u64
    } else {
        (weight as f64 * factor) as u64
    }
}

/// Gerçek süreyi sanal çalışma zamanına (vruntime) dönüştürür.
///
/// Formül: vruntime_delta = (delta * NICE_0_WEIGHT) / task_weight
/// Ağır task'lar daha az vruntime biriktirerek daha sık seçilir.
pub fn weight_to_vruntime(delta: u64, weight: u64) -> u64 {
    if weight == 0 {
        return delta;
    }
    (delta * CFS_NICE_0_WEIGHT) / weight
}

// ============================================================================
// CFS TASK (GÖREVİ)
// ============================================================================

#[derive(Clone, Debug)]
pub struct CfsTask {
    /// Görevin benzersiz kimlik numarası
    pub task_id: u64,
    /// nice değeri (-20 ile +19 arası; negatif = yüksek öncelik)
    pub nice: AtomicI64,
    /// Zamanlayıcı ağırlığı (nice değerinden türetilir)
    pub weight: AtomicU64,
    /// Sanal çalışma zamanı — CFS'in çekirdek değeri, ağaçta sıralama kriteri
    pub vruntime: AtomicU64,
    /// Toplam gerçek çalışma süresi (istatistik)
    pub runtime: AtomicU64,
    /// Bu task'a atanan zaman dilimi (time slice)
    pub slice: AtomicU64,
    /// Şu anda CPU'da çalışıyor mu?
    pub running: AtomicBool,
    /// Çalışma kuyruğunda (run queue) mevcut mu?
    pub on_rq: AtomicBool,
    /// Kuyruğa eklendiği zamanın tick değeri
    pub enqueue_time: AtomicU64,
    /// PELT yük ortalaması (Per-Entity Load Tracking)
    pub load_avg: AtomicU64,
    /// PELT kullanım ortalaması
    pub util_avg: AtomicU64,
    /// Ayrıntılı istatistikler (bekleme süresi, migrasyon sayısı vb.)
    pub stats: Mutex<CfsStats>,
}

#[derive(Clone, Debug, Default)]
pub struct CfsStats {
    pub wait_start: u64,
    pub wait_max: u64,
    pub wait_count: u64,
    pub wait_sum: u64,
    pub iowait_count: u64,
    pub iowait_sum: u64,
    pub slices: u64,
    pub migrations: u64,
}

impl CfsTask {
    pub fn new(task_id: u64, nice: i32) -> Self {
        Self {
            task_id,
            nice: AtomicI64::new(nice as i64),
            weight: AtomicU64::new(nice_to_weight(nice)),
            vruntime: AtomicU64::new(0),
            runtime: AtomicU64::new(0),
            slice: AtomicU64::new(CFS_DEFAULT_SLICE),
            running: AtomicBool::new(false),
            on_rq: AtomicBool::new(false),
            enqueue_time: AtomicU64::new(0),
            load_avg: AtomicU64::new(0),
            util_avg: AtomicU64::new(0),
            stats: Mutex::new(CfsStats::default()),
        }
    }

    /// nice değerini günceller ve ağırlığı yeniden hesaplar.
    pub fn set_nice(&self, nice: i32) {
        self.nice.store(nice as i64, Ordering::SeqCst);
        self.weight.store(nice_to_weight(nice), Ordering::SeqCst);
    }

    /// Çalışma sonrası vruntime'ı günceller.
    /// delta: gerçek çalışma süresi (nanosaniye)
    pub fn update_vruntime(&self, delta: u64) {
        let weight = self.weight.load(Ordering::Relaxed);
        let vruntime_delta = weight_to_vruntime(delta, weight);
        self.vruntime.fetch_add(vruntime_delta, Ordering::Relaxed);
        self.runtime.fetch_add(delta, Ordering::Relaxed);
    }

    /// Ağırlığa göre zaman dilimini hesaplar.
    /// Yüksek ağırlıklı task'lar daha uzun dilim alır.
    pub fn calc_slice(&self, total_weight: u64, nr_running: u64) -> u64 {
        if nr_running == 0 {
            return CFS_DEFAULT_SLICE;
        }

        let weight = self.weight.load(Ordering::Relaxed);
        let slice = (weight * CFS_DEFAULT_SLICE * nr_running) / total_weight;

        slice.max(CFS_MIN_GRANULARITY)
    }

    /// Task'ın çalışmaya uygun olup olmadığını kontrol eder.
    /// min_vruntime'dan büyük vruntime'a sahip task'lar bekletilir.
    pub fn is_eligible(&self, min_vruntime: u64) -> bool {
        self.vruntime.load(Ordering::Relaxed) <= min_vruntime
    }
}

// ============================================================================
// CFS ÇALIŞMA KUYRUĞU (RUN QUEUE)
// ============================================================================
//
// CFS run queue'su kavramsal olarak bir Red-Black Tree'dir.
// Burada BTreeMap ile simüle edilmiştir (key = vruntime).
//
// Görsel:
//   ┌──────────────────────────────────────────┐
//   │  CfsRq (Çalışma Kuyruğu)                │
//   │                                          │
//   │  BTreeMap<vruntime, CfsTask>             │
//   │  ┌────┬────┬────┬────┬────┐             │
//   │  │ 10 │ 30 │ 50 │ 80 │100 │  <-- sol   │
//   │  └────┴────┴────┴────┴────┘  en düşük  │
//   │    ▲                                     │
//   │  pick_next() burayı seçer                │
//   └──────────────────────────────────────────┘

pub struct CfsRq {
    /// vruntime'a göre sıralanmış task listesi (Red-Black Tree simülasyonu)
    pub tasks: Mutex<BTreeMap<u64, Arc<CfsTask>>>, // vruntime -> task
    /// Kuyrukta en küçük vruntime değeri — yeni task'lar buna göre ayarlanır
    pub min_vruntime: AtomicU64,
    /// Kuyruktaki tüm task'ların toplam ağırlığı (zaman dilimi hesabı için)
    pub total_weight: AtomicU64,
    /// Kuyruktaki çalışabilir task sayısı
    pub nr_running: AtomicU32,
    /// Şu an CPU'da çalışan task
    pub curr: Mutex<Option<Arc<CfsTask>>>,
    /// PELT yük ortalaması (tüm kuyruk)
    pub load_avg: AtomicU64,
    /// PELT kullanım ortalaması (tüm kuyruk)
    pub util_avg: AtomicU64,
    /// Monoton artan mantıksal saat
    pub clock: AtomicU64,
}

impl CfsRq {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(BTreeMap::new()),
            min_vruntime: AtomicU64::new(0),
            total_weight: AtomicU64::new(0),
            nr_running: AtomicU32::new(0),
            curr: Mutex::new(None),
            load_avg: AtomicU64::new(0),
            util_avg: AtomicU64::new(0),
            clock: AtomicU64::new(0),
        }
    }

    /// Task'ı çalışma kuyruğuna ekler.
    /// Yeni veya uyuyan task'lar min_vruntime'a sıfırlanır; böylece
    /// çok uzun süre uyuyan task'lar aniden geçmişe dönmez.
    pub fn enqueue(&self, task: Arc<CfsTask>) {
        let vruntime = task.vruntime.load(Ordering::Relaxed);

        // vruntime en az min_vruntime kadar olmalıdır (fairness koruması)
        let min_vr = self.min_vruntime.load(Ordering::Relaxed);
        let adjusted_vr = vruntime.max(min_vr);

        task.vruntime.store(adjusted_vr, Ordering::SeqCst);
        task.on_rq.store(true, Ordering::SeqCst);
        task.enqueue_time.store(self.clock.load(Ordering::Relaxed), Ordering::SeqCst);

        self.tasks.lock().insert(adjusted_vr, task.clone());
        self.total_weight.fetch_add(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
        self.nr_running.fetch_add(1, Ordering::SeqCst);
    }

    /// Task'ı kuyruktan çıkarır (bloklanma veya sonlanma durumunda).
    pub fn dequeue(&self, task: &CfsTask) {
        let vruntime = task.vruntime.load(Ordering::Relaxed);

        self.tasks.lock().remove(&vruntime);
        self.total_weight.fetch_sub(task.weight.load(Ordering::Relaxed), Ordering::SeqCst);
        self.nr_running.fetch_sub(1, Ordering::SeqCst);
        task.on_rq.store(false, Ordering::SeqCst);
    }

    /// Bir sonraki çalıştırılacak task'ı seçer.
    /// Red-Black Tree'nin en sol yaprağı = en küçük vruntime = en çok hak kazanan task.
    pub fn pick_next(&self) -> Option<Arc<CfsTask>> {
        let tasks = self.tasks.lock();

        // En sol düğüm (en düşük vruntime) — O(log n) but effectively O(1) cached
        if let Some((&vruntime, task)) = tasks.iter().next() {
            // min_vruntime'ı güncelle — kuyruk saatini ileri taşır
            self.min_vruntime.store(vruntime, Ordering::SeqCst);

            task.running.store(true, Ordering::SeqCst);
            *self.curr.lock() = Some(task.clone());

            return Some(task.clone());
        }

        None
    }

    /// Önceki task'ı geri kuyruğa alır (preemption veya yield sonrası).
    pub fn put_prev(&self, task: &CfsTask) {
        task.running.store(false, Ordering::SeqCst);

        if task.on_rq.load(Ordering::Relaxed) {
            // Güncellenmiş vruntime ile yeniden ekle
            let vruntime = task.vruntime.load(Ordering::Relaxed);
            self.tasks.lock().insert(vruntime, Arc::new(task.clone()));
        }
    }

    /// Mantıksal saati günceller.
    pub fn update_clock(&self, now: u64) {
        self.clock.store(now, Ordering::SeqCst);
    }

    /// PELT (Per-Entity Load Tracking) yük ortalamasını günceller.
    /// Yük = ağırlık × delta_süre (exponential moving average ile düzeltilir).
    pub fn update_load_avg(&self, task: &CfsTask, delta: u64) {
        // Basitleştirilmiş PELT hesabı
        let weight = task.weight.load(Ordering::Relaxed);
        let contribution = weight * delta;

        task.load_avg.fetch_add(contribution, Ordering::Relaxed);
        self.load_avg.fetch_add(contribution, Ordering::Relaxed);
    }

    /// Uyanan bir task'ın mevcut task'ı preempt edip edemeyeceğini kontrol eder.
    /// CFS_WAKEUP_GRANULARITY'den fazla vruntime avantajı varsa preempt edilir.
    pub fn check_preempt_wakeup(&self, task: &CfsTask) -> bool {
        let curr = self.curr.lock();
        if let Some(curr_task) = curr.as_ref() {
            let curr_vr = curr_task.vruntime.load(Ordering::Relaxed);
            let task_vr = task.vruntime.load(Ordering::Relaxed);

            // Yeni task çok daha düşük vruntime'a sahipse preempt et
            if task_vr + CFS_WAKEUP_GRANULARITY < curr_vr {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// CFS ZAMANLAYICISI
// ============================================================================

pub struct CfsScheduler {
    /// CPU başına çalışma kuyruğu (SMP desteği)
    pub run_queues: Mutex<Vec<CfsRq>>,
    /// Sistem CPU sayısı
    pub nr_cpus: usize,
    /// Zamanlayıcı aktif mi?
    pub enabled: AtomicBool,
    /// Tick aralığı (nanosaniye cinsinden)
    pub tick_interval: u64,
    /// Yük dengeleme aralığı (load balancer ne sıklıkla çalışır)
    pub lb_interval: u64,
}

impl CfsScheduler {
    pub fn new(nr_cpus: usize) -> Self {
        let mut rqs = Vec::new();
        for _ in 0..nr_cpus {
            rqs.push(CfsRq::new());
        }

        Self {
            run_queues: Mutex::new(rqs),
            nr_cpus,
            enabled: AtomicBool::new(true),
            tick_interval: 1_000_000, // 1ms
            lb_interval: 100_000_000, // 100ms
        }
    }

    /// Belirtilen CPU için bir sonraki task'ı seçer.
    pub fn schedule(&self, cpu: usize) -> Option<Arc<CfsTask>> {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.pick_next()
        } else {
            None
        }
    }

    /// Zamanlayıcı tick'i işler.
    /// Her tick'te: saati güncelle, vruntime artır, zaman dilimi dolmuşsa yeniden zamanla.
    pub fn tick(&self, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            let now = crate::task::scheduler::get_ticks();
            rq.update_clock(now);

            // Çalışan task'ı güncelle
            let curr = rq.curr.lock();
            if let Some(task) = curr.as_ref() {
                task.update_vruntime(self.tick_interval);
                rq.update_load_avg(task, self.tick_interval);

                // Zaman dilimi doldu mu kontrol et
                let runtime = task.runtime.load(Ordering::Relaxed);
                let slice = task.slice.load(Ordering::Relaxed);

                if runtime >= slice {
                    // Yeniden zamanlama gerekli
                    drop(curr);
                    rq.put_prev(task);
                }
            }
        }
    }

    /// Task'ı kuyruğa ekler.
    pub fn enqueue(&self, task: Arc<CfsTask>, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.enqueue(task);
        }
    }

    /// Task'ı kuyruktan çıkarır.
    pub fn dequeue(&self, task: &CfsTask, cpu: usize) {
        let rqs = self.run_queues.lock();
        if let Some(rq) = rqs.get(cpu) {
            rq.dequeue(task);
        }
    }

    /// CPU'lar arası yük dengelemesi yapar.
    /// En meşgul ve en boş CPU'yu bularak görev migrasyonuna karar verir.
    pub fn load_balance(&self) {
        // En meşgul ve en boş CPU'yu bul
        let rqs = self.run_queues.lock();
        let mut busiest_load = 0u64;
        let mut busiest_cpu = 0;
        let mut idlest_load = u64::MAX;
        let mut idlest_cpu = 0;

        for (i, rq) in rqs.iter().enumerate() {
            let load = rq.load_avg.load(Ordering::Relaxed);

            if load > busiest_load {
                busiest_load = load;
                busiest_cpu = i;
            }

            if load < idlest_load {
                idlest_load = load;
                idlest_cpu = i;
            }
        }

        // Yük dengesizliği 2 kattan fazlaysa migrasyon yap
        if busiest_load > idlest_load * 2 {
            // Görev migrasyonu burada gerçekleştirilecek
        }
    }

    /// Task'ın nice değerini günceller, ağırlığı yeniden hesaplar.
    pub fn set_nice(&self, task: &CfsTask, nice: i32) {
        task.set_nice(nice);
    }
}

lazy_static::lazy_static! {
    pub static ref CFS_SCHEDULER: CfsScheduler = CfsScheduler::new(1);
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

pub fn sys_sched_setparam(pid: u64, nice: i32) -> i32 {
    // Task'ı bul ve nice değerini ayarla
    nice.clamp(-20, 19);
    0
}

pub fn sys_sched_getparam(pid: u64) -> i32 {
    0 // nice değerini döndür
}

pub fn sys_sched_yield() -> i32 {
    // Mevcut task'ı gönüllü olarak CPU'yu bırakmaya zorla
    0
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[CFS] Tamamen Adil Zamanlayıcı (CFS) başlatıldı");
}
