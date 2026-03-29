pub const WINDOW_SESSION_CONTRACT_ROOTS: &[&str] = &["runtime_model", "runtime_state"];

pub use super::runtime_model::WindowSessionHandle;
pub use super::runtime_state::{attach_window_session, forget_window_session};
