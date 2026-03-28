# Application Model + Core System Services Plan

echOS productization needs a single application contract layered over the existing
runtime spine, service bus, and display/data-plane split. The target is:

```text
[Package / Built-in / External Image]
        ↓
[AppIdentity + AppManifest]
        ↓
[PackageRegistry]
        ↓
[LaunchIntent]
        ↓
[Capability Gate]
        ↓
[ProcessBroker]
        ↓
[RuntimeBootstrap]
        ↓
[WindowSession / HeadlessSession]
        ↓
[Core Services over Control Plane]
        ↓
[Out-of-band Data Plane for bulk payloads]
```

## Ownership boundaries

- `AppIdentity`
  - stable app id, display name, icon id, trust state, install root
- `AppManifest`
  - launch kind (`native`, `pe`, `elf`, `special`)
  - entrypoint
  - requested capabilities
  - window/background policy
  - file associations
  - state contract flags
- `PackageRegistry`
  - package id to manifest/install roots
  - alias resolution for built-ins and installed apps
  - update and provenance metadata
- `ProcessBroker`
  - launch authority for user apps and helper processes
  - MMU/ring transition ownership
  - child-process tree and kill/restart ownership
- `RuntimeSupervisor`
  - crash, restart, suspend, resume, and state rebind ownership
- `Core Services`
  - control-plane only on the service bus
- `Data Plane`
  - shared surfaces, rings, and bulk buffers stay out-of-band

## Canonical app model

### 1. Identity and packaging

- Every app must have a stable `AppIdentity`; paths are fallback probes, not the main identity.
- Runtime-visible install roots should standardize to:
  - `/system/apps/<app-id>`
  - `/apps/<app-id>`
  - `/data/appdata/<app-id>`
  - `/downloads`
- Built-ins must also publish through the same registry, not a parallel hand-written launcher table.

### 2. Manifest contract

- Source manifest format can stay developer-friendly (`TOML` or `JSON`).
- Install/runtime format should be a compiled binary record:
  - fixed header
  - capability bitmap or compact table
  - launch personality
  - association table
  - window/background policy flags
- Rationale:
  - avoids runtime text parsing in hot product paths
  - keeps `no_std` parsing deterministic
  - bounds heap pressure

### 3. Launch and runtime contract

- `LaunchIntent -> ProcessBroker -> RuntimeBootstrap` must be the only privileged app start path.
- `ProcessBroker` owns:
  - address-space creation
  - user/kernel transition setup
  - launch-time capability tokens
  - helper/child process publication
- `RuntimeBootstrap` owns:
  - personality bind (`Native`, `Win32`, `POSIX`, `Special`, `HeadlessService`)
  - session bootstrap
  - window/session attachment

## State serialization contract

Suspend/resume should split into two tiers:

- `WarmSuspend`
  - task stays resident
  - scheduler/input/window presence pauses
- `ColdResume`
  - app exports durable state bundle
  - process may terminate
  - later launch imports previous state

Required app-visible contract:

- `prepare_suspend(deadline_ms)`
- `export_state() -> StateHandle`
- `import_state(StateHandle)`
- `resume(reason)`

Required broker/runtime-visible contract:

- state store namespace per app id
- resume token or bundle descriptor
- restart path that rehydrates window/session state

## MMU and privilege isolation contract

`ProcessBroker` is not complete unless it is bound to the VM layer.

Per-app launch must own:

- distinct address space root
- explicit user/shared/broker-mapped regions
- capability-backed IPC pages or handles
- ring transition contract for user entry
- teardown and revocation on crash/kill

Design rule:

- `Native`, `PE`, and `ELF` personalities differ in ABI surface, not in isolation quality.
- The same MMU cage must apply to all untrusted app classes.

## Core System Services

Tier-0 product services:

- `DisplayService`
- `InputService`
- `ShellService`
- `StoreService`
- `NotificationService`
- `ClipboardService`
- `DialogService`
- `CaptureService`
- `AudioService`
- `NetworkBroker`
- `ProcessBroker`
- `PackageRegistry`

Control-plane responsibilities:

- discovery
- request/response
- lifecycle
- permission checks
- typed events

Out-of-band responsibilities:

- framebuffer/surface payloads
- audio sample rings
- large file transfer buffers
- future high-rate network payload movement

## Product milestones

### AM-1 Identity + Package Registry

- stable `AppIdentity`
- built-ins and installed packages in one registry
- alias and file-association resolution through one contract

### AM-2 Process Broker + Capability Tokens

- canonical process spawn authority
- capability token publication at launch
- child process tree ownership

### AM-3 State Contract + Resume Store

- warm/cold suspend split
- exported app state bundles
- restart/import rehydration

### AM-4 MMU Isolation Contract

- per-app address-space ownership
- broker-mapped IPC region model
- kill/revoke semantics on fault

### AM-5 Manifest Compilation Pipeline

- source manifest -> compiled binary manifest
- deterministic runtime parser
- bounded heap/alloc behavior

### CS-1 Network + Package Brokers

- privileged network ops behind broker boundary
- package install/update/remove broker flow

### CS-2 Recovery + Service Health

- crash recovery/rebind semantics
- service health and restart policy
- user-visible denial/error reasons

## Validation boundary

The plan is only product-credible when:

- installed apps, built-ins, and external images all resolve through one identity model
- privileged launch always flows through the same brokered MMU/capability path
- suspend/resume is testable for at least one stateful app class
- service bus control-plane and out-of-band data plane stay explicitly separated
- crash/restart and deny paths are typed and user-visible rather than silent
