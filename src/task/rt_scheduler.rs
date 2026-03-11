//! # Gerçek Zamanlı Zamanlayıcı (Real-Time Scheduler)
//!
//! POSIX gerçek zamanlı zamanlama politikalarının uygulaması.
//! SCHED_FIFO ve SCHED_RR (Round-Robin) desteklenir.
//!
//! ## SCHED_FIFO — İlk Gelen İlk Çalışır
//!
//! ```text
//!  Öncelik 99 [T1] → CPU almadan bırakmaz!
//!             T1 tamamlanır veya engellenirse sonraki seçilir.
//!
//!  Öncelik 50 [T2, T3]  ← T2 önce geldi, T2 çalışır
//!                           T2 biter → T3 çalışır
//!
//!  Öncelik 1  [T4]
//!
//!  KURAL: Düşük öncelik asla yüksek öncelikli varken çalışmaz!
//! ```
//!
//! ## SCHED_RR — Round-Robin Gerçek Zamanlı
//!
//! ```text
//!  Öncelik 99 [T1, T2, T3]
//!
//!  Zaman:  0──10──20──30──40──50──60─▶
//!              T1   T2   T3   T1   T2
//!           ────┤────┤────┤────┤────┤
//!                     ↑
//!             Her görev 10ms (RR_DEFAULT_TIMESLICE) çalışır,
//!             sonra aynı öncelikte kuyruğun sonuna gider.
//! ```
//!
//! ## SCHED_NORMAL vs RT Karşılaştırması
//!
//! ```text
//!  ┌──────────────┬──────────────────┬────────────────────────┐
//!  │ Politika     │ Öncelik          │ Preemption             │
//!  ├──────────────┼──────────────────┼────────────────────────┤
//!  │ SCHED_NORMAL │ nice -20..+19    │ vruntime bazlı         │
//!  │ SCHED_FIFO   │ 1..99 (RT)       │ Sadece bloke/yield ile │
//!  │ SCHED_RR     │ 1..99 (RT)       │ Zaman dilimi dolunca   │
//!  │ SCHED_DL     │ EDF (deadline)   │ Son tarihe göre        │
//!  └──────────────┴──────────────────┴────────────────────────┘
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use super::task::{Task, TaskId, TaskState};

// ============================================================================
// GERÇEK ZAMANLI ZAMANLAMA SABİTLERİ
// ============================================================================

/// Minimum gerçek zamanlı öncelik (Linux: 1)
pub const RT_PRIO_MIN: i32 = 1;

/// Maksimum gerçek zamanlı öncelik (Linux: 99)
pub const RT_PRIO_MAX: i32 = 99;

/// SCHED_RR için varsayılan zaman dilimi (tick cinsinden)
/// Linux varsayılanı: 100ms (1000Hz'de genellikle 100 tick)
pub const RR_DEFAULT_TIMESLICE: u64 = 100;

/// SCHED_RR için maksimum zaman dilimi
pub const RR_MAX_TIMESLICE: u64 = 200;

/// SCHED_RR için minimum zaman dilimi
pub const RR_MIN_TIMESLICE: u64 = 10;

// ============================================================================
// ZAMANLAMA POLİTİKASI
// ============================================================================

/// Zamanlama politika türleri (Linux ile uyumlu sayısal kodlar)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SchedPolicy {
    /// Normal zamanlama (CFS benzeri, nice değerine göre)
    Normal = 0,
    /// İlk Giren İlk Çıkar gerçek zamanlı (bloke/yield'e kadar çalışır)
    Fifo = 1,
    /// Round-Robin gerçek zamanlı (zaman dilimi dolunca sıraya girer)
    RoundRobin = 2,
    /// Son tarih bazlı zamanlama (EDF — Earliest Deadline First)
    Deadline = 3,
    /// Boşta zamanlama (çok düşük öncelik, sadece CPU boşta iken çalışır)
    Idle = 4,
    /// Toplu işlem (CPU yoğun, etkileşim gecikme toleransı var)
    Batch = 5,
}

impl Default for SchedPolicy {
    fn default() -> Self {
        SchedPolicy::Normal
    }
}

/// Gerçek zamanlı zamanlama parametreleri (sched_param yapısına karşılık gelir)
#[derive(Debug, Clone, Copy)]
pub struct RtSchedParam {
    /// Gerçek zamanlı öncelik (1-99, yüksek = daha önemli)
    pub sched_priority: i32,
    /// SCHED_DEADLINE için: nanosaniye cinsinden çalışma bütçesi
    pub sched_runtime: u64,
    /// SCHED_DEADLINE için: nanosaniye cinsinden son tarih
    pub sched_deadline: u64,
    /// SCHED_DEADLINE için: nanosaniye cinsinden periyot
    pub sched_period: u64,
}

impl Default for RtSchedParam {
    fn default() -> Self {
        Self {
            sched_priority: 0,
            sched_runtime: 0,
            sched_deadline: 0,
            sched_period: 0,
        }
    }
}

// ============================================================================
// GERÇEK ZAMANLI GÖREV BİLGİSİ
// ============================================================================

/// Gerçek zamanlı görevin izleme bilgisi
#[derive(Debug, Clone)]
pub struct RtTaskInfo {
    pub task_id: TaskId,
    pub policy: SchedPolicy,
    pub priority: i32,
    /// Kalan zaman dilimi (SCHED_RR için; her tick azalır)
    pub time_slice: u64,
    /// Toplam zaman dilimi (SCHED_RR için başlangıç değeri)
    pub total_timeslice: u64,
    /// CPU yakınlık maskesi (hangi CPU'larda çalışabilir)
    pub affinity: u64,
    /// Bu görev gerçek zamanlı mı? (FIFO veya RR politikası)
    pub is_rt: bool,
}

impl RtTaskInfo {
    pub fn new(task_id: TaskId) -> Self {
        Self {
            task_id,
            policy: SchedPolicy::Normal,
            priority: 0,
            time_slice: RR_DEFAULT_TIMESLICE,
            total_timeslice: RR_DEFAULT_TIMESLICE,
            affinity: 0xFFFFFFFFFFFFFFFF, // Tüm CPU'lar
            is_rt: false,
        }
    }

    pub fn with_rt(task_id: TaskId, policy: SchedPolicy, priority: i32) -> Self {
        let is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;
        let time_slice = if policy == SchedPolicy::RoundRobin {
            Self::calculate_timeslice(priority)
        } else {
            u64::MAX // FIFO: bloke veya yield edilene kadar çalışır
        };

        Self {
            task_id,
            policy,
            priority,
            time_slice,
            total_timeslice: time_slice,
            affinity: 0xFFFFFFFFFFFFFFFF,
            is_rt,
        }
    }

    /// Önceliğe göre zaman dilimini hesaplar.
    /// Yüksek öncelik = daha uzun zaman dilimi
    fn calculate_timeslice(priority: i32) -> u64 {
        let normalized = (priority as f64 / RT_PRIO_MAX as f64).clamp(0.0, 1.0);
        let slice =
            RR_MIN_TIMESLICE as f64 + normalized * (RR_MAX_TIMESLICE - RR_MIN_TIMESLICE) as f64;
        slice as u64
    }

    /// Zaman dilimini sıfırlar (görev yeniden zamanlandığında çağrılır).
    pub fn reset_timeslice(&mut self) {
        self.time_slice = self.total_timeslice;
    }

    /// Zaman dilimini bir tick azaltır.
    /// true döndürürse zaman dilimi doldu; yeniden zamanlama gerekli.
    pub fn tick(&mut self) -> bool {
        if self.policy == SchedPolicy::RoundRobin && self.time_slice > 0 {
            self.time_slice = self.time_slice.saturating_sub(1);
            return self.time_slice == 0;
        }
        false
    }
}

// ============================================================================
// GERÇEK ZAMANLI ÇALIŞMA KUYRUĞU
// ============================================================================

/// Gerçek zamanlı çalışma kuyruğu (önceliğe göre sıralı)
///
/// RT görevler öncelik kovalarına (1-99) kaydedilir.
/// Yüksek öncelikli görevler her zaman düşük öncelikten önce çalışır.
/// Aynı öncelik içinde:
/// - SCHED_FIFO: İlk gelen ilk çalışır (FIFO sırası)
/// - SCHED_RR: Zaman dilimleriyle döngüsel (round-robin)
pub struct RtRunQueue {
    /// Öncelik kovaları: öncelik → görev listesi
    /// 99 en yüksek, 1 en düşük RT önceliğidir
    queues: BTreeMap<i32, Vec<Box<Task>>>,
    /// Görev ID → RT bilgi eşlemesi
    task_info: BTreeMap<TaskId, RtTaskInfo>,
    /// RT görev sayısı
    rt_count: AtomicU64,
    /// Çalışabilir görev bulunan en yüksek öncelik
    highest_prio: AtomicU64,
    /// RT kısıtlama: bant genişliği kontrolü (CPU zamanının en fazla %95'ini kullanabilir)
    rt_runtime: AtomicU64,
    rt_period: AtomicU64,
    rt_runtime_enabled: AtomicBool,
}

impl RtRunQueue {
    pub fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
            task_info: BTreeMap::new(),
            rt_count: AtomicU64::new(0),
            highest_prio: AtomicU64::new(0),
            rt_runtime: AtomicU64::new(950_000_000), // 1s'nin %95'i
            rt_period: AtomicU64::new(1_000_000_000), // 1s
            rt_runtime_enabled: AtomicBool::new(true),
        }
    }

    /// RT çalışma kuyruğuna görev ekler.
    pub fn enqueue(&mut self, task: Box<Task>) {
        let task_id = task.hot.id;
        let info = self
            .task_info
            .entry(task_id)
            .or_insert_with(|| RtTaskInfo::new(task_id));

        let priority = info.priority;
        let is_rt = info.is_rt;

        // Uygun öncelik kuyruğuna ekle
        let queue = self.queues.entry(priority).or_insert_with(Vec::new);
        queue.push(task);

        // Gerekirse en yüksek önceliği güncelle
        if is_rt && priority as u64 > self.highest_prio.load(Ordering::Relaxed) {
            self.highest_prio.store(priority as u64, Ordering::Relaxed);
        }

        if is_rt {
            self.rt_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// RT çalışma kuyruğundan görev çıkarır.
    pub fn dequeue(&mut self, task_id: TaskId) -> Option<Box<Task>> {
        let info = self.task_info.get(&task_id)?;
        let priority = info.priority;

        if let Some(queue) = self.queues.get_mut(&priority) {
            // Görevi bul ve kaldır
            for i in 0..queue.len() {
                if queue[i].hot.id == task_id {
                    let task = queue.remove(i);
                    if info.is_rt {
                        self.rt_count.fetch_sub(1, Ordering::Relaxed);
                    }
                    // Kuyruk boşaldıysa en yüksek önceliği güncelle
                    if queue.is_empty() {
                        self.queues.remove(&priority);
                        self.update_highest_prio();
                    }
                    return Some(task);
                }
            }
        }
        None
    }

    /// Bir sonraki çalışacak görevi seçer.
    /// En yüksek öncelikli RT görevi döndürür; RT görev yoksa None.
    pub fn pick_next(&mut self) -> Option<Box<Task>> {
        // Çalışabilir görev bulunan en yüksek önceliği bul
        let highest = self.find_highest_prio();
        if highest == 0 {
            return None;
        }

        if let Some(queue) = self.queues.get_mut(&highest) {
            if !queue.is_empty() {
                // SCHED_RR: kuyruğu döndür (round-robin)
                // SCHED_FIFO: önden al (FIFO sırası)
                let task = queue.remove(0);

                // RR için yeniden kuyruğa alma gerekir mi kontrol et
                if let Some(info) = self.task_info.get_mut(&task.hot.id) {
                    if info.policy == SchedPolicy::RoundRobin {
                        // Zaman dilimi dolunca görev yeniden eklenir
                        info.reset_timeslice();
                    }
                }

                return Some(task);
            }
        }
        None
    }

    /// Çalışabilir görev bulunan en yüksek önceliği bulur.
    fn find_highest_prio(&self) -> i32 {
        // BTreeMap sıralı iterasyon yapar; boş olmayan en yüksek anahtarı al
        self.queues
            .iter()
            .rev()
            .find(|(_, q)| !q.is_empty())
            .map(|(p, _)| *p)
            .unwrap_or(0)
    }

    /// En yüksek öncelik izleme değerini günceller.
    fn update_highest_prio(&mut self) {
        let highest = self.find_highest_prio();
        self.highest_prio.store(highest as u64, Ordering::Relaxed);
    }

    /// RT görev sayısını döndürür.
    pub fn rt_task_count(&self) -> u64 {
        self.rt_count.load(Ordering::Relaxed)
    }

    /// Çalışabilir RT görev var mı kontrol eder.
    pub fn has_rt_tasks(&self) -> bool {
        self.rt_count.load(Ordering::Relaxed) > 0
    }

    /// Bir görev için zamanlama parametrelerini alır/ayarlar.
    pub fn set_sched_param(&mut self, task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
        let info = self
            .task_info
            .entry(task_id)
            .or_insert_with(|| RtTaskInfo::new(task_id));

        let old_is_rt = info.is_rt;

        info.policy = policy;
        info.priority = param.sched_priority.clamp(0, RT_PRIO_MAX);
        info.is_rt = policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin;

        if policy == SchedPolicy::RoundRobin {
            info.total_timeslice = RtTaskInfo::calculate_timeslice(info.priority);
            info.time_slice = info.total_timeslice;
        } else {
            info.total_timeslice = u64::MAX;
            info.time_slice = u64::MAX;
        }

        // RT sayısını güncelle (politika değişikliği olabilir)
        if old_is_rt && !info.is_rt {
            self.rt_count.fetch_sub(1, Ordering::Relaxed);
        } else if !old_is_rt && info.is_rt {
            self.rt_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Zamanlama parametrelerini alır.
    pub fn get_sched_param(&self, task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
        self.task_info.get(&task_id).map(|info| {
            (
                info.policy,
                RtSchedParam {
                    sched_priority: info.priority,
                    sched_runtime: 0,
                    sched_deadline: 0,
                    sched_period: 0,
                },
            )
        })
    }

    /// Tick: çalışan görevin zaman dilimini azaltır.
    /// true döndürürse preemption gerekli.
    pub fn tick(&mut self, task_id: TaskId) -> bool {
        if let Some(info) = self.task_info.get_mut(&task_id) {
            if info.tick() {
                // RR görevin zaman dilimi doldu
                return true;
            }
        }
        false
    }

    /// Zaman dilimi dolunca RR görevini yeniden kuyruğa ekler.
    pub fn reenqueue_rr(&mut self, task: Box<Task>) {
        let task_id = task.hot.id;
        if let Some(info) = self.task_info.get_mut(&task_id) {
            if info.policy == SchedPolicy::RoundRobin {
                info.reset_timeslice();
                self.enqueue(task);
            }
        }
    }

    /// RT bant genişliği kısıtlamasını ayarlar (throttling).
    pub fn set_rt_bandwidth(&mut self, runtime: u64, period: u64) {
        self.rt_runtime.store(runtime, Ordering::Relaxed);
        self.rt_period.store(period, Ordering::Relaxed);
    }

    /// RT bant genişliği kısıtlamasını etkinleştirir/devre dışı bırakır.
    pub fn set_rt_throttling(&mut self, enabled: bool) {
        self.rt_runtime_enabled.store(enabled, Ordering::Relaxed);
    }
}

impl Default for RtRunQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL RT ZAMANLAYICI DURUMU
// ============================================================================

lazy_static::lazy_static! {
    /// Global RT çalışma kuyruğu (spin mutex korumalı)
    static ref RT_RUNQUEUE: Mutex<RtRunQueue> = Mutex::new(RtRunQueue::new());
}

// ============================================================================
// GENEL API
// ============================================================================

/// RT zamanlayıcısını başlatır.
pub fn init() {
    crate::serial_println!("[RT-SCHED] Gerçek Zamanlı Zamanlayıcı başlatıldı");
}

/// Çalışabilir RT görev var mı kontrol eder.
pub fn has_rt_tasks() -> bool {
    RT_RUNQUEUE.lock().has_rt_tasks()
}

/// RT görev sayısını döndürür.
pub fn rt_task_count() -> u64 {
    RT_RUNQUEUE.lock().rt_task_count()
}

/// RT görevini kuyruğa ekler.
pub fn enqueue_rt_task(task: Box<Task>) {
    RT_RUNQUEUE.lock().enqueue(task);
}

/// RT görevini kuyruktan çıkarır.
pub fn dequeue_rt_task(task_id: TaskId) -> Option<Box<Task>> {
    RT_RUNQUEUE.lock().dequeue(task_id)
}

/// Bir sonraki RT görevi seçer.
/// Çalışabilir RT görev yoksa None döndürür.
pub fn pick_next_rt_task() -> Option<Box<Task>> {
    RT_RUNQUEUE.lock().pick_next()
}

/// Görev için zamanlama parametrelerini ayarlar.
pub fn set_sched_param(task_id: TaskId, policy: SchedPolicy, param: &RtSchedParam) {
    RT_RUNQUEUE.lock().set_sched_param(task_id, policy, param);
}

/// Görev için zamanlama parametrelerini alır.
pub fn get_sched_param(task_id: TaskId) -> Option<(SchedPolicy, RtSchedParam)> {
    RT_RUNQUEUE.lock().get_sched_param(task_id)
}

/// Tick: çalışan RT görevinin zaman dilimini işler.
/// true döndürürse preemption gerekli.
pub fn rt_tick(task_id: TaskId) -> bool {
    RT_RUNQUEUE.lock().tick(task_id)
}

/// Zaman dilimi dolunca RR görevini yeniden kuyruğa ekler.
pub fn reenqueue_rr_task(task: Box<Task>) {
    RT_RUNQUEUE.lock().reenqueue_rr(task);
}

/// RT bant genişliği sınırlarını ayarlar.
pub fn set_rt_bandwidth(runtime: u64, period: u64) {
    RT_RUNQUEUE.lock().set_rt_bandwidth(runtime, period);
}

/// RT bant genişliği kısıtlamasını etkinleştirir/devre dışı bırakır.
pub fn set_rt_throttling(enabled: bool) {
    RT_RUNQUEUE.lock().set_rt_throttling(enabled);
}

/// Görevin gerçek zamanlı olup olmadığını kontrol eder.
pub fn is_rt_task(task_id: TaskId) -> bool {
    RT_RUNQUEUE
        .lock()
        .task_info
        .get(&task_id)
        .map(|info| info.is_rt)
        .unwrap_or(false)
}

/// Görevin önceliğini alır (RT veya normal).
pub fn get_task_priority(task_id: TaskId) -> i32 {
    RT_RUNQUEUE
        .lock()
        .task_info
        .get(&task_id)
        .map(|info| info.priority)
        .unwrap_or(0)
}

/// Görevin zamanlama politikasını alır.
pub fn get_task_policy(task_id: TaskId) -> SchedPolicy {
    RT_RUNQUEUE
        .lock()
        .task_info
        .get(&task_id)
        .map(|info| info.policy)
        .unwrap_or(SchedPolicy::Normal)
}

/// Mevcut RT görevini gönüllü olarak bırakır (yield).
/// SCHED_FIFO: aynı öncelik kuyruğunun sonuna gider.
/// SCHED_RR: zaman dilimini bırakmakla aynı etkiyi yapar.
pub fn yield_rt_task(task: Box<Task>) {
    let task_id = task.hot.id;
    let mut rq = RT_RUNQUEUE.lock();

    if let Some(info) = rq.task_info.get(&task_id) {
        if info.is_rt {
            // Öncelik kuyruğunun sonuna yeniden ekle
            rq.enqueue(task);
        }
    }
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// sched_setscheduler(2) sistem çağrısı uygulaması.
pub fn sys_sched_setscheduler(task_id: TaskId, policy: i32, param: &RtSchedParam) -> i32 {
    let policy = match policy as u8 {
        1 => SchedPolicy::Fifo,
        2 => SchedPolicy::RoundRobin,
        3 => SchedPolicy::Deadline,
        0 | _ => SchedPolicy::Normal,
    };

    // Öncelik doğrulama: RT politikaları için 1-99 aralığı gereklidir
    if policy == SchedPolicy::Fifo || policy == SchedPolicy::RoundRobin {
        if param.sched_priority < RT_PRIO_MIN || param.sched_priority > RT_PRIO_MAX {
            return -22; // EINVAL
        }
    } else if param.sched_priority != 0 {
        return -22; // EINVAL
    }

    set_sched_param(task_id, policy, param);
    0 // Başarı
}

/// sched_getscheduler(2) sistem çağrısı uygulaması.
pub fn sys_sched_getscheduler(task_id: TaskId) -> i32 {
    get_task_policy(task_id) as i32
}

/// sched_setparam(2) sistem çağrısı uygulaması.
pub fn sys_sched_setparam(task_id: TaskId, param: &RtSchedParam) -> i32 {
    let policy = get_task_policy(task_id);
    set_sched_param(task_id, policy, param);
    0
}

/// sched_getparam(2) sistem çağrısı uygulaması.
pub fn sys_sched_getparam(task_id: TaskId) -> Option<RtSchedParam> {
    get_sched_param(task_id).map(|(_, p)| p)
}

/// sched_yield(2) sistem çağrısı uygulaması.
pub fn sys_sched_yield() {
    // Mevcut görev bağlamından çağrılmalıdır.
    // Gerçek yield ana zamanlayıcı tarafından işlenir.
}

/// sched_get_priority_max(2) sistem çağrısı uygulaması.
pub fn sys_sched_get_priority_max(policy: i32) -> i32 {
    match policy as u8 {
        1 | 2 => RT_PRIO_MAX, // SCHED_FIFO, SCHED_RR
        _ => 0,
    }
}

/// sched_get_priority_min(2) sistem çağrısı uygulaması.
pub fn sys_sched_get_priority_min(policy: i32) -> i32 {
    match policy as u8 {
        1 | 2 => RT_PRIO_MIN, // SCHED_FIFO, SCHED_RR
        _ => 0,
    }
}

/// sched_rr_get_interval(2) sistem çağrısı uygulaması.
/// RR görevinin zaman dilimini nanosaniye cinsinden döndürür.
pub fn sys_sched_rr_get_interval(task_id: TaskId) -> u64 {
    RT_RUNQUEUE
        .lock()
        .task_info
        .get(&task_id)
        .map(|info| info.total_timeslice)
        .unwrap_or(RR_DEFAULT_TIMESLICE)
}
