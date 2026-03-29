#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelMemoryPlane {
    Core,
    Paging,
    SharedRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelMemoryPlaneDescriptor {
    pub plane: KernelMemoryPlane,
    pub root: &'static str,
}

pub const KERNEL_MEMORY_PLANES: &[KernelMemoryPlaneDescriptor] = &[
    KernelMemoryPlaneDescriptor {
        plane: KernelMemoryPlane::Core,
        root: "memory",
    },
    KernelMemoryPlaneDescriptor {
        plane: KernelMemoryPlane::Paging,
        root: "memory::paging",
    },
    KernelMemoryPlaneDescriptor {
        plane: KernelMemoryPlane::SharedRegion,
        root: "memory::shared_region",
    },
];

pub const fn kernel_memory_plane_root(plane: KernelMemoryPlane) -> &'static str {
    match plane {
        KernelMemoryPlane::Core => "memory",
        KernelMemoryPlane::Paging => "memory::paging",
        KernelMemoryPlane::SharedRegion => "memory::shared_region",
    }
}

pub use super::super::memory::*;
pub use super::super::memory::{paging, shared_region};
