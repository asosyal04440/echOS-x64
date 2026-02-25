//! # Message Queue (System V and POSIX)
//!
//! Inter-process message queues.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// MESSAGE QUEUE CONSTANTS
// ============================================================================

/// System V IPC constants
pub const IPC_CREAT: i32 = 0x0200;
pub const IPC_EXCL: i32 = 0x0400;
pub const IPC_NOWAIT: i32 = 0x0800;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;
pub const IPC_INFO: i32 = 3;

/// Message queue limits
pub const MSGMAX: usize = 8192;        // Max message size
pub const MSGMNB: usize = 16384;       // Default max bytes on queue
pub const MSGMNI: usize = 128;         // Max message queue IDs
pub const MSGTQL: usize = 256;         // Max messages system-wide

/// POSIX message queue constants
pub const MQ_PRIO_MAX: u32 = 32;

// ============================================================================
// SYSTEM V MESSAGE
// ============================================================================

#[derive(Clone, Debug)]
pub struct SysvMessage {
    /// Message type
    pub mtype: i64,
    /// Message data
    pub data: Vec<u8>,
}

impl SysvMessage {
    pub fn new(mtype: i64, data: Vec<u8>) -> Self {
        Self { mtype, data }
    }
}

// ============================================================================
// SYSTEM V MESSAGE QUEUE
// ============================================================================

pub struct SysvMsgQueue {
    /// Queue ID
    pub id: i32,
    /// Key
    pub key: i32,
    /// Permissions
    pub mode: u16,
    /// Messages
    pub messages: Mutex<Vec<SysvMessage>>,
    /// Max bytes
    pub msg_bytes_max: usize,
    /// Current bytes
    pub current_bytes: AtomicU64,
    /// Max messages
    pub msg_max: usize,
    /// Current messages count
    pub current_msgs: AtomicU32,
    /// Last send PID
    pub lspid: AtomicU32,
    /// Last receive PID
    pub lrpid: AtomicU32,
    /// Last send time
    pub stime: AtomicU64,
    /// Last receive time
    pub rtime: AtomicU64,
    /// Creation time
    pub ctime: AtomicU64,
    /// UID
    pub uid: AtomicU32,
    /// GID
    pub gid: AtomicU32,
}

impl SysvMsgQueue {
    pub fn new(id: i32, key: i32, mode: u16) -> Self {
        Self {
            id,
            key,
            mode,
            messages: Mutex::new(Vec::new()),
            msg_bytes_max: MSGMNB,
            current_bytes: AtomicU64::new(0),
            msg_max: MSGTQL,
            current_msgs: AtomicU32::new(0),
            lspid: AtomicU32::new(0),
            lrpid: AtomicU32::new(0),
            stime: AtomicU64::new(0),
            rtime: AtomicU64::new(0),
            ctime: AtomicU64::new(crate::task::scheduler::get_ticks()),
            uid: AtomicU32::new(0),
            gid: AtomicU32::new(0),
        }
    }

    /// Send message
    pub fn send(&self, msg: SysvMessage, flags: i32) -> Result<(), MsgError> {
        // Check size
        if msg.data.len() > MSGMAX {
            return Err(MsgError::MessageTooLong);
        }

        // Check queue limits
        let current = self.current_bytes.load(Ordering::SeqCst);
        if current + msg.data.len() as u64 > self.msg_bytes_max as u64 {
            if flags & IPC_NOWAIT != 0 {
                return Err(MsgError::WouldBlock);
            }
            // Would wait
            return Err(MsgError::WouldBlock);
        }

        // Add message
        self.messages.lock().push(msg.clone());
        self.current_bytes.fetch_add(msg.data.len() as u64, Ordering::SeqCst);
        self.current_msgs.fetch_add(1, Ordering::SeqCst);
        self.stime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
        self.lspid.store(0, Ordering::SeqCst); // Current PID

        Ok(())
    }

    /// Receive message
    pub fn recv(&self, mtype: i64, flags: i32) -> Result<SysvMessage, MsgError> {
        let mut messages = self.messages.lock();

        // Find message with matching type
        let index = if mtype == 0 {
            // Receive first message
            if messages.is_empty() {
                if flags & IPC_NOWAIT != 0 {
                    return Err(MsgError::WouldBlock);
                }
                return Err(MsgError::WouldBlock);
            }
            Some(0)
        } else if mtype > 0 {
            // Receive first message with type == mtype
            messages.iter().position(|m| m.mtype == mtype)
        } else {
            // Receive first message with type <= |mtype|
            let abs_type = (-mtype) as u64;
            messages.iter().position(|m| m.mtype as u64 <= abs_type)
        };

        if let Some(idx) = index {
            let msg = messages.remove(idx);
            self.current_bytes.fetch_sub(msg.data.len() as u64, Ordering::SeqCst);
            self.current_msgs.fetch_sub(1, Ordering::SeqCst);
            self.rtime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
            self.lrpid.store(0, Ordering::SeqCst); // Current PID
            return Ok(msg);
        }

        if flags & IPC_NOWAIT != 0 {
            return Err(MsgError::WouldBlock);
        }

        Err(MsgError::WouldBlock)
    }

    /// Get statistics
    pub fn get_stats(&self) -> MsqQueueStats {
        MsqQueueStats {
            msg_qbytes: self.msg_bytes_max as u64,
            msg_qnum: self.current_msgs.load(Ordering::SeqCst) as u64,
            msg_lspid: self.lspid.load(Ordering::SeqCst),
            msg_lrpid: self.lrpid.load(Ordering::SeqCst),
            msg_stime: self.stime.load(Ordering::SeqCst),
            msg_rtime: self.rtime.load(Ordering::SeqCst),
            msg_ctime: self.ctime.load(Ordering::SeqCst),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MsqQueueStats {
    pub msg_qbytes: u64,
    pub msg_qnum: u64,
    pub msg_lspid: u32,
    pub msg_lrpid: u32,
    pub msg_stime: u64,
    pub msg_rtime: u64,
    pub msg_ctime: u64,
}

// ============================================================================
// POSIX MESSAGE
// ============================================================================

#[derive(Clone, Debug)]
pub struct PosixMessage {
    /// Priority
    pub priority: u32,
    /// Data
    pub data: Vec<u8>,
}

impl PosixMessage {
    pub fn new(priority: u32, data: Vec<u8>) -> Self {
        Self { priority, data }
    }
}

// ============================================================================
// POSIX MESSAGE QUEUE
// ============================================================================

pub struct PosixMsgQueue {
    /// Queue name
    pub name: String,
    /// Messages (sorted by priority)
    pub messages: Mutex<Vec<PosixMessage>>,
    /// Max messages
    pub mq_maxmsg: u32,
    /// Max message size
    pub mq_msgsize: u32,
    /// Current message count
    pub mq_curmsgs: AtomicU32,
    /// Flags
    pub mq_flags: AtomicU32,
    /// Is non-blocking
    pub nonblocking: AtomicBool,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Notify registration
    pub notify: Mutex<Option<NotifyInfo>>,
}

#[derive(Clone, Debug)]
pub struct NotifyInfo {
    pub pid: u32,
    pub sig: u32,
}

impl PosixMsgQueue {
    pub fn new(name: &str, maxmsg: u32, msgsize: u32) -> Self {
        Self {
            name: String::from(name),
            messages: Mutex::new(Vec::new()),
            mq_maxmsg: maxmsg,
            mq_msgsize: msgsize,
            mq_curmsgs: AtomicU32::new(0),
            mq_flags: AtomicU32::new(0),
            nonblocking: AtomicBool::new(false),
            ref_count: AtomicU32::new(1),
            notify: Mutex::new(None),
        }
    }

    /// Send message
    pub fn send(&self, msg: PosixMessage, _timeout: Option<u64>) -> Result<(), MsgError> {
        // Check size
        if msg.data.len() > self.mq_msgsize as usize {
            return Err(MsgError::MessageTooLong);
        }

        // Check queue full
        if self.mq_curmsgs.load(Ordering::SeqCst) >= self.mq_maxmsg {
            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(MsgError::WouldBlock);
            }
            return Err(MsgError::WouldBlock);
        }

        // Insert sorted by priority (higher priority first)
        let mut messages = self.messages.lock();
        let pos = messages.iter()
            .position(|m| m.priority < msg.priority)
            .unwrap_or(messages.len());
        messages.insert(pos, msg);

        self.mq_curmsgs.fetch_add(1, Ordering::SeqCst);

        // Notify if registered
        if let Some(notify) = self.notify.lock().as_ref() {
            // Send signal
            crate::serial_println!("[MQ] Notify PID {} with signal {}", notify.pid, notify.sig);
        }

        Ok(())
    }

    /// Receive message
    pub fn recv(&self, _timeout: Option<u64>) -> Result<PosixMessage, MsgError> {
        if self.mq_curmsgs.load(Ordering::SeqCst) == 0 {
            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(MsgError::WouldBlock);
            }
            return Err(MsgError::WouldBlock);
        }

        let mut messages = self.messages.lock();
        if let Some(msg) = messages.pop() {
            self.mq_curmsgs.fetch_sub(1, Ordering::SeqCst);
            return Ok(msg);
        }

        Err(MsgError::WouldBlock)
    }

    /// Get attributes
    pub fn get_attr(&self) -> MqAttr {
        MqAttr {
            mq_flags: self.mq_flags.load(Ordering::SeqCst),
            mq_maxmsg: self.mq_maxmsg,
            mq_msgsize: self.mq_msgsize,
            mq_curmsgs: self.mq_curmsgs.load(Ordering::SeqCst),
        }
    }

    /// Set attributes
    pub fn set_attr(&self, flags: u32) {
        self.mq_flags.store(flags, Ordering::SeqCst);
        self.nonblocking.store((flags & O_NONBLOCK) != 0, Ordering::SeqCst);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MqAttr {
    pub mq_flags: u32,
    pub mq_maxmsg: u32,
    pub mq_msgsize: u32,
    pub mq_curmsgs: u32,
}

const O_NONBLOCK: u32 = 0x800;

// ============================================================================
// MESSAGE QUEUE MANAGER
// ============================================================================

pub struct MsgQueueManager {
    /// System V message queues
    sysv_queues: Mutex<BTreeMap<i32, Arc<SysvMsgQueue>>>,
    /// POSIX message queues
    posix_queues: Mutex<BTreeMap<String, Arc<PosixMsgQueue>>>,
    /// Next System V ID
    next_sysv_id: AtomicI32,
    /// Statistics
    stats: Mutex<MsgStats>,
}

#[derive(Clone, Debug, Default)]
pub struct MsgStats {
    pub sysv_queues: u32,
    pub posix_queues: u32,
    pub messages_sent: u64,
    pub messages_received: u64,
}

impl MsgQueueManager {
    pub const fn new() -> Self {
        Self {
            sysv_queues: Mutex::new(BTreeMap::new()),
            posix_queues: Mutex::new(BTreeMap::new()),
            next_sysv_id: AtomicI32::new(1),
            stats: Mutex::new(MsgStats::default()),
        }
    }

    /// Create System V message queue
    pub fn msgget(&self, key: i32, msgflg: i32) -> Result<i32, MsgError> {
        let mut queues = self.sysv_queues.lock();

        // Check if exists
        for queue in queues.values() {
            if queue.key == key && key != 0 {
                if msgflg & IPC_EXCL != 0 {
                    return Err(MsgError::AlreadyExists);
                }
                return Ok(queue.id);
            }
        }

        if msgflg & IPC_CREAT == 0 {
            return Err(MsgError::NotFound);
        }

        let id = self.next_sysv_id.fetch_add(1, Ordering::SeqCst);
        let queue = Arc::new(SysvMsgQueue::new(id, key, (msgflg & 0x1FF) as u16));
        queues.insert(id, queue);

        let mut stats = self.stats.lock();
        stats.sysv_queues += 1;

        Ok(id)
    }

    /// System V control
    pub fn msgctl(&self, msqid: i32, cmd: i32) -> Result<MsqQueueStats, MsgError> {
        let queues = self.sysv_queues.lock();

        match cmd {
            IPC_RMID => {
                drop(queues);
                self.sysv_queues.lock().remove(&msqid);
                Ok(MsqQueueStats {
                    msg_qbytes: 0, msg_qnum: 0, msg_lspid: 0, msg_lrpid: 0,
                    msg_stime: 0, msg_rtime: 0, msg_ctime: 0,
                })
            }
            IPC_STAT => {
                let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;
                Ok(queue.get_stats())
            }
            _ => Err(MsgError::InvalidCommand),
        }
    }

    /// Send System V message
    pub fn msgsnd(&self, msqid: i32, msg: SysvMessage, flags: i32) -> Result<(), MsgError> {
        let queues = self.sysv_queues.lock();
        let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;

        let result = queue.send(msg, flags);

        let mut stats = self.stats.lock();
        stats.messages_sent += 1;

        result
    }

    /// Receive System V message
    pub fn msgrcv(&self, msqid: i32, mtype: i64, flags: i32) -> Result<SysvMessage, MsgError> {
        let queues = self.sysv_queues.lock();
        let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;

        let result = queue.recv(mtype, flags);

        let mut stats = self.stats.lock();
        stats.messages_received += 1;

        result
    }

    /// Create/open POSIX message queue
    pub fn mq_open(&self, name: &str, oflag: i32, mode: u32, attr: Option<MqAttr>) -> Result<Arc<PosixMsgQueue>, MsgError> {
        let mut queues = self.posix_queues.lock();

        if let Some(queue) = queues.get(name) {
            if oflag & IPC_EXCL != 0 {
                return Err(MsgError::AlreadyExists);
            }
            queue.ref_count.fetch_add(1, Ordering::SeqCst);
            return Ok(queue.clone());
        }

        if oflag & IPC_CREAT == 0 {
            return Err(MsgError::NotFound);
        }

        let (maxmsg, msgsize) = if let Some(a) = attr {
            (a.mq_maxmsg, a.mq_msgsize)
        } else {
            (10, 8192)
        };

        let queue = Arc::new(PosixMsgQueue::new(name, maxmsg, msgsize));
        queues.insert(String::from(name), queue.clone());

        let mut stats = self.stats.lock();
        stats.posix_queues += 1;

        Ok(queue)
    }

    /// Close POSIX message queue
    pub fn mq_close(&self, name: &str) -> Result<(), MsgError> {
        if let Some(queue) = self.posix_queues.lock().get(name) {
            queue.ref_count.fetch_sub(1, Ordering::SeqCst);
            return Ok(());
        }
        Err(MsgError::NotFound)
    }

    /// Unlink POSIX message queue
    pub fn mq_unlink(&self, name: &str) -> Result<(), MsgError> {
        self.posix_queues.lock().remove(name);
        Ok(())
    }

    /// Get statistics
    pub fn get_stats(&self) -> MsgStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref MSG_QUEUE_MANAGER: MsgQueueManager = MsgQueueManager::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgError {
    NotFound,
    AlreadyExists,
    WouldBlock,
    MessageTooLong,
    InvalidCommand,
    PermissionDenied,
}

// ============================================================================
// SYSCALL INTERFACE
// ============================================================================

pub fn sys_msgget(key: i32, msgflg: i32) -> i32 {
    match MSG_QUEUE_MANAGER.msgget(key, msgflg) {
        Ok(id) => id,
        Err(MsgError::NotFound) => -2,
        Err(MsgError::AlreadyExists) => -17,
        Err(_) => -22,
    }
}

pub fn sys_msgsnd(msqid: i32, mtype: i64, data: &[u8], flags: i32) -> i32 {
    let msg = SysvMessage::new(mtype, data.to_vec());
    match MSG_QUEUE_MANAGER.msgsnd(msqid, msg, flags) {
        Ok(()) => 0,
        Err(MsgError::WouldBlock) => -11,
        Err(_) => -22,
    }
}

pub fn sys_msgrcv(msqid: i32, mtype: i64, buf: &mut [u8], flags: i32) -> i64 {
    match MSG_QUEUE_MANAGER.msgrcv(msqid, mtype, flags) {
        Ok(msg) => {
            let len = msg.data.len().min(buf.len());
            buf[..len].copy_from_slice(&msg.data[..len]);
            len as i64
        }
        Err(MsgError::WouldBlock) => -11,
        Err(_) => -22,
    }
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[MSGQ] Message queues initialized");
}
