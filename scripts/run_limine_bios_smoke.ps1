[CmdletBinding()]
param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug",
    [string]$IsoPath = "",
    [string]$QemuPath = "",
    [int]$TimeoutSec = 30,
    [int]$CpuCount = 2,
    [int]$MemoryMiB = 512,
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
if ($TimeoutSec -lt 5) { throw "TimeoutSec en az 5 olmalı" }
if ($CpuCount -lt 1) { throw "CpuCount en az 1 olmalı" }
if ($MemoryMiB -lt 128) { throw "MemoryMiB en az 128 olmalı" }

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$builder = Join-Path $PSScriptRoot "build_limine_bios_iso.ps1"

if (-not $IsoPath) {
    $IsoPath = Join-Path $projectRoot "build\limine-bios\echOS-limine-bios-$Profile.iso"
    if ($NoBuild) {
        & $builder -Profile $Profile -NoBuild
    } else {
        & $builder -Profile $Profile
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Limine BIOS ISO build başarısız"
    }
}
$isoResolved = if ([System.IO.Path]::IsPathRooted($IsoPath)) {
    [System.IO.Path]::GetFullPath($IsoPath)
} else {
    [System.IO.Path]::GetFullPath((Join-Path $projectRoot $IsoPath))
}
if (-not (Test-Path -LiteralPath $isoResolved -PathType Leaf)) {
    throw "Limine BIOS ISO bulunamadı: $isoResolved"
}

if (-not $QemuPath) {
    $defaultQemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
    if (Test-Path -LiteralPath $defaultQemu -PathType Leaf) {
        $QemuPath = $defaultQemu
    } else {
        $qemuCommand = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
        if (-not $qemuCommand) { throw "QEMU bulunamadı" }
        $QemuPath = $qemuCommand.Source
    }
}
$qemuResolved = [System.IO.Path]::GetFullPath($QemuPath)
if (-not (Test-Path -LiteralPath $qemuResolved -PathType Leaf)) {
    throw "QEMU bulunamadı: $qemuResolved"
}

$logDir = Join-Path $projectRoot "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$serialPath = Join-Path $logDir ("limine_bios_{0}_serial.log" -f $stamp)
$debugconPath = Join-Path $logDir ("limine_bios_{0}_debugcon.log" -f $stamp)
$stdoutPath = Join-Path $logDir ("limine_bios_{0}_stdout.log" -f $stamp)
$stderrPath = Join-Path $logDir ("limine_bios_{0}_stderr.log" -f $stamp)

function ConvertTo-ProcessArgumentString {
    param([Parameter(Mandatory)][string[]]$ArgumentList)

    $quoted = foreach ($argument in $ArgumentList) {
        if ($argument -match '[\s"]') {
            '"' + $argument.Replace('"', '\"') + '"'
        } else {
            $argument
        }
    }
    return ($quoted -join " ")
}

$qemuArgs = @(
    "-cdrom", $isoResolved,
    "-machine", "q35",
    "-cpu", "Haswell,+smep,+smap",
    "-smp", [string]$CpuCount,
    "-m", ("{0}M" -f $MemoryMiB),
    "-display", "none",
    "-serial", ("file:{0}" -f $serialPath),
    "-debugcon", ("file:{0}" -f $debugconPath),
    "-global", "isa-debugcon.iobase=0xe9",
    "-monitor", "none",
    "-no-reboot",
    "-no-shutdown",
    "-accel", "tcg"
)

$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = $qemuResolved
$psi.Arguments = ConvertTo-ProcessArgumentString -ArgumentList $qemuArgs
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.WorkingDirectory = $projectRoot
$qemu = [System.Diagnostics.Process]::new()
$qemu.StartInfo = $psi
if (-not $qemu.Start()) { throw "QEMU başlatılamadı" }

$stdoutTask = $qemu.StandardOutput.ReadToEndAsync()
$stderrTask = $qemu.StandardError.ReadToEndAsync()
$deadline = (Get-Date).AddSeconds($TimeoutSec)
$requiredMarkers = @(
    "[BOOT] Magic: 0x0",
    "limine: Loading executable",
    "boot():/boot/ech_os",
    "[LIMINE] Booting via Limine",
    "[LIMINE] Handover complete",
    "phase_name=userspace-ready",
    "phase_name=running",
    "phase_state=running",
    "protocol=limine",
    "[BOOTCTRL] success",
    "[BOOT_TEST] PASS",
    "[RING3_TEST] PASS",
    "[VM_SECURITY_TEST] PASS",
    "[VM_STRESS_TEST] PASS",
    "[IRQ_STRESS_TEST] PASS",
    "[FRAMEBUFFER] protocol=limine state="
)
$fatalMarkers = @(
    "[PANIC]",
    "PAGE_FAULT",
    "DOUBLE_FAULT",
    "TRIPLE_FAULT",
    "qemu: fatal",
    "guest_errors",
    "phase_state=failed"
)
$markerSeen = @{}
while ((Get-Date) -lt $deadline) {
    if (Test-Path -LiteralPath $serialPath) {
        $serial = Get-Content -LiteralPath $serialPath -Raw -ErrorAction SilentlyContinue
        foreach ($marker in $requiredMarkers) {
            $markerSeen[$marker] = $serial -and $serial.Contains($marker)
        }
        if (($requiredMarkers | Where-Object { -not $markerSeen[$_] }).Count -eq 0) {
            break
        }
    }
    if ($qemu.HasExited) { break }
    Start-Sleep -Milliseconds 250
}

if (-not $qemu.HasExited) {
    try { Stop-Process -Id $qemu.Id -Force -ErrorAction SilentlyContinue } catch {}
}
try { $qemu.WaitForExit(5000) | Out-Null } catch {}
$stdoutText = ""
$stderrText = ""
if ($stdoutTask.IsCompleted) { try { $stdoutText = $stdoutTask.Result } catch {} }
if ($stderrTask.IsCompleted) { try { $stderrText = $stderrTask.Result } catch {} }
[System.IO.File]::WriteAllText($stdoutPath, $stdoutText, [System.Text.Encoding]::UTF8)
[System.IO.File]::WriteAllText($stderrPath, $stderrText, [System.Text.Encoding]::UTF8)

$serial = if (Test-Path -LiteralPath $serialPath) { Get-Content -LiteralPath $serialPath -Raw } else { "" }
$stderr = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { "" }
$debugcon = if (Test-Path -LiteralPath $debugconPath) { Get-Content -LiteralPath $debugconPath -Raw } else { "" }
$missing = @($requiredMarkers | Where-Object { -not $serial.Contains($_) })
$fatalSeen = @($fatalMarkers | Where-Object {
    $serial.Contains($_) -or $stderr.Contains($_) -or $debugcon.Contains($_)
})
if ($missing.Count -ne 0) {
    $diagnostic = if ($serial.Length -gt 4000) { $serial.Substring($serial.Length - 4000) } else { $serial }
    throw "Limine BIOS smoke başarısız. Eksik marker: $($missing -join ', '). Serial log: $serialPath`n$diagnostic"
}
if ($fatalSeen.Count -ne 0) {
    throw "Limine BIOS smoke fatal marker gördü: $($fatalSeen -join ', '). Serial: $serialPath; stderr: $stderrPath; debugcon: $debugconPath"
}

Write-Host "Limine 12.5.2 BIOS smoke PASS" -ForegroundColor Green
Write-Host "ISO: $isoResolved" -ForegroundColor DarkGray
Write-Host "Serial: $serialPath" -ForegroundColor DarkGray
Write-Host "Config kaynağı: /boot/limine/limine.conf; SMBIOS override: yok" -ForegroundColor DarkGray
