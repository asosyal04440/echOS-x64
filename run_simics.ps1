# run_simics.ps1 - echOS Simics tek tusla boot
# Kullanim:
#   .\run_simics.ps1              # Debug build + GUI
#   .\run_simics.ps1 -Release     # Release build + GUI
#   .\run_simics.ps1 -NoBuild     # Build atlayip mevcut EFI ile
#   .\run_simics.ps1 -BatchMode   # Ekransiz batch mod

param(
    [switch]$NoBuild,
    [switch]$Release,
    [switch]$BatchMode,
    [int]$TimeoutSec = 3600 # 1 hour default (removed hard timeout from scripts)
)

$ErrorActionPreference = 'Stop'
$ProjectRoot   = $PSScriptRoot
$SimicsProject = Join-Path $ProjectRoot "Simics\echos-simics"
$VhdxPath      = Join-Path $SimicsProject "targets\echos\images\echos-uefi-fixed.vhdx"
$SimicsBat     = Join-Path $SimicsProject "simics.bat"
$SimicsGui     = "D:\Intel Simics Package Manager\simics-simics-gui-external-7.0.3\simics-gui.exe"
$GuiScript     = "targets/echos/gui-boot.simics"
$BatchScript   = "targets/echos/zero-tolerance-gate.simics"
$Log           = Join-Path $ProjectRoot "vhdx_update.log"
$DiskHelper    = Join-Path $ProjectRoot "run_simics_disk_helper.ps1"
$SerialCapture = Join-Path $SimicsProject "targets\echos\logs\serial_capture.txt"

$Profile = if ($Release) { "release" } else { "debug" }
$EfiPath = Join-Path $ProjectRoot "target\x86_64-unknown-uefi\$Profile\ech_os.efi"

Write-Host ""
Write-Host "  =============================" -ForegroundColor Cyan
Write-Host "   echOS Simics Boot Launcher"   -ForegroundColor Cyan
Write-Host "  =============================" -ForegroundColor Cyan
Write-Host "  Profil : $Profile"
$modStr = if ($BatchMode) { "Batch (ekransiz)" } else { "GUI (pencereli)" }
Write-Host "  Mod    : $modStr"
Write-Host ""

# --- 1. Build ---
if (-not $NoBuild) {
    Write-Host "[1/3] Derleniyor ($Profile)..." -ForegroundColor Yellow
    $buildArgs = @("build", "--target", "x86_64-unknown-uefi", "--features", "simics")
    if ($Release) { $buildArgs += "--release" }

    $prevPref = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & cargo @buildArgs 2>&1 | ForEach-Object {
        $line = $_.ToString()
        if ($line -match 'error\[') { Write-Host $line -ForegroundColor Red }
        elseif ($line -match 'warning') { Write-Host $line -ForegroundColor Yellow }
        elseif ($line -match 'Compiling|Finished') { Write-Host "  $line" -ForegroundColor DarkGray }
    }
    $ErrorActionPreference = $prevPref
    if (-not (Test-Path $EfiPath)) {
        Write-Host "[HATA] Build basarisiz: $EfiPath bulunamadi" -ForegroundColor Red
        exit 1
    }
    $sz = (Get-Item $EfiPath).Length
    Write-Host "  EFI binary: $([math]::Round($sz/1KB)) KB" -ForegroundColor Green
} else {
    Write-Host "[1/3] Build atlandi (-NoBuild)" -ForegroundColor DarkGray
    if (-not (Test-Path $EfiPath)) {
        Write-Host "[HATA] $EfiPath bulunamadi. -NoBuild kaldirip tekrar calistir." -ForegroundColor Red
        exit 1
    }
}

# --- 2. VHDX Guncelle ---
Write-Host "[2/3] VHDX guncelleniyor..." -ForegroundColor Yellow

if (-not (Test-Path $VhdxPath)) {
    Write-Host "[HATA] VHDX bulunamadi: $VhdxPath" -ForegroundColor Red
    exit 1
}
if (-not (Test-Path $DiskHelper)) {
    Write-Host "[HATA] Disk helper bulunamadi: $DiskHelper" -ForegroundColor Red
    exit 1
}

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if ($isAdmin) {
    & $DiskHelper -VhdxPath $VhdxPath -EfiPath $EfiPath -LogPath $Log
} else {
    Write-Host "  UAC penceresi cikacak - Evet e tikla" -ForegroundColor Magenta
    Start-Process powershell -Verb RunAs -ArgumentList "-ExecutionPolicy Bypass -File `"$DiskHelper`" -VhdxPath `"$VhdxPath`" -EfiPath `"$EfiPath`" -LogPath `"$Log`"" -Wait
}

if (Test-Path $Log) {
    $logContent = Get-Content $Log -Raw
    if ($logContent -match 'MATCH=True') {
        Write-Host "  VHDX guncellendi (SHA256 OK)" -ForegroundColor Green
    } elseif ($logContent -match 'SKIP') {
        Write-Host "  VHDX zaten guncel" -ForegroundColor Green
    } else {
        Write-Host "  [UYARI] Sonuc belirsiz:" -ForegroundColor Yellow
        Write-Host "  $logContent" -ForegroundColor DarkGray
    }
}

# --- 3. Simics Baslat ---
Write-Host "[3/3] Simics baslatiliyor..." -ForegroundColor Yellow

Push-Location $SimicsProject
try {
    if ($BatchMode) {
        if (Test-Path $SerialCapture) {
            Remove-Item $SerialCapture -Force
        }
        Write-Host "  Batch mod: $TimeoutSec sn simulasyon" -ForegroundColor DarkCyan
        & $SimicsBat --batch-mode $BatchScript
        Write-Host ""
        $serial = $SerialCapture
        if (Test-Path $serial) {
            $bytes = (Get-Item $serial).Length
            $panicCount = (Select-String -Path $serial -Pattern '\[PANIC\]' -SimpleMatch | Measure-Object).Count
            $col = if ($panicCount -gt 0) { "Red" } else { "Green" }
            Write-Host "  Serial: $bytes byte" -ForegroundColor $col
            if ($panicCount -gt 0) {
                Write-Host "  [!] $panicCount PANIC bulundu:" -ForegroundColor Red
                Select-String -Path $serial -Pattern '\[PANIC\]' -SimpleMatch | ForEach-Object {
                    Write-Host "      $_" -ForegroundColor Red
                }
            } else {
                Write-Host "  Panic yok - boot basarili!" -ForegroundColor Green
            }
        }
    } else {
        Write-Host "  Simics aciliyor (VGA + Serial + Konsol)..." -ForegroundColor Cyan
        & $SimicsBat $GuiScript
    }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "  Bitti." -ForegroundColor Green
