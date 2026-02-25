// Bug Condition Exploration Test
// Property 1: Heap Stack Physical Address Translation
// Validates: Requirements 2.1, 2.2, 2.3
//
// This test demonstrates the bug condition by simulating what happens
// when phys_addr() is called on heap-allocated addresses.

const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

fn main() {
    println!("Bug Condition Exploration Test");
    println!("Property 1: Heap Stack Physical Address Translation");
    println!("========================================\n");
    
    // Test Case 1: Typical heap address (observed in actual bug)
    println!("Test Case 1: Typical heap address");
    let typical_heap_addr: u64 = 0x0000_4444_4447_8A90;
    println!("  Virtual address: {:#018x}", typical_heap_addr);
    println!("  Is heap address (< PHYSICAL_MEMORY_OFFSET): {}", 
             typical_heap_addr < PHYSICAL_MEMORY_OFFSET);
    
    // This simulates what the unfixed phys_addr() does
    match typical_heap_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  ERROR: Subtraction succeeded (unexpected): {:#018x}", phys);
            println!("  This indicates the bug may not exist or test is incorrect\n");
        }
        None => {
            println!("  BUG CONFIRMED: Integer underflow detected!");
            println!("  Attempting to subtract PHYSICAL_MEMORY_OFFSET from heap address");
            println!("  would cause panic in debug mode\n");
        }
    }
    
    // Test Case 2: Low address
    println!("Test Case 2: Low address stack");
    let low_addr: u64 = 0x0000_0000_1000_0000;
    println!("  Virtual address: {:#018x}", low_addr);
    println!("  Is heap address (< PHYSICAL_MEMORY_OFFSET): {}", 
             low_addr < PHYSICAL_MEMORY_OFFSET);
    
    match low_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  ERROR: Subtraction succeeded (unexpected): {:#018x}", phys);
        }
        None => {
            println!("  BUG CONFIRMED: Integer underflow detected!\n");
        }
    }
    
    // Test Case 3: Boundary address (just below HHDM)
    println!("Test Case 3: Boundary address (just below HHDM threshold)");
    let boundary_addr: u64 = 0xFFFF_7FFF_FFFF_FFFF;
    println!("  Virtual address: {:#018x}", boundary_addr);
    println!("  Is heap address (< PHYSICAL_MEMORY_OFFSET): {}", 
             boundary_addr < PHYSICAL_MEMORY_OFFSET);
    
    match boundary_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  ERROR: Subtraction succeeded (unexpected): {:#018x}", phys);
        }
        None => {
            println!("  BUG CONFIRMED: Integer underflow detected!\n");
        }
    }
    
    // Test Case 4: HHDM address (should work correctly)
    println!("Test Case 4: HHDM address (control - should work)");
    let hhdm_addr: u64 = 0xFFFF_8000_0010_0000;
    println!("  Virtual address: {:#018x}", hhdm_addr);
    println!("  Is HHDM address (>= PHYSICAL_MEMORY_OFFSET): {}", 
             hhdm_addr >= PHYSICAL_MEMORY_OFFSET);
    
    match hhdm_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  SUCCESS: Physical address calculated: {:#018x}", phys);
            println!("  This is the expected behavior for HHDM addresses\n");
        }
        None => {
            println!("  ERROR: Unexpected underflow for HHDM address!\n");
        }
    }
    
    println!("========================================");
    println!("Root Cause Analysis:");
    println!("  The unfixed KernelStack::phys_addr() method unconditionally");
    println!("  performs: virt_addr - PHYSICAL_MEMORY_OFFSET");
    println!("  This causes integer underflow for heap addresses where");
    println!("  virt_addr < PHYSICAL_MEMORY_OFFSET");
    println!("\nCounterexamples found:");
    println!("  - Heap addresses (< {:#018x}) cause underflow", PHYSICAL_MEMORY_OFFSET);
    println!("  - Low addresses fail");
    println!("  - Boundary addresses just below HHDM threshold fail");
    println!("  - Only HHDM addresses (>= {:#018x}) work correctly", PHYSICAL_MEMORY_OFFSET);
}
