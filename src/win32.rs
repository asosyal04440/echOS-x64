//! # echOS Win32 API Emulation
//!
//! Windows API emulation layer for running Windows binaries
//! Implements common Win32 APIs: kernel32, user32, gdi32

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// WIN32 TYPES
// ============================================================================

pub type HANDLE = u64;
pub type HMODULE = u64;
pub type HWND = u64;
pub type HDC = u64;
pub type HINSTANCE = u64;
pub type HMENU = u64;
pub type HICON = u64;
pub type HCURSOR = u64;
pub type HBRUSH = u64;
pub type HFONT = u64;
pub type HBITMAP = u64;
pub type HGDIOBJ = u64;
pub type HPEN = u64;
pub type HPALETTE = u64;
pub type HRGN = u64;
pub type HKEY = u64;
pub type SC_HANDLE = u64;
pub type HCRYPTPROV = u64;
pub type HCRYPTKEY = u64;
pub type HCRYPTHASH = u64;
pub type HDROP = u64;
pub type HRESULT = i32;
pub type DWORD_PTR = u64;

pub type LPSTR = *mut i8;
pub type LPCSTR = *const i8;
pub type LPWSTR = *mut u16;
pub type LPCWSTR = *const u16;
pub type LPVOID = *mut u8;
pub type LPCVOID = *const u8;

pub type DWORD = u32;
pub type WORD = u16;
pub type BYTE = u8;
pub type BOOL = i32;
pub type UINT = u32;
pub type INT = i32;
pub type LONG = i32;
pub type ULONG = u32;
pub type SIZE_T = u64;
pub type SHORT = i16;

pub type FARPROC = Option<unsafe extern "system" fn() -> isize>;
pub type PROC = Option<unsafe extern "system" fn() -> isize>;

// ============================================================================
// WIN32 CONSTANTS
// ============================================================================

pub const INVALID_HANDLE_VALUE: HANDLE = !0;
pub const NULL: HANDLE = 0;
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// Memory constants
pub const PAGE_NOACCESS: DWORD = 0x01;
pub const PAGE_READONLY: DWORD = 0x02;
pub const PAGE_READWRITE: DWORD = 0x04;
pub const PAGE_EXECUTE: DWORD = 0x10;
pub const PAGE_EXECUTE_READ: DWORD = 0x20;
pub const PAGE_EXECUTE_READWRITE: DWORD = 0x40;

// File constants
pub const GENERIC_READ: DWORD = 0x80000000;
pub const GENERIC_WRITE: DWORD = 0x40000000;
pub const FILE_SHARE_READ: DWORD = 0x00000001;
pub const FILE_SHARE_WRITE: DWORD = 0x00000002;
pub const OPEN_EXISTING: DWORD = 3;
pub const CREATE_NEW: DWORD = 1;
pub const CREATE_ALWAYS: DWORD = 2;
pub const FILE_ATTRIBUTE_NORMAL: DWORD = 0x80;

// Window styles
pub const WS_OVERLAPPED: DWORD = 0x00000000;
pub const WS_CAPTION: DWORD = 0x00C00000;
pub const WS_SYSMENU: DWORD = 0x00080000;
pub const WS_THICKFRAME: DWORD = 0x00040000;
pub const WS_MINIMIZEBOX: DWORD = 0x00020000;
pub const WS_MAXIMIZEBOX: DWORD = 0x00010000;
pub const WS_OVERLAPPEDWINDOW: DWORD = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
pub const WS_VISIBLE: DWORD = 0x10000000;
pub const WS_CHILD: DWORD = 0x40000000;

// Show window commands
pub const SW_HIDE: INT = 0;
pub const SW_SHOWNORMAL: INT = 1;
pub const SW_SHOWMINIMIZED: INT = 2;
pub const SW_SHOWMAXIMIZED: INT = 3;
pub const SW_SHOW: INT = 5;

// Message constants
pub const WM_NULL: UINT = 0x0000;
pub const WM_CREATE: UINT = 0x0001;
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_QUIT: UINT = 0x0012;
pub const WM_PAINT: UINT = 0x000F;
pub const WM_SIZE: UINT = 0x0005;
pub const WM_MOUSEMOVE: UINT = 0x0200;
pub const WM_LBUTTONDOWN: UINT = 0x0201;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_KEYDOWN: UINT = 0x0100;
pub const WM_KEYUP: UINT = 0x0101;
pub const WM_CHAR: UINT = 0x0102;

// Virtual key codes
pub const VK_ESCAPE: INT = 0x1B;
pub const VK_RETURN: INT = 0x0D;
pub const VK_SPACE: INT = 0x20;
pub const VK_LEFT: INT = 0x25;
pub const VK_UP: INT = 0x26;
pub const VK_RIGHT: INT = 0x27;
pub const VK_DOWN: INT = 0x28;

// ============================================================================
// WIN32 STRUCTURES
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct POINT {
    pub x: LONG,
    pub y: LONG,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SIZE {
    pub cx: LONG,
    pub cy: LONG,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RECT {
    pub left: LONG,
    pub top: LONG,
    pub right: LONG,
    pub bottom: LONG,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct WNDCLASSA {
    pub style: UINT,
    pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
    pub cbClsExtra: INT,
    pub cbWndExtra: INT,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: LPCSTR,
    pub lpszClassName: LPCSTR,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct WNDCLASSEXA {
    pub cbSize: UINT,
    pub style: UINT,
    pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
    pub cbClsExtra: INT,
    pub cbWndExtra: INT,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: LPCSTR,
    pub lpszClassName: LPCSTR,
    pub hIconSm: HICON,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: usize,
    pub lParam: isize,
    pub time: DWORD,
    pub pt: POINT,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct PIXELFORMATDESCRIPTOR {
    pub nSize: WORD,
    pub nVersion: WORD,
    pub dwFlags: DWORD,
    pub iPixelType: BYTE,
    pub cColorBits: BYTE,
    pub cRedBits: BYTE,
    pub cRedShift: BYTE,
    pub cGreenBits: BYTE,
    pub cGreenShift: BYTE,
    pub cBlueBits: BYTE,
    pub cBlueShift: BYTE,
    pub cAlphaBits: BYTE,
    pub cAlphaShift: BYTE,
    pub cAccumBits: BYTE,
    pub cAccumRedBits: BYTE,
    pub cAccumGreenBits: BYTE,
    pub cAccumBlueBits: BYTE,
    pub cAccumAlphaBits: BYTE,
    pub cDepthBits: BYTE,
    pub cStencilBits: BYTE,
    pub cAuxBuffers: BYTE,
    pub iLayerType: BYTE,
    pub bReserved: BYTE,
    pub dwLayerMask: DWORD,
    pub dwVisibleMask: DWORD,
    pub dwDamageMask: DWORD,
}

// ADVAPI32 structures
#[repr(C)]
pub struct SERVICE_STATUS {
    pub dwServiceType: DWORD,
    pub dwCurrentState: DWORD,
    pub dwControlsAccepted: DWORD,
    pub dwWin32ExitCode: DWORD,
    pub dwServiceSpecificExitCode: DWORD,
    pub dwCheckPoint: DWORD,
    pub dwWaitHint: DWORD,
}

// SHELL32 structures
#[repr(C)]
pub struct SHELLEXECUTEINFOA {
    pub cbSize: DWORD,
    pub fMask: ULONG,
    pub hwnd: HWND,
    pub lpVerb: LPCSTR,
    pub lpFile: LPCSTR,
    pub lpParameters: LPCSTR,
    pub lpDirectory: LPCSTR,
    pub nShow: INT,
    pub hInstApp: HINSTANCE,
    pub lpIDList: LPVOID,
    pub lpClass: LPCSTR,
    pub hkeyClass: HKEY,
    pub dwHotKey: DWORD,
    pub hIcon: HICON,
    pub hProcess: HANDLE,
}

#[repr(C)]
pub struct BROWSEINFOA {
    pub hwndOwner: HWND,
    pub pidlRoot: LPCSTR,
    pub pszDisplayName: LPSTR,
    pub lpszTitle: LPCSTR,
    pub ulFlags: ULONG,
    pub lpfn: Option<unsafe extern "system" fn(HWND, UINT, LPARAM, LPARAM) -> INT>,
    pub lParam: LPARAM,
    pub iImage: INT,
}

#[repr(C)]
pub struct NOTIFYICONDATAA {
    pub cbSize: DWORD,
    pub hWnd: HWND,
    pub uID: UINT,
    pub uFlags: UINT,
    pub uCallbackMessage: UINT,
    pub hIcon: HICON,
    pub szTip: [i8; 128],
    pub dwState: DWORD,
    pub dwStateMask: DWORD,
    pub szInfo: [i8; 256],
    pub uTimeout: UINT,
    pub szInfoTitle: [i8; 64],
    pub dwInfoFlags: DWORD,
}

#[repr(C)]
pub struct SHFILEINFOA {
    pub hIcon: HICON,
    pub iIcon: INT,
    pub dwAttributes: DWORD,
    pub szDisplayName: [i8; 260],
    pub szTypeName: [i8; 80],
}

#[repr(C)]
pub struct SHFILEOPSTRUCTA {
    pub hwnd: HWND,
    pub wFunc: UINT,
    pub pFrom: LPCSTR,
    pub pTo: LPCSTR,
    pub fFlags: WORD,
    pub fAnyOperationsAborted: BOOL,
    pub hNameMappings: LPVOID,
    pub lpszProgressTitle: LPCSTR,
}

// MSVCRT types
pub type time_t = i64;
pub type clock_t = i64;
pub type LPARAM = isize;

#[repr(C)]
pub struct FILE {
    _ptr: *mut u8,
    _cnt: INT,
    _base: *mut u8,
    _flag: INT,
    _file: INT,
    _bufsiz: INT,
}

#[repr(C)]
pub struct tm {
    pub tm_sec: INT,
    pub tm_min: INT,
    pub tm_hour: INT,
    pub tm_mday: INT,
    pub tm_mon: INT,
    pub tm_year: INT,
    pub tm_wday: INT,
    pub tm_yday: INT,
    pub tm_isdst: INT,
}

// ============================================================================
// WIN32 API TABLE
// ============================================================================

/// Win32 API function signature
type Win32ApiFn = fn(*const u8) -> isize;

/// Win32 API entry
struct Win32ApiEntry {
    name: String,
    func: Win32ApiFn,
}

/// Win32 API module
struct Win32Module {
    name: String,
    functions: BTreeMap<String, Win32ApiFn>,
}

// ============================================================================
// KERNEL32 IMPLEMENTATION
// ============================================================================

mod kernel32 {
    use super::*;
    
    /// GetModuleHandleA
    pub unsafe fn get_module_handle_a(lpModuleName: LPCSTR) -> HMODULE {
        // Return fake handle
        0x00400000
    }
    
    /// LoadLibraryA
    pub unsafe fn load_library_a(lpLibFileName: LPCSTR) -> HMODULE {
        // TODO: Load actual DLL
        0
    }
    
    /// GetProcAddress
    pub unsafe fn get_proc_address(hModule: HMODULE, lpProcName: LPCSTR) -> FARPROC {
        if lpProcName.is_null() {
            return None;
        }
        
        // Get function name
        let mut name = String::new();
        let mut ptr = lpProcName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        
        // Look up in API table
        crate::win32::get_proc_address_internal("kernel32", &name)
    }
    
    /// VirtualAlloc
    pub unsafe fn virtual_alloc(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        flAllocationType: DWORD,
        flProtect: DWORD,
    ) -> LPVOID {
        // Allocate memory (stub)
        let size = dwSize as usize;
        // TODO: Use proper memory allocation
        core::ptr::null_mut()
    }
    
    /// VirtualFree
    pub unsafe fn virtual_free(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        dwFreeType: DWORD,
    ) -> BOOL {
        // Free memory (stub)
        TRUE
    }
    
    /// GetTickCount
    pub unsafe fn get_tick_count() -> DWORD {
        // Return tick count (stub)
        0
    }
    
    /// Sleep
    pub unsafe fn sleep(dwMilliseconds: DWORD) {
        // Simple delay loop (stub)
        for _ in 0..dwMilliseconds * 1000 {
            core::hint::spin_loop();
        }
    }
    
    /// CreateFileA
    pub unsafe fn create_file_a(
        lpFileName: LPCSTR,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: LPVOID,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE {
        // Get filename
        let mut name = String::new();
        let mut ptr = lpFileName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        
        // TODO: Open file via filesystem
        crate::serial_println!("[WIN32] CreateFileA: {}", name);
        0x00000001 as HANDLE // Fake handle
    }
    
    /// ReadFile
    pub unsafe fn read_file(
        hFile: HANDLE,
        lpBuffer: LPVOID,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        // TODO: Read from file
        if !lpNumberOfBytesRead.is_null() {
            *lpNumberOfBytesRead = 0;
        }
        TRUE
    }
    
    /// WriteFile
    pub unsafe fn write_file(
        hFile: HANDLE,
        lpBuffer: LPCVOID,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        // TODO: Write to file
        if !lpNumberOfBytesWritten.is_null() {
            *lpNumberOfBytesWritten = nNumberOfBytesToWrite;
        }
        TRUE
    }
    
    /// CloseHandle
    pub unsafe fn close_handle(hObject: HANDLE) -> BOOL {
        // TODO: Close handle
        TRUE
    }
    
    /// ExitProcess
    pub unsafe fn exit_process(uExitCode: UINT) {
        crate::serial_println!("[WIN32] ExitProcess({})", uExitCode);
        // TODO: Terminate process
        loop {}
    }
    
    // ========================================================================
    // PROCESS MANAGEMENT
    // ========================================================================
    
    /// CreateProcessA
    pub unsafe fn create_process_a(
        lpApplicationName: LPCSTR,
        lpCommandLine: LPSTR,
        lpProcessAttributes: LPVOID,
        lpThreadAttributes: LPVOID,
        bInheritHandles: BOOL,
        dwCreationFlags: DWORD,
        lpEnvironment: LPVOID,
        lpCurrentDirectory: LPCSTR,
        lpStartupInfo: LPVOID,
        lpProcessInformation: LPVOID,
    ) -> BOOL {
        let mut name = String::new();
        let mut ptr = lpApplicationName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] CreateProcessA: {}", name);
        TRUE
    }
    
    /// OpenProcess
    pub unsafe fn open_process(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE {
        crate::serial_println!("[WIN32] OpenProcess: pid={}", dwProcessId);
        dwProcessId as HANDLE
    }
    
    /// TerminateProcess
    pub unsafe fn terminate_process(hProcess: HANDLE, uExitCode: UINT) -> BOOL {
        crate::serial_println!("[WIN32] TerminateProcess: handle={}", hProcess);
        TRUE
    }
    
    /// GetExitCodeProcess
    pub unsafe fn get_exit_code_process(hProcess: HANDLE, lpExitCode: *mut DWORD) -> BOOL {
        if !lpExitCode.is_null() {
            *lpExitCode = 0;
        }
        TRUE
    }
    
    /// GetCurrentProcess
    pub unsafe fn get_current_process() -> HANDLE {
        0xFFFFFFFFFFFFFFFF
    }
    
    /// GetCurrentProcessId
    pub unsafe fn get_current_process_id() -> DWORD {
        crate::task::scheduler::current_task_id() as DWORD
    }
    
    // ========================================================================
    // THREAD MANAGEMENT
    // ========================================================================
    
    /// CreateThread
    pub unsafe fn create_thread(
        lpThreadAttributes: LPVOID,
        dwStackSize: SIZE_T,
        lpStartAddress: LPVOID,
        lpParameter: LPVOID,
        dwCreationFlags: DWORD,
        lpThreadId: *mut DWORD,
    ) -> HANDLE {
        crate::serial_println!("[WIN32] CreateThread");
        if !lpThreadId.is_null() {
            *lpThreadId = 1;
        }
        1 as HANDLE
    }
    
    /// ExitThread
    pub unsafe fn exit_thread(dwExitCode: DWORD) {
        crate::serial_println!("[WIN32] ExitThread({})", dwExitCode);
    }
    
    /// GetCurrentThread
    pub unsafe fn get_current_thread() -> HANDLE {
        0xFFFFFFFFFFFFFFFE
    }
    
    /// GetCurrentThreadId
    pub unsafe fn get_current_thread_id() -> DWORD {
        crate::task::scheduler::current_task_id() as DWORD
    }
    
    /// ResumeThread
    pub unsafe fn resume_thread(hThread: HANDLE) -> DWORD {
        0
    }
    
    /// SuspendThread
    pub unsafe fn suspend_thread(hThread: HANDLE) -> DWORD {
        0
    }
    
    /// WaitForSingleObject
    pub unsafe fn wait_for_single_object(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD {
        0 // WAIT_OBJECT_0
    }
    
    /// WaitForMultipleObjects
    pub unsafe fn wait_for_multiple_objects(
        nCount: DWORD,
        lpHandles: *const HANDLE,
        bWaitAll: BOOL,
        dwMilliseconds: DWORD,
    ) -> DWORD {
        0
    }
    
    // ========================================================================
    // MEMORY MANAGEMENT
    // ========================================================================
    
    /// VirtualProtect
    pub unsafe fn virtual_protect(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        flNewProtect: DWORD,
        lpflOldProtect: *mut DWORD,
    ) -> BOOL {
        if !lpflOldProtect.is_null() {
            *lpflOldProtect = PAGE_READWRITE;
        }
        TRUE
    }
    
    /// VirtualQuery
    pub unsafe fn virtual_query(
        lpAddress: LPCVOID,
        lpBuffer: LPVOID,
        dwLength: SIZE_T,
    ) -> SIZE_T {
        0
    }
    
    /// HeapCreate
    pub unsafe fn heap_create(flOptions: DWORD, dwInitialSize: SIZE_T, dwMaximumSize: SIZE_T) -> HANDLE {
        crate::serial_println!("[WIN32] HeapCreate: size={}", dwInitialSize);
        1 as HANDLE
    }
    
    /// HeapDestroy
    pub unsafe fn heap_destroy(hHeap: HANDLE) -> BOOL {
        TRUE
    }
    
    /// HeapAlloc
    pub unsafe fn heap_alloc(hHeap: HANDLE, dwFlags: DWORD, dwBytes: SIZE_T) -> LPVOID {
        // TODO: Real allocation
        crate::serial_println!("[WIN32] HeapAlloc: {} bytes", dwBytes);
        core::ptr::null_mut()
    }
    
    /// HeapFree
    pub unsafe fn heap_free(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPVOID) -> BOOL {
        TRUE
    }
    
    /// HeapReAlloc
    pub unsafe fn heap_realloc(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPVOID, dwBytes: SIZE_T) -> LPVOID {
        core::ptr::null_mut()
    }
    
    /// HeapSize
    pub unsafe fn heap_size(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPCVOID) -> SIZE_T {
        0
    }
    
    /// GetProcessHeap
    pub unsafe fn get_process_heap() -> HANDLE {
        1 as HANDLE
    }
    
    /// LocalAlloc
    pub unsafe fn local_alloc(uFlags: DWORD, uBytes: SIZE_T) -> HANDLE {
        uBytes as HANDLE
    }
    
    /// LocalFree
    pub unsafe fn local_free(hMem: HANDLE) -> HANDLE {
        0
    }
    
    /// GlobalAlloc
    pub unsafe fn global_alloc(uFlags: DWORD, dwBytes: SIZE_T) -> HANDLE {
        dwBytes as HANDLE
    }
    
    /// GlobalFree
    pub unsafe fn global_free(hMem: HANDLE) -> HANDLE {
        0
    }
    
    // ========================================================================
    // FILE MANAGEMENT
    // ========================================================================
    
    /// SetFilePointer
    pub unsafe fn set_file_pointer(
        hFile: HANDLE,
        lDistanceToMove: LONG,
        lpDistanceToMoveHigh: *mut LONG,
        dwMoveMethod: DWORD,
    ) -> DWORD {
        0
    }
    
    /// SetFilePointerEx
    pub unsafe fn set_file_pointer_ex(
        hFile: HANDLE,
        liDistanceToMove: i64,
        lpNewFilePointer: *mut i64,
        dwMoveMethod: DWORD,
    ) -> BOOL {
        if !lpNewFilePointer.is_null() {
            *lpNewFilePointer = 0;
        }
        TRUE
    }
    
    /// GetFileSize
    pub unsafe fn get_file_size(hFile: HANDLE, lpFileSizeHigh: *mut DWORD) -> DWORD {
        0
    }
    
    /// GetFileSizeEx
    pub unsafe fn get_file_size_ex(hFile: HANDLE, lpFileSize: *mut i64) -> BOOL {
        if !lpFileSize.is_null() {
            *lpFileSize = 0;
        }
        TRUE
    }
    
    /// GetFileAttributesA
    pub unsafe fn get_file_attributes_a(lpFileName: LPCSTR) -> DWORD {
        let mut name = String::new();
        let mut ptr = lpFileName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] GetFileAttributesA: {}", name);
        FILE_ATTRIBUTE_NORMAL
    }
    
    /// SetFileAttributesA
    pub unsafe fn set_file_attributes_a(lpFileName: LPCSTR, dwFileAttributes: DWORD) -> BOOL {
        TRUE
    }
    
    /// DeleteFileA
    pub unsafe fn delete_file_a(lpFileName: LPCSTR) -> BOOL {
        let mut name = String::new();
        let mut ptr = lpFileName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] DeleteFileA: {}", name);
        TRUE
    }
    
    /// MoveFileA
    pub unsafe fn move_file_a(lpExistingFileName: LPCSTR, lpNewFileName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// CopyFileA
    pub unsafe fn copy_file_a(lpExistingFileName: LPCSTR, lpNewFileName: LPCSTR, bFailIfExists: BOOL) -> BOOL {
        TRUE
    }
    
    /// FindFirstFileA
    pub unsafe fn find_first_file_a(lpFileName: LPCSTR, lpFindFileData: LPVOID) -> HANDLE {
        INVALID_HANDLE_VALUE
    }
    
    /// FindNextFileA
    pub unsafe fn find_next_file_a(hFindFile: HANDLE, lpFindFileData: LPVOID) -> BOOL {
        FALSE
    }
    
    /// FindClose
    pub unsafe fn find_close(hFindFile: HANDLE) -> BOOL {
        TRUE
    }
    
    /// CreateDirectoryA
    pub unsafe fn create_directory_a(lpPathName: LPCSTR, lpSecurityAttributes: LPVOID) -> BOOL {
        let mut name = String::new();
        let mut ptr = lpPathName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] CreateDirectoryA: {}", name);
        TRUE
    }
    
    /// RemoveDirectoryA
    pub unsafe fn remove_directory_a(lpPathName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// GetCurrentDirectoryA
    pub unsafe fn get_current_directory_a(nBufferLength: DWORD, lpBuffer: LPSTR) -> DWORD {
        if !lpBuffer.is_null() && nBufferLength >= 2 {
            *lpBuffer = '\\' as i8;
            *((lpBuffer as *mut u8).add(1)) = 0;
        }
        2
    }
    
    /// SetCurrentDirectoryA
    pub unsafe fn set_current_directory_a(lpPathName: LPCSTR) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // ENVIRONMENT
    // ========================================================================
    
    /// GetEnvironmentVariableA
    pub unsafe fn get_environment_variable_a(lpName: LPCSTR, lpBuffer: LPSTR, nSize: DWORD) -> DWORD {
        0
    }
    
    /// SetEnvironmentVariableA
    pub unsafe fn set_environment_variable_a(lpName: LPCSTR, lpValue: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// GetCommandLineA
    pub unsafe fn get_command_line_a() -> LPSTR {
        core::ptr::null_mut()
    }
    
    // ========================================================================
    // CONSOLE
    // ========================================================================
    
    /// GetStdHandle
    pub unsafe fn get_std_handle(nStdHandle: DWORD) -> HANDLE {
        nStdHandle as HANDLE
    }
    
    /// SetStdHandle
    pub unsafe fn set_std_handle(nStdHandle: DWORD, hHandle: HANDLE) -> BOOL {
        TRUE
    }
    
    /// WriteConsoleA
    pub unsafe fn write_console_a(
        hConsoleOutput: HANDLE,
        lpBuffer: *const u8,
        nNumberOfCharsToWrite: DWORD,
        lpNumberOfCharsWritten: *mut DWORD,
        lpReserved: LPVOID,
    ) -> BOOL {
        // Write to serial output
        for i in 0..nNumberOfCharsToWrite {
            crate::serial_print!("{}", *lpBuffer.add(i as usize) as char);
        }
        if !lpNumberOfCharsWritten.is_null() {
            *lpNumberOfCharsWritten = nNumberOfCharsToWrite;
        }
        TRUE
    }
    
    /// ReadConsoleA
    pub unsafe fn read_console_a(
        hConsoleInput: HANDLE,
        lpBuffer: LPSTR,
        nNumberOfCharsToRead: DWORD,
        lpNumberOfCharsRead: *mut DWORD,
        pInputControl: LPVOID,
    ) -> BOOL {
        if !lpNumberOfCharsRead.is_null() {
            *lpNumberOfCharsRead = 0;
        }
        TRUE
    }
    
    /// SetConsoleMode
    pub unsafe fn set_console_mode(hConsoleHandle: HANDLE, dwMode: DWORD) -> BOOL {
        TRUE
    }
    
    /// GetConsoleMode
    pub unsafe fn get_console_mode(hConsoleHandle: HANDLE, lpMode: *mut DWORD) -> BOOL {
        if !lpMode.is_null() {
            *lpMode = 0;
        }
        TRUE
    }
    
    /// SetConsoleTextAttribute
    pub unsafe fn set_console_text_attribute(hConsoleOutput: HANDLE, wAttributes: WORD) -> BOOL {
        TRUE
    }
    
    /// GetConsoleScreenBufferInfo
    pub unsafe fn get_console_screen_buffer_info(hConsoleOutput: HANDLE, lpConsoleScreenBufferInfo: LPVOID) -> BOOL {
        TRUE
    }
    
    /// FillConsoleOutputCharacterA
    pub unsafe fn fill_console_output_character_a(
        hConsoleOutput: HANDLE,
        cCharacter: i8,
        nLength: DWORD,
        dwWriteCoord: DWORD,
        lpNumberOfCharsWritten: *mut DWORD,
    ) -> BOOL {
        if !lpNumberOfCharsWritten.is_null() {
            *lpNumberOfCharsWritten = nLength;
        }
        TRUE
    }
    
    // ========================================================================
    // SYSTEM INFO
    // ========================================================================
    
    /// GetSystemInfo
    pub unsafe fn get_system_info(lpSystemInfo: LPVOID) {
        // Fill SYSTEM_INFO structure
    }
    
    /// GlobalMemoryStatus
    pub unsafe fn global_memory_status(lpMemoryStatus: LPVOID) {
        // Fill MEMORYSTATUS structure
    }
    
    /// GlobalMemoryStatusEx
    pub unsafe fn global_memory_status_ex(lpMemoryStatus: LPVOID) -> BOOL {
        TRUE
    }
    
    /// GetVersion
    pub unsafe fn get_version() -> DWORD {
        // Windows 10 version
        0x000A0000
    }
    
    /// GetVersionExA
    pub unsafe fn get_version_ex_a(lpVersionInfo: LPVOID) -> BOOL {
        TRUE
    }
    
    /// GetComputerNameA
    pub unsafe fn get_computer_name_a(lpBuffer: LPSTR, lpnSize: *mut DWORD) -> BOOL {
        if !lpBuffer.is_null() && !lpnSize.is_null() {
            let name = b"echOS\0";
            let len = core::cmp::min(*lpnSize as usize, name.len());
            core::ptr::copy_nonoverlapping(name.as_ptr(), lpBuffer as *mut u8, len);
            *lpnSize = (len - 1) as DWORD;
        }
        TRUE
    }
    
    /// GetUserNameA
    pub unsafe fn get_user_name_a(lpBuffer: LPSTR, lpnSize: *mut DWORD) -> BOOL {
        if !lpBuffer.is_null() && !lpnSize.is_null() {
            let name = b"user\0";
            let len = core::cmp::min(*lpnSize as usize, name.len());
            core::ptr::copy_nonoverlapping(name.as_ptr(), lpBuffer as *mut u8, len);
            *lpnSize = (len - 1) as DWORD;
        }
        TRUE
    }
    
    /// GetLastError
    pub unsafe fn get_last_error() -> DWORD {
        0
    }
    
    /// SetLastError
    pub unsafe fn set_last_error(dwErrCode: DWORD) {
        let _ = dwErrCode;
    }
    
    /// MultiByteToWideChar
    pub unsafe fn multi_byte_to_wide_char(
        CodePage: DWORD,
        dwFlags: DWORD,
        lpMultiByteStr: LPCSTR,
        cbMultiByte: INT,
        lpWideCharStr: LPWSTR,
        cchWideChar: INT,
    ) -> INT {
        0
    }
    
    /// WideCharToMultiByte
    pub unsafe fn wide_char_to_multi_byte(
        CodePage: DWORD,
        dwFlags: DWORD,
        lpWideCharStr: LPCWSTR,
        cchWideChar: INT,
        lpMultiByteStr: LPSTR,
        cbMultiByte: INT,
        lpDefaultChar: LPCSTR,
        lpUsedDefaultChar: *mut BOOL,
    ) -> INT {
        0
    }
    
    /// lstrlenA
    pub unsafe fn lstrlen_a(lpString: LPCSTR) -> INT {
        let mut len = 0;
        let mut ptr = lpString;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
    
    /// lstrlenW
    pub unsafe fn lstrlen_w(lpString: LPCWSTR) -> INT {
        let mut len = 0;
        let mut ptr = lpString;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
    
    /// lstrcpyA
    pub unsafe fn lstrcpy_a(lpString1: LPSTR, lpString2: LPCSTR) -> LPSTR {
        lpString1
    }
    
    /// lstrcatA
    pub unsafe fn lstrcat_a(lpString1: LPSTR, lpString2: LPCSTR) -> LPSTR {
        lpString1
    }
    
    /// CompareStringA
    pub unsafe fn compare_string_a(
        Locale: DWORD,
        dwCmpFlags: DWORD,
        lpString1: LPCSTR,
        cchCount1: INT,
        lpString2: LPCSTR,
        cchCount2: INT,
    ) -> INT {
        2 // CSTR_EQUAL
    }
    
    /// lstrcmpA
    pub unsafe fn lstrcmp_a(lpString1: LPCSTR, lpString2: LPCSTR) -> INT {
        0
    }
    
    /// lstrcmpiA
    pub unsafe fn lstrcmpi_a(lpString1: LPCSTR, lpString2: LPCSTR) -> INT {
        0
    }
}

// ============================================================================
// USER32 IMPLEMENTATION
// ============================================================================

mod user32 {
    use super::*;
    
    /// RegisterClassA
    pub unsafe fn register_class_a(lpWndClass: *const WNDCLASSA) -> WORD {
        // Return fake atom
        0x0001
    }
    
    /// CreateWindowExA
    pub unsafe fn create_window_ex_a(
        dwExStyle: DWORD,
        lpClassName: LPCSTR,
        lpWindowName: LPCSTR,
        dwStyle: DWORD,
        x: INT,
        y: INT,
        nWidth: INT,
        nHeight: INT,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: LPVOID,
    ) -> HWND {
        // TODO: Create window in GUI system
        crate::serial_println!("[WIN32] CreateWindowExA: {},{} {},{}", x, y, nWidth, nHeight);
        0x00000001 as HWND // Fake window handle
    }
    
    /// ShowWindow
    pub unsafe fn show_window(hWnd: HWND, nCmdShow: INT) -> BOOL {
        crate::serial_println!("[WIN32] ShowWindow: {}", nCmdShow);
        TRUE
    }
    
    /// UpdateWindow
    pub unsafe fn update_window(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// GetMessageA
    pub unsafe fn get_message_a(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: UINT,
        wMsgFilterMax: UINT,
    ) -> BOOL {
        // TODO: Wait for message
        (*lpMsg).hwnd = hWnd;
        (*lpMsg).message = WM_QUIT;
        (*lpMsg).wParam = 0;
        (*lpMsg).lParam = 0;
        FALSE
    }
    
    /// TranslateMessage
    pub unsafe fn translate_message(lpMsg: *const MSG) -> BOOL {
        FALSE
    }
    
    /// DispatchMessageA
    pub unsafe fn dispatch_message_a(lpMsg: *const MSG) -> isize {
        0
    }
    
    /// PostQuitMessage
    pub unsafe fn post_quit_message(nExitCode: INT) {
        crate::serial_println!("[WIN32] PostQuitMessage({})", nExitCode);
    }
    
    /// DefWindowProcA
    pub unsafe fn def_window_proc_a(
        hWnd: HWND,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
    ) -> isize {
        0
    }
    
    /// GetDC
    pub unsafe fn get_dc(hWnd: HWND) -> HDC {
        hWnd as HDC
    }
    
    /// ReleaseDC
    pub unsafe fn release_dc(hWnd: HWND, hDC: HDC) -> INT {
        1
    }
    
    /// SetWindowTextA
    pub unsafe fn set_window_text_a(hWnd: HWND, lpString: LPCSTR) -> BOOL {
        let mut title = String::new();
        let mut ptr = lpString;
        while !ptr.is_null() && *ptr != 0 {
            title.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] SetWindowTextA: {}", title);
        TRUE
    }
    
    /// GetClientRect
    pub unsafe fn get_client_rect(hWnd: HWND, lpRect: *mut RECT) -> BOOL {
        if lpRect.is_null() {
            return FALSE;
        }
        (*lpRect).left = 0;
        (*lpRect).top = 0;
        (*lpRect).right = 640;
        (*lpRect).bottom = 480;
        TRUE
    }
    
    // ========================================================================
    // WINDOW MANAGEMENT
    // ========================================================================
    
    /// DestroyWindow
    pub unsafe fn destroy_window(hWnd: HWND) -> BOOL {
        crate::serial_println!("[WIN32] DestroyWindow: {}", hWnd);
        TRUE
    }
    
    /// IsWindow
    pub unsafe fn is_window(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// IsWindowVisible
    pub unsafe fn is_window_visible(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// IsWindowEnabled
    pub unsafe fn is_window_enabled(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// EnableWindow
    pub unsafe fn enable_window(hWnd: HWND, bEnable: BOOL) -> BOOL {
        TRUE
    }
    
    /// MoveWindow
    pub unsafe fn move_window(hWnd: HWND, x: INT, y: INT, nWidth: INT, nHeight: INT, bRepaint: BOOL) -> BOOL {
        crate::serial_println!("[WIN32] MoveWindow: {},{} {},{}", x, y, nWidth, nHeight);
        TRUE
    }
    
    /// SetWindowPos
    pub unsafe fn set_window_pos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        x: INT,
        y: INT,
        cx: INT,
        cy: INT,
        uFlags: UINT,
    ) -> BOOL {
        TRUE
    }
    
    /// GetWindowRect
    pub unsafe fn get_window_rect(hWnd: HWND, lpRect: *mut RECT) -> BOOL {
        if lpRect.is_null() {
            return FALSE;
        }
        (*lpRect).left = 0;
        (*lpRect).top = 0;
        (*lpRect).right = 640;
        (*lpRect).bottom = 480;
        TRUE
    }
    
    /// GetWindowTextA
    pub unsafe fn get_window_text_a(hWnd: HWND, lpString: LPSTR, nMaxCount: INT) -> INT {
        0
    }
    
    /// GetWindowTextLengthA
    pub unsafe fn get_window_text_length_a(hWnd: HWND) -> INT {
        0
    }
    
    /// GetParent
    pub unsafe fn get_parent(hWnd: HWND) -> HWND {
        0
    }
    
    /// SetParent
    pub unsafe fn set_parent(hWndChild: HWND, hWndNewParent: HWND) -> HWND {
        0
    }
    
    /// GetDesktopWindow
    pub unsafe fn get_desktop_window() -> HWND {
        0xFFFFFFFF as HWND
    }
    
    /// GetForegroundWindow
    pub unsafe fn get_foreground_window() -> HWND {
        1 as HWND
    }
    
    /// SetForegroundWindow
    pub unsafe fn set_foreground_window(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// GetActiveWindow
    pub unsafe fn get_active_window() -> HWND {
        1 as HWND
    }
    
    /// SetActiveWindow
    pub unsafe fn set_active_window(hWnd: HWND) -> HWND {
        hWnd
    }
    
    /// GetFocus
    pub unsafe fn get_focus() -> HWND {
        1 as HWND
    }
    
    /// SetFocus
    pub unsafe fn set_focus(hWnd: HWND) -> HWND {
        hWnd
    }
    
    /// GetCapture
    pub unsafe fn get_capture() -> HWND {
        0
    }
    
    /// SetCapture
    pub unsafe fn set_capture(hWnd: HWND) -> HWND {
        hWnd
    }
    
    /// ReleaseCapture
    pub unsafe fn release_capture() -> BOOL {
        TRUE
    }
    
    /// FindWindowA
    pub unsafe fn find_window_a(lpClassName: LPCSTR, lpWindowName: LPCSTR) -> HWND {
        0
    }
    
    /// FindWindowExA
    pub unsafe fn find_window_ex_a(
        hWndParent: HWND,
        hWndChildAfter: HWND,
        lpszClass: LPCSTR,
        lpszWindow: LPCSTR,
    ) -> HWND {
        0
    }
    
    /// GetWindow
    pub unsafe fn get_window(hWnd: HWND, uCmd: UINT) -> HWND {
        0
    }
    
    /// EnumWindows
    pub unsafe fn enum_windows(lpEnumFunc: Option<unsafe extern "system" fn(HWND, usize) -> BOOL>, lParam: usize) -> BOOL {
        TRUE
    }
    
    /// GetClassNameA
    pub unsafe fn get_class_name_a(hWnd: HWND, lpClassName: LPSTR, nMaxCount: INT) -> INT {
        0
    }
    
    // ========================================================================
    // MESSAGE MANAGEMENT
    // ========================================================================
    
    /// PeekMessageA
    pub unsafe fn peek_message_a(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: UINT,
        wMsgFilterMax: UINT,
        wRemoveMsg: UINT,
    ) -> BOOL {
        FALSE
    }
    
    /// PostMessageA
    pub unsafe fn post_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        TRUE
    }
    
    /// SendMessageA
    pub unsafe fn send_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> isize {
        0
    }
    
    /// SendMessageTimeoutA
    pub unsafe fn send_message_timeout_a(
        hWnd: HWND,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
        fuFlags: UINT,
        uTimeout: UINT,
        lpdwResult: *mut usize,
    ) -> isize {
        0
    }
    
    /// SendNotifyMessageA
    pub unsafe fn send_notify_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        TRUE
    }
    
    /// PostThreadMessageA
    pub unsafe fn post_thread_message_a(idThread: DWORD, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        TRUE
    }
    
    /// ReplyMessage
    pub unsafe fn reply_message(lResult: isize) -> BOOL {
        TRUE
    }
    
    /// GetMessageTime
    pub unsafe fn get_message_time() -> LONG {
        0
    }
    
    /// GetMessagePos
    pub unsafe fn get_message_pos() -> DWORD {
        0
    }
    
    /// WaitMessage
    pub unsafe fn wait_message() -> BOOL {
        TRUE
    }
    
    /// MsgWaitForMultipleObjects
    pub unsafe fn msg_wait_for_multiple_objects(
        nCount: DWORD,
        pHandles: *const HANDLE,
        fWaitAll: BOOL,
        dwMilliseconds: DWORD,
        dwWakeMask: DWORD,
    ) -> DWORD {
        0
    }
    
    /// RegisterWindowMessageA
    pub unsafe fn register_window_message_a(lpString: LPCSTR) -> UINT {
        0xC000
    }
    
    // ========================================================================
    // INPUT - KEYBOARD
    // ========================================================================
    
    /// GetKeyState
    pub unsafe fn get_key_state(nVirtKey: INT) -> SHORT {
        0
    }
    
    /// GetAsyncKeyState
    pub unsafe fn get_async_key_state(vKey: INT) -> SHORT {
        0
    }
    
    /// GetKeyboardState
    pub unsafe fn get_keyboard_state(lpKeyState: *mut BYTE) -> BOOL {
        if !lpKeyState.is_null() {
            for i in 0..256 {
                *lpKeyState.add(i) = 0;
            }
        }
        TRUE
    }
    
    /// SetKeyboardState
    pub unsafe fn set_keyboard_state(lpKeyState: *const BYTE) -> BOOL {
        TRUE
    }
    
    /// keybd_event
    pub unsafe fn keybd_event(bVk: BYTE, bScan: BYTE, dwFlags: DWORD, dwExtraInfo: usize) {
        let _ = (bVk, bScan, dwFlags, dwExtraInfo);
    }
    
    /// MapVirtualKeyA
    pub unsafe fn map_virtual_key_a(uCode: UINT, uMapType: UINT) -> UINT {
        0
    }
    
    /// MapVirtualKeyExA
    pub unsafe fn map_virtual_key_ex_a(uCode: UINT, uMapType: UINT, dwhkl: usize) -> UINT {
        0
    }
    
    /// ToAscii
    pub unsafe fn to_ascii(uVirtKey: UINT, uScanCode: UINT, lpKeyState: *const BYTE, lpChar: *mut WORD, uFlags: UINT) -> INT {
        0
    }
    
    /// ToUnicode
    pub unsafe fn to_unicode(wVirtKey: UINT, wScanCode: UINT, lpKeyState: *const BYTE, pwszBuff: *mut u16, cchBuff: INT, wFlags: UINT) -> INT {
        0
    }
    
    /// VkKeyScanA
    pub unsafe fn vk_key_scan_a(ch: i8) -> SHORT {
        0
    }
    
    /// VkKeyScanExA
    pub unsafe fn vk_key_scan_ex_a(ch: i8, dwhkl: usize) -> SHORT {
        0
    }
    
    /// GetKeyNameTextA
    pub unsafe fn get_key_name_text_a(lParam: LONG, lpString: LPSTR, nSize: INT) -> INT {
        0
    }
    
    /// OemKeyScan
    pub unsafe fn oem_key_scan(wOemChar: WORD) -> DWORD {
        0
    }
    
    // ========================================================================
    // INPUT - MOUSE
    // ========================================================================
    
    /// GetCursorPos
    pub unsafe fn get_cursor_pos(lpPoint: *mut POINT) -> BOOL {
        if !lpPoint.is_null() {
            (*lpPoint).x = 0;
            (*lpPoint).y = 0;
        }
        TRUE
    }
    
    /// SetCursorPos
    pub unsafe fn set_cursor_pos(x: INT, y: INT) -> BOOL {
        crate::serial_println!("[WIN32] SetCursorPos: {},{}", x, y);
        TRUE
    }
    
    /// mouse_event
    pub unsafe fn mouse_event(dwFlags: DWORD, dx: DWORD, dy: DWORD, cButtons: DWORD, dwExtraInfo: usize) {
        let _ = (dwFlags, dx, dy, cButtons, dwExtraInfo);
    }
    
    /// GetDoubleClickTime
    pub unsafe fn get_double_click_time() -> UINT {
        500
    }
    
    /// SetDoubleClickTime
    pub unsafe fn set_double_click_time(uInterval: UINT) -> BOOL {
        TRUE
    }
    
    /// SwapMouseButton
    pub unsafe fn swap_mouse_button(fSwap: BOOL) -> BOOL {
        FALSE
    }
    
    /// GetSystemMetrics
    pub unsafe fn get_system_metrics(nIndex: INT) -> INT {
        match nIndex {
            0 => 640,  // SM_CXSCREEN
            1 => 480,  // SM_CYSCREEN
            2 => 0,    // SM_CXVSCROLL
            3 => 0,    // SM_CYHSCROLL
            4 => 640,  // SM_CXSIZE
            5 => 480,  // SM_CYSIZE
            _ => 0,
        }
    }
    
    // ========================================================================
    // MENUS
    // ========================================================================
    
    /// CreateMenu
    pub unsafe fn create_menu() -> HMENU {
        1 as HMENU
    }
    
    /// CreatePopupMenu
    pub unsafe fn create_popup_menu() -> HMENU {
        2 as HMENU
    }
    
    /// DestroyMenu
    pub unsafe fn destroy_menu(hMenu: HMENU) -> BOOL {
        TRUE
    }
    
    /// AppendMenuA
    pub unsafe fn append_menu_a(hMenu: HMENU, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// InsertMenuA
    pub unsafe fn insert_menu_a(hMenu: HMENU, uPosition: UINT, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// ModifyMenuA
    pub unsafe fn modify_menu_a(hMnu: HMENU, uPosition: UINT, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// RemoveMenu
    pub unsafe fn remove_menu(hMenu: HMENU, uPosition: UINT, uFlags: UINT) -> BOOL {
        TRUE
    }
    
    /// DeleteMenu
    pub unsafe fn delete_menu(hMenu: HMENU, uPosition: UINT, uFlags: UINT) -> BOOL {
        TRUE
    }
    
    /// SetMenu
    pub unsafe fn set_menu(hWnd: HWND, hMenu: HMENU) -> BOOL {
        TRUE
    }
    
    /// GetMenu
    pub unsafe fn get_menu(hWnd: HWND) -> HMENU {
        0
    }
    
    /// DrawMenuBar
    pub unsafe fn draw_menu_bar(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// TrackPopupMenu
    pub unsafe fn track_popup_menu(
        hMenu: HMENU,
        uFlags: UINT,
        x: INT,
        y: INT,
        nReserved: INT,
        hWnd: HWND,
        prcRect: *const RECT,
    ) -> BOOL {
        TRUE
    }
    
    /// GetMenuItemCount
    pub unsafe fn get_menu_item_count(hMenu: HMENU) -> INT {
        0
    }
    
    /// GetMenuItemID
    pub unsafe fn get_menu_item_id(hMenu: HMENU, nPos: INT) -> UINT {
        0xFFFFFFFF
    }
    
    /// GetMenuStringA
    pub unsafe fn get_menu_string_a(hMenu: HMENU, uItem: UINT, lpString: LPSTR, nMaxCount: INT, uFlag: UINT) -> INT {
        0
    }
    
    /// CheckMenuItem
    pub unsafe fn check_menu_item(hMenu: HMENU, uIDCheckItem: UINT, uCheck: UINT) -> DWORD {
        0xFFFFFFFF
    }
    
    /// EnableMenuItem
    pub unsafe fn enable_menu_item(hMenu: HMENU, uIDEnableItem: UINT, uEnable: UINT) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // DIALOGS
    // ========================================================================
    
    /// MessageBoxA
    pub unsafe fn message_box_a(hWnd: HWND, lpText: LPCSTR, lpCaption: LPCSTR, uType: UINT) -> INT {
        let mut text = String::new();
        let mut ptr = lpText;
        while !ptr.is_null() && *ptr != 0 {
            text.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] MessageBoxA: {}", text);
        1 // IDOK
    }
    
    /// MessageBoxExA
    pub unsafe fn message_box_ex_a(hWnd: HWND, lpText: LPCSTR, lpCaption: LPCSTR, uType: UINT, wLanguageId: WORD) -> INT {
        1
    }
    
    /// MessageBoxIndirectA
    pub unsafe fn message_box_indirect_a(lpMsgBoxParams: *const u8) -> INT {
        1
    }
    
    /// DialogBoxParamA
    pub unsafe fn dialog_box_param_a(
        hInstance: HINSTANCE,
        lpTemplateName: LPCSTR,
        hWndParent: HWND,
        lpDialogFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
        dwInitParam: usize,
    ) -> isize {
        0
    }
    
    /// EndDialog
    pub unsafe fn end_dialog(hDlg: HWND, nResult: isize) -> BOOL {
        TRUE
    }
    
    /// CreateDialogParamA
    pub unsafe fn create_dialog_param_a(
        hInstance: HINSTANCE,
        lpTemplateName: LPCSTR,
        hWndParent: HWND,
        lpDialogFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
        dwInitParam: usize,
    ) -> HWND {
        0
    }
    
    /// GetDlgItem
    pub unsafe fn get_dlg_item(hDlg: HWND, nIDDlgItem: INT) -> HWND {
        0
    }
    
    /// SetDlgItemTextA
    pub unsafe fn set_dlg_item_text_a(hDlg: HWND, nIDDlgItem: INT, lpString: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// GetDlgItemTextA
    pub unsafe fn get_dlg_item_text_a(hDlg: HWND, nIDDlgItem: INT, lpString: LPSTR, nMaxCount: INT) -> UINT {
        0
    }
    
    /// SetDlgItemInt
    pub unsafe fn set_dlg_item_int(hDlg: HWND, nIDDlgItem: INT, uValue: UINT, bSigned: BOOL) -> BOOL {
        TRUE
    }
    
    /// GetDlgItemInt
    pub unsafe fn get_dlg_item_int(hDlg: HWND, nIDDlgItem: INT, lpTranslated: *mut BOOL, bSigned: BOOL) -> UINT {
        0
    }
    
    /// CheckDlgButton
    pub unsafe fn check_dlg_button(hDlg: HWND, nIDButton: INT, uCheck: UINT) -> BOOL {
        TRUE
    }
    
    /// CheckRadioButton
    pub unsafe fn check_radio_button(hDlg: HWND, nIDFirstButton: INT, nIDLastButton: INT, nIDCheckButton: INT) -> BOOL {
        TRUE
    }
    
    /// IsDlgButtonChecked
    pub unsafe fn is_dlg_button_checked(hDlg: HWND, nIDButton: INT) -> UINT {
        0
    }
    
    // ========================================================================
    // CONTROLS
    // ========================================================================
    
    /// CreateWindowExA - already defined above
    
    /// SetWindowTextA - already defined above
    
    /// GetWindowTextA - already defined above
    
    /// EnableWindow - already defined above
    
    /// ShowWindow - already defined above
    
    /// GetDlgItemInt - already defined above
    
    /// SetDlgItemInt - already defined above
    
    /// SendDlgItemMessageA
    pub unsafe fn send_dlg_item_message_a(hDlg: HWND, nIDDlgItem: INT, Msg: UINT, wParam: usize, lParam: isize) -> isize {
        0
    }
    
    /// GetNextDlgTabItem
    pub unsafe fn get_next_dlg_tab_item(hDlg: HWND, hCtl: HWND, bPrevious: BOOL) -> HWND {
        0
    }
    
    /// GetNextDlgGroupItem
    pub unsafe fn get_next_dlg_group_item(hDlg: HWND, hCtl: HWND, bPrevious: BOOL) -> HWND {
        0
    }
    
    // ========================================================================
    // TIMERS
    // ========================================================================
    
    /// SetTimer
    pub unsafe fn set_timer(hWnd: HWND, nIDEvent: usize, uElapse: UINT, lpTimerFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, DWORD)>) -> usize {
        crate::serial_println!("[WIN32] SetTimer: {}ms", uElapse);
        nIDEvent
    }
    
    /// KillTimer
    pub unsafe fn kill_timer(hWnd: HWND, uIDEvent: usize) -> BOOL {
        TRUE
    }
    
    /// GetTickCount - from kernel32
    
    // ========================================================================
    // CLIPBOARD
    // ========================================================================
    
    /// OpenClipboard
    pub unsafe fn open_clipboard(hWnd: HWND) -> BOOL {
        TRUE
    }
    
    /// CloseClipboard
    pub unsafe fn close_clipboard() -> BOOL {
        TRUE
    }
    
    /// EmptyClipboard
    pub unsafe fn empty_clipboard() -> BOOL {
        TRUE
    }
    
    /// GetClipboardData
    pub unsafe fn get_clipboard_data(uFormat: UINT) -> HANDLE {
        0
    }
    
    /// SetClipboardData
    pub unsafe fn set_clipboard_data(uFormat: UINT, hMem: HANDLE) -> HANDLE {
        hMem
    }
    
    /// IsClipboardFormatAvailable
    pub unsafe fn is_clipboard_format_available(uFormat: UINT) -> BOOL {
        FALSE
    }
    
    /// RegisterClipboardFormatA
    pub unsafe fn register_clipboard_format_a(lpszFormat: LPCSTR) -> UINT {
        0xC000
    }
    
    /// CountClipboardFormats
    pub unsafe fn count_clipboard_formats() -> INT {
        0
    }
    
    /// EnumClipboardFormats
    pub unsafe fn enum_clipboard_formats(uFormat: UINT) -> UINT {
        0
    }
    
    /// GetClipboardOwner
    pub unsafe fn get_clipboard_owner() -> HWND {
        0
    }
    
    /// SetClipboardViewer
    pub unsafe fn set_clipboard_viewer(hWndNewViewer: HWND) -> HWND {
        0
    }
    
    /// GetClipboardViewer
    pub unsafe fn get_clipboard_viewer() -> HWND {
        0
    }
    
    /// ChangeClipboardChain
    pub unsafe fn change_clipboard_chain(hWndRemove: HWND, hWndNewNext: HWND) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // RESOURCES
    // ========================================================================
    
    /// LoadIconA
    pub unsafe fn load_icon_a(hInstance: HINSTANCE, lpIconName: LPCSTR) -> HICON {
        1 as HICON
    }
    
    /// LoadCursorA
    pub unsafe fn load_cursor_a(hInstance: HINSTANCE, lpCursorName: LPCSTR) -> HCURSOR {
        1 as HCURSOR
    }
    
    /// LoadBitmapA
    pub unsafe fn load_bitmap_a(hInstance: HINSTANCE, lpBitmapName: LPCSTR) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// LoadStringA
    pub unsafe fn load_string_a(hInstance: HINSTANCE, uID: UINT, lpBuffer: LPSTR, nBufferMax: INT) -> INT {
        0
    }
    
    /// LoadImageA
    pub unsafe fn load_image_a(
        hInst: HINSTANCE,
        lpszName: LPCSTR,
        uType: UINT,
        cxDesired: INT,
        cyDesired: INT,
        fuLoad: UINT,
    ) -> HANDLE {
        1 as HANDLE
    }
    
    /// CopyImage
    pub unsafe fn copy_image(hImage: HANDLE, uType: UINT, cxDesired: INT, cyDesired: INT, fuFlags: UINT) -> HANDLE {
        hImage
    }
    
    /// DestroyIcon
    pub unsafe fn destroy_icon(hIcon: HICON) -> BOOL {
        TRUE
    }
    
    /// DestroyCursor
    pub unsafe fn destroy_cursor(hCursor: HCURSOR) -> BOOL {
        TRUE
    }
    
    /// SetCursor
    pub unsafe fn set_cursor(hCursor: HCURSOR) -> HCURSOR {
        hCursor
    }
    
    /// GetCursor
    pub unsafe fn get_cursor() -> HCURSOR {
        1 as HCURSOR
    }
    
    // ========================================================================
    // HOOKS
    // ========================================================================
    
    /// SetWindowsHookExA
    pub unsafe fn set_windows_hook_ex_a(
        idHook: INT,
        lpfn: Option<unsafe extern "system" fn(INT, usize, isize) -> isize>,
        hMod: HINSTANCE,
        dwThreadId: DWORD,
    ) -> HANDLE {
        1 as HANDLE
    }
    
    /// UnhookWindowsHookEx
    pub unsafe fn unhook_windows_hook_ex(hhk: HANDLE) -> BOOL {
        TRUE
    }
    
    /// CallNextHookEx
    pub unsafe fn call_next_hook_ex(hhk: HANDLE, nCode: INT, wParam: usize, lParam: isize) -> isize {
        0
    }
    
    // ========================================================================
    // MISC
    // ========================================================================
    
    /// GetWindowLongA
    pub unsafe fn get_window_long_a(hWnd: HWND, nIndex: INT) -> isize {
        0
    }
    
    /// SetWindowLongA
    pub unsafe fn set_window_long_a(hWnd: HWND, nIndex: INT, dwNewLong: isize) -> isize {
        0
    }
    
    /// GetWindowLongPtrA
    pub unsafe fn get_window_long_ptr_a(hWnd: HWND, nIndex: INT) -> isize {
        0
    }
    
    /// SetWindowLongPtrA
    pub unsafe fn set_window_long_ptr_a(hWnd: HWND, nIndex: INT, dwNewLong: isize) -> isize {
        0
    }
    
    /// GetClassLongA
    pub unsafe fn get_class_long_a(hWnd: HWND, nIndex: INT) -> DWORD {
        0
    }
    
    /// SetClassLongA
    pub unsafe fn set_class_long_a(hWnd: HWND, nIndex: INT, dwNewLong: DWORD) -> DWORD {
        0
    }
    
    /// GetPropA
    pub unsafe fn get_prop_a(hWnd: HWND, lpString: LPCSTR) -> HANDLE {
        0
    }
    
    /// SetPropA
    pub unsafe fn set_prop_a(hWnd: HWND, lpString: LPCSTR, hData: HANDLE) -> BOOL {
        TRUE
    }
    
    /// RemovePropA
    pub unsafe fn remove_prop_a(hWnd: HWND, lpString: LPCSTR) -> HANDLE {
        0
    }
    
    /// EnumPropsA
    pub unsafe fn enum_props_a(hWnd: HWND, lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPCSTR, HANDLE) -> BOOL>) -> INT {
        0
    }
    
    /// GetWindowThreadProcessId
    pub unsafe fn get_window_thread_process_id(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD {
        if !lpdwProcessId.is_null() {
            *lpdwProcessId = 1;
        }
        1
    }
    
    /// AttachThreadInput
    pub unsafe fn attach_thread_input(idAttach: DWORD, idAttachTo: DWORD, fAttach: BOOL) -> BOOL {
        TRUE
    }
    
    /// GetQueueStatus
    pub unsafe fn get_queue_status(uFlags: UINT) -> DWORD {
        0
    }
    
    /// GetInputState
    pub unsafe fn get_input_state() -> BOOL {
        FALSE
    }
}

// ============================================================================
// ADVAPI32 IMPLEMENTATION
// ============================================================================

mod advapi32 {
    use super::*;
    
    // ========================================================================
    // REGISTRY
    // ========================================================================
    
    /// RegOpenKeyExA
    pub unsafe fn reg_open_key_ex_a(
        hKey: HKEY,
        lpSubKey: LPCSTR,
        ulOptions: DWORD,
        samDesired: DWORD,
        phkResult: *mut HKEY,
    ) -> LONG {
        crate::serial_println!("[WIN32] RegOpenKeyExA");
        2 // ERROR_FILE_NOT_FOUND
    }
    
    /// RegCloseKey
    pub unsafe fn reg_close_key(hKey: HKEY) -> LONG {
        0 // ERROR_SUCCESS
    }
    
    /// RegCreateKeyExA
    pub unsafe fn reg_create_key_ex_a(
        hKey: HKEY,
        lpSubKey: LPCSTR,
        Reserved: DWORD,
        lpClass: LPCSTR,
        dwOptions: DWORD,
        samDesired: DWORD,
        lpSecurityAttributes: *const u8,
        phkResult: *mut HKEY,
        lpdwDisposition: *mut DWORD,
    ) -> LONG {
        0
    }
    
    /// RegDeleteKeyA
    pub unsafe fn reg_delete_key_a(hKey: HKEY, lpSubKey: LPCSTR) -> LONG {
        0
    }
    
    /// RegDeleteValueA
    pub unsafe fn reg_delete_value_a(hKey: HKEY, lpValueName: LPCSTR) -> LONG {
        0
    }
    
    /// RegEnumKeyExA
    pub unsafe fn reg_enum_key_ex_a(
        hKey: HKEY,
        dwIndex: DWORD,
        lpName: LPSTR,
        lpcName: *mut DWORD,
        lpReserved: *mut DWORD,
        lpClass: LPSTR,
        lpcClass: *mut DWORD,
        lpftLastWriteTime: *mut u8,
    ) -> LONG {
        259 // ERROR_NO_MORE_ITEMS
    }
    
    /// RegEnumValueA
    pub unsafe fn reg_enum_value_a(
        hKey: HKEY,
        dwIndex: DWORD,
        lpValueName: LPSTR,
        lpcchValueName: *mut DWORD,
        lpReserved: *mut DWORD,
        lpType: *mut DWORD,
        lpData: *mut u8,
        lpcbData: *mut DWORD,
    ) -> LONG {
        259
    }
    
    /// RegQueryValueExA
    pub unsafe fn reg_query_value_ex_a(
        hKey: HKEY,
        lpValueName: LPCSTR,
        lpReserved: *mut DWORD,
        lpType: *mut DWORD,
        lpData: *mut u8,
        lpcbData: *mut DWORD,
    ) -> LONG {
        2 // ERROR_FILE_NOT_FOUND
    }
    
    /// RegSetValueExA
    pub unsafe fn reg_set_value_ex_a(
        hKey: HKEY,
        lpValueName: LPCSTR,
        Reserved: DWORD,
        dwType: DWORD,
        lpData: *const u8,
        cbData: DWORD,
    ) -> LONG {
        0
    }
    
    /// RegConnectRegistryA
    pub unsafe fn reg_connect_registry_a(lpMachineName: LPCSTR, hKey: HKEY, phkResult: *mut HKEY) -> LONG {
        0
    }
    
    /// RegNotifyChangeKeyValue
    pub unsafe fn reg_notify_change_key_value(hKey: HKEY, bWatchSubtree: BOOL, dwNotifyFilter: DWORD, hEvent: HANDLE, fAsynchronous: BOOL) -> LONG {
        0
    }
    
    // ========================================================================
    // SECURITY
    // ========================================================================
    
    /// GetUserNameA
    pub unsafe fn get_user_name_a(lpBuffer: LPSTR, nSize: *mut DWORD) -> BOOL {
        if !lpBuffer.is_null() && !nSize.is_null() {
            let name = b"echOS\0";
            let size = *nSize as usize;
            if size >= name.len() {
                for (i, &c) in name.iter().enumerate() {
                    *lpBuffer.add(i) = c as i8;
                }
                *nSize = (name.len() - 1) as DWORD;
                return TRUE;
            }
        }
        FALSE
    }
    
    /// LookupAccountNameA
    pub unsafe fn lookup_account_name_a(
        lpSystemName: LPCSTR,
        lpAccountName: LPCSTR,
        Sid: *mut u8,
        cbSid: *mut DWORD,
        ReferencedDomainName: LPSTR,
        cchReferencedDomainName: *mut DWORD,
        peUse: *mut DWORD,
    ) -> BOOL {
        FALSE
    }
    
    /// LookupAccountSidA
    pub unsafe fn lookup_account_sid_a(
        lpSystemName: LPCSTR,
        Sid: *const u8,
        Name: LPSTR,
        cchName: *mut DWORD,
        ReferencedDomainName: LPSTR,
        cchReferencedDomainName: *mut DWORD,
        peUse: *mut DWORD,
    ) -> BOOL {
        FALSE
    }
    
    /// InitializeSecurityDescriptor
    pub unsafe fn initialize_security_descriptor(pSecurityDescriptor: *mut u8, dwRevision: DWORD) -> BOOL {
        TRUE
    }
    
    /// InitializeAcl
    pub unsafe fn initialize_acl(pAcl: *mut u8, nAclLength: DWORD, dwAclRevision: DWORD) -> BOOL {
        TRUE
    }
    
    /// AddAccessAllowedAce
    pub unsafe fn add_access_allowed_ace(pAcl: *mut u8, dwAceRevision: DWORD, AccessMask: DWORD, pSid: *const u8) -> BOOL {
        TRUE
    }
    
    /// SetSecurityDescriptorDacl
    pub unsafe fn set_security_descriptor_dacl(pSecurityDescriptor: *mut u8, bDaclPresent: BOOL, pDacl: *const u8, bDaclDefaulted: BOOL) -> BOOL {
        TRUE
    }
    
    /// GetSecurityDescriptorDacl
    pub unsafe fn get_security_descriptor_dacl(pSecurityDescriptor: *const u8, lpbDaclPresent: *mut BOOL, pDacl: *mut *const u8, lpbDaclDefaulted: *mut BOOL) -> BOOL {
        TRUE
    }
    
    /// IsValidSecurityDescriptor
    pub unsafe fn is_valid_security_descriptor(pSecurityDescriptor: *const u8) -> BOOL {
        TRUE
    }
    
    /// GetLengthSid
    pub unsafe fn get_length_sid(pSid: *const u8) -> DWORD {
        28 // Standard SID length
    }
    
    /// CopySid
    pub unsafe fn copy_sid(nDestinationSidLength: DWORD, pDestinationSid: *mut u8, pSourceSid: *const u8) -> BOOL {
        TRUE
    }
    
    /// EqualSid
    pub unsafe fn equal_sid(pSid1: *const u8, pSid2: *const u8) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // SERVICES
    // ========================================================================
    
    /// OpenSCManagerA
    pub unsafe fn open_sc_manager_a(lpMachineName: LPCSTR, lpDatabaseName: LPCSTR, dwDesiredAccess: DWORD) -> SC_HANDLE {
        1 as SC_HANDLE
    }
    
    /// CloseServiceHandle
    pub unsafe fn close_service_handle(hSCObject: SC_HANDLE) -> BOOL {
        TRUE
    }
    
    /// OpenServiceA
    pub unsafe fn open_service_a(hSCManager: SC_HANDLE, lpServiceName: LPCSTR, dwDesiredAccess: DWORD) -> SC_HANDLE {
        0
    }
    
    /// CreateServiceA
    pub unsafe fn create_service_a(
        hSCManager: SC_HANDLE,
        lpServiceName: LPCSTR,
        lpDisplayName: LPCSTR,
        dwDesiredAccess: DWORD,
        dwServiceType: DWORD,
        dwStartType: DWORD,
        dwErrorControl: DWORD,
        lpBinaryPathName: LPCSTR,
        lpLoadOrderGroup: LPCSTR,
        lpdwTagId: *mut DWORD,
        lpDependencies: LPCSTR,
        lpServiceStartName: LPCSTR,
        lpPassword: LPCSTR,
    ) -> SC_HANDLE {
        1 as SC_HANDLE
    }
    
    /// DeleteService
    pub unsafe fn delete_service(hService: SC_HANDLE) -> BOOL {
        TRUE
    }
    
    /// StartServiceA
    pub unsafe fn start_service_a(hService: SC_HANDLE, dwNumServiceArgs: DWORD, lpServiceArgVectors: *const LPCSTR) -> BOOL {
        TRUE
    }
    
    /// ControlService
    pub unsafe fn control_service(hService: SC_HANDLE, dwControl: DWORD, lpServiceStatus: *mut SERVICE_STATUS) -> BOOL {
        TRUE
    }
    
    /// QueryServiceStatus
    pub unsafe fn query_service_status(hService: SC_HANDLE, lpServiceStatus: *mut SERVICE_STATUS) -> BOOL {
        if !lpServiceStatus.is_null() {
            (*lpServiceStatus).dwServiceType = 0x10; // SERVICE_WIN32_OWN_PROCESS
            (*lpServiceStatus).dwCurrentState = 0x04; // SERVICE_RUNNING
            (*lpServiceStatus).dwControlsAccepted = 0;
            (*lpServiceStatus).dwWin32ExitCode = 0;
            (*lpServiceStatus).dwServiceSpecificExitCode = 0;
            (*lpServiceStatus).dwCheckPoint = 0;
            (*lpServiceStatus).dwWaitHint = 0;
        }
        TRUE
    }
    
    /// EnumServicesStatusA
    pub unsafe fn enum_services_status_a(
        hSCManager: SC_HANDLE,
        dwServiceType: DWORD,
        dwServiceState: DWORD,
        lpServices: *mut u8,
        cbBufSize: DWORD,
        pcbBytesNeeded: *mut DWORD,
        lpServicesReturned: *mut DWORD,
        lpResumeHandle: *mut DWORD,
    ) -> BOOL {
        FALSE
    }
    
    /// GetServiceKeyNameA
    pub unsafe fn get_service_key_name_a(hSCManager: SC_HANDLE, lpDisplayName: LPCSTR, lpServiceName: LPSTR, lpcchBuffer: *mut DWORD) -> BOOL {
        FALSE
    }
    
    /// GetServiceDisplayNameA
    pub unsafe fn get_service_display_name_a(hSCManager: SC_HANDLE, lpServiceName: LPCSTR, lpDisplayName: LPSTR, lpcchBuffer: *mut DWORD) -> BOOL {
        FALSE
    }
    
    // ========================================================================
    // EVENT LOG
    // ========================================================================
    
    /// RegisterEventSourceA
    pub unsafe fn register_event_source_a(lpUNCServerName: LPCSTR, lpSourceName: LPCSTR) -> HANDLE {
        1 as HANDLE
    }
    
    /// DeregisterEventSource
    pub unsafe fn deregister_event_source(hEventLog: HANDLE) -> BOOL {
        TRUE
    }
    
    /// ReportEventA
    pub unsafe fn report_event_a(
        hEventLog: HANDLE,
        wType: WORD,
        wCategory: WORD,
        dwEventID: DWORD,
        lpUserSid: *const u8,
        wNumStrings: WORD,
        dwDataSize: DWORD,
        lpStrings: *const LPCSTR,
        lpRawData: *const u8,
    ) -> BOOL {
        TRUE
    }
    
    /// OpenEventLogA
    pub unsafe fn open_event_log_a(lpUNCServerName: LPCSTR, lpSourceName: LPCSTR) -> HANDLE {
        1 as HANDLE
    }
    
    /// CloseEventLog
    pub unsafe fn close_event_log(hEventLog: HANDLE) -> BOOL {
        TRUE
    }
    
    /// ClearEventLogA
    pub unsafe fn clear_event_log_a(hEventLog: HANDLE, lpBackupFileName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// ReadEventLogA
    pub unsafe fn read_event_log_a(
        hEventLog: HANDLE,
        dwReadFlags: DWORD,
        dwRecordOffset: DWORD,
        lpBuffer: *mut u8,
        nNumberOfBytesToRead: DWORD,
        pnBytesRead: *mut DWORD,
        pnMinNumberOfBytesNeeded: *mut DWORD,
    ) -> BOOL {
        FALSE
    }
    
    /// GetNumberOfEventLogRecords
    pub unsafe fn get_number_of_event_log_records(hEventLog: HANDLE, NumberOfRecords: *mut DWORD) -> BOOL {
        if !NumberOfRecords.is_null() {
            *NumberOfRecords = 0;
        }
        TRUE
    }
    
    // ========================================================================
    // CRYPTO
    // ========================================================================
    
    /// CryptAcquireContextA
    pub unsafe fn crypt_acquire_context_a(
        phProv: *mut HCRYPTPROV,
        pszContainer: LPCSTR,
        pszProvider: LPCSTR,
        dwProvType: DWORD,
        dwFlags: DWORD,
    ) -> BOOL {
        if !phProv.is_null() {
            *phProv = 1 as HCRYPTPROV;
        }
        TRUE
    }
    
    /// CryptReleaseContext
    pub unsafe fn crypt_release_context(hProv: HCRYPTPROV, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptGenRandom
    pub unsafe fn crypt_gen_random(hProv: HCRYPTPROV, dwLen: DWORD, pbBuffer: *mut BYTE) -> BOOL {
        if !pbBuffer.is_null() {
            for i in 0..dwLen as usize {
                *pbBuffer.add(i) = crate::random::next_u32() as u8;
            }
        }
        TRUE
    }
    
    /// CryptCreateHash
    pub unsafe fn crypt_create_hash(hProv: HCRYPTPROV, Algid: DWORD, hKey: HCRYPTKEY, dwFlags: DWORD, phHash: *mut HCRYPTHASH) -> BOOL {
        if !phHash.is_null() {
            *phHash = 1 as HCRYPTHASH;
        }
        TRUE
    }
    
    /// CryptDestroyHash
    pub unsafe fn crypt_destroy_hash(hHash: HCRYPTHASH) -> BOOL {
        TRUE
    }
    
    /// CryptHashData
    pub unsafe fn crypt_hash_data(hHash: HCRYPTHASH, pbData: *const BYTE, dwDataLen: DWORD, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptGetHashParam
    pub unsafe fn crypt_get_hash_param(hHash: HCRYPTHASH, dwParam: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptDeriveKey
    pub unsafe fn crypt_derive_key(hProv: HCRYPTPROV, Algid: DWORD, hBaseData: HCRYPTHASH, dwFlags: DWORD, phKey: *mut HCRYPTKEY) -> BOOL {
        if !phKey.is_null() {
            *phKey = 1 as HCRYPTKEY;
        }
        TRUE
    }
    
    /// CryptDestroyKey
    pub unsafe fn crypt_destroy_key(hKey: HCRYPTKEY) -> BOOL {
        TRUE
    }
    
    /// CryptEncrypt
    pub unsafe fn crypt_encrypt(hKey: HCRYPTKEY, hHash: HCRYPTHASH, Final: BOOL, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD, dwBufLen: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptDecrypt
    pub unsafe fn crypt_decrypt(hKey: HCRYPTKEY, hHash: HCRYPTHASH, Final: BOOL, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptImportKey
    pub unsafe fn crypt_import_key(hProv: HCRYPTPROV, pbData: *const BYTE, dwDataLen: DWORD, hPubKey: HCRYPTKEY, dwFlags: DWORD, phKey: *mut HCRYPTKEY) -> BOOL {
        if !phKey.is_null() {
            *phKey = 1 as HCRYPTKEY;
        }
        TRUE
    }
    
    /// CryptExportKey
    pub unsafe fn crypt_export_key(hKey: HCRYPTKEY, hExpKey: HCRYPTKEY, dwBlobType: DWORD, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptSignHashA
    pub unsafe fn crypt_sign_hash_a(hHash: HCRYPTHASH, dwKeySpec: DWORD, sDescription: LPCSTR, dwFlags: DWORD, pbSignature: *mut BYTE, pdwSigLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptVerifySignatureA
    pub unsafe fn crypt_verify_signature_a(hHash: HCRYPTHASH, pbSignature: *const BYTE, dwSigLen: DWORD, hPubKey: HCRYPTKEY, sDescription: LPCSTR, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // PROCESS & THREAD
    // ========================================================================
    
    /// CreateProcessAsUserA
    pub unsafe fn create_process_as_user_a(
        hToken: HANDLE,
        lpApplicationName: LPCSTR,
        lpCommandLine: LPSTR,
        lpProcessAttributes: *const u8,
        lpThreadAttributes: *const u8,
        bInheritHandles: BOOL,
        dwCreationFlags: DWORD,
        lpEnvironment: *const u8,
        lpCurrentDirectory: LPCSTR,
        lpStartupInfo: *mut u8,
        lpProcessInformation: *mut u8,
    ) -> BOOL {
        TRUE
    }
    
    /// OpenProcessToken
    pub unsafe fn open_process_token(hProcess: HANDLE, dwDesiredAccess: DWORD, phToken: *mut HANDLE) -> BOOL {
        if !phToken.is_null() {
            *phToken = 1 as HANDLE;
        }
        TRUE
    }
    
    /// OpenThreadToken
    pub unsafe fn open_thread_token(hThread: HANDLE, dwDesiredAccess: DWORD, bOpenAsSelf: BOOL, phToken: *mut HANDLE) -> BOOL {
        TRUE
    }
    
    /// DuplicateTokenEx
    pub unsafe fn duplicate_token_ex(
        hExistingToken: HANDLE,
        dwDesiredAccess: DWORD,
        lpTokenAttributes: *const u8,
        ImpersonationLevel: DWORD,
        TokenType: DWORD,
        phNewToken: *mut HANDLE,
    ) -> BOOL {
        if !phNewToken.is_null() {
            *phNewToken = 1 as HANDLE;
        }
        TRUE
    }
    
    /// ImpersonateLoggedOnUser
    pub unsafe fn impersonate_logged_on_user(hToken: HANDLE) -> BOOL {
        TRUE
    }
    
    /// RevertToSelf
    pub unsafe fn revert_to_self() -> BOOL {
        TRUE
    }
    
    /// GetTokenInformation
    pub unsafe fn get_token_information(
        hToken: HANDLE,
        TokenInformationClass: DWORD,
        TokenInformation: *mut u8,
        TokenInformationLength: DWORD,
        ReturnLength: *mut DWORD,
    ) -> BOOL {
        TRUE
    }
    
    /// SetTokenInformation
    pub unsafe fn set_token_information(
        hToken: HANDLE,
        TokenInformationClass: DWORD,
        TokenInformation: *const u8,
        TokenInformationLength: DWORD,
    ) -> BOOL {
        TRUE
    }
    
    /// AdjustTokenPrivileges
    pub unsafe fn adjust_token_privileges(
        hToken: HANDLE,
        bDisableAllPrivileges: BOOL,
        NewState: *const u8,
        BufferLength: DWORD,
        PreviousState: *mut u8,
        ReturnLength: *mut DWORD,
    ) -> BOOL {
        TRUE
    }
    
    /// LookupPrivilegeValueA
    pub unsafe fn lookup_privilege_value_a(lpSystemName: LPCSTR, lpName: LPCSTR, lpLuid: *mut u64) -> BOOL {
        TRUE
    }
    
    /// LookupPrivilegeDisplayNameA
    pub unsafe fn lookup_privilege_display_name_a(lpSystemName: LPCSTR, lpName: LPCSTR, lpDisplayName: LPSTR, cchDisplayName: *mut DWORD, lpLanguageId: *mut DWORD) -> BOOL {
        TRUE
    }
}

// ============================================================================
// SHELL32 IMPLEMENTATION
// ============================================================================

mod shell32 {
    use super::*;
    
    /// ShellExecuteA
    pub unsafe fn shell_execute_a(
        hwnd: HWND,
        lpOperation: LPCSTR,
        lpFile: LPCSTR,
        lpParameters: LPCSTR,
        lpDirectory: LPCSTR,
        nShowCmd: INT,
    ) -> HINSTANCE {
        let mut file = String::new();
        let mut ptr = lpFile;
        while !ptr.is_null() && *ptr != 0 {
            file.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] ShellExecuteA: {}", file);
        1 as HINSTANCE
    }
    
    /// ShellExecuteExA
    pub unsafe fn shell_execute_ex_a(pExecInfo: *mut SHELLEXECUTEINFOA) -> BOOL {
        TRUE
    }
    
    /// ShellAboutA
    pub unsafe fn shell_about_a(hWnd: HWND, szApp: LPCSTR, szOtherStuff: LPCSTR, hIcon: HICON) -> BOOL {
        TRUE
    }
    
    /// ExtractIconA
    pub unsafe fn extract_icon_a(hInst: HINSTANCE, lpszExeFileName: LPCSTR, nIconIndex: UINT) -> HICON {
        1 as HICON
    }
    
    /// ExtractIconExA
    pub unsafe fn extract_icon_ex_a(lpszFile: LPCSTR, nIconIndex: INT, phiconLarge: *mut HICON, phiconSmall: *mut HICON, nIcons: UINT) -> UINT {
        0
    }
    
    /// DragAcceptFiles
    pub unsafe fn drag_accept_files(hWnd: HWND, fAccept: BOOL) {
        let _ = (hWnd, fAccept);
    }
    
    /// DragQueryFileA
    pub unsafe fn drag_query_file_a(hDrop: HDROP, iFile: UINT, lpszFile: LPSTR, cch: UINT) -> UINT {
        0
    }
    
    /// DragQueryPoint
    pub unsafe fn drag_query_point(hDrop: HDROP, lppt: *mut POINT) -> BOOL {
        FALSE
    }
    
    /// DragFinish
    pub unsafe fn drag_finish(hDrop: HDROP) {
        let _ = hDrop;
    }
    
    /// Shell_NotifyIconA
    pub unsafe fn shell_notify_icon_a(dwMessage: DWORD, lpData: *const NOTIFYICONDATAA) -> BOOL {
        TRUE
    }
    
    /// SHGetPathFromIDListA
    pub unsafe fn sh_get_path_from_id_list_a(pidl: LPCSTR, pszPath: LPSTR) -> BOOL {
        FALSE
    }
    
    /// SHBrowseForFolderA
    pub unsafe fn sh_browse_for_folder_a(lpbi: *const BROWSEINFOA) -> LPCSTR {
        0 as LPCSTR
    }
    
    /// SHGetSpecialFolderPathA
    pub unsafe fn sh_get_special_folder_path_a(hwnd: HWND, pszPath: LPSTR, csidl: INT, fCreate: BOOL) -> BOOL {
        FALSE
    }
    
    /// SHGetFolderPathA
    pub unsafe fn sh_get_folder_path_a(hwnd: HWND, csidl: INT, hToken: HANDLE, dwFlags: DWORD, pszPath: LPSTR) -> HRESULT {
        let _ = (hwnd, csidl, hToken, dwFlags, pszPath);
        0x80070002u32 as i32 // E_FAIL
    }
    
    /// SHGetDesktopFolder
    pub unsafe fn sh_get_desktop_folder(ppshf: *mut *mut u8) -> HRESULT {
        0
    }
    
    /// SHGetFileInfoA
    pub unsafe fn sh_get_file_info_a(
        pszPath: LPCSTR,
        dwFileAttributes: DWORD,
        psfi: *mut SHFILEINFOA,
        cbFileInfo: UINT,
        uFlags: UINT,
    ) -> DWORD_PTR {
        0
    }
    
    /// SHFileOperationA
    pub unsafe fn sh_file_operation_a(lpFileOp: *mut SHFILEOPSTRUCTA) -> INT {
        0
    }
    
    /// SHEmptyRecycleBinA
    pub unsafe fn sh_empty_recycle_bin_a(hwnd: HWND, pszRootPath: LPCSTR, dwFlags: DWORD) -> HRESULT {
        0
    }
    
    /// SHQueryRecycleBinA
    pub unsafe fn sh_query_recycle_bin_a(pszRootPath: LPCSTR, pSHQueryRBInfo: *mut u8) -> HRESULT {
        0
    }
}

// ============================================================================
// MSVCRT IMPLEMENTATION
// ============================================================================

mod msvcrt {
    use super::*;
    
    // ========================================================================
    // MEMORY
    // ========================================================================
    
    /// malloc
    pub unsafe fn malloc(size: SIZE_T) -> LPVOID {
        // TODO: Use actual heap allocator
        let _ = size;
        0 as LPVOID
    }
    
    /// free
    pub unsafe fn free(ptr: LPVOID) {
        let _ = ptr;
    }
    
    /// calloc
    pub unsafe fn calloc(num: SIZE_T, size: SIZE_T) -> LPVOID {
        let _ = (num, size);
        0 as LPVOID
    }
    
    /// realloc
    pub unsafe fn realloc(ptr: LPVOID, size: SIZE_T) -> LPVOID {
        let _ = (ptr, size);
        0 as LPVOID
    }
    
    /// _msize
    pub unsafe fn _msize(ptr: LPVOID) -> SIZE_T {
        // TODO: Get actual allocation size
        let _ = ptr;
        0
    }
    
    /// _expand
    pub unsafe fn _expand(ptr: LPVOID, size: SIZE_T) -> LPVOID {
        let _ = (ptr, size);
        0 as LPVOID
    }
    
    /// _heapmin
    pub unsafe fn _heapmin() -> INT {
        0
    }
    
    // ========================================================================
    // STRING
    // ========================================================================
    
    /// strlen
    pub unsafe fn strlen(s: LPCSTR) -> SIZE_T {
        let mut len = 0usize;
        let mut ptr = s;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len as SIZE_T
    }
    
    /// strcpy
    pub unsafe fn strcpy(dest: LPSTR, src: LPCSTR) -> LPSTR {
        let mut d = dest;
        let mut s = src;
        while !s.is_null() && *s != 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
        }
        *d = 0;
        dest
    }
    
    /// strncpy
    pub unsafe fn strncpy(dest: LPSTR, src: LPCSTR, count: SIZE_T) -> LPSTR {
        let mut d = dest;
        let mut s = src;
        let mut i = 0usize;
        while i < count as usize && !s.is_null() && *s != 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
            i += 1;
        }
        while i < count as usize {
            *d = 0;
            d = d.add(1);
            i += 1;
        }
        dest
    }
    
    /// strcat
    pub unsafe fn strcat(dest: LPSTR, src: LPCSTR) -> LPSTR {
        let mut d = dest;
        while !d.is_null() && *d != 0 {
            d = d.add(1);
        }
        let mut s = src;
        while !s.is_null() && *s != 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
        }
        *d = 0;
        dest
    }
    
    /// strncat
    pub unsafe fn strncat(dest: LPSTR, src: LPCSTR, count: SIZE_T) -> LPSTR {
        let mut d = dest;
        while !d.is_null() && *d != 0 {
            d = d.add(1);
        }
        let mut s = src;
        let mut i = 0usize;
        while i < count as usize && !s.is_null() && *s != 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
            i += 1;
        }
        *d = 0;
        dest
    }
    
    /// strcmp
    pub unsafe fn strcmp(s1: LPCSTR, s2: LPCSTR) -> INT {
        let mut p1 = s1;
        let mut p2 = s2;
        loop {
            let c1 = if !p1.is_null() { *p1 as u8 } else { 0 };
            let c2 = if !p2.is_null() { *p2 as u8 } else { 0 };
            if c1 != c2 {
                return (c1 as INT) - (c2 as INT);
            }
            if c1 == 0 {
                return 0;
            }
            p1 = p1.add(1);
            p2 = p2.add(1);
        }
    }
    
    /// strncmp
    pub unsafe fn strncmp(s1: LPCSTR, s2: LPCSTR, count: SIZE_T) -> INT {
        let mut p1 = s1;
        let mut p2 = s2;
        let mut i = 0usize;
        while i < count as usize {
            let c1 = if !p1.is_null() { *p1 as u8 } else { 0 };
            let c2 = if !p2.is_null() { *p2 as u8 } else { 0 };
            if c1 != c2 {
                return (c1 as INT) - (c2 as INT);
            }
            if c1 == 0 {
                return 0;
            }
            p1 = p1.add(1);
            p2 = p2.add(1);
            i += 1;
        }
        0
    }
    
    /// strchr
    pub unsafe fn strchr(s: LPCSTR, c: INT) -> LPSTR {
        let mut ptr = s;
        while !ptr.is_null() && *ptr != 0 {
            if *ptr == c as i8 {
                return ptr as LPSTR;
            }
            ptr = ptr.add(1);
        }
        0 as LPSTR
    }
    
    /// strrchr
    pub unsafe fn strrchr(s: LPCSTR, c: INT) -> LPSTR {
        let mut last = 0 as LPSTR;
        let mut ptr = s;
        while !ptr.is_null() && *ptr != 0 {
            if *ptr == c as i8 {
                last = ptr as LPSTR;
            }
            ptr = ptr.add(1);
        }
        last
    }
    
    /// strstr
    pub unsafe fn strstr(haystack: LPCSTR, needle: LPCSTR) -> LPSTR {
        if haystack.is_null() || needle.is_null() {
            return 0 as LPSTR;
        }
        let needle_len = strlen(needle);
        if needle_len == 0 {
            return haystack as LPSTR;
        }
        let mut ptr = haystack;
        while !ptr.is_null() && *ptr != 0 {
            if strncmp(ptr, needle, needle_len) == 0 {
                return ptr as LPSTR;
            }
            ptr = ptr.add(1);
        }
        0 as LPSTR
    }
    
    /// memcpy
    pub unsafe fn memcpy(dest: LPVOID, src: LPCVOID, count: SIZE_T) -> LPVOID {
        let mut d = dest as *mut u8;
        let mut s = src as *const u8;
        for _ in 0..count as usize {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
        }
        dest
    }
    
    /// memmove
    pub unsafe fn memmove(dest: LPVOID, src: LPCVOID, count: SIZE_T) -> LPVOID {
        let d = dest as usize;
        let s = src as usize;
        if d < s || d >= s + count as usize {
            // Forward copy
            memcpy(dest, src, count)
        } else {
            // Backward copy
            let mut d = (dest as *mut u8).add(count as usize - 1);
            let mut s = (src as *const u8).add(count as usize - 1);
            for _ in 0..count as usize {
                *d = *s;
                d = d.sub(1);
                s = s.sub(1);
            }
            dest
        }
    }
    
    /// memset
    pub unsafe fn memset(dest: LPVOID, c: INT, count: SIZE_T) -> LPVOID {
        let mut d = dest as *mut u8;
        for _ in 0..count as usize {
            *d = c as u8;
            d = d.add(1);
        }
        dest
    }
    
    /// memcmp
    pub unsafe fn memcmp(s1: LPCVOID, s2: LPCVOID, count: SIZE_T) -> INT {
        let mut p1 = s1 as *const u8;
        let mut p2 = s2 as *const u8;
        for _ in 0..count as usize {
            if *p1 != *p2 {
                return (*p1 as INT) - (*p2 as INT);
            }
            p1 = p1.add(1);
            p2 = p2.add(1);
        }
        0
    }
    
    // ========================================================================
    // IO
    // ========================================================================
    
    /// fopen
    pub unsafe fn fopen(filename: LPCSTR, mode: LPCSTR) -> *mut FILE {
        let _ = (filename, mode);
        0 as *mut FILE
    }
    
    /// fclose
    pub unsafe fn fclose(stream: *mut FILE) -> INT {
        let _ = stream;
        0
    }
    
    /// fread
    pub unsafe fn fread(ptr: LPVOID, size: SIZE_T, count: SIZE_T, stream: *mut FILE) -> SIZE_T {
        let _ = (ptr, size, count, stream);
        0
    }
    
    /// fwrite
    pub unsafe fn fwrite(ptr: LPCVOID, size: SIZE_T, count: SIZE_T, stream: *mut FILE) -> SIZE_T {
        let _ = (ptr, size, count, stream);
        0
    }
    
    /// fseek
    pub unsafe fn fseek(stream: *mut FILE, offset: LONG, origin: INT) -> INT {
        let _ = (stream, offset, origin);
        0
    }
    
    /// ftell
    pub unsafe fn ftell(stream: *mut FILE) -> LONG {
        let _ = stream;
        0
    }
    
    /// feof
    pub unsafe fn feof(stream: *mut FILE) -> INT {
        let _ = stream;
        0
    }
    
    /// fgetc
    pub unsafe fn fgetc(stream: *mut FILE) -> INT {
        let _ = stream;
        -1 // EOF
    }
    
    /// fputc
    pub unsafe fn fputc(c: INT, stream: *mut FILE) -> INT {
        let _ = (c, stream);
        -1
    }
    
    /// fgets
    pub unsafe fn fgets(s: LPSTR, n: INT, stream: *mut FILE) -> LPSTR {
        let _ = (s, n, stream);
        0 as LPSTR
    }
    
    /// fputs
    pub unsafe fn fputs(s: LPCSTR, stream: *mut FILE) -> INT {
        let _ = (s, stream);
        -1
    }
    
    /// fprintf
    pub unsafe fn fprintf(stream: *mut FILE, format: LPCSTR, args: *const u8) -> INT {
        let _ = (stream, format, args);
        0
    }
    
    /// printf
    pub unsafe fn printf(format: LPCSTR, args: *const u8) -> INT {
        let _ = (format, args);
        0
    }
    
    /// sprintf
    pub unsafe fn sprintf(buffer: LPSTR, format: LPCSTR, args: *const u8) -> INT {
        let _ = (buffer, format, args);
        0
    }
    
    /// snprintf
    pub unsafe fn snprintf(buffer: LPSTR, count: SIZE_T, format: LPCSTR, args: *const u8) -> INT {
        let _ = (buffer, count, format, args);
        0
    }
    
    /// scanf
    pub unsafe fn scanf(format: LPCSTR, args: *const u8) -> INT {
        let _ = (format, args);
        -1
    }
    
    // ========================================================================
    // MATH
    // ========================================================================
    
    /// abs
    pub unsafe fn abs(n: INT) -> INT {
        if n < 0 { -n } else { n }
    }
    
    /// labs
    pub unsafe fn labs(n: LONG) -> LONG {
        if n < 0 { -n } else { n }
    }
    
    /// rand
    pub unsafe fn rand() -> INT {
        (crate::random::next_u32() & 0x7FFF) as INT
    }
    
    /// srand
    pub unsafe fn srand(seed: UINT) {
        let _ = seed;
    }
    
    // ========================================================================
    // TIME
    // ========================================================================
    
    /// time
    pub unsafe fn time(timer: *mut time_t) -> time_t {
        let t = crate::random::next_u32() as time_t;
        if !timer.is_null() {
            *timer = t;
        }
        t
    }
    
    /// clock
    pub unsafe fn clock() -> clock_t {
        crate::random::next_u32() as clock_t
    }
    
    /// localtime
    pub unsafe fn localtime(timer: *const time_t) -> *mut tm {
        let _ = timer;
        0 as *mut tm
    }
    
    /// gmtime
    pub unsafe fn gmtime(timer: *const time_t) -> *mut tm {
        let _ = timer;
        0 as *mut tm
    }
    
    /// asctime
    pub unsafe fn asctime(tm: *const tm) -> LPSTR {
        let _ = tm;
        0 as LPSTR
    }
    
    /// ctime
    pub unsafe fn ctime(timer: *const time_t) -> LPSTR {
        let _ = timer;
        0 as LPSTR
    }
    
    /// strftime
    pub unsafe fn strftime(s: LPSTR, maxsize: SIZE_T, format: LPCSTR, tm: *const tm) -> SIZE_T {
        let _ = (s, maxsize, format, tm);
        0
    }
    
    // ========================================================================
    // MISC
    // ========================================================================
    
    /// exit
    pub unsafe fn exit(code: INT) {
        crate::serial_println!("[WIN32] exit({})", code);
        loop {}
    }
    
    /// abort
    pub unsafe fn abort() {
        crate::serial_println!("[WIN32] abort()");
        loop {}
    }
    
    /// system
    pub unsafe fn system(command: LPCSTR) -> INT {
        let _ = command;
        -1
    }
    
    /// getenv
    pub unsafe fn getenv(varname: LPCSTR) -> LPSTR {
        let _ = varname;
        0 as LPSTR
    }
    
    /// atoi
    pub unsafe fn atoi(s: LPCSTR) -> INT {
        let mut result = 0i32;
        let mut ptr = s;
        let mut sign = 1i32;
        
        // Skip whitespace
        while !ptr.is_null() && (*ptr == ' ' as i8 || *ptr == '\t' as i8 || *ptr == '\n' as i8) {
            ptr = ptr.add(1);
        }
        
        // Handle sign
        if !ptr.is_null() && *ptr == '-' as i8 {
            sign = -1;
            ptr = ptr.add(1);
        } else if !ptr.is_null() && *ptr == '+' as i8 {
            ptr = ptr.add(1);
        }
        
        // Parse digits
        while !ptr.is_null() && *ptr >= '0' as i8 && *ptr <= '9' as i8 {
            result = result * 10 + (*ptr - '0' as i8) as i32;
            ptr = ptr.add(1);
        }
        
        result * sign
    }
    
    /// atol
    pub unsafe fn atol(s: LPCSTR) -> LONG {
        atoi(s) as LONG
    }
    
    /// atof
    pub unsafe fn atof(s: LPCSTR) -> f64 {
        let _ = s;
        0.0
    }
    
    /// strtol
    pub unsafe fn strtol(s: LPCSTR, endptr: *mut LPSTR, base: INT) -> LONG {
        let _ = (s, endptr, base);
        0
    }
    
    /// strtoul
    pub unsafe fn strtoul(s: LPCSTR, endptr: *mut LPSTR, base: INT) -> ULONG {
        let _ = (s, endptr, base);
        0
    }
    
    /// strtod
    pub unsafe fn strtod(s: LPCSTR, endptr: *mut LPSTR) -> f64 {
        let _ = (s, endptr);
        0.0
    }
    
    /// qsort
    pub unsafe fn qsort(base: LPVOID, num: SIZE_T, size: SIZE_T, compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>) {
        let _ = (base, num, size, compar);
        // TODO: Implement quicksort
    }
    
    /// bsearch
    pub unsafe fn bsearch(key: LPCVOID, base: LPCVOID, num: SIZE_T, size: SIZE_T, compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>) -> LPVOID {
        let _ = (key, base, num, size, compar);
        0 as LPVOID
    }
}

// ============================================================================
// GDI32 IMPLEMENTATION
// ============================================================================

mod gdi32 {
    use super::*;
    
    // ========================================================================
    // DRAWING PRIMITIVES
    // ========================================================================
    
    /// MoveToEx
    pub unsafe fn move_to_ex(hdc: HDC, x: INT, y: INT, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// LineTo
    pub unsafe fn line_to(hdc: HDC, nXEnd: INT, nYEnd: INT) -> BOOL {
        crate::serial_println!("[WIN32] LineTo: {},{}", nXEnd, nYEnd);
        TRUE
    }
    
    /// Polyline
    pub unsafe fn polyline(hdc: HDC, lppt: *const POINT, cPoints: INT) -> BOOL {
        TRUE
    }
    
    /// PolylineTo
    pub unsafe fn polyline_to(hdc: HDC, lppt: *const POINT, cCount: DWORD) -> BOOL {
        TRUE
    }
    
    /// PolyDraw
    pub unsafe fn poly_draw(hdc: HDC, lppt: *const POINT, lpbTypes: *const BYTE, cCount: INT) -> BOOL {
        TRUE
    }
    
    /// Arc
    pub unsafe fn arc(
        hdc: HDC,
        x1: INT, y1: INT,
        x2: INT, y2: INT,
        x3: INT, y3: INT,
        x4: INT, y4: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// ArcTo
    pub unsafe fn arc_to(
        hdc: HDC,
        left: INT, top: INT,
        right: INT, bottom: INT,
        xr1: INT, yr1: INT,
        xr2: INT, yr2: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// Chord
    pub unsafe fn chord(
        hdc: HDC,
        x1: INT, y1: INT,
        x2: INT, y2: INT,
        x3: INT, y3: INT,
        x4: INT, y4: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// Ellipse
    pub unsafe fn ellipse(hdc: HDC, left: INT, top: INT, right: INT, bottom: INT) -> BOOL {
        crate::serial_println!("[WIN32] Ellipse: {},{} {},{}", left, top, right, bottom);
        TRUE
    }
    
    /// Pie
    pub unsafe fn pie(
        hdc: HDC,
        x1: INT, y1: INT,
        x2: INT, y2: INT,
        x3: INT, y3: INT,
        x4: INT, y4: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// RoundRect
    pub unsafe fn round_rect(hdc: HDC, left: INT, top: INT, right: INT, bottom: INT, width: INT, height: INT) -> BOOL {
        TRUE
    }
    
    /// Polygon
    pub unsafe fn polygon(hdc: HDC, lpPoints: *const POINT, nCount: INT) -> BOOL {
        TRUE
    }
    
    /// PolyPolygon
    pub unsafe fn poly_polygon(hdc: HDC, lpPoints: *const POINT, lpPolyCounts: *const INT, nCount: INT) -> BOOL {
        TRUE
    }
    
    /// PolyBezier
    pub unsafe fn poly_bezier(hdc: HDC, lppt: *const POINT, cPoints: DWORD) -> BOOL {
        TRUE
    }
    
    /// PolyBezierTo
    pub unsafe fn poly_bezier_to(hdc: HDC, lppt: *const POINT, cCount: DWORD) -> BOOL {
        TRUE
    }
    
    /// AngleArc
    pub unsafe fn angle_arc(hdc: HDC, x: INT, y: INT, r: DWORD, StartAngle: f32, SweepAngle: f32) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // FILLED SHAPES
    // ========================================================================
    
    /// FillRect
    pub unsafe fn fill_rect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> INT {
        if lprc.is_null() {
            return 0;
        }
        let rect = &*lprc;
        crate::serial_println!("[WIN32] FillRect: {},{} {},{}", rect.left, rect.top, rect.right, rect.bottom);
        1
    }
    
    /// FrameRect
    pub unsafe fn frame_rect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> BOOL {
        TRUE
    }
    
    /// InvertRect
    pub unsafe fn invert_rect(hDC: HDC, lprc: *const RECT) -> BOOL {
        TRUE
    }
    
    /// DrawFocusRect
    pub unsafe fn draw_focus_rect(hDC: HDC, lprc: *const RECT) -> BOOL {
        TRUE
    }
    
    /// ExtFloodFill
    pub unsafe fn ext_flood_fill(hdc: HDC, x: INT, y: INT, crColor: DWORD, fuFillType: UINT) -> BOOL {
        TRUE
    }
    
    /// FloodFill
    pub unsafe fn flood_fill(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> BOOL {
        TRUE
    }
    
    /// GradientFill
    pub unsafe fn gradient_fill(hdc: HDC, pVertex: *const u8, nVertex: ULONG, pMesh: *const u8, nMesh: ULONG, ulMode: ULONG) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // BITMAPS
    // ========================================================================
    
    /// CreateBitmap
    pub unsafe fn create_bitmap(nWidth: INT, nHeight: INT, nPlanes: UINT, nBitCount: UINT, lpBits: *const u8) -> HBITMAP {
        crate::serial_println!("[WIN32] CreateBitmap: {}x{}", nWidth, nHeight);
        1 as HBITMAP
    }
    
    /// CreateBitmapIndirect
    pub unsafe fn create_bitmap_indirect(lpbm: *const u8) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// CreateCompatibleBitmap
    pub unsafe fn create_compatible_bitmap(hdc: HDC, cx: INT, cy: INT) -> HBITMAP {
        crate::serial_println!("[WIN32] CreateCompatibleBitmap: {}x{}", cx, cy);
        1 as HBITMAP
    }
    
    /// CreateDiscardableBitmap
    pub unsafe fn create_discardable_bitmap(hdc: HDC, cx: INT, cy: INT) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// GetBitmapBits
    pub unsafe fn get_bitmap_bits(hbmp: HBITMAP, cbBuffer: LONG, lpvBits: LPVOID) -> LONG {
        0
    }
    
    /// SetBitmapBits
    pub unsafe fn set_bitmap_bits(hbmp: HBITMAP, cBytes: DWORD, lpBits: *const u8) -> LONG {
        cBytes as LONG
    }
    
    /// GetBitmapDimensionEx
    pub unsafe fn get_bitmap_dimension_ex(hBitmap: HBITMAP, lpDimension: *mut SIZE) -> BOOL {
        if !lpDimension.is_null() {
            (*lpDimension).cx = 0;
            (*lpDimension).cy = 0;
        }
        TRUE
    }
    
    /// SetBitmapDimensionEx
    pub unsafe fn set_bitmap_dimension_ex(hBitmap: HBITMAP, nX: INT, nY: INT, lpSize: *mut SIZE) -> BOOL {
        TRUE
    }
    
    /// GetDIBits
    pub unsafe fn get_dibits(
        hdc: HDC,
        hbmp: HBITMAP,
        uStartScan: UINT,
        cScanLines: UINT,
        lpvBits: LPVOID,
        lpbmi: *mut u8,
        uUsage: UINT,
    ) -> INT {
        0
    }
    
    /// SetDIBits
    pub unsafe fn set_dibits(
        hdc: HDC,
        hbmp: HBITMAP,
        uStartScan: UINT,
        cScanLines: UINT,
        lpBits: *const u8,
        lpbmi: *const u8,
        fuColorUse: UINT,
    ) -> INT {
        0
    }
    
    /// SetDIBitsToDevice
    pub unsafe fn set_dibits_to_device(
        hdc: HDC,
        xDest: INT, yDest: INT,
        w: DWORD, h: DWORD,
        xSrc: INT, ySrc: INT,
        uStartScan: UINT,
        cScanLines: UINT,
        lpvBits: *const u8,
        lpbmi: *const u8,
        fuColorUse: UINT,
    ) -> INT {
        0
    }
    
    /// StretchDIBits
    pub unsafe fn stretch_dibits(
        hdc: HDC,
        xDest: INT, yDest: INT,
        wDest: INT, hDest: INT,
        xSrc: INT, ySrc: INT,
        wSrc: INT, hSrc: INT,
        lpBits: *const u8,
        lpBitsInfo: *const u8,
        iUsage: UINT,
        dwRop: DWORD,
    ) -> INT {
        0
    }
    
    /// CreateDIBitmap
    pub unsafe fn create_dibitmap(
        hdc: HDC,
        lpbmih: *const u8,
        fdwInit: DWORD,
        lpbInit: *const u8,
        lpbmi: *const u8,
        fuUsage: UINT,
    ) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// CreateDIBSection
    pub unsafe fn create_dib_section(
        hdc: HDC,
        lpbmi: *const u8,
        usage: UINT,
        ppvBits: *mut LPVOID,
        hSection: HANDLE,
        dwOffset: DWORD,
    ) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// GetDIBColorTable
    pub unsafe fn get_dib_color_table(hdc: HDC, uStartIndex: UINT, cEntries: UINT, pColors: *mut u8) -> UINT {
        0
    }
    
    /// SetDIBColorTable
    pub unsafe fn set_dib_color_table(hdc: HDC, uStartIndex: UINT, cEntries: UINT, pColors: *const u8) -> UINT {
        0
    }
    
    // ========================================================================
    // BRUSHES
    // ========================================================================
    
    /// CreateSolidBrush
    pub unsafe fn create_solid_brush(crColor: DWORD) -> HBRUSH {
        crate::serial_println!("[WIN32] CreateSolidBrush: {:08x}", crColor);
        crColor as HBRUSH
    }
    
    /// CreateHatchBrush
    pub unsafe fn create_hatch_brush(fnStyle: INT, clrref: DWORD) -> HBRUSH {
        clrref as HBRUSH
    }
    
    /// CreatePatternBrush
    pub unsafe fn create_pattern_brush(hbmp: HBITMAP) -> HBRUSH {
        hbmp as HBRUSH
    }
    
    /// CreateDIBPatternBrush
    pub unsafe fn create_dib_pattern_brush(hbmp: HBITMAP, fuColorUse: UINT) -> HBRUSH {
        hbmp as HBRUSH
    }
    
    /// CreateDIBPatternBrushPt
    pub unsafe fn create_dib_pattern_brush_pt(lpPackedDIB: *const u8, iUsage: UINT) -> HBRUSH {
        1 as HBRUSH
    }
    
    /// CreateBrushIndirect
    pub unsafe fn create_brush_indirect(lplb: *const u8) -> HBRUSH {
        1 as HBRUSH
    }
    
    /// GetBrushOrgEx
    pub unsafe fn get_brush_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// SetBrushOrgEx
    pub unsafe fn set_brush_org_ex(hdc: HDC, nXOrg: INT, nYOrg: INT, lppt: *mut POINT) -> BOOL {
        TRUE
    }
    
    /// GetSysColorBrush
    pub unsafe fn get_sys_color_brush(nIndex: INT) -> HBRUSH {
        nIndex as HBRUSH
    }
    
    // ========================================================================
    // PENS
    // ========================================================================
    
    /// CreatePen
    pub unsafe fn create_pen(fnPenStyle: INT, nWidth: INT, crColor: DWORD) -> HPEN {
        crColor as HPEN
    }
    
    /// CreatePenIndirect
    pub unsafe fn create_pen_indirect(lplgpn: *const u8) -> HPEN {
        1 as HPEN
    }
    
    /// ExtCreatePen
    pub unsafe fn ext_create_pen(dwPenStyle: DWORD, dwWidth: DWORD, lplb: *const u8, dwStyleCount: DWORD, lpStyle: *const DWORD) -> HPEN {
        1 as HPEN
    }
    
    /// GetObjectA
    pub unsafe fn get_object_a(hgdiobj: HGDIOBJ, cbBuffer: INT, lpvObject: LPVOID) -> INT {
        0
    }
    
    /// GetObjectW
    pub unsafe fn get_object_w(hgdiobj: HGDIOBJ, cbBuffer: INT, lpvObject: LPVOID) -> INT {
        0
    }
    
    /// GetCurrentObject
    pub unsafe fn get_current_object(hdc: HDC, uObjectType: UINT) -> HGDIOBJ {
        1 as HGDIOBJ
    }
    
    // ========================================================================
    // FONTS AND TEXT
    // ========================================================================
    
    /// CreateFontA
    pub unsafe fn create_font_a(
        cHeight: INT,
        cWidth: INT,
        cEscapement: INT,
        cOrientation: INT,
        cWeight: INT,
        bItalic: DWORD,
        bUnderline: DWORD,
        bStrikeOut: DWORD,
        iCharSet: DWORD,
        iOutPrecision: DWORD,
        iClipPrecision: DWORD,
        iQuality: DWORD,
        iPitchAndFamily: DWORD,
        pszFaceName: LPCSTR,
    ) -> HFONT {
        let mut name = String::new();
        let mut ptr = pszFaceName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] CreateFontA: {} ({}pt)", name, cHeight);
        1 as HFONT
    }
    
    /// CreateFontIndirectA
    pub unsafe fn create_font_indirect_a(lplf: *const u8) -> HFONT {
        1 as HFONT
    }
    
    /// CreateFontIndirectExA
    pub unsafe fn create_font_indirect_ex_a(penumlfex: *const u8) -> HFONT {
        1 as HFONT
    }
    
    /// GetTextFaceA
    pub unsafe fn get_text_face_a(hdc: HDC, nCount: INT, lpFaceName: LPSTR) -> INT {
        0
    }
    
    /// GetTextMetricsA
    pub unsafe fn get_text_metrics_a(hdc: HDC, lptm: *mut u8) -> BOOL {
        TRUE
    }
    
    /// GetTextExtentPointA
    pub unsafe fn get_text_extent_point_a(hdc: HDC, lpString: LPCSTR, cbString: INT, lpSize: *mut SIZE) -> BOOL {
        if !lpSize.is_null() {
            (*lpSize).cx = cbString * 8;
            (*lpSize).cy = 16;
        }
        TRUE
    }
    
    /// GetTextExtentPoint32A
    pub unsafe fn get_text_extent_point_32_a(hdc: HDC, lpString: LPCSTR, c: INT, psizl: *mut SIZE) -> BOOL {
        TRUE
    }
    
    /// GetTextExtentExPointA
    pub unsafe fn get_text_extent_ex_point_a(
        hdc: HDC,
        lpszString: LPCSTR,
        cchString: INT,
        nMaxExtent: INT,
        lpnFit: *mut INT,
        lpnDx: *mut INT,
        lpSize: *mut SIZE,
    ) -> BOOL {
        TRUE
    }
    
    /// GetCharWidthA
    pub unsafe fn get_char_width_a(hdc: HDC, iFirst: UINT, iLast: UINT, lpBuffer: *mut INT) -> BOOL {
        TRUE
    }
    
    /// GetCharWidth32A
    pub unsafe fn get_char_width_32_a(hdc: HDC, iFirst: UINT, iLast: UINT, lpBuffer: *mut INT) -> BOOL {
        TRUE
    }
    
    /// GetCharABCWidthsA
    pub unsafe fn get_char_abc_widths_a(hdc: HDC, uFirst: UINT, uLast: UINT, lpabc: *mut u8) -> BOOL {
        TRUE
    }
    
    /// SetTextAlign
    pub unsafe fn set_text_align(hdc: HDC, fMode: UINT) -> UINT {
        0
    }
    
    /// GetTextAlign
    pub unsafe fn get_text_align(hdc: HDC) -> UINT {
        0
    }
    
    /// SetTextColor
    pub unsafe fn set_text_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crate::serial_println!("[WIN32] SetTextColor: {:08x}", crColor);
        0
    }
    
    /// GetTextColor
    pub unsafe fn get_text_color(hdc: HDC) -> DWORD {
        0
    }
    
    /// SetBkColor
    pub unsafe fn set_bk_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crate::serial_println!("[WIN32] SetBkColor: {:08x}", crColor);
        0
    }
    
    /// GetBkColor
    pub unsafe fn get_bk_color(hdc: HDC) -> DWORD {
        0
    }
    
    /// SetBkMode
    pub unsafe fn set_bk_mode(hdc: HDC, iBkMode: INT) -> INT {
        0
    }
    
    /// GetBkMode
    pub unsafe fn get_bk_mode(hdc: HDC) -> INT {
        0
    }
    
    /// TextOutA
    pub unsafe fn text_out_a(hdc: HDC, x: INT, y: INT, lpString: LPCSTR, c: INT) -> BOOL {
        let mut text = String::new();
        let mut ptr = lpString;
        for _ in 0..c {
            if ptr.is_null() || *ptr == 0 { break; }
            text.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] TextOutA: {},{} \"{}\"", x, y, text);
        TRUE
    }
    
    /// ExtTextOutA
    pub unsafe fn ext_text_out_a(
        hdc: HDC,
        x: INT, y: INT,
        fuOptions: UINT,
        lprc: *const RECT,
        lpString: LPCSTR,
        cbCount: UINT,
        lpDx: *const INT,
    ) -> BOOL {
        TRUE
    }
    
    /// DrawTextA
    pub unsafe fn draw_text_a(hdc: HDC, lpchText: LPCSTR, cchText: INT, lprc: *mut RECT, uFormat: UINT) -> INT {
        let mut text = String::new();
        let mut ptr = lpchText;
        for _ in 0..cchText {
            if ptr.is_null() || *ptr == 0 { break; }
            text.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::serial_println!("[WIN32] DrawTextA: \"{}\"", text);
        text.len() as INT
    }
    
    /// DrawTextExA
    pub unsafe fn draw_text_ex_a(
        hdc: HDC,
        lpchText: LPSTR,
        cchText: INT,
        lprc: *mut RECT,
        dwDTFormat: UINT,
        lpDTParams: *const u8,
    ) -> INT {
        0
    }
    
    /// TabbedTextOutA
    pub unsafe fn tabbed_text_out_a(
        hdc: HDC,
        x: INT, y: INT,
        lpString: LPCSTR,
        chCount: INT,
        nTabPositions: INT,
        lpnTabStopPositions: *const INT,
        nTabOrigin: INT,
    ) -> LONG {
        0
    }
    
    /// GetTabbedTextExtentA
    pub unsafe fn get_tabbed_text_extent_a(
        hdc: HDC,
        lpString: LPCSTR,
        nCount: INT,
        nTabPositions: INT,
        lpnTabStopPositions: *const INT,
    ) -> DWORD {
        0
    }
    
    /// PolyTextOutA
    pub unsafe fn poly_text_out_a(hdc: HDC, ppt: *const u8, nstrings: INT) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // REGIONS
    // ========================================================================
    
    /// CreateRectRgn
    pub unsafe fn create_rect_rgn(left: INT, top: INT, right: INT, bottom: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreateRectRgnIndirect
    pub unsafe fn create_rect_rgn_indirect(lprect: *const RECT) -> HRGN {
        1 as HRGN
    }
    
    /// CreateEllipticRgn
    pub unsafe fn create_elliptic_rgn(left: INT, top: INT, right: INT, bottom: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreateEllipticRgnIndirect
    pub unsafe fn create_elliptic_rgn_indirect(lprect: *const RECT) -> HRGN {
        1 as HRGN
    }
    
    /// CreateRoundRectRgn
    pub unsafe fn create_round_rect_rgn(left: INT, top: INT, right: INT, bottom: INT, nWidthEllipse: INT, nHeightEllipse: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreatePolygonRgn
    pub unsafe fn create_polygon_rgn(lppt: *const POINT, cPoints: INT, fnMode: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreatePolyPolygonRgn
    pub unsafe fn create_poly_polygon_rgn(lppt: *const POINT, lpPolyCounts: *const INT, nCount: INT, fnPolyFillMode: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CombineRgn
    pub unsafe fn combine_rgn(hrgnDest: HRGN, hrgnSrc1: HRGN, hrgnSrc2: HRGN, fnCombineMode: INT) -> INT {
        0 // NULLREGION
    }
    
    /// OffsetRgn
    pub unsafe fn offset_rgn(hrgn: HRGN, nXOffset: INT, nYOffset: INT) -> INT {
        0
    }
    
    /// InvertRgn
    pub unsafe fn invert_rgn(hdc: HDC, hrgn: HRGN) -> BOOL {
        TRUE
    }
    
    /// PaintRgn
    pub unsafe fn paint_rgn(hdc: HDC, hrgn: HRGN) -> BOOL {
        TRUE
    }
    
    /// FillRgn
    pub unsafe fn fill_rgn(hdc: HDC, hrgn: HRGN, hbr: HBRUSH) -> BOOL {
        TRUE
    }
    
    /// FrameRgn
    pub unsafe fn frame_rgn(hdc: HDC, hrgn: HRGN, hbr: HBRUSH, nWidth: INT, nHeight: INT) -> BOOL {
        TRUE
    }
    
    /// GetRgnBox
    pub unsafe fn get_rgn_box(hrgn: HRGN, lprc: *mut RECT) -> INT {
        0
    }
    
    /// PtInRegion
    pub unsafe fn pt_in_region(hrgn: HRGN, x: INT, y: INT) -> BOOL {
        FALSE
    }
    
    /// RectInRegion
    pub unsafe fn rect_in_region(hrgn: HRGN, lprect: *const RECT) -> BOOL {
        FALSE
    }
    
    /// EqualRgn
    pub unsafe fn equal_rgn(hrgn1: HRGN, hrgn2: HRGN) -> BOOL {
        FALSE
    }
    
    /// GetRegionData
    pub unsafe fn get_region_data(hrgn: HRGN, nCount: DWORD, lpRgnData: *mut u8) -> DWORD {
        0
    }
    
    /// SetRectRgn
    pub unsafe fn set_rect_rgn(hrgn: HRGN, left: INT, top: INT, right: INT, bottom: INT) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // CLIPPING
    // ========================================================================
    
    /// SelectClipRgn
    pub unsafe fn select_clip_rgn(hdc: HDC, hrgn: HRGN) -> INT {
        0
    }
    
    /// ExtSelectClipRgn
    pub unsafe fn ext_select_clip_rgn(hdc: HDC, hrgn: HRGN, fnMode: INT) -> INT {
        0
    }
    
    /// ExcludeClipRect
    pub unsafe fn exclude_clip_rect(hdc: HDC, left: INT, top: INT, right: INT, bottom: INT) -> INT {
        0
    }
    
    /// ExcludeUpdateRgn
    pub unsafe fn exclude_update_rgn(hdc: HDC, hwnd: HWND) -> INT {
        0
    }
    
    /// IntersectClipRect
    pub unsafe fn intersect_clip_rect(hdc: HDC, left: INT, top: INT, right: INT, bottom: INT) -> INT {
        0
    }
    
    /// OffsetClipRgn
    pub unsafe fn offset_clip_rgn(hdc: HDC, x: INT, y: INT) -> INT {
        0
    }
    
    /// SelectClipPath
    pub unsafe fn select_clip_path(hdc: HDC, iMode: INT) -> BOOL {
        TRUE
    }
    
    /// GetClipBox
    pub unsafe fn get_clip_box(hdc: HDC, lprect: *mut RECT) -> INT {
        0
    }
    
    /// GetClipRgn
    pub unsafe fn get_clip_rgn(hdc: HDC) -> HRGN {
        0
    }
    
    /// PtVisible
    pub unsafe fn pt_visible(hdc: HDC, x: INT, y: INT) -> BOOL {
        TRUE
    }
    
    /// RectVisible
    pub unsafe fn rect_visible(hdc: HDC, lprect: *const RECT) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // COORDINATES AND TRANSFORMS
    // ========================================================================
    
    /// SetMapMode
    pub unsafe fn set_map_mode(hdc: HDC, iMode: INT) -> INT {
        0
    }
    
    /// GetMapMode
    pub unsafe fn get_map_mode(hdc: HDC) -> INT {
        1 // MM_TEXT
    }
    
    /// SetViewportOrgEx
    pub unsafe fn set_viewport_org_ex(hdc: HDC, x: INT, y: INT, lppt: *mut POINT) -> BOOL {
        TRUE
    }
    
    /// GetViewportOrgEx
    pub unsafe fn get_viewport_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// SetWindowOrgEx
    pub unsafe fn set_window_org_ex(hdc: HDC, x: INT, y: INT, lppt: *mut POINT) -> BOOL {
        TRUE
    }
    
    /// GetWindowOrgEx
    pub unsafe fn get_window_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// SetViewportExtEx
    pub unsafe fn set_viewport_ext_ex(hdc: HDC, x: INT, y: INT, lpsize: *mut SIZE) -> BOOL {
        TRUE
    }
    
    /// GetViewportExtEx
    pub unsafe fn get_viewport_ext_ex(hdc: HDC, lpsize: *mut SIZE) -> BOOL {
        if !lpsize.is_null() {
            (*lpsize).cx = 1;
            (*lpsize).cy = 1;
        }
        TRUE
    }
    
    /// SetWindowExtEx
    pub unsafe fn set_window_ext_ex(hdc: HDC, x: INT, y: INT, lpsize: *mut SIZE) -> BOOL {
        TRUE
    }
    
    /// GetWindowExtEx
    pub unsafe fn get_window_ext_ex(hdc: HDC, lpsize: *mut SIZE) -> BOOL {
        if !lpsize.is_null() {
            (*lpsize).cx = 1;
            (*lpsize).cy = 1;
        }
        TRUE
    }
    
    /// DPtoLP
    pub unsafe fn dp_to_lp(hdc: HDC, lppt: *mut POINT, c: INT) -> BOOL {
        TRUE
    }
    
    /// LPtoDP
    pub unsafe fn lp_to_dp(hdc: HDC, lppt: *mut POINT, c: INT) -> BOOL {
        TRUE
    }
    
    /// SetWorldTransform
    pub unsafe fn set_world_transform(hdc: HDC, lpxf: *const u8) -> BOOL {
        TRUE
    }
    
    /// GetWorldTransform
    pub unsafe fn get_world_transform(hdc: HDC, lpxf: *mut u8) -> BOOL {
        TRUE
    }
    
    /// ModifyWorldTransform
    pub unsafe fn modify_world_transform(hdc: HDC, lpxf: *const u8, iMode: DWORD) -> BOOL {
        TRUE
    }
    
    /// CombineTransform
    pub unsafe fn combine_transform(lpxfResult: *mut u8, lpxf1: *const u8, lpxf2: *const u8) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // COLORS
    // ========================================================================
    
    /// SetPixel
    pub unsafe fn set_pixel(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> DWORD {
        crate::serial_println!("[WIN32] SetPixel: {},{}", x, y);
        crColor
    }
    
    /// SetPixelV
    pub unsafe fn set_pixel_v(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> BOOL {
        TRUE
    }
    
    /// GetPixel
    pub unsafe fn get_pixel(hdc: HDC, x: INT, y: INT) -> DWORD {
        0
    }
    
    /// GetNearestColor
    pub unsafe fn get_nearest_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crColor
    }
    
    /// GetNearestPaletteIndex
    pub unsafe fn get_nearest_palette_index(hpal: HPALETTE, crColor: DWORD) -> UINT {
        0
    }
    
    // ========================================================================
    // PALETTES
    // ========================================================================
    
    /// CreatePalette
    pub unsafe fn create_palette(lplgpl: *const u8) -> HPALETTE {
        1 as HPALETTE
    }
    
    /// SelectPalette
    pub unsafe fn select_palette(hdc: HDC, hpal: HPALETTE, bForceBackground: BOOL) -> HPALETTE {
        hpal
    }
    
    /// RealizePalette
    pub unsafe fn realize_palette(hdc: HDC) -> UINT {
        0
    }
    
    /// UpdateColors
    pub unsafe fn update_colors(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// ResizePalette
    pub unsafe fn resize_palette(hpal: HPALETTE, n: UINT) -> BOOL {
        TRUE
    }
    
    /// AnimatePalette
    pub unsafe fn animate_palette(hpal: HPALETTE, iStartIndex: UINT, cEntries: UINT, ppe: *const u8) -> BOOL {
        TRUE
    }
    
    /// SetPaletteEntries
    pub unsafe fn set_palette_entries(hpal: HPALETTE, iStart: UINT, cEntries: UINT, ppe: *const u8) -> UINT {
        0
    }
    
    /// GetPaletteEntries
    pub unsafe fn get_palette_entries(hpal: HPALETTE, iStart: UINT, cEntries: UINT, ppe: *mut u8) -> UINT {
        0
    }
    
    /// GetSystemPaletteEntries
    pub unsafe fn get_system_palette_entries(hdc: HDC, iStart: UINT, cEntries: UINT, ppe: *mut u8) -> UINT {
        0
    }
    
    /// GetSystemPaletteUse
    pub unsafe fn get_system_palette_use(hdc: HDC) -> UINT {
        1 // SYSPAL_STATIC
    }
    
    /// SetSystemPaletteUse
    pub unsafe fn set_system_palette_use(hdc: HDC, uiUsage: UINT) -> UINT {
        1
    }
    
    // ========================================================================
    // PATHS
    // ========================================================================
    
    /// BeginPath
    pub unsafe fn begin_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// EndPath
    pub unsafe fn end_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// AbortPath
    pub unsafe fn abort_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// CloseFigure
    pub unsafe fn close_figure(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// FlattenPath
    pub unsafe fn flatten_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// WidenPath
    pub unsafe fn widen_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// StrokePath
    pub unsafe fn stroke_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// StrokeAndFillPath
    pub unsafe fn stroke_and_fill_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// FillPath
    pub unsafe fn fill_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// PathToRegion
    pub unsafe fn path_to_region(hdc: HDC) -> HRGN {
        1 as HRGN
    }
    
    /// GetPath
    pub unsafe fn get_path(hdc: HDC, lppt: *mut POINT, lpbTypes: *mut BYTE, nSize: INT) -> INT {
        -1
    }
    
    // ========================================================================
    // MISC
    // ========================================================================
    
    /// SaveDC
    pub unsafe fn save_dc(hdc: HDC) -> INT {
        1
    }
    
    /// RestoreDC
    pub unsafe fn restore_dc(hdc: HDC, nSavedDC: INT) -> BOOL {
        TRUE
    }
    
    /// GetCurrentPositionEx
    pub unsafe fn get_current_position_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// GetGraphicsMode
    pub unsafe fn get_graphics_mode(hdc: HDC) -> INT {
        1 // GM_COMPATIBLE
    }
    
    /// SetGraphicsMode
    pub unsafe fn set_graphics_mode(hdc: HDC, iMode: INT) -> INT {
        1
    }
    
    /// GetArcDirection
    pub unsafe fn get_arc_direction(hdc: HDC) -> INT {
        1 // AD_COUNTERCLOCKWISE
    }
    
    /// SetArcDirection
    pub unsafe fn set_arc_direction(hdc: HDC, dir: INT) -> INT {
        1
    }
    
    /// GetPolyFillMode
    pub unsafe fn get_poly_fill_mode(hdc: HDC) -> INT {
        1 // ALTERNATE
    }
    
    /// SetPolyFillMode
    pub unsafe fn set_poly_fill_mode(hdc: HDC, iMode: INT) -> INT {
        1
    }
    
    /// GetStretchBltMode
    pub unsafe fn get_stretch_blt_mode(hdc: HDC) -> INT {
        1 // WHITEONBLACK
    }
    
    /// SetStretchBltMode
    pub unsafe fn set_stretch_blt_mode(hdc: HDC, iStretchMode: INT) -> INT {
        1
    }
    
    /// GetROP2
    pub unsafe fn get_rop2(hdc: HDC) -> INT {
        13 // R2_COPYPEN
    }
    
    /// SetROP2
    pub unsafe fn set_rop2(hdc: HDC, fnDrawMode: INT) -> INT {
        13
    }
    
    /// GetDCOrgEx
    pub unsafe fn get_dc_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
}

// ============================================================================
// API TABLE
// ============================================================================

/// Initialize Win32 API table
fn init_api_table() -> BTreeMap<String, BTreeMap<String, Win32ApiFn>> {
    let mut table: BTreeMap<String, BTreeMap<String, Win32ApiFn>> = BTreeMap::new();
    
    // kernel32
    let mut kernel32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    kernel32_funcs.insert("GetModuleHandleA".to_string(), stub_api);
    kernel32_funcs.insert("LoadLibraryA".to_string(), stub_api);
    kernel32_funcs.insert("GetProcAddress".to_string(), stub_api);
    kernel32_funcs.insert("VirtualAlloc".to_string(), stub_api);
    kernel32_funcs.insert("VirtualFree".to_string(), stub_api);
    kernel32_funcs.insert("VirtualProtect".to_string(), stub_api);
    kernel32_funcs.insert("VirtualQuery".to_string(), stub_api);
    kernel32_funcs.insert("GetTickCount".to_string(), stub_api);
    kernel32_funcs.insert("Sleep".to_string(), stub_api);
    kernel32_funcs.insert("CreateFileA".to_string(), stub_api);
    kernel32_funcs.insert("ReadFile".to_string(), stub_api);
    kernel32_funcs.insert("WriteFile".to_string(), stub_api);
    kernel32_funcs.insert("CloseHandle".to_string(), stub_api);
    kernel32_funcs.insert("ExitProcess".to_string(), stub_api);
    // Process
    kernel32_funcs.insert("CreateProcessA".to_string(), stub_api);
    kernel32_funcs.insert("OpenProcess".to_string(), stub_api);
    kernel32_funcs.insert("TerminateProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetExitCodeProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentProcessId".to_string(), stub_api);
    // Thread
    kernel32_funcs.insert("CreateThread".to_string(), stub_api);
    kernel32_funcs.insert("ExitThread".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentThread".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentThreadId".to_string(), stub_api);
    kernel32_funcs.insert("ResumeThread".to_string(), stub_api);
    kernel32_funcs.insert("SuspendThread".to_string(), stub_api);
    kernel32_funcs.insert("WaitForSingleObject".to_string(), stub_api);
    kernel32_funcs.insert("WaitForMultipleObjects".to_string(), stub_api);
    // Heap
    kernel32_funcs.insert("HeapCreate".to_string(), stub_api);
    kernel32_funcs.insert("HeapDestroy".to_string(), stub_api);
    kernel32_funcs.insert("HeapAlloc".to_string(), stub_api);
    kernel32_funcs.insert("HeapFree".to_string(), stub_api);
    kernel32_funcs.insert("HeapReAlloc".to_string(), stub_api);
    kernel32_funcs.insert("HeapSize".to_string(), stub_api);
    kernel32_funcs.insert("GetProcessHeap".to_string(), stub_api);
    kernel32_funcs.insert("LocalAlloc".to_string(), stub_api);
    kernel32_funcs.insert("LocalFree".to_string(), stub_api);
    kernel32_funcs.insert("GlobalAlloc".to_string(), stub_api);
    kernel32_funcs.insert("GlobalFree".to_string(), stub_api);
    // File
    kernel32_funcs.insert("SetFilePointer".to_string(), stub_api);
    kernel32_funcs.insert("SetFilePointerEx".to_string(), stub_api);
    kernel32_funcs.insert("GetFileSize".to_string(), stub_api);
    kernel32_funcs.insert("GetFileSizeEx".to_string(), stub_api);
    kernel32_funcs.insert("GetFileAttributesA".to_string(), stub_api);
    kernel32_funcs.insert("SetFileAttributesA".to_string(), stub_api);
    kernel32_funcs.insert("DeleteFileA".to_string(), stub_api);
    kernel32_funcs.insert("MoveFileA".to_string(), stub_api);
    kernel32_funcs.insert("CopyFileA".to_string(), stub_api);
    kernel32_funcs.insert("FindFirstFileA".to_string(), stub_api);
    kernel32_funcs.insert("FindNextFileA".to_string(), stub_api);
    kernel32_funcs.insert("FindClose".to_string(), stub_api);
    kernel32_funcs.insert("CreateDirectoryA".to_string(), stub_api);
    kernel32_funcs.insert("RemoveDirectoryA".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentDirectoryA".to_string(), stub_api);
    kernel32_funcs.insert("SetCurrentDirectoryA".to_string(), stub_api);
    // Console
    kernel32_funcs.insert("GetStdHandle".to_string(), stub_api);
    kernel32_funcs.insert("SetStdHandle".to_string(), stub_api);
    kernel32_funcs.insert("WriteConsoleA".to_string(), stub_api);
    kernel32_funcs.insert("ReadConsoleA".to_string(), stub_api);
    kernel32_funcs.insert("SetConsoleMode".to_string(), stub_api);
    kernel32_funcs.insert("GetConsoleMode".to_string(), stub_api);
    kernel32_funcs.insert("SetConsoleTextAttribute".to_string(), stub_api);
    kernel32_funcs.insert("GetConsoleScreenBufferInfo".to_string(), stub_api);
    kernel32_funcs.insert("FillConsoleOutputCharacterA".to_string(), stub_api);
    // Environment
    kernel32_funcs.insert("GetEnvironmentVariableA".to_string(), stub_api);
    kernel32_funcs.insert("SetEnvironmentVariableA".to_string(), stub_api);
    kernel32_funcs.insert("GetCommandLineA".to_string(), stub_api);
    // System
    kernel32_funcs.insert("GetSystemInfo".to_string(), stub_api);
    kernel32_funcs.insert("GlobalMemoryStatus".to_string(), stub_api);
    kernel32_funcs.insert("GlobalMemoryStatusEx".to_string(), stub_api);
    kernel32_funcs.insert("GetVersion".to_string(), stub_api);
    kernel32_funcs.insert("GetVersionExA".to_string(), stub_api);
    kernel32_funcs.insert("GetComputerNameA".to_string(), stub_api);
    kernel32_funcs.insert("GetUserNameA".to_string(), stub_api);
    kernel32_funcs.insert("GetLastError".to_string(), stub_api);
    kernel32_funcs.insert("SetLastError".to_string(), stub_api);
    // String
    kernel32_funcs.insert("MultiByteToWideChar".to_string(), stub_api);
    kernel32_funcs.insert("WideCharToMultiByte".to_string(), stub_api);
    kernel32_funcs.insert("lstrlenA".to_string(), stub_api);
    kernel32_funcs.insert("lstrlenW".to_string(), stub_api);
    kernel32_funcs.insert("lstrcpyA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcatA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcmpA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcmpiA".to_string(), stub_api);
    table.insert("kernel32".to_string(), kernel32_funcs);
    
    // user32
    let mut user32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    user32_funcs.insert("RegisterClassA".to_string(), stub_api);
    user32_funcs.insert("RegisterClassExA".to_string(), stub_api);
    user32_funcs.insert("UnregisterClassA".to_string(), stub_api);
    user32_funcs.insert("CreateWindowExA".to_string(), stub_api);
    user32_funcs.insert("DestroyWindow".to_string(), stub_api);
    user32_funcs.insert("ShowWindow".to_string(), stub_api);
    user32_funcs.insert("UpdateWindow".to_string(), stub_api);
    user32_funcs.insert("GetMessageA".to_string(), stub_api);
    user32_funcs.insert("PeekMessageA".to_string(), stub_api);
    user32_funcs.insert("TranslateMessage".to_string(), stub_api);
    user32_funcs.insert("DispatchMessageA".to_string(), stub_api);
    user32_funcs.insert("PostQuitMessage".to_string(), stub_api);
    user32_funcs.insert("PostMessageA".to_string(), stub_api);
    user32_funcs.insert("SendMessageA".to_string(), stub_api);
    user32_funcs.insert("DefWindowProcA".to_string(), stub_api);
    user32_funcs.insert("GetDC".to_string(), stub_api);
    user32_funcs.insert("ReleaseDC".to_string(), stub_api);
    user32_funcs.insert("SetWindowTextA".to_string(), stub_api);
    user32_funcs.insert("GetWindowTextA".to_string(), stub_api);
    user32_funcs.insert("GetWindowTextLengthA".to_string(), stub_api);
    user32_funcs.insert("GetClientRect".to_string(), stub_api);
    user32_funcs.insert("GetWindowRect".to_string(), stub_api);
    user32_funcs.insert("MoveWindow".to_string(), stub_api);
    user32_funcs.insert("SetWindowPos".to_string(), stub_api);
    user32_funcs.insert("IsWindow".to_string(), stub_api);
    user32_funcs.insert("IsWindowVisible".to_string(), stub_api);
    user32_funcs.insert("IsWindowEnabled".to_string(), stub_api);
    user32_funcs.insert("EnableWindow".to_string(), stub_api);
    user32_funcs.insert("GetParent".to_string(), stub_api);
    user32_funcs.insert("SetParent".to_string(), stub_api);
    user32_funcs.insert("GetDesktopWindow".to_string(), stub_api);
    user32_funcs.insert("GetForegroundWindow".to_string(), stub_api);
    user32_funcs.insert("SetForegroundWindow".to_string(), stub_api);
    user32_funcs.insert("GetActiveWindow".to_string(), stub_api);
    user32_funcs.insert("SetActiveWindow".to_string(), stub_api);
    user32_funcs.insert("GetFocus".to_string(), stub_api);
    user32_funcs.insert("SetFocus".to_string(), stub_api);
    user32_funcs.insert("GetCapture".to_string(), stub_api);
    user32_funcs.insert("SetCapture".to_string(), stub_api);
    user32_funcs.insert("ReleaseCapture".to_string(), stub_api);
    user32_funcs.insert("FindWindowA".to_string(), stub_api);
    user32_funcs.insert("FindWindowExA".to_string(), stub_api);
    user32_funcs.insert("GetWindow".to_string(), stub_api);
    user32_funcs.insert("EnumWindows".to_string(), stub_api);
    user32_funcs.insert("GetClassNameA".to_string(), stub_api);
    // Keyboard
    user32_funcs.insert("GetKeyState".to_string(), stub_api);
    user32_funcs.insert("GetAsyncKeyState".to_string(), stub_api);
    user32_funcs.insert("GetKeyboardState".to_string(), stub_api);
    user32_funcs.insert("SetKeyboardState".to_string(), stub_api);
    user32_funcs.insert("keybd_event".to_string(), stub_api);
    user32_funcs.insert("MapVirtualKeyA".to_string(), stub_api);
    user32_funcs.insert("ToAscii".to_string(), stub_api);
    user32_funcs.insert("VkKeyScanA".to_string(), stub_api);
    // Mouse
    user32_funcs.insert("GetCursorPos".to_string(), stub_api);
    user32_funcs.insert("SetCursorPos".to_string(), stub_api);
    user32_funcs.insert("mouse_event".to_string(), stub_api);
    user32_funcs.insert("GetDoubleClickTime".to_string(), stub_api);
    user32_funcs.insert("SwapMouseButton".to_string(), stub_api);
    user32_funcs.insert("GetSystemMetrics".to_string(), stub_api);
    // Menus
    user32_funcs.insert("CreateMenu".to_string(), stub_api);
    user32_funcs.insert("CreatePopupMenu".to_string(), stub_api);
    user32_funcs.insert("DestroyMenu".to_string(), stub_api);
    user32_funcs.insert("AppendMenuA".to_string(), stub_api);
    user32_funcs.insert("InsertMenuA".to_string(), stub_api);
    user32_funcs.insert("RemoveMenu".to_string(), stub_api);
    user32_funcs.insert("DeleteMenu".to_string(), stub_api);
    user32_funcs.insert("SetMenu".to_string(), stub_api);
    user32_funcs.insert("GetMenu".to_string(), stub_api);
    user32_funcs.insert("DrawMenuBar".to_string(), stub_api);
    user32_funcs.insert("TrackPopupMenu".to_string(), stub_api);
    // Dialogs
    user32_funcs.insert("MessageBoxA".to_string(), stub_api);
    user32_funcs.insert("MessageBoxExA".to_string(), stub_api);
    user32_funcs.insert("DialogBoxParamA".to_string(), stub_api);
    user32_funcs.insert("EndDialog".to_string(), stub_api);
    user32_funcs.insert("CreateDialogParamA".to_string(), stub_api);
    user32_funcs.insert("GetDlgItem".to_string(), stub_api);
    user32_funcs.insert("SetDlgItemTextA".to_string(), stub_api);
    user32_funcs.insert("GetDlgItemTextA".to_string(), stub_api);
    user32_funcs.insert("SetDlgItemInt".to_string(), stub_api);
    user32_funcs.insert("GetDlgItemInt".to_string(), stub_api);
    user32_funcs.insert("CheckDlgButton".to_string(), stub_api);
    user32_funcs.insert("CheckRadioButton".to_string(), stub_api);
    user32_funcs.insert("IsDlgButtonChecked".to_string(), stub_api);
    // Timers
    user32_funcs.insert("SetTimer".to_string(), stub_api);
    user32_funcs.insert("KillTimer".to_string(), stub_api);
    // Clipboard
    user32_funcs.insert("OpenClipboard".to_string(), stub_api);
    user32_funcs.insert("CloseClipboard".to_string(), stub_api);
    user32_funcs.insert("EmptyClipboard".to_string(), stub_api);
    user32_funcs.insert("GetClipboardData".to_string(), stub_api);
    user32_funcs.insert("SetClipboardData".to_string(), stub_api);
    user32_funcs.insert("IsClipboardFormatAvailable".to_string(), stub_api);
    // Resources
    user32_funcs.insert("LoadIconA".to_string(), stub_api);
    user32_funcs.insert("LoadCursorA".to_string(), stub_api);
    user32_funcs.insert("LoadBitmapA".to_string(), stub_api);
    user32_funcs.insert("LoadStringA".to_string(), stub_api);
    user32_funcs.insert("LoadImageA".to_string(), stub_api);
    user32_funcs.insert("DestroyIcon".to_string(), stub_api);
    user32_funcs.insert("DestroyCursor".to_string(), stub_api);
    user32_funcs.insert("SetCursor".to_string(), stub_api);
    user32_funcs.insert("GetCursor".to_string(), stub_api);
    // Hooks
    user32_funcs.insert("SetWindowsHookExA".to_string(), stub_api);
    user32_funcs.insert("UnhookWindowsHookEx".to_string(), stub_api);
    user32_funcs.insert("CallNextHookEx".to_string(), stub_api);
    // Misc
    user32_funcs.insert("GetWindowLongA".to_string(), stub_api);
    user32_funcs.insert("SetWindowLongA".to_string(), stub_api);
    user32_funcs.insert("GetWindowLongPtrA".to_string(), stub_api);
    user32_funcs.insert("SetWindowLongPtrA".to_string(), stub_api);
    user32_funcs.insert("GetClassLongA".to_string(), stub_api);
    user32_funcs.insert("SetClassLongA".to_string(), stub_api);
    user32_funcs.insert("GetPropA".to_string(), stub_api);
    user32_funcs.insert("SetPropA".to_string(), stub_api);
    user32_funcs.insert("RemovePropA".to_string(), stub_api);
    user32_funcs.insert("GetWindowThreadProcessId".to_string(), stub_api);
    user32_funcs.insert("AttachThreadInput".to_string(), stub_api);
    table.insert("user32".to_string(), user32_funcs);
    
    // gdi32
    let mut gdi32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // DC management
    gdi32_funcs.insert("CreateCompatibleDC".to_string(), stub_api);
    gdi32_funcs.insert("DeleteDC".to_string(), stub_api);
    gdi32_funcs.insert("SaveDC".to_string(), stub_api);
    gdi32_funcs.insert("RestoreDC".to_string(), stub_api);
    // Objects
    gdi32_funcs.insert("SelectObject".to_string(), stub_api);
    gdi32_funcs.insert("DeleteObject".to_string(), stub_api);
    gdi32_funcs.insert("GetStockObject".to_string(), stub_api);
    gdi32_funcs.insert("GetObjectA".to_string(), stub_api);
    gdi32_funcs.insert("GetCurrentObject".to_string(), stub_api);
    // Drawing
    gdi32_funcs.insert("MoveToEx".to_string(), stub_api);
    gdi32_funcs.insert("LineTo".to_string(), stub_api);
    gdi32_funcs.insert("Polyline".to_string(), stub_api);
    gdi32_funcs.insert("Arc".to_string(), stub_api);
    gdi32_funcs.insert("Ellipse".to_string(), stub_api);
    gdi32_funcs.insert("Rectangle".to_string(), stub_api);
    gdi32_funcs.insert("RoundRect".to_string(), stub_api);
    gdi32_funcs.insert("Polygon".to_string(), stub_api);
    gdi32_funcs.insert("PolyBezier".to_string(), stub_api);
    // Filled shapes
    gdi32_funcs.insert("FillRect".to_string(), stub_api);
    gdi32_funcs.insert("FrameRect".to_string(), stub_api);
    gdi32_funcs.insert("InvertRect".to_string(), stub_api);
    gdi32_funcs.insert("FloodFill".to_string(), stub_api);
    gdi32_funcs.insert("GradientFill".to_string(), stub_api);
    // Bitmaps
    gdi32_funcs.insert("CreateBitmap".to_string(), stub_api);
    gdi32_funcs.insert("CreateCompatibleBitmap".to_string(), stub_api);
    gdi32_funcs.insert("GetBitmapBits".to_string(), stub_api);
    gdi32_funcs.insert("SetBitmapBits".to_string(), stub_api);
    gdi32_funcs.insert("GetDIBits".to_string(), stub_api);
    gdi32_funcs.insert("SetDIBits".to_string(), stub_api);
    gdi32_funcs.insert("CreateDIBSection".to_string(), stub_api);
    // Blitting
    gdi32_funcs.insert("BitBlt".to_string(), stub_api);
    gdi32_funcs.insert("StretchBlt".to_string(), stub_api);
    gdi32_funcs.insert("StretchDIBits".to_string(), stub_api);
    gdi32_funcs.insert("PatBlt".to_string(), stub_api);
    // Brushes
    gdi32_funcs.insert("CreateSolidBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreateHatchBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreatePatternBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreateBrushIndirect".to_string(), stub_api);
    gdi32_funcs.insert("GetSysColorBrush".to_string(), stub_api);
    // Pens
    gdi32_funcs.insert("CreatePen".to_string(), stub_api);
    gdi32_funcs.insert("CreatePenIndirect".to_string(), stub_api);
    gdi32_funcs.insert("ExtCreatePen".to_string(), stub_api);
    // Fonts & Text
    gdi32_funcs.insert("CreateFontA".to_string(), stub_api);
    gdi32_funcs.insert("CreateFontIndirectA".to_string(), stub_api);
    gdi32_funcs.insert("GetTextFaceA".to_string(), stub_api);
    gdi32_funcs.insert("GetTextMetricsA".to_string(), stub_api);
    gdi32_funcs.insert("GetTextExtentPointA".to_string(), stub_api);
    gdi32_funcs.insert("GetTextExtentPoint32A".to_string(), stub_api);
    gdi32_funcs.insert("GetCharWidthA".to_string(), stub_api);
    gdi32_funcs.insert("SetTextAlign".to_string(), stub_api);
    gdi32_funcs.insert("SetTextColor".to_string(), stub_api);
    gdi32_funcs.insert("GetTextColor".to_string(), stub_api);
    gdi32_funcs.insert("SetBkColor".to_string(), stub_api);
    gdi32_funcs.insert("SetBkMode".to_string(), stub_api);
    gdi32_funcs.insert("TextOutA".to_string(), stub_api);
    gdi32_funcs.insert("ExtTextOutA".to_string(), stub_api);
    gdi32_funcs.insert("DrawTextA".to_string(), stub_api);
    gdi32_funcs.insert("DrawTextExA".to_string(), stub_api);
    // Regions
    gdi32_funcs.insert("CreateRectRgn".to_string(), stub_api);
    gdi32_funcs.insert("CreateEllipticRgn".to_string(), stub_api);
    gdi32_funcs.insert("CreateRoundRectRgn".to_string(), stub_api);
    gdi32_funcs.insert("CreatePolygonRgn".to_string(), stub_api);
    gdi32_funcs.insert("CombineRgn".to_string(), stub_api);
    gdi32_funcs.insert("OffsetRgn".to_string(), stub_api);
    gdi32_funcs.insert("FillRgn".to_string(), stub_api);
    gdi32_funcs.insert("FrameRgn".to_string(), stub_api);
    gdi32_funcs.insert("GetRgnBox".to_string(), stub_api);
    gdi32_funcs.insert("PtInRegion".to_string(), stub_api);
    gdi32_funcs.insert("RectInRegion".to_string(), stub_api);
    // Clipping
    gdi32_funcs.insert("SelectClipRgn".to_string(), stub_api);
    gdi32_funcs.insert("ExcludeClipRect".to_string(), stub_api);
    gdi32_funcs.insert("IntersectClipRect".to_string(), stub_api);
    gdi32_funcs.insert("GetClipBox".to_string(), stub_api);
    gdi32_funcs.insert("PtVisible".to_string(), stub_api);
    gdi32_funcs.insert("RectVisible".to_string(), stub_api);
    // Coordinates
    gdi32_funcs.insert("SetMapMode".to_string(), stub_api);
    gdi32_funcs.insert("GetMapMode".to_string(), stub_api);
    gdi32_funcs.insert("SetViewportOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("GetViewportOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("SetWindowOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("GetWindowOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("DPtoLP".to_string(), stub_api);
    gdi32_funcs.insert("LPtoDP".to_string(), stub_api);
    gdi32_funcs.insert("SetWorldTransform".to_string(), stub_api);
    // Colors
    gdi32_funcs.insert("SetPixel".to_string(), stub_api);
    gdi32_funcs.insert("SetPixelV".to_string(), stub_api);
    gdi32_funcs.insert("GetPixel".to_string(), stub_api);
    gdi32_funcs.insert("GetNearestColor".to_string(), stub_api);
    // Palettes
    gdi32_funcs.insert("CreatePalette".to_string(), stub_api);
    gdi32_funcs.insert("SelectPalette".to_string(), stub_api);
    gdi32_funcs.insert("RealizePalette".to_string(), stub_api);
    gdi32_funcs.insert("UpdateColors".to_string(), stub_api);
    gdi32_funcs.insert("GetPaletteEntries".to_string(), stub_api);
    // Paths
    gdi32_funcs.insert("BeginPath".to_string(), stub_api);
    gdi32_funcs.insert("EndPath".to_string(), stub_api);
    gdi32_funcs.insert("AbortPath".to_string(), stub_api);
    gdi32_funcs.insert("CloseFigure".to_string(), stub_api);
    gdi32_funcs.insert("FlattenPath".to_string(), stub_api);
    gdi32_funcs.insert("StrokePath".to_string(), stub_api);
    gdi32_funcs.insert("FillPath".to_string(), stub_api);
    gdi32_funcs.insert("PathToRegion".to_string(), stub_api);
    gdi32_funcs.insert("GetPath".to_string(), stub_api);
    // Misc
    gdi32_funcs.insert("GetGraphicsMode".to_string(), stub_api);
    gdi32_funcs.insert("SetGraphicsMode".to_string(), stub_api);
    gdi32_funcs.insert("GetPolyFillMode".to_string(), stub_api);
    gdi32_funcs.insert("SetPolyFillMode".to_string(), stub_api);
    gdi32_funcs.insert("GetStretchBltMode".to_string(), stub_api);
    gdi32_funcs.insert("SetStretchBltMode".to_string(), stub_api);
    gdi32_funcs.insert("GetROP2".to_string(), stub_api);
    gdi32_funcs.insert("SetROP2".to_string(), stub_api);
    gdi32_funcs.insert("GetCurrentPositionEx".to_string(), stub_api);
    // OpenGL
    gdi32_funcs.insert("ChoosePixelFormat".to_string(), stub_api);
    gdi32_funcs.insert("SetPixelFormat".to_string(), stub_api);
    gdi32_funcs.insert("SwapBuffers".to_string(), stub_api);
    table.insert("gdi32".to_string(), gdi32_funcs);
    
    // advapi32
    let mut advapi32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // Registry
    advapi32_funcs.insert("RegOpenKeyExA".to_string(), stub_api);
    advapi32_funcs.insert("RegCloseKey".to_string(), stub_api);
    advapi32_funcs.insert("RegCreateKeyExA".to_string(), stub_api);
    advapi32_funcs.insert("RegDeleteKeyA".to_string(), stub_api);
    advapi32_funcs.insert("RegDeleteValueA".to_string(), stub_api);
    advapi32_funcs.insert("RegEnumKeyExA".to_string(), stub_api);
    advapi32_funcs.insert("RegEnumValueA".to_string(), stub_api);
    advapi32_funcs.insert("RegQueryValueExA".to_string(), stub_api);
    advapi32_funcs.insert("RegSetValueExA".to_string(), stub_api);
    advapi32_funcs.insert("RegConnectRegistryA".to_string(), stub_api);
    advapi32_funcs.insert("RegNotifyChangeKeyValue".to_string(), stub_api);
    // Security
    advapi32_funcs.insert("GetUserNameA".to_string(), stub_api);
    advapi32_funcs.insert("LookupAccountNameA".to_string(), stub_api);
    advapi32_funcs.insert("LookupAccountSidA".to_string(), stub_api);
    advapi32_funcs.insert("InitializeSecurityDescriptor".to_string(), stub_api);
    advapi32_funcs.insert("InitializeAcl".to_string(), stub_api);
    advapi32_funcs.insert("AddAccessAllowedAce".to_string(), stub_api);
    advapi32_funcs.insert("SetSecurityDescriptorDacl".to_string(), stub_api);
    advapi32_funcs.insert("GetSecurityDescriptorDacl".to_string(), stub_api);
    advapi32_funcs.insert("IsValidSecurityDescriptor".to_string(), stub_api);
    advapi32_funcs.insert("GetLengthSid".to_string(), stub_api);
    advapi32_funcs.insert("CopySid".to_string(), stub_api);
    advapi32_funcs.insert("EqualSid".to_string(), stub_api);
    // Services
    advapi32_funcs.insert("OpenSCManagerA".to_string(), stub_api);
    advapi32_funcs.insert("CloseServiceHandle".to_string(), stub_api);
    advapi32_funcs.insert("OpenServiceA".to_string(), stub_api);
    advapi32_funcs.insert("CreateServiceA".to_string(), stub_api);
    advapi32_funcs.insert("DeleteService".to_string(), stub_api);
    advapi32_funcs.insert("StartServiceA".to_string(), stub_api);
    advapi32_funcs.insert("ControlService".to_string(), stub_api);
    advapi32_funcs.insert("QueryServiceStatus".to_string(), stub_api);
    advapi32_funcs.insert("EnumServicesStatusA".to_string(), stub_api);
    advapi32_funcs.insert("GetServiceKeyNameA".to_string(), stub_api);
    advapi32_funcs.insert("GetServiceDisplayNameA".to_string(), stub_api);
    // Event Log
    advapi32_funcs.insert("RegisterEventSourceA".to_string(), stub_api);
    advapi32_funcs.insert("DeregisterEventSource".to_string(), stub_api);
    advapi32_funcs.insert("ReportEventA".to_string(), stub_api);
    advapi32_funcs.insert("OpenEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("CloseEventLog".to_string(), stub_api);
    advapi32_funcs.insert("ClearEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("ReadEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("GetNumberOfEventLogRecords".to_string(), stub_api);
    // Crypto
    advapi32_funcs.insert("CryptAcquireContextA".to_string(), stub_api);
    advapi32_funcs.insert("CryptReleaseContext".to_string(), stub_api);
    advapi32_funcs.insert("CryptGenRandom".to_string(), stub_api);
    advapi32_funcs.insert("CryptCreateHash".to_string(), stub_api);
    advapi32_funcs.insert("CryptDestroyHash".to_string(), stub_api);
    advapi32_funcs.insert("CryptHashData".to_string(), stub_api);
    advapi32_funcs.insert("CryptGetHashParam".to_string(), stub_api);
    advapi32_funcs.insert("CryptDeriveKey".to_string(), stub_api);
    advapi32_funcs.insert("CryptDestroyKey".to_string(), stub_api);
    advapi32_funcs.insert("CryptEncrypt".to_string(), stub_api);
    advapi32_funcs.insert("CryptDecrypt".to_string(), stub_api);
    advapi32_funcs.insert("CryptImportKey".to_string(), stub_api);
    advapi32_funcs.insert("CryptExportKey".to_string(), stub_api);
    advapi32_funcs.insert("CryptSignHashA".to_string(), stub_api);
    advapi32_funcs.insert("CryptVerifySignatureA".to_string(), stub_api);
    // Process & Token
    advapi32_funcs.insert("CreateProcessAsUserA".to_string(), stub_api);
    advapi32_funcs.insert("OpenProcessToken".to_string(), stub_api);
    advapi32_funcs.insert("OpenThreadToken".to_string(), stub_api);
    advapi32_funcs.insert("DuplicateTokenEx".to_string(), stub_api);
    advapi32_funcs.insert("ImpersonateLoggedOnUser".to_string(), stub_api);
    advapi32_funcs.insert("RevertToSelf".to_string(), stub_api);
    advapi32_funcs.insert("GetTokenInformation".to_string(), stub_api);
    advapi32_funcs.insert("SetTokenInformation".to_string(), stub_api);
    advapi32_funcs.insert("AdjustTokenPrivileges".to_string(), stub_api);
    advapi32_funcs.insert("LookupPrivilegeValueA".to_string(), stub_api);
    advapi32_funcs.insert("LookupPrivilegeDisplayNameA".to_string(), stub_api);
    table.insert("advapi32".to_string(), advapi32_funcs);
    
    // shell32
    let mut shell32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    shell32_funcs.insert("ShellExecuteA".to_string(), stub_api);
    shell32_funcs.insert("ShellExecuteExA".to_string(), stub_api);
    shell32_funcs.insert("ShellAboutA".to_string(), stub_api);
    shell32_funcs.insert("ExtractIconA".to_string(), stub_api);
    shell32_funcs.insert("ExtractIconExA".to_string(), stub_api);
    shell32_funcs.insert("DragAcceptFiles".to_string(), stub_api);
    shell32_funcs.insert("DragQueryFileA".to_string(), stub_api);
    shell32_funcs.insert("DragQueryPoint".to_string(), stub_api);
    shell32_funcs.insert("DragFinish".to_string(), stub_api);
    shell32_funcs.insert("Shell_NotifyIconA".to_string(), stub_api);
    shell32_funcs.insert("SHGetPathFromIDListA".to_string(), stub_api);
    shell32_funcs.insert("SHBrowseForFolderA".to_string(), stub_api);
    shell32_funcs.insert("SHGetSpecialFolderPathA".to_string(), stub_api);
    shell32_funcs.insert("SHGetFolderPathA".to_string(), stub_api);
    shell32_funcs.insert("SHGetDesktopFolder".to_string(), stub_api);
    shell32_funcs.insert("SHGetFileInfoA".to_string(), stub_api);
    shell32_funcs.insert("SHFileOperationA".to_string(), stub_api);
    shell32_funcs.insert("SHEmptyRecycleBinA".to_string(), stub_api);
    shell32_funcs.insert("SHQueryRecycleBinA".to_string(), stub_api);
    table.insert("shell32".to_string(), shell32_funcs);
    
    // msvcrt
    let mut msvcrt_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // Memory
    msvcrt_funcs.insert("malloc".to_string(), stub_api);
    msvcrt_funcs.insert("free".to_string(), stub_api);
    msvcrt_funcs.insert("calloc".to_string(), stub_api);
    msvcrt_funcs.insert("realloc".to_string(), stub_api);
    msvcrt_funcs.insert("_msize".to_string(), stub_api);
    msvcrt_funcs.insert("_expand".to_string(), stub_api);
    msvcrt_funcs.insert("_heapmin".to_string(), stub_api);
    // String
    msvcrt_funcs.insert("strlen".to_string(), stub_api);
    msvcrt_funcs.insert("strcpy".to_string(), stub_api);
    msvcrt_funcs.insert("strncpy".to_string(), stub_api);
    msvcrt_funcs.insert("strcat".to_string(), stub_api);
    msvcrt_funcs.insert("strncat".to_string(), stub_api);
    msvcrt_funcs.insert("strcmp".to_string(), stub_api);
    msvcrt_funcs.insert("strncmp".to_string(), stub_api);
    msvcrt_funcs.insert("strchr".to_string(), stub_api);
    msvcrt_funcs.insert("strrchr".to_string(), stub_api);
    msvcrt_funcs.insert("strstr".to_string(), stub_api);
    msvcrt_funcs.insert("memcpy".to_string(), stub_api);
    msvcrt_funcs.insert("memmove".to_string(), stub_api);
    msvcrt_funcs.insert("memset".to_string(), stub_api);
    msvcrt_funcs.insert("memcmp".to_string(), stub_api);
    // IO
    msvcrt_funcs.insert("fopen".to_string(), stub_api);
    msvcrt_funcs.insert("fclose".to_string(), stub_api);
    msvcrt_funcs.insert("fread".to_string(), stub_api);
    msvcrt_funcs.insert("fwrite".to_string(), stub_api);
    msvcrt_funcs.insert("fseek".to_string(), stub_api);
    msvcrt_funcs.insert("ftell".to_string(), stub_api);
    msvcrt_funcs.insert("feof".to_string(), stub_api);
    msvcrt_funcs.insert("fgetc".to_string(), stub_api);
    msvcrt_funcs.insert("fputc".to_string(), stub_api);
    msvcrt_funcs.insert("fgets".to_string(), stub_api);
    msvcrt_funcs.insert("fputs".to_string(), stub_api);
    msvcrt_funcs.insert("fprintf".to_string(), stub_api);
    msvcrt_funcs.insert("printf".to_string(), stub_api);
    msvcrt_funcs.insert("sprintf".to_string(), stub_api);
    msvcrt_funcs.insert("snprintf".to_string(), stub_api);
    msvcrt_funcs.insert("scanf".to_string(), stub_api);
    // Math
    msvcrt_funcs.insert("abs".to_string(), stub_api);
    msvcrt_funcs.insert("labs".to_string(), stub_api);
    msvcrt_funcs.insert("rand".to_string(), stub_api);
    msvcrt_funcs.insert("srand".to_string(), stub_api);
    // Time
    msvcrt_funcs.insert("time".to_string(), stub_api);
    msvcrt_funcs.insert("clock".to_string(), stub_api);
    msvcrt_funcs.insert("localtime".to_string(), stub_api);
    msvcrt_funcs.insert("gmtime".to_string(), stub_api);
    msvcrt_funcs.insert("asctime".to_string(), stub_api);
    msvcrt_funcs.insert("ctime".to_string(), stub_api);
    msvcrt_funcs.insert("strftime".to_string(), stub_api);
    // Misc
    msvcrt_funcs.insert("exit".to_string(), stub_api);
    msvcrt_funcs.insert("abort".to_string(), stub_api);
    msvcrt_funcs.insert("system".to_string(), stub_api);
    msvcrt_funcs.insert("getenv".to_string(), stub_api);
    msvcrt_funcs.insert("atoi".to_string(), stub_api);
    msvcrt_funcs.insert("atol".to_string(), stub_api);
    msvcrt_funcs.insert("atof".to_string(), stub_api);
    msvcrt_funcs.insert("strtol".to_string(), stub_api);
    msvcrt_funcs.insert("strtoul".to_string(), stub_api);
    msvcrt_funcs.insert("strtod".to_string(), stub_api);
    msvcrt_funcs.insert("qsort".to_string(), stub_api);
    msvcrt_funcs.insert("bsearch".to_string(), stub_api);
    table.insert("msvcrt".to_string(), msvcrt_funcs);
    
    table
}

/// Stub API function
fn stub_api(_args: *const u8) -> isize {
    crate::serial_println!("[WIN32] Stub API called");
    0
}

// ============================================================================
// GLOBAL API TABLE
// ============================================================================

static WIN32_API_TABLE: Mutex<Option<BTreeMap<String, BTreeMap<String, Win32ApiFn>>>> = Mutex::new(None);

/// Initialize Win32 subsystem
pub fn init() {
    let mut table = WIN32_API_TABLE.lock();
    *table = Some(init_api_table());
    crate::serial_println!("[WIN32] API emulation initialized");
}

/// Get proc address
pub fn get_proc_address(module: &str, func: &str) -> Option<u64> {
    let table = WIN32_API_TABLE.lock();
    if let Some(ref t) = *table {
        if let Some(module_funcs) = t.get(module) {
            if module_funcs.contains_key(func) {
                // Return fake address (in real impl, would return actual function pointer)
                return Some(0xDEADBEEF);
            }
        }
    }
    None
}

/// Get proc address internal
pub fn get_proc_address_internal(module: &str, func: &str) -> FARPROC {
    if get_proc_address(module, func).is_some() {
        // Return stub function pointer
        // In real implementation, would return actual function pointer
        None
    } else {
        None
    }
}
