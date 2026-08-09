//! # eBPF Map Types
//!
//! Linux eBPF map altyapısının echOS implementasyonu.
//! Map'ler, eBPF programları ile kernel arasında paylaşımlı veri yapısıdır.
//!
//! ## Desteklenen Map Tipleri
//!
//! | Map Tipi      | Kullanım Alanı                    |
//! |---------------|-----------------------------------|
//! | Hash          | Anahtar-değer çifti (O(1) lookup) |
//! | Array         | Index-tabanlı sabit boyutlu dizi  |
//! | LruHashMap     | LRU eviction ile hash map         |
//! | Ringbuf        | Kernel→user olay akışı            |
//! | Sockmap        | Socket yönlendirme (kaynak)       |
//! | Devmap         | XDP_REDIRECT hedef haritası       |
//! | Cpumap         | XDP → CPU kuyruk yönlendirme      |
//!
//! ## Map Yaşam Döngüsü
//!
//! ```text
//! create_map(type, key_size, value_size, max_entries)
//!     │
//!     ▼
//! bpf_map_lookup_elem(map_id, key) → value pointer
//! bpf_map_update_elem(map_id, key, value, flags)
//! bpf_map_delete_elem(map_id, key)
//! bpf_map_get_next_key(map_id, key) → next key
//!     │
//!     ▼
//! free_map(map_id)
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// BPF MAP CONSTANTS
// ============================================================================

/// Map türleri (Linux uyumlu)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BpfMapType {
    Unspec = 0,
    Hash = 1,
    Array = 2,
    ProgArray = 3,
    PerfEventArray = 4,
    PerCpuHash = 5,
    PerCpuArray = 6,
    StackTrace = 7,
    CgroupArray = 8,
    LruHash = 9,
    LruPerCpuHash = 10,
    LpmTrie = 11,
    ArrayOfMaps = 12,
    HashOfMaps = 13,
    Devmap = 14,
    Sockmap = 15,
    Cpumap = 16,
    Xskmap = 17,
    Sockhash = 18,
    CgroupStorage = 19,
    ReuseportSockarray = 20,
    PerCpuCgroupStorage = 21,
    Queue = 22,
    Stack = 23,
    SkStorage = 24,
    DevmapHash = 25,
    StructOps = 26,
    Ringbuf = 27,
    InodeStorage = 28,
    TaskStorage = 29,
}

/// Map oluşturma/update flag'leri
pub const BPF_NOEXIST: u64 = 1;
pub const BPF_EXIST: u64 = 2;
pub const BPF_F_LOCK: u64 = 4;

/// Maksimum map entry sayısı
pub const BPF_MAP_MAX_ENTRIES: u32 = 1 << 24; // 16M

/// Map oluştururken hata
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpfMapError {
    InvalidType,
    InvalidKeySize,
    InvalidValueSize,
    InvalidMaxEntries,
    NotFound,
    AlreadyExists,
    Full,
    PermissionDenied,
    OutOfMemory,
}

// ============================================================================
// BPF MAP OPS TRAIT
// ============================================================================

/// Tüm map tiplerinin uygulaması gereken ortak arayüz.
///
/// Key ve value ham byte dilimleri olarak taşınır (`&[u8]`).
/// Map implementations are type-erased at the trait level;
/// concrete types handle serialization internally.
pub trait BpfMapOps: Send + Sync {
    /// Map tipi
    fn map_type(&self) -> BpfMapType;
    /// Key boyutu (byte)
    fn key_size(&self) -> u32;
    /// Value boyutu (byte)
    fn value_size(&self) -> u32;
    /// Maksimum entry sayısı
    fn max_entries(&self) -> u32;
    /// Mevcut entry sayısı
    fn entry_count(&self) -> u32;

    /// Key ile value lookup. Başarılı olursa value bytes döner.
    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Key-value çifti ekle/güncelle. flags: BPF_NOEXIST, BPF_EXIST, 0.
    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError>;
    /// Key ile entry sil.
    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError>;
    /// Verilen key'den sonraki key'i döner (iteration). None = son entry.
    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>>;
}

// ============================================================================
// BPF HASH MAP
// ============================================================================

/// BPF_MAP_TYPE_HASH — anahtar-değer hash map.
///
/// `BTreeMap<Vec<u8>, Vec<u8>>` backed (no_std uyumlu).
/// O(log n) lookup/update/delete.
pub struct BpfHashMap {
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    data: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl BpfHashMap {
    pub fn new(key_size: u32, value_size: u32, max_entries: u32) -> Self {
        BpfHashMap {
            key_size,
            value_size,
            max_entries,
            data: BTreeMap::new(),
        }
    }
}

impl BpfMapOps for BpfHashMap {
    fn map_type(&self) -> BpfMapType { BpfMapType::Hash }
    fn key_size(&self) -> u32 { self.key_size }
    fn value_size(&self) -> u32 { self.value_size }
    fn max_entries(&self) -> u32 { self.max_entries }
    fn entry_count(&self) -> u32 { self.data.len() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.get(key).cloned()
    }

    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
        if key.len() != self.key_size as usize || value.len() != self.value_size as usize {
            return Err(BpfMapError::InvalidKeySize);
        }
        let exists = self.data.contains_key(key);
        if flags & BPF_NOEXIST != 0 && exists {
            return Err(BpfMapError::AlreadyExists);
        }
        if flags & BPF_EXIST != 0 && !exists {
            return Err(BpfMapError::NotFound);
        }
        if !exists && self.data.len() >= self.max_entries as usize {
            return Err(BpfMapError::Full);
        }
        self.data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError> {
        self.data.remove(key).ok_or(BpfMapError::NotFound)?;
        Ok(())
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        match key {
            None => self.data.keys().next().cloned(),
            Some(k) => {
                let mut found = false;
                for existing_key in self.data.keys() {
                    if found {
                        return Some(existing_key.clone());
                    }
                    if existing_key == k {
                        found = true;
                    }
                }
                None
            }
        }
    }
}

// ============================================================================
// BPF ARRAY MAP
// ============================================================================

/// BPF_MAP_TYPE_ARRAY — index-tabanlı sabit boyutlu dizi.
///
/// Index = key (u32). O(1) lookup. Silme desteklenmez (sabit boyut).
pub struct BpfArrayMap {
    value_size: u32,
    data: Vec<Vec<u8>>,
    initialized: Vec<bool>,
}

impl BpfArrayMap {
    pub fn new(value_size: u32, max_entries: u32) -> Self {
        let zero_value = vec![0u8; value_size as usize];
        BpfArrayMap {
            value_size,
            data: vec![zero_value; max_entries as usize],
            initialized: vec![false; max_entries as usize],
        }
    }

    fn key_to_index(&self, key: &[u8]) -> Result<usize, BpfMapError> {
        if key.len() < 4 {
            return Err(BpfMapError::InvalidKeySize);
        }
        let index = u32::from_ne_bytes([key[0], key[1], key[2], key[3]]) as usize;
        if index >= self.data.len() {
            return Err(BpfMapError::NotFound);
        }
        Ok(index)
    }
}

impl BpfMapOps for BpfArrayMap {
    fn map_type(&self) -> BpfMapType { BpfMapType::Array }
    fn key_size(&self) -> u32 { 4 }
    fn value_size(&self) -> u32 { self.value_size }
    fn max_entries(&self) -> u32 { self.data.len() as u32 }
    fn entry_count(&self) -> u32 { self.initialized.iter().filter(|&&v| v).count() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        let index = self.key_to_index(key).ok()?;
        if self.initialized[index] {
            Some(self.data[index].clone())
        } else {
            Some(vec![0u8; self.value_size as usize]) // Array her zaman 0 döner
        }
    }

    fn update(&mut self, key: &[u8], value: &[u8], _flags: u64) -> Result<(), BpfMapError> {
        let index = self.key_to_index(key)?;
        if value.len() != self.value_size as usize {
            return Err(BpfMapError::InvalidValueSize);
        }
        self.data[index] = value.to_vec();
        self.initialized[index] = true;
        Ok(())
    }

    fn delete(&mut self, _key: &[u8]) -> Result<(), BpfMapError> {
        // Array map'te silme yok — BPF_MAP_TYPE_ARRAY delete desteklemez
        Err(BpfMapError::PermissionDenied)
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        let start = match key {
            None => 0,
            Some(k) => {
                let index = self.key_to_index(k).ok()?;
                if index + 1 >= self.data.len() {
                    return None;
                }
                index + 1
            }
        };
        Some((start as u32).to_ne_bytes().to_vec())
    }
}

// ============================================================================
// BPF LRU HASH MAP
// ============================================================================

/// BPF_MAP_TYPE_LRU_HASH — LRU eviction ile hash map.
///
/// Kapasite dolduğunda en eski kullanılan entry otomatik silinir.
pub struct BpfLruHashMap {
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    data: BTreeMap<Vec<u8>, (Vec<u8>, u64)>, // key → (value, access_timestamp)
    access_counter: u64,
}

impl BpfLruHashMap {
    pub fn new(key_size: u32, value_size: u32, max_entries: u32) -> Self {
        BpfLruHashMap {
            key_size,
            value_size,
            max_entries,
            data: BTreeMap::new(),
            access_counter: 0,
        }
    }

    fn evict_lru(&mut self) {
        if self.data.len() < self.max_entries as usize {
            return;
        }
        // En düşük timestamp'e sahip entry'yi bul ve sil
        if let Some(oldest_key) = self.data.iter()
            .min_by_key(|(_, (_, ts))| *ts)
            .map(|(k, _)| k.clone())
        {
            self.data.remove(&oldest_key);
        }
    }
}

impl BpfMapOps for BpfLruHashMap {
    fn map_type(&self) -> BpfMapType { BpfMapType::LruHash }
    fn key_size(&self) -> u32 { self.key_size }
    fn value_size(&self) -> u32 { self.value_size }
    fn max_entries(&self) -> u32 { self.max_entries }
    fn entry_count(&self) -> u32 { self.data.len() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        // NOT: LRU access timestamp güncelleme &mut self gerektirir.
        // Bu trait method'u &self — immutable lookup yapar.
        // Gerçek LRU güncelleme update() tarafında yapılır.
        self.data.get(key).map(|(v, _)| v.clone())
    }

    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
        if key.len() != self.key_size as usize || value.len() != self.value_size as usize {
            return Err(BpfMapError::InvalidKeySize);
        }
        let exists = self.data.contains_key(key);
        if flags & BPF_NOEXIST != 0 && exists {
            return Err(BpfMapError::AlreadyExists);
        }
        if !exists {
            self.evict_lru();
        }
        self.access_counter += 1;
        self.data.insert(key.to_vec(), (value.to_vec(), self.access_counter));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError> {
        self.data.remove(key).ok_or(BpfMapError::NotFound)?;
        Ok(())
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        match key {
            None => self.data.keys().next().cloned(),
            Some(k) => {
                let mut found = false;
                for existing_key in self.data.keys() {
                    if found {
                        return Some(existing_key.clone());
                    }
                    if existing_key == k {
                        found = true;
                    }
                }
                None
            }
        }
    }
}

// ============================================================================
// BPF RINGBUF
// ============================================================================

/// BPF_MAP_TYPE_RINGBUF — SPSC ring buffer (kernel→user event stream).
///
/// eBPF programları `bpf_ringbuf_output` ile olay yazar,
/// user-space `bpf_ringbuf_reserve`/`bpf_ringbuf_submit` ile okur.
pub struct BpfRingbuf {
    buffer: Vec<u8>,
    capacity: usize,
    head: usize,
    tail: usize,
    dropped_count: AtomicU64,
}

impl BpfRingbuf {
    pub fn new(capacity: usize) -> Self {
        // Kapasite 2'nin kuvveti olmalı
        let cap = capacity.next_power_of_two();
        BpfRingbuf {
            buffer: vec![0u8; cap],
            capacity: cap,
            head: 0,
            tail: 0,
            dropped_count: AtomicU64::new(0),
        }
    }

    /// User-space'e veri yaz (producer tarafı).
    pub fn output(&mut self, data: &[u8]) -> Result<(), BpfMapError> {
        let len = data.len();
        if len > self.capacity {
            return Err(BpfMapError::OutOfMemory);
        }

        // Boş alan kontrolü
        let used = if self.head >= self.tail {
            self.head - self.tail
        } else {
            self.capacity - self.tail + self.head
        };

        if used + len > self.capacity {
            self.dropped_count.fetch_add(1, Ordering::Relaxed);
            return Err(BpfMapError::Full);
        }

        // Ring buffer'a yaz
        for (i, &byte) in data.iter().enumerate() {
            let idx = (self.head + i) % self.capacity;
            self.buffer[idx] = byte;
        }
        self.head = (self.head + len) % self.capacity;
        Ok(())
    }

    /// User-space'den veri oku (consumer tarafı).
    pub fn consume(&mut self, max_len: usize) -> Option<Vec<u8>> {
        if self.tail == self.head {
            return None; // Boş
        }

        let available = if self.head > self.tail {
            self.head - self.tail
        } else {
            self.capacity - self.tail + self.head
        };

        let read_len = available.min(max_len);
        let mut result = Vec::with_capacity(read_len);
        for i in 0..read_len {
            let idx = (self.tail + i) % self.capacity;
            result.push(self.buffer[idx]);
        }
        self.tail = (self.tail + read_len) % self.capacity;
        Some(result)
    }

    /// Dropped event sayısı
    pub fn dropped(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Buffer'daki bekleyen byte sayısı
    pub fn available(&self) -> usize {
        if self.head >= self.tail {
            self.head - self.tail
        } else {
            self.capacity - self.tail + self.head
        }
    }
}

// BpfRingbuf BpfMapOps implemente etmez — ringbuf farklı bir API kullanır.
// Ringbuf output/consume ayrı method'lardır.

// ============================================================================
// SOCKMAP / DEVMAP / CPUMAP (Registry-based)
// ============================================================================

/// BPF_MAP_TYPE_SOCKMAP — socket yönlendirme haritası.
///
/// eBPF programları socketleri map'e ekleyebilir ve
/// stream parser/receiver ile socketler arası veri aktarımı yapabilir.
pub struct BpfSockmap {
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    /// key → socket_id mapping
    entries: BTreeMap<Vec<u8>, u32>,
}

impl BpfSockmap {
    pub fn new(key_size: u32, max_entries: u32) -> Self {
        BpfSockmap {
            key_size,
            value_size: 4, // socket_id = u32
            max_entries,
            entries: BTreeMap::new(),
        }
    }
}

impl BpfMapOps for BpfSockmap {
    fn map_type(&self) -> BpfMapType { BpfMapType::Sockmap }
    fn key_size(&self) -> u32 { self.key_size }
    fn value_size(&self) -> u32 { self.value_size }
    fn max_entries(&self) -> u32 { self.max_entries }
    fn entry_count(&self) -> u32 { self.entries.len() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.entries.get(key).map(|id| id.to_ne_bytes().to_vec())
    }

    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
        if key.len() != self.key_size as usize {
            return Err(BpfMapError::InvalidKeySize);
        }
        if value.len() < 4 {
            return Err(BpfMapError::InvalidValueSize);
        }
        let exists = self.entries.contains_key(key);
        if flags & BPF_NOEXIST != 0 && exists {
            return Err(BpfMapError::AlreadyExists);
        }
        if !exists && self.entries.len() >= self.max_entries as usize {
            return Err(BpfMapError::Full);
        }
        let socket_id = u32::from_ne_bytes([value[0], value[1], value[2], value[3]]);
        self.entries.insert(key.to_vec(), socket_id);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError> {
        self.entries.remove(key).ok_or(BpfMapError::NotFound)?;
        Ok(())
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        match key {
            None => self.entries.keys().next().cloned(),
            Some(k) => {
                let mut found = false;
                for existing_key in self.entries.keys() {
                    if found {
                        return Some(existing_key.clone());
                    }
                    if existing_key == k {
                        found = true;
                    }
                }
                None
            }
        }
    }
}

/// BPF_MAP_TYPE_DEVMAP — XDP_REDIRECT hedef device haritası.
pub struct BpfDevmap {
    key_size: u32,
    max_entries: u32,
    /// key (index) → ifindex mapping
    entries: BTreeMap<Vec<u8>, u32>,
}

impl BpfDevmap {
    pub fn new(max_entries: u32) -> Self {
        BpfDevmap {
            key_size: 4,
            max_entries,
            entries: BTreeMap::new(),
        }
    }
}

impl BpfMapOps for BpfDevmap {
    fn map_type(&self) -> BpfMapType { BpfMapType::Devmap }
    fn key_size(&self) -> u32 { self.key_size }
    fn value_size(&self) -> u32 { 4 }
    fn max_entries(&self) -> u32 { self.max_entries }
    fn entry_count(&self) -> u32 { self.entries.len() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.entries.get(key).map(|id| id.to_ne_bytes().to_vec())
    }

    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
        if key.len() != 4 || value.len() < 4 {
            return Err(BpfMapError::InvalidKeySize);
        }
        let exists = self.entries.contains_key(key);
        if flags & BPF_NOEXIST != 0 && exists {
            return Err(BpfMapError::AlreadyExists);
        }
        if !exists && self.entries.len() >= self.max_entries as usize {
            return Err(BpfMapError::Full);
        }
        let ifindex = u32::from_ne_bytes([value[0], value[1], value[2], value[3]]);
        self.entries.insert(key.to_vec(), ifindex);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError> {
        self.entries.remove(key).ok_or(BpfMapError::NotFound)?;
        Ok(())
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        match key {
            None => self.entries.keys().next().cloned(),
            Some(k) => {
                let mut found = false;
                for existing_key in self.entries.keys() {
                    if found {
                        return Some(existing_key.clone());
                    }
                    if existing_key == k {
                        found = true;
                    }
                }
                None
            }
        }
    }
}

/// BPF_MAP_TYPE_CPUMAP — XDP → CPU kuyruk yönlendirme haritası.
pub struct BpfCpumap {
    max_entries: u32,
    /// key (cpu_id) → queue_size mapping
    entries: BTreeMap<Vec<u8>, u32>,
}

impl BpfCpumap {
    pub fn new(max_entries: u32) -> Self {
        BpfCpumap {
            max_entries,
            entries: BTreeMap::new(),
        }
    }
}

impl BpfMapOps for BpfCpumap {
    fn map_type(&self) -> BpfMapType { BpfMapType::Cpumap }
    fn key_size(&self) -> u32 { 4 }
    fn value_size(&self) -> u32 { 4 }
    fn max_entries(&self) -> u32 { self.max_entries }
    fn entry_count(&self) -> u32 { self.entries.len() as u32 }

    fn lookup(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.entries.get(key).map(|sz| sz.to_ne_bytes().to_vec())
    }

    fn update(&mut self, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
        if key.len() != 4 || value.len() < 4 {
            return Err(BpfMapError::InvalidKeySize);
        }
        let exists = self.entries.contains_key(key);
        if flags & BPF_NOEXIST != 0 && exists {
            return Err(BpfMapError::AlreadyExists);
        }
        if !exists && self.entries.len() >= self.max_entries as usize {
            return Err(BpfMapError::Full);
        }
        let queue_size = u32::from_ne_bytes([value[0], value[1], value[2], value[3]]);
        self.entries.insert(key.to_vec(), queue_size);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), BpfMapError> {
        self.entries.remove(key).ok_or(BpfMapError::NotFound)?;
        Ok(())
    }

    fn get_next_key(&self, key: Option<&[u8]>) -> Option<Vec<u8>> {
        match key {
            None => self.entries.keys().next().cloned(),
            Some(k) => {
                let mut found = false;
                for existing_key in self.entries.keys() {
                    if found {
                        return Some(existing_key.clone());
                    }
                    if existing_key == k {
                        found = true;
                    }
                }
                None
            }
        }
    }
}

// ============================================================================
// GLOBAL MAP REGISTRY
// ============================================================================

/// Map ID üreteci
static NEXT_MAP_ID: AtomicU32 = AtomicU32::new(1);

/// Global map registry — map_id → Box<dyn BpfMapOps>
static MAP_REGISTRY: Mutex<BTreeMap<u32, Box<dyn BpfMapOps>>> = Mutex::new(BTreeMap::new());

/// Ringbuf'lar ayrı tutulur (trait implemente etmez)
static RINGBUF_REGISTRY: Mutex<BTreeMap<u32, BpfRingbuf>> = Mutex::new(BTreeMap::new());

/// Yeni bir BPF map oluştur ve registry'ye kaydet.
///
/// Dönüş: map_id (başarılı) veya BpfMapError (hata)
pub fn create_map(
    map_type: BpfMapType,
    key_size: u32,
    value_size: u32,
    max_entries: u32,
) -> Result<u32, BpfMapError> {
    if max_entries == 0 || max_entries > BPF_MAP_MAX_ENTRIES {
        return Err(BpfMapError::InvalidMaxEntries);
    }

    let map_id = NEXT_MAP_ID.fetch_add(1, Ordering::Relaxed);

    match map_type {
        BpfMapType::Hash => {
            if key_size == 0 || value_size == 0 {
                return Err(BpfMapError::InvalidKeySize);
            }
            let map = BpfHashMap::new(key_size, value_size, max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        BpfMapType::Array => {
            if value_size == 0 {
                return Err(BpfMapError::InvalidValueSize);
            }
            let map = BpfArrayMap::new(value_size, max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        BpfMapType::LruHash => {
            if key_size == 0 || value_size == 0 {
                return Err(BpfMapError::InvalidKeySize);
            }
            let map = BpfLruHashMap::new(key_size, value_size, max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        BpfMapType::Ringbuf => {
            let rb = BpfRingbuf::new(max_entries as usize);
            RINGBUF_REGISTRY.lock().insert(map_id, rb);
        }
        BpfMapType::Sockmap => {
            let map = BpfSockmap::new(key_size, max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        BpfMapType::Devmap => {
            let map = BpfDevmap::new(max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        BpfMapType::Cpumap => {
            let map = BpfCpumap::new(max_entries);
            MAP_REGISTRY.lock().insert(map_id, Box::new(map));
        }
        _ => return Err(BpfMapError::InvalidType),
    }

    Ok(map_id)
}

/// Map lookup — key ile value getir
pub fn map_lookup_elem(map_id: u32, key: &[u8]) -> Option<Vec<u8>> {
    let registry = MAP_REGISTRY.lock();
    registry.get(&map_id)?.lookup(key)
}

/// Map update — key-value çifti ekle/güncelle
pub fn map_update_elem(map_id: u32, key: &[u8], value: &[u8], flags: u64) -> Result<(), BpfMapError> {
    let mut registry = MAP_REGISTRY.lock();
    let map = registry.get_mut(&map_id).ok_or(BpfMapError::NotFound)?;
    map.update(key, value, flags)
}

/// Map delete — key ile entry sil
pub fn map_delete_elem(map_id: u32, key: &[u8]) -> Result<(), BpfMapError> {
    let mut registry = MAP_REGISTRY.lock();
    let map = registry.get_mut(&map_id).ok_or(BpfMapError::NotFound)?;
    map.delete(key)
}

/// Map iteration — verilen key'den sonraki key'i döner
pub fn map_get_next_key(map_id: u32, key: Option<&[u8]>) -> Option<Vec<u8>> {
    let registry = MAP_REGISTRY.lock();
    registry.get(&map_id)?.get_next_key(key)
}

/// Ringbuf output — eBPF programından user-space'e olay yaz
pub fn ringbuf_output(map_id: u32, data: &[u8]) -> Result<(), BpfMapError> {
    let mut ringbufs = RINGBUF_REGISTRY.lock();
    let rb = ringbufs.get_mut(&map_id).ok_or(BpfMapError::NotFound)?;
    rb.output(data)
}

/// Ringbuf consume — user-space tarafı olay oku
pub fn ringbuf_consume(map_id: u32, max_len: usize) -> Option<Vec<u8>> {
    let mut ringbufs = RINGBUF_REGISTRY.lock();
    let rb = ringbufs.get_mut(&map_id)?;
    rb.consume(max_len)
}

/// Map sil (registry'den kaldır)
pub fn free_map(map_id: u32) -> bool {
    let mut registry = MAP_REGISTRY.lock();
    if registry.remove(&map_id).is_some() {
        return true;
    }
    let mut ringbufs = RINGBUF_REGISTRY.lock();
    ringbufs.remove(&map_id).is_some()
}

/// Map bilgisi getir
pub fn map_info(map_id: u32) -> Option<(BpfMapType, u32, u32, u32, u32)> {
    let registry = MAP_REGISTRY.lock();
    if let Some(map) = registry.get(&map_id) {
        return Some((map.map_type(), map.key_size(), map.value_size(), map.max_entries(), map.entry_count()));
    }
    let ringbufs = RINGBUF_REGISTRY.lock();
    if let Some(rb) = ringbufs.get(&map_id) {
        return Some((BpfMapType::Ringbuf, 0, 0, rb.capacity as u32, rb.available() as u32));
    }
    None
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_map_crud() {
        let map_id = create_map(BpfMapType::Hash, 4, 4, 100).unwrap();
        let key = 1u32.to_ne_bytes();
        let val = 42u32.to_ne_bytes();

        // Insert
        map_update_elem(map_id, &key, &val, 0).unwrap();

        // Lookup
        let result = map_lookup_elem(map_id, &key).unwrap();
        assert_eq!(result, val);

        // Delete
        map_delete_elem(map_id, &key).unwrap();
        assert!(map_lookup_elem(map_id, &key).is_none());

        free_map(map_id);
    }

    #[test]
    fn hash_map_noexist_flag() {
        let map_id = create_map(BpfMapType::Hash, 4, 4, 100).unwrap();
        let key = 1u32.to_ne_bytes();
        let val = 42u32.to_ne_bytes();

        map_update_elem(map_id, &key, &val, BPF_NOEXIST).unwrap();
        // Tekrar NOEXIST ile eklemeye çalış → AlreadyExists
        assert_eq!(
            map_update_elem(map_id, &key, &val, BPF_NOEXIST),
            Err(BpfMapError::AlreadyExists)
        );

        free_map(map_id);
    }

    #[test]
    fn array_map_indexed_access() {
        let map_id = create_map(BpfMapType::Array, 4, 4, 10).unwrap();
        let key_0 = 0u32.to_ne_bytes();
        let key_5 = 5u32.to_ne_bytes();
        let val = 99u32.to_ne_bytes();

        // Array map index 0 → default 0
        let result = map_lookup_elem(map_id, &key_0).unwrap();
        assert_eq!(result, [0, 0, 0, 0]);

        // Update index 5
        map_update_elem(map_id, &key_5, &val, 0).unwrap();
        let result = map_lookup_elem(map_id, &key_5).unwrap();
        assert_eq!(result, val);

        // Array delete unsupported
        assert_eq!(map_delete_elem(map_id, &key_5), Err(BpfMapError::PermissionDenied));

        free_map(map_id);
    }

    #[test]
    fn lru_hash_eviction() {
        let map_id = create_map(BpfMapType::LruHash, 4, 4, 3).unwrap();
        let val = 1u32.to_ne_bytes();

        // 3 entry doldur
        for i in 0..3u32 {
            map_update_elem(map_id, &i.to_ne_bytes(), &val, 0).unwrap();
        }

        // 4. entry → LRU eviction tetiklenmeli
        map_update_elem(map_id, &3u32.to_ne_bytes(), &val, 0).unwrap();
        assert_eq!(map_info(map_id).unwrap().4, 3); // Hâlâ 3 entry

        free_map(map_id);
    }

    #[test]
    fn ringbuf_output_consume() {
        let map_id = create_map(BpfMapType::Ringbuf, 0, 0, 4096).unwrap();

        ringbuf_output(map_id, b"hello").unwrap();
        ringbuf_output(map_id, b"world").unwrap();

        let data = ringbuf_consume(map_id, 1024).unwrap();
        assert_eq!(data, b"helloworld");

        free_map(map_id);
    }

    #[test]
    fn map_iteration() {
        let map_id = create_map(BpfMapType::Hash, 4, 4, 100).unwrap();

        for i in 0..5u32 {
            map_update_elem(map_id, &i.to_ne_bytes(), &(i * 10).to_ne_bytes(), 0).unwrap();
        }

        // get_next_key(None) → first key
        let first = map_get_next_key(map_id, None).unwrap();
        assert_eq!(first, 0u32.to_ne_bytes());

        // get_next_key(0) → 1
        let second = map_get_next_key(map_id, Some(&first)).unwrap();
        assert_eq!(second, 1u32.to_ne_bytes());

        // get_next_key(4) → None (son eleman)
        let last = map_get_next_key(map_id, Some(&4u32.to_ne_bytes()));
        assert!(last.is_none());

        free_map(map_id);
    }

    #[test]
    fn map_info_returns_correct_metadata() {
        let map_id = create_map(BpfMapType::Hash, 4, 8, 100).unwrap();
        let info = map_info(map_id).unwrap();
        assert_eq!(info.0, BpfMapType::Hash);
        assert_eq!(info.1, 4);  // key_size
        assert_eq!(info.2, 8);  // value_size
        assert_eq!(info.3, 100); // max_entries
        assert_eq!(info.4, 0);  // entry_count
        free_map(map_id);
    }

    #[test]
    fn sockmap_crud() {
        let map_id = create_map(BpfMapType::Sockmap, 4, 4, 100).unwrap();
        let key = 1u32.to_ne_bytes();
        let socket_id = 42u32.to_ne_bytes();

        map_update_elem(map_id, &key, &socket_id, 0).unwrap();
        let result = map_lookup_elem(map_id, &key).unwrap();
        assert_eq!(result, socket_id);

        free_map(map_id);
    }

    #[test]
    fn devmap_crud() {
        let map_id = create_map(BpfMapType::Devmap, 4, 4, 16).unwrap();
        let key = 0u32.to_ne_bytes();
        let ifindex = 3u32.to_ne_bytes();

        map_update_elem(map_id, &key, &ifindex, 0).unwrap();
        let result = map_lookup_elem(map_id, &key).unwrap();
        assert_eq!(result, ifindex);

        free_map(map_id);
    }

    #[test]
    fn cpumap_crud() {
        let map_id = create_map(BpfMapType::Cpumap, 4, 4, 8).unwrap();
        let key = 0u32.to_ne_bytes();
        let queue_size = 64u32.to_ne_bytes();

        map_update_elem(map_id, &key, &queue_size, 0).unwrap();
        let result = map_lookup_elem(map_id, &key).unwrap();
        assert_eq!(result, queue_size);

        free_map(map_id);
    }
}
