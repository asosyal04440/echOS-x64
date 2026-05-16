#!/usr/bin/env python3
import argparse
import datetime as dt
import json
import math
import struct
import zlib
from dataclasses import dataclass
from pathlib import Path

from build_vm_appliance import MiB, SECTOR_SIZE, build_boot_control, build_esp_image

ISO_BLOCK = 2048
BOOT_CATALOG_BLOCKS = 1
PATH_TABLE_BLOCKS = 2
README_NAME = "README.TXT;1"
ESP_NAME = "ECHOS_ESP.IMG;1"


@dataclass
class IsoPayload:
    iso_name: bytes
    size: int
    lba: int = 0
    data: bytes | None = None
    source: Path | None = None


def both_endian_16(value: int) -> bytes:
    return struct.pack("<H", value) + struct.pack(">H", value)


def both_endian_32(value: int) -> bytes:
    return struct.pack("<I", value) + struct.pack(">I", value)


def pad_ascii(text: str, length: int) -> bytes:
    raw = text.encode("ascii", errors="replace")[:length]
    return raw.ljust(length, b" ")


def iso_datetime(now: dt.datetime) -> bytes:
    return (
        f"{now.year:04d}{now.month:02d}{now.day:02d}"
        f"{now.hour:02d}{now.minute:02d}{now.second:02d}00"
    ).encode("ascii") + b"\x00"


def dir_datetime(now: dt.datetime) -> bytes:
    return bytes(
        [
            max(0, now.year - 1900),
            now.month,
            now.day,
            now.hour,
            now.minute,
            now.second,
            0,
        ]
    )


def dir_record(name: bytes, extent_lba: int, size: int, flags: int, now: dt.datetime) -> bytes:
    name_len = len(name)
    record_len = 33 + name_len
    if record_len & 1:
        record_len += 1
    record = bytearray(record_len)
    record[0] = record_len
    record[1] = 0
    record[2:10] = both_endian_32(extent_lba)
    record[10:18] = both_endian_32(size)
    record[18:25] = dir_datetime(now)
    record[25] = flags
    record[26] = 0
    record[27] = 0
    record[28:32] = both_endian_16(1)
    record[32] = name_len
    record[33 : 33 + name_len] = name
    return bytes(record)


def put_descriptor_header(block: bytearray, descriptor_type: int) -> None:
    block[0] = descriptor_type
    block[1:6] = b"CD001"
    block[6] = 1


def build_boot_record(catalog_lba: int) -> bytes:
    block = bytearray(ISO_BLOCK)
    put_descriptor_header(block, 0)
    block[7:39] = b"EL TORITO SPECIFICATION".ljust(32, b"\x00")
    struct.pack_into("<I", block, 71, catalog_lba)
    return bytes(block)


def build_primary_volume_descriptor(
    volume_id: str,
    total_blocks: int,
    root_lba: int,
    root_size: int,
    path_table_lba_le: int,
    path_table_lba_be: int,
    now: dt.datetime,
) -> bytes:
    block = bytearray(ISO_BLOCK)
    put_descriptor_header(block, 1)
    block[8:40] = pad_ascii("ECHOS", 32)
    block[40:72] = pad_ascii(volume_id, 32)
    block[80:88] = both_endian_32(total_blocks)
    block[120:124] = both_endian_16(1)
    block[124:128] = both_endian_16(1)
    block[128:132] = both_endian_16(ISO_BLOCK)
    block[132:140] = both_endian_32(10)
    struct.pack_into("<I", block, 140, path_table_lba_le)
    struct.pack_into("<I", block, 144, 0)
    struct.pack_into(">I", block, 148, path_table_lba_be)
    struct.pack_into(">I", block, 152, 0)
    root = dir_record(b"\x00", root_lba, root_size, 0x02, now)
    block[156 : 156 + len(root)] = root
    block[190:318] = pad_ascii("ECHOS", 128)
    block[318:446] = pad_ascii("ECHOS VM MEDIA", 128)
    block[446:574] = pad_ascii("ECHOS", 128)
    block[574:702] = pad_ascii("ECHOS", 128)
    block[813:850] = pad_ascii("README.TXT;1", 37)
    date = iso_datetime(now)
    for offset in (813 + 37, 813 + 54, 813 + 71, 813 + 88):
        block[offset : offset + 17] = date
    block[881] = 1
    return bytes(block)


def build_terminator() -> bytes:
    block = bytearray(ISO_BLOCK)
    put_descriptor_header(block, 255)
    return bytes(block)


def build_path_tables(root_lba: int) -> tuple[bytes, bytes]:
    le = bytearray(ISO_BLOCK)
    le[0] = 1
    le[1] = 0
    struct.pack_into("<I", le, 2, root_lba)
    struct.pack_into("<H", le, 6, 1)
    le[8] = 0

    be = bytearray(ISO_BLOCK)
    be[0] = 1
    be[1] = 0
    struct.pack_into(">I", be, 2, root_lba)
    struct.pack_into(">H", be, 6, 1)
    be[8] = 0
    return bytes(le), bytes(be)


def build_root_dir(
    root_lba: int,
    root_size: int,
    payloads: list[IsoPayload],
    now: dt.datetime,
) -> bytes:
    block = bytearray(root_size)
    records = [
        dir_record(b"\x00", root_lba, root_size, 0x02, now),
        dir_record(b"\x01", root_lba, root_size, 0x02, now),
    ]
    records.extend(dir_record(payload.iso_name, payload.lba, payload.size, 0x00, now) for payload in payloads)
    cursor = 0
    for record in records:
        if cursor + len(record) > len(block):
            raise RuntimeError("root directory exceeds allocated ISO blocks")
        block[cursor : cursor + len(record)] = record
        cursor += len(record)
    return bytes(block)


def build_boot_catalog(esp_lba: int, esp_size: int) -> bytes:
    catalog = bytearray(ISO_BLOCK)
    validation = bytearray(32)
    validation[0] = 0x01
    validation[1] = 0xEF
    validation[4:28] = b"echOS UEFI".ljust(24, b"\x00")
    validation[30] = 0x55
    validation[31] = 0xAA
    checksum = sum(struct.unpack_from("<H", validation, offset)[0] for offset in range(0, 32, 2))
    struct.pack_into("<H", validation, 28, (-checksum) & 0xFFFF)
    catalog[0:32] = validation

    entry = bytearray(32)
    entry[0] = 0x88
    entry[1] = 0x00
    struct.pack_into("<H", entry, 2, 0)
    entry[4] = 0
    sector_count = min(0xFFFF, math.ceil(esp_size / SECTOR_SIZE))
    struct.pack_into("<H", entry, 6, sector_count)
    struct.pack_into("<I", entry, 8, esp_lba)
    catalog[32:64] = entry
    return bytes(catalog)


def align_blocks(size: int) -> int:
    return math.ceil(size / ISO_BLOCK)


def normalize_iso_name(name: str) -> bytes:
    iso_name = name.strip().upper()
    if not iso_name:
        raise ValueError("empty ISO file name")
    if not iso_name.endswith(";1"):
        iso_name = f"{iso_name};1"
    allowed = set("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.;")
    bad = sorted({ch for ch in iso_name if ch not in allowed})
    if bad:
        raise ValueError(f"ISO file name {name!r} has unsupported character(s): {''.join(bad)!r}")
    encoded = iso_name.encode("ascii")
    if len(encoded) > 31:
        raise ValueError(f"ISO file name {name!r} exceeds 31 bytes after version suffix")
    return encoded


def parse_include_file(spec: str) -> tuple[Path, bytes]:
    if "::" in spec:
        source_text, iso_name_text = spec.split("::", 1)
    else:
        source_text = spec
        iso_name_text = Path(spec).name
    source = Path(source_text)
    return source, normalize_iso_name(iso_name_text)


def root_directory_bytes(root_lba: int, root_size: int, payloads: list[IsoPayload], now: dt.datetime) -> int:
    records = [
        dir_record(b"\x00", root_lba, root_size, 0x02, now),
        dir_record(b"\x01", root_lba, root_size, 0x02, now),
    ]
    records.extend(dir_record(payload.iso_name, payload.lba, payload.size, 0x00, now) for payload in payloads)
    return sum(len(record) for record in records)


def write_block_aligned_data(handle, data: bytes) -> None:
    handle.write(data)
    padding = align_blocks(len(data)) * ISO_BLOCK - len(data)
    if padding:
        handle.write(b"\x00" * padding)


def write_block_aligned_file(handle, source: Path) -> None:
    size = source.stat().st_size
    written = 0
    with source.open("rb") as source_handle:
        while True:
            chunk = source_handle.read(4 * MiB)
            if not chunk:
                break
            handle.write(chunk)
            written += len(chunk)
    if written != size:
        raise RuntimeError(f"short read while writing {source}")
    padding = align_blocks(size) * ISO_BLOCK - size
    if padding:
        handle.write(b"\x00" * padding)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build UEFI El Torito ISO image for echOS VMs")
    parser.add_argument("--efi", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--esp-mib", type=int, default=16)
    parser.add_argument("--volume-id", default="ECHOS_UEFI")
    parser.add_argument("--no-auto-login", action="store_true")
    parser.add_argument("--esp-fat", choices=("fat16", "fat32"), default="fat16")
    parser.add_argument(
        "--include-file",
        action="append",
        default=[],
        metavar="HOST_PATH[::ISO_NAME]",
        help="Add a block-aligned payload file to the ISO root directory",
    )
    args = parser.parse_args()

    efi_bytes = args.efi.read_bytes()
    included_files = []
    seen_iso_names = {normalize_iso_name(README_NAME), normalize_iso_name(ESP_NAME)}
    for include_spec in args.include_file:
        source, iso_name = parse_include_file(include_spec)
        if iso_name in seen_iso_names:
            raise ValueError(f"duplicate ISO file name: {iso_name.decode('ascii')}")
        if not source.is_file():
            raise FileNotFoundError(source)
        seen_iso_names.add(iso_name)
        included_files.append(IsoPayload(iso_name=iso_name, size=source.stat().st_size, source=source))

    bootctrl = build_boot_control(
        active_slot="system_a",
        pending_slot="none",
        auto_login=not args.no_auto_login,
        suspend_resume_smoke=False,
    )
    minimum_esp_mib = 64 if args.esp_fat == "fat32" else 16
    esp_bytes = max(args.esp_mib * MiB, minimum_esp_mib * MiB)
    required = len(efi_bytes) + len(bootctrl) + 4 * MiB
    if esp_bytes < required:
        esp_bytes = math.ceil(required / MiB) * MiB
    esp_image = build_esp_image(
        filesystem=args.esp_fat,
        total_bytes=esp_bytes,
        hidden_sectors=0,
        efi_bytes=efi_bytes,
        bootctrl_bytes=bootctrl,
        pe_smoke_bundle=None,
        curated_bundles=[],
        esp_extra_files=[],
    )
    payload_lines = []
    for payload in included_files:
        payload_lines.append(f"- {payload.iso_name.decode('ascii')}: {payload.size} bytes")
    if not payload_lines:
        payload_lines.append("- none")
    readme = (
        "echOS UEFI VM DVD\r\n"
        "Boot path: EFI/BOOT/BOOTX64.EFI inside the embedded El Torito ESP image.\r\n"
        "Use UEFI firmware/OVMF, x86_64 guest type, and 1 vCPU for this VM demo media.\r\n"
        "Legacy BIOS boot is outside this artifact contract.\r\n"
        "\r\n"
        "Included root payload files:\r\n"
        + "\r\n".join(payload_lines)
        + "\r\n"
    ).encode("ascii")

    catalog_lba = 19
    root_lba = 20
    now = dt.datetime.now(dt.UTC).replace(tzinfo=None)
    readme_payload = IsoPayload(
        iso_name=normalize_iso_name(README_NAME),
        size=len(readme),
        data=readme,
    )
    esp_payload = IsoPayload(
        iso_name=normalize_iso_name(ESP_NAME),
        size=len(esp_image),
        data=esp_image,
    )
    payloads = [readme_payload, esp_payload, *included_files]
    root_size = ISO_BLOCK
    while True:
        path_le_lba = root_lba + align_blocks(root_size)
        path_be_lba = path_le_lba + 1
        data_lba = path_be_lba + 1
        cursor_lba = data_lba
        for payload in payloads:
            payload.lba = cursor_lba
            cursor_lba += align_blocks(payload.size)
        required_root_bytes = root_directory_bytes(root_lba, root_size, payloads, now)
        required_root_size = align_blocks(required_root_bytes) * ISO_BLOCK
        if required_root_size == root_size:
            break
        root_size = required_root_size
    total_blocks = cursor_lba

    metadata_blocks = {
        16: build_primary_volume_descriptor(
        args.volume_id,
        total_blocks,
        root_lba,
        root_size,
        path_le_lba,
        path_be_lba,
        now,
        ),
        17: build_boot_record(catalog_lba),
        18: build_terminator(),
        catalog_lba: build_boot_catalog(esp_payload.lba, len(esp_image)),
    }
    root_dir = build_root_dir(root_lba, root_size, payloads, now)
    for offset in range(0, root_size, ISO_BLOCK):
        metadata_blocks[root_lba + offset // ISO_BLOCK] = root_dir[offset : offset + ISO_BLOCK]
    path_le, path_be = build_path_tables(root_lba)
    metadata_blocks[path_le_lba] = path_le
    metadata_blocks[path_be_lba] = path_be

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as output:
        for lba in range(data_lba):
            output.write(metadata_blocks.get(lba, b"\x00" * ISO_BLOCK))
        for payload in payloads:
            current_lba = output.tell() // ISO_BLOCK
            if current_lba != payload.lba:
                raise RuntimeError(
                    f"payload LBA mismatch for {payload.iso_name.decode('ascii')}: "
                    f"expected {payload.lba}, got {current_lba}"
                )
            if payload.data is not None:
                write_block_aligned_data(output, payload.data)
            elif payload.source is not None:
                write_block_aligned_file(output, payload.source)
            else:
                raise RuntimeError(f"payload has no data source: {payload.iso_name.decode('ascii')}")
        if output.tell() != total_blocks * ISO_BLOCK:
            raise RuntimeError("ISO size accounting mismatch")
    manifest = {
        "iso_bytes": total_blocks * ISO_BLOCK,
        "efi": str(args.efi),
        "volume_id": args.volume_id,
        "boot_catalog_lba": catalog_lba,
        "esp_lba": esp_payload.lba,
        "esp_bytes": len(esp_image),
        "esp_fat": args.esp_fat,
        "esp_crc32": f"{zlib.crc32(esp_image) & 0xFFFFFFFF:08x}",
        "boot_path": "EFI/BOOT/BOOTX64.EFI",
        "firmware": "UEFI/OVMF",
        "payload_files": [
            {
                "iso_name": payload.iso_name.decode("ascii"),
                "source": str(payload.source) if payload.source is not None else None,
                "lba": payload.lba,
                "bytes": payload.size,
            }
            for payload in payloads
        ],
    }
    args.output.with_suffix(".json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"UEFI ISO written: {args.output}")


if __name__ == "__main__":
    main()
