//! echsh - echOS DRM/Virtgpu Aygıt Test Modülü
//!
//! Bu dosya; kullanıcı alanından DRM (Direct Rendering Manager) ioctl çağrıları
//! aracılığıyla virtio-gpu aygıtını sorgulayan ve 64x64 boyutunda bir GPU kaynağı
//! oluşturarak DRM alt sistemini test eden bir uygulamadır.
//! Linux DRM ioctl kodlama formülünü (IOC_NRSHIFT, IOC_SIZEBITS vb.) birebir uygular.

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_IOCTL: usize = 16;
const SYS_EXIT: usize = 60;

const IOC_NRBITS: usize = 8;
const IOC_TYPEBITS: usize = 8;
const IOC_SIZEBITS: usize = 14;
const IOC_NRSHIFT: usize = 0;
const IOC_TYPESHIFT: usize = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: usize = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: usize = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_WRITE: usize = 1;
const IOC_READ: usize = 2;
const DRM_IOCTL_BASE: u8 = b'd';

const fn ioc(dir: usize, type_: usize, nr: usize, size: usize) -> usize {
    (dir << IOC_DIRSHIFT) | (type_ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT)
}

const fn iowr<T>(nr: usize) -> usize {
    ioc(IOC_READ | IOC_WRITE, DRM_IOCTL_BASE as usize, nr, core::mem::size_of::<T>())
}

#[repr(C)]
struct DrmVersion {
    version_major: i32,
    version_minor: i32,
    version_patchlevel: i32,
    name_len: usize,
    name: usize,
    date_len: usize,
    date: usize,
    desc_len: usize,
    desc: usize,
}

#[repr(C)]
struct DrmVirtgpuResourceCreate {
    handle: u32,
    target: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
    size: u32,
    stride: u32,
}

const DRM_IOCTL_VERSION: usize = iowr::<DrmVersion>(0x00);
const DRM_IOCTL_VIRTGPU_RESOURCE_CREATE: usize = iowr::<DrmVirtgpuResourceCreate>(0xC0);

static DRM_PATH: &[u8; 15] = b"/dev/dri/card0\0";

unsafe fn syscall3(n: usize, a: usize, b: usize, c: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        in("rsi") b as isize,
        in("rdx") c as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

unsafe fn syscall1(n: usize, a: usize) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a as isize,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}

fn write_all(bytes: &[u8]) {
    unsafe {
        let _ = syscall3(SYS_WRITE, 1, bytes.as_ptr() as usize, bytes.len());
    }
}

fn write_str(value: &str) {
    write_all(value.as_bytes());
}

fn write_i32(value: i32) {
    let mut tmp = [0u8; 12];
    let mut len = 0usize;
    let v = value;
    if v == 0 {
        tmp[0] = b'0';
        len = 1;
    } else {
        let neg = v < 0;
        let mut n = if neg { v.wrapping_neg() as u32 } else { v as u32 };
        while n > 0 {
            tmp[len] = b'0' + (n % 10) as u8;
            len += 1;
            n /= 10;
        }
        if neg {
            tmp[len] = b'-';
            len += 1;
        }
    }
    let mut out = [0u8; 12];
    for i in 0..len {
        out[i] = tmp[len - 1 - i];
    }
    write_all(&out[..len]);
}

fn name_matches(buf: &[u8], expected: &[u8]) -> bool {
    let mut len = 0usize;
    while len < buf.len() && buf[len] != 0 {
        len += 1;
    }
    if len != expected.len() {
        return false;
    }
    let mut i = 0usize;
    while i < len {
        if buf[i] != expected[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn exit(code: i32) -> ! {
    unsafe {
        let _ = syscall1(SYS_EXIT, code as usize);
    }
    loop {}
}

fn panic_exit(message: &str) -> ! {
    write_str(message);
    write_str("\n");
    exit(1);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let fd = unsafe { syscall3(SYS_OPEN, DRM_PATH.as_ptr() as usize, 0, 0) };
    if fd < 0 {
        panic_exit("open failed");
    }
    let fd = fd as usize;
    let mut name_buf = [0u8; 64];
    let mut version = DrmVersion {
        version_major: 0,
        version_minor: 0,
        version_patchlevel: 0,
        name_len: name_buf.len(),
        name: name_buf.as_mut_ptr() as usize,
        date_len: 0,
        date: 0,
        desc_len: 0,
        desc: 0,
    };
    let ret = unsafe { syscall3(SYS_IOCTL, fd, DRM_IOCTL_VERSION, &mut version as *mut _ as usize) };
    if ret < 0 {
        panic_exit("DRM_IOCTL_VERSION failed");
    }
    if !name_matches(&name_buf, b"virtio_gpu") {
        panic_exit("unexpected drm name");
    }
    write_str("DRM: virtio_gpu ");
    write_i32(version.version_major);
    write_str(".");
    write_i32(version.version_minor);
    write_str(".");
    write_i32(version.version_patchlevel);
    write_str("\n");
    let mut resource = DrmVirtgpuResourceCreate {
        handle: 0,
        target: 0,
        format: 0,
        width: 64,
        height: 64,
        depth: 1,
        array_size: 1,
        last_level: 0,
        nr_samples: 0,
        flags: 0,
        size: 0,
        stride: 0,
    };
    let ret = unsafe {
        syscall3(
            SYS_IOCTL,
            fd,
            DRM_IOCTL_VIRTGPU_RESOURCE_CREATE,
            &mut resource as *mut _ as usize,
        )
    };
    if ret < 0 || resource.handle == 0 {
        panic_exit("DRM_IOCTL_VIRTGPU_RESOURCE_CREATE failed");
    }
    write_str("DRM SUBSYSTEM: ONLINE\n");
    exit(0);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    panic_exit("panic");
}
