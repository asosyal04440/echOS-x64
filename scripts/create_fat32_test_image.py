"""Create a FAT32 test image for QEMU FS certification."""
import struct, os, sys

def create_fat32_image(size_mb=2, label="ECHOS_TEST"):
    """Create a minimal valid FAT32 image with test data."""
    size = size_mb * 1024 * 1024
    img = bytearray(size)

    # BPB (BIOS Parameter Block)
    img[0] = 0xEB; img[1] = 0x58; img[2] = 0x90  # Jump boot
    img[3:11] = b'MSDOS5.0'
    struct.pack_into('<H', img, 11, 512)     # bytes per sector
    img[13] = 4                               # sectors per cluster (2KB clusters)
    struct.pack_into('<H', img, 14, 32)      # reserved sectors
    img[16] = 2                               # number of FATs
    struct.pack_into('<H', img, 17, 0)        # root entry count
    struct.pack_into('<H', img, 19, 0)        # total sectors 16
    img[21] = 0xF8                            # media type
    struct.pack_into('<H', img, 24, 63)       # sectors per track
    struct.pack_into('<H', img, 26, 255)      # number of heads
    struct.pack_into('<I', img, 28, 0)        # hidden sectors

    total_sectors = size // 512
    struct.pack_into('<I', img, 32, total_sectors)

    # Calculate FAT size
    # Each FAT entry = 4 bytes, each cluster = 4 sectors = 2048 bytes
    clusters = (total_sectors - 32) // 4  # rough estimate
    fat_entries = clusters + 2  # +2 for reserved entries
    fat_sectors = (fat_entries * 4 + 511) // 512
    struct.pack_into('<I', img, 36, fat_sectors)  # sectors per FAT 32

    struct.pack_into('<I', img, 44, 2)        # root cluster
    struct.pack_into('<H', img, 48, 1)        # FSInfo sector
    struct.pack_into('<H', img, 50, 6)        # backup boot sector

    img[64] = 0x80                            # drive number
    img[66] = 0x29                            # extended boot signature
    struct.pack_into('<I', img, 67, 0xDEADBEEF)  # volume ID
    label_bytes = label.encode('ascii').ljust(11)[:11]
    img[71:82] = label_bytes
    img[82:90] = b'FAT32   '

    # Boot signature
    img[510] = 0x55; img[511] = 0xAA

    # FSInfo sector (sector 1)
    struct.pack_into('<I', img, 512, 0x41615252)
    struct.pack_into('<I', img, 512+484, 0x61417272)
    free_clusters = clusters - 10  # reserve some
    struct.pack_into('<I', img, 512+488, free_clusters)
    struct.pack_into('<I', img, 512+492, 3)  # next free cluster
    struct.pack_into('<I', img, 512+508, 0xAA550000)

    # FAT table (at sector 32)
    fat_offset = 32 * 512
    # Entry 0: media type
    struct.pack_into('<I', img, fat_offset, 0x0FFFFFF8)
    # Entry 1: reserved
    struct.pack_into('<I', img, fat_offset + 4, 0x0FFFFFFF)
    # Entry 2: root directory (EOF)
    struct.pack_into('<I', img, fat_offset + 8, 0x0FFFFFFF)

    # Root directory (cluster 2)
    root_offset = (32 + fat_sectors * 2) * 512  # data_start
    # Initialize root directory with zeros (empty)

    return bytes(img)

if __name__ == "__main__":
    out_dir = os.path.join(os.path.dirname(__file__), "..", "build")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "test_fat32_qemu.img")

    img = create_fat32_image(size_mb=2, label="ECHOS_TEST")
    with open(out_path, 'wb') as f:
        f.write(img)

    print(f"FAT32 test image created: {len(img)} bytes at {out_path}")
    print(f"  Sectors: {len(img)//512}")
    print(f"  Boot sig: 0x{img[510]:02X}{img[511]:02X}")
    print(f"  FAT32 type: {img[82:90]}")
