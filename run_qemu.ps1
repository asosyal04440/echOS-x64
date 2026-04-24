param(
    [ValidateSet("auto", "iso", "uefi")]
    [string]$Mode = "auto",
    [ValidateSet("fast", "debug")]
    [string]$Profile = "fast",
    [ValidateSet("auto", "whpx", "tcg")]
    [string]$Accel = "auto",
    [int]$DisplayWidth = 1920,
    [int]$DisplayHeight = 1080,
    [int]$CpuCount = 0,
    [switch]$RebuildIso,
    [switch]$RebuildAppliance,
    [switch]$NoBuild,
    [switch]$CreateOnly,
    [ValidateSet("none", "system_a", "system_b", "recovery")]
    [string]$PendingSlot = "none",
    [int]$ResetAfterSeconds = 0,
    [switch]$NoAutoLogin,
    [switch]$Headless,
    [switch]$WaitForExit,
    [switch]$ForceVarsReset,
    [switch]$SuspendResumeSmoke,
    [switch]$PackagedPeSmoke,
    [switch]$MixedUpdateSmoke,
    [string]$CuratedBundleDir = "",
    [string]$EfiPath = "",
    [string]$OvmfCodePath = "",
    [string]$OvmfVarsTemplatePath = "",
    [string]$OvmfVarsPath = "",
    [string[]]$EspExtraFile = @()
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

function Wait-FileMarkerOrProcessExit {
    param(
        [string]$Path,
        [string]$Marker,
        [int]$TimeoutSec,
        [System.Diagnostics.Process]$Process
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path) {
            $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
            if ($content -and $content.Contains($Marker)) {
                return $true
            }
        }
        if ($Process -and $Process.HasExited) {
            return $false
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

function Get-OptionalFeatureInstallState {
    param(
        [string]$FeatureName
    )

    try {
        $feature = Get-CimInstance -ClassName Win32_OptionalFeature -Filter "Name='$FeatureName'" -ErrorAction Stop
        if ($null -eq $feature) {
            return $null
        }
        return [int]$feature.InstallState
    } catch {
        return $null
    }
}

function Test-HypervisorPlatformEnabled {
    $installState = Get-OptionalFeatureInstallState -FeatureName "HypervisorPlatform"
    return $installState -eq 1
}

function Test-StringArrayEqual {
    param(
        [object[]]$Left,
        [object[]]$Right
    )

    $leftValues = @($Left | ForEach-Object { [string]$_ })
    $rightValues = @($Right | ForEach-Object { [string]$_ })
    if ($leftValues.Count -ne $rightValues.Count) {
        return $false
    }
    for ($i = 0; $i -lt $leftValues.Count; $i++) {
        if ($leftValues[$i] -ne $rightValues[$i]) {
            return $false
        }
    }
    return $true
}

function Test-ApplianceImageFresh {
    param(
        [string]$ImagePath,
        [string]$ManifestPath,
        [string[]]$InputPaths,
        [string]$PendingSlot,
        [bool]$AutoLogin,
        [string[]]$BundlePaths
    )

    if (-not (Test-Path $ImagePath) -or -not (Test-Path $ManifestPath)) {
        return $false
    }

    $imageTime = (Get-Item -LiteralPath $ImagePath).LastWriteTimeUtc
    foreach ($inputPath in $InputPaths) {
        if ($inputPath -and (Test-Path $inputPath)) {
            if ((Get-Item -LiteralPath $inputPath).LastWriteTimeUtc -gt $imageTime) {
                return $false
            }
        }
    }

    try {
        $manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
    } catch {
        return $false
    }

    $seed = $manifest.boot_control_seed
    if ($null -eq $seed) {
        return $false
    }
    if ($seed.active_slot -ne "system_a") { return $false }
    if ($seed.pending_slot -ne $PendingSlot) { return $false }
    if ([bool]$seed.auto_login -ne $AutoLogin) { return $false }
    if ([bool]$seed.suspend_resume_smoke) { return $false }
    if ($null -ne $seed.update_smoke_request_url) { return $false }
    if ($null -ne $seed.pe_smoke_bundle) { return $false }
    if (-not (Test-StringArrayEqual -Left @($seed.bundles) -Right $BundlePaths)) { return $false }
    if (@($seed.esp_extra_files).Count -ne 0) { return $false }

    return $true
}

if (($SuspendResumeSmoke -and $PackagedPeSmoke) -or ($SuspendResumeSmoke -and $MixedUpdateSmoke) -or ($PackagedPeSmoke -and $MixedUpdateSmoke)) {
    throw "SuspendResumeSmoke, PackagedPeSmoke ve MixedUpdateSmoke ayni anda kosulmaz"
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
$httpServerStdoutPath = Join-Path $logDir ("http_server_stdout_" + $stamp + ".log")
$httpServerStderrPath = Join-Path $logDir ("http_server_stderr_" + $stamp + ".log")

$transcriptStarted = $false
$httpServerProc = $null
try {
    Start-Transcript -Path $logPath | Out-Null
    $transcriptStarted = $true
} catch {}

Write-Host "Log: $logPath" -ForegroundColor DarkGray
Write-Host "Serial log: $serialLogPath" -ForegroundColor DarkGray
Write-Host "Profile: $Profile" -ForegroundColor DarkGray

$traceEnabled = $Profile -eq "debug"
$fastProfile = $Profile -eq "fast"
$whpxAvailable = Test-HypervisorPlatformEnabled
if ($Accel -eq "whpx" -and -not $whpxAvailable) {
    throw "WHPX istendi ama Windows Hypervisor Platform etkin degil"
}
$whpxEnabled = if ($Accel -eq "whpx") {
    $true
} elseif ($Accel -eq "tcg") {
    $false
} else {
    $whpxAvailable
}
$accelMode = if ($whpxEnabled) { "whpx" } else { "tcg" }
$accelArgs = if ($whpxEnabled) {
    # QEMU 10.2 WHPX can lose interrupt/MSI injection with the default in-kernel irqchip.
    # Keeping irqchip emulation in QEMU makes the echOS UEFI path reach the guest reliably;
    # if it still misses the early UEFI marker, the launcher falls back to TCG below.
    @("-accel", "whpx,kernel-irqchip=off", "-accel", "tcg")
} else {
    @("-accel", "tcg")
}
# WHPX aktifken host CPU talimatları (AES-NI, RDRAND, SHA-NI) doğrudan kullanılır.
# WHPX başarısız olursa TCG fallback devreye girer.
$cpuModel = if ($whpxEnabled) { "host" } else { "Haswell,+smep,+smap,+pcid" }
$storageController = "nvme"
$nicModel = "e1000e"
$videoArgs = @(
    "-vga", "none",
    "-device", "VGA,xres=$DisplayWidth,yres=$DisplayHeight,edid=on"
)
$hostCpuCount = [Environment]::ProcessorCount
$cpuCountExplicit = $CpuCount -gt 0
$defaultTcgCpuCount = [Math]::Min([Math]::Max($hostCpuCount, 1), 4)
$defaultWhpxCpuCount = [Math]::Max($hostCpuCount, 1)
$qemuCpuCount = if ($CpuCount -gt 0) {
    $CpuCount
} elseif ($accelMode -eq "tcg") {
    $defaultTcgCpuCount
} else {
    $defaultWhpxCpuCount
}
$qemuSmpArg = "sockets=1,cores=$qemuCpuCount,threads=1,maxcpus=$qemuCpuCount"
Write-Host "vCPUs: $qemuCpuCount" -ForegroundColor DarkGray
if ((-not $cpuCountExplicit) -and $accelMode -eq "tcg" -and $hostCpuCount -gt $qemuCpuCount) {
    Write-Host "TCG vCPU cap: host has $hostCpuCount logical CPUs, using $qemuCpuCount for responsive emulation" -ForegroundColor DarkGray
}
Write-Host "Acceleration: $accelMode" -ForegroundColor DarkGray
Write-Host "CPU model: $cpuModel" -ForegroundColor DarkGray
Write-Host "Storage controller: $storageController" -ForegroundColor DarkGray
Write-Host "NIC model: $nicModel" -ForegroundColor DarkGray

$llvmBin = "C:\Program Files\LLVM\bin"
if (Test-Path $llvmBin) {
    $env:PATH = "$llvmBin;$env:PATH"
    if (-not $env:CC -or $env:CC -eq "") { $env:CC = "clang" }
    if (-not $env:CC_x86_64_unknown_none -or $env:CC_x86_64_unknown_none -eq "") { $env:CC_x86_64_unknown_none = "clang" }
}
# Incremental derleme aktif — sadece değişen dosyalar yeniden derlenir
# $env:CARGO_INCREMENTAL = "0"  # ESKİ: her seferinde sıfırdan derliyordu

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
        "-cpu", $cpuModel,
        "-smp", $qemuSmpArg,
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
    $efiPath = if ($EfiPath -ne "") {
        $resolvedEfiPath = Resolve-Path -LiteralPath $EfiPath -ErrorAction SilentlyContinue
        if (-not $resolvedEfiPath) { throw "EFI binary not found at $EfiPath" }
        $resolvedEfiPath.Path
    } else {
        Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"
    }
    $imagePath = Join-Path $artifactDir "echos_vm.raw"
    $manifestPath = Join-Path $artifactDir "echos_vm.json"
    $builderPath = Join-Path $projectRoot "scripts\build_vm_appliance.py"
    $peSmokeBundlePath = $null
    $mixedUpdateArtifactDir = $null
    $mixedUpdateIndexUrl = $null

    $qemuShare = "C:\Program Files\qemu\share"
    $ovmfCode = if ($OvmfCodePath -ne "") {
        $resolvedOvmfCode = Resolve-Path -LiteralPath $OvmfCodePath -ErrorAction SilentlyContinue
        if (-not $resolvedOvmfCode) { throw "OVMF code image not found at $OvmfCodePath" }
        $resolvedOvmfCode.Path
    } else {
        Join-Path $qemuShare "edk2-x86_64-code.fd"
    }
    $ovmfVarsTemplate = if ($OvmfVarsTemplatePath -ne "") {
        $resolvedVarsTemplate = Resolve-Path -LiteralPath $OvmfVarsTemplatePath -ErrorAction SilentlyContinue
        if (-not $resolvedVarsTemplate) { throw "OVMF vars template not found at $OvmfVarsTemplatePath" }
        $resolvedVarsTemplate.Path
    } else {
        Join-Path $qemuShare "edk2-i386-vars.fd"
    }
    $ovmfVars = if ($OvmfVarsPath -ne "") {
        [System.IO.Path]::GetFullPath($OvmfVarsPath)
    } else {
        Join-Path $artifactDir "OVMF_VARS.fd"
    }

    if (($EfiPath -eq "") -and (-not $NoBuild)) {
        Write-Host "Building echOS (UEFI)..." -ForegroundColor Yellow
        cargo build --quiet --target x86_64-unknown-uefi
        if ($LASTEXITCODE -ne 0) { throw "UEFI build failed" }
    }
    if (-not (Test-Path $efiPath)) { throw "EFI binary not found at $efiPath" }

    if ($PackagedPeSmoke) {
        if ($NoAutoLogin) {
            throw "Packaged PE smoke auto-login gerektirir"
        }
        $peSmokeManifest = Join-Path $projectRoot "tools\\pe_smoke_windowed\\Cargo.toml"
        $echsdkPath = Join-Path $projectRoot "target\\x86_64-pc-windows-msvc\\debug\\echsdk.exe"
        $peSmokeRoot = Join-Path $projectRoot "tools\\pe_smoke_windowed"
        $peSmokeBundlePath = Join-Path $artifactDir "pe_smoke_windowed.bhd"

        Write-Host "Building packaged PE smoke sample..." -ForegroundColor Yellow
        cargo build --quiet --release --target x86_64-pc-windows-msvc --manifest-path $peSmokeManifest
        if ($LASTEXITCODE -ne 0) { throw "PE smoke sample build failed" }

        Write-Host "Building echosdk host tool..." -ForegroundColor Yellow
        cargo build --quiet --bin echsdk --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw "echsdk host build failed" }
        if (-not (Test-Path $echsdkPath)) { throw "echsdk host tool not found at $echsdkPath" }

        & $echsdkPath sign $peSmokeRoot developer $peSmokeBundlePath
        if ($LASTEXITCODE -ne 0) { throw "PE smoke bundle signing failed" }
    }

    if ($MixedUpdateSmoke) {
        if ($NoAutoLogin) {
            throw "Mixed update smoke auto-login gerektirir"
        }
        $echsdkPath = Join-Path $projectRoot "target\\x86_64-pc-windows-msvc\\debug\\echsdk.exe"
        $slotImageBuilderPath = Join-Path $projectRoot "scripts\\build_f2fs_slot_image.py"
        $mixedUpdateArtifactDir = Join-Path $artifactDir "mixed_update_smoke"
        $platformImagePath = Join-Path $mixedUpdateArtifactDir "platform-system_b.img"
        $serviceArtifactPath = Join-Path $mixedUpdateArtifactDir "service-reboot.bhd"
        $updateSpecPath = Join-Path $mixedUpdateArtifactDir "update.spec"
        $signedIndexPath = Join-Path $mixedUpdateArtifactDir "index.bin"
        $requestPort = 18080
        $mixedUpdateIndexUrl = "http://10.0.2.2:${requestPort}/index.bin"
        $platformImageUrl = "http://10.0.2.2:${requestPort}/platform-system_b.img"
        $serviceArtifactUrl = "http://10.0.2.2:${requestPort}/service-reboot.bhd"
        $publishedEpoch = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $sourceServiceBundle = Join-Path $projectRoot "artifacts\\curated-bundles\\helix.bhd"

        if (-not (Test-Path $sourceServiceBundle)) {
            $fallbackBundle = Get-ChildItem (Join-Path $projectRoot "artifacts\\curated-bundles") -Filter *.bhd -File | Sort-Object Name | Select-Object -First 1
            if (-not $fallbackBundle) {
                throw "Mixed update smoke icin kullanilacak bundle bulunamadi"
            }
            $sourceServiceBundle = $fallbackBundle.FullName
        }

        if (Test-Path $mixedUpdateArtifactDir) {
            Remove-Item $mixedUpdateArtifactDir -Recurse -Force
        }
        New-Item -ItemType Directory -Force -Path $mixedUpdateArtifactDir | Out-Null

        Write-Host "Building echosdk host tool..." -ForegroundColor Yellow
        cargo build --quiet --bin echsdk --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw "echsdk host build failed" }
        if (-not (Test-Path $echsdkPath)) { throw "echsdk host tool not found at $echsdkPath" }

        Write-Host "Building mixed update platform image..." -ForegroundColor Yellow
        & $python.Source $slotImageBuilderPath --output $platformImagePath --image-mib 8
        if ($LASTEXITCODE -ne 0) { throw "mixed update slot image build failed" }

        Copy-Item $sourceServiceBundle $serviceArtifactPath -Force
        @(
            "channel=engineering"
            "release=qemu-mixed-update-r1"
            "published_epoch=$publishedEpoch"
            "artifact=platform|platform-system_b|qemu-mixed-update-r1|platform-system_b.img|$platformImageUrl|true|-|system_b"
            "artifact=service|reboot-service|qemu-mixed-update-r1|service-reboot.bhd|$serviceArtifactUrl|true|reboot-service|-"
        ) | Set-Content -Path $updateSpecPath -Encoding Ascii

        & $echsdkPath update publish $updateSpecPath $signedIndexPath engineering
        if ($LASTEXITCODE -ne 0) { throw "mixed update index publish failed" }

        Write-Host "Starting mixed update HTTP server..." -ForegroundColor Yellow
        $httpServerProc = Start-Process -FilePath $python.Source -ArgumentList @("-m", "http.server", "$requestPort", "--bind", "0.0.0.0", "--directory", $mixedUpdateArtifactDir) -PassThru -RedirectStandardOutput $httpServerStdoutPath -RedirectStandardError $httpServerStderrPath
        Start-Sleep -Seconds 2
        if ($httpServerProc.HasExited) {
            throw "mixed update HTTP server exited early"
        }
    }

    $bundleDir = if ($CuratedBundleDir -ne "") {
        $CuratedBundleDir
    } else {
        Join-Path $projectRoot "artifacts\\curated-bundles"
    }
    $bundleFiles = @()
    if (Test-Path $bundleDir) {
        $bundleFiles = @(Get-ChildItem $bundleDir -Filter *.bhd -File | Sort-Object Name)
        if ($bundleFiles.Count -gt 0) {
            Write-Host ("Curated bundles: {0}" -f $bundleFiles.Count) -ForegroundColor DarkGray
        }
    }

    $builderArgs = @(
        $builderPath,
        "--efi", $efiPath,
        "--output", $imagePath,
        "--active-slot", "system_a",
        "--pending-slot", $PendingSlot,
        "--system-image-mib", "8"
    )
    if (-not $NoAutoLogin) {
        $builderArgs += "--auto-login"
    }
    if ($SuspendResumeSmoke) {
        $builderArgs += "--suspend-resume-smoke"
    }
    if ($PackagedPeSmoke) {
        $builderArgs += @("--pe-smoke-bundle", $peSmokeBundlePath)
    }
    if ($MixedUpdateSmoke) {
        $builderArgs += @("--update-smoke-request-url", $mixedUpdateIndexUrl)
    }
    foreach ($bundle in $bundleFiles) {
        $builderArgs += @("--bundle", $bundle.FullName)
    }
    foreach ($espExtra in $EspExtraFile) {
        $builderArgs += @("--esp-extra-file", $espExtra)
    }
    $canReuseAppliance = (-not $RebuildAppliance) -and `
        (-not $SuspendResumeSmoke) -and `
        (-not $PackagedPeSmoke) -and `
        (-not $MixedUpdateSmoke) -and `
        ($EspExtraFile.Count -eq 0)
    $applianceInputs = @($efiPath, $builderPath, (Join-Path $projectRoot "scripts\\build_f2fs_slot_image.py"))
    $applianceInputs += @($bundleFiles | ForEach-Object { $_.FullName })
    $bundlePaths = @($bundleFiles | ForEach-Object { $_.FullName })
    $autoLogin = -not $NoAutoLogin
    $reuseAppliance = $canReuseAppliance -and (Test-ApplianceImageFresh `
        -ImagePath $imagePath `
        -ManifestPath $manifestPath `
        -InputPaths $applianceInputs `
        -PendingSlot $PendingSlot `
        -AutoLogin $autoLogin `
        -BundlePaths $bundlePaths)

    if ($reuseAppliance) {
        Write-Host "Reusing fresh raw GPT appliance disk. Use -RebuildAppliance to force rebuild." -ForegroundColor DarkGray
    } else {
        Write-Host "Building raw GPT appliance disk..." -ForegroundColor Yellow
        & $python.Source @builderArgs
        if ($LASTEXITCODE -ne 0) { throw "Appliance image build failed" }
    }

    Write-Host "Disk image: $imagePath" -ForegroundColor DarkGray
    Write-Host "Manifest: $manifestPath" -ForegroundColor DarkGray
    if ($CreateOnly) {
        exit 0
    }

    $ovmfVarsDir = Split-Path -Path $ovmfVars -Parent
    if ($ovmfVarsDir -and -not (Test-Path $ovmfVarsDir)) {
        New-Item -ItemType Directory -Force -Path $ovmfVarsDir | Out-Null
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
        "-cpu", $cpuModel,
        "-smp", $qemuSmpArg,
        "-drive", $ovmfCodeDrive,
        "-drive", $ovmfVarsDrive,
        # Donanima daha yakin blok lane'i icin NVMe denetleyicisi kullan.
        "-drive", "file=$imagePath,if=none,id=nvme0,format=raw",
        "-device", "nvme,serial=echosnvme0,drive=nvme0,bootindex=0",
        "-debugcon", "file:$debugLogPath",
        "-global", "isa-debugcon.iobase=0xE9",
        "-serial", "file:$serialLogPath",
        "-m", "2G",
        "-monitor", $monitorEndpoint,
        "-no-reboot",
        "-no-shutdown",
        # Ağ: virtio-net (birincil) + e1000e (donanım davranış testi)
        "-netdev", "user,id=net0,hostfwd=tcp::8080-:80,hostfwd=tcp::4443-:443",
        "-device", "e1000e,netdev=net0"
    ) + $displayArgs + $videoArgs + $accelArgs
    if ($traceEnabled) {
        $qemuArgs += @("-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset", "-D", $traceLogPath)
    }
}

function Assert-SerialMarkers {
    param(
        [string]$Path,
        [string[]]$Markers
    )

    $serialContent = if (Test-Path $Path) { Get-Content $Path -Raw } else { "" }
    $missing = @()
    foreach ($marker in $Markers) {
        if (-not $serialContent.Contains($marker)) {
            $missing += $marker
        }
    }
    if ($missing.Count -gt 0) {
        Write-Host "Missing boot markers in ${Path}:" -ForegroundColor Red
        $missing | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
        throw "Required serial markers missing"
    }
}

function Convert-ToTcgFallbackArgs {
    param(
        [string[]]$InputArgs,
        [string]$OldSerialPath,
        [string]$NewSerialPath,
        [string]$OldDebugPath,
        [string]$NewDebugPath,
        [string]$OldTracePath,
        [string]$NewTracePath,
        [int]$NewCpuCount
    )

    $fallback = @()
    for ($i = 0; $i -lt $InputArgs.Count; $i++) {
        $arg = $InputArgs[$i]
        if ($arg -eq "-accel") {
            $i++
            continue
        }
        if ($arg -eq "-cpu") {
            $fallback += "-cpu"
            $fallback += "Haswell,+smep,+smap,+pcid"
            $i++
            continue
        }
        if ($arg -eq "-smp" -and $NewCpuCount -gt 0) {
            $fallback += "-smp"
            $fallback += "sockets=1,cores=$NewCpuCount,threads=1,maxcpus=$NewCpuCount"
            $i++
            continue
        }
        if ($arg -eq "file:$OldSerialPath") {
            $fallback += "file:$NewSerialPath"
            continue
        }
        if ($arg -eq "file:$OldDebugPath") {
            $fallback += "file:$NewDebugPath"
            continue
        }
        if ($OldTracePath -ne "" -and $arg -eq $OldTracePath) {
            $fallback += $NewTracePath
            continue
        }
        $fallback += $arg
    }
    $fallback += @("-accel", "tcg")
    $fallback
}

function Start-MixedUpdatePhase {
    param(
        [string]$PhaseName,
        [int]$PhaseMonitorPort
    )

    $phaseSerialLogPath = Join-Path $logDir ("serial_" + $stamp + "_" + $PhaseName + ".log")
    $phaseDebugLogPath = Join-Path $logDir ("debugcon_" + $stamp + "_" + $PhaseName + ".log")
    $phaseTraceLogPath = Join-Path $logDir ("qemu_trace_" + $stamp + "_" + $PhaseName + ".log")
    $phaseStdoutPath = Join-Path $logDir ("qemu_stdout_" + $stamp + "_" + $PhaseName + ".log")
    $phaseStderrPath = Join-Path $logDir ("qemu_stderr_" + $stamp + "_" + $PhaseName + ".log")
    $phaseMonitorEndpoint = "tcp:${monitorHost}:${PhaseMonitorPort},server,nowait"
    $phaseArgs = @()
    foreach ($arg in $qemuArgs) {
        if ($arg -eq "file:$serialLogPath") {
            $phaseArgs += "file:$phaseSerialLogPath"
        } elseif ($arg -eq "file:$debugLogPath") {
            $phaseArgs += "file:$phaseDebugLogPath"
        } elseif ($traceEnabled -and $arg -eq $traceLogPath) {
            $phaseArgs += $phaseTraceLogPath
        } elseif ($arg -eq $monitorEndpoint) {
            $phaseArgs += $phaseMonitorEndpoint
        } else {
            $phaseArgs += $arg
        }
    }

    Write-Host "Launching QEMU ($PhaseName)...`n" -ForegroundColor Yellow
    $proc = Start-Process -FilePath $qemu -ArgumentList $phaseArgs -PassThru -RedirectStandardOutput $phaseStdoutPath -RedirectStandardError $phaseStderrPath
    [PSCustomObject]@{
        Name = $PhaseName
        Process = $proc
        SerialLogPath = $phaseSerialLogPath
        DebugLogPath = $phaseDebugLogPath
        TraceLogPath = $phaseTraceLogPath
        StdoutPath = $phaseStdoutPath
        StderrPath = $phaseStderrPath
        MonitorPort = $PhaseMonitorPort
    }
}

try {
    if ($MixedUpdateSmoke) {
        if ($useIso) {
            throw "Mixed update smoke yalnizca UEFI appliance yolunda kosulur"
        }

        $phase1 = $null
        $phase2 = $null
        $phase1 = Start-MixedUpdatePhase -PhaseName "mixed_stage" -PhaseMonitorPort $monitorPort
        try {
            if (-not (Wait-FileMarker -Path $phase1.SerialLogPath -Marker "[DESKTOP] session bootstrap step=login-visible" -TimeoutSec 120)) {
                throw "Mixed update phase-1 login marker gorulmedi"
            }
            if (-not (Wait-FileMarker -Path $phase1.SerialLogPath -Marker "[SMOKE] mixed-update stage ok" -TimeoutSec 180)) {
                throw "Mixed update phase-1 stage marker gorulmedi"
            }
            if (-not (Wait-FileMarker -Path $phase1.SerialLogPath -Marker "[SMOKE] mixed-update reboot arm" -TimeoutSec 120)) {
                throw "Mixed update phase-1 reboot marker gorulmedi"
            }
            $phase1.Process.WaitForExit()
            Write-Host "`nQEMU exited ($($phase1.Name))." -ForegroundColor Magenta
            Assert-SerialMarkers -Path $phase1.SerialLogPath -Markers @(
                "[BOOTCTRL] stage=boot-control-loaded",
                "[BOOTCTRL] stage=kernel-core-ready",
                "[BOOTCTRL] stage=storage-mounted",
                "[BOOTCTRL] stage=display-ready",
                "[BOOTCTRL] stage=desktop-ready",
                "[BOOTCTRL] stage=app-basket-ready",
                "[SMOKE] mixed-update inspect ok",
                "[BOOTCTRL] update target_slot=system_b",
                "[SMOKE] mixed-update stage ok",
                "[SMOKE] mixed-update reboot arm"
            )
        } finally {
            if ($phase1 -and $phase1.Process -and -not $phase1.Process.HasExited) {
                try { $phase1.Process.Kill() } catch {}
                $phase1.Process.WaitForExit()
            }
        }

        $phase2 = Start-MixedUpdatePhase -PhaseName "mixed_commit" -PhaseMonitorPort ($monitorPort + 1)
        try {
            if (-not (Wait-FileMarker -Path $phase2.SerialLogPath -Marker "[DESKTOP] session bootstrap step=login-visible" -TimeoutSec 120)) {
                throw "Mixed update phase-2 login marker gorulmedi"
            }
            if (-not (Wait-FileMarker -Path $phase2.SerialLogPath -Marker "[UPDATE] staged boot apply ok" -TimeoutSec 180)) {
                throw "Mixed update phase-2 staged apply marker gorulmedi"
            }
            if (-not (Wait-FileMarker -Path $phase2.SerialLogPath -Marker "[BOOTCTRL] success active_slot=system_b" -TimeoutSec 180)) {
                throw "Mixed update phase-2 success marker gorulmedi"
            }
            if (-not $phase2.Process.HasExited) {
                try { $phase2.Process.Kill() } catch {}
            }
            $phase2.Process.WaitForExit()
            Write-Host "`nQEMU exited ($($phase2.Name))." -ForegroundColor Magenta
            Assert-SerialMarkers -Path $phase2.SerialLogPath -Markers @(
                "[BOOTCTRL] stage=boot-control-loaded",
                "[BOOTCTRL] stage=kernel-core-ready",
                "[BOOTCTRL] stage=storage-mounted",
                "[BOOTCTRL] stage=display-ready",
                "[BOOTCTRL] stage=desktop-ready",
                "[BOOTCTRL] stage=app-basket-ready",
                "[UPDATE] staged boot apply ok",
                "[BOOTCTRL] success active_slot=system_b"
            )
        } finally {
            if ($phase2 -and $phase2.Process -and -not $phase2.Process.HasExited) {
                try { $phase2.Process.Kill() } catch {}
                $phase2.Process.WaitForExit()
            }
        }
    } else {
        Write-Host "Launching QEMU...`n" -ForegroundColor Yellow
        $proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath

        if ($whpxEnabled -and -not $useIso) {
            $whpxEntryTimeoutSec = if ($Accel -eq "whpx") { 20 } else { 5 }
            if (-not (Wait-FileMarkerOrProcessExit -Path $serialLogPath -Marker "[UEFI] EFI Entry Point Reached!" -TimeoutSec $whpxEntryTimeoutSec -Process $proc)) {
                if ($Accel -eq "whpx") {
                    if (-not $proc.HasExited) {
                        try { $proc.Kill() } catch {}
                    }
                    try { $proc.WaitForExit() } catch {}
                    $whpxError = if (Test-Path $qemuStderrPath) { Get-Content $qemuStderrPath -Raw } else { "" }
                    if ($whpxError -ne "") {
                        Write-Host $whpxError -ForegroundColor Red
                    }
                    throw "WHPX requested explicitly but echOS did not reach UEFI entry"
                }
                $fallbackSerialLogPath = Join-Path $logDir ("serial_" + $stamp + "_tcg_fallback.log")
                $fallbackDebugLogPath = Join-Path $logDir ("debugcon_" + $stamp + "_tcg_fallback.log")
                $fallbackTraceLogPath = Join-Path $logDir ("qemu_trace_" + $stamp + "_tcg_fallback.log")
                $fallbackStdoutPath = Join-Path $logDir ("qemu_stdout_" + $stamp + "_tcg_fallback.log")
                $fallbackStderrPath = Join-Path $logDir ("qemu_stderr_" + $stamp + "_tcg_fallback.log")

                if (-not $proc.HasExited) {
                    try { $proc.Kill() } catch {}
                }
                try { $proc.WaitForExit() } catch {}

                Write-Host "WHPX did not reach UEFI entry; retrying with TCG fallback." -ForegroundColor DarkYellow
                Write-Host "Fallback serial log: $fallbackSerialLogPath" -ForegroundColor DarkGray
                $fallbackCpuCount = if ($cpuCountExplicit) { $qemuCpuCount } else { [Math]::Min($qemuCpuCount, $defaultTcgCpuCount) }
                if ($fallbackCpuCount -ne $qemuCpuCount) {
                    Write-Host "TCG fallback vCPU cap: $qemuCpuCount -> $fallbackCpuCount" -ForegroundColor DarkGray
                }

                $qemuArgs = Convert-ToTcgFallbackArgs `
                    -InputArgs $qemuArgs `
                    -OldSerialPath $serialLogPath `
                    -NewSerialPath $fallbackSerialLogPath `
                    -OldDebugPath $debugLogPath `
                    -NewDebugPath $fallbackDebugLogPath `
                    -OldTracePath $traceLogPath `
                    -NewTracePath $fallbackTraceLogPath `
                    -NewCpuCount $fallbackCpuCount
                $serialLogPath = $fallbackSerialLogPath
                $debugLogPath = $fallbackDebugLogPath
                $traceLogPath = $fallbackTraceLogPath
                $qemuStdoutPath = $fallbackStdoutPath
                $qemuStderrPath = $fallbackStderrPath
                $accelMode = "tcg-fallback"
                $cpuModel = "Haswell,+smep,+smap,+pcid"
                $qemuCpuCount = $fallbackCpuCount
                $qemuSmpArg = "sockets=1,cores=$qemuCpuCount,threads=1,maxcpus=$qemuCpuCount"
                $proc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru -RedirectStandardOutput $qemuStdoutPath -RedirectStandardError $qemuStderrPath
            }
        }

$interactiveReady = $false
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
    } elseif ($PackagedPeSmoke) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SMOKE] packaged-pe install ok" -TimeoutSec 120)) {
            try { $proc.Kill() } catch {}
            throw "Packaged PE smoke install marker not observed"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SMOKE] packaged-pe launch ok" -TimeoutSec 120)) {
            try { $proc.Kill() } catch {}
            throw "Packaged PE smoke launch marker not observed"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[WIN32] CreateWindowExA:" -TimeoutSec 120)) {
            try { $proc.Kill() } catch {}
            throw "Packaged PE smoke window marker not observed"
        }
        if ($Headless -and -not $proc.HasExited) {
            try { $proc.Kill() } catch {}
        }
    } elseif (-not $NoAutoLogin) {
        if (Wait-FileMarker -Path $serialLogPath -Marker "[BOOTCTRL] success" -TimeoutSec 90) {
            $interactiveReady = $true
            if ($Headless -and -not $proc.HasExited) {
                try { $proc.Kill() } catch {}
            }
        }
    } else {
        if (Wait-FileMarker -Path $serialLogPath -Marker "[BOOTCTRL] stage=display-ready" -TimeoutSec 90) {
            $interactiveReady = $true
            if ($Headless -and -not $proc.HasExited) {
                try { $proc.Kill() } catch {}
            }
        }
    }
}

if ($interactiveReady -and (-not $Headless) -and (-not $WaitForExit)) {
    Assert-SerialMarkers -Path $serialLogPath -Markers $successMarkers
    Write-Host "`nQEMU ready; leaving VM running and returning control to the shell." -ForegroundColor Green
    Write-Host "Use -WaitForExit if you want the script to block until the QEMU window closes." -ForegroundColor DarkGray
    return
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
    if ($PackagedPeSmoke) {
        $requiredMarkers += @(
            "[SMOKE] packaged-pe install ok",
            "[SMOKE] packaged-pe launch ok",
            "[WIN32] CreateWindowExA:"
        )
    }

    Assert-SerialMarkers -Path $serialLogPath -Markers $requiredMarkers
}

    }
} finally {
    if ($httpServerProc -and -not $httpServerProc.HasExited) {
        try { $httpServerProc.Kill() } catch {}
        try { $httpServerProc.WaitForExit() } catch {}
    }
    if ($transcriptStarted) { Stop-Transcript | Out-Null }
}
