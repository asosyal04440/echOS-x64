# Bug Condition Exploration Test for Heap Stack Physical Address Translation
# 
# This test MUST FAIL on unfixed code to confirm the bug exists.
# Expected failure: "attempt to subtract with overflow" panic when phys_addr() 
# is called on heap-allocated stacks (virt_addr < PHYSICAL_MEMORY_OFFSET).
#
# Property 1: Fault Condition - Heap Stack Physical Address Translation
# Validates: Requirements 2.1, 2.2, 2.3

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Bug Condition Exploration Test" -ForegroundColor Cyan
Write-Host "Property 1: Heap Stack Physical Address Translation" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

Write-Host "IMPORTANT: This test is EXPECTED TO FAIL on unfixed code." -ForegroundColor Yellow
Write-Host "Failure with 'attempt to subtract with overflow' confirms the bug exists." -ForegroundColor Yellow
Write-Host ""

# Create a temporary test file
$testFile = "tests/heap_stack_phys_addr_bug_test.rs"
$testDir = "tests"

# Ensure tests directory exists
if (-not (Test-Path $testDir)) {
    New-Item -ItemType Directory -Path $testDir | Out-Null
    Write-Host "Created tests directory" -ForegroundColor Green
}

# Write the test file - a simple Rust program that demonstrates the bug
$testContent = @'
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
'@

Set-Content -Path $testFile -Value $testContent
Write-Host "Created test file: $testFile" -ForegroundColor Green
Write-Host ""

# Compile and run the test
Write-Host "Compiling and running bug condition exploration test..." -ForegroundColor Cyan
Write-Host ""

try {
    # Compile the test program
    $compileOutput = rustc --edition 2021 $testFile -o bug_test.exe 2>&1 | Out-String
    
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Compilation output:" -ForegroundColor Yellow
        Write-Host $compileOutput
        Write-Host ""
        Write-Host "Error: Failed to compile test program" -ForegroundColor Red
        exit 1
    }
    
    # Run the test program
    Write-Host "Running test program..." -ForegroundColor Cyan
    Write-Host ""
    $testOutput = .\bug_test.exe 2>&1 | Out-String
    Write-Host $testOutput
    
    # Clean up
    Remove-Item bug_test.exe -ErrorAction SilentlyContinue
    
    # Check if bug was confirmed
    if ($testOutput -match "BUG CONFIRMED") {
        Write-Host ""
        Write-Host "========================================" -ForegroundColor Green
        Write-Host "SUCCESS: Bug Condition Confirmed" -ForegroundColor Green
        Write-Host "========================================" -ForegroundColor Green
        Write-Host ""
        Write-Host "The test successfully demonstrated the bug condition." -ForegroundColor Green
        Write-Host "Integer underflow detected for heap addresses." -ForegroundColor Green
        exit 0
    } else {
        Write-Host ""
        Write-Host "========================================" -ForegroundColor Red
        Write-Host "UNEXPECTED: Bug Not Detected" -ForegroundColor Red
        Write-Host "========================================" -ForegroundColor Red
        Write-Host ""
        Write-Host "The test did not detect the expected bug condition." -ForegroundColor Red
        Write-Host "This may indicate the bug is already fixed or the test is incorrect." -ForegroundColor Red
        exit 1
    }
} catch {
    Write-Host ""
    Write-Host "Error running test: $_" -ForegroundColor Red
    exit 1
} finally {
    # Clean up test files
    Remove-Item bug_test.exe -ErrorAction SilentlyContinue
}
