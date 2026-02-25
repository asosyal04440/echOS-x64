#!/usr/bin/env python3
"""Create UEFI bootable disk image for Simics"""
import struct
import os

DISK = 'images/echos-efi.img'
EFI = r'c:\Users\Bahadir\Desktop\dersler_ve_projeler\echOS\target\x86_64-unknown-uefi\debug\ech_os.efi'

SECTOR = 512
DISK_MB = 100

with open(EFI, 'rb') as f:
    efi_data = f.read()
efi_size = len(efi_data)
print(f'EFI: {efi_size} bytes')

total_sectors = DISK_MB * 1024 * 1024 // SECTOR
partition_start = 2048
partition_sectors = total_sectors - partition_start - 33

sectors_per_cluster = 2
cluster_size = sectors_per_cluster * SECTOR
reserved_sectors = 32
fat_sectors = 396

data_start_lba = partition_start + reserved_sectors + 2 * fat_sectors
print(f'Data LBA: {data_start_lba}')

def cluster_offset(c):
    return (data_start_lba + (c - 2) * sectors_per_cluster) * SECTOR

with open(DISK, 'wb') as f:
    # MBR
    mbr = bytearray(SECTOR)
    mbr[450] = 0xEE
    mbr[454:458] = struct.pack('<I', 1)
    mbr[458:462] = struct.pack('<I', total_sectors - 1)
    mbr[510:512] = b'\x55\xAA'
    f.write(mbr)
    
    # GPT Header
    gpt = bytearray(SECTOR)
    gpt[0:8] = b'EFI PART'
    gpt[8:12] = struct.pack('<I', 0x00010000)
    gpt[12:16] = struct.pack('<I', 92)
    gpt[24:32] = struct.pack('<Q', 1)
    gpt[32:40] = struct.pack('<Q', total_sectors - 1)
    gpt[40:48] = struct.pack('<Q', partition_start)
    gpt[48:56] = struct.pack('<Q', total_sectors - 33)
    gpt[56:72] = os.urandom(16)
    gpt[72:80] = struct.pack('<Q', 2)
    gpt[80:84] = struct.pack('<I', 128)
    gpt[84:88] = struct.pack('<I', 128)
    f.write(gpt)
    
    # Partition Entry
    entry = bytearray(128)
    entry[0:16] = bytes([0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B])
    entry[16:32] = os.urandom(16)
    entry[32:40] = struct.pack('<Q', partition_start)
    entry[40:48] = struct.pack('<Q', partition_start + partition_sectors - 1)
    entry[56:72] = 'EFI System Partition'.encode('utf-16-le')[:72]
    f.write(entry)
    f.write(bytes(32 * SECTOR - 128))
    
    # Gap to partition start
    f.write(bytes((partition_start - 34) * SECTOR))
    
    # FAT32 Boot Sector
    boot = bytearray(SECTOR)
    boot[0:3] = b'\xEB\x58\x90'
    boot[3:11] = b'MSDOS5.0'
    boot[11:13] = struct.pack('<H', SECTOR)
    boot[13] = sectors_per_cluster
    boot[14:16] = struct.pack('<H', reserved_sectors)
    boot[16] = 2
    boot[21] = 0xF8
    boot[24:26] = struct.pack('<H', 32)
    boot[26:28] = struct.pack('<H', 64)
    boot[32:36] = struct.pack('<I', partition_sectors)
    boot[36:40] = struct.pack('<I', fat_sectors)
    boot[44:48] = struct.pack('<I', 2)
    boot[48:50] = struct.pack('<H', 1)
    boot[50:52] = struct.pack('<H', 6)
    boot[64:72] = b'FAT32   '
    boot[510:512] = b'\x55\xAA'
    f.write(boot)
    
    # FSInfo
    fsinfo = bytearray(SECTOR)
    fsinfo[0:4] = b'\x52\x52\x61\x41'
    fsinfo[484:488] = b'\x72\x72\x41\x61'
    fsinfo[488:492] = struct.pack('<I', 0xFFFFFFFF)
    fsinfo[492:496] = struct.pack('<I', 2)
    fsinfo[510:512] = b'\x55\xAA'
    f.write(fsinfo)
    
    # Reserved
    f.write(bytes(SECTOR * 30))
    
    # FAT1
    fat1_pos = f.tell()
    fat = bytearray(fat_sectors * SECTOR)
    fat[0:4] = b'\xF8\xFF\xFF\x0F'  # Cluster 0
    fat[4:8] = b'\xFF\xFF\xFF\x0F'  # Cluster 1
    fat[8:12] = b'\xFF\xFF\xFF\x0F' # Cluster 2 (Root)
    fat[16:20] = b'\xFF\xFF\xFF\x0F' # Cluster 4 (EFI)
    fat[20:24] = b'\xFF\xFF\xFF\x0F' # Cluster 5 (BOOT)
    
    # EFI chain - place after directories (clusters 2,4,5 used)
    efi_clusters = (efi_size + cluster_size - 1) // cluster_size
    efi_start = 6  # After root(2), EFI dir(4), BOOT dir(5)
    for i in range(efi_clusters - 1):
        offset = (efi_start + i) * 4
        fat[offset : offset + 4] = struct.pack('<I', efi_start + i + 1)
    
    last_offset = (efi_start + efi_clusters - 1) * 4
    fat[last_offset : last_offset + 4] = b'\xFF\xFF\xFF\x0F'
    
    f.seek(fat1_pos)
    f.write(fat)
    f.seek(fat1_pos + fat_sectors * SECTOR)
    f.write(fat)
    
    # Root (Cluster 2) - EFI directory directly (no volume label)
    root_pos = cluster_offset(2)
    f.seek(root_pos)
    efi_entry = bytearray(32)
    efi_entry[0:11] = b'EFI        '
    efi_entry[11] = 0x10
    efi_entry[26:28] = struct.pack('<H', 4)
    f.write(efi_entry)
    
    # EFI dir (Cluster 4)
    f.seek(cluster_offset(4))
    dot = bytearray(32)
    dot[0:11] = b'.          '
    dot[11] = 0x10
    dot[26:28] = struct.pack('<H', 4)
    f.write(dot)
    dotdot = bytearray(32)
    dotdot[0:11] = b'..         '
    dotdot[11] = 0x10
    dotdot[26:28] = struct.pack('<H', 2)
    f.write(dotdot)
    boot_dir = bytearray(32)
    boot_dir[0:11] = b'BOOT       '
    boot_dir[11] = 0x10
    boot_dir[26:28] = struct.pack('<H', 5)
    f.write(boot_dir)
    
    # BOOT dir (Cluster 5)
    f.seek(cluster_offset(5))
    dot[26:28] = struct.pack('<H', 5)
    f.write(dot)
    dotdot[26:28] = struct.pack('<H', 4)
    f.write(dotdot)
    bootx64 = bytearray(32)
    bootx64[0:8] = b'BOOTX64 '
    bootx64[8:11] = b'EFI'
    bootx64[11] = 0x20
    bootx64[20:22] = struct.pack('<H', efi_start >> 16)
    bootx64[26:28] = struct.pack('<H', efi_start & 0xFFFF)
    bootx64[28:32] = struct.pack('<I', efi_size)
    f.write(bootx64)
    
    # EFI binary
    f.seek(cluster_offset(efi_start))
    f.write(efi_data)
    
    # Pad
    current = f.tell()
    target = total_sectors * SECTOR
    if current < target:
        f.write(bytes(target - current))

print(f'Created: {DISK}')

# Verify
with open(DISK, 'rb') as f:
    f.seek(root_pos)
    e = f.read(32)
    name = e[0:11].decode('ascii', errors='replace').rstrip()
    cluster = struct.unpack('<H', e[26:28])[0]
    print(f'Root[0]: "{name}" attr={e[11]} cluster={cluster}')
    
    f.seek(cluster_offset(4) + 64)
    e = f.read(32)
    name = e[0:11].decode('ascii', errors='replace').rstrip()
    cluster = struct.unpack('<H', e[26:28])[0]
    print(f'EFI/BOOT: "{name}" cluster={cluster}')
    
    f.seek(cluster_offset(5) + 64)
    e = f.read(32)
    name = e[0:11].decode('ascii', errors='replace').rstrip()
    size = struct.unpack('<I', e[28:32])[0]
    print(f'BOOTX64: "{name}" size={size}')
    
    f.seek(cluster_offset(efi_start))
    print(f'EFI header: {f.read(2).hex()}')
