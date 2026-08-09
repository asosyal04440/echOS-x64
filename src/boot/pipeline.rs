//! # Ortak Boot Pipeline — Kanonik Faz Makinesi (Wave 2)
//!
//! Üç boot yolunun (UEFI, Limine, Multiboot2) normalize edilmiş ortak init
//! sırasını A–J fazları üzerinden yöneten TEK kanonik faz modeli.
//!
//! ## Sahiplik (karar #1)
//!
//! - `boot::pipeline::BootPhase` — faz kimliklerinin kanonik tipi.
//!   `safety::BootPhase` yoktur; `safety.rs` yalnızca bu tipe delegasyon yapar.
//! - Faz kimliği (`PhaseId`) ile yürütme sırası AYRIDIR: kimlik değerleri
//!   kalıcı/stabil (legacy değerler korunur), yürütme geçerliliği ise
//!   numeric karşılaştırma ile DEĞİL, `ALLOWED_SUCCESSORS` geçiş tablosuyla
//!   (Teslim 2 gerçek dependency DAG'i) belirlenir.
//! - Faz yalnızca "entered" olmaz: `begin` / `complete` / `degraded` /
//!   `skipped` / `fail` / `recovery` / `halt` yaşam döngüsüne sahiptir.
//!
//! ## State word encoding (tek authoritative state, doküman — teslimat 3)
//!
//! ```text
//! u32 state word:
//!   bits  0..=7  : phase id      (BootPhase `as u8`)
//!   bits  8..=15 : phase state   (PhaseState `as u8`)
//!   bits 16..=23 : monotonic boot event sequence (8-bit; boot ≤ 64 olay — taşma belgeli)
//!   bits 24..=25 : boot protocol (BootProtocol `as u8`, 2 bit)
//!   bits 26..=31 : reserved — ZORUNLU 0; sıfırdan farklıysa ReservedBitsCorrupt
//! ```
//!
//! Word, `crate::boot::pipeline::BOOT_PIPELINE` içinde yaşar (eski
//! `BOOT_SAFETY.current_phase` buraya TAŞINMIŞTIR — yeni global eklenmedi;
//! mevcut state taşındı, mandate kural 7). `safety.rs` okuma tarafında
//! delegasyon yapar; yazma yalnızca bu modülün CAS geçişleriyle yapılır.
//!
//! ## Atomic ordering gerekçesi (doküman — teslimat 4)
//!
//! - Boot boyunca geçişlerin TEK writer'ı BSP'dir. AP'ler, diagnostic
//!   reader'lar ve fault handler'lar yalnızca OKUR.
//! - Her geçiş tek RMW'dir: `compare_exchange(..., AcqRel, Acquire)`.
//!   - Acquire (CAS başarısı): önceki geçişlerin Release yayınlarını
//!     (faz içinde yazılan veriler) okumadan önce görünür kılar.
//!   - Release (CAS yayını): faz sırasında yazılan verileri sonraki Acquire
//!     okuyucularına yayınlar.
//! - Reader'lar `Acquire` yük kullanır. Tek writer olduğundan yazma sırası
//!   totaldir; son CAS her zaman görülür. `Relaxed` word üzerinde asla
//!   kullanılmaz (kör store yasak — karar 1.3). `SeqCst` yalnızca gerçek
//!   cross-domain sıralama gerektiren yerlerde kalır (safety sayaçları —
//!   değişmez legacy); makine word'ünde SeqCst GEREKMEZ: tek writer + RMW
//!   atomikliği yeterlidir (x86 TSO ek garantidir, gerekçe mimariye bağlı
//!   değildir).
//! - Sequence sayacı 8-bit'tir ve yalnızca teşhis sıralaması içindir;
//!   doğruluk kararına katılmaz.
//!
//! ## RecoveryOnly (karar #2 — Wave 2'de tam, minimal, allocation-free)
//!
//! Heap kurulumu başarısızsa normal pipeline devam edemez:
//!
//! ```text
//! HeapInit/Running --> fail_phase --> HeapInit/Failed
//!   --> enter_recovery --> RecoveryOnly/Running
//!   --> teşhis (stack-only) --> Halted veya ControlledReboot
//! ```
//!
//! RecoveryOnly: heap tahsisi YOK, `Vec/String/Box` YOK, normal pipeline'a
//! dönüş YOK, scheduler/userspace/driver başlatma YOK, başarısız fazı
//! yeniden çalıştırma YOK, doğrulanmamış firmware/runtime'a kör güven YOK,
//! idempotent, çağırana geri dönmez. Yalnız önceden ayrılmış/stack/static
//! altyapılar kullanılır.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use crate::boot::context::{BootProfile, CapabilityFlags};
use crate::boot::safety::ViolationType;

#[repr(align(64))]
struct CacheAligned<T>(T);

impl<T> core::ops::Deref for CacheAligned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ============================================================================
// KANONİK FAZ KİMLİKLERİ
// ============================================================================

/// Faz kimliği — kalıcı, stabil `PhaseId` değerleri.
///
/// Legacy değerler (0–10, 255) KORUNUR (error_counts/telemetry/persisted
/// teşhis uyumluluğu — karar 1.1); yeni fazlar boş aralıktan (11–15) alınır.
/// Yürütme sırası numeric değerle değil `ALLOWED_SUCCESSORS` ile belirlenir.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootPhase {
    /// Başlangıç durumu.
    Reset = 0,
    /// A — BootContext: adapter handover + normalize + validate.
    UefiHandover = 1,
    /// C — MemoryOwnership: PMM → global memory manager.
    MemoryInit = 2,
    /// B — MemoryLayout: init_paging + HHDM + fb/APIC/HPET MMIO eşleme.
    PagingSetup = 3,
    /// C-2 — TLSF heap kurulumu (karar 1.5: artık görünür).
    HeapInit = 4,
    /// D — CorePrivileges: gdt → syscall → cpu → security.
    GdtSetup = 5,
    /// E — InterruptFoundation: interrupts::init → vdso → tty.
    IdtSetup = 6,
    /// F — PlatformServices: ACPI/IOMMU + memory subsystems.
    AcpiInit = 7,
    /// H — Multiprocessing: smp → topology → power → numa → affinity → hotplug.
    SmpInit = 8,
    /// J-1 — Services: driver katmanı + VFS mount + init system.
    DriverInit = 9,
    /// J sonu — userspace kabulü.
    UserspaceReady = 10,
    /// G — Scheduling: scheduler → kick_irq_worker → workers → reclaim (SMP'den ÖNCE).
    Scheduling = 11,
    /// I — InterruptEnable: fault::init → interrupts::enable → mark_bsp_init_complete.
    InterruptEnable = 12,
    /// J-2 — Services: appliance → boot_self_check → diagnostics → compositor/shell.
    Services = 13,
    /// RecoveryOnly containment fazı (karar #2).
    RecoveryOnly = 14,
    /// Terminal faz: kontrollü durma.
    Halted = 15,
    /// Terminal sentinel (legacy).
    Running = 255,
}

impl BootPhase {
    /// Stabil faz kimliği.
    pub const fn phase_id(self) -> u8 {
        self as u8
    }

    /// Teşhis için faz adı.
    pub const fn name(self) -> &'static str {
        match self {
            BootPhase::Reset => "reset",
            BootPhase::UefiHandover => "boot-context",
            BootPhase::MemoryInit => "memory-ownership",
            BootPhase::PagingSetup => "memory-layout",
            BootPhase::HeapInit => "heap-init",
            BootPhase::GdtSetup => "core-privileges",
            BootPhase::IdtSetup => "interrupt-foundation",
            BootPhase::AcpiInit => "platform-services",
            BootPhase::SmpInit => "multiprocessing",
            BootPhase::DriverInit => "services-drivers",
            BootPhase::UserspaceReady => "userspace-ready",
            BootPhase::Scheduling => "scheduling",
            BootPhase::InterruptEnable => "interrupt-enable",
            BootPhase::Services => "services",
            BootPhase::RecoveryOnly => "recovery-only",
            BootPhase::Halted => "halted",
            BootPhase::Running => "running",
        }
    }

    /// Terminal fazlar: bu fazlardan sonra geçiş yoktur.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            BootPhase::RecoveryOnly | BootPhase::Halted | BootPhase::Running
        )
    }
}

impl Default for BootPhase {
    fn default() -> Self {
        BootPhase::Reset
    }
}

impl TryFrom<u8> for BootPhase {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BootPhase::Reset),
            1 => Ok(BootPhase::UefiHandover),
            2 => Ok(BootPhase::MemoryInit),
            3 => Ok(BootPhase::PagingSetup),
            4 => Ok(BootPhase::HeapInit),
            5 => Ok(BootPhase::GdtSetup),
            6 => Ok(BootPhase::IdtSetup),
            7 => Ok(BootPhase::AcpiInit),
            8 => Ok(BootPhase::SmpInit),
            9 => Ok(BootPhase::DriverInit),
            10 => Ok(BootPhase::UserspaceReady),
            11 => Ok(BootPhase::Scheduling),
            12 => Ok(BootPhase::InterruptEnable),
            13 => Ok(BootPhase::Services),
            14 => Ok(BootPhase::RecoveryOnly),
            15 => Ok(BootPhase::Halted),
            255 => Ok(BootPhase::Running),
            _ => Err(()),
        }
    }
}

/// Faz yaşam döngüsü durumu (karar 1.2).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseState {
    NotStarted = 0,
    Running = 1,
    Completed = 2,
    Degraded = 3,
    Skipped = 4,
    Failed = 5,
}

impl PhaseState {
    pub const fn name(self) -> &'static str {
        match self {
            PhaseState::NotStarted => "not-started",
            PhaseState::Running => "running",
            PhaseState::Completed => "completed",
            PhaseState::Degraded => "degraded",
            PhaseState::Skipped => "skipped",
            PhaseState::Failed => "failed",
        }
    }
}

/// Boot protokolü (word'ün 24..=25 bitleri, 2 bit).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootProtocol {
    None = 0,
    Uefi = 1,
    Limine = 2,
    Multiboot2 = 3,
}

impl BootProtocol {
    pub const fn name(self) -> &'static str {
        match self {
            BootProtocol::None => "none",
            BootProtocol::Uefi => "uefi",
            BootProtocol::Limine => "limine",
            BootProtocol::Multiboot2 => "multiboot2",
        }
    }
}

/// Verdict (Teslim 5): faz makinesi hata politikası çıktısı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    Stop,
}

// ============================================================================
// YETENEK SETİ (capability bitflags — Teslim 3/7)
// ============================================================================

pub mod caps {
    use crate::boot::context::CapabilityFlags;

    pub const MEMORY_MAP: CapabilityFlags = CapabilityFlags::MEMORY_MAP;
    pub const HHDM: CapabilityFlags = CapabilityFlags::HHDM;
    pub const RSDP: CapabilityFlags = CapabilityFlags::RSDP;
    pub const CMDLINE: CapabilityFlags = CapabilityFlags::CMDLINE;
    pub const FRAMEBUFFER: CapabilityFlags = CapabilityFlags::FRAMEBUFFER;
    pub const SECURE_BOOT: CapabilityFlags = CapabilityFlags::SECURE_BOOT;
    pub const RUNTIME_SERVICES: CapabilityFlags = CapabilityFlags::RUNTIME_SERVICES;
    pub const RUNTIME_VERIFIED: CapabilityFlags = CapabilityFlags::RUNTIME_VERIFIED;
    pub const REBOOT_SAFE: CapabilityFlags = CapabilityFlags::REBOOT_SAFE;
    pub const ENTROPY: CapabilityFlags = CapabilityFlags::ENTROPY;
    pub const INITRD: CapabilityFlags = CapabilityFlags::MODULES;
    pub const SMBIOS: CapabilityFlags = CapabilityFlags::SMBIOS;
}

pub const fn caps_missing(actual: CapabilityFlags, required: CapabilityFlags) -> CapabilityFlags {
    CapabilityFlags::from_bits(actual.bits() & required.bits() ^ required.bits())
}

pub const fn caps_contains(actual: CapabilityFlags, required: CapabilityFlags) -> bool {
    actual.contains(required)
}

// ============================================================================
// HATA SINIFI VE FAZ HATALARI
// ============================================================================

/// Teslim 6 severity sınıfları.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    Fatal = 0,
    Degraded = 1,
    Unsupported = 2,
    Retryable = 3,
    Disabled = 4,
}

/// Faz geçiş hatası — deterministik hata kodu taşır (teslimat 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseError {
    /// Geri geçiş ya da izinsiz atlama (DAG'de geçiş yok).
    OrderViolation {
        from: BootPhase,
        from_state: PhaseState,
        to: BootPhase,
    },
    /// Running faz bitmeden yeni faz başlatma.
    RunningNotComplete { from: BootPhase, to: BootPhase },
    /// Aynı fazı iki kez başlatma.
    DuplicateBegin { phase: BootPhase },
    /// Completed fazı yeniden çalıştırma.
    CompletedRerun { phase: BootPhase },
    /// complete_* yalnızca Running durumundan yapılabilir.
    NotRunning {
        phase: BootPhase,
        actual: PhaseState,
    },
    /// Yanlış faz complete edilmeye çalışıldı.
    PhaseMismatch {
        expected: BootPhase,
        actual: BootPhase,
    },
    /// Capability gate başarısız: faz girişi için gereken yetenek eksik.
    CapabilityGate {
        phase: BootPhase,
        missing: CapabilityFlags,
    },
    /// Profile gate başarısız.
    ProfileGate { phase: BootPhase },
    /// Failed/RecoveryOnly/Halted durumundan normal boot'a dönüş yasak.
    RecoveryLocked,
    /// İki writer aynı transition ownership'ini kazandı (CAS kaybı).
    OwnershipRace { phase: BootPhase },
    /// Encoding ihlali: reserved bitler 0 değil.
    ReservedBitsCorrupt { raw: u32 },
    /// Heap kurulum hatası (karar #2: → RecoveryOnly).
    HeapInitFailed { code: u32 },
    /// Faz tamamlanamadı — belirli hata kodu.
    Failed { code: u32 },
    /// Zorunlu faz `Skipped` sonucu ile kapatılamaz.
    RequiredPhaseCannotSkip { phase: BootPhase },
}

impl PhaseError {
    /// Deterministik teşhis hata kodu (teslimat 5 tablosu).
    pub const fn error_code(self) -> u16 {
        match self {
            PhaseError::OrderViolation { .. } => 0x0001,
            PhaseError::RunningNotComplete { .. } => 0x0002,
            PhaseError::DuplicateBegin { .. } => 0x0003,
            PhaseError::CompletedRerun { .. } => 0x0004,
            PhaseError::NotRunning { .. } => 0x0005,
            PhaseError::PhaseMismatch { .. } => 0x0006,
            PhaseError::CapabilityGate { .. } => 0x0007,
            PhaseError::ProfileGate { .. } => 0x0008,
            PhaseError::RecoveryLocked => 0x0009,
            PhaseError::OwnershipRace { .. } => 0x000A,
            PhaseError::ReservedBitsCorrupt { .. } => 0x000B,
            PhaseError::HeapInitFailed { .. } => 0x0100,
            PhaseError::Failed { code } => code as u16,
            PhaseError::RequiredPhaseCannotSkip { .. } => 0x000C,
        }
    }

    pub const fn failure_class(self) -> FailureClass {
        match self {
            PhaseError::HeapInitFailed { .. } => FailureClass::Fatal,
            _ => FailureClass::Fatal,
        }
    }
}

/// Skipped nedeni (karar 1.4): capability eksikliği faz numarası ATLANMAZ,
/// kontrollü `Skipped(CapabilityUnavailable)` sonucu üretilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    CapabilityUnavailable = 1,
    ConfigDisabled = 2,
    Unsupported = 3,
}

/// Degraded nedeni (karar 1.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeReason {
    SafeFallback = 1,
    PartialService = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseOutcome {
    Completed,
    Degraded(DegradeReason),
    Skipped(SkipReason),
}

/// SMP publication yalnız `SmpInit` sonucu kesinleştiğinde Release edilir.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmpReadiness {
    NotStarted = 0,
    BspOnly = 1,
    Online = 2,
}

/// Heap'ten bağımsız, ilk-yazar-kazanır fatal capsule görünümü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FatalRecord {
    pub failed_phase: BootPhase,
    pub failed_state: PhaseState,
    pub last_completed_phase: BootPhase,
    pub protocol: BootProtocol,
    pub sequence: u8,
    pub error_code: u16,
    pub error_class: FailureClass,
    pub capabilities: CapabilityFlags,
    pub timestamp: usize,
}

// ============================================================================
// GEÇİŞ KURALI TABLOSU (Teslim 2 gerçek DAG → const tablo)
// ============================================================================

/// Tek geçiş kuralı (karar 1.1'deki asgari alan seti).
#[derive(Clone, Copy, Debug)]
pub struct TransitionRule {
    pub from: BootPhase,
    /// `from` fazının bu kural için gereken durumu.
    pub accepted_from_states: &'static [PhaseState],
    pub to: BootPhase,
    /// `to` fazına giriş için gereken yetenekler (0 = yok).
    pub required_caps: CapabilityFlags,
    /// `to` fazının içinde bulunamayacağı durumlar (ör. tekrar begin).
    pub forbidden_states: &'static [PhaseState],
    /// Kuralın geçerli olduğu boot profilleri (sabit liste).
    pub allowed_profiles: &'static [BootProfile],
    /// Bu geçişin hata politikası.
    pub failure_policy: FailureClass,
    /// Geçiş gerekçesi (Teslim 2 DAG bağımlılığı).
    pub reason: &'static str,
}

const FORBID_REBEGIN: &[PhaseState] = &[
    PhaseState::Running,
    PhaseState::Completed,
    PhaseState::Degraded,
    PhaseState::Skipped,
    PhaseState::Failed,
];

const ACCEPT_NOT_STARTED: &[PhaseState] = &[PhaseState::NotStarted];
const ACCEPT_FINISHED: &[PhaseState] = &[
    PhaseState::Completed,
    PhaseState::Degraded,
    PhaseState::Skipped,
];

const ALL_PROFILES: &[BootProfile] = &[
    BootProfile::Uefi,
    BootProfile::Limine,
    BootProfile::Multiboot2,
    BootProfile::Host,
];

/// Teslim 2 gerçek dependency DAG'inden türetilmiş geçiş tablosu.
/// Numeric karşılaştırma YOKTUR; yalnızca bu tablo geçerliliği belirler.
pub const ALLOWED_SUCCESSORS: &[TransitionRule] = &[
    TransitionRule {
        from: BootPhase::Reset,
        accepted_from_states: ACCEPT_NOT_STARTED,
        to: BootPhase::UefiHandover,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "A: adapter handover + normalize + validate",
    },
    TransitionRule {
        from: BootPhase::UefiHandover,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::PagingSetup,
        required_caps: caps::HHDM,
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "B: init_paging + HHDM + MMIO esleme (C->B bagimliligi)",
    },
    TransitionRule {
        from: BootPhase::PagingSetup,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::MemoryInit,
        required_caps: caps::MEMORY_MAP,
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "C: frame allocator/PMM -> global memory manager (C->A bagimliligi)",
    },
    TransitionRule {
        from: BootPhase::MemoryInit,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::HeapInit,
        required_caps: caps::MEMORY_MAP.union(caps::HHDM),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "C-2: heap frame'leri PMM'den ayrilir (karar 1.5)",
    },
    TransitionRule {
        from: BootPhase::HeapInit,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::GdtSetup,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "D: GDT/IDT kurulumlari heap ister (D->C)",
    },
    TransitionRule {
        from: BootPhase::GdtSetup,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::IdtSetup,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "E: interrupts::init + vdso + tty (klavye IRQ'sundan once)",
    },
    TransitionRule {
        from: BootPhase::IdtSetup,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::AcpiInit,
        required_caps: caps::RSDP,
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "F: init_platform_iommu interrupts::init'ten sonra (F->E)",
    },
    TransitionRule {
        from: BootPhase::AcpiInit,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::Scheduling,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "G: scheduler + workers SMP'den once (G->F)",
    },
    TransitionRule {
        from: BootPhase::Scheduling,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::SmpInit,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "H: smp::init scheduler'dan sonra (H->G)",
    },
    TransitionRule {
        from: BootPhase::SmpInit,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::InterruptEnable,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "I: interrupts::enable smp::init'ten sonra (I->H)",
    },
    TransitionRule {
        from: BootPhase::InterruptEnable,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::DriverInit,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "J-1: driver katmani + VFS + init system",
    },
    TransitionRule {
        from: BootPhase::DriverInit,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::Services,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "J-2: appliance + boot_self_check + diagnostics + compositor/shell",
    },
    TransitionRule {
        from: BootPhase::Services,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::UserspaceReady,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "J: userspace kabulu",
    },
    TransitionRule {
        from: BootPhase::UserspaceReady,
        accepted_from_states: ACCEPT_FINISHED,
        to: BootPhase::Running,
        required_caps: CapabilityFlags::empty(),
        forbidden_states: FORBID_REBEGIN,
        allowed_profiles: ALL_PROFILES,
        failure_policy: FailureClass::Fatal,
        reason: "terminal: running (BootWatchdog::complete)",
    },
];

/// `from` fazı için geçerli kuralları döndürür.
///
/// Doğrusal tarama: tablo DAG yazarlık sırasındadır (faz-id sıralı değildir —
/// örn. `PagingSetup`(3)→`MemoryInit`(2) kuralı `MemoryInit`(2) kuralından
/// önce gelir), bu nedenle ikili arama GÜVENLİ DEĞİLDİR. Doğrusal tarama
/// sıralama varsayımı gerektirmez; grup bitiminden sonra devam edilir.
pub fn rules_from(phase: BootPhase) -> &'static [TransitionRule] {
    let mut lo = 0usize;
    while lo < ALLOWED_SUCCESSORS.len() && ALLOWED_SUCCESSORS[lo].from != phase {
        lo += 1;
    }
    if lo == ALLOWED_SUCCESSORS.len() {
        return &[];
    }
    let mut end = lo + 1;
    while end < ALLOWED_SUCCESSORS.len() && ALLOWED_SUCCESSORS[end].from == phase {
        end += 1;
    }
    &ALLOWED_SUCCESSORS[lo..end]
}

// ============================================================================
// FAZ ANLIK GÖRÜNTÜSÜ
// ============================================================================

/// Authoritative word'ün okunabilir görüntüsü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseSnapshot {
    pub phase: BootPhase,
    pub state: PhaseState,
    /// Monotonik boot event sequence numarası (teşhis; karar 2.2).
    pub sequence: u8,
    pub protocol: BootProtocol,
    pub caps_mask: CapabilityFlags,
    pub profile: BootProfile,
    /// Son hata kodu (0 = yok). (phase, last_error) çifti makine tarafından korunur.
    pub last_error_code: u16,
    pub last_error_class: FailureClass,
}

impl PhaseSnapshot {
    pub const fn verdict(&self) -> Verdict {
        match self.state {
            PhaseState::Failed | PhaseState::NotStarted => Verdict::Stop,
            _ => {
                if self.phase.is_terminal() {
                    Verdict::Stop
                } else {
                    Verdict::Continue
                }
            }
        }
    }
}

// ============================================================================
// RECOVERY INFO (adapter tarafından doldurulan teşhis bağlamı — karar 2.2)
// ============================================================================

/// RecoveryOnly teşhis bağlamı. Adapter (UEFI) bildiği alanları doldurur;
/// makine bilinmeyenleri kendi durumundan tamamlar. HEAP YOKTUR.
#[derive(Clone, Copy, Debug)]
pub struct RecoveryInfo {
    /// BootContext v2 version (bilinmiyorsa 0).
    pub context_version: u32,
    /// Hata sınıfı ve kodu.
    pub error: PhaseError,
    /// Heap/PMM bootstrap durumu özeti (örn. "heap-init/failed").
    pub heap_pmm_state: &'static str,
    /// RSDP provenance/state özeti (örn. "uefi/present-validated").
    pub rsdp_state: &'static str,
    /// Memory-map capability state (örn. "present").
    pub memmap_state: &'static str,
    /// BSP/AP durumu (örn. "bsp-only" veya "smp-up").
    pub bsp_ap_state: &'static str,
}

impl RecoveryInfo {
    /// Makinenin kendi durumundan türetilmiş minimal bağlam (fatal_violation içi).
    pub fn from_machine(snapshot: &PhaseSnapshot, error: PhaseError) -> Self {
        let (heap, rsdp, memmap, bsp_ap) = match snapshot.phase {
            BootPhase::HeapInit => ("heap-init/failed", "unknown", "present", "bsp-only"),
            BootPhase::SmpInit
            | BootPhase::InterruptEnable
            | BootPhase::DriverInit
            | BootPhase::Services
            | BootPhase::UserspaceReady => ("ready", "unknown", "present", "smp-up"),
            _ => ("unknown", "unknown", "unknown", "bsp-only"),
        };
        RecoveryInfo {
            context_version: 0,
            error,
            heap_pmm_state: heap,
            rsdp_state: rsdp,
            memmap_state: memmap,
            bsp_ap_state: bsp_ap,
        }
    }
}

// ============================================================================
// STATE WORD ENCODING
// ============================================================================

const SEQ_SHIFT: u32 = 16;
const PROTO_SHIFT: u32 = 24;
const RESERVED_MASK: u32 = 0xFC00_0000;

const fn encode_word(phase: BootPhase, state: PhaseState, seq: u8, protocol: BootProtocol) -> u32 {
    (phase as u32)
        | ((state as u32) << 8)
        | ((seq as u32) << SEQ_SHIFT)
        | ((protocol as u32) << PROTO_SHIFT)
}

struct DecodedWord {
    phase: BootPhase,
    state: PhaseState,
    seq: u8,
    protocol: BootProtocol,
    reserved_ok: bool,
}

fn decode_word(raw: u32) -> DecodedWord {
    let phase = match BootPhase::try_from((raw & 0xFF) as u8) {
        Ok(p) => p,
        Err(_) => BootPhase::Reset,
    };
    let state = match (raw >> 8) & 0xFF {
        0 => PhaseState::NotStarted,
        1 => PhaseState::Running,
        2 => PhaseState::Completed,
        3 => PhaseState::Degraded,
        4 => PhaseState::Skipped,
        5 => PhaseState::Failed,
        _ => PhaseState::NotStarted,
    };
    let protocol = match (raw >> PROTO_SHIFT) & 0x3 {
        1 => BootProtocol::Uefi,
        2 => BootProtocol::Limine,
        3 => BootProtocol::Multiboot2,
        _ => BootProtocol::None,
    };
    DecodedWord {
        phase,
        state,
        seq: ((raw >> SEQ_SHIFT) & 0xFF) as u8,
        protocol,
        reserved_ok: raw & RESERVED_MASK == 0,
    }
}

// ============================================================================
// FAZ MAKİNESİ
// ============================================================================

/// Kanonik faz makinesi. Tek authoritative state: `word`.
///
/// `capabilities`, `profile`, `reboot_hook`, `last_error`, `last_transition_ticks`
/// ve `smp_started` aynı makinenin destekleyici alanlarıdır (yeni GLOBAL değil;
/// eski `BOOT_SAFETY.current_phase`'in buraya taşınmış genişletilmiş halidir).
#[repr(align(64))]
pub struct PhaseMachine {
    word: CacheAligned<AtomicU32>,
    capabilities: CacheAligned<AtomicU64>,
    profile: CacheAligned<AtomicU8>,
    reboot_hook: CacheAligned<AtomicUsize>,
    last_error_code: CacheAligned<AtomicU32>,
    last_transition_ticks: CacheAligned<AtomicUsize>,
    smp_readiness: CacheAligned<AtomicU8>,
    last_completed_phase: CacheAligned<AtomicU8>,
    fatal_state: CacheAligned<AtomicU8>,
    fatal_header: CacheAligned<AtomicU32>,
    fatal_error: CacheAligned<AtomicU32>,
    fatal_capabilities: CacheAligned<AtomicU64>,
    fatal_timestamp: CacheAligned<AtomicUsize>,
}

impl PhaseMachine {
    pub const fn new() -> Self {
        PhaseMachine {
            word: CacheAligned(AtomicU32::new(encode_word(
                BootPhase::Reset,
                PhaseState::NotStarted,
                0,
                BootProtocol::None,
            ))),
            capabilities: CacheAligned(AtomicU64::new(0)),
            profile: CacheAligned(AtomicU8::new(0)),
            reboot_hook: CacheAligned(AtomicUsize::new(0)),
            last_error_code: CacheAligned(AtomicU32::new(0)),
            last_transition_ticks: CacheAligned(AtomicUsize::new(0)),
            smp_readiness: CacheAligned(AtomicU8::new(SmpReadiness::NotStarted as u8)),
            last_completed_phase: CacheAligned(AtomicU8::new(BootPhase::Reset as u8)),
            fatal_state: CacheAligned(AtomicU8::new(0)),
            fatal_header: CacheAligned(AtomicU32::new(0)),
            fatal_error: CacheAligned(AtomicU32::new(0)),
            fatal_capabilities: CacheAligned(AtomicU64::new(0)),
            fatal_timestamp: CacheAligned(AtomicUsize::new(0)),
        }
    }

    // ------------------------------------------------------------------------
    // Okuma
    // ------------------------------------------------------------------------

    /// Authoritative state'i Acquire ile okur (ordering gerekçesi: doküman).
    pub fn current_snapshot(&self) -> PhaseSnapshot {
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        let (err_code, err_class) = self.last_error();
        PhaseSnapshot {
            phase: d.phase,
            state: d.state,
            sequence: d.seq,
            protocol: d.protocol,
            caps_mask: CapabilityFlags::from_bits(self.capabilities.load(Ordering::Acquire)),
            profile: match self.profile.load(Ordering::Acquire) {
                0 => BootProfile::Uefi,
                1 => BootProfile::Limine,
                2 => BootProfile::Multiboot2,
                _ => BootProfile::Host,
            },
            last_error_code: err_code,
            last_error_class: err_class,
        }
    }

    pub fn current_phase(&self) -> BootPhase {
        decode_word(self.word.load(Ordering::Acquire)).phase
    }

    pub fn current_state(&self) -> PhaseState {
        decode_word(self.word.load(Ordering::Acquire)).state
    }

    pub fn protocol(&self) -> BootProtocol {
        decode_word(self.word.load(Ordering::Acquire)).protocol
    }

    pub fn capabilities(&self) -> CapabilityFlags {
        CapabilityFlags::from_bits(self.capabilities.load(Ordering::Acquire))
    }

    pub fn profile(&self) -> BootProfile {
        self.current_snapshot().profile
    }

    /// Son başarılı geçişin tik zamanı (watchdog control noktası).
    pub fn last_transition_ticks(&self) -> usize {
        self.last_transition_ticks.load(Ordering::Acquire)
    }

    /// Son hata kodu + sınıfı (0 = yok).
    pub fn last_error(&self) -> (u16, FailureClass) {
        let raw = self.last_error_code.load(Ordering::Acquire);
        let code = (raw & 0xFFFF) as u16;
        let class = match (raw >> 16) & 0xFF {
            1 => FailureClass::Degraded,
            2 => FailureClass::Unsupported,
            3 => FailureClass::Retryable,
            4 => FailureClass::Disabled,
            _ => FailureClass::Fatal,
        };
        (code, class)
    }

    fn store_error(&self, error: PhaseError) {
        let class = error.failure_class() as u32;
        self.last_error_code.store(
            (error.error_code() as u32) | (class << 16),
            Ordering::Release,
        );
    }

    fn capture_fatal(&self, snapshot: PhaseSnapshot, error: PhaseError) {
        if self
            .fatal_state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let header = (snapshot.phase as u32)
            | ((snapshot.state as u32) << 8)
            | ((self.last_completed_phase() as u32) << 16)
            | ((snapshot.protocol as u32) << 24);
        let encoded_error = (error.error_code() as u32)
            | ((error.failure_class() as u32) << 16)
            | ((snapshot.sequence as u32) << 24);
        self.fatal_header.store(header, Ordering::Relaxed);
        self.fatal_error.store(encoded_error, Ordering::Relaxed);
        self.fatal_capabilities
            .store(snapshot.caps_mask.bits(), Ordering::Relaxed);
        self.fatal_timestamp
            .store(crate::task::scheduler::get_ticks(), Ordering::Relaxed);
        self.fatal_state.store(2, Ordering::Release);
    }

    pub fn fatal_record(&self) -> Option<FatalRecord> {
        if self.fatal_state.load(Ordering::Acquire) != 2 {
            return None;
        }
        let header = self.fatal_header.load(Ordering::Relaxed);
        let error = self.fatal_error.load(Ordering::Relaxed);
        let error_class = match (error >> 16) & 0xFF {
            1 => FailureClass::Degraded,
            2 => FailureClass::Unsupported,
            3 => FailureClass::Retryable,
            4 => FailureClass::Disabled,
            _ => FailureClass::Fatal,
        };
        Some(FatalRecord {
            failed_phase: BootPhase::try_from((header & 0xFF) as u8).unwrap_or(BootPhase::Reset),
            failed_state: match (header >> 8) & 0xFF {
                1 => PhaseState::Running,
                2 => PhaseState::Completed,
                3 => PhaseState::Degraded,
                4 => PhaseState::Skipped,
                5 => PhaseState::Failed,
                _ => PhaseState::NotStarted,
            },
            last_completed_phase: BootPhase::try_from(((header >> 16) & 0xFF) as u8)
                .unwrap_or(BootPhase::Reset),
            protocol: match (header >> 24) & 0x3 {
                1 => BootProtocol::Uefi,
                2 => BootProtocol::Limine,
                3 => BootProtocol::Multiboot2,
                _ => BootProtocol::None,
            },
            sequence: (error >> 24) as u8,
            error_code: (error & 0xFFFF) as u16,
            error_class,
            capabilities: CapabilityFlags::from_bits(
                self.fatal_capabilities.load(Ordering::Relaxed),
            ),
            timestamp: self.fatal_timestamp.load(Ordering::Relaxed),
        })
    }

    /// Verdict (Teslim 5): faz J öncesi Stop ise boot_self_check aynı kanaldan
    /// üretilir (adapter sorumluluğu; makine yalnızca kararı verir).
    pub fn verdict(&self) -> Verdict {
        self.current_snapshot().verdict()
    }

    // ------------------------------------------------------------------------
    // Kayıt (adapter handover)
    // ------------------------------------------------------------------------

    /// Protokolü kaydeder — yalnızca Reset/NotStarted durumunda (CAS, AcqRel).
    pub fn register_protocol(&self, protocol: BootProtocol) -> Result<(), PhaseError> {
        let current = self.word.load(Ordering::Acquire);
        let d = decode_word(current);
        if !d.reserved_ok {
            return Err(PhaseError::ReservedBitsCorrupt { raw: current });
        }
        if d.phase != BootPhase::Reset || d.state != PhaseState::NotStarted {
            return Err(PhaseError::OrderViolation {
                from: d.phase,
                from_state: d.state,
                to: BootPhase::Reset,
            });
        }
        let expected = current & !(0x3 << PROTO_SHIFT);
        let new = expected | ((protocol as u32) << PROTO_SHIFT);
        match self
            .word
            .compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(PhaseError::OwnershipRace {
                phase: BootPhase::Reset,
            }),
        }
    }

    /// Boot profilini kaydeder (adapter handover).
    pub fn set_profile(&self, profile: BootProfile) {
        let value = match profile {
            BootProfile::Uefi => 0,
            BootProfile::Limine => 1,
            BootProfile::Multiboot2 => 2,
            BootProfile::Host => 3,
        };
        self.profile.store(value, Ordering::Release);
    }

    /// Yetenek setini günceller. İlk çağrı handover'da (faz Reset iken);
    /// doğrulama tamamlandıkça (örn. RUNTIME_VERIFIED) Services fazına kadar
    /// OR ile genişletilebilir. Sonradan küçültme reddedilir.
    pub fn set_capabilities(&self, caps_mask: CapabilityFlags) -> Result<(), PhaseError> {
        if matches!(
            self.current_phase(),
            BootPhase::RecoveryOnly | BootPhase::Halted | BootPhase::Running
        ) {
            return Err(PhaseError::RecoveryLocked);
        }
        self.capabilities
            .fetch_or(caps_mask.bits(), Ordering::AcqRel);
        Ok(())
    }

    /// Doğrulaması sonradan başarısız olan capability'leri atomik olarak geri çeker.
    pub fn revoke_capabilities(
        &self,
        removed: CapabilityFlags,
        _reason: DegradeReason,
    ) -> Result<CapabilityFlags, PhaseError> {
        if matches!(
            self.current_phase(),
            BootPhase::RecoveryOnly | BootPhase::Halted | BootPhase::Running
        ) {
            return Err(PhaseError::RecoveryLocked);
        }
        let previous = self
            .capabilities
            .fetch_and(!removed.bits(), Ordering::AcqRel);
        Ok(CapabilityFlags::from_bits(previous & !removed.bits()))
    }

    /// Kontrollü reboot hook'u (protokol-spesifik; UEFI: doğrulanmış runtime
    /// reset). Yalnızca Reset durumunda ve bir kez kaydedilir (CAS).
    pub fn register_reboot_hook(&self, hook: fn() -> !) -> Result<(), PhaseError> {
        let ptr = hook as usize;
        match self
            .reboot_hook
            .compare_exchange(0, ptr, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(PhaseError::OwnershipRace {
                phase: BootPhase::Reset,
            }),
        }
    }

    // ------------------------------------------------------------------------
    // Saf kural doğrulama (test + ön doğrulama + teşhis kanalı)
    // ------------------------------------------------------------------------

    /// Geçiş kuralını DAG tablosunda doğrular — hiçbir state'i değiştirmez.
    pub fn validate_transition(&self, to: BootPhase) -> Result<(), PhaseError> {
        let snapshot = self.current_snapshot();
        self.validate_from(snapshot, to)
    }

    fn validate_from(&self, snapshot: PhaseSnapshot, to: BootPhase) -> Result<(), PhaseError> {
        if to == snapshot.phase {
            return match snapshot.state {
                PhaseState::Running => Err(PhaseError::DuplicateBegin { phase: to }),
                PhaseState::Completed => Err(PhaseError::CompletedRerun { phase: to }),
                _ => Err(PhaseError::OrderViolation {
                    from: snapshot.phase,
                    from_state: snapshot.state,
                    to,
                }),
            };
        }
        if snapshot.phase.is_terminal() {
            return Err(PhaseError::RecoveryLocked);
        }
        let rules = rules_from(snapshot.phase);
        let rule = rules.iter().find(|r| r.to == to);
        match rule {
            None => Err(PhaseError::OrderViolation {
                from: snapshot.phase,
                from_state: snapshot.state,
                to,
            }),
            Some(rule) => {
                if !rule.accepted_from_states.contains(&snapshot.state) {
                    return if snapshot.state == PhaseState::Running {
                        Err(PhaseError::RunningNotComplete {
                            from: snapshot.phase,
                            to,
                        })
                    } else {
                        Err(PhaseError::OrderViolation {
                            from: snapshot.phase,
                            from_state: snapshot.state,
                            to,
                        })
                    };
                }
                // `forbidden_states` (FORBID_REBEGIN) TO fazının durumu içindir:
                // `to != from` olduğundan TO fazı dolaylı olarak her zaman
                // NotStarted'tır ve bu kümeye hiçbir zaman girmez. Mevcut fazın
                // yeniden başlatılması yukarıdaki `to == phase` dalında
                // DuplicateBegin/CompletedRerun olarak zaten reddedilir;
                // FROM durumunu forbidden kümesine karşı kontrol etmek, her
                // Completed öncülden geçişi yanlışlıkla reddeder (burada
                // yakalanan hata — bakınız advance_from_running_rejected).
                let missing = caps_missing(snapshot.caps_mask, rule.required_caps);
                if missing.bits() != 0 {
                    return Err(PhaseError::CapabilityGate { phase: to, missing });
                }
                if !rule.allowed_profiles.contains(&snapshot.profile) {
                    return Err(PhaseError::ProfileGate { phase: to });
                }
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------------------
    // Geçişler — tek linearization point: CAS
    // ------------------------------------------------------------------------

    /// Faza girer (Running). Kural ihlali FATAL'dir: kayıt → fail geçişi →
    /// RecoveryOnly → halt/reboot zinciri çalışır ve geri dönmez (karar 2.4).
    pub fn begin_phase(&self, phase: BootPhase) -> Result<(), PhaseError> {
        let snapshot = self.current_snapshot();
        match self.validate_from(snapshot, phase) {
            Ok(()) => {}
            Err(err) => {
                self.fatal_error(err);
            }
        }
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        if !d.reserved_ok {
            self.fatal_violation(ViolationType::PhaseOrder, "reserved bit ihlali");
        }
        let new = encode_word(
            phase,
            PhaseState::Running,
            d.seq.wrapping_add(1),
            d.protocol,
        );
        match self
            .word
            .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.after_transition(phase);
                Ok(())
            }
            Err(_) => {
                self.fatal_violation(ViolationType::PhaseOrder, "transition ownership yarisi");
            }
        }
    }

    /// Fazı tamamlar (Running → Completed).
    pub fn complete_phase(&self, phase: BootPhase) -> Result<(), PhaseError> {
        self.complete_to(phase, PhaseState::Completed)
    }

    /// Fazı degrade ederek tamamlar (karar 1.4: SafeFallback).
    pub fn complete_degraded(
        &self,
        phase: BootPhase,
        _reason: DegradeReason,
    ) -> Result<(), PhaseError> {
        self.complete_to(phase, PhaseState::Degraded)
    }

    /// Fazı kontrollü olarak atlayarak tamamlar (karar 1.4:
    /// `Skipped(CapabilityUnavailable)` — faz numarası ATLANMAZ).
    pub fn complete_skipped(
        &self,
        phase: BootPhase,
        _reason: SkipReason,
    ) -> Result<(), PhaseError> {
        if phase != BootPhase::SmpInit {
            return Err(PhaseError::RequiredPhaseCannotSkip { phase });
        }
        self.complete_to(phase, PhaseState::Skipped)
    }

    fn complete_to(&self, phase: BootPhase, state: PhaseState) -> Result<(), PhaseError> {
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        if !d.reserved_ok {
            return Err(PhaseError::ReservedBitsCorrupt { raw });
        }
        if d.phase != phase {
            return Err(PhaseError::PhaseMismatch {
                expected: phase,
                actual: d.phase,
            });
        }
        if d.state != PhaseState::Running {
            return Err(PhaseError::NotRunning {
                phase,
                actual: d.state,
            });
        }
        let new = encode_word(phase, state, d.seq.wrapping_add(1), d.protocol);
        match self
            .word
            .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                if matches!(
                    state,
                    PhaseState::Completed | PhaseState::Degraded | PhaseState::Skipped
                ) {
                    self.last_completed_phase
                        .store(phase as u8, Ordering::Release);
                }
                if phase == BootPhase::SmpInit {
                    let readiness =
                        if state == PhaseState::Completed && crate::cpu::smp::get_cpu_count() > 1 {
                            SmpReadiness::Online
                        } else {
                            SmpReadiness::BspOnly
                        };
                    self.smp_readiness.store(readiness as u8, Ordering::Release);
                }
                self.after_transition(phase);
                Ok(())
            }
            Err(_) => Err(PhaseError::OwnershipRace { phase }),
        }
    }

    /// Ortak terminal publication: test politikalarından bağımsız olarak
    /// Services → UserspaceReady → Running zincirini tek başarı noktasında kapatır.
    pub fn finish_boot(&self, services_outcome: PhaseOutcome) -> Result<(), PhaseError> {
        match services_outcome {
            PhaseOutcome::Completed => self.complete_phase(BootPhase::Services)?,
            PhaseOutcome::Degraded(reason) => {
                self.complete_degraded(BootPhase::Services, reason)?
            }
            PhaseOutcome::Skipped(reason) => self.complete_skipped(BootPhase::Services, reason)?,
        }
        self.begin_phase(BootPhase::UserspaceReady)?;
        self.emit_phase_marker(&self.current_snapshot());
        self.complete_phase(BootPhase::UserspaceReady)?;
        self.emit_phase_marker(&self.current_snapshot());
        self.begin_phase(BootPhase::Running)?;
        self.emit_phase_marker(&self.current_snapshot());
        crate::boot::safety::BootWatchdog::complete();
        Ok(())
    }

    /// Fazı başarısız ilan eder (Running → Failed) — karar 2 akışının ilk adımı.
    pub fn fail_phase(&self, phase: BootPhase, error: PhaseError) -> Result<(), PhaseError> {
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        if !d.reserved_ok {
            return Err(PhaseError::ReservedBitsCorrupt { raw });
        }
        if d.phase != phase {
            return Err(PhaseError::PhaseMismatch {
                expected: phase,
                actual: d.phase,
            });
        }
        if d.state != PhaseState::Running {
            return Err(PhaseError::NotRunning {
                phase,
                actual: d.state,
            });
        }
        self.store_error(error);
        let new = encode_word(phase, PhaseState::Failed, d.seq.wrapping_add(1), d.protocol);
        match self
            .word
            .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.capture_fatal(self.current_snapshot(), error);
                self.after_transition(phase);
                Ok(())
            }
            Err(_) => Err(PhaseError::OwnershipRace { phase }),
        }
    }

    /// RecoveryOnly fazına atomik geçiş (CAS) — idempotent.
    pub fn recovery_transition(&self) -> Result<(), PhaseError> {
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        if d.phase == BootPhase::RecoveryOnly || d.phase == BootPhase::Halted {
            return Ok(()); // zaten recovery/halted — idempotent
        }
        if !d.reserved_ok {
            return Err(PhaseError::ReservedBitsCorrupt { raw });
        }
        let new = encode_word(
            BootPhase::RecoveryOnly,
            PhaseState::Running,
            d.seq.wrapping_add(1),
            d.protocol,
        );
        match self
            .word
            .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.after_transition(BootPhase::RecoveryOnly);
                Ok(())
            }
            Err(_) => {
                // Diğer writer kazandı — karar 2.4: ownership yarışı fatal.
                Err(PhaseError::OwnershipRace {
                    phase: BootPhase::RecoveryOnly,
                })
            }
        }
    }

    /// Faz makinesini Halted terminal durumuna geçirir (CAS), ardından
    /// kontrollü durma döngüsüne girer — geri dönmez.
    pub fn halt(&self) -> ! {
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        if d.phase != BootPhase::Halted {
            let new = encode_word(
                BootPhase::Halted,
                PhaseState::Completed,
                d.seq.wrapping_add(1),
                d.protocol,
            );
            match self
                .word
                .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {}
                Err(_) => crate::serial_println!(
                    "[BOOT_POLICY] halt transition lost ownership; another terminal writer won"
                ),
            }
        }
        self.terminate_loop()
    }

    /// Karar 2.4 zinciri: kayıt → atomik fail geçişi → RecoveryOnly → halt/reboot.
    /// Yalnızca Fatal sınıfı ihlaller için — geri dönmez.
    pub fn fatal_violation(&self, _violation: ViolationType, _message: &str) -> ! {
        let snapshot = self.current_snapshot();
        let error = PhaseError::OrderViolation {
            from: snapshot.phase,
            from_state: snapshot.state,
            to: BootPhase::RecoveryOnly,
        };
        self.fatal_error(error)
    }

    /// Allocation-free fatal giriş noktası. İlk hata capsule'ı korunur.
    pub fn fatal_error(&self, error: PhaseError) -> ! {
        // Atomik fail geçişi: aktif faz (Running/Completed/Degraded/Skipped) → Failed.
        let raw = self.word.load(Ordering::Acquire);
        let d = decode_word(raw);
        self.capture_fatal(self.current_snapshot(), error);
        self.store_error(error);
        if !matches!(d.state, PhaseState::Failed | PhaseState::NotStarted)
            && d.phase != BootPhase::RecoveryOnly
            && d.phase != BootPhase::Halted
        {
            let new = encode_word(
                d.phase,
                PhaseState::Failed,
                d.seq.wrapping_add(1),
                d.protocol,
            );
            match self
                .word
                .compare_exchange(raw, new, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {}
                Err(_) => crate::serial_println!(
                    "[BOOT_POLICY] fatal transition lost ownership; recovery owner already active"
                ),
            }
        }

        let snapshot = self.current_snapshot();
        let info = RecoveryInfo::from_machine(&snapshot, error);
        self.enter_recovery(&info);
    }

    /// RecoveryOnly'ye girer: atomik geçiş + allocation-free teşhis +
    /// kontrollü sonlandırma. GERİ DÖNMEZ (karar 2.1).
    pub fn enter_recovery(&self, info: &RecoveryInfo) -> ! {
        match self.recovery_transition() {
            Ok(()) => {}
            Err(PhaseError::OwnershipRace { .. }) => {
                // İkinci writer yarışı kazandıysa recovery zaten başladı; devam.
                crate::serial_println!(
                    "[RECOVERY] ownership yarisi — recovery zaten basladi, devam ediliyor"
                );
            }
            Err(err) => {
                crate::serial_println!("[RECOVERY] recovery gecis hatasi: {:?}", err);
            }
        }
        self.recovery_diagnostics(info);
        self.terminate()
    }

    /// RecoveryOnly teşhisini allocation-free yayınlar (karar 2.2).
    pub fn recovery_diagnostics(&self, info: &RecoveryInfo) {
        let snapshot = self.current_snapshot();
        let fatal = self.fatal_record();
        crate::serial_println!("[RECOVERY] === BootFailureContainment basladi ===");
        crate::serial_println!("[RECOVERY] context_version={}", info.context_version);
        crate::serial_println!("[RECOVERY] boot_protocol={}", snapshot.protocol.name());
        crate::serial_println!("[RECOVERY] boot_profile={:?}", snapshot.profile);
        crate::serial_println!(
            "[RECOVERY] last_completed_phase={:?}",
            fatal
                .map(|r| r.last_completed_phase)
                .unwrap_or_else(|| self.last_completed_phase())
        );
        crate::serial_println!(
            "[RECOVERY] failed_phase={:?}",
            fatal.map(|r| r.failed_phase).unwrap_or(snapshot.phase)
        );
        crate::serial_println!(
            "[RECOVERY] error_class={:?} error_code={:#06x}",
            fatal
                .map(|r| r.error_class)
                .unwrap_or(snapshot.last_error_class),
            fatal
                .map(|r| r.error_code)
                .unwrap_or(snapshot.last_error_code)
        );
        crate::serial_println!(
            "[RECOVERY] capability_validation=caps={:#010x}",
            fatal
                .map(|r| r.capabilities.bits())
                .unwrap_or(snapshot.caps_mask.bits())
        );
        crate::serial_println!("[RECOVERY] heap_pmm_state={}", info.heap_pmm_state);
        crate::serial_println!("[RECOVERY] rsdp_state={}", info.rsdp_state);
        crate::serial_println!("[RECOVERY] memmap_capability_state={}", info.memmap_state);
        crate::serial_println!("[RECOVERY] bsp_ap_state={}", info.bsp_ap_state);
        crate::serial_println!("[RECOVERY] boot_event_sequence={}", snapshot.sequence);
        crate::serial_println!(
            "[RECOVERY] phase_state={:?} verdict={:?}",
            snapshot.state,
            snapshot.verdict()
        );

        #[cfg(not(target_os = "windows"))]
        {
            crate::serial::uart::debugcon_write_fmt(format_args!(
                "RECOVERY seq={} phase={} state={:?} verdict={:?}\n",
                snapshot.sequence,
                snapshot.phase.name(),
                snapshot.state,
                snapshot.verdict()
            ));
        }
    }

    /// Son Completed/Degraded/Skipped fazı (teşhis için).
    pub fn last_completed_phase(&self) -> BootPhase {
        BootPhase::try_from(self.last_completed_phase.load(Ordering::Acquire))
            .unwrap_or(BootPhase::Reset)
    }

    pub fn smp_readiness(&self) -> SmpReadiness {
        match self.smp_readiness.load(Ordering::Acquire) {
            1 => SmpReadiness::BspOnly,
            2 => SmpReadiness::Online,
            _ => SmpReadiness::NotStarted,
        }
    }

    /// Kontrollü sonlandırma (karar 2.3): doğrulanmış reboot hook'u varsa
    /// denenir; aksi durumda kesintiler maskelenir, AP'ler park edilir, BSP
    /// kontrollü halt döngüsüne girer. GERİ DÖNMEZ.
    pub fn terminate(&self) -> ! {
        let hook = self.reboot_hook.load(Ordering::Acquire);
        let caps_mask = CapabilityFlags::from_bits(self.capabilities.load(Ordering::Acquire));
        if hook != 0 && caps_contains(caps_mask, caps::REBOOT_SAFE) {
            crate::serial_println!("[RECOVERY] kontrollu reboot deneniyor");
            let hook_fn: fn() -> ! = unsafe { core::mem::transmute(hook) };
            hook_fn();
        }
        self.terminate_loop();
    }

    /// AP'leri mevcut güvenli mekanizma üzerinden park eder (smp_state),
    /// ardından BSP `cli; hlt` döngüsüne girer. GERİ DÖNMEZ.
    fn terminate_loop(&self) -> ! {
        // Kesintileri maskele.
        unsafe {
            core::arch::asm!("cli");
        }

        if self.smp_readiness() == SmpReadiness::Online {
            // Release-published panic-stop isteği + IPI, hedef AP'leri kendi
            // `cli; hlt` terminal döngülerine sokar; yalnız metadata yazılmaz.
            crate::cpu::smp::broadcast_panic_stop();
            crate::serial_println!("[RECOVERY] AP panic-stop yayini gonderildi");
        }

        crate::serial_println!("[RECOVERY] BSP kontrollu halt dongusunde");
        loop {
            unsafe {
                core::arch::asm!("cli; hlt");
            }
        }
    }

    fn after_transition(&self, phase: BootPhase) {
        self.last_transition_ticks
            .store(crate::task::scheduler::get_ticks(), Ordering::Release);
    }

    // ------------------------------------------------------------------------
    // Marker (karar: PIPELINE SAFETY API + deterministik format)
    // ------------------------------------------------------------------------

    /// Her geçişte deterministik `[BOOT_PHASE]` marker bloğu yayınlar.
    /// Marker üretimi boot akışının doğruluğunu değiştirmez; marker
    /// başarısızlığı pipeline'ı bozmaz (yalnızca teşhis kanalıdır).
    pub fn emit_phase_marker(&self, snapshot: &PhaseSnapshot) {
        let mut buf = [0u8; 256];
        let len = format_marker(&mut buf, snapshot);
        let text = core::str::from_utf8(&buf[..len]).unwrap_or("<marker>");
        crate::serial_println!("{}", text);

        // The hardware marker is the cross-protocol ground-truth channel:
        // UEFI and bare-metal (Limine/Multiboot2) all expose port 0xE9 in the
        // QEMU smoke contract.  Keep host/unit-test builds serial-only.
        #[cfg(any(target_os = "uefi", target_os = "none"))]
        {
            for byte in [b'P', snapshot.phase.phase_id(), snapshot.state as u8, b'\n'] {
                unsafe {
                    core::arch::asm!(
                        "out 0xe9, al",
                        in("al") byte,
                        options(nomem, nostack, preserves_flags),
                    );
                }
            }
        }
    }
}

/// Deterministik marker formatı (karar: PIPELINE SAFETY API) — saf fonksiyon.
/// Format:
/// ```text
/// [BOOT_PHASE]
/// sequence=<seq>
/// phase_id=<id>
/// phase_name=<name>
/// phase_state=<state>
/// protocol=<proto>
/// verdict=<v>
/// error_code=<code>
/// ```
pub fn format_marker(buf: &mut [u8], snapshot: &PhaseSnapshot) -> usize {
    use core::fmt::Write;

    struct Buf<'a>(&'a mut [u8], usize);
    impl core::fmt::Write for Buf<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let space = self.0.len() - self.1;
            let n = s.len().min(space);
            self.0[self.1..self.1 + n].copy_from_slice(&s.as_bytes()[..n]);
            self.1 += n;
            Ok(())
        }
    }

    let mut w = Buf(buf, 0);
    let verdict = snapshot.verdict();
    if write!(
        w,
        "[BOOT_PHASE]\nsequence={}\nphase_id={}\nphase_name={}\nphase_state={}\nprotocol={}\nverdict={:?}\nerror_code={:#06x}",
        snapshot.sequence,
        snapshot.phase.phase_id(),
        snapshot.phase.name(),
        snapshot.state.name(),
        snapshot.protocol.name(),
        verdict,
        snapshot.last_error_code,
    )
    .is_err()
    {
        // `Buf` is bounded and truncates each write, so this is defensive
        // policy instrumentation for a future formatter change.
        if !w.0.is_empty() {
            w.0[w.0.len() - 1] = b'!';
        }
    }
    w.1
}

// ============================================================================
// GLOBAL — eski `BOOT_SAFETY.current_phase`'in taşınmış halidir (tek state)
// ============================================================================

/// Kanonik faz makinesi global örneği.
pub static BOOT_PIPELINE: PhaseMachine = PhaseMachine::new();

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> PhaseMachine {
        PhaseMachine::new()
    }

    /// Tüm A–J zincirini UEFI dizisiyle yürütür (Teslim 2 UEFI DAG parity'si).
    fn run_to_services(m: &PhaseMachine) {
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::MEMORY_MAP | caps::HHDM | caps::RSDP | caps::CMDLINE)
            .unwrap();

        assert_eq!(m.begin_phase(BootPhase::UefiHandover), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::UefiHandover), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::PagingSetup), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::PagingSetup), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::MemoryInit), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::MemoryInit), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::HeapInit), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::HeapInit), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::GdtSetup), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::GdtSetup), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::IdtSetup), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::IdtSetup), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::AcpiInit), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::AcpiInit), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::Scheduling), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::Scheduling), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::SmpInit), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::SmpInit), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::InterruptEnable), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::InterruptEnable), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::DriverInit), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::DriverInit), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::Services), Ok(()));
    }

    fn run_uefi_chain(m: &PhaseMachine) {
        run_to_services(m);
        assert_eq!(m.complete_phase(BootPhase::Services), Ok(()));

        assert_eq!(m.begin_phase(BootPhase::UserspaceReady), Ok(()));
        assert_eq!(m.complete_phase(BootPhase::UserspaceReady), Ok(()));
    }

    // 1 — Teslim 2 UEFI DAG'inin bütün yasal geçişleri
    #[test]
    fn all_legal_transitions_succeed() {
        let m = fresh();
        run_uefi_chain(&m);
        let snap = m.current_snapshot();
        assert_eq!(snap.phase, BootPhase::UserspaceReady);
        assert_eq!(snap.state, PhaseState::Completed);
        assert_eq!(snap.verdict(), Verdict::Continue);
    }

    // 2 — Mevcut gerçek UEFI call trace parity (faz sırası DAG'e uygun)
    #[test]
    fn uefi_call_trace_parity() {
        let m = fresh();
        run_uefi_chain(&m);
        let expected = [
            (BootPhase::UefiHandover, PhaseState::Completed),
            (BootPhase::PagingSetup, PhaseState::Completed),
            (BootPhase::MemoryInit, PhaseState::Completed),
            (BootPhase::HeapInit, PhaseState::Completed),
            (BootPhase::GdtSetup, PhaseState::Completed),
            (BootPhase::IdtSetup, PhaseState::Completed),
            (BootPhase::AcpiInit, PhaseState::Completed),
            (BootPhase::Scheduling, PhaseState::Completed),
            (BootPhase::SmpInit, PhaseState::Completed),
            (BootPhase::InterruptEnable, PhaseState::Completed),
            (BootPhase::DriverInit, PhaseState::Completed),
            (BootPhase::Services, PhaseState::Completed),
            (BootPhase::UserspaceReady, PhaseState::Completed),
        ];
        let snap = m.current_snapshot();
        assert_eq!((snap.phase, snap.state), expected[expected.len() - 1]);
        // sequence monotonik artmalı ve faz sayısının 2 katı olmalı
        assert_eq!(snap.sequence, (expected.len() * 2) as u8);
    }

    // 3 — Geri geçiş reddi
    #[test]
    fn backward_transition_rejected() {
        let m = fresh();
        run_uefi_chain(&m);
        // Zincir UserspaceReady/Completed'da: Services'e geri dönüş DAG'de yok.
        let err = m.validate_transition(BootPhase::Services).unwrap_err();
        assert!(matches!(
            err,
            PhaseError::OrderViolation {
                from: BootPhase::UserspaceReady,
                to: BootPhase::Services,
                ..
            }
        ));
    }

    // 4 — İzinsiz atlama reddi (A'dan C'ye atlanamaz)
    #[test]
    fn skip_transition_rejected() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::MEMORY_MAP | caps::HHDM).unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        let err = m.validate_transition(BootPhase::MemoryInit).unwrap_err();
        assert!(matches!(
            err,
            PhaseError::OrderViolation {
                from: BootPhase::UefiHandover,
                to: BootPhase::MemoryInit,
                ..
            }
        ));
    }

    // 5 — Duplicate begin reddi
    #[test]
    fn duplicate_begin_rejected() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        let err = m.validate_transition(BootPhase::UefiHandover).unwrap_err();
        assert_eq!(
            err,
            PhaseError::DuplicateBegin {
                phase: BootPhase::UefiHandover
            }
        );
    }

    // 6 — Complete edilmemiş fazdan ilerleme reddi
    #[test]
    fn advance_from_running_rejected() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::HHDM).unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        // PagingSetup hâlâ Running — ilerlenemez.
        let err = m.validate_transition(BootPhase::MemoryInit).unwrap_err();
        assert!(matches!(
            err,
            PhaseError::RunningNotComplete {
                from: BootPhase::PagingSetup,
                ..
            }
        ));
    }

    // 7 — Optional capability eksikliği: Skipped sonucu (capability numarası atlanmaz)
    #[test]
    fn optional_capability_skipped() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::MEMORY_MAP | caps::HHDM | caps::RSDP)
            .unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        assert_eq!(
            m.complete_skipped(BootPhase::UefiHandover, SkipReason::CapabilityUnavailable),
            Err(PhaseError::RequiredPhaseCannotSkip {
                phase: BootPhase::UefiHandover,
            })
        );
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        for phase in [
            BootPhase::PagingSetup,
            BootPhase::MemoryInit,
            BootPhase::HeapInit,
            BootPhase::GdtSetup,
            BootPhase::IdtSetup,
            BootPhase::AcpiInit,
            BootPhase::Scheduling,
        ] {
            m.begin_phase(phase).unwrap();
            m.complete_phase(phase).unwrap();
        }
        m.begin_phase(BootPhase::SmpInit).unwrap();
        assert_eq!(m.smp_readiness(), SmpReadiness::NotStarted);
        m.complete_skipped(BootPhase::SmpInit, SkipReason::CapabilityUnavailable)
            .unwrap();
        let snap = m.current_snapshot();
        assert_eq!(snap.state, PhaseState::Skipped);
        assert_eq!(m.smp_readiness(), SmpReadiness::BspOnly);
        assert_eq!(m.validate_transition(BootPhase::InterruptEnable), Ok(()));
    }

    // 8 — Required capability eksikliği: gate reddi deterministik ve Fatal sınıfı;
    //     gerçek Failed→RecoveryOnly zinciri heap_failure_containment ve
    //     fatal_chain_steps_atomic içinde adım adım doğrulanır.
    #[test]
    fn required_capability_fails_to_recovery() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        // HHDM yok: PagingSetup girişi CapabilityGate hatası verir.
        let err = m.validate_transition(BootPhase::PagingSetup).unwrap_err();
        assert!(matches!(
            err,
            PhaseError::CapabilityGate {
                phase: BootPhase::PagingSetup,
                missing
            } if missing == caps::HHDM
        ));
        // Deterministik hata kodu (teslimat 5): 0x0007, Fatal sınıfı.
        assert_eq!(err.error_code(), 0x0007);
        assert_eq!(err.failure_class(), FailureClass::Fatal);
    }

    // 9 — Heap init failure containment
    #[test]
    fn heap_failure_containment() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::MEMORY_MAP | caps::HHDM | caps::RSDP | caps::CMDLINE)
            .unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        m.complete_phase(BootPhase::PagingSetup).unwrap();
        m.begin_phase(BootPhase::MemoryInit).unwrap();
        m.complete_phase(BootPhase::MemoryInit).unwrap();
        m.begin_phase(BootPhase::HeapInit).unwrap();
        // Heap kurulumu başarısız → Failed.
        m.fail_phase(
            BootPhase::HeapInit,
            PhaseError::HeapInitFailed { code: 0x1234 },
        )
        .unwrap();
        let snap = m.current_snapshot();
        assert_eq!(snap.phase, BootPhase::HeapInit);
        assert_eq!(snap.state, PhaseState::Failed);
        // State word sınıf kodunu taşır (0x0100 = heap init failure);
        // detay kodu (0x1234) PhaseError nesnesinde kalır (RecoveryInfo.error).
        assert_eq!(snap.last_error_code, 0x0100);
        assert_eq!(snap.verdict(), Verdict::Stop);
        // RecoveryOnly geçişi idempotent ve atomik.
        assert_eq!(m.recovery_transition(), Ok(()));
        let snap = m.current_snapshot();
        assert_eq!(snap.phase, BootPhase::RecoveryOnly);
        assert_eq!(snap.state, PhaseState::Running);
        assert_eq!(m.recovery_transition(), Ok(()));
    }

    // 10 — RecoveryOnly normal pipeline'a dönemez (RecoveryLocked)
    #[test]
    fn recovery_locks_normal_boot() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.recovery_transition().unwrap();
        let snap = m.current_snapshot();
        assert_eq!(snap.phase, BootPhase::RecoveryOnly);
        // RecoveryOnly terminaldir: hiçbir normal faza geçiş kabul edilmez.
        for phase in [
            BootPhase::PagingSetup,
            BootPhase::MemoryInit,
            BootPhase::HeapInit,
            BootPhase::GdtSetup,
            BootPhase::IdtSetup,
            BootPhase::AcpiInit,
            BootPhase::Scheduling,
            BootPhase::SmpInit,
            BootPhase::InterruptEnable,
            BootPhase::DriverInit,
            BootPhase::Services,
            BootPhase::UserspaceReady,
        ] {
            assert_eq!(
                m.validate_transition(phase),
                Err(PhaseError::RecoveryLocked),
                "{} kabul edildi",
                phase.name()
            );
        }
        // Verdict Stop: faz makinesi devam kararı vermez.
        assert_eq!(m.verdict(), Verdict::Stop);
        // RecoveryOnly → RecoveryOnly geçişi idempotent (enter_recovery yeniden
        // çağrılabilir; terminate zinciri tek makinede tek kez işler).
        assert_eq!(m.recovery_transition(), Ok(()));
    }

    // 11 — record_violation'ın fatal policy'ye bağlanması (zincirin adımları)
    #[test]
    fn fatal_chain_steps_atomic() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::HHDM).unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        // fail_phase atomik: (PagingSetup, Running) → (PagingSetup, Failed)
        m.fail_phase(BootPhase::PagingSetup, PhaseError::Failed { code: 0x77 })
            .unwrap();
        let snap = m.current_snapshot();
        assert_eq!(
            (snap.phase, snap.state),
            (BootPhase::PagingSetup, PhaseState::Failed)
        );
        assert_eq!(snap.last_error_code, 0x77);
        // Failed durumundan normal boot yasak.
        assert_eq!(
            m.validate_transition(BootPhase::MemoryInit),
            Err(PhaseError::OrderViolation {
                from: BootPhase::PagingSetup,
                from_state: PhaseState::Failed,
                to: BootPhase::MemoryInit,
            })
        );
        // Recovery geçişi çalışır.
        assert_eq!(m.recovery_transition(), Ok(()));
    }

    // 12 — Stabil PhaseId compatibility (legacy değerler korunur)
    #[test]
    fn stable_phase_ids() {
        assert_eq!(BootPhase::Reset.phase_id(), 0);
        assert_eq!(BootPhase::UefiHandover.phase_id(), 1);
        assert_eq!(BootPhase::MemoryInit.phase_id(), 2);
        assert_eq!(BootPhase::PagingSetup.phase_id(), 3);
        assert_eq!(BootPhase::HeapInit.phase_id(), 4);
        assert_eq!(BootPhase::GdtSetup.phase_id(), 5);
        assert_eq!(BootPhase::IdtSetup.phase_id(), 6);
        assert_eq!(BootPhase::AcpiInit.phase_id(), 7);
        assert_eq!(BootPhase::SmpInit.phase_id(), 8);
        assert_eq!(BootPhase::DriverInit.phase_id(), 9);
        assert_eq!(BootPhase::UserspaceReady.phase_id(), 10);
        assert_eq!(BootPhase::Running.phase_id(), 255);
        assert_eq!(BootPhase::Scheduling.phase_id(), 11);
        assert_eq!(BootPhase::InterruptEnable.phase_id(), 12);
        assert_eq!(BootPhase::Services.phase_id(), 13);
        assert_eq!(BootPhase::RecoveryOnly.phase_id(), 14);
        assert_eq!(BootPhase::Halted.phase_id(), 15);
        // TryFrom round-trip
        for id in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 255] {
            assert_eq!(BootPhase::try_from(id).unwrap().phase_id(), id);
        }
    }

    // 13 — Atomic transition yarış testi
    #[test]
    fn atomic_transition_race() {
        use std::sync::Arc;
        use std::thread;

        let m = Arc::new(fresh());
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::MEMORY_MAP | caps::HHDM | caps::RSDP)
            .unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        m.complete_phase(BootPhase::PagingSetup).unwrap();
        m.begin_phase(BootPhase::MemoryInit).unwrap();

        // İki thread aynı fazı complete etmeye çalışır; biri kazanır, diğeri
        // OwnershipRace alır. Word asla bozulmaz.
        let m1 = Arc::clone(&m);
        let m2 = Arc::clone(&m);
        let t1 = thread::spawn(move || m1.complete_phase(BootPhase::MemoryInit));
        let t2 = thread::spawn(move || m2.complete_phase(BootPhase::MemoryInit));
        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();
        let wins = [r1.is_ok(), r2.is_ok()];
        assert_eq!(wins.iter().filter(|w| **w).count(), 1);
        assert!(r1.is_err() || r2.is_err());
        let snap = m.current_snapshot();
        assert_eq!(snap.phase, BootPhase::MemoryInit);
        assert_eq!(snap.state, PhaseState::Completed);
    }

    // 17 — Marker formatı deterministik ve alanları tam
    #[test]
    fn marker_format_deterministic() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::HHDM).unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        let snap = m.current_snapshot();

        let mut buf = [0u8; 256];
        let len = format_marker(&mut buf, &snap);
        let text = core::str::from_utf8(&buf[..len]).unwrap();

        assert!(text.starts_with("[BOOT_PHASE]\n"));
        for field in [
            "sequence=",
            "phase_id=3",
            "phase_name=memory-layout",
            "phase_state=running",
            "protocol=uefi",
            "verdict=Continue",
            "error_code=0x0000",
        ] {
            assert!(text.contains(field), "eksik: {}", field);
        }

        // Determinizm: aynı snapshot iki kez aynı çıktıyı verir.
        let mut buf2 = [0u8; 256];
        let len2 = format_marker(&mut buf2, &snap);
        assert_eq!(len, len2);
        assert_eq!(buf[..len], buf2[..len2]);
    }

    // Encoding: reserved bitler doğrulanır
    #[test]
    fn reserved_bits_validated() {
        let raw = encode_word(
            BootPhase::UefiHandover,
            PhaseState::Running,
            1,
            BootProtocol::Uefi,
        );
        let d = decode_word(raw);
        assert!(d.reserved_ok);
        assert_eq!(d.phase, BootPhase::UefiHandover);
        assert_eq!(d.state, PhaseState::Running);
        assert_eq!(d.seq, 1);
        assert_eq!(d.protocol, BootProtocol::Uefi);

        let corrupt = raw | 0x0400_0000; // reserved bit 26
        assert!(!decode_word(corrupt).reserved_ok);
    }

    // Verdict: Stop/Continue semantiği
    #[test]
    fn verdict_semantics() {
        let m = fresh();
        assert_eq!(m.verdict(), Verdict::Stop); // Reset/NotStarted
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        assert_eq!(m.verdict(), Verdict::Continue);
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        assert_eq!(m.verdict(), Verdict::Continue);
    }

    // Capability gate: eksik yetenekle giriş reddi
    #[test]
    fn capability_gate_missing() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        let err = m.validate_transition(BootPhase::PagingSetup).unwrap_err();
        assert!(matches!(
            err,
            PhaseError::CapabilityGate {
                phase: BootPhase::PagingSetup,
                missing
            } if missing == caps::HHDM
        ));
    }

    // Degraded tamamlama
    #[test]
    fn degraded_completion() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.set_capabilities(caps::HHDM).unwrap();
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_degraded(BootPhase::UefiHandover, DegradeReason::SafeFallback)
            .unwrap();
        let snap = m.current_snapshot();
        assert_eq!(snap.state, PhaseState::Degraded);
        assert_eq!(m.validate_transition(BootPhase::PagingSetup), Ok(()));
    }

    #[test]
    fn concurrent_capability_publication_loses_no_bits() {
        use std::sync::Arc;
        use std::thread;

        let m = Arc::new(fresh());
        m.register_protocol(BootProtocol::Uefi).unwrap();
        let a = Arc::clone(&m);
        let b = Arc::clone(&m);
        let ta = thread::spawn(move || a.set_capabilities(caps::MEMORY_MAP | caps::HHDM));
        let tb = thread::spawn(move || b.set_capabilities(caps::RSDP | caps::CMDLINE));
        ta.join().unwrap().unwrap();
        tb.join().unwrap().unwrap();
        assert!(m
            .capabilities()
            .contains(caps::MEMORY_MAP | caps::HHDM | caps::RSDP | caps::CMDLINE));
    }

    #[test]
    fn fatal_capsule_preserves_failed_and_last_completed_phase() {
        let m = fresh();
        m.register_protocol(BootProtocol::Uefi).unwrap();
        m.set_profile(BootProfile::Uefi);
        m.begin_phase(BootPhase::UefiHandover).unwrap();
        m.complete_phase(BootPhase::UefiHandover).unwrap();
        m.set_capabilities(caps::HHDM).unwrap();
        m.begin_phase(BootPhase::PagingSetup).unwrap();
        m.fail_phase(BootPhase::PagingSetup, PhaseError::Failed { code: 0x55 })
            .unwrap();
        m.recovery_transition().unwrap();
        let record = m.fatal_record().unwrap();
        assert_eq!(record.failed_phase, BootPhase::PagingSetup);
        assert_eq!(record.failed_state, PhaseState::Failed);
        assert_eq!(record.last_completed_phase, BootPhase::UefiHandover);
        assert_eq!(record.error_code, 0x55);
    }

    #[test]
    fn common_terminal_transition_reaches_running() {
        let m = fresh();
        run_to_services(&m);
        m.finish_boot(PhaseOutcome::Completed).unwrap();
        let snapshot = m.current_snapshot();
        assert_eq!(snapshot.phase, BootPhase::Running);
        assert_eq!(snapshot.state, PhaseState::Running);
        assert_eq!(m.verdict(), Verdict::Stop);
    }
}
