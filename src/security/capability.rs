//! # Yetenek Tabanlı Güvenlik (Capability-Based Security)
//!
//! Bu modül, kaynaklar üzerinde ince taneli (fine-grained) erişim denetimi sağlar.
//! Geleneksel "root/non-root" ikili modelinin yerine her kaynak için bağımsız
//! yetki nesneleri (capability) kullanılır.
//!
//! ```
//! Geleneksel Model (kaba taneli):
//!   Kullanıcı -> root mu?    -> Her şeyi yapabilir
//!             -> root değil? -> Hiçbir şey yapamaz
//!
//! Capability Modeli (ince taneli):
//!   Süreç A -> [dosya_cap: okuma]         -> /etc/passwd'ı okuyabilir
//!   Süreç B -> [ağ_cap: bağlanma]          -> TCP soketi açabilir
//!   Süreç C -> [bellek_cap: eşleme]        -> mmap yapabilir
//!   (Her yetki bağımsız olarak verilebilir, kısıtlanabilir ve geri alınabilir)
//! ```
//!
//! Özellikler:
//! - Her süreç kendi `CapabilityTable`'ına sahiptir
//! - Yetkiler türetilebilir (derive): üst küme -> alt küme
//! - Yetkiler devredilebilir (transfer): süreçler arası
//! - Yetkiler geri alınabilir (revoke): alt yetkiler de temizlenir
//! - Mühürleme (seal): yetki dondurunca transfer edilemez

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

/// Yetki nesnesi tanımlayıcısı (u64 = 64-bit benzersiz kimlik).
pub type CapId = u64;

// ============================================================================
// YETKİ HAKLARI (CAPABILITY RIGHTS)
//
// Her yetki nesnesi bir haklar kümesine sahiptir.
// Haklar bool alanları ile temsil edilir; türetme sırasında
// yalnızca üst yetki'nn sahip olduğu haklar alt yetki'ye verilebilir.
//
//  Haklar Hiyerarşisi:
//  NONE < READ < WRITE < READ_WRITE < ALL
//
//  Türetme kuralı: parent.read=false ise child.read=true olamaz!
// ============================================================================

/// Bir yetki nesnesinin sahip olduğu haklar kümesi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapRights {
    /// Kaynağı okuma hakkı
    pub read: bool,
    /// Kaynağa yazma hakkı
    pub write: bool,
    /// Kaynağı çalıştırma hakkı
    pub execute: bool,
    /// Kaynağı başka süreçlerle paylaşma hakkı
    pub share: bool,
    /// Yetkiyi başka sürece devretme hakkı
    pub transfer: bool,
}

impl CapRights {
    /// Hiç hak yok
    pub const NONE: Self = CapRights { read: false, write: false, execute: false, share: false, transfer: false };
    /// Yalnızca okuma
    pub const READ: Self = CapRights { read: true, write: false, execute: false, share: false, transfer: false };
    /// Yalnızca yazma
    pub const WRITE: Self = CapRights { read: false, write: true, execute: false, share: false, transfer: false };
    /// Okuma + Yazma
    pub const READ_WRITE: Self = CapRights { read: true, write: true, execute: false, share: false, transfer: false };
    /// Tüm haklar
    pub const ALL: Self = CapRights { read: true, write: true, execute: true, share: true, transfer: true };
}

// ============================================================================
// KAYNAK TİPLERİ
//
// Bir yetki nesnesi hangi tür kaynağa erişim sağladığını belirtir.
// Bu tip, hangi alt sistemin yetki kontrolü yapacağını belirler.
// ============================================================================

/// Yetki nesnesinin temsil ettiği kaynak türü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    /// Normal dosya
    File,
    /// Dizin
    Directory,
    /// Ağ soketi
    Socket,
    /// Donanım aygıtı
    Device,
    /// Bellek bölgesi
    Memory,
    /// Başka bir süreç
    Process,
    /// İş parçacığı
    Thread,
    /// I/O portu veya ağ portu
    Port,
    /// Kriptografik anahtar
    Key,
    /// Sistem servisi
    Service,
}

// ============================================================================
// YETKİ NESNESİ (CAPABILITY)
//
// Her somut yetki bir Capability örneğidir. Kimlik (id), kaynak tipi ve kimliği,
// haklar, sahip süreç, jenerasyon sayacı ve alt yetki listesi içerir.
//
//  Yetki Ağacı Örneği:
//
//  cap_1 (read+write+transfer) --> türetme --> cap_2 (read only)
//       |                                           |
//       +-> children: [cap_2]          generation: parent+1
//
//  cap_1 iptal edilirse (revoke) cap_2 de otomatik iptal edilir.
// ============================================================================

/// Yetki nesnesi.
#[derive(Clone, Debug)]
pub struct Capability {
    /// Benzersiz yetki kimliği
    pub id: CapId,
    /// Kaynağın türü
    pub resource_type: ResourceType,
    /// Kaynağın sistem-içi kimliği (dosya no, süreç no vb.)
    pub resource_id: u64,
    /// Bu yetki ile kullanılabilecek haklar
    pub rights: CapRights,
    /// Sahibi olan sürecin ID'si
    pub owner: u64,  // Process ID
    /// Türetme jenerasyonu (0 = kaynak yetki, +1 her türetmede)
    pub generation: u32,
    /// Bu yetkiden türetilen alt yetki ID'leri
    pub children: Vec<CapId>,
}

// ============================================================================
// YETKİ TABLOSU (CAPABILITY TABLE)
//
// Her süreç kendi yetki tablosuna sahiptir. Tablo, sürecin sahip olduğu
// tüm yetki nesnelerini BTreeMap'te (sıralı, O(log n)) depolar.
//
//  Süreç Yetki Tablosu:
//  +-----------+---------------------------+
//  | cap_id=1  | dosya /etc/passwd, read   |
//  | cap_id=2  | soket tcp:80, connect     |
//  | cap_id=3  | bellek 0x1000-0x2000, rw  |
//  +-----------+---------------------------+
// ============================================================================

/// Süreç başına yetki tablosu.
#[derive(Clone, Debug)]
pub struct CapabilityTable {
    /// Tablonun sahibi olan sürecin ID'si
    pub process_id: u64,
    /// cap_id -> Capability eşleştirmesi
    pub capabilities: BTreeMap<CapId, Capability>,
    /// Bir sonraki atanacak cap_id (monoton artan)
    pub next_cap_id: CapId,
}

impl CapabilityTable {
    pub fn new(process_id: u64) -> Self {
        CapabilityTable {
            process_id,
            capabilities: BTreeMap::new(),
            next_cap_id: 1,
        }
    }

    /// Yeni bir yetki nesnesi oluşturur ve tabloya ekler; CapId döndürür.
    pub fn create(&mut self, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> CapId {
        let id = self.next_cap_id;
        self.next_cap_id += 1;

        let cap = Capability {
            id,
            resource_type,
            resource_id,
            rights,
            owner: self.process_id,
            generation: 0,
            children: Vec::new(),
        };

        self.capabilities.insert(id, cap);
        id
    }

    /// Verilen CapId'nin tabloda olup olmadığını kontrol eder (salt okunur).
    pub fn get(&self, id: CapId) -> Option<&Capability> {
        self.capabilities.get(&id)
    }

    /// Yetki nesnesinin var olup olmadığını ve istenen hakları içerip içermediğini doğrular.
    ///
    /// Tüm required=true alanları için parent'ın ilgili alanı da true olmalıdır.
    pub fn check(&self, id: CapId, required: CapRights) -> bool {
        if let Some(cap) = self.capabilities.get(&id) {
            let r = cap.rights;
            (!required.read || r.read)
                && (!required.write || r.write)
                && (!required.execute || r.execute)
                && (!required.share || r.share)
                && (!required.transfer || r.transfer)
        } else {
            false
        }
    }

    /// Mevcut bir yetkiden daha kısıtlı (alt küme hakları ile) yeni yetki türetir.
    ///
    /// Türetme kuralı: subset_rights <= parent.rights (her alan için)
    /// Başarısız olursa None döner (haklar aşılmaya çalışıldı).
    pub fn derive(&mut self, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
        let parent = self.capabilities.get(&parent_id)?;

        // Alt küme hakları üst yetki'den taşmıyor mu kontrol et
        if subset_rights.read && !parent.rights.read { return None; }
        if subset_rights.write && !parent.rights.write { return None; }
        if subset_rights.execute && !parent.rights.execute { return None; }
        if subset_rights.share && !parent.rights.share { return None; }
        if subset_rights.transfer && !parent.rights.transfer { return None; }

        let child_id = self.next_cap_id;
        self.next_cap_id += 1;

        let child = Capability {
            id: child_id,
            resource_type: parent.resource_type,
            resource_id: parent.resource_id,
            rights: subset_rights,
            owner: self.process_id,
            generation: parent.generation + 1,
            children: Vec::new(),
        };

        self.capabilities.get_mut(&parent_id)?.children.push(child_id);
        self.capabilities.insert(child_id, child);
        Some(child_id)
    }

    /// Yetki nesnesini ve tüm alt yetkilerini özyinelemeli olarak iptal eder.
    ///
    /// Bu operasyon "cascade revoke" (basamaklı iptal) olarak adlandırılır.
    /// Güvenli: tek bir revoke çağrısı tüm türetilmiş yetki ağacını temizler.
    pub fn revoke(&mut self, id: CapId) -> bool {
        if let Some(cap) = self.capabilities.remove(&id) {
            // Alt yetkiler özyinelemeli olarak iptal edilir
            for child_id in cap.children {
                self.revoke(child_id);
            }
            true
        } else {
            false
        }
    }

    /// Yetki nesnesini başka bir sürece devreder.
    ///
    /// Devir için `transfer` hakkı gereklidir; yoksa None döner ve
    /// yetki tabloda bırakılır. Devredilen yetkinin jenerasyon sayacı artar.
    pub fn transfer(&mut self, id: CapId, target_pid: u64) -> Option<Capability> {
        let cap = self.capabilities.remove(&id)?;
        if !cap.rights.transfer {
            // Transfer hakkı yok, yetki geri yerleştir
            self.capabilities.insert(id, cap);
            return None;
        }

        let mut transferred = cap.clone();
        transferred.owner = target_pid;
        transferred.generation += 1;
        Some(transferred)
    }
}

// ============================================================================
// GLOBAL YETKİ YÖNETİCİSİ
//
// Tüm süreçlere ait yetki tablolarını tek bir global Map'te tutar.
// Mutex ile thread-safe erişim sağlanır.
// ============================================================================

lazy_static::lazy_static! {
    /// pid -> CapabilityTable haritası (global, kilitli)
    static ref CAP_TABLES: Mutex<BTreeMap<u64, CapabilityTable>> = Mutex::new(BTreeMap::new());
}

/// Belirtilen süreç için yeni bir (boş) yetki tablosu başlatır.
pub fn init_process(pid: u64) {
    let mut tables = CAP_TABLES.lock();
    tables.insert(pid, CapabilityTable::new(pid));
}

/// Bir sürecin yetki tablosunun klonunu döndürür (salt okunur anlık görüntü).
pub fn get_table(pid: u64) -> Option<CapabilityTable> {
    CAP_TABLES.lock().get(&pid).cloned()
}

/// Belirtilen süreç için yeni yetki nesnesi oluşturur; CapId döndürür.
pub fn create_capability(pid: u64, resource_type: ResourceType, resource_id: u64, rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    Some(table.create(resource_type, resource_id, rights))
}

/// Belirtilen sürecin verilen yetkiyi ve hakları içerip içermediğini kontrol eder.
pub fn check_capability(pid: u64, cap_id: CapId, rights: CapRights) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        table.check(cap_id, rights)
    } else {
        false
    }
}

/// Mevcut bir yetkiden daha kısıtlı alt yetki türetir.
pub fn derive_capability(pid: u64, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    table.derive(parent_id, subset_rights)
}

/// Belirtilen yetkiyi ve tüm alt yetkilerini iptal eder (cascade revoke).
pub fn revoke_capability(pid: u64, cap_id: CapId) -> bool {
    let mut tables = CAP_TABLES.lock();
    if let Some(table) = tables.get_mut(&pid) {
        table.revoke(cap_id)
    } else {
        false
    }
}

/// Bir yetki nesnesini kaynak süreçten hedef sürece devreder.
///
/// Kaynak süreçte `transfer` hakkı yoksa işlem başarısız olur (false döner).
/// Başarılı devir sonrası yetki hedef süreç tablosuna eklenir.
pub fn transfer_capability(from_pid: u64, cap_id: CapId, to_pid: u64) -> bool {
    let mut tables = CAP_TABLES.lock();

    let transferred = {
        let from_table = tables.get_mut(&from_pid);
        if let Some(table) = from_table {
            table.transfer(cap_id, to_pid)
        } else {
            None
        }
    };

    if let Some(cap) = transferred {
        let to_table = tables.get_mut(&to_pid);
        if let Some(table) = to_table {
            table.capabilities.insert(cap.id, cap);
            return true;
        }
    }
    false
}

/// Bir sürecin tüm yetki tablosunu temizler (süreç sonlanmasında çağrılır).
pub fn cleanup_process(pid: u64) {
    CAP_TABLES.lock().remove(&pid);
}

/// Bir yetki nesnesinin transfer edilip edilemediğini kontrol eder (mühürleme kontrolü).
///
/// Transfer hakkı false ise yetki "mühürlü" sayılır.
/// Gerçek mühürleme için `rights.transfer`'ı kalıcı olarak false yapan
/// ayrı bir mechanism gerekebilir; bu şu an için ön kontroldür.
pub fn seal_capability(pid: u64, cap_id: CapId) -> bool {
    let tables = CAP_TABLES.lock();
    if let Some(table) = tables.get(&pid) {
        if let Some(cap) = table.capabilities.get(&cap_id) {
            // Mühürlü yetki transfer edilemez
            // Bu, transfer bayrağının sıfır olmasıyla uygulanır
            return !cap.rights.transfer;
        }
    }
    false
}
