//! # DNSSEC (DNS Güvenlik Uzantıları)
//!
//! DNSSEC, DNS yanıtlarının gerçekliğini ve bütünlüğünü kriptografik
//! imzalar aracılığıyla doğrulayan bir güvenlik uzantısıdır.
//! RFC 4033, 4034, 4035 ile tanımlanmıştır.
//!
//! ## DNSSEC'in Amacı
//!
//! DNS protokolü tasarlandığında güvenlik gözetilmemişti. Sonradan ortaya
//! çıkan saldırı türleri:
//! - DNS Sahteciliği (Spoofing): Sahte DNS yanıtları enjekte etme
//! - DNS Önbellek Zehirleme (Cache Poisoning): Önbelleğe yanlış kayıt ekleme
//! - MITM Saldırıları: DNS trafiğini değiştirme
//!
//! DNSSEC bu saldırılara karşı kriptografik imzalar kullanır.
//!
//! ## DNSSEC Güven Zinciri (Chain of Trust)
//!
//! ```text
//!  Root (.)               <-- Kök Güven Çapası (ICANN tarafından imzalanır)
//!       |
//!       | DS (Delegation Signer)
//!       v
//!  .com (TLD)             <-- Üst Seviye Alan imzası doğrulandı
//!       |
//!       | DS
//!       v
//!  example.com            <-- Alan adı imzası doğrulandı
//!       |
//!       | RRSIG (Resource Record Signature)
//!       v
//!  www.example.com A IP   <-- Bu DNS kaydı güvenilir kabul edilir
//! ```
//!
//! ## DNSSEC Kayıt Türleri
//!
//! ```text
//! DNSKEY (48) : Alan adının açık anahtarı (imzaları doğrulamak için)
//!               ├── KSK (Key Signing Key, flag=257): Diğer anahtarları imzalar
//!               └── ZSK (Zone Signing Key, flag=256): Zone kayıtlarını imzalar
//!
//! RRSIG  (46) : Resource Record kümesinin dijital imzası
//!               ├── Hangi kayıt türünü kapsadığı (type_covered)
//!               ├── İmza algoritması (RSA, ECDSA, Ed25519...)
//!               └── İmzalayan DNSKEY'in key tag'i
//!
//! DS     (43) : Delegation Signer - Alt zone DNSKEY'inin özeti
//!               Üst domain, alt domain'in KSK'sının hash'ini DS olarak saklar
//!               Bu sayede zincir oluşturulur.
//!
//! NSEC   (47) : Next Secure - Sıradaki mevcut kayıt adını gösterir
//!               "Bu ada kadar kayıt yok" kanıtı (negatif yanıt)
//!
//! NSEC3  (50) : NSEC'in hash'lenmiş versiyonu (zone enumeration'ı önler)
//! ```
//!
//! ## İmza Doğrulama Süreci
//!
//! ```text
//! 1. DNSKEY kaydını al (zone'un açık anahtarı)
//! 2. DS kaydı ile DNSKEY'in hash'ini doğrula (üst domain'den alınan DS ile eşleş)
//! 3. RRSIG ve DNSKEY ile DNS kayıt kümesini doğrula
//! 4. Sonuç: Secure (doğrulandı) | Bogus (hatalı) | Insecure (DNSSEC yok)
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ed448_goldilocks_plus::{Signature as Ed448Signature, VerifyingKey as Ed448VerifyingKey};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use p256::ecdsa::signature::Verifier;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use p256::ecdsa::{Signature as P256Signature, VerifyingKey as P256VerifyingKey};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use p384::ecdsa::{Signature as P384Signature, VerifyingKey as P384VerifyingKey};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384};
use spin::Mutex;

// ============================================================================
// DNSSEC Kayıt Türü Kodları (RFC 4034)
// ============================================================================
const DNSKEY: u16 = 48; // DNS Public Key kaydı
const RRSIG: u16 = 46; // Resource Record Signature kaydı
const DS: u16 = 43; // Delegation Signer kaydı
const NSEC: u16 = 47; // Next Secure kaydı (negatif yanıt kanıtı)
const NSEC3: u16 = 50; // Next Secure v3 (hash'lenmiş, zone enumeration önler)
const NSEC3PARAM: u16 = 51; // NSEC3 parametreleri

// ============================================================================
// DNSSEC İmza Algoritması Kodları (RFC 8624)
// ============================================================================
// Güvenlik önerisi (2024):
//   - Ed25519 (15) ve ECDSA P-256 (13): Önerilir, modern ve hızlı
//   - RSA/SHA-256 (8): Hâlâ yaygın, kabul edilebilir
//   - RSA/SHA-1 (5, 7): Kullanımdan kaldırılmıştır (SHA-1 zayıf)
const RSA_SHA1: u8 = 5; // RSA + SHA-1 (kullanımdan kaldırılmış)
const RSA_SHA1_NSEC3: u8 = 7; // RSA + SHA-1 + NSEC3 (kullanımdan kaldırılmış)
const RSA_SHA256: u8 = 8; // RSA + SHA-256 (yaygın kullanım)
const RSA_SHA512: u8 = 10; // RSA + SHA-512
const ECDSA_P256_SHA256: u8 = 13; // ECDSA P-256 + SHA-256 (önerilir)
const ECDSA_P384_SHA384: u8 = 14; // ECDSA P-384 + SHA-384
const ED25519: u8 = 15; // Ed25519 Edwards eğrisi (önerilir)
const ED448: u8 = 16; // Ed448 (yüksek güvenlik)

// ============================================================================
// DS Özet Türleri (Digest Types)
// ============================================================================
const DIGEST_SHA1: u8 = 1; // SHA-1 (önerilmez, geriye dönük uyumluluk)
const DIGEST_SHA256: u8 = 2; // SHA-256 (önerilir)
const DIGEST_SHA384: u8 = 4; // SHA-384 (yüksek güvenlik)

type DnssecAlgorithmVerifier = fn(public_key: &[u8], signature: &[u8], signed_data: &[u8]) -> bool;
type DnssecDigestCalculator =
    fn(key: &DnsKey, domain: &str, canonical_data: &[u8]) -> Option<Vec<u8>>;

/// DNSKEY kaydı.
///
/// Bir DNS zone'unun açık anahtarını içerir. İki türü vardır:
/// - KSK (Key Signing Key, flags=257): Sadece diğer DNSKEY'leri imzalar
/// - ZSK (Zone Signing Key, flags=256): Zone'daki tüm diğer kayıtları imzalar
///
/// ```text
/// DNSKEY Wire Format:
///  +--------+--------+--------+--------+
///  | Flags  (2 byte) | Proto  | Algo   |
///  +--------+--------+--------+--------+
///  | Public Key Data (değişken uzunluk)|
///  +-----------------------------------+
/// ```
#[derive(Clone, Debug)]
pub struct DnsKey {
    pub flags: u16,          // 256=ZSK, 257=KSK (SEP bayrağı)
    pub protocol: u8,        // Her zaman 3 (RFC 4034)
    pub algorithm: u8,       // İmza algoritması kodu (RSA_SHA256=8, Ed25519=15 vb.)
    pub public_key: Vec<u8>, // DER formatında açık anahtar verisi
    pub key_tag: u16,        // Anahtarın kısa tanımlayıcısı (RRSIG ile eşleştirmek için)
}

impl DnsKey {
    /// DNSKEY RDATA'sından kayıt ayrıştırır.
    ///
    /// RDATA formatı: Flags(2) + Protocol(1) + Algorithm(1) + PublicKey(var.)
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        if rdata.len() < 4 {
            return None;
        }

        let flags = u16::from_be_bytes([rdata[0], rdata[1]]);
        let protocol = rdata[2];
        let algorithm = rdata[3];
        let public_key = rdata[4..].to_vec();

        // Key tag: anahtarın hızlı tanımlayıcısı (RFC 4034 Ek B algoritması)
        let key_tag = Self::calculate_key_tag(flags, protocol, algorithm, &public_key);

        Some(DnsKey {
            flags,
            protocol,
            algorithm,
            public_key,
            key_tag,
        })
    }

    /// Key tag hesaplar (RFC 4034 Ek B).
    ///
    /// Key tag, RRSIG'da hangi DNSKEY'in kullanıldığını belirtmek için
    /// kullanılan 16-bit kısa bir özettir. Güvenlik amacı taşımaz.
    fn calculate_key_tag(flags: u16, protocol: u8, algorithm: u8, key: &[u8]) -> u16 {
        let mut sum: u32 = 0;

        // Flags alanını işle (yüksek byte + düşük byte)
        sum += (flags >> 8) as u32;
        sum += (flags & 0xFF) as u32;

        // Protocol ve algoritma alanlarını ekle
        sum += protocol as u32;
        sum += algorithm as u32;

        // Anahtar verisi: çift indeksteki byte'lar yüksek byte olarak işlenir
        for (i, byte) in key.iter().enumerate() {
            if i % 2 == 0 {
                sum += (*byte as u32) << 8;
            } else {
                sum += *byte as u32;
            }
        }

        // Tek uzunluklarda son byte zaten dahil (eksik byte sıfır olarak sayılır)
        if key.len() % 2 != 0 {
            sum += 0;
        }

        // 32-bit toplamı 16-bit'e katlama (carry wraparound)
        ((sum >> 16) + (sum & 0xFFFF)) as u16
    }

    /// Bu anahtarın Zone Key olup olmadığını kontrol eder.
    ///
    /// Flags bit 7 (0x0100) set ise Zone Key'dir.
    /// Zone Key olmayanlar (örn. SIG(0)) zone imzalaması için kullanılamaz.
    pub fn is_zone_key(&self) -> bool {
        (self.flags & 0x0100) != 0
    }

    /// Bu anahtarın KSK (Key Signing Key) olup olmadığını kontrol eder.
    ///
    /// SEP (Secure Entry Point) biti: flags bit 0 (0x0001).
    /// KSK: Sadece DNSKEY kümesini imzalar, zone kayıtlarını imzalamaz.
    pub fn is_ksk(&self) -> bool {
        (self.flags & 0x0001) != 0
    }

    /// Bu anahtarın ZSK (Zone Signing Key) olup olmadığını kontrol eder.
    ///
    /// ZSK: Zone Key ve SEP bayrağı yok.
    /// Zone'daki tüm RRset'leri imzalar (A, MX, TXT vb.).
    pub fn is_zsk(&self) -> bool {
        self.is_zone_key() && !self.is_ksk()
    }
}

/// RRSIG kaydı (Resource Record Signature).
///
/// Bir RRset (aynı türdeki kayıtlar kümesi) için dijital imza içerir.
///
/// ```text
/// RRSIG Wire Format:
///  Type Covered (2) | Algorithm (1) | Labels (1) | Original TTL (4)
///  Sig Expiration (4) | Sig Inception (4) | Key Tag (2)
///  Signer Name (değişken) | Signature (değişken)
/// ```
#[derive(Clone, Debug)]
pub struct RrSig {
    pub type_covered: u16,         // Bu imzanın kapsadığı kayıt türü (örn. A=1)
    pub algorithm: u8,             // İmza algoritması kodu
    pub labels: u8,                // İmzalanan alan adındaki etiket sayısı (wildcard için)
    pub original_ttl: u32,         // İmzalanmadan önceki orijinal TTL değeri
    pub signature_expiration: u32, // Unix zaman damgası: imzanın son geçerlilik zamanı
    pub signature_inception: u32,  // Unix zaman damgası: imzanın geçerlilik başlangıcı
    pub key_tag: u16,              // İmzalayan DNSKEY'in key tag'i (eşleştirme için)
    pub signer_name: String,       // İmzalayan zone'un adı (örn. "example.com")
    pub signature: Vec<u8>,        // Asıl dijital imza verisi (RSA/ECDSA/Ed25519)
}

impl RrSig {
    /// RRSIG RDATA'sından kayıt ayrıştırır.
    ///
    /// İlk 18 byte sabit alanlar, sonrası imzalayan adı ve imza verisidir.
    pub fn parse(rdata: &[u8], mut offset: usize) -> Option<Self> {
        if rdata.len() < 18 {
            return None;
        }

        let type_covered = u16::from_be_bytes([rdata[0], rdata[1]]);
        let algorithm = rdata[2];
        let labels = rdata[3];
        let original_ttl = u32::from_be_bytes([rdata[4], rdata[5], rdata[6], rdata[7]]);
        let signature_expiration = u32::from_be_bytes([rdata[8], rdata[9], rdata[10], rdata[11]]);
        let signature_inception = u32::from_be_bytes([rdata[12], rdata[13], rdata[14], rdata[15]]);
        let key_tag = u16::from_be_bytes([rdata[16], rdata[17]]);

        offset = 18; // Sabit alanlar bitti, alan adı başlıyor

        // İmzalayan zone adını DNS label formatından ayrıştır
        let signer_name = Self::parse_name(rdata, &mut offset)?;
        let signature = rdata[offset..].to_vec();

        Some(RrSig {
            type_covered,
            algorithm,
            labels,
            original_ttl,
            signature_expiration,
            signature_inception,
            key_tag,
            signer_name,
            signature,
        })
    }

    /// DNS label formatındaki alan adını metin olarak ayrıştırır.
    ///
    /// İşaretçi sıkıştırmasını (0xC0 prefix) destekler ve
    /// sonsuz döngüyü önlemek için maksimum atlama sayısı sınırı uygular.
    fn parse_name(data: &[u8], offset: &mut usize) -> Option<String> {
        let mut name = String::new();
        let mut jumped = false; // İşaretçi atlaması yapıldı mı?
        let mut max_jumps = 5; // Sonsuz döngü koruması: maksimum 5 atlama
        let original_offset = *offset;

        loop {
            if *offset >= data.len() {
                return None;
            }

            let len = data[*offset] as usize;

            if len == 0 {
                *offset += 1; // Root label (alan adı sonu)
                break;
            }

            // DNS sıkıştırma işaretçisi kontrolü (ilk 2 bit = 1 1 ise işaretçi)
            if (len & 0xC0) == 0xC0 {
                if *offset + 1 >= data.len() {
                    return None;
                }
                let ptr = (((data[*offset] & 0x3F) as usize) << 8) | (data[*offset + 1] as usize);
                if !jumped {
                    *offset += 2; // Normal akıştaki konumu 2 byte ileri al
                    jumped = true;
                }
                *offset = ptr; // İşaretçinin gösterdiği konuma atla
                max_jumps -= 1;
                if max_jumps == 0 {
                    return None; // Muhtemel döngü tespit edildi
                }
                continue;
            }

            *offset += 1;
            if *offset + len > data.len() {
                return None;
            }

            if !name.is_empty() {
                name.push('.');
            }

            for i in 0..len {
                name.push(data[*offset + i] as char);
            }
            *offset += len;

            if !jumped {
                // Continue parsing
            }
        }

        if name.is_empty() {
            name.push('.'); // Root zone: tek nokta
        }

        Some(name)
    }

    /// İmzayı RRset üzerinde doğrular.
    ///
    /// Bu implementasyon key-tag, algoritma ve zaman geçerliliğini kontrol eder.
    /// Ed25519, ECDSA P-256 ve RSA/SHA-2 aileleri crate::crypto üstünden doğrulanır.
    /// Destek dışı algoritmalar fail-closed olarak reddedilir.
    ///
    /// Parametre:
    ///   - rrset: Doğrulanacak DNS kayıt kümesi (kanonik formatta)
    ///   - key: İmzayı doğrulamak için kullanılacak DNSKEY kaydı
    pub fn verify_signature(&self, rrset: &[u8], key: &DnsKey) -> bool {
        // ── 1. Key-tag eşleşmesi ──────────────────────────────────────
        // RRSIG'daki key_tag, imzalayan DNSKEY'in hesaplanmış key_tag değeriyle
        // eşleşmelidir. Eşleşmezse yanlış anahtar kullanılıyor demektir.
        if self.key_tag != key.key_tag {
            crate::serial_println!(
                "[DNSSEC] RRSIG key_tag ({}) does not match DNSKEY key_tag ({})",
                self.key_tag,
                key.key_tag
            );
            return false;
        }

        // ── 2. Algoritma eşleşmesi ────────────────────────────────────
        // RRSIG'daki algoritma kodu, DNSKEY'in algoritma koduyla aynı olmalıdır.
        // Farklı algoritmalar farklı imza formatları üretir.
        if self.algorithm != key.algorithm {
            crate::serial_println!(
                "[DNSSEC] RRSIG algorithm ({}) does not match DNSKEY algorithm ({})",
                self.algorithm,
                key.algorithm
            );
            return false;
        }

        // ── 3. Kriptografik imza doğrulaması (RFC 8624) ────────────────
        // İmza algoritmasına göre uygun doğrulama fonksiyonunu çağır.
        // DNSSEC için yaygın algoritmalar: RSA/SHA-256, ECDSA P-256/SHA-256, Ed25519
        //
        // İmza doğrulama adımları:
        //   a) RRset'i kanonik sıraya koy (RFC 4034 Bölüm 6.3)
        //   b) Signed data oluştur: RRSIG RDATA (imza hariç) + kanonik RRset
        //   c) DNSKEY public key kullanarak imzayı doğrula

        let signed_data = self.build_signed_data(rrset);
        if signed_data.is_empty() {
            crate::serial_println!("[DNSSEC] Failed to build signed data");
            return false;
        }

        match self.algorithm {
            ED25519 => {
                // Ed25519 imza doğrulaması (RFC 8624 Section 3.2)
                // DNSKEY algorithm 15: Ed25519 kullanır
                if key.public_key.len() != 32 {
                    crate::serial_println!("[DNSSEC] Invalid Ed25519 key length");
                    return false;
                }

                if self.signature.len() != 64 {
                    crate::serial_println!("[DNSSEC] Invalid Ed25519 signature length");
                    return false;
                }

                let mut sig_bytes = [0u8; 64];
                sig_bytes.copy_from_slice(&self.signature);

                // Ed25519 verify: public_key.verify(signed_data, signature)
                let ed_pubkey = crate::crypto::ed25519::Ed25519PublicKey::from_bytes(
                    key.public_key.as_slice().try_into().unwrap(),
                );

                if ed_pubkey.verify(&signed_data, &sig_bytes) {
                    crate::serial_println!(
                        "[DNSSEC] ✓ Ed25519 signature verified (key_tag={})",
                        self.key_tag
                    );
                    true
                } else {
                    crate::serial_println!(
                        "[DNSSEC] ✗ Ed25519 signature verification failed (key_tag={})",
                        self.key_tag
                    );
                    false
                }
            }
            ECDSA_P256_SHA256 => verify_ecdsa_signature(
                &key.public_key,
                &self.signature,
                32,
                self.key_tag,
                "P-256",
                |pubkey, sig| verify_p256_signature(pubkey, sig, &signed_data),
            ),
            ECDSA_P384_SHA384 => verify_ecdsa_signature(
                &key.public_key,
                &self.signature,
                48,
                self.key_tag,
                "P-384",
                |pubkey, sig| verify_p384_signature(pubkey, sig, &signed_data),
            ),
            ED448 => {
                if key.public_key.len() != 57 {
                    crate::serial_println!(
                        "[DNSSEC] Invalid Ed448 key length: {}",
                        key.public_key.len()
                    );
                    return false;
                }

                if self.signature.len() != 114 {
                    crate::serial_println!(
                        "[DNSSEC] Invalid Ed448 signature length: {}",
                        self.signature.len()
                    );
                    return false;
                }

                if verify_ed448_signature(&key.public_key, &self.signature, &signed_data) {
                    crate::serial_println!(
                        "[DNSSEC] ✓ Ed448 signature verified (key_tag={})",
                        self.key_tag
                    );
                    true
                } else {
                    crate::serial_println!(
                        "[DNSSEC] ✗ Ed448 signature verification failed (key_tag={})",
                        self.key_tag
                    );
                    false
                }
            }
            RSA_SHA1 | RSA_SHA1_NSEC3 | RSA_SHA256 | RSA_SHA512 => {
                // RSA imza doğrulaması (RFC 8624 Section 1.2)
                // DNSKEY algorithm 8 (RSA/SHA-256) veya 10 (RSA/SHA-512)
                if key.public_key.is_empty() {
                    crate::serial_println!("[DNSSEC] Invalid RSA key");
                    return false;
                }

                // Parse RSA public key from DNSKEY format
                // Format: exponent length (1 or 3 bytes) + exponent + modulus
                let mut offset = 0;
                let exp_len = if key.public_key.len() > 0 {
                    key.public_key[0] as usize
                } else {
                    0
                };

                if exp_len == 0 {
                    // 3-byte exponent length (rare, for very large exponents)
                    if key.public_key.len() < 4 {
                        crate::serial_println!("[DNSSEC] RSA key too short for 3-byte exponent");
                        return false;
                    }
                    offset = 3;
                } else {
                    // 1-byte exponent length (common)
                    offset = 1;
                }

                let exponent_bytes = &key.public_key[offset..offset + exp_len];
                let modulus_bytes = &key.public_key[offset + exp_len..];

                let rsa_pubkey =
                    crate::crypto::rsa::RsaPublicKey::new(modulus_bytes, exponent_bytes);

                // Determine hash type based on algorithm
                let hash_type = match self.algorithm {
                    RSA_SHA1 | RSA_SHA1_NSEC3 => "sha1",
                    RSA_SHA256 => "sha256",
                    RSA_SHA512 => "sha512",
                    _ => return false,
                };

                // Verify RSA signature
                if rsa_pubkey.verify(&signed_data, &self.signature, hash_type) {
                    crate::serial_println!(
                        "[DNSSEC] ✓ RSA-{} signature verified (key_tag={}, algo={})",
                        hash_type.to_uppercase(),
                        self.key_tag,
                        self.algorithm
                    );
                    true
                } else {
                    crate::serial_println!(
                        "[DNSSEC] ✗ RSA-{} signature verification failed (key_tag={}, algo={})",
                        hash_type.to_uppercase(),
                        self.key_tag,
                        self.algorithm
                    );
                    false
                }
            }
            _ => {
                if let Some(verifier) = lookup_algorithm_verifier(self.algorithm) {
                    let verified = verifier(&key.public_key, &self.signature, &signed_data);
                    crate::serial_println!(
                        "[DNSSEC] {} custom verifier {} (key_tag={})",
                        if verified { "✓" } else { "✗" },
                        self.algorithm,
                        self.key_tag
                    );
                    verified
                } else {
                    crate::serial_println!(
                        "[DNSSEC] No verifier registered for algorithm {} (key_tag={})",
                        self.algorithm,
                        self.key_tag
                    );
                    false
                }
            }
        }
    }

    /// İmzanın zaman geçerliliğini kontrol eder.
    ///
    /// `inception <= current_time <= expiration` koşulu sağlanmalıdır.
    /// Zaman dışı imzalar geçersiz (Bogus) kabul edilmelidir.
    pub fn is_time_valid(&self, current_time: u32) -> bool {
        current_time >= self.signature_inception && current_time <= self.signature_expiration
    }

    /// İmzalanmış veri oluşturur (RFC 4034 Section 6.3).
    ///
    /// RRSIG doğrulaması için kanonik formatta signed data oluşturulur:
    ///   signed_data = RRSIG_RDATA (imza hariç) || RRset (kanonik sıra)
    ///
    /// Parametre:
    ///   - rrset: DNS kayıt kümesi (wire format veya canonical form)
    /// Dönüş:
    ///   - Vec<u8>: Doğrulama için kullanılacak byte dizisi
    fn build_signed_data(&self, rrset: &[u8]) -> Vec<u8> {
        // RFC 4034 Section 6.3.2: Canonical RR Form
        // Signed data = RRSIG RDATA (signature hariç) + RRset wire format

        let mut signed_data = Vec::with_capacity(18 + rrset.len());

        // RRSIG RDATA'nın ilk kısmı (imza hariç, 18 byte):
        // type_covered(2) + algorithm(1) + labels(1) + original_ttl(4) +
        // expiration(4) + inception(4) + key_tag(2) + signer_name(değişken)
        signed_data.extend_from_slice(&self.type_covered.to_be_bytes());
        signed_data.push(self.algorithm);
        signed_data.push(self.labels);
        signed_data.extend_from_slice(&self.original_ttl.to_be_bytes());
        signed_data.extend_from_slice(&self.signature_expiration.to_be_bytes());
        signed_data.extend_from_slice(&self.signature_inception.to_be_bytes());
        signed_data.extend_from_slice(&self.key_tag.to_be_bytes());

        // Signer name canonical wire form: lower-case labels + root terminator.
        signed_data.extend_from_slice(&canonical_dns_name_wire(&self.signer_name));

        // RRset'i ekle (zaten kanonik formatta olduğunu varsayıyoruz)
        // Canonical form: owner name, class, TTL, RDATA length, RDATA
        signed_data.extend_from_slice(rrset);

        signed_data
    }
}

fn canonical_dns_name_wire(name: &str) -> Vec<u8> {
    let trimmed = name.trim_end_matches('.');
    if trimmed.is_empty() {
        return vec![0];
    }

    let mut wire = Vec::with_capacity(trimmed.len() + 2);
    for label in trimmed.split('.') {
        let lower = label.to_ascii_lowercase();
        wire.push(lower.len() as u8);
        wire.extend_from_slice(lower.as_bytes());
    }
    wire.push(0);
    wire
}

fn verify_ecdsa_signature<F>(
    public_key: &[u8],
    signature: &[u8],
    coord_size: usize,
    key_tag: u16,
    curve_name: &str,
    verify: F,
) -> bool
where
    F: FnOnce(&[u8], &[u8]) -> bool,
{
    if public_key.len() != coord_size * 2 {
        crate::serial_println!(
            "[DNSSEC] Invalid ECDSA {} key length: {}",
            curve_name,
            public_key.len()
        );
        return false;
    }

    if signature.len() != coord_size * 2 {
        crate::serial_println!(
            "[DNSSEC] Invalid ECDSA {} signature length: {}",
            curve_name,
            signature.len()
        );
        return false;
    }

    if verify(public_key, signature) {
        crate::serial_println!(
            "[DNSSEC] ✓ ECDSA {} signature verified (key_tag={})",
            curve_name,
            key_tag
        );
        true
    } else {
        crate::serial_println!(
            "[DNSSEC] ✗ ECDSA {} signature verification failed (key_tag={})",
            curve_name,
            key_tag
        );
        false
    }
}

fn sec1_uncompressed_key(public_key: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(public_key.len() + 1);
    encoded.push(0x04);
    encoded.extend_from_slice(public_key);
    encoded
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn verify_p256_signature(public_key: &[u8], signature: &[u8], signed_data: &[u8]) -> bool {
    let sec1 = sec1_uncompressed_key(public_key);
    let Ok(verifying_key) = P256VerifyingKey::from_sec1_bytes(&sec1) else {
        return false;
    };
    let Ok(signature) = P256Signature::from_slice(signature) else {
        return false;
    };
    verifying_key.verify(signed_data, &signature).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn verify_p256_signature(_public_key: &[u8], _signature: &[u8], _signed_data: &[u8]) -> bool {
    false
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn verify_p384_signature(public_key: &[u8], signature: &[u8], signed_data: &[u8]) -> bool {
    let sec1 = sec1_uncompressed_key(public_key);
    let Ok(verifying_key) = P384VerifyingKey::from_sec1_bytes(&sec1) else {
        return false;
    };
    let Ok(signature) = P384Signature::from_slice(signature) else {
        return false;
    };
    verifying_key.verify(signed_data, &signature).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn verify_p384_signature(_public_key: &[u8], _signature: &[u8], _signed_data: &[u8]) -> bool {
    false
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn verify_ed448_signature(public_key: &[u8], signature: &[u8], signed_data: &[u8]) -> bool {
    let Ok(public_key) = <&[u8; 57]>::try_from(public_key) else {
        return false;
    };
    let Ok(verifying_key) = Ed448VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Ed448Signature::try_from(signature) else {
        return false;
    };
    verifying_key.verify_raw(&signature, signed_data).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn verify_ed448_signature(_public_key: &[u8], _signature: &[u8], _signed_data: &[u8]) -> bool {
    false
}

/// DS kaydı (Delegation Signer).
///
/// Üst zone, alt zone'un KSK DNSKEY kaydının hash'ini DS olarak saklar.
/// Bu sayede üst zoneden alt zone'un anahtarına güven aktarımı sağlanır.
///
/// ```text
/// DS Doğrulama:
///  1. Alt zone'dan DNSKEY al (KSK)
///  2. DNSKEY'in hash'ini hesapla: SHA-256(domain + DNSKEY_RDATA)
///  3. Üst zone'dan DS kaydını al
///  4. Hesaplanan hash == DS.digest ise güven zinciri doğrulandı
/// ```
#[derive(Clone, Debug)]
pub struct DsRecord {
    pub key_tag: u16,    // DS'in kapsadığı DNSKEY'in key tag'i
    pub algorithm: u8,   // İmza algoritması kodu
    pub digest_type: u8, // Özet algoritması: 1=SHA-1, 2=SHA-256, 4=SHA-384
    pub digest: Vec<u8>, // DNSKEY'in hesaplanmış özeti (hash)
}

impl DsRecord {
    /// DS RDATA'sından kayıt ayrıştırır.
    ///
    /// Format: KeyTag(2) + Algorithm(1) + DigestType(1) + Digest(var.)
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        if rdata.len() < 4 {
            return None;
        }

        let key_tag = u16::from_be_bytes([rdata[0], rdata[1]]);
        let algorithm = rdata[2];
        let digest_type = rdata[3];
        let digest = rdata[4..].to_vec();

        Some(DsRecord {
            key_tag,
            algorithm,
            digest_type,
            digest,
        })
    }

    /// DNSKEY'den DS özetini hesaplar (RFC 4034 Bölüm 5.1.4).
    ///
    /// Özet girdisi: DNS alan adı (wire format, küçük harf) + DNSKEY RDATA
    /// Kullanılan hash fonksiyonu: digest_type'a göre SHA-256 veya SHA-384
    pub fn calculate(key: &DnsKey, domain: &str, digest_type: u8) -> Option<Vec<u8>> {
        // Özet için girdi verisi: domain (wire format) + DNSKEY RDATA
        let mut data = Vec::new();

        // Domain adını kanonik (küçük harf) wire formatına çevir
        for label in domain.split('.') {
            data.push(label.len() as u8);
            for c in label.to_lowercase().chars() {
                data.push(c as u8);
            }
        }
        data.push(0); // Root label

        // DNSKEY RDATA'sını ekle (Flags + Protocol + Algorithm + PublicKey)
        data.extend_from_slice(&key.flags.to_be_bytes());
        data.push(key.protocol);
        data.push(key.algorithm);
        data.extend_from_slice(&key.public_key);

        // Belirtilen özet türüne göre hash hesapla
        match digest_type {
            DIGEST_SHA1 => {
                let mut hasher = Sha1::new();
                hasher.update(&data);
                Some(hasher.finalize().to_vec())
            }
            DIGEST_SHA256 => {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                Some(hasher.finalize().to_vec())
            }
            DIGEST_SHA384 => {
                let mut hasher = Sha384::new();
                hasher.update(&data);
                Some(hasher.finalize().to_vec())
            }
            _ => lookup_digest_calculator(digest_type)
                .and_then(|calculator| calculator(key, domain, &data)),
        }
    }

    /// DS kaydının belirtilen DNSKEY ile eşleşip eşleşmediğini doğrular.
    ///
    /// DNSKEY'in hash'ini hesaplar ve saklanan digest ile karşılaştırır.
    pub fn verify(&self, key: &DnsKey, domain: &str) -> bool {
        if let Some(digest) = Self::calculate(key, domain, self.digest_type) {
            digest == self.digest
        } else {
            false
        }
    }
}

/// NSEC kaydı (Next Secure).
///
/// Alfabetik olarak sıralanmış bir zone'da, bu kaydın adından sonraki
/// mevcut kaydın adını gösterir. Bu sayede NXDOMAIN yanıtları
/// kriptografik olarak kanıtlanabilir hale gelir.
///
/// ```text
/// Zone: example.com -> mail.example.com -> www.example.com
///
/// NSEC example.com: "Bir sonraki mevcut ad: mail.example.com"
///                   "Bu aralıkta kayıt türleri: A, NS, SOA, RRSIG, NSEC"
///
/// "ftp.example.com" sorgulandığında:
///   example.com NSEC kaydı gösterir ki "example.com" ile "mail.example.com"
///   arasında hiçbir isim yok, dolayısıyla "ftp.example.com" NXDOMAIN.
/// ```
#[derive(Clone, Debug)]
pub struct NsecRecord {
    pub next_name: String,    // Bir sonraki mevcut isim (alfabetik sıra)
    pub type_bitmap: Vec<u8>, // Bu isimde mevcut kayıt türlerinin bit haritası
}

impl NsecRecord {
    /// NSEC RDATA'sından kayıt ayrıştırır.
    ///
    /// Format: NextName(wire format) + TypeBitmap(değişken)
    pub fn parse(rdata: &[u8]) -> Option<Self> {
        let mut offset = 0;
        let next_name = RrSig::parse_name(rdata, &mut offset)?;
        let type_bitmap = rdata[offset..].to_vec();

        Some(NsecRecord {
            next_name,
            type_bitmap,
        })
    }

    /// Bit haritasında belirtilen kayıt türünün olup olmadığını kontrol eder.
    ///
    /// Bit haritası formatı (RFC 4034 Bölüm 4.1.2):
    /// Window(1) | Bitmap Length(1) | Bitmap(0-32 byte)
    /// Her bit bir kayıt türünü temsil eder (top bit = düşük tür numarası)
    pub fn has_type(&self, qtype: u16) -> bool {
        if self.type_bitmap.is_empty() {
            return false;
        }

        let window = (qtype >> 8) as usize; // Pencere numarası (üst byte)
        let bit = (qtype & 0xFF) as usize; // Bit konumu (alt byte)

        let mut offset = 0;
        while offset + 2 <= self.type_bitmap.len() {
            let win = self.type_bitmap[offset] as usize; // Pencere numarası
            let len = self.type_bitmap[offset + 1] as usize; // Bit haritası uzunluğu

            if win == window && bit < len * 8 {
                let byte_idx = offset + 2 + bit / 8;
                if byte_idx < self.type_bitmap.len() {
                    // En yüksek bit en küçük tür numarasına karşılık gelir
                    return (self.type_bitmap[byte_idx] & (0x80 >> (bit % 8))) != 0;
                }
            }

            offset += 2 + len; // Sonraki pencereye geç
        }

        false
    }
}

/// DNSSEC doğrulama durumu.
///
/// RFC 4033 Bölüm 5'te tanımlanan dört durum:
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnssecState {
    Secure,        // Güven zinciri doğrulandı, kayıt imzalanmış ve geçerli
    Insecure,      // DNSSEC imzası yok ama bu beklenen bir durum (unsigned zone)
    Bogus,         // İmza var ama geçersiz veya zaman dışı (saldırı olabilir!)
    Indeterminate, // Doğrulama yapılamadı (yeterli bilgi yok)
}

/// DNSSEC güven çapası.
///
/// Güven zincirinin başladığı nokta. Genellikle DNS kökü (.)
/// için ICANN tarafından yönetilen DNSKEY kaydıdır.
#[derive(Clone, Debug)]
pub struct TrustAnchor {
    pub domain: String,       // Güven çapasının alan adı (genelde "." kök için)
    pub dnskey: DnsKey,       // Güvenilen açık anahtar
    pub ds: Option<DsRecord>, // İsteğe bağlı DS kaydı (üst zone doğrulaması için)
}

/// DNSSEC doğrulayıcısı.
///
/// Güven çapalarını ve önbelleklenen anahtarları yönetir.
/// DNS yanıtlarının kriptografik geçerliliğini doğrular.
#[derive(Clone)]
pub struct DnssecValidator {
    pub trust_anchors: Vec<TrustAnchor>, // Güven çapaları (genelde kök zone)
    pub cached_keys: BTreeMap<String, Vec<DnsKey>>, // Önbelleklenen DNSKEY kayıtları
    pub cached_ds: BTreeMap<String, Vec<DsRecord>>, // Önbelleklenen DS kayıtları
}

impl DnssecValidator {
    pub fn new() -> Self {
        DnssecValidator {
            trust_anchors: Vec::new(),
            cached_keys: BTreeMap::new(),
            cached_ds: BTreeMap::new(),
        }
    }

    /// Kök zone için güven çapası ekler.
    ///
    /// ICANN'ın yayımladığı KSK anahtarı ile güven zinciri başlatılır.
    /// Pratikte bu anahtar işletim sistemi veya yazılım ile birlikte gelir.
    pub fn add_root_anchor(&mut self, key: DnsKey, ds: Option<DsRecord>) {
        self.trust_anchors.push(TrustAnchor {
            domain: ".".to_string(), // Kök zone
            dnskey: key,
            ds,
        });
    }

    /// Bir DNSKEY kaydını doğrular.
    ///
    /// Güven çapaları ve DS kayıtları kullanılarak doğrulama yapılır.
    /// Sonuç: Secure (doğrulandı) veya Insecure (doğrulanamadı).
    pub fn validate_dnskey(
        &self,
        domain: &str,
        key: &DnsKey,
        ds: Option<&DsRecord>,
    ) -> DnssecState {
        // Güven çapalarıyla doğrula (kök zone için)
        for anchor in &self.trust_anchors {
            if anchor.domain == "." || domain.ends_with(&anchor.domain) {
                if let Some(anchor_ds) = &anchor.ds {
                    if anchor_ds.verify(key, domain) {
                        return DnssecState::Secure;
                    }
                }
            }
        }

        // Üst zone'dan gelen DS kaydı ile doğrula
        if let Some(ds) = ds {
            if ds.verify(key, domain) {
                return DnssecState::Secure;
            }
        }

        DnssecState::Insecure // Doğrulama yapılamadı
    }

    /// Bir RRset'i RRSIG ve DNSKEY kullanarak doğrular.
    ///
    /// Önce imzanın zaman geçerliliği, sonra kriptografik imza doğrulanır.
    pub fn validate_rrset(
        &self,
        _rrset: &[u8],
        rrsig: &RrSig,
        key: &DnsKey,
        current_time: u32,
    ) -> DnssecState {
        // İmzanın zaman geçerliliğini kontrol et
        if !rrsig.is_time_valid(current_time) {
            return DnssecState::Bogus; // Süresi dolmuş veya henüz geçerli değil
        }

        // Kriptografik imza doğrulaması
        if rrsig.verify_signature(_rrset, key) {
            DnssecState::Secure
        } else {
            DnssecState::Bogus // İmza geçersiz: veri değiştirilmiş olabilir!
        }
    }

    /// Bir domain için DNSKEY kaydını önbelleğe alır.
    pub fn cache_key(&mut self, domain: &str, key: DnsKey) {
        self.cached_keys
            .entry(domain.to_string())
            .or_insert_with(Vec::new)
            .push(key);
    }

    /// Bir domain için DS kaydını önbelleğe alır.
    pub fn cache_ds(&mut self, domain: &str, ds: DsRecord) {
        self.cached_ds
            .entry(domain.to_string())
            .or_insert_with(Vec::new)
            .push(ds);
    }

    /// Bir domain için önbelleklenen DNSKEY kayıtlarını döner.
    pub fn get_keys(&self, domain: &str) -> Option<&Vec<DnsKey>> {
        self.cached_keys.get(domain)
    }
}

impl Default for DnssecValidator {
    fn default() -> Self {
        Self::new()
    }
}

// Global DNSSEC doğrulayıcısı: tüm DNSSEC doğrulama işlemleri bu nesne üzerinden yürütülür
lazy_static::lazy_static! {
    static ref DNSSEC_VALIDATOR: Mutex<DnssecValidator> = Mutex::new(DnssecValidator::new());
    static ref DNSSEC_ALGORITHM_REGISTRY: Mutex<BTreeMap<u8, DnssecAlgorithmVerifier>> =
        Mutex::new(BTreeMap::new());
    static ref DNSSEC_DS_DIGEST_REGISTRY: Mutex<BTreeMap<u8, DnssecDigestCalculator>> =
        Mutex::new(BTreeMap::new());
}

// ============================================================================
// NSEC3 Kaydı (RFC 5155)
// ============================================================================
//
// NSEC3, NSEC'in hash'lenmiş versiyonudur. Zone enumeration saldırılarını
// önlemek için alan adları hash'lenerek saklanır.
//
// ```text
// NSEC3 Wire Format:
//  Hash Algorithm (1) | Flags (1) | Iterations (2) | Salt Length (1)
//  Salt (değişken) | Hash Length (1) | Next Hashed Owner (değişken)
//  Type Bitmap (değişken)
//
// Hash hesaplama:
//  H(ad) = hash( ad | salt )  — iterations kez tekrarlanır
//  IH(ad, 0) = H(ad)
//  IH(ad, k) = H( IH(ad, k-1) | salt )
//
// Sonuç hash Base32hex ile kodlanarak owner name olarak kullanılır.
// ```

/// NSEC3 kaydı (Next Secure v3 — hash'lenmiş negatif yanıt kanıtı).
///
/// NSEC3, zone'daki alan adlarını hash'leyerek gizler.
/// Bu sayede bir saldırgan zone'daki tüm adları sıralayamaz.
#[derive(Clone, Debug)]
pub struct Nsec3Record {
    /// Hash algoritması (1 = SHA-1, RFC 5155 Bölüm 4.1)
    pub hash_algorithm: u8,
    /// Bayraklar (bit 0 = Opt-Out: imzasız delegasyonları atla)
    pub flags: u8,
    /// Hash iterasyon sayısı (brute-force zorlaştırma)
    pub iterations: u16,
    /// Tuz değeri (hash girdisine eklenerek gökkuşağı tablosu saldırısını önler)
    pub salt: Vec<u8>,
    /// Owner adının ilk label'ından çözülen gerçek NSEC3 hash'i.
    /// Owner label çözülemiyorsa tam interval kanıtı üretilemez.
    pub owner_hashed_label: Option<Vec<u8>>,
    /// Sıradaki hash'lenmiş owner name (sıralı zincirdeki bir sonraki kayıt)
    pub next_hashed_owner: Vec<u8>,
    /// Bu isimde mevcut kayıt türlerinin bit haritası (NSEC ile aynı format)
    pub type_bitmap: Vec<u8>,
}

impl Nsec3Record {
    /// NSEC3 RDATA'sından kayıt ayrıştırır.
    ///
    /// Format:
    ///   HashAlgorithm(1) | Flags(1) | Iterations(2) | SaltLength(1) |
    ///   Salt(var.) | HashLength(1) | NextHashedOwner(var.) | TypeBitmap(var.)
    pub fn parse(data: &[u8]) -> Option<Self> {
        // Minimum: 1+1+2+1 = 5 byte (salt_len ve hash_len dahil olmadan)
        if data.len() < 5 {
            return None;
        }

        let hash_algorithm = data[0];
        let flags = data[1];
        let iterations = u16::from_be_bytes([data[2], data[3]]);

        let salt_length = data[4] as usize;
        let mut offset = 5;

        if offset + salt_length >= data.len() {
            return None;
        }
        let salt = data[offset..offset + salt_length].to_vec();
        offset += salt_length;

        if offset >= data.len() {
            return None;
        }
        let hash_length = data[offset] as usize;
        offset += 1;

        if offset + hash_length > data.len() {
            return None;
        }
        let next_hashed_owner = data[offset..offset + hash_length].to_vec();
        offset += hash_length;

        let type_bitmap = if offset < data.len() {
            data[offset..].to_vec()
        } else {
            Vec::new()
        };

        Some(Nsec3Record {
            hash_algorithm,
            flags,
            iterations,
            salt,
            owner_hashed_label: None,
            next_hashed_owner,
            type_bitmap,
        })
    }

    pub fn parse_with_owner(owner_name: &str, data: &[u8]) -> Option<Self> {
        let mut record = Self::parse(data)?;
        record.owner_hashed_label = decode_nsec3_owner_label(owner_name);
        Some(record)
    }

    /// Verilen alan adının bu NSEC3 kaydının `next_hashed_owner` üst sınırının
    /// altında kalıp kalmadığını kontrol eder.
    ///
    /// Tam doğrulama için:
    ///   1. Alan adını hash'le: IH(name, iterations) = H(H(…H(name|salt)…)|salt)
    ///   2. Hash'i bu kaydın owner hash'i ile next_hashed_owner arasında mı kontrol et
    ///
    /// Bu kayıt owner-side hashed label çözülmüşse RFC 5155 tarzı dairesel
    /// interval karşılaştırması üretir; owner hash yoksa doğrulama yapılmaz.
    pub fn covers_name(&self, name: &str) -> bool {
        let owner_hash = match self.owner_hashed_label.as_ref() {
            Some(owner_hash) if !owner_hash.is_empty() => owner_hash.as_slice(),
            _ => return false,
        };
        if self.hash_algorithm != 1 || self.next_hashed_owner.is_empty() {
            return false;
        }
        let candidate_hash = hash_nsec3_name(name, &self.salt, self.iterations);
        compare_nsec3_interval(owner_hash, &self.next_hashed_owner, &candidate_hash)
    }

    /// Bit haritasında belirtilen kayıt türünün olup olmadığını kontrol eder.
    ///
    /// NSEC ile aynı bit haritası formatını kullanır (RFC 4034 Bölüm 4.1.2).
    pub fn has_type(&self, qtype: u16) -> bool {
        if self.type_bitmap.is_empty() {
            return false;
        }

        let window = (qtype >> 8) as usize;
        let bit = (qtype & 0xFF) as usize;

        let mut offset = 0;
        while offset + 2 <= self.type_bitmap.len() {
            let win = self.type_bitmap[offset] as usize;
            let len = self.type_bitmap[offset + 1] as usize;

            if win == window && bit < len * 8 {
                let byte_idx = offset + 2 + bit / 8;
                if byte_idx < self.type_bitmap.len() {
                    return (self.type_bitmap[byte_idx] & (0x80 >> (bit % 8))) != 0;
                }
            }

            offset += 2 + len;
        }

        false
    }
}

fn hash_nsec3_name(name: &str, salt: &[u8], iterations: u16) -> Vec<u8> {
    let wire = canonical_dns_name_wire(name);
    let mut hasher = Sha1::new();
    hasher.update(&wire);
    hasher.update(salt);
    let mut digest = hasher.finalize().to_vec();

    for _ in 0..iterations {
        let mut hasher = Sha1::new();
        hasher.update(&digest);
        hasher.update(salt);
        digest = hasher.finalize().to_vec();
    }

    digest
}

fn compare_nsec3_interval(owner: &[u8], next: &[u8], candidate: &[u8]) -> bool {
    match owner.cmp(next) {
        core::cmp::Ordering::Less => owner < candidate && candidate < next,
        core::cmp::Ordering::Greater => candidate > owner || candidate < next,
        core::cmp::Ordering::Equal => candidate != owner,
    }
}

fn decode_nsec3_owner_label(owner_name: &str) -> Option<Vec<u8>> {
    let first_label = owner_name
        .trim_end_matches('.')
        .split('.')
        .next()
        .filter(|label| !label.is_empty())?;
    decode_base32hex_no_pad(first_label)
}

fn decode_base32hex_no_pad(label: &str) -> Option<Vec<u8>> {
    let mut acc = 0u32;
    let mut bits = 0u8;
    let mut out = Vec::new();

    for ch in label.bytes() {
        let value = match ch {
            b'0'..=b'9' => ch - b'0',
            b'A'..=b'V' => 10 + (ch - b'A'),
            b'a'..=b'v' => 10 + (ch - b'a'),
            _ => return None,
        } as u32;

        acc = (acc << 5) | value;
        bits += 5;

        while bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
            acc &= (1 << bits) - 1;
        }
    }

    Some(out)
}

#[cfg(test)]
fn encode_base32hex_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";
    let mut acc = 0u32;
    let mut bits = 0u8;
    let mut out = String::new();

    for byte in bytes {
        acc = (acc << 8) | *byte as u32;
        bits += 8;

        while bits >= 5 {
            bits -= 5;
            let idx = ((acc >> bits) & 0x1f) as usize;
            out.push(ALPHABET[idx] as char);
            acc &= (1 << bits) - 1;
        }
    }

    if bits > 0 {
        let idx = ((acc << (5 - bits)) & 0x1f) as usize;
        out.push(ALPHABET[idx] as char);
    }

    out
}

/// Global DNSSEC doğrulayıcısının bir kopyasını döner.
pub fn get_validator() -> DnssecValidator {
    DNSSEC_VALIDATOR.lock().clone()
}

/// Global doğrulayıcıya güven çapası ekler.
pub fn add_trust_anchor(key: DnsKey, ds: Option<DsRecord>) {
    DNSSEC_VALIDATOR.lock().add_root_anchor(key, ds);
}

pub fn register_signature_algorithm(algorithm: u8, verifier: DnssecAlgorithmVerifier) {
    DNSSEC_ALGORITHM_REGISTRY.lock().insert(algorithm, verifier);
}

fn lookup_algorithm_verifier(algorithm: u8) -> Option<DnssecAlgorithmVerifier> {
    DNSSEC_ALGORITHM_REGISTRY.lock().get(&algorithm).copied()
}

pub fn register_ds_digest(digest_type: u8, calculator: DnssecDigestCalculator) {
    DNSSEC_DS_DIGEST_REGISTRY
        .lock()
        .insert(digest_type, calculator);
}

fn lookup_digest_calculator(digest_type: u8) -> Option<DnssecDigestCalculator> {
    DNSSEC_DS_DIGEST_REGISTRY.lock().get(&digest_type).copied()
}

/// Global DNSSEC doğrulayıcısı kullanarak bir DNSKEY kaydını doğrular.
pub fn validate_dnssec(domain: &str, key: &DnsKey, ds: Option<&DsRecord>) -> DnssecState {
    DNSSEC_VALIDATOR.lock().validate_dnskey(domain, key, ds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed448_goldilocks_plus::SigningKey as Ed448SigningKey;
    use p256::ecdsa::{signature::Signer as P256Signer, SigningKey as P256SigningKey};
    use p384::ecdsa::{signature::Signer as P384Signer, SigningKey as P384SigningKey};
    use rsa::rand_core::{CryptoRng, Error as RandError, RngCore};
    use rsa::traits::PublicKeyParts;
    use rsa::{Pkcs1v15Sign, RsaPrivateKey as ExternalRsaPrivateKey};

    const TEST_CUSTOM_ALGORITHM: u8 = 253;
    const TEST_CUSTOM_DIGEST: u8 = 253;

    const RSA_SHA1_DIGESTINFO_PREFIX: &[u8] = &[
        0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
    ];

    struct TestCryptoRng {
        state: u64,
    }

    impl TestCryptoRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_word(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }
    }

    impl RngCore for TestCryptoRng {
        fn next_u32(&mut self) -> u32 {
            self.next_word() as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.next_word()
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            let mut offset = 0;
            while offset < dest.len() {
                let word = self.next_word().to_le_bytes();
                let chunk = core::cmp::min(8, dest.len() - offset);
                dest[offset..offset + chunk].copy_from_slice(&word[..chunk]);
                offset += chunk;
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RandError> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    impl CryptoRng for TestCryptoRng {}

    fn xor_signature_verifier(public_key: &[u8], signature: &[u8], signed_data: &[u8]) -> bool {
        if public_key.is_empty() || signature.len() != signed_data.len() {
            return false;
        }
        let key = public_key[0];
        signed_data
            .iter()
            .zip(signature.iter())
            .all(|(msg, sig)| (*msg ^ key) == *sig)
    }

    fn xor_digest_calculator(
        key: &DnsKey,
        _domain: &str,
        canonical_data: &[u8],
    ) -> Option<Vec<u8>> {
        if key.public_key.is_empty() || canonical_data.is_empty() {
            return None;
        }
        let seed = key.public_key[0];
        let mut digest = vec![0u8; 24];
        let digest_len = digest.len();
        for (idx, byte) in canonical_data.iter().copied().enumerate() {
            let slot = idx % digest_len;
            digest[slot] ^= byte ^ seed ^ idx as u8;
        }
        Some(digest)
    }

    fn encode_dnskey_rsa_public_key(pubkey: &crate::crypto::rsa::RsaPublicKey) -> Vec<u8> {
        let exponent = pubkey.exponent_bytes();
        let modulus = pubkey.modulus_bytes();
        let mut encoded = Vec::with_capacity(1 + exponent.len() + modulus.len());
        encoded.push(exponent.len() as u8);
        encoded.extend_from_slice(&exponent);
        encoded.extend_from_slice(&modulus);
        encoded
    }

    #[test]
    fn ds_calculate_uses_rfc_digest_lengths() {
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: RSA_SHA256,
            public_key: vec![0x01, 0x02, 0x03, 0x04, 0x05],
            key_tag: 0,
        };

        let sha1 = DsRecord::calculate(&key, "example.com", DIGEST_SHA1).unwrap();
        let sha256 = DsRecord::calculate(&key, "example.com", DIGEST_SHA256).unwrap();
        let sha384 = DsRecord::calculate(&key, "example.com", DIGEST_SHA384).unwrap();

        assert_eq!(sha1.len(), 20);
        assert_eq!(sha256.len(), 32);
        assert_eq!(sha384.len(), 48);
        assert_ne!(sha1, sha256);
        assert_ne!(sha256, sha384);
    }

    #[test]
    fn rrsig_verify_signature_accepts_rsa_sha1_family() {
        let mut keygen_rng = TestCryptoRng::new(0x4d59_5df4_d0f3_3173);
        let private = ExternalRsaPrivateKey::new(&mut keygen_rng, 1024)
            .expect("deterministic RSA key generation");
        let public = crate::crypto::rsa::RsaPublicKey::new(
            &private.n().to_bytes_be(),
            &private.e().to_bytes_be(),
        );
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: RSA_SHA1,
            public_key: encode_dnskey_rsa_public_key(&public),
            key_tag: 0,
        };
        let key = DnsKey {
            key_tag: DnsKey::calculate_key_tag(
                key.flags,
                key.protocol,
                key.algorithm,
                &key.public_key,
            ),
            ..key
        };

        let rrset = b"\x03www\x07example\x03com\x00\x00\x01\x00\x01";
        let signer_name = "example.com".to_string();
        let unsigned_rrsig = RrSig {
            type_covered: 1,
            algorithm: RSA_SHA1,
            labels: 3,
            original_ttl: 300,
            signature_expiration: u32::MAX,
            signature_inception: 0,
            key_tag: key.key_tag,
            signer_name,
            signature: Vec::new(),
        };
        let signed_data = unsigned_rrsig.build_signed_data(rrset);
        let mut hasher = Sha1::new();
        hasher.update(&signed_data);
        let hash = hasher.finalize();
        let padding = Pkcs1v15Sign {
            hash_len: Some(20),
            prefix: RSA_SHA1_DIGESTINFO_PREFIX.to_vec().into_boxed_slice(),
        };
        let mut sign_rng = TestCryptoRng::new(0xa5a5_5a5a_1357_2468);
        let signature = private
            .sign_with_rng(&mut sign_rng, padding, hash.as_slice())
            .expect("RSA SHA-1 PKCS#1 v1.5 signing");
        let rrsig = RrSig {
            signature,
            ..unsigned_rrsig
        };

        assert!(rrsig.verify_signature(rrset, &key));
    }

    #[test]
    fn rrsig_verify_signature_accepts_ecdsa_p256_sha256_family() {
        let signing_key =
            P256SigningKey::from_slice(&[0x11; 32]).expect("deterministic p256 signing key");
        let verifying_key = signing_key.verifying_key();
        let encoded = verifying_key.to_encoded_point(false);
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: ECDSA_P256_SHA256,
            public_key: encoded.as_bytes()[1..].to_vec(),
            key_tag: 0,
        };
        let key = DnsKey {
            key_tag: DnsKey::calculate_key_tag(
                key.flags,
                key.protocol,
                key.algorithm,
                &key.public_key,
            ),
            ..key
        };

        let rrset = b"\x03www\x07example\x03com\x00\x00\x01\x00\x01";
        let unsigned_rrsig = RrSig {
            type_covered: 1,
            algorithm: ECDSA_P256_SHA256,
            labels: 3,
            original_ttl: 300,
            signature_expiration: u32::MAX,
            signature_inception: 0,
            key_tag: key.key_tag,
            signer_name: "example.com".to_string(),
            signature: Vec::new(),
        };
        let signed_data = unsigned_rrsig.build_signed_data(rrset);
        let signature: p256::ecdsa::Signature = signing_key.sign(&signed_data);
        let rrsig = RrSig {
            signature: signature.to_bytes().to_vec(),
            ..unsigned_rrsig
        };

        assert!(rrsig.verify_signature(rrset, &key));
    }

    #[test]
    fn rrsig_verify_signature_accepts_ecdsa_p384_sha384_family() {
        let signing_key =
            P384SigningKey::from_slice(&[0x22; 48]).expect("deterministic p384 signing key");
        let verifying_key = signing_key.verifying_key();
        let encoded = verifying_key.to_encoded_point(false);
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: ECDSA_P384_SHA384,
            public_key: encoded.as_bytes()[1..].to_vec(),
            key_tag: 0,
        };
        let key = DnsKey {
            key_tag: DnsKey::calculate_key_tag(
                key.flags,
                key.protocol,
                key.algorithm,
                &key.public_key,
            ),
            ..key
        };

        let rrset = b"\x03api\x07example\x03com\x00\x00\x10\x00\x01";
        let unsigned_rrsig = RrSig {
            type_covered: 16,
            algorithm: ECDSA_P384_SHA384,
            labels: 3,
            original_ttl: 300,
            signature_expiration: u32::MAX,
            signature_inception: 0,
            key_tag: key.key_tag,
            signer_name: "example.com".to_string(),
            signature: Vec::new(),
        };
        let signed_data = unsigned_rrsig.build_signed_data(rrset);
        let signature: p384::ecdsa::Signature = signing_key.sign(&signed_data);
        let rrsig = RrSig {
            signature: signature.to_bytes().to_vec(),
            ..unsigned_rrsig
        };

        assert!(rrsig.verify_signature(rrset, &key));
    }

    #[test]
    fn rrsig_verify_signature_accepts_ed448_family() {
        let signing_key =
            Ed448SigningKey::try_from(&[0x33; 57][..]).expect("deterministic ed448 signing key");
        let verifying_key = signing_key.verifying_key();
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: ED448,
            public_key: verifying_key.as_bytes().to_vec(),
            key_tag: 0,
        };
        let key = DnsKey {
            key_tag: DnsKey::calculate_key_tag(
                key.flags,
                key.protocol,
                key.algorithm,
                &key.public_key,
            ),
            ..key
        };

        let rrset = b"\x04edge\x07example\x03com\x00\x00\x1c\x00\x01";
        let unsigned_rrsig = RrSig {
            type_covered: 28,
            algorithm: ED448,
            labels: 3,
            original_ttl: 300,
            signature_expiration: u32::MAX,
            signature_inception: 0,
            key_tag: key.key_tag,
            signer_name: "example.com".to_string(),
            signature: Vec::new(),
        };
        let signed_data = unsigned_rrsig.build_signed_data(rrset);
        let signature = signing_key.sign_raw(&signed_data);
        let rrsig = RrSig {
            signature: signature.to_bytes().to_vec(),
            ..unsigned_rrsig
        };

        assert!(rrsig.verify_signature(rrset, &key));
    }

    #[test]
    fn build_signed_data_canonicalizes_signer_name_wire_format() {
        let rrsig = RrSig {
            type_covered: 1,
            algorithm: RSA_SHA256,
            labels: 2,
            original_ttl: 300,
            signature_expiration: 0x01020304,
            signature_inception: 0x05060708,
            key_tag: 0x1122,
            signer_name: "MiXeD.Example.COM.".to_string(),
            signature: Vec::new(),
        };

        let signed = rrsig.build_signed_data(&[0xde, 0xad]);
        let signer_wire = canonical_dns_name_wire("mixed.example.com.");

        assert!(signed
            .windows(signer_wire.len())
            .any(|window| window == signer_wire.as_slice()));
        assert_eq!(&signed[signed.len() - 2..], &[0xde, 0xad]);
    }

    #[test]
    fn nsec3_parse_with_owner_and_cover_check_use_real_sha1_interval() {
        let owner_hash = hash_nsec3_name("alpha.example.com.", b"salt", 1);
        let next_hash = vec![0xff; owner_hash.len()];

        let owner_label = encode_base32hex_no_pad(&owner_hash);
        let owner_name = alloc::format!("{}.example.com.", owner_label);

        let mut rdata = Vec::new();
        rdata.push(1);
        rdata.push(0);
        rdata.extend_from_slice(&1u16.to_be_bytes());
        rdata.push(4);
        rdata.extend_from_slice(b"salt");
        rdata.push(next_hash.len() as u8);
        rdata.extend_from_slice(&next_hash);

        let record = Nsec3Record::parse_with_owner(&owner_name, &rdata).expect("nsec3");
        let covered_name = (0..512)
            .map(|idx| alloc::format!("candidate-{}.example.com.", idx))
            .find(|name| record.covers_name(name))
            .expect("need one hash inside interval");

        assert!(record.covers_name(&covered_name));
        assert!(!record.covers_name("alpha.example.com."));
    }

    #[test]
    fn rrsig_verify_signature_uses_registered_custom_algorithm_verifier() {
        register_signature_algorithm(TEST_CUSTOM_ALGORITHM, xor_signature_verifier);

        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: TEST_CUSTOM_ALGORITHM,
            public_key: vec![0x5a],
            key_tag: 0,
        };
        let key = DnsKey {
            key_tag: DnsKey::calculate_key_tag(
                key.flags,
                key.protocol,
                key.algorithm,
                &key.public_key,
            ),
            ..key
        };

        let rrset = b"\x03svc\x07example\x03com\x00\x00\x01\x00\x01";
        let unsigned_rrsig = RrSig {
            type_covered: 1,
            algorithm: TEST_CUSTOM_ALGORITHM,
            labels: 3,
            original_ttl: 300,
            signature_expiration: u32::MAX,
            signature_inception: 0,
            key_tag: key.key_tag,
            signer_name: "example.com".to_string(),
            signature: Vec::new(),
        };
        let signed_data = unsigned_rrsig.build_signed_data(rrset);
        let signature = signed_data.iter().map(|byte| *byte ^ 0x5a).collect();
        let rrsig = RrSig {
            signature,
            ..unsigned_rrsig
        };

        assert!(rrsig.verify_signature(rrset, &key));
    }

    #[test]
    fn ds_calculate_uses_registered_custom_digest_family() {
        register_ds_digest(TEST_CUSTOM_DIGEST, xor_digest_calculator);
        let key = DnsKey {
            flags: 257,
            protocol: 3,
            algorithm: RSA_SHA256,
            public_key: vec![0x42, 0x10, 0x99, 0x77],
            key_tag: 0,
        };
        let digest = DsRecord::calculate(&key, "example.com", TEST_CUSTOM_DIGEST).unwrap();
        assert_eq!(digest.len(), 24);
        let ds = DsRecord {
            key_tag: 0x1001,
            algorithm: key.algorithm,
            digest_type: TEST_CUSTOM_DIGEST,
            digest: digest.clone(),
        };
        assert!(ds.verify(&key, "example.com"));
        let mut mismatched = ds.clone();
        mismatched.digest[0] ^= 0x5a;
        assert!(!mismatched.verify(&key, "example.com"));
    }
}
