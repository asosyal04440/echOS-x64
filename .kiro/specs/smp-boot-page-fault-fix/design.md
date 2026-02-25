# SMP Scheduler Initialization Order Fix Design

## Overview

The echOS kernel fails to initialize the scheduler during SMP initialization because `update_cpu_count()` is called AFTER an early return check in `startup_all_aps()`. When `cpu_count` is 0 or 1, the function returns early without initializing scheduler workers, causing "ERROR: No workers available to spawn task!" and preventing the system from functioning.

The fix requires moving the `update_cpu_count(cpu_count)` call to BEFORE the `if cpu_count <= 1` early return check. This ensures scheduler workers are always initialized regardless of CPU count, allowing the system to spawn tasks even on single-CPU systems.

## Glossary

- **Bug_Condition (C)**: The condition where `startup_all_aps()` returns early when `cpu_count <= 1` before calling `update_cpu_count()`
- **Property (P)**: `update_cpu_count()` must be called before the early return check to initialize scheduler workers
- **Preservation**: Multi-CPU initialization, AP startup, per-CPU data allocation, and syscall handling must remain unchanged
- **BSP**: Bootstrap Processor - the first CPU that boots and initializes the system
- **AP**: Application Processor - additional CPUs started after BSP initialization
- **cpu_count**: The number of CPUs detected by ACPI and stored in `SMP_STATE.cpu_count`
- **update_cpu_count()**: Scheduler function that allocates workers and stealers for each CPU
- **Worker**: Scheduler component that can spawn and execute tasks on a CPU
- **Early Return**: The `if cpu_count <= 1` check at line 537 that returns without starting APs

## Bug Details

### Fault Condition

The bug manifests when `startup_all_aps()` checks `if cpu_count <= 1` at line 537 and returns early without calling `update_cpu_count()` at line 549. This leaves the scheduler uninitialized with no workers available, causing any attempt to spawn a task to fail with "ERROR: No workers available to spawn task!".

The boot logs show:
- Line 39: "ERROR: No workers available to spawn task!" - scheduler has no workers
- Line 72: "ACPI: 4 CPUs detected" - ACPI correctly detects 4 CPUs
- Line 75: "SMP: Found 4 CPUs via ACPI" - SMP state is initialized with 4 CPUs
- Line 79: "SMP: startup_all_aps cpu_count=0" - BUT cpu_count is 0 when startup_all_aps runs
- Line 80: "SMP: 0/0 CPUs online" - No CPUs are online because early return prevented AP startup

**Formal Specification:**
```
FUNCTION isBugCondition(smp_state)
  INPUT: smp_state containing (cpu_count, scheduler_initialized, update_cpu_count_called)
  OUTPUT: boolean
  
  RETURN smp_state.cpu_count <= 1
         AND NOT smp_state.update_cpu_count_called
         AND smp_state.scheduler_initialized == false
         AND early_return_executed()
END FUNCTION
```

### Examples

- **Single CPU System**: When cpu_count = 1, the function returns at line 537 without calling update_cpu_count() at line 549, leaving scheduler with no workers
- **Zero CPU Count**: When cpu_count = 0 (as shown in logs), the function returns immediately, scheduler is never initialized, and "No workers available" error occurs
- **Multi-CPU System**: When cpu_count > 1, the function continues past the early return, calls update_cpu_count(), and initializes scheduler correctly
- **Root Cause**: The cpu_count value appears to be 0 when startup_all_aps() runs, despite ACPI detecting 4 CPUs, suggesting the SMP_STATE.cpu_count is not properly initialized before this function is called

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- Multi-CPU initialization (cpu_count > 1) must continue to work correctly with all APs started
- BSP per-CPU data allocation and initialization must remain unchanged
- AP per-CPU data allocation and initialization must remain unchanged
- Syscall stack allocation and CpuData structure initialization must remain unchanged
- The AP startup sequence (load_ap_startup_code, startup_ap) must remain unchanged
- Online CPU counting and reporting must remain unchanged

**Scope:**
All code paths that involve multi-CPU initialization should be completely unaffected by this fix. This includes:
- AP startup and initialization (startup_ap function)
- Per-CPU data allocation for APs
- Syscall stack allocation for all CPUs
- APIC initialization and IPI sending
- Online CPU tracking and reporting

## Hypothesized Root Cause

Based on the boot logs and code analysis, the root cause is:

1. **Initialization Order Bug**: The `startup_all_aps()` function at line 537 checks `if cpu_count <= 1` and returns early, but the call to `update_cpu_count(cpu_count)` is at line 549, AFTER this early return.

2. **Zero CPU Count**: The logs show "SMP: startup_all_aps cpu_count=0" at line 79, indicating that `SMP_STATE.cpu_count` is 0 when the function runs, despite ACPI detecting 4 CPUs at line 72.

3. **Scheduler Not Initialized**: Because the early return prevents `update_cpu_count()` from being called, the scheduler never allocates workers, causing "ERROR: No workers available to spawn task!" at line 39.

4. **No APs Started**: The early return also prevents the AP startup loop from executing, so no APs are started and the system reports "SMP: 0/0 CPUs online" at line 80.

5. **Missing CPU Count Initialization**: The deeper issue is that `SMP_STATE.cpu_count` appears to be 0 when `startup_all_aps()` is called, even though ACPI detected 4 CPUs. This suggests that the CPU count from ACPI parsing is not being properly stored in `SMP_STATE.cpu_count` before `startup_all_aps()` is called.

## Correctness Properties

Property 1: Scheduler Initialization Before Early Return

_For any_ execution where `startup_all_aps()` is called, the code SHALL call `update_cpu_count(cpu_count)` BEFORE checking `if cpu_count <= 1`, ensuring scheduler workers are initialized regardless of CPU count and the system can spawn tasks.

**Validates: Requirements 2.1, 2.2, 2.3, 2.4**

Property 2: Preservation - Multi-CPU Initialization

_For any_ execution where `cpu_count > 1`, the fixed code SHALL produce exactly the same behavior as the original code, preserving all existing functionality for AP startup, per-CPU data allocation, and online CPU tracking.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4**

## Fix Implementation

### Changes Required

The fix is straightforward: move the `update_cpu_count(cpu_count)` call to BEFORE the `if cpu_count <= 1` early return check.

**File**: `src/cpu/smp.rs`

**Function**: `startup_all_aps()`

**Current Code Structure (lines 537-549)**:
```rust
if cpu_count <= 1 {
    let online = state.online_cpus.load(Ordering::Acquire);
    let total = state.cpu_count;
    drop(state);
    crate::serial_println!("SMP: startup_all_aps cpu_count={}", cpu_count);
    crate::serial_println!("SMP: {}/{} CPUs online", online, total);
    return;
}

drop(state);

// Scheduler'ı tüm CPU'lar için güncelle (Worker/Stealer alloc et)
crate::task::scheduler::update_cpu_count(cpu_count);
```

**Fixed Code Structure**:
```rust
drop(state);

// CRITICAL: Initialize scheduler BEFORE early return check
// This ensures scheduler workers are allocated even for single-CPU systems
// Without this, the system cannot spawn tasks and fails with "No workers available"
crate::task::scheduler::update_cpu_count(cpu_count);

if cpu_count <= 1 {
    let online = SMP_STATE.lock().online_cpus.load(Ordering::Acquire);
    let total = SMP_STATE.lock().cpu_count;
    crate::serial_println!("SMP: startup_all_aps cpu_count={}", cpu_count);
    crate::serial_println!("SMP: {}/{} CPUs online", online, total);
    return;
}
```

**Specific Changes**:
1. **Move update_cpu_count() call**: Move line 549 to immediately after line 547 (after `drop(state)`)
2. **Add explanatory comment**: Document why scheduler initialization must happen before the early return
3. **Fix state access**: Since `state` is dropped before the early return, access `SMP_STATE.lock()` directly in the early return block

**Rationale**:
- The scheduler needs workers initialized regardless of CPU count
- Single-CPU systems (BSP only) still need at least one worker to spawn tasks
- Moving the call before the early return ensures scheduler is always initialized
- This fixes both the "No workers available" error and enables proper system initialization

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code (scheduler not initialized, no workers available), then verify the fix works correctly and preserves existing behavior.

### Exploratory Fault Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm or refute the root cause analysis.

**Test Plan**: The bug is already demonstrated by the boot logs showing:
1. "ERROR: No workers available to spawn task!" at line 39
2. "SMP: startup_all_aps cpu_count=0" at line 79
3. "SMP: 0/0 CPUs online" at line 80

We can verify the root cause by:
1. Confirming that `update_cpu_count()` is called AFTER the early return check in the source code
2. Adding debug output to verify scheduler worker count before and after the early return
3. Confirming that when cpu_count <= 1, the function returns without initializing scheduler

**Test Cases**:
1. **Scheduler Worker Count**: Verify that scheduler has 0 workers when startup_all_aps returns early (will confirm on unfixed code)
2. **Early Return Execution**: Add debug output to confirm the early return is executed when cpu_count <= 1 (will succeed on unfixed code)
3. **update_cpu_count Not Called**: Verify that update_cpu_count is not called when early return executes (will confirm on unfixed code)
4. **Task Spawn Failure**: Attempt to spawn a task after startup_all_aps returns early (will fail with "No workers available" on unfixed code)

**Expected Counterexamples**:
- Scheduler worker count is 0 after startup_all_aps returns
- update_cpu_count() is never called when cpu_count <= 1
- Task spawn fails with "No workers available" error

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds, the fixed function produces the expected behavior.

**Pseudocode:**
```
FOR ALL smp_state WHERE isBugCondition(smp_state) DO
  result := startup_all_aps_fixed()
  ASSERT result.scheduler_initialized == true
  ASSERT result.worker_count > 0
  ASSERT result.can_spawn_tasks == true
END FOR
```

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold (multi-CPU systems), the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL smp_state WHERE NOT isBugCondition(smp_state) DO
  ASSERT startup_all_aps_original(smp_state) = startup_all_aps_fixed(smp_state)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many test cases automatically across the input domain
- It catches edge cases that manual unit tests might miss
- It provides strong guarantees that behavior is unchanged for multi-CPU systems

**Test Plan**: Observe behavior on UNFIXED code first for multi-CPU initialization, then write property-based tests capturing that behavior.

**Test Cases**:
1. **Multi-CPU Initialization Preservation**: Verify that systems with cpu_count > 1 continue to start all APs correctly
2. **Per-CPU Data Preservation**: Verify that per-CPU data structures are allocated correctly for all CPUs
3. **Online CPU Count Preservation**: Verify that the correct number of CPUs are reported as online after initialization
4. **Scheduler Worker Allocation Preservation**: Verify that scheduler allocates workers for all CPUs in multi-CPU systems

### Unit Tests

- Test that update_cpu_count() is called before the early return check
- Test that scheduler has workers initialized after startup_all_aps() completes
- Test that single-CPU systems can spawn tasks after initialization
- Test that multi-CPU systems continue to work correctly

### Property-Based Tests

- Generate random CPU configurations (1-16 CPUs) and verify scheduler is always initialized
- Verify that all CPUs have workers allocated after initialization
- Test that task spawning works across many scenarios with different CPU counts

### Integration Tests

- Test full kernel boot with single CPU and verify scheduler can spawn tasks
- Test full kernel boot with multiple CPUs and verify all CPUs come online
- Test that the system continues to run normally after scheduler initialization
