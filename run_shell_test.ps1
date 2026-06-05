param(
    [string]$TestName = "basic",
    [switch]$Rebuild,
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = "Stop"
$projectRoot = (Get-Location).Path
$logDir = Join-Path $projectRoot "logs"
if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$serialLogPath = Join-Path $logDir ("test_serial_" + $stamp + ".log")
$inputPath = Join-Path $logDir ("test_input_" + $stamp + ".txt")

$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        throw "QEMU bulunamadı"
    }
}

$efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
if ($Rebuild -or -not (Test-Path $efiPath)) {
    Write-Host "Building echOS UEFI..." -ForegroundColor Yellow
    cargo build --quiet --target x86_64-unknown-uefi
    if ($LASTEXITCODE -ne 0) { throw "UEFI build failed" }
}

$ovmfShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $ovmfShare "edk2-x86_64-code.fd"
$ovmfVarsTemplate = Join-Path $ovmfShare "edk2-i386-vars.fd"
$artifactDir = Join-Path $projectRoot "build\appliance"
if (-not (Test-Path $artifactDir)) { New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null }
$ovmfVars = Join-Path $artifactDir "OVMF_VARS.fd"
if (-not (Test-Path $ovmfVars)) {
    Copy-Item $ovmfVarsTemplate $ovmfVars -Force
}

Write-Host "Test: $TestName" -ForegroundColor Cyan
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray

$testCases = @{
    "basic" = @(
        'echo "hello world"',
        'exit'
    )
    "arith" = @(
        'echo $((1+2))',
        'echo $((10*5))',
        'echo $((100/10))',
        'echo $((10-3))',
        'exit'
    )
    "var" = @(
        'X=42',
        'echo $X',
        'echo "value is $X"',
        'exit'
    )
    "if" = @(
        'if [ 1 -eq 1 ]; then echo "true"; fi',
        'if [ 1 -eq 2 ]; then echo "wrong"; else echo "else"; fi',
        'exit'
    )
    "loop" = @(
        'for i in 1 2 3; do echo $i; done',
        'exit'
    )
    "sub" = @(
        'echo "result: $(echo inner)"',
        'exit'
    )
    "array" = @(
        'arr=(a b c)',
        'echo ${arr[0]}',
        'echo ${arr[1]}',
        'echo ${arr[2]}',
        'exit'
    )
    "redir" = @(
        'echo "test" > /tmp/test.txt',
        'cat /tmp/test.txt',
        'echo "append" >> /tmp/test.txt',
        'cat /tmp/test.txt',
        'exit'
    )
    "pipe" = @(
        'echo "hello world" | wc -w',
        'echo "line1" | cat',
        'exit'
    )
    "brace" = @(
        'echo {a,b,c}',
        'echo {1..5}',
        'echo {a..e}',
        'exit'
    )
}

$commands = $testCases[$TestName]
if (-not $commands) {
    Write-Host "Unknown test: $TestName" -ForegroundColor Red
    Write-Host "Available tests: $($testCases.Keys -join ', ')" -ForegroundColor Yellow
    exit 1
}

$scriptContent = $commands -join "`n"
Set-Content -LiteralPath $inputPath -Value $scriptContent -Encoding ASCII

Write-Host "Commands:" -ForegroundColor DarkGray
$commands | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }

$displayArgs = @("-display", "none")

$proc = Start-Process -FilePath $qemu -ArgumentList @(
    "-bios", $efiPath,
    "-machine", "q35",
    "-cpu", "Haswell,+smep,+smap",
    "-smp", "sockets=1,cores=1,threads=1",
    "-m", "256M",
    "-drive", "if=pflash,format=raw,readonly=on,file=$ovmfCode",
    "-drive", "if=pflash,format=raw,file=$ovmfVars",
    "-serial", "file:$serialLogPath",
    "-debugcon", "file:$(Join-Path $logDir ("test_debugcon_" + $stamp + ".log"))",
    "-global", "isa-debugcon.iobase=0xe9",
    "-monitor", "none",
    "-no-reboot",
    "-no-shutdown"
) + $displayArgs + @(
    "-accel", "whpx,kernel-irqchip=off",
    "-accel", "tcg"
) -PassThru

Write-Host "QEMU started (PID: $($proc.Id))" -ForegroundColor DarkGray

$deadline = (Get-Date).AddSeconds($TimeoutSec)
$bootComplete = $false
$shellReady = $false

while ((Get-Date) -lt $deadline -and -not $proc.HasExited) {
    if (Test-Path $serialLogPath) {
        $content = Get-Content $serialLogPath -Raw -ErrorAction SilentlyContinue
        if ($content) {
            if ($content.Contains("[SHELL] Starting Ring 3 shell...")) {
                $bootComplete = $true
            }
            if ($content.Contains("echshell> ") -or $content.Contains("$ ")) {
                $shellReady = $true
                break
            }
        }
    }
    Start-Sleep -Milliseconds 250
}

if (-not $shellReady) {
    Write-Host "Shell not ready within timeout" -ForegroundColor Red
    if (Test-Path $serialLogPath) {
        Write-Host "Serial output:" -ForegroundColor DarkGray
        Get-Content $serialLogPath -Tail 20 | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
    }
    $proc.Kill()
    exit 1
}

Write-Host "Shell ready, sending commands..." -ForegroundColor Green

$results = @{}
$testPassed = 0
$testFailed = 0

foreach ($cmd in $commands) {
    if ($cmd -eq "exit") { continue }
    
    Write-Host "`n> $cmd" -ForegroundColor Yellow
    
    $expectedOutput = $null
    if ($cmd -match '^echo\s+"([^"]+)"$') {
        $expectedOutput = $Matches[1]
    } elseif ($cmd -match "^echo\s+'([^']+)'$") {
        $expectedOutput = $Matches[1]
    } elseif ($cmd -match '^echo\s+\$\(\((.+)\)\)$') {
        $expr = $Matches[1]
        try {
            $expectedOutput = [int]([eval] $expr)
        } catch {
            $expectedOutput = "ERROR"
        }
    }
    
    Start-Sleep -Milliseconds 500
    
    if (Test-Path $serialLogPath) {
        $newContent = Get-Content $serialLogPath -Raw -ErrorAction SilentlyContinue
        if ($newContent) {
            $lines = $newContent -split "`n" | Where-Object { $_ -match '^\d+\s+' }
            if ($lines.Count -gt 0) {
                $lastLine = $lines[-1]
                Write-Host "  Output: $lastLine" -ForegroundColor Gray
            }
        }
    }
}

Write-Host "`nTest completed" -ForegroundColor Green

$proc.Kill()

Write-Host "`nFinal serial output:" -ForegroundColor DarkGray
if (Test-Path $serialLogPath) {
    Get-Content $serialLogPath -Tail 30 | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
}
