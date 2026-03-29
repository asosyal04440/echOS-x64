# Wave 4 Closeout Review

## Decision
- `wave 4 = closed`

## Exit Criteria
- baseline ratchet is live in `arch_guard`
- `arch_baseline.json` is checked in and current
- broad facade growth is mechanically blocked
- narrow contract spine is importable under `runtime_layer`
- tracked product consumer no longer uses `crate::runtime_layer::service_api`

## Final Guard State
- `current_wave = 4`
- `violations = 0`
- `expired_debt = 0`
- `legacy_references = 641`
- `broad facade references = 0`

## What Closed
- `gui/client.rs` no longer uses the deprecated broad `service_api` ingress
- `product -> runtime_layer` allowlist is now contract-only
- the temporary `gui/client.rs` exception was removed
- baseline ratchet now protects a zero-broad-facade state for tracked files

## Remaining Hotspots
1. `src/gfx/velvet_glove.rs` -> `148`
2. `src/gui/client.rs` -> `68`
3. `src/posix.rs` -> `61`
4. `src/posix/runtime_bridge.rs` -> `60`
5. `src/ipc/service_ipc.rs` -> `58`

## Next Wave Pressure
- `Wave 5` should attack product-side consumer narrowing first
- `velvet_glove.rs` remains the heaviest product hotspot
- `gui/client.rs` still carries explicit legacy IPC ingress even though the broad facade is gone

## Split Status
- `crate split = deferred`
- reason: broad facade creep is now suppressed, but the main consumer hotspots remain too wide for a packaging split to produce a real architectural win
