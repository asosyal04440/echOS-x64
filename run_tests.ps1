#!/usr/bin/env pwsh
# run_tests.ps1 — echOS Host-Side Test Runner
#
# Kullanım:
#   .\run_tests.ps1                        # Tüm testleri koştur
#   .\run_tests.ps1 -Filter integration    # Belirli test suite'ini koştur
#   .\run_tests.ps1 -Verbose               # Ayrıntılı çıktı
#   .\run_tests.ps1 -NoCapture             # Test çıktısını yakalamasız göster
#
# Notlar:
#   - Testler host ortamında (x86_64-pc-windows-msvc) derlenir ve çalıştırılır.
#   - `cargo test` (hedef belirtilmeksizin) bare-metal hedefi (x86_64-unknown-none)
#     kullanır; bu hedefte std ve test crate'leri yoktur. Bunun yerine bu script
#     veya `cargo test-host` alias'ı kullanın.
#   - heap_stack_* testleri harness=false ile standalone binary olarak çalışır;
#     çıkış kodu 0 = başarı, sıfır dışı = başarısız.

param(
    [string]$Filter    = "",
    [switch]$Verbose,
    [switch]$NoCapture,
    [switch]$Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ─── Renkli çıktı yardımcıları ────────────────────────────────────────────────

function Write-Header([string]$msg) {
    Write-Host ""
    Write-Host "══════════════════════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host "  $msg" -ForegroundColor Cyan
    Write-Host "══════════════════════════════════════════════════════════" -ForegroundColor Cyan
}

function Write-Step([string]$msg) {
    Write-Host "  ▶ $msg" -ForegroundColor Yellow
}

function Write-Pass([string]$msg) {
    Write-Host "  ✓ $msg" -ForegroundColor Green
}

function Write-Fail([string]$msg) {
    Write-Host "  ✗ $msg" -ForegroundColor Red
}

function Write-Info([string]$msg) {
    Write-Host "    $msg" -ForegroundColor Gray
}

# ─── Yardım ───────────────────────────────────────────────────────────────────

if ($Help) {
    Write-Host @"
echOS Host-Side Test Runner
Kullanım: .\run_tests.ps1 [seçenekler]

Seçenekler:
  -Filter <isim>   Yalnızca ismi bu metni içeren test suite'lerini koştur
  -Verbose         cargo test --nocapture (test println çıktıları görünür)
  -NoCapture       Her test için --nocapture bayrağını ekle
  -Help            Bu yardım mesajını göster

Örnekler:
  .\run_tests.ps1
  .\run_tests.ps1 -Filter regression
  .\run_tests.ps1 -Filter integration -Verbose
  .\run_tests.ps1 -Filter heap_stack_phys_addr_bug_test
"@
    exit 0
}

# ─── Proje kökünü bul ─────────────────────────────────────────────────────────

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargoToml = Join-Path $scriptDir "Cargo.toml"

if (-not (Test-Path $cargoToml)) {
    Write-Fail "Cargo.toml bulunamadı: $cargoToml"
    Write-Info "Bu script echOS proje kökünden çalıştırılmalıdır."
    exit 1
}

Set-Location $scriptDir

# ─── Araç zinciri kontrolü ────────────────────────────────────────────────────

Write-Header "echOS Test Runner — Host Ortamı"

Write-Step "Rust araç zinciri kontrol ediliyor..."
try {
    $rustcVer = (rustc --version) 2>&1
    $cargoVer = (cargo --version) 2>&1
    Write-Info "$rustcVer"
    Write-Info "$cargoVer"
} catch {
    Write-Fail "rustc/cargo bulunamadı. rustup kurulu olduğundan emin olun."
    exit 1
}

# Target varlık kontrolü
Write-Step "x86_64-pc-windows-msvc target kontrol ediliyor..."
$targetList = (rustup target list --installed) 2>&1
if ($targetList -notmatch "x86_64-pc-windows-msvc") {
    Write-Step "Target eksik; yükleniyor..."
    rustup target add x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "Target yüklenemedi. 'rustup target add x86_64-pc-windows-msvc' el ile deneyin."
        exit 1
    }
}
Write-Pass "x86_64-pc-windows-msvc mevcut."

# ─── Test tanımları ───────────────────────────────────────────────────────────
# Her giriş: @{ Name; Description; Harness }
# Harness=$false → standalone fn main() binary (heap_stack_*)
# Harness=$true  → standart #[test] harness (integration_suite, regression_suite)

$allTests = @(
    @{
        Name        = "heap_stack_phys_addr_bug_test"
        Description = "Heap/stack fiziksel adres dönüşümü hata keşif testi (standalone binary)"
        Harness     = $false
    },
    @{
        Name        = "heap_stack_preservation_test"
        Description = "HHDM yığıt doğrudan dönüşüm koruma testi (standalone binary)"
        Harness     = $false
    },
    @{
        Name        = "integration_suite"
        Description = "Çapraz-alt-sistem entegrasyon senaryoları (ext4/NVMe, TCP/NIC, USB/FAT32, eBPF, container)"
        Harness     = $true
    },
    @{
        Name        = "regression_suite"
        Description = "Regresyon + stress suite (SPSC, RCU, EEVDF, EAS, PSI, MGLRU, WiFi MLO, Btrfs, io_uring)"
        Harness     = $true
    }
)

# ─── Filtre uygula ────────────────────────────────────────────────────────────

$testsToRun = $allTests
if ($Filter -ne "") {
    $testsToRun = $allTests | Where-Object { $_.Name -like "*$Filter*" }
    if ($testsToRun.Count -eq 0) {
        Write-Fail "Filtre '$Filter' hiçbir test suite'iyle eşleşmedi."
        Write-Info "Mevcut suite'ler: $($allTests | ForEach-Object { $_.Name } | Join-String -Separator ', ')"
        exit 1
    }
}

Write-Info "Koşturulacak suite sayısı: $($testsToRun.Count)"

# ─── İnşa aşaması ─────────────────────────────────────────────────────────────

Write-Header "Derleme — x86_64-pc-windows-msvc"
Write-Step "cargo check --target x86_64-pc-windows-msvc (hızlı doğrulama)..."

$checkArgs = @("check", "--target", "x86_64-pc-windows-msvc")
foreach ($suite in $testsToRun) {
    $checkArgs += "--test"
    $checkArgs += $suite.Name
}

& cargo @checkArgs 2>&1 | ForEach-Object {
    if ($_ -match "^error") {
        Write-Fail $_
    } elseif ($Verbose -and ($_ -match "^warning|Compiling|Checking|Finished")) {
        Write-Info $_
    }
}

if ($LASTEXITCODE -ne 0) {
    Write-Fail "Derleme hatası. Testler koşturulmuyor."
    exit $LASTEXITCODE
}

Write-Pass "Derleme başarılı."

# ─── Test koşturma ────────────────────────────────────────────────────────────

Write-Header "Testler Koşturuluyor"

$passCount  = 0
$failCount  = 0
$skipCount  = 0
$results    = @()
$startTotal = Get-Date

foreach ($suite in $testsToRun) {
    $name = $suite.Name
    $desc = $suite.Description

    Write-Step "$name"
    Write-Info "$desc"

    $testArgs = @(
        "test",
        "--target", "x86_64-pc-windows-msvc",
        "--test", $name
    )

    if ($NoCapture -or $Verbose) {
        $testArgs += "--"
        $testArgs += "--nocapture"
    }

    $startTime = Get-Date

    if ($Verbose) {
        # Canlı çıktı — pipe yok
        & cargo @testArgs
        $exitCode = $LASTEXITCODE
    } else {
        # Çıktıyı yakala; yalnızca hata varsa göster
        $output = & cargo @testArgs 2>&1
        $exitCode = $LASTEXITCODE
    }

    $elapsed = ((Get-Date) - $startTime).TotalSeconds

    if ($exitCode -eq 0) {
        Write-Pass "$name geçti  ($([math]::Round($elapsed, 2))s)"
        $passCount++
        $results += [PSCustomObject]@{ Suite = $name; Status = "PASS"; Seconds = $elapsed }
    } else {
        Write-Fail "$name başarısız  ($([math]::Round($elapsed, 2))s)"
        if (-not $Verbose -and $output) {
            Write-Host ""
            Write-Host "─── Hata çıktısı: $name ───" -ForegroundColor Red
            $output | Where-Object { $_ -match "error|FAILED|panicked|thread" } |
                Select-Object -First 40 |
                ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
            Write-Host ""
        }
        $failCount++
        $results += [PSCustomObject]@{ Suite = $name; Status = "FAIL"; Seconds = $elapsed }
    }
}

# ─── Özet ─────────────────────────────────────────────────────────────────────

$totalElapsed = ((Get-Date) - $startTotal).TotalSeconds

Write-Header "Özet"

foreach ($r in $results) {
    if ($r.Status -eq "PASS") {
        Write-Pass "$($r.Suite)  ($([math]::Round($r.Seconds, 2))s)"
    } else {
        Write-Fail "$($r.Suite)  ($([math]::Round($r.Seconds, 2))s)"
    }
}

Write-Host ""
Write-Host ("  Toplam: {0} suite, {1} geçti, {2} başarısız  ({3}s)" -f
    ($passCount + $failCount), $passCount, $failCount,
    [math]::Round($totalElapsed, 2)) -ForegroundColor White

if ($failCount -gt 0) {
    Write-Host ""
    Write-Fail "$failCount suite başarısız."
    Write-Info "Ayrıntılar için: .\run_tests.ps1 -Filter <suite-adı> -Verbose"
    exit 1
} else {
    Write-Host ""
    Write-Pass "Tüm testler geçti."
    exit 0
}
