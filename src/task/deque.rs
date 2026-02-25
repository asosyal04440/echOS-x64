use alloc::sync::Arc;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};
use core::ptr;

// Basit ve sabit boyutlu bir Chase-Lev Deque implementasyonu.
// Gerçek hayatta buffer resizing gerekir ama şimdilik sabit boyut yeterli.
// Kapasite artırıldı: 4096 (Tier 1 OS standardı)
const DEQUE_SIZE: usize = 4096;

pub struct Worker<T> {
    inner: Arc<Inner<T>>,
}

pub struct Stealer<T> {
    inner: Arc<Inner<T>>,
}

// İç yapı
struct Inner<T> {
    buffer: [AtomicPtr<T>; DEQUE_SIZE],
    top: AtomicIsize,    // Stealer'lar buradan çalar (steal)
    bottom: AtomicIsize, // Worker buradan ekler/alır (push/pop)
}

// Inner için Send/Sync
unsafe impl<T: Send> Send for Inner<T> {}
unsafe impl<T: Send> Sync for Inner<T> {}

impl<T> Worker<T> {
    pub fn new() -> (Worker<T>, Stealer<T>) {
        // Buffer'ı null pointerlar ile başlat
        // AtomicPtr null pointer (0) ile başlatılabilir.
        // x86_64 mimarisinde null pointer her zaman 0'dır, bu yüzden zeroed güvenlidir.
        let mut buffer: [AtomicPtr<T>; DEQUE_SIZE] = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        
        let inner = Arc::new(Inner {
            buffer,
            top: AtomicIsize::new(0),
            bottom: AtomicIsize::new(0),
        });

        (
            Worker { inner: inner.clone() },
            Stealer { inner },
        )
    }

    pub fn push(&self, task: Box<T>) {
        let b = self.inner.bottom.load(Ordering::Relaxed);
        let t = self.inner.top.load(Ordering::Acquire);
        
        if (b.wrapping_sub(t)) as usize >= DEQUE_SIZE {
            // Buffer dolu!
            panic!("Worker deque full! Cannot scale beyond 4096 tasks per CPU without resizing."); 
        }

        let task_ptr = Box::into_raw(task);
        let idx = (b as usize) % DEQUE_SIZE;
        self.inner.buffer[idx].store(task_ptr, Ordering::Relaxed);
        
        core::sync::atomic::fence(Ordering::Release);
        self.inner.bottom.store(b.wrapping_add(1), Ordering::Relaxed);
    }

    pub fn pop(&self) -> Option<Box<T>> {
        let b = self.inner.bottom.load(Ordering::Relaxed).wrapping_sub(1);
        self.inner.bottom.store(b, Ordering::Relaxed);
        core::sync::atomic::fence(Ordering::SeqCst);
        
        let t = self.inner.top.load(Ordering::Relaxed);
        
        if t <= b {
            // Kuyruk boş değil
            let idx = (b as usize) % DEQUE_SIZE;
            let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);
            
            if t == b {
                // Son eleman, yarış durumu (race) olabilir
                // Compare-And-Swap (CAS)
                if self.inner.top.compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                    self.inner.bottom.store(b.wrapping_add(1), Ordering::Relaxed);
                    return Some(unsafe { Box::from_raw(task_ptr) });
                } else {
                    // Yarışı kaybettik, başkası çaldı
                    self.inner.bottom.store(b.wrapping_add(1), Ordering::Relaxed);
                    return None;
                }
            }
            
            return Some(unsafe { Box::from_raw(task_ptr) });
        } else {
            // Kuyruk boştu
            self.inner.bottom.store(b.wrapping_add(1), Ordering::Relaxed);
            return None;
        }
    }

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
    pub fn steal(&self) -> Option<Box<T>> {
        let t = self.inner.top.load(Ordering::Acquire);
        core::sync::atomic::fence(Ordering::SeqCst);
        let b = self.inner.bottom.load(Ordering::Acquire);
        
        if t < b {
            let idx = (t as usize) % DEQUE_SIZE;
            let task_ptr = self.inner.buffer[idx].load(Ordering::Relaxed);
            
            if self.inner.top.compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                return Some(unsafe { Box::from_raw(task_ptr) });
            }
        }
        
        None
    }
}
