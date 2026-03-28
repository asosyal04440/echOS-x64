//! Crash-safe resume store for packaged apps.

use crate::boot::appliance;
use crate::ipc::request_store_sync;
use crate::security::package::InstalledPackagedApp;
use crate::services::{StoreCommand, StoreResponse};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use echos_manifest::{AppStateContract, StatePayloadKind, STATE_EXPORT_INLINE_LIMIT_BYTES};
use sha2::{Digest, Sha256};

const RESUME_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SupervisorError {
    Io,
    StateTooLarge,
    ResumeDescriptorInvalid,
    HashMismatch,
    ManifestMismatch,
    PackageMismatch,
    SchemaMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeBundleHeader {
    pub app_id: String,
    pub manifest_digest: [u8; 32],
    pub package_digest: [u8; 32],
    pub state_schema_version: u16,
    pub generation: u64,
    pub boot_epoch: u64,
    pub payload_kind: StatePayloadKind,
    pub payload_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedResumeState {
    pub generation: u64,
    pub payload_kind: StatePayloadKind,
    pub payload: Vec<u8>,
    pub boot_epoch: u64,
}

pub fn resume_token_for_app(app: &InstalledPackagedApp) -> Option<u64> {
    load_committed_state(app).ok().flatten().map(|state| state.generation)
}

pub fn commit_inline_state(
    app: &InstalledPackagedApp,
    payload: &[u8],
) -> Result<u64, SupervisorError> {
    if payload.len() > STATE_EXPORT_INLINE_LIMIT_BYTES {
        return Err(SupervisorError::StateTooLarge);
    }
    commit_state(app, StatePayloadKind::Inline, payload)
}

pub fn commit_resume_ref(
    app: &InstalledPackagedApp,
    relative_ref: &str,
) -> Result<u64, SupervisorError> {
    if !is_valid_resume_ref(relative_ref) {
        return Err(SupervisorError::ResumeDescriptorInvalid);
    }
    commit_state(app, StatePayloadKind::ResumeRef, relative_ref.as_bytes())
}

pub fn load_committed_state(
    app: &InstalledPackagedApp,
) -> Result<Option<CommittedResumeState>, SupervisorError> {
    if app.compiled_manifest.state_contract != AppStateContract::ColdResume {
        return Ok(None);
    }
    let base = resume_root(app);
    let current_path = alloc::format!("{base}/current");
    let current = match store_request(StoreCommand::ReadFile { path: current_path }) {
        Ok(StoreResponse::FileData(bytes)) => bytes,
        Ok(StoreResponse::Error(_)) | Err(_) => return Ok(None),
        Ok(_) => return Err(SupervisorError::Io),
    };
    let generation = parse_generation_pointer(&current)?;
    let meta_path = alloc::format!("{base}/generation-{generation}.meta");
    let payload_path = alloc::format!("{base}/generation-{generation}.payload");
    let meta_bytes = read_required(&meta_path)?;
    let payload = read_required(&payload_path)?;
    let header = ResumeBundleHeader::decode(&meta_bytes)?;
    validate_header(app, &header, &payload)?;
    if header.payload_kind == StatePayloadKind::ResumeRef {
        let ref_path =
            core::str::from_utf8(&payload).map_err(|_| SupervisorError::ResumeDescriptorInvalid)?;
        if !is_valid_resume_ref(ref_path) {
            return Err(SupervisorError::ResumeDescriptorInvalid);
        }
    }
    Ok(Some(CommittedResumeState {
        generation: header.generation,
        payload_kind: header.payload_kind,
        payload,
        boot_epoch: header.boot_epoch,
    }))
}

impl ResumeBundleHeader {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.state_schema_version.to_le_bytes());
        out.extend_from_slice(&self.generation.to_le_bytes());
        out.extend_from_slice(&self.boot_epoch.to_le_bytes());
        out.push(self.payload_kind as u8);
        out.extend_from_slice(&self.manifest_digest);
        out.extend_from_slice(&self.package_digest);
        out.extend_from_slice(&self.payload_hash);
        out.extend_from_slice(&(self.app_id.len() as u16).to_le_bytes());
        out.extend_from_slice(self.app_id.as_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SupervisorError> {
        if bytes.len() < 2 + 8 + 8 + 1 + 32 + 32 + 32 + 2 {
            return Err(SupervisorError::ResumeDescriptorInvalid);
        }
        let mut cursor = 0usize;
        let state_schema_version = read_u16(bytes, &mut cursor)?;
        let generation = read_u64(bytes, &mut cursor)?;
        let boot_epoch = read_u64(bytes, &mut cursor)?;
        let payload_kind = match read_u8(bytes, &mut cursor)? {
            0 => StatePayloadKind::Inline,
            1 => StatePayloadKind::ResumeRef,
            _ => return Err(SupervisorError::ResumeDescriptorInvalid),
        };
        let manifest_digest = read_fixed::<32>(bytes, &mut cursor)?;
        let package_digest = read_fixed::<32>(bytes, &mut cursor)?;
        let payload_hash = read_fixed::<32>(bytes, &mut cursor)?;
        let app_id_len = read_u16(bytes, &mut cursor)? as usize;
        if cursor + app_id_len > bytes.len() {
            return Err(SupervisorError::ResumeDescriptorInvalid);
        }
        let app_id = core::str::from_utf8(&bytes[cursor..cursor + app_id_len])
            .map_err(|_| SupervisorError::ResumeDescriptorInvalid)?
            .to_string();
        Ok(Self {
            app_id,
            manifest_digest,
            package_digest,
            state_schema_version,
            generation,
            boot_epoch,
            payload_kind,
            payload_hash,
        })
    }
}

fn commit_state(
    app: &InstalledPackagedApp,
    payload_kind: StatePayloadKind,
    payload: &[u8],
) -> Result<u64, SupervisorError> {
    let base = resume_root(app);
    create_directory(&base)?;
    let generation = next_generation(app)?;
    let payload_hash = sha256_array(payload);
    let boot_epoch = appliance::shadow_snapshot().boot_epoch;
    let header = ResumeBundleHeader {
        app_id: app.compiled_manifest.app_id.clone(),
        manifest_digest: app.manifest_digest,
        package_digest: app.package_digest,
        state_schema_version: RESUME_SCHEMA_VERSION,
        generation,
        boot_epoch,
        payload_kind,
        payload_hash,
    };
    let payload_temp = alloc::format!("{base}/generation-{generation}.payload.pending");
    let payload_final = alloc::format!("{base}/generation-{generation}.payload");
    let meta_temp = alloc::format!("{base}/generation-{generation}.meta.pending");
    let meta_final = alloc::format!("{base}/generation-{generation}.meta");
    let current_temp = alloc::format!("{base}/current.pending");
    let current_final = alloc::format!("{base}/current");

    write_file(&payload_temp, payload)?;
    rename_path(&payload_temp, &payload_final)?;
    write_file(&meta_temp, &header.encode())?;
    rename_path(&meta_temp, &meta_final)?;
    write_file(&current_temp, generation.to_string().as_bytes())?;
    rename_path(&current_temp, &current_final)?;
    Ok(generation)
}

fn next_generation(app: &InstalledPackagedApp) -> Result<u64, SupervisorError> {
    Ok(load_committed_state(app)?
        .map(|state| state.generation.saturating_add(1))
        .unwrap_or(1))
}

fn validate_header(
    app: &InstalledPackagedApp,
    header: &ResumeBundleHeader,
    payload: &[u8],
) -> Result<(), SupervisorError> {
    if header.state_schema_version != RESUME_SCHEMA_VERSION {
        return Err(SupervisorError::SchemaMismatch);
    }
    if header.app_id != app.compiled_manifest.app_id {
        return Err(SupervisorError::ResumeDescriptorInvalid);
    }
    if header.manifest_digest != app.manifest_digest {
        return Err(SupervisorError::ManifestMismatch);
    }
    if header.package_digest != app.package_digest {
        return Err(SupervisorError::PackageMismatch);
    }
    if sha256_array(payload) != header.payload_hash {
        return Err(SupervisorError::HashMismatch);
    }
    if header.payload_kind == StatePayloadKind::Inline
        && payload.len() > STATE_EXPORT_INLINE_LIMIT_BYTES
    {
        return Err(SupervisorError::StateTooLarge);
    }
    Ok(())
}

fn resume_root(app: &InstalledPackagedApp) -> String {
    alloc::format!("/data/appdata/{}/resume", app.compiled_manifest.app_id)
}

fn is_valid_resume_ref(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains('\0') {
        return false;
    }
    path.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn parse_generation_pointer(bytes: &[u8]) -> Result<u64, SupervisorError> {
    let text = core::str::from_utf8(bytes).map_err(|_| SupervisorError::ResumeDescriptorInvalid)?;
    text.trim()
        .parse::<u64>()
        .map_err(|_| SupervisorError::ResumeDescriptorInvalid)
}

fn read_required(path: &str) -> Result<Vec<u8>, SupervisorError> {
    match store_request(StoreCommand::ReadFile {
        path: path.to_string(),
    }) {
        Ok(StoreResponse::FileData(bytes)) => Ok(bytes),
        Ok(_) | Err(_) => Err(SupervisorError::Io),
    }
}

fn create_directory(path: &str) -> Result<(), SupervisorError> {
    match store_request(StoreCommand::CreateDirectory {
        path: path.to_string(),
    }) {
        Ok(StoreResponse::Success) => Ok(()),
        Ok(StoreResponse::Error(_)) | Err(_) => Err(SupervisorError::Io),
        Ok(_) => Err(SupervisorError::Io),
    }
}

fn write_file(path: &str, data: &[u8]) -> Result<(), SupervisorError> {
    match store_request(StoreCommand::WriteFile {
        path: path.to_string(),
        data: data.to_vec(),
    }) {
        Ok(StoreResponse::Success) => Ok(()),
        Ok(StoreResponse::Error(_)) | Err(_) => Err(SupervisorError::Io),
        Ok(_) => Err(SupervisorError::Io),
    }
}

fn rename_path(from: &str, to: &str) -> Result<(), SupervisorError> {
    match store_request(StoreCommand::RenamePath {
        from: from.to_string(),
        to: to.to_string(),
    }) {
        Ok(StoreResponse::Success) => Ok(()),
        Ok(StoreResponse::Error(_)) | Err(_) => Err(SupervisorError::Io),
        Ok(_) => Err(SupervisorError::Io),
    }
}

fn store_request(command: StoreCommand) -> Result<StoreResponse, SupervisorError> {
    request_store_sync(0, command).ok_or(SupervisorError::Io)
}

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, SupervisorError> {
    if *cursor >= bytes.len() {
        return Err(SupervisorError::ResumeDescriptorInvalid);
    }
    let value = bytes[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, SupervisorError> {
    Ok(u16::from_le_bytes(read_fixed::<2>(bytes, cursor)?))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SupervisorError> {
    Ok(u64::from_le_bytes(read_fixed::<8>(bytes, cursor)?))
}

fn read_fixed<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], SupervisorError> {
    if *cursor + N > bytes.len() {
        return Err(SupervisorError::ResumeDescriptorInvalid);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[*cursor..*cursor + N]);
    *cursor += N;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{is_valid_resume_ref, ResumeBundleHeader, SupervisorError};
    use alloc::string::String;
    use echos_manifest::StatePayloadKind;

    #[test]
    fn resume_ref_rejects_absolute_and_parent_paths() {
        assert!(is_valid_resume_ref("resume/1/state.bin"));
        assert!(!is_valid_resume_ref("../state.bin"));
        assert!(!is_valid_resume_ref("/data/state.bin"));
    }

    #[test]
    fn resume_header_roundtrips() {
        let header = ResumeBundleHeader {
            app_id: String::from("org.echos.demo"),
            manifest_digest: [1; 32],
            package_digest: [2; 32],
            state_schema_version: 1,
            generation: 9,
            boot_epoch: 3,
            payload_kind: StatePayloadKind::ResumeRef,
            payload_hash: [4; 32],
        };
        let decoded = ResumeBundleHeader::decode(&header.encode()).expect("decode");
        assert_eq!(decoded, header);
    }

    #[test]
    fn truncated_resume_header_is_rejected() {
        assert_eq!(
            ResumeBundleHeader::decode(&[0u8; 4]),
            Err(SupervisorError::ResumeDescriptorInvalid)
        );
    }
}
