# Win32 Atomic + Alignment Hardening Plan

## Scope

This document is the durable cached attack map for the next exactness-hardening slice:
`win32 exactness hot-path atomic/alignment burn-down`.

Primary touch set:
- `src/win32.rs`
- `src/drivers/mouse.rs`
- `src/ecosystem_exactness.rs`
- `src/bin/echsdk.rs` only if host validation wiring must tighten further

Non-goals:
- no new Win32 API family
- no new POSIX long-tail surface
- no compatibility-scope expansion
- no cold-path refactor beyond ordering/alignment truth

## Taarruz Plani

1. Memory layout and ownership boundaries
- Split the current atomics into three ownership classes and stop treating them as one flat bag of globals.
- `NEXT_*` families are monotonic allocators, not publication state. They stay cold-path and remain outside GUI/input publication domains.
- `ACTIVE_HWND`, `FOCUSED_HWND`, and `CAPTURED_HWND` become one coherent publication domain with one ownership rule: activation, focus, and capture transitions publish together and readers consume a stable snapshot.
- `LAST_REPORTED_MOUSE_*` and mirrored `crate::drivers::mouse::MOUSE_*` state become one coherent pointer-publication domain. Coordinates and buttons must not be observed from mixed generations.
- `SWAP_MOUSE_BUTTONS` is treated as pointer-policy state and moves into the same publication domain as the button bitmap.

2. Lock-free structure choice with justification
- Replace the hot free-floating atomics with two single-writer publication cells.
- `WindowPublicationState`: `sequence`, `active`, `focused`, `captured`.
- `PointerPublicationState`: `sequence`, `x`, `y`, `buttons`, `swap_policy`.
- Writers use a seqlock-style odd/even generation protocol with release publication.
- Readers spin until one stable even generation is observed with acquire loads.
- Reason: the current layout is instruction-cheap but semantically weak. It allows readers to observe `x` from generation `n` and buttons from generation `n + 1`, which is acceptable for telemetry but not for exact input routing.
- Keep monotonic handle allocators on `Relaxed` unless a concrete ownership transfer requires more. They allocate names, not visibility barriers.

3. Hardware / ABI interaction map
- Win32 ABI touchpoints to migrate onto coherent snapshot helpers:
  - `mouse_event`
  - `GetCursorPos`
  - `WindowFromPoint`
  - `GetActiveWindow` / `SetActiveWindow`
  - `GetFocus` / `SetFocus`
  - `GetCapture` / `SetCapture` / `ReleaseCapture`
- Kernel/input touchpoints:
  - `src/drivers/mouse.rs` physical pointer state
  - Win32 synthetic pointer injection path
  - input-routing code that derives target windows from active/captured state
- Publication contract:
  - mutate local state first
  - publish the completed snapshot with release ordering
  - readers that derive routing, cursor position, or button transitions must first acquire a stable snapshot
- Cold-path allocators such as `NEXT_HWND`, `NEXT_HDC`, `NEXT_HMENU`, `NEXT_HBRUSH`, `NEXT_HPEN`, `NEXT_HFONT`, `NEXT_HRGN`, `NEXT_HPALETTE`, `NEXT_HACCEL`, `NEXT_HMETAFILE`, `NEXT_PRINT_JOB`, `NEXT_FILE_HANDLE`, `NEXT_INTERNET_HANDLE`, `NEXT_COM_COOKIE`, and `NEXT_COM_STREAM_ID` stay explicitly outside the coherent-publication domain.

4. Cache-line and contention strategy
- Hot publication blocks become `#[repr(C, align(64))]` structures so cross-core reads do not share cache lines with cold allocators or unrelated flags.
- Mouse coordinates and buttons are packed together because they are written and consumed together.
- Active/focus/capture are packed together because input routing and activation semantics consume them together.
- Cold-path handle allocators are grouped away from hot publication state so `fetch_add` traffic cannot bounce the same cache line as pointer or activation state.
- Reader helpers become the only sanctioned load path for exactness-sensitive state. Raw direct loads of hot publication members are removed from those paths.
- Success criterion is not “few atomics.” It is “each remaining atomic has one named ownership role, one justified ordering level, and no false-sharing hotspot with unrelated hot state.”

5. Validation strategy
- Add targeted corpus for:
  - coherent pointer snapshot reads under interleaved updates
  - activation/focus/capture snapshot coherence
  - `mouse_event` routing against a coherent active/captured snapshot
  - reset helpers restoring aligned publication blocks to generation `0`
- Preserve and rerun the existing hard gates:
  - `cargo test -p ech_os --target x86_64-pc-windows-msvc --lib`
  - `cargo test -p ech_os --bin echsdk --target x86_64-pc-windows-msvc --no-run`
  - `.\target\x86_64-pc-windows-msvc\debug\echsdk.exe exactness strict`
  - `.\.qoder\skills\echos-architect\scripts\echos_gate.ps1 -Paths <touched paths>`
- This slice does not close until the touched-path gate stays clean and the remaining atomic/alignment warnings are either reduced or backed by named ownership invariants.

## Worst Credible Failure First

- Worst correctness failure: torn pointer snapshot produces an impossible coordinate/button combination and misroutes a click or drag.
- Worst routing failure: activation and capture are observed from different generations and input lands on the wrong window.
- Worst performance failure: cold-path allocator traffic shares cache lines with hot pointer or activation publication and creates avoidable coherency churn.
- Containment rule: move exactness-sensitive readers to snapshot helpers first, then tighten writers, then revisit leftover atomics individually.

## Algorithm Autopsy

- Selected: aligned seqlock-style publication cells for hot GUI state
  - Why over independent atomics: coherent multi-field reads matter more than single-instruction loads on this path
  - Main weakness: readers may retry under write pressure
  - Mitigation for echOS: the GUI state path has one effective writer domain and low cardinality, so retry cost is bounded and preferable to torn snapshots
- Selected: `Relaxed` monotonic allocators for handle families
  - Why over stronger orderings: uniqueness is required, publication ordering is not
  - Main weakness: future code could misuse the counter as an implicit barrier
  - Mitigation for echOS: classify them explicitly as allocation-only and keep them outside aligned hot publication blocks

## Optimization Budget

- Let `R` be reader frequency and `W` be writer frequency for pointer publication.
- Current cost: `O(k)` unrelated atomic loads for one logical snapshot, where `k` is coordinates plus buttons plus policy.
- Planned cost: `O(1 + retries)` stable snapshot reads with one sequence guard and one packed state load group.
- If `W << R`, seqlock-style publication lowers semantic risk with negligible retry cost.
- If `W ~= R` during synthetic-input storms, aligned packing still improves locality and removes false sharing across several independent cache lines.

## Execution Waves

1. Inventory and invariants
- name each hot atomic by role
- freeze which ones are allocators and which ones are publication state
- add coherent read helpers before migrating most call sites

2. Window publication cutover
- introduce aligned `WindowPublicationState`
- migrate active/focus/capture readers and writers
- remove direct raw loads from exactness-sensitive routing paths

3. Pointer publication cutover
- introduce aligned `PointerPublicationState`
- migrate Win32 pointer readers and synthetic injection writers
- reconcile mirrored state with `src/drivers/mouse.rs`

4. Validation and ledger closeout
- add coherence corpus
- rerun exactness strict
- rerun touched-path gate
- update `docs/agent/decision-log.md` with the completed ordering/alignment contract

## Completion Criteria

- hot-path publication state no longer depends on unrelated free-floating atomics
- exactness-sensitive readers use coherent snapshot helpers
- cache-line ownership of pointer and window publication is explicit
- `Relaxed` atomics remain only where the value is an allocator or telemetry counter, not a hidden barrier
- strict exactness stays green after the cutover

## Cached Memory Note

If context is compressed later, resume from this document first. It is the authoritative cached plan for the `win32 exactness hot-path atomic/alignment burn-down` slice.
