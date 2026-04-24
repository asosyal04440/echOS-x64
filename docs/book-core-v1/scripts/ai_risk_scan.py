from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from pypdf import PdfReader
from transformers import pipeline, logging as hf_logging


hf_logging.set_verbosity_error()


@dataclass
class PageRisk:
    page: int
    human_score: float
    fake_score: float
    risk_percent: float
    chars: int


def normalize_text(text: str) -> str:
    text = re.sub(r"\s+", " ", text).strip()
    if len(text) > 1800:
        return text[:1800]
    return text


def load_pages(pdf_path: Path) -> list[str]:
    reader = PdfReader(str(pdf_path))
    pages: list[str] = []
    for p in reader.pages:
        raw = p.extract_text() or ""
        pages.append(normalize_text(raw))
    return pages


def label_map(score_obj: dict) -> tuple[float, float]:
    label = score_obj["label"].lower()
    score = float(score_obj["score"])
    if label in {"human", "real"}:
        return score, 1.0 - score
    return 1.0 - score, score


def scan_pdf(pdf_path: Path) -> tuple[list[PageRisk], dict]:
    detector = pipeline(
        "text-classification", model="Hello-SimpleAI/chatgpt-detector-roberta"
    )
    pages = load_pages(pdf_path)
    risks: list[PageRisk] = []

    for idx, text in enumerate(pages, start=1):
        if len(text) < 80:
            continue
        out = detector(text, truncation=True, max_length=512)[0]
        human_score, fake_score = label_map(out)
        risk_percent = fake_score * 100.0
        risks.append(
            PageRisk(
                page=idx,
                human_score=human_score,
                fake_score=fake_score,
                risk_percent=risk_percent,
                chars=len(text),
            )
        )

    max_risk = max((r.risk_percent for r in risks), default=0.0)
    avg_risk = sum((r.risk_percent for r in risks), 0.0) / max(len(risks), 1)
    over20 = [r for r in risks if r.risk_percent > 20.0]

    summary = {
        "pdf": str(pdf_path),
        "model": "Hello-SimpleAI/chatgpt-detector-roberta",
        "scanned_pages": len(risks),
        "max_risk_percent": round(max_risk, 2),
        "avg_risk_percent": round(avg_risk, 2),
        "pages_over_20": len(over20),
        "status": "pass" if len(over20) == 0 else "fail",
    }
    return risks, summary


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: python ai_risk_scan.py <pdf-path>")
        return 2

    pdf_path = Path(sys.argv[1]).resolve()
    if not pdf_path.exists():
        print(f"PDF not found: {pdf_path}")
        return 2

    risks, summary = scan_pdf(pdf_path)
    out_dir = pdf_path.parent
    report_json = out_dir / "ai-risk-report.json"
    report_txt = out_dir / "ai-risk-report.txt"

    payload = {
        "summary": summary,
        "pages": [
            {
                "page": r.page,
                "risk_percent": round(r.risk_percent, 2),
                "human_score": round(r.human_score, 4),
                "fake_score": round(r.fake_score, 4),
                "chars": r.chars,
            }
            for r in risks
        ],
    }
    report_json.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    lines = [
        f"PDF: {summary['pdf']}",
        f"Model: {summary['model']}",
        f"Scanned pages: {summary['scanned_pages']}",
        f"Max risk %: {summary['max_risk_percent']}",
        f"Avg risk %: {summary['avg_risk_percent']}",
        f"Pages over 20%: {summary['pages_over_20']}",
        f"Status: {summary['status']}",
        "",
        "Top risky pages:",
    ]
    top = sorted(risks, key=lambda x: x.risk_percent, reverse=True)[:20]
    for r in top:
        lines.append(
            f"- page {r.page}: risk={r.risk_percent:.2f}% human={r.human_score:.4f} fake={r.fake_score:.4f} chars={r.chars}"
        )
    report_txt.write_text("\n".join(lines), encoding="utf-8")

    print("AI risk scan summary:")
    print(json.dumps(summary, indent=2))
    return 0 if summary["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
