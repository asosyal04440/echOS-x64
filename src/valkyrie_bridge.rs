//! Valkyrie-V compatibility facade.
//!
//! `valkyrie_virt` already owns the actual hypervisor implementation.
//! This module preserves the feature-gated public surface expected by the
//! crate layout and external callers that look for `crate::valkyrie_bridge`.

pub use crate::valkyrie_virt::*;
