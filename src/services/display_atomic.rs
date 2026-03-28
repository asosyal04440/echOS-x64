use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::cpu::tsc;
use crate::drivers::drm::{
    AtomicKmsTransaction, DamageRegion, DrmDevice, DrmPlaneType, GPUBufferHandle, PlaneCandidate,
    VBlankEvent, DRM_MANAGER,
};
use crate::gui::protocol::{
    CompositorPass, DamageTile, DisplayPresentMode, FrameIntent, PlaneAssignment, Point, Rect,
    ScanoutCandidate, SurfaceId, SurfaceTransform, VblankFeedback,
};

const PRESENT_QUEUE_CAPACITY: usize = 64;
const VBLANK_QUEUE_CAPACITY: usize = 128;
const DAMAGE_TILE_SIZE: i32 = 64;

#[derive(Clone, Copy, Debug)]
pub struct SurfacePlacement {
    pub surface_id: SurfaceId,
    pub rect: Rect,
    pub z_index: u32,
    pub opaque: bool,
}

#[repr(align(64))]
struct CacheLineAtomicUsize {
    value: AtomicUsize,
}

impl CacheLineAtomicUsize {
    const fn new(value: usize) -> Self {
        Self {
            value: AtomicUsize::new(value),
        }
    }
}

pub struct MailboxRing<T> {
    head: CacheLineAtomicUsize,
    tail: CacheLineAtomicUsize,
    mask: usize,
    slots: Vec<UnsafeCell<MaybeUninit<T>>>,
}

unsafe impl<T: Send> Send for MailboxRing<T> {}
unsafe impl<T: Send> Sync for MailboxRing<T> {}

impl<T> MailboxRing<T> {
    pub fn with_capacity_pow2(capacity: usize) -> Self {
        let cap = capacity.max(2).next_power_of_two();
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(UnsafeCell::new(MaybeUninit::uninit()));
        }
        Self {
            head: CacheLineAtomicUsize::new(0),
            tail: CacheLineAtomicUsize::new(0),
            mask: cap - 1,
            slots,
        }
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    #[inline(always)]
    fn slot_ptr(&self, idx: usize) -> *mut T {
        self.slots[idx & self.mask].get().cast::<T>()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.value.load(Ordering::Acquire) == self.tail.value.load(Ordering::Acquire)
    }

    pub fn push_overwrite(&self, value: T) -> bool {
        let mut value = MaybeUninit::new(value);
        let mut dropped = false;

        loop {
            let head = self.head.value.load(Ordering::Acquire);
            let tail = self.tail.value.load(Ordering::Relaxed);
            if tail.wrapping_sub(head) >= self.capacity() {
                if self
                    .head
                    .value
                    .compare_exchange_weak(
                        head,
                        head.wrapping_add(1),
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    unsafe {
                        core::ptr::drop_in_place(self.slot_ptr(head));
                    }
                    dropped = true;
                }
                continue;
            }

            unsafe {
                self.slot_ptr(tail).write(value.assume_init_read());
            }
            self.tail
                .value
                .store(tail.wrapping_add(1), Ordering::Release);
            return dropped;
        }
    }

    pub fn try_push(&self, value: T) -> Result<(), T> {
        let head = self.head.value.load(Ordering::Acquire);
        let tail = self.tail.value.load(Ordering::Relaxed);
        if tail.wrapping_sub(head) >= self.capacity() {
            return Err(value);
        }

        unsafe {
            self.slot_ptr(tail).write(value);
        }
        self.tail
            .value
            .store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.value.load(Ordering::Relaxed);
            let tail = self.tail.value.load(Ordering::Acquire);
            if head == tail {
                return None;
            }

            if self
                .head
                .value
                .compare_exchange_weak(
                    head,
                    head.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_err()
            {
                continue;
            }

            let value = unsafe { self.slot_ptr(head).read() };
            return Some(value);
        }
    }
}

impl<T> Drop for MailboxRing<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

pub struct PresentQueue {
    ring: MailboxRing<FrameIntent>,
}

impl PresentQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: MailboxRing::with_capacity_pow2(capacity),
        }
    }

    pub fn publish(&self, intent: FrameIntent) -> bool {
        self.ring.push_overwrite(intent)
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    pub fn take_latest(&self) -> Option<FrameIntent> {
        let mut latest = None;
        while let Some(intent) = self.ring.pop() {
            latest = Some(intent);
        }
        latest
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerState {
    Idle = 0,
    FramePending = 1,
    CommitPending = 2,
    FlipPending = 3,
}

impl SchedulerState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::FramePending,
            2 => Self::CommitPending,
            3 => Self::FlipPending,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotPathMetrics {
    pub produced_frames: u64,
    pub dropped_frames: u64,
    pub presented_frames: u64,
    pub direct_scanout_frames: u64,
    pub missed_vblank_events: u64,
    pub rho_drop_ppm: u64,
    pub eta_scanout_ppm: u64,
    pub avg_commit_latency_ns: u64,
    pub max_commit_latency_ns: u64,
    pub avg_total_latency_ns: u64,
    pub max_total_latency_ns: u64,
}

fn atomic_fetch_max(target: &AtomicU64, candidate: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while candidate > current {
        match target.compare_exchange_weak(current, candidate, Ordering::Release, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

pub struct FrameScheduler {
    state: AtomicU8,
    present_queue: PresentQueue,
    vblank_queue: MailboxRing<VBlankEvent>,
    next_commit_id: AtomicU64,
    inflight_commit_id: AtomicU64,
    inflight_frame_id: AtomicU64,
    expected_flip_seq: AtomicU64,
    flip_deadline_ns: AtomicU64,
    inflight_enqueue_ns: AtomicU64,
    inflight_commit_start_ns: AtomicU64,
    inflight_direct_scanout: AtomicBool,
    missed_vblank: AtomicU64,
    missed_vblank_total: AtomicU64,
    last_refresh_hz: AtomicU32,
    produced_frames: AtomicU64,
    dropped_frames: AtomicU64,
    presented_frames: AtomicU64,
    direct_scanout_frames: AtomicU64,
    commit_latency_sum_ns: AtomicU64,
    total_latency_sum_ns: AtomicU64,
    max_commit_latency_ns: AtomicU64,
    max_total_latency_ns: AtomicU64,
    replay_intent: UnsafeCell<Option<FrameIntent>>,
    replay_assignment: UnsafeCell<PlaneAssignment>,
    last_feedback: UnsafeCell<Option<VblankFeedback>>,
}

unsafe impl Sync for FrameScheduler {}

impl FrameScheduler {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(SchedulerState::Idle as u8),
            present_queue: PresentQueue::new(PRESENT_QUEUE_CAPACITY),
            vblank_queue: MailboxRing::with_capacity_pow2(VBLANK_QUEUE_CAPACITY),
            next_commit_id: AtomicU64::new(0),
            inflight_commit_id: AtomicU64::new(0),
            inflight_frame_id: AtomicU64::new(0),
            expected_flip_seq: AtomicU64::new(0),
            flip_deadline_ns: AtomicU64::new(0),
            inflight_enqueue_ns: AtomicU64::new(0),
            inflight_commit_start_ns: AtomicU64::new(0),
            inflight_direct_scanout: AtomicBool::new(false),
            missed_vblank: AtomicU64::new(0),
            missed_vblank_total: AtomicU64::new(0),
            last_refresh_hz: AtomicU32::new(60),
            produced_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            presented_frames: AtomicU64::new(0),
            direct_scanout_frames: AtomicU64::new(0),
            commit_latency_sum_ns: AtomicU64::new(0),
            total_latency_sum_ns: AtomicU64::new(0),
            max_commit_latency_ns: AtomicU64::new(0),
            max_total_latency_ns: AtomicU64::new(0),
            replay_intent: UnsafeCell::new(None),
            replay_assignment: UnsafeCell::new(PlaneAssignment::empty()),
            last_feedback: UnsafeCell::new(None),
        }
    }

    pub fn state(&self) -> SchedulerState {
        SchedulerState::from_u8(self.state.load(Ordering::Acquire))
    }

    fn set_state(&self, state: SchedulerState) {
        self.state.store(state as u8, Ordering::Release);
    }

    pub fn publish_intent(&self, mut intent: FrameIntent) {
        if intent.enqueue_timestamp_ns == 0 {
            intent.enqueue_timestamp_ns = tsc::read_ns();
        }
        self.produced_frames.fetch_add(1, Ordering::AcqRel);
        if self.present_queue.publish(intent) {
            self.dropped_frames.fetch_add(1, Ordering::AcqRel);
        }
        if self.state() == SchedulerState::Idle {
            self.set_state(SchedulerState::FramePending);
        }
    }

    pub fn on_vblank_event(&self, event: VBlankEvent) {
        let _ = self.vblank_queue.push_overwrite(event);
        if self.state() == SchedulerState::Idle && !self.present_queue.is_empty() {
            self.set_state(SchedulerState::FramePending);
        }
    }

    pub fn has_hot_path_work(&self) -> bool {
        self.inflight_commit_id.load(Ordering::Acquire) != 0
            || !self.present_queue.is_empty()
            || self.state() != SchedulerState::Idle
    }

    pub fn last_feedback(&self) -> Option<VblankFeedback> {
        unsafe { (*self.last_feedback.get()).as_ref().copied() }
    }

    pub fn last_assignment(&self) -> PlaneAssignment {
        unsafe { (*self.replay_assignment.get()).clone() }
    }

    pub fn metrics_snapshot(&self) -> HotPathMetrics {
        let produced = self.produced_frames.load(Ordering::Acquire);
        let dropped = self.dropped_frames.load(Ordering::Acquire);
        let presented = self.presented_frames.load(Ordering::Acquire);
        let direct = self.direct_scanout_frames.load(Ordering::Acquire);
        let rho_drop_ppm = if produced == 0 {
            0
        } else {
            dropped.saturating_mul(1_000_000).saturating_div(produced)
        };
        let eta_scanout_ppm = if presented == 0 {
            0
        } else {
            direct.saturating_mul(1_000_000).saturating_div(presented)
        };
        let commit_sum = self.commit_latency_sum_ns.load(Ordering::Acquire);
        let total_sum = self.total_latency_sum_ns.load(Ordering::Acquire);
        let avg_commit_latency_ns = if presented == 0 {
            0
        } else {
            commit_sum.saturating_div(presented)
        };
        let avg_total_latency_ns = if presented == 0 {
            0
        } else {
            total_sum.saturating_div(presented)
        };
        HotPathMetrics {
            produced_frames: produced,
            dropped_frames: dropped,
            presented_frames: presented,
            direct_scanout_frames: direct,
            missed_vblank_events: self.missed_vblank_total.load(Ordering::Acquire),
            rho_drop_ppm,
            eta_scanout_ppm,
            avg_commit_latency_ns,
            max_commit_latency_ns: self.max_commit_latency_ns.load(Ordering::Acquire),
            avg_total_latency_ns,
            max_total_latency_ns: self.max_total_latency_ns.load(Ordering::Acquire),
        }
    }

    fn try_complete_flip(
        &self,
        device: &DrmDevice,
        event: VBlankEvent,
    ) -> Option<(FrameIntent, PlaneAssignment, VblankFeedback)> {
        let commit_id = self.inflight_commit_id.load(Ordering::Acquire);
        if commit_id == 0 {
            return None;
        }
        let frame_id = self.inflight_frame_id.load(Ordering::Acquire);
        let expected_seq = self.expected_flip_seq.load(Ordering::Acquire);
        if event.seq < expected_seq {
            return None;
        }
        if !device.report_flip_complete(frame_id, commit_id, event.seq, event.timestamp_ns) {
            return None;
        }

        let enqueue_ns = self.inflight_enqueue_ns.load(Ordering::Acquire);
        if enqueue_ns != 0 && event.timestamp_ns >= enqueue_ns {
            let total_latency = event.timestamp_ns.saturating_sub(enqueue_ns);
            self.total_latency_sum_ns
                .fetch_add(total_latency, Ordering::AcqRel);
            atomic_fetch_max(&self.max_total_latency_ns, total_latency);
        }
        self.presented_frames.fetch_add(1, Ordering::AcqRel);
        if self.inflight_direct_scanout.load(Ordering::Acquire) {
            self.direct_scanout_frames.fetch_add(1, Ordering::AcqRel);
        }

        self.inflight_commit_id.store(0, Ordering::Release);
        self.inflight_frame_id.store(0, Ordering::Release);
        self.expected_flip_seq.store(0, Ordering::Release);
        self.flip_deadline_ns.store(0, Ordering::Release);
        self.missed_vblank.store(0, Ordering::Release);
        self.inflight_enqueue_ns.store(0, Ordering::Release);
        self.inflight_commit_start_ns.store(0, Ordering::Release);
        self.inflight_direct_scanout.store(false, Ordering::Release);

        let feedback = VblankFeedback {
            timestamp_ns: event.timestamp_ns,
            presented_frame_id: frame_id,
            refresh_hz: self.last_refresh_hz.load(Ordering::Acquire),
            graph_crc: (frame_id as u32) ^ (event.seq as u32),
        };
        unsafe {
            *self.last_feedback.get() = Some(feedback);
        }

        let intent = unsafe { (*self.replay_intent.get()).clone()? };
        let assignment = unsafe { (*self.replay_assignment.get()).clone() };
        self.set_state(if self.present_queue.is_empty() {
            SchedulerState::Idle
        } else {
            SchedulerState::FramePending
        });
        Some((intent, assignment, feedback))
    }

    fn handle_missed_vblank(&self, device: &DrmDevice) {
        let deadline = self.flip_deadline_ns.load(Ordering::Acquire);
        if deadline == 0 {
            return;
        }
        if self.inflight_commit_id.load(Ordering::Acquire) == 0 {
            return;
        }
        let now_ns = tsc::read_ns();
        if now_ns <= deadline {
            return;
        }

        let miss = self.missed_vblank.fetch_add(1, Ordering::AcqRel) + 1;
        self.missed_vblank_total.fetch_add(1, Ordering::AcqRel);
        device.abort_inflight_commit();
        self.inflight_commit_id.store(0, Ordering::Release);
        self.inflight_frame_id.store(0, Ordering::Release);
        self.expected_flip_seq.store(0, Ordering::Release);
        self.flip_deadline_ns.store(0, Ordering::Release);
        self.inflight_enqueue_ns.store(0, Ordering::Release);
        self.inflight_commit_start_ns.store(0, Ordering::Release);
        self.inflight_direct_scanout.store(false, Ordering::Release);

        if miss == 1 {
            self.set_state(SchedulerState::CommitPending);
        } else {
            let _ = device.rearm_crtc(0);
            self.missed_vblank.store(0, Ordering::Release);
            let has_replay = unsafe { (*self.replay_intent.get()).is_some() };
            self.set_state(if self.present_queue.is_empty() && !has_replay {
                SchedulerState::Idle
            } else {
                SchedulerState::FramePending
            });
        }
    }

    fn choose_primary(
        &self,
        screen: Rect,
        candidates: &[PlaneCandidate],
        sorted_indices: &[usize],
    ) -> Option<usize> {
        for &idx in sorted_indices {
            let candidate = candidates[idx];
            if candidate.opaque && candidate.dst == screen {
                return Some(idx);
            }
        }
        sorted_indices.first().copied()
    }

    fn build_transaction(
        &self,
        device: &DrmDevice,
        screen: Rect,
        intent: &FrameIntent,
        placements: &[SurfacePlacement],
    ) -> (PlaneAssignment, AtomicKmsTransaction) {
        let placement_map = placements
            .iter()
            .map(|placement| (placement.surface_id, *placement))
            .collect::<BTreeMap<SurfaceId, SurfacePlacement>>();

        let mut candidates = Vec::new();
        for candidate in intent.candidates.iter() {
            let Some(placement) = placement_map.get(&candidate.surface_id) else {
                continue;
            };
            let Some(dst) = placement.rect.intersection(&screen) else {
                continue;
            };
            candidates.push(PlaneCandidate {
                surface_id: candidate.surface_id,
                plane_type: DrmPlaneType::Overlay,
                z: candidate.z,
                src: Rect::new(0, 0, dst.width, dst.height),
                dst,
                opaque: candidate.opaque,
                format: 0x3432_5258,
                buffer: GPUBufferHandle {
                    handle: candidate.surface_id,
                    paddr: candidate.surface_id,
                    width: dst.width,
                    height: dst.height,
                    stride: dst.width.saturating_mul(4),
                    format: 0x3432_5258,
                },
            });
        }

        if candidates.is_empty() {
            for placement in placements.iter() {
                let Some(dst) = placement.rect.intersection(&screen) else {
                    continue;
                };
                candidates.push(PlaneCandidate {
                    surface_id: placement.surface_id,
                    plane_type: DrmPlaneType::Overlay,
                    z: placement.z_index,
                    src: Rect::new(0, 0, dst.width, dst.height),
                    dst,
                    opaque: placement.opaque,
                    format: 0x3432_5258,
                    buffer: GPUBufferHandle {
                        handle: placement.surface_id,
                        paddr: placement.surface_id,
                        width: dst.width,
                        height: dst.height,
                        stride: dst.width.saturating_mul(4),
                        format: 0x3432_5258,
                    },
                });
            }
        }

        let mut sorted_indices = (0..candidates.len()).collect::<Vec<_>>();
        sorted_indices.sort_by_key(|idx| core::cmp::Reverse(candidates[*idx].z));

        let primary_idx = self.choose_primary(screen, &candidates, &sorted_indices);
        let cursor_idx = sorted_indices.iter().copied().find(|idx| {
            Some(*idx) != primary_idx
                && candidates[*idx].dst.width <= 128
                && candidates[*idx].dst.height <= 128
        });

        let overlay_limit = device.max_overlay_planes().max(1);
        let mut assignment = PlaneAssignment::empty();
        if let Some(idx) = primary_idx {
            assignment.primary = Some(candidates[idx].surface_id);
        }
        if let Some(idx) = cursor_idx {
            assignment.cursor = Some(candidates[idx].surface_id);
        }
        for idx in sorted_indices.iter().copied() {
            let sid = candidates[idx].surface_id;
            if Some(sid) == assignment.primary || Some(sid) == assignment.cursor {
                continue;
            }
            if assignment.overlays.len() >= overlay_limit {
                break;
            }
            assignment.overlays.push(sid);
        }

        let mut selected = Vec::new();
        if let Some(sid) = assignment.primary {
            if let Some(candidate) = candidates.iter_mut().find(|c| c.surface_id == sid) {
                candidate.plane_type = DrmPlaneType::Primary;
                selected.push(*candidate);
            }
        }
        for sid in assignment.overlays.iter().copied() {
            if let Some(candidate) = candidates.iter_mut().find(|c| c.surface_id == sid) {
                candidate.plane_type = DrmPlaneType::Overlay;
                selected.push(*candidate);
            }
        }
        if let Some(sid) = assignment.cursor {
            if let Some(candidate) = candidates.iter_mut().find(|c| c.surface_id == sid) {
                candidate.plane_type = DrmPlaneType::Cursor;
                selected.push(*candidate);
            }
        }

        let damage_regions = intent
            .damage_tiles
            .iter()
            .map(|tile| DamageRegion {
                rect: Rect::new(
                    tile.x as i32,
                    tile.y as i32,
                    tile.width as u32,
                    tile.height as u32,
                ),
                epoch: intent.frame_id,
            })
            .collect::<Vec<_>>();

        let commit_id = self.next_commit_id.fetch_add(1, Ordering::AcqRel) + 1;
        let txn = AtomicKmsTransaction {
            frame_id: intent.frame_id,
            commit_id,
            crtc_id: 0,
            connector_id: 0,
            mode: None,
            planes: selected,
            damage_regions,
            target_refresh_hz: intent.target_refresh_hz,
            present_mode: intent.mode,
        };
        (assignment, txn)
    }

    pub fn run_worker_tick(
        &self,
        screen: Rect,
        placements: &[SurfacePlacement],
    ) -> Result<Option<(FrameIntent, PlaneAssignment, VblankFeedback)>, &'static str> {
        let device = DRM_MANAGER.first_device().ok_or("drm device unavailable")?;
        self.run_worker_tick_with_device(device.as_ref(), screen, placements)
    }

    fn run_worker_tick_with_device(
        &self,
        device: &DrmDevice,
        screen: Rect,
        placements: &[SurfacePlacement],
    ) -> Result<Option<(FrameIntent, PlaneAssignment, VblankFeedback)>, &'static str> {
        let mut latest_vblank = None;
        while let Some(event) = self.vblank_queue.pop() {
            latest_vblank = Some(event);
        }

        if let Some(event) = latest_vblank {
            if self.state() == SchedulerState::FramePending {
                self.set_state(SchedulerState::CommitPending);
            }
            if self.state() == SchedulerState::FlipPending {
                if let Some(done) = self.try_complete_flip(device, event) {
                    return Ok(Some(done));
                }
            }
        }

        match self.state() {
            SchedulerState::Idle => {
                if !self.present_queue.is_empty() {
                    self.set_state(SchedulerState::FramePending);
                }
                Ok(None)
            }
            SchedulerState::FramePending => Ok(None),
            SchedulerState::CommitPending => {
                if self.inflight_commit_id.load(Ordering::Acquire) != 0 {
                    return Err("double commit blocked");
                }

                let intent = self
                    .present_queue
                    .take_latest()
                    .or_else(|| unsafe { (*self.replay_intent.get()).clone() })
                    .ok_or("no frame intent")?;
                let (assignment, txn) = self.build_transaction(device, screen, &intent, placements);
                let commit_start_ns = tsc::read_ns();
                let result = device.commit_transaction(&txn)?;
                let commit_latency_ns = result.timestamp_ns.saturating_sub(commit_start_ns);
                self.commit_latency_sum_ns
                    .fetch_add(commit_latency_ns, Ordering::AcqRel);
                atomic_fetch_max(&self.max_commit_latency_ns, commit_latency_ns);

                self.inflight_commit_id
                    .store(txn.commit_id, Ordering::Release);
                self.inflight_frame_id
                    .store(txn.frame_id, Ordering::Release);
                self.expected_flip_seq
                    .store(result.vblank_seq, Ordering::Release);
                self.last_refresh_hz
                    .store(result.refresh_hz, Ordering::Release);
                self.inflight_enqueue_ns
                    .store(intent.enqueue_timestamp_ns, Ordering::Release);
                self.inflight_commit_start_ns
                    .store(commit_start_ns, Ordering::Release);
                self.inflight_direct_scanout
                    .store(result.direct_scanout_planes > 0, Ordering::Release);
                let period = DrmDevice::vblank_period_ns(result.refresh_hz);
                let deadline = result
                    .timestamp_ns
                    .saturating_add(period.saturating_mul(3).saturating_div(2));
                self.flip_deadline_ns.store(deadline, Ordering::Release);
                unsafe {
                    *self.replay_intent.get() = Some(intent);
                    *self.replay_assignment.get() = assignment;
                }
                self.set_state(SchedulerState::FlipPending);
                Ok(None)
            }
            SchedulerState::FlipPending => {
                self.handle_missed_vblank(device);
                Ok(None)
            }
        }
    }
}

pub struct AtomicPresenter {
    mode: DisplayPresentMode,
    next_frame_id: u64,
    scheduler: FrameScheduler,
}

impl AtomicPresenter {
    pub fn new() -> Self {
        Self {
            mode: DisplayPresentMode::AdaptiveSync,
            next_frame_id: 1,
            scheduler: FrameScheduler::new(),
        }
    }

    pub fn mode(&self) -> DisplayPresentMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DisplayPresentMode) {
        self.mode = mode;
    }

    pub fn last_feedback(&self) -> Option<VblankFeedback> {
        self.scheduler.last_feedback()
    }

    pub fn last_assignment(&self) -> PlaneAssignment {
        self.scheduler.last_assignment()
    }

    pub fn metrics_snapshot(&self) -> HotPathMetrics {
        self.scheduler.metrics_snapshot()
    }

    pub fn build_intent(
        &mut self,
        screen: Rect,
        damage_regions: &[Rect],
        placements: &[SurfacePlacement],
        cursor_position: Point,
    ) -> FrameIntent {
        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.saturating_add(1);

        let mut damage_tiles = Vec::new();
        for region in damage_regions {
            if let Some(clipped) = region.intersection(&screen) {
                self.append_damage_tiles(clipped, &mut damage_tiles);
            }
        }

        let mut candidates = Vec::with_capacity(placements.len());
        for placement in placements {
            let clipped_damage = damage_regions
                .iter()
                .find_map(|damage| placement.rect.intersection(damage))
                .unwrap_or(placement.rect);
            candidates.push(ScanoutCandidate {
                surface_id: placement.surface_id,
                z: placement.z_index,
                opaque: placement.opaque,
                transform: SurfaceTransform::Identity,
                damage: clipped_damage,
            });
        }

        let target_refresh_hz = match self.mode {
            DisplayPresentMode::Mailbox => 240,
            DisplayPresentMode::VblankFifo => 120,
            DisplayPresentMode::AdaptiveSync => {
                if damage_regions.is_empty() {
                    1
                } else {
                    240
                }
            }
        };

        FrameIntent {
            frame_id,
            enqueue_timestamp_ns: tsc::read_ns(),
            damage_tiles,
            candidates,
            composed_passes: vec![
                CompositorPass::BaseComposite,
                CompositorPass::KawaseBlur {
                    radius: 12,
                    passes: 3,
                },
                CompositorPass::SdfShadow {
                    radius: 22,
                    spread: 14,
                },
            ],
            target_refresh_hz,
            mode: self.mode,
            cursor_position: Some(cursor_position),
        }
    }

    pub fn enqueue(&self, intent: FrameIntent) {
        self.scheduler.publish_intent(intent);
    }

    pub fn has_pending_intent(&self) -> bool {
        self.scheduler.has_hot_path_work()
    }

    pub fn inject_vblank(&self, event: VBlankEvent) {
        self.scheduler.on_vblank_event(event);
    }

    pub fn commit_latest(
        &self,
        screen: Rect,
        placements: &[SurfacePlacement],
        now_ns: u64,
    ) -> Result<(FrameIntent, PlaneAssignment, VblankFeedback), &'static str> {
        if self.scheduler.has_hot_path_work() {
            let mut injected = false;
            let gpu_count = crate::drivers::gpu_native::device_count();
            for device_index in 0..gpu_count {
                if let Some(event) = crate::drivers::gpu_native::dispatch_vblank_irq(device_index) {
                    self.scheduler.on_vblank_event(event);
                    injected = true;
                }
            }

            // Host/sim fallback path when no physical GPU native device exists.
            if !injected && gpu_count == 0 {
                if let Some(device) = DRM_MANAGER.first_device() {
                    let seq = device.signal_vblank(now_ns);
                    self.scheduler.on_vblank_event(VBlankEvent {
                        seq,
                        timestamp_ns: now_ns,
                        crtc_id: 0,
                    });
                }
            }
        }

        if let Some(done) = self.scheduler.run_worker_tick(screen, placements)? {
            return Ok(done);
        }
        Err("no completed frame")
    }

    fn append_damage_tiles(&self, rect: Rect, out: &mut Vec<DamageTile>) {
        let x_start = rect.x.max(0);
        let y_start = rect.y.max(0);
        let x_end = rect.right().max(x_start);
        let y_end = rect.bottom().max(y_start);
        let mut y = y_start;
        while y < y_end {
            let mut x = x_start;
            while x < x_end {
                let width = (x_end - x).min(DAMAGE_TILE_SIZE).max(0) as u16;
                let height = (y_end - y).min(DAMAGE_TILE_SIZE).max(0) as u16;
                out.push(DamageTile {
                    x: x as u16,
                    y: y as u16,
                    width,
                    height,
                });
                x += DAMAGE_TILE_SIZE;
            }
            y += DAMAGE_TILE_SIZE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameScheduler, MailboxRing, PresentQueue, SchedulerState, SurfacePlacement};
    use crate::cpu::tsc;
    use crate::drivers::drm::{
        DrmConnector, DrmConnectorStatus, DrmCrtc, DrmDevice, DrmPlane, DrmPlaneType, VBlankEvent,
    };
    use crate::gui::protocol::{
        DisplayPresentMode, FrameIntent, Rect, ScanoutCandidate, SurfaceTransform,
    };
    use alloc::sync::Arc;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::sync::atomic::Ordering;

    fn mk_frame(frame_id: u64, surface_id: u64) -> FrameIntent {
        FrameIntent {
            frame_id,
            enqueue_timestamp_ns: tsc::read_ns(),
            damage_tiles: Vec::new(),
            candidates: vec![ScanoutCandidate {
                surface_id,
                z: 10,
                opaque: true,
                transform: SurfaceTransform::Identity,
                damage: Rect::new(0, 0, 128, 128),
            }],
            composed_passes: Vec::new(),
            target_refresh_hz: 60,
            mode: DisplayPresentMode::VblankFifo,
            cursor_position: None,
        }
    }

    fn mk_test_device() -> DrmDevice {
        let device = DrmDevice::new(0xD15A, "test-card");
        device.add_crtc(Arc::new(DrmCrtc::new(0, 0)));
        let connector = Arc::new(DrmConnector::new(0, 0));
        *connector.connection.lock() = DrmConnectorStatus::Connected;
        device.add_connector(connector);
        device.add_plane(Arc::new(DrmPlane::new_with_type(0, DrmPlaneType::Primary)));
        device.add_plane(Arc::new(DrmPlane::new_with_type(1, DrmPlaneType::Overlay)));
        device.add_plane(Arc::new(DrmPlane::new_with_type(2, DrmPlaneType::Cursor)));
        device
    }

    fn placements() -> Vec<SurfacePlacement> {
        vec![SurfacePlacement {
            surface_id: 1,
            rect: Rect::new(0, 0, 128, 128),
            z_index: 10,
            opaque: true,
        }]
    }

    #[test]
    fn mailbox_overwrites_oldest_when_full() {
        let ring = MailboxRing::<u32>::with_capacity_pow2(2);
        let _ = ring.push_overwrite(10);
        let _ = ring.push_overwrite(20);
        let _ = ring.push_overwrite(30);
        assert_eq!(ring.pop(), Some(20));
        assert_eq!(ring.pop(), Some(30));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn present_queue_returns_latest_only() {
        let queue = PresentQueue::new(4);
        let mut mk = |id| FrameIntent {
            frame_id: id,
            enqueue_timestamp_ns: 1,
            damage_tiles: Vec::new(),
            candidates: Vec::new(),
            composed_passes: Vec::new(),
            target_refresh_hz: 60,
            mode: DisplayPresentMode::VblankFifo,
            cursor_position: None,
        };
        let _ = queue.publish(mk(1));
        let _ = queue.publish(mk(2));
        let _ = queue.publish(mk(3));
        assert_eq!(queue.take_latest().map(|f| f.frame_id), Some(3));
    }

    #[test]
    fn scheduler_rejects_double_commit_in_same_window() {
        let scheduler = FrameScheduler::new();
        let device = mk_test_device();
        let placements = placements();
        let screen = Rect::new(0, 0, 128, 128);
        scheduler.publish_intent(mk_frame(1, 1));
        scheduler.on_vblank_event(VBlankEvent {
            seq: 1,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("first commit must succeed");
        assert_eq!(scheduler.state(), SchedulerState::FlipPending);

        scheduler.set_state(SchedulerState::CommitPending);
        let err = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect_err("double commit must be rejected");
        assert_eq!(err, "double commit blocked");
    }

    #[test]
    fn scheduler_discards_out_of_order_flip_complete() {
        let scheduler = FrameScheduler::new();
        let device = mk_test_device();
        let placements = placements();
        let screen = Rect::new(0, 0, 128, 128);

        scheduler.publish_intent(mk_frame(10, 1));
        scheduler.on_vblank_event(VBlankEvent {
            seq: 4,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("commit path should run");
        assert_eq!(scheduler.state(), SchedulerState::FlipPending);

        let inflight_commit = scheduler.inflight_commit_id.load(Ordering::Acquire);
        let inflight_frame = scheduler.inflight_frame_id.load(Ordering::Acquire);
        let expected_seq = scheduler.expected_flip_seq.load(Ordering::Acquire);
        assert!(!device.report_flip_complete(
            inflight_frame.saturating_add(1),
            inflight_commit,
            expected_seq,
            tsc::read_ns()
        ));

        scheduler.on_vblank_event(VBlankEvent {
            seq: expected_seq,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let done = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("flip completion tick should succeed");
        assert!(done.is_some());
    }

    #[test]
    fn scheduler_latest_wins_after_flip_pending() {
        let scheduler = FrameScheduler::new();
        let device = mk_test_device();
        let placements = placements();
        let screen = Rect::new(0, 0, 128, 128);

        scheduler.publish_intent(mk_frame(1, 1));
        scheduler.on_vblank_event(VBlankEvent {
            seq: 10,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("first commit should succeed");

        scheduler.publish_intent(mk_frame(2, 1));
        scheduler.publish_intent(mk_frame(3, 1));

        let expected_seq = scheduler.expected_flip_seq.load(Ordering::Acquire);
        scheduler.on_vblank_event(VBlankEvent {
            seq: expected_seq,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let done = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("flip completion should succeed")
            .expect("one frame should complete");
        assert_eq!(done.0.frame_id, 1);
        assert_eq!(scheduler.state(), SchedulerState::FramePending);

        scheduler.on_vblank_event(VBlankEvent {
            seq: expected_seq.saturating_add(1),
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("second commit should be accepted");
        assert_eq!(scheduler.state(), SchedulerState::FlipPending);
        assert_eq!(scheduler.inflight_frame_id.load(Ordering::Acquire), 3);
    }

    #[test]
    fn scheduler_recovers_after_two_missed_vblanks() {
        let scheduler = FrameScheduler::new();
        let device = mk_test_device();
        let placements = placements();
        let screen = Rect::new(0, 0, 128, 128);

        scheduler.publish_intent(mk_frame(20, 1));
        scheduler.on_vblank_event(VBlankEvent {
            seq: 100,
            timestamp_ns: tsc::read_ns(),
            crtc_id: 0,
        });
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("initial commit should succeed");
        assert_eq!(scheduler.state(), SchedulerState::FlipPending);

        scheduler
            .flip_deadline_ns
            .store(tsc::read_ns().saturating_sub(1), Ordering::Release);
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("first missed-vblank handling should succeed");
        assert_eq!(scheduler.state(), SchedulerState::CommitPending);
        assert_eq!(scheduler.missed_vblank.load(Ordering::Acquire), 1);

        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("re-commit after first miss should succeed");
        assert_eq!(scheduler.state(), SchedulerState::FlipPending);

        scheduler
            .flip_deadline_ns
            .store(tsc::read_ns().saturating_sub(1), Ordering::Release);
        let _ = scheduler
            .run_worker_tick_with_device(&device, screen, &placements)
            .expect("second miss should trigger recovery");

        assert_eq!(scheduler.inflight_commit_id.load(Ordering::Acquire), 0);
        assert_eq!(scheduler.missed_vblank.load(Ordering::Acquire), 0);
        assert!(scheduler.missed_vblank_total.load(Ordering::Acquire) >= 2);
        assert_eq!(scheduler.state(), SchedulerState::FramePending);
    }
}
