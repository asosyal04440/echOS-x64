#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelTaskingPlane {
    Scheduler,
    TaskModel,
    UserExec,
    WaitWake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelTaskingPlaneDescriptor {
    pub plane: KernelTaskingPlane,
    pub root: &'static str,
}

pub const KERNEL_TASKING_PLANES: &[KernelTaskingPlaneDescriptor] = &[
    KernelTaskingPlaneDescriptor {
        plane: KernelTaskingPlane::Scheduler,
        root: "task::scheduler",
    },
    KernelTaskingPlaneDescriptor {
        plane: KernelTaskingPlane::TaskModel,
        root: "task::task",
    },
    KernelTaskingPlaneDescriptor {
        plane: KernelTaskingPlane::UserExec,
        root: "task::user",
    },
    KernelTaskingPlaneDescriptor {
        plane: KernelTaskingPlane::WaitWake,
        root: "task::futex",
    },
];

pub const fn kernel_tasking_plane_root(plane: KernelTaskingPlane) -> &'static str {
    match plane {
        KernelTaskingPlane::Scheduler => "task::scheduler",
        KernelTaskingPlane::TaskModel => "task::task",
        KernelTaskingPlane::UserExec => "task::user",
        KernelTaskingPlane::WaitWake => "task::futex",
    }
}

pub use super::super::task::scheduler;
pub use super::super::task::task;
pub use super::super::task::user as user_exec;
pub use super::super::task::{
    sys_futex, sys_futex_waitv, sys_rseq, wait_on_address, wake_by_address_all,
    wake_by_address_single, Priority, TaskState,
};
