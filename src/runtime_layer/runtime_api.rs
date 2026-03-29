//! Compatibility shell over the narrower runtime-layer contracts.
pub const RUNTIME_API_COMPATIBILITY_SURFACES: &[&str] = &[
    "capability_contract",
    "launch_contract",
    "native_scene_contract",
    "package_registry_contract",
    "process_broker_contract",
];

#[deprecated(
    note = "use process_broker_contract, window_session_contract, native_scene_contract, or capability_contract instead of runtime_api runtime-state helpers"
)]
pub use super::capability_contract::{
    runtime_address_space_for_pid, task_allows_native_capability,
};
#[deprecated(
    note = "use launch_contract or capability_contract instead of runtime_api runtime-spawn helpers"
)]
pub use super::launch_contract::{
    service_process_available, spawn_elf_runtime, spawn_native_runtime, spawn_pe_runtime,
    spawn_service_process_runtime, spawn_service_runtime,
};
#[deprecated(
    note = "use launch_contract, process_broker_contract, package_registry_contract, window_session_contract, or capability_contract as appropriate"
)]
pub use super::launch_contract::{
    BrokeredLaunch, CapabilityToken, CapabilityTokenId, IsolationDomain, ProcessBrokerTicket,
    RuntimeHandle, RuntimeHandleId,
};
#[deprecated(
    note = "use process_broker_contract, window_session_contract, native_scene_contract, or capability_contract instead of runtime_api runtime-state helpers"
)]
pub use super::native_scene_contract::{
    attach_window_session, forget_window_session, runtime_handle_for_task,
};
#[deprecated(
    note = "use package_registry_contract::PackageRegistryEntry instead of runtime_api::PackageRegistryEntry"
)]
pub use super::package_registry_contract::PackageRegistryEntry;
#[deprecated(
    note = "use package_registry_contract::RuntimePackageRegistry instead of runtime_api::RuntimePackageRegistry"
)]
pub use super::package_registry_contract::RuntimePackageRegistry;
#[deprecated(
    note = "use process_broker_contract, window_session_contract, native_scene_contract, or capability_contract instead of runtime_api runtime-state helpers"
)]
pub use super::process_broker_contract::{
    brokered_launch, brokered_launch_children, runtime_handle_for_service, ProcessBroker,
};
