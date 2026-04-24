#!/usr/bin/env python3
import argparse
import json
import math
import struct
import uuid
import zlib
from pathlib import Path

from build_f2fs_slot_image import MiB as SLOT_IMAGE_MIB, build_system_slot_image

SECTOR_SIZE = 512
MiB = 1024 * 1024

ESP_TYPE_GUID = uuid.UUID("c12a7328-f81f-11d2-ba4b-00a0c93ec93b")
LINUX_FS_GUID = uuid.UUID("0fc63daf-8483-4772-8e79-3d69d8477de4")

BOOT_CONTROL_MAGIC = struct.unpack("<I", b"ECBC")[0]
BOOT_CONTROL_VERSION = 1
BOOT_CONTROL_SIZE = 136
BOOT_FLAG_AUTO_LOGIN = 1 << 0
BOOT_FLAG_SUSPEND_RESUME_SMOKE = 1 << 1

SLOT_IDS = {
    "none": 0,
    "system_a": 1,
    "system_b": 2,
    "recovery": 3,
}


def align_up(value: int, alignment: int) -> int:
    return ((value + alignment - 1) // alignment) * alignment


def utf16le_name(name: str) -> bytes:
    raw = name.encode("utf-16le")
    return raw[:72].ljust(72, b"\x00")


def dir_entry(name_83: bytes, attr: int, first_cluster: int, size: int) -> bytes:
    return struct.pack(
        "<11sBBBHHHHHHHI",
        name_83,
        attr,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        first_cluster & 0xFFFF,
        size,
    )


def short_name(name: str) -> bytes:
    base, _, ext = name.partition(".")
    base = "".join(ch for ch in base.upper() if ch.isalnum())[:8].ljust(8)
    ext = "".join(ch for ch in ext.upper() if ch.isalnum())[:3].ljust(3)
    return (base + ext).encode("ascii")


def is_fat83_name(name: str) -> bool:
    base, dot, ext = name.partition(".")
    if not base or len(base) > 8:
        return False
    if dot and (not ext or len(ext) > 3):
        return False
    allowed = set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_")
    return all(ch in allowed for ch in base) and all(ch in allowed for ch in ext)


class FatNode:
    def __init__(self, name: str, directory: bool, data: bytes = b""):
        self.name = name
        self.directory = directory
        self.data = data
        self.children = []
        self.first_cluster = 0
        self.size = len(data)


def build_fat16_image(
    total_bytes: int,
    hidden_sectors: int,
    efi_bytes: bytes,
    bootctrl_bytes: bytes,
    pe_smoke_bundle: bytes | None,
    curated_bundles: list[bytes],
    esp_extra_files: list[tuple[str, bytes]],
) -> bytes:
    total_sectors = total_bytes // SECTOR_SIZE
    reserved_sectors = 1
    root_entries = 512
    root_dir_sectors = (root_entries * 32 + (SECTOR_SIZE - 1)) // SECTOR_SIZE
    fat_count = 2

    sectors_per_cluster = 4
    while True:
        fat_sectors = 1
        while True:
            data_sectors = total_sectors - reserved_sectors - fat_count * fat_sectors - root_dir_sectors
            cluster_count = data_sectors // sectors_per_cluster
            next_fat_sectors = math.ceil(((cluster_count + 2) * 2) / SECTOR_SIZE)
            if next_fat_sectors == fat_sectors:
                break
            fat_sectors = next_fat_sectors
        if cluster_count <= 0xFFF5:
            break
        sectors_per_cluster *= 2
        if sectors_per_cluster > 128:
            raise RuntimeError("ESP FAT16 geometry exceeds supported cluster size")

    cluster_bytes = sectors_per_cluster * SECTOR_SIZE
    root = FatNode("ROOT", True)
    efi = FatNode("EFI", True)
    boot = FatNode("BOOT", True)
    boot.children.append(FatNode("BOOTX64.EFI", False, efi_bytes))
    boot.children.append(FatNode("BOOTCTRL.BIN", False, bootctrl_bytes))
    if pe_smoke_bundle:
        boot.children.append(FatNode("PESMOKE.BHD", False, pe_smoke_bundle))
    for index, bundle in enumerate(curated_bundles, start=1):
        boot.children.append(FatNode(f"APP{index:04d}.BHD", False, bundle))
    for guest_name, payload in esp_extra_files:
        boot.children.append(FatNode(guest_name, False, payload))
    boot.children.append(
        FatNode(
            "APPLINFO.TXT",
            False,
            b"echOS appliance image\nboot target: EFI/BOOT/BOOTX64.EFI\n",
        )
    )
    efi.children.append(boot)
    root.children.append(efi)

    nodes = []

    def collect(node: FatNode) -> None:
        for child in node.children:
            nodes.append(child)
            if child.directory:
                collect(child)

    collect(root)

    fat_entries = [0x0000] * (cluster_count + 2)
    fat_entries[0] = 0xFFF8
    fat_entries[1] = 0xFFFF
    next_cluster = 2

    def allocate(node: FatNode) -> None:
        nonlocal next_cluster
        if node.directory:
            needed_clusters = 1
        else:
            needed_clusters = max(1, math.ceil(len(node.data) / cluster_bytes))
        node.first_cluster = next_cluster
        for idx in range(needed_clusters):
            cluster = next_cluster + idx
            fat_entries[cluster] = 0xFFFF if idx == needed_clusters - 1 else cluster + 1
        next_cluster += needed_clusters

    for node in nodes:
        allocate(node)

    if next_cluster > len(fat_entries):
        raise RuntimeError("ESP FAT16 cluster budget exhausted")

    boot_sector = bytearray(SECTOR_SIZE)
    boot_sector[0:3] = b"\xEB\x3C\x90"
    boot_sector[3:11] = b"ECHOSF16"
    struct.pack_into("<H", boot_sector, 11, SECTOR_SIZE)
    boot_sector[13] = sectors_per_cluster
    struct.pack_into("<H", boot_sector, 14, reserved_sectors)
    boot_sector[16] = fat_count
    struct.pack_into("<H", boot_sector, 17, root_entries)
    struct.pack_into("<H", boot_sector, 19, 0 if total_sectors >= 0x10000 else total_sectors)
    boot_sector[21] = 0xF8
    struct.pack_into("<H", boot_sector, 22, fat_sectors)
    struct.pack_into("<H", boot_sector, 24, 32)
    struct.pack_into("<H", boot_sector, 26, 64)
    struct.pack_into("<I", boot_sector, 28, hidden_sectors)
    struct.pack_into("<I", boot_sector, 32, total_sectors if total_sectors >= 0x10000 else 0)
    boot_sector[36] = 0x80
    boot_sector[38] = 0x29
    struct.pack_into("<I", boot_sector, 39, 0xEC71A11E)
    boot_sector[43:54] = b"ECHOS_ESP  "
    boot_sector[54:62] = b"FAT16   "
    boot_sector[510:512] = b"\x55\xAA"

    fat = bytearray(fat_sectors * SECTOR_SIZE)
    for index, entry in enumerate(fat_entries[: (fat_sectors * SECTOR_SIZE) // 2]):
        struct.pack_into("<H", fat, index * 2, entry)

    root_dir = bytearray(root_dir_sectors * SECTOR_SIZE)
    root_offset = 0
    for child in root.children:
        root_dir[root_offset : root_offset + 32] = dir_entry(
            short_name(child.name), 0x10 if child.directory else 0x20, child.first_cluster, child.size
        )
        root_offset += 32

    data = bytearray(data_sectors * SECTOR_SIZE)

    def write_cluster(cluster: int, payload: bytes) -> None:
        start = (cluster - 2) * cluster_bytes
        data[start : start + len(payload)] = payload

    def build_directory(node: FatNode, parent_cluster: int) -> bytes:
        directory = bytearray(cluster_bytes)
        offset = 0
        directory[offset : offset + 32] = dir_entry(b".          ", 0x10, node.first_cluster, 0)
        offset += 32
        directory[offset : offset + 32] = dir_entry(b"..         ", 0x10, parent_cluster, 0)
        offset += 32
        for child in node.children:
            directory[offset : offset + 32] = dir_entry(
                short_name(child.name),
                0x10 if child.directory else 0x20,
                child.first_cluster,
                child.size,
            )
            offset += 32
        return bytes(directory)

    for child in root.children:
        write_cluster(child.first_cluster, build_directory(child, 0))
        for grand_child in child.children:
            if grand_child.directory:
                write_cluster(grand_child.first_cluster, build_directory(grand_child, child.first_cluster))
                for file_node in grand_child.children:
                    if not file_node.directory:
                        remaining = file_node.data
                        cluster = file_node.first_cluster
                        while remaining:
                            chunk = remaining[:cluster_bytes]
                            write_cluster(cluster, chunk)
                            remaining = remaining[cluster_bytes:]
                            cluster = fat_entries[cluster]
                            if cluster == 0xFFFF:
                                break
            else:
                write_cluster(grand_child.first_cluster, grand_child.data)

    image = bytearray(total_bytes)
    cursor = 0
    image[cursor : cursor + SECTOR_SIZE] = boot_sector
    cursor += SECTOR_SIZE
    image[cursor : cursor + len(fat)] = fat
    cursor += len(fat)
    image[cursor : cursor + len(fat)] = fat
    cursor += len(fat)
    image[cursor : cursor + len(root_dir)] = root_dir
    cursor += len(root_dir)
    image[cursor : cursor + len(data)] = data
    return bytes(image)


def build_seed_loop_image(curated_bundles: list[bytes]) -> bytes:
    header = bytearray()
    header.extend(b"echSID01")
    header.extend(struct.pack("<H", 1))
    header.extend(struct.pack("<H", len(curated_bundles)))
    header.extend(struct.pack("<I", 0))

    records = bytearray()
    payload = bytearray()
    payload_offset = 16
    payload_offset += sum(20 + len(f"bundle-{index + 1}".encode("utf-8")) for index in range(len(curated_bundles)))
    for index, bundle in enumerate(curated_bundles, start=1):
        identity = f"bundle-{index}".encode("utf-8")
        records.extend(struct.pack("<Q", payload_offset))
        records.extend(struct.pack("<Q", len(bundle)))
        records.extend(struct.pack("<H", len(identity)))
        records.extend(struct.pack("<H", 0))
        records.extend(identity)
        payload.extend(bundle)
        payload_offset += len(bundle)
    return bytes(header + records + payload)


def build_seed_fat16_image(
    total_bytes: int,
    hidden_sectors: int,
    curated_bundles: list[bytes],
) -> bytes:
    seed_loop_image = build_seed_loop_image(curated_bundles)
    total_sectors = total_bytes // SECTOR_SIZE
    reserved_sectors = 1
    root_entries = 512
    root_dir_sectors = (root_entries * 32 + (SECTOR_SIZE - 1)) // SECTOR_SIZE
    fat_count = 2

    sectors_per_cluster = 4
    while True:
        fat_sectors = 1
        while True:
            data_sectors = total_sectors - reserved_sectors - fat_count * fat_sectors - root_dir_sectors
            cluster_count = data_sectors // sectors_per_cluster
            next_fat_sectors = math.ceil(((cluster_count + 2) * 2) / SECTOR_SIZE)
            if next_fat_sectors == fat_sectors:
                break
            fat_sectors = next_fat_sectors
        if cluster_count <= 0xFFF5:
            break
        sectors_per_cluster *= 2
        if sectors_per_cluster > 128:
            raise RuntimeError("seed FAT16 geometry exceeds supported cluster size")

    cluster_bytes = sectors_per_cluster * SECTOR_SIZE
    root = FatNode("ROOT", True)
    apps = FatNode("APPS", True)
    for index, bundle in enumerate(curated_bundles, start=1):
        apps.children.append(FatNode(f"APP{index:04d}.BHD", False, bundle))
    root.children.append(apps)
    root.children.append(FatNode("APPS.IMG", False, seed_loop_image))
    root.children.append(
        FatNode(
            "SEEDINFO.TXT",
            False,
            b"echOS seed partition\napps under /APPS\nloop image APPS.IMG\n",
        )
    )

    nodes = []

    def collect(node: FatNode) -> None:
        for child in node.children:
            nodes.append(child)
            if child.directory:
                collect(child)

    collect(root)

    fat_entries = [0x0000] * (cluster_count + 2)
    fat_entries[0] = 0xFFF8
    fat_entries[1] = 0xFFFF
    next_cluster = 2

    def allocate(node: FatNode) -> None:
        nonlocal next_cluster
        if node.directory:
            needed_clusters = 1
        else:
            needed_clusters = max(1, math.ceil(len(node.data) / cluster_bytes))
        node.first_cluster = next_cluster
        for idx in range(needed_clusters):
            cluster = next_cluster + idx
            fat_entries[cluster] = 0xFFFF if idx == needed_clusters - 1 else cluster + 1
        next_cluster += needed_clusters

    for node in nodes:
        allocate(node)

    if next_cluster > len(fat_entries):
        raise RuntimeError("seed FAT16 cluster budget exhausted")

    boot_sector = bytearray(SECTOR_SIZE)
    boot_sector[0:3] = b"\xEB\x3C\x90"
    boot_sector[3:11] = b"ECHOSSED"
    struct.pack_into("<H", boot_sector, 11, SECTOR_SIZE)
    boot_sector[13] = sectors_per_cluster
    struct.pack_into("<H", boot_sector, 14, reserved_sectors)
    boot_sector[16] = fat_count
    struct.pack_into("<H", boot_sector, 17, root_entries)
    struct.pack_into("<H", boot_sector, 19, 0 if total_sectors >= 0x10000 else total_sectors)
    boot_sector[21] = 0xF8
    struct.pack_into("<H", boot_sector, 22, fat_sectors)
    struct.pack_into("<H", boot_sector, 24, 32)
    struct.pack_into("<H", boot_sector, 26, 64)
    struct.pack_into("<I", boot_sector, 28, hidden_sectors)
    struct.pack_into("<I", boot_sector, 32, total_sectors if total_sectors >= 0x10000 else 0)
    boot_sector[36] = 0x80
    boot_sector[38] = 0x29
    struct.pack_into("<I", boot_sector, 39, 0xEC71A55E)
    boot_sector[43:54] = b"ECHOS_SEED "
    boot_sector[54:62] = b"FAT16   "
    boot_sector[510:512] = b"\x55\xAA"

    fat = bytearray(fat_sectors * SECTOR_SIZE)
    for index, entry in enumerate(fat_entries[: (fat_sectors * SECTOR_SIZE) // 2]):
        struct.pack_into("<H", fat, index * 2, entry)

    root_dir = bytearray(root_dir_sectors * SECTOR_SIZE)
    root_offset = 0
    for child in root.children:
        root_dir[root_offset : root_offset + 32] = dir_entry(
            short_name(child.name), 0x10 if child.directory else 0x20, child.first_cluster, child.size
        )
        root_offset += 32

    data = bytearray(data_sectors * SECTOR_SIZE)

    def write_cluster(cluster: int, payload: bytes) -> None:
        start = (cluster - 2) * cluster_bytes
        data[start : start + len(payload)] = payload

    def build_directory(node: FatNode, parent_cluster: int) -> bytes:
        directory = bytearray(cluster_bytes)
        offset = 0
        directory[offset : offset + 32] = dir_entry(b".          ", 0x10, node.first_cluster, 0)
        offset += 32
        directory[offset : offset + 32] = dir_entry(b"..         ", 0x10, parent_cluster, 0)
        offset += 32
        for child in node.children:
            directory[offset : offset + 32] = dir_entry(
                short_name(child.name),
                0x10 if child.directory else 0x20,
                child.first_cluster,
                child.size,
            )
            offset += 32
        return bytes(directory)

    for child in root.children:
        if child.directory:
            write_cluster(child.first_cluster, build_directory(child, 0))
            for file_node in child.children:
                remaining = file_node.data
                cluster = file_node.first_cluster
                while remaining:
                    chunk = remaining[:cluster_bytes]
                    write_cluster(cluster, chunk)
                    remaining = remaining[cluster_bytes:]
                    cluster = fat_entries[cluster]
                    if cluster == 0xFFFF:
                        break
        else:
            remaining = child.data
            cluster = child.first_cluster
            while remaining:
                chunk = remaining[:cluster_bytes]
                write_cluster(cluster, chunk)
                remaining = remaining[cluster_bytes:]
                cluster = fat_entries[cluster]
                if cluster == 0xFFFF:
                    break

    image = bytearray(total_bytes)
    cursor = 0
    image[cursor : cursor + SECTOR_SIZE] = boot_sector
    cursor += SECTOR_SIZE
    image[cursor : cursor + len(fat)] = fat
    cursor += len(fat)
    image[cursor : cursor + len(fat)] = fat
    cursor += len(fat)
    image[cursor : cursor + len(root_dir)] = root_dir
    cursor += len(root_dir)
    image[cursor : cursor + len(data)] = data
    return bytes(image)


def build_partition_table(total_sectors: int, partitions: list[dict]) -> tuple[bytes, bytes, bytes]:
    primary_entries = bytearray(128 * 128)
    backup_entries = bytearray(128 * 128)

    for index, part in enumerate(partitions):
        entry = struct.pack(
            "<16s16sQQQ72s",
            part["type_guid"].bytes_le,
            part["unique_guid"].bytes_le,
            part["first_lba"],
            part["last_lba"],
            0,
            utf16le_name(part["name"]),
        )
        offset = index * 128
        primary_entries[offset : offset + 128] = entry
        backup_entries[offset : offset + 128] = entry

    primary_entries_crc = zlib.crc32(primary_entries) & 0xFFFFFFFF
    backup_entries_crc = zlib.crc32(backup_entries) & 0xFFFFFFFF

    def header(current_lba: int, backup_lba: int, entries_lba: int, entries_crc: int) -> bytes:
        header = bytearray(SECTOR_SIZE)
        header[0:8] = b"EFI PART"
        struct.pack_into("<I", header, 8, 0x00010000)
        struct.pack_into("<I", header, 12, 92)
        struct.pack_into("<Q", header, 24, current_lba)
        struct.pack_into("<Q", header, 32, backup_lba)
        struct.pack_into("<Q", header, 40, 34)
        struct.pack_into("<Q", header, 48, total_sectors - 34)
        struct.pack_into("<16s", header, 56, uuid.uuid4().bytes_le)
        struct.pack_into("<Q", header, 72, entries_lba)
        struct.pack_into("<I", header, 80, 128)
        struct.pack_into("<I", header, 84, 128)
        struct.pack_into("<I", header, 88, entries_crc)
        struct.pack_into("<I", header, 16, 0)
        crc = zlib.crc32(header[:92]) & 0xFFFFFFFF
        struct.pack_into("<I", header, 16, crc)
        return bytes(header)

    primary_header = header(1, total_sectors - 1, 2, primary_entries_crc)
    backup_entries_lba = total_sectors - 33
    backup_header = header(total_sectors - 1, 1, backup_entries_lba, backup_entries_crc)

    protective_mbr = bytearray(SECTOR_SIZE)
    protective_mbr[446:462] = struct.pack(
        "<B3sB3sII",
        0,
        b"\x00\x02\x00",
        0xEE,
        b"\xFF\xFF\xFF",
        1,
        min(total_sectors - 1, 0xFFFFFFFF),
    )
    protective_mbr[510:512] = b"\x55\xAA"
    return bytes(protective_mbr), primary_header + bytes(primary_entries), bytes(backup_entries) + backup_header


def create_layout(disk_bytes: int, esp_bytes: int) -> list[dict]:
    disk_sectors = disk_bytes // SECTOR_SIZE
    sizes = {
        "esp": max(64 * MiB, align_up(esp_bytes, MiB)),
        "seed": 64 * MiB,
        "system_a": 96 * MiB,
        "system_b": 96 * MiB,
        "data": 192 * MiB,
    }
    start_lba = 2048
    layout = []
    for name in ["esp", "seed", "system_a", "system_b", "data"]:
        sectors = sizes[name] // SECTOR_SIZE
        first = start_lba
        last = first + sectors - 1
        layout.append(
            {
                "name": name,
                "first_lba": first,
                "last_lba": last,
                "type_guid": ESP_TYPE_GUID if name == "esp" else LINUX_FS_GUID,
                "unique_guid": uuid.uuid4(),
            }
        )
        start_lba = align_up(last + 1, 2048)

    recovery_last = disk_sectors - 34
    layout.append(
        {
            "name": "recovery",
            "first_lba": start_lba,
            "last_lba": recovery_last,
            "type_guid": LINUX_FS_GUID,
            "unique_guid": uuid.uuid4(),
        }
    )
    if layout[-1]["first_lba"] > layout[-1]["last_lba"]:
        raise RuntimeError("disk image too small for appliance layout")
    return layout


def build_boot_control(
    active_slot: str,
    pending_slot: str,
    auto_login: bool,
    suspend_resume_smoke: bool,
) -> bytes:
    active = SLOT_IDS[active_slot]
    pending = SLOT_IDS[pending_slot]
    blob = bytearray(BOOT_CONTROL_SIZE)
    struct.pack_into("<I", blob, 0, BOOT_CONTROL_MAGIC)
    struct.pack_into("<H", blob, 4, BOOT_CONTROL_VERSION)
    struct.pack_into("<H", blob, 6, BOOT_CONTROL_SIZE)
    blob[8] = active
    blob[9] = pending
    blob[10] = 3
    blob[11] = 0 if pending != 0 else 1
    blob[12] = 1
    blob[13] = 0
    struct.pack_into("<I", blob, 16, 0)
    struct.pack_into("<Q", blob, 24, 1)
    struct.pack_into("<Q", blob, 32, 0)
    struct.pack_into("<Q", blob, 40, 0)
    boot_flags = 0
    if auto_login:
        boot_flags |= BOOT_FLAG_AUTO_LOGIN
    if suspend_resume_smoke:
        boot_flags |= BOOT_FLAG_SUSPEND_RESUME_SMOKE
    blob[48] = boot_flags
    struct.pack_into("<I", blob, 128, 0)
    crc = zlib.crc32(blob) & 0xFFFFFFFF
    struct.pack_into("<I", blob, 128, crc)
    return bytes(blob)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build raw GPT appliance image for echOS")
    parser.add_argument("--efi", required=True, type=Path)
    parser.add_argument("--bootctrl", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--disk-mib", type=int, default=512)
    parser.add_argument("--system-image-mib", type=int, default=8)
    parser.add_argument("--active-slot", choices=SLOT_IDS.keys(), default="system_a")
    parser.add_argument("--pending-slot", choices=SLOT_IDS.keys(), default="none")
    parser.add_argument("--auto-login", action="store_true")
    parser.add_argument("--suspend-resume-smoke", action="store_true")
    parser.add_argument("--update-smoke-request-url")
    parser.add_argument("--pe-smoke-bundle", type=Path)
    parser.add_argument("--bundle", action="append", type=Path, default=[])
    parser.add_argument("--esp-extra-file", action="append", default=[])
    args = parser.parse_args()

    efi_bytes = args.efi.read_bytes()
    if args.bootctrl:
        bootctrl_bytes = args.bootctrl.read_bytes()
    else:
        bootctrl_bytes = build_boot_control(
            args.active_slot,
            args.pending_slot,
            args.auto_login,
            args.suspend_resume_smoke,
        )
    pe_smoke_bundle = args.pe_smoke_bundle.read_bytes() if args.pe_smoke_bundle else None
    curated_bundles = [path.read_bytes() for path in args.bundle]
    esp_extra_files: list[tuple[str, bytes]] = []
    esp_extra_manifest: list[dict[str, str]] = []
    for spec in args.esp_extra_file:
        host_text, separator, guest_name = spec.partition("::")
        host_path = Path(host_text)
        if separator == "":
            guest_name = host_path.name
        if not host_path.is_file():
            raise RuntimeError(f"ESP extra file not found: {host_path}")
        if not is_fat83_name(guest_name):
            raise RuntimeError(
                f"ESP extra guest name must be FAT 8.3 compatible: {guest_name}"
            )
        esp_extra_files.append((guest_name, host_path.read_bytes()))
        esp_extra_manifest.append({"host_path": str(host_path), "guest_name": guest_name})
    requested_disk_bytes = args.disk_mib * MiB
    seed_loop_bytes = build_seed_loop_image(curated_bundles)
    esp_required_bytes = (
        len(efi_bytes)
        + len(bootctrl_bytes)
        + sum(len(bundle) for bundle in curated_bundles)
        + (len(pe_smoke_bundle) if pe_smoke_bundle else 0)
        + sum(len(payload) for _, payload in esp_extra_files)
        + 16 * MiB
    )
    seed_required_bytes = len(seed_loop_bytes) + sum(len(bundle) for bundle in curated_bundles) + 16 * MiB
    minimum_disk_bytes = (
        max(64 * MiB, align_up(esp_required_bytes, MiB))
        + max(64 * MiB, align_up(seed_required_bytes, MiB))
        + 96 * MiB
        + 96 * MiB
        + 192 * MiB
        + 128 * MiB
    )
    disk_bytes = max(requested_disk_bytes, align_up(minimum_disk_bytes, MiB))
    disk_sectors = disk_bytes // SECTOR_SIZE
    layout = create_layout(disk_bytes, esp_required_bytes)
    for part in layout:
        if part["name"] == "seed":
            part_sectors = max(64 * MiB, align_up(seed_required_bytes, MiB)) // SECTOR_SIZE
            part["last_lba"] = part["first_lba"] + part_sectors - 1
            break
    for index in range(1, len(layout)):
        prev = layout[index - 1]
        current = layout[index]
        if current["first_lba"] <= prev["last_lba"]:
            current_size = current["last_lba"] - current["first_lba"] + 1
            current["first_lba"] = align_up(prev["last_lba"] + 1, 2048)
            current["last_lba"] = current["first_lba"] + current_size - 1
    recovery = next(part for part in layout if part["name"] == "recovery")
    recovery["last_lba"] = disk_sectors - 34
    if recovery["first_lba"] > recovery["last_lba"]:
        raise RuntimeError("disk image too small after seed partition sizing")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as image:
        image.truncate(disk_bytes)

    esp = next(part for part in layout if part["name"] == "esp")
    esp_bytes = (esp["last_lba"] - esp["first_lba"] + 1) * SECTOR_SIZE
    esp_image = build_fat16_image(
        esp_bytes,
        esp["first_lba"],
        efi_bytes,
        bootctrl_bytes,
        pe_smoke_bundle,
        curated_bundles,
        esp_extra_files,
    )
    seed = next(part for part in layout if part["name"] == "seed")
    seed_bytes = (seed["last_lba"] - seed["first_lba"] + 1) * SECTOR_SIZE
    seed_image = build_seed_fat16_image(seed_bytes, seed["first_lba"], curated_bundles)
    system_image_bytes = build_system_slot_image(args.system_image_mib * SLOT_IMAGE_MIB)
    active_system_image_bytes = build_system_slot_image(
        args.system_image_mib * SLOT_IMAGE_MIB,
        args.update_smoke_request_url,
    )
    protective_mbr, primary_gpt, backup_gpt = build_partition_table(disk_sectors, layout)

    with args.output.open("r+b") as image:
        image.seek(0)
        image.write(protective_mbr)
        image.seek(SECTOR_SIZE)
        image.write(primary_gpt)
        image.seek(esp["first_lba"] * SECTOR_SIZE)
        image.write(esp_image)
        image.seek(seed["first_lba"] * SECTOR_SIZE)
        image.write(seed_image)
        for part in layout:
            if part["name"] not in {"system_a", "system_b", "recovery"}:
                continue
            image.seek(part["first_lba"] * SECTOR_SIZE)
            if part["name"] == args.active_slot and args.update_smoke_request_url:
                image.write(active_system_image_bytes)
            else:
                image.write(system_image_bytes)
        image.seek((disk_sectors - 33) * SECTOR_SIZE)
        image.write(backup_gpt)

    manifest = {
        "disk_bytes": disk_bytes,
        "boot_control_seed": {
            "active_slot": args.active_slot,
            "pending_slot": args.pending_slot,
            "auto_login": args.auto_login,
            "suspend_resume_smoke": args.suspend_resume_smoke,
            "update_smoke_request_url": args.update_smoke_request_url,
            "pe_smoke_bundle": str(args.pe_smoke_bundle) if args.pe_smoke_bundle else None,
            "bundles": [str(path) for path in args.bundle],
            "esp_extra_files": esp_extra_manifest,
        },
        "partitions": [
            {
                "name": part["name"],
                "first_lba": part["first_lba"],
                "last_lba": part["last_lba"],
                "size_bytes": (part["last_lba"] - part["first_lba"] + 1) * SECTOR_SIZE,
            }
            for part in layout
        ],
    }
    args.output.with_suffix(".json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    args.output.with_suffix(".bootctrl.bin").write_bytes(bootctrl_bytes)
    print(f"appliance image written: {args.output}")


if __name__ == "__main__":
    main()
