# Implementation Plan

- [x] 1. Write bug condition exploration test
  - **Property 1: Fault Condition** - Loop Executes Correct Number of Times
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the loop corruption exists
  - **Scoped PBT Approach**: Scope the property to concrete failing cases: cpu_count=4 (BSP + 3 APs) where the loop should execute 3 times but only executes 2 times due to corruption
  - Test that the per-CPU initialization loop executes exactly (cpu_count - 1) times with cpu_id values 1, 2, 3 (from Fault Condition in design)
  - Verify loop variable cpu_id maintains sequential values without corruption
  - Verify per_cpu_data.len() equals cpu_count after the loop
  - Run test on UNFIXED code by analyzing serial log output
  - **EXPECTED OUTCOME**: Test FAILS (loop executes 2 times instead of 3, cpu_id corrupts to 0 in iteration 2, per_cpu_data.len() is 3 instead of 4)
  - Document counterexamples found: "Loop iteration 2 shows 'cpu_id 0' instead of 'cpu_id 2'", "per_cpu_data.len() is 3 instead of 4"
  - Add debug output to track stack pointer values and loop variable addresses to confirm stack invalidation hypothesis
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.4, 2.5_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - BSP and AP Initialization Behavior
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for non-buggy code paths (BSP initialization, AP startup, scheduler update)
  - Write tests capturing observed behavior patterns from Preservation Requirements
  - Manual testing and log analysis is recommended due to complex hardware side effects
  - Test Case 1: BSP Initialization Preservation - verify BSP per-CPU data allocation produces same stack_top value and data structure contents
  - Test Case 2: Scheduler Update Preservation - verify scheduler receives correct cpu_count at same execution point
  - Test Case 3: AP Startup Preservation - verify AP startup code loading and sequence produce same serial log output
  - Test Case 4: Single-CPU Path Preservation - verify early return path for cpu_count <= 1 works correctly
  - Run tests on UNFIXED code and document observed behavior
  - **EXPECTED OUTCOME**: Tests PASS (confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 3. Fix for per-CPU loop corruption

  - [x] 3.1 Extract BSP initialization into separate function
    - Create new function `initialize_bsp_per_cpu()` in `src/cpu/smp.rs`
    - Move BSP per-CPU setup code (including stack switch) into this function
    - Ensure the stack switch inline assembly completes and returns before function exit
    - Preserve all debug serial_println! statements for observability
    - _Bug_Condition: isBugCondition(input) where input.iteration_number >= 2 AND stack_switch_occurred_in_same_function == true_
    - _Expected_Behavior: Loop executes exactly (cpu_count - 1) times with cpu_id maintaining sequential values (1, 2, 3, ..., cpu_count-1)_
    - _Preservation: BSP initialization, per-CPU data allocation, scheduler update, AP startup must remain unchanged_
    - _Requirements: 2.1, 2.2, 2.4, 2.5, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [x] 3.2 Call BSP initialization before the loop
    - Call `initialize_bsp_per_cpu()` before entering the per-CPU initialization loop in `startup_all_aps()`
    - Ensure the call completes and returns before the loop begins
    - This ensures the stack switch occurs in a different stack frame from the loop
    - Preserve the loop structure unchanged (still iterates from 1 to cpu_count)
    - _Bug_Condition: isBugCondition(input) where loop_variable_stored_on_invalidated_stack == true_
    - _Expected_Behavior: Loop variable cpu_id stored on valid stack frame, no corruption_
    - _Preservation: Lock acquisition pattern, debug output, loop structure must remain unchanged_
    - _Requirements: 2.1, 2.2, 2.4, 2.5, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [x] 3.3 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Loop Executes Correct Number of Times
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms the expected behavior is satisfied
    - Run bug condition exploration test from step 1
    - Verify loop executes exactly 3 times for cpu_count=4
    - Verify cpu_id values are sequential (1, 2, 3) without corruption
    - Verify per_cpu_data.len() equals 4 after the loop
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - _Requirements: 2.1, 2.2, 2.4, 2.5_

  - [x] 3.4 Verify preservation tests still pass
    - **Property 2: Preservation** - BSP and AP Initialization Behavior
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation tests from step 2
    - Verify BSP initialization produces same behavior (stack_top values, data structures)
    - Verify scheduler update occurs at same point with correct cpu_count
    - Verify AP startup sequence produces same serial log output
    - Verify single-CPU path still works correctly
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm all tests still pass after fix (no regressions)

- [x] 4. Checkpoint - Ensure all tests pass
  - Verify bug condition exploration test passes (loop executes correct number of times)
  - Verify preservation tests pass (BSP initialization, scheduler update, AP startup unchanged)
  - Verify full system boot with 4 CPUs shows all APs come online
  - Verify final "X/Y CPUs online" message shows 4/4 CPUs
  - Ensure all tests pass, ask the user if questions arise
