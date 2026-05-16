param(
    [ValidateSet("fast", "debug")]
    [string]$Profile = "fast",
    [switch]$NoBuild,
    [string]$EfiPath = "",
    [string]$OutputPath = "",
    [int]$EspMiB = 16,
    [ValidateSet("fat16", "fat32")]
    [string]$EspFat = "fat16",
    [string[]]$IncludeFile = @(),
    [switch]$NoAutoLogin
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

if (-not $OutputPath) {
    $OutputPath = Join-Path $projectRoot "build\appliance\echOS-uefi.iso"
}

if (-not $EfiPath) {
    if (-not $NoBuild) {
        $cargoProfileArgs = if ($Profile -eq "fast") { @("--release") } else { @() }
        Write-Host "Building echOS UEFI artifact..." -ForegroundColor Yellow
        & cargo build --target x86_64-unknown-uefi @cargoProfileArgs
        if ($LASTEXITCODE -ne 0) {
            throw "UEFI build failed"
        }
    }
    $profileName = if ($Profile -eq "fast") { "release" } else { "debug" }
    $EfiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\$profileName\ech_os.efi"
}

if (-not (Test-Path -LiteralPath $EfiPath)) {
    throw "UEFI artifact not found: $EfiPath"
}

$builder = Join-Path $PSScriptRoot "build_vm_iso.py"
$args = @(
    $builder,
    "--efi", (Resolve-Path -LiteralPath $EfiPath).Path,
    "--output", $OutputPath,
    "--esp-mib", $EspMiB,
    "--esp-fat", $EspFat
)
if ($NoAutoLogin) {
    $args += "--no-auto-login"
}
foreach ($entry in $IncludeFile) {
    $args += @("--include-file", $entry)
}

& python @args
if ($LASTEXITCODE -ne 0) {
    throw "UEFI ISO build failed"
}

Write-Host "ISO ready: $OutputPath" -ForegroundColor Green
Write-Host "VM contract: UEFI firmware/OVMF, attach as optical media." -ForegroundColor DarkGray
