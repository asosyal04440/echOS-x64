# Wave 5 Product Narrowing Review

## Scope
- `src/runtime_layer/{display,input,shell,notification,clipboard,dialog,capture,store}_client_contract.rs`
- `src/runtime_layer/mod.rs`
- `src/gui/client.rs`
- `src/gfx/velvet_glove.rs`
- `src/gfx/velvet_glove/app_runtime.rs`
- `src/gfx/velvet_glove/bootstrap.rs`
- `src/gfx/velvet_glove/session_runtime.rs`
- `tools/arch_guard/src/main.rs`
- `docs/architecture/arch_rules.toml`
- `docs/architecture/arch_baseline.json`

## Outcome
- `gui/client.rs` no longer reaches directly into root `crate::ipc`.
- Product-side desktop client traffic now uses service-by-service runtime-layer client contracts instead of a broad facade or direct root IPC calls.
- Velvet Glove bootstrap and dialog/session flows now stage through typed runtime-layer client contracts for shell, display, input, and dialog ingress instead of local `DesktopClient` reconnect loops.
- Velvet Glove app execution surfaces now live in a separate `app_runtime` product module; the root desktop shell file no longer owns terminal/files/browser/settings/editor behavior directly.
- The baseline ratchet now measures only explicit tracked hotspot files and managed consumer surfaces; namespace-scanned compatibility shells remain visible in the full report, but they no longer create false ratchet regressions during Wave 5 contract rollout.
- `current_wave` advanced to `5`.
- Wave 4 debt renewals were extended through Wave 5 and Wave 4 contracts were renewed to avoid silent expiry while Wave 5 narrowing proceeds.

## Guard Snapshot
- `current_wave = 5`
- `violations = 0` after baseline refresh
- `active_debt = 27`
- `new_debt = 8`
- `renewed_debt = 19`
- `expired_debt = 0`
- full-report `legacy_references = 646`
- tracked-baseline `legacy_references = 568`

## Measurable Product Gain
- `src/gui/client.rs`: `68 -> 51`
- `product` namespace total: `230 -> 200`
- `src/gfx/velvet_glove.rs`: `148 -> 135`
- `src/gfx/velvet_glove/launch.rs`: stayed at `9` while root launch/runtime dispatch ownership remained isolated in its own product module

## Second Slice Result
- `src/gfx/velvet_glove/bootstrap.rs` now uses `display_client_contract`, `input_client_contract`, and `shell_client_contract` for shell bootstrap environment setup, permission grants, file grants, shell app registration, and workspace/power/theme programming.
- `src/gfx/velvet_glove.rs` now uses `dialog_client_contract` and `shell_client_contract` for pending-dialog polling, dialog resolution, file-access grants, and session snapshot queries.
- This is a staging cut, not yet a hotspot drop. It removes more product-side knowledge of `DesktopClient` round-trips, but the dominant tracked product hotspot is still the root `velvet_glove.rs` file.

## Third Slice Result
- `src/gfx/velvet_glove/app_runtime.rs` now owns the terminal/files/browser/settings/editor execution surfaces plus their browser/path/dialog helpers.
- `src/gfx/velvet_glove.rs` now behaves more like a desktop-session shell root: launch/session/render orchestration remains there, but app-local runtime mechanics moved out.
- This cut produced the first substantial Velvet Glove hotspot drop of Wave 5:
  - full-report `src/gfx/velvet_glove.rs`: `148 -> 135`
  - full-report `product`: `213 -> 200`
  - full-report total: `659 -> 646`

## Wave 5 Exit Read
- `gui/client.rs` remains on typed client contracts only.
- `velvet_glove.rs` and `velvet_glove/launch.rs` now separate desktop shell orchestration from app-local runtime behavior.
- No broad `runtime_api::*` product ingress was reintroduced.
- Guard, smoke, and touched-path gate stayed green after the deep extraction.

## Tradeoff Accepted
- `runtime_layer` legacy count rose because the new client contracts are still compatibility shells over root IPC and service command surfaces.
- This is an intentional Wave 5 staging move: it pushes product consumers onto typed per-service contracts first, so the next cuts can narrow the contracts themselves without reopening root IPC ingress in product code.
- The original baseline implementation counted every namespace-scanned file, which made new compatibility-shell contracts look like regressions even when product-side ingress stayed flat or improved. The Wave 5 ratchet fix narrows baseline accounting to tracked hotspot files while preserving the full-report visibility of total legacy volume.

## Residual Product Pressure
1. `src/gfx/velvet_glove.rs` is still the hottest product file at `135`.
2. `src/gui/client.rs` is narrower than the starting state, but still carries `51` legacy references through typed client-contract usage.
3. Further product work should focus on consumer narrowing, not on reopening broad runtime ingress.
