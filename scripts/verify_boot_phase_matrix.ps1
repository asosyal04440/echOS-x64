[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$UefiLog,
    [Parameter(Mandatory)][string]$LimineLog,
    [Parameter(Mandatory)][string]$Multiboot2Log,
    [switch]$AllowMissingFiles
)

$ErrorActionPreference = "Stop"
$expected = @(
    "boot-context", "memory-layout", "memory-ownership", "heap-init",
    "core-privileges", "interrupt-foundation", "platform-services",
    "scheduling", "multiprocessing", "interrupt-enable", "services-drivers",
    "services", "userspace-ready", "running"
)
$fatal = @(
    "[PANIC]", "PAGE_FAULT", "DOUBLE_FAULT", "TRIPLE_FAULT",
    "qemu: fatal", "guest_errors", "phase_state=failed"
)

function Read-PhaseLog {
    param([string]$Path, [string]$Protocol)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        if ($AllowMissingFiles) { return }
        throw "Phase matrix log bulunamadı: $Path"
    }
    $text = Get-Content -LiteralPath $Path -Raw
    $bad = @($fatal | Where-Object { $text.Contains($_) })
    if ($bad.Count -ne 0) { throw "$Protocol log fatal marker içeriyor: $($bad -join ', ') ($Path)" }
    if (-not $text.Contains("[BOOTCTRL] success")) {
        throw "$Protocol logunda [BOOTCTRL] success marker yok: $Path"
    }

    $rx = [regex]'phase_name=([^\r\n]+)\r?\nphase_state=([^\r\n]+)\r?\nprotocol=([^\r\n]+)'
    $matches = $rx.Matches($text)
    if ($matches.Count -eq 0) { throw "$Protocol logunda [BOOT_PHASE] kaydı yok: $Path" }
    $records = foreach ($m in $matches) {
        [pscustomobject]@{ Name=$m.Groups[1].Value; State=$m.Groups[2].Value; Protocol=$m.Groups[3].Value }
    }
    $names = @($records | ForEach-Object Name | Select-Object -Unique)
    $missing = @($expected | Where-Object { $names -notcontains $_ })
    if ($missing.Count -ne 0) { throw "$Protocol phase eksik: $($missing -join ', ') ($Path)" }
    if ($names.Count -ne $expected.Count) {
        throw "$Protocol faz sırası beklenen Wave 3 sözleşmesiyle aynı değil: $($names -join ' -> ') ($Path)"
    }
    for ($index = 0; $index -lt $expected.Count; $index++) {
        if ($names[$index] -ne $expected[$index]) {
            throw "$Protocol faz sırası beklenen Wave 3 sözleşmesiyle aynı değil: $($names -join ' -> ') ($Path)"
        }
    }
    $terminal = @($records | Where-Object { $_.Name -eq "running" -and $_.State -eq "running" -and $_.Protocol -eq $Protocol })
    if ($terminal.Count -eq 0) { throw "$Protocol terminal running/running marker yok: $Path" }
    $wrong = @($records | Where-Object { $_.Protocol -ne $Protocol })
    if ($wrong.Count -ne 0) { throw "$Protocol logunda farklı protocol marker'ı var: $Path" }
    [pscustomobject]@{ Protocol=$Protocol; Path=$Path; Records=$records; Terminal=$terminal[-1] }
}

$results = @(
    (Read-PhaseLog -Path $UefiLog -Protocol "uefi"),
    (Read-PhaseLog -Path $LimineLog -Protocol "limine"),
    (Read-PhaseLog -Path $Multiboot2Log -Protocol "multiboot2")
)
foreach ($result in $results) {
    Write-Host ("{0}: {1} phases, terminal {2}/{3}" -f $result.Protocol, $result.Records.Count, $result.Terminal.Name, $result.Terminal.State) -ForegroundColor Green
}
Write-Host "Wave 3 three-protocol phase matrix PASS" -ForegroundColor Green
