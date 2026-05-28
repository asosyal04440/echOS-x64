//! Energy Aware Scheduling (EAS) + ACPI CPPC skorlama modeli.
//!
//! Hibrit topolojilerde (P-core/E-core) görev yerleşimi için
//! performans/watt tabanlı seçim yapar.

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreKind {
    Performance,
    Efficiency,
}

#[derive(Clone, Copy, Debug)]
pub struct CppcPerfCaps {
    pub highest_perf: u16,
    pub nominal_perf: u16,
    pub lowest_perf: u16,
    pub energy_cost: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct EasCore {
    pub cpu_id: u32,
    pub kind: CoreKind,
    pub caps: CppcPerfCaps,
    pub utilization: u16,
    pub thread_director_bias: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct EasTask {
    pub task_id: u64,
    pub utilization: u16,
    pub latency_sensitive: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct EasPlacement {
    pub cpu_id: u32,
    pub score: i64,
}

fn perf_capacity(caps: &CppcPerfCaps) -> i64 {
    caps.highest_perf
        .max(caps.nominal_perf)
        .saturating_sub(caps.lowest_perf) as i64
}

fn base_energy_score(task: &EasTask, core: &EasCore) -> i64 {
    // Amaç: max (throughput / watt) ve latency-sensitive görevlerde
    // P-core lehine önyargı.
    let capacity = perf_capacity(&core.caps).max(1);
    let util_headroom = (1024i64 - core.utilization as i64).max(1);
    let task_weight = task.utilization as i64 + if task.latency_sensitive { 256 } else { 0 };
    let perf_term = capacity * task_weight;
    let energy_term = core.caps.energy_cost.max(1) as i64;
    let kind_bonus = match (task.latency_sensitive, core.kind) {
        (true, CoreKind::Performance) => capacity.saturating_mul(task_weight),
        (true, CoreKind::Efficiency) => -capacity.saturating_mul(task_weight / 2),
        (false, CoreKind::Performance) => -16,
        (false, CoreKind::Efficiency) => 24,
    };

    perf_term
        .saturating_mul(util_headroom)
        .saturating_div(energy_term)
        .saturating_add(kind_bonus)
        .saturating_add(core.thread_director_bias as i64)
}

pub fn select_energy_aware_cpu(task: &EasTask, cores: &[EasCore]) -> Option<EasPlacement> {
    let mut best: Option<EasPlacement> = None;
    for core in cores {
        let score = base_energy_score(task, core);
        match best {
            None => {
                best = Some(EasPlacement {
                    cpu_id: core.cpu_id,
                    score,
                })
            }
            Some(curr) => {
                if score > curr.score {
                    best = Some(EasPlacement {
                        cpu_id: core.cpu_id,
                        score,
                    });
                }
            }
        }
    }
    best
}

pub fn place_batch(tasks: &[EasTask], cores: &[EasCore]) -> Vec<(u64, EasPlacement)> {
    let mut result = Vec::new();
    for task in tasks {
        if let Some(p) = select_energy_aware_cpu(task, cores) {
            result.push((task.task_id, p));
        }
    }
    result
}
