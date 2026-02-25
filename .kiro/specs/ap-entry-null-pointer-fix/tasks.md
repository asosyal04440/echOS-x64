# Implementation Plan

- [ ] 1. Write bug condition exploration test
  - **Property 1: Fault Condition** - Entry and PML4 Pointer Initialization
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the bug exists
  - **Scoped PBT Approach**: Scope the property to concrete failing cases - any call to `prepare_ap_startup_data()` with valid stack_top and cpu_data values
  - Test that `prepare_ap_startup_data(stack_top, cpu_data)` results in `ApStartupData.entry` pointing to `ap_entry` function address
  - Test that `prepare_ap_startup_data(stack_top, cpu_data)` results in `ApStartupData.pml4_phys` containing a valid non-zero physical address
  - The test assertions should verify: `data.entry == address_of(ap_entry)` AND `data.pml4_phys != 0`
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS (this is correct - it proves the bug exists)
  - Document counterexamples found: `ApStartupData.entry` is 0 or uninitialized, `ApStartupData.pml4_phys` is 0 or uninitialized
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

- [ ] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - Existing Field Initialization
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code: `prepare_ap_startup_data(stack_top, cpu_data)` correctly sets `ApStartupData.stack_top` to `stack_top` and `ApStartupData.cpu_data` to `cpu_data`
  - Write property-based tests capturing observed behavior: for all valid (stack_top, cpu_data) pairs, verify `data.stack_top == stack_top` AND `data.cpu_data == cpu_data`
  - Property-based testing generates many test cases for stronger guarantees
  - Test various stack_top values (aligned, unaligned, high addresses)
  - Test various cpu_data pointer values
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.2, 3.3, 3.5_

- [ ] 3. Fix for AP Entry Null Pointer Bug

  - [x] 3.1 Implement the fix in `prepare_ap_startup_data()`
    - Import `Cr3` from `x86_64::registers::control` if not already imported
    - Add PML4 physical address retrieval logic (same as in `load_ap_startup_code()`)
    - Set `data.pml4_phys` to the kernel's PML4 physical address
    - Set `data.entry` to the physical address of `ap_entry` function: `crate::cpu::ap::ap_entry as *const () as u64`
    - Maintain existing `stack_top` and `cpu_data` initialization
    - Keep the existing `compiler_fence(Ordering::SeqCst)` for memory visibility
    - _Bug_Condition: isBugCondition(input) where `data.entry == 0` AND `data.pml4_phys == 0` after `prepare_ap_startup_data(input)`_
    - _Expected_Behavior: `data.entry == address_of(ap_entry)` AND `data.pml4_phys != 0` AND `data.pml4_phys == valid_kernel_pml4_address`_
    - _Preservation: `data.stack_top == stack_top` AND `data.cpu_data == cpu_data` for all inputs_
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 3.2, 3.3, 3.5_

  - [ ] 3.2 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Entry and PML4 Pointer Initialization
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms the expected behavior is satisfied
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - Verify `ApStartupData.entry` points to `ap_entry` function
    - Verify `ApStartupData.pml4_phys` contains valid non-zero physical address
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ] 3.3 Verify preservation tests still pass
    - **Property 2: Preservation** - Existing Field Initialization
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm `stack_top` and `cpu_data` fields are still set correctly
    - Confirm all tests still pass after fix (no regressions)
    - _Requirements: 3.2, 3.3, 3.5_

- [ ] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
