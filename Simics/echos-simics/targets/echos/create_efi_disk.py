#!/usr/bin/env python3
"""
Create proper UEFI bootable disk image with correct EFI/Boot/BOOTX64.EFI path
"""

import struct
import os

def create_fat32_esp(disk_path, efi_file, size_mb=100):
    """Create a raw disk image with GPT + FAT32 ESP containing BOOTX64.EFI"""
    
    sector_size = 512
    total_sectors = (size_mb * 1024 * 1024) // sector_size
    
    # Read EFI binary
    with open(efi_file, 'rb') as f:
        efi_data = f.read()
    
    efi_size = len(efi_data)
    print(f"EFI binary: {efi_size} bytes")
    
    # FAT32 parameters
    sectors_per_cluster = 1
    cluster_size = sectors_per_cluster * sector_size
    
    # Partition layout
    gpt_sectors = 34  # GPT header + partition table
    partition_start = gpt_sectors  # LBA 34
    partition_sectors = total_sectors - gpt_sectors - 33  # Leave room for backup GPT
    
    # FAT32 layout within partition
    reserved_sectors = 32
    fat_sectors = (partition_sectors // 10) + 1  # ~10% for FAT
    if fat_sectors < 32:
        fat_sectors = 32
    
    data_start_sector = reserved_sectors + 2 * fat_sectors
    total_data_sectors = partition_sectors - data_start_sector
    total_clusters = total_data_sectors // sectors_per_cluster
    
    print(f"Partition: LBA {partition_start}, {partition_sectors} sectors")
    print(f"FAT: {fat_sectors} sectors each, 2 FATs")
    print(f"Data: {total_clusters} clusters")
    
    with open(disk_path, 'wb') as f:
        # === Protective MBR (LBA 0) ===
        mbr = bytearray(512)
        # Partition entry at offset 446
        mbr[446] = 0x00  # Boot flag
        mbr[447:450] = bytes([0x00, 0x02, 0x00])  # CHS start
        mbr[450] = 0xEE  # GPT protective type
        mbr[451:454] = bytes([0xFF, 0xFF, 0xFF])  # CHS end
        mbr[454:458] = struct.pack('<I', 1)  # LBA start
        mbr[458:462] = struct.pack('<I', total_sectors - 1)  # Sectors
        mbr[510:512] = b'\x55\xAA'
        f.write(mbr)
        
        # === GPT Header (LBA 1) ===
        gpt = bytearray(512)
        gpt[0:8] = b'EFI PART'
        gpt[8:12] = struct.pack('<I', 0x00010000)  # Version 1.0
        gpt[12:16] = struct.pack('<I', 92)  # Header size
        gpt[16:20] = struct.pack('<I', 0)  # CRC (placeholder)
        gpt[20:24] = struct.pack('<I', 0)  # Reserved
        gpt[24:32] = struct.pack('<Q', 1)  # My LBA
        gpt[32:40] = struct.pack('<Q', total_sectors - 1)  # Alternate LBA
        gpt[40:48] = struct.pack('<Q', partition_start)  # First usable LBA
        gpt[48:56] = struct.pack('<Q', total_sectors - gpt_sectors)  # Last usable LBA
        gpt[56:72] = os.urandom(16)  # Disk GUID
        gpt[72:80] = struct.pack('<Q', 2)  # Partition entries LBA
        gpt[80:84] = struct.pack('<I', 128)  # Num entries
        gpt[84:88] = struct.pack('<I', 128)  # Entry size
        f.write(gpt)
        
        # === Partition Entry (LBA 2) ===
        # EFI System Partition type: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
        entry = bytearray(128)
        entry[0:16] = bytes([0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
                            0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B])
        entry[16:32] = os.urandom(16)  # Unique GUID
        entry[32:40] = struct.pack('<Q', partition_start)  # Start LBA
        entry[40:48] = struct.pack('<Q', total_sectors - gpt_sectors - 1)  # End LBA
        entry[56:72] = "EFI System Partition".encode('utf-16-le')[:72]
        f.write(entry)
        
        # Pad partition table (LBA 2-33 = 32 sectors total)
        # We wrote 1 entry (128 bytes), need 127 more entries + padding to fill 32 sectors
        f.write(bytes(128 * 127))  # Rest of 127 entries
        # Partition table is now 128 * 128 = 16384 bytes = 32 sectors (LBA 2-33)
        
        # === FAT32 Boot Sector (LBA 34 = partition_start) ===
        boot = bytearray(512)
        boot[0:3] = b'\xEB\x58\x90'  # Jump instruction
        boot[3:11] = b'MSDOS5.0'  # OEM name
        boot[11:13] = struct.pack('<H', sector_size)  # Bytes per sector
        boot[13] = sectors_per_cluster  # Sectors per cluster
        boot[14:16] = struct.pack('<H', reserved_sectors)  # Reserved sectors
        boot[16] = 2  # Number of FATs
        boot[17:19] = struct.pack('<H', 0)  # Root entries (0 for FAT32)
        boot[19:21] = struct.pack('<H', 0)  # Total sectors 16-bit
        boot[21] = 0xF8  # Media type (fixed disk)
        boot[22:24] = struct.pack('<H', 0)  # Sectors per FAT (old)
        boot[24:26] = struct.pack('<H', 32)  # Sectors per track
        boot[26:28] = struct.pack('<H', 64)  # Heads
        boot[28:32] = struct.pack('<I', 0)  # Hidden sectors
        boot[32:36] = struct.pack('<I', partition_sectors)  # Total sectors 32-bit
        
        # FAT32 extended boot record
        boot[36:40] = struct.pack('<I', fat_sectors)  # Sectors per FAT
        boot[40:42] = struct.pack('<H', 0)  # Extended flags
        boot[42:44] = struct.pack('<H', 0)  # FS version
        boot[44:48] = struct.pack('<I', 2)  # Root cluster
        boot[48:50] = struct.pack('<H', 1)  # FS info sector
        boot[50:52] = struct.pack('<H', 6)  # Backup boot sector
        boot[64:72] = b'FAT32   '  # FS type label
        boot[510:512] = b'\x55\xAA'  # Signature
        f.write(boot)
        
        # FSInfo sector (LBA partition_start + 1)
        fsinfo = bytearray(512)
        fsinfo[0:4] = b'\x52\x52\x61\x41'  # Lead signature
        fsinfo[484:488] = b'\x72\x72\x41\x61'  # Struct signature
        fsinfo[488:492] = struct.pack('<I', 0xFFFFFFFF)  # Free clusters
        fsinfo[492:496] = struct.pack('<I', 2)  # Next free cluster
        fsinfo[510:512] = b'\x55\xAA'
        f.write(fsinfo)
        
        # Pad reserved sectors (LBA partition_start + 2 to partition_start + 31)
        f.write(bytes(512 * 30))
        
        # === FAT1 (LBA partition_start + reserved_sectors) ===
        fat1_pos = f.tell()
        fat = bytearray(fat_sectors * 512)
        # Cluster 0-1: media type and reserved
        fat[0:4] = b'\xF8\xFF\xFF\x0F'  # Cluster 0
        fat[4:8] = b'\xFF\xFF\xFF\x0F'  # Cluster 1
        # Cluster 2: root directory (EOF)
        fat[8:12] = b'\xFF\xFF\xFF\x0F'
        f.write(fat)
        
        # === FAT2 (copy) ===
        f.write(fat)
        
        # === Data region starts at partition_start + data_start_sector ===
        data_base = partition_start + data_start_sector
        
        def cluster_to_offset(cluster):
            return data_base * sector_size + (cluster - 2) * cluster_size
        
        # Calculate clusters needed for EFI file
        efi_clusters = (efi_size + cluster_size - 1) // cluster_size
        efi_start_cluster = 3  # First data cluster after root
        
        # Update FAT for EFI file chain
        for i in range(efi_clusters - 1):
            fat[12 + i*4 : 16 + i*4] = struct.pack('<I', efi_start_cluster + i + 1)
        fat[12 + (efi_clusters-1)*4 : 16 + (efi_clusters-1)*4] = b'\xFF\xFF\xFF\x0F'  # EOF
        
        # Write updated FAT to both copies
        f.seek(fat1_pos)
        f.write(fat)
        f.seek(fat1_pos + fat_sectors * 512)
        f.write(fat)
        
        # === Root directory (Cluster 2) ===
        # Entry: EFI directory
        root_dir = bytearray(cluster_size)
        entry = bytearray(32)
        entry[0:11] = b'EFI        '  # Name (8.3 format, directory)
        entry[11] = 0x10  # Attribute: directory
        entry[26:28] = struct.pack('<H', 4)  # First cluster (low)
        entry[20:22] = struct.pack('<H', 0)  # First cluster (high)
        root_dir[0:32] = entry
        f.seek(cluster_to_offset(2))
        f.write(root_dir)
        
        # === EFI directory (Cluster 4) ===
        # Entry: Boot directory
        efi_dir = bytearray(cluster_size)
        entry = bytearray(32)
        entry[0:11] = b'BOOT       '  # Name
        entry[11] = 0x10  # Attribute: directory
        entry[26:28] = struct.pack('<H', 5)  # First cluster
        entry[20:22] = struct.pack('<H', 0)
        efi_dir[0:32] = entry
        f.seek(cluster_to_offset(4))
        f.write(efi_dir)
        
        # === EFI/Boot directory (Cluster 5) ===
        # Entry: BOOTX64.EFI
        boot_dir = bytearray(cluster_size)
        entry = bytearray(32)
        entry[0:11] = b'BOOTX64EFI'  # Name (BOOTX64.EFI -> BOOTX64EFI)
        entry[11] = 0x20  # Attribute: archive
        entry[26:28] = struct.pack('<H', efi_start_cluster & 0xFFFF)  # First cluster low
        entry[20:22] = struct.pack('<H', efi_start_cluster >> 16)  # First cluster high
        entry[28:32] = struct.pack('<I', efi_size)  # File size
        boot_dir[0:32] = entry
        f.seek(cluster_to_offset(5))
        f.write(boot_dir)
        
        # === Write EFI binary data ===
        f.seek(cluster_to_offset(efi_start_cluster))
        f.write(efi_data)
        
        # Pad to total size
        current = f.tell()
        target = total_sectors * sector_size
        if current < target:
            f.write(bytes(target - current))
    
    print(f"Created: {disk_path}")
    print(f"Size: {size_mb}MB")
    print(f"ESP: FAT32 with /EFI/BOOT/BOOTX64.EFI ({efi_size} bytes)")
    return disk_path

if __name__ == "__main__":
    disk = "images/echos-efi.img"
    efi = r"c:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\target\x86_64-unknown-uefi\debug\ech_os.efi"
    create_fat32_esp(disk, efi, 100)
