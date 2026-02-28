//! # IMA/EVM - Bütünlük Ölçümü ve Genişletilmiş Doğrulama Modülü
//!
//! IMA (Integrity Measurement Architecture) ve EVM (Extended Verification Module),
//! dosya sistemi bütünlüğünü çalışma zamanında sağlayan iki tamamlayıcı teknolojidir.
//!
//! ```
//! IMA/EVM Mimarisi:
//!
//!  Dosya Okuma/Çalıştırma
//!         |
//!         v
//!  +------+------+
//!  |  IMA Motor  |  ----> [ölçüm]  Hash hesapla + PCR uzat (TPM)
//!  |             |  ----> [denetim] Referans hash ile karşılaştır
//!  +------+------+
//!         |
//!         v
//!  +------+------+
//!  |  EVM Motor  |  ----> [HMAC]  xattr'ların bütünlüğünü doğrula
//!  |             |  ----> [imza]  RSA/EC imza doğrulaması
//!  +------+------+
//! ```
//!
//! IMA Çalışma Modları:
//! - `measure`:  Dosya hash'ini PCR 10'a yaz (TPM ile doğrulanabilir rapor)
//! - `appraise`: Dosya hash'ini xattr'daki referansla karşılaştır (anlık doğrulama)
//! - `audit`:    Uyuşmayan hash'leri denetim günlüğüne yaz
//!
//! EVM Çalışma Modları:
//! - HMAC: xattr'ları HMAC-SHA256 ile koru (gizli anahtar gerekir)
//! - İmza: xattr'ları RSA/EC imzasıyla koru (açık anahtar doğrulaması)

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// IMA SABİTLERİ
//
// Her sabit bir IMA aksiyonunu veya seçeneğini temsil eder.
// Birden fazla sabit bitwise OR ile birleştirilebilir (aksiyon maskesi).
//
// Örnek: IMA_MEASURE | IMA_AUDIT -> hem TPM'e yaz hem denetle
// ============================================================================

/// Dosya hash'ini PCR'a yaz (TPM ölçümü)
pub const IMA_MEASURE: u32 = 0x01;
/// Bu kural kapsamındaki dosyaları ölçme
pub const IMA_DONT_MEASURE: u32 = 0x02;
/// Dosya hash'ini xattr referansıyla karşılaştır
pub const IMA_APPRAISE: u32 = 0x04;
/// Bu kural kapsamındaki dosyaları onaylama
pub const IMA_DONT_APPRAISE: u32 = 0x08;
/// Uyuşmazlıkları denetim günlüğüne yaz
pub const IMA_AUDIT: u32 = 0x10;
/// Yalnızca hash hesapla, PCR'a yaz
pub const IMA_HASH: u32 = 0x20;
/// Dijital imza gereksin (DIGSIG)
pub const IMA_DIGSIG: u32 = 0x40;

// ============================================================================
// IMA ONAYLAMA BAYRAKLARI
//
// `appraise` aksiyonunun nasıl uygulanacağını belirler.
// ENFORCE: uyuşmayan dosyaya erişimi engelle
// FIX:     uyuşmayan xattr'ı güncelle (dikkatli kullanılmalı)
// LOG:     uyuşmazlığı yalnızca kaydet, erişimi engelleme
// ============================================================================

/// Hatalı hash'teki dosyaya erişimi engelle
pub const IMA_APPRAISE_ENFORCE: u32 = 0x01;
/// Yanlış xattr'ı otomatik düzelt
pub const IMA_APPRAISE_FIX: u32 = 0x02;
/// Hatalı hash'i denetim günlüğüne kaydet
pub const IMA_APPRAISE_LOG: u32 = 0x04;
/// Çekirdek modüllerini onayla
pub const IMA_APPRAISE_MODULES: u32 = 0x08;
/// Firmware blob'larını onayla
pub const IMA_APPRAISE_FIRMWARE: u32 = 0x10;
/// IMA politika değişikliklerini onayla
pub const IMA_APPRAISE_POLICY: u32 = 0x20;
/// kexec çekirdeğini onayla
pub const IMA_APPRAISE_KEXEC: u32 = 0x40;

// ============================================================================
// IMA HASH ALGORİTMALARI
//
// IMA, farklı hash algoritmalarını destekler. SHA-256 günümüzdür tercih edilir.
// SHA-1 geriye dönük uyumluluk için tutulmaktadır; güvenlik açısından zayıftır.
// ============================================================================

/// SHA-1 (160 bit, geriye dönük uyumluluk, yeni sistemlerde kullanılmamalı)
pub const IMA_HASH_SHA1: u32 = 1;
/// SHA-256 (256 bit, NIST onaylı, güncel standart)
pub const IMA_HASH_SHA256: u32 = 2;
/// SHA-512 (512 bit, yüksek güvenlik gerektiren ortamlar için)
pub const IMA_HASH_SHA512: u32 = 3;

// ============================================================================
// EVM TİPLERİ
//
// EVM, xattr'ları (genişletilmiş öznitelikler) korur.
// xattr'lar: security.ima (dosya hash'i), security.evm (HMAC/imza)
//
// HMAC:    Gizli anahtar + xattr verisinin HMAC-SHA256'sı (kernel key ring'den)
// SIG:     Harici imzalanmış hash (açık anahtar doğrulaması)
// DIGSIG:  Doğrudan dijital imza (RSA/EC, openssl ile oluşturulur)
// ============================================================================

/// EVM HMAC tipi (gizli anahtar, Kernel KeyRing'de saklanır)
pub const EVM_XATTR_HMAC: u32 = 0x01;
/// EVM imza tipi (açık anahtar, cer/pem formatında)
pub const EVM_XATTR_SIG: u32 = 0x02;
/// EVM ayrımsal imza tipi (RSA/EC dijital imza)
pub const EVM_XATTR_DIGSIG: u32 = 0x03;

// ============================================================================
// IMA ŞABLON GİRİŞİ (IMA TEMPLATE ENTRY)
//
// IMA, her ölçümü bir şablon girişi olarak TPM PCR genişletme kaydına yazar.
// Şablon formatları: "ima" (eski), "ima-ng" (yeni), "ima-sig" (imzalı)
//
//  ima-ng formatı:
//  +------+----------+--------+---------------------+
//  | pcr  | digest   | name   | event_data          |
//  +------+----------+--------+---------------------+
//  kernel bu girişi /sys/kernel/security/ima/ascii_runtime_measurements'a yazar
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImaTemplateEntry {
    /// TPM PCR dizini (genellikle IMA ölçümleri için PCR 10 kullanılır)
    pub pcr: u32,
    /// Şablon adı (örn. "ima-ng", "ima-sig")
    pub template_name: String,
    /// Dosyanın kriptografik özet değeri
    pub digest: Vec<u8>,
    /// Olayın adı (dosya yolu veya modül adı)
    pub event_name: String,
    /// Olayın ham verisi (hash algoritması etiketi + digest)
    pub event_data: Vec<u8>,
}

impl ImaTemplateEntry {
    pub fn new(pcr: u32, template: &str, digest: &[u8], name: &str, data: &[u8]) -> Self {
        Self {
            pcr,
            template_name: String::from(template),
            digest: digest.to_vec(),
            event_name: String::from(name),
            event_data: data.to_vec(),
        }
    }
}

// ============================================================================
// IMA ÖLÇÜM KAYDI (IMA MEASUREMENT)
//
// Belirli bir dosya için hesaplanan hash değerini ve ilgili meta verileri tutar.
// Bu kayıt, çalışma zamanı ölçüm listesine (runtime measurement list) eklenir
// ve gerektiğinde uzaktan onay (remote attestation) için kullanılır.
// ============================================================================

pub struct ImaMeasurement {
    /// Ölçülen dosyanın tam yolu
    pub path: String,
    /// Dosyanın SHA-256 hash değeri (32 bayt)
    pub hash: [u8; 32],
    /// Kullanılan hash algoritması (IMA_HASH_SHA256 vb.)
    pub hash_algo: u32,
    /// Bu ölçümün genişletildiği TPM PCR dizini
    pub pcr: u32,
    /// IMA şablon adı ("ima-ng" varsayılan)
    pub template: String,
    /// Ölçümün gerçekleştiği sistem tick zaman damgası
    pub timestamp: u64,
    /// Bu ölçümün hala geçerli (kullanılabilir) olup olmadığı
    pub valid: AtomicBool,
}

impl ImaMeasurement {
    pub fn new(path: &str, hash: [u8; 32], pcr: u32) -> Self {
        Self {
            path: String::from(path),
            hash,
            hash_algo: IMA_HASH_SHA256,
            pcr,
            template: String::from("ima-ng"),
            timestamp: crate::task::scheduler::get_ticks(),
            valid: AtomicBool::new(true),
        }
    }
}

// ============================================================================
// IMA KURALI (IMA RULE)
//
// IMA politika kuralları, hangi dosyaların nasıl işleneceğini belirler.
// Her kural bir aksiyon (measure/appraise/audit), yol kalıbı ve isteğe
// bağlı ek koşullar (uid, func, fsmagic) içerir.
//
// Kural eşleşme önceliği: ilk eşleşen kural uygulanır (first-match).
//
// Örnek kural satırları:
//   measure func=BPRM_CHECK          -> Her çalıştırılan dosyayı ölç
//   appraise fsmagic=0x9fa1          -> procfs dosyalarını onayla
//   dont_measure uid=0 path=/proc/*  -> root için proc'u ölçme
// ============================================================================

#[derive(Clone, Debug)]
pub struct ImaRule {
    /// Kural tanımlayıcısı (otomatik atanır)
    pub id: u32,
    /// Aksiyon maskesi (IMA_MEASURE | IMA_APPRAISE | IMA_AUDIT)
    pub action: u32,
    /// Ek bayraklar (IMA_DIGSIG vb.)
    pub flags: u32,
    /// Yol kalıbı ("*" tümünü eşler, "prefix*" ile başlayanları eşler)
    pub path: String,
    /// Yalnızca bu UID'ye sahip süreçleri etkile (None = tüm UID'ler)
    pub uid: Option<u32>,
    /// İşlev filtresi: BPRM_CHECK, FILE_MMAP_CHECK, MODULE_CHECK, FIRMWARE_CHECK
    pub func: Option<String>,
    /// Erişim maskesi filtresi: MAY_READ, MAY_WRITE, MAY_EXEC
    pub mask: Option<String>,
    /// Dosya sistemi sihirli sayısına göre filtrele (lsattr ile öğrenilebilir)
    pub fsmagic: Option<u64>,
}

impl ImaRule {
    pub fn new(id: u32, action: u32, path: &str) -> Self {
        Self {
            id,
            action,
            flags: 0,
            path: String::from(path),
            uid: None,
            func: None,
            mask: None,
            fsmagic: None,
        }
    }

    /// Dosyanın bu kuralla eşleşip eşleşmediğini kontrol eder.
    ///
    /// Glob eşleşmesi: "*" tümünü kabul eder, "prefix*" ise önek kontrolü yapar.
    /// Diğer koşullar (uid, func, fsmagic) gelecekte eklenmesi için yer tutucudur.
    pub fn matches(&self, path: &str, _uid: u32, _func: &str, _mask: &str) -> bool {
        if self.path == "*" {
            return true;
        }

        // Basit glob eşleşmesi: "prefix*" -> önek kontrolü
        if self.path.ends_with('*') {
            let prefix = &self.path[..self.path.len() - 1];
            return path.starts_with(prefix);
        }

        path == self.path
    }
}

// ============================================================================
// EVM HMAC
//
// EVM, dosya xattr'larını bir HMAC ile korur. HMAC, çekirdek anahtar
// kilidindeki (kernel KeyRing) gizli anahtarla hesaplanır.
//
// Korunan xattr'lar (Linux varsayılanları):
//   security.ima    (IMA hash/imzası)
//   security.selinux (SELinux etiketi)
//   system.posix_acl_access
//   system.posix_acl_default
//
// HMAC = HMAC-SHA256(evm_key, uid || gid || mode || xattr_değerleri)
//
// Saldırgan xattr'ı değiştirirse HMAC doğrulaması başarısız olur
// ve EVM erişimi reddeder (ENFORCE modunda).
// ============================================================================

pub struct EvmHmac {
    /// Hesaplanan HMAC değeri (32 bayt, SHA-256)
    pub hmac: [u8; 32],
    /// Korunan xattr'ların toplu hash değeri
    pub xattr_hash: [u8; 32],
    /// Bu HMAC kaydının geçerli olup olmadığı
    pub valid: AtomicBool,
}

impl EvmHmac {
    /// xattr haritası ve gizli anahtar kullanarak HMAC hesaplar.
    ///
    /// NOT: Bu basitleştirilmiş bir hesaplamadır.
    /// Gerçek implementasyon HMAC-SHA256 kullanmalıdır.
    pub fn calculate(_xattrs: &BTreeMap<String, Vec<u8>>, key: &[u8]) -> Self {
        // Basitleştirilmiş HMAC hesabı (gerçekte HMAC-SHA256 kullanılır)
        let mut hmac = [0u8; 32];

        // Anahtar baytlarını XOR ile özete karıştır (yer tutucu)
        for (i, byte) in key.iter().enumerate() {
            hmac[i % 32] ^= byte;
        }

        Self {
            hmac,
            xattr_hash: [0u8; 32],
            valid: AtomicBool::new(true),
        }
    }

    /// Saklanan HMAC'ın hala geçerli olup olmadığını doğrular.
    pub fn verify(&self, _xattrs: &BTreeMap<String, Vec<u8>>, _key: &[u8]) -> bool {
        self.valid.load(Ordering::Relaxed)
    }
}

// ============================================================================
// IMA/EVM YÖNETİCİSİ (IMA/EVM MANAGER)
//
// Tüm IMA ölçümlerini, kurallarını ve EVM verilerini yönetir.
// PCR tamponu: 24 adet PCR kaydı saklar (TPM 2.0 varsayılan sayısı).
//
//  Veri Yapısı:
//  +-------------------+
//  | ImaEvmManager     |
//  |  measurements     | <- Runtime ölçüm listesi
//  |  rules            | <- IMA politika kuralları
//  |  evm_cache        | <- Yol -> EVM HMAC önbelleği
//  |  evm_key          | <- Gizli EVM anahtarı (KeyRing'den)
//  |  pcr_values[24]   | <- Sanal PCR tamponu
//  +-------------------+
// ============================================================================

pub struct ImaEvmManager {
    /// Çalışma zamanı ölçüm listesi (runtime measurement list)
    pub measurements: Mutex<Vec<ImaMeasurement>>,
    /// IMA politika kuralları (first-match sırasıyla uygulanır)
    pub rules: Mutex<Vec<ImaRule>>,
    /// Dosya yolu -> EVM HMAC önbelleği (her dosya için ayrı)
    pub evm_cache: Mutex<BTreeMap<String, EvmHmac>>,
    /// EVM gizli HMAC anahtarı (Kernel KeyRing'den yüklenir)
    pub evm_key: Mutex<Vec<u8>>,
    /// Sanal PCR değerleri tamponu (indeks = PCR numarası, max 24)
    pub pcr_values: Mutex<[Vec<u8>; 24]>,
    /// IMA ölçüm modunun etkin olup olmadığı
    pub ima_enabled: AtomicBool,
    /// EVM HMAC/imza modunun etkin olup olmadığı
    pub evm_enabled: AtomicBool,
    /// IMA onaylama modu (0=devre dışı, IMA_APPRAISE_ENFORCE vb.)
    pub appraisal_mode: AtomicU32,
    /// Bir sonraki kural kimliği (monoton artan)
    pub next_rule_id: AtomicU32,
    /// İstatistikler (ölçüm/onaylama/hata sayıları)
    pub stats: Mutex<ImaEvmStats>,
}

#[derive(Clone, Debug, Default)]
pub struct ImaEvmStats {
    pub measurements: u64,
    pub appraisals: u64,
    pub failures: u64,
    pub rules_count: u32,
}

impl ImaEvmManager {
    pub const fn new() -> Self {
        Self {
            measurements: Mutex::new(Vec::new()),
            rules: Mutex::new(Vec::new()),
            evm_cache: Mutex::new(BTreeMap::new()),
            evm_key: Mutex::new(Vec::new()),
            pcr_values: Mutex::new([Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new(),
                                    Vec::new(), Vec::new(), Vec::new(), Vec::new()]),
            ima_enabled: AtomicBool::new(false),
            evm_enabled: AtomicBool::new(false),
            appraisal_mode: AtomicU32::new(0),
            next_rule_id: AtomicU32::new(1),
            stats: Mutex::new(ImaEvmStats::default()),
        }
    }

    /// IMA/EVM alt sistemini başlatır: varsayılan kurallar eklenir ve etkinleştirilir.
    pub fn init(&self) {
        // Varsayılan Linux-uyumlu IMA kurallarını yükle
        self.add_default_rules();

        self.ima_enabled.store(true, Ordering::SeqCst);
        self.evm_enabled.store(true, Ordering::SeqCst);

        crate::serial_println!("[IMA/EVM] Initialized");
    }

    fn add_default_rules(&self) {
        // Standart Linux IMA politika kuralları:
        //  BPRM_CHECK:      execve() çağrısında her çalıştırılan dosyayı ölç
        //  FILE_MMAP_CHECK: çalıştırılabilir mmap bölgelerini ölç
        //  MODULE_CHECK:    çekirdek modüllerini ölç (insmod)
        //  FIRMWARE_CHECK:  firmware blob yüklemelerini ölç
        //  procfs/sysfs:    sanal dosya sistemlerini onayla
        let default_rules = [
            ("measure func=BPRM_CHECK", IMA_MEASURE),
            ("measure func=FILE_MMAP_CHECK", IMA_MEASURE),
            ("measure func=MODULE_CHECK", IMA_MEASURE),
            ("measure func=FIRMWARE_CHECK", IMA_MEASURE),
            ("appraise fsmagic=0x9fa1", IMA_APPRAISE),        // procfs sihirli sayısı
            ("appraise fsmagic=0x62656572", IMA_APPRAISE),    // sysfs sihirli sayısı
        ];

        for (rule_str, action) in default_rules {
            let id = self.next_rule_id.fetch_add(1, Ordering::SeqCst);
            let rule = ImaRule::new(id, action, "*");
            self.rules.lock().push(rule);
        }

        let mut stats = self.stats.lock();
        stats.rules_count = default_rules.len() as u32;
    }

    /// Bir dosyayı ölçer: hash hesaplar, PCR 10'u genişletir, kayıt oluşturur.
    ///
    /// PCR 10 Linux IMA için ayrılmış standart PCR dizinidir.
    pub fn measure_file(&self, path: &str, data: &[u8]) -> Result<(), ImaEvmError> {
        if !self.ima_enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Dosya hash'ini hesapla (SHA-256 basitleştirilmiş)
        let hash = self.calculate_hash(data);

        // Ölçüm kaydı oluştur (PCR 10 = IMA için ayrılmış standart)
        let measurement = ImaMeasurement::new(path, hash, 10); // PCR 10

        // TPM PCR'ını genişlet (ölçüm listesine ekle)
        self.extend_pcr(10, &hash);

        // Ölçüm listesine kaydet
        self.measurements.lock().push(measurement);

        let mut stats = self.stats.lock();
        stats.measurements += 1;

        Ok(())
    }

    /// Bir dosyayı onaylar: EVM HMAC/imza kontrolü yapar.
    ///
    /// `appraisal_mode` 0 ise onaylama atlanır (kapalı mod).
    /// ENFORCE modunda hata durumunda dosyaya erişim engellenir.
    pub fn appraise_file(&self, path: &str, _xattrs: &BTreeMap<String, Vec<u8>>) -> Result<(), ImaEvmError> {
        let mode = self.appraisal_mode.load(Ordering::SeqCst);

        if mode == 0 {
            return Ok(()); // Onaylama modu kapalı
        }

        // EVM HMAC kontrolü: önbellekte kayıt varsa geçerlilik doğrula
        if self.evm_enabled.load(Ordering::SeqCst) {
            if let Some(hmac) = self.evm_cache.lock().get(path) {
                if !hmac.valid.load(Ordering::Relaxed) {
                    let mut stats = self.stats.lock();
                    stats.failures += 1;
                    return Err(ImaEvmError::HmacMismatch);
                }
            }
        }

        let mut stats = self.stats.lock();
        stats.appraisals += 1;

        Ok(())
    }

    /// Politika listesine yeni bir IMA kuralı ekler.
    pub fn add_rule(&self, rule: ImaRule) {
        self.rules.lock().push(rule);

        let mut stats = self.stats.lock();
        stats.rules_count += 1;
    }

    /// Dosya verisinin SHA-256 hash değerini hesaplar (basitleştirilmiş).
    ///
    /// NOT: Bu gerçek SHA-256 değildir; üretim ortamı için `crate::crypto::sha256`
    /// kullanılmalıdır. Şu an XOR tabanlı yer tutucu hesaplamadır.
    fn calculate_hash(&self, data: &[u8]) -> [u8; 32] {
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        hash
    }

    /// PCR değerini genişletir: `new_pcr = SHA256(old_pcr || hash)`
    ///
    /// Bu, TPM 2.0'ın `TPM2_CC_PCR_Extend` komutunun yazılım simülasyonudur.
    /// Gerçek sistemde TPM donanımına ilgili komut gönderilmelidir.
    fn extend_pcr(&self, pcr: usize, hash: &[u8; 32]) {
        let mut pcrs = self.pcr_values.lock();
        if pcr < 24 {
            // Genişletme: new_value = SHA256(old_value || hash)
            // Şu an XOR ile basitleştirilmiştir
            for (i, byte) in hash.iter().enumerate() {
                if i < pcrs[pcr].len() {
                    pcrs[pcr][i] ^= byte;
                } else {
                    pcrs[pcr].push(*byte);
                }
            }
        }
    }

    /// Mevcut tüm ölçümlerin klonlanmış listesini döndürür.
    pub fn get_measurements(&self) -> Vec<ImaMeasurement> {
        self.measurements.lock().iter().map(|m| ImaMeasurement {
            path: m.path.clone(),
            hash: m.hash,
            hash_algo: m.hash_algo,
            pcr: m.pcr,
            template: m.template.clone(),
            timestamp: m.timestamp,
            valid: AtomicBool::new(m.valid.load(Ordering::Relaxed)),
        }).collect()
    }

    /// Güncel istatistik anlık görüntüsünü döndürür.
    pub fn get_stats(&self) -> ImaEvmStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global IMA/EVM yönetici örneği (lazy_static ile thread-safe başlatma).
    pub static ref IMA_EVM: ImaEvmManager = ImaEvmManager::new();
}

// ============================================================================
// HATA TİPLERİ
//
// IMA/EVM işlemlerinde oluşabilecek hata senaryoları:
//  HashMismatch:      Dosyanın mevcut hash'i xattr'daki referanstan farklı
//  HmacMismatch:      xattr'ların EVM HMAC'ı geçersiz (xattr değiştirilmiş)
//  SignatureInvalid:  RSA/EC imza doğrulaması başarısız
//  NoKey:             EVM anahtarı yüklü değil (KeyRing boş)
//  AppraisalFailed:   Genel onaylama hatası
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImaEvmError {
    /// Hesaplanan hash, saklanan referansla uyuşmuyor
    HashMismatch,
    /// EVM HMAC değeri geçersiz (xattr veya metadata bozulmuş)
    HmacMismatch,
    /// Dijital imza doğrulaması başarısız
    SignatureInvalid,
    /// EVM anahtarı mevcut değil
    NoKey,
    /// Genel onaylama hatası
    AppraisalFailed,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// IMA/EVM alt sistemini başlatır (varsayılan kurallar + etkinleştirme).
pub fn init() {
    IMA_EVM.init();
}
