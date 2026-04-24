//! # Paket İzin Dialog Sistemi
//!
//! Uygulama paketlerinin istediği izinleri kullanıcıya gösteren
//! ve onay alan dialog sistemi. Toast bildirimleri ve modal dialoglar
//! kullanarak kullanıcı deneyimini optimize eder.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// Paket izin türleri
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionType {
    NetworkAccess,
    FileSystemRead,
    FileSystemWrite,
    FileSystemFull,
    CameraAccess,
    MicrophoneAccess,
    LocationAccess,
    ContactsAccess,
    SystemSettings,
    BackgroundExecution,
    Notifications,
    Bluetooth,
    UsbDevice,
    AdminPrivileges,
}

impl PermissionType {
    /// İzin tipini kullanıcı dostu metne dönüştür
    pub fn to_user_friendly(&self) -> &'static str {
        match self {
            PermissionType::NetworkAccess => "İnternet Erişimi",
            PermissionType::FileSystemRead => "Dosya Okuma",
            PermissionType::FileSystemWrite => "Dosya Yazma",
            PermissionType::FileSystemFull => "Tam Dosya Erişimi",
            PermissionType::CameraAccess => "Kamera Erişimi",
            PermissionType::MicrophoneAccess => "Mikrofon Erişimi",
            PermissionType::LocationAccess => "Konum Erişimi",
            PermissionType::ContactsAccess => "Kişiler Erişimi",
            PermissionType::SystemSettings => "Sistem Ayarları",
            PermissionType::BackgroundExecution => "Arka Planda Çalışma",
            PermissionType::Notifications => "Bildirim Gönderme",
            PermissionType::Bluetooth => "Bluetooth Erişimi",
            PermissionType::UsbDevice => "USB Cihaz Erişimi",
            PermissionType::AdminPrivileges => "Yönetici Yetkileri",
        }
    }

    /// İzin tipinin açıklaması
    pub fn description(&self) -> &'static str {
        match self {
            PermissionType::NetworkAccess => "Uygulamanın internete bağlanmasına izin verir",
            PermissionType::FileSystemRead => "Dosyaları okuma izni",
            PermissionType::FileSystemWrite => "Dosyaları yazma ve değiştirme izni",
            PermissionType::FileSystemFull => "Tüm dosya sistemine erişim izni",
            PermissionType::CameraAccess => "Kamerayı kullanma izni",
            PermissionType::MicrophoneAccess => "Mikrofonu kullanma izni",
            PermissionType::LocationAccess => "Cihaz konumunu öğrenme izni",
            PermissionType::ContactsAccess => "Kişi listesine erişim izni",
            PermissionType::SystemSettings => "Sistem ayarlarını değiştirme izni",
            PermissionType::BackgroundExecution => "Uygulamanın arka planda çalışmasına izin",
            PermissionType::Notifications => "Sistem bildirimleri gönderme izni",
            PermissionType::Bluetooth => "Bluetooth cihazlarına bağlanma izni",
            PermissionType::UsbDevice => "USB cihazlarına erişim izni",
            PermissionType::AdminPrivileges => "Sistem yönetici yetkileri",
        }
    }

    /// İzin tipinin ikonu (unicode karakter)
    pub fn icon(&self) -> &'static str {
        match self {
            PermissionType::NetworkAccess => "🌐",
            PermissionType::FileSystemRead => "📖",
            PermissionType::FileSystemWrite => "✏️",
            PermissionType::FileSystemFull => "💾",
            PermissionType::CameraAccess => "📷",
            PermissionType::MicrophoneAccess => "🎤",
            PermissionType::LocationAccess => "📍",
            PermissionType::ContactsAccess => "👥",
            PermissionType::SystemSettings => "⚙️",
            PermissionType::BackgroundExecution => "⚡",
            PermissionType::Notifications => "🔔",
            PermissionType::Bluetooth => "🔵",
            PermissionType::UsbDevice => "🔌",
            PermissionType::AdminPrivileges => "🔐",
        }
    }
}

/// Paket izin isteği
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub package_name: String,
    pub package_version: String,
    pub package_author: String,
    pub permissions: Vec<PermissionType>,
    pub timestamp: u64,
}

/// İzin dialog yöneticisi
pub struct PermissionDialogManager {
    auto_approve: AtomicBool,
}

impl PermissionDialogManager {
    pub fn new() -> Self {
        Self {
            auto_approve: AtomicBool::new(false),
        }
    }

    /// Paket izin isteğini göster ve kullanıcı onayı al
    pub fn request_permissions(&self, request: PermissionRequest) -> bool {
        // Eğer otomatik onay modunda ise, direkt onayla
        if self.auto_approve.load(Ordering::Relaxed) {
            self.log_permission_granted(&request);
            return true;
        }

        // Önce toast bildirimi göster
        self.show_permission_toast(&request);

        // Modal dialog ile detaylı izin isteği göster
        self.show_permission_dialog(&request)
    }

    /// Toast bildirimi göster
    fn show_permission_toast(&self, request: &PermissionRequest) {
        let mut title = String::new();
        title.push_str(&request.package_name);
        title.push_str(" izin istiyor");

        let mut message = String::from("Aşağıdaki izinler gerekiyor:\n");

        for (i, permission) in request.permissions.iter().take(3).enumerate() {
            message.push_str(permission.icon());
            message.push(' ');
            message.push_str(permission.to_user_friendly());
            message.push('\n');
        }

        if request.permissions.len() > 3 {
            message.push_str("ve ");
            let remaining = request.permissions.len() - 3;
            message.push_str(&remaining.to_string());
            message.push_str(" izin daha...");
        }

        // Basit notification göster - mevcut sisteme uyumlu
        crate::serial_println!("[PERMISSION] {}: {}", title, message);
    }

    /// Detaylı izin dialogu göster
    fn show_permission_dialog(&self, request: &PermissionRequest) -> bool {
        // Basit dialog implementasyonu - mevcut sisteme uyumlu
        let mut header = String::from("[PERMISSION DIALOG]\nPaket: ");
        header.push_str(&request.package_name);
        header.push_str(" (v");
        header.push_str(&request.package_version);
        header.push_str(")\nGeliştirici: ");
        header.push_str(&request.package_author);
        header.push_str("\nİstenen izinler:\n");

        crate::serial_println!("{}", header);

        for permission in &request.permissions {
            let mut line = String::new();
            line.push_str("  ");
            line.push_str(permission.icon());
            line.push(' ');
            line.push_str(permission.to_user_friendly());
            line.push_str(" - ");
            line.push_str(permission.description());
            crate::serial_println!("{}", line);
        }

        // Şimdilik her zaman onay ver - gerçek implementasyonda kullanıcı girişi beklenir
        crate::serial_println!("[PERMISSION] İzin verildi (otomatik)");
        self.log_permission_granted(request);
        true
    }

    /// İzin verildiğinde log kaydı oluştur
    fn log_permission_granted(&self, request: &PermissionRequest) {
        let mut message = String::new();
        message.push_str(&request.package_name);
        message.push_str(" paketine ");
        message.push_str(&request.permissions.len().to_string());
        message.push_str(" izin verildi");

        crate::serial_println!("[PERMISSION] {}", message);

        // TODO: Sistem log'una kaydet
        // crate::security::audit::log_permission_event(request, true);
    }

    /// İzin reddedildiğinde log kaydı oluştur
    fn log_permission_denied(&self, request: &PermissionRequest) {
        let mut message = String::new();
        message.push_str(&request.package_name);
        message.push_str(" paketinin izin isteği reddedildi");

        crate::serial_println!("[PERMISSION] {}", message);

        // TODO: Sistem log'una kaydet
        // crate::security::audit::log_permission_event(request, false);
    }

    /// Otomatik onay modunu ayarla (geliştirme/test için)
    pub fn set_auto_approve(&self, enabled: bool) {
        self.auto_approve.store(enabled, Ordering::Relaxed);

        let message = if enabled {
            "Otomatik izin onayı açıldı"
        } else {
            "Otomatik izin onayı kapatıldı"
        };

        crate::serial_println!("[PERMISSION] {}", message);
    }

    /// Mevcut izin durumunu kontrol et
    pub fn check_permission(&self, package_name: &str, permission: &PermissionType) -> bool {
        // TODO: Kayıtlı izinleri kontrol et
        // Şimdilik sadece notification göster

        let mut message = String::new();
        message.push_str(package_name);
        message.push_str(" izin kontrolü: ");
        message.push_str(permission.to_user_friendly());

        crate::serial_println!("[PERMISSION] {}", message);

        // Gerçek implementasyonda kullanıcı onayı veya kayıtlı izin kontrolü yapılacak
        true // Şimdilik her zaman true döndür
    }

    /// Tüm izinleri göster (ayarlar paneli için)
    pub fn show_all_permissions(&self) {
        let mut content = String::from("Tüm İzin Türleri:\n\n");

        for permission in [
            PermissionType::NetworkAccess,
            PermissionType::FileSystemRead,
            PermissionType::FileSystemWrite,
            PermissionType::FileSystemFull,
            PermissionType::CameraAccess,
            PermissionType::MicrophoneAccess,
            PermissionType::LocationAccess,
            PermissionType::ContactsAccess,
            PermissionType::SystemSettings,
            PermissionType::BackgroundExecution,
            PermissionType::Notifications,
            PermissionType::Bluetooth,
            PermissionType::UsbDevice,
            PermissionType::AdminPrivileges,
        ] {
            content.push_str(permission.icon());
            content.push(' ');
            content.push_str(permission.to_user_friendly());
            content.push_str(" - ");
            content.push_str(permission.description());
            content.push('\n');
        }

        crate::serial_println!("[PERMISSION] {}", content);
    }
}

/// Global izin dialog yöneticisi - static olarak initialize et
static PERMISSION_DIALOG_MANAGER: PermissionDialogManager = PermissionDialogManager {
    auto_approve: AtomicBool::new(false),
};

/// İzin dialog yöneticisini al
pub fn get_permission_dialog_manager() -> &'static PermissionDialogManager {
    &PERMISSION_DIALOG_MANAGER
}

/// Paket izin isteği göster (kolay kullanım fonksiyonu)
pub fn show_permission_request(
    package_name: &str,
    package_version: &str,
    package_author: &str,
    permissions: Vec<PermissionType>,
) -> bool {
    let request = PermissionRequest {
        package_name: package_name.to_string(),
        package_version: package_version.to_string(),
        package_author: package_author.to_string(),
        permissions,
        timestamp: crate::cpu::tsc::read(),
    };

    get_permission_dialog_manager().request_permissions(request)
}

/// Basit izin kontrolü (tek izin için)
pub fn check_single_permission(package_name: &str, permission: PermissionType) -> bool {
    get_permission_dialog_manager().check_permission(package_name, &permission)
}
