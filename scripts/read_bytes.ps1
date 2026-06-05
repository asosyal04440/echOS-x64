$bytes = [System.IO.File]::ReadAllBytes('target\x86_64-unknown-uefi\debug\ech_os.efi')
$offset = 0x266A81
$base = 0x140001000
for ($i = -8; $i -lt 24; $i++) {
    $pos = $offset + $i
    $val = $bytes[$pos]
    $marker = if ($i -eq 0) { ' <-- CRASH RIP' } else { '' }
    Write-Host ('{0:X8}: {1:X2}{2}' -f ($base + $pos), $val, $marker)
}
