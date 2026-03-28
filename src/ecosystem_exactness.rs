use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExactnessSurfaceKind {
    Win32StubExport,
    Win32BehaviorBoundary,
    PosixUnsupported,
    IronShimUnsupported,
}

impl ExactnessSurfaceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Win32StubExport => "win32-stub-export",
            Self::Win32BehaviorBoundary => "win32-behavior-boundary",
            Self::PosixUnsupported => "posix-unsupported",
            Self::IronShimUnsupported => "ironshim-unsupported",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactnessCounter {
    pub kind: ExactnessSurfaceKind,
    pub subject: String,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactnessSnapshot {
    pub declared_win32_stub_exports: usize,
    pub declared_win32_stub_samples: Vec<String>,
    pub known_behavior_boundaries: Vec<String>,
    pub runtime_counters: Vec<ExactnessCounter>,
    pub strict_ready: bool,
}

static RUNTIME_COUNTERS: Mutex<BTreeMap<(ExactnessSurfaceKind, String), u64>> =
    Mutex::new(BTreeMap::new());

fn increment(kind: ExactnessSurfaceKind, subject: String) {
    let mut counters = RUNTIME_COUNTERS.lock();
    *counters.entry((kind, subject)).or_insert(0) += 1;
}

pub fn record_win32_stub(module: &str, function: &str) {
    increment(
        ExactnessSurfaceKind::Win32StubExport,
        format!("{module}!{function}"),
    );
}

pub fn record_win32_behavior_boundary(subject: &'static str) {
    increment(
        ExactnessSurfaceKind::Win32BehaviorBoundary,
        String::from(subject),
    );
}

pub fn record_posix_unsupported(subject: &'static str) {
    increment(
        ExactnessSurfaceKind::PosixUnsupported,
        String::from(subject),
    );
}

pub fn record_posix_unsupported_number(number: usize) {
    increment(
        ExactnessSurfaceKind::PosixUnsupported,
        format!("syscall#{number}"),
    );
}

pub fn record_ironshim_unsupported(subject: String) {
    increment(ExactnessSurfaceKind::IronShimUnsupported, subject);
}

pub fn snapshot() -> ExactnessSnapshot {
    let runtime_counters = {
        let counters = RUNTIME_COUNTERS.lock();
        counters
            .iter()
            .map(|((kind, subject), count)| ExactnessCounter {
                kind: *kind,
                subject: subject.clone(),
                count: *count,
            })
            .collect::<Vec<_>>()
    };
    let declared_stubbed_exports = crate::win32::declared_stubbed_exports(12);
    let known_behavior_boundaries = crate::win32::known_exactness_boundaries()
        .iter()
        .map(|(module, detail)| format!("{module}: {detail}"))
        .collect::<Vec<_>>();
    let declared_win32_stub_exports = crate::win32::declared_stubbed_export_count();
    ExactnessSnapshot {
        declared_win32_stub_exports,
        declared_win32_stub_samples: declared_stubbed_exports
            .into_iter()
            .map(|(module, function)| format!("{module}!{function}"))
            .collect(),
        known_behavior_boundaries: known_behavior_boundaries.clone(),
        strict_ready: declared_win32_stub_exports == 0
            && known_behavior_boundaries.is_empty()
            && runtime_counters.is_empty(),
        runtime_counters,
    }
}

#[cfg(test)]
pub fn reset_runtime_counters() {
    RUNTIME_COUNTERS.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::{
        record_ironshim_unsupported, record_posix_unsupported, record_win32_stub,
        reset_runtime_counters, snapshot, ExactnessSurfaceKind,
    };
    use alloc::string::String;

    #[test]
    fn runtime_counters_are_grouped_by_surface() {
        reset_runtime_counters();
        record_win32_stub("kernel32", "CreateFile2");
        record_win32_stub("kernel32", "CreateFile2");
        record_posix_unsupported("pread64");
        record_ironshim_unsupported(String::from("win32:req=99"));
        let snapshot = snapshot();
        assert!(
            snapshot
                .runtime_counters
                .iter()
                .any(|entry| entry.kind == ExactnessSurfaceKind::Win32StubExport
                    && entry.subject == "kernel32!CreateFile2"
                    && entry.count == 2)
        );
        assert!(
            snapshot
                .runtime_counters
                .iter()
                .any(|entry| entry.kind == ExactnessSurfaceKind::PosixUnsupported
                    && entry.subject == "pread64")
        );
        assert!(
            snapshot
                .runtime_counters
                .iter()
                .any(|entry| entry.kind == ExactnessSurfaceKind::IronShimUnsupported
                    && entry.subject == "win32:req=99")
        );
    }

    #[test]
    fn strict_snapshot_is_green_when_runtime_counters_are_clear() {
        reset_runtime_counters();
        let snapshot = snapshot();
        assert_eq!(snapshot.declared_win32_stub_exports, 0);
        assert!(snapshot.known_behavior_boundaries.is_empty());
        assert!(snapshot.runtime_counters.is_empty());
        assert!(snapshot.strict_ready);
    }

    #[test]
    fn strict_snapshot_fails_closed_on_runtime_counter() {
        reset_runtime_counters();
        record_posix_unsupported("poll");
        let snapshot = snapshot();
        assert!(!snapshot.strict_ready);
        assert!(
            snapshot
                .runtime_counters
                .iter()
                .any(|entry| entry.kind == ExactnessSurfaceKind::PosixUnsupported
                    && entry.subject == "poll"
                    && entry.count == 1)
        );
        reset_runtime_counters();
    }
}
