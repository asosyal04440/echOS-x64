//! # DRM/KMS - Direct Rendering Manager
//!
//! GPU and display subsystem management.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// DRM CONSTANTS
// ============================================================================

/// DRM command numbers
pub const DRM_IOCTL_VERSION: u32 = 0x8000_6400;
pub const DRM_IOCTL_GET_MAGIC: u32 = 0x8000_6401;
pub const DRM_IOCTL_IRQ_BUSID: u32 = 0xC008_6402;
pub const DRM_IOCTL_GET_MAP: u32 = 0xC028_6403;
pub const DRM_IOCTL_GET_CLIENT: u32 = 0xC028_6404;
pub const DRM_IOCTL_GET_STATS: u32 = 0xC008_6405;
pub const DRM_IOCTL_SET_VERSION: u32 = 0xC024_6406;
pub const DRM_IOCTL_MODESET_CTL: u32 = 0x4008_6407;
pub const DRM_IOCTL_GEM_CLOSE: u32 = 0x4008_6408;
pub const DRM_IOCTL_GEM_FLINK: u32 = 0xC008_6409;
pub const DRM_IOCTL_GEM_OPEN: u32 = 0xC010_640A;
pub const DRM_IOCTL_GET_CAP: u32 = 0xC010_640B;
pub const DRM_IOCTL_SET_CLIENT_CAP: u32 = 0x4010_640C;
pub const DRM_IOCTL_PRIME_HANDLE_TO_FD: u32 = 0xC00C_642E;
pub const DRM_IOCTL_PRIME_FD_TO_HANDLE: u32 = 0xC00C_642F;

/// Mode setting ioctls
pub const DRM_IOCTL_MODE_GETRESOURCES: u32 = 0xC040_64A0;
pub const DRM_IOCTL_MODE_GETCONNECTOR: u32 = 0xC1A0_64A1;
pub const DRM_IOCTL_MODE_GETENCODER: u32 = 0xC0A0_64A2;
pub const DRM_IOCTL_MODE_GETCRTC: u32 = 0xC0C0_64A3;
pub const DRM_IOCTL_MODE_SETCRTC: u32 = 0xC0C0_64A4;
pub const DRM_IOCTL_MODE_GETPLANE: u32 = 0xC0B0_64A5;
pub const DRM_IOCTL_MODE_SETPLANE: u32 = 0xC0B0_64A6;
pub const DRM_IOCTL_MODE_CURSOR: u32 = 0xC080_64A7;
pub const DRM_IOCTL_MODE_GETFB: u32 = 0xC080_64A8;
pub const DRM_IOCTL_MODE_ADDFB: u32 = 0xC080_64A9;
pub const DRM_IOCTL_MODE_RMFB: u32 = 0x4008_64AA;
pub const DRM_IOCTL_MODE_PAGE_FLIP: u32 = 0xC018_64B0;
pub const DRM_IOCTL_MODE_DIRTYFB: u32 = 0xC018_64B1;
pub const DRM_IOCTL_MODE_CREATE_DUMB: u32 = 0xC0C0_64B2;
pub const DRM_IOCTL_MODE_MAP_DUMB: u32 = 0xC010_64B3;
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u32 = 0xC008_64B4;

// ============================================================================
// DRM VERSION
// ============================================================================

#[repr(C)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: usize,
    pub name: u64,
    pub date_len: usize,
    pub date: u64,
    pub desc_len: usize,
    pub desc: u64,
}

// ============================================================================
// DRM DEVICE
// ============================================================================

pub struct DrmDevice {
    /// Device ID
    pub id: u64,
    /// Device name
    pub name: String,
    /// Driver name
    pub driver_name: String,
    /// Driver version
    pub driver_version: (u32, u32, u32),
    /// Capabilities
    pub caps: Mutex<BTreeMap<u64, u64>>,
    /// Framebuffers
    pub framebuffers: Mutex<BTreeMap<u32, Arc<DrmFramebuffer>>>,
    /// GEM objects
    pub gem_objects: Mutex<BTreeMap<u32, Arc<GemObject>>>,
    /// Next GEM handle
    next_gem_handle: AtomicU32,
    /// Next FB ID
    next_fb_id: AtomicU32,
    /// Modesetting enabled
    pub modeset_enabled: AtomicBool,
    /// CRTCs
    pub crtcs: Mutex<Vec<Arc<DrmCrtc>>>,
    /// Encoders
    pub encoders: Mutex<Vec<Arc<DrmEncoder>>>,
    /// Connectors
    pub connectors: Mutex<Vec<Arc<DrmConnector>>>,
    /// Planes
    pub planes: Mutex<Vec<Arc<DrmPlane>>>,
}

impl DrmDevice {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            driver_name: String::from("echos-drm"),
            driver_version: (1, 0, 0),
            caps: Mutex::new(BTreeMap::new()),
            framebuffers: Mutex::new(BTreeMap::new()),
            gem_objects: Mutex::new(BTreeMap::new()),
            next_gem_handle: AtomicU32::new(1),
            next_fb_id: AtomicU32::new(1),
            modeset_enabled: AtomicBool::new(true),
            crtcs: Mutex::new(Vec::new()),
            encoders: Mutex::new(Vec::new()),
            connectors: Mutex::new(Vec::new()),
            planes: Mutex::new(Vec::new()),
        }
    }

    /// Create GEM object
    pub fn gem_create(&self, size: u64) -> Arc<GemObject> {
        let handle = self.next_gem_handle.fetch_add(1, Ordering::SeqCst);
        let obj = Arc::new(GemObject::new(handle, size));
        self.gem_objects.lock().insert(handle, obj.clone());
        obj
    }

    /// Get GEM object
    pub fn gem_get(&self, handle: u32) -> Option<Arc<GemObject>> {
        self.gem_objects.lock().get(&handle).cloned()
    }

    /// Close GEM handle
    pub fn gem_close(&self, handle: u32) {
        self.gem_objects.lock().remove(&handle);
    }

    /// Create framebuffer
    pub fn fb_create(&self, width: u32, height: u32, format: u32, handles: [u32; 4]) -> u32 {
        let fb_id = self.next_fb_id.fetch_add(1, Ordering::SeqCst);
        let fb = Arc::new(DrmFramebuffer::new(fb_id, width, height, format, handles));
        self.framebuffers.lock().insert(fb_id, fb);
        fb_id
    }

    /// Get framebuffer
    pub fn fb_get(&self, fb_id: u32) -> Option<Arc<DrmFramebuffer>> {
        self.framebuffers.lock().get(&fb_id).cloned()
    }

    /// Remove framebuffer
    pub fn fb_remove(&self, fb_id: u32) {
        self.framebuffers.lock().remove(&fb_id);
    }

    /// Get capability
    pub fn get_cap(&self, cap: u64) -> u64 {
        self.caps.lock().get(&cap).copied().unwrap_or(0)
    }

    /// Set capability
    pub fn set_cap(&self, cap: u64, value: u64) {
        self.caps.lock().insert(cap, value);
    }

    /// Add CRTC
    pub fn add_crtc(&self, crtc: Arc<DrmCrtc>) {
        self.crtcs.lock().push(crtc);
    }

    /// Add connector
    pub fn add_connector(&self, connector: Arc<DrmConnector>) {
        self.connectors.lock().push(connector);
    }

    /// Add encoder
    pub fn add_encoder(&self, encoder: Arc<DrmEncoder>) {
        self.encoders.lock().push(encoder);
    }

    /// Add plane
    pub fn add_plane(&self, plane: Arc<DrmPlane>) {
        self.planes.lock().push(plane);
    }
}

// ============================================================================
// GEM OBJECT
// ============================================================================

pub struct GemObject {
    pub handle: u32,
    pub size: u64,
    pub vaddr: Mutex<Option<u64>>,
    pub paddr: Mutex<Option<u64>>,
    pub ref_count: AtomicU32,
    pub dma_buf: Mutex<Option<u32>>,
}

impl GemObject {
    pub fn new(handle: u32, size: u64) -> Self {
        Self {
            handle,
            size,
            vaddr: Mutex::new(None),
            paddr: Mutex::new(None),
            ref_count: AtomicU32::new(1),
            dma_buf: Mutex::new(None),
        }
    }

    /// Map object
    pub fn map(&self) -> u64 {
        let mut vaddr = self.vaddr.lock();
        if vaddr.is_none() {
            // Allocate and map
            *vaddr = Some(0xFFFF_8000_0000_0000);
        }
        vaddr.unwrap()
    }

    /// Unmap object
    pub fn unmap(&self) {
        *self.vaddr.lock() = None;
    }
}

// ============================================================================
// FRAMEBUFFER
// ============================================================================

pub struct DrmFramebuffer {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],
    pub offsets: [u32; 4],
    pub modifier: u64,
    pub ref_count: AtomicU32,
}

impl DrmFramebuffer {
    pub fn new(id: u32, width: u32, height: u32, format: u32, handles: [u32; 4]) -> Self {
        Self {
            id,
            width,
            height,
            format,
            handles,
            pitches: [width * 4, 0, 0, 0],
            offsets: [0, 0, 0, 0],
            modifier: 0,
            ref_count: AtomicU32::new(1),
        }
    }
}

// ============================================================================
// CRTC
// ============================================================================

pub struct DrmCrtc {
    pub id: u32,
    pub index: u32,
    pub x: u32,
    pub y: u32,
    pub fb_id: AtomicU32,
    pub mode: Mutex<Option<DrmMode>>,
    pub gamma_size: u32,
    pub active: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct DrmMode {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    pub name: [u8; 32],
}

impl DrmCrtc {
    pub fn new(id: u32, index: u32) -> Self {
        Self {
            id,
            index,
            x: 0,
            y: 0,
            fb_id: AtomicU32::new(0),
            mode: Mutex::new(None),
            gamma_size: 256,
            active: AtomicBool::new(false),
        }
    }

    /// Set mode
    pub fn set_mode(&self, mode: DrmMode) {
        *self.mode.lock() = Some(mode);
        self.active.store(true, Ordering::SeqCst);
    }

    /// Set framebuffer
    pub fn set_fb(&self, fb_id: u32) {
        self.fb_id.store(fb_id, Ordering::SeqCst);
    }
}

// ============================================================================
// ENCODER
// ============================================================================

pub struct DrmEncoder {
    pub id: u32,
    pub encoder_type: u32,
    pub crtc_id: AtomicU32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

impl DrmEncoder {
    pub fn new(id: u32, encoder_type: u32) -> Self {
        Self {
            id,
            encoder_type,
            crtc_id: AtomicU32::new(0),
            possible_crtcs: 0xFFFF,
            possible_clones: 0xFFFF,
        }
    }
}

// ============================================================================
// CONNECTOR
// ============================================================================

pub struct DrmConnector {
    pub id: u32,
    pub connector_type: u32,
    pub connector_type_id: u32,
    pub encoder_id: AtomicU32,
    pub connection: Mutex<DrmConnectorStatus>,
    pub modes: Mutex<Vec<DrmMode>>,
    pub width_mm: u32,
    pub height_mm: u32,
    pub subpixel: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmConnectorStatus {
    Unknown,
    Connected,
    Disconnected,
}

impl DrmConnector {
    pub fn new(id: u32, connector_type: u32) -> Self {
        Self {
            id,
            connector_type,
            connector_type_id: 1,
            encoder_id: AtomicU32::new(0),
            connection: Mutex::new(DrmConnectorStatus::Unknown),
            modes: Mutex::new(Vec::new()),
            width_mm: 0,
            height_mm: 0,
            subpixel: 0,
        }
    }

    /// Add mode
    pub fn add_mode(&self, mode: DrmMode) {
        self.modes.lock().push(mode);
    }

    /// Check connection
    pub fn detect(&self) -> DrmConnectorStatus {
        *self.connection.lock()
    }
}

// ============================================================================
// PLANE
// ============================================================================

pub struct DrmPlane {
    pub id: u32,
    pub possible_crtcs: u32,
    pub format_count: u32,
    pub formats: Vec<u32>,
    pub crtc_id: AtomicU32,
    pub fb_id: AtomicU32,
    pub crtc_x: AtomicU32,
    pub crtc_y: AtomicU32,
    pub crtc_w: AtomicU32,
    pub crtc_h: AtomicU32,
    pub src_x: AtomicU32,
    pub src_y: AtomicU32,
    pub src_w: AtomicU32,
    pub src_h: AtomicU32,
}

impl DrmPlane {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            possible_crtcs: 0xFFFF,
            format_count: 0,
            formats: Vec::new(),
            crtc_id: AtomicU32::new(0),
            fb_id: AtomicU32::new(0),
            crtc_x: AtomicU32::new(0),
            crtc_y: AtomicU32::new(0),
            crtc_w: AtomicU32::new(0),
            crtc_h: AtomicU32::new(0),
            src_x: AtomicU32::new(0),
            src_y: AtomicU32::new(0),
            src_w: AtomicU32::new(0),
            src_h: AtomicU32::new(0),
        }
    }
}

// ============================================================================
// DRM MANAGER
// ============================================================================

pub struct DrmManager {
    devices: Mutex<BTreeMap<u64, Arc<DrmDevice>>>,
    next_device_id: AtomicU64,
}

impl DrmManager {
    pub const fn new() -> Self {
        Self {
            devices: Mutex::new(BTreeMap::new()),
            next_device_id: AtomicU64::new(1),
        }
    }

    pub fn register_device(&self, name: &str) -> Arc<DrmDevice> {
        let id = self.next_device_id.fetch_add(1, Ordering::SeqCst);
        let device = Arc::new(DrmDevice::new(id, name));
        self.devices.lock().insert(id, device.clone());
        device
    }

    pub fn get_device(&self, id: u64) -> Option<Arc<DrmDevice>> {
        self.devices.lock().get(&id).cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref DRM_MANAGER: DrmManager = DrmManager::new();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

pub fn init() {
    // Create primary GPU device
    let gpu = DRM_MANAGER.register_device("card0");
    
    // Add default CRTC
    let crtc = Arc::new(DrmCrtc::new(0, 0));
    gpu.add_crtc(crtc);
    
    // Add default connector
    let connector = Arc::new(DrmConnector::new(0, 0)); // VGA
    gpu.add_connector(connector);
    
    crate::serial_println!("[DRM] DRM/KMS initialized");
}
