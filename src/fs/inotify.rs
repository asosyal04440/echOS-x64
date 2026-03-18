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

/// inotify başlatma bayrakları
pub const IN_CLOEXEC: i32 = 0x02000000; // exec sonrası kapat
pub const IN_NONBLOCK: i32 = 0x00004000; // Engellemesiz mod

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
            name_len: name.len() as u32,
            name: String::from(name),
        }
    }

    /// Calculate total event size (for read buffer)
    pub fn size(&self) -> usize {
        // struct size + name (aligned to sizeof(long))
        let base = core::mem::size_of::<InotifyEventRaw>();
        let name_len = self.name.len();
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
pub struct InotifyWatch {
    /// Watch descriptor (unique per instance)
    pub wd: i32,
    /// Inode number being watched
    pub inode: u64,
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
    pub fn new(wd: i32, inode: u64, path: &str, mask: u32) -> Self {
        Self {
            wd,
            inode,
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
    pub fn add_watch(&self, inode: u64, path: &str, mask: u32) -> i32 {
        // Check for existing watch on same inode
        let mut watches = self.watches.lock();

        for (_, watch) in watches.iter_mut() {
            if watch.inode == inode {
                // Update existing watch
                watch.mask = mask;
                watch.oneshot = (mask & IN_ONESHOT) != 0;
                watch.active = true;
                return watch.wd;
            }
        }

        // Create new watch
        let wd = self.next_wd.fetch_add(1, Ordering::SeqCst);
        let watch = InotifyWatch::new(wd, inode, path, mask);
        watches.insert(wd, watch);

        crate::serial_println!(
            "[INOTIFY] Added watch wd={} inode={:#x} mask={:#x}",
            wd,
            inode,
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
    /// Watch index (inode -> list of wd watching this inode)
    watch_index: Mutex<BTreeMap<u64, Vec<(i32, i32)>>>, // (instance_id, wd)
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
                if let Some(watchers) = watch_index.get_mut(&watch.inode) {
                    watchers.retain(|(iid, w)| *iid != id || *w != *wd);
                }
            }

            crate::serial_println!("[INOTIFY] Removed instance id={}", id);
        }
    }

    /// Add watch to index
    pub fn index_watch(&self, inode: u64, instance_id: i32, wd: i32) {
        let mut watch_index = self.watch_index.lock();
        let entry = watch_index.entry(inode).or_insert_with(Vec::new);
        entry.push((instance_id, wd));
        self.total_watches.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove watch from index
    pub fn unindex_watch(&self, inode: u64, instance_id: i32, wd: i32) {
        let mut watch_index = self.watch_index.lock();
        if let Some(watchers) = watch_index.get_mut(&inode) {
            watchers.retain(|(iid, w)| *iid != instance_id || *w != wd);
        }
        self.total_watches.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get watchers for an inode
    pub fn get_watchers(&self, inode: u64) -> Vec<(i32, i32)> {
        self.watch_index
            .lock()
            .get(&inode)
            .cloned()
            .unwrap_or_default()
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

    let inode = match crate::fs::vfs_unified::VFS_UNIFIED.lock().open(pathname) {
        Ok(info) => info.inode,
        Err(_) => return -2, // ENOENT
    };

    let wd = instance.add_watch(inode, pathname, mask);
    INOTIFY_MANAGER.index_watch(inode, fd, wd);

    wd
}

/// inotify_rm_watch syscall implementation
pub fn sys_inotify_rm_watch(fd: i32, wd: i32) -> i32 {
    let instance = match INOTIFY_MANAGER.get_instance(fd) {
        Some(i) => i,
        None => return -9, // EBADF
    };

    if let Some(watch) = instance.get_watch(wd) {
        INOTIFY_MANAGER.unindex_watch(watch.inode, fd, wd);
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

        let raw = InotifyEventRaw {
            wd: event.wd,
            mask: event.mask,
            cookie: event.cookie,
            name_len: event.name_len,
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

        // Copy name
        let name_bytes = event.name.as_bytes();
        if offset + name_bytes.len() <= buf.len() {
            buf[offset..offset + name_bytes.len()].copy_from_slice(name_bytes);
            offset += name_bytes.len();

            // Align to 8 bytes
            let padding = (8 - (name_bytes.len() % 8)) % 8;
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
    let watchers = INOTIFY_MANAGER.get_watchers(inode);

    if watchers.is_empty() {
        return;
    }

    INOTIFY_MANAGER.total_events.fetch_add(1, Ordering::Relaxed);

    for (instance_id, wd) in watchers {
        if let Some(instance) = INOTIFY_MANAGER.get_instance(instance_id) {
            if let Some(watch) = instance.get_watch(wd) {
                if watch.matches(mask) {
                    instance.push_event(InotifyEvent::new(wd, mask, cookie, name));

                    // Handle oneshot
                    if watch.oneshot {
                        instance.remove_watch(wd);
                        INOTIFY_MANAGER.unindex_watch(inode, instance_id, wd);
                    }
                }
            }
        }
    }

    crate::serial_println!(
        "[INOTIFY] Event: inode={:#x} mask={:#x} name='{}'",
        inode,
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

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Generate unique cookie for move events
fn generate_cookie() -> u32 {
    static COOKIE: AtomicU32 = AtomicU32::new(1);
    COOKIE.fetch_add(1, Ordering::Relaxed)
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
