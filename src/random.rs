//! # echOS Rastgele Sayı Üreteci
//!
//! Xorshift32 algoritması ile pseudo-random sayı üretimi.
//! Scheduler lottery scheduling ve load balancing için kullanılır.
//! Lock-free ve per-CPU state ile ölçeklenebilir yapıdadır.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const MAX_CPUS: usize = 32;

/// Per-CPU RNG seed değerleri
static SEEDS: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(123456789) }; MAX_CPUS];

/// Per-CPU entropy havuzları
static ENTROPY_POOL: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

/// Global fallback entropy
static GLOBAL_ENTROPY: AtomicU64 = AtomicU64::new(0);

/// Deterministic seed for GRND_DETERMINISTIC
static DETERMINISTIC_SEED: AtomicU32 = AtomicU32::new(0xA5A5A5A5);

/// RNG'yi seed ile başlatır.
///
/// # Parametreler
/// - `seed`: Başlangıç değeri (örn: TSC'den alınabilir)
pub fn init(seed: u32) {
    let valid_seed = if seed == 0 { 123456789 } else { seed };
    // BSP (CPU 0) seed'ini ayarla
    SEEDS[0].store(valid_seed, Ordering::Relaxed);
    
    // Diğer CPU'lar için seed'i türet
    for i in 1..MAX_CPUS {
        let sub_seed = valid_seed.wrapping_add((i as u32).wrapping_mul(0x9E3779B9)); // Golden ratio
        SEEDS[i].store(sub_seed, Ordering::Relaxed);
    }
}

pub fn add_entropy(value: u64) {
    let mixed = value ^ value.rotate_left(17) ^ value.rotate_right(23);
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    
    if cpu_id < MAX_CPUS {
        ENTROPY_POOL[cpu_id].fetch_xor(mixed, Ordering::Relaxed);
    } else {
        GLOBAL_ENTROPY.fetch_xor(mixed, Ordering::Relaxed);
    }
    
    // Global seed'i de karıştır (legacy desteği için veya global randomness)
    // Ama lock-free yapıda global seed kullanmıyoruz, sadece entropy'i global havuza da ekleyebiliriz.
    GLOBAL_ENTROPY.fetch_xor(mixed, Ordering::Relaxed);
}

/// Rastgele bir u32 değeri üretir.
pub fn next_u32() -> u32 {
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    let seed_ptr = if cpu_id < MAX_CPUS {
        &SEEDS[cpu_id]
    } else {
        &SEEDS[0] // Fallback to BSP
    };

    let mut x = seed_ptr.load(Ordering::Relaxed);
    if x == 0 {
        x = 123456789 + (cpu_id as u32);
    }

    // Xorshift algoritması
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;

    let entropy = if cpu_id < MAX_CPUS {
        ENTROPY_POOL[cpu_id].load(Ordering::Relaxed)
    } else {
        GLOBAL_ENTROPY.load(Ordering::Relaxed)
    };
    
    x ^= entropy as u32;
    x ^= (entropy >> 32) as u32;
    
    seed_ptr.store(x, Ordering::Relaxed);
    x
}

/// Rastgele bir u64 değeri üretir.
pub fn rand_u64() -> u64 {
    let low = next_u32() as u64;
    let high = next_u32() as u64;
    (high << 32) | low
}

/// [0, max) aralığında rastgele sayı üretir.
pub fn next_range(max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    next_u32() % max
}

pub fn fill_bytes(buf: &mut [u8]) {
    let mut offset = 0;
    while offset < buf.len() {
        let value = next_u32();
        let bytes = value.to_le_bytes();
        let remain = buf.len() - offset;
        let count = remain.min(bytes.len());
        buf[offset..offset + count].copy_from_slice(&bytes[..count]);
        offset += count;
    }
}

fn deterministic_next_u32() -> u32 {
    let mut x = DETERMINISTIC_SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    DETERMINISTIC_SEED.store(x, Ordering::Relaxed);
    x
}

pub fn fill_bytes_deterministic(buf: &mut [u8]) {
    let mut offset = 0;
    while offset < buf.len() {
        let value = deterministic_next_u32();
        let bytes = value.to_le_bytes();
        let remain = buf.len() - offset;
        let count = remain.min(bytes.len());
        buf[offset..offset + count].copy_from_slice(&bytes[..count]);
        offset += count;
    }
}

// Legacy uyumluluğu için boş fonksiyon
pub fn init_per_cpu_entropy(_cpu_count: u32) {}
