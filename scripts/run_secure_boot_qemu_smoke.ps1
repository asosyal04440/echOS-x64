param(
    [ValidateSet("prepare", "verify", "auto")]
    [string]$Phase = "prepare",

    [string]$BundleDir = ".\artifacts\secure_boot",

    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "release",

    [ValidateSet("fast", "debug")]
    [string]$QemuProfile = "fast",

    [string]$SignedImage = ".\artifacts\secure_boot\BOOTX64.EFI",
    [string]$VarsPath = ".\build\appliance\OVMF_VARS.secboot.fd",
    [string]$KeyToolPath = "",
    [switch]$ForceBundleRegenerate,
    [switch]$ForceResign,
    [switch]$ForceVarsReset,
    [switch]$Headless,
    [string]$TpmStatePath = ".\build\appliance\tpm2-secure",
    [string]$SwtpmPath = "",
    [int]$TpmServerPort = 2321,
    [int]$TpmControlPort = 2322,
    [int]$DisplayWidth = 1920,
    [int]$DisplayHeight = 1080
)

$ErrorActionPreference = "Stop"

function Resolve-OptionalPath {
    param([string]$PathValue, [string]$Label)
    if (-not $PathValue) {
        return $null
    }
    $resolved = Resolve-Path -LiteralPath $PathValue -ErrorAction SilentlyContinue
    if (-not $resolved) {
        throw "$Label not found: $PathValue"
    }
    $resolved.Path
}

function Resolve-SecureOvmfCode {
    $candidates = @(
        "C:\Program Files\qemu\share\edk2-x86_64-secure-code.fd",
        "C:\Program Files\qemu\share\OVMF_CODE.secboot.fd",
        "C:\Program Files\qemu\share\OVMF_CODE.secure.fd"
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw "Secure OVMF code image not found. Expected one of: $($candidates -join ', ')"
}

function Resolve-OvmfVarsTemplate {
    $candidates = @(
        "C:\Program Files\qemu\share\edk2-i386-vars.fd",
        "C:\Program Files\qemu\share\OVMF_VARS.fd"
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw "OVMF vars template not found in QEMU share."
}

function ConvertTo-WslPath {
    param([Parameter(Mandatory = $true)][string]$PathValue)
    $converted = & wsl.exe -e wslpath -a -u -- $PathValue 2>&1
    if ($LASTEXITCODE -ne 0 -or -not $converted) {
        throw "WSL path conversion failed: $PathValue"
    }
    ($converted | Select-Object -Last 1).ToString().Trim()
}

function Initialize-SecureVarsStore {
    param(
        [Parameter(Mandatory = $true)][string]$VarsPathValue,
        [Parameter(Mandatory = $true)][string]$VarsTemplateValue,
        [Parameter(Mandatory = $true)][string]$BundlePathValue,
        [switch]$Reset
    )

    $flashTool = & wsl.exe -e sh -lc "command -v flash-var" 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $flashTool) {
        throw "WSL efitools flash-var is required for deterministic Secure Boot vars preparation."
    }

    $varsParent = Split-Path -Parent $VarsPathValue
    if ($varsParent -and -not (Test-Path -LiteralPath $varsParent)) {
        New-Item -ItemType Directory -Force -Path $varsParent | Out-Null
    }
    if ($Reset -or -not (Test-Path -LiteralPath $VarsPathValue)) {
        Copy-Item -LiteralPath $VarsTemplateValue -Destination $VarsPathValue -Force
    }

    $manifestPath = Join-Path $BundlePathValue "secure_boot_manifest.json"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        throw "Secure Boot manifest not found: $manifestPath"
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if (-not $manifest.owner_guid) {
        throw "Secure Boot manifest owner_guid is missing: $manifestPath"
    }

    $varsWsl = ConvertTo-WslPath $VarsPathValue
    $globalGuid = "8be4df61-93ca-11d2-aa0d-00e098032b8c"
    $databaseGuid = "d719b2cb-3d3a-4596-a3bc-dad00e67656f"
    $entries = @(
        @{ Name = "PK"; Guid = $globalGuid; File = "PK.esl" },
        @{ Name = "KEK"; Guid = $globalGuid; File = "KEK.esl" },
        @{ Name = "db"; Guid = $databaseGuid; File = "db.esl" },
        @{ Name = "dbx"; Guid = $databaseGuid; File = "dbx.esl" }
    )
    foreach ($entry in $entries) {
        $contentPath = Join-Path $BundlePathValue $entry.File
        if (-not (Test-Path -LiteralPath $contentPath)) {
            throw "Secure Boot signature list not found: $contentPath"
        }
        $contentWsl = ConvertTo-WslPath $contentPath
        & wsl.exe -e flash-var -g $entry.Guid $varsWsl $entry.Name $contentWsl
        if ($LASTEXITCODE -ne 0) {
            throw "flash-var failed for $($entry.Name) (exit $LASTEXITCODE)."
        }
    }
    Write-Host "Secure vars prepared: PK/KEK/db/dbx enrolled in disposable OVMF store." -ForegroundColor Green
}

function Find-LatestSerialLog {
    param([datetime]$StartedAt)
    $logDir = Join-Path $PWD "logs"
    if (-not (Test-Path -LiteralPath $logDir)) {
        return $null
    }
    Get-ChildItem -LiteralPath $logDir -Filter "serial_*.log" -File |
        Where-Object { $_.LastWriteTime -ge $StartedAt.AddSeconds(-2) } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}

function Assert-LogMarkers {
    param(
        [string]$LogPath,
        [string[]]$RequiredMarkers,
        [string[]]$ForbiddenMarkers = @()
    )
    $content = Get-Content -LiteralPath $LogPath -Raw
    $missing = @()
    foreach ($marker in $RequiredMarkers) {
        if (-not $content.Contains($marker)) {
            $missing += $marker
        }
    }
    if ($missing.Count -gt 0) {
        throw "Secure Boot verify markers missing in ${LogPath}: $($missing -join ', ')"
    }
    $seenForbidden = @()
    foreach ($marker in $ForbiddenMarkers) {
        if ($content.Contains($marker)) {
            $seenForbidden += $marker
        }
    }
    if ($seenForbidden.Count -gt 0) {
        throw "Secure Boot verify log contains forbidden markers in ${LogPath}: $($seenForbidden -join ', ')"
    }
}

function Assert-TrustedBootInvocation {
    param(
        [Parameter(Mandatory = $true)][datetime]$StartedAt,
        [Parameter(Mandatory = $true)][string]$PhaseLabel
    )

    if ($LASTEXITCODE -eq 0) {
        return
    }

    # run_qemu deliberately stops immediately after the firmware/TPM markers
    # are observed.  Depending on whether the child is killed before or after
    # WaitForExit, PowerShell can still expose a non-zero child status.  Accept
    # that bounded stop only when the trusted markers and the fatal-marker
    # deny-list prove that the requested phase completed.
    $boundedLog = Find-LatestSerialLog -StartedAt $StartedAt
    if (-not $boundedLog) {
        throw "Secure Boot $PhaseLabel failed (exit $LASTEXITCODE) and produced no serial log."
    }
    Assert-LogMarkers -LogPath $boundedLog.FullName -RequiredMarkers @(
        "[UEFI] Runtime services verified",
        "[UEFI] Secure Boot databases available",
        "[UEFI] Loaded image signature OK",
        "[TPM] Measure OK (PCR4)",
        "[TPM] Event log entries="
    ) -ForbiddenMarkers @(
        "[UEFI] Secure Boot databases unavailable",
        "[UEFI] Loaded image signature failed",
        "[TPM] TCG2 protocol not found",
        "[TPM] TPM not present",
        "[TPM] Measure failed",
        "[TPM] Cmdline measure failed",
        "[TPM] Event log failed",
        "[PANIC]",
        "PAGE_FAULT",
        "DOUBLE_FAULT",
        "TRIPLE_FAULT"
    )
    Write-Host "Secure Boot $PhaseLabel reached trusted markers before bounded QEMU stop (exit $LASTEXITCODE accepted)." -ForegroundColor DarkYellow
}

if (($Phase -eq "prepare") -and $Headless) {
    throw "prepare fazi firmware UI gerektirir; Headless kullanma."
}

$bundlePath = [System.IO.Path]::GetFullPath($BundleDir)
$signedImagePath = [System.IO.Path]::GetFullPath($SignedImage)
$varsStorePath = [System.IO.Path]::GetFullPath($VarsPath)
$secureCuratedBundleDir = Join-Path $bundlePath "qemu_empty_curated_bundles"
New-Item -ItemType Directory -Force -Path $secureCuratedBundleDir | Out-Null
$secureOvmfCode = Resolve-SecureOvmfCode
$ovmfVarsTemplate = Resolve-OvmfVarsTemplate
$keyToolResolved = Resolve-OptionalPath $KeyToolPath "KeyTool EFI"

if ($ForceBundleRegenerate -or -not (Test-Path -LiteralPath (Join-Path $bundlePath "db.crt"))) {
    & (Join-Path $PSScriptRoot "generate_secure_boot_bundle.ps1") -OutputDir $bundlePath
    if ($LASTEXITCODE -ne 0) {
        throw "Secure Boot bundle generation failed."
    }
}

if ($ForceResign -or -not (Test-Path -LiteralPath $signedImagePath)) {
    & (Join-Path $PSScriptRoot "build_signed_uefi.ps1") `
        -BundleDir $bundlePath `
        -Profile $BuildProfile `
        -SignedImage $signedImagePath
    if ($LASTEXITCODE -ne 0) {
        throw "Signed EFI build failed."
    }
}

$espPayloadDir = Join-Path $bundlePath "qemu_esp_payload"
New-Item -ItemType Directory -Force -Path $espPayloadDir | Out-Null

$readmeText = @"
echOS Secure Boot QEMU payload

FAT 8.3 aliases on ESP:
- PK.AUT  => PK.auth
- KEK.AUT => KEK.auth
- DB.AUT  => db.auth
- DBX.AUT => dbx.auth
- DB.SET  => db-setup.auth (Setup Mode first-write path)
- DBX.SET => dbx-setup.auth (Setup Mode first-write path)

Auto-enroll trigger:
- SBENROLL.ON => guest-side authenticated SetVariable apply

Prepare phase:
1. Put OVMF into Custom/Setup mode.
2. Boot signed BOOTX64.EFI with this payload attached.
3. echOS enrolls PK/KEK/db/dbx from authenticated replacement payloads in
   Setup Mode; later User Mode db/dbx updates remain signed append updates.
4. echOS requests a warm reset with the same vars store.
5. Run verify phase headless.
"@
$readmePath = Join-Path $espPayloadDir "SBREADME.TXT"
$readmeText | Set-Content -Encoding Ascii -LiteralPath $readmePath

$startupText = @"
echo echOS Secure Boot payload mounted.
echo SBENROLL.ON is armed for guest-side authenticated variable enroll.
echo Use scripts\run_secure_boot_qemu_smoke.ps1 -Phase verify after prepare reset completes.
"@
$startupPath = Join-Path $espPayloadDir "STARTUP.NSH"
$startupText | Set-Content -Encoding Ascii -LiteralPath $startupPath
$triggerPath = Join-Path $espPayloadDir "SBENROLL.ON"
"AUTOENROLL=1" | Set-Content -Encoding Ascii -LiteralPath $triggerPath

$guestFileSpecs = @(
    @{ Host = (Join-Path $bundlePath "PK.auth"); Guest = "PK.AUT" },
    @{ Host = (Join-Path $bundlePath "KEK.auth"); Guest = "KEK.AUT" },
    @{ Host = (Join-Path $bundlePath "db.auth"); Guest = "DB.AUT" },
    @{ Host = (Join-Path $bundlePath "dbx.auth"); Guest = "DBX.AUT" },
    @{ Host = (Join-Path $bundlePath "db-setup.auth"); Guest = "DB.SET" },
    @{ Host = (Join-Path $bundlePath "dbx-setup.auth"); Guest = "DBX.SET" },
    @{ Host = $triggerPath; Guest = "SBENROLL.ON" }
)

if ($keyToolResolved) {
    $guestFileSpecs += @{ Host = $keyToolResolved; Guest = "KEYTOOL.EFI" }
}

foreach ($spec in $guestFileSpecs) {
    if (-not (Test-Path -LiteralPath $spec.Host)) {
        throw "Secure Boot ESP payload source not found: $($spec.Host)"
    }
}

# `auto` is a disposable QEMU flow. Prepare the NVRAM store before firmware
# starts so OVMF enters User Mode deterministically; host firmware variables
# are never touched. Physical TPM/Secure Boot policy remains firmware-owned.
if ($Phase -eq "auto") {
    Initialize-SecureVarsStore `
        -VarsPathValue $varsStorePath `
        -VarsTemplateValue $ovmfVarsTemplate `
        -BundlePathValue $bundlePath `
        -Reset
}

$runQemuPath = Join-Path $PSScriptRoot "..\run_qemu.ps1"
$runParams = @{
    Mode = "uefi"
    Profile = $QemuProfile
    NoBuild = $true
    EfiPath = $signedImagePath
    OvmfCodePath = $secureOvmfCode
    OvmfVarsTemplatePath = $ovmfVarsTemplate
    OvmfVarsPath = $varsStorePath
    EnableTpm = $true
    TrustedBootSmoke = $true
    CpuCount = 2
    MemoryMiB = 512
    TpmStatePath = [System.IO.Path]::GetFullPath($TpmStatePath)
    TpmServerPort = $TpmServerPort
    TpmControlPort = $TpmControlPort
    DisplayWidth = $DisplayWidth
    DisplayHeight = $DisplayHeight
    CuratedBundleDir = $secureCuratedBundleDir
    EspExtraFile = @()
}
if ($SwtpmPath) {
    $runParams.SwtpmPath = $SwtpmPath
}
if ($ForceVarsReset) {
    $runParams.ForceVarsReset = $true
}
if ($Phase -eq "auto") {
    # The vars store was already flashed above; do not let run_qemu copy the
    # clean template over the enrolled store.
    $runParams.Remove("ForceVarsReset")
}
foreach ($spec in $guestFileSpecs) {
    $runParams.EspExtraFile += ("{0}::{1}" -f $spec.Host, $spec.Guest)
}

if (($Phase -eq "prepare") -or ($Phase -eq "auto")) {
    $runParams.NoAutoLogin = $true
    if ($Phase -eq "auto") {
        $runParams.Headless = $true
        Write-Host "Secure Boot automatic prepare phase" -ForegroundColor Cyan
    } else {
        Write-Host "Secure Boot prepare phase" -ForegroundColor Cyan
    }
    Write-Host "OVMF code : $secureOvmfCode" -ForegroundColor DarkGray
    Write-Host "Vars store: $varsStorePath" -ForegroundColor DarkGray
    Write-Host "Signed EFI: $signedImagePath" -ForegroundColor DarkGray
    Write-Host "ESP payload aliases: PK.AUT, KEK.AUT, DB.AUT, DBX.AUT, DB.SET, DBX.SET, SBENROLL.ON" -ForegroundColor DarkGray
    Write-Host "Auto phase flashes PK/KEK/db/dbx into disposable OVMF vars; guest setup payloads remain attached for firmware-native flows." -ForegroundColor DarkGray
    if ($keyToolResolved) {
        Write-Host "KeyTool EFI attached as KEYTOOL.EFI" -ForegroundColor DarkGray
    }
    $prepareStartedAt = Get-Date
    & $runQemuPath @runParams
    Assert-TrustedBootInvocation -StartedAt $prepareStartedAt -PhaseLabel "prepare"
    if ($Phase -eq "prepare") {
        exit 0
    }
    Write-Host "Secure Boot enrollment reboot completed; continuing with headless verification." -ForegroundColor Green
}

$runParams.Headless = $true
$startedAt = Get-Date
Write-Host "Secure Boot verify phase" -ForegroundColor Cyan
Write-Host "Using vars store: $varsStorePath" -ForegroundColor DarkGray
& $runQemuPath @runParams
Assert-TrustedBootInvocation -StartedAt $startedAt -PhaseLabel "verify"

$latestSerial = Find-LatestSerialLog -StartedAt $startedAt
if (-not $latestSerial) {
    throw "Could not locate serial log for Secure Boot verify phase."
}

Assert-LogMarkers -LogPath $latestSerial.FullName -RequiredMarkers @(
    "[UEFI] Runtime services verified",
    "[UEFI] Secure Boot databases available",
    "[UEFI] Loaded image signature OK",
    "[TPM] Measure OK (PCR4)",
    "[TPM] Event log entries="
) -ForbiddenMarkers @(
    "[UEFI] Secure Boot databases unavailable",
    "[UEFI] Loaded image signature failed",
    "[TPM] TCG2 protocol not found",
    "[TPM] TPM not present",
    "[TPM] Measure failed",
    "[TPM] Cmdline measure failed",
    "[TPM] Event log failed"
)

$verifyContent = Get-Content -LiteralPath $latestSerial.FullName -Raw
if (-not $verifyContent.Contains("[TPM] Cmdline measure OK (PCR8)") -and
    -not $verifyContent.Contains("[TPM] Cmdline absent; PCR8 measure skipped")) {
    throw "Secure Boot verify log has neither a PCR8 cmdline measurement nor an explicit absent-cmdline record: $($latestSerial.FullName)"
}

Write-Host "Secure Boot verify log: $($latestSerial.FullName)" -ForegroundColor Green
$global:LASTEXITCODE = 0
exit 0
