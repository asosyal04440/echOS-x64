# Worktree Triage Inventory - 2026-04-17

## Safety Capture

- Safety branch: `codex/worktree-triage-20260417`
- Source branch at capture: `main`
- Policy: preserve-first. No reset, no `git clean`, no recursive delete, no submodule discard, no mass line-ending normalization.
- Staging state at capture: no staged entries were created before this report.

## Snapshot

| Signal | Count / state |
| --- | ---: |
| `git status --porcelain=v1` modified entries | 120 |
| `git status --porcelain=v1` untracked status entries | 24 |
| Total porcelain status entries | 144 |
| Tracked diffstat | 120 files, 24631 insertions, 3156 deletions |
| Expanded untracked files | 1417 |
| Dirty submodules | 2 |
| `.gitattributes` before triage | absent |

## Untracked Classification

| Class | Count | Decision |
| --- | ---: | --- |
| `artifacts/curated-downloads/*.zip`, `*.tar.gz` | 9 | Ignore as downloaded upstream archives. |
| `artifacts/curated-downloads/*-extract/**` | 1297 | Ignore as extracted upstream release trees. |
| `artifacts/curated-sources/*/*.exe` | 9 | Ignore as copied binary payloads for packaging. |
| `artifacts/curated-sources/*/echos-app.toml` | 9 | Keep visible for review as package metadata source. |
| `artifacts/curated-bundles/*.bhd` | 9 | Ignore as built package outputs. |
| `docs/book-core-v1/out/**` | 2 | Ignore generated book output except `.gitkeep`. |
| `docs/book-core-v1/figures/generated/**` | 1 | Ignore generated figure output except `.gitkeep`. |
| `docs/book-core-v1/src/**`, `figures/*.mmd`, scripts, package manifests | 53 | Keep visible as documentation source. |
| `assets/tts/voices/**` | 5 | Keep visible until license/provenance review is recorded. |
| `tmp_espeak_voices.json` | 1 | Ignore as local voice-scan diagnostic output. |
| `src/**`, `scripts/**`, `docs/agent/**`, `docs/architecture/**`, `EGO/**`, `RELEASE_PLAN.md` | 19 | Keep visible as source/docs/script review candidates. |

## Source Review Candidates

- `docs/agent/curated-app-commercial-license-audit-2026-04-07.md`
- `docs/agent/curated-app-compatibility-matrix-2026-04-08.md`
- `docs/agent/system-update-plane.md`
- `docs/architecture/secure_boot_local_flow.md`
- `EGO/EGO_TODO.md`
- `RELEASE_PLAN.md`
- `scripts/build_f2fs_slot_image.py`
- `scripts/build_signed_uefi.ps1`
- `scripts/generate_secure_boot_bundle.ps1`
- `scripts/package_curated_apps.ps1`
- `scripts/run_secure_boot_qemu_smoke.ps1`
- `scripts/sign_uefi_secure_boot.ps1`
- `src/allocator/doctrine.rs`
- `src/audio/tts.rs`
- `src/drivers/loopback.rs`
- `src/gfx/image_assets.rs`
- `src/security/seed_store.rs`
- `src/security/spectre.rs`
- `src/update.rs`
- `tmp_espeak_voices.json` remains classified as diagnostic output, not a source candidate.

## Submodule Containment

| Submodule | Status |
| --- | --- |
| `third_party/ironshim-rs` | 7 tracked Rust files changed; diffstat is 128 insertions and 133 deletions. No untracked files reported. |
| `third_party/valkyrie-v` | 4 tracked Rust files changed; diffstat is 53 insertions and 90 deletions. Untracked diagnostic logs: `build_errors.txt`, `build_output.txt`, `errors_clean.txt`. |

Submodule parent pointers must not be staged until each submodule's local work is committed, intentionally carried dirty with notes, or explicitly postponed.

## Review Groups

| Group | Representative tracked paths | Validation |
| --- | --- | --- |
| Boot/QEMU/Simics/appliance tooling | `run_qemu.ps1`, `run_simics.ps1`, `scripts/build_vm_appliance.py`, `.cargo/config.toml`, `Cargo.toml` | PowerShell parser checks, UEFI cargo check, targeted smoke when host firmware tooling is available. |
| Runtime/GUI/Velvet Glove/service launch | `src/gfx/**`, `src/gui/**`, `src/services/**`, `src/runtime_layer/**` | `cargo check --target x86_64-pc-windows-msvc --lib -q`, UEFI check, GUI/service smoke. |
| VFS/filesystem capability changes | `src/fs/**`, `docs/agent/fs-capability-matrix.md` | Filesystem unit tests if present, host cargo check, path normalization and read/list regression tests. |
| Network/security/update path | `src/net/**`, `src/security/**`, `src/update.rs`, `src/random.rs` | Host cargo check, targeted crypto/network tests, update-plane smoke. |
| Curated app packaging and docs | `scripts/package_curated_apps.ps1`, `docs/agent/curated-*`, `artifacts/curated-sources/*/echos-app.toml` | Manifest schema inspection, license/provenance review, packaging dry run. |
| Submodule changes | `third_party/ironshim-rs`, `third_party/valkyrie-v` | Run each submodule's own check/test command after its manifest is inspected. |

## Line Ending Risk Gate

- `.gitattributes` is isolated as a policy file. Do not run `git add --renormalize`.
- Any future line-ending-only diff must be committed separately or rejected from feature/code commits.
- If adding `.gitattributes` produces broad content churn during validation, remove it from the active cleanup commit and carry it in a policy-only branch.
