# Canonical Launch + Present Pipeline

echOS product shell canonicalizes user-visible app startup and presentation around the
following ownership chain:

```text
[Click / Shortcut / Command]
        ↓
[Unified Launcher]
        ↓
[App Resolution]
        ↓
[LaunchIntent]
        ↓
[Policy / Capability Gate]
        ↓
[spawn_process]
        ↓
[Loader Dispatch]
   ├─ Native
   ├─ PE
   └─ ELF
        ↓
[Personality Bind]
   ├─ Native ABI
   ├─ Win32 Bridge
   └─ POSIX Bridge
        ↓
[Runtime Bootstrap]
        ↓
[Window Object + IPC Endpoints]
        ↓
[Surface Registration]
        ↓
[Damage / Scene Update]
        ↓
[App Event Loop]
        ↓
[FrameIntent]
        ↓
[Compositor / FrameScheduler]
        ↓
[PresentQueue]
        ↓
[AtomicKmsTransaction]
        ↓
[MMIO Commit / Flip]
        ↓
[Display]
        ↓
[VBLANK ISR / Fence Retire]
```

## Current repository mapping

- Launch contract types live in [`src/gui/launch_pipeline.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/launch_pipeline.rs).
- Whole-system runtime ownership now lives in [`src/runtime.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/runtime.rs): `RuntimeHandle`, `RuntimeCoordinator`, `WindowSessionHandle`, `RuntimePackageRegistry`, and the canonical service/PE/ELF spawn helpers.
- Canonical launch descriptors remain in [`src/gui/launch_pipeline.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/launch_pipeline.rs): `ProcessContract`, `RuntimeBootstrap`, `WindowEndpointContract`, `UnifiedEventLoopContract`, and `LaunchSession`.
- Desktop shell entrypoints currently route through [`src/gfx/velvet_glove.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gfx/velvet_glove.rs).
- `App Resolution` now covers registry-backed built-in aliases plus path-based external images: built-ins such as `terminal`, `files`, `settings`, `editor`, `web`, and `recycle bin` resolve from the package/app registry, while `.exe` resolves to `PE + Win32 Bridge + ShellOwned` and `.elf/.bin` resolves to `ELF + POSIX Bridge + ShellOwned`.
- Shell/service state and permissions live in [`src/services/ech_shell.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/ech_shell.rs) and [`src/gui/client.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/gui/client.rs), with service boot now registering through the canonical runtime in [`src/services/mod.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/mod.rs).
- PE staging and Win32 bootstrap live in [`src/pe_loader.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/pe_loader.rs), [`src/win32.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32.rs), and [`src/win32_abi.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/win32_abi.rs).
- POSIX staging lives in [`src/posix.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/posix.rs).
- Display and present path live in [`src/services/ech_display.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/ech_display.rs), [`src/services/display_atomic.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/services/display_atomic.rs), [`src/drivers/drm.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/drm.rs), and [`src/drivers/gpu_native.rs`](C:/Users/Bahadir/Desktop/dersler_ve_projeler/echOS/src/drivers/gpu_native.rs).

## Current boundary

- Product desktop entrypoints, service boot, shell-owned PE/ELF launches, and window creation now all publish into one named runtime spine.
- `spawn_process` and runtime bootstrap are no longer desktop-only concepts: `spawn_service_runtime`, `spawn_elf_runtime`, `spawn_pe_runtime`, scheduler user-image spawning, and Win32 bound-process task spawn all flow through named canonical seams.
- `Window Object + IPC Endpoints` now has an explicit repository-level handle: `WindowSessionHandle`, registered from `DesktopClient::create_window*` and released on destroy.
- Desktop tick still polls one explicit `UnifiedEventLoopBatch` before routing shell/app events, so event ingestion is no longer only an implicit set of adjacent loops.
- Remaining gap is catalogue breadth and richer package identity, not missing runtime-contract shape.
- Present path already matches the lower half of the pipeline through `FrameIntent -> AtomicPresenter -> AtomicKmsTransaction -> MMIO/VBLANK`.
