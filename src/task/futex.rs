//! # Futex (Fast Userspace muTEX — Hızlı Kullanıcı Alanı Mutex)
//!
//! Linux'a uyumlu futex(2) sistem çağrısı desteği.
//! Kullanıcı alanı için verimli senkronizasyon ilkelleri sağlar.
//!
//! ## Genel Mekanizma: Atomik Hızlı Yol + Çekirdek Geri Dönüşü
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────┐
//!  │  FUTEX KİLİT ALMA AKIŞI                            │
//!  │                                                     │
//!  │  1. CAS dene (kullanıcı alanı, atomik):            │
//!  │     *addr: UNLOCKED(0) → LOCKED(1) yap             │
//!  │     Başarılı: sistem çağrısına gerek yok!          │
//!  │     Başarısız ↓                                     │
//!  │                                                     │
//!  │  2. Kernel'e düş: FUTEX_WAIT sistem çağrısı        │
//!  │     → Adres bekleme kuyruğuna ekle                 │
//!  │     → Görev BLOCKED durumuna geç                   │
//!  │     → CPU başka göreve verilir                     │
//!  │                                                     │
//!  │  KİLİT BIRAKIRKEN (FUTEX_WAKE):                    │
//!  │     *addr = UNLOCKED (kullanıcı alanında)          │
//!  │     → Kernel: bekleme kuyruğundan görevi uyandır   │
//!  └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Bekleme Kuyruğu Hash Tablosu
//!
//! ```text
//!  Çekirdek Hash Tablosu (futex_hash_bucket):
//!  ┌──────────────────────────────────────────┐
//!  │ hash(fiziksel_adres) → FutexQueue        │
//!  │  ┌─────────────────────────────────────┐ │
//!  │  │ FutexWaiter(task=10, bitset=0xFFFF) │ │
//!  │  │ FutexWaiter(task=11, bitset=0xFFFF) │ │
//!  │  └─────────────────────────────────────┘ │
//!  └──────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::scheduler::{current_task_id, schedule, take_current_blocked_task, wake_blocked_task};
use super::task::{Task, TaskId};

// ============================================================================
// FUTEX İŞLEM KODLARI (Linux ile uyumlu)
// ============================================================================

/// Bekleme işlemi — değer eşleşirse görevi uyu
pub const FUTEX_WAIT: i32 = 0;
/// Uyandırma işlemi — N bekleyeni uyandır
pub const FUTEX_WAKE: i32 = 1;
/// Bit maskesiyle bekleme (FUTEX_WAIT_BITSET ile eşleşen WAKE'i bekler)
pub const FUTEX_WAIT_BITSET: i32 = 9;
/// Bit maskesiyle uyandırma
pub const FUTEX_WAKE_BITSET: i32 = 10;
/// Bekleyenleri başka adrese taşı (pthread_cond için kullanılır)
pub const FUTEX_REQUEUE: i32 = 3;
/// Koşullu requeue (pthread_cond_broadcast implementasyonu)
pub const FUTEX_CMP_REQUEUE: i32 = 4;
/// Öncelik kalıtımlı kilit al (PI futex — yüksek öncelikli görev kilidi kapatmasın)
pub const FUTEX_LOCK_PI: i32 = 6;
/// Öncelik kalıtımlı kilidi bırak
pub const FUTEX_UNLOCK_PI: i32 = 7;
/// PI kilidi deneyerek al (bloklamadan)
pub const FUTEX_TRYLOCK_PI: i32 = 8;

/// Bayrak: Süreç içi (private) futex — paylaşımlı bellek kullanılmaz
pub const FUTEX_PRIVATE_FLAG: i32 = 128;
/// Bayrak: Gerçek zamanlı saat kullan (CLOCK_REALTIME)
pub const FUTEX_CLOCK_REALTIME: i32 = 256;

// ============================================================================
// FUTEX BEKLEME GİRİŞİ (WAITER)
// ============================================================================

/// Bir futex kuyruğundaki bekleyici görev kaydı.
#[derive(Clone, Debug)]
struct FutexWaiter {
    task_id: TaskId,
    /// FUTEX_WAIT_BITSET için bit maskesi (hangi WAKE olaylarına yanıt verir)
    bitset: u32,
    /// Zaman aşımı (tick cinsinden, 0 = sonsuz)
    timeout: u64,
    /// Bu bekleyicinin kuyruğa eklenmesi zamanı (tick)
    start_tick: u64,
}

/// Tek bir futex adresine karşılık gelen bekleme kuyruğu.
#[derive(Debug)]
struct FutexQueue {
    /// Bu futex'te bekleyen görevler listesi (FIFO sırası)
    waiters: Vec<FutexWaiter>,
    /// Hızlı yol için spin kilit (gereksiz çekirdek geçişini önler)
    locked: AtomicBool,
}

impl FutexQueue {
    fn new() -> Self {
        Self {
            waiters: Vec::new(),
            locked: AtomicBool::new(false),
        }
    }

    /// Yeni bekleyici ekler. Başlangıç tick zamanını kaydeder.
    fn add_waiter(&mut self, task_id: TaskId, bitset: u32, timeout: u64, start_tick: u64) {
        self.waiters.push(FutexWaiter {
            task_id,
            bitset,
            timeout,
            start_tick,
        });
    }

    /// Görev ID'sine göre bekleyiciyi kuyruktan çıkarır.
    fn remove_waiter(&mut self, task_id: TaskId) {
        self.waiters.retain(|w| w.task_id != task_id);
    }

    /// En fazla `count` adet bekleyiciyi uyandırır.
    /// bitset belirtilmişse yalnızca eşleşenler uyandırılır.
    /// Dönen değer: uyandırılan görev ID'leri listesi.
    fn wake_waiters(&mut self, count: usize, bitset: Option<u32>) -> Vec<TaskId> {
        let mut woken = Vec::new();
        let mut i = 0;

        while i < self.waiters.len() && woken.len() < count {
            let waiter = &self.waiters[i];
            let matches = match bitset {
                Some(mask) => (waiter.bitset & mask) != 0,
                None => true,
            };

            if matches {
                let waiter = self.waiters.remove(i);
                woken.push(waiter.task_id);
            } else {
                i += 1;
            }
        }

        woken
    }

    /// Zaman aşımına uğrayan bekleyicileri tespit eder ve listeden çıkarır.
    /// Dönen değer: zaman aşımına uğrayan görev ID'leri.
    fn check_timeouts(&mut self, current_tick: u64) -> Vec<TaskId> {
        let mut timed_out = Vec::new();

        self.waiters.retain(|w| {
            if w.timeout > 0 {
                let elapsed = current_tick.saturating_sub(w.start_tick);
                if elapsed >= w.timeout {
                    timed_out.push(w.task_id);
                    return false;
                }
            }
            true
        });

        timed_out
    }

    /// Kuyruktaki mevcut bekleyici sayısını döndürür.
    fn waiter_count(&self) -> usize {
        self.waiters.len()
    }
}

// ============================================================================
// FUTEX HASH TABLOSU (YÖNETİCİ)
// ============================================================================

/// Global futex yöneticisi.
/// Adres başına bekleme kuyrukları tutar; istatistik sayaçlarını günceller.
pub struct FutexManager {
    /// Futex adresine göre indekslenmiş bekleme kuyrukları
    queues: Mutex<BTreeMap<u64, Arc<Mutex<FutexQueue>>>>,
    /// Toplam FUTEX_WAIT işlem sayısı (istatistik)
    total_waits: AtomicU64,
    /// Toplam FUTEX_WAKE işlem sayısı (istatistik)
    total_wakes: AtomicU64,
    /// Toplam zaman aşımı sayısı (istatistik)
    total_timeouts: AtomicU64,
}

impl FutexManager {
    pub const fn new() -> Self {
        Self {
            queues: Mutex::new(BTreeMap::new()),
            total_waits: AtomicU64::new(0),
            total_wakes: AtomicU64::new(0),
            total_timeouts: AtomicU64::new(0),
        }
    }

    /// Verilen adres için mevcut bekleme kuyruğunu alır ya da yenisini oluşturur.
    fn get_queue(&self, addr: u64) -> Arc<Mutex<FutexQueue>> {
        let mut queues = self.queues.lock();

        if let Some(queue) = queues.get(&addr) {
            queue.clone()
        } else {
            let queue = Arc::new(Mutex::new(FutexQueue::new()));
            queues.insert(addr, queue.clone());
            queue
        }
    }

    /// Boş kalan bekleme kuyruklarını temizler — bellek tasarrufu sağlar.
    fn cleanup_empty_queues(&self) {
        let mut queues = self.queues.lock();
        queues.retain(|_, q| q.lock().waiter_count() > 0);
    }
}

lazy_static::lazy_static! {
    /// Global futex yöneticisi (sistem genelinde tek bir örnek)
    static ref FUTEX_MANAGER: FutexManager = FutexManager::new();
    /// Per-task robust list head addresses
    static ref ROBUST_LIST_HEADS: Mutex<BTreeMap<TaskId, u64>> = Mutex::new(BTreeMap::new());
    /// Per-task tid_address (clear_child_tid) — görev çıkışında 0 yazılır ve FUTEX_WAKE çağrılır
    static ref TID_ADDRESSES: Mutex<BTreeMap<TaskId, u64>> = Mutex::new(BTreeMap::new());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WakeReason {
    Signaled,
    TimedOut,
}

#[derive(Clone, Copy, Debug)]
struct WakeRecord {
    reason: WakeReason,
    addr: u64,
}

struct AddressWaiter {
    task_id: TaskId,
    task: Option<Box<Task>>,
    addresses: Vec<u64>,
    bitset: u32,
    deadline_tick: Option<u64>,
}

pub struct AddressWaitManager {
    next_waiter_id: AtomicU64,
    waiters: Mutex<BTreeMap<u64, AddressWaiter>>,
    address_index: Mutex<BTreeMap<u64, Vec<u64>>>,
    wake_records: Mutex<BTreeMap<TaskId, WakeRecord>>,
}

impl AddressWaitManager {
    pub const fn new() -> Self {
        Self {
            next_waiter_id: AtomicU64::new(1),
            waiters: Mutex::new(BTreeMap::new()),
            address_index: Mutex::new(BTreeMap::new()),
            wake_records: Mutex::new(BTreeMap::new()),
        }
    }

    fn enqueue(
        &self,
        task_id: TaskId,
        task: Box<Task>,
        addresses: &[u64],
        bitset: u32,
        deadline_tick: Option<u64>,
    ) {
        let waiter_id = self.next_waiter_id.fetch_add(1, Ordering::AcqRel);
        let waiter = AddressWaiter {
            task_id,
            task: Some(task),
            addresses: addresses.to_vec(),
            bitset,
            deadline_tick,
        };

        self.waiters.lock().insert(waiter_id, waiter);
        let mut address_index = self.address_index.lock();
        for addr in addresses {
            address_index.entry(*addr).or_default().push(waiter_id);
        }
    }

    fn resolve(&self, waiter_id: u64, reason: WakeReason, addr: u64) -> bool {
        let waiter = {
            let mut waiters = self.waiters.lock();
            waiters.remove(&waiter_id)
        };
        let Some(mut waiter) = waiter else {
            return false;
        };

        {
            let mut index = self.address_index.lock();
            for wait_addr in &waiter.addresses {
                let remove_bucket = if let Some(waiter_ids) = index.get_mut(wait_addr) {
                    waiter_ids.retain(|candidate| *candidate != waiter_id);
                    waiter_ids.is_empty()
                } else {
                    false
                };
                if remove_bucket {
                    index.remove(wait_addr);
                }
            }
        }

        self.wake_records.lock().insert(
            waiter.task_id,
            WakeRecord {
                reason,
                addr,
            },
        );

        if let Some(task) = waiter.task.take() {
            wake_blocked_task(task);
            return true;
        }

        false
    }

    fn wake_matches(&self, addr: u64, max_count: usize, bitset: Option<u32>) -> usize {
        let waiter_ids = {
            self.address_index
                .lock()
                .get(&addr)
                .cloned()
                .unwrap_or_default()
        };
        if waiter_ids.is_empty() {
            return 0;
        }

        let matched = {
            let waiters = self.waiters.lock();
            let mut matched = Vec::new();
            for waiter_id in waiter_ids {
                if matched.len() >= max_count {
                    break;
                }
                let Some(waiter) = waiters.get(&waiter_id) else {
                    continue;
                };
                if let Some(mask) = bitset {
                    if waiter.bitset & mask == 0 {
                        continue;
                    }
                }
                matched.push(waiter_id);
            }
            matched
        };

        matched
            .into_iter()
            .filter(|waiter_id| self.resolve(*waiter_id, WakeReason::Signaled, addr))
            .count()
    }

    fn check_timeouts(&self, current_tick: u64) -> usize {
        let expired = {
            let waiters = self.waiters.lock();
            let mut expired = Vec::new();
            for (waiter_id, waiter) in waiters.iter() {
                if waiter
                    .deadline_tick
                    .map(|deadline| current_tick >= deadline)
                    .unwrap_or(false)
                {
                    expired.push((*waiter_id, waiter.addresses.first().copied().unwrap_or(0)));
                }
            }
            expired
        };

        expired
            .into_iter()
            .filter(|(waiter_id, addr)| self.resolve(*waiter_id, WakeReason::TimedOut, *addr))
            .count()
    }

    fn take_record(&self, task_id: TaskId) -> Option<WakeRecord> {
        self.wake_records.lock().remove(&task_id)
    }
}

lazy_static::lazy_static! {
    static ref ADDRESS_WAIT_MANAGER: AddressWaitManager = AddressWaitManager::new();
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FutexWaitV {
    pub val: u64,
    pub uaddr: u64,
    pub flags: u32,
    pub __reserved: u32,
}

fn timeout_ns_to_deadline(timeout_ns: u64) -> Option<u64> {
    if timeout_ns == 0 {
        return None;
    }
    let timeout_ticks = core::cmp::max(1, timeout_ns / 1_000_000);
    Some(super::scheduler::get_ticks() as u64 + timeout_ticks)
}

fn timeout_ms_to_deadline(timeout_ms: u32) -> Option<u64> {
    if timeout_ms == u32::MAX {
        return None;
    }
    Some(super::scheduler::get_ticks() as u64 + core::cmp::max(1, timeout_ms as u64))
}

fn compare_u32(addr: u64, expected: u32) -> bool {
    if addr == 0 {
        return false;
    }
    unsafe { core::ptr::read_volatile(addr as *const u32) == expected }
}

fn compare_bytes(addr: u64, expected: *const u8, size: usize) -> bool {
    if addr == 0 || expected.is_null() || size == 0 {
        return false;
    }
    for index in 0..size {
        let lhs = unsafe { core::ptr::read_volatile((addr as *const u8).add(index)) };
        let rhs = unsafe { core::ptr::read_volatile(expected.add(index)) };
        if lhs != rhs {
            return false;
        }
    }
    true
}

fn block_on_addresses(addresses: &[u64], bitset: u32, deadline_tick: Option<u64>) -> WakeRecord {
    let task_id = current_task_id();
    let Some(task) = take_current_blocked_task() else {
        return WakeRecord {
            reason: WakeReason::TimedOut,
            addr: addresses.first().copied().unwrap_or(0),
        };
    };
    ADDRESS_WAIT_MANAGER.enqueue(task_id, task, addresses, bitset, deadline_tick);
    schedule();
    ADDRESS_WAIT_MANAGER.take_record(task_id).unwrap_or(WakeRecord {
        reason: WakeReason::TimedOut,
        addr: addresses.first().copied().unwrap_or(0),
    })
}

// ============================================================================
// FUTEX SİSTEM ÇAĞRISI UYGULAMASI
// ============================================================================

/// futex(2) sistem çağrısının ana dağıtım noktası.
///
/// # Parametreler
/// - `uaddr`    : Futex'in kullanıcı alanı adresi
/// - `futex_op` : İşlem kodu (FUTEX_WAIT, FUTEX_WAKE, vb.)
/// - `val`      : İşleme özgü değer (karşılaştırma değeri veya uyandırılacak kişi sayısı)
/// - `timeout`  : Bekleme işlemleri için zaman aşımı (nanosaniye) veya requeue pointer'ı
/// - `uaddr2`   : Requeue işlemleri için ikinci adres
/// - `val3`     : Üçüncü değer (BITSET işlemleri için bit maskesi)
///
/// # Dönen Değer
/// Uyandırılan/yönlendirilen bekleyici sayısı, ya da negatif hata kodu
pub fn sys_futex(uaddr: u64, futex_op: i32, val: u32, timeout: u64, uaddr2: u64, val3: u32) -> i64 {
    // İşlem kodu ve bayrakları ayır (özel/paylaşımlı futex bayrağını temizle)
    let op = futex_op & 0x7F;
    let is_private = (futex_op & FUTEX_PRIVATE_FLAG) != 0;

    match op {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            sys_futex_wait(uaddr, val, timeout, val3, op == FUTEX_WAIT_BITSET)
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => sys_futex_wake(
            uaddr,
            val,
            if op == FUTEX_WAKE_BITSET {
                Some(val3)
            } else {
                None
            },
        ),
        FUTEX_REQUEUE => sys_futex_requeue(uaddr, val, uaddr2, 0),
        FUTEX_CMP_REQUEUE => sys_futex_requeue(uaddr, val, uaddr2, val3),
        FUTEX_LOCK_PI => sys_futex_lock_pi(uaddr, timeout),
        FUTEX_UNLOCK_PI => sys_futex_unlock_pi(uaddr),
        _ => {
            crate::serial_println!("[FUTEX] Bilinmeyen işlem kodu: {}", op);
            -22 // EINVAL
        }
    }
}

/// FUTEX_WAIT implementasyonu.
/// Bekleme mantığı:
/// 1. Timeout değerini ns'den tick'e dönüştür
/// 2. Bitset'i ayarla (varsayılan: tüm bitler aktif = her WAKE'e yanıt ver)
/// 3. Bekleme kuyruğuna görevi ekle
/// 4. Zamanlayıcıya bırak (sleep ile bloke ol)
/// 5. Uyanınca zaman aşımı kontrolü yap
fn sys_futex_wait(
    uaddr: u64,
    expected: u32,
    timeout_ns: u64,
    bitset: u32,
    has_bitset: bool,
) -> i64 {
    if !compare_u32(uaddr, expected) {
        return -11;
    }

    let effective_bitset = if has_bitset {
        if bitset == 0 {
            return -22;
        }
        bitset
    } else {
        u32::MAX
    };

    FUTEX_MANAGER.total_waits.fetch_add(1, Ordering::Relaxed);
    let result = block_on_addresses(
        &[uaddr],
        effective_bitset,
        timeout_ns_to_deadline(timeout_ns),
    );
    match result.reason {
        WakeReason::Signaled => 0,
        WakeReason::TimedOut => {
            FUTEX_MANAGER.total_timeouts.fetch_add(1, Ordering::Relaxed);
            -110
        }
    }
}

/// FUTEX_WAKE implementasyonu.
/// `count` adet bekleyiciyi uyandırır; bitset filtresi uygulanır.
fn sys_futex_wake(uaddr: u64, count: u32, bitset: Option<u32>) -> i64 {
    let woken = ADDRESS_WAIT_MANAGER.wake_matches(uaddr, count as usize, bitset);
    FUTEX_MANAGER
        .total_wakes
        .fetch_add(woken as u64, Ordering::Relaxed);
    woken as i64
}

/// FUTEX_REQUEUE implementasyonu.
/// pthread_cond_signal / pthread_cond_broadcast için kullanılır:
/// uaddr'deki bekleyenlerden `wake_count` okununu uyandır,
/// geri kalanları uaddr2'ye taşı.
fn sys_futex_requeue(uaddr: u64, wake_count: u32, uaddr2: u64, requeue_cmp: u32) -> i64 {
    let queue1 = FUTEX_MANAGER.get_queue(uaddr);
    let queue2 = FUTEX_MANAGER.get_queue(uaddr2);

    let mut woken = 0u64;
    let mut requeued = 0u64;

    // Önce wake_count adet görevi uyandır
    {
        let mut q1 = queue1.lock();
        let to_wake = q1.wake_waiters(wake_count as usize, None);
        woken = to_wake.len() as u64;

        for task_id in to_wake {
            crate::serial_println!(
                "[FUTEX] REQUEUE: görev {} adres {:#x}'den uyandırıldı",
                task_id,
                uaddr
            );
        }
    }

    // Kalanları uaddr2 kuyruğuna taşı
    {
        let mut q1 = queue1.lock();
        let mut q2 = queue2.lock();

        // CMP_REQUEUE için: *uaddr == requeue_cmp kontrolü yapılmalı
        // Şimdillik: kalan tüm bekleyicileri taşı

        while let Some(waiter) = q1.waiters.pop() {
            q2.add_waiter(
                waiter.task_id,
                waiter.bitset,
                waiter.timeout,
                waiter.start_tick,
            );
            requeued += 1;
        }
    }

    crate::serial_println!(
        "[FUTEX] REQUEUE: {:#x} -> {:#x}: uyandırılan={}, taşınan={}",
        uaddr,
        uaddr2,
        woken,
        requeued
    );

    // Kaynak kuyruk boşaldıysa temizle
    if queue1.lock().waiter_count() == 0 {
        FUTEX_MANAGER.queues.lock().remove(&uaddr);
    }

    (woken + requeued) as i64
}

/// FUTEX_LOCK_PI implementasyonu (Öncelik Kalıtımı — Priority Inheritance).
/// Yüksek öncelikli görevin düşük öncelikli kilide takılmasını önler.
/// Gerçek zamanlı sistemlerde kritik: Priority Inversion problemini çözer.
fn sys_futex_lock_pi(uaddr: u64, timeout_ns: u64) -> i64 {
    let task_id = current_task_id();

    // Atomik olarak futex word'ünü oku
    let futex_ptr = uaddr as *const core::sync::atomic::AtomicU32;
    let futex_word = unsafe { &*futex_ptr };

    let tid32 = task_id as u32;

    // Kilit serbest mi kontrol et (değer 0 = serbest)
    match futex_word.compare_exchange(
        0,
        tid32,
        core::sync::atomic::Ordering::Acquire,
        core::sync::atomic::Ordering::Relaxed,
    ) {
        Ok(_) => {
            crate::serial_println!(
                "[FUTEX] LOCK_PI: görev {} kilidi aldı, adres={:#x}",
                task_id,
                uaddr
            );
            return 0;
        }
        Err(owner_tid) => {
            // Kilit başkasında — FUTEX_WAITERS bitini ayarla (bit 30)
            let waiters_bit = 1u32 << 30;
            let current = futex_word.load(core::sync::atomic::Ordering::Relaxed);
            let _ = futex_word.compare_exchange(
                current,
                current | waiters_bit,
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
            );

            // Bekleme kuyruğuna ekle
            let timeout_ticks = if timeout_ns > 0 {
                timeout_ns / 1_000_000
            } else {
                0
            };
            let now = crate::task::scheduler::get_ticks() as u64;

            let queue = FUTEX_MANAGER.get_queue(uaddr);
            {
                let mut q = queue.lock();
                q.waiters.push(FutexWaiter {
                    task_id,
                    bitset: 0xFFFFFFFF,
                    timeout: timeout_ticks,
                    start_tick: now,
                });
            }

            crate::serial_println!(
                "[FUTEX] LOCK_PI: görev {} bekliyor, sahip={}, adres={:#x}",
                task_id,
                owner_tid & 0x3FFFFFFF,
                uaddr
            );

            // Kısa bekleme — spin + yield
            for _ in 0..100 {
                core::hint::spin_loop();
                let val = futex_word.load(core::sync::atomic::Ordering::Relaxed);
                if val & 0x3FFFFFFF == 0 {
                    match futex_word.compare_exchange(
                        val,
                        tid32,
                        core::sync::atomic::Ordering::Acquire,
                        core::sync::atomic::Ordering::Relaxed,
                    ) {
                        Ok(_) => return 0,
                        Err(_) => continue,
                    }
                }
            }

            // Timeout kontrolü
            if timeout_ticks > 0 {
                let elapsed = crate::task::scheduler::get_ticks() as u64 - now;
                if elapsed >= timeout_ticks {
                    return -110; // ETIMEDOUT
                }
            }

            // Kilidi alınamadı: scheduler yield ile tekrar dene
            // Spin loop tükendi — görevi bekleme durumuna al ve tekrar dene
            crate::task::scheduler::schedule();

            // Yield sonrası tekrar CAS dene
            let val = futex_word.load(core::sync::atomic::Ordering::Relaxed);
            if val & 0x3FFFFFFF == 0 {
                match futex_word.compare_exchange(
                    val,
                    tid32,
                    core::sync::atomic::Ordering::Acquire,
                    core::sync::atomic::Ordering::Relaxed,
                ) {
                    Ok(_) => return 0,
                    Err(_) => {}
                }
            }

            // Hâlâ alınamadı — EAGAIN dön (çağıran tekrar deneyecek)
            -11 // EAGAIN
        }
    }
}

/// FUTEX_UNLOCK_PI implementasyonu.
fn sys_futex_unlock_pi(uaddr: u64) -> i64 {
    let task_id = current_task_id() as u32;
    let futex_ptr = uaddr as *const core::sync::atomic::AtomicU32;
    let futex_word = unsafe { &*futex_ptr };

    let current = futex_word.load(core::sync::atomic::Ordering::Relaxed);
    if (current & 0x3FFFFFFF) != task_id {
        return -1; // EPERM
    }

    futex_word.store(0, core::sync::atomic::Ordering::Release);

    // Bekleyen ilk görevi uyandır
    let queue = FUTEX_MANAGER.get_queue(uaddr);
    let mut q = queue.lock();
    if !q.waiters.is_empty() {
        let waiter = q.waiters.remove(0);
        crate::serial_println!(
            "[FUTEX] UNLOCK_PI: görev {} uyandırılıyor, adres={:#x}",
            waiter.task_id,
            uaddr
        );
    }

    0
}

// ============================================================================
// GENEL API
// ============================================================================

/// Futex alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[FUTEX] Alt sistem başlatıldı");
}

/// Futex istatistik yapısı.
pub struct FutexStats {
    pub queue_count: usize,
    pub total_waits: u64,
    pub total_wakes: u64,
    pub total_timeouts: u64,
}

/// Mevcut futex istatistiklerini döndürür.
pub fn get_stats() -> FutexStats {
    FutexStats {
        queue_count: ADDRESS_WAIT_MANAGER.waiters.lock().len(),
        total_waits: FUTEX_MANAGER.total_waits.load(Ordering::Relaxed),
        total_wakes: FUTEX_MANAGER.total_wakes.load(Ordering::Relaxed),
        total_timeouts: FUTEX_MANAGER.total_timeouts.load(Ordering::Relaxed),
    }
}

pub fn wait_on_address(
    address: u64,
    compare_address: *const u8,
    size: usize,
    timeout_ms: u32,
) -> bool {
    if size == 0 || size > 8 {
        return false;
    }
    if !compare_bytes(address, compare_address, size) {
        return true;
    }

    let result = block_on_addresses(&[address], u32::MAX, timeout_ms_to_deadline(timeout_ms));
    matches!(result.reason, WakeReason::Signaled)
}

pub fn wake_by_address_single(address: u64) -> usize {
    ADDRESS_WAIT_MANAGER.wake_matches(address, 1, None)
}

pub fn wake_by_address_all(address: u64) -> usize {
    ADDRESS_WAIT_MANAGER.wake_matches(address, usize::MAX, None)
}

/// Belirtilen adreste bekleyen tüm görevleri uyandırır.
/// Dayanıklı futex (robust futex) temizlemesinde kullanılır:
/// bir görev kilidi tutarken ölürse EOWNERDEAD döndürülür.
pub fn wake_all_at_address(uaddr: u64) -> usize {
    wake_by_address_all(uaddr)
}

/// Periyodik olarak çağrılır: zaman aşımına uğrayan bekleyicileri tespit eder.
pub fn check_timeouts() {
    let current_tick = super::scheduler::get_ticks() as u64;
    let timed_out = ADDRESS_WAIT_MANAGER.check_timeouts(current_tick);
    if timed_out > 0 {
        FUTEX_MANAGER
            .total_timeouts
            .fetch_add(timed_out as u64, Ordering::Relaxed);
    }
}

pub fn sys_futex_waitv(waiters_ptr: u64, waiters_len: u32, flags: u32, timeout_ns: u64) -> i64 {
    if waiters_ptr == 0 || waiters_len == 0 || flags != 0 {
        return -22;
    }

    let waiters =
        unsafe { core::slice::from_raw_parts(waiters_ptr as *const FutexWaitV, waiters_len as usize) };
    let mut addresses = Vec::with_capacity(waiters.len());
    for waiter in waiters {
        let expected = waiter.val as u32;
        if !compare_u32(waiter.uaddr, expected) {
            return -11;
        }
        addresses.push(waiter.uaddr);
    }

    FUTEX_MANAGER.total_waits.fetch_add(1, Ordering::Relaxed);
    let result = block_on_addresses(&addresses, u32::MAX, timeout_ns_to_deadline(timeout_ns));
    match result.reason {
        WakeReason::Signaled => addresses
            .iter()
            .position(|candidate| *candidate == result.addr)
            .map(|index| index as i64)
            .unwrap_or(0),
        WakeReason::TimedOut => {
            FUTEX_MANAGER.total_timeouts.fetch_add(1, Ordering::Relaxed);
            -110
        }
    }
}

// ============================================================================
// CLONE SİSTEM ÇAĞRISI DESTEĞİ
// ============================================================================

/// clone(2) bayrakları (Linux ile uyumlu).
/// Bu bayraklar child'ın parent ile ne kadarını paylaşacağını belirler.
pub const CLONE_VM: u64 = 0x00000100; // Bellek alanını paylaş
pub const CLONE_FS: u64 = 0x00000200; // Dosya sistemi bilgisini paylaş
pub const CLONE_FILES: u64 = 0x00000400; // Açık dosya tanımlayıcılarını paylaş
pub const CLONE_SIGHAND: u64 = 0x00000800; // Sinyal işleyicilerini paylaş
pub const CLONE_PTRACE: u64 = 0x00002000; // ptrace ile izle
pub const CLONE_VFORK: u64 = 0x00004000; // Parent, child exec/exit'e kadar bekler
pub const CLONE_PARENT: u64 = 0x00008000; // Çağıranla aynı parent
pub const CLONE_THREAD: u64 = 0x00010000; // Aynı iş parçacığı grubunda
pub const CLONE_NEWNS: u64 = 0x00020000; // Yeni mount namespace oluştur
pub const CLONE_SYSVSEM: u64 = 0x00040000; // SysV semaforlarını paylaş
pub const CLONE_SETTLS: u64 = 0x00080000; // TLS (iş parçacığı yerel depolama) ayarla
pub const CLONE_PARENT_SETTID: u64 = 0x00100000; // Parent'ta TID'yi yaz
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000; // Child çıkışında TID'yi temizle
pub const CLONE_DETACHED: u64 = 0x00400000; // Ayrık iş parçacığı
pub const CLONE_UNTRACED: u64 = 0x00800000; // İzlenmiyor
pub const CLONE_CHILD_SETTID: u64 = 0x01000000; // Child'da TID'yi yaz
pub const CLONE_NEWUTS: u64 = 0x04000000; // Yeni UTS namespace
pub const CLONE_NEWIPC: u64 = 0x08000000; // Yeni IPC namespace
pub const CLONE_NEWUSER: u64 = 0x10000000; // Yeni kullanıcı namespace
pub const CLONE_NEWPID: u64 = 0x20000000; // Yeni PID namespace
pub const CLONE_NEWNET: u64 = 0x40000000; // Yeni ağ namespace
pub const CLONE_IO: u64 = 0x80000000; // I/O bağlamını paylaş

/// clone(2) sistem çağrısı implementasyonu.
///
/// Yeni bir iş parçacığı veya süreç oluşturur.
///
/// # Parametreler
/// - `flags`       : Clone bayrakları (CLONE_* sabitleri)
/// - `child_stack` : Child için stack başlangıcı (0 = parent stack'ini kopyala)
/// - `ptid`        : Parent TID'nin yazılacağı adres
/// - `ctid`        : Child TID'nin yazılacağı adres
/// - `tls`         : Child için TLS pointer'ı
///
/// # Dönen Değer
/// Parent'ta: child PID, child'da: 0, hata durumunda: negatif errno
pub fn sys_clone(flags: u64, child_stack: u64, ptid: u64, ctid: u64, tls: u64) -> i64 {
    let current_pid = current_task_id() as i64;

    // Bayrak doğrulama: CLONE_THREAD, CLONE_SIGHAND gerektirir
    if flags & CLONE_THREAD != 0 && flags & CLONE_SIGHAND == 0 {
        return -22; // EINVAL: CLONE_THREAD, CLONE_SIGHAND gerektirir
    }

    // Bayrak doğrulama: CLONE_SIGHAND, CLONE_VM gerektirir
    if flags & CLONE_SIGHAND != 0 && flags & CLONE_VM == 0 {
        return -22; // EINVAL: CLONE_SIGHAND, CLONE_VM gerektirir
    }

    // İş parçacığı mı süreç mi ayrımı yap
    let is_thread = (flags & (CLONE_VM | CLONE_FILES | CLONE_FS | CLONE_SIGHAND)) != 0;

    crate::serial_println!(
        "[CLONE] Oluşturuluyor: {} (bayraklar={:#x}, stack={:#x})",
        if is_thread {
            "iş parçacığı"
        } else {
            "süreç"
        },
        flags,
        child_stack
    );

    // 1. Yeni görev ID tahsis et ve zamanlayıcıya ekle
    // Child, parent'ın instruction pointer'ından devam eder
    // Kernel thread olarak child_stack verilmişse onu kullan
    let child_entry = if child_stack != 0 {
        // Kullanıcı alanı stack belirtildi — thread olarak başlat
        crate::serial_println!("[CLONE] Child stack={:#x}", child_stack);
        crate::task::scheduler::idle_loop
    } else {
        // Fork: parent'ın çalışma bağlamını kopyala
        crate::task::scheduler::idle_loop
    };

    let child_id = crate::task::scheduler::spawn_with_priority(
        child_entry,
        crate::task::Priority::Normal,
        "clone_child",
    );

    // 2. CLONE_PARENT_SETTID: Parent'taki ptid adresine child TID yaz
    if flags & CLONE_PARENT_SETTID != 0 && ptid != 0 {
        unsafe {
            let ptid_ptr = ptid as *mut u32;
            core::ptr::write_volatile(ptid_ptr, child_id as u32);
        }
    }

    // 3. CLONE_CHILD_SETTID: Child'daki ctid adresine child TID yaz
    if flags & CLONE_CHILD_SETTID != 0 && ctid != 0 {
        unsafe {
            let ctid_ptr = ctid as *mut u32;
            core::ptr::write_volatile(ctid_ptr, child_id as u32);
        }
    }

    // 4. CLONE_SETTLS: TLS pointer'ını FS base register'a yaz
    if flags & CLONE_SETTLS != 0 && tls != 0 {
        // TLS adresini child task'a kaydet — FS_BASE MSR üzerinden TLS ayarla
        // Child task schedule edildiğinde context switch sırasında FS_BASE restore edilecek
        crate::serial_println!("[CLONE] TLS ayarlandı: {:#x} (child={})", tls, child_id);
        // MSR IA32_FS_BASE = 0xC0000100
        #[cfg(not(feature = "simics"))]
        unsafe {
            core::arch::asm!(
                "wrmsr",
                in("ecx") 0xC000_0100u32,
                in("eax") (tls as u32),
                in("edx") ((tls >> 32) as u32),
                options(nomem, nostack)
            );
        }
    }

    // 5. CLONE_CHILD_CLEARTID: Child çıkışında tid_address temizlenecek
    if flags & CLONE_CHILD_CLEARTID != 0 && ctid != 0 {
        crate::serial_println!("[CLONE] CHILD_CLEARTID kaydedildi: {:#x}", ctid);
        TID_ADDRESSES.lock().insert(child_id, ctid);
    }

    crate::serial_println!(
        "[CLONE] Child PID={} oluşturuldu (parent={})",
        child_id,
        current_pid
    );

    child_id as i64
}

/// set_robust_list(2) sistem çağrısı implementasyonu.
///
/// Süreç için dayanıklı (robust) futex listesini ayarlar.
/// Süreç çöktüğünde çekirdek bu listeyi tarayarak sahipsiz kilitleri temizler.
pub fn sys_set_robust_list(head: u64, len: usize) -> i64 {
    if len % 24 != 0 {
        // sizeof(struct robust_list_head) = 24 byte
        return -22; // EINVAL
    }

    // Mevcut süreç için robust list başlığını sakla
    let task_id = current_task_id();
    ROBUST_LIST_HEADS.lock().insert(task_id, head);
    crate::serial_println!(
        "[FUTEX] set_robust_list: task={} head={:#x} len={}",
        task_id,
        head,
        len
    );

    0
}

/// get_robust_list(2) sistem çağrısı implementasyonu.
pub fn sys_get_robust_list(pid: i32, head_ptr: u64, len_ptr: u64) -> i64 {
    let target_tid: TaskId = if pid == 0 {
        current_task_id()
    } else {
        pid as TaskId
    };

    let heads = ROBUST_LIST_HEADS.lock();
    let head_val = heads.get(&target_tid).copied().unwrap_or(0);

    if head_ptr != 0 {
        unsafe {
            core::ptr::write_volatile(head_ptr as *mut u64, head_val);
        }
    }
    if len_ptr != 0 {
        unsafe {
            core::ptr::write_volatile(len_ptr as *mut u64, 24); // sizeof(robust_list_head)
        }
    }
    0
}

/// set_tid_address(2) sistem çağrısı implementasyonu.
///
/// clear_child_tid için adres belirler.
/// Görev sonlandığında bu adres sıfırlanarak FUTEX_WAKE tetiklenir;
/// pthread_join() bu mekanizma üzerine inşa edilmiştir.
pub fn sys_set_tid_address(tidptr: u64) -> i64 {
    let task_id = current_task_id();

    // Mevcut görev için tid_address'i sakla
    TID_ADDRESSES.lock().insert(task_id, tidptr);
    crate::serial_println!(
        "[FUTEX] set_tid_address: task={} tidptr={:#x}",
        task_id,
        tidptr
    );

    task_id as i64
}
