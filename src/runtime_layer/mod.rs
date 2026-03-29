pub(crate) mod runtime_model;
pub(crate) mod runtime_registry;
pub(crate) mod runtime_spawn;
pub(crate) mod runtime_state;
pub(crate) mod service_control;

pub mod bootstrap_api;
pub mod capability_contract;
pub mod capture_client_contract;
pub mod clipboard_client_contract;
pub mod dialog_client_contract;
pub mod display_client_contract;
pub mod input_client_contract;
pub mod launch_contract;
pub mod native_scene_contract;
pub mod notification_client_contract;
pub mod package_registry_contract;
pub mod process_broker_contract;
pub mod runtime_api;
pub mod service_api;
pub mod service_endpoint_contract;
pub mod service_parity_contract;
pub mod shell_client_contract;
pub mod store_client_contract;
pub mod window_session_contract;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeLayerSurface {
    Launch,
    ProcessBroker,
    PackageRegistry,
    WindowSession,
    NativeScene,
    Capability,
    ServiceParity,
    ServiceEndpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLayerSurfaceDescriptor {
    pub surface: RuntimeLayerSurface,
    pub root: &'static str,
}

pub const RUNTIME_LAYER_SURFACE_REGISTRY: &[RuntimeLayerSurfaceDescriptor] = &[
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::Launch,
        root: "launch_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::ProcessBroker,
        root: "process_broker_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::PackageRegistry,
        root: "package_registry_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::WindowSession,
        root: "window_session_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::NativeScene,
        root: "native_scene_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::Capability,
        root: "capability_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::ServiceParity,
        root: "service_parity_contract",
    },
    RuntimeLayerSurfaceDescriptor {
        surface: RuntimeLayerSurface::ServiceEndpoint,
        root: "service_endpoint_contract",
    },
];

pub const fn runtime_layer_surface_root(surface: RuntimeLayerSurface) -> &'static str {
    match surface {
        RuntimeLayerSurface::Launch => "launch_contract",
        RuntimeLayerSurface::ProcessBroker => "process_broker_contract",
        RuntimeLayerSurface::PackageRegistry => "package_registry_contract",
        RuntimeLayerSurface::WindowSession => "window_session_contract",
        RuntimeLayerSurface::NativeScene => "native_scene_contract",
        RuntimeLayerSurface::Capability => "capability_contract",
        RuntimeLayerSurface::ServiceParity => "service_parity_contract",
        RuntimeLayerSurface::ServiceEndpoint => "service_endpoint_contract",
    }
}

pub use super::ipc::service_ipc;
