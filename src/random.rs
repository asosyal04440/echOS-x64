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

/// Desteklenen maksimum CPU sayısı.
const MAX_CPUS: usize = 32;

/// CPU başına RNG tohum (seed) değerleri.
///
/// Her CPU kendi tohumunu korur: atomik kilit gerekmez, çekirdek başına izole durum sağlanır.
static SEEDS: [AtomicU32; MAX_CPUS] = [const { AtomicU32::new(123456789) }; MAX_CPUS];

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

/// RNG'yi başlangıç tohumuyla başlatır.
///
/// # Parametreler
/// - `seed`: Başlangıç değeri (örn: TSC'den alınabilir; 0 ise varsayılan kullanılır)
///
/// CPU 0 (BSP) için doğrudan tohum ayarlanır; diğer CPU'lar için altın oran
/// çarpanıyla (0x9E3779B9) benzersiz türev tohumlar hesaplanır.
/// Bu yaklaşım, her CPU'nun birbirinden bağımsız sayı üretmesini sağlar.
pub fn init(seed: u32) {
    let valid_seed = if seed == 0 { 123456789 } else { seed };
    // BSP (CPU 0) seed'ini ayarla
    SEEDS[0].store(valid_seed, Ordering::Relaxed);

    // Diğer CPU'lar için seed'i türet
    for i in 1..MAX_CPUS {
        let sub_seed = valid_seed.wrapping_add((i as u32).wrapping_mul(0x9E3779B9)); // Altın oran
        SEEDS[i].store(sub_seed, Ordering::Relaxed);
    }
}

/// Entropi havuzuna yeni bir değer karıştırır.
///
/// Döndürme (rotate) ve XOR işlemleriyle girişin tüm bitlerini yayar;
/// küçük giriş farklılıkları büyük çıkış farklılıklarına yol açar (çığ etkisi).
pub fn add_entropy(value: u64) {
    // Entropi değerini bit döndürme ve XOR ile karıştır (avalanche etkisi)
    let mixed = value ^ value.rotate_left(17) ^ value.rotate_right(23);
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;

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
    let cpu_id = crate::cpu::smp::current_cpu_id() as usize;
    // Geçerli CPU'nun tohumunu seç; aralık dışıysa BSP'ye geri düş
    let seed_ptr = if cpu_id < MAX_CPUS {
        &SEEDS[cpu_id]
    } else {
        &SEEDS[0] // BSP'ye geri düş
    };

    let mut x = seed_ptr.load(Ordering::Relaxed);
    if x == 0 {
        // Sıfır tohum Xorshift'i bozar; CPU ID ekleyerek kaç
        x = 123456789 + (cpu_id as u32);
    }

    // Xorshift algoritması: üç adımda iyi dağılım
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;

    // CPU'ya özgü entropi havuzuyla ekstra rastlantısallık ekle
    let entropy = if cpu_id < MAX_CPUS {
        ENTROPY_POOL[cpu_id].load(Ordering::Relaxed)
    } else {
        GLOBAL_ENTROPY.load(Ordering::Relaxed)
    };

    // Entropinin hem alt hem üst 32 bitini dahil et
    x ^= entropy as u32;
    x ^= (entropy >> 32) as u32;

    // Yeni tohumu kaydet
    seed_ptr.store(x, Ordering::Relaxed);
    x
}

/// Rastgele bir u64 değeri üretir.
///
/// İki bağımsız `next_u32()` çağrısını birleştirerek 64 bitlik değer oluşturur.
pub fn rand_u64() -> u64 {
    let low = next_u32() as u64;
    let high = next_u32() as u64;
    (high << 32) | low
}

/// `[0, max)` aralığında rastgele bir sayı üretir.
///
/// `max == 0` ise sıfır döner (sıfıra bölünme koruması).
/// Büyük `max` değerlerinde modüler sapma oluşabilir; olasılık dağılımı tam düzgün değildir.
pub fn next_range(max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    next_u32() % max
}

/// Verilen tamponu (buffer) rastgele baytlarla doldurur.
///
/// Her 4 baytlık blok için `next_u32()` çağrısı yapılır.
/// Artan tampon boyutunda verimlidir.
pub fn fill_bytes(buf: &mut [u8]) {
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
    let mut x = DETERMINISTIC_SEED.load(Ordering::Relaxed);
    // Standart Xorshift32 parametreleri (13, 17, 5)
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    DETERMINISTIC_SEED.store(x, Ordering::Relaxed);
    x
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
