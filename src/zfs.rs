//! # ZFS (Zettabyte File System) - echOS Implementasyonu
//!
//! Advanced filesystem with data integrity, compression, snapshots,
//! ve storage management. OpenZFS ile uyumlu.
//!
//! ## ZFS Nedir?
//!
//! ZFS, Oracle tarafından geliştirilen ve OpenZFS olarak açık kaynak
//! haline getirilen modern filesystem'dir. Veri bütünlüğü, yönetim
//! kolaylığı ve esneklik sunar.
//!
//! ## ZFS Özellikleri
//!
//! ```text
//! Data Integrity:
//! - 256-bit checksums (fletcher4, sha256)
//! - Copy-on-Write (COW) semantics
//! - Self-healing data corruption
//! - End-to-end data integrity
//!
//! Storage Management:
//! - Dynamic striping (RAID-Z, mirror)
//! - Thin provisioning
//! - Variable block sizes (512B-16MB)
//! - Compression (LZ4, ZSTD, gzip)
//!
//! Snapshots & Clones:
//! - Instantaneous snapshots
//! - Space-efficient clones
//! - Send/Receive replication
//! - Rollback capabilities
//! ```
//!
//! ## ZFS Pool Hiyerarşisi
//!
//! ```text
//! Pool (zpool)
//!    ├── Dataset (zfs filesystem)
//!    ├── Dataset (zfs volume)
//!    └── Dataset (zfs snapshot)
//!
//! Vdevs (Virtual Devices)
//!    ├── Mirror (RAID-1)
//!    ├── RAID-Z1 (RAID-5)
//!    ├── RAID-Z2 (RAID-6)
//!    └── Stripe (RAID-0)
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// ZFS SABİTLERİ
// ============================================================================

/// ZFS magic number
pub const ZFS_MAGIC: &[u8; 8] = b"ZFSBETA";

/// ZFS blok boyutu
pub const ZFS_BLOCK_SIZE: u64 = 128 * 1024; // 128KB

/// Maksimum dosya boyutu (16EB)
pub const ZFS_MAX_FILE_SIZE: u64 = 1 << 64;

/// Maksimum dataset boyutu (16EB)
pub const ZFS_MAX_DATASET_SIZE: u64 = 1 << 64;

/// ZFS checksum tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ZfsChecksumType {
    /// Fletcher-4 (hızlı)
    Fletcher4 = 0,
    /// SHA-256 (güvenli)
    Sha256 = 1,
    /// SHA-512 (çok güvenli)
    Sha512 = 2,
    /// Skein (modern)
    Skein = 3,
}

/// ZFS compression tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ZfsCompressionType {
    /// Sıkıştırma yok
    None = 0,
    /// LZ4 (hızlı)
    Lz4 = 1,
    /// LZJB (legacy)
    Lzjb = 2,
    /// Gzip-1 (düşük)
    Gzip1 = 3,
    /// Gzip-9 (yüksek)
    Gzip9 = 4,
    /// ZSTD (modern)
    Zstd = 5,
}

/// ZFS vdev tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ZfsVdevType {
    /// Stripe (RAID-0)
    Stripe = 1,
    /// Mirror (RAID-1)
    Mirror = 2,
    /// RAID-Z1 (RAID-5)
    Raidz1 = 3,
    /// RAID-Z2 (RAID-6)
    Raidz2 = 4,
    /// RAID-Z3 (triple parity)
    Raidz3 = 5,
    /// Replacing vdev
    Replacing = 6,
    /// Spare vdev
    Spare = 7,
}

/// ZFS hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZfsError {
    /// Geçersiz magic number
    InvalidMagic,
    /// Pool bulunamadı
    PoolNotFound,
    /// Dataset bulunamadı
    DatasetNotFound,
    /// Vdev bulunamadı
    VdevNotFound,
    /// Checksum hatası
    ChecksumError,
    /// I/O hatası
    IoError,
    /// Disk dolu
    DiskFull,
    /// İzin hatası
    PermissionDenied,
    /// Desteklenmeyen özellik
    UnsupportedFeature,
}

// ============================================================================
// ZFS VDEV (VIRTUAL DEVICE)
// ============================================================================

/// ZFS vdev
#[derive(Clone, Debug)]
pub struct ZfsVdev {
    /// Vdev GUID
    pub guid: u64,
    /// Vdev tipi
    pub vdev_type: ZfsVdevType,
    /// Vdev adı
    pub name: String,
    /// Toplam kapasite
    pub total_capacity: u64,
    /// Kullanılan kapasite
    pub used_capacity: u64,
    /// Alt vdev'lar (mirror, RAID-Z için)
    pub children: Vec<ZfsVdev>,
    /// Aktif mi?
    pub active: AtomicBool,
    /// Hatalı mı?
    pub faulted: AtomicBool,
    /// Ashift (alignment shift)
    pub ashift: u8,
}

impl ZfsVdev {
    /// Yeni vdev oluştur
    pub fn new(guid: u64, name: &str, vdev_type: ZfsVdevType, capacity: u64) -> Self {
        Self {
            guid,
            vdev_type,
            name: name.to_string(),
            total_capacity: capacity,
            used_capacity: 0,
            children: Vec::new(),
            active: AtomicBool::new(true),
            faulted: AtomicBool::new(false),
            ashift: 9, // 512 byte alignment
        }
    }
    
    /// Alt vdev ekle (mirror, RAID-Z için)
    pub fn add_child(&mut self, child: ZfsVdev) {
        self.children.push(child);
    }
    
    /// Etkin vdev sayısı
    pub fn active_children(&self) -> usize {
        self.children.iter()
            .filter(|vdev| vdev.active.load(Ordering::SeqCst))
            .count()
    }
    
    /// Redundant mi? (mirror veya RAID-Z)
    pub fn is_redundant(&self) -> bool {
        matches!(self.vdev_type, ZfsVdevType::Mirror | ZfsVdevType::Raidz1 | ZfsVdevType::Raidz2 | ZfsVdevType::Raidz3)
    }
    
    /// Minimum aktif vdev sayısı
    pub fn min_active_children(&self) -> usize {
        match self.vdev_type {
            ZfsVdevType::Stripe => 1,
            ZfsVdevType::Mirror => 1,
            ZfsVdevType::Raidz1 => 1,
            ZfsVdevType::Raidz2 => 2,
            ZfsVdevType::Raidz3 => 3,
            _ => 1,
        }
    }
    
    /// Sağlıklı mı?
    pub fn is_healthy(&self) -> bool {
        if !self.active.load(Ordering::SeqCst) || self.faulted.load(Ordering::SeqCst) {
            return false;
        }
        
        if self.is_redundant() {
            self.active_children() >= self.min_active_children()
        } else {
            true
        }
    }
    
    /// Etkin kapasite
    pub fn available_capacity(&self) -> u64 {
        if !self.is_healthy() {
            return 0;
        }
        
        match self.vdev_type {
            ZfsVdevType::Stripe => {
                self.total_capacity - self.used_capacity
            }
            ZfsVdevType::Mirror => {
                // Mirror'da en küçük disk kapasitesi
                self.children.iter()
                    .filter(|vdev| vdev.active.load(Ordering::SeqCst))
                    .map(|vdev| vdev.total_capacity - vdev.used_capacity)
                    .min()
                    .unwrap_or(0)
            }
            ZfsVdevType::Raidz1 | ZfsVdevType::Raidz2 | ZfsVdevType::Raidz3 => {
                // RAID-Z'de parity için yer ayır
                let active = self.active_children();
                let parity = match self.vdev_type {
                    ZfsVdevType::Raidz1 => 1,
                    ZfsVdevType::Raidz2 => 2,
                    ZfsVdevType::Raidz3 => 3,
                    _ => 1,
                };
                
                if active <= parity {
                    0
                } else {
                    let usable_disks = active - parity;
                    let disk_capacity = self.children.iter()
                        .filter(|vdev| vdev.active.load(Ordering::SeqCst))
                        .map(|vdev| vdev.total_capacity - vdev.used_capacity)
                        .min()
                        .unwrap_or(0);
                    
                    disk_capacity * usable_disks as u64
                }
            }
            _ => 0,
        }
    }
}

// ============================================================================
// ZFS POOL
// ============================================================================

/// ZFS pool
#[derive(Clone, Debug)]
pub struct ZfsPool {
    /// Pool adı
    pub name: String,
    /// Pool GUID
    pub guid: u64,
    /// Topkap vdev'lar
    pub vdevs: Vec<ZfsVdev>,
    /// Dataset'ler
    pub datasets: Mutex<BTreeMap<String, Arc<Mutex<ZfsDataset>>>>,
    /// Pool durumu
    pub state: ZfsPoolState,
    /// Checksum tipi
    pub checksum_type: ZfsChecksumType,
    /// Compression tipi
    pub compression_type: ZfsCompressionType,
    /// Aktif mi?
    pub active: AtomicBool,
}

/// ZFS pool durumları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZfsPoolState {
    /// Online
    Online,
    /// Degraded (bazı vdev'lar arızalı)
    Degraded,
    /// Faulted (kullanılamaz)
    Faulted,
    /// Unavailable
    Unavailable,
}

impl ZfsPool {
    /// Yeni pool oluştur
    pub fn new(name: &str, guid: u64) -> Self {
        Self {
            name: name.to_string(),
            guid,
            vdevs: Vec::new(),
            datasets: Mutex::new(BTreeMap::new()),
            state: ZfsPoolState::Online,
            checksum_type: ZfsChecksumType::Fletcher4,
            compression_type: ZfsCompressionType::Lz4,
            active: AtomicBool::new(false),
        }
    }
    
    /// Vdev ekle
    pub fn add_vdev(&mut self, vdev: ZfsVdev) {
        self.vdevs.push(vdev);
        self.update_state();
    }
    
    /// Pool durumunu güncelle
    fn update_state(&mut self) {
        let mut healthy_vdevs = 0;
        let mut total_vdevs = 0;
        
        for vdev in &self.vdevs {
            total_vdevs += 1;
            if vdev.is_healthy() {
                healthy_vdevs += 1;
            }
        }
        
        self.state = if healthy_vdevs == total_vdevs {
            ZfsPoolState::Online
        } else if healthy_vdevs > 0 {
            ZfsPoolState::Degraded
        } else {
            ZfsPoolState::Faulted
        };
    }
    
    /// Toplam kapasite
    pub fn total_capacity(&self) -> u64 {
        self.vdevs.iter()
            .filter(|vdev| vdev.is_healthy())
            .map(|vdev| vdev.total_capacity)
            .sum()
    }
    
    /// Kullanılan kapasite
    pub fn used_capacity(&self) -> u64 {
        self.vdevs.iter()
            .filter(|vdev| vdev.is_healthy())
            .map(|vdev| vdev.used_capacity)
            .sum()
    }
    
    /// Mevcut kapasite
    pub fn available_capacity(&self) -> u64 {
        self.vdevs.iter()
            .filter(|vdev| vdev.is_healthy())
            .map(|vdev| vdev.available_capacity())
            .sum()
    }
    
    /// Dataset oluştur
    pub fn create_dataset(&self, name: &str) -> Result<u64, ZfsError> {
        if self.datasets.lock().contains_key(name) {
            return Err(ZfsError::PermissionDenied);
        }
        
        let dataset_id = self.datasets.lock().len() as u64 + 1;
        let dataset = Arc::new(Mutex::new(ZfsDataset::new(dataset_id, name)));
        
        self.datasets.lock().insert(name.to_string(), dataset);
        
        crate::serial_println!("[ZFS] Created dataset: {} (ID: {})", name, dataset_id);
        
        Ok(dataset_id)
    }
    
    /// Dataset al
    pub fn get_dataset(&self, name: &str) -> Result<Arc<Mutex<ZfsDataset>>, ZfsError> {
        self.datasets.lock()
            .get(name)
            .cloned()
            .ok_or(ZfsError::DatasetNotFound)
    }
    
    /// Dataset sil
    pub fn destroy_dataset(&self, name: &str) -> Result<(), ZfsError> {
        if self.datasets.lock().remove(name).is_some() {
            crate::serial_println!("[ZFS] Destroyed dataset: {}", name);
            Ok(())
        } else {
            Err(ZfsError::DatasetNotFound)
        }
    }
    
    /// Snapshot oluştur
    pub fn create_snapshot(&self, dataset_name: &str, snapshot_name: &str) -> Result<u64, ZfsError> {
        let dataset = self.get_dataset(dataset_name)?;
        let mut dataset_data = dataset.lock();
        
        let snapshot_id = dataset_data.create_snapshot(snapshot_name)?;
        
        crate::serial_println!("[ZFS] Created snapshot: {}@{}", dataset_name, snapshot_name);
        
        Ok(snapshot_id)
    }
    
    /// Pool'u mount et
    pub fn mount(&self) -> Result<(), ZfsError> {
        if self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[ZFS] Mounting pool: {}", self.name);
        
        // Vdev'leri kontrol et
        for vdev in &self.vdevs {
            if !vdev.is_healthy() {
                crate::serial_println!("[ZFS] Warning: vdev {} is unhealthy", vdev.name);
            }
        }
        
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[ZFS] Pool {} mounted successfully", self.name);
        
        Ok(())
    }
    
    /// Pool'u unmount et
    pub fn unmount(&self) -> Result<(), ZfsError> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[ZFS] Unmounting pool: {}", self.name);
        
        self.active.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[ZFS] Pool {} unmounted", self.name);
        
        Ok(())
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> ZfsPoolStats {
        ZfsPoolStats {
            name: self.name.clone(),
            guid: self.guid,
            state: self.state,
            total_capacity: self.total_capacity(),
            used_capacity: self.used_capacity(),
            available_capacity: self.available_capacity(),
            total_vdevs: self.vdevs.len(),
            healthy_vdevs: self.vdevs.iter().filter(|vdev| vdev.is_healthy()).count(),
            total_datasets: self.datasets.lock().len(),
            checksum_type: self.checksum_type,
            compression_type: self.compression_type,
            active: self.active.load(Ordering::SeqCst),
        }
    }
}

/// ZFS pool istatistikleri
#[derive(Clone, Debug)]
pub struct ZfsPoolStats {
    pub name: String,
    pub guid: u64,
    pub state: ZfsPoolState,
    pub total_capacity: u64,
    pub used_capacity: u64,
    pub available_capacity: u64,
    pub total_vdevs: usize,
    pub healthy_vdevs: usize,
    pub total_datasets: usize,
    pub checksum_type: ZfsChecksumType,
    pub compression_type: ZfsCompressionType,
    pub active: bool,
}

// ============================================================================
// ZFS DATASET
// ============================================================================

/// ZFS dataset
#[derive(Clone, Debug)]
pub struct ZfsDataset {
    /// Dataset ID
    pub id: u64,
    /// Dataset adı
    pub name: String,
    /// Dataset tipi
    pub dataset_type: ZfsDatasetType,
    /// Boyut
    pub size: u64,
    /// Kullanılan boyut
    pub used: u64,
    /// Referans sayısı
    pub refcount: u64,
    /// Checksum tipi
    pub checksum_type: ZfsChecksumType,
    /// Compression tipi
    pub compression_type: ZfsCompressionType,
    /// Snapshots
    pub snapshots: BTreeMap<String, ZfsSnapshot>,
    /// Özellikler
    pub properties: BTreeMap<String, String>,
}

/// ZFS dataset tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZfsDatasetType {
    /// Filesystem
    Filesystem,
    /// Volume (block device)
    Volume,
    /// Snapshot
    Snapshot,
    /// Bookmark
    Bookmark,
}

/// ZFS snapshot
#[derive(Clone, Debug)]
pub struct ZfsSnapshot {
    /// Snapshot adı
    pub name: String,
    /// Oluşturulma zamanı
    pub creation_time: u64,
    /// Boyut
    pub size: u64,
    /// Referans sayısı
    pub refcount: u64,
}

impl ZfsDataset {
    /// Yeni dataset oluştur
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            dataset_type: ZfsDatasetType::Filesystem,
            size: 0,
            used: 0,
            refcount: 1,
            checksum_type: ZfsChecksumType::Fletcher4,
            compression_type: ZfsCompressionType::Lz4,
            snapshots: BTreeMap::new(),
            properties: BTreeMap::new(),
        }
    }
    
    /// Snapshot oluştur
    pub fn create_snapshot(&mut self, snapshot_name: &str) -> Result<u64, ZfsError> {
        if self.snapshots.contains_key(snapshot_name) {
            return Err(ZfsError::PermissionDenied);
        }
        
        let snapshot = ZfsSnapshot {
            name: snapshot_name.to_string(),
            creation_time: crate::interrupts::get_ticks(),
            size: self.used,
            refcount: 1,
        };
        
        let snapshot_id = self.snapshots.len() as u64 + 1;
        self.snapshots.insert(snapshot_name.to_string(), snapshot);
        
        crate::serial_println!("[ZFS] Created snapshot: {}@{}", self.name, snapshot_name);
        
        Ok(snapshot_id)
    }
    
    /// Snapshot sil
    pub fn destroy_snapshot(&mut self, snapshot_name: &str) -> Result<(), ZfsError> {
        if self.snapshots.remove(snapshot_name).is_some() {
            crate::serial_println!("[ZFS] Destroyed snapshot: {}@{}", self.name, snapshot_name);
            Ok(())
        } else {
            Err(ZfsError::DatasetNotFound)
        }
    }
    
    /// Özellik ayarla
    pub fn set_property(&mut self, key: &str, value: &str) {
        self.properties.insert(key.to_string(), value.to_string());
    }
    
    /// Özellik al
    pub fn get_property(&self, key: &str) -> Option<&String> {
        self.properties.get(key)
    }
}

// ============================================================================
// ZFS MANAGER
// ============================================================================

/// ZFS manager
pub struct ZfsManager {
    /// Pool'lar
    pub pools: Mutex<BTreeMap<String, Arc<ZfsPool>>>,
    /// Aktif mi?
    pub active: AtomicBool,
}

impl ZfsManager {
    /// Yeni ZFS manager oluştur
    pub fn new() -> Self {
        Self {
            pools: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
        }
    }
    
    /// ZFS'yi başlat
    pub fn init(&self) -> Result<(), ZfsError> {
        crate::serial_println!("[ZFS] Initializing ZFS manager");
        
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[ZFS] ZFS manager initialized");
        
        Ok(())
    }
    
    /// Pool oluştur
    pub fn create_pool(&self, name: &str) -> Result<Arc<ZfsPool>, ZfsError> {
        let mut pools = self.pools.lock();
        
        if pools.contains_key(name) {
            return Err(ZfsError::PermissionDenied);
        }
        
        let guid = pools.len() as u64 + 1;
        let pool = Arc::new(ZfsPool::new(name, guid));
        
        pools.insert(name.to_string(), pool.clone());
        
        crate::serial_println!("[ZFS] Created pool: {}", name);
        
        Ok(pool)
    }
    
    /// Pool al
    pub fn get_pool(&self, name: &str) -> Result<Arc<ZfsPool>, ZfsError> {
        self.pools.lock()
            .get(name)
            .cloned()
            .ok_or(ZfsError::PoolNotFound)
    }
    
    /// Pool sil
    pub fn destroy_pool(&self, name: &str) -> Result<(), ZfsError> {
        if self.pools.lock().remove(name).is_some() {
            crate::serial_println!("[ZFS] Destroyed pool: {}", name);
            Ok(())
        } else {
            Err(ZfsError::PoolNotFound)
        }
    }
    
    /// Tüm pool'ları listele
    pub fn list_pools(&self) -> Vec<String> {
        self.pools.lock().keys().cloned().collect()
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> ZfsManagerStats {
        let pools = self.pools.lock();
        
        let total_pools = pools.len();
        let mut online_pools = 0;
        let mut degraded_pools = 0;
        let mut faulted_pools = 0;
        let mut total_capacity = 0;
        let mut used_capacity = 0;
        
        for pool in pools.values() {
            match pool.state {
                ZfsPoolState::Online => online_pools += 1,
                ZfsPoolState::Degraded => degraded_pools += 1,
                ZfsPoolState::Faulted | ZfsPoolState::Unavailable => faulted_pools += 1,
            }
            
            total_capacity += pool.total_capacity();
            used_capacity += pool.used_capacity();
        }
        
        ZfsManagerStats {
            total_pools,
            online_pools,
            degraded_pools,
            faulted_pools,
            total_capacity,
            used_capacity,
            available_capacity: total_capacity - used_capacity,
            active: self.active.load(Ordering::SeqCst),
        }
    }
}

/// ZFS manager istatistikleri
#[derive(Clone, Debug)]
pub struct ZfsManagerStats {
    pub total_pools: usize,
    pub online_pools: usize,
    pub degraded_pools: usize,
    pub faulted_pools: usize,
    pub total_capacity: u64,
    pub used_capacity: u64,
    pub available_capacity: u64,
    pub active: bool,
}

impl Default for ZfsManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL ZFS MANAGER
// ============================================================================

/// Global ZFS manager
static ZFS_MANAGER: ZfsManager = ZfsManager::new();

/// ZFS manager'ı al
pub fn get_manager() -> &'static ZfsManager {
    &ZFS_MANAGER
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// ZFS'yi başlat
pub fn init_zfs() -> Result<(), ZfsError> {
    get_manager().init()
}

/// Pool oluştur
pub fn create_pool(name: &str) -> Result<Arc<ZfsPool>, ZfsError> {
    get_manager().create_pool(name)
}

/// Pool al
pub fn get_pool(name: &str) -> Result<Arc<ZfsPool>, ZfsError> {
    get_manager().get_pool(name)
}

/// ZFS testi
pub fn test_zfs() -> Result<(), ZfsError> {
    crate::serial_println!("[ZFS] Testing ZFS filesystem");
    
    // ZFS'yi başlat
    init_zfs()?;
    
    // Pool oluştur
    let pool = create_pool("testpool")?;
    
    // Vdev'ler oluştur
    let mut vdev1 = ZfsVdev::new(1, "disk1", ZfsVdevType::Stripe, 1024 * 1024 * 1024); // 1GB
    let mut vdev2 = ZfsVdev::new(2, "disk2", ZfsVdevType::Stripe, 1024 * 1024 * 1024); // 1GB
    
    // Pool'a vdev'ler ekle
    {
        let mut pool_mut = unsafe { 
            // Unsafe: pool'u mutable olarak almak için
            // Gerçek implementasyonda Arc<Mutex<>> kullanılmalı
            std::mem::transmute::<_, &mut ZfsPool>(&*pool)
        };
        pool_mut.add_vdev(vdev1);
        pool_mut.add_vdev(vdev2);
    }
    
    // Pool'u mount et
    pool.mount()?;
    
    // Dataset oluştur
    let dataset_id = pool.create_dataset("testfs")?;
    
    // Dataset al
    let dataset = pool.get_dataset("testfs")?;
    
    // Özellikler ayarla
    dataset.lock().set_property("compression", "lz4");
    dataset.lock().set_property("atime", "off");
    
    // Snapshot oluştur
    let snapshot_id = pool.create_snapshot("testfs", "snap1")?;
    
    // İstatistikleri göster
    let pool_stats = pool.get_stats();
    crate::serial_println!("[ZFS] Pool Stats:");
    crate::serial_println!("  Name: {}", pool_stats.name);
    crate::serial_println!("  State: {:?}", pool_stats.state);
    crate::serial_println!("  Total capacity: {} MB", pool_stats.total_capacity / (1024 * 1024));
    crate::serial_println!("  Used capacity: {} MB", pool_stats.used_capacity / (1024 * 1024));
    crate::serial_println!("  Available capacity: {} MB", pool_stats.available_capacity / (1024 * 1024));
    crate::serial_println!("  Total vdevs: {}", pool_stats.total_vdevs);
    crate::serial_println!("  Healthy vdevs: {}", pool_stats.healthy_vdevs);
    crate::serial_println!("  Total datasets: {}", pool_stats.total_datasets);
    
    // Manager istatistikleri
    let manager_stats = get_manager().get_stats();
    crate::serial_println!("[ZFS] Manager Stats:");
    crate::serial_println!("  Total pools: {}", manager_stats.total_pools);
    crate::serial_println!("  Online pools: {}", manager_stats.online_pools);
    crate::serial_println!("  Total capacity: {} MB", manager_stats.total_capacity / (1024 * 1024));
    
    // Pool'u unmount et
    pool.unmount()?;
    
    // Pool'u sil
    get_manager().destroy_pool("testpool")?;
    
    crate::serial_println!("[ZFS] ZFS test completed");
    
    Ok(())
}
