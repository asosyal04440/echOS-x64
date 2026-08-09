param(
    [ValidateSet("auto", "iso", "uefi")]
    [string]$Mode = "uefi",
    [ValidateSet("fast", "debug")]
    [string]$Profile = "fast",
    [ValidateSet("auto", "whpx", "tcg")]
    [string]$Accel = "auto",
    [int]$DisplayWidth = 1920,
    [int]$DisplayHeight = 1080,
    [int]$CpuCount = 0,
    [ValidateRange(128, 32768)]
    [int]$MemoryMiB = 2048,
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
    [switch]$EnableTpm,
    [switch]$TpmGui,
    [switch]$TrustedBootSmoke,
    [string]$TpmStatePath = "",
    [string]$SwtpmPath = "",
    [int]$TpmServerPort = 2321,
    [int]$TpmControlPort = 2322,
    [switch]$ForceVarsReset,
    [switch]$Gdb,
    [switch]$GdbWait,
    [int]$GdbPort = 1234,
    [switch]$SuspendResumeSmoke,
    [switch]$FsSmokeTest,
    [switch]$ShellSmokeTest,
    [switch]$ShellCommandTest,
    [switch]$BootTests,
    [switch]$PackagedPeSmoke,
    [switch]$MixedUpdateSmoke,
    [switch]$SkipBootstrap,
    [string]$CuratedBundleDir = "",
    [string]$EfiPath = "",
    [string]$OvmfCodePath = "",
    [string]$OvmfVarsTemplatePath = "",
    [string]$OvmfVarsPath = "",
    [int]$HostHttpPort = 8080,
    [int]$HostHttpsPort = 4443,
    [string[]]$EspExtraFile = @()
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "scripts\bootstrap_windows.ps1")

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

function Get-FileContentLength {
    param(
        [string]$Path
    )

    if (-not (Test-Path $Path)) {
        return 0
    }
    $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
    if (-not $content) {
        return 0
    }
    return $content.Length
}

function ConvertTo-ProcessArgumentString {
    param(
        [string[]]$ArgumentList
    )

    $quoted = @()
    foreach ($arg in $ArgumentList) {
        if ($null -eq $arg) {
            continue
        }
        if ($arg -match '[\s"]') {
            $escaped = $arg.Replace('"', '\"')
            $quoted += '"' + $escaped + '"'
        } else {
            $quoted += $arg
        }
    }
    return ($quoted -join ' ')
}

function Start-CleanProcess {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList,
        [string]$StdoutPath,
        [string]$StderrPath
    )

    if ($StdoutPath -ne "") { Set-Content -LiteralPath $StdoutPath -Value "" -Encoding ASCII }
    if ($StderrPath -ne "") { Set-Content -LiteralPath $StderrPath -Value "" -Encoding ASCII }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $argString = ConvertTo-ProcessArgumentString -ArgumentList $ArgumentList
    $psi.FileName = $FilePath
    $psi.Arguments = $argString
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    if (-not $proc.Start()) {
        throw "process start failed: $FilePath"
    }
    Start-Sleep -Milliseconds 2000
    if ($proc.HasExited) {
        if ($StdoutPath -ne "") {
            Set-Content -LiteralPath $StdoutPath -Value $proc.StandardOutput.ReadToEnd() -Encoding ASCII
        }
        if ($StderrPath -ne "") {
            Set-Content -LiteralPath $StderrPath -Value $proc.StandardError.ReadToEnd() -Encoding ASCII
        }
    }
    return $proc
}

function ConvertTo-WslPath {
    param([string]$PathValue)

    $converted = & wsl.exe -e wslpath -a -u -- $PathValue 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "WSL path conversion failed for TPM state: $PathValue"
    }
    $converted = ($converted | Select-Object -Last 1).ToString().Trim()
    if (-not $converted) {
        throw "WSL returned an empty TPM state path for: $PathValue"
    }
    $converted
}

function Convert-QemuArgumentsForWsl {
    param([string[]]$Arguments)

    $converted = @()
    foreach ($argument in $Arguments) {
        if ($argument -match '^file:(?<path>[A-Za-z]:\\.*)$') {
            $converted += "file:$(ConvertTo-WslPath $Matches.path)"
            continue
        }
        if ($argument -match '^(?<prefix>.*file=)(?<path>[A-Za-z]:\\[^,]+)(?<suffix>,.*)?$') {
            $suffix = if ($Matches.suffix) { $Matches.suffix } else { "" }
            $converted += "$($Matches.prefix)$(ConvertTo-WslPath $Matches.path)$suffix"
            continue
        }
        if ($argument -match '^[A-Za-z]:\\' -and (Test-Path -LiteralPath $argument)) {
            $converted += ConvertTo-WslPath $argument
            continue
        }
        $converted += $argument
    }
    $converted
}

function Get-QemuLaunchArguments {
    param([string[]]$Arguments)

    if ($EnableTpm) {
        return @($qemuLaunchPrefix) + (Convert-QemuArgumentsForWsl $Arguments)
    }
    return $Arguments
}

function Test-TcpPortReady {
    param(
        [string]$HostName,
        [int]$Port
    )

    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        $connect = $client.ConnectAsync($HostName, $Port)
        if (-not $connect.Wait(250)) {
            return $false
        }
        return $client.Connected
    } catch {
        return $false
    } finally {
        $client.Dispose()
    }
}

function Resolve-TpmEmulator {
    param([string]$RequestedPath)

    if ($RequestedPath) {
        if ($RequestedPath.StartsWith("/")) {
            $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
            if (-not $wsl) { throw "WSL swtpm path requested but wsl.exe is unavailable: $RequestedPath" }
            return [pscustomobject]@{
                FilePath = $wsl.Source
                Prefix = @("-e", $RequestedPath)
                Wsl = $true
            }
        }
        $resolved = Resolve-Path -LiteralPath $RequestedPath -ErrorAction SilentlyContinue
        if (-not $resolved) {
            throw "swtpm not found at $RequestedPath"
        }
        return [pscustomobject]@{
            FilePath = $resolved.Path
            Prefix = @()
            Wsl = $false
        }
    }

    $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
    if ($wsl) {
        $wslSwtpm = & $wsl.Source -e sh -lc "command -v swtpm" 2>$null
        if ($LASTEXITCODE -eq 0 -and $wslSwtpm) {
            return [pscustomobject]@{
                FilePath = $wsl.Source
                Prefix = @("-e", "swtpm")
                Wsl = $true
            }
        }
    }

    $native = Get-Command swtpm -ErrorAction SilentlyContinue
    if ($native) {
        return [pscustomobject]@{
            FilePath = $native.Source
            Prefix = @()
            Wsl = $false
        }
    }

    throw "TPM emulator unavailable: install swtpm (native Windows or WSL) or pass -SwtpmPath."
}

function Start-TpmEmulator {
    param(
        [string]$StatePath,
        [string]$RequestedPath,
        [int]$ServerPort,
        [int]$ControlPort,
        [string]$StdoutPath,
        [string]$StderrPath
    )

    $emulator = Resolve-TpmEmulator $RequestedPath
    if (-not $emulator.Wsl) {
        throw "TPM emulation requires the WSL QEMU backend; the installed Windows QEMU has no -tpmdev backend. Install swtpm in WSL or pass a WSL swtpm path."
    }
    $stateDir = [System.IO.Path]::GetFullPath($StatePath)
    New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
    $stateArg = if ($emulator.Wsl) { ConvertTo-WslPath $stateDir } else { $stateDir }
    $socketPath = "/tmp/echos-tpm-$stamp/swtpm.sock"
    & wsl.exe -e sh -lc "rm -rf /tmp/echos-tpm-$stamp && mkdir -p /tmp/echos-tpm-$stamp" 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "WSL TPM socket directory creation failed: $socketPath"
    }
    $arguments = @($emulator.Prefix) + @(
        "socket",
        "--tpmstate", "dir=$stateArg",
        "--tpm2",
        "--ctrl", "type=unixio,path=$socketPath",
        "--flags", "not-need-init",
        "--log", "level=20"
    )

    $process = Start-CleanProcess `
        -FilePath $emulator.FilePath `
        -ArgumentList $arguments `
        -StdoutPath $StdoutPath `
        -StderrPath $StderrPath

    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        if ($process.HasExited) {
            $stderr = if (Test-Path $StderrPath) { Get-Content $StderrPath -Raw } else { "" }
            throw "swtpm exited before opening Unix socket $socketPath`: $stderr"
        }
        & wsl.exe -e test -S $socketPath 2>$null
        if ($LASTEXITCODE -eq 0) {
            return [pscustomobject]@{
                Process = $process
                SocketPath = $socketPath
            }
        }
        Start-Sleep -Milliseconds 250
    }

    try { $process.Kill() } catch {}
    try { $process.WaitForExit() } catch {}
    throw "swtpm Unix socket did not become ready: $socketPath"
}

function Wait-FileMarkerAfterOffset {
    param(
        [string]$Path,
        [string]$Marker,
        [int]$Offset,
        [int]$TimeoutSec
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path) {
            $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
            if ($content -and $content.Length -gt $Offset) {
                $tail = $content.Substring($Offset)
                if ($tail.Contains($Marker)) {
                    return $true
                }
            }
        }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Wait-FileAnyMarkerAfterOffset {
    param(
        [string]$Path,
        [string[]]$Markers,
        [int]$Offset,
        [int]$TimeoutSec
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $Path) {
            $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
            if ($content -and $content.Length -gt $Offset) {
                $tail = $content.Substring($Offset)
                foreach ($marker in $Markers) {
                    if ($tail.Contains($marker)) {
                        return $marker
                    }
                }
            }
        }
        Start-Sleep -Milliseconds 250
    }
    return ""
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

function Wait-ProcessExitOrTimeout {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$TimeoutSec
    )

    if ($null -eq $Process) {
        return $true
    }
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) {
            return $true
        }
        Start-Sleep -Milliseconds 250
    }
    return $Process.HasExited
}

function New-StageTimer {
    [System.Diagnostics.Stopwatch]::StartNew()
}

function Write-StageElapsed {
    param(
        [string]$Name,
        [System.Diagnostics.Stopwatch]$Timer
    )

    if ($Timer) {
        $Timer.Stop()
        Write-Host ("[TIME] {0}: {1:n3}s" -f $Name, $Timer.Elapsed.TotalSeconds) -ForegroundColor DarkCyan
    }
}

function Invoke-HostCargoBuild {
    param(
        [string[]]$CargoArgs
    )

    $savedCc = $env:CC
    $savedHostCc = $env:HOST_CC
    try {
        Remove-Item Env:CC -ErrorAction SilentlyContinue
        Remove-Item Env:HOST_CC -ErrorAction SilentlyContinue
        $savedEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        cargo @CargoArgs
        $ErrorActionPreference = $savedEap
        return $LASTEXITCODE
    } finally {
        if ($null -ne $savedCc) { $env:CC = $savedCc } else { Remove-Item Env:CC -ErrorAction SilentlyContinue }
        if ($null -ne $savedHostCc) { $env:HOST_CC = $savedHostCc } else { Remove-Item Env:HOST_CC -ErrorAction SilentlyContinue }
    }
}

function Test-OutputFresh {
    param(
        [string]$OutputPath,
        [string[]]$InputPaths
    )

    if (-not (Test-Path $OutputPath)) {
        return $false
    }
    $outputTime = (Get-Item -LiteralPath $OutputPath).LastWriteTimeUtc
    foreach ($inputPath in $InputPaths) {
        if ($inputPath -and (Test-Path $inputPath)) {
            if ((Get-Item -LiteralPath $inputPath).LastWriteTimeUtc -gt $outputTime) {
                return $false
            }
        }
    }
    return $true
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
    if ([bool]$seed.fs_smoke_test) { return $false }
    if ([bool]$seed.shell_smoke_test) { return $false }
    if ([bool]$seed.shell_command_test) { return $false }
    if ($null -ne $seed.update_smoke_request_url) { return $false }
    if ($null -ne $seed.pe_smoke_bundle) { return $false }
    if ($manifest.esp_fat -ne "fat32") { return $false }
    if (-not (Test-StringArrayEqual -Left @($seed.bundles) -Right $BundlePaths)) { return $false }
    if (@($seed.esp_extra_files).Count -ne 0) { return $false }

    return $true
}

$exclusiveSmokeCount = 0
$effectiveShellSmokeTest = $ShellSmokeTest -or $ShellCommandTest
foreach ($smokeSwitch in @($SuspendResumeSmoke, $FsSmokeTest, $effectiveShellSmokeTest, $PackagedPeSmoke, $MixedUpdateSmoke)) {
    if ($smokeSwitch) {
        $exclusiveSmokeCount++
    }
}
if ($exclusiveSmokeCount -gt 1) {
    throw "SuspendResumeSmoke, FsSmokeTest, ShellSmokeTest/ShellCommandTest, PackagedPeSmoke ve MixedUpdateSmoke ayni anda kosulmaz"
}
if ($EnableTpm -and $MixedUpdateSmoke) {
    throw "-EnableTpm ile MixedUpdateSmoke birlikte kullanılamaz; TPM backend tek UEFI QEMU oturumu gerektirir."
}
if ($TpmGui -and -not $EnableTpm) {
    throw "-TpmGui yalnızca -EnableTpm ile kullanılabilir."
}

try { taskkill /IM qemu-system-x86_64.exe /F 2>$null | Out-Null } catch {}

Write-Host "echOS QEMU Appliance" -ForegroundColor Cyan
Write-Host "====================`n" -ForegroundColor Cyan

$projectRoot = [System.IO.Path]::GetFullPath($PSScriptRoot)
$originalLocation = (Get-Location).Path
Set-Location -LiteralPath $projectRoot
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
$tpmStdoutPath = Join-Path $logDir ("tpm_stdout_" + $stamp + ".log")
$tpmStderrPath = Join-Path $logDir ("tpm_stderr_" + $stamp + ".log")

$transcriptStarted = $false
$httpServerProc = $null
$tpmProcess = $null
$tpmSocketPath = $null
$leaveQemuRunning = $false
$qemuLaunchPrefix = @()
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
$tcgCpuModel = "Haswell,+smep,+smap"
$cpuModel = if ($whpxEnabled) { "host" } else { $tcgCpuModel }
$storageController = "nvme"
$nicModel = "virtio-net-pci,disable-legacy=on,disable-modern=off"
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
    $env:Path = "$llvmBin;$env:Path"
    if (-not $env:CC -or $env:CC -eq "") { $env:CC = "clang" }
    if (-not $env:CC_x86_64_unknown_none -or $env:CC_x86_64_unknown_none -eq "") { $env:CC_x86_64_unknown_none = "clang" }
}
# Incremental derleme aktif — sadece değişen dosyalar yeniden derlenir
# $env:CARGO_INCREMENTAL = "0"  # ESKİ: her seferinde sıfırdan derliyordu

$bootstrap = Initialize-EchosWindowsEnvironment `
    -ProjectRoot $projectRoot `
    -NeedBareMetal:($Mode -eq "iso") `
    -NeedPython:$MixedUpdateSmoke `
    -NeedMsvc:($Mode -ne "iso") `
    -SkipInstall:$SkipBootstrap
$qemu = $bootstrap.QemuPath
$qemuLaunchFile = $qemu

if ($EnableTpm) {
    $wslCommand = Get-Command wsl.exe -ErrorAction SilentlyContinue
    if (-not $wslCommand) {
        throw "-EnableTpm requires wsl.exe because the installed Windows QEMU has no TPM backend."
    }
    $wslQemu = & $wslCommand.Source -e sh -lc "command -v qemu-system-x86_64" 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $wslQemu) {
        throw "-EnableTpm requires qemu-system-x86_64 in WSL (with -tpmdev emulator support)."
    }
    $wslTpmBackends = & $wslCommand.Source -e qemu-system-x86_64 -tpmdev help 2>&1
    if (-not (($wslTpmBackends -join "`n") -match "emulator")) {
        throw "WSL QEMU lacks the TPM emulator backend (-tpmdev emulator)."
    }
    $wslTpmDevices = & $wslCommand.Source -e qemu-system-x86_64 -device help 2>&1
    if ($LASTEXITCODE -ne 0 -or -not (($wslTpmDevices -join "`n") -match "tpm-tis")) {
        throw "WSL QEMU lacks the tpm-tis device required for TPM 2.0 firmware discovery."
    }
    if ($TpmGui) {
        $wslGui = & $wslCommand.Source -e sh -lc 'test -n "$DISPLAY" && test -S /mnt/wslg/runtime-dir/wayland-0' 2>$null
        if ($LASTEXITCODE -ne 0) {
            throw "-TpmGui requires WSLg (DISPLAY and /mnt/wslg/runtime-dir/wayland-0). Use -Headless or install/enable WSLg."
        }
        Write-Host "TPM GUI: WSLg GTK display enabled" -ForegroundColor DarkGray
    }
    $qemuLaunchFile = $wslCommand.Source
    $qemuLaunchPrefix = @("-e", "qemu-system-x86_64")
    $whpxEnabled = $false
    $accelMode = "tcg"
    $accelArgs = @("-accel", "tcg")
    $cpuModel = $tcgCpuModel
    if (-not $cpuCountExplicit) {
        $qemuCpuCount = $defaultTcgCpuCount
    }
    $qemuSmpArg = "sockets=1,cores=$qemuCpuCount,threads=1,maxcpus=$qemuCpuCount"
    Write-Host "TPM backend: WSL QEMU + Unix socket (native Windows QEMU TPM backend unavailable)" -ForegroundColor DarkGray
}

$python = $null
if ($MixedUpdateSmoke) {
    $python = Get-Command python -ErrorAction SilentlyContinue
    if (-not $python) {
        throw "python bulunamadı; mixed-update HTTP server için gerekli"
    }
}

$isoPath = Join-Path $projectRoot "echOS_multiboot.iso"
$kernelPath = Join-Path $projectRoot "target\x86_64-unknown-none\debug\ech_os"
$isoKernelPath = Join-Path $projectRoot "multiboot_iso\boot\ech_os"
$useIso = $false
if ($Mode -eq "iso") { $useIso = $true }
elseif ($Mode -eq "uefi") { $useIso = $false }
else { $useIso = $false }

if ($EnableTpm -and $useIso) {
    throw "-EnableTpm yalnızca UEFI appliance yolunda kullanılabilir; Multiboot2 ISO'da TCG2 firmware yolu yok. -Mode uefi kullanın."
}

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
    if ($Gdb) {
        $qemuArgs += @("-gdb", "tcp::${GdbPort},server,nowait")
        Write-Host "GDB endpoint: tcp::${GdbPort}" -ForegroundColor Green
    }
    if ($GdbWait) {
        $qemuArgs += "-S"
        Write-Host "QEMU paused (waiting for GDB). Connect with: gdb -x .gdbinit" -ForegroundColor Green
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
    $builderPath = Join-Path $projectRoot "target\x86_64-pc-windows-msvc\debug\echos_appliance.exe"
    $peSmokeBundlePath = $null
    $mixedUpdateArtifactDir = $null
    $mixedUpdateIndexUrl = $null

$qemuShare = $bootstrap.QemuShare
    $ovmfCode = if ($OvmfCodePath -ne "") {
        $resolvedOvmfCode = Resolve-Path -LiteralPath $OvmfCodePath -ErrorAction SilentlyContinue
        if (-not $resolvedOvmfCode) { throw "OVMF code image not found at $OvmfCodePath" }
        $resolvedOvmfCode.Path
    } else {
    $bootstrap.OvmfCodePath
    }
    $ovmfVarsTemplate = if ($OvmfVarsTemplatePath -ne "") {
        $resolvedVarsTemplate = Resolve-Path -LiteralPath $OvmfVarsTemplatePath -ErrorAction SilentlyContinue
        if (-not $resolvedVarsTemplate) { throw "OVMF vars template not found at $OvmfVarsTemplatePath" }
        $resolvedVarsTemplate.Path
    } else {
    $bootstrap.OvmfVarsTemplatePath
    }
    $ovmfVars = if ($OvmfVarsPath -ne "") {
        [System.IO.Path]::GetFullPath($OvmfVarsPath)
    } else {
        Join-Path $artifactDir "OVMF_VARS.fd"
    }

    if (($EfiPath -eq "") -and (-not $NoBuild)) {
        Write-Host "Building echOS (UEFI)..." -ForegroundColor Yellow
        $stageTimer = New-StageTimer
        cargo build --quiet --target x86_64-unknown-uefi
        if ($LASTEXITCODE -ne 0) { throw "UEFI build failed" }
        Write-StageElapsed "UEFI cargo build" $stageTimer
    }
    if (-not (Test-Path $efiPath)) { throw "EFI binary not found at $efiPath" }

    $applianceToolInputs = @(
        (Join-Path $projectRoot "Cargo.toml"),
        (Join-Path $projectRoot "src\bin\echos_appliance.rs")
    )
    if (Test-OutputFresh -OutputPath $builderPath -InputPaths $applianceToolInputs) {
        Write-Host "Reusing fresh Rust appliance tool." -ForegroundColor DarkGray
    } else {
        Write-Host "Building Rust appliance tool..." -ForegroundColor Yellow
        $stageTimer = New-StageTimer
        $hostBuildExit = Invoke-HostCargoBuild @("build", "--quiet", "--bin", "echos_appliance", "--target", "x86_64-pc-windows-msvc", "--features", "host_smoke")
        if ($hostBuildExit -ne 0) { throw "echos_appliance host build failed" }
        Write-StageElapsed "echos_appliance build" $stageTimer
    }
    if (-not (Test-Path $builderPath)) { throw "echos_appliance host tool not found at $builderPath" }

    if ($PackagedPeSmoke) {
        if ($NoAutoLogin) {
            throw "Packaged PE smoke auto-login gerektirir"
        }
        $peSmokeManifest = Join-Path $projectRoot "tools\\pe_smoke_windowed\\Cargo.toml"
        $echsdkPath = Join-Path $projectRoot "target\\x86_64-pc-windows-msvc\\debug\\echsdk.exe"
        $peSmokeRoot = Join-Path $projectRoot "tools\\pe_smoke_windowed"
        $peSmokeBundlePath = Join-Path $artifactDir "pe_smoke_windowed.bhd"

        Write-Host "Building packaged PE smoke sample..." -ForegroundColor Yellow
        $hostBuildExit = Invoke-HostCargoBuild @("build", "--quiet", "--release", "--target", "x86_64-pc-windows-msvc", "--manifest-path", $peSmokeManifest)
        if ($hostBuildExit -ne 0) { throw "PE smoke sample build failed" }

        Write-Host "Building echosdk host tool..." -ForegroundColor Yellow
        $hostBuildExit = Invoke-HostCargoBuild @("build", "--quiet", "--bin", "echsdk", "--target", "x86_64-pc-windows-msvc", "--features", "host_smoke")
        if ($hostBuildExit -ne 0) { throw "echsdk host build failed" }
        if (-not (Test-Path $echsdkPath)) { throw "echsdk host tool not found at $echsdkPath" }

        & $echsdkPath sign $peSmokeRoot developer $peSmokeBundlePath
        if ($LASTEXITCODE -ne 0) { throw "PE smoke bundle signing failed" }
    }

    if ($MixedUpdateSmoke) {
        if ($NoAutoLogin) {
            throw "Mixed update smoke auto-login gerektirir"
        }
        $echsdkPath = Join-Path $projectRoot "target\\x86_64-pc-windows-msvc\\debug\\echsdk.exe"
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
        $stageTimer = New-StageTimer
        $hostBuildExit = Invoke-HostCargoBuild @("build", "--quiet", "--bin", "echsdk", "--target", "x86_64-pc-windows-msvc", "--features", "host_smoke")
        if ($hostBuildExit -ne 0) { throw "echsdk host build failed" }
        Write-StageElapsed "echsdk build" $stageTimer
        if (-not (Test-Path $echsdkPath)) { throw "echsdk host tool not found at $echsdkPath" }

        Write-Host "Building mixed update platform image..." -ForegroundColor Yellow
        $stageTimer = New-StageTimer
        & $builderPath slot-image --output $platformImagePath --image-mib 8
        if ($LASTEXITCODE -ne 0) { throw "mixed update slot image build failed" }
        Write-StageElapsed "mixed update slot image" $stageTimer

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
        $stageTimer = New-StageTimer
        $httpServerProc = Start-Process -FilePath $python.Source -ArgumentList @("-m", "http.server", "$requestPort", "--bind", "0.0.0.0", "--directory", $mixedUpdateArtifactDir) -PassThru -RedirectStandardOutput $httpServerStdoutPath -RedirectStandardError $httpServerStderrPath
        Start-Sleep -Seconds 2
        if ($httpServerProc.HasExited) {
            throw "mixed update HTTP server exited early"
        }
        Write-StageElapsed "mixed update HTTP server start" $stageTimer
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
        "--efi", $efiPath,
        "--output", $imagePath,
        "--active-slot", "system_a",
        "--pending-slot", $PendingSlot,
        "--system-image-mib", "8",
        "--esp-fat", "fat32"
    )
    if ((-not $NoAutoLogin) -and (-not $effectiveShellSmokeTest)) {
        $builderArgs += "--auto-login"
    }
    if ($SuspendResumeSmoke) {
        $builderArgs += "--suspend-resume-smoke"
    }
    if ($FsSmokeTest) {
        $builderArgs += "--fs-smoke-test"
    }
    if ($ShellSmokeTest) {
        $builderArgs += "--shell-smoke-test"
    }
    if ($ShellCommandTest) {
        $builderArgs += "--shell-command-test"
    }
    if ($BootTests) {
        $builderArgs += "--boot-tests"
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
        (-not $FsSmokeTest) -and `
        (-not $effectiveShellSmokeTest) -and `
        (-not $BootTests) -and `
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
        $stageTimer = New-StageTimer
        & $builderPath @builderArgs
        if ($LASTEXITCODE -ne 0) { throw "Appliance image build failed" }
        Write-StageElapsed "raw GPT appliance disk" $stageTimer
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
    $ovmfCodeRuntimeName = if ($OvmfCodePath -ne "") { "OVMF_CODE.custom.fd" } else { "OVMF_CODE.fd" }
    $ovmfCodeRuntime = Join-Path $artifactDir $ovmfCodeRuntimeName
    if (($OvmfCodePath -ne "") -or (-not (Test-Path $ovmfCodeRuntime)) -or ((Get-Item -LiteralPath $ovmfCode).LastWriteTimeUtc -gt (Get-Item -LiteralPath $ovmfCodeRuntime -ErrorAction SilentlyContinue).LastWriteTimeUtc)) {
        Copy-Item $ovmfCode $ovmfCodeRuntime -Force
    }
    $ovmfCode = $ovmfCodeRuntime

    $displayArgs = if ($Headless -or ($EnableTpm -and -not $TpmGui)) {
        @("-display", "none")
    } else {
        @("-display", "gtk,grab-on-hover=on,zoom-to-fit=on")
    }
    $monitorHost = "127.0.0.1"
    $monitorPort = 45454
    $monitorEndpoint = "tcp:${monitorHost}:${monitorPort},server,nowait"
    $ovmfCodeDrive = "if=pflash,format=raw,readonly=on,file=$ovmfCode"
    $ovmfVarsDrive = "if=pflash,format=raw,file=$ovmfVars"
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
        "-m", ("{0}M" -f $MemoryMiB),
        "-monitor", $monitorEndpoint,
        "-no-reboot",
        "-no-shutdown",
        # Ag: guest init path VirtIO-Net'e bind eder; TCG smoke'ta e1000e log gürültüsü üretmez.
        "-netdev", "user,id=net0,hostfwd=tcp::$HostHttpPort-:80,hostfwd=tcp::$HostHttpsPort-:443",
        "-device", "$nicModel,netdev=net0,mac=52:54:00:12:34:56"
    ) + $displayArgs + $videoArgs + $accelArgs
    if ($EnableTpm) {
        $tpmState = if ($TpmStatePath -ne "") {
            [System.IO.Path]::GetFullPath($TpmStatePath)
        } else {
            Join-Path $artifactDir "tpm2"
        }
        Write-Host "TPM 2.0 emulator: state=$tpmState backend=WSL Unix socket" -ForegroundColor DarkGray
        $tpmRuntime = Start-TpmEmulator `
            -StatePath $tpmState `
            -RequestedPath $SwtpmPath `
            -ServerPort $TpmServerPort `
            -ControlPort $TpmControlPort `
            -StdoutPath $tpmStdoutPath `
            -StderrPath $tpmStderrPath
        $tpmProcess = $tpmRuntime.Process
        $tpmSocketPath = $tpmRuntime.SocketPath
        $qemuArgs += @(
            "-chardev", "socket,id=chrtpm,path=$tpmSocketPath",
            "-tpmdev", "emulator,id=tpm0,chardev=chrtpm",
            "-device", "tpm-tis,tpmdev=tpm0"
        )
    }
    if ($SuspendResumeSmoke) {
        $qemuArgs += @("-global", "ICH9-LPC.disable_s3=0")
    }
    if ($traceEnabled) {
        $qemuArgs += @("-d", "int,guest_errors,unimp,pcall,mmu,cpu_reset", "-D", $traceLogPath)
    }
    if ($Gdb) {
        $qemuArgs += @("-gdb", "tcp::${GdbPort},server,nowait")
        Write-Host "GDB endpoint: tcp::${GdbPort}" -ForegroundColor Green
    }
    if ($GdbWait) {
        $qemuArgs += "-S"
        Write-Host "QEMU paused (waiting for GDB). Connect with: gdb -x .gdbinit" -ForegroundColor Green
    }
}

function Assert-SerialMarkers {
    param(
        [string]$Path,
        [string[]]$Markers
    )

    $missing = @()
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $serialContent = if (Test-Path $Path) { Get-Content $Path -Raw -ErrorAction SilentlyContinue } else { "" }
        $missing = @()
        foreach ($marker in $Markers) {
            if (-not $serialContent.Contains($marker)) {
                $missing += $marker
            }
        }
        if ($missing.Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)

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
            $fallback += $tcgCpuModel
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
    $proc = Start-CleanProcess -FilePath $qemu -ArgumentList $phaseArgs -StdoutPath $phaseStdoutPath -StderrPath $phaseStderrPath
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
            if (-not (Wait-ProcessExitOrTimeout -Process $phase1.Process -TimeoutSec 30)) {
                Write-Host "Mixed update phase-1 reboot marker goruldu; QEMU cikmadi, faz gecisi icin durduruluyor." -ForegroundColor DarkYellow
                try { $phase1.Process.Kill() } catch {}
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
        $proc = Start-CleanProcess -FilePath $qemuLaunchFile -ArgumentList (Get-QemuLaunchArguments $qemuArgs) -StdoutPath $qemuStdoutPath -StderrPath $qemuStderrPath

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
                $cpuModel = $tcgCpuModel
                $qemuCpuCount = $fallbackCpuCount
                $qemuSmpArg = "sockets=1,cores=$qemuCpuCount,threads=1,maxcpus=$qemuCpuCount"
                $proc = Start-CleanProcess -FilePath $qemuLaunchFile -ArgumentList (Get-QemuLaunchArguments $qemuArgs) -StdoutPath $qemuStdoutPath -StderrPath $qemuStderrPath
            }
        }

$interactiveReady = $false
$bootMarkerTimeoutSec = if ($EnableTpm) { 240 } else { 90 }
$trustedBootSatisfied = $false
if ($TrustedBootSmoke -and -not $useIso) {
    $trustedMarkers = @(
        "[UEFI] EFI Entry Point Reached!",
        "[UEFI] Loaded image signature OK",
        "[TPM] Measure OK (PCR4)",
        "[TPM] Event log entries=",
        "[UEFI] Runtime services verified",
        "[UEFI] Secure Boot databases available"
    )
    foreach ($marker in $trustedMarkers) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker $marker -TimeoutSec $bootMarkerTimeoutSec)) {
            if (-not $proc.HasExited) { try { $proc.Kill() } catch {} }
            throw "Trusted boot marker görülmedi: $marker"
        }
    }
    $trustedBootSatisfied = $true
    if (-not $proc.HasExited) { try { $proc.Kill() } catch {} }
} elseif (-not $useIso) {
    $successMarkers = @(
        "[BOOTCTRL] stage=boot-control-loaded",
        "[BOOTCTRL] stage=kernel-core-ready",
        "[BOOTCTRL] stage=storage-mounted",
        "[BOOTCTRL] stage=display-ready"
    )
    if ((-not $NoAutoLogin) -and (-not $effectiveShellSmokeTest)) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[DESKTOP] session bootstrap step=login-visible" -TimeoutSec $bootMarkerTimeoutSec)) {
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
    } elseif ($ShellCommandTest) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SHELL_SMOKE] Ring 3 shell requested via boot control" -TimeoutSec 120)) {
            throw "Shell command test request marker not observed"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SHELL_TEST] command corpus queued bytes=" -TimeoutSec 120)) {
            throw "Shell command corpus queue marker not observed"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SHELL] Ring 3 shell spawned as task" -TimeoutSec 120)) {
            throw "Ring 3 shell spawn marker not observed"
        }
        $expectedShellMarkers = @(
            "ECHTEST:ECHO:PASS",
            "ECHTEST:VAR:42",
            "ECHTEST:ARITH:7",
            "ECHTEST:PRINTF:ok",
            "ECHTEST:IF:PASS",
            "ECHTEST:FOR:1",
            "ECHTEST:FOR:2",
            "ECHTEST:FOR:3",
            "ECHTEST:COLON:PASS",
            "ECHTEST:LENGTH:5",
            "ECHTEST:SUFFIX:bana",
            "ECHTEST:PREFIX:nana",
            "ECHTEST:GREEDY_SUFFIX:bn",
            "ECHTEST:GREEDY_PREFIX:",
            "ECHTEST:EVAL:PASS",
            "ECHTEST:END:PASS"
        )
        foreach ($marker in $expectedShellMarkers) {
            if (-not (Wait-FileMarker -Path $serialLogPath -Marker $marker -TimeoutSec 120)) {
                throw "Shell command marker not observed: $marker"
            }
        }
        $serialContent = if (Test-Path $serialLogPath) { Get-Content $serialLogPath -Raw -ErrorAction SilentlyContinue } else { "" }
        $fatalMarkers = @("USER FAULT:", "STACK_OVERFLOW", "PAGE_FAULT", "Idle task attempted to exit", "[PANIC]")
        foreach ($fatal in $fatalMarkers) {
            if ($serialContent.Contains($fatal)) {
                throw "Shell command test fatal marker observed: $fatal"
            }
        }
        Write-Host "[PASS] shell command test: Ring 3 echshell executed corpus without fatal fault markers" -ForegroundColor Green
        if ($Headless -and -not $proc.HasExited) {
            $proc.Kill()
        }
    } elseif ($ShellSmokeTest) {
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SHELL_SMOKE] Ring 3 shell requested via boot control" -TimeoutSec 120)) {
            throw "Shell smoke request marker not observed"
        }
        if (-not (Wait-FileMarker -Path $serialLogPath -Marker "[SHELL] Ring 3 shell spawned as task" -TimeoutSec 120)) {
            throw "Ring 3 shell spawn marker not observed"
        }
        $serialContent = if (Test-Path $serialLogPath) { Get-Content $serialLogPath -Raw -ErrorAction SilentlyContinue } else { "" }
        $fatalMarkers = @("USER FAULT:", "STACK_OVERFLOW", "PAGE_FAULT", "Idle task attempted to exit", "[PANIC]")
        foreach ($fatal in $fatalMarkers) {
            if ($serialContent.Contains($fatal)) {
                throw "Shell smoke fatal marker observed: $fatal"
            }
        }
        Write-Host "[PASS] shell smoke: Ring 3 echshell spawned without fatal fault markers" -ForegroundColor Green
        if ($Headless -and -not $proc.HasExited) {
            $proc.Kill()
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
        $preWakeContent = if (Test-Path $serialLogPath) { Get-Content $serialLogPath -Raw } else { "" }
        if ($preWakeContent.Contains("[SMOKE] suspend-resume fail:")) {
            try { $proc.Kill() } catch {}
            throw "Suspend/resume smoke failed before host wake"
        }
        $resumeOffset = Get-FileContentLength -Path $serialLogPath
        Send-MonitorCommand -MonitorHost $monitorHost -Port $monitorPort -Command "system_wakeup"
        $healthMarker = Wait-FileAnyMarkerAfterOffset -Path $serialLogPath -Markers @("[SMOKE] resume-health ok", "[SMOKE] resume-health fail", "[SMOKE] suspend-resume fail:") -Offset $resumeOffset -TimeoutSec 120
        if ($healthMarker -ne "[SMOKE] resume-health ok") {
            try { $proc.Kill() } catch {}
            if ([string]::IsNullOrEmpty($healthMarker)) {
                Write-Host "[FAIL] suspend-resume health marker missing after host wake" -ForegroundColor Red
                throw "Suspend/resume smoke did not complete after host wake"
            }
            Write-Host "[FAIL] suspend-resume health marker reported: $healthMarker" -ForegroundColor Red
            throw "Suspend/resume smoke health failed: $healthMarker"
        }
        if (-not (Wait-FileMarkerAfterOffset -Path $debugLogPath -Marker "S3R" -Offset 0 -TimeoutSec 10)) {
            try { $proc.Kill() } catch {}
            Write-Host "[FAIL] suspend-resume debugcon S3R marker missing" -ForegroundColor Red
            throw "Suspend/resume debugcon marker missing"
        }
        Write-Host "[PASS] suspend-resume smoke: serial health + debugcon S3R observed" -ForegroundColor Green
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
        if (Wait-FileMarker -Path $serialLogPath -Marker "[BOOTCTRL] success" -TimeoutSec $bootMarkerTimeoutSec) {
            $interactiveReady = $true
            if ($Headless -and -not $proc.HasExited) {
                try { $proc.Kill() } catch {}
            }
        }
    } else {
        if (Wait-FileMarker -Path $serialLogPath -Marker "[BOOTCTRL] stage=display-ready" -TimeoutSec $bootMarkerTimeoutSec) {
            $interactiveReady = $true
            if ($Headless -and -not $proc.HasExited) {
                try { $proc.Kill() } catch {}
            }
        }
    }
}

if ($interactiveReady -and (-not $Headless) -and (-not $WaitForExit)) {
    Assert-SerialMarkers -Path $serialLogPath -Markers $successMarkers
    # The GUI hand-off is intentional.  Keep QEMU, swtpm, and the Unix socket
    # alive after this PowerShell scope returns; tearing down the TPM backend
    # here makes the guest freeze immediately after the ready screen.
    $leaveQemuRunning = $true
    Write-Host "`nQEMU ready; leaving VM running and returning control to the shell." -ForegroundColor Green
    Write-Host "Use -WaitForExit if you want the script to block until the QEMU window closes." -ForegroundColor DarkGray
    $global:LASTEXITCODE = 0
    return
}

$proc.WaitForExit()

if ($TrustedBootSmoke -and $trustedBootSatisfied) {
    Write-Host "`nTrusted Boot firmware/TPM markers verified; QEMU stopped." -ForegroundColor Green
    $global:LASTEXITCODE = 0
    return
}

Write-Host "`nQEMU exited." -ForegroundColor Magenta

if (-not $useIso -and -not $TrustedBootSmoke) {
    $requiredMarkers = @(
        "[BOOTCTRL] stage=boot-control-loaded",
        "[BOOTCTRL] stage=kernel-core-ready",
        "[BOOTCTRL] stage=storage-mounted",
        "[BOOTCTRL] stage=display-ready"
    )
    if ((-not $NoAutoLogin) -and (-not $effectiveShellSmokeTest)) {
        if ($BootTests) {
            # Wave 4 acceptance runs the bounded suite before compositor
            # publication; the desktop contract is proven by its terminal
            # bootstrap markers, while BOOTCTRL success remains mandatory.
            $requiredMarkers += @(
                "[BOOTCTRL] success",
                "[DESKTOP] session bootstrap step=login-visible",
                "[DESKTOP] session bootstrap step=render-shell"
            )
        } else {
            $requiredMarkers += @(
                "[BOOTCTRL] stage=desktop-ready",
                "[BOOTCTRL] stage=app-basket-ready",
                "[BOOTCTRL] success"
            )
        }
    }
    if ($SuspendResumeSmoke) {
        $requiredMarkers += @(
            "[SMOKE] suspend-resume ok",
            "[SMOKE] resume-health ok"
        )
    }
    if ($PackagedPeSmoke) {
        $requiredMarkers += @(
            "[SMOKE] packaged-pe install ok",
            "[SMOKE] packaged-pe launch ok",
            "[WIN32] CreateWindowExA:"
        )
    }
    if ($BootTests) {
        $requiredMarkers += @(
            "[BOOT_TEST] PASS",
            "[RING3_TEST] PASS",
            "[VM_SECURITY_TEST] PASS",
            "[VM_STRESS_TEST] PASS",
            "[IRQ_STRESS_TEST] PASS"
        )
    }

    Assert-SerialMarkers -Path $serialLogPath -Markers $requiredMarkers
    if ($BootTests) {
        $serialText = Get-Content -LiteralPath $serialLogPath -Raw -ErrorAction SilentlyContinue
        $fatalMarkers = @("[PANIC]", "PAGE_FAULT", "DOUBLE_FAULT", "TRIPLE_FAULT", "phase_state=failed", "qemu: fatal", "guest_errors")
        $fatalSeen = @($fatalMarkers | Where-Object { $serialText -and $serialText.Contains($_) })
        if ($fatalSeen.Count -ne 0) {
            throw "UEFI Wave 4 smoke fatal marker: $($fatalSeen -join ', ')"
        }
    }
}

    }
} finally {
    if (-not $leaveQemuRunning -and $tpmProcess -and -not $tpmProcess.HasExited) {
        try { $tpmProcess.Kill() } catch {}
        try { $tpmProcess.WaitForExit() } catch {}
    }
    if (-not $leaveQemuRunning -and $tpmSocketPath) {
        try {
            $tpmPattern = "/tmp/echos-tpm-$stamp/swtpm.sock"
            $cleanupCommand = "for p in `$(pgrep -f -- 'swtpm socket.*$tpmPattern' || true); do kill `$p 2>/dev/null || true; done; rm -rf '/tmp/echos-tpm-$stamp'"
            & wsl.exe -e sh -lc $cleanupCommand 2>$null
        } catch {}
    }
    if (-not $leaveQemuRunning -and $httpServerProc -and -not $httpServerProc.HasExited) {
        try { $httpServerProc.Kill() } catch {}
        try { $httpServerProc.WaitForExit() } catch {}
    }
    if ($transcriptStarted) { Stop-Transcript | Out-Null }
    if ($originalLocation) { Set-Location -LiteralPath $originalLocation }
}
