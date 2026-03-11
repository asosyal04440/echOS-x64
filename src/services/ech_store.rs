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
    command_queue: Mutex<Vec<StoreCommand>>,
    /// Yanıt kuyruğu
    response_queue: Mutex<Vec<StoreResponse>>,
    /// FAT32 VFS referansı
    fat32_vfs: Option<Arc<Mutex<()>>>, // Placeholder for FAT32 VFS
}

impl EchStore {
    /// Yeni EchStore örneği oluştur
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            file_cache: Mutex::new(BTreeMap::new()),
            command_queue: Mutex::new(Vec::new()),
            response_queue: Mutex::new(Vec::new()),
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
    pub fn send_command(&self, command: StoreCommand) {
        self.command_queue.lock().push(command);
    }

    /// Yanıt al (non-blocking)
    pub fn receive_response(&self) -> Option<StoreResponse> {
        self.response_queue.lock().pop()
    }

    /// Ana servis döngüsü (kernel task olarak çalıştırılır)
    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            // Komutları işle
            let commands = {
                let mut queue = self.command_queue.lock();
                core::mem::take(&mut *queue)
            };

            for command in commands {
                let response = self.process_command(command);
                self.response_queue.lock().push(response);
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
        "/proc" => Ok(proc_entries()),
        "/sys" => Ok(static_directory_entries(
            "/sys",
            &["version", "devices", "fs", "kernel"],
        )),
        "/dev" => Ok(static_directory_entries(
            "/dev",
            &["tty", "null", "zero", "random"],
        )),
        "/tmp" => Ok(Vec::new()),
        _ => crate::fs::f2fs::list_dir(&normalized)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| file_entry_from_f2fs(&normalized, entry))
                    .collect()
            })
            .map_err(fs_error_to_string),
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

    if normalized == "/proc" || normalized == "/sys" || normalized == "/dev" || normalized == "/tmp"
    {
        return Ok(FileEntry {
            name: normalized.trim_start_matches('/').to_string(),
            path: normalized,
            size: 0,
            is_directory: true,
            modified_time: 0,
        });
    }

    if normalized.starts_with("/proc/") {
        let size = crate::fs::vfs_unified::read_file(&normalized)
            .map(|data| data.len() as u64)
            .unwrap_or(0);
        return Ok(FileEntry {
            name: normalized
                .split('/')
                .next_back()
                .unwrap_or("proc")
                .to_string(),
            path: normalized,
            size,
            is_directory: false,
            modified_time: 0,
        });
    }

    crate::fs::f2fs::open_entry(&normalized)
        .map(|entry| file_entry_from_f2fs(parent_dir(&normalized).as_str(), entry))
        .map_err(fs_error_to_string)
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

fn parent_dir(path: &str) -> String {
    let normalized = normalize_path(path);
    if normalized == "/" {
        return normalized;
    }
    let mut parts = normalized.rsplitn(2, '/');
    let _name = parts.next();
    let parent = parts.next().unwrap_or("");
    if parent.is_empty() {
        String::from("/")
    } else {
        String::from(parent)
    }
}

fn static_directory_entries(root: &str, names: &[&str]) -> Vec<FileEntry> {
    names
        .iter()
        .map(|name| FileEntry {
            name: String::from(*name),
            path: format!("{}/{}", root.trim_end_matches('/'), name),
            size: 0,
            is_directory: false,
            modified_time: 0,
        })
        .collect()
}

fn proc_entries() -> Vec<FileEntry> {
    [
        "cpuinfo",
        "meminfo",
        "mounts",
        "uptime",
        "version",
        "interrupts",
        "stat",
        "loadavg",
        "driver",
        "self",
    ]
    .iter()
    .map(|name| FileEntry {
        name: String::from(*name),
        path: format!("/proc/{}", name),
        size: 0,
        is_directory: matches!(*name, "driver" | "self"),
        modified_time: 0,
    })
    .collect()
}

fn fs_error_to_string(err: rcore_fs::vfs::FsError) -> String {
    format!("{:?}", err)
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
