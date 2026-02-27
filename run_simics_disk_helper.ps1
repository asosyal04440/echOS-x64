# run_simics_disk_helper.ps1 — VHDX disk guncelleme (admin olarak calisir)
# Dogrudan calistirmayin, run_simics.ps1 tarafindan cagirilir.

param(
    [Parameter(Mandatory)][string]$VhdxPath,
    [Parameter(Mandatory)][string]$EfiPath,
    [Parameter(Mandatory)][string]$LogPath
)

$ErrorActionPreference = 'Continue'
'' | Out-File $LogPath -Force

# Mevcut hash kontrolu — degismemisse atlayalim
$srcHash = (Get-FileHash $EfiPath -Algorithm SHA256).Hash

# Detach (onceki oturumdan kalmis olabilir)
$dp0 = "select vdisk file=`"$VhdxPath`"`ndetach vdisk"
$dp0 | Out-File "$env:TEMP\dp0.txt" -Encoding ASCII
& diskpart /s "$env:TEMP\dp0.txt" 2>&1 | Out-Null
Start-Sleep 2

# Attach + partition 2 (EFI) ye letter Z ata
$dp1 = @"
select vdisk file="$VhdxPath"
attach vdisk
select partition 2
set id=ebd0a0a2-b9e5-4433-87c0-68b6b72699c7 override
assign letter=Z
"@
$dp1 | Out-File "$env:TEMP\dp1.txt" -Encoding ASCII
& diskpart /s "$env:TEMP\dp1.txt" 2>&1 | Out-Null
Start-Sleep 3

if (Test-Path "Z:\EFI\BOOT\BOOTX64.EFI") {
    $dstHash = (Get-FileHash "Z:\EFI\BOOT\BOOTX64.EFI" -Algorithm SHA256).Hash
    if ($srcHash -eq $dstHash) {
        "SKIP: Already up to date ($srcHash)" | Add-Content $LogPath
    } else {
        Copy-Item $EfiPath "Z:\EFI\BOOT\BOOTX64.EFI" -Force
        $newHash = (Get-FileHash "Z:\EFI\BOOT\BOOTX64.EFI" -Algorithm SHA256).Hash
        "SRC=$srcHash DST=$newHash MATCH=$($srcHash -eq $newHash)" | Add-Content $LogPath
    }
} elseif (Test-Path "Z:\") {
    New-Item "Z:\EFI\BOOT" -ItemType Directory -Force | Out-Null
    Copy-Item $EfiPath "Z:\EFI\BOOT\BOOTX64.EFI" -Force
    $newHash = (Get-FileHash "Z:\EFI\BOOT\BOOTX64.EFI" -Algorithm SHA256).Hash
    "SRC=$srcHash DST=$newHash MATCH=$($srcHash -eq $newHash)" | Add-Content $LogPath
} else {
    "ERROR: Z:\ not accessible after diskpart" | Add-Content $LogPath
}

# EFI tipini geri yaz + letter kaldir + detach
$dp2 = @"
select vdisk file="$VhdxPath"
select partition 2
remove letter=Z
set id=c12a7328-f81f-11d2-ba4b-00a0c93ec93b override
detach vdisk
"@
$dp2 | Out-File "$env:TEMP\dp2.txt" -Encoding ASCII
& diskpart /s "$env:TEMP\dp2.txt" 2>&1 | Out-Null
"DONE" | Add-Content $LogPath
