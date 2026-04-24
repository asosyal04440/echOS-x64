#!/usr/bin/env python3
import argparse
import math
import struct
import zlib
from pathlib import Path

SECTOR_SIZE = 512
BLOCK_SIZE = 4096
MiB = 1024 * 1024

F2FS_MAGIC = 0xF2F52010
F2FS_SUPERBLOCK_SECTOR_OFFSET = 2
F2FS_SUPERBLOCK_SIZE = 4096
F2FS_SUM_BLKSIZE = 4096
SUMMARY_ENTRY_SIZE = 7

SUPER_MAGIC_OFFSET = 0
SUPER_LOG_SECTORSIZE_OFFSET = 8
SUPER_LOG_SECTORS_PER_BLOCK_OFFSET = 12
SUPER_LOG_BLOCKSIZE_OFFSET = 16
SUPER_LOG_BLOCKS_PER_SEG_OFFSET = 20
SUPER_SEGMENT_COUNT_SIT_OFFSET = 56
SUPER_SEGMENT_COUNT_NAT_OFFSET = 60
SUPER_SEGMENT_COUNT_SSA_OFFSET = 64
SUPER_SEGMENT_COUNT_MAIN_OFFSET = 68
SUPER_CP_BLKADDR_OFFSET = 76
SUPER_SIT_BLKADDR_OFFSET = 80
SUPER_NAT_BLKADDR_OFFSET = 84
SUPER_SSA_BLKADDR_OFFSET = 88
SUPER_MAIN_BLKADDR_OFFSET = 92
SUPER_ROOT_INO_OFFSET = 0x60
SUPER_CP_PAYLOAD_OFFSET = 0x680

CP_CHECKPOINT_VER_OFFSET = 0
CP_CKPT_FLAGS_OFFSET = 132
CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET = 136
CP_SIT_VER_BITMAP_BYTESIZE_OFFSET = 156
CP_NAT_VER_BITMAP_BYTESIZE_OFFSET = 160
CP_CHECKSUM_OFFSET_OFFSET = 164

INODE_I_MODE_OFFSET = 0
INODE_I_INLINE_OFFSET = 3
INODE_I_SIZE_OFFSET = 16
INODE_I_NLINK_OFFSET = 24
INODE_I_ADDR_OFFSET = 360
INODE_SIZE_OF_I_NID = 20
NODE_FOOTER_SIZE = 24

S_IFDIR = 0o040000
S_IFREG = 0o100000

DENTRY_BITMAP_SIZE = 27
DENTRY_RESERVED_SIZE = 3
DENTRY_ENTRY_SIZE = 11
DENTRY_SLOT_LEN = 8
DENTRY_SLOTS = 214
DENTRY_ENTRIES_OFFSET = DENTRY_BITMAP_SIZE + DENTRY_RESERVED_SIZE
DENTRY_FILENAME_OFFSET = DENTRY_ENTRIES_OFFSET + (DENTRY_ENTRY_SIZE * DENTRY_SLOTS)

NAT_ENTRY_SIZE = 9
SIT_VBLOCK_MAP_SIZE = 64
SIT_ENTRY_SIZE = 2 + SIT_VBLOCK_MAP_SIZE + 8
NODE_OFS_SENTINEL = 0xFFFF
SUMMARY_ENTRY_SENTINEL = 0xFFFF


def align_up(value: int, alignment: int) -> int:
    return ((value + alignment - 1) // alignment) * alignment


def write_u16(buf: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<H", buf, offset, value)


def write_u32(buf: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<I", buf, offset, value)


def write_u64(buf: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<Q", buf, offset, value)


class DirectoryState:
    def __init__(self, inode_nid: int, parent_nid: int, data_block: int) -> None:
        self.inode_nid = inode_nid
        self.parent_nid = parent_nid
        self.data_block = data_block
        self.entries: list[tuple[str, int, bool]] = [
            (".", inode_nid, True),
            ("..", parent_nid, True),
        ]


class F2fsSlotImageBuilder:
    def __init__(self, total_bytes: int) -> None:
        if total_bytes < 4 * MiB or total_bytes % BLOCK_SIZE != 0:
            raise ValueError("slot image must be >= 4 MiB and 4 KiB aligned")
        self.total_bytes = total_bytes
        self.total_blocks = total_bytes // BLOCK_SIZE
        self.blocks_per_seg = 32
        self.log_sectorsize = 9
        self.log_sectors_per_block = 3
        self.log_blocksize = 12
        self.log_blocks_per_seg = 5
        self.segment_count_sit = 1
        self.segment_count_nat = 1
        self.segment_count_ssa = 2
        self.cp_blkaddr = 1
        self.sit_blkaddr = self.cp_blkaddr + (2 * self.blocks_per_seg)
        self.nat_blkaddr = self.sit_blkaddr + (2 * self.segment_count_sit * self.blocks_per_seg)
        self.ssa_blkaddr = self.nat_blkaddr + (2 * self.segment_count_nat * self.blocks_per_seg)
        self.main_blkaddr = self.ssa_blkaddr + (self.segment_count_ssa * self.blocks_per_seg)
        main_blocks = self.total_blocks - self.main_blkaddr
        self.segment_count_main = max(1, main_blocks // self.blocks_per_seg)
        if self.segment_count_main < 8:
            raise ValueError("slot image too small for F2FS main area")
        if self.segment_count_ssa * self.blocks_per_seg < self.segment_count_main:
            raise ValueError("SSA area too small for main segment summaries")
        sit_capacity = self.segment_count_sit * self.blocks_per_seg * (BLOCK_SIZE // SIT_ENTRY_SIZE)
        if self.segment_count_main > sit_capacity:
            raise ValueError("SIT area cannot describe requested main segments")

        self.image = bytearray(self.total_blocks * BLOCK_SIZE)
        self.root_ino = 1
        self.next_nid = 2
        self.next_main_block = self.main_blkaddr
        self.nat_entries: dict[int, tuple[int, int]] = {}
        self.sit_entries: list[tuple[int, bytearray]] = [
            (0, bytearray(SIT_VBLOCK_MAP_SIZE)) for _ in range(self.segment_count_main)
        ]
        self.summary_blocks: list[bytearray] = [
            bytearray(BLOCK_SIZE) for _ in range(self.segment_count_main)
        ]
        self.inode_blocks: dict[int, int] = {}
        self.directories: dict[int, DirectoryState] = {}
        self.children: dict[int, dict[str, tuple[int, bool]]] = {}

        root_inode_block = self._allocate_main_block(self.root_ino, NODE_OFS_SENTINEL)
        root_dir_block = self._allocate_main_block(self.root_ino, 0)
        self.inode_blocks[self.root_ino] = root_inode_block
        root_dir = DirectoryState(self.root_ino, self.root_ino, root_dir_block)
        self.directories[self.root_ino] = root_dir
        self.children[self.root_ino] = {}
        self._rewrite_dir_block(root_dir)
        self._write_inode_block(self.root_ino, True, BLOCK_SIZE, [root_dir_block])

    def _allocate_nid(self) -> int:
        nid = self.next_nid
        self.next_nid += 1
        return nid

    def _allocate_main_block(self, nid: int, ofs_in_node: int) -> int:
        max_block = self.main_blkaddr + (self.segment_count_main * self.blocks_per_seg)
        if self.next_main_block >= max_block:
            raise ValueError("slot image exhausted main blocks")
        block_addr = self.next_main_block
        self.next_main_block += 1
        relative = block_addr - self.main_blkaddr
        segno = relative // self.blocks_per_seg
        offset = relative % self.blocks_per_seg
        vblocks, valid_map = self.sit_entries[segno]
        byte_index = offset // 8
        bit = 1 << (offset % 8)
        if valid_map[byte_index] & bit:
            raise ValueError("double allocation in SIT")
        valid_map[byte_index] |= bit
        self.sit_entries[segno] = (vblocks + 1, valid_map)
        self._write_summary(segno, offset, nid, ofs_in_node)
        return block_addr

    def _block_offset(self, block_addr: int) -> int:
        return block_addr * BLOCK_SIZE

    def _write_block(self, block_addr: int, data: bytes) -> None:
        if len(data) != BLOCK_SIZE:
            raise ValueError("F2FS blocks must be exactly 4 KiB")
        start = self._block_offset(block_addr)
        self.image[start : start + BLOCK_SIZE] = data

    def _write_summary(self, segno: int, offset: int, nid: int, ofs_in_node: int) -> None:
        block = self.summary_blocks[segno]
        entry_offset = offset * SUMMARY_ENTRY_SIZE
        if entry_offset + SUMMARY_ENTRY_SIZE > BLOCK_SIZE:
            raise ValueError("summary entry exceeds block")
        write_u32(block, entry_offset, nid)
        block[entry_offset + 4] = 0
        write_u16(block, entry_offset + 5, ofs_in_node)

    def _rewrite_dir_block(self, directory: DirectoryState) -> None:
        block = bytearray(BLOCK_SIZE)
        for slot, (name, ino, is_dir) in enumerate(directory.entries):
            name_bytes = name.encode("utf-8")
            slots_needed = math.ceil(len(name_bytes) / DENTRY_SLOT_LEN)
            byte_index = slot // 8
            bit = 1 << (slot % 8)
            block[byte_index] |= bit
            entry_offset = DENTRY_ENTRIES_OFFSET + (slot * DENTRY_ENTRY_SIZE)
            write_u32(block, entry_offset + 4, ino)
            write_u16(block, entry_offset + 8, len(name_bytes))
            block[entry_offset + 10] = 2 if is_dir else 1
            name_offset = DENTRY_FILENAME_OFFSET + (slot * DENTRY_SLOT_LEN)
            block[name_offset : name_offset + len(name_bytes)] = name_bytes
            for extra in range(1, slots_needed):
                extra_slot = slot + extra
                block[extra_slot // 8] |= 1 << (extra_slot % 8)
        self._write_block(directory.data_block, block)

    def _write_inode_block(
        self,
        nid: int,
        is_dir: bool,
        size: int,
        direct_blocks: list[int],
    ) -> None:
        block_addr = self.inode_blocks[nid]
        block = bytearray(BLOCK_SIZE)
        write_u16(block, INODE_I_MODE_OFFSET, (S_IFDIR if is_dir else S_IFREG) | (0o755 if is_dir else 0o644))
        block[INODE_I_INLINE_OFFSET] = 0
        write_u64(block, INODE_I_SIZE_OFFSET, size)
        write_u32(block, INODE_I_NLINK_OFFSET, 2 if is_dir else 1)
        for index, direct in enumerate(direct_blocks):
            write_u32(block, INODE_I_ADDR_OFFSET + index * 4, direct)
        self._write_block(block_addr, block)
        self.nat_entries[nid] = (1, block_addr)

    def _create_dir(self, parent_nid: int, name: str) -> int:
        nid = self._allocate_nid()
        inode_block = self._allocate_main_block(nid, NODE_OFS_SENTINEL)
        data_block = self._allocate_main_block(nid, 0)
        self.inode_blocks[nid] = inode_block
        directory = DirectoryState(nid, parent_nid, data_block)
        self.directories[nid] = directory
        self.children[nid] = {}
        self._rewrite_dir_block(directory)
        self._write_inode_block(nid, True, BLOCK_SIZE, [data_block])
        parent_dir = self.directories[parent_nid]
        parent_dir.entries.append((name, nid, True))
        self.children[parent_nid][name] = (nid, True)
        self._rewrite_dir_block(parent_dir)
        return nid

    def ensure_dir(self, path: str) -> int:
        if path == "/":
            return self.root_ino
        current = self.root_ino
        for component in [entry for entry in path.split("/") if entry]:
            child = self.children[current].get(component)
            if child is None:
                current = self._create_dir(current, component)
                continue
            current, is_dir = child
            if not is_dir:
                raise ValueError(f"path component '{component}' is not a directory")
        return current

    def create_file(self, path: str, payload: bytes) -> None:
        normalized = path if path.startswith("/") else f"/{path}"
        parts = [entry for entry in normalized.split("/") if entry]
        if not parts:
            raise ValueError("file path cannot be root")
        parent_path = "/" + "/".join(parts[:-1]) if len(parts) > 1 else "/"
        name = parts[-1]
        parent_nid = self.ensure_dir(parent_path)
        if name in self.children[parent_nid]:
            raise ValueError(f"duplicate file path '{normalized}'")
        nid = self._allocate_nid()
        inode_block = self._allocate_main_block(nid, NODE_OFS_SENTINEL)
        self.inode_blocks[nid] = inode_block
        direct_blocks = []
        if payload:
            for index in range(0, len(payload), BLOCK_SIZE):
                chunk = payload[index : index + BLOCK_SIZE]
                block_addr = self._allocate_main_block(nid, len(direct_blocks))
                block = bytearray(BLOCK_SIZE)
                block[: len(chunk)] = chunk
                self._write_block(block_addr, block)
                direct_blocks.append(block_addr)
        self._write_inode_block(nid, False, len(payload), direct_blocks)
        parent_dir = self.directories[parent_nid]
        parent_dir.entries.append((name, nid, False))
        self.children[parent_nid][name] = (nid, False)
        self._rewrite_dir_block(parent_dir)

    def _write_superblock(self) -> None:
        block = bytearray(F2FS_SUPERBLOCK_SIZE)
        write_u32(block, SUPER_MAGIC_OFFSET, F2FS_MAGIC)
        write_u32(block, SUPER_LOG_SECTORSIZE_OFFSET, self.log_sectorsize)
        write_u32(block, SUPER_LOG_SECTORS_PER_BLOCK_OFFSET, self.log_sectors_per_block)
        write_u32(block, SUPER_LOG_BLOCKSIZE_OFFSET, self.log_blocksize)
        write_u32(block, SUPER_LOG_BLOCKS_PER_SEG_OFFSET, self.log_blocks_per_seg)
        write_u32(block, SUPER_SEGMENT_COUNT_SIT_OFFSET, self.segment_count_sit)
        write_u32(block, SUPER_SEGMENT_COUNT_NAT_OFFSET, self.segment_count_nat)
        write_u32(block, SUPER_SEGMENT_COUNT_SSA_OFFSET, self.segment_count_ssa)
        write_u32(block, SUPER_SEGMENT_COUNT_MAIN_OFFSET, self.segment_count_main)
        write_u32(block, SUPER_CP_BLKADDR_OFFSET, self.cp_blkaddr)
        write_u32(block, SUPER_SIT_BLKADDR_OFFSET, self.sit_blkaddr)
        write_u32(block, SUPER_NAT_BLKADDR_OFFSET, self.nat_blkaddr)
        write_u32(block, SUPER_SSA_BLKADDR_OFFSET, self.ssa_blkaddr)
        write_u32(block, SUPER_MAIN_BLKADDR_OFFSET, self.main_blkaddr)
        write_u32(block, SUPER_ROOT_INO_OFFSET, self.root_ino)
        write_u32(block, SUPER_CP_PAYLOAD_OFFSET, 0)
        sector_offset = F2FS_SUPERBLOCK_SECTOR_OFFSET * SECTOR_SIZE
        self.image[sector_offset : sector_offset + len(block)] = block

    def _write_checkpoint_block(self, block_addr: int, version: int) -> None:
        block = bytearray(BLOCK_SIZE)
        write_u64(block, CP_CHECKPOINT_VER_OFFSET, version)
        write_u32(block, CP_CKPT_FLAGS_OFFSET, 0x00000005)
        write_u32(block, CP_CP_PACK_TOTAL_BLOCK_COUNT_OFFSET, 1)
        write_u32(block, CP_SIT_VER_BITMAP_BYTESIZE_OFFSET, 0)
        write_u32(block, CP_NAT_VER_BITMAP_BYTESIZE_OFFSET, 0)
        checksum_offset = BLOCK_SIZE - 4
        write_u32(block, CP_CHECKSUM_OFFSET_OFFSET, checksum_offset)
        write_u32(block, checksum_offset, zlib.crc32(block[:checksum_offset]) & 0xFFFFFFFF)
        self._write_block(block_addr, block)

    def _write_nat_tables(self) -> None:
        primary_blocks = self.segment_count_nat * self.blocks_per_seg
        nat_region_blocks = primary_blocks * 2
        for block_index in range(nat_region_blocks):
            self._write_block(self.nat_blkaddr + block_index, bytes(BLOCK_SIZE))
        entries_per_block = BLOCK_SIZE // NAT_ENTRY_SIZE
        for nid, (version, block_addr) in self.nat_entries.items():
            table_index = nid // entries_per_block
            entry_index = nid % entries_per_block
            for base in (self.nat_blkaddr, self.nat_blkaddr + primary_blocks):
                block_addr_abs = base + table_index
                start = self._block_offset(block_addr_abs)
                entry_offset = start + entry_index * NAT_ENTRY_SIZE
                self.image[entry_offset] = version & 0xFF
                struct.pack_into("<I", self.image, entry_offset + 1, nid)
                struct.pack_into("<I", self.image, entry_offset + 5, block_addr)

    def _write_sit_tables(self) -> None:
        sit_copy_blocks = self.segment_count_sit * self.blocks_per_seg
        total_blocks = sit_copy_blocks * 2
        for block_index in range(total_blocks):
            self._write_block(self.sit_blkaddr + block_index, bytes(BLOCK_SIZE))
        entries_per_block = BLOCK_SIZE // SIT_ENTRY_SIZE
        for segno, (vblocks, valid_map) in enumerate(self.sit_entries):
            table_index = segno // entries_per_block
            entry_index = segno % entries_per_block
            for base in (self.sit_blkaddr, self.sit_blkaddr + sit_copy_blocks):
                block_addr = base + table_index
                start = self._block_offset(block_addr) + entry_index * SIT_ENTRY_SIZE
                struct.pack_into("<H", self.image, start, vblocks)
                self.image[start + 2 : start + 2 + SIT_VBLOCK_MAP_SIZE] = valid_map

    def _write_summary_area(self) -> None:
        total_blocks = self.segment_count_ssa * self.blocks_per_seg
        for block_index in range(total_blocks):
            block_addr = self.ssa_blkaddr + block_index
            if block_index < len(self.summary_blocks):
                self._write_block(block_addr, self.summary_blocks[block_index])
            else:
                self._write_block(block_addr, bytes(BLOCK_SIZE))

    def build(self) -> bytes:
        self._write_superblock()
        self._write_checkpoint_block(self.cp_blkaddr, 1)
        self._write_checkpoint_block(self.cp_blkaddr + self.blocks_per_seg, 2)
        self._write_nat_tables()
        self._write_sit_tables()
        self._write_summary_area()
        return bytes(self.image)


def build_system_slot_image(total_bytes: int, request_url: str | None = None) -> bytes:
    builder = F2fsSlotImageBuilder(total_bytes)
    if request_url:
        builder.create_file("/config/update/smoke/request.txt", request_url.encode("utf-8") + b"\n")
    return builder.build()


def main() -> None:
    parser = argparse.ArgumentParser(description="Build minimal echOS F2FS slot image")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--image-mib", type=int, default=8)
    parser.add_argument("--request-url")
    args = parser.parse_args()
    image = build_system_slot_image(args.image_mib * MiB, args.request_url)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(image)
    print(args.output)


if __name__ == "__main__":
    main()
