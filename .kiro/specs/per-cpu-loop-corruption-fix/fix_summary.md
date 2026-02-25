# Per-CPU Loop Corruption Fix - Summary

## Fix Implementation Date
2026-02-21

## Bug Description
The per-CPU data initialization loop in `startup_all_aps()` only executed once (for cpu_id 1) instead of 3 times (for cpu_id 1, 2, 3). The loop variable `cpu_id` was corrupted to 0 during execution, preventing proper initialization of per-CPU data structures for Application Processors 2 and 3.

## Root Cause
Stack frame invalidation by inline assembly stack switch operation. The BSP initialization code performed a stack switch using `mov rsp, {stack_top}` within the same function that contained the per-CPU initialization loop. This invalidated the stack frame containing the loop variable `cpu_id`, causing it to read corrupted values from the old (invalid) stack location.

## Fix Applied

### Changes Made
1. **Extracted BSP initialization into separate function** (`initialize_bsp_per_cpu()`)
   - Moved all BSP per-CPU setup code including the stack switch into a new function
   - Added documentation explaining the purpose of the separation
   - File: `src/cpu/smp.rs` (lines 471-527)

2. **Updated startup_all_aps() to call the new function**
   - Replaced inline BSP initialization code with a call to `initialize_bsp_per_cpu()`
   - Ensured the stack switch completes and returns before the per-CPU initialization loop begins
   - File: `src/cpu/smp.rs` (line 532)

### Code Changes
```rust
// NEW FUNCTION: initialize_bsp_per_cpu()
/// Initialize BSP (Bootstrap Processor) per-CPU data structures
/// This function is separate to ensure the stack switch occurs in a different
/// stack frame from the AP initialization loop, preventing stack corruption
fn initialize_bsp_per_cpu() {
    // ... BSP initialization code including stack switch ...
}

// UPDATED FUNCTION: startup_all_aps()
pub fn startup_all_aps() {
    // BSP per-cpu setup - call separate function to ensure stack switch
    // occurs in a different stack frame from the AP initialization loop
    initialize_bsp_per_cpu();
    
    // Prepare AP per-cpu data - read cpu_count once to avoid multiple lock acquisitions
    let cpu_count = SMP_STATE.lock().cpu_count;
    
    for cpu_id in 1..cpu_count {
        // Loop body unchanged - now executes correctly without corruption
        // ...
    }
    // ... rest of function unchanged ...
}
```

## Test Results

### Bug Condition Exploration Test (BEFORE fix)
- ❌ Loop executed only 1 time instead of 3
- ❌ Loop variable `cpu_id` corrupted to 0 instead of maintaining values 1, 2, 3
- ❌ per_cpu_data.len() was 2 instead of 4

### Bug Condition Exploration Test (AFTER fix)
- ✅ Loop executed 3 times (correct)
- ✅ Loop variable `cpu_id` maintained correct values: 1, 2, 3 (no corruption)
- ✅ per_cpu_data.len() is 4 (BSP + 3 APs)

### Preservation Tests (AFTER fix)
- ✅ BSP initialization works correctly
- ✅ Scheduler receives correct cpu_count (4)
- ✅ AP startup code loading works correctly
- ✅ Per-CPU data structure population works correctly

### Serial Log Evidence (AFTER fix)
```
[81] SMP: Creating per_cpu_data for cpu_id 1
[82] SMP: cpu_id 1 stack_top = 0x444444478ab0
[83] SMP: cpu_id 1 added to per_cpu_data (len=2)
[84] SMP: Creating per_cpu_data for cpu_id 2
[85] SMP: cpu_id 2 stack_top = 0x444444482b90
[86] SMP: cpu_id 2 added to per_cpu_data (len=3)
[87] SMP: Creating per_cpu_data for cpu_id 3
[88] SMP: cpu_id 3 stack_top = 0x44444448cc50
[89] SMP: cpu_id 3 added to per_cpu_data (len=4)
[90] Scheduler updated for 4 CPUs
```

## Verification

### Compilation
- ✅ No compilation errors
- ✅ No new warnings introduced

### Functional Testing
- ✅ Loop executes correct number of times (3 iterations for cpu_count=4)
- ✅ Loop variable maintains correct sequential values (1, 2, 3)
- ✅ per_cpu_data vector reaches correct length (4)
- ✅ Scheduler receives correct cpu_count (4)
- ✅ AP startup code loads successfully
- ✅ All preservation tests pass (no regressions)

## Impact

### Fixed Issues
- Per-CPU data structures are now initialized for all 3 Application Processors
- Loop variable no longer corrupts during execution
- System can now properly utilize all 4 CPUs

### Preserved Behavior
- BSP initialization continues to work correctly
- Scheduler update occurs at the same point with correct cpu_count
- AP startup code loading sequence remains unchanged
- All debug output preserved for observability

## Conclusion

The fix successfully resolves the per-CPU loop corruption bug by ensuring the stack switch operation occurs in a separate stack frame from the per-CPU initialization loop. This prevents stack frame invalidation and allows the loop variable to maintain its correct value throughout all iterations. All tests pass, confirming the bug is fixed and no regressions were introduced.
