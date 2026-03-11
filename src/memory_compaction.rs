//! # Memory Compaction - Bellek Sıkıştırma
//!
//! Parçalanmış belleği birleştirerek büyük sürekli bloklar oluşturan
//! yüksek performanslı bellek yönetimi tekniği.
//!
//! ## Memory Compaction Nedir?
//!
//! Bellek tahsis ve serbest bırma işlemleri zamanla bellek parçalanmasına
//! neden olur. Compaction, parçalanmış sayfaları birleştirerek büyük
//! sürekli bellek alanları oluşturur.
//!
//! ## Compaction Mekanizmaları
//!
//! ```text
//! Parçalanmış Bellek:
//! [Used][Free][Used][Used][Free][Free][Used]
//!
//! Compaction Sonrası:
//! [Used][Used][Used][Used][Free][Free][Free]
//! ```
//!
//! ## Kullanım Alanları
//! - Büyük bellek tahsisleri (huge pages)
//! - Veritabanları ve HPC uygulamaları
//! - Sanallaştırma (VM memory)
//! - Düşük gecikme gerektiren sistemler

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use super::huge_pages::HugePageAllocator;

// ============================================================================
// MEMORY COMPACTION SABİTLERİ
// ============================================================================

/// Compaction tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionType {
    /// Sadece boş sayfaları birleştir
    Defrag,
    /// Kullanılmış sayfaları taşı
    Migrate,
    /// Tam compaction (defrag + migrate)
    Full,
}

/// Compaction öncelikleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionPriority {
    /// Düşük öncelik (background)
    Low,
    /// Normal öncelik
    Normal,
    /// Yüksek öncelik (acil)
    High,
    /// Kritik öncelik (out-of-memory)
    Critical,
}

/// Compaction durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionState {
    /// Çalışmıyor
    Idle,
    /// Taranıyor
    Scanning,
    /// Birleştiriyor
    Compacting,
    /// Taşıyor
    Migrating,
    /// Tamamlandı
    Completed,
    /// Hata durumunda
    Failed,
}

/// Compaction hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionError {
    /// Bellek yetersiz
    OutOfMemory,
    /// Taşıma hatası
    MigrationFailed,
    /// Zaman aşımı
    Timeout,
    /// İzin hatası
    PermissionDenied,
    /// Meşgul
    Busy,
    /// Desteklenmeyen işlem
    Unsupported,
}

// ============================================================================
// MEMORY BÖLGESİ TANIMLAMASI
// ============================================================================

/// Bellek bölgesi
#[derive(Clone, Debug)]
pub struct MemoryRegion {
    /// Başlangıç adresi
    pub start: u64,
    /// Bitiş adresi
    pub end: u64,
    /// Boyut
    pub size: u64,
    /// Kullanımda mı?
    pub in_use: bool,
    /// Taşınabilir mi?
    pub migratable: bool,
    /// Sayfa sayısı
    pub page_count: u64,
    /// Boş sayfa sayısı
    pub free_pages: u64,
    /// Bölge tipi
    pub region_type: MemoryRegionType,
}

/// Bellek bölgesi tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Normal bellek
    Normal,
    /// DMA belleği
    Dma,
    /// HighMem
    HighMem,
    /// Reserved
    Reserved,
}

impl MemoryRegion {
    /// Yeni bellek bölgesi oluştur
    pub fn new(start: u64, size: u64, region_type: MemoryRegionType) -> Self {
        Self {
            start,
            end: start + size,
            size,
            in_use: false,
            migratable: true,
            page_count: size / 4096, // 4KB sayfa boyutu
            free_pages: size / 4096,
            region_type,
        }
    }
    
    /// Boş mu?
    pub fn is_free(&self) -> bool {
        !self.in_use && self.free_pages == self.page_count
    }
    
    /// Kısmen dolu mu?
    pub fn is_fragmented(&self) -> bool {
        self.in_use && self.free_pages > 0 && self.free_pages < self.page_count
    }
    
    /// Tamamen dolu mu?
    pub fn is_full(&self) -> bool {
        self.in_use && self.free_pages == 0
    }
}

// ============================================================================
// COMPACTION İSTATİSTİKLERİ
// ============================================================================

/// Compaction istatistikleri
#[derive(Clone, Debug)]
pub struct CompactionStats {
    /// Toplam taranan sayfa
    pub pages_scanned: u64,
    /// Birleştirilen sayfa
    pub pages_compacted: u64,
    /// Taşınan sayfa
    pub pages_migrated: u64,
    /// Oluşturulan büyük blok sayısı
    pub large_blocks_created: u64,
    /// Geçen süre (ms)
    pub duration_ms: u64,
    /// Başarı oranı (%)
    pub success_rate: f32,
}

impl CompactionStats {
    /// Yeni istatistik oluştur
    pub fn new() -> Self {
        Self {
            pages_scanned: 0,
            pages_compacted: 0,
            pages_migrated: 0,
            large_blocks_created: 0,
            duration_ms: 0,
            success_rate: 0.0,
        }
    }
    
    /// Başarı oranını hesapla
    pub fn calculate_success_rate(&mut self) {
        if self.pages_scanned > 0 {
            self.success_rate = (self.pages_compacted + self.pages_migrated) as f32 / self.pages_scanned as f32 * 100.0;
        }
    }
}

impl Default for CompactionStats {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MEMORY COMPACTION ENGINE
// ============================================================================

/// Bellek sıkıştırma motoru
pub struct MemoryCompactionEngine {
    /// Bellek bölgeleri
    regions: Mutex<Vec<MemoryRegion>>,
    /// Compaction durumu
    state: AtomicU64, // CompactionState as u64
    /// Çalışıyor mu?
    active: AtomicBool,
    /// İstatistikler
    stats: Mutex<CompactionStats>,
    /// Compaction tipi
    compaction_type: Mutex<CompactionType>,
    /// Öncelik
    priority: Mutex<CompactionPriority>,
    /// Başlangıç zamanı
    start_time: AtomicU64,
    /// Huge page allocator referansı
    huge_pages: &'static HugePageAllocator,
}

impl MemoryCompactionEngine {
    /// Yeni compaction motoru oluştur
    pub fn new() -> Self {
        Self {
            regions: Mutex::new(Vec::new()),
            state: AtomicU64::new(CompactionState::Idle as u64),
            active: AtomicBool::new(false),
            stats: Mutex::new(CompactionStats::new()),
            compaction_type: Mutex::new(CompactionType::Full),
            priority: Mutex::new(CompactionPriority::Normal),
            start_time: AtomicU64::new(0),
            huge_pages: super::huge_pages::get_allocator(),
        }
    }
    
    /// Bellek bölgesi ekle
    pub fn add_region(&self, region: MemoryRegion) {
        let mut regions = self.regions.lock();
        regions.push(region);
        crate::serial_println!("[Compaction] Added memory region: 0x{:x}-0x{:x}", region.start, region.end);
    }
    
    /// Compaction başlat
    pub fn start_compaction(&self, compaction_type: CompactionType, priority: CompactionPriority) -> Result<(), CompactionError> {
        if self.active.load(Ordering::SeqCst) {
            return Err(CompactionError::Busy);
        }
        
        self.compaction_type.lock().replace(compaction_type);
        self.priority.lock().replace(priority);
        self.start_time.store(crate::interrupts::get_ticks(), Ordering::SeqCst);
        
        // Durumu güncelle
        self.set_state(CompactionState::Scanning);
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[Compaction] Starting {:?} compaction with priority {:?}", compaction_type, priority);
        
        // Compaction işlemini başlat
        match compaction_type {
            CompactionType::Defrag => self.defrag_compaction(),
            CompactionType::Migrate => self.migrate_compaction(),
            CompactionType::Full => self.full_compaction(),
        }
    }
    
    /// Defrag compaction (sadece boş sayfaları birleştir)
    fn defrag_compaction(&self) -> Result<(), CompactionError> {
        self.set_state(CompactionState::Compacting);
        
        let mut regions = self.regions.lock();
        let mut stats = self.stats.lock();
        
        // Bölgeleri boyuta göre sırala
        regions.sort_by(|a, b| b.size.cmp(&a.size));
        
        let mut compacted_pages = 0;
        
        for region in regions.iter_mut() {
            if region.is_fragmented() {
                // Boş sayfaları birleştir
                let old_free_pages = region.free_pages;
                region.free_pages = region.page_count;
                compacted_pages += region.page_count - old_free_pages;
                
                crate::serial_println!(
                    "[Compaction] Defragmented region 0x{:x}: {} pages freed",
                    region.start,
                    region.page_count - old_free_pages
                );
            }
        }
        
        stats.pages_compacted = compacted_pages;
        stats.calculate_success_rate();
        
        self.set_state(CompactionState::Completed);
        self.active.store(false, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Migrate compaction (sayfaları taşı)
    fn migrate_compaction(&self) -> Result<(), CompactionError> {
        self.set_state(CompactionState::Migrating);
        
        let mut regions = self.regions.lock();
        let mut stats = self.stats.lock();
        
        let mut migrated_pages = 0;
        
        // Parçalanmış bölgeleri tara
        for i in 0..regions.len() {
            if regions[i].is_fragmented() && regions[i].migratable {
                // Uygun hedef bölge ara
                for j in (i + 1)..regions.len() {
                    if regions[j].is_free() && regions[j].size >= regions[i].size {
                        // Sayfaları taşı
                        if self.migrate_pages(&regions[i], &regions[j])? {
                            migrated_pages += regions[i].page_count;
                            
                            crate::serial_println!(
                                "[Compaction] Migrated {} pages from 0x{:x} to 0x{:x}",
                                regions[i].page_count,
                                regions[i].start,
                                regions[j].start
                            );
                            
                            break;
                        }
                    }
                }
            }
        }
        
        stats.pages_migrated = migrated_pages;
        stats.calculate_success_rate();
        
        self.set_state(CompactionState::Completed);
        self.active.store(false, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Full compaction (defrag + migrate)
    fn full_compaction(&self) -> Result<(), CompactionError> {
        crate::serial_println!("[Compaction] Starting full compaction");
        
        // Önce defrag
        self.defrag_compaction()?;
        
        // Sonra migrate
        self.migrate_compaction()?;
        
        // Son olarak huge pages için optimize et
        self.optimize_for_huge_pages()?;
        
        Ok(())
    }
    
    /// Sayfaları bir bölgeden diğerine taşı
    fn migrate_pages(&self, source: &MemoryRegion, target: &MemoryRegion) -> Result<bool, CompactionError> {
        // Sayfa taşıma implementasyonu (placeholder)
        crate::serial_println!(
            "[Compaction] Migrating pages from 0x{:x} to 0x{:x} (placeholder)",
            source.start,
            target.start
        );
        
        // Gerçek implementasyonda:
        // 1. Sayfa tablolarını güncelle
        // 2. Veriyi kopyala
        // 3. Eski sayfaları serbest bırak
        
        Ok(true)
    }
    
    /// Huge pages için optimize et
    fn optimize_for_huge_pages(&self) -> Result<(), CompactionError> {
        crate::serial_println!("[Compaction] Optimizing for huge pages");
        
        let mut stats = self.stats.lock();
        let mut large_blocks = 0;
        
        // 2MB bloklar için uygun bölgeler ara
        let regions = self.regions.lock();
        for region in regions.iter() {
            if region.is_free() && region.size >= super::huge_pages::PAGE_SIZE_2MB as u64 {
                large_blocks += region.size / super::huge_pages::PAGE_SIZE_2MB as u64;
            }
        }
        
        stats.large_blocks_created = large_blocks;
        
        crate::serial_println!("[Compaction] Created {} potential 2MB blocks", large_blocks);
        
        Ok(())
    }
    
    /// Compaction durumu
    pub fn get_state(&self) -> CompactionState {
        match self.state.load(Ordering::SeqCst) {
            0 => CompactionState::Idle,
            1 => CompactionState::Scanning,
            2 => CompactionState::Compacting,
            3 => CompactionState::Migrating,
            4 => CompactionState::Completed,
            _ => CompactionState::Failed,
        }
    }
    
    /// Durumu ayarla
    fn set_state(&self, state: CompactionState) {
        self.state.store(state as u64, Ordering::SeqCst);
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> CompactionStats {
        let mut stats = self.stats.lock();
        
        // Süreyi hesapla
        if self.active.load(Ordering::SeqCst) {
            let elapsed = crate::interrupts::get_ticks() - self.start_time.load(Ordering::SeqCst);
            stats.duration_ms = elapsed * 10; // ticks to ms (placeholder)
        }
        
        stats.clone()
    }
    
    /// Bellek haritasını göster
    pub fn print_memory_map(&self) {
        let regions = self.regions.lock();
        
        crate::serial_println!("[Compaction] Memory Map:");
        for region in regions.iter() {
            let status = if region.is_free() {
                "FREE"
            } else if region.is_fragmented() {
                "FRAG"
            } else if region.is_full() {
                "FULL"
            } else {
                "USED"
            };
            
            crate::serial_println!(
                "  0x{:x}-0x{:x} ({} MB) {} {} pages free",
                region.start,
                region.end,
                region.size / (1024 * 1024),
                status,
                region.free_pages
            );
        }
    }
    
    /// Compaction durdur
    pub fn stop_compaction(&self) {
        if self.active.load(Ordering::SeqCst) {
            self.set_state(CompactionState::Idle);
            self.active.store(false, Ordering::SeqCst);
            crate::serial_println!("[Compaction] Compaction stopped");
        }
    }
    
    /// Otomatik compaction kontrolü
    pub fn check_need_compaction(&self) -> bool {
        let regions = self.regions.lock();
        let mut fragmented_count = 0;
        let mut total_regions = 0;
        
        for region in regions.iter() {
            total_regions += 1;
            if region.is_fragmented() {
                fragmented_count += 1;
            }
        }
        
        // %50'den fazlası parçalanmışsa compaction gerekli
        let fragmentation_ratio = fragmented_count as f32 / total_regions as f32;
        
        if fragmentation_ratio > 0.5 {
            crate::serial_println!(
                "[Compaction] Fragmentation ratio: {:.1}% - compaction needed",
                fragmentation_ratio * 100.0
            );
            return true;
        }
        
        false
    }
    
    /// Otomatik compaction başlat
    pub fn auto_compact(&self) -> Result<(), CompactionError> {
        if self.check_need_compaction() && !self.active.load(Ordering::SeqCst) {
            self.start_compaction(CompactionType::Full, CompactionPriority::Normal)
        } else {
            Err(CompactionError::Busy)
        }
    }
}

impl Default for MemoryCompactionEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL COMPACTION ENGINE
// ============================================================================

/// Global memory compaction engine
static COMPACTION_ENGINE: MemoryCompactionEngine = MemoryCompactionEngine::new();

/// Compaction engine'ı al
pub fn get_compaction_engine() -> &'static MemoryCompactionEngine {
    &COMPACTION_ENGINE
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Memory compaction modülünü başlat
pub fn init() {
    crate::serial_println!("[Compaction] Initializing memory compaction");
    
    // Örnek bellek bölgeleri ekle
    let engine = get_compaction_engine();
    
    // 256MB normal bellek bölgesi
    engine.add_region(MemoryRegion::new(0x100000000, 256 * 1024 * 1024, MemoryRegionType::Normal));
    
    // 128MB DMA bellek bölgesi
    engine.add_region(MemoryRegion::new(0x200000000, 128 * 1024 * 1024, MemoryRegionType::Dma));
    
    // 512MB HighMem bölgesi
    engine.add_region(MemoryRegion::new(0x300000000, 512 * 1024 * 1024, MemoryRegionType::HighMem));
    
    crate::serial_println!("[Compaction] Memory compaction initialized");
}

/// Manuel compaction başlat
pub fn start_manual_compaction(compaction_type: CompactionType, priority: CompactionPriority) -> Result<(), CompactionError> {
    get_compaction_engine().start_compaction(compaction_type, priority)
}

/// Otomatik compaction başlat
pub fn start_auto_compaction() -> Result<(), CompactionError> {
    get_compaction_engine().auto_compact()
}

/// Compaction durumu
pub fn get_compaction_state() -> CompactionState {
    get_compaction_engine().get_state()
}

/// Compaction istatistikleri
pub fn get_compaction_stats() -> CompactionStats {
    get_compaction_engine().get_stats()
}

/// Bellek haritasını göster
pub fn print_memory_map() {
    get_compaction_engine().print_memory_map();
}

/// Compaction testi
pub fn test_compaction() -> Result<(), CompactionError> {
    crate::serial_println!("[Compaction] Testing memory compaction");
    
    // Bellek haritasını göster
    print_memory_map();
    
    // Fragmentation simüle et
    let engine = get_compaction_engine();
    {
        let mut regions = engine.regions.lock();
        if let Some(region) = regions.get_mut(0) {
            region.in_use = true;
            region.free_pages = region.page_count / 2; // %50 boş
        }
    }
    
    crate::serial_println!("[Compaction] Simulated fragmentation");
    print_memory_map();
    
    // Compaction başlat
    start_manual_compaction(CompactionType::Full, CompactionPriority::Normal)?;
    
    // Sonucu bekle
    while engine.active.load(Ordering::SeqCst) {
        crate::task::scheduler::schedule();
    }
    
    // İstatistikleri göster
    let stats = get_compaction_stats();
    crate::serial_println!("[Compaction] Stats:");
    crate::serial_println!("  Pages scanned: {}", stats.pages_scanned);
    crate::serial_println!("  Pages compacted: {}", stats.pages_compacted);
    crate::serial_println!("  Pages migrated: {}", stats.pages_migrated);
    crate::serial_println!("  Large blocks: {}", stats.large_blocks_created);
    crate::serial_println!("  Duration: {} ms", stats.duration_ms);
    crate::serial_println!("  Success rate: {:.1}%", stats.success_rate);
    
    // Son bellek haritası
    print_memory_map();
    
    Ok(())
}

/// Background compaction thread
pub mod background {
    use super::*;
    
    /// Background compaction'ı başlat
    pub fn start_background_compaction() {
        crate::serial_println!("[Compaction] Starting background compaction thread");
        
        // Background thread implementasyonu (placeholder)
        // Gerçek implementasyonda periyodik compaction kontrolü
    }
    
    /// Background compaction'ı durdur
    pub fn stop_background_compaction() {
        crate::serial_println!("[Compaction] Stopping background compaction thread");
        get_compaction_engine().stop_compaction();
    }
}
