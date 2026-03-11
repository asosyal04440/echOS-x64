//! KASAN-benzeri gölge bellek denetimi (opsiyonel).
//!
//! Bu modül, debug derlemelerinde bellek erişim sınır ihlallerini yakalamak için
//! 8-byte granülerlikte shadow byte modeli uygular:
//! - 0   => tüm 8 byte erişilebilir
//! - 1-7 => kısmi erişim (yalnızca ilk N byte geçerli)
//! - 0xFF => tamamen zehirli (poisoned)

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const GRANULE: usize = 8;
const SHADOW_POISON: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Default)]
pub struct KasanStats {
    pub enabled: bool,
    pub tracked_granules: usize,
    pub violations: u64,
}

struct KasanState {
    shadow: BTreeMap<usize, u8>,
    violations: u64,
}

impl KasanState {
    fn new() -> Self {
        Self {
            shadow: BTreeMap::new(),
            violations: 0,
        }
    }

    fn mark_alloc(&mut self, addr: usize, size: usize) {
        if size == 0 {
            return;
        }
        let start = addr / GRANULE;
        let end = (addr + size - 1) / GRANULE;
        for g in start..=end {
            let g_base = g * GRANULE;
            let valid = if g_base < addr {
                (addr + size).saturating_sub(addr).min(GRANULE)
            } else {
                let remain = (addr + size).saturating_sub(g_base);
                remain.min(GRANULE)
            };
            self.shadow.insert(g, valid as u8);
        }
    }

    fn mark_free(&mut self, addr: usize, size: usize) {
        if size == 0 {
            return;
        }
        let start = addr / GRANULE;
        let end = (addr + size - 1) / GRANULE;
        for g in start..=end {
            self.shadow.insert(g, SHADOW_POISON);
        }
    }

    fn check(&mut self, addr: usize, size: usize) -> bool {
        if size == 0 {
            return true;
        }
        let start = addr / GRANULE;
        let end = (addr + size - 1) / GRANULE;
        for g in start..=end {
            let shadow = self.shadow.get(&g).copied().unwrap_or(SHADOW_POISON);
            if shadow == SHADOW_POISON {
                self.violations = self.violations.saturating_add(1);
                return false;
            }
        }
        true
    }
}

static KASAN_ENABLED: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref KASAN: Mutex<KasanState> = Mutex::new(KasanState::new());
}

pub fn init(enabled: bool) {
    KASAN_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn is_enabled() -> bool {
    KASAN_ENABLED.load(Ordering::Acquire)
}

pub fn mark_alloc(addr: usize, size: usize) {
    if !is_enabled() {
        return;
    }
    KASAN.lock().mark_alloc(addr, size);
}

pub fn mark_free(addr: usize, size: usize) {
    if !is_enabled() {
        return;
    }
    KASAN.lock().mark_free(addr, size);
}

pub fn check_access(addr: usize, size: usize) -> bool {
    if !is_enabled() {
        return true;
    }
    KASAN.lock().check(addr, size)
}

pub fn stats() -> KasanStats {
    let guard = KASAN.lock();
    KasanStats {
        enabled: is_enabled(),
        tracked_granules: guard.shadow.len(),
        violations: guard.violations,
    }
}
