//! Valkyrie-V compatibility bridge.
//!
//! Re-exports the (now hardware-backed) `valkyrie_virt` module so that callers
//! can use either `crate::valkyrie_virt::*` or `crate::valkyrie_bridge::*`.

pub use crate::valkyrie_virt::*;
