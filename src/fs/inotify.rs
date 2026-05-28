//! # inotify - Dosya Değişikliği Bildirimi
//!
//! Dosya sistemi olaylarını izlemek için Linux uyumlu inotify alt sistemi.
//!
//! ## inotify Olay Akışı
//!
//! ```text
//!  Dosya sistemi işlemi
//!  (örn. write(), rename(), unlink())
//!          │
//!          ▼
//!  generate_event(inode, mask, cookie, name)
//!          │
//!          ▼
//!  watch_index'te inode için izleyicileri bul
//!          │
//!          ├── InotifyInstance[0].push_event(...)
//!          ├── InotifyInstance[1].push_event(...)
//!          └── InotifyInstance[n].push_event(...)
//!                       │
//!                       ▼
//!  Kullanıcı prosesi: read(inotify_fd, buf, size)
//!                       │
//!                       ▼
//!  ┌──────────────────────────────────────────┐
//!  │  InotifyEventRaw (16 bayt sabit kısım)   │
//!  │  [ wd | mask | cookie | name_len ]       │
//!  │  + name (name_len bayt, 8'e hizalanmış)  │
//!  └──────────────────────────────────────────┘
//!
//!  Yeniden adlandırma olayları eşleşmesi (cookie):
//!  old_parent: IN_MOVED_FROM (cookie=X)
//!  new_parent: IN_MOVED_TO   (cookie=X) ← aynı cookie
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// FS WATCH KEY AND EVENT (Wave 5.6 Notification Semantics)
// ============================================================================

/// Watch kind for filesystem event classification.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WatchKind {
    Inode,
    Mount,
    Directory,
}

/// Unique key identifying a filesystem watch target across namespaces.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FsWatchKey {
    pub namespace_id: u64,
    pub mount_id: u64,
    pub inode_id: u64,
    pub generation: u64,
    /// What kind of object is being watched (inode, mount, directory)
    pub watch_kind: WatchKind,
    /// Snapshot of the path at watch creation time
    pub watched_path_snapshot: String,
}

/// Error classification for filesystem notification events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsErrorClass {
    IoError,
    PermissionDenied,
    NoSpace,
    WatchOverflow,
    WatchRemoved,
}

/// A filesystem notification event with ordering and identity guarantees.
#[derive(Clone, Debug)]
pub struct FsEvent {
    pub seq_no: u64,
    pub watch_key: Option<FsWatchKey>,
    pub parent_key: Option<FsWatchKey>,
    pub cookie: u32,
    pub name_snapshot: String,
    pub old_name_snapshot: Option<String>,
    pub new_name_snapshot: Option<String>,
    pub is_dir: bool,
    pub error_class: Option<FsErrorClass>,
    pub mask: u32,
    pub wd: i32,
}

impl FsEvent {
    pub fn new(wd: i32, mask: u32, cookie: u32, name: &str) -> Self {
        Self {
            seq_no: next_seq(),
            watch_key: None,
            parent_key: None,
            cookie,
            name_snapshot: String::from(name),
            old_name_snapshot: None,
            new_name_snapshot: None,
            is_dir: false,
            error_class: None,
            mask,
            wd,
        }
    }

    pub fn with_error(wd: i32, mask: u32, error: FsErrorClass) -> Self {
        Self {
            seq_no: next_seq(),
            watch_key: None,
            parent_key: None,
            cookie: 0,
            name_snapshot: String::new(),
            old_name_snapshot: None,
            new_name_snapshot: None,
            is_dir: false,
            error_class: Some(error),
            mask,
            wd,
        }
    }
}

// Monotonic sequence counter for FsEvent ordering.
static FS_EVENT_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_seq() -> u64 {
    FS_EVENT_SEQ.fetch_add(1, Ordering::Relaxed)
}

// ============================================================================
// RENAME COOKIE MATCHING (Task 5.6.3)
// ============================================================================

/// Generates unique non-zero cookies for rename event pairing.
pub fn next_cookie() -> u32 {
    static COOKIE: AtomicU32 = AtomicU32::new(1);
    loop {
        let val = COOKIE.fetch_add(1, Ordering::Relaxed);
        if val != 0 {
            return val;
        }
    }
}

/// Pairs IN_MOVED_FROM / IN_MOVED_TO events by their shared cookie value.
///
/// Only events with non-zero cookies are considered. Each cookie produces at
/// most one pair; unpaired events are silently dropped from the result.
pub fn match_rename_events(events: &[FsEvent]) -> Vec<(FsEvent, FsEvent)> {
    let mut moved_from: BTreeMap<u32, usize> = BTreeMap::new();
    let mut moved_to: BTreeMap<u32, usize> = BTreeMap::new();

    for (i, ev) in events.iter().enumerate() {
        if ev.cookie == 0 {
            continue;
        }
        if (ev.mask & IN_MOVED_FROM) != 0 {
            moved_from.entry(ev.cookie).or_insert(i);
        }
        if (ev.mask & IN_MOVED_TO) != 0 {
            moved_to.entry(ev.cookie).or_insert(i);
        }
    }

    let mut pairs = Vec::new();
    for (cookie, &from_idx) in &moved_from {
        if let Some(&to_idx) = moved_to.get(cookie) {
            pairs.push((events[from_idx].clone(), events[to_idx].clone()));
        }
    }

    pairs.sort_by_key(|(a, _)| a.seq_no);
    pairs
}

// ============================================================================
// EVENT QUEUE WITH OVERFLOW AND COALESCING (Task 5.6.4)
// ============================================================================

/// Configurable FIFO event queue with overflow detection and coalescing.
///
/// # Ordering Guarantees (§6.3)
///
/// 1. **In-order delivery per inode** — Events for the same inode are
///    delivered in the order they were generated. The `seq_no` field is
///    monotonic across all events in a single FsEventQueue.
/// 2. **Parent before child** — When a directory operation affects both a
///    parent and child (e.g., delete), the parent IN_DELETE event is
///    dispatched before the child IN_DELETE_SELF event.
/// 3. **Rename atomicity** — `IN_MOVED_FROM` and `IN_MOVED_TO` share the
///    same `cookie` for matching. They are NOT guaranteed to be consecutive
///    in the queue; other events may appear between them. Callers must use
///    the cookie for pair matching.
/// 4. **Coalescing** — Consecutive identical events (same wd, mask, cookie,
///    name) are coalesced into one. This is consistent with Linux inotify(7)
///    behavior: "If successive output inotify events ... are identical ...
///    then they are coalesced into a single event if the older event has not
///    yet been read."
/// 5. **Overflow** — When the queue is full, the oldest event is evicted and
///    an `IN_Q_OVERFLOW` event (wd=-1) is injected at the tail.
pub struct FsEventQueue {
    events: Vec<FsEvent>,
    max_size: usize,
    overflow_pending: bool,
}

impl FsEventQueue {
    pub const DEFAULT_MAX_SIZE: usize = 4096;

    pub fn new(max_size: usize) -> Self {
        Self {
            events: Vec::with_capacity(max_size),
            max_size,
            overflow_pending: false,
        }
    }

    pub fn with_default_max() -> Self {
        Self::new(Self::DEFAULT_MAX_SIZE)
    }

    /// Push an event. Returns the dropped event if the queue was full.
    ///
    /// When the queue is full the oldest event is evicted and an
    /// IN_Q_OVERFLOW event is injected at the tail. The caller receives
    /// the evicted event as `Err(dropped)`.
    pub fn push(&mut self, event: FsEvent) -> Result<(), FsEvent> {
        if self.events.len() >= self.max_size {
            let dropped = self.events.remove(0);
            if !self.overflow_pending {
                self.overflow_pending = true;
                let overflow_ev = FsEvent::new(-1, IN_Q_OVERFLOW, 0, "");
                self.events.push(overflow_ev);
            }
            return Err(dropped);
        }

        self.events.push(event);
        Ok(())
    }

    /// Pop the oldest event from the queue.
    pub fn pop(&mut self) -> Option<FsEvent> {
        if self.events.is_empty() {
            return None;
        }
        let ev = self.events.remove(0);
        if (ev.mask & IN_Q_OVERFLOW) != 0 {
            self.overflow_pending = false;
        }
        Some(ev)
    }

    /// Current number of events in the queue.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Coalesce consecutive identical events and drain the entire queue.
    ///
    /// Two events are considered identical when they share the same `wd`,
    /// `mask`, and `name_snapshot`. The coalesced event keeps the highest
    /// `seq_no` and the latest `cookie` (non-zero preferred).
    pub fn coalesce_and_drain(&mut self) -> Vec<FsEvent> {
        if self.events.is_empty() {
            return Vec::new();
        }

        let mut drained = core::mem::take(&mut self.events);
        self.overflow_pending = false;

        let mut result: Vec<FsEvent> = Vec::with_capacity(drained.len());

        for event in drained.drain(..) {
            if let Some(last) = result.last_mut() {
                if last.wd == event.wd
                    && last.mask == event.mask
                    && last.name_snapshot == event.name_snapshot
                {
                    if event.cookie != 0 {
                        last.cookie = event.cookie;
                    }
                    if event.seq_no > last.seq_no {
                        last.seq_no = event.seq_no;
                    }
                    if event.error_class.is_some() {
                        last.error_class = event.error_class;
                    }
                    continue;
                }
            }
            result.push(event);
        }

        result
    }
}

// ============================================================================
// INOTIFY SABİTLERİ
// ============================================================================

/// inotify olay maskeleri
pub const IN_ACCESS: u32 = 0x00000001; // Dosyaya erişildi
pub const IN_MODIFY: u32 = 0x00000002; // Dosya değiştirildi
pub const IN_ATTRIB: u32 = 0x00000004; // Meta veri değişti
pub const IN_CLOSE_WRITE: u32 = 0x00000008; // Yazılabilir dosya kapatıldı
pub const IN_CLOSE_NOWRITE: u32 = 0x00000010; // Salt okunur dosya kapatıldı
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
pub const IN_OPEN: u32 = 0x00000020; // Dosya açıldı
pub const IN_MOVED_FROM: u32 = 0x00000040; // Dosya X'ten taşındı
pub const IN_MOVED_TO: u32 = 0x00000080; // Dosya Y'ye taşındı
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
pub const IN_CREATE: u32 = 0x00000100; // Alt dosya/dizin oluşturuldu
pub const IN_DELETE: u32 = 0x00000200; // Alt dosya/dizin silindi
pub const IN_DELETE_SELF: u32 = 0x00000400; // İzlenen nesne silindi
pub const IN_MOVE_SELF: u32 = 0x00000800; // İzlenen nesne taşındı

/// Özel sistem olayları
pub const IN_UNMOUNT: u32 = 0x00002000; // Bağlı dosya sistemi çıkarıldı
pub const IN_Q_OVERFLOW: u32 = 0x00004000; // Olay kuyruğu taştı
pub const IN_IGNORED: u32 = 0x00008000; // İzleyici kaldırıldı
pub const IN_ISDIR: u32 = 0x40000000; // Olay bir dizinde oluştu
pub const IN_ONESHOT: u32 = 0x80000000; // Tek sefer olay gönder

/// Convenience macro: all events that can be monitored.
///
/// inotify(7) spec: "The IN_ALL_EVENTS macro is defined as a bit mask of all
/// of the above events."
pub const IN_ALL_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_CREATE
    | IN_DELETE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

/// inotify başlatma bayrakları
pub const IN_CLOEXEC: i32 = 0x02000000; // exec sonrası kapat
pub const IN_NONBLOCK: i32 = 0x00004000; // Engellemesiz mod

/// inotify_add_watch mask flag'leri
pub const IN_DONT_FOLLOW: u32 = 0x02000000; // Symbolic link'i takip etme
pub const IN_EXCL_UNLINK: u32 = 0x04000000; // Unlink sonrası event üretme
pub const IN_MASK_ADD: u32 = 0x20000000; // Mevcut maskeye ekle
pub const IN_ONLYDIR: u32 = 0x01000000; // Sadece dizin ise izle
pub const IN_MASK_CREATE: u32 = 0x10000000; // Sadece yeni watch oluştur

/// Instance başına maksimum izleyici sayısı
pub const INOTIFY_MAX_WATCHES: usize = 8192;
/// Kuyruk başına maksimum olay sayısı
pub const INOTIFY_MAX_EVENTS: usize = 16384;
/// Kullanıcı başına maksimum instance sayısı
pub const INOTIFY_MAX_INSTANCES: usize = 128;

// ============================================================================
// INOTIFY EVENT
// ============================================================================

/// inotify event structure (returned by read())
#[repr(C)]
#[derive(Clone, Debug)]
pub struct InotifyEvent {
    /// Watch descriptor
    pub wd: i32,
    /// Event mask
    pub mask: u32,
    /// Cookie for rename tracking
    pub cookie: u32,
    /// Length of name (0 if no name)
    pub name_len: u32,
    /// Optional filename (null-terminated)
    pub name: String,
}

impl InotifyEvent {
    pub fn new(wd: i32, mask: u32, cookie: u32, name: &str) -> Self {
        Self {
            wd,
            mask,
            cookie,
            name_len: if name.is_empty() {
                0
            } else {
                (name.len() + 1) as u32
            },
            name: String::from(name),
        }
    }

    /// Calculate total event size (for read buffer)
    pub fn size(&self) -> usize {
        // struct size + name + null terminator (aligned to sizeof(long))
        let base = core::mem::size_of::<InotifyEventRaw>();
        let name_len = self.name.len() + 1; // +1 for null terminator
        let padding = (8 - (name_len % 8)) % 8;
        base + name_len + padding
    }
}

/// Raw inotify event (as returned by read)
#[repr(C)]
pub struct InotifyEventRaw {
    pub wd: i32,
    pub mask: u32,
    pub cookie: u32,
    pub name_len: u32,
    // Followed by name bytes
}

// ============================================================================
// INOTIFY WATCH
// ============================================================================

/// A watch on a file/directory
#[derive(Clone, Debug)]
pub struct InotifyWatchTarget {
    /// Filesystem type visible at the VFS boundary
    pub fs_type: crate::fs::vfs_unified::VfsFsType,
    /// Mount point that owns this namespace
    pub mount_point: String,
    /// Mounted backend/source identifier
    pub source: String,
    /// Inode number inside the mounted namespace
    pub inode: u64,
}

impl InotifyWatchTarget {
    pub fn new(
        fs_type: crate::fs::vfs_unified::VfsFsType,
        mount_point: &str,
        source: &str,
        inode: u64,
    ) -> Self {
        Self {
            fs_type,
            mount_point: String::from(mount_point),
            source: String::from(source),
            inode,
        }
    }
}

impl PartialEq for InotifyWatchTarget {
    fn eq(&self, other: &Self) -> bool {
        self.fs_type == other.fs_type
            && self.mount_point == other.mount_point
            && self.source == other.source
            && self.inode == other.inode
    }
}

impl Eq for InotifyWatchTarget {}

impl PartialOrd for InotifyWatchTarget {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InotifyWatchTarget {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.fs_type.as_str(),
            self.mount_point.as_str(),
            self.source.as_str(),
            self.inode,
        )
            .cmp(&(
                other.fs_type.as_str(),
                other.mount_point.as_str(),
                other.source.as_str(),
                other.inode,
            ))
    }
}

/// A watch on a file/directory
#[derive(Clone, Debug)]
pub struct InotifyWatch {
    /// Watch descriptor (unique per instance)
    pub wd: i32,
    /// Namespace-aware target identity
    pub target: InotifyWatchTarget,
    /// Path being watched
    pub path: String,
    /// Event mask (what events to watch)
    pub mask: u32,
    /// One-shot watch (auto-remove after first event)
    pub oneshot: bool,
    /// Is this watch still active?
    pub active: bool,
}

impl InotifyWatch {
    pub fn new(wd: i32, target: InotifyWatchTarget, path: &str, mask: u32) -> Self {
        Self {
            wd,
            target,
            path: String::from(path),
            mask,
            oneshot: (mask & IN_ONESHOT) != 0,
            active: true,
        }
    }

    /// Check if this watch should generate event
    pub fn matches(&self, event_mask: u32) -> bool {
        self.active && (self.mask & event_mask) != 0
    }
}

// ============================================================================
// INOTIFY INSTANCE
// ============================================================================

/// inotify instance (per-process)
pub struct InotifyInstance {
    /// Instance ID
    pub id: i32,
    /// Watch descriptor counter
    next_wd: AtomicI32,
    /// Watches (wd -> watch)
    watches: Mutex<BTreeMap<i32, InotifyWatch>>,
    /// Event queue (pending events)
    events: Mutex<Vec<InotifyEvent>>,
    /// Number of events in queue
    event_count: AtomicU32,
    /// Queue overflow flag
    overflow: AtomicBool,
    /// Non-blocking mode
    nonblock: AtomicBool,
    /// Closed flag
    closed: AtomicBool,
}

impl InotifyInstance {
    pub fn new(id: i32, flags: i32) -> Self {
        Self {
            id,
            next_wd: AtomicI32::new(1),
            watches: Mutex::new(BTreeMap::new()),
            events: Mutex::new(Vec::new()),
            event_count: AtomicU32::new(0),
            overflow: AtomicBool::new(false),
            nonblock: AtomicBool::new((flags & IN_NONBLOCK) != 0),
            closed: AtomicBool::new(false),
        }
    }

    /// Add a watch
    pub fn add_watch(&self, target: InotifyWatchTarget, path: &str, mask: u32) -> i32 {
        // Check for existing watch on same namespace-visible target.
        let mut watches = self.watches.lock();

        for (_, watch) in watches.iter_mut() {
            if watch.target == target {
                // Update existing watch
                watch.mask = mask;
                watch.oneshot = (mask & IN_ONESHOT) != 0;
                watch.active = true;
                watch.path = String::from(path);
                return watch.wd;
            }
        }

        // Create new watch
        let wd = self.next_wd.fetch_add(1, Ordering::SeqCst);
        let watch = InotifyWatch::new(wd, target.clone(), path, mask);
        watches.insert(wd, watch);

        crate::serial_println!(
            "[INOTIFY] Added watch wd={} fs={} mount={} source={} inode={:#x} mask={:#x}",
            wd,
            target.fs_type.as_str(),
            target.mount_point,
            target.source,
            target.inode,
            mask
        );

        wd
    }

    /// Remove a watch
    pub fn remove_watch(&self, wd: i32) -> bool {
        let mut watches = self.watches.lock();

        if let Some(watch) = watches.remove(&wd) {
            // Generate IGNORED event
            drop(watches);
            self.push_event(InotifyEvent::new(wd, IN_IGNORED, 0, ""));
            return true;
        }

        false
    }

    /// Get watch by wd
    pub fn get_watch(&self, wd: i32) -> Option<InotifyWatch> {
        self.watches.lock().get(&wd).cloned()
    }

    /// Push event to queue
    pub fn push_event(&self, event: InotifyEvent) {
        let mut events = self.events.lock();

        if events.len() >= INOTIFY_MAX_EVENTS {
            self.overflow.store(true, Ordering::SeqCst);
            // Push overflow event
            events.push(InotifyEvent::new(-1, IN_Q_OVERFLOW, 0, ""));
            return;
        }

        events.push(event);
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Pop event from queue
    pub fn pop_event(&self) -> Option<InotifyEvent> {
        let mut events = self.events.lock();

        if events.is_empty() {
            return None;
        }

        let event = events.remove(0);
        self.event_count.fetch_sub(1, Ordering::SeqCst);
        Some(event)
    }

    /// Get pending event count
    pub fn pending_count(&self) -> u32 {
        self.event_count.load(Ordering::SeqCst)
    }

    /// Check if queue has overflowed
    pub fn has_overflow(&self) -> bool {
        self.overflow.load(Ordering::SeqCst)
    }

    /// Close instance
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Check if closed
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Get watch count
    pub fn watch_count(&self) -> usize {
        self.watches.lock().len()
    }
}

// ============================================================================
// INOTIFY MANAGER
// ============================================================================

/// Global inotify manager
pub struct InotifyManager {
    /// Instances (id -> instance)
    instances: Mutex<BTreeMap<i32, Arc<InotifyInstance>>>,
    /// Next instance ID
    next_id: AtomicI32,
    /// Watch index (namespace-aware target -> watchers)
    watch_index: Mutex<BTreeMap<InotifyWatchTarget, Vec<(i32, i32)>>>, // (instance_id, wd)
    /// Total watches
    total_watches: AtomicU64,
    /// Total events generated
    total_events: AtomicU64,
}

impl InotifyManager {
    pub const fn new() -> Self {
        Self {
            instances: Mutex::new(BTreeMap::new()),
            next_id: AtomicI32::new(1),
            watch_index: Mutex::new(BTreeMap::new()),
            total_watches: AtomicU64::new(0),
            total_events: AtomicU64::new(0),
        }
    }

    /// Create new inotify instance
    pub fn create_instance(&self, flags: i32) -> Result<Arc<InotifyInstance>, InotifyError> {
        let mut instances = self.instances.lock();

        if instances.len() >= INOTIFY_MAX_INSTANCES {
            return Err(InotifyError::TooManyInstances);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let instance = Arc::new(InotifyInstance::new(id, flags));
        instances.insert(id, instance.clone());

        crate::serial_println!("[INOTIFY] Created instance id={}", id);

        Ok(instance)
    }

    /// Get instance by ID
    pub fn get_instance(&self, id: i32) -> Option<Arc<InotifyInstance>> {
        self.instances.lock().get(&id).cloned()
    }

    /// Remove instance
    pub fn remove_instance(&self, id: i32) {
        let mut instances = self.instances.lock();
        if let Some(instance) = instances.remove(&id) {
            instance.close();

            // Remove from watch index
            let mut watch_index = self.watch_index.lock();
            let watches = instance.watches.lock();
            for (wd, watch) in watches.iter() {
                if let Some(watchers) = watch_index.get_mut(&watch.target) {
                    watchers.retain(|(iid, w)| *iid != id || *w != *wd);
                }
            }

            crate::serial_println!("[INOTIFY] Removed instance id={}", id);
        }
    }

    /// Add watch to index
    pub fn index_watch(&self, target: InotifyWatchTarget, instance_id: i32, wd: i32) {
        let mut watch_index = self.watch_index.lock();
        let entry = watch_index.entry(target).or_insert_with(Vec::new);
        entry.push((instance_id, wd));
        self.total_watches.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove watch from index
    pub fn unindex_watch(&self, target: &InotifyWatchTarget, instance_id: i32, wd: i32) {
        let mut watch_index = self.watch_index.lock();
        if let Some(watchers) = watch_index.get_mut(target) {
            watchers.retain(|(iid, w)| *iid != instance_id || *w != wd);
        }
        self.total_watches.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get watchers for a namespace-visible target
    pub fn get_watchers(&self, target: &InotifyWatchTarget) -> Vec<(i32, i32)> {
        self.watch_index
            .lock()
            .get(target)
            .cloned()
            .unwrap_or_default()
    }

    /// Legacy broad lookup for callers that only know the inode number.
    pub fn get_watchers_by_inode(&self, inode: u64) -> Vec<(i32, i32)> {
        let mut result = Vec::new();
        for (target, watchers) in self.watch_index.lock().iter() {
            if target.inode == inode {
                result.extend_from_slice(watchers.as_slice());
            }
        }
        result
    }
}

lazy_static::lazy_static! {
    /// Global inotify manager
    static ref INOTIFY_MANAGER: InotifyManager = InotifyManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InotifyError {
    TooManyInstances,
    TooManyWatches,
    QueueOverflow,
    InvalidWatch,
    InstanceNotFound,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

/// inotify_init syscall implementation
pub fn sys_inotify_init() -> i32 {
    sys_inotify_init1(0)
}

/// inotify_init1 syscall implementation
pub fn sys_inotify_init1(flags: i32) -> i32 {
    match INOTIFY_MANAGER.create_instance(flags) {
        Ok(instance) => instance.id,
        Err(InotifyError::TooManyInstances) => -24, // EMFILE
        Err(_) => -22,                              // EINVAL
    }
}

/// inotify_add_watch syscall implementation
///
/// # Arguments
/// - `fd`: inotify instance fd
/// - `pathname`: Path to watch
/// - `mask`: Event mask
///
/// # Returns
/// Watch descriptor on success, negative errno on failure
pub fn sys_inotify_add_watch(fd: i32, pathname: &str, mask: u32) -> i32 {
    let instance = match INOTIFY_MANAGER.get_instance(fd) {
        Some(i) => i,
        None => return -9, // EBADF
    };

    if instance.is_closed() {
        return -9; // EBADF
    }

    if instance.watch_count() >= INOTIFY_MAX_WATCHES {
        return -28; // ENOSPC
    }

    let target = match resolve_watch_target_from_path(pathname) {
        Ok(target) => target,
        Err(_) => return -2, // ENOENT
    };

    let wd = instance.add_watch(target.clone(), pathname, mask);
    INOTIFY_MANAGER.index_watch(target, fd, wd);

    wd
}

/// inotify_rm_watch syscall implementation
pub fn sys_inotify_rm_watch(fd: i32, wd: i32) -> i32 {
    let instance = match INOTIFY_MANAGER.get_instance(fd) {
        Some(i) => i,
        None => return -9, // EBADF
    };

    if let Some(watch) = instance.get_watch(wd) {
        INOTIFY_MANAGER.unindex_watch(&watch.target, fd, wd);
        instance.remove_watch(wd);
        return 0;
    }

    -22 // EINVAL
}

/// Read events from inotify instance
///
/// Returns raw bytes that can be parsed as InotifyEventRaw
pub fn sys_inotify_read(fd: i32, buf: &mut [u8]) -> i32 {
    let instance = match INOTIFY_MANAGER.get_instance(fd) {
        Some(i) => i,
        None => return -9, // EBADF
    };

    if instance.is_closed() {
        return -9; // EBADF
    }

    if instance.pending_count() == 0 {
        if instance.nonblock.load(Ordering::SeqCst) {
            return -11; // EAGAIN
        }
        // Would block - in real impl, would sleep
        return 0;
    }

    let mut offset = 0;

    while offset < buf.len() {
        let event = match instance.pop_event() {
            Some(e) => e,
            None => break,
        };

        let name_with_null = event.name.len() + 1; // includes null terminator
        let raw = InotifyEventRaw {
            wd: event.wd,
            mask: event.mask,
            cookie: event.cookie,
            name_len: name_with_null as u32,
        };

        // Copy raw struct
        let raw_bytes = unsafe {
            core::slice::from_raw_parts(
                &raw as *const _ as *const u8,
                core::mem::size_of::<InotifyEventRaw>(),
            )
        };

        if offset + raw_bytes.len() > buf.len() {
            break;
        }

        buf[offset..offset + raw_bytes.len()].copy_from_slice(raw_bytes);
        offset += raw_bytes.len();

        // Copy name + null terminator
        if offset + name_with_null <= buf.len() {
            buf[offset..offset + event.name.len()].copy_from_slice(event.name.as_bytes());
            buf[offset + event.name.len()] = 0; // null terminator
            offset += name_with_null;

            // Align to 8 bytes (padding after null terminator)
            let padding = (8 - (name_with_null % 8)) % 8;
            for i in 0..padding {
                if offset + i < buf.len() {
                    buf[offset + i] = 0;
                }
            }
            offset += padding;
        }
    }

    offset as i32
}

// ============================================================================
// EVENT GENERATION (FOR FILESYSTEM)
// ============================================================================

/// Generate inotify event for a file
/// Called by filesystem when events occur
pub fn generate_event(inode: u64, mask: u32, cookie: u32, name: &str) {
    let watchers = INOTIFY_MANAGER.get_watchers_by_inode(inode);

    if watchers.is_empty() {
        return;
    }

    INOTIFY_MANAGER.total_events.fetch_add(1, Ordering::Relaxed);

    for (instance_id, wd) in watchers {
        if let Some(instance) = INOTIFY_MANAGER.get_instance(instance_id) {
            if let Some(watch) = instance.get_watch(wd) {
                if watch.target.inode == inode && watch.matches(mask) {
                    instance.push_event(InotifyEvent::new(wd, mask, cookie, name));

                    // Handle oneshot
                    if watch.oneshot {
                        instance.remove_watch(wd);
                        INOTIFY_MANAGER.unindex_watch(&watch.target, instance_id, wd);
                    }
                }
            }
        }
    }

    crate::serial_println!(
        "[INOTIFY] Legacy inode event: inode={:#x} mask={:#x} name='{}'",
        inode,
        mask,
        name
    );
}

/// Generate inotify event for a VFS path with namespace-aware watch identity.
pub fn generate_event_for_path(path: &str, mask: u32, cookie: u32, name: &str) {
    let Ok(target) = resolve_watch_target_from_path(path) else {
        return;
    };
    dispatch_event_for_target(&INOTIFY_MANAGER, &target, mask, cookie, name);
}

fn dispatch_event_for_existing_path(
    manager: &InotifyManager,
    path: &str,
    mask: u32,
    cookie: u32,
    name: &str,
) {
    let Ok(target) = resolve_watch_target_from_path(path) else {
        return;
    };
    dispatch_event_for_target(manager, &target, mask, cookie, name);
}

fn dispatch_child_event_for_parent_path(
    manager: &InotifyManager,
    parent_path: &str,
    mask: u32,
    name: &str,
) {
    let Ok(parent_target) = resolve_watch_target_from_path(parent_path) else {
        return;
    };
    dispatch_event_for_target(manager, &parent_target, mask, 0, name);
}

fn dispatch_delete_self_for_path(manager: &InotifyManager, path: &str) {
    dispatch_event_for_existing_path(manager, path, IN_DELETE_SELF, 0, "");
}

fn dispatch_delete_self_for_target(manager: &InotifyManager, target: &InotifyWatchTarget) {
    dispatch_event_for_target(manager, target, IN_DELETE_SELF, 0, "");
}

fn dispatch_move_self_for_path(manager: &InotifyManager, path: &str) {
    dispatch_event_for_existing_path(manager, path, IN_MOVE_SELF, 0, "");
}

fn dispatch_move_between_parent_targets(
    manager: &InotifyManager,
    old_parent: &InotifyWatchTarget,
    new_parent: &InotifyWatchTarget,
    old_name: &str,
    new_name: &str,
    is_dir: bool,
) {
    let cookie = generate_cookie();
    let dir_flag = if is_dir { IN_ISDIR } else { 0 };

    dispatch_event_for_target(
        manager,
        old_parent,
        IN_MOVED_FROM | dir_flag,
        cookie,
        old_name,
    );
    dispatch_event_for_target(
        manager,
        new_parent,
        IN_MOVED_TO | dir_flag,
        cookie,
        new_name,
    );
}

fn dispatch_move_between_parent_paths(
    manager: &InotifyManager,
    old_parent_path: &str,
    new_parent_path: &str,
    old_name: &str,
    new_name: &str,
    is_dir: bool,
) {
    let Ok(old_parent_target) = resolve_watch_target_from_path(old_parent_path) else {
        return;
    };
    let Ok(new_parent_target) = resolve_watch_target_from_path(new_parent_path) else {
        return;
    };
    dispatch_move_between_parent_targets(
        manager,
        &old_parent_target,
        &new_parent_target,
        old_name,
        new_name,
        is_dir,
    );
}

fn dispatch_move_self_for_target(manager: &InotifyManager, target: &InotifyWatchTarget) {
    dispatch_event_for_target(manager, target, IN_MOVE_SELF, 0, "");
}

fn dispatch_store_read_for_target(manager: &InotifyManager, target: &InotifyWatchTarget) {
    dispatch_event_for_target(manager, target, IN_OPEN, 0, "");
    dispatch_event_for_target(manager, target, IN_ACCESS, 0, "");
    dispatch_event_for_target(manager, target, IN_CLOSE_NOWRITE, 0, "");
}

fn dispatch_store_write_for_target(manager: &InotifyManager, target: &InotifyWatchTarget) {
    dispatch_event_for_target(manager, target, IN_OPEN, 0, "");
    dispatch_event_for_target(manager, target, IN_MODIFY, 0, "");
    dispatch_event_for_target(manager, target, IN_CLOSE_WRITE, 0, "");
}

fn dispatch_store_create_for_parent_target(
    manager: &InotifyManager,
    parent_target: &InotifyWatchTarget,
    name: &str,
    is_dir: bool,
) {
    let mask = if is_dir {
        IN_CREATE | IN_ISDIR
    } else {
        IN_CREATE
    };
    dispatch_event_for_target(manager, parent_target, mask, 0, name);
}

fn dispatch_store_delete_for_parent_and_target(
    manager: &InotifyManager,
    parent_target: &InotifyWatchTarget,
    deleted_target: Option<&InotifyWatchTarget>,
    name: &str,
    is_dir: bool,
) {
    let mask = if is_dir {
        IN_DELETE | IN_ISDIR
    } else {
        IN_DELETE
    };
    dispatch_event_for_target(manager, parent_target, mask, 0, name);
    if let Some(target) = deleted_target {
        dispatch_delete_self_for_target(manager, target);
    }
}

fn dispatch_store_move_for_targets(
    manager: &InotifyManager,
    old_parent: &InotifyWatchTarget,
    new_parent: &InotifyWatchTarget,
    moved_target: Option<&InotifyWatchTarget>,
    old_name: &str,
    new_name: &str,
    is_dir: bool,
) {
    dispatch_move_between_parent_targets(
        manager, old_parent, new_parent, old_name, new_name, is_dir,
    );
    if let Some(target) = moved_target {
        dispatch_move_self_for_target(manager, target);
    }
}

fn dispatch_event_for_target(
    manager: &InotifyManager,
    target: &InotifyWatchTarget,
    mask: u32,
    cookie: u32,
    name: &str,
) {
    let watchers = manager.get_watchers(target);

    if watchers.is_empty() {
        return;
    }

    manager.total_events.fetch_add(1, Ordering::Relaxed);

    for (instance_id, wd) in watchers {
        if let Some(instance) = manager.get_instance(instance_id) {
            if let Some(watch) = instance.get_watch(wd) {
                if watch.target == *target && watch.matches(mask) {
                    instance.push_event(InotifyEvent::new(wd, mask, cookie, name));

                    // Handle oneshot
                    if watch.oneshot {
                        instance.remove_watch(wd);
                        manager.unindex_watch(&watch.target, instance_id, wd);
                    }
                }
            }
        }
    }

    crate::serial_println!(
        "[INOTIFY] Event: fs={} mount={} source={} inode={:#x} mask={:#x} name='{}'",
        target.fs_type.as_str(),
        target.mount_point,
        target.source,
        target.inode,
        mask,
        name
    );
}

/// Generate event for file access
pub fn notify_access(inode: u64) {
    generate_event(inode, IN_ACCESS, 0, "");
}

/// Generate event for file modification
pub fn notify_modify(inode: u64) {
    generate_event(inode, IN_MODIFY, 0, "");
}

/// Generate event for file creation
pub fn notify_create(parent_inode: u64, name: &str, is_dir: bool) {
    let mask = if is_dir {
        IN_CREATE | IN_ISDIR
    } else {
        IN_CREATE
    };
    generate_event(parent_inode, mask, 0, name);
}

/// Generate event for file deletion
pub fn notify_delete(parent_inode: u64, name: &str, is_dir: bool) {
    let mask = if is_dir {
        IN_DELETE | IN_ISDIR
    } else {
        IN_DELETE
    };
    generate_event(parent_inode, mask, 0, name);
}

/// Generate event for file move
pub fn notify_move(old_parent: u64, new_parent: u64, old_name: &str, new_name: &str, is_dir: bool) {
    let cookie = generate_cookie();
    let dir_flag = if is_dir { IN_ISDIR } else { 0 };

    generate_event(old_parent, IN_MOVED_FROM | dir_flag, cookie, old_name);
    generate_event(new_parent, IN_MOVED_TO | dir_flag, cookie, new_name);
}

/// Generate event for attribute change
pub fn notify_attrib(inode: u64) {
    generate_event(inode, IN_ATTRIB, 0, "");
}

/// Generate event for file open
pub fn notify_open(inode: u64) {
    generate_event(inode, IN_OPEN, 0, "");
}

/// Generate event for file close
pub fn notify_close(inode: u64, writable: bool) {
    let mask = if writable {
        IN_CLOSE_WRITE
    } else {
        IN_CLOSE_NOWRITE
    };
    generate_event(inode, mask, 0, "");
}

pub fn notify_access_path(path: &str) {
    dispatch_event_for_existing_path(&INOTIFY_MANAGER, path, IN_ACCESS, 0, "");
}

pub fn notify_modify_path(path: &str) {
    dispatch_event_for_existing_path(&INOTIFY_MANAGER, path, IN_MODIFY, 0, "");
}

pub fn notify_attrib_path(path: &str) {
    dispatch_event_for_existing_path(&INOTIFY_MANAGER, path, IN_ATTRIB, 0, "");
}

pub fn notify_open_path(path: &str) {
    dispatch_event_for_existing_path(&INOTIFY_MANAGER, path, IN_OPEN, 0, "");
}

pub fn notify_close_path(path: &str, writable: bool) {
    let mask = if writable {
        IN_CLOSE_WRITE
    } else {
        IN_CLOSE_NOWRITE
    };
    dispatch_event_for_existing_path(&INOTIFY_MANAGER, path, mask, 0, "");
}

pub fn notify_create_path(parent_path: &str, name: &str, is_dir: bool) {
    let Ok(parent_target) = resolve_watch_target_from_path(parent_path) else {
        return;
    };
    dispatch_store_create_for_parent_target(&INOTIFY_MANAGER, &parent_target, name, is_dir);
}

pub fn notify_delete_path(parent_path: &str, path: &str, name: &str, is_dir: bool) {
    let Ok(parent_target) = resolve_watch_target_from_path(parent_path) else {
        return;
    };
    let deleted_target = resolve_watch_target_from_path(path).ok();
    dispatch_store_delete_for_parent_and_target(
        &INOTIFY_MANAGER,
        &parent_target,
        deleted_target.as_ref(),
        name,
        is_dir,
    );
}

pub fn watch_target_for_path(path: &str) -> Option<InotifyWatchTarget> {
    resolve_watch_target_from_path(path).ok()
}

pub fn notify_delete_path_with_target(
    parent_path: &str,
    deleted_target: Option<&InotifyWatchTarget>,
    name: &str,
    is_dir: bool,
) {
    let Ok(parent_target) = resolve_watch_target_from_path(parent_path) else {
        return;
    };
    dispatch_store_delete_for_parent_and_target(
        &INOTIFY_MANAGER,
        &parent_target,
        deleted_target,
        name,
        is_dir,
    );
}

pub fn notify_move_path(
    old_parent_path: &str,
    new_parent_path: &str,
    new_path: &str,
    old_name: &str,
    new_name: &str,
    is_dir: bool,
) {
    let Ok(old_parent_target) = resolve_watch_target_from_path(old_parent_path) else {
        return;
    };
    let Ok(new_parent_target) = resolve_watch_target_from_path(new_parent_path) else {
        return;
    };
    let moved_target = resolve_watch_target_from_path(new_path).ok();
    dispatch_store_move_for_targets(
        &INOTIFY_MANAGER,
        &old_parent_target,
        &new_parent_target,
        moved_target.as_ref(),
        old_name,
        new_name,
        is_dir,
    );
}

pub fn notify_store_read_path(path: &str) {
    let Ok(target) = resolve_watch_target_from_path(path) else {
        return;
    };
    dispatch_store_read_for_target(&INOTIFY_MANAGER, &target);
}

pub fn notify_store_write_path(path: &str) {
    let Ok(target) = resolve_watch_target_from_path(path) else {
        return;
    };
    dispatch_store_write_for_target(&INOTIFY_MANAGER, &target);
}

// ============================================================================
// INODE DELETION HANDLER (Task 5.6.6 — IN_IGNORED automatic)
// ============================================================================

/// When a watched inode is deleted, emit IN_IGNORED for every watch that
/// references it and remove those watches from both the instance and the
/// global index.
///
/// Returns the generated IN_IGNORED events so the caller can deliver them
/// to userspace.
pub fn handle_inode_deletion(inode_id: u64) -> Vec<FsEvent> {
    let mut ignored_events = Vec::new();
    let manager = &INOTIFY_MANAGER;

    let mut targets_to_unindex: Vec<(InotifyWatchTarget, i32, i32)> = Vec::new();

    {
        let mut instances = manager.instances.lock();
        for (&instance_id, instance) in instances.iter() {
            let mut watches = instance.watches.lock();
            let mut to_remove: Vec<i32> = Vec::new();

            for (&wd, watch) in watches.iter() {
                if watch.target.inode == inode_id && watch.active {
                    to_remove.push(wd);
                    ignored_events.push(FsEvent::new(wd, IN_IGNORED, 0, ""));
                }
            }

            for wd in &to_remove {
                if let Some(watch) = watches.remove(wd) {
                    targets_to_unindex.push((watch.target, instance_id, *wd));
                }
            }
        }
    }

    for (target, instance_id, wd) in &targets_to_unindex {
        manager.unindex_watch(target, *instance_id, *wd);
    }

    if !ignored_events.is_empty() {
        crate::serial_println!(
            "[INOTIFY] handle_inode_deletion: inode={:#x} emitted {} IN_IGNORED events",
            inode_id,
            ignored_events.len()
        );
    }

    ignored_events
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Generate unique cookie for move events (delegates to next_cookie).
fn generate_cookie() -> u32 {
    next_cookie()
}

fn resolve_watch_target_from_path(pathname: &str) -> Result<InotifyWatchTarget, &'static str> {
    let vfs = crate::fs::vfs_unified::VFS_UNIFIED.lock();
    let info = vfs.open(pathname).map_err(|_| "vfs open failed")?;
    let mount = vfs
        .resolve_fs(pathname)
        .ok_or("vfs mount resolution failed")?;
    Ok(InotifyWatchTarget::new(
        info.fs_type,
        mount.mount_point.as_str(),
        mount.source.as_str(),
        info.inode,
    ))
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize inotify subsystem
pub fn init() {
    crate::serial_println!("[INOTIFY] Subsystem initialized");
}

/// Get inotify statistics
pub struct InotifyStats {
    pub instance_count: usize,
    pub total_watches: u64,
    pub total_events: u64,
}

/// Get statistics
pub fn get_stats() -> InotifyStats {
    InotifyStats {
        instance_count: INOTIFY_MANAGER.instances.lock().len(),
        total_watches: INOTIFY_MANAGER.total_watches.load(Ordering::Relaxed),
        total_events: INOTIFY_MANAGER.total_events.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn collect_events(instance: &Arc<InotifyInstance>) -> Vec<InotifyEvent> {
        let mut events = Vec::new();
        while let Some(event) = instance.pop_event() {
            events.push(event);
        }
        events
    }

    #[test]
    fn namespace_aware_targets_do_not_collapse_same_inode_across_mounts() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let ext4_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ext4,
            "/mnt/a",
            "ext4:a",
            42,
        );
        let ntfs_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ntfs,
            "/mnt/b",
            "ntfs:b",
            42,
        );

        let ext4_wd = instance.add_watch(ext4_target.clone(), "/mnt/a/hello.txt", IN_MODIFY);
        manager.index_watch(ext4_target.clone(), instance.id, ext4_wd);
        let ntfs_wd = instance.add_watch(ntfs_target.clone(), "/mnt/b/hello.txt", IN_MODIFY);
        manager.index_watch(ntfs_target.clone(), instance.id, ntfs_wd);

        assert_eq!(
            manager.get_watchers(&ext4_target),
            vec![(instance.id, ext4_wd)]
        );
        assert_eq!(
            manager.get_watchers(&ntfs_target),
            vec![(instance.id, ntfs_wd)]
        );
        assert_eq!(manager.get_watchers_by_inode(42).len(), 2);
    }

    #[test]
    fn dispatch_targets_only_matching_namespace_watchers() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let ext4_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ext4,
            "/mnt/a",
            "ext4:a",
            7,
        );
        let ntfs_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ntfs,
            "/mnt/b",
            "ntfs:b",
            7,
        );

        let ext4_wd = instance.add_watch(ext4_target.clone(), "/mnt/a/data.bin", IN_MODIFY);
        manager.index_watch(ext4_target.clone(), instance.id, ext4_wd);
        let ntfs_wd = instance.add_watch(ntfs_target.clone(), "/mnt/b/data.bin", IN_MODIFY);
        manager.index_watch(ntfs_target.clone(), instance.id, ntfs_wd);

        dispatch_event_for_target(&manager, &ext4_target, IN_MODIFY, 0, "data.bin");

        let first = instance.pop_event().expect("ext4 event");
        assert_eq!(first.wd, ext4_wd);
        assert!(instance.pop_event().is_none());

        manager.unindex_watch(&ext4_target, instance.id, ext4_wd);
        manager.unindex_watch(&ntfs_target, instance.id, ntfs_wd);
    }

    #[test]
    fn move_events_keep_cookie_and_order_for_parent_watchers() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let old_parent = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ext4,
            "/mnt/a",
            "ext4:a",
            11,
        );
        let new_parent = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Ext4,
            "/mnt/a",
            "ext4:a",
            12,
        );

        let old_wd = instance.add_watch(old_parent.clone(), "/mnt/a/old", IN_MOVED_FROM);
        manager.index_watch(old_parent.clone(), instance.id, old_wd);
        let new_wd = instance.add_watch(new_parent.clone(), "/mnt/a/new", IN_MOVED_TO);
        manager.index_watch(new_parent.clone(), instance.id, new_wd);

        dispatch_move_between_parent_targets(
            &manager,
            &old_parent,
            &new_parent,
            "old.txt",
            "new.txt",
            false,
        );

        let moved_from = instance.pop_event().expect("moved_from");
        let moved_to = instance.pop_event().expect("moved_to");
        assert_eq!(moved_from.wd, old_wd);
        assert_eq!(moved_from.mask, IN_MOVED_FROM);
        assert_eq!(moved_from.name, "old.txt");
        assert_eq!(moved_to.wd, new_wd);
        assert_eq!(moved_to.mask, IN_MOVED_TO);
        assert_eq!(moved_to.name, "new.txt");
        assert_ne!(moved_from.cookie, 0);
        assert_eq!(moved_from.cookie, moved_to.cookie);
    }

    #[test]
    fn delete_and_move_self_events_target_only_watched_inode() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let file_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Btrfs,
            "/btrfs",
            "btrfs:test",
            257,
        );
        let sibling_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::Btrfs,
            "/btrfs",
            "btrfs:test",
            258,
        );

        let self_wd = instance.add_watch(
            file_target.clone(),
            "/btrfs/hello.txt",
            IN_DELETE_SELF | IN_MOVE_SELF,
        );
        manager.index_watch(file_target.clone(), instance.id, self_wd);
        let sibling_wd = instance.add_watch(
            sibling_target.clone(),
            "/btrfs/other.txt",
            IN_DELETE_SELF | IN_MOVE_SELF,
        );
        manager.index_watch(sibling_target.clone(), instance.id, sibling_wd);

        dispatch_event_for_target(&manager, &file_target, IN_DELETE_SELF, 0, "");
        dispatch_event_for_target(&manager, &file_target, IN_MOVE_SELF, 0, "");

        let delete_self = instance.pop_event().expect("delete self");
        let move_self = instance.pop_event().expect("move self");
        assert_eq!(delete_self.wd, self_wd);
        assert_eq!(delete_self.mask, IN_DELETE_SELF);
        assert_eq!(move_self.wd, self_wd);
        assert_eq!(move_self.mask, IN_MOVE_SELF);
        assert!(instance.pop_event().is_none());

        manager.unindex_watch(&file_target, instance.id, self_wd);
        manager.unindex_watch(&sibling_target, instance.id, sibling_wd);
    }

    #[test]
    fn same_namespace_different_inodes_do_not_collapse() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let alpha_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::F2fs,
            "/",
            "f2fs:root",
            41,
        );
        let beta_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::F2fs,
            "/",
            "f2fs:root",
            42,
        );

        let alpha_wd = instance.add_watch(alpha_target.clone(), "/apps/alpha.txt", IN_OPEN);
        manager.index_watch(alpha_target.clone(), instance.id, alpha_wd);
        let beta_wd = instance.add_watch(beta_target.clone(), "/apps/beta.txt", IN_OPEN);
        manager.index_watch(beta_target.clone(), instance.id, beta_wd);

        dispatch_event_for_target(&manager, &alpha_target, IN_OPEN, 0, "");

        let alpha_event = instance.pop_event().expect("alpha event");
        assert_eq!(alpha_event.wd, alpha_wd);
        assert_eq!(alpha_event.mask, IN_OPEN);
        assert!(instance.pop_event().is_none());

        manager.unindex_watch(&alpha_target, instance.id, alpha_wd);
        manager.unindex_watch(&beta_target, instance.id, beta_wd);
    }

    #[test]
    fn store_style_file_sequences_preserve_parent_and_self_ordering() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let parent_target =
            InotifyWatchTarget::new(crate::fs::vfs_unified::VfsFsType::F2fs, "/", "f2fs:root", 2);
        let file_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::F2fs,
            "/",
            "f2fs:root",
            99,
        );

        let parent_wd = instance.add_watch(
            parent_target.clone(),
            "/phase6",
            IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO,
        );
        manager.index_watch(parent_target.clone(), instance.id, parent_wd);
        let file_wd = instance.add_watch(
            file_target.clone(),
            "/phase6/alpha.txt",
            IN_OPEN
                | IN_ACCESS
                | IN_CLOSE_NOWRITE
                | IN_MODIFY
                | IN_CLOSE_WRITE
                | IN_MOVE_SELF
                | IN_DELETE_SELF,
        );
        manager.index_watch(file_target.clone(), instance.id, file_wd);

        dispatch_store_create_for_parent_target(&manager, &parent_target, "alpha.txt", false);
        dispatch_store_read_for_target(&manager, &file_target);
        dispatch_store_write_for_target(&manager, &file_target);
        dispatch_store_move_for_targets(
            &manager,
            &parent_target,
            &parent_target,
            Some(&file_target),
            "alpha.txt",
            "beta.txt",
            false,
        );

        let events = collect_events(&instance);
        assert_eq!(events.len(), 10);
        assert_eq!(events[0].wd, parent_wd);
        assert_eq!(events[0].mask, IN_CREATE);
        assert_eq!(events[0].name, "alpha.txt");
        assert_eq!(events[1].wd, file_wd);
        assert_eq!(events[1].mask, IN_OPEN);
        assert_eq!(events[2].mask, IN_ACCESS);
        assert_eq!(events[3].mask, IN_CLOSE_NOWRITE);
        assert_eq!(events[4].mask, IN_OPEN);
        assert_eq!(events[5].mask, IN_MODIFY);
        assert_eq!(events[6].mask, IN_CLOSE_WRITE);
        assert_eq!(events[7].wd, parent_wd);
        assert_eq!(events[7].mask, IN_MOVED_FROM);
        assert_eq!(events[7].name, "alpha.txt");
        assert_eq!(events[8].wd, parent_wd);
        assert_eq!(events[8].mask, IN_MOVED_TO);
        assert_eq!(events[8].name, "beta.txt");
        assert_ne!(events[7].cookie, 0);
        assert_eq!(events[7].cookie, events[8].cookie);
        assert_eq!(events[9].wd, file_wd);
        assert_eq!(events[9].mask, IN_MOVE_SELF);

        dispatch_store_delete_for_parent_and_target(
            &manager,
            &parent_target,
            Some(&file_target),
            "beta.txt",
            false,
        );
        let delete_events = collect_events(&instance);
        assert_eq!(delete_events.len(), 2);
        assert_eq!(delete_events[0].wd, parent_wd);
        assert_eq!(delete_events[0].mask, IN_DELETE);
        assert_eq!(delete_events[0].name, "beta.txt");
        assert_eq!(delete_events[1].wd, file_wd);
        assert_eq!(delete_events[1].mask, IN_DELETE_SELF);
    }

    #[test]
    fn store_style_directory_sequences_preserve_isdir_flag() {
        let manager = InotifyManager::new();
        let instance = manager.create_instance(0).expect("instance");

        let parent_target =
            InotifyWatchTarget::new(crate::fs::vfs_unified::VfsFsType::F2fs, "/", "f2fs:root", 2);
        let dir_target = InotifyWatchTarget::new(
            crate::fs::vfs_unified::VfsFsType::F2fs,
            "/",
            "f2fs:root",
            120,
        );

        let parent_wd = instance.add_watch(parent_target.clone(), "/", IN_CREATE | IN_DELETE);
        manager.index_watch(parent_target.clone(), instance.id, parent_wd);
        let dir_wd = instance.add_watch(dir_target.clone(), "/child", IN_DELETE_SELF);
        manager.index_watch(dir_target.clone(), instance.id, dir_wd);

        dispatch_store_create_for_parent_target(&manager, &parent_target, "child", true);
        let create_events = collect_events(&instance);
        assert_eq!(create_events.len(), 1);
        assert_eq!(create_events[0].wd, parent_wd);
        assert_eq!(create_events[0].mask, IN_CREATE | IN_ISDIR);
        assert_eq!(create_events[0].name, "child");

        dispatch_store_delete_for_parent_and_target(
            &manager,
            &parent_target,
            Some(&dir_target),
            "child",
            true,
        );
        let delete_events = collect_events(&instance);
        assert_eq!(delete_events.len(), 2);
        assert_eq!(delete_events[0].wd, parent_wd);
        assert_eq!(delete_events[0].mask, IN_DELETE | IN_ISDIR);
        assert_eq!(delete_events[0].name, "child");
        assert_eq!(delete_events[1].wd, dir_wd);
        assert_eq!(delete_events[1].mask, IN_DELETE_SELF);
    }
}
