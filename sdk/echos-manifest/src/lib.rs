#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const COMPILED_MANIFEST_MAGIC: [u8; 4] = *b"EAM2";
pub const PACKAGE_SIGNATURE_MAGIC: [u8; 4] = *b"EPS1";
pub const APP_ABI_VERSION: u16 = 1;
pub const NATIVE_ABI_VERSION: u16 = APP_ABI_VERSION;
pub const STATE_EXPORT_INLINE_LIMIT_BYTES: usize = 1024 * 1024;
pub const MAX_CAPABILITIES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    InvalidUtf8,
    MissingField,
    InvalidField,
    UnsupportedVersion,
    BufferTooSmall,
    Truncated,
    TooManyCapabilities,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRuntime {
    Native = 0,
    Pe = 1,
    Elf = 2,
    Special = 3,
}

impl AppRuntime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Pe => "pe",
            Self::Elf => "elf",
            Self::Special => "special",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value.trim() {
            "native" => Ok(Self::Native),
            "pe" => Ok(Self::Pe),
            "elf" => Ok(Self::Elf),
            "special" => Ok(Self::Special),
            _ => Err(ManifestError::InvalidField),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppPresentation {
    Windowed = 0,
    ShellOwned = 1,
    SpecialAction = 2,
    Headless = 3,
}

impl AppPresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::ShellOwned => "shell-owned",
            Self::SpecialAction => "special-action",
            Self::Headless => "headless",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value.trim() {
            "windowed" => Ok(Self::Windowed),
            "shell-owned" => Ok(Self::ShellOwned),
            "special-action" => Ok(Self::SpecialAction),
            "headless" => Ok(Self::Headless),
            _ => Err(ManifestError::InvalidField),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppStateContract {
    Stateless = 0,
    WarmSuspend = 1,
    ColdResume = 2,
}

impl AppStateContract {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stateless => "stateless",
            Self::WarmSuspend => "warm-suspend",
            Self::ColdResume => "cold-resume",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value.trim() {
            "stateless" => Ok(Self::Stateless),
            "warm-suspend" => Ok(Self::WarmSuspend),
            "cold-resume" => Ok(Self::ColdResume),
            _ => Err(ManifestError::InvalidField),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashRestartPolicy {
    Never = 0,
    OnFaultOnce = 1,
    BoundedRetry = 2,
}

impl CrashRestartPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnFaultOnce => "on-fault-once",
            Self::BoundedRetry => "bounded-retry",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartPolicy {
    pub kind: CrashRestartPolicy,
    pub retry_budget: u8,
}

impl RestartPolicy {
    pub const fn never() -> Self {
        Self {
            kind: CrashRestartPolicy::Never,
            retry_budget: 0,
        }
    }

    pub const fn on_fault_once() -> Self {
        Self {
            kind: CrashRestartPolicy::OnFaultOnce,
            retry_budget: 1,
        }
    }

    pub const fn bounded_retry(retry_budget: u8) -> Self {
        Self {
            kind: CrashRestartPolicy::BoundedRetry,
            retry_budget,
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        let trimmed = value.trim();
        match trimmed {
            "never" => Ok(Self::never()),
            "on-fault-once" => Ok(Self::on_fault_once()),
            _ => {
                let Some((prefix, raw_budget)) = trimmed.split_once(':') else {
                    return Err(ManifestError::InvalidField);
                };
                if prefix != "bounded-retry" {
                    return Err(ManifestError::InvalidField);
                }
                let retry_budget = raw_budget
                    .trim()
                    .parse::<u8>()
                    .map_err(|_| ManifestError::InvalidField)?;
                Ok(Self::bounded_retry(retry_budget))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustDomain {
    Platform = 0,
    Store = 1,
    Developer = 2,
    LocalUnsigned = 3,
}

impl TrustDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Store => "store",
            Self::Developer => "developer",
            Self::LocalUnsigned => "local-unsigned",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value.trim() {
            "platform" => Ok(Self::Platform),
            "store" => Ok(Self::Store),
            "developer" => Ok(Self::Developer),
            "local-unsigned" => Ok(Self::LocalUnsigned),
            _ => Err(ManifestError::InvalidField),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519 = 0,
    FirmwarePublishedKey = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatePayloadKind {
    Inline = 0,
    ResumeRef = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCapability {
    FsRead,
    FsWrite,
    DialogsOpen,
    DialogsSave,
    NotificationsPost,
    ClipboardRead,
    ClipboardWrite,
    CaptureFrame,
}

impl NativeCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::DialogsOpen => "dialogs.open",
            Self::DialogsSave => "dialogs.save",
            Self::NotificationsPost => "notifications.post",
            Self::ClipboardRead => "clipboard.read",
            Self::ClipboardWrite => "clipboard.write",
            Self::CaptureFrame => "capture.frame",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value.trim() {
            "fs.read" => Ok(Self::FsRead),
            "fs.write" => Ok(Self::FsWrite),
            "dialogs.open" => Ok(Self::DialogsOpen),
            "dialogs.save" => Ok(Self::DialogsSave),
            "notifications.post" => Ok(Self::NotificationsPost),
            "clipboard.read" => Ok(Self::ClipboardRead),
            "clipboard.write" => Ok(Self::ClipboardWrite),
            "capture.frame" => Ok(Self::CaptureFrame),
            _ => Err(ManifestError::InvalidField),
        }
    }

    pub fn bit(self) -> u64 {
        1u64 << (self as u8)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultWindow {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl DefaultWindow {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.title.trim().is_empty() || self.width == 0 || self.height == 0 {
            return Err(ManifestError::InvalidField);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAppManifest {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub sdk_version: u16,
    pub runtime: AppRuntime,
    pub presentation: AppPresentation,
    pub capabilities: Vec<NativeCapability>,
    pub default_window: DefaultWindow,
    pub state_contract: AppStateContract,
    pub restart_policy: RestartPolicy,
}

impl SourceAppManifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let mut app_id = None;
        let mut name = None;
        let mut version = None;
        let mut entry = None;
        let mut sdk_version = None;
        let mut runtime = None;
        let mut presentation = None;
        let mut capabilities = Vec::new();
        let mut default_title = None;
        let mut default_width = None;
        let mut default_height = None;
        let mut state_contract = Some(AppStateContract::ColdResume);
        let mut restart_policy = Some(RestartPolicy::never());

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "app_id" => app_id = Some(parse_string(value)?),
                "name" => name = Some(parse_string(value)?),
                "version" => version = Some(parse_string(value)?),
                "entry" => entry = Some(parse_string(value)?),
                "sdk_version" => sdk_version = Some(parse_u16(value)?),
                "runtime" => runtime = Some(AppRuntime::parse(&parse_string(value)?)?),
                "presentation" => {
                    presentation = Some(AppPresentation::parse(&parse_string(value)?)?)
                }
                "capabilities" => capabilities = parse_capability_list(value)?,
                "default_window.title" => default_title = Some(parse_string(value)?),
                "default_window.width" => default_width = Some(parse_u32(value)?),
                "default_window.height" => default_height = Some(parse_u32(value)?),
                "state_contract" => {
                    state_contract = Some(AppStateContract::parse(&parse_string(value)?)?)
                }
                "restart_policy" => {
                    restart_policy = Some(RestartPolicy::parse(&parse_string(value)?)?)
                }
                _ => {}
            }
        }

        let manifest = Self {
            app_id: app_id.ok_or(ManifestError::MissingField)?,
            name: name.ok_or(ManifestError::MissingField)?,
            version: version.ok_or(ManifestError::MissingField)?,
            entry: entry.ok_or(ManifestError::MissingField)?,
            sdk_version: sdk_version.ok_or(ManifestError::MissingField)?,
            runtime: runtime.ok_or(ManifestError::MissingField)?,
            presentation: presentation.ok_or(ManifestError::MissingField)?,
            capabilities,
            default_window: DefaultWindow {
                title: default_title.ok_or(ManifestError::MissingField)?,
                width: default_width.ok_or(ManifestError::MissingField)?,
                height: default_height.ok_or(ManifestError::MissingField)?,
            },
            state_contract: state_contract.ok_or(ManifestError::MissingField)?,
            restart_policy: restart_policy.ok_or(ManifestError::MissingField)?,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.app_id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.version.trim().is_empty()
            || self.entry.trim().is_empty()
            || self.sdk_version == 0
        {
            return Err(ManifestError::InvalidField);
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(ManifestError::TooManyCapabilities);
        }
        self.default_window.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledAppManifest {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub sdk_version: u16,
    pub abi_version: u16,
    pub runtime: AppRuntime,
    pub presentation: AppPresentation,
    pub capability_bits: u64,
    pub default_window: DefaultWindow,
    pub entry_sha256: [u8; 32],
    pub state_contract: AppStateContract,
    pub restart_policy: RestartPolicy,
}

impl CompiledAppManifest {
    pub fn from_source(
        source: &SourceAppManifest,
        entry_sha256: [u8; 32],
    ) -> Result<Self, ManifestError> {
        source.validate()?;
        let mut capability_bits = 0u64;
        for capability in &source.capabilities {
            capability_bits |= capability.bit();
        }
        Ok(Self {
            app_id: source.app_id.clone(),
            name: source.name.clone(),
            version: source.version.clone(),
            entry: source.entry.clone(),
            sdk_version: source.sdk_version,
            abi_version: APP_ABI_VERSION,
            runtime: source.runtime,
            presentation: source.presentation,
            capability_bits,
            default_window: source.default_window.clone(),
            entry_sha256,
            state_contract: source.state_contract,
            restart_policy: source.restart_policy,
        })
    }

    pub fn capabilities(&self) -> Vec<NativeCapability> {
        let mut capabilities = Vec::new();
        for capability in [
            NativeCapability::FsRead,
            NativeCapability::FsWrite,
            NativeCapability::DialogsOpen,
            NativeCapability::DialogsSave,
            NativeCapability::NotificationsPost,
            NativeCapability::ClipboardRead,
            NativeCapability::ClipboardWrite,
            NativeCapability::CaptureFrame,
        ] {
            if self.capability_bits & capability.bit() != 0 {
                capabilities.push(capability);
            }
        }
        capabilities
    }

    pub fn encode(&self) -> Result<Vec<u8>, ManifestError> {
        let mut out = Vec::new();
        out.extend_from_slice(&COMPILED_MANIFEST_MAGIC);
        out.extend_from_slice(&self.sdk_version.to_le_bytes());
        out.extend_from_slice(&self.abi_version.to_le_bytes());
        out.push(self.runtime as u8);
        out.push(self.presentation as u8);
        out.push(self.state_contract as u8);
        out.push(self.restart_policy.kind as u8);
        out.push(self.restart_policy.retry_budget);
        out.extend_from_slice(&self.capability_bits.to_le_bytes());
        encode_string(&mut out, &self.app_id)?;
        encode_string(&mut out, &self.name)?;
        encode_string(&mut out, &self.version)?;
        encode_string(&mut out, &self.entry)?;
        encode_string(&mut out, &self.default_window.title)?;
        out.extend_from_slice(&self.default_window.width.to_le_bytes());
        out.extend_from_slice(&self.default_window.height.to_le_bytes());
        out.extend_from_slice(&self.entry_sha256);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() < 21 || bytes[..4] != COMPILED_MANIFEST_MAGIC {
            return Err(ManifestError::InvalidField);
        }
        let mut cursor = 4usize;
        let sdk_version = read_u16(bytes, &mut cursor)?;
        let abi_version = read_u16(bytes, &mut cursor)?;
        if abi_version != APP_ABI_VERSION {
            return Err(ManifestError::UnsupportedVersion);
        }
        let runtime = decode_runtime(read_u8(bytes, &mut cursor)?)?;
        let presentation = decode_presentation(read_u8(bytes, &mut cursor)?)?;
        let state_contract = decode_state_contract(read_u8(bytes, &mut cursor)?)?;
        let restart_kind = decode_restart_kind(read_u8(bytes, &mut cursor)?)?;
        let retry_budget = read_u8(bytes, &mut cursor)?;
        let capability_bits = read_u64(bytes, &mut cursor)?;
        let app_id = decode_string(bytes, &mut cursor)?;
        let name = decode_string(bytes, &mut cursor)?;
        let version = decode_string(bytes, &mut cursor)?;
        let entry = decode_string(bytes, &mut cursor)?;
        let title = decode_string(bytes, &mut cursor)?;
        let width = read_u32(bytes, &mut cursor)?;
        let height = read_u32(bytes, &mut cursor)?;
        let entry_sha256 = read_fixed::<32>(bytes, &mut cursor)?;
        Ok(Self {
            app_id,
            name,
            version,
            entry,
            sdk_version,
            abi_version,
            runtime,
            presentation,
            capability_bits,
            default_window: DefaultWindow {
                title,
                width,
                height,
            },
            entry_sha256,
            state_contract,
            restart_policy: RestartPolicy {
                kind: restart_kind,
                retry_budget,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageSignatureMetadata {
    pub signer_key_id: String,
    pub trust_domain: TrustDomain,
    pub signature_algorithm: SignatureAlgorithm,
    pub manifest_digest: [u8; 32],
    pub package_digest: [u8; 32],
    pub entry_digest: [u8; 32],
    pub revocation_epoch: u32,
}

impl PackageSignatureMetadata {
    pub fn encode(&self) -> Result<Vec<u8>, ManifestError> {
        let mut out = Vec::new();
        out.extend_from_slice(&PACKAGE_SIGNATURE_MAGIC);
        out.push(self.trust_domain as u8);
        out.push(self.signature_algorithm as u8);
        out.extend_from_slice(&self.revocation_epoch.to_le_bytes());
        out.extend_from_slice(&self.manifest_digest);
        out.extend_from_slice(&self.package_digest);
        out.extend_from_slice(&self.entry_digest);
        encode_string(&mut out, &self.signer_key_id)?;
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ManifestError> {
        if bytes.len() < 78 || bytes[..4] != PACKAGE_SIGNATURE_MAGIC {
            return Err(ManifestError::InvalidField);
        }
        let mut cursor = 4usize;
        let trust_domain = decode_trust_domain(read_u8(bytes, &mut cursor)?)?;
        let signature_algorithm = decode_signature_algorithm(read_u8(bytes, &mut cursor)?)?;
        let revocation_epoch = read_u32(bytes, &mut cursor)?;
        let manifest_digest = read_fixed::<32>(bytes, &mut cursor)?;
        let package_digest = read_fixed::<32>(bytes, &mut cursor)?;
        let entry_digest = read_fixed::<32>(bytes, &mut cursor)?;
        let signer_key_id = decode_string(bytes, &mut cursor)?;
        Ok(Self {
            signer_key_id,
            trust_domain,
            signature_algorithm,
            manifest_digest,
            package_digest,
            entry_digest,
            revocation_epoch,
        })
    }
}

pub type NativeSourceManifest = SourceAppManifest;
pub type CompiledNativeManifest = CompiledAppManifest;

fn parse_string(value: &str) -> Result<String, ManifestError> {
    let trimmed = value.trim();
    if !(trimmed.starts_with('"') && trimmed.ends_with('"')) {
        return Err(ManifestError::InvalidField);
    }
    Ok(trimmed.trim_matches('"').to_string())
}

fn parse_u16(value: &str) -> Result<u16, ManifestError> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|_| ManifestError::InvalidField)
}

fn parse_u32(value: &str) -> Result<u32, ManifestError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| ManifestError::InvalidField)
}

fn parse_capability_list(value: &str) -> Result<Vec<NativeCapability>, ManifestError> {
    let trimmed = value.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Err(ManifestError::InvalidField);
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut capabilities = Vec::new();
    if inner.trim().is_empty() {
        return Ok(capabilities);
    }
    for raw in inner.split(',') {
        let capability = parse_string(raw.trim())?;
        capabilities.push(NativeCapability::parse(&capability)?);
    }
    Ok(capabilities)
}

fn encode_string(out: &mut Vec<u8>, value: &str) -> Result<(), ManifestError> {
    let bytes = value.as_bytes();
    if bytes.len() > u16::MAX as usize {
        return Err(ManifestError::BufferTooSmall);
    }
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn decode_string(bytes: &[u8], cursor: &mut usize) -> Result<String, ManifestError> {
    let len = read_u16(bytes, cursor)? as usize;
    if *cursor + len > bytes.len() {
        return Err(ManifestError::Truncated);
    }
    let value = core::str::from_utf8(&bytes[*cursor..*cursor + len])
        .map_err(|_| ManifestError::InvalidUtf8)?
        .to_string();
    *cursor += len;
    Ok(value)
}

fn decode_runtime(value: u8) -> Result<AppRuntime, ManifestError> {
    match value {
        0 => Ok(AppRuntime::Native),
        1 => Ok(AppRuntime::Pe),
        2 => Ok(AppRuntime::Elf),
        3 => Ok(AppRuntime::Special),
        _ => Err(ManifestError::InvalidField),
    }
}

fn decode_presentation(value: u8) -> Result<AppPresentation, ManifestError> {
    match value {
        0 => Ok(AppPresentation::Windowed),
        1 => Ok(AppPresentation::ShellOwned),
        2 => Ok(AppPresentation::SpecialAction),
        3 => Ok(AppPresentation::Headless),
        _ => Err(ManifestError::InvalidField),
    }
}

fn decode_state_contract(value: u8) -> Result<AppStateContract, ManifestError> {
    match value {
        0 => Ok(AppStateContract::Stateless),
        1 => Ok(AppStateContract::WarmSuspend),
        2 => Ok(AppStateContract::ColdResume),
        _ => Err(ManifestError::InvalidField),
    }
}

fn decode_restart_kind(value: u8) -> Result<CrashRestartPolicy, ManifestError> {
    match value {
        0 => Ok(CrashRestartPolicy::Never),
        1 => Ok(CrashRestartPolicy::OnFaultOnce),
        2 => Ok(CrashRestartPolicy::BoundedRetry),
        _ => Err(ManifestError::InvalidField),
    }
}

fn decode_trust_domain(value: u8) -> Result<TrustDomain, ManifestError> {
    match value {
        0 => Ok(TrustDomain::Platform),
        1 => Ok(TrustDomain::Store),
        2 => Ok(TrustDomain::Developer),
        3 => Ok(TrustDomain::LocalUnsigned),
        _ => Err(ManifestError::InvalidField),
    }
}

fn decode_signature_algorithm(value: u8) -> Result<SignatureAlgorithm, ManifestError> {
    match value {
        0 => Ok(SignatureAlgorithm::Ed25519),
        1 => Ok(SignatureAlgorithm::FirmwarePublishedKey),
        _ => Err(ManifestError::InvalidField),
    }
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, ManifestError> {
    if *cursor >= bytes.len() {
        return Err(ManifestError::Truncated);
    }
    let value = bytes[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, ManifestError> {
    let raw = read_fixed::<2>(bytes, cursor)?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, ManifestError> {
    let raw = read_fixed::<4>(bytes, cursor)?;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, ManifestError> {
    let raw = read_fixed::<8>(bytes, cursor)?;
    Ok(u64::from_le_bytes(raw))
}

fn read_fixed<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], ManifestError> {
    if *cursor + N > bytes.len() {
        return Err(ManifestError::Truncated);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[*cursor..*cursor + N]);
    *cursor += N;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        AppPresentation, AppRuntime, AppStateContract, CompiledAppManifest, DefaultWindow,
        NativeCapability, PackageSignatureMetadata, RestartPolicy, SourceAppManifest, TrustDomain,
        APP_ABI_VERSION,
    };
    use alloc::string::String;
    use alloc::vec;

    #[test]
    fn source_manifest_parses_and_roundtrips() {
        let text = r#"
app_id = "demo.hello"
name = "Hello"
version = "1.0.0"
entry = "hello_app"
sdk_version = 1
runtime = "native"
presentation = "windowed"
capabilities = ["notifications.post","clipboard.write"]
default_window.title = "Hello"
default_window.width = 640
default_window.height = 360
state_contract = "cold-resume"
restart_policy = "bounded-retry:3"
"#;
        let source = SourceAppManifest::parse(text).expect("source");
        assert_eq!(source.runtime, AppRuntime::Native);
        assert_eq!(source.presentation, AppPresentation::Windowed);
        assert_eq!(source.state_contract, AppStateContract::ColdResume);
        assert_eq!(source.restart_policy, RestartPolicy::bounded_retry(3));
        assert_eq!(
            source.capabilities,
            vec![
                NativeCapability::NotificationsPost,
                NativeCapability::ClipboardWrite
            ]
        );

        let compiled = CompiledAppManifest::from_source(&source, [0xAB; 32]).expect("compiled");
        assert_eq!(compiled.abi_version, APP_ABI_VERSION);
        let bytes = compiled.encode().expect("encode");
        let decoded = CompiledAppManifest::decode(&bytes).expect("decode");
        assert_eq!(decoded.default_window.width, 640);
        assert_eq!(decoded.entry_sha256, [0xAB; 32]);
        assert_eq!(decoded.restart_policy, RestartPolicy::bounded_retry(3));
    }

    #[test]
    fn default_window_validation_rejects_zero_geometry() {
        let window = DefaultWindow {
            title: "bad".into(),
            width: 0,
            height: 200,
        };
        assert!(window.validate().is_err());
    }

    #[test]
    fn signature_metadata_roundtrips() {
        let metadata = PackageSignatureMetadata {
            signer_key_id: String::from("developer-root-v1"),
            trust_domain: TrustDomain::Developer,
            signature_algorithm: super::SignatureAlgorithm::Ed25519,
            manifest_digest: [1; 32],
            package_digest: [2; 32],
            entry_digest: [3; 32],
            revocation_epoch: 7,
        };
        let encoded = metadata.encode().expect("encode");
        let decoded = PackageSignatureMetadata::decode(&encoded).expect("decode");
        assert_eq!(decoded, metadata);
    }
}
