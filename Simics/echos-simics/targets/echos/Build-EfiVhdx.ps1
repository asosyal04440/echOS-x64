<#
.SYNOPSIS
    echOS Enterprise Disk Builder — UEFI-bootable VHDX with 4K sector alignment.
.DESCRIPTION
    Creates a fixed VHDX via Windows diskpart with GPT + FAT32 ESP,
    copies BOOTX64.EFI, verifies bit-perfect integrity via SHA256,
    and optionally generates a CRAFF backup for Simics fallback.
    Self-elevates to Administrator when needed.
.PARAMETER VhdxPath
    Output VHDX path. Overwrites existing file.
.PARAMETER EfiPath
    Path to the compiled ech_os.efi binary.
.PARAMETER SizeMB
    Disk size in MB (fixed type). Default: 512.
.PARAMETER SkipCraff
    Skip CRAFF backup generation.
.EXAMPLE
    .\Build-EfiVhdx.ps1
    .\Build-EfiVhdx.ps1 -SizeMB 256 -SkipCraff
#>
param(
    [string]$VhdxPath,
    [string]$EfiPath,
    [int]$SizeMB = 512,
    [switch]$SkipCraff
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# ── Resolve paths ──────────────────────────────────────────────
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $VhdxPath) {
    $VhdxPath = Join-Path $scriptDir "images\echos-uefi-fixed.vhdx"
}

if (-not $EfiPath) {
    $echOSRoot = (Resolve-Path (Join-Path $scriptDir "..\..\..\..")).Path
    $EfiPath = Join-Path $echOSRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
}

# ── UAC Self-Elevation ────────────────────────────────────────
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)

if (-not $isAdmin) {
    Write-Host "[BUILD-DISK] Requesting Administrator elevation..." -ForegroundColor Yellow
    $argList = @(
        "-ExecutionPolicy", "Bypass",
        "-File", ('"' + $PSCommandPath + '"'),
        "-VhdxPath", ('"' + $VhdxPath + '"'),
        "-EfiPath", ('"' + $EfiPath + '"'),
        "-SizeMB", $SizeMB
    )
    if ($SkipCraff) { $argList += "-SkipCraff" }

    Start-Process -FilePath "powershell.exe" -ArgumentList ($argList -join ' ') -Verb RunAs -Wait
    Write-Host "[BUILD-DISK] Admin process completed." -ForegroundColor Green
    exit 0
}

# ── Validation ─────────────────────────────────────────────────
Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "  echOS Enterprise Disk Builder v2.0" -ForegroundColor Cyan
Write-Host "  4K-Aligned GPT + FAT32 ESP | Fixed VHDX" -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan
Write-Host ""

if (-not (Test-Path $EfiPath)) {
    throw "[BUILD-DISK] FATAL: EFI binary not found: $EfiPath`nRun: cargo build --target x86_64-unknown-uefi"
}

$efiHash = (Get-FileHash -Path $EfiPath -Algorithm SHA256).Hash
$efiSize = (Get-Item $EfiPath).Length
Write-Host "[BUILD-DISK] EFI binary : $EfiPath" -ForegroundColor White
Write-Host "[BUILD-DISK] EFI size   : $efiSize bytes" -ForegroundColor White
Write-Host "[BUILD-DISK] EFI SHA256 : $efiHash" -ForegroundColor DarkGray
Write-Host "[BUILD-DISK] VHDX target: $VhdxPath" -ForegroundColor White
Write-Host "[BUILD-DISK] Disk size  : ${SizeMB} MB (fixed)" -ForegroundColor White
Write-Host ""

# ── Clean previous ─────────────────────────────────────────────
if (Test-Path $VhdxPath) {
    Write-Host "[BUILD-DISK] Removing existing VHDX..." -ForegroundColor Yellow
    Remove-Item $VhdxPath -Force
}

$imagesDir = Split-Path -Parent $VhdxPath
if (-not (Test-Path $imagesDir)) {
    New-Item -ItemType Directory -Path $imagesDir -Force | Out-Null
}

# ── Find available drive letter ───────────────────────────────
function Get-AvailableDriveLetter {
    $used = (Get-PSDrive -PSProvider FileSystem).Name
    foreach ($letter in ([char[]]([int][char]'Z'..[int][char]'D'))) {
        if ($letter -notin $used) { return [string]$letter }
    }
    throw "[BUILD-DISK] FATAL: No available drive letters."
}

$driveLetter = Get-AvailableDriveLetter
Write-Host "[BUILD-DISK] Using drive letter: ${driveLetter}:" -ForegroundColor DarkGray

# ── Create VHDX via diskpart ──────────────────────────────────
Write-Host "[BUILD-DISK] Phase 1/4: Creating fixed VHDX..." -ForegroundColor Cyan

$dpCreate = @"
create vdisk file="$VhdxPath" maximum=$SizeMB type=fixed
select vdisk file="$VhdxPath"
attach vdisk
convert gpt
create partition efi size=128 align=4096
format fs=fat32 quick label="ECHOS_EFI"
assign letter=$driveLetter
exit
"@

$dpFile = Join-Path $env:TEMP "echos-dp-create-$PID.txt"
Set-Content -Path $dpFile -Value $dpCreate -Encoding ASCII

$dpResult = & diskpart /s $dpFile 2>&1
$dpExitCode = $LASTEXITCODE
$dpResult | ForEach-Object { Write-Host "  [diskpart] $_" -ForegroundColor DarkGray }

if ($dpExitCode -ne 0) {
    throw "[BUILD-DISK] FATAL: diskpart failed (exit code $dpExitCode)"
}

$espRoot = "${driveLetter}:\"
if (-not (Test-Path $espRoot)) {
    Start-Sleep -Seconds 2
    if (-not (Test-Path $espRoot)) {
        throw "[BUILD-DISK] FATAL: Drive ${driveLetter}: not mounted after diskpart."
    }
}

Write-Host "[BUILD-DISK] Phase 1/4: VHDX created and mounted at ${driveLetter}:" -ForegroundColor Green

# ── Copy EFI files ─────────────────────────────────────────────
Write-Host "[BUILD-DISK] Phase 2/4: Injecting BOOTX64.EFI..." -ForegroundColor Cyan

$bootDir = "${driveLetter}:\EFI\Boot"
New-Item -ItemType Directory -Force -Path $bootDir | Out-Null
Copy-Item $EfiPath (Join-Path $bootDir "BOOTX64.EFI") -Force

$startupNsh = @"
@echo -off
echo echOS UEFI Boot (startup.nsh fallback)
fs0:\EFI\Boot\BOOTX64.EFI
"@
Set-Content -Path "${driveLetter}:\startup.nsh" -Value $startupNsh -Encoding ASCII

Write-Host "[BUILD-DISK] Phase 2/4: EFI payload deployed." -ForegroundColor Green

# ── SHA256 verification ────────────────────────────────────────
Write-Host "[BUILD-DISK] Phase 3/4: Bit-perfect integrity check..." -ForegroundColor Cyan

$deployedEfi = Join-Path $bootDir "BOOTX64.EFI"
$deployedHash = (Get-FileHash -Path $deployedEfi -Algorithm SHA256).Hash
$deployedSize = (Get-Item $deployedEfi).Length

if ($deployedHash -ne $efiHash) {
    throw "[BUILD-DISK] FATAL: SHA256 MISMATCH!`n  Source : $efiHash`n  Deployed: $deployedHash"
}

if ($deployedSize -ne $efiSize) {
    throw "[BUILD-DISK] FATAL: Size mismatch! Source=$efiSize Deployed=$deployedSize"
}

Write-Host "[BUILD-DISK] SHA256 MATCH: $deployedHash" -ForegroundColor Green
Write-Host "[BUILD-DISK] Size  MATCH: $deployedSize bytes" -ForegroundColor Green

# ── Detach VHDX ────────────────────────────────────────────────
$dpDetach = @"
select vdisk file="$VhdxPath"
detach vdisk
exit
"@
$dpDetachFile = Join-Path $env:TEMP "echos-dp-detach-$PID.txt"
Set-Content -Path $dpDetachFile -Value $dpDetach -Encoding ASCII

& diskpart /s $dpDetachFile 2>&1 | ForEach-Object { Write-Host "  [diskpart] $_" -ForegroundColor DarkGray }

Remove-Item $dpFile -Force -ErrorAction SilentlyContinue
Remove-Item $dpDetachFile -Force -ErrorAction SilentlyContinue

$vhdxSize = (Get-Item $VhdxPath).Length
Write-Host "[BUILD-DISK] Detached. VHDX size: $([math]::Round($vhdxSize / 1MB, 1)) MB" -ForegroundColor Green

# ── CRAFF backup (optional) ───────────────────────────────────
Write-Host "[BUILD-DISK] Phase 4/4: CRAFF fallback generation..." -ForegroundColor Cyan

if ($SkipCraff) {
    Write-Host "[BUILD-DISK] Phase 4/4: Skipped (-SkipCraff)." -ForegroundColor Yellow
} else {
    $craffBat = Join-Path $scriptDir "..\..\bin\craff.bat"
    $craffOut = [System.IO.Path]::ChangeExtension($VhdxPath, ".craff")

    if (Test-Path $craffBat) {
        Write-Host "[BUILD-DISK] Converting to CRAFF: $craffOut" -ForegroundColor DarkGray
        & $craffBat -o $craffOut $VhdxPath 2>&1 | ForEach-Object { Write-Host "  [craff] $_" -ForegroundColor DarkGray }

        if (Test-Path $craffOut) {
            $craffSize = (Get-Item $craffOut).Length
            Write-Host "[BUILD-DISK] CRAFF backup: $([math]::Round($craffSize / 1MB, 1)) MB" -ForegroundColor Green
        } else {
            Write-Host "[BUILD-DISK] WARNING: CRAFF conversion failed (non-fatal)." -ForegroundColor Yellow
        }
    } else {
        Write-Host "[BUILD-DISK] WARNING: craff.bat not found at $craffBat (non-fatal)." -ForegroundColor Yellow
    }
}

# ── Summary ────────────────────────────────────────────────────
Write-Host ""
Write-Host "========================================================" -ForegroundColor Green
Write-Host "  BUILD COMPLETE" -ForegroundColor Green
Write-Host "  VHDX    : $VhdxPath" -ForegroundColor Green
Write-Host "  Size    : $([math]::Round($vhdxSize / 1MB, 1)) MB (fixed, 4K-aligned)" -ForegroundColor Green
Write-Host "  ESP     : \EFI\Boot\BOOTX64.EFI ($deployedSize bytes)" -ForegroundColor Green
Write-Host "  SHA256  : $deployedHash" -ForegroundColor Green
Write-Host "  Fallback: startup.nsh" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Green
