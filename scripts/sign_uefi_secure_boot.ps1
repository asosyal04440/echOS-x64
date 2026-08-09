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
    $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
    if ($wsl) {
        $wslTool = & $wsl.Source -e sh -lc "command -v $Name" 2>$null
        if ($LASTEXITCODE -eq 0 -and $wslTool) {
            return [pscustomobject]@{
                FilePath = $wsl.Source
                Prefix = @("-e", $Name)
                Wsl = $true
            }
        }
    }
    $tool = Get-Command $Name -ErrorAction SilentlyContinue
    if ($tool) {
        return [pscustomobject]@{
            FilePath = $tool.Source
            Prefix = @()
            Wsl = $false
        }
    }
    throw "Required tool '$Name' not found in native PATH or WSL."
}

function Invoke-CheckedTool {
    param($Tool, [string[]]$Arguments)
    $toolArgs = @()
    foreach ($argument in $Arguments) {
        if ($Tool.Wsl -and $argument -match '^[A-Za-z]:\\') {
            $converted = & wsl.exe -e wslpath -a -u -- $argument 2>&1
            if ($LASTEXITCODE -ne 0) {
                throw "WSL path conversion failed: $argument"
            }
            $toolArgs += ($converted | Select-Object -Last 1).ToString().Trim()
        } else {
            $toolArgs += $argument
        }
    }
    & $Tool.FilePath @($Tool.Prefix + $toolArgs)
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $($Tool.FilePath) $($Arguments -join ' ')"
    }
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
$signedOutput = [System.IO.Path]::GetFullPath($SignedImage)

$signArgs = @(
    "--key", $key,
    "--cert", $cert,
    "--output", $signedOutput
)
if ($intermediate) {
    $signArgs += @("--addcert", $intermediate)
}
$signArgs += $unsigned

Invoke-CheckedTool $sbsign $signArgs
Invoke-CheckedTool $sbverify @("--cert", $cert, $signedOutput)

Write-Output "Secure Boot signed image created: $signedOutput"
