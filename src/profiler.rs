//! # Kernel Profiler — PMC Tabanlı Performans Örnekleyici
//!
//! x86_64 Performance Monitoring Counter (PMC) donanımını kullanarak
//! kernel fonksiyonlarının CPU kullanımını örnekler.
//!
//! ## PMC Mimarisi (Intel/AMD)
//!
//! ```text
//! IA32_PERFEVTSELx (MSR 0x186 + x): Olay seçici
//!   ├── bits [7:0]:   Event Select (hangi olay)
//!   ├── bits [15:8]:  Unit Mask (alt olay filtresi)
//!   ├── bit  16:      USR (kullanıcı modunda say)
//!   ├── bit  17:      OS  (kernel modunda say)
//!   ├── bit  20:      INT (overflow → interrupt)
//!   ├── bit  22:      EN  (sayacı etkinleştir)
//!   └── bits [31:24]: Counter Mask
//!
//! IA32_PMCx (MSR 0xC1 + x): Sayaç değeri (48-bit)
//! ```
//!
//! ## Örnekleme Modu
//!
//! PMC overflow → NMI → RIP yakalama → histogram güncelleme
//!
//! Bu yöntem, Linux `perf record` ile aynı prensiple çalışır.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// PMC MSR Adresleri
// ============================================================================

/// Performans olay seçici MSR'leri (IA32_PERFEVTSELx)
const MSR_PERFEVTSEL0: u32 = 0x186;
const MSR_PERFEVTSEL1: u32 = 0x187;
const MSR_PERFEVTSEL2: u32 = 0x188;
const MSR_PERFEVTSEL3: u32 = 0x189;

/// Performans sayacı MSR'leri (IA32_PMCx)
const MSR_PMC0: u32 = 0x0C1;
const MSR_PMC1: u32 = 0x0C2;
const MSR_PMC2: u32 = 0x0C3;
const MSR_PMC3: u32 = 0x0C4;

/// Sabit fonksiyon sayaçları (Intel only)
const MSR_FIXED_CTR0: u32 = 0x309; // Instructions retired
const MSR_FIXED_CTR1: u32 = 0x30A; // CPU cycles (unhalted)
const MSR_FIXED_CTR2: u32 = 0x30B; // Reference cycles
const MSR_FIXED_CTR_CTRL: u32 = 0x38D;
const MSR_PERF_GLOBAL_CTRL: u32 = 0x38F;
const MSR_PERF_GLOBAL_STATUS: u32 = 0x38E;

// PERFEVTSEL bit pozisyonları
const PERFEVTSEL_USR: u64 = 1 << 16; // Kullanıcı modunda say
const PERFEVTSEL_OS: u64 = 1 << 17; // Kernel modunda say
const PERFEVTSEL_INT: u64 = 1 << 20; // Overflow → interrupt
const PERFEVTSEL_EN: u64 = 1 << 22; // Sayacı etkinleştir

// ============================================================================
// Yaygın Performans Olayları
// ============================================================================

/// Profil olayı tanımı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfEvent {
    /// Olay numarası (Event Select)
    pub event: u8,
    /// Alt olay maskesi (Unit Mask)
    pub umask: u8,
    /// İnsan okunabilir isim
    pub name: &'static str,
}

/// Yaygın x86_64 performans olayları
pub mod events {
    use super::PerfEvent;

    /// CPU döngüsü (unhalted)
    pub const CPU_CYCLES: PerfEvent = PerfEvent {
        event: 0x3C,
        umask: 0x00,
        name: "cpu-cycles",
    };
    /// Çalıştırılan talimat sayısı
    pub const INSTRUCTIONS: PerfEvent = PerfEvent {
        event: 0xC0,
        umask: 0x00,
        name: "instructions",
    };
    /// L1 data cache miss
    pub const L1D_CACHE_MISS: PerfEvent = PerfEvent {
        event: 0x51,
        umask: 0x01,
        name: "L1-dcache-miss",
    };
    /// LLC (Last Level Cache) miss
    pub const LLC_MISS: PerfEvent = PerfEvent {
        event: 0x2E,
        umask: 0x41,
        name: "LLC-miss",
    };
    /// Branch misprediction
    pub const BRANCH_MISS: PerfEvent = PerfEvent {
        event: 0xC5,
        umask: 0x00,
        name: "branch-miss",
    };
    /// TLB miss (dTLB)
    pub const DTLB_MISS: PerfEvent = PerfEvent {
        event: 0x08,
        umask: 0x01,
        name: "dTLB-miss",
    };
}

// ============================================================================
// Profil Örneği (Sample)
// ============================================================================

/// Tek bir profil örneği: NMI anında yakalanan RIP + bilgiler
#[derive(Clone, Copy, Debug)]
pub struct ProfileSample {
    /// Instruction Pointer (yakalanan fonksiyon adresi)
    pub rip: u64,
    /// Örneklendiğindeki CPU ID
    pub cpu_id: u32,
    /// Zaman damgası (TSC)
    pub timestamp: u64,
    /// PID (process ID, kernel=0)
    pub pid: u32,
}

// ============================================================================
// Profiler Ana Yapısı
// ============================================================================

/// Profiler durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilerState {
    Stopped,
    Running,
    Paused,
}

/// Kernel profiler — PMC tabanlı örnekleyici
pub struct Profiler {
    /// Aktif olay
    pub event: PerfEvent,
    /// Örnekleme periyodu (kaç olayda bir NMI?)
    pub sample_period: u64,
    /// Durum
    state: AtomicU32,
    /// Toplam örnek sayısı
    pub total_samples: AtomicU64,
    /// Taşma sayısı (ring buffer dolu)
    pub overflow_count: AtomicU64,
}

impl Profiler {
    pub fn new(event: PerfEvent, sample_period: u64) -> Self {
        Self {
            event,
            sample_period,
            state: AtomicU32::new(ProfilerState::Stopped as u32),
            total_samples: AtomicU64::new(0),
            overflow_count: AtomicU64::new(0),
        }
    }

    pub fn state(&self) -> ProfilerState {
        match self.state.load(Ordering::Acquire) {
            0 => ProfilerState::Stopped,
            1 => ProfilerState::Running,
            2 => ProfilerState::Paused,
            _ => ProfilerState::Stopped,
        }
    }
}

// ============================================================================
// Global Profiler State
// ============================================================================

lazy_static::lazy_static! {
    /// Global profiler instance
    static ref PROFILER: Mutex<Profiler> = Mutex::new(
        Profiler::new(events::CPU_CYCLES, 1_000_000)
    );

    /// RIP histogram: adres → örnekleme sayısı
    static ref RIP_HISTOGRAM: Mutex<BTreeMap<u64, u64>> = Mutex::new(BTreeMap::new());

    /// Son N örnek (ring buffer)
    static ref SAMPLE_BUFFER: Mutex<Vec<ProfileSample>> = Mutex::new(Vec::new());
}

const MAX_SAMPLES: usize = 8192;

/// Profil örneklemesini başlatır.
///
/// PMC0'ı yapılandırır ve overflow NMI'ı etkinleştirir.
pub fn start(event: PerfEvent, sample_period: u64) {
    let mut profiler = PROFILER.lock();
    profiler.event = event;
    profiler.sample_period = sample_period;
    profiler.total_samples.store(0, Ordering::Relaxed);
    profiler.overflow_count.store(0, Ordering::Relaxed);

    unsafe {
        // PMC0'ı yapılandır
        let evtsel = (event.event as u64)
            | ((event.umask as u64) << 8)
            | PERFEVTSEL_OS       // Kernel modunda say
            | PERFEVTSEL_USR      // User modunda da say
            | PERFEVTSEL_INT      // Overflow → NMI
            | PERFEVTSEL_EN; // Sayacı aç

        wrmsr(MSR_PERFEVTSEL0, evtsel);

        // Sayacı başlangıç değerine ayarla (negatif period → taşma için)
        // 48-bit counter, overflow threshold = MAX - period
        let initial = (-(sample_period as i64)) as u64;
        wrmsr(MSR_PMC0, initial);
    }

    profiler
        .state
        .store(ProfilerState::Running as u32, Ordering::Release);

    crate::serial_println!(
        "[Profiler] Started: event='{}' period={} (overflow NMI)",
        event.name,
        sample_period
    );
}

/// Profil örneklemesini durdurur.
pub fn stop() {
    let profiler = PROFILER.lock();

    unsafe {
        // PMC0'ı devre dışı bırak
        wrmsr(MSR_PERFEVTSEL0, 0);
    }

    profiler
        .state
        .store(ProfilerState::Stopped as u32, Ordering::Release);

    crate::serial_println!(
        "[Profiler] Stopped: {} samples collected",
        profiler.total_samples.load(Ordering::Relaxed)
    );
}

/// NMI handler'ından çağrılır: RIP'i örnekler.
///
/// Bu fonksiyon interrupt context'te çalışır — kısa olmalı!
pub fn record_sample(rip: u64, cpu_id: u32) {
    let profiler = PROFILER.lock();
    if profiler.state.load(Ordering::Relaxed) != ProfilerState::Running as u32 {
        return;
    }

    profiler.total_samples.fetch_add(1, Ordering::Relaxed);

    // RIP histogramı güncelle
    {
        let mut hist = RIP_HISTOGRAM.lock();
        *hist.entry(rip).or_insert(0) += 1;
    }

    // Örneği ring buffer'a yaz
    {
        let mut samples = SAMPLE_BUFFER.lock();
        if samples.len() >= MAX_SAMPLES {
            profiler.overflow_count.fetch_add(1, Ordering::Relaxed);
            samples.remove(0); // FIFO
        }
        samples.push(ProfileSample {
            rip,
            cpu_id,
            timestamp: unsafe { core::arch::x86_64::_rdtsc() },
            pid: 0, // Kernel context
        });
    }

    // Sayacı yeniden yükle (sonraki overflow için)
    unsafe {
        let initial = (-(profiler.sample_period as i64)) as u64;
        wrmsr(MSR_PMC0, initial);
    }
}

/// En çok örneklenen N fonksiyonu (hot spot) döner.
pub fn top_hotspots(n: usize) -> Vec<(u64, u64)> {
    let hist = RIP_HISTOGRAM.lock();
    let mut sorted: Vec<(u64, u64)> = hist.iter().map(|(&k, &v)| (k, v)).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // Azalan sıra
    sorted.truncate(n);
    sorted
}

/// Profil sonuçlarını seri porta yazdırır.
pub fn print_report(top_n: usize) {
    let profiler = PROFILER.lock();
    crate::serial_println!("=== Profiler Report ===");
    crate::serial_println!(
        "Event: {} | Samples: {} | Overflows: {}",
        profiler.event.name,
        profiler.total_samples.load(Ordering::Relaxed),
        profiler.overflow_count.load(Ordering::Relaxed),
    );

    drop(profiler);

    let hotspots = top_hotspots(top_n);
    crate::serial_println!("Top {} hot spots:", hotspots.len());
    for (i, (rip, count)) in hotspots.iter().enumerate() {
        crate::serial_println!("  #{}: RIP={:#018x}  samples={}", i + 1, rip, count);
    }
    crate::serial_println!("=======================");
}

// ============================================================================
// MSR Helpers
// ============================================================================

/// MSR yazma (WRMSR)
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
    );
}

/// MSR okuma (RDMSR)
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
    );
    (high as u64) << 32 | low as u64
}

/// Profiler alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[Profiler] PMC-based kernel profiler initialized");
    crate::serial_println!("[Profiler]   Events: cpu-cycles, instructions, L1-dcache-miss, LLC-miss, branch-miss, dTLB-miss");
    crate::serial_println!("[Profiler]   Mode: NMI-based sampling (overflow → record RIP)");
    crate::serial_println!("[Profiler]   Max samples: {}", MAX_SAMPLES);
}
