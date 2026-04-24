pub const NATIVE_SCENE_CONTRACT_ROOTS: &[&str] = &["runtime_model", "runtime_state"];

pub use super::runtime_model::{IsolationDomain, RuntimeHandle};
pub use super::runtime_state::{
    attach_window_session, attach_window_session_with_display, forget_window_session,
    runtime_handle_for_service, runtime_handle_for_task,
};
