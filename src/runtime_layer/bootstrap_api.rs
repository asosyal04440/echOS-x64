//! Compatibility shell over launch/process/capability contracts.
pub const BOOTSTRAP_API_COMPATIBILITY_SURFACES: &[&str] = &[
    "capability_contract",
    "launch_contract",
    "process_broker_contract",
    "runtime_state",
];

#[deprecated(
    note = "use launch_contract, process_broker_contract, native_scene_contract, or capability_contract instead of bootstrap_api state helpers"
)]
pub use super::capability_contract::runtime_address_space_for_pid;
#[deprecated(
    note = "use capability_contract::task_allows_native_capability instead of bootstrap_api"
)]
pub use super::capability_contract::task_allows_native_capability;
#[deprecated(
    note = "use launch_contract, process_broker_contract, native_scene_contract, or capability_contract instead of bootstrap_api state helpers"
)]
pub use super::launch_contract::{register_launch_session, register_launch_session_with_parent};
#[deprecated(
    note = "use launch_contract or process_broker_contract instead of bootstrap_api type re-exports"
)]
pub use super::launch_contract::{
    BrokeredLaunch, IsolationDomain, ProcessBrokerTicket, RuntimeHandle, RuntimeHandleId,
};
#[deprecated(
    note = "use launch_contract, process_broker_contract, native_scene_contract, or capability_contract instead of bootstrap_api state helpers"
)]
pub use super::process_broker_contract::{
    brokered_launch, brokered_launch_children, runtime_handle_for_service,
};
#[deprecated(
    note = "bootstrap_api legacy-only state helpers still proxy raw runtime_state; prefer contracts or runtime compatibility only when absolutely required"
)]
pub use super::runtime_state::{annotate_runtime_handle, runtime_handle, runtime_handle_for_task};
