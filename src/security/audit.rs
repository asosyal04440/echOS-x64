//! # Denetim (Audit) Alt Sistemi
//!
//! Bu modül, güvenlik olaylarını, sistem çağrılarını ve dosya erişimlerini
//! kayıt altına alan denetim altyapısını sağlar.
//!
//! Linux Audit sistemiyle uyumlu bir yapı izlenir:
//! - Her olay bir `AuditRecord` nesnesiyle temsil edilir
//! - Kayıtlar sıra numarası (serial) ve zaman damgasıyla etiketlenir
//! - Kural ve izleme (watch) listeleri sayesinde seçici kayıt yapılır
//!
//! ```
//! Denetim Akışı:
//!
//!  [Syscall / Dosya Erişimi]
//!          |
//!          v
//!  [Kural Eşleşmesi Kontrol]  <-- AuditRule listesi karşılaştırılır
//!          |
//!          v
//!  [AuditRecord Oluştur]       <-- Alan ve metadata eklenir
//!          |
//!          v
//!  [Log Tamponuna Yaz]         <-- Mutex korumalı Vec<AuditRecord>
//!          |
//!          v
//!  [get_records() ile Tüket]   <-- Kullanıcı alanı veya daemon okur
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// AUDIT SABİTLERİ
//
// Linux audit sürüm 2 ile uyumlu mesaj tipi sabitleri.
// Bu numaralar, kayıt tipini tanımlayan standart bir sözlük oluşturur.
// ============================================================================

/// Sistem çağrısı denetim kaydı (giriş + çıkış bilgisi)
pub const AUDIT_SYSCALL: u16 = 1300;
/// Dosya izleme (inotify benzeri, path bazlı)
pub const AUDIT_FS_WATCH: u16 = 1301;
/// Dosya yolu kaydı
pub const AUDIT_PATH: u16 = 1302;
/// Oturum açma/kapama olayı
pub const AUDIT_LOGIN: u16 = 1303;
/// Kullanıcı alanı denetim mesajı
pub const AUDIT_USER: u16 = 1304;
/// Çekirdek iç denetim mesajı
pub const AUDIT_KERNEL: u16 = 1305;
/// Anormal süreç sonlanması (sinyal/kilitlenme)
pub const AUDIT_ANOM_ABEND: u16 = 1701;
/// Anormal bağlantı (sembolik link saldırısı vb.)
pub const AUDIT_ANOM_LINK: u16 = 1702;
/// IMA bütünlük verisi
pub const AUDIT_INTEGRITY_DATA: u16 = 1800;
/// IMA bütünlük metadata'sı
pub const AUDIT_INTEGRITY_METADATA: u16 = 1801;
/// IMA bütünlük durumu
pub const AUDIT_INTEGRITY_STATUS: u16 = 1802;

// ============================================================================
// FİLTRE AKSİYONLARI
//
// Bir kural eşleştiğinde hangi işlemin yapılacağını belirler.
// NEVER: kayıt oluşturma, POSSIBLE: koşullu, ALWAYS: her zaman kaydet.
// ============================================================================

/// Bu kuralı hiçbir zaman kaydetme
pub const AUDIT_NEVER: u32 = 0;
/// Koşullara bağlı olarak kaydet
pub const AUDIT_POSSIBLE: u32 = 1;
/// Her zaman kaydet
pub const AUDIT_ALWAYS: u32 = 2;

// ============================================================================
// FİLTRE NOKTALARI
//
// Hangi yürütme noktasında kuralın değerlendirileceğini belirler.
// Linux'ta bu noktalar: user, task, entry (syscall giriş), watch, exit (çıkış).
// ============================================================================

/// Kullanıcı alanı mesajlarını filtrele
pub const AUDIT_FILTER_USER: u32 = 0;
/// Görev oluşturmada filtrele
pub const AUDIT_FILTER_TASK: u32 = 1;
/// Syscall girişinde filtrele
pub const AUDIT_FILTER_ENTRY: u32 = 2;
/// Dosya izleme olaylarında filtrele
pub const AUDIT_FILTER_WATCH: u32 = 3;
/// Syscall çıkışında filtrele
pub const AUDIT_FILTER_EXIT: u32 = 4;
/// Kural listesinin başına ekle (normal: sona)
pub const AUDIT_FILTER_PREPEND: u32 = 0x80000000;

// ============================================================================
// DENETİM KAYDI (AUDIT RECORD)
//
// Her güvenlik olayı bir AuditRecord olarak temsil edilir.
// Linux audit kaydıyla biçim uyruluğu hedeflenir:
//   "audit(zaman_sn.nanosn:oturum): type=... serial=... pid=... ..."
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditRecord {
    /// Kayıt türü (örn. AUDIT_SYSCALL = 1300)
    pub msg_type: u16,
    /// Sıra numarası - her kayıt artarak benzersiz olur
    pub serial: u32,
    /// Zaman damgası (nanosaniye cinsinden tick sayısı)
    pub timestamp: u64,
    /// Oturum kimliği
    pub session_id: u32,
    /// İşlem kimliği (PID)
    pub pid: u32,
    /// Gerçek kullanıcı kimliği (UID)
    pub uid: u32,
    /// Etkin kullanıcı kimliği (EUID)
    pub euid: u32,
    /// Gerçek grup kimliği (GID)
    pub gid: u32,
    /// Etkin grup kimliği (EGID)
    pub egid: u32,
    /// Sistem çağrısı numarası (-1 ise belirsiz)
    pub syscall: i32,
    /// Sistem çağrısı dönüş değeri
    pub ret: i64,
    /// Çalıştırılabilir dosya yolu
    pub exe: String,
    /// Komut adı (comm)
    pub comm: String,
    /// Ek anahtar-değer alanları
    pub fields: Vec<(String, String)>,
}

impl AuditRecord {
    pub fn new(msg_type: u16, serial: u32) -> Self {
        Self {
            msg_type,
            serial,
            timestamp: crate::task::scheduler::get_ticks(),
            session_id: 0,
            pid: 0,
            uid: 0,
            euid: 0,
            gid: 0,
            egid: 0,
            syscall: -1,
            ret: 0,
            exe: String::new(),
            comm: String::new(),
            fields: Vec::new(),
        }
    }

    /// Kayda yeni bir anahtar-değer alanı ekler.
    pub fn add_field(&mut self, name: &str, value: &str) {
        self.fields.push((String::from(name), String::from(value)));
    }

    /// Kaydı standart audit log formatına dönüştürür.
    ///
    /// Format: `audit(sn.nanosn:oturum): type=N serial=N pid=N ...`
    /// Bu format, auditd, ausearch ve auditctl araçlarıyla uyumludur.
    pub fn format(&self) -> String {
        let mut s = alloc::format!(
            "audit({}.{}:{}): type={} serial={} pid={} uid={} euid={} gid={} egid={} syscall={} ret={}",
            self.timestamp / 1_000_000_000,
            self.timestamp % 1_000_000_000,
            self.session_id,
            self.msg_type,
            self.serial,
            self.pid,
            self.uid,
            self.euid,
            self.gid,
            self.egid,
            self.syscall,
            self.ret
        );

        for (name, value) in &self.fields {
            s.push_str(&alloc::format!(" {}={}", name, value));
        }

        s.push_str(&alloc::format!(" exe=\"{}\" comm=\"{}\"", self.exe, self.comm));

        s
    }
}

// ============================================================================
// DENETİM KURALI (AUDIT RULE)
//
// Kural, hangi syscall'ların ve hangi koşullarda kayıt altına alınacağını
// tanımlar. Syscall maskesi bit dizisi olarak depolanır:
//
//   syscall_mask[0] -> syscall 0..63 için bitmask
//   syscall_mask[1] -> syscall 64..127 için bitmask
//
// Bit N set ise, syscall N bu kural kapsamındadır.
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditRule {
    /// Kural tanımlayıcısı
    pub id: u32,
    /// Filtreleme noktası (AUDIT_FILTER_*)
    pub filter: u32,
    /// Aksiyon (AUDIT_NEVER, AUDIT_POSSIBLE, AUDIT_ALWAYS)
    pub action: u32,
    /// 128 syscall için 2x64-bit bitmask (syscall_mask[n/64] & (1<<(n%64)))
    pub syscall_mask: [u64; 2],
    /// Alan sayısı
    pub field_count: u32,
    /// Ek karşılaştırma alanları
    pub fields: Vec<AuditRuleField>,
    /// Kuralın etkin olup olmadığı
    pub enabled: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct AuditRuleField {
    pub field: u32,
    pub op: u32,
    pub value: u32,
    pub value_str: String,
}

// ============================================================================
// DENETİM ALANI TİPLERİ
//
// Kuralda hangi alanın karşılaştırılacağını belirtir.
// Örneğin AUDIT_UID=1 ile yalnızca belirli kullanıcı ID'li işlemler izlenir.
// ============================================================================

/// İşlem kimliğine göre filtrele
pub const AUDIT_PID: u32 = 0;
/// Kullanıcı kimliğine göre filtrele
pub const AUDIT_UID: u32 = 1;
/// Grup kimliğine göre filtrele
pub const AUDIT_GID: u32 = 2;
/// Oturum açma UID'sine göre filtrele
pub const AUDIT_LOGINUID: u32 = 3;
/// Kişilik (personality) alanına göre filtrele
pub const AUDIT_PERS: u32 = 4;
/// Mimari (arch) alanına göre filtrele
pub const AUDIT_ARCH: u32 = 5;
/// Çıkış koduna göre filtrele
pub const AUDIT_EXIT: u32 = 6;
/// Başarı/hata durumuna göre filtrele
pub const AUDIT_SUCCESS: u32 = 7;
/// İzleme (watch) yoluna göre filtrele
pub const AUDIT_WATCH: u32 = 8;
/// İzin bitine göre filtrele (r/w/x)
pub const AUDIT_PERM: u32 = 9;
/// Dizin yoluna göre filtrele
pub const AUDIT_DIR: u32 = 10;
/// Dosya tipine göre filtrele
pub const AUDIT_FILETYPE: u32 = 11;
/// Syscall argüman 0'a göre filtrele
pub const AUDIT_ARG0: u32 = 12;
/// Çalıştırılabilir dosyaya göre filtrele
pub const AUDIT_EXE: u32 = 100;

impl AuditRule {
    pub fn new(id: u32, filter: u32, action: u32) -> Self {
        Self {
            id,
            filter,
            action,
            syscall_mask: [0; 2],
            field_count: 0,
            fields: Vec::new(),
            enabled: AtomicBool::new(true),
        }
    }

    /// Bu kuralın belirtilen syscall numarasını kapsayıp kapsamadığını kontrol eder.
    ///
    /// Bitmask: n < 64 ise syscall_mask[0], 64 <= n < 128 ise syscall_mask[1] kullanılır.
    pub fn matches_syscall(&self, syscall: i32) -> bool {
        if syscall < 0 || syscall >= 128 {
            return false;
        }

        let word = syscall as usize / 64;
        let bit = syscall as usize % 64;

        (self.syscall_mask[word] & (1 << bit)) != 0
    }

    /// Belirtilen syscall numarasını kural bitmask'ına ekler.
    pub fn add_syscall(&mut self, syscall: i32) {
        if syscall < 0 || syscall >= 128 {
            return;
        }

        let word = syscall as usize / 64;
        let bit = syscall as usize % 64;

        self.syscall_mask[word] |= 1 << bit;
    }

    /// Kurala yeni bir alan koşulu ekler.
    pub fn add_field(&mut self, field: AuditRuleField) {
        self.fields.push(field);
        self.field_count += 1;
    }
}

// ============================================================================
// DENETİM İZLEMESİ (AUDIT WATCH)
//
// Belirli bir dosya veya dizin yoluna yapılan erişimleri izler.
// Linux'ta `auditctl -w /etc/passwd -p rw -k auth` gibi kurallara karşılık gelir.
//
//  perms bitmask: bit2=r (okuma), bit1=w (yazma), bit0=x (çalıştırma)
// ============================================================================

#[derive(Clone, Debug)]
pub struct AuditWatch {
    /// İzleme kimliği
    pub id: u32,
    /// İzlenecek dosya/dizin yolu
    pub path: String,
    /// İzlenecek izin bitleri (r=4, w=2, x=1)
    pub perms: u32, // r=4, w=2, x=1
    /// İzleme anahtarı (kayıtta aranabilir etiket)
    pub key: String,
    /// Yol bir dizin mi? (sondaki '/' ile belirlenir)
    pub is_dir: bool,
}

impl AuditWatch {
    pub fn new(id: u32, path: &str, perms: u32, key: &str) -> Self {
        Self {
            id,
            path: String::from(path),
            perms,
            key: String::from(key),
            is_dir: path.ends_with('/'),
        }
    }

    /// Verilen yol ve izin kombinasyonunun bu izlemeyi tetikleyip tetiklemeyeceğini kontrol eder.
    pub fn matches(&self, path: &str, perms: u32) -> bool {
        path.starts_with(&self.path) && (perms & self.perms) != 0
    }
}

// ============================================================================
// DENETİM YÖNETİCİSİ (AUDIT MANAGER)
//
// Tüm denetim altyapısını tek bir veri yapısında toplar.
// Thread-safe: tüm mutable alanlar Mutex veya Atomic tipler ile korunur.
//
//  Bileşen haritası:
//  +---------------------------+
//  | AuditManager              |
//  |  - rules: Vec<AuditRule>  | <- Hangi syscall'lar kayıt altına alınır
//  |  - watches: Vec<Watch>    | <- Hangi dosya yolları izlenir
//  |  - log: Vec<AuditRecord>  | <- Biriktirilen kayıtlar
//  |  - stats: AuditStats      | <- Gönderilen/kaybedilen kayıt sayacı
//  +---------------------------+
// ============================================================================

pub struct AuditManager {
    /// Denetim etkin mi
    pub enabled: AtomicBool,
    /// Denetim kuralları listesi
    pub rules: Mutex<Vec<AuditRule>>,
    /// Dosya izleme listesi
    pub watches: Mutex<Vec<AuditWatch>>,
    /// Biriktirilen kayıt tamponu
    pub log: Mutex<Vec<AuditRecord>>,
    /// Bir sonraki sıra numarası (her kayıtta artırılır)
    pub next_serial: AtomicU32,
    /// Bir sonraki kural kimliği
    pub next_rule_id: AtomicU32,
    /// Bir sonraki izleme kimliği
    pub next_watch_id: AtomicU32,
    /// Hız sınırı: saniyede maksimum kayıt sayısı
    pub rate_limit: AtomicU32,
    /// Tampon sınırı: maksimum biriktirilebilir kayıt sayısı
    pub backlog_limit: AtomicU32,
    /// İstatistikler
    pub stats: Mutex<AuditStats>,
}

#[derive(Clone, Debug, Default)]
pub struct AuditStats {
    pub records_sent: u64,
    pub records_lost: u64,
    pub rules_count: u32,
    pub watches_count: u32,
}

impl AuditManager {
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            rules: Mutex::new(Vec::new()),
            watches: Mutex::new(Vec::new()),
            log: Mutex::new(Vec::new()),
            next_serial: AtomicU32::new(1),
            next_rule_id: AtomicU32::new(1),
            next_watch_id: AtomicU32::new(1),
            rate_limit: AtomicU32::new(1000),
            backlog_limit: AtomicU32::new(10000),
            stats: Mutex::new(AuditStats::default()),
        }
    }

    /// Denetim sistemini etkinleştirir.
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[AUDIT] Enabled");
    }

    /// Denetim sistemini devre dışı bırakır.
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Denetim etkin mi?
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Sistem çağrısını denetim kaydına ekler.
    ///
    /// Denetim kapalıysa hızlıca çıkar (erken dönüş = performans önceliği).
    /// Argümanlar `a0..a5` alanları olarak kayda eklenir.
    pub fn log_syscall(&self, syscall: i32, ret: i64, args: &[u64]) {
        if !self.is_enabled() {
            return;
        }

        let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
        let mut record = AuditRecord::new(AUDIT_SYSCALL, serial);
        record.syscall = syscall;
        record.ret = ret;

        // Argümanları a0, a1, ... a5 alanları olarak ekle
        for (i, arg) in args.iter().enumerate() {
            record.add_field(&alloc::format!("a{}", i), &arg.to_string());
        }

        self.log.lock().push(record);

        let mut stats = self.stats.lock();
        stats.records_sent += 1;
    }

    /// Dosya erişimini denetim kaydına ekler.
    ///
    /// Yalnızca eşleşen izleme (watch) kaydı varsa kayıt oluşturulur.
    /// Bu sayede tüm dosya erişimleri değil, sadece izlenenler loglanır.
    pub fn log_file_access(&self, path: &str, perms: u32, ret: i64) {
        if !self.is_enabled() {
            return;
        }

        // İzleme listesinde eşleşen kayıt ara
        for watch in self.watches.lock().iter() {
            if watch.matches(path, perms) {
                let serial = self.next_serial.fetch_add(1, Ordering::SeqCst);
                let mut record = AuditRecord::new(AUDIT_FS_WATCH, serial);
                record.add_field("path", path);
                record.add_field("perms", &perms.to_string());
                record.add_field("key", &watch.key);
                record.ret = ret;

                self.log.lock().push(record);

                let mut stats = self.stats.lock();
                stats.records_sent += 1;
            }
        }
    }

    /// Yeni bir kural ekler ve atanan kural kimliğini döndürür.
    pub fn add_rule(&self, mut rule: AuditRule) -> u32 {
        let id = self.next_rule_id.fetch_add(1, Ordering::SeqCst);
        rule.id = id;
        self.rules.lock().push(rule);

        let mut stats = self.stats.lock();
        stats.rules_count += 1;

        id
    }

    /// Belirtilen kimliğe sahip kuralı kaldırır.
    pub fn remove_rule(&self, id: u32) {
        self.rules.lock().retain(|r| r.id != id);

        let mut stats = self.stats.lock();
        stats.rules_count = stats.rules_count.saturating_sub(1);
    }

    /// Yeni bir dosya izleme kaydı ekler ve atanan kimliği döndürür.
    pub fn add_watch(&self, path: &str, perms: u32, key: &str) -> u32 {
        let id = self.next_watch_id.fetch_add(1, Ordering::SeqCst);
        let watch = AuditWatch::new(id, path, perms, key);
        self.watches.lock().push(watch);

        let mut stats = self.stats.lock();
        stats.watches_count += 1;

        id
    }

    /// Belirtilen kimliğe sahip izlemeyi kaldırır.
    pub fn remove_watch(&self, id: u32) {
        self.watches.lock().retain(|w| w.id != id);

        let mut stats = self.stats.lock();
        stats.watches_count = stats.watches_count.saturating_sub(1);
    }

    /// Biriktirilen tüm kayıtları tüketir ve döndürür (drain = tamponu boşaltır).
    pub fn get_records(&self) -> Vec<AuditRecord> {
        self.log.lock().drain(..).collect()
    }

    /// Güncel istatistik anlık görüntüsünü döndürür.
    pub fn get_stats(&self) -> AuditStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global AuditManager örneği (lazy_static ile thread-safe başlatma).
    pub static ref AUDIT: AuditManager = AuditManager::new();
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
//
// Kullanıcı alanından audit kaydı yazmak için sys_audit_write çağrılır.
// Bu, Linux'taki `write(audit_fd, msg, len)` mantığına karşılık gelir.
// ============================================================================

/// Kullanıcı alanından ya da çekirdekten doğrudan audit kaydı yazar.
pub fn sys_audit_write(msg_type: u16, data: &[u8]) -> i32 {
    if !AUDIT.is_enabled() {
        return 0;
    }

    let serial = AUDIT.next_serial.fetch_add(1, Ordering::SeqCst);
    let mut record = AuditRecord::new(msg_type, serial);
    record.add_field("data", &core::str::from_utf8(data).unwrap_or(""));

    AUDIT.log.lock().push(record);

    0
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Denetim alt sistemini başlatır (şimdilik yalnızca log mesajı).
pub fn init() {
    crate::serial_println!("[AUDIT] Subsystem initialized");
}
