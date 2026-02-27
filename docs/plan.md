# echOS General Plan

## North Star
Make echOS a Tier-1 desktop-grade OS with a secure driver isolation pipeline, reliable app compatibility, and deterministic UX performance.

## Current Focus
- IronShim enforced in live syscall path and driver registration.
- Seccomp strict/filter policy unified in one enforcement layer.
- MMIO/port access gated by isolated driver manifests where possible.

## Phase 1: Security and Isolation (Immediate)
- Make seccomp policy the single source of truth for syscall allow/deny.
- Enforce filter mode with BPF programs attached per task.
- Gate MMIO/port access to declared driver manifests.
- Normalize PCI parsing through a single bridge path.

## Phase 2: Compatibility and Runtime
- Ensure Win32/PE launch flows surface consistent telemetry.
- Harden POSIX syscall behavior and return codes.
- Add crash/deny reporting for blocked syscalls and driver access.

## Phase 3: UX and Performance
- Maintain compositor latency pressure controls and fallback strategy.
- Keep deterministic window z-order and bounds normalization.
- Validate GPU and input paths under stress.

## Phase 4: Quality and Release
- Consolidate CI smoke and target matrices into gating checks.
- Add minimal release ops checklist (artifacts, checksums, boot validation).
- Track regressions with a small, stable bench set.

## Verification
- `cargo check`
- Boot test via `run_qemu.ps1`
- Inspect IronShim health dump and syscall deny logs
