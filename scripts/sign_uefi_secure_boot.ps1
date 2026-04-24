param(
    [Parameter(Mandatory = $true)]
    [string]$UnsignedImage,

    [Parameter(Mandatory = $true)]
    [string]$SignedImage,

    [Parameter(Mandatory = $true)]
    [string]$Certificate,

    [Parameter(Mandatory = $true)]
    [string]$PrivateKey,

    [string]$IntermediateCertificate
)

$ErrorActionPreference = "Stop"

function Require-Tool {
    param([string]$Name)
    $tool = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $tool) {
        throw "Required tool '$Name' not found in PATH."
    }
    $tool.Source
}

function Require-Path {
    param([string]$PathValue, [string]$Label)
    $resolved = Resolve-Path -LiteralPath $PathValue -ErrorAction SilentlyContinue
    if (-not $resolved) {
        throw "$Label not found: $PathValue"
    }
    $resolved.Path
}

$sbsign = Require-Tool "sbsign"
$sbverify = Require-Tool "sbverify"
$unsigned = Require-Path $UnsignedImage "Unsigned image"
$cert = Require-Path $Certificate "Certificate"
$key = Require-Path $PrivateKey "Private key"
$intermediate = $null
if ($IntermediateCertificate) {
    $intermediate = Require-Path $IntermediateCertificate "Intermediate certificate"
}

$signedDir = Split-Path -Parent $SignedImage
if ($signedDir -and -not (Test-Path -LiteralPath $signedDir)) {
    New-Item -ItemType Directory -Path $signedDir -Force | Out-Null
}

$signArgs = @(
    "--key", $key,
    "--cert", $cert,
    "--output", $SignedImage
)
if ($intermediate) {
    $signArgs += @("--addcert", $intermediate)
}
$signArgs += $unsigned

& $sbsign @signArgs
if ($LASTEXITCODE -ne 0) {
    throw "sbsign failed with exit code $LASTEXITCODE"
}

& $sbverify "--cert" $cert $SignedImage
if ($LASTEXITCODE -ne 0) {
    throw "sbverify failed with exit code $LASTEXITCODE"
}

Write-Output "Secure Boot signed image created: $SignedImage"
