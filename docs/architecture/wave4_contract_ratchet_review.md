# Wave 4 Contract Ratchet Review

## Scope
- `tools/arch_guard`
- `docs/architecture/arch_rules.toml`
- `docs/architecture/arch_baseline.json`
- `src/runtime_layer/{launch_contract,process_broker_contract,package_registry_contract,window_session_contract,native_scene_contract,capability_contract,service_parity_contract}.rs`
- product and compat consumers touching those contracts

## Outcome
- `arch_guard` now supports a baseline ratchet through `--refresh-baseline` and compares:
  - total `legacy_references`
  - `legacy_references` by namespace
  - `legacy_references` by file
  - broad facade references by prefix
  - broad facade references by file
- `arch_baseline.json` is now the mechanical ceiling for legacy growth.
- `runtime_api`, `bootstrap_api`, and `service_api` are explicitly deprecated compatibility shells.
- Wave 4 introduced seven narrow behavior contracts:
  - `launch_contract`
  - `process_broker_contract`
  - `package_registry_contract`
  - `window_session_contract`
  - `native_scene_contract`
  - `capability_contract`
  - `service_parity_contract`

## Guard Snapshot
- `current_wave = 4`
- `violations = 0`
- `active_debt = 19`
- `new_debt = 7`
- `renewed_debt = 12`
- `expired_debt = 0`
- `legacy_references = 641`
- broad facade references recorded by the baseline: `0`

## Hotspots Against The Baseline
- `src/gfx/velvet_glove.rs`: `148`
- `src/gui/client.rs`: `68`
- `src/posix.rs`: `61`
- `src/posix/runtime_bridge.rs`: `60`
- `src/ipc/service_ipc.rs`: `58`

## What Tightened
- `product -> runtime_layer` no longer broadly allows `service_api`.
- `src/gui/client.rs` no longer consumes the broad `service_api` facade; it now uses explicit IPC entry points plus `window_session_contract`.
- Compat native window/session flow was narrowed to `native_scene_contract` instead of using `window_session_contract` directly.
- Internal runtime/service call sites that still needed grouped access were reduced so the baseline now records zero broad facade consumer files.

## Remaining Blockers Before Split Readiness
- `src/gfx/velvet_glove.rs` remains the heaviest product hotspot.
- `src/gui/client.rs` is no longer a broad-facade ingress, but it is still a product hotspot through explicit legacy IPC entry points.
- `src/posix.rs` and `src/posix/runtime_bridge.rs` still carry the widest compat legacy surface.
- `runtime_layer` remains the highest-debt namespace even though broad runtime/bootstrap facade usage is nearly extinguished.

## Crate Candidate Signal
- `runtime_layer` remains the strongest future crate candidate because its ownership seams are now explicit and its broad facade usage is nearly eliminated.
- It is not split-ready yet because the hottest consumers still sit in `product` and `compat`, not because `runtime_layer` lacks a namespace boundary.
