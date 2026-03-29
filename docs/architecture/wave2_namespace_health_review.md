# Wave 2 Namespace Health Review

Date: 2026-03-29

This review is the mandatory Wave 2 checkpoint before any crate-split decision.

## Required Metrics
- Highest violation namespace from `cargo arch-guard --report`: none. `violations = 0`.
- Highest facade-debt namespace: `runtime_layer` with `7` active debt tags.
- Highest legacy-path consumer count: `runtime_layer` with `251` legacy references by namespace.
- Highest `macro_escape` count: none observed. The current report has no `macro_escape` violations.

## Required Decisions
- Namespace leaking most: `runtime_layer`.
  Reason: Wave 2 successfully moved runtime model/state/registry/spawn plus service control-plane ownership under `src/runtime_layer`, but the new ownership files still bridge older roots such as `crate::gfx`, `crate::ipc`, and `crate::services`. That is honest transition debt, not hidden folder churn, and the guard now measures it directly.
- Hottest facade: `runtime_layer::runtime_api`.
  Reason: product, compat, and service-control code all converge on `runtime_api` for runtime-handle lookup, package resolution, broker lookup, and launch/session introspection. It remains temporary and is explicitly renewed only through Wave 2; it should narrow again in Wave 3 once package/broker/session sub-surfaces become independently consumable.
- Modules still in the wrong bucket:
  - `src/ipc/service_ipc.rs`: still a mixed transport/control-plane file even after the runtime service-control extraction. Transport/mailbox/shared-region mechanics belong with IPC ownership, but directory/package/broker orchestration has now started moving out and should continue.
  - `src/gfx/velvet_glove.rs`: product-facing shell/UI code still carries a heavy amount of runtime/package launch coupling.
  - `src/posix.rs`: compat-facing syscall and runtime/service glue still consumes too much broad surface from lower layers.
  - `src/services/mod.rs`: service bootstrap policy remains broad and should keep shrinking toward explicit runtime-owned bootstrap surfaces.
- `subsystems` long-term narrowing:
  - `drivers`: classify as `kernel-adjacent platform I/O`
  - `gpu3d`: classify as `kernel-adjacent platform I/O`
  - `audio`: classify as `reusable subsystem`
  - `ml`: classify as `product-runtime facing`

## Required Domain Classification
- `drivers`: kernel-adjacent platform I/O
- `gpu3d`: kernel-adjacent platform I/O
- `audio`: reusable subsystem
- `ml`: product-runtime facing

## Debt and Boundary Reading
- `active_debt = 14`
- `new_debt = 3`
- `renewed_debt = 11`
- `expired_debt = 0`
- `active_exceptions = 10`

Interpretation:
- Wave 2 debt is bounded but not small.
- The debt profile is still acceptable for a single-crate migration because renewals are explicit, wave-scoped, and guard-enforced.
- The main unresolved hotspot is not hidden namespace drift; it is the still-large `service_ipc` transport file and the remaining runtime coupling in `velvet_glove` and `posix`.

## Exit Statement
- Crate splitting is `deferred`.
- Wave 2 may close with crate splitting still deferred because the runtime/service facade boundary is now real, the mandatory review is complete, and the guard is enforcing renewed debt explicitly.
- No namespace is ready for a clean standalone crate split yet. `runtime_layer` is the closest candidate, but it still carries too much legacy coupling and should first complete the `service_ipc` transport/control-plane decomposition in Wave 3.
