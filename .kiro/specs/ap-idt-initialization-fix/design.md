# AP IDT Initialization Bugfix Design

## Overview

Application Processors (APs) triple fault immediately upon entering Rust code because the Interrupt Descriptor Table (IDT) is not loaded. The BSP loads its IDT during early initialization via `interrupts::init()`, but APs skip this step and enter `ap_entry` with IDT base address 0x0. When any exception occurs (like a page fault), the CPU cannot find the handler and triggers a triple fault.

The fix requires loading the IDT for each AP before entering Rust code where exceptions can occur. The IDT can be shared across all CPUs (same physical table) since the handler code is identical. Each CPU just needs to execute the LIDT instruction to load the IDT base address into its IDTR register.

## Glossary

- **Bug_Condition (C)**: The condition that triggers the bug - when an AP enters Rust code without IDT configured (IDT base = 0x0)
- **Property (P)**: The desired behavior - APs must have a valid IDT loaded before entering Rust code where exceptions can occur
- **Preservation**: Existing BSP IDT initialization and exception handling that must remain unchanged
- **IDT (Interrupt Descriptor Table)**: CPU data structure mapping interrupt/exception vectors to handler functions
- **IDTR**: CPU register holding the base address and limit of the IDT
- **LIDT**: x86 instruction that loads the IDT base address and limit into IDTR
- **ap_entry**: Rust function in `src/cpu/ap.rs` that serves as the entry point for APs after assembly startup
- **init_per_cpu**: Function in `src/interrupts/mod.rs` that loads the IDT for a CPU via `idt.load()`
- **Triple Fault**: CPU fault condition when an exception occurs during double fault handling, causing system reset/hang

## Bug Details

### Fault Condition

The bug manifests when an AP enters the `ap_entry` Rust function without having loaded the IDT. The AP's IDTR register contains base address 0x0, so when any exception occurs (page fault, general protection fault, etc.), the CPU cannot locate the exception handler and triggers a double fault, which also cannot be handled, resulting in a triple fault.

**Formal Specification:**
```
FUNCTION isBugCondition(input)
  INPUT: input of type CPUContext
  OUTPUT: boolean
  
  RETURN input.cpu_type = AP
         AND input.execution_point = "ap_entry"
         AND input.idt_base = 0x0
         AND exception_occurs(input)
END FUNCTION
```

### Examples

- **Example 1**: AP with LAPIC ID 1 enters `ap_entry`, page fault occurs at CR2=0xfffffffffffffffe, IDT base is 0x0, CPU cannot find page fault handler → double fault → triple fault → system hang

- **Example 2**: AP with LAPIC ID 2 enters `ap_entry`, attempts to access invalid memory, general protection fault occurs, IDT base is 0x0, CPU cannot find GPF handler → double fault → triple fault → system hang

- **Example 3**: AP with LAPIC ID 3 enters `ap_entry`, divide by zero exception occurs, IDT base is 0x0, CPU cannot find divide error handler → double fault → triple fault → system hang

- **Edge Case**: AP enters `ap_entry` with IDT loaded but encounters an exception not registered in the IDT → expected behavior is to invoke a default handler or panic gracefully, not triple fault

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- BSP IDT initialization via `interrupts::init()` in `main.rs` must continue to work exactly as before
- BSP exception handling must continue to invoke the correct handlers from the IDT
- All existing exception handler implementations must remain unchanged
- AP assembly startup sequence (GDT, paging, stack setup) must remain unchanged
- IDT structure and handler registration logic must remain unchanged

**Scope:**
All inputs that do NOT involve APs entering Rust code should be completely unaffected by this fix. This includes:
- BSP boot and initialization sequence
- BSP exception and interrupt handling
- AP assembly startup code (before IDT load)
- IDT handler implementations
- Interrupt registration and dispatch logic

## Hypothesized Root Cause

Based on the bug description and code analysis, the root cause is clear:

1. **Missing IDT Load in AP Path**: The BSP calls `interrupts::init()` → `init_per_cpu()` → `idt.load()` during early boot (in `main.rs`), but APs jump directly to `ap_entry` without calling `init_per_cpu()` first.

2. **IDT Load Happens Too Late**: In the current `ap_entry` code, `interrupts::init_per_cpu()` is called AFTER several operations that could trigger exceptions:
   - Raw UART writes (port I/O)
   - Per-CPU data initialization
   - APIC initialization
   - GDT initialization
   
   Any of these operations could cause a page fault or other exception before the IDT is loaded.

3. **No Assembly-Level IDT Load**: The AP assembly startup code (`ap_startup.asm`) loads the GDT and sets up paging, but does not load the IDT. This means the AP enters 64-bit long mode and Rust code with no exception handling capability.

## Correctness Properties

Property 1: Fault Condition - AP IDT Loaded Before Rust Entry

_For any_ AP that enters the `ap_entry` Rust function, the fixed code SHALL ensure that the IDT is loaded (IDTR base address is non-zero and points to a valid IDT) BEFORE any operation that could trigger an exception is executed.

**Validates: Requirements 2.1, 2.2, 2.3, 2.4**

Property 2: Preservation - BSP and Existing Functionality

_For any_ CPU context that is NOT an AP entering Rust code (BSP initialization, BSP exception handling, AP assembly startup), the fixed code SHALL produce exactly the same behavior as the original code, preserving all existing IDT initialization and exception handling functionality.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5**

## Fix Implementation

### Changes Required

The fix requires loading the IDT for each AP before entering Rust code where exceptions can occur. There are two possible approaches:

**Approach 1: Early IDT Load in ap_entry (Recommended)**

Move the `interrupts::init_per_cpu()` call to the very beginning of `ap_entry`, before any other operations.

**File**: `src/cpu/ap.rs`

**Function**: `ap_entry`

**Specific Changes**:
1. **Move init_per_cpu to First Operation**: Call `crate::interrupts::init_per_cpu()` as the FIRST operation in `ap_entry`, before any raw UART writes or other operations
   - This ensures the IDT is loaded before any exception can occur
   - The function is safe to call early since it only loads the IDT for the current CPU

2. **Verify CPU ID Availability**: Ensure that `current_cpu_id()` works correctly when called from `init_per_cpu()` at this early stage
   - May need to set up minimal per-CPU data first if `current_cpu_id()` depends on it
   - Alternative: Pass cpu_id as parameter to a new `init_per_cpu_with_id(cpu_id)` function

**Approach 2: Assembly-Level IDT Load (Alternative)**

Load the IDT in the assembly startup code before jumping to Rust.

**File**: `src/cpu/ap_startup.asm`

**Specific Changes**:
1. **Add IDT Pointer to ap_startup_data**: Add a field for IDT base address and limit
2. **Load IDT Before Rust Call**: Execute LIDT instruction in assembly before calling `ap_entry`
3. **Pass IDT Address from Rust**: Modify `start_ap` in `src/cpu/smp.rs` to pass IDT address in startup data

This approach is more complex and requires assembly changes, so Approach 1 is recommended.

### Recommended Implementation (Approach 1)

**File**: `src/cpu/ap.rs`

**Changes**:
1. Move `crate::interrupts::init_per_cpu()` to the very first line of `ap_entry`
2. Remove the later call to `init_per_cpu()` (currently after GDT init)
3. Ensure this works with the current `current_cpu_id()` implementation

**Pseudocode**:
```rust
#[no_mangle]
pub extern "sysv64" fn ap_entry(cpu_data: &'static mut CpuData) -> ! {
    // CRITICAL: Load IDT FIRST before any operation that could trigger an exception
    crate::interrupts::init_per_cpu();
    
    // Now safe to perform operations that might trigger exceptions
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'A');
    }
    
    // ... rest of initialization
}
```

**Potential Issue**: If `current_cpu_id()` (called by `init_per_cpu()`) depends on per-CPU data being initialized first, we may need to:
- Initialize minimal per-CPU data before calling `init_per_cpu()`
- OR create a variant `init_per_cpu_with_id(cpu_id: u32)` that takes cpu_id as parameter
- OR extract cpu_id from cpu_data parameter: `let cpu_id = cpu_data.cpu_id;`

**Refined Implementation**:
```rust
#[no_mangle]
pub extern "sysv64" fn ap_entry(cpu_data: &'static mut CpuData) -> ! {
    // Initialize per-CPU data pointer FIRST (required for current_cpu_id())
    unsafe {
        init_cpu_data(cpu_data as *mut CpuData);
    }
    
    // CRITICAL: Load IDT immediately after per-CPU data setup
    crate::interrupts::init_per_cpu();
    
    // Now safe to perform operations that might trigger exceptions
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0x3f8).write(b'A');
    }
    
    // ... rest of initialization (remove duplicate init_per_cpu call)
}
```

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bug on unfixed code (AP triple faults without IDT), then verify the fix works correctly (AP handles exceptions) and preserves existing behavior (BSP unchanged).

### Exploratory Fault Condition Checking

**Goal**: Surface counterexamples that demonstrate the bug BEFORE implementing the fix. Confirm that APs triple fault due to missing IDT.

**Test Plan**: Run the unfixed kernel with SMP enabled and observe AP behavior via serial logs and QEMU traces. Document the triple fault sequence and verify that IDT base is 0x0 when the fault occurs.

**Test Cases**:
1. **AP Entry Triple Fault Test**: Boot with multiple APs, observe that APs reach `ap_entry` but triple fault before printing 'A' (will fail on unfixed code)
2. **IDT Base Check Test**: Use QEMU monitor to inspect IDTR register when AP enters `ap_entry`, verify base is 0x0 (will show 0x0 on unfixed code)
3. **Exception Sequence Test**: Trace the exception sequence (page fault → double fault → triple fault) via QEMU logs (will show triple fault on unfixed code)
4. **BSP IDT Verification Test**: Verify that BSP has valid IDT loaded and can handle exceptions correctly (should pass on unfixed code)

**Expected Counterexamples**:
- AP enters `ap_entry` with IDTR base = 0x0
- Page fault or other exception occurs
- CPU cannot find handler in IDT (base is null)
- Double fault occurs, also cannot be handled
- Triple fault → system hang
- Serial log shows AP reached `ap_entry` but no 'A' character printed

### Fix Checking

**Goal**: Verify that for all APs entering Rust code, the fixed code loads the IDT before any exception can occur.

**Pseudocode:**
```
FOR ALL cpu WHERE cpu.type = AP AND cpu.execution_point = "ap_entry" DO
  result := ap_entry_fixed(cpu)
  ASSERT cpu.idt_base != 0x0
  ASSERT idt_is_valid(cpu.idt_base)
  ASSERT can_handle_exceptions(cpu)
  ASSERT ap_completes_initialization(cpu)
END FOR
```

**Test Plan**: Boot the fixed kernel with multiple APs and verify that:
- Each AP loads the IDT before performing any operations
- Each AP can handle exceptions without triple faulting
- Each AP completes initialization and prints debug output
- Each AP reaches the idle loop successfully

**Test Cases**:
1. **IDT Load Verification**: Verify IDTR base is non-zero when AP enters Rust code
2. **Exception Handling Test**: Trigger a controlled exception on AP (e.g., breakpoint) and verify it's handled correctly
3. **AP Initialization Completion**: Verify all APs print 'A' through 'I' debug characters and reach idle loop
4. **Multi-AP Test**: Boot with 4+ APs and verify all complete initialization

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold (BSP initialization, BSP exception handling, AP assembly startup), the fixed code produces the same result as the original code.

**Pseudocode:**
```
FOR ALL input WHERE NOT isBugCondition(input) DO
  ASSERT original_behavior(input) = fixed_behavior(input)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many test cases automatically across the input domain
- It catches edge cases that manual unit tests might miss
- It provides strong guarantees that behavior is unchanged for all non-buggy inputs

**Test Plan**: Observe behavior on UNFIXED code first for BSP initialization and exception handling, then write property-based tests capturing that behavior and verify it's unchanged after the fix.

**Test Cases**:
1. **BSP IDT Initialization Preservation**: Verify BSP calls `interrupts::init()` at the same point in boot sequence and loads IDT correctly
2. **BSP Exception Handling Preservation**: Trigger various exceptions on BSP (page fault, divide by zero, breakpoint) and verify handlers are invoked correctly
3. **AP Assembly Startup Preservation**: Verify AP assembly code (GDT load, paging setup, stack setup) executes identically
4. **IDT Structure Preservation**: Verify IDT structure, handler registrations, and dispatch logic are unchanged
5. **Interrupt Handling Preservation**: Verify timer, keyboard, and other interrupt handlers work identically

### Unit Tests

- Test that `init_per_cpu()` can be called early in `ap_entry` without crashing
- Test that `current_cpu_id()` returns correct value after per-CPU data initialization
- Test that IDT is loaded correctly for each CPU (verify IDTR base address)
- Test that exception handlers are invoked correctly on APs
- Test that BSP IDT initialization remains unchanged

### Property-Based Tests

- Generate random AP configurations (different LAPIC IDs, CPU counts) and verify all APs load IDT successfully
- Generate random exception scenarios and verify all CPUs handle them correctly
- Test that BSP behavior is identical across many boot scenarios
- Test that AP assembly startup is unchanged across many configurations

### Integration Tests

- Test full SMP boot with multiple APs, verify all complete initialization
- Test exception handling on all CPUs (BSP and APs) during normal operation
- Test that system remains stable with APs handling interrupts and exceptions
- Test that AP initialization order doesn't affect IDT loading
- Test edge cases: single CPU, maximum CPU count, AP failures
