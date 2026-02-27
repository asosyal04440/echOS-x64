<#
.SYNOPSIS
    echOS Zero-Tolerance Lock-Free Gate — Atomic Execution Pipeline.
.DESCRIPTION
    Boots echOS in Intel Simics batch mode, captures serial output to a file,
    parses kernel markers across 5 test axes, and issues a JSON verdict.
    Boot + lock-free + fuzzing tests merged into a single lethal barrier.
    EXIT 0 = merge allowed, EXIT 2 = merge HARD-BLOCKED.
.PARAMETER Mode
    Gate enforcement mode (only day1-hard-block is supported).
.PARAMETER TimeoutSec
    Wall-clock timeout in seconds. Default: 600 (10 min).
    Without VMP, UEFI boot + OS init may take several minutes in software emulation.
#>
param(
    [ValidateSet("day1-hard-block")]
    [string]$Mode = "day1-hard-block",
    [int]$TimeoutSec = 600
)

$ErrorActionPreference = "Continue"
Set-StrictMode -Version Latest
$GateTier = "aggressive"
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptRoot
$targetScript = "targets/echos/zero-tolerance-gate.simics"
$simicsBat = Join-Path $projectRoot "simics.bat"
$logDir = Join-Path $projectRoot "targets\echos\logs"
if (-not (Test-Path $logDir)) {
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
}

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$runLog = Join-Path $logDir "gate_run_$stamp.log"
$verdictJson = Join-Path $logDir "gate_verdict_$stamp.json"
$simicsStdout = Join-Path $logDir "gate_simics_stdout_$stamp.log"
$simicsStderr = Join-Path $logDir "gate_simics_stderr_$stamp.log"
$serialCapture = Join-Path $logDir "serial_capture.txt"

Write-Host ""
Write-Host "================================================================" -ForegroundColor Cyan
Write-Host "  echOS Zero-Tolerance Lock-Free Gate" -ForegroundColor Cyan
Write-Host "  profile=$GateTier  mode=$Mode  timeout=${TimeoutSec}s" -ForegroundColor Cyan
Write-Host "================================================================" -ForegroundColor Cyan
Write-Host "[GATE] script   = $targetScript" -ForegroundColor DarkGray
Write-Host "[GATE] run_log  = $runLog" -ForegroundColor DarkGray
Write-Host "[GATE] serial   = $serialCapture" -ForegroundColor DarkGray
Write-Host ""

# ── Validate launcher ─────────────────────────────────────────
if (-not (Test-Path $simicsBat)) {
    Write-Error "[GATE] FATAL: Simics launcher not found: $simicsBat"
    exit 2
}

# ── Resolve Simics-bundled Python ──────────────────────────────
if (-not $env:SIMICS_PYTHON -or $env:SIMICS_PYTHON -eq "") {
    $simicsMiniPython = "D:\Intel Simics Package Manager\simics-python-7.14.0\win64\bin\mini-python.exe"
    if (Test-Path $simicsMiniPython) {
        $env:SIMICS_PYTHON = $simicsMiniPython
        Write-Host "[GATE] SIMICS_PYTHON = $simicsMiniPython" -ForegroundColor DarkGray
    } else {
        Write-Host "[GATE] WARNING: Simics mini-python not found. Using system Python." -ForegroundColor Yellow
    }
}

# ── Clean previous serial capture ─────────────────────────────
if (Test-Path $serialCapture) {
    Remove-Item $serialCapture -Force
}

# ── Launch Simics ──────────────────────────────────────────────
Write-Host "[GATE] Launching Simics (batch mode)..." -ForegroundColor White
Write-Host "[GATE] Wall-clock timeout: ${TimeoutSec}s" -ForegroundColor DarkGray

$proc = Start-Process `
    -FilePath $simicsBat `
    -WorkingDirectory $projectRoot `
    -ArgumentList @("--batch-mode", $targetScript) `
    -PassThru -NoNewWindow `
    -RedirectStandardOutput $simicsStdout `
    -RedirectStandardError $simicsStderr

$timedOut = $false
try {
    if (-not $proc.WaitForExit($TimeoutSec * 1000)) {
        $timedOut = $true
        Write-Host "[GATE] TIMEOUT: Simics exceeded ${TimeoutSec}s wall-clock. Killing." -ForegroundColor Red
        try { $proc.Kill() } catch {}
        Start-Sleep -Seconds 3
        try { $proc.WaitForExit(10000) } catch {}
    }
} catch {
    Write-Host "[GATE] WaitForExit exception: $($_.Exception.Message)" -ForegroundColor Yellow
    $timedOut = $true
}

$simicsExit = -1
try {
    if ($proc.HasExited) { $simicsExit = $proc.ExitCode }
} catch {
    Write-Host "[GATE] Could not read exit code: $($_.Exception.Message)" -ForegroundColor Yellow
}
Write-Host "[GATE] Simics exit code: $simicsExit" -ForegroundColor DarkGray

# Allow file handles to close after process exit
Start-Sleep -Seconds 2

# ── Collect all output sources ─────────────────────────────────
# Primary: serial capture file (written by Simics capture-start command)
# Secondary: Simics stdout/stderr (gate envelope markers, diagnostics)

$serialText = ""
try {
    if (Test-Path $serialCapture) {
        $serialText = Get-Content $serialCapture -Raw -ErrorAction SilentlyContinue
        if (-not $serialText) { $serialText = "" }
        Write-Host "[GATE] Serial capture: $($serialText.Length) bytes" -ForegroundColor DarkGray
    } else {
        Write-Host "[GATE] WARNING: Serial capture file not found! Kernel may not have booted." -ForegroundColor Yellow
    }
} catch {
    Write-Host "[GATE] WARNING: Error reading serial capture: $($_.Exception.Message)" -ForegroundColor Yellow
}

$stdoutText = ""
$stderrText = ""
try {
    if (Test-Path $simicsStdout) { $stdoutText = Get-Content $simicsStdout -Raw -ErrorAction SilentlyContinue }
    if (-not $stdoutText) { $stdoutText = "" }
    if (Test-Path $simicsStderr) { $stderrText = Get-Content $simicsStderr -Raw -ErrorAction SilentlyContinue }
    if (-not $stderrText) { $stderrText = "" }
} catch {
    Write-Host "[GATE] WARNING: Error reading stdout/stderr: $($_.Exception.Message)" -ForegroundColor Yellow
}

# Merge all sources for comprehensive analysis
$allOutput = "$serialText`n$stdoutText`n$stderrText"
if ($timedOut) {
    $allOutput += "`n[GATE] TIMEOUT reached (runner wall-clock)"
}

# Save composite run log
$allOutput | Out-File -FilePath $runLog -Encoding UTF8

# ── Test-Axis function ─────────────────────────────────────────
function Test-Axis {
    param(
        [string]$Axis,
        [string[]]$Required,
        [string[]]$Forbidden
    )

    $missing = @()
    foreach ($token in $Required) {
        if (-not ($allOutput -match [regex]::Escape($token))) {
            $missing += $token
        }
    }

    $hits = @()
    foreach ($token in $Forbidden) {
        if ($allOutput -match [regex]::Escape($token)) {
            $hits += $token
        }
    }

    [pscustomobject]@{
        axis = $Axis
        passed = (($missing.Count -eq 0) -and ($hits.Count -eq 0))
        missing = $missing
        forbidden_hits = $hits
    }
}

# ── Execute all 5 axes ─────────────────────────────────────────
$results = @(
    (Test-Axis -Axis "boot_irq_input" -Required @(
        "[INT] Interrupts enabled",
        "[BOOT] Starting GUI compositor...",
        "Mouse:",
        "[COMPOSITOR]"
    ) -Forbidden @("[PANIC]", "deadlock", "spinlock stuck")),

    (Test-Axis -Axis "syscall_security" -Required @(
        "[WINSRV]",
        "ownership check",
        "user-range validation"
    ) -Forbidden @(
        "kernel pointer leak",
        "invalid user pointer accepted"
    )),

    (Test-Axis -Axis "fs_network" -Required @(
        "[NET]"
    ) -Forbidden @("fs corruption", "network deadlock")),

    (Test-Axis -Axis "performance" -Required @(
        "[PERF]",
        "latency probes"
    ) -Forbidden @("latency regression", "frame pacing violation")),

    (Test-Axis -Axis "extreme_ironshim" -Required @(
        "[IRONSHIM]",
        "fuzz guard",
        "ring3->ring0 blocked"
    ) -Forbidden @(
        "vm escape success",
        "ring0 overwrite",
        "oom queue collapse"
    ))
)

# ── Verdict ────────────────────────────────────────────────────
$failed = @($results | Where-Object { -not $_.passed })
$summary = [pscustomobject]@{
    profile        = $GateTier
    mode           = $Mode
    timestamp      = (Get-Date).ToString("o")
    run_log        = $runLog
    serial_capture = $serialCapture
    simics_stdout  = $simicsStdout
    simics_stderr  = $simicsStderr
    timed_out      = $timedOut
    simics_exit    = $simicsExit
    serial_bytes   = $serialText.Length
    total_axes     = $results.Count
    passed_axes    = @($results | Where-Object { $_.passed }).Count
    failed_axes    = $failed.Count
    block_merge    = ($failed.Count -gt 0)
    results        = $results
}

$summary | ConvertTo-Json -Depth 8 | Out-File -FilePath $verdictJson -Encoding UTF8
Write-Host ""
Write-Host "[GATE] verdict: $verdictJson" -ForegroundColor DarkGray

# ── Print results ──────────────────────────────────────────────
Write-Host ""
foreach ($r in $results) {
    if ($r.passed) {
        Write-Host "[GATE][PASS] $($r.axis)" -ForegroundColor Green
    } else {
        Write-Host "[GATE][FAIL] $($r.axis)" -ForegroundColor Red
        if ($r.missing.Count -gt 0) {
            Write-Host ("  missing   : " + ($r.missing -join ", ")) -ForegroundColor Yellow
        }
        if ($r.forbidden_hits.Count -gt 0) {
            Write-Host ("  forbidden : " + ($r.forbidden_hits -join ", ")) -ForegroundColor Yellow
        }
    }
}

Write-Host ""
if ($summary.block_merge) {
    Write-Host "================================================================" -ForegroundColor Red
    Write-Host "  VERDICT: HARD BLOCK - merge denied ($($failed.Count)/$($results.Count) axes failed)" -ForegroundColor Red
    Write-Host "================================================================" -ForegroundColor Red
    exit 2
}

Write-Host "================================================================" -ForegroundColor Green
Write-Host "  VERDICT: PASS - merge allowed ($($results.Count)/$($results.Count) axes passed)" -ForegroundColor Green
Write-Host "================================================================" -ForegroundColor Green
exit 0
