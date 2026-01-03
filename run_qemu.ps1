$ErrorActionPreference = "Stop"

Write-Host "echOS QEMU Boot (kernel mode)" -ForegroundColor Cyan
Write-Host "=====================================`n" -ForegroundColor Cyan

# ---------- Paths ----------
$projectRoot = (Get-Location).Path
$efiPath = Join-Path $projectRoot "target\x86_64-unknown-uefi\debug\ech_os.efi"

# Use QEMU's bundled EDK2 firmware
$qemuShare = "C:\Program Files\qemu\share"
$ovmfCode = Join-Path $qemuShare "edk2-x86_64-code.fd"
$ovmfVarsTemplate = Join-Path $qemuShare "edk2-i386-vars.fd"
$ovmfVars = Join-Path $projectRoot "OVMF_VARS.fd"

# ---------- Build ----------
Write-Host "Building echOS..." -ForegroundColor Yellow
cargo build --quiet
if ($LASTEXITCODE -ne 0) { Write-Error "Build failed"; exit 1 }

# ---------- Verify EFI binary ----------
if (-Not (Test-Path $efiPath)) {
    Write-Error "EFI binary not found at $efiPath"
    exit 1
}

# ---------- Copy EFI to ESP ----------
Write-Host "Copying EFI to ESP folder..." -ForegroundColor Yellow
Copy-Item $efiPath "esp\EFI\BOOT\BOOTX64.EFI" -Force

# ---------- Create vars file if needed ----------
if (-Not (Test-Path $ovmfVars)) {
    Write-Host "Creating OVMF_VARS.fd from template..." -ForegroundColor Yellow
    Copy-Item $ovmfVarsTemplate $ovmfVars -Force
}

# ---------- Run QEMU ----------
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$qemuArgs = @(
    "-drive", "if=pflash,format=raw,readonly=on,file=$ovmfCode",
    "-drive", "if=pflash,format=raw,file=$ovmfVars",
    "-drive", "format=raw,file=fat:rw:esp",
    "-serial", "file:serial.log",
    "-m", "256M",
    "-display", "gtk",
    "-no-reboot",
    "-no-shutdown"
)

Write-Host "Launching QEMU...`n" -ForegroundColor Yellow
& $qemu @qemuArgs

Write-Host "`nQEMU exited." -ForegroundColor Magenta
