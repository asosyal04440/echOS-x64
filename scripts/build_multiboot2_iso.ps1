[CmdletBinding()]
param(
    [ValidateSet("debug", "release")][string]$Profile = "debug",
    [string]$KernelPath = "",
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
if (-not $KernelPath) {
    $args = if ($Profile -eq "release") { @("--release") } else { @() }
    $previousLinker = $env:ECHOS_KERNEL_LINKER
    try {
        $env:ECHOS_KERNEL_LINKER = "linker.ld"
        & cargo build-kernel @args
        if ($LASTEXITCODE -ne 0) { throw "MB2 kernel build başarısız" }
    } finally {
        if ($null -eq $previousLinker) { Remove-Item Env:ECHOS_KERNEL_LINKER -ErrorAction SilentlyContinue }
        else { $env:ECHOS_KERNEL_LINKER = $previousLinker }
    }
    $KernelPath = Join-Path $root ("target\x86_64-unknown-none\{0}\ech_os" -f $Profile)
}
$kernel = [System.IO.Path]::GetFullPath($KernelPath)
if (-not (Test-Path -LiteralPath $kernel -PathType Leaf)) { throw "Kernel bulunamadı: $kernel" }

function Assert-Multiboot2Header([string]$Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $scanLength = [Math]::Min($bytes.Length, 32768)
    $magic = [byte[]](0xD6, 0x50, 0x52, 0xE8)
    for ($offset = 0; $offset + 16 -le $scanLength; $offset += 8) {
        if ($bytes[$offset] -ne $magic[0] -or $bytes[$offset + 1] -ne $magic[1] -or
            $bytes[$offset + 2] -ne $magic[2] -or $bytes[$offset + 3] -ne $magic[3]) { continue }
        $length = [BitConverter]::ToUInt32($bytes, $offset + 8)
        if ($length -lt 16 -or $offset + $length -gt $bytes.Length) { continue }
        $sum = [uint64]0
        for ($word = 0; $word -lt 16; $word += 4) {
            $sum = ($sum + [uint64]([BitConverter]::ToUInt32($bytes, $offset + $word))) % 4294967296
        }
        if ($sum -eq 0) { return }
    }
    throw "Kernel Multiboot2 header doğrulaması başarısız: $Path"
}
Assert-Multiboot2Header $kernel
if (-not $OutputPath) { $OutputPath = Join-Path $root ("build\multiboot2\echOS-multiboot2-{0}.iso" -f $Profile) }
$output = [System.IO.Path]::GetFullPath($OutputPath)
$parent = Split-Path -Parent $output
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$stage = Join-Path $parent ("staging-{0}" -f $Profile)
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
$grub = Join-Path $stage "boot\grub"
New-Item -ItemType Directory -Force -Path $grub | Out-Null
Copy-Item -LiteralPath (Join-Path $root "multiboot_iso\boot\grub\grub.cfg") -Destination (Join-Path $grub "grub.cfg")
Copy-Item -LiteralPath $kernel -Destination (Join-Path $stage "boot\ech_os")

function WslPath([string]$Path) {
    $r = & wsl.exe -e wslpath -a -u -- $Path 2>&1
    if ($LASTEXITCODE -ne 0) { throw "WSL path çevrilemedi: $Path" }
    ($r | Select-Object -Last 1).ToString().Trim()
}
$wslStage = WslPath $stage
$wslOutput = WslPath $output
& wsl.exe -e grub-mkrescue -o $wslOutput $wslStage
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $output -PathType Leaf)) {
    throw "grub-mkrescue MB2 ISO üretmedi"
}
$manifest = [ordered]@{
    schema = 1
    profile = $Profile
    firmware = @("BIOS")
    protocol = "multiboot2"
    config_path = "/boot/grub/grub.cfg"
    kernel_path = "/boot/ech_os"
    kernel_source = $kernel
    kernel_sha256 = (Get-FileHash -LiteralPath $kernel -Algorithm SHA256).Hash.ToLowerInvariant()
    iso_path = $output
    iso_bytes = (Get-Item -LiteralPath $output).Length
    smbios_config_override = $false
}
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath ([System.IO.Path]::ChangeExtension($output, ".json")) -Encoding UTF8
Write-Host "Multiboot2 ISO hazır: $output" -ForegroundColor Green
