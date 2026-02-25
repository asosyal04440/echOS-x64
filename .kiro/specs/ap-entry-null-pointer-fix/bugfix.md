# Bugfix Requirements Document

## Introduction

The echOS kernel experiences a critical failure during SMP initialization where Application Processors (APs) fail to start, causing a triple fault and system reboot. The root cause is that the `prepare_ap_startup_data()` function in `src/cpu/smp.rs` does not initialize the `entry` field of the `ApStartupData` structure, leaving it as a null pointer. When the AP startup assembly code attempts to call this null pointer, it triggers a jump to address 0x0, resulting in a triple fault and immediate system reboot.

This bug prevents multi-processor initialization and limits the system to single-processor operation. The fix requires properly initializing the `entry` field to point to the `ap_entry` function and ensuring the `pml4_phys` field is also set for proper page table configuration.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN `prepare_ap_startup_data(stack_top, cpu_data)` is called THEN the `ApStartupData.entry` field remains uninitialized (null/0)

1.2 WHEN the AP startup assembly code executes at line 172 in `src/cpu/ap_startup.asm` THEN it loads the null `entry` pointer into RAX

1.3 WHEN the AP startup assembly code executes the call instruction at line 184 THEN it attempts to jump to address 0x0

1.4 WHEN the processor attempts to execute code at address 0x0 THEN a triple fault occurs and the system reboots

1.5 WHEN `prepare_ap_startup_data()` is called THEN the `ApStartupData.pml4_phys` field remains uninitialized

### Expected Behavior (Correct)

2.1 WHEN `prepare_ap_startup_data()` is called THEN the system SHALL set `ApStartupData.entry` to the physical address of the `ap_entry` function

2.2 WHEN `prepare_ap_startup_data()` is called THEN the system SHALL set `ApStartupData.pml4_phys` to the physical address of the AP's PML4 page table

2.3 WHEN the AP startup assembly code loads the `entry` pointer THEN it SHALL contain a valid function address pointing to `ap_entry`

2.4 WHEN the AP startup assembly code calls the `entry` pointer THEN the system SHALL successfully jump to the `ap_entry` function and continue AP initialization

2.5 WHEN all `ApStartupData` fields are properly initialized THEN the AP SHALL complete its startup sequence without triggering a triple fault

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the BSP (Bootstrap Processor) initializes THEN the system SHALL CONTINUE TO initialize successfully without modification

3.2 WHEN `prepare_ap_startup_data()` sets the `stack_top` field THEN it SHALL CONTINUE TO allocate and assign the AP stack correctly

3.3 WHEN `prepare_ap_startup_data()` sets the `cpu_data` field THEN it SHALL CONTINUE TO initialize the CpuData structure correctly

3.4 WHEN the INIT/SIPI IPI sequence is sent to APs THEN it SHALL CONTINUE TO execute without modification

3.5 WHEN the AP startup assembly code in `src/cpu/ap_startup.asm` executes THEN it SHALL CONTINUE TO read the `ApStartupData` structure fields in the same order and manner

3.6 WHEN the AP loads the startup code into low memory THEN the system SHALL CONTINUE TO use the same memory layout and addressing
