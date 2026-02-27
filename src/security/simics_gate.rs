use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateMode {
    Day1HardBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestAxis {
    BootIrqInputSmoke,
    SyscallWinServerSecurity,
    FsNetworkHealth,
    PerformanceBaseline,
    ExtremeIronShimVmEscapeMemoryFuzz,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisVerdict {
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug)]
pub struct ThresholdSet {
    pub timeout_ms: u32,
    pub max_waker_latency_us: u32,
    pub max_input_latency_us: u32,
    pub max_frame_jitter_us: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct GateRule {
    pub axis: TestAxis,
    pub required_markers: &'static [&'static str],
    pub forbidden_markers: &'static [&'static str],
    pub thresholds: ThresholdSet,
}

#[derive(Clone, Debug)]
pub struct VerdictRecord {
    pub axis: TestAxis,
    pub verdict: AxisVerdict,
    pub reason: &'static str,
}

#[derive(Clone, Debug)]
pub struct GateReport {
    pub mode: GateMode,
    pub records: Vec<VerdictRecord>,
}

impl GateReport {
    pub fn new(mode: GateMode) -> Self {
        Self {
            mode,
            records: Vec::new(),
        }
    }

    pub fn push(&mut self, axis: TestAxis, verdict: AxisVerdict, reason: &'static str) {
        self.records.push(VerdictRecord {
            axis,
            verdict,
            reason,
        });
    }

    pub fn should_block_merge(&self) -> bool {
        match self.mode {
            GateMode::Day1HardBlock => self.records.iter().any(|record| record.verdict == AxisVerdict::Fail),
        }
    }
}

const RULE_BOOT_IRQ_INPUT: GateRule = GateRule {
    axis: TestAxis::BootIrqInputSmoke,
    required_markers: &[
        "[INT] Interrupts enabled",
        "[BOOT] Starting GUI compositor...",
        "Mouse: Başarıyla başlatıldı",
        "[COMPOSITOR] Ana döngü başlatıldı",
    ],
    forbidden_markers: &["[PANIC]", "deadlock", "spinlock stuck"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

const RULE_SYSCALL_SECURITY: GateRule = GateRule {
    axis: TestAxis::SyscallWinServerSecurity,
    required_markers: &[
        "[WINSRV]",
        "ownership check",
        "user-range",
    ],
    forbidden_markers: &[
        "kernel pointer leak",
        "invalid user pointer accepted",
    ],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

const RULE_FS_NETWORK: GateRule = GateRule {
    axis: TestAxis::FsNetworkHealth,
    required_markers: &[
        "[NET]",
        "FAT",
        "filesystem",
    ],
    forbidden_markers: &["fs corruption", "network deadlock"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

const RULE_PERFORMANCE: GateRule = GateRule {
    axis: TestAxis::PerformanceBaseline,
    required_markers: &[
        "[PERF]",
        "frame",
        "latency",
    ],
    forbidden_markers: &["latency regression", "frame pacing violation"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 1_000,
        max_input_latency_us: 4_000,
        max_frame_jitter_us: 10_000,
    },
};

const RULE_EXTREME: GateRule = GateRule {
    axis: TestAxis::ExtremeIronShimVmEscapeMemoryFuzz,
    required_markers: &[
        "[IRONSHIM]",
        "fuzz",
        "ring3->ring0 blocked",
    ],
    forbidden_markers: &[
        "vm escape success",
        "ring0 overwrite",
        "oom queue collapse",
    ],
    thresholds: ThresholdSet {
        timeout_ms: 180_000,
        max_waker_latency_us: 1_000,
        max_input_latency_us: 4_000,
        max_frame_jitter_us: 10_000,
    },
};

pub fn aggressive_day1_rules() -> &'static [GateRule] {
    &[
        RULE_BOOT_IRQ_INPUT,
        RULE_SYSCALL_SECURITY,
        RULE_FS_NETWORK,
        RULE_PERFORMANCE,
        RULE_EXTREME,
    ]
}
