//! Host-side SQLite bring-up seam for `ech-db`.
//!
//! The upstream amalgamation stays under `third_party/curated/sqlite/`.
//! This module owns allocator installation, host-VFS registration, open policy,
//! and row collection.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EchDbError {
    EngineUnavailable(&'static str),
    InvalidPath,
    InteriorNul,
    Open { code: i32, message: String },
    Execute { code: i32, message: String },
}

pub type Result<T> = core::result::Result<T, EchDbError>;

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
mod host {
    use super::{EchDbError, Result};
    use alloc::{
        format,
        string::{String, ToString},
        vec::Vec,
    };
    use core::ffi::{c_char, c_int, c_void, CStr};
    use core::mem::size_of;
    use core::ptr::{copy_nonoverlapping, null, null_mut, NonNull};
    use std::alloc::{alloc, dealloc, realloc, Layout};
    use std::collections::{HashMap, HashSet};
    use std::ffi::CString;
    use std::fs::{self, File, OpenOptions};
    use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering},
        Mutex, OnceLock,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    type Sqlite3Int64 = i64;
    type Sqlite3Filename = *const c_char;
    type Sqlite3SyscallPtr = Option<unsafe extern "C" fn()>;

    const SQLITE_OK: c_int = 0;
    const SQLITE_ERROR: c_int = 1;
    const SQLITE_PERM: c_int = 3;
    const SQLITE_BUSY: c_int = 5;
    const SQLITE_NOMEM: c_int = 7;
    const SQLITE_READONLY: c_int = 8;
    const SQLITE_IOERR: c_int = 10;
    const SQLITE_NOTFOUND: c_int = 12;
    const SQLITE_CANTOPEN: c_int = 14;
    const SQLITE_MISUSE: c_int = 21;
    const SQLITE_IOERR_SHORT_READ: c_int = SQLITE_IOERR | (2 << 8);

    const SQLITE_CONFIG_MALLOC: c_int = 4;
    const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
    const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
    const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
    const SQLITE_OPEN_DELETEONCLOSE: c_int = 0x0000_0008;
    const SQLITE_OPEN_EXCLUSIVE: c_int = 0x0000_0010;
    const SQLITE_OPEN_MEMORY: c_int = 0x0000_0080;
    const SQLITE_OPEN_MAIN_DB: c_int = 0x0000_0100;
    const SQLITE_OPEN_TEMP_DB: c_int = 0x0000_0200;
    const SQLITE_OPEN_TRANSIENT_DB: c_int = 0x0000_0400;
    const SQLITE_OPEN_MAIN_JOURNAL: c_int = 0x0000_0800;
    const SQLITE_OPEN_TEMP_JOURNAL: c_int = 0x0000_1000;
    const SQLITE_OPEN_SUBJOURNAL: c_int = 0x0000_2000;
    const SQLITE_OPEN_SUPER_JOURNAL: c_int = 0x0000_4000;
    const SQLITE_OPEN_WAL: c_int = 0x0008_0000;

    const SQLITE_LOCK_NONE: c_int = 0;
    const SQLITE_LOCK_SHARED: c_int = 1;
    const SQLITE_LOCK_RESERVED: c_int = 2;
    const SQLITE_LOCK_PENDING: c_int = 3;
    const SQLITE_LOCK_EXCLUSIVE: c_int = 4;

    const SQLITE_ACCESS_EXISTS: c_int = 0;
    const SQLITE_ACCESS_READWRITE: c_int = 1;
    const SQLITE_ACCESS_READ: c_int = 2;

    const SQLITE_FCNTL_LOCKSTATE: c_int = 1;
    const SQLITE_FCNTL_VFS_POINTER: c_int = 27;

    const HOST_VFS_NAME: &[u8] = b"echos-host-vfs\0";
    const MAX_PATHNAME: usize = 4096;
    const DEFAULT_SECTOR_SIZE: c_int = 4096;
    const JULIAN_UNIX_EPOCH_DAYS: f64 = 2_440_587.5;

    #[repr(C)]
    struct sqlite3 {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct sqlite3_mem_methods {
        x_malloc: Option<unsafe extern "C" fn(c_int) -> *mut c_void>,
        x_free: Option<unsafe extern "C" fn(*mut c_void)>,
        x_realloc: Option<unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_void>,
        x_size: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
        x_roundup: Option<unsafe extern "C" fn(c_int) -> c_int>,
        x_init: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
        x_shutdown: Option<unsafe extern "C" fn(*mut c_void)>,
        p_app_data: *mut c_void,
    }

    #[repr(C)]
    struct sqlite3_file {
        p_methods: *const sqlite3_io_methods,
    }

    #[repr(C)]
    struct sqlite3_io_methods {
        i_version: c_int,
        x_close: Option<unsafe extern "C" fn(*mut sqlite3_file) -> c_int>,
        x_read: Option<
            unsafe extern "C" fn(*mut sqlite3_file, *mut c_void, c_int, Sqlite3Int64) -> c_int,
        >,
        x_write: Option<
            unsafe extern "C" fn(*mut sqlite3_file, *const c_void, c_int, Sqlite3Int64) -> c_int,
        >,
        x_truncate: Option<unsafe extern "C" fn(*mut sqlite3_file, Sqlite3Int64) -> c_int>,
        x_sync: Option<unsafe extern "C" fn(*mut sqlite3_file, c_int) -> c_int>,
        x_file_size: Option<unsafe extern "C" fn(*mut sqlite3_file, *mut Sqlite3Int64) -> c_int>,
        x_lock: Option<unsafe extern "C" fn(*mut sqlite3_file, c_int) -> c_int>,
        x_unlock: Option<unsafe extern "C" fn(*mut sqlite3_file, c_int) -> c_int>,
        x_check_reserved_lock: Option<unsafe extern "C" fn(*mut sqlite3_file, *mut c_int) -> c_int>,
        x_file_control:
            Option<unsafe extern "C" fn(*mut sqlite3_file, c_int, *mut c_void) -> c_int>,
        x_sector_size: Option<unsafe extern "C" fn(*mut sqlite3_file) -> c_int>,
        x_device_characteristics: Option<unsafe extern "C" fn(*mut sqlite3_file) -> c_int>,
        x_shm_map: Option<
            unsafe extern "C" fn(*mut sqlite3_file, c_int, c_int, c_int, *mut *mut c_void) -> c_int,
        >,
        x_shm_lock: Option<unsafe extern "C" fn(*mut sqlite3_file, c_int, c_int, c_int) -> c_int>,
        x_shm_barrier: Option<unsafe extern "C" fn(*mut sqlite3_file)>,
        x_shm_unmap: Option<unsafe extern "C" fn(*mut sqlite3_file, c_int) -> c_int>,
        x_fetch: Option<
            unsafe extern "C" fn(*mut sqlite3_file, Sqlite3Int64, c_int, *mut *mut c_void) -> c_int,
        >,
        x_unfetch:
            Option<unsafe extern "C" fn(*mut sqlite3_file, Sqlite3Int64, *mut c_void) -> c_int>,
    }

    #[repr(C)]
    struct sqlite3_vfs {
        i_version: c_int,
        sz_os_file: c_int,
        mx_pathname: c_int,
        p_next: *mut sqlite3_vfs,
        z_name: *const c_char,
        p_app_data: *mut c_void,
        x_open: Option<
            unsafe extern "C" fn(
                *mut sqlite3_vfs,
                Sqlite3Filename,
                *mut sqlite3_file,
                c_int,
                *mut c_int,
            ) -> c_int,
        >,
        x_delete: Option<unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char, c_int) -> c_int>,
        x_access: Option<
            unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char, c_int, *mut c_int) -> c_int,
        >,
        x_full_pathname: Option<
            unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char, c_int, *mut c_char) -> c_int,
        >,
        x_dl_open: Option<unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char) -> *mut c_void>,
        x_dl_error: Option<unsafe extern "C" fn(*mut sqlite3_vfs, c_int, *mut c_char)>,
        x_dl_sym: Option<
            unsafe extern "C" fn(*mut sqlite3_vfs, *mut c_void, *const c_char) -> Sqlite3SyscallPtr,
        >,
        x_dl_close: Option<unsafe extern "C" fn(*mut sqlite3_vfs, *mut c_void)>,
        x_randomness: Option<unsafe extern "C" fn(*mut sqlite3_vfs, c_int, *mut c_char) -> c_int>,
        x_sleep: Option<unsafe extern "C" fn(*mut sqlite3_vfs, c_int) -> c_int>,
        x_current_time: Option<unsafe extern "C" fn(*mut sqlite3_vfs, *mut f64) -> c_int>,
        x_get_last_error:
            Option<unsafe extern "C" fn(*mut sqlite3_vfs, c_int, *mut c_char) -> c_int>,
        x_current_time_int64:
            Option<unsafe extern "C" fn(*mut sqlite3_vfs, *mut Sqlite3Int64) -> c_int>,
        x_set_system_call: Option<
            unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char, Sqlite3SyscallPtr) -> c_int,
        >,
        x_get_system_call:
            Option<unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char) -> Sqlite3SyscallPtr>,
        x_next_system_call:
            Option<unsafe extern "C" fn(*mut sqlite3_vfs, *const c_char) -> *const c_char>,
    }

    unsafe extern "C" {
        fn sqlite3_config(op: c_int, ...) -> c_int;
        fn sqlite3_initialize() -> c_int;
        fn sqlite3_open_v2(
            filename: *const c_char,
            pp_db: *mut *mut sqlite3,
            flags: c_int,
            z_vfs: *const c_char,
        ) -> c_int;
        fn sqlite3_close(db: *mut sqlite3) -> c_int;
        fn sqlite3_exec(
            db: *mut sqlite3,
            sql: *const c_char,
            callback: Option<
                unsafe extern "C" fn(
                    arg: *mut c_void,
                    columns: c_int,
                    values: *mut *mut c_char,
                    names: *mut *mut c_char,
                ) -> c_int,
            >,
            arg: *mut c_void,
            errmsg: *mut *mut c_char,
        ) -> c_int;
        fn sqlite3_free(ptr: *mut c_void);
        fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
        fn sqlite3_vfs_register(vfs: *mut sqlite3_vfs, make_dflt: c_int) -> c_int;
    }

    #[repr(C)]
    struct AllocationHeader {
        size: usize,
    }

    #[derive(Default)]
    struct LockRecord {
        shared: HashSet<u64>,
        reserved: Option<u64>,
        pending: Option<u64>,
        exclusive: Option<u64>,
    }

    #[repr(C)]
    struct HostFile {
        base: sqlite3_file,
        file: Option<File>,
        path: Option<PathBuf>,
        delete_on_close: bool,
        sector_size: c_int,
        lock_state: c_int,
        lock_id: u64,
    }

    static LAST_OS_ERROR: AtomicI32 = AtomicI32::new(0);
    static VFS_OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TEMP_ID: AtomicU64 = AtomicU64::new(1);
    static LOCK_ID: AtomicU64 = AtomicU64::new(1);
    static RANDOM_SEED: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    static LOCK_TABLE: OnceLock<Mutex<HashMap<PathBuf, LockRecord>>> = OnceLock::new();

    const HEADER_ALIGN: usize = core::mem::align_of::<AllocationHeader>();
    const HEADER_SIZE: usize = core::mem::size_of::<AllocationHeader>();

    static HOST_IO_METHODS: sqlite3_io_methods = sqlite3_io_methods {
        i_version: 1,
        x_close: Some(host_file_close),
        x_read: Some(host_file_read),
        x_write: Some(host_file_write),
        x_truncate: Some(host_file_truncate),
        x_sync: Some(host_file_sync),
        x_file_size: Some(host_file_size),
        x_lock: Some(host_file_lock),
        x_unlock: Some(host_file_unlock),
        x_check_reserved_lock: Some(host_file_check_reserved_lock),
        x_file_control: Some(host_file_control),
        x_sector_size: Some(host_file_sector_size),
        x_device_characteristics: Some(host_file_device_characteristics),
        x_shm_map: None,
        x_shm_lock: None,
        x_shm_barrier: None,
        x_shm_unmap: None,
        x_fetch: None,
        x_unfetch: None,
    };

    static mut HOST_VFS: sqlite3_vfs = sqlite3_vfs {
        i_version: 1,
        sz_os_file: size_of::<HostFile>() as c_int,
        mx_pathname: MAX_PATHNAME as c_int,
        p_next: null_mut(),
        z_name: HOST_VFS_NAME.as_ptr().cast::<c_char>(),
        p_app_data: null_mut(),
        x_open: Some(host_vfs_open),
        x_delete: Some(host_vfs_delete),
        x_access: Some(host_vfs_access),
        x_full_pathname: Some(host_vfs_full_pathname),
        x_dl_open: Some(host_vfs_dl_open),
        x_dl_error: Some(host_vfs_dl_error),
        x_dl_sym: Some(host_vfs_dl_sym),
        x_dl_close: Some(host_vfs_dl_close),
        x_randomness: Some(host_vfs_randomness),
        x_sleep: Some(host_vfs_sleep),
        x_current_time: Some(host_vfs_current_time),
        x_get_last_error: Some(host_vfs_get_last_error),
        x_current_time_int64: None,
        x_set_system_call: None,
        x_get_system_call: None,
        x_next_system_call: None,
    };

    static SQLITE_INIT: OnceLock<Result<()>> = OnceLock::new();

    fn lock_table() -> &'static Mutex<HashMap<PathBuf, LockRecord>> {
        LOCK_TABLE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn layout_for(payload: usize) -> Option<Layout> {
        Layout::from_size_align(payload.checked_add(HEADER_SIZE)?, HEADER_ALIGN).ok()
    }

    unsafe fn header_from_payload(ptr: *mut c_void) -> *mut AllocationHeader {
        unsafe { (ptr as *mut u8).sub(HEADER_SIZE).cast::<AllocationHeader>() }
    }

    unsafe extern "C" fn sqlite_malloc(size: c_int) -> *mut c_void {
        let payload = usize::try_from(size.max(0)).unwrap_or(0).max(1);
        let Some(layout) = layout_for(payload) else {
            return null_mut();
        };
        let raw = unsafe { alloc(layout) };
        if raw.is_null() {
            return null_mut();
        }
        let header = raw.cast::<AllocationHeader>();
        unsafe {
            (*header).size = payload;
            raw.add(HEADER_SIZE).cast::<c_void>()
        }
    }

    unsafe extern "C" fn sqlite_free(ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }
        let header = unsafe { header_from_payload(ptr) };
        let payload = unsafe { (*header).size };
        if let Some(layout) = layout_for(payload) {
            unsafe { dealloc(header.cast::<u8>(), layout) };
        }
    }

    unsafe extern "C" fn sqlite_realloc(ptr: *mut c_void, size: c_int) -> *mut c_void {
        if ptr.is_null() {
            return unsafe { sqlite_malloc(size) };
        }
        let new_payload = usize::try_from(size.max(0)).unwrap_or(0).max(1);
        let header = unsafe { header_from_payload(ptr) };
        let old_payload = unsafe { (*header).size };
        let Some(old_layout) = layout_for(old_payload) else {
            return null_mut();
        };
        let Some(new_total) = new_payload.checked_add(HEADER_SIZE) else {
            return null_mut();
        };
        let raw = unsafe { realloc(header.cast::<u8>(), old_layout, new_total) };
        if raw.is_null() {
            return null_mut();
        }
        unsafe {
            (*raw.cast::<AllocationHeader>()).size = new_payload;
            raw.add(HEADER_SIZE).cast::<c_void>()
        }
    }

    unsafe extern "C" fn sqlite_size(ptr: *mut c_void) -> c_int {
        if ptr.is_null() {
            return 0;
        }
        let size = unsafe { (*header_from_payload(ptr)).size };
        c_int::try_from(size).unwrap_or(c_int::MAX)
    }

    unsafe extern "C" fn sqlite_roundup(size: c_int) -> c_int {
        size.max(0).saturating_add(7) & !7
    }

    unsafe extern "C" fn sqlite_init(_: *mut c_void) -> c_int {
        SQLITE_OK
    }

    unsafe extern "C" fn sqlite_shutdown(_: *mut c_void) {}

    fn set_last_error_from_io(err: &std::io::Error) {
        LAST_OS_ERROR.store(err.raw_os_error().unwrap_or(-1), Ordering::Relaxed);
    }

    fn set_last_error_code(code: i32) {
        LAST_OS_ERROR.store(code, Ordering::Relaxed);
    }

    fn sqlite_error_from_io(err: &std::io::Error, open_path: bool) -> c_int {
        match err.kind() {
            ErrorKind::PermissionDenied => {
                if open_path {
                    SQLITE_CANTOPEN
                } else {
                    SQLITE_READONLY
                }
            }
            ErrorKind::NotFound if open_path => SQLITE_CANTOPEN,
            ErrorKind::OutOfMemory => SQLITE_NOMEM,
            _ => SQLITE_IOERR,
        }
    }

    fn ensure_sqlite_ready() -> Result<()> {
        SQLITE_INIT
            .get_or_init(|| unsafe {
                let methods = sqlite3_mem_methods {
                    x_malloc: Some(sqlite_malloc),
                    x_free: Some(sqlite_free),
                    x_realloc: Some(sqlite_realloc),
                    x_size: Some(sqlite_size),
                    x_roundup: Some(sqlite_roundup),
                    x_init: Some(sqlite_init),
                    x_shutdown: Some(sqlite_shutdown),
                    p_app_data: null_mut(),
                };
                let config = sqlite3_config(SQLITE_CONFIG_MALLOC, &methods as *const _);
                if config != SQLITE_OK {
                    return Err(EchDbError::Open {
                        code: config,
                        message: format!("sqlite3_config(SQLITE_CONFIG_MALLOC) failed: {config}"),
                    });
                }
                let register = sqlite3_vfs_register(&raw mut HOST_VFS, 0);
                if register != SQLITE_OK {
                    return Err(EchDbError::Open {
                        code: register,
                        message: format!("sqlite3_vfs_register(echos-host-vfs) failed: {register}"),
                    });
                }
                let init = sqlite3_initialize();
                if init != SQLITE_OK {
                    return Err(EchDbError::Open {
                        code: init,
                        message: format!("sqlite3_initialize failed: {init}"),
                    });
                }
                Ok(())
            })
            .clone()
    }

    fn absolute_path(path: &Path) -> Result<PathBuf> {
        if path.as_os_str().is_empty() {
            return Err(EchDbError::InvalidPath);
        }
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|err| EchDbError::Open {
                    code: SQLITE_CANTOPEN,
                    message: format!("current_dir failed: {err}"),
                })?
                .join(path)
        };
        Ok(joined)
    }

    fn normalize_open_filename(path: &str, use_vfs: bool) -> Result<CString> {
        if path.is_empty() {
            return Err(EchDbError::InvalidPath);
        }
        if !use_vfs || path == ":memory:" {
            return CString::new(path).map_err(|_| EchDbError::InteriorNul);
        }
        let absolute = absolute_path(Path::new(path))?;
        CString::new(absolute.to_string_lossy().as_bytes()).map_err(|_| EchDbError::InteriorNul)
    }

    fn path_from_vfs_name(name: *const c_char) -> core::result::Result<PathBuf, c_int> {
        if name.is_null() {
            return Ok(unique_temp_path(".db"));
        }
        let raw = unsafe { CStr::from_ptr(name) };
        let text = raw.to_str().map_err(|_| SQLITE_CANTOPEN)?;
        absolute_path(Path::new(text)).map_err(|_| SQLITE_CANTOPEN)
    }

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("echos-sqlite-{id}{suffix}"))
    }

    fn temp_suffix(flags: c_int) -> &'static str {
        if flags & SQLITE_OPEN_MAIN_JOURNAL != 0 || flags & SQLITE_OPEN_TEMP_JOURNAL != 0 {
            ".journal"
        } else if flags & SQLITE_OPEN_SUBJOURNAL != 0 {
            ".subjournal"
        } else if flags & SQLITE_OPEN_SUPER_JOURNAL != 0 {
            ".super-journal"
        } else if flags & SQLITE_OPEN_WAL != 0 {
            ".wal"
        } else {
            ".db"
        }
    }

    fn wants_write(flags: c_int) -> bool {
        flags & SQLITE_OPEN_READWRITE != 0
            || flags & SQLITE_OPEN_CREATE != 0
            || flags & SQLITE_OPEN_DELETEONCLOSE != 0
            || flags & SQLITE_OPEN_EXCLUSIVE != 0
            || flags & SQLITE_OPEN_TEMP_DB != 0
            || flags & SQLITE_OPEN_TRANSIENT_DB != 0
            || flags & SQLITE_OPEN_MAIN_JOURNAL != 0
            || flags & SQLITE_OPEN_TEMP_JOURNAL != 0
            || flags & SQLITE_OPEN_SUBJOURNAL != 0
            || flags & SQLITE_OPEN_SUPER_JOURNAL != 0
            || flags & SQLITE_OPEN_WAL != 0
    }

    fn error_message(db: *mut sqlite3, fallback: *mut c_char) -> String {
        if !fallback.is_null() {
            let text = unsafe { CStr::from_ptr(fallback) }
                .to_string_lossy()
                .into_owned();
            unsafe { sqlite3_free(fallback.cast::<c_void>()) };
            return text;
        }
        if db.is_null() {
            return String::from("unknown sqlite error");
        }
        unsafe { CStr::from_ptr(sqlite3_errmsg(db)) }
            .to_string_lossy()
            .into_owned()
    }

    unsafe fn host_file_mut(file: *mut sqlite3_file) -> &'static mut HostFile {
        unsafe { &mut *(file.cast::<HostFile>()) }
    }

    fn close_host_file(host_file: &mut HostFile) {
        let _ = release_lock_state(host_file);
        host_file.file.take();
        if host_file.delete_on_close {
            if let Some(path) = host_file.path.take() {
                let _ = fs::remove_file(path);
            }
        }
        host_file.base.p_methods = null();
    }

    fn release_lock_state(host_file: &mut HostFile) -> c_int {
        if host_file.lock_state == SQLITE_LOCK_NONE {
            return SQLITE_OK;
        }
        if let Some(path) = host_file.path.as_ref().cloned() {
            let mut locks = lock_table().lock().expect("lock table poisoned");
            if let Some(record) = locks.get_mut(&path) {
                record.shared.remove(&host_file.lock_id);
                if record.reserved == Some(host_file.lock_id) {
                    record.reserved = None;
                }
                if record.pending == Some(host_file.lock_id) {
                    record.pending = None;
                }
                if record.exclusive == Some(host_file.lock_id) {
                    record.exclusive = None;
                }
                if record.shared.is_empty()
                    && record.reserved.is_none()
                    && record.pending.is_none()
                    && record.exclusive.is_none()
                {
                    locks.remove(&path);
                }
            }
        }
        host_file.lock_state = SQLITE_LOCK_NONE;
        SQLITE_OK
    }

    fn promote_lock(host_file: &mut HostFile, requested: c_int) -> c_int {
        if requested <= host_file.lock_state {
            return SQLITE_OK;
        }
        let Some(path) = host_file.path.as_ref().cloned() else {
            host_file.lock_state = requested;
            return SQLITE_OK;
        };
        let mut locks = lock_table().lock().expect("lock table poisoned");
        let record = locks.entry(path).or_default();
        let owner = host_file.lock_id;
        match requested {
            SQLITE_LOCK_SHARED => {
                if record.exclusive.is_some() && record.exclusive != Some(owner) {
                    return SQLITE_BUSY;
                }
                record.shared.insert(owner);
            }
            SQLITE_LOCK_RESERVED => {
                if record.exclusive.is_some() && record.exclusive != Some(owner) {
                    return SQLITE_BUSY;
                }
                if record.reserved.is_some() && record.reserved != Some(owner) {
                    return SQLITE_BUSY;
                }
                record.shared.insert(owner);
                record.reserved = Some(owner);
            }
            SQLITE_LOCK_PENDING => {
                if record.exclusive.is_some() && record.exclusive != Some(owner) {
                    return SQLITE_BUSY;
                }
                if record.pending.is_some() && record.pending != Some(owner) {
                    return SQLITE_BUSY;
                }
                if record.reserved.is_some() && record.reserved != Some(owner) {
                    return SQLITE_BUSY;
                }
                record.shared.insert(owner);
                record.reserved = Some(owner);
                record.pending = Some(owner);
            }
            SQLITE_LOCK_EXCLUSIVE => {
                let others_share = record.shared.iter().any(|holder| *holder != owner);
                let others_reserved = record.reserved.is_some() && record.reserved != Some(owner);
                let others_pending = record.pending.is_some() && record.pending != Some(owner);
                let others_exclusive =
                    record.exclusive.is_some() && record.exclusive != Some(owner);
                if others_share || others_reserved || others_pending || others_exclusive {
                    return SQLITE_BUSY;
                }
                record.shared.clear();
                record.shared.insert(owner);
                record.reserved = Some(owner);
                record.pending = Some(owner);
                record.exclusive = Some(owner);
            }
            _ => return SQLITE_MISUSE,
        }
        host_file.lock_state = requested;
        SQLITE_OK
    }

    fn demote_lock(host_file: &mut HostFile, requested: c_int) -> c_int {
        if requested >= host_file.lock_state {
            return SQLITE_OK;
        }
        let Some(path) = host_file.path.as_ref().cloned() else {
            host_file.lock_state = requested;
            return SQLITE_OK;
        };
        let mut locks = lock_table().lock().expect("lock table poisoned");
        if let Some(record) = locks.get_mut(&path) {
            let owner = host_file.lock_id;
            match requested {
                SQLITE_LOCK_NONE => {
                    record.shared.remove(&owner);
                    if record.reserved == Some(owner) {
                        record.reserved = None;
                    }
                    if record.pending == Some(owner) {
                        record.pending = None;
                    }
                    if record.exclusive == Some(owner) {
                        record.exclusive = None;
                    }
                }
                SQLITE_LOCK_SHARED => {
                    record.shared.insert(owner);
                    if record.reserved == Some(owner) {
                        record.reserved = None;
                    }
                    if record.pending == Some(owner) {
                        record.pending = None;
                    }
                    if record.exclusive == Some(owner) {
                        record.exclusive = None;
                    }
                }
                _ => return SQLITE_MISUSE,
            }
            if record.shared.is_empty()
                && record.reserved.is_none()
                && record.pending.is_none()
                && record.exclusive.is_none()
            {
                locks.remove(&path);
            }
        }
        host_file.lock_state = requested;
        SQLITE_OK
    }

    unsafe extern "C" fn host_vfs_open(
        _: *mut sqlite3_vfs,
        z_name: Sqlite3Filename,
        file: *mut sqlite3_file,
        flags: c_int,
        out_flags: *mut c_int,
    ) -> c_int {
        unsafe {
            (*file).p_methods = null();
        }
        VFS_OPEN_CALLS.fetch_add(1, Ordering::Relaxed);

        let path = if z_name.is_null() {
            unique_temp_path(temp_suffix(flags))
        } else {
            match path_from_vfs_name(z_name) {
                Ok(path) => path,
                Err(code) => return code,
            }
        };

        if flags & SQLITE_OPEN_MEMORY != 0 {
            set_last_error_code(SQLITE_MISUSE);
            return SQLITE_CANTOPEN;
        }

        if wants_write(flags) {
            if let Some(parent) = path.parent() {
                if let Err(err) = fs::create_dir_all(parent) {
                    set_last_error_from_io(&err);
                    return sqlite_error_from_io(&err, true);
                }
            }
        }

        let mut options = OpenOptions::new();
        if flags & SQLITE_OPEN_READONLY != 0 && !wants_write(flags) {
            options.read(true);
        } else {
            options.read(true).write(true);
        }
        if flags & SQLITE_OPEN_CREATE != 0 {
            options.create(true);
        }
        if flags & SQLITE_OPEN_EXCLUSIVE != 0 {
            options.create_new(true);
        }

        let open_result = options.open(&path);
        let host = file.cast::<HostFile>();
        match open_result {
            Ok(opened) => {
                let host_file = HostFile {
                    base: sqlite3_file {
                        p_methods: &HOST_IO_METHODS,
                    },
                    file: Some(opened),
                    path: Some(path),
                    delete_on_close: flags & SQLITE_OPEN_DELETEONCLOSE != 0 || z_name.is_null(),
                    sector_size: DEFAULT_SECTOR_SIZE,
                    lock_state: SQLITE_LOCK_NONE,
                    lock_id: LOCK_ID.fetch_add(1, Ordering::Relaxed),
                };
                unsafe {
                    host.write(host_file);
                    if !out_flags.is_null() {
                        *out_flags = flags;
                    }
                }
                SQLITE_OK
            }
            Err(err) => {
                set_last_error_from_io(&err);
                sqlite_error_from_io(&err, true)
            }
        }
    }

    unsafe extern "C" fn host_vfs_delete(
        _: *mut sqlite3_vfs,
        z_name: *const c_char,
        _: c_int,
    ) -> c_int {
        let Ok(path) = path_from_vfs_name(z_name) else {
            return SQLITE_CANTOPEN;
        };
        match fs::remove_file(path) {
            Ok(()) => SQLITE_OK,
            Err(err) if err.kind() == ErrorKind::NotFound => SQLITE_OK,
            Err(err) => {
                set_last_error_from_io(&err);
                sqlite_error_from_io(&err, false)
            }
        }
    }

    fn can_readwrite(path: &Path) -> bool {
        if path.is_dir() {
            return fs::metadata(path)
                .map(|meta| !meta.permissions().readonly())
                .unwrap_or(false);
        }
        OpenOptions::new().read(true).write(true).open(path).is_ok()
    }

    unsafe extern "C" fn host_vfs_access(
        _: *mut sqlite3_vfs,
        z_name: *const c_char,
        flags: c_int,
        out: *mut c_int,
    ) -> c_int {
        if out.is_null() {
            return SQLITE_MISUSE;
        }
        let result = if z_name.is_null() {
            0
        } else {
            match path_from_vfs_name(z_name) {
                Ok(path) => match flags {
                    SQLITE_ACCESS_EXISTS => i32::from(path.exists()),
                    SQLITE_ACCESS_READ => {
                        i32::from(OpenOptions::new().read(true).open(path).is_ok())
                    }
                    SQLITE_ACCESS_READWRITE => i32::from(can_readwrite(&path)),
                    _ => 0,
                },
                Err(_) => 0,
            }
        };
        unsafe {
            *out = result;
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_vfs_full_pathname(
        _: *mut sqlite3_vfs,
        z_name: *const c_char,
        n_out: c_int,
        out: *mut c_char,
    ) -> c_int {
        if out.is_null() || n_out <= 0 {
            return SQLITE_MISUSE;
        }
        let Ok(path) = path_from_vfs_name(z_name) else {
            return SQLITE_CANTOPEN;
        };
        let rendered = path.to_string_lossy();
        let bytes = rendered.as_bytes();
        if bytes.len() + 1 > n_out as usize {
            return SQLITE_CANTOPEN;
        }
        unsafe {
            copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
            *out.add(bytes.len()) = 0;
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_vfs_dl_open(_: *mut sqlite3_vfs, _: *const c_char) -> *mut c_void {
        null_mut()
    }

    unsafe extern "C" fn host_vfs_dl_error(
        _: *mut sqlite3_vfs,
        n_byte: c_int,
        z_err_msg: *mut c_char,
    ) {
        if z_err_msg.is_null() || n_byte <= 0 {
            return;
        }
        let message = b"dynamic loading disabled\0";
        let copy_len = message.len().min(n_byte as usize);
        unsafe {
            copy_nonoverlapping(message.as_ptr(), z_err_msg.cast::<u8>(), copy_len);
            *z_err_msg.add(copy_len.saturating_sub(1)) = 0;
        }
    }

    unsafe extern "C" fn host_vfs_dl_sym(
        _: *mut sqlite3_vfs,
        _: *mut c_void,
        _: *const c_char,
    ) -> Sqlite3SyscallPtr {
        None
    }

    unsafe extern "C" fn host_vfs_dl_close(_: *mut sqlite3_vfs, _: *mut c_void) {}

    unsafe extern "C" fn host_vfs_randomness(
        _: *mut sqlite3_vfs,
        n_byte: c_int,
        out: *mut c_char,
    ) -> c_int {
        if out.is_null() || n_byte <= 0 {
            return 0;
        }
        let mut state = RANDOM_SEED.fetch_add(0xA076_1D64_78BD_642F, Ordering::Relaxed)
            ^ SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
        let bytes = unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), n_byte as usize) };
        for byte in bytes.iter_mut() {
            state ^= state << 7;
            state ^= state >> 9;
            state = state.rotate_left(11).wrapping_add(0x9E37_79B9);
            *byte = state as u8;
        }
        n_byte
    }

    unsafe extern "C" fn host_vfs_sleep(_: *mut sqlite3_vfs, microseconds: c_int) -> c_int {
        let micros = microseconds.max(0) as u64;
        std::thread::sleep(Duration::from_micros(micros));
        microseconds
    }

    unsafe extern "C" fn host_vfs_current_time(_: *mut sqlite3_vfs, out: *mut f64) -> c_int {
        if out.is_null() {
            return SQLITE_MISUSE;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let days = now.as_secs_f64() / 86_400.0;
        unsafe {
            *out = JULIAN_UNIX_EPOCH_DAYS + days;
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_vfs_get_last_error(
        _: *mut sqlite3_vfs,
        n_byte: c_int,
        out: *mut c_char,
    ) -> c_int {
        let code = LAST_OS_ERROR.load(Ordering::Relaxed);
        if !out.is_null() && n_byte > 0 {
            let text = format!("os error {code}");
            let bytes = text.as_bytes();
            let copy_len = bytes.len().min((n_byte - 1) as usize);
            unsafe {
                copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), copy_len);
                *out.add(copy_len) = 0;
            }
        }
        code
    }

    unsafe extern "C" fn host_file_close(file: *mut sqlite3_file) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        close_host_file(host_file);
        SQLITE_OK
    }

    unsafe extern "C" fn host_file_read(
        file: *mut sqlite3_file,
        buffer: *mut c_void,
        amount: c_int,
        offset: Sqlite3Int64,
    ) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        let Some(handle) = host_file.file.as_mut() else {
            return SQLITE_IOERR;
        };
        let Ok(offset_u64) = u64::try_from(offset) else {
            return SQLITE_IOERR;
        };
        let Ok(amount_usize) = usize::try_from(amount) else {
            return SQLITE_IOERR;
        };
        let bytes = unsafe { core::slice::from_raw_parts_mut(buffer.cast::<u8>(), amount_usize) };
        if let Err(err) = handle.seek(SeekFrom::Start(offset_u64)) {
            set_last_error_from_io(&err);
            return sqlite_error_from_io(&err, false);
        }
        let mut filled = 0usize;
        while filled < bytes.len() {
            match handle.read(&mut bytes[filled..]) {
                Ok(0) => {
                    bytes[filled..].fill(0);
                    return SQLITE_IOERR_SHORT_READ;
                }
                Ok(read) => filled += read,
                Err(err) => {
                    set_last_error_from_io(&err);
                    return sqlite_error_from_io(&err, false);
                }
            }
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_file_write(
        file: *mut sqlite3_file,
        buffer: *const c_void,
        amount: c_int,
        offset: Sqlite3Int64,
    ) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        let Some(handle) = host_file.file.as_mut() else {
            return SQLITE_IOERR;
        };
        let Ok(offset_u64) = u64::try_from(offset) else {
            return SQLITE_IOERR;
        };
        let Ok(amount_usize) = usize::try_from(amount) else {
            return SQLITE_IOERR;
        };
        let bytes = unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), amount_usize) };
        if let Err(err) = handle.seek(SeekFrom::Start(offset_u64)) {
            set_last_error_from_io(&err);
            return sqlite_error_from_io(&err, false);
        }
        if let Err(err) = handle.write_all(bytes) {
            set_last_error_from_io(&err);
            return sqlite_error_from_io(&err, false);
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_file_truncate(file: *mut sqlite3_file, size: Sqlite3Int64) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        let Some(handle) = host_file.file.as_mut() else {
            return SQLITE_IOERR;
        };
        let Ok(length) = u64::try_from(size) else {
            return SQLITE_IOERR;
        };
        match handle.set_len(length) {
            Ok(()) => SQLITE_OK,
            Err(err) => {
                set_last_error_from_io(&err);
                sqlite_error_from_io(&err, false)
            }
        }
    }

    unsafe extern "C" fn host_file_sync(file: *mut sqlite3_file, _: c_int) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        let Some(handle) = host_file.file.as_mut() else {
            return SQLITE_IOERR;
        };
        match handle.sync_all() {
            Ok(()) => SQLITE_OK,
            Err(err) => {
                set_last_error_from_io(&err);
                sqlite_error_from_io(&err, false)
            }
        }
    }

    unsafe extern "C" fn host_file_size(file: *mut sqlite3_file, out: *mut Sqlite3Int64) -> c_int {
        if out.is_null() {
            return SQLITE_MISUSE;
        }
        let host_file = unsafe { host_file_mut(file) };
        let Some(handle) = host_file.file.as_mut() else {
            return SQLITE_IOERR;
        };
        match handle.metadata() {
            Ok(meta) => {
                unsafe {
                    *out = meta.len() as Sqlite3Int64;
                }
                SQLITE_OK
            }
            Err(err) => {
                set_last_error_from_io(&err);
                sqlite_error_from_io(&err, false)
            }
        }
    }

    unsafe extern "C" fn host_file_lock(file: *mut sqlite3_file, level: c_int) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        promote_lock(host_file, level)
    }

    unsafe extern "C" fn host_file_unlock(file: *mut sqlite3_file, level: c_int) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        demote_lock(host_file, level)
    }

    unsafe extern "C" fn host_file_check_reserved_lock(
        file: *mut sqlite3_file,
        out: *mut c_int,
    ) -> c_int {
        if out.is_null() {
            return SQLITE_MISUSE;
        }
        let host_file = unsafe { host_file_mut(file) };
        let result = if let Some(path) = host_file.path.as_ref() {
            let locks = lock_table().lock().expect("lock table poisoned");
            locks.get(path).map_or(0, |record| {
                i32::from(
                    record.reserved.is_some()
                        || record.pending.is_some()
                        || record.exclusive.is_some(),
                )
            })
        } else {
            0
        };
        unsafe {
            *out = result;
        }
        SQLITE_OK
    }

    unsafe extern "C" fn host_file_control(
        file: *mut sqlite3_file,
        op: c_int,
        arg: *mut c_void,
    ) -> c_int {
        match op {
            SQLITE_FCNTL_LOCKSTATE => {
                if arg.is_null() {
                    return SQLITE_MISUSE;
                }
                let host_file = unsafe { host_file_mut(file) };
                unsafe {
                    *(arg.cast::<c_int>()) = host_file.lock_state;
                }
                SQLITE_OK
            }
            SQLITE_FCNTL_VFS_POINTER => {
                if arg.is_null() {
                    return SQLITE_MISUSE;
                }
                unsafe {
                    *(arg.cast::<*mut sqlite3_vfs>()) = &raw mut HOST_VFS;
                }
                SQLITE_OK
            }
            _ => SQLITE_NOTFOUND,
        }
    }

    unsafe extern "C" fn host_file_sector_size(file: *mut sqlite3_file) -> c_int {
        let host_file = unsafe { host_file_mut(file) };
        host_file.sector_size
    }

    unsafe extern "C" fn host_file_device_characteristics(_: *mut sqlite3_file) -> c_int {
        0
    }

    pub struct EchDb {
        handle: NonNull<sqlite3>,
    }

    impl EchDb {
        pub fn open_memory() -> Result<Self> {
            Self::open_inner(":memory:", false)
        }

        pub fn open_path(path: &str) -> Result<Self> {
            Self::open_inner(path, true)
        }

        fn open_inner(path: &str, use_vfs: bool) -> Result<Self> {
            ensure_sqlite_ready()?;
            let filename = normalize_open_filename(path, use_vfs)?;
            let mut db = null_mut();
            let rc = unsafe {
                sqlite3_open_v2(
                    filename.as_ptr(),
                    &mut db,
                    SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                    if use_vfs {
                        HOST_VFS_NAME.as_ptr().cast::<c_char>()
                    } else {
                        null()
                    },
                )
            };
            let Some(handle) = NonNull::new(db) else {
                return Err(EchDbError::Open {
                    code: rc,
                    message: String::from("sqlite returned a null database handle"),
                });
            };
            if rc != SQLITE_OK {
                let message = error_message(handle.as_ptr(), null_mut());
                unsafe {
                    sqlite3_close(handle.as_ptr());
                }
                return Err(EchDbError::Open { code: rc, message });
            }
            Ok(Self { handle })
        }

        pub fn execute_batch(&self, sql: &str) -> Result<()> {
            let sql = CString::new(sql).map_err(|_| EchDbError::InteriorNul)?;
            let mut errmsg = null_mut();
            let rc = unsafe {
                sqlite3_exec(
                    self.handle.as_ptr(),
                    sql.as_ptr(),
                    None,
                    null_mut(),
                    &mut errmsg,
                )
            };
            if rc != SQLITE_OK {
                return Err(EchDbError::Execute {
                    code: rc,
                    message: error_message(self.handle.as_ptr(), errmsg),
                });
            }
            Ok(())
        }

        pub fn query_rows(&self, sql: &str) -> Result<Vec<Vec<String>>> {
            let sql = CString::new(sql).map_err(|_| EchDbError::InteriorNul)?;
            let mut rows = Vec::<Vec<String>>::new();
            let mut errmsg = null_mut();
            let rc = unsafe {
                sqlite3_exec(
                    self.handle.as_ptr(),
                    sql.as_ptr(),
                    Some(collect_row),
                    &mut rows as *mut Vec<Vec<String>> as *mut c_void,
                    &mut errmsg,
                )
            };
            if rc != SQLITE_OK {
                return Err(EchDbError::Execute {
                    code: rc,
                    message: error_message(self.handle.as_ptr(), errmsg),
                });
            }
            Ok(rows)
        }
    }

    unsafe extern "C" fn collect_row(
        arg: *mut c_void,
        columns: c_int,
        values: *mut *mut c_char,
        _: *mut *mut c_char,
    ) -> c_int {
        let rows = unsafe { &mut *(arg.cast::<Vec<Vec<String>>>()) };
        let mut row = Vec::with_capacity(columns.max(0) as usize);
        for index in 0..columns {
            let value = unsafe { *values.add(index as usize) };
            if value.is_null() {
                row.push(String::new());
            } else {
                row.push(
                    unsafe { CStr::from_ptr(value) }
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        rows.push(row);
        SQLITE_OK
    }

    impl Drop for EchDb {
        fn drop(&mut self) {
            unsafe {
                sqlite3_close(self.handle.as_ptr());
            }
        }
    }

    #[cfg(test)]
    fn reset_vfs_open_count() {
        VFS_OPEN_CALLS.store(0, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn vfs_open_count() -> usize {
        VFS_OPEN_CALLS.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    mod tests {
        use super::{reset_vfs_open_count, vfs_open_count, EchDb};
        use alloc::{format, string::String, vec};
        use std::env;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn unique_path(prefix: &str) -> String {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            env::temp_dir()
                .join(format!("{prefix}-{stamp}.db"))
                .to_string_lossy()
                .into_owned()
        }

        #[test]
        fn sqlite_memory_round_trip_works() {
            let db = EchDb::open_memory().unwrap();
            db.execute_batch(
                "create table items(id integer primary key, name text, qty integer);
                 insert into items(name, qty) values('alpha', 3), ('beta', 7);",
            )
            .unwrap();
            let rows = db
                .query_rows("select name, qty from items order by id;")
                .unwrap();
            assert_eq!(
                rows,
                vec![
                    vec![String::from("alpha"), String::from("3")],
                    vec![String::from("beta"), String::from("7")]
                ]
            );
        }

        #[test]
        fn sqlite_file_backed_reopen_preserves_rows() {
            let path = unique_path("echos-sqlite");
            {
                let db = EchDb::open_path(&path).unwrap();
                db.execute_batch(
                    "create table notes(body text);
                     insert into notes(body) values('persisted row');",
                )
                .unwrap();
            }
            let db = EchDb::open_path(&path).unwrap();
            let rows = db.query_rows("select body from notes;").unwrap();
            assert_eq!(rows, vec![vec![String::from("persisted row")]]);
            let _ = fs::remove_file(path);
        }

        #[test]
        fn sqlite_file_backed_paths_flow_through_echos_vfs() {
            let path = unique_path("echos-vfs-proof");
            reset_vfs_open_count();
            {
                let db = EchDb::open_path(&path).unwrap();
                db.execute_batch(
                    "create table proof(value text);
                     insert into proof(value) values('vfs');",
                )
                .unwrap();
            }
            assert!(
                vfs_open_count() >= 1,
                "custom sqlite VFS never received xOpen"
            );
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
pub use host::EchDb;

#[cfg(any(target_os = "none", target_os = "uefi"))]
pub struct EchDb;

#[cfg(any(target_os = "none", target_os = "uefi"))]
impl EchDb {
    pub fn open_memory() -> Result<Self> {
        Err(EchDbError::EngineUnavailable(
            "ech-db host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn open_path(_: &str) -> Result<Self> {
        Err(EchDbError::EngineUnavailable(
            "ech-db host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn execute_batch(&self, _: &str) -> Result<()> {
        Err(EchDbError::EngineUnavailable(
            "ech-db host bring-up currently targets non-UEFI host builds only",
        ))
    }

    pub fn query_rows(&self, _: &str) -> Result<Vec<Vec<String>>> {
        Err(EchDbError::EngineUnavailable(
            "ech-db host bring-up currently targets non-UEFI host builds only",
        ))
    }
}
