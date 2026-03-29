#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubsystemClass {
    KernelAdjacentPlatformIo,
    ReusableSubsystem,
    ProductRuntimeFacing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubsystemDescriptor {
    pub name: &'static str,
    pub class: SubsystemClass,
    pub root: &'static str,
}

pub const SUBSYSTEM_REGISTRY: &[SubsystemDescriptor] = &[
    SubsystemDescriptor {
        name: "drivers",
        class: SubsystemClass::KernelAdjacentPlatformIo,
        root: "drivers",
    },
    SubsystemDescriptor {
        name: "gpu3d",
        class: SubsystemClass::KernelAdjacentPlatformIo,
        root: "gpu3d",
    },
    SubsystemDescriptor {
        name: "fs",
        class: SubsystemClass::ReusableSubsystem,
        root: "fs",
    },
    SubsystemDescriptor {
        name: "net",
        class: SubsystemClass::ReusableSubsystem,
        root: "net",
    },
    SubsystemDescriptor {
        name: "crypto",
        class: SubsystemClass::ReusableSubsystem,
        root: "crypto",
    },
    SubsystemDescriptor {
        name: "audio",
        class: SubsystemClass::ReusableSubsystem,
        root: "audio",
    },
    SubsystemDescriptor {
        name: "ml",
        class: SubsystemClass::ProductRuntimeFacing,
        root: "ml",
    },
];

pub fn subsystem_root(name: &str) -> Option<&'static str> {
    match name {
        "drivers" => Some("drivers"),
        "gpu3d" => Some("gpu3d"),
        "fs" => Some("fs"),
        "net" => Some("net"),
        "crypto" => Some("crypto"),
        "audio" => Some("audio"),
        "ml" => Some("ml"),
        _ => None,
    }
}

pub fn subsystem_class(name: &str) -> Option<SubsystemClass> {
    match name {
        "drivers" | "gpu3d" => Some(SubsystemClass::KernelAdjacentPlatformIo),
        "fs" | "net" | "crypto" | "audio" => Some(SubsystemClass::ReusableSubsystem),
        "ml" => Some(SubsystemClass::ProductRuntimeFacing),
        _ => None,
    }
}

pub use super::audio;
pub use super::crypto;
pub use super::drivers;
pub use super::fs;
pub use super::gpu3d;
pub use super::ml;
pub use super::net;
