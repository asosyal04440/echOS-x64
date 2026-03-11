//! # Win32 API Öykünme Katmanı
//!
//! Windows uygulamalarını echOS üzerinde çalıştırmak için Win32 API öykünmesi.
//! `kernel32`, `user32` ve `gdi32` gibi yaygın Win32 API'lerini uygular.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoRingBufferDescriptor {
    pub address: u64,
    pub length: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IoRingCompletionEntry {
    pub user_data: u64,
    pub result_code: i32,
    pub information: u32,
    pub operation: u32,
}

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
pub const WS_OVERLAPPEDWINDOW: DWORD =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
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
pub const WM_MOVE: UINT = 0x0003;
pub const WM_SIZE: UINT = 0x0005;
pub const WM_ACTIVATE: UINT = 0x0006;
pub const WM_SETFOCUS: UINT = 0x0007;
pub const WM_KILLFOCUS: UINT = 0x0008;
pub const WM_PAINT: UINT = 0x000F;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_QUIT: UINT = 0x0012;
pub const WM_ERASEBKGND: UINT = 0x0014;
pub const WM_SHOWWINDOW: UINT = 0x0018;
pub const WM_KEYDOWN: UINT = 0x0100;
pub const WM_KEYUP: UINT = 0x0101;
pub const WM_CHAR: UINT = 0x0102;
pub const WM_TIMER: UINT = 0x0113;
pub const WM_MOUSEMOVE: UINT = 0x0200;
pub const WM_LBUTTONDOWN: UINT = 0x0201;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_RBUTTONDOWN: UINT = 0x0204;
pub const WM_RBUTTONUP: UINT = 0x0205;

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

// ============================================================================
// ALLOCATION TRACKER — maps raw pointer → (size, align) for dealloc
// ============================================================================

pub static ALLOC_MAP: Mutex<BTreeMap<u64, (usize, usize)>> = Mutex::new(BTreeMap::new());

// ============================================================================
// FILE HANDLE TABLE — maps Win32 HANDLE → VFS fd (file descriptor)
// ============================================================================

static FILE_HANDLES: Mutex<BTreeMap<u64, Win32FileState>> = Mutex::new(BTreeMap::new());
static NEXT_FILE_HANDLE: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0x1000_0000);

/// Win32 file state tracking
struct Win32FileState {
    fd: usize,        // VFS file descriptor
    path: String,     // Original path for debugging
    access: u32,      // GENERIC_READ/GENERIC_WRITE
    is_console: bool, // true for stdin/stdout/stderr
}

/// Standard handles
const STD_INPUT_HANDLE: u64 = 0xFFFF_FFF6;
const STD_OUTPUT_HANDLE: u64 = 0xFFFF_FFF5;
const STD_ERROR_HANDLE: u64 = 0xFFFF_FFF4;

// ============================================================================
// WIN32 GUI BRIDGE — HWND → echOS Window, DC → Surface, Message Queue
// ============================================================================

/// Win32 pencere durumu — echOS compositor'e köprü
pub struct Win32Window {
    pub hwnd: u64,
    pub class_name: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub style: u32,
    pub ex_style: u32,
    pub parent: u64,
    pub visible: bool,
    pub focused: bool,
    /// echOS pencere kimliği (compositor entegrasyonu)
    pub echos_window_id: u32,
    /// Pencere içeriği için piksel tamponu (BGRA format)
    pub surface: Vec<u8>,
    /// Pencere prosedürü adresi
    pub wndproc: u64,
}

/// Win32 mesaj yapısı
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Win32Msg {
    pub hwnd: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: i64,
    pub time: u32,
    pub pt_x: i32,
    pub pt_y: i32,
}

/// Device Context (DC) durumu
pub struct Win32DC {
    pub hdc: u64,
    pub hwnd: u64,
    pub pen_color: u32,
    pub brush_color: u32,
    pub text_color: u32,
    pub bk_color: u32,
    pub bk_mode: i32, // TRANSPARENT=1, OPAQUE=2
    pub pen_x: i32,
    pub pen_y: i32,
    /// Font bilgileri
    pub font_height: i32,
    pub font_weight: i32,
}

/// HWND → Win32Window mapping
static WIN32_WINDOWS: Mutex<BTreeMap<u64, Win32Window>> = Mutex::new(BTreeMap::new());
static NEXT_HWND: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0x0001_0000);

/// HDC → Win32DC mapping
static WIN32_DCS: Mutex<BTreeMap<u64, Win32DC>> = Mutex::new(BTreeMap::new());
static NEXT_HDC: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0x0002_0000);

/// Global mesaj kuyruğu (tüm pencereler için)
static MSG_QUEUE: Mutex<Vec<Win32Msg>> = Mutex::new(Vec::new());

/// Aktif (focused) pencere
static ACTIVE_HWND: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Mesaj kuyruğuna mesaj ekle
pub fn post_message(hwnd: u64, message: u32, wparam: u64, lparam: i64) -> bool {
    MSG_QUEUE.lock().push(Win32Msg {
        hwnd,
        message,
        wparam,
        lparam,
        time: get_tick_count_internal(),
        pt_x: 0,
        pt_y: 0,
    });
    true
}

/// Mesaj kuyruğundan mesaj al (blocking değil)
pub fn peek_message(hwnd_filter: u64) -> Option<Win32Msg> {
    let mut queue = MSG_QUEUE.lock();
    if hwnd_filter == 0 {
        // Herhangi bir pencere için
        if !queue.is_empty() {
            return Some(queue.remove(0));
        }
    } else {
        // Belirli pencere için
        if let Some(idx) = queue
            .iter()
            .position(|m| m.hwnd == hwnd_filter || m.hwnd == 0)
        {
            return Some(queue.remove(idx));
        }
    }
    None
}

/// TSC tabanlı tick sayacı (ms)
fn get_tick_count_internal() -> u32 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    let tsc = ((high as u64) << 32) | (low as u64);
    // Yaklaşık 3GHz CPU varsayımı → /3_000_000 ≈ ms
    (tsc / 3_000_000) as u32
}

/// Pencere surface'ını framebuffer'a kopyala
pub fn blit_window_to_framebuffer(hwnd: u64) {
    let windows = WIN32_WINDOWS.lock();
    if let Some(win) = windows.get(&hwnd) {
        if !win.visible || win.surface.is_empty() {
            return;
        }

        // Framebuffer'a doğrudan blit
        if let Some(mut fb) = crate::boot::get_global_framebuffer() {
            let fb_width = fb.width as i32;
            let fb_height = fb.height as i32;

            for y in 0..win.height {
                let dst_y = win.y + y;
                if dst_y < 0 || dst_y >= fb_height {
                    continue;
                }

                for x in 0..win.width {
                    let dst_x = win.x + x;
                    if dst_x < 0 || dst_x >= fb_width {
                        continue;
                    }

                    let src_idx = ((y * win.width + x) * 4) as usize;
                    if src_idx + 3 >= win.surface.len() {
                        continue;
                    }

                    let b = win.surface[src_idx];
                    let g = win.surface[src_idx + 1];
                    let r = win.surface[src_idx + 2];
                    let a = win.surface[src_idx + 3];

                    if a == 0 {
                        continue;
                    } // Tamamen saydam

                    // ARGB format (0xAARRGGBB)
                    let color =
                        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

                    if a == 255 {
                        // Opak — doğrudan çiz
                        fb.plot_pixel(dst_x as usize, dst_y as usize, color);
                    } else {
                        // Alpha blend
                        let existing = fb.get_pixel(dst_x as usize, dst_y as usize);
                        let dst_r = ((existing >> 16) & 0xFF) as u32;
                        let dst_g = ((existing >> 8) & 0xFF) as u32;
                        let dst_b = (existing & 0xFF) as u32;
                        let alpha = a as u32;
                        let inv_alpha = 255 - alpha;
                        let new_r = (r as u32 * alpha + dst_r * inv_alpha) / 255;
                        let new_g = (g as u32 * alpha + dst_g * inv_alpha) / 255;
                        let new_b = (b as u32 * alpha + dst_b * inv_alpha) / 255;
                        let blended = 0xFF000000 | (new_r << 16) | (new_g << 8) | new_b;
                        fb.plot_pixel(dst_x as usize, dst_y as usize, blended);
                    }
                }
            }
        }
    }
}

// ============================================================================
// DLL HANDLE TABLE — maps HMODULE → normalised module name (lowercase, no .dll)
// ============================================================================

static DLL_HANDLES: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());

/// Well-known baseline handle values for the modules we emulate.
const HMOD_KERNEL32: u64 = 0x0010_0000;
const HMOD_USER32: u64 = 0x0010_1000;
const HMOD_GDI32: u64 = 0x0010_2000;
const HMOD_ADVAPI32: u64 = 0x0010_3000;
const HMOD_MSVCRT: u64 = 0x0010_4000;
const HMOD_SHELL32: u64 = 0x0010_5000;
const HMOD_NTDLL: u64 = 0x0010_6000;
const HMOD_WS2_32: u64 = 0x0010_7000;

/// Initialise the well-known DLL handles (called once at Win32 init).
pub fn init_dll_handles() {
    let mut map = DLL_HANDLES.lock();
    map.insert(HMOD_KERNEL32, "kernel32".into());
    map.insert(HMOD_USER32, "user32".into());
    map.insert(HMOD_GDI32, "gdi32".into());
    map.insert(HMOD_ADVAPI32, "advapi32".into());
    map.insert(HMOD_MSVCRT, "msvcrt".into());
    map.insert(HMOD_SHELL32, "shell32".into());
    map.insert(HMOD_NTDLL, "ntdll".into());
    map.insert(HMOD_WS2_32, "ws2_32".into());
}

/// Resolve a module name to a canonical handle and register it if not present.
pub fn handle_for_module(name: &str) -> u64 {
    let key = name.to_lowercase();
    let key = key.trim_end_matches(".dll");
    // Check existing handles
    {
        let map = DLL_HANDLES.lock();
        for (&h, n) in map.iter() {
            if n == key {
                return h;
            }
        }
    }
    // Assign a new handle: use a simple counter above our static range
    static NEXT_HANDLE: core::sync::atomic::AtomicU64 =
        core::sync::atomic::AtomicU64::new(0x0010_7000);
    let h = NEXT_HANDLE.fetch_add(0x1000, core::sync::atomic::Ordering::Relaxed);
    DLL_HANDLES.lock().insert(h, key.to_string());
    h
}

/// Reverse lookup: handle → module name string.
pub fn module_for_handle(hmod: u64) -> Option<String> {
    DLL_HANDLES.lock().get(&hmod).cloned()
}

/// Allocate `size` bytes aligned to `align` (min 1), zero-initialised.
/// Records the allocation so `win32_dealloc` can free it.
pub fn win32_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let align = align.max(1).next_power_of_two();
    let layout = match alloc::alloc::Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if !ptr.is_null() {
        ALLOC_MAP.lock().insert(ptr as u64, (size, align));
    }
    ptr
}

/// Free a pointer previously returned by `win32_alloc`.
pub fn win32_dealloc(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    if let Some((size, align)) = ALLOC_MAP.lock().remove(&(ptr as u64)) {
        if let Ok(layout) = alloc::alloc::Layout::from_size_align(size, align) {
            unsafe {
                alloc::alloc::dealloc(ptr, layout);
            }
        }
    }
}

/// Reallocate — grow/shrink an existing allocation.
pub fn win32_realloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    if ptr.is_null() {
        return win32_alloc(new_size, 8);
    }
    if new_size == 0 {
        win32_dealloc(ptr);
        return core::ptr::null_mut();
    }
    let (old_size, align) = match ALLOC_MAP.lock().remove(&(ptr as u64)) {
        Some(v) => v,
        None => return core::ptr::null_mut(),
    };
    let align = align.max(1).next_power_of_two();
    let old_layout = match alloc::alloc::Layout::from_size_align(old_size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };
    let new_ptr = unsafe { alloc::alloc::realloc(ptr, old_layout, new_size) };
    if !new_ptr.is_null() {
        ALLOC_MAP.lock().insert(new_ptr as u64, (new_size, align));
    }
    new_ptr
}

// ============================================================================
// RAW FUNCTION ADDRESS TABLE  — used by PE loader to patch IAT entries
// ============================================================================

/// Return the kernel virtual address of a named Win32 API function.
/// The PE loader writes this address directly into the in-memory IAT slot.
pub fn get_fn_address(module: &str, name: &str) -> u64 {
    // Normalise module name: strip ".dll" suffix, lowercase
    let mod_key: &str = &module.to_lowercase();
    let mod_key = mod_key.trim_end_matches(".dll");
    let mod_key = match mod_key {
        "kernelbase" => "kernel32",
        "api-ms-win-core-libraryloader-l1-2-0" => "kernel32",
        "api-ms-win-core-heap-l1-1-0" => "kernel32",
        "api-ms-win-core-synch-l1-2-0" => "kernel32",
        _ => mod_key,
    };
    if mod_key == "ntdll" {
        if let Some(addr) = crate::win32_abi::resolve_ntdll_symbol(name) {
            return addr;
        }
    }
    if mod_key == "ws2_32" || mod_key == "wsock32" {
        if let Some(addr) = crate::win32_abi::resolve_ws2_32_symbol(name) {
            return addr;
        }
    }
    match (mod_key, name) {
        // ---- kernel32 -------------------------------------------------------
        ("kernel32", "GetModuleHandleA") | ("kernel32", "GetModuleHandleW") => {
            kernel32::get_module_handle_a as usize as u64
        }
        ("kernel32", "LoadLibraryA") => kernel32::load_library_a as usize as u64,
        ("kernel32", "LoadLibraryW") => kernel32::load_library_a as usize as u64,
        ("kernel32", "GetProcAddress") => kernel32::get_proc_address as usize as u64,
        ("kernel32", "VirtualAlloc") => kernel32::virtual_alloc as usize as u64,
        ("kernel32", "VirtualFree") => kernel32::virtual_free as usize as u64,
        ("kernel32", "VirtualProtect") => kernel32::virtual_protect as usize as u64,
        ("kernel32", "VirtualQuery") => kernel32::virtual_query as usize as u64,
        ("kernel32", "HeapCreate") => kernel32::heap_create as usize as u64,
        ("kernel32", "HeapAlloc") => kernel32::heap_alloc as usize as u64,
        ("kernel32", "HeapFree") => kernel32::heap_free as usize as u64,
        ("kernel32", "HeapReAlloc") => kernel32::heap_realloc as usize as u64,
        ("kernel32", "HeapSize") => kernel32::heap_size as usize as u64,
        ("kernel32", "HeapDestroy") => kernel32::heap_destroy as usize as u64,
        ("kernel32", "GetProcessHeap") => kernel32::get_process_heap as usize as u64,
        ("kernel32", "LocalAlloc") => kernel32::local_alloc as usize as u64,
        ("kernel32", "LocalFree") => kernel32::local_free as usize as u64,
        ("kernel32", "GlobalAlloc") => kernel32::global_alloc as usize as u64,
        ("kernel32", "GlobalFree") => kernel32::global_free as usize as u64,
        ("kernel32", "CreateFileA") => kernel32::create_file_a as usize as u64,
        ("kernel32", "CreateFileW") => kernel32::create_file_a as usize as u64,
        ("kernel32", "ReadFile") => kernel32::read_file as usize as u64,
        ("kernel32", "WriteFile") => kernel32::write_file as usize as u64,
        ("kernel32", "CloseHandle") => kernel32::close_handle as usize as u64,
        ("kernel32", "SetFilePointer") => kernel32::set_file_pointer as usize as u64,
        ("kernel32", "SetFilePointerEx") => kernel32::set_file_pointer_ex as usize as u64,
        ("kernel32", "GetFileSize") => kernel32::get_file_size as usize as u64,
        ("kernel32", "GetFileSizeEx") => kernel32::get_file_size_ex as usize as u64,
        ("kernel32", "GetFileAttributesA") => kernel32::get_file_attributes_a as usize as u64,
        ("kernel32", "DeleteFileA") => kernel32::delete_file_a as usize as u64,
        ("kernel32", "MoveFileA") => kernel32::move_file_a as usize as u64,
        ("kernel32", "CopyFileA") => kernel32::copy_file_a as usize as u64,
        ("kernel32", "FindFirstFileA") => kernel32::find_first_file_a as usize as u64,
        ("kernel32", "FindNextFileA") => kernel32::find_next_file_a as usize as u64,
        ("kernel32", "FindClose") => kernel32::find_close as usize as u64,
        ("kernel32", "CreateDirectoryA") => kernel32::create_directory_a as usize as u64,
        ("kernel32", "GetCurrentDirectoryA") => kernel32::get_current_directory_a as usize as u64,
        ("kernel32", "ExitProcess") => kernel32::exit_process as usize as u64,
        ("kernel32", "CreateProcessA") => kernel32::create_process_a as usize as u64,
        ("kernel32", "OpenProcess") => kernel32::open_process as usize as u64,
        ("kernel32", "GetCurrentProcess") => kernel32::get_current_process as usize as u64,
        ("kernel32", "GetCurrentProcessId") => kernel32::get_current_process_id as usize as u64,
        ("kernel32", "CreateThread") => kernel32::create_thread as usize as u64,
        ("kernel32", "ExitThread") => kernel32::exit_thread as usize as u64,
        ("kernel32", "GetCurrentThread") => kernel32::get_current_thread as usize as u64,
        ("kernel32", "GetCurrentThreadId") => kernel32::get_current_thread_id as usize as u64,
        ("kernel32", "ResumeThread") => kernel32::resume_thread as usize as u64,
        ("kernel32", "SuspendThread") => kernel32::suspend_thread as usize as u64,
        ("kernel32", "WaitForSingleObject") => kernel32::wait_for_single_object as usize as u64,
        ("kernel32", "WaitForMultipleObjects") => {
            kernel32::wait_for_multiple_objects as usize as u64
        }
        ("kernel32", "WaitOnAddress") => kernel32::wait_on_address as usize as u64,
        ("kernel32", "WakeByAddressSingle") => kernel32::wake_by_address_single as usize as u64,
        ("kernel32", "WakeByAddressAll") => kernel32::wake_by_address_all as usize as u64,
        ("kernel32", "Sleep") => kernel32::sleep as usize as u64,
        ("kernel32", "GetTickCount") => kernel32::get_tick_count as usize as u64,
        ("kernel32", "GetSystemInfo") => kernel32::get_system_info as usize as u64,
        ("kernel32", "GetVersion") => kernel32::get_version as usize as u64,
        ("kernel32", "GetVersionExA") => kernel32::get_version_ex_a as usize as u64,
        ("kernel32", "GetComputerNameA") => kernel32::get_computer_name_a as usize as u64,
        ("kernel32", "GetLastError") => kernel32::get_last_error as usize as u64,
        ("kernel32", "SetLastError") => kernel32::set_last_error as usize as u64,
        ("kernel32", "MultiByteToWideChar") => kernel32::multi_byte_to_wide_char as usize as u64,
        ("kernel32", "WideCharToMultiByte") => kernel32::wide_char_to_multi_byte as usize as u64,
        ("kernel32", "lstrlenA") => kernel32::lstrlen_a as usize as u64,
        ("kernel32", "lstrlenW") => kernel32::lstrlen_w as usize as u64,
        ("kernel32", "lstrcpyA") => kernel32::lstrcpy_a as usize as u64,
        ("kernel32", "lstrcatA") => kernel32::lstrcat_a as usize as u64,
        ("kernel32", "lstrcmpA") => kernel32::lstrcmp_a as usize as u64,
        ("kernel32", "lstrcmpiA") => kernel32::lstrcmpi_a as usize as u64,
        ("kernel32", "GetStdHandle") => kernel32::get_std_handle as usize as u64,
        ("kernel32", "WriteConsoleA") => kernel32::write_console_a as usize as u64,
        ("kernel32", "ReadConsoleA") => kernel32::read_console_a as usize as u64,
        ("kernel32", "SetConsoleMode") => kernel32::set_console_mode as usize as u64,
        ("kernel32", "GetConsoleMode") => kernel32::get_console_mode as usize as u64,
        ("kernel32", "GetEnvironmentVariableA") => {
            kernel32::get_environment_variable_a as usize as u64
        }
        ("kernel32", "SetEnvironmentVariableA") => {
            kernel32::set_environment_variable_a as usize as u64
        }
        ("kernel32", "GetCommandLineA") => kernel32::get_command_line_a as usize as u64,
        ("kernel32", "GlobalMemoryStatusEx") => kernel32::global_memory_status_ex as usize as u64,
        ("kernel32", "TerminateProcess") => kernel32::terminate_process as usize as u64,
        // ---- sync primitives ---------------------------------------------------
        ("kernel32", "CreateMutexA") => kernel32::create_mutex_a as usize as u64,
        ("kernel32", "CreateMutexW") => kernel32::create_mutex_a as usize as u64,
        ("kernel32", "CreateEventA") => kernel32::create_event_a as usize as u64,
        ("kernel32", "CreateEventW") => kernel32::create_event_a as usize as u64,
        ("kernel32", "CreateIoRing") => kernel32::create_io_ring as usize as u64,
        ("kernel32", "BuildIoRingRegisterFileHandles") => {
            kernel32::build_io_ring_register_file_handles as usize as u64
        }
        ("kernel32", "BuildIoRingRegisterBuffers") => {
            kernel32::build_io_ring_register_buffers as usize as u64
        }
        ("kernel32", "BuildIoRingReadFile") => kernel32::build_io_ring_read_file as usize as u64,
        ("kernel32", "BuildIoRingWriteFile") => {
            kernel32::build_io_ring_write_file as usize as u64
        }
        ("kernel32", "SubmitIoRing") => kernel32::submit_io_ring as usize as u64,
        ("kernel32", "PopIoRingCompletion") => kernel32::pop_io_ring_completion as usize as u64,
        ("kernel32", "CloseIoRing") => kernel32::close_io_ring as usize as u64,
        ("kernel32", "CreateSemaphoreA") => kernel32::create_semaphore_a as usize as u64,
        ("kernel32", "SetEvent") => kernel32::set_event as usize as u64,
        ("kernel32", "ResetEvent") => kernel32::reset_event as usize as u64,
        ("kernel32", "PulseEvent") => kernel32::pulse_event as usize as u64,
        ("kernel32", "ReleaseMutex") => kernel32::release_mutex as usize as u64,
        ("kernel32", "ReleaseSemaphore") => kernel32::release_semaphore as usize as u64,
        ("kernel32", "InitializeCriticalSection") => {
            kernel32::initialize_critical_section as usize as u64
        }
        ("kernel32", "EnterCriticalSection") => kernel32::enter_critical_section as usize as u64,
        ("kernel32", "LeaveCriticalSection") => kernel32::leave_critical_section as usize as u64,
        ("kernel32", "DeleteCriticalSection") => kernel32::delete_critical_section as usize as u64,
        ("kernel32", "TryEnterCriticalSection") => {
            kernel32::try_enter_critical_section as usize as u64
        }
        // ---- user32 ---------------------------------------------------------
        ("user32", "RegisterClassA") => user32::register_class_a as usize as u64,
        ("user32", "RegisterClassExA") => user32::register_class_a as usize as u64,
        ("user32", "CreateWindowExA") => user32::create_window_ex_a as usize as u64,
        ("user32", "CreateWindowA") => user32::create_window_ex_a as usize as u64,
        ("user32", "DestroyWindow") => user32::destroy_window as usize as u64,
        ("user32", "ShowWindow") => user32::show_window as usize as u64,
        ("user32", "UpdateWindow") => user32::update_window as usize as u64,
        ("user32", "GetMessageA") => user32::get_message_a as usize as u64,
        ("user32", "PeekMessageA") => user32::peek_message_a as usize as u64,
        ("user32", "TranslateMessage") => user32::translate_message as usize as u64,
        ("user32", "DispatchMessageA") => user32::dispatch_message_a as usize as u64,
        ("user32", "PostQuitMessage") => user32::post_quit_message as usize as u64,
        ("user32", "PostMessageA") => user32::post_message_a as usize as u64,
        ("user32", "SendMessageA") => user32::send_message_a as usize as u64,
        ("user32", "DefWindowProcA") => user32::def_window_proc_a as usize as u64,
        ("user32", "GetDC") => user32::get_dc as usize as u64,
        ("user32", "ReleaseDC") => user32::release_dc as usize as u64,
        ("user32", "SetWindowTextA") => user32::set_window_text_a as usize as u64,
        ("user32", "GetClientRect") => user32::get_client_rect as usize as u64,
        ("user32", "GetWindowRect") => user32::get_window_rect as usize as u64,
        ("user32", "MoveWindow") => user32::move_window as usize as u64,
        ("user32", "SetWindowPos") => user32::set_window_pos as usize as u64,
        ("user32", "MessageBoxA") => user32::message_box_a as usize as u64,
        ("user32", "MessageBoxW") => user32::message_box_a as usize as u64,
        ("user32", "GetSystemMetrics") => user32::get_system_metrics as usize as u64,
        ("user32", "GetDesktopWindow") => user32::get_desktop_window as usize as u64,
        ("user32", "GetForegroundWindow") => user32::get_foreground_window as usize as u64,
        ("user32", "SetForegroundWindow") => user32::set_foreground_window as usize as u64,
        ("user32", "GetFocus") => user32::get_focus as usize as u64,
        ("user32", "SetFocus") => user32::set_focus as usize as u64,
        ("user32", "IsWindow") => user32::is_window as usize as u64,
        ("user32", "IsWindowVisible") => user32::is_window_visible as usize as u64,
        ("user32", "EnableWindow") => user32::enable_window as usize as u64,
        ("user32", "GetKeyState") => user32::get_key_state as usize as u64,
        ("user32", "GetAsyncKeyState") => user32::get_async_key_state as usize as u64,
        ("user32", "GetCursorPos") => user32::get_cursor_pos as usize as u64,
        ("user32", "SetCursorPos") => user32::set_cursor_pos as usize as u64,
        ("user32", "SetTimer") => user32::set_timer as usize as u64,
        ("user32", "KillTimer") => user32::kill_timer as usize as u64,
        ("user32", "LoadIconA") => user32::load_icon_a as usize as u64,
        ("user32", "LoadCursorA") => user32::load_cursor_a as usize as u64,
        ("user32", "SetWindowLongA") => user32::set_window_long_a as usize as u64,
        ("user32", "GetWindowLongA") => user32::get_window_long_a as usize as u64,
        ("user32", "SetWindowLongPtrA") => user32::set_window_long_ptr_a as usize as u64,
        ("user32", "GetWindowLongPtrA") => user32::get_window_long_ptr_a as usize as u64,
        ("user32", "FindWindowA") => user32::find_window_a as usize as u64,
        ("user32", "OpenClipboard") => user32::open_clipboard as usize as u64,
        ("user32", "CloseClipboard") => user32::close_clipboard as usize as u64,
        ("user32", "SetClipboardData") => user32::set_clipboard_data as usize as u64,
        ("user32", "GetClipboardData") => user32::get_clipboard_data as usize as u64,
        ("user32", "EmptyClipboard") => user32::empty_clipboard as usize as u64,
        ("user32", "MapVirtualKeyA") => user32::map_virtual_key_a as usize as u64,
        ("user32", "ToAscii") => user32::to_ascii as usize as u64,
        ("user32", "VkKeyScanA") => user32::vk_key_scan_a as usize as u64,
        ("user32", "GetWindowTextA") => user32::get_window_text_a as usize as u64,
        ("user32", "GetWindowTextLengthA") => user32::get_window_text_length_a as usize as u64,
        ("user32", "GetParent") => user32::get_parent as usize as u64,
        ("user32", "GetWindow") => user32::get_window as usize as u64,
        ("user32", "CreateMenu") => user32::create_menu as usize as u64,
        ("user32", "AppendMenuA") => user32::append_menu_a as usize as u64,
        ("user32", "SetMenu") => user32::set_menu as usize as u64,
        ("user32", "DrawMenuBar") => user32::draw_menu_bar as usize as u64,
        ("user32", "EndDialog") => user32::end_dialog as usize as u64,
        ("user32", "GetDlgItem") => user32::get_dlg_item as usize as u64,
        ("user32", "SetWindowsHookExA") => user32::set_windows_hook_ex_a as usize as u64,
        ("user32", "UnhookWindowsHookEx") => user32::unhook_windows_hook_ex as usize as u64,
        ("user32", "CallNextHookEx") => user32::call_next_hook_ex as usize as u64,
        ("user32", "AttachThreadInput") => user32::attach_thread_input as usize as u64,
        // ---- gdi32 ----------------------------------------------------------
        ("gdi32", "TextOutA") => gdi32::text_out_a as usize as u64,
        ("gdi32", "DrawTextA") => gdi32::draw_text_a as usize as u64,
        ("gdi32", "FillRect") => gdi32::fill_rect as usize as u64,
        ("gdi32", "SetTextColor") => gdi32::set_text_color as usize as u64,
        ("gdi32", "SetBkColor") => gdi32::set_bk_color as usize as u64,
        ("gdi32", "SetBkMode") => gdi32::set_bk_mode as usize as u64,
        ("gdi32", "CreateFontA") => gdi32::create_font_a as usize as u64,
        ("gdi32", "CreateFontIndirectA") => gdi32::create_font_indirect_a as usize as u64,
        ("gdi32", "CreateSolidBrush") => gdi32::create_solid_brush as usize as u64,
        ("gdi32", "CreatePen") => gdi32::create_pen as usize as u64,
        ("gdi32", "CreateCompatibleBitmap") => gdi32::create_compatible_bitmap as usize as u64,
        ("gdi32", "CreateBitmap") => gdi32::create_bitmap as usize as u64,
        ("gdi32", "MoveToEx") => gdi32::move_to_ex as usize as u64,
        ("gdi32", "LineTo") => gdi32::line_to as usize as u64,
        ("gdi32", "Ellipse") => gdi32::ellipse as usize as u64,
        ("gdi32", "SetPixel") => gdi32::set_pixel as usize as u64,
        ("gdi32", "SetPixelV") => gdi32::set_pixel_v as usize as u64,
        ("gdi32", "GetPixel") => gdi32::get_pixel as usize as u64,
        ("gdi32", "BitBlt") => gdi32::bit_blt as usize as u64,
        ("gdi32", "PatBlt") => gdi32::pat_blt as usize as u64,
        ("gdi32", "StretchBlt") => gdi32::stretch_blt as usize as u64,
        ("gdi32", "GetPixel") => gdi32::get_pixel as usize as u64,
        ("gdi32", "GetTextMetricsA") => gdi32::get_text_metrics_a as usize as u64,
        ("gdi32", "GetTextExtentPoint32A") => gdi32::get_text_extent_point_32_a as usize as u64,
        ("gdi32", "SaveDC") => gdi32::save_dc as usize as u64,
        ("gdi32", "RestoreDC") => gdi32::restore_dc as usize as u64,
        ("gdi32", "BeginPath") => gdi32::begin_path as usize as u64,
        ("gdi32", "EndPath") => gdi32::end_path as usize as u64,
        ("gdi32", "StrokePath") => gdi32::stroke_path as usize as u64,
        ("gdi32", "FillPath") => gdi32::fill_path as usize as u64,
        ("gdi32", "CreateRectRgn") => gdi32::create_rect_rgn as usize as u64,
        ("gdi32", "CombineRgn") => gdi32::combine_rgn as usize as u64,
        ("gdi32", "SetWorldTransform") => gdi32::set_world_transform as usize as u64,
        ("gdi32", "GetObjectA") => gdi32::get_object_a as usize as u64,
        // ---- advapi32 -------------------------------------------------------
        ("advapi32", "RegOpenKeyExA") => advapi32::reg_open_key_ex_a as usize as u64,
        ("advapi32", "RegCloseKey") => advapi32::reg_close_key as usize as u64,
        ("advapi32", "RegQueryValueExA") => advapi32::reg_query_value_ex_a as usize as u64,
        ("advapi32", "RegSetValueExA") => advapi32::reg_set_value_ex_a as usize as u64,
        ("advapi32", "RegCreateKeyExA") => advapi32::reg_create_key_ex_a as usize as u64,
        ("advapi32", "CryptAcquireContextA") => advapi32::crypt_acquire_context_a as usize as u64,
        ("advapi32", "CryptGenRandom") => advapi32::crypt_gen_random as usize as u64,
        ("advapi32", "CryptCreateHash") => advapi32::crypt_create_hash as usize as u64,
        ("advapi32", "CryptHashData") => advapi32::crypt_hash_data as usize as u64,
        ("advapi32", "CryptGetHashParam") => advapi32::crypt_get_hash_param as usize as u64,
        ("advapi32", "CryptDestroyHash") => advapi32::crypt_destroy_hash as usize as u64,
        ("advapi32", "CryptReleaseContext") => advapi32::crypt_release_context as usize as u64,
        ("advapi32", "OpenProcessToken") => advapi32::open_process_token as usize as u64,
        ("advapi32", "GetTokenInformation") => advapi32::get_token_information as usize as u64,
        ("advapi32", "LookupPrivilegeValueA") => advapi32::lookup_privilege_value_a as usize as u64,
        ("advapi32", "AdjustTokenPrivileges") => advapi32::adjust_token_privileges as usize as u64,
        // ---- msvcrt ---------------------------------------------------------
        ("msvcrt", "malloc") => msvcrt::malloc as usize as u64,
        ("msvcrt", "free") => msvcrt::free as usize as u64,
        ("msvcrt", "calloc") => msvcrt::calloc as usize as u64,
        ("msvcrt", "realloc") => msvcrt::realloc as usize as u64,
        ("msvcrt", "strlen") => msvcrt::strlen as usize as u64,
        ("msvcrt", "strcpy") => msvcrt::strcpy as usize as u64,
        ("msvcrt", "strncpy") => msvcrt::strncpy as usize as u64,
        ("msvcrt", "strcat") => msvcrt::strcat as usize as u64,
        ("msvcrt", "strcmp") => msvcrt::strcmp as usize as u64,
        ("msvcrt", "strncmp") => msvcrt::strncmp as usize as u64,
        ("msvcrt", "strchr") => msvcrt::strchr as usize as u64,
        ("msvcrt", "strstr") => msvcrt::strstr as usize as u64,
        ("msvcrt", "memcpy") => msvcrt::memcpy as usize as u64,
        ("msvcrt", "memmove") => msvcrt::memmove as usize as u64,
        ("msvcrt", "memset") => msvcrt::memset as usize as u64,
        ("msvcrt", "memcmp") => msvcrt::memcmp as usize as u64,
        ("msvcrt", "printf") => msvcrt::printf as usize as u64,
        ("msvcrt", "sprintf") => msvcrt::sprintf as usize as u64,
        ("msvcrt", "snprintf") => msvcrt::snprintf as usize as u64,
        ("msvcrt", "fprintf") => msvcrt::fprintf as usize as u64,
        ("msvcrt", "fopen") => msvcrt::fopen as usize as u64,
        ("msvcrt", "fclose") => msvcrt::fclose as usize as u64,
        ("msvcrt", "fread") => msvcrt::fread as usize as u64,
        ("msvcrt", "fwrite") => msvcrt::fwrite as usize as u64,
        ("msvcrt", "fseek") => msvcrt::fseek as usize as u64,
        ("msvcrt", "ftell") => msvcrt::ftell as usize as u64,
        ("msvcrt", "atoi") => msvcrt::atoi as usize as u64,
        ("msvcrt", "atol") => msvcrt::atol as usize as u64,
        ("msvcrt", "rand") => msvcrt::rand as usize as u64,
        ("msvcrt", "srand") => msvcrt::srand as usize as u64,
        ("msvcrt", "abs") => msvcrt::abs as usize as u64,
        ("msvcrt", "exit") => msvcrt::exit as usize as u64,
        ("msvcrt", "abort") => msvcrt::abort as usize as u64,
        ("msvcrt", "getenv") => msvcrt::getenv as usize as u64,
        ("msvcrt", "qsort") => msvcrt::qsort as usize as u64,
        ("msvcrt", "bsearch") => msvcrt::bsearch as usize as u64,
        // ---- shell32 --------------------------------------------------------
        ("shell32", "ShellExecuteA") => shell32::shell_execute_a as usize as u64,
        ("shell32", "SHGetFolderPathA") => shell32::sh_get_folder_path_a as usize as u64,
        ("shell32", "SHGetSpecialFolderPathA") => {
            shell32::sh_get_special_folder_path_a as usize as u64
        }
        _ => stub_api as usize as u64,
    }
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

    /// GetModuleHandleA / GetModuleHandleW
    pub unsafe fn get_module_handle_a(lpModuleName: LPCSTR) -> HMODULE {
        if lpModuleName.is_null() {
            // GetModuleHandle(NULL) returns the EXE base — use our PE image base pseudo-handle
            return crate::win32::HMOD_KERNEL32 as HMODULE;
        }
        let mut name = String::new();
        let mut ptr = lpModuleName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        crate::win32::handle_for_module(&name) as HMODULE
    }

    /// LoadLibraryA / LoadLibraryW
    pub unsafe fn load_library_a(lpLibFileName: LPCSTR) -> HMODULE {
        if lpLibFileName.is_null() {
            return 0;
        }
        let mut name = String::new();
        let mut ptr = lpLibFileName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        let key = {
            let k = name.to_lowercase();
            k.trim_end_matches(".dll").to_string()
        };
        crate::serial_println!("[WIN32] LoadLibraryA: {}", key);
        crate::win32::handle_for_module(&key) as HMODULE
    }

    /// GetProcAddress
    pub unsafe fn get_proc_address(hModule: HMODULE, lpProcName: LPCSTR) -> FARPROC {
        if lpProcName.is_null() {
            return None;
        }
        // Ordinal import: low word < 0x10000 and high word is zero
        if (hModule as u64) < 0x10000 {
            // ordinal-based — not supported in our emulation
            return None;
        }
        let mut name = String::new();
        let mut ptr = lpProcName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        // Resolve module name from handle
        let module =
            crate::win32::module_for_handle(hModule as u64).unwrap_or_else(|| "kernel32".into());
        let addr = crate::win32::get_fn_address(&module, &name);
        crate::serial_println!("[WIN32] GetProcAddress: {}!{} = {:#x}", module, name, addr);
        // Transmute the raw address to FARPROC (Option<unsafe extern fn()>)
        let fn_ptr: unsafe extern "system" fn() -> isize = core::mem::transmute(addr);
        Some(fn_ptr)
    }

    /// VirtualAlloc
    pub unsafe fn virtual_alloc(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        flAllocationType: DWORD,
        _flProtect: DWORD,
    ) -> LPVOID {
        // If lpAddress is non-null this is MEM_COMMIT on an already-reserved region;
        // for our flat model we just return it as-is.
        if !lpAddress.is_null() {
            return lpAddress;
        }
        let size = dwSize as usize;
        if size == 0 {
            return core::ptr::null_mut();
        }
        let ptr = crate::win32::win32_alloc(size, 4096) as LPVOID;
        crate::serial_println!("[WIN32] VirtualAlloc {} bytes -> {:p}", size, ptr);
        ptr
    }

    /// VirtualFree
    pub unsafe fn virtual_free(lpAddress: LPVOID, dwSize: SIZE_T, dwFreeType: DWORD) -> BOOL {
        // MEM_RELEASE (0x8000) frees the allocation; MEM_DECOMMIT (0x4000) is a no-op here
        if dwFreeType & 0x8000 != 0 {
            crate::win32::win32_dealloc(lpAddress as *mut u8);
        }
        TRUE
    }

    /// GetTickCount - Returns milliseconds since system boot
    pub unsafe fn get_tick_count() -> DWORD {
        // Use TSC to estimate milliseconds
        let tsc = core::arch::x86_64::_rdtsc();
        // Assume ~2GHz CPU: tsc / 2_000_000 = ms
        (tsc / 2_000_000) as DWORD
    }

    /// Sleep - Suspends execution for specified milliseconds
    pub unsafe fn sleep(dwMilliseconds: DWORD) {
        // Use TSC-based delay: assumes ~2GHz CPU
        let start = core::arch::x86_64::_rdtsc();
        let cycles_to_wait = (dwMilliseconds as u64) * 2_000_000;
        while core::arch::x86_64::_rdtsc() - start < cycles_to_wait {
            core::hint::spin_loop();
        }
    }

    /// CreateFileA - Opens or creates a file via VFS
    pub unsafe fn create_file_a(
        lpFileName: LPCSTR,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: LPVOID,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE {
        if lpFileName.is_null() {
            return INVALID_HANDLE_VALUE;
        }

        // Parse filename
        let mut name = String::new();
        let mut ptr = lpFileName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }

        // Convert Windows path to Unix path
        let unix_path = name.replace("\\\\", "/").replace("\\", "/");
        let unix_path: String = if unix_path.len() > 2 && unix_path.chars().nth(1) == Some(':') {
            // Strip drive letter (C:\foo -> /mnt/c/foo)
            let mut path = String::from("/mnt");
            path.push_str(&unix_path[2..]);
            path
        } else {
            unix_path
        };

        // Map Win32 flags to POSIX flags
        let flags = match dwCreationDisposition {
            1 => 0o1 | 0o100 | 0o200,  // CREATE_NEW: O_WRONLY | O_CREAT | O_EXCL
            2 => 0o1 | 0o100 | 0o1000, // CREATE_ALWAYS: O_WRONLY | O_CREAT | O_TRUNC
            3 => 0o0,                  // OPEN_EXISTING: O_RDONLY
            4 => 0o1 | 0o100,          // OPEN_ALWAYS: O_WRONLY | O_CREAT
            5 => 0o1 | 0o1000,         // TRUNCATE_EXISTING: O_WRONLY | O_TRUNC
            _ => 0o0,
        };
        let flags = if dwDesiredAccess & 0x80000000 != 0 && dwDesiredAccess & 0x40000000 != 0 {
            flags | 0o2 // O_RDWR
        } else if dwDesiredAccess & 0x40000000 != 0 {
            flags | 0o1 // O_WRONLY
        } else {
            flags // O_RDONLY
        };

        crate::serial_println!(
            "[WIN32] CreateFileA: {} -> {} flags={:#o}",
            name,
            unix_path,
            flags
        );

        // Open via VFS
        let fd = crate::fs::sys_open(&unix_path, flags);
        if fd == usize::MAX {
            crate::serial_println!("[WIN32] CreateFileA failed: file not found");
            return INVALID_HANDLE_VALUE;
        }

        // Allocate Win32 handle
        let handle =
            crate::win32::NEXT_FILE_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        crate::win32::FILE_HANDLES.lock().insert(
            handle,
            crate::win32::Win32FileState {
                fd,
                path: unix_path,
                access: dwDesiredAccess,
                is_console: false,
            },
        );

        handle as HANDLE
    }

    /// ReadFile - Reads data from a file via VFS
    pub unsafe fn read_file(
        hFile: HANDLE,
        lpBuffer: LPVOID,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        if lpBuffer.is_null() || nNumberOfBytesToRead == 0 {
            if !lpNumberOfBytesRead.is_null() {
                *lpNumberOfBytesRead = 0;
            }
            return TRUE;
        }

        // Check for console handles
        let h = hFile as u64;
        if h == crate::win32::STD_INPUT_HANDLE {
            // Console input - read from keyboard buffer
            if !lpNumberOfBytesRead.is_null() {
                *lpNumberOfBytesRead = 0;
            }
            return TRUE;
        }
        if h == crate::win32::STD_OUTPUT_HANDLE || h == crate::win32::STD_ERROR_HANDLE {
            if !lpNumberOfBytesRead.is_null() {
                *lpNumberOfBytesRead = 0;
            }
            return TRUE;
        }

        // Lookup file state
        let fd = match crate::win32::FILE_HANDLES.lock().get(&h) {
            Some(state) => state.fd,
            None => {
                crate::serial_println!("[WIN32] ReadFile: invalid handle {:#x}", h);
                if !lpNumberOfBytesRead.is_null() {
                    *lpNumberOfBytesRead = 0;
                }
                return FALSE;
            }
        };

        // Create buffer slice
        let buf = core::slice::from_raw_parts_mut(lpBuffer, nNumberOfBytesToRead as usize);

        // Read via VFS
        match crate::fs::sys_read(fd, buf) {
            Ok(bytes_read) => {
                if !lpNumberOfBytesRead.is_null() {
                    *lpNumberOfBytesRead = bytes_read as DWORD;
                }
                TRUE
            }
            Err(_e) => {
                crate::serial_println!("[WIN32] ReadFile failed: fd={}", fd);
                if !lpNumberOfBytesRead.is_null() {
                    *lpNumberOfBytesRead = 0;
                }
                FALSE
            }
        }
    }

    /// WriteFile - Writes data to a file via VFS or console
    pub unsafe fn write_file(
        hFile: HANDLE,
        lpBuffer: LPCVOID,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        if lpBuffer.is_null() || nNumberOfBytesToWrite == 0 {
            if !lpNumberOfBytesWritten.is_null() {
                *lpNumberOfBytesWritten = 0;
            }
            return TRUE;
        }

        let h = hFile as u64;

        // Check for console handles - write to serial for debugging
        if h == crate::win32::STD_OUTPUT_HANDLE || h == crate::win32::STD_ERROR_HANDLE {
            let buf = core::slice::from_raw_parts(lpBuffer, nNumberOfBytesToWrite as usize);
            // Write to serial console using UART directly via inline asm
            for &byte in buf {
                if byte >= 0x20 && byte < 0x7F || byte == b'\n' || byte == b'\r' || byte == b'\t' {
                    // Wait for transmit buffer empty
                    loop {
                        let status: u8;
                        core::arch::asm!("in al, dx", in("dx") 0x3FDu16, out("al") status);
                        if status & 0x20 != 0 {
                            break;
                        }
                    }
                    // Send byte to COM1 (0x3F8)
                    core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte);
                }
            }
            if !lpNumberOfBytesWritten.is_null() {
                *lpNumberOfBytesWritten = nNumberOfBytesToWrite;
            }
            return TRUE;
        }

        // Lookup file state
        let fd = match crate::win32::FILE_HANDLES.lock().get(&h) {
            Some(state) => state.fd,
            None => {
                crate::serial_println!("[WIN32] WriteFile: invalid handle {:#x}", h);
                if !lpNumberOfBytesWritten.is_null() {
                    *lpNumberOfBytesWritten = 0;
                }
                return FALSE;
            }
        };

        // Create buffer slice
        let buf = core::slice::from_raw_parts(lpBuffer, nNumberOfBytesToWrite as usize);

        // Write via VFS
        match crate::fs::sys_write(fd, buf) {
            Ok(bytes_written) => {
                if !lpNumberOfBytesWritten.is_null() {
                    *lpNumberOfBytesWritten = bytes_written as DWORD;
                }
                TRUE
            }
            Err(_e) => {
                crate::serial_println!("[WIN32] WriteFile failed: fd={}", fd);
                if !lpNumberOfBytesWritten.is_null() {
                    *lpNumberOfBytesWritten = 0;
                }
                FALSE
            }
        }
    }

    /// CloseHandle - Closes an open handle (file, thread, process, etc.)
    pub unsafe fn close_handle(hObject: HANDLE) -> BOOL {
        let h = hObject as u64;

        // Console handles are not closable
        if h == crate::win32::STD_INPUT_HANDLE
            || h == crate::win32::STD_OUTPUT_HANDLE
            || h == crate::win32::STD_ERROR_HANDLE
        {
            return TRUE;
        }

        // Check file handle table
        if let Some(state) = crate::win32::FILE_HANDLES.lock().remove(&h) {
            crate::serial_println!("[WIN32] CloseHandle: fd={} path={}", state.fd, state.path);
            crate::fs::sys_close(state.fd);
            return TRUE;
        }

        if crate::win32_abi::ntdll_nt_close(h) == crate::win32_abi::NtStatus::Success as i32 {
            return TRUE;
        }

        crate::serial_println!("[WIN32] CloseHandle: unknown handle {:#x}", h);
        FALSE
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
    pub unsafe fn open_process(
        dwDesiredAccess: DWORD,
        bInheritHandle: BOOL,
        dwProcessId: DWORD,
    ) -> HANDLE {
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
        let timeout_100ns = if dwMilliseconds == 0xFFFF_FFFF {
            0
        } else {
            -((dwMilliseconds as i64) * 10_000)
        };
        let timeout_ptr = if dwMilliseconds == 0xFFFF_FFFF {
            core::ptr::null()
        } else {
            &timeout_100ns as *const i64
        };
        match crate::win32_abi::ntdll_nt_wait_for_single_object(hHandle as u64, 0, timeout_ptr) {
            value if value == crate::win32_abi::NtStatus::Success as i32 => 0,
            value if value == crate::win32_abi::NtStatus::Timeout as i32 => 258,
            _ => u32::MAX,
        }
    }

    /// WaitForMultipleObjects
    pub unsafe fn wait_for_multiple_objects(
        nCount: DWORD,
        lpHandles: *const HANDLE,
        bWaitAll: BOOL,
        dwMilliseconds: DWORD,
    ) -> DWORD {
        if lpHandles.is_null() || nCount == 0 {
            return u32::MAX;
        }
        let handles = core::slice::from_raw_parts(lpHandles, nCount as usize);
        if bWaitAll != 0 {
            for handle in handles {
                let result = wait_for_single_object(*handle, dwMilliseconds);
                if result != 0 {
                    return result;
                }
            }
            0
        } else {
            for (index, handle) in handles.iter().enumerate() {
                if wait_for_single_object(*handle, 0) == 0 {
                    return index as u32;
                }
            }
            if dwMilliseconds == 0 {
                258
            } else {
                wait_for_single_object(handles[0], dwMilliseconds)
            }
        }
    }

    pub unsafe fn wait_on_address(
        address: LPVOID,
        compare_address: LPCVOID,
        address_size: SIZE_T,
        dwMilliseconds: DWORD,
    ) -> BOOL {
        crate::win32_abi::wait_on_address(
            address as *const u8,
            compare_address as *const u8,
            address_size as usize,
            dwMilliseconds,
        ) as BOOL
    }

    pub unsafe fn wake_by_address_single(address: LPVOID) {
        crate::win32_abi::wake_by_address_single(address as *const u8);
    }

    pub unsafe fn wake_by_address_all(address: LPVOID) {
        crate::win32_abi::wake_by_address_all(address as *const u8);
    }

    pub unsafe fn create_io_ring(
        version: DWORD,
        flags: DWORD,
        submission_queue_size: DWORD,
        completion_queue_size: DWORD,
        ring_handle: *mut HANDLE,
    ) -> HRESULT {
        if ring_handle.is_null() {
            return -1;
        }
        match crate::win32_abi::create_io_ring(
            version,
            flags,
            submission_queue_size,
            completion_queue_size,
        ) {
            Ok(handle) => {
                *ring_handle = handle;
                0
            }
            Err(status) => status as i32,
        }
    }

    pub unsafe fn build_io_ring_register_file_handles(
        ring_handle: HANDLE,
        handles: *const HANDLE,
        count: DWORD,
    ) -> HRESULT {
        crate::win32_abi::register_io_ring_handles(ring_handle, handles as *const u64, count) as i32
    }

    pub unsafe fn build_io_ring_register_buffers(
        ring_handle: HANDLE,
        buffers: *const IoRingBufferDescriptor,
        count: DWORD,
    ) -> HRESULT {
        crate::win32_abi::register_io_ring_buffers(
            ring_handle,
            buffers as *const crate::win32_abi::IoRingBufferDescriptor,
            count,
        ) as i32
    }

    pub unsafe fn build_io_ring_read_file(
        ring_handle: HANDLE,
        file_index: DWORD,
        buffer_index: DWORD,
        file_offset: u64,
        bytes_to_read: DWORD,
        user_data: u64,
        flags: DWORD,
    ) -> HRESULT {
        crate::win32_abi::build_io_ring_read_file(
            ring_handle,
            file_index,
            buffer_index,
            file_offset,
            bytes_to_read,
            user_data,
            flags,
        ) as i32
    }

    pub unsafe fn build_io_ring_write_file(
        ring_handle: HANDLE,
        file_index: DWORD,
        buffer_index: DWORD,
        file_offset: u64,
        bytes_to_write: DWORD,
        user_data: u64,
        flags: DWORD,
    ) -> HRESULT {
        crate::win32_abi::build_io_ring_write_file(
            ring_handle,
            file_index,
            buffer_index,
            file_offset,
            bytes_to_write,
            user_data,
            flags,
        ) as i32
    }

    pub unsafe fn submit_io_ring(
        ring_handle: HANDLE,
        to_submit: DWORD,
        min_complete: DWORD,
        flags: DWORD,
    ) -> HRESULT {
        crate::win32_abi::submit_io_ring(ring_handle, to_submit, min_complete, flags) as i32
    }

    pub unsafe fn pop_io_ring_completion(
        ring_handle: HANDLE,
        completion: *mut IoRingCompletionEntry,
    ) -> HRESULT {
        if completion.is_null() {
            return -1;
        }
        let mut raw = crate::win32_abi::IoRingCompletion::default();
        let status = crate::win32_abi::pop_io_ring_completion(ring_handle, &mut raw);
        if status == crate::win32_abi::NtStatus::Success {
            *completion = IoRingCompletionEntry {
                user_data: raw.user_data,
                result_code: raw.result_code,
                information: raw.information,
                operation: raw.operation,
            };
        }
        status as i32
    }

    pub unsafe fn close_io_ring(ring_handle: HANDLE) -> HRESULT {
        if close_handle(ring_handle) != 0 {
            0
        } else {
            -1
        }
    }

    // ========================================================================
    // SYNCHRONIZATION PRIMITIVES
    // ========================================================================

    /// Sync object state tracking
    static SYNC_OBJECTS: Mutex<BTreeMap<u64, SyncObject>> = Mutex::new(BTreeMap::new());
    static NEXT_SYNC_HANDLE: core::sync::atomic::AtomicU64 =
        core::sync::atomic::AtomicU64::new(0x2000_0000);

    struct SyncObject {
        obj_type: SyncType,
        signaled: bool,
        auto_reset: bool,
        count: i32,     // For semaphores
        max_count: i32, // For semaphores
    }

    #[derive(Clone, Copy, PartialEq)]
    enum SyncType {
        Mutex,
        Event,
        Semaphore,
    }

    /// CreateMutexA - Creates a mutex object
    pub unsafe fn create_mutex_a(
        lpMutexAttributes: LPVOID,
        bInitialOwner: BOOL,
        lpName: LPCSTR,
    ) -> HANDLE {
        let handle = NEXT_SYNC_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        SYNC_OBJECTS.lock().insert(
            handle,
            SyncObject {
                obj_type: SyncType::Mutex,
                signaled: bInitialOwner == 0, // If not owned, it's signaled
                auto_reset: true,
                count: 0,
                max_count: 1,
            },
        );
        crate::serial_println!("[WIN32] CreateMutexA -> {:#x}", handle);
        handle as HANDLE
    }

    /// CreateEventA - Creates an event object
    pub unsafe fn create_event_a(
        lpEventAttributes: LPVOID,
        bManualReset: BOOL,
        bInitialState: BOOL,
        lpName: LPCSTR,
    ) -> HANDLE {
        let _ = (lpEventAttributes, lpName);
        crate::win32_abi::create_waitable_event(bManualReset != 0, bInitialState != 0)
            .unwrap_or(0)
    }

    /// CreateSemaphoreA - Creates a semaphore object
    pub unsafe fn create_semaphore_a(
        lpSemaphoreAttributes: LPVOID,
        lInitialCount: LONG,
        lMaximumCount: LONG,
        lpName: LPCSTR,
    ) -> HANDLE {
        let handle = NEXT_SYNC_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        SYNC_OBJECTS.lock().insert(
            handle,
            SyncObject {
                obj_type: SyncType::Semaphore,
                signaled: lInitialCount > 0,
                auto_reset: true,
                count: lInitialCount,
                max_count: lMaximumCount,
            },
        );
        crate::serial_println!("[WIN32] CreateSemaphoreA -> {:#x}", handle);
        handle as HANDLE
    }

    /// SetEvent - Sets an event to signaled state
    pub unsafe fn set_event(hEvent: HANDLE) -> BOOL {
        if crate::win32_abi::set_waitable_event(hEvent as u64) == crate::win32_abi::NtStatus::Success
        {
            TRUE
        } else {
            FALSE
        }
    }

    /// ResetEvent - Resets an event to non-signaled state
    pub unsafe fn reset_event(hEvent: HANDLE) -> BOOL {
        if crate::win32_abi::reset_waitable_event(hEvent as u64)
            == crate::win32_abi::NtStatus::Success
        {
            TRUE
        } else {
            FALSE
        }
    }

    /// PulseEvent - Sets event, releases waiting threads, then resets
    pub unsafe fn pulse_event(hEvent: HANDLE) -> BOOL {
        if let Some(obj) = SYNC_OBJECTS.lock().get_mut(&(hEvent as u64)) {
            if obj.obj_type == SyncType::Event {
                // Pulse: set then reset (legacy API)
                obj.signaled = false;
                return TRUE;
            }
        }
        FALSE
    }

    /// ReleaseMutex - Releases ownership of a mutex
    pub unsafe fn release_mutex(hMutex: HANDLE) -> BOOL {
        if let Some(obj) = SYNC_OBJECTS.lock().get_mut(&(hMutex as u64)) {
            if obj.obj_type == SyncType::Mutex {
                obj.signaled = true; // Released, now signaled
                return TRUE;
            }
        }
        FALSE
    }

    /// ReleaseSemaphore - Increments semaphore count
    pub unsafe fn release_semaphore(
        hSemaphore: HANDLE,
        lReleaseCount: LONG,
        lpPreviousCount: *mut LONG,
    ) -> BOOL {
        if let Some(obj) = SYNC_OBJECTS.lock().get_mut(&(hSemaphore as u64)) {
            if obj.obj_type == SyncType::Semaphore {
                if !lpPreviousCount.is_null() {
                    *lpPreviousCount = obj.count;
                }
                obj.count = (obj.count + lReleaseCount).min(obj.max_count);
                obj.signaled = obj.count > 0;
                return TRUE;
            }
        }
        FALSE
    }

    /// InitializeCriticalSection - Initialize a critical section
    pub unsafe fn initialize_critical_section(lpCriticalSection: LPVOID) {
        // Critical sections are in-process mutexes, just clear the memory
        if !lpCriticalSection.is_null() {
            core::ptr::write_bytes(lpCriticalSection, 0, 24); // RTL_CRITICAL_SECTION size
        }
    }

    /// EnterCriticalSection - Enter a critical section
    pub unsafe fn enter_critical_section(lpCriticalSection: LPVOID) {
        // No-op in single-threaded emulation
    }

    /// LeaveCriticalSection - Leave a critical section
    pub unsafe fn leave_critical_section(lpCriticalSection: LPVOID) {
        // No-op in single-threaded emulation
    }

    /// DeleteCriticalSection - Delete a critical section
    pub unsafe fn delete_critical_section(lpCriticalSection: LPVOID) {
        // No-op
    }

    /// TryEnterCriticalSection - Try to enter a critical section
    pub unsafe fn try_enter_critical_section(lpCriticalSection: LPVOID) -> BOOL {
        TRUE // Always succeeds in single-threaded emulation
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
    pub unsafe fn virtual_query(lpAddress: LPCVOID, lpBuffer: LPVOID, dwLength: SIZE_T) -> SIZE_T {
        0
    }

    /// HeapCreate
    pub unsafe fn heap_create(
        flOptions: DWORD,
        dwInitialSize: SIZE_T,
        dwMaximumSize: SIZE_T,
    ) -> HANDLE {
        crate::serial_println!("[WIN32] HeapCreate: size={}", dwInitialSize);
        1 as HANDLE
    }

    /// HeapDestroy
    pub unsafe fn heap_destroy(hHeap: HANDLE) -> BOOL {
        TRUE
    }

    /// HeapAlloc
    pub unsafe fn heap_alloc(_hHeap: HANDLE, _dwFlags: DWORD, dwBytes: SIZE_T) -> LPVOID {
        let ptr = crate::win32::win32_alloc(dwBytes as usize, 16) as LPVOID;
        crate::serial_println!("[WIN32] HeapAlloc {} bytes -> {:p}", dwBytes, ptr);
        ptr
    }

    /// HeapFree
    pub unsafe fn heap_free(_hHeap: HANDLE, _dwFlags: DWORD, lpMem: LPVOID) -> BOOL {
        crate::win32::win32_dealloc(lpMem as *mut u8);
        TRUE
    }

    /// HeapReAlloc
    pub unsafe fn heap_realloc(
        _hHeap: HANDLE,
        _dwFlags: DWORD,
        lpMem: LPVOID,
        dwBytes: SIZE_T,
    ) -> LPVOID {
        crate::win32::win32_realloc(lpMem as *mut u8, dwBytes as usize) as LPVOID
    }

    /// HeapSize
    pub unsafe fn heap_size(_hHeap: HANDLE, _dwFlags: DWORD, lpMem: LPCVOID) -> SIZE_T {
        // Return tracked size if available
        match crate::win32::ALLOC_MAP.lock().get(&(lpMem as u64)) {
            Some(&(sz, _)) => sz as SIZE_T,
            None => SIZE_T::MAX, // HEAP_SIZE returns (SIZE_T)-1 on failure
        }
    }

    /// GetProcessHeap
    pub unsafe fn get_process_heap() -> HANDLE {
        1 as HANDLE
    }

    /// LocalAlloc
    pub unsafe fn local_alloc(_uFlags: DWORD, uBytes: SIZE_T) -> HANDLE {
        // Treat as heap alloc; return pointer as pseudo-handle
        crate::win32::win32_alloc(uBytes as usize, 16) as HANDLE
    }

    /// LocalFree
    pub unsafe fn local_free(hMem: HANDLE) -> HANDLE {
        crate::win32::win32_dealloc(hMem as *mut u8);
        0
    }

    /// GlobalAlloc
    pub unsafe fn global_alloc(_uFlags: DWORD, dwBytes: SIZE_T) -> HANDLE {
        crate::win32::win32_alloc(dwBytes as usize, 16) as HANDLE
    }

    /// GlobalFree
    pub unsafe fn global_free(hMem: HANDLE) -> HANDLE {
        crate::win32::win32_dealloc(hMem as *mut u8);
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
    pub unsafe fn copy_file_a(
        lpExistingFileName: LPCSTR,
        lpNewFileName: LPCSTR,
        bFailIfExists: BOOL,
    ) -> BOOL {
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
    pub unsafe fn get_environment_variable_a(
        lpName: LPCSTR,
        lpBuffer: LPSTR,
        nSize: DWORD,
    ) -> DWORD {
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
    pub unsafe fn get_console_screen_buffer_info(
        hConsoleOutput: HANDLE,
        lpConsoleScreenBufferInfo: LPVOID,
    ) -> BOOL {
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

    /// CreateWindowExA — Gerçek pencere oluşturur ve echOS compositor'e kaydeder
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
        // Parse class name
        let class_name = if !lpClassName.is_null() {
            let mut s = String::new();
            let mut ptr = lpClassName;
            while *ptr != 0 {
                s.push(*ptr as u8 as char);
                ptr = ptr.add(1);
            }
            s
        } else {
            String::from("UnknownClass")
        };

        // Parse window title
        let title = if !lpWindowName.is_null() {
            let mut s = String::new();
            let mut ptr = lpWindowName;
            while *ptr != 0 {
                s.push(*ptr as u8 as char);
                ptr = ptr.add(1);
            }
            s
        } else {
            String::from("Untitled")
        };

        // CW_USEDEFAULT işleme
        let actual_x = if x == 0x80000000_u32 as i32 { 100 } else { x };
        let actual_y = if y == 0x80000000_u32 as i32 { 100 } else { y };
        let actual_w = if nWidth == 0x80000000_u32 as i32 {
            640
        } else {
            nWidth
        };
        let actual_h = if nHeight == 0x80000000_u32 as i32 {
            480
        } else {
            nHeight
        };

        // Yeni HWND ata
        let hwnd = crate::win32::NEXT_HWND.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        // Pencere surface'i oluştur (BGRA format)
        let surface_size = (actual_w * actual_h * 4) as usize;
        let mut surface = vec![0u8; surface_size];

        // Varsayılan arkaplan rengi: açık gri (#C0C0C0)
        for i in 0..(actual_w * actual_h) as usize {
            surface[i * 4] = 0xC0; // B
            surface[i * 4 + 1] = 0xC0; // G
            surface[i * 4 + 2] = 0xC0; // R
            surface[i * 4 + 3] = 0xFF; // A (opak)
        }

        // Win32Window oluştur
        let win = crate::win32::Win32Window {
            hwnd,
            class_name: class_name.clone(),
            title: title.clone(),
            x: actual_x,
            y: actual_y,
            width: actual_w,
            height: actual_h,
            style: dwStyle,
            ex_style: dwExStyle,
            parent: hWndParent as u64,
            visible: false,
            focused: false,
            echos_window_id: hwnd as u32,
            surface,
            wndproc: 0,
        };

        crate::win32::WIN32_WINDOWS.lock().insert(hwnd, win);

        crate::serial_println!(
            "[WIN32] CreateWindowExA: hwnd={:#x} class='{}' title='{}' pos={},{} size={}x{}",
            hwnd,
            class_name,
            title,
            actual_x,
            actual_y,
            actual_w,
            actual_h
        );

        // WM_CREATE mesajı gönder
        crate::win32::post_message(hwnd, crate::win32::WM_CREATE, 0, 0);

        hwnd as HWND
    }

    /// ShowWindow — Pencereyi görünür/görünmez yapar ve framebuffer'a çizer
    pub unsafe fn show_window(hWnd: HWND, nCmdShow: INT) -> BOOL {
        let hwnd = hWnd as u64;
        let mut windows = crate::win32::WIN32_WINDOWS.lock();

        if let Some(win) = windows.get_mut(&hwnd) {
            let was_visible = win.visible;

            // SW_HIDE=0, SW_SHOW=5, SW_MINIMIZE=6, SW_MAXIMIZE=3, SW_RESTORE=9
            win.visible = match nCmdShow {
                0 => false, // SW_HIDE
                _ => true,  // Diğerleri
            };

            crate::serial_println!(
                "[WIN32] ShowWindow: hwnd={:#x} cmd={} visible={}",
                hwnd,
                nCmdShow,
                win.visible
            );

            if win.visible {
                // WM_SHOWWINDOW + WM_PAINT mesajları
                drop(windows); // Kilidi serbest bırak
                crate::win32::post_message(hwnd, crate::win32::WM_SHOWWINDOW, 1, 0);
                crate::win32::post_message(hwnd, crate::win32::WM_PAINT, 0, 0);

                // Pencereyi framebuffer'a çiz
                crate::win32::blit_window_to_framebuffer(hwnd);
            }

            if was_visible {
                TRUE
            } else {
                FALSE
            }
        } else {
            FALSE
        }
    }

    /// UpdateWindow — WM_PAINT tetikler ve pencereyi çizer
    pub unsafe fn update_window(hWnd: HWND) -> BOOL {
        let hwnd = hWnd as u64;
        crate::win32::post_message(hwnd, crate::win32::WM_PAINT, 0, 0);
        crate::win32::blit_window_to_framebuffer(hwnd);
        TRUE
    }

    /// GetMessageA — Mesaj kuyruğundan mesaj alır (blocking loop için busy-wait)
    pub unsafe fn get_message_a(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: UINT,
        wMsgFilterMax: UINT,
    ) -> BOOL {
        if lpMsg.is_null() {
            return FALSE;
        }

        let hwnd_filter = hWnd as u64;

        // Mesaj kuyruğunu kontrol et (busy-wait değil, tek kontrol)
        // Gerçek blocking için scheduler entegrasyonu gerekir
        loop {
            // Fare ve klavye olaylarını mesaj kuyruğuna ekle
            poll_input_events();

            if let Some(msg) = crate::win32::peek_message(hwnd_filter) {
                (*lpMsg).hwnd = msg.hwnd as HWND;
                (*lpMsg).message = msg.message;
                (*lpMsg).wParam = msg.wparam as usize;
                (*lpMsg).lParam = msg.lparam as isize;
                (*lpMsg).time = msg.time;
                (*lpMsg).pt.x = msg.pt_x;
                (*lpMsg).pt.y = msg.pt_y;

                // WM_QUIT ise FALSE dön (döngüden çık)
                if msg.message == crate::win32::WM_QUIT {
                    return FALSE;
                }
                return TRUE;
            }

            // Kısa bekleme (CPU'yu yormamak için)
            for _ in 0..10000 {
                core::hint::spin_loop();
            }
        }
    }

    /// Fare ve klavye olaylarını Win32 mesaj kuyruğuna çevir
    fn poll_input_events() {
        // Fare pozisyonunu al
        let (mx, my) = crate::drivers::mouse::get_position();
        let buttons = crate::drivers::mouse::get_buttons();

        // Aktif pencereyi bul
        let active_hwnd = crate::win32::ACTIVE_HWND.load(core::sync::atomic::Ordering::Relaxed);
        if active_hwnd == 0 {
            // İlk görünür pencereyi aktif yap
            let windows = crate::win32::WIN32_WINDOWS.lock();
            for (&hwnd, win) in windows.iter() {
                if win.visible {
                    drop(windows);
                    crate::win32::ACTIVE_HWND.store(hwnd, core::sync::atomic::Ordering::Relaxed);
                    break;
                }
            }
        }

        // Fare hareket mesajı (her frame'de değil, değişiklikte)
        static LAST_MX: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
        static LAST_MY: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

        let last_x = LAST_MX.load(core::sync::atomic::Ordering::Relaxed);
        let last_y = LAST_MY.load(core::sync::atomic::Ordering::Relaxed);

        if mx != last_x || my != last_y {
            LAST_MX.store(mx, core::sync::atomic::Ordering::Relaxed);
            LAST_MY.store(my, core::sync::atomic::Ordering::Relaxed);

            let hwnd = crate::win32::ACTIVE_HWND.load(core::sync::atomic::Ordering::Relaxed);
            if hwnd != 0 {
                let lparam = ((my as i64 & 0xFFFF) << 16) | (mx as i64 & 0xFFFF);
                crate::win32::post_message(hwnd, crate::win32::WM_MOUSEMOVE, 0, lparam);
            }
        }

        // Fare buton mesajları
        static LAST_LEFT: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        static LAST_RIGHT: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);

        let last_left = LAST_LEFT.load(core::sync::atomic::Ordering::Relaxed);
        let last_right = LAST_RIGHT.load(core::sync::atomic::Ordering::Relaxed);

        if buttons.left != last_left || buttons.right != last_right {
            LAST_LEFT.store(buttons.left, core::sync::atomic::Ordering::Relaxed);
            LAST_RIGHT.store(buttons.right, core::sync::atomic::Ordering::Relaxed);
            let hwnd = crate::win32::ACTIVE_HWND.load(core::sync::atomic::Ordering::Relaxed);

            if hwnd != 0 {
                let lparam = ((my as i64 & 0xFFFF) << 16) | (mx as i64 & 0xFFFF);

                // Sol buton
                if buttons.left && !last_left {
                    crate::win32::post_message(hwnd, crate::win32::WM_LBUTTONDOWN, 1, lparam);
                } else if !buttons.left && last_left {
                    crate::win32::post_message(hwnd, crate::win32::WM_LBUTTONUP, 0, lparam);
                }

                // Sağ buton
                if buttons.right && !last_right {
                    crate::win32::post_message(hwnd, crate::win32::WM_RBUTTONDOWN, 2, lparam);
                } else if !buttons.right && last_right {
                    crate::win32::post_message(hwnd, crate::win32::WM_RBUTTONUP, 0, lparam);
                }
            }
        }
    }

    /// TranslateMessage — Sanal tuş kodlarını karakter mesajlarına çevirir
    pub unsafe fn translate_message(lpMsg: *const MSG) -> BOOL {
        if lpMsg.is_null() {
            return FALSE;
        }

        let msg = &*lpMsg;
        // WM_KEYDOWN -> WM_CHAR çevirisi
        if msg.message == crate::win32::WM_KEYDOWN {
            let vk = msg.wParam as u8;
        // Dar ASCII dönüşümü; yalnız temel tek bayt karakter kümesini kapsar
            let ch = match vk {
                0x30..=0x39 => vk,        // 0-9
                0x41..=0x5A => vk + 0x20, // A-Z -> a-z
                0x20 => 0x20,             // Space
                0x0D => 0x0D,             // Enter
                0x08 => 0x08,             // Backspace
                _ => 0,
            };

            if ch != 0 {
                crate::win32::post_message(
                    msg.hwnd as u64,
                    crate::win32::WM_CHAR,
                    ch as u64,
                    msg.lParam as i64,
                );
            }
        }
        TRUE
    }

    /// DispatchMessageA — Mesajı pencere prosedürüne iletir
    pub unsafe fn dispatch_message_a(lpMsg: *const MSG) -> isize {
        if lpMsg.is_null() {
            return 0;
        }

        let msg = &*lpMsg;
        let hwnd = msg.hwnd as u64;

        // Pencere prosedürünü bul ve çağır
        let wndproc = {
            let windows = crate::win32::WIN32_WINDOWS.lock();
            windows.get(&hwnd).map(|w| w.wndproc).unwrap_or(0)
        };

        if wndproc != 0 {
            // WndProc(HWND, UINT, WPARAM, LPARAM) -> LRESULT
            type WndProcFn = unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize;
            let func: WndProcFn = core::mem::transmute(wndproc);
            return func(msg.hwnd, msg.message, msg.wParam, msg.lParam);
        }

        // Varsayılan işleme
        def_window_proc_a(msg.hwnd, msg.message, msg.wParam, msg.lParam)
    }

    /// PostQuitMessage — WM_QUIT mesajı gönderir
    pub unsafe fn post_quit_message(nExitCode: INT) {
        crate::serial_println!("[WIN32] PostQuitMessage({})", nExitCode);
        crate::win32::post_message(0, crate::win32::WM_QUIT, nExitCode as u64, 0);
    }

    /// DefWindowProcA — Varsayılan pencere prosedürü
    pub unsafe fn def_window_proc_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> isize {
        let hwnd = hWnd as u64;

        match Msg {
            0x0010 => {
                // WM_CLOSE
                // Pencereyi kapat
                crate::win32::WIN32_WINDOWS.lock().remove(&hwnd);
                crate::win32::post_message(hwnd, crate::win32::WM_DESTROY, 0, 0);
                0
            }
            0x0002 => {
                // WM_DESTROY
                crate::win32::post_message(0, crate::win32::WM_QUIT, 0, 0);
                0
            }
            0x0014 => {
                // WM_ERASEBKGND
                // Arkaplanı temizle
                1
            }
            _ => 0,
        }
    }

    /// GetDC — Pencere için Device Context oluşturur
    pub unsafe fn get_dc(hWnd: HWND) -> HDC {
        let hwnd = hWnd as u64;

        // Yeni DC oluştur
        let hdc = crate::win32::NEXT_HDC.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let dc = crate::win32::Win32DC {
            hdc,
            hwnd,
            pen_color: 0x000000,   // Siyah
            brush_color: 0xFFFFFF, // Beyaz
            text_color: 0x000000,  // Siyah
            bk_color: 0xFFFFFF,    // Beyaz
            bk_mode: 2,            // OPAQUE
            pen_x: 0,
            pen_y: 0,
            font_height: 16,
            font_weight: 400,
        };

        crate::win32::WIN32_DCS.lock().insert(hdc, dc);

        crate::serial_println!("[WIN32] GetDC: hwnd={:#x} -> hdc={:#x}", hwnd, hdc);
        hdc as HDC
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
    pub unsafe fn move_window(
        hWnd: HWND,
        x: INT,
        y: INT,
        nWidth: INT,
        nHeight: INT,
        bRepaint: BOOL,
    ) -> BOOL {
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
    pub unsafe fn enum_windows(
        lpEnumFunc: Option<unsafe extern "system" fn(HWND, usize) -> BOOL>,
        lParam: usize,
    ) -> BOOL {
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
    pub unsafe fn send_notify_message_a(
        hWnd: HWND,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
    ) -> BOOL {
        TRUE
    }

    /// PostThreadMessageA
    pub unsafe fn post_thread_message_a(
        idThread: DWORD,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
    ) -> BOOL {
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
    pub unsafe fn to_ascii(
        uVirtKey: UINT,
        uScanCode: UINT,
        lpKeyState: *const BYTE,
        lpChar: *mut WORD,
        uFlags: UINT,
    ) -> INT {
        0
    }

    /// ToUnicode
    pub unsafe fn to_unicode(
        wVirtKey: UINT,
        wScanCode: UINT,
        lpKeyState: *const BYTE,
        pwszBuff: *mut u16,
        cchBuff: INT,
        wFlags: UINT,
    ) -> INT {
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
    pub unsafe fn mouse_event(
        dwFlags: DWORD,
        dx: DWORD,
        dy: DWORD,
        cButtons: DWORD,
        dwExtraInfo: usize,
    ) {
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
            0 => 640, // SM_CXSCREEN
            1 => 480, // SM_CYSCREEN
            2 => 0,   // SM_CXVSCROLL
            3 => 0,   // SM_CYHSCROLL
            4 => 640, // SM_CXSIZE
            5 => 480, // SM_CYSIZE
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
    pub unsafe fn append_menu_a(
        hMenu: HMENU,
        uFlags: UINT,
        uIDNewItem: usize,
        lpNewItem: LPCSTR,
    ) -> BOOL {
        TRUE
    }

    /// InsertMenuA
    pub unsafe fn insert_menu_a(
        hMenu: HMENU,
        uPosition: UINT,
        uFlags: UINT,
        uIDNewItem: usize,
        lpNewItem: LPCSTR,
    ) -> BOOL {
        TRUE
    }

    /// ModifyMenuA
    pub unsafe fn modify_menu_a(
        hMnu: HMENU,
        uPosition: UINT,
        uFlags: UINT,
        uIDNewItem: usize,
        lpNewItem: LPCSTR,
    ) -> BOOL {
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
    pub unsafe fn get_menu_string_a(
        hMenu: HMENU,
        uItem: UINT,
        lpString: LPSTR,
        nMaxCount: INT,
        uFlag: UINT,
    ) -> INT {
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
    pub unsafe fn message_box_ex_a(
        hWnd: HWND,
        lpText: LPCSTR,
        lpCaption: LPCSTR,
        uType: UINT,
        wLanguageId: WORD,
    ) -> INT {
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
    pub unsafe fn get_dlg_item_text_a(
        hDlg: HWND,
        nIDDlgItem: INT,
        lpString: LPSTR,
        nMaxCount: INT,
    ) -> UINT {
        0
    }

    /// SetDlgItemInt
    pub unsafe fn set_dlg_item_int(
        hDlg: HWND,
        nIDDlgItem: INT,
        uValue: UINT,
        bSigned: BOOL,
    ) -> BOOL {
        TRUE
    }

    /// GetDlgItemInt
    pub unsafe fn get_dlg_item_int(
        hDlg: HWND,
        nIDDlgItem: INT,
        lpTranslated: *mut BOOL,
        bSigned: BOOL,
    ) -> UINT {
        0
    }

    /// CheckDlgButton
    pub unsafe fn check_dlg_button(hDlg: HWND, nIDButton: INT, uCheck: UINT) -> BOOL {
        TRUE
    }

    /// CheckRadioButton
    pub unsafe fn check_radio_button(
        hDlg: HWND,
        nIDFirstButton: INT,
        nIDLastButton: INT,
        nIDCheckButton: INT,
    ) -> BOOL {
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
    pub unsafe fn send_dlg_item_message_a(
        hDlg: HWND,
        nIDDlgItem: INT,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
    ) -> isize {
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
    pub unsafe fn set_timer(
        hWnd: HWND,
        nIDEvent: usize,
        uElapse: UINT,
        lpTimerFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, DWORD)>,
    ) -> usize {
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
    pub unsafe fn load_string_a(
        hInstance: HINSTANCE,
        uID: UINT,
        lpBuffer: LPSTR,
        nBufferMax: INT,
    ) -> INT {
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
    pub unsafe fn copy_image(
        hImage: HANDLE,
        uType: UINT,
        cxDesired: INT,
        cyDesired: INT,
        fuFlags: UINT,
    ) -> HANDLE {
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
    pub unsafe fn call_next_hook_ex(
        hhk: HANDLE,
        nCode: INT,
        wParam: usize,
        lParam: isize,
    ) -> isize {
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
    pub unsafe fn enum_props_a(
        hWnd: HWND,
        lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPCSTR, HANDLE) -> BOOL>,
    ) -> INT {
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
    // IN-MEMORY REGISTRY STORE
    // ========================================================================

    /// Registry value types
    const REG_NONE: DWORD = 0;
    const REG_SZ: DWORD = 1;
    const REG_EXPAND_SZ: DWORD = 2;
    const REG_BINARY: DWORD = 3;
    const REG_DWORD: DWORD = 4;

    /// Registry key handle mapping
    static REGISTRY_HANDLES: Mutex<BTreeMap<u64, String>> = Mutex::new(BTreeMap::new());
    /// Registry key-value store: "HKEY\subkey\value" -> (type, data)
    static REGISTRY_DATA: Mutex<BTreeMap<String, (DWORD, Vec<u8>)>> = Mutex::new(BTreeMap::new());
    static NEXT_REG_HANDLE: core::sync::atomic::AtomicU64 =
        core::sync::atomic::AtomicU64::new(0x8000_0001);

    /// Predefined registry handles
    const HKEY_CLASSES_ROOT: u64 = 0x80000000;
    const HKEY_CURRENT_USER: u64 = 0x80000001;
    const HKEY_LOCAL_MACHINE: u64 = 0x80000002;
    const HKEY_USERS: u64 = 0x80000003;

    fn hkey_to_string(hkey: u64) -> String {
        match hkey {
            0x80000000 => String::from("HKEY_CLASSES_ROOT"),
            0x80000001 => String::from("HKEY_CURRENT_USER"),
            0x80000002 => String::from("HKEY_LOCAL_MACHINE"),
            0x80000003 => String::from("HKEY_USERS"),
            _ => {
                // Check if it's an opened handle
                if let Some(path) = REGISTRY_HANDLES.lock().get(&hkey) {
                    return path.clone();
                }
                String::from("UNKNOWN")
            }
        }
    }

    // ========================================================================
    // REGISTRY
    // ========================================================================

    /// RegOpenKeyExA - Opens a registry key
    pub unsafe fn reg_open_key_ex_a(
        hKey: HKEY,
        lpSubKey: LPCSTR,
        ulOptions: DWORD,
        samDesired: DWORD,
        phkResult: *mut HKEY,
    ) -> LONG {
        if lpSubKey.is_null() || phkResult.is_null() {
            return 87; // ERROR_INVALID_PARAMETER
        }

        // Parse subkey name
        let mut subkey = String::new();
        let mut ptr = lpSubKey;
        while !ptr.is_null() && *ptr != 0 {
            subkey.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }

        // Build full path
        let parent_path = hkey_to_string(hKey as u64);
        let full_path = if subkey.is_empty() {
            parent_path
        } else {
            let mut path = parent_path;
            path.push('\\');
            path.push_str(&subkey);
            path
        };

        // Allocate new handle and register it
        let handle = NEXT_REG_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        REGISTRY_HANDLES.lock().insert(handle, full_path.clone());

        *phkResult = handle as HKEY;
        crate::serial_println!(
            "[WIN32] RegOpenKeyExA: {} -> handle {:#x}",
            full_path,
            handle
        );
        0 // ERROR_SUCCESS
    }

    /// RegCloseKey - Closes a registry key handle
    pub unsafe fn reg_close_key(hKey: HKEY) -> LONG {
        REGISTRY_HANDLES.lock().remove(&(hKey as u64));
        0 // ERROR_SUCCESS
    }

    /// RegCreateKeyExA - Creates or opens a registry key
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
        if lpSubKey.is_null() || phkResult.is_null() {
            return 87; // ERROR_INVALID_PARAMETER
        }

        // Parse subkey name
        let mut subkey = String::new();
        let mut ptr = lpSubKey;
        while !ptr.is_null() && *ptr != 0 {
            subkey.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }

        // Build full path
        let parent_path = hkey_to_string(hKey as u64);
        let full_path = if subkey.is_empty() {
            parent_path
        } else {
            let mut path = parent_path;
            path.push('\\');
            path.push_str(&subkey);
            path
        };

        // Check if key exists
        let key_exists = REGISTRY_HANDLES.lock().values().any(|p| p == &full_path);

        // Allocate new handle
        let handle = NEXT_REG_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        REGISTRY_HANDLES.lock().insert(handle, full_path.clone());

        *phkResult = handle as HKEY;
        if !lpdwDisposition.is_null() {
            *lpdwDisposition = if key_exists { 2 } else { 1 }; // REG_OPENED_EXISTING_KEY : REG_CREATED_NEW_KEY
        }

        crate::serial_println!(
            "[WIN32] RegCreateKeyExA: {} -> handle {:#x}",
            full_path,
            handle
        );
        0 // ERROR_SUCCESS
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

    /// RegQueryValueExA - Queries a registry value
    pub unsafe fn reg_query_value_ex_a(
        hKey: HKEY,
        lpValueName: LPCSTR,
        lpReserved: *mut DWORD,
        lpType: *mut DWORD,
        lpData: *mut u8,
        lpcbData: *mut DWORD,
    ) -> LONG {
        // Get key path
        let key_path = hkey_to_string(hKey as u64);
        if key_path == "UNKNOWN" {
            return 6; // ERROR_INVALID_HANDLE
        }

        // Parse value name
        let value_name = if lpValueName.is_null() {
            String::new() // Default value
        } else {
            let mut name = String::new();
            let mut ptr = lpValueName;
            while !ptr.is_null() && *ptr != 0 {
                name.push(*ptr as u8 as char);
                ptr = ptr.add(1);
            }
            name
        };

        // Build full value path
        let full_path = if value_name.is_empty() {
            key_path
        } else {
            let mut path = key_path;
            path.push('\\');
            path.push_str(&value_name);
            path
        };

        // Lookup value in registry store
        let registry = REGISTRY_DATA.lock();
        if let Some((reg_type, data)) = registry.get(&full_path) {
            if !lpType.is_null() {
                *lpType = *reg_type;
            }

            let data_len = data.len() as DWORD;
            if !lpcbData.is_null() {
                let requested_size = *lpcbData;
                *lpcbData = data_len;

                if !lpData.is_null() && requested_size >= data_len {
                    core::ptr::copy_nonoverlapping(data.as_ptr(), lpData, data.len());
                } else if !lpData.is_null() {
                    return 234; // ERROR_MORE_DATA
                }
            }

            0 // ERROR_SUCCESS
        } else {
            2 // ERROR_FILE_NOT_FOUND
        }
    }

    /// RegSetValueExA - Sets a registry value
    pub unsafe fn reg_set_value_ex_a(
        hKey: HKEY,
        lpValueName: LPCSTR,
        Reserved: DWORD,
        dwType: DWORD,
        lpData: *const u8,
        cbData: DWORD,
    ) -> LONG {
        // Get key path
        let key_path = hkey_to_string(hKey as u64);
        if key_path == "UNKNOWN" {
            return 6; // ERROR_INVALID_HANDLE
        }

        // Parse value name
        let value_name = if lpValueName.is_null() {
            String::new() // Default value
        } else {
            let mut name = String::new();
            let mut ptr = lpValueName;
            while !ptr.is_null() && *ptr != 0 {
                name.push(*ptr as u8 as char);
                ptr = ptr.add(1);
            }
            name
        };

        // Build full value path
        let full_path = if value_name.is_empty() {
            key_path.clone()
        } else {
            let mut path = key_path.clone();
            path.push('\\');
            path.push_str(&value_name);
            path
        };

        // Copy data
        let data = if !lpData.is_null() && cbData > 0 {
            core::slice::from_raw_parts(lpData, cbData as usize).to_vec()
        } else {
            Vec::new()
        };

        // Store value
        REGISTRY_DATA
            .lock()
            .insert(full_path.clone(), (dwType, data));

        crate::serial_println!(
            "[WIN32] RegSetValueExA: {} type={} len={}",
            full_path,
            dwType,
            cbData
        );
        0 // ERROR_SUCCESS
    }

    /// RegConnectRegistryA
    pub unsafe fn reg_connect_registry_a(
        lpMachineName: LPCSTR,
        hKey: HKEY,
        phkResult: *mut HKEY,
    ) -> LONG {
        0
    }

    /// RegNotifyChangeKeyValue
    pub unsafe fn reg_notify_change_key_value(
        hKey: HKEY,
        bWatchSubtree: BOOL,
        dwNotifyFilter: DWORD,
        hEvent: HANDLE,
        fAsynchronous: BOOL,
    ) -> LONG {
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
    pub unsafe fn initialize_security_descriptor(
        pSecurityDescriptor: *mut u8,
        dwRevision: DWORD,
    ) -> BOOL {
        TRUE
    }

    /// InitializeAcl
    pub unsafe fn initialize_acl(pAcl: *mut u8, nAclLength: DWORD, dwAclRevision: DWORD) -> BOOL {
        TRUE
    }

    /// AddAccessAllowedAce
    pub unsafe fn add_access_allowed_ace(
        pAcl: *mut u8,
        dwAceRevision: DWORD,
        AccessMask: DWORD,
        pSid: *const u8,
    ) -> BOOL {
        TRUE
    }

    /// SetSecurityDescriptorDacl
    pub unsafe fn set_security_descriptor_dacl(
        pSecurityDescriptor: *mut u8,
        bDaclPresent: BOOL,
        pDacl: *const u8,
        bDaclDefaulted: BOOL,
    ) -> BOOL {
        TRUE
    }

    /// GetSecurityDescriptorDacl
    pub unsafe fn get_security_descriptor_dacl(
        pSecurityDescriptor: *const u8,
        lpbDaclPresent: *mut BOOL,
        pDacl: *mut *const u8,
        lpbDaclDefaulted: *mut BOOL,
    ) -> BOOL {
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
    pub unsafe fn copy_sid(
        nDestinationSidLength: DWORD,
        pDestinationSid: *mut u8,
        pSourceSid: *const u8,
    ) -> BOOL {
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
    pub unsafe fn open_sc_manager_a(
        lpMachineName: LPCSTR,
        lpDatabaseName: LPCSTR,
        dwDesiredAccess: DWORD,
    ) -> SC_HANDLE {
        1 as SC_HANDLE
    }

    /// CloseServiceHandle
    pub unsafe fn close_service_handle(hSCObject: SC_HANDLE) -> BOOL {
        TRUE
    }

    /// OpenServiceA
    pub unsafe fn open_service_a(
        hSCManager: SC_HANDLE,
        lpServiceName: LPCSTR,
        dwDesiredAccess: DWORD,
    ) -> SC_HANDLE {
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
    pub unsafe fn start_service_a(
        hService: SC_HANDLE,
        dwNumServiceArgs: DWORD,
        lpServiceArgVectors: *const LPCSTR,
    ) -> BOOL {
        TRUE
    }

    /// ControlService
    pub unsafe fn control_service(
        hService: SC_HANDLE,
        dwControl: DWORD,
        lpServiceStatus: *mut SERVICE_STATUS,
    ) -> BOOL {
        TRUE
    }

    /// QueryServiceStatus
    pub unsafe fn query_service_status(
        hService: SC_HANDLE,
        lpServiceStatus: *mut SERVICE_STATUS,
    ) -> BOOL {
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
    pub unsafe fn get_service_key_name_a(
        hSCManager: SC_HANDLE,
        lpDisplayName: LPCSTR,
        lpServiceName: LPSTR,
        lpcchBuffer: *mut DWORD,
    ) -> BOOL {
        FALSE
    }

    /// GetServiceDisplayNameA
    pub unsafe fn get_service_display_name_a(
        hSCManager: SC_HANDLE,
        lpServiceName: LPCSTR,
        lpDisplayName: LPSTR,
        lpcchBuffer: *mut DWORD,
    ) -> BOOL {
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
    pub unsafe fn get_number_of_event_log_records(
        hEventLog: HANDLE,
        NumberOfRecords: *mut DWORD,
    ) -> BOOL {
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
    pub unsafe fn crypt_create_hash(
        hProv: HCRYPTPROV,
        Algid: DWORD,
        hKey: HCRYPTKEY,
        dwFlags: DWORD,
        phHash: *mut HCRYPTHASH,
    ) -> BOOL {
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
    pub unsafe fn crypt_hash_data(
        hHash: HCRYPTHASH,
        pbData: *const BYTE,
        dwDataLen: DWORD,
        dwFlags: DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptGetHashParam
    pub unsafe fn crypt_get_hash_param(
        hHash: HCRYPTHASH,
        dwParam: DWORD,
        pbData: *mut BYTE,
        pdwDataLen: *mut DWORD,
        dwFlags: DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptDeriveKey
    pub unsafe fn crypt_derive_key(
        hProv: HCRYPTPROV,
        Algid: DWORD,
        hBaseData: HCRYPTHASH,
        dwFlags: DWORD,
        phKey: *mut HCRYPTKEY,
    ) -> BOOL {
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
    pub unsafe fn crypt_encrypt(
        hKey: HCRYPTKEY,
        hHash: HCRYPTHASH,
        Final: BOOL,
        dwFlags: DWORD,
        pbData: *mut BYTE,
        pdwDataLen: *mut DWORD,
        dwBufLen: DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptDecrypt
    pub unsafe fn crypt_decrypt(
        hKey: HCRYPTKEY,
        hHash: HCRYPTHASH,
        Final: BOOL,
        dwFlags: DWORD,
        pbData: *mut BYTE,
        pdwDataLen: *mut DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptImportKey
    pub unsafe fn crypt_import_key(
        hProv: HCRYPTPROV,
        pbData: *const BYTE,
        dwDataLen: DWORD,
        hPubKey: HCRYPTKEY,
        dwFlags: DWORD,
        phKey: *mut HCRYPTKEY,
    ) -> BOOL {
        if !phKey.is_null() {
            *phKey = 1 as HCRYPTKEY;
        }
        TRUE
    }

    /// CryptExportKey
    pub unsafe fn crypt_export_key(
        hKey: HCRYPTKEY,
        hExpKey: HCRYPTKEY,
        dwBlobType: DWORD,
        dwFlags: DWORD,
        pbData: *mut BYTE,
        pdwDataLen: *mut DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptSignHashA
    pub unsafe fn crypt_sign_hash_a(
        hHash: HCRYPTHASH,
        dwKeySpec: DWORD,
        sDescription: LPCSTR,
        dwFlags: DWORD,
        pbSignature: *mut BYTE,
        pdwSigLen: *mut DWORD,
    ) -> BOOL {
        TRUE
    }

    /// CryptVerifySignatureA
    pub unsafe fn crypt_verify_signature_a(
        hHash: HCRYPTHASH,
        pbSignature: *const BYTE,
        dwSigLen: DWORD,
        hPubKey: HCRYPTKEY,
        sDescription: LPCSTR,
        dwFlags: DWORD,
    ) -> BOOL {
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
    pub unsafe fn open_process_token(
        hProcess: HANDLE,
        dwDesiredAccess: DWORD,
        phToken: *mut HANDLE,
    ) -> BOOL {
        if !phToken.is_null() {
            *phToken = 1 as HANDLE;
        }
        TRUE
    }

    /// OpenThreadToken
    pub unsafe fn open_thread_token(
        hThread: HANDLE,
        dwDesiredAccess: DWORD,
        bOpenAsSelf: BOOL,
        phToken: *mut HANDLE,
    ) -> BOOL {
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
    pub unsafe fn lookup_privilege_value_a(
        lpSystemName: LPCSTR,
        lpName: LPCSTR,
        lpLuid: *mut u64,
    ) -> BOOL {
        TRUE
    }

    /// LookupPrivilegeDisplayNameA
    pub unsafe fn lookup_privilege_display_name_a(
        lpSystemName: LPCSTR,
        lpName: LPCSTR,
        lpDisplayName: LPSTR,
        cchDisplayName: *mut DWORD,
        lpLanguageId: *mut DWORD,
    ) -> BOOL {
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
    pub unsafe fn shell_about_a(
        hWnd: HWND,
        szApp: LPCSTR,
        szOtherStuff: LPCSTR,
        hIcon: HICON,
    ) -> BOOL {
        TRUE
    }

    /// ExtractIconA
    pub unsafe fn extract_icon_a(
        hInst: HINSTANCE,
        lpszExeFileName: LPCSTR,
        nIconIndex: UINT,
    ) -> HICON {
        1 as HICON
    }

    /// ExtractIconExA
    pub unsafe fn extract_icon_ex_a(
        lpszFile: LPCSTR,
        nIconIndex: INT,
        phiconLarge: *mut HICON,
        phiconSmall: *mut HICON,
        nIcons: UINT,
    ) -> UINT {
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
    pub unsafe fn sh_get_special_folder_path_a(
        hwnd: HWND,
        pszPath: LPSTR,
        csidl: INT,
        fCreate: BOOL,
    ) -> BOOL {
        FALSE
    }

    /// SHGetFolderPathA
    pub unsafe fn sh_get_folder_path_a(
        hwnd: HWND,
        csidl: INT,
        hToken: HANDLE,
        dwFlags: DWORD,
        pszPath: LPSTR,
    ) -> HRESULT {
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
    pub unsafe fn sh_empty_recycle_bin_a(
        hwnd: HWND,
        pszRootPath: LPCSTR,
        dwFlags: DWORD,
    ) -> HRESULT {
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
        crate::win32::win32_alloc(size as usize, 16) as LPVOID
    }

    /// free
    pub unsafe fn free(ptr: LPVOID) {
        crate::win32::win32_dealloc(ptr as *mut u8);
    }

    /// calloc  (memory is zero-initialised by win32_alloc)
    pub unsafe fn calloc(num: SIZE_T, size: SIZE_T) -> LPVOID {
        crate::win32::win32_alloc((num as usize).saturating_mul(size as usize), 16) as LPVOID
    }

    /// realloc
    pub unsafe fn realloc(ptr: LPVOID, size: SIZE_T) -> LPVOID {
        crate::win32::win32_realloc(ptr as *mut u8, size as usize) as LPVOID
    }

    /// _msize - Get size of allocated memory block
    pub unsafe fn _msize(ptr: LPVOID) -> SIZE_T {
        if ptr.is_null() {
            return 0;
        }
        // Lookup in allocation map
        match crate::win32::ALLOC_MAP.lock().get(&(ptr as u64)) {
            Some(&(size, _align)) => size as SIZE_T,
            None => 0,
        }
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
        if n < 0 {
            -n
        } else {
            n
        }
    }

    /// labs
    pub unsafe fn labs(n: LONG) -> LONG {
        if n < 0 {
            -n
        } else {
            n
        }
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

    /// qsort - QuickSort implementation for C compatibility
    pub unsafe fn qsort(
        base: LPVOID,
        num: SIZE_T,
        size: SIZE_T,
        compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>,
    ) {
        let compar = match compar {
            Some(f) => f,
            None => return,
        };
        let num = num as usize;
        let size = size as usize;
        if num <= 1 || size == 0 || base.is_null() {
            return;
        }

        // Partition function
        unsafe fn partition(
            base: *mut u8,
            low: usize,
            high: usize,
            size: usize,
            compar: unsafe extern "C" fn(*const u8, *const u8) -> INT,
        ) -> usize {
            let pivot = base.add(high * size);
            let mut i = low;

            for j in low..high {
                let elem_j = base.add(j * size);
                if compar(elem_j, pivot) <= 0 {
                    // Swap elements at i and j
                    if i != j {
                        for k in 0..size {
                            let elem_i = base.add(i * size + k);
                            let elem_j = base.add(j * size + k);
                            let tmp = *elem_i;
                            *elem_i = *elem_j;
                            *elem_j = tmp;
                        }
                    }
                    i += 1;
                }
            }
            // Swap pivot to its final position
            if i != high {
                for k in 0..size {
                    let elem_i = base.add(i * size + k);
                    let elem_pivot = base.add(high * size + k);
                    let tmp = *elem_i;
                    *elem_i = *elem_pivot;
                    *elem_pivot = tmp;
                }
            }
            i
        }

        // Iterative quicksort using stack
        let mut stack = alloc::vec::Vec::with_capacity(64);
        stack.push((0usize, num - 1));

        while let Some((low, high)) = stack.pop() {
            if low < high {
                let p = partition(base as *mut u8, low, high, size, compar);
                if p > 0 {
                    stack.push((low, p - 1));
                }
                stack.push((p + 1, high));
            }
        }
    }

    /// bsearch - Binary search implementation for C compatibility
    pub unsafe fn bsearch(
        key: LPCVOID,
        base: LPCVOID,
        num: SIZE_T,
        size: SIZE_T,
        compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>,
    ) -> LPVOID {
        let compar = match compar {
            Some(f) => f,
            None => return core::ptr::null_mut(),
        };
        let num = num as usize;
        let size = size as usize;
        if num == 0 || size == 0 || base.is_null() || key.is_null() {
            return core::ptr::null_mut();
        }

        let mut low = 0usize;
        let mut high = num;

        while low < high {
            let mid = low + (high - low) / 2;
            let elem = (base as *const u8).add(mid * size) as LPCVOID;
            let cmp = compar(key, elem);
            if cmp < 0 {
                high = mid;
            } else if cmp > 0 {
                low = mid + 1;
            } else {
                return elem as LPVOID;
            }
        }
        core::ptr::null_mut()
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
    pub unsafe fn poly_draw(
        hdc: HDC,
        lppt: *const POINT,
        lpbTypes: *const BYTE,
        cCount: INT,
    ) -> BOOL {
        TRUE
    }

    /// Arc
    pub unsafe fn arc(
        hdc: HDC,
        x1: INT,
        y1: INT,
        x2: INT,
        y2: INT,
        x3: INT,
        y3: INT,
        x4: INT,
        y4: INT,
    ) -> BOOL {
        TRUE
    }

    /// ArcTo
    pub unsafe fn arc_to(
        hdc: HDC,
        left: INT,
        top: INT,
        right: INT,
        bottom: INT,
        xr1: INT,
        yr1: INT,
        xr2: INT,
        yr2: INT,
    ) -> BOOL {
        TRUE
    }

    /// Chord
    pub unsafe fn chord(
        hdc: HDC,
        x1: INT,
        y1: INT,
        x2: INT,
        y2: INT,
        x3: INT,
        y3: INT,
        x4: INT,
        y4: INT,
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
        x1: INT,
        y1: INT,
        x2: INT,
        y2: INT,
        x3: INT,
        y3: INT,
        x4: INT,
        y4: INT,
    ) -> BOOL {
        TRUE
    }

    /// RoundRect
    pub unsafe fn round_rect(
        hdc: HDC,
        left: INT,
        top: INT,
        right: INT,
        bottom: INT,
        width: INT,
        height: INT,
    ) -> BOOL {
        TRUE
    }

    /// Polygon
    pub unsafe fn polygon(hdc: HDC, lpPoints: *const POINT, nCount: INT) -> BOOL {
        TRUE
    }

    /// PolyPolygon
    pub unsafe fn poly_polygon(
        hdc: HDC,
        lpPoints: *const POINT,
        lpPolyCounts: *const INT,
        nCount: INT,
    ) -> BOOL {
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
    pub unsafe fn angle_arc(
        hdc: HDC,
        x: INT,
        y: INT,
        r: DWORD,
        StartAngle: f32,
        SweepAngle: f32,
    ) -> BOOL {
        TRUE
    }

    // ========================================================================
    // FILLED SHAPES
    // ========================================================================

    /// FillRect — Dikdörtgeni belirtilen fırça rengiyle doldurur
    pub unsafe fn fill_rect(hDC: HDC, lprc: *const RECT, hbr: HBRUSH) -> INT {
        if lprc.is_null() {
            return 0;
        }
        let rect = &*lprc;
        let hdc = hDC as u64;

        // DC'den pencere HWND'sini al
        let hwnd = {
            let dcs = crate::win32::WIN32_DCS.lock();
            dcs.get(&hdc).map(|dc| dc.hwnd).unwrap_or(0)
        };

        if hwnd == 0 {
            return 0;
        }

        // Fırça rengini al (varsayılan: beyaz)
        let color = if hbr as u64 > 0x10 {
            // Kullanıcı tanımlı fırça - şimdilik varsayılan
            0xFFFFFFu32
        } else {
            // Sistem rengi (COLOR_WINDOW=5, COLOR_BTNFACE=15, vs.)
            match hbr as u64 {
                0 => 0x000000,  // COLOR_SCROLLBAR
                1 => 0xC0C0C0,  // COLOR_BACKGROUND
                5 => 0xFFFFFF,  // COLOR_WINDOW
                15 => 0xC0C0C0, // COLOR_BTNFACE
                _ => 0xFFFFFF,
            }
        };

        // Pencere surface'ına dikdörtgen çiz
        let mut windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get_mut(&hwnd) {
            let x0 = rect.left.max(0) as usize;
            let y0 = rect.top.max(0) as usize;
            let x1 = (rect.right as usize).min(win.width as usize);
            let y1 = (rect.bottom as usize).min(win.height as usize);

            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;

            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y * win.width as usize + x) * 4;
                    if idx + 3 < win.surface.len() {
                        win.surface[idx] = b;
                        win.surface[idx + 1] = g;
                        win.surface[idx + 2] = r;
                        win.surface[idx + 3] = 0xFF;
                    }
                }
            }

            crate::serial_println!(
                "[WIN32] FillRect: {},{} {},{} color={:#06x}",
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                color
            );
        }
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
    pub unsafe fn ext_flood_fill(
        hdc: HDC,
        x: INT,
        y: INT,
        crColor: DWORD,
        fuFillType: UINT,
    ) -> BOOL {
        TRUE
    }

    /// FloodFill
    pub unsafe fn flood_fill(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> BOOL {
        TRUE
    }

    /// GradientFill
    pub unsafe fn gradient_fill(
        hdc: HDC,
        pVertex: *const u8,
        nVertex: ULONG,
        pMesh: *const u8,
        nMesh: ULONG,
        ulMode: ULONG,
    ) -> BOOL {
        TRUE
    }

    // ========================================================================
    // BITMAPS
    // ========================================================================

    /// CreateBitmap
    pub unsafe fn create_bitmap(
        nWidth: INT,
        nHeight: INT,
        nPlanes: UINT,
        nBitCount: UINT,
        lpBits: *const u8,
    ) -> HBITMAP {
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
    pub unsafe fn set_bitmap_dimension_ex(
        hBitmap: HBITMAP,
        nX: INT,
        nY: INT,
        lpSize: *mut SIZE,
    ) -> BOOL {
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
        xDest: INT,
        yDest: INT,
        w: DWORD,
        h: DWORD,
        xSrc: INT,
        ySrc: INT,
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
        xDest: INT,
        yDest: INT,
        wDest: INT,
        hDest: INT,
        xSrc: INT,
        ySrc: INT,
        wSrc: INT,
        hSrc: INT,
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
    pub unsafe fn get_dib_color_table(
        hdc: HDC,
        uStartIndex: UINT,
        cEntries: UINT,
        pColors: *mut u8,
    ) -> UINT {
        0
    }

    /// SetDIBColorTable
    pub unsafe fn set_dib_color_table(
        hdc: HDC,
        uStartIndex: UINT,
        cEntries: UINT,
        pColors: *const u8,
    ) -> UINT {
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
    pub unsafe fn ext_create_pen(
        dwPenStyle: DWORD,
        dwWidth: DWORD,
        lplb: *const u8,
        dwStyleCount: DWORD,
        lpStyle: *const DWORD,
    ) -> HPEN {
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
    pub unsafe fn get_text_extent_point_a(
        hdc: HDC,
        lpString: LPCSTR,
        cbString: INT,
        lpSize: *mut SIZE,
    ) -> BOOL {
        if !lpSize.is_null() {
            (*lpSize).cx = cbString * 8;
            (*lpSize).cy = 16;
        }
        TRUE
    }

    /// GetTextExtentPoint32A
    pub unsafe fn get_text_extent_point_32_a(
        hdc: HDC,
        lpString: LPCSTR,
        c: INT,
        psizl: *mut SIZE,
    ) -> BOOL {
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
    pub unsafe fn get_char_width_a(
        hdc: HDC,
        iFirst: UINT,
        iLast: UINT,
        lpBuffer: *mut INT,
    ) -> BOOL {
        TRUE
    }

    /// GetCharWidth32A
    pub unsafe fn get_char_width_32_a(
        hdc: HDC,
        iFirst: UINT,
        iLast: UINT,
        lpBuffer: *mut INT,
    ) -> BOOL {
        TRUE
    }

    /// GetCharABCWidthsA
    pub unsafe fn get_char_abc_widths_a(
        hdc: HDC,
        uFirst: UINT,
        uLast: UINT,
        lpabc: *mut u8,
    ) -> BOOL {
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

    /// TextOutA — Metni pencereye çizer (8x16 bitmap font)
    pub unsafe fn text_out_a(hdc: HDC, x: INT, y: INT, lpString: LPCSTR, c: INT) -> BOOL {
        if lpString.is_null() {
            return FALSE;
        }

        let hdc_id = hdc as u64;

        // DC'den pencere ve metin rengini al
        let (hwnd, text_color) = {
            let dcs = crate::win32::WIN32_DCS.lock();
            match dcs.get(&hdc_id) {
                Some(dc) => (dc.hwnd, dc.text_color),
                None => (0, 0x000000),
            }
        };

        if hwnd == 0 {
            return FALSE;
        }

        // Metin dizesini parse et
        let mut text = String::new();
        let mut ptr = lpString;
        for _ in 0..c {
            if ptr.is_null() || *ptr == 0 {
                break;
            }
            text.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }

        // Pencere surface'ına çiz
        let mut windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get_mut(&hwnd) {
            let r = ((text_color >> 16) & 0xFF) as u8;
            let g = ((text_color >> 8) & 0xFF) as u8;
            let b = (text_color & 0xFF) as u8;

            let char_w = 8;
            let char_h = 16;
            let mut cx = x as usize;

            for ch in text.chars() {
// 8x16 bitmap font - dar ASCII glif tablosu
                let glyph = get_ascii_glyph(ch);

                for row in 0..char_h {
                    let py = y as usize + row;
                    if py >= win.height as usize {
                        continue;
                    }

                    let bits = glyph[row];
                    for col in 0..char_w {
                        if bits & (1 << (7 - col)) != 0 {
                            let px = cx + col;
                            if px >= win.width as usize {
                                continue;
                            }

                            let idx = (py * win.width as usize + px) * 4;
                            if idx + 3 < win.surface.len() {
                                win.surface[idx] = b;
                                win.surface[idx + 1] = g;
                                win.surface[idx + 2] = r;
                                win.surface[idx + 3] = 0xFF;
                            }
                        }
                    }
                }
                cx += char_w;
            }

            crate::serial_println!("[WIN32] TextOutA: {},{} \"{}\"", x, y, text);
        }
        TRUE
    }

/// Dar 8x16 ASCII bitmap font glyph tablosu
    fn get_ascii_glyph(ch: char) -> [u8; 16] {
        // Sadece temel karakterler için basitleştirilmiş glif
        match ch {
            ' ' => [0x00; 16],
            'A' | 'a' => [
                0x00, 0x00, 0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'B' | 'b' => [
                0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'C' | 'c' => [
                0x00, 0x00, 0x3C, 0x66, 0x60, 0x60, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'D' | 'd' => [
                0x00, 0x00, 0x78, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'E' | 'e' => [
                0x00, 0x00, 0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'F' | 'f' => [
                0x00, 0x00, 0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'G' | 'g' => [
                0x00, 0x00, 0x3C, 0x66, 0x60, 0x60, 0x6E, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'H' | 'h' => [
                0x00, 0x00, 0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'I' | 'i' => [
                0x00, 0x00, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'J' | 'j' => [
                0x00, 0x00, 0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'K' | 'k' => [
                0x00, 0x00, 0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'L' | 'l' => [
                0x00, 0x00, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'M' | 'm' => [
                0x00, 0x00, 0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'N' | 'n' => [
                0x00, 0x00, 0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'O' | 'o' => [
                0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'P' | 'p' => [
                0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'Q' | 'q' => [
                0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x76, 0x6C, 0x36, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'R' | 'r' => [
                0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'S' | 's' => [
                0x00, 0x00, 0x3C, 0x66, 0x60, 0x3C, 0x06, 0x06, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'T' | 't' => [
                0x00, 0x00, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'U' | 'u' => [
                0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'V' | 'v' => [
                0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'W' | 'w' => [
                0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0xC6, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'X' | 'x' => [
                0x00, 0x00, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x3C, 0x66, 0x66, 0x66, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'Y' | 'y' => [
                0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            'Z' | 'z' => [
                0x00, 0x00, 0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x60, 0x60, 0x7E, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '0' => [
                0x00, 0x00, 0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '1' => [
                0x00, 0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '2' => [
                0x00, 0x00, 0x3C, 0x66, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x60, 0x7E, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '3' => [
                0x00, 0x00, 0x3C, 0x66, 0x06, 0x1C, 0x06, 0x06, 0x06, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '4' => [
                0x00, 0x00, 0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x0C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '5' => [
                0x00, 0x00, 0x7E, 0x60, 0x60, 0x7C, 0x06, 0x06, 0x06, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '6' => [
                0x00, 0x00, 0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '7' => [
                0x00, 0x00, 0x7E, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '8' => [
                0x00, 0x00, 0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '9' => [
                0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x0C, 0x38, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '.' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            ',' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x30, 0x00, 0x00,
                0x00, 0x00,
            ],
            ':' => [
                0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            ';' => [
                0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '!' => [
                0x00, 0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '?' => [
                0x00, 0x00, 0x3C, 0x66, 0x06, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '-' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '+' => [
                0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '=' => [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '/' => [
                0x00, 0x00, 0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '\\' => [
                0x00, 0x00, 0x80, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '(' => [
                0x00, 0x00, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            ')' => [
                0x00, 0x00, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            '[' => [
                0x00, 0x00, 0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            ']' => [
                0x00, 0x00, 0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            _ => [
                0x00, 0x00, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ], // Bilinmeyen karakter
        }
    }

    /// ExtTextOutA
    pub unsafe fn ext_text_out_a(
        hdc: HDC,
        x: INT,
        y: INT,
        fuOptions: UINT,
        lprc: *const RECT,
        lpString: LPCSTR,
        cbCount: UINT,
        lpDx: *const INT,
    ) -> BOOL {
        TRUE
    }

    /// DrawTextA
    pub unsafe fn draw_text_a(
        hdc: HDC,
        lpchText: LPCSTR,
        cchText: INT,
        lprc: *mut RECT,
        uFormat: UINT,
    ) -> INT {
        let mut text = String::new();
        let mut ptr = lpchText;
        for _ in 0..cchText {
            if ptr.is_null() || *ptr == 0 {
                break;
            }
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
        x: INT,
        y: INT,
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
    pub unsafe fn create_round_rect_rgn(
        left: INT,
        top: INT,
        right: INT,
        bottom: INT,
        nWidthEllipse: INT,
        nHeightEllipse: INT,
    ) -> HRGN {
        1 as HRGN
    }

    /// CreatePolygonRgn
    pub unsafe fn create_polygon_rgn(lppt: *const POINT, cPoints: INT, fnMode: INT) -> HRGN {
        1 as HRGN
    }

    /// CreatePolyPolygonRgn
    pub unsafe fn create_poly_polygon_rgn(
        lppt: *const POINT,
        lpPolyCounts: *const INT,
        nCount: INT,
        fnPolyFillMode: INT,
    ) -> HRGN {
        1 as HRGN
    }

    /// CombineRgn
    pub unsafe fn combine_rgn(
        hrgnDest: HRGN,
        hrgnSrc1: HRGN,
        hrgnSrc2: HRGN,
        fnCombineMode: INT,
    ) -> INT {
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
    pub unsafe fn intersect_clip_rect(
        hdc: HDC,
        left: INT,
        top: INT,
        right: INT,
        bottom: INT,
    ) -> INT {
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
    pub unsafe fn combine_transform(
        lpxfResult: *mut u8,
        lpxf1: *const u8,
        lpxf2: *const u8,
    ) -> BOOL {
        TRUE
    }

    // ========================================================================
    // COLORS
    // ========================================================================

    /// SetPixel — Belirtilen konumdaki pikseli ayarlar
    pub unsafe fn set_pixel(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> DWORD {
        let hdc_id = hdc as u64;

        // DC'den pencere HWND'sini al
        let hwnd = {
            let dcs = crate::win32::WIN32_DCS.lock();
            dcs.get(&hdc_id).map(|dc| dc.hwnd).unwrap_or(0)
        };

        if hwnd == 0 {
            return 0xFFFFFFFF;
        } // CLR_INVALID

        let mut windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get_mut(&hwnd) {
            if x >= 0 && y >= 0 && x < win.width && y < win.height {
                let idx = ((y * win.width + x) * 4) as usize;
                if idx + 3 < win.surface.len() {
                    let r = ((crColor >> 16) & 0xFF) as u8;
                    let g = ((crColor >> 8) & 0xFF) as u8;
                    let b = (crColor & 0xFF) as u8;
                    win.surface[idx] = b;
                    win.surface[idx + 1] = g;
                    win.surface[idx + 2] = r;
                    win.surface[idx + 3] = 0xFF;
                    return crColor;
                }
            }
        }
        0xFFFFFFFF // CLR_INVALID
    }

    /// SetPixelV — Belirtilen konumdaki pikseli ayarlar (dönüş değeri farklı)
    pub unsafe fn set_pixel_v(hdc: HDC, x: INT, y: INT, crColor: DWORD) -> BOOL {
        if set_pixel(hdc, x, y, crColor) != 0xFFFFFFFF {
            TRUE
        } else {
            FALSE
        }
    }

    /// GetPixel — Belirtilen konumdaki pikselin rengini alır
    pub unsafe fn get_pixel(hdc: HDC, x: INT, y: INT) -> DWORD {
        let hdc_id = hdc as u64;

        let hwnd = {
            let dcs = crate::win32::WIN32_DCS.lock();
            dcs.get(&hdc_id).map(|dc| dc.hwnd).unwrap_or(0)
        };

        if hwnd == 0 {
            return 0xFFFFFFFF;
        }

        let windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get(&hwnd) {
            if x >= 0 && y >= 0 && x < win.width && y < win.height {
                let idx = ((y * win.width + x) * 4) as usize;
                if idx + 3 < win.surface.len() {
                    let b = win.surface[idx] as u32;
                    let g = win.surface[idx + 1] as u32;
                    let r = win.surface[idx + 2] as u32;
                    return (r << 16) | (g << 8) | b;
                }
            }
        }
        0xFFFFFFFF
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
    pub unsafe fn animate_palette(
        hpal: HPALETTE,
        iStartIndex: UINT,
        cEntries: UINT,
        ppe: *const u8,
    ) -> BOOL {
        TRUE
    }

    /// SetPaletteEntries
    pub unsafe fn set_palette_entries(
        hpal: HPALETTE,
        iStart: UINT,
        cEntries: UINT,
        ppe: *const u8,
    ) -> UINT {
        0
    }

    /// GetPaletteEntries
    pub unsafe fn get_palette_entries(
        hpal: HPALETTE,
        iStart: UINT,
        cEntries: UINT,
        ppe: *mut u8,
    ) -> UINT {
        0
    }

    /// GetSystemPaletteEntries
    pub unsafe fn get_system_palette_entries(
        hdc: HDC,
        iStart: UINT,
        cEntries: UINT,
        ppe: *mut u8,
    ) -> UINT {
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

    // ========================================================================
    // BIT BLOCK TRANSFER (BLIT) OPERATIONS
    // ========================================================================

    /// BitBlt — Bir DC'den diğerine piksel bloğu kopyalar
    /// ROP kodları: SRCCOPY=0xCC0020, SRCPAINT=0xEE0086, SRCAND=0x8800C6, etc.
    pub unsafe fn bit_blt(
        hdcDest: HDC,
        nXDest: INT,
        nYDest: INT,
        nWidth: INT,
        nHeight: INT,
        hdcSrc: HDC,
        nXSrc: INT,
        nYSrc: INT,
        dwRop: DWORD,
    ) -> BOOL {
        // Kaynak ve hedef pencereleri al
        let (src_hwnd, dst_hwnd) = {
            let dcs = crate::win32::WIN32_DCS.lock();
            let src = dcs.get(&(hdcSrc as u64)).map(|dc| dc.hwnd).unwrap_or(0);
            let dst = dcs.get(&(hdcDest as u64)).map(|dc| dc.hwnd).unwrap_or(0);
            (src, dst)
        };

        if src_hwnd == 0 || dst_hwnd == 0 {
            return FALSE;
        }
        if src_hwnd == dst_hwnd {
            // Aynı pencere içinde kopyalama
            return self_blit(
                dst_hwnd, nXDest, nYDest, nWidth, nHeight, nXSrc, nYSrc, dwRop,
            );
        }

        // Farklı pencereler arasında kopyalama
        let mut windows = crate::win32::WIN32_WINDOWS.lock();

        // Kaynak piksellerini geçici tampona kopyala
        let src_pixels: Vec<u32> = if let Some(src_win) = windows.get(&src_hwnd) {
            let mut pixels = Vec::with_capacity((nWidth * nHeight) as usize);
            for y in 0..nHeight {
                for x in 0..nWidth {
                    let sx = (nXSrc + x) as usize;
                    let sy = (nYSrc + y) as usize;
                    if sx < src_win.width as usize && sy < src_win.height as usize {
                        let idx = (sy * src_win.width as usize + sx) * 4;
                        if idx + 3 < src_win.surface.len() {
                            let b = src_win.surface[idx] as u32;
                            let g = src_win.surface[idx + 1] as u32;
                            let r = src_win.surface[idx + 2] as u32;
                            pixels.push((r << 16) | (g << 8) | b);
                        } else {
                            pixels.push(0);
                        }
                    } else {
                        pixels.push(0);
                    }
                }
            }
            pixels
        } else {
            return FALSE;
        };

        // Hedef pencereye kopyala
        if let Some(dst_win) = windows.get_mut(&dst_hwnd) {
            for y in 0..nHeight {
                for x in 0..nWidth {
                    let dx = (nXDest + x) as usize;
                    let dy = (nYDest + y) as usize;
                    if dx < dst_win.width as usize && dy < dst_win.height as usize {
                        let src_idx = (y * nWidth + x) as usize;
                        if src_idx < src_pixels.len() {
                            let pixel = apply_rop(src_pixels[src_idx], 0, dwRop);
                            let idx = (dy * dst_win.width as usize + dx) * 4;
                            if idx + 3 < dst_win.surface.len() {
                                dst_win.surface[idx] = (pixel & 0xFF) as u8;
                                dst_win.surface[idx + 1] = ((pixel >> 8) & 0xFF) as u8;
                                dst_win.surface[idx + 2] = ((pixel >> 16) & 0xFF) as u8;
                                dst_win.surface[idx + 3] = 0xFF;
                            }
                        }
                    }
                }
            }
        }

        crate::serial_println!(
            "[WIN32] BitBlt: {},{} {}x{} ROP={:#x}",
            nXDest,
            nYDest,
            nWidth,
            nHeight,
            dwRop
        );
        TRUE
    }

    /// Aynı pencere içinde blit (scroll, kopyalama)
    fn self_blit(
        hwnd: u64,
        dx: INT,
        dy: INT,
        w: INT,
        h: INT,
        sx: INT,
        sy: INT,
        rop: DWORD,
    ) -> BOOL {
        let mut windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get_mut(&hwnd) {
            // Geçici tampon
            let mut temp = vec![0u32; (w * h) as usize];

            // Kaynak piksellerini oku
            for y in 0..h {
                for x in 0..w {
                    let rx = (sx + x) as usize;
                    let ry = (sy + y) as usize;
                    if rx < win.width as usize && ry < win.height as usize {
                        let idx = (ry * win.width as usize + rx) * 4;
                        if idx + 3 < win.surface.len() {
                            let b = win.surface[idx] as u32;
                            let g = win.surface[idx + 1] as u32;
                            let r = win.surface[idx + 2] as u32;
                            temp[(y * w + x) as usize] = (r << 16) | (g << 8) | b;
                        }
                    }
                }
            }

            // Hedef konuma yaz
            for y in 0..h {
                for x in 0..w {
                    let wx = (dx + x) as usize;
                    let wy = (dy + y) as usize;
                    if wx < win.width as usize && wy < win.height as usize {
                        let pixel = apply_rop(temp[(y * w + x) as usize], 0, rop);
                        let idx = (wy * win.width as usize + wx) * 4;
                        if idx + 3 < win.surface.len() {
                            win.surface[idx] = (pixel & 0xFF) as u8;
                            win.surface[idx + 1] = ((pixel >> 8) & 0xFF) as u8;
                            win.surface[idx + 2] = ((pixel >> 16) & 0xFF) as u8;
                            win.surface[idx + 3] = 0xFF;
                        }
                    }
                }
            }
            return TRUE;
        }
        FALSE
    }

    /// ROP (Raster Operation) uygula
    fn apply_rop(src: u32, dst: u32, rop: DWORD) -> u32 {
        match rop {
            0x00CC0020 => src,          // SRCCOPY
            0x00EE0086 => src | dst,    // SRCPAINT
            0x008800C6 => src & dst,    // SRCAND
            0x00660046 => src ^ dst,    // SRCINVERT
            0x00440328 => src & !dst,   // SRCERASE
            0x00330008 => !src,         // NOTSRCCOPY
            0x001100A6 => !(src | dst), // NOTSRCERASE
            0x00C000CA => src & dst,    // MERGECOPY
            0x00BB0226 => !src | dst,   // MERGEPAINT
            0x00F00021 => dst,          // PATCOPY
            0x00A00089 => dst,          // PATPAINT
            0x005A0049 => src ^ dst,    // PATINVERT
            0x00550009 => !dst,         // DSTINVERT
            0x00000042 => 0x000000,     // BLACKNESS
            0x00FF0062 => 0xFFFFFF,     // WHITENESS
            _ => src,                   // Varsayılan: SRCCOPY
        }
    }

    /// PatBlt — Desen blit (fırça ile doldurma)
    pub unsafe fn pat_blt(
        hdc: HDC,
        nXLeft: INT,
        nYLeft: INT,
        nWidth: INT,
        nHeight: INT,
        dwRop: DWORD,
    ) -> BOOL {
        let hdc_id = hdc as u64;

        let (hwnd, brush_color) = {
            let dcs = crate::win32::WIN32_DCS.lock();
            match dcs.get(&hdc_id) {
                Some(dc) => (dc.hwnd, dc.brush_color),
                None => (0, 0xFFFFFF),
            }
        };

        if hwnd == 0 {
            return FALSE;
        }

        let mut windows = crate::win32::WIN32_WINDOWS.lock();
        if let Some(win) = windows.get_mut(&hwnd) {
            let r = ((brush_color >> 16) & 0xFF) as u8;
            let g = ((brush_color >> 8) & 0xFF) as u8;
            let b = (brush_color & 0xFF) as u8;

            for y in 0..nHeight {
                for x in 0..nWidth {
                    let px = (nXLeft + x) as usize;
                    let py = (nYLeft + y) as usize;
                    if px < win.width as usize && py < win.height as usize {
                        let idx = (py * win.width as usize + px) * 4;
                        if idx + 3 < win.surface.len() {
                            let existing = (win.surface[idx + 2] as u32) << 16
                                | (win.surface[idx + 1] as u32) << 8
                                | (win.surface[idx] as u32);
                            let pixel = apply_rop(brush_color, existing, dwRop);
                            win.surface[idx] = (pixel & 0xFF) as u8;
                            win.surface[idx + 1] = ((pixel >> 8) & 0xFF) as u8;
                            win.surface[idx + 2] = ((pixel >> 16) & 0xFF) as u8;
                            win.surface[idx + 3] = 0xFF;
                        }
                    }
                }
            }
        }
        TRUE
    }

    /// StretchBlt — Boyutlandırmalı bitmap kopyalama
    pub unsafe fn stretch_blt(
        hdcDest: HDC,
        nXOriginDest: INT,
        nYOriginDest: INT,
        nWidthDest: INT,
        nHeightDest: INT,
        hdcSrc: HDC,
        nXOriginSrc: INT,
        nYOriginSrc: INT,
        nWidthSrc: INT,
        nHeightSrc: INT,
        dwRop: DWORD,
    ) -> BOOL {
        // Basitleştirilmiş implementasyon: BitBlt gibi davran (boyutlandırma yok)
        // Gerçek boyutlandırma için bilinear/nearest-neighbor interpolasyon gerekir
        bit_blt(
            hdcDest,
            nXOriginDest,
            nYOriginDest,
            nWidthDest,
            nHeightDest,
            hdcSrc,
            nXOriginSrc,
            nYOriginSrc,
            dwRop,
        )
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
    kernel32_funcs.insert("WaitOnAddress".to_string(), stub_api);
    kernel32_funcs.insert("WakeByAddressSingle".to_string(), stub_api);
    kernel32_funcs.insert("WakeByAddressAll".to_string(), stub_api);
    kernel32_funcs.insert("CreateIoRing".to_string(), stub_api);
    kernel32_funcs.insert("BuildIoRingRegisterFileHandles".to_string(), stub_api);
    kernel32_funcs.insert("BuildIoRingRegisterBuffers".to_string(), stub_api);
    kernel32_funcs.insert("BuildIoRingReadFile".to_string(), stub_api);
    kernel32_funcs.insert("BuildIoRingWriteFile".to_string(), stub_api);
    kernel32_funcs.insert("SubmitIoRing".to_string(), stub_api);
    kernel32_funcs.insert("PopIoRingCompletion".to_string(), stub_api);
    kernel32_funcs.insert("CloseIoRing".to_string(), stub_api);
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

/// Unsupported API function — returned by get_fn_address for unknown imports.
pub fn stub_api(_args: *const u8) -> isize {
    crate::serial_println!("[WIN32] Unsupported API called");
    0
}

// ============================================================================
// GLOBAL API TABLE
// ============================================================================

static WIN32_API_TABLE: Mutex<Option<BTreeMap<String, BTreeMap<String, Win32ApiFn>>>> =
    Mutex::new(None);

/// Initialize Win32 subsystem
pub fn init() {
    // Win32 uyumluluk katmanı, ekosistem politika geçidini açmadan başlatılmaz.
    crate::ecosystem::bootstrap();
    init_dll_handles();
    let mut table = WIN32_API_TABLE.lock();
    *table = Some(init_api_table());
    crate::serial_println!("[WIN32] API emulation initialized");
}

/// Get proc address
pub fn get_proc_address(module: &str, func: &str) -> Option<u64> {
    let addr = get_fn_address(module, func);
    if addr != stub_api as usize as u64 {
        return Some(addr);
    }

    let module_key = module.to_lowercase();
    let module_key = module_key.trim_end_matches(".dll");
    let table = WIN32_API_TABLE.lock();
    if let Some(ref t) = *table {
        if let Some(module_funcs) = t.get(module_key) {
            if module_funcs.contains_key(func) {
                return Some(stub_api as usize as u64);
            }
        }
    }
    None
}

/// Get proc address internal
pub fn get_proc_address_internal(module: &str, func: &str) -> FARPROC {
    let addr = get_proc_address(module, func)?;
    let fn_ptr: unsafe extern "system" fn() -> isize = unsafe { core::mem::transmute(addr) };
    Some(fn_ptr)
}
