[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

# Wave 5 production boot scope.  The pipeline unit tests intentionally use
# unwrap/expect to construct invalid-state fixtures; they are excluded after
# the module's #[cfg(test)] boundary and are not kernel execution paths.
$targets = @(
    'src\main.rs',
    'src\boot\appliance.rs',
    'src\boot\error_policy.rs',
    'src\boot\pipeline.rs',
    'src\boot\safety.rs',
    'src\boot\context.rs',
    'src\vdso.rs',
    'src\debug\debugcon.rs',
    'src\debug\serial.rs',
    'src\allocator\mod.rs',
    'src\interrupts\mod.rs'
)

$violations = New-Object System.Collections.Generic.List[string]
foreach ($path in $targets) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Wave5 policy target missing: $path"
    }

    $inTests = $false
    $lineNumber = 0
    foreach ($line in (Get-Content -LiteralPath $path)) {
        $lineNumber++
        if ($line -match '^\s*#\[cfg\(test\)\]') {
            $inTests = $true
        }
        if (-not $inTests -and $line -match '\blet\s+_\s*=|\bexpect\s*\(|\bunwrap\s*\(') {
            $violations.Add("${path}:${lineNumber}: forbidden silent result/expect/unwrap")
        }
    }
}

$policySources = ($targets | ForEach-Object { Get-Content -Raw -LiteralPath $_ }) -join "`n"
$requiredMarkers = @(
    'IommuPolicy::from_cmdline',
    'BootErrorDisposition::Fatal',
    'ViolationType::IommuUnavailable',
    'ViolationType::BootPolicy',
    'ViolationType::CapabilityUnavailable',
    'VdsoInitError',
    'phase_degraded',
    'set_required_capabilities_or_fatal'
)
foreach ($marker in $requiredMarkers) {
    if ($policySources -notmatch [regex]::Escape($marker)) {
        $violations.Add("Wave5 boot scope: missing explicit policy marker '$marker'")
    }
}

if ($violations.Count -ne 0) {
    $violations | ForEach-Object { Write-Error $_ }
    exit 1
}

Write-Output "Wave5 boot error policy PASS: $($targets.Count) production files checked; no silent let _/expect/unwrap paths."
