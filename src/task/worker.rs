//! # echOS io_uring / Asenkron Kernel Worker Havuzu (Lock-Free)
//!
//! Bu modül, io_uring ve genel amaçlı asenkron iş yükleri için
//! bir kernel thread (kthread) havuzu (workqueue) sağlar.
//! `sys_io_uring_enter` çağrıları yüklerini buraya devreder.
//!
//! ## Mimari: Treiber Stack (Lock-Free MPMC)
//!
//! ```text
//!  spawn_work() ─CAS─→ [Global Treiber Stack] ←─CAS─ worker_loop()
//!       (Çoklu üretici + çoklu tüketici, Mutex SIFIR)
//!
//!  Producer (any CPU)          Consumer (Worker Thread)
//!  ┌──────────────────┐        ┌──────────────────────┐
//!  │ spawn_work(f)    │        │ worker_loop_entry()  │
//!  │  → push(node)    │        │  → pop() → f()      │
//!  │  → CAS head      │        │  → CAS head          │
//!  │  → **NO MUTEX**  │        │  → **NO MUTEX**      │
//!  └──────────────────┘        └──────────────────────┘
//! ```
//!
//! **Mutex SIFIR** — Tüm senkronizasyon CAS (Compare-And-Swap) ile yapılır.
//!
//! ## Gelecek Optimizasyon (H5+)
//! Per-worker Chase-Lev deque'lar ile iş çalma (work-stealing) eklenecek.
//! Workers kendi yerel kuyruklarından LIFO ile alacak, boşta kalırsa
//! komşu worker'ların kuyruğundan FIFO ile çalacak.

use alloc::boxed::Box;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

// ────────────────────────────────────────────────────────────
// Lock-Free Treiber Stack (Michael, 2004)
// ────────────────────────────────────────────────────────────
//
// MPMC (Multi-Producer / Multi-Consumer) lock-free stack.
// Push ve pop işlemleri atomik CAS (compare_exchange) ile yapılır.
// Hiçbir noktada Mutex, SpinLock veya interrupt disable YOKTUR.

/// İş kuyruğundaki tek bir düğüm (singly-linked list node)
struct StackNode {
    /// Type-erased closure: `Box<dyn FnOnce() + Send>`
    func: Option<Box<dyn FnOnce() + Send>>,
    /// Sonraki düğüme raw pointer (null = kuyruk sonu)
    next: *mut StackNode,
}

// StackNode ham pointer içerdiği için Send/Sync otomatik türetilmez.
// Ancak push/pop CAS ile korunduğundan thread-safe'dir.
unsafe impl Send for StackNode {}

/// Lock-free global iş kuyruğu
///
/// İç yapı: Treiber Stack (singly-linked list + atomic head pointer)
/// - Push: yeni node → next = old head → CAS(head, old, new)
/// - Pop:  old = head → next = old.next → CAS(head, old, next)
struct LockFreeWorkQueue {
    /// Stack'in tepesi — atomik CAS ile güncellenir
    head: AtomicPtr<StackNode>,
    /// Bekleyen iş sayısı (yaklaşık, diagnostik amaçlı)
    len: AtomicUsize,
}

// LockFreeWorkQueue static olarak kullanılacak, Send+Sync garantisi gerekli.
unsafe impl Send for LockFreeWorkQueue {}
unsafe impl Sync for LockFreeWorkQueue {}

impl LockFreeWorkQueue {
    /// Derleme zamanı sabit oluşturma — `static` için gerekli
    const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
            len: AtomicUsize::new(0),
        }
    }

    /// Lock-free push — CAS döngüsü ile atomik ekleme.
    ///
    /// Birden fazla CPU/thread eş zamanlı çağırabilir.
    /// CAS başarısız olursa (başka bir üretici araya girdiyse) yeniden dener.
    fn push(&self, f: Box<dyn FnOnce() + Send>) {
        let node = Box::into_raw(Box::new(StackNode {
            func: Some(f),
            next: ptr::null_mut(),
        }));

        loop {
            let old_head = self.head.load(Ordering::Acquire);
            // Yeni node'un next'ini mevcut head'e bağla
            unsafe {
                (*node).next = old_head;
            }

            // Atomik CAS: head'i old → new ile değiştir
            if self
                .head
                .compare_exchange_weak(old_head, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                self.len.fetch_add(1, Ordering::Relaxed);
                return;
            }
            // CAS başarısız — başka bir üretici araya girdi, tekrar dene
            core::hint::spin_loop();
        }
    }

    /// Lock-free pop — CAS ile atomik çıkarma.
    ///
    /// Birden fazla worker thread eş zamanlı çağırabilir.
    /// CAS başarısız olursa (başka bir tüketici aldıysa) yeniden dener.
    fn pop(&self) -> Option<Box<dyn FnOnce() + Send>> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head.is_null() {
                return None;
            }

            let next = unsafe { (*head).next };

            // Atomik CAS: head'i current → next ile değiştir
            if self
                .head
                .compare_exchange_weak(head, next, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                self.len.fetch_sub(1, Ordering::Relaxed);
                let mut node = unsafe { Box::from_raw(head) };
                return node.func.take();
            }
            // CAS başarısız — başka bir tüketici aldı, tekrar dene
            core::hint::spin_loop();
        }
    }

    /// Kuyruktaki bekleyen iş sayısını döndürür (yaklaşık).
    ///
    /// Eş zamanlı push/pop nedeniyle kesin değer garanti edilmez.
    /// Diagnostik ve monitoring amaçlıdır.
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Global lock-free iş kuyruğu — **Mutex SIFIR**
static GLOBAL_WORK_QUEUE: LockFreeWorkQueue = LockFreeWorkQueue::new();

/// Aktif worker thread sayısı
static WORKER_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Belirtilen sayıda worker thread başlatır.
///
/// Her worker thread sonsuz döngüde global kuyruktan iş çeker (lock-free CAS).
pub fn init_workers(count: usize) {
    for _ in 0..count {
        crate::task::scheduler::spawn(worker_loop_entry);
        WORKER_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

/// Kuyruğa yeni bir asenkron görev ekler — **LOCK-FREE**.
///
/// Birden fazla CPU/thread eş zamanlı çağırabilir.
/// İç mekanizma: Treiber Stack üzerinde CAS (Compare-And-Swap).
///
/// # Örnek
/// ```
/// spawn_work(move || {
///     // io_uring SQE işlemi
///     let result = do_async_io();
///     // CQE'ye yaz
/// });
/// ```
pub fn spawn_work<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    GLOBAL_WORK_QUEUE.push(Box::new(f));
}

/// Worker thread ana döngüsü — **LOCK-FREE**.
///
/// 1. Global Treiber Stack'ten CAS ile iş çek
/// 2. İş varsa çalıştır
/// 3. İş yoksa CPU'yu diğer thread'lere bırak (sleep)
fn worker_loop_entry() -> ! {
    loop {
        if let Some(f) = GLOBAL_WORK_QUEUE.pop() {
            // İşi çalıştır
            f();
        } else {
            // İş yoksa CPU'yu diğer thread'lere sal (yield/sleep)
            crate::task::scheduler::sleep(1);
        }
    }
}

/// Bekleyen iş sayısını döndürür (yaklaşık, diagnostik amaçlı).
pub fn pending_work_count() -> usize {
    GLOBAL_WORK_QUEUE.len()
}

// ────────────────────────────────────────────────────────────
// WorkFence — İş Grubunu Senkronize Bariyeri
// ────────────────────────────────────────────────────────────
//
// Compositor gibi frame-sync gereken sistemlerde kullanılır:
//   1. N adet dirty tile render işi spawn et
//   2. fence.wait() ile hepsinin bitmesini bekle
//   3. Framebuffer'ı present et
//
// İç yapı: AtomicUsize sayaç (remaining), spawn edilen her iş
// bitiğinde fetch_sub(1) ile azaltır. wait() sayaç 0 olana kadar
// spin-yield döngüsünde bekler.

use alloc::sync::Arc;

/// Frame senkronizasyon bariyeri
///
/// Birden fazla iş grubunu beklemek için kullanılır.
/// `remaining` sıfıra düştüğünde tüm işler tamamlanmıştır.
pub struct WorkFence {
    remaining: Arc<AtomicUsize>,
}

impl WorkFence {
    /// Yeni bariyer oluşturur; sayaç = 0.
    pub fn new() -> Self {
        Self {
            remaining: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Bariyerli iş gönderir — iş bittiğinde sayaç otomatik azalır.
    ///
    /// İç mekanizma: closure'u Arc<AtomicUsize> ile sarar,
    /// iş bittiğinde `fetch_sub(1)` ile sayacı düşürür.
    pub fn spawn_fenced<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.remaining.fetch_add(1, Ordering::SeqCst);
        let counter = Arc::clone(&self.remaining);
        GLOBAL_WORK_QUEUE.push(Box::new(move || {
            f();
            counter.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    /// Tüm bariyerli işlerin bitmesini bekler (spin-yield).
    ///
    /// Bu fonksiyon sayaç 0 olana kadar bloke eder.
    /// Compositor frame sync ve batch render için kullanılır.
    pub fn wait(&self) {
        while self.remaining.load(Ordering::SeqCst) > 0 {
            core::hint::spin_loop();
            // Diğer thread'lere fırsat ver
            crate::task::scheduler::sleep(0);
        }
    }

    /// Bekleyen iş sayısını döndürür.
    pub fn pending(&self) -> usize {
        self.remaining.load(Ordering::SeqCst)
    }

    /// Bariyer sıfırlandı mı (tüm işler bitti mi)?
    pub fn is_complete(&self) -> bool {
        self.remaining.load(Ordering::SeqCst) == 0
    }
}

/// Birden fazla işi toplu olarak kuyruğa ekler.
///
/// Her iş bağımsızdır; bariyer eklenmez.
/// N adet CAS push tek tek yapılır ama API daha temizdir.
pub fn spawn_batch<I, F>(jobs: I)
where
    I: IntoIterator<Item = F>,
    F: FnOnce() + Send + 'static,
{
    for job in jobs {
        spawn_work(job);
    }
}
