#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use echos_manifest::STATE_EXPORT_INLINE_LIMIT_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResumeReason {
    ColdStart,
    FaultRecovery,
    SuspendRestore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Inline(Vec<u8>),
    ResumeRef(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    StateTooLarge,
    InvalidResumeRef,
}

impl RuntimeState {
    pub fn inline(bytes: &[u8]) -> Result<Self, RuntimeError> {
        if bytes.len() > STATE_EXPORT_INLINE_LIMIT_BYTES {
            return Err(RuntimeError::StateTooLarge);
        }
        Ok(Self::Inline(bytes.to_vec()))
    }

    pub fn resume_ref(path: &str) -> Result<Self, RuntimeError> {
        if !is_valid_resume_ref(path) {
            return Err(RuntimeError::InvalidResumeRef);
        }
        Ok(Self::ResumeRef(path.to_string()))
    }
}

pub fn validate_runtime_state(state: &RuntimeState) -> Result<(), RuntimeError> {
    match state {
        RuntimeState::Inline(bytes) => {
            if bytes.len() > STATE_EXPORT_INLINE_LIMIT_BYTES {
                Err(RuntimeError::StateTooLarge)
            } else {
                Ok(())
            }
        }
        RuntimeState::ResumeRef(path) => {
            if is_valid_resume_ref(path) {
                Ok(())
            } else {
                Err(RuntimeError::InvalidResumeRef)
            }
        }
    }
}

pub fn is_valid_resume_ref(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains('\0') {
        return false;
    }
    path.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::{validate_runtime_state, RuntimeError, RuntimeState};
    use alloc::vec;

    #[test]
    fn inline_state_respects_limit() {
        assert!(RuntimeState::inline(&vec![1u8; 64]).is_ok());
        assert_eq!(
            RuntimeState::inline(&vec![0u8; super::STATE_EXPORT_INLINE_LIMIT_BYTES + 1]),
            Err(RuntimeError::StateTooLarge)
        );
    }

    #[test]
    fn resume_ref_rejects_absolute_or_parent_paths() {
        assert!(validate_runtime_state(
            &RuntimeState::resume_ref("resume/state.bin").expect("valid")
        )
        .is_ok());
        assert_eq!(
            RuntimeState::resume_ref("../state.bin"),
            Err(RuntimeError::InvalidResumeRef)
        );
        assert_eq!(
            RuntimeState::resume_ref("/tmp/state.bin"),
            Err(RuntimeError::InvalidResumeRef)
        );
    }
}
