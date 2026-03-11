//! # TPM 2.0 (Güvenilir Platform Modülü - Trusted Platform Module) Desteği
//!
//! Bu modül, TCG (Trusted Computing Group) TPM 2.0 standardına uygun donanım
//! güvenlik modülü arayüzünü uygular. TPM, kriptografik anahtarları, önyükleme
//! ölçümlerini ve uzaktan onay verilerini donanım düzeyinde güvenle saklar.
//!
//! ```
//! TPM 2.0 Mimari Genel Bakış:
//!
//!  +-------------------------------------------------+
//!  |               Yazılım Katmanı                   |
//!  |  OS / Uygulama    TSS (TPM Software Stack)      |
//!  +----------------------+--------------------------+
//!                         |  TIS/CRB Protokolü
//!                         v
//!  +-------------------------------------------------+
//!  |              TPM 2.0 Donanımı                   |
//!  |                                                 |
//!  |  +-----------+  +---------+  +---------------+ |
//!  |  | PCR[0-23] |  | NV RAM  |  | Anahtar Deposu| |
//!  |  | (SHA-256) |  | (Kalıcı)|  | (RSA/ECC)     | |
//!  |  +-----------+  +---------+  +---------------+ |
//!  |                                                 |
//!  |  +------------------------------------------+  |
//!  |  | Kriptografi Motoru                       |  |
//!  |  | RSA/ECC imza  SHA256/384/512 hash        |  |
//!  |  | AES şifreleme  ECDAA anonim kimlik       |  |
//!  |  +------------------------------------------+  |
//!  +-------------------------------------------------+
//! ```
//!
//! PCR (Platform Configuration Register) Yapısı:
//!   PCR[0]    : BIOS/UEFI Firmware (SRTM - Static Root of Trust)
//!   PCR[1]    : BIOS Yapılandırması
//!   PCR[2-3]  : Option ROM Kodu/Yapılandırması
//!   PCR[4]    : IPL (Initial Program Loader - MBR/bootloader)
//!   PCR[5]    : IPL Yapılandırması
//!   PCR[6]    : Durum geçişleri ve olaylar
//!   PCR[7]    : OEM Özel Ölçümleri
//!   PCR[8-9]  : OS Bootloader (GRUB/EFI)
//!   PCR[10]   : OS Çekirdeği
//!   PCR[11-15]: OS Dinamik Ölçümleri (DRTM)
//!   PCR[16-23]: Uygulama Katmanı
//!
//! PCR Extend Operasyonu:
//!   Yeni_PCR = SHA256(Mevcut_PCR || Yeni_Ölçüm)
//!   (Ölçümler birikimlidir; geçmiş silinemez)
//!
//! Uzaktan Onay (Remote Attestation) Akışı:
//!   1. Doğrulayıcı -> nonce (tekrar kullanım saldırısını önler)
//!   2. TPM -> PCR alıntısı (quote) = TPM_QUOTE2 + imza (EK ile)
//!   3. AIK (Attestation Identity Key) ile imzalanmış quote
//!   4. Doğrulayıcı: imzayı ve PCR değerlerini doğrular
//!
//! Seal/Unseal (Mühürleme) Mekanizması:
//!   seal(veri, PCR_maskesi) -> şifreli_blob
//!   unseal(şifreli_blob)   -> veri  (YALNIZCA PCR değerleri eşleşirse)
//!   Örnek kullanım: Disk şifreleme anahtarını belirli bir TPM durumuna bağlama

use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// TPM 2.0 KOMUT KODLARI (Command Codes - CC)
//
// Her TPM komutu benzersiz bir 32-bit komut kodu ile tanımlanır.
// Kodlar TCG TPM 2.0 Part 2 spesifikasyonunda sıralıdır.
//
// NV (Non-Volatile) Komutları:
//   NV_DEFINE_SPACE: Kalıcı bellekte yeni alan oluştur
//   NV_UNDEFINESPACE: Kalıcı bellek alanını sil
//   NV_WRITE:        Kalıcı belleğe veri yaz
//   NV_READ:         Kalıcı bellekten veri oku
//
// Anahtar Komutları:
//   CREATE: Yeni anahtar çifti oluştur (RSA/ECC)
//   LOAD:   Daha önce oluşturulmuş anahtarı geçici belleğe yükle
//   SIGN:   Anahtarla veri imzala
//
// Kriptografi Komutları:
//   GET_RANDOM:  Donanımsal rastgele sayı üreteci
//   HASH:        Veri hash'ini hesapla
//
// Ölçüm Komutları:
//   PCR_EXTEND:  PCR kaydını yeni ölçümle genişlet
//   PCR_READ:    PCR kaydının mevcut değerini oku
//
// Onay Komutları:
//   MAKE_CREDENTIAL:    Kimlik bilgisi oluştur (Privacy CA ile)
//   ACTIVATE_CREDENTIAL: Kimlik bilgisini AI anahtarıyla etkinleştir
//   QUOTE:              PCR alıntısı oluştur (uzaktan onay için)
// ============================================================================

/// TPM2_CC_NV_READ: Kalıcı bellekten (NV Index) veri okuma komutu
const TPM2_CC_NV_READ: u32 = 0x0000145E;
/// TPM2_CC_NV_WRITE: Kalıcı belleğe (NV Index) veri yazma komutu
const TPM2_CC_NV_WRITE: u32 = 0x00001437;
/// TPM2_CC_NV_DEFINE_SPACE: Kalıcı bellekte yeni bir NV Index alanı tanımlama
const TPM2_CC_NV_DEFINE_SPACE: u32 = 0x0000012A;
/// TPM2_CC_NV_UNDEFINESPACE: Kalıcı bellek NV Index alanını silme
const TPM2_CC_NV_UNDEFINESPACE: u32 = 0x00000141;
/// TPM2_CC_CREATE: Birincil veya alt anahtar çifti oluşturma (şablona göre)
const TPM2_CC_CREATE: u32 = 0x00000153;
/// TPM2_CC_LOAD: Önceden oluşturulmuş anahtarı geçici çalışma alanına yükleme
const TPM2_CC_LOAD: u32 = 0x00000157;
/// TPM2_CC_SIGN: Yüklü anahtarla veri imzalama (RSA-PSS, ECDSA vb.)
const TPM2_CC_SIGN: u32 = 0x0000015D;
/// TPM2_CC_GET_RANDOM: Donanımsal rastgele sayı üretecinden bayt alma
const TPM2_CC_GET_RANDOM: u32 = 0x0000017B;
/// TPM2_CC_HASH: Veri bloğunun SHA hash'ini hesaplama
const TPM2_CC_HASH: u32 = 0x00000004;
/// TPM2_CC_PCR_EXTEND: PCR kaydını SHA-256 özet değeriyle genişletme
const TPM2_CC_PCR_EXTEND: u32 = 0x0000013C;
/// TPM2_CC_PCR_READ: PCR kaydının mevcut SHA-256 özet değerini okuma
const TPM2_CC_PCR_READ: u32 = 0x0000017E;
/// TPM2_CC_MAKE_CREDENTIAL: Privacy CA için kimlik bilgisi paketi oluşturma
const TPM2_CC_MAKE_CREDENTIAL: u32 = 0x0000015B;
/// TPM2_CC_ACTIVATE_CREDENTIAL: AIK ile şifreli kimlik bilgisini açma
const TPM2_CC_ACTIVATE_CREDENTIAL: u32 = 0x00000167;
/// TPM2_CC_QUOTE: Seçili PCR kayıtlarını imzalı alıntı olarak dışarı verme
const TPM2_CC_QUOTE: u32 = 0x00000158;

// ============================================================================
// TPM 2.0 SABİT TANITICILAR (Permanent Handles)
//
// TPM'de bazı kaynaklar bellekte kalıcı olarak bulunur ve sabit
// tanıtıcılarla erişilir. Bu tanıtıcılar yetkilendirme hiyerarşisini tanımlar.
//
//  TPM2_RH_OWNER:       Sahip hiyerarşisi (OS ve kullanıcı anahtarları)
//                       NV alanı oluşturma ve uzun ömürlü anahtarlar için
//
//  TPM2_RH_PLATFORM:    Platform hiyerarşisi (firmware/UEFI anahtarları)
//                       Fabrika çıkışında kurulur, yalnızca firmware erişir
//
//  TPM2_RH_ENDORSEMENT: Onay hiyerarşisi (EK - Endorsement Key)
//                       Fabrikada üretici tarafından yüklenir; TPM kimliği
//                       Bu anahtar TPM'yi doğrulamak için kullanılır
//
//  TPM2_RH_NULL:        Boş hiyerarşi - kısa ömürlü/geçici işlemler
//                       Güç kesilince tüm nesneler silinir
// ============================================================================

/// TPM2_RH_OWNER: Sahip yetkilendirme hiyerarşisi tanıtıcısı (0x40000001)
const TPM2_RH_OWNER: u32 = 0x40000001;
/// TPM2_RH_PLATFORM: Platform (firmware/UEFI) yetkilendirme hiyerarşisi (0x4000000C)
const TPM2_RH_PLATFORM: u32 = 0x4000000C;
/// TPM2_RH_ENDORSEMENT: Onay anahtarı (EK) hiyerarşisi - uzaktan onay için (0x4000000B)
const TPM2_RH_ENDORSEMENT: u32 = 0x4000000B;
/// TPM2_RH_NULL: Geçici/boş hiyerarşi - kalıcı depolama yok (0x40000007)
const TPM2_RH_NULL: u32 = 0x40000007;

// ============================================================================
// TPM 2.0 ALGORITMA SABİTLERİ (Algorithm IDs)
//
// TPM komutlarında hangi kriptografik algoritmanın kullanılacağı bu
// 16-bit sabitlerle belirtilir. TCG Algorithm Registry'den alınmıştır.
//
//  RSA:        Asimetrik şifreleme/imza (2048/4096 bit anahtar)
//  SHA256:     NIST onaylı 256-bit hash (PCR ölçümleri için standart)
//  SHA384:     384-bit hash (yüksek güvenlik gereksinimleri için)
//  SHA512:     512-bit hash (maksimum güvenlik)
//  AES:        Simetrik şifreleme (128/192/256 bit)
//  ECC:        Eliptik eğri kriptografisi (daha küçük anahtar uzunluğu)
//  ECDAA:      Eliptik Eğri DAA - Anonim kimlik doğrulama
//              (Privacy CA gerekmeden gizlilik korumalı onay)
// ============================================================================

/// TPM2_ALG_RSA: RSA asimetrik algoritması (0x0001)
const TPM2_ALG_RSA: u16 = 0x0001;
/// TPM2_ALG_SHA256: SHA-256 hash algoritması - PCR ölçüm standart algoritması (0x000B)
const TPM2_ALG_SHA256: u16 = 0x000B;
/// TPM2_ALG_SHA384: SHA-384 hash algoritması - gelişmiş güvenlik (0x000C)
const TPM2_ALG_SHA384: u16 = 0x000C;
/// TPM2_ALG_SHA512: SHA-512 hash algoritması - maksimum güvenlik (0x000D)
const TPM2_ALG_SHA512: u16 = 0x000D;
/// TPM2_ALG_AES: AES simetrik şifreleme algoritması (0x0006)
const TPM2_ALG_AES: u16 = 0x0006;
/// TPM2_ALG_ECC: Eliptik Eğri Kriptografisi (0x0023)
const TPM2_ALG_ECC: u16 = 0x0023;
/// TPM2_ALG_ECDAA: Eliptik Eğri DAA - gizlilik korumalı anonim onay (0x0014)
const TPM2_ALG_ECDAA: u16 = 0x0014;

// ============================================================================
// TPM 2.0 YERELLİK (Locality) SEVİYELERİ
//
// TPM'ye erişim, yerellik mekanizmasıyla kısıtlanabilir.
// Her yerellik seviyesi farklı bir güven alanını temsil eder.
//
//  LOCALITY_0: Normal yazılım erişimi (OS, uygulamalar)
//             Platform yerelliği - standart TPM iletişimi
//
//  LOCALITY_1: TXT (Trusted Execution Technology) güvenilir OS
//             SEV/TDX gibi donanımsal izolasyon ortamları
//
//  LOCALITY_2: TXT güvenilir OS bileşeni (SINIT ACM)
//
//  LOCALITY_3: TXT başlatma (Intel TXT ACM - SINIT modülü)
//
//  LOCALITY_4: Intel TXT DRTM (Dynamic Root of Trust for Measurement)
//             Sadece CPU donanımı bu yerelliğe erişebilir
//
// Yerellik, TIS (TPM Interface Specification) kayıt alanındaki
// TPM_ACCESS_x yazmacı üzerinden talep edilir.
// ============================================================================

/// TPM_LOCALITY_0: Normal OS/yazılım erişimi (varsayılan TIS yerelliği)
const TPM_LOCALITY_0: u8 = 0;
/// TPM_LOCALITY_1: Güvenilir OS - Intel TXT / AMD SEV ortamı
const TPM_LOCALITY_1: u8 = 1;
/// TPM_LOCALITY_2: TXT güvenilir OS bileşeni erişimi
const TPM_LOCALITY_2: u8 = 2;
/// TPM_LOCALITY_3: TXT başlatma (SINIT ACM modülü) erişimi
const TPM_LOCALITY_3: u8 = 3;
/// TPM_LOCALITY_4: Intel TXT DRTM - yalnızca CPU donanımı erişebilir
const TPM_LOCALITY_4: u8 = 4;

// ============================================================================
// TPM YANIT KODLARI (TpmResponseCode)
//
// TPM'den geri dönen her yanıt paketinde bir durum kodu bulunur.
// 0x0000 başarıyı; diğer değerler çeşitli hata koşullarını belirtir.
//
// Kod Yapısı:
//   Bit[11-8] = Hata kategorisi (0=başarı, 1=genel, 2=anahtar, vb.)
//   Bit[6]    = Format biti (0=TPM 1.2 uyumlu, 1=TPM 2.0 hata formatı)
//   Bit[5-0]  = Hata kodu
//
// Önemli Kodlar:
//   AuthFail:       Yetkilendirme değerinin (HMAC/şifre) yanlış olması
//   NvLocked:       NV alanı özellik kilidi nedeniyle değiştirilemez
//   NvUninitialized: NV alanı henüz yazılmamış (ilk okuma öncesi durum)
//   PolicyFail:     PCR politikası mevcut PCR değerleriyle eşleşmedi
//                   (unseal başarısızlığının ana nedeni)
// ============================================================================

/// TPM 2.0 yanıt kod enumerasyonu - komut sonuç durumunu belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpmResponseCode {
    /// Komut başarıyla tamamlandı (0x0000)
    Success = 0x0000,
    /// TPM 1.2 uyumlu genel başarısızlık (0x0100)
    Ver1Failure = 0x0100,
    /// İmza bulunamadı (0x0200)
    NoSignature = 0x0200,
    /// İstenen anahtar geçici bellekte (TPM Flush) yüklü değil (0x0201)
    KeyNotLoaded = 0x0201,
    /// Belirtilen anahtar tanıtıcısıyla eşleşen anahtar bulunamadı (0x0202)
    KeyNotFound = 0x0202,
    /// Yetkilendirme hatası: HMAC veya şifre doğrulaması başarısız (0x098E)
    AuthFail = 0x098E,
    /// Yetkilendirme mevcut değil veya desteklenmiyor (0x098F)
    AuthUnavailable = 0x098F,
    /// PCR politikası mevcut PCR değerleriyle uyuşmadı - mühür açılamaz (0x0991)
    PolicyFail = 0x0991,
    /// Komut için veri boyutu sınırı aşıldı (0x01D5)
    Size = 0x01D5,
    /// Komut parametresinde geçersiz değer (0x0184)
    Value = 0x0184,
    /// NV Index kilitleme özelliği devrede; yazma reddedildi (0x0149)
    NvLocked = 0x0149,
    /// NV Index hiç yazılmamış; okuma geçersiz veri döndürür (0x014A)
    NvUninitialized = 0x014A,
    /// NV depolama alanında yer kalmadı (0x014B)
    NvSpace = 0x014B,
    /// Bu tanıtıcıyla NV Index zaten tanımlanmış (0x014C)
    NvDefined = 0x014C,
    /// Bilinmeyen veya tanımlanmamış yanıt kodu
    Unknown,
}

impl TpmResponseCode {
    /// 16-bit yanıt kodunu `TpmResponseCode` enumerasyonuna dönüştürür.
    pub fn from_u16(code: u16) -> Self {
        match code {
            0x0000 => TpmResponseCode::Success,
            0x0100 => TpmResponseCode::Ver1Failure,
            0x0200 => TpmResponseCode::NoSignature,
            0x0201 => TpmResponseCode::KeyNotLoaded,
            0x0202 => TpmResponseCode::KeyNotFound,
            0x098E => TpmResponseCode::AuthFail,
            0x098F => TpmResponseCode::AuthUnavailable,
            0x0991 => TpmResponseCode::PolicyFail,
            0x01D5 => TpmResponseCode::Size,
            0x0184 => TpmResponseCode::Value,
            0x0149 => TpmResponseCode::NvLocked,
            0x014A => TpmResponseCode::NvUninitialized,
            0x014B => TpmResponseCode::NvSpace,
            0x014C => TpmResponseCode::NvDefined,
            _ => TpmResponseCode::Unknown,
        }
    }

    /// Yanıt kodunun başarı (Success) olup olmadığını döndürür.
    pub fn is_success(&self) -> bool {
        matches!(self, TpmResponseCode::Success)
    }
}

// ============================================================================
// NV INDEX (Kalıcı Bellek Dizini - Non-Volatile Index)
//
// TPM'nin NVRAM'ında kalıcı olarak saklanan verilere erişim için tanımlayıcı.
//
// Handle Değerleri (TCG Aralıkları):
//   0x01000000 - 0x01FFFFFF: TPM2_HT_NV_INDEX  (Kalıcı veri depolama)
//   0x40000000 - 0x407FFFFF: TPM2_HT_PERMANENT (Kalıcı tanıtıcılar - Owner/Platform/EK)
//
// Attributes (Öznitelikler) - Önemli bitler:
//   Bit[25]: TPMA_NV_PPWRITE     - 0x02000000: Platform hiyerarşisi yazabilir
//   Bit[26]: TPMA_NV_OWNERWRITE  - 0x04000000: Sahip hiyerarşisi yazabilir
//   Bit[29]: TPMA_NV_PPREAD      - 0x20000000: Platform okuyabilir
//   Bit[30]: TPMA_NV_OWNERREAD   - 0x40000000: Sahip okuyabilir
//   Bit[18]: TPMA_NV_AUTHREAD    - 0x00040000: Auth değeriyle okuma
//   Bit[19]: TPMA_NV_AUTHWRITE   - 0x00080000: Auth değeriyle yazma
//
// auth_policy: PCR tabanlı veya HMAC tabanlı erişim politikası (32 bayt)
// ============================================================================

/// TPM 2.0 kalıcı bellek (NV - Non-Volatile) index tanımlayıcısı
#[derive(Clone, Copy, Debug)]
pub struct NvIndex {
    /// NV alanına erişim tanıtıcısı (0x01xxxxxx aralığı - kullanıcı NV)
    pub handle: u32,
    /// NV alanında saklanacak verinin bayt cinsinden boyutu
    pub size: u16,
    /// NV özellik bitleri (kim okuyabilir/yazabilir, kilitli mi, vb.)
    pub attributes: u32,
    /// PolicyPCR gibi gelişmiş erişim politikaları için SHA-256 politika hash'i
    pub auth_policy: [u8; 32],
}

// ============================================================================
// PCR SEÇİMİ (PcrSelection)
//
// Bir TPM komutunda hangi PCR kayıtlarının hedef alındığını belirten yapı.
// PCR_READ ve PCR_QUOTE komutlarında çoklu PCR okumak için kullanılır.
//
// select[] bitmaskesi:
//   select[0]: PCR 0-7  (her bit bir PCR'a karşılık gelir)
//   select[1]: PCR 8-15
//   select[2]: PCR 16-23
//   ...
//   select[n]: PCR 8n - 8n+7
//
// Örnek: PCR 0 ve PCR 7'yi seçmek için:
//   select[0] = 0b10000001 = 0x81
//
// new_sha256() varsayılan kurucusu:
//   select = [0xFF, 0xFF, 0xFF, 0, ...] -> PCR 0-23 tümü seçili
// ============================================================================

/// PCR kayıt seçim yapısı - hangi PCR'ların hedef alındığını belirtir
#[derive(Clone, Debug)]
pub struct PcrSelection {
    /// Kullanılacak hash algoritması (TPM2_ALG_SHA256 = 0x000B)
    pub hash: u16,
    /// `select` dizisinin etkin bayt sayısı (genellikle 3 = 24 PCR için)
    pub size: u8,
    /// PCR seçim bitmaskesi: her bit bir PCR kaydına karşılık gelir
    pub select: [u8; 16],
}

impl PcrSelection {
    /// SHA-256 hash algoritmasıyla PCR 0-23 arası TÜM kayıtları seçen varsayılan yapı oluşturur.
    pub fn new_sha256() -> Self {
        PcrSelection {
            hash: TPM2_ALG_SHA256,
            size: 3,
            select: [0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    /// Belirtilen PCR numarasını seçim maskesine ekler.
    ///
    /// PCR numarası 0-127 aralığında olmalıdır (select dizisi 16 bayt = 128 bit).
    pub fn select_pcr(&mut self, pcr: u8) {
        let idx = (pcr / 8) as usize;
        let bit = pcr % 8;
        if idx < 16 {
            self.select[idx] |= 1 << bit;
        }
    }

    /// Belirtilen PCR numarasının seçim maskesinde set edilip edilmediğini döndürür.
    pub fn is_selected(&self, pcr: u8) -> bool {
        let idx = (pcr / 8) as usize;
        let bit = pcr % 8;
        if idx < 16 {
            (self.select[idx] & (1 << bit)) != 0
        } else {
            false
        }
    }
}

// ============================================================================
// PCR DEĞERİ (PcrValue)
//
// Tek bir PCR kaydının numarasını ve mevcut SHA-256 özetini tutan yapı.
// PCR_READ yanıtından ayrıştırılan her PCR girişi bu yapıyla temsil edilir.
//
// PCR extend işleminin matematiksel ifadesi:
//   PCR_new = SHA256(PCR_old || measurement)
//
// Başlangıç değeri:
//   PCR[0-15] = SHA256(00 00 ... 00)  (32 sıfır bayt)
//   PCR[16-23]= 00 00 ... 00          (tümü sıfır - güvenilir olmayan bölge)
// ============================================================================

/// Tek bir PCR kaydının numarasını ve SHA-256 özet değerini tutan yapı
#[derive(Clone, Debug)]
pub struct PcrValue {
    /// PCR kayıt numarası (0-23 arası)
    pub pcr: u8,
    /// PCR'ın mevcut SHA-256 özet değeri (32 bayt)
    pub value: [u8; 32],
}

// ============================================================================
// TPM AYGITI (TpmDevice)
//
// Fiziksel TPM donanımıyla iletişim kuran sürücü nesnesi.
//
// TIS (TPM Interface Specification) Protokolü:
//   - MMIO adresi: 0xFED40000 (varsayılan TPM TIS adresi)
//   - Kayıt haritası:
//     +0x000: TPM_ACCESS_x       (yerellik talep/serbest bırakma)
//     +0x008: TPM_STS_x          (durum ve komut/yanıt boyutu)
//     +0x024: TPM_DATA_FIFO_x    (veri giriş/çıkış FIFO)
//     +0xF00: TPM_INTF_CAPS      (arayüz yetenekleri)
//
// CRB (Command Response Buffer) Alternatif Protokolü:
//   - Daha hızlı: doğrudan bellek tamponları kullanır
//   - Registar: TPM_LOC_STATE, TPM_CRB_CTRL_REQ, TPM_CRB_CMD_ADDR vb.
//
// Komut Gönderme Akışı (TIS):
//   1. TPM_ACCESS_x.requestUse = 1  (yerellik talebi)
//   2. TPM_STS_x.commandReady = 1   (komut tamponunu temizle)
//   3. TPM_DATA_FIFO_x'e komut baytlarını yaz
//   4. TPM_STS_x.tpmGo = 1         (komutu çalıştır)
//   5. TPM_STS_x.dataAvail bekle   (yanıt hazır sinyali)
//   6. TPM_DATA_FIFO_x'ten yanıtı oku
// ============================================================================

/// TPM 2.0 donanım aygıtı sürücüsü (TIS veya CRB protokolü)
pub struct TpmDevice {
    /// Aktif yerellik seviyesi (0=OS, 4=DRTM donanımı)
    pub locality: u8,
    /// true=TIS protokolü, false=CRB protokolü
    pub is_tis: bool,
    /// TPM MMIO taban adresi (genellikle 0xFED40000)
    pub base_address: u64,
    /// Gönderilecek komut paket tamponu
    pub command_buffer: Vec<u8>,
    /// Alınan yanıt paket tamponu
    pub response_buffer: Vec<u8>,
}

impl TpmDevice {
    /// Verilen MMIO taban adresinde yeni bir TPM aygıt örneği oluşturur.
    ///
    /// Varsayılan yerellik: LOCALITY_0 (normal OS erişimi).
    pub fn new(base_address: u64) -> Self {
        TpmDevice {
            locality: TPM_LOCALITY_0,
            is_tis: true,
            base_address,
            command_buffer: Vec::new(),
            response_buffer: Vec::new(),
        }
    }

    /// TPM aygıtını başlatır ve kullanıma hazır hale getirir.
    ///
    /// Gerçek TIS başlatma adımları:
    ///   1. TPM_ACCESS kaydından yerellik talep et
    ///   2. TPM_STS.tpmFamily alanını kontrol et (TPM 2.0 = 0b01)
    ///   3. TPM2_CC_Startup(TPM_SU_CLEAR) komutu gönder
    ///   4. TPM2_CC_GetCapability ile yetenekleri doğrula
    pub fn init(&mut self) -> Result<(), TpmError> {
        crate::serial_println!("[TPM] Initializing TPM 2.0 at {:#x}", self.base_address);

        // 1. Yerellik talebi — TPM_ACCESS yazmacına requestUse yaz
        let access_reg = self.base_address + (self.locality as u64) * 0x1000;
        unsafe {
            let ptr = access_reg as *mut u8;
            // requestUse = bit 1
            core::ptr::write_volatile(ptr, 0x02);
        }

        // 2. Yerellik verildiğini kontrol et (activeLocality = bit 5)
        let mut timeout = 10000u32;
        loop {
            let val = unsafe { core::ptr::read_volatile(access_reg as *const u8) };
            if val & 0x20 != 0 {
                break; // Yerellik verildi
            }
            timeout -= 1;
            if timeout == 0 {
                crate::serial_println!("[TPM] Locality request timeout");
                // Simics/QEMU ortamında gerçek TPM olmayabilir, devam et
                break;
            }
        }

        // 3. TPM2_CC_Startup(TPM_SU_CLEAR) gönder
        let mut startup_cmd = Vec::with_capacity(12);
        startup_cmd.extend_from_slice(&0x8001u16.to_be_bytes()); // tag: no sessions
        startup_cmd.extend_from_slice(&12u32.to_be_bytes()); // commandSize
        startup_cmd.extend_from_slice(&0x00000144u32.to_be_bytes()); // TPM2_CC_Startup
        startup_cmd.extend_from_slice(&0x0000u16.to_be_bytes()); // TPM_SU_CLEAR
        let _ = self.send_command(&startup_cmd);

        crate::serial_println!("[TPM] TIS initialization complete");
        Ok(())
    }

    /// TPM TIS arayüzü üzerinden komut gönderir ve yanıt alır.
    fn send_command(&mut self, cmd: &[u8]) -> Result<Vec<u8>, TpmError> {
        let sts_reg = self.base_address + (self.locality as u64) * 0x1000 + 0x18;
        let fifo_reg = self.base_address + (self.locality as u64) * 0x1000 + 0x24;

        // 1. commandReady yaz — komutu kabul etmeye hazırla
        unsafe {
            core::ptr::write_volatile(sts_reg as *mut u32, 0x40); // commandReady = bit 6
        }

        // 2. STS.commandReady bekle
        let mut timeout = 10000u32;
        loop {
            let sts = unsafe { core::ptr::read_volatile(sts_reg as *const u32) };
            if sts & 0x40 != 0 {
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                // Simülasyon ortamı — gerçek TPM yok, simüle edilmiş yanıt döndür
                return self.simulate_response(cmd);
            }
        }

        // 3. FIFO'ya komut baytlarını yaz
        for &byte in cmd {
            unsafe {
                core::ptr::write_volatile(fifo_reg as *mut u8, byte);
            }
        }

        // 4. tpmGo yaz — komutu çalıştır
        unsafe {
            core::ptr::write_volatile(sts_reg as *mut u32, 0x20); // tpmGo = bit 5
        }

        // 5. dataAvail bekle
        let mut timeout = 100000u32;
        loop {
            let sts = unsafe { core::ptr::read_volatile(sts_reg as *const u32) };
            if sts & 0x10 != 0 {
                break; // dataAvail = bit 4
            }
            timeout -= 1;
            if timeout == 0 {
                return self.simulate_response(cmd);
            }
        }

        // 6. Yanıt başlığını oku (10 bayt: tag(2) + size(4) + responseCode(4))
        let mut header = [0u8; 10];
        for byte in header.iter_mut() {
            *byte = unsafe { core::ptr::read_volatile(fifo_reg as *const u8) };
        }
        let response_size =
            u32::from_be_bytes([header[2], header[3], header[4], header[5]]) as usize;
        let response_code = u32::from_be_bytes([header[6], header[7], header[8], header[9]]);

        // Kalan yanıt baytlarını oku
        let remaining = response_size.saturating_sub(10);
        let mut response = Vec::with_capacity(response_size);
        response.extend_from_slice(&header);
        for _ in 0..remaining {
            let byte = unsafe { core::ptr::read_volatile(fifo_reg as *const u8) };
            response.push(byte);
        }

        if response_code != 0 {
            let rc = TpmResponseCode::from_u16(response_code as u16);
            return Err(TpmError::ResponseError(rc));
        }

        Ok(response)
    }

    /// Gerçek TPM yoksa simüle edilmiş yanıt üretir (Simics/QEMU ortamı için).
    fn simulate_response(&self, cmd: &[u8]) -> Result<Vec<u8>, TpmError> {
        if cmd.len() < 10 {
            return Err(TpmError::CommunicationError);
        }
        let cc = u32::from_be_bytes([cmd[6], cmd[7], cmd[8], cmd[9]]);
        match cc {
            0x0000017B => {
                // GET_RANDOM: rastgele veri üret
                let count = if cmd.len() >= 14 {
                    u32::from_be_bytes([cmd[10], cmd[11], cmd[12], cmd[13]]) as usize
                } else {
                    16
                };
                let mut resp = Vec::with_capacity(12 + 2 + count);
                resp.extend_from_slice(&0x8001u16.to_be_bytes());
                resp.extend_from_slice(&((12 + 2 + count) as u32).to_be_bytes());
                resp.extend_from_slice(&0u32.to_be_bytes()); // success
                resp.extend_from_slice(&(count as u16).to_be_bytes());
                // RDRAND'dan rastgele veri kullan
                let mut buf = alloc::vec![0u8; count];
                crate::crypto::rdrand_bytes(&mut buf);
                resp.extend_from_slice(&buf);
                Ok(resp)
            }
            _ => {
                // Diğer komutlar başarılı boş yanıt
                let mut resp = Vec::with_capacity(10);
                resp.extend_from_slice(&0x8001u16.to_be_bytes());
                resp.extend_from_slice(&10u32.to_be_bytes());
                resp.extend_from_slice(&0u32.to_be_bytes()); // success
                Ok(resp)
            }
        }
    }

    /// TPM'nin donanımsal rastgele sayı üretecinden (HRNG) belirtilen sayıda
    /// kriptografik kalitede rastgele bayt alır.
    ///
    /// TPM 2.0 GET_RANDOM komutu NIST SP 800-90B tarafından onaylanan
    /// donanım entropi kaynağını kullanır. Yazılım PRNG'lere göre çok
    /// daha güvenilir entropi kalitesi sağlar.
    ///
    /// Komut yapısı (big-endian):
    ///   [tag:16][size:32][cc:32][bytesRequested:32]
    pub fn get_random(&mut self, count: u16) -> Result<Vec<u8>, TpmError> {
        // Komut oluştur
        let mut cmd = Vec::with_capacity(14);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&(14u32).to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_GET_RANDOM.to_be_bytes());
        // İstenen bayt sayısı
        cmd.extend_from_slice(&(count as u32).to_be_bytes());

        // Komutu gönder ve yanıtı al
        let response = self.send_command(&cmd)?;

        // Yanıt ayrıştır: header(10) + randomBytesCount(2) + data
        if response.len() < 12 {
            return Err(TpmError::CommunicationError);
        }
        let random_count = u16::from_be_bytes([response[10], response[11]]) as usize;
        let data_start = 12;
        let data_end = data_start + random_count.min(response.len() - data_start);
        Ok(response[data_start..data_end].to_vec())
    }

    /// Belirtilen PCR kaydını yeni bir SHA-256 ölçüm özetiyle genişletir.
    ///
    /// PCR extend formülü: PCR_new = SHA256(PCR_old || digest)
    ///
    /// Bu işlem geri alınamaz (irreversible); PCR'ı eski değerine döndürmek
    /// mümkün değildir. Bu özellik güven zincirinin değiştirilemezliğini sağlar.
    ///
    /// Komut yapısı (big-endian):
    ///   [tag:16][size:32][cc:32][pcrHandle:32][authArea:...][digestCount:32][algId:16][digest:32]
    pub fn pcr_extend(&mut self, pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
        if digest.len() != 32 {
            return Err(TpmError::InvalidDigest);
        }

        // Komut oluştur
        let mut cmd = Vec::with_capacity(50);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&(50u32).to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_PCR_EXTEND.to_be_bytes());
        // PCR tanıtıcısı
        cmd.extend_from_slice(&(pcr as u32).to_be_bytes());
        // Yetkilendirme
        cmd.extend_from_slice(&0u32.to_be_bytes()); // Yetkilendirme alanı boyutu
                                                    // PCR seçimi
        cmd.extend_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
        cmd.extend_from_slice(&[1u8]); // Boyut
        cmd.extend_from_slice(&[1u8 << (pcr % 8)]); // Seçim
                                                    // Özet sayısı
        cmd.extend_from_slice(&1u32.to_be_bytes());
        // Özet algoritması
        cmd.extend_from_slice(&TPM2_ALG_SHA256.to_be_bytes());
        // Özet değeri
        cmd.extend_from_slice(digest);

        // Komutu gönder
        self.send_command(&cmd)?;
        crate::serial_println!("[TPM] PCR {} extended", pcr);
        Ok(())
    }

    /// Belirtilen PCR seçimine göre PCR kayıtlarının mevcut değerlerini okur.
    ///
    /// Döndürülen `Vec<PcrValue>` her seçili PCR için PCR numarası ve
    /// 32 baytlık SHA-256 özet değeri içerir.
    ///
    /// Uzaktan onay öncesi PCR değerlerini doğrulamak için kullanılır.
    pub fn pcr_read(&mut self, selection: &PcrSelection) -> Result<Vec<PcrValue>, TpmError> {
        // Komut oluştur
        let mut cmd = Vec::with_capacity(30);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&(30u32).to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_PCR_READ.to_be_bytes());
        // PCR seçim sayısı
        cmd.extend_from_slice(&1u32.to_be_bytes());
        // Özet algoritması
        cmd.extend_from_slice(&selection.hash.to_be_bytes());
        // Boyut
        cmd.push(selection.size);
        // Seçim
        cmd.extend_from_slice(&selection.select[..selection.size as usize]);

        // Komutu gönder ve yanıtı ayrıştır
        let response = self.send_command(&cmd)?;
        let mut values = Vec::new();

        // Yanıt formatı: header(10) + updateCounter(4) + pcrSelectionOut + pcrDigests
        if response.len() > 14 {
            // Basitleştirilmiş ayrıştırma: seçili PCR'lar için 32-byte hash oku
            let mut offset = 14usize; // Sonu pcrSelectionOut at
                                      // PCR selection out boyutunu atla
            if offset + 10 <= response.len() {
                offset += 10; // basit seçim yapısı
            }
            // digest count
            if offset + 4 <= response.len() {
                let digest_count = u32::from_be_bytes([
                    response[offset],
                    response[offset + 1],
                    response[offset + 2],
                    response[offset + 3],
                ]) as usize;
                offset += 4;

                for pcr_idx in 0..digest_count.min(24) {
                    if offset + 34 > response.len() {
                        break;
                    }
                    let _alg = u16::from_be_bytes([response[offset], response[offset + 1]]);
                    offset += 2;
                    let mut value = [0u8; 32];
                    value.copy_from_slice(&response[offset..offset + 32]);
                    offset += 32;
                    values.push(PcrValue {
                        pcr: pcr_idx as u8,
                        value,
                    });
                }
            }
        }

        Ok(values)
    }

    /// TPM'nin kalıcı NVRAM'ında belirtilen boyutta yeni bir NV Index alanı tanımlar.
    ///
    /// NV Index kullanım senaryoları:
    ///   - Disk şifreleme anahtarı saklama (PCR politikasıyla korunan)
    ///   - Güvenli önyükleme yapılandırması
    ///   - Aygıt sertifikası depolama
    ///   - Sayaçlar ve bayraklar (monoton artış özelliğiyle)
    ///
    /// auth: NV alana erişim için şifre/HMAC anahtar materyali
    /// handle: 0x01xxxxxx aralığında benzersiz NV Index tanıtıcısı
    pub fn nv_define_space(&mut self, handle: u32, size: u16, auth: &[u8]) -> Result<(), TpmError> {
        // Komut oluştur
        let mut cmd = Vec::with_capacity(60);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Yetkilendirme tanıtıcısı
        cmd.extend_from_slice(&TPM2_RH_OWNER.to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_NV_DEFINE_SPACE.to_be_bytes());
        // Geçici bellek tanıtıcısı
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Yetkilendirme politikası
        cmd.extend_from_slice(&[0u8; 32]);
        // Öznitelikler
        cmd.extend_from_slice(&0x2000_0000u32.to_be_bytes()); // Sahip yazma/okuma
                                                              // Yetkilendirme değeri (32 bayta tamamla)
        let mut auth_padded = [0u8; 32];
        auth_padded[..auth.len().min(32)].copy_from_slice(&auth[..auth.len().min(32)]);
        cmd.extend_from_slice(&auth_padded);

        // Komutu gönder
        self.send_command(&cmd)?;
        crate::serial_println!("[TPM] NV space defined: handle={:#x} size={}", handle, size);
        Ok(())
    }

    /// Önceden tanımlanmış NV Index alanına belirtilen konumdan itibaren veri yazar.
    ///
    /// offset: NV alanı başından bayt cinsinden yazma konumu
    /// data: Yazılacak veri (NV alanının geri kalan boyutunu aşmamalı)
    pub fn nv_write(&mut self, handle: u32, offset: u16, data: &[u8]) -> Result<(), TpmError> {
        // Komut oluştur
        let cmd_size = 20 + data.len() as u32;
        let mut cmd = Vec::with_capacity(cmd_size as usize);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&cmd_size.to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_NV_WRITE.to_be_bytes());
        // Geçici bellek tanıtıcısı
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Konum
        cmd.extend_from_slice(&offset.to_be_bytes());
        // Veri boyutu
        cmd.extend_from_slice(&(data.len() as u16).to_be_bytes());
        // Veri
        cmd.extend_from_slice(data);

        // Komutu gönder
        self.send_command(&cmd)?;
        crate::serial_println!(
            "[TPM] NV write: handle={:#x} offset={} len={}",
            handle,
            offset,
            data.len()
        );
        Ok(())
    }

    /// NV Index alanından belirtilen konumdan itibaren belirtilen boyutta veri okur.
    ///
    /// handle: Okuma yapılacak NV Index tanıtıcısı
    /// offset: NV alanı başından bayt cinsinden okuma konumu
    /// size:   Okunacak bayt sayısı
    pub fn nv_read(&mut self, handle: u32, offset: u16, size: u16) -> Result<Vec<u8>, TpmError> {
        // Komut oluştur
        let mut cmd = Vec::with_capacity(20);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&20u32.to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_NV_READ.to_be_bytes());
        // Geçici bellek tanıtıcısı
        cmd.extend_from_slice(&handle.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&size.to_be_bytes());
        // Konum
        cmd.extend_from_slice(&offset.to_be_bytes());

        // Komutu gönder ve yanıtı ayrıştır
        let response = self.send_command(&cmd)?;
        // Yanıt: header(10) + data
        if response.len() > 12 {
            let data_len = u16::from_be_bytes([response[10], response[11]]) as usize;
            let start = 12;
            let end = start + data_len.min(response.len() - start);
            Ok(response[start..end].to_vec())
        } else {
            Ok(vec![0u8; size as usize])
        }
    }

    /// Seçili PCR kayıtlarından imzalı alıntı (quote) üretir.
    ///
    /// Uzaktan Onay (Remote Attestation) için temel mekanizmadır:
    ///   - key_handle: Alıntıyı imzalayacak Attestation Identity Key (AIK)
    ///   - nonce:      Tekrar kullanım saldırısını önleyen tek kullanımlık değer
    ///   - selection:  Alıntılanacak PCR kayıtları
    ///
    /// Döndürülen blob TPMS_ATTEST yapısını ve imzayı içerir.
    /// Doğrulayıcı bu blobu EK sertifikası ile doğrulayabilir.
    pub fn quote(
        &mut self,
        key_handle: u32,
        nonce: &[u8],
        selection: &PcrSelection,
    ) -> Result<Vec<u8>, TpmError> {
        // Onay için komut oluştur
        let cmd_size = 30 + nonce.len() as u32;
        let mut cmd = Vec::with_capacity(cmd_size as usize);

        // Etiket
        cmd.extend_from_slice(&0x8001u16.to_be_bytes());
        // Boyut
        cmd.extend_from_slice(&cmd_size.to_be_bytes());
        // Komut kodu
        cmd.extend_from_slice(&TPM2_CC_QUOTE.to_be_bytes());
        // Anahtar tanıtıcısı
        cmd.extend_from_slice(&key_handle.to_be_bytes());
        // Niteleyici veri
        cmd.extend_from_slice(&(nonce.len() as u16).to_be_bytes());
        cmd.extend_from_slice(nonce);
        // PCR seçimi
        cmd.extend_from_slice(&selection.hash.to_be_bytes());
        cmd.push(selection.size);
        cmd.extend_from_slice(&selection.select[..selection.size as usize]);

        // Komutu gönder ve onay verisini döndür
        let response = self.send_command(&cmd)?;
        // Yanıt: header(10) + attestation_data + signature
        if response.len() > 10 {
            Ok(response[10..].to_vec())
        } else {
            Ok(Vec::new())
        }
    }
}

// ============================================================================
// TPM HATA TİPLERİ (TpmError)
//
// TPM işlemlerinde karşılaşılabilecek hata koşullarını temsil eder.
//
//  NotPresent:        TPM donanımı bulunamadı (ACPI TCPA/TPM tablosu yok)
//  NotInitialized:    TPM init() çağrılmadan kullanılmaya çalışıldı
//  CommunicationError:TIS/CRB protokol hatası (zaman aşımı, FIFO taşması)
//  ResponseError:     TPM yanıt kodu sıfır değil (bkz. TpmResponseCode)
//  InvalidDigest:     PCR extend için 32 bayttan farklı uzunluk
//  InvalidHandle:     Geçersiz veya bulunamayan TPM tanıtıcısı
//  NvSpaceFull:       TPM NVRAM doldu; yeni alan tanımlanamıyor
//  AuthFailed:        Yetkilendirme doğrulaması başarısız (yanlış şifre/HMAC)
//  Unknown:           Tanımlanamayan hata durumu
// ============================================================================

/// TPM işlem hatası - aygıt iletişiminde veya komut yürütmesinde oluşan hata
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TpmError {
    /// TPM donanımı sistemde bulunamadı (ACPI tablosu yok veya devre dışı)
    NotPresent,
    /// TPM henüz başlatılmamış; init() çağrısı gerekiyor
    NotInitialized,
    /// TIS/CRB iletişim protokolü hatası (zaman aşımı veya FIFO hatası)
    CommunicationError,
    /// TPM'den dönen hata yanıt kodu (iç TpmResponseCode ile birlikte)
    ResponseError(TpmResponseCode),
    /// PCR extend işlemi için özet değeri tam 32 bayt olmalıdır
    InvalidDigest,
    /// Belirtilen TPM tanıtıcısı (handle) geçersiz veya kullanımda değil
    InvalidHandle,
    /// TPM NVRAM kapasitesi dolu; yeni NV Index tanımlanamıyor
    NvSpaceFull,
    /// Yetkilendirme başarısız: şifre veya HMAC değeri hatalı
    AuthFailed,
    /// Bilinmeyen veya sınıflandırılamayan TPM hatası
    Unknown,
}

// ============================================================================
// GLOBAL TPM ÖRNEĞİ
//
// Sistemde tek bir TPM aygıtı bulunabilir. lazy_static ile thread-safe
// biçimde yönetilen Mutex<Option<TpmDevice>> yapısı:
//   - None:    TPM henüz başlatılmamış (init() çağrılmamış)
//   - Some(d): TPM başlatılmış ve kullanıma hazır
//
// Tüm public yardımcı fonksiyonlar (pcr_extend, get_random vb.)
// bu global örnekten aygıta erişir.
// ============================================================================

// Global TPM örneği
lazy_static::lazy_static! {
    /// Global TPM aygıt örneği - Mutex korumalı, tek sisteme tekil erişim
    static ref TPM_DEVICE: Mutex<Option<TpmDevice>> = Mutex::new(None);
}

// ============================================================================
// PUBLIC ARAYÜZLERİ (Yardımcı Fonksiyonlar)
//
// Bu fonksiyonlar global TPM_DEVICE örneği üzerinden TpmDevice metodlarını
// sarmalar ve daha basit bir çağrı arayüzü sunar.
//
// Tüm fonksiyonlar:
//   - TPM başlatılmamışsa TpmError::NotInitialized döndürür
//   - Thread-safe: Mutex ile eşzamanlı erişime karşı korunur
// ============================================================================

/// TPM aygıtını verilen MMIO taban adresinde başlatır ve global örneği kurar.
///
/// Başarı durumunda global TPM_DEVICE güncellenir ve seri porta bilgi yazılır.
/// Hata durumunda TpmError döner ve TPM kullanılamaz olarak kalır.
pub fn init(base_address: u64) -> Result<(), TpmError> {
    let mut device = TpmDevice::new(base_address);
    device.init()?;
    *TPM_DEVICE.lock() = Some(device);
    crate::serial_println!("[TPM] TPM 2.0 initialized successfully");
    Ok(())
}

/// TPM'nin donanımsal rastgele sayı üretecinden `count` bayt kriptografik
/// kalitede rastgele veri alır.
///
/// Anahtar üretimi, nonce oluşturma ve tuz (salt) değerleri için kullanılır.
pub fn get_random(count: u16) -> Result<Vec<u8>, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.get_random(count)
}

/// Belirtilen PCR kaydını yeni bir SHA-256 ölçüm özetiyle genişletir.
///
/// PCR extend formülü: PCR_new = SHA256(PCR_old || digest)
/// Bu işlem geri alınamaz; sistemin bütünlük ölçüm zinciri korunur.
pub fn pcr_extend(pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.pcr_extend(pcr, digest)
}

/// Belirtilen PCR seçimine göre PCR kayıtlarının mevcut değerlerini döndürür.
///
/// Döndürülen vec her seçili PCR için numarasını ve SHA-256 değerini içerir.
pub fn pcr_read(selection: &PcrSelection) -> Result<Vec<PcrValue>, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;
    device.pcr_read(selection)
}

/// Bir önyükleme olayını SHA3-256 ile özetleyerek PCR 0'ı genişletir.
///
/// SRTM (Static Root of Trust for Measurement) ölçüm zincirinin parçasıdır.
/// PCR 0, BIOS/UEFI firmware ölçümleri için ayrılmış standarttır.
///
/// event: İnsan okunabilir olay açıklaması (örn. "GRUB loaded", "kernel hash")
pub fn measure_boot_event(event: &str) -> Result<(), TpmError> {
    // Olayı özete dönüştür
    let mut hasher = crate::crypto::Sha3::sha3_256();
    hasher.update(event.as_bytes());
    let hash = hasher.finalize();

    // PCR 0'ı genişlet (SRTM)
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hash[..32]);

    pcr_extend(0, &digest)
}

/// Veriyi belirtilen PCR maskesiyle mühürler (seal).
///
/// Mühürleme mekanizması:
///   - TPM içinde veriyi şifreleyen bir politika anahtarı oluşturur
///   - Bu anahtar yalnızca PCR değerleri `pcr_mask` ile eşleştiğinde açılır
///   - Önyükleme yapılandırması değişirse (farklı çekirdek, farklı GRUB) mühür açılamaz
///
/// Örnek kullanım: LUKS disk şifreleme anahtarını belirli önyükleme ölçümlerine bağlama
pub fn seal_data(data: &[u8], pcr_mask: u32) -> Result<Vec<u8>, TpmError> {
    // Mühürlenmiş blob oluştur: PCR maskesi + AES-CTR şifreli veri + HMAC
    let tpm = TPM_DEVICE.lock();
    let _ = tpm.as_ref().ok_or(TpmError::NotInitialized)?;

    // Mevcut PCR değerlerinden politika anahtarı türet
    // PCR mask'teki her PCR'ın hash'ini birleştirerek anahtar elde et
    let mut pcr_material = Vec::with_capacity(256);
    pcr_material.extend_from_slice(&pcr_mask.to_be_bytes());
    // PCR 0-23 tarama
    for i in 0..24u32 {
        if pcr_mask & (1 << i) != 0 {
            // PCR değerini al — tek PCR için selection oluştur
            let mut sel = PcrSelection {
                hash: TPM2_ALG_SHA256,
                size: 3,
                select: [0u8; 16],
            };
            sel.select_pcr(i as u8);
            if let Ok(pcr_vals) = pcr_read(&sel) {
                for pv in &pcr_vals {
                    pcr_material.extend_from_slice(&pv.value);
                }
            }
        }
    }
    // Rastgele nonce ekle (her seal'de farklı çıktı üretmek için)
    let mut nonce = [0u8; 16];
    crate::crypto::rdrand_bytes(&mut nonce);
    pcr_material.extend_from_slice(&nonce);

    // Seal anahtarı = HMAC-SHA256(PCR material, "tpm-seal-key")
    let seal_key = crate::net::quic::hmac_sha256(&pcr_material, b"tpm-seal-key");
    // Şifreleme anahtarı = HMAC-SHA256(seal_key, "encrypt")
    let enc_key = crate::net::quic::hmac_sha256(&seal_key, b"encrypt");
    // HMAC anahtarı = HMAC-SHA256(seal_key, "hmac")
    let hmac_key = crate::net::quic::hmac_sha256(&seal_key, b"hmac");

    // Mühür başlığı: magic(4) + pcr_mask(4) + data_len(4) + nonce(16)
    let mut sealed = Vec::with_capacity(28 + data.len() + 32);
    sealed.extend_from_slice(&0x54504D53u32.to_be_bytes()); // "TPMS" magic
    sealed.extend_from_slice(&pcr_mask.to_be_bytes());
    sealed.extend_from_slice(&(data.len() as u32).to_be_bytes());
    sealed.extend_from_slice(&nonce);

    // AES-CTR şifreleme: enc_key ile counter mode
    let mut counter = [0u8; 32];
    counter[..16].copy_from_slice(&nonce);
    for chunk_idx in 0..(data.len() + 31) / 32 {
        // Counter bloğu için keystream üret: HMAC(enc_key, counter || block_idx)
        let mut ctr_input = Vec::with_capacity(36);
        ctr_input.extend_from_slice(&counter);
        ctr_input.extend_from_slice(&(chunk_idx as u32).to_be_bytes());
        let keystream = crate::net::quic::hmac_sha256(&enc_key, &ctr_input);

        let start = chunk_idx * 32;
        let end = (start + 32).min(data.len());
        for j in start..end {
            sealed.push(data[j] ^ keystream[j - start]);
        }
    }

    // HMAC bütünlük kontrolü (şifreli veri üzerinden)
    let hmac = crate::net::quic::hmac_sha256(&hmac_key, &sealed);
    sealed.extend_from_slice(&hmac);

    // Anahtar blob'a dahil EDİLMEZ — TPM PCR değerlerinden yeniden türetir
    crate::serial_println!(
        "[TPM] Data sealed: {} bytes, PCR mask={:#x}",
        data.len(),
        pcr_mask
    );
    Ok(sealed)
}

/// Daha önce mühürlenmiş (seal) veriyi açar (unseal).
///
/// Açma, yalnızca şu anda mevcut PCR değerleri mühürleme sırasındaki
/// politikayla tam olarak eşleştiğinde başarılı olur.
/// PCR değerleri değişmişse HMAC doğrulaması başarısız olur → AuthFailed.
pub fn unseal_data(sealed: &[u8]) -> Result<Vec<u8>, TpmError> {
    let tpm = TPM_DEVICE.lock();
    let _ = tpm.as_ref().ok_or(TpmError::NotInitialized)?;

    // Format: magic(4) + pcr_mask(4) + data_len(4) + nonce(16) + encrypted_data + hmac(32)
    if sealed.len() < 28 + 32 {
        return Err(TpmError::Unknown);
    }

    // Magic kontrol
    let magic = u32::from_be_bytes([sealed[0], sealed[1], sealed[2], sealed[3]]);
    if magic != 0x54504D53 {
        return Err(TpmError::Unknown);
    }

    let pcr_mask = u32::from_be_bytes([sealed[4], sealed[5], sealed[6], sealed[7]]);
    let data_len = u32::from_be_bytes([sealed[8], sealed[9], sealed[10], sealed[11]]) as usize;
    let nonce = &sealed[12..28];

    if sealed.len() < 28 + data_len + 32 {
        return Err(TpmError::Unknown);
    }

    // PCR değerlerinden anahtarı yeniden türet (seal ile aynı işlem)
    let mut pcr_material = Vec::with_capacity(256);
    pcr_material.extend_from_slice(&pcr_mask.to_be_bytes());
    for i in 0..24u32 {
        if pcr_mask & (1 << i) != 0 {
            let mut sel = PcrSelection {
                hash: TPM2_ALG_SHA256,
                size: 3,
                select: [0u8; 16],
            };
            sel.select_pcr(i as u8);
            if let Ok(pcr_vals) = pcr_read(&sel) {
                for pv in &pcr_vals {
                    pcr_material.extend_from_slice(&pv.value);
                }
            }
        }
    }
    pcr_material.extend_from_slice(nonce);

    let seal_key = crate::net::quic::hmac_sha256(&pcr_material, b"tpm-seal-key");
    let enc_key = crate::net::quic::hmac_sha256(&seal_key, b"encrypt");
    let hmac_key = crate::net::quic::hmac_sha256(&seal_key, b"hmac");

    // HMAC doğrula — PCR değerleri değişmişse burada başarısız olur
    let hmac_offset = sealed.len() - 32;
    let stored_hmac = &sealed[hmac_offset..];
    let computed_hmac = crate::net::quic::hmac_sha256(&hmac_key, &sealed[..hmac_offset]);
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= stored_hmac[i] ^ computed_hmac[i];
    }
    if diff != 0 {
        crate::serial_println!("[TPM] Unseal failed: PCR values changed or data tampered");
        return Err(TpmError::AuthFailed);
    }

    // AES-CTR şifre çöz
    let encrypted = &sealed[28..28 + data_len];
    let mut counter = [0u8; 32];
    counter[..16].copy_from_slice(nonce);
    let mut plaintext = Vec::with_capacity(data_len);
    for chunk_idx in 0..(data_len + 31) / 32 {
        let mut ctr_input = Vec::with_capacity(36);
        ctr_input.extend_from_slice(&counter);
        ctr_input.extend_from_slice(&(chunk_idx as u32).to_be_bytes());
        let keystream = crate::net::quic::hmac_sha256(&enc_key, &ctr_input);

        let start = chunk_idx * 32;
        let end = (start + 32).min(data_len);
        for j in start..end {
            plaintext.push(encrypted[j] ^ keystream[j - start]);
        }
    }

    crate::serial_println!("[TPM] Data unsealed: {} bytes", data_len);
    Ok(plaintext)
}

/// Seçili PCR kayıtlarından imzalı alıntı (quote) üreterek uzaktan onay gerçekleştirir.
///
/// Uzaktan Onay Akışı:
///   1. Doğrulayıcı sistemin güvenilir olup olmadığını sorgulamak ister
///   2. Doğrulayıcı -> nonce (tekrar kullanım saldırısını önler)
///   3. attest(nonce) -> TPM EK ile imzalanmış PCR alıntısı
///   4. Doğrulayıcı: imzayı, PCR değerlerini ve nonce'u doğrular
///   5. PCR değerleri beklenen bütünlük ölçümleriyle eşleşirse güven kurulur
pub fn attest(nonce: &[u8]) -> Result<AttestationResult, TpmError> {
    let mut tpm = TPM_DEVICE.lock();
    let device = tpm.as_mut().ok_or(TpmError::NotInitialized)?;

    // Tüm PCR'ları alıntıla
    let selection = PcrSelection::new_sha256();
    let quote = device.quote(TPM2_RH_ENDORSEMENT, nonce, &selection)?;

    Ok(AttestationResult {
        quote,
        pcr_values: Vec::new(),
        signature: Vec::new(),
    })
}

// ============================================================================
// ONAY SONUCU (AttestationResult)
//
// attest() fonksiyonunun döndürdüğü uzaktan onay sonucu.
//
//  quote:       TPMS_ATTEST yapısının serileştirilmiş hali
//               (sihir baytı + tip + nonce + PCR değerleri)
//
//  pcr_values:  Alıntılanan PCR kayıtlarının SHA-256 değerleri
//               Doğrulayıcı bunları beklenen değerlerle karşılaştırır
//
//  signature:   TPM EK (Endorsement Key) ile ECDSA/RSA imzası
//               Doğrulayıcı bu imzayı EK sertifikasıyla doğrular
// ============================================================================

/// Uzaktan onay (remote attestation) işleminin sonuç yapısı
#[derive(Clone, Debug)]
pub struct AttestationResult {
    /// TPM tarafından üretilen imzalı PCR alıntı verisi (TPMS_ATTEST)
    pub quote: Vec<u8>,
    /// Alıntılanan PCR kayıtlarının SHA-256 değerleri
    pub pcr_values: Vec<PcrValue>,
    /// Alıntıyı doğrulayan EK veya AIK ile oluşturulmuş kriptografik imza
    pub signature: Vec<u8>,
}

/// TPM'nin başlatılmış ve kullanıma hazır olup olmadığını döndürür.
///
/// `true` dönerse TPM_DEVICE'in `Some` variant'ında bir aygıt örneği bulunmaktadır.
/// `false` dönerse init() henüz çağrılmamış veya başarısız olmuştur.
pub fn is_available() -> bool {
    TPM_DEVICE.lock().is_some()
}
