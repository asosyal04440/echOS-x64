# Wave 6 Closeout Review

Date: 2026-03-29

## Exit Status

Wave 6 is closed.

The compat consumer-narrowing objective for this wave was to stop treating the POSIX bridge as a single mixed ABI surface and to move the remaining broad compat ingress behind physically distinct ownership seams.
That objective is complete.

## What Closed

- `src/posix/native_scene_bridge.rs` owns native scene/window/event/clipboard/notification ABI flow
- `src/posix/service_bridge.rs` owns service bootstrap/status/parity/endpoint ABI flow
- `src/posix/process_bridge.rs` owns process/exec/fork/wait/clone/session ABI flow
- `src/posix.rs` is now a thinner syscall dispatcher and re-export shell instead of the direct owner of all three concern groups

## Exit Metrics

- `current_wave = 6`
- `violations = 0`
- `expired_debt = 0`
- tracked `legacy_references = 654`
- tracked `compat = 191`
- `src/posix.rs 61 -> 56`
- `src/posix/service_bridge.rs = 38`
- `src/posix/native_scene_bridge.rs = 28`
- `src/posix/process_bridge.rs = 7`

## Why This Counts As Wave 6 Closure

Wave 6 was a compat consumer-narrowing wave, not a crate split and not a full POSIX rewrite.
The success condition was to replace broad mixed compat ownership with ABI-specific seams that can be measured independently by the ratchet and the architecture guard.

That is now true:

- broad root ownership in `src/posix.rs` decreased
- the old mixed bridge identity no longer exists as a single file
- guard coverage includes each new compat slice
- no new broad runtime facade ingress was introduced
- the ratchet remains green with `violations = 0` and `expired_debt = 0`

## Honest Remaining Pressure

Wave 6 closure does not imply compat is clean enough for crate split today.
The main remaining pressure is:

- direct `crate::ipc` ingress inside `src/posix/native_scene_bridge.rs`
- residual broad syscall dispatch surface in `src/posix.rs`
- cross-cutting split-readiness blockers still led by product and runtime hotspots

Those are next-wave concerns rather than reasons to keep Wave 6 open.

## Validation

- `cargo check --target x86_64-pc-windows-msvc --lib -q`
- `cargo arch-guard --refresh-baseline --report`
- `cargo arch-guard --check --report`
- `cargo test --target x86_64-pc-windows-msvc --lib gui::launch_pipeline::tests::launch_session_projects_runtime_bootstrap_and_event_loop -- --exact`
- `cargo test --target x86_64-pc-windows-msvc --lib ipc::service_ipc::tests::bootstrap_fallback_still_returns_direct_response_before_runtime_task -- --exact`
- `.\scripts\wave1_gate.ps1`
- `.\.qoder\skills\echos-architect\scripts\echos_gate.ps1 -Paths src\posix.rs,src\posix\native_scene_bridge.rs,src\posix\service_bridge.rs,src\posix\process_bridge.rs,docs\architecture\arch_rules.toml,docs\architecture\arch_baseline.json,docs\architecture\wave6_compat_narrowing_review.md,docs\architecture\wave6_closeout_review.md,docs\agent\decision-log.md -Json`
