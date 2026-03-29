# Wave 3 Closeout Review

Date: 2026-03-29

This review closes the Wave 3 decomposition pass and records the remaining debt honestly before any future crate-split or cleanup wave.

## Required Metrics
- Highest violation namespace from `cargo arch-guard --report`: none. `violations = 0`.
- Highest facade-debt namespace: `runtime_layer` with `7` active debt tags.
- Highest legacy-path consumer count: `runtime_layer` with `269` legacy references by namespace.
- Highest `macro_escape` count: none observed.
- Active debt summary: `active_debt = 14`, `new_debt = 0`, `renewed_debt = 14`, `expired_debt = 0`, `active_exceptions = 12`.

## Wave 3 Structural Outcome
- `src/ipc/service_ipc.rs` is no longer a mixed transport plus public-surface monolith. Endpoint registration and public API wrappers are now physically split into:
  - `src/ipc/service_ipc/endpoints.rs`
  - `src/ipc/service_ipc/api.rs`
- `src/ipc/service_ipc.rs` line count is now `698`, down to an orchestration shell that primarily owns shared types, manager state, dispatch helpers, and tests.
- `src/posix.rs` no longer owns the Windows runtime registry and PE/Secure Boot pipeline inline. Those ownership seams now live in:
  - `src/posix/runtime_bridge.rs`
  - `src/posix/windows_runtime.rs`
  - `src/posix/windows_image.rs`
- `src/gfx/velvet_glove.rs` still remains large at `9642` lines, but the runtime/package/launch seam is now physically split into `src/gfx/velvet_glove/launch.rs`.

## Required Decisions
- Namespace leaking most: `runtime_layer`.
  Reason: even after the Wave 3 IPC and compat/product splits, most remaining legacy traffic still converges through runtime-owned launch, package, broker, and service-control seams.
- Hottest remaining coupling files:
  - `src/gfx/velvet_glove.rs` with `148` legacy references
  - `src/gui/client.rs` with `67` legacy references
  - `src/posix.rs` with `61` legacy references
  - `src/posix/runtime_bridge.rs` with `60` legacy references
  - `src/ipc/service_ipc.rs` with `58` legacy references
- Crate split decision: `deferred`.
  Reason: the wave succeeded in converting the largest mixed-ownership roots into measured shells plus submodules, but product and compat still carry broad runtime consumption and are not yet narrow enough for a clean crate boundary.

## Exit Statement
- Wave 3 may close.
- The close condition is satisfied because the remaining large hotspots are now explicitly measured, guard-managed, and no longer block the core objective of this wave: physical decomposition of the biggest mixed-ownership roots.
- The next wave should focus on consumer-side narrowing rather than more namespace creation:
  - continue shrinking `src/gfx/velvet_glove.rs`
  - continue shrinking `src/gui/client.rs`
  - reduce broad runtime consumption inside `src/posix.rs` and `src/posix/runtime_bridge.rs`
  - only revisit crate split after those consumer seams are materially narrower
