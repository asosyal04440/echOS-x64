[CmdletBinding()]
param(
    [ValidateSet("debug", "release")][string]$Profile = "debug",
    [string]$IsoPath = "",
    [string]$QemuPath = "",
    [int]$TimeoutSec = 45,
    [int]$CpuCount = 2,
    [int]$MemoryMiB = 512,
    [switch]$NoBuild
)
$ErrorActionPreference = "Stop"
if ($TimeoutSec -lt 5 -or $CpuCount -lt 1 -or $MemoryMiB -lt 128) { throw "Geçersiz smoke parametresi" }
$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$builder = Join-Path $PSScriptRoot "build_multiboot2_iso.ps1"
if (-not $IsoPath) {
    $IsoPath = Join-Path $root ("build\multiboot2\echOS-multiboot2-{0}.iso" -f $Profile)
    if (-not $NoBuild) {
        & $builder -Profile $Profile -OutputPath $IsoPath
    }
    if ($LASTEXITCODE -ne 0) { throw "MB2 ISO build başarısız" }
}
$iso = if ([System.IO.Path]::IsPathRooted($IsoPath)) {
    [System.IO.Path]::GetFullPath($IsoPath)
} else {
    [System.IO.Path]::GetFullPath((Join-Path $root $IsoPath))
}
if (-not (Test-Path -LiteralPath $iso -PathType Leaf)) { throw "MB2 ISO bulunamadı: $iso" }
if (-not $QemuPath) {
    $default = "C:\Program Files\qemu\qemu-system-x86_64.exe"
    if (Test-Path -LiteralPath $default -PathType Leaf) {
        $QemuPath = $default
    } else {
        $q = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
        if (-not $q) { throw "QEMU bulunamadı" }
        $QemuPath = $q.Source
    }
}
$qemu = [System.IO.Path]::GetFullPath($QemuPath)
if (-not (Test-Path -LiteralPath $qemu -PathType Leaf)) { throw "QEMU bulunamadı: $qemu" }
$logDir = Join-Path $root "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$serialPath = Join-Path $logDir ("multiboot2_{0}_serial.log" -f $stamp)
$debugconPath = Join-Path $logDir ("multiboot2_{0}_debugcon.log" -f $stamp)
$stdoutPath = Join-Path $logDir ("multiboot2_{0}_stdout.log" -f $stamp)
$stderrPath = Join-Path $logDir ("multiboot2_{0}_stderr.log" -f $stamp)
$qemuArgs = @(
    "-cdrom", $iso, "-machine", "q35", "-cpu", "Haswell,+smep,+smap",
    "-smp", [string]$CpuCount, "-m", ("{0}M" -f $MemoryMiB),
    "-display", "none", "-serial", ("file:{0}" -f $serialPath),
    "-debugcon", ("file:{0}" -f $debugconPath), "-global", "isa-debugcon.iobase=0xe9",
    "-monitor", "none", "-no-reboot", "-no-shutdown", "-accel", "tcg"
)
function ConvertTo-ProcessArgumentString {
    param([Parameter(Mandatory)][string[]]$ArgumentList)
    $quoted = foreach ($argument in $ArgumentList) {
        if ($argument -match '[\s"]') { '"' + $argument.Replace('"', '\"') + '"' } else { $argument }
    }
    return ($quoted -join " ")
}
$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = $qemu
$psi.Arguments = ConvertTo-ProcessArgumentString -ArgumentList $qemuArgs
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.WorkingDirectory = $root
$proc = [System.Diagnostics.Process]::new()
$proc.StartInfo = $psi
if (-not $proc.Start()) { throw "QEMU başlatılamadı" }
$stdoutTask = $proc.StandardOutput.ReadToEndAsync()
$stderrTask = $proc.StandardError.ReadToEndAsync()
$required = @(
    "[BOOT] Magic: 0x36d76289", "[MULTIBOOT] Info parsed", "phase_name=userspace-ready",
    "phase_name=running", "phase_state=running", "protocol=multiboot2", "[BOOTCTRL] success",
    "[BOOT_TEST] PASS", "[RING3_TEST] PASS", "[VM_SECURITY_TEST] PASS",
    "[VM_STRESS_TEST] PASS", "[IRQ_STRESS_TEST] PASS",
    "[FRAMEBUFFER] protocol=multiboot2 state="
)
$fatal = @("[PANIC]", "PAGE_FAULT", "DOUBLE_FAULT", "TRIPLE_FAULT", "qemu: fatal", "guest_errors", "phase_state=failed")
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) {
    $serial = if (Test-Path -LiteralPath $serialPath) { Get-Content -LiteralPath $serialPath -Raw -ErrorAction SilentlyContinue } else { "" }
    if ($null -eq $serial) { $serial = "" }
    if (($required | Where-Object { -not ($serial -and $serial.Contains($_)) }).Count -eq 0) { break }
    if ($proc.HasExited) { break }
    Start-Sleep -Milliseconds 250
}
if (-not $proc.HasExited) { try { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue } catch {} }
try { $proc.WaitForExit(5000) | Out-Null } catch {}
$stdoutText = ""
$stderrText = ""
if ($stdoutTask.IsCompleted) { try { $stdoutText = $stdoutTask.Result } catch {} }
if ($stderrTask.IsCompleted) { try { $stderrText = $stderrTask.Result } catch {} }
[System.IO.File]::WriteAllText($stdoutPath, $stdoutText, [System.Text.Encoding]::UTF8)
[System.IO.File]::WriteAllText($stderrPath, $stderrText, [System.Text.Encoding]::UTF8)
$serial = if (Test-Path -LiteralPath $serialPath) { Get-Content -LiteralPath $serialPath -Raw } else { "" }
$stderr = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { "" }
$debugcon = if (Test-Path -LiteralPath $debugconPath) { Get-Content -LiteralPath $debugconPath -Raw } else { "" }
$missing = @($required | Where-Object { -not ($serial -and $serial.Contains($_)) })
$fatalSeen = @($fatal | Where-Object {
    ($serial -and $serial.Contains($_)) -or
    ($stderr -and $stderr.Contains($_)) -or
    ($debugcon -and $debugcon.Contains($_))
})
if ($missing.Count -ne 0) {
    $diagnostic = if ($serial.Length -gt 4000) { $serial.Substring($serial.Length - 4000) } else { $serial }
    throw "MB2 smoke başarısız; eksik marker: $($missing -join ', '); serial=$serialPath`n$diagnostic"
}
if ($fatalSeen.Count -ne 0) { throw "MB2 smoke fatal marker: $($fatalSeen -join ', '); serial=$serialPath" }
Write-Host "Multiboot2 BIOS smoke PASS" -ForegroundColor Green
Write-Host "ISO: $iso" -ForegroundColor DarkGray
Write-Host "Serial: $serialPath" -ForegroundColor DarkGray
