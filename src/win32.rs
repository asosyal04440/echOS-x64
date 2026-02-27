//! # echOS Win32 API Emülasyonu
//!
//! Windows ikili dosyalarını çalıştırmak için Win32 API uyumluluk katmanı.
//! Yaygın Win32 API'leri taklit eder: kernel32, user32, gdi32

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// WIN32 VERİ TİPLERİ (Windows API tür takma adları)
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
// WIN32 SABİTLERİ (Windows API sabit değerler)
// ============================================================================

pub const INVALID_HANDLE_VALUE: HANDLE = !0;
pub const NULL: HANDLE = 0;
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// Bellek sabitleri: sayfa koruma bayrakları (VirtualAlloc/VirtualProtect ile kullanılır)
pub const PAGE_NOACCESS: DWORD = 0x01;
pub const PAGE_READONLY: DWORD = 0x02;
pub const PAGE_READWRITE: DWORD = 0x04;
pub const PAGE_EXECUTE: DWORD = 0x10;
pub const PAGE_EXECUTE_READ: DWORD = 0x20;
pub const PAGE_EXECUTE_READWRITE: DWORD = 0x40;

// Dosya sabitleri: erişim türü ve oluşturma modu bayrakları (CreateFileA ile kullanılır)
pub const GENERIC_READ: DWORD = 0x80000000;
pub const GENERIC_WRITE: DWORD = 0x40000000;
pub const FILE_SHARE_READ: DWORD = 0x00000001;
pub const FILE_SHARE_WRITE: DWORD = 0x00000002;
pub const OPEN_EXISTING: DWORD = 3;
pub const CREATE_NEW: DWORD = 1;
pub const CREATE_ALWAYS: DWORD = 2;
pub const FILE_ATTRIBUTE_NORMAL: DWORD = 0x80;

// Pencere stilleri: WS_ ön ekli bayraklar pencere görünümünü belirler
pub const WS_OVERLAPPED: DWORD = 0x00000000;
pub const WS_CAPTION: DWORD = 0x00C00000;
pub const WS_SYSMENU: DWORD = 0x00080000;
pub const WS_THICKFRAME: DWORD = 0x00040000;
pub const WS_MINIMIZEBOX: DWORD = 0x00020000;
pub const WS_MAXIMIZEBOX: DWORD = 0x00010000;
pub const WS_OVERLAPPEDWINDOW: DWORD = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
pub const WS_VISIBLE: DWORD = 0x10000000;
pub const WS_CHILD: DWORD = 0x40000000;

// Pencere görünüm komutları: SW_ ön ekli sabitler ShowWindow fonksiyonuna geçirilir
pub const SW_HIDE: INT = 0;
pub const SW_SHOWNORMAL: INT = 1;
pub const SW_SHOWMINIMIZED: INT = 2;
pub const SW_SHOWMAXIMIZED: INT = 3;
pub const SW_SHOW: INT = 5;

// Windows mesaj sabitleri: WM_ ön ekli kodlar pencere olaylarını tanımlar
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

// Sanal tuş kodları: VK_ ön ekli sabitler fiziksel klavye tuşlarını temsil eder
pub const VK_ESCAPE: INT = 0x1B;
pub const VK_RETURN: INT = 0x0D;
pub const VK_SPACE: INT = 0x20;
pub const VK_LEFT: INT = 0x25;
pub const VK_UP: INT = 0x26;
pub const VK_RIGHT: INT = 0x27;
pub const VK_DOWN: INT = 0x28;

// ============================================================================
// WIN32 YAPIları (Windows API C struct'larının Rust #[repr(C)] karşılıkları)
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

// ADVAPI32 yapıları: güvenlik, servis yönetimi ve kayıt defteri işlemleri için
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

// SHELL32 yapıları: kabuk işlemleri ve dosya sistemi diyaloğu için kullanılan binary-uyumlu yapılar
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

// MSVCRT tip tanımları: C çalışma zamanı kütüphanesine ait özel türler
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
// WIN32 APİ TABLOSU (modül adı → fonksiyon adı → Rust fonksiyon işaretçisi eşlemesi)
// ============================================================================

/// Win32 API fonksiyon imzası: tüm emüle edilen Win32 fonksiyonları bu tip üzerinden çağrılır
type Win32ApiFn = fn(*const u8) -> isize;

#[derive(Clone, Debug)]
pub struct Win32CompatLaunchInfo {
    pub process_handle: HANDLE,
    pub thread_handle: HANDLE,
    pub process_id: DWORD,
    pub thread_id: DWORD,
}

#[repr(C)]
struct CompatProcessInformation {
    h_process: HANDLE,
    h_thread: HANDLE,
    process_id: DWORD,
    thread_id: DWORD,
}

fn to_cstring_i8(input: &str) -> Vec<i8> {
    let mut out = Vec::with_capacity(input.len().saturating_add(1));
    for byte in input.as_bytes() {
        out.push(*byte as i8);
    }
    out.push(0);
    out
}

pub fn launch_compat_process(image_name: &str, command_line: &str) -> Option<Win32CompatLaunchInfo> {
    let mut app = to_cstring_i8(image_name);
    let mut cmd = to_cstring_i8(command_line);
    let mut info = CompatProcessInformation {
        h_process: 0,
        h_thread: 0,
        process_id: 0,
        thread_id: 0,
    };

    let ok = unsafe {
        kernel32::create_process_a(
            app.as_mut_ptr() as LPCSTR,
            cmd.as_mut_ptr() as LPSTR,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            FALSE,
            0,
            core::ptr::null_mut(),
            core::ptr::null(),
            core::ptr::null_mut(),
            &mut info as *mut CompatProcessInformation as LPVOID,
        )
    };

    if ok == FALSE {
        return None;
    }

    let process_id = unsafe { kernel32::get_process_id(info.h_process) };
    let thread_id = unsafe { kernel32::get_thread_id(info.h_thread) };

    Some(Win32CompatLaunchInfo {
        process_handle: info.h_process,
        thread_handle: info.h_thread,
        process_id: if process_id == 0 { info.process_id } else { process_id },
        thread_id: if thread_id == 0 { info.thread_id } else { thread_id },
    })
}

/// Win32 API giriş noktası kaydı: fonksiyon adı ile işaretçisini bir arada tutar
struct Win32ApiEntry {
    name: String,
    func: Win32ApiFn,
}

/// Win32 API modül kaydı: bir DLL'in tüm dışa aktardığı fonksiyonları içerir
struct Win32Module {
    name: String,
    functions: BTreeMap<String, Win32ApiFn>,
}

// ============================================================================
// KERNEL32 UYGULAMASI (çekirdek Win32 API'lerinin echOS üzerinde emülasyonu)
// ============================================================================

mod kernel32 {
    use super::*;
    use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

    const ERROR_SUCCESS: DWORD = 0;
    const ERROR_FILE_NOT_FOUND: DWORD = 2;
    const ERROR_ACCESS_DENIED: DWORD = 5;
    const ERROR_INVALID_HANDLE: DWORD = 6;
    const ERROR_INVALID_PARAMETER: DWORD = 87;
    const ERROR_INSUFFICIENT_BUFFER: DWORD = 122;
    const ERROR_ALREADY_EXISTS: DWORD = 183;
    const INVALID_SET_FILE_POINTER: DWORD = 0xFFFF_FFFF;
    const STILL_ACTIVE: DWORD = 259;
    const WAIT_OBJECT_0: DWORD = 0;
    const WAIT_TIMEOUT: DWORD = 258;
    const WAIT_FAILED: DWORD = 0xFFFF_FFFF;
    const INFINITE: DWORD = 0xFFFF_FFFF;
    const CREATE_SUSPENDED: DWORD = 0x0000_0004;

    const FILE_BEGIN: DWORD = 0;
    const FILE_CURRENT: DWORD = 1;
    const FILE_END: DWORD = 2;

    #[derive(Clone)]
    struct Win32FileHandle {
        path: String,
        position: usize,
        readable: bool,
        writable: bool,
    }

    #[derive(Clone)]
    struct Win32ProcessHandle {
        pid: DWORD,
        name: String,
        exit_code: DWORD,
        signaled: bool,
    }

    #[derive(Clone)]
    struct Win32ThreadHandle {
        tid: DWORD,
        owner_pid: DWORD,
        exit_code: DWORD,
        suspended: bool,
        signaled: bool,
    }

    #[repr(C)]
    struct PROCESS_INFORMATION {
        hProcess: HANDLE,
        hThread: HANDLE,
        dwProcessId: DWORD,
        dwThreadId: DWORD,
    }

    static LAST_ERROR: AtomicU32 = AtomicU32::new(ERROR_SUCCESS);
    static NEXT_HANDLE: AtomicU64 = AtomicU64::new(0x100);
    static NEXT_PROCESS_ID: AtomicU32 = AtomicU32::new(10_000);
    static NEXT_THREAD_ID: AtomicU32 = AtomicU32::new(20_000);
    static FILE_STORAGE: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    static FILE_HANDLES: Mutex<BTreeMap<HANDLE, Win32FileHandle>> = Mutex::new(BTreeMap::new());
    static PROCESS_HANDLES: Mutex<BTreeMap<HANDLE, Win32ProcessHandle>> = Mutex::new(BTreeMap::new());
    static THREAD_HANDLES: Mutex<BTreeMap<HANDLE, Win32ThreadHandle>> = Mutex::new(BTreeMap::new());
    static ENV_VARS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

    fn set_last_error_internal(code: DWORD) {
        LAST_ERROR.store(code, Ordering::Relaxed);
    }

    unsafe fn cstr_to_string(ptr: LPCSTR) -> Option<String> {
        if ptr.is_null() {
            return None;
        }

        let mut out = String::new();
        let mut cursor = ptr;
        while !cursor.is_null() && *cursor != 0 {
            out.push(*cursor as u8 as char);
            cursor = cursor.add(1);
        }
        Some(out)
    }

    unsafe fn write_cstr_bytes(dst: LPSTR, dst_size: DWORD, src: &[u8]) -> DWORD {
        if dst.is_null() || dst_size == 0 {
            return src.len() as DWORD;
        }

        let cap = dst_size as usize;
        if src.len() + 1 > cap {
            *((dst as *mut u8).add(0)) = 0;
            return src.len() as DWORD;
        }

        core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len());
        *((dst as *mut u8).add(src.len())) = 0;
        src.len() as DWORD
    }

    fn handle_signaled(handle: HANDLE) -> Option<bool> {
        if FILE_HANDLES.lock().contains_key(&handle) {
            return Some(true);
        }

        if let Some(process) = PROCESS_HANDLES.lock().get(&handle) {
            return Some(process.signaled);
        }

        if let Some(thread) = THREAD_HANDLES.lock().get(&handle) {
            return Some(thread.signaled);
        }

        None
    }
    
    /// GetModuleHandleA - Yüklü modülün taban adresini döndürür; yoksa NULL
    pub unsafe fn get_module_handle_a(lpModuleName: LPCSTR) -> HMODULE {
        // Gerçek modül tablosu olmadığından PE yükleme taban adresi olarak sabit döndürüyoruz
        0x00400000
    }
    
    /// LoadLibraryA - Belirtilen DLL dosyasını belleğe yükler ve modül tanıtıcısı döndürür
    pub unsafe fn load_library_a(lpLibFileName: LPCSTR) -> HMODULE {
        // TODO: Gerçek DLL yükleme mekanizması eklenecek (PE ayrıştırıcısına bağlanacak)
        0
    }
    
    /// GetProcAddress - Yüklü modül içindeki dışa aktarılmış fonksiyonun adresini döndürür
    pub unsafe fn get_proc_address(hModule: HMODULE, lpProcName: LPCSTR) -> FARPROC {
        if lpProcName.is_null() {
            return None;
        }
        
        // Ham C string'den fonksiyon adını oku (null-terminate olana kadar ilerle)
        let mut name = String::new();
        let mut ptr = lpProcName;
        while !ptr.is_null() && *ptr != 0 {
            name.push(*ptr as u8 as char);
            ptr = ptr.add(1);
        }
        
        // Fonksiyon adını API tablosunda ara ve adresini döndür
        crate::win32::get_proc_address_internal("kernel32", &name)
    }
    
    /// VirtualAlloc - Sanal adres alanında belek tahsis eder ve isteğe bağlı commit eder
    pub unsafe fn virtual_alloc(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        flAllocationType: DWORD,
        flProtect: DWORD,
    ) -> LPVOID {
        // Bellek tahsisi (taslak uygulama - gerçek sayfa yöneticisine bağlanacak)
        let size = dwSize as usize;
        // TODO: Gerçek bellek ayırıcısı buraya bağlanacak (frame allocator kullanılacak)
        core::ptr::null_mut()
    }
    
    /// VirtualFree - VirtualAlloc ile tahsis edilen sanal belleği serbest bırakır
    pub unsafe fn virtual_free(
        lpAddress: LPVOID,
        dwSize: SIZE_T,
        dwFreeType: DWORD,
    ) -> BOOL {
        // Bellek serbest bırakma (taslak uygulama - gerçekleştirilecek)
        TRUE
    }
    
    /// GetTickCount - Sistem başlatıldığından bu yana geçen milisaniye sayısını döndürür
    pub unsafe fn get_tick_count() -> DWORD {
        crate::task::scheduler::get_ticks() as DWORD
    }
    
    /// Sleep - Geçerli iş parçacığını belirtilen milisaniye kadar uyutur
    pub unsafe fn sleep(dwMilliseconds: DWORD) {
        if dwMilliseconds == 0 {
            crate::task::scheduler::sleep(1);
            return;
        }

        let ticks = core::cmp::max(1usize, (dwMilliseconds as usize).saturating_div(10));
        crate::task::scheduler::sleep(ticks);
        for _ in 0..(dwMilliseconds.min(5) * 256) {
            core::hint::spin_loop();
        }
    }
    
    /// CreateFileA - Dosya, aygıt, boru veya sanal cihaz oluşturur ya da var olanı açar
    pub unsafe fn create_file_a(
        lpFileName: LPCSTR,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: LPVOID,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE {
        let _ = (dwShareMode, lpSecurityAttributes, dwFlagsAndAttributes, hTemplateFile);

        let Some(name) = cstr_to_string(lpFileName) else {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return INVALID_HANDLE_VALUE;
        };

        let readable = (dwDesiredAccess & GENERIC_READ) != 0 || dwDesiredAccess == 0;
        let writable = (dwDesiredAccess & GENERIC_WRITE) != 0;

        let mut storage = FILE_STORAGE.lock();
        let exists = storage.contains_key(&name);
        match dwCreationDisposition {
            CREATE_NEW => {
                if exists {
                    set_last_error_internal(ERROR_ALREADY_EXISTS);
                    return INVALID_HANDLE_VALUE;
                }
                storage.insert(name.clone(), Vec::new());
            }
            CREATE_ALWAYS => {
                storage.insert(name.clone(), Vec::new());
            }
            OPEN_EXISTING => {
                if !exists {
                    set_last_error_internal(ERROR_FILE_NOT_FOUND);
                    return INVALID_HANDLE_VALUE;
                }
            }
            _ => {
                set_last_error_internal(ERROR_INVALID_PARAMETER);
                return INVALID_HANDLE_VALUE;
            }
        }

        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        FILE_HANDLES.lock().insert(
            handle,
            Win32FileHandle {
                path: name.clone(),
                position: 0,
                readable,
                writable,
            },
        );
        set_last_error_internal(ERROR_SUCCESS);
        crate::serial_println!("[WIN32] CreateFileA: {} -> handle={}", name, handle);
        handle
    }
    
    /// ReadFile - Dosyadan veya G/Ç aygıtından veri okur, okunan bayt sayısını lpNumberOfBytesRead'e yazar
    pub unsafe fn read_file(
        hFile: HANDLE,
        lpBuffer: LPVOID,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        let _ = lpOverlapped;
        if !lpNumberOfBytesRead.is_null() {
            *lpNumberOfBytesRead = 0;
        }

        if lpBuffer.is_null() && nNumberOfBytesToRead > 0 {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        let mut handles = FILE_HANDLES.lock();
        let Some(file) = handles.get_mut(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };

        if !file.readable {
            set_last_error_internal(ERROR_ACCESS_DENIED);
            return FALSE;
        }

        let storage = FILE_STORAGE.lock();
        let Some(data) = storage.get(&file.path) else {
            set_last_error_internal(ERROR_FILE_NOT_FOUND);
            return FALSE;
        };

        let start = file.position.min(data.len());
        let remaining = data.len().saturating_sub(start);
        let requested = nNumberOfBytesToRead as usize;
        let to_copy = remaining.min(requested);

        if to_copy > 0 {
            core::ptr::copy_nonoverlapping(data.as_ptr().add(start), lpBuffer as *mut u8, to_copy);
            file.position = file.position.saturating_add(to_copy);
        }

        if !lpNumberOfBytesRead.is_null() {
            *lpNumberOfBytesRead = to_copy as DWORD;
        }
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// WriteFile - Dosyaya veya G/Ç aygıtına veri yazar, yazılan bayt sayısını lpNumberOfBytesWritten'a yazar
    pub unsafe fn write_file(
        hFile: HANDLE,
        lpBuffer: LPCVOID,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: LPVOID,
    ) -> BOOL {
        let _ = lpOverlapped;
        if !lpNumberOfBytesWritten.is_null() {
            *lpNumberOfBytesWritten = 0;
        }

        if lpBuffer.is_null() && nNumberOfBytesToWrite > 0 {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        let mut handles = FILE_HANDLES.lock();
        let Some(file) = handles.get_mut(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };

        if !file.writable {
            set_last_error_internal(ERROR_ACCESS_DENIED);
            return FALSE;
        }

        let mut storage = FILE_STORAGE.lock();
        let data = storage.entry(file.path.clone()).or_insert_with(Vec::new);
        let write_len = nNumberOfBytesToWrite as usize;
        let needed_len = file.position.saturating_add(write_len);
        if data.len() < needed_len {
            data.resize(needed_len, 0);
        }

        if write_len > 0 {
            core::ptr::copy_nonoverlapping(
                lpBuffer as *const u8,
                data.as_mut_ptr().add(file.position),
                write_len,
            );
            file.position = file.position.saturating_add(write_len);
        }

        if !lpNumberOfBytesWritten.is_null() {
            *lpNumberOfBytesWritten = nNumberOfBytesToWrite;
        }
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// CloseHandle - Açık bir nesne tanıtıcısını kapatır ve kaynakları serbest bırakır
    pub unsafe fn close_handle(hObject: HANDLE) -> BOOL {
        let removed = FILE_HANDLES.lock().remove(&hObject).is_some()
            || PROCESS_HANDLES.lock().remove(&hObject).is_some()
            || THREAD_HANDLES.lock().remove(&hObject).is_some();
        if removed {
            set_last_error_internal(ERROR_SUCCESS);
            TRUE
        } else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            FALSE
        }
    }
    
    /// ExitProcess - Geçerli süreci ve tüm iş parçacıklarını verilen çıkış koduyla sonlandırır
    pub unsafe fn exit_process(uExitCode: UINT) {
        crate::serial_println!("[WIN32] ExitProcess({})", uExitCode);
        // TODO: Gerçek süreç sonlandırma mekanizması uygulanacak (görev zamanlayıcısına bildirilecek)
        loop {}
    }
    
    // ========================================================================
    // SÜREÇ YÖNETİMİ (proses oluşturma, kimlik sorgulama ve yaşam döngüsü)
    // ========================================================================
    
    /// CreateProcessA - Yeni bir süreç ve birincil iş parçacığı oluşturur
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
        let _ = (
            lpProcessAttributes,
            lpThreadAttributes,
            bInheritHandles,
            dwCreationFlags,
            lpEnvironment,
            lpCurrentDirectory,
            lpStartupInfo,
        );

        let mut name = cstr_to_string(lpApplicationName).unwrap_or_default();
        if name.is_empty() {
            name = cstr_to_string(lpCommandLine as LPCSTR).unwrap_or_else(|| "process".to_string());
        }

        let pid = NEXT_PROCESS_ID.fetch_add(1, Ordering::Relaxed);
        let tid = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
        let process_handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let thread_handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

        PROCESS_HANDLES.lock().insert(
            process_handle,
            Win32ProcessHandle {
                pid,
                name: name.clone(),
                exit_code: STILL_ACTIVE,
                signaled: false,
            },
        );
        THREAD_HANDLES.lock().insert(
            thread_handle,
            Win32ThreadHandle {
                tid,
                owner_pid: pid,
                exit_code: STILL_ACTIVE,
                suspended: (dwCreationFlags & CREATE_SUSPENDED) != 0,
                signaled: false,
            },
        );

        if !lpProcessInformation.is_null() {
            let info = lpProcessInformation as *mut PROCESS_INFORMATION;
            (*info).hProcess = process_handle;
            (*info).hThread = thread_handle;
            (*info).dwProcessId = pid;
            (*info).dwThreadId = tid;
        }

        set_last_error_internal(ERROR_SUCCESS);
        crate::serial_println!("[WIN32] CreateProcessA: {}", name);
        TRUE
    }
    
    /// OpenProcess - Mevcut bir sürece erişim tanıtıcısı döndürür; izin denetimi yapılır
    pub unsafe fn open_process(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE {
        let _ = (dwDesiredAccess, bInheritHandle);
        crate::serial_println!("[WIN32] OpenProcess: pid={}", dwProcessId);
        let processes = PROCESS_HANDLES.lock();
        for (handle, process) in processes.iter() {
            if process.pid == dwProcessId {
                set_last_error_internal(ERROR_SUCCESS);
                return *handle;
            }
        }
        set_last_error_internal(ERROR_INVALID_HANDLE);
        0
    }
    
    /// TerminateProcess - Belirtilen süreci zorla sonlandırır ve çıkış kodunu ayarlar
    pub unsafe fn terminate_process(hProcess: HANDLE, uExitCode: UINT) -> BOOL {
        crate::serial_println!("[WIN32] TerminateProcess: handle={}", hProcess);
        let mut processes = PROCESS_HANDLES.lock();
        let Some(process) = processes.get_mut(&hProcess) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };

        process.exit_code = uExitCode;
        process.signaled = true;
        let pid = process.pid;
        drop(processes);

        let mut threads = THREAD_HANDLES.lock();
        for thread in threads.values_mut() {
            if thread.owner_pid == pid {
                thread.exit_code = uExitCode;
                thread.signaled = true;
                thread.suspended = false;
            }
        }
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// GetExitCodeProcess - Sürecin çıkış kodunu sorgular; süreç hâlâ çalışıyorsa STILL_ACTIVE döner
    pub unsafe fn get_exit_code_process(hProcess: HANDLE, lpExitCode: *mut DWORD) -> BOOL {
        if lpExitCode.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        if hProcess == get_current_process() {
            *lpExitCode = STILL_ACTIVE;
            set_last_error_internal(ERROR_SUCCESS);
            return TRUE;
        }

        let processes = PROCESS_HANDLES.lock();
        let Some(process) = processes.get(&hProcess) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };
        *lpExitCode = process.exit_code;
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }

    pub unsafe fn get_exit_code_thread(hThread: HANDLE, lpExitCode: *mut DWORD) -> BOOL {
        if lpExitCode.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        if hThread == get_current_thread() {
            *lpExitCode = STILL_ACTIVE;
            set_last_error_internal(ERROR_SUCCESS);
            return TRUE;
        }

        let threads = THREAD_HANDLES.lock();
        let Some(thread) = threads.get(&hThread) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };
        *lpExitCode = thread.exit_code;
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }

    pub unsafe fn get_process_id(hProcess: HANDLE) -> DWORD {
        if hProcess == get_current_process() {
            return get_current_process_id();
        }

        PROCESS_HANDLES
            .lock()
            .get(&hProcess)
            .map(|p| p.pid)
            .unwrap_or(0)
    }

    pub unsafe fn get_thread_id(hThread: HANDLE) -> DWORD {
        if hThread == get_current_thread() {
            return get_current_thread_id();
        }

        THREAD_HANDLES
            .lock()
            .get(&hThread)
            .map(|t| t.tid)
            .unwrap_or(0)
    }
    
    /// GetCurrentProcess - Geçerli sürece ait sabit bir sözde tanıtıcı döndürür (0xFFFF...)
    pub unsafe fn get_current_process() -> HANDLE {
        0xFFFFFFFFFFFFFFFF
    }
    
    /// GetCurrentProcessId - Geçerli sürecin benzersiz işletim sistemi kimliğini döndürür
    pub unsafe fn get_current_process_id() -> DWORD {
        crate::task::scheduler::current_task_id() as DWORD
    }
    
    // ========================================================================
    // İŞ PARÇACİĞİ YÖNETİMİ (thread oluşturma, askıya alma ve senkronizasyon)
    // ========================================================================
    
    /// CreateThread - Süreç adres alanında çalışacak yeni bir iş parçacığı oluşturur
    pub unsafe fn create_thread(
        lpThreadAttributes: LPVOID,
        dwStackSize: SIZE_T,
        lpStartAddress: LPVOID,
        lpParameter: LPVOID,
        dwCreationFlags: DWORD,
        lpThreadId: *mut DWORD,
    ) -> HANDLE {
        let _ = (lpThreadAttributes, dwStackSize, lpStartAddress, lpParameter);
        crate::serial_println!("[WIN32] CreateThread");
        let tid = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        THREAD_HANDLES.lock().insert(
            handle,
            Win32ThreadHandle {
                tid,
                owner_pid: get_current_process_id(),
                exit_code: STILL_ACTIVE,
                suspended: (dwCreationFlags & CREATE_SUSPENDED) != 0,
                signaled: false,
            },
        );
        if !lpThreadId.is_null() {
            *lpThreadId = tid;
        }
        set_last_error_internal(ERROR_SUCCESS);
        handle
    }
    
    /// ExitThread - Geçerli iş parçacığını belirtilen çıkış koduyla sonlandırır
    pub unsafe fn exit_thread(dwExitCode: DWORD) {
        crate::serial_println!("[WIN32] ExitThread({})", dwExitCode);
    }
    
    /// GetCurrentThread - Geçerli iş parçacığına ait sabit sözde tanıtıcı döndürür (0xFFFFFFFE...)
    pub unsafe fn get_current_thread() -> HANDLE {
        0xFFFFFFFFFFFFFFFE
    }
    
    /// GetCurrentThreadId - Geçerli iş parçacığının benzersiz kimliğini döndürür
    pub unsafe fn get_current_thread_id() -> DWORD {
        crate::task::scheduler::current_task_id() as DWORD
    }
    
    /// ResumeThread - Askıya alınmış iş parçacığını yeniden çalıştırır; eski askı sayacını döndürür
    pub unsafe fn resume_thread(hThread: HANDLE) -> DWORD {
        let mut threads = THREAD_HANDLES.lock();
        let Some(thread) = threads.get_mut(&hThread) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return WAIT_FAILED;
        };
        let was_suspended = thread.suspended;
        thread.suspended = false;
        set_last_error_internal(ERROR_SUCCESS);
        if was_suspended { 1 } else { 0 }
    }
    
    /// SuspendThread - İş parçacığını askıya alır; birden fazla çağrı sayacı artırır
    pub unsafe fn suspend_thread(hThread: HANDLE) -> DWORD {
        let mut threads = THREAD_HANDLES.lock();
        let Some(thread) = threads.get_mut(&hThread) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return WAIT_FAILED;
        };
        let was_suspended = thread.suspended;
        thread.suspended = true;
        set_last_error_internal(ERROR_SUCCESS);
        if was_suspended { 1 } else { 0 }
    }
    
    /// WaitForSingleObject - Nesne sinyal alana veya zaman aşımı dolana kadar bekler
    pub unsafe fn wait_for_single_object(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD {
        let Some(signaled_now) = handle_signaled(hHandle) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return WAIT_FAILED;
        };

        if signaled_now {
            return WAIT_OBJECT_0;
        }

        if dwMilliseconds == 0 {
            return WAIT_TIMEOUT;
        }

        if dwMilliseconds != INFINITE {
            sleep(core::cmp::min(dwMilliseconds, 20));
        } else {
            sleep(10);
        }

        match handle_signaled(hHandle) {
            Some(true) => WAIT_OBJECT_0,
            Some(false) => WAIT_TIMEOUT,
            None => {
                set_last_error_internal(ERROR_INVALID_HANDLE);
                WAIT_FAILED
            }
        }
    }
    
    /// WaitForMultipleObjects - Birden fazla nesneden bir veya hepsinin sinyal almasını bekler
    pub unsafe fn wait_for_multiple_objects(
        nCount: DWORD,
        lpHandles: *const HANDLE,
        bWaitAll: BOOL,
        dwMilliseconds: DWORD,
    ) -> DWORD {
        if nCount == 0 || (lpHandles.is_null() && nCount > 0) {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return WAIT_FAILED;
        }

        let count = nCount as usize;
        let handles = core::slice::from_raw_parts(lpHandles, count);

        if bWaitAll != FALSE {
            let mut all_signaled = true;
            for handle in handles.iter() {
                match handle_signaled(*handle) {
                    Some(true) => {}
                    Some(false) => all_signaled = false,
                    None => {
                        set_last_error_internal(ERROR_INVALID_HANDLE);
                        return WAIT_FAILED;
                    }
                }
            }

            if all_signaled {
                return WAIT_OBJECT_0;
            }

            if dwMilliseconds == 0 {
                return WAIT_TIMEOUT;
            }
            sleep(core::cmp::min(dwMilliseconds, 20));

            for handle in handles.iter() {
                match handle_signaled(*handle) {
                    Some(true) => {}
                    Some(false) => return WAIT_TIMEOUT,
                    None => {
                        set_last_error_internal(ERROR_INVALID_HANDLE);
                        return WAIT_FAILED;
                    }
                }
            }
            WAIT_OBJECT_0
        } else {
            for (index, handle) in handles.iter().enumerate() {
                match handle_signaled(*handle) {
                    Some(true) => return WAIT_OBJECT_0 + index as DWORD,
                    Some(false) => {}
                    None => {
                        set_last_error_internal(ERROR_INVALID_HANDLE);
                        return WAIT_FAILED;
                    }
                }
            }

            if dwMilliseconds == 0 {
                return WAIT_TIMEOUT;
            }
            sleep(core::cmp::min(dwMilliseconds, 20));

            for (index, handle) in handles.iter().enumerate() {
                if matches!(handle_signaled(*handle), Some(true)) {
                    return WAIT_OBJECT_0 + index as DWORD;
                }
            }
            WAIT_TIMEOUT
        }
    }
    
    // ========================================================================
    // BELLEK YÖNETİMİ (sanal bellek koruma, heap ve yerel bellek işlemleri)
    // ========================================================================
    
    /// VirtualProtect - Sanal bellek bölgesinin koruma özniteliklerini değiştirir
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
    
    /// VirtualQuery - Belirtilen adresteki sanal bellek bölgesi hakkında bilgi döndürür
    pub unsafe fn virtual_query(
        lpAddress: LPCVOID,
        lpBuffer: LPVOID,
        dwLength: SIZE_T,
    ) -> SIZE_T {
        0
    }
    
    /// HeapCreate - İşlem için özel bir heap bölgesi oluşturur
    pub unsafe fn heap_create(flOptions: DWORD, dwInitialSize: SIZE_T, dwMaximumSize: SIZE_T) -> HANDLE {
        crate::serial_println!("[WIN32] HeapCreate: size={}", dwInitialSize);
        1 as HANDLE
    }
    
    /// HeapDestroy - Heap'i ve içindeki tüm bellek bloklarını yok eder
    pub unsafe fn heap_destroy(hHeap: HANDLE) -> BOOL {
        TRUE
    }
    
    /// HeapAlloc - Heap’ten belirtilen boyutta bellek ayırır
    pub unsafe fn heap_alloc(hHeap: HANDLE, dwFlags: DWORD, dwBytes: SIZE_T) -> LPVOID {
        // TODO: Gerçek heap ayırıcısıyla entegre edilecek
        crate::serial_println!("[WIN32] HeapAlloc: {} bytes", dwBytes);
        core::ptr::null_mut()
    }
    
    /// HeapFree - HeapAlloc ile ayrılmış bir bellek bloğunu serbest bırakır
    pub unsafe fn heap_free(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPVOID) -> BOOL {
        TRUE
    }
    
    /// HeapReAlloc - Var olan bir heap bloğunu yeni bir boyuta yeniden tahsis eder
    pub unsafe fn heap_realloc(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPVOID, dwBytes: SIZE_T) -> LPVOID {
        core::ptr::null_mut()
    }
    
    /// HeapSize - Heap bloğunun boyutunu sorgular
    pub unsafe fn heap_size(hHeap: HANDLE, dwFlags: DWORD, lpMem: LPCVOID) -> SIZE_T {
        0
    }
    
    /// GetProcessHeap - Varsayılan işlem heap’inin tanıtıcısını döndürür
    pub unsafe fn get_process_heap() -> HANDLE {
        1 as HANDLE
    }
    
    /// LocalAlloc - Yerel belleğe blok ayırır (eski API; tercih HeapAlloc)
    pub unsafe fn local_alloc(uFlags: DWORD, uBytes: SIZE_T) -> HANDLE {
        uBytes as HANDLE
    }
    
    /// LocalFree - LocalAlloc ile ayrılmış belleği serbest bırakır
    pub unsafe fn local_free(hMem: HANDLE) -> HANDLE {
        0
    }
    
    /// GlobalAlloc - Küresel belleğe blok ayırır (eski API; tercih HeapAlloc)
    pub unsafe fn global_alloc(uFlags: DWORD, dwBytes: SIZE_T) -> HANDLE {
        dwBytes as HANDLE
    }
    
    /// GlobalFree - GlobalAlloc ile ayrılmış belleği serbest bırakır
    pub unsafe fn global_free(hMem: HANDLE) -> HANDLE {
        0
    }
    
    // ========================================================================
    // DOSYA YÖNETİMİ (dosya işaretisi konumlandırma, boyut sorgulama ve dizin işlemleri)
    // ========================================================================
    
    /// SetFilePointer - Dosya okuma/yazma işaretçisini istenen konuma taşır
    pub unsafe fn set_file_pointer(
        hFile: HANDLE,
        lDistanceToMove: LONG,
        lpDistanceToMoveHigh: *mut LONG,
        dwMoveMethod: DWORD,
    ) -> DWORD {
        let mut handles = FILE_HANDLES.lock();
        let Some(file) = handles.get_mut(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return INVALID_SET_FILE_POINTER;
        };

        let mut distance = lDistanceToMove as i64;
        if !lpDistanceToMoveHigh.is_null() {
            distance |= (*lpDistanceToMoveHigh as i64) << 32;
        }

        let storage = FILE_STORAGE.lock();
        let file_len = storage.get(&file.path).map(|v| v.len()).unwrap_or(0) as i64;
        let base = match dwMoveMethod {
            FILE_BEGIN => 0,
            FILE_CURRENT => file.position as i64,
            FILE_END => file_len,
            _ => {
                set_last_error_internal(ERROR_INVALID_PARAMETER);
                return INVALID_SET_FILE_POINTER;
            }
        };

        let new_pos = base.saturating_add(distance);
        if new_pos < 0 {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return INVALID_SET_FILE_POINTER;
        }

        let new_u64 = new_pos as u64;
        file.position = core::cmp::min(new_u64 as usize, usize::MAX);
        if !lpDistanceToMoveHigh.is_null() {
            *lpDistanceToMoveHigh = ((new_u64 >> 32) & 0xFFFF_FFFF) as LONG;
        }
        set_last_error_internal(ERROR_SUCCESS);
        (new_u64 & 0xFFFF_FFFF) as DWORD
    }
    
    /// SetFilePointerEx - 64-bit duyarlıkla dosya işaretçisini konumlandırır
    pub unsafe fn set_file_pointer_ex(
        hFile: HANDLE,
        liDistanceToMove: i64,
        lpNewFilePointer: *mut i64,
        dwMoveMethod: DWORD,
    ) -> BOOL {
        let mut handles = FILE_HANDLES.lock();
        let Some(file) = handles.get_mut(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };

        let storage = FILE_STORAGE.lock();
        let file_len = storage.get(&file.path).map(|v| v.len()).unwrap_or(0) as i64;
        let base = match dwMoveMethod {
            FILE_BEGIN => 0,
            FILE_CURRENT => file.position as i64,
            FILE_END => file_len,
            _ => {
                set_last_error_internal(ERROR_INVALID_PARAMETER);
                return FALSE;
            }
        };

        let new_pos = base.saturating_add(liDistanceToMove);
        if new_pos < 0 {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        file.position = new_pos as usize;
        if !lpNewFilePointer.is_null() {
            *lpNewFilePointer = new_pos;
        }
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// GetFileSize - Dosyanın boyutunu 32-bit üst/alt word olarak döndürür
    pub unsafe fn get_file_size(hFile: HANDLE, lpFileSizeHigh: *mut DWORD) -> DWORD {
        let handles = FILE_HANDLES.lock();
        let Some(file) = handles.get(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return INVALID_SET_FILE_POINTER;
        };

        let storage = FILE_STORAGE.lock();
        let len = storage.get(&file.path).map(|v| v.len()).unwrap_or(0) as u64;
        if !lpFileSizeHigh.is_null() {
            *lpFileSizeHigh = (len >> 32) as DWORD;
        }
        set_last_error_internal(ERROR_SUCCESS);
        (len & 0xFFFF_FFFF) as DWORD
    }
    
    /// GetFileSizeEx - Dosya boyutunu tek 64-bit işaretli tamsayı olarak döndürür
    pub unsafe fn get_file_size_ex(hFile: HANDLE, lpFileSize: *mut i64) -> BOOL {
        if lpFileSize.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        }

        let handles = FILE_HANDLES.lock();
        let Some(file) = handles.get(&hFile) else {
            set_last_error_internal(ERROR_INVALID_HANDLE);
            return FALSE;
        };

        let storage = FILE_STORAGE.lock();
        let len = storage.get(&file.path).map(|v| v.len()).unwrap_or(0) as i64;
        *lpFileSize = len;
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// GetFileAttributesA - Dosyanın öznitelik maskesini döndürür (e.g. FILE_ATTRIBUTE_NORMAL)
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
    
    /// SetFileAttributesA - Dosyanın öznitelik maskesini değiştirir (gizli, salt okunur vb.)
    pub unsafe fn set_file_attributes_a(lpFileName: LPCSTR, dwFileAttributes: DWORD) -> BOOL {
        TRUE
    }
    
    /// DeleteFileA - Belirtilen dosyayı dosya sisteminden siler
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
    
    /// MoveFileA - Dosyayı ya da dizini yeni konuma taşır ya da yeniden adlandırır
    pub unsafe fn move_file_a(lpExistingFileName: LPCSTR, lpNewFileName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// CopyFileA - Dosyayı yeni konuma kopyalar; bFailIfExists TRUE ise üzerine yazmaz
    pub unsafe fn copy_file_a(lpExistingFileName: LPCSTR, lpNewFileName: LPCSTR, bFailIfExists: BOOL) -> BOOL {
        TRUE
    }
    
    /// FindFirstFileA - Bir glob deseni ile eşleşen ilk dosyayı bulmak için arama başlatır
    pub unsafe fn find_first_file_a(lpFileName: LPCSTR, lpFindFileData: LPVOID) -> HANDLE {
        INVALID_HANDLE_VALUE
    }
    
    /// FindNextFileA - Bir önceki FindFirstFileA aramaıyla eşleşen sonraki dosyayı döndürür
    pub unsafe fn find_next_file_a(hFindFile: HANDLE, lpFindFileData: LPVOID) -> BOOL {
        FALSE
    }
    
    /// FindClose - FindFirstFileA ile başlatılmış dizin arama tanıtıcısını kapatır
    pub unsafe fn find_close(hFindFile: HANDLE) -> BOOL {
        TRUE
    }
    
    /// CreateDirectoryA - Yeni bir dizin oluşturur; lpSecurityAttributes güvenlik tanımlarıı öngörür
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
    
    /// RemoveDirectoryA - Boş bir dizini siler
    pub unsafe fn remove_directory_a(lpPathName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// GetCurrentDirectoryA - Geçerli çalışma dizinini lpBuffer’a yazar
    pub unsafe fn get_current_directory_a(nBufferLength: DWORD, lpBuffer: LPSTR) -> DWORD {
        if !lpBuffer.is_null() && nBufferLength >= 2 {
            *lpBuffer = '\\' as i8;
            *((lpBuffer as *mut u8).add(1)) = 0;
        }
        2
    }
    
    /// SetCurrentDirectoryA - Sürecin geçerli çalışma dizinini değiştirir
    pub unsafe fn set_current_directory_a(lpPathName: LPCSTR) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // ORTAM DEĞİŞKENLERİ (süreç ortamı okuma, yazma ve komut satırı erişimi)
    // ========================================================================
    
    /// GetEnvironmentVariableA - Ortam değişkeni değerini lpBuffer’a yazar ve uzunluğunu döndürür
    pub unsafe fn get_environment_variable_a(lpName: LPCSTR, lpBuffer: LPSTR, nSize: DWORD) -> DWORD {
        let Some(name) = cstr_to_string(lpName) else {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return 0;
        };

        let env = ENV_VARS.lock();
        let value = match env.get(&name) {
            Some(v) => v.as_str(),
            None => {
                set_last_error_internal(ERROR_FILE_NOT_FOUND);
                return 0;
            }
        };

        let needed = value.len() + 1;
        if (nSize as usize) < needed {
            set_last_error_internal(ERROR_INSUFFICIENT_BUFFER);
            return needed as DWORD;
        }

        let written = write_cstr_bytes(lpBuffer, nSize, value.as_bytes());
        set_last_error_internal(ERROR_SUCCESS);
        written
    }
    
    /// SetEnvironmentVariableA - Ortam değişkenini belirler ya da lpValue NULL ise siler
    pub unsafe fn set_environment_variable_a(lpName: LPCSTR, lpValue: LPCSTR) -> BOOL {
        let Some(name) = cstr_to_string(lpName) else {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        };

        let mut env = ENV_VARS.lock();
        if lpValue.is_null() {
            env.remove(&name);
            set_last_error_internal(ERROR_SUCCESS);
            return TRUE;
        }

        let Some(value) = cstr_to_string(lpValue) else {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return FALSE;
        };
        env.insert(name, value);
        set_last_error_internal(ERROR_SUCCESS);
        TRUE
    }
    
    /// GetCommandLineA - Sürecin komut satırı dizesine işaretçi döndürür
    pub unsafe fn get_command_line_a() -> LPSTR {
        static mut CMDLINE_BUF: [u8; 128] = [0; 128];
        let cmd = b"echos.exe\0";
        core::ptr::copy_nonoverlapping(cmd.as_ptr(), CMDLINE_BUF.as_mut_ptr(), cmd.len());
        CMDLINE_BUF.as_mut_ptr() as LPSTR
    }
    
    // ========================================================================
    // KONSOL İŞLEMLERİ (standart giriş/çıkış tanıtıcıları ve konsol tampon yönetimi)
    // ========================================================================
    
    /// GetStdHandle - Standart giriş (−0xa), çıkış veya hata akışının tanıtıcısını döndürür
    pub unsafe fn get_std_handle(nStdHandle: DWORD) -> HANDLE {
        nStdHandle as HANDLE
    }
    
    /// SetStdHandle - Standart giriş/çıkış/hata akışını belirtilen tanıtıcıya yönlendirir
    pub unsafe fn set_std_handle(nStdHandle: DWORD, hHandle: HANDLE) -> BOOL {
        TRUE
    }
    
    /// WriteConsoleA - Konsol çıkış tamponuna karakter yazar
    pub unsafe fn write_console_a(
        hConsoleOutput: HANDLE,
        lpBuffer: *const u8,
        nNumberOfCharsToWrite: DWORD,
        lpNumberOfCharsWritten: *mut DWORD,
        lpReserved: LPVOID,
    ) -> BOOL {
        // Karakterleri seri porta yaz (konsolun echOS’daki karşılığı seri port çıkışıdır)
        for i in 0..nNumberOfCharsToWrite {
            crate::serial_print!("{}", *lpBuffer.add(i as usize) as char);
        }
        if !lpNumberOfCharsWritten.is_null() {
            *lpNumberOfCharsWritten = nNumberOfCharsToWrite;
        }
        TRUE
    }
    
    /// ReadConsoleA - Konsol giriş tamponundan karakter okur
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
    
    /// SetConsoleMode - Konsol giriş/çıkış modunu belirler (satır tamponu, yankı vb.)
    pub unsafe fn set_console_mode(hConsoleHandle: HANDLE, dwMode: DWORD) -> BOOL {
        TRUE
    }
    
    /// GetConsoleMode - Geçerli konsol modunu sorgular
    pub unsafe fn get_console_mode(hConsoleHandle: HANDLE, lpMode: *mut DWORD) -> BOOL {
        if !lpMode.is_null() {
            *lpMode = 0;
        }
        TRUE
    }
    
    /// SetConsoleTextAttribute - Konsol çıkışının metin rengini ve arka plan rengini ayarlar
    pub unsafe fn set_console_text_attribute(hConsoleOutput: HANDLE, wAttributes: WORD) -> BOOL {
        TRUE
    }
    
    /// GetConsoleScreenBufferInfo - Konsol ekran tampon bilgilerini yapıya yazar
    pub unsafe fn get_console_screen_buffer_info(hConsoleOutput: HANDLE, lpConsoleScreenBufferInfo: LPVOID) -> BOOL {
        TRUE
    }
    
    /// FillConsoleOutputCharacterA - Konsol ekranı belirli bir karakter ile doldurur
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
    // SİSTEM BİLGİSİ (işlemci, bellek miktarı ve işletim sistemi sürüm sorgusu)
    // ========================================================================
    
    /// GetSystemInfo - İşlemci sayısı, sayfa boyutu vb. donanım bilgilerini yapıya yazar
    pub unsafe fn get_system_info(lpSystemInfo: LPVOID) {
        // SYSTEM_INFO yapısını henüz doldurmuyoruz (taslak)
    }
    
    /// GlobalMemoryStatus - Fiziksel ve sanal bellek kullanım istatistiklerini yapıya yazar
    pub unsafe fn global_memory_status(lpMemoryStatus: LPVOID) {
        // MEMORYSTATUS yapısını henüz doldurmuyoruz (taslak)
    }
    
    /// GlobalMemoryStatusEx - Genişletilmiş bellek durum bilgisini döndürür (64-bit uyumlu)
    pub unsafe fn global_memory_status_ex(lpMemoryStatus: LPVOID) -> BOOL {
        TRUE
    }
    
    /// GetVersion - Windows sürüm numarasını paketlenmiş DWORD olarak döndürür
    pub unsafe fn get_version() -> DWORD {
        // Windows 10 sürümünü taklit ediyoruz (ana sürüm 10 = 0x000A)
        0x000A0000
    }
    
    /// GetVersionExA - Genişletilmiş sürüm bilgisini OSVERSIONINFO yapısına yazar
    pub unsafe fn get_version_ex_a(lpVersionInfo: LPVOID) -> BOOL {
        TRUE
    }
    
    /// GetComputerNameA - Bilgisayarın NetBIOS adını lpBuffer’a yazar
    pub unsafe fn get_computer_name_a(lpBuffer: LPSTR, lpnSize: *mut DWORD) -> BOOL {
        if !lpBuffer.is_null() && !lpnSize.is_null() {
            let name = b"echOS\0";
            let len = core::cmp::min(*lpnSize as usize, name.len());
            core::ptr::copy_nonoverlapping(name.as_ptr(), lpBuffer as *mut u8, len);
            *lpnSize = (len - 1) as DWORD;
        }
        TRUE
    }
    
    /// GetUserNameA - Geçerli kullanıcının adını lpBuffer'a yazar; uzunluğu lpnSize ile döndürür
    pub unsafe fn get_user_name_a(lpBuffer: LPSTR, lpnSize: *mut DWORD) -> BOOL {
        if !lpBuffer.is_null() && !lpnSize.is_null() {
            let name = b"user\0";
            let len = core::cmp::min(*lpnSize as usize, name.len());
            core::ptr::copy_nonoverlapping(name.as_ptr(), lpBuffer as *mut u8, len);
            *lpnSize = (len - 1) as DWORD;
        }
        TRUE
    }
    
    /// GetLastError - Bu iş parçacığı için en son oluşan hata kodunu döndürür
    pub unsafe fn get_last_error() -> DWORD {
        LAST_ERROR.load(Ordering::Relaxed)
    }
    
    /// SetLastError - Bu iş parçacığı için son hata kodunu elle belirler
    pub unsafe fn set_last_error(dwErrCode: DWORD) {
        set_last_error_internal(dwErrCode);
    }
    
    /// MultiByteToWideChar - Çok baytlı karakter dizisini UTF-16 geniş karakter dizisine çevirir
    pub unsafe fn multi_byte_to_wide_char(
        CodePage: DWORD,
        dwFlags: DWORD,
        lpMultiByteStr: LPCSTR,
        cbMultiByte: INT,
        lpWideCharStr: LPWSTR,
        cchWideChar: INT,
    ) -> INT {
        let _ = (CodePage, dwFlags);
        if lpMultiByteStr.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return 0;
        }

        let src_len = if cbMultiByte < 0 {
            let mut len = 0usize;
            let mut p = lpMultiByteStr;
            while !p.is_null() && *p != 0 {
                len += 1;
                p = p.add(1);
            }
            len
        } else {
            cbMultiByte as usize
        };

        if lpWideCharStr.is_null() || cchWideChar <= 0 {
            set_last_error_internal(ERROR_SUCCESS);
            return src_len as INT;
        }

        let out_cap = cchWideChar as usize;
        let to_copy = core::cmp::min(src_len, out_cap);
        for idx in 0..to_copy {
            *lpWideCharStr.add(idx) = *lpMultiByteStr.add(idx) as u8 as u16;
        }
        if to_copy < out_cap {
            *lpWideCharStr.add(to_copy) = 0;
        }
        set_last_error_internal(ERROR_SUCCESS);
        to_copy as INT
    }
    
    /// WideCharToMultiByte - UTF-16 geniş karakter dizisini çok baytlı karakter dizisine çevirir
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
        let _ = (CodePage, dwFlags, lpDefaultChar, lpUsedDefaultChar);
        if lpWideCharStr.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return 0;
        }

        let src_len = if cchWideChar < 0 {
            let mut len = 0usize;
            let mut p = lpWideCharStr;
            while !p.is_null() && *p != 0 {
                len += 1;
                p = p.add(1);
            }
            len
        } else {
            cchWideChar as usize
        };

        if lpMultiByteStr.is_null() || cbMultiByte <= 0 {
            set_last_error_internal(ERROR_SUCCESS);
            return src_len as INT;
        }

        let out_cap = cbMultiByte as usize;
        let to_copy = core::cmp::min(src_len, out_cap.saturating_sub(1));
        for idx in 0..to_copy {
            let ch = *lpWideCharStr.add(idx);
            *lpMultiByteStr.add(idx) = if ch <= 0xFF { ch as i8 } else { b'?' as i8 };
        }
        *lpMultiByteStr.add(to_copy) = 0;
        set_last_error_internal(ERROR_SUCCESS);
        to_copy as INT
    }
    
    /// lstrlenA - Null-sonlandırılmış ANSI dizisinin karakter uzunluğunu döndürür
    pub unsafe fn lstrlen_a(lpString: LPCSTR) -> INT {
        let mut len = 0;
        let mut ptr = lpString;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
    
    /// lstrlenW - Null-sonlandırılmış Unicode (UTF-16) dizisinin karakter uzunluğunu döndürür
    pub unsafe fn lstrlen_w(lpString: LPCWSTR) -> INT {
        let mut len = 0;
        let mut ptr = lpString;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len
    }
    
    /// lstrcpyA - Kaynak ANSI dizisini hedef tampona kopyalar (null bileşeni dahil)
    pub unsafe fn lstrcpy_a(lpString1: LPSTR, lpString2: LPCSTR) -> LPSTR {
        if lpString1.is_null() || lpString2.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return core::ptr::null_mut();
        }

        let mut src = lpString2;
        let mut dst = lpString1;
        while !src.is_null() {
            let ch = *src;
            *dst = ch;
            if ch == 0 {
                break;
            }
            src = src.add(1);
            dst = dst.add(1);
        }

        set_last_error_internal(ERROR_SUCCESS);
        lpString1
    }
    
    /// lstrcatA - Kaynak ANSI dizisini hedef dizinin sonuna ekler
    pub unsafe fn lstrcat_a(lpString1: LPSTR, lpString2: LPCSTR) -> LPSTR {
        if lpString1.is_null() || lpString2.is_null() {
            set_last_error_internal(ERROR_INVALID_PARAMETER);
            return core::ptr::null_mut();
        }

        let mut dst = lpString1;
        while !dst.is_null() && *dst != 0 {
            dst = dst.add(1);
        }

        let mut src = lpString2;
        while !src.is_null() {
            let ch = *src;
            *dst = ch;
            if ch == 0 {
                break;
            }
            src = src.add(1);
            dst = dst.add(1);
        }

        set_last_error_internal(ERROR_SUCCESS);
        lpString1
    }
    
    /// CompareStringA - İki ANSI dizisini yerel ayara göre kıyaslar (CSTR_EQUAL=2 döner)
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
    
    /// lstrcmpA - İki ANSI dizisini büyük/küçük harfe duyarlı kıyaslar; sıra farkını döndürür
    pub unsafe fn lstrcmp_a(lpString1: LPCSTR, lpString2: LPCSTR) -> INT {
        0
    }
    
    /// lstrcmpiA - İki ANSI dizisini büyük/küçük harfe duyarsız kıyaslar
    pub unsafe fn lstrcmpi_a(lpString1: LPCSTR, lpString2: LPCSTR) -> INT {
        0
    }
}

// ============================================================================
// USER32 UYGULAMASI (pencere, mesaj döngüsü ve kullanıcı girişi API emülasyonu)
// ============================================================================

mod user32 {
    use super::*;
    use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

    const PM_REMOVE: UINT = 0x0001;
    const WM_SETTEXT: UINT = 0x000C;
    const WM_GETTEXT: UINT = 0x000D;
    const WM_GETTEXTLENGTH: UINT = 0x000E;
    const WM_NCDESTROY: UINT = 0x0082;
    const WM_COMMAND: UINT = 0x0111;
    const WM_TIMER: UINT = 0x0113;
    const GW_HWNDFIRST: UINT = 0;
    const GW_HWNDLAST: UINT = 1;
    const GW_HWNDNEXT: UINT = 2;
    const GW_HWNDPREV: UINT = 3;
    const GW_OWNER: UINT = 4;
    const GW_CHILD: UINT = 5;
    const GW_ENABLEDPOPUP: UINT = 6;
    const HWND_BROADCAST: HWND = 0xFFFF;
    const WAIT_OBJECT_0: DWORD = 0;
    const WAIT_TIMEOUT: DWORD = 258;
    const WAIT_FAILED: DWORD = 0xFFFF_FFFF;
    const MF_BYPOSITION: UINT = 0x0400;
    const MF_CHECKED: UINT = 0x0008;
    const MF_DISABLED: UINT = 0x0002;

    #[derive(Clone)]
    struct WindowState {
        class_name: String,
        title: String,
        x: INT,
        y: INT,
        width: INT,
        height: INT,
        parent: HWND,
        visible: bool,
        enabled: bool,
        owner_thread_id: DWORD,
        owner_process_id: DWORD,
    }

    #[derive(Clone)]
    struct MenuItemState {
        id: usize,
        text: String,
        enabled: bool,
        checked: bool,
    }

    #[derive(Clone)]
    struct MenuState {
        popup: bool,
        items: Vec<MenuItemState>,
    }

    #[derive(Clone)]
    struct DialogState {
        parent: HWND,
        ended: bool,
        result: isize,
        text_controls: BTreeMap<INT, String>,
        int_controls: BTreeMap<INT, UINT>,
        check_controls: BTreeMap<INT, UINT>,
        item_handles: BTreeMap<INT, HWND>,
    }

    #[derive(Clone)]
    struct TimerState {
        hwnd: HWND,
        id: usize,
        elapse_ms: UINT,
        accum_ms: UINT,
    }

    #[derive(Clone)]
    struct ClipboardState {
        owner: HWND,
        open_owner: Option<HWND>,
        data: BTreeMap<UINT, HANDLE>,
        viewer: HWND,
    }

    static NEXT_WINDOW_HANDLE: AtomicU64 = AtomicU64::new(1);
    static NEXT_CLASS_ATOM: AtomicU32 = AtomicU32::new(1);
    static NEXT_MENU_HANDLE: AtomicU64 = AtomicU64::new(1);
    static NEXT_DIALOG_HANDLE: AtomicU64 = AtomicU64::new(0x8000);
    static NEXT_CONTROL_HANDLE: AtomicU64 = AtomicU64::new(0x10000);
    static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);
    static WINDOW_REGISTRY: Mutex<BTreeMap<HWND, WindowState>> = Mutex::new(BTreeMap::new());
    static CLASS_REGISTRY: Mutex<BTreeMap<String, WORD>> = Mutex::new(BTreeMap::new());
    static WINDOW_LONG_PTRS: Mutex<BTreeMap<(HWND, INT), isize>> = Mutex::new(BTreeMap::new());
    static CLASS_LONG_VALUES: Mutex<BTreeMap<(String, INT), DWORD>> = Mutex::new(BTreeMap::new());
    static WINDOW_PROPERTIES: Mutex<BTreeMap<(HWND, String), HANDLE>> = Mutex::new(BTreeMap::new());
    static MENU_REGISTRY: Mutex<BTreeMap<HMENU, MenuState>> = Mutex::new(BTreeMap::new());
    static WINDOW_MENUS: Mutex<BTreeMap<HWND, HMENU>> = Mutex::new(BTreeMap::new());
    static DIALOG_REGISTRY: Mutex<BTreeMap<HWND, DialogState>> = Mutex::new(BTreeMap::new());
    static TIMER_REGISTRY: Mutex<BTreeMap<(HWND, usize), TimerState>> = Mutex::new(BTreeMap::new());
    static CLIPBOARD_STATE: Mutex<ClipboardState> = Mutex::new(ClipboardState {
        owner: 0,
        open_owner: None,
        data: BTreeMap::new(),
        viewer: 0,
    });
    static MESSAGE_QUEUE: Mutex<Vec<MSG>> = Mutex::new(Vec::new());
    static LAST_MESSAGE_TIME: AtomicU32 = AtomicU32::new(0);
    static LAST_MESSAGE_POS: AtomicU32 = AtomicU32::new(0);
    static CURSOR_X: AtomicI32 = AtomicI32::new(0);
    static CURSOR_Y: AtomicI32 = AtomicI32::new(0);
    static DOUBLE_CLICK_TIME_MS: AtomicU32 = AtomicU32::new(500);
    static MOUSE_BUTTON_SWAPPED: AtomicBool = AtomicBool::new(false);
    static FOREGROUND_WINDOW: AtomicU64 = AtomicU64::new(0);
    static ACTIVE_WINDOW: AtomicU64 = AtomicU64::new(0);
    static FOCUS_WINDOW: AtomicU64 = AtomicU64::new(0);
    static CAPTURE_WINDOW: AtomicU64 = AtomicU64::new(0);
    static KEYBOARD_STATE: Mutex<[u8; 256]> = Mutex::new([0; 256]);
    static NEXT_REGISTERED_MESSAGE: AtomicU32 = AtomicU32::new(0xC000);
    static REGISTERED_MESSAGES: Mutex<BTreeMap<String, UINT>> = Mutex::new(BTreeMap::new());
    static NEXT_CLIPBOARD_FORMAT: AtomicU32 = AtomicU32::new(0xC000);
    static CLIPBOARD_FORMATS: Mutex<BTreeMap<String, UINT>> = Mutex::new(BTreeMap::new());

    unsafe fn cstr_to_string(ptr: LPCSTR) -> String {
        if ptr.is_null() {
            return String::new();
        }

        let mut out = String::new();
        let mut cursor = ptr;
        while !cursor.is_null() && *cursor != 0 {
            out.push(*cursor as u8 as char);
            cursor = cursor.add(1);
        }
        out
    }

    unsafe fn copy_cstr(dst: LPSTR, max_count: INT, text: &str) -> INT {
        if dst.is_null() || max_count <= 0 {
            return 0;
        }

        let cap = max_count as usize;
        let bytes = text.as_bytes();
        let to_copy = core::cmp::min(bytes.len(), cap.saturating_sub(1));
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, to_copy);
        *((dst as *mut u8).add(to_copy)) = 0;
        to_copy as INT
    }

    fn enqueue_message(hwnd: HWND, message: UINT, w_param: usize, l_param: isize) {
        let time = crate::task::scheduler::get_ticks() as DWORD;
        LAST_MESSAGE_TIME.store(time, Ordering::Relaxed);
        let x = CURSOR_X.load(Ordering::Relaxed);
        let y = CURSOR_Y.load(Ordering::Relaxed);
        let packed_pos = ((y as u32) << 16) | (x as u32 & 0xFFFF);
        LAST_MESSAGE_POS.store(packed_pos, Ordering::Relaxed);
        let msg = MSG {
            hwnd,
            message,
            wParam: w_param,
            lParam: l_param,
            time,
            pt: POINT { x, y },
        };
        MESSAGE_QUEUE.lock().push(msg);
    }

    fn message_matches_filter(msg: &MSG, hwnd: HWND, min: UINT, max: UINT) -> bool {
        let hwnd_ok = hwnd == 0 || msg.hwnd == hwnd;
        let filter_ok = if min == 0 && max == 0 {
            true
        } else {
            msg.message >= min && msg.message <= max
        };
        hwnd_ok && filter_ok
    }

    fn ordered_windows_by_parent(parent: HWND) -> Vec<HWND> {
        WINDOW_REGISTRY
            .lock()
            .iter()
            .filter_map(|(hwnd, window)| {
                if window.parent == parent {
                    Some(*hwnd)
                } else {
                    None
                }
            })
            .collect()
    }

    fn infer_owner_ids() -> (DWORD, DWORD) {
        let id = crate::task::scheduler::current_task_id() as DWORD;
        (id, id)
    }
    
    /// RegisterClassA - Pencere sinifini sisteme kaydeder; CreateWindowExA oncesinde cagrilmalidir
    pub unsafe fn register_class_a(lpWndClass: *const WNDCLASSA) -> WORD {
        if lpWndClass.is_null() || (*lpWndClass).lpszClassName.is_null() {
            return 0;
        }

        let class_name = cstr_to_string((*lpWndClass).lpszClassName);
        if class_name.is_empty() {
            return 0;
        }

        let mut classes = CLASS_REGISTRY.lock();
        if let Some(atom) = classes.get(&class_name) {
            return *atom;
        }

        let atom = NEXT_CLASS_ATOM.fetch_add(1, Ordering::Relaxed) as WORD;
        classes.insert(class_name, atom);
        atom
    }
    
    /// CreateWindowExA - Belirtilen sinif ve stile sahip yeni bir pencere olusturur, hwnd dondurur
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
        let _ = (dwExStyle, hMenu, hInstance, lpParam);
        let class_name = cstr_to_string(lpClassName);
        let title = cstr_to_string(lpWindowName);
        let (owner_thread_id, owner_process_id) = infer_owner_ids();

        let hwnd = NEXT_WINDOW_HANDLE.fetch_add(1, Ordering::Relaxed);
        WINDOW_REGISTRY.lock().insert(
            hwnd,
            WindowState {
                class_name,
                title,
                x,
                y,
                width: nWidth,
                height: nHeight,
                parent: hWndParent,
                visible: (dwStyle & WS_VISIBLE) != 0,
                enabled: true,
                owner_thread_id,
                owner_process_id,
            },
        );

        FOREGROUND_WINDOW.store(hwnd, Ordering::Relaxed);
        ACTIVE_WINDOW.store(hwnd, Ordering::Relaxed);
        FOCUS_WINDOW.store(hwnd, Ordering::Relaxed);
        enqueue_message(hwnd, WM_CREATE, 0, 0);
        crate::serial_println!("[WIN32] CreateWindowExA: hwnd={} {},{} {},{}", hwnd, x, y, nWidth, nHeight);
        hwnd
    }
    
    /// ShowWindow - Pencerenin gorunurlugunu nCmdShow komutuna gore ayarlar (goster/gizle/kucuk)
    pub unsafe fn show_window(hWnd: HWND, nCmdShow: INT) -> BOOL {
        let mut windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get_mut(&hWnd) else {
            return FALSE;
        };
        window.visible = nCmdShow != SW_HIDE;
        crate::serial_println!("[WIN32] ShowWindow: hwnd={} cmd={}", hWnd, nCmdShow);
        TRUE
    }
    
    /// UpdateWindow - WM_PAINT mesaji gondererek pencerenin hemen yeniden cizilmesini saglar
    pub unsafe fn update_window(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            enqueue_message(hWnd, WM_PAINT, 0, 0);
            return TRUE;
        }
        FALSE
    }

    fn dequeue_matching_message(hwnd: HWND, min: UINT, max: UINT, remove: bool) -> Option<MSG> {
        let mut queue = MESSAGE_QUEUE.lock();
        let index = queue
            .iter()
            .position(|msg| message_matches_filter(msg, hwnd, min, max));
        let idx = index?;
        if remove {
            Some(queue.remove(idx))
        } else {
            Some(queue[idx].clone())
        }
    }

    fn pump_timers(delta_ms: UINT) {
        let mut timers = TIMER_REGISTRY.lock();
        let keys: Vec<(HWND, usize)> = timers.keys().copied().collect();
        let mut fired: Vec<(HWND, usize)> = Vec::new();

        for key in keys {
            if let Some(timer) = timers.get_mut(&key) {
                timer.accum_ms = timer.accum_ms.saturating_add(delta_ms);
                let threshold = timer.elapse_ms.max(1);
                if timer.accum_ms >= threshold {
                    timer.accum_ms %= threshold;
                    fired.push((timer.hwnd, timer.id));
                }
            }
        }
        drop(timers);

        for (hwnd, id) in fired {
            enqueue_message(hwnd, WM_TIMER, id, 0);
        }
    }

    /// GetMessageA - Mesaj kuyrugundaki bir sonraki mesaji alir; WM_QUIT alinirsa FALSE doner
    pub unsafe fn get_message_a(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: UINT,
        wMsgFilterMax: UINT,
    ) -> BOOL {
        if lpMsg.is_null() {
            return FALSE;
        }

        for _ in 0..256 {
            if let Some(msg) = dequeue_matching_message(hWnd, wMsgFilterMin, wMsgFilterMax, true) {
                *lpMsg = msg.clone();
                if msg.message == WM_QUIT {
                    return FALSE;
                }
                return TRUE;
            }
            pump_timers(10);
            crate::task::scheduler::sleep(1);
        }
        FALSE
    }

    /// TranslateMessage - WM_KEYDOWN mesajlarini WM_CHAR karakter mesajlarina ceviren yardimci
    pub unsafe fn translate_message(lpMsg: *const MSG) -> BOOL {
        if lpMsg.is_null() {
            return FALSE;
        }
        if (*lpMsg).message == WM_KEYDOWN {
            enqueue_message((*lpMsg).hwnd, WM_CHAR, (*lpMsg).wParam, (*lpMsg).lParam);
            return TRUE;
        }
        TRUE
    }
    
    /// DispatchMessageA - Mesaji hedef pencerenin pencere yordamina iletir ve sonucu dondurur
    pub unsafe fn dispatch_message_a(lpMsg: *const MSG) -> isize {
        if lpMsg.is_null() {
            return 0;
        }

        let msg = &*lpMsg;
        def_window_proc_a(msg.hwnd, msg.message, msg.wParam, msg.lParam)
    }
    
    /// PostQuitMessage - WM_QUIT mesajini kuyruga ekler; mesaj dongusu bunu gorununce sonlanir
    pub unsafe fn post_quit_message(nExitCode: INT) {
        crate::serial_println!("[WIN32] PostQuitMessage({})", nExitCode);
        enqueue_message(0, WM_QUIT, nExitCode as usize, 0);
    }
    
    /// DefWindowProcA - Uygulama tarafindan islenmemis pencere mesajlari icin varsayilan isleme saglar
    pub unsafe fn def_window_proc_a(
        hWnd: HWND,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
    ) -> isize {
        match Msg {
            WM_CLOSE => {
                enqueue_message(hWnd, WM_DESTROY, 0, 0);
                0
            }
            WM_DESTROY => {
                enqueue_message(0, WM_QUIT, 0, 0);
                enqueue_message(hWnd, WM_NCDESTROY, 0, 0);
                0
            }
            WM_SETTEXT => {
                if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
                    let title = cstr_to_string(lParam as LPCSTR);
                    if let Some(window) = WINDOW_REGISTRY.lock().get_mut(&hWnd) {
                        window.title = title;
                    }
                    enqueue_message(hWnd, WM_PAINT, 0, 0);
                    return 1;
                }
                0
            }
            WM_GETTEXTLENGTH => WINDOW_REGISTRY
                .lock()
                .get(&hWnd)
                .map(|window| window.title.len() as isize)
                .unwrap_or(0),
            WM_GETTEXT => {
                let max_count = wParam as INT;
                let dst = lParam as LPSTR;
                let windows = WINDOW_REGISTRY.lock();
                let Some(window) = windows.get(&hWnd) else {
                    return 0;
                };
                copy_cstr(dst, max_count, &window.title) as isize
            }
            _ => 0,
        }
    }
    
    /// GetDC - Pencere icin cizim yapilabilecek bir cihaz baglami (DC) tanitici dondurur
    pub unsafe fn get_dc(hWnd: HWND) -> HDC {
        hWnd as HDC
    }
    
    /// ReleaseDC - GetDC ile alinan DC taniticiyi serbest birakar
    pub unsafe fn release_dc(hWnd: HWND, hDC: HDC) -> INT {
        1
    }
    
    /// SetWindowTextA - Pencerenin baslik cubugu metnini degistirir ve WM_PAINT gonderir
    pub unsafe fn set_window_text_a(hWnd: HWND, lpString: LPCSTR) -> BOOL {
        let title = cstr_to_string(lpString);
        let mut windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get_mut(&hWnd) else {
            return FALSE;
        };
        window.title = title.clone();
        enqueue_message(hWnd, WM_PAINT, 0, 0);
        crate::serial_println!("[WIN32] SetWindowTextA: {}", title);
        TRUE
    }
    
    /// GetClientRect - Pencerenin istemci alaninin dikdortgenini lpRect'e yazar (sifirdan baslar)
    pub unsafe fn get_client_rect(hWnd: HWND, lpRect: *mut RECT) -> BOOL {
        if lpRect.is_null() {
            return FALSE;
        }
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return FALSE;
        };
        (*lpRect).left = 0;
        (*lpRect).top = 0;
        (*lpRect).right = window.width.max(0);
        (*lpRect).bottom = window.height.max(0);
        TRUE
    }
    
    // ========================================================================
    // PENCERE YONETIMI (pencere yasam dongusu, konumlandirma ve hiyerarsi)
    // ========================================================================
    
    /// DestroyWindow - Pencereyi ve alt pencerelerini yok eder; WM_DESTROY gonderir
    pub unsafe fn destroy_window(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY.lock().remove(&hWnd).is_none() {
            return FALSE;
        }

        WINDOW_MENUS.lock().remove(&hWnd);
        DIALOG_REGISTRY.lock().remove(&hWnd);
        TIMER_REGISTRY.lock().retain(|(timer_hwnd, _), _| *timer_hwnd != hWnd);

        if FOREGROUND_WINDOW.load(Ordering::Relaxed) == hWnd {
            FOREGROUND_WINDOW.store(0, Ordering::Relaxed);
        }
        if ACTIVE_WINDOW.load(Ordering::Relaxed) == hWnd {
            ACTIVE_WINDOW.store(0, Ordering::Relaxed);
        }
        if FOCUS_WINDOW.load(Ordering::Relaxed) == hWnd {
            FOCUS_WINDOW.store(0, Ordering::Relaxed);
        }
        if CAPTURE_WINDOW.load(Ordering::Relaxed) == hWnd {
            CAPTURE_WINDOW.store(0, Ordering::Relaxed);
        }

        enqueue_message(hWnd, WM_DESTROY, 0, 0);
        crate::serial_println!("[WIN32] DestroyWindow: {}", hWnd);
        TRUE
    }
    
    /// IsWindow - Tanitici gecerli bir pencereye ait olup olmadigini sinar
    pub unsafe fn is_window(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) { TRUE } else { FALSE }
    }
    
    /// IsWindowVisible - Pencere ve tum ata pencereleri gorunurse TRUE doner
    pub unsafe fn is_window_visible(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY
            .lock()
            .get(&hWnd)
            .map(|window| window.visible)
            .unwrap_or(false)
        {
            TRUE
        } else {
            FALSE
        }
    }
    
    /// IsWindowEnabled - Pencerenin fare ve klavye girisini kabul edip etmedigini dondurur
    pub unsafe fn is_window_enabled(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY
            .lock()
            .get(&hWnd)
            .map(|window| window.enabled)
            .unwrap_or(false)
        {
            TRUE
        } else {
            FALSE
        }
    }
    
    /// EnableWindow - Pencerenin giris almasini etkinlestirir ya da devre disi birakar
    pub unsafe fn enable_window(hWnd: HWND, bEnable: BOOL) -> BOOL {
        let mut windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get_mut(&hWnd) else {
            return FALSE;
        };
        window.enabled = bEnable != 0;
        TRUE
    }
    
    /// MoveWindow - Pencerenin konum ve boyutunu degistirir; bRepaint TRUE ise yeniden cizer
    pub unsafe fn move_window(hWnd: HWND, x: INT, y: INT, nWidth: INT, nHeight: INT, bRepaint: BOOL) -> BOOL {
        let mut windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get_mut(&hWnd) else {
            return FALSE;
        };
        window.x = x;
        window.y = y;
        window.width = nWidth;
        window.height = nHeight;
        let size_lparam = ((nHeight as u32 as usize) << 16) | (nWidth as u32 as usize & 0xFFFF);
        enqueue_message(hWnd, WM_SIZE, 0, size_lparam as isize);
        if bRepaint != 0 {
            enqueue_message(hWnd, WM_PAINT, 0, 0);
        }
        crate::serial_println!("[WIN32] MoveWindow: {},{} {},{}", x, y, nWidth, nHeight);
        TRUE
    }
    
    /// SetWindowPos - Pencerenin z-sirasini, konumunu ve boyutunu tek cagriyla degistirir
    pub unsafe fn set_window_pos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        x: INT,
        y: INT,
        cx: INT,
        cy: INT,
        uFlags: UINT,
    ) -> BOOL {
        let _ = (hWndInsertAfter, uFlags);
        move_window(hWnd, x, y, cx, cy, TRUE)
    }
    
    /// GetWindowRect - Ekran koordinatlarinda pencerenin sinir dikdortgenini dondurur
    pub unsafe fn get_window_rect(hWnd: HWND, lpRect: *mut RECT) -> BOOL {
        if lpRect.is_null() {
            return FALSE;
        }
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return FALSE;
        };
        (*lpRect).left = window.x;
        (*lpRect).top = window.y;
        (*lpRect).right = window.x.saturating_add(window.width);
        (*lpRect).bottom = window.y.saturating_add(window.height);
        TRUE
    }
    
    /// GetWindowTextA - Pencerenin baslik cubugu metnini lpString tamponuna kopyalar
    pub unsafe fn get_window_text_a(hWnd: HWND, lpString: LPSTR, nMaxCount: INT) -> INT {
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return 0;
        };
        copy_cstr(lpString, nMaxCount, &window.title)
    }
    
    /// GetWindowTextLengthA - Baslik cubugu metninin karakter sayisini dondurur
    pub unsafe fn get_window_text_length_a(hWnd: HWND) -> INT {
        let windows = WINDOW_REGISTRY.lock();
        windows
            .get(&hWnd)
            .map(|window| window.title.len() as INT)
            .unwrap_or(0)
    }
    
    /// GetParent - Pencere hiyerarsisinde pencerenin ebeveyn taniticisinI dondurur
    pub unsafe fn get_parent(hWnd: HWND) -> HWND {
        WINDOW_REGISTRY
            .lock()
            .get(&hWnd)
            .map(|window| window.parent)
            .unwrap_or(0)
    }
    
    /// SetParent - Pencereyi farkli bir ebeveyn pencereye tasir; eski ebeveynI dondurur
    pub unsafe fn set_parent(hWndChild: HWND, hWndNewParent: HWND) -> HWND {
        let mut windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get_mut(&hWndChild) else {
            return 0;
        };
        let old = window.parent;
        window.parent = hWndNewParent;
        old
    }
    
    /// GetDesktopWindow - Masaustu (kok) penceresinin taniticisinI dondurur
    pub unsafe fn get_desktop_window() -> HWND {
        0xFFFFFFFF as HWND
    }
    
    /// GetForegroundWindow - Kullanicinin etkilesimde oldugu on plan penceresini dondurur
    pub unsafe fn get_foreground_window() -> HWND {
        FOREGROUND_WINDOW.load(Ordering::Relaxed)
    }
    
    /// SetForegroundWindow - Belirtilen pencereyi on plana alir ve etkin odak verir
    pub unsafe fn set_foreground_window(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            FOREGROUND_WINDOW.store(hWnd, Ordering::Relaxed);
            ACTIVE_WINDOW.store(hWnd, Ordering::Relaxed);
            return TRUE;
        }
        FALSE
    }

    /// GetActiveWindow - Gecerli is parcacigi icin etkin pencere taniticisinI dondurur
    pub unsafe fn get_active_window() -> HWND {
        ACTIVE_WINDOW.load(Ordering::Relaxed)
    }

    /// SetActiveWindow - Pencereyi etkinlestirir; eski etkin pencereyi dondurur
    pub unsafe fn set_active_window(hWnd: HWND) -> HWND {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            ACTIVE_WINDOW.store(hWnd, Ordering::Relaxed);
            return hWnd;
        }
        0
    }

    /// GetFocus - Gecerli is parcaciginda klavye odagina sahip pencereyi dondurur
    pub unsafe fn get_focus() -> HWND {
        FOCUS_WINDOW.load(Ordering::Relaxed)
    }

    /// SetFocus - Klavye odagini belirtilen pencereye verir; eski odak penceresini dondurur
    pub unsafe fn set_focus(hWnd: HWND) -> HWND {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            let old = FOCUS_WINDOW.load(Ordering::Relaxed);
            FOCUS_WINDOW.store(hWnd, Ordering::Relaxed);
            return old;
        }
        0
    }

    /// GetCapture - Fare mesajlarini yakalayan pencere taniticisinI dondurur
    pub unsafe fn get_capture() -> HWND {
        CAPTURE_WINDOW.load(Ordering::Relaxed)
    }

    /// SetCapture - Fare mesajlarini belirtilen pencereye yonlendirir; eski yakalayiciyi dondurur
    pub unsafe fn set_capture(hWnd: HWND) -> HWND {
        let old = CAPTURE_WINDOW.load(Ordering::Relaxed);
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            CAPTURE_WINDOW.store(hWnd, Ordering::Relaxed);
        }
        old
    }

    /// ReleaseCapture - SetCapture ile ayarlanmis fare yakalamasini kaldirir
    pub unsafe fn release_capture() -> BOOL {
        CAPTURE_WINDOW.store(0, Ordering::Relaxed);
        TRUE
    }
    
    /// FindWindowA - Sinif adi veya basliga gore pencere arayanI dondurur
    pub unsafe fn find_window_a(lpClassName: LPCSTR, lpWindowName: LPCSTR) -> HWND {
        let class_name = cstr_to_string(lpClassName);
        let window_name = cstr_to_string(lpWindowName);
        let windows = WINDOW_REGISTRY.lock();
        windows
            .iter()
            .find_map(|(hwnd, window)| {
                let class_ok = class_name.is_empty() || window.class_name == class_name;
                let title_ok = window_name.is_empty() || window.title == window_name;
                if class_ok && title_ok {
                    Some(*hwnd)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }
    
    /// FindWindowExA - Belirli bir ebeveyn altinda, onceki pencereden sonra esleyen pencereyi arar
    pub unsafe fn find_window_ex_a(
        hWndParent: HWND,
        hWndChildAfter: HWND,
        lpszClass: LPCSTR,
        lpszWindow: LPCSTR,
    ) -> HWND {
        let class_name = cstr_to_string(lpszClass);
        let window_name = cstr_to_string(lpszWindow);
        let windows = WINDOW_REGISTRY.lock();
        let mut seen_after = hWndChildAfter == 0;

        for (hwnd, window) in windows.iter() {
            if !seen_after {
                if *hwnd == hWndChildAfter {
                    seen_after = true;
                }
                continue;
            }

            if hWndParent != 0 && window.parent != hWndParent {
                continue;
            }
            if !class_name.is_empty() && window.class_name != class_name {
                continue;
            }
            if !window_name.is_empty() && window.title != window_name {
                continue;
            }
            return *hwnd;
        }

        0
    }
    
    /// GetWindow - Z-sirasi veya iliskiye gore pencereyi dondurur (GW_ sabiti kullanilir)
    pub unsafe fn get_window(hWnd: HWND, uCmd: UINT) -> HWND {
        if hWnd == 0 {
            return 0;
        }

        match uCmd {
            GW_HWNDFIRST => ordered_windows_by_parent(0).first().copied().unwrap_or(0),
            GW_HWNDLAST => ordered_windows_by_parent(0).last().copied().unwrap_or(0),
            GW_CHILD => ordered_windows_by_parent(hWnd).first().copied().unwrap_or(0),
            GW_OWNER => 0,
            GW_ENABLEDPOPUP => {
                let handles = ordered_windows_by_parent(0);
                handles
                    .iter()
                    .rev()
                    .find(|candidate| {
                        WINDOW_REGISTRY
                            .lock()
                            .get(candidate)
                            .map(|window| window.enabled && window.visible)
                            .unwrap_or(false)
                    })
                    .copied()
                    .unwrap_or(0)
            }
            GW_HWNDNEXT | GW_HWNDPREV => {
                let parent = WINDOW_REGISTRY
                    .lock()
                    .get(&hWnd)
                    .map(|window| window.parent)
                    .unwrap_or(0);
                let siblings = ordered_windows_by_parent(parent);
                let Some(pos) = siblings.iter().position(|candidate| *candidate == hWnd) else {
                    return 0;
                };

                if uCmd == GW_HWNDNEXT {
                    siblings.get(pos + 1).copied().unwrap_or(0)
                } else if pos > 0 {
                    siblings.get(pos - 1).copied().unwrap_or(0)
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
    
    /// EnumWindows - Tum ust-duzey pencereleri listeleyerek geri cagrimi cagirIr
    pub unsafe fn enum_windows(lpEnumFunc: Option<unsafe extern "system" fn(HWND, usize) -> BOOL>, lParam: usize) -> BOOL {
        let Some(callback) = lpEnumFunc else {
            return FALSE;
        };
        let handles = ordered_windows_by_parent(0);
        for hwnd in handles {
            if callback(hwnd, lParam) == FALSE {
                return FALSE;
            }
        }
        TRUE
    }

    /// EnumChildWindows - Bir ebeveyne ait alt pencereleri listeleyerek geri cagrimi cagirir
    pub unsafe fn enum_child_windows(hWndParent: HWND, lpEnumFunc: Option<unsafe extern "system" fn(HWND, usize) -> BOOL>, lParam: usize) -> BOOL {
        let Some(callback) = lpEnumFunc else {
            return FALSE;
        };

        let children = ordered_windows_by_parent(hWndParent);
        for hwnd in children {
            if callback(hwnd, lParam) == FALSE {
                return FALSE;
            }
        }
        TRUE
    }

    /// EnumThreadWindows - Belirtilen is parcacigina ait pencereleri listeleyerek geri cagrimi cagirir
    pub unsafe fn enum_thread_windows(dwThreadId: DWORD, lpfn: Option<unsafe extern "system" fn(HWND, usize) -> BOOL>, lParam: usize) -> BOOL {
        let Some(callback) = lpfn else {
            return FALSE;
        };

        let handles: Vec<HWND> = WINDOW_REGISTRY
            .lock()
            .iter()
            .filter_map(|(hwnd, state)| if state.owner_thread_id == dwThreadId { Some(*hwnd) } else { None })
            .collect();

        for hwnd in handles {
            if callback(hwnd, lParam) == FALSE {
                return FALSE;
            }
        }
        TRUE
    }
    
    /// GetClassNameA - Pencerenin ait oldugu sinifin adini lpClassName tamponuna yazar
    pub unsafe fn get_class_name_a(hWnd: HWND, lpClassName: LPSTR, nMaxCount: INT) -> INT {
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return 0;
        };
        copy_cstr(lpClassName, nMaxCount, &window.class_name)
    }
    
    // ========================================================================
    // MESAJ YONETIMI (mesaj kuyrugu, gonderim ve alma mekanizmalari)
    // ========================================================================
    
    /// PeekMessageA - Mesaj kuyrugundan mesaj alir; bRemove FALSE ise kuyrukta birakar
    pub unsafe fn peek_message_a(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: UINT,
        wMsgFilterMax: UINT,
        wRemoveMsg: UINT,
    ) -> BOOL {
        if lpMsg.is_null() {
            return FALSE;
        }

        let remove = (wRemoveMsg & PM_REMOVE) != 0;
        let Some(msg) = dequeue_matching_message(hWnd, wMsgFilterMin, wMsgFilterMax, remove) else {
            return FALSE;
        };
        *lpMsg = msg;
        TRUE
    }
    
    /// PostMessageA - Mesaji hedef pencerenin kuyrugunaaas ekler, beklemeden geri doner
    pub unsafe fn post_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        if hWnd == HWND_BROADCAST {
            let targets = ordered_windows_by_parent(0);
            for target in targets {
                enqueue_message(target, Msg, wParam, lParam);
            }
            return TRUE;
        }

        if hWnd != 0 && !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return FALSE;
        }

        enqueue_message(hWnd, Msg, wParam, lParam);
        TRUE
    }
    
    /// SendMessageA - Mesaji hedef pencerenin yordamina dogrudan iletir ve yaniti bekler
    pub unsafe fn send_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> isize {
        if hWnd == HWND_BROADCAST {
            let targets = ordered_windows_by_parent(0);
            let mut last_result = 0isize;
            for target in targets {
                last_result = send_message_a(target, Msg, wParam, lParam);
            }
            return last_result;
        }

        if hWnd != 0 && !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return 0;
        }

        let msg = MSG {
            hwnd: hWnd,
            message: Msg,
            wParam,
            lParam,
            time: crate::task::scheduler::get_ticks() as DWORD,
            pt: POINT { x: 0, y: 0 },
        };
        dispatch_message_a(&msg)
    }
    
    /// SendMessageTimeoutA - Mesaji hedef pencereye gonderir; zaman asiminda hata dondurur
    pub unsafe fn send_message_timeout_a(
        hWnd: HWND,
        Msg: UINT,
        wParam: usize,
        lParam: isize,
        fuFlags: UINT,
        uTimeout: UINT,
        lpdwResult: *mut usize,
    ) -> isize {
        let _ = fuFlags;
        if hWnd != HWND_BROADCAST && hWnd != 0 && !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            if !lpdwResult.is_null() {
                *lpdwResult = 0;
            }
            return 0;
        }

        if uTimeout > 0 {
            crate::task::scheduler::sleep(core::cmp::max(1usize, (uTimeout as usize) / 10));
        }

        let result = if hWnd == HWND_BROADCAST {
            let targets = ordered_windows_by_parent(0);
            let mut last_result = 0isize;
            for target in targets {
                last_result = send_message_a(target, Msg, wParam, lParam);
            }
            last_result
        } else {
            send_message_a(hWnd, Msg, wParam, lParam)
        };

        if !lpdwResult.is_null() {
            *lpdwResult = result as usize;
        }
        1
    }
    
    /// SendNotifyMessageA - Mesaji asenkron olarak gonderir; WinProc cevaplayana kadar beklemez
    pub unsafe fn send_notify_message_a(hWnd: HWND, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        enqueue_message(hWnd, Msg, wParam, lParam);
        TRUE
    }
    
    /// PostThreadMessageA - Belirli bir is parcaciginin mesaj kuyrugunaaasenkron mesaj ekler
    pub unsafe fn post_thread_message_a(idThread: DWORD, Msg: UINT, wParam: usize, lParam: isize) -> BOOL {
        let _ = idThread;
        enqueue_message(0, Msg, wParam, lParam);
        TRUE
    }
    
    /// ReplyMessage - SendMessage bekleyeni serbest birakir; WinProc icinden cagrilir
    pub unsafe fn reply_message(lResult: isize) -> BOOL {
        TRUE
    }
    
    /// GetMessageTime - Son islenen mesajin gonderilme zamanini milisaniye cinsinden dondurur
    pub unsafe fn get_message_time() -> LONG {
        LAST_MESSAGE_TIME.load(Ordering::Relaxed) as LONG
    }
    
    /// GetMessagePos - Son islenen mesajin fare konumunu paketlenmis koordinat olarak dondurur
    pub unsafe fn get_message_pos() -> DWORD {
        LAST_MESSAGE_POS.load(Ordering::Relaxed)
    }
    
    /// WaitMessage - Mesaj kuyrugundan mesaj gelene kadar is parcacigini bekletir
    pub unsafe fn wait_message() -> BOOL {
        for _ in 0..256 {
            if !MESSAGE_QUEUE.lock().is_empty() {
                return TRUE;
            }
            pump_timers(10);
            crate::task::scheduler::sleep(1);
        }
        FALSE
    }
    
    /// MsgWaitForMultipleObjects - Nesne veya mesaj gelene kadar is parcacigini bekletir
    pub unsafe fn msg_wait_for_multiple_objects(
        nCount: DWORD,
        pHandles: *const HANDLE,
        fWaitAll: BOOL,
        dwMilliseconds: DWORD,
        dwWakeMask: DWORD,
    ) -> DWORD {
        let _ = (pHandles, fWaitAll, dwWakeMask);
        if !MESSAGE_QUEUE.lock().is_empty() {
            return WAIT_OBJECT_0 + nCount;
        }
        if dwMilliseconds == 0 {
            return WAIT_TIMEOUT;
        }

        let mut waits = core::cmp::max(1usize, (dwMilliseconds as usize) / 10);
        if waits > 1024 {
            waits = 1024;
        }
        for _ in 0..waits {
            if !MESSAGE_QUEUE.lock().is_empty() {
                return WAIT_OBJECT_0 + nCount;
            }
            pump_timers(10);
            crate::task::scheduler::sleep(1);
        }

        WAIT_TIMEOUT
    }
    
    /// RegisterWindowMessageA - Benzersiz mesaj kimlik numarasi uretir; tum uygulamalar paylasir
    pub unsafe fn register_window_message_a(lpString: LPCSTR) -> UINT {
        let name = cstr_to_string(lpString);
        if name.is_empty() {
            return 0;
        }

        let mut registry = REGISTERED_MESSAGES.lock();
        if let Some(id) = registry.get(&name) {
            return *id;
        }

        let id = NEXT_REGISTERED_MESSAGE.fetch_add(1, Ordering::Relaxed);
        registry.insert(name, id);
        id
    }
    
    // ========================================================================
    // GIRIS - KLAVYE (klavye durumu sorgulama ve sanal tus esleme islemleri)
    // ========================================================================
    
    /// GetKeyState - Belirtilen sanal tusun son mesaj isleme sirasindaki durumunu dondurur
    pub unsafe fn get_key_state(nVirtKey: INT) -> SHORT {
        if nVirtKey < 0 || nVirtKey >= 256 {
            return 0;
        }
        let state = KEYBOARD_STATE.lock()[nVirtKey as usize];
        if state & 0x80 != 0 { 0x8000u16 as SHORT } else { 0 }
    }
    
    /// GetAsyncKeyState - Tusun su anki fiziksel durumunu dogrudan sorgular (kuyruk gerekmiyor)
    pub unsafe fn get_async_key_state(vKey: INT) -> SHORT {
        get_key_state(vKey)
    }
    
    /// GetKeyboardState - 256 sanal tusun tamaminin durumunu bir diziye yazar
    pub unsafe fn get_keyboard_state(lpKeyState: *mut BYTE) -> BOOL {
        if !lpKeyState.is_null() {
            let state = KEYBOARD_STATE.lock();
            for i in 0..256 {
                *lpKeyState.add(i) = state[i];
            }
            return TRUE;
        }
        FALSE
    }
    
    /// SetKeyboardState - 256 sanal tusun durumunu parametre dizisiyle gunceller
    pub unsafe fn set_keyboard_state(lpKeyState: *const BYTE) -> BOOL {
        if lpKeyState.is_null() {
            return FALSE;
        }
        let mut state = KEYBOARD_STATE.lock();
        for i in 0..256 {
            state[i] = *lpKeyState.add(i);
        }
        TRUE
    }
    
    /// keybd_event - Donanim klavye olaylarini simule eder; KEYEVENTF_ bayraklari ile kontrol edilir
    pub unsafe fn keybd_event(bVk: BYTE, bScan: BYTE, dwFlags: DWORD, dwExtraInfo: usize) {
        let _ = (bScan, dwExtraInfo);
        const KEYEVENTF_KEYUP: DWORD = 0x0002;
        {
            let mut state = KEYBOARD_STATE.lock();
            state[bVk as usize] = if (dwFlags & KEYEVENTF_KEYUP) != 0 { 0 } else { 0x80 };
        }

        let target = {
            let capture = CAPTURE_WINDOW.load(Ordering::Relaxed);
            if capture != 0 {
                capture
            } else {
                let focus = FOCUS_WINDOW.load(Ordering::Relaxed);
                if focus != 0 {
                    focus
                } else {
                    ACTIVE_WINDOW.load(Ordering::Relaxed)
                }
            }
        };
        if target != 0 {
            let msg = if (dwFlags & KEYEVENTF_KEYUP) != 0 { WM_KEYUP } else { WM_KEYDOWN };
            enqueue_message(target, msg, bVk as usize, 0);
        }
    }
    
    /// MapVirtualKeyA - Sanal tus kodu ile tarama kodu veya ASCII arasinda donusum yapar
    pub unsafe fn map_virtual_key_a(uCode: UINT, uMapType: UINT) -> UINT {
        let _ = uMapType;
        uCode
    }
    
    /// MapVirtualKeyExA - MapVirtualKeyA ile ayni islem; klavye duzeni belirtilebilir
    pub unsafe fn map_virtual_key_ex_a(uCode: UINT, uMapType: UINT, dwhkl: usize) -> UINT {
        let _ = dwhkl;
        map_virtual_key_a(uCode, uMapType)
    }
    
    /// ToAscii - Sanal tus ve klavye durumunu ASCII karakter koduna cevirir
    pub unsafe fn to_ascii(uVirtKey: UINT, uScanCode: UINT, lpKeyState: *const BYTE, lpChar: *mut WORD, uFlags: UINT) -> INT {
        let _ = (uScanCode, lpKeyState, uFlags);
        if lpChar.is_null() {
            return 0;
        }
        if uVirtKey < 0x20 || uVirtKey > 0x7E {
            return 0;
        }
        *lpChar = uVirtKey as WORD;
        1
    }
    
    /// ToUnicode - Sanal tus ve klavye durumunu Unicode karakter dizisine cevirir
    pub unsafe fn to_unicode(wVirtKey: UINT, wScanCode: UINT, lpKeyState: *const BYTE, pwszBuff: *mut u16, cchBuff: INT, wFlags: UINT) -> INT {
        let _ = (wScanCode, lpKeyState, wFlags);
        if pwszBuff.is_null() || cchBuff <= 0 {
            return 0;
        }
        if wVirtKey < 0x20 || wVirtKey > 0x7E {
            return 0;
        }
        *pwszBuff = wVirtKey as u16;
        1
    }
    
    /// VkKeyScanA - ASCII karakterini sanal tus modifikator kombinasyonuna donusturur
    pub unsafe fn vk_key_scan_a(ch: i8) -> SHORT {
        ch as SHORT
    }
    
    /// VkKeyScanExA - VkKeyScanA ile ayni; klavye duzeni belirtilebilir
    pub unsafe fn vk_key_scan_ex_a(ch: i8, dwhkl: usize) -> SHORT {
        let _ = dwhkl;
        vk_key_scan_a(ch)
    }
    
    /// GetKeyNameTextA - Tarayici tus kodu icin insanin okuyabilecegi tus adini dondurur
    pub unsafe fn get_key_name_text_a(lParam: LONG, lpString: LPSTR, nSize: INT) -> INT {
        let _ = lParam;
        copy_cstr(lpString, nSize, "Key")
    }
    
    /// OemKeyScan - OEM karakterini OEM tarama kodu ve vardiyaya donusturur
    pub unsafe fn oem_key_scan(wOemChar: WORD) -> DWORD {
        wOemChar as DWORD
    }
    
    // ========================================================================
    // GIRIS - FARE (fare durumu sorgulama ve takas islemleri)
    // ========================================================================
    
    /// GetCursorPos - Farenin ekrandaki mevcut koordinatlarini lpPoint'e yazar
    pub unsafe fn get_cursor_pos(lpPoint: *mut POINT) -> BOOL {
        if !lpPoint.is_null() {
            (*lpPoint).x = CURSOR_X.load(Ordering::Relaxed);
            (*lpPoint).y = CURSOR_Y.load(Ordering::Relaxed);
            return TRUE;
        }
        FALSE
    }
    
    /// SetCursorPos - Fare imlecini belirtilen ekran koordinatina tasir
    pub unsafe fn set_cursor_pos(x: INT, y: INT) -> BOOL {
        CURSOR_X.store(x, Ordering::Relaxed);
        CURSOR_Y.store(y, Ordering::Relaxed);
        let target = {
            let capture = CAPTURE_WINDOW.load(Ordering::Relaxed);
            if capture != 0 {
                capture
            } else {
                let focus = FOCUS_WINDOW.load(Ordering::Relaxed);
                if focus != 0 {
                    focus
                } else {
                    FOREGROUND_WINDOW.load(Ordering::Relaxed)
                }
            }
        };
        if target != 0 {
            let packed = ((y as u32 as usize) << 16) | (x as u32 as usize & 0xFFFF);
            enqueue_message(target, WM_MOUSEMOVE, 0, packed as isize);
        }
        crate::serial_println!("[WIN32] SetCursorPos: {},{}", x, y);
        TRUE
    }
    
    /// mouse_event - Fare olaylarini yazilimsal olarak simule eder (MOUSEEVENTF_ bayraklari)
    pub unsafe fn mouse_event(dwFlags: DWORD, dx: DWORD, dy: DWORD, cButtons: DWORD, dwExtraInfo: usize) {
        let _ = (cButtons, dwExtraInfo);
        const MOUSEEVENTF_MOVE: DWORD = 0x0001;
        const MOUSEEVENTF_LEFTDOWN: DWORD = 0x0002;
        const MOUSEEVENTF_LEFTUP: DWORD = 0x0004;
        const MOUSEEVENTF_RIGHTDOWN: DWORD = 0x0008;
        const MOUSEEVENTF_RIGHTUP: DWORD = 0x0010;

        if (dwFlags & MOUSEEVENTF_MOVE) != 0 {
            let _ = set_cursor_pos(dx as INT, dy as INT);
        }

        let target = {
            let capture = CAPTURE_WINDOW.load(Ordering::Relaxed);
            if capture != 0 {
                capture
            } else {
                let focus = FOCUS_WINDOW.load(Ordering::Relaxed);
                if focus != 0 {
                    focus
                } else {
                    FOREGROUND_WINDOW.load(Ordering::Relaxed)
                }
            }
        };
        if target == 0 {
            return;
        }

        let x = CURSOR_X.load(Ordering::Relaxed);
        let y = CURSOR_Y.load(Ordering::Relaxed);
        let packed = ((y as u32 as usize) << 16) | (x as u32 as usize & 0xFFFF);
        if (dwFlags & MOUSEEVENTF_LEFTDOWN) != 0 {
            enqueue_message(target, WM_LBUTTONDOWN, 1, packed as isize);
        }
        if (dwFlags & MOUSEEVENTF_LEFTUP) != 0 {
            enqueue_message(target, WM_LBUTTONUP, 0, packed as isize);
        }
        if (dwFlags & MOUSEEVENTF_RIGHTDOWN) != 0 {
            enqueue_message(target, WM_LBUTTONDOWN, 2, packed as isize);
        }
        if (dwFlags & MOUSEEVENTF_RIGHTUP) != 0 {
            enqueue_message(target, WM_LBUTTONUP, 2, packed as isize);
        }
    }
    
    /// GetDoubleClickTime - Cift tiklamayi olusturan maksimum milisaniye araligini dondurur
    pub unsafe fn get_double_click_time() -> UINT {
        DOUBLE_CLICK_TIME_MS.load(Ordering::Relaxed)
    }
    
    /// SetDoubleClickTime - Cift tiklamayi teshis eden zaman araligini milisaniye cinsinden ayarlar
    pub unsafe fn set_double_click_time(uInterval: UINT) -> BOOL {
        DOUBLE_CLICK_TIME_MS.store(uInterval.max(100), Ordering::Relaxed);
        TRUE
    }
    
    /// SwapMouseButton - Sol ve sag fare dugmesi islevlerini yer degistirir
    pub unsafe fn swap_mouse_button(fSwap: BOOL) -> BOOL {
        let old = MOUSE_BUTTON_SWAPPED.swap(fSwap != 0, Ordering::Relaxed);
        if old { TRUE } else { FALSE }
    }
    
    /// GetSystemMetrics - Pencere, ekran veya sistem ozellikleri icin metrik degerleri dondurur
    pub unsafe fn get_system_metrics(nIndex: INT) -> INT {
        let screen_w = 1024;
        let screen_h = 768;
        match nIndex {
            0 => screen_w,     // SM_CXSCREEN
            1 => screen_h,     // SM_CYSCREEN
            2 => 16,           // SM_CXVSCROLL
            3 => 16,           // SM_CYHSCROLL
            4 => 32,           // SM_CYCAPTION
            5 => 32,           // SM_CXBORDER
            6 => 32,           // SM_CYBORDER
            7 => 16,           // SM_CXDLGFRAME
            8 => 16,           // SM_CYDLGFRAME
            11 => 32,          // SM_CYVTHUMB
            12 => 32,          // SM_CXHTHUMB
            76 => if MOUSE_BUTTON_SWAPPED.load(Ordering::Relaxed) { 1 } else { 0 }, // SM_SWAPBUTTON
            _ => 0,
        }
    }
    
    // ========================================================================
    // MENULAR (menular olusturma, doldurma ve gosterme islemleri)
    // ========================================================================
    
    /// CreateMenu - Bos bir menu olusturur; AppendMenuA ile ogeler eklenir
    pub unsafe fn create_menu() -> HMENU {
        let handle = NEXT_MENU_HANDLE.fetch_add(1, Ordering::Relaxed);
        MENU_REGISTRY.lock().insert(
            handle,
            MenuState {
                popup: false,
                items: Vec::new(),
            },
        );
        handle
    }
    
    /// CreatePopupMenu - Acilir (popup) menu olusturur; TrackPopupMenu ile gosterilir
    pub unsafe fn create_popup_menu() -> HMENU {
        let handle = NEXT_MENU_HANDLE.fetch_add(1, Ordering::Relaxed);
        MENU_REGISTRY.lock().insert(
            handle,
            MenuState {
                popup: true,
                items: Vec::new(),
            },
        );
        handle
    }
    
    /// DestroyMenu - Menu taniticiyi ve tum alt ogelerini serbest birakar
    pub unsafe fn destroy_menu(hMenu: HMENU) -> BOOL {
        if MENU_REGISTRY.lock().remove(&hMenu).is_none() {
            return FALSE;
        }
        WINDOW_MENUS.lock().retain(|_, menu| *menu != hMenu);
        TRUE
    }
    
    /// AppendMenuA - Menunun sonuna yeni bir oge ekler (metin, ayirici veya alt menu)
    pub unsafe fn append_menu_a(hMenu: HMENU, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMenu) else {
            return FALSE;
        };
        menu.items.push(MenuItemState {
            id: uIDNewItem,
            text: cstr_to_string(lpNewItem),
            enabled: (uFlags & MF_DISABLED) == 0,
            checked: (uFlags & MF_CHECKED) != 0,
        });
        TRUE
    }
    
    /// InsertMenuA - Belirtilen konuma menu ogesi ekler; mevcut ogeleri kaydirIr
    pub unsafe fn insert_menu_a(hMenu: HMENU, uPosition: UINT, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMenu) else {
            return FALSE;
        };
        let idx = (uPosition as usize).min(menu.items.len());
        menu.items.insert(
            idx,
            MenuItemState {
                id: uIDNewItem,
                text: cstr_to_string(lpNewItem),
                enabled: (uFlags & MF_DISABLED) == 0,
                checked: (uFlags & MF_CHECKED) != 0,
            },
        );
        TRUE
    }
    
    /// ModifyMenuA - Mevcut bir menu ogesinin metin veya davranisini gunceller
    pub unsafe fn modify_menu_a(hMnu: HMENU, uPosition: UINT, uFlags: UINT, uIDNewItem: usize, lpNewItem: LPCSTR) -> BOOL {
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMnu) else {
            return FALSE;
        };
        let by_position = (uFlags & MF_BYPOSITION) != 0;
        let item_opt = if by_position {
            menu.items.get_mut(uPosition as usize)
        } else {
            menu.items.iter_mut().find(|item| item.id == uPosition as usize)
        };

        let Some(item) = item_opt else {
            return FALSE;
        };
        item.id = uIDNewItem;
        item.text = cstr_to_string(lpNewItem);
        item.enabled = (uFlags & MF_DISABLED) == 0;
        item.checked = (uFlags & MF_CHECKED) != 0;
        TRUE
    }
    
    /// RemoveMenu - Menüden belirtilen öğeyi kaldırır; alt menüyü yok etmez
    pub unsafe fn remove_menu(hMenu: HMENU, uPosition: UINT, uFlags: UINT) -> BOOL {
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMenu) else {
            return FALSE;
        };
        let by_position = (uFlags & MF_BYPOSITION) != 0;
        let idx_opt = if by_position {
            if (uPosition as usize) < menu.items.len() {
                Some(uPosition as usize)
            } else {
                None
            }
        } else {
            menu.items.iter().position(|item| item.id == uPosition as usize)
        };

        if let Some(idx) = idx_opt {
            menu.items.remove(idx);
            TRUE
        } else {
            FALSE
        }
    }
    
    /// DeleteMenu - Menuden bir ogeyi ve varsa alt menuyu kalicilikla siler
    pub unsafe fn delete_menu(hMenu: HMENU, uPosition: UINT, uFlags: UINT) -> BOOL {
        remove_menu(hMenu, uPosition, uFlags)
    }
    
    /// SetMenu - Pencereye bir menu cubugu atar; NULL gecilirse mevcut menu kaldirilir
    pub unsafe fn set_menu(hWnd: HWND, hMenu: HMENU) -> BOOL {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return FALSE;
        }
        if hMenu != 0 && !MENU_REGISTRY.lock().contains_key(&hMenu) {
            return FALSE;
        }
        if hMenu == 0 {
            WINDOW_MENUS.lock().remove(&hWnd);
        } else {
            WINDOW_MENUS.lock().insert(hWnd, hMenu);
        }
        TRUE
    }
    
    /// GetMenu - Pencerenin menu cubugu taniticisinI dondurur; yoksa NULL
    pub unsafe fn get_menu(hWnd: HWND) -> HMENU {
        WINDOW_MENUS.lock().get(&hWnd).copied().unwrap_or(0)
    }
    
    /// DrawMenuBar - Pencerenin menu cubugunun yeniden cizilmesini zorlar
    pub unsafe fn draw_menu_bar(hWnd: HWND) -> BOOL {
        if WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            enqueue_message(hWnd, WM_PAINT, 0, 0);
            return TRUE;
        }
        FALSE
    }

    fn first_enabled_menu_item(hMenu: HMENU) -> Option<MenuItemState> {
        MENU_REGISTRY
            .lock()
            .get(&hMenu)
            .and_then(|menu| menu.items.iter().find(|item| item.enabled).cloned())
    }

    /// TrackPopupMenu - Belirtilen konumda popup menuyu gosterir ve secimi WM_COMMAND olarak gonderir
    pub unsafe fn track_popup_menu(
        hMenu: HMENU,
        uFlags: UINT,
        x: INT,
        y: INT,
        nReserved: INT,
        hWnd: HWND,
        prcRect: *const RECT,
    ) -> BOOL {
        let _ = (uFlags, x, y, nReserved, prcRect);
        if hWnd != 0 && !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return FALSE;
        }
        let Some(item) = first_enabled_menu_item(hMenu) else {
            return FALSE;
        };
        if hWnd != 0 {
            enqueue_message(hWnd, WM_COMMAND, item.id, 0);
        }
        TRUE
    }
    
    /// GetMenuItemCount - Menudeki toplam oge sayisini dondurur; hatada -1 doner
    pub unsafe fn get_menu_item_count(hMenu: HMENU) -> INT {
        MENU_REGISTRY
            .lock()
            .get(&hMenu)
            .map(|menu| menu.items.len() as INT)
            .unwrap_or(-1)
    }
    
    /// GetMenuItemID - Konumuyla belirtilen menu ogesinin komut kimligini dondurur
    pub unsafe fn get_menu_item_id(hMenu: HMENU, nPos: INT) -> UINT {
        MENU_REGISTRY
            .lock()
            .get(&hMenu)
            .and_then(|menu| menu.items.get(nPos.max(0) as usize))
            .map(|item| item.id as UINT)
            .unwrap_or(0xFFFF_FFFF)
    }
    
    /// GetMenuStringA - Menu ogesinin metnini lpString tamponuna kopyalar
    pub unsafe fn get_menu_string_a(hMenu: HMENU, uItem: UINT, lpString: LPSTR, nMaxCount: INT, uFlag: UINT) -> INT {
        let by_position = (uFlag & MF_BYPOSITION) != 0;
        let menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get(&hMenu) else {
            return 0;
        };
        let item_opt = if by_position {
            menu.items.get(uItem as usize)
        } else {
            menu.items.iter().find(|item| item.id == uItem as usize)
        };
        let Some(item) = item_opt else {
            return 0;
        };
        copy_cstr(lpString, nMaxCount, &item.text)
    }
    
    /// CheckMenuItem - Menu ogesinin isaretli/isaretli-degil durumunu degistirir
    pub unsafe fn check_menu_item(hMenu: HMENU, uIDCheckItem: UINT, uCheck: UINT) -> DWORD {
        let by_position = (uCheck & MF_BYPOSITION) != 0;
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMenu) else {
            return 0xFFFF_FFFF;
        };
        let item_opt = if by_position {
            menu.items.get_mut(uIDCheckItem as usize)
        } else {
            menu.items.iter_mut().find(|item| item.id == uIDCheckItem as usize)
        };
        let Some(item) = item_opt else {
            return 0xFFFF_FFFF;
        };
        let prev = if item.checked { MF_CHECKED as DWORD } else { 0 };
        item.checked = (uCheck & MF_CHECKED) != 0;
        prev
    }
    
    /// EnableMenuItem - Menu ogesini etkinlestirir veya devre disi birakar (MF_ENABLED/MF_DISABLED)
    pub unsafe fn enable_menu_item(hMenu: HMENU, uIDEnableItem: UINT, uEnable: UINT) -> BOOL {
        let by_position = (uEnable & MF_BYPOSITION) != 0;
        let mut menus = MENU_REGISTRY.lock();
        let Some(menu) = menus.get_mut(&hMenu) else {
            return FALSE;
        };
        let item_opt = if by_position {
            menu.items.get_mut(uIDEnableItem as usize)
        } else {
            menu.items.iter_mut().find(|item| item.id == uIDEnableItem as usize)
        };
        let Some(item) = item_opt else {
            return FALSE;
        };
        item.enabled = (uEnable & MF_DISABLED) == 0;
        TRUE
    }
    
    // ========================================================================
    // ILETISIM KUTULARI (modal ve modal olmayan iletisim kutusu islemleri)
    // ========================================================================
    
    /// MessageBoxA - Modal bir mesaj kutusu gosterir; MB_ bayraklariyla dugmeler secilir
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
    
    /// MessageBoxExA - MessageBoxA ile ayni; dil kimligi belirtilebilir
    pub unsafe fn message_box_ex_a(hWnd: HWND, lpText: LPCSTR, lpCaption: LPCSTR, uType: UINT, wLanguageId: WORD) -> INT {
        1
    }
    
    /// MessageBoxIndirectA - Gelismis parametreli mesaj kutusu gosterir (MSGBOXPARAMS yapisi ile)
    pub unsafe fn message_box_indirect_a(lpMsgBoxParams: *const u8) -> INT {
        1
    }
    
    /// DialogBoxParamA - Modal iletisim kutusu olusturur; dwInitParam geri cagrima iletilir
    pub unsafe fn dialog_box_param_a(
        hInstance: HINSTANCE,
        lpTemplateName: LPCSTR,
        hWndParent: HWND,
        lpDialogFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
        dwInitParam: usize,
    ) -> isize {
        let _ = (hInstance, lpTemplateName, dwInitParam);
        let dialog = create_dialog_param_a(hInstance, lpTemplateName, hWndParent, lpDialogFunc, dwInitParam);
        if dialog == 0 {
            return -1;
        }

        if let Some(proc_fn) = lpDialogFunc {
            let _ = proc_fn(dialog, WM_CREATE, 0, 0);
        }

        1
    }
    
    /// EndDialog - DialogBoxA ile baslayan modal kutuyu kapatir ve sonuc degerini belirler
    pub unsafe fn end_dialog(hDlg: HWND, nResult: isize) -> BOOL {
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return FALSE;
        };
        dialog.ended = true;
        dialog.result = nResult;
        enqueue_message(hDlg, WM_CLOSE, 0, 0);
        TRUE
    }
    
    /// CreateDialogParamA - Modeless iletisim kutusu olusturur; dwInitParam geri cagrima iletilir
    pub unsafe fn create_dialog_param_a(
        hInstance: HINSTANCE,
        lpTemplateName: LPCSTR,
        hWndParent: HWND,
        lpDialogFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, isize) -> isize>,
        dwInitParam: usize,
    ) -> HWND {
        let _ = (hInstance, lpTemplateName, lpDialogFunc, dwInitParam);
        let (owner_thread_id, owner_process_id) = infer_owner_ids();
        let hwnd = NEXT_DIALOG_HANDLE.fetch_add(1, Ordering::Relaxed);
        WINDOW_REGISTRY.lock().insert(
            hwnd,
            WindowState {
                class_name: "#32770".to_string(),
                title: "Dialog".to_string(),
                x: 100,
                y: 80,
                width: 420,
                height: 280,
                parent: hWndParent,
                visible: true,
                enabled: true,
                owner_thread_id,
                owner_process_id,
            },
        );
        DIALOG_REGISTRY.lock().insert(
            hwnd,
            DialogState {
                parent: hWndParent,
                ended: false,
                result: 0,
                text_controls: BTreeMap::new(),
                int_controls: BTreeMap::new(),
                check_controls: BTreeMap::new(),
                item_handles: BTreeMap::new(),
            },
        );
        hwnd
    }
    
    /// GetDlgItem - Iletisim kutusu icindeki bir denetimin pencere taniticisinI dondurur
    pub unsafe fn get_dlg_item(hDlg: HWND, nIDDlgItem: INT) -> HWND {
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return 0;
        };
        if let Some(hwnd) = dialog.item_handles.get(&nIDDlgItem) {
            return *hwnd;
        }
        let hwnd = NEXT_CONTROL_HANDLE.fetch_add(1, Ordering::Relaxed);
        dialog.item_handles.insert(nIDDlgItem, hwnd);
        hwnd
    }
    
    /// SetDlgItemTextA - Iletisim kutusu denetiminin metnini degistirir
    pub unsafe fn set_dlg_item_text_a(hDlg: HWND, nIDDlgItem: INT, lpString: LPCSTR) -> BOOL {
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return FALSE;
        };
        dialog.text_controls.insert(nIDDlgItem, cstr_to_string(lpString));
        TRUE
    }
    
    /// GetDlgItemTextA - Iletisim kutusu denetiminin metnini lpString tamponuna kopyalar
    pub unsafe fn get_dlg_item_text_a(hDlg: HWND, nIDDlgItem: INT, lpString: LPSTR, nMaxCount: INT) -> UINT {
        let dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get(&hDlg) else {
            return 0;
        };
        let text = dialog
            .text_controls
            .get(&nIDDlgItem)
            .cloned()
            .unwrap_or_else(String::new);
        copy_cstr(lpString, nMaxCount, &text) as UINT
    }
    
    /// SetDlgItemInt
    pub unsafe fn set_dlg_item_int(hDlg: HWND, nIDDlgItem: INT, uValue: UINT, bSigned: BOOL) -> BOOL {
        let _ = bSigned;
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return FALSE;
        };
        dialog.int_controls.insert(nIDDlgItem, uValue);
        TRUE
    }
    
    /// GetDlgItemInt
    pub unsafe fn get_dlg_item_int(hDlg: HWND, nIDDlgItem: INT, lpTranslated: *mut BOOL, bSigned: BOOL) -> UINT {
        let _ = bSigned;
        let dialogs = DIALOG_REGISTRY.lock();
        let value = dialogs
            .get(&hDlg)
            .and_then(|dialog| dialog.int_controls.get(&nIDDlgItem).copied())
            .unwrap_or(0);
        if !lpTranslated.is_null() {
            *lpTranslated = TRUE;
        }
        value
    }
    
    /// CheckDlgButton - Onay kutusu veya radio dugmesinin isaretleme durumunu ayarlar
    pub unsafe fn check_dlg_button(hDlg: HWND, nIDButton: INT, uCheck: UINT) -> BOOL {
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return FALSE;
        };
        dialog.check_controls.insert(nIDButton, uCheck);
        TRUE
    }
    
    /// CheckRadioButton - Grubun belirli radio dugmesini isaretler, digerlerini temizler
    pub unsafe fn check_radio_button(hDlg: HWND, nIDFirstButton: INT, nIDLastButton: INT, nIDCheckButton: INT) -> BOOL {
        let mut dialogs = DIALOG_REGISTRY.lock();
        let Some(dialog) = dialogs.get_mut(&hDlg) else {
            return FALSE;
        };
        for id in nIDFirstButton..=nIDLastButton {
            dialog.check_controls.insert(id, if id == nIDCheckButton { 1 } else { 0 });
        }
        TRUE
    }
    
    /// IsDlgButtonChecked - Onay kutusu veya radio dugmesinin isaretlenip isaretlenmedigini sinar
    pub unsafe fn is_dlg_button_checked(hDlg: HWND, nIDButton: INT) -> UINT {
        DIALOG_REGISTRY
            .lock()
            .get(&hDlg)
            .and_then(|dialog| dialog.check_controls.get(&nIDButton).copied())
            .unwrap_or(0)
    }
    
    // ========================================================================
    // DENETIMLER (referans icin - asagida tanimlanan islevler kullanilir)
    // ========================================================================
    
    /// CreateWindowExA - asagida tanimli fonksiyon kullanilir
    
    /// SetWindowTextA - asagida tanimli fonksiyon kullanilir
    
    /// GetWindowTextA - asagida tanimli fonksiyon kullanilir
    
    /// EnableWindow - asagida tanimli fonksiyon kullanilir
    
    /// ShowWindow - asagida tanimli fonksiyon kullanilir
    
    /// GetDlgItemInt - yukarida tanimli fonksiyon kullanilir
    
    /// SetDlgItemInt - yukarida tanimli fonksiyon kullanilir
    
    /// SendDlgItemMessageA - Iletisim kutusu denetimine dogrudan mesaj gonderir
    pub unsafe fn send_dlg_item_message_a(hDlg: HWND, nIDDlgItem: INT, Msg: UINT, wParam: usize, lParam: isize) -> isize {
        match Msg {
            WM_SETTEXT => {
                let _ = set_dlg_item_text_a(hDlg, nIDDlgItem, lParam as LPCSTR);
                1
            }
            WM_GETTEXT => get_dlg_item_text_a(hDlg, nIDDlgItem, lParam as LPSTR, wParam as INT) as isize,
            WM_GETTEXTLENGTH => DIALOG_REGISTRY
                .lock()
                .get(&hDlg)
                .and_then(|dialog| dialog.text_controls.get(&nIDDlgItem))
                .map(|text| text.len() as isize)
                .unwrap_or(0),
            _ => 0,
        }
    }
    
    /// GetNextDlgTabItem - Iletisim kutusunda sekme siralamasina gore sonraki / onceki denetimi dondurur
    pub unsafe fn get_next_dlg_tab_item(hDlg: HWND, hCtl: HWND, bPrevious: BOOL) -> HWND {
        0
    }
    
    /// GetNextDlgGroupItem - Iletisim kutusunda group box icinde bir sonraki denetimi dondurur
    pub unsafe fn get_next_dlg_group_item(hDlg: HWND, hCtl: HWND, bPrevious: BOOL) -> HWND {
        0
    }
    
    // ========================================================================
    // ZAMANLAYICILAR (WM_TIMER mesaji ile periyodik bildirim mekanizmasi)
    // ========================================================================
    
    /// SetTimer - uElapse milisaniyede bir WM_TIMER mesaji olusturacak zamanlayici kurar
    pub unsafe fn set_timer(hWnd: HWND, nIDEvent: usize, uElapse: UINT, lpTimerFunc: Option<unsafe extern "system" fn(HWND, UINT, usize, DWORD)>) -> usize {
        let _ = lpTimerFunc;
        let id = if nIDEvent == 0 {
            NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed) as usize
        } else {
            nIDEvent
        };
        TIMER_REGISTRY.lock().insert(
            (hWnd, id),
            TimerState {
                hwnd: hWnd,
                id,
                elapse_ms: uElapse.max(1),
                accum_ms: 0,
            },
        );
        crate::serial_println!("[WIN32] SetTimer: hwnd={} id={} {}ms", hWnd, id, uElapse);
        id
    }
    
    /// KillTimer - SetTimer ile kurulan zamanlayiciyi iptal eder
    pub unsafe fn kill_timer(hWnd: HWND, uIDEvent: usize) -> BOOL {
        if TIMER_REGISTRY.lock().remove(&(hWnd, uIDEvent)).is_some() {
            TRUE
        } else {
            FALSE
        }
    }
    
    /// GetTickCount - kernel32 fonksiyonu burada referans olarak listelendi
    
    // ========================================================================
    // PANO (OS genelinde metin ve veri paylasim mekanizmasi)
    // ========================================================================
    
    /// OpenClipboard - Panoyu erisme icin kilitler; kapatilana kadar baska islem erisemez
    pub unsafe fn open_clipboard(hWnd: HWND) -> BOOL {
        let mut clip = CLIPBOARD_STATE.lock();
        if let Some(owner) = clip.open_owner {
            if owner != hWnd {
                return FALSE;
            }
            return TRUE;
        }
        clip.open_owner = Some(hWnd);
        TRUE
    }
    
    /// CloseClipboard - OpenClipboard ile alinan kilidi serbest birakar
    pub unsafe fn close_clipboard() -> BOOL {
        let mut clip = CLIPBOARD_STATE.lock();
        if clip.open_owner.is_none() {
            return FALSE;
        }
        clip.open_owner = None;
        TRUE
    }
    
    /// EmptyClipboard - Panodaki tum verileri siler ve sahibi gunceller
    pub unsafe fn empty_clipboard() -> BOOL {
        let mut clip = CLIPBOARD_STATE.lock();
        if clip.open_owner.is_none() {
            return FALSE;
        }
        clip.data.clear();
        clip.owner = clip.open_owner.unwrap_or(0);
        TRUE
    }
    
    /// GetClipboardData - Belirtilen bicem (format) icin pano verisinin taniticisinI dondurur
    pub unsafe fn get_clipboard_data(uFormat: UINT) -> HANDLE {
        CLIPBOARD_STATE
            .lock()
            .data
            .get(&uFormat)
            .copied()
            .unwrap_or(0)
    }
    
    /// SetClipboardData - Panoyu belirtilen bicemdeki veri ile doldurur
    pub unsafe fn set_clipboard_data(uFormat: UINT, hMem: HANDLE) -> HANDLE {
        let mut clip = CLIPBOARD_STATE.lock();
        if clip.open_owner.is_none() {
            return 0;
        }
        clip.data.insert(uFormat, hMem);
        clip.owner = clip.open_owner.unwrap_or(0);
        hMem
    }
    
    /// IsClipboardFormatAvailable - Belirtilen bicemin panoda bulunup bulunmadigini sinar
    pub unsafe fn is_clipboard_format_available(uFormat: UINT) -> BOOL {
        if CLIPBOARD_STATE.lock().data.contains_key(&uFormat) {
            TRUE
        } else {
            FALSE
        }
    }
    
    /// RegisterClipboardFormatA - Yeni ozel pano bicemi kaydeder; var ise mevcut kimligini dondurur
    pub unsafe fn register_clipboard_format_a(lpszFormat: LPCSTR) -> UINT {
        let name = cstr_to_string(lpszFormat);
        if name.is_empty() {
            return 0;
        }
        let mut formats = CLIPBOARD_FORMATS.lock();
        if let Some(id) = formats.get(&name) {
            return *id;
        }
        let id = NEXT_CLIPBOARD_FORMAT.fetch_add(1, Ordering::Relaxed);
        formats.insert(name, id);
        id
    }
    
    /// CountClipboardFormats - Panoda mevcut olan bicemlerin sayisini dondurur
    pub unsafe fn count_clipboard_formats() -> INT {
        CLIPBOARD_STATE.lock().data.len() as INT
    }
    
    /// EnumClipboardFormats - Panodaki bicemleri sirali olarak numaralayarak dondurur
    pub unsafe fn enum_clipboard_formats(uFormat: UINT) -> UINT {
        let mut keys: Vec<UINT> = CLIPBOARD_STATE.lock().data.keys().copied().collect();
        keys.sort_unstable();
        if uFormat == 0 {
            return keys.first().copied().unwrap_or(0);
        }
        for key in keys {
            if key > uFormat {
                return key;
            }
        }
        0
    }
    
    /// GetClipboardOwner - Pano sahibinin pencere taniticisinI dondurur
    pub unsafe fn get_clipboard_owner() -> HWND {
        CLIPBOARD_STATE.lock().owner
    }
    
    /// SetClipboardViewer - Pano izleyici zinciriyle yeni bir pencere kaydeder
    pub unsafe fn set_clipboard_viewer(hWndNewViewer: HWND) -> HWND {
        let mut clip = CLIPBOARD_STATE.lock();
        let prev = clip.viewer;
        clip.viewer = hWndNewViewer;
        prev
    }
    
    /// GetClipboardViewer - Pano izleyici zincirindeki ilk pencereyi dondurur
    pub unsafe fn get_clipboard_viewer() -> HWND {
        CLIPBOARD_STATE.lock().viewer
    }
    
    /// ChangeClipboardChain - Pano izleyici zincirinden bir pencereyi cikarir
    pub unsafe fn change_clipboard_chain(hWndRemove: HWND, hWndNewNext: HWND) -> BOOL {
        let _ = hWndNewNext;
        let mut clip = CLIPBOARD_STATE.lock();
        if clip.viewer == hWndRemove {
            clip.viewer = 0;
        }
        TRUE
    }
    
    // ========================================================================
    // KAYNAKLAR (ikon, imleç, bit eşlem ve dize kaynak yukleyicileri)
    // ========================================================================
    
    /// LoadIconA - Kaynaklardan veya on tanimli sabitlerden ikon yukler
    pub unsafe fn load_icon_a(hInstance: HINSTANCE, lpIconName: LPCSTR) -> HICON {
        1 as HICON
    }
    
    /// LoadCursorA - Kaynaklardan veya on tanimli sabitlerden fare imleci yukler
    pub unsafe fn load_cursor_a(hInstance: HINSTANCE, lpCursorName: LPCSTR) -> HCURSOR {
        1 as HCURSOR
    }
    
    /// LoadBitmapA - Kaynaklardan bit eslem goruntu yukler
    pub unsafe fn load_bitmap_a(hInstance: HINSTANCE, lpBitmapName: LPCSTR) -> HBITMAP {
        1 as HBITMAP
    }
    
    /// LoadStringA - Kaynak tablosundan dize yukler; lpBuffer'a kopyalar
    pub unsafe fn load_string_a(hInstance: HINSTANCE, uID: UINT, lpBuffer: LPSTR, nBufferMax: INT) -> INT {
        0
    }
    
    /// LoadImageA - Ikon, imleç veya bit eslem yukler; uType ile tur secilir
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
    
    /// CopyImage - Mevcut bir ikon, imleç veya bit eslemi kopyalar ve donusturur
    pub unsafe fn copy_image(hImage: HANDLE, uType: UINT, cxDesired: INT, cyDesired: INT, fuFlags: UINT) -> HANDLE {
        hImage
    }
    
    /// DestroyIcon - LoadIconA ile yüklenen ikon bellegini serbest birakar
    pub unsafe fn destroy_icon(hIcon: HICON) -> BOOL {
        TRUE
    }
    
    /// DestroyCursor - LoadCursorA ile yüklenen imleç bellegini serbest birakar
    pub unsafe fn destroy_cursor(hCursor: HCURSOR) -> BOOL {
        TRUE
    }
    
    /// SetCursor - Fare imlecini belirtilen imleç tanıtıcısıyla değiştirir; önceki imleci döndürür
    pub unsafe fn set_cursor(hCursor: HCURSOR) -> HCURSOR {
        hCursor
    }
    
    /// GetCursor - Geçerli fare imleci tanıtıcısını döndürür
    pub unsafe fn get_cursor() -> HCURSOR {
        1 as HCURSOR
    }
    
    // ========================================================================
    // KANCALAR (sistem olaylarini yakalamanin kanca (hook) mekanizmasi)
    // ========================================================================
    
    /// SetWindowsHookExA - Sistem olaylarini yakalayan kanca fonksiyonu kurar (WH_ sabiti ile)
    pub unsafe fn set_windows_hook_ex_a(
        idHook: INT,
        lpfn: Option<unsafe extern "system" fn(INT, usize, isize) -> isize>,
        hMod: HINSTANCE,
        dwThreadId: DWORD,
    ) -> HANDLE {
        1 as HANDLE
    }
    
    /// UnhookWindowsHookEx - SetWindowsHookExA ile kurulan kancayi kaldirir
    pub unsafe fn unhook_windows_hook_ex(hhk: HANDLE) -> BOOL {
        TRUE
    }
    
    /// CallNextHookEx - Kanca zincirindeki bir sonraki kancayi cagirir
    pub unsafe fn call_next_hook_ex(hhk: HANDLE, nCode: INT, wParam: usize, lParam: isize) -> isize {
        0
    }
    
    // ========================================================================
    // CESITLI YARDIMCI ISLEVLER (pencere uzun degerleri, ozellikler, diger)
    // ======================================================================== - Pencereye bagli uzun tamsayi degerini dondurur (GWL_ sabiti ile)
        get_window_long_ptr_a(hWnd, nIndex)
    }
    
    /// SetWindowLongA - Pencereye bagli uzun tamsayi degerini gunceller
    pub unsafe fn set_window_long_a(hWnd: HWND, nIndex: INT, dwNewLong: isize) -> isize {
        set_window_long_ptr_a(hWnd, nIndex, dwNewLong)
    }
    
    /// GetWindowLongPtrA - 64-bit uyumlu pencere uzun isaretci degerini dondurur
    pub unsafe fn get_window_long_ptr_a(hWnd: HWND, nIndex: INT) -> isize {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return 0;
        }
        WINDOW_LONG_PTRS
            .lock()
            .get(&(hWnd, nIndex))
            .copied()
            .unwrap_or(0)
    }
    
    /// SetWindowLongPtrA - 64-bit uyumlu pencere uzun isaretci degerini gunceller
    pub unsafe fn set_window_long_ptr_a(hWnd: HWND, nIndex: INT, dwNewLong: isize) -> isize {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return 0;
        }
        let mut longs = WINDOW_LONG_PTRS.lock();
        let prev = longs.get(&(hWnd, nIndex)).copied().unwrap_or(0);
        longs.insert((hWnd, nIndex), dwNewLong);
        prev
    }
    
    /// GetClassLongA - Pencere sinifina bagli uzun tamsayi degerini dondurur
    pub unsafe fn get_class_long_a(hWnd: HWND, nIndex: INT) -> DWORD {
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return 0;
        };
        CLASS_LONG_VALUES
            .lock()
            .get(&(window.class_name.clone(), nIndex))
            .copied()
            .unwrap_or(0)
    }
    
    /// SetClassLongA - Pencere sinifina bagli uzun tamsayi degerini gunceller
    pub unsafe fn set_class_long_a(hWnd: HWND, nIndex: INT, dwNewLong: DWORD) -> DWORD {
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            return 0;
        };
        let key = (window.class_name.clone(), nIndex);
        let mut class_values = CLASS_LONG_VALUES.lock();
        let prev = class_values.get(&key).copied().unwrap_or(0);
        class_values.insert(key, dwNewLong);
        prev
    }
    
    /// GetPropA - Pencereye eklenmis adlandirilmis ozellik degerini dondurur
    pub unsafe fn get_prop_a(hWnd: HWND, lpString: LPCSTR) -> HANDLE {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return 0;
        }
        let name = cstr_to_string(lpString);
        if name.is_empty() {
            return 0;
        }
        WINDOW_PROPERTIES
            .lock()
            .get(&(hWnd, name))
            .copied()
            .unwrap_or(0)
    }
    
    /// SetPropA - Pencereye adlandirilmis ozellik ekler veya var olani gunceller
    pub unsafe fn set_prop_a(hWnd: HWND, lpString: LPCSTR, hData: HANDLE) -> BOOL {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return FALSE;
        }
        let name = cstr_to_string(lpString);
        if name.is_empty() {
            return FALSE;
        }
        WINDOW_PROPERTIES.lock().insert((hWnd, name), hData);
        TRUE
    }
    
    /// RemovePropA - Pencereden adlandirilmis ozelligi siler ve onceki degerini dondurur
    pub unsafe fn remove_prop_a(hWnd: HWND, lpString: LPCSTR) -> HANDLE {
        if !WINDOW_REGISTRY.lock().contains_key(&hWnd) {
            return 0;
        }
        let name = cstr_to_string(lpString);
        if name.is_empty() {
            return 0;
        }
        WINDOW_PROPERTIES
            .lock()
            .remove(&(hWnd, name))
            .unwrap_or(0)
    }
    
    /// EnumPropsA - Pencereye eklenmis tum ozellikler icin geri cagrimi cagirir
    pub unsafe fn enum_props_a(hWnd: HWND, lpEnumFunc: Option<unsafe extern "system" fn(HWND, LPCSTR, HANDLE) -> BOOL>) -> INT {
        let Some(callback) = lpEnumFunc else {
            return -1;
        };
        let entries: Vec<(String, HANDLE)> = WINDOW_PROPERTIES
            .lock()
            .iter()
            .filter_map(|((window, name), value)| {
                if *window == hWnd {
                    Some((name.clone(), *value))
                } else {
                    None
                }
            })
            .collect();

        let mut count = 0;
        for (name, value) in entries {
            let mut bytes = name.into_bytes();
            bytes.push(0);
            let ptr = bytes.as_ptr() as LPCSTR;
            if callback(hWnd, ptr, value) == FALSE {
                break;
            }
            count += 1;
        }
        count
    }
    
    /// GetWindowThreadProcessId
    pub unsafe fn get_window_thread_process_id(hWnd: HWND, lpdwProcessId: *mut DWORD) -> DWORD {
        let windows = WINDOW_REGISTRY.lock();
        let Some(window) = windows.get(&hWnd) else {
            if !lpdwProcessId.is_null() {
                *lpdwProcessId = 0;
            }
            return 0;
        };
        if !lpdwProcessId.is_null() {
            *lpdwProcessId = window.owner_process_id;
        }
        window.owner_thread_id
    }
    
    /// AttachThreadInput
    pub unsafe fn attach_thread_input(idAttach: DWORD, idAttachTo: DWORD, fAttach: BOOL) -> BOOL {
        TRUE
    }
    
    /// GetQueueStatus - Mesaj kuyruğundaki mesaj türlerini gösteren durum bayrağını döndürür
    pub unsafe fn get_queue_status(uFlags: UINT) -> DWORD {
        let _ = uFlags;
        if MESSAGE_QUEUE.lock().is_empty() {
            0
        } else {
            1
        }
    }
    
    /// GetInputState - Mesaj kuyruğunda fare veya klavye girişi mesajı olup olmadığını sınar
    pub unsafe fn get_input_state() -> BOOL {
        if MESSAGE_QUEUE.lock().is_empty() {
            FALSE
        } else {
            TRUE
        }
    }
}

// ============================================================================
// ADVAPI32 UYGULAMASI (kayit defteri, guvenlik, servisler, olay gunlugu, kriptografi)
// ============================================================================

mod advapi32 {
    use super::*;
    
    // ========================================================================
    // KAYIT DEFTERI (Win32 kayit hiyerarsisi sorgulama ve duzenleme islemleri)
    // ========================================================================
    
    /// RegOpenKeyExA - Belirtilen kayit defteri anahtarini erisim maskesiyle acar
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
    
    /// RegCloseKey - Acik kayit defteri anahtar taniticisinI kapatir
    pub unsafe fn reg_close_key(hKey: HKEY) -> LONG {
        0 // ERROR_SUCCESS
    }
    
    /// RegCreateKeyExA - Varsa mevcut anahtari acar; yoksa yeni anahtar olusturur
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
    
    /// RegDeleteKeyA - Belirtilen alt anahtari kayit defterinden siler
    pub unsafe fn reg_delete_key_a(hKey: HKEY, lpSubKey: LPCSTR) -> LONG {
        0
    }
    
    /// RegDeleteValueA - Bir kayit defteri anahtarindan deger girdisini siler
    pub unsafe fn reg_delete_value_a(hKey: HKEY, lpValueName: LPCSTR) -> LONG {
        0
    }
    
    /// RegEnumKeyExA - Bir anahtarin alt anahtarlarini indeks ile tek tek numaralandirir
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
    
    /// RegEnumValueA - Bir anahtardaki deger girdilerini tek tek numaralandirir
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
    
    /// RegQueryValueExA - Bir kayit defteri degerinin tipini ve verisini okur
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
    
    /// RegSetValueExA - Bir kayit defteri anahtarina deger yazar veya guncelleyer
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
    
    /// RegConnectRegistryA - Uzak bir bilgisayardaki kayit defterine baglanti kurar
    pub unsafe fn reg_connect_registry_a(lpMachineName: LPCSTR, hKey: HKEY, phkResult: *mut HKEY) -> LONG {
        0
    }
    
    /// RegNotifyChangeKeyValue - Kayit defteri anahtarindaki degisiklikleri izler ve bildirim uretir
    pub unsafe fn reg_notify_change_key_value(hKey: HKEY, bWatchSubtree: BOOL, dwNotifyFilter: DWORD, hEvent: HANDLE, fAsynchronous: BOOL) -> LONG {
        0
    }
    
    // ========================================================================
    // GUVENLIK (kullanici kimligi, SID ve erisim denetim listesi islemleri)
    // ========================================================================
    
    /// GetUserNameA (advapi32) - Etkin oturumdaki kullanicinin adini lpBuffer'a yazar
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
    
    /// LookupAccountNameA - Hesap adi ile SID ve etki alani bilgisini cozumler
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
    
    /// LookupAccountSidA - SID ile hesap adi ve etki alani bilgisini cozumler
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
    
    /// InitializeSecurityDescriptor - Yeni bos bir guvenlik belirteci yapisinI hazirlar
    pub unsafe fn initialize_security_descriptor(pSecurityDescriptor: *mut u8, dwRevision: DWORD) -> BOOL {
        TRUE
    }
    
    /// InitializeAcl - Bos bir erisim denetim listesi (ACL) yapisinI hazirlar
    pub unsafe fn initialize_acl(pAcl: *mut u8, nAclLength: DWORD, dwAclRevision: DWORD) -> BOOL {
        TRUE
    }
    
    /// AddAccessAllowedAce - ACL'e erisime izin veren bir giris (ACE) ekler
    pub unsafe fn add_access_allowed_ace(pAcl: *mut u8, dwAceRevision: DWORD, AccessMask: DWORD, pSid: *const u8) -> BOOL {
        TRUE
    }
    
    /// SetSecurityDescriptorDacl - Guvenlik belirtecine DACL atar; bDaclPresent FALSE ise DACL yoksayilir
    pub unsafe fn set_security_descriptor_dacl(pSecurityDescriptor: *mut u8, bDaclPresent: BOOL, pDacl: *const u8, bDaclDefaulted: BOOL) -> BOOL {
        TRUE
    }
    
    /// GetSecurityDescriptorDacl - Guvenlik belirtecindeki DACL'yi okur
    pub unsafe fn get_security_descriptor_dacl(pSecurityDescriptor: *const u8, lpbDaclPresent: *mut BOOL, pDacl: *mut *const u8, lpbDaclDefaulted: *mut BOOL) -> BOOL {
        TRUE
    }
    
    /// IsValidSecurityDescriptor - Guvenlik belirtecinin yapi butunlugunu dogrular
    pub unsafe fn is_valid_security_descriptor(pSecurityDescriptor: *const u8) -> BOOL {
        TRUE
    }
    
    /// GetLengthSid - Bir SID yapisinin bayt cinsinden uzunlugunu dondurur
    pub unsafe fn get_length_sid(pSid: *const u8) -> DWORD {
        28 // SID yapisinin standart bayt uzunlugu (28 bayt)
    }
    
    /// CopySid - Bir SID yapisini hedef tampona kopyalar
    pub unsafe fn copy_sid(nDestinationSidLength: DWORD, pDestinationSid: *mut u8, pSourceSid: *const u8) -> BOOL {
        TRUE
    }
    
    /// EqualSid - Iki SID yapisinin esdeger olup olmadigini karsilastirir
    pub unsafe fn equal_sid(pSid1: *const u8, pSid2: *const u8) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // SERVISLER (SCM uzerinden Windows hizmet yonetimi islemleri)
    // ========================================================================
    
    /// OpenSCManagerA - Servis Denetim Yoneticisine (SCM) baglanti tanitici acar
    pub unsafe fn open_sc_manager_a(lpMachineName: LPCSTR, lpDatabaseName: LPCSTR, dwDesiredAccess: DWORD) -> SC_HANDLE {
        1 as SC_HANDLE
    }
    
    /// CloseServiceHandle - SCM veya servis taniticisinI kapatir
    pub unsafe fn close_service_handle(hSCObject: SC_HANDLE) -> BOOL {
        TRUE
    }
    
    /// OpenServiceA - Adini belirttigimiz mevcut servise erisim tanitici acar
    pub unsafe fn open_service_a(hSCManager: SC_HANDLE, lpServiceName: LPCSTR, dwDesiredAccess: DWORD) -> SC_HANDLE {
        0
    }
    
    /// CreateServiceA - SCM veritabanina yeni bir Windows servisi kaydeder
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
    
    /// DeleteService - SCM veritabanindan servisi siler; tum taniticilar kapaninca kaldirilir
    pub unsafe fn delete_service(hService: SC_HANDLE) -> BOOL {
        TRUE
    }
    
    /// StartServiceA - Durdurulmus veya kurulmus bir servisi baslatir
    pub unsafe fn start_service_a(hService: SC_HANDLE, dwNumServiceArgs: DWORD, lpServiceArgVectors: *const LPCSTR) -> BOOL {
        TRUE
    }
    
    /// ControlService - Calisan servise kontrol kodu (SERVICE_CONTROL_*) gonderir
    pub unsafe fn control_service(hService: SC_HANDLE, dwControl: DWORD, lpServiceStatus: *mut SERVICE_STATUS) -> BOOL {
        TRUE
    }
    
    /// QueryServiceStatus - Servisin mevcut durum bilgisini SERVICE_STATUS yapisina yazar
    pub unsafe fn query_service_status(hService: SC_HANDLE, lpServiceStatus: *mut SERVICE_STATUS) -> BOOL {
        if !lpServiceStatus.is_null() {
            (*lpServiceStatus).dwServiceType = 0x10; // SERVICE_WIN32_OWN_PROCESS: tek islem servisi
            (*lpServiceStatus).dwCurrentState = 0x04; // SERVICE_RUNNING: servis calisma durumunda
            (*lpServiceStatus).dwControlsAccepted = 0;
            (*lpServiceStatus).dwWin32ExitCode = 0;
            (*lpServiceStatus).dwServiceSpecificExitCode = 0;
            (*lpServiceStatus).dwCheckPoint = 0;
            (*lpServiceStatus).dwWaitHint = 0;
        }
        TRUE
    }
    
    /// EnumServicesStatusA - SCM'deki tum servislerin liste ve durum bilgilerini dondurur
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
    
    /// GetServiceKeyNameA - Gorunen ad kullanarak servisin kayit adi elesinin taniyicisini dondurur
    pub unsafe fn get_service_key_name_a(hSCManager: SC_HANDLE, lpDisplayName: LPCSTR, lpServiceName: LPSTR, lpcchBuffer: *mut DWORD) -> BOOL {
        FALSE
    }
    
    /// GetServiceDisplayNameA - Kayit adi ile servisin kullaniciya gosterilen gorunen adini dondurur
    pub unsafe fn get_service_display_name_a(hSCManager: SC_HANDLE, lpServiceName: LPCSTR, lpDisplayName: LPSTR, lpcchBuffer: *mut DWORD) -> BOOL {
        FALSE
    }
    
    // ========================================================================
    // OLAY GUNLUGU (Windows olay gunluguna kayit yazma ve okuma islemleri)
    // ========================================================================
    
    /// RegisterEventSourceA - Belirtilen kaynaktan olay gunluguna yazabilmek icin tanitici acar
    pub unsafe fn register_event_source_a(lpUNCServerName: LPCSTR, lpSourceName: LPCSTR) -> HANDLE {
        1 as HANDLE
    }
    
    /// DeregisterEventSource - RegisterEventSourceA tanIticisini kapatir
    pub unsafe fn deregister_event_source(hEventLog: HANDLE) -> BOOL {
        TRUE
    }
    
    /// ReportEventA - Olay gunluguna tip, kategori ve girdileriyle birlikte olay yazar
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
    
    /// OpenEventLogA - Belirtilen olay gunlugunu okuma amacli acar
    pub unsafe fn open_event_log_a(lpUNCServerName: LPCSTR, lpSourceName: LPCSTR) -> HANDLE {
        1 as HANDLE
    }
    
    /// CloseEventLog - OpenEventLogA ile alinan taniticiyi kapatir
    pub unsafe fn close_event_log(hEventLog: HANDLE) -> BOOL {
        TRUE
    }
    
    /// ClearEventLogA - Olay gunlugunu temizler; isteye bagli olarak yedek dosyaya yazar
    pub unsafe fn clear_event_log_a(hEventLog: HANDLE, lpBackupFileName: LPCSTR) -> BOOL {
        TRUE
    }
    
    /// ReadEventLogA - Olay gunlugundaki kayitlari sirayla veya rasgele okur
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
    
    /// GetNumberOfEventLogRecords - Olay gunlugundaki toplam kayit sayisini dondurur
    pub unsafe fn get_number_of_event_log_records(hEventLog: HANDLE, NumberOfRecords: *mut DWORD) -> BOOL {
        if !NumberOfRecords.is_null() {
            *NumberOfRecords = 0;
        }
        TRUE
    }
    
    // ========================================================================
    // KRIPTOGRAFI (sifreli rastgele sayi, hash ve sifreleme islemleri)
    // ========================================================================
    
    /// CryptAcquireContextA - Kriptografik hizmet saglayicisina (CSP) erisim tanitici alir
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
    
    /// CryptReleaseContext - CryptAcquireContextA ile alinan CSP taniticisinI serbest birakar
    pub unsafe fn crypt_release_context(hProv: HCRYPTPROV, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptGenRandom - Kriptografik acidan guvenli rastgele baytlar uretir
    pub unsafe fn crypt_gen_random(hProv: HCRYPTPROV, dwLen: DWORD, pbBuffer: *mut BYTE) -> BOOL {
        if !pbBuffer.is_null() {
            for i in 0..dwLen as usize {
                *pbBuffer.add(i) = crate::random::next_u32() as u8;
            }
        }
        TRUE
    }
    
    /// CryptCreateHash - Belirtilen algoritma icin bos bir hash nesnesi olusturur
    pub unsafe fn crypt_create_hash(hProv: HCRYPTPROV, Algid: DWORD, hKey: HCRYPTKEY, dwFlags: DWORD, phHash: *mut HCRYPTHASH) -> BOOL {
        if !phHash.is_null() {
            *phHash = 1 as HCRYPTHASH;
        }
        TRUE
    }
    
    /// CryptDestroyHash - CryptCreateHash ile olusturulan hash nesnesini serbest birakar
    pub unsafe fn crypt_destroy_hash(hHash: HCRYPTHASH) -> BOOL {
        TRUE
    }
    
    /// CryptHashData - Veriyi hash nesnesine ekleyerek hash hesaplamaya katkida bulunur
    pub unsafe fn crypt_hash_data(hHash: HCRYPTHASH, pbData: *const BYTE, dwDataLen: DWORD, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptGetHashParam - Tamamlanmis hash degerini veya parametresini okur
    pub unsafe fn crypt_get_hash_param(hHash: HCRYPTHASH, dwParam: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptDeriveKey - Hash degerinden sifreli anahtar turetir
    pub unsafe fn crypt_derive_key(hProv: HCRYPTPROV, Algid: DWORD, hBaseData: HCRYPTHASH, dwFlags: DWORD, phKey: *mut HCRYPTKEY) -> BOOL {
        if !phKey.is_null() {
            *phKey = 1 as HCRYPTKEY;
        }
        TRUE
    }
    
    /// CryptDestroyKey - Kriptografik anahtar taniticisinI serbest birakar
    pub unsafe fn crypt_destroy_key(hKey: HCRYPTKEY) -> BOOL {
        TRUE
    }
    
    /// CryptEncrypt - Veriyi belirtilen anahtarla sifreler
    pub unsafe fn crypt_encrypt(hKey: HCRYPTKEY, hHash: HCRYPTHASH, Final: BOOL, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD, dwBufLen: DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptDecrypt - Sifrelenmis veriyi anahtar kullanarak cozumler
    pub unsafe fn crypt_decrypt(hKey: HCRYPTKEY, hHash: HCRYPTHASH, Final: BOOL, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptImportKey - Dis kaynaktan veri tamponunu kriptografik anahtara aktarir
    pub unsafe fn crypt_import_key(hProv: HCRYPTPROV, pbData: *const BYTE, dwDataLen: DWORD, hPubKey: HCRYPTKEY, dwFlags: DWORD, phKey: *mut HCRYPTKEY) -> BOOL {
        if !phKey.is_null() {
            *phKey = 1 as HCRYPTKEY;
        }
        TRUE
    }
    
    /// CryptExportKey - Kriptografik anahtari tasinabilir tampon (BLOB) formatina donusturur
    pub unsafe fn crypt_export_key(hKey: HCRYPTKEY, hExpKey: HCRYPTKEY, dwBlobType: DWORD, dwFlags: DWORD, pbData: *mut BYTE, pdwDataLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptSignHashA - Tamamlanmis hash nesnesini ozel anahtarla imzalar
    pub unsafe fn crypt_sign_hash_a(hHash: HCRYPTHASH, dwKeySpec: DWORD, sDescription: LPCSTR, dwFlags: DWORD, pbSignature: *mut BYTE, pdwSigLen: *mut DWORD) -> BOOL {
        TRUE
    }
    
    /// CryptVerifySignatureA - Hash icin imzayi acik anahtarla dogrular
    pub unsafe fn crypt_verify_signature_a(hHash: HCRYPTHASH, pbSignature: *const BYTE, dwSigLen: DWORD, hPubKey: HCRYPTKEY, sDescription: LPCSTR, dwFlags: DWORD) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // ISLEM VE IS PARCACIGI GUVENLIK TOKENI (kimlik dogrulama ve ayricalik yonetimi)
    // ========================================================================
    
    /// CreateProcessAsUserA - Belirtilen kullanici tokeninde yeni islem olusturur
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
        let _ = hToken;
        super::kernel32::create_process_a(
            lpApplicationName,
            lpCommandLine,
            lpProcessAttributes as LPVOID,
            lpThreadAttributes as LPVOID,
            bInheritHandles,
            dwCreationFlags,
            lpEnvironment as LPVOID,
            lpCurrentDirectory,
            lpStartupInfo as LPVOID,
            lpProcessInformation as LPVOID,
        )
    }
    
    /// OpenProcessToken - Islem icin erisim belirteci (token) tanitici acar
    pub unsafe fn open_process_token(hProcess: HANDLE, dwDesiredAccess: DWORD, phToken: *mut HANDLE) -> BOOL {
        if !phToken.is_null() {
            *phToken = 1 as HANDLE;
        }
        TRUE
    }
    
    /// OpenThreadToken - Is parcacigi icin erisim belirteci tanitici acar
    pub unsafe fn open_thread_token(hThread: HANDLE, dwDesiredAccess: DWORD, bOpenAsSelf: BOOL, phToken: *mut HANDLE) -> BOOL {
        TRUE
    }
    
    /// DuplicateTokenEx - Mevcut tokeni kopyalayarak yeni bir token tanitici olusturur
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
    
    /// ImpersonateLoggedOnUser - Belirtilen kullanici tokenini is parcacigina atar
    pub unsafe fn impersonate_logged_on_user(hToken: HANDLE) -> BOOL {
        TRUE
    }
    
    /// RevertToSelf - Taklit edilen kullanici kimligini kaldirip is parcacigini eski haline getirir
    pub unsafe fn revert_to_self() -> BOOL {
        TRUE
    }
    
    /// GetTokenInformation - Erisim belirtecinden belirtilen bilgi sinifini okur
    pub unsafe fn get_token_information(
        hToken: HANDLE,
        TokenInformationClass: DWORD,
        TokenInformation: *mut u8,
        TokenInformationLength: DWORD,
        ReturnLength: *mut DWORD,
    ) -> BOOL {
        TRUE
    }
    
    /// SetTokenInformation - Erisim belirtecine belirtilen bilgi sinifini yazar
    pub unsafe fn set_token_information(
        hToken: HANDLE,
        TokenInformationClass: DWORD,
        TokenInformation: *const u8,
        TokenInformationLength: DWORD,
    ) -> BOOL {
        TRUE
    }
    
    /// AdjustTokenPrivileges - Tokendeki ayricaliklari etkinlestirir, devre disi birakar veya kaldirir
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
    
    /// LookupPrivilegeValueA - Ayricalik adini LUID degeriyle eslestirerek sorgular
    pub unsafe fn lookup_privilege_value_a(lpSystemName: LPCSTR, lpName: LPCSTR, lpLuid: *mut u64) -> BOOL {
        TRUE
    }
    
    /// LookupPrivilegeDisplayNameA - Bir ayricalik adinI insan tarafindan okunabilir metne donusturur
    pub unsafe fn lookup_privilege_display_name_a(lpSystemName: LPCSTR, lpName: LPCSTR, lpDisplayName: LPSTR, cchDisplayName: *mut DWORD, lpLanguageId: *mut DWORD) -> BOOL {
        TRUE
    }
}

// ============================================================================
// SHELL32 IMPLEMENTATION
// ============================================================================

mod shell32 {
    use super::*;
    use core::sync::atomic::{AtomicU64, Ordering};

    const FO_MOVE: UINT = 0x0001;
    const FO_COPY: UINT = 0x0002;
    const FO_DELETE: UINT = 0x0003;
    const FO_RENAME: UINT = 0x0004;

    static SHELL_VFS: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    static DRAG_FILES: Mutex<BTreeMap<HDROP, Vec<String>>> = Mutex::new(BTreeMap::new());
    static NEXT_DROP_HANDLE: AtomicU64 = AtomicU64::new(1);

    unsafe fn cstr_to_string(ptr: LPCSTR) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut out = String::new();
        let mut cursor = ptr;
        while !cursor.is_null() && *cursor != 0 {
            out.push(*cursor as u8 as char);
            cursor = cursor.add(1);
        }
        out
    }

    unsafe fn copy_cstr(dst: LPSTR, cap: UINT, text: &str) -> UINT {
        if dst.is_null() || cap == 0 {
            return text.len() as UINT;
        }
        let cap_usize = cap as usize;
        let bytes = text.as_bytes();
        let to_copy = core::cmp::min(bytes.len(), cap_usize.saturating_sub(1));
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, to_copy);
        *((dst as *mut u8).add(to_copy)) = 0;
        to_copy as UINT
    }

    unsafe fn read_multisz_first(ptr: LPCSTR) -> String {
        cstr_to_string(ptr)
    }
    
    /// ShellExecuteA - Belirtilen işlemi (aç, çalıştır, yazdır) bir dosya üzerinde gerçekleştirir
    pub unsafe fn shell_execute_a(
        hwnd: HWND,
        lpOperation: LPCSTR,
        lpFile: LPCSTR,
        lpParameters: LPCSTR,
        lpDirectory: LPCSTR,
        nShowCmd: INT,
    ) -> HINSTANCE {
        let operation = cstr_to_string(lpOperation);
        let file = cstr_to_string(lpFile);
        let parameters = cstr_to_string(lpParameters);
        let directory = cstr_to_string(lpDirectory);
        crate::serial_println!(
            "[WIN32] ShellExecuteA: op={} file={} args={} dir={} show={} hwnd={}",
            operation,
            file,
            parameters,
            directory,
            nShowCmd,
            hwnd
        );

        if file.is_empty() {
            return 31 as HINSTANCE;
        }

        SHELL_VFS.lock().entry(file.clone()).or_insert_with(Vec::new);
        let drop = NEXT_DROP_HANDLE.fetch_add(1, Ordering::Relaxed);
        DRAG_FILES.lock().insert(drop, vec![file]);
        33 as HINSTANCE
    }
    
    /// ShellExecuteExA - Genişletilmiş kabuk yürütme; SHELLEXECUTEINFOA yapısıyla tam denetim sağlar
    pub unsafe fn shell_execute_ex_a(pExecInfo: *mut SHELLEXECUTEINFOA) -> BOOL {
        TRUE
    }
    
    /// ShellAboutA - Uygulama hakkında standart Shell Hakkında iletişim kutusunu gösterir
    pub unsafe fn shell_about_a(hWnd: HWND, szApp: LPCSTR, szOtherStuff: LPCSTR, hIcon: HICON) -> BOOL {
        TRUE
    }
    
    /// ExtractIconA
    pub unsafe fn extract_icon_a(hInst: HINSTANCE, lpszExeFileName: LPCSTR, nIconIndex: UINT) -> HICON {
        1 as HICON
    }
    
    /// ExtractIconExA
    pub unsafe fn extract_icon_ex_a(lpszFile: LPCSTR, nIconIndex: INT, phiconLarge: *mut HICON, phiconSmall: *mut HICON, nIcons: UINT) -> UINT {
        let _ = (cstr_to_string(lpszFile), nIconIndex);
        if nIcons == 0 {
            return 0;
        }
        if !phiconLarge.is_null() {
            *phiconLarge = 1;
        }
        if !phiconSmall.is_null() {
            *phiconSmall = 1;
        }
        1
    }
    
    /// DragAcceptFiles
    pub unsafe fn drag_accept_files(hWnd: HWND, fAccept: BOOL) {
        let _ = (hWnd, fAccept);
    }
    
    /// DragQueryFileA
    pub unsafe fn drag_query_file_a(hDrop: HDROP, iFile: UINT, lpszFile: LPSTR, cch: UINT) -> UINT {
        let files = DRAG_FILES.lock();
        let Some(list) = files.get(&hDrop) else {
            return 0;
        };

        if iFile == 0xFFFF_FFFF {
            return list.len() as UINT;
        }

        let idx = iFile as usize;
        let Some(path) = list.get(idx) else {
            return 0;
        };
        copy_cstr(lpszFile, cch, path)
    }
    
    /// DragQueryPoint
    pub unsafe fn drag_query_point(hDrop: HDROP, lppt: *mut POINT) -> BOOL {
        FALSE
    }
    
    /// DragFinish
    pub unsafe fn drag_finish(hDrop: HDROP) {
        DRAG_FILES.lock().remove(&hDrop);
    }
    
    /// Shell_NotifyIconA
    pub unsafe fn shell_notify_icon_a(dwMessage: DWORD, lpData: *const NOTIFYICONDATAA) -> BOOL {
        TRUE
    }
    
    /// SHGetPathFromIDListA
    pub unsafe fn sh_get_path_from_id_list_a(pidl: LPCSTR, pszPath: LPSTR) -> BOOL {
        FALSE
    }
    
    /// SHBrowseForFolderA - Klasör seçme iletişim kutusunu gösterir; seçilen yolun tanıtıcısını döndürür
    pub unsafe fn sh_browse_for_folder_a(lpbi: *const BROWSEINFOA) -> LPCSTR {
        0 as LPCSTR
    }
    
    /// SHGetSpecialFolderPathA
    pub unsafe fn sh_get_special_folder_path_a(hwnd: HWND, pszPath: LPSTR, csidl: INT, fCreate: BOOL) -> BOOL {
        FALSE
    }
    
    /// SHGetFolderPathA - Özel klasörün dosya sistemi yolunu pszPath tamponuna yazar
    pub unsafe fn sh_get_folder_path_a(hwnd: HWND, csidl: INT, hToken: HANDLE, dwFlags: DWORD, pszPath: LPSTR) -> HRESULT {
        let _ = (hwnd, csidl, hToken, dwFlags, pszPath);
        0x80070002u32 as i32 // E_FAIL
    }
    
    /// SHGetDesktopFolder - Masaüstü IShellFolder arabirimini döndürür (COM-uyumlu kabuğ giriş noktası)
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
        let _ = (dwFileAttributes, cbFileInfo, uFlags);
        if psfi.is_null() {
            return 0;
        }

        let path = cstr_to_string(pszPath);
        (*psfi).hIcon = 1;
        (*psfi).iIcon = 0;
        (*psfi).dwAttributes = FILE_ATTRIBUTE_NORMAL;
        for byte in (*psfi).szDisplayName.iter_mut() {
            *byte = 0;
        }
        for byte in (*psfi).szTypeName.iter_mut() {
            *byte = 0;
        }

        let name = if path.is_empty() { "item" } else { path.rsplit('\\').next().unwrap_or("item") };
        let kind = if name.contains('.') { "File" } else { "Folder" };
        let name_bytes = name.as_bytes();
        let kind_bytes = kind.as_bytes();
        let name_len = core::cmp::min(name_bytes.len(), (*psfi).szDisplayName.len().saturating_sub(1));
        let kind_len = core::cmp::min(kind_bytes.len(), (*psfi).szTypeName.len().saturating_sub(1));
        for i in 0..name_len {
            (*psfi).szDisplayName[i] = name_bytes[i] as i8;
        }
        for i in 0..kind_len {
            (*psfi).szTypeName[i] = kind_bytes[i] as i8;
        }
        1
    }
    
    /// SHFileOperationA
    pub unsafe fn sh_file_operation_a(lpFileOp: *mut SHFILEOPSTRUCTA) -> INT {
        if lpFileOp.is_null() {
            return 1;
        }

        let op = (*lpFileOp).wFunc;
        let from = read_multisz_first((*lpFileOp).pFrom);
        let to = read_multisz_first((*lpFileOp).pTo);

        let mut vfs = SHELL_VFS.lock();
        let result = match op {
            FO_COPY => {
                if from.is_empty() || to.is_empty() {
                    1
                } else {
                    let payload = vfs.get(&from).cloned().unwrap_or_else(Vec::new);
                    vfs.insert(to, payload);
                    0
                }
            }
            FO_MOVE | FO_RENAME => {
                if from.is_empty() || to.is_empty() {
                    1
                } else {
                    let payload = vfs.remove(&from).unwrap_or_else(Vec::new);
                    vfs.insert(to, payload);
                    0
                }
            }
            FO_DELETE => {
                if from.is_empty() {
                    1
                } else {
                    vfs.remove(&from);
                    0
                }
            }
            _ => 1,
        };

        (*lpFileOp).fAnyOperationsAborted = if result == 0 { FALSE } else { TRUE };
        result
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
    // BELLEK YÖNETİMİ (heap tahsisi, yeniden boyutlandırma ve serbest bırakma)
    // ========================================================================
    
    /// malloc - Heap'ten belirtilen boyutta belleği tahsis eder; NULL başarısız olursa döner
    pub unsafe fn malloc(size: SIZE_T) -> LPVOID {
        // TODO: Gerçek heap ayırıcısı bağlanacak
        let _ = size;
        0 as LPVOID
    }
    
    /// free - malloc/calloc/realloc ile ayırılmış belleği serbest bırakır
    pub unsafe fn free(ptr: LPVOID) {
        let _ = ptr;
    }
    
    /// calloc - num*size baytlık sıfırlanmış bellek tahsis eder
    pub unsafe fn calloc(num: SIZE_T, size: SIZE_T) -> LPVOID {
        let _ = (num, size);
        0 as LPVOID
    }
    
    /// realloc - Mevcut bellek bloğunu yeni boyuta yeniden tahsis eder
    pub unsafe fn realloc(ptr: LPVOID, size: SIZE_T) -> LPVOID {
        let _ = (ptr, size);
        0 as LPVOID
    }
    
    /// _msize - malloc ile ayrılan bellek bloğunun boyutunu döndürür
    pub unsafe fn _msize(ptr: LPVOID) -> SIZE_T {
        // TODO: Gerçek tahsis boyutunu sorgula
        let _ = ptr;
        0
    }
    
    /// _expand - Mevcut bellek bloğunu taşımadan yeni boyuta genişletmeye çalışır
    pub unsafe fn _expand(ptr: LPVOID, size: SIZE_T) -> LPVOID {
        let _ = (ptr, size);
        0 as LPVOID
    }
    
    /// _heapmin - Serbest heap hafızasını işletim sistemine geri verir
    pub unsafe fn _heapmin() -> INT {
        0
    }
    
    // ========================================================================
    // DİZİ İŞLEMLERİ (null-sonlandırılmış C dizileri üzerine standart işlemler)
    // ========================================================================
    
    /// strlen - Null-sonlandırılmış C dizisinin karakter uzunluğunu döndürür
    pub unsafe fn strlen(s: LPCSTR) -> SIZE_T {
        let mut len = 0usize;
        let mut ptr = s;
        while !ptr.is_null() && *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        len as SIZE_T
    }
    
    /// strcpy - Kaynak C dizisini hedef tampona kopyalar; null bileşeni dahildir
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
    
    /// strncpy - Kaynak dizinin en fazla count karakterini hedefe kopyalar; kalan alanı sıfırlar
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
    
    /// strcat - Kaynak C dizisini hedef dizinin sonuna ekler; hedefte yeterli alan olmalıdır
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
    
    /// strncat - Kaynak dizinden en fazla count karakteri hedefe ekler
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
    
    /// strcmp - İki C dizisini büyük/küçük harfe duyarlı kıyaslar; sıra farkını döndürür
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
    
    /// strncmp - İki C dizisinin en fazla count karakterini kıyaslar
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
    
    /// strchr - Dizide belirtilen karakterin ilk geçtiği yeri bulur
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
    
    /// strrchr - Dizide belirtilen karakterin son geçtiği yeri bulur
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
    
    /// strstr - Ana dizide alt diziyi arar; bulunursa işaretçi döndürür
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
    
    /// memcpy - count baytı kaynaktan hedefe kopyalar; bölgeler çakışmamalıdır
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
    
    /// memmove - count baytı kaynaktan hedefe taşır; bölgeler çakışsa bile doğru çalışır
    pub unsafe fn memmove(dest: LPVOID, src: LPCVOID, count: SIZE_T) -> LPVOID {
        let d = dest as usize;
        let s = src as usize;
        if d < s || d >= s + count as usize {
            // İleri yönlü kopyalama
            memcpy(dest, src, count)
        } else {
            // Geri yönlü kopyalama (çakışan bölgelerde güvenli)
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
    
    /// memset - Bellek bloğunun her baytnı belirtilen değerle doldurur
    pub unsafe fn memset(dest: LPVOID, c: INT, count: SIZE_T) -> LPVOID {
        let mut d = dest as *mut u8;
        for _ in 0..count as usize {
            *d = c as u8;
            d = d.add(1);
        }
        dest
    }
    
    /// memcmp - İki bellek bölgesini bayt bayt kıyaslar; fark sıfıra göre döner
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
    // GİRİŞ/ÇIKIŞ (ıdosya akışı komütları ve biçimli metin g/ç)
    // ========================================================================
    
    /// fopen - Belirtilen modda dosya için akış açar; NULL başarısızlıkta döner
    pub unsafe fn fopen(filename: LPCSTR, mode: LPCSTR) -> *mut FILE {
        let _ = (filename, mode);
        0 as *mut FILE
    }
    
    /// fclose - Akışı kapatarak arabellekö diske yazar
    pub unsafe fn fclose(stream: *mut FILE) -> INT {
        let _ = stream;
        0
    }
    
    /// fread - Akıştan count adette size baytlık öğe okur
    pub unsafe fn fread(ptr: LPVOID, size: SIZE_T, count: SIZE_T, stream: *mut FILE) -> SIZE_T {
        let _ = (ptr, size, count, stream);
        0
    }
    
    /// fwrite - Akışa count adette size baytlık öğe yazar
    pub unsafe fn fwrite(ptr: LPCVOID, size: SIZE_T, count: SIZE_T, stream: *mut FILE) -> SIZE_T {
        let _ = (ptr, size, count, stream);
        0
    }
    
    /// fseek - Akışın konum göstergesini origin'e göre offset kadar taşır
    pub unsafe fn fseek(stream: *mut FILE, offset: LONG, origin: INT) -> INT {
        let _ = (stream, offset, origin);
        0
    }
    
    /// ftell - Akışın mevcut konumunu döndürür
    pub unsafe fn ftell(stream: *mut FILE) -> LONG {
        let _ = stream;
        0
    }
    
    /// feof - Akışın dosya sonu göstergesini sorgular; sıfır dışı değer EOF demektir
    pub unsafe fn feof(stream: *mut FILE) -> INT {
        let _ = stream;
        0
    }
    
    /// fgetc - Akıştan tek bir karakter okur; EOF'ta -1 döndürür
    pub unsafe fn fgetc(stream: *mut FILE) -> INT {
        let _ = stream;
        -1 // EOF
    }
    
    /// fputc - Akışa tek bir karakter yazar; başarısızlıkta -1 döndürür
    pub unsafe fn fputc(c: INT, stream: *mut FILE) -> INT {
        let _ = (c, stream);
        -1
    }
    
    /// fgets - Akıştan en fazla n-1 karakter okuarak satıra kadar s dizisine yazar
    pub unsafe fn fgets(s: LPSTR, n: INT, stream: *mut FILE) -> LPSTR {
        let _ = (s, n, stream);
        0 as LPSTR
    }
    
    /// fputs - Akışa null-sonlandırılmış C dizisi yazar
    pub unsafe fn fputs(s: LPCSTR, stream: *mut FILE) -> INT {
        let _ = (s, stream);
        -1
    }
    
    /// fprintf - Biçimlendirilmiş çıktıyı dosya akışına yazar
    pub unsafe fn fprintf(stream: *mut FILE, format: LPCSTR, args: *const u8) -> INT {
        let _ = (stream, format, args);
        0
    }
    
    /// printf - Biçimlendirilmiş çıktıyı standart çıkışa yazar
    pub unsafe fn printf(format: LPCSTR, args: *const u8) -> INT {
        let _ = (format, args);
        0
    }
    
    /// sprintf - Biçimlendirilmiş çıktıyı tampon diziye yazar
    pub unsafe fn sprintf(buffer: LPSTR, format: LPCSTR, args: *const u8) -> INT {
        let _ = (buffer, format, args);
        0
    }
    
    /// snprintf - En fazla count karakterlik biçimlendirilmiş çıktıyı tampona yazar
    pub unsafe fn snprintf(buffer: LPSTR, count: SIZE_T, format: LPCSTR, args: *const u8) -> INT {
        let _ = (buffer, count, format, args);
        0
    }
    
    /// scanf - Standart girişten biçimlenmiş veri okur
    pub unsafe fn scanf(format: LPCSTR, args: *const u8) -> INT {
        let _ = (format, args);
        -1
    }
    
    // ========================================================================
    // MATEMATİK (çış mutlak değer, rassal sayı üretimi)
    // ========================================================================
    
    /// abs - Tam sayının mutlak değerini döndürür
    pub unsafe fn abs(n: INT) -> INT {
        if n < 0 { -n } else { n }
    }
    
    /// labs - Uzun tam sayının mutlak değerini döndürür
    pub unsafe fn labs(n: LONG) -> LONG {
        if n < 0 { -n } else { n }
    }
    
    /// rand - 0-RAND_MAX arasında sözde rassal tam sayı üretir
    pub unsafe fn rand() -> INT {
        (crate::random::next_u32() & 0x7FFF) as INT
    }
    
    /// srand - Rassal sayı üreticisini verilen başlangıç değeriyle başlatır
    pub unsafe fn srand(seed: UINT) {
        let _ = seed;
    }
    
    // ========================================================================
    // ZAMAN (saat, takvim ve tarih dönüşüm işlemleri)
    // ========================================================================
    
    /// time - 1 Ocak 1970'ten bu yana geçen saniye sayısını döndürür (Unix epochu)
    pub unsafe fn time(timer: *mut time_t) -> time_t {
        let t = crate::random::next_u32() as time_t;
        if !timer.is_null() {
            *timer = t;
        }
        t
    }
    
    /// clock - Programın başlatılmasından bu yana geçen işlemci tik sayısını döndürür
    pub unsafe fn clock() -> clock_t {
        crate::random::next_u32() as clock_t
    }
    
    /// localtime - time_t değerini yerel saat dilimine göre tm yapısına dönüştürür
    pub unsafe fn localtime(timer: *const time_t) -> *mut tm {
        let _ = timer;
        0 as *mut tm
    }
    
    /// gmtime - time_t değerini UTC saat dilimine göre tm yapısına dönüştürür
    pub unsafe fn gmtime(timer: *const time_t) -> *mut tm {
        let _ = timer;
        0 as *mut tm
    }
    
    /// asctime - tm yapısını okunabilir metin biçimine dönüştürür ("Mon Jan  1 00:00:00 1970")
    pub unsafe fn asctime(tm: *const tm) -> LPSTR {
        let _ = tm;
        0 as LPSTR
    }
    
    /// ctime - time_t değerini okunabilir metin biçimine dönüştürür
    pub unsafe fn ctime(timer: *const time_t) -> LPSTR {
        let _ = timer;
        0 as LPSTR
    }
    
    /// strftime - Zaman yapısını biçim dizisine göre biçimlendirir; yazılan karakter sayısını döndürür
    pub unsafe fn strftime(s: LPSTR, maxsize: SIZE_T, format: LPCSTR, tm: *const tm) -> SIZE_T {
        let _ = (s, maxsize, format, tm);
        0
    }
    
    // ========================================================================
    // ÇEŞİTLİ (çıkış, ortam değişkenleri, sayı dönüşüm ve arama)
    // ========================================================================
    
    /// exit - Programı belirtilen çıkış koduyla düzgün sonlandırır (at exit işlevlerini çağırır)
    pub unsafe fn exit(code: INT) {
        crate::serial_println!("[WIN32] exit({})", code);
        loop {}
    }
    
    /// abort - Anormal program sonlandırması; SIGABRT sinyali gönderir
    pub unsafe fn abort() {
        crate::serial_println!("[WIN32] abort()");
        loop {}
    }
    
    /// system - Sistem kabuğunu çağırarak komut satırı komutu yürütür
    pub unsafe fn system(command: LPCSTR) -> INT {
        let _ = command;
        -1
    }
    
    /// getenv - Süreç ortamında belirtilen değişkenin değerini döndürür
    pub unsafe fn getenv(varname: LPCSTR) -> LPSTR {
        let _ = varname;
        0 as LPSTR
    }
    
    /// atoi
    pub unsafe fn atoi(s: LPCSTR) -> INT {
        let mut result = 0i32;
        let mut ptr = s;
        let mut sign = 1i32;
        
        // Baş boşlukları atla
        while !ptr.is_null() && (*ptr == ' ' as i8 || *ptr == '\t' as i8 || *ptr == '\n' as i8) {
            ptr = ptr.add(1);
        }
        
        // İşareti işle
        if !ptr.is_null() && *ptr == '-' as i8 {
            sign = -1;
            ptr = ptr.add(1);
        } else if !ptr.is_null() && *ptr == '+' as i8 {
            ptr = ptr.add(1);
        }
        
        // Rakamları çözümle
        while !ptr.is_null() && *ptr >= '0' as i8 && *ptr <= '9' as i8 {
            result = result * 10 + (*ptr - '0' as i8) as i32;
            ptr = ptr.add(1);
        }
        
        result * sign
    }
    
    /// atol - ASCII dizisini uzun tam sayıya dönüştürür
    pub unsafe fn atol(s: LPCSTR) -> LONG {
        atoi(s) as LONG
    }
    
    /// atof - ASCII dizisini kayan noktalı sayıya dönüştürür
    pub unsafe fn atof(s: LPCSTR) -> f64 {
        let _ = s;
        0.0
    }
    
    /// strtol - Tabana göre ASCII dizisini uzun tam sayıya dönüştürür; bitiş noktasını endptr'a yazar
    pub unsafe fn strtol(s: LPCSTR, endptr: *mut LPSTR, base: INT) -> LONG {
        let _ = (s, endptr, base);
        0
    }
    
    /// strtoul - Tabana göre ASCII dizisini işaretsiz uzun tam sayıya dönüştürür
    pub unsafe fn strtoul(s: LPCSTR, endptr: *mut LPSTR, base: INT) -> ULONG {
        let _ = (s, endptr, base);
        0
    }
    
    /// strtod - ASCII dizisini çift duyarlıklı kayan noktalı sayıya dönüştürür
    pub unsafe fn strtod(s: LPCSTR, endptr: *mut LPSTR) -> f64 {
        let _ = (s, endptr);
        0.0
    }
    
    /// qsort - Dizi öğelerini compar fonksiyonuna göre hızlı sıralamayı kullanarak sıralar
    pub unsafe fn qsort(base: LPVOID, num: SIZE_T, size: SIZE_T, compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>) {
        let _ = (base, num, size, compar);
        // TODO: Hızlı sıralama algoritması uygulanacak
    }
    
    /// bsearch - Sıralı dizide ikili arama yapar; bulunursa işaretçi döndürür
    pub unsafe fn bsearch(key: LPCVOID, base: LPCVOID, num: SIZE_T, size: SIZE_T, compar: Option<unsafe extern "C" fn(LPCVOID, LPCVOID) -> INT>) -> LPVOID {
        let _ = (key, base, num, size, compar);
        0 as LPVOID
    }
}

// ============================================================================
// GDI32 UYGULAMASI (çizim, piksel biçimleri, font ve koordinat dönüşümleri)
// ============================================================================

mod gdi32 {
    use super::*;
    
    // ========================================================================
    // ÇİZİM PRİMİTİFLERİ (temel çizgi, eğri, yay ve çokgen çizme işlemleri)
    // ========================================================================
    
    /// MoveToEx - Çizim konumunu belirtilen koordinata taşır; eski konumu lppt'ye yazar
    pub unsafe fn move_to_ex(hdc: HDC, x: INT, y: INT, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// LineTo - Geçerli konumdan belirtilen koordinata çizgi çizer; konumu günceller
    pub unsafe fn line_to(hdc: HDC, nXEnd: INT, nYEnd: INT) -> BOOL {
        crate::serial_println!("[WIN32] LineTo: {},{}", nXEnd, nYEnd);
        TRUE
    }
    
    /// Polyline - Bir dizi noktayı birleştiren çok parçalı çizgi çizer
    pub unsafe fn polyline(hdc: HDC, lppt: *const POINT, cPoints: INT) -> BOOL {
        TRUE
    }
    
    /// PolylineTo - Geçerli konumdan başlayarak çok parçalı çizgi çizer; konumu günceller
    pub unsafe fn polyline_to(hdc: HDC, lppt: *const POINT, cCount: DWORD) -> BOOL {
        TRUE
    }
    
    /// PolyDraw - Kapalı Bézier ve doğru listesini bir çağrıda çizer
    pub unsafe fn poly_draw(hdc: HDC, lppt: *const POINT, lpbTypes: *const BYTE, cCount: INT) -> BOOL {
        TRUE
    }
    
    /// Arc - Elipsin sınır dikdörtgeni ve iki radyal doğruyla tanımlanan yay çizer
    pub unsafe fn arc(
        hdc: HDC,
        x1: INT, y1: INT,
        x2: INT, y2: INT,
        x3: INT, y3: INT,
        x4: INT, y4: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// ArcTo - Arc gibi yay çizer; ancak geçerli konumdan başlar ve konumu günceller
    pub unsafe fn arc_to(
        hdc: HDC,
        left: INT, top: INT,
        right: INT, bottom: INT,
        xr1: INT, yr1: INT,
        xr2: INT, yr2: INT,
    ) -> BOOL {
        TRUE
    }
    
    /// Chord - Elips üzerinde iki radyal kesimin tanımladığı kiriti çizer
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
    
    /// RoundRect - Yuvarlatilmış köşeli dikdörtgen çizer ve içini doldurur
    pub unsafe fn round_rect(hdc: HDC, left: INT, top: INT, right: INT, bottom: INT, width: INT, height: INT) -> BOOL {
        TRUE
    }
    
    /// Polygon - Verilen nokta dizisine bağlı çokgen çizer ve doldurur
    pub unsafe fn polygon(hdc: HDC, lpPoints: *const POINT, nCount: INT) -> BOOL {
        TRUE
    }
    
    /// PolyPolygon - Birden fazla çokgeni tek çağrıda çizer ve doldurur
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
    
    /// AngleArc - Merkez ve yarıçap + başla yürüme açısı ile yay çizer
    pub unsafe fn angle_arc(hdc: HDC, x: INT, y: INT, r: DWORD, StartAngle: f32, SweepAngle: f32) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // DOLU ŞEKİLLER (dikdörtgen, elips, tarama ve gradyan çizme)
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
    
    /// DrawFocusRect - Odak göstergesi olarak noktalı çerçeve çizer (sekmeli denetimler için)
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
    // BİTMAP İŞLEMLERİ (oluşturma, piksel kopyalama ve DIB biçim dönüşümleri)
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
    // FİRELER (çizim fıresi oluşturma, boyama ve sorgu işlemleri)
    // ========================================================================
    
    /// CreateSolidBrush - Düz renk ile dolu bir fıre oluşturur
    pub unsafe fn create_solid_brush(crColor: DWORD) -> HBRUSH {
        crate::serial_println!("[WIN32] CreateSolidBrush: {:08x}", crColor);
        crColor as HBRUSH
    }
    
    /// CreateHatchBrush - Belirtilen tarama desenli fıre oluşturur
    pub unsafe fn create_hatch_brush(fnStyle: INT, clrref: DWORD) -> HBRUSH {
        clrref as HBRUSH
    }
    
    /// CreatePatternBrush - Verilen bitçer tabanlı desen fıresi oluşturur
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
    
    /// GetBrushOrgEx - DC'deki geçerli fıre başlangıç koordinatlarını lppt'ye yazar
    pub unsafe fn get_brush_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
    
    /// SetBrushOrgEx - DC'deki fıre çizim başlangıç koordinatlarını ayarlar
    pub unsafe fn set_brush_org_ex(hdc: HDC, nXOrg: INT, nYOrg: INT, lppt: *mut POINT) -> BOOL {
        TRUE
    }
    
    /// GetSysColorBrush - Sistem rengi stoku fıresinin tanıtıcısını döndürür (COLOR_ sabiti ile)
    pub unsafe fn get_sys_color_brush(nIndex: INT) -> HBRUSH {
        nIndex as HBRUSH
    }
    
    // ========================================================================
    // KALEMLER (kalem oluşturma ve genişletilmiş kalem işlemleri)
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
    
    /// GetObjectA - Grafik nesnesinin bilgilerini lpvObject tamponuna yazar
    pub unsafe fn get_object_a(hgdiobj: HGDIOBJ, cbBuffer: INT, lpvObject: LPVOID) -> INT {
        0
    }
    
    /// GetObjectW - GetObjectA ile aynı; geniş karakter (Unicode) sürümñ
    pub unsafe fn get_object_w(hgdiobj: HGDIOBJ, cbBuffer: INT, lpvObject: LPVOID) -> INT {
        0
    }
    
    /// GetCurrentObject - DC'deki seçili nesnenin (fıre, kalem, font vb.) tanıtıcısını döndürür
    pub unsafe fn get_current_object(hdc: HDC, uObjectType: UINT) -> HGDIOBJ {
        1 as HGDIOBJ
    }
    
    // ========================================================================
    // FONTLAR VE METİN (yazı tipi oluşturma, ölçüm ve metin çizme)
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
    
    /// GetTextFaceA - Seçili fontu tanımlayan yüz adını lpFaceName tamponuna yazar
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
    
    /// SetTextColor - DC'deki metin çizim rengini belirtilen RGB değerine ayarlar
    pub unsafe fn set_text_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crate::serial_println!("[WIN32] SetTextColor: {:08x}", crColor);
        0
    }
    
    /// GetTextColor - DC'deki geçerli metin rengini döndürür
    pub unsafe fn get_text_color(hdc: HDC) -> DWORD {
        0
    }
    
    /// SetBkColor - Metin ve bitmap arka plan rengini ayarlar
    pub unsafe fn set_bk_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crate::serial_println!("[WIN32] SetBkColor: {:08x}", crColor);
        0
    }
    
    /// GetBkColor - DC'deki geçerli arka plan rengini döndürür
    pub unsafe fn get_bk_color(hdc: HDC) -> DWORD {
        0
    }
    
    /// SetBkMode - Metin/tarama arka plan modunu ayarlar (OPAQUE=2 veya TRANSPARENT=1)
    pub unsafe fn set_bk_mode(hdc: HDC, iBkMode: INT) -> INT {
        0
    }
    
    /// GetBkMode - DC'deki geçerli arka plan modunu döndürür
    pub unsafe fn get_bk_mode(hdc: HDC) -> INT {
        0
    }
    
    /// TextOutA - DC üzerine x,y konumundan başlayarak metin yazar
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
    
    /// ExtTextOutA - Genişletilmiş metin çizimi; kleçö dikdörtgen ve karakter aralıkları destekler
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
    
    /// DrawTextA - Dikdörtgen içine biçimlendirilmiş metin çizer (DT_ bayraklarıyla hizalama)
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
    
    /// DrawTextExA - DrawTextA genişletmesi; ek parametreler için DRAWTEXTPARAMS yapısı kabul eder
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
    
    /// TabbedTextOutA - Sekme duraklara göre hizalanmış metin yazar
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
    
    /// GetTabbedTextExtentA - Sekme duraklara göre metnin piksel boyutunu döndürür
    pub unsafe fn get_tabbed_text_extent_a(
        hdc: HDC,
        lpString: LPCSTR,
        nCount: INT,
        nTabPositions: INT,
        lpnTabStopPositions: *const INT,
    ) -> DWORD {
        0
    }
    
    /// PolyTextOutA - Birden fazla metin dizesini tek çağrıda çizer
    pub unsafe fn poly_text_out_a(hdc: HDC, ppt: *const u8, nstrings: INT) -> BOOL {
        TRUE
    }
    
    // ========================================================================
    // BÖLGELER (klipling ve çizim âlanı sınırlama nesneleri)
    // ========================================================================
    
    /// CreateRectRgn - Dikdörtgenel kırpım bölgesi oluşturur
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
    
    /// CreateRoundRectRgn - Yuvarlatilmış köşeli dikdörtgenel bölge oluşturur
    pub unsafe fn create_round_rect_rgn(left: INT, top: INT, right: INT, bottom: INT, nWidthEllipse: INT, nHeightEllipse: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreatePolygonRgn - Nokta listesiyle tanımlanan çokgenel bölge oluşturur
    pub unsafe fn create_polygon_rgn(lppt: *const POINT, cPoints: INT, fnMode: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CreatePolyPolygonRgn - Birden fazla çokgenin bileşiminden bölge oluşturur
    pub unsafe fn create_poly_polygon_rgn(lppt: *const POINT, lpPolyCounts: *const INT, nCount: INT, fnPolyFillMode: INT) -> HRGN {
        1 as HRGN
    }
    
    /// CombineRgn
    pub unsafe fn combine_rgn(hrgnDest: HRGN, hrgnSrc1: HRGN, hrgnSrc2: HRGN, fnCombineMode: INT) -> INT {
        0 // NULLREGION
    }
    
    /// OffsetRgn - Bölgeyi x ve y yönünde kaydırır
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
    
    /// FrameRgn - Bölgenin çerçevesini belirtilen fıre ve boyutla boyar
    pub unsafe fn frame_rgn(hdc: HDC, hrgn: HRGN, hbr: HBRUSH, nWidth: INT, nHeight: INT) -> BOOL {
        TRUE
    }
    
    /// GetRgnBox - Bölgenin sınır dikdörtgenini lprc'ye yazar; tür ködünü döndürür
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
    
    /// EqualRgn - İki bölgenin aynı boyut ve şekle sahip olup olmadığını kıyaslar
    pub unsafe fn equal_rgn(hrgn1: HRGN, hrgn2: HRGN) -> BOOL {
        FALSE
    }
    
    /// GetRegionData
    pub unsafe fn get_region_data(hrgn: HRGN, nCount: DWORD, lpRgnData: *mut u8) -> DWORD {
        0
    }
    
    /// SetRectRgn - Var olan bölgeyi yeni dikdörtgen koordinatlarla yeniden tanımlar
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
    
    /// ExcludeUpdateRgn - Bir pencerenin güncelleme bölgesini DC'deki klip bölgesinden çıkarır
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
    // KORDİNATLAR VE DÖNÜŞÜMLER (görüntü aynınası, pencere/ekran dönüşümleri)
    // ========================================================================
    
    /// SetMapMode - DC koordinat sistemi eşleşme modunu ayarlar (MM_TEXT, MM_LOMETRIC vb.)
    pub unsafe fn set_map_mode(hdc: HDC, iMode: INT) -> INT {
        0
    }
    
    /// GetMapMode - DC'deki geçerli koordinat eşleşme modunu döndürür
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
    
    /// DPtoLP - Cihaz piksel koordinatlarını mantıksal koordinatlara dönüştürür
    pub unsafe fn dp_to_lp(hdc: HDC, lppt: *mut POINT, c: INT) -> BOOL {
        TRUE
    }
    
    /// LPtoDP - Mantıksal koordinatları cihaz piksel koordinatlarına dönüştürür
    pub unsafe fn lp_to_dp(hdc: HDC, lppt: *mut POINT, c: INT) -> BOOL {
        TRUE
    }
    
    /// SetWorldTransform - DC'nin dünya (world) dönüşüm matrisini ayarlar
    pub unsafe fn set_world_transform(hdc: HDC, lpxf: *const u8) -> BOOL {
        TRUE
    }
    
    /// GetWorldTransform - DC'deki geçerli dünya dönüşüm matrisini lpxf yapısına yazar
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
    // RENKLER (piksel çizme ve renk sorgulama işlemleri)
    // ========================================================================
    
    /// SetPixel - Belirtilen koordinata piksel rengi çizer; gerçek rengi döndürür
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
    
    /// GetNearestColor - Verilen renge DC paletinde en yakın desteklenen rengi bulur
    pub unsafe fn get_nearest_color(hdc: HDC, crColor: DWORD) -> DWORD {
        crColor
    }
    
    /// GetNearestPaletteIndex
    pub unsafe fn get_nearest_palette_index(hpal: HPALETTE, crColor: DWORD) -> UINT {
        0
    }
    
    // ========================================================================
    // PALETLER (renk paleti oluşturma, seçim ve güncelleme işlemleri)
    // ========================================================================
    
    /// CreatePalette - Mantıksal renk paleti oluşturur; LOGPALETTE yapısından tanıtıcı döndürür
    pub unsafe fn create_palette(lplgpl: *const u8) -> HPALETTE {
        1 as HPALETTE
    }
    
    /// SelectPalette - Paleti DC'ye seçer; eski palet tanıtıcısını döndürür
    pub unsafe fn select_palette(hdc: HDC, hpal: HPALETTE, bForceBackground: BOOL) -> HPALETTE {
        hpal
    }
    
    /// RealizePalette
    pub unsafe fn realize_palette(hdc: HDC) -> UINT {
        0
    }
    
    /// UpdateColors - DC'nin piksellerini geçerli mantıksal-fiziksel palet eşleşmesiyle yeniden boyar
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
    
    /// GetSystemPaletteUse - Sistem paletinin tam kullanım durumunu sorgular
    pub unsafe fn get_system_palette_use(hdc: HDC) -> UINT {
        1 // SYSPAL_STATIC
    }
    
    /// SetSystemPaletteUse - Sistem paletinin tam ya da statik kullanım modunu ayarlar
    pub unsafe fn set_system_palette_use(hdc: HDC, uiUsage: UINT) -> UINT {
        1
    }
    
    // ========================================================================
    // YOLLAR (path oluşturma, kontrol noktası ve konturdan bölge üretme)
    // ========================================================================
    
    /// BeginPath - DC için yol toplama modunu başlatır
    pub unsafe fn begin_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// EndPath - Yol toplama modunu sona erdirir; yolu DC'nin geçerli yolu yapar
    pub unsafe fn end_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// AbortPath - Yol toplama modunu iptal eder ve yolu atar
    pub unsafe fn abort_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// CloseFigure
    pub unsafe fn close_figure(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// FlattenPath - Yoldaki eğri bileşenleri doğru parçalara dönüştürür
    pub unsafe fn flatten_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// WidenPath
    pub unsafe fn widen_path(hdc: HDC) -> BOOL {
        TRUE
    }
    
    /// StrokePath - Geçerli yolun konturunu seçili kalemle çizer
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
    
    /// GetPath - Yoldaki noktaları ve tür kodlarını dizilere kopyalar
    pub unsafe fn get_path(hdc: HDC, lppt: *mut POINT, lpbTypes: *mut BYTE, nSize: INT) -> INT {
        -1
    }
    
    // ========================================================================
    // ÇEŞİTLİ GDİ (çizim durumu kaydetme, mod alma/ayarlama)
    // ========================================================================
    
    /// SaveDC - DC'nin geçerli durumunu bir yığa kaydeder; kaydetme kimliğini döndürür
    pub unsafe fn save_dc(hdc: HDC) -> INT {
        1
    }
    
    /// RestoreDC - SaveDC ile kaydedilmiş DC durumunu geri yükler
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
    
    /// GetStretchBltMode - DC'deki geçerli esneme blit modunu döndürür
    pub unsafe fn get_stretch_blt_mode(hdc: HDC) -> INT {
        1 // WHITEONBLACK
    }
    
    /// SetStretchBltMode - Gerilen blit işleminin renk alma algoritmasını ayarlar
    pub unsafe fn set_stretch_blt_mode(hdc: HDC, iStretchMode: INT) -> INT {
        1
    }
    
    /// GetROP2 - DC'deki geçerli ikili raster işlem (ROP2) çizim modunu döndürür
    pub unsafe fn get_rop2(hdc: HDC) -> INT {
        13 // R2_COPYPEN
    }
    
    /// SetROP2 - Çizim işlemlerinde kullanılacak ikili raster işlem modunu ayarlar
    pub unsafe fn set_rop2(hdc: HDC, fnDrawMode: INT) -> INT {
        13
    }
    
    /// GetDCOrgEx - DC'nin ekran koordinatlarına göre başlangıç noktasını lppt'ye yazar
    pub unsafe fn get_dc_org_ex(hdc: HDC, lppt: *mut POINT) -> BOOL {
        if !lppt.is_null() {
            (*lppt).x = 0;
            (*lppt).y = 0;
        }
        TRUE
    }
}

// ============================================================================
// APİ TABLOSU (her modül için işlev isim → taslak işaretçi eşleşmesi)
// ============================================================================

/// Win32 API tablosunu başlat - modül isimlerini fonksiyon eşleştirmelerine doldurur
fn init_api_table() -> BTreeMap<String, BTreeMap<String, Win32ApiFn>> {
    let mut table: BTreeMap<String, BTreeMap<String, Win32ApiFn>> = BTreeMap::new();
    
    // kernel32 (çekirdek API'leri)
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
    // Süreç yönetimi
    kernel32_funcs.insert("CreateProcessA".to_string(), stub_api);
    kernel32_funcs.insert("OpenProcess".to_string(), stub_api);
    kernel32_funcs.insert("TerminateProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetExitCodeProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentProcess".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentProcessId".to_string(), stub_api);
    // İş parçacığı yönetimi
    kernel32_funcs.insert("CreateThread".to_string(), stub_api);
    kernel32_funcs.insert("ExitThread".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentThread".to_string(), stub_api);
    kernel32_funcs.insert("GetCurrentThreadId".to_string(), stub_api);
    kernel32_funcs.insert("GetExitCodeThread".to_string(), stub_api);
    kernel32_funcs.insert("GetProcessId".to_string(), stub_api);
    kernel32_funcs.insert("GetThreadId".to_string(), stub_api);
    kernel32_funcs.insert("ResumeThread".to_string(), stub_api);
    kernel32_funcs.insert("SuspendThread".to_string(), stub_api);
    kernel32_funcs.insert("WaitForSingleObject".to_string(), stub_api);
    kernel32_funcs.insert("WaitForMultipleObjects".to_string(), stub_api);
    // Heap bellek yönetimi
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
    // Dosya işlemleri
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
    // Konsol
    kernel32_funcs.insert("GetStdHandle".to_string(), stub_api);
    kernel32_funcs.insert("SetStdHandle".to_string(), stub_api);
    kernel32_funcs.insert("WriteConsoleA".to_string(), stub_api);
    kernel32_funcs.insert("ReadConsoleA".to_string(), stub_api);
    kernel32_funcs.insert("SetConsoleMode".to_string(), stub_api);
    kernel32_funcs.insert("GetConsoleMode".to_string(), stub_api);
    kernel32_funcs.insert("SetConsoleTextAttribute".to_string(), stub_api);
    kernel32_funcs.insert("GetConsoleScreenBufferInfo".to_string(), stub_api);
    kernel32_funcs.insert("FillConsoleOutputCharacterA".to_string(), stub_api);
    // Ortam değişkenleri
    kernel32_funcs.insert("GetEnvironmentVariableA".to_string(), stub_api);
    kernel32_funcs.insert("SetEnvironmentVariableA".to_string(), stub_api);
    kernel32_funcs.insert("GetCommandLineA".to_string(), stub_api);
    // Sistem bilgisi
    kernel32_funcs.insert("GetSystemInfo".to_string(), stub_api);
    kernel32_funcs.insert("GlobalMemoryStatus".to_string(), stub_api);
    kernel32_funcs.insert("GlobalMemoryStatusEx".to_string(), stub_api);
    kernel32_funcs.insert("GetVersion".to_string(), stub_api);
    kernel32_funcs.insert("GetVersionExA".to_string(), stub_api);
    kernel32_funcs.insert("GetComputerNameA".to_string(), stub_api);
    kernel32_funcs.insert("GetUserNameA".to_string(), stub_api);
    kernel32_funcs.insert("GetLastError".to_string(), stub_api);
    kernel32_funcs.insert("SetLastError".to_string(), stub_api);
    // Dizi işlemleri (kernel32)
    kernel32_funcs.insert("MultiByteToWideChar".to_string(), stub_api);
    kernel32_funcs.insert("WideCharToMultiByte".to_string(), stub_api);
    kernel32_funcs.insert("lstrlenA".to_string(), stub_api);
    kernel32_funcs.insert("lstrlenW".to_string(), stub_api);
    kernel32_funcs.insert("lstrcpyA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcatA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcmpA".to_string(), stub_api);
    kernel32_funcs.insert("lstrcmpiA".to_string(), stub_api);
    table.insert("kernel32".to_string(), kernel32_funcs);
    
    // user32 (pencere yönetimi ve kullanıcı arayüz API'leri)
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
    user32_funcs.insert("EnumChildWindows".to_string(), stub_api);
    user32_funcs.insert("EnumThreadWindows".to_string(), stub_api);
    user32_funcs.insert("GetClassNameA".to_string(), stub_api);
    // Klavye girişi
    user32_funcs.insert("GetKeyState".to_string(), stub_api);
    user32_funcs.insert("GetAsyncKeyState".to_string(), stub_api);
    user32_funcs.insert("GetKeyboardState".to_string(), stub_api);
    user32_funcs.insert("SetKeyboardState".to_string(), stub_api);
    user32_funcs.insert("keybd_event".to_string(), stub_api);
    user32_funcs.insert("MapVirtualKeyA".to_string(), stub_api);
    user32_funcs.insert("ToAscii".to_string(), stub_api);
    user32_funcs.insert("VkKeyScanA".to_string(), stub_api);
    // Fare girişi
    user32_funcs.insert("GetCursorPos".to_string(), stub_api);
    user32_funcs.insert("SetCursorPos".to_string(), stub_api);
    user32_funcs.insert("mouse_event".to_string(), stub_api);
    user32_funcs.insert("GetDoubleClickTime".to_string(), stub_api);
    user32_funcs.insert("SwapMouseButton".to_string(), stub_api);
    user32_funcs.insert("GetSystemMetrics".to_string(), stub_api);
    // Menüler
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
    // İletişim Kutuları
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
    // Zamanlayıcılar
    user32_funcs.insert("SetTimer".to_string(), stub_api);
    user32_funcs.insert("KillTimer".to_string(), stub_api);
    // Pano (Clipboard)
    user32_funcs.insert("OpenClipboard".to_string(), stub_api);
    user32_funcs.insert("CloseClipboard".to_string(), stub_api);
    user32_funcs.insert("EmptyClipboard".to_string(), stub_api);
    user32_funcs.insert("GetClipboardData".to_string(), stub_api);
    user32_funcs.insert("SetClipboardData".to_string(), stub_api);
    user32_funcs.insert("IsClipboardFormatAvailable".to_string(), stub_api);
    // Kaynaklar (ikon, imleç, bitmap)
    user32_funcs.insert("LoadIconA".to_string(), stub_api);
    user32_funcs.insert("LoadCursorA".to_string(), stub_api);
    user32_funcs.insert("LoadBitmapA".to_string(), stub_api);
    user32_funcs.insert("LoadStringA".to_string(), stub_api);
    user32_funcs.insert("LoadImageA".to_string(), stub_api);
    user32_funcs.insert("DestroyIcon".to_string(), stub_api);
    user32_funcs.insert("DestroyCursor".to_string(), stub_api);
    user32_funcs.insert("SetCursor".to_string(), stub_api);
    user32_funcs.insert("GetCursor".to_string(), stub_api);
    // Kancalar (işitim kanaları)
    user32_funcs.insert("SetWindowsHookExA".to_string(), stub_api);
    user32_funcs.insert("UnhookWindowsHookEx".to_string(), stub_api);
    user32_funcs.insert("CallNextHookEx".to_string(), stub_api);
    // Çeşitli (user32)
    user32_funcs.insert("GetWindowLongA".to_string(), stub_api);
    user32_funcs.insert("SetWindowLongA".to_string(), stub_api);
    user32_funcs.insert("GetWindowLongPtrA".to_string(), stub_api);
    user32_funcs.insert("SetWindowLongPtrA".to_string(), stub_api);
    user32_funcs.insert("GetClassLongA".to_string(), stub_api);
    user32_funcs.insert("SetClassLongA".to_string(), stub_api);
    user32_funcs.insert("GetPropA".to_string(), stub_api);
    user32_funcs.insert("SetPropA".to_string(), stub_api);
    user32_funcs.insert("RemovePropA".to_string(), stub_api);
    user32_funcs.insert("EnumPropsA".to_string(), stub_api);
    user32_funcs.insert("GetWindowThreadProcessId".to_string(), stub_api);
    user32_funcs.insert("AttachThreadInput".to_string(), stub_api);
    table.insert("user32".to_string(), user32_funcs);
    
    // gdi32 (grafik cihaz arayüzü API'leri)
    let mut gdi32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // DC yönetimi
    gdi32_funcs.insert("CreateCompatibleDC".to_string(), stub_api);
    gdi32_funcs.insert("DeleteDC".to_string(), stub_api);
    gdi32_funcs.insert("SaveDC".to_string(), stub_api);
    gdi32_funcs.insert("RestoreDC".to_string(), stub_api);
    // GDI nesneleri
    gdi32_funcs.insert("SelectObject".to_string(), stub_api);
    gdi32_funcs.insert("DeleteObject".to_string(), stub_api);
    gdi32_funcs.insert("GetStockObject".to_string(), stub_api);
    gdi32_funcs.insert("GetObjectA".to_string(), stub_api);
    gdi32_funcs.insert("GetCurrentObject".to_string(), stub_api);
    // Çizim işlemleri
    gdi32_funcs.insert("MoveToEx".to_string(), stub_api);
    gdi32_funcs.insert("LineTo".to_string(), stub_api);
    gdi32_funcs.insert("Polyline".to_string(), stub_api);
    gdi32_funcs.insert("Arc".to_string(), stub_api);
    gdi32_funcs.insert("Ellipse".to_string(), stub_api);
    gdi32_funcs.insert("Rectangle".to_string(), stub_api);
    gdi32_funcs.insert("RoundRect".to_string(), stub_api);
    gdi32_funcs.insert("Polygon".to_string(), stub_api);
    gdi32_funcs.insert("PolyBezier".to_string(), stub_api);
    // Dolu şekiller
    gdi32_funcs.insert("FillRect".to_string(), stub_api);
    gdi32_funcs.insert("FrameRect".to_string(), stub_api);
    gdi32_funcs.insert("InvertRect".to_string(), stub_api);
    gdi32_funcs.insert("FloodFill".to_string(), stub_api);
    gdi32_funcs.insert("GradientFill".to_string(), stub_api);
    // Bitmapler
    gdi32_funcs.insert("CreateBitmap".to_string(), stub_api);
    gdi32_funcs.insert("CreateCompatibleBitmap".to_string(), stub_api);
    gdi32_funcs.insert("GetBitmapBits".to_string(), stub_api);
    gdi32_funcs.insert("SetBitmapBits".to_string(), stub_api);
    gdi32_funcs.insert("GetDIBits".to_string(), stub_api);
    gdi32_funcs.insert("SetDIBits".to_string(), stub_api);
    gdi32_funcs.insert("CreateDIBSection".to_string(), stub_api);
    // Piksel aktarımı (Blitting)
    gdi32_funcs.insert("BitBlt".to_string(), stub_api);
    gdi32_funcs.insert("StretchBlt".to_string(), stub_api);
    gdi32_funcs.insert("StretchDIBits".to_string(), stub_api);
    gdi32_funcs.insert("PatBlt".to_string(), stub_api);
    // Fıreler
    gdi32_funcs.insert("CreateSolidBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreateHatchBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreatePatternBrush".to_string(), stub_api);
    gdi32_funcs.insert("CreateBrushIndirect".to_string(), stub_api);
    gdi32_funcs.insert("GetSysColorBrush".to_string(), stub_api);
    // Kalemler
    gdi32_funcs.insert("CreatePen".to_string(), stub_api);
    gdi32_funcs.insert("CreatePenIndirect".to_string(), stub_api);
    gdi32_funcs.insert("ExtCreatePen".to_string(), stub_api);
    // Fontlar ve metin
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
    // Bölgeler (gdi32 tablo)
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
    // Kırpma işlemleri
    gdi32_funcs.insert("SelectClipRgn".to_string(), stub_api);
    gdi32_funcs.insert("ExcludeClipRect".to_string(), stub_api);
    gdi32_funcs.insert("IntersectClipRect".to_string(), stub_api);
    gdi32_funcs.insert("GetClipBox".to_string(), stub_api);
    gdi32_funcs.insert("PtVisible".to_string(), stub_api);
    gdi32_funcs.insert("RectVisible".to_string(), stub_api);
    // Koordinatlar
    gdi32_funcs.insert("SetMapMode".to_string(), stub_api);
    gdi32_funcs.insert("GetMapMode".to_string(), stub_api);
    gdi32_funcs.insert("SetViewportOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("GetViewportOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("SetWindowOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("GetWindowOrgEx".to_string(), stub_api);
    gdi32_funcs.insert("DPtoLP".to_string(), stub_api);
    gdi32_funcs.insert("LPtoDP".to_string(), stub_api);
    gdi32_funcs.insert("SetWorldTransform".to_string(), stub_api);
    // Renkler
    gdi32_funcs.insert("SetPixel".to_string(), stub_api);
    gdi32_funcs.insert("SetPixelV".to_string(), stub_api);
    gdi32_funcs.insert("GetPixel".to_string(), stub_api);
    gdi32_funcs.insert("GetNearestColor".to_string(), stub_api);
    // Paletler
    gdi32_funcs.insert("CreatePalette".to_string(), stub_api);
    gdi32_funcs.insert("SelectPalette".to_string(), stub_api);
    gdi32_funcs.insert("RealizePalette".to_string(), stub_api);
    gdi32_funcs.insert("UpdateColors".to_string(), stub_api);
    gdi32_funcs.insert("GetPaletteEntries".to_string(), stub_api);
    // Yollar
    gdi32_funcs.insert("BeginPath".to_string(), stub_api);
    gdi32_funcs.insert("EndPath".to_string(), stub_api);
    gdi32_funcs.insert("AbortPath".to_string(), stub_api);
    gdi32_funcs.insert("CloseFigure".to_string(), stub_api);
    gdi32_funcs.insert("FlattenPath".to_string(), stub_api);
    gdi32_funcs.insert("StrokePath".to_string(), stub_api);
    gdi32_funcs.insert("FillPath".to_string(), stub_api);
    gdi32_funcs.insert("PathToRegion".to_string(), stub_api);
    gdi32_funcs.insert("GetPath".to_string(), stub_api);
    // Çeşitli GDI
    gdi32_funcs.insert("GetGraphicsMode".to_string(), stub_api);
    gdi32_funcs.insert("SetGraphicsMode".to_string(), stub_api);
    gdi32_funcs.insert("GetPolyFillMode".to_string(), stub_api);
    gdi32_funcs.insert("SetPolyFillMode".to_string(), stub_api);
    gdi32_funcs.insert("GetStretchBltMode".to_string(), stub_api);
    gdi32_funcs.insert("SetStretchBltMode".to_string(), stub_api);
    gdi32_funcs.insert("GetROP2".to_string(), stub_api);
    gdi32_funcs.insert("SetROP2".to_string(), stub_api);
    gdi32_funcs.insert("GetCurrentPositionEx".to_string(), stub_api);
    // OpenGL (piksel biçimi ve tampon değiştirme)
    gdi32_funcs.insert("ChoosePixelFormat".to_string(), stub_api);
    gdi32_funcs.insert("SetPixelFormat".to_string(), stub_api);
    gdi32_funcs.insert("SwapBuffers".to_string(), stub_api);
    table.insert("gdi32".to_string(), gdi32_funcs);
    
    // advapi32 (gelişmiş Windows API'leri: kayıt defteri, güvenlik, servisler)
    let mut advapi32_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // Kayıt defteri
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
    // Güvenlik
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
    // Servisler
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
    // Olay Günlüğü
    advapi32_funcs.insert("RegisterEventSourceA".to_string(), stub_api);
    advapi32_funcs.insert("DeregisterEventSource".to_string(), stub_api);
    advapi32_funcs.insert("ReportEventA".to_string(), stub_api);
    advapi32_funcs.insert("OpenEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("CloseEventLog".to_string(), stub_api);
    advapi32_funcs.insert("ClearEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("ReadEventLogA".to_string(), stub_api);
    advapi32_funcs.insert("GetNumberOfEventLogRecords".to_string(), stub_api);
    // Kriptografi
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
    // Süreç ve Güvenlik Belirteci
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
    
    // msvcrt (Microsoft C çalışma zamanı kütüphanesi)
    let mut msvcrt_funcs: BTreeMap<String, Win32ApiFn> = BTreeMap::new();
    // Bellek işlemleri
    msvcrt_funcs.insert("malloc".to_string(), stub_api);
    msvcrt_funcs.insert("free".to_string(), stub_api);
    msvcrt_funcs.insert("calloc".to_string(), stub_api);
    msvcrt_funcs.insert("realloc".to_string(), stub_api);
    msvcrt_funcs.insert("_msize".to_string(), stub_api);
    msvcrt_funcs.insert("_expand".to_string(), stub_api);
    msvcrt_funcs.insert("_heapmin".to_string(), stub_api);
    // Dizi işlemleri
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
    // Giriş/çıkış
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
    // Matematik
    msvcrt_funcs.insert("abs".to_string(), stub_api);
    msvcrt_funcs.insert("labs".to_string(), stub_api);
    msvcrt_funcs.insert("rand".to_string(), stub_api);
    msvcrt_funcs.insert("srand".to_string(), stub_api);
    // Zaman
    msvcrt_funcs.insert("time".to_string(), stub_api);
    msvcrt_funcs.insert("clock".to_string(), stub_api);
    msvcrt_funcs.insert("localtime".to_string(), stub_api);
    msvcrt_funcs.insert("gmtime".to_string(), stub_api);
    msvcrt_funcs.insert("asctime".to_string(), stub_api);
    msvcrt_funcs.insert("ctime".to_string(), stub_api);
    msvcrt_funcs.insert("strftime".to_string(), stub_api);
    // Çeşitli
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

/// Taslak API fonksiyonu - henüz uygulanmamış Win32 API çağrıları için
fn stub_api(_args: *const u8) -> isize {
    crate::serial_println!("[WIN32] Stub API called");
    0
}

// ============================================================================
// GLOBAL APİ TABLOSU
// ============================================================================

static WIN32_API_TABLE: Mutex<Option<BTreeMap<String, BTreeMap<String, Win32ApiFn>>>> = Mutex::new(None);

/// Win32 alt sistemini başlat - API tablosunu oluşturur
pub fn init() {
    let mut table = WIN32_API_TABLE.lock();
    *table = Some(init_api_table());
    crate::serial_println!("[WIN32] API emulation initialized");
}

/// get_proc_address - Modül ve fonksiyon adıyla Win32 APİ fonksiyon adresini sorgular
pub fn get_proc_address(module: &str, func: &str) -> Option<u64> {
    let table = WIN32_API_TABLE.lock();
    if let Some(ref t) = *table {
        if let Some(module_funcs) = t.get(module) {
            if module_funcs.contains_key(func) {
                // Sahte adres döndürülüyor (gerçek uygulamada gerçek fonksiyon işaretçisi dönecek)
                return Some(0xDEADBEEF);
            }
        }
    }
    None
}

/// get_proc_address_internal - İç kullanım için FARPROC döndüren APİ arama yardımcısı
pub fn get_proc_address_internal(module: &str, func: &str) -> FARPROC {
    if get_proc_address(module, func).is_some() {
        // Taslak fonksiyon işaretçisi döndürülüyor
        // Gerçek uygulamada gerçek fonksiyon işaretçisi döndürülecek
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    #[repr(C)]
    struct ProcessInformation {
        h_process: HANDLE,
        h_thread: HANDLE,
        process_id: DWORD,
        thread_id: DWORD,
    }

    unsafe extern "system" fn count_windows_callback(_hwnd: HWND, lparam: usize) -> BOOL {
        let counter = &*(lparam as *const AtomicU32);
        counter.fetch_add(1, Ordering::Relaxed);
        TRUE
    }

    #[test]
    fn user32_window_state_and_enumeration_interop() {
        unsafe {
            let class_name = b"InteropClass\0";
            let parent_title = b"ParentWindow\0";
            let child_title = b"ChildWindow\0";
            let prop_name = b"interop.prop\0";

            let parent = user32::create_window_ex_a(
                0,
                class_name.as_ptr() as LPCSTR,
                parent_title.as_ptr() as LPCSTR,
                WS_VISIBLE,
                0,
                0,
                640,
                480,
                0,
                0,
                0,
                core::ptr::null_mut(),
            );
            assert_ne!(parent, 0);

            let child = user32::create_window_ex_a(
                0,
                class_name.as_ptr() as LPCSTR,
                child_title.as_ptr() as LPCSTR,
                WS_VISIBLE | WS_CHILD,
                8,
                8,
                320,
                200,
                parent,
                0,
                0,
                core::ptr::null_mut(),
            );
            assert_ne!(child, 0);

            assert_eq!(user32::set_window_long_ptr_a(parent, 0, 0x1234), 0);
            assert_eq!(user32::get_window_long_ptr_a(parent, 0), 0x1234);

            assert_eq!(user32::set_class_long_a(parent, 1, 0xABCD), 0);
            assert_eq!(user32::get_class_long_a(parent, 1), 0xABCD);

            assert_eq!(user32::set_prop_a(parent, prop_name.as_ptr() as LPCSTR, 0x55AA), TRUE);
            assert_eq!(user32::get_prop_a(parent, prop_name.as_ptr() as LPCSTR), 0x55AA);
            assert_eq!(user32::remove_prop_a(parent, prop_name.as_ptr() as LPCSTR), 0x55AA);
            assert_eq!(user32::get_prop_a(parent, prop_name.as_ptr() as LPCSTR), 0);

            let mut pid = 0;
            let tid = user32::get_window_thread_process_id(parent, &mut pid as *mut DWORD);
            assert_ne!(tid, 0);
            assert_ne!(pid, 0);

            let child_count = AtomicU32::new(0);
            assert_eq!(
                user32::enum_child_windows(parent, Some(count_windows_callback), &child_count as *const AtomicU32 as usize),
                TRUE
            );
            assert!(child_count.load(Ordering::Relaxed) >= 1);

            let thread_count = AtomicU32::new(0);
            assert_eq!(
                user32::enum_thread_windows(tid, Some(count_windows_callback), &thread_count as *const AtomicU32 as usize),
                TRUE
            );
            assert!(thread_count.load(Ordering::Relaxed) >= 1);
        }
    }

    #[test]
    fn kernel32_process_wait_lifecycle_interop() {
        unsafe {
            let app = b"interop.exe\0";
            let mut info = ProcessInformation {
                h_process: 0,
                h_thread: 0,
                process_id: 0,
                thread_id: 0,
            };

            assert_eq!(
                kernel32::create_process_a(
                    app.as_ptr() as LPCSTR,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    FALSE,
                    0,
                    core::ptr::null_mut(),
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    &mut info as *mut ProcessInformation as LPVOID,
                ),
                TRUE
            );

            assert_ne!(info.h_process, 0);
            assert_ne!(info.h_thread, 0);
            assert_ne!(info.process_id, 0);
            assert_ne!(info.thread_id, 0);

            let mut exit_code = 0;
            assert_eq!(kernel32::get_exit_code_process(info.h_process, &mut exit_code as *mut DWORD), TRUE);
            assert_eq!(exit_code, 259);
            assert_eq!(kernel32::wait_for_single_object(info.h_process, 0), 258);

            assert_eq!(kernel32::terminate_process(info.h_process, 77), TRUE);
            assert_eq!(kernel32::wait_for_single_object(info.h_process, 0), 0);
            assert_eq!(kernel32::get_exit_code_process(info.h_process, &mut exit_code as *mut DWORD), TRUE);
            assert_eq!(exit_code, 77);

            assert_eq!(kernel32::close_handle(info.h_thread), TRUE);
            assert_eq!(kernel32::close_handle(info.h_process), TRUE);
        }
    }

    #[test]
    fn advapi32_create_process_bridge_interop() {
        unsafe {
            let app = b"secure_interop.exe\0";
            let mut info = ProcessInformation {
                h_process: 0,
                h_thread: 0,
                process_id: 0,
                thread_id: 0,
            };

            assert_eq!(
                advapi32::create_process_as_user_a(
                    1,
                    app.as_ptr() as LPCSTR,
                    core::ptr::null_mut(),
                    core::ptr::null(),
                    core::ptr::null(),
                    FALSE,
                    0,
                    core::ptr::null(),
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    &mut info as *mut ProcessInformation as *mut u8,
                ),
                TRUE
            );

            assert_ne!(info.h_process, 0);
            assert_ne!(info.h_thread, 0);
            assert_ne!(info.process_id, 0);
            assert_ne!(info.thread_id, 0);
        }
    }
}
