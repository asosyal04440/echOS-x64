//! Signed package installation and packaged-app launch verification for `.bhd` bundles.

use crate::gui::protocol::AppId;
use crate::ipc::request_store_sync;
use crate::services::{StoreCommand, StoreResponse};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use echos_manifest::{
    AppRuntime, CompiledAppManifest, NativeCapability, PackageSignatureMetadata,
    SignatureAlgorithm, SourceAppManifest, TrustDomain,
};
use p256::ecdsa::{Signature as P256Signature, SigningKey as P256SigningKey, VerifyingKey as P256VerifyingKey};
use sha2::{Digest, Sha256};
use signature::{Signer, Verifier};
use spin::Mutex;

const MAGIC: [u8; 8] = *b"echBHD01";
const SIGNATURE_LEN: usize = 64;
const COMPILED_MANIFEST_PATH: &str = "app.manifest.bin";
const SIGNATURE_METADATA_PATH: &str = "app.signature.bin";
const BUNDLE_STORE_ROOT: &str = "/apps/.bundles";
const LIVE_REVOCATION_FEED_PATH: &str = "/config/security/package_revocations.feed";
const REVOCATION_FEED_MAGIC: [u8; 8] = *b"echRVK01";
const REVOCATION_FEED_VERSION: u16 = 1;
const EMBEDDED_PLATFORM_REVOCATION_EPOCH: u32 = 0;
const EMBEDDED_REVOKED_SIGNER_KEY_IDS: &[&str] = &[];

pub const DEV_PACKAGE_SIGNING_SEED: [u8; 32] = [
    0x45, 0x63, 0x68, 0x4f, 0x53, 0x2d, 0x4e, 0x61, 0x74, 0x69, 0x76, 0x65, 0x2d, 0x50, 0x6b,
    0x67, 0x2d, 0x44, 0x65, 0x76, 0x2d, 0x4b, 0x65, 0x79, 0x2d, 0x30, 0x31, 0x2d, 0x42, 0x48,
    0x44, 0x31,
];
const PLATFORM_PACKAGE_SIGNING_SEED: [u8; 32] = [
    0x65, 0x63, 0x68, 0x4f, 0x53, 0x2d, 0x70, 0x6c, 0x61, 0x74, 0x66, 0x6f, 0x72, 0x6d, 0x2d,
    0x72, 0x6f, 0x6f, 0x74, 0x2d, 0x6b, 0x65, 0x79, 0x2d, 0x76, 0x31, 0x2d, 0x62, 0x68, 0x64,
    0x2d, 0x30,
];
const STORE_PACKAGE_SIGNING_SEED: [u8; 32] = [
    0x65, 0x63, 0x68, 0x4f, 0x53, 0x2d, 0x73, 0x74, 0x6f, 0x72, 0x65, 0x2d, 0x72, 0x6f, 0x6f,
    0x74, 0x2d, 0x6b, 0x65, 0x79, 0x2d, 0x76, 0x31, 0x2d, 0x62, 0x68, 0x64, 0x2d, 0x30, 0x30,
    0x31, 0x21,
];

#[derive(Clone, Copy)]
struct TrustRootRecord {
    signer_key_id: &'static str,
    trust_domain: TrustDomain,
    signature_algorithm: SignatureAlgorithm,
    signing_seed: &'static [u8; 32],
    requires_firmware_anchor: bool,
}

const TRUST_ROOTS: &[TrustRootRecord] = &[
    TrustRootRecord {
        signer_key_id: "developer-root-v1",
        trust_domain: TrustDomain::Developer,
        signature_algorithm: SignatureAlgorithm::Ed25519,
        signing_seed: &DEV_PACKAGE_SIGNING_SEED,
        requires_firmware_anchor: false,
    },
    TrustRootRecord {
        signer_key_id: "platform-root-v1",
        trust_domain: TrustDomain::Platform,
        signature_algorithm: SignatureAlgorithm::FirmwarePublishedKey,
        signing_seed: &PLATFORM_PACKAGE_SIGNING_SEED,
        requires_firmware_anchor: true,
    },
    TrustRootRecord {
        signer_key_id: "store-root-v1",
        trust_domain: TrustDomain::Store,
        signature_algorithm: SignatureAlgorithm::FirmwarePublishedKey,
        signing_seed: &STORE_PACKAGE_SIGNING_SEED,
        requires_firmware_anchor: true,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct RevocationFeed {
    minimum_epoch: u32,
    revoked_signer_key_ids: Vec<String>,
}

impl Default for RevocationFeed {
    fn default() -> Self {
        Self {
            minimum_epoch: EMBEDDED_PLATFORM_REVOCATION_EPOCH,
            revoked_signer_key_ids: EMBEDDED_REVOKED_SIGNER_KEY_IDS
                .iter()
                .map(|entry| (*entry).to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageTrustLevel {
    Platform,
    Store,
    Developer,
}

impl PackageTrustLevel {
    pub const fn from_domain(domain: TrustDomain) -> Self {
        match domain {
            TrustDomain::Platform => Self::Platform,
            TrustDomain::Store => Self::Store,
            TrustDomain::Developer | TrustDomain::LocalUnsigned => Self::Developer,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PackageError {
    InvalidMagic,
    InvalidFormat,
    InvalidSignature,
    InvalidManifest,
    UnsupportedAbi,
    HashMismatch,
    IoError,
    PackageExists,
    PackageNotFound,
    PermissionDenied,
    RepositoryUnavailable,
    UnsafePayloadPath,
    EmptyPayload,
    MissingPackagedPayload,
    SignatureMetadataMissing,
    TrustRootUnavailable,
    TrustRevoked,
    TrustMetadataInvalid,
    RuntimeMismatch,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::InvalidMagic => write!(f, "Gecersiz paket formati"),
            PackageError::InvalidFormat => write!(f, "Paket formati hatali"),
            PackageError::InvalidSignature => write!(f, "Paket imzasi gecersiz"),
            PackageError::InvalidManifest => write!(f, "Manifest dosyasi hatali"),
            PackageError::UnsupportedAbi => write!(f, "Desteklenmeyen uygulama ABI surumu"),
            PackageError::HashMismatch => write!(f, "Paket hash dogrulamasi basarisiz"),
            PackageError::IoError => write!(f, "I/O hatasi"),
            PackageError::PackageExists => write!(f, "Paket zaten kurulu"),
            PackageError::PackageNotFound => write!(f, "Paket bulunamadi"),
            PackageError::PermissionDenied => write!(f, "Izin reddedildi"),
            PackageError::RepositoryUnavailable => write!(f, "Paket deposu kullanima hazir degil"),
            PackageError::UnsafePayloadPath => write!(f, "Payload icinde guvensiz yol bulundu"),
            PackageError::EmptyPayload => write!(f, "Paket payload'u bos"),
            PackageError::MissingPackagedPayload => write!(f, "Packaged uygulama yuku eksik"),
            PackageError::SignatureMetadataMissing => write!(f, "Paket imza metadatasi eksik"),
            PackageError::TrustRootUnavailable => write!(f, "UEFI/TPM trust root kullanilamiyor"),
            PackageError::TrustRevoked => write!(f, "Paket signer anahtari iptal edildi"),
            PackageError::TrustMetadataInvalid => write!(f, "Canli revocation feed gecersiz"),
            PackageError::RuntimeMismatch => write!(f, "Packaged runtime kimligi uyusmuyor"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub executable: Option<String>,
    pub icon_type: Option<String>,
    pub permissions: Option<Vec<String>>,
}

impl PackageInfo {
    pub fn new() -> Self {
        Self {
            name: None,
            version: None,
            description: None,
            author: None,
            executable: None,
            icon_type: None,
            permissions: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstalledPackagedApp {
    pub runtime_app_id: AppId,
    pub manifest_app_id: &'static str,
    pub package_id: &'static str,
    pub title: &'static str,
    pub bundle_root: &'static str,
    pub bundle_path: &'static str,
    pub entry_path: &'static str,
    pub compiled_manifest_path: &'static str,
    pub compiled_manifest: CompiledAppManifest,
    pub capability_set: Vec<NativeCapability>,
    pub package_digest: [u8; 32],
    pub manifest_digest: [u8; 32],
    pub entry_digest: [u8; 32],
    pub trust_level: PackageTrustLevel,
    pub trust_domain: TrustDomain,
    pub signer_key_id: &'static str,
    pub revocation_epoch: u32,
}

pub type InstalledNativeApp = InstalledPackagedApp;

#[derive(Debug, Clone)]
pub struct VerifiedPackagedLaunch {
    pub installed: InstalledPackagedApp,
    pub entry_image: Vec<u8>,
}

pub type VerifiedNativeLaunch = VerifiedPackagedLaunch;

#[derive(Debug, Clone)]
pub struct BundleInspection {
    pub source_manifest: SourceAppManifest,
    pub compiled_manifest: CompiledAppManifest,
    pub signature_metadata: PackageSignatureMetadata,
}

#[derive(Debug)]
struct ParsedBundle {
    source_manifest: Vec<u8>,
    payload_files: Vec<(String, Vec<u8>)>,
    package_digest: [u8; 32],
    signature_metadata: Option<PackageSignatureMetadata>,
}

pub struct PackageManager {
    packages: BTreeMap<String, PackageInfo>,
    installed_paths: BTreeMap<String, String>,
    packaged_apps: BTreeMap<String, InstalledPackagedApp>,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            packages: BTreeMap::new(),
            installed_paths: BTreeMap::new(),
            packaged_apps: BTreeMap::new(),
        }
    }

    pub fn install_package(&mut self, data: &[u8]) -> Result<String, PackageError> {
        let parsed = self.parse_signed_bundle(data)?;
        if parsed.payload_files.is_empty() {
            return Err(PackageError::EmptyPayload);
        }

        let source_text = core::str::from_utf8(&parsed.source_manifest)
            .map_err(|_| PackageError::InvalidManifest)?;
        let source_manifest = SourceAppManifest::parse(source_text).ok();
        if parsed
            .payload_files
            .iter()
            .any(|(path, _)| path == COMPILED_MANIFEST_PATH)
        {
            return self.install_packaged_app(parsed, source_manifest);
        }

        self.install_legacy_package(parsed.source_manifest, parsed.payload_files)
    }

    pub fn remove_package(&mut self, name: &str) -> Result<(), PackageError> {
        if !self.packages.contains_key(name) {
            return Err(PackageError::PackageNotFound);
        }

        let mut dir_path = String::from("/apps/");
        dir_path.push_str(name);
        match store_request(StoreCommand::DeleteFile { path: dir_path }) {
            Ok(StoreResponse::Success) => {}
            Ok(_) | Err(_) => return Err(PackageError::IoError),
        }

        if let Some(installed) = self.packaged_apps.remove(name) {
            let _ = store_request(StoreCommand::DeleteFile {
                path: installed.bundle_path.to_string(),
            });
        }

        self.packages.remove(name);
        self.installed_paths.remove(name);
        Ok(())
    }

    pub fn list_packages(&self) -> Vec<(String, PackageInfo)> {
        self.packages
            .iter()
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect()
    }

    pub fn get_package_info(&self, name: &str) -> Option<PackageInfo> {
        self.packages.get(name).cloned()
    }

    pub fn search_packages(&self, term: &str) -> Vec<(String, PackageInfo)> {
        let term_lower = term.to_lowercase();
        self.packages
            .iter()
            .filter(|(name, info)| {
                name.to_lowercase().contains(&term_lower)
                    || info
                        .description
                        .as_ref()
                        .map(|desc| desc.to_lowercase().contains(&term_lower))
                        .unwrap_or(false)
                    || info
                        .name
                        .as_ref()
                        .map(|name| name.to_lowercase().contains(&term_lower))
                        .unwrap_or(false)
            })
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect()
    }

    pub fn update_package_list(&mut self) -> Result<(), PackageError> {
        Err(PackageError::RepositoryUnavailable)
    }

    pub fn installed_app(&self, query: &str) -> Option<InstalledPackagedApp> {
        if let Some(app) = self.packaged_apps.get(query) {
            return Some(app.clone());
        }
        self.packaged_apps
            .values()
            .find(|app| {
                app.manifest_app_id.eq_ignore_ascii_case(query)
                    || app.package_id.eq_ignore_ascii_case(query)
                    || app.title.eq_ignore_ascii_case(query)
                    || app.entry_path == query
            })
            .cloned()
    }

    pub fn installed_native_app(&self, query: &str) -> Option<InstalledNativeApp> {
        self.installed_app(query)
            .filter(|app| app.compiled_manifest.runtime == AppRuntime::Native)
    }

    pub fn verify_installed_package(&self, name: &str) -> Result<(), PackageError> {
        if let Some(app) = self.installed_app(name) {
            self.verify_packaged_bundle(&app)?;
            return Ok(());
        }
        if self.packages.contains_key(name) {
            return Ok(());
        }
        Err(PackageError::PackageNotFound)
    }

    pub fn verify_packaged_launch(
        &self,
        entry_path: &str,
    ) -> Result<VerifiedPackagedLaunch, PackageError> {
        let Some(installed) = self
            .packaged_apps
            .values()
            .find(|app| app.entry_path == entry_path)
            .cloned()
        else {
            return Err(PackageError::PackageNotFound);
        };
        let parsed = self.verify_packaged_bundle(&installed)?;
        let entry_image = lookup_payload(&parsed.payload_files, &installed.compiled_manifest.entry)
            .ok_or(PackageError::MissingPackagedPayload)?;
        Ok(VerifiedPackagedLaunch {
            installed,
            entry_image,
        })
    }

    pub fn verify_native_launch(
        &self,
        entry_path: &str,
    ) -> Result<VerifiedNativeLaunch, PackageError> {
        let verified = self.verify_packaged_launch(entry_path)?;
        if verified.installed.compiled_manifest.runtime != AppRuntime::Native {
            return Err(PackageError::RuntimeMismatch);
        }
        Ok(verified)
    }
 
    fn install_legacy_package(
        &mut self,
        manifest_data: Vec<u8>,
        extracted_files: Vec<(String, Vec<u8>)>,
    ) -> Result<String, PackageError> {
        let manifest = self.parse_legacy_manifest(&manifest_data)?;
        let package_name = manifest
            .name
            .as_ref()
            .ok_or(PackageError::InvalidManifest)?
            .clone();
        if self.packages.contains_key(&package_name) {
            return Err(PackageError::PackageExists);
        }
        for (path, content) in extracted_files {
            let full_path = bundle_file_path(&package_name, &path);
            let response = store_request(StoreCommand::WriteFile {
                path: full_path,
                data: content,
            });
            match response {
                Ok(StoreResponse::Success) => {}
                Ok(_) | Err(_) => return Err(PackageError::IoError),
            }
        }
        if let Some(executable) = &manifest.executable {
            self.installed_paths
                .insert(package_name.clone(), bundle_file_path(&package_name, executable));
        }
        self.packages.insert(package_name.clone(), manifest);
        let mut success = package_name.clone();
        success.push_str(" paketi basariyla kuruldu");
        Ok(success)
    }

    fn install_packaged_app(
        &mut self,
        parsed: ParsedBundle,
        source_manifest: Option<SourceAppManifest>,
    ) -> Result<String, PackageError> {
        let signature_metadata = parsed
            .signature_metadata
            .clone()
            .ok_or(PackageError::SignatureMetadataMissing)?;
        let compiled_bytes = lookup_payload(&parsed.payload_files, COMPILED_MANIFEST_PATH)
            .ok_or(PackageError::MissingPackagedPayload)?;
        let compiled_manifest = CompiledAppManifest::decode(&compiled_bytes).map_err(|err| match err {
            echos_manifest::ManifestError::UnsupportedVersion => PackageError::UnsupportedAbi,
            _ => PackageError::InvalidManifest,
        })?;
        if let Some(source) = source_manifest.as_ref() {
            validate_manifest_pair(source, &compiled_manifest)?;
        }
        let package_name = compiled_manifest.app_id.clone();
        if self.packages.contains_key(&package_name) {
            return Err(PackageError::PackageExists);
        }
        let entry_bytes = lookup_payload(&parsed.payload_files, &compiled_manifest.entry)
            .ok_or(PackageError::MissingPackagedPayload)?;
        let entry_digest = sha256_array(&entry_bytes);
        if compiled_manifest.entry_sha256 != entry_digest {
            return Err(PackageError::HashMismatch);
        }
        let manifest_digest = sha256_array(&compiled_bytes);
        if signature_metadata.manifest_digest != manifest_digest
            || signature_metadata.package_digest != parsed.package_digest
            || signature_metadata.entry_digest != entry_digest
        {
            return Err(PackageError::HashMismatch);
        }
        for (path, content) in &parsed.payload_files {
            let response = store_request(StoreCommand::WriteFile {
                path: bundle_file_path(&package_name, path),
                data: content.clone(),
            });
            match response {
                Ok(StoreResponse::Success) => {}
                Ok(_) | Err(_) => return Err(PackageError::IoError),
            }
        }
        let bundle_path = bundle_archive_path(&package_name);
        let _ = store_request(StoreCommand::CreateDirectory {
            path: BUNDLE_STORE_ROOT.to_string(),
        });
        let rebuilt = rebuild_bundle_bytes_with_metadata(
            &parsed.source_manifest,
            &parsed.payload_files,
            &signature_metadata,
        )?;
        match store_request(StoreCommand::WriteFile {
            path: bundle_path.clone(),
            data: rebuilt,
        }) {
            Ok(StoreResponse::Success) => {}
            Ok(_) | Err(_) => return Err(PackageError::IoError),
        }
        let entry_path = bundle_file_path(&package_name, &compiled_manifest.entry);
        let compiled_manifest_path = bundle_file_path(&package_name, COMPILED_MANIFEST_PATH);
        let capability_set = compiled_manifest.capabilities();
        let installed = InstalledPackagedApp {
            runtime_app_id: hash_manifest_app_id(&compiled_manifest.app_id, compiled_manifest.runtime),
            manifest_app_id: leak_string(compiled_manifest.app_id.clone()),
            package_id: leak_string(compiled_manifest.app_id.clone()),
            title: leak_string(compiled_manifest.name.clone()),
            bundle_root: leak_string(bundle_root(&package_name)),
            bundle_path: leak_string(bundle_path.clone()),
            entry_path: leak_string(entry_path.clone()),
            compiled_manifest_path: leak_string(compiled_manifest_path),
            compiled_manifest: compiled_manifest.clone(),
            capability_set: capability_set.clone(),
            package_digest: parsed.package_digest,
            manifest_digest,
            entry_digest,
            trust_level: PackageTrustLevel::from_domain(signature_metadata.trust_domain),
            trust_domain: signature_metadata.trust_domain,
            signer_key_id: leak_string(signature_metadata.signer_key_id.clone()),
            revocation_epoch: signature_metadata.revocation_epoch,
        };
        let mut info = PackageInfo::new();
        info.name = Some(compiled_manifest.name.clone());
        info.version = Some(compiled_manifest.version.clone());
        info.executable = Some(compiled_manifest.entry.clone());
        info.permissions = Some(
            capability_set
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect(),
        );
        self.installed_paths
            .insert(package_name.clone(), entry_path);
        self.packaged_apps.insert(package_name.clone(), installed);
        self.packages.insert(package_name.clone(), info);
        let mut success = package_name.clone();
        success.push_str(" packaged uygulamasi basariyla kuruldu");
        Ok(success)
    }

    fn verify_packaged_bundle(
        &self,
        installed: &InstalledPackagedApp,
    ) -> Result<ParsedBundle, PackageError> {
        let data = match store_request(StoreCommand::ReadFile {
            path: installed.bundle_path.to_string(),
        }) {
            Ok(StoreResponse::FileData(bytes)) => bytes,
            Ok(_) | Err(_) => return Err(PackageError::IoError),
        };
        let parsed = self.parse_signed_bundle(&data)?;
        if parsed.package_digest != installed.package_digest {
            return Err(PackageError::HashMismatch);
        }
        let signature_metadata = parsed
            .signature_metadata
            .as_ref()
            .ok_or(PackageError::SignatureMetadataMissing)?;
        if signature_metadata.trust_domain != installed.trust_domain
            || signature_metadata.revocation_epoch != installed.revocation_epoch
            || signature_metadata.signer_key_id != installed.signer_key_id
        {
            return Err(PackageError::HashMismatch);
        }
        let compiled_bytes = lookup_payload(&parsed.payload_files, COMPILED_MANIFEST_PATH)
            .ok_or(PackageError::MissingPackagedPayload)?;
        let compiled_manifest = CompiledAppManifest::decode(&compiled_bytes).map_err(|err| match err {
            echos_manifest::ManifestError::UnsupportedVersion => PackageError::UnsupportedAbi,
            _ => PackageError::InvalidManifest,
        })?;
        if compiled_manifest.app_id != installed.compiled_manifest.app_id
            || compiled_manifest.entry != installed.compiled_manifest.entry
            || compiled_manifest.sdk_version != installed.compiled_manifest.sdk_version
            || compiled_manifest.runtime != installed.compiled_manifest.runtime
        {
            return Err(PackageError::HashMismatch);
        }
        let manifest_digest = sha256_array(&compiled_bytes);
        if manifest_digest != installed.manifest_digest
            || signature_metadata.manifest_digest != manifest_digest
        {
            return Err(PackageError::HashMismatch);
        }
        let entry_bytes = lookup_payload(&parsed.payload_files, &compiled_manifest.entry)
            .ok_or(PackageError::MissingPackagedPayload)?;
        let entry_digest = sha256_array(&entry_bytes);
        if entry_digest != installed.entry_digest
            || compiled_manifest.entry_sha256 != entry_digest
            || signature_metadata.entry_digest != entry_digest
        {
            return Err(PackageError::HashMismatch);
        }
        Ok(parsed)
    }

    fn parse_signed_bundle(&self, data: &[u8]) -> Result<ParsedBundle, PackageError> {
        if data.len() < MAGIC.len() + 4 + SIGNATURE_LEN || &data[..MAGIC.len()] != &MAGIC {
            return Err(PackageError::InvalidMagic);
        }
        if data.len() < 12 {
            return Err(PackageError::InvalidFormat);
        }
        let manifest_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let manifest_start = MAGIC.len() + 4;
        let manifest_end = manifest_start + manifest_size;
        if data.len() < manifest_end + SIGNATURE_LEN {
            return Err(PackageError::InvalidFormat);
        }
        let signature_start = data.len() - SIGNATURE_LEN;
        let content_to_verify = &data[..signature_start];
        let signature = &data[signature_start..];
        let payload = &data[manifest_end..signature_start];
        let payload_files = self.extract_payload(payload)?;
        let signature_metadata = lookup_payload(&payload_files, SIGNATURE_METADATA_PATH)
            .map(|bytes| {
                PackageSignatureMetadata::decode(&bytes)
                    .map_err(|_| PackageError::InvalidFormat)
            })
            .transpose()?;
        let package_digest = canonical_package_digest(&data[manifest_start..manifest_end], &payload_files)?;
        if let Some(metadata) = signature_metadata.as_ref() {
            if metadata.package_digest != package_digest {
                return Err(PackageError::HashMismatch);
            }
            verify_package_signature(content_to_verify, signature, metadata)?;
        } else if signature.iter().any(|byte| *byte != 0) {
            self.verify_legacy_signature(content_to_verify, signature)?;
        }
        Ok(ParsedBundle {
            source_manifest: data[manifest_start..manifest_end].to_vec(),
            payload_files,
            package_digest,
            signature_metadata,
        })
    }

    fn verify_legacy_signature(
        &self,
        content: &[u8],
        signature: &[u8],
    ) -> Result<(), PackageError> {
        if signature.len() != SIGNATURE_LEN {
            return Err(PackageError::InvalidSignature);
        }
        let mut signature_array = [0u8; SIGNATURE_LEN];
        signature_array.copy_from_slice(signature);
        if sign_dev_package(content) == signature_array {
            Ok(())
        } else {
            Err(PackageError::InvalidSignature)
        }
    }

    fn parse_legacy_manifest(&self, data: &[u8]) -> Result<PackageInfo, PackageError> {
        let content = core::str::from_utf8(data).map_err(|_| PackageError::InvalidManifest)?;
        let mut manifest = PackageInfo::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "name" => manifest.name = Some(value.to_string()),
                "version" => manifest.version = Some(value.to_string()),
                "description" => manifest.description = Some(value.to_string()),
                "author" => manifest.author = Some(value.to_string()),
                "executable" => manifest.executable = Some(value.to_string()),
                "icon_type" => manifest.icon_type = Some(value.to_string()),
                "permissions" => {
                    manifest.permissions =
                        Some(value.split(',').map(|part| part.trim().to_string()).collect());
                }
                _ => {}
            }
        }
        Ok(manifest)
    }

    fn extract_payload(&self, data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, PackageError> {
        let mut files = Vec::new();
        let mut offset = 0usize;
        while offset < data.len() {
            if data.len().saturating_sub(offset) < 6 {
                return Err(PackageError::InvalidFormat);
            }
            let path_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            let content_len = u32::from_le_bytes([
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
            ]) as usize;
            offset += 6;
            if offset + path_len + content_len > data.len() {
                return Err(PackageError::InvalidFormat);
            }
            let filename = core::str::from_utf8(&data[offset..offset + path_len])
                .map_err(|_| PackageError::InvalidFormat)?;
            validate_payload_path(filename)?;
            offset += path_len;
            files.push((
                filename.to_string(),
                data[offset..offset + content_len].to_vec(),
            ));
            offset += content_len;
        }
        Ok(files)
    }
}

lazy_static::lazy_static! {
    static ref PACKAGE_MANAGER: Mutex<PackageManager> = Mutex::new(PackageManager::new());
}

pub fn get_package_manager() -> &'static Mutex<PackageManager> {
    &PACKAGE_MANAGER
}

pub fn install_package_from_path(path: &str) -> Result<String, PackageError> {
    let data = match store_request(StoreCommand::ReadFile {
        path: path.to_string(),
    }) {
        Ok(StoreResponse::FileData(d)) => d,
        Ok(StoreResponse::Error(_)) | Ok(_) | Err(_) => return Err(PackageError::IoError),
    };
    get_package_manager().lock().install_package(&data)
}

pub fn resolve_installed_app(query: &str) -> Option<InstalledPackagedApp> {
    get_package_manager().lock().installed_app(query)
}

pub fn resolve_installed_native_app(query: &str) -> Option<InstalledNativeApp> {
    get_package_manager().lock().installed_native_app(query)
}

pub fn verify_packaged_launch(entry_path: &str) -> Result<VerifiedPackagedLaunch, PackageError> {
    get_package_manager().lock().verify_packaged_launch(entry_path)
}

pub fn verify_native_launch(entry_path: &str) -> Result<VerifiedNativeLaunch, PackageError> {
    get_package_manager().lock().verify_native_launch(entry_path)
}

pub fn build_signed_bundle(
    source: &SourceAppManifest,
    compiled: &CompiledAppManifest,
    entry_bytes: &[u8],
    trust_domain: TrustDomain,
) -> Result<Vec<u8>, PackageError> {
    let source_text = render_source_manifest(source);
    let manifest_bytes = compiled
        .encode()
        .map_err(|_| PackageError::InvalidManifest)?;
    let entry_digest = sha256_array(entry_bytes);
    let manifest_digest = sha256_array(&manifest_bytes);
    let package_digest = canonical_package_digest(
        source_text.as_bytes(),
        &[
            (
                String::from(COMPILED_MANIFEST_PATH),
                compiled.encode().map_err(|_| PackageError::InvalidManifest)?,
            ),
            (compiled.entry.clone(), entry_bytes.to_vec()),
        ],
    )?;
    let signature_metadata = package_signature_metadata(
        trust_domain,
        manifest_digest,
        package_digest,
        entry_digest,
    );
    rebuild_bundle_bytes_with_metadata(
        source_text.as_bytes(),
        &[
            (
                String::from(COMPILED_MANIFEST_PATH),
                compiled.encode().map_err(|_| PackageError::InvalidManifest)?,
            ),
            (compiled.entry.clone(), entry_bytes.to_vec()),
        ],
        &signature_metadata,
    )
}

pub fn inspect_signed_bundle(bytes: &[u8]) -> Result<BundleInspection, PackageError> {
    let parsed = PackageManager::new().parse_signed_bundle(bytes)?;
    let source_text = core::str::from_utf8(&parsed.source_manifest)
        .map_err(|_| PackageError::InvalidManifest)?;
    let source_manifest =
        SourceAppManifest::parse(source_text).map_err(|_| PackageError::InvalidManifest)?;
    let compiled_bytes = lookup_payload(&parsed.payload_files, COMPILED_MANIFEST_PATH)
        .ok_or(PackageError::MissingPackagedPayload)?;
    let compiled_manifest =
        CompiledAppManifest::decode(&compiled_bytes).map_err(|_| PackageError::InvalidManifest)?;
    let signature_metadata = parsed
        .signature_metadata
        .ok_or(PackageError::SignatureMetadataMissing)?;
    Ok(BundleInspection {
        source_manifest,
        compiled_manifest,
        signature_metadata,
    })
}

pub fn build_revocation_feed(
    minimum_epoch: u32,
    revoked_signer_key_ids: &[String],
) -> Result<Vec<u8>, PackageError> {
    RevocationFeed {
        minimum_epoch,
        revoked_signer_key_ids: revoked_signer_key_ids.to_vec(),
    }
    .encode_signed()
}

fn store_request(command: StoreCommand) -> Result<StoreResponse, PackageError> {
    request_store_sync(0, command).ok_or(PackageError::IoError)
}

pub fn sign_dev_package(content: &[u8]) -> [u8; 64] {
    sign_with_seed(content, &DEV_PACKAGE_SIGNING_SEED)
}

fn sign_with_seed(content: &[u8], seed: &[u8; 32]) -> [u8; 64] {
    let mut signature = [0u8; 64];
    let mut r_hasher = crate::crypto::Sha3::sha3_512();
    r_hasher.update(seed);
    r_hasher.update(content);
    let r_hash = r_hasher.finalize();
    signature[..32].copy_from_slice(&r_hash[..32]);

    let mut k_hasher = crate::crypto::Sha3::sha3_512();
    k_hasher.update(&signature[..32]);
    k_hasher.update(seed);
    k_hasher.update(content);
    let k_hash = k_hasher.finalize();

    let mut s_hasher = crate::crypto::Sha3::sha3_256();
    s_hasher.update(&k_hash);
    s_hasher.update(&signature[..32]);
    s_hasher.update(seed);
    let s_hash = s_hasher.finalize();
    signature[32..].copy_from_slice(&s_hash[..32]);
    signature
}

fn sign_with_firmware_key_seed(
    content: &[u8],
    seed: &[u8; 32],
) -> Result<[u8; 64], PackageError> {
    let signing_key =
        P256SigningKey::from_slice(seed).map_err(|_| PackageError::InvalidSignature)?;
    let signature: P256Signature = signing_key.sign(content);
    let mut out = [0u8; 64];
    out.copy_from_slice(&signature.to_bytes());
    Ok(out)
}

fn verify_with_firmware_key_seed(content: &[u8], seed: &[u8; 32], signature: &[u8; 64]) -> bool {
    let Ok(signing_key) = P256SigningKey::from_slice(seed) else {
        return false;
    };
    let verifying_key = P256VerifyingKey::from(&signing_key);
    let Ok(signature) = P256Signature::from_slice(signature) else {
        return false;
    };
    verifying_key.verify(content, &signature).is_ok()
}

fn verify_package_signature(
    content: &[u8],
    signature: &[u8],
    metadata: &PackageSignatureMetadata,
) -> Result<(), PackageError> {
    if signature.len() != SIGNATURE_LEN {
        return Err(PackageError::InvalidSignature);
    }
    let root = trust_root_for(metadata)?;
    let feed = load_live_revocation_feed()?;
    if metadata.revocation_epoch < feed.minimum_epoch {
        return Err(PackageError::TrustRevoked);
    }
    if feed
        .revoked_signer_key_ids
        .iter()
        .any(|revoked| revoked == &metadata.signer_key_id)
    {
        return Err(PackageError::TrustRevoked);
    }
    let mut provided = [0u8; 64];
    provided.copy_from_slice(signature);
    let verified = match root.signature_algorithm {
        SignatureAlgorithm::Ed25519 => provided == sign_with_seed(content, root.signing_seed),
        SignatureAlgorithm::FirmwarePublishedKey => {
            verify_with_firmware_key_seed(content, root.signing_seed, &provided)
        }
    };
    if verified {
        Ok(())
    } else {
        Err(PackageError::InvalidSignature)
    }
}

fn firmware_trust_anchor_available() -> bool {
    crate::boot::secure_boot_enabled()
        && crate::posix::secure_boot_db_available()
        && crate::security::tpm::is_available()
}

fn trust_root_for(metadata: &PackageSignatureMetadata) -> Result<TrustRootRecord, PackageError> {
    let Some(root) = TRUST_ROOTS.iter().copied().find(|root| {
        root.signer_key_id == metadata.signer_key_id
            && root.trust_domain == metadata.trust_domain
            && root.signature_algorithm == metadata.signature_algorithm
    }) else {
        return Err(PackageError::InvalidSignature);
    };
    if root.requires_firmware_anchor && !firmware_trust_anchor_available() {
        return Err(PackageError::TrustRootUnavailable);
    }
    Ok(root)
}

fn trust_root_for_domain(trust_domain: TrustDomain) -> Option<TrustRootRecord> {
    TRUST_ROOTS
        .iter()
        .copied()
        .find(|root| root.trust_domain == trust_domain)
}

fn sign_with_trust_domain(content: &[u8], trust_domain: TrustDomain) -> Result<[u8; 64], PackageError> {
    let Some(root) = trust_root_for_domain(trust_domain) else {
        return Err(PackageError::InvalidSignature);
    };
    match root.signature_algorithm {
        SignatureAlgorithm::Ed25519 => Ok(sign_with_seed(content, root.signing_seed)),
        SignatureAlgorithm::FirmwarePublishedKey => sign_with_firmware_key_seed(content, root.signing_seed),
    }
}

fn load_live_revocation_feed() -> Result<RevocationFeed, PackageError> {
    let response = match store_request(StoreCommand::ReadFile {
        path: LIVE_REVOCATION_FEED_PATH.to_string(),
    }) {
        Ok(response) => response,
        Err(_) => return Ok(RevocationFeed::default()),
    };
    match response {
        StoreResponse::FileData(bytes) => RevocationFeed::decode_signed(&bytes),
        StoreResponse::Error(_) => Ok(RevocationFeed::default()),
        _ => Err(PackageError::TrustMetadataInvalid),
    }
}

fn validate_manifest_pair(
    source: &SourceAppManifest,
    compiled: &CompiledAppManifest,
) -> Result<(), PackageError> {
    if source.app_id != compiled.app_id
        || source.name != compiled.name
        || source.version != compiled.version
        || source.entry != compiled.entry
        || source.sdk_version != compiled.sdk_version
        || source.runtime != compiled.runtime
        || source.presentation != compiled.presentation
        || source.state_contract != compiled.state_contract
        || source.restart_policy != compiled.restart_policy
    {
        return Err(PackageError::InvalidManifest);
    }
    Ok(())
}

fn rebuild_bundle_bytes_with_metadata(
    source_manifest: &[u8],
    files: &[(String, Vec<u8>)],
    signature_metadata: &PackageSignatureMetadata,
) -> Result<Vec<u8>, PackageError> {
    let signature_bytes = signature_metadata
        .encode()
        .map_err(|_| PackageError::InvalidManifest)?;
    let mut full_files = Vec::with_capacity(files.len() + 1);
    full_files.push((String::from(SIGNATURE_METADATA_PATH), signature_bytes));
    for (path, content) in files {
        full_files.push((path.clone(), content.clone()));
    }
    let unsigned = build_unsigned_bundle(source_manifest, &full_files)?;
    let signature = sign_with_trust_domain(&unsigned, signature_metadata.trust_domain)?;
    let mut signed = unsigned;
    signed.extend_from_slice(&signature);
    Ok(signed)
}

fn build_unsigned_bundle(
    source_manifest: &[u8],
    files: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, PackageError> {
    let mut unsigned = Vec::new();
    unsigned.extend_from_slice(&MAGIC);
    unsigned.extend_from_slice(&(source_manifest.len() as u32).to_le_bytes());
    unsigned.extend_from_slice(source_manifest);
    for (path, content) in files {
        validate_payload_path(path)?;
        unsigned.extend_from_slice(&(path.len() as u16).to_le_bytes());
        unsigned.extend_from_slice(&(content.len() as u32).to_le_bytes());
        unsigned.extend_from_slice(path.as_bytes());
        unsigned.extend_from_slice(content);
    }
    Ok(unsigned)
}

fn canonical_package_digest(
    source_manifest: &[u8],
    files: &[(String, Vec<u8>)],
) -> Result<[u8; 32], PackageError> {
    let filtered = files
        .iter()
        .filter(|(path, _)| path != SIGNATURE_METADATA_PATH)
        .cloned()
        .collect::<Vec<_>>();
    let unsigned = build_unsigned_bundle(source_manifest, &filtered)?;
    Ok(sha256_array(&unsigned))
}

fn package_signature_metadata(
    trust_domain: TrustDomain,
    manifest_digest: [u8; 32],
    package_digest: [u8; 32],
    entry_digest: [u8; 32],
) -> PackageSignatureMetadata {
    let root = trust_root_for_domain(trust_domain);
    PackageSignatureMetadata {
        signer_key_id: root
            .map(|root| root.signer_key_id)
            .unwrap_or("unsigned")
            .to_string(),
        trust_domain,
        signature_algorithm: root
            .map(|root| root.signature_algorithm)
            .unwrap_or(SignatureAlgorithm::Ed25519),
        manifest_digest,
        package_digest,
        entry_digest,
        revocation_epoch: EMBEDDED_PLATFORM_REVOCATION_EPOCH,
    }
}

impl RevocationFeed {
    fn encode_unsigned(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&REVOCATION_FEED_MAGIC);
        out.extend_from_slice(&REVOCATION_FEED_VERSION.to_le_bytes());
        out.extend_from_slice(&self.minimum_epoch.to_le_bytes());
        out.extend_from_slice(&(self.revoked_signer_key_ids.len() as u16).to_le_bytes());
        for signer in &self.revoked_signer_key_ids {
            out.extend_from_slice(&(signer.len() as u16).to_le_bytes());
            out.extend_from_slice(signer.as_bytes());
        }
        out
    }

    fn encode_signed(&self) -> Result<Vec<u8>, PackageError> {
        let mut unsigned = self.encode_unsigned();
        let signature = sign_with_trust_domain(&unsigned, TrustDomain::Platform)?;
        unsigned.extend_from_slice(&signature);
        Ok(unsigned)
    }

    fn decode_signed(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() < REVOCATION_FEED_MAGIC.len() + 2 + 4 + 2 + SIGNATURE_LEN {
            return Err(PackageError::TrustMetadataInvalid);
        }
        let split = bytes.len() - SIGNATURE_LEN;
        let unsigned = &bytes[..split];
        let signature = bytes[split..]
            .try_into()
            .map_err(|_| PackageError::TrustMetadataInvalid)?;
        let Some(root) = trust_root_for_domain(TrustDomain::Platform) else {
            return Err(PackageError::TrustMetadataInvalid);
        };
        if !verify_with_firmware_key_seed(unsigned, root.signing_seed, &signature) {
            return Err(PackageError::TrustMetadataInvalid);
        }
        Self::decode_unsigned(unsigned)
    }

    fn decode_unsigned(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() < REVOCATION_FEED_MAGIC.len() + 2 + 4 + 2 {
            return Err(PackageError::TrustMetadataInvalid);
        }
        let mut cursor = 0usize;
        let magic = read_slice(bytes, &mut cursor, REVOCATION_FEED_MAGIC.len())?;
        if magic != REVOCATION_FEED_MAGIC {
            return Err(PackageError::TrustMetadataInvalid);
        }
        let version = u16::from_le_bytes(read_fixed::<2>(bytes, &mut cursor)?);
        if version != REVOCATION_FEED_VERSION {
            return Err(PackageError::TrustMetadataInvalid);
        }
        let minimum_epoch = u32::from_le_bytes(read_fixed::<4>(bytes, &mut cursor)?);
        let count = u16::from_le_bytes(read_fixed::<2>(bytes, &mut cursor)?) as usize;
        let mut revoked_signer_key_ids = EMBEDDED_REVOKED_SIGNER_KEY_IDS
            .iter()
            .map(|entry| (*entry).to_string())
            .collect::<Vec<_>>();
        for _ in 0..count {
            let length = u16::from_le_bytes(read_fixed::<2>(bytes, &mut cursor)?) as usize;
            let signer = core::str::from_utf8(read_slice(bytes, &mut cursor, length)?)
                .map_err(|_| PackageError::TrustMetadataInvalid)?
                .to_string();
            revoked_signer_key_ids.push(signer);
        }
        Ok(Self {
            minimum_epoch: minimum_epoch.max(EMBEDDED_PLATFORM_REVOCATION_EPOCH),
            revoked_signer_key_ids,
        })
    }
}

fn render_source_manifest(source: &SourceAppManifest) -> String {
    let capabilities = source
        .capabilities
        .iter()
        .map(|capability| alloc::format!("\"{}\"", capability.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let restart_policy = match source.restart_policy.kind {
        echos_manifest::CrashRestartPolicy::Never => String::from("never"),
        echos_manifest::CrashRestartPolicy::OnFaultOnce => String::from("on-fault-once"),
        echos_manifest::CrashRestartPolicy::BoundedRetry => {
            alloc::format!("bounded-retry:{}", source.restart_policy.retry_budget)
        }
    };
    alloc::format!(
        "app_id = \"{app_id}\"\nname = \"{name}\"\nversion = \"{version}\"\nentry = \"{entry}\"\nsdk_version = {sdk_version}\nruntime = \"{runtime}\"\npresentation = \"{presentation}\"\ncapabilities = [{capabilities}]\ndefault_window.title = \"{title}\"\ndefault_window.width = {width}\ndefault_window.height = {height}\nstate_contract = \"{state_contract}\"\nrestart_policy = \"{restart_policy}\"\n",
        app_id = source.app_id,
        name = source.name,
        version = source.version,
        entry = source.entry,
        sdk_version = source.sdk_version,
        runtime = source.runtime.as_str(),
        presentation = source.presentation.as_str(),
        capabilities = capabilities,
        title = source.default_window.title,
        width = source.default_window.width,
        height = source.default_window.height,
        state_contract = source.state_contract.as_str(),
        restart_policy = restart_policy,
    )
}

fn lookup_payload(files: &[(String, Vec<u8>)], path: &str) -> Option<Vec<u8>> {
    files.iter().find_map(|(candidate, bytes)| {
        if candidate == path {
            Some(bytes.clone())
        } else {
            None
        }
    })
}

fn bundle_archive_path(package_name: &str) -> String {
    let mut path = String::from(BUNDLE_STORE_ROOT);
    path.push('/');
    path.push_str(package_name);
    path.push_str(".bhd");
    path
}

fn bundle_root(package_name: &str) -> String {
    let mut path = String::from("/apps/");
    path.push_str(package_name);
    path
}

fn bundle_file_path(package_name: &str, relative: &str) -> String {
    let mut full_path = bundle_root(package_name);
    full_path.push('/');
    full_path.push_str(relative);
    full_path
}

fn hash_manifest_app_id(value: &str, runtime: AppRuntime) -> AppId {
    let mut hash = 0x811C_9DC5u32;
    for byte in value.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let tag = match runtime {
        AppRuntime::Native => 0x1000_0000,
        AppRuntime::Pe => 0x5000_0000,
        AppRuntime::Elf => 0x6000_0000,
        AppRuntime::Special => 0x7000_0000,
    };
    tag | (hash & 0x0FFF_FFFF)
}

fn leak_string(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn sha256_array(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn read_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], PackageError> {
    if *cursor + len > bytes.len() {
        return Err(PackageError::TrustMetadataInvalid);
    }
    let value = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(value)
}

fn read_fixed<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], PackageError> {
    let slice = read_slice(bytes, cursor, N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    Ok(out)
}

fn validate_payload_path(path: &str) -> Result<(), PackageError> {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return Err(PackageError::UnsafePayloadPath);
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(PackageError::UnsafePayloadPath);
    }
    if path.split('\\').count() > 1 || path.contains('\0') {
        return Err(PackageError::UnsafePayloadPath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_revocation_feed, build_signed_bundle, inspect_signed_bundle, PackageError,
        PackageManager, RevocationFeed, COMPILED_MANIFEST_PATH, SIGNATURE_METADATA_PATH,
    };
    use alloc::string::{String, ToString};
    use alloc::vec;
    use echos_manifest::{
        AppPresentation, AppRuntime, AppStateContract, CompiledAppManifest, DefaultWindow,
        RestartPolicy, SourceAppManifest, TrustDomain,
    };

    fn demo_source(runtime: AppRuntime, entry: &str) -> SourceAppManifest {
        SourceAppManifest {
            app_id: alloc::format!("org.echos.{entry}"),
            name: String::from("Hello"),
            version: String::from("0.1.0"),
            entry: entry.to_string(),
            sdk_version: 1,
            runtime,
            presentation: AppPresentation::Windowed,
            capabilities: vec![echos_manifest::NativeCapability::NotificationsPost],
            default_window: DefaultWindow {
                title: String::from("Hello"),
                width: 640,
                height: 480,
            },
            state_contract: AppStateContract::ColdResume,
            restart_policy: RestartPolicy::bounded_retry(2),
        }
    }

    #[test]
    fn signed_bundle_roundtrip_validates_signature_and_payload() {
        let source = demo_source(AppRuntime::Native, "hello.elf");
        let entry = b"fake-elf-image".to_vec();
        let compiled =
            CompiledAppManifest::from_source(&source, super::sha256_array(&entry)).expect("compiled");
        let bundle =
            build_signed_bundle(&source, &compiled, &entry, TrustDomain::Developer).expect("bundle");
        let parsed = PackageManager::new()
            .parse_signed_bundle(&bundle)
            .expect("signed bundle");
        assert!(parsed
            .payload_files
            .iter()
            .any(|(path, _)| path == COMPILED_MANIFEST_PATH));
        assert!(parsed
            .payload_files
            .iter()
            .any(|(path, _)| path == SIGNATURE_METADATA_PATH));
        let compiled_bytes = parsed
            .payload_files
            .iter()
            .find(|(path, _)| path == COMPILED_MANIFEST_PATH)
            .map(|(_, bytes)| bytes.clone())
            .expect("compiled payload");
        let decoded = CompiledAppManifest::decode(&compiled_bytes).expect("decode");
        assert_eq!(decoded.app_id, source.app_id);
    }

    #[test]
    fn tampered_bundle_fails_signature_verification() {
        let source = demo_source(AppRuntime::Native, "hello.elf");
        let entry = b"fake-elf-image".to_vec();
        let compiled =
            CompiledAppManifest::from_source(&source, super::sha256_array(&entry)).expect("compiled");
        let mut bundle =
            build_signed_bundle(&source, &compiled, &entry, TrustDomain::Developer).expect("bundle");
        let index = bundle.len().saturating_sub(70);
        bundle[index] ^= 0xAA;
        let err = PackageManager::new()
            .parse_signed_bundle(&bundle)
            .expect_err("signature should fail");
        assert!(matches!(err, PackageError::HashMismatch | PackageError::InvalidSignature));
    }

    #[test]
    fn inspection_reports_signature_metadata() {
        let source = demo_source(AppRuntime::Pe, "hello.exe");
        let entry = b"fake-pe-image".to_vec();
        let compiled =
            CompiledAppManifest::from_source(&source, super::sha256_array(&entry)).expect("compiled");
        let bundle =
            build_signed_bundle(&source, &compiled, &entry, TrustDomain::Developer).expect("bundle");
        let inspection = inspect_signed_bundle(&bundle).expect("inspect");
        assert_eq!(inspection.compiled_manifest.runtime, AppRuntime::Pe);
        assert_eq!(inspection.signature_metadata.trust_domain, TrustDomain::Developer);
    }

    #[test]
    fn revocation_feed_roundtrips_when_signed() {
        let feed = build_revocation_feed(9, &[String::from("store-root-v1")]).expect("feed");
        let decoded = RevocationFeed::decode_signed(&feed).expect("decode");
        assert_eq!(decoded.minimum_epoch, 9);
        assert!(decoded
            .revoked_signer_key_ids
            .iter()
            .any(|entry| entry == "store-root-v1"));
    }

    #[test]
    fn tampered_revocation_feed_is_rejected() {
        let mut feed = build_revocation_feed(2, &[String::from("platform-root-v1")]).expect("feed");
        let index = feed.len().saturating_sub(20);
        feed[index] ^= 0x44;
        let err = RevocationFeed::decode_signed(&feed).expect_err("tamper should fail");
        assert!(matches!(err, PackageError::TrustMetadataInvalid));
    }
}
