param(
    [string]$OutputPath = ".\artifacts\field\physical_profile.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-FirmwareType {
    $control = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control"
    switch ($control.PEFirmwareType) {
        2 { return "UEFI" }
        1 { return "BIOS" }
        default { return "Unknown" }
    }
}

function Convert-PciIdentity {
    param(
        [string]$InstanceId
    )

    $result = @{
        instance_id = $InstanceId
        vendor_id = $null
        device_id = $null
    }
    if ($InstanceId -match "VEN_([0-9A-Fa-f]{4})") {
        $result.vendor_id = $matches[1].ToUpperInvariant()
    }
    if ($InstanceId -match "DEV_([0-9A-Fa-f]{4})") {
        $result.device_id = $matches[1].ToUpperInvariant()
    }
    return $result
}

function Get-PresentDevicesByClass {
    param(
        [string]$Class
    )

    @(Get-PnpDevice -PresentOnly -Class $Class -ErrorAction SilentlyContinue | ForEach-Object {
        $identity = Convert-PciIdentity -InstanceId $_.InstanceId
        [ordered]@{
            friendly_name = $_.FriendlyName
            class = $_.Class
            status = $_.Status
            instance_id = $_.InstanceId
            vendor_id = $identity.vendor_id
            device_id = $identity.device_id
        }
    })
}

function Get-KeyboardSummary {
    $devices = @(Get-PnpDevice -PresentOnly -Class Keyboard -ErrorAction SilentlyContinue)
    $entries = @()
    foreach ($device in $devices) {
        $isPs2 = $device.InstanceId -match "^ACPI\\PNP03(03|0B)"
        $entries += [ordered]@{
            friendly_name = $device.FriendlyName
            instance_id = $device.InstanceId
            status = $device.Status
            is_ps2 = $isPs2
        }
    }
    return $entries
}

function Get-NetSummary {
    $entries = @()
    foreach ($adapter in Get-NetAdapter -IncludeHidden | Where-Object { $_.Status -ne "Disabled" -or $_.HardwareInterface }) {
        $kind = if ($adapter.NdisPhysicalMedium -match "Wireless") {
            "wireless"
        } elseif ($adapter.InterfaceDescription -match "Bluetooth") {
            "bluetooth"
        } else {
            "wired"
        }
        $entries += [ordered]@{
            name = $adapter.Name
            interface_description = $adapter.InterfaceDescription
            status = $adapter.Status
            physical_medium = [string]$adapter.NdisPhysicalMedium
            link_speed = [string]$adapter.LinkSpeed
            kind = $kind
        }
    }
    return $entries
}

$outputDirectory = Split-Path -Parent $OutputPath
if ($outputDirectory) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
}

$profile = [ordered]@{
    captured_at = (Get-Date).ToString("s")
    machine = [ordered]@{
        manufacturer = (Get-CimInstance Win32_ComputerSystem).Manufacturer
        model = (Get-CimInstance Win32_ComputerSystem).Model
        firmware = Get-FirmwareType
        secure_boot_capable = [bool](Confirm-SecureBootUEFI -ErrorAction SilentlyContinue)
    }
    storage = [ordered]@{
        nvme = Get-PresentDevicesByClass -Class "SCSIAdapter" | Where-Object {
            $_.friendly_name -match "NVMe" -or $_.instance_id -match "NVME"
        }
        ide = Get-PresentDevicesByClass -Class "IDE"
    }
    input = [ordered]@{
        keyboards = Get-KeyboardSummary
        usb_controllers = Get-PresentDevicesByClass -Class "USB" | Where-Object {
            $_.friendly_name -match "xHCI|USB"
        }
    }
    display = [ordered]@{
        adapters = Get-PresentDevicesByClass -Class "Display"
    }
    network = [ordered]@{
        adapters = Get-NetSummary
    }
    media = [ordered]@{
        audio = Get-PresentDevicesByClass -Class "MEDIA"
        bluetooth = Get-PresentDevicesByClass -Class "Bluetooth"
    }
}

$profile | ConvertTo-Json -Depth 6 | Set-Content -Path $OutputPath -Encoding utf8
Write-Host "physical profile captured: $OutputPath" -ForegroundColor Green
