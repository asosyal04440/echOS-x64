# Implementation Plan

- [x] 1. Write bug condition exploration test
  - **Property 1: Fault Condition** - AP Entry Point Execution Hang
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate APs hang when calling kernel virtual address entry point
  - **Scoped PBT Approach**: Scope the property to concrete failing cases - boot with 2-4 CPUs and verify APs complete assembly (ABCDEFG printed) but hang at entry point call (no 'A' on COM1)
  - Test that for AP startup where cpu_id != 0 AND execution_stage == "long_mode_entry_call" AND entry_address >= 0x7000000000000000, the AP hangs without executing ap_entry
  - Verify assembly debug output (ABCDEFG) appears on debugcon port 0xE9
  - Verify NO 'A' character appears on COM1 port 0x3f8 (indicates ap_entry never executes)
  - Verify system hangs without reaching wait_for_online timeout
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS (APs hang at entry point call - this is correct and proves the bug exists)
  - Document counterexamples found: which APs hang, at what address, what debug output appears
  - Add debug output in prepare_ap_startup_data to print entry point address and verify it's in kernel virtual range (0x7xxxxxxx)
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - BSP and Assembly Startup Behavior
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for non-buggy inputs (BSP boot, assembly startup sequence)
  - Verify BSP (CPU 0) boots successfully with kernel virtual addresses working correctly
  - Verify AP assembly startup displays 'ABCDEFG' characters to debugcon port 0xE9
  - Verify prepare_ap_startup_data() correctly populates all ApStartupData fields (pml4_phys, entry, stack_top, cpu_data)
  - Verify kernel virtual address usage functions correctly for existing kernel code
  - Write property-based tests capturing observed behavior patterns: BSP boot succeeds, assembly output appears, ApStartupData is valid
  - Property-based testing generates many test cases for stronger guarantees (different CPU counts, memory layouts)
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 3. Fix for AP entry point mapping issue

  - [x] 3.1 Implement the fix in prepare_ap_startup_data
    - Convert entry point from kernel virtual address to physical address
    - Calculate physical address: physical_entry = virtual_entry - KERNEL_VIRTUAL_BASE + KERNEL_PHYSICAL_BASE
    - Store physical address in data.entry field instead of virtual address
    - Add validation to ensure address is accessible (either identity-mapped or in known-mapped region)
    - Add debug output to confirm address translation (print both virtual and physical addresses)
    - Document why physical address is needed: APs cannot use arbitrary kernel virtual addresses immediately after enabling paging
    - Alternative approach if needed: Add identity mapping for entry point physical page to kernel PML4
    - _Bug_Condition: isBugCondition(input) where input.cpu_id != 0 AND input.execution_stage == "long_mode_entry_call" AND input.entry_address >= 0x7000000000000000 AND NOT isAddressMappedAndExecutable(input.entry_address, input.page_tables)_
    - _Expected_Behavior: AP successfully executes ap_entry function, prints 'A' to COM1, continues with full initialization without hanging_
    - _Preservation: BSP boot sequence, AP assembly debug output (ABCDEFG), prepare_ap_startup_data field population, kernel virtual address usage, page table setup_
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [x] 3.2 Add TLB flush verification in ap_startup.asm if needed
    - Verify that mov cr3, eax properly flushes TLB after loading kernel PML4
    - Add debug output immediately before call rax to confirm address is loaded correctly
    - This helps diagnose if address loading is working as expected
    - _Requirements: 2.2, 2.3_

  - [x] 3.3 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - AP Entry Point Execution Success
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms APs successfully execute ap_entry
    - Run bug condition exploration test from step 1
    - Verify assembly debug output (ABCDEFG) still appears on debugcon
    - Verify 'A' character NOW appears on COM1 (indicates ap_entry executes)
    - Verify APs complete initialization and mark themselves online
    - Verify wait_for_online() completes without timeout
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [x] 3.4 Verify preservation tests still pass
    - **Property 2: Preservation** - BSP and Assembly Startup Behavior
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - Verify BSP boot continues to work exactly as before
    - Verify AP assembly startup still displays 'ABCDEFG' correctly
    - Verify prepare_ap_startup_data() still populates all fields correctly
    - Verify kernel virtual address usage still functions correctly
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm all tests still pass after fix (no regressions in BSP boot or assembly startup)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [ ] 4. Checkpoint - Ensure all tests pass
  - Verify bug condition exploration test passes (APs successfully execute ap_entry)
  - Verify preservation tests pass (BSP boot and assembly startup unchanged)
  - Verify full SMP initialization completes with all APs online
  - Verify system stability after all APs are online (scheduler runs, interrupts handled)
  - Ensure all tests pass, ask the user if questions arise
