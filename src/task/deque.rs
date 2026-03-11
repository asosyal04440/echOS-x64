//! # Chase-Lev Lock-Free Work-Stealing Deque (Çift Uçlu Kuyruk)
//!
//! Bu modül, SMP (Çok İşlemcili) sistemlerde iş yükü dengemesi için
//! kullanılan kilit-serbest (lock-free) Chase-Lev Deque algoritmasını uygular.
//!
//! ## Temel Fikir: Work Stealing (İş Çalma)
//!
//! ```text
//!  CPU 0 (meşgul)               CPU 1 (boşta)
//!  ┌──────────────┐              ┌──────────────┐
//!  │ Worker       │              │ Worker       │
//!  │ bottom=5     │              │ bottom=0     │
//!  │ [T1,T2,T3,T4,T5]           │ []           │
//!  │       ↑ pop()              │       ↑      │
//!  │       (LIFO, yerel erişim) │              │
//!  │                            │ Stealer      │
//!  │  ←───────────steal()──────┤ (T1'i çalar) │
//!  └──────────────┘              └──────────────┘
//!     CPU 0 sondan alır (pop),
//!     Stealer baştan alır (steal) — çakışma azaltılır!
//! ```
//!
//! ## Bellek Sıralaması (Memory Ordering)
//!
//! Bu yapı `Acquire`/`Release` bellek bariyerleri kullanır:
//! - `push`:  bottom Release (yeni eleman görünür olsun)
//! - `pop`:   SeqCst fence (son elemanla yarış durumu ele alınır)
//! - `steal`: Acquire/SeqCst (tutarlı okuma yapılır)
//!
//! ## Kaynak
//! "Dynamic Circular Work-Stealing Deque", Chase & Lev (2005)

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ptr;
use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};

// Sabit boyutlu Chase-Lev Deque uygulaması.
// Gerçek üretim sistemlerinde buffer yeniden boyutlandırma gerekir,
// ancak şimdilik sabit kapasiteli sürüm kullanılıyor.
// Kapasite: 4096 görev/CPU (Tier 1 OS standardı)
const DEQUE_SIZE: usize = 4096;

/// Yerel (local) işlemci tarafından kullanılan Worker ucu.
/// Sadece sahibi olan iş parçacığı güvenle push/pop yapabilir.
pub struct Worker<T> {
    inner: Arc<Inner<T>>,
}

/// Uzak (remote) işlemcilerin iş çalmak için kullandığı Stealer ucu.
/// Birden fazla Stealer aynı anda çalışabilir (lock-free CAS ile).
pub struct Stealer<T> {
    inner: Arc<Inner<T>>,
}

// Worker ve Stealer'ın paylaştığı iç yapı
struct Inner<T> {
    buffer: [AtomicPtr<T>; DEQUE_SIZE],
    top: AtomicIsize,    // Stealer'lar buradan çalar (steal) — baştan okur
    bottom: AtomicIsize, // Worker buradan ekler/alır (push/pop) — sondan okur
}

// Inner yapısının farklı thread'lere güvenle gönderilmesine izin ver
unsafe impl<T: Send> Send for Inner<T> {}
unsafe impl<T: Send> Sync for Inner<T> {}

impl<T> Worker<T> {
    /// Yeni bir Worker/Stealer çifti oluşturur.
    /// İkisi de aynı iç tamponu paylaşır (Arc ile referans sayılı).
    pub fn new() -> (Worker<T>, Stealer<T>) {
        // Tamponu sıfır-başlatılmış null pointer'larla hazırla.
        // AtomicPtr null pointer (0) ile başlatılabilir.
        // x86_64 mimarisinde null pointer her zaman 0'dır, bu yüzden zeroed güvenlidir.
        let mut buffer: [AtomicPtr<T>; DEQUE_SIZE] =
            unsafe { core::mem::MaybeUninit::zeroed().assume_init() };

        let inner = Arc::new(Inner {
            buffer,
            top: AtomicIsize::new(0),
            bottom: AtomicIsize::new(0),
        });

        (
            Worker {
                inner: inner.clone(),
            },
            Stealer { inner },
        )
    }

    /// Kuyruğun sonuna (bottom) yeni bir görev ekler.
    /// Sadece sahibi Worker tarafından çağrılabilir (tek üretici).
    pub fn push(&self, task: Box<T>) {
        let b = self.inner.bottom.load(Ordering::Relaxed);
        let t = self.inner.top.load(Ordering::Acquire);

        if (b.wrapping_sub(t)) as usize >= DEQUE_SIZE {
            // Tampon doldu! Daha fazla görev kabul edilemiyor.
            panic!("Worker deque full! Cannot scale beyond 4096 tasks per CPU without resizing.");
        }

        let task_ptr = Box::into_raw(task);
        let idx = (b as usize) % DEQUE_SIZE;
        self.inner.buffer[idx].store(task_ptr, Ordering::Relaxed);

        // Release fence: önceki bellek yazmaları yayınlanır
        core::sync::atomic::fence(Ordering::Release);
        self.inner
            .bottom
            .store(b.wrapping_add(1), Ordering::Relaxed);
    }

    /// Kuyruğun sonundan (bottom) bir görev alır — LIFO davranışı.
    /// Sadece sahibi Worker tarafından çağrılabilir.
    ///
    /// Son eleman için Stealer ile yarış durumu oluşabilir;
    /// bu durumda CAS (Compare-And-Swap) ile çözülür.
    pub fn pop(&self) -> Option<Box<T>> {
        let b = self.inner.bottom.load(Ordering::Relaxed).wrapping_sub(1);
        self.inner.bottom.store(b, Ordering::Relaxed);
        core::sync::atomic::fence(Ordering::SeqCst);

        let t = self.inner.top.load(Ordering::Relaxed);

        if t <= b {
            // Kuyruk boş değil — normal durum
            let idx = (b as usize) % DEQUE_SIZE;
            let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);

            if t == b {
                // Son eleman: Stealer ile yarış olabilir!
                // CAS ile sahipliği atomik olarak talep et
                if self
                    .inner
                    .top
                    .compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
                {
                    self.inner
                        .bottom
                        .store(b.wrapping_add(1), Ordering::Relaxed);
                    return Some(unsafe { Box::from_raw(task_ptr) });
                } else {
                    // Yarışı kaybettik, bir Stealer bu görevi aldı
                    self.inner
                        .bottom
                        .store(b.wrapping_add(1), Ordering::Relaxed);
                    return None;
                }
            }

            return Some(unsafe { Box::from_raw(task_ptr) });
        } else {
            // Kuyruk boştu — bottom'u geri al
            self.inner
                .bottom
                .store(b.wrapping_add(1), Ordering::Relaxed);
            return None;
        }
    }

    /// Kuyruktaki mevcut görev sayısını döndürür.
    pub fn len(&self) -> usize {
        let b = self.inner.bottom.load(Ordering::Relaxed);
        let t = self.inner.top.load(Ordering::Relaxed);
        if b < t {
            0
        } else {
            (b - t) as usize
        }
    }
}

impl<T> Stealer<T> {
    /// Kuyruğun başından (top) bir görev çalar — FIFO davranışı.
    /// Birden fazla uzak Stealer aynı anda çağırabilir; CAS ile çakışmalar önlenir.
    ///
    /// CAS başarısız olursa None döner (başka bir Stealer önce aldı demektir).
    pub fn steal(&self) -> Option<Box<T>> {
        let t = self.inner.top.load(Ordering::Acquire);
        core::sync::atomic::fence(Ordering::SeqCst);
        let b = self.inner.bottom.load(Ordering::Acquire);

        if t < b {
            let idx = (t as usize) % DEQUE_SIZE;
            let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);

            // Atomik karşılaştırma-değiştirme: top'u t'den t+1'e güncelle
            if self
                .inner
                .top
                .compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return Some(unsafe { Box::from_raw(task_ptr) });
            }
        }

        None
    }
}
