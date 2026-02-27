param(
    [ValidateSet("auto", "iso", "uefi")]
    [string]$Mode = "auto",
    [ValidateSet("fast", "debug")]
    [string]$Profile = "fast",
    [switch]$RebuildIso,
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
try { taskkill /IM qemu-system-x86_64.exe /F 2>$null | Out-Null } catch {}

Write-Host "echOS QEMU Boot" -ForegroundColor Cyan
Write-Host "================`n" -ForegroundColor Cyan

$projectRoot = (Get-Location).Path
$logDir = Join-Path $projectRoot "logs"
if (-Not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
$logPath = Join-Path $logDir ("qemu_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$serialLogPath = Join-Path $logDir ("serial_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$debugLogPath = Join-Path $logDir ("debugcon_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$traceLogPath = Join-Path $logDir ("qemu_trace_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$qemuStdoutPath = Join-Path $logDir ("qemu_stdout_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$qemuStderrPath = Join-Path $logDir ("qemu_stderr_" + (Get-Date -Format "yyyyMMdd_HHmmss") + ".log")
$timeoutSec = 0  # No timeout
$transcriptStarted = $false
try {
    Start-Transcript -Path $logPath | Out-Null
    $transcriptStarted = $true
} catch {}

Write-Host "Log: $logPath" -ForegroundColor DarkGray
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray
Write-Host "QEMU stdout: $qemuStdoutPath" -ForegroundColor DarkGray
Write-Host "QEMU stderr: $qemuStderrPath" -ForegroundColor DarkGray
Write-Host "Profile: $Profile" -ForegroundColor DarkGray

$traceEnabled = $Profile -eq "debug"
$fastProfile = $Profile -eq "fast"
$accelArgs = if ($fastProfile) { @("-accel", "tcg") } else { @("-accel", "whpx", "-accel", "tcg") }

$llvmBin = "C:\Program Files\LLVM\bin"
if (Test-Path $llvmBin) {
    $env:PATH = "$llvmBin;$env:PATH"
    if (-Not $env:CC -or $env:CC -eq "") { $env:CC = "clang" }
    if (-Not $env:CC_x86_64_unknown_none -or $env:CC_x86_64_unknown_none -eq "") { $env:CC_x86_64_unknown_none = "clang" }
}
$env:CARGO_INCREMENTAL = "0"

$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-Not (Test-Path $qemu)) {
    $qemuCmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($qemuCmd) {
        $qemu = $qemuCmd.Source
    } else {
        Write-Error "QEMU bulunamadı"
        if ($transcriptStarted) { Stop-Transcript | Out-Null }
        exit 1
    }
}

$isoPath = Join-Path $projectRoot "echOS_multiboot.iso"
$kernelPath = Join-Path $projectRoot "target\x86_64-unknown-none\debug\ech_os"
$isoKernelPath = Join-Path $projectRoot "multiboot_iso\boot\ech_os"

$useIso = $false
if ($Mode -eq "iso") { $useIso = $true }
elseif ($Mode -eq "uefi") { $useIso = $false }
else { $useIso = Test-Path $isoPath }

if ($useIso) {
    if (-Not $NoBuild) {
        Write-Host "Building echOS (multiboot)..." -ForegroundColor Yellow
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $buildOut = cargo build --quiet 2>&1
        $buildExitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP
        $buildOut | Where-Object { $_ -notmatch "^warning:" } | Write-Host
        if ($buildExitCode -ne 0) {
            Write-Error "Build failed"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
        if (-Not (Test-Path $kernelPath)) {
            Write-Error "Kernel bulunamadı: $kernelPath"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
        Copy-Item $kernelPath $isoKernelPath -Force
    }

    if ($RebuildIso -or -Not (Test-Path $isoPath)) {
        Write-Host "Building multiboot ISO via WSL..." -ForegroundColor Yellow
        wsl -d Ubuntu -- bash -lc "cd /mnt/c/Users/Bahadir/Desktop/dersler_ve_projeler/echOS && grub-mkrescue -o echOS_multiboot.iso multiboot_iso"
        if ($LASTEXITCODE -ne 0) {
            Write-Error "ISO build failed"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
    }

    if (-Not (Test-Path $isoPath)) {
        Write-Error "ISO bulunamadı: $isoPath"
        if ($transcriptStarted) { Stop-Transcript | Out-Null }
        exit 1
    }

    $qemuArgs = @(
        "-cdrom", $isoPath,
        "-machine", "q35",
        "-m", "512M",
        "-serial", "file:$serialLogPath",
        "-debugcon", "file:$debugLogPath",
        "-global", "isa-debugcon.iobase=0xe9",
        "-display", "sdl",
        "-monitor", "none",
        "-no-reboot",
        "-no-shutdown"
    )
    $qemuArgs += $accelArgs
    if ($traceEnabled) {
        $qemuArgs += @(
            "-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset",
            "-D", $traceLogPath
        )
    }
} else {
    $efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
    $gpuTestSrc = Join-Path $projectRoot "userspace\gpu_test.rs"
    $gpuTestLinker = Join-Path $projectRoot "userspace\user.ld"
    $gpuTestOut = Join-Path $projectRoot "esp\gpu_test"
    $ESP_PATH = Join-Path $projectRoot "esp"

    $qemuShare = "C:\Program Files\qemu\share"
    $ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
    $ovmfVarsTemplate = Join-Path $qemuShare "edk2-i386-vars.fd"
    $ovmfVars = Join-Path $projectRoot "OVMF_VARS.fd"

    if (-Not $NoBuild) {
        Write-Host "Building echOS (UEFI)..." -ForegroundColor Yellow
        $prevEAP = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $buildOut = cargo build --quiet --target x86_64-unknown-uefi 2>&1
        $buildExitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP
        $buildOut | Where-Object { $_ -notmatch "^warning:" } | Write-Host
        if ($buildExitCode -ne 0) {
            Write-Error "Build failed"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
    } else {
        Write-Host "Skipping UEFI build (-NoBuild)" -ForegroundColor DarkYellow
    }

    if (-Not (Test-Path $efiPath)) {
        Write-Error "EFI binary not found at $efiPath"
        if ($transcriptStarted) { Stop-Transcript | Out-Null }
        exit 1
    }

    Write-Host "Copying EFI to ESP folder..." -ForegroundColor Yellow
    Copy-Item $efiPath "esp\EFI\BOOT\BOOTX64.EFI" -Force

    if (-Not $NoBuild) {
        Write-Host "Building gpu_test..." -ForegroundColor Yellow
    }
    if ((-Not $NoBuild) -and (Test-Path $gpuTestSrc)) {
        $prevEAP2 = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        rustc $gpuTestSrc -o $gpuTestOut --target x86_64-unknown-none -C opt-level=s -C panic=abort -C relocation-model=static -C link-arg=-T$gpuTestLinker
        $gpuExitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP2
        if ($gpuExitCode -ne 0) {
            Write-Error "gpu_test build failed"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
    } elseif (-Not $NoBuild) {
        Write-Host "  gpu_test.rs kaynak bulunamadı, mevcut binary kullanılıyor" -ForegroundColor DarkYellow
    }

    $echshSrc = Join-Path $projectRoot "userspace\echsh.rs"
    $echshOut = Join-Path $projectRoot "esp\echsh"
    if (-Not $NoBuild) {
        Write-Host "Building echsh..." -ForegroundColor Yellow
    }
    if ((-Not $NoBuild) -and (Test-Path $echshSrc)) {
        $prevEAP2 = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        rustc $echshSrc -o $echshOut --target x86_64-unknown-none -C opt-level=s -C panic=abort -C relocation-model=static -C link-arg=-T$gpuTestLinker
        $echshExitCode = $LASTEXITCODE
        $ErrorActionPreference = $prevEAP2
        if ($echshExitCode -ne 0) {
            Write-Error "echsh build failed"
            if ($transcriptStarted) { Stop-Transcript | Out-Null }
            exit 1
        }
    } elseif (-Not $NoBuild) {
        Write-Host "  echsh.rs kaynak bulunamadı, mevcut binary kullanılıyor" -ForegroundColor DarkYellow
    }

    if (-Not (Test-Path $ovmfVars)) {
        Write-Host "Creating OVMF_VARS.fd from template..." -ForegroundColor Yellow
        Copy-Item $ovmfVarsTemplate $ovmfVars -Force
    }

    $ovmfCodeDrive = "if=pflash,format=raw,readonly=on,file=`"$ovmfCode`""
    $ovmfVarsDrive = "if=pflash,format=raw,file=`"$ovmfVars`""
    $qemuArgs = @(
        "-machine", "q35",
        "-cpu", "Haswell,+smep,+smap,+pcid",
        "-smp", $(if ($fastProfile) { "2" } else { "4" }),
        "-drive", $ovmfCodeDrive,
        "-drive", $ovmfVarsDrive,
        "-drive", "format=raw,file=fat:rw:esp",
        "-debugcon", "file:$debugLogPath",
        "-global", "isa-debugcon.iobase=0xE9",
        "-serial", "file:$serialLogPath",
        "-m", "2G",
        "-display", "sdl",
        "-monitor", "none",
        "-no-reboot",
        "-no-shutdown",
        "-netdev", "user,id=net0,hostfwd=tcp::8080-:80,hostfwd=tcp::4443-:443",
        "-device", "virtio-net-pci,netdev=net0,disable-modern=off,disable-legacy=on"
    )
    $qemuArgs += $accelArgs
    if ($traceEnabled) {
        $qemuArgs += @(
            "-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset",
            "-D", $traceLogPath
        )
    }
    $diskImg = Join-Path $ESP_PATH "disk.img"
    if (Test-Path $diskImg) {
        $qemuArgs += @(
            "-drive", "file=$diskImg,if=none,format=raw,id=drive0",
            "-device", "virtio-blk-pci,drive=drive0,disable-modern=on,disable-legacy=off"
        )
    } else {
        Write-Host "disk.img not found, skipping virtio-blk device" -ForegroundColor DarkGray
    }
}

Write-Host "Launching QEMU...`n" -ForegroundColor Yellow
$proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath
if ($timeoutSec -gt 0) {
    if (-not $proc.WaitForExit($timeoutSec * 1000)) {
        Write-Host "QEMU timeout after $timeoutSec seconds, terminating..." -ForegroundColor Red
        try { $proc.Kill() } catch {}
    }
} else {
    $proc.WaitForExit()
}

Write-Host "`nQEMU exited." -ForegroundColor Magenta
if ($transcriptStarted) { Stop-Transcript | Out-Null }
