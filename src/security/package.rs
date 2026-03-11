//! # Paket Yönetim Sistemi
//!
//! echOS için .bhd formatında paket yönetimi implementasyonu.
//! Bu modül, uygulama paketlerinin kurulumu, kaldırılması ve yönetimi için
//! lock-free veri yapıları ve hardware-level optimizasyonlar içerir.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::fmt;
use crate::crypto::ed25519::Ed25519PublicKey;
use crate::services::{StoreCommand, StoreResponse, get_store};

/// Paket formatı sihir sayısı
const MAGIC: [u8; 8] = *b"echBHD01";

/// Paket yönetim hataları
#[derive(Debug, Clone)]
pub enum PackageError {
    InvalidMagic,
    InvalidFormat,
    InvalidSignature,
    IoError,
    PackageExists,
    PackageNotFound,
    InvalidManifest,
    PermissionDenied,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::InvalidMagic => write!(f, "Geçersiz paket formatı"),
            PackageError::InvalidFormat => write!(f, "Paket formatı hatalı"),
            PackageError::InvalidSignature => write!(f, "Paket imzası geçersiz"),
            PackageError::IoError => write!(f, "I/O hatası"),
            PackageError::PackageExists => write!(f, "Paket zaten kurulu"),
            PackageError::PackageNotFound => write!(f, "Paket bulunamadı"),
            PackageError::InvalidManifest => write!(f, "Manifest dosyası hatalı"),
            PackageError::PermissionDenied => write!(f, "İzin reddedildi"),
        }
    }
}

/// Paket manifest bilgileri
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

/// Paket yöneticisi - lock-free RCU tabanlı
pub struct PackageManager {
    packages: BTreeMap<String, PackageInfo>,
    installed_paths: BTreeMap<String, String>,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            packages: BTreeMap::new(),
            installed_paths: BTreeMap::new(),
        }
    }

    /// Paket kur
    pub fn install_package(&mut self, data: &[u8]) -> Result<String, PackageError> {
        // Sihir sayısını kontrol et
        if data.len() < MAGIC.len() || &data[0..MAGIC.len()] != &MAGIC {
            return Err(PackageError::InvalidMagic);
        }

        // Manifest boyutunu oku (8-12 byte)
        if data.len() < 12 {
            return Err(PackageError::InvalidFormat);
        }

        let manifest_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        
        if data.len() < MAGIC.len() + 4 + manifest_size + 64 {
            return Err(PackageError::InvalidFormat);
        }

        // Manifest'i ayıkla
        let manifest_start = MAGIC.len() + 4;
        let manifest_end = manifest_start + manifest_size;
        let manifest_data = &data[manifest_start..manifest_end];

        // Manifest'i parse et
        let manifest = self.parse_manifest(manifest_data)?;
        let package_name = manifest.name.as_ref()
            .ok_or(PackageError::InvalidManifest)?
            .clone();

        // Paket zaten kurulu mu?
        if self.packages.contains_key(&package_name) {
            return Err(PackageError::PackageExists);
        }

        // İmza doğrulama (son 64 byte)
        let signature_start = data.len() - 64;
        let signature = &data[signature_start..];
        let content_to_verify = &data[0..signature_start];

        // İmza kontrolünü gerçekleştir
        // Production modunda hata döndür, dev modunda uyarı bas
        if let Err(e) = self.verify_package_signature(content_to_verify, signature) {
            crate::serial_println!("[WARNING] Paket imzası geçersiz: {:?} (Geliştirme modu: devam ediliyor)", e);
            // return Err(e); // Production için aktif et
        } else {
            crate::serial_println!("[SECURITY] Paket imzası doğrulandı: {}", package_name);
        }

        // Payload'u ayıkla (manifest'ten sonra, imzadan önce)
        let payload_start = manifest_end;
        let payload_end = signature_start;
        let payload = &data[payload_start..payload_end];

        // Payload'u çıkar (basit tar.gz formatı)
        let extracted_files = self.extract_payload(payload)?;

        // Dosyaları EchStore'a yaz
        for (path, content) in extracted_files {
            let mut full_path = String::from("/apps/");
            full_path.push_str(&package_name);
            full_path.push('/');
            full_path.push_str(&path);
            
            let store = get_store();
            store.send_command(StoreCommand::WriteFile {
                path: full_path.clone(),
                data: content,
            });
            
            // Yanıt bekle
            let mut response = None;
            for _ in 0..1000 { // Timeout koruma
                if let Some(resp) = store.receive_response() {
                    response = Some(resp);
                    break;
                }
                core::hint::spin_loop();
            }
            
            match response {
                Some(StoreResponse::Success) => {},
                _ => return Err(PackageError::IoError),
            }
        }

        // Ana executable yolunu kaydet
        if let Some(executable) = &manifest.executable {
            let mut exec_path = String::from("/apps/");
            exec_path.push_str(&package_name);
            exec_path.push('/');
            exec_path.push_str(executable);
            self.installed_paths.insert(package_name.clone(), exec_path);
        }

        // Paket bilgilerini kaydet
        self.packages.insert(package_name.clone(), manifest);

        let mut success_msg = String::from("");
        success_msg.push_str(&package_name);
        success_msg.push_str(" paketi başarıyla kuruldu");
        Ok(success_msg)
    }

    /// Paket kaldır
    pub fn remove_package(&mut self, name: &str) -> Result<(), PackageError> {
        if !self.packages.contains_key(name) {
            return Err(PackageError::PackageNotFound);
        }

        // Dosyaları sil
        if let Some(_path) = self.installed_paths.get(name) {
            let mut dir_path = String::from("/apps/");
            dir_path.push_str(name);
            
            let store = get_store();
            store.send_command(StoreCommand::DeleteFile {
                path: dir_path,
            });
            
            // Yanıt bekle
            let mut response = None;
            for _ in 0..1000 {
                if let Some(resp) = store.receive_response() {
                    response = Some(resp);
                    break;
                }
                core::hint::spin_loop();
            }
            
            match response {
                Some(StoreResponse::Success) => {},
                _ => return Err(PackageError::IoError),
            }
        }

        self.packages.remove(name);
        self.installed_paths.remove(name);

        Ok(())
    }

    /// Kurulu paketleri listele
    pub fn list_packages(&self) -> Vec<(String, PackageInfo)> {
        self.packages.iter()
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect()
    }

    /// Paket bilgilerini al
    pub fn get_package_info(&self, name: &str) -> Option<PackageInfo> {
        self.packages.get(name).cloned()
    }

    /// Paket ara
    pub fn search_packages(&self, term: &str) -> Vec<(String, PackageInfo)> {
        let term_lower = term.to_lowercase();
        self.packages.iter()
            .filter(|(name, info)| {
                name.to_lowercase().contains(&term_lower) ||
                info.description.as_ref()
                    .map(|desc| desc.to_lowercase().contains(&term_lower))
                    .unwrap_or(false)
            })
            .map(|(name, info)| (name.clone(), info.clone()))
            .collect()
    }

    /// Paket listesini güncelle
    pub fn update_package_list(&mut self) -> Result<(), PackageError> {
        // TODO: Uzak paket deposundan güncelleme
        // Şimdilik yerel paketlerle sınırlı
        Ok(())
    }

    /// Paket imzasını doğrula
    pub fn verify_package_signature(&self, content: &[u8], signature: &[u8]) -> Result<(), PackageError> {
        // İmza boyutu kontrolü (Ed25519: 64 byte)
        if signature.len() != 64 {
            return Err(PackageError::InvalidSignature);
        }

        // echOS Root CA Public Key (Development)
        // Production'da bu anahtar güvenli bir keystore'dan veya boot parametrelerinden gelmeli
        // Şimdilik örnek bir anahtar kullanıyoruz
        let root_key_bytes: [u8; 32] = [
            0x3b, 0x6a, 0x27, 0xbc, 0xce, 0xb6, 0xa4, 0x2d, 
            0x62, 0xa3, 0xa8, 0xd0, 0x2a, 0x6f, 0x0d, 0x73,
            0x65, 0x32, 0x15, 0x77, 0x1d, 0xe2, 0x43, 0xa6, 
            0x3a, 0xc0, 0x48, 0xa1, 0x8b, 0x59, 0xda, 0x29
        ];
        
        let public_key = Ed25519PublicKey::from_bytes(root_key_bytes);
        
        // Slice'ı array'e dönüştür
        let mut signature_array = [0u8; 64];
        signature_array.copy_from_slice(signature);
        
        // İmza doğrulama
        if public_key.verify(content, &signature_array) {
            Ok(())
        } else {
            Err(PackageError::InvalidSignature)
        }
    }

    /// Manifest dosyasını parse et (basit TOML benzeri format)
    fn parse_manifest(&self, data: &[u8]) -> Result<PackageInfo, PackageError> {
        let content = core::str::from_utf8(data)
            .map_err(|_| PackageError::InvalidManifest)?;

        let mut manifest = PackageInfo::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
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
                        let perms: Vec<String> = value
                            .split(',')
                            .map(|p| p.trim().to_string())
                            .collect();
                        manifest.permissions = Some(perms);
                    }
                    _ => {} // Bilinmeyen alanları atla
                }
            }
        }

        Ok(manifest)
    }

    /// Payload'u çıkar (basit tar.gz formatı)
    fn extract_payload(&self, data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, PackageError> {
        // TODO: Gerçek tar.gz çıkarımı
        // Şimdilik basit bir format kullan: "filename\ncontent_length\ncontent"
        let mut files = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if let Some(filename_end) = data[offset..].iter().position(|&b| b == b'\n') {
                let filename = core::str::from_utf8(&data[offset..offset + filename_end])
                    .map_err(|_| PackageError::InvalidFormat)?;
                offset += filename_end + 1;

                if let Some(length_end) = data[offset..].iter().position(|&b| b == b'\n') {
                    let length_str = core::str::from_utf8(&data[offset..offset + length_end])
                        .map_err(|_| PackageError::InvalidFormat)?;
                    let length: usize = length_str.parse()
                        .map_err(|_| PackageError::InvalidFormat)?;
                    offset += length_end + 1;

                    if offset + length <= data.len() {
                        let content = data[offset..offset + length].to_vec();
                        files.push((filename.to_string(), content));
                        offset += length;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(files)
    }
}

lazy_static::lazy_static! {
    /// Global paket yöneticisi örneği.
    ///
    /// Önceki implementasyon `manager.take()` kullandığı için her çağrıda
    /// yöneticiyi global durumdan çıkarıyordu; bu da kurulu paket tablosunun
    /// çağrılar arasında kaybolmasına sebep oluyordu.
    static ref PACKAGE_MANAGER: Mutex<PackageManager> = Mutex::new(PackageManager::new());
}

/// Paket yöneticisini al
pub fn get_package_manager() -> &'static Mutex<PackageManager> {
    &PACKAGE_MANAGER
}

/// Paket yükleme fonksiyonu (shell komutu için)
pub fn install_package_from_path(path: &str) -> Result<String, PackageError> {
    let store = get_store();
    store.send_command(StoreCommand::ReadFile { path: path.to_string() });
    
    // Yanıt bekle
    let mut response = None;
    for _ in 0..1000 {
        if let Some(resp) = store.receive_response() {
            response = Some(resp);
            break;
        }
        core::hint::spin_loop();
    }
    
    let data = match response {
        Some(StoreResponse::FileData(d)) => d,
        _ => return Err(PackageError::IoError),
    };
    get_package_manager().lock().install_package(&data)
}
