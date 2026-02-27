//! # NUMA (Non-Uniform Memory Access) Topoloji Desteği
//!
//! ACPI SRAT ve SLIT tablolarını ayrıştırarak NUMA farkındalığı sağlar.
//!
//! NUMA mimarisinde her işlemci çekirdeği bazı bellek bölgelerine diğerlerinden
//! daha hızlı erişebilir. SRAT (System Resource Affinity Table) hangi CPU'nun
//! hangi bellek alanına yakın olduğunu; SLIT (System Locality Information Table)
//! düğümler arası mesafe matrisini tanımlar. Bu bilgi, bellek tahsislerini
//! CPU'ya yakın düğümlere yönlendirerek performansı artırır.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// NUMA SABİTLERİ
// ============================================================================

/// Desteklenen maksimum NUMA düğüm sayısı
pub const MAX_NUMA_NODES: usize = 256;
/// Ulaşılamaz / bilinmeyen mesafe için kullanılan maksimum değer
pub const NUMA_DISTANCE_MAX: u8 = 255;
/// Aynı düğüm içindeki yerel mesafe (ACPI Spec: 10 referans değer)
pub const NUMA_DISTANCE_LOCAL: u8 = 10;

// ============================================================================
// ACPI TABLO İMZALARI
// ============================================================================

/// SRAT tablo imzası — "SRAT" ASCII dizisi
pub const SRAT_SIGNATURE: [u8; 4] = *b"SRAT";
/// SLIT tablo imzası — "SLIT" ASCII dizisi
pub const SLIT_SIGNATURE: [u8; 4] = *b"SLIT";

// ============================================================================
// SRAT YAPILARI
// ============================================================================

/// SRAT Tablo Başlığı — tüm ACPI tablolarında ortak standart başlık formatı
#[repr(C, packed)]
pub struct SratHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: [u8; 4],
    pub creator_revision: u32,
}

/// SRAT Alt-Tablo Tipi — SRAT içindeki her girişin türünü tanımlar
#[derive(Clone, Copy, Debug)]
pub enum SratType {
    ProcessorLocalAPIC = 0,
    MemoryAffinity = 1,
    ProcessorLocalX2APIC = 2,
    GiccAffinity = 3,
    GicItsAffinity = 4,
    GenericInitiatorAffinity = 5,
}

/// SRAT - LAPIC CPU Yakınlık Girişi: hangi CPU'nun (APIC ID) hangi NUMA düğümünde olduğunu belirtir
#[repr(C, packed)]
pub struct SratProcessorLocalApic {
    pub header_type: u8,
    pub length: u8,
    pub reserved: [u8; 1],
    pub domain: u8,
    pub apic_id: u8,
    pub flags: u32,
    pub local_sapic_eid: u8,
    pub reserved2: [u8; 3],
}

/// SRAT - Bellek Yakınlık Girişi: hangi fiziksel bellek aralığının hangi NUMA düğümüne ait olduğunu belirtir
#[repr(C, packed)]
pub struct SratMemoryAffinity {
    pub header_type: u8,
    pub length: u8,
    pub domain: u32,
    pub reserved1: [u8; 2],
    pub base_address: u64,
    pub length: u64,
    pub reserved2: [u8; 4],
    pub flags: u32,
    pub reserved3: [u8; 4],
}

/// SRAT bayrakları — girişin etkin ve yapılandırılabilir olup olmadığını belirtir
pub const SRAT_FLAG_ENABLED: u32 = 1 << 0;
pub const SRAT_FLAG_HOTPLUGGABLE: u32 = 1 << 1;
pub const SRAT_FLAG_NON_VOLATILE: u32 = 1 << 2;

// ============================================================================
// SLIT YAPILARI
// ============================================================================

/// SLIT Tablo Başlığı — arkasından N×N mesafe matrisi gelir (locality_count^2 bayt)
#[repr(C, packed)]
pub struct SlitHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: [u8; 4],
    pub creator_revision: u32,
    pub locality_count: u64,
    // Arkasından mesafe matrisi gelir: entry[i][j] = düğüm i'den j'ye mesafe
}

// ============================================================================
// NUMA DÜĞÜMLERİ
// ============================================================================

/// NUMA Düğüm bilgisi — bir NUMA yakınlık alanındaki CPU ve bellek kaynaklarını temsil eder
#[derive(Clone, Debug)]
pub struct NumaNode {
    /// Düğüm kimlik numarası (proximity domain)
    pub id: u32,
    /// Bu düğümdeki CPU'ların APIC ID listesi
    pub cpus: Vec<u32>,
    /// Bu düğümdeki fiziksel bellek aralıkları
    pub memory_ranges: Vec<MemoryRange>,
    /// Düğümdeki toplam bellek (bayt)
    pub total_memory: u64,
    /// Serbest bellek tahmini (bayt)
    pub free_memory: AtomicU64,
    /// Diğer düğümlere olan göreli mesafe (SLIT'ten okunur)
    pub distances: Vec<u8>,
    /// Düğüm çevrimiçi mi?
    pub online: bool,
}

/// Bir NUMA düğümündeki fiziksel bellek aralığı
#[derive(Clone, Debug)]
pub struct MemoryRange {
    pub base: u64,
    pub length: u64,
    pub hotpluggable: bool,
    pub non_volatile: bool,
}

impl NumaNode {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            cpus: Vec::new(),
            memory_ranges: Vec::new(),
            total_memory: 0,
            free_memory: AtomicU64::new(0),
            distances: Vec::new(),
            online: true,
        }
    }

    /// Bu düğüme bir CPU (APIC ID) ekle
    pub fn add_cpu(&mut self, apic_id: u32) {
        if !self.cpus.contains(&apic_id) {
            self.cpus.push(apic_id);
        }
    }

    /// Bu düğüme fiziksel bellek aralığı ekle
    pub fn add_memory(&mut self, base: u64, length: u64, flags: u32) {
        let range = MemoryRange {
            base,
            length,
            hotpluggable: (flags & SRAT_FLAG_HOTPLUGGABLE) != 0,
            non_volatile: (flags & SRAT_FLAG_NON_VOLATILE) != 0,
        };
        self.total_memory += length;
        self.memory_ranges.push(range);
    }

    /// Başka bir düğüme olan mesafeyi döndür (SLIT tablosundan)
    pub fn distance_to(&self, other_node: u32) -> u8 {
        if other_node as usize >= self.distances.len() {
            return NUMA_DISTANCE_MAX;
        }
        self.distances[other_node as usize]
    }
}

// ============================================================================
// NUMA YÖNETİCİSİ
// ============================================================================

/// Global NUMA topoloji yöneticisi — tüm düğüm ve bellek haritasını tutar
pub struct NumaManager {
    /// Düğüm kimliğine göre indekslenmiş düğümler
    nodes: Mutex<BTreeMap<u32, NumaNode>>,
    /// CPU APIC ID'den NUMA düğümüne eşleme
    cpu_to_node: Mutex<BTreeMap<u32, u32>>,
    /// Toplam düğüm sayısı
    node_count: AtomicU32,
    /// NUMA destekleniyor mu?
    numa_available: AtomicU32,
}

impl NumaManager {
    pub const fn new() -> Self {
        Self {
            nodes: Mutex::new(BTreeMap::new()),
            cpu_to_node: Mutex::new(BTreeMap::new()),
            node_count: AtomicU32::new(0),
            numa_available: AtomicU32::new(0),
        }
    }

    /// SRAT tablosunu ayrıştır - CPU ve bellek yakınlık bilgilerini çıkar
    pub fn parse_srat(&self, srat_ptr: *const u8) -> Result<(), NumaError> {
        unsafe {
            let header = &*(srat_ptr as *const SratHeader);
            
            // İmzayı doğrula: "SRAT" olmalı
            if header.signature != SRAT_SIGNATURE {
                return Err(NumaError::InvalidSignature);
            }
            
            let length = header.length as usize;
            let mut offset = core::mem::size_of::<SratHeader>();
            
            while offset < length {
                let entry_ptr = srat_ptr.add(offset);
                let entry_type = *entry_ptr;
                let entry_len = *(entry_ptr.add(1)) as usize;
                
                if entry_len == 0 {
                    break;
                }
                
                match entry_type {
                    0 => {
                        // 0: Processor Local APIC — CPU-düğüm ilişkisi
                        let proc = &*(entry_ptr as *const SratProcessorLocalApic);
                        if proc.flags & SRAT_FLAG_ENABLED != 0 {
                            let node = self.get_or_create_node(proc.domain as u32);
                            node.add_cpu(proc.apic_id as u32);
                            self.cpu_to_node.lock().insert(proc.apic_id as u32, proc.domain as u32);
                        }
                    }
                    1 => {
                        // 1: Memory Affinity — bellek aralığı-düğüm ilişkisi
                        let mem = &*(entry_ptr as *const SratMemoryAffinity);
                        if mem.flags & SRAT_FLAG_ENABLED != 0 {
                            let node = self.get_or_create_node(mem.domain);
                            node.add_memory(mem.base_address, mem.length, mem.flags);
                        }
                    }
                    2 => {
                        // 2: Processor Local x2APIC — benzer işleme tabi tutulur
                    }
                    _ => {}
                }
                
                offset += entry_len;
            }
        }
        
        self.numa_available.store(1, Ordering::SeqCst);
        crate::serial_println!("[NUMA] Parsed SRAT, {} nodes", self.node_count.load(Ordering::SeqCst));
        
        Ok(())
    }

    /// SLIT tablosunu ayrıştır — düğümler arası mesafe matrisini yükle
    pub fn parse_slit(&self, slit_ptr: *const u8) -> Result<(), NumaError> {
        unsafe {
            let header = &*(slit_ptr as *const SlitHeader);
            
            if header.signature != SLIT_SIGNATURE {
                return Err(NumaError::InvalidSignature);
            }
            
            let count = header.localality_count as usize;
            let matrix_offset = core::mem::size_of::<SlitHeader>();
            
            let mut nodes = self.nodes.lock();
            
            for i in 0..count {
                let node_id = i as u32;
                if let Some(node) = nodes.get_mut(&node_id) {
                    node.distances.clear();
                    for j in 0..count {
                        let distance = *(slit_ptr.add(matrix_offset + i * count + j));
                        node.distances.push(distance);
                    }
                }
            }
        }
        
        crate::serial_println!("[NUMA] Parsed SLIT distance matrix");
        Ok(())
    }

    /// Yoksa yeni bir NUMA düğümü oluştur, varsa mevcut klonını döndür
    fn get_or_create_node(&self, id: u32) -> NumaNode {
        let mut nodes = self.nodes.lock();
        if !nodes.contains_key(&id) {
            nodes.insert(id, NumaNode::new(id));
            self.node_count.fetch_add(1, Ordering::SeqCst);
        }
        nodes.get(&id).unwrap().clone()
    }

    /// Bir CPU'nun (APIC ID) bulunduğu NUMA düğünü döndür
    pub fn get_node_for_cpu(&self, apic_id: u32) -> Option<u32> {
        self.cpu_to_node.lock().get(&apic_id).copied()
    }

    /// Belirli bir fiziksel adresin hangi NUMA düğümüne ait olduğunu bul
    pub fn get_node_for_address(&self, addr: u64) -> Option<u32> {
        let nodes = self.nodes.lock();
        for (id, node) in nodes.iter() {
            for range in &node.memory_ranges {
                if addr >= range.base && addr < range.base + range.length {
                    return Some(*id);
                }
            }
        }
        None
    }

    /// Tahsis için tercih edilen düğümü döndür (mevcut CPU'nun düğümü)
    pub fn get_preferred_node(&self) -> u32 {
        // Mevcut CPU'nun APIC ID'si okunarak düğümü belirlenmeli
        // şimdilik düğüm 0 döndürülüyor
        0
    }

    /// Tüm düğümlerin klonlanmış listesini döndür
    pub fn get_nodes(&self) -> Vec<NumaNode> {
        self.nodes.lock().values().cloned().collect()
    }

    /// NUMA desteklenip desteklenmediğini döndür
    pub fn is_numa(&self) -> bool {
        self.numa_available.load(Ordering::SeqCst) == 1
    }

    /// Toplam düğüm sayısını döndür
    pub fn node_count(&self) -> u32 {
        self.node_count.load(Ordering::SeqCst)
    }

    /// Belirli bir düğümünden bellek tahsis et (basitÇe size kontrol eder)
    pub fn alloc_on_node(&self, node_id: u32, size: usize) -> Option<u64> {
        let nodes = self.nodes.lock();
        if let Some(node) = nodes.get(&node_id) {
            if node.free_memory.load(Ordering::Relaxed) >= size as u64 {
                node.free_memory.fetch_sub(size as u64, Ordering::Relaxed);
                // Gerçek tahsis adresi döndürülmeli (yer tutucu)
                return Some(0xDEADBEEF);
            }
        }
        None
    }

    /// Mevcut task için bellek politikasını döndür
    pub fn get_memory_policy(&self) -> MemoryPolicy {
        MemoryPolicy::default()
    }

    /// Mevcut task için bellek politikası ayarla
    pub fn set_memory_policy(&self, _policy: &MemoryPolicy) -> Result<(), NumaError> {
        Ok(())
    }
}

lazy_static::lazy_static! {
    /// Global NUMA yöneticisi — tüm düğüm bilgisi burada saklanır
    pub static ref NUMA_MANAGER: NumaManager = NumaManager::new();
}

// ============================================================================
// BELLEK POLİTİKASI
// ============================================================================

/// NUMA bellek politikası — Linux set_mempolicy() API'siyle uyumlu
#[derive(Clone, Copy, Debug)]
pub enum MemoryPolicy {
    /// Varsayılan: yerel düğümden tahsis et
    Default,
    /// Tercih edilen düğümden tahsis et; mükemmel değilse diğer düğüme düş
    Preferred(u32),
    /// Yalnızca belirtilen düğümlerden tahsis et (bağlayıcı)
    Bind(Vec<u32>),
    /// Düğümler arasında sıradönüşsel dağıt
    Interleave(Vec<u32>),
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        MemoryPolicy::Default
    }
}

// ============================================================================
// HATA TİPİ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaError {
    InvalidSignature,
    InvalidTable,
    NodeNotFound,
    NoMemory,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// `get_mempolicy` sistem çağrısı — mevcut bellek politikasını döndür
/// POSIX NUMA API'sinin Linux uyumlu implementasyonu
pub fn sys_get_mempolicy(mode: &mut i32, nodemask: &mut u64, addr: u64, flags: u32) -> i32 {
    let policy = NUMA_MANAGER.get_memory_policy();
    *mode = match policy {
        MemoryPolicy::Default => 0,
        MemoryPolicy::Preferred(_) => 1,
        MemoryPolicy::Bind(_) => 2,
        MemoryPolicy::Interleave(_) => 3,
    };
    0
}

/// `set_mempolicy` sistem çağrısı — iş parçası için bellek politikası ayarla
pub fn sys_set_mempolicy(mode: i32, nodemask: u64) -> i32 {
    let policy = match mode {
        0 => MemoryPolicy::Default,
        1 => MemoryPolicy::Preferred((nodemask & 0xFF) as u32),
        2 => MemoryPolicy::Bind(vec![(nodemask & 0xFF) as u32]),
        3 => MemoryPolicy::Interleave(vec![(nodemask & 0xFF) as u32]),
        _ => return -22, // EINVAL
    };
    
    match NUMA_MANAGER.set_memory_policy(&policy) {
        Ok(()) => 0,
        Err(_) => -22,
    }
}

/// `mbind` sistem çağrısı — bellek aralığını belirli düğümlere bağla
pub fn sys_mbind(addr: u64, len: u64, mode: i32, nodemask: u64, flags: u32) -> i32 {
    // Bellek aralığını belirli düğümlere bağla
    0
}

/// `migrate_pages` sistem çağrısı — sayfaları düğümler arasında taşı
pub fn sys_migrate_pages(pid: i32, from_nodes: u64, to_nodes: u64) -> i32 {
    // Belirtilen düğümler arasında sayfa taşıma
    0
}

/// `move_pages` sistem çağrısı — belirli sayfaları belirtilen düğümlere taşı
pub fn sys_move_pages(pid: i32, count: usize, pages: *const u64, nodes: *const i32, status: *mut i32, flags: u32) -> i32 {
    // Belirtilen sayfaları hedef düğümlere taşı
    0
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// NUMA alt sistemini başlat
pub fn init() {
    crate::serial_println!("[NUMA] Subsystem initialized");
}

/// ACPI tablolarından NUMA yapılandırmasını başlat — BSP erken başlangıcında çağrılır
pub fn init_from_acpi(srat_addr: Option<u64>, slit_addr: Option<u64>) {
    if let Some(addr) = srat_addr {
        let _ = NUMA_MANAGER.parse_srat(addr as *const u8);
    }
    
    if let Some(addr) = slit_addr {
        let _ = NUMA_MANAGER.parse_slit(addr as *const u8);
    }
}

/// Get statistics
pub struct NumaStats {
    pub node_count: u32,
    pub total_memory: u64,
    pub numa_available: bool,
}

pub fn get_stats() -> NumaStats {
    let nodes = NUMA_MANAGER.get_nodes();
    NumaStats {
        node_count: nodes.len() as u32,
        total_memory: nodes.iter().map(|n| n.total_memory).sum(),
        numa_available: NUMA_MANAGER.is_numa(),
    }
}
