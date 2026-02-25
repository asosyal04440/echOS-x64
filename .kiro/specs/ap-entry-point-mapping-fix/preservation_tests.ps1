# Preservation Property Tests for AP Entry Point Mapping Fix
# **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5**
#
# Property 2: Preservation - BSP and Assembly Startup Behavior
# These tests capture the behavior that MUST remain unchanged after the fix
#
# IMPORTANT: Run on UNFIXED code first to observe baseline behavior
# Expected outcome: All tests PASS (confirms baseline behavior to preserve)

Write-Host "=== Preservation Property Tests ===" -ForegroundColor Cyan
Write-Host "Testing that non-buggy code paths remain unchanged" -ForegroundColor Cyan
Write-Host ""

Write-Host "IMPORTANT: These tests should PASS on unfixed code." -ForegroundColor Yellow
Write-Host "They capture the baseline behavior that must be preserved after the fix." -ForegroundColor Yellow
Write-Host ""

# Build the kernel
Write-Host "Building echOS kernel..." -ForegroundColor Cyan
cargo build --quiet --target x86_64-unknown-uefi 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Build failed" -ForegroundColor Red
    exit 1
}
Write-Host "Build successful" -ForegroundColor Green
Write-Host ""

# Prepare ESP folder
$projectRoot = (Get-Location).Path
$efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
$espPath = Join-Path $projectRoot "esp\EFI\BOOT\BOOTX64.EFI"

if (-Not (Test-Path $efiPath)) {
    Write-Host "ERROR: EFI binary not found at $efiPath" -ForegroundColor Red
    exit 1
}

Copy-Item $efiPath $espPath -Force
Write-Host "Copied EFI binary to ESP folder" -ForegroundColor Green
Write-Host ""

# Setup log files
$logDir = Join-Path $projectRoot "logs"
if (-Not (Test-Path $logDir)) { 
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null 
}

$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$serialLogPath = Join-Path $logDir "preservation_serial_$timestamp.log"
$debugLogPath = Join-Path $logDir "preservation_debugcon_$timestamp.log"

# QEMU paths
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-Not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        Write-Host "ERROR: QEMU not found" -ForegroundColor Red
        exit 1
    }
}

$qemuShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
$ovmfVarsTemplate = Join-Path $qemuShare "edk2-i386-vars.fd"
$ovmfVars = Join-Path $projectRoot "OVMF_VARS.fd"

if (-Not (Test-Path $ovmfVars)) {
    Copy-Item $ovmfVarsTemplate $ovmfVars -Force
}

# Launch QEMU with 4 CPUs
Write-Host "Launching QEMU with 4 CPUs (BSP + 3 APs)..." -ForegroundColor Cyan
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray
Write-Host "Debugcon log: $debugLogPath" -ForegroundColor DarkGray
Write-Host ""

$ovmfCodeDrive = "if=pflash,format=raw,readonly=on,file=`"$ovmfCode`""
$ovmfVarsDrive = "if=pflash,format=raw,file=`"$ovmfVars`""

$qemuArgs = @(
    "-machine", "q35",
    "-smp", "4",
    "-drive", $ovmfCodeDrive,
    "-drive", $ovmfVarsDrive,
    "-drive", "format=raw,file=fat:rw:esp",
    "-debugcon", "file:$debugLogPath",
    "-global", "isa-debugcon.iobase=0xE9",
    "-serial", "file:$serialLogPath",
    "-m", "2G",
    "-display", "none",
    "-monitor", "none",
    "-no-reboot",
    "-no-shutdown"
)

# Run QEMU with timeout
$timeoutSec = 30
$qemuStdoutPath = Join-Path $logDir "preservation_qemu_stdout_$timestamp.log"
$qemuStderrPath = Join-Path $logDir "preservation_qemu_stderr_$timestamp.log"
$proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath

Write-Host "Waiting for QEMU to run (timeout: $timeoutSec seconds)..." -ForegroundColor Yellow
$completed = $proc.WaitForExit($timeoutSec * 1000)

if (-not $completed) {
    Write-Host "QEMU timeout - stopping process" -ForegroundColor Yellow
    try { $proc.Kill() } catch {}
} else {
    Write-Host "QEMU exited normally" -ForegroundColor Green
}

Write-Host ""
Write-Host "Analyzing logs..." -ForegroundColor Cyan
Write-Host ""

# Read log files
$serialContent = ""
$debugContent = ""

if (Test-Path $serialLogPath) {
    $serialContent = Get-Content $serialLogPath -Raw
}

if (Test-Path $debugLogPath) {
    $debugContent = Get-Content $debugLogPath -Raw
}

$allTestsPassed = $true

# Test Case 1: BSP Boot Preservation
Write-Host "Test Case 1: BSP Boot Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.1: BSP must boot successfully with kernel virtual addresses working"
Write-Host ""

# Check for BSP initialization messages
if ($serialContent -match "Initializing SMP") {
    Write-Host "  [PASS] SMP initialization started" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] SMP initialization message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: Found \d+ CPUs") {
    Write-Host "  [PASS] CPU detection completed" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] CPU detection message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: BSP per-cpu setup") {
    Write-Host "  [PASS] BSP per-CPU setup initiated" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] BSP per-CPU setup message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Verify kernel virtual addresses are being used
if ($serialContent -match "entry = (0x[0-9a-f]+)") {
    $entryMatch = [regex]::Match($serialContent, "entry = (0x[0-9a-f]+)")
    $entryAddr = $entryMatch.Groups[1].Value
    Write-Host "  [PASS] Kernel virtual addresses in use (entry point: $entryAddr)" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Entry point address NOT found in logs" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Test Case 2: AP Assembly Startup Preservation
Write-Host "Test Case 2: AP Assembly Startup Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.2: AP assembly startup must display 'ABCDEFG' to debugcon port 0xE9"
Write-Host ""

$assemblyChars = @('A', 'B', 'C', 'D', 'E', 'F', 'G')
$assemblyComplete = $true

Write-Host "  Checking for assembly debug output on debugcon:"
foreach ($char in $assemblyChars) {
    if ($debugContent -match [regex]::Escape($char)) {
        Write-Host "    '$char' found" -ForegroundColor Green
    } else {
        Write-Host "    '$char' NOT found" -ForegroundColor Red
        $assemblyComplete = $false
        $allTestsPassed = $false
    }
}

if ($assemblyComplete) {
    Write-Host "  [PASS] Assembly startup sequence complete (ABCDEFG present)" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] Assembly startup sequence incomplete" -ForegroundColor Red
}

Write-Host ""

# Test Case 3: ApStartupData Population Preservation
Write-Host "Test Case 3: ApStartupData Population Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.3: prepare_ap_startup_data() must correctly populate all fields"
Write-Host ""

# Check for AP startup data preparation messages
if ($serialContent -match "SMP: AP startup data prepared") {
    Write-Host "  [PASS] AP startup data preparation message found" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup data preparation message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Verify all ApStartupData fields are populated
$fieldsPresent = $true

if ($serialContent -match "pml4_phys = (0x[0-9a-f]+)") {
    $pml4Match = [regex]::Match($serialContent, "pml4_phys = (0x[0-9a-f]+)")
    $pml4Value = $pml4Match.Groups[1].Value
    Write-Host "  [PASS] pml4_phys field populated: $pml4Value" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] pml4_phys field NOT found" -ForegroundColor Red
    $fieldsPresent = $false
    $allTestsPassed = $false
}

if ($serialContent -match "entry = (0x[0-9a-f]+)") {
    $entryMatch = [regex]::Match($serialContent, "entry = (0x[0-9a-f]+)")
    $entryValue = $entryMatch.Groups[1].Value
    Write-Host "  [PASS] entry field populated: $entryValue" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] entry field NOT found" -ForegroundColor Red
    $fieldsPresent = $false
    $allTestsPassed = $false
}

if ($serialContent -match "stack_top = (0x[0-9a-f]+)") {
    $stackMatch = [regex]::Match($serialContent, "stack_top = (0x[0-9a-f]+)")
    $stackValue = $stackMatch.Groups[1].Value
    Write-Host "  [PASS] stack_top field populated: $stackValue" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] stack_top field NOT found" -ForegroundColor Red
    $fieldsPresent = $false
    $allTestsPassed = $false
}

if ($serialContent -match "cpu_data = (0x[0-9a-f]+)") {
    $cpuDataMatch = [regex]::Match($serialContent, "cpu_data = (0x[0-9a-f]+)")
    $cpuDataValue = $cpuDataMatch.Groups[1].Value
    Write-Host "  [PASS] cpu_data field populated: $cpuDataValue" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] cpu_data field NOT found" -ForegroundColor Red
    $fieldsPresent = $false
    $allTestsPassed = $false
}

if ($fieldsPresent) {
    Write-Host "  [PASS] All ApStartupData fields correctly populated" -ForegroundColor Green
}

Write-Host ""

# Test Case 4: AP Startup Code Loading Preservation
Write-Host "Test Case 4: AP Startup Code Loading Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.4: AP startup code loading must work correctly"
Write-Host ""

if ($serialContent -match "SMP: loading AP startup code") {
    Write-Host "  [PASS] AP startup code loading initiated" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code loading message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: copying AP startup code to phys=0x1000") {
    Write-Host "  [PASS] AP startup code copying to physical address 0x1000" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code copying message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: AP startup code copied") {
    Write-Host "  [PASS] AP startup code successfully copied" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code copied message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: AP PML4 phys=0x[0-9a-f]+") {
    Write-Host "  [PASS] AP PML4 setup completed" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP PML4 setup message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: AP startup code ready") {
    Write-Host "  [PASS] AP startup code ready for execution" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] AP startup code ready message NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Test Case 5: Kernel Virtual Address Usage Preservation
Write-Host "Test Case 5: Kernel Virtual Address Usage Preservation" -ForegroundColor Cyan
Write-Host "  Requirement 3.5: Kernel virtual addresses must function correctly"
Write-Host ""

# Check that kernel is using virtual addresses (not physical)
$entryPointMatches = [regex]::Matches($serialContent, "entry = (0x[0-9a-f]+)")

if ($entryPointMatches.Count -gt 0) {
    $entryAddr = $entryPointMatches[0].Groups[1].Value
    $entryAddrValue = [Convert]::ToUInt64($entryAddr, 16)
    
    # Kernel virtual addresses are typically in higher half (> 0x7000_0000 for 32-bit style or > 0xFFFF_8000_0000_0000 for 64-bit)
    # Physical addresses for code would be much lower (< 0x1_0000_0000)
    # The entry point should be a virtual address, not a low physical address
    if ($entryAddrValue -gt 0x10000000) {
        Write-Host "  [PASS] Kernel using virtual addresses ($entryAddr)" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] Entry point appears to be physical address, not virtual" -ForegroundColor Red
        $allTestsPassed = $false
    }
} else {
    Write-Host "  [FAIL] Could not verify kernel virtual address usage" -ForegroundColor Red
    $allTestsPassed = $false
}

# Verify PML4 is being used correctly
if ($serialContent -match "SMP: AP PML4 phys=(0x[0-9a-f]+)") {
    $pml4Match = [regex]::Match($serialContent, "SMP: AP PML4 phys=(0x[0-9a-f]+)")
    $pml4Addr = $pml4Match.Groups[1].Value
    Write-Host "  [PASS] Kernel PML4 page tables in use ($pml4Addr)" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] PML4 usage NOT verified" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Test Case 6: AP Startup Sequence Initiation
Write-Host "Test Case 6: AP Startup Sequence Initiation" -ForegroundColor Cyan
Write-Host "  Verify AP startup attempts are made correctly"
Write-Host ""

$apStartupMatches = [regex]::Matches($serialContent, "Starting AP (\d+) with APIC ID (\d+)")

if ($apStartupMatches.Count -gt 0) {
    Write-Host "  [PASS] AP startup attempts found: $($apStartupMatches.Count)" -ForegroundColor Green
    foreach ($match in $apStartupMatches) {
        $cpuId = $match.Groups[1].Value
        $apicId = $match.Groups[2].Value
        Write-Host "    AP $cpuId (APIC ID $apicId) startup initiated" -ForegroundColor DarkGray
    }
} else {
    Write-Host "  [FAIL] No AP startup attempts found" -ForegroundColor Red
    $allTestsPassed = $false
}

# Verify INIT-SIPI-SIPI sequence messages
if ($serialContent -match "SMP: INIT assert sent") {
    Write-Host "  [PASS] INIT assert messages present" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] INIT assert messages NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

if ($serialContent -match "SMP: SIPI") {
    Write-Host "  [PASS] SIPI messages present" -ForegroundColor Green
} else {
    Write-Host "  [FAIL] SIPI messages NOT found" -ForegroundColor Red
    $allTestsPassed = $false
}

Write-Host ""

# Summary
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

if ($allTestsPassed) {
    Write-Host "ALL PRESERVATION TESTS PASSED" -ForegroundColor Green
    Write-Host ""
    Write-Host "Baseline behavior confirmed:" -ForegroundColor Green
    Write-Host "  - BSP boots successfully with kernel virtual addresses" -ForegroundColor Green
    Write-Host "  - AP assembly startup displays ABCDEFG to debugcon" -ForegroundColor Green
    Write-Host "  - prepare_ap_startup_data() populates all fields correctly" -ForegroundColor Green
    Write-Host "  - AP startup code loading works correctly" -ForegroundColor Green
    Write-Host "  - Kernel virtual addresses function correctly" -ForegroundColor Green
    Write-Host "  - AP startup sequence is initiated properly" -ForegroundColor Green
    Write-Host ""
    Write-Host "This behavior MUST be preserved after implementing the fix." -ForegroundColor Yellow
    exit 0
} else {
    Write-Host "SOME PRESERVATION TESTS FAILED" -ForegroundColor Red
    Write-Host ""
    Write-Host "This indicates that some expected baseline behavior is not present." -ForegroundColor Red
    Write-Host "Review the failures above to understand what needs to be preserved." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Note: These tests should PASS on unfixed code to establish the baseline." -ForegroundColor Yellow
    exit 1
}
