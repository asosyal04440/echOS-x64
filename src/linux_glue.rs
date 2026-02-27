use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{c_char, c_void, CStr};
use spin::Mutex;

#[repr(C)]
pub struct Device {
    pub parent: *mut Device,      // offset = 0x00
    pub driver: *mut c_void,      // offset = 0x08
    pub driver_data: *mut c_void, // offset = 0x10
}

#[repr(C)]
pub struct PciResource {
    pub start: u64,
    pub end: u64,
    pub flags: u64,
}

#[repr(C)]
pub struct PciDev {
    pub dev: Device,           // offset = 0x00
    pub vendor: u16,           // offset = 0x18
    pub device: u16,           // offset = 0x1A
    pub subsystem_vendor: u16, // offset = 0x1C
    pub subsystem_device: u16, // offset = 0x1E
    pub class: u32,            // offset = 0x20
    pub revision: u8,          // offset = 0x24
    pub _pad0: [u8; 3],
    pub resource: [PciResource; 6], // offset = 0x28
    pub driver_data: *mut c_void,   // offset = 0xB8
}

#[repr(C)]
pub struct PciDeviceId {
    pub vendor: u16,
    pub device: u16,
    pub subvendor: u16,
    pub subdevice: u16,
    pub class: u32,
    pub class_mask: u32,
    pub driver_data: usize,
}

pub type PciProbeFn = Option<unsafe extern "C" fn(dev: *mut PciDev, id: *const PciDeviceId) -> i32>;
pub type PciRemoveFn = Option<unsafe extern "C" fn(dev: *mut PciDev)>;

#[repr(C)]
pub struct PciDriver {
    pub name: *const c_char,
    pub id_table: *const PciDeviceId,
    pub probe: PciProbeFn,
    pub remove: PciRemoveFn,
}

struct ClaimedDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub driver: usize,
    pub dev: usize,
    pub name: String,
}

static CLAIMED_DEVICES: Mutex<Vec<ClaimedDevice>> = Mutex::new(Vec::new());

#[repr(C)]
pub struct LinuxPciPriv {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub _pad: [u8; 5],
}

pub unsafe fn attach_pci_bdf(dev: *mut PciDev, bus: u8, device: u8, function: u8) -> bool {
    if dev.is_null() {
        return false;
    }
    let size = core::mem::size_of::<LinuxPciPriv>();
    let ptr = crate::allocator::heap_alloc(size) as *mut LinuxPciPriv;
    if ptr.is_null() {
        return false;
    }
    ptr.write(LinuxPciPriv {
        bus,
        device,
        function,
        _pad: [0; 5],
    });
    (*dev).driver_data = ptr as *mut c_void;
    true
}

const IORESOURCE_MEM: u64 = 0x0000_0200;

pub(crate) unsafe fn create_pci_dev(bus: u8, device: u8, function: u8) -> *mut PciDev {
    let id = crate::drivers::pci::read_config_dword(bus, device, function, 0x00);
    if id == 0xFFFF_FFFF {
        return core::ptr::null_mut();
    }
    let class_rev = crate::drivers::pci::read_config_dword(bus, device, function, 0x08);
    let subsys = crate::drivers::pci::read_config_dword(bus, device, function, 0x2C);
    let class_code = ((class_rev >> 24) & 0xFF) as u8;
    let subclass = ((class_rev >> 16) & 0xFF) as u8;
    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
    let revision = (class_rev & 0xFF) as u8;
    let mut dev = PciDev {
        dev: Device {
            parent: core::ptr::null_mut(),
            driver: core::ptr::null_mut(),
            driver_data: core::ptr::null_mut(),
        },
        vendor: (id & 0xFFFF) as u16,
        device: ((id >> 16) & 0xFFFF) as u16,
        subsystem_vendor: (subsys & 0xFFFF) as u16,
        subsystem_device: ((subsys >> 16) & 0xFFFF) as u16,
        class: ((class_code as u32) << 16) | ((subclass as u32) << 8) | (prog_if as u32),
        revision,
        _pad0: [0; 3],
        resource: core::mem::zeroed(),
        driver_data: core::ptr::null_mut(),
    };
    for bar_index in 0..6u8 {
        if let Some(bar) = crate::drivers::pci::read_bar_mmio(bus, device, function, bar_index) {
            if bar.size == 0 {
                continue;
            }
            let end = bar.base.saturating_add(bar.size.saturating_sub(1));
            dev.resource[bar_index as usize] = PciResource {
                start: bar.base,
                end,
                flags: IORESOURCE_MEM,
            };
        }
    }
    let ptr = crate::allocator::heap_alloc(core::mem::size_of::<PciDev>()) as *mut PciDev;
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    ptr.write(dev);
    attach_pci_bdf(ptr, bus, device, function);
    ptr
}

pub(crate) unsafe fn destroy_pci_dev(dev: *mut PciDev) {
    if dev.is_null() {
        return;
    }
    let data = (*dev).driver_data as *mut u8;
    if !data.is_null() {
        crate::allocator::heap_free(data);
    }
    crate::allocator::heap_free(dev as *mut u8);
}

pub(crate) fn id_table_end(id: &PciDeviceId) -> bool {
    id.vendor == 0 && id.device == 0
}

pub(crate) fn id_match(dev: &crate::drivers::pci::PciDevice, id: &PciDeviceId) -> bool {
    if id.vendor != 0xFFFF && id.vendor != dev.vendor_id {
        return false;
    }
    if id.device != 0xFFFF && id.device != dev.device_id {
        return false;
    }
    if id.class_mask != 0 {
        let dev_class =
            ((dev.class_code as u32) << 16) | ((dev.subclass as u32) << 8) | (dev.prog_if as u32);
        if (dev_class & id.class_mask) != (id.class & id.class_mask) {
            return false;
        }
    }
    true
}

pub(crate) fn is_claimed(bus: u8, device: u8, function: u8) -> bool {
    let claimed = CLAIMED_DEVICES.lock();
    claimed
        .iter()
        .any(|entry| entry.bus == bus && entry.device == device && entry.function == function)
}

pub(crate) fn claim_device(driver: *mut PciDriver, dev: *mut PciDev, bus: u8, device: u8, function: u8) {
    let mut claimed = CLAIMED_DEVICES.lock();
    let name = driver_name(driver);
    claimed.push(ClaimedDevice {
        bus,
        device,
        function,
        driver: driver as usize,
        dev: dev as usize,
        name,
    });
}

#[no_mangle]
pub unsafe extern "C" fn pci_register_driver(driver: *mut PciDriver) -> i32 {
    crate::ironshim_bridge::safe_pci_register_driver(driver)
}

#[no_mangle]
pub unsafe extern "C" fn pci_unregister_driver(driver: *mut PciDriver) {
    if driver.is_null() {
        return;
    }
    let mut claimed = CLAIMED_DEVICES.lock();
    let mut i = 0usize;
    while i < claimed.len() {
        if claimed[i].driver == driver as usize {
            let dev = claimed[i].dev as *mut PciDev;
            if let Some(remove) = (*driver).remove {
                remove(dev);
            }
            destroy_pci_dev(dev);
            claimed.swap_remove(i);
            continue;
        }
        i += 1;
    }

    let name = driver_name(driver);
    let removed = crate::ironshim_bridge::unregister_isolated_driver_by_name(&name);
    if removed > 0 {
        crate::serial_println!(
            "[linux_glue] Isolated slots removed for '{}': {}",
            name,
            removed
        );
    }
}

pub(crate) fn driver_name(driver: *mut PciDriver) -> String {
    if driver.is_null() {
        return String::from("unknown");
    }
    let name_ptr = unsafe { (*driver).name };
    if name_ptr.is_null() {
        return String::from("unknown");
    }
    unsafe { String::from(CStr::from_ptr(name_ptr).to_str().unwrap_or("unknown")) }
}

pub fn list_driver_names() -> Vec<String> {
    let claimed = CLAIMED_DEVICES.lock();
    let mut names: Vec<String> = Vec::new();
    for entry in claimed.iter() {
        if !names.iter().any(|n| n == &entry.name) {
            names.push(entry.name.clone());
        }
    }
    names
}

pub fn debug_dump_drivers() {
    let claimed = CLAIMED_DEVICES.lock();
    let mut names: Vec<String> = Vec::new();
    for entry in claimed.iter() {
        if !names.iter().any(|n| n == &entry.name) {
            names.push(entry.name.clone());
        }
    }
    for name in names.iter() {
        let mut count = 0usize;
        for entry in claimed.iter() {
            if &entry.name == name {
                count += 1;
            }
        }
        crate::serial_println!("driver={} devices={}", name, count);
    }
}

unsafe extern "C" fn virtio_gpu_probe(_dev: *mut PciDev, _id: *const PciDeviceId) -> i32 {
    if crate::drivers::virtio_gpu::init_from_pci(_dev) {
        0
    } else {
        -1
    }
}

static VIRTIO_GPU_ID_TABLE: [PciDeviceId; 2] = [
    PciDeviceId {
        vendor: 0x1AF4,
        device: 0x1050,
        subvendor: 0xFFFF,
        subdevice: 0xFFFF,
        class: 0,
        class_mask: 0,
        driver_data: 0,
    },
    PciDeviceId {
        vendor: 0,
        device: 0,
        subvendor: 0,
        subdevice: 0,
        class: 0,
        class_mask: 0,
        driver_data: 0,
    },
];

static mut VIRTIO_GPU_DRIVER: PciDriver = PciDriver {
    name: b"virtio_gpu\0".as_ptr() as *const c_char,
    id_table: VIRTIO_GPU_ID_TABLE.as_ptr(),
    probe: Some(virtio_gpu_probe),
    remove: None,
};

pub fn init() {
    crate::ironshim_bridge::init_ironshim_bridge();
    unsafe {
        let _ = pci_register_driver(&raw mut VIRTIO_GPU_DRIVER as *mut PciDriver);
    }
}

#[repr(C)]
pub struct Inode {
    pub _opaque: [u8; 0],
}

#[repr(C)]
pub struct File {
    pub _opaque: [u8; 0],
}

#[repr(C)]
pub struct DrmFile {
    pub _opaque: [u8; 0],
}

pub type DrmLoadFn = Option<unsafe extern "C" fn(dev: *mut Device, flags: u32) -> i32>;
pub type DrmUnloadFn = Option<unsafe extern "C" fn(dev: *mut Device)>;
pub type DrmIoctlFn =
    Option<unsafe extern "C" fn(dev: *mut Device, data: *mut c_void, file: *mut DrmFile) -> i32>;

#[repr(C)]
pub struct FileOperations {
    pub owner: *mut c_void, // offset = 0x00
    pub open: Option<unsafe extern "C" fn(inode: *mut Inode, file: *mut File) -> i32>, // offset = 0x08
    pub release: Option<unsafe extern "C" fn(inode: *mut Inode, file: *mut File) -> i32>, // offset = 0x10
    pub read: Option<
        unsafe extern "C" fn(file: *mut File, buf: *mut u8, len: usize, offset: *mut i64) -> isize,
    >, // offset = 0x18
    pub write: Option<
        unsafe extern "C" fn(
            file: *mut File,
            buf: *const u8,
            len: usize,
            offset: *mut i64,
        ) -> isize,
    >, // offset = 0x20
    pub unlocked_ioctl: Option<unsafe extern "C" fn(file: *mut File, cmd: u32, arg: usize) -> i32>, // offset = 0x28
}

#[repr(C)]
pub struct DrmDriver {
    pub major: i32,
    pub minor: i32,
    pub load: DrmLoadFn,     // offset = 0x08
    pub unload: DrmUnloadFn, // offset = 0x10
    pub ioctl: DrmIoctlFn,   // offset = 0x18
    pub fops: *const FileOperations,
    pub name: *const c_char,
}

#[macro_export]
macro_rules! offset_of {
    ($ty:ty, $field:ident) => {{
        let base = core::ptr::null::<$ty>();
        unsafe { &(*base).$field as *const _ as usize }
    }};
}

#[macro_export]
macro_rules! verify_layout {
    ($ty:ty, $size:expr, $( $field:ident => $offset:expr ),+ $(,)?) => {{
        assert_eq!(core::mem::size_of::<$ty>(), $size);
        $(assert_eq!($crate::offset_of!($ty, $field), $offset);)+
    }};
}
