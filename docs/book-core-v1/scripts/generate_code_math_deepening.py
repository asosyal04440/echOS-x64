from __future__ import annotations

import re
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
BOOK_SRC = Path(__file__).resolve().parents[1] / "src"


SECTIONS = [
    {
        "title": "Boot ve erken init: state publication",
        "path": "src/main.rs",
        "math": [
            r"L_{boot}=L_{fw}+L_{loader}+L_{early\_map}+L_{subsys}",
            r"P_{fail}=1-\prod_i (1-p_i)",
            r"S_{boot}=\frac{1}{1+\sigma_{state}}",
        ],
    },
    {
        "title": "SMP scheduler orkestrasyonu",
        "path": "src/task/scheduler.rs",
        "math": [
            r"Skew=\max_i q_i-\min_i q_i",
            r"W_i=\alpha q_i+\beta u_i+\gamma m_i",
            r"J_{p99}=\operatorname*{argmin}_{policy} \; tail(policy)",
        ],
    },
    {
        "title": "RT scheduler: bandwidth ve dilim kontrolu",
        "path": "src/task/rt_scheduler.rs",
        "math": [
            r"U_{rt}=\sum_i \frac{C_i}{T_i}",
            r"U_{rt}\le U_{cap}",
            r"Q_{rr}(p)=\operatorname{clip}(Q_{min},Q_{max},f(p))",
        ],
    },
    {
        "title": "CFS: vruntime geometrisi",
        "path": "src/task/cfs.rs",
        "math": [
            r"\Delta v = \frac{\Delta t\,W_0}{w_i}",
            r"v_i(t+1)=v_i(t)+\Delta v_i",
            r"Fairness\;Gap=\max_i v_i-\min_i v_i",
        ],
    },
    {
        "title": "EEVDF: eligibility ve virtual deadline",
        "path": "src/task/eevdf.rs",
        "math": [
            r"lag_i = service_i - fair_i",
            r"eligible_i = [lag_i\ge 0]",
            r"vd_i = vtime + \frac{slice_i}{w_i}",
        ],
    },
    {
        "title": "Deadline scheduler: admission denklemi",
        "path": "src/task/deadline.rs",
        "math": [
            r"U=\sum_i \frac{C_i}{T_i}",
            r"U\le 1-\epsilon",
            r"R_i=C_i+\sum_{j\ne i}\left\lceil\frac{R_i}{T_j}\right\rceil C_j",
        ],
    },
    {
        "title": "Chase-Lev deque: lock-free yarismalar",
        "path": "src/task/deque.rs",
        "math": [
            r"P_{race}=P(pop\cap steal\cap single\_slot)",
            r"E[T_{retry}] = \frac{1}{1-p_{cas\_fail}}",
            r"Throughput\approx\frac{ops}{CAS+fence}",
        ],
    },
    {
        "title": "Timing wheel: amortized analiz",
        "path": "src/task/timer.rs",
        "math": [
            r"T_{insert}=O(1)",
            r"T_{tick}=O(1)\;\text{(amortized)}",
            r"L_{timer}=L_{bucket}+L_{cascade}",
        ],
    },
    {
        "title": "Zone-aware PMM",
        "path": "src/memory/fibonacci_pmm.rs",
        "math": [
            r"F_{free}=F_{total}-F_{used}-F_{reserved}",
            r"p(z)=\frac{alloc_z}{\sum_k alloc_k}",
            r"Fallback\;Rate=\frac{fallbacks}{alloc\_req}",
        ],
    },
    {
        "title": "Fibonacci buddy",
        "path": "src/memory/fibonacci_buddy.rs",
        "math": [
            r"F_n=F_{n-1}+F_{n-2}",
            r"Frag_{int}=\frac{unused}{allocated}",
            r"Coalesce\;Success=\frac{merge\_ok}{free\_ops}",
        ],
    },
    {
        "title": "TLSF wrapper",
        "path": "src/allocator/tlsf.rs",
        "math": [
            r"T_{alloc}=O(1)",
            r"B_{bucket}=2^{fli}\cdot(1+\frac{sli}{N_{sli}})",
            r"P_{corrupt}\propto P(check\_skip)",
        ],
    },
    {
        "title": "Page fault, COW ve THP",
        "path": "src/memory/mod.rs",
        "math": [
            r"T_{fault}=T_{walk}+T_{policy}+T_{map}",
            r"Gain_{thp}=Hit_{tlb}^{2M}-Hit_{tlb}^{4K}",
            r"Cost_{cow}=P(write\_shared)\cdot C_{copy}",
        ],
    },
    {
        "title": "MGLRU ve zswap",
        "path": "src/memory/mglru.rs",
        "math": [
            r"\rho=\frac{\lambda_{dirty}}{\mu_{writeback}}",
            r"Refault\;Ratio=\frac{refault}{evict}",
            r"Score(page)=a\,gen+b\,hot-c\,io\_cost",
        ],
    },
    {
        "title": "zswap core",
        "path": "src/memory/zswap.rs",
        "math": [
            r"CR=\frac{size_{orig}}{size_{comp}}",
            r"Benefit=IO_{saved}-CPU_{comp}",
            r"Hit_{zswap}=\frac{swapin_{hit}}{swapin_{total}}",
        ],
    },
    {
        "title": "io_uring lock-free publication",
        "path": "src/posix/io_uring_ring.rs",
        "math": [
            r"Latency_{ring}=L_{submit}+L_{consume}",
            r"Overflow\;Rate=\frac{cq\_overflow}{cq\_events}",
            r"P_{stale}\downarrow\;\text{with Release/Acquire}",
        ],
    },
    {
        "title": "TLS 1.3 key schedule",
        "path": "src/net/tls.rs",
        "math": [
            r"secret_{k+1}=HKDF(secret_k, transcript_k)",
            r"P_{forge}\approx 2^{-tag\_bits}",
            r"State\;Drift=\|state_{peer}-state_{local}\|",
        ],
    },
    {
        "title": "QUIC parser",
        "path": "src/net/quic.rs",
        "math": [
            r"T_{parse}=\sum_i T(frame_i)",
            r"ACK\;Cost=O(n_{ranges})",
            r"Amplification\le A_{max}",
        ],
    },
    {
        "title": "WireGuard",
        "path": "src/net/wireguard.rs",
        "math": [
            r"nonce_{new}>nonce_{last}",
            r"Replay\;Risk\to 0\;\text{with monotonic window}",
            r"Key\;Rotation\;Interval=\arg\min(C_{cpu}+R_{security})",
        ],
    },
    {
        "title": "HPACK Huffman decode",
        "path": "src/net/http2_huffman.rs",
        "math": [
            r"T_{decode}=O(n_{bits})",
            r"P_{invalid}=P(padding\_bad)+P(eos\_bad)",
            r"FailClosed=1\iff valid\_tree\land valid\_end",
        ],
    },
]


def collect_code(path: Path, max_lines: int = 80) -> list[str]:
    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    out: list[str] = []
    for raw in lines:
        s = raw.strip()
        if not s:
            continue
        if s.startswith("//"):
            continue
        if re.match(r"^(pub\s+)?(fn|struct|enum|impl|const)\b", s):
            out.append(raw.rstrip())
        elif "->" in s and "{" in s and len(s) < 140:
            out.append(raw.rstrip())
        if len(out) >= max_lines:
            break
    return out


def main() -> None:
    out: list[str] = []
    out.append("# Cilt 1 Kod ve Matematik Derinlesme")
    out.append("")
    out.append(
        "Bu bolumde her cekirdek alt sistem icin iki sey birlikte verilir: "
        "(1) dogrudan kod parcasi, (2) karar/maliyet modelini aciklayan matematiksel cerceve."
    )
    out.append("")
    out.append(
        "Matematik burada formel kanit iddiasi degil, muhendislik kararinin hesaplanabilir ozetidir."
    )
    out.append("")
    out.append("---")
    out.append("")

    for idx, sec in enumerate(SECTIONS, start=1):
        rel = sec["path"]
        abs_path = REPO_ROOT / rel
        if not abs_path.exists():
            continue

        code = collect_code(abs_path, max_lines=90)

        out.append(f"## KM{idx:02d} - {sec['title']}")
        out.append("")
        out.append(f"Kaynak dosya: `{rel}`")
        out.append("")

        out.append("### Kod parcasi")
        out.append("")
        out.append("```rust")
        out.extend(code if code else ["// uygun kod parcasi bulunamadi"])
        out.append("```")
        out.append("")

        out.append("### Matematiksel model")
        out.append("")
        for m in sec["math"]:
            out.append(rf"\[{m}\]")
            out.append("")

        out.append("### Muhendislik yorumu")
        out.append("")
        out.append(
            "Bu alt sistemde kod parcasi, state gecislerinin hangi sira ve hangi kosullarla "
            "ilerledigini gosteren bir publication haritasi gibi okunur. Denklem seti ise bu haritanin "
            "tail-latency, dogruluk ve kaynak maliyeti tarafindaki etkisini nicel olarak ifade eder."
        )
        out.append("")
        out.append(
            "Modelde en kritik nokta, tek bir metriği optimize etmek yerine, p99 gecikme, hata yuzeyi "
            "ve kaynak tuketimi arasindaki gerilimi ayni anda gormektir. Bu nedenle kararlar satir-bazli "
            "kod referansi ile desteklenmeden kabul edilmez."
        )
        out.append("")
        out.append("---")
        out.append("")

    target = BOOK_SRC / "cilt1-kod-ve-matematik-derinlesme.md"
    target.write_text("\n".join(out).rstrip() + "\n", encoding="utf-8")
    print(f"[GENERATE] wrote {target}")


if __name__ == "__main__":
    main()
