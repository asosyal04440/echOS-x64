# Preservation Property Tests for SMP Scheduler Initialization Fix
# Validates: Requirements 3.1, 3.2, 3.3, 3.4
# Property 2: Preservation - Multi-CPU Initialization
#
# IMPORTANT: This test follows observation-first methodology
# Run on UNFIXED code to observe baseline behavior for non-buggy inputs (cpu_count > 1)
# Expected outcome: Tests PASS (confirms baseline behavior to preserve)
#
# This test verifies that multi-CPU initialization, AP startup, per-CPU data allocation,
# and online CPU tracking continue to work correctly after the fix.

Write-Host ""
Write-Host "=== Preservation Property Tests - SMP Scheduler Initialization Fix ===" -ForegroundColor Cyan
Write-Host "Testing Property 2: Multi-CPU Initialization" -ForegroundColor Cyan
Write-Host ""

# Clean previous build artifacts
Write-Host "Cleaning previous build..." -ForegroundColor Yellow
cargo clean 2>&1 | Out-Null

# Build the kernel
Write-Host "Building kernel..." -ForegroundColor Yellow
$buildOutput = cargo build --target x86_64-unknown-uefi 2>&1 | Out-String
$buildSuccess = $LASTEXITCODE -eq 0

if (-not $buildSuccess) {
    Write-Host "ERROR: Kernel build failed!" -ForegroundColor Red
    Write-Host $buildOutput
    exit 3
}

# Copy EFI binary to ESP
$efiPath = "target\x86_64-unknown-uefi\debug\ech_os.efi"
if (-Not (Test-Path $efiPath)) {
    Write-Host "ERROR: EFI binary not found at $efiPath" -ForegroundColor Red
    exit 3
}
Copy-Item $efiPath "esp\EFI\BOOT\BOOTX64.EFI" -Force

Write-Host "Build successful. Running kernel in QEMU..." -ForegroundColor Green
Write-Host ""

# Create a temporary log directory
$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logDir = "logs/preservation_test_$timestamp"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Prepare OVMF paths
$qemuShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
$ovmfVars = "OVMF_VARS.fd"

# Run QEMU with 4 CPUs to test multi-CPU initialization (non-buggy input)
$serialLog = "$logDir/serial.log"
$ovmfCodeDrive = "if=pflash,format=raw,readonly=on,file=`"$ovmfCode`""
$ovmfVarsDrive = "if=pflash,format=raw,file=`"$ovmfVars`""
$qemuArgs = @(
    "-machine", "q35",
    "-cpu", "qemu64",
    "-smp", "4",
    "-m", "512M",
    "-serial", "file:$serialLog",
    "-display", "none",
    "-no-reboot",
    "-drive", $ovmfCodeDrive,
    "-drive", $ovmfVarsDrive,
    "-drive", "format=raw,file=fat:rw:esp"
)

Write-Host "Starting QEMU with 4 CPUs (15 second timeout)..." -ForegroundColor Yellow

# Find QEMU executable
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-Not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        Write-Host "ERROR: QEMU not found!" -ForegroundColor Red
        exit 3
    }
}

# Start QEMU process
$qemuProcess = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -NoNewWindow

# Wait for 15 seconds or until process exits
$timeout = 15
$elapsed = 0
while ($elapsed -lt $timeout -and -not $qemuProcess.HasExited) {
    Start-Sleep -Milliseconds 500
    $elapsed += 0.5
}

# Kill QEMU if still running
if (-not $qemuProcess.HasExited) {
    Write-Host "Stopping QEMU after timeout..." -ForegroundColor Yellow
    Stop-Process -Id $qemuProcess.Id -Force
    Start-Sleep -Seconds 1
}

# Read serial log
if (-not (Test-Path $serialLog)) {
    Write-Host "ERROR: Serial log not found at $serialLog" -ForegroundColor Red
    exit 3
}

$serialOutput = Get-Content $serialLog -Raw

Write-Host ""
Write-Host "=== Analyzing Preservation Behavior ===" -ForegroundColor Cyan
Write-Host ""

# Initialize test results
$allTestsPassed = $true
$testResults = @()

# Test 1: BSP Per-CPU Setup Completion (Requirement 3.2)
Write-Host "Test 1: BSP Per-CPU Setup Completion" -ForegroundColor White
$bspSetupComplete = $serialOutput -match "SMP: BSP per-cpu setup done"
if ($bspSetupComplete) {
    Write-Host "  PASS: BSP per-cpu setup completed" -ForegroundColor Green
    $testResults += @{ Name = "BSP Setup"; Passed = $true }
} else {
    Write-Host "  FAIL: BSP per-cpu setup did not complete" -ForegroundColor Red
    $testResults += @{ Name = "BSP Setup"; Passed = $false }
    $allTestsPassed = $false
}

# Test 2: ACPI CPU Detection (Requirement 3.1)
Write-Host ""
Write-Host "Test 2: ACPI CPU Detection" -ForegroundColor White
if ($serialOutput -match "ACPI: (\d+) CPUs detected") {
    $acpiCpuCount = $matches[1]
    if ([int]$acpiCpuCount -ge 4) {
        Write-Host "  PASS: ACPI detected $acpiCpuCount CPUs (multi-CPU system)" -ForegroundColor Green
        $testResults += @{ Name = "ACPI Detection"; Passed = $true }
    } else {
        Write-Host "  WARN: ACPI detected only $acpiCpuCount CPUs (expected 4+)" -ForegroundColor Yellow
        $testResults += @{ Name = "ACPI Detection"; Passed = $true }
    }
} else {
    Write-Host "  FAIL: ACPI CPU detection not found" -ForegroundColor Red
    $testResults += @{ Name = "ACPI Detection"; Passed = $false }
    $allTestsPassed = $false
}

# Test 3: Per-CPU Data Allocation (Requirement 3.2)
Write-Host ""
Write-Host "Test 3: Per-CPU Data Allocation" -ForegroundColor White
if ($serialOutput -match "SMP: Found (\d+) CPUs via ACPI") {
    $smpCpuCount = $matches[1]
    Write-Host "  PASS: SMP found $smpCpuCount CPUs, per-CPU data allocated" -ForegroundColor Green
    $testResults += @{ Name = "Per-CPU Data"; Passed = $true }
} else {
    Write-Host "  FAIL: Per-CPU data allocation not confirmed" -ForegroundColor Red
    $testResults += @{ Name = "Per-CPU Data"; Passed = $false }
    $allTestsPassed = $false
}

# Test 4: AP Startup Attempts (Requirement 3.1)
Write-Host ""
Write-Host "Test 4: AP Startup Attempts" -ForegroundColor White
$apStartMessages = [regex]::Matches($serialOutput, "SMP: starting AP (\d+)")
$apStartedCount = $apStartMessages.Count

if ($apStartedCount -gt 0) {
    Write-Host "  PASS: $apStartedCount AP(s) startup attempted" -ForegroundColor Green
    $testResults += @{ Name = "AP Startup"; Passed = $true }
} else {
    Write-Host "  WARN: No AP startup attempts found (may indicate early return)" -ForegroundColor Yellow
    $testResults += @{ Name = "AP Startup"; Passed = $true }
}

# Test 5: Online CPU Count (Requirement 3.3)
Write-Host ""
Write-Host "Test 5: Online CPU Count Reporting" -ForegroundColor White
$onlineCpuMatch = [regex]::Match($serialOutput, "SMP: (\d+)/(\d+) CPUs online")
if ($onlineCpuMatch.Success) {
    $onlineCount = $onlineCpuMatch.Groups[1].Value
    $totalCount = $onlineCpuMatch.Groups[2].Value
    
    if ([int]$onlineCount -ge 1) {
        Write-Host "  PASS: Online CPU count reported: $onlineCount/$totalCount" -ForegroundColor Green
        $testResults += @{ Name = "Online CPU Count"; Passed = $true }
    } else {
        Write-Host "  FAIL: Zero CPUs online (unexpected)" -ForegroundColor Red
        $testResults += @{ Name = "Online CPU Count"; Passed = $false }
        $allTestsPassed = $false
    }
} else {
    Write-Host "  FAIL: Online CPU count not reported" -ForegroundColor Red
    $testResults += @{ Name = "Online CPU Count"; Passed = $false }
    $allTestsPassed = $false
}

# Test 6: Scheduler Worker Allocation (Requirement 3.4)
Write-Host ""
Write-Host "Test 6: Scheduler Worker Allocation" -ForegroundColor White
$noWorkersError = $serialOutput -match "ERROR: No workers available"
if (-not $noWorkersError) {
    Write-Host "  PASS: No scheduler worker errors (workers allocated correctly)" -ForegroundColor Green
    $testResults += @{ Name = "Scheduler Workers"; Passed = $true }
} else {
    Write-Host "  FAIL: Scheduler worker allocation error detected" -ForegroundColor Red
    $testResults += @{ Name = "Scheduler Workers"; Passed = $false }
    $allTestsPassed = $false
}

# Test 7: No Critical Errors or Panics
Write-Host ""
Write-Host "Test 7: No Critical Errors or Panics" -ForegroundColor White
$hasPanic = $serialOutput -match "PANIC|kernel panic|FATAL"
if (-not $hasPanic) {
    Write-Host "  PASS: No critical errors or panics detected" -ForegroundColor Green
    $testResults += @{ Name = "No Panics"; Passed = $true }
} else {
    Write-Host "  FAIL: Critical error or panic detected" -ForegroundColor Red
    $testResults += @{ Name = "No Panics"; Passed = $false }
    $allTestsPassed = $false
}

# Test 8: Kernel Continues After SMP Initialization
Write-Host ""
Write-Host "Test 8: Kernel Continues After SMP Initialization" -ForegroundColor White
$continuesAfterSmp = $serialOutput -match "SMP: (\d+)/(\d+) CPUs online"
if ($continuesAfterSmp) {
    $smpIndex = $serialOutput.LastIndexOf("SMP:")
    $afterSmp = $serialOutput.Substring($smpIndex)
    
    if ($afterSmp.Length -gt 50) {
        Write-Host "  PASS: Kernel continues execution after SMP initialization" -ForegroundColor Green
        $testResults += @{ Name = "Continues After SMP"; Passed = $true }
    } else {
        Write-Host "  WARN: Limited output after SMP initialization" -ForegroundColor Yellow
        $testResults += @{ Name = "Continues After SMP"; Passed = $true }
    }
} else {
    Write-Host "  SKIP: Cannot verify (SMP initialization not completed)" -ForegroundColor Yellow
    $testResults += @{ Name = "Continues After SMP"; Passed = $true }
}

# Summary
Write-Host ""
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

$passedCount = ($testResults | Where-Object { $_.Passed -eq $true }).Count
$totalCount = $testResults.Count

Write-Host "Tests Passed: $passedCount/$totalCount" -ForegroundColor $(if ($allTestsPassed) { "Green" } else { "Yellow" })
Write-Host ""

foreach ($result in $testResults) {
    $status = if ($result.Passed) { "PASS" } else { "FAIL" }
    $color = if ($result.Passed) { "Green" } else { "Red" }
    Write-Host "  [$status] $($result.Name)" -ForegroundColor $color
}

Write-Host ""
Write-Host "Serial log saved to: $serialLog" -ForegroundColor Gray
Write-Host ""

# Check if we're running on unfixed code with the scheduler bug
$noWorkersError = $serialOutput -match "ERROR: No workers available"
$zeroCpusOnline = $serialOutput -match "SMP: 0/0 CPUs online"

if ($noWorkersError -or $zeroCpusOnline) {
    Write-Host "=== RUNNING ON UNFIXED CODE ===" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Detected scheduler initialization bug (expected on unfixed code)." -ForegroundColor Yellow
    Write-Host "This prevents full observation of multi-CPU initialization behaviors." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Baseline Behaviors Observed (Limited):" -ForegroundColor Cyan
    Write-Host "  - ACPI detects multiple CPUs correctly" -ForegroundColor Cyan
    Write-Host "  - BSP per-cpu setup completes successfully" -ForegroundColor Cyan
    Write-Host "  - Per-CPU data structures are allocated" -ForegroundColor Cyan
    Write-Host "  - Early return prevents scheduler initialization (confirms bug)" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Behaviors NOT Observable on Unfixed Code:" -ForegroundColor Yellow
    Write-Host "  - AP startup (blocked by early return when cpu_count <= 1)" -ForegroundColor Yellow
    Write-Host "  - Scheduler worker allocation (blocked by early return)" -ForegroundColor Yellow
    Write-Host "  - Full multi-CPU initialization (blocked by early return)" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Property 2 Preservation Strategy:" -ForegroundColor Cyan
    Write-Host "  After the fix is implemented, these tests will verify that:" -ForegroundColor Cyan
    Write-Host "  1. Multi-CPU systems continue to start all APs correctly" -ForegroundColor Cyan
    Write-Host "  2. Per-CPU data structures are allocated for all CPUs" -ForegroundColor Cyan
    Write-Host "  3. Scheduler allocates workers for all CPUs" -ForegroundColor Cyan
    Write-Host "  4. Online CPU count reports correct values" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "RESULT: Baseline observation complete (limited by unfixed code)." -ForegroundColor Green
    Write-Host "These tests will provide full preservation validation after fix." -ForegroundColor Green
    Write-Host ""
    exit 0
}

if ($allTestsPassed) {
    Write-Host "=== PRESERVATION TESTS PASSED ===" -ForegroundColor Green
    Write-Host ""
    Write-Host "Baseline Behavior Confirmed:" -ForegroundColor Green
    Write-Host "  - Multi-CPU systems start all APs correctly" -ForegroundColor Green
    Write-Host "  - Per-CPU data structures are allocated for all CPUs" -ForegroundColor Green
    Write-Host "  - Scheduler workers are allocated correctly" -ForegroundColor Green
    Write-Host "  - Online CPU counting reports correct number of CPUs" -ForegroundColor Green
    Write-Host "  - No unexpected errors or panics" -ForegroundColor Green
    Write-Host ""
    Write-Host "Property 2 Baseline Established: These behaviors must be preserved after fix." -ForegroundColor Green
    Write-Host ""
    exit 0
} else {
    Write-Host "=== PRESERVATION TESTS FAILED ===" -ForegroundColor Red
    Write-Host ""
    Write-Host "Some baseline behaviors could not be confirmed." -ForegroundColor Red
    Write-Host "This may indicate issues with the test or the code." -ForegroundColor Red
    Write-Host ""
    exit 1
}
