from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


def read_lines(name: str) -> list[str]:
    return (SRC / name).read_text(encoding="utf-8").splitlines()


def write_lines(name: str, lines: list[str]) -> None:
    payload = "\n".join(lines).rstrip() + "\n"
    (SRC / name).write_text(payload, encoding="utf-8")


def normalize_sentence(text: str) -> str:
    cleaned = re.sub(r"\s+", " ", text).strip()
    cleaned = cleaned.rstrip("?").strip()
    if cleaned and not cleaned.endswith("."):
        cleaned += "."
    return cleaned


def refactor_lab_to_walkthrough() -> None:
    src = read_lines("cilt1-lab-kilavuzu.md")
    out: list[str] = [
        "# Cilt 1 Kod Yuruyus Rehberi",
        "",
        "Bu bolumde, echOS cekirdek kodu uzerinden sistematik kod yuruyus adimlari verilir.",
        "Her yuruyus dogrudan kaynak dosya ve karar noktasi eslestirmesi uzerinden ilerler.",
        "",
    ]

    skip_mode = False
    i = 0
    while i < len(src):
        line = src[i]

        if line.startswith("## Degerlendirme ust tablosu"):
            break

        if line.startswith("### Hata avi") or line.startswith("### Rubrik"):
            skip_mode = True
            i += 1
            continue

        if skip_mode:
            if (
                line.startswith("### ")
                or line.startswith("## ")
                or line.startswith("---")
            ):
                skip_mode = False
            else:
                i += 1
                continue

        if line.startswith("# Cilt 1 Lab Kilavuzu"):
            i += 1
            continue

        line = line.replace("Lab ", "Kod Yuruyus ")
        line = line.replace("lab ", "yuruyus ")
        line = line.replace("Her lab", "Her yuruyus")
        line = line.replace("Lab formati", "Yuruyus formati")
        line = line.replace("laboratuvarlarin", "kod yuruyuslerinin")

        out.append(line)
        i += 1

    write_lines("cilt1-kod-yuruyus-rehberi.md", out)


def refactor_small_question_bank() -> None:
    src = read_lines("cilt1-soru-bankasi.md")
    out: list[str] = [
        "# Cilt 1 Kritik Kontrol Listesi",
        "",
        "Bu bolum, Cilt 1 kapsamindaki temel kontrol maddelerini soru formundan cikarip",
        "operasyonel inceleme listesi olarak sunar.",
        "",
        "Kontrol maddeleri uc lane'de okunur:",
        "",
        "- Kavramsal dogruluk",
        "- Kod-yol tutarliligi",
        "- Failure-path mitigasyonu",
        "",
        "---",
    ]

    for line in src:
        if line.startswith("## Kume"):
            out.append("")
            out.append(line.replace("Kume", "Kontrol Kumesi"))
            out.append("")
            continue

        m = re.match(r"^(\d+)\.\s+(.*)$", line)
        if m:
            text = normalize_sentence(m.group(2))
            out.append(f"- Kontrol maddesi: {text}")
            continue

        if line.startswith("## Cevaplama onerisi"):
            out.append("")
            out.append("## Raporlama notasyonu")
            out.append("")
            continue

        if line.startswith("1. ") or line.startswith("2. ") or line.startswith("3. "):
            text = normalize_sentence(line.split(".", 1)[1].strip())
            out.append(f"- Rapor adimi: {text}")
            continue

        if (
            line.startswith("# ")
            or line.startswith("Bu bankada")
            or line.startswith("Toplam hedef")
        ):
            continue

        out.append(line)

    write_lines("cilt1-kritik-kontrol-listesi.md", out)


def refactor_large_question_bank() -> None:
    src = read_lines("cilt1-soru-bankasi-1000.md")
    out: list[str] = [
        "# Cilt 1 Derin Inceleme Protokolleri",
        "",
        "Bu bolum, onceki genis soru kumesinin tamamini hard-core muhendislik denetim",
        "maddelerine donusturur. Her satir, belirli bir kod yolda dogrulanacak bir kontrol",
        "ifadesi olarak yazilir.",
        "",
        "Uygulama disiplini:",
        "",
        "1. Maddeyi oku ve ilgili dosyada satir referansini cikar.",
        "2. Invarianti yazili hale getir ve ihlal semptomunu not et.",
        "3. Mitigasyon satirini ayni raporda belirt.",
        "",
        "---",
    ]

    for line in src:
        if line.startswith("## Topic"):
            m = re.match(r"## Topic\s+(\d+)\s+-\s+(.*)$", line)
            if m:
                out.append("")
                out.append(f"## Protokol {m.group(1)} - {m.group(2)}")
                out.append("")
            continue

        m = re.match(r"^(\d+)\.\s+(.*)$", line)
        if m:
            text = normalize_sentence(m.group(2))
            out.append(f"- Inceleme odagi: {text}")
            continue

        if line.startswith("# ") or line.startswith("Bu bankada"):
            continue

        out.append(line)

    write_lines("cilt1-core-inceleme-protokolleri.md", out)


def refactor_case_bank() -> None:
    src = read_lines("cilt1-vaka-ve-cozumler.md")
    out: list[str] = [
        "# Cilt 1 Core Ariza Atlasi",
        "",
        "Bu atlas, onceki senaryo kumesini sinav/odev formatindan cikarip operasyonel",
        "ariza paternlerine donusturur. Her patern, semptomdan kok nedene giden teknik",
        "teshis zinciri ve duzeltme etkisiyle verilir.",
        "",
        "Okuma sirasi:",
        "",
        "- Semptomu dogru siniflandir",
        "- Kok nedene giden publication/ownership sinirini bul",
        "- Duzeltme adiminin yan etkisini raporla",
        "",
        "---",
    ]

    in_diag = False

    for raw in src:
        line = raw.rstrip()

        m_group = re.match(r"## Vaka Kumesi\s+(\d+)\s+-\s+(.*)$", line)
        if m_group:
            in_diag = False
            out.append("")
            out.append(f"## Ariza Sinifi {m_group.group(1)} - {m_group.group(2)}")
            out.append("")
            continue

        m_case = re.match(
            r"### Vaka\s+(\d+)\s+-\s+(.*?)(?:\s*/\s*senaryo\s*\d+)?$", line
        )
        if m_case:
            in_diag = False
            out.append("")
            out.append(f"### Ariza Paterni {m_case.group(1)} - {m_case.group(2)}")
            out.append("")
            continue

        if line.startswith("- Senaryo:"):
            in_diag = False
            out.append("**Ariza paterni**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("- Belirti:"):
            in_diag = False
            out.append("**Gozlenebilir semptom**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("- Muhtemel kok neden:"):
            in_diag = False
            out.append("**Kok neden**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("- Inceleme adimi"):
            if not in_diag:
                out.append("**Teshis akisi**")
                in_diag = True
            out.append(f"- {normalize_sentence(line.split(':', 1)[1])}")
            continue

        if line.startswith("- Cozum yaklasimi:"):
            in_diag = False
            out.append("")
            out.append("**Mimarik duzeltme**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("- Dogrulama:"):
            in_diag = False
            out.append("**Dogrulama protokolu**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("- Son not:"):
            in_diag = False
            out.append("**Yan etki ve trade-off**")
            out.append("")
            out.append(normalize_sentence(line.split(":", 1)[1]))
            out.append("")
            continue

        if line.startswith("# ") or line.startswith("Bu bolumde"):
            continue

        if line.startswith("---"):
            in_diag = False
            out.append("---")
            continue

        if line.strip() == "":
            if out and out[-1] != "":
                out.append("")
            continue

        out.append(line)

    write_lines("cilt1-core-ariza-atlasi.md", out)


def strip_monograph_mini_questions() -> None:
    src = read_lines("cilt1-core-monograf-expanded.md")
    out: list[str] = []
    i = 0

    while i < len(src):
        line = src[i]
        if line.startswith("### Mini sorular"):
            i += 1
            while i < len(src) and not src[i].startswith("---"):
                i += 1
            continue
        out.append(line)
        i += 1

    write_lines("cilt1-core-monograf-expanded.md", out)


def main() -> None:
    refactor_lab_to_walkthrough()
    refactor_small_question_bank()
    refactor_large_question_bank()
    refactor_case_bank()
    strip_monograph_mini_questions()
    print("[REFORMAT] hardcore content transformations completed")


if __name__ == "__main__":
    main()
