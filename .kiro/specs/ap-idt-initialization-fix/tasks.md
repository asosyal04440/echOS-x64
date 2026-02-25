# Implementation Plan

- [ ] 1. Write bug condition exploration test
  - **Property 1: Fault Condition** - AP Triple Fault Without IDT
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate APs triple fault due to missing IDT
  - **Scoped PBT Approach**: Scope the property to concrete failing case - AP enters ap_entry with IDT base = 0x0 and encounters exception
  - Test that AP enters ap_entry with IDTR base = 0x0 and triple faults when exception occurs (from Fault Condition in design)
  - The test assertions should verify: AP reaches ap_entry, IDT base is 0x0, exception triggers triple fault
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS (this is correct - it proves the bug exists)
  - Document counterexamples found: AP LAPIC ID, exception type, triple fault sequence
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [ ] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - BSP and Existing Functionality
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for BSP initialization and exception handling
  - Write property-based tests capturing observed behavior patterns from Preservation Requirements:
    - BSP IDT initialization via interrupts::init() works correctly
    - BSP exception handling invokes correct handlers
    - AP assembly startup (GDT, paging, stack) executes correctly
    - IDT structure and handler registration unchanged
  - Property-based testing generates many test cases for stronger guarantees
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [ ] 3. Fix for AP IDT initialization

  - [ ] 3.1 Implement the fix in src/cpu/ap.rs
    - Move init_cpu_data() call to very beginning of ap_entry (required for current_cpu_id())
    - Move crate::interrupts::init_per_cpu() immediately after init_cpu_data()
    - Ensure IDT is loaded BEFORE any operation that could trigger exception (raw UART writes, APIC init, etc.)
    - Remove duplicate init_per_cpu() call later in the function
    - Verify cpu_data parameter provides correct cpu_id for per-CPU data initialization
    - _Bug_Condition: isBugCondition(input) where input.cpu_type = AP AND input.execution_point = "ap_entry" AND input.idt_base = 0x0 AND exception_occurs(input)_
    - _Expected_Behavior: AP has valid IDT loaded (IDTR base != 0x0) BEFORE any exception can occur_
    - _Preservation: BSP IDT initialization, BSP exception handling, AP assembly startup, IDT structure all unchanged_
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ] 3.2 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - AP Handles Exceptions With IDT
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms APs load IDT and handle exceptions correctly
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - Verify: AP loads IDT before Rust operations, IDTR base is non-zero, exceptions are handled, no triple faults
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [ ] 3.3 Verify preservation tests still pass
    - **Property 2: Preservation** - BSP and Existing Functionality
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm BSP IDT initialization, BSP exception handling, AP assembly startup all unchanged
    - Confirm all tests still pass after fix (no regressions)

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
