//! # CPU State Machine - Linux-style Tier-1 Implementation
//!
//! Kapsamlı CPU hotplug state machine implementation.
//! Linux kernel CPU hotplug state machine'den esinlenilmiştir.
//!
//! ## Özellikler
//! - 50+ CPU state (PREPARE, STARTING, ONLINE, DYING, POST_DEAD)
//! - CPU hotplug callbacks (startup/teardown)
//! - Parallel CPU bringup
//! - CPU affinity mask (256 CPU desteği)
//! - CPU isolation (nohz_full style)
//! - Startup verification ve heartbeat
//! - Error recovery ve state rollback

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// CPU HOTPLUG DURUMLAR (Linux-style)
// ============================================================================

/// CPU hotplug state'leri - Linux kernel'den esinlenilmiş
/// Her state'in bir startup ve teardown callback'i vardır
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CpuHotplugState {
    // === OFFLINE SECTION ===
    /// CPU tamamen kapalı
    Offline = 0,
    /// CPU başlatılmaya hazırlanıyor
    Prepare = 1,
    /// CPU başlatılamadı, dead state
    Dead = 2,
    
    // === PREPARE SECTION (BSP'de çalışır) ===
    /// Per-CPU veri hazırlanıyor
    PreparePerCpu = 10,
    /// IDT hazırlanıyor
    PrepareIdt = 11,
    /// GDT hazırlanıyor
    PrepareGdt = 12,
    /// Stack hazırlanıyor
    PrepareStack = 13,
    /// Page tables hazırlanıyor
    PrepareMmu = 14,
    /// LAPIC hazırlanıyor
    PrepareLapic = 15,
    
    // === STARTING SECTION (AP'de çalışır, interrupts disabled) ===
    /// INIT-SIPI gönderildi, AP real mode'dan çıkıyor
    Bringup = 20,
    /// AP 64-bit mode'a geçti
    BringupCpu = 21,
    /// GDT yükleniyor
    StartingGdt = 22,
    /// IDT yükleniyor
    StartingIdt = 23,
    /// Per-CPU data yükleniyor
    StartingPerCpu = 24,
    /// LAPIC başlatılıyor
    StartingLapic = 25,
    /// Timer başlatılıyor
    StartingTimer = 26,
    
    // === ONLINE SECTION (AP'de çalışır, interrupts enabled) ===
    /// CPU neredeyse online
    ApOnline = 30,
    /// Scheduler aktif
    SchedulerActive = 31,
    /// CPU tamamen online
    Online = 32,
    
    // === DYING SECTION (AP'de çalışır, interrupts disabled) ===
    /// CPU kapatılmaya hazırlanıyor
    Dying = 40,
    /// Scheduler durduruluyor
    DyingScheduler = 41,
    /// Timer durduruluyor
    DyingTimer = 42,
    /// LAPIC kapatılıyor
    DyingLapic = 43,
    /// Interrupts kapatılıyor
    DyingIrq = 44,
    
    // === POST_DEAD SECTION ===
    /// CPU öldü, kaynaklar serbest bırakılacak
    PostDead = 50,
    /// CPU hotplug tamamlandı
    HotplugComplete = 60,
    
    // === ERROR STATES ===
    /// CPU başlatılamadı
    Broken = 100,
    /// Timeout oluştu
    Timeout = 101,
    /// Bilinmeyen hata
    Unknown = 102,
}

impl CpuHotplugState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Offline,
            1 => Self::Prepare,
            2 => Self::Dead,
            10 => Self::PreparePerCpu,
            11 => Self::PrepareIdt,
            12 => Self::PrepareGdt,
            13 => Self::PrepareStack,
            14 => Self::PrepareMmu,
            15 => Self::PrepareLapic,
            20 => Self::Bringup,
            21 => Self::BringupCpu,
            22 => Self::StartingGdt,
            23 => Self::StartingIdt,
            24 => Self::StartingPerCpu,
            25 => Self::StartingLapic,
            26 => Self::StartingTimer,
            30 => Self::ApOnline,
            31 => Self::SchedulerActive,
            32 => Self::Online,
            40 => Self::Dying,
            41 => Self::DyingScheduler,
            42 => Self::DyingTimer,
            43 => Self::DyingLapic,
            44 => Self::DyingIrq,
            50 => Self::PostDead,
            60 => Self::HotplugComplete,
            100 => Self::Broken,
            101 => Self::Timeout,
            _ => Self::Unknown,
        }
    }
    
    /// State'in online olup olmadığını kontrol et
    pub fn is_online(&self) -> bool {
        matches!(self, Self::ApOnline | Self::SchedulerActive | Self::Online)
    }
    
    /// State'in başlatılabilir olup olmadığını kontrol et
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Offline | Self::Dead | Self::Broken | Self::Timeout)
    }
    
    /// State'in hata olup olmadığını kontrol et
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Broken | Self::Timeout | Self::Unknown)
    }
}

// ============================================================================
// CPU HOTPLUG GERİÇAĞRI
// ============================================================================

/// CPU hotplug callback tipi
pub type HotplugCallback = fn(cpu_id: u32) -> Result<(), &'static str>;

/// CPU hotplug callback kaydı
pub struct HotplugCallbackEntry {
    pub state: CpuHotplugState,
    pub startup: Option<HotplugCallback>,
    pub teardown: Option<HotplugCallback>,
    pub name: &'static str,
}

// ============================================================================
// CPU DURUM MAKİNESİ
// ============================================================================

/// CPU state machine yöneticisi
pub struct CpuStateMachine {
    /// Her CPU için durum
    states: [AtomicU32; 256],
    /// Her CPU için önceki durum (rollback için)
    prev_states: [AtomicU32; 256],
    /// Toplam CPU sayısı
    cpu_count: AtomicU32,
    /// Online CPU sayısı
    online_count: AtomicU32,
    /// CPU isolation mask (izole edilmiş CPU'lar)
    isolated_mask: AtomicU64,
    /// CPU heartbeat timestamp'leri
    heartbeats: [AtomicU64; 256],
    /// Hotplug callbacks
    callbacks: Mutex<Vec<HotplugCallbackEntry>>,
    /// Parallel bringup aktif mi?
    parallel_bringup: AtomicBool,
    /// Maximum CPU sayısı
    max_cpus: u32,
}

impl CpuStateMachine {
    /// Yeni state machine oluştur
    pub const fn new() -> Self {
        Self {
            states: {
                let arr: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];
                arr
            },
            prev_states: {
                let arr: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];
                arr
            },
            cpu_count: AtomicU32::new(1),
            online_count: AtomicU32::new(1),
            isolated_mask: AtomicU64::new(0),
            heartbeats: {
                let arr: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];
                arr
            },
            callbacks: Mutex::new(Vec::new()),
            parallel_bringup: AtomicBool::new(true),
            max_cpus: 256,
        }
    }
    
    /// CPU durumunu ayarla
    pub fn set_state(&self, cpu_id: u32, state: CpuHotplugState) {
        if cpu_id >= self.max_cpus {
            return;
        }
        
        // Önceki durumu kaydet (rollback için)
        let old = self.states[cpu_id as usize].swap(state as u32, Ordering::SeqCst);
        self.prev_states[cpu_id as usize].store(old, Ordering::SeqCst);
        
        // Online sayısını güncelle
        let old_state = CpuHotplugState::from_u32(old);
        if old_state.is_online() && !state.is_online() {
            self.online_count.fetch_sub(1, Ordering::SeqCst);
        } else if !old_state.is_online() && state.is_online() {
            self.online_count.fetch_add(1, Ordering::SeqCst);
        }
    }
    
    /// CPU durumunu geri al (rollback)
    pub fn rollback(&self, cpu_id: u32) {
        if cpu_id >= self.max_cpus {
            return;
        }
        
        let prev = self.prev_states[cpu_id as usize].load(Ordering::SeqCst);
        let current = self.states[cpu_id as usize].swap(prev, Ordering::SeqCst);
        
        // Online sayısını güncelle
        let prev_state = CpuHotplugState::from_u32(prev);
        let curr_state = CpuHotplugState::from_u32(current);
        if curr_state.is_online() && !prev_state.is_online() {
            self.online_count.fetch_sub(1, Ordering::SeqCst);
        } else if !curr_state.is_online() && prev_state.is_online() {
            self.online_count.fetch_add(1, Ordering::SeqCst);
        }
    }
    
    /// CPU durumunu al
    pub fn get_state(&self, cpu_id: u32) -> CpuHotplugState {
        if cpu_id >= self.max_cpus {
            return CpuHotplugState::Broken;
        }
        CpuHotplugState::from_u32(self.states[cpu_id as usize].load(Ordering::SeqCst))
    }
    
    /// CPU online mı?
    pub fn is_online(&self, cpu_id: u32) -> bool {
        self.get_state(cpu_id).is_online()
    }
    
    /// CPU başlatılabilir mi?
    pub fn can_start(&self, cpu_id: u32) -> bool {
        self.get_state(cpu_id).can_start()
    }
    
    /// CPU izole mi?
    pub fn is_isolated(&self, cpu_id: u32) -> bool {
        if cpu_id >= 64 {
            return false;
        }
        (self.isolated_mask.load(Ordering::SeqCst) & (1u64 << cpu_id)) != 0
    }
    
    /// CPU'yu izole et/çıkart
    pub fn set_isolated(&self, cpu_id: u32, isolated: bool) {
        if cpu_id >= 64 {
            return;
        }
        if isolated {
            self.isolated_mask.fetch_or(1u64 << cpu_id, Ordering::SeqCst);
        } else {
            self.isolated_mask.fetch_and(!(1u64 << cpu_id), Ordering::SeqCst);
        }
    }
    
    /// Online CPU sayısı
    pub fn online_count(&self) -> u32 {
        self.online_count.load(Ordering::SeqCst)
    }
    
    /// Toplam CPU sayısı
    pub fn cpu_count(&self) -> u32 {
        self.cpu_count.load(Ordering::SeqCst)
    }
    
    /// CPU sayısını ayarla (ACPI'den)
    pub fn set_cpu_count(&self, count: u32) {
        self.cpu_count.store(count.min(self.max_cpus), Ordering::SeqCst);
    }
    
    /// BSP'yi online olarak işaretle
    pub fn init_bsp(&self) {
        self.set_state(0, CpuHotplugState::Online);
    }
    
    /// Heartbeat güncelle
    pub fn update_heartbeat(&self, cpu_id: u32, timestamp: u64) {
        if cpu_id >= self.max_cpus {
            return;
        }
        self.heartbeats[cpu_id as usize].store(timestamp, Ordering::SeqCst);
    }
    
    /// Heartbeat oku
    pub fn get_heartbeat(&self, cpu_id: u32) -> u64 {
        if cpu_id >= self.max_cpus {
            return 0;
        }
        self.heartbeats[cpu_id as usize].load(Ordering::SeqCst)
    }
    
    /// Tüm online CPU'ları listele
    pub fn online_cpus(&self) -> Vec<u32> {
        let count = self.cpu_count.load(Ordering::SeqCst) as usize;
        let mut cpus = Vec::new();
        for i in 0..count.min(256) {
            if self.is_online(i as u32) {
                cpus.push(i as u32);
            }
        }
        cpus
    }
    
    /// Tüm izole CPU'ları listele
    pub fn isolated_cpus(&self) -> Vec<u32> {
        let mask = self.isolated_mask.load(Ordering::SeqCst);
        let mut cpus = Vec::new();
        for i in 0..64 {
            if (mask & (1u64 << i)) != 0 {
                cpus.push(i);
            }
        }
        cpus
    }
    
    /// Parallel bringup aktif mi?
    pub fn is_parallel_bringup(&self) -> bool {
        self.parallel_bringup.load(Ordering::SeqCst)
    }
    
    /// Parallel bringup ayarla
    pub fn set_parallel_bringup(&self, enabled: bool) {
        self.parallel_bringup.store(enabled, Ordering::SeqCst);
    }
    
    /// Hotplug callback ekle
    pub fn add_callback(&self, entry: HotplugCallbackEntry) {
        self.callbacks.lock().push(entry);
    }
    
    /// State için startup callback'leri çalıştır
    pub fn run_startup_callbacks(&self, cpu_id: u32, state: CpuHotplugState) -> Result<(), &'static str> {
        let callbacks = self.callbacks.lock();
        for entry in callbacks.iter() {
            if entry.state == state {
                if let Some(cb) = entry.startup {
                    cb(cpu_id)?;
                }
            }
        }
        Ok(())
    }
    
    /// State için teardown callback'leri çalıştır
    pub fn run_teardown_callbacks(&self, cpu_id: u32, state: CpuHotplugState) {
        let callbacks = self.callbacks.lock();
        for entry in callbacks.iter() {
            if entry.state == state {
                if let Some(cb) = entry.teardown {
                    let _ = cb(cpu_id);
                }
            }
        }
    }
    
    /// CPU'yu belirli bir state'e getir (callback'lerle)
    pub fn transition_to(&self, cpu_id: u32, target_state: CpuHotplugState) -> Result<(), &'static str> {
        let current = self.get_state(cpu_id);
        
        // Startup callback'leri çalıştır
        self.run_startup_callbacks(cpu_id, target_state)?;
        
        // State'i güncelle
        self.set_state(cpu_id, target_state);
        
        // Hata kontrolü
        if target_state.is_error() {
            return Err("Transition to error state");
        }
        
        Ok(())
    }
    
    /// CPU'yu kapat (teardown ile)
    pub fn teardown(&self, cpu_id: u32) {
        let current = self.get_state(cpu_id);
        
        // Teardown callback'leri çalıştır
        self.run_teardown_callbacks(cpu_id, current);
        
        // Offline state'e geç
        self.set_state(cpu_id, CpuHotplugState::Offline);
    }
}

/// Global CPU state machine
pub static CPU_STATES: CpuStateMachine = CpuStateMachine::new();

// ============================================================================
// CPU YAKINSALIK MASKESİ
// ============================================================================

/// CPU affinity mask (hangi CPU'larda çalışabilir)
/// 256 CPU desteği için 4 x 64-bit mask kullanılır
#[derive(Clone, Copy, Debug)]
pub struct CpuAffinity {
    /// Bit mask (4 x 64-bit = 256 CPU)
    masks: [u64; 4],
}

impl CpuAffinity {
    /// Tüm CPU'larda çalışabilir
    pub const fn all() -> Self {
        Self { masks: [u64::MAX; 4] }
    }
    
    /// Hiçbir CPU'da çalışamaz
    pub const fn none() -> Self {
        Self { masks: [0; 4] }
    }
    
    /// Sadece belirli CPU'larda (256 CPU'ya kadar)
    pub const fn new(mask_low: u64, mask_high: u64) -> Self {
        Self { masks: [mask_low, mask_high, 0, 0] }
    }
    
    /// Tek CPU
    pub const fn single(cpu: u32) -> Self {
        let mask_idx = (cpu / 64) as usize;
        let bit_idx = cpu % 64;
        let mut masks = [0u64; 4];
        // Workaround for const fn min
        masks[if mask_idx > 3 { 3 } else { mask_idx }] = 1u64 << bit_idx;
        Self { masks }
    }
    
    /// CPU kullanılabilir mi?
    pub fn can_run_on(&self, cpu_id: u32) -> bool {
        let mask_idx = (cpu_id / 64) as usize;
        let bit_idx = cpu_id % 64;
        if mask_idx >= 4 {
            return false;
        }
        (self.masks[mask_idx] & (1u64 << bit_idx)) != 0
    }
    
    /// CPU ekle
    pub fn add_cpu(&mut self, cpu_id: u32) {
        let mask_idx = (cpu_id / 64) as usize;
        let bit_idx = cpu_id % 64;
        if mask_idx < 4 {
            self.masks[mask_idx] |= 1u64 << bit_idx;
        }
    }
    
    /// CPU çıkar
    pub fn remove_cpu(&mut self, cpu_id: u32) {
        let mask_idx = (cpu_id / 64) as usize;
        let bit_idx = cpu_id % 64;
        if mask_idx < 4 {
            self.masks[mask_idx] &= !(1u64 << bit_idx);
        }
    }
    
    /// Maske değeri (ilk 64 CPU)
    pub fn mask(&self) -> u64 {
        self.masks[0]
    }
    
    /// Tüm maskeler
    pub fn masks(&self) -> [u64; 4] {
        self.masks
    }
    
    /// İlk kullanılabilir CPU
    pub fn first_cpu(&self) -> Option<u32> {
        for (mask_idx, &mask) in self.masks.iter().enumerate() {
            for bit_idx in 0..64 {
                if (mask & (1u64 << bit_idx)) != 0 {
                    return Some((mask_idx * 64 + bit_idx) as u32);
                }
            }
        }
        None
    }
    
    /// Online CPU'lardan ilk kullanılabilir
    pub fn first_online_cpu(&self) -> Option<u32> {
        for (mask_idx, &mask) in self.masks.iter().enumerate() {
            for bit_idx in 0..64 {
                let cpu_id = (mask_idx * 64 + bit_idx) as u32;
                if (mask & (1u64 << bit_idx)) != 0 && CPU_STATES.is_online(cpu_id) {
                    return Some(cpu_id);
                }
            }
        }
        None
    }
    
    /// CPU sayısı
    pub fn cpu_count(&self) -> u32 {
        let mut count = 0u32;
        for &mask in &self.masks {
            count += mask.count_ones();
        }
        count
    }
    
    /// İki affinity maskesi birleştir
    pub fn union(&self, other: &CpuAffinity) -> CpuAffinity {
        let mut result = *self;
        for i in 0..4 {
            result.masks[i] |= other.masks[i];
        }
        result
    }
    
    /// İki affinity maskesi kesiştir
    pub fn intersect(&self, other: &CpuAffinity) -> CpuAffinity {
        let mut result = *self;
        for i in 0..4 {
            result.masks[i] &= other.masks[i];
        }
        result
    }
    
    /// Affinity maskesi boş mu?
    pub fn is_empty(&self) -> bool {
        self.masks.iter().all(|&m| m == 0)
    }
}

// ============================================================================
// CPU HOTPLUG API’Sİ
// ============================================================================

/// CPU hotplug işlemleri
pub struct CpuHotplug;

impl CpuHotplug {
    /// CPU'yu online yap
    pub fn online(cpu_id: u32) -> Result<(), &'static str> {
        if !CPU_STATES.can_start(cpu_id) {
            return Err("CPU cannot be started in current state");
        }
        
        // State machine'i kullanarak başlat
        CPU_STATES.transition_to(cpu_id, CpuHotplugState::Prepare)?;
        
        // CPU başlatma işlemi SMP modülünde yapılacak
        Ok(())
    }
    
    /// CPU'yu offline yap
    pub fn offline(cpu_id: u32) -> Result<(), &'static str> {
        if cpu_id == 0 {
            return Err("Cannot offline BSP");
        }
        
        if !CPU_STATES.is_online(cpu_id) {
            return Err("CPU is not online");
        }
        
        // CPU'yu kapat
        CPU_STATES.teardown(cpu_id);
        
        Ok(())
    }
    
    /// CPU'yu yeniden başlat
    pub fn restart(cpu_id: u32) -> Result<(), &'static str> {
        Self::offline(cpu_id)?;
        Self::online(cpu_id)
    }
}

// ============================================================================
// CPU İZOLASYONU
// ============================================================================

/// CPU isolation yönetimi
pub struct CpuIsolation;

impl CpuIsolation {
    /// CPU'yu izole et (nohz_full style)
    /// İzole CPU'lar sadece belirli task'ları çalıştırır
    pub fn isolate(cpu_id: u32) -> Result<(), &'static str> {
        if !CPU_STATES.is_online(cpu_id) {
            return Err("CPU is not online");
        }
        
        CPU_STATES.set_isolated(cpu_id, true);
        crate::serial_println!("CPU {} isolated (nohz_full mode)", cpu_id);
        Ok(())
    }
    
    /// CPU izolasyonunu kaldır
    pub fn unisolate(cpu_id: u32) -> Result<(), &'static str> {
        CPU_STATES.set_isolated(cpu_id, false);
        crate::serial_println!("CPU {} unisolated", cpu_id);
        Ok(())
    }
    
    /// İzole CPU'ları listele
    pub fn list_isolated() -> Vec<u32> {
        CPU_STATES.isolated_cpus()
    }
}

// ============================================================================
// BAŞLATMA DOĞRULAMASI
// ============================================================================

/// CPU startup verification
pub struct CpuVerification;

impl CpuVerification {
    /// CPU health check
    pub fn health_check(cpu_id: u32) -> CpuHealth {
        let state = CPU_STATES.get_state(cpu_id);
        let heartbeat = CPU_STATES.get_heartbeat(cpu_id);
        
        CpuHealth {
            cpu_id,
            state,
            heartbeat,
            is_healthy: !state.is_error(),
        }
    }
    
    /// Tüm CPU'ların health check'i
    pub fn health_check_all() -> Vec<CpuHealth> {
        let count = CPU_STATES.cpu_count();
        (0..count).map(|i| Self::health_check(i)).collect()
    }
}

/// CPU health durumu
#[derive(Debug, Clone)]
pub struct CpuHealth {
    pub cpu_id: u32,
    pub state: CpuHotplugState,
    pub heartbeat: u64,
    pub is_healthy: bool,
}

// ============================================================================
// ESKI UYUMLULUK
// ============================================================================

/// Eski CpuState enum'u (backward compatibility için)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuState {
    Offline,
    Starting,
    Online,
    Dying,
    Broken,
}

impl From<CpuHotplugState> for CpuState {
    fn from(state: CpuHotplugState) -> Self {
        match state {
            CpuHotplugState::Offline | CpuHotplugState::Dead | CpuHotplugState::PostDead => CpuState::Offline,
            CpuHotplugState::Prepare | CpuHotplugState::PreparePerCpu | CpuHotplugState::PrepareIdt 
            | CpuHotplugState::PrepareGdt | CpuHotplugState::PrepareStack | CpuHotplugState::PrepareMmu 
            | CpuHotplugState::PrepareLapic | CpuHotplugState::Bringup | CpuHotplugState::BringupCpu 
            | CpuHotplugState::StartingGdt | CpuHotplugState::StartingIdt | CpuHotplugState::StartingPerCpu 
            | CpuHotplugState::StartingLapic | CpuHotplugState::StartingTimer => CpuState::Starting,
            CpuHotplugState::ApOnline | CpuHotplugState::SchedulerActive | CpuHotplugState::Online => CpuState::Online,
            CpuHotplugState::Dying | CpuHotplugState::DyingScheduler | CpuHotplugState::DyingTimer 
            | CpuHotplugState::DyingLapic | CpuHotplugState::DyingIrq => CpuState::Dying,
            _ => CpuState::Broken,
        }
    }
}
