#!/usr/bin/env python3
"""
Format disk image with GPT and FAT32 ESP for echOS
This script prepares a bootable disk image for Simics
"""

import os
import subprocess
import sys
import shutil

def create_bootable_disk():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    disk_img = os.path.join(base_dir, "images", "echos-disk.img")
    esp_temp = os.path.join(base_dir, "images", "esp-temp")
    esp_target = os.path.join(base_dir, "images", "esp-formatted")
    
    print("Creating bootable disk image...")
    print("Base directory:", base_dir)
    print("Disk image:", disk_img)
    
    # Check if disk image exists
    if not os.path.exists(disk_img):
        print("Error: Disk image not found:", disk_img)
        return False
    
    # Check if ESP temp exists
    if not os.path.exists(esp_temp):
        print("Error: ESP temp directory not found:", esp_temp)
        return False
        
    print("\nDisk image found. To make it bootable:")
    print("1. The disk needs to be partitioned with GPT")
    print("2. An EFI System Partition (ESP) needs to be created")
    print("3. The ESP needs to be formatted as FAT32")
    print("4. EFI files need to be copied to the ESP")
    
    print("\nFor now, we'll prepare the structure for manual setup.")
    print("For full automation, you would need:")
    print("- A tool like 'parted' or 'fdisk' to create GPT partitions")
    print("- 'mkfs.fat' to format the ESP as FAT32")
    print("- 'mtools' or mounting to copy files to the ESP")
    
    # Create a simple setup guide
    setup_guide = f"""
ECHOS SIMICS BOOT SETUP GUIDE
============================

1. PREREQUISITES:
   - Install WSL2 with Ubuntu
   - Install required tools in WSL:
     sudo apt update
     sudo apt install parted dosfstools mtools

2. PREPARE DISK IMAGE IN WSL:
   # Convert raw image to a format that can be partitioned
   cp {disk_img.replace('\\', '/')} /tmp/echos-disk.img
   
   # Create GPT partition table
   sudo parted /tmp/echos-disk.img mklabel gpt
   
   # Create EFI System Partition (100MB)
   sudo parted /tmp/echos-disk.img mkpart primary fat32 1MiB 101MiB
   sudo parted /tmp/echos-disk.img set 1 boot on
   
   # Format ESP as FAT32
   sudo losetup -P /dev/loop99 /tmp/echos-disk.img
   sudo mkfs.fat -F32 /dev/loop99p1
   sudo losetup -d /dev/loop99
   
   # Mount and copy EFI files
   mkdir -p /tmp/esp-mount
   sudo losetup -P /dev/loop99 /tmp/echos-disk.img
   sudo mount /dev/loop99p1 /tmp/esp-mount
   sudo mkdir -p /tmp/esp-mount/EFI/BOOT
   sudo cp {esp_temp.replace('\\', '/').replace('C:', '/mnt/c')}/* /tmp/esp-mount/ -r
   sudo umount /tmp/esp-mount
   sudo losetup -d /dev/loop99
   
   # Copy back to Windows
   cp /tmp/echos-disk.img {disk_img.replace('\\', '/')}

3. ALTERNATIVE METHOD (MANUAL):
   - Use a tool like Rufus or BalenaEtcher to create a bootable image
   - Or manually partition and format using disk management tools

4. RUN IN SIMICS:
   cd C:\\Users\\Bahadir\\Desktop\\dersler_ve_projeler\\echOS\\Simics\\echos-simics
   simics.bat --project . --target echos
    """
    
    guide_file = os.path.join(base_dir, "SETUP_GUIDE.txt")
    with open(guide_file, "w") as f:
        f.write(setup_guide)
    
    print(f"\nSetup guide written to: {guide_file}")
    print("Follow the instructions in the guide to make the disk image bootable.")
    
    return True

if __name__ == "__main__":
    success = create_bootable_disk()
    if success:
        print("\n✓ Disk preparation steps completed!")
        print("Next: Follow SETUP_GUIDE.txt instructions to make disk bootable")
    else:
        print("\n✗ Disk preparation failed!")
        sys.exit(1)
