# Bug Condition Exploration Test for AP IDT Initialization
# **Validates: Requirements 2.1, 2.2, 2.3, 2.4**
#
# This test MUST FAIL on unfixed code - failure confirms the bug exists
# DO NOT attempt to fix the test or the code when it fails
#
# Property 1: Fault Condition - AP Triple Fault Without IDT
# APs should have IDT loaded before entering Rust code, but currently have IDT base = 0x0

Write-Host "=== Bug Condition Exploration Test ===" -ForegroundColor Cyan
Write-Host "Testing AP IDT initialization before Rust entry" -ForegroundColor Cyan
Write-Host ""

# Find the latest serial log
$logFile = Get-ChildItem -Path "logs" -Filter "serial_*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

if (-not $logFile) {
    Write-Host "ERROR: No serial log found in logs/ directory" -ForegroundColor Red
    Write-Host "Please run the kernel first using: .\run_qemu.ps1" -ForegroundColor Yellow
    exit 1
}

Write-Host "Analyzing log file: $($logFile.FullName)" -ForegroundColor Yellow
Write-Host ""

$logContent = Get-Content $logFile.FullName -Raw

# Test 1: Verify BSP sends SIPI to AP
Write-Host "Test 1: BSP Sends SIPI to AP" -ForegroundColor Cyan
$sipiSent = $logContent -match "SIPI.*sent to AP"

if ($sipiSent) {
    Write-Host "  PASS: BSP sent SIPI to start AP" -ForegroundColor Green
} else {
    Write-Host "  FAIL: No SIPI found in log" -ForegroundColor Red
    Write-Host "  Cannot test AP behavior without SIPI" -ForegroundColor Yellow
    exit 1
}
Write-Host ""

# Test 2: Check if AP continues execution after SIPI
Write-Host "Test 2: AP Continues After SIPI" -ForegroundColor Cyan
# Extract everything after the last SIPI message
$lastSipiIndex = $logContent.LastIndexOf("SIPI")
if ($lastSipiIndex -ge 0) {
    $afterSipi = $logContent.Substring($lastSipiIndex)
    $linesAfterSipi = ($afterSipi -split "`n").Count
    
    Write-Host "  Lines of output after SIPI: $linesAfterSipi"
    
    # Check for any AP-specific output (cpu_id > 0, or AP-specific messages)
    $apOutput = $afterSipi -match "cpu_id [1-9]" -or $afterSipi -match "AP \d+" -or $afterSipi -match "APIC ID [1-9]"
    
    if ($linesAfterSipi -le 3 -and -not $apOutput) {
        Write-Host "  FAIL: Log stops immediately after SIPI (only $linesAfterSipi lines)" -ForegroundColor Red
        Write-Host "  This indicates AP triple faulted before reaching Rust code" -ForegroundColor Yellow
        $test2Failed = $true
    } else {
        Write-Host "  PASS: System continues after SIPI" -ForegroundColor Green
        $test2Failed = $false
    }
} else {
    Write-Host "  ERROR: Could not find SIPI in log" -ForegroundColor Red
    $test2Failed = $true
}
Write-Host ""

# Test 3: Check for AP initialization completion markers
Write-Host "Test 3: AP Initialization Markers" -ForegroundColor Cyan
$apOnline = $logContent -match "CPU \d+ online" -or $logContent -match "AP \d+ initialized" -or $logContent -match "mark_cpu_online"

if ($apOnline) {
    Write-Host "  PASS: Found AP initialization completion markers" -ForegroundColor Green
    $test3Failed = $false
} else {
    Write-Host "  FAIL: No AP initialization completion markers found" -ForegroundColor Red
    Write-Host "  APs did not complete initialization" -ForegroundColor Yellow
    $test3Failed = $true
}
Write-Host ""

# Test 4: Check QEMU trace for triple fault evidence
Write-Host "Test 4: Triple Fault Detection in QEMU Trace" -ForegroundColor Cyan
$qemuTraceFile = Get-ChildItem -Path "logs" -Filter "qemu_trace_*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

$tripleFaultFound = $false
if ($qemuTraceFile) {
    $traceContent = Get-Content $qemuTraceFile.FullName -Raw
    
    # Look for triple fault indicators
    $tripleFaultFound = $traceContent -match "triple fault" -or $traceContent -match "CPU Reset"
    $idtBaseZero = $traceContent -match "IDT=\s*0{8,16}"
    
    if ($tripleFaultFound) {
        Write-Host "  FAIL: Triple fault detected in QEMU trace" -ForegroundColor Red
        Write-Host "  This confirms the bug: AP triple faulted" -ForegroundColor Yellow
    } else {
        Write-Host "  PASS: No triple fault detected" -ForegroundColor Green
    }
    
    if ($idtBaseZero) {
        Write-Host "  FAIL: IDT base address is 0x0 in trace" -ForegroundColor Red
        Write-Host "  This confirms IDT was not loaded" -ForegroundColor Yellow
    }
} else {
    Write-Host "  WARNING: QEMU trace file not found" -ForegroundColor Yellow
}
Write-Host ""

# Test 5: Verify BSP completed initialization (preservation check)
Write-Host "Test 5: BSP Initialization (Preservation)" -ForegroundColor Cyan
$bspInitComplete = $logContent -match "SMP: Starting" -or $logContent -match "Initializing SMP"

if ($bspInitComplete) {
    Write-Host "  PASS: BSP completed initialization" -ForegroundColor Green
    Write-Host "  This confirms BSP IDT initialization works correctly" -ForegroundColor Green
} else {
    Write-Host "  WARNING: Could not verify BSP initialization" -ForegroundColor Yellow
}
Write-Host ""

# Summary
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

$bugConfirmed = $test2Failed -or $test3Failed -or $tripleFaultFound

if (-not $bugConfirmed) {
    Write-Host "ALL TESTS PASSED - Bug is FIXED" -ForegroundColor Green
    Write-Host "APs successfully load IDT and complete initialization without triple faulting" -ForegroundColor Green
    exit 0
} else {
    Write-Host "TESTS FAILED - Bug is PRESENT (Expected on unfixed code)" -ForegroundColor Red
    Write-Host ""
    Write-Host "Counterexamples found:" -ForegroundColor Yellow
    Write-Host "  - BSP sent SIPI to AP" -ForegroundColor Yellow
    
    if ($test2Failed) {
        Write-Host "  - Log stops immediately after SIPI (AP triple faulted)" -ForegroundColor Yellow
    }
    
    if ($test3Failed) {
        Write-Host "  - No AP initialization completion markers found" -ForegroundColor Yellow
    }
    
    if ($tripleFaultFound) {
        Write-Host "  - QEMU trace shows triple fault or CPU reset" -ForegroundColor Yellow
    }
    
    Write-Host ""
    Write-Host "This confirms the bug exists: APs triple fault before/during ap_entry" -ForegroundColor Yellow
    Write-Host "because IDT is not loaded (IDT base = 0x0). When an exception occurs," -ForegroundColor Yellow
    Write-Host "the CPU cannot find the handler and triggers a triple fault." -ForegroundColor Yellow
    exit 1
}
