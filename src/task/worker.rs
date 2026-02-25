//! # echOS io_uring / Asenkron Kernel Worker Havuzu
//!
//! Bu modül, io_uring ve genel amaçlı asenkron iş yükleri için
//! bir kernel thread (kthread) havuzu (workqueue) sağlar.
//! `sys_io_uring_enter` çağrıları yüklerini buraya devreder.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicUsize, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

/// Kuyruğa atılacak genel geçer iş (closure)
pub type WorkItem = Box<dyn FnOnce() + Send>;

lazy_static! {
    /// Global iş kuyruğu
    static ref WORK_QUEUE: Mutex<VecDeque<WorkItem>> = Mutex::new(VecDeque::new());
}

static WORKER_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Belirtilen sayıda worker thread başlatır.
pub fn init_workers(count: usize) {
    for _ in 0..count {
        crate::task::scheduler::spawn(worker_loop_entry);
        WORKER_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

/// Kuyruğa yeni bir iş (asenkron görev) ekler.
pub fn spawn_work<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    WORK_QUEUE.lock().push_back(Box::new(f));
}

/// Worker thread ana döngüsü.
fn worker_loop_entry() -> ! {
    loop {
        let work = {
            let mut q = WORK_QUEUE.lock();
            q.pop_front()
        };

        if let Some(f) = work {
            // İşi çalıştır
            f();
        } else {
            // İş yoksa CPU'yu diğer thread'lere sal (yield/sleep)
            crate::task::scheduler::sleep(1);
        }
    }
}
