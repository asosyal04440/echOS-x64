param(
    [ValidateSet("auto", "iso", "uefi")]
    [string]$Mode = "auto",
    [ValidateSet("fast", "debug")]
    [string]$Profile = "fast",
    [int]$DisplayWidth = 1920,
    [int]$DisplayHeight = 1080,
    [int]$CpuCount = 0,
    [switch]$RebuildIso,
    [switch]$NoBuild,
    [switch]$CreateOnly,
    [ValidateSet("none", "system_a", "system_b", "recovery")]
    [string]$PendingSlot = "none",
    [int]$ResetAfterSeconds = 0,
    [switch]$NoAutoLogin,
    [switch]$Headless,
    [switch]$ForceVarsReset,
    [switch]$SuspendResumeSmoke
)

$ErrorActionPreference = "Stop"

function Wait-FileMarker {
    param(
        [string]$Path,
        [string]$Marker,
        [int]$TimeoutSec
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path) {
            $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
            if ($content -and $content.Contains($Marker)) {
                return $true
            }
        }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Send-MonitorCommand {
    param(
        [string]$MonitorHost,
        [int]$Port,
        [string]$Command
    )

    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        $client.Connect($MonitorHost, $Port)
        $stream = $client.GetStream()
        $writer = [System.IO.StreamWriter]::new($stream)
        $writer.NewLine = "`n"
        $writer.AutoFlush = $true
        Start-Sleep -Milliseconds 200
        $writer.WriteLine($Command)
        Start-Sleep -Milliseconds 200
        $writer.Dispose()
    } finally {
        $client.Dispose()
    }
}

try { taskkill /IM qemu-system-x86_64.exe /F 2>$null | Out-Null } catch {}

Write-Host "echOS QEMU Appliance" -ForegroundColor Cyan
Write-Host "====================`n" -ForegroundColor Cyan

$projectRoot = (Get-Location).Path
$logDir = Join-Path $projectRoot "logs"
$artifactDir = Join-Path $projectRoot "build\appliance"
if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
if (-not (Test-Path $artifactDir)) { New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null }

$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$logPath = Join-Path $logDir ("qemu_" + $stamp + ".log")
$serialLogPath = Join-Path $logDir ("serial_" + $stamp + ".log")
$debugLogPath = Join-Path $logDir ("debugcon_" + $stamp + ".log")
$traceLogPath = Join-Path $logDir ("qemu_trace_" + $stamp + ".log")
$qemuStdoutPath = Join-Path $logDir ("qemu_stdout_" + $stamp + ".log")
$qemuStderrPath = Join-Path $logDir ("qemu_stderr_" + $stamp + ".log")

$transcriptStarted = $false
try {
    Start-Transcript -Path $logPath | Out-Null
    $transcriptStarted = $true
} catch {}

Write-Host "Log: $logPath" -ForegroundColor DarkGray
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray
Write-Host "Profile: $Profile" -ForegroundColor DarkGray

$traceEnabled = $Profile -eq "debug"
$fastProfile = $Profile -eq "fast"
$accelArgs = if ($fastProfile) { @("-accel", "tcg") } else { @("-accel", "whpx", "-accel", "tcg") }
$videoArgs = @(
    "-vga", "none",
    "-device", "VGA,xres=$DisplayWidth,yres=$DisplayHeight,edid=on"
)
$hostCpuCount = [Environment]::ProcessorCount
$defaultFastCpuCount = [Math]::Min([Math]::Max($hostCpuCount, 1), 4)
$qemuCpuCount = if ($CpuCount -gt 0) {
    $CpuCount
} elseif ($fastProfile) {
    $defaultFastCpuCount
} else {
    [Math]::Max($hostCpuCount, 1)
}
Write-Host "vCPUs: $qemuCpuCount" -ForegroundColor DarkGray

$llvmBin = "C:\Program Files\LLVM\bin"
if (Test-Path $llvmBin) {
    $env:PATH = "$llvmBin;$env:PATH"
    if (-not $env:CC -or $env:CC -eq "") { $env:CC = "clang" }
    if (-not $env:CC_x86_64_unknown_none -or $env:CC_x86_64_unknown_none -eq "") { $env:CC_x86_64_unknown_none = "clang" }
}
$env:CARGO_INCREMENTAL = "0"

$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        throw "QEMU bulunamadı"
    }
}

$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) {
    throw "python bulunamadı"
}

$isoPath = Join-Path $projectRoot "echOS_multiboot.iso"
$kernelPath = Join-Path $projectRoot "target\x86_64-unknown-none\debug\ech_os"
$isoKernelPath = Join-Path $projectRoot "multiboot_iso\boot\ech_os"
$useIso = $false
if ($Mode -eq "iso") { $useIso = $true }
elseif ($Mode -eq "uefi") { $useIso = $false }
else { $useIso = Test-Path $isoPath }

if ($useIso) {
    if (-not $NoBuild) {
        Write-Host "Building echOS (multiboot)..." -ForegroundColor Yellow
        cargo build --quiet
        if ($LASTEXITCODE -ne 0) { throw "Multiboot build failed" }
        if (-not (Test-Path $kernelPath)) { throw "Kernel bulunamadı: $kernelPath" }
        Copy-Item $kernelPath $isoKernelPath -Force
    }

    if ($RebuildIso -or -not (Test-Path $isoPath)) {
        throw "ISO rebuild yolu WSL gerektiriyor; bu trust-building fazında UEFI appliance kullanın."
    }

    $displayArgs = if ($Headless) {
        @("-display", "none")
    } else {
        @("-display", "gtk,grab-on-hover=on,zoom-to-fit=on")
    }
    $qemuArgs = @(
        "-cdrom", $isoPath,
        "-machine", "q35",
        "-m", "512M",
        "-serial", "file:$serialLogPath",
        "-debugcon", "file:$debugLogPath",
        "-global", "isa-debugcon.iobase=0xe9",
        "-monitor", "none",
        "-no-reboot",
        "-no-shutdown"
    ) + $displayArgs + $videoArgs + $accelArgs
    if ($traceEnabled) {
        $qemuArgs += @("-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset", "-D", $traceLogPath)
    }
} else {
    $efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
    $imagePath = Join-Path $artifactDir "echos_vm.raw"
    $manifestPath = Join-Path $artifactDir "echos_vm.json"
    $builderPath = Join-Path $projectRoot "scripts\build_vm_appliance.py"

    $qemuShare = "C:\Program Files\qemu\share"
    $ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
    $ovmfVarsTemplate = Join-Path $qemuShare "edk2-i386-vars.fd"
    $ovmfVars = Join-Path $artifactDir "OVMF_VARS.fd"

    if (-not $NoBuild) {
        Write-Host "Building echOS (UEFI)..." -ForegroundColor Yellow
        cargo build --quiet --target x86_64-unknown-uefi
        if ($LASTEXITCODE -ne 0) { throw "UEFI build failed" }
    }
    if (-not (Test-Path $efiPath)) { throw "EFI binary not found at $efiPath" }

    Write-Host "Building raw GPT appliance disk..." -ForegroundColor Yellow
    $builderArgs = @(
        $builderPath,
        "--efi", $efiPath,
        "--output", $imagePath,
        "--active-slot", "system_a",
        "--pending-slot", $PendingSlot
    )
    if (-not $NoAutoLogin) {
        $builderArgs += "--auto-login"
    }
    if ($SuspendResumeSmoke) {
        $builderArgs += "--suspend-resume-smoke"
    }
    & $python.Source @builderArgs
    if ($LASTEXITCODE -ne 0) { throw "Appliance image build failed" }

    Write-Host "Disk image: $imagePath" -ForegroundColor DarkGray
    Write-Host "Manifest: $manifestPath" -ForegroundColor DarkGray
    if ($CreateOnly) {
        if ($transcriptStarted) { Stop-Transcript | Out-Null }
        exit 0
    }

    if ($ForceVarsReset -and (Test-Path $ovmfVars)) {
        Remove-Item $ovmfVars -Force
    }
    if (-not (Test-Path $ovmfVars)) {
        Copy-Item $ovmfVarsTemplate $ovmfVars -Force
    }

    $displayArgs = if ($Headless) {
        @("-display", "none")
    } else {
        @("-display", "gtk,grab-on-hover=on,zoom-to-fit=on")
    }
    $monitorHost = "127.0.0.1"
    $monitorPort = 45454
    $monitorEndpoint = "tcp:${monitorHost}:${monitorPort},server,nowait"
    $ovmfCodeDrive = "if=pflash,format=raw,readonly=on,file=`"$ovmfCode`""
    $ovmfVarsDrive = "if=pflash,format=raw,file=`"$ovmfVars`""
    $qemuArgs = @(
        "-machine", "q35",
        "-cpu", "Haswell,+smep,+smap,+pcid",
        "-smp", "$qemuCpuCount",
        "-drive", $ovmfCodeDrive,
        "-drive", $ovmfVarsDrive,
        "-drive", "file=$imagePath,if=virtio,format=raw",
        "-debugcon", "file:$debugLogPath",
        "-global", "isa-debugcon.iobase=0xE9",
        "-serial", "file:$serialLogPath",
        "-m", "2G",
        "-monitor", $monitorEndpoint,
        "-no-reboot",
        "-no-shutdown",
        "-netdev", "user,id=net0,hostfwd=tcp::8080-:80,hostfwd=tcp::4443-:443",
        "-device", "virtio-net-pci,netdev=net0,disable-modern=off,disable-legacy=on"
    ) + $displayArgs + $videoArgs + $accelArgs
    if ($traceEnabled) {
        $qemuArgs += @("-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset", "-D", $traceLogPath)
    }
}

Write-Host "Launching QEMU...`n" -ForegroundColor Yellow
$proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath

if (-not $useIso) {
    $successMarkers = @(
        "[BOOTCTRL] stage=boot-control-loaded",
        "[BOOTCTRL] stage=kernel-core-ready",
        "[BOOTCTRL] stage=storage-mounted",
        "[BOOTCTRL] stage=display-ready"
    )
    if (-not $NoAutoLogin) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[DESKTOP] session bootstrap step=login-visible" -TimeoutSec 90)) {
            try { $proc.Kill() } catch {}
            throw "Login screen marker görülmedi"
        }
        $successMarkers += @(
            "[BOOTCTRL] stage=desktop-ready",
            "[BOOTCTRL] stage=app-basket-ready",
            "[BOOTCTRL] success"
        )
    }

    if ($ResetAfterSeconds -gt 0) {
        Start-Sleep -Seconds $ResetAfterSeconds
        if (-not $proc.HasExited) {
            Write-Host "Injecting hard reset after $ResetAfterSeconds seconds" -ForegroundColor DarkYellow
            try { $proc.Kill() } catch {}
        }
    } elseif ($SuspendResumeSmoke) {
        if ($NoAutoLogin) {
            try { $proc.Kill() } catch {}
            throw "Suspend/resume smoke auto-login gerektirir"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SMOKE] suspend-resume arm" -TimeoutSec 120)) {
            try { $proc.Kill() } catch {}
            throw "Suspend/resume smoke arm marker not observed"
        }
        Start-Sleep -Seconds 2
        Send-MonitorCommand -MonitorHost $monitorHost -Port $monitorPort -Command "system_wakeup"
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SMOKE] suspend-resume ok" -TimeoutSec 120)) {
            try { $proc.Kill() } catch {}
            throw "Suspend/resume smoke did not complete"
        }
        if ($Headless -and -not $proc.HasExited) {
            try { $proc.Kill() } catch {}
        }
    } elseif (-not $NoAutoLogin) {
        if (Wait-FileMarker -Path $serialLogPath -Marker "[BOOTCTRL] success" -TimeoutSec 90) {
            if ($Headless -and -not $proc.HasExited) {
                try { $proc.Kill() } catch {}
            }
        }
    }
}

$proc.WaitForExit()

Write-Host "`nQEMU exited." -ForegroundColor Magenta

if (-not $useIso) {
    $requiredMarkers = @(
        "[BOOTCTRL] stage=boot-control-loaded",
        "[BOOTCTRL] stage=kernel-core-ready",
        "[BOOTCTRL] stage=storage-mounted",
        "[BOOTCTRL] stage=display-ready"
    )
    if (-not $NoAutoLogin) {
        $requiredMarkers += @(
            "[BOOTCTRL] stage=desktop-ready",
            "[BOOTCTRL] stage=app-basket-ready",
            "[BOOTCTRL] success"
        )
    }
    if ($SuspendResumeSmoke) {
        $requiredMarkers += "[SMOKE] suspend-resume ok"
    }

    $serialContent = if (Test-Path $serialLogPath) { Get-Content $serialLogPath -Raw } else { "" }
    $missing = @()
    foreach ($marker in $requiredMarkers) {
        if (-not $serialContent.Contains($marker)) {
            $missing += $marker
        }
    }
    if ($missing.Count -gt 0) {
        Write-Host "Missing boot markers:" -ForegroundColor Red
        $missing | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
        if ($transcriptStarted) { Stop-Transcript | Out-Null }
        exit 1
    }
}

if ($transcriptStarted) { Stop-Transcript | Out-Null }
