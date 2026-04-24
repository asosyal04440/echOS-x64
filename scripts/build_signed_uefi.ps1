param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,

    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [string]$SignedImage = ".\artifacts\secure_boot\BOOTX64.EFI",
    [string]$UnsignedImage
)

$ErrorActionPreference = "Stop"

function Resolve-RequiredPath {
    param([string]$PathValue, [string]$Label)
    $resolved = Resolve-Path -LiteralPath $PathValue -ErrorAction SilentlyContinue
    if (-not $resolved) {
        throw "$Label not found: $PathValue"
    }
    $resolved.Path
}

function Resolve-UefiArtifact {
    param([string]$ProfileName)

    $preferred = Join-Path $PWD "target\x86_64-unknown-uefi\$ProfileName\ech_os.efi"
    if (Test-Path -LiteralPath $preferred) {
        return (Resolve-Path -LiteralPath $preferred).Path
    }

    $candidate = Get-ChildItem "target\x86_64-unknown-uefi\$ProfileName" -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -eq ".efi" -and $_.BaseName -like "ech_os*" } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1

    if (-not $candidate) {
        throw "No UEFI artifact found under target\\x86_64-unknown-uefi\\$ProfileName"
    }
    $candidate.FullName
}

$bundle = Resolve-RequiredPath $BundleDir "Bundle directory"
$dbCert = Resolve-RequiredPath (Join-Path $bundle "db.crt") "db certificate"
$dbKey = Resolve-RequiredPath (Join-Path $bundle "db.key") "db private key"

if (-not $UnsignedImage) {
    $cargoArgs = @("build", "--target", "x86_64-unknown-uefi", "--bin", "ech_os")
    if ($Profile -eq "release") {
        $cargoArgs += "--release"
    }
    cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
    $UnsignedImage = Resolve-UefiArtifact $Profile
} else {
    $UnsignedImage = Resolve-RequiredPath $UnsignedImage "Unsigned image"
}

& (Join-Path $PSScriptRoot "sign_uefi_secure_boot.ps1") `
    -UnsignedImage $UnsignedImage `
    -SignedImage $SignedImage `
    -Certificate $dbCert `
    -PrivateKey $dbKey

if ($LASTEXITCODE -ne 0) {
    throw "Signing wrapper failed."
}

Write-Output "Signed echOS UEFI artifact: $SignedImage"
