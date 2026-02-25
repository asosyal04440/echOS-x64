use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

use crate::linux_glue::PciDev;

const VIRTIO_PCI_CAP_ID: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

const VIRTIO_GPU_F_VIRGL: u64 = 1 << 0;

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0108;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0202;
const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0205;
const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1107;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const VIRTIO_GPU_FLAG_FENCE: u32 = 1;

const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

const VIRTIO_GPU_CAPSET_VIRGL: u32 = 1;
const VIRTIO_GPU_CAPSET_VIRGL2: u32 = 2;

const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 67;

const VIRGL_CCMD_CREATE_OBJECT: u32 = 1;
const VIRGL_CCMD_SET_FRAMEBUFFER_STATE: u32 = 2;
const VIRGL_CCMD_CLEAR: u32 = 3;
const VIRGL_OBJECT_SURFACE: u32 = 1;
const VIRGL_CLEAR_COLOR: u32 = 1;

#[repr(C)]
struct VirtioPciCommonCfg {
    device_feature_select: u32,
    device_feature: u32,
    driver_feature_select: u32,
    driver_feature: u32,
    msix_config: u16,
    num_queues: u16,
    device_status: u8,
    config_generation: u8,
    queue_select: u16,
    queue_size: u16,
    queue_msix_vector: u16,
    queue_enable: u16,
    queue_notify_off: u16,
    queue_desc: u64,
    queue_avail: u64,
    queue_used: u64,
}

#[repr(C)]
struct VirtioGpuCtrlHdr {
    type_: u32,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtioGpuDisplayOne {
    r: VirtioGpuRect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
struct VirtioGpuRespDisplayInfo {
    hdr: VirtioGpuCtrlHdr,
    pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
}

#[repr(C)]
struct VirtioGpuGetCapset {
    hdr: VirtioGpuCtrlHdr,
    capset_id: u32,
    capset_version: u32,
}

#[repr(C)]
struct VirtioGpuRespCapset {
    hdr: VirtioGpuCtrlHdr,
    capset_id: u32,
    capset_version: u32,
    size: u32,
    padding: u32,
}

#[repr(C)]
struct VirtioGpuCtxCreate {
    hdr: VirtioGpuCtrlHdr,
    ctx_id: u32,
    nlen: u32,
}

#[repr(C)]
struct VirtioGpuResourceCreate3d {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
}

#[repr(C)]
struct VirtioGpuSetScanout {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
struct VirtioGpuResourceFlush {
    hdr: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
struct VirtioGpuCmdSubmit3d {
    hdr: VirtioGpuCtrlHdr,
    size: u32,
    padding: u32,
}

struct VirglEncoder {
    words: Vec<u32>,
}

impl VirglEncoder {
    fn new() -> Self {
        Self { words: Vec::new() }
    }

    fn push_cmd(&mut self, opcode: u32, payload: &[u32]) {
        let header = opcode | ((payload.len() as u32) << 16);
        self.words.push(header);
        self.words.extend_from_slice(payload);
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for word in self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}
#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; 8],
    used_event: u16,
}

#[repr(C)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; 8],
    avail_event: u16,
}

struct VirtQueue {
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
    size: u16,
    last_used: u16,
    notify_off: u16,
}

struct VirtioGpuDevice {
    common: *mut VirtioPciCommonCfg,
    notify: *mut u8,
    notify_off_multiplier: u32,
    isr: *mut u8,
    device_cfg: *mut u8,
    features: u64,
    virgl: bool,
    capset_id: u32,
    capset_version: u32,
    capset_size: u32,
    capset_data: *mut u8,
    ctx_id: u32,
    resource_id: u32,
    surface_handle: u32,
    fence_counter: u64,
    ctrl_queue: VirtQueue,
}

unsafe impl Send for VirtioGpuDevice {}

static GPU_DEVICE: Mutex<Option<VirtioGpuDevice>> = Mutex::new(None);

pub unsafe fn init_from_pci(dev: *mut PciDev) -> bool {
    if dev.is_null() {
        return false;
    }
    let (common, notify, notify_mul, isr, device_cfg) = match read_pci_caps(dev) {
        Some(data) => data,
        None => return false,
    };
    let mut gpu = VirtioGpuDevice {
        common,
        notify,
        notify_off_multiplier: notify_mul,
        isr,
        device_cfg,
        features: 0,
        virgl: false,
        capset_id: 0,
        capset_version: 0,
        capset_size: 0,
        capset_data: core::ptr::null_mut(),
        ctx_id: 1,
        resource_id: 1,
        surface_handle: 1,
        fence_counter: 1,
        ctrl_queue: unsafe { core::mem::zeroed() },
    };
    if !negotiate_features(&mut gpu) {
        return false;
    }
    if !setup_ctrl_queue(&mut gpu) {
        return false;
    }
    let capset = query_capset(&mut gpu);
    if capset == 0 {
        return false;
    }
    gpu.capset_id = capset;
    if !create_context(&mut gpu) {
        return false;
    }
    if !create_resource_3d(&mut gpu, 64, 64) {
        return false;
    }
    let status = read_volatile(&(*gpu.common).device_status);
    write_volatile(
        &mut (*gpu.common).device_status,
        status | VIRTIO_STATUS_DRIVER_OK,
    );
    *GPU_DEVICE.lock() = Some(gpu);
    true
}

unsafe fn read_pci_caps(
    dev: *mut PciDev,
) -> Option<(*mut VirtioPciCommonCfg, *mut u8, u32, *mut u8, *mut u8)> {
    let (bus, device, function) = unsafe {
        let priv_ptr = (*dev).driver_data as *const crate::linux_glue::LinuxPciPriv;
        if priv_ptr.is_null() {
            return None;
        }
        ((*priv_ptr).bus, (*priv_ptr).device, (*priv_ptr).function)
    };
    let status = read_config_u16(bus, device, function, 0x06);
    if (status & 0x10) == 0 {
        return None;
    }
    let mut cap_ptr = read_config_u8(bus, device, function, 0x34);
    let mut common = None;
    let mut notify = None;
    let mut notify_mul = 0u32;
    let mut isr = None;
    let mut device_cfg = None;
    while cap_ptr != 0 {
        let cap_id = read_config_u8(bus, device, function, cap_ptr);
        let next = read_config_u8(bus, device, function, cap_ptr + 1);
        if cap_id == VIRTIO_PCI_CAP_ID {
            let cfg_type = read_config_u8(bus, device, function, cap_ptr + 3);
            let bar = read_config_u8(bus, device, function, cap_ptr + 4);
            let offset = read_config_u32(bus, device, function, cap_ptr + 8);
            let length = read_config_u32(bus, device, function, cap_ptr + 12);
            let bar_base = get_bar_base(dev, bar)?;
            let base = (bar_base + offset as u64) as usize;
            match cfg_type {
                VIRTIO_PCI_CAP_COMMON_CFG => {
                    common = Some(base as *mut VirtioPciCommonCfg);
                }
                VIRTIO_PCI_CAP_NOTIFY_CFG => {
                    notify = Some(base as *mut u8);
                    notify_mul = read_config_u32(bus, device, function, cap_ptr + 16);
                }
                VIRTIO_PCI_CAP_ISR_CFG => {
                    if length > 0 {
                        isr = Some(base as *mut u8);
                    }
                }
                VIRTIO_PCI_CAP_DEVICE_CFG => {
                    device_cfg = Some(base as *mut u8);
                }
                _ => {}
            }
        }
        cap_ptr = next;
    }
    Some((common?, notify?, notify_mul, isr?, device_cfg?))
}

unsafe fn negotiate_features(gpu: &mut VirtioGpuDevice) -> bool {
    let common = &mut *gpu.common;
    write_volatile(&mut common.device_status, 0);
    write_volatile(
        &mut common.device_status,
        VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER,
    );
    write_volatile(&mut common.device_feature_select, 0);
    let low = read_volatile(&common.device_feature) as u64;
    write_volatile(&mut common.device_feature_select, 1);
    let high = read_volatile(&common.device_feature) as u64;
    let features = low | (high << 32);
    gpu.features = features;
    let enabled = features & VIRTIO_GPU_F_VIRGL;
    gpu.virgl = enabled != 0;
    write_volatile(&mut common.driver_feature_select, 0);
    write_volatile(&mut common.driver_feature, (enabled & 0xFFFF_FFFF) as u32);
    write_volatile(&mut common.driver_feature_select, 1);
    write_volatile(
        &mut common.driver_feature,
        ((enabled >> 32) & 0xFFFF_FFFF) as u32,
    );
    let status = read_volatile(&common.device_status);
    write_volatile(
        &mut common.device_status,
        status | VIRTIO_STATUS_FEATURES_OK,
    );
    let status = read_volatile(&common.device_status);
    (status & VIRTIO_STATUS_FEATURES_OK) != 0 && gpu.virgl
}

unsafe fn setup_ctrl_queue(gpu: &mut VirtioGpuDevice) -> bool {
    let common = &mut *gpu.common;
    write_volatile(&mut common.queue_select, 0);
    let size = read_volatile(&common.queue_size);
    if size == 0 {
        return false;
    }
    let queue_size = 8u16.min(size);
    let (desc, avail, used) = match allocate_queue(queue_size) {
        Some(queue) => queue,
        None => return false,
    };
    write_volatile(&mut common.queue_size, queue_size);
    write_volatile(&mut common.queue_desc, desc as u64);
    write_volatile(&mut common.queue_avail, avail as u64);
    write_volatile(&mut common.queue_used, used as u64);
    write_volatile(&mut common.queue_enable, 1);
    let notify_off = read_volatile(&common.queue_notify_off);
    gpu.ctrl_queue = VirtQueue {
        desc,
        avail,
        used,
        size: queue_size,
        last_used: 0,
        notify_off,
    };
    true
}

unsafe fn allocate_queue(size: u16) -> Option<(*mut VirtqDesc, *mut VirtqAvail, *mut VirtqUsed)> {
    let desc_bytes = core::mem::size_of::<VirtqDesc>() * size as usize;
    let avail_bytes = core::mem::size_of::<u16>() * 3 + core::mem::size_of::<u16>() * size as usize;
    let used_bytes =
        core::mem::size_of::<VirtqUsedElem>() * size as usize + core::mem::size_of::<u16>() * 3;
    let total = align_up(desc_bytes + avail_bytes + used_bytes, 4096);
    let ptr = crate::allocator::heap_alloc(total) as *mut u8;
    if ptr.is_null() {
        return None;
    }
    core::ptr::write_bytes(ptr, 0, total);
    let desc = ptr as *mut VirtqDesc;
    let avail = ptr.add(desc_bytes) as *mut VirtqAvail;
    let used = ptr.add(align_up(desc_bytes + avail_bytes, 4096)) as *mut VirtqUsed;
    Some((desc, avail, used))
}

unsafe fn query_capset(gpu: &mut VirtioGpuDevice) -> u32 {
    if fetch_capset(gpu, VIRTIO_GPU_CAPSET_VIRGL2) {
        return VIRTIO_GPU_CAPSET_VIRGL2;
    }
    if fetch_capset(gpu, VIRTIO_GPU_CAPSET_VIRGL) {
        return VIRTIO_GPU_CAPSET_VIRGL;
    }
    0
}

unsafe fn create_context(gpu: &mut VirtioGpuDevice) -> bool {
    let req = VirtioGpuCtxCreate {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_CTX_CREATE,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        ctx_id: gpu.ctx_id,
        nlen: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuCtxCreate>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

unsafe fn create_resource_3d(gpu: &mut VirtioGpuDevice, width: u32, height: u32) -> bool {
    let req = VirtioGpuResourceCreate3d {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
            flags: 0,
            fence_id: 0,
            ctx_id: gpu.ctx_id,
            padding: 0,
        },
        resource_id: gpu.resource_id,
        format: VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
        width,
        height,
        depth: 1,
        array_size: 1,
        last_level: 0,
        nr_samples: 1,
        flags: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuResourceCreate3d>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

unsafe fn get_display_info(gpu: &mut VirtioGpuDevice) -> Option<(u32, u32)> {
    let req = VirtioGpuCtrlHdr {
        type_: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    let mut resp = VirtioGpuRespDisplayInfo {
        hdr: VirtioGpuCtrlHdr {
            type_: 0,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        pmodes: [VirtioGpuDisplayOne {
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            enabled: 0,
            flags: 0,
        }; VIRTIO_GPU_MAX_SCANOUTS],
    };
    if !submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuRespDisplayInfo>(),
    ) {
        return None;
    }
    if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
        return None;
    }
    let first = &resp.pmodes[0];
    if first.enabled == 0 || first.r.width == 0 || first.r.height == 0 {
        return None;
    }
    Some((first.r.width, first.r.height))
}

unsafe fn fetch_capset(gpu: &mut VirtioGpuDevice, capset_id: u32) -> bool {
    let Some((size, version)) = send_get_capset_info(gpu, capset_id) else {
        return false;
    };
    if size == 0 {
        return false;
    }
    let total = align_up(
        size as usize + core::mem::size_of::<VirtioGpuRespCapset>(),
        8,
    );
    let buffer = crate::allocator::heap_alloc(total) as *mut u8;
    if buffer.is_null() {
        return false;
    }
    core::ptr::write_bytes(buffer, 0, total);
    let req = VirtioGpuGetCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_GET_CAPSET,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id,
        capset_version: version,
    };
    if !submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuGetCapset>(),
        buffer,
        total,
    ) {
        return false;
    }
    let resp = &*(buffer as *const VirtioGpuRespCapset);
    if resp.hdr.type_ != VIRTIO_GPU_RESP_OK_CAPSET || resp.size == 0 {
        return false;
    }
    gpu.capset_version = resp.capset_version;
    gpu.capset_size = resp.size;
    gpu.capset_data = unsafe { buffer.add(core::mem::size_of::<VirtioGpuRespCapset>()) };
    true
}

unsafe fn send_get_capset_info(gpu: &mut VirtioGpuDevice, capset_id: u32) -> Option<(u32, u32)> {
    let req = VirtioGpuGetCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_GET_CAPSET,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id,
        capset_version: 0,
    };
    let mut resp = VirtioGpuRespCapset {
        hdr: VirtioGpuCtrlHdr {
            type_: 0,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        capset_id: 0,
        capset_version: 0,
        size: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuGetCapset>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuRespCapset>(),
    )
    .then_some(())
    .filter(|_| resp.hdr.type_ == VIRTIO_GPU_RESP_OK_CAPSET)
    .map(|_| (resp.size, resp.capset_version))
}

unsafe fn submit_3d_command(gpu: &mut VirtioGpuDevice, data: *const u8, len: usize) -> bool {
    if data.is_null() || len == 0 {
        return false;
    }
    let total = core::mem::size_of::<VirtioGpuCmdSubmit3d>() + len;
    let buffer = crate::allocator::heap_alloc(align_up(total, 8)) as *mut u8;
    if buffer.is_null() {
        return false;
    }
    let fence_id = gpu.fence_counter;
    gpu.fence_counter = gpu.fence_counter.wrapping_add(1);
    let header = VirtioGpuCmdSubmit3d {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_SUBMIT_3D,
            flags: VIRTIO_GPU_FLAG_FENCE,
            fence_id,
            ctx_id: gpu.ctx_id,
            padding: 0,
        },
        size: len as u32,
        padding: 0,
    };
    core::ptr::copy_nonoverlapping(
        &header as *const _ as *const u8,
        buffer,
        core::mem::size_of::<VirtioGpuCmdSubmit3d>(),
    );
    core::ptr::copy_nonoverlapping(
        data,
        buffer.add(core::mem::size_of::<VirtioGpuCmdSubmit3d>()),
        len,
    );
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        buffer,
        total,
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
        && resp.fence_id == fence_id
}

unsafe fn set_scanout(
    gpu: &mut VirtioGpuDevice,
    resource_id: u32,
    width: u32,
    height: u32,
) -> bool {
    let req = VirtioGpuSetScanout {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_SET_SCANOUT,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        r: make_rect(width, height),
        scanout_id: 0,
        resource_id,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuSetScanout>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

unsafe fn resource_flush(
    gpu: &mut VirtioGpuDevice,
    resource_id: u32,
    width: u32,
    height: u32,
) -> bool {
    let req = VirtioGpuResourceFlush {
        hdr: VirtioGpuCtrlHdr {
            type_: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        r: make_rect(width, height),
        resource_id,
        padding: 0,
    };
    let mut resp = VirtioGpuCtrlHdr {
        type_: 0,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        padding: 0,
    };
    submit_ctrl(
        gpu,
        &req as *const _ as *const u8,
        core::mem::size_of::<VirtioGpuResourceFlush>(),
        &mut resp as *mut _ as *mut u8,
        core::mem::size_of::<VirtioGpuCtrlHdr>(),
    ) && resp.type_ == VIRTIO_GPU_RESP_OK_NODATA
}

fn f32_bits(value: f32) -> u32 {
    value.to_bits()
}

fn make_rect(width: u32, height: u32) -> VirtioGpuRect {
    VirtioGpuRect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

fn push_create_surface(
    encoder: &mut VirglEncoder,
    handle: u32,
    resource_id: u32,
    width: u32,
    height: u32,
) {
    encoder.push_cmd(
        VIRGL_CCMD_CREATE_OBJECT,
        &[
            VIRGL_OBJECT_SURFACE,
            handle,
            resource_id,
            VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            width,
            height,
            0,
            0,
        ],
    );
}

fn push_set_framebuffer(encoder: &mut VirglEncoder, surface_handle: u32) {
    encoder.push_cmd(VIRGL_CCMD_SET_FRAMEBUFFER_STATE, &[1, 0, surface_handle]);
}

fn push_clear(encoder: &mut VirglEncoder, r: f32, g: f32, b: f32, a: f32) {
    encoder.push_cmd(
        VIRGL_CCMD_CLEAR,
        &[
            VIRGL_CLEAR_COLOR,
            f32_bits(r),
            f32_bits(g),
            f32_bits(b),
            f32_bits(a),
            f32_bits(1.0),
            0,
        ],
    );
}

fn build_clear_command(surface_handle: u32, resource_id: u32, width: u32, height: u32) -> Vec<u8> {
    let mut encoder = VirglEncoder::new();
    push_create_surface(&mut encoder, surface_handle, resource_id, width, height);
    push_set_framebuffer(&mut encoder, surface_handle);
    push_clear(&mut encoder, 1.0, 0.749, 0.0, 1.0);
    encoder.into_bytes()
}

pub fn drm_resource_create_3d(width: u32, height: u32) -> Option<u32> {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return None;
    };
    if !gpu.virgl {
        return None;
    }
    gpu.resource_id = gpu.resource_id.wrapping_add(1);
    let resource_id = gpu.resource_id;
    if unsafe { !create_resource_3d(gpu, width, height) } {
        return None;
    }
    Some(resource_id)
}

pub unsafe fn drm_submit_3d_command(data: *const u8, len: usize) -> bool {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return false;
    };
    if !gpu.virgl {
        return false;
    }
    unsafe { submit_3d_command(gpu, data, len) }
}

pub fn hardware_clear_amber(width: u32, height: u32) -> bool {
    let mut guard = GPU_DEVICE.lock();
    let Some(gpu) = guard.as_mut() else {
        return false;
    };
    if !gpu.virgl {
        return false;
    }
    let (target_width, target_height) = unsafe { get_display_info(gpu) }.unwrap_or((width, height));
    gpu.resource_id = gpu.resource_id.wrapping_add(1);
    gpu.surface_handle = gpu.surface_handle.wrapping_add(1);
    let resource_id = gpu.resource_id;
    let surface_handle = gpu.surface_handle;
    if unsafe { !create_resource_3d(gpu, target_width, target_height) } {
        return false;
    }
    let payload = build_clear_command(surface_handle, resource_id, target_width, target_height);
    if unsafe { !submit_3d_command(gpu, payload.as_ptr(), payload.len()) } {
        return false;
    }
    if unsafe { !set_scanout(gpu, resource_id, target_width, target_height) } {
        return false;
    }
    unsafe { resource_flush(gpu, resource_id, target_width, target_height) }
}

unsafe fn submit_ctrl(
    gpu: &mut VirtioGpuDevice,
    req: *const u8,
    req_len: usize,
    resp: *mut u8,
    resp_len: usize,
) -> bool {
    let q = &mut gpu.ctrl_queue;
    let desc = &mut *q.desc;
    let desc1 = q.desc.add(1);
    (*desc).addr = req as u64;
    (*desc).len = req_len as u32;
    (*desc).flags = 0x0002;
    (*desc).next = 1;
    (*desc1).addr = resp as u64;
    (*desc1).len = resp_len as u32;
    (*desc1).flags = 0x0002 | 0x0001;
    (*desc1).next = 0;

    let avail = &mut *q.avail;
    let idx = avail.idx;
    avail.ring[(idx as usize) % q.size as usize] = 0;
    avail.idx = idx.wrapping_add(1);
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    let notify_offset = q.notify_off as u32 * gpu.notify_off_multiplier;
    let notify_ptr = gpu.notify.add(notify_offset as usize) as *mut u16;
    write_volatile(notify_ptr, 0);

    let target = avail.idx;
    let used_ptr = q.used;
    loop {
        let used_idx = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*used_ptr).idx)) };
        if used_idx == target {
            q.last_used = used_idx;
            break;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
    true
}

unsafe fn get_bar_base(dev: *mut PciDev, index: u8) -> Option<u64> {
    if dev.is_null() {
        return None;
    }
    let resources = &(*dev).resource;
    let res = resources.get(index as usize)?;
    if res.start == 0 {
        return None;
    }
    Some(res.start)
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn read_config_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = crate::drivers::pci::read_config_dword(bus, device, function, offset as u16);
    let shift = (offset & 3) * 8;
    ((value >> shift) & 0xFF) as u8
}

fn read_config_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = crate::drivers::pci::read_config_dword(bus, device, function, offset as u16);
    let shift = (offset & 2) * 8;
    ((value >> shift) & 0xFFFF) as u16
}

fn read_config_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    crate::drivers::pci::read_config_dword(bus, device, function, offset as u16)
}
