//! # echOS Rastgele Sayı Üreteci
//!
//! Xorshift32 algoritması ile pseudo-random (sözde rastgele) sayı üretimi.
//! Zamanlayıcı piyango zamanlaması (lottery scheduling) ve yük dengeleme (load balancing) için kullanılır.
//! Her CPU'ya özel durum (per-CPU state) ve kilit-serbest (lock-free) mimari ile ölçeklenebilir yapıdadır.
//!
//! ## Xorshift Nedir?
//! Xorshift, üç XOR/kaydırma adımı kullanan hızlı bir sözde rastgele sayı üretme
//! algoritmasıdır. Kriptografi için uygun değildir, ancak zamanlayıcı kararları için yeterince
//! kaliteli rastlantısallık sağlar.
//!
//! ```ascii
//! tohum --> x ^= x << 13
//!       --> x ^= x >> 17
//!       --> x ^= x << 5
//!       --> sonuç (yeni tohum)
//! ```

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use echos_random::{normalize_seed, xorshift32_step, DEFAULT_NONZERO_SEED};

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
#[repr(align(64))]
struct HostCpuKeyAllocator(AtomicU32);

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
static NEXT_HOST_RANDOM_CPU_KEY: HostCpuKeyAllocator = HostCpuKeyAllocator(AtomicU32::new(0));

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
std::thread_local! {
    static HOST_RANDOM_CPU_KEY: u32 =
        NEXT_HOST_RANDOM_CPU_KEY.0.fetch_add(1, Ordering::Relaxed) % (MAX_CPUS as u32);
}

/// Desteklenen maksimum CPU sayısı.
const MAX_CPUS: usize = crate::cpu::cpu_slots::MAX_CPU_SLOTS;
const UNINITIALIZED_SEED: u32 = 0;

#[repr(align(64))]
struct CpuRngState {
    seed: AtomicU32,
    entropy: AtomicU64,
}

impl CpuRngState {
    const fn new() -> Self {
        Self {
            seed: AtomicU32::new(UNINITIALIZED_SEED),
            entropy: AtomicU64::new(0),
        }
    }
}

/// Every CPU owns a dedicated cache line; unrelated RNG updates cannot false-share.
static CPU_RNG: [CpuRngState; MAX_CPUS] = [const { CpuRngState::new() }; MAX_CPUS];

/// Out-of-range CPU identities use a separate lock-free state instead of CPU 0.
static FALLBACK_RNG: CpuRngState = CpuRngState::new();

#[repr(align(64))]
struct GlobalEntropy(AtomicU64);

static GLOBAL_ENTROPY: GlobalEntropy = GlobalEntropy(AtomicU64::new(0));

#[repr(align(64))]
struct DeterministicSeed(AtomicU32);

/// `GRND_DETERMINISTIC` is intentionally one global reproducible stream.
static DETERMINISTIC_SEED: DeterministicSeed = DeterministicSeed(AtomicU32::new(0xA5A5_A5A5));

#[inline]
fn tsc_entropy() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        unsafe { core::arch::x86_64::_rdtsc() as u64 }
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

fn secure_seed_material(cpu_id: usize) -> u32 {
    let mut bytes = [0u8; 4];
    if crate::crypto::rdseed_bytes(&mut bytes) || crate::crypto::rdrand_bytes(&mut bytes) {
        let seeded = u32::from_le_bytes(bytes) ^ (cpu_id as u32).wrapping_mul(0x9E37_79B9);
        return normalize_seed(seeded);
    }

    let ticks = crate::interrupts::get_ticks() as u64;
    let mixed = ticks
        ^ tsc_entropy()
        ^ GLOBAL_ENTROPY.0.load(Ordering::Relaxed)
        ^ ((cpu_id as u64) << 32)
        ^ 0xA5A5_5A5A_A5A5_5A5A;
    let seeded = (mixed as u32) ^ ((mixed >> 32) as u32);
    normalize_seed(seeded)
}

#[inline]
fn derived_seed(base_seed: u32, cpu_id: usize) -> u32 {
    normalize_seed(base_seed.wrapping_add((cpu_id as u32).wrapping_mul(0x9E37_79B9)))
}

fn initialize_cpu_states(cpu_count: u32, requested_seed: u32) {
    let count = (cpu_count as usize).clamp(1, MAX_CPUS);
    let base_seed = if requested_seed == 0 {
        secure_seed_material(0)
    } else {
        normalize_seed(requested_seed)
    };

    for (cpu_id, state) in CPU_RNG.iter().enumerate() {
        if cpu_id < count {
            let seed = if requested_seed == 0 {
                secure_seed_material(cpu_id)
            } else {
                derived_seed(base_seed, cpu_id)
            };
            // Entropy reset precedes Release publication of the initialized seed.
            state.entropy.store(0, Ordering::Relaxed);
            state.seed.store(seed, Ordering::Release);
        } else {
            state.entropy.store(0, Ordering::Relaxed);
            state.seed.store(UNINITIALIZED_SEED, Ordering::Release);
        }
    }

    FALLBACK_RNG.entropy.store(0, Ordering::Relaxed);
    FALLBACK_RNG
        .seed
        .store(secure_seed_material(MAX_CPUS), Ordering::Release);

    GLOBAL_ENTROPY.0.fetch_xor(
        ((base_seed as u64) << 32)
            ^ tsc_entropy()
            ^ crate::interrupts::get_ticks() as u64
            ^ count as u64,
        Ordering::AcqRel,
    );
}

#[inline]
fn current_rng_cpu_id() -> usize {
    #[cfg(any(target_os = "none", target_os = "uefi"))]
    {
        crate::cpu::smp::current_cpu_id() as usize
    }

    #[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
    {
        HOST_RANDOM_CPU_KEY.with(|cpu_id| *cpu_id as usize)
    }
}

/// RNG'yi başlangıç tohumuyla başlatır.
///
/// # Parametreler
/// - `seed`: Başlangıç değeri (örn: TSC'den alınabilir; 0 ise varsayılan kullanılır)
///
/// CPU 0 (BSP) için doğrudan tohum ayarlanır; diğer CPU'lar için altın oran
/// çarpanıyla (0x9E3779B9) benzersiz türev tohumlar hesaplanır.
/// Bu yaklaşım, her CPU'nun birbirinden bağımsız sayı üretmesini sağlar.
pub fn init(seed: u32) {
    initialize_cpu_states(crate::cpu::cpu_slots::cpu_count(), seed);
}

#[inline]
fn state_for_cpu(cpu_id: usize) -> &'static CpuRngState {
    CPU_RNG.get(cpu_id).unwrap_or(&FALLBACK_RNG)
}

/// Entropi havuzuna yeni bir değer karıştırır.
///
/// Döndürme (rotate) ve XOR işlemleriyle girişin tüm bitlerini yayar;
/// küçük giriş farklılıkları büyük çıkış farklılıklarına yol açar (çığ etkisi).
pub fn add_entropy(value: u64) {
    let mixed = value ^ value.rotate_left(17) ^ value.rotate_right(23);
    let cpu_id = current_rng_cpu_id();
    let state = state_for_cpu(cpu_id);

    // Entropy changes statistical input only; it does not publish kernel data.
    state.entropy.fetch_xor(mixed, Ordering::Relaxed);
    GLOBAL_ENTROPY.0.fetch_xor(mixed, Ordering::Relaxed);
}

/// Rastgele bir u32 değeri üretir.
///
/// Xorshift algoritması uygulanır; ardından CPU'nun entropi havuzuyla XOR'lanarak
/// çıktı kalitesi artırılır. Kilit almadan çalışır (lock-free).
pub fn next_u32() -> u32 {
    let cpu_id = current_rng_cpu_id();
    let state = state_for_cpu(cpu_id);

    // Acquire pairs with boot/hotplug seed publication after state initialization.
    let mut current = state.seed.load(Ordering::Acquire);
    loop {
        let seeded = if current == UNINITIALIZED_SEED {
            secure_seed_material(cpu_id)
        } else {
            current
        };
        let mut next = xorshift32_step(seeded);

        let entropy =
            state.entropy.load(Ordering::Relaxed) ^ GLOBAL_ENTROPY.0.load(Ordering::Relaxed);
        next ^= entropy as u32;
        next ^= (entropy >> 32) as u32;
        next = normalize_seed(next);

        match state
            .seed
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

/// Rastgele bir u64 değeri üretir.
///
/// İki bağımsız `next_u32()` çağrısını birleştirerek 64 bitlik değer oluşturur.
pub fn rand_u64() -> u64 {
    let low = next_u32() as u64;
    let high = next_u32() as u64;
    (high << 32) | low
}

fn best_effort_secure_u32() -> (u32, bool) {
    let mut bytes = [0u8; 4];
    if crate::crypto::rdseed_bytes(&mut bytes) || crate::crypto::rdrand_bytes(&mut bytes) {
        return (u32::from_le_bytes(bytes), true);
    }

    let fallback = rand_u64() ^ crate::interrupts::get_ticks();
    let mixed = (fallback as u32) ^ ((fallback >> 32) as u32);
    (mixed, false)
}

pub fn secure_u16() -> (u16, bool) {
    let (value, secure) = best_effort_secure_u32();
    ((value & 0xFFFF) as u16, secure)
}

pub fn secure_range_u16(min_inclusive: u16, max_inclusive: u16) -> (u16, bool) {
    if min_inclusive >= max_inclusive {
        return (min_inclusive, true);
    }

    let span = (max_inclusive as u32)
        .saturating_sub(min_inclusive as u32)
        .saturating_add(1);
    let threshold = span.wrapping_neg() % span;
    let mut secure = true;

    loop {
        let (sample, sample_secure) = best_effort_secure_u32();
        secure &= sample_secure;
        if sample >= threshold {
            let value = min_inclusive as u32 + (sample % span);
            return (value as u16, secure);
        }
    }
}

/// `[0, max)` aralığında rastgele bir sayı üretir.
///
/// `max == 0` ise sıfır döner (sıfıra bölünme koruması).
/// Rejection sampling ile modulo bias engellenir.
pub fn next_range(max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let threshold = max.wrapping_neg() % max;
    loop {
        let sample = next_u32();
        if sample >= threshold {
            return sample % max;
        }
    }
}

/// Verilen tamponu (buffer) rastgele baytlarla doldurur.
///
/// Her 4 baytlık blok için `next_u32()` çağrısı yapılır.
/// Artan tampon boyutunda verimlidir.
pub fn fill_bytes(buf: &mut [u8]) {
    if crate::crypto::rdseed_bytes(buf) || crate::crypto::rdrand_bytes(buf) {
        return;
    }

    let mut offset = 0;
    while offset < buf.len() {
        let value = next_u32();
        // Little-endian sırasıyla bayt dizisine dönüştür
        let bytes = value.to_le_bytes();
        let remain = buf.len() - offset;
        let count = remain.min(bytes.len());
        buf[offset..offset + count].copy_from_slice(&bytes[..count]);
        offset += count;
    }
}

/// Deterministik (yeniden üretilebilir) rastgele u32 üretir.
///
/// Global `DETERMINISTIC_SEED` üzerinde Xorshift uygular. Aynı tohum
/// verildiğinde her çalıştırmada aynı sayı dizisi üretilir.
fn deterministic_next_u32() -> u32 {
    let mut current = DETERMINISTIC_SEED.0.load(Ordering::Acquire);
    loop {
        let seeded = if current == 0 {
            DEFAULT_NONZERO_SEED
        } else {
            current
        };
        let next = normalize_seed(xorshift32_step(seeded));
        match DETERMINISTIC_SEED.0.compare_exchange(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}

/// Verilen tamponu deterministik (yeniden üretilebilir) rastgele baytlarla doldurur.
///
/// `GRND_DETERMINISTIC` sistem çağrısı bayrağıyla kullanılır. Test ortamlarında
/// sabit tohum verilerek her çalıştırmada aynı bayt dizisi garantilenebilir.
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

/// Initializes exactly the CPU slots published by SMP discovery.
///
/// Remaining slots stay in the lazy-uninitialized state for future hotplug.
pub fn init_per_cpu_entropy(cpu_count: u32) {
    initialize_cpu_states(cpu_count, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_seeds_are_nonzero_and_cpu_distinct() {
        let base = 0x1234_5678;
        let cpu0 = derived_seed(base, 0);
        let cpu1 = derived_seed(base, 1);
        let cpu255 = derived_seed(base, 255);

        assert_ne!(cpu0, 0);
        assert_ne!(cpu1, 0);
        assert_ne!(cpu255, 0);
        assert_ne!(cpu0, cpu1);
        assert_ne!(cpu1, cpu255);
    }
}
