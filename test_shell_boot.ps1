param(
    [int]$TimeoutSec = 45
)

$ErrorActionPreference = "Stop"
$projectRoot = (Get-Location).Path
$logDir = Join-Path $projectRoot "logs"
if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$serialLogPath = Join-Path $logDir ("shell_test_" + $stamp + ".log")

$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        throw "QEMU bulunamadı"
    }
}

$efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
if (-not (Test-Path $efiPath)) {
    Write-Host "Building echOS UEFI..." -ForegroundColor Yellow
    cargo build --quiet --target x86_64-unknown-uefi
    if ($LASTEXITCODE -ne 0) { throw "UEFI build failed" }
}

$ovmfShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $ovmfShare "edk2-x86_64-code.fd"
$ovmfVarsTemplate = Join-Path $ovmfShare "edk2-i386-vars.fd"
$artifactDir = Join-Path $projectRoot "build\appliance"
if (-not (Test-Path $artifactDir)) { New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null }
$ovmfVars = Join-Path $artifactDir "OVMF_VARS.fd"
if (-not (Test-Path $ovmfVars)) {
    Copy-Item $ovmfVarsTemplate $ovmfVars -Force
}

Write-Host "echOS Shell Test" -ForegroundColor Cyan
Write-Host "================" -ForegroundColor Cyan
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray

$qemuArgs = @(
    "-bios", $efiPath,
    "-machine", "q35",
    "-cpu", "Haswell,+smep,+smap",
    "-smp", "sockets=1,cores=1,threads=1",
    "-m", "256M",
    "-drive", "if=pflash,format=raw,readonly=on,file=$ovmfCode",
    "-drive", "if=pflash,format=raw,file=$ovmfVars",
    "-serial", "file:$serialLogPath",
    "-debugcon", "file:$(Join-Path $logDir ("shell_test_debugcon_" + $stamp + ".log"))",
    "-global", "isa-debugcon.iobase=0xe9",
    "-monitor", "none",
    "-no-reboot",
    "-no-shutdown",
    "-display", "none",
    "-accel", "whpx,kernel-irqchip=off",
    "-accel", "tcg"
)

$proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru

Write-Host "QEMU started (PID: $($proc.Id))" -ForegroundColor DarkGray

$deadline = (Get-Date).AddSeconds($TimeoutSec)
$bootComplete = $false
$shellReady = $false

Write-Host "`nWaiting for boot..." -ForegroundColor Yellow

while ((Get-Date) -lt $deadline -and -not $proc.HasExited) {
    if (Test-Path $serialLogPath) {
        $content = Get-Content $serialLogPath -Raw -ErrorAction SilentlyContinue
        if ($content) {
            if ($content.Contains("[SHELL] Starting Ring 3 shell...")) {
                if (-not $bootComplete) {
                    Write-Host "Shell module loaded" -ForegroundColor Green
                    $bootComplete = $true
                }
            }
            if ($content.Contains("echshell> ") -or $content.Contains("$ ")) {
                $shellReady = $true
                Write-Host "Shell prompt detected" -ForegroundColor Green
                break
            }
        }
    }
    Start-Sleep -Milliseconds 250
}

Write-Host "`nResults:" -ForegroundColor Cyan
Write-Host "========" -ForegroundColor Cyan

if ($shellReady) {
    Write-Host "[PASS] Shell booted successfully" -ForegroundColor Green
    Write-Host "[PASS] Shell prompt available" -ForegroundColor Green
    
    if ($bootComplete) {
        Write-Host "[PASS] Ring 3 shell module loaded" -ForegroundColor Green
    }
    
    Write-Host "`n[INFO] Shell is ready for interactive testing" -ForegroundColor Yellow
    Write-Host "[INFO] Serial log: $serialLogPath" -ForegroundColor DarkGray
} else {
    Write-Host "[FAIL] Shell did not become ready within timeout" -ForegroundColor Red
    
    if (Test-Path $serialLogPath) {
        Write-Host "`nSerial output:" -ForegroundColor DarkGray
        Get-Content $serialLogPath -Tail 30 | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
    }
}

Write-Host "`nKilling QEMU..." -ForegroundColor DarkGray
$proc.Kill()

exit $(if ($shellReady) { 0 } else { 1 })
