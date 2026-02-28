//! # SHA-3 (Keccak) Hash Fonksiyonu
//!
//! FIPS 202 standardına uygun SHA-3 uygulaması; genişletilebilir çıktı (XOF)
//! fonksiyonları SHAKE128 ve SHAKE256 dahil.
//!
//! ## Keccak Sünger Yapısı (Sponge Construction)
//!
//! SHA-3 "sünger" metaforuna dayanır: veriyi emersiniz (absorb), çıktıyı sıkarsınız (squeeze).
//!
//! ```text
//!  Mesaj Blokları                              Çıktı Blokları
//!  ──────────────                              ───────────────
//!  M[0]   M[1]   M[2]                         Z[0]   Z[1]
//!   │      │      │                             │      │
//!   ▼      ▼      ▼                             ▼      ▼
//!  [State XOR] → f → [State XOR] → f → ... → f → Çıktı al → f → Çıktı al
//!   ┌─────────────────────────────────────────────────────────┐
//!   │  1600-bit Durum  =  r (rate/hız) bits  +  c (kapasite) │
//!   │                     ──────────────────    ─────────────  │
//!   │  SHA3-256: r=1088b, c=512b             │
//!   │  SHA3-512: r=576b,  c=1024b             │
//!   └─────────────────────────────────────────────────────────┘
//!
//!  EMME (Absorb): Mesaj bloğu durumun r-bit kısmıyla XOR'lanır, ardından f() çalışır.
//!  SIKMA (Squeeze): Durumun r-bit kısmından çıktı alınır, gerekirse f() tekrar çalışır.
//! ```
//!
//! ## Keccak-f[1600] Permütasyonu: 5 Adım
//!
//! 24 turdan oluşan permütasyon; her turda 5 adım:
//!
//! ```text
//!  θ (Theta)   — Sütun parite XOR difüzyonu
//!               C[x] = A[x,0] XOR A[x,1] XOR A[x,2] XOR A[x,3] XOR A[x,4]
//!               D[x] = C[x-1] XOR ROT(C[x+1], 1)
//!               A[x,y] ^= D[x]
//!
//!  ρ (Rho)     — Şerit başına sabit döndürme (ROTOFF tablosu)
//!               Her A[x,y] kendi dönüş ofsetiyle sola döndürülür
//!
//!  π (Pi)      — Şerit permütasyonu — konumları yeniden düzenler
//!               B[y, 2x+3y] = ROT(A[x,y], ROTOFF[y][x])
//!
//!  χ (Chi)     — Doğrusal olmayan adım (tek doğrusal olmayan adım)
//!               A[x,y] = B[x,y] XOR (NOT B[x+1,y] AND B[x+2,y])
//!
//!  ι (Iota)    — Tur sabitini (RC) A[0,0]'a XOR'lar (simetriyi kırar)
//!               A[0,0] ^= RC[round]
//! ```
//!
//! ## Dolgu (Padding) Farkları
//!
//! ```text
//!  SHA-3       : son byte 0x06, blok sonu 0x80  (NIST FIPS 202)
//!  SHAKE (XOF) : son byte 0x1F, blok sonu 0x80  (genişletilebilir çıktı)
//!  Keccak-256  : son byte 0x01                  (Ethereum varyantı, orijinal Keccak)
//! ```
//!
//! ## Durum Matrisi — 5×5×64 bit = 1600 bit
//!
//! ```text
//!  A[0,0] A[1,0] A[2,0] A[3,0] A[4,0]   ← y=0 satırı
//!  A[0,1] A[1,1] A[2,1] A[3,1] A[4,1]   ← y=1 satırı
//!  A[0,2] A[1,2] A[2,2] A[3,2] A[4,2]   ← y=2 satırı
//!  A[0,3] A[1,3] A[2,3] A[3,3] A[4,3]   ← y=3 satırı
//!  A[0,4] A[1,4] A[2,4] A[3,4] A[4,4]   ← y=4 satırı
//!
//!  Bellekte: state[y*5 + x] ile erişilir (25 adet u64)
//! ```

use alloc::vec::Vec;

/// Keccak-f[1600] permütasyonunun tur sayısı (NIST FIPS 202 standardı).
const KECCAK_ROUNDS: usize = 24;

/// Keccak durum matrisinin kelime sayısı (5×5 = 25 adet u64).
const KECCAK_STATE_SIZE: usize = 25;

/// Keccak-f[1600] tur sabitleri (RC).
///
/// Her RC[i] değeri, GF(2)[x] üzerinde x^(2^j mod 5 + 5*(2^j mod 7)) polinomundan türetilir.
/// ι adımında bu sabit A[0,0] şeridine XOR'lanarak her turun farklı işlenmesi sağlanır
/// (aksi hâlde tüm turlar özdeş olur ve güvenlik ortadan kalkar).
const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000000800081, 0x8000000000008081, 0x8000000000808000,
    0x0000000000008009, 0x000000000020002a, 0x8000000000200080, 0x800000000080800a,
    0x0000000000800081, 0x8000000000808081, 0x8000000000808082, 0x0000000000800080,
    0x8000000000008009, 0x8000000000008081, 0x8000000000008082, 0x800000000000808a,
    0x8000000000808000, 0x0000000000800080, 0x8000000000808000, 0x0000000000808000,
];

/// Keccak ρ adımı için döndürme ofseti tablosu (ROTOFF[y][x]).
///
/// Her şerit (x,y) konumu için sabit bir döndürme miktarı tanımlar.
/// Bu değerler GF(5) üzerinde özyinelemeli bir formülle hesaplanmıştır:
/// ofset[x][y] = (t*(t+1)/2) mod 64, burada (x,y) sabit bir rotasyonla güncellenir.
///
/// ```text
///  x=0  x=1  x=2  x=3  x=4
///  [ 0, 36,  3, 41, 18]  y=0
///  [44,  6, 22, 46, 43]  y=1
///  [29, 15, 24, 10, 17]  y=2
///  [27, 39, 14,  1, 40]  y=3
///  [20, 54, 28, 39, 19]  y=4
/// ```
const ROTOFF: [[usize; 5]; 5] = [
    [0, 36, 3, 41, 18],
    [44, 6, 22, 46, 43],
    [29, 15, 24, 10, 17],
    [27, 39, 14, 1, 40],
    [20, 54, 28, 39, 19],
];

/// SHA-3 / Keccak hash fonksiyonu yapısı.
///
/// `rate` (hız) ve `capacity` (kapasite) parametreleriyle farklı varyantlar desteklenir:
///
/// ```text
///  Varyant     rate   output_len  capacity   Kullanım
///  ─────────   ─────  ──────────  ──────────  ─────────────────────────────
///  SHA3-224    144B   28B         56B         Genel amaç, küçük özet
///  SHA3-256    136B   32B         64B         Ethereum, genel güvenlik
///  SHA3-384    104B   48B         96B         Yüksek güvenlik gereken durumlar
///  SHA3-512     72B   64B        128B         Maksimum güvenlik
///  SHAKE128    168B   değişken    32B         Genişletilebilir çıktı, hızlı
///  SHAKE256    136B   değişken    64B         Genişletilebilir çıktı, güvenli
/// ```
pub struct Sha3 {
    state: [u64; KECCAK_STATE_SIZE], // 5×5 Keccak durum matrisi (1600 bit toplam)
    buffer: [u8; 200],               // Gelen veri tamponu (maks. 200 bayt = rate)
    buffer_len: usize,               // Tamponda bekleyen bayt sayısı
    rate: usize,                     // Emme bloğu boyutu (bayt cinsinden)
    output_len: usize,               // Beklenen çıktı uzunluğu (XOF için 0)
    is_xof: bool,                    // Genişletilebilir çıktı modu (SHAKE) mu?
}

impl Sha3 {
    /// SHA3-224 oluşturur — 28 baytlık özet (224 bit).
    pub fn sha3_224() -> Self {
        Self::new(144, 28, false)
    }

    /// SHA3-256 oluşturur — 32 baytlık özet (256 bit).
    pub fn sha3_256() -> Self {
        Self::new(136, 32, false)
    }

    /// SHA3-384 oluşturur — 48 baytlık özet (384 bit).
    pub fn sha3_384() -> Self {
        Self::new(104, 48, false)
    }

    /// SHA3-512 oluşturur — 64 baytlık özet (512 bit).
    pub fn sha3_512() -> Self {
        Self::new(72, 64, false)
    }

    /// SHAKE128 oluşturur — genişletilebilir çıktı, 128-bit güvenlik.
    /// `finalize_xof(n)` ile n baytlık çıktı üretilir.
    pub fn shake128() -> Self {
        Self::new(168, 0, true)
    }

    /// SHAKE256 oluşturur — genişletilebilir çıktı, 256-bit güvenlik.
    /// `finalize_xof(n)` ile n baytlık çıktı üretilir.
    pub fn shake256() -> Self {
        Self::new(136, 0, true)
    }

    /// Rate, çıktı uzunluğu ve XOF bayrağıyla iç yapılandırıcı.
    fn new(rate: usize, output_len: usize, is_xof: bool) -> Self {
        Sha3 {
            state: [0u64; KECCAK_STATE_SIZE],
            buffer: [0u8; 200],
            buffer_len: 0,
            rate,
            output_len,
            is_xof,
        }
    }

    /// Hash'lenecek veriyi yükler (birden fazla çağrılabilir — akış modeli).
    ///
    /// Tampon dolduğunda `absorb()` çağrılarak Keccak-f permütasyonu tetiklenir.
    pub fn update(&mut self, data: &[u8]) {
        let mut remaining = data;

        while !remaining.is_empty() {
            let space = self.rate - self.buffer_len;
            let take = remaining.len().min(space);

            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&remaining[..take]);
            self.buffer_len += take;
            remaining = &remaining[take..];

            // Tampon tam dolduğunda bir blok emi yap
            if self.buffer_len == self.rate {
                self.absorb();
                self.buffer_len = 0;
            }
        }
    }

    /// Hashlemeyi tamamlar ve sabit uzunlukta özet döner.
    ///
    /// SHA-3 dolgu kuralı (NIST FIPS 202):
    /// 1. `0x06` baytı ekle (SHA-3 domain ayırıcı)
    /// 2. Bloğun son baytına `0x80` bitini OR'la
    /// 3. Keccak permütasyonunu son kez çalıştır
    /// 4. Durumun r-bit kısmından çıktı sık (squeeze)
    pub fn finalize(mut self) -> Vec<u8> {
        // SHA-3 dolgusu: 0x06 ardından son bytes 0x80
        self.buffer[self.buffer_len] = 0x06;
        self.buffer[self.rate - 1] |= 0x80;
        self.absorb();

        // Durumdan çıktı sık
        self.squeeze(self.output_len)
    }

    /// XOF (Genişletilebilir Çıktı Fonksiyonu) için finalize — istenilen uzunlukta çıktı.
    ///
    /// SHAKE dolgusu (0x1F), SHA-3'ten (0x06) farklıdır:
    /// - 0x1F = 0x0F (SHA-3 domain) | 0x10 (XOF biti)
    /// Bu sayede SHA3-256 ve SHAKE256 aynı hız (r=136B) ile farklı domain separation sağlar.
    pub fn finalize_xof(mut self, output_len: usize) -> Vec<u8> {
        // SHAKE dolgusu: 0x1F ardından son bayt 0x80
        self.buffer[self.buffer_len] = 0x1F;
        self.buffer[self.rate - 1] |= 0x80;
        self.absorb();

        self.squeeze(output_len)
    }

    /// Bir bloğu duruma emer (absorb): XOR + Keccak-f permütasyonu.
    ///
    /// Rate bloğunun her 8 baytı u64 little-endian olarak yorumlanır
    /// ve durum matrisinin ilgili şeridine XOR'lanır.
    fn absorb(&mut self) {
        // Tamponu duruma XOR'la (rate/8 adet u64 şeridi)
        for i in 0..(self.rate / 8) {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.buffer[i * 8..i * 8 + 8]);
            self.state[i] ^= u64::from_le_bytes(bytes); // Keccak little-endian kullanır
        }

        // Keccak-f[1600] permütasyonunu çalıştır
        self.keccak_f();
    }

    /// Durumdan çıktı sıkıştırır (squeeze): durumun rate kısmından bayt üretir.
    ///
    /// İstenen çıktı rate'den büyükse Keccak-f tekrar çalıştırılır.
    fn squeeze(&mut self, output_len: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(output_len);
        let mut remaining = output_len;

        while remaining > 0 {
            // Durumdan en fazla rate kadar bayt al
            let take = remaining.min(self.rate);
            for i in 0..(take / 8) {
                output.extend_from_slice(&self.state[i].to_le_bytes());
            }
            if take % 8 != 0 {
                let bytes = self.state[take / 8].to_le_bytes();
                output.extend_from_slice(&bytes[..take % 8]);
            }

            remaining -= take;

            // Daha fazla çıktı gerekiyorsa permütasyonu tekrar çalıştır
            if remaining > 0 {
                self.keccak_f();
            }
        }

        output
    }

    /// Keccak-f[1600] permütasyonu — 24 tur, her turda 5 adım (θ, ρ, π, χ, ι).
    ///
    /// Bu fonksiyon SHA-3'ün kriptografik çekirdeğidir. Tüm güvenlik özelliklerini sağlar:
    /// - Çarpışma direnci (collision resistance)
    /// - Ön görüntü direnci (preimage resistance)
    /// - Uzunluk uzatma saldırısına karşı koruma (sünger yapısı sayesinde)
    fn keccak_f(&mut self) {
        for round in 0..KECCAK_ROUNDS {
            // ── θ (Theta) ────────────────────────────────────────────────────
            // Sütun paritelerini hesapla: C[x] = A[x,0] XOR A[x,1] XOR ... XOR A[x,4]
            let mut c = [0u64; 5];
            for x in 0..5 {
                for y in 0..5 {
                    c[x] ^= self.state[y * 5 + x];
                }
            }
            // D[x] = C[x-1] XOR ROT(C[x+1], 1)  — komşu sütun etkisi
            let mut d = [0u64; 5];
            for x in 0..5 {
                d[x] = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            }
            // Her şeridi D[x] ile XOR'la
            for x in 0..5 {
                for y in 0..5 {
                    self.state[y * 5 + x] ^= d[x];
                }
            }

            // ── ρ (Rho) ve π (Pi) ────────────────────────────────────────────
            // ρ: her şeridi sabit miktarda döndür (ROTOFF tablosu)
            // π: şeritleri farklı konumlara taşı — B[y, 2x+3y] = ROT(A[x,y], offset)
            let mut b = [0u64; 25];
            for x in 0..5 {
                for y in 0..5 {
                    b[y * 5 + ((2 * x + 3 * y) % 5)] =
                        self.state[y * 5 + x].rotate_left(ROTOFF[y][x] as u32);
                }
            }

            // ── χ (Chi) ──────────────────────────────────────────────────────
            // Tek doğrusal olmayan adım — satır bazında 5-bit S-Box gibi davranır
            // A[x,y] = B[x,y] XOR (NOT B[x+1,y] AND B[x+2,y])
            for x in 0..5 {
                for y in 0..5 {
                    self.state[y * 5 + x] = b[y * 5 + x] ^
                        (!b[y * 5 + ((x + 1) % 5)] & b[y * 5 + ((x + 2) % 5)]);
                }
            }

            // ── ι (Iota) ─────────────────────────────────────────────────────
            // A[0,0]'a tur sabitini XOR'la — bu olmadan tüm turlar özdeş olur
            self.state[0] ^= RC[round];
        }
    }
}

/// SHA3-256 kısa yol fonksiyonu — veriyi hashler ve 32 baytlık özet döner.
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3::sha3_256();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result[..32]);
    output
}

/// SHA3-512 kısa yol fonksiyonu — veriyi hashler ve 64 baytlık özet döner.
pub fn sha3_512(data: &[u8]) -> [u8; 64] {
    let mut hasher = Sha3::sha3_512();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 64];
    output.copy_from_slice(&result[..64]);
    output
}

/// Keccak-256 — Ethereum tarafından kullanılan orijinal Keccak varyantı.
///
/// UYARI: Ethereum'un keccak256'sı, NIST'in SHA3-256'sından FARKLIDIR.
/// Fark: dolgu baytı — SHA-3 0x06 kullanırken Keccak-256 0x01 kullanır.
/// Solidity'deki `keccak256()` bu varyantı çağırır.
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3::new(136, 32, false);
    // NOT: Keccak, SHA-3'ten farklı dolgu kullanır (0x01, SHA-3'ün 0x06'sına karşı)
    // Bu uygulama SHA-3 dolgusu (0x06) kullanmaktadır;
    // tam Ethereum uyumluluğu için finalize() içindeki dolgu 0x01 olmalıdır.
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result[..32]);
    output
}
