pub mod arch;
pub mod memory;
pub mod tasking;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelDomain {
    Boot,
    Arch,
    Memory,
    Tasking,
    Ipc,
    Preempt,
    Rcu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelDomainDescriptor {
    pub domain: KernelDomain,
    pub root: &'static str,
}

pub const KERNEL_DOMAIN_REGISTRY: &[KernelDomainDescriptor] = &[
    KernelDomainDescriptor {
        domain: KernelDomain::Boot,
        root: "boot",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Arch,
        root: "kernel::arch",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Memory,
        root: "kernel::memory",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Tasking,
        root: "kernel::tasking",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Ipc,
        root: "ipc",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Preempt,
        root: "preempt",
    },
    KernelDomainDescriptor {
        domain: KernelDomain::Rcu,
        root: "rcu",
    },
];

pub const fn kernel_domain_root(domain: KernelDomain) -> &'static str {
    match domain {
        KernelDomain::Boot => "boot",
        KernelDomain::Arch => "kernel::arch",
        KernelDomain::Memory => "kernel::memory",
        KernelDomain::Tasking => "kernel::tasking",
        KernelDomain::Ipc => "ipc",
        KernelDomain::Preempt => "preempt",
        KernelDomain::Rcu => "rcu",
    }
}

pub use super::boot;
pub use super::ipc;
pub use super::preempt;
pub use super::rcu;
pub use arch::{cpu, gdt, interrupts, syscall};
pub use tasking::{scheduler, task, user_exec};
