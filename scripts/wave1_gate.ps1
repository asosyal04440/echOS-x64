param(
    [switch]$RebuildHarness,
    [switch]$VerboseWarnings,
    [switch]$Full
)

$ErrorActionPreference = "Stop"
$previousRustFlags = $env:RUSTFLAGS
if (-not $VerboseWarnings) {
    $joinedFlags = @($env:RUSTFLAGS, "-Awarnings") | Where-Object { $_ -and $_.Trim().Length -gt 0 }
    $env:RUSTFLAGS = ($joinedFlags -join " ").Trim()
}

function Write-GateStatus {
    param([string]$Message)
    $timestamp = Get-Date -Format "HH:mm:ss"
    Write-Host ("[wave1_gate {0}] {1}" -f $timestamp, $Message)
}

function Invoke-Step {
    param(
        [string]$Label,
        [scriptblock]$Command
    )

    Write-GateStatus $Label
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw ("wave1_gate step failed: {0} (exit={1})" -f $Label, $LASTEXITCODE)
    }
}

function Resolve-LibTestHarness {
    param(
        [switch]$AllowMissing
    )

    $depsDir = Join-Path $PSScriptRoot "..\target\x86_64-pc-windows-msvc\debug\deps"
    $candidate = Get-ChildItem $depsDir -Filter "ech_os-*.exe" |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if (-not $candidate) {
        if ($AllowMissing) {
            return $null
        }
        throw "Wave 1 gate could not find the ech_os lib test harness under $depsDir"
    }
    return $candidate.FullName
}

function Get-NewestWave1InputTimestamp {
    $roots = @(
        (Join-Path $PSScriptRoot "..\src"),
        (Join-Path $PSScriptRoot "..\tests")
    )
    $trackedFiles = @(
        (Join-Path $PSScriptRoot "..\Cargo.toml"),
        (Join-Path $PSScriptRoot "..\Cargo.lock"),
        (Join-Path $PSScriptRoot "..\docs\architecture\arch_rules.toml")
    ) | Where-Object { Test-Path $_ }

    $timestamps = New-Object System.Collections.Generic.List[datetime]
    foreach ($root in $roots) {
        if (Test-Path $root) {
            Get-ChildItem $root -Recurse -File | ForEach-Object {
                $timestamps.Add($_.LastWriteTimeUtc)
            }
        }
    }
    foreach ($file in $trackedFiles) {
        $timestamps.Add((Get-Item $file).LastWriteTimeUtc)
    }
    if ($timestamps.Count -eq 0) {
        return [datetime]::MinValue
    }
    return ($timestamps | Sort-Object -Descending | Select-Object -First 1)
}

function Test-HarnessStale {
    param([string]$HarnessPath)

    if (-not (Test-Path $HarnessPath)) {
        return $true
    }

    $harnessTime = (Get-Item $HarnessPath).LastWriteTimeUtc
    $newestInput = Get-NewestWave1InputTimestamp
    return $newestInput -gt $harnessTime
}

function Ensure-LibTestHarness {
    if ($RebuildHarness) {
        Write-GateStatus "lib test harness rebuild forced; cargo test --no-run can take several minutes after source edits"
        Invoke-Step "compile lib test harness once" { cargo test --target x86_64-pc-windows-msvc --lib --no-run -q }
        return Resolve-LibTestHarness
    }

    $existing = Resolve-LibTestHarness -AllowMissing
    if ($existing) {
        if (-not (Test-HarnessStale -HarnessPath $existing)) {
            Write-GateStatus ("reusing existing lib test harness {0}" -f $existing)
            return $existing
        }
        Write-GateStatus ("existing lib test harness is stale; recompiling {0}" -f $existing)
    }

    Write-GateStatus "compiling lib test harness once; this is the slowest Wave 1 gate step after source edits"
    Invoke-Step "compile lib test harness once" { cargo test --target x86_64-pc-windows-msvc --lib --no-run -q }
    return Resolve-LibTestHarness
}

$tests = @(
    "runtime::tests::register_launch_session_issues_broker_ticket_and_capability_token",
    "runtime::tests::headless_service_session_uses_native_headless_runtime",
    "ipc::service_ipc::tests::bootstrap_fallback_still_returns_direct_response_before_runtime_task",
    "ipc::service_ipc::tests::strict_mode_marks_migrated_service_compat_usage_as_violation",
    "ipc::service_ipc::tests::process_broker_service_describes_registered_launch_and_children",
    "pe_loader::tests::spawn_process_contract_populates_win32_bootstrap_bundle",
    "pe_loader::tests::thread_bootstrap_reuses_process_peb_and_process_parameters",
    "pe_loader::tests::win32_teb_keeps_peb_pointer_at_gs_0x60_contract"
)

try {
    Invoke-Step "arch_guard report" { cargo arch-guard --report }
    Invoke-Step "arch_guard check" { cargo arch-guard --check }
    Invoke-Step "host lib check" { cargo check --target x86_64-pc-windows-msvc --lib -q }
    Invoke-Step "uefi check" { cargo check --target x86_64-unknown-uefi --features simics -q }
    if ($Full) {
        Invoke-Step "uefi build" { cargo build --target x86_64-unknown-uefi -q }
    }

    $testHarness = Ensure-LibTestHarness
    foreach ($testName in $tests) {
        Invoke-Step ("test {0}" -f $testName) { & $testHarness $testName "--exact" "--test-threads" "1" }
    }
}
finally {
    if ($null -ne $previousRustFlags) {
        $env:RUSTFLAGS = $previousRustFlags
    } else {
        Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    }
}
