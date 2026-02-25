# Bug Condition Exploration Test for AP Entry Point Mapping
# **Validates: Requirements 2.1, 2.2, 2.3, 2.4**
#
# This test MUST FAIL on unfixed code - failure confirms the bug exists
# DO NOT attempt to fix the test or the code when it fails
#
# Property 1: Fault Condition - AP Entry Point Execution Hang
# For cpu_count=4 (BSP + 3 APs), APs should execute ap_entry and print 'A' to COM1,
# but on unfixed code they hang at the entry point call without executing Rust code

Write-Host "=== Bug Condition Exploration Test ===" -ForegroundColor Cyan
Write-Host "Testing AP entry point execution with kernel virtual address" -ForegroundColor Cyan
Write-Host ""

Write-Host "IMPORTANT: This test is EXPECTED TO FAIL on unfixed code." -ForegroundColor Yellow
Write-Host "Failure confirms APs hang when calling kernel virtual address entry point." -ForegroundColor Yellow
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
$serialLogPath = Join-Path $logDir "serial_$timestamp.log"
$debugLogPath = Join-Path $logDir "debugcon_$timestamp.log"
$traceLogPath = Join-Path $logDir "qemu_trace_$timestamp.log"
$qemuStdoutPath = Join-Path $logDir "qemu_stdout_$timestamp.log"
$qemuStderrPath = Join-Path $logDir "qemu_stderr_$timestamp.log"

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
    "-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset",
    "-D", $traceLogPath,
    "-no-reboot",
    "-no-shutdown"
)

# Run QEMU with timeout (system will hang on unfixed code)
$timeoutSec = 30
$proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath

Write-Host "Waiting for QEMU to run (timeout: $timeoutSec seconds)..." -ForegroundColor Yellow
$completed = $proc.WaitForExit($timeoutSec * 1000)

if (-not $completed) {
    Write-Host "QEMU timeout - system appears to be hanging (expected on unfixed code)" -ForegroundColor Yellow
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

# Test 1: Verify entry point address is in kernel virtual range
Write-Host "Test 1: Entry Point Address Verification" -ForegroundColor Cyan
$entryPointMatches = [regex]::Matches($serialContent, "entry = (0x[0-9a-f]+)")

if ($entryPointMatches.Count -gt 0) {
    $entryAddr = $entryPointMatches[0].Groups[1].Value
    $entryAddrValue = [Convert]::ToUInt64($entryAddr, 16)
    $kernelVirtBase = 0x7000000000000000
    
    Write-Host "  Entry point address: $entryAddr"
    
    if ($entryAddrValue -ge $kernelVirtBase) {
        Write-Host "  CONFIRMED: Entry point is in kernel virtual address range (>= 0x7000000000000000)" -ForegroundColor Green
        $test1Confirmed = $true
    } else {
        Write-Host "  WARNING: Entry point is NOT in expected kernel virtual range" -ForegroundColor Yellow
        $test1Confirmed = $false
    }
} else {
    Write-Host "  ERROR: Could not find entry point address in log" -ForegroundColor Red
    $test1Confirmed = $false
}
Write-Host ""

# Test 2: Verify assembly startup completes (ABCDEFG on debugcon)
Write-Host "Test 2: Assembly Startup Completion" -ForegroundColor Cyan
$assemblyChars = @('A', 'B', 'C', 'D', 'E', 'F', 'G')
$assemblyComplete = $true

Write-Host "  Checking for assembly debug output (ABCDEFG) on debugcon port 0xE9:"
foreach ($char in $assemblyChars) {
    if ($debugContent -match [regex]::Escape($char)) {
        Write-Host "    '$char' found" -ForegroundColor Green
    } else {
        Write-Host "    '$char' NOT found" -ForegroundColor Red
        $assemblyComplete = $false
    }
}

if ($assemblyComplete) {
    Write-Host "  PASS: Assembly startup completed successfully (ABCDEFG present)" -ForegroundColor Green
} else {
    Write-Host "  FAIL: Assembly startup incomplete" -ForegroundColor Red
}
Write-Host ""

# Test 3: Verify ap_entry does NOT execute (no 'A' on COM1)
Write-Host "Test 3: AP Entry Point Execution Check" -ForegroundColor Cyan
Write-Host "  Checking for 'A' character on COM1 port 0x3f8 (indicates ap_entry executed):"

# Count 'A' characters in serial output (COM1)
# Note: We need to distinguish between assembly 'A' (debugcon) and ap_entry 'A' (COM1)
# The serial log is from COM1, debugcon log is from 0xE9
$apEntryAFound = $serialContent -match "A"

if ($apEntryAFound) {
    Write-Host "  'A' character found on COM1 - ap_entry executed" -ForegroundColor Green
    $test3Failed = $false
} else {
    Write-Host "  NO 'A' character on COM1 - ap_entry never executed" -ForegroundColor Red
    $test3Failed = $true
}
Write-Host ""

# Test 4: Check for timeout message (should NOT appear if system hangs)
Write-Host "Test 4: System Hang Detection" -ForegroundColor Cyan
$timeoutMessageFound = $serialContent -match "Timeout waiting for AP"

if ($timeoutMessageFound) {
    Write-Host "  Timeout message found - system reached wait_for_online timeout" -ForegroundColor Green
    $test4Hang = $false
} else {
    Write-Host "  NO timeout message - system appears to have hung before timeout" -ForegroundColor Red
    $test4Hang = $true
}
Write-Host ""

# Test 5: Check for AP startup attempts
Write-Host "Test 5: AP Startup Sequence" -ForegroundColor Cyan
$apStartupMatches = [regex]::Matches($serialContent, "Starting AP (\d+) with APIC ID (\d+)")

Write-Host "  AP startup attempts found: $($apStartupMatches.Count)"
foreach ($match in $apStartupMatches) {
    $cpuId = $match.Groups[1].Value
    $apicId = $match.Groups[2].Value
    Write-Host "    AP $cpuId (APIC ID $apicId) startup initiated"
}

if ($apStartupMatches.Count -gt 0) {
    Write-Host "  PASS: AP startup sequence initiated" -ForegroundColor Green
} else {
    Write-Host "  WARNING: No AP startup attempts found" -ForegroundColor Yellow
}
Write-Host ""

# Summary
Write-Host "=== Test Summary ===" -ForegroundColor Cyan
Write-Host ""

# Bug is present if:
# 1. Entry point is in kernel virtual range (>= 0x7000000000000000)
# 2. Assembly startup completes (ABCDEFG present)
# 3. ap_entry does NOT execute (no 'A' on COM1)
# 4. System hangs (no timeout message)

$bugPresent = $test1Confirmed -and $assemblyComplete -and $test3Failed -and $test4Hang

if ($bugPresent) {
    Write-Host "BUG CONFIRMED - Test FAILED as expected on unfixed code" -ForegroundColor Red
    Write-Host ""
    Write-Host "Counterexamples found:" -ForegroundColor Yellow
    Write-Host "  - Entry point address is kernel virtual address: $entryAddr" -ForegroundColor Yellow
    Write-Host "  - Assembly startup completed successfully (ABCDEFG printed to debugcon)" -ForegroundColor Yellow
    Write-Host "  - ap_entry function never executed (no 'A' on COM1 port 0x3f8)" -ForegroundColor Yellow
    Write-Host "  - System hung without reaching wait_for_online timeout" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Root Cause Analysis:" -ForegroundColor Yellow
    Write-Host "  The AP completes assembly startup and enables paging with kernel PML4," -ForegroundColor Yellow
    Write-Host "  but when it attempts to call the entry point at kernel virtual address" -ForegroundColor Yellow
    Write-Host "  $entryAddr, the instruction fetch fails because the virtual address" -ForegroundColor Yellow
    Write-Host "  is not properly mapped or accessible in the AP's execution context." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "This confirms the bug exists: APs hang at entry point call instruction." -ForegroundColor Yellow
    exit 1
} else {
    Write-Host "BUG NOT DETECTED - Test PASSED (unexpected on unfixed code)" -ForegroundColor Green
    Write-Host ""
    Write-Host "Analysis:" -ForegroundColor Cyan
    
    if (-not $test1Confirmed) {
        Write-Host "  - Entry point address is NOT in kernel virtual range" -ForegroundColor Cyan
    }
    
    if (-not $assemblyComplete) {
        Write-Host "  - Assembly startup did not complete" -ForegroundColor Cyan
    }
    
    if (-not $test3Failed) {
        Write-Host "  - ap_entry executed successfully (bug may be fixed)" -ForegroundColor Cyan
    }
    
    if (-not $test4Hang) {
        Write-Host "  - System reached timeout (no hang detected)" -ForegroundColor Cyan
    }
    
    Write-Host ""
    Write-Host "The bug may already be fixed, or the test conditions are not met." -ForegroundColor Green
    exit 0
}
