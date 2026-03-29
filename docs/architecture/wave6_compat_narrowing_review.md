# Wave 6 Compat Narrowing Review

Date: 2026-03-29

## Cut

Wave 6 narrowed the remaining compat-side POSIX bridge into three explicit ABI behavior modules:

- `src/posix/native_scene_bridge.rs`
- `src/posix/service_bridge.rs`
- `src/posix/process_bridge.rs`

The root `src/posix.rs` syscall dispatcher now routes:

- native scene, window, clipboard, notification, and event syscalls into `native_scene_bridge`
- service bootstrap, status, parity, endpoint, and notification-service ABI syscalls into `service_bridge`
- process, exec, fork, wait, clone, and session/identity syscalls into `process_bridge`

## Why This Cut

`src/posix.rs` was still a compat junk drawer even after the earlier Windows runtime/image extraction.
Wave 6 needed a real compat consumer-narrowing move, not another rename.

The remaining POSIX bridge pressure fell into three different ABI personalities:

- native scene/window/event ABI
- service bootstrap/status/parity/endpoint ABI
- process/exec/fork/session ABI

Those paths have different runtime-layer contract targets, different ownership pressure, and different future crate-split implications.
Keeping them inline would have preserved a fake modular root even though Wave 1-5 had already erected the broader namespace and contract spine.

## Metrics

After the split and baseline refresh:

- `current_wave = 6`
- `violations = 0`
- `expired_debt = 0`
- tracked `legacy_references = 654`
- tracked `compat = 191`
- `src/posix.rs = 56`
- `src/posix/native_scene_bridge.rs = 28`
- `src/posix/service_bridge.rs = 38`
- `src/posix/process_bridge.rs = 7`

Interpretation:

- the POSIX root hotspot fell from `61` to `56`
- the old broad service/runtime bridge identity was retired; the service-facing slice is now explicitly measured as `service_bridge`
- tracked compat totals rose slightly because the new `process_bridge` module is now visible instead of hiding inside the root file
- this is still a ratchet win because broad root ownership went down and no new broad facade ingress was introduced

## Remaining Post-Wave-6 Pressure

Wave 6 closes the compat consumer-narrowing plan item, but it does not eliminate all compat coupling.
The next architectural pressure points are:

- reducing direct `crate::ipc` ingress inside `src/posix/native_scene_bridge.rs`
- shrinking the broad syscall surface still owned by `src/posix.rs`
- continuing split-readiness work around the hottest remaining product and compat consumers

These are post-Wave-6 follow-on concerns, not blockers for the Wave 6 exit itself.

## Validation

- `cargo check --target x86_64-pc-windows-msvc --lib -q`
- `cargo arch-guard --refresh-baseline --report`
- `cargo arch-guard --check --report`
- `cargo test --target x86_64-pc-windows-msvc --lib gui::launch_pipeline::tests::launch_session_projects_runtime_bootstrap_and_event_loop -- --exact`
- `cargo test --target x86_64-pc-windows-msvc --lib ipc::service_ipc::tests::bootstrap_fallback_still_returns_direct_response_before_runtime_task -- --exact`
- `.\scripts\wave1_gate.ps1`
- `.\.qoder\skills\echos-architect\scripts\echos_gate.ps1 -Paths src\posix.rs,src\posix\native_scene_bridge.rs,src\posix\service_bridge.rs,src\posix\process_bridge.rs,docs\architecture\arch_rules.toml,docs\architecture\arch_baseline.json,docs\architecture\wave6_compat_narrowing_review.md,docs\architecture\wave6_closeout_review.md,docs\agent\decision-log.md -Json`
