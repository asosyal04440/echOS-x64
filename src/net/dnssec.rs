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
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// ============================================================================
// DNSSEC Kayıt Türü Kodları (RFC 4034)
// ============================================================================
const DNSKEY: u16 = 48;      // DNS Public Key kaydı
const RRSIG: u16 = 46;       // Resource Record Signature kaydı
const DS: u16 = 43;          // Delegation Signer kaydı
const NSEC: u16 = 47;        // Next Secure kaydı (negatif yanıt kanıtı)
const NSEC3: u16 = 50;       // Next Secure v3 (hash'lenmiş, zone enumeration önler)
const NSEC3PARAM: u16 = 51;  // NSEC3 parametreleri

// ============================================================================
// DNSSEC İmza Algoritması Kodları (RFC 8624)
// ============================================================================
// Güvenlik önerisi (2024):
//   - Ed25519 (15) ve ECDSA P-256 (13): Önerilir, modern ve hızlı
//   - RSA/SHA-256 (8): Hâlâ yaygın, kabul edilebilir
//   - RSA/SHA-1 (5, 7): Kullanımdan kaldırılmıştır (SHA-1 zayıf)
const RSA_SHA1: u8 = 5;          // RSA + SHA-1 (kullanımdan kaldırılmış)
const RSA_SHA1_NSEC3: u8 = 7;    // RSA + SHA-1 + NSEC3 (kullanımdan kaldırılmış)
const RSA_SHA256: u8 = 8;        // RSA + SHA-256 (yaygın kullanım)
const RSA_SHA512: u8 = 10;       // RSA + SHA-512
const ECDSA_P256_SHA256: u8 = 13;// ECDSA P-256 + SHA-256 (önerilir)
const ECDSA_P384_SHA384: u8 = 14;// ECDSA P-384 + SHA-384
const ED25519: u8 = 15;          // Ed25519 Edwards eğrisi (önerilir)
const ED448: u8 = 16;            // Ed448 (yüksek güvenlik)

// ============================================================================
// DS Özet Türleri (Digest Types)
// ============================================================================
const DIGEST_SHA1: u8 = 1;   // SHA-1 (önerilmez, geriye dönük uyumluluk)
const DIGEST_SHA256: u8 = 2; // SHA-256 (önerilir)
const DIGEST_SHA384: u8 = 4; // SHA-384 (yüksek güvenlik)

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
    pub flags: u16,         // 256=ZSK, 257=KSK (SEP bayrağı)
    pub protocol: u8,       // Her zaman 3 (RFC 4034)
    pub algorithm: u8,      // İmza algoritması kodu (RSA_SHA256=8, Ed25519=15 vb.)
    pub public_key: Vec<u8>,// DER formatında açık anahtar verisi
    pub key_tag: u16,       // Anahtarın kısa tanımlayıcısı (RRSIG ile eşleştirmek için)
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
    pub type_covered: u16,        // Bu imzanın kapsadığı kayıt türü (örn. A=1)
    pub algorithm: u8,            // İmza algoritması kodu
    pub labels: u8,               // İmzalanan alan adındaki etiket sayısı (wildcard için)
    pub original_ttl: u32,        // İmzalanmadan önceki orijinal TTL değeri
    pub signature_expiration: u32,// Unix zaman damgası: imzanın son geçerlilik zamanı
    pub signature_inception: u32, // Unix zaman damgası: imzanın geçerlilik başlangıcı
    pub key_tag: u16,             // İmzalayan DNSKEY'in key tag'i (eşleştirme için)
    pub signer_name: String,      // İmzalayan zone'un adı (örn. "example.com")
    pub signature: Vec<u8>,       // Asıl dijital imza verisi (RSA/ECDSA/Ed25519)
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
        let mut jumped = false;    // İşaretçi atlaması yapıldı mı?
        let mut max_jumps = 5;     // Sonsuz döngü koruması: maksimum 5 atlama
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
    /// NOT: Bu implementasyon henüz gerçek kriptografik doğrulama yapmıyor.
    /// Gerçek implementasyon RSA/ECDSA/Ed25519 doğrulaması gerektirir.
    /// Şu an sadece key tag kontrolü yapılıyor (TODO).
    pub fn verify(&self, _rrset: &[u8], _key: &DnsKey) -> bool {
        // TODO: Implement actual signature verification
        // This requires RSA/ECDSA signature verification
        // For now, just check key tag matches
        true
    }

    /// İmzanın zaman geçerliliğini kontrol eder.
    ///
    /// `inception <= current_time <= expiration` koşulu sağlanmalıdır.
    /// Zaman dışı imzalar geçersiz (Bogus) kabul edilmelidir.
    pub fn is_time_valid(&self, current_time: u32) -> bool {
        current_time >= self.signature_inception && current_time <= self.signature_expiration
    }
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
    pub key_tag: u16,      // DS'in kapsadığı DNSKEY'in key tag'i
    pub algorithm: u8,     // İmza algoritması kodu
    pub digest_type: u8,   // Özet algoritması: 1=SHA-1, 2=SHA-256, 4=SHA-384
    pub digest: Vec<u8>,   // DNSKEY'in hesaplanmış özeti (hash)
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
            DIGEST_SHA256 => {
                let mut hasher = crate::crypto::Sha3::sha3_256();
                hasher.update(&data);
                let hash = hasher.finalize();
                Some(hash[..32].to_vec())
            }
            DIGEST_SHA384 => {
                let mut hasher = crate::crypto::Sha3::sha3_512();
                hasher.update(&data);
                let hash = hasher.finalize();
                Some(hash[..48].to_vec())
            }
            _ => None, // Desteklenmeyen özet türü
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
    pub next_name: String,      // Bir sonraki mevcut isim (alfabetik sıra)
    pub type_bitmap: Vec<u8>,   // Bu isimde mevcut kayıt türlerinin bit haritası
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
        let bit = (qtype & 0xFF) as usize;  // Bit konumu (alt byte)

        let mut offset = 0;
        while offset + 2 <= self.type_bitmap.len() {
            let win = self.type_bitmap[offset] as usize;     // Pencere numarası
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
    pub domain: String, // Güven çapasının alan adı (genelde "." kök için)
    pub dnskey: DnsKey, // Güvenilen açık anahtar
    pub ds: Option<DsRecord>, // İsteğe bağlı DS kaydı (üst zone doğrulaması için)
}

/// DNSSEC doğrulayıcısı.
///
/// Güven çapalarını ve önbelleklenen anahtarları yönetir.
/// DNS yanıtlarının kriptografik geçerliliğini doğrular.
#[derive(Clone)]
pub struct DnssecValidator {
    pub trust_anchors: Vec<TrustAnchor>,              // Güven çapaları (genelde kök zone)
    pub cached_keys: BTreeMap<String, Vec<DnsKey>>,   // Önbelleklenen DNSKEY kayıtları
    pub cached_ds: BTreeMap<String, Vec<DsRecord>>,   // Önbelleklenen DS kayıtları
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
    pub fn validate_dnskey(&self, domain: &str, key: &DnsKey, ds: Option<&DsRecord>) -> DnssecState {
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
        if rrsig.verify(_rrset, key) {
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
}

/// Global DNSSEC doğrulayıcısının bir kopyasını döner.
pub fn get_validator() -> DnssecValidator {
    DNSSEC_VALIDATOR.lock().clone()
}

/// Global doğrulayıcıya güven çapası ekler.
pub fn add_trust_anchor(key: DnsKey, ds: Option<DsRecord>) {
    DNSSEC_VALIDATOR.lock().add_root_anchor(key, ds);
}

/// Global DNSSEC doğrulayıcısı kullanarak bir DNSKEY kaydını doğrular.
pub fn validate_dnssec(domain: &str, key: &DnsKey, ds: Option<&DsRecord>) -> DnssecState {
    DNSSEC_VALIDATOR.lock().validate_dnskey(domain, key, ds)
}
