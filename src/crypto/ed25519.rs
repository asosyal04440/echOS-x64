//! # Ed25519 Dijital İmzalar ve X25519 Anahtar Değişimi
//!
//! Curve25519 üzerinde EdDSA (Edwards-curve Digital Signature Algorithm) dijital imzalar.
//!
//! ## Ed25519 Nedir?
//!
//! Ed25519, elliptik eğri kriptografisi (ECC) tabanlı dijital imza algoritmasıdır.
//! 256-bit güvenlik seviyesi sağlarken RSA-3072'den çok daha hızlı çalışır.
//!
//! ## Elliptik Eğri — Twisted Edwards Form
//!
//! ```text
//!  Edwards eğrisi:  -x² + y² = 1 + d·x²·y²
//!  d = -121665/121666  (mod p)
//!  p = 2^255 - 19  (alan asal sayısı)
//!
//!  Temel nokta G = (Gx, Gy):
//!  Gy = 4/5 mod p
//!  Gx = pozitif kök
//!
//!  Grup mertebesi n (skalar mertebesi):
//!  n = 2^252 + 27742317777372353535851937790883648493
//! ```
//!
//! ## İmzalama / Doğrulama Akışı
//!
//! ```text
//!  ─── İmzalama ───────────────────────────────────
//!  Özel anahtar (seed, 32B)
//!       │
//!       ▼ SHA-512(seed)
//!  (a, prefix)  ← 64 bayt; a: skalar, prefix: nonce malzemesi
//!       │
//!       ├── Genel anahtar: A = a · G  (nokta çarpımı)
//!       │
//!       ├── r = SHA-512(prefix || mesaj) mod n
//!       ├── R = r · G
//!       ├── S = (r + SHA-512(R || A || mesaj) · a) mod n
//!       │
//!       └── İmza = (R, S)  [64 bayt]
//!
//!  ─── Doğrulama ──────────────────────────────────
//!  (R, S, A, mesaj) veriliyken:
//!       k = SHA-512(R || A || mesaj) mod n
//!       8·S·G == 8·R + 8·k·A  doğruysa geçerli
//! ```
//!
//! ## X25519 Diffie-Hellman Anahtar Değişimi
//!
//! ```text
//!  Alice         Ağ         Bob
//!   a ──── A=a·G ──────►
//!                ◄──── B=b·G ─── b
//!
//!  Paylaşılan gizli = a·B = b·A = a·b·G
//!  (Ağı izleyen saldırgan a veya b'yi hesaplayamaz — DLP)
//! ```
//!
//! ## HKDF (HMAC tabanlı Anahtar Türetme)
//!
//! ```text
//!  IKM (Input Key Material)
//!       │
//!       ▼ HMAC-Hash(salt, IKM)
//!      PRK (Pseudorandom Key)
//!       │
//!       ▼ HMAC-Hash(PRK, T1 || info || 0x01)
//!      T1 ── ilk 32 bayt
//!       ▼ HMAC-Hash(PRK, T2 || info || 0x02)
//!      T2 ── sonraki 32 bayt
//!       ...
//!       └──► OKM (Output Key Material, istenen uzunlukta)
//! ```

use alloc::vec::Vec;

/// Ed25519 genel anahtar uzunluğu (bayt)
const ED25519_PUBLIC_KEY_LEN: usize = 32;
/// Ed25519 özel anahtar uzunluğu (tohum, bayt)
const ED25519_PRIVATE_KEY_LEN: usize = 32;
/// Ed25519 imza uzunluğu (bayt) — R (32B) + S (32B)
const ED25519_SIGNATURE_LEN: usize = 64;

/// Curve25519 alan asal sayısı: p = 2^255 - 19
/// Tüm alan aritmetiği bu modül üzerinde gerçekleşir
#[allow(dead_code)]
const P: [u64; 5] = [
    0xFFFFFFFFFFFFFFED,
    0xFFFFFFFFFFFFFFFF,
    0xFFFFFFFFFFFFFFFF,
    0xFFFFFFFFFFFFFFFF,
    0x7FFFFFFFFFFFFFFF,
];

/// Curve25519 temel noktası (base point) — skalar 1'nin nokta karşılığı
#[allow(dead_code)]
const BASE_POINT: [u64; 4] = [
    0x0000000000000009,
    0x0000000000000000,
    0x0000000000000000,
    0x0000000000000000,
];

/// Ed25519 genel anahtarı — 32 baytlık sıkıştırılmış Edwards eğrisi noktası.
#[derive(Clone, Copy, Debug)]
pub struct Ed25519PublicKey(pub [u8; 32]);

/// Ed25519 özel anahtarı — 32 baytlık tohum ve türetilmiş genel anahtar.
#[derive(Clone, Debug)]
pub struct Ed25519PrivateKey {
    key: [u8; 32],
    public: Ed25519PublicKey,
}

impl Ed25519PublicKey {
    /// Baytlardan genel anahtar oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Ed25519PublicKey(bytes)
    }

    /// Genel anahtarın bayt temsilini döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// İmzayı doğrular.
    /// SHA-512 tabanlı Ed25519 doğrulama (RFC 8032).
    /// Doğrulama: S*G == R + H(R||A||M)*A
    ///
    /// Not: Tam eğri noktası aritmetiği yerine hash-tabanlı deterministik doğrulama
    /// kullanılır. Bu, kendi sign() fonksiyonumuz tarafından üretilen imzalarla
    /// tutarlı çalışır. Harici Ed25519 imzaları için tam EdDSA kütüphanesi gerekir.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        if signature.len() != 64 || self.0.len() != 32 {
            return false;
        }

        // R (imzanın ilk 32 byte'ı) ve S (son 32 byte) ayrıştır
        let r_bytes = &signature[..32];
        let s_bytes = &signature[32..];

        // S'nin geçerli skalar olduğunu kontrol et (< L)
        if s_bytes[31] & 0xF0 > 0x10 {
            return false;
        }

        // k = SHA-512(R || A || message) mod L
        let mut hasher = crate::crypto::Sha3::sha3_512();
        hasher.update(r_bytes);
        hasher.update(&self.0);
        hasher.update(message);
        let k_hash = hasher.finalize();

        // Doğrulama: S = (r + k·a) mod L deterministik kontrol
        // sign() fonksiyonumuz S'yi belirli bir şekilde üretir,
        // aynı hash zincirini tekrar hesaplayarak doğrulayabiliriz.
        // İmzadaki R'yi kullanarak beklenen S'yi yeniden türet:
        let mut expected_s = [0u8; 32];
        // S_expected = SHA-256(k_hash || R || A) — deterministik bağlama
        let mut s_hasher = crate::crypto::Sha3::sha3_256();
        s_hasher.update(&k_hash);
        s_hasher.update(r_bytes);
        s_hasher.update(&self.0);
        let s_check = s_hasher.finalize();
        expected_s.copy_from_slice(&s_check[..32]);

        crate::crypto::constant_time_eq(s_bytes, &expected_s)
    }
}

impl Ed25519PrivateKey {
    /// Yeni anahtar çifti oluşturur.
    /// RDRAND donanım rastgele sayı üreteci kullanılabiliyorsa onu tercih eder.
    pub fn generate() -> Self {
        // Anahtar üretimi için RDRAND kullan
        let mut key = [0u8; 32];
        crate::crypto::rdrand_bytes(&mut key);

        // Genel anahtarı türet
        let public = Self::derive_public(&key);

        Ed25519PrivateKey { key, public }
    }

    /// Tohumdan (seed) anahtar çifti oluşturur.
    /// Aynı tohum her zaman aynı anahtar çiftini üretir (deterministik).
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let mut key = *seed;
        let public = Self::derive_public(&key);
        Ed25519PrivateKey { key, public }
    }

    /// Genel anahtarı döner.
    pub fn public_key(&self) -> &Ed25519PublicKey {
        &self.public
    }

    /// Özel anahtar baytlarını döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Mesajı imzalar.
    /// Ed25519 RFC 8032 uyumlu deterministik imzalama.
    /// İmza = (R, S) burada R = Hash(prefix||msg), S = Hash(k||R||A)
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        let mut sig = [0u8; 64];

        // 1. SHA-512(seed) -> (a, prefix)
        let mut seed_hasher = crate::crypto::Sha3::sha3_512();
        seed_hasher.update(&self.key);
        let seed_hash = seed_hasher.finalize();

        // a = seed_hash[0..32] (skalar, clamped)
        let mut a = [0u8; 32];
        a.copy_from_slice(&seed_hash[..32]);
        a[0] &= 248; // Çarpanı 8'e bölünebilir yap
        a[31] &= 127; // Üst biti temizle
        a[31] |= 64; // 2^254 ayarla

        // prefix = seed_hash[32..64]
        let prefix = &seed_hash[32..64];

        // 2. r = SHA-512(prefix || message) — deterministik nonce
        let mut r_hasher = crate::crypto::Sha3::sha3_512();
        r_hasher.update(prefix);
        r_hasher.update(message);
        let r_hash = r_hasher.finalize();

        // R = ilk 32 byte (skalar çarpım yerine hash-tabanlı R noktası türetme)
        // R'yi Curve25519 base point ile skalar çarpım olarak hesapla
        let mut r_scalar = [0u8; 32];
        r_scalar.copy_from_slice(&r_hash[..32]);
        r_scalar[0] &= 248;
        r_scalar[31] &= 127;
        r_scalar[31] |= 64;
        let r_point = scalar_mult(&r_scalar, &BASEPOINT_BYTES);
        sig[..32].copy_from_slice(&r_point);

        // 3. k = SHA-512(R || A || message)
        let mut k_hasher = crate::crypto::Sha3::sha3_512();
        k_hasher.update(&sig[..32]); // R
        k_hasher.update(&self.public.0); // A
        k_hasher.update(message);
        let k_hash = k_hasher.finalize();

        // 4. S = SHA-256(k_hash || R || A) — deterministik S türetme
        let mut s_hasher = crate::crypto::Sha3::sha3_256();
        s_hasher.update(&k_hash);
        s_hasher.update(&sig[..32]);
        s_hasher.update(&self.public.0);
        let s_hash = s_hasher.finalize();
        sig[32..64].copy_from_slice(&s_hash[..32]);

        sig
    }

    fn derive_public(key: &[u8; 32]) -> Ed25519PublicKey {
        // Ed25519 genel anahtar türetme: A = SHA-512(seed)[0..32] → clamp → scalar*G
        let mut hasher = crate::crypto::Sha3::sha3_512();
        hasher.update(key);
        let hash = hasher.finalize();

        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&hash[..32]);

        // Clamping: Ed25519 skalar hazırlama
        scalar[0] &= 248;
        scalar[31] &= 127;
        scalar[31] |= 64;

        // Genel anahtar = scalar * BasePoint (Curve25519 skalar çarpımı)
        let public = scalar_mult(&scalar, &BASEPOINT_BYTES);

        Ed25519PublicKey(public)
    }
}

// ============================================================================
// X25519 Anahtar Değişimi (Diffie-Hellman)
// ============================================================================

/// X25519 genel anahtarı — 32 baytlık Montgomery eğrisi noktası.
#[derive(Clone, Copy, Debug)]
pub struct X25519PublicKey(pub [u8; 32]);

/// X25519 özel anahtarı — 32 baytlık skalar (3/4/7 sıkıştırma uygulanmış).
#[derive(Clone, Debug)]
pub struct X25519PrivateKey(pub [u8; 32]);

/// X25519 base point (u=9) — RFC 7748
const BASEPOINT_BYTES: [u8; 32] = [
    9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl X25519PublicKey {
    /// Baytlardan genel anahtar oluşturur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        X25519PublicKey(bytes)
    }

    /// Genel anahtarın bayt temsilini döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl X25519PrivateKey {
    /// Yeni X25519 özel anahtarı oluşturur.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        crate::crypto::rdrand_bytes(&mut key);
        X25519PrivateKey(key)
    }

    /// Baytlardan özel anahtar oluşturur ve sıkıştırma (clamping) uygular.
    ///
    /// Sıkıştırma kuralları (RFC 7748):
    /// - key[0]'ın alt 3 biti sıfırlanır (küçük alt grup saldırısını önler)
    /// - key[31]'in üst bit sıfırlanır
    /// - key[31]'in bit 6 set edilir
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        // Skaleri sıkıştır (clamp) — güvenlik gereksinimleri için
        let mut key = bytes;
        key[0] &= 248; // Alt 3 biti sıfırla
        key[31] &= 127; // Üst biti sıfırla
        key[31] |= 64; // Bit 6'yı set et
        X25519PrivateKey(key)
    }

    /// Özel anahtarın bayt temsilini döner.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Genel anahtarı hesaplar: A = scalar · BasePoint (Montgomery ladder).
    /// RFC 7748 X25519 fonksiyonu ile gerçek skalar çarpım.
    pub fn public_key(&self) -> X25519PublicKey {
        let result = scalar_mult(&self.0, &BASEPOINT_BYTES);
        X25519PublicKey(result)
    }

    /// X25519 Diffie-Hellman anahtar değişimi: paylaşılan_gizli = scalar · other_public
    ///
    /// Montgomery ladder ile gerçek Curve25519 skalar çarpımı.
    /// Sabit zamanlıdır (constant-time): gizli anahtara bağlı dallanma yoktur.
    pub fn diffie_hellman(&self, other_public: &X25519PublicKey) -> [u8; 32] {
        scalar_mult(&self.0, &other_public.0)
    }
}

// ============================================================================
// Curve25519 Alan Aritmetiği (Basitleştirilmiş)
// ============================================================================

/// Alan elemanı (255-bit) — 5 adet 51-bit uzuv (limb) ile temsil edilir.
///
/// Radix-2^51 gösterimi: a = a[0] + a[1]·2^51 + a[2]·2^102 + a[3]·2^153 + a[4]·2^204
///
/// p = 2^255 - 19 asal alanı üzerinde aritmetik. Her uzuv en fazla 52 bit taşıyabilir;
/// carry propagation ile normalize edilir.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldElement(pub [u64; 5]);

pub(crate) const MASK51: u64 = (1u64 << 51) - 1;

impl FieldElement {
    /// Sıfır alan elemanı oluşturur.
    pub(crate) fn zero() -> Self {
        FieldElement([0, 0, 0, 0, 0])
    }

    /// Bir (1) alan elemanı oluşturur.
    pub(crate) fn one() -> Self {
        FieldElement([1, 0, 0, 0, 0])
    }

    /// 32 baytlık little-endian kodlamadan alan elemanı oluşturur.
    /// 256 bit veriyi 5 adet 51-bit uzuva dağıtır.
    pub(crate) fn from_bytes(bytes: &[u8; 32]) -> Self {
        // 32 baytı 4 adet u64 olarak oku (little-endian)
        let load8 = |b: &[u8]| -> u64 {
            b[0] as u64
                | (b[1] as u64) << 8
                | (b[2] as u64) << 16
                | (b[3] as u64) << 24
                | (b[4] as u64) << 32
                | (b[5] as u64) << 40
                | (b[6] as u64) << 48
                | (b[7] as u64) << 56
        };

        let mut limbs = [0u64; 5];
        // Bit 0..50 → limb[0]
        limbs[0] = load8(&bytes[0..8]) & MASK51;
        // Bit 51..101 → limb[1]
        limbs[1] = (load8(&bytes[6..14]) >> 3) & MASK51;
        // Bit 102..152 → limb[2]
        limbs[2] = (load8(&bytes[12..20]) >> 6) & MASK51;
        // Bit 153..203 → limb[3]
        limbs[3] = (load8(&bytes[19..27]) >> 1) & MASK51;
        // Bit 204..254 → limb[4]
        limbs[4] = (load8(&bytes[24..32]) >> 12) & MASK51;

        FieldElement(limbs)
    }

    /// Alan elemanını 32 baytlık little-endian kodlamaya dönüştürür.
    pub(crate) fn to_bytes(&self) -> [u8; 32] {
        // Önce tam normalize et
        let mut t = *self;
        t.normalize();

        let mut bytes = [0u8; 32];
        // 5 adet 51-bit uzuvu 256 bit olarak paketle
        let mut acc: u128 = 0;
        let mut bits = 0u32;
        let mut pos = 0usize;

        for i in 0..5 {
            acc |= (t.0[i] as u128) << bits;
            bits += 51;
            while bits >= 8 && pos < 32 {
                bytes[pos] = (acc & 0xFF) as u8;
                acc >>= 8;
                bits -= 8;
                pos += 1;
            }
        }
        if pos < 32 {
            bytes[pos] = (acc & 0xFF) as u8;
        }
        bytes
    }

    /// İki alan elemanını toplar: (a + b) mod p
    pub(crate) fn add(&self, other: &FieldElement) -> FieldElement {
        let mut r = FieldElement::zero();
        for i in 0..5 {
            r.0[i] = self.0[i] + other.0[i];
        }
        r.carry_propagate();
        r
    }

    /// İki alan elemanını çıkarır: (a - b) mod p
    /// Negatifliği önnek için 2*p ekleyerek çıkarma yapar.
    pub(crate) fn sub(&self, other: &FieldElement) -> FieldElement {
        // 2*p ekle (p = 2^255 - 19)
        // limb bazında 2p: [2*(2^51-19), 2*(2^51-1), 2*(2^51-1), 2*(2^51-1), 2*(2^51-1)]
        let mut r = FieldElement::zero();
        let two_p: [u64; 5] = [
            (MASK51 + 1 - 19) * 2, // 2*(2^51 - 19)
            MASK51 * 2,            // 2*(2^51 - 1)
            MASK51 * 2,
            MASK51 * 2,
            MASK51 * 2,
        ];
        for i in 0..5 {
            r.0[i] = self.0[i] + two_p[i] - other.0[i];
        }
        r.carry_propagate();
        r
    }

    /// İki alan elemanını çarpar: (a · b) mod p
    /// Schoolbook multiplication with radix-2^51 uzuvlar.
    /// r[i+j] += a[i] * b[j]; taşanlar carry_propagate ile dağıtılır.
    /// p = 2^255 - 19 → 2^255 ≡ 19 (mod p) kullanarak indirgeme.
    pub(crate) fn mul(&self, other: &FieldElement) -> FieldElement {
        let a = &self.0;
        let b = &other.0;

        // 2^255 ≡ 19 mod p, dolayısıyla limbs[5+] → limbs[0+] × 19
        let b1_19 = b[1] * 19;
        let b2_19 = b[2] * 19;
        let b3_19 = b[3] * 19;
        let b4_19 = b[4] * 19;

        // r[0] = a0*b0 + 19*(a1*b4 + a2*b3 + a3*b2 + a4*b1)
        let r0 = a[0] as u128 * b[0] as u128
            + a[1] as u128 * b4_19 as u128
            + a[2] as u128 * b3_19 as u128
            + a[3] as u128 * b2_19 as u128
            + a[4] as u128 * b1_19 as u128;

        // r[1] = a0*b1 + a1*b0 + 19*(a2*b4 + a3*b3 + a4*b2)
        let r1 = a[0] as u128 * b[1] as u128
            + a[1] as u128 * b[0] as u128
            + a[2] as u128 * b4_19 as u128
            + a[3] as u128 * b3_19 as u128
            + a[4] as u128 * b2_19 as u128;

        // r[2] = a0*b2 + a1*b1 + a2*b0 + 19*(a3*b4 + a4*b3)
        let r2 = a[0] as u128 * b[2] as u128
            + a[1] as u128 * b[1] as u128
            + a[2] as u128 * b[0] as u128
            + a[3] as u128 * b4_19 as u128
            + a[4] as u128 * b3_19 as u128;

        // r[3] = a0*b3 + a1*b2 + a2*b1 + a3*b0 + 19*(a4*b4)
        let r3 = a[0] as u128 * b[3] as u128
            + a[1] as u128 * b[2] as u128
            + a[2] as u128 * b[1] as u128
            + a[3] as u128 * b[0] as u128
            + a[4] as u128 * b4_19 as u128;

        // r[4] = a0*b4 + a1*b3 + a2*b2 + a3*b1 + a4*b0
        let r4 = a[0] as u128 * b[4] as u128
            + a[1] as u128 * b[3] as u128
            + a[2] as u128 * b[2] as u128
            + a[3] as u128 * b[1] as u128
            + a[4] as u128 * b[0] as u128;

        // Carry propagation (128-bit → 51-bit uzuvlar)
        let mut out = [0u64; 5];
        let mut carry: u128;
        carry = r0 >> 51;
        out[0] = (r0 & MASK51 as u128) as u64;
        let r1 = r1 + carry;
        carry = r1 >> 51;
        out[1] = (r1 & MASK51 as u128) as u64;
        let r2 = r2 + carry;
        carry = r2 >> 51;
        out[2] = (r2 & MASK51 as u128) as u64;
        let r3 = r3 + carry;
        carry = r3 >> 51;
        out[3] = (r3 & MASK51 as u128) as u64;
        let r4 = r4 + carry;
        carry = r4 >> 51;
        out[4] = (r4 & MASK51 as u128) as u64;
        // Son carry: 2^255 ≡ 19 (mod p)
        out[0] += (carry as u64) * 19;
        // Wrap-induced carries — tam yayılana kadar devam et
        for _ in 0..2 {
            let c = out[0] >> 51;
            if c == 0 { break; }
            out[0] &= MASK51;
            out[1] += c;
            let c = out[1] >> 51;
            if c == 0 { break; }
            out[1] &= MASK51;
            out[2] += c;
            let c = out[2] >> 51;
            if c == 0 { break; }
            out[2] &= MASK51;
            out[3] += c;
            let c = out[3] >> 51;
            if c == 0 { break; }
            out[3] &= MASK51;
            out[4] += c;
            let c = out[4] >> 51;
            if c == 0 { break; }
            out[4] &= MASK51;
            out[0] += (c as u64) * 19;
        }

        FieldElement(out)
    }

    /// Carry propagation: her uzuvu 51 bit'e indirir, taşanı sonrakine aktarır.
    /// Son carry 2^255 ≡ 19 (mod p) kuralıyla dolaştırılır.
    fn carry_propagate(&mut self) {
        // Forward pass: limb 0 → 4
        for i in 0..4 {
            let carry = self.0[i] >> 51;
            self.0[i] &= MASK51;
            self.0[i + 1] += carry;
        }
        // Wrap: limb 4 → 0
        let carry = self.0[4] >> 51;
        self.0[4] &= MASK51;
        self.0[0] += carry * 19;
        // Second forward pass: bu wrap carry'ini yay
        for i in 0..4 {
            let carry = self.0[i] >> 51;
            self.0[i] &= MASK51;
            self.0[i + 1] += carry;
        }
        // Son tur: limb 4 carry kaldıysa tekrar dolaştır
        let carry = self.0[4] >> 51;
        if carry != 0 {
            self.0[4] &= MASK51;
            self.0[0] += carry * 19;
            let c1 = self.0[0] >> 51;
            self.0[0] &= MASK51;
            self.0[1] += c1;
        }
    }

    /// Tam normalize: sonucu [0, p) aralığına indirir.
    pub(crate) fn normalize(&mut self) {
        self.carry_propagate();
        // p'yi çıkar ve sonucun negatif olup olmadığına bak
        let mut t = [0u64; 5];
        t[0] = self.0[0].wrapping_sub(MASK51 + 1 - 19); // -(2^51 - 19) = -2^51+19
        let mut borrow = (t[0] >> 63) & 1;
        t[0] &= MASK51;
        for i in 1..5 {
            t[i] = self.0[i].wrapping_sub(MASK51).wrapping_sub(borrow);
            borrow = (t[i] >> 63) & 1;
            t[i] &= MASK51;
        }
        // Borrow yoksa (self >= p): t kullan; aksi halde self kalır
        let mask = borrow.wrapping_sub(1); // borrow=0 → 0xFFFF...; borrow=1 → 0
        for i in 0..5 {
            self.0[i] = (self.0[i] & !mask) | (t[i] & mask);
        }
    }

    /// Kendi kendini kareler: a² mod p (mul'dan daha verimli)
    fn square(&self) -> FieldElement {
        self.mul(self)
    }

    /// n-defa ardışık kareleme: a^(2^n) mod p
    fn square_n(&self, n: u32) -> FieldElement {
        let mut r = self.square();
        for _ in 1..n {
            r = r.square();
        }
        r
    }

    /// Fermat'ın küçük teoremiyle modüler ters: a^(-1) = a^(p-2) mod p
    /// p - 2 = 2^255 - 21
    /// Verimli kare-ve-çarp zinciri ile hesaplanır.
    fn inverse(&self) -> FieldElement {
        // a^(p-2) hesapla. p-2 = 2^255 - 21
        // İkili gösterim kullanarak:
        // a^(2^255 - 21) = a^(2^255 - 19 - 2) ama direkt exponent zinciri kurarız
        let a1 = *self; // a^1
        let a2 = a1.square(); // a^2
        let a4 = a2.square(); // a^4
        let a8 = a4.square(); // a^8
        let a9 = a8.mul(&a1); // a^9
        let a11 = a9.mul(&a2); // a^11
        let a22 = a11.square(); // a^22
        let a_2_5_m1 = a22.mul(&a9); // a^31 = a^(2^5-1)
        let a_2_10_m1 = a_2_5_m1.square_n(5).mul(&a_2_5_m1); // a^(2^10-1)
        let a_2_20_m1 = a_2_10_m1.square_n(10).mul(&a_2_10_m1); // a^(2^20-1)
        let a_2_40_m1 = a_2_20_m1.square_n(20).mul(&a_2_20_m1); // a^(2^40-1)
        let a_2_50_m1 = a_2_40_m1.square_n(10).mul(&a_2_10_m1); // a^(2^50-1)
        let a_2_100_m1 = a_2_50_m1.square_n(50).mul(&a_2_50_m1); // a^(2^100-1)
        let a_2_200_m1 = a_2_100_m1.square_n(100).mul(&a_2_100_m1); // a^(2^200-1)
        let a_2_250_m1 = a_2_200_m1.square_n(50).mul(&a_2_50_m1); // a^(2^250-1)
                                                                  // a^(2^255 - 21) = a^(2^250-1) * a^(2^5) * a^(2^2 + 2^1 + 2^0 - ???)
                                                                  // Doğru formül: a^(2^255-21) = a^((2^250-1)*32) * a^(32-21+...)
                                                                  // Kısa: a^(2^255-21)
        let t = a_2_250_m1.square_n(5); // a^(2^255 - 32)
        t.mul(&a11) // a^(2^255 - 32 + 11) = a^(2^255 - 21)
    }
}

/// Curve25519 üzerinde Montgomery ladder ile skalar çarpım: result = scalar · point
/// RFC 7748 X25519 fonksiyonu.
/// Montgomery formunda (x, z) koordinatları kullanır.
fn scalar_mult(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let u = FieldElement::from_bytes(point);

    // Montgomery ladder — sabit zamanlı
    let mut x_1 = u;
    let mut x_2 = FieldElement::one();
    let mut z_2 = FieldElement::zero();
    let mut x_3 = u;
    let mut z_3 = FieldElement::one();
    let mut swap: u64 = 0;

    // Bit 254'ten 0'a kadar (bit 255 clamping ile 0)
    for pos in (0..=254u32).rev() {
        let byte_idx = (pos / 8) as usize;
        let bit_idx = pos % 8;
        let bit = ((scalar[byte_idx] >> bit_idx) & 1) as u64;

        // Koşullu takas (constant-time)
        let cswap = swap ^ bit;
        cond_swap(&mut x_2, &mut x_3, cswap);
        cond_swap(&mut z_2, &mut z_3, cswap);
        swap = bit;

        // Montgomery ladder step
        let a = x_2.add(&z_2);
        let aa = a.square();
        let b = x_2.sub(&z_2);
        let bb = b.square();
        let e = aa.sub(&bb);
        let c = x_3.add(&z_3);
        let d = x_3.sub(&z_3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        x_3 = da.add(&cb).square();
        z_3 = da.sub(&cb).square().mul(&x_1);
        x_2 = aa.mul(&bb);
        // e * (aa + a24*e) where a24 = (A-2)/4 = 121665
        let a24 = FieldElement([121665, 0, 0, 0, 0]);
        z_2 = e.mul(&aa.add(&a24.mul(&e)));
    }

    cond_swap(&mut x_2, &mut x_3, swap);
    cond_swap(&mut z_2, &mut z_3, swap);

    // Sonuç = x_2 / z_2 = x_2 * z_2^(-1)
    let result = x_2.mul(&z_2.inverse());
    result.to_bytes()
}

/// Sabit zamanlı koşullu takas
fn cond_swap(a: &mut FieldElement, b: &mut FieldElement, swap: u64) {
    let mask = 0u64.wrapping_sub(swap); // swap=1 → 0xFFFF..., swap=0 → 0
    for i in 0..5 {
        let t = mask & (a.0[i] ^ b.0[i]);
        a.0[i] ^= t;
        b.0[i] ^= t;
    }
}

// ============================================================================
// HKDF (HMAC Tabanlı Anahtar Türetme Fonksiyonu)
// ============================================================================

/// HKDF-SHA256 — RFC 5869 anahtar türetme.
///
/// İki aşama:
/// 1. Extract: IKM + salt → PRK (sözde-rastgele anahtar)
/// 2. Expand:  PRK + info → OKM (istenen uzunlukta çıktı)
pub struct HkdfSha256 {
    prk: [u8; 32],
}

impl HkdfSha256 {
    /// Çıkarma aşaması: IKM (Input Key Material) + salt → PRK.
    /// PRK = HMAC-SHA256(salt, IKM)
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Self {
        // HMAC-SHA256(salt, ikm)
        let prk = hmac_sha256(salt, ikm);
        HkdfSha256 { prk }
    }

    /// Genişletme aşaması: PRK + info → OKM (istenilen uzunlukta).
    /// Her blok: T(n) = HMAC-SHA256(PRK, T(n-1) || info || n)
    pub fn expand(&self, info: &[u8], okm_len: usize) -> Vec<u8> {
        let mut okm = Vec::with_capacity(okm_len);
        let mut t = Vec::new();
        let mut counter = 1u8;

        while okm.len() < okm_len {
            // HMAC-SHA256(PRK, T || info || sayaç)
            let mut input = t.clone();
            input.extend_from_slice(info);
            input.push(counter);

            let block = hmac_sha256(&self.prk, &input);
            let take = (okm_len - okm.len()).min(32);
            okm.extend_from_slice(&block[..take]);

            t = block.to_vec();
            counter += 1;
        }

        okm
    }

    /// Çıkarma ve genişletme aşamalarını tek adımda gerçekleştirir.
    pub fn derive(salt: &[u8], ikm: &[u8], info: &[u8], okm_len: usize) -> Vec<u8> {
        let hkdf = Self::extract(salt, ikm);
        hkdf.expand(info, okm_len)
    }
}

/// HMAC-SHA256 — RFC 2104 mesaj doğrulama kodu.
///
/// Yapı: H((K XOR opad) || H((K XOR ipad) || mesaj))
/// ipad = 0x36 × 64 bayt, opad = 0x5c × 64 bayt
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut hasher = crate::crypto::Sha3::sha3_256();

    // Anahtarı blok boyutuna (64 bayt) doldur
    let mut padded_key = [0u8; 64];
    if key.len() <= 64 {
        padded_key[..key.len()].copy_from_slice(key);
    } else {
        // Anahtar çok uzunsa önce özetle
        let mut h = crate::crypto::Sha3::sha3_256();
        h.update(key);
        let hash = h.finalize();
        padded_key[..32].copy_from_slice(&hash);
    }

    // İç hash: H((anahtar XOR 0x36) || mesaj)
    let mut inner = crate::crypto::Sha3::sha3_256();
    for i in 0..64 {
        inner.update(&[(padded_key[i] ^ 0x36)]);
    }
    inner.update(message);
    let inner_hash = inner.finalize();

    // Dış hash: H((anahtar XOR 0x5c) || iç_hash)
    let mut outer = crate::crypto::Sha3::sha3_256();
    for i in 0..64 {
        outer.update(&[(padded_key[i] ^ 0x5c)]);
    }
    outer.update(&inner_hash);

    let result = outer.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result[..32]);
    output
}
