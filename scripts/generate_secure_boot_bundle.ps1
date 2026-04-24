param(
    [string]$OutputDir = ".\artifacts\secure_boot",
    [string]$CommonNamePrefix = "echOS Test",
    [int]$ValidDays = 3650,
    [string]$UnsignedImage,
    [string]$SignedImage
)

$ErrorActionPreference = "Stop"

function Require-Tool {
    param([string]$Name)
    $tool = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $tool) {
        throw "Required tool '$Name' not found in PATH."
    }
    $tool.Source
}

function Invoke-Checked {
    param([string]$ToolPath, [string[]]$Arguments)
    & $ToolPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $ToolPath $($Arguments -join ' ')"
    }
}

function New-X509Pair {
    param(
        [string]$OpenSsl,
        [string]$Name,
        [string]$Cn,
        [string]$OutDir,
        [int]$Days
    )

    $key = Join-Path $OutDir "$Name.key"
    $crt = Join-Path $OutDir "$Name.crt"
    Invoke-Checked $OpenSsl @(
        "req", "-new", "-x509",
        "-newkey", "rsa:4096",
        "-sha256",
        "-nodes",
        "-subj", "/CN=$Cn/",
        "-keyout", $key,
        "-out", $crt,
        "-days", "$Days"
    )
    [pscustomobject]@{
        Key = $key
        Cert = $crt
    }
}

function New-EfiAuthSet {
    param(
        [string]$CertToEsl,
        [string]$SignEsl,
        [string]$VariableName,
        [string]$OwnerGuid,
        [string]$SignerKey,
        [string]$SignerCert,
        [string]$PayloadCert,
        [string]$OutDir
    )

    $esl = Join-Path $OutDir "$VariableName.esl"
    $auth = Join-Path $OutDir "$VariableName.auth"
    Invoke-Checked $CertToEsl @("-g", $OwnerGuid, $PayloadCert, $esl)
    Invoke-Checked $SignEsl @("-k", $SignerKey, "-c", $SignerCert, $VariableName, $esl, $auth)
    [pscustomobject]@{
        Esl = $esl
        Auth = $auth
    }
}

$openssl = Require-Tool "openssl"
$certToEsl = Require-Tool "cert-to-efi-sig-list"
$signEsl = Require-Tool "sign-efi-sig-list"

$resolvedOut = [System.IO.Path]::GetFullPath($OutputDir)
New-Item -ItemType Directory -Force -Path $resolvedOut | Out-Null

$ownerGuid = [guid]::NewGuid().Guid
$pk = New-X509Pair $openssl "PK" "$CommonNamePrefix Platform Key" $resolvedOut $ValidDays
$kek = New-X509Pair $openssl "KEK" "$CommonNamePrefix Key Exchange Key" $resolvedOut $ValidDays
$db = New-X509Pair $openssl "db" "$CommonNamePrefix Image Signing" $resolvedOut $ValidDays
$dbx = New-X509Pair $openssl "dbx-bootstrap" "$CommonNamePrefix Revocation Bootstrap" $resolvedOut $ValidDays

$pkAuth = New-EfiAuthSet $certToEsl $signEsl "PK" $ownerGuid $pk.Key $pk.Cert $pk.Cert $resolvedOut
$kekAuth = New-EfiAuthSet $certToEsl $signEsl "KEK" $ownerGuid $pk.Key $pk.Cert $kek.Cert $resolvedOut
$dbAuth = New-EfiAuthSet $certToEsl $signEsl "db" $ownerGuid $kek.Key $kek.Cert $db.Cert $resolvedOut
$dbxAuth = New-EfiAuthSet $certToEsl $signEsl "dbx" $ownerGuid $kek.Key $kek.Cert $dbx.Cert $resolvedOut

$manifest = [ordered]@{
    owner_guid = $ownerGuid
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    common_name_prefix = $CommonNamePrefix
    valid_days = $ValidDays
    keys = [ordered]@{
        PK = [ordered]@{ cert = "PK.crt"; key = "PK.key"; esl = "PK.esl"; auth = "PK.auth" }
        KEK = [ordered]@{ cert = "KEK.crt"; key = "KEK.key"; esl = "KEK.esl"; auth = "KEK.auth" }
        db = [ordered]@{ cert = "db.crt"; key = "db.key"; esl = "db.esl"; auth = "db.auth" }
        dbx = [ordered]@{ cert = "dbx-bootstrap.crt"; key = "dbx-bootstrap.key"; esl = "dbx.esl"; auth = "dbx.auth" }
    }
    notes = @(
        "PK signs KEK updates; KEK signs db/dbx updates.",
        "dbx currently carries a bootstrap revoked certificate so the variable exists and the runtime parser stays fail-closed.",
        "Replace dbx-bootstrap with real revoked hashes/certs before production distribution."
    )
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $resolvedOut "secure_boot_manifest.json")

$readme = @"
echOS Secure Boot bundle

Owner GUID: $ownerGuid

Files:
- PK.auth / PK.esl
- KEK.auth / KEK.esl
- db.auth / db.esl
- dbx.auth / dbx.esl
- PK.crt|key, KEK.crt|key, db.crt|key, dbx-bootstrap.crt|key

Suggested order:
1. Enroll PK.auth
2. Enroll KEK.auth
3. Enroll db.auth
4. Enroll dbx.auth
5. Sign your EFI image with db.crt + db.key

Use docs/architecture/secure_boot_local_flow.md for the full flow.
"@
$readme | Set-Content -Encoding utf8 (Join-Path $resolvedOut "README.txt")

if ($UnsignedImage) {
    $defaultSigned = if ($SignedImage) { $SignedImage } else { Join-Path $resolvedOut "BOOTX64-signed.EFI" }
    & (Join-Path $PSScriptRoot "sign_uefi_secure_boot.ps1") `
        -UnsignedImage $UnsignedImage `
        -SignedImage $defaultSigned `
        -Certificate $db.Cert `
        -PrivateKey $db.Key
    if ($LASTEXITCODE -ne 0) {
        throw "EFI image signing failed."
    }
}

Write-Output "Secure Boot bundle generated: $resolvedOut"
