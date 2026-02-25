# Bugfix Requirements Document

## Introduction

The per-CPU data initialization loop in `startup_all_aps()` only executes once (for cpu_id 1) instead of 3 times (for cpu_id 1, 2, 3). Debug logs show the loop variable `cpu_id` is corrupted, displaying as 0 when it should be 1. This prevents proper initialization of per-CPU data structures for Application Processors (APs) 2 and 3, causing the system to fail to utilize all 4 CPUs.

Root cause analysis indicates a type mismatch in the `syscall_stacks` field: the field is declared as `Vec<&'static mut [u8; SYSCALL_STACK_SIZE]>` (fixed-size array reference) but the code attempts to push `&'static mut [u8]` (slice reference). This type mismatch likely causes memory corruption or undefined behavior that breaks the loop iteration.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN the initialization loop in `startup_all_aps()` executes for cpu_id range 1..cpu_count (where cpu_count=4) THEN the loop only executes once instead of 3 times

1.2 WHEN the loop variable `cpu_id` is printed in debug output during the first iteration THEN it displays as 0 instead of the expected value 1

1.3 WHEN a slice reference `&'static mut [u8]` is pushed to `syscall_stacks` field expecting `&'static mut [u8; SYSCALL_STACK_SIZE]` THEN memory corruption or undefined behavior occurs

1.4 WHEN the loop exits prematurely after one iteration THEN per-CPU data structures for cpu_id 2 and 3 are not initialized

1.5 WHEN APs with cpu_id 2 and 3 attempt to start THEN they fail due to missing per-CPU data structures

### Expected Behavior (Correct)

2.1 WHEN the initialization loop in `startup_all_aps()` executes for cpu_id range 1..cpu_count (where cpu_count=4) THEN the loop SHALL execute exactly 3 times (for cpu_id 1, 2, and 3)

2.2 WHEN the loop variable `cpu_id` is accessed during each iteration THEN it SHALL maintain its correct value (1, 2, then 3) without corruption

2.3 WHEN allocating syscall stacks THEN the code SHALL use the correct type that matches the `syscall_stacks` field declaration

2.4 WHEN the loop completes THEN per-CPU data structures SHALL be initialized for all 3 APs (cpu_id 1, 2, and 3)

2.5 WHEN APs with cpu_id 2 and 3 attempt to start THEN they SHALL have properly initialized per-CPU data structures available

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the BSP (Bootstrap Processor, cpu_id 0) initializes its per-CPU data THEN the system SHALL CONTINUE TO initialize it correctly before the AP initialization loop

3.2 WHEN per-CPU data is allocated for cpu_id 1 THEN the system SHALL CONTINUE TO allocate and initialize it correctly

3.3 WHEN the scheduler is updated after per-CPU initialization THEN it SHALL CONTINUE TO receive the correct CPU count

3.4 WHEN other fields in `SmpState` are populated during the loop (per_cpu_data, ap_started, syscall_cpu_data) THEN they SHALL CONTINUE TO be populated correctly for all iterations

3.5 WHEN the system boots with a different CPU count THEN the loop SHALL CONTINUE TO execute the correct number of times (cpu_count - 1)
