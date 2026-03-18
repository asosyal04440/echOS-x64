$ProgressPreference = 'SilentlyContinue'
$query = [Convert]::FromBase64String('EjQBAAABAAAAAAAAB2V4YW1wbGUDY29tAAABAAE=')
$providers = @(
  @{ Name = 'cloudflare'; Ip = '1.1.1.1'; Sni = 'cloudflare-dns.com' },
  @{ Name = 'google'; Ip = '8.8.8.8'; Sni = 'dns.google' },
  @{ Name = 'quad9'; Ip = '9.9.9.9'; Sni = 'dns.quad9.net' }
)

foreach ($p in $providers) {
  $tcp = $null
  $ssl = $null
  try {
    $tcp = [System.Net.Sockets.TcpClient]::new()
    $tcp.ReceiveTimeout = 5000
    $tcp.SendTimeout = 5000
    $tcp.Connect($p.Ip, 853)

    $ssl = [System.Net.Security.SslStream]::new($tcp.GetStream(), $false)
    $ssl.ReadTimeout = 5000
    $ssl.WriteTimeout = 5000
    $ssl.AuthenticateAsClient($p.Sni)

    $prefix = [byte[]]@(
      (($query.Length -shr 8) -band 0xff),
      ($query.Length -band 0xff)
    )
    $ssl.Write($prefix, 0, 2)
    $ssl.Write($query, 0, $query.Length)
    $ssl.Flush()

    $lenBuf = New-Object byte[] 2
    $read = 0
    while ($read -lt 2) {
      $n = $ssl.Read($lenBuf, $read, 2 - $read)
      if ($n -le 0) { throw 'dot-length-eof' }
      $read += $n
    }

    $len = ([int]$lenBuf[0] -shl 8) -bor [int]$lenBuf[1]
    $resp = New-Object byte[] $len
    $read = 0
    while ($read -lt $len) {
      $n = $ssl.Read($resp, $read, $len - $read)
      if ($n -le 0) { throw 'dot-body-eof' }
      $read += $n
    }

    Write-Output ($p.Name + ':OK:' + $len)
  } catch {
    $message = $_.Exception.GetBaseException().Message.Replace("`r", ' ').Replace("`n", ' ')
    Write-Output ($p.Name + ':ERR:' + $message)
  } finally {
    if ($ssl) { $ssl.Dispose() }
    if ($tcp) { $tcp.Dispose() }
  }
}
