//! # Google GhOSt (Global Herbert Operating System Threading) Benzeri Kullanıcı Alanı Zamanlayıcısı
//!
//! Bu modül, çekirdek (kernel) zamanlayıcısının kararlarını kullanıcı alanına (userspace)
//! devretmesine olanak tanıyan altyapıyı sağlar.
//!
//! ## Ghost Zamanlama Nedir?
//!
//! Geleneksel çekirdek zamanlayıcıları (CFS, EDF, FIFO) değiştirilmesi zor ve test edilmesi
//! güçtür. GhOSt fikri şudur: Zamanlama kararlarını **kullanıcı alanı process'ine** dev-et.
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────┐
//!  │             GHOST ZAMANLAYICISı MİMARİSİ                │
//!  │                                                          │
//!  │  Kullanıcı Alanı (Ghost Agent Process)                  │
//!  │  ┌──────────────────────────────────────────┐           │
//!  │  │  zamanlayici.rs / agent.py / agent.go    │           │
//!  │  │  - Hangi thread çalışsın? BUNU BİZ seçeriz│          │
//!  │  │  - commit("T3'ü CPU1'de çalıştır")        │          │
//!  │  └────────────┬────────────▲────────────────┘           │
//!  │               │ commit     │ mesaj (event)               │
//!  │  ─────────────┼────────────┼──────────────────────────  │
//!  │  Çekirdek     ▼            │                             │
//!  │  ┌──────────────────────────────────────────┐           │
//!  │  │  Ghost Çekirdek Modülü                  │           │
//!  │  │  - Paylaşımlı durum sayfası (mmap)      │           │
//!  │  │  - Mesaj kuyruğu (lock-free ring buffer) │          │
//!  │  │  - commit() sistem çağrısı              │           │
//!  │  │  → Gerçek context switch yapılır        │           │
//!  │  └──────────────────────────────────────────┘           │
//!  └──────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Mesaj Akışı (Kernel → Agent)
//!
//! ```text
//!  Thread T3 bloklandı (I/O bekleniyor)
//!        ↓
//!  Kernel mesaj kuyruğuna yaz: MSG_TASK_BLOCKED(T3)
//!        ↓
//!  Agent mesajı okur, yeni karar verir: "T7'yi çalıştır"
//!        ↓
//!  Agent commit() çağırır: commit(CPU=0, task=T7)
//!        ↓
//!  Kernel context switch yapar: T7 CPU0'da çalışır
//! ```
//!
//! ## M:N Threading Modeli
//! - **Kernel Thread (KSE)**: Çekirdek tarafından yönetilen, fiziksel CPU üzerinde çalışan thread.
//! - **User Thread (Task)**: Kullanıcı alanı tarafından yönetilen, KSE üzerinde çalışan "hafif" thread.
//!
//! ## Temel Bileşenler
//! 1. **Paylaşımlı Durum Sayfası**: Her thread için kernel ve userspace arasında paylaşılan durum bilgisi.
//!    (mmap ile paylaşılan bellek, sistem çağrısı gerekmeden okunabilir)
//! 2. **Mesaj Kuyruğu**: Kernel'dan userspace'e olay (event) bildirimi (örn. thread bloklandı, uyandı).
//!    Lock-free ring buffer — düşük gecikme
//! 3. **Commit**: Userspace'in kernel'a "bu thread'i çalıştır" emri.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use alloc::sync::Arc;
use alloc::vec::Vec;
use x86_64::VirtAddr;

/// Ghost Task Durum Bayrakları
pub mod flags {
    pub const GHOST_TASK_BLOCKED: u32 = 1 << 0;  // Görev bloklandı (I/O veya kilit bekleniyor)
    pub const GHOST_TASK_RUNNING: u32 = 1 << 1;  // Görev CPU'da çalışıyor
    pub const GHOST_TASK_YIELD: u32 = 1 << 2;    // Görev gönüllü CPU'yu bıraktı
    pub const GHOST_TASK_PREEMPT: u32 = 1 << 3;  // Görev zorla preempt edildi
}

/// Kernel ve Kullanıcı Alanı arasında paylaşılan Görev Durumu.
/// `repr(C)` olmak zorunda çünkü userspace C/C++ veya Rust ile bu yapıya erişecek.
/// Bu yapı mmap ile paylaşıldığı için sistem çağrısı olmadan okunabilir.
#[repr(C)]
#[derive(Debug)]
pub struct GhostTaskStatus {
    /// Görevin benzersiz kimlik numarası
    pub task_id: u64,
    /// Durum bayrakları (Atomik — kernel ve agent aynı anda okuyabilir)
    pub flags: AtomicU32,
    /// Görevin çalıştığı CPU kimliği (veya son çalıştığı)
    pub cpu_id: AtomicU32,
    /// Görevin önceliği (kullanıcı alanındaki agent belirler)
    pub priority: AtomicU32,
    /// Sanal çalışma süresi — agent kendi CFS benzeri algoritması için kullanabilir
    pub vruntime: AtomicU64,
}

/// Kernel → Kullanıcı Alanı Mesaj Kuyruğu Başlığı
/// Lock-free ring buffer yapısı: producer (kernel) ve consumer (agent) yarışmaz.
#[repr(C)]
pub struct GhostQueueHeader {
    pub producer_head: AtomicU32,  // Kernel buraya yazar (üretici ucu)
    pub consumer_tail: AtomicU32,  // Agent buradan okur (tüketici ucu)
    pub capacity: u32,             // Kuyruk kapasitesi (eleman sayısı)
    pub _padding: u32,
}

/// Kuyruk Elemanı (Mesaj)
/// Kernel'ın agent'a gönderdiği olay bildirimi.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GhostMessage {
    pub msg_type: u32,  // Mesaj türü (MSG_TASK_* sabitleri)
    pub task_id: u64,   // Hangi görevle ilgili
    pub payload: u64,   // Ek veri (örn. uyandırma nedeni, CPU ID)
}

pub const MSG_TASK_NEW: u32 = 1;      // Yeni görev oluşturuldu → agent kaydetmeli
pub const MSG_TASK_BLOCKED: u32 = 2;  // Görev bloklandı → agent başka birini seçmeli
pub const MSG_TASK_WAKEUP: u32 = 3;   // Görev uyandı → agent yeniden kuyruğa alabilir
pub const MSG_TASK_PREEMPT: u32 = 4;  // Görev preempt edildi → agent kararını gözden geçirmeli
pub const MSG_TASK_DEAD: u32 = 5;     // Görev sonlandı → agent kaynaklarını temizlemeli

/// Ghost Agent (Kullanıcı Alanı Zamanlayıcı Süreci) Kontrol Yapısı.
/// Her agent bir CPU veya CPU grubu için zamanlama kararları verir.
pub struct GhostAgent {
    pub pid: u64,
    pub status_page: VirtAddr, // Durum sayfasının kullanıcı alanı sanal adresi (mmap)
    pub queue_page: VirtAddr,  // Mesaj kuyruğunun kullanıcı alanı sanal adresi (mmap)
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

    /// Yeni bir Ghost Task kaydeder.
    /// Kernel, agent'a MSG_TASK_NEW mesajı gönderir; agent bu ID'yi kendi kuyruğuna alır.
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
    
    // Mesaj kuyruğuna yazma ve okuma işlemleri burada implement edilecek.
    // Kernel "üretici" (Producer), Kullanıcı Alanı "tüketici" (Consumer) rolündedir.
    // Paylaşımlı bellek (mmap) üzerinden sistem çağrısı gerektirmeden iletişim kurulur.
}
