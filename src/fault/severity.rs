//! # Fault Severity Levels
//!
//! Classification of fault severity and recovery outcomes.

use super::FaultType;

// ============================================================================
// SEVERITY LEVELS
// ============================================================================

/// Severity level of a fault
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Normal operation, no fault
    Normal = 0,
    /// Minor issue, system fully functional
    Warning = 1,
    /// Non-critical module failure, degraded operation
    Degraded = 2,
    /// Critical module failure, limited operation
    Critical = 3,
    /// Unrecoverable fault, emergency shutdown
    Emergency = 4,
}

impl Default for Severity {
    fn default() -> Self {
        Severity::Normal
    }
}

impl Severity {
    /// Determine severity from fault type
    pub fn from_type(fault_type: &FaultType) -> Self {
        match fault_type {
            // Emergency - immediate halt required
            FaultType::HeapCorruption
            | FaultType::PmmCorruption
            | FaultType::IdtCorruption
            | FaultType::RunQueueCorruption => Severity::Emergency,
            
            // Critical - system can barely function
            FaultType::OutOfMemory
            | FaultType::ApStartupFailed
            | FaultType::CpuHung
            | FaultType::MetadataCorruption
            | FaultType::CanaryMismatch
            | FaultType::ThermalEvent => Severity::Critical,
            
            // Degraded - reduced functionality
            FaultType::DoubleFree
            | FaultType::UseAfterFree
            | FaultType::TlbShootdownTimeout
            | FaultType::HandlerTimeout
            | FaultType::DeviceTimeout
            | FaultType::DeviceError
            | FaultType::JournalError
            | FaultType::AmlError => Severity::Degraded,
            
            // Warning - logged but system continues
            FaultType::NullPointer
            | FaultType::InvalidPointer
            | FaultType::PageFault
            | FaultType::IrqStorm
            | FaultType::SpuriousInterrupt
            | FaultType::TaskLeak
            | FaultType::SocketLeak
            | FaultType::GpeStorm
            | FaultType::BootTimeout => Severity::Warning,
            
            // Default to warning for unknown
            _ => Severity::Warning,
        }
    }
    
    /// Check if fault requires immediate action
    pub fn requires_immediate_action(&self) -> bool {
        matches!(self, Severity::Critical | Severity::Emergency)
    }
    
    /// Check if system can continue
    pub fn can_continue(&self) -> bool {
        !matches!(self, Severity::Emergency)
    }
    
    /// Check if recovery should be attempted
    pub fn should_recover(&self) -> bool {
        matches!(self, Severity::Warning | Severity::Degraded | Severity::Critical)
    }
    
    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Severity::Normal => "Normal operation",
            Severity::Warning => "Minor issue detected",
            Severity::Degraded => "Module failure, degraded operation",
            Severity::Critical => "Critical failure, limited operation",
            Severity::Emergency => "Unrecoverable fault, emergency shutdown",
        }
    }
    
    /// Get recommended action
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
// RECOMMENDED ACTIONS
// ============================================================================

/// Recommended action for a fault
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendedAction {
    /// No action needed
    None,
    /// Log the fault and continue
    Log,
    /// Disable the faulty module
    DisableModule,
    /// Switch to fallback mode
    FallbackMode,
    /// Emergency halt
    EmergencyHalt,
}

// ============================================================================
// RECOVERY RESULTS
// ============================================================================

/// Result of a recovery attempt
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryResult {
    /// Fault was fully recovered
    Recovered,
    /// System is in degraded mode
    Degraded,
    /// Recovery failed, module disabled
    Failed,
    /// System reboot required
    RequiresReboot,
    /// Recovery not possible
    Unrecoverable,
}

impl RecoveryResult {
    /// Check if recovery was successful
    pub fn is_success(&self) -> bool {
        matches!(self, RecoveryResult::Recovered | RecoveryResult::Degraded)
    }
    
    /// Check if system can continue
    pub fn can_continue(&self) -> bool {
        matches!(self, RecoveryResult::Recovered | RecoveryResult::Degraded | RecoveryResult::Failed)
    }
    
    /// Check if reboot is needed
    pub fn needs_reboot(&self) -> bool {
        matches!(self, RecoveryResult::RequiresReboot | RecoveryResult::Unrecoverable)
    }
}

// ============================================================================
// RECOVERY LEVELS
// ============================================================================

/// System recovery level (0-4)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecoveryLevel {
    /// Normal operation
    Level0 = 0,
    /// Warning state
    Level1 = 1,
    /// Degraded state
    Level2 = 2,
    /// Critical state
    Level3 = 3,
    /// Emergency state
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
    /// Get description of current level
    pub fn description(&self) -> &'static str {
        match self {
            RecoveryLevel::Level0 => "Normal operation",
            RecoveryLevel::Level1 => "Warning: Faults detected but system functional",
            RecoveryLevel::Level2 => "Degraded: Some modules disabled",
            RecoveryLevel::Level3 => "Critical: Minimal functionality only",
            RecoveryLevel::Level4 => "Emergency: System halt imminent",
        }
    }
    
    /// Get modules that should be disabled at this level
    pub fn disabled_modules(&self) -> &'static [&'static str] {
        match self {
            RecoveryLevel::Level0 => &[],
            RecoveryLevel::Level1 => &[],
            RecoveryLevel::Level2 => &["audio", "bluetooth", "gui"],
            RecoveryLevel::Level3 => &["audio", "bluetooth", "gui", "network", "usb"],
            RecoveryLevel::Level4 => &["audio", "bluetooth", "gui", "network", "usb", "fs_write"],
        }
    }
    
    /// Check if module should be active at this level
    pub fn is_module_active(&self, module: &str) -> bool {
        !self.disabled_modules().contains(&module)
    }
}
