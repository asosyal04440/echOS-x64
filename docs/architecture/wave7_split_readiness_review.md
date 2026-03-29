# Wave 7 Split Readiness Review

Date: 2026-03-29

## Decision

- `managed legacy backlog = 0`
- `crate split = decision-ready`

## Guard State

- `current_wave = 7`
- `managed_files = 54`
- `violations = 0`
- `expired_debt = 0`
- `active_debt = 0`
- `active_exceptions = 0`
- `legacy_references = 0`
- broad facade references recorded by the ratchet baseline: `0`

## Wave 7 Outcome

Wave 7 finished as an honest repo-scope managed cleanup, not just a wave-local closeout.

The important structural result is now measurable:

- `src/runtime_layer/runtime_api.rs`, `src/runtime_layer/bootstrap_api.rs`, and `src/runtime_layer/service_api.rs` remain contract-backed compatibility shells rather than raw implementation aliases
- `src/runtime_layer/service_endpoint_contract.rs` remains the compat-side service endpoint ingress
- `src/gui/client.rs`, `src/gfx/velvet_glove.rs`, `src/ipc/service_ipc.rs`, `src/posix.rs`, `src/posix/native_scene_bridge.rs`, `src/win32_abi.rs`, and `src/pe_loader.rs` no longer contribute any managed `crate::...` legacy ingress
- all managed namespace roots, compatibility shells, product hotspots, compat bridges, and IPC shells now report `0` legacy references
- debt, exceptions, and broad facade ingress all remain at `0`

## Strongest Crate Candidates

- `runtime_layer`
- `compat`

## Why The Split Is Now Decision-Ready

- the public runtime compatibility facades no longer expose raw implementation modules on the managed path
- behavior contracts sit between compatibility shells and implementation-owned modules
- compat-side service endpoint ingress is contract-backed instead of raw-IPC rooted
- the managed-file guard now reports `legacy_references = 0`, so there is no remaining measured namespace-root or consumer-root sprawl in the tracked architecture surface
- any remaining split blocker must now be treated as an explicit crate-boundary design choice, not hidden cleanup backlog

## Remaining Guard Hotspots

- none; `legacy_references_by_file` is empty

## Split Blockers

- no remaining blocker is reported by `arch_guard`
- the next step is crate-boundary execution order, not more guard-driven legacy cleanup

## Ready To Split First?

- Yes, if desired.
- `runtime_layer` remains the cleanest first split candidate.
- `compat` is now also credible because its managed backlog is `0`.

## What Must Fall Before The First Real Split

- nothing remains in the guard-managed legacy backlog
- the next plan should choose crate boundaries and migration order directly

## Validation

- `cargo check --target x86_64-pc-windows-msvc --lib -q`
- `cargo arch-guard --report`
- `cargo arch-guard --check --report`
- `cargo arch-guard --refresh-baseline --report`
- `.\.qoder\skills\echos-architect\scripts\echos_gate.ps1 -Paths src\win32_abi.rs,src\pe_loader.rs,src\posix.rs,docs\architecture\arch_baseline.json,docs\architecture\wave7_split_readiness_review.md,docs\architecture\wave7_closeout_review.md,docs\agent\decision-log.md -Json`
