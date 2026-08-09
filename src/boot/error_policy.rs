//! Boot initialization error policy.
//!
//! Wave 5 keeps error handling explicit at the adapter/common-pipeline
//! boundary.  The policy is intentionally allocation-free so it can be used
//! while the bootstrap heap is still being established.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootErrorDisposition {
    /// The invariant required by the next phase is not satisfied.
    Fatal,
    /// Continue with a bounded, documented safe fallback.
    DegradedSafe,
    /// The feature is not provided by this platform/protocol.
    Unsupported,
    /// The operation may be attempted again by its owner.
    Retryable,
    /// The feature was deliberately disabled by policy/configuration.
    Disabled,
}

impl BootErrorDisposition {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fatal => "fatal",
            Self::DegradedSafe => "degraded-safe",
            Self::Unsupported => "unsupported",
            Self::Retryable => "retryable",
            Self::Disabled => "disabled",
        }
    }

    pub const fn continues_boot(self) -> bool {
        !matches!(self, Self::Fatal)
    }
}

/// Policy used for platform DMA isolation.
///
/// `Preferred` is the default: it never silently enables an unisolated
/// device; the adapter records a degraded decision and its device policy
/// remains constrained.  `Permissive` is only selected by an explicit
/// `iommu=permissive` command-line token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IommuPolicy {
    Required,
    Preferred,
    Permissive,
    Unavailable,
}

impl IommuPolicy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Preferred => "preferred",
            Self::Permissive => "permissive",
            Self::Unavailable => "unavailable",
        }
    }

    pub const fn default_policy() -> Self {
        Self::Preferred
    }

    /// Parse one canonical `iommu=<mode>` token.  Unknown values intentionally
    /// fall back to `Preferred`; the caller logs the effective policy.
    pub fn from_cmdline(cmdline: Option<&str>) -> Self {
        let Some(cmdline) = cmdline else {
            return Self::default_policy();
        };
        for token in cmdline.split_ascii_whitespace() {
            let Some(value) = token.strip_prefix("iommu=") else {
                continue;
            };
            return match value {
                "required" => Self::Required,
                "preferred" => Self::Preferred,
                "permissive" => Self::Permissive,
                "unavailable" => Self::Unavailable,
                _ => Self::default_policy(),
            };
        }
        Self::default_policy()
    }

    pub const fn failure_disposition(self) -> BootErrorDisposition {
        match self {
            Self::Required => BootErrorDisposition::Fatal,
            Self::Preferred => BootErrorDisposition::DegradedSafe,
            Self::Permissive => BootErrorDisposition::Disabled,
            Self::Unavailable => BootErrorDisposition::Unsupported,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BootErrorDisposition, IommuPolicy};

    #[test]
    fn explicit_iommu_policy_is_deterministic() {
        assert_eq!(
            IommuPolicy::from_cmdline(Some("quiet iommu=required")),
            IommuPolicy::Required
        );
        assert_eq!(
            IommuPolicy::from_cmdline(Some("iommu=permissive")),
            IommuPolicy::Permissive
        );
        assert_eq!(
            IommuPolicy::from_cmdline(Some("iommu=invalid")),
            IommuPolicy::Preferred
        );
        assert_eq!(
            IommuPolicy::Required.failure_disposition(),
            BootErrorDisposition::Fatal
        );
        assert_eq!(
            IommuPolicy::Preferred.failure_disposition(),
            BootErrorDisposition::DegradedSafe
        );
        assert_eq!(
            IommuPolicy::Permissive.failure_disposition(),
            BootErrorDisposition::Disabled
        );
        assert_eq!(
            IommuPolicy::Unavailable.failure_disposition(),
            BootErrorDisposition::Unsupported
        );
        assert!(BootErrorDisposition::Unsupported.continues_boot());
    }
}
