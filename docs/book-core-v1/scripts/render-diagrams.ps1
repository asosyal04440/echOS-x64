param(
    [switch]$SkipNpmInstall
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $PSCommandPath
$bookRoot = Split-Path -Parent $scriptDir
$figuresDir = Join-Path $bookRoot "figures"
$generatedDir = Join-Path $figuresDir "generated"

if (-not (Test-Path $generatedDir)) {
    New-Item -Path $generatedDir -ItemType Directory | Out-Null
}

if (-not $SkipNpmInstall -and -not (Test-Path (Join-Path $bookRoot "node_modules"))) {
    Push-Location $bookRoot
    try {
        npm install
    }
    finally {
        Pop-Location
    }
}

$mmdc = Join-Path $bookRoot "node_modules\.bin\mmdc.cmd"
if (-not (Test-Path $mmdc)) {
    throw "Mermaid CLI bulunamadi: $mmdc"
}

$sources = Get-ChildItem -Path $figuresDir -Filter "*.mmd" -File | Sort-Object Name
if ($sources.Count -eq 0) {
    Write-Host "Mermaid kaynak diyagram bulunamadi."
    exit 0
}

foreach ($src in $sources) {
    $dst = Join-Path $generatedDir ($src.BaseName + ".svg")
    Write-Host "[DIAGRAM] $($src.Name) -> $([System.IO.Path]::GetFileName($dst))"
    & $mmdc -i $src.FullName -o $dst -b transparent
    if (-not $?) {
        throw "Diyagram render basarisiz: $($src.FullName)"
    }
}

Write-Host "[DIAGRAM] Tamamlandi: $($sources.Count) dosya"
