//! File-backed loopback block devices for image-backed workflows.

use crate::drivers::block::{BlockDevice, BlockDeviceError, BlockDeviceType};
use crate::drivers::driver_model::{DeviceType, DRIVER_MODEL};
use crate::ipc::request_store_sync;
use crate::services::{StoreCommand, StoreResponse};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp::max;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

const DEFAULT_BLOCK_SIZE: u32 = 512;
const RESIDENT_IMAGE_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const MOUNT_SNAPSHOT_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const PAGE_CACHE_TARGET_BYTES: usize = 8 * 1024 * 1024;

lazy_static! {
    static ref LOOPBACK_REGISTRY: Mutex<BTreeMap<String, Arc<Mutex<LoopbackState>>>> =
        Mutex::new(BTreeMap::new());
}

static NEXT_LOOP_INDEX: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct LoopbackState {
    name: String,
    backing_path: Option<String>,
    storage: LoopbackStorage,
    block_size: u32,
    read_only: bool,
    mount_points: Vec<String>,
    driver_device_id: u64,
}

#[derive(Debug)]
enum LoopbackStorage {
    Resident {
        image: Vec<u8>,
        dirty: bool,
    },
    Paged {
        image_len: usize,
        cache_limit_blocks: usize,
        cache: BTreeMap<u64, Vec<u8>>,
        dirty_blocks: BTreeMap<u64, Vec<u8>>,
        lru: VecDeque<u64>,
    },
}

#[derive(Clone)]
pub struct LoopbackBlockDevice {
    state: Arc<Mutex<LoopbackState>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopbackDescriptor {
    pub name: String,
    pub backing_path: Option<String>,
    pub storage_mode: &'static str,
    pub block_size: u32,
    pub block_count: u64,
    pub read_only: bool,
    pub dirty: bool,
    pub mount_points: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopbackMount {
    pub device_name: String,
    pub mount_point: String,
    pub fs_type: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopbackFilesystem {
    Fat32,
    ExFat,
    Ext4,
    Ntfs,
}

impl LoopbackStorage {
    fn image_len(&self) -> usize {
        match self {
            Self::Resident { image, .. } => image.len(),
            Self::Paged { image_len, .. } => *image_len,
        }
    }

    fn storage_mode(&self) -> &'static str {
        match self {
            Self::Resident { .. } => "resident",
            Self::Paged { .. } => "paged",
        }
    }

    fn is_dirty(&self) -> bool {
        match self {
            Self::Resident { dirty, .. } => *dirty,
            Self::Paged { dirty_blocks, .. } => !dirty_blocks.is_empty(),
        }
    }
}

impl LoopbackState {
    fn descriptor(&self) -> LoopbackDescriptor {
        LoopbackDescriptor {
            name: self.name.clone(),
            backing_path: self.backing_path.clone(),
            storage_mode: self.storage.storage_mode(),
            block_size: self.block_size,
            block_count: (self.storage.image_len() / self.block_size as usize) as u64,
            read_only: self.read_only,
            dirty: self.storage.is_dirty(),
            mount_points: self.mount_points.clone(),
        }
    }

    fn image_len(&self) -> usize {
        self.storage.image_len()
    }
}

impl LoopbackBlockDevice {
    pub fn descriptor(&self) -> LoopbackDescriptor {
        self.state.lock().descriptor()
    }

    pub fn snapshot_for_mount(&self) -> Result<Vec<u8>, &'static str> {
        let (path, len, mode) = {
            let state = self.state.lock();
            (
                state.backing_path.clone(),
                state.storage.image_len(),
                state.storage.storage_mode(),
            )
        };
        if len > MOUNT_SNAPSHOT_LIMIT_BYTES {
            return Err("loopback mount: image exceeds safe snapshot limit");
        }
        match self.state.lock().storage {
            LoopbackStorage::Resident { ref image, .. } => Ok(image.clone()),
            LoopbackStorage::Paged { .. } => {
                let path = path.ok_or("loopback paged image lost backing path")?;
                read_full_backing(path.as_str()).map_err(|_| {
                    if mode == "paged" {
                        "loopback mount: paged snapshot read failed"
                    } else {
                        "loopback mount: image snapshot failed"
                    }
                })
            }
        }
    }
}

impl BlockDevice for LoopbackBlockDevice {
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError> {
        let mut state = self.state.lock();
        let block_size = state.block_size as usize;
        let backing_path = state.backing_path.clone();
        if buffer.len() != block_size {
            return Err(BlockDeviceError::IoError);
        }
        match &mut state.storage {
            LoopbackStorage::Resident { image, .. } => {
                read_resident_block(image, block_size, lba, buffer)
            }
            LoopbackStorage::Paged {
                image_len,
                cache_limit_blocks,
                cache,
                dirty_blocks,
                lru,
            } => {
                if let Some(block) = dirty_blocks.get(&lba) {
                    buffer.copy_from_slice(block.as_slice());
                    touch_lru(lru, lba);
                    return Ok(());
                }
                if let Some(block) = cache.get(&lba) {
                    buffer.copy_from_slice(block.as_slice());
                    touch_lru(lru, lba);
                    return Ok(());
                }
                let path = backing_path.ok_or(BlockDeviceError::DeviceNotFound)?;
                let block = read_backing_block(path.as_str(), *image_len, block_size, lba)?;
                buffer.copy_from_slice(block.as_slice());
                cache.insert(lba, block);
                touch_lru(lru, lba);
                evict_clean_cache(cache, dirty_blocks, lru, *cache_limit_blocks);
                Ok(())
            }
        }
    }

    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
        let mut state = self.state.lock();
        if state.read_only {
            return Err(BlockDeviceError::WriteProtected);
        }
        let block_size = state.block_size as usize;
        if buffer.len() != block_size {
            return Err(BlockDeviceError::IoError);
        }
        match &mut state.storage {
            LoopbackStorage::Resident { image, dirty } => {
                write_resident_block(image, block_size, lba, buffer)?;
                *dirty = true;
                Ok(())
            }
            LoopbackStorage::Paged {
                image_len,
                cache_limit_blocks,
                cache,
                dirty_blocks,
                lru,
            } => {
                validate_block_bounds(*image_len, block_size, lba)?;
                dirty_blocks.insert(lba, buffer.to_vec());
                cache.insert(lba, buffer.to_vec());
                touch_lru(lru, lba);
                evict_clean_cache(cache, dirty_blocks, lru, *cache_limit_blocks);
                Ok(())
            }
        }
    }

    fn block_size(&self) -> u32 {
        self.state.lock().block_size
    }

    fn block_count(&self) -> u64 {
        let state = self.state.lock();
        (state.storage.image_len() / state.block_size as usize) as u64
    }

    fn device_name(&self) -> String {
        self.state.lock().name.clone()
    }

    fn device_type(&self) -> BlockDeviceType {
        BlockDeviceType::Virtual
    }

    fn is_read_only(&self) -> bool {
        self.state.lock().read_only
    }

    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        let mut state = self.state.lock();
        let backing_path = state.backing_path.clone();
        let block_size = state.block_size as usize;
        if state.read_only {
            return Err(BlockDeviceError::WriteProtected);
        }
        match &mut state.storage {
            LoopbackStorage::Resident { image, dirty } => {
                if !*dirty {
                    return Ok(());
                }
                if let Some(path) = backing_path {
                    write_full_backing(path.as_str(), image.as_slice())?;
                }
                *dirty = false;
                Ok(())
            }
            LoopbackStorage::Paged {
                image_len,
                cache,
                dirty_blocks,
                lru,
                ..
            } => {
                if dirty_blocks.is_empty() {
                    return Ok(());
                }
                let path = backing_path.ok_or(BlockDeviceError::DeviceNotFound)?;
                let dirty = core::mem::take(dirty_blocks);
                for (lba, block) in dirty.iter() {
                    write_backing_block(
                        path.as_str(),
                        *image_len,
                        block_size,
                        *lba,
                        block.as_slice(),
                    )?;
                    cache.insert(*lba, block.clone());
                    touch_lru(lru, *lba);
                }
                Ok(())
            }
        }
    }
}

pub fn attach_file(
    path: &str,
    block_size: Option<u32>,
    force_read_only: Option<bool>,
) -> Result<LoopbackDescriptor, &'static str> {
    let normalized = normalize_path(path);
    if let Some(existing) = find_existing_by_backing(normalized.as_str()) {
        return Ok(existing);
    }
    let block_size = block_size.unwrap_or(DEFAULT_BLOCK_SIZE);
    let read_only = force_read_only.unwrap_or_else(|| {
        crate::fs::mount::MOUNT_TABLE
            .find_mount(normalized.as_str())
            .map(|mount| mount.flags.read_only)
            .unwrap_or(false)
    });
    let info = request_store_sync(
        0,
        StoreCommand::GetFileInfo {
            path: normalized.clone(),
        },
    );
    let image_len = match info {
        Some(StoreResponse::FileInfo(info)) if !info.is_directory => info.size as usize,
        _ => {
            let image = crate::fs::vfs_unified::read_file(normalized.as_str())?;
            return register_loopback_storage(
                Some(normalized),
                block_size,
                read_only,
                LoopbackStorage::Resident {
                    image,
                    dirty: false,
                },
            );
        }
    };
    if block_size == 0 || image_len == 0 || image_len % block_size as usize != 0 {
        return Err("loopback image size must align to block size");
    }

    if image_len <= RESIDENT_IMAGE_LIMIT_BYTES || !can_use_paged_backing(normalized.as_str()) {
        let image = crate::fs::vfs_unified::read_file(normalized.as_str())?;
        return register_loopback_storage(
            Some(normalized),
            block_size,
            read_only,
            LoopbackStorage::Resident {
                image,
                dirty: false,
            },
        );
    }

    register_loopback_storage(
        Some(normalized),
        block_size,
        read_only,
        LoopbackStorage::Paged {
            image_len,
            cache_limit_blocks: max(16, PAGE_CACHE_TARGET_BYTES / block_size as usize),
            cache: BTreeMap::new(),
            dirty_blocks: BTreeMap::new(),
            lru: VecDeque::new(),
        },
    )
}

pub fn open(name: &str) -> Option<LoopbackBlockDevice> {
    LOOPBACK_REGISTRY
        .lock()
        .get(name)
        .cloned()
        .map(|state| LoopbackBlockDevice { state })
}

pub fn list() -> Vec<LoopbackDescriptor> {
    LOOPBACK_REGISTRY
        .lock()
        .values()
        .map(|state| state.lock().descriptor())
        .collect()
}

pub fn flush_device(name: &str) -> Result<(), &'static str> {
    let Some(mut device) = open(name) else {
        return Err("loopback device not found");
    };
    device.flush().map_err(block_error_str)
}

pub fn detach(name: &str) -> Result<(), &'static str> {
    let state = {
        let mut registry = LOOPBACK_REGISTRY.lock();
        let Some(state) = registry.get(name).cloned() else {
            return Err("loopback device not found");
        };
        let descriptor = state.lock().descriptor();
        if !descriptor.mount_points.is_empty() {
            return Err("loopback device still mounted");
        }
        registry.remove(name);
        state
    };
    let mut device = LoopbackBlockDevice {
        state: state.clone(),
    };
    if !device.is_read_only() {
        let _ = device.flush();
    }
    let device_id = state.lock().driver_device_id;
    DRIVER_MODEL.unregister_device(device_id);
    Ok(())
}

pub fn mount(
    spec: &str,
    mount_point: &str,
    fs_hint: Option<&str>,
) -> Result<LoopbackMount, &'static str> {
    if mount_point.is_empty() || !mount_point.starts_with('/') {
        return Err("mount point must be an absolute path");
    }
    if crate::fs::mount::MOUNT_TABLE
        .list()
        .iter()
        .any(|entry| entry.target == mount_point)
    {
        return Err("mount point already in use");
    }

    let mut device = match open(spec) {
        Some(device) => device,
        None => {
            let descriptor = attach_file(spec, None, None)?;
            open(descriptor.name.as_str()).ok_or("loopback device attach failed")?
        }
    };
    let descriptor = device.descriptor();
    let fs_kind = match fs_hint {
        Some("fat32") | Some("vfat") | Some("fat") => LoopbackFilesystem::Fat32,
        Some("exfat") => LoopbackFilesystem::ExFat,
        Some("ext4") => LoopbackFilesystem::Ext4,
        Some("ntfs") => LoopbackFilesystem::Ntfs,
        Some(_) => return Err("loopback mount: unsupported filesystem hint"),
        None => detect_loopback_filesystem(&mut device)?
            .ok_or("loopback mount: filesystem signature unsupported")?,
    };

    let (fs_label, mount_fs_type, source) = match fs_kind {
        LoopbackFilesystem::Fat32 => {
            let index = crate::fs::fat::mount_fat32_loopback(descriptor.name.as_str())
                .ok_or("loopback mount: FAT32 attach failed")?;
            (
                "fat32",
                crate::fs::vfs_unified::VfsFsType::Fat32,
                format!("fat32:{}", index),
            )
        }
        LoopbackFilesystem::ExFat => {
            let index = crate::fs::fat::mount_exfat_loopback(descriptor.name.as_str())
                .ok_or("loopback mount: exFAT attach failed")?;
            (
                "exfat",
                crate::fs::vfs_unified::VfsFsType::ExFat,
                format!("exfat:{}", index),
            )
        }
        LoopbackFilesystem::Ext4 => {
            crate::fs::ext4::mount_ext4_loopback(
                descriptor.name.as_str(),
                descriptor.name.as_str(),
            )
            .map_err(|_| "loopback mount: ext4 attach failed")?;
            (
                "ext4",
                crate::fs::vfs_unified::VfsFsType::Ext4,
                descriptor.name.clone(),
            )
        }
        LoopbackFilesystem::Ntfs => {
            crate::fs::ntfs::mount_ntfs_loopback(
                descriptor.name.as_str(),
                descriptor.name.as_str(),
            )
            .map_err(|_| "loopback mount: ntfs attach failed")?;
            (
                "ntfs",
                crate::fs::vfs_unified::VfsFsType::Ntfs,
                descriptor.name.clone(),
            )
        }
    };
    let mount_flags = if descriptor.read_only {
        crate::fs::mount::MountFlags::read_only()
    } else {
        crate::fs::mount::MountFlags::default_rw()
    };
    crate::fs::mount::MOUNT_TABLE.mount(
        descriptor.name.as_str(),
        mount_point,
        mount_fs_type.as_str(),
        mount_flags,
    )?;
    crate::fs::vfs_unified::VFS_UNIFIED.lock().mount(
        mount_point,
        mount_fs_type,
        source.as_str(),
        crate::fs::vfs_unified::VfsMountFlags::default(),
    );
    {
        let mut state = device.state.lock();
        if !state.mount_points.iter().any(|entry| entry == mount_point) {
            state.mount_points.push(mount_point.to_string());
        }
    }
    Ok(LoopbackMount {
        device_name: descriptor.name,
        mount_point: mount_point.to_string(),
        fs_type: fs_label,
    })
}

pub fn umount(mount_point: &str) -> Result<(), &'static str> {
    let mount_record = crate::fs::mount::MOUNT_TABLE
        .list()
        .into_iter()
        .find(|entry| entry.target == mount_point);
    let owner = LOOPBACK_REGISTRY.lock().values().find_map(|state| {
        let guard = state.lock();
        if guard.mount_points.iter().any(|entry| entry == mount_point) {
            Some(guard.name.clone())
        } else {
            None
        }
    });
    crate::fs::mount::MOUNT_TABLE.umount(mount_point)?;
    crate::fs::vfs_unified::VFS_UNIFIED
        .lock()
        .umount(mount_point)
        .map_err(|_| "loopback VFS mount missing")?;
    if let Some(mount) = mount_record {
        match mount.fs_type.as_str() {
            "ext4" => {
                let _ = crate::fs::ext4::unmount_ext4(mount.source.as_str());
            }
            "ntfs" => {
                let _ = crate::fs::ntfs::unmount_ntfs(mount.source.as_str());
            }
            "vfat" | "fat32" => {
                if let Some(index) = mount
                    .source
                    .strip_prefix("fat32:")
                    .and_then(|raw| raw.parse::<usize>().ok())
                {
                    let _ = crate::fs::fat::unmount_fat32(index);
                }
            }
            "exfat" => {
                if let Some(index) = mount
                    .source
                    .strip_prefix("exfat:")
                    .and_then(|raw| raw.parse::<usize>().ok())
                {
                    let _ = crate::fs::fat::unmount_exfat(index);
                }
            }
            _ => {}
        }
    }
    if let Some(owner) = owner {
        if let Some(device) = open(owner.as_str()) {
            let mut state = device.state.lock();
            state.mount_points.retain(|entry| entry != mount_point);
        }
    }
    Ok(())
}

fn register_loopback_storage(
    backing_path: Option<String>,
    block_size: u32,
    read_only: bool,
    storage: LoopbackStorage,
) -> Result<LoopbackDescriptor, &'static str> {
    if block_size == 0 {
        return Err("loopback block size must be non-zero");
    }
    let image_len = storage.image_len();
    if image_len == 0 || image_len % block_size as usize != 0 {
        return Err("loopback image size must align to block size");
    }
    let name = format!("loop{}", NEXT_LOOP_INDEX.fetch_add(1, Ordering::SeqCst));
    let device = DRIVER_MODEL.register_device(name.as_str(), DeviceType::Block);
    device.set_attr("kind", "loopback");
    if let Some(path) = backing_path.as_ref() {
        device.set_attr("backing", path.as_str());
    }
    device.set_attr("storage_mode", storage.storage_mode());
    device.set_attr("block_size", &format!("{}", block_size));
    device.set_attr("blocks", &format!("{}", image_len / block_size as usize));
    device.set_attr("read_only", if read_only { "1" } else { "0" });
    let state = Arc::new(Mutex::new(LoopbackState {
        name: name.clone(),
        backing_path,
        storage,
        block_size,
        read_only,
        mount_points: Vec::new(),
        driver_device_id: device.id,
    }));
    let descriptor = state.lock().descriptor();
    LOOPBACK_REGISTRY.lock().insert(name, state);
    Ok(descriptor)
}

fn find_existing_by_backing(path: &str) -> Option<LoopbackDescriptor> {
    LOOPBACK_REGISTRY.lock().values().find_map(|state| {
        let guard = state.lock();
        match guard.backing_path.as_deref() {
            Some(existing) if existing == path => Some(guard.descriptor()),
            _ => None,
        }
    })
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    }
}

fn block_error_str(error: BlockDeviceError) -> &'static str {
    match error {
        BlockDeviceError::DeviceNotFound => "loopback device not found",
        BlockDeviceError::IoError => "loopback I/O failure",
        BlockDeviceError::InvalidSector => "loopback invalid sector",
        BlockDeviceError::DeviceBusy => "loopback device busy",
        BlockDeviceError::WriteProtected => "loopback device is read-only",
        BlockDeviceError::Timeout => "loopback I/O timeout",
        BlockDeviceError::Unknown => "loopback unknown error",
    }
}

fn can_use_paged_backing(path: &str) -> bool {
    matches!(crate::fs::f2fs::open_entry(path), Ok(entry) if !entry.is_dir)
}

fn detect_loopback_filesystem(
    device: &mut LoopbackBlockDevice,
) -> Result<Option<LoopbackFilesystem>, &'static str> {
    let sector_size = device.block_size() as usize;
    if sector_size < 512 {
        return Ok(None);
    }
    let sector0 = read_loopback_range(device, 0, sector_size)?;
    if let Some(fs) = crate::fs::fat::detect_filesystem(sector0.as_slice()) {
        return Ok(Some(match fs {
            crate::fs::fat::FilesystemType::Fat32 => LoopbackFilesystem::Fat32,
            crate::fs::fat::FilesystemType::ExFat => LoopbackFilesystem::ExFat,
        }));
    }
    let ext4_super = read_loopback_range(device, 1024, 1024)?;
    if crate::fs::ext4::Ext4Superblock::parse(ext4_super.as_slice()).is_some() {
        return Ok(Some(LoopbackFilesystem::Ext4));
    }
    if crate::fs::ntfs::NtfsBootSector::parse(sector0.as_slice()).is_some() {
        return Ok(Some(LoopbackFilesystem::Ntfs));
    }
    Ok(None)
}

fn read_loopback_range(
    device: &mut LoopbackBlockDevice,
    offset: usize,
    len: usize,
) -> Result<Vec<u8>, &'static str> {
    let block_size = device.block_size() as usize;
    let start_block = offset / block_size;
    let end_block = (offset + len + block_size - 1) / block_size;
    let mut blocks = Vec::with_capacity((end_block - start_block) * block_size);
    for lba in start_block..end_block {
        let mut block = alloc::vec![0u8; block_size];
        crate::drivers::block::BlockDevice::read_block(device, lba as u64, block.as_mut_slice())
            .map_err(block_error_str)?;
        blocks.extend_from_slice(block.as_slice());
    }
    let inner = offset % block_size;
    Ok(blocks[inner..inner + len].to_vec())
}

fn read_full_backing(path: &str) -> Result<Vec<u8>, &'static str> {
    crate::fs::vfs_unified::read_file(path).map_err(|_| "loopback backing read failed")
}

fn write_full_backing(path: &str, data: &[u8]) -> Result<(), BlockDeviceError> {
    match request_store_sync(
        0,
        StoreCommand::WriteFile {
            path: path.to_string(),
            data: data.to_vec(),
        },
    ) {
        Some(StoreResponse::Success) => Ok(()),
        _ => Err(BlockDeviceError::IoError),
    }
}

fn validate_block_bounds(
    image_len: usize,
    block_size: usize,
    lba: u64,
) -> Result<usize, BlockDeviceError> {
    let offset = (lba as usize)
        .checked_mul(block_size)
        .ok_or(BlockDeviceError::InvalidSector)?;
    let end = offset
        .checked_add(block_size)
        .ok_or(BlockDeviceError::InvalidSector)?;
    if end > image_len {
        return Err(BlockDeviceError::InvalidSector);
    }
    Ok(offset)
}

fn read_resident_block(
    image: &[u8],
    block_size: usize,
    lba: u64,
    buffer: &mut [u8],
) -> Result<(), BlockDeviceError> {
    let offset = validate_block_bounds(image.len(), block_size, lba)?;
    let end = offset + block_size;
    buffer.copy_from_slice(&image[offset..end]);
    Ok(())
}

fn write_resident_block(
    image: &mut [u8],
    block_size: usize,
    lba: u64,
    buffer: &[u8],
) -> Result<(), BlockDeviceError> {
    let offset = validate_block_bounds(image.len(), block_size, lba)?;
    let end = offset + block_size;
    image[offset..end].copy_from_slice(buffer);
    Ok(())
}

fn read_backing_block(
    path: &str,
    image_len: usize,
    block_size: usize,
    lba: u64,
) -> Result<Vec<u8>, BlockDeviceError> {
    let offset = validate_block_bounds(image_len, block_size, lba)?;
    let mut block = alloc::vec![0u8; block_size];
    let read = crate::fs::f2fs::read_f2fs_file_at(path, offset, block.as_mut_slice())
        .map_err(|_| BlockDeviceError::IoError)?;
    if read != block_size {
        return Err(BlockDeviceError::IoError);
    }
    Ok(block)
}

fn write_backing_block(
    path: &str,
    image_len: usize,
    block_size: usize,
    lba: u64,
    data: &[u8],
) -> Result<(), BlockDeviceError> {
    let offset = validate_block_bounds(image_len, block_size, lba)?;
    let written = crate::fs::f2fs::write_f2fs_file_at(path, offset, data)
        .map_err(|_| BlockDeviceError::IoError)?;
    if written != block_size {
        return Err(BlockDeviceError::IoError);
    }
    Ok(())
}

fn touch_lru(lru: &mut VecDeque<u64>, lba: u64) {
    if let Some(position) = lru.iter().position(|entry| *entry == lba) {
        lru.remove(position);
    }
    lru.push_back(lba);
}

fn evict_clean_cache(
    cache: &mut BTreeMap<u64, Vec<u8>>,
    dirty_blocks: &BTreeMap<u64, Vec<u8>>,
    lru: &mut VecDeque<u64>,
    cache_limit_blocks: usize,
) {
    while cache.len() > cache_limit_blocks {
        let Some(candidate) = lru.pop_front() else {
            break;
        };
        if dirty_blocks.contains_key(&candidate) {
            lru.push_back(candidate);
            if lru.len() > cache_limit_blocks.saturating_mul(2) {
                break;
            }
            continue;
        }
        cache.remove(&candidate);
    }
}

#[cfg(test)]
fn attach_memory_image(
    backing_label: &str,
    image: Vec<u8>,
    block_size: u32,
    read_only: bool,
) -> Result<LoopbackDescriptor, &'static str> {
    register_loopback_storage(
        Some(format!("/mem/{}", backing_label)),
        block_size,
        read_only,
        LoopbackStorage::Resident {
            image,
            dirty: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{attach_memory_image, detach, mount, open, umount};
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn loopback_block_reads_and_writes_roundtrip() {
        let descriptor =
            attach_memory_image("rw.img", vec![0u8; 1024], 512, false).expect("attach");
        let mut device = open(descriptor.name.as_str()).expect("device");
        let mut block = [0u8; 512];
        block[..5].copy_from_slice(b"echos");
        crate::drivers::block::BlockDevice::write_block(&mut device, 1, &block).expect("write");
        let mut readback = [0u8; 512];
        crate::drivers::block::BlockDevice::read_block(&mut device, 1, &mut readback)
            .expect("read");
        assert_eq!(&readback[..5], b"echos");
        detach(descriptor.name.as_str()).expect("detach");
    }

    #[test]
    fn loopback_mounts_fat32_image_into_vfs() {
        let image = fat32_test_image();
        let descriptor = attach_memory_image("fat.img", image, 512, true).expect("attach");
        let mount_point = "/loop-test-fat32";
        let mounted = mount(descriptor.name.as_str(), mount_point, None).expect("mount");
        assert_eq!(mounted.fs_type, "fat32");
        let entries = crate::fs::vfs_unified::list_dir(mount_point).expect("list");
        assert!(entries.iter().any(|entry| entry.name == "HELLO.TXT"));
        umount(mount_point).expect("umount");
        detach(descriptor.name.as_str()).expect("detach");
    }

    #[test]
    fn loopback_mounts_ntfs_image_into_vfs() {
        let image = ntfs_test_image();
        let descriptor = attach_memory_image("ntfs.img", image, 512, true).expect("attach");
        let mount_point = "/loop-test-ntfs";
        let mounted = mount(descriptor.name.as_str(), mount_point, Some("ntfs")).expect("mount");
        assert_eq!(mounted.fs_type, "ntfs");
        let bytes = crate::fs::vfs_unified::read_file("/loop-test-ntfs/hello.txt").expect("read");
        assert_eq!(bytes, b"hello");
        umount(mount_point).expect("umount");
        detach(descriptor.name.as_str()).expect("detach");
    }

    #[test]
    fn loopback_rejects_btrfs_and_xfs_hints_fail_closed() {
        let descriptor =
            attach_memory_image("unsupported.img", vec![0u8; 1024], 512, true).expect("attach");
        assert_eq!(
            mount(descriptor.name.as_str(), "/loop-test-btrfs", Some("btrfs")).unwrap_err(),
            "loopback mount: unsupported filesystem hint"
        );
        assert_eq!(
            mount(descriptor.name.as_str(), "/loop-test-xfs", Some("xfs")).unwrap_err(),
            "loopback mount: unsupported filesystem hint"
        );
        detach(descriptor.name.as_str()).expect("detach");
    }

    fn fat32_test_image() -> Vec<u8> {
        let mut image = vec![0u8; 4 * 512];

        image[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
        image[3..11].copy_from_slice(b"MSWIN4.1");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 1;
        image[21] = 0xF8;
        image[32..36].copy_from_slice(&4u32.to_le_bytes());
        image[36..40].copy_from_slice(&1u32.to_le_bytes());
        image[44..48].copy_from_slice(&2u32.to_le_bytes());
        image[48..50].copy_from_slice(&1u16.to_le_bytes());
        image[50..52].copy_from_slice(&6u16.to_le_bytes());
        image[64] = 0x80;
        image[66] = 0x29;
        image[67..71].copy_from_slice(&0xEC05_5A5Au32.to_le_bytes());
        image[71..82].copy_from_slice(b"ECHOS TEST ");
        image[82..90].copy_from_slice(b"FAT32   ");
        image[510..512].copy_from_slice(&0xAA55u16.to_le_bytes());

        let fat = 512;
        image[fat + 0..fat + 4].copy_from_slice(&0x0FFFFFF8u32.to_le_bytes());
        image[fat + 4..fat + 8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        image[fat + 8..fat + 12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());
        image[fat + 12..fat + 16].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

        let root = 2 * 512;
        image[root..root + 11].copy_from_slice(b"HELLO   TXT");
        image[root + 11] = 0x20;
        image[root + 26..root + 28].copy_from_slice(&3u16.to_le_bytes());
        image[root + 28..root + 32].copy_from_slice(&5u32.to_le_bytes());

        let file = 3 * 512;
        image[file..file + 5].copy_from_slice(b"hello");

        image
    }

    fn ntfs_test_image() -> Vec<u8> {
        let mut image = vec![0u8; 12 * 1024];

        image[3..11].copy_from_slice(b"NTFS    ");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[40..48].copy_from_slice(&24u64.to_le_bytes());
        image[48..56].copy_from_slice(&1u64.to_le_bytes());
        image[56..64].copy_from_slice(&2u64.to_le_bytes());
        image[64] = (-10i8) as u8;
        image[68] = 1;
        image[72..80].copy_from_slice(&0x1122334455667788u64.to_le_bytes());

        let mft_base = 512usize;
        write_test_mft_entry(
            &mut image[mft_base + 5 * 1024..mft_base + 6 * 1024],
            5,
            5,
            "",
            None,
            true,
        );
        write_bitmap_entry(&mut image[mft_base + 6 * 1024..mft_base + 7 * 1024], 6);
        write_test_mft_entry(
            &mut image[mft_base + 8 * 1024..mft_base + 9 * 1024],
            8,
            5,
            "hello.txt",
            Some(b"hello"),
            false,
        );

        image
    }

    fn write_test_mft_entry(
        entry: &mut [u8],
        entry_number: u64,
        parent: u64,
        name: &str,
        data: Option<&[u8]>,
        directory: bool,
    ) {
        entry[0..4].copy_from_slice(b"FILE");
        entry[16..18].copy_from_slice(&1u16.to_le_bytes());
        entry[20..22].copy_from_slice(&56u16.to_le_bytes());
        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let mut filename_payload = vec![0u8; 66 + name_utf16.len() * 2];
        filename_payload[0..8].copy_from_slice(&parent.to_le_bytes());
        filename_payload[56..64]
            .copy_from_slice(&(data.map(|bytes| bytes.len()).unwrap_or(0) as u64).to_le_bytes());
        let flags = if directory { 0x10000000u32 } else { 0x20u32 };
        filename_payload[52..56].copy_from_slice(&flags.to_le_bytes());
        filename_payload[64] = name_utf16.len() as u8;
        filename_payload[65] = 1;
        for (index, code_unit) in name_utf16.iter().enumerate() {
            let offset = 66 + index * 2;
            filename_payload[offset..offset + 2].copy_from_slice(&code_unit.to_le_bytes());
        }

        let mut offset = 56usize;
        offset += write_resident_attr(entry, offset, 0x30, &filename_payload);
        if let Some(bytes) = data {
            offset += write_resident_attr(entry, offset, 0x80, bytes);
        }
        entry[offset..offset + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        entry[24..28].copy_from_slice(&((offset + 8) as u32).to_le_bytes());
        entry[44..48].copy_from_slice(&(entry_number as u32).to_le_bytes());
    }

    fn write_resident_attr(
        entry: &mut [u8],
        offset: usize,
        attr_type: u32,
        payload: &[u8],
    ) -> usize {
        let total_length = 24 + payload.len();
        entry[offset..offset + 4].copy_from_slice(&attr_type.to_le_bytes());
        entry[offset + 4..offset + 8].copy_from_slice(&(total_length as u32).to_le_bytes());
        entry[offset + 8] = 0;
        entry[offset + 9] = 0;
        entry[offset + 10..offset + 12].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 12..offset + 14].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 14..offset + 16].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 16..offset + 20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        entry[offset + 20..offset + 22].copy_from_slice(&24u16.to_le_bytes());
        entry[offset + 22..offset + 24].copy_from_slice(&0u16.to_le_bytes());
        entry[offset + 24..offset + 24 + payload.len()].copy_from_slice(payload);
        total_length
    }

    fn write_bitmap_entry(entry: &mut [u8], entry_number: u64) {
        write_test_mft_entry(
            entry,
            entry_number,
            5,
            "$Bitmap",
            Some(&[0b0000_0111]),
            false,
        );
    }
}
