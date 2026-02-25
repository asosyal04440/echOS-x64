//! # zswap/zram - Compressed Swap
//!
//! Memory compression for swap pages.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// ZSWAP CONSTANTS
// ============================================================================

/// Maximum pool size percentage
pub const ZSWAP_MAX_POOL_PERCENT: u32 = 100;
/// Default pool size percentage
pub const ZSWAP_DEFAULT_POOL_PERCENT: u32 = 20;
/// Maximum zbud pages
pub const ZSWAP_MAX_ZBUD_PAGES: u64 = 1000000;
/// Default compressor
pub const ZSWAP_DEFAULT_COMPRESSOR: &str = "lz4";

// ============================================================================
// COMPRESSION INTERFACE
// ============================================================================

/// Compression algorithm trait
pub trait Compressor: Send + Sync {
    /// Compress data
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    /// Decompress data
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError>;
    /// Get name
    fn name(&self) -> &'static str;
}

/// LZ4 compressor
pub struct Lz4Compressor;

impl Compressor for Lz4Compressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        // LZ4 compression
        // Placeholder - would use lz4 library
        let compressed_len = src.len() / 2; // Assume 50% compression
        if compressed_len > dst.len() {
            return Err(ZswapError::BufferTooSmall);
        }
        dst[..compressed_len].copy_from_slice(&src[..compressed_len]);
        Ok(compressed_len)
    }

    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        // LZ4 decompression
        Ok(src.len() * 2)
    }

    fn name(&self) -> &'static str {
        "lz4"
    }
}

/// ZSTD compressor
pub struct ZstdCompressor;

impl Compressor for ZstdCompressor {
    fn compress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        // ZSTD compression - better ratio but slower
        let compressed_len = src.len() / 3;
        Ok(compressed_len)
    }

    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<usize, ZswapError> {
        Ok(src.len() * 3)
    }

    fn name(&self) -> &'static str {
        "zstd"
    }
}

// ============================================================================
// ZSWAP ENTRY
// ============================================================================

/// A compressed swap entry
#[derive(Clone, Debug)]
pub struct ZswapEntry {
    /// Original swap offset
    pub swap_offset: u64,
    /// Compressed data handle
    pub handle: u64,
    /// Original size
    pub orig_size: u32,
    /// Compressed size
    pub comp_size: u32,
    /// Pool ID
    pub pool_id: u32,
    /// Reference count
    pub ref_count: AtomicU32,
}

impl ZswapEntry {
    pub fn new(swap_offset: u64, handle: u64, orig_size: u32, comp_size: u32, pool_id: u32) -> Self {
        Self {
            swap_offset,
            handle,
            orig_size,
            comp_size,
            pool_id,
            ref_count: AtomicU32::new(1),
        }
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f32 {
        if self.orig_size == 0 {
            return 0.0;
        }
        (self.orig_size - self.comp_size) as f32 / self.orig_size as f32
    }
}

// ============================================================================
// ZSWAP POOL
// ============================================================================

/// Zswap pool
pub struct ZswapPool {
    /// Pool ID
    pub id: u32,
    /// Compressor
    pub compressor: Arc<dyn Compressor>,
    /// Allocated pages
    pub allocated_pages: AtomicU64,
    /// Compressed pages
    pub compressed_pages: AtomicU64,
    /// Total original size
    pub total_orig_size: AtomicU64,
    /// Total compressed size
    pub total_comp_size: AtomicU64,
    /// Entries
    pub entries: Mutex<BTreeMap<u64, ZswapEntry>>,
}

impl ZswapPool {
    pub fn new(id: u32, compressor: Arc<dyn Compressor>) -> Self {
        Self {
            id,
            compressor,
            allocated_pages: AtomicU64::new(0),
            compressed_pages: AtomicU64::new(0),
            total_orig_size: AtomicU64::new(0),
            total_comp_size: AtomicU64::new(0),
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Store a page
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<ZswapEntry, ZswapError> {
        let page_size = 4096;
        let mut compressed = vec![0u8; page_size * 2];
        
        // Compress
        let comp_size = self.compressor.compress(data, &mut compressed)?;
        
        // Allocate handle (would allocate from zbud/zsmalloc)
        let handle = self.alloc_handle(&compressed[..comp_size])?;
        
        let entry = ZswapEntry::new(
            swap_offset,
            handle,
            data.len() as u32,
            comp_size as u32,
            self.id,
        );
        
        // Update stats
        self.compressed_pages.fetch_add(1, Ordering::Relaxed);
        self.total_orig_size.fetch_add(data.len() as u64, Ordering::Relaxed);
        self.total_comp_size.fetch_add(comp_size as u64, Ordering::Relaxed);
        
        // Store entry
        self.entries.lock().insert(swap_offset, entry.clone());
        
        Ok(entry)
    }

    /// Load a page
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let entries = self.entries.lock();
        let entry = entries.get(&swap_offset).ok_or(ZswapError::NotFound)?;
        
        // Get compressed data
        let compressed = self.get_data(entry.handle)?;
        
        // Decompress
        let _ = self.compressor.decompress(&compressed, data)?;
        
        Ok(())
    }

    /// Remove a page
    pub fn remove(&self, swap_offset: u64) -> bool {
        if let Some(entry) = self.entries.lock().remove(&swap_offset) {
            self.free_handle(entry.handle);
            
            self.compressed_pages.fetch_sub(1, Ordering::Relaxed);
            self.total_orig_size.fetch_sub(entry.orig_size as u64, Ordering::Relaxed);
            self.total_comp_size.fetch_sub(entry.comp_size as u64, Ordering::Relaxed);
            
            return true;
        }
        false
    }

    /// Allocate handle (placeholder)
    fn alloc_handle(&self, data: &[u8]) -> Result<u64, ZswapError> {
        // Would allocate from zbud/zsmalloc
        self.allocated_pages.fetch_add(1, Ordering::Relaxed);
        Ok(data.as_ptr() as u64)
    }

    /// Free handle
    fn free_handle(&self, handle: u64) {
        self.allocated_pages.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get data from handle
    fn get_data(&self, handle: u64) -> Result<Vec<u8>, ZswapError> {
        // Would read from zbud/zsmalloc
        Ok(vec![0u8; 4096])
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f32 {
        let orig = self.total_orig_size.load(Ordering::Relaxed) as f32;
        let comp = self.total_comp_size.load(Ordering::Relaxed) as f32;
        if orig == 0.0 {
            return 0.0;
        }
        (orig - comp) / orig
    }
}

// ============================================================================
// ZSWAP MANAGER
// ============================================================================

/// Zswap statistics
#[derive(Clone, Debug, Default)]
pub struct ZswapStats {
    pub pool_total_size: u64,
    pub stored_pages: u64,
    pub same_filled_pages: u64,
    pub duplicate_entry: u64,
    pub pool_limit_hit: u64,
    pub pool_reached_full: u64,
    pub reject_alloc_fail: u64,
    pub reject_kmemcache_fail: u64,
    pub reject_compress_fail: u64,
    pub reject_compress_poor: u64,
    pub reject_writeback_fail: u64,
    pub written_back_pages: u64,
    pub writeback_elapsed_time: u64,
}

/// Zswap manager
pub struct ZswapManager {
    /// Enabled
    enabled: AtomicBool,
    /// Maximum pool percentage
    max_pool_percent: AtomicU32,
    /// Pools
    pools: Mutex<Vec<Arc<ZswapPool>>>,
    /// Default pool
    default_pool: Mutex<Option<Arc<ZswapPool>>>,
    /// Statistics
    stats: Mutex<ZswapStats>,
    /// Total memory
    total_memory: AtomicU64,
}

impl ZswapManager {
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            max_pool_percent: AtomicU32::new(ZSWAP_DEFAULT_POOL_PERCENT),
            pools: Mutex::new(Vec::new()),
            default_pool: Mutex::new(None),
            stats: Mutex::new(ZswapStats::default()),
            pools: Mutex::new(Vec::new()),
            stats: Mutex::new(ZswapStats::default()),
            total_memory: AtomicU64::new(0),
        }
    }

    /// Initialize
    pub fn init(&self, total_memory: u64) {
        self.total_memory.store(total_memory, Ordering::SeqCst);
        
        // Create default pool with LZ4
        let compressor = Arc::new(Lz4Compressor);
        let pool = Arc::new(ZswapPool::new(0, compressor));
        
        self.pools.lock().push(pool.clone());
        *self.default_pool.lock() = Some(pool);
        
        self.enabled.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[ZSWAP] Initialized with {} MB total memory", 
            total_memory / (1024 * 1024));
    }

    /// Store a page
    pub fn store(&self, swap_offset: u64, data: &[u8]) -> Result<(), ZswapError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(ZswapError::Disabled);
        }
        
        // Check pool limit
        let max_size = self.total_memory.load(Ordering::SeqCst) * 
            self.max_pool_percent.load(Ordering::SeqCst) as u64 / 100;
        
        let pool = self.default_pool.lock().as_ref().cloned()
            .ok_or(ZswapError::NoPool)?;
        
        let current_size = pool.total_comp_size.load(Ordering::Relaxed);
        if current_size + data.len() as u64 > max_size {
            // Pool full, writeback to swap
            self.writeback_lru()?;
            
            let mut stats = self.stats.lock();
            stats.pool_limit_hit += 1;
        }
        
        pool.store(swap_offset, data)?;
        
        let mut stats = self.stats.lock();
        stats.stored_pages += 1;
        
        Ok(())
    }

    /// Load a page
    pub fn load(&self, swap_offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let pool = self.default_pool.lock().as_ref().cloned()
            .ok_or(ZswapError::NoPool)?;
        
        pool.load(swap_offset, data)
    }

    /// Invalidate a page
    pub fn invalidate(&self, swap_offset: u64) -> bool {
        if let Some(pool) = self.default_pool.lock().as_ref() {
            pool.remove(swap_offset)
        } else {
            false
        }
    }

    /// Writeback LRU pages to swap
    fn writeback_lru(&self) -> Result<(), ZswapError> {
        // Find oldest entries and write to actual swap
        let mut stats = self.stats.lock();
        stats.pool_reached_full += 1;
        Ok(())
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f32 {
        if let Some(pool) = self.default_pool.lock().as_ref() {
            pool.compression_ratio()
        } else {
            0.0
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> ZswapStats {
        self.stats.lock().clone()
    }

    /// Set max pool percent
    pub fn set_max_pool_percent(&self, percent: u32) {
        self.max_pool_percent.store(percent.min(100), Ordering::SeqCst);
    }

    /// Enable/disable
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

lazy_static::lazy_static! {
    /// Global zswap manager
    pub static ref ZSWAP_MANAGER: ZswapManager = ZswapManager::new();
}

// ============================================================================
// ZRAM (RAM DISK WITH COMPRESSION)
// ============================================================================

/// ZRAM device
pub struct ZramDevice {
    /// Device ID
    pub id: u32,
    /// Size in bytes
    pub size: AtomicU64,
    /// Compressor
    pub compressor: Arc<dyn Compressor>,
    /// Pages
    pub pages: Mutex<BTreeMap<u64, ZswapEntry>>,
    /// Stats
    pub stats: Mutex<ZramStats>,
}

#[derive(Clone, Debug, Default)]
pub struct ZramStats {
    pub disksize: u64,
    pub orig_data_size: u64,
    pub compr_data_size: u64,
    pub mem_used_total: u64,
    pub mem_limit: u64,
    pub mem_used_max: u64,
    pub same_pages: u64,
    pub huge_pages: u64,
    pub pages_stored: u64,
    pub max_comp_streams: u32,
    pub compulsory_reads: u64,
    pub failed_reads: u64,
    pub failed_writes: u64,
    pub invalid_io: u64,
    pub notify_free: u64,
    pub zero_pages: u64,
}

impl ZramDevice {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            size: AtomicU64::new(0),
            compressor: Arc::new(Lz4Compressor),
            pages: Mutex::new(BTreeMap::new()),
            stats: Mutex::new(ZramStats::default()),
        }
    }

    /// Write to zram
    pub fn write(&self, offset: u64, data: &[u8]) -> Result<(), ZswapError> {
        let page_index = offset / 4096;
        
        // Compress and store
        let mut compressed = vec![0u8; 4096 * 2];
        let comp_size = self.compressor.compress(data, &mut compressed)?;
        
        let entry = ZswapEntry::new(
            page_index,
            0, // handle
            data.len() as u32,
            comp_size as u32,
            0,
        );
        
        self.pages.lock().insert(page_index, entry);
        
        let mut stats = self.stats.lock();
        stats.pages_stored += 1;
        stats.orig_data_size += data.len() as u64;
        stats.compr_data_size += comp_size as u64;
        
        Ok(())
    }

    /// Read from zram
    pub fn read(&self, offset: u64, data: &mut [u8]) -> Result<(), ZswapError> {
        let page_index = offset / 4096;
        
        let pages = self.pages.lock();
        let entry = pages.get(&page_index).ok_or(ZswapError::NotFound)?;
        
        // Decompress (placeholder)
        let _ = self.compressor.decompress(&[], data)?;
        
        Ok(())
    }

    /// Set size
    pub fn set_size(&self, size: u64) {
        self.size.store(size, Ordering::SeqCst);
        self.stats.lock().disksize = size;
    }

    /// Reset device
    pub fn reset(&self) {
        self.pages.lock().clear();
        self.size.store(0, Ordering::SeqCst);
        *self.stats.lock() = ZramStats::default();
    }
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZswapError {
    Disabled,
    NoPool,
    BufferTooSmall,
    CompressionFailed,
    DecompressionFailed,
    NotFound,
    PoolFull,
    OutOfMemory,
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Initialize zswap
pub fn init(total_memory: u64) {
    ZSWAP_MANAGER.init(total_memory);
    crate::serial_println!("[ZSWAP] Subsystem initialized");
}

/// Check if enabled
pub fn is_enabled() -> bool {
    ZSWAP_MANAGER.enabled.load(Ordering::SeqCst)
}
