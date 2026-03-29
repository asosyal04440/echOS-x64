#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelArchPlane {
    Cpu,
    Gdt,
    Interrupts,
    Syscall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelArchPlaneDescriptor {
    pub plane: KernelArchPlane,
    pub root: &'static str,
}

pub const KERNEL_ARCH_PLANES: &[KernelArchPlaneDescriptor] = &[
    KernelArchPlaneDescriptor {
        plane: KernelArchPlane::Cpu,
        root: "cpu",
    },
    KernelArchPlaneDescriptor {
        plane: KernelArchPlane::Gdt,
        root: "gdt",
    },
    KernelArchPlaneDescriptor {
        plane: KernelArchPlane::Interrupts,
        root: "interrupts",
    },
    KernelArchPlaneDescriptor {
        plane: KernelArchPlane::Syscall,
        root: "syscall",
    },
];

pub const fn kernel_arch_plane_root(plane: KernelArchPlane) -> &'static str {
    match plane {
        KernelArchPlane::Cpu => "cpu",
        KernelArchPlane::Gdt => "gdt",
        KernelArchPlane::Interrupts => "interrupts",
        KernelArchPlane::Syscall => "syscall",
    }
}

pub use super::super::cpu;
pub use super::super::gdt;
pub use super::super::interrupts;
pub use super::super::syscall;
