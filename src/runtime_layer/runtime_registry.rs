use super::super::gui::launch_pipeline::{
    resolve_external_image, resolve_file_association, resolve_launch_query, AbiPersonality,
    AppDescriptor, AppInstallRoot, AppPresentation, AppResolution, AppTrust, CapabilityProfile,
    LoaderDispatch, PackageRecord, StateContract,
};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use echos_manifest::{AppRuntime, NativeCapability};

use super::runtime_model::{PackageRegistryEntry, RegistryEntrySource};

pub struct RuntimePackageRegistry<'a> {
    records: &'a [PackageRecord],
}

impl<'a> RuntimePackageRegistry<'a> {
    pub fn new(records: &'a [PackageRecord]) -> Self {
        Self { records }
    }

    pub fn resolve(&self, query: &str) -> Option<AppResolution> {
        resolve_installed_packaged(query).or_else(|| resolve_launch_query(query, self.records))
    }

    pub fn resolve_with_probe<F>(&self, query: &str, path_exists: F) -> Option<AppResolution>
    where
        F: FnMut(&str) -> bool,
    {
        resolve_installed_packaged(query).or_else(|| {
            super::super::gui::launch_pipeline::resolve_launch_query_with_probe(
                query,
                self.records,
                path_exists,
            )
        })
    }

    pub fn records(&self) -> &'a [PackageRecord] {
        self.records
    }

    pub fn resolve_file_association(&self, path: &str) -> Option<AppResolution> {
        resolve_file_association(path, self.records)
    }

    pub fn describe(&self, query: &str) -> Option<PackageRegistryEntry> {
        super::super::security::package::resolve_installed_app(query)
            .map(|installed| installed_registry_entry(&installed))
            .or_else(|| registry_entry_for_query(query, self.records))
    }

    pub fn entries(&self) -> Vec<PackageRegistryEntry> {
        let mut entries = Vec::new();
        for record in self.records {
            entries.push(PackageRegistryEntry {
                manifest: record.descriptor.manifest(),
                source: if record.external_candidates.is_empty() {
                    RegistryEntrySource::BuiltIn
                } else {
                    RegistryEntrySource::ExternalCandidate
                },
                aliases: record
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                entry_path: None,
                external_candidates: record
                    .external_candidates
                    .iter()
                    .map(|candidate| (*candidate).to_string())
                    .collect(),
            });
        }
        for installed in super::super::security::package::list_installed_apps() {
            entries.push(installed_registry_entry(&installed));
        }
        entries
    }
}

fn resolve_installed_packaged(query: &str) -> Option<AppResolution> {
    let installed = super::super::security::package::resolve_installed_app(query)?;
    packaged_resolution_for(&installed)
}

fn packaged_resolution_for(
    installed: &super::super::security::package::InstalledPackagedApp,
) -> Option<AppResolution> {
    let (loader, abi, presentation) = runtime_contract_for(
        installed.compiled_manifest.runtime,
        installed.compiled_manifest.presentation,
    )?;
    let descriptor = AppDescriptor::new(
        installed.runtime_app_id,
        installed.package_id,
        installed.title,
        loader,
        abi,
        presentation,
        capability_profile_for_packaged(installed),
    )
    .with_package_id(installed.package_id)
    .with_install_root(AppInstallRoot::UserApps)
    .with_trust(match installed.trust_level {
        super::super::security::package::PackageTrustLevel::Platform => AppTrust::Platform,
        super::super::security::package::PackageTrustLevel::Store
        | super::super::security::package::PackageTrustLevel::Developer => AppTrust::Installed,
    })
    .with_state_contract(match installed.compiled_manifest.state_contract {
        echos_manifest::AppStateContract::Stateless => StateContract::Stateless,
        echos_manifest::AppStateContract::WarmSuspend => StateContract::WarmSuspend,
        echos_manifest::AppStateContract::ColdResume => StateContract::ColdResume,
    });
    Some(AppResolution::ExternalPath {
        descriptor,
        path: installed.entry_path.to_string(),
    })
}

fn installed_registry_entry(
    installed: &super::super::security::package::InstalledPackagedApp,
) -> PackageRegistryEntry {
    let descriptor = packaged_resolution_for(installed)
        .map(|resolution| resolution.descriptor())
        .unwrap_or_else(|| {
            AppDescriptor::new(
                installed.runtime_app_id,
                installed.package_id,
                installed.title,
                LoaderDispatch::Native,
                AbiPersonality::Native,
                AppPresentation::Windowed,
                CapabilityProfile::service_defaults(),
            )
            .with_package_id(installed.package_id)
        });
    PackageRegistryEntry {
        manifest: descriptor.manifest(),
        source: RegistryEntrySource::InstalledPackage,
        aliases: vec![
            installed.package_id.to_string(),
            installed.manifest_app_id.to_string(),
            installed.title.to_string(),
        ],
        entry_path: Some(installed.entry_path.to_string()),
        external_candidates: Vec::new(),
    }
}

fn registry_entry_for_query(
    query: &str,
    registry: &[PackageRecord],
) -> Option<PackageRegistryEntry> {
    let normalized = query.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Some(resolution) = resolve_external_image(normalized) {
        return Some(PackageRegistryEntry {
            manifest: resolution.manifest(),
            source: RegistryEntrySource::ExternalImage,
            aliases: vec![normalized.to_string()],
            entry_path: resolution.path().map(|path| path.to_string()),
            external_candidates: Vec::new(),
        });
    }
    let lowered = normalized.to_ascii_lowercase();
    registry.iter().find_map(|record| {
        if record.matches_query(lowered.as_str()) {
            Some(PackageRegistryEntry {
                manifest: record.descriptor.manifest(),
                source: if record.external_candidates.is_empty() {
                    RegistryEntrySource::BuiltIn
                } else {
                    RegistryEntrySource::ExternalCandidate
                },
                aliases: record
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                entry_path: None,
                external_candidates: record
                    .external_candidates
                    .iter()
                    .map(|candidate| (*candidate).to_string())
                    .collect(),
            })
        } else {
            None
        }
    })
}

pub(crate) fn capability_profile_for_packaged(
    installed: &super::super::security::package::InstalledPackagedApp,
) -> CapabilityProfile {
    let mut profile = CapabilityProfile::service_defaults();
    for capability in &installed.capability_set {
        match capability {
            NativeCapability::FsRead | NativeCapability::FsWrite => profile.file_system = true,
            NativeCapability::DialogsOpen | NativeCapability::DialogsSave => {
                profile.file_dialogs = true
            }
            NativeCapability::NotificationsPost => profile.notifications = true,
            NativeCapability::ClipboardRead
            | NativeCapability::ClipboardWrite
            | NativeCapability::CaptureFrame => {}
        }
    }
    profile
}

pub(crate) fn runtime_contract_for(
    runtime: AppRuntime,
    presentation: echos_manifest::AppPresentation,
) -> Option<(LoaderDispatch, AbiPersonality, AppPresentation)> {
    match runtime {
        AppRuntime::Native => Some((
            LoaderDispatch::Native,
            AbiPersonality::Native,
            match presentation {
                echos_manifest::AppPresentation::Windowed => AppPresentation::Windowed,
                echos_manifest::AppPresentation::ShellOwned => AppPresentation::ShellOwned,
                echos_manifest::AppPresentation::SpecialAction => AppPresentation::SpecialAction,
                echos_manifest::AppPresentation::Headless => AppPresentation::Headless,
            },
        )),
        AppRuntime::Pe => Some((
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
        )),
        AppRuntime::Elf => Some((
            LoaderDispatch::Elf,
            AbiPersonality::Posix,
            AppPresentation::ShellOwned,
        )),
        AppRuntime::Special => None,
    }
}
