# Per-CPU Loop Corruption Fix Design

## Overview

The per-CPU data initialization loop in `startup_all_aps()` exhibits memory corruption where the loop variable `cpu_id` becomes corrupted during the second iteration, changing from 2 to 0. This causes the loop to execute only twice (for cpu_id 1 and a corrupted iteration) instead of three times (for cpu_id 1, 2, and 3). The root cause is stack corruption triggered by the inline assembly stack switch operation that occurs during BSP initialization. When the BSP switches to a newly allocated kernel stack using `mov rsp, {stack_top}`, the current stack frame (containing the loop variable `cpu_id`) is invalidated, leading to undefined behavior when the loop variable is subsequently accessed.

## Glossary

- **Bug_Condition (C)**: The condition that triggers the bug - when the per-CPU initialization loop executes and the loop variable `cpu_id` is accessed after a stack switch operation has invalidated the stack frame
- **Property (P)**: The desired behavior - the loop SHALL execute exactly (cpu_count - 1) times with cpu_id maintaining correct sequential values (1, 2, 3, ..., cpu_count-1)
- **Preservation**: Existing BSP initialization, per-CPU data allocation, and AP startup behavior that must remain unchanged by the fix
- **startup_all_aps()**: The function in `src/cpu/smp.rs` that initializes per-CPU data structures for all Application Processors
- **BSP (Bootstrap Processor)**: The first CPU (cpu_id 0) that boots the system and initializes other CPUs
- **AP (Application Processor)**: Secondary CPUs (cpu_id 1, 2, 3, ...) that are started by the BSP
- **Stack Switch**: The inline assembly operation `mov rsp, {stack_top}` that changes the stack pointer to a newly allocated kernel stack
- **SMP_STATE**: The global state structure containing per-CPU data arrays and system configuration

## Bug Details

### Fault Condition

The bug manifests when the per-CPU initialization loop in `startup_all_aps()` executes for cpu_id values 1 through (cpu_count - 1). During the second iteration (cpu_id = 2), the loop variable becomes corrupted and displays as 0 instead of 2 in debug output. This corruption occurs because the BSP initialization code performs a stack switch using inline assembly, which invalidates the stack frame containing local variables. When the loop subsequently accesses the `cpu_id` variable, it reads corrupted memory.

**Formal Specification:**
```
FUNCTION isBugCondition(input)
  INPUT: input of type LoopIteration
  OUTPUT: boolean
  
  RETURN input.iteration_number >= 2
         AND input.cpu_id_variable_corrupted == true
         AND stack_switch_occurred_in_same_function == true
         AND loop_variable_stored_on_invalidated_stack == true
END FUNCTION
```

### Examples

- **Iteration 1 (cpu_id = 1)**: Loop executes correctly, debug output shows "cpu_id 1 stack_top = 0x444444478a90", per-CPU data is allocated successfully
- **Iteration 2 (cpu_id = 2)**: Loop begins correctly with "Creating per_cpu_data for cpu_id 2", but then debug output shows "cpu_id 0 stack_top = 0x444444482b50" (CORRUPTION - should be cpu_id 2)
- **Iteration 3 (cpu_id = 3)**: Loop never executes because the corrupted cpu_id value causes premature loop termination
- **Edge case (cpu_count = 2)**: With only 2 CPUs, the loop should execute once for cpu_id 1, and the bug may not manifest if the stack corruption doesn't affect the single iteration

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- BSP (cpu_id 0) per-CPU data initialization must continue to work correctly, including stack allocation and stack switching
- Per-CPU data structure allocation (PerCpuData, CpuData, syscall stacks) must continue to use the same allocation strategy
- The scheduler update with cpu_count must continue to occur at the same point in the function
- AP startup code loading and AP startup sequence must remain unchanged
- All fields in SmpState (per_cpu_data, ap_started, syscall_cpu_data, syscall_stacks) must continue to be populated correctly

**Scope:**
All code paths that do NOT involve the per-CPU initialization loop should be completely unaffected by this fix. This includes:
- BSP initialization logic (before the loop)
- AP startup code loading and execution
- Scheduler initialization
- Single-CPU early return path
- APIC ID lookup and mapping

## Hypothesized Root Cause

Based on the bug manifestation and code analysis, the root cause is:

**Stack Frame Invalidation by Inline Assembly Stack Switch**

The BSP initialization code performs a stack switch using inline assembly:
```rust
core::arch::asm!(
    "mov rsp, {stack_top}",
    stack_top = in(reg) stack_top,
    options(nostack)
);
```

This operation changes the stack pointer to a newly allocated kernel stack, which invalidates the current stack frame. The problem is that this stack switch occurs within the same function (`startup_all_aps()`) that contains the per-CPU initialization loop. When the loop variable `cpu_id` is accessed in subsequent iterations, it attempts to read from the old (now invalid) stack location, resulting in corrupted values.

**Why the first iteration works:**
The first iteration (cpu_id = 1) executes correctly because the stack switch hasn't occurred yet when the loop begins, or the compiler happens to place the loop variable in a register that survives the stack switch.

**Why the second iteration fails:**
By the second iteration (cpu_id = 2), the stack switch has already occurred during BSP initialization. The loop variable `cpu_id` is stored on the old stack, which is no longer valid. When the code tries to read `cpu_id`, it gets garbage data (in this case, 0).

**Alternative hypotheses (less likely):**
1. **Optimizer bug**: The compiler might be incorrectly optimizing the loop variable storage, but this is unlikely given the consistent corruption pattern
2. **Memory allocator corruption**: The Box::leak allocations might be corrupting memory, but this would likely cause more widespread issues
3. **Lock contention**: Multiple lock acquisitions might cause issues, but this wouldn't explain the specific corruption pattern

## Correctness Properties

Property 1: Fault Condition - Loop Executes Correct Number of Times

_For any_ system configuration where cpu_count > 1, the per-CPU initialization loop in the fixed startup_all_aps function SHALL execute exactly (cpu_count - 1) times, with the loop variable cpu_id taking sequential values from 1 to (cpu_count - 1) without corruption.

**Validates: Requirements 2.1, 2.2, 2.4, 2.5**

Property 2: Preservation - BSP and AP Initialization Behavior

_For any_ code path that does NOT involve the per-CPU initialization loop (BSP initialization, AP startup, scheduler update), the fixed code SHALL produce exactly the same behavior as the original code, preserving all existing initialization logic and data structure population.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5**

## Fix Implementation

### Changes Required

Assuming our root cause analysis is correct (stack frame invalidation by inline assembly):

**File**: `src/cpu/smp.rs`

**Function**: `startup_all_aps()`

**Specific Changes**:

1. **Separate BSP Initialization into a Separate Function**: Extract the BSP per-CPU setup code (including the stack switch) into a separate function called `initialize_bsp_per_cpu()`. This ensures the stack switch occurs in a different stack frame from the per-CPU initialization loop.

2. **Call BSP Initialization Before the Loop**: Call `initialize_bsp_per_cpu()` before entering the per-CPU initialization loop. This ensures the stack switch completes and returns before the loop begins, preventing stack frame invalidation.

3. **Preserve Loop Structure**: Keep the per-CPU initialization loop structure unchanged, ensuring it still iterates from 1 to cpu_count with the same allocation logic.

4. **Maintain Lock Acquisition Pattern**: Keep the same pattern of lock acquisitions within the loop to avoid introducing new concurrency issues.

5. **Preserve Debug Output**: Keep all debug serial_println! statements to maintain observability and debugging capability.

**Pseudocode for the fix:**
```rust
fn initialize_bsp_per_cpu() {
    // Move all BSP initialization code here, including stack switch
    // This function returns after the stack switch completes
}

pub fn startup_all_aps() {
    // Call BSP initialization in a separate function
    initialize_bsp_per_cpu();
    
    // Now the per-CPU initialization loop runs on the new stack
    // with a fresh stack frame that won't be invalidated
    let cpu_count = SMP_STATE.lock().cpu_count;
    for cpu_id in 1..cpu_count {
        // Loop body unchanged
    }
    
    // Rest of the function unchanged
}
```

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code by observing loop execution and variable corruption, then verify the fix works correctly and preserves existing behavior.

### Exploratory Fault Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm that the loop variable `cpu_id` becomes corrupted during the second iteration, and confirm that the root cause is stack frame invalidation.

**Test Plan**: Analyze serial log output from the UNFIXED code to observe the loop corruption pattern. Add additional debug output to track stack pointer values and loop variable addresses to confirm the stack invalidation hypothesis.

**Test Cases**:
1. **Loop Iteration Count Test**: Count the number of "Creating per_cpu_data for cpu_id" messages in serial output (will show 2 instead of 3 on unfixed code)
2. **Loop Variable Corruption Test**: Observe that cpu_id displays as 0 in the second iteration's stack_top message (will fail on unfixed code)
3. **Stack Pointer Tracking Test**: Add debug output to print the stack pointer (rsp) value before and after the stack switch, and at each loop iteration (will show stack pointer change on unfixed code)
4. **Per-CPU Data Length Test**: Verify that per_cpu_data.len() reaches 4 (BSP + 3 APs) after the loop (will show len=3 on unfixed code)

**Expected Counterexamples**:
- Serial log shows only 2 loop iterations instead of 3
- Second iteration shows "cpu_id 0" instead of "cpu_id 2"
- Stack pointer changes during BSP initialization and remains different during loop execution
- per_cpu_data vector has length 3 instead of 4 after the loop

### Fix Checking

**Goal**: Verify that for all system configurations where cpu_count > 1, the fixed function executes the loop the correct number of times with uncorrupted loop variables.

**Pseudocode:**
```
FOR ALL cpu_count WHERE cpu_count > 1 DO
  result := startup_all_aps_fixed()
  ASSERT loop_executed_count == (cpu_count - 1)
  ASSERT per_cpu_data.len() == cpu_count
  ASSERT all cpu_id values in debug output are sequential (1, 2, 3, ...)
END FOR
```

### Preservation Checking

**Goal**: Verify that for all code paths that do NOT involve the per-CPU initialization loop, the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL code_path WHERE code_path NOT IN [per_cpu_loop] DO
  ASSERT startup_all_aps_original(code_path) = startup_all_aps_fixed(code_path)
END FOR
```

**Testing Approach**: Manual testing and log analysis is recommended for preservation checking because:
- The function has complex side effects (memory allocation, stack switching, hardware initialization)
- Property-based testing would require extensive mocking of hardware and memory subsystems
- Serial log output provides clear evidence of correct behavior for each code path

**Test Plan**: Observe behavior on UNFIXED code first for BSP initialization, AP startup, and scheduler update, then verify the FIXED code produces identical serial log output for these code paths.

**Test Cases**:
1. **BSP Initialization Preservation**: Verify that BSP per-CPU data is allocated correctly with the same stack_top value and data structure contents
2. **Scheduler Update Preservation**: Verify that the scheduler receives the correct cpu_count value at the same point in execution
3. **AP Startup Preservation**: Verify that AP startup code loading and AP startup sequence produce the same serial log output
4. **Single-CPU Path Preservation**: Verify that the early return path for cpu_count <= 1 continues to work correctly

### Unit Tests

- Test that the loop executes the correct number of times for cpu_count values 2, 3, 4, 8
- Test that cpu_id values in debug output are sequential and uncorrupted
- Test that per_cpu_data vector length equals cpu_count after the loop
- Test that stack pointer values are stable during loop execution

### Property-Based Tests

Property-based testing is not practical for this bug fix due to:
- Hardware-specific initialization that cannot be easily mocked
- Inline assembly operations that require real CPU execution
- Complex side effects involving memory allocation and stack manipulation

Instead, we rely on:
- Serial log analysis for multiple boot attempts
- Manual verification of loop execution counts
- Comparison of debug output between unfixed and fixed versions

### Integration Tests

- Test full system boot with 4 CPUs and verify all APs come online
- Test that all 4 CPUs are reported as online in the final "X/Y CPUs online" message
- Test that the scheduler correctly recognizes all 4 CPUs
- Test that tasks can be scheduled on all 4 CPUs after boot
