//! # EchStore - Storage Service
//!
//! Depolama servisi. FAT32 VFS entegrasyonu ve dosya sistemi işlemleri sağlar.
//!
//! ## Özellikler
//!
//! - FAT32 dosya sistemi desteği
//! - Dosya okuma/yazma işlemleri
//! - Dizin işlemleri
//! - Dosya arama ve listeleme
//!
//! ## Mimari
//!
//! EchStore ayrı bir kernel görevi olarak çalışır ve:
//! - FAT32 VFS ile dosya sistemi erişimi sağlar
//! - IPC üzerinden dosya işlemlerini yönetir
//! - Önbellekleme ve performans optimizasyonu yapar

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use lazy_static::lazy_static;
use crate::services::display_atomic::MailboxRing;

const STORE_COMMAND_QUEUE_CAPACITY: usize = 128;
const STORE_RESPONSE_QUEUE_CAPACITY: usize = 128;

/// Dosya sistemi girişi
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub modified_time: u64,
}

/// EchStore servisi komutları
#[derive(Clone, Debug)]
pub enum StoreCommand {
    /// Dosya oku
    ReadFile { path: String },
    /// Dosyaya yaz
    WriteFile { path: String, data: Vec<u8> },
    /// Dosya veya dizin yeniden adlandir
    RenamePath { from: String, to: String },
    /// Dosya sil
    DeleteFile { path: String },
    /// Dizin oluştur
    CreateDirectory { path: String },
    /// Dizin sil
    DeleteDirectory { path: String },
    /// Dizin içeriğini listele
    ListDirectory { path: String },
    /// Dosya ara
    SearchFiles { pattern: String, path: String },
    /// Dosya var mı kontrol et
    FileExists { path: String },
    /// Dosya bilgileri al
    GetFileInfo { path: String },
}

/// Store servisi yanıtı
#[derive(Clone, Debug)]
pub enum StoreResponse {
    /// Dosya içeriği
    FileData(Vec<u8>),
    /// Dizin içeriği
    DirectoryContents(Vec<FileEntry>),
    /// Arama sonuçları
    SearchResults(Vec<FileEntry>),
    /// Dosya bilgileri
    FileInfo(FileEntry),
    /// Boolean sonuç
    BooleanResult(bool),
    /// Başarılı işlem
    Success,
    /// Hata oluştu
    Error(String),
}

/// EchStore servisi
pub struct EchStore {
    /// Çalışma durumu
    running: AtomicBool,
    /// Dosya sistemi önbelleği
    file_cache: Mutex<BTreeMap<String, Vec<u8>>>,
    /// Komut kuyruğu
    command_queue: MailboxRing<StoreCommand>,
    /// Yanıt kuyruğu
    response_queue: MailboxRing<StoreResponse>,
    /// Host-side compatibility hook for legacy direct FAT32 callers
    fat32_vfs: Option<Arc<Mutex<()>>>,
}

impl EchStore {
    /// Yeni EchStore örneği oluştur
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            file_cache: Mutex::new(BTreeMap::new()),
            command_queue: MailboxRing::with_capacity_pow2(STORE_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(STORE_RESPONSE_QUEUE_CAPACITY),
            fat32_vfs: None,
        }
    }

    /// FAT32 VFS'yi ayarla
    pub fn set_fat32_vfs(&mut self, vfs: Arc<Mutex<()>>) {
        self.fat32_vfs = Some(vfs);
    }

    /// Servisi başlat
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHSTORE] Storage service started");
    }

    /// Servisi durdur
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        crate::serial_println!("[ECHSTORE] Storage service stopped");
    }

    /// Komut gönder
    pub fn send_command(&self, command: StoreCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    /// Yanıt al (non-blocking)
    pub fn receive_response(&self) -> Option<StoreResponse> {
        self.response_queue.pop()
    }

    /// Ana servis döngüsü (kernel task olarak çalıştırılır)
    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            // Komutları işle
            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            // Kısa bekleme
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }

    /// Komutu işle
    pub fn process_command(&self, command: StoreCommand) -> StoreResponse {
        match command {
            StoreCommand::ReadFile { path } => self.read_file(&path),
            StoreCommand::WriteFile { path, data } => self.write_file(&path, data),
            StoreCommand::RenamePath { from, to } => self.rename_path(&from, &to),
            StoreCommand::DeleteFile { path } => self.delete_file(&path),
            StoreCommand::CreateDirectory { path } => self.create_directory(&path),
            StoreCommand::DeleteDirectory { path } => self.delete_directory(&path),
            StoreCommand::ListDirectory { path } => self.list_directory(&path),
            StoreCommand::SearchFiles { pattern, path } => self.search_files(&pattern, &path),
            StoreCommand::FileExists { path } => self.file_exists(&path),
            StoreCommand::GetFileInfo { path } => self.get_file_info(&path),
        }
    }

    /// Dosya oku
    fn read_file(&self, path: &str) -> StoreResponse {
        // Önce önbellekten kontrol et
        if let Some(data) = self.file_cache.lock().get(path) {
            return StoreResponse::FileData(data.clone());
        }

        crate::serial_println!("[ECHSTORE] Reading file: {}", path);
        match crate::fs::vfs_unified::read_file(path) {
            Ok(data) => {
                self.file_cache
                    .lock()
                    .insert(String::from(path), data.clone());
                StoreResponse::FileData(data)
            }
            Err(err) => StoreResponse::Error(String::from(err)),
        }
    }

    /// Dosyaya yaz
    fn write_file(&self, path: &str, data: Vec<u8>) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Writing file: {} ({} bytes)", path, data.len());
        if is_virtual_path(path) {
            return StoreResponse::Error(String::from("virtual filesystem is read-only"));
        }

        let normalized = normalize_path(path);
        let result = match crate::fs::f2fs::open_entry(&normalized) {
            Ok(entry) if entry.is_dir => Err(String::from("cannot write directory")),
            Ok(_) => crate::fs::f2fs::write_f2fs_file_at(&normalized, 0, &data)
                .map(|_| ())
                .map_err(fs_error_to_string),
            Err(_) => {
                let (parent, name) = match split_parent_name(&normalized) {
                    Ok(parts) => parts,
                    Err(err) => return StoreResponse::Error(err),
                };
                crate::fs::f2fs::create_f2fs_file_with_data(&parent, &name, &data)
                    .map_err(fs_error_to_string)
            }
        };

        match result {
            Ok(()) => {
                self.file_cache.lock().insert(normalized, data);
                StoreResponse::Success
            }
            Err(err) => StoreResponse::Error(err),
        }
    }

    /// Dosya sil
    fn delete_file(&self, path: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Deleting file: {}", path);
        if is_virtual_path(path) {
            return StoreResponse::Error(String::from(
                "virtual filesystem entry cannot be removed",
            ));
        }

        let normalized = normalize_path(path);
        let (parent, name) = match split_parent_name(&normalized) {
            Ok(parts) => parts,
            Err(err) => return StoreResponse::Error(err),
        };

        match crate::fs::f2fs::unlink_f2fs(&parent, &name) {
            Ok(()) => {
                self.file_cache.lock().remove(&normalized);
                StoreResponse::Success
            }
            Err(err) => StoreResponse::Error(fs_error_to_string(err)),
        }
    }

    /// Dosya veya dizini ayni ebeveyn altinda yeniden adlandir
    fn rename_path(&self, from: &str, to: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Renaming path: {} -> {}", from, to);
        if is_virtual_path(from) || is_virtual_path(to) {
            return StoreResponse::Error(String::from("virtual filesystem is read-only"));
        }

        let from_normalized = normalize_path(from);
        let to_normalized = normalize_path(to);
        let (from_parent, from_name) = match split_parent_name(&from_normalized) {
            Ok(parts) => parts,
            Err(err) => return StoreResponse::Error(err),
        };
        let (to_parent, to_name) = match split_parent_name(&to_normalized) {
            Ok(parts) => parts,
            Err(err) => return StoreResponse::Error(err),
        };
        if from_parent != to_parent {
            return StoreResponse::Error(String::from(
                "cross-directory rename is not supported by ech_store",
            ));
        }

        match crate::fs::f2fs::rename_f2fs(&from_parent, &from_name, &to_name) {
            Ok(()) => {
                self.file_cache.lock().remove(&from_normalized);
                StoreResponse::Success
            }
            Err(err) => StoreResponse::Error(fs_error_to_string(err)),
        }
    }

    /// Dizin oluştur
    fn create_directory(&self, path: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Creating directory: {}", path);
        if is_virtual_path(path) {
            return StoreResponse::Error(String::from("virtual filesystem is read-only"));
        }

        let normalized = normalize_path(path);
        let (parent, name) = match split_parent_name(&normalized) {
            Ok(parts) => parts,
            Err(err) => return StoreResponse::Error(err),
        };

        match crate::fs::f2fs::create_f2fs_dir(&parent, &name) {
            Ok(()) => StoreResponse::Success,
            Err(err) => StoreResponse::Error(fs_error_to_string(err)),
        }
    }

    /// Dizin sil
    fn delete_directory(&self, path: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Deleting directory: {}", path);
        if is_virtual_path(path) {
            return StoreResponse::Error(String::from(
                "virtual filesystem entry cannot be removed",
            ));
        }

        let normalized = normalize_path(path);
        let (parent, name) = match split_parent_name(&normalized) {
            Ok(parts) => parts,
            Err(err) => return StoreResponse::Error(err),
        };

        match crate::fs::f2fs::unlink_f2fs(&parent, &name) {
            Ok(()) => StoreResponse::Success,
            Err(err) => StoreResponse::Error(fs_error_to_string(err)),
        }
    }

    /// Dizin içeriğini listele
    fn list_directory(&self, path: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Listing directory: {}", path);
        match real_list_directory(path) {
            Ok(entries) => StoreResponse::DirectoryContents(entries),
            Err(err) => StoreResponse::Error(err),
        }
    }

    /// Dosya ara
    fn search_files(&self, pattern: &str, path: &str) -> StoreResponse {
        crate::serial_println!("[ECHSTORE] Searching for '{}' in {}", pattern, path);
        let lowered = pattern.to_ascii_lowercase();
        match real_list_directory(path) {
            Ok(entries) => StoreResponse::SearchResults(
                entries
                    .into_iter()
                    .filter(|entry| entry.name.to_ascii_lowercase().contains(&lowered))
                    .collect(),
            ),
            Err(err) => StoreResponse::Error(err),
        }
    }

    /// Dosya var mı kontrol et
    fn file_exists(&self, path: &str) -> StoreResponse {
        let normalized = normalize_path(path);
        let exists =
            self.file_cache.lock().contains_key(&normalized) || real_file_info(&normalized).is_ok();
        StoreResponse::BooleanResult(exists)
    }

    /// Dosya bilgileri al
    fn get_file_info(&self, path: &str) -> StoreResponse {
        match real_file_info(path) {
            Ok(entry) => StoreResponse::FileInfo(entry),
            Err(err) => StoreResponse::Error(err),
        }
    }
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        String::from("/")
    } else if trimmed.starts_with('/') {
        String::from(trimmed)
    } else {
        format!("/{}", trimmed)
    }
}

fn split_parent_name(path: &str) -> Result<(String, String), String> {
    let normalized = normalize_path(path);
    if normalized == "/" {
        return Err(String::from("root path is not mutable"));
    }

    let mut parts = normalized.rsplitn(2, '/');
    let name = parts.next().unwrap_or_default();
    let parent = parts.next().unwrap_or("");
    if name.is_empty() {
        return Err(String::from("path must include a file or directory name"));
    }
    let parent = if parent.is_empty() {
        String::from("/")
    } else {
        String::from(parent)
    };
    Ok((parent, String::from(name)))
}

fn is_virtual_path(path: &str) -> bool {
    let normalized = normalize_path(path);
    matches!(normalized.as_str(), "/proc" | "/sys" | "/dev" | "/tmp")
        || normalized.starts_with("/proc/")
        || normalized.starts_with("/sys/")
        || normalized.starts_with("/dev/")
        || normalized.starts_with("/tmp/")
}

fn real_list_directory(path: &str) -> Result<Vec<FileEntry>, String> {
    let normalized = normalize_path(path);
    match normalized.as_str() {
        "/" => {
            let mut entries = Vec::new();
            if let Ok(root_entries) = crate::fs::f2fs::list_dir("/") {
                for entry in root_entries {
                    entries.push(file_entry_from_f2fs("/", entry));
                }
            }

            for mount in crate::fs::vfs_unified::list_mounts() {
                let mut parts = mount.split_whitespace();
                let _source = parts.next();
                let _on = parts.next();
                let mount_point = parts.next().unwrap_or("/");
                if mount_point == "/" {
                    continue;
                }
                let name = mount_point.trim_start_matches('/').to_string();
                if entries.iter().any(|entry| entry.name == name) {
                    continue;
                }
                entries.push(FileEntry {
                    name,
                    path: String::from(mount_point),
                    size: 0,
                    is_directory: true,
                    modified_time: 0,
                });
            }

            entries.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(entries)
        }
        _ => crate::fs::vfs_unified::list_dir(&normalized)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| FileEntry {
                        path: if normalized == "/" {
                            format!("/{}", entry.name.trim_start_matches('/'))
                        } else {
                            format!("{}/{}", normalized.trim_end_matches('/'), entry.name)
                        },
                        name: entry.name,
                        size: entry.size,
                        is_directory: entry.is_directory,
                        modified_time: 0,
                    })
                    .collect()
            })
            .map_err(String::from),
    }
}

fn real_file_info(path: &str) -> Result<FileEntry, String> {
    let normalized = normalize_path(path);
    if normalized == "/" {
        return Ok(FileEntry {
            name: String::from("/"),
            path: normalized,
            size: 0,
            is_directory: true,
            modified_time: 0,
        });
    }

    let info = crate::fs::vfs_unified::VFS_UNIFIED
        .lock()
        .open(&normalized)
        .map_err(String::from)?;
    let is_directory = (info.mode & 0o170000) == 0o040000;
    Ok(FileEntry {
        name: normalized
            .split('/')
            .next_back()
            .filter(|part| !part.is_empty())
            .unwrap_or("/")
            .to_string(),
        path: normalized,
        size: info.size,
        is_directory,
        modified_time: 0,
    })
}

fn fs_error_to_string(err: rcore_fs::vfs::FsError) -> String {
    format!("{:?}", err)
}

fn file_entry_from_f2fs(parent: &str, entry: crate::fs::f2fs::F2fsEntry) -> FileEntry {
    let parent = normalize_path(parent);
    let path = if parent == "/" {
        format!("/{}", entry.name.trim_start_matches('/'))
    } else if entry.name.starts_with('/') {
        entry.name.clone()
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), entry.name)
    };

    FileEntry {
        name: entry
            .name
            .split('/')
            .next_back()
            .unwrap_or(entry.name.as_str())
            .to_string(),
        path,
        size: entry.size,
        is_directory: entry.is_dir,
        modified_time: 0,
    }
}

/// Global EchStore örneği
lazy_static::lazy_static! {
    static ref ECH_STORE: Arc<EchStore> = Arc::new(EchStore::new());
}

/// EchStore'u başlat
pub fn init() {
    ECH_STORE.start();
    crate::serial_println!("[ECHSTORE] Initialized");
}

/// Global EchStore referansı
pub fn get_store() -> Arc<EchStore> {
    Arc::clone(&ECH_STORE)
}

pub fn service_task() -> ! {
    let svc = get_store();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}
