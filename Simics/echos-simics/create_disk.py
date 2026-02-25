#!/usr/bin/env python3
"""
Create UEFI bootable disk image for echOS Simics testing
"""

import struct
import sys
import os

def create_gpt_disk(disk_path, efi_file, disk_size_gb=2):
    """Create GPT disk with EFI System Partition"""
    
    sector_size = 512
    disk_size = disk_size_gb * 1024 * 1024 * 1024
    total_sectors = disk_size // sector_size
    
    # Read EFI file
    with open(efi_file, 'rb') as f:
        efi_data = f.read()
    
    print(f"Creating {disk_size_gb}GB disk...")
    
    with open(disk_path, 'wb') as f:
        # Protective MBR (LBA 0)
        mbr = bytearray(512)
        mbr[510:512] = b'\x55\xAA'
        f.write(mbr)
        
        # GPT Header (LBA 1)
        gpt_header = bytearray(512)
        gpt_header[0:8] = b'EFI PART'
        gpt_header[8:12] = struct.pack('<I', 0x00010000)  # Version
        gpt_header[12:16] = struct.pack('<I', 92)  # Header size
        gpt_header[16:20] = struct.pack('<I', 0)  # CRC (placeholder)
        gpt_header[20:24] = struct.pack('<I', 0)  # Reserved
        gpt_header[24:32] = struct.pack('<Q', 1)  # My LBA
        gpt_header[32:40] = struct.pack('<Q', total_sectors - 1)  # Alternate LBA
        gpt_header[40:48] = struct.pack('<Q', 34)  # First usable LBA
        gpt_header[48:56] = struct.pack('<Q', total_sectors - 34)  # Last usable LBA
        gpt_header[56:72] = bytes([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                                   0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00])  # Disk GUID
        gpt_header[72:80] = struct.pack('<Q', 2)  # Partition entry LBA
        gpt_header[80:84] = struct.pack('<I', 128)  # Number of entries
        gpt_header[84:88] = struct.pack('<I', 128)  # Entry size
        gpt_header[88:92] = struct.pack('<I', 0)  # CRC (placeholder)
        f.write(gpt_header)
        
        # Partition entries (LBA 2-33)
        # Entry 1: EFI System Partition
        entry = bytearray(128)
        # Type GUID: EFI System (C12A7328-F81F-11D2-BA4B-00A0C93EC93B)
        entry[0:16] = bytes([0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
                          0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B])
        # Unique GUID
        entry[16:32] = bytes([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                             0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10])
        # Start LBA (2048 = 1MB)
        entry[32:40] = struct.pack('<Q', 2048)
        # End LBA (2048 + 204800 - 1 = ~100MB)
        entry[40:48] = struct.pack('<Q', 206847)
        # Attributes
        entry[48:56] = struct.pack('<Q', 0)
        # Name (EFI System)
        name = "EFI System".encode('utf-16-le')
        entry[56:56+len(name)] = name
        f.write(entry)
        
        # Fill rest of partition table with zeros
        f.write(bytes(128 * 127))
        
        # FAT32 Boot Sector at LBA 2048
        fat32_start = 2048 * sector_size
        f.seek(fat32_start)
        
        boot_sector = bytearray(512)
        boot_sector[0:3] = b'\xEB\x58\x90'  # Jump
        boot_sector[3:11] = b'MSDOS5.0'   # OEM
        boot_sector[11:13] = struct.pack('<H', 512)  # Bytes per sector
        boot_sector[13] = 1  # Sectors per cluster
        boot_sector[14:16] = struct.pack('<H', 32)  # Reserved sectors
        boot_sector[16] = 2  # Number of FATs
        boot_sector[17:19] = struct.pack('<H', 0)  # Root entries (0 for FAT32)
        boot_sector[19:21] = struct.pack('<H', 0)  # Total sectors 16-bit
        boot_sector[21] = 0xF8  # Media type
        boot_sector[22:24] = struct.pack('<H', 0)  # Sectors per FAT
        boot_sector[24:26] = struct.pack('<H', 32)  # Sectors per track
        boot_sector[26:28] = struct.pack('<H', 64)  # Heads
        boot_sector[28:32] = struct.pack('<I', 2048)  # Hidden sectors
        boot_sector[32:36] = struct.pack('<I', 206848 - 2048)  # Total sectors 32-bit
        # FAT32 extended
        boot_sector[36:40] = struct.pack('<I', 101)  # Sectors per FAT
        boot_sector[40:42] = struct.pack('<H', 0)  # Extended flags
        boot_sector[42:44] = struct.pack('<H', 0)  # FS version
        boot_sector[44:48] = struct.pack('<I', 2)  # Root cluster
        boot_sector[48:50] = struct.pack('<H', 1)  # FS info sector
        boot_sector[50:52] = struct.pack('<H', 6)  # Backup boot sector
        boot_sector[64:70] = b'FAT32   '  # FS type
        boot_sector[510:512] = b'\x55\xAA'
        f.write(boot_sector)
        
        # FAT tables
        fat1_offset = fat32_start + 32 * 512
        f.seek(fat1_offset)
        f.write(b'\xF8\xFF\xFF\x0F')  # Media
        f.write(b'\xFF\xFF\xFF\x0F')  # Reserved
        f.write(b'\xFF\xFF\xFF\x0F')  # Root
        
        # Write EFI file at cluster 3 (offset 2048 + 32 + 101 + 101 + 1 = cluster 3)
        data_start = fat32_start + (32 + 101 + 101) * 512
        f.seek(data_start + 512)  # Cluster 3
        
        # Write directory entry for BOOTX64.EFI
        entry = bytearray(32)
        name = b'BOOTX64 EFI'
        entry[0:11] = name
        entry[11] = 0x20  # Archive
        entry[26:28] = struct.pack('<H', 3)  # Starting cluster
        entry[28:32] = struct.pack('<I', len(efi_data))
        f.write(entry)
        
        # Write actual EFI data at cluster 3
        f.seek(data_start + 512)
        f.write(efi_data)
        
        # FAT entry for cluster 3 = EOF
        f.seek(fat1_offset + 3 * 4)
        f.write(b'\xFF\xFF\xFF\x0F')
    
    print(f"Created: {disk_path}")
    print(f"Disk size: {disk_size_gb}GB")
    print(f"ESP: 100MB at offset 1MB")
    print(f"EFI file: {len(efi_data)} bytes")

if __name__ == "__main__":
    disk = "targets/echos/images/echos-disk.img"
    efi = "../../target/x86_64-unknown-uefi/debug/ech_os.efi"
    
    create_gpt_disk(disk, efi)
