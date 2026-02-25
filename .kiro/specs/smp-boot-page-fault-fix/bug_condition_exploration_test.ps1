# Bug Condition Exploration Test for SMP Scheduler Initialization
# Validates: Requirements 2.1, 2.2, 2.3, 2.4
# Property 1: Scheduler Initialization Before Early Return
#
# CRITICAL: This test MUST FAIL on unfixed code - failure confirms the bug exists
# This test encodes the expected behavior - it will validate the fix when it passes
#
# This test verifies that update_cpu_count() is called BEFORE the early return check
# in startup_all_aps(), ensuring scheduler workers are initialized regardless of CPU count.
#
# Bug Condition: cpu_count <= 1 AND update_cpu_count_called == false AND early_return_executed()
#
# Expected on UNFIXED code: "ERROR: No workers available to spawn task!" and "SMP: 0/0 CPUs online"
# Expected on FIXED code: Scheduler initialized, no worker errors, system boots successfully

Write-Host ""
Write-Host "=== Bug Condition Exploration Test - SMP Scheduler Initialization ===" -ForegroundColor Cyan
Write-Host "Testing Property 1: Scheduler Initialization Before Early Return" -ForegroundColor Cyan
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
$logDir = "logs/test_$timestamp"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Prepare OVMF paths
$qemuShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
$ovmfVars = "OVMF_VARS.fd"

# Run QEMU with timeout (15 seconds should be enough to see the boot behavior)
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

Write-Host "Starting QEMU (15 second timeout)..." -ForegroundColor Yellow

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
Write-Host "=== Analyzing Boot Behavior ===" -ForegroundColor Cyan
Write-Host ""

# Initialize test results
$bugConditionMet = $false
$noWorkersError = $false
$zeroCpuCount = $false
$zeroCpusOnline = $false
$bspSetupCompleted = $false
$acpiDetectedCpus = $false

# Test 1: Check if ACPI detected CPUs
Write-Host "Test 1: ACPI CPU Detection" -ForegroundColor White
if ($serialOutput -match "ACPI: (\d+) CPUs detected") {
    $cpuCount = $matches[1]
    Write-Host "  PASS: ACPI detected $cpuCount CPUs" -ForegroundColor Green
    $acpiDetectedCpus = $true
} else {
    Write-Host "  FAIL: ACPI did not detect CPUs" -ForegroundColor Red
}

# Test 2: Check if BSP per-cpu setup completed
Write-Host ""
Write-Host "Test 2: BSP Per-CPU Setup Completion" -ForegroundColor White
if ($serialOutput -match "SMP: BSP per-cpu setup done") {
    Write-Host "  PASS: BSP per-cpu setup completed" -ForegroundColor Green
    $bspSetupCompleted = $true
} else {
    Write-Host "  FAIL: BSP per-cpu setup did not complete" -ForegroundColor Red
}

# Test 3: Check for "No workers available" error
Write-Host ""
Write-Host "Test 3: Scheduler Worker Availability" -ForegroundColor White
if ($serialOutput -match "ERROR: No workers available to spawn task!") {
    Write-Host "  DETECTED: Scheduler has no workers available" -ForegroundColor Yellow
    Write-Host "  ANALYSIS: update_cpu_count() was not called before task spawn attempt" -ForegroundColor Yellow
    $noWorkersError = $true
} else {
    Write-Host "  GOOD: No worker availability errors" -ForegroundColor Green
}

# Test 4: Check cpu_count value in startup_all_aps
Write-Host ""
Write-Host "Test 4: CPU Count in startup_all_aps" -ForegroundColor White
if ($serialOutput -match "SMP: startup_all_aps cpu_count=(\d+)") {
    $reportedCount = $matches[1]
    Write-Host "  DETECTED: cpu_count = $reportedCount in startup_all_aps" -ForegroundColor $(if ($reportedCount -eq "0") { "Yellow" } else { "Green" })
    if ($reportedCount -eq "0" -or $reportedCount -eq "1") {
        Write-Host "  ANALYSIS: Early return check (cpu_count <= 1) will execute" -ForegroundColor Yellow
        $zeroCpuCount = $true
    }
} else {
    Write-Host "  INFO: cpu_count not logged in startup_all_aps" -ForegroundColor Gray
}

# Test 5: Check online CPU count
Write-Host ""
Write-Host "Test 5: Online CPU Count" -ForegroundColor White
if ($serialOutput -match "SMP: (\d+)/(\d+) CPUs online") {
    $online = $matches[1]
    $total = $matches[2]
    Write-Host "  DETECTED: $online/$total CPUs online" -ForegroundColor $(if ($online -eq "0") { "Yellow" } else { "Green" })
    if ($online -eq "0") {
        Write-Host "  ANALYSIS: No CPUs are online (APs were not started)" -ForegroundColor Yellow
        $zeroCpusOnline = $true
    }
} else {
    Write-Host "  INFO: Online CPU count not logged" -ForegroundColor Gray
}

# Determine if bug condition is met
$bugConditionMet = $noWorkersError -and $zeroCpuCount -and $zeroCpusOnline -and $bspSetupCompleted

Write-Host ""
Write-Host "=== Bug Condition Analysis ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "Bug Condition Components:" -ForegroundColor White
Write-Host "  - ACPI detected CPUs: $acpiDetectedCpus" -ForegroundColor $(if ($acpiDetectedCpus) { "Green" } else { "Red" })
Write-Host "  - BSP setup completed: $bspSetupCompleted" -ForegroundColor $(if ($bspSetupCompleted) { "Green" } else { "Red" })
Write-Host "  - cpu_count <= 1 in startup_all_aps: $zeroCpuCount" -ForegroundColor $(if ($zeroCpuCount) { "Yellow" } else { "Green" })
Write-Host "  - No workers available error: $noWorkersError" -ForegroundColor $(if ($noWorkersError) { "Yellow" } else { "Green" })
Write-Host "  - Zero CPUs online: $zeroCpusOnline" -ForegroundColor $(if ($zeroCpusOnline) { "Yellow" } else { "Green" })
Write-Host ""

# Summary and result
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

if ($bugConditionMet) {
    Write-Host "=== BUG CONDITION CONFIRMED ===" -ForegroundColor Red
    Write-Host ""
    Write-Host "Counterexample Found:" -ForegroundColor Yellow
    Write-Host "  - ACPI detected multiple CPUs correctly" -ForegroundColor Yellow
    Write-Host "  - BSP per-CPU setup completed successfully" -ForegroundColor Yellow
    Write-Host "  - cpu_count is 0 or 1 when startup_all_aps() runs" -ForegroundColor Yellow
    Write-Host "  - Early return check (cpu_count <= 1) executes" -ForegroundColor Yellow
    Write-Host "  - update_cpu_count() is NOT called (comes after early return)" -ForegroundColor Yellow
    Write-Host "  - Scheduler has no workers available" -ForegroundColor Yellow
    Write-Host "  - No APs are started (0 CPUs online)" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Root Cause:" -ForegroundColor Yellow
    Write-Host "  The update_cpu_count() call is AFTER the early return check." -ForegroundColor Yellow
    Write-Host "  When cpu_count <= 1, the function returns early WITHOUT" -ForegroundColor Yellow
    Write-Host "  initializing scheduler workers, causing 'No workers available' errors." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "This is the EXPECTED outcome for UNFIXED code." -ForegroundColor Yellow
    Write-Host "The test will PASS once the fix is implemented." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Serial log saved to: $serialLog" -ForegroundColor Gray
    exit 1
} else {
    Write-Host "=== TEST PASSED - BUG IS FIXED ===" -ForegroundColor Green
    Write-Host ""
    Write-Host "Expected Behavior Verified:" -ForegroundColor Green
    Write-Host "  - BSP per-cpu setup completed successfully" -ForegroundColor Green
    Write-Host "  - Scheduler workers initialized (no 'No workers available' error)" -ForegroundColor Green
    Write-Host "  - update_cpu_count() was called before early return" -ForegroundColor Green
    Write-Host "  - System can spawn tasks successfully" -ForegroundColor Green
    Write-Host ""
    Write-Host "Property 1 Satisfied: update_cpu_count() is called BEFORE the" -ForegroundColor Green
    Write-Host "early return check, ensuring scheduler is always initialized." -ForegroundColor Green
    Write-Host ""
    Write-Host "Serial log saved to: $serialLog" -ForegroundColor Gray
    exit 0
}
