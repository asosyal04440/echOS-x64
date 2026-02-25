# Implementation Plan

- [x] 1. Write bug condition exploration test
  - **Property 1: Fault Condition** - Heap Stack Physical Address Translation
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the bug exists
  - **Scoped PBT Approach**: Scope the property to concrete failing cases - heap-allocated stacks with virtual addresses < PHYSICAL_MEMORY_OFFSET
  - Test that `phys_addr()` called on heap-allocated stacks (virt_addr < PHYSICAL_MEMORY_OFFSET) does not panic and returns valid physical addresses
  - Create test cases:
    - Heap-allocated byte array wrapped in mock KernelStack-like structure
    - Syscall stack simulation via `Box::new([0u8; SYSCALL_STACK_SIZE])`
    - Low address test with virt_addr known to be < PHYSICAL_MEMORY_OFFSET
    - Boundary test with virt_addr = 0xFFFF_7FFF_FFFF_FFFF (just below HHDM threshold)
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS with "attempt to subtract with overflow" panic (this is correct - it proves the bug exists)
  - Document counterexamples found to understand root cause
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 2.1, 2.2, 2.3_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - HHDM Stack Direct Translation
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for HHDM-mapped stacks (virt_addr >= PHYSICAL_MEMORY_OFFSET)
  - Observe: `KernelStack::new()` allocations work correctly on unfixed code
  - Observe: For HHDM addresses, calculation is `virt_addr - PHYSICAL_MEMORY_OFFSET`
  - Write property-based tests capturing observed behavior patterns:
    - For all HHDM virtual addresses (>= PHYSICAL_MEMORY_OFFSET), `phys_addr()` returns `virt_addr - PHYSICAL_MEMORY_OFFSET`
    - HHDM stack allocation via `KernelStack::new()` continues to work identically
    - Drop behavior for HHDM stacks continues to work correctly
    - Clone behavior for HHDM stacks continues to work correctly
  - Property-based testing generates many test cases for stronger guarantees
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3_

- [x] 3. Fix for heap stack physical address translation

  - [x] 3.1 Implement the fix in `src/allocator/stack.rs`
    - Add address space detection: check if `virt_addr >= PHYSICAL_MEMORY_OFFSET`
    - Implement conditional translation logic:
      - If HHDM-mapped: use existing direct calculation `virt_addr - PHYSICAL_MEMORY_OFFSET`
      - If heap-allocated: use `crate::memory::paging::translate_addr()` for page table translation
    - Import required module: add `use crate::memory::paging;` if needed
    - Handle translation failure: panic with descriptive error if `translate_addr()` returns `None`
    - Preserve performance: HHDM path remains O(1) subtraction
    - _Bug_Condition: isBugCondition(stack) where virt_addr < PHYSICAL_MEMORY_OFFSET AND phys_addr() is called_
    - _Expected_Behavior: For heap stacks, use page table translation via translate_addr() to return valid physical address without panicking_
    - _Preservation: HHDM stacks (virt_addr >= PHYSICAL_MEMORY_OFFSET) continue to use direct offset calculation with identical results_
    - _Requirements: 2.1, 2.2, 2.3, 3.1, 3.2, 3.3_

  - [x] 3.2 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Heap Stack Physical Address Translation
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms the expected behavior is satisfied
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bug is fixed)
    - Verify that heap-allocated stacks no longer cause integer underflow panic
    - Verify that `phys_addr()` returns valid physical addresses for heap stacks
    - _Requirements: 2.1, 2.2, 2.3_

  - [x] 3.3 Verify preservation tests still pass
    - **Property 2: Preservation** - HHDM Stack Direct Translation
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm all tests still pass after fix (no regressions)
    - Verify HHDM stacks produce identical results to unfixed code
    - Verify performance characteristics remain unchanged for HHDM path

- [x] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
  - Verify integration: test full SMP initialization flow with heap-allocated syscall stacks
  - Verify AP startup succeeds with the fix
  - Verify mixed usage: some stacks from PMM (HHDM), some from heap, all work correctly
