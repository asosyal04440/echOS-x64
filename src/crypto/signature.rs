//! # RSA ve ECDSA İmza Doğrulama
//!
//! X.509 sertifika zinciri doğrulaması için RSA-PKCS#1 v1.5, RSA-PSS
//! ve ECDSA imza doğrulama uygulamaları.
//!
//! ## RSA İmza Doğrulama
//!
//! RSA'da imzalama özel anahtarla, doğrulama ise açık anahtarla yapılır:
//!
//! ```text
//!  İmzalama  (özel anahtar):  s = m^d mod n
//!  Doğrulama (açık anahtar):  m = s^e mod n
//!
//!  n: modulus (kamu bilgisi)
//!  e: açık üs (genellikle 65537 = 0x10001)
//!  d: özel üs (gizli tutulur)
//! ```
//!
//! ## PKCS#1 v1.5 Dolgu Yapısı
//!
//! ```text
//!  0x00 | 0x01 | 0xFF...FF | 0x00 | DigestInfo | Hash
//!  ──── ────── ─────────── ──── ──────────────── ────
//!   1B    1B    PS (dolgu)  1B   AlgID (DER OID)  32B
//!
//!  DigestInfo (SHA-256): 30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20
//! ```
//!
//! ## PSS (Probabilistic Signature Scheme) Yapısı
//!
//! PSS, PKCS#1 v1.5'e göre daha güvenlidir (rastgelelik içerir):
//!
//! ```text
//!  EM = maskedDB || H || 0xbc
//!
//!  maskedDB = DB XOR MGF1(H, dbLen)
//!  DB       = 0x00...00 || 0x01 || salt
//!  H        = Hash(M' = 0x00×8 || mHash || salt)
//!
//!  Doğrulama: H' hesapla, H ile karşılaştır
//! ```
//!
//! ## ECDSA İmza Doğrulama
//!
//! Eliptik eğri Dijital İmza Algoritması:
//!
//! ```text
//!  İmza: (r, s) çifti — DER formatında SEQUENCE { r INTEGER, s INTEGER }
//!
//!  Doğrulama adımları:
//!  1. 0 < r < n ve 0 < s < n kontrolü
//!  2. e = H(mesaj)
//!  3. w = s^-1 mod n
//!  4. u1 = e*w mod n
//!  5. u2 = r*w mod n
//!  6. (x1, y1) = u1*G + u2*Q  (nokta çarpımı + toplama)
//!  7. v = x1 mod n
//!  8. İmza geçerli ⟺ v == r
//!
//!  G: üretici nokta (eğriye özgü sabit)
//!  Q: açık anahtar noktası
//! ```

use alloc::vec;
use alloc::vec::Vec;
use sha2::{Digest, Sha256, Sha384};

// ============================================================================
// RSA İMZA DOĞRULAMA
// ============================================================================

/// RSA Açık Anahtarı — modulus (n) ve açık üs (e).
///
/// TLS 1.2 ve X.509 sertifika doğrulamasında kullanılır.
/// Anahtar boyutu tipik olarak 2048 veya 4096 bittir.
#[derive(Clone, Debug)]
pub struct RsaPublicKey {
    /// Modulus (n) — büyük asal çarpan çarpımı (big-endian bayt dizisi)
    pub n: Vec<u8>,
    /// Açık üs (e) — genellikle 65537 (0x10001) (big-endian bayt dizisi)
    pub e: Vec<u8>,
}

impl RsaPublicKey {
    /// Yeni RSA açık anahtarı oluşturur.
    pub fn new(n: Vec<u8>, e: Vec<u8>) -> Self {
        RsaPublicKey { n, e }
    }

    /// Anahtar boyutunu bit cinsinden döner (n'nin bayt uzunluğu × 8).
    pub fn key_size(&self) -> usize {
        self.n.len() * 8
    }

    /// PKCS#1 v1.5 imza doğrulaması.
    ///
    /// TLS 1.2 ve eski X.509 sertifikalarında kullanılır.
    ///
    /// İşlem:
    /// 1. `m = signature^e mod n` hesapla (RSA "şifre çözme")
    /// 2. Beklenen PKCS#1 v1.5 dolgu yapısını oluştur
    /// 3. Hesaplanan m ile beklenen dolguyu karşılaştır
    pub fn verify_pkcs1_v15(
        &self,
        hash: &[u8],
        signature: &[u8],
        hash_algo: HashAlgorithm,
    ) -> bool {
        if signature.len() > self.n.len() {
            return false;
        }

        // İmzayı büyük tamsayıya çevir
        let sig_int = bytes_to_biguint(signature);
        let n_int = bytes_to_biguint(&self.n);
        let e_int = bytes_to_biguint(&self.e);

        // RSA doğrulama: m = s^e mod n (açık anahtar ile "şifre çözme")
        let m_int = mod_exp(&sig_int, &e_int, &n_int);
        let m = biguint_to_bytes(&m_int, self.n.len());

        // Beklenen PKCS#1 v1.5 dolgulu hash değerini oluştur
        let padded = self.build_pkcs1_v15_padding(hash, hash_algo);

        // Hesaplanan ve beklenen değerleri karşılaştır
        m == padded
    }

    /// PSS (Probabilistic Signature Scheme) imza doğrulaması.
    ///
    /// TLS 1.3 ve yeni sertifikalarda kullanılır.
    /// PKCS#1 v1.5'e göre daha güvenlidir çünkü tuz (salt) değeri içerir.
    pub fn verify_pss(&self, hash: &[u8], signature: &[u8], hash_algo: HashAlgorithm) -> bool {
        if signature.len() > self.n.len() {
            return false;
        }

        let em_len = self.n.len();
        let hash_len = hash_algo.hash_len();
        let salt_len = hash_len; // Tuz uzunluğu hash uzunluğuna eşit

        // İmzayı büyük tamsayıya çevir ve açık anahtarla işle
        let sig_int = bytes_to_biguint(signature);
        let n_int = bytes_to_biguint(&self.n);
        let e_int = bytes_to_biguint(&self.e);

        let m_int = mod_exp(&sig_int, &e_int, &n_int);
        let em = biguint_to_bytes(&m_int, em_len);

        // PSS Doğrulama Adım 1: En sağ bayt 0xbc olmalı
        if em.is_empty() || em[em.len() - 1] != 0xbc {
            return false;
        }

        // Adım 2: maskedDB ve H'yi ayır
        let masked_db_len = em_len - hash_len - 1;
        let masked_db = &em[..masked_db_len];
        let h = &em[masked_db_len..masked_db_len + hash_len];

        // Adım 3: DB'nin en solundaki bitleri sıfır olmalı
        let em_bits = self.key_size() - 1;
        let zero_bits = 8 * em_len - em_bits;
        if zero_bits > 0 {
            let mask = 0xff >> zero_bits;
            if masked_db[0] & !mask != 0 {
                return false;
            }
        }

        // Adım 4: MGF1 ile dbMask üret (H'den maskedDB boyutunda)
        let db_mask = mgf1(h, masked_db_len, hash_algo);

        // Adım 5: DB = maskedDB XOR dbMask
        let mut db = vec![0u8; masked_db_len];
        for i in 0..masked_db_len {
            db[i] = masked_db[i] ^ db_mask[i];
        }

        // Adım 6: DB'nin en solundaki bitleri sıfırla
        if zero_bits > 0 {
            let mask = 0xff >> zero_bits;
            db[0] &= mask;
        }

        // Adım 7: DB = PS (sıfır dolgu) || 0x01 || salt — bu yapıyı doğrula
        let mut salt_start = 0;
        for i in 0..masked_db_len {
            if db[i] == 0x01 {
                salt_start = i + 1;
                break;
            }
            if db[i] != 0 {
                return false; // PS yalnızca sıfır içermeli
            }
        }

        if salt_start == 0 || salt_start + salt_len > db.len() {
            return false;
        }

        let salt = &db[salt_start..salt_start + salt_len];

        // Adım 8: H' = Hash(M') hesapla; M' = 0x00×8 || mHash || salt
        let mut m_prime = vec![0u8; 8]; // 8 sıfır bayt (sabit önek)
        m_prime.extend_from_slice(hash);
        m_prime.extend_from_slice(salt);

        let h_prime = hash_algo.hash(&m_prime);

        // Adım 9: H == H' ise imza geçerli
        h == h_prime
    }

    /// PKCS#1 v1.5 dolgu yapısını oluşturur.
    ///
    /// Yapı: 0x00 | 0x01 | 0xFF...FF (PS) | 0x00 | DigestInfo | Hash
    fn build_pkcs1_v15_padding(&self, hash: &[u8], hash_algo: HashAlgorithm) -> Vec<u8> {
        let mut padded = Vec::new();

        // Başlık: 0x00 | 0x01
        padded.push(0x00);
        padded.push(0x01);

        // PS: 0xFF dolgu baytları
        let hash_len = hash_algo.hash_len();
        let digest_info_len = hash_algo.digest_info().len();
        let ps_len = self.n.len() - 3 - digest_info_len - hash_len;

        for _ in 0..ps_len {
            padded.push(0xff);
        }

        // 0x00 ayırıcı
        padded.push(0x00);

        // DigestInfo (DER kodlu AlgorithmIdentifier) + Hash
        padded.extend_from_slice(hash_algo.digest_info());
        padded.extend_from_slice(hash);

        padded
    }
}

// ============================================================================
// ECDSA İMZA DOĞRULAMA
// ============================================================================

/// ECDSA Açık Anahtarı — P-256 veya P-384 eğrisi üzerindeki nokta.
///
/// Açık anahtar, eğri üzerindeki bir nokta (x, y) koordinat çiftidir.
/// Güvenlik seviyesi: P-256 = 128-bit, P-384 = 192-bit
#[derive(Clone, Debug)]
pub struct EcdsaPublicKey {
    /// Eğri türü (P-256 veya P-384)
    pub curve: EllipticCurve,
    /// X koordinatı (sıkıştırılmamış format, big-endian)
    pub x: Vec<u8>,
    /// Y koordinatı (sıkıştırılmamış format, big-endian)
    pub y: Vec<u8>,
}

/// Desteklenen eliptik eğri türleri.
///
/// ```text
///  P-256 (secp256r1/prime256v1): 128-bit güvenlik, 32-byte koordinat
///  P-384 (secp384r1):            192-bit güvenlik, 48-byte koordinat
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EllipticCurve {
    P256,
    P384,
}

impl EllipticCurve {
    /// Koordinat boyutunu bayt cinsinden döner (P-256: 32B, P-384: 48B).
    pub fn coord_size(&self) -> usize {
        match self {
            EllipticCurve::P256 => 32,
            EllipticCurve::P384 => 48,
        }
    }

    /// Eğrinin alan asal sayısını (p) döner (big-endian bayt dizisi).
    ///
    /// P-256: p = 2^256 - 2^224 + 2^192 + 2^96 - 1
    /// P-384: p = 2^384 - 2^128 - 2^96 + 2^32 - 1
    pub fn prime(&self) -> &'static [u8] {
        match self {
            EllipticCurve::P256 => &[
                0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff,
            ],
            EllipticCurve::P384 => &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            ],
        }
    }

    /// Eğrinin nokta grubunun mertebesini (n) döner (big-endian bayt dizisi).
    ///
    /// n: üretici G noktasının mertebesi — tüm hesaplamalar mod n yapılır.
    pub fn order(&self) -> &'static [u8] {
        match self {
            EllipticCurve::P256 => &[
                0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2,
                0xfc, 0x63, 0x25, 0x51,
            ],
            EllipticCurve::P384 => &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc7, 0x63, 0x4d, 0x81,
                0xf4, 0x37, 0x2d, 0xdf, 0x58, 0x1a, 0x0d, 0xb2, 0x48, 0xb0, 0xa7, 0x7a, 0xec, 0xec,
                0x19, 0x6a, 0xcc, 0xc5, 0x29, 0x73,
            ],
        }
    }
}

impl EcdsaPublicKey {
    /// Yeni ECDSA açık anahtarı oluşturur.
    pub fn new(curve: EllipticCurve, x: Vec<u8>, y: Vec<u8>) -> Self {
        EcdsaPublicKey { curve, x, y }
    }

    /// Sıkıştırılmamış nokta formatından ayrıştırır (0x04 || x || y).
    ///
    /// X.509 sertifikalarında SubjectPublicKeyInfo alanından okunur.
    pub fn from_uncompressed(curve: EllipticCurve, data: &[u8]) -> Option<Self> {
        // Sıkıştırılmamış format etiketi 0x04 olmalı
        if data.is_empty() || data[0] != 0x04 {
            return None;
        }

        let coord_size = curve.coord_size();
        if data.len() != 1 + 2 * coord_size {
            return None;
        }

        Some(EcdsaPublicKey {
            curve,
            x: data[1..1 + coord_size].to_vec(),
            y: data[1 + coord_size..].to_vec(),
        })
    }

    /// ECDSA imzasını doğrular.
    ///
    /// İmza DER formatındadır: SEQUENCE { r INTEGER, s INTEGER }
    ///
    /// TLS el sıkışmasında sunucunun sertifika imzasını doğrulamak için kullanılır.
    pub fn verify(&self, hash: &[u8], signature: &[u8]) -> bool {
        // DER formatındaki imzayı r ve s bileşenlerine ayrıştır
        let (r, s) = match parse_der_signature(signature) {
            Some((r, s)) => (r, s),
            None => return false,
        };

        // Eğri mertebesi n
        let n = self.curve.order();
        let n_int = bytes_to_biguint(n);

        // r ve s'yi büyük tamsayıya çevir
        let r_int = bytes_to_biguint(&r);
        let s_int = bytes_to_biguint(&s);

        // Kısıt kontrolü: 0 < r < n ve 0 < s < n
        if r_int.is_zero()
            || s_int.is_zero()
            || biguint_cmp(&r_int, &n_int) >= 0
            || biguint_cmp(&s_int, &n_int) >= 0
        {
            return false;
        }

        // e = H(mesaj) — hash değerini tamsayıya çevir
        let e_int = bytes_to_biguint(hash);

        // w = s^-1 mod n — modüler ters
        let s_inv = mod_inverse(&s_int, &n_int);
        if s_inv.is_zero() {
            return false;
        }

        // u1 = e * w mod n
        let u1 = mod_mul(&e_int, &s_inv, &n_int);
        let u1_bytes = biguint_to_bytes(&u1, 0);

        // u2 = r * w mod n
        let u2 = mod_mul(&r_int, &s_inv, &n_int);
        let u2_bytes = biguint_to_bytes(&u2, 0);

        // (x1, y1) = u1*G + u2*Q  — çift skaler nokta çarpımı
        let (x1, _y1) = self.ec_double_mul(&u1_bytes, &u2_bytes);

        // v = x1 mod n
        let x1_int = bytes_to_biguint(&x1);
        let v = mod_reduce(&x1_int, &n_int);

        // İmza geçerli ⟺ v == r
        biguint_cmp(&v, &r_int) == 0
    }

    /// Çift skaler çarpım: u1*G + u2*Q (Shamir'in hilesi ile verimli)
    ///
    /// Tam uygulama için gereken adımlar:
    /// 1. Eğri üzerinde nokta toplama (affine koordinatlarda)
    /// 2. Skaler çarpım (double-and-add algoritması)
    /// 3. Zamanlama saldırısına karşı sabit zamanlı işlemler
    fn ec_double_mul(&self, u1: &[u8], u2: &[u8]) -> (Vec<u8>, Vec<u8>) {
        // Not: Bu yer tutucu bir uygulamadır.
        // Gerçek uygulama şunları hesaplamalıdır:
        // P1 = u1 * G  (G: üretici nokta, eğriye özgü sabittir)
        // P2 = u2 * Q  (Q: açık anahtar noktası = this.x, this.y)
        // Sonuç = P1 + P2  (eğri üzerinde nokta toplama işlemi)

        let coord_size = self.curve.coord_size();
        (vec![0u8; coord_size], vec![0u8; coord_size])
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// Hash algoritması — SHA-256, SHA-384 veya SHA-512.
///
/// RSA-PSS ve ECDSA doğrulamasında hangi hash fonksiyonunun kullanıldığını belirtir.
/// X.509 sertifikasının AlgorithmIdentifier alanından belirlenir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlgorithm {
    /// Hash sonucunun bayt uzunluğunu döner.
    pub fn hash_len(&self) -> usize {
        match self {
            HashAlgorithm::Sha256 => 32,
            HashAlgorithm::Sha384 => 48,
            HashAlgorithm::Sha512 => 64,
        }
    }

    /// Verilen veriyi hashler ve sonucu Vec<u8> olarak döner.
    pub fn hash(&self, data: &[u8]) -> Vec<u8> {
        match self {
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha384 => {
                let mut hasher = Sha384::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
            HashAlgorithm::Sha512 => {
                let mut hasher = sha2::Sha512::new();
                hasher.update(data);
                hasher.finalize().to_vec()
            }
        }
    }

    /// DER kodlu DigestInfo (AlgorithmIdentifier) baytlarını döner.
    ///
    /// PKCS#1 v1.5 dolgusu için hash OID'ini DER formatında tanımlar.
    ///
    /// ```text
    ///  SHA-256 OID: 2.16.840.1.101.3.4.2.1
    ///              30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20
    ///  SHA-384 OID: 2.16.840.1.101.3.4.2.2
    ///              30 41 30 0d 06 09 60 86 48 01 65 03 04 02 02 05 00 04 30
    ///  SHA-512 OID: 2.16.840.1.101.3.4.2.3
    ///              30 51 30 0d 06 09 60 86 48 01 65 03 04 02 03 05 00 04 40
    /// ```
    pub fn digest_info(&self) -> &'static [u8] {
        match self {
            // SHA-256 OID: 2.16.840.1.101.3.4.2.1
            HashAlgorithm::Sha256 => &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ],
            // SHA-384 OID
            HashAlgorithm::Sha384 => &[
                0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x02, 0x05, 0x00, 0x04, 0x30,
            ],
            // SHA-512 OID
            HashAlgorithm::Sha512 => &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ],
        }
    }
}

/// MGF1 (Mask Generation Function 1) — PSS için maske üretici.
///
/// ```text
///  MGF1(seed, maskLen, Hash):
///    maske = ""
///    sayaç = 0
///    while len(maske) < maskLen:
///        maske ||= Hash(seed || sayaç)  [sayaç 4-byte big-endian]
///        sayaç++
///    return maske[:maskLen]
/// ```
fn mgf1(seed: &[u8], mask_len: usize, hash_algo: HashAlgorithm) -> Vec<u8> {
    let mut mask = Vec::new();
    let mut counter = 0u32;

    while mask.len() < mask_len {
        // Hash(seed || counter) — sayaç big-endian 4 bayt
        let mut data = seed.to_vec();
        data.extend_from_slice(&counter.to_be_bytes());

        let h = hash_algo.hash(&data);
        mask.extend_from_slice(&h);

        counter += 1;
    }

    mask.truncate(mask_len);
    mask
}

/// DER kodlu ECDSA imzasını ayrıştırır.
///
/// DER formatı: SEQUENCE { r INTEGER, s INTEGER }
///
/// ```text
///  30 <seq_len>           — SEQUENCE etiketi ve uzunluk
///    02 <r_len> <r bytes> — INTEGER r
///    02 <s_len> <s bytes> — INTEGER s
/// ```
fn parse_der_signature(sig: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // SEQUENCE { r INTEGER, s INTEGER }
    if sig.len() < 8 {
        return None;
    }

    // SEQUENCE etiketi 0x30 olmalı
    if sig[0] != 0x30 {
        return None;
    }

    let seq_len = sig[1] as usize;
    if sig.len() < 2 + seq_len {
        return None;
    }

    let mut pos = 2;

    // r değerini ayrıştır (INTEGER etiketi 0x02)
    if sig[pos] != 0x02 {
        return None;
    }
    pos += 1;

    let r_len = sig[pos] as usize;
    pos += 1;

    if pos + r_len > sig.len() {
        return None;
    }

    let r = sig[pos..pos + r_len].to_vec();
    pos += r_len;

    // s değerini ayrıştır (INTEGER etiketi 0x02)
    if pos >= sig.len() || sig[pos] != 0x02 {
        return None;
    }
    pos += 1;

    let s_len = sig[pos] as usize;
    pos += 1;

    if pos + s_len > sig.len() {
        return None;
    }

    let s = sig[pos..pos + s_len].to_vec();

    Some((r, s))
}

// ============================================================================
// BÜYÜK TAM SAYI İŞLEMLERİ (basitleştirilmiş)
// ============================================================================

/// Basit büyük işaretsiz tam sayı yapısı (küçük-endian 64-bit limb dizisi).
///
/// RSA modular exponentiation için kullanılır.
/// Her limb 64-bit bir parçayı temsil eder.
///
/// ```text
///  Örnek: 0x0102030405060708090a0b0c sayısı için
///  limbs = [0x090a0b0c, 0x05060708, 0x01020304]  (küçük-endian sıra)
/// ```
#[derive(Clone, Debug)]
struct BigUint {
    limbs: Vec<u64>, // Küçük-endian: limbs[0] en az önemli 64-bit parça
}

impl BigUint {
    /// Tüm limb'ler sıfır mı? (sıfır mı?) kontrolü.
    fn is_zero(&self) -> bool {
        self.limbs.iter().all(|&x| x == 0)
    }

    /// Big-endian bayt dizisinden büyük tamsayı oluşturur.
    fn from_bytes_be(bytes: &[u8]) -> Self {
        let mut limbs = Vec::new();

        // Big-endian baytları küçük-endian u64 limb'lere çevir
        let mut i = bytes.len();
        while i > 0 {
            let start = if i >= 8 { i - 8 } else { 0 };
            let end = i;

            let mut arr = [0u8; 8];
            let copy_len = end - start;
            arr[8 - copy_len..].copy_from_slice(&bytes[start..end]);

            limbs.push(u64::from_be_bytes(arr));
            i = start;
        }

        if limbs.is_empty() {
            limbs.push(0);
        }

        BigUint { limbs }
    }

    /// Büyük tamsayıyı big-endian bayt dizisine çevirir.
    /// `min_len` ile minimum uzunluk (sıfır dolgu ile) belirtilir.
    fn to_bytes_be(&self, min_len: usize) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Küçük-endian limb'leri big-endian baytlara çevir
        for &limb in self.limbs.iter().rev() {
            bytes.extend_from_slice(&limb.to_be_bytes());
        }

        // Baştaki sıfırları kaldır
        while bytes.len() > min_len && bytes.first() == Some(&0) {
            bytes.remove(0);
        }

        // Minimum uzunluğa sıfır dolgu yap
        while bytes.len() < min_len {
            bytes.insert(0, 0);
        }

        bytes
    }
}

/// Big-endian bayt dizisini BigUint'e çevirir.
fn bytes_to_biguint(bytes: &[u8]) -> BigUint {
    BigUint::from_bytes_be(bytes)
}

/// BigUint'i big-endian bayt dizisine çevirir.
fn biguint_to_bytes(n: &BigUint, min_len: usize) -> Vec<u8> {
    n.to_bytes_be(min_len)
}

/// İki BigUint'i karşılaştırır: a < b → -1, a == b → 0, a > b → 1
fn biguint_cmp(a: &BigUint, b: &BigUint) -> i8 {
    // En önemli limb'den karşılaştır
    let max_len = a.limbs.len().max(b.limbs.len());

    for i in (0..max_len).rev() {
        let a_val = a.limbs.get(i).copied().unwrap_or(0);
        let b_val = b.limbs.get(i).copied().unwrap_or(0);

        if a_val < b_val {
            return -1;
        }
        if a_val > b_val {
            return 1;
        }
    }

    0
}

/// Modüler üs alma: base^exp mod modulus
///
/// Kare-ve-çarp (square-and-multiply) algoritması:
/// ```text
///  result = 1
///  for her bit in exp (düşükten yükseğe):
///      if bit == 1: result = result * base mod modulus
///      base = base^2 mod modulus
/// ```
fn mod_exp(base: &BigUint, exp: &BigUint, modulus: &BigUint) -> BigUint {
    // Kare-ve-çarp algoritması
    let mut result = BigUint { limbs: vec![1] };
    let mut base = base.clone();

    // exp'nin her limb'indeki her bit için
    for limb in &exp.limbs {
        for bit in 0..64 {
            if (limb >> bit) & 1 != 0 {
                result = mod_mul(&result, &base, modulus);
            }
            base = mod_mul(&base, &base, modulus);
        }
    }

    result
}

/// Modüler çarpma: a * b mod m
///
/// Not: Bu basitleştirilmiş bir yer tutucudur.
/// Gerçek uygulama için tam büyük tam sayı çarpımı + Montgomery indirgeme gerekir.
fn mod_mul(a: &BigUint, b: &BigUint, m: &BigUint) -> BigUint {
    // Basitleştirilmiş yer tutucu — tam uygulama büyük tamsayı çarpımı gerektirir
    a.clone()
}

/// Modüler indirgeme: a mod m
///
/// Not: Bu basitleştirilmiş bir yer tutucudur.
fn mod_reduce(a: &BigUint, m: &BigUint) -> BigUint {
    // Basitleştirilmiş yer tutucu
    a.clone()
}

/// Genişletilmiş Öklid algoritmasıyla modüler ters: a^-1 mod m
///
/// ```text
///  Genişletilmiş Öklid:
///  gcd(a, m) = 1 ise a^-1 mod m mevcuttur.
///  Bezout kimliği: a*x + m*y = 1 → x = a^-1 mod m
/// ```
fn mod_inverse(a: &BigUint, m: &BigUint) -> BigUint {
    // Basitleştirilmiş yer tutucu — tam uygulama EEA gerektirir
    BigUint { limbs: vec![1] }
}

// ============================================================================
// TEST
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_algorithms() {
        let data = b"hello world";

        let sha256 = HashAlgorithm::Sha256.hash(data);
        assert_eq!(sha256.len(), 32);

        let sha384 = HashAlgorithm::Sha384.hash(data);
        assert_eq!(sha384.len(), 48);
    }

    #[test]
    fn test_mgf1() {
        let seed = b"seed";
        let mask = mgf1(seed, 32, HashAlgorithm::Sha256);
        assert_eq!(mask.len(), 32);
    }

    #[test]
    fn test_der_parsing() {
        // Minimal geçerli DER imzası
        let sig = [
            0x30, 0x08, // SEQUENCE, uzunluk 8
            0x02, 0x02, 0x01, 0x02, // INTEGER r = 0x0102
            0x02, 0x02, 0x03, 0x04, // INTEGER s = 0x0304
        ];

        let (r, s) = parse_der_signature(&sig).unwrap();
        assert_eq!(r, vec![0x01, 0x02]);
        assert_eq!(s, vec![0x03, 0x04]);
    }
}
