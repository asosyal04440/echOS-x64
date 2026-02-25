# Bug Condition Exploration Test for Per-CPU Loop Corruption
# **Validates: Requirements 2.1, 2.2, 2.4, 2.5**
#
# This test MUST FAIL on unfixed code - failure confirms the bug exists
# DO NOT attempt to fix the test or the code when it fails
#
# Property 1: Fault Condition - Loop Executes Correct Number of Times
# For cpu_count=4 (BSP + 3 APs), the loop should execute 3 times but only executes once due to corruption

Write-Host "=== Bug Condition Exploration Test ===" -ForegroundColor Cyan
Write-Host "Testing per-CPU initialization loop execution count and variable corruption" -ForegroundColor Cyan
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

# Test 1: Count loop iterations (should be 3 for cpu_count=4, but will be 1 on unfixed code)
Write-Host "Test 1: Loop Iteration Count" -ForegroundColor Cyan
$loopIterations = ([regex]::Matches($logContent, "SMP: Creating per_cpu_data for cpu_id \d+")).Count
Write-Host "  Expected: 3 iterations (for cpu_id 1, 2, 3)"
Write-Host "  Actual: $loopIterations iterations"

if ($loopIterations -ne 3) {
    Write-Host "  FAIL: Loop executed $loopIterations times instead of 3" -ForegroundColor Red
    $test1Failed = $true
} else {
    Write-Host "  PASS: Loop executed correct number of times" -ForegroundColor Green
    $test1Failed = $false
}
Write-Host ""

# Test 2: Check for loop variable corruption (cpu_id should be 1, 2, 3 but will show 0 on unfixed code)
Write-Host "Test 2: Loop Variable Corruption Detection" -ForegroundColor Cyan
$stackTopMessages = [regex]::Matches($logContent, "SMP: cpu_id (\d+) stack_top = (0x[0-9a-f]+)")

Write-Host "  Stack top messages found:"
foreach ($match in $stackTopMessages) {
    $cpuId = $match.Groups[1].Value
    $stackTop = $match.Groups[2].Value
    Write-Host "    cpu_id $cpuId stack_top = $stackTop"
}

# Check if any cpu_id is 0 when it should be 1, 2, or 3
$corruptionDetected = $false
foreach ($match in $stackTopMessages) {
    $cpuId = [int]$match.Groups[1].Value
    if ($cpuId -eq 0) {
        Write-Host "  FAIL: Loop variable corrupted to 0 (should be 1, 2, or 3)" -ForegroundColor Red
        $corruptionDetected = $true
        break
    }
}

if (-not $corruptionDetected -and $stackTopMessages.Count -gt 0) {
    Write-Host "  PASS: No loop variable corruption detected" -ForegroundColor Green
}
Write-Host ""

# Test 3: Check per_cpu_data length (should be 4 after loop, but will be 2 on unfixed code)
Write-Host "Test 3: Per-CPU Data Length" -ForegroundColor Cyan
$perCpuDataLengths = [regex]::Matches($logContent, "SMP: cpu_id \d+ added to per_cpu_data \(len=(\d+)\)")

if ($perCpuDataLengths.Count -gt 0) {
    $lastLength = [int]$perCpuDataLengths[$perCpuDataLengths.Count - 1].Groups[1].Value
    Write-Host "  Expected final length: 4 (BSP + 3 APs)"
    Write-Host "  Actual final length: $lastLength"
    
    if ($lastLength -ne 4) {
        Write-Host "  FAIL: per_cpu_data.len() is $lastLength instead of 4" -ForegroundColor Red
        $test3Failed = $true
    } else {
        Write-Host "  PASS: per_cpu_data.len() is correct" -ForegroundColor Green
        $test3Failed = $false
    }
} else {
    Write-Host "  ERROR: Could not find per_cpu_data length in log" -ForegroundColor Red
    $test3Failed = $true
}
Write-Host ""

# Test 4: Verify cpu_count is 4
Write-Host "Test 4: CPU Count Verification" -ForegroundColor Cyan
if ($logContent -match "SMP: cpu_count = (\d+)") {
    $cpuCount = [int]$Matches[1]
    Write-Host "  CPU count: $cpuCount"
    
    if ($cpuCount -ne 4) {
        Write-Host "  WARNING: Test expects cpu_count=4, but found $cpuCount" -ForegroundColor Yellow
    } else {
        Write-Host "  PASS: CPU count is 4 as expected" -ForegroundColor Green
    }
} else {
    Write-Host "  ERROR: Could not find cpu_count in log" -ForegroundColor Red
}
Write-Host ""

# Summary
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

$allTestsPassed = (-not $test1Failed) -and (-not $corruptionDetected) -and (-not $test3Failed)

if ($allTestsPassed) {
    Write-Host "ALL TESTS PASSED - Bug is FIXED" -ForegroundColor Green
    Write-Host "The loop executes the correct number of times with no variable corruption" -ForegroundColor Green
    exit 0
} else {
    Write-Host "TESTS FAILED - Bug is PRESENT (Expected on unfixed code)" -ForegroundColor Red
    Write-Host ""
    Write-Host "Counterexamples found:" -ForegroundColor Yellow
    
    if ($test1Failed) {
        Write-Host "  - Loop executed $loopIterations times instead of 3" -ForegroundColor Yellow
    }
    
    if ($corruptionDetected) {
        Write-Host "  - Loop variable cpu_id corrupted to 0 instead of maintaining values 1, 2, 3" -ForegroundColor Yellow
    }
    
    if ($test3Failed) {
        Write-Host "  - per_cpu_data.len() is $lastLength instead of 4" -ForegroundColor Yellow
    }
    
    Write-Host ""
    Write-Host "This confirms the bug exists: the per-CPU initialization loop only executes once" -ForegroundColor Yellow
    Write-Host "due to stack frame invalidation by the inline assembly stack switch operation." -ForegroundColor Yellow
    exit 1
}
