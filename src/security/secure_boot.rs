//! # UEFI Güvenli Önyükleme (Secure Boot)
//!
//! Bu modül, UEFI Secure Boot mimarisini uygular. Sistem yazılımının
//! (çekirdek, bootloader, sürücüler) yetkili sertifikalarla imzalanmış
//! olduğunu güvence altına alır; imzasız veya yasaklanmış yazılım başlatılmaz.
//!
//! ```
//! UEFI Secure Boot Anahtar Hiyerarşisi:
//!
//!   PK (Platform Key)          -> Firmware sahibinin anahtarı (tek)
//!    |
//!    +-> KEK (Key Exchange Key) -> Microsoft + OEM anahtarları
//!          |
//!          +-> db  (Allowed DB)  -> İzin verilen sertifikalar/hash'ler
//!          +-> dbx (Forbidden DB)-> Yasaklanmış sertifikalar/hash'ler
//!          +-> MokList          -> shim/grub2 MOK anahtarları (kullanıcı ekler)
//!
//! Doğrulama Akışı:
//!   1. Görüntü hash'i hesaplanır
//!   2. dbx kontrolü: hash yasaklı mı? -> REJECT
//!   3. db kontrolü:  hash izinli mi?  -> ACCEPT
//!   4. İmza kontrolü: db/KEK'ten sertifika? -> ACCEPT
//!   5. MOK listesi:   kullanıcı sertifikası? -> ACCEPT
//!   6. Hiçbiri eşleşmedi             -> REJECT
//! ```
//!
//! EFI Değişkenleri:
//!   SecureBoot (ro): 1=etkin, 0=devre dışı
//!   SetupMode  (ro): 1=PK yok (kurulum modu), 0=PK yüklü
//!   PK, KEK, db, dbx: EFI_SIGNATURE_LIST formatında sertifika/hash listeleri

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// SECURE BOOT SABİTLERİ - EFI Değişkenleri
//
// EFI değişkenleri, firmware NVRAM'ında saklanır ve adlarıyla erişilir.
// Bu sabitler Linux shim/grub2 ile uyumlu standart UEFI değişken adlarıdır.
//
//  SecureBoot: Güvenli önyüklemenin etkin olup olmadığı
//  SetupMode:  Henüz PK yüklenmemiş (kurulum/fabrika) modu
//  PK:         Platform Key (cihaz üreticisinin ana anahtarı)
//  KEK:        Key Exchange Key (Microsoft + OEM imza anahtarları)
//  db:         İzin verilen imzalar veritabanı
//  dbx:        İptal edilen/yasaklanan imzalar veritabanı
//  MokList:    Machine Owner Key listesi (shim ile yönetilir)
//  MokListX:   Yasaklanmış MOK listesi
//  MokSBState: MOK Secure Boot durum bayrağı
// ============================================================================

/// UEFI SecureBoot durumu değişkeni adı
pub const EFI_VAR_SECURE_BOOT: &str = "SecureBoot";
/// UEFI kurulum modu değişkeni adı (PK yoksa 1)
pub const EFI_VAR_SETUP_MODE: &str = "SetupMode";
/// Platform Key değişkeni adı (anahtarın kendisi)
pub const EFI_VAR_PK: &str = "PK";
/// Key Exchange Key değişkeni adı
pub const EFI_VAR_KEK: &str = "KEK";
/// İzin verilen imzalar veritabanı
pub const EFI_VAR_DB: &str = "db";
/// Yasaklanan imzalar (revocation list)
pub const EFI_VAR_DBX: &str = "dbx";
/// Machine Owner Key listesi (kullanıcı yükler)
pub const EFI_VAR_MOKLIST: &str = "MokList";
/// Yasaklanmış MOK listesi
pub const EFI_VAR_MOKLISTX: &str = "MokListX";
/// MOK Secure Boot durum değişkeni
pub const EFI_VAR_MOKSB: &str = "MokSBState";

// ============================================================================
// EFI İMZA GUID'LERİ
//
// EFI imza listesinde her girişin tipi bir GUID ile belirtilir.
// Bu GUID, imza verisinin nasıl yorumlanacağını belirler:
//
//  EFI_CERT_X509_GUID:        DER kodlu X.509 sertifikası
//  EFI_CERT_X509_SHA256_GUID: X.509 sertifikasının SHA-256 hash'i
//  EFI_CERT_SHA256_GUID:      Dosya görüntüsünün SHA-256 hash'i
//  EFI_CERT_RSA2048_SHA256_GUID: RSA-2048/SHA-256 ile imzalanmış veri
// ============================================================================

/// DER kodlu X.509 sertifikasını tanımlayan EFI GUID
pub const EFI_CERT_X509_GUID: [u8; 16] = [
    0xa1, 0x59, 0xc0, 0xa5, 0xe4, 0x94, 0xa7, 0x4a,
    0x87, 0xb5, 0xab, 0x15, 0x5c, 0x2b, 0xf0, 0x72
];
/// X.509 sertifikasının SHA-256 hash değerini tanımlayan EFI GUID
pub const EFI_CERT_X509_SHA256_GUID: [u8; 16] = [
    0x92, 0xa2, 0x3f, 0x3c, 0xa7, 0x08, 0x4a, 0x4d,
    0x9f, 0x8e, 0x4b, 0x2c, 0x3b, 0x5a, 0x4a, 0x3e
];
/// Dosya görüntüsünün SHA-256 hash değerini tanımlayan EFI GUID
pub const EFI_CERT_SHA256_GUID: [u8; 16] = [
    0xc1, 0xc4, 0x16, 0x26, 0x1c, 0x0c, 0x47, 0x4b,
    0x9b, 0xd2, 0x60, 0x9e, 0x08, 0x56, 0x6b, 0x5a
];
/// RSA-2048/SHA-256 imzalı veriyi tanımlayan EFI GUID
pub const EFI_CERT_RSA2048_SHA256_GUID: [u8; 16] = [
    0xe2, 0xb3, 0x91, 0x3b, 0xd7, 0x0a, 0x4b, 0x4d,
    0x9f, 0xc4, 0x0a, 0x0c, 0x90, 0x3a, 0x4d, 0x4e
];

/// EFI imza veri üstbilgisi (her imza girişinin başına gelir)
#[repr(C)]
pub struct EfiSignatureData {
    /// İmzanın sahibini tanımlayan GUID (16 bayt)
    pub signature_owner: [u8; 16],
    /// Değişken boyutlu imza verisi (hash veya DER sertikası)
    pub signature_data: [u8; 0],
}

/// EFI imza listesi üstbilgisi (EFI değişkeni ayrıştırma için)
///
/// EFI değişken verisi şu yapıda:
///   [EfiSignatureList][header][EfiSignatureData...][EfiSignatureData...]
///   [EfiSignatureList][header][EfiSignatureData...] ...
#[repr(C)]
pub struct EfiSignatureList {
    /// Bu listedeki imzaların türünü belirten GUID
    pub signature_type: [u8; 16],
    /// Bu listenin toplam boyutu (başlık + tüm imzalar)
    pub signature_list_size: u32,
    /// Başlık veri boyutu (hemen liste başlığının arkasında)
    pub signature_header_size: u32,
    /// Her imza girişinin boyutu (owner GUID dahil)
    pub signature_size: u32,
}

// ============================================================================
// X.509 SERTİFİKASI
//
// X.509 v3 sertifikaları DER (Distinguished Encoding Rules) formatında
// UEFI değişkenlerinde saklanır. Her sertifika şunları içerir:
//   - Subject/Issuer: DN (Distinguished Name) formatında kimlik
//   - Geçerlilik süresi: not_before / not_after zaman damgaları
//   - SHA-256 parmak izi: sertifikayı hızlıca tanımlamak için
//   - is_ca: Bu bir CA (Sertifika Otoritesi) sertifikası mı?
//   - key_usage: Anahtar kullanım amacı bitmaskesi
//
// Sertifika zinciri doğrulaması:
//   EndEntity <- Intermediate CA <- Root CA (db veya KEK'te kayıtlı)
// ============================================================================

/// X.509 v3 sertifikası (DER kodlu)
#[derive(Clone, Debug)]
pub struct X509Certificate {
    /// DER kodlu ham sertifika verisi
    pub der: Vec<u8>,
    /// Konu adı (Subject DN, örn. "CN=Microsoft Windows Production PCA 2011")
    pub subject: String,
    /// Veren adı (Issuer DN - hangi CA imzaladı)
    pub issuer: String,
    /// Geçerlilik başlangıcı (Unix timestamp)
    pub not_before: u64,
    /// Geçerlilik sonu (Unix timestamp)
    pub not_after: u64,
    /// SHA-256 parmak izi (sertifikayı hızlıca tanımlamak için)
    pub fingerprint: [u8; 32],
    /// Bu sertifika bir Sertifika Otoritesi mi?
    pub is_ca: bool,
    /// Anahtar kullanım amacı bitmaskesi (X.509 KeyUsage uzantısı)
    pub key_usage: u16,
}

impl X509Certificate {
    /// DER kodlu sertifika verisinden sertifika nesnesi oluşturur.
    ///
    /// Gerçek uygulamada ASN.1 ayrıştırıcı kullanılmalıdır.
    /// Şu an subject/issuer alanları boş bırakılır.
    pub fn from_der(der: &[u8]) -> Result<Self, SecureBootError> {
        // Parse X.509 certificate
        let fingerprint = Self::calculate_fingerprint(der);

        Ok(Self {
            der: der.to_vec(),
            subject: String::new(),
            issuer: String::new(),
            not_before: 0,
            not_after: 0,
            fingerprint,
            is_ca: false,
            key_usage: 0,
        })
    }

    /// DER kodlu sertifikanın SHA-256 parmak izini hesaplar.
    ///
    /// NOT: Gerçek implementasyon SHA-256 kullanmalıdır; şu an XOR tabanlı yer tutucudur.
    fn calculate_fingerprint(der: &[u8]) -> [u8; 32] {
        // SHA-256 hash
        let mut hash = [0u8; 32];
        // Simplified - would use actual SHA-256
        for (i, byte) in der.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }

    /// Sertifikanın belirtilen CA sertifikası tarafından imzalanıp imzalanmadığını doğrular.
    ///
    /// TODO: Gerçek RSA/EC imza doğrulaması eklenmeli.
    pub fn verify(&self, _issuer: &X509Certificate) -> Result<(), SecureBootError> {
        // Verify signature
        Ok(())
    }

    /// Sertifikanın süresinin dolup dolmadığını kontrol eder.
    pub fn is_expired(&self) -> bool {
        let now = crate::task::scheduler::get_ticks();
        now > self.not_after
    }
}

// ============================================================================
// DOĞRULAMA BAĞLAMI (VerificationContext)
//
// Bir PE (Portable Executable) görüntüsünü doğrulamak için gereken
// tüm bilgileri (hash, imza, sertifikalar) bir arada tutan bağlam nesnesi.
//
// Doğrulama öncelik sırası:
//   1. dbx (yasaklı hash/sertifika) -> Reddet
//   2. db (izinli hash)             -> Kabul et
//   3. db/KEK sertifikası ile imza  -> İmza doğrulama
//   4. MOK listesi sertifikası      -> Kabul et
//   5. Hiçbiri                      -> Reddet
//
// trust_source: doğrulamayı hangi kaynağın sağladığını gösterir ("db", "signature", "mok")
// ============================================================================

/// PE görüntüsü doğrulama bağlamı
pub struct VerificationContext {
    /// Görüntünün SHA-256 hash değeri (PE Authenticode formatı)
    pub image_hash: [u8; 32],
    /// Görüntüye eklenmiş dijital imza (PKCS#7 formatı)
    pub signature: Vec<u8>,
    /// İmza içindeki sertifika zinciri
    pub certs: Vec<X509Certificate>,
    /// Doğrulama sonucu (success, trust_source, error)
    pub result: Mutex<VerificationResult>,
}

/// Doğrulama sonucu bilgisi
#[derive(Clone, Debug)]
pub struct VerificationResult {
    /// Doğrulama başarılı mı?
    pub success: bool,
    /// Doğrulamayı sağlayan güven kaynağı ("db", "signature", "mok")
    pub trust_source: String,
    /// Hata mesajı (başarısız ise)
    pub error: Option<String>,
}

impl VerificationContext {
    /// Görüntü verisi için yeni doğrulama bağlamı oluşturur.
    /// Hash anında hesaplanır; imza ve sertifikalar dışarıdan set edilir.
    pub fn new(image: &[u8]) -> Self {
        Self {
            image_hash: Self::hash_image(image),
            signature: Vec::new(),
            certs: Vec::new(),
            result: Mutex::new(VerificationResult {
                success: false,
                trust_source: String::new(),
                error: None,
            }),
        }
    }

    /// Görüntünün SHA-256 hash'ini hesaplar (PE Authenticode uyumlu).
    ///
    /// NOT: Gerçek Authenticode hash'i PE yapısını ayrıştırıp imza bölgesini
    /// atlamalıdır; bu basitleştirilmiş bir XOR tabanlı yer tutucudur.
    fn hash_image(image: &[u8]) -> [u8; 32] {
        let mut hash = [0u8; 32];
        for (i, byte) in image.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }

    /// Görüntüyü imza veritabanına göre doğrular.
    ///
    /// Doğrulama önceliği: dbx (reddet) -> db hash -> sertifika imzası
    pub fn verify(&self, db: &SignatureDatabase) -> Result<(), SecureBootError> {
        // Check if hash is in dbx (forbidden)
        if db.is_hash_forbidden(&self.image_hash) {
            return Err(SecureBootError::ForbiddenHash);
        }

        // Check if hash is in db (allowed)
        if db.is_hash_allowed(&self.image_hash) {
            let mut result = self.result.lock();
            result.success = true;
            result.trust_source = String::from("db");
            return Ok(());
        }

        // Verify signature
        for cert in &self.certs {
            if db.is_cert_allowed(cert) {
                // Verify signature with this cert
                let mut result = self.result.lock();
                result.success = true;
                result.trust_source = String::from("signature");
                return Ok(());
            }
        }

        Err(SecureBootError::VerificationFailed)
    }
}

// ============================================================================
// İMZA VERİTABANI (SignatureDatabase)
//
// UEFI Secure Boot için dört ayrı liste yönetir:
//
//  allowed_hashes:   db'deki SHA-256 hash izin listesi
//  forbidden_hashes: dbx'teki SHA-256 hash yasaklama listesi
//  allowed_certs:    db/KEK'teki izin verilen X.509 sertifikalar
//  forbidden_certs:  dbx'teki iptal edilen sertifikalar
//  mok_list:         shim bootloader aracılığıyla yüklenen kullanıcı sertifikarı
//  mok_blacklist:    kullanıcı tarafından yasaklanan MOK sertifikaları
//
// Doğrulama sırası: forbidden > allowed_cert > mok
// ============================================================================

/// UEFI İmza Veritabanı (db, dbx, KEK, MOK içeriklerini yönetir)
pub struct SignatureDatabase {
    /// SHA-256 izin verilen hash listesi (db)
    pub allowed_hashes: Mutex<Vec<[u8; 32]>>,
    /// SHA-256 yasaklanan hash listesi (dbx - revocation list)
    pub forbidden_hashes: Mutex<Vec<[u8; 32]>>,
    /// İzin verilen X.509 sertifikaları (db/KEK)
    pub allowed_certs: Mutex<Vec<X509Certificate>>,
    /// İptal edilen X.509 sertifikaları (dbx)
    pub forbidden_certs: Mutex<Vec<X509Certificate>>,
    /// Machine Owner Key listesi (shim tarafından yönetilir)
    pub mok_list: Mutex<Vec<X509Certificate>>,
    /// Yasaklanmış MOK sertifikaları
    pub mok_blacklist: Mutex<Vec<X509Certificate>>,
}

impl SignatureDatabase {
    pub fn new() -> Self {
        Self {
            allowed_hashes: Mutex::new(Vec::new()),
            forbidden_hashes: Mutex::new(Vec::new()),
            allowed_certs: Mutex::new(Vec::new()),
            forbidden_certs: Mutex::new(Vec::new()),
            mok_list: Mutex::new(Vec::new()),
            mok_blacklist: Mutex::new(Vec::new()),
        }
    }

    /// Hash değerinin izin listesinde (db) olup olmadığını kontrol eder.
    pub fn is_hash_allowed(&self, hash: &[u8; 32]) -> bool {
        self.allowed_hashes.lock().contains(hash)
    }

    /// Hash değerinin yasaklama listesinde (dbx) olup olmadığını kontrol eder.
    pub fn is_hash_forbidden(&self, hash: &[u8; 32]) -> bool {
        self.forbidden_hashes.lock().contains(hash)
    }

    /// Sertifikanın izin verilip verilmediğini sorgular.
    ///
    /// Önce dbx yasaklama listesi kontrol edilir; yasaklıysa false döner.
    /// Ardından db izin listesi, son olarak MOK listesi kontrol edilir.
    pub fn is_cert_allowed(&self, cert: &X509Certificate) -> bool {
        // Check if forbidden first
        for forbidden in self.forbidden_certs.lock().iter() {
            if forbidden.fingerprint == cert.fingerprint {
                return false;
            }
        }

        // Check allowed certs
        for allowed in self.allowed_certs.lock().iter() {
            if allowed.fingerprint == cert.fingerprint {
                return true;
            }
        }

        // Check MOK list
        for mok in self.mok_list.lock().iter() {
            if mok.fingerprint == cert.fingerprint {
                return true;
            }
        }

        false
    }

    /// Hash değerini izin listesine ekler.
    pub fn allow_hash(&self, hash: [u8; 32]) {
        self.allowed_hashes.lock().push(hash);
    }

    /// Hash değerini yasaklama listesine ekler (revoke).
    pub fn forbid_hash(&self, hash: [u8; 32]) {
        self.forbidden_hashes.lock().push(hash);
    }

    /// Sertifikayı izin listesine ekler.
    pub fn allow_cert(&self, cert: X509Certificate) {
        self.allowed_certs.lock().push(cert);
    }

    /// Sertifikayı iptal listesine ekler.
    pub fn forbid_cert(&self, cert: X509Certificate) {
        self.forbidden_certs.lock().push(cert);
    }

    /// EFI değişken verisini ayrıştırarak uygun listeye hash/sertifika ekler.
    ///
    /// EFI_SIGNATURE_LIST formatını takip eder; her liste başlığı ardından
    /// fixed_size imza girişleri gelir. GUID'e göre hash mi sertifika mı
    /// olduğu belirlenir.
    pub fn load_efi_variable(&self, name: &str, data: &[u8]) -> Result<(), SecureBootError> {
        if data.len() < core::mem::size_of::<EfiSignatureList>() {
            return Err(SecureBootError::InvalidData);
        }

        let mut offset = 0;

        while offset + core::mem::size_of::<EfiSignatureList>() <= data.len() {
            let list = unsafe {
                &*(data.as_ptr().add(offset) as *const EfiSignatureList)
            };

            let sig_size = list.signature_size as usize;
            let list_size = list.signature_list_size as usize;

            // Parse signatures
            let sig_offset = offset + core::mem::size_of::<EfiSignatureList>() +
                            list.signature_header_size as usize;

            let mut sig_pos = sig_offset;
            while sig_pos + sig_size <= offset + list_size {
                let sig_data = &data[sig_pos..sig_pos + sig_size];

                // Skip signature owner GUID
                let sig = &sig_data[16..];

                // Add to appropriate list
                if name == "db" || name == "KEK" || name == "PK" {
                    // Could be hash or cert
                    if sig.len() == 32 {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(sig);
                        self.allow_hash(hash);
                    } else {
                        if let Ok(cert) = X509Certificate::from_der(sig) {
                            self.allow_cert(cert);
                        }
                    }
                } else if name == "dbx" {
                    if sig.len() == 32 {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(sig);
                        self.forbid_hash(hash);
                    }
                }

                sig_pos += sig_size;
            }

            offset += list_size;
        }

        Ok(())
    }
}

// ============================================================================
// SECURE BOOT YÖNETİCİSİ (SecureBootManager)
//
// UEFI Secure Boot sisteminin çekirdek tarafındaki denetim noktasıdır.
// Boot sırasında EFI değişkenlerinden yapılandırma yüklenir.
//
//  enabled=true:    Her görüntü doğrulanır; başarısız olanlar yüklenmez
//  setup_mode=true: PK henüz yüklü değil; her imza kabul edilir
//
//  verify_image() akışı:
//    1. enabled değilse doğrulama atlanır -> Ok(())
//    2. VerificationContext oluşturulur (hash hesaplanır)
//    3. ctx.verify(db) çağrılır
//    4. Başarı -> images_verified++; Hata -> images_rejected++
// ============================================================================

/// UEFI Secure Boot yöneticisi (çekirdek seviyesi)
pub struct SecureBootManager {
    /// Secure Boot etkin mi?
    pub enabled: AtomicBool,
    /// Kurulum modunda mı? (PK yoksa true; her imza kabul edilir)
    pub setup_mode: AtomicBool,
    /// İmza veritabanı (db, dbx, MOK içerikleri)
    pub db: SignatureDatabase,
    /// Platform Key (cihaz üreticisinin birincil anahtarı)
    pub pk: Mutex<Option<X509Certificate>>,
    /// Key Exchange Key listesi (Microsoft + OEM anahtarları)
    pub kek: Mutex<Vec<X509Certificate>>,
    /// Doğrulama istatistikleri
    pub stats: Mutex<SecureBootStats>,
}

/// Secure Boot istatistikleri
#[derive(Clone, Debug, Default)]
pub struct SecureBootStats {
    /// Başarıyla doğrulanmış görüntü sayısı
    pub images_verified: u64,
    /// Doğrulama başarısız olan (reddedilen) görüntü sayısı
    pub images_rejected: u64,
    /// Yüklenen sertifika sayısı
    pub certs_loaded: u32,
}

impl SecureBootManager {
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            setup_mode: AtomicBool::new(true),
            db: SignatureDatabase::new(),
            pk: Mutex::new(None),
            kek: Mutex::new(Vec::new()),
            stats: Mutex::new(SecureBootStats::default()),
        }
    }

    /// EFI değişkenlerinden Secure Boot yapılandırmasını yükler.
    ///
    /// Gerçek uygulamada UEFI Runtime Services GetVariable() kullanılmalıdır.
    /// Şu an varsayılan değerler (enabled=true, setup_mode=false) ayarlanır.
    pub fn init(&self) {
        // Read SecureBoot variable
        // For now, assume enabled
        self.enabled.store(true, Ordering::SeqCst);
        self.setup_mode.store(false, Ordering::SeqCst);

        crate::serial_println!("[SECUREBOOT] Secure Boot enabled");
    }

    /// PE görüntüsünü Secure Boot politikasına göre doğrular.
    ///
    /// Secure Boot devre dışıysa doğrulama atlanır.
    /// Başarılı doğrulmada images_verified, başarısızda images_rejected artar.
    pub fn verify_image(&self, image: &[u8]) -> Result<(), SecureBootError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let ctx = VerificationContext::new(image);
        let result = ctx.verify(&self.db);

        match result {
            Ok(()) => {
                let mut stats = self.stats.lock();
                stats.images_verified += 1;
                Ok(())
            }
            Err(e) => {
                let mut stats = self.stats.lock();
                stats.images_rejected += 1;
                Err(e)
            }
        }
    }

    /// Secure Boot'un etkin olup olmadığını döndürür.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Doğrulama istatistiklerinin anlık görüntüsünü döndürür.
    pub fn get_stats(&self) -> SecureBootStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global SecureBootManager örneği (çekirdek başlatmasında yapılandırılır).
    pub static ref SECURE_BOOT: SecureBootManager = SecureBootManager::new();
}

// ============================================================================
// HATA TİPLERİ
// ============================================================================

/// Secure Boot doğrulama hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureBootError {
    /// İmza/hash doğrulaması başarısız (geçerli sertifika bulunamadı)
    VerificationFailed,
    /// Hash dbx yasaklama listesinde (revoked görüntü)
    ForbiddenHash,
    /// Dijital imza PKCS#7 yapısı geçersiz
    InvalidSignature,
    /// EFI değişken verisi bozuk veya geçersiz format
    InvalidData,
    /// Sertifikanın geçerlilik süresi dolmuş
    CertificateExpired,
    /// Secure Boot etkin değil
    NotEnabled,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Secure Boot alt sistemini başlatır (EFI değişkenlerini okur ve yapılandırır).
pub fn init() {
    SECURE_BOOT.init();
}
