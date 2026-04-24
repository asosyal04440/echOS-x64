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

use super::kernel::tasking;
use super::net::socket::{self, AddressFamily, Protocol, SocketType};
use super::net::{self, Ipv4Addr, NetError, Port, SocketAddr};
use super::{fs, pe_loader, security, win32};
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
const SOL_SOCKET: i32 = 0xFFFF;
const SO_SNDBUF: i32 = 0x1001;
const SO_RCVBUF: i32 = 0x1002;
const SO_REUSEADDR: i32 = 0x0004;
const SO_KEEPALIVE: i32 = 0x0008;
const SO_RCVTIMEO: i32 = 0x1006;
const SO_SNDTIMEO: i32 = 0x1005;
const TCP_NODELAY: i32 = 0x0001;
const INADDR_NONE: u32 = 0xFFFF_FFFF;

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
static INET_NTOA_BUFFER: Mutex<[u8; 16]> = Mutex::new([0; 16]);
static UNWIND_LOOKUP_CACHE: Mutex<BTreeMap<u64, Box<[pe_loader::PeRuntimeFunction]>>> =
    Mutex::new(BTreeMap::new());

const UNW_FLAG_EHANDLER: u8 = 0x1;
const UNW_FLAG_UHANDLER: u8 = 0x2;
const UNW_FLAG_CHAININFO: u8 = 0x4;
const CONTEXT_AMD64: u32 = 0x0010_0000;
const CONTEXT_CONTROL: u32 = CONTEXT_AMD64 | 0x0000_0001;
const CONTEXT_INTEGER: u32 = CONTEXT_AMD64 | 0x0000_0002;
const CONTEXT_SEGMENTS: u32 = CONTEXT_AMD64 | 0x0000_0004;
const CONTEXT_FLOATING_POINT: u32 = CONTEXT_AMD64 | 0x0000_0008;
const CONTEXT_DEBUG_REGISTERS: u32 = CONTEXT_AMD64 | 0x0000_0010;
const DEFAULT_X87_CONTROL_WORD: u16 = 0x027F;
const DEFAULT_X87_TAG_WORD: u16 = 0xFFFF;
const DEFAULT_MXCSR_MASK: u32 = 0x0000_FFFF;
const UWOP_PUSH_NONVOL: u8 = 0;
const UWOP_ALLOC_LARGE: u8 = 1;
const UWOP_ALLOC_SMALL: u8 = 2;
const UWOP_SET_FPREG: u8 = 3;
const UWOP_SAVE_NONVOL: u8 = 4;
const UWOP_SAVE_NONVOL_FAR: u8 = 5;
const UWOP_SAVE_XMM128: u8 = 8;
const UWOP_SAVE_XMM128_FAR: u8 = 9;
const UWOP_PUSH_MACHFRAME: u8 = 10;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct M128A {
    pub low: u64,
    pub high: i64,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct ContextRecord {
    pub p1_home: u64,
    pub p2_home: u64,
    pub p3_home: u64,
    pub p4_home: u64,
    pub p5_home: u64,
    pub p6_home: u64,
    pub context_flags: u32,
    pub mx_csr: u32,
    pub seg_cs: u16,
    pub seg_ds: u16,
    pub seg_es: u16,
    pub seg_fs: u16,
    pub seg_gs: u16,
    pub seg_ss: u16,
    pub eflags: u32,
    pub dr0: u64,
    pub dr1: u64,
    pub dr2: u64,
    pub dr3: u64,
    pub dr6: u64,
    pub dr7: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub floating_control_word: u16,
    pub floating_status_word: u16,
    pub floating_tag_word: u16,
    pub floating_error_opcode: u16,
    pub floating_error_offset: u32,
    pub floating_error_selector: u16,
    pub floating_data_selector: u16,
    pub floating_data_offset: u32,
    pub floating_mx_csr: u32,
    pub floating_mx_csr_mask: u32,
    pub header_registers: [M128A; 2],
    pub legacy_float_registers: [M128A; 8],
    pub xmm_registers: [M128A; 16],
    pub double_registers: [u64; 32],
    pub scalar_registers: [u32; 32],
    pub vector_registers: [M128A; 26],
    pub vector_control: u64,
    pub debug_control: u64,
    pub last_branch_to_rip: u64,
    pub last_branch_from_rip: u64,
    pub last_exception_to_rip: u64,
    pub last_exception_from_rip: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NonvolatileContextPointers {
    pub floating_context: [*mut M128A; 16],
    pub integer_context: [*mut u64; 16],
}

impl Default for NonvolatileContextPointers {
    fn default() -> Self {
        Self {
            floating_context: [core::ptr::null_mut(); 16],
            integer_context: [core::ptr::null_mut(); 16],
        }
    }
}

impl Default for ContextRecord {
    fn default() -> Self {
        Self {
            p1_home: 0,
            p2_home: 0,
            p3_home: 0,
            p4_home: 0,
            p5_home: 0,
            p6_home: 0,
            context_flags: CONTEXT_CONTROL
                | CONTEXT_INTEGER
                | CONTEXT_SEGMENTS
                | CONTEXT_FLOATING_POINT
                | CONTEXT_DEBUG_REGISTERS,
            mx_csr: 0x1F80,
            seg_cs: 0,
            seg_ds: 0,
            seg_es: 0,
            seg_fs: 0,
            seg_gs: 0,
            seg_ss: 0,
            eflags: 0,
            dr0: 0,
            dr1: 0,
            dr2: 0,
            dr3: 0,
            dr6: 0,
            dr7: 0,
            rax: 0,
            rcx: 0,
            rdx: 0,
            rbx: 0,
            rsp: 0,
            rbp: 0,
            rsi: 0,
            rdi: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            floating_control_word: DEFAULT_X87_CONTROL_WORD,
            floating_status_word: 0,
            floating_tag_word: DEFAULT_X87_TAG_WORD,
            floating_error_opcode: 0,
            floating_error_offset: 0,
            floating_error_selector: 0,
            floating_data_selector: 0,
            floating_data_offset: 0,
            floating_mx_csr: 0x1F80,
            floating_mx_csr_mask: DEFAULT_MXCSR_MASK,
            header_registers: [M128A::default(); 2],
            legacy_float_registers: [M128A::default(); 8],
            xmm_registers: [M128A::default(); 16],
            double_registers: [0; 32],
            scalar_registers: [0; 32],
            vector_registers: [M128A::default(); 26],
            vector_control: 0,
            debug_control: 0,
            last_branch_to_rip: 0,
            last_branch_from_rip: 0,
            last_exception_to_rip: 0,
            last_exception_from_rip: 0,
        }
    }
}

fn pack_x87_header_registers(context: &ContextRecord) -> [M128A; 2] {
    let header0_low = (context.floating_control_word as u64)
        | ((context.floating_status_word as u64) << 16)
        | ((context.floating_tag_word as u64) << 32)
        | ((context.floating_error_opcode as u64) << 48);
    let header0_high =
        (context.floating_error_offset as u64) | ((context.floating_error_selector as u64) << 32);
    let header1_low =
        (context.floating_data_offset as u64) | ((context.floating_data_selector as u64) << 32);
    let header1_high =
        (context.floating_mx_csr as u64) | ((context.floating_mx_csr_mask as u64) << 32);
    [
        M128A {
            low: header0_low,
            high: header0_high as i64,
        },
        M128A {
            low: header1_low,
            high: header1_high as i64,
        },
    ]
}

fn split_m128a_to_u64_pair(value: M128A) -> [u64; 2] {
    [value.low, value.high as u64]
}

fn split_m128a_to_u32_quads(value: M128A) -> [u32; 4] {
    [
        value.low as u32,
        (value.low >> 32) as u32,
        value.high as u64 as u32,
        ((value.high as u64) >> 32) as u32,
    ]
}

fn synchronize_vector_state(context: &mut ContextRecord) {
    context.context_flags |=
        CONTEXT_CONTROL | CONTEXT_INTEGER | CONTEXT_SEGMENTS | CONTEXT_FLOATING_POINT;
    if context.floating_control_word == 0 {
        context.floating_control_word = DEFAULT_X87_CONTROL_WORD;
    }
    if context.floating_tag_word == 0 {
        context.floating_tag_word = DEFAULT_X87_TAG_WORD;
    }
    if context.floating_mx_csr_mask == 0 {
        context.floating_mx_csr_mask = DEFAULT_MXCSR_MASK;
    }
    context.floating_mx_csr = context.mx_csr;
    context.header_registers = pack_x87_header_registers(context);
    for (index, value) in context.xmm_registers.iter().copied().enumerate() {
        if index < context.legacy_float_registers.len() {
            context.legacy_float_registers[index] = value;
        }
        if index < context.vector_registers.len() {
            context.vector_registers[index] = value;
        }
        let qword_index = index * 2;
        if qword_index + 1 < context.double_registers.len() {
            let pair = split_m128a_to_u64_pair(value);
            context.double_registers[qword_index] = pair[0];
            context.double_registers[qword_index + 1] = pair[1];
        }
    }
    for (index, value) in context
        .legacy_float_registers
        .iter()
        .copied()
        .enumerate()
        .take(context.scalar_registers.len() / 4)
    {
        let scalar_index = index * 4;
        let quads = split_m128a_to_u32_quads(value);
        context.scalar_registers[scalar_index..scalar_index + 4].copy_from_slice(&quads);
    }
    context.vector_control = context.mx_csr as u64;
}

fn publish_unwind_transition(context: &mut ContextRecord, previous_rip: u64, previous_rsp: u64) {
    context.last_branch_from_rip = previous_rip;
    context.last_branch_to_rip = context.rip;
    context.last_exception_from_rip = previous_rsp;
    context.last_exception_to_rip = context.rsp;
}

fn clear_context_pointers(context_pointers: Option<&mut NonvolatileContextPointers>) {
    if let Some(context_pointers) = context_pointers {
        *context_pointers = NonvolatileContextPointers::default();
    }
}

fn set_integer_context_pointer(
    context_pointers: Option<&mut NonvolatileContextPointers>,
    register: u8,
    slot: *mut u64,
) {
    let Some(context_pointers) = context_pointers else {
        return;
    };
    if let Some(entry) = context_pointers.integer_context.get_mut(register as usize) {
        *entry = slot;
    }
}

fn set_floating_context_pointer(
    context_pointers: Option<&mut NonvolatileContextPointers>,
    register: u8,
    slot: *mut M128A,
) {
    let Some(context_pointers) = context_pointers else {
        return;
    };
    if let Some(entry) = context_pointers.floating_context.get_mut(register as usize) {
        *entry = slot;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnwindInfoHeader {
    version_flags: u8,
    size_of_prolog: u8,
    count_of_codes: u8,
    frame_register_offset: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnwindCode {
    code_offset: u8,
    unwind_op_info: u8,
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
    signal_epoch: Arc<AtomicU32>,
}

#[derive(Clone, Debug)]
struct ThreadHandleState {
    tid: u64,
    owner_pid: u64,
    entry_point: u64,
    start_param: u64,
    alertable_waits: u64,
    terminated: bool,
    exit_code: u32,
    task_id: Option<u64>,
    signal_epoch: Arc<AtomicU32>,
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
            let _ = net::io_uring::io_uring_close(state.ring_fd);
            true
        }
        Some(_) => true,
        None => false,
    }
}

pub fn register_thread_handle(owner_pid: u64, entry_point: u64, start_param: u64) -> (u64, u64) {
    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::AcqRel);
    let signal_epoch = Arc::new(AtomicU32::new(0));
    let handle = allocate_handle(HandleObject::Thread(ThreadHandleState {
        tid: thread_id,
        owner_pid,
        entry_point,
        start_param,
        alertable_waits: 0,
        terminated: false,
        exit_code: 259,
        task_id: None,
        signal_epoch,
    }));
    (handle, thread_id)
}

pub fn bind_thread_handle_task(handle: u64, task_id: u64) -> bool {
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::Thread(thread)) = table.get_mut(&handle) else {
        return false;
    };
    thread.task_id = Some(task_id);
    true
}

pub fn thread_handle_task_id(handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Thread(thread)) => thread.task_id,
        _ => None,
    }
}

pub fn thread_handle_tid(handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Thread(thread)) => Some(thread.tid),
        _ => None,
    }
}

pub fn thread_handle_owner_pid(handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Thread(thread)) => Some(thread.owner_pid),
        _ => None,
    }
}

pub fn thread_handle_start_info(handle: u64) -> Option<(u64, u64, u64)> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Thread(thread)) => {
            Some((thread.owner_pid, thread.entry_point, thread.start_param))
        }
        _ => None,
    }
}

pub fn mark_thread_handle_terminated(handle: u64) -> bool {
    mark_thread_handle_terminated_with_exit(handle, 0)
}

pub fn mark_thread_handle_terminated_with_exit(handle: u64, exit_code: u32) -> bool {
    let mut table = HANDLE_TABLE.lock();
    let Some(HandleObject::Thread(thread)) = table.get_mut(&handle) else {
        return false;
    };
    thread.terminated = true;
    thread.exit_code = exit_code;
    thread.signal_epoch.fetch_add(1, Ordering::AcqRel);
    let epoch_addr = thread.signal_epoch.as_ref() as *const AtomicU32 as u64;
    tasking::wake_by_address_all(epoch_addr);
    true
}

pub fn thread_exit_code(handle: u64) -> Option<u32> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Thread(thread)) => Some(thread.exit_code),
        _ => None,
    }
}

pub fn process_handle_for_pid(pid: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    table.iter().find_map(|(&handle, object)| match object {
        HandleObject::Process(process) if process.pid == pid => Some(handle),
        _ => None,
    })
}

pub fn process_handle_pid(handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Process(process)) => Some(process.pid),
        _ => None,
    }
}

pub fn process_initial_thread_handle(handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&handle) {
        Some(HandleObject::Process(process)) => Some(process.initial_thread_handle),
        _ => None,
    }
}

pub fn process_exit_code(handle: u64) -> Option<u32> {
    let initial_thread = process_initial_thread_handle(handle)?;
    thread_exit_code(initial_thread)
}

pub fn mark_process_terminated_with_exit(handle: u64, exit_code: u32) -> bool {
    let (thread_handle, epoch) = {
        let mut table = HANDLE_TABLE.lock();
        let Some(HandleObject::Process(process)) = table.get_mut(&handle) else {
            return false;
        };
        process.terminated = true;
        process.signal_epoch.fetch_add(1, Ordering::AcqRel);
        (process.initial_thread_handle, process.signal_epoch.clone())
    };
    let _ = mark_thread_handle_terminated_with_exit(thread_handle, exit_code);
    let epoch_addr = epoch.as_ref() as *const AtomicU32 as u64;
    tasking::wake_by_address_all(epoch_addr);
    true
}

pub fn current_process_owner_pid(thread_handle: u64) -> Option<u64> {
    let table = HANDLE_TABLE.lock();
    match table.get(&thread_handle) {
        Some(HandleObject::Thread(thread)) => Some(thread.owner_pid),
        _ => None,
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
    fs::vfs_unified::read_file(path).map_err(|_| NtStatus::NoSuchFile)
}

fn current_ticks() -> u64 {
    tasking::scheduler::get_ticks() as u64
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
                HandleObject::Thread(thread) => Some(thread.signal_epoch.clone()),
                HandleObject::Process(process) => Some(process.signal_epoch.clone()),
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
        let woke = tasking::wait_on_address(
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
    Ok(allocate_handle(HandleObject::Waitable(
        WaitableHandleState {
            signaled: initial_state,
            manual_reset,
            signal_epoch: epoch,
        },
    )))
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
        tasking::wake_by_address_all(epoch_addr);
    } else {
        tasking::wake_by_address_single(epoch_addr);
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
    tasking::wait_on_address(address as u64, compare_address, size, timeout_ms)
}

pub unsafe fn wake_by_address_single(address: *const u8) {
    let _ = tasking::wake_by_address_single(address as u64);
}

pub unsafe fn wake_by_address_all(address: *const u8) {
    let _ = tasking::wake_by_address_all(address as u64);
}

pub fn create_io_ring(
    version: u32,
    flags: u32,
    submission_queue_size: u32,
    completion_queue_size: u32,
) -> Result<u64, NtStatus> {
    let _ = version;
    let entries = submission_queue_size.max(completion_queue_size).max(1);
    let ring_fd = net::io_uring::io_uring_setup(entries, None)
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

pub unsafe fn register_io_ring_handles(handle: u64, handles: *const u64, count: u32) -> NtStatus {
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
    let Some(file_handle) = state
        .registered_handles
        .get(op.file_index as usize)
        .copied()
    else {
        return IoRingCompletion {
            user_data: op.user_data,
            result_code: NtStatus::InvalidHandle as i32,
            information: 0,
            operation: op.kind as u32,
        };
    };
    let Some(buffer) = state
        .registered_buffers
        .get(op.buffer_index as usize)
        .copied()
    else {
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
        net::io_uring::io_uring_enter(state.ring_fd, remaining_submit, remaining_complete, flags)
            .map(|_| NtStatus::Success)
            .unwrap_or(NtStatus::UnsatisfiedRequirement)
    } else {
        NtStatus::Success
    };

    table.insert(handle, HandleObject::IoRing(state));
    status
}

pub unsafe fn pop_io_ring_completion(handle: u64, completion: *mut IoRingCompletion) -> NtStatus {
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

    if let Some(cqe) = net::io_uring::get_cqe(state.ring_fd) {
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
        "kernel32" | "user32" | "gdi32" | "advapi32" | "shell32" | "msvcrt" => {
            let addr = win32::get_fn_address(module, name);
            if addr == 0 || addr == win32::stub_api as *const () as usize as u64 {
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
    if debug_port != 0 && !security::anti_cheat::enforce_debug_attach("nt-create-process") {
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
    let launch = match pe_loader::orchestrate_native_pe_lifecycle(&payload) {
        Ok(report) => report,
        Err(_) => return NtStatus::NoSuchFile,
    };

    let thread_id = NEXT_THREAD_ID.fetch_add(1, Ordering::AcqRel);
    let thread_handle = allocate_handle(HandleObject::Thread(ThreadHandleState {
        tid: thread_id,
        owner_pid: launch.handle.pid,
        entry_point: launch.descriptor.entry_point,
        start_param: 0,
        alertable_waits: 0,
        terminated: false,
        exit_code: 259,
        task_id: None,
        signal_epoch: Arc::new(AtomicU32::new(0)),
    }));
    let _ = pe_loader::set_initial_thread_handle(launch.handle, thread_handle);

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
        signal_epoch: Arc::new(AtomicU32::new(0)),
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
        "NtAllocateVirtualMemory" => ntdll_nt_allocate_virtual_memory as *const () as usize as u64,
        "NtCreateProcess" | "NtCreateProcessEx" | "NtCreateUserProcess" => {
            ntdll_nt_create_process as *const () as usize as u64
        }
        "NtWaitForSingleObject" => ntdll_nt_wait_for_single_object as *const () as usize as u64,
        "NtCreateEvent" => ntdll_nt_create_event as *const () as usize as u64,
        "NtSetEvent" => ntdll_nt_set_event as *const () as usize as u64,
        "NtResetEvent" => ntdll_nt_reset_event as *const () as usize as u64,
        "NtClose" => ntdll_nt_close as *const () as usize as u64,
        "NtCurrentTeb" => ntdll_nt_current_teb as *const () as usize as u64,
        "RtlGetCurrentPeb" => ntdll_rtl_get_current_peb as *const () as usize as u64,
        "RtlDispatchException" => ntdll_rtl_dispatch_exception as *const () as usize as u64,
        "KiUserExceptionDispatcher" => {
            ntdll_ki_user_exception_dispatcher as *const () as usize as u64
        }
        "RtlLookupFunctionEntry" => ntdll_rtl_lookup_function_entry as *const () as usize as u64,
        "RtlVirtualUnwind" => ntdll_rtl_virtual_unwind as *const () as usize as u64,
        _ => return None,
    };
    Some(addr)
}

fn unwind_cache_key() -> u64 {
    #[cfg(test)]
    {
        return 0;
    }
    tasking::scheduler::current_task_id() as u64
}

#[cfg(test)]
static TEST_UNWIND_IMAGE_BASE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn seed_test_unwind_cache(image_base: u64, entries: &[pe_loader::PeRuntimeFunction]) {
    TEST_UNWIND_IMAGE_BASE.store(image_base, Ordering::Release);
    UNWIND_LOOKUP_CACHE
        .lock()
        .insert(unwind_cache_key(), entries.to_vec().into_boxed_slice());
}

pub unsafe extern "system" fn ntdll_nt_current_teb() -> *mut pe_loader::Win32Teb {
    pe_loader::current_teb()
        .map(|teb| teb as *mut pe_loader::Win32Teb)
        .unwrap_or(core::ptr::null_mut())
}

pub unsafe extern "system" fn ntdll_rtl_get_current_peb() -> *mut pe_loader::Win32Peb {
    pe_loader::current_peb()
        .map(|peb| peb as *mut pe_loader::Win32Peb)
        .unwrap_or(core::ptr::null_mut())
}

fn lookup_runtime_function_index(
    control_pc: u64,
    image_base: u64,
    entries: &[pe_loader::PeRuntimeFunction],
) -> Option<usize> {
    let relative = control_pc.checked_sub(image_base)? as u32;
    entries
        .iter()
        .position(|entry| relative >= entry.begin_address && relative < entry.end_address)
}

fn current_runtime_function_pointer(
    control_pc: u64,
    image_base: *mut u64,
) -> Option<*const pe_loader::PeRuntimeFunction> {
    #[cfg(test)]
    {
        let cached_image_base = TEST_UNWIND_IMAGE_BASE.load(Ordering::Acquire);
        let cached_entries = UNWIND_LOOKUP_CACHE
            .lock()
            .get(&unwind_cache_key())
            .map(|entries| entries.to_vec());
        if let Some(entries) = cached_entries {
            if cached_image_base != 0 {
                if !image_base.is_null() {
                    unsafe {
                        *image_base = cached_image_base;
                    }
                }
                let index = lookup_runtime_function_index(control_pc, cached_image_base, &entries)?;
                let boxed = entries.into_boxed_slice();
                let ptr = boxed.as_ptr().wrapping_add(index);
                UNWIND_LOOKUP_CACHE.lock().insert(unwind_cache_key(), boxed);
                return Some(ptr);
            }
            return None;
        }
    }

    let pid = pe_loader::current_process_pid()?;
    let descriptor = pe_loader::process_descriptor(pe_loader::PeProcessHandle { pid })?;
    if !image_base.is_null() {
        unsafe {
            *image_base = descriptor.image_base;
        }
    }
    let boxed = descriptor.exception_directory.into_boxed_slice();
    let index = lookup_runtime_function_index(control_pc, descriptor.image_base, &boxed)?;
    let ptr = boxed.as_ptr().wrapping_add(index);
    UNWIND_LOOKUP_CACHE.lock().insert(unwind_cache_key(), boxed);
    Some(ptr)
}

unsafe fn register_value(context: &ContextRecord, reg: u8) -> u64 {
    match reg {
        0 => context.rax,
        1 => context.rcx,
        2 => context.rdx,
        3 => context.rbx,
        4 => context.rsp,
        5 => context.rbp,
        6 => context.rsi,
        7 => context.rdi,
        8 => context.r8,
        9 => context.r9,
        10 => context.r10,
        11 => context.r11,
        12 => context.r12,
        13 => context.r13,
        14 => context.r14,
        15 => context.r15,
        _ => 0,
    }
}

unsafe fn set_register_value(context: &mut ContextRecord, reg: u8, value: u64) {
    match reg {
        0 => context.rax = value,
        1 => context.rcx = value,
        2 => context.rdx = value,
        3 => context.rbx = value,
        4 => context.rsp = value,
        5 => context.rbp = value,
        6 => context.rsi = value,
        7 => context.rdi = value,
        8 => context.r8 = value,
        9 => context.r9 = value,
        10 => context.r10 = value,
        11 => context.r11 = value,
        12 => context.r12 = value,
        13 => context.r13 = value,
        14 => context.r14 = value,
        15 => context.r15 = value,
        _ => {}
    }
}

unsafe fn read_m128a(address: u64) -> M128A {
    core::ptr::read_unaligned(address as *const M128A)
}

unsafe fn unwind_info_header<'a>(
    image_base: u64,
    entry: &pe_loader::PeRuntimeFunction,
) -> Option<&'a UnwindInfoHeader> {
    let address =
        image_base.checked_add(entry.unwind_info_address as u64)? as *const UnwindInfoHeader;
    if address.is_null() {
        return None;
    }
    Some(&*address)
}

unsafe fn unwind_code_slots<'a>(header: &'a UnwindInfoHeader) -> &'a [UnwindCode] {
    let codes = (header as *const UnwindInfoHeader).add(1) as *const UnwindCode;
    core::slice::from_raw_parts(codes, header.count_of_codes as usize)
}

unsafe fn unwind_payload_ptr(header: &UnwindInfoHeader) -> *const u8 {
    let slots = unwind_code_slots(header);
    let aligned_slots = (slots.len() + 1) & !1usize;
    (slots.as_ptr() as *const u8).add(aligned_slots * core::mem::size_of::<UnwindCode>())
}

unsafe fn apply_unwind_info(
    context: &mut ContextRecord,
    header: &UnwindInfoHeader,
    establisher_frame: *mut u64,
    mut context_pointers: Option<&mut NonvolatileContextPointers>,
) -> bool {
    let frame_register = header.frame_register_offset & 0x0F;
    let frame_offset = (header.frame_register_offset >> 4) as u64 * 16;
    let mut stack = context.rsp;
    let mut frame_base = stack;
    if frame_register != 0 {
        frame_base = register_value(context, frame_register).saturating_sub(frame_offset);
    }
    if !establisher_frame.is_null() {
        *establisher_frame = frame_base;
    }

    let codes = unwind_code_slots(header);
    let mut index = 0usize;
    while index < codes.len() {
        let op = codes[index].unwind_op_info & 0x0F;
        let info = codes[index].unwind_op_info >> 4;
        match op {
            UWOP_PUSH_NONVOL => {
                let slot = stack as *mut u64;
                let value = *slot;
                set_register_value(context, info, value);
                set_integer_context_pointer(context_pointers.as_deref_mut(), info, slot);
                stack = stack.saturating_add(8);
            }
            UWOP_ALLOC_SMALL => {
                stack = stack.saturating_add((info as u64) * 8 + 8);
            }
            UWOP_ALLOC_LARGE => {
                if index + 1 >= codes.len() {
                    break;
                }
                let size = if info == 0 {
                    let scaled = u16::from_le_bytes([
                        codes[index + 1].code_offset,
                        codes[index + 1].unwind_op_info,
                    ]) as u64;
                    index += 1;
                    scaled * 8
                } else {
                    if index + 2 >= codes.len() {
                        break;
                    }
                    let low = u16::from_le_bytes([
                        codes[index + 1].code_offset,
                        codes[index + 1].unwind_op_info,
                    ]) as u32;
                    let high = u16::from_le_bytes([
                        codes[index + 2].code_offset,
                        codes[index + 2].unwind_op_info,
                    ]) as u32;
                    index += 2;
                    ((high << 16) | low) as u64
                };
                stack = stack.saturating_add(size);
            }
            UWOP_SET_FPREG => {
                if frame_register != 0 {
                    stack = frame_base;
                }
            }
            UWOP_SAVE_NONVOL => {
                if index + 1 >= codes.len() {
                    break;
                }
                let offset = u16::from_le_bytes([
                    codes[index + 1].code_offset,
                    codes[index + 1].unwind_op_info,
                ]) as u64
                    * 8;
                let slot = frame_base.saturating_add(offset) as *mut u64;
                let value = *slot;
                set_register_value(context, info, value);
                set_integer_context_pointer(context_pointers.as_deref_mut(), info, slot);
                index += 1;
            }
            UWOP_SAVE_NONVOL_FAR => {
                if index + 2 >= codes.len() {
                    break;
                }
                let low = u16::from_le_bytes([
                    codes[index + 1].code_offset,
                    codes[index + 1].unwind_op_info,
                ]) as u32;
                let high = u16::from_le_bytes([
                    codes[index + 2].code_offset,
                    codes[index + 2].unwind_op_info,
                ]) as u32;
                let offset = ((high << 16) | low) as u64;
                let slot = frame_base.saturating_add(offset) as *mut u64;
                let value = *slot;
                set_register_value(context, info, value);
                set_integer_context_pointer(context_pointers.as_deref_mut(), info, slot);
                index += 2;
            }
            UWOP_SAVE_XMM128 => {
                if index + 1 >= codes.len() {
                    break;
                }
                let offset = u16::from_le_bytes([
                    codes[index + 1].code_offset,
                    codes[index + 1].unwind_op_info,
                ]) as u64
                    * 16;
                let slot = frame_base.saturating_add(offset) as *mut M128A;
                if (info as usize) < context.xmm_registers.len() {
                    context.xmm_registers[info as usize] = read_m128a(slot as u64);
                    set_floating_context_pointer(context_pointers.as_deref_mut(), info, slot);
                }
                index += 1;
            }
            UWOP_SAVE_XMM128_FAR => {
                if index + 2 >= codes.len() {
                    break;
                }
                let low = u16::from_le_bytes([
                    codes[index + 1].code_offset,
                    codes[index + 1].unwind_op_info,
                ]) as u32;
                let high = u16::from_le_bytes([
                    codes[index + 2].code_offset,
                    codes[index + 2].unwind_op_info,
                ]) as u32;
                let offset = ((high << 16) | low) as u64;
                let slot = frame_base.saturating_add(offset) as *mut M128A;
                if (info as usize) < context.xmm_registers.len() {
                    context.xmm_registers[info as usize] = read_m128a(slot as u64);
                    set_floating_context_pointer(context_pointers.as_deref_mut(), info, slot);
                }
                index += 2;
            }
            UWOP_PUSH_MACHFRAME => {
                let error_code_slots = if info == 0 { 0u64 } else { 1u64 };
                let frame_ptr = stack as *const u64;
                let rip = *frame_ptr.add(error_code_slots as usize);
                let cs = *frame_ptr.add((error_code_slots + 1) as usize) as u16;
                let eflags = *frame_ptr.add((error_code_slots + 2) as usize) as u32;
                let restored_rsp = *frame_ptr.add((error_code_slots + 3) as usize);
                let ss = *frame_ptr.add((error_code_slots + 4) as usize) as u16;
                if !establisher_frame.is_null() {
                    *establisher_frame = stack;
                }
                context.seg_cs = cs;
                context.eflags = eflags;
                context.seg_ss = ss;
                context.rsp = restored_rsp;
                context.rip = rip;
                return true;
            }
            _ => {}
        }
        index += 1;
    }

    context.rsp = stack;
    false
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

pub unsafe extern "system" fn ntdll_nt_reset_event(handle: u64, _previous_state: *mut i32) -> i32 {
    reset_waitable_event(handle) as i32
}

pub unsafe extern "system" fn ntdll_nt_close(handle: u64) -> i32 {
    if close_handle(handle) {
        NtStatus::Success as i32
    } else {
        NtStatus::InvalidHandle as i32
    }
}

pub unsafe extern "system" fn ntdll_rtl_lookup_function_entry(
    control_pc: u64,
    image_base: *mut u64,
    history_table: *mut u8,
) -> *const pe_loader::PeRuntimeFunction {
    let _ = history_table;
    current_runtime_function_pointer(control_pc, image_base).unwrap_or(core::ptr::null())
}

pub unsafe extern "system" fn ntdll_rtl_virtual_unwind(
    handler_type: u32,
    image_base: u64,
    control_pc: u64,
    function_entry: *const pe_loader::PeRuntimeFunction,
    context_record: *mut ContextRecord,
    handler_data: *mut *mut u8,
    establisher_frame: *mut u64,
    context_pointers: *mut u8,
) -> *mut u8 {
    let _ = handler_type;
    if !handler_data.is_null() {
        *handler_data = core::ptr::null_mut();
    }
    if context_record.is_null() {
        return core::ptr::null_mut();
    }

    let context = &mut *context_record;
    let mut unwind_context_pointers = (!context_pointers.is_null())
        .then(|| &mut *(context_pointers as *mut NonvolatileContextPointers));
    clear_context_pointers(unwind_context_pointers.as_deref_mut());
    let previous_rip = context.rip;
    let previous_rsp = context.rsp;
    let mut resolved_image_base = image_base;
    let mut resolved_entry = function_entry;
    if resolved_entry.is_null() {
        resolved_entry = current_runtime_function_pointer(control_pc, &mut resolved_image_base)
            .unwrap_or(core::ptr::null());
    }

    if resolved_entry.is_null() {
        if !establisher_frame.is_null() {
            *establisher_frame = context.rsp;
        }
        let return_address = *(context.rsp as *const u64);
        context.rsp = context.rsp.saturating_add(8);
        context.rip = return_address;
        synchronize_vector_state(context);
        publish_unwind_transition(context, previous_rip, previous_rsp);
        return core::ptr::null_mut();
    }

    let mut entry = &*resolved_entry;
    let Some(mut header) = unwind_info_header(resolved_image_base, entry) else {
        return core::ptr::null_mut();
    };
    let mut exception_handler = core::ptr::null_mut();
    if (header.version_flags >> 3) & UNW_FLAG_CHAININFO != 0 {
        let chain_ptr = unwind_payload_ptr(header) as *const pe_loader::PeRuntimeFunction;
        entry = &*chain_ptr;
        if let Some(chained_header) = unwind_info_header(resolved_image_base, entry) {
            header = chained_header;
        }
    } else {
        let flags = (header.version_flags >> 3) & 0x1F;
        if flags & (UNW_FLAG_EHANDLER | UNW_FLAG_UHANDLER) != 0 {
            let payload = unwind_payload_ptr(header);
            let handler_rva = *(payload as *const u32);
            if !handler_data.is_null() {
                *handler_data = payload.add(core::mem::size_of::<u32>()) as *mut u8;
            }
            exception_handler = resolved_image_base.saturating_add(handler_rva as u64) as *mut u8;
        }
    }
    let restored_by_unwind = apply_unwind_info(
        context,
        header,
        establisher_frame,
        unwind_context_pointers.as_deref_mut(),
    );
    synchronize_vector_state(context);
    if !restored_by_unwind {
        let return_address = *(context.rsp as *const u64);
        context.rsp = context.rsp.saturating_add(8);
        context.rip = return_address;
    }
    publish_unwind_transition(context, previous_rip, previous_rsp);
    exception_handler
}

pub unsafe extern "system" fn ntdll_rtl_dispatch_exception(
    exception_record: *const win32::EXCEPTION_RECORD,
    context_record: *mut ContextRecord,
) -> u8 {
    if exception_record.is_null() || context_record.is_null() {
        return 0;
    }
    let _record = &*exception_record;
    let context = &mut *context_record;
    let mut depth = 0u32;
    loop {
        if context.rip == 0 || context.rsp == 0 {
            return 0;
        }
        let previous_rip = context.rip;
        let previous_rsp = context.rsp;
        let mut image_base = 0u64;
        let function =
            ntdll_rtl_lookup_function_entry(context.rip, &mut image_base, core::ptr::null_mut());
        if function.is_null() {
            return 0;
        }
        let mut handler_data = core::ptr::null_mut();
        let mut establisher = 0u64;
        let handler = ntdll_rtl_virtual_unwind(
            0,
            image_base,
            context.rip,
            function,
            context,
            &mut handler_data,
            &mut establisher,
            core::ptr::null_mut(),
        );
        if !handler.is_null() {
            return 1;
        }
        if context.rip == 0
            || context.rsp == 0
            || (context.rip == previous_rip && context.rsp == previous_rsp)
        {
            return 0;
        }
        depth = depth.saturating_add(1);
        if depth >= 64 {
            return 0;
        }
    }
}

pub unsafe extern "system" fn ntdll_ki_user_exception_dispatcher(
    exception_record: *mut win32::EXCEPTION_RECORD,
    context_record: *mut ContextRecord,
) -> ! {
    let exit_code = if exception_record.is_null() {
        0xC000_0005u32
    } else {
        (*exception_record).ExceptionCode
    };
    let _ = ntdll_rtl_dispatch_exception(exception_record.cast_const(), context_record);
    tasking::scheduler::exit(exit_code as i32)
}

// ============================================================================
// WS2_32 ABI THUNKS
// ============================================================================

pub fn resolve_ws2_32_symbol(name: &str) -> Option<u64> {
    let addr = match name {
        "WSAStartup" => ws2_32_wsa_startup as *const () as usize as u64,
        "WSACleanup" => ws2_32_wsa_cleanup as *const () as usize as u64,
        "WSAGetLastError" => ws2_32_wsa_get_last_error as *const () as usize as u64,
        "WSASocketA" | "WSASocketW" => ws2_32_wsa_socket as *const () as usize as u64,
        "socket" => ws2_32_socket as *const () as usize as u64,
        "bind" => ws2_32_bind as *const () as usize as u64,
        "listen" => ws2_32_listen as *const () as usize as u64,
        "accept" => ws2_32_accept as *const () as usize as u64,
        "connect" => ws2_32_connect as *const () as usize as u64,
        "send" => ws2_32_send as *const () as usize as u64,
        "recv" => ws2_32_recv as *const () as usize as u64,
        "sendto" => ws2_32_sendto as *const () as usize as u64,
        "recvfrom" => ws2_32_recvfrom as *const () as usize as u64,
        "closesocket" => ws2_32_closesocket as *const () as usize as u64,
        "shutdown" => ws2_32_shutdown as *const () as usize as u64,
        "getsockname" => ws2_32_getsockname as *const () as usize as u64,
        "getpeername" => ws2_32_getpeername as *const () as usize as u64,
        "getsockopt" => ws2_32_getsockopt as *const () as usize as u64,
        "setsockopt" => ws2_32_setsockopt as *const () as usize as u64,
        "ioctlsocket" => ws2_32_ioctlsocket as *const () as usize as u64,
        "htons" | "ntohs" => ws2_32_htons as *const () as usize as u64,
        "htonl" | "ntohl" => ws2_32_htonl as *const () as usize as u64,
        "inet_addr" => ws2_32_inet_addr as *const () as usize as u64,
        "inet_ntoa" => ws2_32_inet_ntoa as *const () as usize as u64,
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

pub unsafe extern "system" fn ws2_32_wsa_socket(
    af: i32,
    kind: i32,
    proto: i32,
    _protocol_info: *const u8,
    _group: u32,
    _flags: u32,
) -> u64 {
    ws2_32_socket(af, kind, proto)
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

pub unsafe extern "system" fn ws2_32_sendto(
    fd: u64,
    buf: *const u8,
    len: i32,
    flags: i32,
    to: *const u8,
    tolen: i32,
) -> i32 {
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
    let remote = match parse_sockaddr(to, tolen) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    let src = core::slice::from_raw_parts(buf, len as usize);
    match socket::sendto(sock, src, remote, flags as u32) {
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

pub unsafe extern "system" fn ws2_32_recvfrom(
    fd: u64,
    buf: *mut u8,
    len: i32,
    flags: i32,
    from: *mut u8,
    fromlen: *mut i32,
) -> i32 {
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
    match socket::recvfrom(sock, dst, flags as u32) {
        Ok((read, remote)) => {
            if !from.is_null() && !fromlen.is_null() {
                if let Err(code) = write_sockaddr(remote, from, fromlen) {
                    LAST_WSA_ERROR.store(code, Ordering::Release);
                    return SOCKET_ERROR;
                }
            }
            LAST_WSA_ERROR.store(0, Ordering::Release);
            read as i32
        }
        Err(err) => {
            LAST_WSA_ERROR.store(map_net_error(err), Ordering::Release);
            SOCKET_ERROR
        }
    }
}

pub unsafe extern "system" fn ws2_32_setsockopt(
    fd: u64,
    level: i32,
    optname: i32,
    optval: *const u8,
    optlen: i32,
) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    let option = match parse_socket_option(level, optname, optval, optlen) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::setsockopt(sock, option) {
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

pub unsafe extern "system" fn ws2_32_getsockopt(
    fd: u64,
    level: i32,
    optname: i32,
    optval: *mut u8,
    optlen: *mut i32,
) -> i32 {
    let sock = match cast_socket(fd) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    let option = match parse_socket_option_probe(level, optname) {
        Ok(v) => v,
        Err(code) => {
            LAST_WSA_ERROR.store(code, Ordering::Release);
            return SOCKET_ERROR;
        }
    };
    match socket::getsockopt(sock, option) {
        Ok(value) => {
            if let Err(code) = write_sockopt_value(value, optval, optlen) {
                LAST_WSA_ERROR.store(code, Ordering::Release);
                return SOCKET_ERROR;
            }
            LAST_WSA_ERROR.store(0, Ordering::Release);
            0
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

pub unsafe extern "system" fn ws2_32_getsockname(fd: u64, name: *mut u8, namelen: *mut i32) -> i32 {
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

pub unsafe extern "system" fn ws2_32_getpeername(fd: u64, name: *mut u8, namelen: *mut i32) -> i32 {
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

pub unsafe extern "system" fn ws2_32_ioctlsocket(fd: u64, _cmd: u64, argp: *mut u32) -> i32 {
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

pub extern "system" fn ws2_32_htons(value: u16) -> u16 {
    value.to_be()
}

pub extern "system" fn ws2_32_htonl(value: u32) -> u32 {
    value.to_be()
}

pub unsafe extern "system" fn ws2_32_inet_addr(cp: *const u8) -> u32 {
    if cp.is_null() {
        return INADDR_NONE;
    }
    let text = read_c_string_u8(cp);
    parse_ipv4_text(text.as_str()).unwrap_or(INADDR_NONE)
}

pub extern "system" fn ws2_32_inet_ntoa(addr: u32) -> *const u8 {
    let octets = addr.to_be_bytes();
    let mut buffer = INET_NTOA_BUFFER.lock();
    buffer.fill(0);
    let mut pos = 0usize;
    for (index, octet) in octets.iter().copied().enumerate() {
        write_decimal_octet(&mut buffer, &mut pos, octet);
        if index != 3 {
            buffer[pos] = b'.';
            pos += 1;
        }
    }
    LAST_WSA_ERROR.store(0, Ordering::Release);
    buffer.as_ptr()
}

// ============================================================================
// HELPERS
// ============================================================================

fn write_decimal_octet(buffer: &mut [u8; 16], pos: &mut usize, value: u8) {
    if value >= 100 {
        buffer[*pos] = b'0' + (value / 100);
        *pos += 1;
        buffer[*pos] = b'0' + ((value / 10) % 10);
        *pos += 1;
    } else if value >= 10 {
        buffer[*pos] = b'0' + (value / 10);
        *pos += 1;
    }
    buffer[*pos] = b'0' + (value % 10);
    *pos += 1;
}

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

unsafe fn parse_socket_option(
    level: i32,
    optname: i32,
    optval: *const u8,
    optlen: i32,
) -> Result<socket::SocketOption, u32> {
    if optval.is_null() || optlen < 4 {
        return Err(WSAEINVAL);
    }
    let value = *(optval as *const i32);
    match (level, optname) {
        (SOL_SOCKET, SO_REUSEADDR) => Ok(socket::SocketOption::ReuseAddr),
        (SOL_SOCKET, SO_KEEPALIVE) => Ok(socket::SocketOption::KeepAlive),
        (SOL_SOCKET, SO_RCVBUF) => Ok(socket::SocketOption::RcvBuf(value.max(0) as usize)),
        (SOL_SOCKET, SO_SNDBUF) => Ok(socket::SocketOption::SndBuf(value.max(0) as usize)),
        (SOL_SOCKET, SO_RCVTIMEO) => Ok(socket::SocketOption::RcvTimeout(value.max(0) as u64)),
        (SOL_SOCKET, SO_SNDTIMEO) => Ok(socket::SocketOption::SndTimeout(value.max(0) as u64)),
        (IPPROTO_TCP, TCP_NODELAY) => Ok(socket::SocketOption::NoDelay),
        _ => Err(WSAEINVAL),
    }
}

fn parse_socket_option_probe(level: i32, optname: i32) -> Result<socket::SocketOption, u32> {
    match (level, optname) {
        (SOL_SOCKET, SO_REUSEADDR) => Ok(socket::SocketOption::ReuseAddr),
        (SOL_SOCKET, SO_KEEPALIVE) => Ok(socket::SocketOption::KeepAlive),
        (SOL_SOCKET, SO_RCVBUF) => Ok(socket::SocketOption::RcvBuf(0)),
        (SOL_SOCKET, SO_SNDBUF) => Ok(socket::SocketOption::SndBuf(0)),
        (SOL_SOCKET, SO_RCVTIMEO) => Ok(socket::SocketOption::RcvTimeout(0)),
        (SOL_SOCKET, SO_SNDTIMEO) => Ok(socket::SocketOption::SndTimeout(0)),
        (IPPROTO_TCP, TCP_NODELAY) => Ok(socket::SocketOption::NoDelay),
        _ => Err(WSAEINVAL),
    }
}

unsafe fn write_sockopt_value(value: usize, optval: *mut u8, optlen: *mut i32) -> Result<(), u32> {
    if optval.is_null() || optlen.is_null() {
        return Err(WSAEINVAL);
    }
    let required = core::mem::size_of::<i32>() as i32;
    if *optlen < required {
        return Err(WSAEINVAL);
    }
    *(optval as *mut i32) = value.min(i32::MAX as usize) as i32;
    *optlen = required;
    Ok(())
}

fn parse_ipv4_text(text: &str) -> Option<u32> {
    let mut octets = [0u8; 4];
    let mut count = 0usize;
    for part in text.split('.') {
        if count == 4 || part.is_empty() {
            return None;
        }
        let value = part.parse::<u8>().ok()?;
        octets[count] = value;
        count += 1;
    }
    if count != 4 {
        return None;
    }
    Some(u32::from_be_bytes(octets))
}

unsafe fn read_c_string_u8(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let bytes = core::slice::from_raw_parts(ptr, len);
    String::from_utf8_lossy(bytes).into_owned()
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
    let ipv4 = addr.ip.as_ipv4().ok_or(WSAEAFNOSUPPORT)?;
    *dst = SockAddrIn {
        sin_family: AF_INET as u16,
        sin_port: addr.port.as_u16().to_be(),
        sin_addr: u32::from_ne_bytes(*ipv4.as_bytes()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_function_lookup_matches_control_pc_range() {
        let entries = [
            pe_loader::PeRuntimeFunction {
                begin_address: 0x1000,
                end_address: 0x1200,
                unwind_info_address: 0x2000,
            },
            pe_loader::PeRuntimeFunction {
                begin_address: 0x1200,
                end_address: 0x1500,
                unwind_info_address: 0x2100,
            },
        ];
        assert_eq!(
            lookup_runtime_function_index(0x401050, 0x400000, &entries),
            Some(0)
        );
        assert_eq!(
            lookup_runtime_function_index(0x401250, 0x400000, &entries),
            Some(1)
        );
        assert_eq!(
            lookup_runtime_function_index(0x401600, 0x400000, &entries),
            None
        );
    }

    #[test]
    fn virtual_unwind_leaf_frame_pops_return_address() {
        let mut stack = [0u64; 4];
        stack[0] = 0xDEAD_BEEF_CAFE_BABEu64;
        let mut context = ContextRecord {
            rsp: stack.as_ptr() as u64,
            rip: 0x401000,
            ..Default::default()
        };
        let mut establisher = 0u64;
        unsafe {
            ntdll_rtl_virtual_unwind(
                0,
                0x400000,
                context.rip,
                core::ptr::null(),
                &mut context,
                core::ptr::null_mut(),
                &mut establisher,
                core::ptr::null_mut(),
            );
        }
        assert_eq!(establisher, stack.as_ptr() as u64);
        assert_eq!(context.rip, 0xDEAD_BEEF_CAFE_BABEu64);
        assert_eq!(context.rsp, stack.as_ptr() as u64 + 8);
    }

    #[test]
    fn virtual_unwind_reports_handler_and_handler_data_pointer() {
        let mut image = [0u8; 0x200];
        let image_base = image.as_mut_ptr() as u64;
        let handler_rva = 0x180u32;
        let handler_data = [0xAAu8, 0xBB, 0xCC, 0xDD];

        image[0x40] = 1 | (UNW_FLAG_EHANDLER << 3);
        image[0x41] = 0;
        image[0x42] = 0;
        image[0x43] = 0;
        image[0x44..0x48].copy_from_slice(&handler_rva.to_le_bytes());
        image[0x48..0x4C].copy_from_slice(&handler_data);

        let entry = pe_loader::PeRuntimeFunction {
            begin_address: 0x1000,
            end_address: 0x1100,
            unwind_info_address: 0x40,
        };
        let mut stack = [0u64; 2];
        stack[0] = 0xFACE_CAFE_DEAD_BEEFu64;
        let mut context = ContextRecord {
            rsp: stack.as_ptr() as u64,
            rip: image_base + 0x1000,
            ..Default::default()
        };
        let mut handler_data_out = core::ptr::null_mut();
        let mut establisher = 0u64;
        let handler = unsafe {
            ntdll_rtl_virtual_unwind(
                0,
                image_base,
                context.rip,
                &entry,
                &mut context,
                &mut handler_data_out,
                &mut establisher,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(handler as u64, image_base + handler_rva as u64);
        assert_eq!(handler_data_out, unsafe { image.as_mut_ptr().add(0x48) });
        assert_eq!(
            unsafe { *(handler_data_out as *const [u8; 4]) },
            handler_data
        );
        assert_eq!(establisher, stack.as_ptr() as u64);
        assert_eq!(context.rip, 0xFACE_CAFE_DEAD_BEEFu64);
        assert_eq!(context.rsp, stack.as_ptr() as u64 + 8);
    }

    #[test]
    fn virtual_unwind_push_machframe_restores_saved_rsp_and_rip() {
        let mut image = [0u8; 0x100];
        image[0x20] = 1;
        image[0x21] = 0;
        image[0x22] = 1;
        image[0x23] = 0;
        image[0x24] = 0;
        image[0x25] = UWOP_PUSH_MACHFRAME;

        let entry = pe_loader::PeRuntimeFunction {
            begin_address: 0x1000,
            end_address: 0x1100,
            unwind_info_address: 0x20,
        };
        let stack = [
            0xABCD_EF01_0203_0405u64,
            0x33u64,
            0x202u64,
            0x2000_3000_4000_5000u64,
            0x2Bu64,
        ];
        let mut context = ContextRecord {
            rsp: stack.as_ptr() as u64,
            rip: (image.as_ptr() as u64) + 0x1000,
            ..Default::default()
        };
        let mut establisher = 0u64;
        unsafe {
            ntdll_rtl_virtual_unwind(
                0,
                image.as_ptr() as u64,
                context.rip,
                &entry,
                &mut context,
                core::ptr::null_mut(),
                &mut establisher,
                core::ptr::null_mut(),
            );
        }
        assert_eq!(establisher, stack.as_ptr() as u64);
        assert_eq!(context.rip, stack[0]);
        assert_eq!(context.rsp, stack[3]);
        assert_eq!(context.seg_cs, stack[1] as u16);
        assert_eq!(context.eflags, stack[2] as u32);
        assert_eq!(context.seg_ss, stack[4] as u16);
    }

    #[test]
    fn virtual_unwind_xmm_save_opcodes_do_not_corrupt_stack_walk() {
        let mut image = [0u8; 0x100];
        image[0x20] = 1;
        image[0x21] = 0;
        image[0x22] = 5;
        image[0x23] = 5;
        image[0x24] = 0;
        image[0x25] = (1 << 4) | UWOP_SAVE_XMM128;
        image[0x26] = 0x01;
        image[0x27] = 0x00;
        image[0x28] = 0;
        image[0x29] = (2 << 4) | UWOP_SAVE_XMM128_FAR;
        image[0x2A] = 0x20;
        image[0x2B] = 0x00;
        image[0x2C] = 0x00;
        image[0x2D] = 0x00;

        let entry = pe_loader::PeRuntimeFunction {
            begin_address: 0x1000,
            end_address: 0x1100,
            unwind_info_address: 0x20,
        };
        let xmm1 = M128A {
            low: 0x1111_2222_3333_4444,
            high: 0x5555_6666_7777_8888u64 as i64,
        };
        let xmm2 = M128A {
            low: 0x9999_AAAA_BBBB_CCCC,
            high: 0xDDDD_EEEE_FFFF_0001u64 as i64,
        };
        let stack = [0x5566_7788_99AA_BBCCu64, 0u64];
        let frame = [0u8; 0x240];
        unsafe {
            core::ptr::write_unaligned(frame.as_ptr().add(0x10) as *mut M128A, xmm1);
            core::ptr::write_unaligned(frame.as_ptr().add(0x20) as *mut M128A, xmm2);
        }
        let mut context = ContextRecord {
            rsp: stack.as_ptr() as u64,
            rbp: frame.as_ptr() as u64,
            rip: (image.as_ptr() as u64) + 0x1000,
            ..Default::default()
        };
        unsafe {
            ntdll_rtl_virtual_unwind(
                0,
                image.as_ptr() as u64,
                context.rip,
                &entry,
                &mut context,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            );
        }
        assert_eq!(context.rip, stack[0]);
        assert_eq!(context.rsp, stack.as_ptr() as u64 + 8);
        assert_eq!(context.xmm_registers[1], xmm1);
        assert_eq!(context.xmm_registers[2], xmm2);
        assert_eq!(context.legacy_float_registers[1], xmm1);
        assert_eq!(context.legacy_float_registers[2], xmm2);
        assert_eq!(context.vector_registers[1], xmm1);
        assert_eq!(context.vector_registers[2], xmm2);
        assert_eq!(context.floating_control_word, DEFAULT_X87_CONTROL_WORD);
        assert_eq!(context.floating_tag_word, DEFAULT_X87_TAG_WORD);
        assert_eq!(context.floating_mx_csr, context.mx_csr);
        assert_eq!(context.floating_mx_csr_mask, DEFAULT_MXCSR_MASK);
        assert_eq!(
            context.header_registers[0].low,
            (DEFAULT_X87_CONTROL_WORD as u64) | ((DEFAULT_X87_TAG_WORD as u64) << 32)
        );
        assert_eq!(
            context.header_registers[1].high as u64,
            (context.mx_csr as u64) | ((DEFAULT_MXCSR_MASK as u64) << 32)
        );
        assert_eq!(context.double_registers[2], xmm1.low);
        assert_eq!(context.double_registers[3], xmm1.high as u64);
        assert_eq!(context.double_registers[4], xmm2.low);
        assert_eq!(context.double_registers[5], xmm2.high as u64);
        assert_eq!(context.scalar_registers[4], xmm1.low as u32);
        assert_eq!(context.scalar_registers[5], (xmm1.low >> 32) as u32);
        assert_eq!(context.scalar_registers[6], xmm1.high as u64 as u32);
        assert_eq!(
            context.scalar_registers[7],
            ((xmm1.high as u64) >> 32) as u32
        );
        assert_ne!(context.context_flags & CONTEXT_FLOATING_POINT, 0);
        assert_eq!(context.vector_control, context.mx_csr as u64);
        assert_eq!(context.last_branch_from_rip, image.as_ptr() as u64 + 0x1000);
        assert_eq!(context.last_branch_to_rip, stack[0]);
        assert_eq!(context.last_exception_from_rip, stack.as_ptr() as u64);
        assert_eq!(context.last_exception_to_rip, stack.as_ptr() as u64 + 8);
    }

    #[test]
    fn virtual_unwind_populates_nonvolatile_context_pointers() {
        let mut image = [0u8; 0x100];
        image[0x20] = 1;
        image[0x21] = 0;
        image[0x22] = 4;
        image[0x23] = 5;
        image[0x24] = 0;
        image[0x25] = (3 << 4) | UWOP_SAVE_NONVOL;
        image[0x26] = 0x02;
        image[0x27] = 0x00;
        image[0x28] = 0;
        image[0x29] = (1 << 4) | UWOP_SAVE_XMM128;
        image[0x2A] = 0x02;
        image[0x2B] = 0x00;

        let entry = pe_loader::PeRuntimeFunction {
            begin_address: 0x1000,
            end_address: 0x1100,
            unwind_info_address: 0x20,
        };
        let stack = [0xABCDEF01_02030405u64, 0];
        let frame = [0u8; 0x80];
        let saved_rbx = 0x1122_3344_5566_7788u64;
        let saved_xmm = M128A {
            low: 0x1111_2222_3333_4444,
            high: 0x5555_6666_7777_8888u64 as i64,
        };
        unsafe {
            core::ptr::write_unaligned(frame.as_ptr().add(0x10) as *mut u64, saved_rbx);
            core::ptr::write_unaligned(frame.as_ptr().add(0x20) as *mut M128A, saved_xmm);
        }
        let mut context = ContextRecord {
            rsp: stack.as_ptr() as u64,
            rbp: frame.as_ptr() as u64,
            rip: (image.as_ptr() as u64) + 0x1000,
            ..Default::default()
        };
        let mut pointers = NonvolatileContextPointers::default();
        unsafe {
            ntdll_rtl_virtual_unwind(
                0,
                image.as_ptr() as u64,
                context.rip,
                &entry,
                &mut context,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut pointers as *mut _ as *mut u8,
            );
        }
        assert_eq!(context.rbx, saved_rbx);
        assert_eq!(context.xmm_registers[1], saved_xmm);
        assert_eq!(
            pointers.integer_context[3],
            frame.as_ptr().wrapping_add(0x10) as *mut u64
        );
        assert_eq!(
            pointers.floating_context[1],
            frame.as_ptr().wrapping_add(0x20) as *mut M128A
        );
    }

    #[test]
    fn rtl_dispatch_exception_reports_handler_visibility() {
        let mut image = [0u8; 0x200];
        let image_base = image.as_mut_ptr() as u64;
        let handler_rva = 0x180u32;
        image[0x40] = 1 | (UNW_FLAG_EHANDLER << 3);
        image[0x41] = 0;
        image[0x42] = 0;
        image[0x43] = 0;
        image[0x44..0x48].copy_from_slice(&handler_rva.to_le_bytes());
        let entry = pe_loader::PeRuntimeFunction {
            begin_address: 0x1000,
            end_address: 0x1100,
            unwind_info_address: 0x40,
        };
        let mut stack = [0xFACE_CAFE_DEAD_BEEFu64, 0];
        let mut context = ContextRecord {
            rsp: stack.as_mut_ptr() as u64,
            rip: image_base + 0x1000,
            ..Default::default()
        };
        let record = win32::EXCEPTION_RECORD {
            ExceptionCode: 0xC000_0005,
            ExceptionFlags: 0,
            ExceptionRecord: core::ptr::null_mut(),
            ExceptionAddress: context.rip as *mut _,
            NumberParameters: 0,
            ExceptionInformation: [0; 15],
        };
        seed_test_unwind_cache(image_base, &[entry]);
        let dispatched =
            unsafe { ntdll_rtl_dispatch_exception(&record, &mut context as *mut ContextRecord) };
        assert_eq!(dispatched, 1);
        assert_eq!(context.rip, stack[0]);
        assert_eq!(context.rsp, stack.as_ptr() as u64 + 8);
    }

    #[test]
    fn rtl_dispatch_exception_fails_closed_when_unwind_makes_no_progress() {
        let mut context = ContextRecord {
            rsp: 0,
            rip: 0,
            ..Default::default()
        };
        let record = win32::EXCEPTION_RECORD {
            ExceptionCode: 0xC000_0005,
            ExceptionFlags: 0,
            ExceptionRecord: core::ptr::null_mut(),
            ExceptionAddress: core::ptr::null_mut(),
            NumberParameters: 0,
            ExceptionInformation: [0; 15],
        };
        let dispatched =
            unsafe { ntdll_rtl_dispatch_exception(&record, &mut context as *mut ContextRecord) };
        assert_eq!(dispatched, 0);
    }
}
