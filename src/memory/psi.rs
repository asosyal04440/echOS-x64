//! Pressure Stall Information (PSI) — bellek baskısı telemetrisi.
//!
//! Linux PSI modeline benzer şekilde "some" ve "full" bellek stall sürelerini
//! tutar; OOM ve reclaim politikaları bu sinyali erken uyarı olarak kullanır.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const PSI_WINDOWS: [u64; 3] = [10, 60, 300];
const SCALE: u64 = 1000;

#[derive(Clone, Copy, Debug, Default)]
pub struct PsiSnapshot {
    pub some_avg10: u64,
    pub some_avg60: u64,
    pub some_avg300: u64,
    pub full_avg10: u64,
    pub full_avg60: u64,
    pub full_avg300: u64,
    pub total_some_stall_ticks: u64,
    pub total_full_stall_ticks: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct PsiWindow {
    some: u64,
    full: u64,
}

impl PsiWindow {
    fn update(&mut self, some_sample: u64, full_sample: u64, window: u64) {
        // EMA: next = old + alpha * (sample - old), alpha = 1/window
        self.some = self
            .some
            .saturating_add(some_sample.saturating_sub(self.some) / window.max(1));
        self.full = self
            .full
            .saturating_add(full_sample.saturating_sub(self.full) / window.max(1));
    }
}

struct PsiState {
    windows: [PsiWindow; 3],
    total_some: u64,
    total_full: u64,
}

impl PsiState {
    fn new() -> Self {
        Self {
            windows: [
                PsiWindow::default(),
                PsiWindow::default(),
                PsiWindow::default(),
            ],
            total_some: 0,
            total_full: 0,
        }
    }

    fn record(&mut self, stalled_ticks: u64, total_ticks: u64, full: bool) {
        if total_ticks == 0 {
            return;
        }
        let some_sample = stalled_ticks
            .saturating_mul(SCALE)
            .saturating_div(total_ticks)
            .min(SCALE);
        let full_sample = if full { some_sample } else { 0 };

        self.total_some = self.total_some.saturating_add(stalled_ticks);
        if full {
            self.total_full = self.total_full.saturating_add(stalled_ticks);
        }

        for (idx, window) in PSI_WINDOWS.iter().enumerate() {
            self.windows[idx].update(some_sample, full_sample, *window);
        }
    }

    fn snapshot(&self) -> PsiSnapshot {
        PsiSnapshot {
            some_avg10: self.windows[0].some,
            some_avg60: self.windows[1].some,
            some_avg300: self.windows[2].some,
            full_avg10: self.windows[0].full,
            full_avg60: self.windows[1].full,
            full_avg300: self.windows[2].full,
            total_some_stall_ticks: self.total_some,
            total_full_stall_ticks: self.total_full,
        }
    }
}

static PSI_ENABLED: AtomicBool = AtomicBool::new(true);

static PSI: spin::Lazy<Mutex<PsiState>> = spin::Lazy::new(|| Mutex::new(PsiState::new()));

pub fn init(enabled: bool) {
    PSI_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    PSI_ENABLED.load(Ordering::Acquire)
}

/// Bellek stall olayı kaydet.
///
/// `stalled_ticks`: iş yürütmenin ilerleyemediği süre.
/// `total_ticks`: örnekleme penceresi.
/// `full`: true ise tüm işler stall, false ise yalnızca bir kısmı stall.
pub fn record_memory_stall(stalled_ticks: u64, total_ticks: u64, full: bool) {
    if !is_enabled() {
        return;
    }
    PSI.lock().record(stalled_ticks, total_ticks, full);
}

pub fn snapshot() -> PsiSnapshot {
    PSI.lock().snapshot()
}

/// OOM öncesi erken uyarı sinyali.
///
/// some_avg10 >= 70% veya full_avg10 >= 35% ise true döner.
pub fn severe_memory_pressure() -> bool {
    let s = snapshot();
    s.some_avg10 >= 700 || s.full_avg10 >= 350
}
