#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilitySurface {
    Posix,
    Win32,
    Win32Abi,
    PortableExecutable,
    Elf,
    IronshimApp,
    IronshimBridge,
    LinuxGlue,
    ShimLayer,
    Vdso,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatibilityDescriptor {
    pub surface: CompatibilitySurface,
    pub root: &'static str,
}

pub const COMPATIBILITY_REGISTRY: &[CompatibilityDescriptor] = &[
    CompatibilityDescriptor {
        surface: CompatibilitySurface::Posix,
        root: "posix",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::Win32,
        root: "win32",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::Win32Abi,
        root: "win32_abi",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::PortableExecutable,
        root: "pe_loader",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::Elf,
        root: "elf",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::IronshimApp,
        root: "ironshim_app",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::IronshimBridge,
        root: "ironshim_bridge",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::LinuxGlue,
        root: "linux_glue",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::ShimLayer,
        root: "shim_layer",
    },
    CompatibilityDescriptor {
        surface: CompatibilitySurface::Vdso,
        root: "vdso",
    },
];

pub const fn compatibility_surface_root(surface: CompatibilitySurface) -> &'static str {
    match surface {
        CompatibilitySurface::Posix => "posix",
        CompatibilitySurface::Win32 => "win32",
        CompatibilitySurface::Win32Abi => "win32_abi",
        CompatibilitySurface::PortableExecutable => "pe_loader",
        CompatibilitySurface::Elf => "elf",
        CompatibilitySurface::IronshimApp => "ironshim_app",
        CompatibilitySurface::IronshimBridge => "ironshim_bridge",
        CompatibilitySurface::LinuxGlue => "linux_glue",
        CompatibilitySurface::ShimLayer => "shim_layer",
        CompatibilitySurface::Vdso => "vdso",
    }
}

pub use super::elf;
pub use super::ironshim_app;
pub use super::ironshim_bridge;
pub use super::linux_glue;
pub use super::pe_loader;
pub use super::posix;
pub use super::shim_layer;
pub use super::vdso;
pub use super::win32;
pub use super::win32_abi;
