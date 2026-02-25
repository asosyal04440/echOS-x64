param(
    [string]$VhdxPath = "c:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\Simics\echos-simics\targets\echos\images\echos-uefi.vhdx",
    [string]$EfiPath = "c:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\target\x86_64-unknown-uefi\debug\ech_os.efi",
    [int]$SizeMB = 512
)

$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)

if (-not $isAdmin) {
    $argList = @(
        "-ExecutionPolicy", "Bypass",
        "-File", ('"' + $PSCommandPath + '"'),
        "-VhdxPath", ('"' + $VhdxPath + '"'),
        "-EfiPath", ('"' + $EfiPath + '"'),
        "-SizeMB", $SizeMB
    ) -join ' '

    Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs
    Write-Host "Elevation requested. Re-run will continue in admin PowerShell."
    exit 0
}

if (-not (Test-Path $EfiPath)) {
    throw "EFI binary not found: $EfiPath"
}

if (Test-Path $VhdxPath) {
    Remove-Item $VhdxPath -Force
}

$dp = @"
create vdisk file="$VhdxPath" maximum=$SizeMB type=fixed
select vdisk file="$VhdxPath"
attach vdisk
convert gpt
create partition efi size=128
format fs=fat32 quick label="ECHOS_EFI"
assign letter=Z
exit
"@

$dpFile = Join-Path $env:TEMP "echos-diskpart.txt"
Set-Content -Path $dpFile -Value $dp -Encoding ASCII

diskpart /s $dpFile | Out-Host

$bootDir = "Z:\EFI\Boot"
New-Item -ItemType Directory -Force -Path $bootDir | Out-Null
Copy-Item $EfiPath (Join-Path $bootDir "BOOTX64.EFI") -Force

# optional startup.nsh for easier shell boot fallback
Set-Content -Path "Z:\startup.nsh" -Value "fs0:\EFI\Boot\BOOTX64.EFI" -Encoding ASCII

$dpDetach = @"
select vdisk file="$VhdxPath"
detach vdisk
exit
"@
$dpDetachFile = Join-Path $env:TEMP "echos-diskpart-detach.txt"
Set-Content -Path $dpDetachFile -Value $dpDetach -Encoding ASCII

diskpart /s $dpDetachFile | Out-Host

Write-Host "Created bootable VHDX: $VhdxPath"
