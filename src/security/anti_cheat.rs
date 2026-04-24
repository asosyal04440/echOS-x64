//! Anti-cheat parity contract for kernel integrity and runtime attestation.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegritySignal {
    MeasuredBoot,
    CodeIntegrity,
    SignedDriverPolicy,
    RuntimeAntiTamper,
    KernelEventAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeViolation {
    UnsignedDriverLoad,
    KernelTextMutation,
    DebugAttachDenied,
    CallbackTamper,
    TelemetryGap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AntiCheatRuntimeEvent {
    pub seq: u64,
    pub tick: u64,
    pub violation: RuntimeViolation,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AntiCheatParitySnapshot {
    pub measured_boot: bool,
    pub code_integrity: bool,
    pub signed_driver_policy: bool,
    pub runtime_anti_tamper: bool,
    pub kernel_event_audit: bool,
    pub violation_count: u64,
    pub attestation_epoch: u64,
}

static MEASURED_BOOT: AtomicBool = AtomicBool::new(false);
static CODE_INTEGRITY: AtomicBool = AtomicBool::new(false);
static SIGNED_DRIVER_POLICY: AtomicBool = AtomicBool::new(false);
static RUNTIME_ANTI_TAMPER: AtomicBool = AtomicBool::new(false);
static KERNEL_EVENT_AUDIT: AtomicBool = AtomicBool::new(false);
static VIOLATION_COUNT: AtomicU64 = AtomicU64::new(0);
static ATTESTATION_EPOCH: AtomicU64 = AtomicU64::new(0);
static EVENT_SEQ: AtomicU64 = AtomicU64::new(1);
static LAST_TELEMETRY_GAP_SEQ: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    static ref EVENT_LOG: Mutex<Vec<AntiCheatRuntimeEvent>> = Mutex::new(Vec::new());
}

pub fn init() {
    let secure_boot_enabled =
        crate::security::is_nx_enabled() && crate::security::is_aslr_enabled();
    let code_integrity = crate::security::is_nx_enabled() && crate::security::is_wxorx_enabled();
    let runtime_tamper = crate::security::is_smep_enabled() && crate::security::is_smap_enabled();
    let audit = true;

    MEASURED_BOOT.store(secure_boot_enabled, Ordering::Release);
    CODE_INTEGRITY.store(code_integrity, Ordering::Release);
    SIGNED_DRIVER_POLICY.store(true, Ordering::Release);
    RUNTIME_ANTI_TAMPER.store(runtime_tamper, Ordering::Release);
    KERNEL_EVENT_AUDIT.store(audit, Ordering::Release);
    ATTESTATION_EPOCH.fetch_add(1, Ordering::AcqRel);
}

pub fn attest(signal: IntegritySignal) -> bool {
    match signal {
        IntegritySignal::MeasuredBoot => MEASURED_BOOT.load(Ordering::Acquire),
        IntegritySignal::CodeIntegrity => CODE_INTEGRITY.load(Ordering::Acquire),
        IntegritySignal::SignedDriverPolicy => SIGNED_DRIVER_POLICY.load(Ordering::Acquire),
        IntegritySignal::RuntimeAntiTamper => RUNTIME_ANTI_TAMPER.load(Ordering::Acquire),
        IntegritySignal::KernelEventAudit => KERNEL_EVENT_AUDIT.load(Ordering::Acquire),
    }
}

pub fn set_signed_driver_policy(enabled: bool) {
    SIGNED_DRIVER_POLICY.store(enabled, Ordering::Release);
    ATTESTATION_EPOCH.fetch_add(1, Ordering::AcqRel);
}

pub fn signed_driver_policy_enabled() -> bool {
    SIGNED_DRIVER_POLICY.load(Ordering::Acquire)
}

pub fn enforce_debug_attach(channel: &str) -> bool {
    if RUNTIME_ANTI_TAMPER.load(Ordering::Acquire) {
        record_runtime_violation(RuntimeViolation::DebugAttachDenied, channel);
        return false;
    }
    true
}

pub fn record_runtime_violation(violation: RuntimeViolation, detail: &str) {
    let seq = EVENT_SEQ.fetch_add(1, Ordering::AcqRel);
    let tick = crate::task::scheduler::get_ticks() as u64;
    EVENT_LOG.lock().push(AntiCheatRuntimeEvent {
        seq,
        tick,
        violation,
        detail: detail.into(),
    });
    VIOLATION_COUNT.fetch_add(1, Ordering::AcqRel);
}

pub fn events_since(last_seq: u64) -> Vec<AntiCheatRuntimeEvent> {
    EVENT_LOG
        .lock()
        .iter()
        .filter(|event| event.seq > last_seq)
        .cloned()
        .collect()
}

pub fn snapshot() -> AntiCheatParitySnapshot {
    AntiCheatParitySnapshot {
        measured_boot: MEASURED_BOOT.load(Ordering::Acquire),
        code_integrity: CODE_INTEGRITY.load(Ordering::Acquire),
        signed_driver_policy: SIGNED_DRIVER_POLICY.load(Ordering::Acquire),
        runtime_anti_tamper: RUNTIME_ANTI_TAMPER.load(Ordering::Acquire),
        kernel_event_audit: KERNEL_EVENT_AUDIT.load(Ordering::Acquire),
        violation_count: VIOLATION_COUNT.load(Ordering::Acquire),
        attestation_epoch: ATTESTATION_EPOCH.load(Ordering::Acquire),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttestationReasonCode {
    MeasuredBootMissing,
    CodeIntegrityMissing,
    SignedDriverPolicyMissing,
    RuntimeAntiTamperMissing,
    KernelAuditMissing,
    UnsignedDriverLoad,
    DebugAttachDenied,
    CallbackTamper,
    KernelTextMutation,
    TelemetryGap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AntiCheatAttestationReport {
    pub snapshot: AntiCheatParitySnapshot,
    pub reason_codes: Vec<AttestationReasonCode>,
    pub delta_events: Vec<AntiCheatRuntimeEvent>,
    pub next_seq: u64,
}

pub fn attestation_report(last_seq: u64) -> AntiCheatAttestationReport {
    let snapshot = snapshot();
    let mut delta_events = events_since(last_seq);
    let mut reason_codes = Vec::new();

    if !snapshot.measured_boot {
        reason_codes.push(AttestationReasonCode::MeasuredBootMissing);
    }
    if !snapshot.code_integrity {
        reason_codes.push(AttestationReasonCode::CodeIntegrityMissing);
    }
    if !snapshot.signed_driver_policy {
        reason_codes.push(AttestationReasonCode::SignedDriverPolicyMissing);
    }
    if !snapshot.runtime_anti_tamper {
        reason_codes.push(AttestationReasonCode::RuntimeAntiTamperMissing);
    }
    if !snapshot.kernel_event_audit {
        reason_codes.push(AttestationReasonCode::KernelAuditMissing);
    }

    if last_seq != 0 && delta_events.is_empty() && snapshot.kernel_event_audit {
        let observed = LAST_TELEMETRY_GAP_SEQ.load(Ordering::Acquire);
        if observed != last_seq {
            record_runtime_violation(
                RuntimeViolation::TelemetryGap,
                "no attestation delta events",
            );
            LAST_TELEMETRY_GAP_SEQ.store(last_seq, Ordering::Release);
            delta_events = events_since(last_seq);
        }
    }

    for event in delta_events.iter() {
        let reason = match event.violation {
            RuntimeViolation::UnsignedDriverLoad => AttestationReasonCode::UnsignedDriverLoad,
            RuntimeViolation::KernelTextMutation => AttestationReasonCode::KernelTextMutation,
            RuntimeViolation::DebugAttachDenied => AttestationReasonCode::DebugAttachDenied,
            RuntimeViolation::CallbackTamper => AttestationReasonCode::CallbackTamper,
            RuntimeViolation::TelemetryGap => AttestationReasonCode::TelemetryGap,
        };
        if !reason_codes.contains(&reason) {
            reason_codes.push(reason);
        }
    }

    AntiCheatAttestationReport {
        snapshot,
        reason_codes,
        next_seq: EVENT_SEQ.load(Ordering::Acquire),
        delta_events,
    }
}
