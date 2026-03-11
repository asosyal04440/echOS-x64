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

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;
use x86_64::VirtAddr;

/// Ghost Task Durum Bayrakları
pub mod flags {
    pub const GHOST_TASK_BLOCKED: u32 = 1 << 0; // Görev bloklandı (I/O veya kilit bekleniyor)
    pub const GHOST_TASK_RUNNING: u32 = 1 << 1; // Görev CPU'da çalışıyor
    pub const GHOST_TASK_YIELD: u32 = 1 << 2; // Görev gönüllü CPU'yu bıraktı
    pub const GHOST_TASK_PREEMPT: u32 = 1 << 3; // Görev zorla preempt edildi
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
    pub producer_head: AtomicU32, // Kernel buraya yazar (üretici ucu)
    pub consumer_tail: AtomicU32, // Agent buradan okur (tüketici ucu)
    pub capacity: u32,            // Kuyruk kapasitesi (eleman sayısı)
    pub _padding: u32,
}

/// Kuyruk Elemanı (Mesaj)
/// Kernel'ın agent'a gönderdiği olay bildirimi.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct GhostMessage {
    pub msg_type: u32, // Mesaj türü (MSG_TASK_* sabitleri)
    pub task_id: u64,  // Hangi görevle ilgili
    pub payload: u64,  // Ek veri (örn. uyandırma nedeni, CPU ID)
}

pub const MSG_TASK_NEW: u32 = 1; // Yeni görev oluşturuldu → agent kaydetmeli
pub const MSG_TASK_BLOCKED: u32 = 2; // Görev bloklandı → agent başka birini seçmeli
pub const MSG_TASK_WAKEUP: u32 = 3; // Görev uyandı → agent yeniden kuyruğa alabilir
pub const MSG_TASK_PREEMPT: u32 = 4; // Görev preempt edildi → agent kararını gözden geçirmeli
pub const MSG_TASK_DEAD: u32 = 5; // Görev sonlandı → agent kaynaklarını temizlemeli
pub const MSG_POLICY_ACCEPTED: u32 = 6; // Policy lease kabul edildi
pub const MSG_POLICY_REJECTED: u32 = 7; // Policy lease reddedildi
pub const MSG_POLICY_EXPIRED: u32 = 8; // Policy lease süresi doldu

const GHOST_QUEUE_CAPACITY: u32 = 256;
const MAX_GHOST_POLICY_CPUS: usize = 8192;

#[repr(align(64))]
pub struct GhostRing {
    header: GhostQueueHeader,
    entries: UnsafeCell<[GhostMessage; GHOST_QUEUE_CAPACITY as usize]>,
    dropped: AtomicU64,
}

unsafe impl Send for GhostRing {}
unsafe impl Sync for GhostRing {}

impl GhostQueueHeader {
    pub const fn new(capacity: u32) -> Self {
        Self {
            producer_head: AtomicU32::new(0),
            consumer_tail: AtomicU32::new(0),
            capacity,
            _padding: 0,
        }
    }
}

impl GhostRing {
    pub const fn new() -> Self {
        Self {
            header: GhostQueueHeader::new(GHOST_QUEUE_CAPACITY),
            entries: UnsafeCell::new([GhostMessage {
                msg_type: 0,
                task_id: 0,
                payload: 0,
            }; GHOST_QUEUE_CAPACITY as usize]),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn push(&self, message: GhostMessage) -> bool {
        let head = self.header.producer_head.load(Ordering::Acquire);
        let tail = self.header.consumer_tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) >= self.header.capacity {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = (head % self.header.capacity) as usize;
        unsafe {
            (*self.entries.get())[slot] = message;
        }
        self.header
            .producer_head
            .store(head.wrapping_add(1), Ordering::Release);
        true
    }

    pub fn pop(&self) -> Option<GhostMessage> {
        let tail = self.header.consumer_tail.load(Ordering::Acquire);
        let head = self.header.producer_head.load(Ordering::Acquire);
        if tail == head {
            return None;
        }
        let slot = (tail % self.header.capacity) as usize;
        let message = unsafe { (*self.entries.get())[slot] };
        self.header
            .consumer_tail
            .store(tail.wrapping_add(1), Ordering::Release);
        Some(message)
    }

    pub fn pending_count(&self) -> u32 {
        self.header
            .producer_head
            .load(Ordering::Acquire)
            .wrapping_sub(self.header.consumer_tail.load(Ordering::Acquire))
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Acquire)
    }
}

static GHOST_EVENT_QUEUE: GhostRing = GhostRing::new();

lazy_static::lazy_static! {
    static ref GHOST_AGENTS: Mutex<BTreeMap<u64, Arc<Mutex<GhostAgent>>>> = Mutex::new(BTreeMap::new());
    static ref GHOST_POLICY_MAILBOXES: Vec<GhostPolicyMailbox> = {
        let mut mailboxes = Vec::with_capacity(MAX_GHOST_POLICY_CPUS);
        for _ in 0..MAX_GHOST_POLICY_CPUS {
            mailboxes.push(GhostPolicyMailbox::new());
        }
        mailboxes
    };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GhostPolicyDecision {
    pub agent_pid: u64,
    pub cpu_id: u32,
    pub task_id: u64,
    pub vruntime: u64,
    pub priority_boost: u32,
    pub generation: u64,
    pub lease_until_tick: u64,
    pub policy_token: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GhostPolicySnapshot {
    pub committed: u64,
    pub dispatched: u64,
    pub rejected: u64,
    pub expired: u64,
    pub active_generation: u64,
    pub active_task_id: u64,
    pub lease_until_tick: u64,
}

#[repr(align(64))]
struct GhostPolicyMailbox {
    valid: AtomicBool,
    committed: AtomicU64,
    dispatched: AtomicU64,
    rejected: AtomicU64,
    expired: AtomicU64,
    active_generation: AtomicU64,
    decision: UnsafeCell<GhostPolicyDecision>,
}

unsafe impl Send for GhostPolicyMailbox {}
unsafe impl Sync for GhostPolicyMailbox {}

impl GhostPolicyMailbox {
    fn new() -> Self {
        Self {
            valid: AtomicBool::new(false),
            committed: AtomicU64::new(0),
            dispatched: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            expired: AtomicU64::new(0),
            active_generation: AtomicU64::new(0),
            decision: UnsafeCell::new(GhostPolicyDecision::default()),
        }
    }

    fn snapshot(&self) -> GhostPolicySnapshot {
        let decision = unsafe { *self.decision.get() };
        GhostPolicySnapshot {
            committed: self.committed.load(Ordering::Acquire),
            dispatched: self.dispatched.load(Ordering::Acquire),
            rejected: self.rejected.load(Ordering::Acquire),
            expired: self.expired.load(Ordering::Acquire),
            active_generation: self.active_generation.load(Ordering::Acquire),
            active_task_id: decision.task_id,
            lease_until_tick: decision.lease_until_tick,
        }
    }
}

fn mailbox(cpu_id: u32) -> Option<&'static GhostPolicyMailbox> {
    GHOST_POLICY_MAILBOXES.get(cpu_id as usize)
}

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

    pub fn update_task_status(
        &self,
        task_id: u64,
        flags: u32,
        cpu_id: u32,
        vruntime: u64,
    ) -> bool {
        let Some(status) = self.tasks.iter().find(|status| status.task_id == task_id) else {
            return false;
        };
        status.flags.store(flags, Ordering::Release);
        status.cpu_id.store(cpu_id, Ordering::Release);
        status.vruntime.store(vruntime, Ordering::Release);
        true
    }

    pub fn push_event(&self, msg_type: u32, task_id: u64, payload: u64) -> bool {
        GHOST_EVENT_QUEUE.push(GhostMessage {
            msg_type,
            task_id,
            payload,
        })
    }

    pub fn drain_events(&self, max_events: usize) -> Vec<GhostMessage> {
        let mut events = Vec::new();
        while events.len() < max_events {
            let Some(message) = GHOST_EVENT_QUEUE.pop() else {
                break;
            };
            events.push(message);
        }
        events
    }

    pub fn commit(&self, cpu_id: u32, task_id: u64, vruntime: u64) -> bool {
        let priority_boost = self
            .tasks
            .iter()
            .find(|status| status.task_id == task_id)
            .map(|status| status.priority.load(Ordering::Acquire))
            .unwrap_or(100);
        commit_policy(self.pid, cpu_id, task_id, vruntime, priority_boost, 8)
    }

    pub fn watchdog_snapshot(&self) -> (u32, u64) {
        (
            GHOST_EVENT_QUEUE.pending_count(),
            GHOST_EVENT_QUEUE.dropped_count(),
        )
    }
}

pub fn register_agent(
    pid: u64,
    status_page: VirtAddr,
    queue_page: VirtAddr,
) -> Arc<Mutex<GhostAgent>> {
    let agent = Arc::new(Mutex::new(GhostAgent::new(pid, status_page, queue_page)));
    GHOST_AGENTS.lock().insert(pid, agent.clone());
    agent
}

pub fn get_agent(pid: u64) -> Option<Arc<Mutex<GhostAgent>>> {
    GHOST_AGENTS.lock().get(&pid).cloned()
}

pub fn publish_event(msg_type: u32, task_id: u64, payload: u64) -> bool {
    GHOST_EVENT_QUEUE.push(GhostMessage {
        msg_type,
        task_id,
        payload,
    })
}

pub fn pending_events() -> u32 {
    GHOST_EVENT_QUEUE.pending_count()
}

pub fn dropped_events() -> u64 {
    GHOST_EVENT_QUEUE.dropped_count()
}

pub fn task_cpu_hint(task_id: u64) -> Option<u32> {
    let agents = GHOST_AGENTS.lock();
    for agent in agents.values() {
        let guard = agent.lock();
        if let Some(status) = guard.tasks.iter().find(|status| status.task_id == task_id) {
            let cpu_id = status.cpu_id.load(Ordering::Acquire);
            if cpu_id != u32::MAX {
                return Some(cpu_id);
            }
        }
    }
    None
}

pub fn commit_policy(
    agent_pid: u64,
    cpu_id: u32,
    task_id: u64,
    vruntime: u64,
    priority_boost: u32,
    lease_ticks: u64,
) -> bool {
    let Some(mailbox) = mailbox(cpu_id) else {
        return false;
    };

    if get_agent(agent_pid).is_none() {
        mailbox.rejected.fetch_add(1, Ordering::AcqRel);
        let _ = publish_event(MSG_POLICY_REJECTED, task_id, cpu_id as u64);
        return false;
    }

    let proof = match crate::valkyrie_virt::validate_scheduler_policy(
        cpu_id,
        task_id,
        lease_ticks,
        priority_boost,
    ) {
        Ok(proof) => proof,
        Err(_) => {
            mailbox.rejected.fetch_add(1, Ordering::AcqRel);
            let _ = publish_event(MSG_POLICY_REJECTED, task_id, cpu_id as u64);
            return false;
        }
    };

    let generation = mailbox.active_generation.fetch_add(1, Ordering::AcqRel) + 1;
    let decision = GhostPolicyDecision {
        agent_pid,
        cpu_id,
        task_id,
        vruntime,
        priority_boost,
        generation,
        lease_until_tick: crate::interrupts::get_ticks().saturating_add(lease_ticks),
        policy_token: proof.policy_token,
    };

    unsafe {
        *mailbox.decision.get() = decision;
    }
    core::sync::atomic::fence(Ordering::Release);
    mailbox.valid.store(true, Ordering::Release);
    mailbox.committed.fetch_add(1, Ordering::AcqRel);
    let _ = publish_event(MSG_POLICY_ACCEPTED, task_id, generation);
    true
}

pub fn active_policy(cpu_id: u32, now_tick: u64) -> Option<GhostPolicyDecision> {
    let mailbox = mailbox(cpu_id)?;
    if !mailbox.valid.load(Ordering::Acquire) {
        return None;
    }

    let decision = unsafe { *mailbox.decision.get() };
    if decision.lease_until_tick < now_tick {
        mailbox.valid.store(false, Ordering::Release);
        mailbox.expired.fetch_add(1, Ordering::AcqRel);
        let _ = publish_event(MSG_POLICY_EXPIRED, decision.task_id, decision.generation);
        return None;
    }

    Some(decision)
}

pub fn note_policy_dispatch(cpu_id: u32, task_id: u64, generation: u64) -> bool {
    let Some(mailbox) = mailbox(cpu_id) else {
        return false;
    };
    if !mailbox.valid.load(Ordering::Acquire) {
        return false;
    }

    let decision = unsafe { *mailbox.decision.get() };
    if decision.task_id != task_id || decision.generation != generation {
        return false;
    }

    mailbox.valid.store(false, Ordering::Release);
    mailbox.dispatched.fetch_add(1, Ordering::AcqRel);
    true
}

pub fn policy_snapshot(cpu_id: u32) -> Option<GhostPolicySnapshot> {
    mailbox(cpu_id).map(|mailbox| mailbox.snapshot())
}
