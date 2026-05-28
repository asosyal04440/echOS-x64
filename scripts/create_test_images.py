import struct, os

# Create minimal FAT32 image (1MB)
img = bytearray(1024 * 1024)

# Boot sector
img[0] = 0xEB; img[1] = 0x58; img[2] = 0x90  # Jump boot
img[3:11] = b'MSDOS5.0'  # OEM
struct.pack_into('<H', img, 11, 512)   # bytes per sector
img[13] = 8                             # sectors per cluster
struct.pack_into('<H', img, 14, 32)    # reserved sectors
img[16] = 2                             # number of FATs
struct.pack_into('<H', img, 17, 0)      # root entry count
struct.pack_into('<H', img, 19, 0)      # total sectors 16
img[21] = 0xF8                          # media type
struct.pack_into('<H', img, 22, 0)      # sectors per FAT 16
struct.pack_into('<H', img, 24, 63)     # sectors per track
struct.pack_into('<H', img, 26, 255)    # number of heads
struct.pack_into('<I', img, 28, 0)      # hidden sectors
struct.pack_into('<I', img, 32, 2048)   # total sectors 32
struct.pack_into('<I', img, 36, 8)      # sectors per FAT 32
struct.pack_into('<I', img, 44, 2)      # root cluster
struct.pack_into('<H', img, 48, 1)      # FSInfo sector
struct.pack_into('<H', img, 50, 6)      # backup boot sector
img[64] = 0x80                          # drive number
img[66] = 0x29                          # boot signature
struct.pack_into('<I', img, 67, 0x12345678)  # volume ID
img[71:82] = b'NO NAME    '            # volume label
img[82:90] = b'FAT32   '              # file system type
img[510] = 0x55; img[511] = 0xAA      # boot signature

# FAT32 FSInfo sector (sector 1)
struct.pack_into('<I', img, 512, 0x41615252)  # lead signature
struct.pack_into('<I', img, 512+484, 0x61417272)  # structure signature
struct.pack_into('<I', img, 512+488, 2000)  # free cluster count
struct.pack_into('<I', img, 512+492, 3)    # next free cluster
struct.pack_into('<I', img, 512+508, 0xAA550000)  # trail signature

# FAT32 FAT entries
fat_offset = 32 * 512  # reserved sectors = 32
struct.pack_into('<I', img, fat_offset, 0x0FFFFFF8)      # Entry 0: media type
struct.pack_into('<I', img, fat_offset + 4, 0x0FFFFFFF)  # Entry 1: reserved
struct.pack_into('<I', img, fat_offset + 8, 0x0FFFFFFF)  # Entry 2: root cluster (EOF)

out_path = os.path.join(os.path.dirname(__file__), '..', 'build', 'test_fat32.img')
os.makedirs(os.path.dirname(out_path), exist_ok=True)
with open(out_path, 'wb') as f:
    f.write(img)
print(f'FAT32 image created: 1MB at {out_path}')
print(f'Boot sig: 0x{img[510]:02X}{img[511]:02X}')
print(f'FAT32 type: {img[82:90]}')
