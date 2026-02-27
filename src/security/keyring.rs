//! # Anahtar Halkası (Keyring)
//!
//! Çekirdek anahtar saklama hizmeti.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// ANAHTARLIK SABİTLERİ
// ============================================================================

/// Anahtar türleri
pub const KEY_TYPE_KEYRING: &str = "keyring";
pub const KEY_TYPE_USER: &str = "user";
pub const KEY_TYPE_ENCRYPTED: &str = "encrypted";
pub const KEY_TYPE_TRUSTED: &str = "trusted";
pub const KEY_TYPE_LOGON: &str = "logon";
pub const KEY_TYPE_BIG_KEY: &str = "big_key";

/// Anahtar izinleri
pub const KEY_POS_VIEW: u32 = 0x01000000;
pub const KEY_POS_READ: u32 = 0x02000000;
pub const KEY_POS_WRITE: u32 = 0x04000000;
pub const KEY_POS_SEARCH: u32 = 0x08000000;
pub const KEY_POS_LINK: u32 = 0x10000000;
pub const KEY_POS_SETATTR: u32 = 0x20000000;

pub const KEY_USR_VIEW: u32 = 0x00010000;
pub const KEY_USR_READ: u32 = 0x00020000;
pub const KEY_USR_WRITE: u32 = 0x00040000;
pub const KEY_USR_SEARCH: u32 = 0x00080000;
pub const KEY_USR_LINK: u32 = 0x00100000;
pub const KEY_USR_SETATTR: u32 = 0x00200000;

pub const KEY_GRP_VIEW: u32 = 0x00000100;
pub const KEY_GRP_READ: u32 = 0x00000200;
pub const KEY_GRP_WRITE: u32 = 0x00000400;
pub const KEY_GRP_SEARCH: u32 = 0x00000800;
pub const KEY_GRP_LINK: u32 = 0x00001000;
pub const KEY_GRP_SETATTR: u32 = 0x00002000;

pub const KEY_OTH_VIEW: u32 = 0x00000001;
pub const KEY_OTH_READ: u32 = 0x00000002;
pub const KEY_OTH_WRITE: u32 = 0x00000004;
pub const KEY_OTH_SEARCH: u32 = 0x00000008;
pub const KEY_OTH_LINK: u32 = 0x00000010;
pub const KEY_OTH_SETATTR: u32 = 0x00000020;

/// Özel anahtarlık kimlikleri
pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;
pub const KEY_SPEC_REQUESTOR_KEYRING: i32 = -8;

// ============================================================================
// ANAHTAR
// ============================================================================

pub struct Key {
    /// Anahtar seri numarası
    pub serial: AtomicU64,
    /// Anahtar türü
    pub key_type: String,
    /// Anahtar açıklaması
    pub description: String,
    /// Anahtar verisi
    pub payload: Mutex<Vec<u8>>,
    /// İzinler
    pub permissions: AtomicU32,
    /// Kullanıcı kimliği
    pub uid: AtomicU32,
    /// Grup kimliği
    pub gid: AtomicU32,
    /// Oluşturulma zamanı
    pub created: AtomicU64,
    /// Son kullanma zamanı (0 = hiçbir zaman)
    pub expiry: AtomicU64,
    /// İptal edildi mi
    pub revoked: AtomicBool,
    /// Örneklendi mi
    pub instantiated: AtomicBool,
    /// Başvuru sayısı
    pub ref_count: AtomicU32,
}

impl Key {
    pub fn new(serial: u64, key_type: &str, description: &str) -> Self {
        Self {
            serial: AtomicU64::new(serial),
            key_type: String::from(key_type),
            description: String::from(description),
            payload: Mutex::new(Vec::new()),
            permissions: AtomicU32::new(
                KEY_POS_VIEW | KEY_POS_READ | KEY_POS_SEARCH | KEY_POS_LINK |
                KEY_USR_VIEW | KEY_USR_READ | KEY_USR_SEARCH | KEY_USR_LINK |
                KEY_GRP_VIEW | KEY_GRP_READ | KEY_GRP_SEARCH | KEY_GRP_LINK |
                KEY_OTH_VIEW | KEY_OTH_READ | KEY_OTH_SEARCH | KEY_OTH_LINK
            ),
            uid: AtomicU32::new(0),
            gid: AtomicU32::new(0),
            created: AtomicU64::new(crate::task::scheduler::get_ticks()),
            expiry: AtomicU64::new(0),
            revoked: AtomicBool::new(false),
            instantiated: AtomicBool::new(false),
            ref_count: AtomicU32::new(1),
        }
    }

    /// İzni kontrol eder
    pub fn check_permission(&self, perm: u32, uid: u32, gid: u32) -> bool {
        let perms = self.permissions.load(Ordering::Relaxed);

        if uid == self.uid.load(Ordering::Relaxed) {
            // Sahip izinleri
            (perms & (perm << 16)) != 0
        } else if gid == self.gid.load(Ordering::Relaxed) {
            // Grup izinleri
            (perms & (perm << 8)) != 0
        } else {
            // Diğer izinler
            (perms & perm) != 0
        }
    }

    /// Yük verisini okur
    pub fn read(&self) -> Option<Vec<u8>> {
        if self.revoked.load(Ordering::Relaxed) {
            return None;
        }
        Some(self.payload.lock().clone())
    }

    /// Yük verisini yazar
    pub fn write(&self, data: &[u8]) {
        *self.payload.lock() = data.to_vec();
        self.instantiated.store(true, Ordering::SeqCst);
    }

    /// Anahtarı iptal eder
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }

    /// Yük verisini günceller
    pub fn update(&self, data: &[u8]) -> Result<(), KeyError> {
        if self.revoked.load(Ordering::Relaxed) {
            return Err(KeyError::Revoked);
        }
        *self.payload.lock() = data.to_vec();
        Ok(())
    }

    /// Seri numarasını getirir
    pub fn get_serial(&self) -> u64 {
        self.serial.load(Ordering::Relaxed)
    }
}

// ============================================================================
// ANAHTARLIK
// ============================================================================

pub struct Keyring {
    /// Anahtar yapısı
    pub key: Arc<Key>,
    /// Bağlı anahtarlar (seri -> anahtar)
    pub links: Mutex<BTreeMap<u64, Arc<Key>>>,
}

impl Keyring {
    pub fn new(serial: u64, description: &str) -> Self {
        let key = Arc::new(Key::new(serial, KEY_TYPE_KEYRING, description));
        key.instantiated.store(true, Ordering::SeqCst);

        Self {
            key,
            links: Mutex::new(BTreeMap::new()),
        }
    }

    /// Anahtar bağlar
    pub fn link(&self, key: Arc<Key>) {
        let serial = key.get_serial();
        self.links.lock().insert(serial, key);
    }

    /// Anahtar bağlantısını kaldırır
    pub fn unlink(&self, serial: u64) -> Option<Arc<Key>> {
        self.links.lock().remove(&serial)
    }

    /// Açıklamaya göre anahtar arar
    pub fn search(&self, description: &str) -> Option<Arc<Key>> {
        for key in self.links.lock().values() {
            if key.description == description {
                return Some(key.clone());
            }
        }
        None
    }

    /// Tüm anahtarları listeler
    pub fn list(&self) -> Vec<u64> {
        self.links.lock().keys().copied().collect()
    }
}

// ============================================================================
// ANAHTAR YÖNETİCİSİ
// ============================================================================

pub struct KeyManager {
    /// Seri numarasına göre tüm anahtarlar
    keys: Mutex<BTreeMap<u64, Arc<Key>>>,
    /// Seri numarasına göre tüm anahtarlıklar
    keyrings: Mutex<BTreeMap<u64, Arc<Keyring>>>,
    /// İş parçacığı başına anahtarlık
    thread_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// İşlem başına anahtarlık
    process_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// Oturum başına anahtarlık
    session_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// Kullanıcı anahtarlıkları
    user_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// Sonraki seri numarası
    next_serial: AtomicU64,
    /// İstatistikler
    stats: Mutex<KeyStats>,
}

#[derive(Clone, Debug, Default)]
pub struct KeyStats {
    pub keys_count: u32,
    pub keyrings_count: u32,
    pub reads: u64,
    pub writes: u64,
}

impl KeyManager {
    pub const fn new() -> Self {
        Self {
            keys: Mutex::new(BTreeMap::new()),
            keyrings: Mutex::new(BTreeMap::new()),
            thread_keyrings: Mutex::new(BTreeMap::new()),
            process_keyrings: Mutex::new(BTreeMap::new()),
            session_keyrings: Mutex::new(BTreeMap::new()),
            user_keyrings: Mutex::new(BTreeMap::new()),
            next_serial: AtomicU64::new(1),
            stats: Mutex::new(KeyStats::default()),
        }
    }

    /// Anahtar oluşturur
    pub fn create_key(&self, key_type: &str, description: &str, payload: &[u8]) -> Arc<Key> {
        let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
        let key = Arc::new(Key::new(serial, key_type, description));

        if !payload.is_empty() {
            key.write(payload);
        }

        self.keys.lock().insert(serial, key.clone());

        let mut stats = self.stats.lock();
        stats.keys_count += 1;

        key
    }

    /// Anahtarlık oluşturur
    pub fn create_keyring(&self, description: &str) -> Arc<Keyring> {
        let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
        let keyring = Arc::new(Keyring::new(serial, description));

        self.keyrings.lock().insert(serial, keyring.clone());
        self.keys.lock().insert(serial, keyring.key.clone());

        let mut stats = self.stats.lock();
        stats.keyrings_count += 1;

        keyring
    }

    /// Seri numarasına göre anahtar getirir
    pub fn get_key(&self, serial: u64) -> Option<Arc<Key>> {
        self.keys.lock().get(&serial).cloned()
    }

    /// Seri numarasına göre anahtarlık getirir
    pub fn get_keyring(&self, serial: u64) -> Option<Arc<Keyring>> {
        self.keyrings.lock().get(&serial).cloned()
    }

    /// Özel anahtarlık getirir
    pub fn get_special_keyring(&self, spec: i32, pid: u32) -> Option<Arc<Keyring>> {
        let serial = match spec {
            KEY_SPEC_THREAD_KEYRING => self.thread_keyrings.lock().get(&pid).copied()?,
            KEY_SPEC_PROCESS_KEYRING => self.process_keyrings.lock().get(&pid).copied()?,
            KEY_SPEC_SESSION_KEYRING => self.session_keyrings.lock().get(&pid).copied()?,
            KEY_SPEC_USER_KEYRING => self.user_keyrings.lock().get(&pid).copied()?,
            _ => return None,
        };

        self.get_keyring(serial)
    }

    /// Anahtarlığa anahtar bağlar
    pub fn link_key(&self, keyring_serial: u64, key_serial: u64) -> Result<(), KeyError> {
        let keyring = self.get_keyring(keyring_serial).ok_or(KeyError::NotFound)?;
        let key = self.get_key(key_serial).ok_or(KeyError::NotFound)?;

        keyring.link(key);
        Ok(())
    }

    /// Anahtarlıktan anahtar bağlantısını kaldırır
    pub fn unlink_key(&self, keyring_serial: u64, key_serial: u64) -> Result<(), KeyError> {
        let keyring = self.get_keyring(keyring_serial).ok_or(KeyError::NotFound)?;
        keyring.unlink(key_serial);
        Ok(())
    }

    /// Anahtarlıkta anahtar arar
    pub fn search(&self, keyring_serial: u64, description: &str) -> Option<Arc<Key>> {
        let keyring = self.get_keyring(keyring_serial)?;
        keyring.search(description)
    }

    /// Anahtarı iptal eder
    pub fn revoke_key(&self, serial: u64) -> Result<(), KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.revoke();
        Ok(())
    }

    /// Anahtarı günceller
    pub fn update_key(&self, serial: u64, payload: &[u8]) -> Result<(), KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.update(payload)
    }

    /// Anahtarı okur
    pub fn read_key(&self, serial: u64) -> Result<Vec<u8>, KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.read().ok_or(KeyError::Revoked)
    }

    /// İstatistikleri getirir
    pub fn get_stats(&self) -> KeyStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref KEY_MANAGER: KeyManager = KeyManager::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    NotFound,
    Revoked,
    PermissionDenied,
    QuotaExceeded,
    InvalidType,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

pub fn sys_add_key(key_type: &str, description: &str, payload: &[u8], keyring_serial: u64) -> i64 {
    let key = KEY_MANAGER.create_key(key_type, description, payload);
    let serial = key.get_serial();

    if keyring_serial != 0 {
        let _ = KEY_MANAGER.link_key(keyring_serial, serial);
    }

    serial as i64
}

pub fn sys_request_key(key_type: &str, description: &str, callout: &str, dest_keyring: u64) -> i64 {
    // Mevcut anahtarı ara
    if let Some(key) = KEY_MANAGER.search(dest_keyring, description) {
        return key.get_serial() as i64;
    }

    // Çağrı aracılığıyla yeni anahtar oluşturulur
    -2 // ENOENT
}

pub fn sys_keyctl(cmd: i32, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    match cmd {
        0 => { // KEYCTL_GET_KEYRING_ID
            if let Some(keyring) = KEY_MANAGER.get_special_keyring(arg2 as i32, arg3 as u32) {
                keyring.key.get_serial() as i64
            } else {
                -1
            }
        }
        1 => { // KEYCTL_JOIN_SESSION_KEYRING
            let keyring = KEY_MANAGER.create_keyring(&String::new());
            keyring.key.get_serial() as i64
        }
        2 => { // KEYCTL_UPDATE
            let _ = KEY_MANAGER.update_key(arg2, unsafe {
                core::slice::from_raw_parts(arg3 as *const u8, arg4 as usize)
            });
            0
        }
        3 => { // KEYCTL_REVOKE
            let _ = KEY_MANAGER.revoke_key(arg2);
            0
        }
        4 => { // KEYCTL_CHOWN
            // Sahipliği değiştir
            0
        }
        5 => { // KEYCTL_SETPERM
            // İzinleri ayarla
            0
        }
        6 => { // KEYCTL_DESCRIBE
            // Anahtarı açıkla
            0
        }
        7 => { // KEYCTL_CLEAR
            // Anahtarlığı temizle
            0
        }
        8 => { // KEYCTL_LINK
            let _ = KEY_MANAGER.link_key(arg2, arg3);
            0
        }
        9 => { // KEYCTL_UNLINK
            let _ = KEY_MANAGER.unlink_key(arg2, arg3);
            0
        }
        10 => { // KEYCTL_SEARCH
            if let Some(key) = KEY_MANAGER.search(arg2, unsafe {
                core::str::from_utf8(core::slice::from_raw_parts(arg3 as *const u8, arg4 as usize))
                    .unwrap_or("")
            }) {
                key.get_serial() as i64
            } else {
                -2
            }
        }
        11 => { // KEYCTL_READ
            match KEY_MANAGER.read_key(arg2) {
                Ok(data) => data.len() as i64,
                Err(_) => -1,
            }
        }
        _ => -22
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    // Varsayılan kullanıcı anahtarlığını oluştur
    let user_keyring = KEY_MANAGER.create_keyring("_uid.0");
    KEY_MANAGER.user_keyrings.lock().insert(0, user_keyring.key.get_serial());

    crate::serial_println!("[KEYRING] Subsystem initialized");
}
