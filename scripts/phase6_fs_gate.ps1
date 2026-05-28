param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [switch]$SkipFullTests
)

$ErrorActionPreference = "Stop"

function Invoke-Gate {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string[]]$Args
    )

    $started = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Write-Host "[$started] phase6 gate start: $Name"
    & cargo @Args
    if ($LASTEXITCODE -ne 0) {
        throw "phase6 gate failed: $Name (exit=$LASTEXITCODE)"
    }
    $finished = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Write-Host "[$finished] phase6 gate ok: $Name"
}

Invoke-Gate "xfstests_corpus" @("test", "--target", $Target, "--test", "xfstests_corpus", "-q")
Invoke-Gate "crash_consistency_corpus" @("test", "--target", $Target, "--test", "crash_consistency_corpus", "-q")
Invoke-Gate "fsck_oracle" @("test", "--target", $Target, "--test", "fsck_oracle", "-q")
Invoke-Gate "corrupt_image_hardening" @("test", "--target", $Target, "--test", "corrupt_image_hardening", "-q")
Invoke-Gate "fs_backend" @("test", "--target", $Target, "--test", "fs_backend", "-q")
Invoke-Gate "fs_loopback" @("test", "--target", $Target, "--test", "fs_loopback", "-q")
Invoke-Gate "fs_notify" @("test", "--target", $Target, "--test", "fs_notify", "-q")
Invoke-Gate "fs_path" @("test", "--target", $Target, "--test", "fs_path", "-q")
Invoke-Gate "fs_vfs" @("test", "--target", $Target, "--test", "fs_vfs", "-q")
Invoke-Gate "fs_package" @("test", "--target", $Target, "--test", "fs_package", "-q")

if (-not $SkipFullTests) {
    Invoke-Gate "all-tests-build-and-run" @("test", "--target", $Target, "--tests", "-q")
}

Write-Host "phase6 fs gate complete"
