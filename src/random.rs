//! # echOS Rastgele Sayı Üreteci
//!
//! Xorshift32 algoritması ile pseudo-random (sözde rastgele) sayı üretimi.
//! Zamanlayıcı piyango zamanlaması (lottery scheduling) ve yük dengeleme (load balancing) için kullanılır.
//! Her CPU'ya özel durum (per-CPU state) ve kilit-serbest (lock-free) mimari ile ölçeklenebilir yapıdadır.
//!
//! ## Xorshift Nedir?
//! Xorshift, basit bit kaydırma ve XOR işlemlerini kullanan hızlı bir sözde rastgele sayı üretme
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

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
static NEXT_HOST_RANDOM_CPU_KEY: AtomicU32 = AtomicU32::new(0);

#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
std::thread_local! {
    static HOST_RANDOM_CPU_KEY: u32 =
        NEXT_HOST_RANDOM_CPU_KEY.fetch_add(1, Ordering::AcqRel) % (MAX_CPUS as u32);
}

/// Desteklenen maksimum CPU sayısı.
const MAX_CPUS: usize = 32;
const UNINITIALIZED_SEED: u32 = 0;
const FALLBACK_NONZERO_SEED: u32 = 0x9E37_79B9;

/// CPU başına RNG tohum (seed) değerleri.
///
/// Her CPU kendi tohumunu korur: atomik kilit gerekmez, çekirdek başına izole durum sağlanır.
static SEEDS: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(UNINITIALIZED_SEED) }; MAX_CPUS];

/// CPU başına entropi havuzları.
///
/// Donanım olayları, zamanlayıcı tick'leri vb. kaynaklardan gelen entropi bu havuzlara
/// eklenerek üretilen sayıların kalitesi artırılır.
static ENTROPY_POOL: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

/// Global yedek entropi havuzu.
///
/// CPU sayısı `MAX_CPUS`'u aştığında veya CPU kimliği belirlenemediğinde bu havuz kullanılır.
static GLOBAL_ENTROPY: AtomicU64 = AtomicU64::new(0);

/// `GRND_DETERMINISTIC` modu için deterministik tohum.
///
/// Test ve üretim ortamları için yeniden üretilebilir (reproducible) sayı dizileri
/// oluşturmaya yarar. 0xA5A5A5A5 çarpraz bit deseni ile başlatılır.
static DETERMINISTIC_SEED: AtomicU32 = AtomicU32::new(0xA5A5A5A5);

#[inline]
fn xorshift32_step(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

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
        return seeded.max(1);
    }

    let ticks = crate::interrupts::get_ticks() as u64;
    let mixed = ticks
        ^ tsc_entropy()
        ^ GLOBAL_ENTROPY.load(Ordering::Relaxed)
        ^ ((cpu_id as u64) << 32)
        ^ 0xA5A5_5A5A_A5A5_5A5A;
    let seeded = (mixed as u32) ^ ((mixed >> 32) as u32);
    seeded.max(1)
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
    let base_seed = if seed == 0 {
        secure_seed_material(0)
    } else {
        seed
    };
    // BSP (CPU 0) seed'ini ayarla
    SEEDS[0].store(base_seed.max(1), Ordering::Release);

    // Diğer CPU'lar için seed'i türet
    for i in 1..MAX_CPUS {
        let derived = if seed == 0 {
            secure_seed_material(i)
        } else {
            base_seed.wrapping_add((i as u32).wrapping_mul(0x9E37_79B9))
        };
        SEEDS[i].store(derived.max(1), Ordering::Release);
    }

    GLOBAL_ENTROPY.fetch_xor(
        ((base_seed as u64) << 32) ^ tsc_entropy() ^ crate::interrupts::get_ticks() as u64,
        Ordering::Relaxed,
    );
}

/// Entropi havuzuna yeni bir değer karıştırır.
///
/// Döndürme (rotate) ve XOR işlemleriyle girişin tüm bitlerini yayar;
/// küçük giriş farklılıkları büyük çıkış farklılıklarına yol açar (çığ etkisi).
pub fn add_entropy(value: u64) {
    // Entropi değerini bit döndürme ve XOR ile karıştır (avalanche etkisi)
    let mixed = value ^ value.rotate_left(17) ^ value.rotate_right(23);
    let cpu_id = current_rng_cpu_id();

    if cpu_id < MAX_CPUS {
        // Mevcut CPU'nun havuzuna XOR ile karıştır
        ENTROPY_POOL[cpu_id].fetch_xor(mixed, Ordering::Relaxed);
    } else {
        // CPU aralık dışıysa global havuza ekle
        GLOBAL_ENTROPY.fetch_xor(mixed, Ordering::Relaxed);
    }

    // Global seed'i de karıştır (legacy desteği için veya global randomness)
    // Ama lock-free yapıda global seed kullanmıyoruz, sadece entropy'i global havuza da ekleyebiliriz.
    GLOBAL_ENTROPY.fetch_xor(mixed, Ordering::Relaxed);
}

/// Rastgele bir u32 değeri üretir.
///
/// Xorshift algoritması uygulanır; ardından CPU'nun entropi havuzuyla XOR'lanarak
/// çıktı kalitesi artırılır. Kilit almadan çalışır (lock-free).
pub fn next_u32() -> u32 {
    let cpu_id = current_rng_cpu_id();
    // Geçerli CPU'nun tohumunu seç; aralık dışıysa BSP'ye geri düş
    let seed_ptr = if cpu_id < MAX_CPUS {
        &SEEDS[cpu_id]
    } else {
        &SEEDS[0] // BSP'ye geri düş
    };

    let mut current = seed_ptr.load(Ordering::Acquire);
    loop {
        let seeded = if current == UNINITIALIZED_SEED {
            secure_seed_material(cpu_id)
        } else {
            current
        };
        let mut next = xorshift32_step(seeded);

        let entropy = if cpu_id < MAX_CPUS {
            ENTROPY_POOL[cpu_id].load(Ordering::Relaxed)
        } else {
            GLOBAL_ENTROPY.load(Ordering::Relaxed)
        };
        next ^= entropy as u32;
        next ^= (entropy >> 32) as u32;
        next = next.max(FALLBACK_NONZERO_SEED);

        match seed_ptr.compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire) {
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
    let zone = u32::MAX - (u32::MAX % span);
    let mut secure = true;

    loop {
        let (sample, sample_secure) = best_effort_secure_u32();
        secure &= sample_secure;
        if sample < zone {
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
    let zone = u32::MAX - (u32::MAX % max);
    loop {
        let sample = next_u32();
        if sample < zone {
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
    let mut current = DETERMINISTIC_SEED.load(Ordering::Acquire);
    loop {
        let seeded = if current == 0 { 0xA5A5_A5A5 } else { current };
        let next = xorshift32_step(seeded).max(1);
        match DETERMINISTIC_SEED.compare_exchange(
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

// Legacy uyumluluğu için boş fonksiyon; per-CPU entropi önceden init() ile kurulur
pub fn init_per_cpu_entropy(_cpu_count: u32) {}
