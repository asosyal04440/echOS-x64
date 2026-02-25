# Bugfix Requirements Document

## Introduction

Application Processors (APs) successfully reach the Rust entry point function `ap_entry` after the HHDM address fix, but immediately triple fault because the Interrupt Descriptor Table (IDT) is not configured. When any exception occurs (like a page fault), the CPU cannot handle it and triggers a triple fault, causing the system to hang. This prevents APs from completing initialization and blocks full SMP (Symmetric Multi-Processing) functionality.

Evidence from logs (logs/serial_20260221_233618.log and QEMU trace) shows:
- AP successfully calls ap_entry at HHDM address (RIP=ffff80007d46dc59)
- Page fault occurs at CR2=0xfffffffffffffffe (invalid memory access)
- IDT is not configured (IDT=0000000000000000 in QEMU trace)
- Page fault → Double fault → Triple fault sequence
- System hangs before 'A' character can be printed from ap_entry

The root cause is that the BSP (Bootstrap Processor) configures its IDT during early initialization, but APs do not set up their own IDT before entering Rust code. Each CPU needs its own IDT loaded via the LIDT instruction.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN an AP enters the `ap_entry` Rust function THEN the system has IDT base address set to 0x0 (not configured)

1.2 WHEN an AP encounters any exception (page fault, general protection fault, etc.) THEN the system triggers a triple fault because the IDT is not configured

1.3 WHEN an AP triple faults due to missing IDT THEN the system hangs without completing AP initialization

1.4 WHEN an AP attempts to execute code that causes an exception THEN the system cannot invoke any exception handler because IDT entries are not loaded

### Expected Behavior (Correct)

2.1 WHEN an AP enters the `ap_entry` Rust function THEN the system SHALL have a valid IDT configured and loaded via LIDT instruction

2.2 WHEN an AP encounters any exception (page fault, general protection fault, etc.) THEN the system SHALL invoke the appropriate exception handler from the IDT

2.3 WHEN an AP encounters an exception with a properly configured IDT THEN the system SHALL handle the exception gracefully without triple faulting

2.4 WHEN an AP completes IDT initialization THEN the system SHALL continue with AP initialization and print debug output successfully

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the BSP initializes its IDT during early boot THEN the system SHALL CONTINUE TO configure and load the BSP's IDT correctly

3.2 WHEN the BSP encounters an exception THEN the system SHALL CONTINUE TO handle it using the BSP's configured IDT

3.3 WHEN the BSP's exception handlers are invoked THEN the system SHALL CONTINUE TO execute the correct handler code

3.4 WHEN the AP assembly startup sequence executes before `ap_entry` THEN the system SHALL CONTINUE TO perform the same initialization steps (GDT, paging, stack setup)

3.5 WHEN any CPU (BSP or AP) handles interrupts after IDT initialization THEN the system SHALL CONTINUE TO use the same interrupt handler implementations

## Bug Condition and Property Specification

### Bug Condition Function

```pascal
FUNCTION isBugCondition(X)
  INPUT: X of type CPUContext
  OUTPUT: boolean
  
  // Returns true when an AP enters Rust code without IDT configured
  RETURN (X.cpu_type = AP) AND (X.execution_point = "ap_entry") AND (X.idt_base = 0x0)
END FUNCTION
```

### Property: Fix Checking

```pascal
// Property: AP IDT Configuration Before Rust Entry
FOR ALL X WHERE isBugCondition(X) DO
  result ← ap_initialization'(X)
  ASSERT (X.idt_base != 0x0) AND 
         (idt_is_valid(X.idt_base)) AND
         (can_handle_exceptions(X))
END FOR
```

This ensures that when an AP enters Rust code, it has a valid IDT configured that can handle exceptions.

### Property: Preservation Checking

```pascal
// Property: BSP and Existing Functionality Preserved
FOR ALL X WHERE NOT isBugCondition(X) DO
  ASSERT F(X) = F'(X)
END FOR
```

This ensures that:
- BSP IDT initialization remains unchanged
- BSP exception handling continues to work
- AP assembly startup sequence is preserved
- All existing interrupt handlers function identically

### Counterexample

A concrete example demonstrating the bug:

```
Input: AP with LAPIC ID 1 enters ap_entry function
State:
  - cpu_type: AP
  - execution_point: ap_entry (RIP=ffff80007d46dc59)
  - idt_base: 0x0
  - page_fault_occurs: true (CR2=0xfffffffffffffffe)

Current Behavior (F):
  1. AP enters ap_entry
  2. Page fault occurs
  3. CPU attempts to look up page fault handler in IDT
  4. IDT base is 0x0 (invalid)
  5. Double fault occurs
  6. Double fault handler also cannot be found
  7. Triple fault → System hang

Expected Behavior (F'):
  1. AP enters ap_entry
  2. IDT is already configured (idt_base != 0x0)
  3. Page fault occurs
  4. CPU successfully invokes page fault handler from IDT
  5. Exception is handled gracefully
  6. AP continues initialization
  7. Debug output 'A' is printed
```
