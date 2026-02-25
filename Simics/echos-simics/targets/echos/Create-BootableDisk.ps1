# Create-BootableDisk.ps1
# Creates UEFI bootable disk image for Simics

param(
    [string]$DiskImage = "targets/echos/images/echos-disk.img",
    [string]$EfiSource = "target/x86_64-unknown-uefi/debug/ech_os.efi",
    [int]$DiskSizeMB = 512
)

Write-Host "Creating UEFI bootable disk image for Simics..."

# Create raw disk image
$sizeBytes = $DiskSizeMB * 1024 * 1024
$fsutil = "fsutil"
& $fsutil file createnew $DiskImage $sizeBytes

Write-Host "Disk image created: $DiskImage ($DiskSizeMB MB)"

# Note: To make this bootable, we need to:
# 1. Create GPT partition table
# 2. Create EFI System Partition (FAT32)
# 3. Copy echOS.efi to \EFI\Boot\BOOTX64.EFI
# 
# This requires admin privileges and diskpart or third-party tools.
# For now, create a minimal ESP structure manually.

Write-Host ""
Write-Host "To complete disk preparation (requires admin):"
Write-Host "1. Mount disk image with ImDisk or similar"
Write-Host "2. Format as GPT with ESP (FAT32, ~100MB)"
Write-Host "3. Copy $EfiSource to ESP:\EFI\Boot\BOOTX64.EFI"
Write-Host ""
Write-Host "Alternative: Use Linux with qemu-img and mkfs.vfat"
