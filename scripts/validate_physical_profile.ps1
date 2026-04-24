param(
    [string]$ProfilePath = ".\artifacts\field\physical_profile.json",
    [string]$AdmissionProfilePath = ".\docs\agent\single-pc-admission-profile.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $ProfilePath)) {
    throw "Captured profile not found: $ProfilePath"
}
if (-not (Test-Path -LiteralPath $AdmissionProfilePath)) {
    throw "Admission profile not found: $AdmissionProfilePath"
}

$captured = Get-Content -LiteralPath $ProfilePath -Raw | ConvertFrom-Json
$admission = Get-Content -LiteralPath $AdmissionProfilePath -Raw | ConvertFrom-Json

$failures = New-Object System.Collections.Generic.List[string]
$warnings = New-Object System.Collections.Generic.List[string]

if ($captured.machine.firmware -ne $admission.requirements.firmware.required) {
    $failures.Add("firmware mismatch: expected $($admission.requirements.firmware.required), got $($captured.machine.firmware)")
}

$nvmeControllers = @($captured.storage.nvme)
if ($nvmeControllers.Count -lt 1) {
    $failures.Add("no NVMe controller detected")
}

$wiredAdapters = @($captured.network.adapters | Where-Object { $_.kind -eq "wired" })
if ($wiredAdapters.Count -lt 1) {
    $failures.Add("no wired Ethernet adapter detected")
}

$ps2Keyboards = @($captured.input.keyboards | Where-Object { $_.is_ps2 })
if ($ps2Keyboards.Count -lt 1) {
    $failures.Add("no PS/2 keyboard fallback detected")
}

$usbControllers = @($captured.input.usb_controllers)
if ($usbControllers.Count -gt 0) {
    $warnings.Add("USB controller present; USB input is not part of admission exactness and should not be the only keyboard path")
}

$audioDevices = @($captured.media.audio)
if ($audioDevices.Count -gt 0) {
    $warnings.Add("audio devices present; audio is currently outside admission profile")
}

$wirelessAdapters = @($captured.network.adapters | Where-Object { $_.kind -eq "wireless" })
if ($wirelessAdapters.Count -gt 0) {
    $warnings.Add("wireless adapters present; WiFi is outside admission profile")
}

$bluetoothDevices = @($captured.media.bluetooth)
if ($bluetoothDevices.Count -gt 0) {
    $warnings.Add("Bluetooth devices present; Bluetooth is outside admission profile")
}

$displayAdapters = @($captured.display.adapters)
if ($displayAdapters.Count -eq 0) {
    $warnings.Add("no display adapters detected from host inventory; GOP path must be manually confirmed")
}

Write-Host "Admission profile: $($admission.profile_name)" -ForegroundColor Cyan
Write-Host "Captured machine: $($captured.machine.manufacturer) $($captured.machine.model)" -ForegroundColor DarkGray
Write-Host "Firmware: $($captured.machine.firmware)" -ForegroundColor DarkGray
Write-Host "NVMe controllers: $($nvmeControllers.Count)" -ForegroundColor DarkGray
Write-Host "Wired NICs: $($wiredAdapters.Count)" -ForegroundColor DarkGray
Write-Host "PS/2 keyboards: $($ps2Keyboards.Count)" -ForegroundColor DarkGray

foreach ($warning in $warnings) {
    Write-Host "WARN: $warning" -ForegroundColor Yellow
}

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) {
        Write-Host "FAIL: $failure" -ForegroundColor Red
    }
    throw "single-PC admission profile rejected"
}

Write-Host "Admission profile accepted." -ForegroundColor Green
