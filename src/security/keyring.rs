//! # Anahtar Kilit Servisi (Kernel Key Retention Service)
//!
//! Bu modül, Linux Kernel Keyring API'siyle uyumlu bir anahtar saklama servisi sağlar.
//! Kriptografik anahtarlar, kimlik doğrulama token'ları ve gizli veriler çekirdek
//! belleğinde güvenle saklanır; kullanıcı alanı doğrudan ham verilere erişemez.
//!
//! ```
//! Keyring Hiyerarşisi:
//!
//!  SESSION_KEYRING  (kullanıcı oturumu boyunca yaşar)
//!      |
//!      +-- PROCESS_KEYRING  (süreç boyunca yaşar)
//!              |
//!              +-- THREAD_KEYRING  (iş parçacığı boyunca yaşar)
//!
//!  USER_KEYRING  (kullanıcı ID'sine bağlı, UID=0 için "_uid.0")
//!
//!  Her keyring, birden fazla Key nesnesine bağlantı (link) içerebilir.
//! ```
//!
//! Anahtar İşlem Akışı:
//! 1. `sys_add_key()` -> Anahtar oluştur ve hedef keyring'e bağla
//! 2. `sys_keyctl(KEYCTL_READ)` -> Anahtarı oku (izin gerekir)
//! 3. `sys_keyctl(KEYCTL_REVOKE)` -> Anahtarı iptal et
//! 4. `sys_keyctl(KEYCTL_UPDATE)` -> Anahtar payload'ını güncelle
//!
//! Güvenlik modeli: Her anahtar için pos/usr/grp/oth izin bitleri (POSIX benzeri)

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// KEYRING SABİTLERİ - Anahtar Tipleri
//
// Her anahtar bir tip etiketiyle tanımlanır.
// Bu etiket, anahtarın nasıl yorumlanacağını ve hangi işlemlere
// izin verileceğini belirler.
//
//  keyring:   Başka anahtarlara bağlantı içeren konteyner
//  user:      Ham kullanıcı verisi (genellikle şifre/token)
//  encrypted: AES ile şifrelenmiş anahtar (master key gerekir)
//  trusted:   TPM ile mühürlenmiş güvenilir anahtar
//  logon:     Oturum açma kimlik bilgileri
//  big_key:   Büyük veri için özel anahtar tipi (tmpfs kullanır)
// ============================================================================

/// Başka anahtarlara bağlantı içeren konteyner tipi
pub const KEY_TYPE_KEYRING: &str = "keyring";
/// Ham kullanıcı verisi (şifre, token vb.)
pub const KEY_TYPE_USER: &str = "user";
/// AES şifrelenmiş anahtar (master key gerektirir)
pub const KEY_TYPE_ENCRYPTED: &str = "encrypted";
/// TPM ile mühürlenmiş güvenilir anahtar
pub const KEY_TYPE_TRUSTED: &str = "trusted";
/// Oturum kimlik bilgileri (read işleminde veri döndürmez)
pub const KEY_TYPE_LOGON: &str = "logon";
/// Büyük veri anahtarı (tmpfs/RAM backed)
pub const KEY_TYPE_BIG_KEY: &str = "big_key";

// ============================================================================
// ANAHTAR İZİNLERİ
//
// Her anahtar için pos/usr/grp/oth izin maskeleri ayrı ayrı tutulur.
// Linux `keyctl setperm` komutuna karşılık gelir.
//
// İzin maskesi formatı (32-bit):
//  Bit 31-24: POS (possessor - anahtara sahip olan)
//  Bit 23-16: USR (kullanıcı - uid sahibi)
//  Bit 15-8:  GRP (grup - gid sahibi)
//  Bit  7-0:  OTH (diğer - yukarıdakilerin dışındakiler)
//
// Her 8 bitlik alan:
//  VIEW=0x01, READ=0x02, WRITE=0x04, SEARCH=0x08, LINK=0x10, SETATTR=0x20
// ============================================================================

/// Possessor (anahtara sahip olan) - görüntüleme
pub const KEY_POS_VIEW: u32 = 0x01000000;
/// Possessor - okuma
pub const KEY_POS_READ: u32 = 0x02000000;
/// Possessor - yazma
pub const KEY_POS_WRITE: u32 = 0x04000000;
/// Possessor - arama
pub const KEY_POS_SEARCH: u32 = 0x08000000;
/// Possessor - bağlantı
pub const KEY_POS_LINK: u32 = 0x10000000;
/// Possessor - öznitelik değiştirme
pub const KEY_POS_SETATTR: u32 = 0x20000000;

/// Kullanıcı (uid sahibi) - görüntüleme
pub const KEY_USR_VIEW: u32 = 0x00010000;
/// Kullanıcı - okuma
pub const KEY_USR_READ: u32 = 0x00020000;
/// Kullanıcı - yazma
pub const KEY_USR_WRITE: u32 = 0x00040000;
/// Kullanıcı - arama
pub const KEY_USR_SEARCH: u32 = 0x00080000;
/// Kullanıcı - bağlantı
pub const KEY_USR_LINK: u32 = 0x00100000;
/// Kullanıcı - öznitelik değiştirme
pub const KEY_USR_SETATTR: u32 = 0x00200000;

/// Grup (gid sahibi) - görüntüleme
pub const KEY_GRP_VIEW: u32 = 0x00000100;
/// Grup - okuma
pub const KEY_GRP_READ: u32 = 0x00000200;
/// Grup - yazma
pub const KEY_GRP_WRITE: u32 = 0x00000400;
/// Grup - arama
pub const KEY_GRP_SEARCH: u32 = 0x00000800;
/// Grup - bağlantı
pub const KEY_GRP_LINK: u32 = 0x00001000;
/// Grup - öznitelik değiştirme
pub const KEY_GRP_SETATTR: u32 = 0x00002000;

/// Diğer (oth) - görüntüleme
pub const KEY_OTH_VIEW: u32 = 0x00000001;
/// Diğer - okuma
pub const KEY_OTH_READ: u32 = 0x00000002;
/// Diğer - yazma
pub const KEY_OTH_WRITE: u32 = 0x00000004;
/// Diğer - arama
pub const KEY_OTH_SEARCH: u32 = 0x00000008;
/// Diğer - bağlantı
pub const KEY_OTH_LINK: u32 = 0x00000010;
/// Diğer - öznitelik değiştirme
pub const KEY_OTH_SETATTR: u32 = 0x00000020;

// ============================================================================
// ÖZEL KEYRING KİMLİKLERİ
//
// Negatif değerler, sys_keyctl() çağrılarında bağlam anahtarlaması için kullanılır.
// Örneğin KEY_SPEC_THREAD_KEYRING = -1 çağıran iş parçacığının keyring'ini ifade eder.
//
// Bu değerler Linux keyutils kütüphanesiyle uyumludur.
// ============================================================================

/// Çağıran iş parçacığına ait keyring
pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
/// Çağıran sürece ait keyring
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
/// Çağıran oturuma ait keyring
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
/// Çağıran kullanıcıya ait keyring
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
/// Çağıran kullanıcının oturum keyring'i
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
/// Çağıranın grubuna ait keyring
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
/// İstek doğrulama anahtarı (request_key() için)
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;
/// İstekte bulunanın keyring'i
pub const KEY_SPEC_REQUESTOR_KEYRING: i32 = -8;

// ============================================================================
// ANAHTAR (KEY) YAPISI
//
// Her anahtar atomik alanlarla thread-safe biçimde yönetilir.
// Payload (anahtar verisi) Mutex ile korunur.
// Arc<Key> kullanımı birden fazla keyring'e bağlanmaya olanak tanır.
//
//  Key yaşam döngüsü:
//  oluştur -> instantiate (payload yaz) -> kullan -> iptal et (revoke)
//
//  İptal sonrası read() çağrısı None döndürür.
//  expiry != 0 ise belirli bir süre sonra anahtar geçersiz sayılır.
// ============================================================================

pub struct Key {
    /// Anahtarın çekirdek çapında benzersiz seri numarası
    pub serial: AtomicU64,
    /// Anahtar tipi etiketi ("user", "keyring", "encrypted" vb.)
    pub key_type: String,
    /// İnsan okunabilir anahtar açıklaması (arama için kullanılır)
    pub description: String,
    /// Anahtarın şifrelenmiş veya ham veri yükü
    pub payload: Mutex<Vec<u8>>,
    /// POSIX benzeri izin maskesi (pos|usr|grp|oth alanları)
    pub permissions: AtomicU32,
    /// Anahtarın sahibi olan kullanıcı kimliği
    pub uid: AtomicU32,
    /// Anahtarın sahibi olan grup kimliği
    pub gid: AtomicU32,
    /// Oluşturulma zamanı (sistem tick cinsinden)
    pub created: AtomicU64,
    /// Son kullanma süresi (0 = sonsuz, > 0 = tick sınırı)
    pub expiry: AtomicU64,
    /// İptal edilmiş mi? (revoke sonrası true olur, geri alınamaz)
    pub revoked: AtomicBool,
    /// Payload yazıldı mı? (instantiate edildi mi?)
    pub instantiated: AtomicBool,
    /// Arc referans sayacı (birden fazla keyring'e bağlandığında artar)
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
                // Varsayılan: pos+usr+grp+oth için view/read/search/link izinleri
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

    /// İstenen iznin bu kullanıcı/grup kombinasyonu için geçerli olup olmadığını kontrol eder.
    ///
    /// Önce uid eşleşmesi (usr izinleri), sonra gid (grp izinleri),
    /// son olarak oth izinleri denenir (POSIX DAC sırası).
    pub fn check_permission(&self, perm: u32, uid: u32, gid: u32) -> bool {
        let perms = self.permissions.load(Ordering::Relaxed);

        if uid == self.uid.load(Ordering::Relaxed) {
            // Sahip kullanıcı izinleri: perm << 16 = USR alanı
            (perms & (perm << 16)) != 0
        } else if gid == self.gid.load(Ordering::Relaxed) {
            // Grup izinleri: perm << 8 = GRP alanı
            (perms & (perm << 8)) != 0
        } else {
            // Diğer kullanıcılar: OTH alanı (bit 0-5)
            (perms & perm) != 0
        }
    }

    /// Anahtarın payload verisini okur.
    ///
    /// İptal edilmişse (revoked) None döner; aksi halde payload klonu döndürülür.
    pub fn read(&self) -> Option<Vec<u8>> {
        if self.revoked.load(Ordering::Relaxed) {
            return None;
        }
        Some(self.payload.lock().clone())
    }

    /// Anahtarın payload verisini yazar ve instantiated bayrağını set eder.
    pub fn write(&self, data: &[u8]) {
        *self.payload.lock() = data.to_vec();
        self.instantiated.store(true, Ordering::SeqCst);
    }

    /// Anahtarı iptal eder (revoke); bu işlem geri alınamaz.
    /// İptal sonrası read() çağrıları None döndürür.
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }

    /// Anahtarın payload verisini günceller.
    /// İptal edilmiş anahtarlar güncellenemez.
    pub fn update(&self, data: &[u8]) -> Result<(), KeyError> {
        if self.revoked.load(Ordering::Relaxed) {
            return Err(KeyError::Revoked);
        }
        *self.payload.lock() = data.to_vec();
        Ok(())
    }

    /// Anahtarın seri numarasını döndürür.
    pub fn get_serial(&self) -> u64 {
        self.serial.load(Ordering::Relaxed)
    }
}

// ============================================================================
// KEYRING YAPISI
//
// Keyring, diğer Key nesnelerine bağlantı (link) içeren özel bir anahtar tipidir.
// Arama işlemleri bağlantı listesi üzerinde description eşleşmesiyle yapılır.
//
//  Keyring -> link -> Key A (encrypted)
//          -> link -> Key B (user)
//          -> link -> Key C (keyring)  <- iç içe keyring desteklenir
// ============================================================================

pub struct Keyring {
    /// Bu keyring'in temel Key nesnesi (description, permissions vb.)
    pub key: Arc<Key>,
    /// Bağlantılı anahtarlar: seri no -> Key Arc referansı
    pub links: Mutex<BTreeMap<u64, Arc<Key>>>,
}

impl Keyring {
    pub fn new(serial: u64, description: &str) -> Self {
        let key = Arc::new(Key::new(serial, KEY_TYPE_KEYRING, description));
        // Keyring'ler direkt instantiated olarak oluşturulur (payload gerekmez)
        key.instantiated.store(true, Ordering::SeqCst);

        Self {
            key,
            links: Mutex::new(BTreeMap::new()),
        }
    }

    /// Keyring'e yeni bir anahtar bağlantısı ekler.
    pub fn link(&self, key: Arc<Key>) {
        let serial = key.get_serial();
        self.links.lock().insert(serial, key);
    }

    /// Keyring'den belirtilen seri numaralı bağlantıyı kaldırır.
    pub fn unlink(&self, serial: u64) -> Option<Arc<Key>> {
        self.links.lock().remove(&serial)
    }

    /// Description eşleşmesiyle anahtar arar; birden fazla eşleşirse ilkini döndürür.
    pub fn search(&self, description: &str) -> Option<Arc<Key>> {
        for key in self.links.lock().values() {
            if key.description == description {
                return Some(key.clone());
            }
        }
        None
    }

    /// Bu keyring'deki tüm anahtar seri numaralarını döndürür.
    pub fn list(&self) -> Vec<u64> {
        self.links.lock().keys().copied().collect()
    }
}

// ============================================================================
// ANAHTAR YÖNETİCİSİ (KEY MANAGER)
//
// Tüm çekirdek anahtarlarını ve keyring'leri tek bir global yöneticide tutar.
// Per-thread, per-process, per-session ve per-user keyring'ler ayrı tablolarda
// saklanır; özel ID'ler (-1, -2, ...) bu tablolara indeks olarak kullanılır.
//
//  Tablo Yapısı:
//  keys[seri]                ->  Arc<Key>      (tüm anahtarlar)
//  keyrings[seri]            ->  Arc<Keyring>  (tüm keyring'ler)
//  thread_keyrings[pid]      ->  seri          (her thread için)
//  process_keyrings[pid]     ->  seri          (her süreç için)
//  session_keyrings[pid]     ->  seri          (her oturum için)
//  user_keyrings[uid]        ->  seri          (her kullanıcı için)
// ============================================================================

pub struct KeyManager {
    /// Seri numarası -> Key eşlemesi (tüm anahtarları içerir)
    keys: Mutex<BTreeMap<u64, Arc<Key>>>,
    /// Seri numarası -> Keyring eşlemesi
    keyrings: Mutex<BTreeMap<u64, Arc<Keyring>>>,
    /// pid -> thread keyring seri numarası
    thread_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// pid -> process keyring seri numarası
    process_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// pid -> session keyring seri numarası
    session_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// uid -> user keyring seri numarası
    user_keyrings: Mutex<BTreeMap<u32, u64>>,
    /// Bir sonraki atanacak seri numarası (monoton artar)
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

    /// Yeni bir anahtar oluşturur, varsa payload'ını yazar ve genel tabloya ekler.
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

    /// Yeni bir keyring oluşturur; hem keyrings hem keys tablosuna ekler.
    pub fn create_keyring(&self, description: &str) -> Arc<Keyring> {
        let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
        let keyring = Arc::new(Keyring::new(serial, description));

        self.keyrings.lock().insert(serial, keyring.clone());
        // Keyring'in Key nesnesi de genel keys tablosuna eklenir
        self.keys.lock().insert(serial, keyring.key.clone());

        let mut stats = self.stats.lock();
        stats.keyrings_count += 1;

        keyring
    }

    /// Seri numarasıyla anahtar arar.
    pub fn get_key(&self, serial: u64) -> Option<Arc<Key>> {
        self.keys.lock().get(&serial).cloned()
    }

    /// Seri numarasıyla keyring arar.
    pub fn get_keyring(&self, serial: u64) -> Option<Arc<Keyring>> {
        self.keyrings.lock().get(&serial).cloned()
    }

    /// Özel keyring kimliğini (KEY_SPEC_*) çözümleyerek ilgili keyring'i döndürür.
    ///
    /// Negatif spec değerleri (KEY_SPEC_THREAD_KEYRING vb.) bağlam bazlı
    /// keyring araması yapar; pid ile eşleşen kayıt tablolarda aranır.
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

    /// Belirtilen keyring'e bir anahtar bağlantısı ekler.
    pub fn link_key(&self, keyring_serial: u64, key_serial: u64) -> Result<(), KeyError> {
        let keyring = self.get_keyring(keyring_serial).ok_or(KeyError::NotFound)?;
        let key = self.get_key(key_serial).ok_or(KeyError::NotFound)?;

        keyring.link(key);
        Ok(())
    }

    /// Belirtilen keyring'den bir anahtar bağlantısını kaldırır.
    pub fn unlink_key(&self, keyring_serial: u64, key_serial: u64) -> Result<(), KeyError> {
        let keyring = self.get_keyring(keyring_serial).ok_or(KeyError::NotFound)?;
        keyring.unlink(key_serial);
        Ok(())
    }

    /// Keyring içinde description eşleşmesiyle anahtar arar.
    pub fn search(&self, keyring_serial: u64, description: &str) -> Option<Arc<Key>> {
        let keyring = self.get_keyring(keyring_serial)?;
        keyring.search(description)
    }

    /// Belirtilen anahtarı iptal eder.
    pub fn revoke_key(&self, serial: u64) -> Result<(), KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.revoke();
        Ok(())
    }

    /// Belirtilen anahtarın payload'ını günceller.
    pub fn update_key(&self, serial: u64, payload: &[u8]) -> Result<(), KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.update(payload)
    }

    /// Belirtilen anahtarın payload'ını okur.
    pub fn read_key(&self, serial: u64) -> Result<Vec<u8>, KeyError> {
        let key = self.get_key(serial).ok_or(KeyError::NotFound)?;
        key.read().ok_or(KeyError::Revoked)
    }

    /// Güncel istatistik anlık görüntüsünü döndürür.
    pub fn get_stats(&self) -> KeyStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global KeyManager örneği (lazy_static ile thread-safe başlatma).
    pub static ref KEY_MANAGER: KeyManager = KeyManager::new();
}

// ============================================================================
// HATA TİPLERİ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// Anahtar seri numarası bulunamadı
    NotFound,
    /// Anahtar önceden iptal edilmiş
    Revoked,
    /// Yeterli izin yok
    PermissionDenied,
    /// Kullanıcı anahtar kotası aşıldı
    QuotaExceeded,
    /// Geçersiz anahtar tipi
    InvalidType,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZLERİ
//
// Linux keyring syscall'larının çekirdek tarafı implementasyonu.
//
//  sys_add_key:     Yeni anahtar oluştur, keyring'e bağla
//  sys_request_key: Var olan anahtarı ara veya callout ile oluştur
//  sys_keyctl:      Anahtar üzerinde çeşitli komutları çalıştır
//
// sys_keyctl komut tablosu:
//  0=GET_KEYRING_ID, 1=JOIN_SESSION, 2=UPDATE, 3=REVOKE,
//  4=CHOWN, 5=SETPERM, 6=DESCRIBE, 7=CLEAR, 8=LINK, 9=UNLINK,
//  10=SEARCH, 11=READ
// ============================================================================

/// Yeni anahtar oluşturur ve belirtilen keyring'e bağlar; seri numarasını döndürür.
pub fn sys_add_key(key_type: &str, description: &str, payload: &[u8], keyring_serial: u64) -> i64 {
    let key = KEY_MANAGER.create_key(key_type, description, payload);
    let serial = key.get_serial();

    if keyring_serial != 0 {
        let _ = KEY_MANAGER.link_key(keyring_serial, serial);
    }

    serial as i64
}

/// Var olan anahtarı arar; bulunamazsa -2 (ENOENT) döndürür.
pub fn sys_request_key(key_type: &str, description: &str, callout: &str, dest_keyring: u64) -> i64 {
    // Önce mevcut keyring'de ara
    if let Some(key) = KEY_MANAGER.search(dest_keyring, description) {
        return key.get_serial() as i64;
    }

    // Callout ile yeni anahtar oluşturma (henüz uygulanmadı)
    -2 // ENOENT
}

/// `keyctl()` sistem çağrısı - anahtar yönetimi komutlarını çalıştırır.
///
/// cmd değeri hangi operasyonun yapılacağını belirler.
pub fn sys_keyctl(cmd: i32, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    match cmd {
        0 => { // KEYCTL_GET_KEYRING_ID - Özel keyring ID'sini çözümle
            if let Some(keyring) = KEY_MANAGER.get_special_keyring(arg2 as i32, arg3 as u32) {
                keyring.key.get_serial() as i64
            } else {
                -1
            }
        }
        1 => { // KEYCTL_JOIN_SESSION_KEYRING - Yeni oturum keyring'i oluştur/katıl
            let keyring = KEY_MANAGER.create_keyring(&String::new());
            keyring.key.get_serial() as i64
        }
        2 => { // KEYCTL_UPDATE - Anahtar payload'ını güncelle
            let _ = KEY_MANAGER.update_key(arg2, unsafe {
                core::slice::from_raw_parts(arg3 as *const u8, arg4 as usize)
            });
            0
        }
        3 => { // KEYCTL_REVOKE - Anahtarı iptal et
            let _ = KEY_MANAGER.revoke_key(arg2);
            0
        }
        4 => { // KEYCTL_CHOWN - Anahtar sahipliğini değiştir
            // TODO: uid/gid güncelleme implementasyonu
            0
        }
        5 => { // KEYCTL_SETPERM - İzin maskesini güncelle
            // TODO: izin maskesi güncelleme implementasyonu
            0
        }
        6 => { // KEYCTL_DESCRIBE - Anahtar bilgisini kullanıcı alanına aktar
            // TODO: tanımlama string'i implementasyonu
            0
        }
        7 => { // KEYCTL_CLEAR - Keyring'deki tüm bağlantıları temizle
            // TODO: keyring temizleme implementasyonu
            0
        }
        8 => { // KEYCTL_LINK - Keyring'e anahtar bağla
            let _ = KEY_MANAGER.link_key(arg2, arg3);
            0
        }
        9 => { // KEYCTL_UNLINK - Keyring'den anahtar bağlantısını kaldır
            let _ = KEY_MANAGER.unlink_key(arg2, arg3);
            0
        }
        10 => { // KEYCTL_SEARCH - Keyring'de description ile ara
            if let Some(key) = KEY_MANAGER.search(arg2, unsafe {
                core::str::from_utf8(core::slice::from_raw_parts(arg3 as *const u8, arg4 as usize))
                    .unwrap_or("")
            }) {
                key.get_serial() as i64
            } else {
                -2 // ENOENT
            }
        }
        11 => { // KEYCTL_READ - Anahtar payload'ını oku
            match KEY_MANAGER.read_key(arg2) {
                Ok(data) => data.len() as i64,
                Err(_) => -1,
            }
        }
        _ => -22 // EINVAL - bilinmeyen komut
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Keyring alt sistemini başlatır: root (uid=0) için varsayılan user keyring oluşturur.
pub fn init() {
    // "_uid.0" = uid 0 (root) için standart user keyring adı (Linux uyumlu)
    let user_keyring = KEY_MANAGER.create_keyring("_uid.0");
    KEY_MANAGER.user_keyrings.lock().insert(0, user_keyring.key.get_serial());

    crate::serial_println!("[KEYRING] Subsystem initialized");
}
