$ErrorActionPreference = "Stop"

function Refresh-EchosProcessPath {
    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @($env:Path -split ';') + @($machinePath -split ';') + @($userPath -split ';')
    $env:Path = ($parts | Where-Object { $_ } | Select-Object -Unique) -join ';'
}

function Resolve-EchosCommand {
    param(
        [Parameter(Mandatory)]
        [string]$Name
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) {
        return $command.Source
    }

    $userProfile = [Environment]::GetFolderPath("UserProfile")
    $knownPaths = @(
        (Join-Path $userProfile ".cargo\bin\$Name"),
        (Join-Path $env:ProgramFiles "qemu\$Name"),
        (Join-Path ${env:ProgramFiles(x86)} "qemu\$Name"),
        (Join-Path $env:LOCALAPPDATA "Programs\qemu\$Name"),
        (Join-Path $env:ProgramFiles "LLVM\bin\$Name")
    )
    foreach ($knownPath in $knownPaths) {
        if ($knownPath -and (Test-Path -LiteralPath $knownPath)) {
            return (Resolve-Path -LiteralPath $knownPath).Path
        }
    }

    return $null
}

function Invoke-EchosWingetInstall {
    param(
        [Parameter(Mandatory)]
        [string]$PackageId,
        [Parameter(Mandatory)]
        [string]$DisplayName,
        [switch]$SkipInstall
    )

    if ($SkipInstall) {
        throw "$DisplayName bulunamadı. Otomatik kurulum kapalı olduğu için işlem durduruldu."
    }

    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) {
        throw "$DisplayName bulunamadı ve winget.exe kullanılamıyor. Windows App Installer'ı kurun veya -SkipBootstrap kullanmadan tekrar deneyin."
    }

    Write-Host "$DisplayName bulunamadı; winget ile otomatik kuruluyor..." -ForegroundColor Yellow
    & $winget.Source install --id $PackageId --exact --silent --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        throw "$DisplayName kurulamadı (winget exit code: $LASTEXITCODE)."
    }
    Refresh-EchosProcessPath
}

function Ensure-EchosRust {
    param(
        [Parameter(Mandatory)]
        [string]$ProjectRoot,
        [switch]$NeedBareMetal,
        [switch]$SkipInstall
    )

    $cargo = Resolve-EchosCommand "cargo.exe"
    if (-not $cargo) {
        Invoke-EchosWingetInstall -PackageId "Rustlang.Rustup" -DisplayName "Rustup/Rust" -SkipInstall:$SkipInstall
        $cargo = Resolve-EchosCommand "cargo.exe"
    }
    if (-not $cargo) {
        throw "cargo bulunamadı. Rustup kurulumundan sonra yeni bir terminal açıp tekrar deneyin."
    }

    $rustup = Resolve-EchosCommand "rustup.exe"
    if (-not $rustup) {
        throw "rustup bulunamadı; Rust kurulumunu tamamlayın."
    }

    $targets = @("x86_64-unknown-uefi")
    if ($NeedBareMetal) {
        $targets += "x86_64-unknown-none"
    }

    Push-Location -LiteralPath $ProjectRoot
    try {
        $installedTargets = @(& $rustup target list --installed 2>$null)
        foreach ($target in $targets) {
            if ($installedTargets -notcontains $target) {
                Write-Host "Rust target hazırlanıyor: $target" -ForegroundColor Yellow
                & $rustup target add $target
                if ($LASTEXITCODE -ne 0) {
                    throw "Rust target kurulamadı: $target"
                }
            }
        }
    } finally {
        Pop-Location
    }

    return $cargo
}

function Find-EchosQemuPath {
    $resolved = Resolve-EchosCommand "qemu-system-x86_64.exe"
    if ($resolved) {
        return $resolved
    }

    $wingetRoot = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages"
    if (Test-Path -LiteralPath $wingetRoot) {
        $packageRoots = Get-ChildItem -LiteralPath $wingetRoot -Directory -Filter "SoftwareFreedomConservancy.QEMU_*" -ErrorAction SilentlyContinue
        foreach ($packageRoot in $packageRoots) {
            $candidate = Get-ChildItem -LiteralPath $packageRoot.FullName -File -Filter "qemu-system-x86_64.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($candidate) {
                return $candidate.FullName
            }
        }
    }

    return $null
}

function Resolve-EchosOvmfFiles {
    param(
        [Parameter(Mandatory)]
        [string]$QemuPath
    )

    $qemuShare = Join-Path (Split-Path -Parent $QemuPath) "share"
    $codeNames = @("edk2-x86_64-code.fd", "OVMF_CODE.fd", "OVMF_CODE_4M.fd")
    $varsNames = @("edk2-i386-vars.fd", "edk2-x86_64-vars.fd", "OVMF_VARS.fd", "OVMF_VARS_4M.fd")
    $codePath = $null
    $varsPath = $null

    foreach ($name in $codeNames) {
        $candidate = Join-Path $qemuShare $name
        if (Test-Path -LiteralPath $candidate) {
            $codePath = (Resolve-Path -LiteralPath $candidate).Path
            break
        }
    }
    foreach ($name in $varsNames) {
        $candidate = Join-Path $qemuShare $name
        if (Test-Path -LiteralPath $candidate) {
            $varsPath = (Resolve-Path -LiteralPath $candidate).Path
            break
        }
    }

    if (-not $codePath -or -not $varsPath) {
        throw "QEMU bulundu fakat OVMF firmware dosyaları bulunamadı. Beklenen konum: $qemuShare"
    }

    return [pscustomobject]@{
        SharePath = $qemuShare
        CodePath = $codePath
        VarsTemplatePath = $varsPath
    }
}

function Ensure-EchosQemu {
    param(
        [switch]$SkipInstall
    )

    $qemu = Find-EchosQemuPath
    if (-not $qemu) {
        Invoke-EchosWingetInstall -PackageId "SoftwareFreedomConservancy.QEMU" -DisplayName "QEMU" -SkipInstall:$SkipInstall
        $qemu = Find-EchosQemuPath
    }
    if (-not $qemu) {
        throw "QEMU kurulamadı veya qemu-system-x86_64.exe PATH içinde bulunamadı."
    }

    $ovmf = Resolve-EchosOvmfFiles -QemuPath $qemu
    return [pscustomobject]@{
        QemuPath = $qemu
        QemuShare = $ovmf.SharePath
        OvmfCodePath = $ovmf.CodePath
        OvmfVarsTemplatePath = $ovmf.VarsTemplatePath
    }
}

function Ensure-EchosPython {
    param(
        [switch]$SkipInstall
    )

    $python = Resolve-EchosCommand "python.exe"
    if (-not $python) {
        Invoke-EchosWingetInstall -PackageId "Python.Python.3.12" -DisplayName "Python 3.12" -SkipInstall:$SkipInstall
        $python = Resolve-EchosCommand "python.exe"
    }
    if (-not $python) {
        throw "Python bulunamadı."
    }
    return $python
}

function Find-EchosVcVars {
    $vswhere = Get-Command vswhere.exe -ErrorAction SilentlyContinue
    if ($vswhere) {
        $installationPath = & $vswhere.Source -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1
        if ($installationPath) {
            $candidate = Join-Path $installationPath "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path -LiteralPath $candidate) {
                return $candidate
            }
        }
    }

    $visualStudioRoot = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio"
    if (Test-Path -LiteralPath $visualStudioRoot) {
        $candidate = Get-ChildItem -LiteralPath $visualStudioRoot -Filter "vcvars64.bat" -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($candidate) {
            return $candidate.FullName
        }
    }

    return $null
}

function Import-EchosVcEnvironment {
    param(
        [Parameter(Mandatory)]
        [string]$VcVarsPath
    )

    $command = 'call "' + $VcVarsPath + '" amd64 >nul 2>&1 && set'
    $environmentLines = & cmd.exe /d /s /c $command
    foreach ($line in $environmentLines) {
        if ($line -match '^([^=]+)=(.*)$') {
            [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process")
        }
    }
    Refresh-EchosProcessPath
}

function Ensure-EchosMsvc {
    param(
        [switch]$SkipInstall
    )

    if ((Resolve-EchosCommand "link.exe") -or (Resolve-EchosCommand "lld-link.exe")) {
        return
    }

    $vcvars = Find-EchosVcVars
    if (-not $vcvars) {
        Invoke-EchosWingetInstall `
            -PackageId "Microsoft.VisualStudio.2022.BuildTools" `
            -DisplayName "Visual Studio Build Tools (C++)" `
            -SkipInstall:$SkipInstall
        $vcvars = Find-EchosVcVars
    }
    if (-not $vcvars) {
        throw "MSVC linker bulunamadı. Visual Studio Build Tools içindeki Desktop development with C++ workload gereklidir."
    }

    Import-EchosVcEnvironment -VcVarsPath $vcvars
    if (-not (Resolve-EchosCommand "link.exe")) {
        throw "MSVC ortamı yüklenemedi; link.exe PATH içinde görünmüyor."
    }
}

function Initialize-EchosWindowsEnvironment {
    param(
        [Parameter(Mandatory)]
        [string]$ProjectRoot,
        [switch]$NeedBareMetal,
        [switch]$NeedPython,
        [switch]$NeedMsvc,
        [switch]$SkipInstall
    )

    Refresh-EchosProcessPath
    $cargo = Ensure-EchosRust -ProjectRoot $ProjectRoot -NeedBareMetal:$NeedBareMetal -SkipInstall:$SkipInstall
    if ($NeedMsvc) {
        Ensure-EchosMsvc -SkipInstall:$SkipInstall
    }
    $qemu = Ensure-EchosQemu -SkipInstall:$SkipInstall
    $python = if ($NeedPython) { Ensure-EchosPython -SkipInstall:$SkipInstall } else { $null }

    return [pscustomobject]@{
        CargoPath = $cargo
        QemuPath = $qemu.QemuPath
        QemuShare = $qemu.QemuShare
        OvmfCodePath = $qemu.OvmfCodePath
        OvmfVarsTemplatePath = $qemu.OvmfVarsTemplatePath
        PythonPath = $python
    }
}
