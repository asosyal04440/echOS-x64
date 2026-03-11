//! # perf — Donanım Performans Sayaçları (PMU)
//!
//! x86_64 Performance Monitoring Unit (PMU) ile donanım düzeyinde
//! performans profilleme. CPU döngüleri, talimat sayısı, önbellek
//! kaçırmaları, dal öngörü hataları gibi metrikleri ölçer.
//!
//! ## Desteklenen PMC'ler (Performance Monitoring Counters)
//!
//! ```text
//! Mimarisel Sayaçlar (IA32_PERFEVTSELx / IA32_PMCx):
//!   - CPU_CYCLES              (0x003C)
//!   - INSTRUCTIONS_RETIRED    (0x00C0)
//!   - LLC_MISSES              (0x412E)
//!   - BRANCH_MISSES           (0x00C5)
//!   - CACHE_REFERENCES        (0x4F2E)
//!   - BUS_CYCLES              (0x013C)
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// MSR ADRESLERİ
// ============================================================================

/// IA32_PERF_GLOBAL_CTRL — Tüm sayaçları etkinleştir/devre dışı bırak
pub const MSR_PERF_GLOBAL_CTRL: u32 = 0x38F;
/// IA32_PERF_GLOBAL_STATUS — Taşma durumu
pub const MSR_PERF_GLOBAL_STATUS: u32 = 0x38E;
/// IA32_PERF_GLOBAL_OVF_CTRL — Taşma temizleme
pub const MSR_PERF_GLOBAL_OVF_CTRL: u32 = 0x390;
/// IA32_FIXED_CTR_CTRL — Sabit sayaç kontrolü
pub const MSR_FIXED_CTR_CTRL: u32 = 0x38D;

/// IA32_PERFEVTSELx — Olay seçim MSR'leri (4 adet)
pub const MSR_PERFEVTSEL0: u32 = 0x186;
pub const MSR_PERFEVTSEL1: u32 = 0x187;
pub const MSR_PERFEVTSEL2: u32 = 0x188;
pub const MSR_PERFEVTSEL3: u32 = 0x189;

/// IA32_PMCx — Performans sayaç MSR'leri (4 adet)
pub const MSR_PMC0: u32 = 0x0C1;
pub const MSR_PMC1: u32 = 0x0C2;
pub const MSR_PMC2: u32 = 0x0C3;
pub const MSR_PMC3: u32 = 0x0C4;

/// IA32_FIXED_CTRx — Sabit sayaçlar (3 adet)
pub const MSR_FIXED_CTR0: u32 = 0x309; // INST_RETIRED.ANY
pub const MSR_FIXED_CTR1: u32 = 0x30A; // CPU_CLK_UNHALTED.THREAD
pub const MSR_FIXED_CTR2: u32 = 0x30B; // CPU_CLK_UNHALTED.REF

// ============================================================================
// PERFORMANS OLAYLARI
// ============================================================================

/// Önceden tanımlı performans olayları (Event + UMask).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerfEvent {
    /// CPU döngüleri (UnHalted Core Cycles)
    CpuCycles,
    /// Emekli talimat sayısı
    Instructions,
    /// Son seviye önbellek kaçırması (LLC miss)
    LlcMisses,
    /// Son seviye önbellek erişimi (LLC reference)
    CacheReferences,
    /// Dal öngörü hatası (branch misprediction)
    BranchMisses,
    /// Bus döngüleri
    BusCycles,
    /// L1 veri önbellek kaçırması
    L1dMisses,
    /// L1 talimat önbellek kaçırması
    L1iMisses,
    /// dTLB kaçırması
    DtlbMisses,
    /// iTLB kaçırması
    ItlbMisses,
}

impl PerfEvent {
    /// Event+UMask kodunu döner (PERFEVTSELx formatı).
    pub fn event_code(&self) -> u64 {
        match self {
            PerfEvent::CpuCycles => 0x003C,
            PerfEvent::Instructions => 0x00C0,
            PerfEvent::LlcMisses => 0x412E,
            PerfEvent::CacheReferences => 0x4F2E,
            PerfEvent::BranchMisses => 0x00C5,
            PerfEvent::BusCycles => 0x013C,
            PerfEvent::L1dMisses => 0x0151,
            PerfEvent::L1iMisses => 0x0283,
            PerfEvent::DtlbMisses => 0x0849,
            PerfEvent::ItlbMisses => 0x0185,
        }
    }

    /// Olay adını döner.
    pub fn name(&self) -> &'static str {
        match self {
            PerfEvent::CpuCycles => "cpu-cycles",
            PerfEvent::Instructions => "instructions",
            PerfEvent::LlcMisses => "LLC-misses",
            PerfEvent::CacheReferences => "cache-references",
            PerfEvent::BranchMisses => "branch-misses",
            PerfEvent::BusCycles => "bus-cycles",
            PerfEvent::L1dMisses => "L1-dcache-misses",
            PerfEvent::L1iMisses => "L1-icache-misses",
            PerfEvent::DtlbMisses => "dTLB-misses",
            PerfEvent::ItlbMisses => "iTLB-misses",
        }
    }

    /// PERFEVTSELx register değerini döner.
    ///
    /// Bit alanları:
    ///   [7:0]   Event Select
    ///   [15:8]  UMask
    ///   [16]    USR (kullanıcı modu sayımı)
    ///   [17]    OS (çekirdek modu sayımı)
    ///   [22]    EN (sayacı etkinleştir)
    pub fn perfevtsel_value(&self) -> u64 {
        let event = self.event_code();
        event | (1 << 16) | (1 << 17) | (1 << 22) // USR + OS + EN
    }
}

// ============================================================================
// PMU Okuma/Yazma (MSR erişimi)
// ============================================================================

/// MSR okur (RDMSR).
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
    );
    (high as u64) << 32 | low as u64
}

/// MSR yazar (WRMSR).
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
    );
}

// ============================================================================
// Perf Sayaç Okuma
// ============================================================================

/// Performans sayacını yapılandırır ve başlatır.
///
/// `counter_idx`: Kullanılacak PMC indeksi (0-3)
/// `event`: İzlenecek olay
pub fn setup_counter(counter_idx: u32, event: PerfEvent) {
    if counter_idx > 3 {
        return;
    }

    unsafe {
        // Olay seçim register'ını ayarla
        let sel_msr = MSR_PERFEVTSEL0 + counter_idx;
        wrmsr(sel_msr, event.perfevtsel_value());

        // Sayacı sıfırla
        let pmc_msr = MSR_PMC0 + counter_idx;
        wrmsr(pmc_msr, 0);

        // Global kontrol: sayacı etkinleştir
        let global = rdmsr(MSR_PERF_GLOBAL_CTRL);
        wrmsr(MSR_PERF_GLOBAL_CTRL, global | (1 << counter_idx));
    }
}

/// Performans sayacını okur.
pub fn read_counter(counter_idx: u32) -> u64 {
    if counter_idx > 3 {
        return 0;
    }
    unsafe { rdmsr(MSR_PMC0 + counter_idx) }
}

/// Performans sayacını durdurur.
pub fn stop_counter(counter_idx: u32) {
    if counter_idx > 3 {
        return;
    }
    unsafe {
        let global = rdmsr(MSR_PERF_GLOBAL_CTRL);
        wrmsr(MSR_PERF_GLOBAL_CTRL, global & !(1 << counter_idx));
    }
}

// ============================================================================
// Profil Oturumu
// ============================================================================

/// Profilleme oturumu — birden fazla olayı eş zamanlı izler.
#[derive(Debug, Clone)]
pub struct PerfSession {
    /// Oturum ID
    pub id: u64,
    /// İzlenen olaylar ve sonuçları
    pub counters: Vec<PerfCounter>,
    /// Başlangıç TSC
    pub start_tsc: u64,
    /// Bitiş TSC
    pub end_tsc: u64,
    /// Aktif mi
    pub active: bool,
    /// Açıklama
    pub label: String,
}

/// Tek bir perf sayacı.
#[derive(Debug, Clone)]
pub struct PerfCounter {
    /// Olay tipi
    pub event: PerfEvent,
    /// PMC indeksi (0-3)
    pub counter_idx: u32,
    /// Başlangıç değeri
    pub start_value: u64,
    /// Bitiş değeri
    pub end_value: u64,
}

impl PerfCounter {
    /// Delta (ölçülen değer).
    pub fn delta(&self) -> u64 {
        self.end_value.saturating_sub(self.start_value)
    }
}

impl PerfSession {
    /// Yeni profilleme oturumu başlatır.
    pub fn start(label: &str, events: &[PerfEvent]) -> Self {
        let mut counters = Vec::new();

        for (idx, event) in events.iter().take(4).enumerate() {
            setup_counter(idx as u32, *event);
            let start_value = read_counter(idx as u32);
            counters.push(PerfCounter {
                event: *event,
                counter_idx: idx as u32,
                start_value,
                end_value: 0,
            });
        }

        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);

        Self {
            id,
            counters,
            start_tsc: tsc,
            end_tsc: 0,
            active: true,
            label: String::from(label),
        }
    }

    /// Oturumu sonlandırır ve sonuçları toplar.
    pub fn stop(&mut self) {
        self.end_tsc = unsafe { core::arch::x86_64::_rdtsc() };
        for counter in &mut self.counters {
            counter.end_value = read_counter(counter.counter_idx);
            stop_counter(counter.counter_idx);
        }
        self.active = false;
    }

    /// IPC (Instructions Per Cycle) hesaplar.
    pub fn ipc(&self) -> f64 {
        let cycles = self
            .counters
            .iter()
            .find(|c| matches!(c.event, PerfEvent::CpuCycles))
            .map(|c| c.delta())
            .unwrap_or(1);
        let instructions = self
            .counters
            .iter()
            .find(|c| matches!(c.event, PerfEvent::Instructions))
            .map(|c| c.delta())
            .unwrap_or(0);

        if cycles == 0 {
            0.0
        } else {
            instructions as f64 / cycles as f64
        }
    }

    /// Sonuç özetini döner.
    pub fn summary(&self) -> Vec<(String, u64)> {
        self.counters
            .iter()
            .map(|c| (String::from(c.event.name()), c.delta()))
            .collect()
    }

    /// Geçen süre (TSC tick).
    pub fn elapsed_tsc(&self) -> u64 {
        self.end_tsc.saturating_sub(self.start_tsc)
    }
}

// ============================================================================
// perf stat — Hızlı İstatistik
// ============================================================================

/// `perf stat` benzeri hızlı ölçüm.
///
/// Verilen fonksiyonu çalıştırır ve CPU döngüleri, talimat sayısı,
/// LLC kaçırma ve dal öngörü hatasını ölçer.
pub fn perf_stat<F: FnOnce()>(label: &str, f: F) -> PerfSession {
    let events = [
        PerfEvent::CpuCycles,
        PerfEvent::Instructions,
        PerfEvent::LlcMisses,
        PerfEvent::BranchMisses,
    ];

    let mut session = PerfSession::start(label, &events);
    f();
    session.stop();

    // Sonuçları seri porta yaz
    crate::serial_println!("── perf stat: {} ──", label);
    for (name, value) in session.summary() {
        crate::serial_println!("  {:>20}: {:>15}", name, value);
    }
    crate::serial_println!("  {:>20}: {:.2}", "IPC", session.ipc());
    crate::serial_println!("  {:>20}: {} ticks", "elapsed", session.elapsed_tsc());

    session
}

// ============================================================================
// Global State
// ============================================================================

lazy_static::lazy_static! {
    /// Tamamlanmış oturumlar
    static ref PERF_SESSIONS: Mutex<Vec<PerfSession>> = Mutex::new(Vec::new());
    /// Sonraki oturum ID
    static ref NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
}

/// PMU destekleniyor mu kontrol eder (CPUID ile).
pub fn pmu_supported() -> bool {
    // CPUID.0AH: Architectural Performance Monitoring
    let result = unsafe { core::arch::x86_64::__cpuid(0x0A) };
    let version_id = result.eax & 0xFF;
    version_id >= 1
}

/// Desteklenen sayaç sayısını döner.
pub fn num_counters() -> u32 {
    let result = unsafe { core::arch::x86_64::__cpuid(0x0A) };
    (result.eax >> 8) & 0xFF
}

/// Modülü başlatır.
pub fn init() {
    let supported = pmu_supported();
    let counters = if supported { num_counters() } else { 0 };
    crate::serial_println!(
        "[perf] PMU: {}, {} genel sayaç",
        if supported {
            "destekleniyor"
        } else {
            "desteklenmiyor"
        },
        counters
    );
}
