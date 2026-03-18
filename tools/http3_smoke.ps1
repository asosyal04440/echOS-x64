$ProgressPreference = 'SilentlyContinue'

$edgeCandidates = @(
  'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
  'C:\Program Files\Microsoft\Edge\Application\msedge.exe'
)
$edge = $edgeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) {
  Write-Output 'http3:ERR:edge-not-found'
  exit 1
}

$log = Join-Path $env:TEMP ('echos-http3-' + [Guid]::NewGuid().ToString() + '.json')

try {
  $edgeOutput = & $edge `
    --headless=new `
    --disable-gpu `
    --enable-quic `
    --origin-to-force-quic-on=cloudflare-quic.com:443 `
    --log-net-log=$log `
    --net-log-capture-mode=Everything `
    --dump-dom `
    https://cloudflare-quic.com/ 2>&1

  $netlog = if (Test-Path $log) { Get-Content $log -Raw } else { '' }
  $quicSeen = $netlog -match 'HTTP3_SESSION' `
    -or $netlog -match 'QUIC_SESSION' `
    -or $netlog -match '"next_proto":"h3"' `
    -or $netlog -match '"protocol":"h3"' `
    -or $netlog -match 'h3'
  if (-not $quicSeen) {
    throw 'quic-netlog-marker-missing'
  }

  $status = if ($edgeOutput -match 'cloudflare-quic') { '200' } else { 'unknown' }
  Write-Output ('http3:OK:' + $status + ':edge-quic')
} catch {
  $message = $_.Exception.GetBaseException().Message.Replace("`r", ' ').Replace("`n", ' ')
  Write-Output ('http3:ERR:' + $message)
  exit 1
} finally {
  if (Test-Path $log) {
    Remove-Item $log -Force
  }
}
