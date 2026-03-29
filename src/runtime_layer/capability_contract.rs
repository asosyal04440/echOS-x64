pub const CAPABILITY_CONTRACT_ROOTS: &[&str] = &["runtime_spawn", "runtime_state"];

pub use super::runtime_spawn::task_allows_native_capability;
pub use super::runtime_state::runtime_address_space_for_pid;
