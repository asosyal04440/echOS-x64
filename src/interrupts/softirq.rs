//! # echOS Softirq + Tasklet Framework (Linux Bottom-Half)
//!
//! Linux `kernel/softirq.c` karşılığı.
//! Interrupt handler'ın hızlı kısmı (top-half) bittikten sonra
//! ertelenmiş ağır işler burada çalıştırılır.
//!
//! ## Top-Half / Bottom-Half Ayrımı
//!
//! ```text
//!  Donanım IRQ gelir
//!        │
//!        ▼
//!  ┌─────────────────────────────┐
//!  │   TOP-HALF (IRQ Handler)    │  ← Çok kısa! Interrupt kapalıyken çalışır
//!  │   • Donanımı onayla (ACK)   │
//!  │   • Kritik veriyi kaydet    │
//!  │   • raise_softirq() çağır   │
//!  │   • EOI gönder              │
//!  └────────────┬────────────────┘
//!               │  interrupt'lar yeniden açılır
//!               ▼
//!  ┌─────────────────────────────┐
//!  │  BOTTOM-HALF (Softirq)      │  ← Daha uzun, interrupt açıkken çalışır
//!  │   • do_softirq() → handler  │
//!  │   • Ağ paketi işle (NetRx)  │
//!  │   • Timer callback'leri     │
//!  │   • Tasklet'leri çalıştır   │
//!  └─────────────────────────────┘
//! ```
//!
//! ## Softirq Vektör Öncelik Sırası
//!
//! ```text
//!  0: HI       — Yüksek öncelikli tasklet (en önce işlenir)
//!  1: TIMER    — Hrtimer ve timer wheel callback'leri
//!  2: NET_TX   — Ağ gönderim kuyruğu boşaltma
//!  3: NET_RX   — Ağ alım paketi işleme
//!  4: BLOCK    — Blok I/O tamamlanma bildirimleri
//!  5: IRQ_POLL — NAPI/polling tabanlı IRQ işleme
//!  6: TASKLET  — Normal öncelikli tasklet kuyruğu
//!  7: SCHED    — Scheduler load balancing
//!  8: HRTIMER  — High-resolution timer son işlemler
//!  9: RCU      — Read-Copy-Update synchronization callbacks
//! ```
//!
//! ## Tasklet Yaşam Döngüsü
//!
//! ```text
//!  Tasklet::new(func, data)      — Tanımla (Idle durumunda)
//!       │
//!       ▼
//!  tasklet_schedule()            — TASKLET_VEC kuyruğuna ekle
//!  raise_softirq(Tasklet)        — SOFTIRQ_PENDING bit'i set et
//!       │
//!       ▼  (timer_interrupt veya ksoftirqd)
//!  do_softirq()                  — Bekleyen softirq'ları çalıştır
//!  tasklet_action()              — TASKLET_VEC kuyruğunu boşalt
//!       │
//!       ▼
//!  func(data) çalıştır           — Tasklet işlevi yürütülür
//!       │
//!       ▼
//!  state = Idle                  — Tekrar planlanmaya hazır
//! ```

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// Softirq Vektörleri (Linux: include/linux/interrupt.h)
//
// SOFTIRQ_PENDING atomic u32 değişkeni bir bitmask'tir.
// Her bit, ilgili vektörün bekleyip beklemediğini gösterir.
//   Bit 0 = Hi, Bit 1 = Timer, ..., Bit 9 = Rcu
//
// raise_softirq(vec): İlgili biti atomik olarak set eder (fetch_or).
// do_softirq():       Tüm set bitleri tek tek handler'larla çalıştırır.
//
// Bu tasarım, interrupt handler içindeki atomik bit-set işleminin
// yeterince hızlı (birkaç ns) olmasını sağlar — ağır iş ertelenir.
// ============================================================================

/// Softirq türleri — Linux ile birebir aynı sıralama
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SoftirqVec {
    Hi = 0,      // Yüksek öncelikli tasklet'ler
    Timer = 1,   // Timer callback'leri
    NetTx = 2,   // Ağ gönderimi
    NetRx = 3,   // Ağ alımı
    Block = 4,   // Blok I/O tamamlanma
    IrqPoll = 5, // IRQ polling mode
    Tasklet = 6, // Normal tasklet'ler
    Sched = 7,   // Scheduler
    Hrtimer = 8, // High-resolution timer
    Rcu = 9,     // RCU callback'leri
}

const NR_SOFTIRQS: usize = 10;

/// Softirq handler fonksiyonu
type SoftirqAction = fn();

/// Bekleyen softirq'lar — per-CPU dizisi (her CPU kendi softirq'larını işler)
static mut SOFTIRQ_PENDING: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];

/// Softirq handler tablosu
static SOFTIRQ_ACTIONS: Mutex<[Option<SoftirqAction>; NR_SOFTIRQS]> =
    Mutex::new([None; NR_SOFTIRQS]);

/// Softirq istatistikleri
static SOFTIRQ_COUNTS: [AtomicU64; NR_SOFTIRQS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; NR_SOFTIRQS]
};

/// Softirq handler'ı kaydet
pub fn open_softirq(vec: SoftirqVec, action: SoftirqAction) {
    SOFTIRQ_ACTIONS.lock()[vec as usize] = Some(action);
}

/// Softirq tetikle — interrupt context'ten çağrılır
#[inline]
pub fn raise_softirq(vec: SoftirqVec) {
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    unsafe {
        SOFTIRQ_PENDING[cpu_id].fetch_or(1 << (vec as u32), Ordering::Release);
    }
}

/// Bekleyen softirq var mı?
#[inline]
pub fn softirq_pending() -> bool {
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    unsafe { SOFTIRQ_PENDING[cpu_id].load(Ordering::Acquire) != 0 }
}

/// Tüm bekleyen softirq'ları çalıştır.
/// IRQ return path'te veya `ksoftirqd` thread'inden çağrılır.
///
/// **CRITICAL**: Bu fonksiyon interrupt'lar açıkken çalışır
/// (Linux: `__do_softirq()` interrupt'ları enable eder).
pub fn do_softirq() {
    // Max tekrar sayısı — sonsuz döngüyü engeller (Linux: MAX_SOFTIRQ_RESTART = 10)
    const MAX_RESTART: u32 = 10;

    // SoftIRQ bağlamını işaretle — preemptible() ve in_interrupt()
    // kontrolleri bu bit'e bakar. Softirq handler'ları içinden
    // yanlışlıkla schedule() çağrılmasını engeller.
    let _softirq_guard = crate::preempt::SoftIRQGuard::new();

    let mut restart_count = 0;

    loop {
        let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
        let pending = unsafe { SOFTIRQ_PENDING[cpu_id].swap(0, Ordering::AcqRel) };
        if pending == 0 {
            break;
        }

        let actions = SOFTIRQ_ACTIONS.lock();

        for i in 0..NR_SOFTIRQS {
            if (pending >> i) & 1 != 0 {
                if let Some(action) = actions[i] {
                    // Softirq handler'ı çalıştır
                    action();
                    SOFTIRQ_COUNTS[i].fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        drop(actions);

        restart_count += 1;
        if restart_count >= MAX_RESTART {
            // Çok fazla softirq — ksoftirqd'ye bırak
            if unsafe { SOFTIRQ_PENDING[cpu_id].load(Ordering::Relaxed) } != 0 {
                wake_ksoftirqd();
            }
            break;
        }

        // Yeni pending var mı kontrol et
        if unsafe { SOFTIRQ_PENDING[cpu_id].load(Ordering::Relaxed) } == 0 {
            break;
        }
    }
}

/// Softirq istatistiklerini raporla
pub fn print_softirq_stats() {
    crate::serial_println!("[SOFTIRQ] Statistics:");
    let names = [
        "HI", "TIMER", "NET_TX", "NET_RX", "BLOCK", "IRQ_POLL", "TASKLET", "SCHED", "HRTIMER",
        "RCU",
    ];
    for i in 0..NR_SOFTIRQS {
        let count = SOFTIRQ_COUNTS[i].load(Ordering::Relaxed);
        if count > 0 {
            crate::serial_println!("  {}: {}", names[i], count);
        }
    }
}

// ============================================================================
// Tasklet (Linux: include/linux/interrupt.h)
//
// Tasklet, tek seferlik çalıştırılmak üzere planlanabilen minimal
// bir bottom-half iş birimidir. Softirq'dan farkı:
//   • Aynı anda yalnızca bir CPU'da çalışır (mutual exclusion var)
//   • Dinamik olarak oluşturulabilir (softirq derleme zamanında sabit)
//   • İki öncelik: normal (TASKLET) ve yüksek (HI)
//
// Durum makinesi (state machine):
//   Idle ──► Scheduled ──► Running ──► Idle
//                 │            │
//                 │  (disable)  │  (enable)
//                 └────►──────►┘
//
// `count > 0` ise tasklet devre dışıdır: schedule edilse bile
// çalıştırılmaz, retry kuyruğuna geri konur.
// ============================================================================

/// Tasklet durumu
#[derive(Debug, Clone, Copy, PartialEq)]
enum TaskletState {
    Idle,
    Scheduled,
    Running,
}

/// Tasklet — tek seferlik ertelenmiş iş birimi
pub struct Tasklet {
    /// Çalıştırılacak fonksiyon
    func: fn(data: u64),
    /// Fonksiyona geçilecek veri
    data: u64,
    /// Mevcut durum
    state: AtomicU32,
    /// Disable sayacı (0 = enabled)
    count: AtomicU32,
}

impl Tasklet {
    /// Yeni tasklet oluştur
    pub const fn new(func: fn(u64), data: u64) -> Self {
        Self {
            func,
            data,
            state: AtomicU32::new(TaskletState::Idle as u32),
            count: AtomicU32::new(0),
        }
    }

    /// Tasklet'i devre dışı bırak (nested)
    pub fn disable(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    /// Tasklet'i etkinleştir
    pub fn enable(&self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Tasklet enabled mı?
    pub fn is_enabled(&self) -> bool {
        self.count.load(Ordering::SeqCst) == 0
    }
}

/// Global tasklet kuyrukları
static TASKLET_VEC: Mutex<VecDeque<&'static Tasklet>> = Mutex::new(VecDeque::new());
static TASKLET_HI_VEC: Mutex<VecDeque<&'static Tasklet>> = Mutex::new(VecDeque::new());

/// Normal öncelikli tasklet'i planla
pub fn tasklet_schedule(tasklet: &'static Tasklet) {
    // Zaten scheduled ise atla
    if tasklet
        .state
        .compare_exchange(
            TaskletState::Idle as u32,
            TaskletState::Scheduled as u32,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }

    TASKLET_VEC.lock().push_back(tasklet);
    raise_softirq(SoftirqVec::Tasklet);
}

/// Yüksek öncelikli tasklet'i planla
pub fn tasklet_hi_schedule(tasklet: &'static Tasklet) {
    if tasklet
        .state
        .compare_exchange(
            TaskletState::Idle as u32,
            TaskletState::Scheduled as u32,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }

    TASKLET_HI_VEC.lock().push_back(tasklet);
    raise_softirq(SoftirqVec::Hi);
}

/// Tasklet softirq handler — TASKLET_SOFTIRQ
pub fn tasklet_action() {
    let mut queue = TASKLET_VEC.lock();
    let mut retry = VecDeque::new();

    while let Some(tasklet) = queue.pop_front() {
        if !tasklet.is_enabled() {
            // Disabled — tekrar kuyruğa koy
            retry.push_back(tasklet);
            continue;
        }

        tasklet
            .state
            .store(TaskletState::Running as u32, Ordering::SeqCst);

        // Tasklet'i çalıştır
        (tasklet.func)(tasklet.data);

        tasklet
            .state
            .store(TaskletState::Idle as u32, Ordering::SeqCst);
    }

    // Disabled olanları geri koy
    while let Some(tasklet) = retry.pop_front() {
        queue.push_back(tasklet);
    }
}

/// Hi-priority tasklet softirq handler — HI_SOFTIRQ
pub fn tasklet_hi_action() {
    let mut queue = TASKLET_HI_VEC.lock();
    let mut retry = VecDeque::new();

    while let Some(tasklet) = queue.pop_front() {
        if !tasklet.is_enabled() {
            retry.push_back(tasklet);
            continue;
        }

        tasklet
            .state
            .store(TaskletState::Running as u32, Ordering::SeqCst);
        (tasklet.func)(tasklet.data);
        tasklet
            .state
            .store(TaskletState::Idle as u32, Ordering::SeqCst);
    }

    while let Some(tasklet) = retry.pop_front() {
        queue.push_back(tasklet);
    }
}

// ============================================================================
// ksoftirqd (Kernel Softirq Daemon)
//
// Linux'ta her CPU için bir `ksoftirqd/N` kernel thread'i bulunur.
// Çok fazla softirq birikirsek (MAX_RESTART aşılırsa) veya
// interrupt handler dışından softirq işlenmesi gerektiğinde
// bu daemon devreye girer.
//
// echOS'ta tek bir global ksoftirqd thread'i vardır.
// Thread, softirq_pending() kontrol ederek bekler.
// Softirq varsa do_softirq() çağırır, yoksa sleep(1) ile bekler.
//
// Avantajı: Uzun süren softirq işleri sistem responsiveness'ını
// bozmaz çünkü thread preemptible'dır (scheduler tarafından kesilebilir).
// ============================================================================

static KSOFTIRQD_STARTED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// ksoftirqd thread'ini başlat
pub fn start_ksoftirqd() {
    if KSOFTIRQD_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        crate::task::scheduler::spawn_with_priority(
            ksoftirqd_thread,
            crate::task::Priority::High,
            "ksoftirqd",
        );
    }
}

fn ksoftirqd_thread() -> ! {
    loop {
        if softirq_pending() {
            do_softirq();
        } else {
            crate::task::scheduler::sleep(1);
        }
    }
}

fn wake_ksoftirqd() {
    // ksoftirqd henüz başlatılmadıysa başlat
    start_ksoftirqd();
    // Thread zaten periyodik olarak kontrol ediyor
}

// ============================================================================
// Softirq Subsystem Init
//
// init() fonksiyonu, interrupts::init() tarafından çağrılır.
// Tasklet ve Hi-tasklet softirq vektörlerine handler'ları kaydeder.
// Diğer vektörler (TIMER, NET_TX, NET_RX, vb.) ilgili alt sistemler
// başlatıldığında open_softirq() ile kaydedilmelidir.
// ============================================================================

/// Softirq subsystemini başlat — tüm handler'ları kaydet
pub fn init() {
    open_softirq(SoftirqVec::Tasklet, tasklet_action);
    open_softirq(SoftirqVec::Hi, tasklet_hi_action);
    open_softirq(SoftirqVec::Timer, timer_softirq_action);
    open_softirq(SoftirqVec::Sched, sched_softirq_action);
    open_softirq(SoftirqVec::Rcu, rcu_softirq_action);
    crate::serial_println!(
        "[SOFTIRQ] Subsystem initialized (10 vectors, tasklet + timer + sched + rcu)"
    );
}

/// Timer softirq handler — ertelenmiş zamanlayıcı callback'leri işler
fn timer_softirq_action() {
    // Zamanlayıcı tekerleği (timer wheel) callback'lerini işle
    // Şu an scheduler::tick() doğrudan hard IRQ'dan çağrıldığı için
    // burada ek timer callback'leri (hrtimer, wheel) işlenebilir
    crate::task::scheduler::process_deferred_timers();
}

/// Scheduler softirq handler — ertelenmiş zamanlayıcı yeniden dengeleme
fn sched_softirq_action() {
    // CPU arası yük dengeleme — bottom-half'ta güvenli
    crate::cpu::smp::balance_load();
}

/// RCU softirq handler — ertelenmiş RCU callback'leri
fn rcu_softirq_action() {
    // Tamamlanan zariflik periyotlarının callback'lerini işle
    crate::rcu::process_callbacks();
}
