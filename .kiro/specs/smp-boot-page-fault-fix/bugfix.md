# Bugfix Requirements Document

## Introduction

The echOS kernel fails to initialize the scheduler and start Application Processors (APs) during SMP (Symmetric Multiprocessing) initialization. The boot logs show "ERROR: No workers available to spawn task!" at line 39 and "SMP: 0/0 CPUs online" at line 80, despite ACPI detecting 4 CPUs.

The root cause is an initialization order bug in `startup_all_aps()`: the function calls `update_cpu_count()` to initialize scheduler workers AFTER an early return check `if cpu_count <= 1`. When `cpu_count` is 0 or 1, the function returns early without initializing the scheduler, leaving the system with no workers available and no APs started. This prevents the kernel from spawning tasks and completing initialization.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN `startup_all_aps()` is called and `SMP_STATE.cpu_count` is 0 or 1 THEN the function returns early at line 537 without calling `update_cpu_count()`

1.2 WHEN `update_cpu_count()` is not called THEN scheduler workers remain uninitialized and any attempt to spawn a task fails with "ERROR: No workers available to spawn task!"

1.3 WHEN the early return occurs THEN no Application Processors (APs) are started, resulting in "SMP: 0/0 CPUs online" despite ACPI detecting multiple CPUs

1.4 WHEN the scheduler has no workers THEN the system cannot spawn tasks and fails to complete initialization

### Expected Behavior (Correct)

2.1 WHEN `startup_all_aps()` is called THEN `update_cpu_count()` SHALL be called BEFORE the early return check to ensure scheduler workers are initialized regardless of CPU count

2.2 WHEN `update_cpu_count()` is called with the correct CPU count THEN the scheduler SHALL initialize workers for all detected CPUs

2.3 WHEN the scheduler is initialized THEN the system SHALL be able to spawn tasks without "No workers available" errors

2.4 WHEN the system has only one CPU (BSP only) THEN the scheduler SHALL still be initialized with at least one worker for the BSP

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the system has multiple CPUs (cpu_count > 1) THEN the system SHALL CONTINUE TO start all APs and initialize them correctly

3.2 WHEN BSP per-CPU setup completes THEN the system SHALL CONTINUE TO allocate per-CPU data structures and syscall stacks correctly

3.3 WHEN APs are started THEN the system SHALL CONTINUE TO report the correct number of online CPUs

3.4 WHEN the scheduler is initialized for multiple CPUs THEN the system SHALL CONTINUE TO allocate workers and stealers for load balancing
