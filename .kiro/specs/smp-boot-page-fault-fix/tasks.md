# Implementation Plan

- [x] 1. Write bug condition exploration test
  - **Property 1: Scheduler Initialization** - update_cpu_count() called before early return
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the bug exists
  - **Scoped PBT Approach**: For this deterministic bug, scope the property to the concrete failing case: cpu_count <= 1 causing early return before update_cpu_count()
  - Test that when cpu_count <= 1, startup_all_aps() calls update_cpu_count() BEFORE returning
  - Verify that scheduler has workers initialized after startup_all_aps() completes
  - The test assertions should verify: scheduler worker count > 0 AND can spawn tasks without "No workers available" error
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS with "No workers available" error (this is correct - it proves the bug exists)
  - Document counterexamples found: update_cpu_count() not called, scheduler has 0 workers, task spawn fails
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - Multi-CPU Initialization
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for non-buggy inputs (cpu_count > 1, multi-CPU systems)
  - Write property-based tests capturing observed behavior patterns:
    - Multi-CPU systems start all APs correctly
    - Per-CPU data structures are allocated for all CPUs
    - Scheduler allocates workers for all CPUs
    - Online CPU count reports correct number of CPUs
  - Property-based testing generates many test cases for stronger guarantees
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [x] 3. Fix for scheduler initialization order

  - [x] 3.1 Implement the fix in src/cpu/smp.rs
    - Locate the early return check `if cpu_count <= 1` at line 537 in startup_all_aps()
    - Move the `update_cpu_count(cpu_count)` call from line 549 to immediately after `drop(state)` at line 547
    - Add explanatory comment documenting why scheduler initialization must happen before early return
    - Fix state access in early return block to use `SMP_STATE.lock()` directly since state is dropped
    - Ensure the fix is minimal and only changes the order of operations
    - _Bug_Condition: isBugCondition(smp_state) where cpu_count <= 1 AND update_cpu_count_called == false AND early_return_executed()_
    - _Expected_Behavior: update_cpu_count() SHALL be called BEFORE the early return check, ensuring scheduler workers are initialized regardless of CPU count_
    - _Preservation: Multi-CPU initialization, AP startup, per-CPU data allocation, and online CPU tracking must remain unchanged_
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 3.1, 3.2, 3.3, 3.4_

  - [x] 3.2 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Scheduler Initialization Before Early Return
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms the expected behavior is satisfied
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed - scheduler has workers, can spawn tasks)
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [x] 3.3 Verify preservation tests still pass
    - **Property 2: Preservation** - Multi-CPU Initialization
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions in multi-CPU initialization, AP startup, per-CPU data allocation)
    - Confirm all tests still pass after fix (no regressions)

- [-] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
