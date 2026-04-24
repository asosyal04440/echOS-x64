//! Allocation doctrine for echOS.
//!
//! This module is intentionally small. It owns policy routing, telemetry, and
//! exception governance for allocation classes. It must not become a subsystem
//! convenience layer.

use super::stack::KernelStack;
use crate::memory::{dma_alloc, dma_dealloc, PAGE_SIZE};
use alloc::alloc::{alloc, alloc_zeroed, dealloc};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

const SMALL_OBJECT_LIMIT_BYTES: usize = 512;
const SMALL_OBJECT_ALIGN_LIMIT: usize = 16;
const LARGE_LINEAR_THRESHOLD_BYTES: usize = 128 * 1024;
const GUI_SURFACE_THRESHOLD_BYTES: usize = 512 * 1024;
const SNAPSHOT_CLONE_INLINE_LIMIT_BYTES: usize = 64 * 1024;
const SNAPSHOT_CLONE_EXCEPTION_LIMIT_BYTES: usize = 512 * 1024;
const MAX_EXCEPTIONS_PER_SUBSYSTEM: usize = 3;
const MAX_SNAPSHOT_EXCEPTIONS_PER_SUBSYSTEM: usize = 1;

const REGISTRY_UNVALIDATED: u8 = 0;
const REGISTRY_VALID: u8 = 1;
const REGISTRY_INVALID: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationClass {
    BootPermanent,
    SmallKernelObject,
    GeneralHeapBuffer,
    LargeLinearBuffer,
    PhysContigDma,
    KernelStackLike,
    SnapshotClone,
}

impl AllocationClass {
    const fn index(self) -> usize {
        match self {
            Self::BootPermanent => 0,
            Self::SmallKernelObject => 1,
            Self::GeneralHeapBuffer => 2,
            Self::LargeLinearBuffer => 3,
            Self::PhysContigDma => 4,
            Self::KernelStackLike => 5,
            Self::SnapshotClone => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationExceptionCategory {
    Bounded,
    ColdPath,
    DebugOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfacePixelFormat {
    Argb8888,
}

impl SurfacePixelFormat {
    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Argb8888 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaRole {
    Generic,
    GuiSurface,
    Renderer,
    Storage,
    Network,
    Loopback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationThresholds {
    pub small_object_limit_bytes: usize,
    pub small_object_align_limit: usize,
    pub large_linear_threshold_bytes: usize,
    pub gui_surface_threshold_bytes: usize,
    pub snapshot_clone_inline_limit_bytes: usize,
    pub snapshot_clone_exception_limit_bytes: usize,
}

pub const fn thresholds() -> AllocationThresholds {
    AllocationThresholds {
        small_object_limit_bytes: SMALL_OBJECT_LIMIT_BYTES,
        small_object_align_limit: SMALL_OBJECT_ALIGN_LIMIT,
        large_linear_threshold_bytes: LARGE_LINEAR_THRESHOLD_BYTES,
        gui_surface_threshold_bytes: GUI_SURFACE_THRESHOLD_BYTES,
        snapshot_clone_inline_limit_bytes: SNAPSHOT_CLONE_INLINE_LIMIT_BYTES,
        snapshot_clone_exception_limit_bytes: SNAPSHOT_CLONE_EXCEPTION_LIMIT_BYTES,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationException {
    pub subsystem: &'static str,
    pub function_or_path: &'static str,
    pub allocation_class: AllocationClass,
    pub reason: &'static str,
    pub category: AllocationExceptionCategory,
    pub max_bytes: usize,
    pub review_owner: &'static str,
    pub removal_condition: &'static str,
}

const ALLOCATION_EXCEPTIONS: &[AllocationException] = &[
    AllocationException {
        subsystem: "gfx",
        function_or_path: "gfx.canvas.into_vec",
        allocation_class: AllocationClass::SnapshotClone,
        reason: "bounded shell raster overlay surfaces still emit Vec<u32> for commit and tests",
        category: AllocationExceptionCategory::Bounded,
        max_bytes: 1024 * 1024,
        review_owner: "gui",
        removal_condition: "remove after shell overlay helpers commit directly from borrowed slices or retained scenes",
    },
    AllocationException {
        subsystem: "surface",
        function_or_path: "gui.shared_surface.snapshot",
        allocation_class: AllocationClass::SnapshotClone,
        reason: "debug and export snapshot consumers still require owned pixel vectors",
        category: AllocationExceptionCategory::DebugOnly,
        max_bytes: 8 * 1024 * 1024,
        review_owner: "display",
        removal_condition: "remove after snapshot/export users switch to borrowed traversal or streaming capture",
    },
    AllocationException {
        subsystem: "text",
        function_or_path: "gui.text.blob_pixels",
        allocation_class: AllocationClass::SnapshotClone,
        reason: "glyph blobs still cross the render protocol as owned pixel vectors",
        category: AllocationExceptionCategory::Bounded,
        max_bytes: 2 * 1024 * 1024,
        review_owner: "text",
        removal_condition: "remove after glyph protocol carries borrowed or shared pixel backing instead of Vec<u32>",
    },
    AllocationException {
        subsystem: "gop",
        function_or_path: "gop.framebuffer.clone",
        allocation_class: AllocationClass::SnapshotClone,
        reason: "global framebuffer getters still require an owned clone of the shadow buffer",
        category: AllocationExceptionCategory::ColdPath,
        max_bytes: 16 * 1024 * 1024,
        review_owner: "boot",
        removal_condition: "remove after global framebuffer access switches to borrowed or closure-based access",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctrineError {
    OutOfMemory,
    PolicyViolation(&'static str),
    ExceptionDenied(&'static str),
    InvalidRegistry(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationTelemetrySnapshot {
    pub slab_alloc_count: u64,
    pub tlsf_alloc_count: u64,
    pub page_backed_alloc_count: u64,
    pub dma_alloc_count: u64,
    pub large_heap_violation_count: u64,
    pub snapshot_clone_count: u64,
    pub snapshot_clone_bytes_total: u64,
    pub snapshot_clone_max_bytes: u64,
    pub exception_hit_count: u64,
    pub failure_counts: [u64; 7],
}

struct AllocationTelemetry {
    slab_alloc_count: AtomicU64,
    tlsf_alloc_count: AtomicU64,
    page_backed_alloc_count: AtomicU64,
    dma_alloc_count: AtomicU64,
    large_heap_violation_count: AtomicU64,
    snapshot_clone_count: AtomicU64,
    snapshot_clone_bytes_total: AtomicU64,
    snapshot_clone_max_bytes: AtomicU64,
    exception_hit_count: AtomicU64,
    failure_counts: [AtomicU64; 7],
}

impl AllocationTelemetry {
    const fn new() -> Self {
        Self {
            slab_alloc_count: AtomicU64::new(0),
            tlsf_alloc_count: AtomicU64::new(0),
            page_backed_alloc_count: AtomicU64::new(0),
            dma_alloc_count: AtomicU64::new(0),
            large_heap_violation_count: AtomicU64::new(0),
            snapshot_clone_count: AtomicU64::new(0),
            snapshot_clone_bytes_total: AtomicU64::new(0),
            snapshot_clone_max_bytes: AtomicU64::new(0),
            exception_hit_count: AtomicU64::new(0),
            failure_counts: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    fn snapshot(&self) -> AllocationTelemetrySnapshot {
        AllocationTelemetrySnapshot {
            slab_alloc_count: self.slab_alloc_count.load(Ordering::Relaxed),
            tlsf_alloc_count: self.tlsf_alloc_count.load(Ordering::Relaxed),
            page_backed_alloc_count: self.page_backed_alloc_count.load(Ordering::Relaxed),
            dma_alloc_count: self.dma_alloc_count.load(Ordering::Relaxed),
            large_heap_violation_count: self.large_heap_violation_count.load(Ordering::Relaxed),
            snapshot_clone_count: self.snapshot_clone_count.load(Ordering::Relaxed),
            snapshot_clone_bytes_total: self.snapshot_clone_bytes_total.load(Ordering::Relaxed),
            snapshot_clone_max_bytes: self.snapshot_clone_max_bytes.load(Ordering::Relaxed),
            exception_hit_count: self.exception_hit_count.load(Ordering::Relaxed),
            failure_counts: [
                self.failure_counts[0].load(Ordering::Relaxed),
                self.failure_counts[1].load(Ordering::Relaxed),
                self.failure_counts[2].load(Ordering::Relaxed),
                self.failure_counts[3].load(Ordering::Relaxed),
                self.failure_counts[4].load(Ordering::Relaxed),
                self.failure_counts[5].load(Ordering::Relaxed),
                self.failure_counts[6].load(Ordering::Relaxed),
            ],
        }
    }
}

static TELEMETRY: AllocationTelemetry = AllocationTelemetry::new();
static REGISTRY_STATE: AtomicU8 = AtomicU8::new(REGISTRY_UNVALIDATED);

fn ensure_registry_validated() -> Result<(), DoctrineError> {
    match REGISTRY_STATE.load(Ordering::Acquire) {
        REGISTRY_VALID => return Ok(()),
        REGISTRY_INVALID => {
            return Err(DoctrineError::InvalidRegistry(
                "allocation exception registry is invalid",
            ))
        }
        _ => {}
    }

    validate_exception_registry_entries(ALLOCATION_EXCEPTIONS)?;
    REGISTRY_STATE.store(REGISTRY_VALID, Ordering::Release);
    Ok(())
}

fn validate_exception_registry_entries(
    entries: &[AllocationException],
) -> Result<(), DoctrineError> {
    for entry in entries.iter() {
        if entry.subsystem.is_empty()
            || entry.function_or_path.is_empty()
            || entry.reason.is_empty()
            || entry.review_owner.is_empty()
            || entry.removal_condition.is_empty()
            || entry.max_bytes == 0
        {
            return Err(DoctrineError::InvalidRegistry(
                "allocation exception entry contains empty required fields",
            ));
        }
    }

    for (index, entry) in entries.iter().enumerate() {
        if entries[..index]
            .iter()
            .any(|candidate| candidate.subsystem == entry.subsystem)
        {
            continue;
        }
        let mut subsystem_total = 0usize;
        let mut subsystem_snapshot = 0usize;
        for candidate in entries.iter() {
            if candidate.subsystem == entry.subsystem {
                if candidate.category != AllocationExceptionCategory::DebugOnly {
                    subsystem_total += 1;
                }
                if candidate.allocation_class == AllocationClass::SnapshotClone {
                    subsystem_snapshot += 1;
                }
            }
        }
        if subsystem_total > MAX_EXCEPTIONS_PER_SUBSYSTEM {
            return Err(DoctrineError::InvalidRegistry(
                "allocation exception quota exceeded for subsystem",
            ));
        }
        if subsystem_snapshot > MAX_SNAPSHOT_EXCEPTIONS_PER_SUBSYSTEM {
            return Err(DoctrineError::InvalidRegistry(
                "snapshot clone exception quota exceeded for subsystem",
            ));
        }
    }

    Ok(())
}

fn doctrine_violation(message: &'static str) -> DoctrineError {
    crate::serial_println!("[ALLOC_DOCTRINE] violation: {}", message);
    DoctrineError::PolicyViolation(message)
}

fn record_failure(class: AllocationClass) {
    TELEMETRY.failure_counts[class.index()].fetch_add(1, Ordering::Relaxed);
}

fn record_snapshot_clone(bytes: usize, used_exception: bool) {
    TELEMETRY
        .snapshot_clone_count
        .fetch_add(1, Ordering::Relaxed);
    TELEMETRY
        .snapshot_clone_bytes_total
        .fetch_add(bytes as u64, Ordering::Relaxed);
    if used_exception {
        TELEMETRY
            .exception_hit_count
            .fetch_add(1, Ordering::Relaxed);
    }
    let bytes = bytes as u64;
    let mut current = TELEMETRY.snapshot_clone_max_bytes.load(Ordering::Relaxed);
    while bytes > current {
        match TELEMETRY.snapshot_clone_max_bytes.compare_exchange_weak(
            current,
            bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn clone_slice_u32(slice: &[u32], reason_tag: &'static str) -> Result<Vec<u32>, DoctrineError> {
    let bytes = slice.len().saturating_mul(core::mem::size_of::<u32>());
    let _ = alloc_snapshot_clone(bytes, reason_tag)?;
    let mut cloned = Vec::new();
    cloned.try_reserve_exact(slice.len()).map_err(|_| {
        record_failure(AllocationClass::SnapshotClone);
        DoctrineError::OutOfMemory
    })?;
    cloned.extend_from_slice(slice);
    Ok(cloned)
}

fn snapshot_exception(
    reason_tag: &'static str,
    bytes: usize,
) -> Option<&'static AllocationException> {
    ALLOCATION_EXCEPTIONS.iter().find(|entry| {
        entry.allocation_class == AllocationClass::SnapshotClone
            && entry.function_or_path == reason_tag
            && bytes <= entry.max_bytes
            && (!matches!(entry.category, AllocationExceptionCategory::DebugOnly)
                || cfg!(debug_assertions))
    })
}

pub fn alloc_small_object<T>(value: T) -> Result<Box<T>, DoctrineError> {
    ensure_registry_validated()?;
    let layout = Layout::new::<T>();
    if layout.size() > SMALL_OBJECT_LIMIT_BYTES || layout.align() > SMALL_OBJECT_ALIGN_LIMIT {
        TELEMETRY
            .large_heap_violation_count
            .fetch_add(1, Ordering::Relaxed);
        return Err(doctrine_violation(
            "alloc_small_object called for object outside slab bounds",
        ));
    }

    let ptr = unsafe { alloc(layout) as *mut T };
    if ptr.is_null() {
        record_failure(AllocationClass::SmallKernelObject);
        return Err(DoctrineError::OutOfMemory);
    }
    unsafe {
        ptr::write(ptr, value);
        TELEMETRY.slab_alloc_count.fetch_add(1, Ordering::Relaxed);
        Ok(Box::from_raw(ptr))
    }
}

pub fn alloc_heap_buffer(
    bytes: usize,
    class_hint: AllocationClass,
) -> Result<Vec<u8>, DoctrineError> {
    ensure_registry_validated()?;
    match class_hint {
        AllocationClass::SmallKernelObject | AllocationClass::GeneralHeapBuffer => {}
        _ => {
            return Err(doctrine_violation(
                "alloc_heap_buffer used with non-heap allocation class",
            ))
        }
    }

    if bytes >= LARGE_LINEAR_THRESHOLD_BYTES {
        TELEMETRY
            .large_heap_violation_count
            .fetch_add(1, Ordering::Relaxed);
        return Err(doctrine_violation(
            "alloc_heap_buffer used for large linear allocation",
        ));
    }

    let mut buffer = Vec::new();
    buffer.try_reserve_exact(bytes).map_err(|_| {
        record_failure(class_hint);
        DoctrineError::OutOfMemory
    })?;
    buffer.resize(bytes, 0);
    if bytes <= SMALL_OBJECT_LIMIT_BYTES {
        TELEMETRY.slab_alloc_count.fetch_add(1, Ordering::Relaxed);
    } else {
        TELEMETRY.tlsf_alloc_count.fetch_add(1, Ordering::Relaxed);
    }
    Ok(buffer)
}

#[derive(Debug)]
pub struct PageBackedAllocation<T> {
    ptr: NonNull<T>,
    len: usize,
    layout: Layout,
}

impl<T> PageBackedAllocation<T> {
    fn new_zeroed(elements: usize) -> Result<Self, DoctrineError> {
        let bytes = elements
            .checked_mul(core::mem::size_of::<T>())
            .ok_or(DoctrineError::OutOfMemory)?;
        let size = bytes.max(1);
        let align = 64usize.max(core::mem::align_of::<T>());
        let layout = Layout::from_size_align(size, align).map_err(|_| {
            record_failure(AllocationClass::LargeLinearBuffer);
            DoctrineError::PolicyViolation("page-backed allocation layout is invalid")
        })?;
        let ptr = unsafe { alloc_zeroed(layout) as *mut T };
        let ptr = NonNull::new(ptr).ok_or_else(|| {
            record_failure(AllocationClass::LargeLinearBuffer);
            DoctrineError::OutOfMemory
        })?;
        Ok(Self {
            ptr,
            len: elements,
            layout,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl<T> Drop for PageBackedAllocation<T> {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.ptr.as_ptr() as *mut u8, self.layout);
        }
    }
}

unsafe impl<T: Send> Send for PageBackedAllocation<T> {}
unsafe impl<T: Sync> Sync for PageBackedAllocation<T> {}

pub fn alloc_large_pages(
    bytes: usize,
    class_hint: AllocationClass,
) -> Result<PageBackedAllocation<u8>, DoctrineError> {
    ensure_registry_validated()?;
    match class_hint {
        AllocationClass::LargeLinearBuffer | AllocationClass::KernelStackLike => {}
        _ => {
            return Err(doctrine_violation(
                "alloc_large_pages used with non page-backed allocation class",
            ))
        }
    }
    let allocation = PageBackedAllocation::<u8>::new_zeroed(bytes)?;
    TELEMETRY
        .page_backed_alloc_count
        .fetch_add(1, Ordering::Relaxed);
    Ok(allocation)
}

#[derive(Debug)]
pub enum SurfacePixelBuffer {
    Heap(Vec<u32>),
    PageBacked(PageBackedAllocation<u32>),
}

impl SurfacePixelBuffer {
    pub fn new(
        width: usize,
        height: usize,
        format: SurfacePixelFormat,
        background: u32,
    ) -> Result<Self, DoctrineError> {
        ensure_registry_validated()?;
        let len = width.saturating_mul(height);
        let bytes = len.saturating_mul(format.bytes_per_pixel());
        if bytes >= GUI_SURFACE_THRESHOLD_BYTES {
            let mut buffer = PageBackedAllocation::<u32>::new_zeroed(len)?;
            buffer.as_mut_slice().fill(background);
            TELEMETRY
                .page_backed_alloc_count
                .fetch_add(1, Ordering::Relaxed);
            return Ok(Self::PageBacked(buffer));
        }

        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| {
            record_failure(AllocationClass::GeneralHeapBuffer);
            DoctrineError::OutOfMemory
        })?;
        pixels.resize(len, background);
        TELEMETRY.tlsf_alloc_count.fetch_add(1, Ordering::Relaxed);
        Ok(Self::Heap(pixels))
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Heap(pixels) => pixels.len(),
            Self::PageBacked(pixels) => pixels.len(),
        }
    }

    pub fn as_slice(&self) -> &[u32] {
        match self {
            Self::Heap(pixels) => pixels.as_slice(),
            Self::PageBacked(pixels) => pixels.as_slice(),
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        match self {
            Self::Heap(pixels) => pixels.as_mut_slice(),
            Self::PageBacked(pixels) => pixels.as_mut_slice(),
        }
    }

    pub fn as_ptr(&self) -> *const u32 {
        self.as_slice().as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u32 {
        self.as_mut_slice().as_mut_ptr()
    }

    pub fn copy_from_slice(&mut self, pixels: &[u32]) {
        self.as_mut_slice().copy_from_slice(pixels);
    }

    pub fn resize_zeroed(&mut self, new_len: usize) -> Result<(), DoctrineError> {
        if self.len() == new_len {
            return Ok(());
        }
        let mut replacement =
            if new_len.saturating_mul(core::mem::size_of::<u32>()) >= GUI_SURFACE_THRESHOLD_BYTES {
                let buffer = PageBackedAllocation::<u32>::new_zeroed(new_len)?;
                TELEMETRY
                    .page_backed_alloc_count
                    .fetch_add(1, Ordering::Relaxed);
                Self::PageBacked(buffer)
            } else {
                let mut pixels = Vec::new();
                pixels.try_reserve_exact(new_len).map_err(|_| {
                    record_failure(AllocationClass::GeneralHeapBuffer);
                    DoctrineError::OutOfMemory
                })?;
                pixels.resize(new_len, 0);
                TELEMETRY.tlsf_alloc_count.fetch_add(1, Ordering::Relaxed);
                Self::Heap(pixels)
            };
        let copy_len = self.len().min(new_len);
        replacement.as_mut_slice()[..copy_len].copy_from_slice(&self.as_slice()[..copy_len]);
        *self = replacement;
        Ok(())
    }

    pub fn snapshot_vec(&self, reason_tag: &'static str) -> Result<Vec<u32>, DoctrineError> {
        clone_slice_u32(self.as_slice(), reason_tag)
    }

    pub fn into_vec(self, reason_tag: &'static str) -> Result<Vec<u32>, DoctrineError> {
        match self {
            Self::Heap(pixels) => Ok(pixels),
            Self::PageBacked(pixels) => clone_slice_u32(pixels.as_slice(), reason_tag),
        }
    }
}

pub fn alloc_surface_pixels(
    width: usize,
    height: usize,
    format: SurfacePixelFormat,
) -> Result<SurfacePixelBuffer, DoctrineError> {
    SurfacePixelBuffer::new(width, height, format, 0)
}

pub fn alloc_dma_buffer(
    pages: usize,
    _dma_role: DmaRole,
) -> Result<(usize, NonNull<u8>), DoctrineError> {
    ensure_registry_validated()?;
    let allocation = dma_alloc(pages).ok_or_else(|| {
        record_failure(AllocationClass::PhysContigDma);
        DoctrineError::OutOfMemory
    })?;
    TELEMETRY.dma_alloc_count.fetch_add(1, Ordering::Relaxed);
    Ok(allocation)
}

pub fn alloc_kernel_stack(pages: usize) -> Result<KernelStack, DoctrineError> {
    ensure_registry_validated()?;
    let stack = KernelStack::new(pages.saturating_mul(PAGE_SIZE)).ok_or_else(|| {
        record_failure(AllocationClass::KernelStackLike);
        DoctrineError::OutOfMemory
    })?;
    TELEMETRY
        .page_backed_alloc_count
        .fetch_add(1, Ordering::Relaxed);
    Ok(stack)
}

pub fn alloc_snapshot_clone(bytes: usize, reason_tag: &'static str) -> Result<(), DoctrineError> {
    ensure_registry_validated()?;
    if bytes <= SNAPSHOT_CLONE_INLINE_LIMIT_BYTES {
        record_snapshot_clone(bytes, false);
        return Ok(());
    }

    let Some(exception) = snapshot_exception(reason_tag, bytes) else {
        return Err(doctrine_violation(
            "snapshot clone exceeded inline budget without allowlisted exception",
        ));
    };
    if bytes > SNAPSHOT_CLONE_EXCEPTION_LIMIT_BYTES && bytes > exception.max_bytes {
        return Err(doctrine_violation(
            "snapshot clone exceeded allowlisted exception budget",
        ));
    }
    record_snapshot_clone(bytes, true);
    Ok(())
}

pub fn allocation_telemetry_snapshot() -> AllocationTelemetrySnapshot {
    TELEMETRY.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(entries: &[AllocationException]) -> Result<(), DoctrineError> {
        validate_exception_registry_entries(entries)
    }

    #[test]
    fn small_object_wrapper_rejects_large_payloads() {
        let result = alloc_small_object([0u8; SMALL_OBJECT_LIMIT_BYTES + 1]);
        assert!(matches!(result, Err(DoctrineError::PolicyViolation(_))));
    }

    #[test]
    fn surface_pixels_route_large_buffers_page_backed() {
        let pixels =
            alloc_surface_pixels(512, 256, SurfacePixelFormat::Argb8888).expect("surface pixels");
        assert!(matches!(pixels, SurfacePixelBuffer::PageBacked(_)));
    }

    #[test]
    fn snapshot_clone_denies_unreviewed_mid_size_copy() {
        let result = alloc_snapshot_clone(128 * 1024, "gui.unreviewed.snapshot");
        assert!(matches!(result, Err(DoctrineError::PolicyViolation(_))));
    }

    #[test]
    fn snapshot_clone_accepts_reviewed_exception() {
        alloc_snapshot_clone(768 * 1024, "gfx.canvas.into_vec")
            .expect("exception-backed snapshot clone");
    }

    #[test]
    fn exception_registry_rejects_missing_owner() {
        let invalid = [AllocationException {
            subsystem: "gfx",
            function_or_path: "gfx.invalid",
            allocation_class: AllocationClass::SnapshotClone,
            reason: "invalid",
            category: AllocationExceptionCategory::Bounded,
            max_bytes: 1,
            review_owner: "",
            removal_condition: "never",
        }];
        assert!(matches!(
            validate(&invalid),
            Err(DoctrineError::InvalidRegistry(_))
        ));
    }

    #[test]
    fn exception_registry_rejects_quota_violation() {
        let invalid = [
            AllocationException {
                subsystem: "gfx",
                function_or_path: "gfx.one",
                allocation_class: AllocationClass::GeneralHeapBuffer,
                reason: "one",
                category: AllocationExceptionCategory::Bounded,
                max_bytes: 1,
                review_owner: "gui",
                removal_condition: "remove one",
            },
            AllocationException {
                subsystem: "gfx",
                function_or_path: "gfx.two",
                allocation_class: AllocationClass::GeneralHeapBuffer,
                reason: "two",
                category: AllocationExceptionCategory::Bounded,
                max_bytes: 1,
                review_owner: "gui",
                removal_condition: "remove two",
            },
            AllocationException {
                subsystem: "gfx",
                function_or_path: "gfx.three",
                allocation_class: AllocationClass::GeneralHeapBuffer,
                reason: "three",
                category: AllocationExceptionCategory::Bounded,
                max_bytes: 1,
                review_owner: "gui",
                removal_condition: "remove three",
            },
            AllocationException {
                subsystem: "gfx",
                function_or_path: "gfx.four",
                allocation_class: AllocationClass::GeneralHeapBuffer,
                reason: "four",
                category: AllocationExceptionCategory::Bounded,
                max_bytes: 1,
                review_owner: "gui",
                removal_condition: "remove four",
            },
        ];
        assert!(matches!(
            validate(&invalid),
            Err(DoctrineError::InvalidRegistry(_))
        ));
    }
}
