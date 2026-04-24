from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"


def load(name: str) -> list[str]:
    return (SRC / name).read_text(encoding="utf-8").splitlines()


def save(name: str, lines: list[str]) -> None:
    (SRC / name).write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def to_statement(text: str) -> str:
    s = text.strip()
    s = s.rstrip(".").strip()

    replacements = [
        (
            "fonksiyonunun ownership sinirini acikla",
            "fonksiyonunun ownership siniri acik ve dogrulanabilir olarak belgelenmelidir",
        ),
        (
            "icin en kotu gecikme patikasi nasil olusur",
            "icin en kotu gecikme patikasi modellenmeli ve telemetriyle dogrulanmalidir",
        ),
        (
            "alt sisteminde fail-closed davranis hangi satirda baslar",
            "alt sisteminde fail-closed davranisin baslangic satiri referansla kaydedilmelidir",
        ),
        (
            "dosyasinda guard kaldirilirsa ilk hangi test kirilmali",
            "dosyasinda guard kaldirma etkisi ilk kirilan test corpusu ile kanitlanmalidir",
        ),
        (
            "icin bir invariant yaz ve ihlal semptomunu belirt",
            "icin temel invariant ve ihlal semptomu beraber raporlanmalidir",
        ),
        (
            "modelinde telemetry olmadan hangi karar yanlis kalir",
            "modelinde telemetry yoklugunda yanlis karar yuzeyi acikca belirtilmelidir",
        ),
        (
            "icin p99 odakli tuning planini 3 adimda yaz",
            "icin p99 odakli tuning plani uc adimli ve olculebilir olmalidir",
        ),
        (
            "alt sisteminde publication boundary neden kritiktir",
            "alt sisteminde publication boundary gerekcesi memory-order sinirlariyla birlikte belgelenmelidir",
        ),
        (
            "ile bagli bir admission limiti oner ve gerekcesini yaz",
            "ile bagli admission limiti nicel gerekceyle tanimlanmalidir",
        ),
        (
            "kodunda hata donuslerinin fail-open olmasi niye riskli",
            "kodunda hata donuslerinin fail-open olma riski tehdit modeliyle birlikte kaydedilmelidir",
        ),
    ]

    for old, new in replacements:
        if old in s:
            s = s.replace(old, new)

    if s and not s.endswith("."):
        s += "."
    return s


def harden_monograf() -> None:
    src = load("cilt1-core-monograf-expanded.md")
    out: list[str] = []

    in_invariant = False
    in_failure = False

    for line in src:
        if line.startswith("### Kod okuma gorevleri"):
            out.append("### Invariant denetim cercevesi")
            in_invariant = True
            in_failure = False
            continue

        if line.startswith("### Vaka analizi"):
            out.append("### Failure envelope analizi")
            in_invariant = False
            in_failure = True
            continue

        if line.startswith("### Hedeflenen ogrenme ciktilari"):
            out.append("### Cekirdek cikarsama seti")
            in_invariant = False
            in_failure = False
            continue

        if line.startswith("### "):
            in_invariant = False
            in_failure = False

        if in_invariant:
            m = re.match(r"^\d+\.\s+(.*)$", line)
            if m:
                out.append(f"- Denetim kurali: {m.group(1)}")
                continue

        if in_failure:
            m = re.match(r"^Vaka\s+([A-Z]):\s+(.*)$", line)
            if m:
                out.append(f"- Failure paterni {m.group(1)}: {m.group(2)}")
                continue

        out.append(line)

    save("cilt1-core-monograf-expanded.md", out)


def build_denetim_korpusu() -> None:
    src = load("cilt1-core-inceleme-protokolleri.md")
    out: list[str] = []

    for line in src:
        if line.startswith("# Cilt 1 Derin Inceleme Protokolleri"):
            out.append("# Cilt 1 Core Denetim Korpusu")
            continue

        if line.strip() == "Uygulama disiplini:":
            out.append("Denetim protokolu:")
            continue

        line = line.replace("Inceleme odagi", "Denetim maddesi")
        line = line.replace("Karma final sorular", "Karma sistem denetimi")
        line = line.replace("Karma tasarim sorulari", "Karma tasarim denetimi")

        m = re.match(r"^-\s+Denetim maddesi:\s+(.*)$", line)
        if m:
            line = f"- Denetim maddesi: {to_statement(m.group(1))}"

        out.append(line)

    save("cilt1-core-denetim-korpusu.md", out)


def main() -> None:
    harden_monograf()
    build_denetim_korpusu()
    print("[HARDEN] hardcore source hardening completed")


if __name__ == "__main__":
    main()
