# Preservation Property Tests

**Property 2: HHDM Stack Direct Translation Preservation**  
**Requirements: 3.1, 3.2, 3.3**

## Purpose

This document captures the CURRENT CORRECT BEHAVIOR for HHDM-mapped stacks that MUST be preserved after implementing the fix for heap stack physical address translation. These properties serve as the baseline that the fix must not break.

## Test Methodology

Following the observation-first methodology, these properties were observed on the UNFIXED code for HHDM-mapped stacks (virtual addresses >= PHYSICAL_MEMORY_OFFSET = 0xFFFF_8000_0000_0000).

## Preservation Properties

### Property 1: HHDM Direct Translation Formula

**Observation**: For all HHDM virtual addresses (>= PHYSICAL_MEMORY_OFFSET), `phys_addr()` returns `virt_addr - PHYSICAL_MEMORY_OFFSET`

**Test Cases**:
- Address 0xFFFF_8000_0000_0000 (exactly at PHYSICAL_MEMORY_OFFSET)
  - Is HHDM: ✓ (>= PHYSICAL_MEMORY_OFFSET)
  - Expected physical address: 0x0000_0000_0000_0000
  - Formula: virt_addr - PHYSICAL_MEMORY_OFFSET
  - **PRESERVATION**: This calculation MUST remain unchanged

- Address 0xFFFF_8000_0010_0000 (typical HHDM address)
  - Is HHDM: ✓ (>= PHYSICAL_MEMORY_OFFSET)
  - Expected physical address: 0x0000_0000_0010_0000
  - Formula: virt_addr - PHYSICAL_MEMORY_OFFSET
  - **PRESERVATION**: This calculation MUST remain unchanged

- Address 0xFFFF_8000_0100_0000 (another HHDM address)
  - Is HHDM: ✓ (>= PHYSICAL_MEMORY_OFFSET)
  - Expected physical address: 0x0000_0000_0100_0000
  - Formula: virt_addr - PHYSICAL_MEMORY_OFFSET
  - **PRESERVATION**: This calculation MUST remain unchanged

- Address 0xFFFF_8000_1000_0000 (higher HHDM address)
  - Is HHDM: ✓ (>= PHYSICAL_MEMORY_OFFSET)
  - Expected physical address: 0x0000_0000_1000_0000
  - Formula: virt_addr - PHYSICAL_MEMORY_OFFSET
  - **PRESERVATION**: This calculation MUST remain unchanged

- Address 0xFFFF_FFFF_FFFF_F000 (near top of address space)
  - Is HHDM: ✓ (>= PHYSICAL_MEMORY_OFFSET)
  - Expected physical address: 0x0000_7FFF_FFFF_F000
  - Formula: virt_addr - PHYSICAL_MEMORY_OFFSET
  - **PRESERVATION**: This calculation MUST remain unchanged

**Validates**: Requirements 3.1, 3.2

---

### Property 2: HHDM Stack Allocation via KernelStack::new()

**Observation**: Stack allocation from PMM with HHDM mapping continues to work identically

**Current Behavior**:
- `KernelStack::new(size)` allocates contiguous physical frames from PMM
- Physical address is mapped to HHDM: `virt_addr = phys_addr + PHYSICAL_MEMORY_OFFSET`
- Virtual address is always >= PHYSICAL_MEMORY_OFFSET
- Memory is zeroed for security and determinism
- Each allocation gets a unique address
- Stack size is rounded up to page boundaries (4096 bytes)

**PRESERVATION**: This behavior MUST remain unchanged after the fix

**Validates**: Requirement 3.3

---

### Property 3: Drop Behavior for HHDM Stacks

**Observation**: HHDM stacks are correctly deallocated when dropped

**Current Behavior**:
- `Drop` implementation calls `phys_addr()` to get the physical address
- Physical address is used to deallocate PMM frames via `deallocate_contiguous_frames()`
- For HHDM stacks: `phys_addr = virt_addr - PHYSICAL_MEMORY_OFFSET`
- The calculation is correct and frames are properly returned to PMM
- No memory leaks occur

**PRESERVATION**: This behavior MUST remain unchanged after the fix

**Validates**: Requirement 3.3

---

### Property 4: Clone Behavior for HHDM Stacks

**Observation**: HHDM stacks can be cloned correctly with deep copy semantics

**Current Behavior**:
- `Clone` implementation allocates a new HHDM stack via `KernelStack::new()`
- Content is deep-copied from original to clone using `ptr::copy_nonoverlapping()`
- Clone has different virtual and physical addresses than the original
- Both original and clone use HHDM direct translation for `phys_addr()`
- Clone is independent - dropping one doesn't affect the other
- Content is identical byte-for-byte after cloning

**PRESERVATION**: This behavior MUST remain unchanged after the fix

**Validates**: Requirement 3.3

---

### Property 5: Performance Characteristics

**Observation**: HHDM address translation is O(1) direct calculation with no page table lookup

**Current Behavior**:
- HHDM translation is a simple subtraction: `virt_addr - PHYSICAL_MEMORY_OFFSET`
- No page table lookup required
- O(1) time complexity
- No memory accesses beyond the calculation itself
- Extremely fast - just arithmetic on the CPU

**PRESERVATION**: Performance characteristics MUST remain unchanged after the fix

**Implementation Note**: The fix should only add page table lookup for heap addresses (virt_addr < PHYSICAL_MEMORY_OFFSET). HHDM addresses must continue to use the fast direct calculation path.

**Validates**: Requirement 3.2

---

### Property 6: Boundary Behavior at PHYSICAL_MEMORY_OFFSET

**Observation**: The fix must correctly distinguish HHDM from heap addresses at the boundary

**Boundary Condition**:
- `virt_addr < PHYSICAL_MEMORY_OFFSET` (0xFFFF_8000_0000_0000) => Heap address (use page table translation)
- `virt_addr >= PHYSICAL_MEMORY_OFFSET` (0xFFFF_8000_0000_0000) => HHDM address (use direct calculation)

**Critical Implementation Detail**:
The boundary check MUST be: `virt_addr >= PHYSICAL_MEMORY_OFFSET`

This ensures:
- All HHDM addresses use the preserved direct translation
- All heap addresses use the new page table translation
- No ambiguity at the boundary
- Addresses exactly at PHYSICAL_MEMORY_OFFSET are treated as HHDM (correct behavior)

**PRESERVATION**: The boundary must be precisely at PHYSICAL_MEMORY_OFFSET with >= comparison

**Validates**: Requirements 3.1, 3.2

---

## Preservation Summary

All HHDM-mapped stacks (virt_addr >= 0xFFFF_8000_0000_0000) must continue to use the direct translation formula:

```
phys_addr = virt_addr - PHYSICAL_MEMORY_OFFSET
```

The fix should ONLY affect heap-allocated stacks (virt_addr < 0xFFFF_8000_0000_0000) by adding page table translation for those addresses.

**HHDM behavior must remain completely unchanged!**

## Implementation Guidance

When implementing the fix in `src/allocator/stack.rs`, the `phys_addr()` method should:

1. Check if `virt_addr >= PHYSICAL_MEMORY_OFFSET`
2. If true (HHDM): Use existing direct calculation `virt_addr - PHYSICAL_MEMORY_OFFSET`
3. If false (heap): Use new page table translation via `crate::memory::paging::translate_addr()`

This ensures all preservation properties are maintained while fixing the bug for heap-allocated stacks.

## Verification

After implementing the fix:
1. All HHDM stack operations must continue to work identically
2. Performance of HHDM translation must remain O(1)
3. Drop, Clone, and other operations must produce identical results
4. No regressions in existing functionality

These preservation properties serve as acceptance criteria for the fix implementation.
