param(
    [Parameter(Mandatory=$true)]
    [string]$Vmcore,
    [string]$Kernel = "target/x86_64-unknown-none/debug/ech_os",
    [switch]$Batch
)

$projectRoot = Split-Path -Parent $PSScriptRoot
$kernelPath = if ([System.IO.Path]::IsPathRooted($Kernel)) { $Kernel } else { Join-Path $projectRoot $Kernel }
$vmcorePath = if ([System.IO.Path]::IsPathRooted($Vmcore)) { $Vmcore } else { Join-Path $projectRoot $Vmcore }

if (-not (Test-Path $kernelPath)) {
    Write-Host "Kernel not found: $kernelPath" -ForegroundColor Red
    exit 1
}
if (-not (Test-Path $vmcorePath)) {
    Write-Host "Vmcore not found: $vmcorePath" -ForegroundColor Red
    exit 1
}

$gdbScript = Join-Path $PSScriptRoot "analyze-crash.gdb"
if (-not (Test-Path $gdbScript)) {
    Write-Host "GDB script not found: $gdbScript" -ForegroundColor Red
    exit 1
}

Write-Host "echOS Crash Analyzer" -ForegroundColor Cyan
Write-Host "Kernel: $kernelPath" -ForegroundColor DarkGray
Write-Host "Vmcore: $vmcorePath" -ForegroundColor DarkGray
Write-Host ""

if ($Batch) {
    gdb -batch -x $gdbScript -ex "core-file $vmcorePath" -ex "symbol-file $kernelPath"
} else {
    gdb -x $gdbScript -ex "core-file $vmcorePath" -ex "symbol-file $kernelPath"
}
