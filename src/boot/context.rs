#[cfg(target_os = "uefi")]
use crate::boot::BootInfo;
use crate::gop::framebuffer::Framebuffer;
use alloc::vec::Vec;
use core::arch::x86_64::{__cpuid, __cpuid_count};

/// BootContext şema sürümü. v2: field-state + capability + ownership modeli.
pub const BOOT_CONTEXT_VERSION: u32 = 2;

/// Command line için kernel-owned sabit buffer boyutu.
///
/// Sessiz truncation yasaktır: NUL terminator'dan önceki veri bu kapasiteyi
/// aşarsa açık validation hatası üretilir (`CmdlineOverflow`).
pub const CMDLINE_BUFFER_LEN: usize = 256;

// ============================================================================
// FIELD STATE
// ============================================================================

/// Bir boot alanının mevcudiyet/doğrulama durumu.
///
/// Bir alanın *zorunlu* olup olmadığı burada değil, stage-gate validator'da
/// (boot profili + init stage) belirlenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldState {
    /// Kaynak doğrulandı ve kopyalandı; tüketici güvenle kullanabilir.
    PresentValidated,
    /// Kaynaktan alındı ancak henüz doğrulanmadı (ör. RSDP signature/checksum).
    PresentUntrusted,
    /// Kaynak yok; kernel kendi hesapladığı değeri yerleştirdi.
    Synthesized,
    /// Kaynak bu alanı desteklemiyor (ör. bootloader base revision yetersiz).
    Unsupported,
    /// Kaynak bu alanı sağlamadı.
    Absent,
    /// Değer var ancak doğrulamayı geçemedi (kapasite aşımı, geçersiz UTF-8,
    /// checksum uyumsuzluğu vb.). Tüketici bu alana güvenmemelidir.
    Invalid,
}

impl FieldState {
    /// Alan tüketici tarafından okunabilir durumda mı?
    pub fn is_present(self) -> bool {
        matches!(
            self,
            FieldState::PresentValidated | FieldState::PresentUntrusted | FieldState::Synthesized
        )
    }
}

// ============================================================================
// CAPABILITY FLAGS
// ============================================================================

/// Boot kaynağının sağlayabildiği/ürettiği alanları temsil eden bit seti.
///
/// Parity iddiaları capability'lere dayanır: bir protokol, yalnızca
/// capability biti set olan alanlar için diğer protokollerle eşdeğer kabul
/// edilir. `memory_map` (normalize, BootContext içinde) Wave 1'de henüz
/// üretilmediğinden biti de set değildir — legacy kanal Wave 3'te kaldırılır.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilityFlags(u64);

impl CapabilityFlags {
    pub const MEMORY_MAP: CapabilityFlags = CapabilityFlags(1 << 0);
    pub const CMDLINE: CapabilityFlags = CapabilityFlags(1 << 1);
    pub const RSDP: CapabilityFlags = CapabilityFlags(1 << 2);
    pub const FRAMEBUFFER: CapabilityFlags = CapabilityFlags(1 << 3);
    pub const SYSTEM_TABLE: CapabilityFlags = CapabilityFlags(1 << 4);
    pub const RUNTIME_SERVICES: CapabilityFlags = CapabilityFlags(1 << 5);
    pub const SECURE_BOOT: CapabilityFlags = CapabilityFlags(1 << 6);
    pub const IMAGE_HASH: CapabilityFlags = CapabilityFlags(1 << 7);
    /// Wave 3: page-frame allocator parite alanı.
    pub const PMM: CapabilityFlags = CapabilityFlags(1 << 8);
    /// Karar 7: kernel-owned entropy tohumu (`EntropyState`).
    pub const ENTROPY: CapabilityFlags = CapabilityFlags(1 << 9);
    /// Karar 7: AP trampoline sahipliği/aralığı (`ApTrampolineState`).
    /// Wave 1'de hiçbir protokol sağlamaz; SMP fazı (sonraki wave) doldurur.
    pub const AP_TRAMPOLINE: CapabilityFlags = CapabilityFlags(1 << 10);
    /// Karar 7: erken CPU feature snapshot'ı (`CpuFeatureSnapshot`, BSP CPUID).
    pub const PRE_CPU_FEATURES: CapabilityFlags = CapabilityFlags(1 << 11);
    /// Karar 7: firmware güven kanıtı (`FirmwareTrustEvidence`, tri-state SB).
    pub const FIRMWARE_TRUST: CapabilityFlags = CapabilityFlags(1 << 12);
    /// Modül/initrd descriptor kopyaları (`ModuleListState`).
    pub const MODULES: CapabilityFlags = CapabilityFlags(1 << 13);
    /// Doğrulanmış higher-half direct map. Boot adapter'ı sıfır/canonical
    /// kontrollerinden ve gerçek eşlemeden sonra yayınlar.
    pub const HHDM: CapabilityFlags = CapabilityFlags(1 << 14);
    /// UEFI runtime sanal adres geçişi tamamlandı ve çağrı yüzeyi doğrulandı.
    pub const RUNTIME_VERIFIED: CapabilityFlags = CapabilityFlags(1 << 15);
    /// Recovery yolu firmware reset hook'unu güvenle çağırabilir.
    pub const REBOOT_SAFE: CapabilityFlags = CapabilityFlags(1 << 16);
    /// SMBIOS giriş noktası normalize edilip doğrulandı.
    pub const SMBIOS: CapabilityFlags = CapabilityFlags(1 << 17);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl core::ops::BitOr for CapabilityFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for CapabilityFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAnd for CapabilityFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::Not for CapabilityFlags {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// Capability negotiation sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityNegotiation {
    pub required: CapabilityFlags,
    pub present: CapabilityFlags,
    pub missing: CapabilityFlags,
}

impl CapabilityNegotiation {
    pub fn supported(self) -> bool {
        self.missing.bits() == 0
    }
}

// ============================================================================
// BOOT PROFILE & STAGE
// ============================================================================

/// Boot kaynağı profili — stage-gate zorunluluk matrisinin bir ekseni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootProfile {
    Uefi,
    Limine,
    Multiboot2,
    Host,
}

/// Init aşaması — stage-gate zorunluluk matrisinin ikinci ekseni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootInitStage {
    /// Pipeline handover; profile'a özgü zorunluluklar burada denetlenir.
    Handover,
    Paging,
    Heap,
    /// ACPI aşaması; RSDP PresentValidated olmalıdır.
    Acpi,
    Services,
    /// Karar 7: SMP fazı — `ap_trampoline` alanı present olmalıdır.
    /// Wave 1'de çağrı yoktur; şema + gate burada tanımlıdır.
    Smp,
    /// Karar 7: KASLR fazı — güvenilir entropy istenir; deterministic
    /// fallback yalnızca `Host` profili için geçerlidir.
    Kaslr,
}

// ============================================================================
// RSDP CANDIDATE (typed authoritative ACPI bootstrap)
// ============================================================================

/// RSDP adresinin bellek alanı türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsdpAddressKind {
    Physical,
    Virtual,
}

/// RSDP'nin geldiği boot kaynağı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsdpProvenance {
    Uefi,
    Limine,
    Multiboot2,
    BiosScan,
}

/// Tek authoritative ACPI bootstrap adayı.
///
/// Wave 1 kararı: `cpu::acpi::UEFI_RSDP_ADDRESS` ve `acpi::RSDP_PHYS`
/// biçimindeki iki bağımsız doğruluk kaynağı kaldırıldı; tek doğruluk kaynağı
/// `acpi::publish_rsdp` üzerinden kurulan bu adaydır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RsdpCandidate {
    pub address: u64,
    pub address_kind: RsdpAddressKind,
    pub provenance: RsdpProvenance,
    pub field_state: FieldState,
    pub acpi_revision: u8,
}

impl RsdpCandidate {
    /// `field_state = PresentUntrusted`, `acpi_revision = 0` ile kurar;
    /// signature/checksum/length doğrulaması ACPI fazında
    /// `acpi::validate_authoritative_rsdp` ile yapılır.
    pub fn new(address: u64, address_kind: RsdpAddressKind, provenance: RsdpProvenance) -> Self {
        Self {
            address,
            address_kind,
            provenance,
            field_state: FieldState::PresentUntrusted,
            acpi_revision: 0,
        }
    }

    pub fn absent() -> Self {
        Self {
            address: 0,
            address_kind: RsdpAddressKind::Physical,
            provenance: RsdpProvenance::BiosScan,
            field_state: FieldState::Absent,
            acpi_revision: 0,
        }
    }
}

/// RSDP yayınlama/doğrulama hataları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsdpError {
    /// Sıfır adres asla kabul edilmez.
    ZeroAddress,
    /// Adres canonical lower-half değil.
    NonCanonicalAddress(u64),
    /// Fiziksel/sanal adres türleri karıştırılamaz.
    KindMismatch {
        existing: RsdpAddressKind,
        incoming: RsdpAddressKind,
    },
    /// İkinci ve çelişkili RSDP sessizce kabul edilmez.
    ConflictingExisting { existing: u64, incoming: u64 },
    /// Henüz aday yok.
    NoCandidate,
    /// Okunan veri bir RSDP için çok kısa.
    TooShort { len: usize },
    /// "RSD PTR " imzası eşleşmedi.
    SignatureMismatch,
    /// Checksum toplamı sıfır değil.
    ChecksumMismatch,
    /// ACPI 2.0+ length alanı geçersiz (< 36).
    LengthInvalid { declared: u32 },
}

// ============================================================================
// MEMORY MAP STATE (Wave 3 sahiplik/ömür/doğrulama modeli)
// ============================================================================

/// Normalize edilmiş bellek bölgesi türü (Wave 3'te doldurulur).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
    ACPIReclaim,
    ACPINVS,
    Framebuffer,
    Kernel,
    BootloaderReclaimable,
    Unknown,
}

/// Kernel-owned canonical memory map kapasitesi.
///
/// Bootloader response'ları heap kurulmadan önce tüketildiği için bu yapı
/// hiçbir dinamik tahsis yapmaz. Her üç adapter aynı sabit kapasiteyi kullanır;
/// aşım sessizce kesilmez ve handover fatal gate'ine gider.
pub const MAX_NORMALIZED_MEMORY_REGIONS: usize = 256;

/// Normalize edilmiş bellek haritası.
#[derive(Debug, Clone, Copy)]
pub struct NormalizedMemoryMap {
    pub regions: [MemoryRegion; MAX_NORMALIZED_MEMORY_REGIONS],
    pub len: usize,
    pub total_pages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    pub base: u64,
    pub len: u64,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub const EMPTY: Self = Self {
        base: 0,
        len: 0,
        kind: MemoryRegionKind::Reserved,
    };
}

impl NormalizedMemoryMap {
    pub const fn empty() -> Self {
        Self {
            regions: [MemoryRegion::EMPTY; MAX_NORMALIZED_MEMORY_REGIONS],
            len: 0,
            total_pages: 0,
        }
    }

    /// Bölgeyi doğrulayarak bounded bootstrap storage'a ekler.
    pub fn push(&mut self, region: MemoryRegion) -> Result<(), ()> {
        if region.len == 0
            || region.base.checked_add(region.len).is_none()
            || self.len >= MAX_NORMALIZED_MEMORY_REGIONS
        {
            return Err(());
        }
        let pages = region.len.saturating_add(4095) / 4096;
        let total = self.total_pages.checked_add(pages).ok_or(())?;
        self.regions[self.len] = region;
        self.len += 1;
        self.total_pages = total;
        Ok(())
    }

    pub fn as_slice(&self) -> &[MemoryRegion] {
        &self.regions[..self.len]
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Bellek haritası kaynağının sahipliği.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapOwnership {
    /// Bootloader (firmware/protokol) tarafından sağlanıyor.
    Bootloader,
    /// Kernel tarafından normalize edilip sahiplenildi.
    Kernel,
    Unowned,
}

/// Bellek haritası verisinin ömrü.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapLifetime {
    Static,
    Transient,
}

/// Bellek haritası doğrulama derecesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapValidation {
    Unvalidated,
    LengthChecked,
    FullyValidated,
}

/// BootContext `memory_map` alanının durumu.
///
/// Wave 3'te her adapter bu alanı heap öncesi bounded storage ile doldurur.
#[derive(Debug, Clone)]
pub struct MemoryMapState {
    pub field_state: FieldState,
    pub ownership: MemoryMapOwnership,
    pub lifetime: MemoryMapLifetime,
    pub validation: MemoryMapValidation,
    pub normalized: Option<NormalizedMemoryMap>,
}

impl MemoryMapState {
    pub fn unavailable() -> Self {
        Self {
            field_state: FieldState::Absent,
            ownership: MemoryMapOwnership::Unowned,
            lifetime: MemoryMapLifetime::Transient,
            validation: MemoryMapValidation::Unvalidated,
            normalized: None,
        }
    }

    pub fn validated(normalized: NormalizedMemoryMap) -> Self {
        Self {
            field_state: FieldState::PresentValidated,
            ownership: MemoryMapOwnership::Kernel,
            lifetime: MemoryMapLifetime::Static,
            validation: MemoryMapValidation::FullyValidated,
            normalized: Some(normalized),
        }
    }
}

// ============================================================================
// ENTROPY STATE (Karar 7: provenance + quality)
// ============================================================================

/// Entropi tohumunun geldiği kaynak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropySource {
    /// UEFI RNG protokolü (efi_main, ExitBootServices öncesi okunur).
    UefiRng,
    /// Limine entropy request (limine-protocol-for-rust 0.2.1 desteklemez).
    Limine,
    /// Multiboot2 bir entropy kanalı tanımlamaz.
    Multiboot2,
    Unknown,
}

/// Entropi kalitesi derecesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropyQuality {
    /// Firmware RNG protokolü (UEFI RNG / donanım RNG tabanlı).
    High,
    /// Bootloader tohumu, donanım RNG'si değil.
    Medium,
    /// Bilinen deterministik kaynak.
    Low,
    Unknown,
}

/// Kernel-owned entropy tohumu.
///
/// Sabit 32-byte sözleşme: farklı uzunluktaki kaynak `Invalid` olur
/// (sessiz truncation yasaktır). Kaynak pointer'a ömür bağlanmaz.
#[derive(Debug, Clone)]
pub struct EntropyState {
    pub field_state: FieldState,
    pub source: EntropySource,
    pub quality: EntropyQuality,
    pub seed: [u8; 32],
}

impl EntropyState {
    pub fn absent() -> Self {
        Self {
            field_state: FieldState::Absent,
            source: EntropySource::Unknown,
            quality: EntropyQuality::Unknown,
            seed: [0u8; 32],
        }
    }

    /// Tohumu kernel-owned buffer'a kopyalar. Tam 32 bayt değilse `Invalid`.
    pub fn store_seed(&mut self, src: &[u8]) {
        if src.len() != self.seed.len() {
            self.field_state = FieldState::Invalid;
            return;
        }
        self.seed.copy_from_slice(src);
        self.field_state = FieldState::PresentValidated;
    }
}

// ============================================================================
// AP TRAMPOLINE STATE (Karar 7: ownership/range/alignment)
// ============================================================================

/// AP trampoline bölgesinin sahibi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrampolineOwnership {
    /// Bootloader bir trampoline bölgesi yayınladı.
    Bootloader,
    /// Kernel kendi trampoline'ını kurdu (SMP fazı, sonraki wave).
    Kernel,
    None,
}

/// AP trampoline bölgesi (fiziksel aralık + alignment).
///
/// Wave 1'de hiçbir boot protokolü trampoline sağlamaz: tüm builder'lar
/// `absent()` kurar; SMP fazı (sonraki wave) `Kernel` sahipliğiyle doldurur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApTrampolineState {
    pub field_state: FieldState,
    pub ownership: TrampolineOwnership,
    /// Fiziksel başlangıç adresi.
    pub base: u64,
    /// Bölge uzunluğu (bayt).
    pub range_len: u64,
    /// Gerekli hizalama (0 = belirtilmemiş).
    pub alignment: u64,
}

impl ApTrampolineState {
    pub fn absent() -> Self {
        Self {
            field_state: FieldState::Absent,
            ownership: TrampolineOwnership::None,
            base: 0,
            range_len: 0,
            alignment: 0,
        }
    }

    pub fn kernel_owned(base: u64, range_len: u64, alignment: u64) -> Self {
        Self {
            field_state: FieldState::Synthesized,
            ownership: TrampolineOwnership::Kernel,
            base,
            range_len,
            alignment,
        }
    }
}

// ============================================================================
// PRE-CPU FEATURE SNAPSHOT (Karar 7: erken CPUID)
// ============================================================================

/// CPU vendor'ı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Other,
    Unknown,
}

/// Erken CPU feature seti (CPUID leaf 1 / 7 / 0x8000_0001).
///
/// cpu::init'ten ÖNCE BSP'de alınır; kernel tüm adaptörlerde kendisi üretir
/// (FieldState::Synthesized). Donanım gerçeği iddia etmez — yalnızca snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuFeatureFlags(u64);

impl CpuFeatureFlags {
    pub const SSE: CpuFeatureFlags = CpuFeatureFlags(1 << 0);
    pub const SSE2: CpuFeatureFlags = CpuFeatureFlags(1 << 1);
    pub const AVX: CpuFeatureFlags = CpuFeatureFlags(1 << 2);
    pub const AVX2: CpuFeatureFlags = CpuFeatureFlags(1 << 3);
    pub const XSAVE: CpuFeatureFlags = CpuFeatureFlags(1 << 4);
    pub const SMEP: CpuFeatureFlags = CpuFeatureFlags(1 << 5);
    pub const SMAP: CpuFeatureFlags = CpuFeatureFlags(1 << 6);
    pub const SYSCALL: CpuFeatureFlags = CpuFeatureFlags(1 << 7);
    pub const NX: CpuFeatureFlags = CpuFeatureFlags(1 << 8);
    pub const X2APIC: CpuFeatureFlags = CpuFeatureFlags(1 << 9);
    pub const RDRAND: CpuFeatureFlags = CpuFeatureFlags(1 << 10);
    pub const FSGSBASE: CpuFeatureFlags = CpuFeatureFlags(1 << 11);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl core::ops::BitOr for CpuFeatureFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// BSP CPUID snapshot'ı — cpu::init'ten önce alınır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFeatureSnapshot {
    pub field_state: FieldState,
    pub vendor: CpuVendor,
    pub max_std_level: u32,
    pub max_ext_level: u32,
    pub features: CpuFeatureFlags,
}

impl CpuFeatureSnapshot {
    /// BSP'de gerçek CPUID okuyarak snapshot alır (`Synthesized`).
    pub fn synthesize() -> Self {
        let l0 = unsafe { __cpuid(0) };
        let mut vendor_bytes = [0u8; 12];
        vendor_bytes[..4].copy_from_slice(&l0.ebx.to_le_bytes());
        vendor_bytes[4..8].copy_from_slice(&l0.edx.to_le_bytes());
        vendor_bytes[8..12].copy_from_slice(&l0.ecx.to_le_bytes());
        let vendor = match &vendor_bytes {
            b"GenuineIntel" => CpuVendor::Intel,
            b"AuthenticAMD" => CpuVendor::Amd,
            _ => CpuVendor::Other,
        };
        let max_std_level = l0.eax;
        let mut features = CpuFeatureFlags::empty();
        let l1 = unsafe { __cpuid(1) };
        if l1.edx & (1 << 25) != 0 {
            features.insert(CpuFeatureFlags::SSE);
        }
        if l1.edx & (1 << 26) != 0 {
            features.insert(CpuFeatureFlags::SSE2);
        }
        if l1.ecx & (1 << 28) != 0 {
            features.insert(CpuFeatureFlags::AVX);
        }
        if l1.ecx & (1 << 26) != 0 {
            features.insert(CpuFeatureFlags::XSAVE);
        }
        if l1.ecx & (1 << 21) != 0 {
            features.insert(CpuFeatureFlags::X2APIC);
        }
        if l1.ecx & (1 << 30) != 0 {
            features.insert(CpuFeatureFlags::RDRAND);
        }
        if max_std_level >= 7 {
            let l7 = unsafe { __cpuid_count(7, 0) };
            if l7.ebx & (1 << 0) != 0 {
                features.insert(CpuFeatureFlags::FSGSBASE);
            }
            if l7.ebx & (1 << 5) != 0 {
                features.insert(CpuFeatureFlags::AVX2);
            }
            if l7.ebx & (1 << 7) != 0 {
                features.insert(CpuFeatureFlags::SMEP);
            }
            if l7.ebx & (1 << 20) != 0 {
                features.insert(CpuFeatureFlags::SMAP);
            }
        }
        let ext = unsafe { __cpuid(0x8000_0000) };
        let max_ext_level = ext.eax;
        if max_ext_level >= 0x8000_0001 {
            let lx = unsafe { __cpuid(0x8000_0001) };
            if lx.edx & (1 << 11) != 0 {
                features.insert(CpuFeatureFlags::SYSCALL);
            }
            if lx.edx & (1 << 20) != 0 {
                features.insert(CpuFeatureFlags::NX);
            }
        }
        Self {
            field_state: FieldState::Synthesized,
            vendor,
            max_std_level,
            max_ext_level,
            features,
        }
    }
}

// ============================================================================
// FIRMWARE TRUST EVIDENCE (Karar 7: tri-state secure boot)
// ============================================================================

/// Secure boot durumu — tri-state: bilinen / kanal yok / raporlanmadı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureBootStatus {
    /// Firmware durumu biliniyor (UEFI Secure Boot vars/protokol).
    Known(bool),
    /// Kaynak protokol güven kanalı raporlamıyor (Limine/MB2).
    Unsupported,
    /// Kaynak hiç raporlamadı.
    Unknown,
}

/// Kernel imaj ölçümünün kaynağı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMeasurementSource {
    /// UEFI: load-time SHA-256 ölçümü (inspect_loaded_image).
    Uefi,
    None,
}

/// Firmware güven kanıtı.
///
/// `image_hash_verified` yoktur: hash yalnızca *ölçülür* (measured), güvenilir
/// bir kök değere karşı doğrulanmaz — doğrulama iddiası fake olur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareTrustEvidence {
    pub field_state: FieldState,
    pub secure_boot: SecureBootStatus,
    pub image_measurement: ImageMeasurementSource,
    pub image_hash: [u8; 32],
    /// `image_size != 0` olduğunda efi_main ölçüm yapmıştır.
    pub image_hash_present: bool,
    pub image_size: u64,
}

impl FirmwareTrustEvidence {
    pub fn unavailable() -> Self {
        Self {
            field_state: FieldState::Unsupported,
            secure_boot: SecureBootStatus::Unsupported,
            image_measurement: ImageMeasurementSource::None,
            image_hash: [0u8; 32],
            image_hash_present: false,
            image_size: 0,
        }
    }
}

// ============================================================================
// MODULE / INITRD DESCRIPTORS (ownership-safe kopya)
// ============================================================================

/// Modül adı için kernel-owned sabit buffer boyutu.
pub const MODULE_NAME_LEN: usize = 64;

/// Kernel-owned modül descriptor'ı — ad bootloader verisinden kopyalanır;
/// modül içeriği (initrd payload) bootloader bölgesinde kalır.
#[derive(Debug, Clone, Copy)]
pub struct ModuleDescriptor {
    pub name: [u8; MODULE_NAME_LEN],
    pub name_len: usize,
    /// Fiziksel aralık (MB2: [mod_start, mod_end)).
    pub base: u64,
    pub len: u64,
    pub flags: u32,
}

impl ModuleDescriptor {
    /// Adı kernel-owned buffer'a kopyalar; taşma açık hata döner
    /// (kısmi kopya yapılmaz).
    pub fn new(name: &[u8], base: u64, len: u64, flags: u32) -> Result<Self, BootContextError> {
        let mut raw = [0u8; MODULE_NAME_LEN];
        if name.len() > MODULE_NAME_LEN {
            return Err(BootContextError::ModuleNameOverflow {
                capacity: MODULE_NAME_LEN,
            });
        }
        raw[..name.len()].copy_from_slice(name);
        Ok(Self {
            name: raw,
            name_len: name.len(),
            base,
            len,
            flags,
        })
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

/// Modül descriptor listesinin durumu.
#[derive(Debug, Clone)]
pub struct ModuleListState {
    pub field_state: FieldState,
    pub modules: Vec<ModuleDescriptor>,
}

impl ModuleListState {
    pub fn unavailable() -> Self {
        Self {
            field_state: FieldState::Absent,
            modules: Vec::new(),
        }
    }
}

// ============================================================================
// LIMINE RAW REQUEST TYPES (Karar 7 kapanışı)
// ============================================================================

/// Limine module request (spec `Module Feature` bölümü).
///
/// limine-protocol-for-rust 0.2.1'de `ModuleRequest` yoktur; bu raw yapı
/// `D:\echOS Kaynak Arşivi\specs\limine-protocol.md`'den doğrulanan ABI ile
/// birebir eşleşir: `id[4] = { common_magic[2], 0x3e7e279702be32af,
/// 0xca1c4f3bd1280cee }`, ardından `revision` ve `response` pointer'ı.
/// Request revision 0 kullanılır (internal module yok); module feature base
/// revision 0'dan beri protokolde mevcuttur.
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct LimineModuleRequest {
    id: [u64; 4],
    revision: u64,
    response: core::mem::MaybeUninit<usize>,
}

#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
impl LimineModuleRequest {
    pub const fn new() -> Self {
        Self {
            id: [
                0xc7b1dd30df4c8b88,
                0x0a82e883a194f07b,
                0x3e7e279702be32af,
                0xca1c4f3bd1280cee,
            ],
            revision: 0,
            response: core::mem::MaybeUninit::uninit(),
        }
    }

    /// Bootloader'ın doldurduğu response pointer'ını okur (null → None).
    pub fn get_response(&self) -> Option<&'static LimineModuleResponseView> {
        let ptr = unsafe { self.response.assume_init() } as *const LimineModuleResponseView;
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*ptr })
        }
    }
}

/// `limine_module_response` view'ı (spec): revision @0, module_count @8,
/// modules @16.
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct LimineModuleResponseView {
    pub revision: u64,
    pub module_count: u64,
    pub modules: *const *const LimineModuleFileView,
}

/// `struct limine_file`'in tüm sürümlerde sabit olan ön kısmı.
///
/// Arşiv spec'i (trunk) ile crate 0.2.1 `LimineFile`'ı modül yuvalama alanları
/// içermez ve ilk 7 alan iki kaynakta da birebir aynıdır; bu prefix-only view
/// iki layout ile de uyumludur (daha sonra eklenen alanlar okunmaz).
#[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct LimineModuleFileView {
    pub revision: u64,
    pub address: u64,
    pub size: u64,
    pub path: *const u8,
    pub string: *const u8,
    pub media_type: u32,
    pub _unused: u32,
}

// ============================================================================
// BOOT CONTEXT v2
// ============================================================================

/// BootContext yapı hataları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootContextError {
    /// Kaynak veri sabit buffer'a sığmıyor — kısmi kopya yapılmaz.
    CmdlineOverflow { capacity: usize },
    /// C-stili string NUL terminator içermiyor.
    CmdlineMissingNulTerminator,
    /// Kaynak pointer null.
    CmdlineNullPointer,
    /// Modül descriptor adı sabit buffer'a sığmıyor — kısmi kopya yapılmaz.
    ModuleNameOverflow { capacity: usize },
    /// Limine modül yanıtında null pointer (modül dizisi öğesi veya path).
    ModuleNullPointer,
    /// Limine modül path'i `MODULE_NAME_LEN` içinde NUL terminator içermiyor.
    ModuleNameMissingNulTerminator,
    /// Bootloader revision'ı istenen minimumu karşılamıyor.
    UnsupportedRevision { min: u64, actual: u64 },
    /// Stage-gate: zorunlu alan mevcut değil / geçersiz.
    MissingRequiredField {
        field: &'static str,
        state: FieldState,
    },
}

impl core::fmt::Display for BootContextError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CmdlineOverflow { capacity } => {
                write!(f, "cmdline kapasite aşımı (buffer {})", capacity)
            }
            Self::CmdlineMissingNulTerminator => write!(f, "cmdline NUL terminator eksik"),
            Self::CmdlineNullPointer => write!(f, "cmdline pointer null"),
            Self::ModuleNameOverflow { capacity } => {
                write!(f, "modül adı kapasite aşımı (buffer {})", capacity)
            }
            Self::ModuleNullPointer => write!(f, "modül pointer null"),
            Self::ModuleNameMissingNulTerminator => write!(f, "modül adı NUL terminator eksik"),
            Self::UnsupportedRevision { min, actual } => {
                write!(f, "desteklenmeyen revision: min {} actual {}", min, actual)
            }
            Self::MissingRequiredField { field, state } => {
                write!(f, "zorunlu alan '{}' durumu {:?}", field, state)
            }
        }
    }
}

/// Protocol‑agnostic boot‑context carrying everything a boot protocol
/// can supply after its adapter normalises the data.  Non‑UEFI paths
/// leave UEFI‑only fields at their zero / default values.
#[derive(Debug, Clone)]
pub struct BootContext {
    pub version: u32,
    pub physical_memory_offset: u64,
    pub hhdm_offset: u64,
    /// Typed authoritative RSDP adayı (address her zaman fiziksel olarak
    /// normalize edilir; `address_kind` gelecekteki sanal kaynaklar için).
    pub rsdp: RsdpCandidate,
    /// Kernel-owned sabit buffer; kaynak pointer'a ömür bağlamaz.
    pub cmdline_raw: [u8; CMDLINE_BUFFER_LEN],
    pub cmdline_len: usize,
    pub cmdline_state: FieldState,
    /// Kernel-owned, bounded ve doğrulanmış canonical harita.
    pub memory_map: MemoryMapState,
    pub capabilities: CapabilityFlags,
    /// Framebuffer moved from the protocol‑specific boot‑info.
    /// Mutated in‑place during the UEFI pipeline to remap `base_addr`.
    pub framebuffer: Option<Framebuffer>,
    pub system_table: u64,
    pub runtime_services: usize,
    /// Legacy tek-bit bayrağı — mevcut secure-boot pipeline'ı (sb syscall'ları)
    /// tarafından kullanılır. Şema alanı `firmware_trust.secure_boot` (tri-state).
    pub secure_boot: bool,
    /// Legacy: kernel imaj SHA-256'sı (UEFI pipeline ölçümü).
    pub image_hash: [u8; 32],
    pub image_size: u64,
    /// Karar 7: entropy tohumu (provenance + quality).
    pub entropy: EntropyState,
    /// Karar 7: AP trampoline sahipliği/aralığı/hizası.
    pub ap_trampoline: ApTrampolineState,
    /// Karar 7: cpu::init'ten önce alınan BSP CPUID snapshot'ı.
    pub pre_cpu_features: CpuFeatureSnapshot,
    /// Karar 7: firmware güven kanıtı (tri-state secure boot + imaj ölçümü).
    pub firmware_trust: FirmwareTrustEvidence,
    /// Modül/initrd descriptor kopyaları (kernel-owned adlar).
    pub modules: ModuleListState,
}

impl BootContext {
    /// Tüm builder'ların ortak çekirdeği: tüm alanlar kapalı/değer başlar.
    fn base(physical_memory_offset: u64, hhdm_offset: u64) -> Self {
        Self {
            version: BOOT_CONTEXT_VERSION,
            physical_memory_offset,
            hhdm_offset,
            rsdp: RsdpCandidate::absent(),
            cmdline_raw: [0u8; CMDLINE_BUFFER_LEN],
            cmdline_len: 0,
            cmdline_state: FieldState::Absent,
            memory_map: MemoryMapState::unavailable(),
            capabilities: {
                // CPUID snapshot'ı tüm adaptörlerde aynı şekilde üretilir
                // (BSP, cpu::init öncesi) — base seviyesinde set edilir.
                let mut caps = CapabilityFlags::empty();
                caps.insert(CapabilityFlags::PRE_CPU_FEATURES);
                caps
            },
            framebuffer: None,
            system_table: 0,
            runtime_services: 0,
            secure_boot: false,
            image_hash: [0u8; 32],
            image_size: 0,
            entropy: EntropyState::absent(),
            ap_trampoline: ApTrampolineState::absent(),
            pre_cpu_features: CpuFeatureSnapshot::synthesize(),
            firmware_trust: FirmwareTrustEvidence::unavailable(),
            modules: ModuleListState::unavailable(),
        }
    }

    /// Normalize RSDP fiziksel adresi (tüm protokoller fiziksele çevirir).
    pub fn rsdp_address(&self) -> u64 {
        self.rsdp.address
    }

    /// Typed RSDP adayını belirtilen provenance ile dışa verir.
    pub fn rsdp_candidate(&self, provenance: RsdpProvenance) -> RsdpCandidate {
        RsdpCandidate {
            address: self.rsdp.address,
            address_kind: RsdpAddressKind::Physical,
            provenance,
            field_state: self.rsdp.field_state,
            acpi_revision: self.rsdp.acpi_revision,
        }
    }

    /// Command line'a erişim: yalnızca alan okunabilir durumda (present) ve
    /// veri geçerli UTF-8 ise `Some` döner.
    ///
    /// Lossy dönüşüm YOKTUR: ham baytlar her zaman `cmdline_bytes` ile
    /// saklanır; `Invalid`/`Absent`/`Unsupported` durumlarında `None` döner.
    pub fn cmdline_str(&self) -> Option<&str> {
        if self.cmdline_len == 0 || !self.cmdline_state.is_present() {
            return None;
        }
        core::str::from_utf8(self.cmdline_bytes()).ok()
    }

    /// Ham cmdline baytları (NUL terminator hariç).
    pub fn cmdline_bytes(&self) -> &[u8] {
        &self.cmdline_raw[..self.cmdline_len]
    }

    pub fn cmdline_state(&self) -> FieldState {
        self.cmdline_state
    }

    pub fn capabilities(&self) -> CapabilityFlags {
        self.capabilities
    }

    /// Canonical map'i kernel-owned static storage olarak yayınlar.
    pub fn publish_normalized_memory_map(&mut self, normalized: NormalizedMemoryMap) -> bool {
        if normalized.is_empty() {
            self.memory_map.field_state = FieldState::Invalid;
            self.memory_map.validation = MemoryMapValidation::Unvalidated;
            self.memory_map.normalized = None;
            self.capabilities = self.capabilities & !CapabilityFlags::MEMORY_MAP;
            return false;
        }
        self.memory_map = MemoryMapState::validated(normalized);
        self.capabilities.insert(CapabilityFlags::MEMORY_MAP);
        true
    }

    pub fn normalized_memory_map(&self) -> Option<&NormalizedMemoryMap> {
        self.memory_map.normalized.as_ref()
    }

    /// İstenen capability'lerin mevcut seti karşılayıp karşılamadığını hesaplar.
    pub fn negotiate(&self, required: CapabilityFlags) -> CapabilityNegotiation {
        let missing = CapabilityFlags::from_bits(required.bits() & !self.capabilities.bits());
        CapabilityNegotiation {
            required,
            present: self.capabilities,
            missing,
        }
    }

    /// Stage-gate doğrulaması: boot profili + init stage'e göre zorunlu
    /// alanları denetler. Zorunluluk matrisi burada yaşar — FieldState'te değil.
    pub fn validate_for_stage(
        &self,
        profile: BootProfile,
        stage: BootInitStage,
    ) -> Result<(), BootContextError> {
        match stage {
            BootInitStage::Handover => match profile {
                BootProfile::Uefi => {
                    self.require_nonzero("system_table", self.system_table)?;
                    self.require_nonzero("runtime_services", self.runtime_services as u64)?;
                }
                BootProfile::Limine => {
                    self.require_nonzero("hhdm_offset", self.hhdm_offset)?;
                    self.require_nonzero("rsdp", self.rsdp.address)?;
                }
                BootProfile::Multiboot2 => {
                    self.require_nonzero("rsdp", self.rsdp.address)?;
                }
                BootProfile::Host => {}
            },
            BootInitStage::Acpi => {
                if self.rsdp.address == 0 {
                    return Err(BootContextError::MissingRequiredField {
                        field: "rsdp",
                        state: self.rsdp.field_state,
                    });
                }
                if self.rsdp.field_state != FieldState::PresentValidated {
                    return Err(BootContextError::MissingRequiredField {
                        field: "rsdp",
                        state: self.rsdp.field_state,
                    });
                }
            }
            BootInitStage::Smp => {
                if !self.ap_trampoline.field_state.is_present() {
                    return Err(BootContextError::MissingRequiredField {
                        field: "ap_trampoline",
                        state: self.ap_trampoline.field_state,
                    });
                }
            }
            BootInitStage::Kaslr => {
                if profile == BootProfile::Host {
                    return Ok(());
                }
                if !self.entropy.field_state.is_present() {
                    return Err(BootContextError::MissingRequiredField {
                        field: "entropy",
                        state: self.entropy.field_state,
                    });
                }
            }
            BootInitStage::Paging | BootInitStage::Heap | BootInitStage::Services => {}
        }
        if stage == BootInitStage::Handover && profile != BootProfile::Host {
            if self.memory_map.field_state != FieldState::PresentValidated
                || self.memory_map.validation != MemoryMapValidation::FullyValidated
                || self.memory_map.normalized.is_none()
                || !self.capabilities.contains(CapabilityFlags::MEMORY_MAP)
            {
                return Err(BootContextError::MissingRequiredField {
                    field: "memory_map",
                    state: self.memory_map.field_state,
                });
            }
        }
        Ok(())
    }

    fn require_nonzero(&self, field: &'static str, value: u64) -> Result<(), BootContextError> {
        if value == 0 {
            Err(BootContextError::MissingRequiredField {
                field,
                state: FieldState::Absent,
            })
        } else {
            Ok(())
        }
    }

    /// C-stili kaynak veriyi kernel-owned buffer'a kopyalar.
    ///
    /// NUL terminator'da durur (C-string semantiği). NUL'dan önceki veri
    /// kapasiteyi aşarsa kısmi kopya YAPILMAZ — açık `CmdlineOverflow`.
    pub fn copy_cmdline_bytes(src: &[u8], dst: &mut [u8]) -> Result<usize, BootContextError> {
        let mut n = 0;
        for &b in src {
            if b == 0 {
                break;
            }
            if n >= dst.len() {
                return Err(BootContextError::CmdlineOverflow {
                    capacity: dst.len(),
                });
            }
            dst[n] = b;
            n += 1;
        }
        Ok(n)
    }

    /// Kaynak baytları buffer'a kopyalar ve durumu günceller.
    ///
    /// Başarı: `PresentValidated` (UTF-8 geçerli ise) veya `Invalid` (geçersiz
    /// UTF-8 — ham baytlar yine de saklanır). Kopya hatası: `Invalid`.
    fn store_cmdline(&mut self, src: &[u8]) {
        match Self::copy_cmdline_bytes(src, &mut self.cmdline_raw) {
            Ok(len) => {
                self.cmdline_len = len;
                self.cmdline_state = if core::str::from_utf8(&self.cmdline_raw[..len]).is_ok() {
                    FieldState::PresentValidated
                } else {
                    FieldState::Invalid
                };
            }
            Err(e) => {
                self.cmdline_len = 0;
                self.cmdline_state = FieldState::Invalid;
                crate::serial_println!("[BOOT] cmdline kopyalama hatası: {:?}", e);
            }
        }
    }

    /// Normalise a UEFI `BootInfo` into a protocol‑agnostic context.
    ///
    /// Takes `&mut` to move the framebuffer out (no clone needed).
    ///
    /// # Safety
    ///
    /// `boot_info` must point to a valid BootInfo whose memory map,
    /// framebuffer, and cmdline pointer are still live.
    #[cfg(target_os = "uefi")]
    pub unsafe fn from_uefi(boot_info: &mut BootInfo) -> Self {
        let mut ctx = Self::base(
            boot_info.physical_memory_offset as u64,
            boot_info.hhdm_offset,
        );
        ctx.rsdp = RsdpCandidate::new(
            boot_info.rsdp_address,
            RsdpAddressKind::Physical,
            RsdpProvenance::Uefi,
        );
        ctx.system_table = boot_info.system_table;
        ctx.runtime_services = boot_info.runtime_services;
        ctx.secure_boot = boot_info.secure_boot;
        ctx.image_hash = boot_info.image_hash;
        ctx.image_size = boot_info.image_size;
        ctx.capabilities.insert(
            CapabilityFlags::SYSTEM_TABLE
                | CapabilityFlags::RUNTIME_SERVICES
                | CapabilityFlags::SECURE_BOOT
                | CapabilityFlags::IMAGE_HASH
                | CapabilityFlags::CMDLINE
                | CapabilityFlags::RSDP,
        );
        // Karar 7: entropy — UEFI RNG protokolü efi_main'de (EBS öncesi)
        // okunup UEFI_ENTROPY_SEED köprüsüne yazılır; EBS sonrası ölüdür.
        if let Some(seed) = crate::boot::UEFI_ENTROPY_SEED.lock().take() {
            ctx.entropy.source = EntropySource::UefiRng;
            ctx.entropy.quality = EntropyQuality::High;
            ctx.entropy.store_seed(&seed);
            ctx.capabilities.insert(CapabilityFlags::ENTROPY);
            crate::serial_println!("[BOOT] UEFI RNG entropy: 32 bayt (High)");
        } else {
            ctx.entropy.source = EntropySource::UefiRng;
            ctx.entropy.field_state = FieldState::Absent;
            crate::serial_println!("[BOOT] UEFI RNG protokolü yok — entropy Absent");
        }
        // Karar 7: firmware güven kanıtı — hash yalnızca ölçülür (doğrulanmaz).
        ctx.firmware_trust = FirmwareTrustEvidence {
            field_state: FieldState::PresentValidated,
            secure_boot: SecureBootStatus::Known(boot_info.secure_boot),
            image_measurement: ImageMeasurementSource::Uefi,
            image_hash: boot_info.image_hash,
            image_hash_present: boot_info.image_size != 0,
            image_size: boot_info.image_size,
        };
        ctx.capabilities.insert(CapabilityFlags::FIRMWARE_TRUST);
        if boot_info.framebuffer.is_some() {
            ctx.capabilities.insert(CapabilityFlags::FRAMEBUFFER);
        }
        ctx.framebuffer = boot_info.framebuffer.take();

        if boot_info.cmdline_ptr != 0 && boot_info.cmdline_len > 0 {
            if boot_info.cmdline_len <= isize::MAX as u64 {
                let ptr = crate::memory::phys_to_virt(boot_info.cmdline_ptr as usize) as *const u8;
                if !ptr.is_null() {
                    let slice = core::slice::from_raw_parts(ptr, boot_info.cmdline_len as usize);
                    ctx.store_cmdline(slice);
                } else {
                    ctx.cmdline_state = FieldState::Invalid;
                    crate::serial_println!("[BOOT] UEFI cmdline pointer null (Invalid)");
                }
            } else {
                ctx.cmdline_state = FieldState::Invalid;
                crate::serial_println!("[BOOT] UEFI cmdline uzunluğu geçersiz (Invalid)");
            }
        } else {
            ctx.cmdline_state = FieldState::Absent;
        }

        ctx
    }

    /// NUL sonlandırmalı C string'i `max_len` üst sınırıyla güvenli okur.
    ///
    /// `max_len`'i aşan okuma yapılmaz: NUL sınır içinde bulunamazsa
    /// `nul_err` döner (sınır kontrolü okumadan ÖNCE yapılır). Pointer null
    /// ise `null_err` döner. Dönen slice kaynak bootloader belleğine
    /// ömür bağlamaz — çağıran kopyalamak zorundadır.
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    fn scan_c_string(
        ptr: *const u8,
        max_len: usize,
        null_err: BootContextError,
        nul_err: BootContextError,
    ) -> Result<&'static [u8], BootContextError> {
        if ptr.is_null() {
            return Err(null_err);
        }
        let mut len = 0usize;
        loop {
            if len > max_len {
                return Err(nul_err);
            }
            let byte = unsafe { *ptr.add(len) };
            if byte == 0 {
                break;
            }
            len += 1;
        }
        Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
    }

    /// Build a context from Limine protocol responses.
    ///
    /// `rsdp_raw` is the RSDP address from `LIMINE_RSDP_REQUEST`; the Limine
    /// protocol returns it as a *physical* address only at base revision 3 and
    /// as a *virtual* (HHDM-linear) address at every other base revision.
    /// `rsdp_virtual` must be `true` exactly when the actual base revision used
    /// by the bootloader is not 3, so the virtual address can be converted back
    /// to physical via the HHDM base. `BootContext::rsdp_address` is always a
    /// physical address across all three boot protocols.
    ///
    /// `cmdline_resp` is the executable-cmdline response; the adapter reads the
    /// raw pointer with NUL scan (bounded by `CMDLINE_BUFFER_LEN`), validates
    /// null pointer / missing terminator / unsupported revision and copies the
    /// bytes into the kernel-owned buffer. `None` (bootloader did not fill the
    /// request — typically base revision < 1) yields `Unsupported`.
    ///
    /// `module_resp` is the module response; the adapter walks the module file
    /// array (spec `Module Feature`), copies each file's `path` (NUL bounded by
    /// `MODULE_NAME_LEN`) into a kernel-owned descriptor and records base/size.
    /// `None` (spec: no response when no modules are available) leaves the list
    /// `Absent`. The Limine protocol spec defines NO entropy feature, so the
    /// entropy field stays absent for this profile (protocol constraint, not a
    /// gap).
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    pub fn from_limine(
        hhdm_offset: u64,
        rsdp_raw: u64,
        rsdp_virtual: bool,
        cmdline_resp: Option<
            &limine_protocol_for_rust::requests::executable_cmdline::ExecutableCmdlineResponse,
        >,
        module_resp: Option<&LimineModuleResponseView>,
    ) -> Self {
        let rsdp_address = if rsdp_virtual && rsdp_raw != 0 {
            rsdp_raw.saturating_sub(hhdm_offset)
        } else {
            rsdp_raw
        };
        let mut ctx = Self::base(hhdm_offset, hhdm_offset);
        ctx.rsdp = RsdpCandidate::new(
            rsdp_address,
            RsdpAddressKind::Physical,
            RsdpProvenance::Limine,
        );
        ctx.capabilities
            .insert(CapabilityFlags::RSDP | CapabilityFlags::CMDLINE);
        match cmdline_resp {
            None => {
                ctx.cmdline_state = FieldState::Unsupported;
                crate::serial_println!(
                    "[BOOT] Limine cmdline request tanınmadı (base revision < 1 olabilir)"
                );
            }
            Some(resp) => match Self::read_limine_cmdline(resp) {
                Ok(src) => ctx.store_cmdline(src),
                Err(e) => {
                    ctx.cmdline_state = FieldState::Invalid;
                    crate::serial_println!("[BOOT] Limine cmdline doğrulama hatası: {:?}", e);
                }
            },
        }
        // Karar 7 kapanışı (Limine): modüller spec-doğrulanmış ABI ile
        // kopyalanır; entropy ise Limine protokolünde TANIMLANMAMIŞTIR
        // (spec'te entropy/kaslr feature'ı yoktur) — bu yüzden ENTROPY
        // capability set edilmez ve alan Absent kalır. Bu bir protokol
        // kısıtıdır, kapatılacak açık değildir.
        crate::serial_println!("[BOOT] Limine entropy kanalı yok (protokol spec'te tanımsız)");
        ctx.capabilities.insert(CapabilityFlags::MODULES);
        match module_resp {
            None => {
                // Spec: "If no modules are available, no response will be provided."
                crate::serial_println!("[BOOT] Limine modül yanıtı yok (modül yüklenmedi)");
            }
            Some(resp) => {
                for idx in 0..resp.module_count {
                    match Self::read_limine_module_file(resp, idx as usize) {
                        Ok(desc) => ctx.modules.modules.push(desc),
                        Err(e) => {
                            ctx.modules.field_state = FieldState::Invalid;
                            ctx.modules.modules.clear();
                            crate::serial_println!(
                                "[BOOT] Limine modül descriptor hatası: {:?}",
                                e
                            );
                            break;
                        }
                    }
                }
                if !ctx.modules.modules.is_empty() {
                    ctx.modules.field_state = FieldState::PresentValidated;
                    crate::serial_println!(
                        "[BOOT] Limine modül descriptor: {} adet (kernel-owned ad, rev={})",
                        ctx.modules.modules.len(),
                        resp.revision
                    );
                }
            }
        }
        ctx
    }

    /// Imports the first Limine RGB framebuffer into the canonical context.
    ///
    /// Limine's Framebuffer Feature is optional: no response means that the
    /// protocol supplied no display surface and the caller must keep the text
    /// fallback.  The response layout and RGB masks are validated against the
    /// archived Limine protocol (`Framebuffer Feature`, `struct
    /// limine_framebuffer`) before publishing the capability.  The renderer
    /// writes `u32` pixels, so only the linear 32-bpp RGB layout is accepted.
    /// `hhdm_offset` is used when the response carries a physical address; an
    /// already-HHDM-mapped virtual address is left unchanged.
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    pub fn install_limine_framebuffer(
        &mut self,
        response: Option<&limine_protocol_for_rust::requests::framebuffer::FramebufferResponse>,
        hhdm_offset: u64,
    ) -> bool {
        let Some(response) = response else {
            self.clear_framebuffer();
            crate::serial_println!("[FRAMEBUFFER] Limine response absent");
            return false;
        };
        let framebuffers = response.get_framebuffers();
        let Some(raw) = framebuffers.iter().find(|fb| {
            fb.memory_model == 1
                && fb.bpp == 32
                && fb.red_mask_size == 8
                && fb.green_mask_size == 8
                && fb.blue_mask_size == 8
                && fb.red_mask_shift == 16
                && fb.green_mask_shift == 8
                && fb.blue_mask_shift == 0
        }) else {
            self.clear_framebuffer();
            crate::serial_println!(
                "[FRAMEBUFFER] Limine response has no supported linear RGB32 surface"
            );
            return false;
        };

        let accepted = self.install_raw_framebuffer(
            raw.address as u64,
            hhdm_offset,
            raw.width,
            raw.height,
            raw.pitch,
            raw.bpp,
            raw.memory_model,
            raw.red_mask_size,
            raw.red_mask_shift,
            raw.green_mask_size,
            raw.green_mask_shift,
            raw.blue_mask_size,
            raw.blue_mask_shift,
        );
        if accepted {
            crate::serial_println!(
                "[FRAMEBUFFER] Limine RGB32 {}x{} pitch={} address={:#x}",
                raw.width,
                raw.height,
                raw.pitch,
                self.framebuffer
                    .as_ref()
                    .map(|fb| fb.base_addr)
                    .unwrap_or(0)
            );
        }
        accepted
    }

    /// Imports the Multiboot2 framebuffer information tag into the canonical
    /// context.  MB2 framebuffer addresses are physical by specification, so
    /// the synthesized HHDM offset is always applied by the shared importer.
    /// Indexed and EGA text tags are intentionally rejected because the
    /// kernel's drawing contract is a 32-bpp linear RGB surface.
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    pub fn install_multiboot2_framebuffer(
        &mut self,
        tag: Option<&multiboot2::FramebufferTag<'_>>,
        hhdm_offset: u64,
    ) -> bool {
        let Some(tag) = tag else {
            self.clear_framebuffer();
            crate::serial_println!("[FRAMEBUFFER] Multiboot2 framebuffer tag absent");
            return false;
        };
        let multiboot2::FramebufferType::RGB { red, green, blue } = &tag.buffer_type else {
            self.clear_framebuffer();
            crate::serial_println!(
                "[FRAMEBUFFER] Multiboot2 framebuffer tag is indexed/text, not RGB32"
            );
            return false;
        };
        let accepted = self.install_raw_framebuffer(
            tag.address,
            hhdm_offset,
            tag.width as u64,
            tag.height as u64,
            tag.pitch as u64,
            tag.bpp as u16,
            1,
            red.size,
            red.position,
            green.size,
            green.position,
            blue.size,
            blue.position,
        );
        if accepted {
            crate::serial_println!(
                "[FRAMEBUFFER] Multiboot2 RGB32 {}x{} pitch={} address={:#x}",
                tag.width,
                tag.height,
                tag.pitch,
                self.framebuffer
                    .as_ref()
                    .map(|fb| fb.base_addr)
                    .unwrap_or(0)
            );
        }
        accepted
    }

    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    fn install_raw_framebuffer(
        &mut self,
        address: u64,
        hhdm_offset: u64,
        width: u64,
        height: u64,
        pitch: u64,
        bpp: u16,
        memory_model: u8,
        red_size: u8,
        red_shift: u8,
        green_size: u8,
        green_shift: u8,
        blue_size: u8,
        blue_shift: u8,
    ) -> bool {
        self.clear_framebuffer();
        if address == 0
            || memory_model != 1
            || bpp != 32
            || red_size != 8
            || green_size != 8
            || blue_size != 8
            || red_shift != 16
            || green_shift != 8
            || blue_shift != 0
        {
            return false;
        }
        let mapped_address = if hhdm_offset != 0 && address < hhdm_offset {
            match hhdm_offset.checked_add(address) {
                Some(mapped) => mapped,
                None => return false,
            }
        } else {
            address
        };
        let base_addr = match usize::try_from(mapped_address) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let width = match usize::try_from(width) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let height = match usize::try_from(height) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let pitch = match usize::try_from(pitch) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let Some(framebuffer) =
            (unsafe { Framebuffer::from_raw_parts(base_addr, width, height, pitch, bpp) })
        else {
            return false;
        };
        self.framebuffer = Some(framebuffer);
        self.capabilities.insert(CapabilityFlags::FRAMEBUFFER);
        true
    }

    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    fn clear_framebuffer(&mut self) {
        self.framebuffer = None;
        self.capabilities = self.capabilities & !CapabilityFlags::FRAMEBUFFER;
    }

    /// Limine cmdline response'unu güvenli şekilde okur.
    ///
    /// `ExecutableCmdlineResponse` alanları private olduğundan aynı `repr(C)`
    /// düzene sahip bir view üzerinden okunur: `revision` @0, `cmdline` @8.
    /// Bu layout, crate'in `#[repr(C, align(8))]` sözleşmesine bağlıdır
    /// (limine-protocol-for-rust 0.2.1, pinned).
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    fn read_limine_cmdline(
        resp: &limine_protocol_for_rust::requests::executable_cmdline::ExecutableCmdlineResponse,
    ) -> Result<&'static [u8], BootContextError> {
        #[repr(C, align(8))]
        struct CmdlineResponseView {
            revision: u64,
            cmdline: *const u8,
        }
        let view: &CmdlineResponseView = unsafe {
            &*(resp as *const limine_protocol_for_rust::requests::executable_cmdline::ExecutableCmdlineResponse
                as *const CmdlineResponseView)
        };
        if view.revision < 1 {
            return Err(BootContextError::UnsupportedRevision {
                min: 1,
                actual: view.revision,
            });
        }
        Self::scan_c_string(
            view.cmdline,
            CMDLINE_BUFFER_LEN,
            BootContextError::CmdlineNullPointer,
            BootContextError::CmdlineMissingNulTerminator,
        )
    }

    /// Limine module response'undan `idx`'inci modülü güvenli okur.
    ///
    /// `modules` pointer dizisinin her öğesi bir `limine_file` pointer'ıdır;
    /// descriptor adı `path`'ten alınır (NUL sınırlı, `MODULE_NAME_LEN`).
    /// Adres/size doğrudan kopyalanır (spec: adres en az 4KiB hizalıdır ve
    /// modül kendi 4KiB chunk'larına tek başına sahiptir). Sözleşme ihlali
    /// (null dizi öğesi / null path / NUL'suz path) açık hata döndürür.
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    fn read_limine_module_file(
        resp: &LimineModuleResponseView,
        idx: usize,
    ) -> Result<ModuleDescriptor, BootContextError> {
        let file_ptr = unsafe { *resp.modules.add(idx) };
        if file_ptr.is_null() {
            return Err(BootContextError::ModuleNullPointer);
        }
        let file = unsafe { &*file_ptr };
        let name = Self::scan_c_string(
            file.path,
            MODULE_NAME_LEN,
            BootContextError::ModuleNullPointer,
            BootContextError::ModuleNameMissingNulTerminator,
        )?;
        ModuleDescriptor::new(name, file.address, file.size, 0)
    }

    /// Build a context from a Multiboot2 info structure.
    ///
    /// `physical_memory_offset` is set from the kernel's compile‑time
    /// constant because MB2 provides no HHDM. The cmdline tag (C-style UTF-8,
    /// NUL-terminated) is copied into the kernel-owned buffer; the RSDP is
    /// taken from the ACPI old/new RSDP tags (types 14/15). Each tag carries a
    /// verbatim copy of the RSDP right after its 8‑byte tag header
    /// (`typ` + `size`), so the copy's address is `tag + 8`. The boot info
    /// structure is read through the identity mapping, therefore the tag
    /// pointer value is also the copy's *physical* address.
    #[cfg(all(not(target_os = "uefi"), not(target_os = "windows")))]
    pub fn from_multiboot2(info: &multiboot2::BootInformation) -> Self {
        let rsdp_address = info
            .rsdp_v2_tag()
            .map(|tag| tag as *const _ as u64 + 8)
            .or_else(|| info.rsdp_v1_tag().map(|tag| tag as *const _ as u64 + 8))
            .unwrap_or(0);
        // MB2 does not advertise an HHDM feature, therefore the kernel's
        // compile-time physical offset is the synthesized canonical mapping.
        let physical_offset = crate::memory::PHYSICAL_MEMORY_OFFSET;
        let mut ctx = Self::base(physical_offset, physical_offset);
        ctx.rsdp = RsdpCandidate::new(
            rsdp_address,
            RsdpAddressKind::Physical,
            RsdpProvenance::Multiboot2,
        );
        ctx.capabilities
            .insert(CapabilityFlags::RSDP | CapabilityFlags::CMDLINE | CapabilityFlags::HHDM);
        if let Some(tag) = info.command_line_tag() {
            // `command_line()` NUL dahil döner; copy NUL'da durur.
            ctx.store_cmdline(tag.command_line().as_bytes());
        }
        // Modül/initrd descriptor kopyaları (kernel-owned adlar). MB2 bir
        // entropy/güven kanalı tanımlamadığından bunlar Absent/Unsupported kalır.
        ctx.capabilities.insert(CapabilityFlags::MODULES);
        for tag in info.module_tags() {
            match ModuleDescriptor::new(
                tag.cmdline().as_bytes(),
                tag.start_address() as u64,
                tag.module_size() as u64,
                0,
            ) {
                Ok(desc) => ctx.modules.modules.push(desc),
                Err(e) => {
                    ctx.modules.field_state = FieldState::Invalid;
                    ctx.modules.modules.clear();
                    crate::serial_println!("[BOOT] MB2 modül descriptor hatası: {:?}", e);
                    break;
                }
            }
        }
        if !ctx.modules.modules.is_empty() {
            ctx.modules.field_state = FieldState::PresentValidated;
            crate::serial_println!(
                "[BOOT] MB2 modül descriptor: {} adet (kernel-owned ad)",
                ctx.modules.modules.len()
            );
        }
        ctx
    }
}

// ============================================================================
// HOST VALIDATION TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BootContext {
        BootContext::base(0, 0)
    }

    fn publish_test_map(ctx: &mut BootContext) {
        let mut map = NormalizedMemoryMap::empty();
        map.push(MemoryRegion {
            base: 0x1000,
            len: 0x20_000,
            kind: MemoryRegionKind::Usable,
        })
        .unwrap();
        assert!(ctx.publish_normalized_memory_map(map));
    }

    #[test]
    fn cmdline_copy_plain() {
        let mut dst = [0u8; CMDLINE_BUFFER_LEN];
        let n = BootContext::copy_cmdline_bytes(b"boot_tests=1", &mut dst).unwrap();
        assert_eq!(n, 12);
        assert_eq!(&dst[..n], b"boot_tests=1");
    }

    #[test]
    fn cmdline_copy_stops_at_nul() {
        let mut dst = [0u8; CMDLINE_BUFFER_LEN];
        let src = b"kernel vmlinuz\0ignored";
        let n = BootContext::copy_cmdline_bytes(src, &mut dst).unwrap();
        assert_eq!(n, 14);
        assert_eq!(&dst[..n], b"kernel vmlinuz");
    }

    #[test]
    fn cmdline_copy_overflow_is_explicit_error() {
        let mut dst = [0u8; 8];
        let err = BootContext::copy_cmdline_bytes(b"0123456789", &mut dst).unwrap_err();
        assert_eq!(err, BootContextError::CmdlineOverflow { capacity: 8 });
    }

    #[test]
    fn cmdline_copy_empty() {
        let mut dst = [0u8; CMDLINE_BUFFER_LEN];
        let n = BootContext::copy_cmdline_bytes(b"", &mut dst).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn store_valid_utf8_marks_validated() {
        let mut ctx = base();
        ctx.store_cmdline(b"boot_tests=1");
        assert_eq!(ctx.cmdline_state(), FieldState::PresentValidated);
        assert_eq!(ctx.cmdline_str(), Some("boot_tests=1"));
    }

    #[test]
    fn store_invalid_utf8_keeps_raw_bytes() {
        let mut ctx = base();
        ctx.store_cmdline(&[0xFF, 0xFE, 0x00]);
        assert_eq!(ctx.cmdline_state(), FieldState::Invalid);
        assert_eq!(ctx.cmdline_str(), None);
        assert_eq!(ctx.cmdline_bytes(), &[0xFF, 0xFE]);
    }

    #[test]
    fn store_overflow_marks_invalid() {
        let mut ctx = base();
        ctx.store_cmdline(&[b'x'; CMDLINE_BUFFER_LEN + 32]);
        assert_eq!(ctx.cmdline_state(), FieldState::Invalid);
        assert_eq!(ctx.cmdline_len, 0);
        assert_eq!(ctx.cmdline_str(), None);
    }

    #[test]
    fn negotiate_reports_missing() {
        let mut ctx = base();
        ctx.capabilities.insert(CapabilityFlags::RSDP);
        let neg = ctx.negotiate(CapabilityFlags::RSDP | CapabilityFlags::CMDLINE);
        assert!(!neg.supported());
        assert_eq!(neg.missing, CapabilityFlags::CMDLINE);
    }

    #[test]
    fn negotiate_full_match() {
        let mut ctx = base();
        ctx.capabilities
            .insert(CapabilityFlags::RSDP | CapabilityFlags::CMDLINE);
        let neg = ctx.negotiate(CapabilityFlags::RSDP);
        assert!(neg.supported());
        assert_eq!(neg.missing.bits(), 0);
    }

    #[test]
    fn stage_gate_uefi_requires_system_table() {
        let err = base().validate_for_stage(BootProfile::Uefi, BootInitStage::Handover);
        assert!(matches!(
            err,
            Err(BootContextError::MissingRequiredField {
                field: "system_table",
                ..
            })
        ));
    }

    #[test]
    fn stage_gate_uefi_ok_when_fields_present() {
        let mut ctx = base();
        ctx.system_table = 0x1000;
        ctx.runtime_services = 0x2000;
        publish_test_map(&mut ctx);
        assert!(ctx
            .validate_for_stage(BootProfile::Uefi, BootInitStage::Handover)
            .is_ok());
    }

    #[test]
    fn stage_gate_limine_requires_hhdm_and_rsdp() {
        let mut ctx = base();
        ctx.rsdp = RsdpCandidate::new(0x1000, RsdpAddressKind::Physical, RsdpProvenance::Limine);
        let err = ctx.validate_for_stage(BootProfile::Limine, BootInitStage::Handover);
        assert!(matches!(
            err,
            Err(BootContextError::MissingRequiredField {
                field: "hhdm_offset",
                ..
            })
        ));
        ctx.hhdm_offset = 0xFFFF800000000000;
        publish_test_map(&mut ctx);
        assert!(ctx
            .validate_for_stage(BootProfile::Limine, BootInitStage::Handover)
            .is_ok());
    }

    #[test]
    fn stage_gate_multiboot_requires_rsdp() {
        let err = base().validate_for_stage(BootProfile::Multiboot2, BootInitStage::Handover);
        assert!(matches!(
            err,
            Err(BootContextError::MissingRequiredField { field: "rsdp", .. })
        ));
        let mut ctx = base();
        ctx.rsdp = RsdpCandidate::new(
            0x1000,
            RsdpAddressKind::Physical,
            RsdpProvenance::Multiboot2,
        );
        publish_test_map(&mut ctx);
        assert!(ctx
            .validate_for_stage(BootProfile::Multiboot2, BootInitStage::Handover)
            .is_ok());
    }

    #[test]
    fn stage_gate_host_always_ok() {
        assert!(base()
            .validate_for_stage(BootProfile::Host, BootInitStage::Handover)
            .is_ok());
    }

    #[test]
    fn stage_gate_acpi_requires_validated_rsdp() {
        let mut ctx = base();
        ctx.rsdp = RsdpCandidate::new(0x1000, RsdpAddressKind::Physical, RsdpProvenance::Uefi);
        assert!(ctx
            .validate_for_stage(BootProfile::Uefi, BootInitStage::Acpi)
            .is_err());
        ctx.rsdp.field_state = FieldState::PresentValidated;
        ctx.rsdp.acpi_revision = 2;
        assert!(ctx
            .validate_for_stage(BootProfile::Uefi, BootInitStage::Acpi)
            .is_ok());
    }

    #[test]
    fn rsdp_candidate_defaults_untrusted() {
        let c = RsdpCandidate::new(0x1234, RsdpAddressKind::Physical, RsdpProvenance::Limine);
        assert_eq!(c.field_state, FieldState::PresentUntrusted);
        assert_eq!(c.acpi_revision, 0);
    }

    #[test]
    fn memory_map_unavailable_by_default() {
        let ctx = base();
        assert_eq!(ctx.memory_map.field_state, FieldState::Absent);
        assert!(ctx.memory_map.normalized.is_none());
        assert!(!ctx.capabilities.contains(CapabilityFlags::MEMORY_MAP));
    }

    #[test]
    fn boot_context_version_is_v2() {
        assert_eq!(base().version, 2);
    }

    #[test]
    fn entropy_absent_by_default() {
        let ctx = base();
        assert_eq!(ctx.entropy.field_state, FieldState::Absent);
        assert_eq!(ctx.entropy.seed, [0u8; 32]);
        assert!(!ctx.capabilities.contains(CapabilityFlags::ENTROPY));
    }

    #[test]
    fn entropy_store_seed_marks_validated() {
        let mut state = EntropyState::absent();
        state.source = EntropySource::UefiRng;
        state.quality = EntropyQuality::High;
        let seed = [0xAAu8; 32];
        state.store_seed(&seed);
        assert_eq!(state.field_state, FieldState::PresentValidated);
        assert_eq!(state.seed, seed);
    }

    #[test]
    fn entropy_wrong_len_marks_invalid() {
        let mut state = EntropyState::absent();
        state.store_seed(&[0u8; 16]);
        assert_eq!(state.field_state, FieldState::Invalid);
        assert_eq!(state.seed, [0u8; 32]);
    }

    #[test]
    fn ap_trampoline_absent_by_default() {
        let ctx = base();
        assert_eq!(ctx.ap_trampoline.field_state, FieldState::Absent);
        assert_eq!(ctx.ap_trampoline.ownership, TrampolineOwnership::None);
        assert!(!ctx.capabilities.contains(CapabilityFlags::AP_TRAMPOLINE));
    }

    #[test]
    fn ap_trampoline_kernel_owned_marks_synthesized() {
        let t = ApTrampolineState::kernel_owned(0x1000, 4096, 16);
        assert_eq!(t.field_state, FieldState::Synthesized);
        assert_eq!(t.ownership, TrampolineOwnership::Kernel);
        assert_eq!(t.base, 0x1000);
        assert_eq!(t.range_len, 4096);
        assert_eq!(t.alignment, 16);
    }

    #[test]
    fn stage_gate_smp_requires_trampoline() {
        let err = base().validate_for_stage(BootProfile::Multiboot2, BootInitStage::Smp);
        assert!(matches!(
            err,
            Err(BootContextError::MissingRequiredField {
                field: "ap_trampoline",
                ..
            })
        ));
        let mut ctx = base();
        ctx.ap_trampoline = ApTrampolineState::kernel_owned(0x1000, 4096, 16);
        assert!(ctx
            .validate_for_stage(BootProfile::Multiboot2, BootInitStage::Smp)
            .is_ok());
    }

    #[test]
    fn stage_gate_kaslr_requires_entropy() {
        let err = base().validate_for_stage(BootProfile::Uefi, BootInitStage::Kaslr);
        assert!(matches!(
            err,
            Err(BootContextError::MissingRequiredField {
                field: "entropy",
                ..
            })
        ));
        let mut ctx = base();
        ctx.entropy.store_seed(&[0xBBu8; 32]);
        assert!(ctx
            .validate_for_stage(BootProfile::Uefi, BootInitStage::Kaslr)
            .is_ok());
        // Developer (Host) profili deterministik fallback'e izin verir.
        assert!(base()
            .validate_for_stage(BootProfile::Host, BootInitStage::Kaslr)
            .is_ok());
    }

    #[test]
    fn cpu_snapshot_synthesized_on_real_cpu() {
        let snap = CpuFeatureSnapshot::synthesize();
        assert_eq!(snap.field_state, FieldState::Synthesized);
        // Host x86-64'tür: vendor bilinmeli, SSE2 taban çizgisidir.
        assert_ne!(snap.vendor, CpuVendor::Unknown);
        assert!(snap.max_std_level >= 1);
        assert!(snap.features.contains(CpuFeatureFlags::SSE2));
        assert!(snap.features.contains(CpuFeatureFlags::SSE));
    }

    #[test]
    fn pre_cpu_features_capability_set_in_base() {
        let ctx = base();
        assert!(ctx.capabilities.contains(CapabilityFlags::PRE_CPU_FEATURES));
    }

    #[test]
    fn firmware_trust_unavailable_for_non_uefi() {
        let ctx = base();
        assert_eq!(ctx.firmware_trust.field_state, FieldState::Unsupported);
        assert_eq!(
            ctx.firmware_trust.secure_boot,
            SecureBootStatus::Unsupported
        );
        assert!(!ctx.capabilities.contains(CapabilityFlags::FIRMWARE_TRUST));
    }

    #[test]
    fn firmware_trust_known_maps_tri_state() {
        let trust = FirmwareTrustEvidence {
            field_state: FieldState::PresentValidated,
            secure_boot: SecureBootStatus::Known(true),
            image_measurement: ImageMeasurementSource::Uefi,
            image_hash: [0x01u8; 32],
            image_hash_present: true,
            image_size: 0x1234,
        };
        assert_eq!(trust.secure_boot, SecureBootStatus::Known(true));
        assert!(trust.image_hash_present);
        assert_ne!(trust.image_hash, [0u8; 32]);
    }

    #[test]
    fn modules_store_name_and_range() {
        let desc = ModuleDescriptor::new(b"initrd.img", 0x100_000, 4096, 0).unwrap();
        assert_eq!(desc.name(), b"initrd.img");
        assert_eq!(desc.base, 0x100_000);
        assert_eq!(desc.len, 4096);
    }

    #[test]
    fn modules_overflow_is_explicit_error() {
        let long: Vec<u8> = (0..MODULE_NAME_LEN + 1)
            .map(|i| (i % 26) as u8 + b'a')
            .collect();
        let err = ModuleDescriptor::new(&long, 0, 0, 0).unwrap_err();
        assert_eq!(
            err,
            BootContextError::ModuleNameOverflow {
                capacity: MODULE_NAME_LEN
            }
        );
    }

    #[test]
    fn capability_bits_9_to_13_occupied() {
        assert_eq!(CapabilityFlags::ENTROPY.bits(), 1 << 9);
        assert_eq!(CapabilityFlags::AP_TRAMPOLINE.bits(), 1 << 10);
        assert_eq!(CapabilityFlags::PRE_CPU_FEATURES.bits(), 1 << 11);
        assert_eq!(CapabilityFlags::FIRMWARE_TRUST.bits(), 1 << 12);
        assert_eq!(CapabilityFlags::MODULES.bits(), 1 << 13);
        assert_eq!(CapabilityFlags::HHDM.bits(), 1 << 14);
        assert_eq!(CapabilityFlags::RUNTIME_VERIFIED.bits(), 1 << 15);
        assert_eq!(CapabilityFlags::REBOOT_SAFE.bits(), 1 << 16);
        assert_eq!(CapabilityFlags::SMBIOS.bits(), 1 << 17);
    }
}
