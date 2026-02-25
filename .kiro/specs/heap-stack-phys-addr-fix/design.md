# Heap Stack Physical Address Fix Design

## Overview

The `KernelStack::phys_addr()` method currently assumes all kernel stacks are HHDM-mapped (Higher Half Direct Mapping) and calculates physical addresses by subtracting `PHYSICAL_MEMORY_OFFSET` from virtual addresses. This causes integer underflow panics when called on heap-allocated stacks (e.g., syscall stacks allocated via `Box::new([0u8; SYSCALL_STACK_SIZE])`) because heap virtual addresses are in the low half of memory (< `PHYSICAL_MEMORY_OFFSET`).

The fix will detect whether a stack is HHDM-mapped or heap-allocated by comparing the virtual address against `PHYSICAL_MEMORY_OFFSET`, then use the appropriate translation method: direct offset subtraction for HHDM stacks, or page table translation for heap stacks.

## Glossary

- **Bug_Condition (C)**: The condition that triggers the bug - when `phys_addr()` is called on a heap-allocated stack (virtual address < PHYSICAL_MEMORY_OFFSET)
- **Property (P)**: The desired behavior - correctly translate heap virtual addresses to physical addresses using page table translation
- **Preservation**: Existing HHDM stack translation behavior that must remain unchanged
- **HHDM (Higher Half Direct Mapping)**: Memory region starting at `PHYSICAL_MEMORY_OFFSET` (0xFFFF_8000_0000_0000) where physical memory is directly mapped with a constant offset
- **PHYSICAL_MEMORY_OFFSET**: The constant offset (0xFFFF_8000_0000_0000) used for HHDM translation
- **KernelStack**: The struct in `src/allocator/stack.rs` that manages kernel stack allocations
- **phys_addr()**: The method that returns the physical address of a kernel stack
- **translate_addr()**: The function in `src/memory/paging.rs` that walks page tables to translate virtual addresses to physical addresses

## Bug Details

### Fault Condition

The bug manifests when `KernelStack::phys_addr()` is called on a heap-allocated stack. The method unconditionally attempts to calculate the physical address as `virt_addr - PHYSICAL_MEMORY_OFFSET`, which causes integer underflow when the virtual address is less than `PHYSICAL_MEMORY_OFFSET`.

**Formal Specification:**
```
FUNCTION isBugCondition(stack)
  INPUT: stack of type KernelStack
  OUTPUT: boolean
  
  LET virt_addr = stack.ptr.as_ptr() as u64
  
  RETURN virt_addr < PHYSICAL_MEMORY_OFFSET
         AND phys_addr() is called on stack
END FUNCTION
```

### Examples

- **Heap-allocated syscall stack**: Virtual address 0x444444478a90 < 0xFFFF_8000_0000_0000 → Attempting `0x444444478a90 - 0xFFFF_8000_0000_0000` causes underflow panic
- **HHDM-mapped stack from PMM**: Virtual address 0xFFFF_8000_0010_0000 ≥ 0xFFFF_8000_0000_0000 → Calculation `0xFFFF_8000_0010_0000 - 0xFFFF_8000_0000_0000 = 0x0010_0000` works correctly
- **Heap-allocated stack at 0x1000000**: Virtual address 0x1000000 < 0xFFFF_8000_0000_0000 → Causes underflow panic
- **Edge case - address just below HHDM**: Virtual address 0xFFFF_7FFF_FFFF_FFFF < 0xFFFF_8000_0000_0000 → Would cause underflow if such a stack existed

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- HHDM-mapped stacks (allocated via `KernelStack::new()` from PMM) must continue to use direct offset calculation
- The physical address calculation for HHDM stacks must remain `virt_addr - PHYSICAL_MEMORY_OFFSET`
- All existing stack operations (allocation, deallocation, cloning, dereferencing) must continue to work identically
- The `Drop` implementation must continue to correctly deallocate PMM frames for HHDM stacks

**Scope:**
All kernel stacks with virtual addresses ≥ `PHYSICAL_MEMORY_OFFSET` should be completely unaffected by this fix. This includes:
- Stacks allocated via `KernelStack::new()` which uses PMM and HHDM mapping
- Any other HHDM-mapped memory regions that might use similar address translation logic
- The performance characteristics of HHDM address translation (should remain O(1) subtraction)

## Hypothesized Root Cause

Based on the bug description and code analysis, the root cause is:

1. **Assumption Violation**: The `phys_addr()` method assumes all `KernelStack` instances are allocated via `KernelStack::new()`, which allocates from PMM and maps to HHDM. However, syscall stacks are allocated via `Box::new([0u8; SYSCALL_STACK_SIZE])` which uses the heap allocator.

2. **Heap vs HHDM Address Spaces**: Heap allocations have virtual addresses in the low half of memory (< 0xFFFF_8000_0000_0000), while HHDM mappings are in the high half (≥ 0xFFFF_8000_0000_0000). The method doesn't distinguish between these two address spaces.

3. **Unchecked Arithmetic**: The subtraction `virt_addr - PHYSICAL_MEMORY_OFFSET` is performed without checking whether `virt_addr ≥ PHYSICAL_MEMORY_OFFSET`, causing integer underflow in debug builds (panic) or wraparound in release builds (incorrect physical address).

4. **Missing Translation Path**: The codebase has `translate_addr()` in `src/memory/paging.rs` for page table translation, but `phys_addr()` doesn't use it for heap-allocated stacks.

## Correctness Properties

Property 1: Fault Condition - Heap Stack Physical Address Translation

_For any_ kernel stack where the virtual address is less than PHYSICAL_MEMORY_OFFSET (heap-allocated), the fixed `phys_addr()` method SHALL use page table translation via `translate_addr()` to correctly return the physical address without panicking.

**Validates: Requirements 2.1, 2.2, 2.3**

Property 2: Preservation - HHDM Stack Direct Translation

_For any_ kernel stack where the virtual address is greater than or equal to PHYSICAL_MEMORY_OFFSET (HHDM-mapped), the fixed `phys_addr()` method SHALL produce exactly the same result as the original method, preserving the direct offset calculation `virt_addr - PHYSICAL_MEMORY_OFFSET`.

**Validates: Requirements 3.1, 3.2, 3.3**

## Fix Implementation

### Changes Required

Assuming our root cause analysis is correct:

**File**: `src/allocator/stack.rs`

**Function**: `KernelStack::phys_addr()`

**Specific Changes**:
1. **Add Address Space Detection**: Check if `virt_addr >= PHYSICAL_MEMORY_OFFSET` to determine if the stack is HHDM-mapped or heap-allocated

2. **Conditional Translation Logic**:
   - If HHDM-mapped: Use existing direct calculation `virt_addr - PHYSICAL_MEMORY_OFFSET`
   - If heap-allocated: Use `crate::memory::paging::translate_addr()` for page table translation

3. **Import Required Module**: Add `use crate::memory::paging;` to access the `translate_addr()` function

4. **Handle Translation Failure**: If `translate_addr()` returns `None` for a heap address, panic with a descriptive error message (this indicates an unmapped address, which is a critical error)

5. **Preserve Performance**: The HHDM path remains a simple O(1) subtraction, while the heap path uses O(1) page table lookup (4 memory accesses for 4-level paging)

### Implementation Pseudocode

```rust
pub fn phys_addr(&self) -> PhysAddr {
    let virt_addr = self.ptr.as_ptr() as u64;
    
    if virt_addr >= PHYSICAL_MEMORY_OFFSET {
        // HHDM-mapped stack: use direct offset calculation
        PhysAddr::new(virt_addr - PHYSICAL_MEMORY_OFFSET)
    } else {
        // Heap-allocated stack: use page table translation
        use x86_64::VirtAddr;
        crate::memory::paging::translate_addr(VirtAddr::new(virt_addr))
            .expect("KernelStack virtual address is not mapped")
    }
}
```

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code, then verify the fix works correctly and preserves existing behavior.

### Exploratory Fault Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm or refute the root cause analysis. If we refute, we will need to re-hypothesize.

**Test Plan**: Create test cases that allocate heap-based stacks (simulating syscall stack allocation) and call `phys_addr()` on them. Run these tests on the UNFIXED code to observe the integer underflow panic and confirm the root cause.

**Test Cases**:
1. **Heap Stack Physical Address Test**: Create a heap-allocated byte array, wrap it in a mock KernelStack-like structure, and call `phys_addr()` (will panic on unfixed code with "attempt to subtract with overflow")
2. **Syscall Stack Simulation Test**: Allocate a stack via `Box::new([0u8; SYSCALL_STACK_SIZE])` and attempt to get its physical address (will panic on unfixed code)
3. **Low Address Test**: Create a stack with a virtual address known to be < PHYSICAL_MEMORY_OFFSET and verify it causes underflow (will panic on unfixed code)
4. **Boundary Test**: Test with virtual address 0xFFFF_7FFF_FFFF_FFFF (just below HHDM threshold) to verify underflow detection (will panic on unfixed code)

**Expected Counterexamples**:
- Panic with "attempt to subtract with overflow" when `phys_addr()` is called on heap-allocated stacks
- Possible causes: unconditional subtraction without address space detection, missing page table translation path

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds, the fixed function produces the expected behavior.

**Pseudocode:**
```
FOR ALL stack WHERE isBugCondition(stack) DO
  result := phys_addr_fixed(stack)
  ASSERT result is valid PhysAddr
  ASSERT result does not panic
  ASSERT result matches page table translation
END FOR
```

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold, the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL stack WHERE NOT isBugCondition(stack) DO
  ASSERT phys_addr_original(stack) = phys_addr_fixed(stack)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many test cases automatically across the input domain
- It catches edge cases that manual unit tests might miss
- It provides strong guarantees that behavior is unchanged for all HHDM-mapped stacks

**Test Plan**: Observe behavior on UNFIXED code first for HHDM-mapped stacks allocated via `KernelStack::new()`, then write property-based tests capturing that behavior.

**Test Cases**:
1. **HHDM Stack Preservation**: Observe that `KernelStack::new()` allocations work correctly on unfixed code, then write test to verify `phys_addr()` produces identical results after fix
2. **Direct Calculation Preservation**: Verify that for HHDM addresses, the calculation remains `virt_addr - PHYSICAL_MEMORY_OFFSET` with no page table lookup overhead
3. **Drop Behavior Preservation**: Verify that stack deallocation continues to work correctly for HHDM stacks after the fix
4. **Clone Behavior Preservation**: Verify that stack cloning continues to work correctly after the fix

### Unit Tests

- Test `phys_addr()` on heap-allocated stacks with various virtual addresses < PHYSICAL_MEMORY_OFFSET
- Test `phys_addr()` on HHDM-mapped stacks with various virtual addresses ≥ PHYSICAL_MEMORY_OFFSET
- Test boundary condition: virtual address exactly at PHYSICAL_MEMORY_OFFSET
- Test that heap stack translation matches `translate_addr()` results
- Test that HHDM stack translation matches direct offset calculation

### Property-Based Tests

- Generate random heap virtual addresses (< PHYSICAL_MEMORY_OFFSET) and verify `phys_addr()` doesn't panic and returns valid physical addresses
- Generate random HHDM virtual addresses (≥ PHYSICAL_MEMORY_OFFSET) and verify `phys_addr()` produces identical results to direct offset calculation
- Test across many stack sizes to ensure the fix works regardless of stack size
- Test that all HHDM stacks continue to use O(1) direct calculation (no page table lookup)

### Integration Tests

- Test full SMP initialization flow with heap-allocated syscall stacks
- Test that AP (Application Processor) startup succeeds with the fix
- Test mixed usage: some stacks from PMM (HHDM), some from heap, verify all work correctly
- Test that the system boots successfully with multiple CPUs after the fix
