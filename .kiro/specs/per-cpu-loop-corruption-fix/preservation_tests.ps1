# Preservation Property Tests for Per-CPU Loop Corruption Fix
# **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5**
#
# Property 2: Preservation - BSP and AP Initialization Behavior
# These tests capture the behavior that MUST remain unchanged after the fix

Write-Host "=== Preservation Property Tests ===" -ForegroundColor Cyan
Write-Host "Testing that non-buggy code paths remain unchanged" -ForegroundColor Cyan
Write-Host ""

# Find the latest serial log
$logFile = Get-ChildItem -Path "logs" -Filter "serial_*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

if (-not $logFile) {
    Write-Host "ERROR: No serial log found in logs/ directory" -ForegroundColor Red
    exit 1
}

Write-Host "Analyzing log file: $($logFile.FullName)" -ForegroundColor Yellow
Write-Host ""

$logContent = Get-Content $logFile.FullName -Raw

$allTestsPassed = $true

# Test Case 1: BSP Initialization Preservation
Write-Host "Test Case 1: BSP Initialization Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.1: BSP per-CPU data initialization must work correctly"

# Check for BSP setup messages
if ($logContent -match "SMP: BSP per-cpu setup begin") {
    Write-Host "  [PASS] BSP per-cpu setup begin message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] BSP per-cpu setup begin message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($logContent -match "SMP: BSP per-cpu setup done") {
    Write-Host "  [PASS] BSP per-cpu setup done message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] BSP per-cpu setup done message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Verify BSP setup occurs before the loop
$bspSetupDoneMatch = [regex]::Match($logContent, "SMP: BSP per-cpu setup done")
$loopStartMatch = [regex]::Match($logContent, "SMP: About to read cpu_count")

if ($bspSetupDoneMatch.Success -and $loopStartMatch.Success) {
    if ($bspSetupDoneMatch.Index -lt $loopStartMatch.Index) {
        Write-Host "  [PASS] BSP setup completes before AP initialization loop" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] BSP setup does NOT complete before AP initialization loop" -ForegroundColor Red
        $allTestsPassed = $false
    }
}

Write-Host ""

# Test Case 2: Scheduler Update Preservation
Write-Host "Test Case 2: Scheduler Update Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.3: Scheduler must receive correct cpu_count"

$schedulerMatch = [regex]::Match($logContent, "Scheduler updated for (\d+) CPUs")
if ($schedulerMatch.Success) {
    $schedulerCpuCount = [int]$schedulerMatch.Groups[1].Value
    Write-Host "  Scheduler updated for: $schedulerCpuCount CPUs"
    
    if ($schedulerCpuCount -eq 4) {
        Write-Host "  [PASS] Scheduler receives correct cpu_count (4)" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] Scheduler receives incorrect cpu_count ($schedulerCpuCount instead of 4)" -ForegroundColor Red
        $allTestsPassed = $false
    }
} else {
    Write-Host "  [FAIL] Scheduler update message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Test Case 3: AP Startup Preservation
Write-Host "Test Case 3: AP Startup Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.4: AP startup code loading must work correctly"

if ($logContent -match "SMP: loading AP startup code") {
    Write-Host "  [PASS] AP startup code loading message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code loading message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($logContent -match "SMP: copying AP startup code to phys=0x1000") {
    Write-Host "  [PASS] AP startup code copying message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code copying message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($logContent -match "SMP: AP startup code copied") {
    Write-Host "  [PASS] AP startup code copied message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code copied message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($logContent -match "SMP: AP PML4 phys=0x[0-9a-f]+") {
    Write-Host "  [PASS] AP PML4 setup message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP PML4 setup message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($logContent -match "SMP: AP startup code ready") {
    Write-Host "  [PASS] AP startup code ready message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code ready message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Test Case 4: Per-CPU Data Structure Population
Write-Host "Test Case 4: Per-CPU Data Structure Population" -ForegroundColor Cyan
Write-Host "  Requirement 3.4: All fields in SmpState must be populated correctly"

# Check that per_cpu_data is being populated
if ($logContent -match "SMP: cpu_id \d+ added to per_cpu_data") {
    Write-Host "  [PASS] per_cpu_data population messages found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] per_cpu_data population messages NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Check that AP startup attempts occur
if ($logContent -match "SMP: starting AP \d+") {
    Write-Host "  [PASS] AP startup attempt messages found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup attempt messages NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Check that per_cpu_data lookup occurs
if ($logContent -match "SMP: Looking for cpu_id = \d+") {
    Write-Host "  [PASS] per_cpu_data lookup messages found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] per_cpu_data lookup messages NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Summary
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

if ($allTestsPassed) {
    Write-Host "ALL PRESERVATION TESTS PASSED" -ForegroundColor Green
    Write-Host "Non-buggy code paths are working correctly and should remain unchanged after fix" -ForegroundColor Green
    exit 0
} else {
    Write-Host "SOME PRESERVATION TESTS FAILED" -ForegroundColor Red
    Write-Host "This indicates that some expected behavior is not present in the current code" -ForegroundColor Red
    Write-Host "Review the failures above to understand what needs to be preserved" -ForegroundColor Yellow
    exit 1
}
