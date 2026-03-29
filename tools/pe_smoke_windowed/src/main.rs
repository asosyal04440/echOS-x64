#![no_std]
#![no_main]

use core::ffi::{c_char, c_void};
use core::panic::PanicInfo;
use core::ptr::{null, null_mut};

type Bool = i32;
type Dword = u32;
type Handle = isize;
type Hinstance = isize;
type Hicon = isize;
type Hcursor = isize;
type Hbrush = isize;
type Hwnd = isize;
type Hmenu = isize;
type Lparam = isize;
type Lresult = isize;
type Lpvoid = *mut c_void;
type Uint = u32;
type Wparam = usize;

const SW_SHOW: i32 = 5;
const WM_DESTROY: Uint = 0x0002;
const WM_CLOSE: Uint = 0x0010;
const WAIT_OBJECT_0: Dword = 0;
const TLS_OUT_OF_INDEXES: Dword = 0xFFFF_FFFF;

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: Uint,
    w_param: Wparam,
    l_param: Lparam,
    time: Dword,
    point: Point,
}

type WndProc = unsafe extern "system" fn(Hwnd, Uint, Wparam, Lparam) -> Lresult;

#[repr(C)]
struct WndclassA {
    style: Uint,
    lpfn_wnd_proc: Option<WndProc>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hinstance,
    h_icon: Hicon,
    h_cursor: Hcursor,
    hbr_background: Hbrush,
    lpsz_menu_name: *const c_char,
    lpsz_class_name: *const c_char,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExitProcess(code: Uint) -> !;
    fn GetProcessHeap() -> Handle;
    fn CreateThread(
        attributes: Lpvoid,
        stack_size: usize,
        start: Option<unsafe extern "system" fn(Lpvoid) -> Dword>,
        parameter: Lpvoid,
        creation_flags: Dword,
        thread_id: *mut Dword,
    ) -> Handle;
    fn WaitForSingleObject(handle: Handle, milliseconds: Dword) -> Dword;
    fn CloseHandle(handle: Handle) -> Bool;
    fn TlsAlloc() -> Dword;
    fn TlsSetValue(index: Dword, value: Lpvoid) -> Bool;
    fn TlsGetValue(index: Dword) -> Lpvoid;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassA(class: *const WndclassA) -> u16;
    fn CreateWindowExA(
        ex_style: Dword,
        class_name: *const c_char,
        window_name: *const c_char,
        style: Dword,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Hmenu,
        instance: Hinstance,
        param: Lpvoid,
    ) -> Hwnd;
    fn ShowWindow(hwnd: Hwnd, cmd_show: i32) -> Bool;
    fn UpdateWindow(hwnd: Hwnd) -> Bool;
    fn GetMessageA(msg: *mut Msg, hwnd: Hwnd, min: Uint, max: Uint) -> i32;
    fn TranslateMessage(msg: *const Msg) -> Bool;
    fn DispatchMessageA(msg: *const Msg) -> Lresult;
    fn DefWindowProcA(hwnd: Hwnd, msg: Uint, w_param: Wparam, l_param: Lparam) -> Lresult;
    fn PostQuitMessage(exit_code: i32);
    fn PostMessageA(hwnd: Hwnd, msg: Uint, w_param: Wparam, l_param: Lparam) -> Bool;
    fn DestroyWindow(hwnd: Hwnd) -> Bool;
}

static CLASS_NAME: &[u8] = b"echos_pe_windowed_smoke\0";
static WINDOW_TITLE: &[u8] = b"echOS PE Windowed Smoke\0";
static mut TLS_SLOT: Dword = TLS_OUT_OF_INDEXES;
static mut MAIN_WINDOW: Hwnd = 0;

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    unsafe { ExitProcess(250) }
}

unsafe extern "system" fn worker_thread(_: Lpvoid) -> Dword {
    let token = 0xBADC0DEusize as Lpvoid;
    if TLS_SLOT == TLS_OUT_OF_INDEXES {
        ExitProcess(221);
    }
    if TlsSetValue(TLS_SLOT, token) == 0 {
        ExitProcess(222);
    }
    if TlsGetValue(TLS_SLOT) != token {
        ExitProcess(223);
    }
    if MAIN_WINDOW == 0 {
        ExitProcess(224);
    }
    if PostMessageA(MAIN_WINDOW, WM_CLOSE, 0, 0) == 0 {
        ExitProcess(225);
    }
    0
}

unsafe extern "system" fn smoke_wnd_proc(
    hwnd: Hwnd,
    msg: Uint,
    w_param: Wparam,
    l_param: Lparam,
) -> Lresult {
    match msg {
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcA(hwnd, msg, w_param, l_param),
    }
}

#[export_name = "mainCRTStartup"]
pub unsafe extern "C" fn main_crt_startup() -> ! {
    if GetProcessHeap() == 0 {
        ExitProcess(201);
    }

    TLS_SLOT = TlsAlloc();
    if TLS_SLOT == TLS_OUT_OF_INDEXES {
        ExitProcess(202);
    }

    let class = WndclassA {
        style: 0,
        lpfn_wnd_proc: Some(smoke_wnd_proc),
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: 0,
        h_icon: 0,
        h_cursor: 0,
        hbr_background: 0,
        lpsz_menu_name: null(),
        lpsz_class_name: CLASS_NAME.as_ptr() as *const c_char,
    };
    if RegisterClassA(&class) == 0 {
        ExitProcess(203);
    }

    let hwnd = CreateWindowExA(
        0,
        CLASS_NAME.as_ptr() as *const c_char,
        WINDOW_TITLE.as_ptr() as *const c_char,
        0x10CF0000,
        96,
        96,
        720,
        420,
        0,
        0,
        0,
        null_mut(),
    );
    if hwnd == 0 {
        ExitProcess(204);
    }
    MAIN_WINDOW = hwnd;
    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);

    let thread = CreateThread(
        null_mut(),
        0,
        Some(worker_thread),
        null_mut(),
        0,
        null_mut(),
    );
    if thread == 0 {
        ExitProcess(205);
    }

    let mut msg = Msg {
        hwnd: 0,
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point { x: 0, y: 0 },
    };
    loop {
        let rc = GetMessageA(&mut msg, 0, 0, 0);
        if rc == 0 {
            break;
        }
        if rc < 0 {
            ExitProcess(206);
        }
        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }

    if WaitForSingleObject(thread, 5000) != WAIT_OBJECT_0 {
        ExitProcess(207);
    }
    if CloseHandle(thread) == 0 {
        ExitProcess(208);
    }
    ExitProcess(0)
}
