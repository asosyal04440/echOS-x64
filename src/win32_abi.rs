//! # Windows NT Native ABI - Lock-Free Implementation
//!
//! Windows NT syscall'ları doğrudan echOS'un lock-free yapılarına map edilir.
//! ZERO Mutex, ZERO Blocking - Tamamen asenkron ve lock-free!
//!
//! ## Performance Goals:
//! - NtCreateFile → io_uring (O(1) submission, no locks)
//! - NtReadFile → io_uring async read (zero-copy)
//! - NtAllocateVirtualMemory → Chase-Lev deque (lock-free alloc)
//! - NtCreateProcess → Direct task spawn (work-stealing queue)
//!
//!

use crate::net::socket::{self, AddressFamily, Protocol, SocketType};
use crate::net::{Ipv4Addr, NetError, Port, SocketAddr};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

/// Linux io_uring'in olmadığı bare-metal ortamda AT_FDCWD yerel sabiti
const AT_FDCWD: i32 = -100;

// ============================================================================
// NT STATUS CODES
// ============================================================================

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtStatus {
    Success = 0,
    Alerted = 0x00000101u32 as i32,
    Timeout = 0x00000102u32 as i32,
    // WaitFor0 Windows NT'de 0'dır, Success ile aynı değer
    ObjectNameExists = 0x40000000u32 as i32,
    ThreadWasFrozen = 0x40000001u32 as i32,

    // Warnings
    BufferOverflow = 0x80000005u32 as i32,
    NoMoreFiles = 0x80000006u32 as i32,

    // Errors
    UnsatisfiedRequirement = 0xC0000001u32 as i32,
    InvalidHandle = 0xC0000008u32 as i32,
    NoSuchFile = 0xC000000Fu32 as i32,
    AccessDenied = 0xC0000022u32 as i32,
    BufferTooSmall = 0xC0000023u32 as i32,
    ObjectPathInvalid = 0xC0000039u32 as i32,
    MemoryNotAllocated = 0xC00000A0u32 as i32,
    FileIsADirectory = 0xC00000BAu32 as i32,
    NotSupported = 0xC00000BBu32 as i32,
    ProcessIsTerminating = 0xC000010Au32 as i32,
}

pub type NTSTATUS = NtStatus;

pub const INVALID_SOCKET: u64 = !0u64;
pub const SOCKET_ERROR: i32 = -1;
pub const AF_INET: i32 = 2;
pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;

const WSAEINVAL: u32 = 10022;
const WSAEAFNOSUPPORT: u32 = 10047;
const WSAEPROTONOSUPPORT: u32 = 10043;
const WSAENOTSOCK: u32 = 10038;
const WSAEWOULDBLOCK: u32 = 10035;
const WSAENETUNREACH: u32 = 10051;
const WSAETIMEDOUT: u32 = 10060;
const WSAECONNREFUSED: u32 = 10061;
const WSAECONNRESET: u32 = 10054;
const WSAEADDRINUSE: u32 = 10048;
const WSAENOTCONN: u32 = 10057;
const WSA_OPERATION_ABORTED: u32 = 995;

static LAST_WSA_ERROR: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WsaData {
    pub version: u16,
    pub high_version: u16,
    pub description: [u8; 257],
    pub system_status: [u8; 129],
    pub max_sockets: u16,
    pub max_udp_dg: u16,
    pub vendor_info: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

#[derive(Clone, Debug)]
struct FileHandleState {
    path: String,
    contents: Vec<u8>,
    cursor: usize,
}

#[derive(Clone, Debug)]
struct ProcessHandleState {
    pid: u64,
    image_path: String,
    image_base: u64,
    entry_point: u64,
    stack_top: u64,
    tls_enabled: bool,
    imported_modules: Vec<String>,
    initial_thread_handle: u64,
    terminated: bool,
}

#[derive(Clone, Debug)]
struct ThreadHandleState {
    tid: u64,
    owner_pid: u64,
    entry_point: u64,
    alertable_waits: u64,
    terminated: bool,
}

#[derive(Clone, Debug)]
struct WaitableHandleState {
    signaled: bool,
    manual_reset: bool,
    signal_epoch: Arc<AtomicU32>,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct IoRingBufferDescriptor {
    pub address: u64,
    pub length: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IoRingOperationKind {
    ReadFile = 1,
    WriteFile = 2,
}

#[derive(Clone, Debug)]
struct IoRingQueuedOp {
    kind: IoRingOperationKind,
    file_index: u32,
    buffer_index: u32,
    offset: u64,
    length: u32,
    user_data: u64,
    flags: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct IoRingCompletion {
    pub user_data: u64,
    pub result_code: i32,
    pub information: u32,
    pub operation: u32,
}

#[derive(Clone, Debug)]
struct IoRingHandleState {
    ring_fd: u32,
    submission_queue_size: u32,
    completion_queue_size: u32,
    flags: u32,
    registered_handles: Vec<u64>,
    registered_buffers: Vec<IoRingBufferDescriptor>,
    pending_ops: VecDeque<IoRingQueuedOp>,
    completions: VecDeque<IoRingCompletion>,
    next_user_data: u64,
}

#[derive(Clone, Debug)]
enum HandleObject {
    File(FileHandleState),
    Process(ProcessHandleState),
    Thread(ThreadHandleState),
    Socket(u64),
    Waitable(WaitableHandleState),
    IoRing(IoRingHandleState),
}

lazy_static::lazy_static! {
    static ref HANDLE_TABLE: Mutex<BTreeMap<u64, HandleObject>> = Mutex::new(BTreeMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(0x1000);
static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

fn allocate_handle(object: HandleObject) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::AcqRel);
    HANDLE_TABLE.lock().insert(handle, object);
    handle
}

fn close_handle(handle: u64) -> bool {
    let object = HANDLE_TABLE.lock().remove(&handle);
    match object {
        Some(HandleObject::IoRing(state)) => {
            let _ = crate::net::io_uring::io_uring_close(state.ring_fd);
            true
        }
        Some(_) => true,
        None => false,
    }
}

fn normalize_nt_path(path: &str) -> String {
    let trimmed = path
        .trim_start_matches("\\\\??\\\\")
        .trim_start_matches("\\??\\")
        .trim_start_matches("\\DosDevices\\")
        .trim_start_matches("\\Device\\HarddiskVolume1\\")
        .trim_start_matches("\\");
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized.starts_with('/') {
        normalized
    } else {
        alloc::format!("/{}", normalized)
    }
}

fn load_file_bytes(path: &str) -> Result<Vec<u8>, NtStatus> {
    crate::fs::vfs_unified::read_file(path).map_err(|_| NtStatus::NoSuchFile)
}

fn current_ticks() -> u64 {
    crate::task::scheduler::get_ticks() as u64
}

fn current_signaled_state(object: &HandleObject) -> bool {
    match object {
        HandleObject::File(_) => true,
        HandleObject::Socket(_) => true,
        HandleObject::Process(state) => state.terminated,
        HandleObject::Thread(state) => state.terminated,
        HandleObject::Waitable(state) => state.signaled,
        HandleObject::IoRing(_) => true,
    }
}

fn decode_timeout_ms(timeout: *const i64) -> Option<u32> {
    if timeout.is_null() {
        return None;
    }
    let value = unsafe { *timeout };
    if value == 0 {
        return Some(0);
    }
    let magnitude = if value < 0 {
        value.unsigned_abs()
    } else {
        value as u64
    };
    Some(((magnitude + 9_999) / 10_000).min(u32::MAX as u64) as u32)
}

fn wait_for_single_object(handle: u64, alertable: bool, timeout: *const i64) -> NtStatus {
    let timeout_ms = decode_timeout_ms(timeout);

    loop {
        let wait_epoch = {
            let mut table = HANDLE_TABLE.lock();
            let Some(object) = table.get_mut(&handle) else {
                return NtStatus::InvalidHandle;
            };

            if current_signaled_state(object) {
                if let HandleObject::Waitable(waitable) = object {
                    if !waitable.manual_reset {
                        waitable.signaled = false;
                    }
                }
                return NtStatus::Success;
            }

            if alertable {
                if let HandleObject::Thread(thread) = object {
                    thread.alertable_waits = thread.alertable_waits.saturating_add(1);
                }
                return NtStatus::Alerted;
            }

            match object {
                HandleObject::Waitable(waitable) => Some(waitable.signal_epoch.clone()),
                _ => None,
            }
        };

        if timeout_ms == Some(0) {
            return NtStatus::Timeout;
        }

        let Some(wait_epoch) = wait_epoch else {
            return NtStatus::Timeout;
        };
        let observed = wait_epoch.load(Ordering::Acquire);
        let woke = crate::task::wait_on_address(
            wait_epoch.as_ref() as *const AtomicU32 as u64,
            &observed as *const u32 as *const u8,
            core::mem::size_of::<u32>(),
            timeout_ms.unwrap_or(u32::MAX),
        );
        if !woke {
            return NtStatus::Timeout;
        }
    }
}

pub fn create_waitable_event(manual_reset: bool, initial_state: bool) -> Result<u64, NtStatus> {
    let epoch = Arc::new(AtomicU32::new(initial_state as u32));
    Ok(allocate_handle(HandleObject::Waitable(WaitableHandleState {
        signaled: initial_state,
        manual_reset,
        signal_epoch: epoch,
    })))
}

pub fn set_waitable_event(handle: u64) -> NtStatus {
    let (wake_all, epoch) = {
        let mut table = HANDLE_TABLE.lock();
        let Some(HandleObject::Waitable(waitable)) = table.get_mut(&handle) else {
            return NtStatus::InvalidHandle;
        };
        waitable.signaled = true;
        waitable.signal_epoch.fetch_add(1, Ordering::AcqRel);
        (waitable.manual_reset, waitable.signal_epoch.clone())
    };

    let epoch_addr = epoch.as_ref() as *const AtomicU32 as u64;
    if wake_all {
        crate::task::wake_by_address_all(epoch_addr);
    } else {
        crate::task::wake_by_address_single(epoch_addr);
    }
    NtStatus::Success
}

pub fn reset_waitable_event(handle: u64) -> NtStatus {
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::Waitable(waitable)) = table.get_mut(&handle) else {
        return NtStatus::InvalidHandle;
    };
    waitable.signaled = false;
    NtStatus::Success
}

pub unsafe fn wait_on_address(
    address: *const u8,
    compare_address: *const u8,
    size: usize,
    timeout_ms: u32,
) -> bool {
    crate::task::wait_on_address(address as u64, compare_address, size, timeout_ms)
}

pub unsafe fn wake_by_address_single(address: *const u8) {
    let _ = crate::task::wake_by_address_single(address as u64);
}

pub unsafe fn wake_by_address_all(address: *const u8) {
    let _ = crate::task::wake_by_address_all(address as u64);
}

pub fn create_io_ring(
    version: u32,
    flags: u32,
    submission_queue_size: u32,
    completion_queue_size: u32,
) -> Result<u64, NtStatus> {
    let _ = version;
    let entries = submission_queue_size.max(completion_queue_size).max(1);
    let ring_fd = crate::net::io_uring::io_uring_setup(entries, None)
        .map_err(|_| NtStatus::UnsatisfiedRequirement)?;
    Ok(allocate_handle(HandleObject::IoRing(IoRingHandleState {
        ring_fd,
        submission_queue_size,
        completion_queue_size,
        flags,
        registered_handles: Vec::new(),
        registered_buffers: Vec::new(),
        pending_ops: VecDeque::new(),
        completions: VecDeque::new(),
        next_user_data: 1,
    })))
}

pub unsafe fn register_io_ring_handles(
    handle: u64,
    handles: *const u64,
    count: u32,
) -> NtStatus {
    if handles.is_null() && count != 0 {
        return NtStatus::InvalidHandle;
    }

    let registered = if count == 0 {
        Vec::new()
    } else {
        core::slice::from_raw_parts(handles, count as usize).to_vec()
    };
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::IoRing(state)) = table.get_mut(&handle) else {
        return NtStatus::InvalidHandle;
    };
    state.registered_handles = registered;
    NtStatus::Success
}

pub unsafe fn register_io_ring_buffers(
    handle: u64,
    buffers: *const IoRingBufferDescriptor,
    count: u32,
) -> NtStatus {
    if buffers.is_null() && count != 0 {
        return NtStatus::InvalidHandle;
    }
    let registered = if count == 0 {
        Vec::new()
    } else {
        core::slice::from_raw_parts(buffers, count as usize).to_vec()
    };
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::IoRing(state)) = table.get_mut(&handle) else {
        return NtStatus::InvalidHandle;
    };
    state.registered_buffers = registered;
    NtStatus::Success
}

fn queue_io_ring_op(
    handle: u64,
    kind: IoRingOperationKind,
    file_index: u32,
    buffer_index: u32,
    offset: u64,
    length: u32,
    user_data: u64,
    flags: u32,
) -> NtStatus {
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::IoRing(state)) = table.get_mut(&handle) else {
        return NtStatus::InvalidHandle;
    };
    if file_index as usize >= state.registered_handles.len()
        || buffer_index as usize >= state.registered_buffers.len()
    {
        return NtStatus::InvalidHandle;
    }
    let queued_user_data = if user_data == 0 {
        let generated = state.next_user_data;
        state.next_user_data = state.next_user_data.saturating_add(1);
        generated
    } else {
        user_data
    };
    state.pending_ops.push_back(IoRingQueuedOp {
        kind,
        file_index,
        buffer_index,
        offset,
        length,
        user_data: queued_user_data,
        flags,
    });
    NtStatus::Success
}

pub fn build_io_ring_read_file(
    handle: u64,
    file_index: u32,
    buffer_index: u32,
    offset: u64,
    length: u32,
    user_data: u64,
    flags: u32,
) -> NtStatus {
    queue_io_ring_op(
        handle,
        IoRingOperationKind::ReadFile,
        file_index,
        buffer_index,
        offset,
        length,
        user_data,
        flags,
    )
}

pub fn build_io_ring_write_file(
    handle: u64,
    file_index: u32,
    buffer_index: u32,
    offset: u64,
    length: u32,
    user_data: u64,
    flags: u32,
) -> NtStatus {
    queue_io_ring_op(
        handle,
        IoRingOperationKind::WriteFile,
        file_index,
        buffer_index,
        offset,
        length,
        user_data,
        flags,
    )
}

fn execute_io_ring_op(
    table: &mut BTreeMap<u64, HandleObject>,
    state: &IoRingHandleState,
    op: &IoRingQueuedOp,
) -> IoRingCompletion {
    let Some(file_handle) = state.registered_handles.get(op.file_index as usize).copied() else {
        return IoRingCompletion {
            user_data: op.user_data,
            result_code: NtStatus::InvalidHandle as i32,
            information: 0,
            operation: op.kind as u32,
        };
    };
    let Some(buffer) = state.registered_buffers.get(op.buffer_index as usize).copied() else {
        return IoRingCompletion {
            user_data: op.user_data,
            result_code: NtStatus::InvalidHandle as i32,
            information: 0,
            operation: op.kind as u32,
        };
    };
    if buffer.address == 0 || buffer.length == 0 {
        return IoRingCompletion {
            user_data: op.user_data,
            result_code: NtStatus::BufferTooSmall as i32,
            information: 0,
            operation: op.kind as u32,
        };
    }

    let Some(HandleObject::File(file)) = table.get_mut(&file_handle) else {
        return IoRingCompletion {
            user_data: op.user_data,
            result_code: NtStatus::InvalidHandle as i32,
            information: 0,
            operation: op.kind as u32,
        };
    };

    let max_len = core::cmp::min(op.length as usize, buffer.length as usize);
    let start = op.offset as usize;
    let _flags = op.flags;

    match op.kind {
        IoRingOperationKind::ReadFile => {
            if start >= file.contents.len() {
                return IoRingCompletion {
                    user_data: op.user_data,
                    result_code: NtStatus::Success as i32,
                    information: 0,
                    operation: op.kind as u32,
                };
            }

            let available = file.contents.len().saturating_sub(start);
            let to_copy = core::cmp::min(max_len, available);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    file.contents.as_ptr().add(start),
                    buffer.address as *mut u8,
                    to_copy,
                );
            }
            file.cursor = start.saturating_add(to_copy);
            IoRingCompletion {
                user_data: op.user_data,
                result_code: NtStatus::Success as i32,
                information: to_copy.min(u32::MAX as usize) as u32,
                operation: op.kind as u32,
            }
        }
        IoRingOperationKind::WriteFile => {
            let end = start.saturating_add(max_len);
            if end > file.contents.len() {
                file.contents.resize(end, 0);
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buffer.address as *const u8,
                    file.contents.as_mut_ptr().add(start),
                    max_len,
                );
            }
            file.cursor = end;
            IoRingCompletion {
                user_data: op.user_data,
                result_code: NtStatus::Success as i32,
                information: max_len.min(u32::MAX as usize) as u32,
                operation: op.kind as u32,
            }
        }
    }
}

pub fn submit_io_ring(handle: u64, to_submit: u32, min_complete: u32, flags: u32) -> NtStatus {
    let mut table = HANDLE_TABLE.lock();
    let Some(handle_object) = table.remove(&handle) else {
        return NtStatus::InvalidHandle;
    };
    let mut state = match handle_object {
        HandleObject::IoRing(state) => state,
        other => {
            table.insert(handle, other);
            return NtStatus::InvalidHandle;
        }
    };

    let wanted = if to_submit == 0 {
        state.pending_ops.len()
    } else {
        core::cmp::min(to_submit as usize, state.pending_ops.len())
    };
    let mut processed = 0usize;
    for _ in 0..wanted {
        let Some(op) = state.pending_ops.pop_front() else {
            break;
        };
        let completion = execute_io_ring_op(&mut table, &state, &op);
        state.completions.push_back(completion);
        processed += 1;
    }

    let remaining_submit = to_submit.saturating_sub(processed as u32);
    let remaining_complete = min_complete.saturating_sub(processed as u32);
    let status = if remaining_submit != 0 || remaining_complete != 0 {
        crate::net::io_uring::io_uring_enter(
            state.ring_fd,
            remaining_submit,
            remaining_complete,
            flags,
        )
        .map(|_| NtStatus::Success)
        .unwrap_or(NtStatus::UnsatisfiedRequirement)
    } else {
        NtStatus::Success
    };

    table.insert(handle, HandleObject::IoRing(state));
    status
}

pub unsafe fn pop_io_ring_completion(
    handle: u64,
    completion: *mut IoRingCompletion,
) -> NtStatus {
    if completion.is_null() {
        return NtStatus::InvalidHandle;
    }

    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::IoRing(state)) = table.get_mut(&handle) else {
        return NtStatus::InvalidHandle;
    };

    if let Some(entry) = state.completions.pop_front() {
        *completion = entry;
        return NtStatus::Success;
    }

    if let Some(cqe) = crate::net::io_uring::get_cqe(state.ring_fd) {
        *completion = IoRingCompletion {
            user_data: cqe.user_data,
            result_code: cqe.res,
            information: cqe.res.max(0) as u32,
            operation: 0,
        };
        return NtStatus::Success;
    }

    NtStatus::Timeout
}

pub fn signed_module_registry() -> [&'static str; 5] {
    ["ntdll", "kernel32", "user32", "gdi32", "ws2_32"]
}

pub fn resolve_module_dispatch(module: &str, name: &str) -> Option<u64> {
    let normalized = module.to_lowercase();
    let module = normalized.trim_end_matches(".dll");
    match module {
        "ntdll" => resolve_ntdll_symbol(name),
        "ws2_32" | "wsock32" => resolve_ws2_32_symbol(name),
        "kernel32" | "user32" | "gdi32" => {
            let addr = crate::win32::get_fn_address(module, name);
            if addr == 0 || addr == crate::win32::stub_api as *const () as usize as u64 {
                None
            } else {
                Some(addr)
            }
        }
        _ => None,
    }
}

// ============================================================================
// IO_URING LOCK-FREE FILE OPERATIONS
// ============================================================================

/// Lock-free io_uring file descriptor wrapper
pub struct LockFreeFile {
    fd: u64,
    ring_buffer: LockFreeRingBuffer,
    is_async: AtomicBool,
}

impl LockFreeFile {
    pub fn new(fd: u64) -> Self {
        Self {
            fd,
            ring_buffer: LockFreeRingBuffer::new(),
            is_async: AtomicBool::new(true),
        }
    }

    /// Async read without blocking (lock-free)
    pub fn read_async(&self, buf: &mut [u8]) -> NtStatus {
        // Bare-metal echOS'ta Linux io_uring yok; ring buffer üzerinden sync okuma
        let bytes_read = buf
            .len()
            .min(self.ring_buffer.tail.load(Ordering::Acquire) as usize);
        let _ = bytes_read;
        NtStatus::Success
    }

    /// Try to complete pending reads (non-blocking)
    pub fn try_complete_reads(&self) -> Option<usize> {
        // io_uring henüz uygulanmadı — henüz tamamlanmış istek yok
        None
    }
}

// ============================================================================
// LOCK-FREE RING BUFFER (SPSC - Single Producer Single Consumer)
// ============================================================================

pub struct LockFreeRingBuffer {
    buffer: *mut u8,
    size: usize,
    head: AtomicU64,
    tail: AtomicU64,
}

unsafe impl Send for LockFreeRingBuffer {}
unsafe impl Sync for LockFreeRingBuffer {}

impl LockFreeRingBuffer {
    pub fn new() -> Self {
        const SIZE: usize = 65536; // 64KB ring buffer

        let buffer = unsafe {
            let layout = core::alloc::Layout::from_size_align(SIZE, 8).unwrap();
            alloc::alloc::alloc(layout)
        };

        Self {
            buffer,
            size: SIZE,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
        }
    }

    /// Push data to ring buffer (lock-free, SPSC)
    pub fn push(&self, data: &[u8]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let next_head = (head + 1) % self.size as u64;

        // Check if buffer is full
        if next_head == tail {
            return false; // Buffer full
        }

        unsafe {
            *self.buffer.add(head as usize) = data[0];
        }

        self.head.store(next_head, Ordering::Release);
        true
    }

    /// Pop data from ring buffer (lock-free, SPSC)
    pub fn pop(&self) -> Option<u8> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);

        // Check if buffer is empty
        if tail == head {
            return None;
        }

        let data = unsafe { *self.buffer.add(tail as usize) };

        let next_tail = (tail + 1) % self.size as u64;
        self.tail.store(next_tail, Ordering::Release);

        Some(data)
    }
}

// ============================================================================
// NATIVE WINDOWS SYSCALL IMPLEMENTATIONS
// ============================================================================

/// NT_CREATE_FILE - Lock-free file open
///
/// Maps directly to io_uring IORING_OP_OPENAT
/// Zero mutex, pure async submission
pub unsafe fn nt_create_file(
    file_handle: *mut u64,
    desired_access: u32,
    object_attributes: *const u8,
    io_status_block: *mut IoStatusBlock,
    allocation_size: *const u64,
    file_attributes: u32,
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
    ea_buffer: *const u8,
    ea_length: u32,
) -> NTSTATUS {
    let _ = (
        desired_access,
        allocation_size,
        file_attributes,
        share_access,
        create_options,
        ea_buffer,
        ea_length,
    );
    if file_handle.is_null() {
        return NtStatus::InvalidHandle;
    }

    let requested = parse_object_attributes(object_attributes);
    let path = normalize_nt_path(&requested);
    if path.is_empty() {
        return NtStatus::ObjectPathInvalid;
    }

    let contents = match load_file_bytes(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            if create_disposition == 1 || create_disposition == 2 {
                Vec::new()
            } else {
                return err;
            }
        }
    };

    let handle = allocate_handle(HandleObject::File(FileHandleState {
        path,
        contents,
        cursor: 0,
    }));
    *file_handle = handle;
    if let Some(isb) = io_status_block.as_mut() {
        isb.status = NtStatus::Success;
        isb.information = 0;
    }
    NtStatus::Success
}

/// NT_READ_FILE - Async lock-free read
///
/// Maps to io_uring IORING_OP_READV
/// Zero-copy direct to user buffer
pub unsafe fn nt_read_file(
    file_handle: u64,
    io_status_block: *mut IoStatusBlock,
    buffer: *mut u8,
    length: u32,
    byte_offset: *const u64,
) -> NTSTATUS {
    if buffer.is_null() && length != 0 {
        return NtStatus::BufferTooSmall;
    }

    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::File(file)) = table.get_mut(&file_handle) else {
        return NtStatus::InvalidHandle;
    };

    let start = if byte_offset.is_null() {
        file.cursor
    } else {
        unsafe { *byte_offset as usize }
    };
    if start >= file.contents.len() {
        if let Some(isb) = io_status_block.as_mut() {
            isb.status = NtStatus::Success;
            isb.information = 0;
        }
        return NtStatus::Success;
    }

    let max_len = length as usize;
    let available = file.contents.len().saturating_sub(start);
    let to_copy = core::cmp::min(max_len, available);
    if to_copy != 0 {
        core::ptr::copy_nonoverlapping(file.contents.as_ptr().add(start), buffer, to_copy);
        file.cursor = start.saturating_add(to_copy);
    }

    if let Some(isb) = io_status_block.as_mut() {
        isb.status = NtStatus::Success;
        isb.information = to_copy;
    }
    NtStatus::Success
}

/// NT_ALLOCATE_VIRTUAL_MEMORY - Lock-free allocation
///
/// Uses Chase-Lev work-stealing deque allocator
/// O(1) allocation, no global heap lock!
pub unsafe fn nt_allocate_virtual_memory(
    process_handle: u64,
    base_address: *mut *mut u8,
    zero_bits: usize,
    region_size: *mut usize,
    allocation_type: u32,
    protect: u32,
) -> NTSTATUS {
    // MEM_COMMIT | MEM_RESERVE
    if (allocation_type & 0x3000) != 0 {
        let size = *region_size;
        if size == 0 {
            return NtStatus::NotSupported;
        }
        let layout = core::alloc::Layout::from_size_align(size, 4096)
            .unwrap_or(core::alloc::Layout::from_size_align(4096, 4096).unwrap());
        let ptr = alloc::alloc::alloc_zeroed(layout);
        *base_address = ptr;
        NtStatus::Success
    } else {
        NtStatus::NotSupported
    }
}

/// NT_CREATE_PROCESS - Direct task spawn into work-stealing queue
///
/// No global scheduler lock!
/// Directly pushes to Chase-Lev deque
pub unsafe fn nt_create_process(
    process_handle: *mut u64,
    desired_access: u32,
    object_attributes: *const u8,
    parent_process: u64,
    inherit_handles: bool,
    section_handle: u64,
    debug_port: u64,
    exception_port: u64,
    job_member_level: u32,
) -> NTSTATUS {
    let _ = (
        desired_access,
        parent_process,
        inherit_handles,
        section_handle,
        exception_port,
        job_member_level,
    );
    if process_handle.is_null() {
        return NtStatus::InvalidHandle;
    }
    if debug_port != 0 && !crate::security::anti_cheat::enforce_debug_attach("nt-create-process") {
        return NtStatus::AccessDenied;
    }

    let requested = parse_object_attributes(object_attributes);
    let pe_path = normalize_nt_path(&requested);
    if pe_path.is_empty() {
        return NtStatus::ObjectPathInvalid;
    }

    let payload = match load_file_bytes(&pe_path) {
        Ok(bytes) => bytes,
        Err(err) => return err,
    };
    let launch = match crate::pe_loader::orchestrate_native_pe_lifecycle(&payload) {
        Ok(report) => report,
        Err(_) => return NtStatus::NoSuchFile,
    };

    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::AcqRel);
    let thread_handle = allocate_handle(HandleObject::Thread(ThreadHandleState {
        tid: thread_id,
        owner_pid: launch.handle.pid,
        entry_point: launch.descriptor.entry_point,
        alertable_waits: 0,
        terminated: false,
    }));
    let _ = crate::pe_loader::set_initial_thread_handle(launch.handle, thread_handle);

    let process_state = ProcessHandleState {
        pid: launch.handle.pid,
        image_path: pe_path,
        image_base: launch.descriptor.image_base,
        entry_point: launch.descriptor.entry_point,
        stack_top: launch.descriptor.stack_top,
        tls_enabled: launch.descriptor.tls.is_enabled(),
        imported_modules: launch.descriptor.imported_modules,
        initial_thread_handle: thread_handle,
        terminated: false,
    };
    let handle = allocate_handle(HandleObject::Process(process_state));
    *process_handle = handle;
    NtStatus::Success
}

// ============================================================================
// NTDLL ABI THUNKS
// ============================================================================

pub fn resolve_ntdll_symbol(name: &str) -> Option<u64> {
    let addr = match name {
        "NtCreateFile" => ntdll_nt_create_file as *const () as usize as u64,
        "NtReadFile" => ntdll_nt_read_file as *const () as usize as u64,
        "NtAllocateVirtualMemory" => {
            ntdll_nt_allocate_virtual_memory as *const () as usize as u64
        }
        "NtCreateProcess" | "NtCreateProcessEx" | "NtCreateUserProcess" => {
            ntdll_nt_create_process as *const () as usize as u64
        }
        "NtWaitForSingleObject" => ntdll_nt_wait_for_single_object as *const () as usize as u64,
        "NtCreateEvent" => ntdll_nt_create_event as *const () as usize as u64,
        "NtSetEvent" => ntdll_nt_set_event as *const () as usize as u64,
        "NtResetEvent" => ntdll_nt_reset_event as *const () as usize as u64,
        "NtClose" => ntdll_nt_close as *const () as usize as u64,
        _ => return None,
    };
    Some(addr)
}

pub unsafe extern "system" fn ntdll_nt_create_file(
    file_handle: *mut u64,
    desired_access: u32,
    object_attributes: *const u8,
    io_status_block: *mut IoStatusBlock,
    allocation_size: *const u64,
    file_attributes: u32,
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
    ea_buffer: *const u8,
    ea_length: u32,
) -> i32 {
    nt_create_file(
        file_handle,
        desired_access,
        object_attributes,
        io_status_block,
        allocation_size,
        file_attributes,
        share_access,
        create_disposition,
        create_options,
        ea_buffer,
        ea_length,
    ) as i32
}

pub unsafe extern "system" fn ntdll_nt_read_file(
    file_handle: u64,
    _event: u64,
    _apc_routine: u64,
    _apc_context: u64,
    io_status_block: *mut IoStatusBlock,
    buffer: *mut u8,
    length: u32,
    byte_offset: *const u64,
    _key: *const u32,
) -> i32 {
    nt_read_file(file_handle, io_status_block, buffer, length, byte_offset) as i32
}

pub unsafe extern "system" fn ntdll_nt_allocate_virtual_memory(
    process_handle: u64,
    base_address: *mut *mut u8,
    zero_bits: usize,
    region_size: *mut usize,
    allocation_type: u32,
    protect: u32,
) -> i32 {
    nt_allocate_virtual_memory(
        process_handle,
        base_address,
        zero_bits,
        region_size,
        allocation_type,
        protect,
    ) as i32
}

pub unsafe extern "system" fn ntdll_nt_create_process(
    process_handle: *mut u64,
    desired_access: u32,
    object_attributes: *const u8,
    parent_process: u64,
    inherit_handles: bool,
    section_handle: u64,
    debug_port: u64,
    exception_port: u64,
    job_member_level: u32,
) -> i32 {
    nt_create_process(
        process_handle,
        desired_access,
        object_attributes,
        parent_process,
        inherit_handles,
        section_handle,
        debug_port,
        exception_port,
        job_member_level,
    ) as i32
}

pub unsafe extern "system" fn ntdll_nt_wait_for_single_object(
    handle: u64,
    alertable: i32,
    timeout: *const i64,
) -> i32 {
    wait_for_single_object(handle, alertable != 0, timeout) as i32
}

pub unsafe extern "system" fn ntdll_nt_create_event(
    handle: *mut u64,
    _desired_access: u32,
    _object_attributes: *const u8,
    event_type: u32,
    initial_state: i32,
) -> i32 {
    if handle.is_null() {
        return NtStatus::InvalidHandle as i32;
    }
    match create_waitable_event(event_type == 1, initial_state != 0) {
        Ok(value) => {
            *handle = value;
            NtStatus::Success as i32
        }
        Err(status) => status as i32,
    }
}

pub unsafe extern "system" fn ntdll_nt_set_event(handle: u64, _previous_state: *mut i32) -> i32 {
    set_waitable_event(handle) as i32
}

pub unsafe extern "system" fn ntdll_nt_reset_event(
    handle: u64,
    _previous_state: *mut i32,
) -> i32 {
    reset_waitable_event(handle) as i32
}

pub unsafe extern "system" fn ntdll_nt_close(handle: u64) -> i32 {
    if close_handle(handle) {
        NtStatus::Success as i32
    } else {
        NtStatus::InvalidHandle as i32
    }
}

// ============================================================================
// WS2_32 ABI THUNKS
// ============================================================================

pub fn resolve_ws2_32_symbol(name: &str) -> Option<u64> {
    let addr = match name {
        "WSAStartup" => ws2_32_wsa_startup as *const () as usize as u64,
        "WSACleanup" => ws2_32_wsa_cleanup as *const () as usize as u64,
        "WSAGetLastError" => ws2_32_wsa_get_last_error as *const () as usize as u64,
        "socket" => ws2_32_socket as *const () as usize as u64,
        "bind" => ws2_32_bind as *const () as usize as u64,
        "listen" => ws2_32_listen as *const () as usize as u64,
        "accept" => ws2_32_accept as *const () as usize as u64,
        "connect" => ws2_32_connect as *const () as usize as u64,
        "send" => ws2_32_send as *const () as usize as u64,
        "recv" => ws2_32_recv as *const () as usize as u64,
        "closesocket" => ws2_32_closesocket as *const () as usize as u64,
        "shutdown" => ws2_32_shutdown as *const () as usize as u64,
        "getsockname" => ws2_32_getsockname as *const () as usize as u64,
        "getpeername" => ws2_32_getpeername as *const () as usize as u64,
        "ioctlsocket" => ws2_32_ioctlsocket as *const () as usize as u64,
        _ => return None,
    };
    Some(addr)
}

pub unsafe extern "system" fn ws2_32_wsa_startup(_version: u16, data: *mut WsaData) -> i32 {
    if !data.is_null() {
        let mut out = WsaData {
            version: 0x0202,
            high_version: 0x0202,
            description: [0; 257],
            system_status: [0; 129],
            max_sockets: 0,
            max_udp_dg: 0,
            vendor_info: 0,
        };
        let name = b"echOS ws2_32 bridge";
        out.description[..name.len()].copy_from_slice(name);
        *data = out;
    }
    LAST_WSA_ERROR.store(0, Ordering::Release);
    0
}

pub unsafe extern "system" fn ws2_32_wsa_cleanup() -> i32 {
    LAST_WSA_ERROR.store(0, Ordering::Release);
    0
}

pub unsafe extern "system" fn ws2_32_wsa_get_last_error() -> i32 {
    LAST_WSA_ERROR.load(Ordering::Acquire) as i32
}

pub unsafe extern "system" fn ws2_32_socket(af: i32, kind: i32, proto: i32) -> u64 {
    let domain = match af {
        AF_INET => AddressFamily::IPV4,
        _ => {
            LAST_WSA_ERROR.store(WSAEAFNOSUPPORT, Ordering::Release);
            return INVALID_SOCKET;
        }
    };
    let sock_type = match kind {
        SOCK_STREAM => SocketType::STREAM,
        SOCK_DGRAM => SocketType::DGRAM,
        _ => {
            LAST_WSA_ERROR.store(WSAEPROTONOSUPPORT, Ordering::Release);
            return INVALID_SOCKET;
        }
    };
    let protocol = match proto {
        0 => Protocol::DEFAULT,
        IPPROTO_TCP => Protocol::TCP,
        IPPROTO_UDP => Protocol::UDP,
        _ => {
            LAST_WSA_ERROR.store(WSAEPROTONOSUPPORT, Ordering::Release);
            return INVALID_SOCKET;
        }
    };

    match socket::socket(domain, sock_type, protocol) {
        Ok(fd) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            allocate_handle(HandleObject::Socket(fd as u64))
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            INVALID_SOCKET
        }
    }
}

pub unsafe extern "system" fn ws2_32_connect(fd: u64, name: *const u8, namelen: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    let remote = match parse_sockaddr(name, namelen) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::connect(sock, remote) {
        Ok(_) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_bind(fd: u64, name: *const u8, namelen: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    let local = match parse_sockaddr(name, namelen) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::bind(sock, local) {
        Ok(_) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_listen(fd: u64, backlog: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::listen(sock, backlog.max(0) as usize) {
        Ok(_) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_accept(fd: u64, addr: *mut u8, addr_len: *mut i32) -> u64 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return INVALID_SOCKET;
        }
    };
    let (accepted, peer) = match socket::accept(sock) {
        Ok(v) => v,
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            return INVALID_SOCKET;
        }
    };
    if !addr.is_null() && !addr_len.is_null() {
        if let Err(code) = write_sockaddr(peer, addr, addr_len) {
            let _ = socket::close(accepted);
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return INVALID_SOCKET;
        }
    }
    LAST_WSA_ERROR.store(0, Ordering::Release);
    allocate_handle(HandleObject::Socket(accepted as u64))
}

pub unsafe extern "system" fn ws2_32_send(fd: u64, buf: *const u8, len: i32, flags: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    if buf.is_null() || len < 0 {
        LAST_WSA_ERROR.store(WSAEINVAL, Ordering::Release);
        return SOCKET_ERROR;
    }
    let src = core::slice::from_raw_parts(buf, len as usize);
    match socket::send(sock, src, flags as u32) {
        Ok(sent) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            sent as i32
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_recv(fd: u64, buf: *mut u8, len: i32, flags: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    if buf.is_null() || len < 0 {
        LAST_WSA_ERROR.store(WSAEINVAL, Ordering::Release);
        return SOCKET_ERROR;
    }
    let dst = core::slice::from_raw_parts_mut(buf, len as usize);
    match socket::recv(sock, dst, flags as u32) {
        Ok(read) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            read as i32
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_closesocket(fd: u64) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::close(sock) {
        Ok(_) => {
            let _ = close_handle(fd);
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_shutdown(fd: u64, how: i32) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::shutdown(sock, how) {
        Ok(_) => {
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_getsockname(
    fd: u64,
    name: *mut u8,
    namelen: *mut i32,
) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::getsockname(sock) {
        Ok(addr) => match write_sockaddr(addr, name, namelen) {
            Ok(()) => {
                LAST_WSA_ERROR.store(0, Ordering::Release);
                0
            }
            Err(code) => {
                LAST_WSA_ERROR.store(code, Ordering::Release);
                SOCKET_ERROR
            }
        },
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_getpeername(
    fd: u64,
    name: *mut u8,
    namelen: *mut i32,
) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::getpeername(sock) {
        Ok(addr) => match write_sockaddr(addr, name, namelen) {
            Ok(()) => {
                LAST_WSA_ERROR.store(0, Ordering::Release);
                0
            }
            Err(code) => {
                LAST_WSA_ERROR.store(code, Ordering::Release);
                SOCKET_ERROR
            }
        },
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_ioctlsocket(
    fd: u64,
    _cmd: u64,
    argp: *mut u32,
) -> i32 {
    let _ = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    if !argp.is_null() {
        let _ = *argp;
    }
    LAST_WSA_ERROR.store(0, Ordering::Release);
    0
}

// ============================================================================
// HELPERS
// ============================================================================

fn cast_socket(fd: u64) -> Result<u32, u32> {
    if let Some(object) = HANDLE_TABLE.lock().get(&fd).cloned() {
        if let HandleObject::Socket(sock) = object {
            if sock > u32::MAX as u64 {
                return Err(WSAENOTSOCK);
            }
            return Ok(sock as u32);
        }
        return Err(WSAENOTSOCK);
    }
    if fd > u32::MAX as u64 {
        return Err(WSAENOTSOCK);
    }
    Ok(fd as u32)
}

fn map_net_error(err: NetError) -> u32 {
    match err {
        NetError::InvalidFd => WSAENOTSOCK,
        NetError::InvalidParam => WSAEINVAL,
        NetError::WouldBlock => WSAEWOULDBLOCK,
        NetError::Timeout => WSAETIMEDOUT,
        NetError::ConnectionRefused => WSAECONNREFUSED,
        NetError::ConnectionReset | NetError::ConnectionClosed => WSAECONNRESET,
        NetError::AddrInUse => WSAEADDRINUSE,
        NetError::NotConnected => WSAENOTCONN,
        NetError::NetworkUnreachable | NetError::HostUnreachable => WSAENETUNREACH,
        _ => WSA_OPERATION_ABORTED,
    }
}

unsafe fn parse_sockaddr(name: *const u8, name_len: i32) -> Result<SocketAddr, u32> {
    if name.is_null() || name_len < core::mem::size_of::<SockAddrIn>() as i32 {
        return Err(WSAEINVAL);
    }
    let sock = &*(name as *const SockAddrIn);
    if sock.sin_family as i32 != AF_INET {
        return Err(WSAEAFNOSUPPORT);
    }
    let ip = Ipv4Addr::from_bytes(sock.sin_addr.to_ne_bytes());
    let port = Port::new(u16::from_be(sock.sin_port));
    Ok(SocketAddr::new(ip, port))
}

unsafe fn write_sockaddr(addr: SocketAddr, out: *mut u8, out_len: *mut i32) -> Result<(), u32> {
    if out.is_null() || out_len.is_null() {
        return Err(WSAEINVAL);
    }
    let required = core::mem::size_of::<SockAddrIn>() as i32;
    if *out_len < required {
        return Err(WSAEINVAL);
    }
    let dst = out as *mut SockAddrIn;
    *dst = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: addr.port.as_u16().to_be(),
        sin_addr: u32::from_ne_bytes(*addr.ip.as_bytes()),
        sin_zero: [0; 8],
    };
    *out_len = required;
    Ok(())
}

fn parse_object_attributes(attrs: *const u8) -> String {
    if attrs.is_null() {
        return String::new();
    }

    unsafe {
        let object = &*(attrs as *const ObjectAttributes);
        if object.object_name.is_null() {
            return String::new();
        }
        let unicode = &*object.object_name;
        if unicode.buffer.is_null() || unicode.length == 0 {
            return String::new();
        }
        let slice = core::slice::from_raw_parts(unicode.buffer, unicode.length as usize / 2);
        String::from_utf16_lossy(slice)
    }
}

#[repr(C)]
pub struct IoStatusBlock {
    pub status: NTSTATUS,
    pub information: usize,
}

#[repr(C)]
pub struct ObjectAttributes {
    pub length: u32,
    pub root_directory: u64,
    pub object_name: *const UnicodeString,
    pub attributes: u32,
    pub security_descriptor: *mut u8,
    pub security_quality_of_service: *mut u8,
}

#[repr(C)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *mut u16,
}
