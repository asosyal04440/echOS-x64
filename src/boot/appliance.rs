use alloc::vec::Vec;
use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

#[cfg(target_os = "uefi")]
use uefi::table::runtime::{VariableAttributes, VariableVendor};
#[cfg(target_os = "uefi")]
use uefi::{cstr16, Status};

const BOOT_CONTROL_MAGIC: u32 = u32::from_le_bytes(*b"ECBC");
const BOOT_CONTROL_VERSION: u16 = 1;
const DEFAULT_BOOT_ATTEMPTS: u8 = 3;
const BOOT_FLAG_AUTO_LOGIN: u8 = 1 << 0;
const BOOT_FLAG_SUSPEND_RESUME_SMOKE: u8 = 1 << 1;
const BOOT_FLAG_FS_SMOKE_TEST: u8 = 1 << 2;
#[cfg(target_os = "uefi")]
const APPLIANCE_VENDOR_GUID: VariableVendor = VariableVendor(uefi::Guid::new(
    [0x83, 0x61, 0x26, 0x6d],
    [0x25, 0x4b],
    [0xab, 0x49],
    0x8c,
    0x4d,
    [0x74, 0x2f, 0x57, 0x78, 0x62, 0x90],
));

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotId {
    None = 0,
    SystemA = 1,
    SystemB = 2,
    Recovery = 3,
}

impl SlotId {
    pub const fn inactive_pair(self) -> Self {
        match self {
            Self::SystemA => Self::SystemB,
            Self::SystemB => Self::SystemA,
            Self::Recovery | Self::None => Self::SystemA,
        }
    }

    pub const fn system_partition_label(self) -> &'static str {
        match self {
            Self::SystemA => "system_a",
            Self::SystemB => "system_b",
            Self::Recovery => "recovery",
            Self::None => "system_a",
        }
    }
}

impl From<u8> for SlotId {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::SystemA,
            2 => Self::SystemB,
            3 => Self::Recovery,
            _ => Self::None,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootStage {
    LoaderEntry = 1,
    BootControlLoaded = 2,
    KernelCoreReady = 3,
    StorageMounted = 4,
    NetworkReady = 5,
    DisplayReady = 6,
    DesktopReady = 7,
    AppBasketReady = 8,
    Recovery = 9,
}

impl From<u8> for BootStage {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::LoaderEntry,
            2 => Self::BootControlLoaded,
            3 => Self::KernelCoreReady,
            4 => Self::StorageMounted,
            5 => Self::NetworkReady,
            6 => Self::DisplayReady,
            7 => Self::DesktopReady,
            8 => Self::AppBasketReady,
            9 => Self::Recovery,
            _ => Self::LoaderEntry,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollbackReason {
    None = 0,
    BootAttemptsExceeded = 1,
    Panic = 2,
    UpdateRejected = 3,
    RecoveryRequested = 4,
}

impl From<u8> for RollbackReason {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::BootAttemptsExceeded,
            2 => Self::Panic,
            3 => Self::UpdateRejected,
            4 => Self::RecoveryRequested,
            _ => Self::None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BootControlBlock {
    pub magic: u32,
    pub version: u16,
    pub header_size: u16,
    pub active_slot: u8,
    pub pending_slot: u8,
    pub attempts_remaining: u8,
    pub boot_ok: u8,
    pub last_boot_stage: u8,
    pub rollback_reason: u8,
    pub panic_count: u32,
    pub slot_generation_a: u64,
    pub slot_generation_b: u64,
    pub boot_epoch: u64,
    pub reserved: [u8; 80],
    pub crc32: u32,
}

impl BootControlBlock {
    pub const fn new() -> Self {
        Self {
            magic: BOOT_CONTROL_MAGIC,
            version: BOOT_CONTROL_VERSION,
            header_size: size_of::<Self>() as u16,
            active_slot: SlotId::SystemA as u8,
            pending_slot: SlotId::None as u8,
            attempts_remaining: DEFAULT_BOOT_ATTEMPTS,
            boot_ok: 1,
            last_boot_stage: BootStage::LoaderEntry as u8,
            rollback_reason: RollbackReason::None as u8,
            panic_count: 0,
            slot_generation_a: 1,
            slot_generation_b: 0,
            boot_epoch: 0,
            reserved: [0; 80],
            crc32: 0,
        }
    }

    pub fn with_crc(mut self) -> Self {
        self.crc32 = 0;
        self.crc32 = crc32_of(bytes_of(&self));
        self
    }

    pub fn validate(&self) -> bool {
        if self.magic != BOOT_CONTROL_MAGIC
            || self.version != BOOT_CONTROL_VERSION
            || self.header_size as usize != size_of::<Self>()
        {
            return false;
        }
        let mut temp = *self;
        let expected = temp.crc32;
        temp.crc32 = 0;
        expected == crc32_of(bytes_of(&temp))
    }

    pub fn active_slot(&self) -> SlotId {
        SlotId::from(self.active_slot)
    }

    pub fn pending_slot(&self) -> SlotId {
        SlotId::from(self.pending_slot)
    }

    pub fn last_stage(&self) -> BootStage {
        BootStage::from(self.last_boot_stage)
    }

    pub fn rollback_reason(&self) -> RollbackReason {
        RollbackReason::from(self.rollback_reason)
    }

    pub fn boot_flags(&self) -> u8 {
        self.reserved[0]
    }

    pub fn set_boot_flags(&mut self, flags: u8) {
        self.reserved[0] = flags;
    }

    pub fn auto_login_enabled(&self) -> bool {
        (self.boot_flags() & BOOT_FLAG_AUTO_LOGIN) != 0
    }

    pub fn suspend_resume_smoke_enabled(&self) -> bool {
        (self.boot_flags() & BOOT_FLAG_SUSPEND_RESUME_SMOKE) != 0
    }

    pub fn fs_smoke_test_enabled(&self) -> bool {
        (self.boot_flags() & BOOT_FLAG_FS_SMOKE_TEST) != 0
    }

    pub fn begin_boot(&mut self) {
        let pending = self.pending_slot();
        let active = self.active_slot();
        if pending != SlotId::None {
            self.active_slot = pending as u8;
        } else if active == SlotId::None {
            self.active_slot = SlotId::SystemA as u8;
        }

        self.boot_epoch = self.boot_epoch.saturating_add(1);
        self.boot_ok = 0;
        self.last_boot_stage = BootStage::LoaderEntry as u8;
        self.rollback_reason = RollbackReason::None as u8;
        if self.attempts_remaining == 0 {
            self.rollback_to_stable(RollbackReason::BootAttemptsExceeded);
        } else {
            self.attempts_remaining = self.attempts_remaining.saturating_sub(1);
        }
        *self = self.with_crc();
    }

    pub fn stage_update(&mut self, target_slot: SlotId) {
        self.pending_slot = target_slot as u8;
        self.boot_ok = 0;
        self.attempts_remaining = DEFAULT_BOOT_ATTEMPTS;
        match target_slot {
            SlotId::SystemA => {
                self.slot_generation_a = self.slot_generation_a.saturating_add(1);
            }
            SlotId::SystemB => {
                self.slot_generation_b = self.slot_generation_b.saturating_add(1);
            }
            _ => {}
        }
        *self = self.with_crc();
    }

    pub fn publish_stage(&mut self, stage: BootStage) {
        if stage > self.last_stage() {
            self.last_boot_stage = stage as u8;
            *self = self.with_crc();
        }
    }

    pub fn mark_boot_success(&mut self) {
        self.boot_ok = 1;
        self.attempts_remaining = DEFAULT_BOOT_ATTEMPTS;
        if self.pending_slot() != SlotId::None {
            self.active_slot = self.pending_slot;
            self.pending_slot = SlotId::None as u8;
        }
        self.rollback_reason = RollbackReason::None as u8;
        self.publish_stage(BootStage::AppBasketReady);
        *self = self.with_crc();
    }

    pub fn record_panic(&mut self, stage: BootStage) {
        self.panic_count = self.panic_count.saturating_add(1);
        self.boot_ok = 0;
        self.last_boot_stage = stage as u8;
        self.rollback_reason = RollbackReason::Panic as u8;
        *self = self.with_crc();
    }

    pub fn rollback_to_stable(&mut self, reason: RollbackReason) {
        let stable = self.pending_slot().inactive_pair();
        self.active_slot = stable as u8;
        self.pending_slot = SlotId::None as u8;
        self.attempts_remaining = DEFAULT_BOOT_ATTEMPTS;
        self.boot_ok = 0;
        self.last_boot_stage = BootStage::Recovery as u8;
        self.rollback_reason = reason as u8;
        *self = self.with_crc();
    }
}

impl Default for BootControlBlock {
    fn default() -> Self {
        Self::new().with_crc()
    }
}

#[derive(Clone, Debug)]
pub struct BootHealthReport {
    pub active_slot: SlotId,
    pub pending_slot: SlotId,
    pub last_boot_stage: BootStage,
    pub rollback_reason: RollbackReason,
    pub attempts_remaining: u8,
    pub boot_ok: bool,
    pub panic_count: u32,
    pub boot_epoch: u64,
}

impl From<&BootControlBlock> for BootHealthReport {
    fn from(value: &BootControlBlock) -> Self {
        Self {
            active_slot: value.active_slot(),
            pending_slot: value.pending_slot(),
            last_boot_stage: value.last_stage(),
            rollback_reason: value.rollback_reason(),
            attempts_remaining: value.attempts_remaining,
            boot_ok: value.boot_ok != 0,
            panic_count: value.panic_count,
            boot_epoch: value.boot_epoch,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct UpdateBundleManifest {
    pub target_slot: u8,
    pub generation: u64,
    pub image_size: u64,
    pub image_hash: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RecoveryRecord {
    pub boot_epoch: u64,
    pub slot: u8,
    pub stage: u8,
    pub reason: u8,
    pub panic_count: u32,
}

static BOOT_CONTROL_SHADOW: Mutex<BootControlBlock> = Mutex::new(BootControlBlock::new());
static CURRENT_BOOT_STAGE: AtomicU32 = AtomicU32::new(BootStage::LoaderEntry as u32);
static BOOT_FLAGS: AtomicU32 = AtomicU32::new(0);
static PACKAGED_PE_SMOKE_BUNDLE: Mutex<Option<Vec<u8>>> = Mutex::new(None);
static CURATED_APP_BUNDLES: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

pub fn init_shadow(block: BootControlBlock) {
    let mut guard = BOOT_CONTROL_SHADOW.lock();
    *guard = if block.validate() {
        block
    } else {
        BootControlBlock::default()
    };
    CURRENT_BOOT_STAGE.store(guard.last_boot_stage as u32, Ordering::Release);
    BOOT_FLAGS.store(guard.boot_flags() as u32, Ordering::Release);
}

pub fn shadow_snapshot() -> BootControlBlock {
    *BOOT_CONTROL_SHADOW.lock()
}

pub fn health_report() -> BootHealthReport {
    BootHealthReport::from(&*BOOT_CONTROL_SHADOW.lock())
}

pub fn current_active_slot() -> SlotId {
    BOOT_CONTROL_SHADOW.lock().active_slot()
}

pub fn current_system_partition_label() -> &'static str {
    current_active_slot().system_partition_label()
}

pub fn auto_login_requested() -> bool {
    (BOOT_FLAGS.load(Ordering::Acquire) as u8 & BOOT_FLAG_AUTO_LOGIN) != 0
}

pub fn suspend_resume_smoke_requested() -> bool {
    (BOOT_FLAGS.load(Ordering::Acquire) as u8 & BOOT_FLAG_SUSPEND_RESUME_SMOKE) != 0
}

pub fn fs_smoke_test_requested() -> bool {
    (BOOT_FLAGS.load(Ordering::Acquire) as u8 & BOOT_FLAG_FS_SMOKE_TEST) != 0
}

pub fn clear_suspend_resume_smoke_request() {
    let snapshot = {
        let mut guard = BOOT_CONTROL_SHADOW.lock();
        let mut flags = guard.boot_flags();
        if (flags & BOOT_FLAG_SUSPEND_RESUME_SMOKE) == 0 {
            return;
        }
        flags &= !BOOT_FLAG_SUSPEND_RESUME_SMOKE;
        guard.set_boot_flags(flags);
        *guard = guard.with_crc();
        *guard
    };
    BOOT_FLAGS.store(snapshot.boot_flags() as u32, Ordering::Release);
    persist_shadow(&snapshot);
}

pub fn seed_packaged_pe_smoke_bundle(bytes: Vec<u8>) {
    *PACKAGED_PE_SMOKE_BUNDLE.lock() = Some(bytes);
}

pub fn take_packaged_pe_smoke_bundle() -> Option<Vec<u8>> {
    PACKAGED_PE_SMOKE_BUNDLE.lock().take()
}

pub fn seed_curated_app_bundle(bytes: Vec<u8>) {
    CURATED_APP_BUNDLES.lock().push(bytes);
}

pub fn take_curated_app_bundles() -> Vec<Vec<u8>> {
    let mut guard = CURATED_APP_BUNDLES.lock();
    let mut out = Vec::new();
    core::mem::swap(&mut *guard, &mut out);
    out
}

pub fn publish_stage(stage: BootStage) {
    CURRENT_BOOT_STAGE.store(stage as u32, Ordering::Release);
    let snapshot = {
        let mut guard = BOOT_CONTROL_SHADOW.lock();
        guard.publish_stage(stage);
        *guard
    };
    crate::serial_println!(
        "[BOOTCTRL] stage={} active_slot={} pending_slot={}",
        boot_stage_label(stage),
        slot_label(snapshot.active_slot()),
        slot_label(snapshot.pending_slot())
    );
    persist_shadow(&snapshot);
}

pub fn mark_boot_success() {
    let snapshot = {
        let mut guard = BOOT_CONTROL_SHADOW.lock();
        guard.mark_boot_success();
        *guard
    };
    CURRENT_BOOT_STAGE.store(BootStage::AppBasketReady as u32, Ordering::Release);
    crate::serial_println!(
        "[BOOTCTRL] success active_slot={} boot_epoch={}",
        slot_label(snapshot.active_slot()),
        snapshot.boot_epoch
    );
    persist_shadow(&snapshot);
}

pub fn record_panic() {
    let stage = BootStage::from(CURRENT_BOOT_STAGE.load(Ordering::Acquire) as u8);
    let snapshot = {
        let mut guard = BOOT_CONTROL_SHADOW.lock();
        guard.record_panic(stage);
        *guard
    };
    crate::serial_println!(
        "[BOOTCTRL] panic stage={} count={}",
        boot_stage_label(stage),
        snapshot.panic_count
    );
    persist_shadow(&snapshot);
}

pub fn begin_update(target_slot: SlotId) {
    let snapshot = {
        let mut guard = BOOT_CONTROL_SHADOW.lock();
        guard.stage_update(target_slot);
        *guard
    };
    crate::serial_println!(
        "[BOOTCTRL] update target_slot={} generation_a={} generation_b={}",
        slot_label(target_slot),
        snapshot.slot_generation_a,
        snapshot.slot_generation_b
    );
    persist_shadow(&snapshot);
}

#[cfg(target_os = "uefi")]
pub fn load_persisted() -> Option<BootControlBlock> {
    let runtime = crate::boot::runtime_services()?;
    let (data, _) = runtime
        .get_variable_boxed(cstr16!("echOSBootControl"), &APPLIANCE_VENDOR_GUID)
        .ok()?;
    if data.len() != size_of::<BootControlBlock>() {
        return None;
    }
    let block = unsafe { *(data.as_ptr() as *const BootControlBlock) };
    block.validate().then_some(block)
}

#[cfg(not(target_os = "uefi"))]
pub fn load_persisted() -> Option<BootControlBlock> {
    None
}

#[cfg(target_os = "uefi")]
pub fn persist_shadow(block: &BootControlBlock) {
    let runtime = match crate::boot::runtime_services() {
        Some(runtime) => runtime,
        None => return,
    };
    let attrs = VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS
        | VariableAttributes::NON_VOLATILE;
    let _ = runtime.set_variable(
        cstr16!("echOSBootControl"),
        &APPLIANCE_VENDOR_GUID,
        attrs,
        bytes_of(block),
    );
}

#[cfg(not(target_os = "uefi"))]
pub fn persist_shadow(_block: &BootControlBlock) {}

pub fn merge_seed(
    file_seed: Option<BootControlBlock>,
    persisted: Option<BootControlBlock>,
) -> BootControlBlock {
    match (
        file_seed.filter(|seed| seed.validate()),
        persisted.filter(|seed| seed.validate()),
    ) {
        (Some(file), Some(var)) => {
            let mut merged = if var.boot_epoch >= file.boot_epoch {
                var
            } else {
                file
            };
            merged.set_boot_flags(file.boot_flags());
            merged.with_crc()
        }
        (Some(file), None) => file,
        (None, Some(var)) => var,
        (None, None) => BootControlBlock::default(),
    }
}

fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe { core::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

fn crc32_of(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

fn boot_stage_label(stage: BootStage) -> &'static str {
    match stage {
        BootStage::LoaderEntry => "loader-entry",
        BootStage::BootControlLoaded => "boot-control-loaded",
        BootStage::KernelCoreReady => "kernel-core-ready",
        BootStage::StorageMounted => "storage-mounted",
        BootStage::NetworkReady => "network-ready",
        BootStage::DisplayReady => "display-ready",
        BootStage::DesktopReady => "desktop-ready",
        BootStage::AppBasketReady => "app-basket-ready",
        BootStage::Recovery => "recovery",
    }
}

fn slot_label(slot: SlotId) -> &'static str {
    match slot {
        SlotId::None => "none",
        SlotId::SystemA => "system_a",
        SlotId::SystemB => "system_b",
        SlotId::Recovery => "recovery",
    }
}
