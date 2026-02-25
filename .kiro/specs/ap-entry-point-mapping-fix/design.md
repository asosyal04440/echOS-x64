# AP Entry Point Mapping Fix - Bugfix Design

## Overview

This bugfix addresses a critical issue where Application Processors (APs) successfully complete assembly startup but hang when attempting to call the Rust entry point function `ap_entry`. The root cause is that the assembly code attempts to call a kernel virtual address (0x7d5264f0) that is not properly accessible from the AP's execution context immediately after enabling paging. The fix will ensure the entry point address is either identity-mapped or converted to a physical address that can be safely called during the AP startup transition, while preserving all existing BSP and assembly startup behavior.

## Glossary

- **Bug_Condition (C)**: The condition that triggers the bug - when an AP attempts to call the Rust entry point at a kernel virtual address after enabling paging but before proper virtual memory context is established
- **Property (P)**: The desired behavior when the bug condition occurs - the AP should successfully execute the ap_entry function without hanging
- **Preservation**: Existing BSP boot behavior, assembly startup sequence, and debug output that must remain unchanged by the fix
- **ap_entry**: The Rust function in `src/cpu/ap.rs` that serves as the entry point for APs after assembly startup
- **prepare_ap_startup_data**: The function in `src/cpu/smp.rs` that sets up the ApStartupData structure with the entry point address
- **ap_startup.asm**: The assembly code in `src/cpu/ap_startup.asm` that transitions APs from real mode through protected mode to long mode
- **ApStartupData**: The data structure at physical address 0x1000 containing pml4_phys, entry point address, stack_top, and cpu_data pointer
- **Kernel Virtual Address**: Higher-half virtual addresses (typically 0x7xxxxxxx range) that require proper page table mapping to access

## Bug Details

### Fault Condition

The bug manifests when an AP completes the assembly startup sequence in `ap_startup.asm`, enables paging with the kernel PML4 page tables, and attempts to call the Rust entry point at a kernel virtual address. The `call rax` instruction at the end of the long_mode section tries to jump to address 0x7d5264f0, but this virtual address is not properly mapped or accessible in the AP's current execution context, causing an instruction fetch failure.

**Formal Specification:**
```
FUNCTION isBugCondition(input)
  INPUT: input of type ApStartupContext
  OUTPUT: boolean
  
  RETURN input.cpu_id != 0
         AND input.execution_stage == "long_mode_entry_call"
         AND input.entry_address >= 0x7000000000000000
         AND NOT isAddressMappedAndExecutable(input.entry_address, input.page_tables)
END FUNCTION
```

### Examples

- **AP 1 startup**: Assembly completes successfully (ABCDEFG printed to debugcon), paging enabled with kernel PML4, `call rax` with rax=0x7d5264f0 causes hang, no 'A' character printed to COM1
- **AP 2 startup**: Same sequence - assembly succeeds, long mode entered, call to 0x7d5264f0 hangs, system never reaches wait_for_online timeout
- **AP 3 startup**: Identical behavior - all assembly debug output appears, but Rust entry point never executes
- **BSP (CPU 0)**: Boots successfully with kernel virtual addresses working correctly - this behavior must be preserved

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- BSP (CPU 0) boot sequence must continue to work exactly as before with all existing functionality
- AP assembly startup code must continue to display 'ABCDEFG' characters to debugcon port 0xE9
- prepare_ap_startup_data() must continue to correctly populate all ApStartupData fields
- Kernel virtual address usage must continue to function correctly for all existing kernel code
- Page table setup and kernel PML4 usage must remain unchanged for normal kernel operation

**Scope:**
All inputs that do NOT involve AP startup (cpu_id != 0 during assembly-to-Rust transition) should be completely unaffected by this fix. This includes:
- BSP initialization and boot process
- Normal kernel code execution with virtual addresses
- Existing page table mappings and memory management
- All other SMP initialization steps that occur after APs successfully enter Rust code

## Hypothesized Root Cause

Based on the bug description and code analysis, the most likely issues are:

1. **Virtual Address Not Identity-Mapped**: The kernel virtual address 0x7d5264f0 for `ap_entry` is not identity-mapped in the kernel PML4 page tables
   - The AP loads the kernel PML4 into CR3 during assembly startup
   - The kernel PML4 maps the higher-half kernel region but may not have identity mappings for the physical location of ap_entry
   - When the AP tries to fetch instructions from 0x7d5264f0, the MMU cannot translate it to a physical address

2. **Physical Address Calculation Missing**: The code stores the virtual address directly without converting to physical
   - `prepare_ap_startup_data` stores `ap_entry as *const () as u64` which gives the virtual address
   - The assembly code loads this address into rax and calls it directly
   - No conversion from virtual to physical address occurs before the call

3. **Page Table Context Mismatch**: The AP's execution context immediately after enabling paging may not have the same virtual memory view as the BSP
   - Even though the same PML4 is loaded, the AP's instruction pointer and execution state differ
   - The virtual address may require additional setup (GDT, segment registers, etc.) to be accessible

4. **TLB Not Flushed**: The Translation Lookaside Buffer may contain stale entries from the identity-mapped startup region
   - After loading CR3 with the kernel PML4, the TLB may not be properly flushed
   - Instruction fetches may use stale TLB entries that don't map the entry point address

## Correctness Properties

Property 1: Fault Condition - AP Entry Point Execution

_For any_ AP startup where the CPU completes assembly initialization and attempts to call the Rust entry point, the fixed code SHALL successfully execute the ap_entry function, printing debug character 'A' to COM1 port 0x3f8 and continuing with full AP initialization without hanging.

**Validates: Requirements 2.1, 2.2, 2.3, 2.4**

Property 2: Preservation - BSP and Assembly Startup Behavior

_For any_ system boot or AP startup sequence that does NOT involve the assembly-to-Rust transition call instruction, the fixed code SHALL produce exactly the same behavior as the original code, preserving BSP boot functionality, assembly debug output (ABCDEFG), and all existing kernel virtual address usage.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5**

## Fix Implementation

### Changes Required

Assuming our root cause analysis is correct (virtual address not accessible during AP startup):

**File**: `src/cpu/smp.rs`

**Function**: `prepare_ap_startup_data`

**Specific Changes**:
1. **Convert Entry Point to Physical Address**: Instead of storing the virtual address of `ap_entry`, convert it to a physical address that can be safely called
   - Use the kernel's virtual-to-physical address translation mechanism
   - Calculate: `physical_entry = virtual_entry - KERNEL_VIRTUAL_BASE + KERNEL_PHYSICAL_BASE`
   - Store the physical address in `data.entry`

2. **Alternative: Identity Map Entry Point**: If physical address conversion is not feasible, ensure the entry point is identity-mapped
   - Add identity mapping for the physical page containing `ap_entry` to the kernel PML4
   - This allows the virtual address to be called directly after paging is enabled

3. **Verify Address Accessibility**: Add validation to ensure the entry point address is accessible
   - Check that the address is either identity-mapped or in a known-mapped region
   - Add debug output to confirm the address translation

4. **Document Address Requirements**: Add comments explaining why physical address or identity mapping is needed
   - Clarify that APs cannot use arbitrary kernel virtual addresses immediately after enabling paging
   - Document the address translation mechanism used

**File**: `src/cpu/ap_startup.asm`

**Section**: `long_mode`

**Specific Changes** (if needed):
1. **Add TLB Flush**: Ensure TLB is flushed after loading CR3 if not already done
   - The `mov cr3, eax` instruction should flush TLB automatically, but verify this is working

2. **Verify Address Before Call**: Add debug output immediately before the call to confirm rax contains the expected address
   - This helps diagnose if the address is being loaded correctly

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code by attempting to boot APs and observing the hang, then verify the fix works correctly by confirming APs successfully execute the Rust entry point and complete initialization.

### Exploratory Fault Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm that APs hang when attempting to call the kernel virtual address entry point. If the hang does not occur or occurs for different reasons, we will need to re-hypothesize.

**Test Plan**: Boot the system with multiple APs enabled and observe the serial output. Run these tests on the UNFIXED code to observe failures and understand the root cause. Monitor both debugcon (0xE9) for assembly output and COM1 (0x3f8) for Rust entry point output.

**Test Cases**:
1. **Single AP Boot Test**: Boot with 2 CPUs (BSP + 1 AP), observe that assembly prints ABCDEFG but no 'A' appears on COM1 (will fail on unfixed code)
2. **Multiple AP Boot Test**: Boot with 4 CPUs (BSP + 3 APs), observe that all APs complete assembly but none enter Rust code (will fail on unfixed code)
3. **Entry Point Address Verification**: Add debug output in prepare_ap_startup_data to print the entry point address, confirm it's in kernel virtual address range 0x7xxxxxxx (will show virtual address on unfixed code)
4. **Page Table Mapping Check**: Manually inspect kernel PML4 to verify whether the entry point virtual address is mapped (may reveal missing identity mapping on unfixed code)

**Expected Counterexamples**:
- APs complete assembly startup (ABCDEFG printed) but hang at the `call rax` instruction
- No 'A' character appears on COM1, indicating ap_entry never executes
- System hangs indefinitely without reaching wait_for_online timeout
- Possible causes: virtual address not identity-mapped, physical address calculation missing, page table context mismatch

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds (AP attempting to call entry point), the fixed function produces the expected behavior (successful execution of ap_entry).

**Pseudocode:**
```
FOR ALL ap_startup WHERE isBugCondition(ap_startup) DO
  result := ap_startup_with_fix(ap_startup)
  ASSERT result.ap_entry_executed == true
  ASSERT result.debug_output_contains('A')
  ASSERT result.ap_marked_online == true
END FOR
```

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold (BSP boot, normal kernel operation), the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL system_operation WHERE NOT isBugCondition(system_operation) DO
  ASSERT original_behavior(system_operation) = fixed_behavior(system_operation)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many test cases automatically across the input domain (different boot configurations, memory layouts)
- It catches edge cases that manual unit tests might miss (unusual page table configurations, different CPU counts)
- It provides strong guarantees that behavior is unchanged for all non-buggy inputs (BSP boot, existing kernel operations)

**Test Plan**: Observe behavior on UNFIXED code first for BSP boot and normal operations, then write property-based tests capturing that behavior. Verify the fix does not alter any existing functionality.

**Test Cases**:
1. **BSP Boot Preservation**: Observe that BSP boots successfully on unfixed code, then verify this continues after fix with identical behavior
2. **Assembly Startup Preservation**: Observe that APs print ABCDEFG on unfixed code, then verify this continues after fix with identical output
3. **Kernel Virtual Address Preservation**: Observe that kernel code uses virtual addresses correctly on unfixed code, then verify this continues after fix
4. **Page Table Setup Preservation**: Observe that prepare_ap_startup_data sets up PML4 correctly on unfixed code, then verify this continues after fix

### Unit Tests

- Test prepare_ap_startup_data with different entry point addresses and verify correct physical address calculation
- Test that BSP boot completes successfully with the fix applied
- Test that AP assembly startup continues to produce correct debug output (ABCDEFG)
- Test edge cases: single AP, maximum APs, different memory configurations

### Property-Based Tests

- Generate random CPU configurations (1-16 CPUs) and verify all APs successfully enter Rust code
- Generate random memory layouts and verify entry point address is always accessible
- Test that BSP boot behavior is identical across many random configurations
- Verify assembly debug output (ABCDEFG) appears for all APs across many test runs

### Integration Tests

- Test full SMP initialization with multiple APs, verifying all reach ap_entry and mark themselves online
- Test that wait_for_online() completes successfully without timeout after fix
- Test that all debug output appears in correct sequence: ABCDEFG from assembly, then A-I from Rust
- Test system stability after all APs are online (run scheduler, handle interrupts, etc.)
