param(
    [string]$OutputName = "echos-cilt1-core-v0.1.pdf",
    [switch]$SkipDiagramRender,
    [switch]$SkipNpmInstall,
    [ValidateSet("internals", "legacy")]
    [string]$Mode = "internals",
    [switch]$SkipAiScan
)

$ErrorActionPreference = "Stop"

function Resolve-PandocPath {
    $cmd = Get-Command pandoc -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Path
    }

    $candidate = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\JohnMacFarlane.Pandoc_Microsoft.Winget.Source_8wekyb3d8bbwe\pandoc-3.9.0.2\pandoc.exe"
    if (Test-Path $candidate) {
        return $candidate
    }

    throw "Pandoc bulunamadi. LUTFEN once pandoc kurun."
}

$scriptDir = Split-Path -Parent $PSCommandPath
$bookRoot = Split-Path -Parent $scriptDir
$outDir = Join-Path $bookRoot "out"

if (-not (Test-Path $outDir)) {
    New-Item -Path $outDir -ItemType Directory | Out-Null
}

if (-not $SkipDiagramRender) {
    $renderScript = Join-Path $scriptDir "render-diagrams.ps1"
    if (-not (Test-Path $renderScript)) {
        throw "Diyagram render script bulunamadi: $renderScript"
    }

    if ($SkipNpmInstall) {
        & $renderScript -SkipNpmInstall
    } else {
        & $renderScript
    }
}

$pandoc = Resolve-PandocPath
$outputFile = Join-Path $outDir $OutputName
$nodeScript = Join-Path $scriptDir "build_pdf.js"
$aiScanScript = Join-Path $scriptDir "ai_risk_scan.py"
$internalsForbiddenPattern = '(?i)\bhafta\b|\bhaftalik\b|\bweek\b|\bweekly\b'

Write-Host "[BUILD] Pandoc: $pandoc"
Write-Host "[BUILD] Node:   $(node --version)"
Write-Host "[BUILD] Mode:   $Mode"
Write-Host "[BUILD] Output: $outputFile"

Push-Location $bookRoot
try {
    node "$nodeScript" "$bookRoot" "$outputFile" "$Mode"

    if (-not $?) {
        throw "Node tabanli PDF build basarisiz oldu."
    }

    if ($Mode -eq "internals") {
        $htmlOutput = [System.IO.Path]::ChangeExtension($outputFile, ".html")
        if (-not (Test-Path $htmlOutput)) {
            throw "Internals terminology gate icin HTML cikti bulunamadi: $htmlOutput"
        }

        $termHits = Select-String -Path $htmlOutput -Pattern $internalsForbiddenPattern
        if ($termHits) {
            Write-Host "[BUILD] Internals terminology gate FAIL" -ForegroundColor Red
            $termHits | Select-Object -First 10 | ForEach-Object {
                Write-Host ("  {0}:{1}:{2}" -f $_.Path, $_.LineNumber, $_.Line.Trim())
            }
            throw "Internals modunda hafta/weekly ifadesi tespit edildi. Metni veya metadata'yi temizleyin."
        }

        Write-Host "[BUILD] Internals terminology gate: pass"
    }

    if (-not $SkipAiScan) {
        if (-not (Test-Path $aiScanScript)) {
            throw "AI risk scan script bulunamadi: $aiScanScript"
        }
        Write-Host "[BUILD] AI risk scan calistiriliyor..."
        python "$aiScanScript" "$outputFile"
        if (-not $?) {
            throw "AI risk scan 20% ustu sayfa tespit etti. out/ai-risk-report.txt dosyasini inceleyin."
        }
    }
}
finally {
    Pop-Location
}

Write-Host "[BUILD] Tamamlandi -> $outputFile"
