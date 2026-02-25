#!/usr/bin/env python3
"""
Create UEFI bootable disk image for Simics
Creates GPT disk with EFI System Partition containing echOS
"""

import struct
import sys
import os

# Constants
SECTOR_SIZE = 512
ESP_SIZE_MB = 100
DISK_SIZE_GB = 2

def create_gpt_disk(disk_path, efi_file_path):
    """Create GPT disk with ESP containing echOS.efi"""
    
    # Calculate sizes
    disk_size = DISK_SIZE_GB * 1024 * 1024 * 1024
    esp_size = ESP_SIZE_MB * 1024 * 1024
    
    # Sector counts
    total_sectors = disk_size // SECTOR_SIZE
    esp_sectors = esp_size // SECTOR_SIZE
    
    print(f"Creating {DISK_SIZE_GB}GB disk image...")
    print(f"Total sectors: {total_sectors}")
    print(f"ESP sectors: {esp_sectors}")
    
    # Create disk image
    with open(disk_path, 'wb') as f:
        # Write entire disk as zeros
        f.write(bytes(disk_size))
    
    print(f"Disk image created: {disk_path}")
    print("Next: Format with GPT and FAT32...")
    
    # Note: For proper FAT32 formatting, we need to use tools like:
    # - mkfs.vfat (Linux)
    # - format.com (Windows with admin)
    # - Or mount in QEMU and format there
    
    print("\nTo complete setup:")
    print("1. Mount disk image in QEMU or loopback")
    print("2. Format ESP partition as FAT32")
    print("3. Copy BOOTX64.EFI to \\EFI\\Boot\\")

if __name__ == "__main__":
    import os
    # Get the absolute paths
    base_dir = os.path.dirname(os.path.abspath(__file__))
    disk_path = os.path.join(base_dir, "images", "echos-disk.img")
    efi_path = os.path.join(base_dir, "..", "..", "..", "..", "target", "x86_64-unknown-uefi", "debug", "ech_os.efi")
    
    # Ensure images directory exists
    os.makedirs(os.path.dirname(disk_path), exist_ok=True)
    
    create_gpt_disk(disk_path, efi_path)
