//! # echOS Task Modülü
//! 
//! Preemptive multitasking altyapısı.
//! Task yapısı, scheduler ve context switch.

/// Task yapısı ve context
pub mod task;

/// Priority-based aging scheduler
pub mod scheduler;

/// User mode task desteği
pub mod user;

pub use task::Priority;
pub use scheduler::{spawn, spawn_with_priority, schedule, init as init_scheduler, sleep, exit, get_ticks};
