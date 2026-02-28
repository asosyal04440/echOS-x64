//! # SELinux Benzeri Zorunlu Erişim Denetimi (Mandatory Access Control - MAC)
//!
//! Bu modül, politika tabanlı zorunlu erişim denetimi (MAC) sağlar.
//! SELinux'un uygulama mantığını model alarak süreçler ve kaynaklar üzerindeki
//! erişimleri çekirdek düzeyinde denetler.
//!
//! ```
//! Geleneksel DAC vs MAC Modeli:
//!
//!  DAC (İsteğe Bağlı Erişim - Discretionary):
//!   Dosya sahibi izinleri belirler -> saldırgan sahibi ele geçirirse tüm haklar ele geçer
//!
//!  MAC (Zorunlu Erişim - Mandatory):
//!   Politika merkezi olarak yönetilir -> sahip izin verse bile politika reddedebilir
//!
//!  MAC Karar Akışı:
//!
//!   süreç (kaynak tip: user_t)
//!        |
//!        v
//!   [MAC motoru]
//!        |---- TE Kuralları: user_t -> file_t FILE_READ izni var mı?
//!        |---- MLS  Kontrolü: kaynak seviyesi >= hedef seviyesi?
//!        |---- Rol   Guard:  geçiş kuralı var mı?
//!        |
//!        v
//!   ALLOW / DENY / AUDIT
//! ```
//!
//! MLS/MCS Güvenlik Seviyeleri:
//!   SystemLow (0) < Low (1) < Medium (2) < High (3) < Secret (4) < TopSecret (5) < SystemHigh (255)
//!
//! Bell-LaPadula Modeli:
//!   - "No read up": düşük seviyedeki süreç, yüksek seviyeli dosyayı OKUYAMAZ
//!   - "No write down": yüksek seviyedeki süreç, düşük seviyeli dosyaya YAZAMAZ

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

/// Güvenlik bağlamı (SELinux context'e karşılık gelir)
///
/// SELinux'ta bir sürecin veya nesnenin kimliğini tanımlayan 4 bileşenli yapı:
///   user:role:type:level
/// Örnek: system_u:system_r:kernel_t:s15:c0.c1023
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecurityContext {
    /// Güvenlik kullanıcısı (örn. "system_u", "user_u")
    pub user: String,
    /// Güvenlik rolü (örn. "system_r", "user_r")
    pub role: String,
    /// Güvenlik tipi - TE (Type Enforcement) için ana belirleyici (örn. "kernel_t", "user_t")
    pub type_: String,
    /// MLS/MCS güvenlik seviyesi - Bell-LaPadula gizlilik derecesi
    pub level: SecurityLevel,
}

impl SecurityContext {
    pub fn new(user: &str, role: &str, type_: &str, level: SecurityLevel) -> Self {
        SecurityContext {
            user: String::from(user),
            role: String::from(role),
            type_: String::from(type_),
            level,
        }
    }

    /// Çekirdek bağlamı oluşturur: sistem_u:system_r:kernel_t:SystemHigh
    pub fn system_u() -> Self {
        SecurityContext::new("system_u", "system_r", "kernel_t", SecurityLevel::SystemHigh)
    }

    /// Kullanıcı bağlamı oluşturur: user_u:user_r:user_t:Low
    pub fn user_u() -> Self {
        SecurityContext::new("user_u", "user_r", "user_t", SecurityLevel::Low)
    }

    /// Kısıtlanmamış bağlam: tüm kaynaklara erişebilen özel tip
    pub fn unconfined_u() -> Self {
        SecurityContext::new("unconfined_u", "unconfined_r", "unconfined_t", SecurityLevel::Low)
    }
}

/// MLS/MCS Güvenlik Seviyesi
///
/// MLS = Multi-Level Security (Çok Seviyeli Güvenlik)
/// MCS = Multi-Category Security (Çok Kategorili Güvenlik)
///
/// `sensitivity`: Gizlilik derecesi (0=kamu, 255=sistem_yüksek)
/// `categories`:  Kategori bitmaskesi (hangi proje/bölümlere erişim var)
///
/// dominates() metodu -> self.sensitivity >= other.sensitivity AND categories kapsama
/// Bu kontrol "no read up" (Bell-LaPadula okuma kuralı) uygulamasında kullanılır
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecurityLevel {
    pub sensitivity: u8,
    pub categories: u32,  // Bitmask for categories
}

impl SecurityLevel {
    pub const SystemHigh: Self = SecurityLevel { sensitivity: 255, categories: 0xFFFFFFFF };
    pub const SystemLow: Self = SecurityLevel { sensitivity: 0, categories: 0 };
    pub const Low: Self = SecurityLevel { sensitivity: 1, categories: 0 };
    pub const Medium: Self = SecurityLevel { sensitivity: 2, categories: 0 };
    pub const High: Self = SecurityLevel { sensitivity: 3, categories: 0 };
    pub const Secret: Self = SecurityLevel { sensitivity: 4, categories: 0 };
    pub const TopSecret: Self = SecurityLevel { sensitivity: 5, categories: 0 };

    /// Bell-LaPadula "dominates" ilişkisi:
    /// self >= other ise true döner (hem duyarlılık hem kategori kapsama)
    pub fn dominates(&self, other: &SecurityLevel) -> bool {
        self.sensitivity >= other.sensitivity && (self.categories & other.categories) == other.categories
    }
}

// ============================================================================
// ERİŞİM VEKTÖRLERİ - AccessVector
//
// SELinux'ta her nesne sınıfı için ayrı izin bitleri tanımlanır.
// Örnek: Dosya sınıfı FILE_READ | FILE_WRITE | FILE_EXECUTE ...
//        Süreç sınıfı PROCESS_FORK | PROCESS_SIGKILL | PROCESS_PTRACE ...
//
// AccessVector::ALL     -> tüm izinler açık (kernel_t gibi tam yetkili tipler için)
// AccessVector::NONE    -> hiçbir izin yok (varsayılan - reddet)
//
// union()     -> iki vektörün bit OR'u  (izinleri birleştir)
// intersect() -> iki vektörün bit AND'i (ortak izinleri bul)
// ============================================================================

/// Erişim vektörü (izin bitmaskesi)
///
/// Her nesne sınıfı için hangi işlemlere izin verileceğini belirleyen 32-bit maske.
/// Dosya, süreç ve soket için ayrı sabit kümeleri bulunur (aynı bit değerleri farklı anlam taşır).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessVector {
    pub permissions: u32,
}

impl AccessVector {
    // Dosya izinleri (ObjectClass::File için)
    pub const FILE_READ: u32 = 1 << 0;
    pub const FILE_WRITE: u32 = 1 << 1;
    pub const FILE_EXECUTE: u32 = 1 << 2;
    pub const FILE_APPEND: u32 = 1 << 3;
    pub const FILE_CREATE: u32 = 1 << 4;
    pub const FILE_DELETE: u32 = 1 << 5;
    pub const FILE_RENAME: u32 = 1 << 6;
    pub const FILE_LINK: u32 = 1 << 7;
    pub const FILE_CHMOD: u32 = 1 << 8;
    pub const FILE_CHOWN: u32 = 1 << 9;

    // Süreç izinleri (ObjectClass::Process için)
    pub const PROCESS_FORK: u32 = 1 << 0;
    pub const PROCESS_TRANSITION: u32 = 1 << 1;
    pub const PROCESS_SIGCHLD: u32 = 1 << 2;
    pub const PROCESS_SIGKILL: u32 = 1 << 3;
    pub const PROCESS_SIGSTOP: u32 = 1 << 4;
    pub const PROCESS_SIGINJECT: u32 = 1 << 5;
    pub const PROCESS_PTRACE: u32 = 1 << 6;  // Hata ayıklama ekleme yetkisi
    pub const PROCESS_EXECMEM: u32 = 1 << 7;
    pub const PROCESS_EXECSTACK: u32 = 1 << 8;
    pub const PROCESS_NOATSECURE: u32 = 1 << 9;

    // Soket izinleri (ObjectClass::Socket için)
    pub const SOCKET_READ: u32 = 1 << 0;
    pub const SOCKET_WRITE: u32 = 1 << 1;
    pub const SOCKET_CONNECT: u32 = 1 << 2;
    pub const SOCKET_BIND: u32 = 1 << 3;
    pub const SOCKET_LISTEN: u32 = 1 << 4;
    pub const SOCKET_ACCEPT: u32 = 1 << 5;

    pub const NONE: Self = AccessVector { permissions: 0 };
    pub const ALL: Self = AccessVector { permissions: 0xFFFFFFFF };

    pub fn new(permissions: u32) -> Self {
        AccessVector { permissions }
    }

    /// Belirtilen iznin bu vektörde set edilip edilmediğini kontrol eder.
    pub fn has(&self, perm: u32) -> bool {
        (self.permissions & perm) != 0
    }

    /// İki erişim vektörünü birleştirir (bit OR). İzinleri artırır.
    pub fn union(&self, other: &AccessVector) -> AccessVector {
        AccessVector::new(self.permissions | other.permissions)
    }

    /// İki erişim vektörünün kesişimini döndürür (bit AND). Ortak izinleri bulur.
    pub fn intersect(&self, other: &AccessVector) -> AccessVector {
        AccessVector::new(self.permissions & other.permissions)
    }
}

// ============================================================================
// NESNE SINIFI (ObjectClass)
//
// SELinux'ta her kaynak bir sınıfa aittir. Erişim kararı
// (kaynak_tipi, hedef_tipi, nesne_sınıfı, istenen_izinler) dörtlüsüne göre verilir.
//
//  Yaygın sınıflar: File, Dir, Process, Socket, Device, Key, Port
// ============================================================================

/// Nesne sınıfı - hangi tür kaynağa erişildiğini belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectClass {
    File,
    Directory,
    Process,
    Socket,
    Device,
    Key,
    Port,
    Node,
    NetworkInterface,
    Security,
    Capability,
}

// ============================================================================
// ERİŞİM KARARI (AccessDecision)
//
// MAC motorunun bir erişim isteğine verdiği yanıt.
//
//  Allow      -> İzin ver, denetim kaydı tutma
//  Deny       -> Reddet, denetim kaydı tutma
//  AuditAllow -> İzin ver VE denetim kaydına yaz
//  AuditDeny  -> Reddet VE denetim kaydına yaz
//  DontAudit  -> Reddet ama sessizce (gürültülü kuralları bastırmak için)
//  NeverAllow -> Derleme zamanı güvenlik teyidi (politika hatası)
// ============================================================================

/// Erişim kararı - MAC motorunun verdiği nihai yanıt
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessDecision {
    Allow,
    Deny,
    AuditAllow,
    AuditDeny,
    DontAudit,
    NeverAllow,
}

// ============================================================================
// TİP ZORLAMA KURALI (TeRule - Type Enforcement Rule)
//
// TE kuralları MAC politikasının çekirdeğini oluşturur.
// Her kural şu anlamı taşır:
//
//   allow <kaynak_tip> <hedef_tip>:<sınıf> { <izinler> }
//
// Örnek: allow user_t bin_t:file { read execute };
//        -> user_t tipindeki süreçler bin_t tipindeki dosyaları okuyup çalıştırabilir
//
// TransitionRule: execve() sonrası yeni tipin ne olacağını belirler
//   type_transition user_t bin_t:process user_t;
//
// RoleAllowRule:  hangi role geçişine izin verileceğini belirler
//   allow system_r user_r;
// ============================================================================

/// TE (Tip Zorlama) kuralı - MAC politikasının temel yapı taşı
#[derive(Clone, Debug)]
pub struct TeRule {
    /// Erişimi isteyen sürecin tipi (örn. "user_t")
    pub source_type: String,
    /// Erişilmek istenen nesnenin tipi (örn. "file_t")
    pub target_type: String,
    /// Nesne sınıfı (dosya, süreç, soket vb.)
    pub object_class: ObjectClass,
    /// İstenen izin bitmaskesi
    pub permissions: AccessVector,
    /// Bu kural uygulandığında verilecek karar
    pub decision: AccessDecision,
}

/// Tip geçiş kuralı (execve() sırasında uygulanır)
#[derive(Clone, Debug)]
pub struct TransitionRule {
    /// Kaynak süreç tipi
    pub source_type: String,
    /// Hedef dosyanın tipi
    pub target_type: String,
    /// Nesne sınıfı
    pub object_class: ObjectClass,
    /// execve() sonrası sürecin alacağı yeni tip
    pub new_type: String,
}

/// Rol geçiş izin kuralı (role transition allow)
#[derive(Clone, Debug)]
pub struct RoleAllowRule {
    /// Geçiş yapan mevcut rol
    pub current_role: String,
    /// Geçiş yapılacak yeni rol
    pub new_role: String,
}

/// Rol geçiş kuralı - hangi tipten hangi role geçileceğini belirler
#[derive(Clone, Debug)]
pub struct RoleTransitionRule {
    /// Mevcut rol
    pub current_role: String,
    /// Kaynak süreç tipi
    pub source_type: String,
    /// Geçiş yapılacak yeni rol
    pub new_role: String,
}

// ============================================================================
// MAC POLİTİKASI (MacPolicy)
//
// Tüm TE kurallarını, geçiş kurallarını ve rol kurallarını bir arada tutan
// büyük veri yapısı. SELinux'ta bu yapı çekirdekte binary policy dosyasından
// yüklenir; burada ünite testleri ve başlatma için el ile oluşturulur.
//
// enforce=true  -> Deny kararları gerçekten erişimi engeller
// enforce=false -> Permissive mod; reddedilen erişimler yalnızca loglanır
// ============================================================================

/// MAC politikası - tüm erişim kurallarını, geçişleri ve rol kurallarını içerir
#[derive(Clone, Debug)]
pub struct MacPolicy {
    /// Politika tanımlayıcı adı (örn. "default", "strict")
    pub name: String,
    /// Politika şema sürümü
    pub version: u32,
    /// Tip zorlama kuralları listesi (first-match değil; tüm eşleşmeler değerlendirilebilir)
    pub te_rules: Vec<TeRule>,
    /// Tip geçiş kuralları (execve() için)
    pub transitions: Vec<TransitionRule>,
    /// Rol geçişine izin veren kurallar
    pub role_allows: Vec<RoleAllowRule>,
    /// Rol geçiş kuralları
    pub role_transitions: Vec<RoleTransitionRule>,
    /// Bağlamı bilinmeyen süreçlere atanan varsayılan kullanıcı
    pub default_user: String,
    /// Bağlamı bilinmeyen süreçlere atanan varsayılan rol
    pub default_role: String,
    /// Bağlamı bilinmeyen süreçlere atanan varsayılan tip
    pub default_type: String,
    /// Zorlama modu: true=enforce (erişim engel), false=permissive (yalnızca log)
    pub enforce: bool,
}

impl MacPolicy {
    pub fn new(name: &str) -> Self {
        MacPolicy {
            name: String::from(name),
            version: 1,
            te_rules: Vec::new(),
            transitions: Vec::new(),
            role_allows: Vec::new(),
            role_transitions: Vec::new(),
            default_user: String::from("system_u"),
            default_role: String::from("system_r"),
            default_type: String::from("kernel_t"),
            enforce: true,
        }
    }

    /// TE kuralı ekler (allow/deny <kaynak> <hedef>:<sınıf> { <izinler> })
    pub fn add_rule(&mut self, source: &str, target: &str, class: ObjectClass, perms: AccessVector, decision: AccessDecision) {
        self.te_rules.push(TeRule {
            source_type: String::from(source),
            target_type: String::from(target),
            object_class: class,
            permissions: perms,
            decision,
        });
    }

    /// Tip geçiş kuralı ekler (execve() sırasında uygulanır)
    pub fn add_transition(&mut self, source: &str, target: &str, class: ObjectClass, new_type: &str) {
        self.transitions.push(TransitionRule {
            source_type: String::from(source),
            target_type: String::from(target),
            object_class: class,
            new_type: String::from(new_type),
        });
    }

    /// Erişim kararı verir: MLS kontrolü -> TE kuralları -> varsayılan karar
    ///
    /// Karar akışı:
    ///   1. MLS "no read up" kontrolü (Bell-LaPadula okuma kuralı)
    ///   2. TE kuralları taranır; eşleşen ilk kural uygulanır
    ///   3. Eşleşme yoksa enforce moduna göre Deny veya Allow döner
    pub fn check_access(&self, source_ctx: &SecurityContext, target_ctx: &SecurityContext, class: ObjectClass, requested: AccessVector) -> AccessDecision {
        // MLS check first
        if !source_ctx.level.dominates(&target_ctx.level) && requested.has(AccessVector::FILE_READ) {
            return AccessDecision::Deny;
        }

        // TE rules
        for rule in &self.te_rules {
            if rule.source_type == source_ctx.type_
                && rule.target_type == target_ctx.type_
                && rule.object_class == class
            {
                if rule.permissions.intersect(&requested).permissions != 0 {
                    return rule.decision;
                }
            }
        }

        // Default deny
        if self.enforce {
            AccessDecision::Deny
        } else {
            AccessDecision::Allow
        }
    }

    /// Geçiş tipi arar: execve() sonrası sürecin hangi tipi alacağını döndürür.
    /// Eşleşme yoksa None döner (kaynak tip değişmez).
    pub fn get_transition(&self, source_type: &str, target_type: &str, class: ObjectClass) -> Option<&str> {
        for trans in &self.transitions {
            if trans.source_type == source_type && trans.target_type == target_type && trans.object_class == class {
                return Some(&trans.new_type);
            }
        }
        None
    }
}

/// Varsayılan MAC politikasını oluşturur.
///
/// Politika içeriği:
///   - kernel_t: tüm kaynaklara tam erişim (çekirdek süreçleri)
///   - user_t:   ev dizini, geçici dizin, /bin ve /lib'e sınırlı erişim
///   - execve() geçişleri: kullanıcı alanı yürütme akışı
///   - Rol geçiş kuralları: system_r -> user_r
pub fn create_default_policy() -> MacPolicy {
    let mut policy = MacPolicy::new("default");

    // Kernel can do everything
    policy.add_rule("kernel_t", "kernel_t", ObjectClass::Process, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "file_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "dir_t", ObjectClass::Directory, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("kernel_t", "device_t", ObjectClass::Device, AccessVector::ALL, AccessDecision::Allow);

    // User domain
    policy.add_rule("user_t", "user_home_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "user_home_t", ObjectClass::Directory, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "user_tmp_t", ObjectClass::File, AccessVector::ALL, AccessDecision::Allow);
    policy.add_rule("user_t", "bin_t", ObjectClass::File, AccessVector::new(AccessVector::FILE_READ | AccessVector::FILE_EXECUTE), AccessDecision::Allow);
    policy.add_rule("user_t", "lib_t", ObjectClass::File, AccessVector::new(AccessVector::FILE_READ | AccessVector::FILE_EXECUTE), AccessDecision::Allow);

    // Process transitions
    policy.add_transition("user_t", "bin_t", ObjectClass::Process, "user_t");
    policy.add_transition("kernel_t", "init_t", ObjectClass::Process, "init_t");

    // Role transitions
    policy.role_allows.push(RoleAllowRule {
        current_role: String::from("system_r"),
        new_role: String::from("user_r"),
    });

    policy.role_transitions.push(RoleTransitionRule {
        current_role: String::from("system_r"),
        source_type: String::from("init_t"),
        new_role: String::from("user_r"),
    });

    policy
}

// ============================================================================
// GLOBAL DURUM - Global Policy ve Bağlam Tabloları
//
// MAC durumu üç global harita ile yönetilir:
//
//  MAC_POLICY         -> Aktif politika (kurallar + mod)
//  PROCESS_CONTEXTS   -> pid -> SecurityContext eşlemesi (her sürecin bağlamı)
//  FILE_CONTEXTS      -> dosya_yolu -> SecurityContext eşlemesi (her dosyanın tipi)
//
// Bu tablolar lazy_static ile thread-safe biçimde başlatılır.
// ============================================================================

// Global policy
lazy_static::lazy_static! {
    /// Global MAC politika örneği (varsayılan politika ile başlar)
    static ref MAC_POLICY: Mutex<MacPolicy> = Mutex::new(create_default_policy());
    /// pid -> SecurityContext eşlemesi (süreç bağlam tablosu)
    static ref PROCESS_CONTEXTS: Mutex<BTreeMap<u64, SecurityContext>> = Mutex::new(BTreeMap::new());
    /// dosya_yolu -> SecurityContext eşlemesi (dosya bağlam tablosu)
    static ref FILE_CONTEXTS: Mutex<BTreeMap<String, SecurityContext>> = Mutex::new(BTreeMap::new());
}

/// Bir sürece güvenlik bağlamı atar (fork/exec sırasında çağrılır).
pub fn init_process_context(pid: u64, context: SecurityContext) {
    PROCESS_CONTEXTS.lock().insert(pid, context);
}

/// Sürecin güvenlik bağlamını döndürür. Bilinmiyorsa None.
pub fn get_process_context(pid: u64) -> Option<SecurityContext> {
    PROCESS_CONTEXTS.lock().get(&pid).cloned()
}

/// Bir dosya yoluna güvenlik bağlamı atar (xattr security.selinux'a karşılık gelir).
pub fn set_file_context(path: &str, context: SecurityContext) {
    FILE_CONTEXTS.lock().insert(String::from(path), context);
}

/// Dosyanın güvenlik bağlamını döndürür. Bilinmiyorsa None.
pub fn get_file_context(path: &str) -> Option<SecurityContext> {
    FILE_CONTEXTS.lock().get(path).cloned()
}

/// Sürecin bir dosyaya erişip erişemeyeceğine karar verir.
///
/// Süreç veya dosya bağlamı bilinmiyorsa varsayılan olarak Deny döner.
/// Dosya bağlamı tanımsızsa "file_t:Low" varsayılan bağlamı kullanılır.
pub fn check_file_access(pid: u64, path: &str, requested: AccessVector) -> AccessDecision {
    let process_ctx = match get_process_context(pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let file_ctx = match get_file_context(path) {
        Some(ctx) => ctx,
        None => {
            // Default context
            SecurityContext::new("system_u", "object_r", "file_t", SecurityLevel::Low)
        }
    };

    let policy = MAC_POLICY.lock();
    policy.check_access(&process_ctx, &file_ctx, ObjectClass::File, requested)
}

/// Bir sürecin başka bir sürece erişip erişemeyeceğine karar verir.
/// Her iki sürecin bağlamı bilinmiyorsa Deny döner.
pub fn check_process_access(source_pid: u64, target_pid: u64, requested: AccessVector) -> AccessDecision {
    let source_ctx = match get_process_context(source_pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let target_ctx = match get_process_context(target_pid) {
        Some(ctx) => ctx,
        None => return AccessDecision::Deny,
    };

    let policy = MAC_POLICY.lock();
    policy.check_access(&source_ctx, &target_ctx, ObjectClass::Process, requested)
}

/// execve() sırasında sürecin alacağı yeni bağlamı hesaplar.
///
/// Geçiş kuralı bulunursa yeni tiple aynı kullanıcı/rol/seviye döner.
/// Kural yoksa None döner (tip değişmez).
pub fn compute_transition(source_pid: u64, target_type: &str) -> Option<SecurityContext> {
    let source_ctx = get_process_context(source_pid)?;
    let policy = MAC_POLICY.lock();

    let new_type = policy.get_transition(&source_ctx.type_, target_type, ObjectClass::Process)?;

    Some(SecurityContext::new(&source_ctx.user, &source_ctx.role, new_type, source_ctx.level))
}

/// Çalışma zamanında özel politika yükler (varsayılan politikanın üzerine yazar).
pub fn load_policy(policy: MacPolicy) {
    *MAC_POLICY.lock() = policy;
}

/// Mevcut politikanın klonunu döndürür.
pub fn get_policy() -> MacPolicy {
    MAC_POLICY.lock().clone()
}

/// Zorlama modunu değiştirir: true=enforce, false=permissive.
pub fn set_enforcing(enforce: bool) {
    MAC_POLICY.lock().enforce = enforce;
}

/// Politikanın zorlama modunda olup olmadığını döndürür.
pub fn is_enforcing() -> bool {
    MAC_POLICY.lock().enforce
}

/// Erişim kararını denetim günlüğüne yazar (yalnızca AuditAllow/AuditDeny için).
///
/// Sessiz kararlar (Allow/Deny/DontAudit) loglanmaz; yalnızca denetim istenen
/// kararlar seri porta yazılır.
pub fn audit_decision(decision: AccessDecision, source_pid: u64, target: &str, class: ObjectClass, perms: AccessVector) {
    match decision {
        AccessDecision::AuditAllow | AccessDecision::AuditDeny => {
            crate::serial_println!(
                "[MAC/AUDIT] {:?}: pid={} target={} class={:?} perms={:#x}",
                decision, source_pid, target, class, perms.permissions
            );
        }
        _ => {}
    }
}
