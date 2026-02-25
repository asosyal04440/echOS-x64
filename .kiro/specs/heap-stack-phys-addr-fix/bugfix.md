# Bugfix Requirements Document

## Introduction

The system panics during SMP initialization with "attempt to subtract with overflow" in the `KernelStack::phys_addr()` method. This occurs because the method assumes all kernel stacks are HHDM-mapped (Higher Half Direct Mapping) and attempts to calculate physical addresses by subtracting `PHYSICAL_MEMORY_OFFSET` from virtual addresses. However, syscall stacks allocated via `Box::new([0u8; SYSCALL_STACK_SIZE])` are heap-allocated with virtual addresses in the low half of memory (e.g., 0x444444478a90), causing integer underflow when the subtraction is attempted.

This bug completely blocks SMP initialization, preventing the system from booting with multiple CPUs and blocking AP (Application Processor) startup.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN `KernelStack::phys_addr()` is called on a heap-allocated stack (virtual address < PHYSICAL_MEMORY_OFFSET) THEN the system panics with "attempt to subtract with overflow"

1.2 WHEN syscall stacks are allocated via `Box::new([0u8; SYSCALL_STACK_SIZE])` during SMP initialization THEN their virtual addresses are in the low half of memory (e.g., 0x444444478a90)

1.3 WHEN the method attempts to calculate physical address as `virt_addr - PHYSICAL_MEMORY_OFFSET` for heap addresses THEN integer underflow occurs because heap addresses are smaller than PHYSICAL_MEMORY_OFFSET

### Expected Behavior (Correct)

2.1 WHEN `KernelStack::phys_addr()` is called on a heap-allocated stack THEN the system SHALL correctly translate the heap virtual address to its physical address without panicking

2.2 WHEN `KernelStack::phys_addr()` is called on a heap-allocated stack THEN the system SHALL use the page table translation mechanism to obtain the physical address

2.3 WHEN syscall stacks are allocated via `Box::new([0u8; SYSCALL_STACK_SIZE])` during SMP initialization THEN the system SHALL successfully retrieve their physical addresses and continue SMP initialization

### Unchanged Behavior (Regression Prevention)

3.1 WHEN `KernelStack::phys_addr()` is called on an HHDM-mapped stack (virtual address >= PHYSICAL_MEMORY_OFFSET) THEN the system SHALL CONTINUE TO calculate the physical address as `virt_addr - PHYSICAL_MEMORY_OFFSET`

3.2 WHEN `KernelStack::phys_addr()` is called on an HHDM-mapped stack THEN the system SHALL CONTINUE TO return the correct physical address using the direct mapping calculation

3.3 WHEN the system uses kernel stacks for normal operations (non-SMP initialization) THEN the system SHALL CONTINUE TO function correctly with existing stack allocation patterns
