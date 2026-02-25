# echOS Triple Fault Debug Script
# Bu script QEMU'yu debug modunda çalıştırır ve hata logunu kaydeder

Write-Host "=== echOS Triple Fault Debug ===" -ForegroundColor Cyan
Write-Host ""

# QEMU debug flags:
# -d int     : Log interrupts
# -d cpu_reset: Log CPU resets
# -d exec    : Log executed instructions (çok verbose)
# -d in_asm  : Log incoming assembly (çok verbose)

$debugFlags = "int,cpu_reset"
$logFile = "qemu_debug.log"

# OVMF dosyaları
$ovmfCode = "ovmf\OVMF_CODE.fd"
$ovmfVars = "OVMF_VARS.fd"
$efiApp = "esp\EFI\BOOT\BOOTX64.EFI"

# QEMU arguments
$qemuArgs = @(
    "-m", "2G",
    "-cpu", "qemu64,+smep,+smap",
    "-machine", "q35,accel=tcg",
    "-drive", "if=pflash,format=raw,readonly=on,file=$ovmfCode",
    "-drive", "if=pflash,format=raw,file=$ovmfVars",
    "-drive", "format=raw,file=fat:rw:esp",
    "-net", "none",
    "-nographic",
    "-debugcon", "stdio",  # Serial output to console
    "-d", $debugFlags,
    "-D", $logFile
)

Write-Host "QEMU Debug Mode:" -ForegroundColor Yellow
Write-Host "  Flags: $debugFlags"
Write-Host "  Log file: $logFile"
Write-Host ""
Write-Host "Starting QEMU..." -ForegroundColor Green
Write-Host "Look for 'Triple fault' or 'exception' in output" -ForegroundColor Cyan
Write-Host ""

# Run QEMU
& qemu-system-x86_64 @qemuArgs 2>&1

Write-Host ""
Write-Host "=== Debug Complete ===" -ForegroundColor Cyan
Write-Host "Check $logFile for detailed debug info"