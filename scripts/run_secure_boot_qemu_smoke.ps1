param(
    [ValidateSet("prepare", "verify")]
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

if (($Phase -eq "prepare") -and $Headless) {
    throw "prepare fazi firmware UI gerektirir; Headless kullanma."
}

$bundlePath = [System.IO.Path]::GetFullPath($BundleDir)
$signedImagePath = [System.IO.Path]::GetFullPath($SignedImage)
$varsStorePath = [System.IO.Path]::GetFullPath($VarsPath)
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

Auto-enroll trigger:
- SBENROLL.ON => guest-side authenticated SetVariable apply

Prepare phase:
1. Put OVMF into Custom/Setup mode.
2. Boot signed BOOTX64.EFI with this payload attached.
3. echOS enrolls PK/KEK/db/dbx from PK.AUT/KEK.AUT/DB.AUT/DBX.AUT.
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
    @{ Host = (Join-Path $bundlePath "PK.crt"); Guest = "PK.CER" },
    @{ Host = (Join-Path $bundlePath "KEK.crt"); Guest = "KEK.CER" },
    @{ Host = (Join-Path $bundlePath "db.crt"); Guest = "DB.CER" },
    @{ Host = (Join-Path $bundlePath "dbx-bootstrap.crt"); Guest = "DBX.CER" },
    @{ Host = $readmePath; Guest = "SBREADME.TXT" },
    @{ Host = $startupPath; Guest = "STARTUP.NSH" },
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

$runQemuPath = Join-Path $PSScriptRoot "..\run_qemu.ps1"
$runArgs = @(
    "-Mode", "uefi",
    "-Profile", $QemuProfile,
    "-NoBuild",
    "-EfiPath", $signedImagePath,
    "-OvmfCodePath", $secureOvmfCode,
    "-OvmfVarsTemplatePath", $ovmfVarsTemplate,
    "-OvmfVarsPath", $varsStorePath,
    "-DisplayWidth", "$DisplayWidth",
    "-DisplayHeight", "$DisplayHeight"
)
if ($ForceVarsReset) {
    $runArgs += "-ForceVarsReset"
}
foreach ($spec in $guestFileSpecs) {
    $runArgs += @("-EspExtraFile", ("{0}::{1}" -f $spec.Host, $spec.Guest))
}

if ($Phase -eq "prepare") {
    $runArgs += "-NoAutoLogin"
    Write-Host "Secure Boot prepare phase" -ForegroundColor Cyan
    Write-Host "OVMF code : $secureOvmfCode" -ForegroundColor DarkGray
    Write-Host "Vars store: $varsStorePath" -ForegroundColor DarkGray
    Write-Host "Signed EFI: $signedImagePath" -ForegroundColor DarkGray
    Write-Host "ESP payload aliases: PK.AUT, KEK.AUT, DB.AUT, DBX.AUT, SBENROLL.ON" -ForegroundColor DarkGray
    Write-Host "Guest will auto-enroll authenticated variables and request warm reset." -ForegroundColor DarkGray
    if ($keyToolResolved) {
        Write-Host "KeyTool EFI attached as KEYTOOL.EFI" -ForegroundColor DarkGray
    }
    & $runQemuPath @runArgs
    exit $LASTEXITCODE
}

$runArgs += "-Headless"
$startedAt = Get-Date
Write-Host "Secure Boot verify phase" -ForegroundColor Cyan
Write-Host "Using vars store: $varsStorePath" -ForegroundColor DarkGray
& $runQemuPath @runArgs
if ($LASTEXITCODE -ne 0) {
    throw "QEMU verify phase failed before Secure Boot marker validation."
}

$latestSerial = Find-LatestSerialLog -StartedAt $startedAt
if (-not $latestSerial) {
    throw "Could not locate serial log for Secure Boot verify phase."
}

Assert-LogMarkers -LogPath $latestSerial.FullName -RequiredMarkers @(
    "[UEFI] Runtime services verified",
    "[UEFI] Secure Boot databases available",
    "[UEFI] Loaded image signature OK"
) -ForbiddenMarkers @(
    "[UEFI] Secure Boot databases unavailable",
    "[UEFI] Loaded image signature failed"
)

Write-Host "Secure Boot verify log: $($latestSerial.FullName)" -ForegroundColor Green
