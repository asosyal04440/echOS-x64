#!/usr/bin/env python3
"""
Prepare disk image with ESP for echOS
This script sets up the disk image with necessary files for booting echOS
"""

import os
import shutil
import sys

def prepare_disk():
    # Paths
    base_dir = os.path.dirname(os.path.abspath(__file__))
    disk_img = os.path.join(base_dir, "images", "echos-disk.img")
    esp_src = os.path.join(base_dir, "..", "..", "..", "..", "esp")
    esp_dest = os.path.join(base_dir, "images", "esp-temp")
    
    # Check if disk image exists
    if not os.path.exists(disk_img):
        print("Error: Disk image not found:", disk_img)
        return False
        
    # Check if ESP source exists
    if not os.path.exists(esp_src):
        print("Error: ESP source not found:", esp_src)
        return False
    
    print("Preparing disk image for echOS...")
    print("Source ESP:", esp_src)
    print("Disk image:", disk_img)
    
    # Here we would normally:
    # 1. Mount the disk image
    # 2. Format it as GPT/FAT32
    # 3. Copy ESP files to it
    
    # Since that requires special tools, we'll just ensure the ESP files are properly structured
    print("\nESP preparation steps (manual):")
    print("1. Mount echos-disk.img in a VM or with ImDisk")
    print("2. Format as GPT with FAT32 EFI System Partition")
    print("3. Copy contents of esp/ directory to the mounted drive")
    print("4. Unmount the disk image")
    
    # For now, just copy the ESP structure to images directory as reference
    try:
        if os.path.exists(esp_dest):
            shutil.rmtree(esp_dest)
        
        shutil.copytree(esp_src, esp_dest)
        print("\nESP structure copied to images/esp-temp for reference")
        
        # Update echos-system.simics to point to our ESP
        system_config = os.path.join(base_dir, "echos-system.simics")
        if os.path.exists(system_config):
            print("\nPlease update echos-system.simics to use the prepared disk image")
            
        return True
    except Exception as e:
        print("Error preparing disk:", str(e))
        return False

if __name__ == "__main__":
    success = prepare_disk()
    if success:
        print("\nDisk preparation completed!")
        print("Next steps:")
        print("1. Run Simics: cd C:\\Users\\Bahadir\\Desktop\\dersler_ve_projeler\\echOS\\Simics\\echos-simics")
        print("2. Execute: simics.bat --project . --target echos")
    else:
        print("\nDisk preparation failed!")
        sys.exit(1)
