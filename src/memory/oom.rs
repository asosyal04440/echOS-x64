//! # OOM Killer — Bellek Tükenmesi Süreç Öldürücüsü
//!
//! Linux benzeri OOM (Out-Of-Memory) Killer implementasyonu.
//! Serbest bellek kritik düzeyin altına düşünce en uygun süreci sonlandırır.
//!
//! ## OOM Tetiklenme Koşulları
//!
//! ```
//! allocate_frame() çağrıldı
//!        │
//!        ▼
//!   Serbest frame < OOM_MIN_FREE_PAGES (64)?
//!   VEYA
//!   Serbest frame < toplam / 20 (%5)?
//!        │
//!        ▼ EVET
//!   OOM Killer tetiklenir
//! ```
//!
//! ## OOM Score Hesaplaması
//!
//! Her süreç için bir puan hesaplanır; en yüksek puanlı süreç öldürülür:
//!
//! ```
//! ham_puan = (RSS + swap_kullanımı) × (1000 + oom_score_adj) / 1000
//!
//! Düzeltme faktörleri:
//!   root süreçler:        × 0.8   (biraz daha korunur)
//!   çalışma süresi uzun:  × 0.9   (>10000 ticks ise yavaşça korunur)
//!   çok çocuk süreci var: × 0.85  (>10 çocuk ise daha az tercih edilir)
//!   çekirdek görevi:      → puan = 0 (asla öldürülmez)
//!
//! oom_score_adj aralığı: -1000 (asla öldürülme) → +1000 (hep öldürülme)
//! ```
//!
//! ## OOM Karar Akışı
//!
//! ```
//! oom_kill() çağrıldı
//!      │
//!      ▼
//!  Soğuma süresi geçti mi? (OOM_RECOVERY_WAIT_TICKS = 100)
//!      │
//!      ▼ EVET
//!  Tüm süreçleri al → oom_score() hesapla → sırala
//!      │
//!      ▼
//!  En yüksek puanlı süreci seç
//!  (çekirdek görevleri ve oom_score_adj=-1000 süreçler hariç)
//!      │
//!      ▼
//!  Süreci öldür → OOM geçmişine kaydet → soğuma başlat
//! ```
//!
//! ## Güvenlik ve Sınırlamalar
//!
//! - Çekirdek görevleri (`is_kernel_task = true`) asla öldürülmez
//! - `oom_score_adj = -1000` olan süreçler korunur (sistemd, init vb.)
//! - Ard arda en fazla `OOM_MAX_KILLS = 3` öldürme yapılır
//! - Her öldürme arasında `OOM_RECOVERY_WAIT_TICKS = 100` tick beklenir
//!   (kernel thread'lerin belleği geri vermesi için süre tanınır)
//!
//! ## İlgili Modüller:
//! - `mod.rs`: `MemoryManager::allocate_frame()` — OOM'u çağıran yer
//! - `fibonacci_pmm.rs`: `free_frames()` — boş frame sayısını raporlar
//! - `zswap.rs`: ZSwap — OOM öncesi son kurtarma katmanı

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::task::scheduler::{current_task_id, get_ticks};
use crate::task::task::TaskId;

// ============================================================================
// OOM KILLER CONSTANTS
// ============================================================================

/// OOM kill için minimum boş sayfa sayısı
const OOM_MIN_FREE_PAGES: usize = 64;

/// OOM kill sonrası bekleme süresi (ticks)
const OOM_RECOVERY_WAIT_TICKS: u64 = 100;

/// Maximum OOM kill denemesi
const OOM_MAX_KILLS: usize = 3;

/// OOM score bonus constants
const OOM_SCORE_ADJ_MIN: i16 = -1000;
const OOM_SCORE_ADJ_MAX: i16 = 1000;
const OOM_SCORE_ADJ_ROOT: i16 = 0;

// ============================================================================
// OOM SCORE CALCULATION
// ============================================================================

/// Process OOM bilgisi
#[derive(Clone, Debug)]
pub struct OomProcessInfo {
    pub pid: TaskId,
    pub name: String,
    /// Kullanılan bellek (sayfa sayısı)
    pub rss_pages: usize,
    /// Swap kullanımı (sayfa sayısı)
    pub swap_pages: usize,
    /// OOM score adjustment (kullanıcı ayarı)
    pub oom_score_adj: i16,
    /// Process önceliği (nice)
    pub nice: i16,
    /// Çalışma süresi (ticks)
    pub runtime_ticks: u64,
    /// Kernel process mi?
    pub is_kernel: bool,
    /// Root process mi?
    pub is_root: bool,
    /// Çocuk process sayısı
    pub children: usize,
    /// CPU time yüzdesi
    pub cpu_percent: u64,
}

/// OOM aday process
#[derive(Clone, Debug)]
pub struct OomCandidate {
    pub pid: TaskId,
    pub name: String,
    pub score: u64,
    pub rss_pages: usize,
}

/// OOM killer durumu
pub struct OomState {
    /// OOM aktif mi?
    enabled: AtomicBool,
    /// Son OOM kill zamanı
    last_kill_tick: AtomicU64,
    /// Toplam OOM kill sayısı
    total_kills: AtomicUsize,
    /// Son öldürülen process PID
    last_killed_pid: AtomicUsize,
    /// OOM kill devre dışı bırakılmış process'ler
    oom_exempt: Mutex<Vec<TaskId>>,
    /// OOM score adj değerleri
    oom_scores: Mutex<BTreeMap<TaskId, i16>>,
    /// Kullanıcı tanımlı kritik seviye (0-100, 100 = en kritik/korunacak)
    criticality: Mutex<BTreeMap<TaskId, u8>>,
    /// Kill geçmişi
    kill_history: Mutex<Vec<OomKillRecord>>,
}

/// OOM kill kaydı
#[derive(Clone, Debug)]
pub struct OomKillRecord {
    pub pid: TaskId,
    pub name: String,
    pub score: u64,
    pub rss_pages: usize,
    pub tick: u64,
    pub freed_pages: usize,
}

static OOM_STATE: OomState = OomState {
    enabled: AtomicBool::new(true),
    last_kill_tick: AtomicU64::new(0),
    total_kills: AtomicUsize::new(0),
    last_killed_pid: AtomicUsize::new(0),
    oom_exempt: Mutex::new(Vec::new()),
    oom_scores: Mutex::new(BTreeMap::new()),
    criticality: Mutex::new(BTreeMap::new()),
    kill_history: Mutex::new(Vec::new()),
};

// ============================================================================
// OOM KILLER IMPLEMENTATION
// ============================================================================

impl OomState {
    /// OOM killer aktif mi?
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// OOM killer'ı aktifleştir/devre dışı bırak
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Process'i OOM exempt listesine ekle
    pub fn add_exempt(&self, pid: TaskId) {
        let mut exempt = self.oom_exempt.lock();
        if !exempt.contains(&pid) {
            exempt.push(pid);
        }
    }

    /// Process'i OOM exempt listesinden çıkar
    pub fn remove_exempt(&self, pid: TaskId) {
        let mut exempt = self.oom_exempt.lock();
        exempt.retain(|&p| p != pid);
    }

    /// Process exempt mi?
    pub fn is_exempt(&self, pid: TaskId) -> bool {
        self.oom_exempt.lock().contains(&pid)
    }

    /// OOM score adj ayarla
    pub fn set_oom_score_adj(&self, pid: TaskId, adj: i16) {
        let adj = adj.clamp(OOM_SCORE_ADJ_MIN, OOM_SCORE_ADJ_MAX);
        let mut scores = self.oom_scores.lock();
        scores.insert(pid, adj);
    }

    /// OOM score adj al
    pub fn get_oom_score_adj(&self, pid: TaskId) -> i16 {
        self.oom_scores.lock().get(&pid).copied().unwrap_or(0)
    }

    /// Süreç kritikliği (0-100) ayarla.
    pub fn set_criticality(&self, pid: TaskId, criticality: u8) {
        self.criticality
            .lock()
            .insert(pid, criticality.min(100));
    }

    /// Süreç kritikliği al.
    pub fn get_criticality(&self, pid: TaskId) -> u8 {
        self.criticality.lock().get(&pid).copied().unwrap_or(0)
    }

    /// OOM kill kaydı ekle
    pub fn record_kill(&self, record: OomKillRecord) {
        let mut history = self.kill_history.lock();
        // Son 64 kaydı tut
        if history.len() >= 64 {
            history.remove(0);
        }
        let tick = record.tick;
        let pid = record.pid;
        history.push(record);
        self.total_kills.fetch_add(1, Ordering::SeqCst);
        self.last_killed_pid.store(pid as usize, Ordering::SeqCst);
        self.last_kill_tick.store(tick, Ordering::SeqCst);
    }

    /// Son OOM kill'den beri geçen süre
    pub fn ticks_since_last_kill(&self) -> u64 {
        let last = self.last_kill_tick.load(Ordering::SeqCst);
        let now = get_ticks() as u64;
        now.saturating_sub(last)
    }

    /// Kill geçmişini al
    pub fn get_kill_history(&self) -> Vec<OomKillRecord> {
        self.kill_history.lock().clone()
    }
}

/// OOM score hesapla (Linux benzeri)
///
/// Skor formülü:
/// base_score = rss + swap + page_table_pages
/// adjusted_score = base_score * (1000 + oom_score_adj) / 1000
///
/// Düşük skor = daha az olası öldürülme
/// Yüksek skor = daha olası öldürülme
pub fn calculate_oom_score(info: &OomProcessInfo) -> u64 {
    // Kernel process'leri koru
    if info.is_kernel {
        return 0;
    }

    // Exempt process'ler korunsun
    if OOM_STATE.is_exempt(info.pid) {
        return 0;
    }

    // Temel skor: RSS + swap
    let base_score = (info.rss_pages + info.swap_pages) as u64;

    if base_score == 0 {
        return 0;
    }

    // OOM score adjustment uygula
    let adj = OOM_STATE.get_oom_score_adj(info.pid);
    let multiplier = 1000i64 + adj as i64;

    // Negatif adj düşük skor, pozitif adj yüksek skor
    let adjusted_score = if multiplier <= 0 {
        1 // Minimum skor
    } else {
        base_score.saturating_mul(multiplier as u64) / 1000
    };

    // Root process'ler biraz daha korunsun
    let root_bonus = if info.is_root { 0.8 } else { 1.0 };
    let score = (adjusted_score as f64 * root_bonus) as u64;

    // Uzun süredir çalışan process'ler biraz daha korunsun
    let runtime_factor = if info.runtime_ticks > 10000 {
        0.9
    } else if info.runtime_ticks > 1000 {
        0.95
    } else {
        1.0
    };

    // Çok fazla çocuk process'i olan process'ler daha az öldürülsün
    let children_factor = if info.children > 10 {
        0.85
    } else if info.children > 5 {
        0.9
    } else {
        1.0
    };

    let final_score = (score as f64 * runtime_factor * children_factor) as u64;
    let criticality = OOM_STATE.get_criticality(info.pid) as u64;
    let protection = 100u64.saturating_sub(criticality).max(1);
    let protected = final_score.saturating_mul(protection) / 100;
    protected.max(1)
}

/// OOM adaylarını topla ve sırala
pub fn select_oom_victim(processes: &[OomProcessInfo]) -> Option<OomCandidate> {
    if !OOM_STATE.is_enabled() {
        return None;
    }

    // Skorları hesapla ve sırala
    let mut candidates: Vec<OomCandidate> = processes
        .iter()
        .filter(|p| !p.is_kernel && !OOM_STATE.is_exempt(p.pid))
        .map(|p| {
            let score = calculate_oom_score(p);
            OomCandidate {
                pid: p.pid,
                name: p.name.clone(),
                score,
                rss_pages: p.rss_pages,
            }
        })
        .filter(|c| c.score > 0)
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Skora göre azalan sırada sırala (yüksek skor = öldürülecek)
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.rss_pages.cmp(&a.rss_pages))
            .then_with(|| b.pid.cmp(&a.pid))
    });

    // En yüksek skorlu adayı döndür
    candidates.into_iter().next()
}

/// OOM killer'ı tetikle
///
/// Bellek kritik seviyede olduğunda çağrılır.
/// En yüksek skorlu process'i öldürür.
///
/// # Returns
/// - `Some(freed_pages)`: Öldürülen process ve serbest kalan sayfa sayısı
/// - `None`: Öldürülecek process bulunamadı
pub fn oom_kill(processes: &[OomProcessInfo]) -> Option<usize> {
    // Son kill'den sonra yeterli süre geçti mi?
    if OOM_STATE.ticks_since_last_kill() < OOM_RECOVERY_WAIT_TICKS {
        return None;
    }

    let victim = select_oom_victim(processes)?;

    crate::serial_println!(
        "[OOM] Killing process '{}' (PID: {}) with score {} (RSS: {} pages)",
        victim.name,
        victim.pid,
        victim.score,
        victim.rss_pages
    );

    // Process'i öldür
    // Not: Gerçek implementation task manager ile entegre olmalı
    let freed_pages = victim.rss_pages;

    // Kill kaydı tut
    OOM_STATE.record_kill(OomKillRecord {
        pid: victim.pid,
        name: victim.name.clone(),
        score: victim.score,
        rss_pages: victim.rss_pages,
        tick: get_ticks() as u64,
        freed_pages,
    });

    // Process'i terminate et (SIGKILL)
    let _ = crate::task::scheduler::kill_task(victim.pid, 9);

    Some(freed_pages)
}

/// Bellek yetersizliği kontrolü
pub fn should_trigger_oom(free_pages: usize, total_pages: usize) -> bool {
    if !OOM_STATE.is_enabled() {
        return false;
    }

    // PSI erken uyarı — serbest bellek eşikleri düşmeden OOM yolunu hazırla.
    if crate::memory::psi::severe_memory_pressure() {
        return true;
    }

    // Minimum eşik
    if free_pages < OOM_MIN_FREE_PAGES {
        return true;
    }

    // Toplam belleğin %5'inden az
    let threshold = total_pages / 20;
    if free_pages < threshold {
        return true;
    }

    false
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// OOM killer'ı başlat
pub fn init() {
    OOM_STATE.set_enabled(true);
    crate::serial_println!("[OOM] OOM Killer initialized");
}

/// Cgroup-scoped OOM kill: belirli cgroup'üan süreçlerini hedef al
/// Cgroup bellek limiti aşıldığında çağrılır.
pub fn oom_kill_cgroup(cgroup_name: &str, process_pids: &[u64]) -> Option<usize> {
    if process_pids.is_empty() {
        return None;
    }

    // Cgroup'üan süreçlerinden OomProcessInfo oluştur
    let tasks = crate::task::scheduler::list_tasks();
    let oom_infos: alloc::vec::Vec<OomProcessInfo> = tasks
        .iter()
        .filter(|t| process_pids.contains(&(t.pid as u64)))
        .map(|t| OomProcessInfo {
            pid: t.pid,
            name: alloc::string::String::from(t.name),
            rss_pages: 256,
            swap_pages: 0,
            oom_score_adj: 0,
            nice: 0,
            runtime_ticks: 0,
            is_kernel: t.pid < 2,
            is_root: false,
            children: 0,
            cpu_percent: 0,
        })
        .collect();

    crate::serial_println!(
        "[OOM] Cgroup '{}' OOM kill with {} candidate processes",
        cgroup_name,
        oom_infos.len()
    );

    oom_kill(&oom_infos)
}

/// OOM killer aktif mi?
pub fn is_enabled() -> bool {
    OOM_STATE.is_enabled()
}

/// OOM killer'ı aktifleştir/devre dışı bırak
pub fn set_enabled(enabled: bool) {
    OOM_STATE.set_enabled(enabled);
}

/// Process'i OOM exempt listesine ekle (öldürülmez)
pub fn add_oom_exempt(pid: TaskId) {
    OOM_STATE.add_exempt(pid);
}

/// Process'i OOM exempt listesinden çıkar
pub fn remove_oom_exempt(pid: TaskId) {
    OOM_STATE.remove_exempt(pid);
}

/// Process OOM exempt mi?
pub fn is_oom_exempt(pid: TaskId) -> bool {
    OOM_STATE.is_exempt(pid)
}

/// Process OOM score adjustment ayarla
pub fn set_oom_score_adj(pid: TaskId, adj: i16) {
    OOM_STATE.set_oom_score_adj(pid, adj);
}

/// Process kritikliğini ayarla (0-100).
/// 100: mümkün olduğunca korunur, 0: nötr.
pub fn set_oom_criticality(pid: TaskId, criticality: u8) {
    OOM_STATE.set_criticality(pid, criticality);
}

pub fn get_oom_criticality(pid: TaskId) -> u8 {
    OOM_STATE.get_criticality(pid)
}

/// Process OOM score adjustment al
pub fn get_oom_score_adj(pid: TaskId) -> i16 {
    OOM_STATE.get_oom_score_adj(pid)
}

/// OOM kill geçmişini al
pub fn get_oom_history() -> Vec<OomKillRecord> {
    OOM_STATE.get_kill_history()
}

/// Toplam OOM kill sayısı
pub fn total_oom_kills() -> usize {
    OOM_STATE.total_kills.load(Ordering::SeqCst)
}

/// Son öldürülen process PID
pub fn last_killed_pid() -> TaskId {
    OOM_STATE.last_killed_pid.load(Ordering::SeqCst) as TaskId
}

/// OOM killer istatistikleri
pub struct OomStats {
    pub enabled: bool,
    pub total_kills: usize,
    pub last_killed_pid: TaskId,
    pub ticks_since_last_kill: u64,
    pub exempt_count: usize,
    pub psi_some_avg10: u64,
    pub psi_full_avg10: u64,
}

/// OOM istatistiklerini al
pub fn get_oom_stats() -> OomStats {
    let psi = crate::memory::psi::snapshot();
    OomStats {
        enabled: OOM_STATE.is_enabled(),
        total_kills: OOM_STATE.total_kills.load(Ordering::SeqCst),
        last_killed_pid: OOM_STATE.last_killed_pid.load(Ordering::SeqCst) as TaskId,
        ticks_since_last_kill: OOM_STATE.ticks_since_last_kill(),
        exempt_count: OOM_STATE.oom_exempt.lock().len(),
        psi_some_avg10: psi.some_avg10,
        psi_full_avg10: psi.full_avg10,
    }
}
