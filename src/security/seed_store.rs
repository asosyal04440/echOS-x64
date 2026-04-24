use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use lazy_static::lazy_static;
use spin::Mutex;

const DEFAULT_SEED_STORE_ROOTS: &[&str] = &["/seed/apps", "/system/seed/apps"];
const DEFAULT_LOOP_IMAGE_PATHS: &[&str] = &[
    "/seed/apps.img",
    "/seed/curated-seed.img",
    "/system/seed/apps.img",
    "/system/seed/curated-seed.img",
];
const LOOP_IMAGE_MAGIC: [u8; 8] = *b"echSID01";
const LOOP_IMAGE_VERSION: u16 = 1;
const LOOP_IMAGE_HEADER_LEN: usize = 16;
const SEED_RETRY_BASE_NS: u64 = 2_000_000_000;
const SEED_RETRY_MAX_NS: u64 = 30_000_000_000;
const SEED_QUARANTINE_THRESHOLD: u32 = 3;
const EXPLICIT_SEED_MOUNT_TARGETS: &[&str] =
    &["/seed", "/seed/apps", "/system/seed", "/system/seed/apps"];
const EXPLICIT_SEED_MOUNT_SOURCES: &[&str] = &[
    "seed",
    "/dev/appliance/seed",
    "/dev/disk/by-label/seed",
    "/dev/seed",
];

lazy_static! {
    static ref SEED_CATALOG: Mutex<BTreeMap<String, SeedCatalogEntry>> =
        Mutex::new(BTreeMap::new());
    static ref SEED_INSPECTIONS: Mutex<BTreeMap<String, CachedSeedInspection>> =
        Mutex::new(BTreeMap::new());
    static ref SEED_FAILURES: Mutex<BTreeMap<String, SeedFailureState>> =
        Mutex::new(BTreeMap::new());
    static ref SEED_HASH_QUEUE: Mutex<VecDeque<SeedHashJob>> = Mutex::new(VecDeque::new());
}

#[cfg(test)]
lazy_static! {
    static ref TEST_SEED_MOUNTS: Mutex<Option<Vec<crate::fs::mount::MountPoint>>> =
        Mutex::new(None);
    static ref TEST_SEED_FILES: Mutex<Option<BTreeMap<String, Vec<u8>>>> = Mutex::new(None);
    static ref TEST_SEED_DIRS: Mutex<Option<BTreeMap<String, Vec<crate::fs::vfs_unified::VfsDirEntry>>>> =
        Mutex::new(None);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedBundleOrigin {
    ApplianceEsp,
    SeedPartition,
    LoopImage,
}

impl SeedBundleOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApplianceEsp => "appliance-esp",
            Self::SeedPartition => "seed-partition",
            Self::LoopImage => "loop-image",
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::SeedPartition => 3,
            Self::LoopImage => 2,
            Self::ApplianceEsp => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SeedBundleLocator {
    ResidentBytes(Vec<u8>),
    ReferencedPath {
        path: String,
    },
    LoopImageEntry {
        image_path: String,
        offset: usize,
        length: usize,
    },
}

#[derive(Clone, Debug)]
pub struct SeedCatalogEntry {
    pub origin: SeedBundleOrigin,
    pub identity: String,
    pub package_id: String,
    pub manifest_app_id: String,
    pub title: String,
    pub seed_version: String,
    pub installed_version: Option<String>,
    pub source_title: String,
    pub entry_rel_path: String,
    pub state: SeedCatalogState,
    pub failure_count: u32,
    pub last_error: Option<String>,
    pub retry_after_ns: Option<u64>,
    pub bundle_size: usize,
    pub source_size: usize,
    pub locator: SeedBundleLocator,
}

impl SeedCatalogEntry {
    fn matches_query(&self, query: &str) -> bool {
        self.package_id.eq_ignore_ascii_case(query)
            || self.manifest_app_id.eq_ignore_ascii_case(query)
            || self.title.eq_ignore_ascii_case(query)
            || self.source_title.eq_ignore_ascii_case(query)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedCatalogState {
    HashPending,
    Available,
    Installed,
    UpdateAvailable,
    Retryable,
    Quarantined,
}

#[derive(Clone, Debug, Default)]
struct SeedFailureState {
    count: u32,
    last_error: Option<String>,
    next_retry_ns: Option<u64>,
}

#[derive(Clone, Debug)]
struct CachedSeedInspection {
    source_size: usize,
    bundle_size: usize,
    package_id: String,
    manifest_app_id: String,
    title: String,
    seed_version: String,
    source_title: String,
    entry_rel_path: String,
}

#[derive(Clone, Debug)]
struct SeedHashJob {
    origin: SeedBundleOrigin,
    identity: String,
    source_size: usize,
    bundle_size: usize,
    locator: SeedBundleLocator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedInstallOutcome {
    Installed,
    Updated,
    AlreadyInstalled,
    NotFound,
}

pub fn refresh_seed_catalog() -> Vec<SeedCatalogEntry> {
    let mut catalog = SEED_CATALOG.lock();
    let mut seen = Vec::new();
    ingest_appliance_seed_queue(&mut catalog, &mut seen);
    ingest_partition_seed_roots(&mut catalog, &mut seen);
    ingest_loop_images(&mut catalog, &mut seen);
    catalog.retain(|identity, entry| {
        matches!(entry.origin, SeedBundleOrigin::ApplianceEsp)
            || seen.iter().any(|seen_identity| seen_identity == identity)
    });
    catalog.values().cloned().collect()
}

pub fn catalog_entries() -> Vec<SeedCatalogEntry> {
    refresh_seed_catalog()
}

pub fn pump_seed_hash_queue(limit: usize) -> usize {
    let now = crate::gui::animation::get_time_ns();
    let mut processed = 0usize;
    let mut refreshed = false;
    while processed < limit {
        let Some(job) = pop_ready_seed_hash_job(now) else {
            break;
        };
        if let Some(cached) = cached_seed_inspection(job.identity.as_str(), job.source_size) {
            let entry = catalog_entry_from_cached(
                job.origin,
                job.identity.clone(),
                job.source_size,
                job.bundle_size,
                job.locator.clone(),
                &cached,
            );
            SEED_CATALOG.lock().insert(job.identity.clone(), entry);
            processed += 1;
            refreshed = true;
            continue;
        }

        let bytes = match read_seed_locator_bytes(&job.locator) {
            Ok(bytes) => bytes,
            Err(err) => {
                record_seed_failure(job.identity.as_str(), err.to_string());
                processed += 1;
                refreshed = true;
                continue;
            }
        };
        let inspection = match crate::security::package::inspect_signed_bundle(bytes.as_slice()) {
            Ok(inspection) => inspection,
            Err(err) => {
                record_seed_failure(job.identity.as_str(), err.to_string());
                crate::serial_println!(
                    "[SEED] async inspect fail origin={} identity={} err={}",
                    job.origin.as_str(),
                    job.identity,
                    err
                );
                processed += 1;
                refreshed = true;
                continue;
            }
        };
        cache_seed_inspection(
            job.identity.as_str(),
            job.source_size,
            bytes.len(),
            &inspection,
        );
        clear_seed_failure(job.identity.as_str());
        let cached = cached_seed_inspection(job.identity.as_str(), job.source_size)
            .expect("cached inspection present after successful inspect");
        let entry = catalog_entry_from_cached(
            job.origin,
            job.identity.clone(),
            job.source_size,
            bytes.len(),
            job.locator.clone(),
            &cached,
        );
        SEED_CATALOG.lock().insert(job.identity.clone(), entry);
        processed += 1;
        refreshed = true;
    }
    if refreshed {
        let _ = refresh_seed_catalog();
    }
    processed
}

pub fn install_seed_for_query(
    query: &str,
) -> Result<SeedInstallOutcome, crate::security::package::PackageError> {
    refresh_seed_catalog();
    let entry = {
        let catalog = SEED_CATALOG.lock();
        find_best_seed_entry(catalog.values(), query)
    };
    let Some(entry) = entry else {
        return Ok(SeedInstallOutcome::NotFound);
    };
    if matches!(entry.state, SeedCatalogState::Installed) {
        return Ok(SeedInstallOutcome::AlreadyInstalled);
    }
    if entry.state == SeedCatalogState::Quarantined {
        return Err(crate::security::package::PackageError::RepositoryUnavailable);
    }
    let updating = matches!(entry.state, SeedCatalogState::UpdateAvailable);
    if updating {
        crate::security::package::remove_installed_package(entry.package_id.as_str())?;
    }

    let install_result = match &entry.locator {
        SeedBundleLocator::ResidentBytes(bytes) => {
            crate::security::package::install_bundle(bytes.as_slice())
        }
        SeedBundleLocator::ReferencedPath { path } => {
            crate::security::package::install_bundle_from_path_reference(path.as_str())
        }
        SeedBundleLocator::LoopImageEntry {
            image_path,
            offset,
            length,
        } => crate::security::package::install_bundle_from_loop_image_reference(
            image_path.as_str(),
            *offset,
            *length,
        ),
    };
    match install_result {
        Ok(_) => {
            clear_seed_failure(entry.identity.as_str());
            refresh_seed_catalog();
            Ok(if updating {
                SeedInstallOutcome::Updated
            } else {
                SeedInstallOutcome::Installed
            })
        }
        Err(err) => {
            record_seed_failure(entry.identity.as_str(), err.to_string());
            refresh_seed_catalog();
            Err(err)
        }
    }
}

pub fn install_seed_for_identity(
    identity: &str,
) -> Result<SeedInstallOutcome, crate::security::package::PackageError> {
    refresh_seed_catalog();
    let entry = {
        let catalog = SEED_CATALOG.lock();
        catalog.get(identity).cloned()
    };
    let Some(entry) = entry else {
        return Ok(SeedInstallOutcome::NotFound);
    };
    if matches!(entry.state, SeedCatalogState::Installed) {
        return Ok(SeedInstallOutcome::AlreadyInstalled);
    }
    if entry.state == SeedCatalogState::Quarantined {
        return Err(crate::security::package::PackageError::RepositoryUnavailable);
    }
    let updating = matches!(entry.state, SeedCatalogState::UpdateAvailable);
    if updating {
        crate::security::package::remove_installed_package(entry.package_id.as_str())?;
    }
    let install_result = match &entry.locator {
        SeedBundleLocator::ResidentBytes(bytes) => {
            crate::security::package::install_bundle(bytes.as_slice())
        }
        SeedBundleLocator::ReferencedPath { path } => {
            crate::security::package::install_bundle_from_path_reference(path.as_str())
        }
        SeedBundleLocator::LoopImageEntry {
            image_path,
            offset,
            length,
        } => crate::security::package::install_bundle_from_loop_image_reference(
            image_path.as_str(),
            *offset,
            *length,
        ),
    };
    match install_result {
        Ok(_) => {
            clear_seed_failure(identity);
            refresh_seed_catalog();
            Ok(if updating {
                SeedInstallOutcome::Updated
            } else {
                SeedInstallOutcome::Installed
            })
        }
        Err(err) => {
            record_seed_failure(identity, err.to_string());
            refresh_seed_catalog();
            Err(err)
        }
    }
}

pub fn clear_seed_quarantine(identity: &str) -> bool {
    let existed = SEED_FAILURES.lock().remove(identity).is_some();
    let _ = refresh_seed_catalog();
    existed
}

pub fn retry_seed_identity(identity: &str) -> bool {
    let entry = {
        let catalog = SEED_CATALOG.lock();
        catalog.get(identity).cloned()
    };
    let Some(entry) = entry else {
        return false;
    };
    SEED_FAILURES.lock().remove(identity);
    enqueue_seed_hash_job(SeedHashJob {
        origin: entry.origin,
        identity: entry.identity.clone(),
        source_size: entry.source_size,
        bundle_size: entry.bundle_size,
        locator: entry.locator.clone(),
    });
    let _ = pump_seed_hash_queue(1);
    let _ = refresh_seed_catalog();
    true
}

fn seed_vfs_read_file(path: &str) -> Result<Vec<u8>, &'static str> {
    #[cfg(test)]
    if let Some(files) = TEST_SEED_FILES.lock().as_ref() {
        return files.get(path).cloned().ok_or("seed test file missing");
    }
    crate::fs::vfs_unified::read_file(path)
}

fn seed_vfs_list_dir(path: &str) -> Result<Vec<crate::fs::vfs_unified::VfsDirEntry>, &'static str> {
    #[cfg(test)]
    if let Some(dirs) = TEST_SEED_DIRS.lock().as_ref() {
        return dirs.get(path).cloned().ok_or("seed test dir missing");
    }
    crate::fs::vfs_unified::list_dir(path)
}

fn seed_mounts() -> Vec<crate::fs::mount::MountPoint> {
    #[cfg(test)]
    if let Some(mounts) = TEST_SEED_MOUNTS.lock().as_ref() {
        return mounts.clone();
    }
    crate::fs::mount::MOUNT_TABLE.list()
}

pub fn read_loop_image_bundle(
    image_path: &str,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, &'static str> {
    let image = seed_vfs_read_file(image_path)?;
    let records = parse_seed_loop_image(image.as_slice())?;
    let Some(record) = records
        .iter()
        .find(|record| record.offset == offset && record.length == length)
    else {
        return Err("seed loop entry missing");
    };
    image
        .get(record.offset..record.offset + record.length)
        .map(|slice| slice.to_vec())
        .ok_or("seed loop entry range invalid")
}

fn ingest_appliance_seed_queue(
    catalog: &mut BTreeMap<String, SeedCatalogEntry>,
    seen: &mut Vec<String>,
) {
    for (index, bytes) in crate::boot::appliance::take_curated_app_bundles()
        .into_iter()
        .enumerate()
    {
        let identity = format!("esp-seed-{}", index + 1);
        let bundle_size = bytes.len();
        let inspection_bytes = bytes.clone();
        let Some(entry) = catalog_entry_from_bundle(
            SeedBundleOrigin::ApplianceEsp,
            identity,
            bundle_size,
            SeedBundleLocator::ResidentBytes(bytes),
            inspection_bytes.as_slice(),
        ) else {
            continue;
        };
        seen.push(entry.identity.clone());
        upsert_catalog_entry(catalog, entry);
    }
}

fn ingest_partition_seed_roots(
    catalog: &mut BTreeMap<String, SeedCatalogEntry>,
    seen: &mut Vec<String>,
) {
    for root in discover_seed_partition_roots() {
        let Ok(entries) = seed_vfs_list_dir(root.as_str()) else {
            continue;
        };
        for entry in entries {
            if entry.is_directory || !is_seed_bundle_name(entry.name.as_str()) {
                continue;
            }
            let full_path = join_seed_root(root.as_str(), entry.name.as_str());
            let entry_size = entry.size as usize;
            let identity = full_path.clone();
            seen.push(identity.clone());
            let locator = SeedBundleLocator::ReferencedPath {
                path: full_path.clone(),
            };
            if let Some(cached) = cached_seed_inspection(identity.as_str(), entry_size) {
                let seed_entry = catalog_entry_from_cached(
                    SeedBundleOrigin::SeedPartition,
                    identity,
                    entry_size,
                    cached.bundle_size,
                    locator,
                    &cached,
                );
                upsert_catalog_entry(catalog, seed_entry);
                continue;
            }

            enqueue_seed_hash_job(SeedHashJob {
                origin: SeedBundleOrigin::SeedPartition,
                identity: identity.clone(),
                source_size: entry_size,
                bundle_size: entry_size,
                locator: locator.clone(),
            });
            let seed_entry = pending_catalog_entry(
                SeedBundleOrigin::SeedPartition,
                identity,
                entry_size,
                entry_size,
                locator,
            );
            upsert_catalog_entry(catalog, seed_entry);
        }
    }
}

fn ingest_loop_images(catalog: &mut BTreeMap<String, SeedCatalogEntry>, seen: &mut Vec<String>) {
    for image_path in discover_loop_image_paths() {
        let image_bytes = match seed_vfs_read_file(image_path.as_str()) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let records = match parse_seed_loop_image(image_bytes.as_slice()) {
            Ok(records) => records,
            Err(err) => {
                record_seed_failure(image_path.as_str(), err.to_string());
                crate::serial_println!(
                    "[SEED] loop image parse fail path={} err={}",
                    image_path,
                    err
                );
                continue;
            }
        };
        for record in records {
            let identity = format!("{}::{}", image_path, record.identity);
            seen.push(identity.clone());
            let locator = SeedBundleLocator::LoopImageEntry {
                image_path: image_path.clone(),
                offset: record.offset,
                length: record.length,
            };
            if let Some(cached) = cached_seed_inspection(identity.as_str(), image_bytes.len()) {
                let seed_entry = catalog_entry_from_cached(
                    SeedBundleOrigin::LoopImage,
                    identity,
                    image_bytes.len(),
                    cached.bundle_size,
                    locator,
                    &cached,
                );
                upsert_catalog_entry(catalog, seed_entry);
                continue;
            }

            enqueue_seed_hash_job(SeedHashJob {
                origin: SeedBundleOrigin::LoopImage,
                identity: identity.clone(),
                source_size: image_bytes.len(),
                bundle_size: record.length,
                locator: locator.clone(),
            });
            let seed_entry = pending_catalog_entry(
                SeedBundleOrigin::LoopImage,
                identity,
                image_bytes.len(),
                record.length,
                locator,
            );
            upsert_catalog_entry(catalog, seed_entry);
        }
    }
}

fn upsert_catalog_entry(catalog: &mut BTreeMap<String, SeedCatalogEntry>, entry: SeedCatalogEntry) {
    match catalog.get(entry.identity.as_str()) {
        Some(existing)
            if existing.source_size == entry.source_size
                && existing.bundle_size == entry.bundle_size
                && existing.origin == entry.origin => {}
        _ => {
            catalog.insert(entry.identity.clone(), entry);
        }
    }
}

fn find_best_seed_entry<'a, I>(entries: I, query: &str) -> Option<SeedCatalogEntry>
where
    I: Iterator<Item = &'a SeedCatalogEntry>,
{
    entries
        .filter(|entry| entry.matches_query(query))
        .max_by_key(|entry| entry.origin.priority())
        .cloned()
}

fn catalog_entry_from_bundle(
    origin: SeedBundleOrigin,
    identity: String,
    source_size: usize,
    locator: SeedBundleLocator,
    bytes: &[u8],
) -> Option<SeedCatalogEntry> {
    let inspection = match crate::security::package::inspect_signed_bundle(bytes) {
        Ok(inspection) => inspection,
        Err(err) => {
            record_seed_failure(identity.as_str(), err.to_string());
            crate::serial_println!(
                "[SEED] inspect fail origin={} identity={} err={}",
                origin.as_str(),
                identity,
                err
            );
            return None;
        }
    };
    cache_seed_inspection(identity.as_str(), source_size, bytes.len(), &inspection);
    let cached = cached_seed_inspection(identity.as_str(), source_size)
        .expect("seed inspection cache populated after inline inspect");
    Some(catalog_entry_from_cached(
        origin,
        identity,
        source_size,
        bytes.len(),
        locator,
        &cached,
    ))
}

fn catalog_entry_from_cached(
    origin: SeedBundleOrigin,
    identity: String,
    source_size: usize,
    bundle_size: usize,
    locator: SeedBundleLocator,
    inspection: &CachedSeedInspection,
) -> SeedCatalogEntry {
    let failure_count = seed_failure_count(identity.as_str());
    let last_error = seed_last_error(identity.as_str());
    let retry_after_ns = seed_retry_after_ns(identity.as_str());
    let installed = crate::security::package::resolve_installed_app(inspection.package_id.as_str());
    let installed_version = installed
        .as_ref()
        .map(|installed| installed.compiled_manifest.version.clone());
    let state = if failure_count >= SEED_QUARANTINE_THRESHOLD {
        SeedCatalogState::Quarantined
    } else if failure_count > 0 {
        SeedCatalogState::Retryable
    } else if let Some(installed) = installed.as_ref() {
        if version_ordering(
            inspection.seed_version.as_str(),
            installed.compiled_manifest.version.as_str(),
        ) == Ordering::Greater
        {
            SeedCatalogState::UpdateAvailable
        } else {
            SeedCatalogState::Installed
        }
    } else {
        SeedCatalogState::Available
    };
    SeedCatalogEntry {
        origin,
        identity,
        package_id: inspection.package_id.clone(),
        manifest_app_id: inspection.manifest_app_id.clone(),
        title: inspection.title.clone(),
        seed_version: inspection.seed_version.clone(),
        installed_version,
        source_title: inspection.source_title.clone(),
        entry_rel_path: inspection.entry_rel_path.clone(),
        state,
        failure_count,
        last_error,
        retry_after_ns,
        bundle_size,
        source_size,
        locator,
    }
}

fn pending_catalog_entry(
    origin: SeedBundleOrigin,
    identity: String,
    source_size: usize,
    bundle_size: usize,
    locator: SeedBundleLocator,
) -> SeedCatalogEntry {
    let failure_count = seed_failure_count(identity.as_str());
    let last_error = seed_last_error(identity.as_str());
    let retry_after_ns = seed_retry_after_ns(identity.as_str());
    let fallback = fallback_seed_identity(identity.as_str());
    let state = if failure_count >= SEED_QUARANTINE_THRESHOLD {
        SeedCatalogState::Quarantined
    } else if failure_count > 0 {
        SeedCatalogState::Retryable
    } else {
        SeedCatalogState::HashPending
    };
    SeedCatalogEntry {
        origin,
        identity,
        package_id: fallback.clone(),
        manifest_app_id: fallback.clone(),
        title: fallback.clone(),
        seed_version: String::from("pending"),
        installed_version: None,
        source_title: fallback,
        entry_rel_path: String::new(),
        state,
        failure_count,
        last_error,
        retry_after_ns,
        bundle_size,
        source_size,
        locator,
    }
}

fn discover_seed_partition_roots() -> Vec<String> {
    let mut roots = Vec::new();
    for mount in seed_mounts() {
        if !is_explicit_seed_mount(mount.target.as_str(), mount.source.as_str()) {
            continue;
        }
        let target_lower = mount.target.to_ascii_lowercase();
        if target_lower.ends_with("/apps") {
            push_unique_path(&mut roots, mount.target.clone());
        } else {
            push_unique_path(&mut roots, join_seed_root(mount.target.as_str(), "apps"));
        }
    }
    for root in DEFAULT_SEED_STORE_ROOTS {
        push_unique_path(&mut roots, (*root).to_string());
    }
    roots
}

fn discover_loop_image_paths() -> Vec<String> {
    let mut paths = Vec::new();
    for mount in seed_mounts() {
        if !is_explicit_seed_mount(mount.target.as_str(), mount.source.as_str()) {
            continue;
        }
        push_unique_path(
            &mut paths,
            join_seed_root(mount.target.as_str(), "apps.img"),
        );
        push_unique_path(
            &mut paths,
            join_seed_root(mount.target.as_str(), "curated-seed.img"),
        );
    }
    for path in DEFAULT_LOOP_IMAGE_PATHS {
        push_unique_path(&mut paths, (*path).to_string());
    }
    paths
}

fn is_explicit_seed_mount(target: &str, source: &str) -> bool {
    let target_lower = target.to_ascii_lowercase();
    let source_lower = source.to_ascii_lowercase();
    EXPLICIT_SEED_MOUNT_TARGETS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(target_lower.as_str()))
        || EXPLICIT_SEED_MOUNT_SOURCES
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(source_lower.as_str()))
        || source_lower.ends_with("/seed")
        || source_lower.ends_with(":seed")
        || source_lower.contains("/by-label/seed")
}

fn push_unique_path(paths: &mut Vec<String>, candidate: String) {
    if !paths.iter().any(|entry| entry == &candidate) {
        paths.push(candidate);
    }
}

fn seed_failure_count(identity: &str) -> u32 {
    SEED_FAILURES
        .lock()
        .get(identity)
        .map(|state| state.count)
        .unwrap_or(0)
}

fn seed_last_error(identity: &str) -> Option<String> {
    SEED_FAILURES
        .lock()
        .get(identity)
        .and_then(|state| state.last_error.clone())
}

fn seed_retry_after_ns(identity: &str) -> Option<u64> {
    SEED_FAILURES
        .lock()
        .get(identity)
        .and_then(|state| state.next_retry_ns)
}

fn record_seed_failure(identity: &str, err: String) {
    let now = crate::gui::animation::get_time_ns();
    let mut failures = SEED_FAILURES.lock();
    let state = failures.entry(identity.to_string()).or_default();
    state.count = state.count.saturating_add(1);
    state.last_error = Some(err);
    let shift = state.count.saturating_sub(1).min(6);
    let backoff = (SEED_RETRY_BASE_NS << shift).min(SEED_RETRY_MAX_NS);
    state.next_retry_ns = Some(now.saturating_add(backoff));
}

fn clear_seed_failure(identity: &str) {
    SEED_FAILURES.lock().remove(identity);
}

fn cached_seed_inspection(identity: &str, source_size: usize) -> Option<CachedSeedInspection> {
    SEED_INSPECTIONS
        .lock()
        .get(identity)
        .filter(|cached| cached.source_size == source_size)
        .cloned()
}

fn cache_seed_inspection(
    identity: &str,
    source_size: usize,
    bundle_size: usize,
    inspection: &crate::security::package::BundleInspection,
) {
    SEED_INSPECTIONS.lock().insert(
        identity.to_string(),
        CachedSeedInspection {
            source_size,
            bundle_size,
            package_id: inspection.compiled_manifest.app_id.clone(),
            manifest_app_id: inspection.source_manifest.app_id.clone(),
            title: inspection.compiled_manifest.name.clone(),
            seed_version: inspection.compiled_manifest.version.clone(),
            source_title: inspection.source_manifest.name.clone(),
            entry_rel_path: inspection.compiled_manifest.entry.clone(),
        },
    );
}

fn enqueue_seed_hash_job(job: SeedHashJob) {
    let mut queue = SEED_HASH_QUEUE.lock();
    if let Some(existing) = queue
        .iter_mut()
        .find(|queued| queued.identity == job.identity && queued.source_size == job.source_size)
    {
        *existing = job;
        return;
    }
    queue.push_back(job);
}

fn pop_ready_seed_hash_job(now: u64) -> Option<SeedHashJob> {
    let mut queue = SEED_HASH_QUEUE.lock();
    let queue_len = queue.len();
    for _ in 0..queue_len {
        let job = queue.pop_front()?;
        if cached_seed_inspection(job.identity.as_str(), job.source_size).is_some() {
            continue;
        }
        let retry_after = seed_retry_after_ns(job.identity.as_str()).unwrap_or(0);
        if retry_after > now {
            queue.push_back(job);
            continue;
        }
        return Some(job);
    }
    None
}

fn read_seed_locator_bytes(locator: &SeedBundleLocator) -> Result<Vec<u8>, &'static str> {
    match locator {
        SeedBundleLocator::ResidentBytes(bytes) => Ok(bytes.clone()),
        SeedBundleLocator::ReferencedPath { path } => seed_vfs_read_file(path),
        SeedBundleLocator::LoopImageEntry {
            image_path,
            offset,
            length,
        } => read_loop_image_bundle(image_path.as_str(), *offset, *length),
    }
}

fn fallback_seed_identity(identity: &str) -> String {
    let leaf = identity.rsplit('/').next().unwrap_or(identity);
    let leaf = leaf.rsplit("::").next().unwrap_or(leaf);
    leaf.trim_end_matches(".bhd")
        .trim_end_matches(".img")
        .to_string()
}

fn version_ordering(left: &str, right: &str) -> Ordering {
    let mut left_parts = version_tokens(left);
    let mut right_parts = version_tokens(right);
    let len = left_parts.len().max(right_parts.len());
    left_parts.resize(len, VersionToken::Number(0));
    right_parts.resize(len, VersionToken::Number(0));
    for (left, right) in left_parts.iter().zip(right_parts.iter()) {
        match (left, right) {
            (VersionToken::Number(a), VersionToken::Number(b)) => match a.cmp(b) {
                Ordering::Equal => {}
                other => return other,
            },
            (VersionToken::Text(a), VersionToken::Text(b)) => match a.cmp(b) {
                Ordering::Equal => {}
                other => return other,
            },
            (VersionToken::Number(_), VersionToken::Text(_)) => return Ordering::Greater,
            (VersionToken::Text(_), VersionToken::Number(_)) => return Ordering::Less,
        }
    }
    Ordering::Equal
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VersionToken {
    Number(u64),
    Text(String),
}

fn version_tokens(version: &str) -> Vec<VersionToken> {
    let mut out = Vec::new();
    for part in version
        .split(|ch: char| !(ch.is_ascii_alphanumeric()))
        .filter(|part| !part.is_empty())
    {
        if let Ok(number) = part.parse::<u64>() {
            out.push(VersionToken::Number(number));
        } else {
            out.push(VersionToken::Text(part.to_ascii_lowercase()));
        }
    }
    if out.is_empty() {
        out.push(VersionToken::Text(version.to_ascii_lowercase()));
    }
    out
}

fn is_seed_bundle_name(name: &str) -> bool {
    name.len() > 4 && name.to_ascii_lowercase().ends_with(".bhd")
}

fn join_seed_root(root: &str, name: &str) -> String {
    if root.ends_with('/') {
        format!("{}/{}", root.trim_end_matches('/'), name)
    } else {
        format!("{}/{}", root, name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LoopImageRecord {
    identity: String,
    offset: usize,
    length: usize,
}

fn parse_seed_loop_image(bytes: &[u8]) -> Result<Vec<LoopImageRecord>, &'static str> {
    if bytes.len() < LOOP_IMAGE_HEADER_LEN || &bytes[..8] != &LOOP_IMAGE_MAGIC {
        return Err("invalid seed image magic");
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != LOOP_IMAGE_VERSION {
        return Err("unsupported seed image version");
    }
    let entry_count = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
    let mut cursor = LOOP_IMAGE_HEADER_LEN;
    let mut out = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        if cursor + 20 > bytes.len() {
            return Err("seed image header truncated");
        }
        let offset = u64::from_le_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| "seed image offset invalid")?,
        ) as usize;
        cursor += 8;
        let length = u64::from_le_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| "seed image length invalid")?,
        ) as usize;
        cursor += 8;
        let identity_len = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2;
        cursor += 2;
        if cursor + identity_len > bytes.len() {
            return Err("seed image identity truncated");
        }
        let identity = core::str::from_utf8(&bytes[cursor..cursor + identity_len])
            .map_err(|_| "seed image identity invalid")?
            .to_string();
        cursor += identity_len;
        if offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .is_none()
        {
            return Err("seed image entry range invalid");
        }
        out.push(LoopImageRecord {
            identity,
            offset,
            length,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        clear_seed_quarantine, install_seed_for_query, is_explicit_seed_mount, is_seed_bundle_name,
        join_seed_root, parse_seed_loop_image, pending_catalog_entry, pump_seed_hash_queue,
        record_seed_failure, refresh_seed_catalog, version_ordering, LoopImageRecord,
        SeedBundleLocator, SeedBundleOrigin, SeedCatalogState, SeedInstallOutcome,
        LOOP_IMAGE_MAGIC, LOOP_IMAGE_VERSION, SEED_CATALOG, SEED_FAILURES, SEED_HASH_QUEUE,
        SEED_INSPECTIONS, TEST_SEED_DIRS, TEST_SEED_FILES, TEST_SEED_MOUNTS,
    };
    use alloc::collections::BTreeMap;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::cmp::Ordering;
    use echos_manifest::{
        AppPresentation, AppRuntime, AppStateContract, CompiledAppManifest, DefaultWindow,
        RestartPolicy, SourceAppManifest, TrustDomain,
    };
    use sha2::{Digest, Sha256};

    fn encode_seed_image(records: &[(&str, &[u8])]) -> Vec<u8> {
        let mut header = Vec::new();
        header.extend_from_slice(&LOOP_IMAGE_MAGIC);
        header.extend_from_slice(&LOOP_IMAGE_VERSION.to_le_bytes());
        header.extend_from_slice(&(records.len() as u16).to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        let mut table = Vec::new();
        let mut data = Vec::new();
        for (identity, payload) in records {
            data.extend_from_slice(payload);
        }
        let mut payload_offset = header.len()
            + records
                .iter()
                .map(|(identity, _)| 20 + identity.len())
                .sum::<usize>();
        for (identity, payload) in records {
            table.extend_from_slice(&(payload_offset as u64).to_le_bytes());
            table.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            table.extend_from_slice(&(identity.len() as u16).to_le_bytes());
            table.extend_from_slice(&0u16.to_le_bytes());
            table.extend_from_slice(identity.as_bytes());
            payload_offset += payload.len();
        }
        header.extend_from_slice(&table);
        header.extend_from_slice(&data);
        header
    }

    fn reset_seed_test_environment() {
        SEED_CATALOG.lock().clear();
        SEED_INSPECTIONS.lock().clear();
        SEED_FAILURES.lock().clear();
        SEED_HASH_QUEUE.lock().clear();
        *TEST_SEED_MOUNTS.lock() = Some(Vec::new());
        *TEST_SEED_FILES.lock() = Some(BTreeMap::new());
        *TEST_SEED_DIRS.lock() = Some(BTreeMap::new());
        crate::security::package::clear_test_installed_apps();
        crate::security::package::clear_test_store();
    }

    fn demo_source(app_id: &str, title: &str, version: &str, entry: &str) -> SourceAppManifest {
        SourceAppManifest {
            app_id: app_id.into(),
            name: title.into(),
            version: version.into(),
            entry: entry.into(),
            sdk_version: 1,
            runtime: AppRuntime::Native,
            presentation: AppPresentation::Windowed,
            capabilities: vec![echos_manifest::NativeCapability::NotificationsPost],
            default_window: DefaultWindow {
                title: title.into(),
                width: 640,
                height: 480,
            },
            state_contract: AppStateContract::ColdResume,
            restart_policy: RestartPolicy::bounded_retry(2),
        }
    }

    fn build_seed_bundle(app_id: &str, title: &str, version: &str, entry: &str) -> Vec<u8> {
        let source = demo_source(app_id, title, version, entry);
        let entry_bytes = alloc::format!("{title}:{version}:{entry}").into_bytes();
        let digest = Sha256::digest(entry_bytes.as_slice());
        let compiled =
            CompiledAppManifest::from_source(&source, digest.into()).expect("compiled manifest");
        crate::security::package::build_signed_bundle(
            &source,
            &compiled,
            entry_bytes.as_slice(),
            TrustDomain::Developer,
        )
        .expect("signed bundle")
    }

    fn install_test_seed_mount(target: &str, source: &str) {
        *TEST_SEED_MOUNTS.lock() = Some(vec![crate::fs::mount::MountPoint {
            source: source.into(),
            target: target.into(),
            fs_type: crate::fs::mount::FsType::Fat32,
            flags: crate::fs::mount::MountFlags::read_only(),
        }]);
    }

    fn install_test_seed_dir(
        path: &str,
        entries: &[(&str, usize, bool)],
        fs_type: crate::fs::vfs_unified::VfsFsType,
    ) {
        let rendered = entries
            .iter()
            .map(
                |(name, size, is_directory)| crate::fs::vfs_unified::VfsDirEntry {
                    name: (*name).into(),
                    size: *size as u64,
                    is_directory: *is_directory,
                    fs_type,
                },
            )
            .collect::<Vec<_>>();
        TEST_SEED_DIRS
            .lock()
            .as_mut()
            .expect("seed dirs override initialized")
            .insert(path.into(), rendered);
    }

    fn install_test_seed_file(path: &str, bytes: &[u8]) {
        TEST_SEED_FILES
            .lock()
            .as_mut()
            .expect("seed files override initialized")
            .insert(path.into(), bytes.to_vec());
    }

    fn catalog_entry(identity: &str) -> super::SeedCatalogEntry {
        refresh_seed_catalog()
            .into_iter()
            .find(|entry| entry.identity == identity || entry.package_id == identity)
            .expect("seed catalog entry")
    }

    #[test]
    fn seed_bundle_name_filter_accepts_bhd_case_insensitively() {
        assert!(is_seed_bundle_name("helix.bhd"));
        assert!(is_seed_bundle_name("HELIX.BHD"));
        assert!(!is_seed_bundle_name("helix.exe"));
        assert!(!is_seed_bundle_name(".bhd"));
    }

    #[test]
    fn join_seed_root_normalizes_separator_once() {
        assert_eq!(
            join_seed_root("/seed/apps", "helix.bhd"),
            "/seed/apps/helix.bhd"
        );
        assert_eq!(
            join_seed_root("/seed/apps/", "helix.bhd"),
            "/seed/apps/helix.bhd"
        );
    }

    #[test]
    fn explicit_seed_mount_detection_is_deny_by_default() {
        assert!(is_explicit_seed_mount("/seed", "/dev/appliance/seed"));
        assert!(is_explicit_seed_mount(
            "/system/seed",
            "/dev/disk/by-label/seed"
        ));
        assert!(is_explicit_seed_mount("/mnt/vendor", "disk0:seed"));
        assert!(!is_explicit_seed_mount("/mnt/seed-tools", "/dev/nvme0n1p2"));
        assert!(!is_explicit_seed_mount(
            "/packages",
            "/dev/disk/by-label/data"
        ));
    }

    #[test]
    fn loop_image_parser_reports_entries_with_offsets() {
        let image = encode_seed_image(&[("helix", &[1, 2, 3]), ("bat", &[4, 5])]);
        let parsed = parse_seed_loop_image(image.as_slice()).expect("parse");
        assert_eq!(
            parsed,
            vec![
                LoopImageRecord {
                    identity: String::from("helix"),
                    offset: 64,
                    length: 3,
                },
                LoopImageRecord {
                    identity: String::from("bat"),
                    offset: 67,
                    length: 2,
                }
            ]
        );
    }

    #[test]
    fn version_ordering_prefers_newer_seed_versions() {
        assert_eq!(version_ordering("1.2.0", "1.1.9"), Ordering::Greater);
        assert_eq!(version_ordering("1.2.0", "1.2.0"), Ordering::Equal);
        assert_eq!(version_ordering("1.2.0", "2.0.0"), Ordering::Less);
        assert_eq!(
            version_ordering("2.0.0-beta", "2.0.0-alpha"),
            Ordering::Greater
        );
    }

    #[test]
    fn pending_seed_entry_tracks_retry_and_quarantine_lifecycle() {
        let identity = "seed-lifecycle";
        let locator = SeedBundleLocator::ReferencedPath {
            path: String::from("/seed/apps/demo.bhd"),
        };

        let initial = pending_catalog_entry(
            SeedBundleOrigin::SeedPartition,
            String::from(identity),
            128,
            128,
            locator.clone(),
        );
        assert_eq!(initial.state, SeedCatalogState::HashPending);
        assert_eq!(initial.failure_count, 0);
        assert!(initial.retry_after_ns.is_none());

        record_seed_failure(identity, String::from("io fail"));
        let retryable = pending_catalog_entry(
            SeedBundleOrigin::SeedPartition,
            String::from(identity),
            128,
            128,
            locator.clone(),
        );
        assert_eq!(retryable.state, SeedCatalogState::Retryable);
        assert_eq!(retryable.failure_count, 1);
        assert_eq!(retryable.last_error.as_deref(), Some("io fail"));
        assert!(retryable.retry_after_ns.is_some());

        record_seed_failure(identity, String::from("io fail"));
        record_seed_failure(identity, String::from("io fail"));
        let quarantined = pending_catalog_entry(
            SeedBundleOrigin::LoopImage,
            String::from(identity),
            128,
            64,
            SeedBundleLocator::LoopImageEntry {
                image_path: String::from("/seed/curated-seed.img"),
                offset: 32,
                length: 64,
            },
        );
        assert_eq!(quarantined.state, SeedCatalogState::Quarantined);
        assert_eq!(quarantined.failure_count, 3);
        assert!(quarantined.retry_after_ns.is_some());

        assert!(clear_seed_quarantine(identity));
        let reset = pending_catalog_entry(
            SeedBundleOrigin::SeedPartition,
            String::from(identity),
            128,
            128,
            locator,
        );
        assert_eq!(reset.state, SeedCatalogState::HashPending);
        assert_eq!(reset.failure_count, 0);
        assert!(reset.last_error.is_none());
    }

    #[test]
    fn mounted_seed_roots_and_loop_images_cover_install_and_update_lifecycle() {
        reset_seed_test_environment();

        let root_path = "/seed/apps/demo.bhd";
        let loop_image_path = "/seed/curated-seed.img";
        let root_v1 = build_seed_bundle("org.echos.seed.demo", "Seed Demo", "1.0.0", "demo.elf");
        let root_v2 = build_seed_bundle("org.echos.seed.demo", "Seed Demo", "2.0.0", "demo-v2.elf");
        let loop_bundle =
            build_seed_bundle("org.echos.seed.loop", "Loop Tool", "1.0.0", "loop.elf");

        install_test_seed_mount("/seed", "/dev/appliance/seed");
        install_test_seed_dir(
            "/seed/apps",
            &[("demo.bhd", root_v1.len(), false)],
            crate::fs::vfs_unified::VfsFsType::Fat32,
        );
        install_test_seed_file(root_path, root_v1.as_slice());
        crate::security::package::write_test_store_file(root_path, root_v1.as_slice());

        let loop_image = encode_seed_image(&[("loop-tool", loop_bundle.as_slice())]);
        install_test_seed_file(loop_image_path, loop_image.as_slice());

        let pending = refresh_seed_catalog();
        assert!(pending.iter().any(|entry| entry.identity == root_path));
        assert!(pending
            .iter()
            .any(|entry| entry.identity == "/seed/curated-seed.img::loop-tool"));

        assert_eq!(pump_seed_hash_queue(8), 2);

        let root_available = catalog_entry("org.echos.seed.demo");
        assert_eq!(root_available.state, SeedCatalogState::Available);
        let loop_available = catalog_entry("org.echos.seed.loop");
        assert_eq!(loop_available.state, SeedCatalogState::Available);

        assert_eq!(
            install_seed_for_query("org.echos.seed.demo").expect("install from mounted seed"),
            SeedInstallOutcome::Installed
        );
        let installed_root = crate::security::package::resolve_installed_app("org.echos.seed.demo")
            .expect("installed mounted seed");
        assert_eq!(installed_root.compiled_manifest.version, "1.0.0");
        assert_eq!(
            installed_root.bundle_backing.kind,
            crate::security::package::InstalledBundleBackingKind::ReferencedSeedPath
        );

        install_test_seed_dir(
            "/seed/apps",
            &[("demo.bhd", root_v2.len(), false)],
            crate::fs::vfs_unified::VfsFsType::Fat32,
        );
        install_test_seed_file(root_path, root_v2.as_slice());
        crate::security::package::write_test_store_file(root_path, root_v2.as_slice());

        let _ = refresh_seed_catalog();
        assert_eq!(pump_seed_hash_queue(8), 1);
        let root_update = catalog_entry("org.echos.seed.demo");
        assert_eq!(root_update.state, SeedCatalogState::UpdateAvailable);
        assert_eq!(root_update.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(root_update.seed_version, "2.0.0");

        assert_eq!(
            install_seed_for_query("org.echos.seed.demo").expect("update from mounted seed"),
            SeedInstallOutcome::Updated
        );
        let updated_root = crate::security::package::resolve_installed_app("org.echos.seed.demo")
            .expect("updated mounted seed");
        assert_eq!(updated_root.compiled_manifest.version, "2.0.0");
        assert_eq!(
            updated_root.bundle_backing.kind,
            crate::security::package::InstalledBundleBackingKind::ReferencedSeedPath
        );

        assert_eq!(
            install_seed_for_query("org.echos.seed.loop").expect("install from loop image"),
            SeedInstallOutcome::Installed
        );
        let installed_loop = crate::security::package::resolve_installed_app("org.echos.seed.loop")
            .expect("installed loop image seed");
        assert_eq!(installed_loop.compiled_manifest.version, "1.0.0");
        assert_eq!(
            installed_loop.bundle_backing.kind,
            crate::security::package::InstalledBundleBackingKind::ReferencedLoopImage
        );
        assert_eq!(installed_loop.bundle_backing.source_path, loop_image_path);
    }
}
