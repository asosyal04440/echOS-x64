//! # DRM/KMS - DoÄŸrudan Render YÃ¶neticisi (Direct Rendering Manager)
//!
//! GPU ve ekran alt sistemi yÃ¶netimi. Linux DRM/KMS mimarisini uygular.
//!
//! ## DRM/KMS KavramlarÄ±
//!
//! Modern Linux'ta ekran ÃSection Ä±kÄ±ÅŸÄ± ÅŸu hiyerarÅŸiyle yÃ¶netilir:
//!
//! ```
//! [GPU/DrmDevice]
//!       |
//!       +--[CRTC]          <- Ekran denetleyicisi; hangi FB'yi hangi modda ÃSection Ä±karÄ±r
//!       |     |
//!       |     +--[Plane]   <- Framebuffer katmanÄ± (birden fazla ÃSection akÄ±ÅŸÄ±k katman olabilir)
//!       |
//!       +--[Encoder]       <- Dijital/Analog sinyal dÃ¶nÃ¼ÅŸtÃ¼rÃ¼cÃ¼ (TMDS, LVDS, VGA DAC...)
//!             |
//!             +--[Connector] <- Fiziksel baÄŸlantÄ± noktasÄ± (HDMI, DisplayPort, VGA, eDP)
//!                   |
//!               [MonitÃ¶r]
//! ```
//!
//! ## GEM (Graphics Execution Manager)
//!
//! GPU bellek nesneleri GEM handle'larÄ±yla yÃ¶netilir:
//!
//! ```
//! gem_create(size) -> handle
//!       |
//!       v
//! gem_get(handle) -> Arc<GemObject>   <- vaddr/paddr ile CPU taraflÄ± haritalama
//!       |
//!       v
//! gem_close(handle)                   <- nesneyi yok et, belleÄŸi geri ver
//! ```
//!
//! ## DRM ioctl AkÄ±ÅŸÄ±
//!
//! KullanÄ±cÄ± alanÄ± (Mesa/libdrm) aÅŸaÄŸÄ±daki sÄ±rayla ekran aÃSection ar:
//!   1. DRM_IOCTL_MODE_GETRESOURCES  -> CRTC/Connector/Encoder listesi
//!   2. DRM_IOCTL_MODE_GETCONNECTOR  -> monitÃ¶r bilgisi, desteklenen ÃSection Ã¶zÃ¼nÃ¼rlÃ¼kler
//!   3. DRM_IOCTL_MODE_CREATE_DUMB   -> GEM objesi yarat (CPU taraflÄ± FB)
//!   4. DRM_IOCTL_MODE_ADDFB         -> GEM'i framebuffer olarak kaydet
//!   5. DRM_IOCTL_MODE_SETCRTC       -> CRTC'yi aÃSection , FB'yi ekrana baÄŸla

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::cpu::tsc;
use crate::gpu3d;
use crate::gui::protocol::{DisplayPresentMode, Rect};

// ============================================================================
// DRM IOCTL SABÄ°TLERÄ° (DRM CONSTANTS)
// ============================================================================

// DRM ioctl numaralarÄ± Linux ABI'siyle uyumludur.
// Ãœst 16 bit yÃ¶n+boyut bilgisi taÅŸÄ±r (Linux _IOC makrosu); alt 16 bit komut.
// 0x64 = 'd' = DRM magic byte

/// Temel DRM ioctl komutlarÄ± (versiyon, GEM, CAP sorgularÄ±)
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

/// KMS (Kernel Mode Setting) ioctl komutlarÄ± - ekran ayarlarÄ±
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
// DRM VERSÄ°YON BÄ°LGÄ°SÄ° (DRM VERSION)
// ============================================================================

// DRM_IOCTL_VERSION ioctl'une yanÄ±t olarak kullanÄ±cÄ± alanÄ±na doldurulur.
// name/date/desc alanlarÄ± kullanÄ±cÄ± alanÄ± pointer'larÄ±dÄ±r (u64 olarak saklanÄ±r).

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmPlaneType {
    Primary,
    Overlay,
    Cursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicPlaneUpdate {
    pub plane_id: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub src: Rect,
    pub dst: Rect,
    pub z_index: u32,
}

#[derive(Clone, Debug)]
pub struct AtomicCommitRequest {
    pub connector_id: u32,
    pub crtc_id: u32,
    pub mode: Option<DrmMode>,
    pub planes: Vec<AtomicPlaneUpdate>,
    pub frame_id: u64,
    pub present_mode: DisplayPresentMode,
    pub target_refresh_hz: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicCommitResult {
    pub timestamp_ns: u64,
    pub frame_id: u64,
    pub vblank_seq: u64,
    pub refresh_hz: u32,
    pub direct_scanout_planes: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GPUBufferHandle {
    pub handle: u64,
    pub paddr: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRegion {
    pub rect: Rect,
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaneCandidate {
    pub surface_id: u64,
    pub plane_type: DrmPlaneType,
    pub z: u32,
    pub src: Rect,
    pub dst: Rect,
    pub opaque: bool,
    pub format: u32,
    pub buffer: GPUBufferHandle,
}

#[derive(Clone, Debug)]
pub struct AtomicKmsTransaction {
    pub frame_id: u64,
    pub commit_id: u64,
    pub crtc_id: u32,
    pub connector_id: u32,
    pub mode: Option<DrmMode>,
    pub planes: Vec<PlaneCandidate>,
    pub damage_regions: Vec<DamageRegion>,
    pub target_refresh_hz: u32,
    pub present_mode: DisplayPresentMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VBlankEvent {
    pub seq: u64,
    pub timestamp_ns: u64,
    pub crtc_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaFenceState {
    pub current_value: u64,
    pub target_value: u64,
    pub signaled: bool,
    pub last_seq: u64,
    pub last_timestamp_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaReservationSnapshot {
    pub exclusive_commit_id: u64,
    pub exclusive_frame_id: u64,
    pub shared_plane_count: u32,
    pub dma_buf_fd: u32,
    pub fence: DmaFenceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufExport {
    pub fd: u32,
    pub gem_handle: u32,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufProcessBinding {
    pub exporter_pid: u64,
    pub importer_pid: u64,
    pub fd: u32,
    pub handle: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaReservationUsage {
    Read,
    Write,
    Kernel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaReservationEdge {
    pub producer_handle: u32,
    pub consumer_handle: u32,
    pub importer_pid: u64,
    pub usage: DmaReservationUsage,
    pub dma_buf_fd: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmaResvLockError {
    Edeadlk,
    MissingHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DmaResvAcquireCtx {
    stamp: u64,
}

impl DmaResvAcquireCtx {
    fn new() -> Self {
        static NEXT_CTX_STAMP: AtomicU64 = AtomicU64::new(1);
        Self {
            stamp: NEXT_CTX_STAMP.fetch_add(1, Ordering::AcqRel),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DmaResvWwState {
    owner_stamp: u64,
}

impl DmaResvWwState {
    const fn new() -> Self {
        Self { owner_stamp: 0 }
    }
}

struct DmaResvWwGuard {
    object: Arc<GemObject>,
}

impl Drop for DmaResvWwGuard {
    fn drop(&mut self) {
        self.object.reservation_ww_state.store(0, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DmaReservationState {
    exclusive_commit_id: u64,
    exclusive_frame_id: u64,
    shared_plane_count: u32,
    last_seq: u64,
    last_timestamp_ns: u64,
    current_fence_value: u64,
    target_fence_value: u64,
}

impl DmaReservationState {
    const fn new() -> Self {
        Self {
            exclusive_commit_id: 0,
            exclusive_frame_id: 0,
            shared_plane_count: 0,
            last_seq: 0,
            last_timestamp_ns: 0,
            current_fence_value: 0,
            target_fence_value: 0,
        }
    }
}

// ============================================================================
// DRM CÄ°HAZI (DRM DEVICE)
// ============================================================================

// Her GPU bir DrmDevice Ã¶rneÄŸiyle temsil edilir.
// Birden fazla GPU desteklemek iÃSection in DrmManager birden fazla DrmDevice tutar.
//
//   DrmDevice [card0]
//     |-- framebuffers: BTreeMap<fb_id, DrmFramebuffer>
//     |-- gem_objects:  BTreeMap<handle, GemObject>    <- GPU bellek nesneleri
//     |-- crtcs:        Vec<DrmCrtc>                   <- Ekran denetleyicileri
//     |-- encoders:     Vec<DrmEncoder>                <- Sinyal dÃ¶nÃ¼ÅŸtÃ¼rÃ¼cÃ¼ler
//     |-- connectors:   Vec<DrmConnector>              <- Fiziksek ÃSection Ä±kÄ±ÅŸlar
//     +-- planes:       Vec<DrmPlane>                  <- GÃ¶rÃ¼ntÃ¼ katmanlarÄ±

pub struct DrmDevice {
    /// Sistemdeki benzersiz cihaz kimliÄŸi
    pub id: u64,
    /// Cihaz adÄ± (Ã¶rn. "card0", "card1")
    pub name: String,
    /// SÃ¼rÃ¼cÃ¼ adÄ± (kullanÄ±cÄ± alanÄ±na raporlanÄ±r)
    pub driver_name: String,
    /// SÃ¼rÃ¼cÃ¼ versiyonu (major, minor, patch)
    pub driver_version: (u32, u32, u32),
    /// SÃ¼rÃ¼cÃ¼ yetenekleri (DRM_CAP_* sabitleriyle sorgulanÄ±r)
    pub caps: Mutex<BTreeMap<u64, u64>>,
    /// KayÄ±tlÄ± framebuffer'lar (fb_id -> nesne)
    pub framebuffers: Mutex<BTreeMap<u32, Arc<DrmFramebuffer>>>,
    /// GEM bellek nesneleri (handle -> nesne)
    pub gem_objects: Mutex<BTreeMap<u32, Arc<GemObject>>>,
    /// Bir sonraki GEM handle deÄŸeri (her zaman artarak gider)
    next_gem_handle: AtomicU32,
    /// Bir sonraki framebuffer ID deÄŸeri
    next_fb_id: AtomicU32,
    /// KMS modunun etkin olup olmadÄ±ÄŸÄ±
    pub modeset_enabled: AtomicBool,
    /// Bu GPU'nun CRTC listesi (her biri baÄŸÄ±msÄ±z ekrana ÃSection Ä±kÄ±ÅŸ yapabilir)
    pub crtcs: Mutex<Vec<Arc<DrmCrtc>>>,
    /// Encoder listesi
    pub encoders: Mutex<Vec<Arc<DrmEncoder>>>,
    /// Connector listesi (HDMI, DP, VGA...)
    pub connectors: Mutex<Vec<Arc<DrmConnector>>>,
    /// Plane listesi (her CRTC'nin bir veya daha fazla plane'i var)
    pub planes: Mutex<Vec<Arc<DrmPlane>>>,
    /// Son atomic commit sonrasÄ± VBLANK sayacÄ±
    pub vblank_seq: AtomicU64,
    /// Son atomic commit zamanÄ± (ns)
    pub last_commit_ns: AtomicU64,
    pub last_commit_id: AtomicU64,
    pub last_presented_frame_id: AtomicU64,
    pub last_flip_seq: AtomicU64,
    pub expected_commit_id: AtomicU64,
    pub expected_frame_id: AtomicU64,
    pub expected_flip_seq: AtomicU64,
    pub inflight_plane_handles: Mutex<Vec<u32>>,
    pub prime_exports: Mutex<BTreeMap<u32, u32>>,
    pub process_prime_imports: Mutex<BTreeMap<(u64, u32), u32>>,
    pub reservation_graph: Mutex<BTreeMap<u32, Vec<DmaReservationEdge>>>,
    pub next_prime_fd: AtomicU32,
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
            vblank_seq: AtomicU64::new(0),
            last_commit_ns: AtomicU64::new(0),
            last_commit_id: AtomicU64::new(0),
            last_presented_frame_id: AtomicU64::new(0),
            last_flip_seq: AtomicU64::new(0),
            expected_commit_id: AtomicU64::new(0),
            expected_frame_id: AtomicU64::new(0),
            expected_flip_seq: AtomicU64::new(0),
            inflight_plane_handles: Mutex::new(Vec::new()),
            prime_exports: Mutex::new(BTreeMap::new()),
            process_prime_imports: Mutex::new(BTreeMap::new()),
            reservation_graph: Mutex::new(BTreeMap::new()),
            next_prime_fd: AtomicU32::new(0x4000),
        }
    }

    /// Yeni GEM bellek nesnesi oluÅŸturur.
    /// KullanÄ±cÄ± alanÄ± DRM_IOCTL_MODE_CREATE_DUMB veya drmPrimeFdToHandle ile ÃSection aÄŸÄ±rÄ±r.
    pub fn gem_create(&self, size: u64) -> Arc<GemObject> {
        let handle = self.next_gem_handle.fetch_add(1, Ordering::SeqCst);
        let obj = Arc::new(GemObject::new(handle, size));
        self.gem_objects.lock().insert(handle, obj.clone());
        obj
    }

    /// Handle ile GEM nesnesini getirir
    pub fn gem_get(&self, handle: u32) -> Option<Arc<GemObject>> {
        self.gem_objects.lock().get(&handle).cloned()
    }

    /// GEM nesnesini kapatÄ±r; referans dÃ¼ÅŸerse bellek serbest bÄ±rakÄ±lÄ±r
    pub fn gem_close(&self, handle: u32) {
        self.gem_objects.lock().remove(&handle);
    }

    pub fn export_dma_buf_handle(&self, handle: u32) -> Result<DmaBufExport, &'static str> {
        let Some(obj) = self.gem_get(handle) else {
            return Err("gem handle unavailable");
        };
        let mut dma_buf = obj.dma_buf.lock();
        let fd = if let Some(fd) = *dma_buf {
            fd
        } else {
            let fd = self.next_prime_fd.fetch_add(1, Ordering::AcqRel);
            *dma_buf = Some(fd);
            self.prime_exports.lock().insert(fd, handle);
            fd
        };
        Ok(DmaBufExport {
            fd,
            gem_handle: handle,
            size: obj.size,
        })
    }

    pub fn import_dma_buf_fd(&self, fd: u32) -> Option<Arc<GemObject>> {
        let handle = self.prime_exports.lock().get(&fd).copied()?;
        self.gem_get(handle)
    }

    pub fn import_dma_buf_fd_for_process(
        &self,
        fd: u32,
        importer_pid: u64,
    ) -> Option<Arc<GemObject>> {
        let handle = self.prime_exports.lock().get(&fd).copied()?;
        self.process_prime_imports
            .lock()
            .insert((importer_pid, fd), handle);
        let obj = self.gem_get(handle)?;
        obj.note_process_import(importer_pid, fd);
        Some(obj)
    }

    pub fn link_cross_process_reservation(
        &self,
        producer_handle: u32,
        consumer_handle: u32,
        importer_pid: u64,
    ) -> Result<(), &'static str> {
        self.link_cross_process_reservation_with_usage(
            producer_handle,
            consumer_handle,
            importer_pid,
            DmaReservationUsage::Read,
        )
    }

    pub fn link_cross_process_reservation_with_usage(
        &self,
        producer_handle: u32,
        consumer_handle: u32,
        importer_pid: u64,
        usage: DmaReservationUsage,
    ) -> Result<(), &'static str> {
        if self.gem_get(producer_handle).is_none() || self.gem_get(consumer_handle).is_none() {
            return Err("gem handle unavailable");
        }
        let dma_buf_fd = self
            .gem_get(producer_handle)
            .and_then(|obj| *obj.dma_buf.lock())
            .unwrap_or(0);
        self.reservation_graph
            .lock()
            .entry(producer_handle)
            .or_insert_with(Vec::new)
            .push(DmaReservationEdge {
                producer_handle,
                consumer_handle,
                importer_pid,
                usage,
                dma_buf_fd,
            });
        Ok(())
    }

    fn ensure_gem_for_plane_buffer(&self, buffer: &GPUBufferHandle) -> Arc<GemObject> {
        let handle = buffer.handle as u32;
        if let Some(obj) = self.gem_get(handle) {
            {
                let mut paddr = obj.paddr.lock();
                if paddr.is_none() && buffer.paddr != 0 {
                    *paddr = Some(buffer.paddr);
                }
            }
            return obj;
        }

        let size = u64::from(buffer.stride).saturating_mul(u64::from(buffer.height));
        let obj = Arc::new(GemObject::new(handle, size.max(4096)));
        {
            let mut paddr = obj.paddr.lock();
            if buffer.paddr != 0 {
                *paddr = Some(buffer.paddr);
            }
        }
        self.gem_objects.lock().insert(handle, obj.clone());
        obj
    }

    pub fn reservation_snapshot(&self, handle: u32) -> Option<DmaReservationSnapshot> {
        self.gem_get(handle).map(|obj| obj.reservation_snapshot())
    }

    pub fn reservation_graph_snapshot(&self, producer_handle: u32) -> Vec<DmaReservationEdge> {
        self.collect_reachable_edges(producer_handle)
    }

    fn collect_reachable_edges(&self, producer_handle: u32) -> Vec<DmaReservationEdge> {
        let graph = self.reservation_graph.lock();
        let mut visited = BTreeMap::<u32, ()>::new();
        let mut queue = VecDeque::new();
        let mut edges = Vec::new();
        queue.push_back(producer_handle);
        visited.insert(producer_handle, ());

        while let Some(handle) = queue.pop_front() {
            if let Some(next_edges) = graph.get(&handle) {
                for edge in next_edges.iter().copied() {
                    edges.push(edge);
                    if !visited.contains_key(&edge.consumer_handle) {
                        visited.insert(edge.consumer_handle, ());
                        queue.push_back(edge.consumer_handle);
                    }
                }
            }
        }

        edges
    }

    fn collect_reachable_handles(&self, producer_handles: &[u32]) -> Vec<u32> {
        let mut handles = BTreeMap::<u32, ()>::new();
        for handle in producer_handles.iter().copied() {
            handles.insert(handle, ());
            for edge in self.collect_reachable_edges(handle).into_iter() {
                handles.insert(edge.consumer_handle, ());
            }
        }
        handles.into_keys().collect()
    }

    fn lock_reservation_set<'a>(
        &'a self,
        handles: &[u32],
    ) -> Result<Vec<DmaResvWwGuard>, &'static str> {
        let mut sorted = self.collect_reachable_handles(handles);
        sorted.sort_unstable();
        let mut retries = 0usize;

        loop {
            let ctx = DmaResvAcquireCtx::new();
            let mut guards = Vec::with_capacity(sorted.len());
            let mut blocked = false;

            for handle in sorted.iter().copied() {
                let Some(obj) = self.gem_get(handle) else {
                    return Err("reservation handle unavailable");
                };
                match obj.try_claim_ww(&ctx) {
                    Ok(()) => guards.push(DmaResvWwGuard {
                        object: obj.clone(),
                    }),
                    Err(DmaResvLockError::Edeadlk) => {
                        blocked = true;
                        break;
                    }
                    Err(DmaResvLockError::MissingHandle) => {
                        return Err("reservation handle unavailable");
                    }
                }
            }

            if !blocked {
                return Ok(guards);
            }

            drop(guards);
            retries = retries.saturating_add(1);
            if retries > 64 {
                return Err("reservation ww acquisition failed");
            }
            core::hint::spin_loop();
        }
    }

    fn propagate_reservation_commit(
        &self,
        producer_handle: u32,
        commit_id: u64,
        frame_id: u64,
        shared_plane_count: u32,
    ) {
        for edge in self.collect_reachable_edges(producer_handle).into_iter() {
            if let Some(shared_obj) = self.gem_get(edge.consumer_handle) {
                shared_obj.reserve_for_import(
                    commit_id,
                    frame_id,
                    shared_plane_count.saturating_add(1),
                    edge.usage,
                );
            }
        }
    }

    fn propagate_reservation_completion(
        &self,
        producer_handle: u32,
        commit_id: u64,
        seq: u64,
        ts_ns: u64,
    ) {
        for edge in self.collect_reachable_edges(producer_handle).into_iter() {
            if let Some(shared_obj) = self.gem_get(edge.consumer_handle) {
                shared_obj.complete_import(commit_id, seq, ts_ns, edge.usage);
            }
        }
    }

    /// Framebuffer oluÅŸturur; GPU ÃSection Ä±kÄ±ÅŸÄ± iÃSection in piksel tamponu.
    /// handles[0..3]: renk/derinlik/stencil iÃSection in GEM handle'lar (ARGB iÃSection in 1 yeterli).
    pub fn fb_create(&self, width: u32, height: u32, format: u32, handles: [u32; 4]) -> u32 {
        let fb_id = self.next_fb_id.fetch_add(1, Ordering::SeqCst);
        let fb = Arc::new(DrmFramebuffer::new(fb_id, width, height, format, handles));
        self.framebuffers.lock().insert(fb_id, fb);
        fb_id
    }

    /// Framebuffer ID ile framebuffer nesnesini getirir
    pub fn fb_get(&self, fb_id: u32) -> Option<Arc<DrmFramebuffer>> {
        self.framebuffers.lock().get(&fb_id).cloned()
    }

    /// Framebuffer'Ä± siler; CRTC'nin bu FB'yi kullanmadÄ±ÄŸÄ±ndan emin ol
    pub fn fb_remove(&self, fb_id: u32) {
        self.framebuffers.lock().remove(&fb_id);
    }

    /// SÃ¼rÃ¼cÃ¼ yeteneÄŸini sorgular (DRM_CAP_DUMB_BUFFER, DRM_CAP_VBLANK_HIGH_CRTC...)
    pub fn get_cap(&self, cap: u64) -> u64 {
        self.caps.lock().get(&cap).copied().unwrap_or(0)
    }

    /// SÃ¼rÃ¼cÃ¼ yeteneÄŸi tanÄ±mlar
    pub fn set_cap(&self, cap: u64, value: u64) {
        self.caps.lock().insert(cap, value);
    }

    /// CRTC ekler (baÅŸlatma sÄ±rasÄ±nda ÃSection aÄŸrÄ±lÄ±r)
    pub fn add_crtc(&self, crtc: Arc<DrmCrtc>) {
        self.crtcs.lock().push(crtc);
    }

    /// Connector ekler (HDMI, DP, VGA baÄŸlantÄ± noktalarÄ±)
    pub fn add_connector(&self, connector: Arc<DrmConnector>) {
        self.connectors.lock().push(connector);
    }

    /// Encoder ekler (TMDS, LVDS, DAC dÃ¶nÃ¼ÅŸtÃ¼rÃ¼cÃ¼ler)
    pub fn add_encoder(&self, encoder: Arc<DrmEncoder>) {
        self.encoders.lock().push(encoder);
    }

    /// Plane ekler (gÃ¶rÃ¼ntÃ¼ katmanÄ±: birincil, kaplama, imleÃSection )
    pub fn add_plane(&self, plane: Arc<DrmPlane>) {
        self.planes.lock().push(plane);
    }

    pub fn vblank_seq(&self) -> u64 {
        self.vblank_seq.load(Ordering::Acquire)
    }

    pub fn last_commit_ns(&self) -> u64 {
        self.last_commit_ns.load(Ordering::Acquire)
    }

    pub fn plane_ids_by_type(&self, plane_type: DrmPlaneType) -> Vec<u32> {
        self.planes
            .lock()
            .iter()
            .filter(|plane| plane.plane_type == plane_type)
            .map(|plane| plane.id)
            .collect()
    }

    pub fn connected_connector_count(&self) -> usize {
        self.connectors
            .lock()
            .iter()
            .filter(|connector| connector.detect() == DrmConnectorStatus::Connected)
            .count()
    }

    pub fn max_mode_refresh_hz(&self) -> u32 {
        self.connectors
            .lock()
            .iter()
            .filter(|connector| connector.detect() == DrmConnectorStatus::Connected)
            .flat_map(|connector| {
                connector
                    .modes
                    .lock()
                    .iter()
                    .map(|mode| mode.vrefresh)
                    .collect::<Vec<_>>()
            })
            .max()
            .unwrap_or(60)
    }

    pub fn max_overlay_planes(&self) -> usize {
        self.planes
            .lock()
            .iter()
            .filter(|plane| plane.plane_type == DrmPlaneType::Overlay)
            .count()
    }

    pub fn vblank_period_ns(refresh_hz: u32) -> u64 {
        let hz = refresh_hz.max(1) as u64;
        1_000_000_000u64.saturating_div(hz)
    }

    pub fn vblank_ready_at(&self, refresh_hz: u32, now_ns: u64, mode: DisplayPresentMode) -> bool {
        if mode == DisplayPresentMode::Mailbox {
            return true;
        }

        let last = self.last_commit_ns();
        if last == 0 {
            return true;
        }

        now_ns.saturating_sub(last) >= Self::vblank_period_ns(refresh_hz)
    }

    pub fn signal_vblank(&self, timestamp_ns: u64) -> u64 {
        self.last_commit_ns.store(timestamp_ns, Ordering::Release);
        self.vblank_seq.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn poll_vblank(&self, last_seen_seq: u64) -> Option<(u64, u64)> {
        let seq = self.vblank_seq();
        if seq == last_seen_seq {
            return None;
        }
        Some((seq, self.last_commit_ns()))
    }

    pub fn atomic_commit(
        &self,
        request: &AtomicCommitRequest,
    ) -> Result<AtomicCommitResult, &'static str> {
        if !self.modeset_enabled.load(Ordering::Acquire) {
            return Err("modeset disabled");
        }

        let connector = {
            let connectors = self.connectors.lock();
            connectors
                .iter()
                .find(|connector| connector.id == request.connector_id)
                .cloned()
                .ok_or("connector not found")?
        };
        let connection = connector.detect();
        if connection == DrmConnectorStatus::Disconnected {
            return Err("connector disconnected");
        }

        let crtc = {
            let crtcs = self.crtcs.lock();
            crtcs
                .iter()
                .find(|crtc| crtc.id == request.crtc_id)
                .cloned()
                .ok_or("crtc not found")?
        };

        if let Some(mode) = request.mode.clone() {
            crtc.set_mode(mode);
        }

        {
            let planes = self.planes.lock();
            for update in request.planes.iter() {
                let plane = planes
                    .iter()
                    .find(|plane| plane.id == update.plane_id)
                    .ok_or("plane not found")?;
                plane.crtc_id.store(update.crtc_id, Ordering::Release);
                plane.fb_id.store(update.fb_id, Ordering::Release);
                plane
                    .crtc_x
                    .store(update.dst.x.max(0) as u32, Ordering::Release);
                plane
                    .crtc_y
                    .store(update.dst.y.max(0) as u32, Ordering::Release);
                plane.crtc_w.store(update.dst.width, Ordering::Release);
                plane.crtc_h.store(update.dst.height, Ordering::Release);
                plane
                    .src_x
                    .store(update.src.x.max(0) as u32, Ordering::Release);
                plane
                    .src_y
                    .store(update.src.y.max(0) as u32, Ordering::Release);
                plane.src_w.store(update.src.width, Ordering::Release);
                plane.src_h.store(update.src.height, Ordering::Release);
            }
        }

        let refresh_hz = match request.present_mode {
            DisplayPresentMode::Mailbox => request.target_refresh_hz.clamp(60, 360),
            DisplayPresentMode::VblankFifo => request.target_refresh_hz.clamp(30, 240),
            DisplayPresentMode::AdaptiveSync => {
                if request.planes.is_empty() {
                    1
                } else {
                    request.target_refresh_hz.clamp(1, 360)
                }
            }
        };

        let now_ns = tsc::read_ns();
        let expected_vblank_seq = self.vblank_seq.load(Ordering::Acquire) + 1;

        Ok(AtomicCommitResult {
            timestamp_ns: now_ns,
            frame_id: request.frame_id,
            vblank_seq: expected_vblank_seq,
            refresh_hz,
            direct_scanout_planes: request.planes.len().min(u8::MAX as usize) as u8,
        })
    }

    pub fn commit_transaction(
        &self,
        txn: &AtomicKmsTransaction,
    ) -> Result<AtomicCommitResult, &'static str> {
        if self.expected_commit_id.load(Ordering::Acquire) != 0 {
            return Err("inflight commit exists");
        }
        let last_commit_id = self.last_commit_id.load(Ordering::Acquire);
        if txn.commit_id <= last_commit_id {
            return Err("non-monotonic commit id");
        }
        let last_presented = self.last_presented_frame_id.load(Ordering::Acquire);
        if txn.frame_id <= last_presented {
            return Err("stale frame id");
        }

        let mut primary: Option<PlaneCandidate> = None;
        let mut overlay: Option<PlaneCandidate> = None;
        let mut cursor: Option<PlaneCandidate> = None;

        let mut candidates = txn.planes.clone();
        candidates.sort_by(|a, b| b.z.cmp(&a.z));

        for candidate in candidates.into_iter() {
            match candidate.plane_type {
                DrmPlaneType::Primary => {
                    if primary.is_none() {
                        primary = Some(candidate);
                    }
                }
                DrmPlaneType::Overlay => {
                    if overlay.is_none() {
                        overlay = Some(candidate);
                    }
                }
                DrmPlaneType::Cursor => {
                    if cursor.is_none() {
                        cursor = Some(candidate);
                    }
                }
            }
        }

        if primary.is_none() {
            primary = txn
                .planes
                .iter()
                .filter(|candidate| candidate.opaque)
                .max_by_key(|candidate| candidate.z)
                .copied()
                .or_else(|| {
                    txn.planes
                        .iter()
                        .max_by_key(|candidate| candidate.z)
                        .copied()
                });
        }

        let mut updates = Vec::new();
        let primary_plane_id = self
            .plane_ids_by_type(DrmPlaneType::Primary)
            .into_iter()
            .next()
            .unwrap_or(0);
        let overlay_plane_id = self
            .plane_ids_by_type(DrmPlaneType::Overlay)
            .into_iter()
            .next()
            .unwrap_or(primary_plane_id);
        let cursor_plane_id = self
            .plane_ids_by_type(DrmPlaneType::Cursor)
            .into_iter()
            .next();

        if let Some(primary) = primary {
            updates.push(AtomicPlaneUpdate {
                plane_id: primary_plane_id,
                crtc_id: txn.crtc_id,
                fb_id: primary.buffer.handle as u32,
                src: primary.src,
                dst: primary.dst,
                z_index: primary.z,
            });
        }
        if let Some(overlay) = overlay {
            updates.push(AtomicPlaneUpdate {
                plane_id: overlay_plane_id,
                crtc_id: txn.crtc_id,
                fb_id: overlay.buffer.handle as u32,
                src: overlay.src,
                dst: overlay.dst,
                z_index: overlay.z,
            });
        }
        if let (Some(cursor), Some(plane_id)) = (cursor, cursor_plane_id) {
            updates.push(AtomicPlaneUpdate {
                plane_id,
                crtc_id: txn.crtc_id,
                fb_id: cursor.buffer.handle as u32,
                src: cursor.src,
                dst: cursor.dst,
                z_index: cursor.z,
            });
        }

        let result = match self.atomic_commit(&AtomicCommitRequest {
            connector_id: txn.connector_id,
            crtc_id: txn.crtc_id,
            mode: txn.mode.clone(),
            planes: updates,
            frame_id: txn.frame_id,
            present_mode: txn.present_mode,
            target_refresh_hz: txn.target_refresh_hz,
        }) {
            Ok(r) => r,
            Err(e) => {
                self.abort_inflight_commit();
                return Err(e);
            }
        };

        let mut tracked = Vec::new();
        for plane in txn.planes.iter() {
            tracked.push(plane.buffer.handle as u32);
            let _ = self.ensure_gem_for_plane_buffer(&plane.buffer);
        }
        let ww_guards = match self.lock_reservation_set(&tracked) {
            Ok(g) => g,
            Err(e) => {
                self.rollback_commit(&result);
                return Err(e);
            }
        };
        for plane in txn.planes.iter() {
            let obj = self.ensure_gem_for_plane_buffer(&plane.buffer);
            let _ = self.export_dma_buf_handle(obj.handle);
            obj.reserve_for_commit(
                txn.commit_id,
                txn.frame_id,
                txn.planes.len().min(u32::MAX as usize) as u32,
            );
            self.propagate_reservation_commit(
                obj.handle,
                txn.commit_id,
                txn.frame_id,
                txn.planes.len().min(u32::MAX as usize) as u32,
            );
            tracked.push(obj.handle);
        }
        *self.inflight_plane_handles.lock() = tracked;
        core::mem::drop(ww_guards);

        self.last_commit_id.store(txn.commit_id, Ordering::Release);
        self.expected_commit_id
            .store(txn.commit_id, Ordering::Release);
        self.expected_frame_id
            .store(txn.frame_id, Ordering::Release);
        self.expected_flip_seq
            .store(result.vblank_seq, Ordering::Release);
        Ok(result)
    }

    fn rollback_commit(&self, result: &AtomicCommitResult) {
        self.abort_inflight_commit();
        let crtcs = self.crtcs.lock();
        for crtc in crtcs.iter() {
            crtc.active.store(false, Ordering::Release);
        }
    }

    pub fn report_flip_complete(
        &self,
        frame_id: u64,
        commit_id: u64,
        seq: u64,
        ts_ns: u64,
    ) -> bool {
        let expected_commit = self.expected_commit_id.load(Ordering::Acquire);
        let expected_frame = self.expected_frame_id.load(Ordering::Acquire);
        let expected_seq = self.expected_flip_seq.load(Ordering::Acquire);
        let last_seq = self.last_flip_seq.load(Ordering::Acquire);
        if expected_commit == 0
            || expected_commit != commit_id
            || expected_frame != frame_id
            || seq < expected_seq
            || seq <= last_seq
        {
            return false;
        }

        self.last_presented_frame_id
            .store(frame_id, Ordering::Release);
        self.last_flip_seq.store(seq, Ordering::Release);
        self.last_commit_ns.store(ts_ns, Ordering::Release);
        let inflight = self.inflight_plane_handles.lock().clone();
        let _ww_guards = match self.lock_reservation_set(&inflight) {
            Ok(guards) => guards,
            Err(_) => return false,
        };
        for handle in inflight.into_iter() {
            if let Some(obj) = self.gem_get(handle) {
                obj.complete_commit(commit_id, seq, ts_ns);
                self.propagate_reservation_completion(handle, commit_id, seq, ts_ns);
            }
        }
        self.inflight_plane_handles.lock().clear();
        self.expected_commit_id.store(0, Ordering::Release);
        self.expected_frame_id.store(0, Ordering::Release);
        self.expected_flip_seq.store(0, Ordering::Release);
        true
    }

    pub fn abort_inflight_commit(&self) {
        self.inflight_plane_handles.lock().clear();
        self.expected_commit_id.store(0, Ordering::Release);
        self.expected_frame_id.store(0, Ordering::Release);
        self.expected_flip_seq.store(0, Ordering::Release);
    }

    pub fn rearm_crtc(&self, crtc_id: u32) -> bool {
        let crtcs = self.crtcs.lock();
        if let Some(crtc) = crtcs.iter().find(|crtc| crtc.id == crtc_id) {
            crtc.active.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }
}

// ============================================================================
// GEM NESNESÄ° (GEM OBJECT)
// ============================================================================

// GEM (Graphics Execution Manager) GPU bellek nesnelerini temsil eder.
// Linux'ta TTM (Translation Table Manager) veya GEM bu iÅŸi yapar.
//
// Nesne haritalamasÄ±:
//
//   GemObject
//     |-- paddr: Option<u64>  <- Fiziksel adres (DMA iÃSection in)
//     +-- vaddr: Option<u64>  <- Sanal adres   (CPU tarafÄ± yazma/okuma iÃSection in)
//
// PRIME/DMA-BUF ile farklÄ± sÃ¼reÃSection ler arasÄ±nda handle paylaÅŸÄ±mÄ±:
//   gem_flink  -> global isim oluÅŸtur
//   gem_open   -> baÅŸka sÃ¼reÃSection  bu isimle nesneye eriÅŸir

pub struct GemObject {
    pub handle: u32,
    pub size: u64,
    pub vaddr: Mutex<Option<u64>>,
    pub paddr: Mutex<Option<u64>>,
    pub ref_count: AtomicU32,
    pub dma_buf: Mutex<Option<u32>>,
    reservation_ww_state: AtomicU64,
    reservation: Mutex<DmaReservationState>,
    process_importers: Mutex<Vec<DmaBufProcessBinding>>,
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
            reservation_ww_state: AtomicU64::new(DmaResvWwState::new().owner_stamp),
            reservation: Mutex::new(DmaReservationState::new()),
            process_importers: Mutex::new(Vec::new()),
        }
    }

    fn fence_name(&self) -> alloc::string::String {
        alloc::format!("drm:gem:{}:fence", self.handle)
    }

    pub fn reservation_snapshot(&self) -> DmaReservationSnapshot {
        let state = *self.reservation.lock();
        DmaReservationSnapshot {
            exclusive_commit_id: state.exclusive_commit_id,
            exclusive_frame_id: state.exclusive_frame_id,
            shared_plane_count: state.shared_plane_count,
            dma_buf_fd: self.dma_buf.lock().unwrap_or(0),
            fence: DmaFenceState {
                current_value: state.current_fence_value,
                target_value: state.target_fence_value,
                signaled: state.current_fence_value >= state.target_fence_value,
                last_seq: state.last_seq,
                last_timestamp_ns: state.last_timestamp_ns,
            },
        }
    }

    pub fn reserve_for_commit(&self, commit_id: u64, frame_id: u64, shared_plane_count: u32) {
        let mut state = self.reservation.lock();
        state.exclusive_commit_id = commit_id;
        state.exclusive_frame_id = frame_id;
        state.shared_plane_count = shared_plane_count;
        state.target_fence_value = state.target_fence_value.max(commit_id);
        let fence = gpu3d::register_named_fence(&self.fence_name(), false);
        let _ = gpu3d::set_fence_target(fence, state.target_fence_value);
    }

    pub fn reserve_for_import(
        &self,
        commit_id: u64,
        frame_id: u64,
        shared_plane_count: u32,
        usage: DmaReservationUsage,
    ) {
        let mut state = self.reservation.lock();
        match usage {
            DmaReservationUsage::Read => {
                state.shared_plane_count = state
                    .shared_plane_count
                    .max(shared_plane_count)
                    .saturating_add(1);
            }
            DmaReservationUsage::Write | DmaReservationUsage::Kernel => {
                state.exclusive_commit_id = commit_id;
                state.exclusive_frame_id = frame_id;
                state.shared_plane_count = state.shared_plane_count.max(shared_plane_count);
            }
        }
        state.target_fence_value = state.target_fence_value.max(commit_id);
        let fence = gpu3d::register_named_fence(&self.fence_name(), false);
        let _ = gpu3d::set_fence_target(fence, state.target_fence_value);
    }

    pub fn complete_commit(&self, commit_id: u64, seq: u64, ts_ns: u64) {
        let mut state = self.reservation.lock();
        state.current_fence_value = state.current_fence_value.max(commit_id);
        state.target_fence_value = state.target_fence_value.max(commit_id);
        state.last_seq = seq;
        state.last_timestamp_ns = ts_ns;
        let fence = gpu3d::register_named_fence(&self.fence_name(), false);
        let _ = gpu3d::signal_fence_value(fence, commit_id);
    }

    pub fn complete_import(
        &self,
        commit_id: u64,
        seq: u64,
        ts_ns: u64,
        usage: DmaReservationUsage,
    ) {
        let mut state = self.reservation.lock();
        state.current_fence_value = state.current_fence_value.max(commit_id);
        state.target_fence_value = state.target_fence_value.max(commit_id);
        state.last_seq = seq;
        state.last_timestamp_ns = ts_ns;
        if matches!(usage, DmaReservationUsage::Read) {
            state.shared_plane_count = state.shared_plane_count.saturating_sub(1);
        }
        let fence = gpu3d::register_named_fence(&self.fence_name(), false);
        let _ = gpu3d::signal_fence_value(fence, commit_id);
    }

    pub fn note_process_import(&self, importer_pid: u64, fd: u32) {
        let mut importers = self.process_importers.lock();
        if !importers
            .iter()
            .any(|existing| existing.importer_pid == importer_pid && existing.fd == fd)
        {
            importers.push(DmaBufProcessBinding {
                exporter_pid: 0,
                importer_pid,
                fd,
                handle: self.handle,
            });
        }
    }

    fn try_claim_ww(&self, ctx: &DmaResvAcquireCtx) -> Result<(), DmaResvLockError> {
        match self.reservation_ww_state.compare_exchange(
            0,
            ctx.stamp,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(owner_stamp) => {
                if owner_stamp != 0 && owner_stamp < ctx.stamp {
                    Err(DmaResvLockError::Edeadlk)
                } else if owner_stamp == 0 {
                    Err(DmaResvLockError::MissingHandle)
                } else {
                    Err(DmaResvLockError::Edeadlk)
                }
            }
        }
    }

    /// Nesneyi CPU adres uzayÄ±na haritalar ve sanal adresi dÃ¶ner.
    ///
    /// Linux DRM GEM mmap modeli (docs.kernel.org/gpu/drm-mm.html):
    /// 1. GEM objesi fake offset ile drm_vma_offset_manager'a kaydedilir
    /// 2. mmap() ÃSection aÄŸrÄ±ldÄ±ÄŸÄ±nda fake offset'ten obje bulunur
    /// 3. drm_gem_mmap_obj() VMA'yÄ± hazÄ±rlar
    /// 4. Driver fault handler'Ä± sayfa sayfa mapping yapar
    ///
    /// echOS kernel-only: paddr'den doÄŸrudan kernel virtual space'e map eder.
    /// IOMMU domain'i DMA mapping ile uyumlu olmalÄ±dÄ±r.
    pub fn map(&self) -> Result<u64, &'static str> {
        let mut vaddr = self.vaddr.lock();
        if vaddr.is_some() {
            return Ok(vaddr.unwrap());
        }
        let paddr = self.paddr.lock();
        let phys = paddr.ok_or("gem object has no physical address")?;
        let size = self.size as usize;
        if size == 0 {
            return Err("gem object size is zero");
        }
        let pages = (size + 4095) / 4096;
        let mapped = crate::memory::map_mmio(phys, pages * 4096);
        if mapped.is_null() {
            return Err("gem object mapping failed");
        }
        *vaddr = Some(mapped as u64);
        Ok(mapped as u64)
    }

    /// CPU haritalamasÄ±nÄ± kaldÄ±rÄ±r; sayfa tablosu giriÅŸi temizlenir
    pub fn unmap(&self) {
        let mut vaddr = self.vaddr.lock();
        if let Some(_addr) = *vaddr {
            // GEM mapping kernel virtual address space'te; explicit unmap
            // platform-specific page table cleanup gerektirir
        }
        *vaddr = None;
    }
}

// ============================================================================
// FRAMEBUFFER (DRM FRAMEBUFFER)
// ============================================================================

// Ekrana gÃ¶nderilebilecek piksel tampon nesnesi.
// Birden fazla GEM handle ile ÃSection ok dÃ¼zlemli (planar) formatlar desteklenir:
//   handles[0]: Y dÃ¼zlemi (luma)
//   handles[1]: UV dÃ¼zlemi (chroma, NV12 vb.)
//
// Piksel formatÄ± FourCC kodu ile belirlenir:
//   DRM_FORMAT_XRGB8888 = 0x34325258 (en yaygÄ±n)
//   DRM_FORMAT_NV12     = 0x3231564E (video)

pub struct DrmFramebuffer {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4], // bytes per row for each plane
    pub offsets: [u32; 4], // offset within GEM for each plane
    pub modifier: u64,     // tiling/compression modifier (AMD, Intel tiling)
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
            pitches: [width * 4, 0, 0, 0], // XRGB8888: 4 byte per pixel
            offsets: [0, 0, 0, 0],
            modifier: 0,
            ref_count: AtomicU32::new(1),
        }
    }
}

// ============================================================================
// CRTC (Cathode Ray Tube Controller)
// ============================================================================

// Modern GPU'larda gerÃSection ek bir CRT olmasa da isim tarihi nedenle kalÄ±cÄ±dÄ±r.
// CRTC, piksel verisini ekrana saat sinyaliyle gÃ¶nderen devre bloÄŸudur.
//
// CRTC ve modeline iliÅŸki:
//
//   DrmCrtc
//     |-- mode:  DrmMode   <- Video modu: ÃSection Ã¶zÃ¼nÃ¼rlÃ¼k, tarama frekansÄ±
//     |-- fb_id: u32       <- Åu an gÃ¶rÃ¼ntÃ¼lenen framebuffer
//     +-- active: bool     <- Ekran aÃSection Ä±k mÄ±?
//
// DrmMode alanlarÄ± VESA/CEA standart zamanlamasÄ±nÄ± tanÄ±mlar:
//   hdisplay x vdisplay @ vrefresh Hz
//   Ã–rn: 1920 x 1080 @ 60 Hz  -> clock = 148500 kHz

pub struct DrmCrtc {
    pub id: u32,
    pub index: u32,
    /// GÃ¶rÃ¼ntÃ¼nÃ¼n framebuffer iÃSection indeki X ofseti (scissor/pan)
    pub x: u32,
    /// GÃ¶rÃ¼ntÃ¼nÃ¼n framebuffer iÃSection indeki Y ofseti
    pub y: u32,
    /// Åu an gÃ¶rÃ¼ntÃ¼lenen framebuffer ID'si (0 = yok)
    pub fb_id: AtomicU32,
    /// Aktif video modu (None = devre dÄ±ÅŸÄ±)
    pub mode: Mutex<Option<DrmMode>>,
    /// Gamma tablosunun boyutu (genellikle 256 giriÅŸ)
    pub gamma_size: u32,
    /// CRTC'nin aktif olup olmadÄ±ÄŸÄ±
    pub active: AtomicBool,
}

/// Video zamanlama modu; piksel saati ve tarama parametrelerini tanÄ±mlar.
/// DRM_IOCTL_MODE_GETCONNECTOR ile monitÃ¶rden EDID bilgisi alÄ±narak doldurulur.
#[derive(Clone, Debug)]
pub struct DrmMode {
    /// Piksel saati (kHz cinsinden, Ã¶rn. 148500 = 1080p60)
    pub clock: u32,
    pub hdisplay: u16,    // GÃ¶rÃ¼nÃ¼r yatay piksel sayÄ±sÄ±
    pub hsync_start: u16, // Yatay senkron baÅŸlangÄ±cÄ±
    pub hsync_end: u16,   // Yatay senkron bitiÅŸi
    pub htotal: u16,      // Toplam yatay sÃ¼re (gÃ¶rÃ¼nÃ¼r + blanking)
    pub hskew: u16,
    pub vdisplay: u16, // GÃ¶rÃ¼nÃ¼r dikey satÄ±r sayÄ±sÄ±
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    /// Dikey yenileme hÄ±zÄ± (Hz)
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    /// Mod adÄ± (Ã¶rn. "1920x1080") UTF-8, null-padded
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

    /// Video modunu ayarlar ve CRTC'yi aktif duruma getirir
    pub fn set_mode(&self, mode: DrmMode) {
        *self.mode.lock() = Some(mode);
        self.active.store(true, Ordering::SeqCst);
    }

    /// GÃ¶rÃ¼ntÃ¼lenen framebuffer'Ä± deÄŸiÅŸtirir (page flip Ã¶ncesi)
    pub fn set_fb(&self, fb_id: u32) {
        self.fb_id.store(fb_id, Ordering::SeqCst);
    }
}

// ============================================================================
// ENCODER (DRM ENCODER)
// ============================================================================

// CRTC'den gelen dijital piksel verisini fiziksel sinyal standardÄ±na dÃ¶nÃ¼ÅŸtÃ¼rÃ¼r:
//   DRM_MODE_ENCODER_TMDS  -> HDMI / DVI (Transition Minimized Differential Signaling)
//   DRM_MODE_ENCODER_LVDS  -> Dahili panel (laptop)
//   DRM_MODE_ENCODER_DAC   -> VGA analog
//   DRM_MODE_ENCODER_DSI   -> Mobil MIPI DSI
//
// possible_crtcs bitmask'i hangi CRTC'lerin bu encoder'Ä± kullanabileceÄŸini gÃ¶sterir.

pub struct DrmEncoder {
    pub id: u32,
    /// Encoder tÃ¼rÃ¼ (DRM_MODE_ENCODER_* sabitleri)
    pub encoder_type: u32,
    /// Åu an baÄŸlÄ± CRTC'nin ID'si
    pub crtc_id: AtomicU32,
    /// Bu encoder ile uyumlu CRTC'lerin bitmask'i
    pub possible_crtcs: u32,
    /// Klonlanabilir encoder bitmask'i (mirror mode iÃSection in)
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
// CONNECTOR (DRM CONNECTOR)
// ============================================================================

// Fiziksel ÃSection Ä±kÄ±ÅŸ baÄŸlantÄ± noktasÄ±: HDMI, DisplayPort, VGA, eDP, DVI-D...
// Connector durumu hotplug ile deÄŸiÅŸebilir:
//   Connected    -> monitÃ¶r baÄŸlÄ±, EDID okundu
//   Disconnected -> monitÃ¶r yok
//   Unknown      -> henÃ¼z algÄ±lanmadÄ±
//
// Connector->Encoder->CRTC zinciri:
//
//   [HDMI-A-1 connector] --baÄŸlÄ±--> [TMDS encoder] --baÄŸlÄ±--> [CRTC 0]

pub struct DrmConnector {
    pub id: u32,
    /// BaÄŸlayÄ±cÄ± tÃ¼rÃ¼ (DRM_MODE_CONNECTOR_HDMIA, VGA, DP, eDP...)
    pub connector_type: u32,
    /// AynÄ± tÃ¼rdeki sÄ±ra numarasÄ± (Ã¶rn. HDMI-A-1, HDMI-A-2)
    pub connector_type_id: u32,
    /// BaÄŸlÄ± encoder'Ä±n ID'si
    pub encoder_id: AtomicU32,
    /// BaÄŸlantÄ± durumu (hotplug olaylarÄ±yla gÃ¼ncellenir)
    pub connection: Mutex<DrmConnectorStatus>,
    /// MonitÃ¶rÃ¼n desteklediÄŸi video modlarÄ± (EDID'den okunur)
    pub modes: Mutex<Vec<DrmMode>>,
    /// MonitÃ¶rÃ¼n fiziksel geniÅŸliÄŸi (mm)
    pub width_mm: u32,
    /// MonitÃ¶rÃ¼n fiziksel yÃ¼ksekliÄŸi (mm)
    pub height_mm: u32,
    /// Alt piksel dÃ¼zeni (renk kalitesi iÃSection in)
    pub subpixel: u32,
}

/// Fiziksel baÄŸlantÄ± algÄ±lama durumu
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

    /// Desteklenen yeni video modu ekler (EDID ayrÄ±ÅŸtÄ±rmasÄ±ndan ÃSection aÄŸrÄ±lÄ±r)
    pub fn add_mode(&self, mode: DrmMode) {
        self.modes.lock().push(mode);
    }

    /// MonitÃ¶r baÄŸlantÄ± durumunu dÃ¶ner (hotplug polling iÃSection in)
    pub fn detect(&self) -> DrmConnectorStatus {
        *self.connection.lock()
    }

    /// EDID blob'unu ayrÄ±ÅŸtÄ±rÄ±r ve desteklenen modlarÄ± connector'a ekler.
    ///
    /// EDID 1.3 formatÄ± (VESA E-EDID Release A Rev. 2):
    /// - Byte 0-7: Header (00 FF FF FF FF FF FF 00)
    /// - Byte 8-9: Manufacturer ID (3 karakter, 5-bit packed)
    /// - Byte 10-11: Product code
    /// - Byte 12-15: Serial number
    /// - Byte 16: Hafta, Byte 17: YÄ±l (1990 + value)
    /// - Byte 18: EDID versiyon (3)
    /// - Byte 19: EDID revizyon
    /// - Byte 20-21: Video giriÅŸ tanÄ±mlarÄ±
    /// - Byte 22: Maksimum yatay boyut (cm)
    /// - Byte 23: Maksimum dikey boyut (cm)
    /// - Byte 24: Gamma
    /// - Byte 25: Ã–zellik bayraklarÄ±
    /// - Byte 26-37: Renk karakteristikleri
    /// - Byte 38: KurulmuÅŸ zamanlama
    /// - Byte 39-40: Standart zamanlama tanÄ±mlayÄ±cÄ±larÄ±
    /// - Byte 41-53: DetaylÄ± zamanlama tanÄ±mlayÄ±cÄ±larÄ± (DTD)
    ///   - Her DTD 18 byte: pixel clock, h/v aktif, blanking, sync
    /// - Byte 126: DTD sayÄ±sÄ±
    /// - Byte 127: Checksum
    pub fn parse_edid(&mut self, edid: &[u8; 128]) -> Result<(), &'static str> {
        if edid[0] != 0x00
            || edid[1] != 0xFF
            || edid[2] != 0xFF
            || edid[3] != 0xFF
            || edid[4] != 0xFF
            || edid[5] != 0xFF
            || edid[6] != 0xFF
            || edid[7] != 0x00
        {
            return Err("invalid EDID header");
        }
        let checksum: u8 = edid.iter().copied().fold(0u8, |a, b| a.wrapping_add(b));
        if checksum != 0 {
            return Err("EDID checksum mismatch");
        }
        self.width_mm = edid[21] as u32;
        self.height_mm = edid[22] as u32;
        let mut modes = self.modes.lock();
        for dtd_idx in 0..4usize {
            let offset = 54 + dtd_idx * 18;
            if edid[offset] == 0 && edid[offset + 1] == 0 {
                continue;
            }
            let pixel_clock_khz = ((edid[offset + 1] as u32) << 8 | edid[offset] as u32) * 10;
            let h_active = ((edid[offset + 4] as u16 & 0xF0) << 4) | edid[offset + 2] as u16;
            let h_blanking = ((edid[offset + 4] as u16 & 0x0F) << 8) | edid[offset + 3] as u16;
            let v_active = ((edid[offset + 7] as u16 & 0xF0) << 4) | edid[offset + 5] as u16;
            let v_blanking = ((edid[offset + 7] as u16 & 0x0F) << 8) | edid[offset + 6] as u16;
            let h_total = h_active + h_blanking;
            let v_total = v_active + v_blanking;
            let vrefresh = if h_total > 0 && v_total > 0 {
                (pixel_clock_khz as u64 * 1000) / (h_total as u64 * v_total as u64)
            } else {
                60
            };
            let hsync_start =
                h_active + (((edid[offset + 11] as u16 & 0xC0) << 2) | edid[offset + 8] as u16);
            let hsync_end =
                hsync_start + (((edid[offset + 11] as u16 & 0x30) << 4) | edid[offset + 9] as u16);
            let vsync_start = v_active
                + (((edid[offset + 11] as u16 & 0x0C) << 6) | (edid[offset + 10] as u16 >> 4));
            let vsync_end = vsync_start
                + (((edid[offset + 11] as u16 & 0x03) << 8) | (edid[offset + 10] as u16 & 0x0F));
            let mut name = [0u8; 32];
            let mode_str = alloc::format!("{}x{}@{}", h_active, v_active, vrefresh);
            for (i, b) in mode_str.bytes().enumerate() {
                if i < 32 {
                    name[i] = b;
                }
            }
            modes.push(DrmMode {
                clock: pixel_clock_khz,
                hdisplay: h_active,
                hsync_start,
                hsync_end,
                htotal: h_total,
                hskew: 0,
                vdisplay: v_active,
                vsync_start,
                vsync_end,
                vtotal: v_total,
                vscan: 0,
                vrefresh: vrefresh as u32,
                flags: 0,
                type_: 0,
                name,
            });
        }
        Ok(())
    }

    /// EDID ile hotplug mode probe: yeni EDID alÄ±nÄ±rsa modlarÄ± gÃ¼nceller.
    pub fn hotplug_mode_probe(&mut self, edid: &[u8; 128]) -> Result<(), &'static str> {
        *self.connection.lock() = DrmConnectorStatus::Connected;
        self.parse_edid(edid)
    }
}

// ============================================================================
// PLANE (DRM PLANE)
// ============================================================================

// Ekrana gÃ¶nderilecek gÃ¶rÃ¼ntÃ¼ katmanÄ±. ÃœÃSection  tÃ¼r plane vardÄ±r:
//   Primary   -> CRTC'nin ana framebuffer'Ä±
//   Overlay   -> DonanÄ±m video overlay (YUV video iÃSection in)
//   Cursor    -> DonanÄ±m imleÃSection  katmanÄ± (16x16 veya 64x64 piksel)
//
// Her plane baÄŸÄ±msÄ±z kaynak (src_x/y/w/h) ve hedef (crtc_x/y/w/h) dikdÃ¶rtgenleri
// tanÄ±mlar; donanÄ±m Ã¶lÃSection ekleme desteÄŸiyle farklÄ± boyutlarda gÃ¶sterilebilir.

pub struct DrmPlane {
    pub id: u32,
    pub plane_type: DrmPlaneType,
    /// Bu plane'in kullanabileceÄŸi CRTC'lerin bitmask'i
    pub possible_crtcs: u32,
    /// Desteklenen piksel formatlarÄ±nÄ±n sayÄ±sÄ±
    pub format_count: u32,
    /// FourCC piksel formatlarÄ± (DRM_FORMAT_*)
    pub formats: Vec<u32>,
    /// BaÄŸlÄ± CRTC ID'si
    pub crtc_id: AtomicU32,
    /// GÃ¶rÃ¼ntÃ¼lenen framebuffer ID'si
    pub fb_id: AtomicU32,
    /// Hedef CRTC alanÄ± (ekran koordinatlarÄ±nda)
    pub crtc_x: AtomicU32,
    pub crtc_y: AtomicU32,
    pub crtc_w: AtomicU32,
    pub crtc_h: AtomicU32,
    /// Kaynak framebuffer alanÄ± (16.16 sabit nokta formatÄ±)
    pub src_x: AtomicU32,
    pub src_y: AtomicU32,
    pub src_w: AtomicU32,
    pub src_h: AtomicU32,
}

impl DrmPlane {
    pub fn new(id: u32) -> Self {
        Self::new_with_type(id, DrmPlaneType::Overlay)
    }

    pub fn new_with_type(id: u32, plane_type: DrmPlaneType) -> Self {
        Self {
            id,
            plane_type,
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
// DRM YÃ–NETÄ°CÄ°SÄ° (DRM MANAGER)
// ============================================================================

// Sistemdeki tÃ¼m DRM cihazlarÄ±nÄ±n (GPU'larÄ±n) kayÄ±tlarÄ±nÄ± tutar.
// Linux'ta /dev/dri/card0, /dev/dri/card1, ... olarak gÃ¶rÃ¼nÃ¼r.

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

    pub fn first_device(&self) -> Option<Arc<DrmDevice>> {
        self.devices.lock().values().next().cloned()
    }
}

lazy_static::lazy_static! {
    pub static ref DRM_MANAGER: DrmManager = DrmManager::new();
}

// ============================================================================
// BAÅLATMA (INITIALIZATION)
// ============================================================================

pub fn init() {
    if DRM_MANAGER.first_device().is_some() {
        return;
    }

    // Birincil GPU cihazÄ±nÄ± (card0) sisteme kaydet
    let gpu = DRM_MANAGER.register_device("card0");

    // VarsayÄ±lan CRTC ekle (ekran denetleyicisi; tek ekran iÃSection in yeterli)
    let crtc = Arc::new(DrmCrtc::new(0, 0));
    gpu.add_crtc(crtc);

    // VarsayÄ±lan connector ekle (VGA tipi; gerÃSection ekte EDID algÄ±lama yapÄ±lÄ±r)
    let connector = Arc::new(DrmConnector::new(0, 0)); // VGA
    *connector.connection.lock() = DrmConnectorStatus::Connected;
    gpu.add_connector(connector);

    gpu.add_plane(Arc::new(DrmPlane::new_with_type(0, DrmPlaneType::Primary)));
    gpu.add_plane(Arc::new(DrmPlane::new_with_type(1, DrmPlaneType::Overlay)));
    gpu.add_plane(Arc::new(DrmPlane::new_with_type(2, DrmPlaneType::Cursor)));

    crate::serial_println!("[DRM] DRM/KMS initialized with atomic plane topology");
}

// ============================================================================
// Test Corpus (DRM/KMS + dma-buf host contracts)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use alloc::vec;
    use core::sync::atomic::Ordering;

    const TEST_FORMAT_XRGB8888: u32 = 0x3432_5258;

    fn mode_1920x1080_60hz() -> DrmMode {
        let mut name = [0u8; 32];
        name[..9].copy_from_slice(b"1920x1080");
        DrmMode {
            clock: 148500,
            hdisplay: 1920,
            hsync_start: 2008,
            hsync_end: 2052,
            htotal: 2200,
            hskew: 0,
            vdisplay: 1080,
            vsync_start: 1084,
            vsync_end: 1089,
            vtotal: 1125,
            vscan: 0,
            vrefresh: 60,
            flags: 0,
            type_: 0,
            name,
        }
    }

    fn mk_device() -> DrmDevice {
        let device = DrmDevice::new(1, "card-test");
        let crtc = Arc::new(DrmCrtc::new(1, 0));
        let connector = Arc::new(DrmConnector::new(1, 1));
        *connector.connection.lock() = DrmConnectorStatus::Connected;
        connector.add_mode(mode_1920x1080_60hz());
        device.add_crtc(crtc);
        device.add_connector(connector);
        device.add_plane(Arc::new(DrmPlane::new_with_type(1, DrmPlaneType::Primary)));
        device
    }

    #[test]
    fn drm_mode_1920x1080_60hz_contract() {
        let mode = mode_1920x1080_60hz();
        assert_eq!(mode.hdisplay, 1920);
        assert_eq!(mode.vdisplay, 1080);
        assert_eq!(mode.vrefresh, 60);
        assert_eq!(&mode.name[..9], b"1920x1080");
    }

    #[test]
    fn drm_connector_creation() {
        let connector = DrmConnector::new(7, 1);
        assert_eq!(connector.id, 7);
        assert_eq!(connector.connector_type, 1);
        assert_eq!(connector.connector_type_id, 1);
        assert_eq!(*connector.connection.lock(), DrmConnectorStatus::Unknown);
    }

    #[test]
    fn drm_framebuffer_creation() {
        let fb = DrmFramebuffer::new(3, 1920, 1080, TEST_FORMAT_XRGB8888, [9, 0, 0, 0]);
        assert_eq!(fb.id, 3);
        assert_eq!(fb.width, 1920);
        assert_eq!(fb.height, 1080);
        assert_eq!(fb.format, TEST_FORMAT_XRGB8888);
        assert_eq!(fb.handles[0], 9);
        assert_eq!(fb.pitches[0], 7680);
    }

    #[test]
    fn drm_device_registration() {
        let gpu = mk_device();
        assert_eq!(gpu.name, "card-test");
        assert_eq!(gpu.crtcs.lock().len(), 1);
        assert_eq!(gpu.connectors.lock().len(), 1);
        assert_eq!(gpu.planes.lock().len(), 1);
    }

    #[test]
    fn drm_plane_type_enum_matches_current_order() {
        assert_eq!(DrmPlaneType::Primary as u8, 0);
        assert_eq!(DrmPlaneType::Overlay as u8, 1);
        assert_eq!(DrmPlaneType::Cursor as u8, 2);
    }

    #[test]
    fn atomic_transaction_commits_and_vblank_completes() {
        let gpu = mk_device();
        let txn = AtomicKmsTransaction {
            frame_id: 42,
            commit_id: 11,
            crtc_id: 1,
            connector_id: 1,
            mode: Some(mode_1920x1080_60hz()),
            planes: vec![PlaneCandidate {
                surface_id: 7,
                plane_type: DrmPlaneType::Primary,
                z: 0,
                src: Rect::new(0, 0, 64, 64),
                dst: Rect::new(0, 0, 64, 64),
                opaque: true,
                format: TEST_FORMAT_XRGB8888,
                buffer: GPUBufferHandle {
                    handle: 77,
                    paddr: 0x1000,
                    width: 64,
                    height: 64,
                    stride: 256,
                    format: TEST_FORMAT_XRGB8888,
                },
            }],
            damage_regions: Vec::new(),
            target_refresh_hz: 60,
            present_mode: DisplayPresentMode::VblankFifo,
        };

        let result = gpu.commit_transaction(&txn).expect("atomic commit");
        assert_eq!(result.frame_id, 42);
        assert_eq!(gpu.last_commit_id.load(Ordering::Acquire), 11);

        let seq = result.vblank_seq;
        assert!(gpu.report_flip_complete(42, 11, seq, 16_666_667));
        assert_eq!(gpu.last_presented_frame_id.load(Ordering::Acquire), 42);
        assert_eq!(gpu.inflight_plane_handles.lock().len(), 0);
    }
}
