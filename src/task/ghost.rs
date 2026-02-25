//! # Google GhOSt (Global Herbert Operating System Threading) Benzeri Userspace Scheduling
//!
//! Bu modül, çekirdek (kernel) zamanlayıcısının kararlarını kullanıcı alanına (userspace)
//! devretmesine olanak tanıyan altyapıyı sağlar.
//!
//! ## M:N Threading Modeli
//! - **Kernel Thread (KSE)**: Çekirdek tarafından yönetilen, fiziksel CPU üzerinde çalışan thread.
//! - **User Thread (Task)**: Kullanıcı alanı tarafından yönetilen, KSE üzerinde çalışan "hafif" thread.
//!
//! ## Temel Bileşenler
//! 1. **Shared Status Word**: Her thread için kernel ve userspace arasında paylaşılan durum bilgisi.
//! 2. **Message Queue**: Kernel'dan userspace'e olay (event) bildirimi (örn. thread bloklandı, uyandı).
//! 3. **Commit**: Userspace'in kernel'a "bu thread'i çalıştır" emri.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use alloc::sync::Arc;
use alloc::vec::Vec;
use x86_64::VirtAddr;

/// Ghost Task Durum Bayrakları
pub mod flags {
    pub const GHOST_TASK_BLOCKED: u32 = 1 << 0;
    pub const GHOST_TASK_RUNNING: u32 = 1 << 1;
    pub const GHOST_TASK_YIELD: u32 = 1 << 2;
    pub const GHOST_TASK_PREEMPT: u32 = 1 << 3;
}

/// Kernel ve Userspace arasında paylaşılan Task Durumu.
/// `repr(C)` olmak zorunda çünkü userspace C/C++ veya Rust ile bu yapıya erişecek.
#[repr(C)]
#[derive(Debug)]
pub struct GhostTaskStatus {
    /// Task'ın benzersiz ID'si
    pub task_id: u64,
    /// Durum bayrakları (Atomic)
    pub flags: AtomicU32,
    /// Task'ın çalıştığı CPU ID (veya son çalıştığı)
    pub cpu_id: AtomicU32,
    /// Task'ın önceliği (Userspace belirler)
    pub priority: AtomicU32,
    /// Sanal çalışma süresi (vruntime)
    pub vruntime: AtomicU64,
}

/// Kernel -> Userspace Mesaj Kuyruğu Başlığı
#[repr(C)]
pub struct GhostQueueHeader {
    pub producer_head: AtomicU32,
    pub consumer_tail: AtomicU32,
    pub capacity: u32,
    pub _padding: u32,
}

/// Kuyruk Elemanı (Mesaj)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GhostMessage {
    pub msg_type: u32,
    pub task_id: u64,
    pub payload: u64,
}

pub const MSG_TASK_NEW: u32 = 1;
pub const MSG_TASK_BLOCKED: u32 = 2;
pub const MSG_TASK_WAKEUP: u32 = 3;
pub const MSG_TASK_PREEMPT: u32 = 4;
pub const MSG_TASK_DEAD: u32 = 5;

/// Ghost Agent (Userspace Scheduler Process) Kontrol Yapısı
pub struct GhostAgent {
    pub pid: u64,
    pub status_page: VirtAddr, // Userspace sanal adresi
    pub queue_page: VirtAddr,  // Userspace sanal adresi
    pub tasks: Vec<Arc<GhostTaskStatus>>,
}

impl GhostAgent {
    pub fn new(pid: u64, status_page: VirtAddr, queue_page: VirtAddr) -> Self {
        Self {
            pid,
            status_page,
            queue_page,
            tasks: Vec::new(),
        }
    }

    /// Yeni bir Ghost Task kaydeder
    pub fn register_task(&mut self, task_id: u64) -> Arc<GhostTaskStatus> {
        let status = Arc::new(GhostTaskStatus {
            task_id,
            flags: AtomicU32::new(0),
            cpu_id: AtomicU32::new(u32::MAX),
            priority: AtomicU32::new(100),
            vruntime: AtomicU64::new(0),
        });
        
        self.tasks.push(status.clone());
        status
    }
    
    // Mesaj kuyruğuna yazma ve okuma işlemleri burada implement edilecek
    // Kernel tarafı "Producer", Userspace tarafı "Consumer"
}
