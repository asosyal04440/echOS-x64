use crate::boot::appliance::{self, SlotId};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use p256::ecdsa::{
    Signature as P256Signature, SigningKey as P256SigningKey, VerifyingKey as P256VerifyingKey,
};
use sha2::{Digest, Sha256};
use signature::{Signer, Verifier};

const INDEX_MAGIC: [u8; 8] = *b"echUPD01";
const INDEX_VERSION: u16 = 1;
const SIGNATURE_LEN: usize = 64;
const BLOCK_SIZE: usize = 512;
const LIVE_REVOCATION_FEED_ID: &str = "package-revocations";
const LAST_REPORT_PATH: &str = "/config/update/last-report.txt";
const LAST_STAGE_PATH: &str = "/config/update/last-stage.txt";
const SLOT_STAGE_ROOT: &str = "/config/update/slot-stage";
const SLOT_STAGE_JOURNAL_PATH: &str = "/config/update/slot-stage/journal.txt";
const SMOKE_REQUEST_PATH: &str = "/config/update/smoke/request.txt";

const ENGINEERING_UPDATE_SIGNING_SEED: [u8; 32] = [
    0x65, 0x63, 0x68, 0x4f, 0x53, 0x2d, 0x75, 0x70, 0x64, 0x61, 0x74, 0x65, 0x2d, 0x65, 0x6e, 0x67,
    0x2d, 0x72, 0x6f, 0x6f, 0x74, 0x2d, 0x76, 0x31, 0x2d, 0x69, 0x64, 0x78, 0x2d, 0x30, 0x31, 0x21,
];
const STABLE_UPDATE_SIGNING_SEED: [u8; 32] = [
    0x65, 0x63, 0x68, 0x4f, 0x53, 0x2d, 0x75, 0x70, 0x64, 0x61, 0x74, 0x65, 0x2d, 0x73, 0x74, 0x61,
    0x62, 0x6c, 0x65, 0x2d, 0x72, 0x6f, 0x6f, 0x74, 0x2d, 0x76, 0x31, 0x2d, 0x69, 0x64, 0x78, 0x21,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateArtifactKind {
    PlatformImage,
    ServiceBundle,
    UserBundle,
    SeedCatalog,
    RevocationFeed,
}

impl UpdateArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlatformImage => "platform",
            Self::ServiceBundle => "service",
            Self::UserBundle => "user",
            Self::SeedCatalog => "seed",
            Self::RevocationFeed => "revocation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "platform" => Some(Self::PlatformImage),
            "service" => Some(Self::ServiceBundle),
            "user" => Some(Self::UserBundle),
            "seed" => Some(Self::SeedCatalog),
            "revocation" => Some(Self::RevocationFeed),
            _ => None,
        }
    }

    const fn encode_tag(self) -> u8 {
        match self {
            Self::PlatformImage => 1,
            Self::ServiceBundle => 2,
            Self::UserBundle => 3,
            Self::SeedCatalog => 4,
            Self::RevocationFeed => 5,
        }
    }

    fn decode_tag(value: u8) -> Result<Self, UpdateError> {
        match value {
            1 => Ok(Self::PlatformImage),
            2 => Ok(Self::ServiceBundle),
            3 => Ok(Self::UserBundle),
            4 => Ok(Self::SeedCatalog),
            5 => Ok(Self::RevocationFeed),
            _ => Err(UpdateError::InvalidFormat),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateDelivery {
    LiveStore,
    InactiveSlot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateArtifact {
    pub id: String,
    pub version: String,
    pub source: String,
    pub kind: UpdateArtifactKind,
    pub bytes: u64,
    pub digest: [u8; 32],
    pub reboot_required: bool,
    pub package_id: Option<String>,
    pub requested_slot: Option<SlotId>,
}

impl UpdateArtifact {
    pub fn for_platform_image(
        id: &str,
        version: &str,
        source: &str,
        bytes: u64,
        digest: [u8; 32],
    ) -> Self {
        Self {
            id: id.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            kind: UpdateArtifactKind::PlatformImage,
            bytes,
            digest,
            reboot_required: true,
            package_id: None,
            requested_slot: None,
        }
    }

    pub fn for_service_bundle(
        package_id: &str,
        version: &str,
        source: &str,
        bytes: u64,
        digest: [u8; 32],
        reboot_required: bool,
    ) -> Self {
        Self {
            id: package_id.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            kind: UpdateArtifactKind::ServiceBundle,
            bytes,
            digest,
            reboot_required,
            package_id: Some(package_id.to_string()),
            requested_slot: None,
        }
    }

    pub fn for_user_bundle(
        package_id: &str,
        version: &str,
        source: &str,
        bytes: u64,
        digest: [u8; 32],
    ) -> Self {
        Self {
            id: package_id.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            kind: UpdateArtifactKind::UserBundle,
            bytes,
            digest,
            reboot_required: false,
            package_id: Some(package_id.to_string()),
            requested_slot: None,
        }
    }

    pub fn for_seed_catalog(
        id: &str,
        version: &str,
        source: &str,
        bytes: u64,
        digest: [u8; 32],
    ) -> Self {
        Self {
            id: id.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            kind: UpdateArtifactKind::SeedCatalog,
            bytes,
            digest,
            reboot_required: false,
            package_id: None,
            requested_slot: None,
        }
    }

    pub fn for_revocation_feed(version: &str, source: &str, bytes: u64, digest: [u8; 32]) -> Self {
        Self {
            id: LIVE_REVOCATION_FEED_ID.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            kind: UpdateArtifactKind::RevocationFeed,
            bytes,
            digest,
            reboot_required: false,
            package_id: None,
            requested_slot: None,
        }
    }

    fn requires_slot_staging(&self) -> bool {
        matches!(self.kind, UpdateArtifactKind::PlatformImage)
            || matches!(self.kind, UpdateArtifactKind::ServiceBundle if self.reboot_required)
    }

    fn package_id_or_err(&self) -> Result<&str, UpdatePlanError> {
        self.package_id
            .as_deref()
            .ok_or(UpdatePlanError::MissingPackageIdentity {
                artifact_id: self.id.clone(),
                kind: self.kind,
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOperationKind {
    PublishRevocationFeed,
    InstallSeedCatalog,
    StagePlatformImage { target_slot: SlotId },
    StageServiceBundle { target_slot: SlotId },
    InstallServiceBundle { reboot_required: bool },
    InstallUserBundle,
    ArmBootControl { target_slot: SlotId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOperation {
    pub artifact_id: String,
    pub kind: UpdateOperationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePlanClass {
    NoOp,
    LiveOnly,
    RebootRequired,
    Mixed,
}

impl UpdatePlanClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoOp => "noop",
            Self::LiveOnly => "live-only",
            Self::RebootRequired => "reboot-required",
            Self::Mixed => "mixed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "noop" => Some(Self::NoOp),
            "live-only" => Some(Self::LiveOnly),
            "reboot-required" => Some(Self::RebootRequired),
            "mixed" => Some(Self::Mixed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePlan {
    pub class: UpdatePlanClass,
    pub target_slot: Option<SlotId>,
    pub requires_reboot: bool,
    pub total_download_bytes: u64,
    pub affected_packages: Vec<String>,
    pub operations: Vec<UpdateOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdatePlanError {
    MissingPackageIdentity {
        artifact_id: String,
        kind: UpdateArtifactKind,
    },
    ConflictingTargetSlots {
        existing: SlotId,
        requested: SlotId,
    },
    PlatformTargetsActiveSlot {
        artifact_id: String,
        active_slot: SlotId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateIndexSignatureProfile {
    Engineering,
    Stable,
}

impl UpdateIndexSignatureProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Engineering => "engineering",
            Self::Stable => "stable",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "engineering" => Some(Self::Engineering),
            "stable" => Some(Self::Stable),
            _ => None,
        }
    }

    const fn signer_key_id(self) -> &'static str {
        match self {
            Self::Engineering => "update-engineering-root-v1",
            Self::Stable => "update-stable-root-v1",
        }
    }

    const fn signing_seed(self) -> &'static [u8; 32] {
        match self {
            Self::Engineering => &ENGINEERING_UPDATE_SIGNING_SEED,
            Self::Stable => &STABLE_UPDATE_SIGNING_SEED,
        }
    }

    const fn encode_tag(self) -> u8 {
        match self {
            Self::Engineering => 1,
            Self::Stable => 2,
        }
    }

    fn decode_tag(value: u8) -> Result<Self, UpdateError> {
        match value {
            1 => Ok(Self::Engineering),
            2 => Ok(Self::Stable),
            _ => Err(UpdateError::InvalidFormat),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateIndex {
    pub channel: String,
    pub release: String,
    pub published_epoch: u64,
    pub artifacts: Vec<UpdateArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateIndexSignatureMetadata {
    pub signer_key_id: String,
    pub profile: UpdateIndexSignatureProfile,
    pub manifest_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInspection {
    pub index: UpdateIndex,
    pub signature: UpdateIndexSignatureMetadata,
    pub plan: UpdatePlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateApplyState {
    Staged,
    Applied,
    Failed,
}

impl UpdateApplyState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "staged" => Some(Self::Staged),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateApplyReport {
    pub state: UpdateApplyState,
    pub channel: String,
    pub release: String,
    pub signer_key_id: String,
    pub plan_class: UpdatePlanClass,
    pub target_slot: Option<SlotId>,
    pub requires_reboot: bool,
    pub total_download_bytes: u64,
    pub applied_artifacts: Vec<String>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StagedServiceBundle {
    artifact_id: String,
    package_id: String,
    version: String,
    staged_path: String,
    bytes: u64,
    digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SlotStageJournal {
    channel: String,
    release: String,
    signer_key_id: String,
    target_slot: SlotId,
    bundles: Vec<StagedServiceBundle>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    InvalidMagic,
    UnsupportedVersion,
    InvalidFormat,
    InvalidSignature,
    InvalidManifestDigest,
    ManifestTooLarge,
    StoreIo,
    NetworkUnavailable,
    NoBlockDevice,
    TargetPartitionNotFound {
        label: String,
    },
    ArtifactTooLarge {
        artifact_id: String,
        available_bytes: u64,
        image_bytes: u64,
    },
    ArtifactDigestMismatch {
        artifact_id: String,
    },
    Package(String),
    Plan(UpdatePlanError),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "update index magic gecersiz"),
            Self::UnsupportedVersion => write!(f, "desteklenmeyen update index surumu"),
            Self::InvalidFormat => write!(f, "update index formati gecersiz"),
            Self::InvalidSignature => write!(f, "update index imzasi gecersiz"),
            Self::InvalidManifestDigest => write!(f, "update manifest digest uyusmuyor"),
            Self::ManifestTooLarge => write!(f, "update manifest boyutu desteklenen siniri asti"),
            Self::StoreIo => write!(f, "store I/O hatasi"),
            Self::NetworkUnavailable => write!(f, "ag download yolu kullanilamiyor"),
            Self::NoBlockDevice => write!(f, "blok aygit secilemedi"),
            Self::TargetPartitionNotFound { label } => {
                write!(f, "hedef partition bulunamadi: {}", label)
            }
            Self::ArtifactTooLarge {
                artifact_id,
                available_bytes,
                image_bytes,
            } => write!(
                f,
                "artifact hedef partitiona sigmiyor: {} ({} > {})",
                artifact_id, image_bytes, available_bytes
            ),
            Self::ArtifactDigestMismatch { artifact_id } => {
                write!(f, "artifact digest uyusmadi: {}", artifact_id)
            }
            Self::Package(message) => write!(f, "paket/update apply hatasi: {}", message),
            Self::Plan(err) => write!(f, "planlama hatasi: {}", err),
        }
    }
}

impl fmt::Display for UpdatePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPackageIdentity { artifact_id, kind } => write!(
                f,
                "package kimligi eksik: {} ({})",
                artifact_id,
                kind.as_str()
            ),
            Self::ConflictingTargetSlots {
                existing,
                requested,
            } => write!(
                f,
                "hedef slot cakismasi: mevcut={} yeni={}",
                slot_to_str(*existing),
                slot_to_str(*requested)
            ),
            Self::PlatformTargetsActiveSlot {
                artifact_id,
                active_slot,
            } => write!(
                f,
                "platform artifact aktif slota hedeflenmis: {} ({})",
                artifact_id,
                slot_to_str(*active_slot)
            ),
        }
    }
}

impl From<UpdatePlanError> for UpdateError {
    fn from(value: UpdatePlanError) -> Self {
        Self::Plan(value)
    }
}

fn classify_plan(operations: &[UpdateOperation], requires_reboot: bool) -> UpdatePlanClass {
    if operations.is_empty() {
        return UpdatePlanClass::NoOp;
    }
    let has_live = operations.iter().any(|operation| {
        matches!(
            operation.kind,
            UpdateOperationKind::InstallUserBundle
                | UpdateOperationKind::InstallServiceBundle { .. }
                | UpdateOperationKind::PublishRevocationFeed
                | UpdateOperationKind::InstallSeedCatalog
        )
    });
    let has_slot_lane = operations.iter().any(|operation| {
        matches!(
            operation.kind,
            UpdateOperationKind::StagePlatformImage { .. }
                | UpdateOperationKind::StageServiceBundle { .. }
                | UpdateOperationKind::ArmBootControl { .. }
        )
    });
    match (has_live, has_slot_lane, requires_reboot) {
        (false, false, _) => UpdatePlanClass::NoOp,
        (true, false, false) => UpdatePlanClass::LiveOnly,
        (true, false, true) | (false, true, true) => UpdatePlanClass::RebootRequired,
        (true, true, _) => UpdatePlanClass::Mixed,
        (false, true, false) => UpdatePlanClass::RebootRequired,
    }
}

fn push_affected_package(packages: &mut Vec<String>, package_id: &str) {
    if packages.iter().any(|existing| existing == package_id) {
        return;
    }
    packages.push(package_id.to_string());
}

fn resolve_target_slot(
    active_slot: SlotId,
    current: Option<SlotId>,
    artifact: &UpdateArtifact,
) -> Result<Option<SlotId>, UpdatePlanError> {
    if !artifact.requires_slot_staging() {
        return Ok(current);
    }
    let requested = artifact
        .requested_slot
        .unwrap_or_else(|| active_slot.inactive_pair());
    if requested == active_slot {
        return Err(UpdatePlanError::PlatformTargetsActiveSlot {
            artifact_id: artifact.id.clone(),
            active_slot,
        });
    }
    match current {
        Some(existing) if existing != requested => Err(UpdatePlanError::ConflictingTargetSlots {
            existing,
            requested,
        }),
        _ => Ok(Some(requested)),
    }
}

pub fn plan_update(
    active_slot: SlotId,
    artifacts: &[UpdateArtifact],
) -> Result<UpdatePlan, UpdatePlanError> {
    let mut target_slot = None;
    for artifact in artifacts {
        target_slot = resolve_target_slot(active_slot, target_slot, artifact)?;
    }

    let mut operations = Vec::new();
    let mut affected_packages = Vec::new();
    let mut total_download_bytes = 0u64;
    let mut requires_reboot = false;

    for artifact in artifacts {
        total_download_bytes = total_download_bytes.saturating_add(artifact.bytes);
        match artifact.kind {
            UpdateArtifactKind::RevocationFeed => operations.push(UpdateOperation {
                artifact_id: artifact.id.clone(),
                kind: UpdateOperationKind::PublishRevocationFeed,
            }),
            UpdateArtifactKind::SeedCatalog => operations.push(UpdateOperation {
                artifact_id: artifact.id.clone(),
                kind: UpdateOperationKind::InstallSeedCatalog,
            }),
            UpdateArtifactKind::PlatformImage => {
                let slot = target_slot.expect("platform image requires target slot");
                requires_reboot = true;
                operations.push(UpdateOperation {
                    artifact_id: artifact.id.clone(),
                    kind: UpdateOperationKind::StagePlatformImage { target_slot: slot },
                });
            }
            UpdateArtifactKind::ServiceBundle => {
                let package_id = artifact.package_id_or_err()?;
                push_affected_package(&mut affected_packages, package_id);
                if artifact.reboot_required {
                    let slot =
                        target_slot.expect("reboot-required service bundle requires target slot");
                    requires_reboot = true;
                    operations.push(UpdateOperation {
                        artifact_id: artifact.id.clone(),
                        kind: UpdateOperationKind::StageServiceBundle { target_slot: slot },
                    });
                } else {
                    operations.push(UpdateOperation {
                        artifact_id: artifact.id.clone(),
                        kind: UpdateOperationKind::InstallServiceBundle {
                            reboot_required: false,
                        },
                    });
                }
            }
            UpdateArtifactKind::UserBundle => {
                let package_id = artifact.package_id_or_err()?;
                push_affected_package(&mut affected_packages, package_id);
                operations.push(UpdateOperation {
                    artifact_id: artifact.id.clone(),
                    kind: UpdateOperationKind::InstallUserBundle,
                });
            }
        }
    }

    if let Some(slot) = target_slot {
        operations.push(UpdateOperation {
            artifact_id: String::from("boot-control"),
            kind: UpdateOperationKind::ArmBootControl { target_slot: slot },
        });
    }

    let class = classify_plan(&operations, requires_reboot);
    Ok(UpdatePlan {
        class,
        target_slot,
        requires_reboot,
        total_download_bytes,
        affected_packages,
        operations,
    })
}

pub fn build_signed_index(
    index: &UpdateIndex,
    profile: UpdateIndexSignatureProfile,
) -> Result<Vec<u8>, UpdateError> {
    let unsigned = encode_unsigned_index(index)?;
    let manifest_digest = sha256_array(&unsigned);
    let metadata = UpdateIndexSignatureMetadata {
        signer_key_id: profile.signer_key_id().to_string(),
        profile,
        manifest_digest,
    };
    let mut out = unsigned.clone();
    let metadata_bytes = encode_signature_metadata(&metadata)?;
    out.extend_from_slice(&(metadata_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&metadata_bytes);
    out.extend_from_slice(&sign_signed_index_bytes(&unsigned, profile)?);
    Ok(out)
}

pub fn inspect_signed_index(
    bytes: &[u8],
    active_slot: SlotId,
) -> Result<UpdateInspection, UpdateError> {
    let (index, unsigned_len) = decode_unsigned_index(bytes)?;
    if bytes.len() < unsigned_len + 2 + SIGNATURE_LEN {
        return Err(UpdateError::InvalidFormat);
    }
    let metadata_len = u16::from_le_bytes(
        bytes[unsigned_len..unsigned_len + 2]
            .try_into()
            .map_err(|_| UpdateError::InvalidFormat)?,
    ) as usize;
    let metadata_start = unsigned_len + 2;
    let metadata_end = metadata_start + metadata_len;
    if bytes.len() < metadata_end + SIGNATURE_LEN {
        return Err(UpdateError::InvalidFormat);
    }
    if metadata_end + SIGNATURE_LEN != bytes.len() {
        return Err(UpdateError::InvalidFormat);
    }
    let metadata = decode_signature_metadata(&bytes[metadata_start..metadata_end])?;
    let unsigned = &bytes[..unsigned_len];
    if metadata.manifest_digest != sha256_array(unsigned) {
        return Err(UpdateError::InvalidManifestDigest);
    }
    let signature: [u8; 64] = bytes[metadata_end..]
        .try_into()
        .map_err(|_| UpdateError::InvalidSignature)?;
    if metadata.signer_key_id != metadata.profile.signer_key_id() {
        return Err(UpdateError::InvalidSignature);
    }
    verify_signed_index_bytes(unsigned, metadata.profile, &signature)?;
    let plan = plan_update(active_slot, &index.artifacts)?;
    Ok(UpdateInspection {
        index,
        signature: metadata,
        plan,
    })
}

pub fn inspect_update_source(locator: &str) -> Result<UpdateInspection, UpdateError> {
    let bytes = read_locator(locator)?;
    inspect_signed_index(&bytes, appliance::current_active_slot())
}

pub fn apply_update_source(locator: &str) -> Result<UpdateApplyReport, UpdateError> {
    let bytes = read_locator(locator)?;
    let inspection = inspect_signed_index(&bytes, appliance::current_active_slot())?;
    apply_inspection(&inspection)
}

pub fn last_apply_report() -> Option<UpdateApplyReport> {
    let response = crate::ipc::request_store_sync(
        0,
        crate::services::StoreCommand::ReadFile {
            path: LAST_REPORT_PATH.to_string(),
        },
    )?;
    match response {
        crate::services::StoreResponse::FileData(bytes) => UpdateApplyReport::decode(&bytes).ok(),
        _ => None,
    }
}

pub fn smoke_request_locator() -> Option<String> {
    let bytes = read_store_file(SMOKE_REQUEST_PATH).ok()?;
    let locator = String::from_utf8_lossy(&bytes).trim().to_string();
    if locator.is_empty() {
        None
    } else {
        Some(locator)
    }
}

pub fn clear_smoke_request() -> Result<(), UpdateError> {
    delete_store_path(SMOKE_REQUEST_PATH)
}

pub fn apply_staged_boot_updates() -> Result<(), UpdateError> {
    let journal_bytes = match read_store_file(SLOT_STAGE_JOURNAL_PATH) {
        Ok(bytes) => bytes,
        Err(UpdateError::StoreIo) => return Ok(()),
        Err(err) => return Err(err),
    };
    let journal = SlotStageJournal::decode(&journal_bytes)?;
    if journal.target_slot != appliance::current_active_slot() {
        let err = UpdateError::InvalidFormat;
        persist_boot_failure(&journal, &err)?;
        crate::serial_println!(
            "[UPDATE] staged boot apply fail release={} target_slot={} err={}",
            journal.release,
            slot_to_str(journal.target_slot),
            err
        );
        return Err(err);
    }

    for bundle in &journal.bundles {
        let bytes = read_store_file(bundle.staged_path.as_str())?;
        if bundle.bytes != bytes.len() as u64 || bundle.digest != sha256_array(&bytes) {
            let err = UpdateError::ArtifactDigestMismatch {
                artifact_id: bundle.artifact_id.clone(),
            };
            persist_boot_failure(&journal, &err)?;
            crate::serial_println!(
                "[UPDATE] staged boot apply fail release={} target_slot={} err={}",
                journal.release,
                slot_to_str(journal.target_slot),
                err
            );
            return Err(err);
        }
        if let Err(err) = crate::security::package::install_bundle(&bytes) {
            let err = UpdateError::Package(err.to_string());
            persist_boot_failure(&journal, &err)?;
            crate::serial_println!(
                "[UPDATE] staged boot apply fail release={} target_slot={} err={}",
                journal.release,
                slot_to_str(journal.target_slot),
                err
            );
            return Err(err);
        }
    }

    for bundle in &journal.bundles {
        let _ = delete_store_path(bundle.staged_path.as_str());
    }
    let _ = delete_store_path(SLOT_STAGE_JOURNAL_PATH);
    persist_boot_success(&journal)?;
    crate::serial_println!(
        "[UPDATE] staged boot apply ok release={} target_slot={} bundles={}",
        journal.release,
        slot_to_str(journal.target_slot),
        journal.bundles.len()
    );
    Ok(())
}

fn apply_inspection(inspection: &UpdateInspection) -> Result<UpdateApplyReport, UpdateError> {
    let mut applied_artifacts = Vec::new();
    let mut staged_bundles = Vec::new();
    for operation in &inspection.plan.operations {
        match operation.kind {
            UpdateOperationKind::ArmBootControl { target_slot } => {
                if !staged_bundles.is_empty() {
                    let journal = SlotStageJournal {
                        channel: inspection.index.channel.clone(),
                        release: inspection.index.release.clone(),
                        signer_key_id: inspection.signature.signer_key_id.clone(),
                        target_slot,
                        bundles: staged_bundles.clone(),
                    };
                    persist_slot_stage_journal(target_slot, &journal)?;
                }
                appliance::begin_update(target_slot);
                persist_stage_note(
                    inspection.index.release.as_str(),
                    target_slot,
                    &applied_artifacts,
                )?;
            }
            _ => {
                let artifact = inspection
                    .index
                    .artifacts
                    .iter()
                    .find(|candidate| candidate.id == operation.artifact_id)
                    .ok_or(UpdateError::InvalidFormat)?;
                let bytes = read_locator(artifact.source.as_str())?;
                validate_artifact_bytes(artifact, &bytes)?;
                match operation.kind {
                    UpdateOperationKind::PublishRevocationFeed => {
                        crate::security::package::publish_revocation_feed(&bytes)
                            .map_err(|err| UpdateError::Package(err.to_string()))?;
                    }
                    UpdateOperationKind::InstallSeedCatalog => {
                        install_seed_catalog(artifact, &bytes)?;
                    }
                    UpdateOperationKind::StagePlatformImage { target_slot } => {
                        stage_platform_image(artifact, target_slot, &bytes)?;
                    }
                    UpdateOperationKind::StageServiceBundle { target_slot } => {
                        staged_bundles.push(stage_service_bundle(
                            inspection,
                            artifact,
                            target_slot,
                            &bytes,
                        )?);
                    }
                    UpdateOperationKind::InstallServiceBundle { .. }
                    | UpdateOperationKind::InstallUserBundle => {
                        crate::security::package::install_bundle(&bytes)
                            .map_err(|err| UpdateError::Package(err.to_string()))?;
                    }
                    UpdateOperationKind::ArmBootControl { .. } => {}
                }
                applied_artifacts.push(artifact.id.clone());
            }
        }
    }

    let report = UpdateApplyReport {
        state: if inspection.plan.requires_reboot {
            UpdateApplyState::Staged
        } else {
            UpdateApplyState::Applied
        },
        channel: inspection.index.channel.clone(),
        release: inspection.index.release.clone(),
        signer_key_id: inspection.signature.signer_key_id.clone(),
        plan_class: inspection.plan.class,
        target_slot: inspection.plan.target_slot,
        requires_reboot: inspection.plan.requires_reboot,
        total_download_bytes: inspection.plan.total_download_bytes,
        applied_artifacts,
        failure: None,
    };
    persist_report(&report)?;
    Ok(report)
}

fn validate_artifact_bytes(artifact: &UpdateArtifact, bytes: &[u8]) -> Result<(), UpdateError> {
    if artifact.bytes != bytes.len() as u64 || artifact.digest != sha256_array(bytes) {
        return Err(UpdateError::ArtifactDigestMismatch {
            artifact_id: artifact.id.clone(),
        });
    }
    Ok(())
}

fn install_seed_catalog(artifact: &UpdateArtifact, bytes: &[u8]) -> Result<(), UpdateError> {
    crate::security::package::inspect_signed_bundle(bytes)
        .map_err(|err| UpdateError::Package(err.to_string()))?;
    let file_name = format!("{}-{}.bhd", artifact.id, artifact.version);
    let path = format!("/seed/apps/{}", file_name);
    write_store_file(path.as_str(), bytes)?;
    let _ = crate::security::seed_store::refresh_seed_catalog();
    Ok(())
}

fn stage_service_bundle(
    inspection: &UpdateInspection,
    artifact: &UpdateArtifact,
    target_slot: SlotId,
    bytes: &[u8],
) -> Result<StagedServiceBundle, UpdateError> {
    let package_id = artifact.package_id_or_err()?;
    let stage_dir = slot_stage_directory(inspection);
    let staged_path = format!(
        "{}/{}-{}-{}.bhd",
        stage_dir,
        sanitize_path_component(package_id),
        sanitize_path_component(artifact.version.as_str()),
        short_hex(&artifact.digest)
    );
    crate::fs::f2fs::write_new_f2fs_file_on_partition(
        target_slot.system_partition_label(),
        staged_path.as_str(),
        bytes,
    )
    .map_err(|_| UpdateError::StoreIo)?;
    Ok(StagedServiceBundle {
        artifact_id: artifact.id.clone(),
        package_id: package_id.to_string(),
        version: artifact.version.clone(),
        staged_path,
        bytes: artifact.bytes,
        digest: artifact.digest,
    })
}

fn persist_slot_stage_journal(
    target_slot: SlotId,
    journal: &SlotStageJournal,
) -> Result<(), UpdateError> {
    crate::fs::f2fs::write_new_f2fs_file_on_partition(
        target_slot.system_partition_label(),
        SLOT_STAGE_JOURNAL_PATH,
        &journal.encode(),
    )
    .map_err(|_| UpdateError::StoreIo)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PartitionSpan {
    first_lba: u32,
    sector_count: u64,
}

fn stage_platform_image(
    artifact: &UpdateArtifact,
    target_slot: SlotId,
    image: &[u8],
) -> Result<(), UpdateError> {
    let mut drive =
        crate::drivers::linux::select_block_device().map_err(|_| UpdateError::NoBlockDevice)?;
    let partition = find_partition_by_label(&mut *drive, target_slot.system_partition_label())
        .ok_or_else(|| UpdateError::TargetPartitionNotFound {
            label: target_slot.system_partition_label().to_string(),
        })?;
    let available_bytes = partition.sector_count.saturating_mul(BLOCK_SIZE as u64);
    if image.len() as u64 > available_bytes {
        return Err(UpdateError::ArtifactTooLarge {
            artifact_id: artifact.id.clone(),
            available_bytes,
            image_bytes: image.len() as u64,
        });
    }

    let mut next_lba = partition.first_lba as u64;
    let mut cursor = 0usize;
    while cursor < image.len() {
        let remaining = image.len() - cursor;
        let batch_sectors = remaining.div_ceil(BLOCK_SIZE).min(u8::MAX as usize);
        let batch_len = batch_sectors * BLOCK_SIZE;
        let end = (cursor + batch_len).min(image.len());
        let mut sector_batch = vec![0u8; batch_len];
        sector_batch[..end - cursor].copy_from_slice(&image[cursor..end]);
        drive
            .write_sectors(next_lba.min(u32::MAX as u64) as u32, &sector_batch)
            .map_err(|_| UpdateError::StoreIo)?;
        next_lba = next_lba.saturating_add(batch_sectors as u64);
        cursor = end;
    }

    let stage_note = format!(
        "release={}\nslot={}\ndigest={}\nbytes={}\n",
        artifact.version,
        slot_to_str(target_slot),
        hex_digest(&artifact.digest),
        artifact.bytes
    );
    write_store_file(LAST_STAGE_PATH, stage_note.as_bytes())?;
    Ok(())
}

fn find_partition_by_label(
    drive: &mut dyn crate::drivers::linux::BlockDevice,
    label: &str,
) -> Option<PartitionSpan> {
    const GPT_HEADER_LBA: u32 = 1;
    const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
    const GPT_ENTRY_TYPE_GUID_OFFSET: usize = 0;
    const GPT_ENTRY_FIRST_LBA_OFFSET: usize = 32;
    const GPT_ENTRY_LAST_LBA_OFFSET: usize = 40;
    const GPT_ENTRY_NAME_OFFSET: usize = 56;

    let header = drive.read_sectors(GPT_HEADER_LBA, 1);
    if header.len() < 92 || &header[0..8] != GPT_SIGNATURE {
        return None;
    }
    let entry_lba = u64::from_le_bytes(header[72..80].try_into().ok()?);
    let entry_count = u32::from_le_bytes(header[80..84].try_into().ok()?);
    let entry_size = u32::from_le_bytes(header[84..88].try_into().ok()?);
    if entry_count == 0 || entry_size < 128 {
        return None;
    }

    let sectors = ((entry_count as usize * entry_size as usize) + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let mut entries = Vec::with_capacity(sectors * BLOCK_SIZE);
    let mut next_lba = entry_lba;
    let mut remaining = sectors;
    while remaining > 0 {
        let batch = remaining.min(u8::MAX as usize) as u8;
        entries.extend_from_slice(&drive.read_sectors(next_lba.min(u32::MAX as u64) as u32, batch));
        next_lba = next_lba.saturating_add(batch as u64);
        remaining -= batch as usize;
    }

    for index in 0..entry_count as usize {
        let offset = index * entry_size as usize;
        if offset + entry_size as usize > entries.len() {
            break;
        }
        let entry = &entries[offset..offset + entry_size as usize];
        if entry[GPT_ENTRY_TYPE_GUID_OFFSET..GPT_ENTRY_TYPE_GUID_OFFSET + 16]
            .iter()
            .all(|byte| *byte == 0)
        {
            continue;
        }
        let first_lba = u64::from_le_bytes(
            entry[GPT_ENTRY_FIRST_LBA_OFFSET..GPT_ENTRY_FIRST_LBA_OFFSET + 8]
                .try_into()
                .ok()?,
        );
        let last_lba = u64::from_le_bytes(
            entry[GPT_ENTRY_LAST_LBA_OFFSET..GPT_ENTRY_LAST_LBA_OFFSET + 8]
                .try_into()
                .ok()?,
        );
        let name = parse_gpt_name(&entry[GPT_ENTRY_NAME_OFFSET..GPT_ENTRY_NAME_OFFSET + 72]);
        if name == label {
            return Some(PartitionSpan {
                first_lba: first_lba.min(u32::MAX as u64) as u32,
                sector_count: last_lba.saturating_sub(first_lba).saturating_add(1),
            });
        }
    }
    None
}

fn parse_gpt_name(raw: &[u8]) -> String {
    let mut utf16 = Vec::new();
    for chunk in raw.chunks_exact(2) {
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        if code == 0 {
            break;
        }
        utf16.push(code);
    }
    String::from_utf16_lossy(&utf16)
}

fn read_locator(locator: &str) -> Result<Vec<u8>, UpdateError> {
    if locator.starts_with("http://") || locator.starts_with("https://") {
        use crate::runtime_layer::service_control::{NetworkBrokerCommand, NetworkBrokerResponse};

        let Some(response) = crate::ipc::request_network_broker_sync(
            0,
            NetworkBrokerCommand::Download(locator.to_string()),
        ) else {
            return Err(UpdateError::NetworkUnavailable);
        };
        return match response {
            NetworkBrokerResponse::Payload(bytes) => Ok(bytes),
            NetworkBrokerResponse::Error(_) => Err(UpdateError::NetworkUnavailable),
        };
    }
    read_store_file(locator)
}

fn read_store_file(path: &str) -> Result<Vec<u8>, UpdateError> {
    let response = crate::ipc::request_store_sync(
        0,
        crate::services::StoreCommand::ReadFile {
            path: path.to_string(),
        },
    )
    .ok_or(UpdateError::StoreIo)?;
    match response {
        crate::services::StoreResponse::FileData(bytes) => Ok(bytes),
        _ => Err(UpdateError::StoreIo),
    }
}

fn write_store_file(path: &str, bytes: &[u8]) -> Result<(), UpdateError> {
    let response = crate::ipc::request_store_sync(
        0,
        crate::services::StoreCommand::WriteFile {
            path: path.to_string(),
            data: bytes.to_vec(),
        },
    )
    .ok_or(UpdateError::StoreIo)?;
    match response {
        crate::services::StoreResponse::Success => Ok(()),
        _ => Err(UpdateError::StoreIo),
    }
}

fn delete_store_path(path: &str) -> Result<(), UpdateError> {
    let response = crate::ipc::request_store_sync(
        0,
        crate::services::StoreCommand::DeleteFile {
            path: path.to_string(),
        },
    )
    .ok_or(UpdateError::StoreIo)?;
    match response {
        crate::services::StoreResponse::Success => Ok(()),
        _ => Err(UpdateError::StoreIo),
    }
}

fn persist_report(report: &UpdateApplyReport) -> Result<(), UpdateError> {
    write_store_file(LAST_REPORT_PATH, &report.encode())
}

fn persist_stage_note(
    release: &str,
    target_slot: SlotId,
    artifacts: &[String],
) -> Result<(), UpdateError> {
    let mut out = format!(
        "release={}\nslot={}\nartifacts={}\n",
        release,
        slot_to_str(target_slot),
        artifacts.join(",")
    );
    if !out.ends_with('\n') {
        out.push('\n');
    }
    write_store_file(LAST_STAGE_PATH, out.as_bytes())
}

fn slot_stage_directory(inspection: &UpdateInspection) -> String {
    format!(
        "{}/{}/{}",
        SLOT_STAGE_ROOT,
        sanitize_path_component(inspection.index.release.as_str()),
        short_hex(&inspection.signature.manifest_digest)
    )
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn short_hex(bytes: &[u8; 32]) -> String {
    hex_digest(bytes)[..16].to_string()
}

fn persist_boot_success(journal: &SlotStageJournal) -> Result<(), UpdateError> {
    let mut report = last_apply_report().unwrap_or(UpdateApplyReport {
        state: UpdateApplyState::Applied,
        channel: journal.channel.clone(),
        release: journal.release.clone(),
        signer_key_id: journal.signer_key_id.clone(),
        plan_class: UpdatePlanClass::RebootRequired,
        target_slot: Some(journal.target_slot),
        requires_reboot: true,
        total_download_bytes: 0,
        applied_artifacts: journal
            .bundles
            .iter()
            .map(|bundle| bundle.artifact_id.clone())
            .collect(),
        failure: None,
    });
    report.state = UpdateApplyState::Applied;
    report.failure = None;
    persist_report(&report)
}

fn persist_boot_failure(journal: &SlotStageJournal, err: &UpdateError) -> Result<(), UpdateError> {
    let mut report = last_apply_report().unwrap_or(UpdateApplyReport {
        state: UpdateApplyState::Failed,
        channel: journal.channel.clone(),
        release: journal.release.clone(),
        signer_key_id: journal.signer_key_id.clone(),
        plan_class: UpdatePlanClass::RebootRequired,
        target_slot: Some(journal.target_slot),
        requires_reboot: true,
        total_download_bytes: 0,
        applied_artifacts: journal
            .bundles
            .iter()
            .map(|bundle| bundle.artifact_id.clone())
            .collect(),
        failure: None,
    });
    report.state = UpdateApplyState::Failed;
    report.failure = Some(err.to_string());
    persist_report(&report)
}

impl UpdateApplyReport {
    fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("state=");
        out.push_str(self.state.as_str());
        out.push('\n');
        out.push_str("channel=");
        out.push_str(self.channel.as_str());
        out.push('\n');
        out.push_str("release=");
        out.push_str(self.release.as_str());
        out.push('\n');
        out.push_str("signer_key_id=");
        out.push_str(self.signer_key_id.as_str());
        out.push('\n');
        out.push_str("plan_class=");
        out.push_str(self.plan_class.as_str());
        out.push('\n');
        out.push_str("target_slot=");
        out.push_str(slot_option_to_str(self.target_slot));
        out.push('\n');
        out.push_str("requires_reboot=");
        out.push_str(if self.requires_reboot { "1" } else { "0" });
        out.push('\n');
        out.push_str("total_download_bytes=");
        out.push_str(&self.total_download_bytes.to_string());
        out.push('\n');
        out.push_str("applied_artifacts=");
        out.push_str(self.applied_artifacts.join(",").as_str());
        out.push('\n');
        out.push_str("failure=");
        out.push_str(self.failure.clone().unwrap_or_default().as_str());
        out.push('\n');
        out.into_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, UpdateError> {
        let text = core::str::from_utf8(bytes).map_err(|_| UpdateError::InvalidFormat)?;
        let mut state = None;
        let mut channel = None;
        let mut release = None;
        let mut signer_key_id = None;
        let mut plan_class = None;
        let mut target_slot = None;
        let mut requires_reboot = None;
        let mut total_download_bytes = None;
        let mut applied_artifacts = Vec::new();
        let mut failure = None;

        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "state" => state = UpdateApplyState::parse(value),
                "channel" => channel = Some(value.to_string()),
                "release" => release = Some(value.to_string()),
                "signer_key_id" => signer_key_id = Some(value.to_string()),
                "plan_class" => plan_class = UpdatePlanClass::parse(value),
                "target_slot" => target_slot = parse_slot_option(value),
                "requires_reboot" => requires_reboot = Some(value == "1"),
                "total_download_bytes" => total_download_bytes = value.parse::<u64>().ok(),
                "applied_artifacts" => applied_artifacts = split_csv(value),
                "failure" => {
                    if !value.is_empty() {
                        failure = Some(value.to_string());
                    }
                }
                _ => {}
            }
        }

        Ok(Self {
            state: state.ok_or(UpdateError::InvalidFormat)?,
            channel: channel.ok_or(UpdateError::InvalidFormat)?,
            release: release.ok_or(UpdateError::InvalidFormat)?,
            signer_key_id: signer_key_id.ok_or(UpdateError::InvalidFormat)?,
            plan_class: plan_class.ok_or(UpdateError::InvalidFormat)?,
            target_slot: target_slot.ok_or(UpdateError::InvalidFormat)?,
            requires_reboot: requires_reboot.ok_or(UpdateError::InvalidFormat)?,
            total_download_bytes: total_download_bytes.ok_or(UpdateError::InvalidFormat)?,
            applied_artifacts,
            failure,
        })
    }
}

impl SlotStageJournal {
    fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("channel=");
        out.push_str(self.channel.as_str());
        out.push('\n');
        out.push_str("release=");
        out.push_str(self.release.as_str());
        out.push('\n');
        out.push_str("signer_key_id=");
        out.push_str(self.signer_key_id.as_str());
        out.push('\n');
        out.push_str("target_slot=");
        out.push_str(slot_to_str(self.target_slot));
        out.push('\n');
        for bundle in &self.bundles {
            out.push_str("bundle=");
            out.push_str(bundle.artifact_id.as_str());
            out.push('|');
            out.push_str(bundle.package_id.as_str());
            out.push('|');
            out.push_str(bundle.version.as_str());
            out.push('|');
            out.push_str(bundle.staged_path.as_str());
            out.push('|');
            out.push_str(&bundle.bytes.to_string());
            out.push('|');
            out.push_str(hex_digest(&bundle.digest).as_str());
            out.push('\n');
        }
        out.into_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, UpdateError> {
        let text = core::str::from_utf8(bytes).map_err(|_| UpdateError::InvalidFormat)?;
        let mut channel = None;
        let mut release = None;
        let mut signer_key_id = None;
        let mut target_slot = None;
        let mut bundles = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "channel" => channel = Some(value.to_string()),
                "release" => release = Some(value.to_string()),
                "signer_key_id" => signer_key_id = Some(value.to_string()),
                "target_slot" => target_slot = parse_slot(value),
                "bundle" => bundles.push(parse_staged_bundle(value)?),
                _ => {}
            }
        }
        Ok(Self {
            channel: channel.ok_or(UpdateError::InvalidFormat)?,
            release: release.ok_or(UpdateError::InvalidFormat)?,
            signer_key_id: signer_key_id.ok_or(UpdateError::InvalidFormat)?,
            target_slot: target_slot.ok_or(UpdateError::InvalidFormat)?,
            bundles,
        })
    }
}

fn encode_unsigned_index(index: &UpdateIndex) -> Result<Vec<u8>, UpdateError> {
    if index.artifacts.len() > u16::MAX as usize {
        return Err(UpdateError::ManifestTooLarge);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&INDEX_MAGIC);
    out.extend_from_slice(&INDEX_VERSION.to_le_bytes());
    write_string(&mut out, index.channel.as_str())?;
    write_string(&mut out, index.release.as_str())?;
    out.extend_from_slice(&index.published_epoch.to_le_bytes());
    out.extend_from_slice(&(index.artifacts.len() as u16).to_le_bytes());
    for artifact in &index.artifacts {
        out.push(artifact.kind.encode_tag());
        out.push(if artifact.reboot_required { 1 } else { 0 });
        out.push(slot_option_to_u8(artifact.requested_slot));
        write_string(&mut out, artifact.id.as_str())?;
        write_string(&mut out, artifact.version.as_str())?;
        write_string(&mut out, artifact.source.as_str())?;
        write_option_string(&mut out, artifact.package_id.as_deref())?;
        out.extend_from_slice(&artifact.bytes.to_le_bytes());
        out.extend_from_slice(&artifact.digest);
    }
    Ok(out)
}

fn decode_unsigned_index(bytes: &[u8]) -> Result<(UpdateIndex, usize), UpdateError> {
    if bytes.len() < INDEX_MAGIC.len() + 2 {
        return Err(UpdateError::InvalidFormat);
    }
    let mut cursor = 0usize;
    let magic = read_slice(bytes, &mut cursor, INDEX_MAGIC.len())?;
    if magic != INDEX_MAGIC {
        return Err(UpdateError::InvalidMagic);
    }
    let version = u16::from_le_bytes(read_fixed::<2>(bytes, &mut cursor)?);
    if version != INDEX_VERSION {
        return Err(UpdateError::UnsupportedVersion);
    }
    let channel = read_string(bytes, &mut cursor)?;
    let release = read_string(bytes, &mut cursor)?;
    let published_epoch = u64::from_le_bytes(read_fixed::<8>(bytes, &mut cursor)?);
    let artifact_count = u16::from_le_bytes(read_fixed::<2>(bytes, &mut cursor)?) as usize;
    let mut artifacts = Vec::with_capacity(artifact_count);
    for _ in 0..artifact_count {
        let kind = UpdateArtifactKind::decode_tag(read_u8(bytes, &mut cursor)?)?;
        let reboot_required = read_u8(bytes, &mut cursor)? != 0;
        let requested_slot = slot_option_from_u8(read_u8(bytes, &mut cursor)?);
        let id = read_string(bytes, &mut cursor)?;
        let version = read_string(bytes, &mut cursor)?;
        let source = read_string(bytes, &mut cursor)?;
        let package_id = read_option_string(bytes, &mut cursor)?;
        let artifact_bytes = u64::from_le_bytes(read_fixed::<8>(bytes, &mut cursor)?);
        let digest = read_fixed::<32>(bytes, &mut cursor)?;
        artifacts.push(UpdateArtifact {
            id,
            version,
            source,
            kind,
            bytes: artifact_bytes,
            digest,
            reboot_required,
            package_id,
            requested_slot,
        });
    }
    Ok((
        UpdateIndex {
            channel,
            release,
            published_epoch,
            artifacts,
        },
        cursor,
    ))
}

fn encode_signature_metadata(
    metadata: &UpdateIndexSignatureMetadata,
) -> Result<Vec<u8>, UpdateError> {
    let mut out = Vec::new();
    out.push(metadata.profile.encode_tag());
    write_string(&mut out, metadata.signer_key_id.as_str())?;
    out.extend_from_slice(&metadata.manifest_digest);
    Ok(out)
}

fn decode_signature_metadata(bytes: &[u8]) -> Result<UpdateIndexSignatureMetadata, UpdateError> {
    let mut cursor = 0usize;
    let profile = UpdateIndexSignatureProfile::decode_tag(read_u8(bytes, &mut cursor)?)?;
    let signer_key_id = read_string(bytes, &mut cursor)?;
    let manifest_digest = read_fixed::<32>(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err(UpdateError::InvalidFormat);
    }
    Ok(UpdateIndexSignatureMetadata {
        signer_key_id,
        profile,
        manifest_digest,
    })
}

fn sign_signed_index_bytes(
    content: &[u8],
    profile: UpdateIndexSignatureProfile,
) -> Result<[u8; 64], UpdateError> {
    let signing_key = P256SigningKey::from_slice(profile.signing_seed())
        .map_err(|_| UpdateError::InvalidSignature)?;
    let signature: P256Signature = signing_key.sign(content);
    let mut out = [0u8; 64];
    out.copy_from_slice(&signature.to_bytes());
    Ok(out)
}

fn verify_signed_index_bytes(
    content: &[u8],
    profile: UpdateIndexSignatureProfile,
    signature: &[u8; 64],
) -> Result<(), UpdateError> {
    let signing_key = P256SigningKey::from_slice(profile.signing_seed())
        .map_err(|_| UpdateError::InvalidSignature)?;
    let verifying_key = P256VerifyingKey::from(&signing_key);
    let signature =
        P256Signature::from_slice(signature).map_err(|_| UpdateError::InvalidSignature)?;
    verifying_key
        .verify(content, &signature)
        .map_err(|_| UpdateError::InvalidSignature)
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), UpdateError> {
    if value.len() > u16::MAX as usize {
        return Err(UpdateError::ManifestTooLarge);
    }
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_option_string(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), UpdateError> {
    match value {
        Some(value) => {
            out.push(1);
            write_string(out, value)
        }
        None => {
            out.push(0);
            Ok(())
        }
    }
}

fn read_option_string(bytes: &[u8], cursor: &mut usize) -> Result<Option<String>, UpdateError> {
    match read_u8(bytes, cursor)? {
        0 => Ok(None),
        1 => Ok(Some(read_string(bytes, cursor)?)),
        _ => Err(UpdateError::InvalidFormat),
    }
}

fn read_string(bytes: &[u8], cursor: &mut usize) -> Result<String, UpdateError> {
    let len = u16::from_le_bytes(read_fixed::<2>(bytes, cursor)?) as usize;
    let slice = read_slice(bytes, cursor, len)?;
    let text = core::str::from_utf8(slice).map_err(|_| UpdateError::InvalidFormat)?;
    Ok(text.to_string())
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, UpdateError> {
    let value = *bytes.get(*cursor).ok_or(UpdateError::InvalidFormat)?;
    *cursor += 1;
    Ok(value)
}

fn read_fixed<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], UpdateError> {
    let slice = read_slice(bytes, cursor, N)?;
    slice.try_into().map_err(|_| UpdateError::InvalidFormat)
}

fn read_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], UpdateError> {
    if bytes.len().saturating_sub(*cursor) < len {
        return Err(UpdateError::InvalidFormat);
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        let upper = byte >> 4;
        let lower = byte & 0x0f;
        out.push(nibble_to_hex(upper));
        out.push(nibble_to_hex(lower));
    }
    out
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + (value - 10)) as char,
        _ => '0',
    }
}

fn split_csv(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    value
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect()
}

fn parse_staged_bundle(value: &str) -> Result<StagedServiceBundle, UpdateError> {
    let mut parts = value.split('|');
    let artifact_id = parts.next().ok_or(UpdateError::InvalidFormat)?;
    let package_id = parts.next().ok_or(UpdateError::InvalidFormat)?;
    let version = parts.next().ok_or(UpdateError::InvalidFormat)?;
    let staged_path = parts.next().ok_or(UpdateError::InvalidFormat)?;
    let bytes = parts
        .next()
        .ok_or(UpdateError::InvalidFormat)?
        .parse::<u64>()
        .map_err(|_| UpdateError::InvalidFormat)?;
    let digest_hex = parts.next().ok_or(UpdateError::InvalidFormat)?;
    if parts.next().is_some() {
        return Err(UpdateError::InvalidFormat);
    }
    Ok(StagedServiceBundle {
        artifact_id: artifact_id.to_string(),
        package_id: package_id.to_string(),
        version: version.to_string(),
        staged_path: staged_path.to_string(),
        bytes,
        digest: parse_hex_digest(digest_hex)?,
    })
}

fn parse_hex_digest(value: &str) -> Result<[u8; 32], UpdateError> {
    if value.len() != 64 {
        return Err(UpdateError::InvalidFormat);
    }
    let mut out = [0u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        out[index] = (hex_to_nibble(chunk[0])? << 4) | hex_to_nibble(chunk[1])?;
    }
    Ok(out)
}

fn slot_option_to_str(slot: Option<SlotId>) -> &'static str {
    slot.map(slot_to_str).unwrap_or("-")
}

pub(crate) fn slot_to_str(slot: SlotId) -> &'static str {
    match slot {
        SlotId::None => "none",
        SlotId::SystemA => "system_a",
        SlotId::SystemB => "system_b",
        SlotId::Recovery => "recovery",
    }
}

fn parse_slot_option(value: &str) -> Option<Option<SlotId>> {
    match value {
        "-" => Some(None),
        "none" => Some(Some(SlotId::None)),
        "system_a" => Some(Some(SlotId::SystemA)),
        "system_b" => Some(Some(SlotId::SystemB)),
        "recovery" => Some(Some(SlotId::Recovery)),
        _ => None,
    }
}

fn parse_slot(value: &str) -> Option<SlotId> {
    parse_slot_option(value).and_then(|slot| slot)
}

fn slot_option_to_u8(slot: Option<SlotId>) -> u8 {
    slot.map(|value| value as u8).unwrap_or(0xff)
}

fn slot_option_from_u8(value: u8) -> Option<SlotId> {
    match value {
        0xff => None,
        0 => Some(SlotId::None),
        1 => Some(SlotId::SystemA),
        2 => Some(SlotId::SystemB),
        3 => Some(SlotId::Recovery),
        _ => None,
    }
}

fn hex_to_nibble(value: u8) -> Result<u8, UpdateError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(UpdateError::InvalidFormat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn sample_index() -> UpdateIndex {
        UpdateIndex {
            channel: String::from("engineering"),
            release: String::from("2026.04.08.1"),
            published_epoch: 1_775_620_800,
            artifacts: vec![
                UpdateArtifact::for_revocation_feed(
                    "7",
                    "https://updates.echos.dev/package-revocations.feed",
                    4096,
                    digest(0x11),
                ),
                UpdateArtifact::for_platform_image(
                    "echos-platform",
                    "2.1.0",
                    "https://updates.echos.dev/echos-platform-2.1.0.img",
                    96 * 1024 * 1024,
                    digest(0x22),
                ),
                UpdateArtifact::for_service_bundle(
                    "ech_store",
                    "2.1.0",
                    "https://updates.echos.dev/ech_store-2.1.0.bhd",
                    256 * 1024,
                    digest(0x33),
                    true,
                ),
                UpdateArtifact::for_user_bundle(
                    "org.echos.editor",
                    "1.5.0",
                    "https://updates.echos.dev/editor-1.5.0.bhd",
                    768 * 1024,
                    digest(0x44),
                ),
                UpdateArtifact::for_seed_catalog(
                    "seed-catalog",
                    "9",
                    "https://updates.echos.dev/seed-catalog-9.bhd",
                    64 * 1024,
                    digest(0x55),
                ),
            ],
        }
    }

    #[test]
    fn platform_update_targets_inactive_slot_and_arms_boot_control() {
        let artifacts = [UpdateArtifact::for_platform_image(
            "echos-platform",
            "2.0.0",
            "https://updates/echos-platform-2.0.0.img",
            64 * 1024 * 1024,
            digest(0x11),
        )];
        let plan = plan_update(SlotId::SystemA, &artifacts).expect("plan");
        assert_eq!(plan.class, UpdatePlanClass::RebootRequired);
        assert_eq!(plan.target_slot, Some(SlotId::SystemB));
        assert!(plan.requires_reboot);
        assert_eq!(plan.operations.len(), 2);
        assert!(matches!(
            plan.operations[0].kind,
            UpdateOperationKind::StagePlatformImage {
                target_slot: SlotId::SystemB
            }
        ));
        assert!(matches!(
            plan.operations[1].kind,
            UpdateOperationKind::ArmBootControl {
                target_slot: SlotId::SystemB
            }
        ));
    }

    #[test]
    fn live_bundle_update_avoids_slot_staging() {
        let artifacts = [UpdateArtifact::for_user_bundle(
            "org.echos.editor",
            "1.4.2",
            "https://updates/editor-1.4.2.bhd",
            512 * 1024,
            digest(0x22),
        )];
        let plan = plan_update(SlotId::SystemA, &artifacts).expect("plan");
        assert_eq!(plan.class, UpdatePlanClass::LiveOnly);
        assert_eq!(plan.target_slot, None);
        assert!(!plan.requires_reboot);
        assert_eq!(
            plan.affected_packages,
            vec![String::from("org.echos.editor")]
        );
        assert!(matches!(
            plan.operations[0].kind,
            UpdateOperationKind::InstallUserBundle
        ));
    }

    #[test]
    fn reboot_service_update_targets_inactive_slot_and_arms_boot_control() {
        let artifacts = [UpdateArtifact::for_service_bundle(
            "ech_store",
            "2.1.0",
            "https://updates/ech_store-2.1.0.bhd",
            128 * 1024,
            digest(0x31),
            true,
        )];
        let plan = plan_update(SlotId::SystemA, &artifacts).expect("plan");
        assert_eq!(plan.class, UpdatePlanClass::RebootRequired);
        assert_eq!(plan.target_slot, Some(SlotId::SystemB));
        assert!(plan.requires_reboot);
        assert!(plan.operations.iter().any(|operation| matches!(
            operation.kind,
            UpdateOperationKind::StageServiceBundle {
                target_slot: SlotId::SystemB
            }
        )));
        assert!(plan.operations.iter().any(|operation| matches!(
            operation.kind,
            UpdateOperationKind::ArmBootControl {
                target_slot: SlotId::SystemB
            }
        )));
    }

    #[test]
    fn mixed_platform_and_live_updates_split_correctly() {
        let artifacts = [
            UpdateArtifact::for_revocation_feed(
                "7",
                "https://updates/revocations.feed",
                4096,
                digest(0x33),
            ),
            UpdateArtifact::for_platform_image(
                "echos-platform",
                "2.1.0",
                "https://updates/echos-platform-2.1.0.img",
                96 * 1024 * 1024,
                digest(0x44),
            ),
            UpdateArtifact::for_service_bundle(
                "ech_store",
                "2.1.0",
                "https://updates/ech_store-2.1.0.bhd",
                128 * 1024,
                digest(0x55),
                true,
            ),
            UpdateArtifact::for_user_bundle(
                "org.echos.editor",
                "1.5.0",
                "https://updates/editor-1.5.0.bhd",
                768 * 1024,
                digest(0x66),
            ),
            UpdateArtifact::for_seed_catalog(
                "seed-catalog",
                "9",
                "https://updates/seed-catalog-9.bhd",
                32 * 1024,
                digest(0x77),
            ),
        ];
        let plan = plan_update(SlotId::SystemB, &artifacts).expect("plan");
        assert_eq!(plan.class, UpdatePlanClass::Mixed);
        assert_eq!(plan.target_slot, Some(SlotId::SystemA));
        assert!(plan.requires_reboot);
        assert!(plan
            .operations
            .iter()
            .any(|operation| matches!(operation.kind, UpdateOperationKind::PublishRevocationFeed)));
        assert!(plan.operations.iter().any(|operation| matches!(
            operation.kind,
            UpdateOperationKind::StagePlatformImage {
                target_slot: SlotId::SystemA
            }
        )));
        assert!(plan.operations.iter().any(|operation| matches!(
            operation.kind,
            UpdateOperationKind::StageServiceBundle {
                target_slot: SlotId::SystemA
            }
        )));
        assert!(plan
            .operations
            .iter()
            .any(|operation| matches!(operation.kind, UpdateOperationKind::InstallSeedCatalog)));
    }

    #[test]
    fn active_slot_target_requests_fail_closed() {
        let mut platform = UpdateArtifact::for_platform_image(
            "echos-platform",
            "2.0.1",
            "https://updates/echos-platform-2.0.1.img",
            4 * 1024 * 1024,
            digest(0x88),
        );
        platform.requested_slot = Some(SlotId::SystemB);
        let error = plan_update(SlotId::SystemB, &[platform]).expect_err("active-slot");
        assert_eq!(
            error,
            UpdatePlanError::PlatformTargetsActiveSlot {
                artifact_id: String::from("echos-platform"),
                active_slot: SlotId::SystemB,
            }
        );
    }

    #[test]
    fn conflicting_non_active_slot_requests_fail_closed() {
        let mut platform = UpdateArtifact::for_platform_image(
            "echos-platform",
            "2.0.1",
            "https://updates/echos-platform-2.0.1.img",
            4 * 1024 * 1024,
            digest(0x88),
        );
        platform.requested_slot = Some(SlotId::SystemA);
        let mut second_platform = UpdateArtifact::for_platform_image(
            "echos-platform-hotfix",
            "2.0.2",
            "https://updates/echos-platform-2.0.2.img",
            4 * 1024 * 1024,
            digest(0x99),
        );
        second_platform.requested_slot = Some(SlotId::Recovery);
        let error =
            plan_update(SlotId::SystemB, &[platform, second_platform]).expect_err("conflict");
        assert_eq!(
            error,
            UpdatePlanError::ConflictingTargetSlots {
                existing: SlotId::SystemA,
                requested: SlotId::Recovery,
            }
        );
    }

    #[test]
    fn signed_index_roundtrips_and_plans() {
        let index = sample_index();
        let bytes =
            build_signed_index(&index, UpdateIndexSignatureProfile::Engineering).expect("build");
        let inspection = inspect_signed_index(&bytes, SlotId::SystemA).expect("inspect");
        assert_eq!(inspection.index, index);
        assert_eq!(
            inspection.signature.profile,
            UpdateIndexSignatureProfile::Engineering
        );
        assert_eq!(inspection.plan.class, UpdatePlanClass::Mixed);
        assert_eq!(inspection.plan.target_slot, Some(SlotId::SystemB));
    }

    #[test]
    fn tampered_index_fails_signature_validation() {
        let index = sample_index();
        let mut bytes =
            build_signed_index(&index, UpdateIndexSignatureProfile::Stable).expect("build");
        let tail = bytes.len() - SIGNATURE_LEN - 1;
        bytes[tail] ^= 0x5a;
        let error = inspect_signed_index(&bytes, SlotId::SystemA).expect_err("tamper");
        assert!(matches!(
            error,
            UpdateError::InvalidSignature | UpdateError::InvalidManifestDigest
        ));
    }

    #[test]
    fn apply_report_roundtrips() {
        let report = UpdateApplyReport {
            state: UpdateApplyState::Staged,
            channel: String::from("engineering"),
            release: String::from("2026.04.08.1"),
            signer_key_id: String::from("update-engineering-root-v1"),
            plan_class: UpdatePlanClass::Mixed,
            target_slot: Some(SlotId::SystemB),
            requires_reboot: true,
            total_download_bytes: 128,
            applied_artifacts: vec![String::from("a"), String::from("b")],
            failure: None,
        };
        let decoded = UpdateApplyReport::decode(&report.encode()).expect("decode");
        assert_eq!(decoded, report);
    }

    #[test]
    fn slot_stage_journal_roundtrips() {
        let journal = SlotStageJournal {
            channel: String::from("engineering"),
            release: String::from("2026.04.08.1"),
            signer_key_id: String::from("update-engineering-root-v1"),
            target_slot: SlotId::SystemB,
            bundles: vec![StagedServiceBundle {
                artifact_id: String::from("ech_store"),
                package_id: String::from("ech_store"),
                version: String::from("2.1.0"),
                staged_path: String::from("/config/update/slot-stage/r1/ech_store.bhd"),
                bytes: 4096,
                digest: digest(0xaa),
            }],
        };
        let decoded = SlotStageJournal::decode(&journal.encode()).expect("decode");
        assert_eq!(decoded, journal);
    }
}
