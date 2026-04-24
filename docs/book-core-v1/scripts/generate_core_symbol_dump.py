from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
BOOK_SRC = Path(__file__).resolve().parents[1] / "src"


TARGET_FILES = [
    "src/main.rs",
    "src/task/scheduler.rs",
    "src/task/rt_scheduler.rs",
    "src/task/cfs.rs",
    "src/task/eevdf.rs",
    "src/task/deadline.rs",
    "src/task/deque.rs",
    "src/task/timer.rs",
    "src/memory/fibonacci_pmm.rs",
    "src/memory/fibonacci_buddy.rs",
    "src/allocator/tlsf.rs",
    "src/memory/mod.rs",
    "src/memory/mglru.rs",
    "src/memory/zswap.rs",
    "src/posix/io_uring_ring.rs",
    "src/net/tls.rs",
    "src/net/quic.rs",
    "src/net/wireguard.rs",
    "src/net/http2_huffman.rs",
]


@dataclass
class Symbol:
    kind: str
    name: str
    line: int
    is_pub: bool


def parse_symbols(path: Path) -> list[Symbol]:
    out: list[Symbol] = []
    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    for idx, raw in enumerate(lines, start=1):
        line = raw.strip()
        if not line or line.startswith("//"):
            continue

        m = re.match(r"^(pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            out.append(Symbol("struct", m.group(2), idx, bool(m.group(1))))
            continue

        m = re.match(r"^(pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            out.append(Symbol("enum", m.group(2), idx, bool(m.group(1))))
            continue

        m = re.match(r"^(pub\s+)?(?:const\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", line)
        if m:
            out.append(Symbol("fn", m.group(2), idx, bool(m.group(1))))
            continue

        m = re.match(r"^(pub\s+)?const\s+([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            out.append(Symbol("const", m.group(2), idx, bool(m.group(1))))
            continue

        m = re.match(r"^(pub\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)", line)
        if m:
            out.append(Symbol("type", m.group(2), idx, bool(m.group(1))))
            continue

    return out


def section_for_file(rel: str, symbols: list[Symbol], line_count: int) -> list[str]:
    pub_count = sum(1 for s in symbols if s.is_pub)
    fn_count = sum(1 for s in symbols if s.kind == "fn")
    struct_count = sum(1 for s in symbols if s.kind == "struct")
    enum_count = sum(1 for s in symbols if s.kind == "enum")
    const_count = sum(1 for s in symbols if s.kind == "const")

    lines: list[str] = []
    lines.append(f"## {rel}")
    lines.append("")
    lines.append(f"- Satir sayisi: {line_count}")
    lines.append(f"- Toplam sembol: {len(symbols)}")
    lines.append(f"- Public sembol: {pub_count}")
    lines.append(
        f"- Fonksiyon: {fn_count}, Struct: {struct_count}, Enum: {enum_count}, Const: {const_count}"
    )
    lines.append("")

    if symbols:
        lines.append("### Sembol envanteri")
        lines.append("")
        lines.append("| Kind | Name | Line | Visibility |")
        lines.append("|---|---|---:|---|")
        for s in symbols:
            vis = "pub" if s.is_pub else "internal"
            lines.append(f"| {s.kind} | `{s.name}` | {s.line} | {vis} |")
        lines.append("")

        lines.append("### Muhendislik notu")
        lines.append("")
        lines.append(
            "Bu dosyada API yuzeyi ve internal yardimci fonksiyonlarin dagilimi, kodun sahiplik "
            "ve publication sinirlarini yorumlamak icin ilk sinyal katmanidir. Sembol yogunlugu "
            "tek basina kalite olcutu degildir; ancak degisen release'lerde kontrat yuzeyinin hangi "
            "dosyada buyudugunu sayisal olarak izlemeyi mumkun kilar."
        )
        lines.append("")

    lines.append("---")
    lines.append("")
    return lines


def main() -> None:
    out: list[str] = []
    out.append("# Cilt 1 Core Sembol Dokumu")
    out.append("")
    out.append(
        "Bu bolum, Cilt 1 kapsamindaki cekirdek dosyalarin sembol envanterini tek bir teknik "
        "dokumde toplar. Hedef, API yuzeyi, internal yardimci fonksiyonlar ve veri-tip kontratlarini "
        "sayisal bir cizelgeyle okumayi kolaylastirmaktir."
    )
    out.append("")
    out.append("Okuma protokolu:")
    out.append("")
    out.append("1. Dosya bazli sembol yogunlugunu incele.")
    out.append("2. Public/internal dagilimini ownership modeliyle eslestir.")
    out.append("3. Buyuk kontrat yuzeyleri icin degisim riski notunu cikar.")
    out.append("")
    out.append("---")
    out.append("")

    for rel in TARGET_FILES:
        file_path = REPO_ROOT / rel
        if not file_path.exists():
            continue
        text = file_path.read_text(encoding="utf-8", errors="ignore")
        line_count = len(text.splitlines())
        symbols = parse_symbols(file_path)
        out.extend(section_for_file(rel, symbols, line_count))

    out_path = BOOK_SRC / "cilt1-core-sembol-dokumu.md"
    out_path.write_text("\n".join(out).rstrip() + "\n", encoding="utf-8")
    print(f"[GENERATE] wrote {out_path}")


if __name__ == "__main__":
    main()
