from __future__ import annotations

import re
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
BOOK_SRC = Path(__file__).resolve().parents[1] / "src"


TARGETS = [
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


def pick_symbol_centers(lines: list[str]) -> list[int]:
    centers: list[int] = []
    for i, raw in enumerate(lines, start=1):
        s = raw.strip()
        if re.match(r"^(pub\s+)?fn\s+", s):
            centers.append(i)
        elif re.match(r"^(pub\s+)?struct\s+", s):
            centers.append(i)
        elif re.match(r"^(pub\s+)?enum\s+", s):
            centers.append(i)
    if not centers:
        for i in range(40, min(len(lines), 800), 80):
            centers.append(i)
    return centers[:20]


def make_snippet(lines: list[str], center: int, radius: int = 10) -> list[str]:
    lo = max(1, center - radius)
    hi = min(len(lines), center + radius)
    return [f"{i:04d}: {lines[i - 1]}" for i in range(lo, hi + 1)]


def eqs(idx: int) -> list[str]:
    packs = [
        [
            r"L_{tail}=\operatorname{p99}(L)",
            r"R=\frac{ok}{ok+err}",
            r"G=\Delta perf-\lambda\Delta risk",
        ],
        [
            r"U=\sum_i\frac{C_i}{T_i}",
            r"J=\max_i q_i-\min_i q_i",
            r"S=\frac{throughput}{cpu}",
        ],
        [r"P_{fail}=1-\prod_i(1-p_i)", r"M=\frac{used}{cap}", r"C_{tot}=\sum_i C_i"],
    ]
    return packs[idx % len(packs)]


def section_for_file(rel: str) -> list[str]:
    p = REPO_ROOT / rel
    if not p.exists():
        return []
    lines = p.read_text(encoding="utf-8", errors="ignore").splitlines()
    centers = pick_symbol_centers(lines)

    out: list[str] = []
    out.append(f"## {rel}")
    out.append("")
    out.append(f"- Satir sayisi: {len(lines)}")
    out.append(f"- Derin kesit sayisi: {len(centers)}")
    out.append("")

    for k, c in enumerate(centers, start=1):
        out.append(f"### Kesit {k:02d} (line {c})")
        out.append("")
        out.append("```rust")
        out.extend(make_snippet(lines, c, radius=10))
        out.append("```")
        out.append("")
        out.append("Matematiksel cerceve:")
        out.append("")
        for e in eqs(k):
            out.append(rf"\[{e}\]")
            out.append("")
        out.append("Kod-matematik baglami:")
        out.append("")
        out.append(
            "Kesitteki satirlar publication siniri, veri sahipligi ve hata geri donus yollariyla"
            " birlikte okunur. Denklem paketi, ayni kesitin tail-latency ve kaynak riski tarafini"
            " nicel dille ifade eder."
        )
        out.append("")

    out.append("---")
    out.append("")
    return out


def main() -> None:
    out: list[str] = []
    out.append("# Cilt 1 Kod-Matematik Atlas V2")
    out.append("")
    out.append(
        "Bu atlas, cekirdek dosyalardan secilen kod kesitlerini matematiksel model setleriyle birlikte verir."
    )
    out.append("")
    out.append("---")
    out.append("")

    for rel in TARGETS:
        out.extend(section_for_file(rel))

    target = BOOK_SRC / "cilt1-kod-matematik-atlas-v2.md"
    target.write_text("\n".join(out).rstrip() + "\n", encoding="utf-8")
    print(f"[GENERATE] wrote {target}")


if __name__ == "__main__":
    main()
