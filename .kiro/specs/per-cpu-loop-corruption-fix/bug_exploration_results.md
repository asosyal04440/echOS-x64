# Bug Condition Exploration Test Results

## Test Execution Date
2026-02-21

## Test Status
**FAILED (Expected)** - Bug is present in unfixed code

## Counterexamples Found

### 1. Loop Variable Corruption
- **Expected**: Loop variable `cpu_id` should maintain sequential values 1, 2, 3
- **Actual**: Loop variable `cpu_id` corrupted to 0 in all iterations
- **Evidence**: Serial log shows "SMP: cpu_id 0 stack_top = 0x444444478ab0" when it should show cpu_id 1, 2, 3

### 2. Insufficient Per-CPU Data Initialization
- **Expected**: per_cpu_data.len() should be 4 (BSP + 3 APs)
- **Actual**: per_cpu_data.len() is 2 (BSP + 1 AP only)
- **Evidence**: Serial log line 83 shows "cpu_id 0 added to per_cpu_data (len=2)"

### 3. Loop Execution Count
- **Expected**: Loop should execute 3 times (for cpu_id 1, 2, 3)
- **Actual**: Loop appears to execute only once successfully, then system behavior becomes undefined
- **Evidence**: Only one "Creating per_cpu_data for cpu_id 1" message per boot attempt

## Root Cause Confirmation

The test results confirm the hypothesized root cause: **Stack Frame Invalidation by Inline Assembly Stack Switch**

The BSP initialization code performs a stack switch using inline assembly:
```rust
core::arch::asm!(
    "mov rsp, {stack_top}",
    stack_top = in(reg) stack_top,
    options(nostack)
);
```

This operation changes the stack pointer to a newly allocated kernel stack, which invalidates the current stack frame. When the loop variable `cpu_id` is accessed in subsequent iterations, it attempts to read from the old (now invalid) stack location, resulting in corrupted values (reading 0 instead of the correct cpu_id).

## Test Conclusion

The bug condition exploration test successfully surfaced counterexamples that demonstrate:
1. The loop variable becomes corrupted after the stack switch
2. The loop fails to execute the correct number of times
3. Per-CPU data structures are not initialized for all APs

These findings validate that the bug exists and confirm the need for the proposed fix: extracting BSP initialization into a separate function to ensure the stack switch occurs in a different stack frame from the per-CPU initialization loop.
