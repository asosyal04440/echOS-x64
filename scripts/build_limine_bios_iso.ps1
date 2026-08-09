[CmdletBinding()]
param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug",
    [string]$KernelPath = "",
    [string]$OutputPath = "",
    [string]$LimineRoot = "",
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))

function Resolve-ProjectPath {
    param([Parameter(Mandatory)][string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $projectRoot $Path))
}

function Require-File {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Description)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Description bulunamadı: $Path"
    }
    return [System.IO.Path]::GetFullPath($Path)
}

function Convert-ToWslPath {
    param([Parameter(Mandatory)][string]$Path)

    $wslPath = & wsl.exe -e wslpath -a -u -- $Path 2>&1
    if ($LASTEXITCODE -ne 0 -or -not $wslPath) {
        throw "WSL yolu çözülemedi: $Path"
    }
    return (($wslPath | Select-Object -Last 1).ToString().Trim())
}

function Get-Multiboot2Header {
    param([Parameter(Mandatory)][byte[]]$Bytes)

    $magic = [byte[]](0xd6, 0x50, 0x52, 0xe8)
    $scanLimit = [Math]::Min($Bytes.Length - 16, 32768)
    for ($offset = 0; $offset -le $scanLimit; $offset += 8) {
        if ($Bytes[$offset] -ne $magic[0] -or
            $Bytes[$offset + 1] -ne $magic[1] -or
            $Bytes[$offset + 2] -ne $magic[2] -or
            $Bytes[$offset + 3] -ne $magic[3]) {
            continue
        }

        $length = [BitConverter]::ToUInt32($Bytes, $offset + 8)
        if ($length -lt 16 -or $offset + $length -gt $Bytes.Length) {
            throw "Multiboot2 header uzunluğu geçersiz: offset=0x{0:x}, length=0x{1:x}" -f $offset, $length
        }

        [UInt64]$sum = 0
        # Multiboot2 defines the checksum over magic, architecture, length,
        # and checksum only; optional header tags are excluded.
        for ($cursor = 0; $cursor -lt 16; $cursor += 4) {
            $sum = ($sum + [UInt64]([BitConverter]::ToUInt32($Bytes, $offset + $cursor))) % [UInt64]0x100000000
        }
        if ($sum -ne 0) {
            throw "Multiboot2 header checksum geçersiz: offset=0x{0:x}, sum=0x{1:x8}" -f $offset, $sum
        }

        return [PSCustomObject]@{
            Offset = $offset
            Length = [UInt32]$length
        }
    }

    return $null
}

if (-not $KernelPath) {
    if (-not $NoBuild) {
        $cargoProfileArgs = if ($Profile -eq "release") { @("--release") } else { @() }
        Write-Host "Building echOS bare-metal kernel ($Profile)..." -ForegroundColor Yellow
        $previousKernelLinker = $env:ECHOS_KERNEL_LINKER
        try {
            $env:ECHOS_KERNEL_LINKER = "linker_limine.ld"
            & cargo build-kernel @cargoProfileArgs
            if ($LASTEXITCODE -ne 0) {
                throw "Bare-metal native Limine kernel build başarısız"
            }
        } finally {
            if ($null -eq $previousKernelLinker) {
                Remove-Item Env:ECHOS_KERNEL_LINKER -ErrorAction SilentlyContinue
            } else {
                $env:ECHOS_KERNEL_LINKER = $previousKernelLinker
            }
        }
    }

    $kernelProfile = if ($Profile -eq "release") { "release" } else { "debug" }
    $KernelPath = Join-Path $projectRoot "target\x86_64-unknown-none\$kernelProfile\ech_os"
}
$kernelPathResolved = Require-File -Path (Resolve-ProjectPath $KernelPath) -Description "Bare-metal kernel"

$limineRootResolved = if ($LimineRoot) {
    Resolve-ProjectPath $LimineRoot
} else {
    Join-Path $projectRoot "limine_iso\boot\limine"
}
$limineRootResolved = Require-File -Path (Join-Path $limineRootResolved "limine-bios-cd.bin") -Description "Limine BIOS CD imajı" | Split-Path -Parent
$limineBiosCd = Require-File -Path (Join-Path $limineRootResolved "limine-bios-cd.bin") -Description "Limine BIOS CD imajı"
$limineBiosSys = Require-File -Path (Join-Path $limineRootResolved "limine-bios.sys") -Description "Limine BIOS sistem dosyası"
$limineUefiCd = Require-File -Path (Join-Path $limineRootResolved "limine-uefi-cd.bin") -Description "Limine UEFI CD imajı"
$limineBootx64 = Require-File -Path (Join-Path $limineRootResolved "BOOTX64.EFI") -Description "Limine UEFI loader"
$limineExe = Require-File -Path (Join-Path $limineRootResolved "limine.exe") -Description "Limine host aracı"

$limineVersion = (& $limineExe version 2>&1 | Select-Object -First 1).ToString().Trim()
if ($limineVersion -notlike "Limine 12.5.2*") {
    throw "Limine sürüm doğrulaması başarısız; beklenen 'Limine 12.5.2', bulunan '$limineVersion'"
}

$kernelBytes = [System.IO.File]::ReadAllBytes($kernelPathResolved)
if ($kernelBytes.Length -lt 64 -or
    $kernelBytes[0] -ne 0x7f -or $kernelBytes[1] -ne 0x45 -or
    $kernelBytes[2] -ne 0x4c -or $kernelBytes[3] -ne 0x46) {
    throw "Kernel geçerli ELF dosyası değil: $kernelPathResolved"
}
if ($kernelBytes[4] -ne 2 -or $kernelBytes[5] -ne 1) {
    throw "Kernel ELF64 little-endian değil: $kernelPathResolved"
}
if ([BitConverter]::ToUInt16($kernelBytes, 18) -ne 0x3e) {
    throw "Kernel x86-64 ELF makine tipinde değil: $kernelPathResolved"
}
$multibootHeader = Get-Multiboot2Header -Bytes $kernelBytes

if (-not $OutputPath) {
    $OutputPath = Join-Path $projectRoot "build\limine-bios\echOS-limine-bios-$Profile.iso"
}
$outputResolved = Resolve-ProjectPath $OutputPath
$outputParent = Split-Path -Parent $outputResolved
New-Item -ItemType Directory -Force -Path $outputParent | Out-Null

$buildRoot = [System.IO.Path]::GetFullPath((Join-Path $projectRoot "build"))
$stageRoot = [System.IO.Path]::GetFullPath((Join-Path $outputParent ("staging-" + $Profile)))
$buildPrefix = $buildRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $stageRoot.StartsWith($buildPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Stage yolu build kökü dışında: $stageRoot"
}
if (Test-Path -LiteralPath $stageRoot) {
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
}
$stageBoot = Join-Path $stageRoot "boot"
$stageLimine = Join-Path $stageBoot "limine"
$stageEfiBoot = Join-Path $stageRoot "EFI\BOOT"
New-Item -ItemType Directory -Force -Path $stageLimine, $stageEfiBoot | Out-Null

Copy-Item -LiteralPath $kernelPathResolved -Destination (Join-Path $stageBoot "ech_os") -Force
Copy-Item -LiteralPath $limineBiosCd -Destination (Join-Path $stageLimine "limine-bios-cd.bin") -Force
Copy-Item -LiteralPath $limineBiosSys -Destination (Join-Path $stageLimine "limine-bios.sys") -Force
Copy-Item -LiteralPath $limineUefiCd -Destination (Join-Path $stageLimine "limine-uefi-cd.bin") -Force
Copy-Item -LiteralPath $limineBootx64 -Destination (Join-Path $stageLimine "BOOTX64.EFI") -Force
Copy-Item -LiteralPath $limineBootx64 -Destination (Join-Path $stageEfiBoot "BOOTX64.EFI") -Force

$config = @"
timeout: 0
serial: yes

/echOS Limine BIOS smoke
    protocol: limine
    path: boot():/boot/ech_os
    cmdline: boot_tests=1
"@
$configPath = Join-Path $stageLimine "limine.conf"
[System.IO.File]::WriteAllText($configPath, $config, [System.Text.Encoding]::ASCII)
$configBytes = [System.IO.File]::ReadAllBytes($configPath)
if ($configBytes.Length -ge 3 -and $configBytes[0] -eq 0xef -and $configBytes[1] -eq 0xbb -and $configBytes[2] -eq 0xbf) {
    throw "Limine config UTF-8 BOM içeriyor"
}
if ((Get-ChildItem -LiteralPath $stageRoot -Recurse -File -Filter "limine.conf").Count -ne 1) {
    throw "Stage içinde tekil Limine config invariant'ı bozuldu"
}

$outputWsl = Convert-ToWslPath $outputResolved
$stageWsl = Convert-ToWslPath $stageRoot
$xorrisoArgs = @(
    "-as", "mkisofs",
    "-iso-level", "3",
    "-V", "ECHOS_LIMINE_BIOS",
    "-b", "boot/limine/limine-bios-cd.bin",
    "-no-emul-boot",
    "-boot-load-size", "4",
    "-boot-info-table",
    "--efi-boot", "boot/limine/limine-uefi-cd.bin",
    "-efi-boot-part",
    "--efi-boot-image",
    "--protective-msdos-label",
    "-o", $outputWsl,
    $stageWsl
)
Write-Host "Building Limine 12.5.2 BIOS/UEFI hybrid ISO..." -ForegroundColor Yellow
& wsl.exe -e xorriso @xorrisoArgs
if ($LASTEXITCODE -ne 0) {
    throw "xorriso Limine ISO üretimi başarısız"
}
if (-not (Test-Path -LiteralPath $outputResolved -PathType Leaf)) {
    throw "ISO üretilemedi: $outputResolved"
}

$manifestPath = [System.IO.Path]::ChangeExtension($outputResolved, ".json")
$manifest = [ordered]@{
    schema = 1
    profile = $Profile
    firmware = @("BIOS", "UEFI")
    limine_version = $limineVersion
    protocol = "limine"
    config_path = "/boot/limine/limine.conf"
    config_shadow_paths = @()
    kernel_path = "/boot/ech_os"
    kernel_source = $kernelPathResolved
    kernel_bytes = $kernelBytes.Length
    kernel_sha256 = (Get-FileHash -LiteralPath $kernelPathResolved -Algorithm SHA256).Hash.ToLowerInvariant()
    multiboot2_header_offset = if ($null -eq $multibootHeader) { $null } else { $multibootHeader.Offset }
    multiboot2_header_length = if ($null -eq $multibootHeader) { $null } else { $multibootHeader.Length }
    iso_path = $outputResolved
    iso_bytes = (Get-Item -LiteralPath $outputResolved).Length
    smbios_config_override = $false
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifestPath -Encoding UTF8

Write-Host ("Limine ISO hazır: {0}" -f $outputResolved) -ForegroundColor Green
if ($null -eq $multibootHeader) {
    Write-Host ("Kernel: {0} bytes, native Limine request image (no Multiboot2 header required)" -f $kernelBytes.Length) -ForegroundColor DarkGray
} else {
    Write-Host ("Kernel: {0} bytes, Multiboot2 header: 0x{1:x}" -f $kernelBytes.Length, $multibootHeader.Offset) -ForegroundColor DarkGray
}
Write-Host ("Config: /boot/limine/limine.conf (tekil, SMBIOS override yok)") -ForegroundColor DarkGray
