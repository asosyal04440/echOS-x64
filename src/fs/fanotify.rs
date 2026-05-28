//! # fanotify - Filesystem Access Notification
//!
//! Higher-level filesystem notification API compared to inotify.
//! fanotify monitors at mount/filesystem granularity rather than per-directory watches.
//!
//! ## fanotify vs inotify
//!
//! | Aspect          | inotify                     | fanotify                        |
//! |-----------------|-----------------------------|---------------------------------|
//! | Granularity     | Per-directory watch (wd)    | Mount / filesystem mark         |
//! | Event info      | wd + name                   | pid + fd + path                 |
//! | Permission hook | No                          | Yes (FAN_ACCESS_PERM, etc.)     |
//! | Create/delete   | Yes                         | Yes (since Linux 5.1)           |
//! | File handles    | No                          | Optional (FAN_REPORT_FID)       |
//!
//! ## fanotify Event Flow
//!
//! ```text
//!  fanotify_init(flags) → FanotifyGroup
//!          │
//!  fanotify_group.mark("/mnt/data", FAN_MODIFY | FAN_OPEN)
//!          │
//!          ▼
//!  Filesystem operation on /mnt/data
//!          │
//!          ▼
//!  FanotifyEvent { mask, pid, path, fd }
//!          │
//!          ▼
//!  fanotify_group.poll_events() → Vec<FanotifyEvent>
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// FANOTIFY EVENT MASKS (from include/uapi/linux/fanotify.h)
// ============================================================================

pub const FAN_ACCESS: u32 = 0x00000001;
pub const FAN_MODIFY: u32 = 0x00000002;
pub const FAN_ATTRIB: u32 = 0x00000004;
pub const FAN_CLOSE_WRITE: u32 = 0x00000008;
pub const FAN_CLOSE_NOWRITE: u32 = 0x00000010;
pub const FAN_CLOSE: u32 = FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE;
pub const FAN_OPEN: u32 = 0x00000020;
pub const FAN_MOVED_FROM: u32 = 0x00000040;
pub const FAN_MOVED_TO: u32 = 0x00000080;
pub const FAN_CREATE: u32 = 0x00000100;
pub const FAN_DELETE: u32 = 0x00000200;
pub const FAN_DELETE_SELF: u32 = 0x00000400;
pub const FAN_MOVE_SELF: u32 = 0x00000800;
pub const FAN_OPEN_EXEC: u32 = 0x00001000;
pub const FAN_Q_OVERFLOW: u32 = 0x00004000;
pub const FAN_FS_ERROR: u32 = 0x00008000;
pub const FAN_OPEN_PERM: u32 = 0x00010000;
pub const FAN_ACCESS_PERM: u32 = 0x00020000;
pub const FAN_OPEN_EXEC_PERM: u32 = 0x00040000;
pub const FAN_PRE_ACCESS: u32 = 0x00100000;
pub const FAN_MNT_ATTACH: u32 = 0x01000000;
pub const FAN_MNT_DETACH: u32 = 0x02000000;
pub const FAN_EVENT_ON_CHILD: u32 = 0x08000000;
pub const FAN_RENAME: u32 = 0x10000000;
pub const FAN_ONDIR: u32 = 0x40000000;

pub const FAN_MOVE: u32 = FAN_MOVED_FROM | FAN_MOVED_TO;

// ============================================================================
// FANOTIFY RESPONSE (for permission events)
// ============================================================================

/// File descriptor value returned when queue overflows (spec: FAN_NOFD = -1).
pub const FAN_NOFD: i32 = -1;

/// Response values for permission events (written back via write(2)).
pub const FAN_ALLOW: u32 = 0x01;
pub const FAN_DENY: u32 = 0x02;

/// Raw fanotify_event_metadata as returned by read(2).
///
/// Layout matches the Linux kernel spec exactly:
///   event_len(u32) | vers(u8) | reserved(u8) | metadata_len(u16) |
///   mask(u64, aligned) | fd(i32) | pid(i32)
#[repr(C)]
pub struct FanotifyEventMetadata {
    pub event_len: u32,
    pub vers: u8,
    pub reserved: u8,
    pub metadata_len: u16,
    pub mask: u64,
    pub fd: i32,
    pub pid: i32,
}

/// File handle structure for name_to_handle_at / open_by_handle_at.
#[repr(C)]
pub struct FileHandle {
    pub handle_bytes: u32,
    pub handle_type: i32,
    pub f_handle: [u8; 64],
}

impl Default for FileHandle {
    fn default() -> Self {
        Self {
            handle_bytes: 0,
            handle_type: 0,
            f_handle: [0u8; 64],
        }
    }
}

/// Compile-time size check: FanotifyEventMetadata must be 24 bytes on 64-bit.
const _: () = assert!(
    core::mem::size_of::<FanotifyEventMetadata>() == 24,
    "FanotifyEventMetadata must be 24 bytes"
);

/// Version number for fanotify_event_metadata (must match at runtime).
pub const FANOTIFY_METADATA_VERSION: u8 = 3;

/// Response structure written to fanotify fd for permission events.
#[repr(C)]
pub struct FanotifyResponse {
    pub fd: i32,
    pub response: u32,
}

// ============================================================================
// FANOTIFY INIT FLAGS
// ============================================================================

pub const FAN_CLOEXEC: u32 = 0x00000001;
pub const FAN_NONBLOCK: u32 = 0x00000002;
pub const FAN_CLASS_NOTIF: u32 = 0x00000000;
pub const FAN_CLASS_CONTENT: u32 = 0x00000004;
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x00000008;
pub const FAN_UNLIMITED_QUEUE: u32 = 0x00000010;
pub const FAN_UNLIMITED_MARKS: u32 = 0x00000020;
pub const FAN_ENABLE_AUDIT: u32 = 0x00000040;
pub const FAN_REPORT_PIDFD: u32 = 0x00000080;
pub const FAN_REPORT_TID: u32 = 0x00000100;
pub const FAN_REPORT_FID: u32 = 0x00000200;
pub const FAN_REPORT_DIR_FID: u32 = 0x00000400;
pub const FAN_REPORT_NAME: u32 = 0x00000800;
pub const FAN_REPORT_TARGET_FID: u32 = 0x00001000;
pub const FAN_REPORT_FD_ERROR: u32 = 0x00002000;
pub const FAN_REPORT_MNT: u32 = 0x00004000;

// ============================================================================
// FANOTIFY MARK FLAGS
// ============================================================================

pub const FAN_MARK_ADD: u32 = 0x00000001;
pub const FAN_MARK_REMOVE: u32 = 0x00000002;
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x00000004;
pub const FAN_MARK_ONLYDIR: u32 = 0x00000008;
pub const FAN_MARK_MOUNT: u32 = 0x00000010;
pub const FAN_MARK_FILESYSTEM: u32 = 0x00000100;
pub const FAN_MARK_IGNORED_MASK: u32 = 0x00000020;
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x00000040;
pub const FAN_MARK_FLUSH: u32 = 0x00000080;

// ============================================================================
// FANOTIFY EVENT
// ============================================================================

/// A fanotify event carrying process and path context.
#[derive(Clone, Debug)]
pub struct FanotifyEvent {
    /// Event mask (FAN_ACCESS, FAN_MODIFY, FAN_OPEN, etc.)
    pub mask: u32,
    /// Process ID that triggered the event.
    pub pid: u32,
    /// Path of the filesystem object involved.
    pub path: String,
    /// Optional file descriptor for the object (present when not using FAN_REPORT_FID).
    pub fd: Option<i32>,
}

impl FanotifyEvent {
    pub fn new(mask: u32, pid: u32, path: &str, fd: Option<i32>) -> Self {
        Self {
            mask,
            pid,
            path: String::from(path),
            fd,
        }
    }
}

// ============================================================================
// FANOTIFY MARK
// ============================================================================

/// A mark on a mount point or filesystem path.
#[derive(Clone, Debug)]
pub struct FanotifyMark {
    pub mount_point: String,
    pub mask: u32,
    pub ignore_mask: u32,
    pub is_mount_scope: bool,
    pub is_filesystem_scope: bool,
}

impl FanotifyMark {
    pub fn new(mount_point: &str, mask: u32) -> Self {
        Self {
            mount_point: String::from(mount_point),
            mask,
            ignore_mask: 0,
            is_mount_scope: false,
            is_filesystem_scope: false,
        }
    }

    pub fn should_generate_event(&self, event_mask: u32) -> bool {
        (self.mask & event_mask) != 0 && (self.ignore_mask & event_mask) == 0
    }
}

// ============================================================================
// FANOTIFY GROUP
// ============================================================================

/// An fanotify notification group — the central object created by fanotify_init.
///
/// Holds marks, an event queue, and configuration flags.
pub struct FanotifyGroup {
    pub id: u32,
    pub flags: u32,
    pub class: u32,
    marks: Mutex<Vec<FanotifyMark>>,
    event_queue: Mutex<Vec<FanotifyEvent>>,
    /// Permission events that are awaiting a response (blocking the caller).
    /// Keyed by path to allow deduplication.
    pending_permissions: Mutex<BTreeMap<String, FanotifyEvent>>,
    /// Maps per-event fd → path for permission response dispatch.
    /// The fd value corresponds to the fd field in FanotifyEventMetadata
    /// that the user writes back in FanotifyResponse.
    path_by_event_fd: Mutex<BTreeMap<i32, String>>,
    /// Counter for synthetic event fd values.
    next_event_fd: AtomicI32,
    nonblock: bool,
    unlimited_queue: bool,
    max_queue_size: usize,
    /// Enable event coalescing (merge consecutive same-path+same-mask events).
    coalesce: bool,
}

impl FanotifyGroup {
    const DEFAULT_MAX_QUEUE: usize = 8192;

    /// Create a new fanotify group with the given init flags.
    ///
    /// Validates that exactly one notification class is specified and that
    /// reserved flag combinations are rejected.
    pub fn new(flags: u32) -> Result<Self, &'static str> {
        let class = flags & 0x0000000C;
        if class != FAN_CLASS_NOTIF && class != FAN_CLASS_CONTENT && class != FAN_CLASS_PRE_CONTENT
        {
            return Err("invalid or conflicting notification class");
        }

        let nonblock = (flags & FAN_NONBLOCK) != 0;
        let unlimited_queue = (flags & FAN_UNLIMITED_QUEUE) != 0;

        let group = Self {
            id: next_group_id(),
            flags,
            class,
            marks: Mutex::new(Vec::new()),
            event_queue: Mutex::new(Vec::new()),
            pending_permissions: Mutex::new(BTreeMap::new()),
            path_by_event_fd: Mutex::new(BTreeMap::new()),
            next_event_fd: AtomicI32::new(1),
            nonblock,
            unlimited_queue,
            max_queue_size: Self::DEFAULT_MAX_QUEUE,
            coalesce: true, // default: coalesce enabled per fanotify(7)
        };

        crate::serial_println!(
            "[FANOTIFY] Created group id={} flags={:#x} class={:#x}",
            group.id,
            flags,
            class
        );

        Ok(group)
    }

    /// Add or modify a mark on a mount point or directory.
    ///
    /// The `mask` argument uses FAN_* event constants. The mount_point string
    /// identifies the filesystem object to monitor.
    pub fn mark(&self, mount_point: &str, mask: u32) -> Result<(), &'static str> {
        if mask == 0 {
            return Err("mark mask must be non-zero");
        }

        let mut marks = self.marks.lock();

        for existing in marks.iter_mut() {
            if existing.mount_point == mount_point {
                existing.mask |= mask;
                crate::serial_println!(
                    "[FANOTIFY] Updated mark on '{}' mask={:#x}",
                    mount_point,
                    existing.mask
                );
                return Ok(());
            }
        }

        let mut mark = FanotifyMark::new(mount_point, mask);
        mark.is_mount_scope = (self.flags & FAN_REPORT_MNT) != 0;
        mark.is_filesystem_scope = false;
        marks.push(mark);

        crate::serial_println!(
            "[FANOTIFY] Added mark on '{}' mask={:#x}",
            mount_point,
            mask
        );

        Ok(())
    }

    /// Remove a mark from a mount point.
    pub fn unmark(&self, mount_point: &str) -> Result<(), &'static str> {
        let mut marks = self.marks.lock();
        let before = marks.len();
        marks.retain(|m| m.mount_point != mount_point);

        if marks.len() == before {
            return Err("no mark found for mount point");
        }

        crate::serial_println!("[FANOTIFY] Removed mark on '{}'", mount_point);
        Ok(())
    }

    /// Poll and drain all pending events from the queue.
    ///
    /// Returns the events in FIFO order. The queue is emptied after this call.
    pub fn poll_events(&self) -> Vec<FanotifyEvent> {
        let mut queue = self.event_queue.lock();
        let mut events = Vec::with_capacity(queue.len());
        for ev in queue.drain(..) {
            events.push(ev);
        }
        events
    }

    /// Push an event into the group's queue.
    ///
    /// Respects the queue size limit unless FAN_UNLIMITED_QUEUE was set.
    /// When the queue is full an FAN_Q_OVERFLOW event is injected.
    ///
    /// Coalescing: if the last event in the queue has the same path and mask,
    /// it is merged rather than duplicated (per fanotify(7) coalescing rule).
    ///
    /// Permission events (FAN_ACCESS_PERM, FAN_OPEN_PERM, FAN_OPEN_EXEC_PERM)
    /// are placed in the pending_permissions map and block the caller until
    /// a FAN_ALLOW or FAN_DENY response is received.
    pub fn push_event(&self, event: FanotifyEvent) {
        // Permission events: block until response
        if event.mask & (FAN_ACCESS_PERM | FAN_OPEN_PERM | FAN_OPEN_EXEC_PERM) != 0 {
            let mask_copy = event.mask;
            let path_copy = event.path.clone();
            // Assign a synthetic event fd for the response lookup
            let event_fd = self.next_event_fd.fetch_add(1, Ordering::SeqCst);
            let mut pending = self.pending_permissions.lock();
            pending.insert(event.path.clone(), event);
            let mut fd_map = self.path_by_event_fd.lock();
            fd_map.insert(event_fd, path_copy.clone());
            crate::serial_println!(
                "[FANOTIFY] Permission event pending for path='{}' mask={:#x} group={} event_fd={}",
                path_copy,
                mask_copy,
                self.id,
                event_fd,
            );
            return;
        }

        let mut queue = self.event_queue.lock();

        if !self.unlimited_queue && queue.len() >= self.max_queue_size {
            let overflow = FanotifyEvent::new(FAN_Q_OVERFLOW, 0, "", None);
            queue.push(overflow);
            crate::serial_println!(
                "[FANOTIFY] Queue overflow for group id={}, injecting FAN_Q_OVERFLOW",
                self.id
            );
            return;
        }

        // Coalescing: merge with last event if same path+mask
        if self.coalesce {
            if let Some(last) = queue.last() {
                if last.path == event.path && last.mask == event.mask {
                    // Same event, skip duplicate (coalesce)
                    return;
                }
            }
        }

        queue.push(event);
    }

    /// Respond to a pending permission event.
    ///
    /// `response` must be FAN_ALLOW or FAN_DENY.
    /// Returns true if the permission event was found and responded to.
    /// Per fanotify(7): FAN_DENY blocks the filesystem operation with EPERM.
    pub fn respond_permission(&self, path: &str, response: u32) -> Result<bool, &'static str> {
        let mut pending = self.pending_permissions.lock();
        let event = pending.remove(path);
        // Clean up the fd→path map for this path
        {
            let mut fd_map = self.path_by_event_fd.lock();
            fd_map.retain(|_, v| v != path);
        }

        match event {
            Some(ev) => {
                if response == FAN_ALLOW {
                    crate::serial_println!(
                        "[FANOTIFY] Permission ALLOWED for path='{}' group={}",
                        path,
                        self.id
                    );
                    // Allow the operation — no event pushed to queue
                    Ok(true)
                } else if response == FAN_DENY {
                    crate::serial_println!(
                        "[FANOTIFY] Permission DENIED for path='{}' group={}",
                        path,
                        self.id
                    );
                    // Deny — push a notification event so the listener knows
                    let deny_event = FanotifyEvent::new(ev.mask | 0x80000000, ev.pid, path, None);
                    drop(pending);
                    self.push_event(deny_event);
                    Ok(true)
                } else {
                    Err("invalid fanotify response (must be FAN_ALLOW or FAN_DENY)")
                }
            }
            None => Ok(false),
        }
    }

    /// Check if a path has a pending permission block.
    ///
    /// Returns true if the filesystem operation on `path` should be blocked
    /// awaiting a fanotify response.
    pub fn is_permission_blocked(&self, path: &str) -> bool {
        self.pending_permissions.lock().contains_key(path)
    }

    /// Check if the group has pending events.
    pub fn has_events(&self) -> bool {
        !self.event_queue.lock().is_empty()
    }

    /// Get the number of marks registered with this group.
    pub fn mark_count(&self) -> usize {
        self.marks.lock().len()
    }

    /// Flush all marks from this group.
    pub fn flush_marks(&self) {
        self.marks.lock().clear();
        crate::serial_println!("[FANOTIFY] Flushed all marks for group id={}", self.id);
    }

    /// Find a matching mark for the given path and event mask.
    ///
    /// Returns a copy of the mark if one matches, used by the event dispatch
    /// path to decide whether an event should be generated.
    pub fn find_mark_for_path(&self, path: &str) -> Option<FanotifyMark> {
        let marks = self.marks.lock();
        for mark in marks.iter() {
            if path.starts_with(&mark.mount_point) && mark.mask != 0 {
                return Some(mark.clone());
            }
        }
        None
    }
}

// ============================================================================
// GLOBAL FANOTIFY REGISTRY
// ============================================================================

static FANOTIFY_GROUP_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_group_id() -> u32 {
    FANOTIFY_GROUP_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Global fanotify group registry.
pub struct FanotifyRegistry {
    groups: Mutex<Vec<alloc::sync::Arc<FanotifyGroup>>>,
    total_events: AtomicU64,
}

impl FanotifyRegistry {
    pub const fn new() -> Self {
        Self {
            groups: Mutex::new(Vec::new()),
            total_events: AtomicU64::new(0),
        }
    }

    pub fn register_group(&self, group: alloc::sync::Arc<FanotifyGroup>) {
        self.groups.lock().push(group);
    }

    pub fn get_group(&self, id: u32) -> Option<alloc::sync::Arc<FanotifyGroup>> {
        self.groups.lock().iter().find(|g| g.id == id).cloned()
    }

    pub fn remove_group(&self, id: u32) {
        self.groups.lock().retain(|g| g.id != id);
    }

    pub fn record_event(&self) {
        self.total_events.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total_events(&self) -> u64 {
        self.total_events.load(Ordering::Relaxed)
    }

    pub fn group_count(&self) -> usize {
        self.groups.lock().len()
    }
}

lazy_static::lazy_static! {
    static ref FANOTIFY_REGISTRY: FanotifyRegistry = FanotifyRegistry::new();
}

// ============================================================================
// EVENT DISPATCH (called by filesystem layer)
// ============================================================================

/// Dispatch a fanotify event to all groups that have a matching mark.
///
/// This is the entry point called by the VFS/filesystem layer when a file
/// operation occurs. It iterates all registered groups, checks marks, and
/// pushes events into matching groups.
pub fn dispatch_event(path: &str, mask: u32, pid: u32) {
    let groups = FANOTIFY_REGISTRY.groups.lock();

    for group in groups.iter() {
        if let Some(mark) = group.find_mark_for_path(path) {
            if mark.should_generate_event(mask) {
                let event = FanotifyEvent::new(mask, pid, path, None);
                group.push_event(event);
                FANOTIFY_REGISTRY.record_event();
            }
        }
    }
}

/// Convenience: dispatch an access event.
pub fn notify_access(path: &str, pid: u32) {
    dispatch_event(path, FAN_ACCESS, pid);
}

/// Convenience: dispatch a modify event.
pub fn notify_modify(path: &str, pid: u32) {
    dispatch_event(path, FAN_MODIFY, pid);
}

/// Convenience: dispatch an open event.
pub fn notify_open(path: &str, pid: u32) {
    dispatch_event(path, FAN_OPEN, pid);
}

/// Convenience: dispatch an open-for-exec event.
pub fn notify_open_exec(path: &str, pid: u32) {
    dispatch_event(path, FAN_OPEN_EXEC, pid);
}

/// Convenience: dispatch a close-write event.
pub fn notify_close_write(path: &str, pid: u32) {
    dispatch_event(path, FAN_CLOSE_WRITE, pid);
}

/// Convenience: dispatch a close-nowrite event.
pub fn notify_close_nowrite(path: &str, pid: u32) {
    dispatch_event(path, FAN_CLOSE_NOWRITE, pid);
}

/// Convenience: dispatch a create event.
pub fn notify_create(parent_path: &str, pid: u32) {
    dispatch_event(parent_path, FAN_CREATE, pid);
}

/// Convenience: dispatch a delete event.
pub fn notify_delete(parent_path: &str, pid: u32) {
    dispatch_event(parent_path, FAN_DELETE, pid);
}

/// Convenience: dispatch a moved-from event.
pub fn notify_moved_from(path: &str, pid: u32) {
    dispatch_event(path, FAN_MOVED_FROM, pid);
}

/// Convenience: dispatch a moved-to event.
pub fn notify_moved_to(path: &str, pid: u32) {
    dispatch_event(path, FAN_MOVED_TO, pid);
}

/// Convenience: dispatch a permission access event (blocks until response).
pub fn notify_access_perm(path: &str, pid: u32) {
    dispatch_event(path, FAN_ACCESS_PERM, pid);
}

/// Convenience: dispatch a permission open event (blocks until response).
pub fn notify_open_perm(path: &str, pid: u32) {
    dispatch_event(path, FAN_OPEN_PERM, pid);
}

/// Convenience: dispatch a permission open-for-exec event (blocks until response).
pub fn notify_open_exec_perm(path: &str, pid: u32) {
    dispatch_event(path, FAN_OPEN_EXEC_PERM, pid);
}

/// Check if any group has a pending permission block for the given path.
///
/// Used by the VFS to decide whether to block a filesystem operation.
pub fn is_any_permission_blocked(path: &str) -> bool {
    let groups = FANOTIFY_REGISTRY.groups.lock();
    for group in groups.iter() {
        if group.is_permission_blocked(path) {
            return true;
        }
    }
    false
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Initialize the fanotify subsystem.
pub fn init() {
    crate::serial_println!("[FANOTIFY] Subsystem initialized");
}

/// fanotify_init syscall implementation.
///
/// Creates a new fanotify notification group and returns its ID.
/// The `flags` argument combines FAN_CLOEXEC, FAN_NONBLOCK, notification class,
/// and other init-time options.
pub fn sys_fanotify_init(flags: u32, _event_f_flags: u32) -> i32 {
    match FanotifyGroup::new(flags) {
        Ok(group) => {
            let arc = alloc::sync::Arc::new(group);
            let id = arc.id;
            FANOTIFY_REGISTRY.register_group(arc);
            id as i32
        }
        Err(_) => -22, // EINVAL
    }
}

/// fanotify_mark syscall implementation.
///
/// Adds, removes, or modifies a mark on a filesystem object.
/// `fanotify_fd` is the group ID returned by fanotify_init.
/// `flags` uses FAN_MARK_* constants.
/// `mask` uses FAN_* event constants.
pub fn sys_fanotify_mark(fanotify_fd: i32, flags: u32, mask: u32, mount_point: &str) -> i32 {
    let group = match FANOTIFY_REGISTRY.get_group(fanotify_fd as u32) {
        Some(g) => g,
        None => return -9, // EBADF
    };

    if (flags & FAN_MARK_FLUSH) != 0 {
        group.flush_marks();
        return 0;
    }

    if (flags & FAN_MARK_REMOVE) != 0 {
        match group.unmark(mount_point) {
            Ok(()) => 0,
            Err(_) => -22, // EINVAL
        }
    } else {
        match group.mark(mount_point, mask) {
            Ok(()) => 0,
            Err(_) => -22, // EINVAL
        }
    }
}

/// Read events from a fanotify group.
///
/// Returns events in a Vec. In non-blocking mode returns empty vec if no events.
pub fn sys_fanotify_read(fanotify_fd: i32) -> Vec<FanotifyEvent> {
    let group = match FANOTIFY_REGISTRY.get_group(fanotify_fd as u32) {
        Some(g) => g,
        None => return Vec::new(),
    };

    if group.nonblock && !group.has_events() {
        return Vec::new();
    }

    group.poll_events()
}

/// Close a fanotify group.
pub fn sys_fanotify_close(fanotify_fd: i32) -> i32 {
    FANOTIFY_REGISTRY.remove_group(fanotify_fd as u32);
    crate::serial_println!("[FANOTIFY] Closed group id={}", fanotify_fd);
    0
}

/// Write a permission response to a fanotify group.
///
/// The `response` bytes must contain a `FanotifyResponse` struct with the fd
/// from the permission event and either FAN_ALLOW or FAN_DENY.
pub fn sys_fanotify_write(fanotify_fd: i32, response: FanotifyResponse) -> i32 {
    let group = match FANOTIFY_REGISTRY.get_group(fanotify_fd as u32) {
        Some(g) => g,
        None => return -9, // EBADF
    };

    if response.response != FAN_ALLOW && response.response != FAN_DENY {
        return -22; // EINVAL
    }

    // Look up the event path by the event fd
    let path = {
        let fd_map = group.path_by_event_fd.lock();
        fd_map.get(&response.fd).cloned()
    };
    let path = match path {
        Some(p) => p,
        None => {
            crate::serial_println!(
                "[FANOTIFY] No pending permission found for event_fd={}",
                response.fd
            );
            return -2; // ENOENT
        }
    };

    match group.respond_permission(&path, response.response) {
        Ok(true) => {
            crate::serial_println!(
                "[FANOTIFY] Permission resolved: event_fd={} path='{}' response={}",
                response.fd,
                path,
                response.response
            );
            0
        }
        Ok(false) => {
            crate::serial_println!(
                "[FANOTIFY] Permission already resolved for event_fd={} path='{}'",
                response.fd,
                path
            );
            -2 // ENOENT
        }
        Err(e) => {
            crate::serial_println!(
                "[FANOTIFY] Permission response error for event_fd={}: {}",
                response.fd,
                e
            );
            -22 // EINVAL
        }
    }
}

/// Get fanotify statistics.
pub struct FanotifyStats {
    pub group_count: usize,
    pub total_events: u64,
}

pub fn get_stats() -> FanotifyStats {
    FanotifyStats {
        group_count: FANOTIFY_REGISTRY.group_count(),
        total_events: FANOTIFY_REGISTRY.total_events(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fanotify_group_creation_and_marking() {
        let group = FanotifyGroup::new(FAN_CLASS_NOTIF | FAN_NONBLOCK).expect("group");
        assert_eq!(group.mark_count(), 0);

        group
            .mark("/mnt/data", FAN_MODIFY | FAN_OPEN)
            .expect("mark");
        assert_eq!(group.mark_count(), 1);

        let mark = group
            .find_mark_for_path("/mnt/data/file.txt")
            .expect("mark");
        assert!((mark.mask & FAN_MODIFY) != 0);
        assert!((mark.mask & FAN_OPEN) != 0);
    }

    #[test]
    fn fanotify_event_dispatch_and_poll() {
        let group = alloc::sync::Arc::new(FanotifyGroup::new(FAN_CLASS_NOTIF).expect("group"));
        FANOTIFY_REGISTRY.register_group(group.clone());

        group.mark("/test", FAN_ACCESS | FAN_MODIFY).expect("mark");

        dispatch_event("/test/hello.txt", FAN_ACCESS, 1234);
        dispatch_event("/test/hello.txt", FAN_MODIFY, 1234);

        let events = group.poll_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].mask, FAN_ACCESS);
        assert_eq!(events[0].pid, 1234);
        assert_eq!(events[0].path, "/test/hello.txt");
        assert_eq!(events[1].mask, FAN_MODIFY);

        FANOTIFY_REGISTRY.remove_group(group.id);
    }

    #[test]
    fn fanotify_ignore_mask_blocks_events() {
        let group = FanotifyGroup::new(FAN_CLASS_NOTIF).expect("group");
        group.mark("/mnt", FAN_ACCESS | FAN_MODIFY).expect("mark");

        {
            let mut marks = group.marks.lock();
            marks[0].ignore_mask = FAN_ACCESS;
        }

        assert!(group
            .find_mark_for_path("/mnt/file")
            .expect("mark")
            .should_generate_event(FAN_MODIFY));
        assert!(!group
            .find_mark_for_path("/mnt/file")
            .expect("mark")
            .should_generate_event(FAN_ACCESS));
    }

    #[test]
    fn fanotify_unmark_and_flush() {
        let group = FanotifyGroup::new(FAN_CLASS_NOTIF).expect("group");
        group.mark("/a", FAN_OPEN).expect("mark");
        group.mark("/b", FAN_MODIFY).expect("mark");
        assert_eq!(group.mark_count(), 2);

        group.unmark("/a").expect("unmark");
        assert_eq!(group.mark_count(), 1);

        group.flush_marks();
        assert_eq!(group.mark_count(), 0);
    }

    #[test]
    fn fanotify_invalid_class_rejected() {
        let result = FanotifyGroup::new(0x0000000C | 0x00000008);
        assert!(result.is_err());
    }

    #[test]
    fn fanotify_overflow_injects_q_overflow_event() {
        let mut group = FanotifyGroup::new(FAN_CLASS_NOTIF).expect("group");
        group.max_queue_size = 2;
        group.unlimited_queue = false;

        group.push_event(FanotifyEvent::new(FAN_ACCESS, 1, "/a", None));
        group.push_event(FanotifyEvent::new(FAN_MODIFY, 1, "/b", None));
        group.push_event(FanotifyEvent::new(FAN_OPEN, 1, "/c", None));

        let events = group.poll_events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].mask, FAN_Q_OVERFLOW);
    }
}
