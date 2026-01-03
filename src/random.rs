//! # echOS Rastgele Sayı Üreteci
//! 
//! Xorshift32 algoritması ile pseudo-random sayı üretimi.
//! Scheduler lottery scheduling için kullanılır.

use core::sync::atomic::{AtomicU32, Ordering};

/// RNG seed değeri
static SEED: AtomicU32 = AtomicU32::new(123456789);

/// RNG'yi seed ile başlatır.
/// 
/// # Parametreler
/// - `seed`: Başlangıç değeri (örn: TSC'den alınabilir)
pub fn init(seed: u32) {
    let valid_seed = if seed == 0 { 123456789 } else { seed };
    SEED.store(valid_seed, Ordering::Relaxed);
}

/// Rastgele bir u32 değeri üretir.
pub fn next_u32() -> u32 {
    let mut x = SEED.load(Ordering::Relaxed);
    
    // Xorshift algoritması
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    
    SEED.store(x, Ordering::Relaxed);
    x
}

/// [0, max) aralığında rastgele sayı üretir.
pub fn next_range(max: u32) -> u32 {
    if max == 0 { return 0; }
    next_u32() % max
}
