use super::super::gui::launch_pipeline::AppResolution;
use super::super::ipc::service_ipc::{
    get_service_ipc, CapabilityRights, ServiceId, ServiceMessage, ServiceResponse,
};
use super::super::security::capability::CapRights;
use super::super::update::{UpdateApplyReport, UpdateError, UpdateInspection};
use super::{
    launch_contract, native_scene_contract, package_registry_contract, process_broker_contract,
};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceParityStatus {
    pub required_services: u32,
    pub packaged_service_slots: u32,
    pub live_user_process_slots: u32,
    pub published_user_process_slots: u32,
    pub strict_mode_enabled: bool,
    pub full_parity_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceClass {
    Directory,
    Network,
    Ui,
    Input,
    Media,
    Storage,
    Session,
    Integration,
    Runtime,
    Package,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceDescriptor {
    pub id: ServiceId,
    pub name: String,
    pub class: ServiceClass,
    pub control_plane: bool,
    pub bulk_data_out_of_band: bool,
    pub openable_rights: CapabilityRights,
    pub bulk_region_classes: Vec<String>,
    pub service_process_available: bool,
    pub runtime_isolation: Option<launch_contract::IsolationDomain>,
    pub runtime_task_id: Option<u64>,
    pub runtime_image_path: Option<String>,
    pub user_published_endpoint: bool,
}

#[derive(Clone, Debug)]
pub enum DirectoryCommand {
    ListServices,
    DescribeService(ServiceId),
}

#[derive(Clone, Debug)]
pub enum DirectoryResponse {
    Services(Vec<ServiceDescriptor>),
    Service(Option<ServiceDescriptor>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageRegistryCommand {
    ListEntries,
    DescribeQuery(String),
    ResolveFileAssociation(String),
    ListPackages,
    DescribePackage(String),
    SearchPackages(String),
    InstallBundle(Vec<u8>),
    InstallFromPath(String),
    RemovePackage(String),
    VerifyPackage(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageLifecycleAction {
    Install,
    Remove,
    Verify,
}

impl PackageLifecycleAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Verify => "verify",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageRecord {
    pub name: String,
    pub info: crate::security::package::PackageInfo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageLifecycleReport {
    pub action: PackageLifecycleAction,
    pub subject: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageRegistryErrorKind {
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
    ServiceUnavailable,
    RightsDenied,
    Revoked,
    StaleGeneration,
    QueueFull,
    SyncCycleRisk,
    EndpointRestarted,
    WrongService,
    WrongResponseKind,
    SharedRegionUnavailable,
}

impl PackageRegistryErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidMagic => "invalid-magic",
            Self::InvalidFormat => "invalid-format",
            Self::InvalidSignature => "invalid-signature",
            Self::InvalidManifest => "invalid-manifest",
            Self::UnsupportedAbi => "unsupported-abi",
            Self::HashMismatch => "hash-mismatch",
            Self::IoError => "io-error",
            Self::PackageExists => "package-exists",
            Self::PackageNotFound => "package-not-found",
            Self::PermissionDenied => "permission-denied",
            Self::RepositoryUnavailable => "repository-unavailable",
            Self::UnsafePayloadPath => "unsafe-payload-path",
            Self::EmptyPayload => "empty-payload",
            Self::MissingPackagedPayload => "missing-packaged-payload",
            Self::SignatureMetadataMissing => "signature-metadata-missing",
            Self::TrustRootUnavailable => "trust-root-unavailable",
            Self::TrustRevoked => "trust-revoked",
            Self::TrustMetadataInvalid => "trust-metadata-invalid",
            Self::RuntimeMismatch => "runtime-mismatch",
            Self::ServiceUnavailable => "service-unavailable",
            Self::RightsDenied => "rights-denied",
            Self::Revoked => "revoked",
            Self::StaleGeneration => "stale-generation",
            Self::QueueFull => "queue-full",
            Self::SyncCycleRisk => "sync-cycle-risk",
            Self::EndpointRestarted => "endpoint-restarted",
            Self::WrongService => "wrong-service",
            Self::WrongResponseKind => "wrong-response-kind",
            Self::SharedRegionUnavailable => "shared-region-unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageRegistryError {
    pub kind: PackageRegistryErrorKind,
    pub detail: String,
}

impl PackageRegistryError {
    fn new(kind: PackageRegistryErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    fn from_package_error(err: crate::security::package::PackageError) -> Self {
        let kind = match err {
            crate::security::package::PackageError::InvalidMagic => {
                PackageRegistryErrorKind::InvalidMagic
            }
            crate::security::package::PackageError::InvalidFormat => {
                PackageRegistryErrorKind::InvalidFormat
            }
            crate::security::package::PackageError::InvalidSignature => {
                PackageRegistryErrorKind::InvalidSignature
            }
            crate::security::package::PackageError::InvalidManifest => {
                PackageRegistryErrorKind::InvalidManifest
            }
            crate::security::package::PackageError::UnsupportedAbi => {
                PackageRegistryErrorKind::UnsupportedAbi
            }
            crate::security::package::PackageError::HashMismatch => {
                PackageRegistryErrorKind::HashMismatch
            }
            crate::security::package::PackageError::IoError => PackageRegistryErrorKind::IoError,
            crate::security::package::PackageError::PackageExists => {
                PackageRegistryErrorKind::PackageExists
            }
            crate::security::package::PackageError::PackageNotFound => {
                PackageRegistryErrorKind::PackageNotFound
            }
            crate::security::package::PackageError::PermissionDenied => {
                PackageRegistryErrorKind::PermissionDenied
            }
            crate::security::package::PackageError::RepositoryUnavailable => {
                PackageRegistryErrorKind::RepositoryUnavailable
            }
            crate::security::package::PackageError::UnsafePayloadPath => {
                PackageRegistryErrorKind::UnsafePayloadPath
            }
            crate::security::package::PackageError::EmptyPayload => {
                PackageRegistryErrorKind::EmptyPayload
            }
            crate::security::package::PackageError::MissingPackagedPayload => {
                PackageRegistryErrorKind::MissingPackagedPayload
            }
            crate::security::package::PackageError::SignatureMetadataMissing => {
                PackageRegistryErrorKind::SignatureMetadataMissing
            }
            crate::security::package::PackageError::TrustRootUnavailable => {
                PackageRegistryErrorKind::TrustRootUnavailable
            }
            crate::security::package::PackageError::TrustRevoked => {
                PackageRegistryErrorKind::TrustRevoked
            }
            crate::security::package::PackageError::TrustMetadataInvalid => {
                PackageRegistryErrorKind::TrustMetadataInvalid
            }
            crate::security::package::PackageError::RuntimeMismatch => {
                PackageRegistryErrorKind::RuntimeMismatch
            }
        };
        Self::new(kind, err.to_string())
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::new(PackageRegistryErrorKind::ServiceUnavailable, detail)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageRegistryResponse {
    Entries(Vec<package_registry_contract::PackageRegistryEntry>),
    Entry(Option<package_registry_contract::PackageRegistryEntry>),
    Resolution(Option<AppResolution>),
    Packages(Vec<PackageRecord>),
    Package(Option<PackageRecord>),
    Lifecycle(PackageLifecycleReport),
    Error(PackageRegistryError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessBrokerCommand {
    DescribeLaunch(process_broker_contract::ProcessBrokerTicket),
    DescribeChildren(process_broker_contract::ProcessBrokerTicket),
    DescribeRuntimeTask(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessBrokerResponse {
    Launch(Option<process_broker_contract::BrokeredLaunch>),
    Children(Vec<process_broker_contract::ProcessBrokerTicket>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateInstallerCommand {
    Inspect(String),
    Apply(String),
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateInstallerErrorKind {
    InvalidMagic,
    UnsupportedVersion,
    InvalidFormat,
    InvalidSignature,
    InvalidManifestDigest,
    ManifestTooLarge,
    StoreIo,
    NetworkUnavailable,
    NoBlockDevice,
    TargetPartitionNotFound,
    ArtifactTooLarge,
    ArtifactDigestMismatch,
    Package,
    Plan,
    ServiceUnavailable,
}

impl UpdateInstallerErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidMagic => "invalid-magic",
            Self::UnsupportedVersion => "unsupported-version",
            Self::InvalidFormat => "invalid-format",
            Self::InvalidSignature => "invalid-signature",
            Self::InvalidManifestDigest => "invalid-manifest-digest",
            Self::ManifestTooLarge => "manifest-too-large",
            Self::StoreIo => "store-io",
            Self::NetworkUnavailable => "network-unavailable",
            Self::NoBlockDevice => "no-block-device",
            Self::TargetPartitionNotFound => "target-partition-not-found",
            Self::ArtifactTooLarge => "artifact-too-large",
            Self::ArtifactDigestMismatch => "artifact-digest-mismatch",
            Self::Package => "package-failure",
            Self::Plan => "plan-failure",
            Self::ServiceUnavailable => "service-unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInstallerError {
    pub kind: UpdateInstallerErrorKind,
    pub detail: String,
}

impl UpdateInstallerError {
    fn new(kind: UpdateInstallerErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    fn from_update_error(err: UpdateError) -> Self {
        let kind = match err {
            UpdateError::InvalidMagic => UpdateInstallerErrorKind::InvalidMagic,
            UpdateError::UnsupportedVersion => UpdateInstallerErrorKind::UnsupportedVersion,
            UpdateError::InvalidFormat => UpdateInstallerErrorKind::InvalidFormat,
            UpdateError::InvalidSignature => UpdateInstallerErrorKind::InvalidSignature,
            UpdateError::InvalidManifestDigest => UpdateInstallerErrorKind::InvalidManifestDigest,
            UpdateError::ManifestTooLarge => UpdateInstallerErrorKind::ManifestTooLarge,
            UpdateError::StoreIo => UpdateInstallerErrorKind::StoreIo,
            UpdateError::NetworkUnavailable => UpdateInstallerErrorKind::NetworkUnavailable,
            UpdateError::NoBlockDevice => UpdateInstallerErrorKind::NoBlockDevice,
            UpdateError::TargetPartitionNotFound { .. } => {
                UpdateInstallerErrorKind::TargetPartitionNotFound
            }
            UpdateError::ArtifactTooLarge { .. } => UpdateInstallerErrorKind::ArtifactTooLarge,
            UpdateError::ArtifactDigestMismatch { .. } => {
                UpdateInstallerErrorKind::ArtifactDigestMismatch
            }
            UpdateError::Package(_) => UpdateInstallerErrorKind::Package,
            UpdateError::Plan(_) => UpdateInstallerErrorKind::Plan,
        };
        Self::new(kind, err.to_string())
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::new(UpdateInstallerErrorKind::ServiceUnavailable, detail)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateInstallerResponse {
    Inspection(UpdateInspection),
    Apply(UpdateApplyReport),
    Status(Option<UpdateApplyReport>),
    Error(UpdateInstallerError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkBrokerCommand {
    Download(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkBrokerErrorKind {
    InvalidUrl,
    InvalidResponse,
    ConnectionFailed,
    Timeout,
    TooManyRedirects,
    NotFound,
    ServerError,
    ProxyAuthenticationRequired,
    InvalidHeader,
    ChunkedEncoding,
    ContentLength,
    TlsHandshakeFailed,
    TlsDecodeFailed,
    TlsCertDateInvalid,
    TlsCertCnInvalid,
    TlsInvalidCa,
    TlsInvalidCertificate,
    TlsCertRevoked,
    NoInterface,
    NotUp,
    BufferFull,
    BufferEmpty,
    InvalidPacket,
    InvalidFd,
    InvalidParam,
    ChecksumError,
    ConnectionRefused,
    ConnectionReset,
    ConnectionClosed,
    WouldBlock,
    AddrInUse,
    AddrNotAvailable,
    NetworkUnreachable,
    HostUnreachable,
    ProtocolError,
    NotSupported,
    NotConnected,
    Unknown,
    ServiceUnavailable,
}

impl NetworkBrokerErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUrl => "invalid-url",
            Self::InvalidResponse => "invalid-response",
            Self::ConnectionFailed => "connection-failed",
            Self::Timeout => "timeout",
            Self::TooManyRedirects => "too-many-redirects",
            Self::NotFound => "not-found",
            Self::ServerError => "server-error",
            Self::ProxyAuthenticationRequired => "proxy-auth-required",
            Self::InvalidHeader => "invalid-header",
            Self::ChunkedEncoding => "chunked-encoding",
            Self::ContentLength => "content-length",
            Self::TlsHandshakeFailed => "tls-handshake-failed",
            Self::TlsDecodeFailed => "tls-decode-failed",
            Self::TlsCertDateInvalid => "tls-cert-date-invalid",
            Self::TlsCertCnInvalid => "tls-cert-cn-invalid",
            Self::TlsInvalidCa => "tls-invalid-ca",
            Self::TlsInvalidCertificate => "tls-invalid-certificate",
            Self::TlsCertRevoked => "tls-cert-revoked",
            Self::NoInterface => "no-interface",
            Self::NotUp => "not-up",
            Self::BufferFull => "buffer-full",
            Self::BufferEmpty => "buffer-empty",
            Self::InvalidPacket => "invalid-packet",
            Self::InvalidFd => "invalid-fd",
            Self::InvalidParam => "invalid-param",
            Self::ChecksumError => "checksum-error",
            Self::ConnectionRefused => "connection-refused",
            Self::ConnectionReset => "connection-reset",
            Self::ConnectionClosed => "connection-closed",
            Self::WouldBlock => "would-block",
            Self::AddrInUse => "addr-in-use",
            Self::AddrNotAvailable => "addr-not-available",
            Self::NetworkUnreachable => "network-unreachable",
            Self::HostUnreachable => "host-unreachable",
            Self::ProtocolError => "protocol-error",
            Self::NotSupported => "not-supported",
            Self::NotConnected => "not-connected",
            Self::Unknown => "unknown",
            Self::ServiceUnavailable => "service-unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkBrokerError {
    pub kind: NetworkBrokerErrorKind,
    pub detail: String,
}

impl NetworkBrokerError {
    fn new(kind: NetworkBrokerErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    fn from_http_error(err: crate::net::http::HttpError) -> Self {
        use crate::net::http::HttpError;

        let kind = match err {
            HttpError::Network(net) => match net {
                crate::net::NetError::NoInterface => NetworkBrokerErrorKind::NoInterface,
                crate::net::NetError::NotUp => NetworkBrokerErrorKind::NotUp,
                crate::net::NetError::BufferFull => NetworkBrokerErrorKind::BufferFull,
                crate::net::NetError::BufferEmpty => NetworkBrokerErrorKind::BufferEmpty,
                crate::net::NetError::InvalidPacket => NetworkBrokerErrorKind::InvalidPacket,
                crate::net::NetError::InvalidFd => NetworkBrokerErrorKind::InvalidFd,
                crate::net::NetError::InvalidParam => NetworkBrokerErrorKind::InvalidParam,
                crate::net::NetError::ChecksumError => NetworkBrokerErrorKind::ChecksumError,
                crate::net::NetError::Timeout => NetworkBrokerErrorKind::Timeout,
                crate::net::NetError::ConnectionRefused => {
                    NetworkBrokerErrorKind::ConnectionRefused
                }
                crate::net::NetError::ConnectionReset => NetworkBrokerErrorKind::ConnectionReset,
                crate::net::NetError::ConnectionClosed => NetworkBrokerErrorKind::ConnectionClosed,
                crate::net::NetError::WouldBlock => NetworkBrokerErrorKind::WouldBlock,
                crate::net::NetError::AddrInUse => NetworkBrokerErrorKind::AddrInUse,
                crate::net::NetError::AddrNotAvailable => NetworkBrokerErrorKind::AddrNotAvailable,
                crate::net::NetError::NetworkUnreachable => {
                    NetworkBrokerErrorKind::NetworkUnreachable
                }
                crate::net::NetError::HostUnreachable => NetworkBrokerErrorKind::HostUnreachable,
                crate::net::NetError::ProtocolError => NetworkBrokerErrorKind::ProtocolError,
                crate::net::NetError::NotSupported => NetworkBrokerErrorKind::NotSupported,
                crate::net::NetError::NotConnected => NetworkBrokerErrorKind::NotConnected,
                crate::net::NetError::Unknown => NetworkBrokerErrorKind::Unknown,
            },
            HttpError::InvalidUrl => NetworkBrokerErrorKind::InvalidUrl,
            HttpError::InvalidResponse => NetworkBrokerErrorKind::InvalidResponse,
            HttpError::ConnectionFailed => NetworkBrokerErrorKind::ConnectionFailed,
            HttpError::Timeout => NetworkBrokerErrorKind::Timeout,
            HttpError::TooManyRedirects => NetworkBrokerErrorKind::TooManyRedirects,
            HttpError::NotFound => NetworkBrokerErrorKind::NotFound,
            HttpError::ServerError => NetworkBrokerErrorKind::ServerError,
            HttpError::ProxyAuthenticationRequired => {
                NetworkBrokerErrorKind::ProxyAuthenticationRequired
            }
            HttpError::InvalidHeader => NetworkBrokerErrorKind::InvalidHeader,
            HttpError::ChunkedEncoding => NetworkBrokerErrorKind::ChunkedEncoding,
            HttpError::ContentLength => NetworkBrokerErrorKind::ContentLength,
            HttpError::TlsHandshakeFailed => NetworkBrokerErrorKind::TlsHandshakeFailed,
            HttpError::TlsDecodeFailed => NetworkBrokerErrorKind::TlsDecodeFailed,
            HttpError::TlsCertDateInvalid => NetworkBrokerErrorKind::TlsCertDateInvalid,
            HttpError::TlsCertCnInvalid => NetworkBrokerErrorKind::TlsCertCnInvalid,
            HttpError::TlsInvalidCa => NetworkBrokerErrorKind::TlsInvalidCa,
            HttpError::TlsInvalidCertificate => NetworkBrokerErrorKind::TlsInvalidCertificate,
            HttpError::TlsCertRevoked => NetworkBrokerErrorKind::TlsCertRevoked,
        };

        Self::new(kind, alloc::format!("{:?}", err))
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::new(NetworkBrokerErrorKind::ServiceUnavailable, detail)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkBrokerResponse {
    Payload(Vec<u8>),
    Error(NetworkBrokerError),
}

const REQUIRED_PARITY_SERVICES: [ServiceId; 9] = [
    ServiceId::EchDisplay,
    ServiceId::EchInput,
    ServiceId::EchAudio,
    ServiceId::EchStore,
    ServiceId::EchShell,
    ServiceId::EchNotifications,
    ServiceId::EchClipboard,
    ServiceId::EchDialogs,
    ServiceId::EchCapture,
];

static FULL_PARITY_STRICT_MODE: AtomicBool = AtomicBool::new(false);

pub(crate) fn dispatch_directory_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::DirectoryCommand(command) => {
            ServiceResponse::DirectoryResponse(match command {
                DirectoryCommand::ListServices => DirectoryResponse::Services(service_directory()),
                DirectoryCommand::DescribeService(service_id) => {
                    DirectoryResponse::Service(service_descriptor(service_id))
                }
            })
        }
        _ => ServiceResponse::DirectoryResponse(DirectoryResponse::Service(None)),
    }
}

pub(crate) fn dispatch_package_registry_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::PackageRegistryCommand(command) => {
            let registry_records =
                super::super::gfx::velvet_glove_registry::desktop_launch_registry();
            let registry =
                package_registry_contract::RuntimePackageRegistry::new(&registry_records);
            ServiceResponse::PackageRegistryResponse(match command {
                PackageRegistryCommand::ListEntries => {
                    PackageRegistryResponse::Entries(registry.entries())
                }
                PackageRegistryCommand::DescribeQuery(query) => {
                    PackageRegistryResponse::Entry(registry.describe(query.as_str()))
                }
                PackageRegistryCommand::ResolveFileAssociation(path) => {
                    PackageRegistryResponse::Resolution(
                        registry.resolve_file_association(path.as_str()),
                    )
                }
                PackageRegistryCommand::ListPackages => {
                    let packages = crate::security::package::get_package_manager()
                        .lock()
                        .list_packages()
                        .into_iter()
                        .map(|(name, info)| PackageRecord { name, info })
                        .collect();
                    PackageRegistryResponse::Packages(packages)
                }
                PackageRegistryCommand::DescribePackage(name) => {
                    let info = crate::security::package::get_package_manager()
                        .lock()
                        .get_package_info(name.as_str());
                    PackageRegistryResponse::Package(info.map(|info| PackageRecord { name, info }))
                }
                PackageRegistryCommand::SearchPackages(term) => {
                    let packages = crate::security::package::get_package_manager()
                        .lock()
                        .search_packages(term.as_str())
                        .into_iter()
                        .map(|(name, info)| PackageRecord { name, info })
                        .collect();
                    PackageRegistryResponse::Packages(packages)
                }
                PackageRegistryCommand::InstallBundle(bytes) => {
                    match crate::security::package::install_bundle(bytes.as_slice()) {
                        Ok(detail) => PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                            action: PackageLifecycleAction::Install,
                            subject: String::from("inline-bundle"),
                            detail,
                        }),
                        Err(err) => PackageRegistryResponse::Error(
                            PackageRegistryError::from_package_error(err),
                        ),
                    }
                }
                PackageRegistryCommand::InstallFromPath(path) => {
                    match crate::security::package::install_package_from_path(path.as_str()) {
                        Ok(detail) => PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                            action: PackageLifecycleAction::Install,
                            subject: path,
                            detail,
                        }),
                        Err(err) => PackageRegistryResponse::Error(
                            PackageRegistryError::from_package_error(err),
                        ),
                    }
                }
                PackageRegistryCommand::RemovePackage(name) => {
                    match crate::security::package::remove_installed_package(name.as_str()) {
                        Ok(()) => PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                            action: PackageLifecycleAction::Remove,
                            subject: name,
                            detail: String::from("paket kaldirildi"),
                        }),
                        Err(err) => PackageRegistryResponse::Error(
                            PackageRegistryError::from_package_error(err),
                        ),
                    }
                }
                PackageRegistryCommand::VerifyPackage(name) => {
                    match crate::security::package::get_package_manager()
                        .lock()
                        .verify_installed_package(name.as_str())
                    {
                        Ok(()) => PackageRegistryResponse::Lifecycle(PackageLifecycleReport {
                            action: PackageLifecycleAction::Verify,
                            subject: name,
                            detail: String::from("paket butunlugu dogrulandi"),
                        }),
                        Err(err) => PackageRegistryResponse::Error(
                            PackageRegistryError::from_package_error(err),
                        ),
                    }
                }
            })
        }
        _ => ServiceResponse::PackageRegistryResponse(PackageRegistryResponse::Error(
            PackageRegistryError::service_unavailable("unexpected package registry message"),
        )),
    }
}

pub(crate) fn dispatch_process_broker_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::ProcessBrokerCommand(command) => {
            ServiceResponse::ProcessBrokerResponse(match command {
                ProcessBrokerCommand::DescribeLaunch(ticket) => {
                    ProcessBrokerResponse::Launch(process_broker_contract::brokered_launch(ticket))
                }
                ProcessBrokerCommand::DescribeChildren(ticket) => ProcessBrokerResponse::Children(
                    process_broker_contract::brokered_launch_children(ticket),
                ),
                ProcessBrokerCommand::DescribeRuntimeTask(task_id) => {
                    ProcessBrokerResponse::Launch(
                        native_scene_contract::runtime_handle_for_task(task_id).and_then(
                            |runtime| {
                                process_broker_contract::brokered_launch(runtime.broker_ticket)
                            },
                        ),
                    )
                }
            })
        }
        _ => ServiceResponse::ProcessBrokerResponse(ProcessBrokerResponse::Launch(None)),
    }
}

pub(crate) fn dispatch_update_installer_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::UpdateInstallerCommand(command) => {
            ServiceResponse::UpdateInstallerResponse(match command {
                UpdateInstallerCommand::Inspect(locator) => {
                    match crate::update::inspect_update_source(locator.as_str()) {
                        Ok(inspection) => UpdateInstallerResponse::Inspection(inspection),
                        Err(err) => UpdateInstallerResponse::Error(
                            UpdateInstallerError::from_update_error(err),
                        ),
                    }
                }
                UpdateInstallerCommand::Apply(locator) => {
                    match crate::update::apply_update_source(locator.as_str()) {
                        Ok(report) => UpdateInstallerResponse::Apply(report),
                        Err(err) => UpdateInstallerResponse::Error(
                            UpdateInstallerError::from_update_error(err),
                        ),
                    }
                }
                UpdateInstallerCommand::Status => {
                    UpdateInstallerResponse::Status(crate::update::last_apply_report())
                }
            })
        }
        _ => ServiceResponse::UpdateInstallerResponse(UpdateInstallerResponse::Error(
            UpdateInstallerError::service_unavailable("unexpected update installer message"),
        )),
    }
}

pub(crate) fn dispatch_network_broker_command(message: ServiceMessage) -> ServiceResponse {
    match message {
        ServiceMessage::NetworkBrokerCommand(command) => {
            ServiceResponse::NetworkBrokerResponse(match command {
                NetworkBrokerCommand::Download(locator) => {
                    match crate::net::http::HttpClient::new().download(locator.as_str()) {
                        Ok(bytes) => NetworkBrokerResponse::Payload(bytes),
                        Err(err) => {
                            NetworkBrokerResponse::Error(NetworkBrokerError::from_http_error(err))
                        }
                    }
                }
            })
        }
        _ => ServiceResponse::NetworkBrokerResponse(NetworkBrokerResponse::Error(
            NetworkBrokerError::service_unavailable("unexpected network broker message"),
        )),
    }
}

pub fn describe_service(service: ServiceId) -> Option<ServiceDescriptor> {
    service_descriptor(service)
}

pub fn service_parity_status() -> ServiceParityStatus {
    compute_service_parity_status()
}

pub fn refresh_full_parity_mode() -> ServiceParityStatus {
    let status = compute_service_parity_status();
    let strict = status.packaged_service_slots == status.required_services;
    FULL_PARITY_STRICT_MODE.store(strict, Ordering::Release);
    ServiceParityStatus {
        strict_mode_enabled: strict,
        ..status
    }
}

pub fn strict_full_parity_mode_enabled() -> bool {
    FULL_PARITY_STRICT_MODE.load(Ordering::Acquire)
}

pub(crate) fn set_full_parity_strict_mode_for_tests(strict: bool) {
    FULL_PARITY_STRICT_MODE.store(strict, Ordering::Release);
}

pub fn request_directory_sync(app_id: u32, command: DirectoryCommand) -> Option<DirectoryResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::Directory,
        ServiceMessage::DirectoryCommand(command),
        "request_directory_sync",
    ) {
        ServiceResponse::DirectoryResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_package_registry_sync(
    app_id: u32,
    command: PackageRegistryCommand,
) -> Option<PackageRegistryResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::PackageRegistry,
        ServiceMessage::PackageRegistryCommand(command),
        "request_package_registry_sync",
    ) {
        ServiceResponse::PackageRegistryResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_process_broker_sync(
    app_id: u32,
    command: ProcessBrokerCommand,
) -> Option<ProcessBrokerResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::ProcessBroker,
        ServiceMessage::ProcessBrokerCommand(command),
        "request_process_broker_sync",
    ) {
        ServiceResponse::ProcessBrokerResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_update_installer_sync(
    app_id: u32,
    command: UpdateInstallerCommand,
) -> Option<UpdateInstallerResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::UpdateInstaller,
        ServiceMessage::UpdateInstallerCommand(command),
        "request_update_installer_sync",
    ) {
        ServiceResponse::UpdateInstallerResponse(response) => Some(response),
        _ => None,
    }
}

pub fn request_network_broker_sync(
    app_id: u32,
    command: NetworkBrokerCommand,
) -> Option<NetworkBrokerResponse> {
    match get_service_ipc().request_sync_compat(
        app_id,
        ServiceId::NetworkBroker,
        ServiceMessage::NetworkBrokerCommand(command),
        "request_network_broker_sync",
    ) {
        ServiceResponse::NetworkBrokerResponse(response) => Some(response),
        _ => None,
    }
}

fn service_directory() -> Vec<ServiceDescriptor> {
    fn runtime_fields(
        service_id: ServiceId,
        service_slug: &str,
    ) -> (
        bool,
        Option<launch_contract::IsolationDomain>,
        Option<u64>,
        Option<String>,
        bool,
    ) {
        let available = launch_contract::service_process_available(service_slug);
        let runtime = process_broker_contract::runtime_handle_for_service(service_id);
        let published = get_service_ipc()
            .published_user_endpoint(service_id)
            .is_some();
        (
            available,
            runtime.as_ref().map(|handle| handle.isolation_domain),
            runtime.as_ref().and_then(|handle| handle.task_id),
            runtime.and_then(|handle| handle.image_path),
            published,
        )
    }

    let (display_process, display_iso, display_task, display_image, display_published) =
        runtime_fields(ServiceId::EchDisplay, "ech_display");
    let (input_process, input_iso, input_task, input_image, input_published) =
        runtime_fields(ServiceId::EchInput, "ech_input");
    let (audio_process, audio_iso, audio_task, audio_image, audio_published) =
        runtime_fields(ServiceId::EchAudio, "ech_audio");
    let (store_process, store_iso, store_task, store_image, store_published) =
        runtime_fields(ServiceId::EchStore, "ech_store");
    let (shell_process, shell_iso, shell_task, shell_image, shell_published) =
        runtime_fields(ServiceId::EchShell, "ech_shell");
    let (
        notifications_process,
        notifications_iso,
        notifications_task,
        notifications_image,
        notifications_published,
    ) = runtime_fields(ServiceId::EchNotifications, "ech_notifications");
    let (clipboard_process, clipboard_iso, clipboard_task, clipboard_image, clipboard_published) =
        runtime_fields(ServiceId::EchClipboard, "ech_clipboard");
    let (dialogs_process, dialogs_iso, dialogs_task, dialogs_image, dialogs_published) =
        runtime_fields(ServiceId::EchDialogs, "ech_dialogs");
    let (capture_process, capture_iso, capture_task, capture_image, capture_published) =
        runtime_fields(ServiceId::EchCapture, "ech_capture");

    vec![
        ServiceDescriptor {
            id: ServiceId::Directory,
            name: String::from("ServiceDirectory"),
            class: ServiceClass::Directory,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::NetworkBroker,
            name: String::from("NetworkBroker"),
            class: ServiceClass::Network,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::PackageRegistry,
            name: String::from("PackageRegistry"),
            class: ServiceClass::Package,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::ProcessBroker,
            name: String::from("ProcessBroker"),
            class: ServiceClass::Runtime,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        ServiceDescriptor {
            id: ServiceId::UpdateInstaller,
            name: String::from("UpdateInstaller"),
            class: ServiceClass::Package,
            control_plane: true,
            bulk_data_out_of_band: false,
            openable_rights: CapRights::READ_WRITE,
            bulk_region_classes: Vec::new(),
            service_process_available: false,
            runtime_isolation: Some(launch_contract::IsolationDomain::KernelTask),
            runtime_task_id: None,
            runtime_image_path: None,
            user_published_endpoint: false,
        },
        service_descriptor_entry(
            ServiceId::EchDisplay,
            "EchDisplay",
            ServiceClass::Ui,
            true,
            CapRights::READ_WRITE,
            vec![String::from("surface")],
            display_process,
            display_iso,
            display_task,
            display_image,
            display_published,
        ),
        service_descriptor_entry(
            ServiceId::EchInput,
            "EchInput",
            ServiceClass::Input,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            input_process,
            input_iso,
            input_task,
            input_image,
            input_published,
        ),
        service_descriptor_entry(
            ServiceId::EchAudio,
            "EchAudio",
            ServiceClass::Media,
            true,
            CapRights::READ_WRITE,
            vec![String::from("audio-stream")],
            audio_process,
            audio_iso,
            audio_task,
            audio_image,
            audio_published,
        ),
        service_descriptor_entry(
            ServiceId::EchStore,
            "EchStore",
            ServiceClass::Storage,
            true,
            CapRights::READ_WRITE,
            vec![String::from("file-io")],
            store_process,
            store_iso,
            store_task,
            store_image,
            store_published,
        ),
        service_descriptor_entry(
            ServiceId::EchShell,
            "EchShell",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            shell_process,
            shell_iso,
            shell_task,
            shell_image,
            shell_published,
        ),
        service_descriptor_entry(
            ServiceId::EchNotifications,
            "EchNotifications",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            notifications_process,
            notifications_iso,
            notifications_task,
            notifications_image,
            notifications_published,
        ),
        service_descriptor_entry(
            ServiceId::EchClipboard,
            "EchClipboard",
            ServiceClass::Session,
            true,
            CapRights::READ_WRITE,
            vec![String::from("clipboard-payload")],
            clipboard_process,
            clipboard_iso,
            clipboard_task,
            clipboard_image,
            clipboard_published,
        ),
        service_descriptor_entry(
            ServiceId::EchDialogs,
            "EchDialogs",
            ServiceClass::Session,
            false,
            CapRights::READ_WRITE,
            Vec::new(),
            dialogs_process,
            dialogs_iso,
            dialogs_task,
            dialogs_image,
            dialogs_published,
        ),
        service_descriptor_entry(
            ServiceId::EchCapture,
            "EchCapture",
            ServiceClass::Integration,
            true,
            CapRights::READ_WRITE,
            vec![String::from("screenshot")],
            capture_process,
            capture_iso,
            capture_task,
            capture_image,
            capture_published,
        ),
    ]
}

fn service_descriptor(service_id: ServiceId) -> Option<ServiceDescriptor> {
    service_directory()
        .into_iter()
        .find(|descriptor| descriptor.id == service_id)
}

fn compute_service_parity_status() -> ServiceParityStatus {
    let descriptors = service_directory();
    let required_services = REQUIRED_PARITY_SERVICES.len() as u32;
    let mut packaged_service_slots = 0u32;
    let mut live_user_process_slots = 0u32;
    let mut published_user_process_slots = 0u32;

    for service_id in REQUIRED_PARITY_SERVICES {
        let Some(descriptor) = descriptors
            .iter()
            .find(|descriptor| descriptor.id == service_id)
        else {
            continue;
        };
        if descriptor.service_process_available {
            packaged_service_slots = packaged_service_slots.saturating_add(1);
        }
        if descriptor.runtime_isolation == Some(launch_contract::IsolationDomain::UserProcess) {
            live_user_process_slots = live_user_process_slots.saturating_add(1);
        }
        if descriptor.user_published_endpoint {
            published_user_process_slots = published_user_process_slots.saturating_add(1);
        }
    }

    ServiceParityStatus {
        required_services,
        packaged_service_slots,
        live_user_process_slots,
        published_user_process_slots,
        strict_mode_enabled: FULL_PARITY_STRICT_MODE.load(Ordering::Acquire),
        full_parity_ready: published_user_process_slots == required_services,
    }
}

fn service_descriptor_entry(
    id: ServiceId,
    name: &'static str,
    class: ServiceClass,
    bulk_data_out_of_band: bool,
    openable_rights: CapabilityRights,
    bulk_region_classes: Vec<String>,
    service_process_available: bool,
    runtime_isolation: Option<launch_contract::IsolationDomain>,
    runtime_task_id: Option<u64>,
    runtime_image_path: Option<String>,
    user_published_endpoint: bool,
) -> ServiceDescriptor {
    ServiceDescriptor {
        id,
        name: String::from(name),
        class,
        control_plane: true,
        bulk_data_out_of_band,
        openable_rights,
        bulk_region_classes,
        service_process_available,
        runtime_isolation,
        runtime_task_id,
        runtime_image_path,
        user_published_endpoint,
    }
}
