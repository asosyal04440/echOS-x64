//! # Hata Şiddet Seviyeleri
//!
//! Hata şiddetinin ve kurtarma sonuçlarının sınıflandırılması.

use super::FaultType;

// ============================================================================
// ŞİDDET SEVİYELERİ
// ============================================================================

/// Bir hatanın şiddet seviyesi
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Normal çalışma, hata yok
    Normal = 0,
    /// Küçük sorun, sistem tam işlevsel
    Warning = 1,
    /// Kritik olmayan modül arızası, kısıtlı çalışma
    Degraded = 2,
    /// Kritik modül arızası, sınırlı çalışma
    Critical = 3,
    /// Kurtarılamaz hata, acil kapatma
    Emergency = 4,
}

impl Default for Severity {
    fn default() -> Self {
        Severity::Normal
    }
}

impl Severity {
    /// Hata türünden şiddet seviyesini belirler
    pub fn from_type(fault_type: &FaultType) -> Self {
        match fault_type {
            // Acil - anında durdurma gerekli
            FaultType::HeapCorruption
            | FaultType::PmmCorruption
            | FaultType::IdtCorruption
            | FaultType::RunQueueCorruption => Severity::Emergency,

            // Kritik - sistem neredeyse işlevsiz
            FaultType::OutOfMemory
            | FaultType::ApStartupFailed
            | FaultType::CpuHung
            | FaultType::MetadataCorruption
            | FaultType::CanaryMismatch
            | FaultType::ThermalEvent => Severity::Critical,

            // Bozunmuş - azaltılmış işlevsellik
            FaultType::DoubleFree
            | FaultType::UseAfterFree
            | FaultType::TlbShootdownTimeout
            | FaultType::HandlerTimeout
            | FaultType::DeviceTimeout
            | FaultType::DeviceError
            | FaultType::JournalError
            | FaultType::AmlError => Severity::Degraded,

            // Uyarı - günlüğe yazılır, sistem devam eder
            FaultType::NullPointer
            | FaultType::InvalidPointer
            | FaultType::PageFault
            | FaultType::IrqStorm
            | FaultType::SpuriousInterrupt
            | FaultType::TaskLeak
            | FaultType::SocketLeak
            | FaultType::GpeStorm
            | FaultType::BootTimeout => Severity::Warning,

            // Bilinmeyen için varsayılan: uyarı
            _ => Severity::Warning,
        }
    }

    /// Hatanın anında müdahale gerektirip gerektirmediğini kontrol eder
    pub fn requires_immediate_action(&self) -> bool {
        matches!(self, Severity::Critical | Severity::Emergency)
    }

    /// Sistemin devam edip edemeyeceğini kontrol eder
    pub fn can_continue(&self) -> bool {
        !matches!(self, Severity::Emergency)
    }

    /// Kurtarmanın denenmesi gerekip gerekmediğini kontrol eder
    pub fn should_recover(&self) -> bool {
        matches!(
            self,
            Severity::Warning | Severity::Degraded | Severity::Critical
        )
    }

    /// İnsan tarafından okunabilir açıklama döndürür
    pub fn description(&self) -> &'static str {
        match self {
            Severity::Normal => "Normal çalışma",
            Severity::Warning => "Küçük sorun tespit edildi",
            Severity::Degraded => "Modül arızası, kısıtlı çalışma",
            Severity::Critical => "Kritik arıza, sınırlı çalışma",
            Severity::Emergency => "Kurtarılamaz hata, acil kapatma",
        }
    }

    /// Önerilen eylemi döndürür
    pub fn recommended_action(&self) -> RecommendedAction {
        match self {
            Severity::Normal => RecommendedAction::None,
            Severity::Warning => RecommendedAction::Log,
            Severity::Degraded => RecommendedAction::DisableModule,
            Severity::Critical => RecommendedAction::FallbackMode,
            Severity::Emergency => RecommendedAction::EmergencyHalt,
        }
    }
}

// ============================================================================
// ÖNERİLEN EYLEMLER
// ============================================================================

/// Bir hata için önerilen eylem
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendedAction {
    /// Eylem gerekmez
    None,
    /// Hatayı günlüğe yaz ve devam et
    Log,
    /// Hatalı modülü devre dışı bırak
    DisableModule,
    /// Yedek moda geç
    FallbackMode,
    /// Acil durdurma
    EmergencyHalt,
}

// ============================================================================
// KURTARMA SONUÇLARI
// ============================================================================

/// Kurtarma girişiminin sonucu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryResult {
    /// Hata tamamen kurtarıldı
    Recovered,
    /// Sistem bozunmuş modda
    Degraded,
    /// Kurtarma başarısız, modül devre dışı
    Failed,
    /// Sistem yeniden başlatma gerektirir
    RequiresReboot,
    /// Kurtarma mümkün değil
    Unrecoverable,
}

impl RecoveryResult {
    /// Kurtarmanın başarılı olup olmadığını kontrol eder
    pub fn is_success(&self) -> bool {
        matches!(self, RecoveryResult::Recovered | RecoveryResult::Degraded)
    }

    /// Sistemin devam edip edemeyeceğini kontrol eder
    pub fn can_continue(&self) -> bool {
        matches!(
            self,
            RecoveryResult::Recovered | RecoveryResult::Degraded | RecoveryResult::Failed
        )
    }

    /// Yeniden başlatma gerekip gerekmediğini kontrol eder
    pub fn needs_reboot(&self) -> bool {
        matches!(
            self,
            RecoveryResult::RequiresReboot | RecoveryResult::Unrecoverable
        )
    }
}

// ============================================================================
// KURTARMA SEVİYELERİ
// ============================================================================

/// Sistem kurtarma seviyesi (0-4)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoveryLevel {
    /// Normal çalışma
    Level0 = 0,
    /// Uyarı durumu
    Level1 = 1,
    /// Bozunmuş durum
    Level2 = 2,
    /// Kritik durum
    Level3 = 3,
    /// Acil durum
    Level4 = 4,
}

impl From<u32> for RecoveryLevel {
    fn from(value: u32) -> Self {
        match value {
            0 => RecoveryLevel::Level0,
            1 => RecoveryLevel::Level1,
            2 => RecoveryLevel::Level2,
            3 => RecoveryLevel::Level3,
            4 => RecoveryLevel::Level4,
            _ => RecoveryLevel::Level4,
        }
    }
}

impl RecoveryLevel {
    /// Mevcut seviyenin açıklamasını döndürür
    pub fn description(&self) -> &'static str {
        match self {
            RecoveryLevel::Level0 => "Normal çalışma",
            RecoveryLevel::Level1 => "Uyarı: Hatalar tespit edildi, sistem işlevsel",
            RecoveryLevel::Level2 => "Bozunmuş: Bazı modüller devre dışı",
            RecoveryLevel::Level3 => "Kritik: Yalnızca minimal işlevsellik",
            RecoveryLevel::Level4 => "Acil: Sistem durdurma yakın",
        }
    }

    /// Bu seviyede devre dışı bırakılması gereken modülleri döndürür
    pub fn disabled_modules(&self) -> &'static [&'static str] {
        match self {
            RecoveryLevel::Level0 => &[],
            RecoveryLevel::Level1 => &[],
            RecoveryLevel::Level2 => &["audio", "bluetooth", "gui"],
            RecoveryLevel::Level3 => &["audio", "bluetooth", "gui", "network", "usb"],
            RecoveryLevel::Level4 => &["audio", "bluetooth", "gui", "network", "usb", "fs_write"],
        }
    }

    /// Modülün bu seviyede aktif olup olmayacağını kontrol eder
    pub fn is_module_active(&self, module: &str) -> bool {
        !self.disabled_modules().contains(&module)
    }
}
